//! Was der Abgleich gegen eine Buchungsliste kosten darf.
//!
//! # Warum es diese Datei gibt
//!
//! Eine Performance-Prüfung hat am fertigen Programm zwei Wachstumsformen
//! gemessen, die beide nicht linear waren:
//!
//! **(1) Quadratisch in der Zeilenlänge.** Für jede Fundstelle rief
//! `collect` `TextRun::rect_for_byte_range` auf, und das zählt jedes Mal ab
//! Glyphe 0. Bei M Fundstellen in einer Zeile aus G Glyphen sind das G·M/2
//! Schritte. Gemessen (Release, eine Seite, ein `Tj` mit k IBANs, ein
//! Eintrag): k = 5 000 → 1,95 s, 10 000 → 5,30 s, 20 000 → 19,4 s,
//! 40 000 → 128 s — aus einer Eingabedatei von 2 899 Byte, Rückgabewert 0.
//! Dieselben 40 000 Treffer über die eingebauten Muster kosteten 5,8 s; die
//! hatten den `GlyphCursor` schon.
//!
//! **(2) Einträge × Text.** Jeder Eintrag durchsuchte jede Zeile für sich.
//! Gemessen an 2 000 Seiten mit Einträgen ohne Treffer: ohne Liste 2,9 s,
//! E = 2 000 → 9,9 s, 16 000 → 92 s; Dokument *und* Liste verdoppelt
//! vervierfachte die Zeit. Die Liste darf 16 MB groß sein, das sind einige
//! 100 000 Einträge.
//!
//! Beide Messreihen hier verdoppeln die Eingabe und verlangen höchstens den
//! Faktor [`SCHWELLE`] — es geht um 4 gegen 2. Gemessen wird je Größe der Bestwert aus
//! mehreren Läufen; warum, steht in `redact-patterns/tests/backtracking.rs`
//! bei `bestzeit`.

use std::time::{Duration, Instant};

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

fn entry(id: String, pattern: String) -> BookingEntry {
    BookingEntry {
        id,
        list_type: ListType::Positive,
        pattern,
        context_before: None,
        context_after: None,
        is_regex: false,
    }
}

const MESSLAEUFE: usize = 5;

/// Höchster erlaubter Faktor bei doppelter Eingabe. Linear ist 2, die alten
/// Fehler lagen bei 3,3 bis 4. 3,0 statt der 2,5 aus `backtracking.rs`, weil
/// die Messungen hier nur Dutzende Millisekunden dauern: unter fremder Last
/// lag ein linearer Lauf schon bei 2,28.
const SCHWELLE: f64 = 3.0;

/// Bestwert aus [`MESSLAEUFE`] Läufen; `erwartet` ist die Trefferzahl.
fn bestzeit(matcher: &BookingMatcher, runs: &[TextRun], erwartet: usize) -> Duration {
    (0..MESSLAEUFE)
        .map(|_| {
            let start = Instant::now();
            let regions = matcher.find_matches(runs).unwrap();
            let dauer = start.elapsed();
            assert_eq!(regions.len(), erwartet);
            dauer
        })
        .min()
        .expect("MESSLAEUFE ist größer als null")
}

fn faktor(kurz: Duration, lang: Duration) -> f64 {
    lang.as_secs_f64() / kurz.as_secs_f64().max(1e-9)
}

/// Befund (1): viele Fundstellen in **einer** Zeile.
///
/// Gemessen (Debug-Build, Bestwert aus fünf Läufen): vorher 6 000 → 2,58 s,
/// 12 000 → 10,2 s (×3,96); nachher 10 000 → 31 ms, 20 000 → 66 ms.
#[test]
fn viele_treffer_in_einer_zeile_wachsen_linear() {
    let matcher = BookingMatcher::new(vec![entry("b".into(), "DE89 3704".into())]).expect("baubar");
    let messe = |k: usize| {
        let runs = [run(&"DE89 3704 ".repeat(k))];
        bestzeit(&matcher, &runs, k)
    };
    let t_kurz = messe(10_000);
    let t_lang = messe(20_000);
    let f = faktor(t_kurz, t_lang);
    println!("eine Zeile: 10 000 Treffer {t_kurz:?}, 20 000 Treffer {t_lang:?} — Faktor {f:.2}");
    assert!(
        f < SCHWELLE,
        "Treffer in einer Zeile wachsen überlinear (Faktor {f:.2}): {t_kurz:?} → {t_lang:?}"
    );
}

/// Befund (2): Liste **und** Text verdoppelt dürfen die Zeit nur verdoppeln.
///
/// Gemessen (Debug-Build): vorher 1 000 × 1 000 → 226 ms, 2 000 × 2 000 →
/// 753 ms (×3,34); nachher ×2,0 — der Automat liest jede Zeile einmal.
#[test]
fn liste_mal_text_waechst_nicht_bilinear() {
    let messe = |e: usize, zeilen: usize| {
        let entries = (0..e)
            .map(|i| entry(format!("b{i}"), format!("Empfaenger Nummer {i:07} GmbH")))
            .collect();
        let matcher = BookingMatcher::new(entries).expect("baubar");
        let runs: Vec<TextRun> = (0..zeilen)
            .map(|i| {
                run(&format!(
                    "Buchung {i:06} Lastschrift DE89 3704 0044 0532 0130 00 EUR"
                ))
            })
            .collect();
        bestzeit(&matcher, &runs, 0)
    };
    let t_kurz = messe(2_000, 2_000);
    let t_lang = messe(4_000, 4_000);
    let f = faktor(t_kurz, t_lang);
    println!("Liste × Text: 2 000 × 2 000 {t_kurz:?}, 4 000 × 4 000 {t_lang:?} — Faktor {f:.2}");
    assert!(
        f < SCHWELLE,
        "Einträge × Zeilen wachsen bilinear (Faktor {f:.2}): {t_kurz:?} → {t_lang:?}"
    );
}
