//! Ableitung von Ausgabedateinamen.
//!
//! Voreinstellung für das Speichern: neben dem Original, mit einem Zusatz im
//! Dateinamen. Aus `kontoauszug.pdf` wird also `kontoauszug_geschwaerzt.pdf`.
//! Der Zusatz ist absichtlich ein Parameter und keine Konstante im Code —
//! er soll später konfigurierbar sein (Einstellungsdatei bzw. `--output-suffix`).

use std::path::{Path, PathBuf};

use crate::{RedactError, Result};

/// Standardzusatz für geschwärzte Dateien.
pub const DEFAULT_OUTPUT_SUFFIX: &str = "_geschwaerzt";

/// Zusatz für das Audit-Log.
pub const AUDIT_SUFFIX: &str = "_audit";

/// Zusatz für die Review-Datei.
pub const REVIEW_SUFFIX: &str = "_review";

/// Prüft einen Namenszusatz, **bevor** er zu einem Pfad wird.
///
/// ## Warum das eine eigene Prüfung braucht
///
/// Ein Namenszusatz ist ein *Namens*bestandteil. Steht ein Pfadtrenner darin,
/// ist er keiner mehr, sondern ein Wegweiser: aus dem Zusatz
/// `/../../ziel/alle` und der Eingabe `b3/eins.pdf` wird
/// `b3/eins/../../ziel/alle.pdf`. Das hat drei Folgen, und alle drei sind
/// falsch.
///
/// 1. **Der Dateistamm geht verloren.** `eins` steht nur noch in einem
///    Verzeichnisnamen, den es nicht gibt; die Datei heißt `alle.pdf`.
/// 2. **Im Stapel kollidiert alles.** Jede Eingabe zeigt auf denselben Pfad,
///    jedes Ergebnis überschreibt das vorige. Gemessen: drei Dateien, Meldung
///    „3 verarbeitet, 0 fehlgeschlagen“, Rückgabewert 0 — und in `ziel/alle.pdf`
///    steht nur das Ergebnis der letzten. Genau diese Kollision verhindert
///    `batch::reject_single_target_switches` für `-o`; über den Zusatz war sie
///    weiter erreichbar, nur ohne jede Meldung.
/// 3. **Geschrieben wird außerhalb des Eingabeverzeichnisses.** `check_target`
///    legt fehlende Zwischenverzeichnisse mit `create_dir_all` an, der Pfad
///    entsteht also einfach. Symlinkschutz und Eingabeschutz halten zwar
///    weiterhin — aber sie sind die letzte Bremse, nicht die erste.
///
/// Abgelehnt werden deshalb: beide Pfadtrenner (`/` und `\`, unabhängig vom
/// System — eine Einstellungsdatei wandert zwischen Rechnern), die Folge `..`,
/// und Steuerzeichen (die machen aus einem Dateinamen eine Terminalsteuerung,
/// siehe [`crate::display`]).
///
/// Ein **leerer** Zusatz ist kein Fehler: er bedeutet „nichts angegeben“ und
/// führt in [`output_path_with_suffix`] auf [`DEFAULT_OUTPUT_SUFFIX`].
///
/// ```
/// use redact_core::check_output_suffix;
///
/// assert!(check_output_suffix("_geschwaerzt").is_ok());
/// assert!(check_output_suffix("/../../ziel/alle").is_err());
/// ```
pub fn check_output_suffix(suffix: &str) -> Result<()> {
    let reason = if suffix.contains('/') || suffix.contains('\\') {
        "er enthält einen Pfadtrenner"
    } else if suffix.contains("..") {
        "er enthält .."
    } else if suffix.chars().any(|c| c.is_control()) {
        "er enthält ein Steuerzeichen"
    } else {
        return Ok(());
    };

    Err(RedactError::Config(format!(
        "der Namenszusatz „{}“ ist keiner: {reason}. Der Zusatz wird an den \
         Dateinamen der Eingabe angehängt und darf deshalb nur aus \
         Namensbestandteilen bestehen — mit einem Pfadtrenner darin ginge der \
         Dateistamm verloren, im Stapel schriebe jede Datei auf dasselbe Ziel, \
         und geschrieben würde außerhalb des Eingabeverzeichnisses. Wer die \
         Ausgabe woanders haben will, gibt sie mit -o an.",
        crate::display::safe_text(suffix)
    )))
}

/// Leitet aus dem Eingabepfad einen Vorschlag für die Ausgabedatei ab.
///
/// * Verzeichnis und Endung bleiben erhalten.
/// * `suffix` wird an den Dateinamen-Stamm angehängt.
/// * Ein leerer Zusatz fällt auf [`DEFAULT_OUTPUT_SUFFIX`] zurück — sonst
///   würde die Eingabedatei überschrieben.
/// * Hat die Datei keinen Stamm, wird ein sprechender Name erzeugt.
///
/// **Die Prüfung des Zusatzes gehört nicht hierher**, sondern an den Rand:
/// [`check_output_suffix`] wird beim Einlesen der Einstellungsdatei und in
/// `redact_pipeline::plan_outputs` gerufen, also bevor irgendetwas geschieht.
/// Diese Funktion gibt einen Pfad zurück und keinen Fehler; sie kann nichts
/// melden. Was sie tut, ist die letzte Bremse: ein Pfadtrenner im Zusatz wird
/// hier zu `_`, damit ein künftiger Aufrufer, der die Prüfung vergisst, nicht
/// aus dem Verzeichnis der Eingabe herausschreibt. Erreicht wird diese Bremse
/// im laufenden Programm nie — der Test
/// `the_result_never_leaves_the_input_directory` hält fest, dass sie trotzdem
/// hält.
///
/// ```
/// use std::path::Path;
/// use redact_core::{output_path_with_suffix, DEFAULT_OUTPUT_SUFFIX};
///
/// let out = output_path_with_suffix(Path::new("/tmp/kontoauszug.pdf"), DEFAULT_OUTPUT_SUFFIX);
/// assert_eq!(out, Path::new("/tmp/kontoauszug_geschwaerzt.pdf"));
/// ```
pub fn output_path_with_suffix(input: &Path, suffix: &str) -> PathBuf {
    let suffix = if suffix.trim().is_empty() {
        DEFAULT_OUTPUT_SUFFIX.to_string()
    } else {
        neutralise(suffix)
    };
    let suffix = suffix.as_str();

    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "dokument".to_string());

    let mut name = format!("{stem}{suffix}");
    if let Some(ext) = input.extension() {
        name.push('.');
        name.push_str(&ext.to_string_lossy());
    } else {
        name.push_str(".pdf");
    }

    match input.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(name),
        _ => PathBuf::from(name),
    }
}

/// Die letzte Bremse aus [`output_path_with_suffix`]: alles, was aus einem
/// Namen einen Weg machen würde, wird zu `_`.
fn neutralise(suffix: &str) -> String {
    suffix
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c == std::path::MAIN_SEPARATOR || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// Wie [`output_path_with_suffix`], aber mit anderer Endung — für Audit-Log
/// und Review-Datei.
pub fn sibling_path(input: &Path, suffix: &str, extension: &str) -> PathBuf {
    let mut path = output_path_with_suffix(input, suffix);
    path.set_extension(extension);
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_suffix_before_extension() {
        assert_eq!(
            output_path_with_suffix(Path::new("/daten/kontoauszug.pdf"), "_geschwaerzt"),
            PathBuf::from("/daten/kontoauszug_geschwaerzt.pdf")
        );
    }

    #[test]
    fn keeps_relative_paths_relative() {
        assert_eq!(
            output_path_with_suffix(Path::new("kontoauszug.pdf"), "_x"),
            PathBuf::from("kontoauszug_x.pdf")
        );
    }

    #[test]
    fn adds_pdf_extension_when_missing() {
        assert_eq!(
            output_path_with_suffix(Path::new("/daten/kontoauszug"), "_geschwaerzt"),
            PathBuf::from("/daten/kontoauszug_geschwaerzt.pdf")
        );
    }

    #[test]
    fn empty_suffix_falls_back_to_default() {
        let out = output_path_with_suffix(Path::new("/daten/a.pdf"), "   ");
        assert_eq!(out, PathBuf::from("/daten/a_geschwaerzt.pdf"));
    }

    #[test]
    fn never_returns_the_input_path() {
        for input in ["/daten/a.pdf", "a.pdf", "/daten/.pdf", "a"] {
            let path = Path::new(input);
            for suffix in ["", "_geschwaerzt", "_x"] {
                assert_ne!(output_path_with_suffix(path, suffix), path.to_path_buf());
            }
        }
    }

    #[test]
    fn handles_dotfiles_without_stem_gracefully() {
        // `.pdf` hat den Stamm ".pdf" und keine Endung — der Vorschlag muss
        // trotzdem sinnvoll sein und darf nicht die Eingabe treffen.
        let out = output_path_with_suffix(Path::new(".pdf"), "_geschwaerzt");
        assert_ne!(out, PathBuf::from(".pdf"));
        assert!(out.to_string_lossy().contains("_geschwaerzt"));
    }

    // ------------------------------------------- Zusatz mit Pfadanteilen

    /// Der Fall aus dem Befund und seine Nachbarn — alle abgelehnt, und die
    /// Meldung nennt den Zusatz.
    #[test]
    fn a_suffix_with_a_path_component_is_refused() {
        for suffix in [
            "/../../ziel/alle",
            "../alle",
            "unter/alle",
            "a\\b",
            "..",
            "_x/",
            "/",
        ] {
            let error = check_output_suffix(suffix).unwrap_err().to_string();
            assert!(
                error.contains("Namenszusatz"),
                "{suffix}: unerwartete Meldung {error}"
            );
        }
    }

    /// Ein Steuerzeichen im Zusatz landete sonst in jedem erzeugten Dateinamen
    /// — und von dort in jeder Zeile der Zusammenfassung.
    #[test]
    fn a_suffix_with_a_control_character_is_refused() {
        assert!(check_output_suffix("_a\u{1b}[31m").is_err());
        assert!(check_output_suffix("_a\nb").is_err());
    }

    /// Und die Gegenprobe: gewöhnliche Zusätze bleiben erlaubt, der leere
    /// eingeschlossen (er heißt „nicht angegeben“).
    #[test]
    fn ordinary_suffixes_stay_allowed() {
        for suffix in [
            "",
            "   ",
            DEFAULT_OUTPUT_SUFFIX,
            "_anonym",
            "-geschwärzt",
            "_2026.05.01",
            " (geschwärzt)",
        ] {
            assert!(check_output_suffix(suffix).is_ok(), "{suffix:?}");
        }
    }

    /// Die letzte Bremse: selbst mit einem ungeprüften Zusatz bleibt das
    /// Ergebnis im Verzeichnis der Eingabe.
    ///
    /// Im laufenden Programm kommt hier nichts Ungeprüftes an — geprüft wird
    /// beim Einlesen der Einstellungen und noch einmal vor dem ersten Schreiben.
    /// Dieser Test misst, was passierte, wenn ein neuer Aufrufer beides
    /// vergäße.
    #[test]
    fn the_result_never_leaves_the_input_directory() {
        let input = Path::new("/daten/b3/eins.pdf");
        for suffix in ["/../../ziel/alle", "../alle", "unter/alle", "a\\b"] {
            let out = output_path_with_suffix(input, suffix);
            assert_eq!(
                out.parent(),
                Some(Path::new("/daten/b3")),
                "{suffix} führt aus dem Verzeichnis heraus: {}",
                out.display()
            );
            // Und der Stamm der Eingabe steht weiterhin im Namen: zwei
            // Eingaben können sich damit nicht auf dieselbe Ausgabe treffen.
            assert!(
                out.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("eins"),
                "{suffix}: der Dateistamm ist weg ({})",
                out.display()
            );
        }
    }

    #[test]
    fn sibling_path_swaps_extension() {
        assert_eq!(
            sibling_path(Path::new("/daten/kontoauszug.pdf"), AUDIT_SUFFIX, "json"),
            PathBuf::from("/daten/kontoauszug_audit.json")
        );
        assert_eq!(
            sibling_path(Path::new("/daten/kontoauszug.pdf"), REVIEW_SUFFIX, "json"),
            PathBuf::from("/daten/kontoauszug_review.json")
        );
    }
}
