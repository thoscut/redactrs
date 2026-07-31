//! Konfliktauflösung: die Negativliste gewinnt immer.

use serde::{Deserialize, Serialize};

use crate::geometry::Rect;
use crate::model::{MatchType, Region, Source};

/// Ein durch die Negativliste blockierter Treffer (für das Audit-Log).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockedRegion {
    pub page: usize,
    pub rect: Rect,
    /// Text der blockierenden Negativlisten-Region.
    pub pattern: String,
    pub booking_id: String,
    /// Beschreibung des Treffers, der dadurch verhindert wurde.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
}

/// Ergebnis der Konfliktauflösung.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Resolution {
    /// Regionen, die geschwärzt werden sollen.
    pub redact: Vec<Region>,
    /// Treffer, die durch die Negativliste verhindert wurden.
    pub blocked: Vec<BlockedRegion>,
}

/// Ab welchem Überdeckungsgrad eine Negativregion einen Treffer blockiert.
const BLOCK_COVERAGE_THRESHOLD: f64 = 0.5;

/// Trennt Negativlisten-Treffer ab und entfernt alle Regionen, die von ihnen
/// überdeckt werden.
///
/// Regeln (siehe Konzept §5.3):
/// * Ein Treffer in der Negativliste blockiert eine Schwärzung — auch wenn ein
///   Pattern oder die Positivliste ebenfalls trifft.
/// * Manuelle Regionen sind eine bewusste Nutzerentscheidung und werden **nicht**
///   blockiert; sie überstimmen die Negativliste.
pub fn resolve_conflicts(regions: Vec<Region>) -> Resolution {
    let (negatives, candidates): (Vec<Region>, Vec<Region>) =
        regions.into_iter().partition(|r| r.is_blocking());

    let mut result = Resolution::default();

    'outer: for cand in candidates {
        // Manuelle Regionen sind immer stärker als die Negativliste.
        let manual = matches!(cand.source, Source::Manual { .. });
        if !manual {
            for neg in &negatives {
                if neg.page != cand.page {
                    continue;
                }
                if cand.rect.covered_fraction(&neg.rect) >= BLOCK_COVERAGE_THRESHOLD {
                    result.blocked.push(BlockedRegion {
                        page: cand.page,
                        rect: cand.rect,
                        pattern: neg.text.clone().unwrap_or_default(),
                        booking_id: match &neg.source {
                            Source::Booking { booking_id, .. } => booking_id.clone(),
                            _ => String::new(),
                        },
                        blocked_reason: Some(cand.reason()),
                    });
                    continue 'outer;
                }
            }
        }
        result.redact.push(cand);
    }

    dedup(&mut result.redact);
    result
}

/// Entfernt exakte Duplikate und Regionen, die vollständig in einer anderen
/// Region derselben Seite enthalten sind.
fn dedup(regions: &mut Vec<Region>) {
    let mut keep: Vec<Region> = Vec::with_capacity(regions.len());
    for region in regions.drain(..) {
        let redundant = keep.iter().any(|k| {
            k.page == region.page
                && k.source == region.source
                && region.rect.covered_fraction(&k.rect) >= 0.999
        });
        if !redundant {
            keep.push(region);
        }
    }
    *regions = keep;
}

/// Alle Rechtecke einer Seite, die verhindern, dass Text entfernt wird.
pub fn negative_rects(regions: &[Region], page: usize) -> Vec<Rect> {
    regions
        .iter()
        .filter(|r| {
            r.page == page
                && matches!(
                    r.source,
                    Source::Booking {
                        match_type: MatchType::Negative,
                        ..
                    }
                )
        })
        .map(|r| r.rect)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Region;

    fn pattern_region(page: usize, rect: Rect) -> Region {
        Region::new(
            page,
            rect,
            Some("DE89 3704 0044 0532 0130 00".into()),
            Source::Pattern {
                pattern_id: "iban_de".into(),
                confidence: 0.99,
            },
        )
    }

    fn negative_region(page: usize, rect: Rect) -> Region {
        Region::new(
            page,
            rect,
            Some("Max Mustermann".into()),
            Source::Booking {
                booking_id: "b003".into(),
                match_type: MatchType::Negative,
            },
        )
    }

    #[test]
    fn negative_list_blocks_overlapping_hit() {
        let regions = vec![
            pattern_region(0, Rect::new(10.0, 10.0, 50.0, 20.0)),
            negative_region(0, Rect::new(0.0, 0.0, 100.0, 30.0)),
        ];
        let res = resolve_conflicts(regions);
        assert!(res.redact.is_empty());
        assert_eq!(res.blocked.len(), 1);
        assert_eq!(res.blocked[0].booking_id, "b003");
    }

    #[test]
    fn negative_list_on_other_page_does_not_block() {
        let regions = vec![
            pattern_region(1, Rect::new(10.0, 10.0, 50.0, 20.0)),
            negative_region(0, Rect::new(0.0, 0.0, 100.0, 30.0)),
        ];
        let res = resolve_conflicts(regions);
        assert_eq!(res.redact.len(), 1);
        assert!(res.blocked.is_empty());
    }

    #[test]
    fn manual_region_overrides_negative_list() {
        let manual = Region::new(
            0,
            Rect::new(10.0, 10.0, 50.0, 20.0),
            None,
            Source::Manual {
                reason: "Gehalt".into(),
            },
        );
        let res = resolve_conflicts(vec![manual, negative_region(0, Rect::new(0.0, 0.0, 100.0, 30.0))]);
        assert_eq!(res.redact.len(), 1);
        assert!(res.blocked.is_empty());
    }

    #[test]
    fn barely_touching_negative_does_not_block() {
        // Nur 20% Überdeckung → Treffer bleibt bestehen.
        let regions = vec![
            pattern_region(0, Rect::new(0.0, 0.0, 100.0, 10.0)),
            negative_region(0, Rect::new(80.0, 0.0, 200.0, 10.0)),
        ];
        let res = resolve_conflicts(regions);
        assert_eq!(res.redact.len(), 1);
    }

    #[test]
    fn duplicates_are_removed() {
        let r = Rect::new(10.0, 10.0, 50.0, 20.0);
        let res = resolve_conflicts(vec![pattern_region(0, r), pattern_region(0, r)]);
        assert_eq!(res.redact.len(), 1);
    }
}
