//! Übereinander gedruckter Text und Ströme, die kein `Do` erreicht.
//!
//! Vier Fälle, in denen der Lauf früher mit „Treffer 0, Rückgabewert 0“ endete
//! — dem schlimmsten aller Ergebnisse, weil es sich wie „geprüft und sauber“
//! liest:
//!
//! 1. **Doppelt gedruckter Text** (Fett-Imitat, Schlagschatten, Rückkern in
//!    einer `TJ`-Operation). Die Zeilenbildung verschränkte die beiden Drucke
//!    zeichenweise zu `IIBBAANN::  DDEE8899 …`.
//! 2. **Mehrere Erscheinungsströme einer Annotation** — jede Checkbox, jedes
//!    Radio-Feld, jedes `/N` + `/D`. Zwei Textflüsse auf demselben `/Rect`,
//!    verschränkt wie oben.
//! 3. **Text in `/ExtGState /SMask /G`** — ein Strom, den der Interpreter nie
//!    betrat.
//! 4. **Ein nie gezeichnetes Form-XObject** in `/Resources`.
//!
//! Gemessen wird nicht mit dem eigenen Extraktor — das wäre ein Zirkelschluss
//! — sondern mit [`redact_pdf::leaks`] an der **fertigen Datei**. Die
//! Gegenproben stehen daneben: eine Tabellenzeile aus vielen `Tj`-Aufrufen
//! muss weiterhin zu *einer* Zeile verschmelzen, sonst ist der Preis für die
//! Reparatur höher als der Schaden.

mod common;

use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Redaction, Region, Source, TextRun};
use redact_pdf::{
    leaks, load_from_bytes, save_to_bytes, strip_metadata, PdfExtractor, PdfRedactor,
};

const SECRET: &str = "DE89 3704 0044 0532 0130 00";

fn iban() -> String {
    format!("IBAN: {SECRET}")
}

// ---------------------------------------------------------------------------
// Werkzeug
// ---------------------------------------------------------------------------

struct Builder {
    doc: Document,
    font_id: ObjectId,
}

impl Builder {
    fn new() -> Self {
        let mut doc = Document::with_version("1.5");
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1",
            "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding",
        });
        Self { doc, font_id }
    }

    fn stream(&mut self, dict: lopdf::Dictionary, data: Vec<u8>) -> ObjectId {
        self.doc.add_object(Object::Stream(
            Stream::new(dict, data).with_compression(false),
        ))
    }

    /// Ein Form-XObject mit eigener `/BBox` und eigenen Ressourcen.
    fn form(&mut self, bbox: [i64; 4], content: &str) -> ObjectId {
        let font_id = self.font_id;
        self.stream(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => bbox.iter().map(|v| Object::Integer(*v)).collect::<Vec<_>>(),
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            },
            content.as_bytes().to_vec(),
        )
    }

    /// Schließt das Dokument mit einer Seite ab.
    fn finish(
        mut self,
        content: &str,
        extra_resources: Vec<(&str, Object)>,
        annots: Option<Vec<Object>>,
    ) -> Vec<u8> {
        let font_id = self.font_id;
        let mut resources = dictionary! { "Font" => dictionary! { "F1" => font_id } };
        for (key, value) in extra_resources {
            resources.set(key, value);
        }
        let resources_id = self.doc.add_object(resources);
        let content_id = self.stream(dictionary! {}, content.as_bytes().to_vec());
        let pages_id = self.doc.new_object_id();
        let mut page = dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
            "Resources" => Object::Reference(resources_id),
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        };
        if let Some(annots) = annots {
            page.set("Annots", Object::Array(annots));
        }
        let page_id = self.doc.add_object(page);
        self.doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![Object::Reference(page_id)], "Count" => 1_i64,
            }),
        );
        let catalog = self
            .doc
            .add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        self.doc.trailer.set("Root", catalog);
        let mut out = Vec::new();
        self.doc.save_to(&mut out).expect("speicherbar");
        out
    }
}

fn extract(bytes: &[u8]) -> Vec<TextRun> {
    let doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfExtractor::new().extract(&doc).expect("Extraktion")
}

fn lines(bytes: &[u8]) -> Vec<String> {
    extract(bytes).into_iter().map(|r| r.text).collect()
}

fn warnings(bytes: &[u8]) -> Vec<String> {
    let doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("Extraktion")
        .1
}

/// **Alle** Fundstellen eines Textstücks — nicht nur die erste. Genau darum
/// geht es bei doppelt gedrucktem Text.
fn redactions_for(runs: &[TextRun], needle: &str) -> Vec<Redaction> {
    let mut out = Vec::new();
    for run in runs {
        let mut from = 0;
        while let Some(offset) = run.text[from..].find(needle) {
            let start = from + offset;
            if let Some(rect) = run.rect_for_byte_range(start, start + needle.len()) {
                out.push(Redaction::new(
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
                ));
            }
            from = start + needle.len();
        }
    }
    out
}

/// Die vollständige Verarbeitung, so wie das Werkzeug sie fährt.
fn pipeline(bytes: &[u8], redactions: &[Redaction]) -> Vec<u8> {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfRedactor::new()
        .apply_with_report(&mut doc, redactions)
        .expect("Schwärzung");
    strip_metadata(&mut doc);
    save_to_bytes(&doc).expect("Speichern")
}

/// Der ehrliche Prüfmaßstab: Muster finden, schwärzen, fertige Datei messen.
fn residue_after_redaction(bytes: &[u8]) -> usize {
    assert!(
        !leaks(bytes, SECRET).is_empty(),
        "Vorbedingung: das Geheimnis muss vor der Schwärzung in der Datei stehen"
    );
    let runs = extract(bytes);
    let redactions = redactions_for(&runs, SECRET);
    assert!(
        !redactions.is_empty(),
        "die Analyse hat das Geheimnis in keiner Zeile gefunden: {:?}",
        runs.iter().map(|r| &r.text).collect::<Vec<_>>()
    );
    leaks(&pipeline(bytes, &redactions), SECRET).len()
}

// ---------------------------------------------------------------------------
// Befund 1 — doppelt gedruckter Text
// ---------------------------------------------------------------------------

/// Dieselbe Zeile zweimal, um `dx` versetzt — das gängige Fett-Imitat.
fn fake_bold(dx: f64) -> Vec<u8> {
    let b = Builder::new();
    let content = format!(
        "BT /F1 10 Tf 72 700 Td ({s}) Tj ET\nBT /F1 10 Tf {x} 700 Td ({s}) Tj ET\n",
        s = iban(),
        x = 72.0 + dx
    );
    b.finish(&content, Vec::new(), None)
}

#[test]
fn doppelt_gedruckter_text_ergibt_zwei_lesbare_zeilen() {
    // Der Versatz ist gleichgültig: von deckungsgleich bis eine Zeichenbreite.
    for dx in [0.0, 0.1, 0.3, 0.5, 1.0, 2.0, 3.0, 5.0, 6.0] {
        let bytes = fake_bold(dx);
        let lines = lines(&bytes);
        assert_eq!(
            lines.len(),
            2,
            "dx = {dx}: aus zwei Drucken müssen zwei Zeilen werden, nicht {lines:?}"
        );
        for line in &lines {
            assert!(
                line.contains(SECRET),
                "dx = {dx}: die Zeile ist verschränkt: {line:?}"
            );
        }
    }
}

#[test]
fn doppelt_gedruckter_text_bleibt_nach_der_schwaerzung_nicht_stehen() {
    for dx in [0.0, 0.3, 1.0, 3.0, 6.0] {
        assert_eq!(
            residue_after_redaction(&fake_bold(dx)),
            0,
            "dx = {dx}: nach der Schwärzung steht das Geheimnis noch in der Datei"
        );
    }
}

#[test]
fn ein_schlagschatten_macht_die_zeile_nicht_unlesbar() {
    let b = Builder::new();
    let content = format!(
        "BT 0.6 0.6 0.6 rg /F1 10 Tf 72.6 699.4 Td ({s}) Tj ET\n\
         BT 0 0 0 rg /F1 10 Tf 72 700 Td ({s}) Tj ET\n",
        s = iban()
    );
    let bytes = b.finish(&content, Vec::new(), None);
    assert_eq!(lines(&bytes).len(), 2);
    assert_eq!(residue_after_redaction(&bytes), 0);
}

#[test]
fn ein_rueckkern_in_einer_tj_operation_ist_auch_ein_ueberdruck() {
    // 16789/1000 em bei 10 pt ist genau die Breite von „IBAN: DE89 …“ —
    // derselbe Text zweimal übereinander, aber in *einer* Operation. Nach
    // Herkunft zu trennen fände das nie.
    let b = Builder::new();
    let content = format!(
        "BT /F1 10 Tf 72 700 Td [({s}) 16789 ({s})] TJ ET\n",
        s = iban()
    );
    let bytes = b.finish(&content, Vec::new(), None);
    let lines = lines(&bytes);
    assert_eq!(
        lines.len(),
        2,
        "erwartet zwei Druckschichten, nicht {lines:?}"
    );
    assert_eq!(residue_after_redaction(&bytes), 0);
}

// --- Gegenproben: was verschmelzen muss, verschmilzt weiterhin --------------

#[test]
fn eine_tabellenzeile_aus_vielen_tj_bleibt_eine_zeile() {
    // Der Preis, den die Reparatur nicht kosten darf. Vier Spalten, vier
    // `Tj`-Aufrufe, eine Zeile — und die IBAN steht über zwei Spalten hinweg
    // nur dann als Muster da, wenn beide in derselben Zeile landen.
    let b = Builder::new();
    let content = format!(
        "BT /F1 10 Tf 72 700 Td (05.01.2026) Tj 60 0 Td (Ueberweisung) Tj \
         80 0 Td (IBAN: ) Tj 34 0 Td ({SECRET}) Tj 190 0 Td (1.234,56) Tj ET\n\
         BT /F1 10 Tf 72 680 Td (06.01.2026) Tj 60 0 Td (Lastschrift) Tj \
         80 0 Td (Miete Januar) Tj 190 0 Td (900,00) Tj ET\n"
    );
    let bytes = b.finish(&content, Vec::new(), None);
    let lines = lines(&bytes);
    assert_eq!(
        lines.len(),
        2,
        "die Tabelle ist in Druckschichten zerfallen: {lines:?}"
    );
    assert!(
        lines[0].contains(&iban()),
        "die IBAN wurde über die Spaltengrenze hinweg nicht mehr zusammengesetzt: {:?}",
        lines[0]
    );
    assert!(lines[0].contains("1.234,56"));
    assert!(lines[1].contains("Miete Januar"));
    assert_eq!(residue_after_redaction(&bytes), 0);
}

#[test]
fn ein_vorwaertskern_bleibt_eine_zeile() {
    // Der Prüfstein: `-20000` schiebt das zweite Textstück **nach vorn**
    // (200 pt), es liegt also gerade *nicht* übereinander. Zwei Stücke
    // nebeneinander sind eine Zeile.
    let b = Builder::new();
    let content = format!(
        "BT /F1 10 Tf 72 700 Td [({s}) -20000 (BITTE AUFHEBEN)] TJ ET\n",
        s = iban()
    );
    let bytes = b.finish(&content, Vec::new(), None);
    assert_eq!(
        lines(&bytes),
        vec![format!("{} BITTE AUFHEBEN", iban())],
        "ein Kern nach vorn ist kein Überdruck"
    );
}

#[test]
fn saubere_fettung_ueber_strich_und_fuellung_bleibt_eine_zeile() {
    let b = Builder::new();
    let content = format!(
        "BT 2 Tr 0.3 w /F1 10 Tf 72 700 Td ({s}) Tj ET\n",
        s = iban()
    );
    let bytes = b.finish(&content, Vec::new(), None);
    assert_eq!(lines(&bytes), vec![iban()]);
    assert_eq!(residue_after_redaction(&bytes), 0);
}

#[test]
fn ein_kerningpaar_zerlegt_die_zeile_nicht() {
    // Feinausgleich innerhalb einer Zahl: der Rücksprung ist ein Bruchteil
    // eines Punktes und darf keine zweite Schicht eröffnen.
    let b = Builder::new();
    let content = format!("BT /F1 10 Tf 72 700 Td [(IBAN: )40({SECRET})] TJ ET\n");
    let bytes = b.finish(&content, Vec::new(), None);
    assert_eq!(lines(&bytes), vec![iban()]);
}

// ---------------------------------------------------------------------------
// Befund 2 — mehrere Erscheinungsströme einer Annotation
// ---------------------------------------------------------------------------

/// Schließt den Bau mit genau einer Annotation ab; die `/AP`-Einträge sind
/// bereits als Objekte im Dokument angelegt.
fn with_annotation(mut b: Builder, ap: lopdf::Dictionary, selected: Option<&str>) -> Vec<u8> {
    let mut annot = dictionary! {
        "Type" => "Annot", "Subtype" => "Widget",
        "Rect" => vec![300.into(), 400.into(), 540.into(), 420.into()],
        "F" => 4_i64,
        "AP" => ap,
    };
    if let Some(state) = selected {
        annot.set("AS", Object::Name(state.as_bytes().to_vec()));
    }
    let annot_id = b.doc.add_object(annot);
    b.finish(
        "BT /F1 10 Tf 72 700 Td (Kontoauszug Januar) Tj ET\n",
        Vec::new(),
        Some(vec![Object::Reference(annot_id)]),
    )
}

#[test]
fn zwei_erscheinungszustaende_einer_checkbox_bleiben_lesbar() {
    let mut b = Builder::new();
    let off = b.form(
        [0, 0, 240, 20],
        "BT /F1 10 Tf 2 5 Td (Nicht gewaehlt) Tj ET\n",
    );
    let on = b.form(
        [0, 0, 240, 20],
        &format!("BT /F1 10 Tf 2 5 Td ({}) Tj ET\n", iban()),
    );
    let bytes = with_annotation(
        b,
        dictionary! {
            "N" => dictionary! { "Off" => Object::Reference(off), "On" => Object::Reference(on) },
        },
        Some("On"),
    );

    let lines = lines(&bytes);
    assert!(
        lines.iter().any(|l| l.contains(SECRET)),
        "die beiden Zustände haben sich gegenseitig unlesbar gemacht: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("Nicht gewaehlt")),
        "der andere Zustand darf nicht aus der Analyse fallen: {lines:?}"
    );
    // Der sichtbare Zustand (`/AS`) steht vorn.
    let visible = lines.iter().position(|l| l.contains(SECRET)).unwrap();
    let hidden = lines
        .iter()
        .position(|l| l.contains("Nicht gewaehlt"))
        .unwrap();
    assert!(visible < hidden, "der /AS-Zustand gehört zuerst: {lines:?}");
    assert_eq!(residue_after_redaction(&bytes), 0);
}

#[test]
fn normalzustand_und_gedrueckter_zustand_bleiben_lesbar() {
    let mut b = Builder::new();
    let normal = b.form(
        [0, 0, 240, 20],
        &format!("BT /F1 10 Tf 2 5 Td ({}) Tj ET\n", iban()),
    );
    let down = b.form([0, 0, 240, 20], "BT /F1 10 Tf 2 5 Td (Klick mich) Tj ET\n");
    let bytes = with_annotation(
        b,
        dictionary! { "N" => Object::Reference(normal), "D" => Object::Reference(down) },
        None,
    );
    let lines = lines(&bytes);
    assert!(
        lines.iter().any(|l| l.contains(SECRET)),
        "verschränkt: {lines:?}"
    );
    assert!(lines.iter().any(|l| l.contains("Klick mich")), "{lines:?}");
    assert_eq!(residue_after_redaction(&bytes), 0);
}

#[test]
fn ein_einzelner_erscheinungsstrom_bleibt_wie_er_war() {
    let mut b = Builder::new();
    let normal = b.form(
        [0, 0, 240, 20],
        &format!("BT /F1 10 Tf 2 5 Td ({}) Tj ET\n", iban()),
    );
    let bytes = with_annotation(b, dictionary! { "N" => Object::Reference(normal) }, None);
    assert!(lines(&bytes).iter().any(|l| l == &iban()));
    assert_eq!(residue_after_redaction(&bytes), 0);
}

// ---------------------------------------------------------------------------
// Befund 4 — Text in `/ExtGState /SMask /G`
// ---------------------------------------------------------------------------

#[test]
fn text_in_einer_weichen_maske_wird_gefunden_und_entfernt() {
    let mut b = Builder::new();
    let font_id = b.font_id;
    // Eine Transparenzgruppe, so wie jeder Erzeuger sie schreibt.
    let group = b.stream(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Group" => dictionary! { "S" => "Transparency", "CS" => "DeviceGray" },
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        },
        format!("BT /F1 10 Tf 72 700 Td ({}) Tj ET\n", iban()).into_bytes(),
    );
    let gs = b.doc.add_object(dictionary! {
        "Type" => "ExtGState",
        "SMask" => dictionary! { "S" => "Luminosity", "G" => Object::Reference(group) },
    });
    let bytes = b.finish(
        "q /GS0 gs 0 0 0 rg 60 690 400 20 re f Q\n\
         BT /F1 10 Tf 72 640 Td (Kontoauszug Januar) Tj ET\n",
        vec![("ExtGState", Object::Dictionary(dictionary! { "GS0" => gs }))],
        None,
    );
    assert!(
        lines(&bytes).iter().any(|l| l.contains(SECRET)),
        "die Gruppen-Form der Maske wurde nicht betreten: {:?}",
        lines(&bytes)
    );
    assert_eq!(residue_after_redaction(&bytes), 0);
}

#[test]
fn eine_maske_ohne_gruppen_form_bleibt_stumm() {
    // `/SMask /None` ist der Normalfall und darf nichts auslösen.
    let mut b = Builder::new();
    let gs = b
        .doc
        .add_object(dictionary! { "Type" => "ExtGState", "SMask" => "None" });
    let bytes = b.finish(
        &format!("q /GS0 gs Q\nBT /F1 10 Tf 72 700 Td ({}) Tj ET\n", iban()),
        vec![("ExtGState", Object::Dictionary(dictionary! { "GS0" => gs }))],
        None,
    );
    assert!(warnings(&bytes).is_empty(), "{:?}", warnings(&bytes));
    assert_eq!(residue_after_redaction(&bytes), 0);
}

// ---------------------------------------------------------------------------
// Befund 5 — ein nie gezeichnetes Form-XObject
// ---------------------------------------------------------------------------

/// `drawn` entscheidet, ob der Seiteninhalt das Formular auch zeichnet.
fn form_in_resources(content: &str, drawn: bool) -> Vec<u8> {
    let mut b = Builder::new();
    let form = b.form([0, 0, 595, 842], content);
    let page = if drawn {
        "BT /F1 10 Tf 72 640 Td (Kontoauszug Januar) Tj ET\nq /Fm0 Do Q\n"
    } else {
        "BT /F1 10 Tf 72 640 Td (Kontoauszug Januar) Tj ET\n"
    };
    b.finish(
        page,
        vec![(
            "XObject",
            Object::Dictionary(dictionary! { "Fm0" => Object::Reference(form) }),
        )],
        None,
    )
}

#[test]
fn ein_nie_gezeichnetes_formular_mit_text_ist_eine_deckungsluecke() {
    let bytes = form_in_resources(
        &format!("BT /F1 10 Tf 72 700 Td ({}) Tj ET\n", iban()),
        false,
    );
    let warnings = warnings(&bytes);
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("Fm0") && w.contains("nirgends gezeichnet")),
        "ein stiller Nulltreffer ist das Schlimmste: {warnings:?}"
    );
    // Der Text steht weiterhin in der Datei — genau das sagt die Meldung.
    assert!(!leaks(&bytes, SECRET).is_empty());
}

#[test]
fn ein_gezeichnetes_formular_erzeugt_keine_meldung() {
    let bytes = form_in_resources(
        &format!("BT /F1 10 Tf 72 700 Td ({}) Tj ET\n", iban()),
        true,
    );
    assert!(warnings(&bytes).is_empty(), "{:?}", warnings(&bytes));
    assert_eq!(residue_after_redaction(&bytes), 0);
}

#[test]
fn ein_formular_ohne_text_bleibt_unerwaehnt() {
    // Ein Logo, ein Rahmen, eine Schraffur: nichts zu schwärzen, nichts zu
    // melden. Ein Rückgabewert 3, der bei jedem zweiten Dokument anspringt,
    // ist keiner.
    let bytes = form_in_resources("0 0 0 rg 10 10 40 40 re f\n", false);
    assert!(warnings(&bytes).is_empty(), "{:?}", warnings(&bytes));
}

#[test]
fn ein_formular_das_nur_eine_andere_seite_zeichnet_ist_keine_luecke() {
    // Geteilte Ressourcen sind der Regelfall: Seite 1 bietet das Formular an,
    // gezeichnet wird es erst auf Seite 2. Wer nur je Seite zählte, meldete
    // hier eine Lücke, die keine ist.
    let mut b = Builder::new();
    let font_id = b.font_id;
    let form = b.form(
        [0, 0, 595, 842],
        &format!("BT /F1 10 Tf 72 700 Td ({}) Tj ET\n", iban()),
    );
    let resources = b.doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
        "XObject" => dictionary! { "Fm0" => Object::Reference(form) },
    });
    let first = b.stream(
        dictionary! {},
        b"BT /F1 10 Tf 72 640 Td (Seite 1) Tj ET\n".to_vec(),
    );
    let second = b.stream(
        dictionary! {},
        b"BT /F1 10 Tf 72 640 Td (Seite 2) Tj ET\nq /Fm0 Do Q\n".to_vec(),
    );
    let pages_id = b.doc.new_object_id();
    let mut kids = Vec::new();
    for content in [first, second] {
        kids.push(Object::Reference(b.doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content,
            "Resources" => Object::Reference(resources),
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        })));
    }
    let count = kids.len() as i64;
    b.doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }),
    );
    let catalog = b
        .doc
        .add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    b.doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    b.doc.save_to(&mut bytes).expect("speicherbar");

    assert!(warnings(&bytes).is_empty(), "{:?}", warnings(&bytes));
    assert_eq!(residue_after_redaction(&bytes), 0);
}

// ---------------------------------------------------------------------------
// Zusatzbefund — geerbte `/Resources` als *direktes* Dictionary
// ---------------------------------------------------------------------------

#[test]
fn geerbte_ressourcen_als_direktes_dictionary_gelten_auch() {
    // `/Resources` darf am `/Pages`-Knoten direkt stehen (PDF 32000-1,
    // Tabelle 30). Ohne Objekt-Id fiel es früher aus der Vererbung: die Seite
    // sah aus, als hätte sie gar keine Ressourcen, ihr `Do` ging ins Leere,
    // und der Text des Formulars wurde nie gelesen.
    let mut b = Builder::new();
    let font_id = b.font_id;
    let form = b.form(
        [0, 0, 595, 842],
        &format!("BT /F1 10 Tf 72 700 Td ({}) Tj ET\n", iban()),
    );
    let content = b.stream(
        dictionary! {},
        b"BT /F1 10 Tf 72 640 Td (Kontoauszug Januar) Tj ET\nq /Fm0 Do Q\n".to_vec(),
    );
    let pages_id = b.doc.new_object_id();
    let page_id = b.doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => content,
    });
    b.doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![Object::Reference(page_id)], "Count" => 1_i64,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            // Direktes Dictionary, kein Verweis — genau darum geht es.
            "Resources" => dictionary! {
                "Font" => dictionary! { "F1" => font_id },
                "XObject" => dictionary! { "Fm0" => Object::Reference(form) },
            },
        }),
    );
    let catalog = b
        .doc
        .add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    b.doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    b.doc.save_to(&mut bytes).expect("speicherbar");

    assert!(
        lines(&bytes).iter().any(|l| l.contains(SECRET)),
        "das geerbte /XObject war nicht auflösbar: {:?}",
        lines(&bytes)
    );
    assert_eq!(residue_after_redaction(&bytes), 0);
}
