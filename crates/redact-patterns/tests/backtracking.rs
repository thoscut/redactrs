//! Was eine einzelne Textzeile das Pattern-Matching kosten darf.
//!
//! # Warum es diese Datei gibt
//!
//! Eine Sicherheitsprüfung hat beobachtet, dass sich „eine Datei innerhalb
//! aller dokumentierten Grenzen gar nicht verarbeiten lässt". Die Schwelle lag
//! nicht bei den 16 MB von `--max-parsed-mb`, sondern bei **rund 1 500 Zeichen
//! in einem einzigen Textlauf**: eine Zeile aus 1 600 Buchstaben ließ
//! `konto_nr` mit
//!
//! ```text
//! Error executing regex: Max limit for backtracking count exceeded
//! ```
//!
//! abbrechen, Rückgabewert 1. 1 400 Zeichen liefen noch; ein realistischer
//! Mischtext aus 8 000 Zeichen war unauffällig. Es traf nur ununterbrochene
//! Buchstabenketten — und die stehen in jedem Fließtext, sobald ein PDF eine
//! lange Zeile in *einer* Show-Text-Operation setzt.
//!
//! ## Zwei verschiedene Befunde
//!
//! **(1) Quadratisch.** Der Kontextpräfix von `konto_nr` war
//! `(?i:[a-zäöüß]*konto…)`. `[a-zäöüß]*` ist unbegrenzt und beginnt an *jeder*
//! Position — es gibt keinen Look-behind davor, der die Startpositionen
//! einschränkt. Über einer Buchstabenkette der Länge n frisst die Suche damit
//! n Positionen × n Buchstaben = O(n²) Schritte. Das ist die eigentliche
//! Ursache und wird an der Quelle behoben: der Präfix ist jetzt begrenzt
//! ([`MAX_KEYWORD_PREFIX`] Zeichen), die Suche damit linear.
//!
//! **(2) Linear, aber nicht umsonst.** `fancy_regex` verbucht die
//! Rückschritte *pro Suchaufruf*, und ein Suchaufruf läuft über den ganzen
//! Textlauf. Auch ein streng lineares Muster verbraucht deshalb ein Budget,
//! das mit der Länge des Laufs wächst — gemessen rund ein Dutzend Schritte je
//! Zeichen bei `phone_de`, das die meisten Schlüsselwort-Alternativen hat. Mit
//! der Vorgabe von `fancy_regex` (1 000 000) ist bei rund 80 000 Zeichen
//! Schluss, ganz ohne Backtracking-Pathologie. Dagegen hilft kein Regex,
//! sondern nur ein bewusst gesetztes Budget:
//! [`redact_patterns::BACKTRACK_LIMIT`].
//!
//! Beide Befunde werden hier getrennt festgehalten — der erste an der
//! Laufzeit, der zweite an der Länge.

use std::time::{Duration, Instant};

use redact_core::{Glyph, Rect, TextRun};
use redact_patterns::{
    builtin_pattern_ids, builtin_patterns, PatternDef, PatternMatcher, BACKTRACK_LIMIT,
    MAX_KEYWORD_PREFIX,
};

/// Die Vorgabe von `fancy-regex` (1 000 000) wäre keine Entscheidung dieses
/// Projekts — und sie reicht bei `phone_de` nur für rund 80 000 Zeichen in
/// einem Run. Beim Übersetzen geprüft, damit sie nicht unbemerkt zurückfällt.
const _: () = assert!(BACKTRACK_LIMIT > 1_000_000);

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

/// Sucht mit *einem* Muster und liefert die reine Suchzeit dazu.
fn suche(id: &str, text: &str) -> (redact_core::Result<Vec<redact_core::Region>>, Duration) {
    let m = PatternMatcher::new(&[id.to_string()]).unwrap();
    let runs = [run(text)];
    let start = Instant::now();
    let result = m.find_matches(&runs);
    (result, start.elapsed())
}

/// Der Kern des Befunds: eine Zeile aus lauter Buchstaben.
///
/// 1 600 war die gemessene Schwelle; 8 000 und 100 000 zeigen, dass es keine
/// neue Schwelle etwas weiter oben gibt.
#[test]
fn eine_zeile_aus_buchstaben_sprengt_kein_muster() {
    for laenge in [1_400usize, 1_600, 2_000, 8_000, 100_000] {
        for text in [
            "a".repeat(laenge),         // Kleinbuchstaben
            "A".repeat(laenge),         // Großbuchstaben (der Präfix ist (?i:…))
            "ä".repeat(laenge),         // Umlaute — sie stehen in der Klasse
            "konto".repeat(laenge / 5), // lauter Beinahe-Treffer
            "Vertragskonto".repeat(laenge / 13),
        ] {
            for id in builtin_pattern_ids() {
                let (result, _) = suche(id, &text);
                assert!(
                    result.is_ok(),
                    "{id} bricht an {} Zeichen ab: {}",
                    text.chars().count(),
                    result.unwrap_err()
                );
            }
        }
    }
}

/// Und die Gegenprobe zur Ursache: die Laufzeit von `konto_nr` wächst
/// **linear**, nicht quadratisch.
///
/// Verdopplung der Zeilenlänge darf die Suche nicht vervierfachen. Gemessen
/// wird mit reichlich Luft (Faktor 2,5 statt 2), weil eine Testmaschine unter
/// Last schwankt — der Unterschied, um den es geht, ist ein Faktor 4 gegen 1.
#[test]
fn konto_nr_waechst_linear_mit_der_zeilenlaenge() {
    // Groß genug, dass die Messung über dem Rauschen liegt.
    let kurz = "a".repeat(200_000);
    let lang = "a".repeat(400_000);

    // Ein Aufwärmlauf: der erste Aufruf zahlt das Kompilieren mit.
    let _ = suche("konto_nr", &kurz);

    let (a, t_kurz) = suche("konto_nr", &kurz);
    let (b, t_lang) = suche("konto_nr", &lang);
    a.expect("200 000 Buchstaben");
    b.expect("400 000 Buchstaben");

    let faktor = t_lang.as_secs_f64() / t_kurz.as_secs_f64().max(1e-9);
    println!(
        "konto_nr: 200 000 Zeichen {t_kurz:?}, 400 000 Zeichen {t_lang:?} — Faktor {faktor:.2}"
    );
    assert!(
        faktor < 2.5,
        "die Suche wächst überlinear (Faktor {faktor:.2} bei doppelter Länge): \
         {t_kurz:?} → {t_lang:?}"
    );
}

/// Der zweite Befund: ein sehr langer Textlauf allein darf kein Muster
/// abbrechen lassen.
///
/// 200 000 Zeichen liegen weit über der Vorgabe von `fancy_regex`
/// (1 000 000 Rückschritte reichen bei `phone_de` für rund 80 000 Zeichen) und
/// belegen damit, dass [`BACKTRACK_LIMIT`] wirklich gesetzt ist.
#[test]
fn ein_sehr_langer_textlauf_sprengt_kein_muster() {
    // Ein Text ohne jeden Treffer, dafür mit allem, was die Muster anlockt:
    // Leerzeichen, Ziffern, Buchstaben, Satzzeichen.
    let text = "Buchung ohne Befund 12 34 - / . ".repeat(6_250);
    assert!(text.len() >= 200_000);
    for id in builtin_pattern_ids() {
        let (result, dauer) = suche(id, &text);
        assert!(
            result.is_ok(),
            "{id} bricht an {} Zeichen ab: {}",
            text.len(),
            result.unwrap_err()
        );
        println!("{id}: {} Zeichen in {dauer:?}", text.len());
    }
}

/// Jedem Muster sein eigener schlimmster Fall.
///
/// Der Test darüber füttert alle Muster mit *demselben* Text; das findet nur,
/// was allen wehtut. Hier bekommt jede Bauform die Eingabe, die genau sie
/// reizt — insbesondere die **zweite** Stelle mit einem unbegrenzten Quantor
/// vor einem Literal: `steuerliche[ ]+identifikationsnummer` in `steuer_id`,
/// und das `[:.]?[ \t]*` zwischen Schlüsselwort und Ziffern, das alle vier
/// kontextgestützten Muster aus dem Makro `mit_kontext!` erben.
///
/// **Befund:** beide sind linear, und zwar aus demselben Grund. Ein `+` hinter
/// einem Literal ist nur so oft erreichbar, wie das Literal im Text vorkommt —
/// die Zahl der Startpositionen hängt damit nicht an der Textlänge. Genau das
/// fehlte `konto_nr`: dort stand der Quantor **vor** dem Literal und war von
/// jeder Position aus erreichbar.
///
/// Gemessen an je 100 000 Zeichen; die Zeiten stehen in der Ausgabe.
#[test]
fn jede_bauform_bekommt_ihre_eigene_reizende_eingabe() {
    let faelle: &[(&str, String)] = &[
        // Die zweite `+`-vor-Literal-Stelle: erst das Schlüsselwort, dann ein
        // sehr langer Zwischenraum, den `[ ]+` schlucken will.
        ("steuer_id", format!("steuerliche{}x", " ".repeat(100_000))),
        // Dieselbe Stelle, aber mit vielen Startpositionen statt einem langen
        // Lauf — das wäre der quadratische Fall, wenn es einer wäre.
        ("steuer_id", "steuerliche ".repeat(8_000)),
        // Das `[ \t]*` aus `mit_kontext!`, für jedes der vier Muster.
        ("konto_nr", format!("Kto.{}1", " ".repeat(100_000))),
        ("blz", format!("BLZ{}1", " ".repeat(100_000))),
        ("phone_de", format!("Telefon{}1", " ".repeat(100_000))),
        ("konto_nr", "Kto. ".repeat(20_000)),
        // Der Local-Part von `email` hinter seinem Look-behind.
        ("email", "a".repeat(100_000) + "@"),
        ("email", format!("a@b{}1", ".bbbbbbbb".repeat(11_000))),
        // Die Wiederholungsgruppe von `amount_eur` ohne abschließendes Komma.
        ("amount_eur", format!("1{}", ".111".repeat(25_000))),
        // Ziffern mit Trennzeichen — `credit_card` und `phone_de`.
        ("credit_card", "1 ".repeat(50_000)),
        ("credit_card", "1-".repeat(50_000)),
        ("phone_de", format!("+49{}", " 1".repeat(50_000))),
        // Großbuchstaben für die IBAN-artigen Muster.
        ("iban_de", "A".repeat(100_000)),
        ("iban_intl", "A".repeat(100_000)),
        ("glaeubiger_id", format!("DE00{}", "A".repeat(100_000))),
        ("bic", "A".repeat(100_000)),
    ];

    for (id, text) in faelle {
        let (result, dauer) = suche(id, text);
        assert!(
            result.is_ok(),
            "{id} bricht an {} Zeichen ab: {}",
            text.len(),
            result.unwrap_err()
        );
        println!("{id}: {} Zeichen in {dauer:?}", text.len());
    }
}

/// Die Kontexterkennung, um deretwillen der Präfix überhaupt da ist, bleibt
/// erhalten — die Begrenzung darf sie nicht beschneiden.
///
/// `crates/redact-patterns/tests/precision.rs` hält je Muster 100 % Präzision
/// und 100 % Trefferquote fest; das hier ist die Ergänzung um genau die
/// Zusammensetzungen, für die der Präfix überhaupt da ist.
#[test]
fn die_kontexterkennung_ueberlebt_den_begrenzten_praefix() {
    let m = PatternMatcher::new(&["konto_nr".to_string()]).unwrap();
    for wort in KONTO_WOERTER {
        // Die Obergrenze ist für genau diese Wörter bemessen — wächst die
        // Liste über sie hinaus, muss die Zahl mitwachsen.
        let vor_konto = wort.to_lowercase();
        let vor_konto = vor_konto
            .split("konto")
            .next()
            .unwrap_or("")
            .chars()
            .count();
        assert!(
            vor_konto <= MAX_KEYWORD_PREFIX,
            "„{wort}“ hat {vor_konto} Zeichen vor „konto“, erlaubt sind \
             {MAX_KEYWORD_PREFIX} (MAX_KEYWORD_PREFIX anheben)"
        );

        let zeile = format!("{wort} 30012345 Abschlag Strom");
        let treffer = m.find_matches(&[run(&zeile)]).unwrap();
        let texte: Vec<&str> = treffer
            .iter()
            .map(|r| r.text.as_deref().unwrap_or(""))
            .collect();
        assert_eq!(
            texte,
            vec!["30012345"],
            "„{wort}“ wird nicht mehr als Kontobezug erkannt"
        );
    }
}

/// Schlüsselwörter und Zusammensetzungen, die als Kontobezug gelten müssen.
///
/// Das längste Bestimmungswort hier ist „Wertpapierverrechnungs“ mit 22
/// Zeichen — dafür ist [`MAX_KEYWORD_PREFIX`] bemessen.
const KONTO_WOERTER: &[&str] = &[
    "Konto",
    "Kto.",
    "Kto",
    "Kontonummer",
    "Konto-Nr.",
    "Kontonr",
    "Girokonto",
    "Vertragskonto",
    "Verrechnungskonto",
    "Gemeinschaftskonto",
    "Fremdwaehrungskonto",
    "Wertpapierverrechnungskonto",
];

/// **Die eigentliche Gegenprobe: die Korrektur ändert am Ergebnis nichts.**
///
/// Das ist die stärkste Aussage, die sich über diese Änderung treffen lässt,
/// und sie ist nicht selbstverständlich — deshalb steht sie hier ausgeschrieben
/// statt in einer Zeile Kommentar.
///
/// Warum sie gilt: die Suche probiert ohnehin *jede* Startposition durch. Ein
/// „konto“ mit 40 Buchstaben davor wird deshalb auch mit `{0,24}` gefunden —
/// die Suche setzt dann eben 24 Zeichen weiter rechts auf. Begrenzt wird
/// allein, wie weit die **Fanggruppe** `context` nach links reicht, und die
/// wird nirgends geschwärzt: geschwärzt wird `target`. Die Obergrenze nimmt
/// der Suche also die quadratische Arbeit, ohne ihr einen Treffer zu nehmen.
///
/// Gemessen wird das, indem der alte Regex (mit `*`) und der neue (mit
/// `{0,24}`) über dieselben Zeilen laufen und Zeichen für Zeichen dasselbe
/// liefern müssen.
#[test]
fn der_begrenzte_praefix_findet_genau_dasselbe_wie_der_alte() {
    let neu = builtin_patterns()
        .into_iter()
        .find(|d| d.id == "konto_nr")
        .expect("konto_nr");
    // Genau der Regex von vor der Korrektur.
    let alt = PatternDef {
        regex: neu
            .regex
            .replace(&format!("{{0,{MAX_KEYWORD_PREFIX}}}"), "*"),
        ..neu.clone()
    };
    assert_ne!(
        alt.regex, neu.regex,
        "der alte Regex muss sich unterscheiden"
    );

    let mit_altem = PatternMatcher::with_defs(vec![alt]).unwrap();
    let mit_neuem = PatternMatcher::with_defs(vec![neu]).unwrap();

    let mut zeilen: Vec<String> = KONTO_WOERTER
        .iter()
        .map(|w| format!("{w} 30012345 Abschlag Strom"))
        .collect();
    zeilen.extend(
        [
            // Die Zeilen aus `precision.rs`.
            "Kontonummer: 532013000",
            "Konto-Nr. 1234567 bei der Musterbank",
            "Kto. 4711 0815 BLZ 50010517  Rechnung Nr. 2026-0042",
            "Vertragskonto 30012345 Abschlag Strom",
            "Lohn/Gehalt 01/2026 Personalnummer 8891234",
            "27.01.2026  Dauerauftrag Versicherung Police 90123456",
            "Bankleitzahl: 37040044",
            "Telefon: +49 30 123456789",
            "IBAN: DE89 3704 0044 0532 0130 00",
            "Musterbank AG - Kontoauszug Nr. 1/2026",
            // Und die Fälle, an denen sich die beiden unterscheiden *könnten*:
            // ein Bestimmungswort, das länger ist als die Obergrenze.
            "Aussergewoehnlichlangesvertragskonto 30012345",
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxkonto 30012345",
            "einwortmitgenauvierundzwanzigzeichenkonto 532013000",
        ]
        .iter()
        .map(|s| s.to_string()),
    );

    for zeile in &zeilen {
        let a = mit_altem.find_matches(&[run(zeile)]).unwrap();
        let b = mit_neuem.find_matches(&[run(zeile)]).unwrap();
        let beschreibe = |regionen: &[redact_core::Region]| -> Vec<(String, f32)> {
            regionen
                .iter()
                .map(|r| {
                    let konfidenz = match &r.source {
                        redact_core::Source::Pattern { confidence, .. } => *confidence,
                        _ => unreachable!("nur Pattern-Treffer"),
                    };
                    (r.text.clone().unwrap_or_default(), konfidenz)
                })
                .collect()
        };
        assert_eq!(
            beschreibe(&a),
            beschreibe(&b),
            "die Begrenzung ändert das Ergebnis an {zeile:?}"
        );
    }
    println!(
        "{} Zeilen: alter und neuer Regex liefern identische Treffer",
        zeilen.len()
    );
}

/// Realistischer Kontoauszugstext — die Gegenprobe „es ist nicht nur schnell,
/// weil es nichts mehr findet".
#[test]
fn realistischer_auszugstext_bleibt_schnell_und_findet_weiterhin() {
    const ZEILEN: &[&str] = &[
        "Musterbank AG - Kontoauszug Nr. 1/2026",
        "IBAN: DE89 3704 0044 0532 0130 00",
        "Kontonummer: 532013000",
        "Bankleitzahl: 37040044",
        "            Kto. 4711 0815 BLZ 50010517  Rechnung Nr. 2026-0042",
        "            Vertragskonto 30012345 Abschlag Strom",
        "Kontakt: max.mustermann@example.org",
        "Telefon: +49 30 123456789",
        "Steuer-ID: 12345678901",
    ];
    // Rund 8 000 Zeichen echter Mischtext, wie in der Messung des Befunds.
    let text = ZEILEN.join("  ").repeat(25);
    assert!(text.len() >= 8_000, "{} Zeichen", text.len());

    let m = PatternMatcher::new(&[]).unwrap();
    let runs = [run(&text)];
    let start = Instant::now();
    let treffer = m.find_matches(&runs).expect("realistischer Text");
    let dauer = start.elapsed();
    println!(
        "Mischtext: {} Zeichen, {} Treffer, alle Muster in {dauer:?}",
        text.len(),
        treffer.len()
    );

    // Gefunden wird weiterhin, was gefunden werden soll.
    let texte: Vec<&str> = treffer
        .iter()
        .map(|r| r.text.as_deref().unwrap_or(""))
        .collect();
    for erwartet in [
        "DE89 3704 0044 0532 0130 00",
        "532013000",
        "37040044",
        "4711 0815",
        "50010517",
        "30012345",
        "max.mustermann@example.org",
        "+49 30 123456789",
        "12345678901",
    ] {
        assert!(texte.contains(&erwartet), "{erwartet:?} fehlt in {texte:?}");
    }
}
