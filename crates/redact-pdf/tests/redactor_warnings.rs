//! Was der Scanner findet, muss auch im Bericht der Schwärzung ankommen.
//!
//! Zwei Befunde stecken hier drin, beide messbar:
//!
//! * **Warnungen wurden verschluckt.** `PdfRedactor::apply_with_report` hat die
//!   Befunde des Seiten-Scans nirgends übernommen und ist bei „0 Schwärzungen“
//!   sofort zurückgesprungen. Ausgerechnet der gefährlichste Ausgang —
//!   0 Schwärzungen, Exit 0 — meldete also nichts, obwohl der Scanner sehr wohl
//!   etwas zu sagen hatte.
//! * **Annotationstext wurde gelöscht, nicht geschwärzt.** Er verschwand nur,
//!   weil die ganze Annotation entfernt und ihr Erscheinungsstrom beim
//!   Aufräumen mitgenommen wurde. Eine Annotation ohne `/Rect` trifft diese
//!   Regel nicht — ihr Text blieb stehen.

use lopdf::{dictionary, Document, Object, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{leaks, save_to_bytes, PdfRedactor};

const SECRET: &str = "DE89 3704 0044 0532 0130 00";

/// Eine Seite mit Helvetica/WinAnsi und dem übergebenen Content-Stream.
fn page(content: &[u8]) -> (Document, lopdf::ObjectId, lopdf::ObjectId, lopdf::ObjectId) {
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
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1_i64,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    (doc, page_id, resources_id, font_id)
}

fn blackout(rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            0,
            rect,
            None,
            Source::Manual {
                reason: "Test".into(),
            },
        ),
        Action::Blackout,
    )
}

// ---------------------------------------------------------------------------
// Warnungen erreichen den Bericht
// ---------------------------------------------------------------------------

/// Ein XObject ohne `/Subtype` verschluckt seinen Text. Der Scanner sagt das —
/// und der Bericht muss es weitergeben, auch wenn nichts zu schwärzen ist.
#[test]
fn a_scanner_warning_reaches_the_report_even_without_redactions() {
    let (mut doc, _, resources_id, font_id) = page(b"q /Fm0 Do Q\n");
    let body = format!("BT\n/F1 10 Tf\n72 640 Td\n(IBAN: {SECRET}) Tj\nET\n");
    let form_id = doc.add_object(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                // Kein /Subtype — genau der blinde Fleck.
                "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            },
            body.into_bytes(),
        )
        .with_compression(false),
    ));
    doc.get_dictionary_mut(resources_id)
        .unwrap()
        .set("XObject", dictionary! { "Fm0" => form_id });

    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &[])
        .expect("Schwärzung");
    assert_eq!(report.removed_glyphs, 0);
    assert!(
        report.warnings.iter().any(|w| w.contains("/Subtype")),
        "der Befund des Scanners ist im Bericht nicht angekommen: {:?}",
        report.warnings
    );
    assert!(
        report.warnings.iter().any(|w| w.contains("Fm0")),
        "die Warnung nennt das betroffene XObject nicht: {:?}",
        report.warnings
    );
}

/// Dieselbe Datei mit einer Schwärzung: die Warnung darf nicht doppelt
/// erscheinen und auch nicht verschwinden.
#[test]
fn the_same_warning_appears_once_when_there_are_redactions() {
    let (mut doc, _, resources_id, font_id) = page(b"q /Fm0 Do Q\n");
    let form_id = doc.add_object(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            },
            b"BT\n/F1 10 Tf\n72 640 Td\n(x) Tj\nET\n".to_vec(),
        )
        .with_compression(false),
    ));
    doc.get_dictionary_mut(resources_id)
        .unwrap()
        .set("XObject", dictionary! { "Fm0" => form_id });

    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &[blackout(Rect::new(40.0, 600.0, 560.0, 700.0))])
        .expect("Schwärzung");
    let hits = report
        .warnings
        .iter()
        .filter(|w| w.contains("/Subtype"))
        .count();
    assert_eq!(hits, 1, "{:?}", report.warnings);
}

/// Eine saubere Seite bleibt still — sonst wäre die Warnung wertlos.
#[test]
fn a_clean_page_produces_no_warning() {
    let (mut doc, _, _, _) = page(b"BT\n/F1 10 Tf\n72 700 Td\n(Hallo) Tj\nET\n");
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &[])
        .expect("Schwärzung");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

// ---------------------------------------------------------------------------
// Annotationstext wird geschwärzt, nicht gelöscht
// ---------------------------------------------------------------------------

/// Eine Annotation ohne `/Rect` fällt durch das Raster von
/// `remove_annotations` — sie wird also *nicht* entfernt. Ihr
/// Erscheinungsstrom zeichnet die IBAN trotzdem mitten in den
/// Schwärzungsbereich. Nur wenn der Redaktor die Annotationen mitscannt,
/// verschwindet der Text.
#[test]
fn text_in_an_annotation_without_rect_is_redacted_not_merely_dropped() {
    let (mut doc, page_id, _, font_id) = page(b"BT\n/F1 10 Tf\n72 700 Td\n(Kopf) Tj\nET\n");
    let appearance = format!("BT\n/F1 10 Tf\n72 640 Td\n(IBAN: {SECRET}) Tj\nET\n");
    let ap_id = doc.add_object(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            },
            appearance.into_bytes(),
        )
        .with_compression(false),
    ));
    let annot_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "FreeText",
        // Kein /Rect: `remove_annotations` findet hier nichts zum Vergleichen.
        "F" => 4_i64,
        "AP" => dictionary! { "N" => ap_id },
    }));
    doc.get_dictionary_mut(page_id)
        .unwrap()
        .set("Annots", Object::Array(vec![Object::Reference(annot_id)]));

    let before = save_to_bytes(&doc).expect("Speichern");
    assert!(
        !leaks(&before, SECRET).is_empty(),
        "Vorbedingung: die IBAN steht in der Datei"
    );

    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &[blackout(Rect::new(40.0, 600.0, 560.0, 700.0))])
        .expect("Schwärzung");
    assert_eq!(
        report.removed_annotations, 0,
        "die Annotation wurde entfernt statt geschwärzt — dann misst dieser Test nichts"
    );
    assert!(
        report.removed_glyphs > 0,
        "im Erscheinungsstrom wurde kein Zeichen gefunden"
    );

    strip_and_check(&mut doc);
}

/// Metadaten strippen, speichern, und dann in der *Datei* nachsehen.
fn strip_and_check(doc: &mut Document) {
    redact_pdf::strip_metadata(doc);
    let out = save_to_bytes(doc).expect("Speichern");
    let hits = leaks(&out, SECRET);
    assert!(
        hits.is_empty(),
        "die IBAN steht noch {} mal in der Datei:\n{}",
        hits.len(),
        hits.join("\n")
    );
}
