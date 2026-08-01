//! Seitendarstellung und Koordinatenumrechnung.
//!
//! ## Zwei Bilder derselben Seite
//!
//! Der Hauptbereich zeigt das **gerasterte** Seitenbild aus `redact-render`
//! (siehe [`crate::render`]). Kann eine Seite nicht rasterisiert werden —
//! `RenderedPage::degraded` —, springt die **schematische** Vorschau
//! [`PagePreview`] ein: sie zeichnet ein weißes Blatt und setzt darauf die von
//! `redact-pdf` extrahierten [`TextRun`]s an ihren echten User-Space-
//! Koordinaten neu. Das ist kein hübsches, aber ein koordinatentreues Bild —
//! man sieht wenigstens, wo Text steht, und kann Rechtecke platzieren.
//!
//! Dieselbe Notlösung wird auch gezeigt, solange das Seitenbild noch im
//! Hintergrund gerendert wird.
//!
//! ## Koordinaten
//!
//! Es gibt **drei** Räume, und der mittlere ist der Grund, warum diese Datei
//! existiert:
//!
//! 1. **User-Space** — der Raum, in dem alle [`redact_core::Region`]s liegen.
//!    Ursprung links unten der MediaBox, Y wächst nach oben, `/Rotate` ist
//!    darin *nicht* enthalten.
//! 2. **Anzeigeraum** — der User-Space nach Anwendung von `/Rotate`. Genau
//!    diese Fläche zeigt das gerasterte Bild; `RenderedPage::page_box` ist ihr
//!    Rechteck.
//! 3. **Bildschirm** — egui, Ursprung links oben, Y wächst nach unten.
//!
//! [`PageView`] kennt MediaBox *und* Drehung und rechnet zwischen 1 und 2 um;
//! [`pdf_to_screen`] und [`screen_to_pdf`] hängen 2 → 3 an. Ohne Schritt 1 → 2
//! läge auf einer um 90° gedrehten Seite jedes Schwärzungsrechteck an der
//! falschen Stelle — und würde beim Export die falsche Zeile schwärzen.
//!
//! Alle Funktionen hier sind rein und paarweise zueinander invers; genau das
//! prüfen die Tests am Ende der Datei für alle vier Drehungen.

use egui::{Align2, Color32, FontId, Pos2, Stroke, Vec2};
use redact_core::{Point, Rect, TextRun};

/// Kleinste Schriftgröße in Bildschirmpunkten — darunter ist Text unlesbar
/// und egui erzeugt nur noch Matsch.
pub const MIN_FONT_SIZE: f32 = 3.0;
/// Größte Schriftgröße; verhindert absurde Werte bei kaputten Glyph-Boxen.
pub const MAX_FONT_SIZE: f32 = 200.0;

// Maße fürs Zeichnen.
//
// Alle Werte sind **ausdrücklich** als `f32` deklariert. egui nimmt Rundungen
// und Strichstärken als `impl Into<Rounding>` bzw. `impl Into<f32>` entgegen;
// dort kann ein nacktes Fließkomma-Literal nicht auf `f64` zurückfallen, und
// neuere rustc-Versionen warnen darüber (`float_literal_f32_fallback`).
// Benannte Konstanten mit Typ vermeiden das ein für alle Mal.

/// Eckenrundung des Seitenblatts.
pub const SHEET_ROUNDING: f32 = 2.0;
/// Schwärzungsrechtecke bekommen scharfe Ecken.
pub const NO_ROUNDING: f32 = 0.0;
/// Strichstärke der Blattkante.
pub const SHEET_STROKE: f32 = 1.0;
/// Strichstärke einer nicht ausgewählten Region.
pub const REGION_STROKE: f32 = 1.0;
/// Strichstärke der ausgewählten Region.
pub const SELECTED_STROKE: f32 = 2.5;
/// Strichstärke des Rechtecks, das gerade aufgezogen wird.
pub const DRAG_STROKE: f32 = 1.5;
/// Kantenlänge der **gezeichneten** Griffpunkte an den Ecken der Auswahl.
///
/// Angefasst werden sie über die deutlich größere Zone
/// [`crate::selector::HANDLE_HIT_SIZE`] — 5 pt trifft man mit der Maus nicht.
pub const HANDLE_SIZE: f32 = 5.0;
/// Versatz des Schlagschattens unter dem Blatt.
pub const SHADOW_OFFSET: f32 = 4.0;

// ---------------------------------------------------------------------------
// PageView — MediaBox und /Rotate
// ---------------------------------------------------------------------------

/// Geometrie einer Seite: MediaBox **und** Drehung.
///
/// Enthält bewusst keinen egui-Typ, damit die Umrechnung ohne Fenster prüfbar
/// bleibt. Die Formeln sind exakt die Umkehrung dessen, was
/// `redact_render::raster::Geometry` beim Rastern tut — [`PageView::display_box`]
/// liefert deshalb dasselbe Rechteck wie `RenderedPage::page_box`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageView {
    /// MediaBox im User-Space (normalisiert, ungedreht).
    pub media_box: Rect,
    /// Seitendrehung in Grad, bereits auf 0/90/180/270 normalisiert.
    pub rotate: i64,
}

impl PageView {
    /// `rotate` wird auf 0/90/180/270 normalisiert; alles andere wird zu 0.
    pub fn new(media_box: Rect, rotate: i64) -> Self {
        Self {
            media_box: media_box.normalized(),
            rotate: normalize_rotation(rotate),
        }
    }

    /// Ungedrehte Seite.
    pub fn upright(media_box: Rect) -> Self {
        Self::new(media_box, 0)
    }

    /// Sind Breite und Höhe durch die Drehung vertauscht?
    pub fn is_quarter_turned(self) -> bool {
        self.rotate == 90 || self.rotate == 270
    }

    /// Das Rechteck, das das gerasterte Bild abdeckt (Anzeigeraum).
    ///
    /// Die linke untere Ecke ist die der MediaBox; bei 90° und 270° sind Breite
    /// und Höhe getauscht. Deckungsgleich mit `RenderedPage::page_box`.
    pub fn display_box(self) -> Rect {
        let mb = self.media_box;
        if self.is_quarter_turned() {
            Rect::new(
                mb.ll.x,
                mb.ll.y,
                mb.ll.x + mb.height(),
                mb.ll.y + mb.width(),
            )
        } else {
            mb
        }
    }

    /// User-Space → Anzeigeraum.
    pub fn user_to_display(self, p: Point) -> Point {
        let mb = self.media_box;
        let (x0, y0, x1, y1) = (mb.ll.x, mb.ll.y, mb.ur.x, mb.ur.y);
        match self.rotate {
            90 => Point::new(x0 + (p.y - y0), y0 + (x1 - p.x)),
            180 => Point::new(x0 + x1 - p.x, y0 + y1 - p.y),
            270 => Point::new(x0 + (y1 - p.y), y0 + (p.x - x0)),
            _ => p,
        }
    }

    /// Anzeigeraum → User-Space (Umkehrung von [`PageView::user_to_display`]).
    pub fn display_to_user(self, p: Point) -> Point {
        let mb = self.media_box;
        let (x0, y0, x1, y1) = (mb.ll.x, mb.ll.y, mb.ur.x, mb.ur.y);
        match self.rotate {
            90 => Point::new(x1 - (p.y - y0), y0 + (p.x - x0)),
            180 => Point::new(x0 + x1 - p.x, y0 + y1 - p.y),
            270 => Point::new(x0 + (p.y - y0), y1 - (p.x - x0)),
            _ => p,
        }
    }

    /// Größe des Blatts auf dem Bildschirm.
    pub fn size_screen(self, zoom: f32) -> Vec2 {
        page_size_screen(&self.display_box(), zoom)
    }
}

/// Drehung auf 0/90/180/270 bringen; krumme Werte bedeuten „keine Drehung“.
///
/// Gleiche Regel wie in `redact_render` — sonst zeigten Bild und Rechtecke
/// unterschiedliche Vorstellungen von „oben“.
pub fn normalize_rotation(rotate: i64) -> i64 {
    let normalized = rotate.rem_euclid(360);
    if normalized % 90 == 0 {
        normalized
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// Umrechnung Anzeigeraum ↔ Bildschirm
// ---------------------------------------------------------------------------

/// Rechnet ein PDF-Rechteck (User-Space) in Bildschirmkoordinaten um.
///
/// `origin` ist die linke **obere** Ecke des Seitenblatts auf dem Bildschirm.
/// Eine Drehung um 90° bildet gegenüberliegende Ecken wieder auf
/// gegenüberliegende Ecken ab, deshalb genügen die beiden Eckpunkte.
pub fn pdf_to_screen(rect: &Rect, view: &PageView, zoom: f32, origin: Pos2) -> egui::Rect {
    let a = pdf_point_to_screen(rect.ll.x, rect.ll.y, view, zoom, origin);
    let b = pdf_point_to_screen(rect.ur.x, rect.ur.y, view, zoom, origin);
    egui::Rect::from_two_pos(a, b)
}

/// Rechnet zwei Bildschirmpunkte in ein normalisiertes PDF-Rechteck um.
pub fn screen_to_pdf(a: Pos2, b: Pos2, view: &PageView, zoom: f32, origin: Pos2) -> Rect {
    Rect::from_corners(
        screen_to_pdf_point(a, view, zoom, origin),
        screen_to_pdf_point(b, view, zoom, origin),
    )
}

/// Ein einzelner PDF-Punkt (User-Space) → Bildschirm.
pub fn pdf_point_to_screen(x: f64, y: f64, view: &PageView, zoom: f32, origin: Pos2) -> Pos2 {
    let d = view.user_to_display(Point::new(x, y));
    let page_box = view.display_box();
    let z = zoom as f64;
    Pos2::new(
        origin.x + ((d.x - page_box.ll.x) * z) as f32,
        // Y-Spiegelung: die Oberkante des Anzeigeraums liegt auf `origin.y`.
        origin.y + ((page_box.ur.y - d.y) * z) as f32,
    )
}

/// Ein einzelner Bildschirmpunkt → PDF-User-Space.
pub fn screen_to_pdf_point(p: Pos2, view: &PageView, zoom: f32, origin: Pos2) -> Point {
    let page_box = view.display_box();
    let z = (zoom as f64).max(f64::EPSILON);
    let d = Point::new(
        page_box.ll.x + (p.x - origin.x) as f64 / z,
        page_box.ur.y - (p.y - origin.y) as f64 / z,
    );
    view.display_to_user(d)
}

/// Größe eines Anzeigerechtecks auf dem Bildschirm.
pub fn page_size_screen(page_box: &Rect, zoom: f32) -> Vec2 {
    Vec2::new(
        (page_box.width() as f32 * zoom).max(1.0),
        (page_box.height() as f32 * zoom).max(1.0),
    )
}

/// Zoomfaktor, bei dem die Seite vollständig in `available` passt.
pub fn fit_zoom(available: Vec2, view: &PageView) -> f32 {
    let page_box = view.display_box();
    let w = page_box.width() as f32;
    let h = page_box.height() as f32;
    if w <= 0.0 || h <= 0.0 {
        return 1.0;
    }
    (available.x / w)
        .min(available.y / h)
        .clamp(crate::state::MIN_ZOOM, crate::state::MAX_ZOOM)
}

/// Schriftgröße für einen Text-Run aus der Höhe seiner Glyph-Box.
///
/// Die Glyph-Box umfasst nur die tatsächlich gesetzten Zeichen, ist also etwas
/// kleiner als die nominelle Schriftgröße. Der Faktor gleicht das grob aus.
pub fn font_size_for(run_height: f64, zoom: f32) -> f32 {
    const CAP_HEIGHT_RATIO: f32 = 1.18;
    ((run_height as f32) * zoom * CAP_HEIGHT_RATIO).clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)
}

/// Verkleinert die Schriftgröße so weit, dass der gemessene Text in die
/// Zielbreite passt. Vergrößert nie.
pub fn shrink_to_width(font_size: f32, measured_width: f32, target_width: f32) -> f32 {
    if measured_width <= 0.0 || target_width <= 0.0 || measured_width <= target_width {
        return font_size;
    }
    (font_size * (target_width / measured_width)).max(MIN_FONT_SIZE)
}

// ---------------------------------------------------------------------------
// Schematische Vorschau (Notnagel)
// ---------------------------------------------------------------------------

/// Schematische Vorschau einer einzelnen Seite.
///
/// Hält nur Referenzen — der Zustand lebt in [`crate::state::AppState`].
///
/// **Grenzen**: Schriftart, Laufweite und Kerning entsprechen nicht dem
/// Original, Grafiken und Bilder fehlen, und auf gedrehten Seiten steht der
/// Text zwar an der richtigen Stelle, aber weiter waagerecht. Das ist bewusst
/// so: die Vorschau soll die Textlage zeigen, nicht das Dokument ersetzen.
pub struct PagePreview<'a> {
    pub page: usize,
    pub view: PageView,
    pub runs: &'a [TextRun],
    pub zoom: f32,
}

impl<'a> PagePreview<'a> {
    pub fn new(page: usize, view: PageView, runs: &'a [TextRun], zoom: f32) -> Self {
        Self {
            page,
            view,
            runs,
            zoom,
        }
    }

    /// Größe des Blatts auf dem Bildschirm.
    pub fn size(&self) -> Vec2 {
        self.view.size_screen(self.zoom)
    }

    /// Zeichnet Blatt, Schatten und Text. `origin` ist die linke obere Ecke.
    pub fn paint(&self, painter: &egui::Painter, origin: Pos2) {
        self.paint_sheet(painter, origin);
        self.paint_text(painter, origin);
    }

    /// Nur das leere Blatt mit Schatten und Kante.
    ///
    /// Getrennt vom Text, weil unter dem gerasterten Seitenbild derselbe
    /// Schatten liegen soll, der Text darunter aber nicht.
    pub fn paint_sheet(&self, painter: &egui::Painter, origin: Pos2) {
        let sheet = egui::Rect::from_min_size(origin, self.size());
        painter.rect_filled(
            sheet.translate(Vec2::splat(SHADOW_OFFSET)),
            SHEET_ROUNDING,
            Color32::from_black_alpha(48),
        );
        painter.rect(
            sheet,
            SHEET_ROUNDING,
            Color32::WHITE,
            Stroke::new(SHEET_STROKE, Color32::from_gray(150)),
        );
    }

    /// Nur die Textzeilen.
    pub fn paint_text(&self, painter: &egui::Painter, origin: Pos2) {
        for run in self.runs.iter().filter(|r| r.page == self.page) {
            self.paint_run(painter, origin, run);
        }
    }

    fn paint_run(&self, painter: &egui::Painter, origin: Pos2, run: &TextRun) {
        let text = run.text.trim_end();
        if text.is_empty() {
            return;
        }
        let target = pdf_to_screen(&run.rect, &self.view, self.zoom, origin);
        let mut size = font_size_for(run.rect.height(), self.zoom);
        if size <= MIN_FONT_SIZE {
            // Zu klein für Text — als graue Linie andeuten, damit man sieht,
            // dass dort etwas steht.
            painter.rect_filled(target, NO_ROUNDING, Color32::from_gray(190));
            return;
        }

        let color = Color32::from_gray(35);
        let mut galley = painter.layout_no_wrap(text.to_owned(), FontId::proportional(size), color);
        let measured = galley.size().x;
        let fitted = shrink_to_width(size, measured, target.width());
        if fitted < size {
            size = fitted;
            galley = painter.layout_no_wrap(text.to_owned(), FontId::proportional(size), color);
        }

        // Unterkante der Glyph-Box als Grundlinie verwenden.
        let anchor = Pos2::new(target.left(), target.bottom());
        let pos = Align2::LEFT_BOTTOM.anchor_size(anchor, galley.size()).min;
        painter.galley(pos, galley, color);
    }
}

/// Wie ein Rechteck im Seitenbild aussieht.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionStyle {
    /// Wird geschwärzt: halbtransparent gefüllt, durchgezogener Rand.
    Redacted,
    /// Aktiv und wirksam, aber keine Schwärzung (Schutzbereich): nur Rand.
    Outlined,
    /// Zählt nicht mit (abgewählt, blockiert, doppelt): gestrichelter Rand.
    Discarded,
}

impl RegionStyle {
    /// Ableitung aus dem Ergebnis der Konfliktauflösung.
    pub fn from_outcome(outcome: crate::state::HitOutcome) -> Self {
        use crate::state::HitOutcome::*;
        match outcome {
            Redacted => RegionStyle::Redacted,
            Protecting => RegionStyle::Outlined,
            Disabled | Blocked | Duplicate => RegionStyle::Discarded,
        }
    }
}

/// Strichstärke des Polsterungsrahmens.
pub const PADDING_STROKE: f32 = 1.0;
/// Strichlänge des Polsterungsrahmens.
pub const PADDING_DASH: f32 = 3.0;
/// Lückenlänge des Polsterungsrahmens.
pub const PADDING_GAP: f32 = 3.0;

/// Das Rechteck, das beim Export wirklich schwarz wird.
///
/// `--padding` vergrößert **jede** Schwärzung auf jeder Seite um diesen Betrag
/// (`redact_core::Rect::expanded`, angewandt in `redact_pipeline::run`). Auf
/// dem Bildschirm sind das `padding * zoom` Punkte.
///
/// `None`, wenn nichts zu zeigen ist: ohne Polsterung, oder wenn eine negative
/// Polsterung vom Rechteck nichts übrig lässt.
pub fn padded_rect(rect: egui::Rect, padding_screen: f32) -> Option<egui::Rect> {
    if padding_screen == 0.0 {
        return None;
    }
    let padded = rect.expand(padding_screen);
    (padded.width() > 0.0 && padded.height() > 0.0).then_some(padded)
}

/// Die Polsterung in Bildschirmpunkten.
pub fn padding_screen(padding: f64, zoom: f32) -> f32 {
    padding as f32 * zoom
}

/// Zeichnet, wie weit die Schwärzung über das Rechteck hinausgeht.
///
/// **Warum es das gibt**: `--padding 6` heißt, dass der schwarze Balken auf
/// jeder Seite 6 pt größer wird als das gezeichnete Rechteck. Nachbartext
/// darin verschwindet mit — und im Bild war davon nichts zu sehen. Auch bei der
/// Vorgabe 1,0 stimmten Anzeige und Wirkung nie ganz überein.
///
/// Gezeichnet wird ein gestrichelter Rahmen um die wirkliche Fläche plus eine
/// sehr schwache Füllung; beides in der Farbe der Region, damit die Zuordnung
/// eindeutig bleibt.
pub fn paint_padding(painter: &egui::Painter, rect: egui::Rect, rgb: (u8, u8, u8), padding: f32) {
    let Some(padded) = padded_rect(rect, padding) else {
        return;
    };
    let (r, g, b) = rgb;
    painter.rect_filled(
        padded,
        NO_ROUNDING,
        Color32::from_rgba_unmultiplied(r, g, b, 30),
    );
    let stroke = Stroke::new(
        PADDING_STROKE,
        Color32::from_rgba_unmultiplied(r, g, b, 160),
    );
    for edge in [
        [padded.left_top(), padded.right_top()],
        [padded.right_top(), padded.right_bottom()],
        [padded.right_bottom(), padded.left_bottom()],
        [padded.left_bottom(), padded.left_top()],
    ] {
        painter.extend(egui::Shape::dashed_line(
            &edge,
            stroke,
            PADDING_DASH,
            PADDING_GAP,
        ));
    }
}

/// Zeichnet ein Schwärzungsrechteck.
///
/// Gefüllt wird **nur**, was am Ende wirklich geschwärzt wird. Ein bloß
/// angehakter Treffer, den die Konfliktauflösung verwirft, bekommt einen
/// gestrichelten Rand; sonst sähe im Seitenbild etwas nach Schwärzung aus, das
/// keine ist.
pub fn paint_region(
    painter: &egui::Painter,
    rect: egui::Rect,
    rgb: (u8, u8, u8),
    style: RegionStyle,
    selected: bool,
) {
    let (r, g, b) = rgb;
    let color = Color32::from_rgb(r, g, b);
    if style == RegionStyle::Redacted {
        painter.rect_filled(
            rect,
            NO_ROUNDING,
            Color32::from_rgba_unmultiplied(r, g, b, 70),
        );
    }
    let width: f32 = if selected {
        SELECTED_STROKE
    } else {
        REGION_STROKE
    };
    let stroke = Stroke::new(width, color);
    if style != RegionStyle::Discarded {
        painter.rect_stroke(rect, NO_ROUNDING, stroke);
    } else {
        // Gestrichelt = „zählt nicht mit“.
        for edge in [
            [rect.left_top(), rect.right_top()],
            [rect.right_top(), rect.right_bottom()],
            [rect.right_bottom(), rect.left_bottom()],
            [rect.left_bottom(), rect.left_top()],
        ] {
            painter.extend(egui::Shape::dashed_line(&edge, stroke, 4.0_f32, 4.0_f32));
        }
    }
    if selected {
        // Griffpunkte an den Ecken der Auswahl. Sie sind ein Versprechen: an
        // ihnen lässt sich ziehen (siehe [`crate::selector::hit_handle`] und
        // `RedactApp::apply_pointer`). Wer sie zeichnet, ohne sie abzugreifen,
        // baut eine Zusage ein, welche die Oberfläche nicht einlöst.
        for corner in [
            rect.left_top(),
            rect.right_top(),
            rect.left_bottom(),
            rect.right_bottom(),
        ] {
            painter.rect_filled(
                egui::Rect::from_center_size(corner, Vec2::splat(HANDLE_SIZE)),
                NO_ROUNDING,
                color,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MediaBox, deren linke untere Ecke *nicht* im Ursprung liegt.
    fn offset_box() -> Rect {
        Rect::new(10.0, 20.0, 605.0, 862.0)
    }

    fn assert_close(a: f64, b: f64, what: &str) {
        assert!((a - b).abs() < 0.01, "{what}: {a} != {b}");
    }

    fn upright() -> PageView {
        PageView::upright(offset_box())
    }

    #[test]
    fn roundtrip_survives_several_rects_zooms_and_offsets() {
        let boxes = [Rect::new(0.0, 0.0, 595.0, 842.0), offset_box()];
        let rects = [
            Rect::new(72.0, 700.0, 300.0, 715.0),
            Rect::new(10.0, 20.0, 11.0, 21.0),
            Rect::new(500.0, 100.0, 590.0, 800.0),
            Rect::new(123.5, 456.25, 124.75, 457.5),
        ];
        let origins = [Pos2::new(0.0, 0.0), Pos2::new(37.0, 91.0)];

        for page_box in boxes {
            for zoom in [0.5_f32, 1.0_f32, 2.5_f32] {
                for origin in origins {
                    for rect in rects {
                        let view = PageView::upright(page_box);
                        let screen = pdf_to_screen(&rect, &view, zoom, origin);
                        let back = screen_to_pdf(screen.min, screen.max, &view, zoom, origin);
                        assert_close(back.ll.x, rect.ll.x, "ll.x");
                        assert_close(back.ll.y, rect.ll.y, "ll.y");
                        assert_close(back.ur.x, rect.ur.x, "ur.x");
                        assert_close(back.ur.y, rect.ur.y, "ur.y");
                    }
                }
            }
        }
    }

    /// Der gefährlichste Teil der ganzen Oberfläche: liegt ein Rechteck auf
    /// einer gedrehten Seite falsch, wird beim Export die falsche Stelle
    /// geschwärzt. Hin- und Rückweg müssen für **alle vier** Drehungen und für
    /// eine versetzte MediaBox deckungsgleich sein.
    #[test]
    fn roundtrip_is_exact_for_every_rotation_and_an_offset_media_box() {
        let boxes = [
            Rect::new(0.0, 0.0, 595.0, 842.0),
            offset_box(),
            // Auch der Fall „breiter als hoch“ muss stimmen.
            Rect::new(-30.0, -15.0, 842.0, 595.0),
        ];
        let rects = [
            Rect::new(72.0, 700.0, 300.0, 715.0),
            Rect::new(100.0, 100.0, 101.0, 101.0),
            Rect::new(0.0, 0.0, 400.0, 500.0),
        ];
        let origins = [Pos2::new(0.0, 0.0), Pos2::new(37.0, 91.0)];

        for page_box in boxes {
            for rotate in [0_i64, 90, 180, 270] {
                let view = PageView::new(page_box, rotate);
                for zoom in [0.5_f32, 1.0_f32, 2.5_f32] {
                    for origin in origins {
                        for rect in rects {
                            let screen = pdf_to_screen(&rect, &view, zoom, origin);
                            let back = screen_to_pdf(screen.min, screen.max, &view, zoom, origin);
                            let what = format!("rot={rotate} zoom={zoom} box={page_box:?}");
                            assert_close(back.ll.x, rect.ll.x, &format!("ll.x {what}"));
                            assert_close(back.ll.y, rect.ll.y, &format!("ll.y {what}"));
                            assert_close(back.ur.x, rect.ur.x, &format!("ur.x {what}"));
                            assert_close(back.ur.y, rect.ur.y, &format!("ur.y {what}"));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn rotation_swaps_width_and_height_of_the_display_box() {
        let mb = offset_box(); // 595 × 842
        for rotate in [0_i64, 180, 360, 720, -180] {
            let view = PageView::new(mb, rotate);
            assert_eq!(view.display_box(), mb, "rot={rotate}");
            assert!(!view.is_quarter_turned());
        }
        for rotate in [90_i64, 270, 450, -90] {
            let view = PageView::new(mb, rotate);
            let db = view.display_box();
            assert!(view.is_quarter_turned(), "rot={rotate}");
            assert_close(db.width(), 842.0, "Breite");
            assert_close(db.height(), 595.0, "Höhe");
            // Die linke untere Ecke bleibt die der MediaBox.
            assert_eq!(db.ll, mb.ll);
        }
        // Krumme Werte gelten als „nicht gedreht“ — genau wie im Rasterizer.
        assert_eq!(PageView::new(mb, 45).rotate, 0);
    }

    /// Wo landen die vier Ecken der MediaBox auf dem Bildschirm? Für jede
    /// Drehung eine andere — und immer genau eine je Bildecke.
    #[test]
    fn rotation_moves_the_media_box_corners_as_expected() {
        let mb = Rect::new(0.0, 0.0, 600.0, 800.0);
        let origin = Pos2::ZERO;
        // Linke *obere* Ecke der ungedrehten Seite.
        let top_left_user = (0.0_f64, 800.0_f64);

        let landing = |rotate: i64| {
            let view = PageView::new(mb, rotate);
            pdf_point_to_screen(top_left_user.0, top_left_user.1, &view, 1.0, origin)
        };

        // 0°: bleibt links oben.
        assert_eq!(landing(0), Pos2::new(0.0, 0.0));
        // 90° im Uhrzeigersinn: die linke obere Ecke wandert nach rechts oben.
        assert_eq!(landing(90), Pos2::new(800.0, 0.0));
        // 180°: nach rechts unten.
        assert_eq!(landing(180), Pos2::new(600.0, 800.0));
        // 270°: nach links unten.
        assert_eq!(landing(270), Pos2::new(0.0, 600.0));

        // Und in jedem Fall füllt die ganze Seite genau das Blatt aus.
        for rotate in [0_i64, 90, 180, 270] {
            let view = PageView::new(mb, rotate);
            let screen = pdf_to_screen(&mb, &view, 1.0, origin);
            assert_eq!(screen.min, Pos2::ZERO, "rot={rotate}");
            assert_eq!(
                screen.size(),
                view.size_screen(1.0),
                "rot={rotate}: Blattgröße"
            );
        }
    }

    /// Ein schmaler Streifen am oberen Seitenrand muss auf einer um 90°
    /// gedrehten Seite als schmaler Streifen am **rechten** Bildrand landen.
    #[test]
    fn a_banner_at_the_top_ends_up_at_the_right_edge_when_turned() {
        let mb = Rect::new(0.0, 0.0, 600.0, 800.0);
        let banner = Rect::new(0.0, 780.0, 600.0, 800.0);

        let view = PageView::new(mb, 90);
        let screen = pdf_to_screen(&banner, &view, 1.0, Pos2::ZERO);
        // Anzeigeraum ist 800 × 600 groß.
        assert_close(screen.left() as f64, 780.0, "links");
        assert_close(screen.right() as f64, 800.0, "rechts");
        assert_close(screen.top() as f64, 0.0, "oben");
        assert_close(screen.bottom() as f64, 600.0, "unten");
    }

    /// Gefüllt darf nur werden, was auch geschwärzt wird — sonst sieht im
    /// Seitenbild etwas nach Schwärzung aus, das keine ist.
    #[test]
    fn only_a_real_redaction_is_drawn_filled() {
        use crate::state::HitOutcome;
        assert_eq!(
            RegionStyle::from_outcome(HitOutcome::Redacted),
            RegionStyle::Redacted
        );
        for outcome in [
            HitOutcome::Protecting,
            HitOutcome::Disabled,
            HitOutcome::Blocked,
            HitOutcome::Duplicate,
        ] {
            assert_ne!(
                RegionStyle::from_outcome(outcome),
                RegionStyle::Redacted,
                "{outcome:?} darf nicht gefüllt gezeichnet werden"
            );
        }
        // Ein Schutzbereich ist wirksam und wird darum nicht gestrichelt.
        assert_eq!(
            RegionStyle::from_outcome(HitOutcome::Protecting),
            RegionStyle::Outlined
        );
    }

    /// **Befund: die Polsterung war im Bild nicht zu sehen.** `--padding 6`
    /// macht den schwarzen Balken auf jeder Seite 6 pt größer als das
    /// gezeichnete Rechteck; Nachbartext darin verschwindet mit.
    #[test]
    fn the_padded_rectangle_is_the_one_that_really_turns_black() {
        let rect = egui::Rect::from_min_max(Pos2::new(100.0, 200.0), Pos2::new(300.0, 260.0));

        // Auf jeder Seite `padding * zoom`.
        assert_eq!(padding_screen(6.0, 1.0), 6.0);
        assert_eq!(padding_screen(6.0, 1.5), 9.0);
        let padded = padded_rect(rect, padding_screen(6.0, 1.5)).expect("gepolstert");
        assert_eq!(padded.min, Pos2::new(91.0, 191.0));
        assert_eq!(padded.max, Pos2::new(309.0, 269.0));
        assert!(padded.contains_rect(rect));

        // Auch die Vorgabe 1,0 ist ein Unterschied — sie war es vorher auch,
        // nur sah man ihn nicht.
        let default = padded_rect(rect, padding_screen(1.0, 1.0)).expect("Vorgabe");
        assert_ne!(default, rect);

        // Ohne Polsterung gibt es nichts zu zeigen …
        assert_eq!(padded_rect(rect, 0.0), None);
        // … und eine negative, die nichts übrig lässt, ebenso wenig
        // (`--padding=-100`).
        assert_eq!(padded_rect(rect, -100.0), None);
        // Eine kleine negative schrumpft dagegen sichtbar.
        let shrunk = padded_rect(rect, -5.0).expect("geschrumpft");
        assert!(rect.contains_rect(shrunk));
    }

    /// Und sie wird auch wirklich gemalt.
    #[test]
    fn the_padding_paints_something_and_nothing_without_it() {
        let rect = egui::Rect::from_min_max(Pos2::new(100.0, 200.0), Pos2::new(300.0, 260.0));
        let shapes = |padding: f32| -> usize {
            let ctx = egui::Context::default();
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    paint_padding(ui.painter(), rect, (176, 88, 0), padding);
                });
            });
            output.shapes.len()
        };
        let none = shapes(0.0);
        assert!(
            shapes(6.0) > none,
            "eine Polsterung von 6 pt muss zu sehen sein"
        );
        assert!(shapes(1.0) > none, "und die Vorgabe von 1,0 ebenso");
    }

    #[test]
    fn y_axis_is_flipped() {
        let view = upright();
        let origin = Pos2::new(0.0, 0.0);
        // Ganz oben auf der Seite (y nahe ur.y) …
        let top = Rect::new(20.0, 850.0, 100.0, 860.0);
        // … und ganz unten (y nahe ll.y).
        let bottom = Rect::new(20.0, 22.0, 100.0, 32.0);

        let top_screen = pdf_to_screen(&top, &view, 1.0, origin);
        let bottom_screen = pdf_to_screen(&bottom, &view, 1.0, origin);

        assert!(
            top_screen.min.y < bottom_screen.min.y,
            "Oben im PDF muss oben auf dem Bildschirm sein"
        );
        // Die Oberkante der MediaBox liegt genau auf `origin.y`.
        assert_close(
            top_screen.min.y as f64,
            862.0 - 860.0,
            "Abstand zur Oberkante",
        );
        assert_close(
            bottom_screen.max.y as f64,
            862.0 - 22.0,
            "Abstand Unterkante",
        );
    }

    #[test]
    fn origin_offset_and_zoom_scale_correctly() {
        let view = upright();
        let origin = Pos2::new(100.0, 50.0);
        // Die linke untere Ecke der MediaBox landet auf (origin.x, origin.y + h*zoom).
        let corner = pdf_point_to_screen(10.0, 20.0, &view, 2.0, origin);
        assert_close(corner.x as f64, 100.0, "x");
        assert_close(corner.y as f64, 50.0 + 842.0 * 2.0, "y");

        // Die linke obere Ecke landet exakt auf dem Ursprung.
        let top_left = pdf_point_to_screen(10.0, 862.0, &view, 2.0, origin);
        assert_close(top_left.x as f64, 100.0, "x");
        assert_close(top_left.y as f64, 50.0, "y");
    }

    #[test]
    fn screen_to_pdf_normalizes_dragged_corners() {
        let view = upright();
        let origin = Pos2::new(5.0, 5.0);
        // Von rechts unten nach links oben gezogen.
        let a = Pos2::new(300.0, 400.0);
        let b = Pos2::new(100.0, 200.0);
        let rect = screen_to_pdf(a, b, &view, 1.5, origin);
        assert!(rect.ll.x < rect.ur.x);
        assert!(rect.ll.y < rect.ur.y);
        assert!(rect.width() > 0.0 && rect.height() > 0.0);

        // Gleiches Ergebnis, egal in welcher Reihenfolge die Ecken kommen.
        let swapped = screen_to_pdf(b, a, &view, 1.5, origin);
        assert_eq!(rect, swapped);

        // Auch auf einer gedrehten Seite bleibt das Ergebnis normalisiert.
        let turned = PageView::new(offset_box(), 270);
        let rect = screen_to_pdf(a, b, &turned, 1.5, origin);
        assert!(rect.ll.x < rect.ur.x && rect.ll.y < rect.ur.y);
    }

    #[test]
    fn page_size_and_fit_zoom() {
        let view = upright();
        assert_eq!(view.size_screen(1.0), Vec2::new(595.0, 842.0));
        assert_eq!(view.size_screen(2.0), Vec2::new(1190.0, 1684.0));
        // Gedreht sind Breite und Höhe getauscht.
        assert_eq!(
            PageView::new(offset_box(), 90).size_screen(1.0),
            Vec2::new(842.0, 595.0)
        );

        // Passt auf halbe Höhe → Zoom ~0.5.
        let z = fit_zoom(Vec2::new(1190.0, 421.0), &view);
        assert!((z - 0.5).abs() < 1e-6, "{z}");
        // Entartete Seite → 1.0.
        assert_eq!(
            fit_zoom(
                Vec2::new(100.0, 100.0),
                &PageView::upright(Rect::new(0.0, 0.0, 0.0, 0.0))
            ),
            1.0
        );
    }

    #[test]
    fn font_size_scales_with_zoom_and_is_clamped() {
        let a = font_size_for(10.0, 1.0);
        let b = font_size_for(10.0, 2.0);
        assert!((b - 2.0 * a).abs() < 1e-4);
        assert_eq!(font_size_for(0.0, 1.0), MIN_FONT_SIZE);
        assert_eq!(font_size_for(10_000.0, 4.0), MAX_FONT_SIZE);
    }

    #[test]
    fn shrink_to_width_only_shrinks() {
        // Passt schon → unverändert.
        assert_eq!(shrink_to_width(12.0, 50.0, 100.0), 12.0);
        // Doppelt so breit → halbe Größe.
        assert!((shrink_to_width(12.0, 200.0, 100.0) - 6.0).abs() < 1e-4);
        // Entartete Eingaben ändern nichts.
        assert_eq!(shrink_to_width(12.0, 0.0, 100.0), 12.0);
        assert_eq!(shrink_to_width(12.0, 100.0, 0.0), 12.0);
        // Untergrenze wird eingehalten.
        assert_eq!(shrink_to_width(4.0, 10_000.0, 1.0), MIN_FONT_SIZE);
    }

    /// Rauchtest ohne Bildschirm: egui baut den Kontext auch headless auf, das
    /// Zeichnen selbst lässt sich damit wenigstens *ausführen*.
    #[test]
    fn preview_and_regions_paint_without_panicking() {
        let runs = vec![
            demo_run(0, "IBAN: DE89 3704 0044", 72.0, 700.0),
            demo_run(0, "   ", 72.0, 680.0),
            demo_run(1, "andere Seite", 72.0, 660.0),
        ];

        egui::__run_test_ui(|ui| {
            let origin = Pos2::new(12.0, 12.0);
            for rotate in [0_i64, 90, 180, 270] {
                let view = PageView::new(offset_box(), rotate);
                for zoom in [0.05_f32, 1.0_f32, 2.5_f32] {
                    // 0.05 erzwingt den Zweig „zu klein für Text“.
                    PagePreview::new(0, view, &runs, zoom).paint(ui.painter(), origin);
                }
                PagePreview::new(0, view, &runs, 1.0).paint_sheet(ui.painter(), origin);
            }
            let view = upright();
            let rect = pdf_to_screen(&runs[0].rect, &view, 1.0, origin);
            for style in [
                RegionStyle::Redacted,
                RegionStyle::Outlined,
                RegionStyle::Discarded,
            ] {
                paint_region(ui.painter(), rect, (60, 130, 246), style, true);
                paint_region(ui.painter(), rect, (220, 60, 60), style, false);
            }
        });
    }

    /// Ein Text-Run mit einer Glyph-Box je Zeichen (12 pt hoch, 6 pt breit).
    fn demo_run(page: usize, text: &str, x: f64, y: f64) -> TextRun {
        let glyphs = text
            .chars()
            .enumerate()
            .map(|(i, ch)| redact_core::Glyph {
                ch,
                rect: Rect::new(x + i as f64 * 6.0, y, x + (i as f64 + 1.0) * 6.0, y + 12.0),
            })
            .collect();
        TextRun::new(page, glyphs)
    }
}
