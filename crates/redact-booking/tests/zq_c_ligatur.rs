//! Temporärer Reproduzierer (Korrektheits-Review): Ligatur-Codepunkt im PDF-Text.
use redact_booking::BookingMatcher;
use redact_core::{BookingEntry, Glyph, ListType, Rect, TextRun};

fn run(text: &str) -> TextRun {
    let glyphs = text
        .chars()
        .enumerate()
        .map(|(i, ch)| Glyph {
            ch,
            rect: Rect::new(i as f64 * 5.0, 0.0, (i as f64 + 1.0) * 5.0, 10.0),
        })
        .collect();
    TextRun::new(0, glyphs)
}

#[test]
fn ligatur_fi_trifft_eintrag() {
    let m = BookingMatcher::new(vec![BookingEntry {
        id: "b1".into(),
        list_type: ListType::Positive,
        pattern: "Max Fischer".into(),
        context_before: None,
        context_after: None,
        is_regex: false,
    }])
    .unwrap();
    let r = m
        .find_matches(&[run("Kontoinhaber: Max \u{fb01}scher")])
        .unwrap();
    assert_eq!(r.len(), 1, "U+FB01 im Text verhindert den Treffer");
}
