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
        confidence_without_context: None,
        enabled,
        validator,
    }
}

/// Wie [`def`], zusätzlich mit der Konfidenz für einen Treffer *ohne* Kontext.
fn def_ctx(
    id: &str,
    regex: &str,
    description: &str,
    confidence: f32,
    confidence_without_context: f32,
    enabled: bool,
) -> PatternDef {
    PatternDef {
        confidence_without_context: Some(confidence_without_context),
        ..def(id, regex, description, confidence, enabled, None)
    }
}

/// Schlüsselwort-Präfix für die kontextgestützten Ziffern-Patterns.
///
/// `context` ist eine *optionale* Gruppe: greift sie nicht, bleibt der Treffer
/// erhalten, bekommt aber nur `confidence_without_context` — und fällt damit
/// unter das voreingestellte Mindestvertrauen.
macro_rules! mit_kontext {
    ($keywords:expr, $target:expr) => {
        concat!(
            "(?:(?<context>",
            $keywords,
            // Ein optionaler Doppelpunkt/Punkt und Zwischenraum zwischen
            // Schlüsselwort und Ziffern — „BLZ: 370 …", „Kto.  123 …".
            r")[:.]?[ \t]*)?(?<target>",
            $target,
            ")"
        )
    };
}

/// Ziffernkette, die weder in einer längeren Kette noch in einer IBAN steckt.
///
/// `\b` genügt bei reinen Ziffernfolgen nicht, weil eine Kontonummer sonst
/// innerhalb einer längeren Ziffernkette oder innerhalb einer IBAN anschlagen
/// würde. Deshalb wird durchgängig mit Look-around gearbeitet.
macro_rules! ziffern {
    ($n:expr) => {
        concat!(r"(?<![0-9A-Za-z])[0-9]{", $n, r"}(?![0-9A-Za-z])")
    };
}

/// Alle eingebauten Patterns in fester (deterministischer) Reihenfolge.
///
/// **Was standardmäßig an ist, ist eine Produktentscheidung.** Schutzgut eines
/// Kontoauszugs sind die Merkmale, die eine Person oder eine Bankverbindung
/// identifizieren: IBAN, BIC, Kontonummer, Bankleitzahl, Steuer-ID, E-Mail,
/// Telefon, Kreditkarte. Datum und Betrag einer Buchung sind es in aller Regel
/// *nicht* — sie sind der eigentliche Inhalt, den der Empfänger des Dokuments
/// lesen soll. Ein Auszug, in dem jedes Datum und jeder Betrag geschwärzt ist,
/// ist unbrauchbar, und die wenigen echten Treffer gehen in der Masse unter.
/// `date_de` und `amount_eur` sind deshalb aus; wer sie braucht (etwa für ein
/// Gehaltsdatum), schaltet sie mit `--patterns date_de` gezielt ein.
///
/// Umgekehrt ist `steuer_id` jetzt an: die Steuer-ID *ist* Schutzgut, und seit
/// sie an ihr Schlüsselwort gebunden ist, ist sie kein Rauschfänger mehr
/// (vorher: jede elfstellige Ziffernkette).
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
        def_ctx(
            "konto_nr",
            // „Kto.", „Konto", „Kontonummer", „Konto-Nr." und Zusammen-
            // setzungen wie „Vertragskonto" oder „Girokonto".
            //
            // Kontonummern stehen auf Auszügen oft in Gruppen („Kto. 4711
            // 0815"). Ein kurzer erster Block zählt deshalb nur, wenn ihm
            // mindestens ein weiterer Block folgt — sonst wäre jede Jahreszahl
            // eine Kontonummer.
            mit_kontext!(
                r"(?i:[a-zäöüß]*konto(?:nummer|-?nr\.?)?|kto\.?)",
                r"(?<![0-9A-Za-z])(?:[0-9]{4,10}(?:[ ][0-9]{2,6})+|[0-9]{6,10})(?![0-9A-Za-z])"
            ),
            "Kontonummer (6–10 Ziffern nach einem Schlüsselwort wie „Kto.“)",
            0.85,
            // Ohne Schlüsselwort ist eine 6–10-stellige Ziffernkette bloß eine
            // Ziffernkette: Rechnungs-, Personal-, Policen-, Telefonnummer.
            0.3,
            true,
        ),
        def_ctx(
            "blz",
            mit_kontext!(r"(?i:blz|bankleitzahl)", ziffern!("8")),
            "Bankleitzahl (8 Ziffern nach „BLZ“ oder „Bankleitzahl“)",
            0.8,
            // Bewusst niedriger als der entsprechende Wert von `konto_nr`:
            // beide Patterns treffen dieselbe achtstellige Ziffernkette, und
            // ohne Schlüsselwort ist eine Kontonummer die häufigere Deutung.
            // Die unterschiedliche Konfidenz macht die Überlappung auflösbar.
            0.25,
            true,
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
            "Geldbetrag in Euro (standardmäßig aus: Betrag ist Inhalt, nicht Schutzgut)",
            0.7,
            false,
            None,
        ),
        def(
            "date_de",
            r"\b[0-9]{2}\.[0-9]{2}\.[0-9]{4}\b",
            "Datum TT.MM.JJJJ (standardmäßig aus: Datum ist Inhalt, nicht Schutzgut)",
            0.6,
            false,
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
        def_ctx(
            "steuer_id",
            mit_kontext!(
                r"(?i:steuer-?(?:id|nr\.?|nummer|identifikationsnummer)|steuerliche[ ]+identifikationsnummer|id-?nr\.?)",
                ziffern!("11")
            ),
            "Steuerliche Identifikationsnummer (11 Ziffern nach „Steuer-ID“)",
            0.9,
            // Elf nackte Ziffern sind genauso gut eine Referenznummer.
            0.25,
            true,
        ),
        def(
            "email",
            r"(?<![A-Za-z0-9._%+-])[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}(?![A-Za-z0-9-])",
            "E-Mail-Adresse",
            0.9,
            true,
            None,
        ),
        def_ctx(
            "phone_de",
            // Als Kontext zählt entweder ein Schlüsselwort (wird mitgelesen)
            // oder die Ländervorwahl (nur vorausgeschaut, sie gehört zur
            // Nummer und wird deshalb mitgeschwärzt). Der Längen-Guard
            // (mindestens 7, höchstens 16 Ziffern) hält kurze Zahlen fern.
            mit_kontext!(
                r"(?i:telefonnummer|telefon|tel\.?|mobil|handy|fax|fon)|(?=\+49|0049)",
                r"(?<![0-9A-Za-z+])(?:\+49[ /-]?|0049[ /-]?|0)[1-9][0-9 /-]{5,14}[0-9](?![0-9A-Za-z])"
            ),
            "Deutsche Telefonnummer",
            0.85,
            // Ohne Schlüsselwort und ohne Ländervorwahl trifft das Muster auch
            // die Ziffernblöcke einer gruppierten IBAN („… 0532 0130 00“).
            0.35,
            true,
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
            if let Some(weak) = d.confidence_without_context {
                assert!(
                    (0.0..=1.0).contains(&weak),
                    "Kontextlose Konfidenz von {} liegt außerhalb 0..=1",
                    d.id
                );
                assert!(
                    weak < d.confidence,
                    "{}: ohne Kontext darf nicht mehr Vertrauen genießen als mit",
                    d.id
                );
            }
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
        // `date_de` und `amount_eur` sind Inhalt, kein Schutzgut (siehe
        // Modulkommentar von `builtin_patterns`); `iban_intl` ist zu unspezifisch.
        assert_eq!(disabled, vec!["iban_intl", "amount_eur", "date_de"]);
    }

    /// Jedes standardmäßig aktive Pattern muss im Regelfall auch durch das
    /// voreingestellte Mindestvertrauen kommen — sonst wäre es nur Zierde.
    #[test]
    fn enabled_patterns_can_pass_the_default_threshold() {
        for d in builtin_patterns().iter().filter(|d| d.enabled) {
            assert!(
                d.confidence >= crate::DEFAULT_MIN_CONFIDENCE,
                "{} ist aktiv, kommt aber nie über das Mindestvertrauen",
                d.id
            );
        }
    }

    /// Kontextlose Treffer sollen standardmäßig *nicht* durchkommen — genau
    /// dafür gibt es die zweite Konfidenz.
    #[test]
    fn context_free_hits_stay_below_the_default_threshold() {
        for d in builtin_patterns() {
            if let Some(weak) = d.confidence_without_context {
                assert!(
                    weak < crate::DEFAULT_MIN_CONFIDENCE,
                    "{} würde auch ohne Schlüsselwort geschwärzt",
                    d.id
                );
            }
        }
    }

    /// Die Überlappung von `konto_nr` und `blz` auf achtstelligen Ziffern muss
    /// auflösbar bleiben: gleiche Konfidenz hieße Münzwurf.
    #[test]
    fn konto_nr_and_blz_rank_differently_without_context() {
        let defs = builtin_patterns();
        let weak = |id: &str| {
            defs.iter()
                .find(|d| d.id == id)
                .unwrap()
                .confidence_without_context
                .unwrap()
        };
        assert!(weak("konto_nr") > weak("blz"));
    }
}
