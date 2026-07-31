//! Kompilieren von Patterns, Laden aus Konfigurationsdateien und das eigentliche
//! Matching auf Text-Runs.

use std::path::Path;

use fancy_regex::Regex;
use redact_core::{Analyzer, RedactError, Region, Result, Source, TextRun};
use serde::{Deserialize, Deserializer, Serialize};

use crate::builtin::{builtin_pattern_ids, builtin_patterns};
use crate::validate::{validate_bic, validate_iban, validate_luhn};
use crate::{PatternDef, Validator};

/// Konfidenz, auf die ein erfolgreich geprüfter Treffer mindestens angehoben wird.
const VALIDATED_CONFIDENCE: f32 = 0.99;

/// Ein Eintrag in einer Pattern-Konfigurationsdatei.
///
/// Alle Felder außer `id` sind optional: fehlende Felder lassen den Wert des
/// eingebauten Patterns unverändert (siehe [`PatternConfig::extend_builtins`]).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternEntry {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Doppeltes `Option`, damit `validator: null` einen eingebauten Validator
    /// entfernen kann, während ein fehlendes Feld ihn unverändert lässt.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub validator: Option<Option<Validator>>,
}

/// Unterscheidet „Feld fehlt" von „Feld ist null".
fn deserialize_present<'de, D, T>(de: D) -> std::result::Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(de).map(Some)
}

/// Inhalt einer Pattern-Konfigurationsdatei (YAML oder JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternConfig {
    /// `true` (Vorgabe): die Einträge ergänzen bzw. überschreiben die
    /// eingebauten Patterns. `false`: nur die gelisteten Patterns werden benutzt.
    #[serde(default = "crate::default_true")]
    pub extend_builtins: bool,
    #[serde(default)]
    pub patterns: Vec<PatternEntry>,
}

impl Default for PatternConfig {
    fn default() -> Self {
        Self {
            extend_builtins: true,
            patterns: Vec::new(),
        }
    }
}

impl PatternConfig {
    /// Baut die endgültige Pattern-Liste aus Konfiguration und Vorgaben.
    pub fn resolve(&self) -> Result<Vec<PatternDef>> {
        let mut defs: Vec<PatternDef> = if self.extend_builtins {
            builtin_patterns()
        } else {
            Vec::new()
        };

        for entry in &self.patterns {
            if entry.id.trim().is_empty() {
                return Err(RedactError::Pattern(
                    "Pattern-Eintrag ohne 'id' in der Konfiguration".to_string(),
                ));
            }
            match defs.iter_mut().find(|d| d.id == entry.id) {
                Some(def) => merge_entry(def, entry),
                None => {
                    let regex = entry.regex.clone().ok_or_else(|| {
                        RedactError::Pattern(format!(
                            "Neues Pattern '{}' benötigt ein 'regex'-Feld (bekannte IDs: {})",
                            entry.id,
                            known_ids_list()
                        ))
                    })?;
                    let mut def = PatternDef {
                        id: entry.id.clone(),
                        regex,
                        description: String::new(),
                        confidence: crate::default_confidence(),
                        enabled: true,
                        validator: None,
                    };
                    merge_entry(&mut def, entry);
                    defs.push(def);
                }
            }
        }
        Ok(defs)
    }
}

/// Überträgt die gesetzten Felder eines Konfigurationseintrags auf eine Definition.
fn merge_entry(def: &mut PatternDef, entry: &PatternEntry) {
    if let Some(regex) = &entry.regex {
        def.regex = regex.clone();
    }
    if let Some(description) = &entry.description {
        def.description = description.clone();
    }
    if let Some(confidence) = entry.confidence {
        def.confidence = confidence;
    }
    if let Some(enabled) = entry.enabled {
        def.enabled = enabled;
    }
    if let Some(validator) = entry.validator {
        def.validator = validator;
    }
}

/// Kommaseparierte Liste aller eingebauten IDs (für Fehlermeldungen).
fn known_ids_list() -> String {
    builtin_pattern_ids().join(", ")
}

/// Ein kompiliertes, aktives Pattern.
struct Compiled {
    /// Index in [`PatternMatcher::defs`] — hält die Ausgabereihenfolge stabil.
    def_index: usize,
    regex: Regex,
}

/// Sucht Regex-Treffer in Text-Runs und liefert Regionen mit exakter Bounding-Box.
pub struct PatternMatcher {
    defs: Vec<PatternDef>,
    compiled: Vec<Compiled>,
}

impl std::fmt::Debug for PatternMatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PatternMatcher")
            .field("defs", &self.defs)
            .field("compiled", &self.compiled.len())
            .finish()
    }
}

impl PatternMatcher {
    /// Wählt eingebaute Patterns anhand ihrer IDs. Leere Liste => alle
    /// eingebauten Patterns (mit ihren Vorgabe-Zuständen).
    ///
    /// Wird eine ID explizit genannt, gilt das Pattern als eingeschaltet — so
    /// lassen sich auch standardmäßig deaktivierte Patterns wie `iban_intl`
    /// gezielt auswählen. Eine unbekannte ID führt zu
    /// [`RedactError::Pattern`] inklusive Liste der bekannten IDs.
    pub fn new(ids: &[String]) -> Result<Self> {
        let builtins = builtin_patterns();
        if ids.is_empty() {
            return Self::with_defs(builtins);
        }
        let mut defs = Vec::with_capacity(ids.len());
        for id in ids {
            let found = builtins.iter().find(|d| &d.id == id).ok_or_else(|| {
                RedactError::Pattern(format!(
                    "Unbekannte Pattern-ID '{id}'. Bekannte IDs: {}",
                    known_ids_list()
                ))
            })?;
            let mut def = found.clone();
            // Explizit angeforderte Patterns sind immer aktiv.
            def.enabled = true;
            defs.push(def);
        }
        Self::with_defs(defs)
    }

    /// Übernimmt eine fertige Liste von Definitionen und kompiliert die aktiven.
    pub fn with_defs(defs: Vec<PatternDef>) -> Result<Self> {
        let mut compiled = Vec::new();
        for (index, def) in defs.iter().enumerate() {
            if defs[..index].iter().any(|d| d.id == def.id) {
                return Err(RedactError::Pattern(format!(
                    "Doppelte Pattern-ID '{}'",
                    def.id
                )));
            }
            if !(0.0..=1.0).contains(&def.confidence) {
                return Err(RedactError::Pattern(format!(
                    "Konfidenz von '{}' muss zwischen 0.0 und 1.0 liegen (ist {})",
                    def.id, def.confidence
                )));
            }
            if !def.enabled {
                // Deaktivierte Patterns werden nie kompiliert.
                continue;
            }
            let regex = Regex::new(&def.regex).map_err(|e| {
                RedactError::Pattern(format!("Regex von '{}' ist ungültig: {e}", def.id))
            })?;
            compiled.push(Compiled {
                def_index: index,
                regex,
            });
        }
        Ok(Self { defs, compiled })
    }

    /// Lädt eine Pattern-Konfiguration; Endung `.yaml`/`.yml` => YAML, sonst JSON.
    pub fn from_config_file(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let is_yaml = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("yaml") || e.eq_ignore_ascii_case("yml"))
            .unwrap_or(false);
        if is_yaml {
            Self::from_yaml(&text)
        } else {
            Self::from_json(&text)
        }
    }

    /// Wie [`PatternMatcher::from_config_file`], aber aus einem YAML-String.
    pub fn from_yaml(s: &str) -> Result<Self> {
        let config: PatternConfig = serde_yaml::from_str(s)
            .map_err(|e| RedactError::Parse(format!("Pattern-Konfiguration (YAML): {e}")))?;
        Self::with_defs(config.resolve()?)
    }

    /// Wie [`PatternMatcher::from_config_file`], aber aus einem JSON-String.
    pub fn from_json(s: &str) -> Result<Self> {
        let config: PatternConfig = serde_json::from_str(s)
            .map_err(|e| RedactError::Parse(format!("Pattern-Konfiguration (JSON): {e}")))?;
        Self::with_defs(config.resolve()?)
    }

    /// Alle Definitionen — auch die deaktivierten — in Ausgabereihenfolge.
    pub fn defs(&self) -> &[PatternDef] {
        &self.defs
    }

    /// Anzahl der tatsächlich kompilierten (aktiven) Patterns.
    pub fn active_count(&self) -> usize {
        self.compiled.len()
    }

    /// Sucht alle Treffer in den Text-Runs und liefert Regionen mit
    /// [`Source::Pattern`] und exakter Bounding-Box.
    ///
    /// Die Ausgabereihenfolge ist deterministisch: Runs in Eingabereihenfolge,
    /// darin die Patterns in der Reihenfolge von [`PatternMatcher::defs`].
    pub fn find_matches(&self, runs: &[TextRun]) -> Result<Vec<Region>> {
        let mut regions = Vec::new();
        for run in runs {
            for compiled in &self.compiled {
                let def = &self.defs[compiled.def_index];
                for found in compiled.regex.find_iter(&run.text) {
                    let m = found.map_err(|e| {
                        RedactError::Pattern(format!(
                            "Fehler beim Suchen mit Pattern '{}': {e}",
                            def.id
                        ))
                    })?;
                    let Some((start, end)) = trim_range(&run.text, m.start(), m.end()) else {
                        continue;
                    };
                    let text = &run.text[start..end];
                    let Some(confidence) = check(def, text) else {
                        continue;
                    };
                    let Some(rect) = run.rect_for_byte_range(start, end) else {
                        continue;
                    };
                    regions.push(Region::new(
                        run.page,
                        rect,
                        Some(text.to_string()),
                        Source::Pattern {
                            pattern_id: def.id.clone(),
                            confidence,
                        },
                    ));
                }
            }
        }
        Ok(regions)
    }
}

/// Schneidet führenden/abschließenden Whitespace aus dem Byte-Bereich heraus,
/// damit das schwarze Rechteck nicht über den Treffer hinausragt.
fn trim_range(text: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    let slice = text.get(start..end)?;
    let trimmed = slice.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lead = slice.len() - slice.trim_start().len();
    Some((start + lead, start + lead + trimmed.len()))
}

/// Wendet den optionalen Validator an.
///
/// `None` bedeutet: Treffer verwerfen. Sonst die (ggf. angehobene) Konfidenz.
fn check(def: &PatternDef, text: &str) -> Option<f32> {
    let ok = match def.validator {
        None => return Some(def.confidence),
        Some(Validator::Iban) => validate_iban(text),
        Some(Validator::Bic) => validate_bic(text),
        Some(Validator::Luhn) => validate_luhn(text),
    };
    if !ok {
        return None;
    }
    Some(def.confidence.max(VALIDATED_CONFIDENCE))
}

impl Analyzer for PatternMatcher {
    fn analyze(&self, runs: &[TextRun]) -> Result<Vec<Region>> {
        self.find_matches(runs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_range_strips_whitespace() {
        let text = "  abc  ";
        assert_eq!(trim_range(text, 0, text.len()), Some((2, 5)));
        assert_eq!(trim_range("   ", 0, 3), None);
    }

    #[test]
    fn config_defaults_extend_builtins() {
        let cfg: PatternConfig = serde_yaml::from_str("patterns: []").unwrap();
        assert!(cfg.extend_builtins);
        assert_eq!(cfg.resolve().unwrap().len(), builtin_patterns().len());
    }

    #[test]
    fn config_without_builtins_uses_only_listed_patterns() {
        let yaml = r#"
extend_builtins: false
patterns:
  - id: only
    regex: 'X+'
"#;
        let m = PatternMatcher::from_yaml(yaml).unwrap();
        assert_eq!(m.defs().len(), 1);
        assert_eq!(m.defs()[0].id, "only");
        assert_eq!(m.active_count(), 1);
    }

    #[test]
    fn validator_can_be_removed_explicitly() {
        let yaml = r#"
patterns:
  - id: bic
    validator: null
"#;
        let m = PatternMatcher::from_yaml(yaml).unwrap();
        let bic = m.defs().iter().find(|d| d.id == "bic").unwrap();
        assert!(bic.validator.is_none());
    }

    #[test]
    fn json_config_is_supported() {
        let json = r#"{"extend_builtins": false,
            "patterns": [{"id":"kd","regex":"KdNr\\.?\\s*\\d{5,}","confidence":0.8}]}"#;
        let m = PatternMatcher::from_json(json).unwrap();
        assert_eq!(m.defs()[0].confidence, 0.8);
    }

    #[test]
    fn invalid_regex_is_reported_with_id() {
        let err = PatternMatcher::with_defs(vec![PatternDef {
            id: "kaputt".into(),
            regex: "(".into(),
            description: String::new(),
            confidence: 0.5,
            enabled: true,
            validator: None,
        }])
        .unwrap_err();
        assert!(err.to_string().contains("kaputt"));
    }

    #[test]
    fn confidence_out_of_range_is_rejected() {
        let err = PatternMatcher::with_defs(vec![PatternDef {
            id: "x".into(),
            regex: "a".into(),
            description: String::new(),
            confidence: 1.5,
            enabled: true,
            validator: None,
        }])
        .unwrap_err();
        assert!(err.to_string().contains("Konfidenz"));
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let def = PatternDef {
            id: "dup".into(),
            regex: "a".into(),
            description: String::new(),
            confidence: 0.5,
            enabled: true,
            validator: None,
        };
        let err = PatternMatcher::with_defs(vec![def.clone(), def]).unwrap_err();
        assert!(err.to_string().contains("Doppelte"));
    }
}
