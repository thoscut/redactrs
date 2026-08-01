//! Rechteck-Interaktion: Ziehen zum Anlegen, Klicken zum Auswählen, Ziehen an
//! den Eckgriffen zum Korrigieren.
//!
//! Der Zustand des Ziehvorgangs ist bewusst winzig und egui-nah; alles, was
//! danach passiert (Region anlegen, auswählen, verschieben, ändern), macht
//! [`crate::state::AppState`]. Die Treffersuche [`hit_test`] ist eine reine
//! Funktion und arbeitet in PDF-Koordinaten — sie ist damit unabhängig von
//! Zoom, Scrollposition und Fenstergröße testbar. Die Griffsuche [`hit_handle`]
//! ist ebenso rein und arbeitet in Bildschirmkoordinaten, weil ein Griff eine
//! Sache der Maus ist und nicht des Dokuments.
//!
//! Was egui je Bild über die Maus weiß, steht in [`PointerFrame`] — reine
//! Daten. Nur so lässt sich prüfen, dass ein Rechteck **am Druckpunkt** und
//! nicht dort beginnt, wo egui den Zug bemerkt hat.

use egui::{CursorIcon, Pos2, Response};
use redact_core::Point;

use crate::state::{AnnotatedRegion, RegionId};

/// Ab welcher Kantenlänge (in Bildschirmpunkten) ein Ziehen als Rechteck zählt.
/// Alles darunter ist ein Klick mit zittriger Hand.
pub const MIN_DRAG_SIZE: f32 = 4.0;

/// Durchmesser der Trefferzone eines Eckgriffs in Bildschirmpunkten.
///
/// Deutlich größer als das gezeichnete Quadrat ([`crate::viewer::HANDLE_SIZE`],
/// 5 pt): eine 5-pt-Zone trifft man mit der Maus nur zufällig. Ein Fangbereich,
/// der großzügiger ist als die Zeichnung, ist bei Griffen üblich und richtig.
pub const HANDLE_HIT_SIZE: f32 = 16.0;

/// Was egui in einem Bild über die Maus weiß — als reine Daten.
///
/// `press_origin` ist der Grund, warum diese Struktur existiert: egui meldet
/// [`Response::drag_started`] erst, wenn der Zeiger die Klickschwelle
/// überschritten hat (`InputOptions::max_click_dist`, 6 pt). Zu diesem
/// Zeitpunkt steht [`Response::interact_pointer_pos`] schon einige Punkte
/// weiter in Ziehrichtung — der Anfang des Rechtecks wanderte damit je nach
/// Ziehrichtung mit. Der Druckpunkt kommt deshalb aus
/// [`egui::PointerState::press_origin`].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PointerFrame {
    /// egui hat den Zug in diesem Bild erkannt.
    pub drag_started: bool,
    /// Es wird gezogen.
    pub dragged: bool,
    /// Die Taste wurde in diesem Bild losgelassen.
    pub drag_stopped: bool,
    /// Aktuelle Zeigerposition.
    pub pos: Option<Pos2>,
    /// Wo die Maustaste gedrückt wurde. `None`, sobald sie losgelassen ist.
    pub press_origin: Option<Pos2>,
}

impl PointerFrame {
    /// Liest die fünf Angaben aus egui.
    ///
    /// In egui 0.29 heißt die Methode [`Response::drag_stopped`];
    /// `drag_released` ist entfernt.
    pub fn from_response(response: &Response) -> Self {
        Self {
            drag_started: response.drag_started(),
            dragged: response.dragged(),
            drag_stopped: response.drag_stopped(),
            pos: response.interact_pointer_pos(),
            press_origin: response.ctx.input(|i| i.pointer.press_origin()),
        }
    }

    /// Der Punkt, an dem die Taste gedrückt wurde — notfalls die aktuelle
    /// Position, wenn egui den Druckpunkt nicht (mehr) kennt.
    pub fn press_point(&self) -> Option<Pos2> {
        self.press_origin.or(self.pos)
    }
}

// ---------------------------------------------------------------------------
// Eckgriffe
// ---------------------------------------------------------------------------

/// Eine Ecke des Rechtecks **auf dem Bildschirm**.
///
/// Ausdrücklich in Bildschirmkoordinaten und nicht im PDF-User-Space: auf einer
/// gedrehten Seite (`/Rotate`) ist „links oben“ auf dem Bildschirm je nach
/// Drehung eine andere Ecke im User-Space. Wer den Griff anfasst, meint immer
/// die Bildschirmecke; die Umrechnung macht [`crate::viewer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Alle vier Griffe in fester Reihenfolge.
pub const HANDLES: [Handle; 4] = [
    Handle::TopLeft,
    Handle::TopRight,
    Handle::BottomLeft,
    Handle::BottomRight,
];

impl Handle {
    /// Wo dieser Griff am Bildschirmrechteck sitzt.
    pub fn pos(self, rect: egui::Rect) -> Pos2 {
        match self {
            Handle::TopLeft => rect.left_top(),
            Handle::TopRight => rect.right_top(),
            Handle::BottomLeft => rect.left_bottom(),
            Handle::BottomRight => rect.right_bottom(),
        }
    }

    /// Die Ecke, die beim Ziehen festbleibt.
    pub fn opposite(self) -> Handle {
        match self {
            Handle::TopLeft => Handle::BottomRight,
            Handle::TopRight => Handle::BottomLeft,
            Handle::BottomLeft => Handle::TopRight,
            Handle::BottomRight => Handle::TopLeft,
        }
    }

    /// Mauszeiger, der die Ziehrichtung dieser Ecke anzeigt.
    pub fn cursor_icon(self) -> CursorIcon {
        match self {
            Handle::TopLeft | Handle::BottomRight => CursorIcon::ResizeNwSe,
            Handle::TopRight | Handle::BottomLeft => CursorIcon::ResizeNeSw,
        }
    }
}

/// Welcher Eckgriff liegt unter `pointer`?
///
/// Die Zone ist kreisförmig um die Ecke; bei einem sehr kleinen Rechteck
/// überlappen sich die vier Zonen, dann gewinnt die nächstgelegene Ecke.
pub fn hit_handle(rect: egui::Rect, pointer: Pos2) -> Option<Handle> {
    let reach = HANDLE_HIT_SIZE / 2.0;
    HANDLES
        .iter()
        .copied()
        .map(|handle| (handle, handle.pos(rect).distance(pointer)))
        .filter(|(_, distance)| *distance <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(handle, _)| handle)
}

/// Eine laufende Größenänderung an einem Eckgriff.
///
/// Genauso winzig wie [`RectangleSelector`]: welche Region, und welcher Punkt
/// bleibt stehen. Der festgehaltene Punkt wird im **PDF-User-Space** gemerkt
/// und nicht auf dem Bildschirm — dann übersteht der Ziehvorgang auch Rollen
/// und Zoomen mitten in der Bewegung.
///
/// **Die Region wird über ihre Kennung gemerkt, nicht über ihren Platz in der
/// Liste.** Ein Zug läuft über viele Bilder, und dazwischen kann die
/// Trefferliste sich ändern: Entf löscht einen Eintrag, Strg+Z tauscht die
/// ganze Liste, ein geladenes Review ersetzt sie. Ein gemerkter Index zeigte
/// danach auf eine **andere** Region — und die sprang lautlos auf das
/// Ziehrechteck, während ihre eigentliche Fläche ungeschwärzt blieb (Befund
/// #65). Eine Kennung kann das nicht: sie zeigt entweder auf dieselbe Region
/// oder auf gar keine, und „auf gar keine“ beendet den Zug
/// ([`crate::app::RedactApp`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HandleDrag {
    /// Kennung der Region — siehe [`crate::state::RegionId`].
    pub region: RegionId,
    /// Der gegenüberliegende Eckpunkt, der festbleibt.
    pub anchor: Point,
}

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

    /// Verarbeitet einen Bildzustand der Maus.
    ///
    /// Gibt beim Loslassen die beiden Eckpunkte zurück.
    ///
    /// **Der Anfang liegt am Druckpunkt**, nicht dort, wo egui den Zug bemerkt
    /// hat: [`PointerFrame::press_point`] liefert `press_origin`. Vorher stand
    /// hier die Position aus `interact_pointer_pos()` zum Zeitpunkt von
    /// `drag_started()` — die liegt immer schon jenseits der Klickschwelle
    /// (6 pt) und wandert mit der Ziehrichtung mit. Ein Rechteck begann damit
    /// nie da, wo geklickt wurde.
    pub fn step(&mut self, frame: PointerFrame) -> Option<(Pos2, Pos2)> {
        if frame.drag_started {
            if let Some(press) = frame.press_point() {
                self.begin(press);
            }
            // Die aktuelle Position ist schon bekannt — ohne sie wäre das
            // Rechteck im ersten Bild ein Punkt.
            if let Some(pos) = frame.pos {
                self.update(pos);
            }
        } else if frame.dragged {
            if let Some(pos) = frame.pos {
                self.update(pos);
            }
        } else if frame.drag_stopped {
            // Beim Loslassen ist `interact_pointer_pos` noch gültig; ohne
            // diesen letzten Schritt endete das Rechteck ein Bild zu früh.
            if let Some(pos) = frame.pos {
                self.update(pos);
            }
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

    /// Ein Bildzustand, wie ihn egui beim Erkennen eines Zuges liefert.
    fn started(press: Pos2, pos: Pos2) -> PointerFrame {
        PointerFrame {
            drag_started: true,
            dragged: true,
            pos: Some(pos),
            press_origin: Some(press),
            ..PointerFrame::default()
        }
    }

    /// **Fehler 1.** egui meldet `drag_started()` erst, wenn der Zeiger die
    /// Klickschwelle (6 pt) überschritten hat. Wer dann `interact_pointer_pos()`
    /// nimmt, beginnt das Rechteck einige Punkte neben der Maus — und zwar in
    /// Ziehrichtung, der Anfang wandert also mit.
    #[test]
    fn a_rectangle_begins_at_the_press_point_not_where_egui_noticed_the_drag() {
        let press = Pos2::new(100.0, 100.0);

        // Vier Ziehrichtungen; die Stelle, an der egui den Zug bemerkt, liegt
        // jedes Mal woanders.
        for noticed in [
            Pos2::new(107.0, 103.0),
            Pos2::new(93.0, 97.0),
            Pos2::new(104.0, 92.0),
            Pos2::new(96.0, 108.0),
        ] {
            let mut sel = RectangleSelector::new();
            sel.step(started(press, noticed));

            let preview = sel.preview().expect("Vorschau ab dem ersten Bild");
            assert!(
                preview.contains(press),
                "Der Druckpunkt muss eine Ecke sein, nicht innen oder außen"
            );
            assert_eq!(
                egui::Rect::from_two_pos(press, noticed),
                preview,
                "Rechteck spannt vom Druckpunkt bis zur aktuellen Position"
            );

            // Losgelassen wird weit rechts unten.
            let released = Pos2::new(300.0, 260.0);
            let (a, b) = sel
                .step(PointerFrame {
                    drag_stopped: true,
                    pos: Some(released),
                    ..PointerFrame::default()
                })
                .expect("Rechteck beim Loslassen");
            assert_eq!(a, press, "Anfang bleibt exakt der Druckpunkt");
            assert_eq!(b, released, "Ende ist die Stelle des Loslassens");

            // Gegenprobe: die alte Logik nahm bei `drag_started()` die
            // *aktuelle* Position — also `noticed`. Sie kommt bei jeder der
            // vier Ziehrichtungen auf ein anderes Rechteck als das richtige.
            let with_the_old_logic = egui::Rect::from_two_pos(noticed, released);
            assert_ne!(
                with_the_old_logic,
                egui::Rect::from_two_pos(a, b),
                "Gegenprobe muss sich unterscheiden, sonst prüft dieser Test nichts"
            );
            let old_corners = [
                with_the_old_logic.left_top(),
                with_the_old_logic.right_top(),
                with_the_old_logic.left_bottom(),
                with_the_old_logic.right_bottom(),
            ];
            assert!(
                !old_corners.contains(&press),
                "mit der alten Logik ist der Druckpunkt keine Ecke des Rechtecks"
            );
        }
    }

    /// Kennt egui den Druckpunkt nicht (Touch, verlorener Zeiger), bleibt es
    /// bei der bekannten Position — besser als gar kein Rechteck.
    #[test]
    fn without_a_press_origin_the_current_position_is_used() {
        let mut sel = RectangleSelector::new();
        sel.step(PointerFrame {
            drag_started: true,
            dragged: true,
            pos: Some(Pos2::new(20.0, 30.0)),
            press_origin: None,
            ..PointerFrame::default()
        });
        assert_eq!(sel.preview().unwrap().min, Pos2::new(20.0, 30.0));
    }

    // ------------------------------------------------------------- Eckgriffe

    /// **Fehler 2, erster Teil.** Jede Ecke hat ihre eigene Zone, und die ist
    /// größer als das gezeichnete Quadrat — sonst trifft man sie nicht.
    #[test]
    fn every_corner_has_its_own_reachable_zone() {
        let rect = egui::Rect::from_min_max(Pos2::new(100.0, 200.0), Pos2::new(300.0, 260.0));

        for handle in HANDLES {
            let corner = handle.pos(rect);
            assert_eq!(hit_handle(rect, corner), Some(handle), "genau auf der Ecke");
            // Reichweite: knapp innerhalb und knapp außerhalb.
            let reach = HANDLE_HIT_SIZE / 2.0;
            for offset in [
                egui::Vec2::new(reach - 1.0, 0.0),
                egui::Vec2::new(0.0, 1.0 - reach),
                egui::Vec2::new(-(reach - 1.0), 0.0),
            ] {
                assert_eq!(
                    hit_handle(rect, corner + offset),
                    Some(handle),
                    "{handle:?} bei Versatz {offset:?}"
                );
            }
            assert_eq!(
                hit_handle(rect, corner + egui::Vec2::new(reach + 1.0, reach + 1.0)),
                None,
                "{handle:?}: außerhalb der Zone"
            );
        }

        // Die Zone ist großzügiger als die Zeichnung: ein Punkt, der das
        // gemalte Quadrat (Kantenlänge `viewer::HANDLE_SIZE`, also ±2,5 pt um
        // die Ecke) klar verfehlt, trifft den Griff trotzdem.
        let just_outside_the_drawing =
            rect.left_top() + egui::Vec2::splat(crate::viewer::HANDLE_SIZE);
        assert_eq!(
            hit_handle(rect, just_outside_the_drawing),
            Some(Handle::TopLeft)
        );

        // Mitte und weit draußen sind keine Griffe.
        assert_eq!(hit_handle(rect, rect.center()), None);
        assert_eq!(hit_handle(rect, Pos2::new(1000.0, 1000.0)), None);
    }

    /// Gegenecke und Mauszeiger gehören zusammen: die feste Ecke liegt
    /// diagonal gegenüber, das Symbol zeigt dieselbe Diagonale.
    #[test]
    fn each_handle_knows_its_anchor_and_its_cursor() {
        let rect = egui::Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(10.0, 20.0));
        for handle in HANDLES {
            assert_eq!(handle.opposite().opposite(), handle);
            assert_ne!(handle.opposite(), handle);
            // Gegenüber heißt: beide Koordinaten unterscheiden sich.
            let a = handle.pos(rect);
            let b = handle.opposite().pos(rect);
            assert!(a.x != b.x && a.y != b.y, "{handle:?} liegt nicht diagonal");
            assert_eq!(handle.cursor_icon(), handle.opposite().cursor_icon());
        }
        assert_eq!(Handle::TopLeft.cursor_icon(), CursorIcon::ResizeNwSe);
        assert_eq!(Handle::TopRight.cursor_icon(), CursorIcon::ResizeNeSw);
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
