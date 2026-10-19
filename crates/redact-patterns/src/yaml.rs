//! YAML lesen — für Muster- und Einstellungsdatei an genau einer Stelle.
//!
//! ## Warum `serde-saphyr` und nicht mehr `serde_yaml`
//!
//! `serde_yaml` ist seit 2024 eingestellt (`0.9.34+deprecated`), und darunter
//! lag `unsafe-libyaml`: libyaml, maschinell nach Rust übersetzt, 240-mal
//! `unsafe`, ebenfalls nicht mehr gepflegt (RUSTSEC-2024-0449). Es war die
//! größte `unsafe`-Fläche unter allem, was Dateien dieses Programms liest
//! (SECURITY.md). `serde-saphyr` ist ein Parser in sicherem Rust und die
//! Empfehlung des Advisories.
//!
//! ## Warum die Optionen von den Vorgaben abweichen
//!
//! Gelesen wird so, wie `serde_yaml` 0.9 gelesen hat — YAML 1.2, ohne
//! Zugeständnisse an 1.1:
//!
//! * `strict_booleans`: nur `true` und `false` sind Wahrheitswerte. Die
//!   Vorgabe nähme auch `yes`/`no`/`on`/`off` an; `serde_yaml` hat das
//!   abgelehnt, und eine Datei soll nicht mit dem Wechsel der Bibliothek
//!   plötzlich etwas anderes bedeuten.
//! * `merge_keys: AsOrdinary`: `<<` ist ein gewöhnlicher Schlüssel und fällt
//!   damit als unbekannter auf, wie bisher. Die Vorgabe würde ihn auflösen.
//! * `with_snippet: false`: die Meldung zitiert keinen Ausschnitt der Datei.
//!   Die Einstellungsdatei kann auf eine beliebige fremde Datei zeigen (siehe
//!   `redact_pipeline::settings`); ein Ausschnitt trüge deren Zeilen in die
//!   Meldung.
//!
//! Doppelte Schlüssel sind wie bisher ein Fehler (Vorgabe von `serde-saphyr`),
//! ebenso die Begrenzung für Alias-Bomben (`budget`, `alias_limits`).

use serde::de::DeserializeOwned;

pub use serde_saphyr::Error;

/// Liest `text` als YAML in `T`, mit den oben begründeten Optionen.
pub fn from_str<T: DeserializeOwned>(text: &str) -> Result<T, Error> {
    serde_saphyr::from_str_with_options(
        text,
        serde_saphyr::options! {
            strict_booleans: true,
            merge_keys: serde_saphyr::options::MergeKeyPolicy::AsOrdinary,
            with_snippet: false,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Probe {
        #[serde(default)]
        flag: bool,
        #[serde(default)]
        name: String,
    }

    #[test]
    fn only_true_and_false_are_booleans() {
        assert!(from_str::<Probe>("flag: true\n").unwrap().flag);
        for word in ["yes", "no", "on", "off", "y", "n"] {
            assert!(
                from_str::<Probe>(&format!("flag: {word}\n")).is_err(),
                "{word} gilt als Wahrheitswert"
            );
        }
    }

    #[test]
    fn duplicate_keys_are_an_error() {
        assert_eq!(from_str::<Probe>("name: a\n").unwrap().name, "a");
        assert!(from_str::<Probe>("name: a\nname: b\n").is_err());
    }

    #[test]
    fn a_merge_key_is_an_unknown_key() {
        let error = from_str::<Probe>("base: &b {name: a}\n<<: *b\n")
            .expect_err("`<<` ist kein Schlüssel dieser Struktur");
        assert!(error.to_string().contains("unknown field"), "{error}");
    }

    #[test]
    fn the_message_carries_no_excerpt() {
        let error = from_str::<Probe>("geheim_123: x\n")
            .unwrap_err()
            .to_string();
        assert!(!error.contains('|'), "Ausschnitt in der Meldung: {error}");
        assert!(error.lines().count() <= 1, "{error}");
    }
}
