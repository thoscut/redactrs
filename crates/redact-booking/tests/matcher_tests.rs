//! Tests für den Abgleich gegen Positiv- und Negativliste.

use redact_booking::BookingMatcher;
use redact_core::{
    Analyzer, BookingEntry, Glyph, ListType, MatchType, Rect, RedactError, Region, Source, TextRun,
};

/// Breite jedes synthetischen Glyphen in pt.
const W: f64 = 5.0;
/// Höhe jedes synthetischen Glyphen in pt.
const H: f64 = 10.0;

/// Baut einen Text-Run aus gleich breiten 5pt-Glyphen (ein Glyph je Zeichen).
fn run(page: usize, text: &str) -> TextRun {
    let glyphs = text
        .chars()
        .enumerate()
        .map(|(i, ch)| Glyph {
            ch,
            rect: Rect::new(i as f64 * W, 0.0, (i as f64 + 1.0) * W, H),
        })
        .collect();
    TextRun::new(page, glyphs)
}

/// Erwartetes Rechteck für die Zeichen `[from, to)` eines mit [`run`] gebauten Runs.
fn rect_for_chars(from: usize, to: usize) -> Rect {
    Rect::new(from as f64 * W, 0.0, to as f64 * W, H)
}

fn entry(id: &str, list_type: ListType, pattern: &str) -> BookingEntry {
    BookingEntry {
        id: id.to_string(),
        list_type,
        pattern: pattern.to_string(),
        context_before: None,
        context_after: None,
        is_regex: false,
    }
}

fn matcher(entries: Vec<BookingEntry>) -> BookingMatcher {
    BookingMatcher::new(entries).expect("Matcher sollte baubar sein")
}

fn booking_id(region: &Region) -> &str {
    match &region.source {
        Source::Booking { booking_id, .. } => booking_id,
        other => panic!("Booking-Source erwartet, war: {other:?}"),
    }
}

fn match_type(region: &Region) -> MatchType {
    match region.source {
        Source::Booking { match_type, .. } => match_type,
        _ => panic!("Booking-Source erwartet"),
    }
}

#[test]
fn empty_matcher_reports_empty_and_finds_nothing() {
    let m = matcher(Vec::new());
    assert!(m.is_empty());
    assert!(m.positive().is_empty());
    assert!(m.negative().is_empty());
    assert!(m
        .find_matches(&[run(0, "Musterfirma GmbH")])
        .unwrap()
        .is_empty());
}

#[test]
fn splits_positive_and_negative_in_file_order() {
    let m = matcher(vec![
        entry("b001", ListType::Positive, "A"),
        entry("b003", ListType::Negative, "B"),
        entry("b002", ListType::Positive, "C"),
    ]);
    assert_eq!(
        m.positive()
            .iter()
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>(),
        vec!["b001", "b002"]
    );
    assert_eq!(m.negative().len(), 1);
    assert!(!m.is_empty());
}

#[test]
fn matches_iban_whitespace_insensitively() {
    // Im PDF steht die IBAN mit abweichender *Menge* an Leerraum — hier ein
    // doppeltes und ein dreifaches Leerzeichen. Genau dagegen ist die
    // Normalisierung gebaut.
    let text = "Konto: DE89  3704 0044   0532 0130 00";
    let m = matcher(vec![entry(
        "b002",
        ListType::Positive,
        "DE89 3704 0044 0532 0130 00",
    )]);
    let regions = m.find_matches(&[run(0, text)]).unwrap();
    assert_eq!(regions.len(), 1);
    assert_eq!(
        regions[0].text.as_deref(),
        Some("DE89  3704 0044   0532 0130 00")
    );
    // Treffer beginnt bei Zeichen 7 und reicht bis zum Textende (37 Zeichen).
    assert_eq!(regions[0].rect, rect_for_chars(7, 37));
}

/// Leerraum wird **zusammengefasst, nicht entfernt**.
///
/// Nachgemessen am fertigen Programm: eine Seite mit
/// `IBAN: DE89370400440532013000` und dem Buchungslisten-Eintrag
/// `DE89 3704 0044 0532 0130 00` ergibt `Textzeilen: 1, Treffer gesamt: 0`.
/// Wer beide Schreibweisen abdecken will, braucht zwei Einträge — oder das
/// Muster `iban_de`, das die ungruppierte Form selbst erkennt.
#[test]
fn collapsing_whitespace_is_not_the_same_as_ignoring_it() {
    let m = matcher(vec![entry(
        "b002",
        ListType::Positive,
        "DE89 3704 0044 0532 0130 00",
    )]);
    let regions = m
        .find_matches(&[run(0, "IBAN: DE89370400440532013000")])
        .unwrap();
    assert!(
        regions.is_empty(),
        "ohne Leerzeichen trifft ein Muster mit Leerzeichen nicht"
    );
}

/// Hält die **tatsächliche** Lage fest: über einen Zeilenumbruch hinweg wird
/// nicht gefunden.
///
/// Der Extraktor (`redact_pdf::extract::PdfExtractor::build_lines`) gruppiert
/// Glyphen entlang der Grundlinie und liefert je Zeile einen eigenen
/// `TextRun`; `assemble_line` setzt als Trennzeichen ausschließlich `' '`.
/// Ein `TextRun` enthält deshalb **nie** ein `\n`, und eine über zwei Zeilen
/// verteilte IBAN erreicht den Matcher als zwei getrennte Runs.
///
/// Nachgemessen am fertigen Programm mit einer Seite, auf der
/// `IBAN: DE89 3704` und `0044 0532 0130 00` in zwei Zeilen stehen:
/// `Textzeilen: 2, Treffer gesamt: 0` — sowohl mit `--booking-list` als auch
/// mit `--patterns iban_de`.
///
/// Dieser Test darf nicht „repariert“ werden, indem man die beiden Runs im
/// Test zu einem zusammenzieht: dann prüft er den Extraktor nicht mehr nach,
/// sondern nur noch sich selbst. Soll der Zeilenumbruch wirklich überbrückt
/// werden, muss der Abgleich über Run-Grenzen hinweg suchen — das ist ein
/// Eingriff in `redact-pdf` (Leserichtung/Nachbarschaft der Zeilen) und in die
/// Rechteckbildung (ein Treffer bekäme dann mehrere Rechtecke).
#[test]
fn iban_split_across_two_extracted_lines_is_not_found() {
    let m = matcher(vec![entry(
        "b002",
        ListType::Positive,
        "DE89 3704 0044 0532 0130 00",
    )]);

    // Genau das, was der Extraktor für zwei Zeilen liefert: zwei Runs.
    let runs = vec![run(0, "IBAN: DE89 3704"), run(0, "0044 0532 0130 00")];

    // Vorbedingung: kein Run trägt einen Zeilenumbruch.
    assert!(runs.iter().all(|r| !r.text.contains('\n')));

    assert!(
        m.find_matches(&runs).unwrap().is_empty(),
        "Buchungslisten-Abgleich läuft je Run — über Zeilengrenzen hinweg darf \
         (und kann) er nicht treffen"
    );

    // Gegenprobe: derselbe Eintrag trifft, sobald beides in *einer* Zeile steht.
    let one_line = vec![run(0, "IBAN: DE89 3704 0044 0532 0130 00")];
    assert_eq!(m.find_matches(&one_line).unwrap().len(), 1);
}

#[test]
fn matches_names_case_insensitively() {
    let m = matcher(vec![entry("b001", ListType::Positive, "musterfirma gmbh")]);
    let regions = m
        .find_matches(&[run(2, "Zahlung an MUSTERFIRMA GmbH")])
        .unwrap();
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].page, 2);
    assert_eq!(regions[0].text.as_deref(), Some("MUSTERFIRMA GmbH"));
}

#[test]
fn byte_offsets_map_to_exact_bounding_box() {
    // Synthetische 5pt-Glyphen: Zeichen i liegt auf [i*5, (i+1)*5).
    let text = "abc Max Mustermann xyz";
    let m = matcher(vec![entry("b003", ListType::Positive, "Max Mustermann")]);
    let regions = m.find_matches(&[run(0, text)]).unwrap();
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].rect, Rect::new(20.0, 0.0, 90.0, 10.0));
    assert_eq!(regions[0].text.as_deref(), Some("Max Mustermann"));
}

#[test]
fn byte_offsets_survive_multibyte_characters() {
    // 'ü' und 'ß' belegen zwei Bytes — die Rechtecke müssen trotzdem stimmen.
    let text = "Konto von Jürgen Straße GmbH";
    let m = matcher(vec![entry("b010", ListType::Positive, "jürgen straße")]);
    let regions = m.find_matches(&[run(0, text)]).unwrap();
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].text.as_deref(), Some("Jürgen Straße"));
    // Zeichen 10 bis 23 (nicht Bytes!).
    assert_eq!(regions[0].rect, rect_for_chars(10, 23));
}

#[test]
fn negative_hits_are_expanded_by_two_points() {
    let m = matcher(vec![entry("b003", ListType::Negative, "Max Mustermann")]);
    let regions = m.find_matches(&[run(0, "Max Mustermann")]).unwrap();
    assert_eq!(regions.len(), 1);
    assert_eq!(match_type(&regions[0]), MatchType::Negative);
    assert!(regions[0].is_blocking());
    // 14 Zeichen à 5pt, danach 2pt Rand in alle Richtungen.
    assert_eq!(regions[0].rect, Rect::new(-2.0, -2.0, 72.0, 12.0));
}

#[test]
fn context_before_gates_the_match_and_search_continues() {
    let mut e = entry("b001", ListType::Positive, "Musterfirma GmbH");
    e.context_before = Some("Überweisung an".to_string());
    let m = matcher(vec![e]);

    // Erste Fundstelle scheitert am Kontext, die zweite nicht.
    let text = "Zahlung an Musterfirma GmbH, Überweisung an Musterfirma GmbH";
    let regions = m.find_matches(&[run(0, text)]).unwrap();
    assert_eq!(regions.len(), 1, "nur die zweite Fundstelle zählt");
    let start = text.find("Überweisung").unwrap();
    assert!(regions[0].rect.ll.x > start as f64 * W);

    // Ohne den Kontext gibt es gar keinen Treffer.
    assert!(m
        .find_matches(&[run(0, "Zahlung an Musterfirma GmbH")])
        .unwrap()
        .is_empty());
}

#[test]
fn context_after_gates_the_match() {
    let mut e = entry("b003", ListType::Positive, "Max Mustermann");
    e.context_after = Some("Kontoinhaber".to_string());
    let m = matcher(vec![e]);

    let hit = m
        .find_matches(&[run(0, "Max Mustermann ist KONTOINHABER")])
        .unwrap();
    assert_eq!(hit.len(), 1);
    let miss = m
        .find_matches(&[run(0, "Max Mustermann ist Empfänger")])
        .unwrap();
    assert!(miss.is_empty());
}

#[test]
fn finds_every_occurrence_left_to_right() {
    let m = matcher(vec![entry("b001", ListType::Positive, "abc")]);
    let regions = m.find_matches(&[run(0, "abc xx abc")]).unwrap();
    assert_eq!(regions.len(), 2);
    assert_eq!(regions[0].rect, rect_for_chars(0, 3));
    assert_eq!(regions[1].rect, rect_for_chars(7, 10));
}

#[test]
fn ordering_is_deterministic_negative_first_then_file_order() {
    let m = matcher(vec![
        entry("p1", ListType::Positive, "alpha"),
        entry("n1", ListType::Negative, "beta"),
        entry("p2", ListType::Positive, "gamma"),
        entry("n2", ListType::Negative, "delta"),
    ]);
    let runs = vec![run(0, "alpha beta gamma delta"), run(1, "delta alpha")];
    let ids: Vec<String> = m
        .find_matches(&runs)
        .unwrap()
        .iter()
        .map(|r| format!("{}:{}", r.page, booking_id(r)))
        .collect();
    assert_eq!(
        ids,
        vec!["0:n1", "0:n2", "0:p1", "0:p2", "1:n2", "1:p1"],
        "Negativliste zuerst, dann Positivliste, jeweils in Dateireihenfolge"
    );
    // Zweiter Durchlauf muss identisch sein.
    assert_eq!(
        m.find_matches(&runs).unwrap(),
        m.find_matches(&runs).unwrap()
    );
}

#[test]
fn match_region_prefers_negative_list() {
    let m = matcher(vec![
        entry("b001", ListType::Positive, "Musterfirma GmbH"),
        entry("b003", ListType::Negative, "Max Mustermann"),
    ]);
    let region = Region::new(
        0,
        Rect::new(0.0, 0.0, 100.0, 10.0),
        Some("Max Mustermann, Musterfirma GmbH".into()),
        Source::Manual {
            reason: "test".into(),
        },
    );
    assert_eq!(
        m.match_region(&region),
        Some(Source::Booking {
            booking_id: "b003".into(),
            match_type: MatchType::Negative,
        })
    );
}

#[test]
fn match_region_returns_positive_and_none() {
    let m = matcher(vec![entry("b001", ListType::Positive, "Musterfirma GmbH")]);
    let with_hit = Region::new(
        1,
        Rect::new(0.0, 0.0, 10.0, 10.0),
        Some("zahlung an musterfirma  gmbh".into()),
        Source::Manual {
            reason: "test".into(),
        },
    );
    assert_eq!(
        m.match_region(&with_hit),
        Some(Source::Booking {
            booking_id: "b001".into(),
            match_type: MatchType::Positive,
        })
    );

    let without_text = Region::new(
        1,
        Rect::new(0.0, 0.0, 10.0, 10.0),
        None,
        Source::Manual {
            reason: "test".into(),
        },
    );
    assert_eq!(m.match_region(&without_text), None);
}

#[test]
fn match_region_respects_context() {
    let mut e = entry("b001", ListType::Positive, "Musterfirma GmbH");
    e.context_before = Some("Überweisung an".to_string());
    let m = matcher(vec![e]);
    let region = |text: &str| {
        Region::new(
            0,
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Some(text.to_string()),
            Source::Manual {
                reason: "test".into(),
            },
        )
    };
    assert!(m
        .match_region(&region("Überweisung an Musterfirma GmbH"))
        .is_some());
    assert!(m
        .match_region(&region("Gutschrift von Musterfirma GmbH"))
        .is_none());
}

#[test]
fn regex_pattern_with_metacharacters_is_rejected() {
    let mut e = entry("b004", ListType::Positive, r"Rg\.\s*\d{4,}");
    e.is_regex = true;
    match BookingMatcher::new(vec![e]) {
        Err(RedactError::Booking(msg)) => {
            assert!(msg.contains("b004"), "{msg}");
            assert!(msg.contains("--patterns-config"), "{msg}");
        }
        other => panic!("Buchungslisten-Fehler erwartet, war: {other:?}"),
    }
}

#[test]
fn regex_pattern_without_metacharacters_is_treated_literally() {
    let mut e = entry("b004", ListType::Positive, "Rechnung 2024");
    e.is_regex = true;
    let m = matcher(vec![e]);
    let regions = m
        .find_matches(&[run(0, "Beleg: rechnung  2024 bezahlt")])
        .unwrap();
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].text.as_deref(), Some("rechnung  2024"));
}

#[test]
fn blank_pattern_is_rejected_by_the_matcher() {
    let e = entry("b099", ListType::Positive, "   ");
    match BookingMatcher::new(vec![e]) {
        Err(RedactError::Booking(msg)) => assert!(msg.contains("b099"), "{msg}"),
        other => panic!("Buchungslisten-Fehler erwartet, war: {other:?}"),
    }
}

#[test]
fn analyzer_trait_delegates_to_find_matches() {
    let m = matcher(vec![entry("b001", ListType::Positive, "Musterfirma GmbH")]);
    let runs = vec![run(0, "Musterfirma GmbH")];
    let analyzer: &dyn Analyzer = &m;
    assert_eq!(
        analyzer.analyze(&runs).unwrap(),
        m.find_matches(&runs).unwrap()
    );
}

#[test]
fn negative_regions_block_positive_hits_via_core_resolver() {
    // Zusammenspiel mit der Konfliktauflösung aus redact-core.
    let m = matcher(vec![
        entry("b001", ListType::Positive, "Max Mustermann"),
        entry("b003", ListType::Negative, "Max Mustermann"),
    ]);
    let regions = m.find_matches(&[run(0, "Max Mustermann")]).unwrap();
    assert_eq!(regions.len(), 2);
    let resolution = redact_core::resolve_conflicts(regions);
    assert!(resolution.redact.is_empty());
    assert_eq!(resolution.blocked.len(), 1);
    assert_eq!(resolution.blocked[0].booking_id, "b003");
}
