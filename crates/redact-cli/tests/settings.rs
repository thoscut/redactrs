//! Die Einstellungsdatei am gebauten Binary — und vor allem die **Rangfolge**.
//!
//! ```text
//! Kommandozeile  schlägt  Einstellungsdatei  schlägt  Vorgabe
//! ```
//!
//! Die Reihenfolge steht in `redact_cli::cli` als Aufruf von `unwrap_or`; hier
//! wird sie am laufenden Programm gemessen. Sichtbar wird sie am Namen der
//! erzeugten Datei (`output_suffix`) und an der Polsterung (`padding`), die
//! die Ausgabe Byte für Byte verändert.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("redact-settings-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Ein Lauf mit einer bestimmten Einstellungsdatei (oder ganz ohne).
fn run_with(settings: Option<&Path>, args: &[&str]) -> Output {
    let mut command = Command::new(bin());
    command.args(args).env_remove("REDACT_RS_PASSWORD");
    match settings {
        // Ein Pfad, den es nicht gibt: so bleibt eine Einstellungsdatei des
        // Systems außen vor und der Lauf zeigt die reinen Vorgaben.
        None => command.env("REDACT_RS_CONFIG", "/nicht/vorhanden.yaml"),
        Some(path) => command.env("REDACT_RS_CONFIG", path),
    };
    command.output().expect("Binary startbar")
}

fn demo(dir: &Path) -> PathBuf {
    let path = dir.join("kontoauszug.pdf");
    std::fs::write(&path, redact_pdf::testing::demo_statement()).unwrap();
    path
}

fn settings_file(dir: &Path, content: &str) -> PathBuf {
    let path = dir.join("settings.yaml");
    std::fs::write(&path, content).unwrap();
    path
}

#[track_caller]
fn succeeds(out: &Output) {
    assert!(
        out.status.success(),
        "Lauf fehlgeschlagen: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Stufe 1: ohne Datei und ohne Schalter gilt die eingebaute Vorgabe.
#[test]
fn without_a_file_the_built_in_default_applies() {
    let dir = workdir("default");
    let input = demo(&dir);

    succeeds(&run_with(
        None,
        &[input.to_str().unwrap(), "--patterns", "iban_de"],
    ));
    assert!(dir.join("kontoauszug_geschwaerzt.pdf").exists());

    std::fs::remove_dir_all(&dir).ok();
}

/// Stufe 2: die Datei schlägt die Vorgabe.
#[test]
fn the_settings_file_beats_the_built_in_default() {
    let dir = workdir("file");
    let input = demo(&dir);
    let settings = settings_file(&dir, "output_suffix: _ausDatei\npatterns: [iban_de]\n");

    succeeds(&run_with(Some(&settings), &[input.to_str().unwrap()]));
    assert!(
        dir.join("kontoauszug_ausDatei.pdf").exists(),
        "der Namenszusatz aus der Datei wirkt nicht"
    );
    assert!(!dir.join("kontoauszug_geschwaerzt.pdf").exists());

    std::fs::remove_dir_all(&dir).ok();
}

/// Stufe 3: die Kommandozeile schlägt die Datei.
#[test]
fn the_command_line_beats_the_settings_file() {
    let dir = workdir("cli");
    let input = demo(&dir);
    let settings = settings_file(&dir, "output_suffix: _ausDatei\npatterns: [iban_de]\n");

    succeeds(&run_with(
        Some(&settings),
        &[input.to_str().unwrap(), "--output-suffix", "_vonHand"],
    ));
    assert!(dir.join("kontoauszug_vonHand.pdf").exists());
    assert!(!dir.join("kontoauszug_ausDatei.pdf").exists());

    std::fs::remove_dir_all(&dir).ok();
}

/// Auch die Polsterung folgt der Rangfolge — und sie ist im Ergebnis messbar:
/// dieselbe Eingabe mit anderer Polsterung ergibt andere Bytes.
///
/// Das ist die Gegenprobe zum Namenstest oben: ein Dateiname ließe sich auch
/// dann richtig setzen, wenn der Rest der Einstellungen nirgends ankäme.
#[test]
fn padding_follows_the_same_order_and_changes_the_result() {
    let dir = workdir("padding");
    let input = demo(&dir);
    let settings = settings_file(&dir, "padding: 6.0\npatterns: [iban_de]\n");

    let bytes = |name: &str| std::fs::read(dir.join(name)).unwrap();

    // Vorgabe (1.0)
    succeeds(&run_with(
        None,
        &[
            input.to_str().unwrap(),
            "-o",
            dir.join("vorgabe.pdf").to_str().unwrap(),
            "--patterns",
            "iban_de",
        ],
    ));
    // Datei (6.0)
    succeeds(&run_with(
        Some(&settings),
        &[
            input.to_str().unwrap(),
            "-o",
            dir.join("datei.pdf").to_str().unwrap(),
        ],
    ));
    // Kommandozeile (1.0) schlägt die Datei — also wieder wie die Vorgabe.
    succeeds(&run_with(
        Some(&settings),
        &[
            input.to_str().unwrap(),
            "-o",
            dir.join("hand.pdf").to_str().unwrap(),
            "--padding",
            "1.0",
        ],
    ));

    assert_ne!(
        bytes("vorgabe.pdf"),
        bytes("datei.pdf"),
        "die Polsterung aus der Datei kam nicht an"
    );
    assert_eq!(
        bytes("vorgabe.pdf"),
        bytes("hand.pdf"),
        "die Kommandozeile hat die Datei nicht geschlagen"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Eine fehlerhafte Einstellungsdatei wird gemeldet, nicht übergangen.
#[test]
fn a_broken_settings_file_stops_the_run_with_a_message() {
    let dir = workdir("broken");
    let input = demo(&dir);
    let settings = settings_file(&dir, "output_sufix: _tippfehler\n");

    let out = run_with(Some(&settings), &[input.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2), "Konfigurationsfehler ⇒ 2");
    let message = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(message.contains("settings.yaml"), "{message}");
    assert!(message.contains("output_sufix"), "{message}");
    assert!(
        !dir.join("kontoauszug_geschwaerzt.pdf").exists(),
        "trotz kaputter Einstellungsdatei geschrieben"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Eine fehlende Datei ist kein Fehler — es gilt dann die Vorgabe.
#[test]
fn a_missing_settings_file_is_not_an_error() {
    let dir = workdir("missing");
    let input = demo(&dir);
    let out = run_with(
        Some(&dir.join("gibt-es-nicht.yaml")),
        &[input.to_str().unwrap(), "--patterns", "iban_de"],
    );
    succeeds(&out);
    assert!(dir.join("kontoauszug_geschwaerzt.pdf").exists());
    std::fs::remove_dir_all(&dir).ok();
}
