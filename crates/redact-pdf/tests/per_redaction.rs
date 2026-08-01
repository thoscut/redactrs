//! Wirkung **je Region** — die Grundlage für ein ehrliches Audit-Log.
//!
//! Ohne diese Aufschlüsselung kennt ein Protokoll nur die Gesamtsumme. Eine
//! Region, die gültig aussieht, im Strom aber nichts trifft — falsche
//! Koordinaten, Glyphen außerhalb, nur ein Bild darunter —, wäre darin von
//! einer wirksamen nicht zu unterscheiden und stünde als „angewendet“ im Log.
//! Genau diese Verwechslung schließt [`RedactionReport::per_redaction`] aus.

use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::testing::{build_pdf, TextItem};
use redact_pdf::{load_from_bytes, PdfRedactor, RedactionReport};

const SECRET: &str = "DE89 3704 0044 0532 0130 00";

fn manual(page: usize, rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            page,
            rect,
            None,
            Source::Manual {
                reason: "Audit".into(),
            },
        ),
        Action::Blackout,
    )
}

/// Eine Seite mit zwei Zeilen: die IBAN bei y = 700, ein unbeteiligter Satz
/// bei y = 650. Der Rest der Seite ist leer.
fn statement() -> Document {
    let bytes = build_pdf(&[vec![
        TextItem::new(72.0, 700.0, 10.0, format!("IBAN: {SECRET}")),
        TextItem::new(72.0, 650.0, 10.0, "Kontoinhaber: Max Mustermann"),
    ]]);
    load_from_bytes(&bytes).expect("PDF ladbar")
}

fn apply(doc: &mut Document, redactions: &[Redaction]) -> RedactionReport {
    PdfRedactor::new()
        .apply_with_report(doc, redactions)
        .expect("Schwärzung")
}

#[test]
fn a_region_that_hits_nothing_is_reported_as_zero() {
    let mut doc = statement();
    let redactions = [
        // Trifft die IBAN-Zeile.
        manual(0, Rect::new(72.0, 698.0, 250.0, 710.0)),
        // Gültig, aber im leeren Teil der Seite — trifft kein Zeichen.
        manual(0, Rect::new(300.0, 300.0, 400.0, 340.0)),
    ];
    let report = apply(&mut doc, &redactions);

    assert_eq!(report.per_redaction.len(), redactions.len());
    assert!(
        report.per_redaction[0] > 0,
        "die IBAN-Zeile wurde nicht getroffen"
    );
    assert_eq!(
        report.per_redaction[1], 0,
        "der leere Bereich hat angeblich etwas entfernt"
    );
    // Beide Bereiche haben trotzdem ein Deck-Rechteck bekommen: gezeichnet
    // wird, was der Nutzer wollte — gezählt wird, was wirklich verschwand.
    assert_eq!(report.drawn_rects, 2);
    assert_eq!(report.removed_glyphs, report.per_redaction[0]);
}

#[test]
fn every_redaction_gets_a_slot_even_across_pages() {
    let bytes = build_pdf(&[
        vec![TextItem::new(72.0, 700.0, 10.0, "Seite eins")],
        vec![TextItem::new(72.0, 700.0, 10.0, format!("IBAN: {SECRET}"))],
    ]);
    let mut doc = load_from_bytes(&bytes).expect("PDF ladbar");
    let redactions = [
        // Seite 2 (0-basiert: 1) — trifft.
        manual(1, Rect::new(72.0, 698.0, 250.0, 710.0)),
        // Seite 1 — trifft nichts.
        manual(0, Rect::new(400.0, 100.0, 450.0, 120.0)),
        // Seite 1 — trifft.
        manual(0, Rect::new(70.0, 698.0, 130.0, 710.0)),
    ];
    let report = apply(&mut doc, &redactions);

    assert_eq!(report.per_redaction.len(), 3);
    assert!(report.per_redaction[0] > 0, "Seite 2 nicht getroffen");
    assert_eq!(report.per_redaction[1], 0);
    assert!(report.per_redaction[2] > 0, "Seite 1 nicht getroffen");
    assert_eq!(
        report.per_redaction.iter().sum::<usize>(),
        report.removed_glyphs,
        "ohne Überlappung muss die Summe aufgehen"
    );
}

/// Ein `--padding`, das die Region auf nichts zusammenschrumpfen lässt, ist
/// ein Sonderfall derselben Frage: der Eintrag bleibt, die Zahl ist 0.
#[test]
fn a_region_that_padding_makes_degenerate_counts_zero() {
    let mut doc = statement();
    let redactions = [manual(0, Rect::new(100.0, 700.0, 100.5, 700.5))];
    let report = PdfRedactor::with_padding(-2.0)
        .apply_with_report(&mut doc, &redactions)
        .expect("Schwärzung");
    assert_eq!(report.per_redaction, vec![0]);
    assert_eq!(report.removed_glyphs, 0);
}

/// Überlappende Bereiche werden jeder für sich gezählt — die Summe darf
/// deshalb größer sein als die Gesamtzahl. Das ist der Preis dafür, die Frage
/// „hat *diese* Region etwas bewirkt?“ überhaupt beantworten zu können.
#[test]
fn overlapping_regions_are_counted_each_for_itself() {
    let mut doc = statement();
    let rect = Rect::new(72.0, 698.0, 250.0, 710.0);
    let report = apply(&mut doc, &[manual(0, rect), manual(0, rect)]);

    assert_eq!(report.per_redaction.len(), 2);
    assert_eq!(report.per_redaction[0], report.per_redaction[1]);
    assert_eq!(report.per_redaction[0], report.removed_glyphs);
    assert_eq!(
        report.per_redaction.iter().sum::<usize>(),
        2 * report.removed_glyphs
    );
}

/// Auch Text in einem Form-XObject wird der Region zugerechnet, die ihn
/// verdeckt — und ein mehrfach platziertes Formular zählt trotzdem nur einmal.
#[test]
fn glyphs_inside_a_form_xobject_are_attributed_too() {
    let (mut doc, _) = form_fixture();
    let redactions = [
        manual(0, Rect::new(72.0, 698.0, 250.0, 710.0)),
        manual(0, Rect::new(400.0, 200.0, 460.0, 220.0)),
    ];
    let report = apply(&mut doc, &redactions);

    assert!(
        report.per_redaction[0] > 0,
        "Text im Form-XObject wurde keiner Region zugerechnet"
    );
    assert_eq!(report.per_redaction[1], 0);
    assert_eq!(report.removed_glyphs, report.per_redaction[0]);
}

/// Ein Formular mit der IBAN, **zweimal** platziert: einmal bei y = 700,
/// einmal bei y = 400. Nur die obere Platzierung liegt im Schwärzungsbereich.
fn form_fixture() -> (Document, ObjectId) {
    let mut doc = Document::with_version("1.5");
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let form_id = doc.add_object(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 595.into(), 20.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            },
            format!("BT /F1 10 Tf 1 0 0 1 72 0 Tm (IBAN: {SECRET}) Tj ET").into_bytes(),
        )
        .with_compression(false),
    ));
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
        "XObject" => dictionary! { "Fm0" => form_id },
    });
    let content_id = doc.add_object(Stream::new(
        dictionary! {},
        b"q 1 0 0 1 0 700 cm /Fm0 Do Q\nq 1 0 0 1 0 400 cm /Fm0 Do Q\n".to_vec(),
    ));
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
    (doc, page_id)
}

#[test]
fn without_redactions_the_list_is_empty() {
    let mut doc = statement();
    let report = apply(&mut doc, &[]);
    assert!(report.per_redaction.is_empty());
}
