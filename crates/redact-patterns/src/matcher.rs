//! Kompilieren von Patterns, Laden aus Konfigurationsdateien und das eigentliche
//! Matching auf Text-Runs.

use std::path::Path;

use fancy_regex::Regex;
use redact_core::{Analyzer, RedactError, Region, Result, Source, TextRun};
use serde::{Deserialize, Deserializer, Serialize};

use crate::builtin::{builtin_pattern_ids, builtin_patterns};
use crate::validate::{validate_bic, validate_creditor_id, validate_iban, validate_luhn};
use crate::{PatternDef, Validator, CONTEXT_GROUP, TARGET_GROUP};

/// Konfidenz, auf die ein erfolgreich geprüfter Treffer mindestens angehoben wird.
const VALIDATED_CONFIDENCE: f32 = 0.99;

/// Vorgabe für das Mindestvertrauen eines Treffers.
///
/// Der Wert trennt die beiden Sorten von Treffern, die es gibt: solche, die
/// durch eine Prüfsumme (IBAN, Gläubiger-ID, BIC, Luhn ⇒ 0.99) oder durch ein Schlüsselwort
/// im Text (⇒ 0.8 … 0.9) gestützt sind, und solche, die nur auf der Form einer
/// Ziffernkette beruhen (⇒ 0.25 … 0.35). Genau dazwischen liegt 0.5. Wer die
/// Verdachtsfälle sehen will, senkt die Schwelle bewusst ab; die Vorgabe
/// schwärzt lieber zu wenig als einen ganzen Auszug unleserlich zu machen.
pub const DEFAULT_MIN_CONFIDENCE: f32 = 0.5;

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
    /// Doppeltes `Option` wie bei `validator`: `null` entfernt den Wert.
    #[serde(
        default,
        deserialize_with = "deserialize_present",
        skip_serializing_if = "Option::is_none"
    )]
    pub confidence_without_context: Option<Option<f32>>,
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
    /// Mindestvertrauen für einen Treffer; fehlt der Wert, gilt
    /// [`DEFAULT_MIN_CONFIDENCE`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_confidence: Option<f32>,
    #[serde(default)]
    pub patterns: Vec<PatternEntry>,
}

impl Default for PatternConfig {
    fn default() -> Self {
        Self {
            extend_builtins: true,
            min_confidence: None,
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
                        confidence_without_context: None,
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
    if let Some(weak) = entry.confidence_without_context {
        def.confidence_without_context = weak;
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
    min_confidence: f32,
}

impl std::fmt::Debug for PatternMatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PatternMatcher")
            .field("defs", &self.defs)
            .field("compiled", &self.compiled.len())
            .field("min_confidence", &self.min_confidence)
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
            if let Some(weak) = def.confidence_without_context {
                if !(0.0..=1.0).contains(&weak) {
                    return Err(RedactError::Pattern(format!(
                        "confidence_without_context von '{}' muss zwischen 0.0 und 1.0 liegen (ist {weak})",
                        def.id
                    )));
                }
            }
            if !def.enabled {
                // Deaktivierte Patterns werden nie kompiliert.
                continue;
            }
            let regex = Regex::new(&def.regex).map_err(|e| {
                RedactError::Pattern(format!("Regex von '{}' ist ungültig: {e}", def.id))
            })?;
            // Ein zweiter Konfidenzwert ohne Kontext-Gruppe wäre wirkungslos —
            // und damit ein stiller Konfigurationsfehler.
            if def.confidence_without_context.is_some()
                && !regex.capture_names().any(|n| n == Some(CONTEXT_GROUP))
            {
                return Err(RedactError::Pattern(format!(
                    "'{}' setzt confidence_without_context, hat aber keine Regex-Gruppe \
                     (?<{CONTEXT_GROUP}>…). Entweder der Regex bekommt die Gruppe, oder \
                     der Wert wird mit 'confidence_without_context: null' entfernt.",
                    def.id
                )));
            }
            compiled.push(Compiled {
                def_index: index,
                regex,
            });
        }
        Ok(Self {
            defs,
            compiled,
            min_confidence: DEFAULT_MIN_CONFIDENCE,
        })
    }

    /// Setzt das Mindestvertrauen; Treffer darunter werden verworfen.
    ///
    /// 0.0 liefert alles, was die Regexe hergeben — auch die reinen
    /// Ziffernketten ohne Schlüsselwort.
    pub fn with_min_confidence(mut self, min_confidence: f32) -> Result<Self> {
        if !(0.0..=1.0).contains(&min_confidence) {
            return Err(RedactError::Pattern(format!(
                "Mindestvertrauen muss zwischen 0.0 und 1.0 liegen (ist {min_confidence})"
            )));
        }
        self.min_confidence = min_confidence;
        Ok(self)
    }

    /// Aktuelles Mindestvertrauen.
    pub fn min_confidence(&self) -> f32 {
        self.min_confidence
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
        Self::from_config(&config)
    }

    /// Wie [`PatternMatcher::from_config_file`], aber aus einem JSON-String.
    pub fn from_json(s: &str) -> Result<Self> {
        let config: PatternConfig = serde_json::from_str(s)
            .map_err(|e| RedactError::Parse(format!("Pattern-Konfiguration (JSON): {e}")))?;
        Self::from_config(&config)
    }

    /// Baut einen Matcher aus einer bereits geparsten Konfiguration.
    pub fn from_config(config: &PatternConfig) -> Result<Self> {
        let matcher = Self::with_defs(config.resolve()?)?;
        match config.min_confidence {
            Some(min) => matcher.with_min_confidence(min),
            None => Ok(matcher),
        }
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
    /// Geschwärzt wird die Gruppe `target`, falls der Regex sie hat, sonst der
    /// gesamte Treffer — so bleibt das Schlüsselwort („BLZ", „Kto.") lesbar,
    /// das den Treffer überhaupt erst erklärt. Treffer unterhalb von
    /// [`PatternMatcher::min_confidence`] werden verworfen.
    ///
    /// Die Ausgabereihenfolge ist deterministisch: Runs in Eingabereihenfolge,
    /// darin die Patterns in der Reihenfolge von [`PatternMatcher::defs`].
    pub fn find_matches(&self, runs: &[TextRun]) -> Result<Vec<Region>> {
        let mut regions = Vec::new();
        for run in runs {
            for compiled in &self.compiled {
                let def = &self.defs[compiled.def_index];
                for found in compiled.regex.captures_iter(&run.text) {
                    let caps = found.map_err(|e| {
                        RedactError::Pattern(format!(
                            "Fehler beim Suchen mit Pattern '{}': {e}",
                            def.id
                        ))
                    })?;
                    // Gruppe 0 existiert bei jedem Treffer.
                    let whole = caps.get(0).expect("Gesamttreffer existiert immer");
                    let m = caps.name(TARGET_GROUP).unwrap_or(whole);
                    let Some((start, end)) = trim_range(&run.text, m.start(), m.end()) else {
                        continue;
                    };
                    let text = &run.text[start..end];
                    let has_context = caps.name(CONTEXT_GROUP).is_some();
                    let Some(confidence) = check(def, text, has_context) else {
                        continue;
                    };
                    if confidence < self.min_confidence {
                        continue;
                    }
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

/// Bestimmt die Konfidenz eines Treffers und wendet den Validator an.
///
/// `None` bedeutet: Treffer verwerfen. Sonst die Konfidenz — abgesenkt, wenn
/// das Pattern einen Kontext erwartet, der nicht gegriffen hat, und angehoben,
/// wenn eine Prüfsumme bestanden wurde.
fn check(def: &PatternDef, text: &str, has_context: bool) -> Option<f32> {
    let confidence = match def.confidence_without_context {
        Some(weak) if !has_context => weak,
        _ => def.confidence,
    };
    let ok = match def.validator {
        None => return Some(confidence),
        Some(Validator::Iban) => validate_iban(text),
        Some(Validator::CreditorId) => validate_creditor_id(text),
        Some(Validator::Bic) => validate_bic(text),
        Some(Validator::Luhn) => validate_luhn(text),
    };
    if !ok {
        return None;
    }
    Some(confidence.max(VALIDATED_CONFIDENCE))
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

    /// Minimale Definition für die Fehlerfälle unten.
    fn def(id: &str, regex: &str, confidence: f32) -> PatternDef {
        PatternDef {
            id: id.into(),
            regex: regex.into(),
            description: String::new(),
            confidence,
            confidence_without_context: None,
            enabled: true,
            validator: None,
        }
    }

    #[test]
    fn invalid_regex_is_reported_with_id() {
        let err = PatternMatcher::with_defs(vec![def("kaputt", "(", 0.5)]).unwrap_err();
        assert!(err.to_string().contains("kaputt"));
    }

    #[test]
    fn confidence_out_of_range_is_rejected() {
        let err = PatternMatcher::with_defs(vec![def("x", "a", 1.5)]).unwrap_err();
        assert!(err.to_string().contains("Konfidenz"));
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let d = def("dup", "a", 0.5);
        let err = PatternMatcher::with_defs(vec![d.clone(), d]).unwrap_err();
        assert!(err.to_string().contains("Doppelte"));
    }

    #[test]
    fn weak_confidence_without_a_context_group_is_rejected() {
        let mut d = def("x", "[0-9]+", 0.9);
        d.confidence_without_context = Some(0.2);
        let err = PatternMatcher::with_defs(vec![d]).unwrap_err();
        assert!(err.to_string().contains("context"), "{err}");
    }

    #[test]
    fn weak_confidence_out_of_range_is_rejected() {
        let mut d = def("x", "(?<context>A)?(?<target>[0-9]+)", 0.9);
        d.confidence_without_context = Some(-0.1);
        let err = PatternMatcher::with_defs(vec![d]).unwrap_err();
        assert!(
            err.to_string().contains("confidence_without_context"),
            "{err}"
        );
    }

    #[test]
    fn min_confidence_defaults_and_is_validated() {
        let m = PatternMatcher::new(&[]).unwrap();
        assert!((m.min_confidence() - DEFAULT_MIN_CONFIDENCE).abs() < 1e-6);
        let m = PatternMatcher::new(&[]).unwrap().with_min_confidence(0.0);
        assert!(m.is_ok());
        let err = PatternMatcher::new(&[])
            .unwrap()
            .with_min_confidence(1.5)
            .unwrap_err();
        assert!(err.to_string().contains("Mindestvertrauen"), "{err}");
    }

    #[test]
    fn config_can_set_the_min_confidence() {
        let m = PatternMatcher::from_yaml("min_confidence: 0.25\npatterns: []").unwrap();
        assert!((m.min_confidence() - 0.25).abs() < 1e-6);
        let m = PatternMatcher::from_json(r#"{"patterns": []}"#).unwrap();
        assert!((m.min_confidence() - DEFAULT_MIN_CONFIDENCE).abs() < 1e-6);
    }

    #[test]
    fn config_can_drop_the_weak_confidence_of_a_builtin() {
        let yaml = r#"
patterns:
  - id: konto_nr
    confidence_without_context: null
"#;
        let m = PatternMatcher::from_yaml(yaml).unwrap();
        let konto = m.defs().iter().find(|d| d.id == "konto_nr").unwrap();
        assert!(konto.confidence_without_context.is_none());
    }
}
