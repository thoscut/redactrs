//! Ligaturen dürfen eine Schwärzung nicht halb überleben.
//!
//! Eine Ligatur ist im Content-Stream **ein** Zeichencode, steht aber für
//! mehrere Zeichen. Die Schwärzung entscheidet je Zeichen; geschrieben wird
//! aber je Code. Trifft der Schwärzungsbereich nur die zweite Hälfte, entschied
//! früher das *erste* Teilzeichen — der Code wurde wieder ausgegeben und mit
//! ihm die ganze Ligatur.
//!
//! ## Warum hier nicht `redact_pdf::leaks` misst
//!
//! [`redact_pdf::leaks`] sucht **Bytes**. Der Text einer Ligatur steht aber
//! nirgends als Bytes in der Datei: im Strom steht nur ihr Code (hier `0xC8`),
//! die Zeichen entstehen erst über `/Differences` bzw. `/ToUnicode`. Ein
//! Bytesucher kann diesen Leck-Typ prinzipiell nicht sehen. Gemessen wird
//! deshalb an der Stelle, an der er entsteht: an den Textoperationen der
//! Ausgabedatei. Zusätzlich wird der lesbare Text der Ausgabe geprüft — das
//! ist es, was `pdftotext` und Copy-&-Paste liefern.

use lopdf::content::Operation;
use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{PdfExtractor, PdfRedactor};

/// Code der Ligatur „fi“ im Testdokument.
const LIGATURE_CODE: u8 = 0xC8;

/// Baut ein einseitiges PDF, dessen `/F1` den Code `0xC8` auf die Ligatur
/// „fi“ abbildet (`uni00660069` — zwei Zeichen, ein Code).
fn fixture(content: &str) -> (Document, ObjectId) {
    let mut doc = Document::with_version("1.5");
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => dictionary! {
            "Type" => "Encoding",
            "BaseEncoding" => "WinAnsiEncoding",
            "Differences" => vec![
                Object::Integer(i64::from(LIGATURE_CODE)),
                Object::Name(b"uni00660069".to_vec()),
            ],
        },
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
    (doc, page_id)
}

fn manual(rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            0,
            rect,
            None,
            Source::Manual {
                reason: "Ligatur".into(),
            },
        ),
        Action::Blackout,
    )
}

/// Alle Bytes, die die Ausgabe noch über Textoperationen ausgibt.
fn shown_bytes(doc: &Document, page_id: ObjectId) -> Vec<u8> {
    let data = doc.get_page_content(page_id).expect("Content lesbar");
    let mut out = Vec::new();
    for op in redact_pdf::ops::decode_content(&data) {
        collect(&op, &mut out);
    }
    out
}

/// Die Rechtecke der beiden Teilzeichen der Ligatur, wie der Interpreter sie
/// liefert. Aus dem zweiten wird der Schwärzungsbereich.
fn ligature_halves(doc: &Document, page_id: ObjectId) -> (Rect, Rect) {
    let scan = redact_pdf::scan_page(doc, page_id).expect("Scan");
    let glyphs: Vec<_> = scan.shows.iter().flat_map(|s| s.glyphs()).collect();
    assert_eq!(glyphs[0].text, "f", "erstes Teilzeichen");
    assert_eq!(glyphs[1].text, "i", "zweites Teilzeichen");
    assert!(
        glyphs[1].bytes.is_empty(),
        "das zweite Teilzeichen trägt keine eigenen Bytes"
    );
    (glyphs[0].rect, glyphs[1].rect)
}

fn collect(op: &Operation, out: &mut Vec<u8>) {
    if !matches!(op.operator.as_str(), "Tj" | "TJ" | "'" | "\"") {
        return;
    }
    for operand in &op.operands {
        match operand {
            Object::String(bytes, _) => out.extend_from_slice(bytes),
            Object::Array(items) => {
                for item in items {
                    if let Object::String(bytes, _) = item {
                        out.extend_from_slice(bytes);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Der Bereich deckt **genau** das zweite Teilzeichen der Ligatur ab — nicht
/// mehr und nicht weniger. Ohne zusätzlichen Rand (`padding = 0`) berührt er
/// weder das erste Teilzeichen noch das folgende „n“; getroffen ist damit
/// ausschließlich die zweite Hälfte eines einzigen Codes.
#[test]
fn a_ligature_hit_only_on_its_second_half_disappears_completely() {
    let content = "BT /F1 10 Tf 1 0 0 1 72 700 Tm (\\310n 4711000) Tj ET";
    let (mut doc, page_id) = fixture(content);

    // Vorbedingung: der Code steht im Strom und liefert zwei Zeichen.
    let before = PdfExtractor::new().extract(&doc).expect("Extraktion");
    // Die beiden Teilzeichen teilen sich einen Ursprung; die Zeilenbildung
    // setzt deshalb hinter der Ligatur ein Leerzeichen. Für diesen Test zählt
    // nur, dass „f“ und „i“ überhaupt da sind.
    assert!(
        before[0].text.starts_with("fi"),
        "Vorbedingung: Ligaturtext, war {:?}",
        before[0].text
    );

    let second_half = ligature_halves(&doc, page_id).1;
    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(&mut doc, &[manual(second_half)])
        .expect("Schwärzung");

    // Beide Teilzeichen der Ligatur zählen als entfernt.
    assert_eq!(
        report.removed_glyphs, 2,
        "die Ligatur wurde nicht als Ganzes entfernt"
    );

    let shown = shown_bytes(&doc, page_id);
    assert!(
        !shown.contains(&LIGATURE_CODE),
        "der Ligatur-Code steht noch im Strom: {shown:?}"
    );

    let bytes = redact_pdf::save_to_bytes(&doc).expect("Speichern");
    let out = redact_pdf::load_from_bytes(&bytes).expect("Laden");
    let after: Vec<String> = PdfExtractor::new()
        .extract(&out)
        .expect("Extraktion")
        .into_iter()
        .map(|r| r.text)
        .collect();
    assert!(
        !after
            .iter()
            .any(|line| line.contains('f') || line.contains('i')),
        "die Ligatur ist noch lesbar: {after:?}"
    );
    // Gegenprobe: der unbeteiligte Rest der Zeile steht noch da.
    assert!(
        !redact_pdf::leaks(&bytes, "4711000").is_empty(),
        "unbeteiligter Text wurde mitentfernt"
    );
}

/// Gegenprobe: wird die Ligatur gar nicht getroffen, bleibt sie unangetastet.
#[test]
fn an_untouched_ligature_stays() {
    let content = "BT /F1 10 Tf 1 0 0 1 72 700 Tm (\\310n 4711000) Tj ET";
    let (mut doc, page_id) = fixture(content);

    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &[manual(Rect::new(85.0, 699.0, 125.0, 708.0))])
        .expect("Schwärzung");
    assert!(report.removed_glyphs > 0);

    let shown = shown_bytes(&doc, page_id);
    assert!(
        shown.contains(&LIGATURE_CODE),
        "die Ligatur wurde ohne Treffer entfernt"
    );
}
