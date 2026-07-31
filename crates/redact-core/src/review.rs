//! Review-Datei: das Austauschformat zwischen Analyse und Anwendung.
//!
//! Im Review-Modus schreibt die CLI alle gefundenen Regionen in eine JSON-Datei.
//! Diese kann manuell (oder in der GUI) geprüft, ergänzt und korrigiert werden;
//! mit `--apply-review` wird sie anschließend angewendet.

use serde::{Deserialize, Serialize};

use crate::conflict::BlockedRegion;
use crate::error::{RedactError, Result};
use crate::model::{Action, Redaction, Region};

pub const REVIEW_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewFile {
    pub version: u32,
    pub input: ReviewInput,
    /// Alle Treffer — auch die deaktivierten, damit die Entscheidung dokumentiert ist.
    pub items: Vec<ReviewItem>,
    /// Durch die Negativliste blockierte Treffer (rein informativ).
    #[serde(default)]
    pub blocked_by_negative_list: Vec<BlockedRegion>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewInput {
    pub path: String,
    pub sha256: String,
    pub pages: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewItem {
    pub id: usize,
    /// Wird diese Region geschwärzt? Auf `false` setzen, um sie zu verwerfen.
    pub enabled: bool,
    #[serde(default)]
    pub action: Action,
    pub region: Region,
}

impl ReviewFile {
    pub fn new(input: ReviewInput, regions: Vec<Region>, blocked: Vec<BlockedRegion>) -> Self {
        let items = regions
            .into_iter()
            .enumerate()
            .map(|(id, region)| ReviewItem {
                id,
                enabled: true,
                action: Action::Blackout,
                region,
            })
            .collect();
        Self {
            version: REVIEW_FORMAT_VERSION,
            input,
            items,
            blocked_by_negative_list: blocked,
        }
    }

    pub fn from_json(data: &str) -> Result<Self> {
        let file: ReviewFile = serde_json::from_str(data)?;
        if file.version != REVIEW_FORMAT_VERSION {
            return Err(RedactError::Parse(format!(
                "Review-Datei hat Version {}, erwartet wird {REVIEW_FORMAT_VERSION}",
                file.version
            )));
        }
        Ok(file)
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Alle aktivierten Einträge als Schwärzungs-Anweisungen.
    pub fn redactions(&self) -> Vec<Redaction> {
        self.items
            .iter()
            .filter(|i| i.enabled)
            .filter(|i| !i.region.is_blocking())
            .map(|i| Redaction {
                region: i.region.clone(),
                action: i.action.clone(),
                reason: i.region.reason(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Rect;
    use crate::model::{MatchType, Source};

    fn sample() -> ReviewFile {
        ReviewFile::new(
            ReviewInput {
                path: "in.pdf".into(),
                sha256: "abc".into(),
                pages: 1,
            },
            vec![
                Region::new(
                    0,
                    Rect::new(0.0, 0.0, 10.0, 10.0),
                    Some("DE89".into()),
                    Source::Pattern {
                        pattern_id: "iban_de".into(),
                        confidence: 1.0,
                    },
                ),
                Region::new(
                    0,
                    Rect::new(20.0, 0.0, 30.0, 10.0),
                    Some("Max Mustermann".into()),
                    Source::Booking {
                        booking_id: "b003".into(),
                        match_type: MatchType::Negative,
                    },
                ),
            ],
            vec![],
        )
    }

    #[test]
    fn roundtrip() {
        let f = sample();
        let json = f.to_json().unwrap();
        assert_eq!(ReviewFile::from_json(&json).unwrap(), f);
    }

    #[test]
    fn disabled_and_blocking_items_are_skipped() {
        let mut f = sample();
        assert_eq!(f.redactions().len(), 1); // Negativlisten-Eintrag fällt raus
        f.items[0].enabled = false;
        assert!(f.redactions().is_empty());
    }

    #[test]
    fn rejects_unknown_version() {
        let mut f = sample();
        f.version = 99;
        let json = serde_json::to_string(&f).unwrap();
        assert!(ReviewFile::from_json(&json).is_err());
    }
}
