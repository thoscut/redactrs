//! Gedrehter Text: Extraktion **und** Schwärzung.
//!
//! ## Der geprüfte Befund
//!
//! Gemeldet war: gedrehter Text werde gar nicht extrahiert — eine um 90°
//! gedrehte IBAN zerfalle in 22 Einzelzeilen, eine um 180° gedrehte werde
//! rückwärts gelesen. Die Zeilenbildung in [`redact_pdf::PdfExtractor`]
//! gruppiert inzwischen entlang der Grundlinie und trennt nach Schreibrichtung
//! (`DIRECTION_BUCKET_DEGREES`); die Zahlen unten sind der Nachweis, dass der
//! Befund damit erledigt ist.
//!
//! Diese Datei bleibt als **Regressionswächter** stehen: sie misst die
//! Zeilenzahl (nicht „mindestens eine“), die Leserichtung und, mit
//! [`redact_pdf::leaks`] als Orakel, dass eine Schwärzung über gedrehtem Text
//! auch wirklich greift.

use lopdf::{dictionary, Document, Object, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{leaks, save_to_bytes, PdfExtractor, PdfRedactor};

const SECRET: &str = "DE89 3704 0044 0532 0130 00";
/// Ein unbeteiligter, waagerechter Satz auf derselben Seite.
const NEIGHBOUR: &str = "Kontoinhaber Max Mustermann";

fn doc_with(content: &str) -> Document {
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
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
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
    doc
}

/// Textmatrix für die vier rechten Winkel, verankert in (x, y).
fn tm(degrees: i32, x: f64, y: f64) -> String {
    let m = match degrees {
        90 => "0 1 -1 0",
        180 => "-1 0 0 -1",
        270 => "0 -1 1 0",
        _ => "1 0 0 1",
    };
    format!("{m} {x} {y} Tm ")
}

/// Eine gedrehte IBAN plus eine waagerechte Nachbarzeile.
fn page(degrees: i32) -> Document {
    let content = format!(
        "BT /F1 9 Tf {}({SECRET}) Tj ET\nBT /F1 9 Tf {}({NEIGHBOUR}) Tj ET\n",
        tm(degrees, 300.0, 400.0),
        tm(0, 60.0, 60.0),
    );
    doc_with(&content)
}

fn lines(doc: &Document) -> Vec<String> {
    PdfExtractor::new()
        .extract(doc)
        .expect("Extraktion")
        .into_iter()
        .map(|run| run.text)
        .collect()
}

// ---------------------------------------------------------------------------
// Extraktion
// ---------------------------------------------------------------------------

/// Der Kern des gemeldeten Befunds: 22 Einzelzeilen statt einer.
///
/// Gemessen wird die **exakte** Zeilenzahl — „mindestens eine“ würde den
/// Rückfall in Einzelglyphen nicht bemerken.
#[test]
fn rotated_text_stays_one_line_per_angle() {
    for degrees in [0, 90, 180, 270] {
        let got = lines(&page(degrees));
        assert_eq!(
            got.len(),
            2,
            "{degrees}°: aus zwei Zeilen wurden {} — {got:?}",
            got.len()
        );
        assert!(
            got.contains(&SECRET.to_string()),
            "{degrees}°: die IBAN steht nicht als eine Zeile da: {got:?}"
        );
        assert!(
            got.contains(&NEIGHBOUR.to_string()),
            "{degrees}°: die waagerechte Zeile ist beschädigt: {got:?}"
        );
    }
}

/// 180° darf nicht rückwärts gelesen werden.
#[test]
fn text_rotated_by_180_degrees_is_not_reversed() {
    let got = lines(&page(180));
    let reversed: String = SECRET.chars().rev().collect();
    assert!(
        !got.contains(&reversed),
        "180° wurde rückwärts gelesen: {got:?}"
    );
}

/// Zwei gedrehte Zeilen nebeneinander bleiben zwei Zeilen — und behalten ihre
/// Reihenfolge in Vorschubrichtung.
#[test]
fn two_rotated_lines_stay_apart() {
    let content = format!(
        "BT /F1 9 Tf {}(Kontonummer) Tj ET\nBT /F1 9 Tf {}({SECRET}) Tj ET\n",
        tm(90, 300.0, 400.0),
        tm(90, 316.0, 400.0),
    );
    assert_eq!(
        lines(&doc_with(&content)),
        vec!["Kontonummer".to_string(), SECRET.to_string()]
    );
}

// ---------------------------------------------------------------------------
// Schwärzung
// ---------------------------------------------------------------------------

/// Der Bereich, den ein Nutzer über die gedrehte Zeile ziehen würde.
///
/// Die Zeile beginnt in (300, 400) und läuft in Drehrichtung; 9-pt-Helvetica
/// bleibt in jeder Richtung unter 12 pt Höhe und unter 140 pt Länge. Der
/// Bereich ist aus diesen Konstruktionsdaten gerechnet, nicht aus der
/// Glyphengeometrie des Interpreters — sonst prüfte der Test sich selbst.
fn band(degrees: i32) -> Rect {
    let (x, y) = (300.0, 400.0);
    let (long, thick) = (150.0, 12.0);
    match degrees {
        90 => Rect::new(x - thick, y - thick, x + thick, y + long),
        180 => Rect::new(x - long, y - thick, x + thick, y + thick),
        270 => Rect::new(x - thick, y - long, x + thick, y + thick),
        _ => Rect::new(x - thick, y - thick, x + long, y + thick),
    }
}

#[test]
fn a_redaction_over_rotated_text_really_removes_it() {
    for degrees in [0, 90, 180, 270] {
        let mut doc = page(degrees);
        let before = save_to_bytes(&doc).expect("Speichern");
        assert!(
            !leaks(&before, SECRET).is_empty(),
            "{degrees}°: Vorbedingung — die IBAN steht in der Datei"
        );

        let report = PdfRedactor::new()
            .apply_with_report(
                &mut doc,
                &[Redaction::new(
                    Region::new(
                        0,
                        band(degrees),
                        None,
                        Source::Manual {
                            reason: "gedreht".into(),
                        },
                    ),
                    Action::Blackout,
                )],
            )
            .expect("Schwärzung");
        assert_eq!(
            report.removed_glyphs,
            SECRET.chars().count(),
            "{degrees}°: nicht alle Zeichen der Zeile wurden entfernt"
        );

        redact_pdf::strip_metadata(&mut doc);
        let after = save_to_bytes(&doc).expect("Speichern");
        let hits = leaks(&after, SECRET);
        assert!(
            hits.is_empty(),
            "{degrees}°: die IBAN steht noch {} mal in der Datei:\n{}",
            hits.len(),
            hits.join("\n")
        );
        assert!(
            !leaks(&after, NEIGHBOUR).is_empty(),
            "{degrees}°: die unbeteiligte Zeile wurde mitentfernt"
        );
    }
}
