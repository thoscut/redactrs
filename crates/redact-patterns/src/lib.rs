//! # redact-patterns
//!
//! Regex-basierte Erkennung sensibler Daten (IBAN, BIC, Kontonummer, Beträge,
//! Datumsangaben, Kreditkarten, E-Mail, Telefonnummern …) auf den Text-Runs,
//! die `redact-pdf` aus einem PDF extrahiert.
//!
//! Das Crate liefert eine Liste eingebauter Patterns ([`builtin_patterns`]),
//! die per Konfigurationsdatei (YAML oder JSON) ergänzt, überschrieben oder
//! abgeschaltet werden kann. Gefundene Stellen werden als
//! [`redact_core::Region`] mit [`redact_core::Source::Pattern`] und exakter
//! Bounding-Box zurückgegeben — die Box entsteht aus den Glyph-Rechtecken des
//! Treffers, nicht aus dem gesamten Text-Run.
//!
//! Zusätzlich zu den regulären Ausdrücken gibt es Prüfsummen-Validatoren
//! ([`Validator`]): IBAN (mod 97), BIC (Struktur + Länderkennung) und Luhn.
//! Ein Treffer, der seine Prüfung nicht besteht, wird verworfen; ein bestandener
//! Treffer bekommt eine Konfidenz von mindestens 0.99.
//!
//! ```
//! use redact_patterns::PatternMatcher;
//!
//! let matcher = PatternMatcher::new(&["iban_de".to_string()])?;
//! let regions = matcher.find_matches(&[])?;
//! assert!(regions.is_empty());
//! # Ok::<(), redact_core::RedactError>(())
//! ```

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

mod builtin;
mod matcher;
mod validate;

pub use builtin::{builtin_pattern_ids, builtin_patterns};
pub use matcher::{PatternConfig, PatternEntry, PatternMatcher};
pub use validate::{validate_bic, validate_iban, validate_luhn};

/// Eine Pattern-Definition (eingebaut oder aus Konfiguration geladen).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternDef {
    /// Eindeutige ID, wird von der CLI und im Audit-Log referenziert.
    pub id: String,
    /// Regulärer Ausdruck in der Syntax von `fancy-regex` (inkl. Look-around).
    pub regex: String,
    /// Kurzbeschreibung für UI und Log.
    #[serde(default)]
    pub description: String,
    /// Konfidenz des Treffers (0.0 … 1.0).
    #[serde(default = "default_confidence")]
    pub confidence: f32,
    /// Deaktivierte Patterns werden nie kompiliert.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Optionale Zusatzprüfung, die einen Treffer verwerfen oder die Konfidenz
    /// anheben kann.
    #[serde(default)]
    pub validator: Option<Validator>,
}

/// Verfügbare Zusatzprüfungen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Validator {
    /// IBAN-Prüfsumme nach ISO 7064 (mod 97 == 1).
    Iban,
    /// BIC-Struktur inklusive Länderkennung.
    Bic,
    /// Luhn-Prüfsumme (Kreditkarten).
    Luhn,
}

/// Vorgabe-Konfidenz, wenn die Konfiguration keine angibt.
pub fn default_confidence() -> f32 {
    0.9
}

/// Vorgabewert für `enabled` und `extend_builtins`.
pub fn default_true() -> bool {
    true
}
