//! Kommandozeilen-Definition.

use std::path::PathBuf;

use clap::{ArgAction, Parser, ValueEnum};

/// Lokales Schwärzen sensibler Daten in PDF-Dokumenten.
///
/// Ohne Eingabedatei (oder mit `--gui`) startet die grafische Oberfläche.
#[derive(Debug, Parser)]
#[command(
    name = "redact-rs",
    version,
    about = "Schwärzt sensible Daten in PDFs – lokal, ohne Cloud.",
    long_about = None,
    after_help = EXAMPLES,
)]
pub struct Cli {
    /// Eingabe-PDF.
    pub input: Option<PathBuf>,

    /// Ausgabe-PDF. Ohne Angabe wird neben der Eingabedatei gespeichert,
    /// mit dem Zusatz aus `--output-suffix` im Dateinamen.
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Namenszusatz für die Ausgabedatei, wenn `-o` fehlt.
    #[arg(long, value_name = "TEXT", default_value = redact_core::DEFAULT_OUTPUT_SUFFIX)]
    pub output_suffix: String,

    /// Vorhandene Ausgabedateien überschreiben.
    #[arg(short, long)]
    pub force: bool,

    /// Zu verwendende Patterns (kommagetrennt, z.B. `iban_de,bic`).
    /// Ohne Angabe werden die standardmäßig aktiven Patterns benutzt.
    #[arg(long, value_delimiter = ',', num_args = 1..)]
    pub patterns: Vec<String>,

    /// Keine Patterns anwenden.
    #[arg(long, conflicts_with_all = ["patterns", "patterns_config"])]
    pub no_patterns: bool,

    /// Eigene Pattern-Konfiguration (YAML oder JSON).
    #[arg(long, value_name = "DATEI")]
    pub patterns_config: Option<PathBuf>,

    /// Buchungsliste (CSV) mit Positiv- und Negativeinträgen.
    #[arg(long, value_name = "CSV")]
    pub booking_list: Option<PathBuf>,

    /// Manuell festgelegte Regionen (JSON).
    #[arg(long, value_name = "JSON")]
    pub manual_regions: Option<PathBuf>,

    /// Nur analysieren: gefundene Regionen exportieren, nichts schwärzen.
    #[arg(long)]
    pub review: bool,

    /// Zieldatei für den Review-Export (Standard: `<input>.review.json`).
    #[arg(long, value_name = "JSON")]
    pub review_out: Option<PathBuf>,

    /// Geprüfte Review-Datei anwenden (überspringt die Analyse).
    #[arg(long, value_name = "JSON", conflicts_with = "review")]
    pub apply_review: Option<PathBuf>,

    /// Audit-Log schreiben.
    #[arg(long, value_name = "JSON")]
    pub audit_log: Option<PathBuf>,

    /// Art der Schwärzung.
    #[arg(long, value_enum, default_value_t = ActionArg::Blackout)]
    pub action: ActionArg,

    /// Ersatztext für `--action replace`.
    #[arg(long, value_name = "TEXT", default_value = "[GESCHWÄRZT]")]
    pub replace_with: String,

    /// Zusätzlicher Rand um jede Schwärzung, in Punkt.
    #[arg(long, default_value_t = 1.0)]
    pub padding: f64,

    /// Obergrenze für die Summe aller entpackten Streams einer Eingabedatei.
    ///
    /// Schutz gegen Dekompressionsbomben: eine kleine Datei, die sich beim
    /// Öffnen vervielfacht.
    #[arg(long, value_name = "MB", default_value_t = 1024)]
    pub max_decompressed_mb: u64,

    /// Davon: Obergrenze für die Streams, die geparst werden
    /// (Seiteninhalt und Objekt-Streams).
    ///
    /// Aus einem Byte Seiteninhalt werden beim Parsen rund 60 Byte
    /// Arbeitsspeicher — deshalb ist diese Grenze deutlich enger.
    #[arg(long, value_name = "MB", default_value_t = 16)]
    pub max_parsed_mb: u64,

    /// Obergrenze für die Zahl der Trefferkandidaten in einer Datei.
    ///
    /// Die Konfliktauflösung wächst quadratisch mit dieser Zahl; ohne Grenze
    /// genügt eine kleine Datei mit sehr vielen Treffern, um die Maschine
    /// stundenlang zu beschäftigen.
    #[arg(long, value_name = "N", default_value_t = 100_000)]
    pub max_candidates: usize,

    /// Grafische Oberfläche starten.
    #[arg(long)]
    pub gui: bool,

    /// Verfügbare Patterns auflisten und beenden.
    #[arg(long)]
    pub list_patterns: bool,

    /// Beispiel-PDF (Kontoauszug) schreiben und beenden.
    #[arg(long, value_name = "PDF")]
    pub write_demo: Option<PathBuf>,

    /// Zusammenfassung als JSON auf stdout ausgeben.
    #[arg(long)]
    pub json: bool,

    /// Weniger Ausgabe.
    #[arg(short, long, action = ArgAction::SetTrue)]
    pub quiet: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ActionArg {
    /// Schwarzes Rechteck.
    Blackout,
    /// Weißes Rechteck.
    Whiteout,
    /// Weißes Rechteck mit Ersatztext.
    Replace,
}

impl Cli {
    /// Grenzen, mit denen fremde PDFs gelesen werden.
    pub fn limits(&self) -> redact_pdf::document::Limits {
        let mb = |n: u64| n.saturating_mul(1024 * 1024);
        redact_pdf::document::Limits {
            max_decompressed_bytes: mb(self.max_decompressed_mb),
            max_parsed_bytes: mb(self.max_parsed_mb),
            ..redact_pdf::document::Limits::default()
        }
    }
}

impl ActionArg {
    pub fn to_action(self, replacement: &str) -> redact_core::Action {
        match self {
            ActionArg::Blackout => redact_core::Action::Blackout,
            ActionArg::Whiteout => redact_core::Action::Whiteout,
            ActionArg::Replace => redact_core::Action::Replace(replacement.to_string()),
        }
    }
}

const EXAMPLES: &str = "\
Beispiele:
  # Automatisch schwärzen (Standard-Patterns)
  redact-rs kontoauszug.pdf -o geschwaerzt.pdf

  # Nur bestimmte Patterns
  redact-rs kontoauszug.pdf -o geschwaerzt.pdf --patterns iban_de,bic

  # Mit Buchungsliste (Positiv-/Negativliste)
  redact-rs kontoauszug.pdf -o geschwaerzt.pdf --booking-list buchungen.csv

  # Zwei Schritte: erst prüfen, dann anwenden
  redact-rs kontoauszug.pdf --review --review-out review.json
  redact-rs kontoauszug.pdf -o geschwaerzt.pdf --apply-review review.json \\
      --audit-log audit.json

  # Grafische Oberfläche
  redact-rs --gui kontoauszug.pdf

  # Beispieldatei zum Ausprobieren erzeugen
  redact-rs --write-demo beispiel.pdf
";

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_comma_separated_patterns() {
        let cli = Cli::parse_from([
            "redact-rs",
            "in.pdf",
            "-o",
            "out.pdf",
            "--patterns",
            "iban_de,bic",
        ]);
        assert_eq!(cli.patterns, vec!["iban_de", "bic"]);
        assert_eq!(cli.output.unwrap().to_str().unwrap(), "out.pdf");
    }

    #[test]
    fn review_and_apply_review_are_mutually_exclusive() {
        let r = Cli::try_parse_from([
            "redact-rs",
            "in.pdf",
            "--review",
            "--apply-review",
            "review.json",
        ]);
        assert!(r.is_err());
    }

    #[test]
    fn action_maps_to_core_action() {
        assert_eq!(
            ActionArg::Replace.to_action("[IBAN]"),
            redact_core::Action::Replace("[IBAN]".into())
        );
    }

    #[test]
    fn no_arguments_is_allowed_gui_mode() {
        let cli = Cli::parse_from(["redact-rs"]);
        assert!(cli.input.is_none());
    }
}
