//! Die eframe-Anwendung: Leiste oben, Seitenleiste links, Seite in der Mitte,
//! Statuszeile unten.
//!
//! Dieses Modul ist absichtlich dünn. Es übersetzt Klicks und Tastendrücke in
//! Aufrufe von [`AppState`] und zeichnet dessen Inhalt — mehr nicht. Alles,
//! was ohne Bildschirm prüfbar sein muss, liegt in [`crate::state`],
//! [`crate::viewer`] und [`crate::selector`].

use std::path::PathBuf;

use egui::{Color32, Key, Pos2, RichText, Stroke, Vec2};
use redact_core::ReviewFile;

use crate::selector::{hit_test, RectangleSelector};
use crate::state::{AppState, MAX_ZOOM, MIN_ZOOM};
use crate::viewer::{self, PagePreview};

/// Rand zwischen Scrollbereich und Seitenblatt.
const SHEET_MARGIN: f32 = 24.0;
/// Schrittweite der Pfeiltasten im PDF-User-Space (Punkt).
const NUDGE: f64 = 1.0;
/// Schrittweite mit gedrückter Umschalttaste.
const NUDGE_FAST: f64 = 10.0;

/// Zustand der Oberfläche.
pub struct RedactApp {
    pub state: AppState,
    selector: RectangleSelector,
    /// Vom Aufrufer gewünschte Pattern-IDs (leer = alle eingebauten).
    pub pattern_ids: Vec<String>,
    /// Letzte Fehlermeldung; wird als roter Text in der Statuszeile gezeigt.
    error: Option<String>,
}

impl Default for RedactApp {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl RedactApp {
    pub fn new(pattern_ids: Vec<String>) -> Self {
        Self {
            state: AppState::new(),
            selector: RectangleSelector::new(),
            pattern_ids,
            error: None,
        }
    }

    /// Meldet einen Fehler in der Statuszeile statt das Programm zu beenden.
    fn report(&mut self, result: redact_core::Result<()>) {
        match result {
            Ok(()) => self.error = None,
            Err(e) => {
                let message = e.to_string();
                self.error = Some(message.clone());
                self.state.status = message;
            }
        }
    }

    /// Lädt ein PDF und analysiert es sofort.
    pub fn open_and_analyze(&mut self, path: PathBuf) {
        let booking = self.state.booking_path.clone();
        let ids = self.pattern_ids.clone();
        let result = self
            .state
            .load_document(&path)
            .and_then(|()| self.state.analyze(&ids, booking.as_deref()).map(|_| ()));
        self.report(result);
    }

    /// Führt nur die Analyse aus (z.B. nach dem Laden einer Buchungsliste).
    pub fn analyze(&mut self) {
        if !self.state.is_loaded() {
            self.state.status = "Erst ein PDF öffnen".to_string();
            return;
        }
        let booking = self.state.booking_path.clone();
        let ids = self.pattern_ids.clone();
        let result = self.state.analyze(&ids, booking.as_deref()).map(|_| ());
        self.report(result);
    }

    fn export_to(&mut self, out: PathBuf) {
        let audit = out.with_extension("audit.json");
        match self.state.export(&out, Some(&audit)) {
            Ok(report) => {
                self.error = None;
                self.state.warnings = report.warnings.clone();
                self.state.status = format!(
                    "Export: {} Rechteck(e), {} Zeichen entfernt → {}",
                    report.drawn_rects,
                    report.removed_glyphs,
                    out.display()
                );
            }
            Err(e) => self.report(Err(e)),
        }
    }

    // ------------------------------------------------------------- Zeichnen

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("PDF öffnen …").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("PDF", &["pdf"])
                    .set_title("PDF öffnen")
                    .pick_file()
                {
                    self.open_and_analyze(path);
                }
            }

            let loaded = self.state.is_loaded();
            if ui
                .add_enabled(loaded, egui::Button::new("Analysieren"))
                .clicked()
            {
                self.analyze();
            }

            if ui.button("Buchungsliste …").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("CSV", &["csv"])
                    .set_title("Buchungsliste laden")
                    .pick_file()
                {
                    self.state.booking_path = Some(path);
                    self.analyze();
                }
            }

            ui.separator();

            if ui
                .add_enabled(loaded, egui::Button::new("Exportieren …"))
                .clicked()
            {
                let suggested = self
                    .state
                    .pdf_path
                    .as_ref()
                    .and_then(|p| {
                        p.file_stem()
                            .map(|s| format!("{}_geschwaerzt.pdf", s.to_string_lossy()))
                    })
                    .unwrap_or_else(|| "geschwaerzt.pdf".to_string());
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("PDF", &["pdf"])
                    .set_file_name(suggested)
                    .set_title("Geschwärztes PDF speichern")
                    .save_file()
                {
                    self.export_to(path);
                }
            }

            if ui
                .add_enabled(loaded, egui::Button::new("Review speichern …"))
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("JSON", &["json"])
                    .set_file_name("review.json")
                    .save_file()
                {
                    let result = self
                        .state
                        .to_review_file()
                        .to_json()
                        .and_then(|json| std::fs::write(&path, json).map_err(Into::into));
                    self.report(result);
                }
            }

            if ui.button("Review laden …").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("JSON", &["json"])
                    .pick_file()
                {
                    let result = std::fs::read_to_string(&path)
                        .map_err(redact_core::RedactError::from)
                        .and_then(|data| ReviewFile::from_json(&data))
                        .map(|review| self.state.apply_review_file(review));
                    self.report(result);
                }
            }

            ui.separator();

            ui.label("Zoom");
            let mut zoom = self.state.zoom;
            if ui
                .add(egui::Slider::new(&mut zoom, MIN_ZOOM..=MAX_ZOOM).fixed_decimals(2))
                .changed()
            {
                self.state.set_zoom(zoom);
            }
            if ui.button("Einpassen").clicked() {
                let page_box = self.state.current_page_box();
                let available = ui.ctx().screen_rect().size() - Vec2::new(360.0, 140.0);
                self.state.set_zoom(viewer::fit_zoom(available, &page_box));
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let pages = self.state.page_count();
            let current = if pages == 0 {
                0
            } else {
                self.state.current_page + 1
            };
            ui.label(RichText::new(format!("Seite {current} / {pages}")).strong());
            ui.separator();

            if ui
                .add_enabled(self.state.current_page > 0, egui::Button::new("◀"))
                .clicked()
            {
                self.state.prev_page();
            }
            if ui
                .add_enabled(
                    pages > 0 && self.state.current_page + 1 < pages,
                    egui::Button::new("▶"),
                )
                .clicked()
            {
                self.state.next_page();
            }
            ui.separator();

            match &self.error {
                Some(message) => {
                    ui.label(RichText::new(message).color(Color32::from_rgb(220, 60, 60)));
                }
                None => {
                    ui.label(&self.state.status);
                }
            }

            if !self.state.warnings.is_empty() {
                ui.separator();
                ui.label(
                    RichText::new(format!("{} Warnung(en)", self.state.warnings.len()))
                        .color(Color32::from_rgb(200, 140, 0)),
                )
                .on_hover_text(self.state.warnings.join("\n"));
            }
        });
    }

    fn page_view(&mut self, ui: &mut egui::Ui) {
        let page = self.state.current_page;
        let page_box = self.state.current_page_box();
        let zoom = self.state.zoom;
        let sheet_size = viewer::page_size_screen(&page_box, zoom);
        let total = sheet_size + Vec2::splat(SHEET_MARGIN * 2.0);

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let (area, response) = ui.allocate_exact_size(total, egui::Sense::click_and_drag());
                let origin = area.min + Vec2::splat(SHEET_MARGIN);
                let painter = ui.painter_at(area);

                // --- Blatt und Text ---
                {
                    let preview = PagePreview::new(page, &page_box, &self.state.runs, zoom);
                    preview.paint(&painter, origin);
                }

                // --- Regionen ---
                for index in self.state.regions_on_page(page) {
                    let entry = &self.state.regions[index];
                    let screen = viewer::pdf_to_screen(&entry.region.rect, &page_box, zoom, origin);
                    viewer::paint_region(
                        &painter,
                        screen,
                        entry.color.rgb(),
                        entry.enabled,
                        self.state.selected_region == Some(index),
                    );
                }

                // --- Laufender Ziehvorgang ---
                if let Some(preview) = self.selector.preview() {
                    painter.rect_stroke(
                        preview,
                        0.0,
                        Stroke::new(1.5, Color32::from_rgb(240, 150, 30)),
                    );
                }

                self.handle_pointer(&response, &page_box, origin, page, zoom);
            });
    }

    fn handle_pointer(
        &mut self,
        response: &egui::Response,
        page_box: &redact_core::Rect,
        origin: Pos2,
        page: usize,
        zoom: f32,
    ) {
        // Klick wählt aus (oder hebt die Auswahl auf).
        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                let point = viewer::screen_to_pdf_point(pos, page_box, zoom, origin);
                self.state.selected_region = hit_test(&self.state.regions, page, point);
            }
        }

        // Ziehen legt eine manuelle Region an.
        if let Some((a, b)) = self.selector.interact(response) {
            let rect = viewer::screen_to_pdf(a, b, page_box, zoom, origin);
            self.state.add_manual_region(page, rect, "manuell markiert");
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let (delete, escape, left, right, up, down, page_up, page_down, shift) = ctx.input(|i| {
            (
                i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
                i.key_pressed(Key::Escape),
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::PageUp),
                i.key_pressed(Key::PageDown),
                i.modifiers.shift,
            )
        });

        if escape {
            self.selector.cancel();
            self.state.selected_region = None;
        }
        if delete {
            self.state.delete_selected();
        }

        let step = if shift { NUDGE_FAST } else { NUDGE };
        if self.state.selected_region.is_some() {
            // Y zeigt im PDF nach oben — „Pfeil hoch“ erhöht also y.
            if left {
                self.state.move_selected(-step, 0.0);
            }
            if right {
                self.state.move_selected(step, 0.0);
            }
            if up {
                self.state.move_selected(0.0, step);
            }
            if down {
                self.state.move_selected(0.0, -step);
            }
        } else {
            if left || page_up {
                self.state.prev_page();
            }
            if right || page_down {
                self.state.next_page();
            }
        }
    }
}

impl eframe::App for RedactApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_keys(ctx);

        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.add_space(2.0);
            self.top_bar(ui);
            ui.add_space(2.0);
        });

        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            self.status_bar(ui);
        });

        egui::SidePanel::left("sidebar")
            .default_width(320.0)
            .width_range(220.0..=520.0)
            .show(ctx, |ui| {
                crate::sidebar::show(ui, &mut self.state);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.state.is_loaded() {
                self.page_view(ui);
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new("Kein Dokument geladen.\n\n„PDF öffnen …“ oben links.")
                            .weak(),
                    );
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyze_without_document_sets_status_instead_of_failing() {
        let mut app = RedactApp::new(vec!["iban_de".to_string()]);
        app.analyze();
        assert!(app.state.status.contains("PDF"));
        assert!(app.error.is_none());
    }

    #[test]
    fn report_puts_the_error_into_the_status_line() {
        let mut app = RedactApp::default();
        app.report(Err(redact_core::RedactError::Pdf("kaputt".into())));
        assert!(app.error.as_deref().unwrap().contains("kaputt"));
        assert!(app.state.status.contains("kaputt"));
        app.report(Ok(()));
        assert!(app.error.is_none());
    }

    #[test]
    fn open_and_analyze_reports_unreadable_files() {
        let mut app = RedactApp::default();
        app.open_and_analyze(PathBuf::from("/gibt/es/nicht.pdf"));
        assert!(app.error.is_some());
        assert!(!app.state.is_loaded());
    }
}
