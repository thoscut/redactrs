//! redact-rs — lokales Schwärzen sensibler Daten in PDF-Dokumenten.

#![forbid(unsafe_code)]

mod batch;
mod cli;

use std::process::ExitCode;

use clap::Parser;
use redact_core::{RedactError, Result};
use redact_pipeline::{Outcome, Settings};

use crate::cli::Cli;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(&cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Fehler: {e}");
            if let RedactError::Config(_) = e {
                return ExitCode::from(2);
            }
            ExitCode::FAILURE
        }
    }
}

fn dispatch(cli: &Cli) -> Result<ExitCode> {
    if cli.list_patterns {
        list_patterns()?;
        return Ok(ExitCode::SUCCESS);
    }

    if let Some(path) = &cli.write_demo {
        // Auch die Beispieldatei geht durch den zentralen Schreibpfad. Vorher
        // hat sie `--force` ignoriert und wäre über einen Symlink an eine
        // beliebige Stelle geschrieben worden.
        redact_pdf::document::write_file(
            path,
            &redact_pdf::testing::demo_statement(),
            &redact_pdf::document::WriteOptions::new().force(cli.force),
        )?;
        println!("Beispiel-PDF geschrieben: {}", path.display());
        return Ok(ExitCode::SUCCESS);
    }

    // Die Einstellungsdatei — die Schicht zwischen Vorgabe und Kommandozeile.
    // Eine fehlerhafte Datei ist ein Fehler und wird nicht übergangen.
    let settings = Settings::load()?;

    // Ohne Eingabedatei oder mit --gui: grafische Oberfläche.
    if cli.gui || (cli.inputs.is_empty() && cli.output.is_none()) {
        return start_gui(cli, &settings).map(|()| ExitCode::SUCCESS);
    }

    if cli.inputs.is_empty() {
        return Err(RedactError::Config(
            "keine Eingabedatei angegeben (`redact-rs --help` zeigt Beispiele)".into(),
        ));
    }

    let inputs = batch::gather_inputs(&cli.inputs, &cli.config(&settings).output_suffix)?;

    // Eine Datei bleibt eine Datei: derselbe Ablauf und dieselben
    // Rückgabewerte wie bisher — ein Fehler wandert nach oben und wird dort
    // nach Art unterschieden (Konfiguration ⇒ 2, sonst 1).
    if let [input] = inputs.as_slice() {
        let outcome = redact_pipeline::run(&cli.config_for(&settings, input))?;
        if cli.json {
            println!("{}", serde_json::to_string_pretty(&outcome)?);
        } else if !cli.quiet {
            report(&outcome);
        }
        return Ok(ExitCode::SUCCESS);
    }

    batch::run(cli, &settings, &inputs)
}

fn report(outcome: &Outcome) {
    println!("Eingabe:            {}", outcome.input);
    println!("Seiten:             {}", outcome.pages);
    if outcome.text_runs > 0 {
        println!("Textzeilen:         {}", outcome.text_runs);
        println!("Treffer gesamt:     {}", outcome.candidates);
    }
    if outcome.blocked > 0 {
        println!("Durch Negativliste blockiert: {}", outcome.blocked);
        for detail in &outcome.blocked_details {
            println!("  - {detail}");
        }
    }
    match &outcome.review_out {
        Some(path) => {
            println!("Review geschrieben: {path}");
            println!();
            println!("Datei prüfen, `enabled` anpassen und dann anwenden mit:");
            println!("  redact-rs {} --apply-review {path}", outcome.input);
        }
        None => {
            // Absicht und Ergebnis werden getrennt ausgewiesen: eine Region,
            // von der `--padding` nichts übrig lässt, wird übersprungen und
            // darf nicht als Schwärzung durchgehen.
            println!("Schwärzungen:       {}", outcome.redactions);
            if outcome.degenerate_redactions > 0 {
                println!(
                    "  davon wirkungslos:  {} (leeres Rechteck nach --padding)",
                    outcome.degenerate_redactions
                );
            }
            println!("Entfernte Zeichen:  {}", outcome.removed_glyphs);
            println!("Deck-Rechtecke:     {}", outcome.drawn_rects);
            if outcome.removed_annotations > 0 {
                println!("Entfernte Annotationen: {}", outcome.removed_annotations);
            }
            // Ein überschriebenes Bild gehört gemeldet: die Bildpunkte
            // außerhalb der Schwärzung bleiben zwar unverändert (neu kodiert
            // wird verlustfrei mit Flate), die *Datei* ist danach aber eine
            // andere — aus einem JPEG-Stream wird ein Flate-Stream, und die
            // Ausgabe wächst dadurch spürbar.
            if outcome.redacted_images > 0 {
                println!(
                    "Überschriebene Bilder: {} (neu kodiert: außerhalb der \
                     Schwärzung verlustfrei, Datei dadurch größer)",
                    outcome.redacted_images
                );
            }
            if outcome.metadata_removed.is_empty() {
                println!("Metadaten:          nichts zu entfernen");
            } else {
                println!(
                    "Metadaten entfernt: {}",
                    outcome.metadata_removed.join(", ")
                );
            }
            if let Some(path) = &outcome.output {
                println!("Ausgabe:            {path}");
            }
            if let Some(path) = &outcome.audit_log {
                println!("Audit-Log:          {path}");
            }
        }
    }
    for warning in &outcome.warnings {
        eprintln!("Warnung: {warning}");
    }
}

fn list_patterns() -> Result<()> {
    let min = redact_patterns::DEFAULT_MIN_CONFIDENCE;
    println!("Eingebaute Patterns:\n");
    println!(
        "  [an/aus] {:<14} {:<8} {:<8} Beschreibung",
        "id", "mit Kt.", "ohne"
    );
    for def in redact_patterns::builtin_patterns() {
        let state = if def.enabled { "an " } else { "aus" };
        // Zwei Spalten, weil dasselbe Pattern je nach Umfeld unterschiedlich
        // bewertet wird: „mit Kt." gilt, wenn die Gruppe `context` gegriffen
        // hat, „ohne" sonst. Ein `-` heißt: das Pattern kennt keinen Kontext,
        // der eine Wert gilt immer.
        let weak = match def.confidence_without_context {
            Some(value) => format!("{value:.2}"),
            None => "-".to_string(),
        };
        println!(
            "  [{state}]    {:<14} {:<8} {:<8} {}",
            def.id,
            format!("{:.2}", def.confidence),
            weak,
            def.description
        );
    }
    println!(
        "\nAuswahl mit --patterns id1,id2 — standardmäßig ausgeschaltete Patterns\n\
         lassen sich so gezielt einschalten.\n\n\
         Treffer unterhalb des Mindestvertrauens werden verworfen; die Vorgabe\n\
         ist {min:.2}. Ein Pattern, dessen Spalte „ohne“ darunter liegt, findet\n\
         also nichts, solange kein Schlüsselwort danebensteht — mit\n\
         `--min-confidence 0.25` kommen auch diese Verdachtsfälle durch."
    );
    Ok(())
}

#[cfg(feature = "gui")]
fn start_gui(cli: &Cli, settings: &Settings) -> Result<()> {
    if cli.inputs.len() > 1 {
        return Err(RedactError::Config(
            "Die Oberfläche zeigt ein Dokument. Für mehrere Dateien den Stapel \
             ohne --gui benutzen."
                .into(),
        ));
    }
    // Dieselbe Konfiguration wie ein Lauf auf der Kommandozeile — die
    // Oberfläche analysiert und exportiert damit über dieselbe Kette.
    redact_gui::run(cli.config(settings))
}

#[cfg(not(feature = "gui"))]
fn start_gui(_cli: &Cli, _settings: &Settings) -> Result<()> {
    Err(RedactError::Config(
        "Diese Fassung wurde ohne grafische Oberfläche gebaut \
         (Feature `gui` deaktiviert). Bitte Eingabe- und Ausgabedatei angeben."
            .into(),
    ))
}
