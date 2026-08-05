//! Ein Rechteck **neben dem Blatt** — und beide Wege sagen dasselbe darüber.
//!
//! ## Der Befund, den dieser Test schließt
//!
//! `HitOutcome::OffPage` gab es nur in der Oberfläche. Dort wird ein Rechteck,
//! das vollständig neben seiner Seite liegt, aus der Zahl „n werden
//! geschwärzt“ herausgerechnet und **vor** dem Export benannt.
//!
//! Die Kommandozeile kannte den Fall nicht. Dasselbe Rechteck ging dort als
//! geplante Schwärzung durch, wurde als `EntryEffect::Covered` verbucht — also
//! unter „Deck-Rechteck gezeichnet, aber kein Zeichen entfernt“ — und die
//! Wahrheit kam als Warnung **hinterher**, mit einem Satz, der über einer
//! Grafik der Normalfall ist („wo gar kein Text steht, ist das richtig“).
//!
//! Zwei Programme, ein Rechteck, zwei verschiedene Auskünfte. Genau die
//! Divergenz, für die es das gemeinsame `redact-pipeline`-Crate gibt.
//!
//! ## Was hier geprüft wird
//!
//! Derselbe Fall läuft einmal durch das gebaute Binary und einmal durch
//! `redact_gui::AppState`; die beiden Ausgaben werden nebeneinandergelegt und
//! auf **dieselbe Aussage** geprüft: eine Schwärzung wirksam, eine neben der
//! Seite, und keine davon als „geschwärzt“ gezählt.
//!
//! ## Der Rückgabewert bleibt 0 — mit Absicht
//!
//! Rückgabewert 3 beantwortet in diesem Programm die Frage *„hat das Werkzeug
//! alles gesehen?“* (siehe `redact_pipeline::coverage`). Hier lautet die
//! Antwort darauf **ja**: die Seite wurde vollständig durchsucht. Offen ist die
//! andere Frage — *„hat meine Angabe gestimmt?“* —, und die hat eine andere
//! Abhilfe (Koordinaten prüfen statt das Ergebnis von Hand nachlesen). Beides
//! unter eine Zahl zu legen nähme den Fällen, für die es die 3 gibt, ihre
//! Unterscheidbarkeit; das ist derselbe Grund, aus dem der Nachbarfall
//! „Seite gibt es nicht“ seit jeher mit 0 endet.
//!
//! Verschwiegen wird deshalb nichts: der Fall steht als eigene Zeile in der
//! Zusammenfassung, als eigenes Feld `off_page` im Audit-Log, als eigener
//! Befund je Region und in einer Warnung **mit Seitenzahl**. Der Test hält
//! genau das fest.

#![cfg(feature = "gui")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use redact_core::ReviewFile;
use redact_gui::AppState;
use redact_pipeline::Config;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("redact-offpage-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("Binary startbar")
}

fn demo(dir: &Path) -> PathBuf {
    let path = dir.join("kontoauszug.pdf");
    std::fs::write(&path, redact_pdf::testing::demo_statement()).unwrap();
    path
}

/// Eine Review-Datei mit **zwei** Einträgen: einer trifft die IBAN, der andere
/// liegt weit rechts neben dem A4-Blatt.
///
/// Sie entsteht über `--review-out` und wird dann ergänzt — nur so trägt sie
/// die richtige Prüfsumme, und nur mit ihr nimmt die Oberfläche sie an
/// (`AppState::apply_review_file` prüft die Identität wie `--apply-review`).
fn review_with_one_rect_beside_the_sheet(dir: &Path, input: &Path) -> PathBuf {
    let review = dir.join("review.json");
    let out = run(&[
        input.to_str().unwrap(),
        "--review",
        "--review-out",
        review.to_str().unwrap(),
        "--patterns",
        "iban_de",
    ]);
    assert!(
        out.status.success(),
        "Review-Lauf fehlgeschlagen: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let mut file: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&review).unwrap()).unwrap();
    let items = file["items"].as_array_mut().expect("items");
    // Genau ein echter Treffer, damit die Zahlen unten eindeutig sind.
    items.truncate(1);
    let echt = items[0].clone();
    // Dasselbe noch einmal, nur 700 Punkt weiter rechts: das A4-Blatt endet
    // bei 595, das Rechteck beginnt also jenseits davon.
    let mut daneben = echt.clone();
    daneben["id"] = serde_json::json!(1);
    for (feld, verschiebung) in [("ll", 700.0f64), ("ur", 700.0)] {
        let x = daneben["region"]["rect"][feld]["x"].as_f64().unwrap();
        daneben["region"]["rect"][feld]["x"] = serde_json::json!(x + verschiebung);
    }
    items.push(daneben);
    std::fs::write(&review, serde_json::to_string_pretty(&file).unwrap()).unwrap();
    review
}

/// **Derselbe Fall, beide Wege, die Ausgaben nebeneinander.**
#[test]
fn a_rect_beside_the_sheet_is_told_the_same_way_by_both() {
    let dir = workdir("parity");
    let input = demo(&dir);
    let review = review_with_one_rect_beside_the_sheet(&dir, &input);

    // --- Weg 1: das Binary.
    let cli_out = dir.join("cli.pdf");
    let cli_audit = dir.join("cli_audit.json");
    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        cli_out.to_str().unwrap(),
        "--apply-review",
        review.to_str().unwrap(),
        "--audit-log",
        cli_audit.to_str().unwrap(),
    ]);
    let bericht = String::from_utf8_lossy(&out.stdout).to_string();
    let meldungen = String::from_utf8_lossy(&out.stderr).to_string();

    // --- Weg 2: die Oberfläche, mit derselben Datei.
    let mut state = AppState::with_config(Config {
        input: input.clone(),
        ..Config::default()
    });
    state.load_document(&input).expect("PDF ladbar");
    state
        .apply_review_file(
            ReviewFile::from_json(&std::fs::read_to_string(&review).unwrap())
                .expect("Review-Datei lesbar"),
        )
        .expect("Prüfsumme stimmt");
    let summary = state.hit_summary();
    let kopfzeile = summary.headline();

    println!("--- Kommandozeile (stdout) ---\n{bericht}");
    println!("--- Kommandozeile (stderr) ---\n{meldungen}");
    println!("--- Oberfläche (Kopfzeile) ---\n{kopfzeile}");

    // (a) Beide zählen **eine** wirkliche Schwärzung, nicht zwei.
    assert_eq!(summary.redacted, 1, "Oberfläche: {kopfzeile}");
    assert_eq!(summary.off_page, 1, "Oberfläche: {kopfzeile}");
    assert!(
        bericht.contains("davon wirksam:      1"),
        "Kommandozeile: {bericht}"
    );
    assert!(
        bericht.contains("davon wirkungslos:  1 (Rechteck liegt neben der Seite)"),
        "Kommandozeile: {bericht}"
    );

    // (b) Beide benennen den Fall mit **denselben Worten**.
    assert!(
        kopfzeile.contains("1 neben der Seite"),
        "Oberfläche: {kopfzeile}"
    );
    assert!(
        meldungen.contains("neben der Seite, auf der sie stehen sollen"),
        "Kommandozeile: {meldungen}"
    );

    // (c) Und **nicht** mehr als „Deck-Rechteck gezeichnet“ — das war der
    //     falsche Satz zum richtigen Befund. Dreht man `EntryEffect::of` auf
    //     die Fassung ohne `OffPage` zurück, wird genau diese Zeile rot.
    assert!(
        !bericht.contains("davon ohne Textfund"),
        "der Fall läuft wieder als „überdeckt“ mit: {bericht}"
    );
    assert!(
        !meldungen.contains("haben ein Deck-Rechteck gezeichnet"),
        "der falsche Satz ist zurück: {meldungen}"
    );

    // (d) Das Audit-Log führt den Befund je Region — die Oberfläche zeigt ihn
    //     in der Zeile, die Kommandozeile schreibt ihn auf.
    let log: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&cli_audit).unwrap()).unwrap();
    assert_eq!(log["effect"]["off_page"], serde_json::json!(1));
    assert_eq!(log["effect"]["off_page_pages"], serde_json::json!([0]));
    assert_eq!(log["effect"]["covered"], serde_json::json!(0));
    assert_eq!(
        log["redactions"][1]["effect"],
        serde_json::json!("off_page")
    );

    // (e) Der Rückgabewert bleibt 0 — siehe Modulkommentar. Die 3 gehört der
    //     Frage „hat das Werkzeug alles gesehen?“, und die Antwort ist ja.
    assert!(
        out.status.success(),
        "Rückgabewert {:?}: {meldungen}",
        out.status.code()
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Gegenprobe: **ohne** das danebenliegende Rechteck sagt keiner der beiden
/// Wege etwas davon.
///
/// Ohne sie wäre der Test oben auch dann grün, wenn die neue Meldung bei jedem
/// Lauf erschiene.
#[test]
fn a_run_without_such_a_rect_says_nothing_about_it() {
    let dir = workdir("gegenprobe");
    let input = demo(&dir);

    let cli_out = dir.join("cli.pdf");
    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        cli_out.to_str().unwrap(),
        "--patterns",
        "iban_de",
    ]);
    let bericht = String::from_utf8_lossy(&out.stdout).to_string();
    let meldungen = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(out.status.success(), "{meldungen}");
    assert!(!bericht.contains("neben der Seite"), "{bericht}");
    assert!(!meldungen.contains("neben der Seite"), "{meldungen}");

    let mut state = AppState::with_config(Config {
        input: input.clone(),
        patterns: vec!["iban_de".to_string()],
        ..Config::default()
    });
    state.load_document(&input).expect("PDF ladbar");
    state.analyze().expect("Analyse läuft");
    let kopfzeile = state.hit_summary().headline();
    assert!(!kopfzeile.contains("neben der Seite"), "{kopfzeile}");
    assert_eq!(state.hit_summary().off_page, 0);

    std::fs::remove_dir_all(&dir).ok();
}
