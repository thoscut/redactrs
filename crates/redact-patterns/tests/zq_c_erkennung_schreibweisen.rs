//! Schreibweisen, an denen die eingebauten Muster vorbeisahen — und die
//! Gegenproben, damit die Korrektur keine neuen Fehltreffer kostet.
//!
//! * **K1:** Leerraum außer Leerzeichen/Tabulator zwischen Schlüsselwort und
//!   Wert (U+00A0, U+202F) nahm den Kontext; der Treffer fiel unter die
//!   Schwelle.
//! * **K2:** Ligatur-Codepunkt im Schlüsselwort (`Identiﬁkationsnummer`).
//!   Der Extraktor zerlegt die Ligatur beim Zeilenbau (am Binary geprüft in
//!   `redact-cli/tests/zq_c_erkennung_am_binary.rs`); hier steht, dass das
//!   Muster die zerlegte Form trifft.
//! * **K3:** „Konto-Nummer“, „+49 (0)30 …“, „(030) …“, `de89 …`, `DE 89 …`,
//!   IBAN direkt vor Buchstaben, klein geschriebene Gläubiger-ID.
//! * **K4:** Großbuchstabenwörter mit einem Ländercode an Stelle 5–6
//!   (`KREDITKARTE`, `MUENCHEN`) bestanden die BIC-Prüfung und wurden mit 0.99
//!   geschwärzt.

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

/// Was der Standardlauf in `line` findet: (Muster, Text).
fn found(line: &str) -> Vec<(String, String)> {
    let m = PatternMatcher::new(&[]).unwrap();
    m.find_matches(&[run(line)])
        .unwrap()
        .into_iter()
        .map(|r| {
            let id = match r.source {
                Source::Pattern { pattern_id, .. } => pattern_id,
                _ => String::new(),
            };
            (id, r.text.unwrap_or_default())
        })
        .collect()
}

/// Jede Zeile muss genau `(muster, text)` liefern.
#[track_caller]
fn expect(cases: &[(&str, &str, &str)]) {
    let wrong: Vec<String> = cases
        .iter()
        .filter(|(line, id, text)| found(line) != vec![(id.to_string(), text.to_string())])
        .map(|(line, _, _)| format!("{line:?} -> {:?}", found(line)))
        .collect();
    assert!(wrong.is_empty(), "falsch erkannt:\n{}", wrong.join("\n"));
}

#[test]
fn k1_jeder_leerraum_hinter_dem_schluesselwort() {
    expect(&[
        ("Steuer-ID:\u{a0}12345678903", "steuer_id", "12345678903"),
        ("Steuer-ID:\u{202f}12345678903", "steuer_id", "12345678903"),
        ("BLZ:\u{a0}37040044", "blz", "37040044"),
        ("Kto.-Nr.:\u{a0}0532013000", "konto_nr", "0532013000"),
        ("Telefon:\u{a0}030 1234567", "phone_de", "030 1234567"),
        (
            "Steuer-ID:\u{a0}\u{a0} 12345678903",
            "steuer_id",
            "12345678903",
        ),
    ]);
}

/// Leerzeichen bleiben unbegrenzt erlaubt wie vor der Korrektur; nur der
/// *andere* Leerraum ist auf vier Zeichen begrenzt.
#[test]
fn k1_leerzeichen_bleiben_unbegrenzt_anderer_leerraum_nicht() {
    expect(&[("Steuer-ID:      12345678903", "steuer_id", "12345678903")]);
    expect(&[(
        "Steuer-ID: \u{a0} \u{a0}  \u{2007}\u{202f} 12345678903",
        "steuer_id",
        "12345678903",
    )]);
    assert!(found("Steuer-ID:\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}12345678903").is_empty());
}

#[test]
fn k2_ligatur_zerlegt_trifft_das_schluesselwort() {
    expect(&[(
        "Identifikationsnummer: 12345678903",
        "steuer_id",
        "12345678903",
    )]);
}

#[test]
fn k3_schreibweisen() {
    expect(&[
        ("Konto-Nummer: 0532013000", "konto_nr", "0532013000"),
        ("Konto Nummer 0532013000", "konto_nr", "0532013000"),
        ("Tel.: +49 (0)30 12345678", "phone_de", "+49 (0)30 12345678"),
        ("Telefon: (030) 12345678", "phone_de", "(030) 12345678"),
        ("+49 (0)30 12345678", "phone_de", "+49 (0)30 12345678"),
        (
            "IBAN: de89 3704 0044 0532 0130 00",
            "iban_de",
            "de89 3704 0044 0532 0130 00",
        ),
        (
            "IBAN: De89370400440532013000",
            "iban_de",
            "De89370400440532013000",
        ),
        (
            "IBAN: DE 89 3704 0044 0532 0130 00",
            "iban_de",
            "DE 89 3704 0044 0532 0130 00",
        ),
        (
            "DE89370400440532013000EUR",
            "iban_de",
            "DE89370400440532013000",
        ),
        (
            "Gläubiger-ID: de98zzz09999999999",
            "glaeubiger_id",
            "de98zzz09999999999",
        ),
    ]);
}

/// Gegenproben zu K3: was vorher richtig war, bleibt es.
#[test]
fn k3_gegenproben() {
    expect(&[
        (
            "IBAN: DE89 3704 0044 0532 0130 00",
            "iban_de",
            "DE89 3704 0044 0532 0130 00",
        ),
        (
            "Gläubiger-ID: DE98ZZZ09999999999 Lastschrift",
            "glaeubiger_id",
            "DE98ZZZ09999999999",
        ),
        ("Kontonummer 0532013000", "konto_nr", "0532013000"),
        ("Tel.: +49 30 1234567", "phone_de", "+49 30 1234567"),
    ]);
    // Eine 19. Ziffer macht weiter keine IBAN; eine falsche Prüfziffer auch
    // klein geschrieben nicht.
    // (Gefragt wird nur `iban_de`: dieselbe Ziffernfolge besteht zufällig die
    // Prüfung der Gläubiger-ID — das war vorher genauso.)
    let iban = |line: &str| {
        found(line)
            .into_iter()
            .filter(|(id, _)| id == "iban_de")
            .count()
    };
    assert_eq!(iban("DE893704004405320130001"), 0);
    assert_eq!(iban("de88 3704 0044 0532 0130 00"), 0);
    // Ohne Schlüsselwort und ohne Ländervorwahl bleibt die Ortsvorwahl in
    // Klammern unter der Schwelle — wie die ohne Klammern.
    assert!(found("(030) 12345678").is_empty());
    assert!(found("030 12345678").is_empty());
}

#[test]
fn k4_grossbuchstabenwoerter_sind_keine_bic() {
    for w in [
        "PRIVATKONTO",
        "KREDITKARTE",
        "80331 MUENCHEN",
        "40213 DUESSELDORF",
        "LASTSCHRIFT",
    ] {
        let f = found(w);
        assert!(f.is_empty(), "{w:?} -> {f:?}");
    }
}

/// Gegenprobe zu K4: echte BICs werden weiter gefunden — nach einem
/// Schlüsselwort jede, ohne Schlüsselwort die mit Ziffer oder `XXX`.
#[test]
fn k4_echte_bics_bleiben() {
    expect(&[
        ("BIC: COBADEFFXXX", "bic", "COBADEFFXXX"),
        ("BIC: DEUTDEFF", "bic", "DEUTDEFF"),
        ("BIC DEUTDEFF500", "bic", "DEUTDEFF500"),
        ("bic: DEUTDEFF", "bic", "DEUTDEFF"),
        ("SWIFT-Code: DEUTDEFF", "bic", "DEUTDEFF"),
        ("BIC/SWIFT: DEUTDEFF", "bic", "DEUTDEFF"),
        ("BIC:\u{a0}DEUTDEFF", "bic", "DEUTDEFF"),
        ("COBADEFFXXX", "bic", "COBADEFFXXX"),
        ("GENODEF1S01", "bic", "GENODEF1S01"),
        ("MARKDEF1100", "bic", "MARKDEF1100"),
    ]);
    // Ohne Schlüsselwort und ganz aus Buchstaben: unter der Schwelle, aber
    // mit gesenkter Schwelle sichtbar — mit dem Wert ohne Kontext, nicht 0.99.
    assert!(found("DEUTDEFF").is_empty());
    let m = PatternMatcher::new(&[])
        .unwrap()
        .with_min_confidence(0.3)
        .unwrap();
    let r = m.find_matches(&[run("DEUTDEFF")]).unwrap();
    assert_eq!(r.len(), 1);
    match &r[0].source {
        Source::Pattern { confidence, .. } => assert!((*confidence - 0.3).abs() < 1e-6),
        other => panic!("{other:?}"),
    }
}
