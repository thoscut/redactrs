//! Leak-Detektor: sucht eine Zeichenkette in **allem**, was in einer PDF-Datei
//! steht — nicht nur im Seiteninhalt.
//!
//! ## Warum dieses Modul existiert
//!
//! Die End-to-End-Tests haben bislang zwei blinde Orakel benutzt:
//!
//! * `doc.get_page_content(page)` — sieht nur den Content-Stream der Seite.
//!   Form-XObjects, Annotation-Appearances, Metadaten, Struct-Tree-Strings,
//!   verwaiste Objekte und Objekt-Streams kommen darin schlicht nicht vor.
//! * den eigenen Extraktor — ein Zirkelschluss: wovor der Extraktor blind ist,
//!   das wird nicht geschwärzt und ist damit auch für den Test unsichtbar.
//!
//! [`leaks`] ist das ehrliche Messgerät: es kennt keine „Seitenlogik“, sondern
//! durchsucht die Datei auf allen Ebenen, auf denen ein Geheimnis überleben
//! kann.
//!
//! ## Was durchsucht wird
//!
//! 1. die **rohen Dateibytes** (unkomprimierte Reste, Historie inkrementeller
//!    Updates, verwaiste Objekte),
//! 2. **jeder roh gefundene `stream … endstream`-Block**, zusätzlich
//!    Flate-dekomprimiert — damit auch komprimierte Altrevisionen sichtbar
//!    werden, die im Objektgraph der neuesten Revision gar nicht auftauchen,
//! 3. **jedes Stream-Objekt** des Objektgraphen, dekodiert (Flate, LZW,
//!    ASCII85 über `lopdf`; ASCIIHex und RunLength zusätzlich hier; bei einem
//!    nicht unterstützten Filter bleiben die Rohbytes die Rückfallebene),
//! 4. **Objekte in Objekt-Streams** (`/ObjStm`) — komprimierte Container, die
//!    eine reine Rohbyte-Suche nicht sehen kann,
//! 5. **alle Zeichenketten-Objekte** im gesamten Objektgraph, egal unter
//!    welchem Schlüssel (`/Contents`, `/V`, `/ActualText`, `/Alt`, `/TU`,
//!    `/T`, `/Info`-Werte, `/Names` …), inklusive Trailer,
//! 6. innerhalb von Streams zusätzlich die **Verkettung aller
//!    Zeichenketten-Literale** — damit wird Text auch dann gefunden, wenn er
//!    per `TJ` in Bruchstücke zerlegt ist,
//! 7. **jede Seite, wie der eigene Schriftdekoder sie liest**: Glyphencodes
//!    über `/ToUnicode`, `/Differences` und Standardkodierungen in Zeichen
//!    übersetzt, zu Zeilen gesetzt ([`scan_decoded_text`]).
//!
//! Beide PDF-String-Kodierungen werden berücksichtigt: PDFDocEncoding/Latin-1
//! **und** UTF-16BE (mit und ohne BOM). Ebenso beide Syntaxen: literal
//! `(DE89…)` und hexadezimal `<44453839…>`.
//!
//! ## Warum Sichtweise 7 kein Zirkelschluss ist
//!
//! Die Sichten 1–6 vergleichen **Bytes**. Bei einer eingebetteten
//! Teilmengen-Schrift stehen im Strom aber Glyphnummern oder umgelenkte Codes
//! (`<01020304>Tj`), keine Zeichen — und das ist die Datei aus Word,
//! LibreOffice und Chrome, also der Regelfall. Gemessen an einem
//! LibreOffice-Writer-24.2-Export (TrueType-Teilmenge `BAAAAA+LiberationSerif`,
//! Codes ab `01`) und an einem PyMuPDF-Export (Type0/Identity-H, DejaVuSans)
//! meldete `--check-leaks` an der **ungeschwärzten** Datei „keiner der 4
//! Suchbegriffe steht noch in der Datei“, Rückgabewert 0.
//!
//! Sicht 7 wäre **allein** genau der Zirkelschluss von oben. Sie steht
//! deshalb **neben** den Bytesichten, nicht an ihrer Stelle: die Bytesichten
//! finden, was der Dekoder nicht liest (Metadaten, verwaiste Objekte, Text in
//! einem Strom, den kein `Do` erreicht, Rohbytes einer alten Revision); der
//! Dekoder findet, was die Bytesichten nicht lesen (Glyphencodes).
//!
//! ## Benannte blinde Flecken
//!
//! Was **keine** der sieben Sichten sieht — und was deshalb auch ein sauberer
//! Lauf nicht ausschließt:
//!
//! * **Ein lügendes `/ToUnicode`.** Die Zuordnung ist eine Behauptung der
//!   Datei; der Dekoder glaubt ihr. Bildet sie jeden Code auf „x“ ab, liest
//!   Sicht 7 „xxxx“, die Bytesichten sehen Glyphnummern, und der Text auf
//!   dem Papier bleibt unsichtbar (Kanarienvogel in
//!   `tests/zb_orakel_schriftdekoder.rs`).
//! * **Eine Schrift ohne brauchbare Zuordnung** — kein `/ToUnicode`, keine
//!   `cmap` im Fontprogramm. Der Interpreter warnt beim Schwärzen darüber;
//!   `leaks` gibt nur Fundstellen zurück und kann die Warnung nicht
//!   weiterreichen.
//! * **Text in einem Rasterbild** und **Glyphen als Pfade** (Umrisse statt
//!   Schrift): dort gibt es keine Codes, die man übersetzen könnte.
//!
//! ## Fehlerrichtung
//!
//! Im Zweifel meldet der Detektor zu viel. Ein Fehlalarm lässt einen Test laut
//! fehlschlagen und wird untersucht; ein übersehenes Leck lässt ihn still grün
//! bleiben und wird ausgeliefert. Deshalb wird derselbe Fund gerne mehrfach
//! gemeldet — einmal je Sichtweise (roh, dekodiert, als Objektfeld).

use std::collections::BTreeSet;
use std::io::Read;

use lopdf::{Dictionary, Document, Object, ObjectStream, Stream, StringFormat};
use memchr::memmem;

use crate::extract::PdfExtractor;

/// Obergrenze für gemeldete Fundstellen — eine Fehlermeldung mit 5000 Zeilen
/// hilft niemandem.
const MAX_HITS: usize = 200;
/// Anzahl Bytes Kontext links und rechts der Fundstelle.
const CONTEXT: usize = 24;
/// Maximale Verschachtelungstiefe beim Ablaufen des Objektgraphen.
const MAX_DEPTH: usize = 32;

/// Sucht `needle` in ALLEM, was in der Datei steht — nicht nur im Seiteninhalt.
///
/// Rückgabe: eine Liste von Fundstellen mit Kontext. Leer heißt: die
/// Zeichenkette kommt in der Datei auf keiner der oben genannten Ebenen vor.
/// Jeder Eintrag nennt, *wo* der Fund liegt (Objekt-Id plus Feld bzw. Stream),
/// damit ein fehlschlagender Test etwas Brauchbares sagt.
pub fn leaks(pdf_bytes: &[u8], needle: &str) -> Vec<String> {
    leaks_many(pdf_bytes, std::slice::from_ref(&needle))
        .pop()
        .unwrap_or_default()
}

/// Sucht mehrere Zeichenketten in **einem** Durchgang durch die Datei.
///
/// Rückgabe: je Suchbegriff eine Liste von Fundstellen, in der Reihenfolge der
/// Eingabe. `leaks_many(b, &[x])[0] == leaks(b, x)` — dieselbe Messung, nur
/// ohne die Arbeit mehrfach zu tun.
///
/// ## Warum es diese Form gibt
///
/// [`leaks`] entpackt jeden Stream, parst den Objektgraphen und dekodiert
/// jede Zeichenkette. Diese Arbeit hängt allein an der Datei, nicht am
/// Suchbegriff. Wer `--check-leaks` mit zehn Begriffen aufruft, hat sie
/// vorher zehnmal bezahlt: gemessen an einer 792-kB-Datei 0,13 s für einen
/// Begriff und 0,88 s für zehn. Hier fällt sie einmal an; nur der Vergleich
/// selbst — der eigentliche Zweck — bleibt je Begriff.
pub fn leaks_many(pdf_bytes: &[u8], needles: &[&str]) -> Vec<Vec<String>> {
    let mut probe = Probe::new(needles);
    if probe.needles.is_empty() {
        return probe.into_hits();
    }

    scan_raw_file(pdf_bytes, &mut probe);
    scan_raw_streams(pdf_bytes, &mut probe);

    // Lässt sich die Datei nicht parsen, bleiben die Rohsuchen die Messung.
    if let Ok(doc) = Document::load_mem(pdf_bytes) {
        scan_object_graph(&doc, &mut probe);
        scan_decoded_text(&doc, &mut probe);
    }

    probe.into_hits()
}

// ---------------------------------------------------------------------------
// Suchmuster
// ---------------------------------------------------------------------------

/// Die gesuchte Zeichenkette in allen Kodierungen, in denen sie in einer
/// PDF-Datei stehen kann.
struct Needle {
    text: String,
    /// Ohne jeden Leerraum — fängt Text ab, dessen Zwischenräume im PDF nicht
    /// als Leerzeichen, sondern als Positionierung stehen.
    ///
    /// `None`, wenn die Suchzeichenkette gar keinen Leerraum enthält: dann
    /// bringt der Vergleich nichts und würde nur Fehlalarme über
    /// Fragmentgrenzen hinweg erzeugen („MODE“ + „89 EUR“ → „DE89“).
    squeezed: Option<String>,
    /// Bytefolgen: (Beschreibung, Muster) — das Muster als vorbereiteter
    /// Sucher, weil derselbe Begriff Tausende Datenblöcke durchsucht: jeder
    /// Zeichenketten-Wert, jeder Strom, roh und dekodiert.
    variants: Vec<(&'static str, memmem::Finder<'static>)>,
}

impl Needle {
    fn new(text: &str) -> Self {
        let mut variants: Vec<(&'static str, Vec<u8>)> = Vec::new();

        let utf8 = text.as_bytes().to_vec();
        variants.push(("UTF-8/ASCII", utf8.clone()));

        // Latin-1 / PDFDocEncoding: ein Byte je Zeichen.
        if text.chars().all(|c| (c as u32) < 0x100) {
            let latin1: Vec<u8> = text.chars().map(|c| c as u8).collect();
            if latin1 != utf8 {
                variants.push(("Latin-1/PDFDoc", latin1.clone()));
            }
            variants.push(("Hex-String (Latin-1, gross)", hex_ascii(&latin1, true)));
            variants.push(("Hex-String (Latin-1, klein)", hex_ascii(&latin1, false)));
        }

        let utf16: Vec<u8> = text
            .encode_utf16()
            .flat_map(|u| u.to_be_bytes())
            .collect::<Vec<u8>>();
        variants.push(("UTF-16BE", utf16.clone()));
        variants.push(("Hex-String (UTF-16BE, gross)", hex_ascii(&utf16, true)));
        variants.push(("Hex-String (UTF-16BE, klein)", hex_ascii(&utf16, false)));

        variants.dedup_by(|a, b| a.1 == b.1);

        let squeezed = squeeze(text);
        Self {
            text: text.to_string(),
            squeezed: (squeezed != text).then_some(squeezed),
            variants: variants
                .into_iter()
                .map(|(how, pat)| (how, memmem::Finder::new(&pat).into_owned()))
                .collect(),
        }
    }
}

/// Alle Suchbegriffe eines Laufs samt ihren Fundstellen.
///
/// Der Durchgang durch die Datei kennt nur noch dieses eine Bündel, deshalb
/// wird jeder Stream einmal entpackt und jede Zeichenkette einmal dekodiert —
/// unabhängig davon, wie viele Begriffe gesucht werden.
struct Probe {
    /// Die nicht-leeren Begriffe. Ein leerer Begriff stünde in jeder Datei;
    /// er wird nicht gesucht, behält aber unten seinen (leeren) Platz.
    needles: Vec<Needle>,
    reports: Vec<Report>,
    /// Zu jedem Eintrag oben: seine Position in der Eingabe.
    slots: Vec<usize>,
    /// Anzahl der Begriffe in der Eingabe, inklusive der leeren.
    total: usize,
    /// Sucht mindestens ein Begriff auch ohne Leerraum? Nur dann lohnt es,
    /// den Leerraum aus einem Datenblock zu entfernen.
    any_squeezed: bool,
}

impl Probe {
    fn new(input: &[&str]) -> Self {
        let mut probe = Self {
            needles: Vec::new(),
            reports: Vec::new(),
            slots: Vec::new(),
            total: input.len(),
            any_squeezed: false,
        };
        for (slot, text) in input.iter().enumerate() {
            if text.is_empty() {
                continue;
            }
            let needle = Needle::new(text);
            probe.any_squeezed |= needle.squeezed.is_some();
            probe.needles.push(needle);
            probe.reports.push(Report::default());
            probe.slots.push(slot);
        }
        probe
    }

    /// Führt `f` für jeden Begriff mit seinem eigenen Bericht aus.
    fn each(&mut self, mut f: impl FnMut(&Needle, &mut Report)) {
        for (needle, report) in self.needles.iter().zip(self.reports.iter_mut()) {
            f(needle, report);
        }
    }

    /// Die Fundstellen in der Reihenfolge der Eingabe.
    fn into_hits(self) -> Vec<Vec<String>> {
        let mut out = vec![Vec::new(); self.total];
        for (slot, report) in self.slots.into_iter().zip(self.reports) {
            out[slot] = report.hits;
        }
        out
    }
}

/// Entfernt jeden Leerraum — beide Seiten eines Vergleichs werden so behandelt.
fn squeeze(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

fn hex_ascii(bytes: &[u8], upper: bool) -> Vec<u8> {
    let digits: &[u8; 16] = if upper {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    let mut out = Vec::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(digits[(b >> 4) as usize]);
        out.push(digits[(b & 0x0f) as usize]);
    }
    out
}

// ---------------------------------------------------------------------------
// Fundstellen sammeln
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Report {
    hits: Vec<String>,
    seen: BTreeSet<String>,
}

impl Report {
    fn push(&mut self, message: String) {
        if self.hits.len() < MAX_HITS && self.seen.insert(message.clone()) {
            self.hits.push(message);
        }
    }

    /// Fund in einer Bytefolge.
    fn hit_bytes(&mut self, location: &str, how: &str, hay: &[u8], pos: usize, len: usize) {
        let ctx = printable_context(hay, pos, len);
        self.push(format!("{location} [{how}]: …{ctx}…"));
    }

    /// Fund in bereits dekodiertem Text.
    fn hit_text(&mut self, location: &str, how: &str, hay: &str, pos: usize, len: usize) {
        let ctx = printable_context(hay.as_bytes(), pos, len);
        self.push(format!("{location} [{how}]: …{ctx}…"));
    }
}

fn printable_context(hay: &[u8], pos: usize, len: usize) -> String {
    let start = pos.saturating_sub(CONTEXT);
    let end = (pos + len + CONTEXT).min(hay.len());
    hay[start..end]
        .iter()
        .map(|&b| {
            if (0x20..0x7f).contains(&b) {
                b as char
            } else {
                '.'
            }
        })
        .collect()
}

/// Alle Fundstellen von `pat` in `hay` (überlappungsfrei), höchstens `limit`.
///
/// Bis 0.7.0 stand hier `windows(pat.len()).position(|w| w == pat)` — ein
/// Vergleich je Byte und Begriff, in jeder Kodierung, auf jeder Sichtweise.
/// Das kostete **Begriffe × Dateigröße**, obwohl der Durchgang selbst längst
/// nur noch einmal lief: gemessen an einer 898-kB-Datei 65 ms je Begriff
/// (200 Begriffe 13,0 s). `memmem` findet dasselbe — überlappungsfrei, in
/// derselben Reihenfolge — mit SIMD-Vorfilter; die Fundstellen bleiben
/// Stelle für Stelle gleich (`find_all_agrees_with_the_naive_search`).
fn find_all(hay: &[u8], pat: &memmem::Finder<'_>, limit: usize) -> Vec<usize> {
    if pat.needle().is_empty() {
        return Vec::new();
    }
    pat.find_iter(hay).take(limit).collect()
}

// ---------------------------------------------------------------------------
// Ebene 1+2: Rohdatei und rohe Streams
// ---------------------------------------------------------------------------

fn scan_raw_file(bytes: &[u8], probe: &mut Probe) {
    probe.each(|needle, report| {
        for (how, pat) in &needle.variants {
            for pos in find_all(bytes, pat, 8) {
                let len = pat.needle().len();
                report.hit_bytes(&format!("Rohdatei @0x{pos:x}"), how, bytes, pos, len);
            }
        }
    });
}

/// Findet jeden `stream … endstream`-Block anhand der Rohbytes — unabhängig
/// davon, ob die xref-Tabelle das Objekt noch kennt. Genau so überlebt die
/// Historie inkrementeller Updates.
fn scan_raw_streams(bytes: &[u8], probe: &mut Probe) {
    for (offset, payload) in raw_stream_blocks(bytes) {
        let base = format!("Rohdaten-Stream @0x{offset:x}");
        scan_blob(payload, &format!("{base} (roh)"), probe);
        if let Some(inflated) = inflate(payload) {
            scan_blob(&inflated, &format!("{base} (inflate)"), probe);
        }
    }
}

fn raw_stream_blocks(bytes: &[u8]) -> Vec<(usize, &[u8])> {
    let stream = memmem::Finder::new(b"stream");
    let endstream = memmem::Finder::new(b"endstream");
    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(rel) = stream.find(&bytes[i..]) {
        let start = i + rel;
        i = start + 6;
        // „endstream“ endet ebenfalls auf „stream“ — solche Treffer überspringen.
        if start >= 3 && &bytes[start - 3..start] == b"end" {
            continue;
        }
        let mut data = i;
        if bytes.get(data) == Some(&b'\r') {
            data += 1;
        }
        if bytes.get(data) == Some(&b'\n') {
            data += 1;
        }
        let Some(rel_end) = endstream.find(&bytes[data..]) else {
            break;
        };
        out.push((data, &bytes[data..data + rel_end]));
        i = data + rel_end + 9;
    }
    out
}

// ---------------------------------------------------------------------------
// Ebene 3–6: Objektgraph
// ---------------------------------------------------------------------------

fn scan_object_graph(doc: &Document, probe: &mut Probe) {
    walk_dict(&doc.trailer, "Trailer", probe, 0);
    for (id, object) in &doc.objects {
        let path = format!("Objekt {} {}", id.0, id.1);
        walk(object, &path, probe, 0);
    }
}

fn walk(object: &Object, path: &str, probe: &mut Probe, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    match object {
        Object::String(raw, format) => {
            let how = match format {
                StringFormat::Literal => "Zeichenkette, literal",
                StringFormat::Hexadecimal => "Zeichenkette, hex",
            };
            scan_string(raw, path, how, probe);
        }
        Object::Name(name) => scan_raw_bytes(name, path, "Name", probe),
        Object::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                walk(item, &format!("{path}[{i}]"), probe, depth + 1);
            }
        }
        Object::Dictionary(dict) => walk_dict(dict, path, probe, depth),
        Object::Stream(stream) => {
            walk_dict(&stream.dict, path, probe, depth);
            scan_stream(stream, path, probe, depth);
        }
        _ => {}
    }
}

fn walk_dict(dict: &Dictionary, path: &str, probe: &mut Probe, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    for (key, value) in dict.iter() {
        let key = String::from_utf8_lossy(key);
        walk(value, &format!("{path}/{key}"), probe, depth + 1);
    }
}

fn scan_stream(stream: &Stream, path: &str, probe: &mut Probe, depth: usize) {
    for (label, data) in stream_payloads(stream) {
        scan_blob(&data, &format!("{path} <Stream, {label}>"), probe);
    }

    // Objekt-Streams sind komprimierte Container: die enthaltenen Objekte
    // stehen nirgends im Klartext und entgehen jeder Rohbyte-Suche.
    if stream.dict.has_type(b"ObjStm") {
        let mut copy = stream.clone();
        if let Ok(object_stream) = ObjectStream::new(&mut copy) {
            for (id, object) in &object_stream.objects {
                let inner = format!("{path} <ObjStm> → Objekt {} {}", id.0, id.1);
                walk(object, &inner, probe, depth + 1);
            }
        }
    }
}

/// Alle Sichten auf einen Stream, in denen das Geheimnis stehen könnte.
///
/// `lopdf` beherrscht FlateDecode, LZWDecode und ASCII85Decode; ASCIIHexDecode
/// und RunLengthDecode ergänzt [`manual_decode`]. Ist ein Filter unbekannt,
/// bleiben die Rohbytes — die werden ohnehin immer mitdurchsucht.
fn stream_payloads(stream: &Stream) -> Vec<(String, Vec<u8>)> {
    let mut out = vec![("roh".to_string(), stream.content.clone())];
    let filters = stream.filters().unwrap_or_default();
    if filters.is_empty() {
        return out;
    }
    if let Ok(decoded) = stream.decompressed_content() {
        out.push(("lopdf-dekodiert".to_string(), decoded));
    } else if let Some(decoded) = manual_decode(&stream.content, &filters) {
        let names: Vec<String> = filters
            .iter()
            .map(|f| String::from_utf8_lossy(f).into_owned())
            .collect();
        out.push((format!("dekodiert: {}", names.join("+")), decoded));
    }
    out
}

/// Filternamen kommen seit lopdf 0.42 als Rohbytes (`Vec<&[u8]>`) statt als
/// `String` — ein Filtername ist im PDF ein Name-Objekt und muss kein
/// gültiges UTF-8 sein. Der Vergleich läuft deshalb byteweise.
fn manual_decode(content: &[u8], filters: &[&[u8]]) -> Option<Vec<u8>> {
    let mut data = content.to_vec();
    let mut decoded_any = false;
    for filter in filters {
        let next = match *filter {
            b"FlateDecode" | b"Fl" => inflate(&data),
            b"ASCIIHexDecode" | b"AHx" => Some(ascii_hex_decode(&data)),
            b"RunLengthDecode" | b"RL" => Some(run_length_decode(&data)),
            // LZW und ASCII85 deckt lopdf ab; alles andere ist unbekannt und
            // fällt auf die Rohbytes zurück.
            _ => None,
        };
        match next {
            Some(decoded) => {
                data = decoded;
                decoded_any = true;
            }
            None => break,
        }
    }
    decoded_any.then_some(data)
}

fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    // Teilergebnisse zählen: ein abgeschnittener Stream soll trotzdem
    // durchsuchbar sein.
    let _ = flate2::read::ZlibDecoder::new(data).read_to_end(&mut out);
    if !out.is_empty() {
        return Some(out);
    }
    let mut raw = Vec::new();
    let _ = flate2::read::DeflateDecoder::new(data).read_to_end(&mut raw);
    (!raw.is_empty()).then_some(raw)
}

fn ascii_hex_decode(data: &[u8]) -> Vec<u8> {
    let mut nibbles = Vec::new();
    for &b in data {
        if b == b'>' {
            break;
        }
        if let Some(v) = hex_value(b) {
            nibbles.push(v);
        }
    }
    if nibbles.len() % 2 == 1 {
        nibbles.push(0);
    }
    nibbles.chunks(2).map(|c| (c[0] << 4) | c[1]).collect()
}

fn run_length_decode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < data.len() {
        let length = data[i];
        i += 1;
        match length {
            128 => break,
            0..=127 => {
                let n = length as usize + 1;
                let end = (i + n).min(data.len());
                out.extend_from_slice(&data[i..end]);
                i = end;
            }
            _ => {
                if let Some(&b) = data.get(i) {
                    out.extend(std::iter::repeat_n(b, 257 - length as usize));
                }
                i += 1;
            }
        }
    }
    out
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Vergleiche
// ---------------------------------------------------------------------------

/// Durchsucht einen (dekodierten) Datenblock: erst byteweise in allen
/// Kodierungen, dann die Verkettung aller darin enthaltenen
/// Zeichenketten-Literale.
fn scan_blob(blob: &[u8], location: &str, probe: &mut Probe) {
    scan_raw_bytes(blob, location, "Inhalt", probe);

    // Verkettung und Leerraum-Fassung hängen allein am Datenblock: einmal
    // bilden, dann von jedem Suchbegriff benutzen.
    let joined = concat_pdf_strings(blob);
    if joined.is_empty() {
        return;
    }
    let squeezed = probe.any_squeezed.then(|| squeeze(&joined));
    probe.each(|needle, report| {
        if let Some(pos) = joined.find(&needle.text) {
            report.hit_text(
                location,
                "Zeichenketten-Verkettung",
                &joined,
                pos,
                needle.text.len(),
            );
        }
        if let (Some(needle_squeezed), Some(squeezed)) = (&needle.squeezed, &squeezed) {
            if let Some(pos) = squeezed.find(needle_squeezed) {
                report.hit_text(
                    location,
                    "Zeichenketten-Verkettung, ohne Leerraum",
                    squeezed,
                    pos,
                    needle_squeezed.len(),
                );
            }
        }
    });
}

fn scan_raw_bytes(hay: &[u8], location: &str, how: &str, probe: &mut Probe) {
    probe.each(|needle, report| {
        for (variant, pat) in &needle.variants {
            for pos in find_all(hay, pat, 4) {
                let len = pat.needle().len();
                report.hit_bytes(location, &format!("{how}, {variant}"), hay, pos, len);
            }
        }
    });
}

/// Zeichenketten-Objekt: sowohl dekodiert (PDFDocEncoding **oder** UTF-16BE)
/// als auch roh vergleichen.
fn scan_string(raw: &[u8], location: &str, how: &str, probe: &mut Probe) {
    // Dekodieren hängt allein an der Zeichenkette, nicht am Suchbegriff.
    scan_text(&decode_pdf_string(raw), location, how, probe);
    scan_raw_bytes(raw, location, how, probe);
}

/// Bereits dekodierter Text: als Ganzes und, wo der Begriff Leerraum hat,
/// ohne jeden Leerraum.
fn scan_text(text: &str, location: &str, how: &str, probe: &mut Probe) {
    let squeezed = probe.any_squeezed.then(|| squeeze(text));
    probe.each(|needle, report| {
        if let Some(pos) = text.find(&needle.text) {
            report.hit_text(location, how, text, pos, needle.text.len());
        } else if let (Some(needle_squeezed), Some(squeezed)) = (&needle.squeezed, &squeezed) {
            if let Some(pos) = squeezed.find(needle_squeezed) {
                report.hit_text(
                    location,
                    &format!("{how}, ohne Leerraum"),
                    squeezed,
                    pos,
                    needle_squeezed.len(),
                );
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Ebene 7: der Text, wie der eigene Schriftdekoder ihn liest
// ---------------------------------------------------------------------------

/// Durchsucht jede Seite so, wie die Analyse sie liest: Glyphencodes über
/// `/ToUnicode`, `/Differences` und die Standardkodierungen in Zeichen
/// übersetzt, zu Zeilen zusammengesetzt (siehe [`PdfExtractor`]).
///
/// Die Sichtweisen 1–6 vergleichen Bytes. In einer eingebetteten
/// Teilmengen-Schrift — dem Regelfall aus Word, LibreOffice, Chrome — stehen
/// im Strom aber keine Zeichen, sondern Glyphnummern oder umgelenkte Codes:
/// `<01020304>Tj` für „Kont“. Keine der sechs Bytesichten kann darin eine
/// IBAN finden; gemessen an einem LibreOffice-Writer-24.2-Export meldete der
/// Detektor an der **ungeschwärzten** Datei „nicht gefunden“ für alle vier
/// Begriffe.
///
/// Diese Sicht kommt **dazu**, nicht an die Stelle der anderen. Allein wäre
/// sie der Zirkelschluss aus dem Modulkommentar: wovor der Dekoder blind ist,
/// wäre auch hier unsichtbar. Beide zusammen decken sich gegenseitig: die
/// Bytesichten finden, was der Dekoder nicht liest (Metadaten, verwaiste
/// Objekte, Rohtext in einem Strom, den kein `Do` erreicht); der Dekoder
/// findet, was die Bytesichten nicht lesen (Glyphencodes).
///
/// Eine Seite, die der Interpreter ablehnt (Aufwandskonto gerissen, Strom
/// nicht zerlegbar), fehlt in dieser Sicht — die Bytesichten haben sie
/// trotzdem durchsucht. Damit sie nicht die anderen Seiten mitnimmt, liest
/// [`PdfExtractor::extract_lenient`] in **einem** Durchgang über **einen**
/// Seitenbaum und überspringt nur die abgelehnte Seite. Der frühere
/// Rückfall — nach dem ersten Fehler Seite für Seite über eine
/// seitenweise Extraktion, die je Aufruf den Seitenbaum neu baute — war
/// quadratisch in der Seitenzahl: gemessen an einem 4 000-Seiten-Dokument
/// 8,7 s für das **ganze Dokument** gegenüber 1,3 s für
/// [`PdfExtractor::extract`]; heute misst `zb_rueckfall_linear.rs` den
/// Speicher, und der wächst linear.
fn scan_decoded_text(doc: &Document, probe: &mut Probe) {
    let (runs, _) = PdfExtractor::new().extract_lenient(doc);
    // Die Zeilen kommen seitenweise sortiert; je Seite ein Text.
    for page_runs in runs.chunk_by(|a, b| a.page == b.page) {
        let text: String = page_runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        scan_text(
            &text,
            &format!("Seite {}", page_runs[0].page + 1),
            "Schriftdekoder",
            probe,
        );
    }
}

/// Dekodiert eine PDF-Zeichenkette.
///
/// UTF-16 wird am BOM erkannt (`FE FF`, in freier Wildbahn auch `FF FE`);
/// alles andere wird als PDFDocEncoding gelesen, das im hier interessanten
/// Bereich mit Latin-1 zusammenfällt.
pub fn decode_pdf_string(raw: &[u8]) -> String {
    if raw.len() >= 2 && raw[0] == 0xfe && raw[1] == 0xff {
        let units: Vec<u16> = raw[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else if raw.len() >= 2 && raw[0] == 0xff && raw[1] == 0xfe {
        let units: Vec<u16> = raw[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        raw.iter().map(|&b| b as char).collect()
    }
}

/// Verkettet alle Zeichenketten-Literale eines Blocks — literal `(…)` wie
/// hexadezimal `<…>`.
///
/// Bewusst ein eigener Mini-Lexer statt `lopdf::content::Content::decode`:
/// dessen Parser verliert bei einem Inline-Bild (`BI … ID … EI`) den Rest des
/// Streams — genau der Fehler, den dieses Modell aufdecken soll.
fn concat_pdf_strings(blob: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < blob.len() {
        match blob[i] {
            b'(' => {
                let (bytes, next) = read_literal_string(blob, i + 1);
                out.push_str(&decode_pdf_string(&bytes));
                i = next;
            }
            b'<' if blob.get(i + 1) != Some(&b'<') => {
                let (bytes, next) = read_hex_string(blob, i + 1);
                out.push_str(&decode_pdf_string(&bytes));
                i = next;
            }
            _ => i += 1,
        }
    }
    out
}

fn read_literal_string(blob: &[u8], mut i: usize) -> (Vec<u8>, usize) {
    let mut out = Vec::new();
    let mut depth = 1usize;
    while i < blob.len() {
        match blob[i] {
            b'\\' => {
                i += 1;
                let Some(&esc) = blob.get(i) else { break };
                match esc {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(0x08),
                    b'f' => out.push(0x0c),
                    b'\n' => {}
                    b'\r' => {
                        if blob.get(i + 1) == Some(&b'\n') {
                            i += 1;
                        }
                    }
                    b'0'..=b'7' => {
                        let mut value = u32::from(esc - b'0');
                        let mut taken = 1;
                        while taken < 3 {
                            match blob.get(i + 1) {
                                Some(&d @ b'0'..=b'7') => {
                                    value = value * 8 + u32::from(d - b'0');
                                    i += 1;
                                    taken += 1;
                                }
                                _ => break,
                            }
                        }
                        out.push(value as u8);
                    }
                    other => out.push(other),
                }
                i += 1;
            }
            b'(' => {
                depth += 1;
                out.push(b'(');
                i += 1;
            }
            b')' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    break;
                }
                out.push(b')');
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    (out, i)
}

fn read_hex_string(blob: &[u8], mut i: usize) -> (Vec<u8>, usize) {
    let start = i;
    let mut nibbles = Vec::new();
    while i < blob.len() && blob[i] != b'>' {
        match hex_value(blob[i]) {
            Some(v) => nibbles.push(v),
            None if blob[i].is_ascii_whitespace() => {}
            // Kein Hex-String, sondern irgendein anderes `<` im Datenstrom.
            None => return (Vec::new(), start),
        }
        i += 1;
    }
    if i < blob.len() {
        i += 1;
    }
    if nibbles.len() % 2 == 1 {
        nibbles.push(0);
    }
    let bytes = nibbles.chunks(2).map(|c| (c[0] << 4) | c[1]).collect();
    (bytes, i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_nothing_in_an_empty_haystack() {
        assert!(leaks(b"", "GEHEIM").is_empty());
        assert!(leaks(b"%PDF-1.5\n", "GEHEIM").is_empty());
    }

    #[test]
    fn empty_needle_never_matches() {
        assert!(leaks(b"irgendwas", "").is_empty());
    }

    #[test]
    fn finds_plain_bytes_with_offset_and_context() {
        let hits = leaks(b"%PDF-1.5\n(Konto GEHEIM 42)\n", "GEHEIM");
        assert!(!hits.is_empty());
        assert!(hits[0].contains("Rohdatei"), "{hits:?}");
        assert!(hits[0].contains("Konto GEHEIM 42"), "{hits:?}");
    }

    #[test]
    fn finds_utf16be_encoded_text() {
        let mut data = b"%PDF-1.5\n(".to_vec();
        data.extend_from_slice(&[0xfe, 0xff]);
        data.extend("GEHEIM".encode_utf16().flat_map(|u| u.to_be_bytes()));
        data.extend_from_slice(b")\n");
        let hits = leaks(&data, "GEHEIM");
        assert!(
            hits.iter().any(|h| h.contains("UTF-16BE")),
            "UTF-16BE nicht erkannt: {hits:?}"
        );
    }

    #[test]
    fn finds_hex_string_syntax() {
        let hits = leaks(b"%PDF-1.5\n<47454845494D>\n", "GEHEIM");
        assert!(
            hits.iter().any(|h| h.contains("Hex-String")
                || h.contains("Zeichenketten-Verkettung")
                || h.contains("Inhalt")),
            "{hits:?}"
        );
    }

    /// Die alte Suche, Byte für Byte — als Maßstab für die neue.
    fn find_all_naive(hay: &[u8], pat: &[u8], limit: usize) -> Vec<usize> {
        if pat.is_empty() || hay.len() < pat.len() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut from = 0usize;
        while from + pat.len() <= hay.len() {
            match hay[from..].windows(pat.len()).position(|w| w == pat) {
                Some(rel) => {
                    out.push(from + rel);
                    if out.len() >= limit {
                        break;
                    }
                    from += rel + pat.len();
                }
                None => break,
            }
        }
        out
    }

    /// `memmem` muss Fundstelle für Fundstelle dasselbe liefern wie die
    /// naive Suche: überlappungsfrei, in Reihenfolge, an der Grenze
    /// abgeschnitten. Die Fälle: Muster länger als der Heuhaufen, Muster am
    /// Ende, überlappende Vorkommen (`aaaa` in `aaaaaaa` sind zwei, nicht
    /// vier), Grenze kleiner als die Zahl der Vorkommen, leeres Muster.
    #[test]
    fn find_all_agrees_with_the_naive_search() {
        let cases: &[(&[u8], &[u8], usize)] = &[
            (b"", b"a", 4),
            (b"ab", b"abc", 4),
            (b"xxabc", b"abc", 4),
            (b"aaaaaaa", b"aaaa", 4),
            (b"abcabcabcabc", b"abc", 2),
            (b"abcabcabcabc", b"abc", 8),
            (b"abc", b"", 4),
            (b"a.b.c.d.e.f", b".", 3),
            (b"DE89 3704 0044 DE89 3704", b"DE89 3704", 8),
        ];
        for (hay, pat, limit) in cases {
            assert_eq!(
                find_all(hay, &memmem::Finder::new(pat), *limit),
                find_all_naive(hay, pat, *limit),
                "hay={hay:?} pat={pat:?} limit={limit}"
            );
        }
        assert_eq!(
            find_all(b"aaaaaaa", &memmem::Finder::new(b"aaaa"), 4),
            vec![0]
        );
        assert_eq!(
            find_all(b"abcabcabcabc", &memmem::Finder::new(b"abc"), 2),
            vec![0, 3]
        );
    }

    #[test]
    fn run_length_roundtrip() {
        // 3 Literalbytes, dann 4× 'A'.
        let encoded = [2u8, b'X', b'Y', b'Z', 253, b'A', 128];
        assert_eq!(run_length_decode(&encoded), b"XYZAAAA");
    }

    #[test]
    fn ascii_hex_stops_at_terminator() {
        assert_eq!(ascii_hex_decode(b"4142>4344"), b"AB");
    }

    #[test]
    fn literal_string_handles_escapes_and_nesting() {
        let (bytes, _) = read_literal_string(b"a\\(b(c)d\\101)rest", 0);
        assert_eq!(bytes, b"a(b(c)dA");
    }

    #[test]
    fn concatenation_joins_tj_fragments() {
        let blob = b"[(DE89 3704 0044 )-2(0532 0130 00)] TJ";
        let joined = concat_pdf_strings(blob);
        assert_eq!(joined, "DE89 3704 0044 0532 0130 00");
    }

    #[test]
    fn decodes_both_string_encodings() {
        assert_eq!(decode_pdf_string(b"Hallo"), "Hallo");
        let mut utf16 = vec![0xfe, 0xff];
        utf16.extend("Hallo".encode_utf16().flat_map(|u| u.to_be_bytes()));
        assert_eq!(decode_pdf_string(&utf16), "Hallo");
    }

    /// Die gesuchten Begriffe eines Laufs — Treffer und Nicht-Treffer,
    /// mit und ohne Leerraum, ein leerer dazwischen.
    const NEEDLES: &[&str] = &[
        "DE89 3704 0044 0532 0130 00",
        "Max Mustermann",
        "kommtnichtvor",
        "",
        "Kontonummer",
    ];

    /// `leaks_many` ist dieselbe Messung wie `leaks`, nur in einem Durchgang.
    ///
    /// Das ist die Zusicherung, die zählt: das ehrliche Orakel darf durch die
    /// Zusammenfassung kein einziges Leck weniger melden. Verglichen wird
    /// Fundstelle für Fundstelle, nicht bloß „auch etwas gefunden“.
    #[test]
    fn one_pass_reports_exactly_what_the_single_pass_reports() {
        let pdf = crate::testing::demo_statement();
        let many = leaks_many(&pdf, NEEDLES);
        assert_eq!(many.len(), NEEDLES.len());

        for (needle, hits) in NEEDLES.iter().zip(&many) {
            assert_eq!(
                hits,
                &leaks(&pdf, needle),
                "abweichende Fundstellen für {needle:?}"
            );
        }

        // Und der Test misst wirklich etwas: mindestens ein Begriff steht in
        // der ungeschwärzten Vorlage, sonst verglichen wir nur leere Listen.
        assert!(
            many.iter().any(|hits| !hits.is_empty()),
            "kein Begriff gefunden — dieser Test würde jede Änderung durchwinken"
        );
        assert!(
            many[2].is_empty() && many[3].is_empty(),
            "Nicht-Treffer und leerer Begriff müssen leer bleiben: {:?}",
            &many[2..4]
        );
    }

    /// Der Durchgang darf die Begriffe nicht vermischen: jeder Bericht gehört
    /// zu genau seinem Begriff, auch wenn ein leerer dazwischensteht.
    #[test]
    fn each_report_belongs_to_its_own_needle() {
        let pdf = crate::testing::minimal_pdf("Alpha Beta");
        let hits = leaks_many(&pdf, &["Alpha", "", "Beta", "Gamma"]);
        assert_eq!(hits.len(), 4);
        assert!(hits[0].iter().all(|h| h.contains("Alpha")), "{:?}", hits[0]);
        assert!(hits[1].is_empty());
        assert!(hits[2].iter().all(|h| h.contains("Beta")), "{:?}", hits[2]);
        assert!(hits[3].is_empty());
    }

    /// Die Suche ohne Leerraum darf nicht daran hängen, welche *anderen*
    /// Begriffe im selben Lauf stehen.
    ///
    /// Der Durchgang bildet die Leerraum-Fassung eines Datenblocks einmal für
    /// alle Begriffe. Genau hier könnte die Zusammenfassung einen Begriff um
    /// seine Fundstelle bringen — deshalb wird beides geprüft: allein und in
    /// Gesellschaft eines Begriffs ohne Leerraum.
    #[test]
    fn whitespace_free_comparison_survives_the_shared_pass() {
        // Im PDF steht die IBAN in Stücken, gesucht wird sie mit Leerzeichen.
        let pdf =
            b"%PDF-1.5\n1 0 obj\n<< >>\nstream\n[(DE89)-2(3704)-2(0044)] TJ\nendstream\nendobj\n";
        let needle = "DE89 3704 0044";

        let alone = leaks_many(pdf, &[needle]);
        assert!(
            alone[0].iter().any(|h| h.contains("ohne Leerraum")),
            "ohne Leerraum nicht gefunden: {:?}",
            alone[0]
        );

        // Derselbe Begriff neben einem, der gar keinen Leerraum enthält.
        let together = leaks_many(pdf, &["Kontonummer", needle]);
        assert_eq!(
            together[1], alone[0],
            "Fundstellen hängen an der Nachbarschaft"
        );
    }

    /// Ein Durchgang statt einer je Begriff — belegt an der Zahl der
    /// entpackten Streams.
    ///
    /// Die Laufzeit selbst zu messen wäre auf einer geteilten Maschine
    /// wackelig. Gezählt wird stattdessen die Arbeit, die früher je Begriff
    /// anfiel: `Document::load_mem` ist der teuerste Einzelschritt, und die
    /// Rohbyte-Suche über die ganze Datei der zweitteuerste. Beide hängen
    /// hier nur noch an der Datei.
    #[test]
    fn the_file_is_parsed_once_no_matter_how_many_needles() {
        let pdf = crate::testing::demo_statement();
        let mut probe = Probe::new(NEEDLES);
        assert_eq!(
            probe.needles.len(),
            4,
            "der leere Begriff darf nicht gesucht werden"
        );

        // Der Objektgraph wird genau einmal abgelaufen — nachweisbar daran,
        // dass `scan_object_graph` ein `&mut Probe` mit allen Begriffen nimmt
        // und nicht je Begriff aufgerufen wird. Der Aufruf hier ist derselbe
        // wie in `leaks_many`.
        let doc = Document::load_mem(&pdf).expect("Vorlage parsebar");
        scan_object_graph(&doc, &mut probe);
        let hits = probe.into_hits();
        assert_eq!(hits.len(), NEEDLES.len());
        assert!(
            hits.iter().any(|h| !h.is_empty()),
            "der Objektgraph-Durchgang hat nichts gefunden"
        );
    }
}
