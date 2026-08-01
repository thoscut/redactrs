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

/// Wie viele Buchstaben ein Kontext-Schlüsselwort **vor** seinem Kern haben
/// darf — „Vertrags“ in „Vertragskonto“.
///
/// ## Warum die Zahl da steht, wo vorher `*` stand
///
/// `(?i:[a-zäöüß]*konto…)` hatte keinen Look-behind davor, das `*` konnte also
/// an *jeder* Position anfangen und lief von dort bis zum Ende der
/// Buchstabenkette, bevor es rückwärts nach „konto“ suchte. Über einer Zeile
/// aus n Buchstaben sind das n Startpositionen × n Buchstaben — quadratisch.
/// Gemessen: **eine einzige Zeile aus 1 600 Buchstaben** riss das
/// Rückschritt-Budget von `fancy-regex`, und der ganze Lauf endete mit
/// „Max limit for backtracking count exceeded“ und Rückgabewert 1. Die
/// dokumentierten Grenzen (16 MB geparster Inhalt) waren dabei nie im Spiel.
///
/// Mit einer Obergrenze probiert die Suche je Position höchstens
/// `MAX_KEYWORD_PREFIX + 1` Längen durch: der Aufwand wird linear, und die
/// Schwelle verschwindet ganz statt sich nur zu verschieben.
///
/// ## Warum 24
///
/// Es ist die Länge des längsten Bestimmungsworts, das auf einem deutschen
/// Auszug realistisch vor „konto“ steht, plus Luft:
/// „Wertpapierverrechnungs“ sind 22 Zeichen, „Gemeinschafts“ 13,
/// „Fremdwährungs“ 13, „Vertrags“ 8. `tests/backtracking.rs` hält genau diese
/// Zusammensetzungen fest, damit die Zahl nicht unter die Sprache rutscht.
///
/// Was darüber liegt, verliert nur seinen *Kontextbonus*: der Treffer bleibt,
/// bekommt aber `confidence_without_context` und fällt damit unter das
/// voreingestellte Mindestvertrauen. Ein 25 Zeichen langes Wort vor „konto“
/// ist kein Kontoauszugsdeutsch mehr, sondern genau die Buchstabenkette, um
/// die es hier geht.
///
/// ## Warum öffentlich
///
/// Die Zahl ist eine Eigenschaft der *gelieferten* Muster: wer sich in einer
/// eigenen Pattern-Konfiguration an `konto_nr` anlehnt, soll sie nachschlagen
/// können, statt sie aus dem Regex abzulesen.
pub const MAX_KEYWORD_PREFIX: usize = 24;

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
///
/// Ebenso `glaeubiger_id`: die SEPA-Gläubiger-ID identifiziert den
/// Zahlungsempfänger einer Lastschrift eindeutig; sie steht damit auf einer
/// Stufe mit der Empfänger-IBAN, die der Standardlauf schon immer schwärzt.
/// Die Begründung im Einzelnen steht an der Definition selbst.
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
        // **Warum an?** Die Gläubiger-ID identifiziert den Zahlungsempfänger
        // einer Lastschrift eindeutig und dauerhaft — bei Einzelunternehmern
        // ist sie damit unmittelbar personenbezogen. Sie ist damit dasselbe
        // wie die Empfänger-IBAN eine Zeile weiter, und die schwärzt der
        // Standardlauf längst. Der Einwand, die Lastschriftzeile solle lesbar
        // bleiben, trifft das Muster nicht: geschwärzt wird nur die
        // maschinenlesbare Kennung, während „Lastschrift Stadtwerke
        // Musterstadt" und der Betrag stehen bleiben (Namen erkennt ohnehin
        // kein Muster). Anders als bei `date_de`/`amount_eur` kostet die
        // Schwärzung also keine Lesbarkeit.
        //
        // **Warum kein Kontext?** Die mod-97-Prüfung trägt das Muster allein;
        // ein Schlüsselwort („Glaeubiger-ID") wäre nur eine zweite Hürde vor
        // einer schon gesicherten Aussage. Ein `confidence_without_context`
        // wäre hier sogar wirkungslos, weil eine bestandene Prüfsumme die
        // Konfidenz ohnehin auf 0.99 anhebt.
        //
        // **Warum 0.95?** Derselbe Wert wie bei `iban_de`: dieselbe
        // Prüfsummenstärke, gleiche Aussagekraft. Der Wert liegt deutlich über
        // `DEFAULT_MIN_CONFIDENCE` (0.5) und ist damit das, was das Muster
        // wert wäre, wenn jemand den Validator per Konfiguration entfernt —
        // mit Validator kommen wie bei allen prüfsummengestützten Mustern
        // 0.99 heraus.
        def(
            "glaeubiger_id",
            // Ländercode, zwei Prüfziffern, drei Zeichen
            // Geschäftsbereichskennung, dann die nationale Kennung. Die
            // Untergrenze von sechs Zeichen für die nationale Kennung ist die
            // kürzeste im SEPA-Raum vergebene (Frankreich); alles darunter
            // wäre für eine reine Prüfsummen-Absicherung zu wenig Substanz.
            r"(?<![0-9A-Za-z])[A-Z]{2}[0-9]{2}[0-9A-Z]{3}[0-9A-Z]{6,28}(?![0-9A-Za-z])",
            "SEPA-Gläubiger-ID (Creditor Identifier, mod-97-geprüft)",
            0.95,
            true,
            Some(Validator::CreditorId),
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
            // Das `{0,24}` vor „konto“ ist keine Kosmetik, sondern die
            // Korrektur zu #78 — siehe [`MAX_KEYWORD_PREFIX`]. Mit `*` an
            // dieser Stelle sprengte eine Zeile aus 1 600 Buchstaben das
            // Rückschritt-Budget und beendete den ganzen Lauf mit einem Fehler.
            mit_kontext!(
                r"(?i:[a-zäöüß]{0,24}konto(?:nummer|-?nr\.?)?|kto\.?)",
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
        "glaeubiger_id",
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

    /// Liefert jeden **unbegrenzten** Quantor eines Regex zusammen mit dem
    /// Ausdruck, auf den er sich bezieht — `"[a-zäöüß]*"`, `"(?:\.[0-9]{3})*"`.
    ///
    /// Zeichenklassen werden dabei übersprungen: das `+` in
    /// `[A-Za-z0-9._%+-]` ist ein Literal und kein Quantor. Genau daran ist
    /// eine Suche nach `regex.contains('+')` gescheitert, und deshalb steht
    /// hier ein kleiner Zeichenlauf statt einer Textsuche. Unbegrenzt sind
    /// `*`, `+` und `{n,}`; `{n,m}` ist begrenzt und interessiert nicht.
    fn unbegrenzte_quantoren(regex: &str) -> Vec<String> {
        let c: Vec<char> = regex.chars().collect();
        let text = |von: usize, bis: usize| c[von..bis.min(c.len())].iter().collect::<String>();

        let mut gefunden = Vec::new();
        let mut i = 0usize;
        // Anfang und Ende des zuletzt gelesenen Ausdrucks.
        let mut atom: Option<(usize, usize)> = None;
        let mut gruppen: Vec<usize> = Vec::new();

        while i < c.len() {
            match c[i] {
                // Maskiertes Zeichen: zwei Zeichen, ein Ausdruck.
                '\\' => {
                    atom = Some((i, i + 2));
                    i += 2;
                }
                // Zeichenklasse am Stück überlesen.
                '[' => {
                    let start = i;
                    i += 1;
                    if c.get(i) == Some(&'^') {
                        i += 1;
                    }
                    // Ein `]` unmittelbar am Anfang ist ein Literal.
                    if c.get(i) == Some(&']') {
                        i += 1;
                    }
                    while i < c.len() && c[i] != ']' {
                        i += if c[i] == '\\' { 2 } else { 1 };
                    }
                    i += 1;
                    atom = Some((start, i));
                }
                '(' => {
                    gruppen.push(i);
                    atom = None;
                    i += 1;
                }
                ')' => {
                    let start = gruppen.pop().unwrap_or(i);
                    i += 1;
                    atom = Some((start, i));
                }
                '*' | '+' => {
                    let (von, bis) = atom.unwrap_or((i, i));
                    gefunden.push(format!("{}{}", text(von, bis), c[i]));
                    atom = None;
                    i += 1;
                }
                '{' => {
                    let start = i;
                    while i < c.len() && c[i] != '}' {
                        i += 1;
                    }
                    i += 1;
                    // `{n,}` ist nach oben offen, `{n,m}` und `{n}` nicht.
                    if text(start, i).ends_with(",}") {
                        let (von, bis) = atom.unwrap_or((start, start));
                        gefunden.push(format!("{}{}", text(von, bis), text(start, i)));
                    }
                    atom = None;
                }
                _ => {
                    atom = Some((i, i + 1));
                    i += 1;
                }
            }
        }
        gefunden
    }

    /// Kein eingebautes Muster darf einen unbegrenzten Quantor an einer
    /// Stelle haben, an die die Suche von *jeder* Position aus hineinlaufen
    /// kann — genau daran hing #78.
    ///
    /// ## Wonach gesucht wird
    ///
    /// Ein unbegrenzter Quantor allein ist harmlos. Teuer wird er erst, wenn
    /// die Suche **von jeder Position aus** hineinlaufen kann: dann läuft sie
    /// über denselben Text so oft, wie er Zeichen hat. Zwei Dinge verhindern
    /// das, und jede Ausnahme unten nennt, welches davon greift:
    ///
    /// * ein **Look-behind** davor (`(?<![0-9A-Za-z])`) — er schneidet die
    ///   Startpositionen innerhalb eines Tokens weg, es bleibt eine je Token;
    /// * ein **Literal** davor („steuerliche", „konto") — dann sind die
    ///   Startpositionen die Fundstellen dieses Literals, nicht die Zeichen.
    ///
    /// Wo beides nicht ging, steht eine Obergrenze am Quantor selbst
    /// ([`MAX_KEYWORD_PREFIX`]).
    ///
    /// Geprüft wird die Bauform, nicht die Laufzeit — die misst
    /// `tests/backtracking.rs`. Dieser Test hier sagt einem neuen Muster
    /// *vorher*, worauf es zu achten hat.
    #[test]
    fn no_builtin_pattern_has_an_unguarded_unbounded_quantifier() {
        // Ausdruck + Begründung, je Muster. Wer ein Muster ändert und hier
        // landet, muss die Begründung mitliefern statt die Zeile zu streichen.
        let begruendet: &[(&str, &str)] = &[
            // Zwischen Schlüsselwort und Ziffern (Makro `mit_kontext!`):
            // erreichbar erst, nachdem ein Schlüsselwort-Literal gegriffen hat.
            (r"[ \t]*", "steht hinter einem Schlüsselwort-Literal"),
            // Dasselbe Argument: hinter dem Literal „steuerliche".
            (r"[ ]+", "steht hinter dem Literal „steuerliche“"),
            // Alles am Local-Part und an der Domain steht hinter
            // `(?<![A-Za-z0-9._%+-])` — je zusammenhängendem Token genau eine
            // Startposition. Die Label-Wiederholung beginnt zusätzlich mit
            // einem eigenen Punkt.
            (r"[A-Za-z0-9._%+-]+", "steht hinter einem Look-behind"),
            (r"[A-Za-z0-9-]+", "steht hinter `@` bzw. einem Punkt"),
            (
                r"(?:\.[A-Za-z0-9-]+)*",
                "jede Wiederholung beginnt mit einem Punkt",
            ),
            (
                r"[A-Za-z]{2,}",
                "letzter Ausdruck des Musters, davor ein Punkt",
            ),
            // Jede Wiederholung beginnt mit einem eigenen Trennzeichen, und
            // davor steht ein Look-behind.
            (
                r"(?:\.[0-9]{3})*",
                "jede Wiederholung beginnt mit einem Punkt",
            ),
            (
                r"(?:[ ][0-9]{2,6})+",
                "jede Wiederholung beginnt mit einem Leerzeichen",
            ),
        ];

        for d in builtin_patterns() {
            for quantor in unbegrenzte_quantoren(&d.regex) {
                assert!(
                    begruendet.iter().any(|(ausdruck, _)| *ausdruck == quantor),
                    "{}: unbegrenzter Quantor „{quantor}“ ohne Begründung.\n\
                     Entweder ein Look-behind oder ein Literal davor, oder eine \n\
                     Obergrenze am Quantor (siehe MAX_KEYWORD_PREFIX) — oder ein \n\
                     Eintrag in der Ausnahmeliste dieses Tests *mit* Begründung.\n\
                     Regex: {}",
                    d.id,
                    d.regex
                );
            }
        }

        // Gegenprobe: der Zeichenlauf findet die Bauform überhaupt, sonst wäre
        // der Test oben ein grüner Haken ohne Aussage.
        assert_eq!(
            unbegrenzte_quantoren(r"(?i:[a-zäöüß]*konto)"),
            vec!["[a-zäöüß]*"],
            "der alte, quadratische Präfix von #78 würde nicht mehr auffallen"
        );
        // Und ein Literal `+` in einer Zeichenklasse ist kein Quantor.
        assert!(unbegrenzte_quantoren(r"(?<![A-Za-z0-9._%+-])x").is_empty());
    }

    /// Die Zahl in [`MAX_KEYWORD_PREFIX`] und die Zahl im Regex sind dieselbe.
    ///
    /// `concat!` verlangt Literale, der Regex trägt die Obergrenze also
    /// ausgeschrieben. Ohne diesen Test liefe die Begründung an der Konstanten
    /// von der Wirkung im Muster weg.
    #[test]
    fn the_keyword_prefix_bound_is_the_one_that_is_documented() {
        let konto = builtin_patterns()
            .into_iter()
            .find(|d| d.id == "konto_nr")
            .unwrap();
        assert!(
            konto.regex.contains(&format!("{{0,{MAX_KEYWORD_PREFIX}}}")),
            "der Regex von konto_nr benutzt nicht die dokumentierte Obergrenze \
             {MAX_KEYWORD_PREFIX}: {}",
            konto.regex
        );
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
