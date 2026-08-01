//! Eigenständiges Programm für die grafische Oberfläche.
//!
//! ```text
//! redact-gui [PDF]
//! ```
//!
//! Bewusst ohne `clap`: der Aufruf hat genau ein optionales Argument. Die
//! vollständige Kommandozeile lebt in `redact-cli`, die diese Oberfläche über
//! `redact_gui::run` hinter ihrem `gui`-Feature startet.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("redact-gui {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if let Some(unknown) = args.iter().find(|a| a.starts_with('-')) {
        eprintln!("Unbekannte Option: {unknown}\n\n{USAGE}");
        return ExitCode::from(2);
    }
    if args.len() > 1 {
        eprintln!("Zu viele Argumente.\n\n{USAGE}");
        return ExitCode::from(2);
    }

    let config = redact_pipeline::Config {
        input: args
            .into_iter()
            .next()
            .map(PathBuf::from)
            .unwrap_or_default(),
        ..redact_pipeline::Config::default()
    };
    match redact_gui::run(config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Fehler: {e}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "\
redact-gui — Schwärzungen prüfen und zeichnen

Aufruf:
  redact-gui [PDF]

Optionen:
  -h, --help      Diese Hilfe anzeigen
  -V, --version   Version anzeigen

Die Buchungsliste wird im Fenster geladen. Alles Weitere — Musterauswahl,
Mindestvertrauen, Polsterung, eigene Pattern-Konfiguration — kennt nur die
Konsolenfassung: `redact-rs --gui [PDF] --patterns iban_de …` startet
dasselbe Fenster mit diesen Einstellungen.";
