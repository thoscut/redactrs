//! Domänenmodell: Regionen, Schwärzungen und Buchungslisteneinträge.

use serde::{Deserialize, Deserializer, Serialize};

use crate::geometry::Rect;

/// Größte Seitennummer, die aus einer Datei angenommen wird (0-basiert).
///
/// Gebunden an lopdf: `Document::get_pages()` nummeriert Seiten als `u32`,
/// mehr Seiten kann ein Dokument für dieses Programm also gar nicht haben.
/// Alles darüber ist keine Seitenzahl, sondern ein Wert, der irgendwo
/// überlaufen wird.
///
/// ## Warum überhaupt eine Grenze
///
/// `Region::page` kommt ungeprüft aus Review-JSON (`--apply-review`) und aus
/// `--manual-regions`; `serde_json` nimmt für ein `usize` jede Zahl bis
/// `18446744073709551615` an. Die Anzeige rechnet daraus „Seite n + 1“
/// (Fließtext zählt ab 1, JSON ab 0) — und genau das lief über. Gemessen mit
/// `"page": 18446744073709551615` in einer sonst gültigen Review-Datei:
///
/// | Binary | Rückgabewert | Verhalten |
/// |---|---|---|
/// | Debug | 101 | Panic in `audit.rs:453` (`attempt to add with overflow`) |
/// | Release | 0 | `usize::MAX + 1 = 0` — Warnung nennt „Seite 0“, Ausgabe wird geschrieben |
///
/// Der Release-Fall ist der schlimmere: rc 0, eine Ausgabedatei, und eine
/// Warnung, die auf die falsche Seite zeigt. Mit der Grenze bricht der Lauf
/// mit rc 1 ab, bevor er das Dokument anfasst: die Datei taugt nicht, wie
/// bei krummem JSON.
///
/// ## Was die Grenze **nicht** tut
///
/// Sie unterscheidet nicht zwischen „Seite gibt es in *diesem* Dokument
/// nicht“ und „das ist keine Seitenzahl“. `"page": 99` im Einseiter bleibt
/// weich: rc 0, Warnung mit `missing_page` im Audit-Log
/// (`a_wildly_wrong_page_number_is_reported_as_such`). Wer die Seitenzahl
/// eines anderen Dokuments einträgt, hat sich vertan; wer `u64::MAX`
/// einträgt, hat keine Seite gemeint.
pub const MAX_PAGE_INDEX: usize = u32::MAX as usize;

/// `deserialize_with` für Seitennummern aus fremder Hand — siehe
/// [`MAX_PAGE_INDEX`].
///
/// Ein serde-Fehler hieraus läuft in `ReviewFile::from_json` über
/// `From<serde_json::Error>` in [`RedactError::Parse`](crate::RedactError::Parse)
/// und damit in der Kommandozeile auf Rückgabewert 1 — denselben Weg, den
/// „keine UTF-8-Datei“ und „krummes JSON“ nehmen.
pub(crate) fn page_index<'de, D: Deserializer<'de>>(d: D) -> Result<usize, D::Error> {
    let page = usize::deserialize(d)?;
    if page > MAX_PAGE_INDEX {
        return Err(serde::de::Error::custom(format!(
            "\"page\": {page} ist keine Seitenzahl (höchstens {MAX_PAGE_INDEX})"
        )));
    }
    Ok(page)
}

/// Rechteckige Region auf einer PDF-Seite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    /// 0-basierte Seitennummer. Aus einer Datei höchstens [`MAX_PAGE_INDEX`].
    #[serde(deserialize_with = "page_index")]
    pub page: usize,
    /// Bounding Box im PDF-User-Space.
    pub rect: Rect,
    /// Extrahierter Text (falls verfügbar).
    #[serde(default)]
    pub text: Option<String>,
    /// Herkunft des Treffers.
    pub source: Source,
}

impl Region {
    pub fn new(page: usize, rect: Rect, text: Option<String>, source: Source) -> Self {
        Self {
            page,
            rect: rect.normalized(),
            text,
            source,
        }
    }

    /// Soll diese Region geschwärzt werden? Negativlisten-Treffer nicht.
    pub fn is_blocking(&self) -> bool {
        matches!(
            self.source,
            Source::Booking {
                match_type: MatchType::Negative,
                ..
            }
        )
    }

    /// Kurzbeschreibung für Audit-Log und UI.
    pub fn reason(&self) -> String {
        match &self.source {
            Source::Pattern {
                pattern_id,
                confidence,
            } => format!("pattern: {pattern_id} (confidence {confidence:.2})"),
            Source::Booking {
                booking_id,
                match_type,
            } => format!(
                "booking: {booking_id} ({})",
                match match_type {
                    MatchType::Positive => "positive",
                    MatchType::Negative => "negative",
                }
            ),
            Source::Manual { reason } => reason.clone(),
        }
    }

    /// Grobe Herkunftskategorie für das Audit-Log.
    pub fn source_kind(&self) -> &'static str {
        match self.source {
            Source::Pattern { .. } => "auto",
            Source::Booking { .. } => "booking_list",
            Source::Manual { .. } => "manual",
        }
    }
}

/// Herkunft eines Treffers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Pattern {
        pattern_id: String,
        confidence: f32,
    },
    Booking {
        booking_id: String,
        match_type: MatchType,
    },
    Manual {
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchType {
    /// In Positivliste → muss geschwärzt werden.
    Positive,
    /// In Negativliste → darf NICHT geschwärzt werden.
    Negative,
}

/// Eine konkrete Schwärzungsoperation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Redaction {
    pub region: Region,
    pub action: Action,
    pub reason: String,
}

impl Redaction {
    pub fn new(region: Region, action: Action) -> Self {
        let reason = region.reason();
        Self {
            region,
            action,
            reason,
        }
    }

    pub fn page(&self) -> usize {
        self.region.page
    }

    pub fn rect(&self) -> Rect {
        self.region.rect
    }
}

/// Vorgabe für den Ersatztext von [`Action::Replace`].
///
/// ## Warum die Zeichenkette hier steht und nicht zweimal woanders
///
/// Sie war zweimal da: als clap-Literal in `crates/redact-cli/src/cli.rs`
/// (`--replace-with`, `default_value`) und als eigene Konstante in
/// `crates/redact-gui/src/state.rs`. Davor war sie zweimal *verschieden* — die
/// Oberfläche schlug `"[REDACTED]"` vor: englischer Text in einer deutschen
/// Oberfläche und ein anderer als der, den ein Lauf ohne `--gui` schreibt.
/// Repariert wurde das mit einem Test, der die Gleichheit der beiden Literale
/// festhielt. Ein Test, der zwei Wahrheiten vergleicht, ist aber ein Pflaster:
/// er meldet das Auseinanderlaufen, nachdem es passiert ist, und nur solange
/// jemand ihn mitpflegt.
///
/// `redact-core` ist der Ort, an dem beide Programme ohnehin dieselbe Sprache
/// sprechen ([`Action`] steht schon hier). Eine Konstante an dieser Stelle
/// *kann* nicht auseinanderlaufen.
///
/// ## Warum dieser Text
///
/// Deutsch, weil das Programm deutsch ist. In eckigen Klammern, weil der Leser
/// des Dokuments sehen soll, dass hier etwas entfernt wurde — ein Ersatztext,
/// der wie Inhalt aussieht, verschleiert die Schwärzung, statt sie
/// auszuweisen.
pub const DEFAULT_REPLACEMENT: &str = "[GESCHWÄRZT]";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Schwarzes Rechteck + Text entfernen.
    #[default]
    Blackout,
    /// Weißes Rechteck + Text entfernen.
    Whiteout,
    /// Text entfernen und durch einen Platzhalter ersetzen (z.B. `[IBAN]`).
    ///
    /// Die Vorgabe für den Text ist [`DEFAULT_REPLACEMENT`].
    Replace(String),
}

impl Action {
    /// Füllfarbe des Deck-Rechtecks als RGB (0.0 … 1.0).
    pub fn fill_color(&self) -> Option<(f64, f64, f64)> {
        match self {
            Action::Blackout => Some((0.0, 0.0, 0.0)),
            Action::Whiteout => Some((1.0, 1.0, 1.0)),
            Action::Replace(_) => Some((1.0, 1.0, 1.0)),
        }
    }

    /// Ersatztext, der über das Rechteck gesetzt wird.
    pub fn replacement(&self) -> Option<&str> {
        match self {
            Action::Replace(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

/// Ein Eintrag aus der Buchungsliste (CSV).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BookingEntry {
    /// Eindeutige Buchungs-ID.
    pub id: String,
    /// Positiv- oder Negativliste.
    pub list_type: ListType,
    /// Zu suchender Text (oder Regex, wenn `is_regex` gesetzt ist).
    pub pattern: String,
    /// Kontext vor dem Match (zur Verifikation).
    #[serde(default)]
    pub context_before: Option<String>,
    /// Kontext nach dem Match.
    #[serde(default)]
    pub context_after: Option<String>,
    /// `pattern` als regulärer Ausdruck auswerten.
    #[serde(default)]
    pub is_regex: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ListType {
    Positive,
    Negative,
}

impl ListType {
    pub fn match_type(&self) -> MatchType {
        match self {
            ListType::Positive => MatchType::Positive,
            ListType::Negative => MatchType::Negative,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Point;

    fn region(source: Source) -> Region {
        Region::new(1, Rect::new(0.0, 0.0, 10.0, 10.0), None, source)
    }

    #[test]
    fn manual_region_json_matches_spec_format() {
        let json = r#"{
            "page": 2,
            "rect": { "ll": { "x": 100.0, "y": 500.0 }, "ur": { "x": 300.0, "y": 520.0 } },
            "text": null,
            "source": { "manual": { "reason": "Gehaltsabrechnung" } }
        }"#;
        let r: Region = serde_json::from_str(json).unwrap();
        assert_eq!(r.page, 2);
        assert_eq!(r.rect.ll, Point::new(100.0, 500.0));
        assert_eq!(
            r.source,
            Source::Manual {
                reason: "Gehaltsabrechnung".into()
            }
        );
    }

    /// Die Grenze liegt genau bei [`MAX_PAGE_INDEX`]: der Wert selbst geht
    /// durch, eins darüber nicht — mit einer Meldung, die den Wert und die
    /// Grenze nennt.
    #[test]
    fn a_page_number_beyond_the_page_tree_is_refused_at_the_boundary() {
        let json = |page: u64| {
            format!(
                r#"{{"page": {page}, "rect": {{ "ll": {{ "x": 0.0, "y": 0.0 }}, "ur": {{ "x": 1.0, "y": 1.0 }} }},
                     "source": {{ "manual": {{ "reason": "x" }} }}}}"#
            )
        };
        let max = MAX_PAGE_INDEX as u64;
        assert_eq!(
            serde_json::from_str::<Region>(&json(max)).unwrap().page,
            MAX_PAGE_INDEX
        );
        let err = serde_json::from_str::<Region>(&json(max + 1)).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("keine Seitenzahl"), "{msg}");
        assert!(msg.contains("4294967296"), "{msg}");
        assert!(msg.contains("4294967295"), "{msg}");
        let err = serde_json::from_str::<Region>(&json(u64::MAX)).unwrap_err();
        assert!(err.to_string().contains("keine Seitenzahl"), "{err}");
    }

    /// Die Grenze ist eine Zusage an den Seitenbaum von lopdf, keine
    /// Geschmacksfrage: dort ist die Seitennummer ein `u32`.
    #[test]
    fn the_page_limit_is_the_page_tree_key_type_of_lopdf() {
        let pages: std::collections::BTreeMap<u32, lopdf::ObjectId> =
            lopdf::Document::new().get_pages();
        let _: Option<&u32> = pages.keys().next();
        assert_eq!(MAX_PAGE_INDEX, u32::MAX as usize);
    }

    #[test]
    fn region_constructor_normalizes_rect() {
        let r = Region::new(
            0,
            Rect {
                ll: Point::new(50.0, 90.0),
                ur: Point::new(10.0, 20.0),
            },
            None,
            Source::Manual { reason: "x".into() },
        );
        assert_eq!(r.rect.ll, Point::new(10.0, 20.0));
        assert_eq!(r.rect.ur, Point::new(50.0, 90.0));
    }

    #[test]
    fn negative_booking_blocks() {
        let neg = region(Source::Booking {
            booking_id: "b003".into(),
            match_type: MatchType::Negative,
        });
        assert!(neg.is_blocking());
        let pos = region(Source::Booking {
            booking_id: "b001".into(),
            match_type: MatchType::Positive,
        });
        assert!(!pos.is_blocking());
    }

    #[test]
    fn action_serializes_as_lowercase_tag() {
        assert_eq!(
            serde_json::to_string(&Action::Blackout).unwrap(),
            "\"blackout\""
        );
        assert_eq!(
            serde_json::to_string(&Action::Replace("[IBAN]".into())).unwrap(),
            "{\"replace\":\"[IBAN]\"}"
        );
    }
}
