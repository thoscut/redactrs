//! Integrationstests für das Pattern-Matching auf synthetischen Text-Runs.

use redact_core::{Analyzer, Glyph, Rect, Region, Source, TextRun};
use redact_patterns::{builtin_pattern_ids, PatternMatcher};

/// Baut einen Text-Run, in dem jedes Zeichen 5 pt breit und 10 pt hoch ist.
fn run(page: usize, text: &str) -> TextRun {
    let glyphs = text
        .chars()
        .enumerate()
        .map(|(i, ch)| Glyph {
            ch,
            rect: Rect::new(i as f64 * 5.0, 0.0, (i as f64 + 1.0) * 5.0, 10.0),
        })
        .collect();
    TextRun::new(page, glyphs)
}

/// Erwartetes Rechteck für einen Zeichenbereich (nur für ASCII-Texte gültig).
fn rect_for_chars(from: usize, to: usize) -> Rect {
    Rect::new(from as f64 * 5.0, 0.0, to as f64 * 5.0, 10.0)
}

fn matcher_for(ids: &[&str]) -> PatternMatcher {
    let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
    PatternMatcher::new(&ids).unwrap()
}

fn texts(regions: &[Region]) -> Vec<&str> {
    regions
        .iter()
        .map(|r| r.text.as_deref().unwrap_or(""))
        .collect()
}

#[test]
fn finds_iban_with_and_without_spaces() {
    let m = matcher_for(&["iban_de"]);
    let runs = vec![
        run(0, "IBAN: DE89 3704 0044 0532 0130 00"),
        run(1, "IBAN: DE89370400440532013000 fertig"),
    ];
    let regions = m.find_matches(&runs).unwrap();
    assert_eq!(
        texts(&regions),
        vec!["DE89 3704 0044 0532 0130 00", "DE89370400440532013000"]
    );
    assert_eq!(regions[0].page, 0);
    assert_eq!(regions[1].page, 1);
    for r in &regions {
        match &r.source {
            Source::Pattern {
                pattern_id,
                confidence,
            } => {
                assert_eq!(pattern_id, "iban_de");
                // Bestandene Prüfsumme hebt die Konfidenz an.
                assert!((*confidence - 0.99).abs() < 1e-6, "{confidence}");
            }
            other => panic!("falsche Quelle: {other:?}"),
        }
    }
}

#[test]
fn iban_rect_equals_union_of_glyph_rects() {
    let m = matcher_for(&["iban_de"]);
    let text = "IBAN: DE89370400440532013000 x";
    let runs = vec![run(0, text)];
    let regions = m.find_matches(&runs).unwrap();
    assert_eq!(regions.len(), 1);
    // "IBAN: " sind 6 Zeichen, die IBAN ist 22 Zeichen lang.
    assert_eq!(regions[0].rect, rect_for_chars(6, 28));

    // Gegenprobe: Union der Glyph-Rechtecke des Treffers.
    let start = text.find("DE89").unwrap();
    let matched = regions[0].text.as_ref().unwrap();
    let union = redact_core::bounding_box(
        runs[0].glyphs[start..start + matched.chars().count()]
            .iter()
            .map(|g| &g.rect),
    )
    .unwrap();
    assert_eq!(regions[0].rect, union);
}

#[test]
fn iban_with_broken_check_digits_is_discarded() {
    let m = matcher_for(&["iban_de"]);
    let regions = m
        .find_matches(&[run(0, "IBAN: DE88 3704 0044 0532 0130 00")])
        .unwrap();
    assert!(regions.is_empty(), "{:?}", texts(&regions));
}

#[test]
fn surrounding_whitespace_is_trimmed_from_the_box() {
    // Ein Pattern, das bewusst Leerzeichen einschließt.
    let yaml = r#"
extend_builtins: false
patterns:
  - id: mit_leerzeichen
    regex: '\s+ABC\s+'
"#;
    let m = PatternMatcher::from_yaml(yaml).unwrap();
    let regions = m.find_matches(&[run(0, "xx ABC yy")]).unwrap();
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].text.as_deref(), Some("ABC"));
    assert_eq!(regions[0].rect, rect_for_chars(3, 6));
}

#[test]
fn konto_nr_does_not_fire_inside_longer_digit_runs() {
    let m = matcher_for(&["konto_nr"]);
    let regions = m
        .find_matches(&[
            run(0, "Referenz 1234567890123 Ende"),
            run(1, "IBAN DE89370400440532013000"),
            run(2, "IBAN DE89 3704 0044 0532 0130 00"),
        ])
        .unwrap();
    assert!(regions.is_empty(), "{:?}", texts(&regions));

    // Positivprobe: eine Kontonummer hinter ihrem Schlüsselwort wird gefunden …
    let hits = m
        .find_matches(&[run(0, "Kto. 1234567 bei der Bank")])
        .unwrap();
    assert_eq!(texts(&hits), vec!["1234567"]);
    match &hits[0].source {
        Source::Pattern { confidence, .. } => assert!((*confidence - 0.85).abs() < 1e-6),
        other => panic!("falsche Quelle: {other:?}"),
    }

    // … das Schlüsselwort selbst bleibt lesbar: geschwärzt wird nur die
    // Gruppe `target`, hier die sechs Zeichen ab Position 5.
    assert_eq!(hits[0].rect, rect_for_chars(5, 12));
}

/// Dieselbe Ziffernkette ohne Schlüsselwort ist nur ein Verdacht: sie kommt
/// erst durch, wenn die Schwelle bewusst gesenkt wird — und dann mit einer
/// Konfidenz, die sie von einem echten Treffer unterscheidbar macht.
#[test]
fn konto_nr_without_a_keyword_needs_a_lowered_threshold() {
    let m = matcher_for(&["konto_nr"]);
    let runs = vec![run(0, "Rechnung 1234567 vom Vormonat")];
    assert!(m.find_matches(&runs).unwrap().is_empty());

    let lax = matcher_for(&["konto_nr"]).with_min_confidence(0.1).unwrap();
    let hits = lax.find_matches(&runs).unwrap();
    assert_eq!(texts(&hits), vec!["1234567"]);
    match &hits[0].source {
        Source::Pattern { confidence, .. } => assert!((*confidence - 0.3).abs() < 1e-6),
        other => panic!("falsche Quelle: {other:?}"),
    }
}

/// Der Kern des Befunds: jedes achtstellige Token traf früher `konto_nr` *und*
/// `blz`. Jetzt entscheidet das Schlüsselwort, und wo keines steht, trennt die
/// Konfidenz die beiden Deutungen.
#[test]
fn konto_nr_and_blz_are_told_apart_by_their_keyword() {
    let m = matcher_for(&["konto_nr", "blz"]);
    let runs = vec![
        run(0, "Bankleitzahl: 37040044"),
        run(1, "Kontonummer: 87654321"),
    ];
    let found: Vec<(String, String)> = m
        .find_matches(&runs)
        .unwrap()
        .iter()
        .map(|r| match &r.source {
            Source::Pattern { pattern_id, .. } => (
                pattern_id.clone(),
                r.text.as_deref().unwrap_or_default().to_string(),
            ),
            other => panic!("falsche Quelle: {other:?}"),
        })
        .collect();
    // Reihenfolge: Runs zuerst — Zeile 0 ist die BLZ, Zeile 1 die Kontonummer.
    // Entscheidend ist, dass jede Zeile genau ein Pattern auslöst.
    assert_eq!(
        found,
        vec![
            ("blz".to_string(), "37040044".to_string()),
            ("konto_nr".to_string(), "87654321".to_string()),
        ]
    );

    // Ohne Schlüsselwort bleiben beide Deutungen möglich — aber nicht
    // gleichrangig: `konto_nr` liegt vorn, und beide sind unterhalb der
    // Vorgabeschwelle.
    let lax = matcher_for(&["konto_nr", "blz"])
        .with_min_confidence(0.0)
        .unwrap();
    let hits = lax
        .find_matches(&[run(0, "Referenz 87654321 vom 1.2.")])
        .unwrap();
    let mut ranked: Vec<(String, f32)> = hits
        .iter()
        .map(|r| match &r.source {
            Source::Pattern {
                pattern_id,
                confidence,
            } => (pattern_id.clone(), *confidence),
            _ => unreachable!(),
        })
        .collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    assert_eq!(ranked[0].0, "konto_nr");
    assert_eq!(ranked[1].0, "blz");
    assert!(ranked[0].1 > ranked[1].1, "{ranked:?}");
    assert!(
        ranked[0].1 < redact_patterns::DEFAULT_MIN_CONFIDENCE,
        "{ranked:?}"
    );
}

/// IBAN und Gläubiger-ID benutzen dieselbe mod-97-Rechnung und sehen sich zum
/// Verwechseln ähnlich — auseinandergehalten werden sie allein von der
/// Prüfsumme: bei der Gläubiger-ID fallen vorher die drei Zeichen
/// Geschäftsbereichskennung heraus, bei der IBAN nicht. Beide Muster
/// gleichzeitig aktiv, jede Zeile darf genau ein Muster auslösen.
#[test]
fn iban_and_glaeubiger_id_do_not_claim_each_other() {
    let m = matcher_for(&["iban_de", "iban_intl", "glaeubiger_id"]);
    let runs = vec![
        run(0, "Glaeubiger-ID: DE98ZZZ09999999999"),
        run(1, "IBAN DE89370400440532013000"),
        run(2, "IBAN: DE89 3704 0044 0532 0130 00"),
    ];
    let regions = m.find_matches(&runs).unwrap();
    let found: Vec<(&str, &str)> = regions
        .iter()
        .map(|r| match &r.source {
            Source::Pattern { pattern_id, .. } => {
                (pattern_id.as_str(), r.text.as_deref().unwrap_or_default())
            }
            other => panic!("falsche Quelle: {other:?}"),
        })
        .collect();
    assert_eq!(
        found,
        vec![
            ("glaeubiger_id", "DE98ZZZ09999999999"),
            ("iban_de", "DE89370400440532013000"),
            ("iban_intl", "DE89370400440532013000"),
            ("iban_de", "DE89 3704 0044 0532 0130 00"),
            ("iban_intl", "DE89 3704 0044 0532 0130 00"),
        ]
    );
}

/// Die bestandene Prüfsumme hebt die Konfidenz auf 0.99 — dieselbe Stufe wie
/// bei IBAN, BIC und Kreditkarte. Deshalb braucht das Muster kein
/// Kontext-Schlüsselwort: es kommt auch nackt durch das Mindestvertrauen.
#[test]
fn glaeubiger_id_passes_the_default_threshold_without_a_keyword() {
    let m = matcher_for(&["glaeubiger_id"]);
    let hits = m
        .find_matches(&[run(
            0,
            "Lastschrift Stadtwerke DE24ZZZ00000561652 Mandat 4711",
        )])
        .unwrap();
    assert_eq!(texts(&hits), vec!["DE24ZZZ00000561652"]);
    match &hits[0].source {
        Source::Pattern { confidence, .. } => {
            assert!((*confidence - 0.99).abs() < 1e-6, "{confidence}");
            assert!(*confidence >= redact_patterns::DEFAULT_MIN_CONFIDENCE);
        }
        other => panic!("falsche Quelle: {other:?}"),
    }
}

#[test]
fn bic_validator_rejects_plain_uppercase_words() {
    let m = matcher_for(&["bic"]);
    let regions = m
        .find_matches(&[run(0, "RECHNUNG BIC DEUTDEFF und COBADEFFXXX")])
        .unwrap();
    assert_eq!(texts(&regions), vec!["DEUTDEFF", "COBADEFFXXX"]);

    // Auch andere achtbuchstabige Wörter fallen durch die Länderprüfung.
    let regions = m.find_matches(&[run(0, "ABSENDER MAHNUNGS")]).unwrap();
    assert!(regions.is_empty(), "{:?}", texts(&regions));
}

#[test]
fn luhn_discards_invalid_card_numbers() {
    let m = matcher_for(&["credit_card"]);
    let regions = m
        .find_matches(&[
            run(0, "Karte 4539 1488 0343 6467 gueltig"),
            run(1, "Karte 4539 1488 0343 6468 falsch"),
        ])
        .unwrap();
    assert_eq!(texts(&regions), vec!["4539 1488 0343 6467"]);
    assert_eq!(regions[0].page, 0);
}

#[test]
fn amount_date_email_and_phone_patterns_work() {
    let m = matcher_for(&["amount_eur", "date_de", "email", "phone_de"]);
    let runs = vec![
        run(0, "Betrag 1.234,56 EUR am 05.03.2024"),
        run(1, "Kontakt max.muster@example.com Tel. +49 30 123456"),
    ];
    let regions = m.find_matches(&runs).unwrap();
    assert_eq!(
        texts(&regions),
        vec![
            "1.234,56 EUR",
            "05.03.2024",
            "max.muster@example.com",
            "+49 30 123456"
        ]
    );
}

#[test]
fn empty_id_list_selects_all_builtins() {
    let m = PatternMatcher::new(&[]).unwrap();
    assert_eq!(m.defs().len(), builtin_pattern_ids().len());
    // Drei Patterns sind standardmäßig deaktiviert (`iban_intl`, `amount_eur`,
    // `date_de`) und werden nicht kompiliert.
    assert_eq!(m.active_count(), builtin_pattern_ids().len() - 3);
}

/// Datum und Betrag sind der Inhalt eines Kontoauszugs, nicht sein Schutzgut —
/// standardmäßig bleiben sie stehen, per `--patterns` sind sie erreichbar.
#[test]
fn dates_and_amounts_are_off_by_default_but_still_reachable() {
    let zeile = run(
        0,
        "05.01.2026  Ueberweisung an Musterfirma GmbH   1.234,56 EUR",
    );

    let voreinstellung = PatternMatcher::new(&[]).unwrap();
    let regions = voreinstellung
        .find_matches(std::slice::from_ref(&zeile))
        .unwrap();
    assert!(
        regions.is_empty(),
        "Standardlauf schwärzt Datum/Betrag: {:?}",
        texts(&regions)
    );

    let gezielt = matcher_for(&["date_de", "amount_eur"]);
    let regions = gezielt.find_matches(&[zeile]).unwrap();
    assert_eq!(texts(&regions), vec!["05.01.2026", "1.234,56 EUR"]);
}

#[test]
fn explicitly_selected_pattern_is_enabled_even_if_off_by_default() {
    let m = matcher_for(&["iban_intl"]);
    assert_eq!(m.active_count(), 1);
    let regions = m
        .find_matches(&[run(0, "IBAN GB82 WEST 1234 5698 7654 32")])
        .unwrap();
    assert_eq!(texts(&regions), vec!["GB82 WEST 1234 5698 7654 32"]);
}

#[test]
fn unknown_id_lists_the_valid_ids() {
    let err = PatternMatcher::new(&["nope".to_string()]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("nope"), "{msg}");
    for id in builtin_pattern_ids() {
        assert!(
            msg.contains(id),
            "ID {id} fehlt in der Fehlermeldung: {msg}"
        );
    }
}

#[test]
fn yaml_config_can_disable_a_builtin_and_add_a_custom_pattern() {
    let yaml = r#"
extend_builtins: true
patterns:
  - id: iban_de
    enabled: false
  - id: kundennummer
    regex: 'KdNr\.?\s*\d{5,}'
    description: Kundennummer
    confidence: 0.8
"#;
    let m = PatternMatcher::from_yaml(yaml).unwrap();
    let iban = m.defs().iter().find(|d| d.id == "iban_de").unwrap();
    assert!(!iban.enabled);
    let kd = m.defs().iter().find(|d| d.id == "kundennummer").unwrap();
    assert_eq!(kd.description, "Kundennummer");
    assert!((kd.confidence - 0.8).abs() < 1e-6);

    let regions = m
        .find_matches(&[run(0, "KdNr. 12345 IBAN DE89370400440532013000")])
        .unwrap();
    let ids: Vec<&str> = regions
        .iter()
        .map(|r| match &r.source {
            Source::Pattern { pattern_id, .. } => pattern_id.as_str(),
            _ => unreachable!(),
        })
        .collect();
    assert!(!ids.contains(&"iban_de"), "{ids:?}");
    assert!(ids.contains(&"kundennummer"), "{ids:?}");
}

#[test]
fn yaml_config_with_unknown_id_and_without_regex_is_an_error() {
    let yaml = r#"
patterns:
  - id: gibtsnicht
    confidence: 0.5
"#;
    let err = PatternMatcher::from_yaml(yaml).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("gibtsnicht"), "{msg}");
    assert!(msg.contains("regex"), "{msg}");
}

#[test]
fn config_file_extension_selects_the_format() {
    let dir = std::env::temp_dir().join(format!("redact-patterns-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let yaml_path = dir.join("patterns.yml");
    std::fs::write(
        &yaml_path,
        "extend_builtins: false\npatterns:\n  - id: y\n    regex: 'A+'\n",
    )
    .unwrap();
    let m = PatternMatcher::from_config_file(&yaml_path).unwrap();
    assert_eq!(m.defs()[0].id, "y");

    let json_path = dir.join("patterns.json");
    std::fs::write(
        &json_path,
        r#"{"extend_builtins": false, "patterns": [{"id":"j","regex":"B+"}]}"#,
    )
    .unwrap();
    let m = PatternMatcher::from_config_file(&json_path).unwrap();
    assert_eq!(m.defs()[0].id, "j");

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn output_is_deterministic_and_ordered_by_run_then_pattern() {
    let m = PatternMatcher::new(&[]).unwrap();
    let runs = vec![
        run(0, "Rg. 05.03.2024 IBAN DE89370400440532013000 1.234,56 EUR"),
        run(1, "BIC DEUTDEFF Kto. 1234567 Tel. 030 1234567"),
    ];
    let first = m.find_matches(&runs).unwrap();
    let second = m.find_matches(&runs).unwrap();
    assert_eq!(first, second);
    assert!(!first.is_empty());

    // Seiten bleiben in Eingabereihenfolge …
    let pages: Vec<usize> = first.iter().map(|r| r.page).collect();
    let mut sorted = pages.clone();
    sorted.sort_unstable();
    assert_eq!(pages, sorted);

    // … und innerhalb einer Seite gilt die Reihenfolge aus `defs()`.
    let order: Vec<usize> = first
        .iter()
        .filter(|r| r.page == 0)
        .map(|r| match &r.source {
            Source::Pattern { pattern_id, .. } => m
                .defs()
                .iter()
                .position(|d| &d.id == pattern_id)
                .expect("ID muss in defs() vorkommen"),
            _ => unreachable!(),
        })
        .collect();
    let mut sorted_order = order.clone();
    sorted_order.sort_unstable();
    assert_eq!(order, sorted_order);
}

#[test]
fn analyzer_trait_delegates_to_find_matches() {
    let m = matcher_for(&["date_de"]);
    let runs = vec![run(3, "am 01.02.2023")];
    let via_trait = Analyzer::analyze(&m, &runs).unwrap();
    assert_eq!(via_trait, m.find_matches(&runs).unwrap());
    assert_eq!(texts(&via_trait), vec!["01.02.2023"]);
}
