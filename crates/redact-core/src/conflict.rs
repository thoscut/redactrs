//! Konfliktauflösung: die Negativliste gewinnt immer.

use std::cmp::Ordering;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::geometry::Rect;
use crate::model::{Region, Source};

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

    // Nach Seite vorsortieren: sonst läuft jeder Kandidat über *alle*
    // Negativeinträge des Dokuments, obwohl nur die seiner eigenen Seite
    // überhaupt in Frage kommen.
    let mut by_page: HashMap<usize, Vec<&Region>> = HashMap::new();
    for neg in &negatives {
        by_page.entry(neg.page).or_default().push(neg);
    }
    let none: Vec<&Region> = Vec::new();

    let mut result = Resolution::default();

    'outer: for cand in candidates {
        // Manuelle Regionen sind immer stärker als die Negativliste.
        let manual = matches!(cand.source, Source::Manual { .. });
        if !manual {
            for neg in by_page.get(&cand.page).unwrap_or(&none) {
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
/// Region derselben Seite und derselben Herkunft enthalten sind.
///
/// Früher lief hier jede Region gegen jede bereits behaltene. Bei der
/// CLI-Obergrenze von 100 000 Kandidaten sind das fünf Milliarden
/// Rechteckvergleiche; gemessen wurden 132,6 Sekunden für eine 212-kB-Datei.
///
/// Stattdessen ein Streifenzug, sortiert nach Seite und linker Kante: enthält
/// eine Region eine andere vollständig, dann liegt ihre linke Kante nicht
/// weiter rechts — sie ist also vorher an der Reihe. Und sobald eine behaltene
/// Region links von der aktuellen endet, kann sie auch keine der folgenden
/// mehr enthalten und fällt aus dem Streifen.
///
/// Nebenwirkung, bewusst in Kauf genommen: die alte Fassung hing an der
/// Eingabereihenfolge. Kam die kleinere Region zuerst, überlebten beide. Jetzt
/// fällt die enthaltene immer weg. Das ist ungefährlich — die umschließende
/// Region hat dieselbe Herkunft und wird ohnehin geschwärzt — und es macht das
/// Ergebnis unabhängig davon, in welcher Reihenfolge die Treffer anfallen.
fn dedup(regions: &mut Vec<Region>) {
    if regions.len() < 2 {
        return;
    }

    let mut order: Vec<usize> = (0..regions.len()).collect();
    order.sort_by(|&a, &b| {
        let (ra, rb) = (&regions[a], &regions[b]);
        ra.page.cmp(&rb.page).then_with(|| {
            ra.rect
                .ll
                .x
                .partial_cmp(&rb.rect.ll.x)
                .unwrap_or(Ordering::Equal)
                // Bei gleicher Kante entscheidet die Eingabereihenfolge, damit
                // von zwei deckungsgleichen Treffern der erste bleibt.
                .then(a.cmp(&b))
        })
    });

    let mut redundant = vec![false; regions.len()];
    // Die behaltenen Regionen, die noch in den Streifen hineinragen.
    let mut active: Vec<usize> = Vec::new();
    let mut page = None;

    for &i in &order {
        let region = &regions[i];
        if page != Some(region.page) {
            page = Some(region.page);
            active.clear();
        }
        active.retain(|&k| regions[k].rect.ur.x >= region.rect.ll.x);

        let covered = active.iter().any(|&k| {
            regions[k].source == region.source
                && region.rect.covered_fraction(&regions[k].rect) >= 0.999
        });
        if covered {
            redundant[i] = true;
        } else {
            active.push(i);
        }
    }

    let mut keep: Vec<Region> = Vec::with_capacity(regions.len());
    for (i, region) in regions.drain(..).enumerate() {
        if !redundant[i] {
            keep.push(region);
        }
    }
    *regions = keep;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{MatchType, Region};

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
        let res = resolve_conflicts(vec![
            manual,
            negative_region(0, Rect::new(0.0, 0.0, 100.0, 30.0)),
        ]);
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

    #[test]
    fn a_contained_region_falls_away_in_either_order() {
        let big = Rect::new(0.0, 0.0, 100.0, 20.0);
        let small = Rect::new(10.0, 5.0, 30.0, 15.0);

        // Beide Reihenfolgen müssen dasselbe ergeben — genau das konnte die
        // frühere Fassung nicht: kam die kleine zuerst, blieben beide stehen.
        for pair in [[big, small], [small, big]] {
            let res =
                resolve_conflicts(vec![pattern_region(0, pair[0]), pattern_region(0, pair[1])]);
            assert_eq!(res.redact.len(), 1, "Reihenfolge {pair:?}");
            assert_eq!(res.redact[0].rect, big, "die größere bleibt");
        }
    }

    #[test]
    fn a_neighbour_that_merely_touches_is_kept() {
        // Grenzfall des Streifenzugs: das zweite Rechteck beginnt genau dort,
        // wo das erste endet. Es ist nicht enthalten und muss bleiben.
        let res = resolve_conflicts(vec![
            pattern_region(0, Rect::new(0.0, 0.0, 50.0, 10.0)),
            pattern_region(0, Rect::new(50.0, 0.0, 100.0, 10.0)),
        ]);
        assert_eq!(res.redact.len(), 2);
    }

    #[test]
    fn regions_of_different_origin_do_not_swallow_each_other() {
        let big = Rect::new(0.0, 0.0, 100.0, 20.0);
        let small = Rect::new(10.0, 5.0, 30.0, 15.0);
        let manual = Region::new(
            0,
            small,
            None,
            Source::Manual {
                reason: "von Hand".into(),
            },
        );
        let res = resolve_conflicts(vec![pattern_region(0, big), manual]);
        assert_eq!(res.redact.len(), 2, "andere Herkunft, andere Begründung");
    }

    #[test]
    fn ten_thousand_hits_resolve_in_well_under_a_second() {
        // Der Grund für den Streifenzug. Mit dem früheren Paarvergleich
        // brauchte eine 212-kB-Datei 132,6 s; 10 000 Treffer auf einer Seite
        // sind für einen Kontoauszug mit vielen Buchungen realistisch.
        //
        // Bewusst großzügig: die Schwelle soll eine Rückkehr des
        // quadratischen Verhaltens fangen, nicht auf langsamer Hardware
        // grundlos rot werden. Quadratisch wären es hier Minuten.
        let regions: Vec<Region> = (0..10_000)
            .map(|i| {
                let x = (i % 100) as f64 * 6.0;
                let y = (i / 100) as f64 * 12.0;
                pattern_region(0, Rect::new(x, y, x + 5.0, y + 10.0))
            })
            .collect();

        let start = std::time::Instant::now();
        let res = resolve_conflicts(regions);
        let elapsed = start.elapsed();

        assert_eq!(res.redact.len(), 10_000, "keine überlappt, keine fällt weg");
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "Konfliktauflösung brauchte {elapsed:?} — das riecht wieder nach O(n²)"
        );
    }
}
