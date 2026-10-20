//! Befunde aus dem Experten-Review der Erkennungslogik.
//!
//! Jeder Test hält eine Schreibweise fest, die auf echten Auszügen vorkommt
//! und vorher **gar nicht** erkannt wurde — und, wo es darauf ankommt, die
//! Gegenprobe, dass die Erweiterung keine neuen Fehltreffer bringt.

use redact_core::{Glyph, Rect, Source, TextRun};
use redact_patterns::PatternMatcher;

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

/// Alle Treffer der Standardkonfiguration als (Pattern-ID, Text).
fn hits(text: &str) -> Vec<(String, String)> {
    let m = PatternMatcher::new(&[]).unwrap();
    m.find_matches(&[run(text)])
        .unwrap()
        .into_iter()
        .map(|r| {
            let id = match r.source {
                Source::Pattern { pattern_id, .. } => pattern_id,
                other => panic!("unerwartete Quelle {other:?}"),
            };
            (id, r.text.unwrap_or_default())
        })
        .collect()
}

fn assert_hit(text: &str, id: &str, expected: &str) {
    let h = hits(text);
    assert!(
        h.iter().any(|(i, t)| i == id && t == expected),
        "{text:?}: erwartet {id} auf {expected:?}, bekommen {h:?}"
    );
}

fn assert_no_hit(text: &str, id: &str) {
    let h = hits(text);
    assert!(
        !h.iter().any(|(i, _)| i == id),
        "{text:?}: kein {id} erwartet, bekommen {h:?}"
    );
}

/// Eine gültige Karte, hinter der eine kurze Zahl steht (Spalte „Tag"),
/// wird trotzdem gefunden: das Muster darf die fremde Zahl nicht mitfressen,
/// sonst scheitert die Luhn-Prüfung über die ganze Kette und die Karte bleibt
/// stehen.
#[test]
fn credit_card_followed_by_a_short_number_is_still_found() {
    assert_hit(
        "Karte 4539 1488 0343 6467 Betrag",
        "credit_card",
        "4539 1488 0343 6467",
    );
    assert_hit(
        "Karte 4539 1488 0343 6467 12.03. Umsatz",
        "credit_card",
        "4539 1488 0343 6467",
    );
    assert_hit(
        "Karte 4539-1488-0343-6467 12",
        "credit_card",
        "4539-1488-0343-6467",
    );
    assert_hit(
        "Karte 4539148803436467 12",
        "credit_card",
        "4539148803436467",
    );
}

/// Gegenprobe Kreditkarte: Luhn-falsch bleibt ungeschwärzt, auch mit der
/// neuen Gruppenstruktur.
#[test]
fn credit_card_with_broken_checksum_stays_unreported() {
    assert_no_hit("Karte 4539 1488 0343 6468 12.03.", "credit_card");
}

/// IBAN mit geschütztem Leerzeichen (U+00A0) als Gruppentrenner.
#[test]
fn iban_grouped_with_no_break_space() {
    let text = "IBAN: DE89\u{a0}3704\u{a0}0044\u{a0}0532\u{a0}0130\u{a0}00";
    assert_hit(
        text,
        "iban_de",
        "DE89\u{a0}3704\u{a0}0044\u{a0}0532\u{a0}0130\u{a0}00",
    );
}

/// IBAN mit schmalem geschütztem Leerzeichen (U+202F) als Gruppentrenner.
#[test]
fn iban_grouped_with_narrow_no_break_space() {
    let text = "IBAN: DE89\u{202f}3704\u{202f}0044\u{202f}0532\u{202f}0130\u{202f}00";
    assert_hit(
        text,
        "iban_de",
        "DE89\u{202f}3704\u{202f}0044\u{202f}0532\u{202f}0130\u{202f}00",
    );
}

/// IBAN mit einer durch Blocksatz verdoppelten Lücke.
#[test]
fn iban_with_a_double_space() {
    assert_hit(
        "IBAN: DE89  3704 0044 0532 0130 00",
        "iban_de",
        "DE89  3704 0044 0532 0130 00",
    );
}

/// Gegenprobe IBAN: eine falsche Prüfziffer bleibt auch mit den neuen
/// Trennzeichen verworfen.
#[test]
fn iban_with_no_break_space_and_broken_checksum_stays_unreported() {
    assert_no_hit(
        "IBAN: DE88\u{a0}3704\u{a0}0044\u{a0}0532\u{a0}0130\u{a0}00",
        "iban_de",
    );
}

/// Bankleitzahl in der gedruckten Gruppierung 3-3-2.
#[test]
fn blz_grouped_3_3_2() {
    assert_hit("BLZ: 370 400 44", "blz", "370 400 44");
}

/// Gegenprobe BLZ: ohne Schlüsselwort bleibt eine gruppierte Ziffernfolge
/// unter dem Mindestvertrauen.
#[test]
fn grouped_blz_without_keyword_stays_unreported() {
    assert_no_hit("Menge 370 400 44 Stück", "blz");
}

/// Steuer-ID in der amtlich gedruckten Gruppierung 2-3-3-3.
#[test]
fn steuer_id_grouped_2_3_3_3() {
    assert_hit("Steuer-ID: 12 345 678 903", "steuer_id", "12 345 678 903");
}

/// „Identifikationsnummer" allein (Lohnsteuerbescheinigung) zählt als
/// Kontext der Steuer-ID.
#[test]
fn steuer_id_after_identifikationsnummer() {
    assert_hit(
        "Identifikationsnummer: 12345678903",
        "steuer_id",
        "12345678903",
    );
}

/// „Kto.-Nr.:" zählt als Kontext der Kontonummer.
#[test]
fn konto_nr_after_kto_dash_nr() {
    assert_hit("Kto.-Nr.: 1234567890", "konto_nr", "1234567890");
}

/// „Konto Nr." (mit Leerzeichen) zählt als Kontext der Kontonummer.
#[test]
fn konto_nr_after_konto_space_nr() {
    assert_hit("Konto Nr. 1234567890", "konto_nr", "1234567890");
}
