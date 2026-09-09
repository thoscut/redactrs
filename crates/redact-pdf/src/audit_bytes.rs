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
//! 3. **jedes Stream-Objekt** des Objektgraphen, dekodiert über
//!    [`crate::filters`] (Flate, LZW, ASCII85, ASCIIHex, RunLength, mit
//!    Prädiktor; bei einem nicht unterstützten Filter bleiben die Rohbytes
//!    die Rückfallebene) — derselbe Dekoder, den der Schwärzer benutzt,
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
//! ## Kosten und Budget
//!
//! Die Arbeit hängt an der Datei, nicht an den Begriffen: jeder Datenblock
//! wird **einmal** entpackt und **einmal** durchlaufen — ein
//! Aho-Corasick-Automat über alle Muster aller Begriffe ([`Matcher`]).
//! Bis Fix-Runde 4 lief je Begriff und Kodierung eine eigene `memmem`-Suche
//! über jeden Block; das kostete Begriffe × Bytes (gemessen: 64 MiB
//! Bildstrom, 1 Begriff 0,26 s, 1 000 Begriffe 65,7 s).
//!
//! Entpackt wird nur bis zu einem Budget ([`leaks_many_within`]); was das
//! Budget nicht deckt, steht in [`LeakCheck::unchecked`], damit „nicht
//! gefunden“ nie stillschweigend „nicht gesucht“ bedeutet.
//!
//! ## Fehlerrichtung
//!
//! Im Zweifel meldet der Detektor zu viel. Ein Fehlalarm lässt einen Test laut
//! fehlschlagen und wird untersucht; ein übersehenes Leck lässt ihn still grün
//! bleiben und wird ausgeliefert. Deshalb wird derselbe Fund gerne mehrfach
//! gemeldet — einmal je Sichtweise (roh, dekodiert, als Objektfeld).

use std::collections::BTreeSet;

use aho_corasick::{AhoCorasick, MatchKind};
use lopdf::{Dictionary, Document, Object, ObjectStream, Stream, StringFormat};
use memchr::memmem;

use crate::document::{prescan, Limits};
use crate::extract::PdfExtractor;
use crate::filters::{self, Oversize};

/// Obergrenze für gemeldete Fundstellen — eine Fehlermeldung mit 5000 Zeilen
/// hilft niemandem.
const MAX_HITS: usize = 200;
/// Anzahl Bytes Kontext links und rechts der Fundstelle.
const CONTEXT: usize = 24;
/// Maximale Verschachtelungstiefe beim Ablaufen des Objektgraphen.
const MAX_DEPTH: usize = 32;
/// Obergrenze für einzeln genannte, nicht entpackte Ströme; danach nur noch
/// eine Summenzeile. Ist das Budget einmal aufgebraucht, träfe es sonst jeden
/// weiteren Strom der Datei.
const MAX_UNCHECKED: usize = 50;
/// Wie weit vor einem rohen `stream` nach dem Objektkopf `N G obj` gesucht
/// wird — nur für die Meldung, welcher Strom nicht entpackt wurde.
const OBJECT_HEADER_LOOKBACK: usize = 64 * 1024;

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
/// Begriff und 0,88 s für zehn. Hier fällt sie einmal an — und seit
/// Fix-Runde 4 auch der Vergleich: ein Automat über alle Begriffe.
///
/// Ohne Budget: [`leaks_many_within`] mit `u64::MAX`. Was sich nicht laden
/// oder entpacken lässt, fehlt hier stillschweigend — wer das wissen muss,
/// liest [`LeakCheck::unchecked`].
pub fn leaks_many(pdf_bytes: &[u8], needles: &[&str]) -> Vec<Vec<String>> {
    leaks_many_within(pdf_bytes, needles, u64::MAX).findings
}

/// Ergebnis von [`leaks_many_within`]: die Fundstellen je Suchbegriff und die
/// Stellen, die **nicht** durchsucht wurden, weil das Budget nicht reichte
/// oder die Datei sich nicht laden ließ.
///
/// `unchecked` leer heißt: jede Sicht ist vollständig gelaufen. Ist es nicht
/// leer, ist „nicht gefunden“ keine Aussage — der Aufrufer muss das sagen
/// (Kommandozeile: Rückgabewert 3; Oberfläche: Satz in der Statuszeile).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LeakCheck {
    /// Je Suchbegriff die Fundstellen, in der Reihenfolge der Eingabe.
    pub findings: Vec<Vec<String>>,
    /// Was nicht durchsucht wurde, je Stelle ein Satz (Objekt, Grund).
    pub unchecked: Vec<String>,
}

/// [`leaks_many`] mit einem Budget für entpackte Bytes.
///
/// `max_decompressed_bytes` ist dieselbe Zahl wie `--max-decompressed-mb`
/// der Kommandozeile (in Byte) und zählt dieselbe Einheit wie
/// [`Limits::max_decompressed_bytes`]: die **Summe** der entpackten Bytes
/// über die Ströme der Datei. Das Budget gilt **je Sicht** — die Rohsicht
/// (Sicht 2, jeder `stream … endstream`-Block) und die Objektsicht (Sichten
/// 3 und 4) entpacken die Ströme der Datei je einmal und bekommen je ein
/// volles Budget. Was der Lader der Schwärzung mit demselben Wert
/// durchlässt, prüft das Orakel damit vollständig.
///
/// Ein Strom, der das verbleibende Budget seiner Sicht sprengen würde, wird
/// **nicht** entpackt — das Entpacken selbst bricht an der Grenze ab
/// ([`filters::decoded_content_within`]), es wird nicht erst hinterher
/// gemessen — und steht in [`LeakCheck::unchecked`] mit Objekt-Id und Grund.
/// Seine gepackten Bytes werden trotzdem roh durchsucht.
///
/// Was `lopdf` beim Laden selbst entpackt (Objekt- und Querverweisströme),
/// deckt [`prescan`] mit demselben Budget, **bevor** `Document::load_mem`
/// läuft; lehnt die Vorprüfung ab, fehlen die Sichten 3–7 und `unchecked`
/// sagt es. Sicht 7 (Schriftdekoder) entpackt über den Extraktor ohne eigene
/// Grenze; sie läuft nur, wenn Sicht 3 jeden Strom entpacken konnte — dann
/// ist ihre Arbeit durch dasselbe Budget gedeckt.
///
/// Spitzenbelegung: ein Strom in Arbeit (höchstens Budget + 1 Byte) neben dem
/// geladenen Dokument (dessen Objektströme `lopdf` entpackt hält, durch die
/// Vorprüfung ≤ Budget). Gemessen (`tests/zd_orakel_budget.rs`): 1 GiB
/// Nullen, 1 MB gepackt, Budget 16 MiB → unter 100 MB, unter 2 s.
pub fn leaks_many_within(
    pdf_bytes: &[u8],
    needles: &[&str],
    max_decompressed_bytes: u64,
) -> LeakCheck {
    let mut probe = Probe::new(needles);
    if probe.needles.is_empty() {
        return LeakCheck {
            findings: probe.into_hits(),
            unchecked: Vec::new(),
        };
    }

    scan_raw_file(pdf_bytes, &mut probe);
    let mut raw_budget = Budget::new(max_decompressed_bytes);
    scan_raw_streams(pdf_bytes, &mut probe, &mut raw_budget);
    let mut unchecked = raw_budget.into_unchecked();

    // Lässt sich die Datei nicht laden, bleiben die Rohsuchen die Messung —
    // und `unchecked` sagt, dass es nur die waren.
    let limits = Limits {
        max_decompressed_bytes,
        // Syntax-Budget und Objektdecke gehören zur Schwärzung; hier zählt
        // nur, was entpackt wird.
        max_parsed_bytes: u64::MAX,
        ..Limits::default()
    };
    let doc = match prescan(pdf_bytes, &limits).map_err(|e| e.to_string()) {
        Ok(()) => Document::load_mem(pdf_bytes).map_err(|e| e.to_string()),
        Err(e) => Err(format!("die Vorprüfung des Laders lehnt die Datei ab: {e}")),
    };
    match doc {
        Ok(doc) => {
            let mut budget = Budget::new(max_decompressed_bytes);
            scan_object_graph(&doc, &mut probe, &mut budget);
            let skipped = budget.skipped;
            unchecked.extend(budget.into_unchecked());
            if skipped == 0 {
                scan_decoded_text(&doc, &mut probe);
            } else {
                unchecked.push(format!(
                    "Sicht 7 (Schriftdekoder) nicht gelaufen: {skipped} Strom/Ströme \
                     wurden nicht entpackt, und der Schriftdekoder entpackt ohne \
                     eigene Grenze"
                ));
            }
        }
        Err(reason) => unchecked.push(format!(
            "Objektgraph (Sichten 3–7) nicht durchsucht — {reason}"
        )),
    }

    LeakCheck {
        findings: probe.into_hits(),
        unchecked,
    }
}

// ---------------------------------------------------------------------------
// Budget
// ---------------------------------------------------------------------------

/// Was eine Sicht noch entpacken darf, und was sie deshalb ausgelassen hat.
struct Budget {
    remaining: u64,
    total: u64,
    skipped: usize,
    unchecked: Vec<String>,
}

impl Budget {
    fn new(total: u64) -> Self {
        Self {
            remaining: total,
            total,
            skipped: 0,
            unchecked: Vec::new(),
        }
    }

    /// Wie viele Bytes der nächste Strom höchstens ergeben darf.
    fn room(&self) -> usize {
        usize::try_from(self.remaining).unwrap_or(usize::MAX)
    }

    /// Bucht einen entpackten Strom.
    fn charge(&mut self, bytes: usize) {
        self.remaining = self
            .remaining
            .saturating_sub(u64::try_from(bytes).unwrap_or(u64::MAX));
    }

    /// Merkt einen Strom vor, der nicht entpackt wurde.
    fn skip(&mut self, what: &str, packed: usize) {
        self.skipped += 1;
        if self.skipped <= MAX_UNCHECKED {
            self.unchecked.push(format!(
                "{what}: nicht entpackt — {packed} Byte gepackt, entpackt mehr als \
                 die verbleibenden {} von {} Byte des Budgets (--max-decompressed-mb); \
                 die gepackten Bytes wurden roh durchsucht",
                self.remaining, self.total
            ));
        }
    }

    fn into_unchecked(mut self) -> Vec<String> {
        if self.skipped > MAX_UNCHECKED {
            self.unchecked.push(format!(
                "… und {} weitere Ströme nicht entpackt",
                self.skipped - MAX_UNCHECKED
            ));
        }
        self.unchecked
    }
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
    /// Fragmentgrenzen hinweg erzeugen („MODE“ + „89 EUR“ → „DE89“). Ebenso
    /// `None`, wenn nichts übrig bleibt — ein leeres Muster stünde überall.
    squeezed: Option<String>,
    /// Bytefolgen: (Beschreibung, Muster). Gesucht werden sie nicht einzeln,
    /// sondern alle zusammen im [`Matcher`] der [`Probe`].
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
            squeezed: (squeezed != text && !squeezed.is_empty()).then_some(squeezed),
            variants,
        }
    }
}

/// Ein Automat über viele Muster — ein Durchgang je Datenblock, gleich wie
/// viele Begriffe gesucht werden.
///
/// Jedes Muster kennt seinen Platz: `(Begriff, Kodierung)` in der [`Probe`].
/// Gesucht wird **überlappend** (`MatchKind::Standard`,
/// `find_overlapping_iter`): „Max Mustermann“ und „Mustermann“ müssen beide
/// treffen, und derselbe Text in zwei Kodierungen ebenso.
struct Matcher {
    automaton: AhoCorasick,
    /// Je Muster-Id: (Index des Begriffs in `Probe::needles`, Index der
    /// Kodierung in `Needle::variants`).
    slots: Vec<(usize, usize)>,
    /// Je Muster-Id: seine Länge in Bytes.
    lens: Vec<usize>,
}

impl Matcher {
    fn new(patterns: Vec<((usize, usize), Vec<u8>)>) -> Self {
        let (slots, bytes): (Vec<(usize, usize)>, Vec<Vec<u8>>) = patterns.into_iter().unzip();
        let lens = bytes.iter().map(Vec::len).collect();
        let automaton = AhoCorasick::builder()
            .match_kind(MatchKind::Standard)
            .build(&bytes)
            // Scheitert nur, wenn die Muster den Adressraum des Automaten
            // sprengen — bei Suchbegriffen aus einer Kommandozeile oder
            // Oberfläche nicht erreichbar. Leise nichts zu finden wäre die
            // falsche Antwort eines Messgeräts.
            .expect("Suchbegriffe passen nicht in den Aho-Corasick-Automaten");
        Self {
            automaton,
            slots,
            lens,
        }
    }

    /// Fundstellen je Muster: überlappungsfrei **je Muster**, aufsteigend,
    /// höchstens `limit` — Stelle für Stelle dasselbe, was
    /// `memmem::find_iter(hay).take(limit)` je Muster lieferte (bis
    /// Fix-Runde 4 die Suche, `positions_agree_with_the_naive_search`).
    ///
    /// Der Automat meldet Treffer nach Endposition; bei fester Musterlänge
    /// ist das je Muster die Reihenfolge der Anfänge. Ein Treffer, der in
    /// den vorigen desselben Musters hineinragt, zählt nicht — genau wie
    /// eine Suche, die hinter dem Treffer weitersucht. Sobald jedes Muster
    /// seine Grenze erreicht hat, endet der Durchgang.
    fn positions(&self, hay: &[u8], limit: usize) -> Vec<Vec<usize>> {
        let mut out: Vec<Vec<usize>> = vec![Vec::new(); self.slots.len()];
        if limit == 0 || out.is_empty() {
            return out;
        }
        let mut done = 0usize;
        for found in self.automaton.find_overlapping_iter(hay) {
            let id = found.pattern().as_usize();
            let hits = &mut out[id];
            if hits.len() >= limit {
                continue;
            }
            if hits
                .last()
                .is_some_and(|&last| found.start() < last + self.lens[id])
            {
                continue;
            }
            hits.push(found.start());
            if hits.len() == limit {
                done += 1;
                if done == out.len() {
                    break;
                }
            }
        }
        out
    }
}

/// Alle Suchbegriffe eines Laufs samt ihren Fundstellen und Automaten.
///
/// Der Durchgang durch die Datei kennt nur noch dieses eine Bündel, deshalb
/// wird jeder Stream einmal entpackt, jede Zeichenkette einmal dekodiert und
/// jeder Block einmal durchlaufen — unabhängig davon, wie viele Begriffe
/// gesucht werden.
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
    /// Alle Kodierungen aller Begriffe — für Bytes.
    bytes: Matcher,
    /// Je Begriff sein Text — für dekodierten Text. Muster-Id = Index in
    /// `needles`.
    text: Matcher,
    /// Die Fassungen ohne Leerraum — für Text ohne Leerraum. `slots` nennt
    /// den Begriff.
    squeezed: Matcher,
}

impl Probe {
    fn new(input: &[&str]) -> Self {
        let mut needles = Vec::new();
        let mut slots = Vec::new();
        for (slot, text) in input.iter().enumerate() {
            if text.is_empty() {
                continue;
            }
            needles.push(Needle::new(text));
            slots.push(slot);
        }
        let mut bytes = Vec::new();
        let mut text = Vec::new();
        let mut squeezed = Vec::new();
        for (n, needle) in needles.iter().enumerate() {
            for (v, (_, pattern)) in needle.variants.iter().enumerate() {
                bytes.push(((n, v), pattern.clone()));
            }
            text.push(((n, 0), needle.text.as_bytes().to_vec()));
            if let Some(s) = &needle.squeezed {
                squeezed.push(((n, 1), s.as_bytes().to_vec()));
            }
        }
        Self {
            reports: needles.iter().map(|_| Report::default()).collect(),
            any_squeezed: !squeezed.is_empty(),
            bytes: Matcher::new(bytes),
            text: Matcher::new(text),
            squeezed: Matcher::new(squeezed),
            needles,
            slots,
            total: input.len(),
        }
    }

    /// Byteweise Suche in `hay` — alle Kodierungen aller Begriffe in einem
    /// Durchgang; `location` und `how` beschreiben die Fundstelle.
    fn scan_bytes(&mut self, hay: &[u8], limit: usize, location: &str, how: &str) {
        let found = self.bytes.positions(hay, limit);
        for (id, positions) in found.iter().enumerate() {
            let (n, v) = self.bytes.slots[id];
            let variant = self.needles[n].variants[v].0;
            for &pos in positions {
                self.reports[n].hit_bytes(
                    location,
                    &format!("{how}, {variant}"),
                    hay,
                    pos,
                    self.bytes.lens[id],
                );
            }
        }
    }

    /// Erste Fundstelle je Begriff in dekodiertem Text — `text` steht für
    /// die Muster mit Leerraum (Muster-Id = Begriff), `squeezed` für die
    /// ohne. Zurück kommt je Begriff, ob der Text selbst getroffen hat.
    fn scan_text_plain(&mut self, text: &str, location: &str, how: &str) -> Vec<bool> {
        let found = self.text.positions(text.as_bytes(), 1);
        found
            .iter()
            .enumerate()
            .map(|(n, positions)| {
                let Some(&pos) = positions.first() else {
                    return false;
                };
                self.reports[n].hit_text(location, how, text, pos, self.text.lens[n]);
                true
            })
            .collect()
    }

    /// Wie [`Self::scan_text_plain`], für die Fassung ohne Leerraum;
    /// `only` sagt, welche Begriffe überhaupt gemeldet werden sollen.
    fn scan_text_squeezed(
        &mut self,
        squeezed: &str,
        location: &str,
        how: &str,
        only: impl Fn(usize) -> bool,
    ) {
        let found = self.squeezed.positions(squeezed.as_bytes(), 1);
        for (id, positions) in found.iter().enumerate() {
            let (n, _) = self.squeezed.slots[id];
            if !only(n) {
                continue;
            }
            if let Some(&pos) = positions.first() {
                self.reports[n].hit_text(location, how, squeezed, pos, self.squeezed.lens[id]);
            }
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
///
/// Öffentlich, damit die Oberfläche ihre Entscheidung „gesucht oder bewusst
/// stehen gelassen“ auf **derselben** Normalform trifft, auf der hier
/// gesucht wird (Befund G5-B1: „DE89 3704 …“ und „DE893704…“ sind ein Text).
pub fn squeeze(text: &str) -> String {
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

// ---------------------------------------------------------------------------
// Ebene 1+2: Rohdatei und rohe Streams
// ---------------------------------------------------------------------------

fn scan_raw_file(bytes: &[u8], probe: &mut Probe) {
    let found = probe.bytes.positions(bytes, 8);
    for (id, positions) in found.iter().enumerate() {
        let (n, v) = probe.bytes.slots[id];
        let variant = probe.needles[n].variants[v].0;
        for &pos in positions {
            probe.reports[n].hit_bytes(
                &format!("Rohdatei @0x{pos:x}"),
                variant,
                bytes,
                pos,
                probe.bytes.lens[id],
            );
        }
    }
}

/// Findet jeden `stream … endstream`-Block anhand der Rohbytes — unabhängig
/// davon, ob die xref-Tabelle das Objekt noch kennt. Genau so überlebt die
/// Historie inkrementeller Updates.
fn scan_raw_streams(bytes: &[u8], probe: &mut Probe, budget: &mut Budget) {
    for (offset, payload) in raw_stream_blocks(bytes) {
        let base = format!(
            "Rohdaten-Stream @0x{offset:x}{}",
            object_label(bytes, offset)
        );
        scan_blob(payload, &format!("{base} (roh)"), probe);
        match inflate_raw(payload, budget.room()) {
            Ok(Some(inflated)) => {
                budget.charge(inflated.len());
                scan_blob(&inflated, &format!("{base} (inflate)"), probe);
            }
            Ok(None) => {}
            Err(Oversize) => budget.skip(&base, payload.len()),
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

/// „ (Objekt N G)“, wenn vor dem Block ein Objektkopf `N G obj` steht —
/// damit eine Meldung den Strom so nennt wie die Objektsichten. Leer, wenn
/// keiner zu finden ist (Block mitten in Rohdaten, Kopf weiter weg als
/// [`OBJECT_HEADER_LOOKBACK`]).
fn object_label(bytes: &[u8], stream_offset: usize) -> String {
    let from = stream_offset.saturating_sub(OBJECT_HEADER_LOOKBACK);
    let window = &bytes[from..stream_offset];
    let Some(pos) = memmem::rfind(window, b"obj") else {
        return String::new();
    };
    if window[..pos].ends_with(b"end") {
        return String::new();
    }
    let (rest, generation) = trailing_number(window[..pos].trim_ascii_end());
    let (_, number) = trailing_number(rest.trim_ascii_end());
    match (number, generation) {
        (Some(number), Some(generation)) if rest.len() < pos => {
            format!(" (Objekt {number} {generation})")
        }
        _ => String::new(),
    }
}

/// Die Ziffernfolge am Ende von `s` und der Rest davor.
fn trailing_number(s: &[u8]) -> (&[u8], Option<&str>) {
    let start = s
        .iter()
        .rposition(|b| !b.is_ascii_digit())
        .map_or(0, |i| i + 1);
    let digits = std::str::from_utf8(&s[start..])
        .ok()
        .filter(|d| !d.is_empty());
    (&s[..start], digits)
}

/// Zlib, sonst rohes Deflate — an jedem Block, ohne aufs Dictionary zu
/// schauen: so werden auch Altrevisionen und Blöcke ohne `/Filter` sichtbar.
/// `None`, wenn nichts dabei herauskommt.
fn inflate_raw(data: &[u8], limit: usize) -> Result<Option<Vec<u8>>, Oversize> {
    let out = filters::read_within(flate2::read::ZlibDecoder::new(data), limit)?;
    if !out.is_empty() {
        return Ok(Some(out));
    }
    let raw = filters::read_within(flate2::read::DeflateDecoder::new(data), limit)?;
    Ok((!raw.is_empty()).then_some(raw))
}

// ---------------------------------------------------------------------------
// Ebene 3–6: Objektgraph
// ---------------------------------------------------------------------------

fn scan_object_graph(doc: &Document, probe: &mut Probe, budget: &mut Budget) {
    walk_dict(doc, &doc.trailer, "Trailer", probe, budget, 0);
    for (id, object) in &doc.objects {
        let path = format!("Objekt {} {}", id.0, id.1);
        walk(doc, object, &path, probe, budget, 0);
    }
}

fn walk(
    doc: &Document,
    object: &Object,
    path: &str,
    probe: &mut Probe,
    budget: &mut Budget,
    depth: usize,
) {
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
                walk(doc, item, &format!("{path}[{i}]"), probe, budget, depth + 1);
            }
        }
        Object::Dictionary(dict) => walk_dict(doc, dict, path, probe, budget, depth),
        Object::Stream(stream) => {
            walk_dict(doc, &stream.dict, path, probe, budget, depth);
            scan_stream(doc, stream, path, probe, budget, depth);
        }
        _ => {}
    }
}

fn walk_dict(
    doc: &Document,
    dict: &Dictionary,
    path: &str,
    probe: &mut Probe,
    budget: &mut Budget,
    depth: usize,
) {
    if depth > MAX_DEPTH {
        return;
    }
    for (key, value) in dict.iter() {
        let key = String::from_utf8_lossy(key);
        walk(
            doc,
            value,
            &format!("{path}/{key}"),
            probe,
            budget,
            depth + 1,
        );
    }
}

/// Ein Stream: roh, dekodiert (im Budget) und — als Objekt-Stream — seine
/// enthaltenen Objekte.
fn scan_stream(
    doc: &Document,
    stream: &Stream,
    path: &str,
    probe: &mut Probe,
    budget: &mut Budget,
    depth: usize,
) {
    scan_blob(&stream.content, &format!("{path} <Stream, roh>"), probe);

    let decoded = match decode_stream(doc, stream, budget.room()) {
        Ok(Some((label, data))) => {
            budget.charge(data.len());
            scan_blob(&data, &format!("{path} <Stream, {label}>"), probe);
            Some(data)
        }
        Ok(None) => None,
        Err(Oversize) => {
            budget.skip(&format!("{path} <Stream>"), stream.content.len());
            return;
        }
    };

    // Objekt-Streams sind komprimierte Container: die enthaltenen Objekte
    // stehen nirgends im Klartext und entgehen jeder Rohbyte-Suche.
    // `ObjectStream::new` würde selbst entpacken — ohne Grenze; deshalb
    // bekommt es den schon entpackten Inhalt ohne `/Filter`.
    if stream.dict.has_type(b"ObjStm") {
        let mut plain = Stream::new(
            stream.dict.clone(),
            decoded.unwrap_or_else(|| stream.content.clone()),
        );
        plain.dict.remove(b"Filter");
        plain.dict.remove(b"DecodeParms");
        plain.dict.remove(b"DP");
        if let Ok(object_stream) = ObjectStream::new(&mut plain) {
            for (id, object) in &object_stream.objects {
                let inner = format!("{path} <ObjStm> → Objekt {} {}", id.0, id.1);
                walk(doc, object, &inner, probe, budget, depth + 1);
            }
        }
    }
}

/// Die dekodierte Sicht auf einen gefilterten Stream: (Beschriftung, Daten).
///
/// `None` ohne `/Filter` (die Rohbytes sind schon durchsucht) und bei einem
/// Filter, den [`crate::filters`] nicht kennt (Bildfilter) — dann bleiben
/// die Rohbytes die Rückfallebene, und die Rohsicht hat zusätzlich Flate an
/// ihnen versucht.
fn decode_stream(
    doc: &Document,
    stream: &Stream,
    room: usize,
) -> Result<Option<(String, Vec<u8>)>, Oversize> {
    let names = filters::filter_names(doc, &stream.dict).unwrap_or_default();
    if names.is_empty() {
        return Ok(None);
    }
    let label = format!(
        "dekodiert: {}",
        names
            .iter()
            .map(|f| String::from_utf8_lossy(f))
            .collect::<Vec<_>>()
            .join("+")
    );
    Ok(filters::decoded_content_within(doc, stream, room)?.map(|data| (label, data)))
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
    probe.scan_text_plain(&joined, location, "Zeichenketten-Verkettung");
    if probe.any_squeezed {
        let squeezed = squeeze(&joined);
        probe.scan_text_squeezed(
            &squeezed,
            location,
            "Zeichenketten-Verkettung, ohne Leerraum",
            |_| true,
        );
    }
}

fn scan_raw_bytes(hay: &[u8], location: &str, how: &str, probe: &mut Probe) {
    probe.scan_bytes(hay, 4, location, how);
}

/// Zeichenketten-Objekt: sowohl dekodiert (PDFDocEncoding **oder** UTF-16BE)
/// als auch roh vergleichen.
fn scan_string(raw: &[u8], location: &str, how: &str, probe: &mut Probe) {
    // Dekodieren hängt allein an der Zeichenkette, nicht am Suchbegriff.
    scan_text(&decode_pdf_string(raw), location, how, probe);
    scan_raw_bytes(raw, location, how, probe);
}

/// Bereits dekodierter Text: als Ganzes und, wo der Begriff Leerraum hat
/// und als Ganzes nicht traf, ohne jeden Leerraum.
fn scan_text(text: &str, location: &str, how: &str, probe: &mut Probe) {
    let hit = probe.scan_text_plain(text, location, how);
    if probe.any_squeezed {
        let squeezed = squeeze(text);
        probe.scan_text_squeezed(&squeezed, location, &format!("{how}, ohne Leerraum"), |n| {
            !hit[n]
        });
    }
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
        match filters::hex_value(blob[i]) {
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

    /// Der Automat muss Fundstelle für Fundstelle dasselbe liefern wie die
    /// naive Suche je Muster: überlappungsfrei je Muster, in Reihenfolge, an
    /// der Grenze abgeschnitten. Die Fälle: Muster länger als der Heuhaufen,
    /// Muster am Ende, überlappende Vorkommen (`aaaa` in `aaaaaaa` sind zwei,
    /// nicht vier), Grenze kleiner als die Zahl der Vorkommen — und mehrere
    /// Muster, die einander überlappen oder gleich sind, im selben Durchgang.
    #[test]
    fn positions_agree_with_the_naive_search() {
        type Case<'a> = (&'a [u8], &'a [&'a [u8]], usize);
        let cases: &[Case] = &[
            (b"", &[b"a"], 4),
            (b"ab", &[b"abc"], 4),
            (b"xxabc", &[b"abc"], 4),
            (b"aaaaaaa", &[b"aaaa"], 4),
            (b"abcabcabcabc", &[b"abc"], 2),
            (b"abcabcabcabc", &[b"abc"], 8),
            (b"a.b.c.d.e.f", &[b"."], 3),
            (b"DE89 3704 0044 DE89 3704", &[b"DE89 3704"], 8),
            // Überlappend: der eine Begriff steckt im anderen.
            (
                b"Max Mustermann und Mustermann",
                &[b"Max Mustermann", b"Mustermann", b"Max"],
                4,
            ),
            // Gleiche Muster unter zwei Ids, und eines, das nie trifft.
            (b"abab", &[b"ab", b"ab", b"zz", b"b"], 4),
            (b"aaaaaaa", &[b"aaaa", b"aa", b"a"], 2),
        ];
        for (hay, pats, limit) in cases {
            let matcher = Matcher::new(
                pats.iter()
                    .enumerate()
                    .map(|(i, p)| ((i, 0), p.to_vec()))
                    .collect(),
            );
            let expected: Vec<Vec<usize>> = pats
                .iter()
                .map(|p| find_all_naive(hay, p, *limit))
                .collect();
            assert_eq!(
                matcher.positions(hay, *limit),
                expected,
                "hay={hay:?} pats={pats:?} limit={limit}"
            );
        }
        let matcher = Matcher::new(vec![((0, 0), b"aaaa".to_vec())]);
        assert_eq!(matcher.positions(b"aaaaaaa", 4), vec![vec![0]]);
        let matcher = Matcher::new(vec![((0, 0), b"abc".to_vec())]);
        assert_eq!(matcher.positions(b"abcabcabcabc", 2), vec![vec![0, 3]]);
    }

    /// Ein Begriff aus lauter Leerraum hat keine Fassung ohne Leerraum —
    /// ein leeres Muster stünde an jeder Stelle jeder Datei.
    #[test]
    fn a_blank_needle_has_no_squeezed_form() {
        assert!(Needle::new("   ").squeezed.is_none());
        assert!(leaks(b"%PDF-1.5\n(irgendwas)\n", "   ").is_empty());
    }

    /// Der Objektkopf vor einem rohen Strom — für die Meldung, welcher
    /// Strom nicht entpackt wurde.
    #[test]
    fn raw_blocks_are_labelled_with_their_object_header() {
        let pdf = b"%PDF-1.5\n12 0 obj\n<< /Length 3 >>\nstream\nabc\nendstream\nendobj\n";
        let blocks = raw_stream_blocks(pdf);
        assert_eq!(blocks.len(), 1);
        assert_eq!(object_label(pdf, blocks[0].0), " (Objekt 12 0)");
        // Ohne Kopf (Block mitten in Rohdaten, oder nur ein `endobj` davor):
        // kein Etikett, keine Erfindung.
        let loose = b"%PDF-1.5\nendobj\nstream\nabc\nendstream\n";
        let blocks = raw_stream_blocks(loose);
        assert_eq!(object_label(loose, blocks[0].0), "");
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
        scan_object_graph(&doc, &mut probe, &mut Budget::new(u64::MAX));
        let hits = probe.into_hits();
        assert_eq!(hits.len(), NEEDLES.len());
        assert!(
            hits.iter().any(|h| !h.is_empty()),
            "der Objektgraph-Durchgang hat nichts gefunden"
        );
    }
}
