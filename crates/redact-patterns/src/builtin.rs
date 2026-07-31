//! Die eingebauten Pattern-Definitionen.
//!
//! Die IDs sind Teil der öffentlichen Schnittstelle (die CLI referenziert sie),
//! sie dürfen also nicht ohne Not umbenannt werden.

use crate::{PatternDef, Validator};

/// Hilfskonstruktor für die Tabelle unten.
fn def(
    id: &str,
    regex: &str,
    description: &str,
    confidence: f32,
    enabled: bool,
    validator: Option<Validator>,
) -> PatternDef {
    PatternDef {
        id: id.to_string(),
        regex: regex.to_string(),
        description: description.to_string(),
        confidence,
        enabled,
        validator,
    }
}

/// Alle eingebauten Patterns in fester (deterministischer) Reihenfolge.
///
/// Anmerkungen zu den Guards: `\b` reicht bei reinen Ziffernfolgen nicht aus,
/// weil eine Kontonummer sonst innerhalb einer längeren Ziffernkette oder
/// innerhalb einer IBAN anschlagen würde. Deshalb wird durchgängig mit
/// Look-around `(?<![0-9A-Za-z])` / `(?![0-9A-Za-z])` gearbeitet.
pub fn builtin_patterns() -> Vec<PatternDef> {
    vec![
        def(
            "iban_de",
            r"(?<![0-9A-Za-z])DE[0-9]{2}(?: ?[0-9]){18}(?![0-9A-Za-z])",
            "Deutsche IBAN (DE + 2 Prüfziffern + 18 Ziffern, optional gruppiert)",
            0.95,
            true,
            Some(Validator::Iban),
        ),
        def(
            "iban_intl",
            r"(?<![0-9A-Za-z])[A-Z]{2}[0-9]{2}(?: ?[0-9A-Z]){11,30}(?![0-9A-Za-z])",
            "Internationale IBAN (standardmäßig aus, weil zu unspezifisch)",
            0.9,
            false,
            Some(Validator::Iban),
        ),
        def(
            "konto_nr",
            r"(?<![0-9A-Za-z])[0-9]{6,10}(?![0-9A-Za-z])",
            "Kontonummer (Heuristik, 6–10 Ziffern)",
            0.4,
            true,
            None,
        ),
        def(
            "blz",
            r"(?<![0-9A-Za-z])[0-9]{8}(?![0-9A-Za-z])",
            "Bankleitzahl (8 Ziffern)",
            0.5,
            true,
            None,
        ),
        def(
            "bic",
            r"\b[A-Z]{6}[A-Z0-9]{2}(?:[A-Z0-9]{3})?\b",
            "BIC/SWIFT-Code",
            0.8,
            true,
            Some(Validator::Bic),
        ),
        def(
            "amount_eur",
            r"(?<![0-9.,])[0-9]{1,3}(?:\.[0-9]{3})*,[0-9]{2} ?(?:€|EUR)(?![A-Za-z])",
            "Geldbetrag in Euro (deutsche Schreibweise)",
            0.7,
            true,
            None,
        ),
        def(
            "date_de",
            r"\b[0-9]{2}\.[0-9]{2}\.[0-9]{4}\b",
            "Datum im Format TT.MM.JJJJ",
            0.6,
            true,
            None,
        ),
        def(
            "credit_card",
            r"(?<![0-9A-Za-z])[0-9](?:[ -]?[0-9]){12,18}(?![0-9A-Za-z])",
            "Kreditkartennummer (13–19 Ziffern, Luhn-geprüft)",
            0.9,
            true,
            Some(Validator::Luhn),
        ),
        def(
            "steuer_id",
            r"(?<![0-9A-Za-z])[0-9]{11}(?![0-9A-Za-z])",
            "Steuerliche Identifikationsnummer (11 Ziffern, standardmäßig aus)",
            0.5,
            false,
            None,
        ),
        def(
            "email",
            r"(?<![A-Za-z0-9._%+-])[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}(?![A-Za-z0-9-])",
            "E-Mail-Adresse",
            0.9,
            true,
            None,
        ),
        def(
            "phone_de",
            // Entweder Ländervorwahl +49/0049 oder eine mit 0 beginnende Vorwahl.
            // Der Längen-Guard (mindestens 7, höchstens 16 Ziffern) verhindert,
            // dass kurze Zahlen oder Beträge als Telefonnummer gelten.
            r"(?<![0-9A-Za-z+])(?:\+49[ /-]?|0049[ /-]?|0)[1-9][0-9 /-]{5,14}[0-9](?![0-9A-Za-z])",
            "Deutsche Telefonnummer",
            0.6,
            true,
            None,
        ),
    ]
}

/// IDs aller eingebauten Patterns — in derselben Reihenfolge wie
/// [`builtin_patterns`].
pub fn builtin_pattern_ids() -> Vec<&'static str> {
    vec![
        "iban_de",
        "iban_intl",
        "konto_nr",
        "blz",
        "bic",
        "amount_eur",
        "date_de",
        "credit_card",
        "steuer_id",
        "email",
        "phone_de",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_match_definitions() {
        let defs = builtin_patterns();
        let ids = builtin_pattern_ids();
        assert_eq!(defs.len(), ids.len());
        for (d, id) in defs.iter().zip(ids.iter()) {
            assert_eq!(&d.id, id);
        }
    }

    #[test]
    fn all_regexes_compile_and_confidences_are_sane() {
        for d in builtin_patterns() {
            fancy_regex::Regex::new(&d.regex)
                .unwrap_or_else(|e| panic!("Pattern {} ist kein gültiger Regex: {e}", d.id));
            assert!(
                (0.0..=1.0).contains(&d.confidence),
                "Konfidenz von {} liegt außerhalb 0..=1",
                d.id
            );
        }
    }

    #[test]
    fn noisy_patterns_are_disabled_by_default() {
        let defs = builtin_patterns();
        let disabled: Vec<&str> = defs
            .iter()
            .filter(|d| !d.enabled)
            .map(|d| d.id.as_str())
            .collect();
        assert_eq!(disabled, vec!["iban_intl", "steuer_id"]);
    }
}
