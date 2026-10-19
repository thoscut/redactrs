//! Befunde aus dem Experten-Review der Buchungsliste.

use redact_booking::{BookingMatcher, CsvBookingLoader};
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

fn entry(pattern: &str) -> BookingEntry {
    BookingEntry {
        id: "b1".into(),
        list_type: ListType::Positive,
        pattern: pattern.into(),
        context_before: None,
        context_after: None,
        is_regex: false,
    }
}

/// Ein zusammengesetzt (NFC) geschriebener Listeneintrag findet denselben
/// Namen, wenn das PDF ihn zerlegt (NFD, Buchstabe + U+0308) enthält — und
/// die Schwärzung deckt dabei auch den letzten Akzent mit ab.
#[test]
fn nfc_entry_matches_nfd_text() {
    let text = "Empfänger: Ju\u{308}rgen Mu\u{308}ller";
    let m = BookingMatcher::new(vec![entry("Jürgen Müller")]).unwrap();
    let r = m.find_matches(&[run(text)]).unwrap();
    assert_eq!(r.len(), 1, "NFD-Text im PDF wird nicht gefunden: {r:?}");
    assert_eq!(
        r[0].text.as_deref(),
        Some("Ju\u{308}rgen Mu\u{308}ller"),
        "die Schwärzung muss den ganzen zerlegten Namen abdecken"
    );
}

/// Eine Kopfzeile in anderer Groß-/Kleinschreibung wird nicht nur von der
/// Spaltenprüfung akzeptiert, sondern auch beim Einlesen der Zeilen.
#[test]
fn uppercase_header_is_accepted() {
    let csv = "ID,List_Type,Pattern\nb1,positive,Max Mustermann\n";
    let entries = CsvBookingLoader
        .from_reader(csv.as_bytes())
        .unwrap_or_else(|e| panic!("Kopfzeile in Großbuchstaben abgelehnt: {e}"));
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, "b1");
    assert_eq!(entries[0].pattern, "Max Mustermann");
}
