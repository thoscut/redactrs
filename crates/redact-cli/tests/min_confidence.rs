//! `--min-confidence`: die Schwelle, unterhalb derer ein Treffer verworfen
//! wird.
//!
//! `redact-patterns` bewertet denselben Treffer je nach Umfeld unterschiedlich:
//! „Kto. 532013000“ ist eine Kontonummer (0.85), ein nacktes „532013000“
//! bestenfalls ein Verdacht (0.30). Voreingestellt ist
//! [`redact_patterns::DEFAULT_MIN_CONFIDENCE`] = 0.5, der Verdachtsfall fällt
//! also durch. Wer ihn sehen will, muss die Schwelle senken können — bis hierher
//! ging das nur über eine eigene Pattern-Konfigurationsdatei.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Eine nackte Ziffernkette ohne Schlüsselwort daneben.
const BARE_NUMBER: &str = "532013000";

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("redact-minconf-{}-{name}", std::process::id()));
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

#[track_caller]
fn succeeds(out: &Output) -> String {
    assert!(
        out.status.success(),
        "Lauf fehlgeschlagen: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Ein PDF mit genau einer Zeile: der Ziffernkette ohne jeden Kontext.
fn bare_number_pdf(dir: &Path) -> PathBuf {
    let path = dir.join("referenz.pdf");
    std::fs::write(
        &path,
        redact_pdf::testing::minimal_pdf(&format!("Referenz {BARE_NUMBER}")),
    )
    .unwrap();
    path
}

fn leaks_in(path: &Path, needle: &str) -> Vec<String> {
    redact_pdf::leaks(&std::fs::read(path).expect("Ausgabedatei lesbar"), needle)
}

#[test]
fn a_bare_number_stays_below_the_default_threshold() {
    let dir = workdir("default");
    let input = bare_number_pdf(&dir);
    let output = dir.join("out.pdf");

    let stdout = succeeds(&run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--patterns",
        "konto_nr",
        "--json",
    ]));
    let outcome: serde_json::Value = serde_json::from_str(&stdout).expect("JSON");
    assert_eq!(
        outcome["candidates"], 0,
        "ohne Schlüsselwort darf die Ziffernkette nicht durchkommen: {outcome}"
    );
    assert!(
        !leaks_in(&output, BARE_NUMBER).is_empty(),
        "die Zahl wurde geschwärzt, obwohl sie unter der Schwelle liegt"
    );
}

#[test]
fn lowering_the_threshold_lets_the_suspicion_through() {
    let dir = workdir("gesenkt");
    let input = bare_number_pdf(&dir);
    let output = dir.join("out.pdf");

    let stdout = succeeds(&run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--patterns",
        "konto_nr",
        "--min-confidence",
        "0.25",
        "--json",
    ]));
    let outcome: serde_json::Value = serde_json::from_str(&stdout).expect("JSON");
    assert_eq!(
        outcome["redactions"], 1,
        "mit gesenkter Schwelle muss der Verdachtsfall geschwärzt werden: {outcome}"
    );
    let hits = leaks_in(&output, BARE_NUMBER);
    assert!(hits.is_empty(), "{hits:?}");
}

#[test]
fn an_impossible_threshold_is_refused_with_a_message() {
    let dir = workdir("ungueltig");
    let input = bare_number_pdf(&dir);
    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
        "--min-confidence",
        "1.5",
    ]);
    assert!(!out.status.success(), "1.5 wurde klaglos akzeptiert");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Mindestvertrauen"),
        "unbrauchbare Meldung: {stderr}"
    );
}

#[test]
fn list_patterns_names_both_confidences_and_the_threshold() {
    // Ohne die zweite Spalte wundert sich, wer `--patterns konto_nr` aufruft
    // und bei einem Dokument ohne „Kto.“ nichts bekommt.
    let stdout = succeeds(&run(&["--list-patterns"]));
    assert!(stdout.contains("konto_nr"), "{stdout}");
    assert!(
        stdout.contains("0.85") && stdout.contains("0.30"),
        "{stdout}"
    );
    assert!(stdout.contains("--min-confidence"), "{stdout}");
}
