//! Gegenprüfung R1 (Fix-Runde 6): die Decke `MAX_MIRROR_FORM_PLACEMENTS` —
//! ihre Grenze in beide Richtungen, ihre Einheit, ihre Summe über ein
//! Dokument.
//!
//! Die Decke zählt **Spiegel × `Do`** (Aufbau) und Aufklappungen, je
//! Seiten-Scan. Drei Fragen:
//!
//! 1. Warnt sie nur, wenn wirklich etwas weggelassen wurde — genau N still,
//!    N+1 laut, für Aufbau und Aufklappen getrennt?
//! 2. Was bleibt ungezählt? `scan_marked_text` schneidet neben den `Do` auch
//!    die **Textoperationen** je Klammer heraus (`shows`), mit derselben
//!    Produktstruktur `Klammern × Tj` — und ohne Decke.
//! 3. Was kostet die Summe? Die Decke gilt je Seite; ein Dokument aus `P`
//!    Seiten, die sich **einen** Content-Stream teilen, führt `P × 100 000`
//!    Paare — und `redact::apply_with_report` hält die Spiegel jeder Seite
//!    über einem Formular bis zum Ende fest (`PendingPage::deferred`).
//!
//! Die Messungen (`#[ignore = "Messung"]`) laufen je in eigenem Prozess:
//! `R1_PAGES=… cargo test -p redact-pdf --test zg_r1_decke -- --ignored
//! --nocapture mess_…`.

use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Redaction, Region, Source, TextRun};
use redact_pdf::{load_from_bytes, scan_page, PdfExtractor, PdfRedactor, ScanResult};

/// `content::MAX_MIRROR_FORM_PLACEMENTS`.
const DECKE: usize = 100_000;

/// Ein Dokument mit `pages` Seiten, die sich Ressourcen und Content-Stream
/// teilen — jede Seite ein eigenes Objekt, der Inhalt einmal in der Datei.
struct Multi {
    doc: Document,
    resources_id: ObjectId,
    content_id: ObjectId,
    font_id: ObjectId,
    page_ids: Vec<ObjectId>,
}

impl Multi {
    fn new(pages: usize) -> Self {
        let mut doc = Document::with_version("1.5");
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let content_id = doc.add_object(Stream::new(dictionary! {}, Vec::new()));
        let pages_id = doc.new_object_id();
        let page_ids: Vec<ObjectId> = (0..pages)
            .map(|_| {
                doc.add_object(dictionary! {
                    "Type" => "Page",
                    "Parent" => pages_id,
                    "Contents" => content_id,
                    "Resources" => resources_id,
                    "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                })
            })
            .collect();
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => page_ids.iter().map(|id| Object::Reference(*id)).collect::<Vec<_>>(),
                "Count" => pages as i64,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        Self {
            doc,
            resources_id,
            content_id,
            font_id,
            page_ids,
        }
    }

    fn set_content(&mut self, raw: &[u8]) {
        self.doc.objects.insert(
            self.content_id,
            Object::Stream(Stream::new(dictionary! {}, raw.to_vec())),
        );
    }

    /// Gibt Seite `index` einen eigenen Content-Stream.
    fn own_content(&mut self, index: usize, raw: &[u8]) {
        let id = self
            .doc
            .add_object(Stream::new(dictionary! {}, raw.to_vec()).with_compression(false));
        self.doc
            .get_dictionary_mut(self.page_ids[index])
            .expect("Seite")
            .set("Contents", id);
    }

    fn link(&mut self, holder: ObjectId, name: &str, target: ObjectId) {
        let holder = self.doc.get_dictionary_mut(holder).expect("Ressourcen");
        let mut xobjects = holder
            .get(b"XObject")
            .and_then(|o| o.as_dict())
            .cloned()
            .unwrap_or_default();
        xobjects.set(name, target);
        holder.set("XObject", xobjects);
    }

    /// Ein Form-XObject mit eigenem Ressourcenobjekt. Liefert (Formular, Ressourcen).
    fn add_form(&mut self, holder: ObjectId, name: &str, body: &str) -> (ObjectId, ObjectId) {
        let font_id = self.font_id;
        let form_resources = self.doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let form_id = self.doc.add_object(
            Stream::new(
                dictionary! {
                    "Type" => "XObject",
                    "Subtype" => "Form",
                    "BBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
                    "Resources" => form_resources,
                },
                body.as_bytes().to_vec(),
            )
            .with_compression(false),
        );
        self.link(holder, name, form_id);
        (form_id, form_resources)
    }

    fn finish(&self) -> Vec<u8> {
        let mut buffer = Vec::new();
        self.doc.clone().save_to(&mut buffer).expect("speicherbar");
        buffer
    }
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

fn text_at(y: i32, text: &str) -> String {
    format!("BT /F1 10 Tf 72 {y} Td ({}) Tj ET\n", escape(text))
}

fn hwm() -> String {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("VmHWM"))
        .unwrap_or("VmHWM: ?")
        .trim()
        .to_string()
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn scan_first(bytes: &[u8]) -> ScanResult {
    let doc = load_from_bytes(bytes).expect("PDF ladbar");
    let page_id = doc.page_iter().next().expect("eine Seite");
    scan_page(&doc, page_id).expect("Scan")
}

fn paare(scan: &ScanResult) -> usize {
    scan.marked.iter().map(|record| record.forms.len()).sum()
}

fn textzuordnungen(scan: &ScanResult) -> usize {
    scan.marked.iter().map(|record| record.shows.len()).sum()
}

fn decken_warnungen(warnings: &[String]) -> Vec<&String> {
    warnings
        .iter()
        .filter(|w| w.contains("Zuordnungen zwischen einem Spiegel"))
        .collect()
}

/// Nur die Decke über `Spiegel × Formularplatzierung`.
fn formular_warnungen(warnings: &[String]) -> Vec<&String> {
    decken_warnungen(warnings)
        .into_iter()
        .filter(|w| w.contains("Formularplatzierung"))
        .collect()
}

/// Nur die Decke über `Spiegel × Textoperation`.
fn text_warnungen(warnings: &[String]) -> Vec<&String> {
    decken_warnungen(warnings)
        .into_iter()
        .filter(|w| w.contains("Textoperation"))
        .collect()
}

/// Die dokumentweite Decke des Redaktors (`MAX_DEFERRED_MIRRORS`).
fn zurueckgestellt_warnungen(warnings: &[String]) -> Vec<&String> {
    warnings
        .iter()
        .filter(|w| w.contains("Textspiegel über Form-XObjects zurück"))
        .collect()
}

// ---------------------------------------------------------------------------
// Material
// ---------------------------------------------------------------------------

/// `b` verschachtelte Spiegel-Klammern über `d` Platzierungen von `Fm1`
/// (ein Blatt mit einer Glyphe): Aufbau `b × d` Paare, keine Aufklappung.
///
/// `ehrlich`: der Spiegel nennt genau die Glyphen darunter (`d` mal `A`),
/// damit die Datei **keine** Spiegelwarnung auslöst und die Kosten wirklich
/// still anfallen.
fn klammern_ueber_platzierungen(pages: usize, b: usize, d: usize, ehrlich: bool) -> Vec<u8> {
    let mut m = Multi::new(pages);
    let res = m.resources_id;
    m.add_form(res, "Fm1", &text_at(600, "A"));
    let wahr = "A".repeat(d);
    let mut raw = text_at(700, "Kontoinhaber Max Mustermann");
    for i in 0..b {
        if ehrlich {
            raw.push_str(&format!("/Span <</ActualText ({wahr})>> BDC\n"));
        } else {
            raw.push_str(&format!("/Span <</ActualText (A{i})>> BDC\n"));
        }
    }
    raw.push_str(&"/Fm1 Do\n".repeat(d));
    for _ in 0..b {
        raw.push_str("EMC\n");
    }
    m.set_content(raw.as_bytes());
    m.finish()
}

/// **Ein** Spiegel über `outer` Platzierungen von `Fm0`; `Fm0` zeichnet `Fm1`
/// (ein Blatt) `fanout`-mal. Aufbau: `outer` Paare; Aufklappen: `outer ×
/// fanout` Kanten.
fn aufklappungen(outer: usize, fanout: usize) -> Vec<u8> {
    let mut m = Multi::new(1);
    let res = m.resources_id;
    let (inner, _) = m.add_form(res, "Fm1", &text_at(600, "A"));
    let (_, outer_res) = m.add_form(res, "Fm0", &"/Fm1 Do\n".repeat(fanout));
    m.link(outer_res, "Fm1", inner);
    let mut raw = text_at(700, "Kontoinhaber Max Mustermann");
    raw.push_str("/Span <</ActualText (Alpha)>> BDC\n");
    raw.push_str(&"/Fm0 Do\n".repeat(outer));
    raw.push_str("EMC\n");
    m.set_content(raw.as_bytes());
    m.finish()
}

/// `b` verschachtelte Spiegel-Klammern über `s` Textoperationen — und
/// wahlweise **einem** `Do`, damit der Datensatz als Spiegel über einem
/// Formular gilt und `redact` ihn je Seite bis zum Ende festhält.
fn klammern_ueber_text(pages: usize, b: usize, s: usize, mit_do: bool) -> Vec<u8> {
    let mut m = Multi::new(pages);
    let res = m.resources_id;
    if mit_do {
        m.add_form(res, "Fm1", &text_at(600, "A"));
    }
    let mut raw = String::new();
    for i in 0..b {
        raw.push_str(&format!("/Span <</ActualText (A{i})>> BDC\n"));
    }
    raw.push_str("BT /F1 10 Tf 12 TL 72 700 Td\n");
    for _ in 0..s {
        raw.push_str("(A) '\n");
    }
    raw.push_str("ET\n");
    if mit_do {
        raw.push_str("/Fm1 Do\n");
    }
    for _ in 0..b {
        raw.push_str("EMC\n");
    }
    m.set_content(raw.as_bytes());
    m.finish()
}

/// Ein Word-artiges getaggtes Dokument: je Seite ein Logo-Formular unter
/// `/Figure <</Alt …>>`, vierzig Absätze `/P <</MCID n>>`, darin
/// Ligatur-Spiegel `/Span <</ActualText (fi)>>`, eine Tabelle mit
/// `/TD`-Zellen, ein Fußzeilen-Formular als `/Artifact`. Jede Seite hat
/// ihren eigenen Strom.
fn word_artig(pages: usize) -> Vec<u8> {
    let mut m = Multi::new(pages);
    let res = m.resources_id;
    // Logo: Zeichnung ohne Text.
    m.add_form(res, "Fm0", "0 0 1 rg 0 0 100 30 re f\n");
    // Kopfzeile mit Text.
    m.add_form(res, "Fm1", &text_at(0, "Musterbank AG - Kontoauszug"));
    for index in 0..pages {
        let mut raw = String::new();
        raw.push_str("/Figure <</Alt (Firmenlogo)>> BDC\nq 1 0 0 1 72 790 cm /Fm0 Do Q\nEMC\n");
        raw.push_str("/Artifact <</Type /Pagination>> BDC\nq 1 0 0 1 300 800 cm /Fm1 Do Q\nEMC\n");
        let mut y = 760;
        for n in 0..40 {
            raw.push_str(&format!("/P <</MCID {n}>> BDC\nBT /F1 10 Tf 72 {y} Td\n"));
            if n % 5 == 0 {
                raw.push_str(
                    "(Auszug ) Tj /Span <</ActualText (fi)>> BDC (fi) Tj EMC (nanzieren Sie ) Tj\n",
                );
            }
            raw.push_str(&format!(
                "(Buchung {n} auf Seite {} mit Betrag 12,50 EUR) Tj\nET\nEMC\n",
                index + 1
            ));
            y -= 12;
        }
        for cell in 0..20 {
            let x = 72 + (cell % 4) * 120;
            let yy = 200 - (cell / 4) * 14;
            raw.push_str(&format!(
                "/TD <</MCID {}>> BDC\nBT /F1 9 Tf {x} {yy} Td (Zelle {cell}) Tj ET\nEMC\n",
                40 + cell
            ));
        }
        m.own_content(index, raw.as_bytes());
    }
    m.finish()
}

// ---------------------------------------------------------------------------
// 1. Die Grenze, in beide Richtungen, je Zähler
// ---------------------------------------------------------------------------

/// Aufbau: 2 Klammern × 50 000 `Do` = genau 100 000 Paare — nichts fällt weg.
#[test]
fn aufbau_genau_an_der_decke_bleibt_still() {
    let scan = scan_first(&klammern_ueber_platzierungen(1, 2, 50_000, false));
    assert_eq!(paare(&scan), DECKE);
    let hits = decken_warnungen(&scan.warnings);
    assert!(hits.is_empty(), "nichts ging verloren, trotzdem: {hits:?}");
}

/// Aufbau: 2 × 50 001 = 100 002 — zwei Paare fallen weg, und das steht da.
#[test]
fn aufbau_eine_zuordnung_ueber_der_decke_sagt_es() {
    let scan = scan_first(&klammern_ueber_platzierungen(1, 2, 50_001, false));
    assert_eq!(paare(&scan), DECKE);
    let hits = decken_warnungen(&scan.warnings);
    assert_eq!(hits.len(), 1, "{:?}", scan.warnings);
}

/// Aufklappen: 50 000 Platzierungen × 2 Kinder = genau 100 000 Kanten; der
/// Aufbau liegt mit 50 000 darunter. Nichts fällt weg.
#[test]
fn aufklappen_genau_an_der_decke_bleibt_still() {
    let scan = scan_first(&aufklappungen(50_000, 2));
    assert_eq!(paare(&scan), 50_000 + DECKE);
    let hits = decken_warnungen(&scan.warnings);
    assert!(hits.is_empty(), "nichts ging verloren, trotzdem: {hits:?}");
}

/// Aufklappen: 50 001 × 2 = 100 002 Kanten — zwei fallen weg, und das steht da.
#[test]
fn aufklappen_eine_ueber_der_decke_sagt_es() {
    let scan = scan_first(&aufklappungen(50_001, 2));
    assert_eq!(paare(&scan), 50_001 + DECKE);
    let hits = decken_warnungen(&scan.warnings);
    assert_eq!(hits.len(), 1, "{:?}", scan.warnings);
}

/// **Befund R1-4, erste Hälfte: die Decke des Seiten-Scans bleibt je Seite.**
/// Sie gilt je Seiten-Scan, und die alte Begründung im Kommentar („die Kosten
/// bleiben gedeckelt, weil jede Seite ihren eigenen Inhalt mitbringen muss“)
/// stimmte nicht: `/Contents` darf auf **denselben** Strom zeigen wie die
/// Nachbarseite. Zwei Seiten, ein Strom von 9 kB, und jede zahlt die volle
/// Decke — das ist hier festgehalten und bleibt so: `scan_page` ist öffentlich
/// und seitenweise, und eine Warnung über *diese* Seite darf nicht davon
/// abhängen, was auf der Nachbarseite steht.
///
/// Was daraus wurde, entschied der **Redaktor**: er hielt die Spiegel jeder
/// Seite mit allen Paaren bis zum Ende der Formularschleife fest. Gemessen mit
/// 1 000 Seiten (Datei 224 752 B, Spiegel deckungsgleich, also ohne jede
/// Warnung; `mess_seiten_mal_paare`, `R1_PAGES=1000 R1_B=100 R1_D=999`,
/// Debug, eigener Prozess): Redaktor 105 s / **6 439 MB**. Seit Fix-Runde 7
/// hält `redact::DeferredMirror` nur noch die Objekt-Ids der Formulare und die
/// fertige Antwort: 63 s / **37 MB**. Die zweite Hälfte — die dokumentweite
/// Decke darüber — steht in
/// [`genau_an_der_dokumentweiten_decke_bleibt_es_still`].
#[test]
fn jede_seite_zahlt_die_volle_decke_aus_einem_geteilten_strom() {
    let bytes = klammern_ueber_platzierungen(2, 100, 1_000, false);
    assert!(bytes.len() < 30_000, "Datei {} B", bytes.len());
    let doc = load_from_bytes(&bytes).expect("PDF ladbar");
    let mut summe = 0;
    for page_id in doc.page_iter() {
        let scan = scan_page(&doc, page_id).expect("Scan");
        assert_eq!(paare(&scan), DECKE, "je Seite die volle Decke");
        assert!(
            decken_warnungen(&scan.warnings).is_empty(),
            "{:?}",
            scan.warnings
        );
        summe += paare(&scan);
    }
    assert_eq!(summe, 2 * DECKE, "{summe}");
}

// ---------------------------------------------------------------------------
// 2. Die Einheit: `Klammern × Tj` zählt nicht
// ---------------------------------------------------------------------------

/// **Befund R1-3 (Sollfassung).** 400 verschachtelte Klammern über 400
/// Textoperationen bieten 160 000 Textzuordnungen an — dieselbe
/// Produktstruktur wie `Klammern × Do`, nur die andere Liste. Sie steht jetzt
/// unter derselben Decke, und dass etwas wegfiel, steht da.
///
/// Vorher: 160 000 Zuordnungen, keine Warnung. Gemessen an 6 000 × 6 000 aus
/// 263 kB (Debug, eigener Prozess): `scan_page` 0,72 s / 315 MB, Extraktor
/// **31,1 s** / 316 MB, Redaktor 588 MB. Nachher: 0,45 s / 35 MB, 0,55 s /
/// 36 MB, 36 MB.
#[test]
fn klammern_mal_textoperationen_stehen_unter_derselben_decke() {
    let scan = scan_first(&klammern_ueber_text(1, 400, 400, false));
    let zuordnungen = textzuordnungen(&scan);
    assert_eq!(zuordnungen, DECKE, "{zuordnungen}");
    let hits = text_warnungen(&scan.warnings);
    assert_eq!(hits.len(), 1, "{:?}", scan.warnings);
    assert!(
        formular_warnungen(&scan.warnings).is_empty(),
        "die Formulardecke hat nichts verloren: {:?}",
        scan.warnings
    );
}

/// Text: 2 Klammern × 50 000 Textoperationen = genau 100 000 — nichts fällt
/// weg, also kein Wort. Dieselbe Prüfung wie oben für die Formulardecke: eine
/// Decke, die an der aufgehenden Zahl warnt, ist ein falscher Alarm.
#[test]
fn text_genau_an_der_decke_bleibt_still() {
    let scan = scan_first(&klammern_ueber_text(1, 2, 50_000, false));
    assert_eq!(textzuordnungen(&scan), DECKE);
    let hits = decken_warnungen(&scan.warnings);
    assert!(hits.is_empty(), "nichts ging verloren, trotzdem: {hits:?}");
}

/// Text: 2 × 50 001 = 100 002 — zwei Zuordnungen fallen weg, und das steht da.
#[test]
fn text_eine_zuordnung_ueber_der_decke_sagt_es() {
    let scan = scan_first(&klammern_ueber_text(1, 2, 50_001, false));
    assert_eq!(textzuordnungen(&scan), DECKE);
    assert_eq!(
        text_warnungen(&scan.warnings).len(),
        1,
        "{:?}",
        scan.warnings
    );
}

/// Die beiden Decken zehren nicht voneinander: dieselbe Seite trägt 100 000
/// Textzuordnungen **und** 100 000 Spiegel-Formular-Paare, ohne dass eine der
/// beiden die andere aufbraucht.
#[test]
fn text_und_formulare_haben_getrennte_konten() {
    let mut m = Multi::new(1);
    let res = m.resources_id;
    m.add_form(res, "Fm1", &text_at(600, "A"));
    let mut raw = String::new();
    for i in 0..2 {
        raw.push_str(&format!("/Span <</ActualText (A{i})>> BDC\n"));
    }
    raw.push_str("BT /F1 10 Tf 12 TL 72 700 Td\n");
    raw.push_str(&"(A) '\n".repeat(50_000));
    raw.push_str("ET\n");
    raw.push_str(&"/Fm1 Do\n".repeat(50_000));
    raw.push_str("EMC\nEMC\n");
    m.set_content(raw.as_bytes());
    let scan = scan_first(&m.finish());
    assert_eq!(textzuordnungen(&scan), DECKE, "Textkonto");
    assert_eq!(paare(&scan), DECKE, "Formularkonto");
    let hits = decken_warnungen(&scan.warnings);
    assert!(hits.is_empty(), "nichts ging verloren, trotzdem: {hits:?}");
}

// ---------------------------------------------------------------------------
// 2b. Die dokumentweite Decke des Redaktors
// ---------------------------------------------------------------------------

/// `redact::MAX_DEFERRED_MIRRORS`.
const DOKUMENTDECKE: usize = 100_000;

/// Schwärzt nichts und liefert die Warnungen des Redaktors.
fn redaktor_warnungen(bytes: &[u8]) -> Vec<String> {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfRedactor::new()
        .apply_with_report(&mut doc, &[])
        .expect("Schwärzung")
        .warnings
}

/// **Befund R1-4, zweite Hälfte.** Was der Redaktor zwischen Seiten- und
/// Formularschleife festhält, ist dokumentweit und braucht eine dokumentweite
/// Decke. Genau `MAX_DEFERRED_MIRRORS` zurückgestellte Abschnitte
/// (1 000 Seiten × 100 Spiegel über je einer Platzierung) gehen auf — und
/// bleiben still. Eine Decke, die an der aufgehenden Zahl warnt, ist derselbe
/// falsche Alarm wie in Befund Q3-1b.
#[test]
fn genau_an_der_dokumentweiten_decke_bleibt_es_still() {
    let bytes = klammern_ueber_platzierungen(DOKUMENTDECKE / 100, 100, 1, true);
    let warnings = redaktor_warnungen(&bytes);
    let hits = zurueckgestellt_warnungen(&warnings);
    assert!(hits.is_empty(), "nichts ging verloren, trotzdem: {hits:?}");
}

/// Einer mehr — und der Redaktor sagt, wie viele Abschnitte er nicht mehr
/// mitgeführt hat. Ohne diese Decke wuchs der Speicher linear mit
/// Seitenzahl × Spiegeln, aus einer Datei, die dafür nichts mitbringen muss.
#[test]
fn ueber_der_dokumentweiten_decke_sagt_der_redaktor_es_an() {
    let bytes = klammern_ueber_platzierungen(DOKUMENTDECKE / 100 + 1, 100, 1, true);
    let warnings = redaktor_warnungen(&bytes);
    let hits = zurueckgestellt_warnungen(&warnings);
    assert_eq!(hits.len(), 1, "{warnings:?}");
    assert!(hits[0].contains("100 davon"), "{}", hits[0]);
}

/// Die Gegenrichtung zur Sparsamkeit: ein Spiegel im Seitenstrom über einem
/// Formular, das erst eine **spätere** Seite trifft, wird weiterhin geleert
/// (Befund G1-A2). `DeferredMirror` hält dafür nur noch die Objekt-Id des
/// Formulars und die fertige Antwort — nicht mehr die Eigenschaftsliste.
#[test]
fn ein_spiegel_ueber_einem_spaeter_geschwaerzten_formular_faellt_weiterhin() {
    const GEHEIM: &str = "DE89 3704 0044 0532 0130 00";
    let mut m = Multi::new(2);
    let res = m.resources_id;
    m.add_form(res, "Fm0", &text_at(600, GEHEIM));
    // Seite 1: der Spiegel über der Platzierung, mehrfach gezeichnet.
    let mut erste = String::from("/Span <</ActualText (Zahlung an DE89)>> BDC\n");
    erste.push_str(&"/Fm0 Do\n".repeat(50));
    erste.push_str("EMC\n");
    m.own_content(0, erste.as_bytes());
    // Seite 2: dasselbe Formular, ohne Spiegel — hier wird geschwärzt.
    m.own_content(1, b"/Fm0 Do\n");
    let bytes = m.finish();

    let doc = load_from_bytes(&bytes).expect("PDF ladbar");
    let (runs, _) = PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("Extraktion");
    let redactions: Vec<Redaction> = redactions_for(&runs, GEHEIM)
        .into_iter()
        .filter(|r| r.region.page == 1)
        .collect();
    assert!(!redactions.is_empty(), "nichts auf Seite 2 zu schwärzen");

    let mut doc = load_from_bytes(&bytes).expect("PDF ladbar");
    PdfRedactor::new()
        .apply_with_report(&mut doc, &redactions)
        .expect("Schwärzung");
    let out = redact_pdf::save_to_bytes(&doc).expect("Speichern");
    let found = redact_pdf::leaks(&out, "Zahlung an DE89");
    assert!(found.is_empty(), "der Spiegel steht noch: {found:?}");
}

// ---------------------------------------------------------------------------
// 3. Gewöhnliches Material bleibt still
// ---------------------------------------------------------------------------

fn redactions_for(runs: &[TextRun], needle: &str) -> Vec<Redaction> {
    runs.iter()
        .filter_map(|run| {
            let pos = run.text.find(needle)?;
            let rect = run.rect_for_byte_range(pos, pos + needle.len())?;
            Some(Redaction::new(
                Region::new(
                    run.page,
                    rect,
                    Some(needle.to_string()),
                    Source::Pattern {
                        pattern_id: "iban_de".into(),
                        confidence: 1.0,
                    },
                ),
                Action::Blackout,
            ))
        })
        .collect()
}

/// Fünfzig getaggte Seiten, ein Logo und eine Kopfzeile je Seite: keine
/// einzige Warnung, weder beim Lesen noch beim Schwärzen.
#[test]
fn gewoehnliches_getaggtes_dokument_gibt_keine_warnung() {
    let bytes = word_artig(50);
    let start = std::time::Instant::now();
    let doc = load_from_bytes(&bytes).expect("PDF ladbar");
    let (runs, warnings) = PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("Extraktion");
    let lesen = start.elapsed();
    assert!(warnings.is_empty(), "{warnings:?}");
    let redactions = redactions_for(&runs, "12,50 EUR");
    assert!(redactions.len() >= 50, "{}", redactions.len());
    let start = std::time::Instant::now();
    let mut doc = load_from_bytes(&bytes).expect("PDF ladbar");
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &redactions)
        .expect("Schwärzung");
    let schwaerzen = start.elapsed();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    println!(
        "word_artig(50): {} B, {} Läufe, lesen {lesen:?}, schwärzen {schwaerzen:?}, {}",
        bytes.len(),
        runs.len(),
        hwm()
    );
}

// ---------------------------------------------------------------------------
// Messungen (ignoriert; je eigener Prozess)
// ---------------------------------------------------------------------------

/// Was eine Zuordnung kostet: dieselbe Seite (2 Klammern × 50 000 `Do`)
/// einmal mit Spiegel (`R1_MIRROR=1`, 100 000 Paare) und einmal ohne.
#[test]
#[ignore = "Messung"]
fn mess_kosten_je_zuordnung() {
    let mirror = env_usize("R1_MIRROR", 1) == 1;
    let b = if mirror { 2 } else { 0 };
    let d = env_usize("R1_D", 50_000);
    let bytes = klammern_ueber_platzierungen(1, b, d, false);
    let vorher = hwm();
    let start = std::time::Instant::now();
    let scan = scan_first(&bytes);
    println!(
        "Spiegel {mirror}, {b} × {d}: scan_page {:?}, {} Paare, {} Warnung(en), vorher {vorher}, nachher {}",
        start.elapsed(),
        paare(&scan),
        scan.warnings.len(),
        hwm()
    );
}

/// Die Summe über Seiten: `R1_PAGES` Seiten teilen sich einen Strom mit
/// `R1_B` Klammern × `R1_D` `Do` (Vorgabe 100 × 999 = 99 900 Paare je
/// Seite, knapp unter der Decke). Gemessen werden Extraktor und Redaktor
/// getrennt, wie die Pipeline sie ruft — der Redaktor ohne eine einzige
/// Schwärzung, wie bei einer Datei ohne Treffer.
#[test]
#[ignore = "Messung"]
fn mess_seiten_mal_paare() {
    let pages = env_usize("R1_PAGES", 50);
    let b = env_usize("R1_B", 100);
    let d = env_usize("R1_D", 999);
    let ehrlich = env_usize("R1_HONEST", 1) == 1;
    let bytes = klammern_ueber_platzierungen(pages, b, d, ehrlich);
    println!(
        "{pages} Seiten × ({b} × {d}, ehrlich {ehrlich}): Datei {} B, {}",
        bytes.len(),
        hwm()
    );
    if env_usize("R1_SKIP_EXTRACT", 0) == 0 {
        let doc = load_from_bytes(&bytes).expect("PDF ladbar");
        let start = std::time::Instant::now();
        let (runs, warnings) = PdfExtractor::new()
            .extract_with_warnings(&doc)
            .expect("Extraktion");
        println!(
            "  Extraktor: {:?}, {} Läufe, {} Warnung(en) {:?}, {}",
            start.elapsed(),
            runs.len(),
            warnings.len(),
            warnings.first(),
            hwm()
        );
    }
    let mut doc = load_from_bytes(&bytes).expect("PDF ladbar");
    let start = std::time::Instant::now();
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &[])
        .expect("Schwärzung");
    println!(
        "  Redaktor (0 Schwärzungen): {:?}, {} Warnung(en) {:?}, {}",
        start.elapsed(),
        report.warnings.len(),
        report.warnings.first(),
        hwm()
    );
}

/// `R1_B` Klammern × `R1_S` Textoperationen auf `R1_PAGES` Seiten
/// (`R1_DO=1`: mit einem `Do`, damit der Redaktor die Sätze festhält).
#[test]
#[ignore = "Messung"]
fn mess_klammern_mal_textoperationen() {
    let pages = env_usize("R1_PAGES", 1);
    let b = env_usize("R1_B", 2000);
    let s = env_usize("R1_S", 2000);
    let mit_do = env_usize("R1_DO", 0) == 1;
    let bytes = klammern_ueber_text(pages, b, s, mit_do);
    println!(
        "{pages} Seiten × ({b} × {s} Tj, Do {mit_do}): Datei {} B, {}",
        bytes.len(),
        hwm()
    );
    let start = std::time::Instant::now();
    let scan = scan_first(&bytes);
    println!(
        "  scan_page: {:?}, {} Textzuordnungen, {} Paare, {} Warnung(en), {}",
        start.elapsed(),
        textzuordnungen(&scan),
        paare(&scan),
        scan.warnings.len(),
        hwm()
    );
    drop(scan);
    let doc = load_from_bytes(&bytes).expect("PDF ladbar");
    let start = std::time::Instant::now();
    let (runs, warnings) = PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("Extraktion");
    println!(
        "  Extraktor: {:?}, {} Läufe, {} Warnung(en), {}",
        start.elapsed(),
        runs.len(),
        warnings.len(),
        hwm()
    );
    drop(runs);
    drop(doc);
    let mut doc = load_from_bytes(&bytes).expect("PDF ladbar");
    let start = std::time::Instant::now();
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &[])
        .expect("Schwärzung");
    println!(
        "  Redaktor (0 Schwärzungen): {:?}, {} Warnung(en), {}",
        start.elapsed(),
        report.warnings.len(),
        hwm()
    );
}

/// Schreibt Material für Läufe über die Kommandozeile nach `R1_OUT`.
#[test]
#[ignore = "Material"]
fn schreibt_material() {
    let Ok(dir) = std::env::var("R1_OUT") else {
        return;
    };
    let pages = env_usize("R1_PAGES", 200);
    let b = env_usize("R1_B", 100);
    let d = env_usize("R1_D", 999);
    let sc = env_usize("R1_S", 1000);
    for (name, bytes) in [
        (
            format!("seiten_{pages}_{b}x{d}.pdf"),
            klammern_ueber_platzierungen(pages, b, d, true),
        ),
        (
            format!("text_{pages}_{b}x{sc}.pdf"),
            klammern_ueber_text(pages, b, sc, true),
        ),
        ("word_50.pdf".to_string(), word_artig(50)),
    ] {
        std::fs::write(format!("{dir}/{name}"), &bytes).expect("schreibbar");
        println!("{dir}/{name}: {} B", bytes.len());
    }
}
