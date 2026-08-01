//! redact-rs — lokales Schwärzen sensibler Daten in PDF-Dokumenten.

#![forbid(unsafe_code)]

mod audit;
mod cli;
mod pipeline;

use std::process::ExitCode;

use clap::Parser;
use redact_core::{RedactError, Result};

use crate::cli::Cli;
use crate::pipeline::Config;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Fehler: {e}");
            if let RedactError::Config(_) = e {
                return ExitCode::from(2);
            }
            ExitCode::FAILURE
        }
    }
}

fn dispatch(cli: Cli) -> Result<()> {
    if cli.list_patterns {
        return list_patterns();
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
        return Ok(());
    }

    // Ohne Eingabedatei oder mit --gui: grafische Oberfläche.
    if cli.gui || (cli.input.is_none() && cli.output.is_none()) {
        return start_gui(&cli);
    }

    let Some(input) = cli.input.clone() else {
        return Err(RedactError::Config(
            "keine Eingabedatei angegeben (`redact-rs --help` zeigt Beispiele)".into(),
        ));
    };

    let config = Config {
        input,
        output: cli.output.clone(),
        output_suffix: cli.output_suffix.clone(),
        force: cli.force,
        patterns: cli.patterns.clone(),
        no_patterns: cli.no_patterns,
        patterns_config: cli.patterns_config.clone(),
        min_confidence: cli.min_confidence,
        booking_list: cli.booking_list.clone(),
        manual_regions: cli.manual_regions.clone(),
        review: cli.review,
        review_out: cli.review_out.clone(),
        apply_review: cli.apply_review.clone(),
        audit_log: cli.audit_log.clone(),
        action: cli.action.to_action(&cli.replace_with),
        padding: cli.padding,
        limits: cli.limits(),
        max_candidates: cli.max_candidates,
    };

    let outcome = pipeline::run(&config)?;

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&outcome)?);
    } else if !cli.quiet {
        report(&outcome);
    }
    Ok(())
}

fn report(outcome: &pipeline::Outcome) {
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
fn start_gui(cli: &Cli) -> Result<()> {
    redact_gui::run(
        cli.input.clone(),
        cli.booking_list.clone(),
        cli.patterns.clone(),
    )
}

#[cfg(not(feature = "gui"))]
fn start_gui(_cli: &Cli) -> Result<()> {
    Err(RedactError::Config(
        "Diese Fassung wurde ohne grafische Oberfläche gebaut \
         (Feature `gui` deaktiviert). Bitte Eingabe- und Ausgabedatei angeben."
            .into(),
    ))
}
