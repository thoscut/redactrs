//! Stapelverarbeitung: mehrere Dateien, ein Ergebnis je Datei.
//!
//! ## Die eine Regel, auf die es ankommt
//!
//! **Eine kaputte Datei beendet den Stapel nicht.** Wer zwanzig Kontoauszüge
//! übergibt und bei Nummer sieben eine Datei erwischt, die kein PDF ist, will
//! die übrigen neunzehn trotzdem geschwärzt haben — und will von Nummer sieben
//! *erfahren*. Also: jede Datei einzeln, Fehler sammeln, am Ende eine
//! Zusammenfassung, und der Rückgabewert sagt, ob alles gut ging.
//!
//! ## Kein Ergebnis überschreibt ein anderes
//!
//! Das ergibt sich aus der Namensregel: die Ausgabe entsteht **neben** ihrer
//! Eingabe. Zwei gleichnamige Dateien aus verschiedenen Verzeichnissen landen
//! deshalb auch in verschiedenen Verzeichnissen. Damit das so bleibt, sind die
//! Schalter mit *einem* festen Ziel im Stapelbetrieb verboten (`-o`,
//! `--review-out`, `--audit-log`, `--apply-review`) — sie würden für jede
//! Datei auf denselben Pfad zeigen. Dieselbe Datei zweimal genannt scheitert
//! beim zweiten Mal am Schreibpfad („existiert bereits“), wird gemeldet und
//! reißt nichts mit.
//!
//! ## Jede Datei wird genannt, bevor sie angefasst wird
//!
//! Vor der Stapelverarbeitung suchte immer ein Mensch die Datei aus; wer den
//! Lauf hängen sah, wusste, woran. Jetzt genügt eine Datei im Verzeichnis, und
//! die Zusammenfassung kommt erst am Ende. Eine dünn belegte Datei mit 6 GB
//! Nennlänge hielt den Lauf 21 s lang bei 6 149 MB Spitzenspeicher auf, ohne
//! dass ihr Name irgendwo stand. Deshalb geht der Name **vor** dem Öffnen nach
//! stderr — stderr, weil `--json` seine Zusammenfassung nach stdout schreibt
//! und die eine Maschine liest.
//!
//! ## Namen aus fremder Hand
//!
//! Ein Dateiname darf unter Unix fast jedes Byte enthalten, auch `ESC [ 2 K`.
//! Roh ausgegeben löscht das Zeilen und färbt Text — damit ließe sich diese
//! Zusammenfassung optisch fälschen, bis zur erfundenen Zeile
//! „0 fehlgeschlagen“. Jeder Name geht deshalb durch
//! [`redact_core::safe_path`]. Für `--json` braucht es das nicht: `serde_json`
//! schreibt Steuerzeichen selbst als ``.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use redact_core::{safe_path, safe_text, RedactError, Result};
use redact_pipeline::{Outcome, Settings};

use crate::cli::Cli;

/// Ergebnis einer einzelnen Datei des Stapels.
#[derive(serde::Serialize)]
struct Entry {
    input: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<Outcome>,
    /// Klartext des Fehlers, falls diese Datei gescheitert ist.
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Löst die Eingaben auf: Verzeichnisse werden zu ihren PDF-Dateien.
///
/// `suffix` ist der bereits aufgelöste Namenszusatz. Dateien, die ihn tragen,
/// sind Ergebnisse eines früheren Laufs und werden beim Auflösen eines
/// Verzeichnisses übergangen — sonst entstünde beim zweiten Lauf über
/// dasselbe Verzeichnis `auszug_geschwaerzt_geschwaerzt.pdf`. Eine
/// **ausdrücklich genannte** Datei wird nie übergangen.
pub fn gather_inputs(paths: &[PathBuf], suffix: &str) -> Result<Vec<PathBuf>> {
    let mut inputs = Vec::new();
    for path in paths {
        if path.is_dir() {
            inputs.extend(pdfs_in(path, suffix)?);
        } else {
            inputs.push(path.clone());
        }
    }
    if inputs.is_empty() {
        return Err(RedactError::Config(format!(
            "keine PDF-Datei gefunden in: {}",
            paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    Ok(inputs)
}

/// Die PDF-Dateien der obersten Ebene eines Verzeichnisses, alphabetisch.
///
/// Nicht rekursiv: wer einen ganzen Baum schwärzen will, sagt das mit der
/// Shell — ein Werkzeug, das ungefragt in Unterverzeichnisse absteigt, ist bei
/// Kontoauszügen die falsche Überraschung.
fn pdfs_in(dir: &Path, suffix: &str) -> Result<Vec<PathBuf>> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_pdf(path) && !already_redacted(path, suffix))
        .collect();
    found.sort();
    Ok(found)
}

fn is_pdf(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
}

fn already_redacted(path: &Path, suffix: &str) -> bool {
    !suffix.is_empty()
        && path
            .file_stem()
            .is_some_and(|stem| stem.to_string_lossy().ends_with(suffix))
}

/// Schalter, die auf genau ein Ziel zeigen und deshalb keinen Stapel vertragen.
fn reject_single_target_switches(cli: &Cli) -> Result<()> {
    let offending = [
        ("--output", cli.output.is_some()),
        ("--review-out", cli.review_out.is_some()),
        ("--audit-log", cli.audit_log.is_some()),
        ("--apply-review", cli.apply_review.is_some()),
    ]
    .into_iter()
    .find_map(|(name, given)| given.then_some(name));

    match offending {
        Some(name) => Err(RedactError::Config(format!(
            "{name} nennt ein festes Ziel und passt deshalb nicht zu mehreren \
             Eingabedateien — jedes Ergebnis überschriebe das vorige. Ohne den \
             Schalter entsteht je Datei ein Ergebnis daneben."
        ))),
        None => Ok(()),
    }
}

/// Arbeitet den Stapel ab.
///
/// Gibt [`ExitCode::FAILURE`] zurück, sobald **eine** Datei gescheitert ist —
/// der Rückgabewert ist die Antwort auf „ist alles gut gegangen?“, und
/// „größtenteils“ ist darauf keine Antwort.
pub fn run(cli: &Cli, settings: &Settings, inputs: &[PathBuf]) -> Result<ExitCode> {
    reject_single_target_switches(cli)?;

    let mut entries = Vec::with_capacity(inputs.len());
    let total = inputs.len();
    for (index, input) in inputs.iter().enumerate() {
        // **Vor** dem Öffnen: welche Datei ist gerade dran. Sonst hängt der
        // Lauf an einer Datei, deren Name nirgends steht — und je größer sie
        // ist, desto länger dauert das.
        if !cli.quiet {
            eprintln!("[{}/{total}] {}", index + 1, safe_path(input));
        }
        let entry = match redact_pipeline::run(&cli.config_for(settings, input)) {
            Ok(outcome) => Entry {
                input: input.display().to_string(),
                outcome: Some(outcome),
                error: None,
            },
            Err(e) => Entry {
                input: input.display().to_string(),
                outcome: None,
                error: Some(e.to_string()),
            },
        };
        // Gescheiterte Dateien werden sofort gemeldet — auch mit `--quiet` und
        // auch mit `--json`. Wer einen Stapel laufen lässt, sieht sonst erst
        // ganz am Ende, dass etwas nicht geklappt hat.
        if let Some(error) = &entry.error {
            eprintln!("FEHLGESCHLAGEN {}: {}", safe_path(input), safe_text(error));
        }
        entries.push(entry);
    }

    report(cli, &entries)?;
    let failed = entries.iter().filter(|e| e.error.is_some()).count();
    Ok(if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// Die Zusammenfassung am Ende.
fn report(cli: &Cli, entries: &[Entry]) -> Result<()> {
    if cli.json {
        println!("{}", serde_json::to_string_pretty(entries)?);
        return Ok(());
    }
    if cli.quiet {
        return Ok(());
    }

    for entry in entries {
        let Some(outcome) = &entry.outcome else {
            continue;
        };
        let target = outcome.output.as_deref().or(outcome.review_out.as_deref());
        println!(
            "{} → {} ({} Schwärzung(en))",
            safe_text(&entry.input),
            target.map(safe_text).unwrap_or_else(|| "—".to_string()),
            outcome.redactions
        );
    }

    let failed = entries.iter().filter(|e| e.error.is_some()).count();
    println!(
        "\n{} Datei(en): {} verarbeitet, {failed} fehlgeschlagen.",
        entries.len(),
        entries.len() - failed
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn workdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("redact-batch-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, b"%PDF-1.5\n").unwrap();
        path
    }

    /// Ein Verzeichnis wird zu seinen PDF-Dateien — sortiert, ohne Fremdes.
    #[test]
    fn a_directory_becomes_its_pdf_files() {
        let dir = workdir("dir");
        touch(&dir, "b.pdf");
        touch(&dir, "a.PDF");
        touch(&dir, "notiz.txt");
        std::fs::create_dir(dir.join("unterordner")).unwrap();
        touch(&dir.join("unterordner"), "tief.pdf");

        let found = gather_inputs(std::slice::from_ref(&dir), "_geschwaerzt").unwrap();
        assert_eq!(found, vec![dir.join("a.PDF"), dir.join("b.pdf")]);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Ergebnisse eines früheren Laufs werden beim Auflösen übergangen —
    /// sonst entstünde `_geschwaerzt_geschwaerzt`.
    #[test]
    fn results_of_an_earlier_run_are_skipped_in_a_directory() {
        let dir = workdir("skip");
        touch(&dir, "auszug.pdf");
        let done = touch(&dir, "auszug_geschwaerzt.pdf");

        assert_eq!(
            gather_inputs(std::slice::from_ref(&dir), "_geschwaerzt").unwrap(),
            vec![dir.join("auszug.pdf")]
        );
        // Ausdrücklich genannt wird sie trotzdem genommen.
        assert_eq!(
            gather_inputs(std::slice::from_ref(&done), "_geschwaerzt").unwrap(),
            vec![done]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_empty_directory_is_an_error_and_says_which_one() {
        let dir = workdir("leer");
        let error = gather_inputs(std::slice::from_ref(&dir), "_geschwaerzt")
            .expect_err("ein leeres Verzeichnis muss auffallen")
            .to_string();
        assert!(error.contains(&dir.display().to_string()), "{error}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Jeder Schalter mit festem Ziel wird im Stapelbetrieb abgelehnt — und
    /// die Meldung nennt ihn beim Namen.
    #[test]
    fn switches_with_a_single_target_are_refused_for_a_batch() {
        for (switch, value) in [
            ("-o", "out.pdf"),
            ("--review-out", "r.json"),
            ("--audit-log", "a.json"),
            ("--apply-review", "r.json"),
        ] {
            let cli = Cli::parse_from(["redact-rs", "a.pdf", "b.pdf", switch, value]);
            let error = reject_single_target_switches(&cli)
                .expect_err(&format!("{switch} muss abgelehnt werden"))
                .to_string();
            let long = if switch == "-o" { "--output" } else { switch };
            assert!(error.contains(long), "{error}");
        }
        // Ohne solche Schalter läuft der Stapel.
        let cli = Cli::parse_from(["redact-rs", "a.pdf", "b.pdf"]);
        assert!(reject_single_target_switches(&cli).is_ok());
    }
}
