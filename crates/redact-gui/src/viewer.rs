//! Seitendarstellung und Koordinatenumrechnung.
//!
//! ## Warum kein echter Rasterizer?
//!
//! Die Vorschau kommt **ohne native PDF-Rasterisierung** aus. Statt die Seite
//! zu rendern, zeichnet [`PagePreview`] ein weißes Blatt mit Schlagschatten und
//! setzt darauf die von `redact-pdf` extrahierten [`TextRun`]s an ihren echten
//! User-Space-Koordinaten neu — mit egui-eigenen Schriften, deren Größe aus der
//! Höhe der Glyph-Box des Runs abgeleitet wird.
//!
//! Das ist ein **schematisches, aber koordinatentreues** Bild: jede Textzeile
//! steht an genau der Stelle, an der sie auch im PDF steht, und die Boxen der
//! Schwärzungen liegen darum exakt richtig. Für das eigentliche Ziel der GUI —
//! Rechtecke platzieren und Treffer kontrollieren — genügt das vollständig, und
//! es kostet keine einzige native Abhängigkeit.
//!
//! **Grenzen** (bewusst in Kauf genommen):
//!
//! * Schriftart, Laufweite und Kerning entsprechen nicht dem Original; die
//!   Zeile wird nur so weit gestaucht, dass sie in ihre Originalbreite passt.
//! * Grafiken, Bilder, Linien, Tabellenrahmen und Hintergründe fehlen komplett.
//! * Text in Vektorgrafiken oder gescannte Seiten ohne Textebene erscheinen als
//!   leeres Blatt — dort hilft nur die manuelle Rechteckauswahl.
//! * Gedrehte Seiten (`/Rotate`) werden nicht berücksichtigt.
//!
//! Wer eine pixelgenaue Vorschau braucht, baut mit `--features pdfium`; siehe
//! [`pdfium_backend`].
//!
//! ## Koordinaten
//!
//! PDF: Ursprung links **unten** der MediaBox, Y wächst nach **oben**.
//! egui: Ursprung links **oben** des Widgets, Y wächst nach **unten**.
//! Die Umrechnung erledigen [`pdf_to_screen`] und [`screen_to_pdf`]; beide sind
//! reine Funktionen und zueinander invers.

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
/// Kantenlänge der Griffpunkte an den Ecken der Auswahl.
pub const HANDLE_SIZE: f32 = 5.0;
/// Versatz des Schlagschattens unter dem Blatt.
pub const SHADOW_OFFSET: f32 = 4.0;

/// Rechnet ein PDF-Rechteck in Bildschirmkoordinaten um.
///
/// `origin` ist die linke **obere** Ecke des Seitenblatts auf dem Bildschirm.
pub fn pdf_to_screen(rect: &Rect, page_box: &Rect, zoom: f32, origin: Pos2) -> egui::Rect {
    let min = pdf_point_to_screen(rect.ll.x, rect.ur.y, page_box, zoom, origin);
    let max = pdf_point_to_screen(rect.ur.x, rect.ll.y, page_box, zoom, origin);
    egui::Rect::from_min_max(min, max)
}

/// Rechnet zwei Bildschirmpunkte in ein normalisiertes PDF-Rechteck um.
pub fn screen_to_pdf(a: Pos2, b: Pos2, page_box: &Rect, zoom: f32, origin: Pos2) -> Rect {
    Rect::from_corners(
        screen_to_pdf_point(a, page_box, zoom, origin),
        screen_to_pdf_point(b, page_box, zoom, origin),
    )
}

/// Ein einzelner PDF-Punkt → Bildschirm.
pub fn pdf_point_to_screen(x: f64, y: f64, page_box: &Rect, zoom: f32, origin: Pos2) -> Pos2 {
    let z = zoom as f64;
    Pos2::new(
        origin.x + ((x - page_box.ll.x) * z) as f32,
        // Y-Spiegelung: die Oberkante der MediaBox liegt auf `origin.y`.
        origin.y + ((page_box.ur.y - y) * z) as f32,
    )
}

/// Ein einzelner Bildschirmpunkt → PDF-User-Space.
pub fn screen_to_pdf_point(p: Pos2, page_box: &Rect, zoom: f32, origin: Pos2) -> Point {
    let z = (zoom as f64).max(f64::EPSILON);
    Point::new(
        page_box.ll.x + (p.x - origin.x) as f64 / z,
        page_box.ur.y - (p.y - origin.y) as f64 / z,
    )
}

/// Größe des Seitenblatts auf dem Bildschirm.
pub fn page_size_screen(page_box: &Rect, zoom: f32) -> Vec2 {
    Vec2::new(
        (page_box.width() as f32 * zoom).max(1.0),
        (page_box.height() as f32 * zoom).max(1.0),
    )
}

/// Zoomfaktor, bei dem die Seite vollständig in `available` passt.
pub fn fit_zoom(available: Vec2, page_box: &Rect) -> f32 {
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

/// Schematische Vorschau einer einzelnen Seite.
///
/// Hält nur Referenzen — der Zustand lebt in [`crate::state::AppState`].
pub struct PagePreview<'a> {
    pub page: usize,
    pub page_box: &'a Rect,
    pub runs: &'a [TextRun],
    pub zoom: f32,
}

impl<'a> PagePreview<'a> {
    pub fn new(page: usize, page_box: &'a Rect, runs: &'a [TextRun], zoom: f32) -> Self {
        Self {
            page,
            page_box,
            runs,
            zoom,
        }
    }

    /// Größe des Blatts auf dem Bildschirm.
    pub fn size(&self) -> Vec2 {
        page_size_screen(self.page_box, self.zoom)
    }

    /// Zeichnet Blatt, Schatten und Text. `origin` ist die linke obere Ecke.
    pub fn paint(&self, painter: &egui::Painter, origin: Pos2) {
        let sheet = egui::Rect::from_min_size(origin, self.size());

        // Schlagschatten, damit das Blatt vom Hintergrund abhebt.
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

        for run in self.runs.iter().filter(|r| r.page == self.page) {
            self.paint_run(painter, origin, run);
        }
    }

    fn paint_run(&self, painter: &egui::Painter, origin: Pos2, run: &TextRun) {
        let text = run.text.trim_end();
        if text.is_empty() {
            return;
        }
        let target = pdf_to_screen(&run.rect, self.page_box, self.zoom, origin);
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

/// Zeichnet ein Schwärzungsrechteck.
///
/// Aktivierte Regionen werden halbtransparent gefüllt, deaktivierte nur
/// umrandet. Die ausgewählte Region bekommt einen dickeren Rand.
pub fn paint_region(
    painter: &egui::Painter,
    rect: egui::Rect,
    rgb: (u8, u8, u8),
    enabled: bool,
    selected: bool,
) {
    let (r, g, b) = rgb;
    let color = Color32::from_rgb(r, g, b);
    if enabled {
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
    painter.rect_stroke(rect, NO_ROUNDING, Stroke::new(width, color));
    if selected {
        // Griffpunkte an den Ecken der Auswahl.
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

/// Pixelgenaue Vorschau über pdfium.
///
/// **Noch nicht implementiert.** Der Platzhalter existiert, damit das
/// Feature-Gate und die Modulstruktur stehen; die Standardfassung des Programms
/// benutzt ausschließlich [`PagePreview`]. Außerhalb dieses `cfg`-Blocks wird
/// kein einziger pdfium-Typ referenziert, das Crate baut also ohne das Feature
/// vollständig ohne native Bibliotheken.
#[cfg(feature = "pdfium")]
pub mod pdfium_backend {
    use redact_core::{RedactError, Result};

    /// Rendert eine Seite als RGBA-Puffer `(pixel, breite, höhe)`.
    pub fn render_page_rgba(
        _pdf_bytes: &[u8],
        _page: usize,
        _scale: f32,
    ) -> Result<(Vec<u8>, usize, usize)> {
        Err(RedactError::Pdf(
            "pdfium-Backend ist in dieser Fassung noch nicht implementiert".into(),
        ))
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
            for zoom in [0.5f32, 1.0, 2.5] {
                for origin in origins {
                    for rect in rects {
                        let screen = pdf_to_screen(&rect, &page_box, zoom, origin);
                        let back = screen_to_pdf(screen.min, screen.max, &page_box, zoom, origin);
                        assert_close(back.ll.x, rect.ll.x, "ll.x");
                        assert_close(back.ll.y, rect.ll.y, "ll.y");
                        assert_close(back.ur.x, rect.ur.x, "ur.x");
                        assert_close(back.ur.y, rect.ur.y, "ur.y");
                    }
                }
            }
        }
    }

    #[test]
    fn y_axis_is_flipped() {
        let page_box = offset_box();
        let origin = Pos2::new(0.0, 0.0);
        // Ganz oben auf der Seite (y nahe ur.y) …
        let top = Rect::new(20.0, 850.0, 100.0, 860.0);
        // … und ganz unten (y nahe ll.y).
        let bottom = Rect::new(20.0, 22.0, 100.0, 32.0);

        let top_screen = pdf_to_screen(&top, &page_box, 1.0, origin);
        let bottom_screen = pdf_to_screen(&bottom, &page_box, 1.0, origin);

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
        let page_box = offset_box();
        let origin = Pos2::new(100.0, 50.0);
        // Die linke untere Ecke der MediaBox landet auf (origin.x, origin.y + h*zoom).
        let corner = pdf_point_to_screen(10.0, 20.0, &page_box, 2.0, origin);
        assert_close(corner.x as f64, 100.0, "x");
        assert_close(corner.y as f64, 50.0 + 842.0 * 2.0, "y");

        // Die linke obere Ecke landet exakt auf dem Ursprung.
        let top_left = pdf_point_to_screen(10.0, 862.0, &page_box, 2.0, origin);
        assert_close(top_left.x as f64, 100.0, "x");
        assert_close(top_left.y as f64, 50.0, "y");
    }

    #[test]
    fn screen_to_pdf_normalizes_dragged_corners() {
        let page_box = offset_box();
        let origin = Pos2::new(5.0, 5.0);
        // Von rechts unten nach links oben gezogen.
        let a = Pos2::new(300.0, 400.0);
        let b = Pos2::new(100.0, 200.0);
        let rect = screen_to_pdf(a, b, &page_box, 1.5, origin);
        assert!(rect.ll.x < rect.ur.x);
        assert!(rect.ll.y < rect.ur.y);
        assert!(rect.width() > 0.0 && rect.height() > 0.0);

        // Gleiches Ergebnis, egal in welcher Reihenfolge die Ecken kommen.
        let swapped = screen_to_pdf(b, a, &page_box, 1.5, origin);
        assert_eq!(rect, swapped);
    }

    #[test]
    fn page_size_and_fit_zoom() {
        let page_box = offset_box();
        assert_eq!(page_size_screen(&page_box, 1.0), Vec2::new(595.0, 842.0));
        assert_eq!(page_size_screen(&page_box, 2.0), Vec2::new(1190.0, 1684.0));

        // Passt auf halbe Höhe → Zoom ~0.5.
        let z = fit_zoom(Vec2::new(1190.0, 421.0), &page_box);
        assert!((z - 0.5).abs() < 1e-6, "{z}");
        // Entartete Seite → 1.0.
        assert_eq!(
            fit_zoom(Vec2::new(100.0, 100.0), &Rect::new(0.0, 0.0, 0.0, 0.0)),
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
        let page_box = offset_box();
        let runs = vec![
            demo_run(0, "IBAN: DE89 3704 0044", 72.0, 700.0),
            demo_run(0, "   ", 72.0, 680.0),
            demo_run(1, "andere Seite", 72.0, 660.0),
        ];

        egui::__run_test_ui(|ui| {
            let origin = Pos2::new(12.0, 12.0);
            for zoom in [0.05f32, 1.0, 2.5] {
                // 0.05 erzwingt den Zweig „zu klein für Text“.
                PagePreview::new(0, &page_box, &runs, zoom).paint(ui.painter(), origin);
            }
            let rect = pdf_to_screen(&runs[0].rect, &page_box, 1.0, origin);
            paint_region(ui.painter(), rect, (60, 130, 246), true, true);
            paint_region(ui.painter(), rect, (220, 60, 60), false, false);
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
