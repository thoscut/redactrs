//! # redact-patterns
//!
//! Regex-basierte Erkennung sensibler Daten (IBAN, BIC, Kontonummer,
//! Bankleitzahl, Steuer-ID, Kreditkarten, E-Mail, Telefonnummern …) auf den
//! Text-Runs, die `redact-pdf` aus einem PDF extrahiert.
//!
//! Das Crate liefert eine Liste eingebauter Patterns ([`builtin_patterns`]),
//! die per Konfigurationsdatei (YAML oder JSON) ergänzt, überschrieben oder
//! abgeschaltet werden kann. Gefundene Stellen werden als
//! [`redact_core::Region`] mit [`redact_core::Source::Pattern`] und exakter
//! Bounding-Box zurückgegeben — die Box entsteht aus den Glyph-Rechtecken des
//! Treffers, nicht aus dem gesamten Text-Run.
//!
//! Zusätzlich zu den regulären Ausdrücken gibt es Prüfsummen-Validatoren
//! ([`Validator`]): IBAN (mod 97), SEPA-Gläubiger-ID (mod 97 ohne die
//! Geschäftsbereichskennung), BIC (Struktur + Länderkennung) und Luhn.
//! Ein Treffer, der seine Prüfung nicht besteht, wird verworfen; ein bestandener
//! Treffer bekommt eine Konfidenz von mindestens 0.99.
//!
//! ## Kontext statt reiner Ziffernlänge
//!
//! Reine Ziffernmuster (Kontonummer, Bankleitzahl, Steuer-ID) sind an einer
//! Ziffernkette allein nicht zu unterscheiden — jedes achtstellige Token ist
//! zugleich eine mögliche BLZ und eine mögliche Kontonummer. Solche Patterns
//! benennen deshalb zwei Gruppen:
//!
//! * `target` — der Teil, der tatsächlich geschwärzt wird. Fehlt die Gruppe,
//!   gilt der gesamte Treffer.
//! * `context` — ein *optionales* Schlüsselwort davor („BLZ", „Kto.", …).
//!   Hat es gegriffen, gilt [`PatternDef::confidence`]; sonst der niedrigere
//!   Wert aus [`PatternDef::confidence_without_context`].
//!
//! ## Mindestvertrauen
//!
//! Treffer unterhalb von [`PatternMatcher::min_confidence`] werden verworfen.
//! Vorgabe ist [`DEFAULT_MIN_CONFIDENCE`]; damit überleben nur Treffer, die
//! entweder durch eine Prüfsumme oder durch ein Schlüsselwort gestützt sind.
//!
//! ## Was eine einzelne Textzeile kosten darf
//!
//! Ein Regex mit Look-around wird zurückverfolgend ausgewertet; wie teuer eine
//! Suche wird, hängt damit nicht nur vom Muster ab, sondern auch vom Text. Zwei
//! Dinge halten das im Zaum, und sie sind **nicht** dasselbe:
//!
//! * **Die Muster selbst sind linear.** Kein eingebautes Muster hat einen
//!   unbegrenzten Quantor an einer Stelle, an der die Suche von jeder Position
//!   aus hineinlaufen könnte. Die Look-behinds (`(?<![0-9A-Za-z])`, …) sind
//!   genau dafür da: sie schneiden die Startpositionen weg, die sonst dieselbe
//!   Ziffern- oder Buchstabenkette ein zweites Mal durchliefen. Wo das nicht
//!   ging, steht eine Obergrenze am Quantor (`{0,24}` statt `*`).
//!   `tests/backtracking.rs` misst das nach.
//! * **[`BACKTRACK_LIMIT`] ist kein Ersatz dafür**, sondern ein Fangnetz für
//!   Muster aus einer [`PatternConfig`]: die kommen von außen und können
//!   beliebig teuer sein.
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

pub use builtin::{builtin_pattern_ids, builtin_patterns, MAX_KEYWORD_PREFIX};
pub use matcher::{
    PatternConfig, PatternEntry, PatternMatcher, BACKTRACK_LIMIT, DEFAULT_MIN_CONFIDENCE,
};
pub use validate::{validate_bic, validate_creditor_id, validate_iban, validate_luhn};

/// Name der Regex-Gruppe, die den tatsächlich zu schwärzenden Teil umfasst.
pub const TARGET_GROUP: &str = "target";

/// Name der optionalen Regex-Gruppe mit dem Kontext-Schlüsselwort.
pub const CONTEXT_GROUP: &str = "context";

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
    ///
    /// Hat der Regex eine Gruppe `context`, gilt dieser Wert nur, wenn die
    /// Gruppe gegriffen hat.
    #[serde(default = "default_confidence")]
    pub confidence: f32,
    /// Konfidenz, wenn die Gruppe `context` **nicht** gegriffen hat.
    ///
    /// Nur zulässig, wenn der Regex eine Gruppe `context` besitzt. Damit
    /// bekommt derselbe Treffer je nach Kontext ein anderes Vertrauen: „BLZ
    /// 37040044" ist eine Bankleitzahl, ein nacktes „37040044" irgendwo im
    /// Text ist bestenfalls ein Verdacht.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_without_context: Option<f32>,
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
    /// SEPA-Gläubiger-ID: dieselbe mod-97-Rechnung wie bei der IBAN, aber ohne
    /// die drei Zeichen Geschäftsbereichskennung, plus Prüfung des Ländercodes.
    CreditorId,
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
