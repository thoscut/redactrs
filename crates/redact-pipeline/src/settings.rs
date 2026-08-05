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

/// Die Schlüssel, die [`Settings`] kennt — für die Fehlermeldung.
///
/// Sie stehen hier von Hand, weil `serde` sie zwar in seiner eigenen Meldung
/// aufzählt, diese Meldung aber nicht benutzt wird (siehe
/// [`Settings::from_yaml`]). Der Test `the_listed_keys_are_the_real_ones`
/// misst nach, dass die Liste stimmt.
pub const KEYS: [&str; 6] = [
    "output_suffix",
    "patterns",
    "disabled_patterns",
    "min_confidence",
    "padding",
    "theme",
];

/// Obergrenze für die Einstellungsdatei.
///
/// Sie ist eine von Hand gepflegte Datei mit einer Handvoll Schlüsseln; ein Kilobyte
/// reicht dafür tausendfach. Die Grenze steht trotzdem da, und zwar aus
/// demselben Grund wie die für die Eingabe-PDF: `REDACT_RS_CONFIG` zeigt auf
/// einen beliebigen Pfad, und `std::fs::read_to_string` liest, was da steht —
/// eine dünn belegte Datei mit 6 GB Nennlänge belegte 6 GB Arbeitsspeicher,
/// bevor die erste Zeile ausgewertet wäre.
pub const MAX_SETTINGS_BYTES: u64 = 1024 * 1024;

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
    /// Standardmäßig abgeschaltete Muster (`--disable-pattern`).
    ///
    /// Für Muster, die im eigenen Bestand mehr Fehltreffer als Funde erzeugen
    /// — `date_de` ist der Regelfall. Die Kommandozeile schlägt die Datei: eine
    /// nicht leere Angabe hinter `--disable-pattern` ersetzt diese Liste.
    ///
    /// ## Warum es hier kein `no_patterns` gibt
    ///
    /// „Alle automatischen Funde aus“ ist die weitreichendste Einstellung, die
    /// dieses Werkzeug kennt, und sie ließe sich hier **nicht mehr
    /// widerrufen**: `--no-patterns` ist ein Schalter ohne Gegenstück, es gibt
    /// kein `--patterns-an`. Stünde er in der Datei, arbeitete jeder Aufruf
    /// ohne Erkennung, und die Kommandozeile hätte kein Mittel dagegen — genau
    /// die stille falsche Entwarnung, gegen die die Meldung in
    /// [`crate::detection_notice`] gedacht ist. Ein einzelnes Muster
    /// abzuschalten ist etwas anderes: die Angabe ist widerrufbar, weil eine
    /// Liste auf der Kommandozeile die Liste aus der Datei ersetzt.
    pub disabled_patterns: Vec<String>,
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
            disabled_patterns: Vec::new(),
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
    ///
    /// Gelesen wird erst, nachdem feststeht, dass es eine gewöhnliche Datei
    /// von vernünftiger Größe ist — `REDACT_RS_CONFIG` zeigt auf einen
    /// beliebigen Pfad, und der kann eine benannte Pipe oder eine 6 GB große
    /// dünn belegte Datei sein.
    pub fn load_from(path: &Path) -> Result<Self> {
        let name = redact_core::safe_path(path);
        let meta = std::fs::metadata(path).map_err(|e| {
            RedactError::Config(format!("Einstellungsdatei {name} nicht lesbar: {e}"))
        })?;
        if !meta.is_file() {
            return Err(RedactError::Config(format!(
                "Einstellungsdatei {name} ist keine gewöhnliche Datei"
            )));
        }
        if meta.len() > MAX_SETTINGS_BYTES {
            return Err(RedactError::Config(format!(
                "Einstellungsdatei {name} ist mit {} Byte zu groß (Grenze {} Byte). \
                 Die Einstellungsdatei hat {} Schlüssel; das ist keine.",
                meta.len(),
                MAX_SETTINGS_BYTES,
                KEYS.len()
            )));
        }
        let text = std::fs::read_to_string(path).map_err(|e| {
            RedactError::Config(format!("Einstellungsdatei {name} nicht lesbar: {e}"))
        })?;
        Self::from_yaml(&text).map_err(|e| match e {
            RedactError::Config(msg) => RedactError::Config(format!("{name}: {msg}")),
            other => other,
        })
    }

    /// Wertet den Inhalt einer Einstellungsdatei aus.
    ///
    /// ## Warum die Meldung den Dateiinhalt nicht wiedergibt
    ///
    /// `serde_yaml` schreibt in seine Fehlermeldung, woran es lag — und dazu
    /// gehört der Text, der nicht gepasst hat. Bei einer Einstellungsdatei ist
    /// das genau richtig. Nur ist nicht gesichert, dass hier eine
    /// Einstellungsdatei liegt: `REDACT_RS_CONFIG` zeigt auf einen beliebigen
    /// Pfad, und an der Vorgabestelle kann ein Symlink stehen. Zeigt einer der
    /// beiden auf `/etc/shadow`, lautete die Meldung bisher
    ///
    /// ```text
    /// Einstellungen nicht lesbar: unknown field `root:*:20501:0:99999:7::`, …
    /// ```
    ///
    /// Keine Rechtegrenze wird dabei überschritten — das Programm liest mit den
    /// Rechten des Nutzers, der die Datei ohnehin lesen dürfte. Es ist aber ein
    /// Weg, beliebige Zeilen einer fremden Datei in Protokolle,
    /// Fehlerberichte und Bildschirmfotos zu befördern, und dafür gibt es
    /// keinen Grund.
    ///
    /// Deshalb wird die Meldung hier selbst gebaut: Art des Fehlers, Zeile und
    /// Spalte, die erlaubten Schlüssel. Der beanstandete Schlüssel wird nur
    /// dann genannt, wenn er *wie ein Schlüssel dieser Datei aussieht* — siehe
    /// [`echoable_key`]. Ein Tippfehler (`output_sufix`) ist damit weiterhin
    /// beim Namen genannt, eine Zeile aus einer Passwortdatei nicht.
    pub fn from_yaml(text: &str) -> Result<Self> {
        // Eine leere Datei ist `null` und keine leere Abbildung — sie soll die
        // Vorgaben ergeben und keinen Fehler.
        if text.trim().is_empty() {
            return Ok(Self::default());
        }
        let settings: Self = serde_yaml::from_str(text).map_err(describe_yaml_error)?;
        settings.validate()?;
        Ok(settings)
    }

    /// Prüft, was `serde` nicht prüfen kann.
    fn validate(&self) -> Result<()> {
        if !THEMES.contains(&self.theme.as_str()) {
            return Err(RedactError::Config(format!(
                "unbekanntes Thema „{}“ — erlaubt sind {}",
                redact_core::safe_text(&self.theme),
                THEMES.join(" und ")
            )));
        }
        // Ein Namenszusatz mit Pfadanteilen ist kein Zusatz, sondern ein
        // Wegweiser aus dem Verzeichnis heraus. Geprüft wird hier, weil die
        // Einstellungsdatei der Ort ist, an dem er üblicherweise steht —
        // `plan_outputs` prüft ihn ein zweites Mal, dann für alle Wege.
        redact_core::check_output_suffix(&self.output_suffix)?;
        Ok(())
    }
}

/// Baut aus einem `serde_yaml`-Fehler eine Meldung **ohne** Dateiinhalt.
fn describe_yaml_error(error: serde_yaml::Error) -> RedactError {
    let raw = error.to_string();
    let kind = if raw.starts_with("unknown field") {
        "unbekannter Schlüssel"
    } else if raw.starts_with("missing field") {
        "ein Schlüssel fehlt"
    } else if raw.starts_with("duplicate") {
        "ein Schlüssel steht doppelt"
    } else if raw.starts_with("invalid type") || raw.starts_with("invalid value") {
        "ein Wert hat die falsche Art"
    } else {
        "die Datei ist kein YAML dieser Form"
    };

    let mut message = String::from("Einstellungen nicht lesbar");
    if let Some(location) = error.location() {
        message.push_str(&format!(
            " (Zeile {}, Spalte {})",
            location.line(),
            location.column()
        ));
    }
    message.push_str(": ");
    message.push_str(kind);
    if let Some(key) = echoable_key(&raw) {
        message.push_str(&format!(" `{key}`"));
    }
    message.push_str(&format!(
        ". Erlaubt sind: {}. (Der Inhalt der Datei wird hier nicht \
         wiedergegeben — {SETTINGS_ENV} und die Vorgabestelle können auf eine \
         beliebige fremde Datei zeigen.)",
        KEYS.join(", ")
    ));
    RedactError::Config(message)
}

/// Der beanstandete Schlüssel — aber nur, wenn er einer sein könnte.
///
/// `serde` setzt ihn in Rückwärts-Anführungszeichen. Wiedergegeben wird er nur,
/// wenn er aussieht wie ein Schlüssel dieser Datei: höchstens 32 Zeichen, ASCII,
/// beginnend mit Buchstabe oder `_`, danach Buchstaben, Ziffern, `_` und `-`.
/// `output_sufix` besteht die Prüfung, `root:*:20501:0:99999:7::` nicht — und
/// eine Zeile, die sie besteht, trägt nichts, was ein Angreifer irgendwo
/// hinbekommen wollte.
fn echoable_key(raw: &str) -> Option<&str> {
    let rest = raw.split_once('`')?.1;
    let key = rest.split_once('`')?.0;
    let mut chars = key.chars();
    let first = chars.next()?;
    let ok = key.len() <= 32
        && (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    ok.then_some(key)
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
        assert!(settings.disabled_patterns.is_empty());
        assert_eq!(settings.min_confidence, None);
        assert_eq!(settings.theme, "hell");
    }

    /// „Alles aus“ darf nicht in der Datei stehen: der Schalter hat kein
    /// Gegenstück auf der Kommandozeile und wäre damit nicht mehr zu widerrufen.
    #[test]
    fn the_file_cannot_switch_off_the_detection_as_a_whole() {
        let error = Settings::from_yaml("no_patterns: true\n")
            .expect_err("no_patterns gehört nicht in die Einstellungsdatei")
            .to_string();
        assert!(error.contains("unbekannter Schlüssel"), "{error}");
        // Einzelne Muster dagegen schon — die Liste ist widerrufbar.
        let settings = Settings::from_yaml("disabled_patterns: [date_de, bic]\n").unwrap();
        assert_eq!(settings.disabled_patterns, vec!["date_de", "bic"]);
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
        // Und die Meldung sagt, was stattdessen erlaubt wäre.
        for key in KEYS {
            assert!(error.contains(key), "{key} fehlt in: {error}");
        }
    }

    /// Die aufgezählten Schlüssel sind wirklich die der Struktur.
    ///
    /// [`KEYS`] steht von Hand da; ohne diese Gegenprobe zeigte die Meldung
    /// nach einem neuen Feld auf eine veraltete Liste.
    #[test]
    fn the_listed_keys_are_the_real_ones() {
        for key in KEYS {
            let text = format!("{key}: nix\n");
            let error = Settings::from_yaml(&text)
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            assert!(
                !error.contains("unbekannter Schlüssel"),
                "{key} steht in KEYS, ist aber keiner: {error}"
            );
        }
        // Gegenprobe: ein erfundener Schlüssel fällt auf.
        let error = Settings::from_yaml("gibt_es_nicht: 1\n")
            .expect_err("unbekannter Schlüssel")
            .to_string();
        assert!(error.contains("unbekannter Schlüssel"), "{error}");
    }

    // ------------------------------------------ Befund 7: fremder Inhalt

    /// **Die Auflage:** die Meldung gibt keine Zeile der Datei wieder.
    ///
    /// Gemessen an dem, was der Befund benutzt hat: `REDACT_RS_CONFIG` auf
    /// `/etc/shadow` (bzw. hier eine Datei desselben Aufbaus). Vorher stand
    /// die erste Zeile wörtlich in der Meldung.
    #[test]
    fn the_message_does_not_quote_the_contents_of_a_foreign_file() {
        const SHADOW: &str = "root:*:20501:0:99999:7::\n\
                              nutzer:$6$abcdefgh$SEHRGEHEIM:20501:0:99999:7:::\n";
        const PASSWD: &str = "root:x:0:0:root:/root:/bin/bash\n";
        const PRIVATE_KEY: &str = "-----BEGIN OPENSSH PRIVATE KEY-----\n\
                                   b3BlbnNzaC1rZXktdjEAAAAABG5vbmU\n";

        for content in [SHADOW, PASSWD, PRIVATE_KEY] {
            let error = Settings::from_yaml(content)
                .expect_err("eine fremde Datei ist keine Einstellungsdatei")
                .to_string();
            for line in content.lines().filter(|l| !l.trim().is_empty()) {
                assert!(
                    !error.contains(line.trim()),
                    "die Meldung gibt eine Zeile der Datei wieder:\n  Zeile: {line}\n  Meldung: {error}"
                );
            }
            // Auch nicht in Stücken: die auffälligen Teile fehlen ebenso.
            for secret in ["20501", "SEHRGEHEIM", "/bin/bash", "b3BlbnNzaC1r"] {
                assert!(!error.contains(secret), "{secret} steht in: {error}");
            }
            // Die Meldung bleibt trotzdem brauchbar: sie sagt, wo es klemmt.
            assert!(error.contains("Zeile"), "{error}");
        }
    }

    /// Auch ein Schlüssel, der *fast* wie einer aussieht, wird nicht
    /// wiedergegeben, sobald er Zeichen enthält, die in keinem Schlüssel
    /// dieser Datei vorkommen.
    #[test]
    fn only_key_shaped_names_are_echoed() {
        assert_eq!(
            echoable_key("unknown field `output_sufix`, expected"),
            Some("output_sufix")
        );
        assert_eq!(
            echoable_key("unknown field `root:*:20501:0:99999:7::`, e"),
            None
        );
        assert_eq!(echoable_key("unknown field `-----BEGIN OPENSSH`, e"), None);
        assert_eq!(echoable_key("unknown field `/etc/passwd`, e"), None);
        // Zu lang, also eher eine Datenzeile als ein Schlüssel.
        assert_eq!(
            echoable_key("unknown field `aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa`, e"),
            None
        );
        assert_eq!(echoable_key("kein Zitat darin"), None);
    }

    // -------------------------------- Befund 5: Zusatz mit Pfadanteilen

    /// Ein Namenszusatz mit Pfadanteilen wird schon beim Lesen der Datei
    /// abgelehnt — nicht erst, wenn drei Ergebnisse übereinander liegen.
    #[test]
    fn a_suffix_with_a_path_component_is_refused_in_the_settings_file() {
        let error = Settings::from_yaml("output_suffix: \"/../../ziel/alle\"\n")
            .expect_err("ein Zusatz mit Pfadtrenner muss auffallen")
            .to_string();
        assert!(error.contains("Namenszusatz"), "{error}");
        assert!(error.contains("Pfadtrenner"), "{error}");
        // Der gewöhnliche Fall bleibt erlaubt.
        assert!(Settings::from_yaml("output_suffix: _anonym\n").is_ok());
    }

    // ------------------------------------- Befund 4, kleiner Bruder davon

    /// Eine riesige „Einstellungsdatei“ wird nicht in den Speicher gelesen.
    #[test]
    fn an_oversized_settings_file_is_refused_before_it_is_read() {
        let dir =
            std::env::temp_dir().join(format!("redact-settings-gross-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.yaml");
        std::fs::write(&path, vec![b'#'; MAX_SETTINGS_BYTES as usize + 1]).unwrap();
        let error = Settings::load_from(&path)
            .expect_err("zu groß muss auffallen")
            .to_string();
        assert!(error.contains("zu groß"), "{error}");
        std::fs::remove_dir_all(&dir).ok();
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
            "output_suffix: _anonym\npatterns: [iban_de, bic]\ndisabled_patterns: [bic]\n\
             min_confidence: 0.25\npadding: 2.0\ntheme: dunkel\n",
        )
        .unwrap();
        assert_eq!(settings.output_suffix, "_anonym");
        assert_eq!(settings.patterns, vec!["iban_de", "bic"]);
        assert_eq!(settings.disabled_patterns, vec!["bic"]);
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
