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

// Maße der Oberfläche. Ausdrücklich `f32`, siehe die Anmerkung in
// [`crate::viewer`] zu `float_literal_f32_fallback`.

/// Startbreite der Seitenleiste.
const SIDEBAR_WIDTH: f32 = 320.0;
/// Kleinste Breite der Seitenleiste.
const SIDEBAR_MIN_WIDTH: f32 = 220.0;
/// Größte Breite der Seitenleiste.
const SIDEBAR_MAX_WIDTH: f32 = 520.0;
/// Vertikaler Abstand in der oberen Leiste.
const BAR_PADDING: f32 = 2.0;
/// Grober Platzbedarf von Seitenleiste und Leisten für „Einpassen“.
const CHROME_SIZE: Vec2 = Vec2::new(360.0, 140.0);

// Maße des Hinweises beim Ziehen von Dateien über das Fenster.

/// Abstand des gestrichelten Rahmens zum Rand des Hauptbereichs.
const DROP_MARGIN: f32 = 16.0;
/// Strichstärke des gestrichelten Rahmens.
const DROP_STROKE: f32 = 3.0;
/// Länge eines Strichs des gestrichelten Rahmens.
const DROP_DASH: f32 = 12.0;
/// Länge einer Lücke des gestrichelten Rahmens.
const DROP_GAP: f32 = 8.0;
/// Eckenrundung der Abdunklung.
const DROP_ROUNDING: f32 = 6.0;
/// Schriftgröße des Hinweistextes.
const DROP_FONT_SIZE: f32 = 28.0;

/// Farbe des Ablege-Hinweises (dasselbe Orange wie manuelle Regionen).
const DROP_ACCENT: Color32 = Color32::from_rgb(240, 150, 30);

/// Baut einen Speichern-Dialog mit Verzeichnis- und Namensvorgabe.
///
/// `suggested` liefert beides; fehlt es (kein Dokument geladen), wird
/// `fallback_name` benutzt. `rfd` nimmt für den Dateinamen nur den reinen
/// Namen entgegen, das Verzeichnis kommt getrennt über `set_directory`.
fn save_dialog(
    title: &str,
    filter_name: &str,
    extensions: &[&str],
    directory: Option<PathBuf>,
    suggested: Option<PathBuf>,
    fallback_name: &str,
) -> rfd::FileDialog {
    let name = suggested
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| fallback_name.to_string());

    let mut dialog = rfd::FileDialog::new()
        .add_filter(filter_name, extensions)
        .set_title(title)
        .set_file_name(name);

    // Verzeichnis des Vorschlags hat Vorrang, sonst das des Originals.
    let dir = suggested
        .as_ref()
        .and_then(|p| p.parent())
        .filter(|d| !d.as_os_str().is_empty())
        .map(|d| d.to_path_buf())
        .or(directory);
    if let Some(dir) = dir {
        dialog = dialog.set_directory(dir);
    }
    dialog
}

/// Was mit den auf das Fenster gezogenen Dateien geschehen soll.
///
/// Reine Datenentscheidung — dadurch lässt sich das Verhalten ohne Fenster,
/// ohne Maus und ohne echtes Ablegen testen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropAction {
    /// Es wurde nichts (Brauchbares) abgelegt.
    Nothing,
    /// Diese Datei öffnen. `ignored` weitere Dateien wurden übergangen.
    Open { path: PathBuf, ignored: usize },
    /// Nur Inhalt ohne Pfad — so liefert der Web-Build ab. Über
    /// [`AppState::load_bytes`] laden.
    OpenBytes {
        name: String,
        bytes: std::sync::Arc<[u8]>,
        ignored: usize,
    },
    /// Nichts davon war ein PDF.
    Rejected { names: Vec<String> },
}

/// Heißt diese Datei auf `.pdf` (Groß-/Kleinschreibung egal)?
pub fn is_pdf_name(name: &str) -> bool {
    std::path::Path::new(name)
        .extension()
        .map(|e| e.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false)
}

/// Anzeigename einer abgelegten Datei.
fn dropped_name(file: &egui::DroppedFile) -> String {
    match &file.path {
        Some(path) => path.display().to_string(),
        None if !file.name.is_empty() => file.name.clone(),
        None => "(unbenannt)".to_string(),
    }
}

/// Entscheidet, was mit einer Menge abgelegter Dateien passiert.
///
/// Es wird die **erste** PDF-Datei geöffnet; alle weiteren Dateien werden
/// gezählt und in der Statuszeile erwähnt. Ist keine PDF-Datei dabei, kommt
/// [`DropAction::Rejected`] mit den Namen zurück — geladen wird dann nichts.
pub fn classify_drop(files: &[egui::DroppedFile]) -> DropAction {
    if files.is_empty() {
        return DropAction::Nothing;
    }

    let position = files.iter().position(|f| match &f.path {
        Some(path) => is_pdf_name(&path.to_string_lossy()),
        // Web-Build: kein Pfad, dafür Name und Inhalt.
        None => is_pdf_name(&f.name) || f.mime == "application/pdf",
    });

    let Some(index) = position else {
        return DropAction::Rejected {
            names: files.iter().map(dropped_name).collect(),
        };
    };

    let ignored = files.len() - 1;
    let file = &files[index];
    match (&file.path, &file.bytes) {
        (Some(path), _) => DropAction::Open {
            path: path.clone(),
            ignored,
        },
        (None, Some(bytes)) => DropAction::OpenBytes {
            name: file.name.clone(),
            bytes: bytes.clone(),
            ignored,
        },
        // Weder Pfad noch Inhalt — damit lässt sich nichts anfangen.
        (None, None) => DropAction::Rejected {
            names: vec![dropped_name(file)],
        },
    }
}

/// Zustand der Oberfläche.
pub struct RedactApp {
    pub state: AppState,
    selector: RectangleSelector,
    /// Vom Aufrufer gewünschte Pattern-IDs (leer = alle eingebauten).
    pub pattern_ids: Vec<String>,
    /// Letzte Fehlermeldung; wird als roter Text in der Statuszeile gezeigt.
    error: Option<String>,
    /// Fläche des Hauptbereichs im letzten Frame — Grundlage für den
    /// Ablege-Hinweis, der über allem liegt.
    central_rect: Option<egui::Rect>,
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
            central_rect: None,
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

    /// Öffnet ein aus dem Speicher abgelegtes PDF (Web-Build ohne Pfad).
    pub fn open_bytes_and_analyze(&mut self, bytes: &[u8], name: &str) {
        let path = (!name.is_empty()).then(|| PathBuf::from(name));
        let ids = self.pattern_ids.clone();
        let booking = self.state.booking_path.clone();
        let result = self
            .state
            .load_bytes(bytes, path)
            .and_then(|()| self.state.analyze(&ids, booking.as_deref()).map(|_| ()));
        self.report(result);
    }

    /// Führt die Entscheidung aus, die [`classify_drop`] getroffen hat.
    pub fn apply_drop(&mut self, action: DropAction) {
        match action {
            DropAction::Nothing => {}
            DropAction::Open { path, ignored } => {
                self.open_and_analyze(path);
                self.note_ignored(ignored);
            }
            DropAction::OpenBytes {
                name,
                bytes,
                ignored,
            } => {
                self.open_bytes_and_analyze(&bytes, &name);
                self.note_ignored(ignored);
            }
            DropAction::Rejected { names } => {
                self.error = None;
                self.state.status = format!(
                    "Keine PDF-Datei abgelegt — übergangen: {}",
                    names.join(", ")
                );
            }
        }
    }

    /// Ergänzt die Statuszeile um die Zahl der übergangenen Dateien.
    fn note_ignored(&mut self, ignored: usize) {
        if ignored > 0 {
            self.state.status = format!(
                "{} ({ignored} weitere Datei(en) ignoriert)",
                self.state.status
            );
        }
    }

    fn export_to(&mut self, out: PathBuf) {
        let audit = AppState::audit_path_for(&out);
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
                let mut dialog = rfd::FileDialog::new()
                    .add_filter("CSV", &["csv"])
                    .set_title("Buchungsliste laden");
                if let Some(dir) = self.state.dialog_directory() {
                    dialog = dialog.set_directory(dir);
                }
                if let Some(path) = dialog.pick_file() {
                    self.state.booking_path = Some(path);
                    self.analyze();
                }
            }

            ui.separator();

            if ui
                .add_enabled(loaded, egui::Button::new("Exportieren …"))
                .clicked()
            {
                // Vorgabe: neben dem Original, Stamm + Namenszusatz.
                let suggested = self.state.suggested_output_path();
                let dialog = save_dialog(
                    "Geschwärztes PDF speichern",
                    "PDF",
                    &["pdf"],
                    self.state.dialog_directory(),
                    suggested,
                    "geschwaerzt.pdf",
                );
                if let Some(path) = dialog.save_file() {
                    self.export_to(path);
                }
            }

            if ui
                .add_enabled(loaded, egui::Button::new("Review speichern …"))
                .clicked()
            {
                let dialog = save_dialog(
                    "Review-Datei speichern",
                    "JSON",
                    &["json"],
                    self.state.dialog_directory(),
                    self.state.suggested_review_path(),
                    "review.json",
                );
                if let Some(path) = dialog.save_file() {
                    let result = self
                        .state
                        .to_review_file()
                        .to_json()
                        .and_then(|json| std::fs::write(&path, json).map_err(Into::into));
                    self.report(result);
                }
            }

            if ui.button("Review laden …").clicked() {
                let mut dialog = rfd::FileDialog::new()
                    .add_filter("JSON", &["json"])
                    .set_title("Review-Datei laden");
                if let Some(dir) = self.state.dialog_directory() {
                    dialog = dialog.set_directory(dir);
                }
                if let Some(path) = dialog.pick_file() {
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
                let available = ui.ctx().screen_rect().size() - CHROME_SIZE;
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
                        viewer::NO_ROUNDING,
                        Stroke::new(viewer::DRAG_STROKE, Color32::from_rgb(240, 150, 30)),
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

    /// Nimmt auf das Fenster gezogene Dateien entgegen.
    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }
        self.apply_drop(classify_drop(&dropped));
    }

    /// Zeigt an, dass hier abgelegt werden darf, solange Dateien über dem
    /// Fenster schweben.
    ///
    /// Gezeichnet wird auf der Vordergrundebene, damit der Hinweis über
    /// Seitenleiste und Seitenvorschau liegt.
    fn paint_drop_hint(&self, ctx: &egui::Context) {
        let hovering = ctx.input(|i| i.raw.hovered_files.len());
        if hovering == 0 {
            return;
        }
        let Some(area) = self.central_rect else {
            return;
        };

        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("drop_hint"),
        ));

        // Abdunkeln, damit der Hinweis nicht im Seiteninhalt untergeht.
        painter.rect_filled(area, DROP_ROUNDING, Color32::from_black_alpha(160));

        // Gestrichelter Rahmen aus vier Kanten.
        let frame = area.shrink(DROP_MARGIN);
        let stroke = Stroke::new(DROP_STROKE, DROP_ACCENT);
        let corners = [
            frame.left_top(),
            frame.right_top(),
            frame.right_bottom(),
            frame.left_bottom(),
            frame.left_top(),
        ];
        for edge in corners.windows(2) {
            painter.extend(egui::Shape::dashed_line(edge, stroke, DROP_DASH, DROP_GAP));
        }

        let text = if hovering > 1 {
            format!("{hovering} Dateien hier ablegen — die erste PDF wird geöffnet")
        } else {
            "PDF hier ablegen".to_string()
        };
        painter.text(
            frame.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(DROP_FONT_SIZE),
            Color32::WHITE,
        );
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
        self.handle_dropped_files(ctx);
        self.handle_keys(ctx);

        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.add_space(BAR_PADDING);
            self.top_bar(ui);
            ui.add_space(BAR_PADDING);
        });

        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            self.status_bar(ui);
        });

        egui::SidePanel::left("sidebar")
            .default_width(SIDEBAR_WIDTH)
            .width_range(SIDEBAR_MIN_WIDTH..=SIDEBAR_MAX_WIDTH)
            .show(ctx, |ui| {
                crate::sidebar::show(ui, &mut self.state);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.central_rect = Some(ui.max_rect());
            if self.state.is_loaded() {
                self.page_view(ui);
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new(
                            "Kein Dokument geladen.\n\n\
                             „PDF öffnen …“ oben links — oder eine PDF-Datei hier ablegen.",
                        )
                        .weak(),
                    );
                });
            }
        });

        // Zuletzt, damit der Hinweis über allem liegt.
        self.paint_drop_hint(ctx);
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
