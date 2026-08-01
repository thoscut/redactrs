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
//!    per `TJ` in Bruchstücke zerlegt ist.
//!
//! Beide PDF-String-Kodierungen werden berücksichtigt: PDFDocEncoding/Latin-1
//! **und** UTF-16BE (mit und ohne BOM). Ebenso beide Syntaxen: literal
//! `(DE89…)` und hexadezimal `<44453839…>`.
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
    if needle.is_empty() {
        return Vec::new();
    }
    let needle = Needle::new(needle);
    let mut report = Report::default();

    scan_raw_file(pdf_bytes, &needle, &mut report);
    scan_raw_streams(pdf_bytes, &needle, &mut report);
    scan_object_graph(pdf_bytes, &needle, &mut report);

    report.hits
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
    /// Bytefolgen: (Beschreibung, Muster).
    variants: Vec<(&'static str, Vec<u8>)>,
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
            variants,
        }
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
fn find_all(hay: &[u8], pat: &[u8], limit: usize) -> Vec<usize> {
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

// ---------------------------------------------------------------------------
// Ebene 1+2: Rohdatei und rohe Streams
// ---------------------------------------------------------------------------

fn scan_raw_file(bytes: &[u8], needle: &Needle, report: &mut Report) {
    for (how, pat) in &needle.variants {
        for pos in find_all(bytes, pat, 8) {
            report.hit_bytes(&format!("Rohdatei @0x{pos:x}"), how, bytes, pos, pat.len());
        }
    }
}

/// Findet jeden `stream … endstream`-Block anhand der Rohbytes — unabhängig
/// davon, ob die xref-Tabelle das Objekt noch kennt. Genau so überlebt die
/// Historie inkrementeller Updates.
fn scan_raw_streams(bytes: &[u8], needle: &Needle, report: &mut Report) {
    for (offset, payload) in raw_stream_blocks(bytes) {
        let base = format!("Rohdaten-Stream @0x{offset:x}");
        scan_blob(payload, &format!("{base} (roh)"), needle, report);
        if let Some(inflated) = inflate(payload) {
            scan_blob(&inflated, &format!("{base} (inflate)"), needle, report);
        }
    }
}

fn raw_stream_blocks(bytes: &[u8]) -> Vec<(usize, &[u8])> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(rel) = bytes[i..].windows(6).position(|w| w == b"stream") {
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
        let Some(rel_end) = bytes[data..].windows(9).position(|w| w == b"endstream") else {
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

fn scan_object_graph(bytes: &[u8], needle: &Needle, report: &mut Report) {
    // Lässt sich die Datei nicht parsen, bleiben die Rohsuchen die Messung.
    let Ok(doc) = Document::load_mem(bytes) else {
        return;
    };
    walk_dict(&doc.trailer, "Trailer", needle, report, 0);
    for (id, object) in &doc.objects {
        let path = format!("Objekt {} {}", id.0, id.1);
        walk(object, &path, needle, report, 0);
    }
}

fn walk(object: &Object, path: &str, needle: &Needle, report: &mut Report, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    match object {
        Object::String(raw, format) => {
            let how = match format {
                StringFormat::Literal => "Zeichenkette, literal",
                StringFormat::Hexadecimal => "Zeichenkette, hex",
            };
            scan_string(raw, path, how, needle, report);
        }
        Object::Name(name) => scan_raw_bytes(name, path, "Name", needle, report),
        Object::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                walk(item, &format!("{path}[{i}]"), needle, report, depth + 1);
            }
        }
        Object::Dictionary(dict) => walk_dict(dict, path, needle, report, depth),
        Object::Stream(stream) => {
            walk_dict(&stream.dict, path, needle, report, depth);
            scan_stream(stream, path, needle, report, depth);
        }
        _ => {}
    }
}

fn walk_dict(dict: &Dictionary, path: &str, needle: &Needle, report: &mut Report, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    for (key, value) in dict.iter() {
        let key = String::from_utf8_lossy(key);
        walk(value, &format!("{path}/{key}"), needle, report, depth + 1);
    }
}

fn scan_stream(stream: &Stream, path: &str, needle: &Needle, report: &mut Report, depth: usize) {
    for (label, data) in stream_payloads(stream) {
        scan_blob(&data, &format!("{path} <Stream, {label}>"), needle, report);
    }

    // Objekt-Streams sind komprimierte Container: die enthaltenen Objekte
    // stehen nirgends im Klartext und entgehen jeder Rohbyte-Suche.
    if stream.dict.has_type(b"ObjStm") {
        let mut copy = stream.clone();
        if let Ok(object_stream) = ObjectStream::new(&mut copy) {
            for (id, object) in &object_stream.objects {
                let inner = format!("{path} <ObjStm> → Objekt {} {}", id.0, id.1);
                walk(object, &inner, needle, report, depth + 1);
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
fn scan_blob(blob: &[u8], location: &str, needle: &Needle, report: &mut Report) {
    scan_raw_bytes(blob, location, "Inhalt", needle, report);

    let joined = concat_pdf_strings(blob);
    if joined.is_empty() {
        return;
    }
    if let Some(pos) = joined.find(&needle.text) {
        report.hit_text(
            location,
            "Zeichenketten-Verkettung",
            &joined,
            pos,
            needle.text.len(),
        );
    }
    if let Some(needle_squeezed) = &needle.squeezed {
        let squeezed = squeeze(&joined);
        if let Some(pos) = squeezed.find(needle_squeezed) {
            report.hit_text(
                location,
                "Zeichenketten-Verkettung, ohne Leerraum",
                &squeezed,
                pos,
                needle_squeezed.len(),
            );
        }
    }
}

fn scan_raw_bytes(hay: &[u8], location: &str, how: &str, needle: &Needle, report: &mut Report) {
    for (variant, pat) in &needle.variants {
        for pos in find_all(hay, pat, 4) {
            report.hit_bytes(location, &format!("{how}, {variant}"), hay, pos, pat.len());
        }
    }
}

/// Zeichenketten-Objekt: sowohl dekodiert (PDFDocEncoding **oder** UTF-16BE)
/// als auch roh vergleichen.
fn scan_string(raw: &[u8], location: &str, how: &str, needle: &Needle, report: &mut Report) {
    let decoded = decode_pdf_string(raw);
    if let Some(pos) = decoded.find(&needle.text) {
        report.hit_text(location, how, &decoded, pos, needle.text.len());
    } else if let Some(needle_squeezed) = &needle.squeezed {
        let squeezed = squeeze(&decoded);
        if let Some(pos) = squeezed.find(needle_squeezed) {
            report.hit_text(
                location,
                &format!("{how}, ohne Leerraum"),
                &squeezed,
                pos,
                needle_squeezed.len(),
            );
        }
    }
    scan_raw_bytes(raw, location, how, needle, report);
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
}
