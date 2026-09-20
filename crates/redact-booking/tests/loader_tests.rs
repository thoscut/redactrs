//! Tests für den CSV-Loader.

use std::path::Path;

use redact_booking::CsvBookingLoader;
use redact_core::{ListType, RedactError};

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

// ---------------------------------------------------------------------------
// Die Datei bestimmt nicht mehr, wie viel Arbeitsspeicher sie kostet
// ---------------------------------------------------------------------------
//
// Der Befund: `--booking-list` prüfte nicht, was da liegt, sondern las erst und
// fragte dann. Gemessen am gebauten Binary — eine dünn belegte Datei belegt
// 4 kB auf der Platte:
//
// | Datei | vorher | nachher |
// |---|---|---|
// | 512 MB dünn belegt | 2 638 MB, 24,5 s | 17 MB, 0,00 s |
// | 1 GB dünn belegt | 5 259 MB, 50,4 s | 17 MB, 0,00 s |
// | 6 GB dünn belegt | OOM-Killer (SIGKILL, Messung der Prüfung) | 17 MB, 0,04 s |
// | benannte Pipe | 7 201 MB nach 20 s, kein Ende | 17 MB, 0,01 s |
//
// Gemessen wird hier nicht der Speicher — ein Test, der 5 GB belegt, reißt den
// Testläufer mit —, sondern **ob abgelehnt wird und woran**: dass die Meldung
// die Größe nennt, kann sie nur aus der Angabe des Dateisystems haben, also von
// vor dem ersten gelesenen Byte.

/// Ein eigenes Verzeichnis je Test; Tests laufen nebenläufig.
fn tempdir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("redact-booking-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// **Die Auflage:** die Grenze greift *vor* dem CSV-Parser.
///
/// Die Verstärkung steckt nicht im Lesen, sondern im Parsen — eine einzige
/// riesige Zelle wird als `String` materialisiert, dazu Puffer und
/// kleingeschriebene Kopie. Deshalb muss die Datei abgelehnt sein, bevor
/// `csv` sie überhaupt sieht.
#[test]
fn a_file_beyond_the_limit_is_refused_before_the_parser_sees_it() {
    let dir = tempdir("gross");
    let path = dir.join("riesig.csv");
    let file = std::fs::File::create(&path).unwrap();
    // Dünn belegt: 6 GB Nennlänge, 0 Byte geschrieben.
    file.set_len(6 * 1024 * 1024 * 1024).unwrap();
    drop(file);

    match CsvBookingLoader.load(&path) {
        Err(RedactError::Booking(msg)) => {
            assert!(msg.contains("riesig.csv"), "{msg}");
            // Die Größe kann nur aus der Dateiangabe stammen …
            assert!(msg.contains("6144 MB"), "{msg}");
            // … und die Meldung sagt, welche Grenze das war …
            assert!(msg.contains("16 MB"), "{msg}");
            // … und wo sie herkommt.
            assert!(msg.contains("feste Grenze"), "{msg}");
            assert!(msg.contains("--booking-list"), "{msg}");
            // Keine Meldung des CSV-Parsers: der war nie dran.
            assert!(!msg.contains("Kopfzeile"), "{msg}");
        }
        other => panic!("Buchungslisten-Fehler erwartet, war: {other:?}"),
    }

    std::fs::remove_dir_all(&dir).ok();
}

/// **Die Auflage:** eine benannte Pipe wird abgelehnt, statt endlos zu liefern.
///
/// Der Test kommt ohne Schreiber am anderen Ende aus, und das ist gerade der
/// Punkt: schon das *Öffnen* einer Pipe ohne Schreiber blockiert endlos. Dass
/// dieser Test überhaupt zurückkehrt, ist die Aussage.
///
/// `cfg(unix)` sagt, dass es benannte Pipes gibt — nicht, dass `mkfifo` im
/// `PATH` steht. Auf einem schlanken Unix-Bild ohne die Werkzeuge (BusyBox
/// ohne `mkfifo`, ein Container mit `scratch`-Basis) ließ
/// `expect("mkfifo startbar")` den Testlauf platzen, obwohl am Programm nichts
/// falsch war — derselbe Fehler, den `belege.rs` bei `python3` schon
/// vermeidet: fehlt das Werkzeug, ist hier nichts zu prüfen, und der Test
/// **sagt das** und endet grün. Ein **vorhandenes** `mkfifo`, das scheitert,
/// bleibt dagegen ein Fehler: dann gibt es die Pipe, und die Prüfung wäre
/// klammheimlich ausgefallen.
#[cfg(unix)]
#[test]
fn a_named_pipe_is_refused_without_opening_it() {
    let dir = tempdir("pipe");
    let path = dir.join("liste.csv");
    let status = match std::process::Command::new("mkfifo").arg(&path).status() {
        Ok(status) => status,
        Err(e) => {
            eprintln!(
                "kein mkfifo im Pfad ({e}) — dass `CsvBookingLoader` eine benannte \
                 Pipe ablehnt, bleibt hier ungeprüft"
            );
            std::fs::remove_dir_all(&dir).ok();
            return;
        }
    };
    assert!(status.success(), "mkfifo ist fehlgeschlagen");

    match CsvBookingLoader.load(&path) {
        Err(RedactError::Booking(msg)) => {
            assert!(msg.contains("gewöhnliche Datei"), "{msg}");
            assert!(msg.contains("liste.csv"), "{msg}");
        }
        other => panic!("Buchungslisten-Fehler erwartet, war: {other:?}"),
    }

    std::fs::remove_dir_all(&dir).ok();
}

/// Die Gegenprobe zur Grenze: eine gewöhnliche Liste geht weiterhin durch —
/// auch eine große. Ohne diesen Test wäre alles darüber nur kaputt statt
/// abgesichert.
#[test]
fn an_ordinary_list_still_loads_completely() {
    let dir = tempdir("normal");
    let path = dir.join("liste.csv");
    let mut csv = String::from(HEADER);
    for i in 0..20_000 {
        csv.push_str(&format!("b{i:05},positive,Musterfirma {i} GmbH,,,false\n"));
    }
    std::fs::write(&path, &csv).unwrap();

    let entries = CsvBookingLoader
        .load(&path)
        .expect("20 000 Einträge sind eine gewöhnliche Liste");
    assert_eq!(entries.len(), 20_000);

    std::fs::remove_dir_all(&dir).ok();
}
