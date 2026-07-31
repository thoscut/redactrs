//! Domänenmodell: Regionen, Schwärzungen und Buchungslisteneinträge.

use serde::{Deserialize, Serialize};

use crate::geometry::Rect;

/// Rechteckige Region auf einer PDF-Seite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    /// 0-basierte Seitennummer.
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
    Pattern { pattern_id: String, confidence: f32 },
    Booking {
        booking_id: String,
        match_type: MatchType,
    },
    Manual { reason: String },
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Schwarzes Rechteck + Text entfernen.
    Blackout,
    /// Weißes Rechteck + Text entfernen.
    Whiteout,
    /// Text entfernen und durch einen Platzhalter ersetzen (z.B. `[IBAN]`).
    Replace(String),
}

impl Default for Action {
    fn default() -> Self {
        Action::Blackout
    }
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

    #[test]
    fn region_constructor_normalizes_rect() {
        let r = Region::new(
            0,
            Rect {
                ll: Point::new(50.0, 90.0),
                ur: Point::new(10.0, 20.0),
            },
            None,
            Source::Manual {
                reason: "x".into(),
            },
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
        assert_eq!(serde_json::to_string(&Action::Blackout).unwrap(), "\"blackout\"");
        assert_eq!(
            serde_json::to_string(&Action::Replace("[IBAN]".into())).unwrap(),
            "{\"replace\":\"[IBAN]\"}"
        );
    }
}
