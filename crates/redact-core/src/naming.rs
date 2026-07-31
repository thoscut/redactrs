//! Ableitung von Ausgabedateinamen.
//!
//! Voreinstellung für das Speichern: neben dem Original, mit einem Zusatz im
//! Dateinamen. Aus `kontoauszug.pdf` wird also `kontoauszug_geschwaerzt.pdf`.
//! Der Zusatz ist absichtlich ein Parameter und keine Konstante im Code —
//! er soll später konfigurierbar sein (Einstellungsdatei bzw. `--output-suffix`).

use std::path::{Path, PathBuf};

/// Standardzusatz für geschwärzte Dateien.
pub const DEFAULT_OUTPUT_SUFFIX: &str = "_geschwaerzt";

/// Zusatz für das Audit-Log.
pub const AUDIT_SUFFIX: &str = "_audit";

/// Zusatz für die Review-Datei.
pub const REVIEW_SUFFIX: &str = "_review";

/// Leitet aus dem Eingabepfad einen Vorschlag für die Ausgabedatei ab.
///
/// * Verzeichnis und Endung bleiben erhalten.
/// * `suffix` wird an den Dateinamen-Stamm angehängt.
/// * Ein leerer Zusatz fällt auf [`DEFAULT_OUTPUT_SUFFIX`] zurück — sonst
///   würde die Eingabedatei überschrieben.
/// * Hat die Datei keinen Stamm, wird ein sprechender Name erzeugt.
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
        DEFAULT_OUTPUT_SUFFIX
    } else {
        suffix
    };

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
