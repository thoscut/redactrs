//! Rechteck-Interaktion: Ziehen zum Anlegen, Klicken zum Auswählen.
//!
//! Der Zustand des Ziehvorgangs ist bewusst winzig und egui-nah; alles, was
//! danach passiert (Region anlegen, auswählen, verschieben), macht
//! [`crate::state::AppState`]. Die Treffersuche [`hit_test`] ist eine reine
//! Funktion und arbeitet in PDF-Koordinaten — sie ist damit unabhängig von
//! Zoom, Scrollposition und Fenstergröße testbar.

use egui::{Pos2, Response};
use redact_core::Point;

use crate::state::AnnotatedRegion;

/// Ab welcher Kantenlänge (in Bildschirmpunkten) ein Ziehen als Rechteck zählt.
/// Alles darunter ist ein Klick mit zittriger Hand.
pub const MIN_DRAG_SIZE: f32 = 4.0;

/// Verfolgt einen laufenden Ziehvorgang.
#[derive(Debug, Clone, Default)]
pub struct RectangleSelector {
    start: Option<Pos2>,
    current: Option<Pos2>,
}

impl RectangleSelector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Läuft gerade ein Ziehvorgang?
    pub fn is_active(&self) -> bool {
        self.start.is_some()
    }

    /// Das aufgezogene Rechteck in Bildschirmkoordinaten, solange gezogen wird.
    pub fn preview(&self) -> Option<egui::Rect> {
        match (self.start, self.current) {
            (Some(a), Some(b)) => Some(egui::Rect::from_two_pos(a, b)),
            _ => None,
        }
    }

    pub fn begin(&mut self, pos: Pos2) {
        self.start = Some(pos);
        self.current = Some(pos);
    }

    pub fn update(&mut self, pos: Pos2) {
        if self.start.is_some() {
            self.current = Some(pos);
        }
    }

    pub fn cancel(&mut self) {
        self.start = None;
        self.current = None;
    }

    /// Beendet den Ziehvorgang und liefert die beiden Eckpunkte — aber nur,
    /// wenn das Rechteck groß genug ist.
    pub fn finish(&mut self) -> Option<(Pos2, Pos2)> {
        let start = self.start.take();
        let current = self.current.take();
        let (a, b) = (start?, current?);
        is_significant(a, b).then_some((a, b))
    }

    /// Verarbeitet die egui-Response eines Seitenbereichs.
    ///
    /// Gibt beim Loslassen die beiden Eckpunkte zurück. egui 0.29 heißt die
    /// Methode [`Response::drag_stopped`]; `drag_released` ist entfernt.
    pub fn interact(&mut self, response: &Response) -> Option<(Pos2, Pos2)> {
        if response.drag_started() {
            if let Some(pos) = response.interact_pointer_pos() {
                self.begin(pos);
            }
        } else if response.dragged() {
            if let Some(pos) = response.interact_pointer_pos() {
                self.update(pos);
            }
        } else if response.drag_stopped() {
            return self.finish();
        }
        None
    }
}

/// Ist der aufgezogene Bereich groß genug, um als Rechteck zu gelten?
pub fn is_significant(a: Pos2, b: Pos2) -> bool {
    (a.x - b.x).abs() >= MIN_DRAG_SIZE && (a.y - b.y).abs() >= MIN_DRAG_SIZE
}

/// Sucht die Region unter einem Punkt (PDF-User-Space).
///
/// Es gewinnt die **kleinste** treffende Region; bei gleicher Fläche die
/// zuletzt angelegte — sie liegt zuoberst. Ohne diese Regel wäre eine kleine
/// Region, die in einer großen liegt, nicht mehr anklickbar.
pub fn hit_test(regions: &[AnnotatedRegion], page: usize, point: Point) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (index, entry) in regions.iter().enumerate() {
        if entry.region.page != page || !entry.region.rect.contains(point) {
            continue;
        }
        let area = entry.region.rect.area();
        match best {
            Some((_, best_area)) if area > best_area => {}
            _ => best = Some((index, area)),
        }
    }
    best.map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use redact_core::{Rect, Region, Source};

    fn manual(page: usize, rect: Rect) -> AnnotatedRegion {
        AnnotatedRegion::new(Region::new(
            page,
            rect,
            None,
            Source::Manual {
                reason: "test".into(),
            },
        ))
    }

    #[test]
    fn selector_tracks_a_drag() {
        let mut sel = RectangleSelector::new();
        assert!(!sel.is_active());
        assert!(sel.preview().is_none());

        sel.begin(Pos2::new(10.0, 10.0));
        assert!(sel.is_active());
        sel.update(Pos2::new(60.0, 40.0));
        let preview = sel.preview().unwrap();
        assert_eq!(preview.min, Pos2::new(10.0, 10.0));
        assert_eq!(preview.max, Pos2::new(60.0, 40.0));

        let (a, b) = sel.finish().unwrap();
        assert_eq!(a, Pos2::new(10.0, 10.0));
        assert_eq!(b, Pos2::new(60.0, 40.0));
        assert!(!sel.is_active());
    }

    #[test]
    fn selector_discards_tiny_drags_and_can_be_cancelled() {
        let mut sel = RectangleSelector::new();
        sel.begin(Pos2::new(10.0, 10.0));
        sel.update(Pos2::new(11.0, 11.0));
        assert!(sel.finish().is_none());
        assert!(!sel.is_active());

        sel.begin(Pos2::new(0.0, 0.0));
        sel.cancel();
        assert!(!sel.is_active());
        assert!(sel.finish().is_none());

        // Ohne begin() passiert bei update() nichts.
        let mut fresh = RectangleSelector::new();
        fresh.update(Pos2::new(5.0, 5.0));
        assert!(!fresh.is_active());
        assert!(fresh.preview().is_none());
    }

    #[test]
    fn significance_needs_both_axes() {
        assert!(is_significant(Pos2::new(0.0, 0.0), Pos2::new(10.0, 10.0)));
        // Nur breit, aber flach → kein Rechteck.
        assert!(!is_significant(Pos2::new(0.0, 0.0), Pos2::new(50.0, 1.0)));
        assert!(!is_significant(Pos2::new(0.0, 0.0), Pos2::new(1.0, 50.0)));
        // Richtung ist egal.
        assert!(is_significant(Pos2::new(50.0, 50.0), Pos2::new(0.0, 0.0)));
    }

    #[test]
    fn hit_test_prefers_the_smallest_region() {
        let regions = vec![
            manual(0, Rect::new(0.0, 0.0, 100.0, 100.0)),
            manual(0, Rect::new(40.0, 40.0, 60.0, 60.0)),
        ];
        assert_eq!(hit_test(&regions, 0, Point::new(50.0, 50.0)), Some(1));
        assert_eq!(hit_test(&regions, 0, Point::new(10.0, 10.0)), Some(0));
        assert_eq!(hit_test(&regions, 0, Point::new(200.0, 200.0)), None);
    }

    #[test]
    fn hit_test_prefers_the_topmost_on_a_tie() {
        let rect = Rect::new(0.0, 0.0, 10.0, 10.0);
        let regions = vec![manual(0, rect), manual(0, rect)];
        // Gleiche Fläche → die zuletzt angelegte Region gewinnt.
        assert_eq!(hit_test(&regions, 0, Point::new(5.0, 5.0)), Some(1));
    }

    #[test]
    fn hit_test_respects_the_page() {
        let regions = vec![
            manual(0, Rect::new(0.0, 0.0, 100.0, 100.0)),
            manual(1, Rect::new(0.0, 0.0, 100.0, 100.0)),
        ];
        assert_eq!(hit_test(&regions, 1, Point::new(5.0, 5.0)), Some(1));
        assert_eq!(hit_test(&regions, 2, Point::new(5.0, 5.0)), None);
        assert_eq!(hit_test(&[], 0, Point::new(5.0, 5.0)), None);
    }
}
