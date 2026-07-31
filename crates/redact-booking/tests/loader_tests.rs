//! Tests für den CSV-Loader.

use std::path::Path;

use redact_booking::CsvBookingLoader;
use redact_core::{BookingLoader, ListType, RedactError};

/// Lädt CSV aus einem String und erwartet Erfolg.
fn load(csv: &str) -> Vec<redact_core::BookingEntry> {
    CsvBookingLoader
        .from_reader(csv.as_bytes())
        .expect("CSV sollte ladbar sein")
}

/// Lädt CSV aus einem String und erwartet einen Buchungslisten-Fehler.
fn load_err(csv: &str) -> String {
    match CsvBookingLoader.from_reader(csv.as_bytes()) {
        Err(RedactError::Booking(msg)) => msg,
        Err(other) => panic!("falscher Fehlertyp: {other}"),
        Ok(_) => panic!("Fehler erwartet"),
    }
}

const HEADER: &str = "id,list_type,pattern,context_before,context_after,is_regex\n";

#[test]
fn loads_valid_rows_in_file_order() {
    let csv = format!(
        "{HEADER}\
         b001,positive,\"Musterfirma GmbH\",\"Überweisung an\",,\n\
         b003,negative,\"Max Mustermann\",,\"Kontoinhaber\",\n"
    );
    let entries = load(&csv);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].id, "b001");
    assert_eq!(entries[0].list_type, ListType::Positive);
    assert_eq!(entries[0].pattern, "Musterfirma GmbH");
    assert_eq!(entries[0].context_before.as_deref(), Some("Überweisung an"));
    assert!(!entries[0].is_regex);
    assert_eq!(entries[1].list_type, ListType::Negative);
    assert_eq!(entries[1].context_after.as_deref(), Some("Kontoinhaber"));
}

#[test]
fn empty_context_cells_become_none() {
    // Sowohl leere als auch gequotet-leere und nur-Leerraum-Zellen ergeben None.
    let csv = format!("{HEADER}b001,positive,Musterfirma,\"\",\"   \",\n");
    let entries = load(&csv);
    assert_eq!(entries[0].context_before, None);
    assert_eq!(entries[0].context_after, None);
}

#[test]
fn header_only_file_is_valid_and_empty() {
    assert!(load(HEADER).is_empty());
}

#[test]
fn tolerates_bom_and_extra_columns_and_missing_is_regex() {
    let csv = "\u{feff}id,list_type,pattern,context_before,context_after,kommentar\n\
               b001,POSITIVE,Musterfirma,,,\"nur eine Notiz\"\n";
    let entries = load(csv);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].list_type, ListType::Positive);
    assert!(!entries[0].is_regex);
}

#[test]
fn rejects_invalid_list_type_with_line_number() {
    let csv = format!("{HEADER}b001,positive,A,,,\nb002,vielleicht,B,,,\n");
    let msg = load_err(&csv);
    assert!(msg.contains("Zeile 3"), "{msg}");
    assert!(msg.contains("vielleicht"), "{msg}");
}

#[test]
fn rejects_duplicate_id() {
    let csv = format!("{HEADER}b001,positive,A,,,\nb001,negative,B,,,\n");
    let msg = load_err(&csv);
    assert!(msg.contains("Zeile 3"), "{msg}");
    assert!(msg.contains("doppelte id `b001`"), "{msg}");
}

#[test]
fn rejects_empty_pattern_and_empty_id() {
    let msg = load_err(&format!("{HEADER}b001,positive,\"\",,,\n"));
    assert!(msg.contains("Zeile 2"), "{msg}");
    assert!(msg.contains("pattern"), "{msg}");

    let msg = load_err(&format!("{HEADER}\"\",positive,Musterfirma,,,\n"));
    assert!(msg.contains("Zeile 2"), "{msg}");
    assert!(msg.contains("id"), "{msg}");
}

#[test]
fn rejects_missing_required_column() {
    let msg = load_err("id,pattern\nb001,Musterfirma\n");
    assert!(msg.contains("Kopfzeile"), "{msg}");
    assert!(msg.contains("list_type"), "{msg}");
}

#[test]
fn parses_all_accepted_is_regex_spellings() {
    let csv = format!(
        "{HEADER}\
         b001,positive,A,,,TRUE\n\
         b002,positive,B,,,1\n\
         b003,positive,C,,,Yes\n\
         b004,positive,D,,,false\n\
         b005,positive,E,,,0\n\
         b006,positive,F,,,no\n\
         b007,positive,G,,,\n"
    );
    let entries = load(&csv);
    let flags: Vec<bool> = entries.iter().map(|e| e.is_regex).collect();
    assert_eq!(flags, vec![true, true, true, false, false, false, false]);
}

#[test]
fn rejects_invalid_is_regex_value() {
    let msg = load_err(&format!("{HEADER}b001,positive,A,,,vielleicht\n"));
    assert!(msg.contains("Zeile 2"), "{msg}");
    assert!(msg.contains("is_regex"), "{msg}");
}

#[test]
fn skips_completely_empty_rows() {
    let csv = format!("{HEADER}b001,positive,A,,,\n,,,,,\nb002,negative,B,,,\n");
    let entries = load(&csv);
    assert_eq!(entries.len(), 2);
}

#[test]
fn loads_example_file_from_repository() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/booking_list.csv");
    let entries = CsvBookingLoader
        .load(&path)
        .expect("Beispieldatei sollte ladbar sein");
    assert_eq!(entries.len(), 7);
    assert!(entries.iter().any(|e| e.list_type == ListType::Negative));
    // Beispiel-CSV nutzt eine zusätzliche Spalte `hinweis` — muss toleriert werden.
    assert!(entries.iter().any(|e| e.pattern.contains("DE89 3704")));
}

#[test]
fn load_error_mentions_file_name() {
    let missing = Path::new("/nicht/vorhanden/booking_list.csv");
    match CsvBookingLoader.load(missing) {
        Err(RedactError::Booking(msg)) => assert!(msg.contains("booking_list.csv"), "{msg}"),
        other => panic!("Buchungslisten-Fehler erwartet, war: {other:?}"),
    }
}
