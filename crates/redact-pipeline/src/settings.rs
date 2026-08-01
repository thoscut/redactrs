//! Die Einstellungsdatei — Vorgaben, die nicht bei jedem Aufruf getippt werden.
//!
//! ## Eine Struktur, ein `Default`
//!
//! [`Settings`] ist die *einzige* Schicht zwischen den eingebauten Vorgaben und
//! der Kommandozeile. Kein Schichtenmodell, keine Zusammenführung mehrerer
//! Dateien, kein „Projekt- schlägt Benutzer-Einstellung“: eine Datei, eine
//! Struktur, ein [`Default`]. Fehlt die Datei, gilt [`Settings::default`] —
//! und das sind exakt die Werte, die das Werkzeug ohne Datei auch hätte.
//!
//! ## Rangfolge
//!
//! ```text
//! Kommandozeile  schlägt  Einstellungsdatei  schlägt  Vorgabe
//! ```
//!
//! Angewendet wird sie an genau einer Stelle: `redact_cli::Cli::config`. Die
//! Schalter der Kommandozeile sind dort `Option`, und `None` heißt „nicht
//! angegeben“ — deshalb braucht es kein Nachfragen bei clap, welcher Wert vom
//! Nutzer stammt und welcher aus einem `default_value`.
//!
//! ## Warum YAML
//!
//! `serde_yaml` steht bereits im Abhängigkeitsbaum (`--patterns-config` liest
//! YAML), es kommt also keine Abhängigkeit hinzu. Gegenüber JSON hat YAML für
//! eine von Hand gepflegte Datei den entscheidenden Vorteil, Kommentare zu
//! erlauben — und diese Datei wird von Hand gepflegt.
//!
//! ## Wo die Datei liegt
//!
//! | System        | Pfad                                        |
//! |---------------|---------------------------------------------|
//! | Linux, BSD    | `$XDG_CONFIG_HOME/redact-rs/settings.yaml` bzw. `~/.config/redact-rs/settings.yaml` |
//! | macOS         | `~/.config/redact-rs/settings.yaml`         |
//! | Windows       | `%APPDATA%\redact-rs\settings.yaml`         |
//!
//! `REDACT_RS_CONFIG` zeigt auf eine andere Datei und schlägt alles davon —
//! das ist zugleich der Weg, mit dem die Tests ohne echtes Benutzerprofil
//! auskommen.
//!
//! Ermittelt wird der Pfad aus den Umgebungsvariablen, nicht über eine
//! zusätzliche Abhängigkeit (`dirs`, `directories`): es sind drei Zeilen, und
//! jede weitere Kiste im Baum will gepflegt und geprüft werden. Auf macOS
//! benutzt redact-rs bewusst `~/.config` statt `~/Library/Application Support`
//! — es ist ein Kommandozeilenwerkzeug, und die Datei soll dort liegen, wo man
//! sie mit einem Editor sucht.

use std::path::{Path, PathBuf};

use redact_core::{RedactError, Result};

/// Name der Einstellungsdatei unterhalb des Konfigurationsverzeichnisses.
pub const SETTINGS_FILE: &str = "redact-rs/settings.yaml";

/// Umgebungsvariable, die den Pfad der Einstellungsdatei überschreibt.
pub const SETTINGS_ENV: &str = "REDACT_RS_CONFIG";

/// Erlaubte Werte für [`Settings::theme`].
pub const THEMES: [&str; 2] = ["hell", "dunkel"];

/// Die Einstellungsdatei als Struktur.
///
/// Jedes Feld hat eine eingebaute Vorgabe; eine Datei, die nur einen Schlüssel
/// nennt, ändert auch nur den einen. Unbekannte Schlüssel werden **abgelehnt**
/// statt überlesen: ein Tippfehler in `output_suffix` wäre sonst eine
/// Einstellung, die stillschweigend nicht wirkt.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// Namenszusatz der Ausgabedatei (`--output-suffix`).
    pub output_suffix: String,
    /// Standardmäßig benutzte Muster (`--patterns`); leer = die eingebaute Auswahl.
    pub patterns: Vec<String>,
    /// Mindestvertrauen (`--min-confidence`).
    ///
    /// `None` heißt „nicht festgelegt“ und ist etwas anderes als ein Wert:
    /// nur dann behält eine eigene Pattern-Konfigurationsdatei ihre eigene
    /// Schwelle. Ein hier eingetragener Wert überschreibt sie.
    pub min_confidence: Option<f32>,
    /// Polsterung um jede Schwärzung in Punkt (`--padding`).
    pub padding: f64,
    /// Thema der Oberfläche: `hell` oder `dunkel`.
    pub theme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_suffix: redact_core::DEFAULT_OUTPUT_SUFFIX.to_string(),
            patterns: Vec::new(),
            min_confidence: None,
            padding: crate::DEFAULT_PADDING,
            theme: THEMES[0].to_string(),
        }
    }
}

impl Settings {
    /// Liest die Einstellungsdatei des Systems; fehlt sie, gilt [`Default`].
    ///
    /// Eine **vorhandene**, aber fehlerhafte Datei ist ein Fehler und wird
    /// nicht stillschweigend übergangen — sonst arbeitete das Werkzeug mit
    /// anderen Werten als der Nutzer meint, eingestellt zu haben.
    pub fn load() -> Result<Self> {
        match settings_path() {
            Some(path) if path.exists() => Self::load_from(&path),
            _ => Ok(Self::default()),
        }
    }

    /// Liest eine bestimmte Datei.
    pub fn load_from(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            RedactError::Config(format!(
                "Einstellungsdatei {} nicht lesbar: {e}",
                path.display()
            ))
        })?;
        Self::from_yaml(&text).map_err(|e| match e {
            RedactError::Config(msg) => RedactError::Config(format!("{}: {msg}", path.display())),
            other => other,
        })
    }

    /// Wertet den Inhalt einer Einstellungsdatei aus.
    pub fn from_yaml(text: &str) -> Result<Self> {
        // Eine leere Datei ist `null` und keine leere Abbildung — sie soll die
        // Vorgaben ergeben und keinen Fehler.
        if text.trim().is_empty() {
            return Ok(Self::default());
        }
        let settings: Self = serde_yaml::from_str(text)
            .map_err(|e| RedactError::Config(format!("Einstellungen nicht lesbar: {e}")))?;
        settings.validate()?;
        Ok(settings)
    }

    /// Prüft, was `serde` nicht prüfen kann.
    fn validate(&self) -> Result<()> {
        if !THEMES.contains(&self.theme.as_str()) {
            return Err(RedactError::Config(format!(
                "unbekanntes Thema „{}“ — erlaubt sind {}",
                self.theme,
                THEMES.join(" und ")
            )));
        }
        Ok(())
    }
}

/// Pfad der Einstellungsdatei, oder `None`, wenn sich kein Heimatverzeichnis
/// ermitteln lässt.
pub fn settings_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(SETTINGS_ENV) {
        return Some(PathBuf::from(path));
    }
    config_dir().map(|dir| dir.join(SETTINGS_FILE))
}

/// Konfigurationsverzeichnis des Systems.
fn config_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        return std::env::var_os("APPDATA").map(PathBuf::from);
    }
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir));
        }
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ohne Datei gilt genau das, was das Werkzeug ohne Einstellungen tut.
    #[test]
    fn the_default_is_the_built_in_behaviour() {
        let settings = Settings::default();
        assert_eq!(settings.output_suffix, redact_core::DEFAULT_OUTPUT_SUFFIX);
        assert_eq!(settings.padding, crate::DEFAULT_PADDING);
        assert!(settings.patterns.is_empty());
        assert_eq!(settings.min_confidence, None);
        assert_eq!(settings.theme, "hell");
    }

    /// Eine Datei, die einen Schlüssel nennt, ändert auch nur diesen einen.
    #[test]
    fn a_partial_file_leaves_the_other_defaults_alone() {
        let settings = Settings::from_yaml("padding: 4.5\n").unwrap();
        assert_eq!(settings.padding, 4.5);
        assert_eq!(settings.output_suffix, redact_core::DEFAULT_OUTPUT_SUFFIX);
        assert_eq!(settings.theme, "hell");
    }

    #[test]
    fn an_empty_file_is_not_an_error() {
        assert_eq!(Settings::from_yaml("").unwrap(), Settings::default());
        assert_eq!(
            Settings::from_yaml("# nur ein Kommentar\n").unwrap(),
            Settings::default()
        );
    }

    /// Ein Tippfehler im Schlüssel muss auffallen — eine übergangene
    /// Einstellung wäre schlimmer als eine Fehlermeldung.
    #[test]
    fn an_unknown_key_is_refused() {
        let error = Settings::from_yaml("output_sufix: _x\n")
            .expect_err("Tippfehler muss auffallen")
            .to_string();
        assert!(error.contains("output_sufix"), "{error}");
    }

    #[test]
    fn an_unknown_theme_is_refused_with_the_allowed_values() {
        let error = Settings::from_yaml("theme: neon\n")
            .expect_err("unbekanntes Thema muss auffallen")
            .to_string();
        assert!(error.contains("neon"), "{error}");
        assert!(
            error.contains("hell") && error.contains("dunkel"),
            "{error}"
        );
    }

    #[test]
    fn every_field_can_be_set() {
        let settings = Settings::from_yaml(
            "output_suffix: _anonym\npatterns: [iban_de, bic]\nmin_confidence: 0.25\n\
             padding: 2.0\ntheme: dunkel\n",
        )
        .unwrap();
        assert_eq!(settings.output_suffix, "_anonym");
        assert_eq!(settings.patterns, vec!["iban_de", "bic"]);
        assert_eq!(settings.min_confidence, Some(0.25));
        assert_eq!(settings.padding, 2.0);
        assert_eq!(settings.theme, "dunkel");
    }

    /// Die Datei benennt der Nutzer, wenn etwas darin nicht stimmt.
    #[test]
    fn a_broken_file_names_itself() {
        let dir = std::env::temp_dir().join(format!("redact-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.yaml");
        std::fs::write(&path, "theme: neon\n").unwrap();
        let error = Settings::load_from(&path).unwrap_err().to_string();
        assert!(error.contains("settings.yaml"), "{error}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// `REDACT_RS_CONFIG` schlägt das Konfigurationsverzeichnis.
    ///
    /// Der Test setzt keine Umgebungsvariable (das wäre in einem Prozess mit
    /// parallelen Tests nicht sicher), sondern prüft beide Zweige über das,
    /// was sie unterscheidet: gesetzt ⇒ genau dieser Pfad.
    #[test]
    fn the_environment_variable_wins_over_the_config_directory() {
        match std::env::var_os(SETTINGS_ENV) {
            Some(value) => assert_eq!(settings_path(), Some(PathBuf::from(value))),
            None => {
                let path = settings_path().expect("Konfigurationsverzeichnis ermittelbar");
                assert!(path.ends_with(SETTINGS_FILE), "{}", path.display());
            }
        }
    }
}
