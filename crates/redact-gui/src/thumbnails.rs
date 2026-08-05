//! Miniaturansichten in der linken Spalte.
//!
//! Es wird **nichts neu gerendert**: die Kleinbilder liegen ohnehin schon in
//! [`crate::render::PageCache`] (sie überbrücken im Hauptbereich die Wartezeit
//! auf das große Bild). Diese Spalte zeigt sie nur zusätzlich an und fordert
//! über [`PageCache::request_thumb`] nach, was noch fehlt — eines je Bild,
//! damit der Arbeits-Thread nicht Aufträge verwirft.
//!
//! Die Seitenzahl steht **neben** jedem Bild. Bei fünfzig Seiten sehen die
//! Miniaturen einander zum Verwechseln ähnlich; ohne Zahl wäre die Spalte eine
//! Reihe grauer Rechtecke.

use egui::{Align, Color32, Pos2, RichText, Sense, Stroke, Vec2};

use crate::render::PageCache;
use crate::state::{AppState, HitSummary};
use crate::viewer::{PageView, NO_ROUNDING};

/// Breite der Spalte.
pub const PANEL_WIDTH: f32 = 152.0;
/// Kleinste und größte Breite beim Ziehen am Rand.
pub const PANEL_MIN_WIDTH: f32 = 96.0;
/// Siehe [`PANEL_MIN_WIDTH`].
pub const PANEL_MAX_WIDTH: f32 = 260.0;
/// Breite eines Kleinbilds.
pub const THUMB_WIDTH: f32 = 96.0;
/// Rahmenstärke der aktuellen Seite.
const CURRENT_STROKE: f32 = 2.5;
/// Rahmenstärke der übrigen Seiten.
const IDLE_STROKE: f32 = 1.0;

/// Größe eines Kleinbilds bei gegebener Seitengeometrie.
///
/// Maßgeblich ist der **Anzeigeraum** (nach `/Rotate`), sonst stünde eine quer
/// gedrehte Seite hochkant in der Spalte. Die Höhe wird begrenzt, damit eine
/// extrem schmale Seite die Spalte nicht sprengt.
pub fn thumb_size(view: &PageView, width: f32) -> Vec2 {
    let display = view.display_box();
    let ratio = if display.width() > 0.0 {
        (display.height() / display.width()) as f32
    } else {
        1.0
    };
    let height = (width * ratio).clamp(width * 0.25, width * 4.0);
    Vec2::new(width, height)
}

/// Zeichnet die Spalte.
///
/// `scroll_to_current` bringt die markierte Seite in den sichtbaren Bereich —
/// nötig, wenn die Seite über Tastatur oder Trefferliste gewechselt wurde.
///
/// ## Warum die Bilanz hereingereicht wird
///
/// Die Zahl neben einem Kleinbild kam aus `AppState::regions_on_page(page).len()`
/// und zählte damit **jede** Zeile der Seite mit: abgewählte, blockierte,
/// doppelte und Schutzeinträge der Negativliste. Eine Seite, auf der ein
/// Schutzeintrag alles blockiert und drei Handrechtecke abgewählt sind, trug
/// so die **5** mit der Sprechblase „Treffer auf dieser Seite“ — geschwärzt
/// wird dort nichts. Genau dieser Fehler war in der Kopfzeile der Seitenleiste
/// schon abgestellt („Schutzeinträge sind keine Funde“); hier stand er noch,
/// und hier ist er gefährlicher: die Spalte ist die Übersicht, mit der man am
/// Ende durchgeht, ob auf keiner Seite etwas stehen geblieben ist.
///
/// [`crate::app`] rechnet die Bilanz ohnehin einmal je Bild aus. Sie hier zu
/// benutzen behebt zugleich den Aufwand: siehe
/// [`AppState::redactions_per_page`].
pub fn show(
    ui: &mut egui::Ui,
    state: &mut AppState,
    pages: &mut PageCache,
    summary: &HitSummary,
    scroll_to_current: bool,
) {
    ui.heading("Seiten");
    let count = state.page_count();
    if count == 0 {
        ui.add_space(8.0);
        ui.label(RichText::new(crate::app::EMPTY_DOCUMENT_HINT).weak());
        return;
    }

    let width = (ui.available_width() - 44.0).clamp(48.0, THUMB_WIDTH);
    let mut jump: Option<usize> = None;
    // Ein Durchlauf über die Regionen für die ganze Spalte statt einer je
    // Kleinbild.
    let redactions = state.redactions_per_page(summary);

    egui::ScrollArea::vertical()
        .id_salt("thumbnails")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for page in 0..count {
                let current = page == state.current_page;
                let size = thumb_size(&state.page_view(page), width);
                let hits = redactions.get(page).copied().unwrap_or(0);

                let response = ui
                    .horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{}", page + 1))
                                .small()
                                .strong()
                                .color(number_color(ui, current)),
                        );
                        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
                        paint_thumb(ui, rect, pages, page, current);
                        if hits > 0 {
                            ui.label(RichText::new(format!("{hits}")).small().weak())
                                .on_hover_text("Schwärzungen auf dieser Seite");
                        }
                        // Ein Zeichen statt einer Zahl: auf dieser Seite hat
                        // der Rasterizer nichts gezeichnet. In der Spalte, mit
                        // der man am Ende durchgeht, ob nirgends etwas stehen
                        // geblieben ist, gehört das genannt — das Kleinbild
                        // selbst ist ja weiß und sagt nichts. Siehe
                        // [`crate::render::PageCache::nothing_drawn`].
                        if pages.nothing_drawn(page) {
                            ui.label(
                                RichText::new(crate::app::NOTHING_DRAWN_MARK)
                                    .small()
                                    .color(ui.visuals().warn_fg_color),
                            )
                            .on_hover_text(crate::app::NOTHING_DRAWN_NOTICE);
                        }
                        response
                    })
                    .inner;

                let response = response.on_hover_text(format!("Seite {}", page + 1));
                if response.clicked() {
                    jump = Some(page);
                }
                if current && scroll_to_current {
                    response.scroll_to_me(Some(Align::Center));
                }
                ui.add_space(4.0);

                // Fehlendes Kleinbild nachfordern; tut nichts, solange schon
                // eines unterwegs ist.
                if !pages.has_thumb(page) {
                    pages.request_thumb(page, ui.ctx());
                }
            }
        });

    if let Some(page) = jump {
        state.set_page(page);
    }
}

/// Farbe der Seitenzahl — die aktuelle Seite hebt sich ab.
fn number_color(ui: &egui::Ui, current: bool) -> Color32 {
    if current {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().weak_text_color()
    }
}

/// Malt ein Kleinbild samt Rahmen; fehlt es noch, bleibt ein leeres Blatt.
fn paint_thumb(ui: &egui::Ui, rect: egui::Rect, pages: &PageCache, page: usize, current: bool) {
    let painter = ui.painter_at(rect);
    // Papier ist weiß, auch im dunklen Thema — die Seite wird nicht eingefärbt.
    painter.rect_filled(rect, NO_ROUNDING, Color32::WHITE);

    match pages.thumb(page) {
        Some(texture) => {
            painter.image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        // Noch nicht gerechnet — ein leeres Blatt mit drei Punkten, damit die
        // Spalte nicht springt, sobald das Bild eintrifft.
        None => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "…",
                egui::FontId::proportional(14.0),
                Color32::from_gray(120),
            );
        }
    }

    let stroke = if current {
        Stroke::new(CURRENT_STROKE, ui.visuals().selection.bg_fill)
    } else {
        Stroke::new(IDLE_STROKE, ui.visuals().weak_text_color())
    };
    painter.rect_stroke(rect, NO_ROUNDING, stroke);
}

#[cfg(test)]
mod tests {
    use super::*;

    use redact_core::Rect;

    #[test]
    fn thumb_size_follows_the_display_box() {
        let portrait = PageView::new(Rect::new(0.0, 0.0, 600.0, 800.0), 0);
        let size = thumb_size(&portrait, 90.0);
        assert_eq!(size.x, 90.0);
        assert!((size.y - 120.0).abs() < 0.01, "{size:?}");

        // Gedreht ist die Seite quer — das Kleinbild also flacher als breit.
        let turned = PageView::new(Rect::new(0.0, 0.0, 600.0, 800.0), 90);
        let turned_size = thumb_size(&turned, 90.0);
        assert!(turned_size.y < turned_size.x, "{turned_size:?}");

        // Entartete Seiten kippen die Spalte nicht.
        let flat = PageView::new(Rect::new(0.0, 0.0, 600.0, 1.0), 0);
        assert!(thumb_size(&flat, 90.0).y >= 22.5);
        let tall = PageView::new(Rect::new(0.0, 0.0, 1.0, 600.0), 0);
        assert!(thumb_size(&tall, 90.0).y <= 360.0);
    }

    /// Rauchtest ohne Bildschirm: leer und gefüllt zeichnen, dabei einen Klick
    /// simulieren gibt es nicht — geprüft wird, dass nichts panickt und dass
    /// fehlende Kleinbilder angefordert werden.
    #[test]
    fn the_column_draws_and_asks_for_missing_thumbnails() {
        use std::cell::RefCell;

        let mut state = AppState::new();
        let pages = RefCell::new(PageCache::new());

        // Ohne Dokument: nur der leere Zustand.
        let empty = RefCell::new(state);
        let summary = empty.borrow().hit_summary();
        egui::__run_test_ui(|ui| {
            show(
                ui,
                &mut empty.borrow_mut(),
                &mut pages.borrow_mut(),
                &summary,
                false,
            );
        });
        assert!(!pages.borrow().is_busy());

        state = empty.into_inner();
        state
            .load_bytes(&redact_pdf::testing::demo_statement(), None)
            .unwrap();
        pages
            .borrow_mut()
            .set_document(state.document.clone().unwrap());
        let loaded = RefCell::new(state);
        let summary = loaded.borrow().hit_summary();
        egui::__run_test_ui(|ui| {
            show(
                ui,
                &mut loaded.borrow_mut(),
                &mut pages.borrow_mut(),
                &summary,
                true,
            );
        });
        assert!(
            pages.borrow().is_busy(),
            "die Spalte muss fehlende Kleinbilder anfordern"
        );
    }
}
