//! Rückgängig und Wiederholen als Schnappschuss-Stapel.
//!
//! Kein Befehlsmuster: die Trefferliste ist ein `Vec<AnnotatedRegion>` mit
//! typischerweise ein paar Dutzend, im schlimmsten Fall ein paar hundert
//! Einträgen — je Änderung eine vollständige Kopie kostet weniger als der
//! Aufwand, für jede Änderungsart eine eigene Umkehroperation zu schreiben und
//! zu prüfen. Der Stapel ist auf [`HISTORY_LIMIT`] Schnappschüsse begrenzt;
//! ältere fallen unten heraus.
//!
//! Zwei Regeln, die man leicht falsch macht:
//!
//! * [`History::record`] wird **vor** der Änderung gerufen und legt den
//!   Zustand *davor* ab. Nur so führt ein Rückgängig dorthin zurück.
//! * Eine neue Änderung nach einem Rückgängig macht den Wiederholen-Stapel
//!   ungültig — der Zweig, in den er zurückführte, existiert nicht mehr.

use std::collections::VecDeque;

use crate::state::AnnotatedRegion;

/// Höchstzahl aufbewahrter Schnappschüsse je Richtung.
pub const HISTORY_LIMIT: usize = 50;

/// Ein Stand der Trefferliste.
pub type Snapshot = Vec<AnnotatedRegion>;

/// Der Stapel für Rückgängig/Wiederholen.
#[derive(Debug, Default)]
pub struct History {
    past: VecDeque<Snapshot>,
    future: Vec<Snapshot>,
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    /// Legt den Zustand **vor** einer Änderung ab und verwirft den
    /// Wiederholen-Stapel.
    pub fn record(&mut self, current: &[AnnotatedRegion]) {
        self.push_past(current.to_vec());
        self.future.clear();
    }

    /// Einen Schritt zurück. Gibt den wiederherzustellenden Stand zurück;
    /// `current` wandert dafür auf den Wiederholen-Stapel.
    pub fn undo(&mut self, current: &[AnnotatedRegion]) -> Option<Snapshot> {
        let previous = self.past.pop_back()?;
        self.future.push(current.to_vec());
        if self.future.len() > HISTORY_LIMIT {
            self.future.remove(0);
        }
        Some(previous)
    }

    /// Einen Schritt vor. Der Wiederholen-Stapel bleibt dabei erhalten —
    /// deshalb **nicht** über [`History::record`].
    pub fn redo(&mut self, current: &[AnnotatedRegion]) -> Option<Snapshot> {
        let next = self.future.pop()?;
        self.push_past(current.to_vec());
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// Wirft alles weg — beim Laden eines anderen Dokuments gehört der alte
    /// Verlauf nicht mehr zum Inhalt.
    pub fn clear(&mut self) {
        self.past.clear();
        self.future.clear();
    }

    /// Anzahl der Schritte, die zurückgegangen werden können.
    pub fn undo_depth(&self) -> usize {
        self.past.len()
    }

    /// Anzahl der Schritte, die wieder vorgegangen werden können.
    pub fn redo_depth(&self) -> usize {
        self.future.len()
    }

    fn push_past(&mut self, snapshot: Snapshot) {
        if self.past.len() == HISTORY_LIMIT {
            self.past.pop_front();
        }
        self.past.push_back(snapshot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use redact_core::{Rect, Region, Source};

    /// Trefferliste mit `n` unterscheidbaren Einträgen.
    fn regions(n: usize) -> Snapshot {
        (0..n)
            .map(|i| {
                AnnotatedRegion::new(Region::new(
                    0,
                    Rect::new(i as f64, 0.0, i as f64 + 5.0, 10.0),
                    None,
                    Source::Manual {
                        reason: format!("R{i}"),
                    },
                ))
            })
            .collect()
    }

    #[test]
    fn undo_and_redo_walk_back_and_forth() {
        let mut history = History::new();
        assert!(!history.can_undo());
        assert!(!history.can_redo());
        assert_eq!(history.undo(&regions(1)), None);
        assert_eq!(history.redo(&regions(1)), None);

        // Drei Änderungen: 0 → 1 → 2 → 3 Einträge.
        let mut current = regions(0);
        for step in 1..=3 {
            history.record(&current);
            current = regions(step);
        }
        assert_eq!(history.undo_depth(), 3);

        // Zurück bis zum Anfang.
        for step in (0..3).rev() {
            current = history.undo(&current).expect("Schritt zurück");
            assert_eq!(current.len(), step);
        }
        assert!(!history.can_undo());
        assert_eq!(history.redo_depth(), 3);

        // Und wieder vor.
        for step in 1..=3 {
            current = history.redo(&current).expect("Schritt vor");
            assert_eq!(current.len(), step);
        }
        assert!(!history.can_redo());
        assert_eq!(history.undo_depth(), 3);
    }

    /// Die Grenze ist hart: der 51. Schnappschuss verdrängt den ersten.
    #[test]
    fn the_stack_stops_at_the_limit_and_drops_the_oldest() {
        let mut history = History::new();
        let mut current = regions(0);
        // 60 Änderungen: 0 → 1 → … → 60 Einträge.
        for step in 1..=60 {
            history.record(&current);
            current = regions(step);
        }
        assert_eq!(history.undo_depth(), HISTORY_LIMIT);

        // Alle aufbewahrten Schritte zurückgehen.
        for _ in 0..HISTORY_LIMIT {
            current = history.undo(&current).unwrap();
        }
        assert!(!history.can_undo());
        // 60 Änderungen, 50 aufbewahrt → der älteste erreichbare Stand ist der
        // mit 10 Einträgen, **nicht** der leere Anfangszustand.
        assert_eq!(current.len(), 60 - HISTORY_LIMIT);
    }

    /// Nach einer neuen Änderung darf „Wiederholen“ nicht in einen Zweig
    /// zurückführen, den es nicht mehr gibt.
    #[test]
    fn a_new_change_invalidates_redo() {
        let mut history = History::new();
        let mut current = regions(0);

        history.record(&current);
        current = regions(1);
        current = history.undo(&current).unwrap();
        assert!(history.can_redo());
        assert_eq!(current.len(), 0);

        // Eine andere Änderung an derselben Stelle.
        history.record(&current);
        current = regions(7);
        assert!(!history.can_redo(), "Wiederholen muss verfallen sein");
        assert_eq!(history.redo(&current), None);

        // Rückgängig führt in den neuen Zweig zurück.
        current = history.undo(&current).unwrap();
        assert_eq!(current.len(), 0);
    }

    #[test]
    fn clearing_removes_both_directions() {
        let mut history = History::new();
        let mut current = regions(0);
        history.record(&current);
        current = regions(2);
        current = history.undo(&current).unwrap();
        assert!(history.can_redo());

        history.clear();
        assert!(!history.can_undo());
        assert!(!history.can_redo());
        assert_eq!(history.undo(&current), None);
    }
}
