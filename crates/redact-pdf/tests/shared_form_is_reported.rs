//! Ein Form-XObject auf mehreren Seiten: die Schwärzung trifft sie alle — und
//! das muss gesagt werden.
//!
//! Ein Formular steht **einmal** in der Datei, gleichgültig wie oft es
//! platziert ist. Wer eine Region auf Seite 1 schwärzt, entfernt den Text damit
//! auch auf Seite 2. Das ist die sichere Richtung und im Modulkopf von
//! `redact_pdf::redact` auch so beschrieben — gemeldet wurde es bisher nur für
//! *Bilder* (`redact_pdf::image`), nicht für Text. Eine Seite, die niemand
//! ausgewählt hat, ändert sich also stillschweigend.
//!
//! Die Meldung trägt bewusst dieselbe Textmarke wie die Bildvariante:
//! `redact_pipeline::coverage::NOT_A_COVERAGE_GAP` sortiert beide darüber ein
//! („zu viel geschwärzt, nicht zu wenig — das Gegenteil einer Lücke“). Der
//! Rückgabewert bleibt deshalb 0; nur erfährt die Nutzerin jetzt davon.

use lopdf::{dictionary, Document, Object, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{PdfRedactor, RedactionReport};

const SECRET: &str = "DE89 3704 0044 0532 0130 00";

/// Die Textmarke, an der `redact-pipeline` „keine Deckungslücke“ erkennt.
/// Läuft sie hier weg, springt der Rückgabewert plötzlich auf 3.
const MARKE: &str = "die Schwärzung wirkt deshalb auch auf die anderen Seiten";

/// `pages` Seiten, die sich alle dasselbe Form-XObject mit der IBAN teilen.
fn document_with_shared_form(pages: usize) -> Document {
    let mut doc = Document::with_version("1.5");
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let form = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        },
        format!("BT /F1 10 Tf 72 700 Td (IBAN: {SECRET}) Tj ET\n").into_bytes(),
    );
    let form_id = doc.add_object(Object::Stream(form));
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
        "XObject" => dictionary! { "Fm0" => form_id },
    });

    let pages_id = doc.new_object_id();
    let mut page_ids = Vec::new();
    for index in 0..pages {
        let content = doc.add_object(Stream::new(
            dictionary! {},
            format!(
                "BT /F1 10 Tf 72 760 Td (Seite {}) Tj ET\nq /Fm0 Do Q\n",
                index + 1
            )
            .into_bytes(),
        ));
        page_ids.push(doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        }));
    }
    let count = page_ids.len() as i64;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => page_ids.iter().copied().map(Object::Reference).collect::<Vec<_>>(),
            "Count" => count,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    doc
}

/// Schwärzt die IBAN — und zwar ausdrücklich nur auf Seite 1.
fn redact_page_one(doc: &mut Document) -> RedactionReport {
    let redaction = Redaction::new(
        Region::new(
            0,
            Rect::new(70.0, 695.0, 300.0, 712.0),
            None,
            Source::Manual {
                reason: "IBAN auf Seite 1".into(),
            },
        ),
        Action::Blackout,
    );
    PdfRedactor::with_padding(0.0)
        .apply_with_report(doc, &[redaction])
        .expect("Schwärzung")
}

fn shared_form_warning(report: &RedactionReport) -> Option<&String> {
    report.warnings.iter().find(|w| w.contains(MARKE))
}

/// Der Befund: die Schwärzung wirkt auf Seite 2, ohne dass jemand sie genannt
/// hätte — ungemeldet war das eine Überraschung in einer Datei, die danach
/// weitergegeben wird.
#[test]
fn a_form_on_two_pages_is_reported_when_it_is_redacted() {
    let mut doc = document_with_shared_form(2);
    let report = redact_page_one(&mut doc);
    assert!(report.removed_glyphs > 0, "es wurde gar nichts entfernt");

    let warning = shared_form_warning(&report).unwrap_or_else(|| {
        panic!(
            "keine Meldung über das geteilte Formular: {:#?}",
            report.warnings
        )
    });
    assert!(
        warning.contains("2 Seiten") && warning.contains("Seite 1, 2"),
        "die Meldung nennt die betroffenen Seiten nicht: {warning}"
    );
    assert!(
        warning.contains(&report.removed_glyphs.to_string()),
        "die Meldung nennt nicht, wie viel entfallen ist: {warning}"
    );
}

/// Steht das Formular nur auf einer Seite, gibt es nichts zu melden — eine
/// Warnung, die bei jeder Datei anspringt, sagt nach der dritten nichts mehr.
#[test]
fn a_form_on_one_page_is_not_reported() {
    let mut doc = document_with_shared_form(1);
    let report = redact_page_one(&mut doc);
    assert!(report.removed_glyphs > 0, "es wurde gar nichts entfernt");
    assert_eq!(
        shared_form_warning(&report),
        None,
        "Meldung ohne Anlass: {:#?}",
        report.warnings
    );
}

/// Und ohne Schwärzung im Formular auch dann nicht, wenn es geteilt wird.
#[test]
fn an_untouched_shared_form_is_not_reported() {
    let mut doc = document_with_shared_form(2);
    // Ein Bereich weit weg vom Formulartext, aber auf der Seite selbst.
    let redaction = Redaction::new(
        Region::new(
            0,
            Rect::new(70.0, 755.0, 300.0, 772.0),
            None,
            Source::Manual {
                reason: "Kopfzeile".into(),
            },
        ),
        Action::Blackout,
    );
    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(&mut doc, &[redaction])
        .expect("Schwärzung");
    assert!(report.removed_glyphs > 0, "es wurde gar nichts entfernt");
    assert_eq!(
        shared_form_warning(&report),
        None,
        "das Formular blieb unangetastet: {:#?}",
        report.warnings
    );
}

/// Die Wirkung selbst bleibt, wie sie war: der Text ist auf **beiden** Seiten
/// fort. Gemeldet wird sie, nicht geändert.
#[test]
fn the_redaction_still_reaches_every_page_that_uses_the_form() {
    let mut doc = document_with_shared_form(2);
    redact_page_one(&mut doc);
    let bytes = redact_pdf::save_to_bytes(&doc).expect("Speichern");
    let found = redact_pdf::leaks(&bytes, SECRET);
    assert!(
        found.is_empty(),
        "die IBAN steht noch in der Datei: {found:?}"
    );
}

/// Ein Formular auf vielen Seiten: die Liste bleibt lesbar.
#[test]
fn many_pages_are_listed_briefly() {
    let mut doc = document_with_shared_form(12);
    let report = redact_page_one(&mut doc);
    let warning = shared_form_warning(&report).expect("Meldung");
    assert!(
        warning.contains("12 Seiten") && warning.contains("Seite 1, 2, 3, 4, 5, 6, 7, 8 …"),
        "die Seitenliste ist nicht gekürzt: {warning}"
    );
}

/// Sicherheitsnetz gegen einen Wortlaut, der wegläuft: `redact-pipeline`
/// erkennt an genau dieser Marke, dass hier **zu viel** und nicht zu wenig
/// geschwärzt wurde. Ohne sie liefe der Rückgabewert auf 3.
#[test]
fn the_wording_carries_the_mark_the_exit_code_depends_on() {
    let mut doc = document_with_shared_form(2);
    let report = redact_page_one(&mut doc);
    let warning = shared_form_warning(&report).expect("Meldung");
    assert!(warning.contains(MARKE), "{warning}");
}
