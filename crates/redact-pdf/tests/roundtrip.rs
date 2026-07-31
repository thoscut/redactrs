//! End-to-End: PDF bauen → Text finden → schwärzen → prüfen, dass er weg ist.
//!
//! ## Zwei Orakel, bewusst getrennt
//!
//! * Der **eigene Extraktor** belegt genau eine Sache: der *Seiteninhalt* ist
//!   sauber. Mehr nicht. Wovor der Extraktor blind ist, das schwärzt das
//!   Werkzeug auch nicht — und genau das sähe ein extraktorbasierter Test dann
//!   ebenfalls nicht. Das ist ein Zirkelschluss, und Testnamen, die hier mehr
//!   behaupten, lügen.
//! * [`redact_pdf::leaks`] durchsucht die geschriebene Datei auf allen Ebenen
//!   (Rohbytes, alle Streams dekodiert, Objekt-Streams, sämtliche
//!   Zeichenketten, beide String-Kodierungen). Nur das belegt das
//!   Akzeptanzkriterium „Copy-Paste aus dem geschwärzten PDF liefert keinen
//!   sensitiven Text“ — denn „Copy-Paste“ heißt in der Praxis: irgendein
//!   fremdes Werkzeug, nicht unser eigenes.

use redact_core::{Action, Extractor, Redaction, Redactor, Region, Source, TextRun};
use redact_pdf::testing::{build_pdf, demo_statement, TextItem};
use redact_pdf::{
    leaks, load_from_bytes, save_to_bytes, strip_metadata, PdfExtractor, PdfRedactor,
};

fn extract_text(bytes: &[u8]) -> Vec<TextRun> {
    let doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfExtractor::new().extract(&doc).expect("Extraktion")
}

/// Was der **eigene** Extraktor aus dem Seiteninhalt holt.
///
/// Kein Leck-Orakel: das kann nur belegen, dass der Content-Stream sauber ist.
fn extractor_text(bytes: &[u8]) -> String {
    extract_text(bytes)
        .iter()
        .map(|r| r.text.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Das ehrliche Orakel: `needle` darf nirgends in der Datei mehr vorkommen.
#[track_caller]
fn assert_no_leak(bytes: &[u8], needle: &str) {
    let hits = leaks(bytes, needle);
    assert!(
        hits.is_empty(),
        "„{needle}“ steht noch {} mal in der Datei:\n{}",
        hits.len(),
        hits.join("\n")
    );
}

/// Sucht ein Textstück und baut daraus eine Schwärzung.
fn redaction_for(runs: &[TextRun], needle: &str) -> Redaction {
    for run in runs {
        if let Some(pos) = run.text.find(needle) {
            let rect = run
                .rect_for_byte_range(pos, pos + needle.len())
                .expect("Bounding-Box");
            return Redaction::new(
                Region::new(
                    run.page,
                    rect,
                    Some(needle.to_string()),
                    Source::Pattern {
                        pattern_id: "test".into(),
                        confidence: 1.0,
                    },
                ),
                Action::Blackout,
            );
        }
    }
    panic!("„{needle}“ nicht im extrahierten Text gefunden");
}

fn redact(bytes: &[u8], redactions: &[Redaction]) -> Vec<u8> {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfRedactor::new()
        .apply(&mut doc, redactions)
        .expect("Schwärzung");
    strip_metadata(&mut doc);
    save_to_bytes(&doc).expect("Speichern")
}

#[test]
fn extracts_text_with_positions() {
    let pdf = demo_statement();
    let runs = extract_text(&pdf);
    let joined = runs
        .iter()
        .map(|r| r.text.as_str())
        .collect::<Vec<_>>()
        .join("|");
    assert!(
        joined.contains("DE89 3704 0044 0532 0130 00"),
        "war: {joined}"
    );
    assert!(joined.contains("Musterfirma GmbH"), "war: {joined}");

    // Die erste Zeile steht bei y=780 und beginnt bei x=72.
    let headline = runs.iter().find(|r| r.text.contains("Musterbank")).unwrap();
    assert!(
        (headline.rect.ll.x - 72.0).abs() < 2.0,
        "x war {}",
        headline.rect.ll.x
    );
    assert!(headline.rect.ll.y > 770.0 && headline.rect.ll.y < 785.0);
    assert_eq!(headline.glyphs.len(), headline.text.chars().count());
}

/// Belegt **nur**, dass der Content-Stream sauber ist — gemessen mit dem
/// eigenen Extraktor. Der Beweis, dass in der Datei nichts mehr steht, ist
/// [`redaction_leaves_no_trace_in_the_file`].
#[test]
fn redaction_removes_text_from_the_extracted_content_stream() {
    let pdf = demo_statement();
    let runs = extract_text(&pdf);
    let redaction = redaction_for(&runs, "DE89 3704 0044 0532 0130 00");

    let redacted = redact(&pdf, &[redaction]);
    let text = extractor_text(&redacted);

    assert!(
        !text.contains("DE89"),
        "IBAN ist noch extrahierbar:\n{text}"
    );
    assert!(!text.contains("0532 0130 00"), "IBAN-Rest noch da:\n{text}");
    // Der Rest des Dokuments muss unangetastet bleiben.
    assert!(text.contains("Musterbank AG"), "Kontext verloren:\n{text}");
    assert!(text.contains("COBADEFFXXX"), "BIC verloren:\n{text}");
    assert!(text.contains("IBAN:"), "Label sollte bleiben:\n{text}");
}

/// Der eigentliche Beweis für das Akzeptanzkriterium: nach der Schwärzung
/// steht die IBAN **nirgends** mehr in der Datei.
#[test]
fn redaction_leaves_no_trace_in_the_file() {
    let pdf = demo_statement();
    let runs = extract_text(&pdf);
    let redaction = redaction_for(&runs, "DE89 3704 0044 0532 0130 00");

    let redacted = redact(&pdf, &[redaction]);

    assert_no_leak(&redacted, "DE89 3704 0044 0532 0130 00");
    assert_no_leak(&redacted, "DE89");
    // Gegenprobe: unbeteiligter Text muss erhalten bleiben.
    assert!(extractor_text(&redacted).contains("Musterbank AG"));
}

#[test]
fn surrounding_text_keeps_its_position() {
    // "AAAA BBBB CCCC" — nur BBBB wird geschwärzt, AAAA und CCCC dürfen
    // sich nicht verschieben.
    let pdf = build_pdf(&[vec![TextItem::new(50.0, 700.0, 12.0, "AAAA BBBB CCCC")]]);
    let runs = extract_text(&pdf);
    let before: Vec<(char, f64)> = runs[0].glyphs.iter().map(|g| (g.ch, g.rect.ll.x)).collect();

    let redacted = redact(&pdf, &[redaction_for(&runs, "BBBB")]);
    let after_runs = extract_text(&redacted);
    let after: Vec<(char, f64)> = after_runs[0]
        .glyphs
        .iter()
        .map(|g| (g.ch, g.rect.ll.x))
        .collect();

    assert_no_leak(&redacted, "BBBB");

    // Jedes verbliebene C muss exakt dort stehen, wo es vorher stand.
    let c_before: Vec<f64> = before
        .iter()
        .filter(|(c, _)| *c == 'C')
        .map(|(_, x)| *x)
        .collect();
    let c_after: Vec<f64> = after
        .iter()
        .filter(|(c, _)| *c == 'C')
        .map(|(_, x)| *x)
        .collect();
    assert_eq!(c_before.len(), 4);
    assert_eq!(c_after.len(), 4);
    for (b, a) in c_before.iter().zip(&c_after) {
        assert!((b - a).abs() < 0.5, "C verschoben: {b} -> {a}");
    }
}

#[test]
fn multiple_redactions_across_pages() {
    let pdf = demo_statement();
    let runs = extract_text(&pdf);
    let redactions = vec![
        redaction_for(&runs, "DE89 3704 0044 0532 0130 00"),
        redaction_for(&runs, "DE02 1203 0000 0000 2020 51"),
        redaction_for(&runs, "max.mustermann@example.org"),
    ];
    assert_eq!(
        redactions[1].region.page, 1,
        "zweite IBAN steht auf Seite 2"
    );

    let redacted = redact(&pdf, &redactions);
    assert_no_leak(&redacted, "DE89");
    assert_no_leak(&redacted, "DE02");
    assert_no_leak(&redacted, "example.org");
    assert!(
        extractor_text(&redacted).contains("Steuer-ID"),
        "Seite 2 sonst unversehrt"
    );
}

#[test]
fn whiteout_and_replace_also_remove_the_text() {
    let pdf = build_pdf(&[vec![TextItem::new(
        50.0,
        700.0,
        12.0,
        "IBAN DE89 3704 0044",
    )]]);
    let runs = extract_text(&pdf);

    for action in [Action::Whiteout, Action::Replace("[IBAN]".into())] {
        let mut r = redaction_for(&runs, "DE89 3704 0044");
        r.action = action.clone();
        let redacted = redact(&pdf, &[r]);
        let hits = leaks(&redacted, "DE89");
        assert!(
            hits.is_empty(),
            "{action:?} hat den Text nicht entfernt:\n{}",
            hits.join("\n")
        );
    }
}

#[test]
fn metadata_is_stripped() {
    let pdf = demo_statement();
    assert!(!leaks(&pdf, "Kontoauszug Max Mustermann").is_empty());

    let runs = extract_text(&pdf);
    let redacted = redact(&pdf, &[redaction_for(&runs, "DE89 3704 0044 0532 0130 00")]);
    assert_no_leak(&redacted, "Kontoauszug Max Mustermann");
    assert_no_leak(&redacted, "redact-rs testing");
}

#[test]
fn output_is_deterministic() {
    let pdf = demo_statement();
    let runs = extract_text(&pdf);
    let redactions = vec![redaction_for(&runs, "DE89 3704 0044 0532 0130 00")];
    let a = redact(&pdf, &redactions);
    let b = redact(&pdf, &redactions);
    assert_eq!(
        a, b,
        "gleiche Eingabe muss zu byteweise gleicher Ausgabe führen"
    );
}

#[test]
fn report_counts_removed_glyphs() {
    let pdf = build_pdf(&[vec![TextItem::new(50.0, 700.0, 12.0, "GEHEIM sichtbar")]]);
    let runs = extract_text(&pdf);
    let mut doc = load_from_bytes(&pdf).unwrap();
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &[redaction_for(&runs, "GEHEIM")])
        .unwrap();
    // 6 Zeichen "GEHEIM"; durch das Padding von 1pt kann das angrenzende
    // Leerzeichen mit entfernt werden — zu viel zu schwärzen ist die sichere
    // Richtung, zu wenig nicht.
    assert!(
        (6..=7).contains(&report.removed_glyphs),
        "entfernt: {}",
        report.removed_glyphs
    );
    assert_eq!(report.drawn_rects, 1);
}

#[test]
fn empty_redaction_list_is_a_no_op() {
    let pdf = demo_statement();
    let mut doc = load_from_bytes(&pdf).unwrap();
    let report = PdfRedactor::new().apply_with_report(&mut doc, &[]).unwrap();
    assert_eq!(report.removed_glyphs, 0);
}
