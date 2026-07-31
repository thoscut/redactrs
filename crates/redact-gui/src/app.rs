//! Die eframe-Anwendung: Leiste oben, Seitenleiste links, Seite in der Mitte,
//! Statuszeile unten.
//!
//! Dieses Modul ist absichtlich dünn. Es übersetzt Klicks und Tastendrücke in
//! Aufrufe von [`AppState`] und zeichnet dessen Inhalt — mehr nicht. Alles,
//! was ohne Bildschirm prüfbar sein muss, liegt in [`crate::state`],
//! [`crate::viewer`], [`crate::render`] und [`crate::selector`] — und, was die
//! Tastatur angeht, in [`key_commands`].
//!
//! ## Rückfragen vor Datenverlust
//!
//! Fenster schließen, ein zweites PDF öffnen, eine Datei ablegen oder ein
//! Review laden warf bisher alle von Hand gezogenen Rechtecke kommentarlos weg.
//! Alle vier Wege laufen jetzt über [`RedactApp::may_discard`]; beim Schließen
//! wird zusätzlich [`egui::ViewportCommand::CancelClose`] geschickt, solange
//! nicht bestätigt wurde.

use std::path::PathBuf;

use egui::{Color32, Key, Pos2, RichText, Stroke, Vec2};
use redact_core::ReviewFile;

use crate::render::PageCache;
use crate::selector::{hit_test, RectangleSelector};
use crate::state::{AppState, HitSummary, RegionColor, MAX_ZOOM, MIN_ZOOM};
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

/// Farbe des Ablege-Hinweises — dasselbe Orange wie manuelle Regionen.
fn drop_accent() -> Color32 {
    let (r, g, b) = RegionColor::Manual.rgb();
    Color32::from_rgb(r, g, b)
}

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

// ---------------------------------------------------------------------------
// Tastatur
// ---------------------------------------------------------------------------

/// Die gedrückten Tasten eines Bildes, als reine Daten.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyState {
    pub delete: bool,
    pub escape: bool,
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
    pub page_up: bool,
    pub page_down: bool,
    pub shift: bool,
    /// Liegt der Eingabefokus in einem Textfeld?
    pub text_focus: bool,
}

/// Was ein Tastendruck bewirken soll.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyCommand {
    /// Auswahl aufheben und einen laufenden Ziehvorgang abbrechen.
    Deselect,
    DeleteSelected,
    /// Ausgewählte Region verschieben (PDF-User-Space, Y zeigt nach oben).
    Move {
        dx: f64,
        dy: f64,
    },
    PrevPage,
    NextPage,
}

/// Übersetzt gedrückte Tasten in Befehle.
///
/// **Der Fokus entscheidet zuerst.** Vorher las die Oberfläche die Tasten
/// global aus, ohne zu prüfen, wo die Eingabe hingehört. Ergebnis: die
/// Rücktaste im Feld „Ersetzen“ löschte die ausgewählte Region — ohne
/// Rückfrage, ohne Rückgängig —, die Pfeiltasten verschoben sie beim
/// Textcursor-Bewegen um 1 bzw. 10 pt, und Escape ließ das Feld mitten im Wort
/// verschwinden. Steht der Fokus in einem Textfeld, gehören die Tasten dorthin
/// und **nirgendwo sonst hin**.
pub fn key_commands(keys: KeyState, has_selection: bool) -> Vec<KeyCommand> {
    if keys.text_focus {
        return Vec::new();
    }

    let mut commands = Vec::new();
    if keys.escape {
        commands.push(KeyCommand::Deselect);
    }
    if keys.delete && has_selection {
        commands.push(KeyCommand::DeleteSelected);
    }

    let step = if keys.shift { NUDGE_FAST } else { NUDGE };
    // Escape hebt die Auswahl auf — danach sind die Pfeiltasten wieder für das
    // Blättern zuständig.
    let selected = has_selection && !keys.escape;
    if selected {
        // Y zeigt im PDF nach oben — „Pfeil hoch“ erhöht also y.
        if keys.left {
            commands.push(KeyCommand::Move { dx: -step, dy: 0.0 });
        }
        if keys.right {
            commands.push(KeyCommand::Move { dx: step, dy: 0.0 });
        }
        if keys.up {
            commands.push(KeyCommand::Move { dx: 0.0, dy: step });
        }
        if keys.down {
            commands.push(KeyCommand::Move { dx: 0.0, dy: -step });
        }
    } else {
        if keys.left {
            commands.push(KeyCommand::PrevPage);
        }
        if keys.right {
            commands.push(KeyCommand::NextPage);
        }
    }
    // Bild auf/ab blättert immer, auch mit ausgewählter Region.
    if keys.page_up {
        commands.push(KeyCommand::PrevPage);
    }
    if keys.page_down {
        commands.push(KeyCommand::NextPage);
    }
    commands
}

// ---------------------------------------------------------------------------
// Rückfragen
// ---------------------------------------------------------------------------

/// Muss vor dem Schließen des Fensters nachgefragt werden?
///
/// Reine Entscheidung, damit sie ohne Fenster prüfbar ist.
pub fn needs_close_confirmation(has_manual_work: bool, already_confirmed: bool) -> bool {
    has_manual_work && !already_confirmed
}

/// Text der Rückfrage. `what` beschreibt, was gleich passiert.
pub fn discard_question(regions: usize, what: &str) -> String {
    format!(
        "Es sind {regions} Schwärzung(en) von Hand bearbeitet oder gezeichnet worden. \
         {what} verwirft sie. Es gibt kein Rückgängig.\n\n\
         Tipp: „Review speichern …“ sichert den Stand als Datei.\n\n\
         Trotzdem fortfahren?"
    )
}

/// Meldung nach einem geglückten Export.
///
/// Zwei Dinge, die früher nur als Sprechblase oder gar nicht auftauchten,
/// stehen jetzt im Text:
///
/// * **die erste Warnung** des Schwärzers — etwa „Bild nur überdeckt“. Das ist
///   kein Randdetail, sondern die Aussage, dass an dieser Stelle Bildinhalt
///   bloß verdeckt und nicht entfernt wurde;
/// * **das Protokoll**, das ungefragt neben der Ausgabe entsteht. Enthält es
///   Einträge der Negativliste, stehen darin Klartextnamen (Feld
///   `blocked_by_negative_list[].pattern`) — wer die Ausgabe weitergibt, darf
///   das Protokoll nicht versehentlich mitschicken.
pub fn export_status(
    rects: usize,
    glyphs: usize,
    out: &std::path::Path,
    audit: &std::path::Path,
    blocked: usize,
    first_warning: Option<&str>,
) -> String {
    let mut text = format!(
        "Export: {rects} Rechteck(e), {glyphs} Zeichen entfernt → {}",
        out.display()
    );
    if let Some(warning) = first_warning {
        text.push_str(&format!("  ⚠ {warning}"));
    }
    let audit_name = audit
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| audit.display().to_string());
    text.push_str(&format!("  ·  Protokoll: {audit_name}"));
    if blocked > 0 {
        text.push_str(" (enthält Klartext aus Ihrer Schutzliste — nicht mitgeben)");
    }
    text
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
    /// Gerasterte Seitenbilder; rechnet auf einem eigenen Thread.
    pages: PageCache,
    /// Wurde das Schließen des Fensters bereits bestätigt?
    close_confirmed: bool,
    /// Rückfragen unterdrücken (nur für Tests ohne Bildschirm).
    ///
    /// Ein `rfd`-Dialog blockiert und braucht ein Fenster; im Test gibt es
    /// beides nicht.
    ask_before_discarding: bool,
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
            pages: PageCache::new(),
            close_confirmed: false,
            ask_before_discarding: true,
        }
    }

    /// Ohne Rückfragen — für Tests ohne Bildschirm.
    #[cfg(test)]
    fn silent(pattern_ids: Vec<String>) -> Self {
        Self {
            ask_before_discarding: false,
            ..Self::new(pattern_ids)
        }
    }

    /// Darf Handarbeit weggeworfen werden?
    ///
    /// Ohne Handarbeit sofort `true` — die Rückfrage soll nur dann kommen,
    /// wenn wirklich etwas verloren geht.
    fn may_discard(&self, what: &str) -> bool {
        if !self.state.has_manual_work() {
            return true;
        }
        if !self.ask_before_discarding {
            return true;
        }
        let count = self
            .state
            .regions
            .iter()
            .filter(|a| a.is_hand_made())
            .count();
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title("Von Hand bearbeitete Schwärzungen verwerfen?")
            .set_description(discard_question(count, what))
            .set_buttons(rfd::MessageButtons::YesNo)
            .show()
            == rfd::MessageDialogResult::Yes
    }

    /// Übergibt das geladene Dokument an den Rasterizer.
    fn hand_document_to_the_renderer(&mut self) {
        match self.state.document.as_ref() {
            Some(doc) => self.pages.set_document(doc.clone()),
            None => self.pages.reset(),
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
        let loaded = self.state.load_document(&path);
        self.after_loading(loaded);
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
        let loaded = self.state.load_bytes(bytes, path);
        self.after_loading(loaded);
    }

    /// Gemeinsamer Abschluss beider Ladewege: Rasterizer versorgen, dann
    /// analysieren.
    ///
    /// Der Rasterizer bekommt das Dokument **nur bei geglücktem Laden** —
    /// scheitert es, bleibt der alte Zustand samt Seitenbildern stehen, statt
    /// den Zwischenspeicher grundlos zu leeren.
    fn after_loading(&mut self, loaded: redact_core::Result<()>) {
        if loaded.is_err() {
            self.report(loaded);
            return;
        }
        self.hand_document_to_the_renderer();
        self.close_confirmed = false;
        let booking = self.state.booking_path.clone();
        let ids = self.pattern_ids.clone();
        let analyzed = self.state.analyze(&ids, booking.as_deref()).map(|_| ());
        self.report(analyzed);
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
        let blocked = self.state.blocked_regions().len();
        match self.state.export(&out, Some(&audit)) {
            Ok(report) => {
                self.error = None;
                self.state.warnings = report.warnings.clone();
                self.state.status = export_status(
                    report.drawn_rects,
                    report.removed_glyphs,
                    &out,
                    &audit,
                    blocked,
                    report.warnings.first().map(String::as_str),
                );
            }
            Err(e) => self.report(Err(e)),
        }
    }

    // ------------------------------------------------------------- Zeichnen

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("PDF öffnen …").clicked() && self.may_discard("Ein anderes PDF zu öffnen")
            {
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

            if ui.button("Review laden …").clicked()
                && self.may_discard("Ein Review zu laden ersetzt die Trefferliste und")
            {
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
                let view = self.state.current_page_view();
                let available = ui.ctx().screen_rect().size() - CHROME_SIZE;
                self.state.set_zoom(viewer::fit_zoom(available, &view));
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

            // Warnungen kommen aus zwei Quellen: vom Schwärzen (bleiben nach
            // einem Export stehen) und vom Rastern der aktuellen Seite. Beide
            // gehören in die Zeile, nicht in einen Dialog — ein Dialog müsste
            // weggeklickt werden und ist im nächsten Bild wieder da.
            let warnings = self.visible_warnings();
            if let Some(first) = warnings.first() {
                ui.separator();
                let mut text = first.clone();
                if warnings.len() > 1 {
                    text.push_str(&format!(" (+{} weitere)", warnings.len() - 1));
                }
                ui.label(RichText::new(text).color(Color32::from_rgb(160, 100, 0)))
                    .on_hover_text(warnings.join("\n"));
            }

            if self.pages.is_busy() {
                ui.separator();
                ui.label(RichText::new("Seitenbild wird erstellt …").weak());
            }
        });
    }

    /// Warnungen des letzten Exports **und** des Rasterizers zur aktuellen
    /// Seite, ohne Dopplungen.
    fn visible_warnings(&self) -> Vec<String> {
        let mut all = self.state.warnings.clone();
        for warning in self.pages.warnings(self.state.current_page) {
            if !all.contains(warning) {
                all.push(warning.clone());
            }
        }
        all
    }

    /// Der Hauptbereich: gerastertes Seitenbild, Schwärzungsrechtecke, Maus.
    fn paint_page(&mut self, ui: &mut egui::Ui, summary: &HitSummary) {
        let page = self.state.current_page;
        let view = self.state.current_page_view();
        let zoom = self.state.zoom;
        let total = view.size_screen(zoom) + Vec2::splat(SHEET_MARGIN * 2.0);

        // Fehlendes Bild anfordern — löst nur bei Wechsel von Seite oder
        // Zoomstufe wirklich etwas aus.
        self.pages.request(page, &view, zoom, ui.ctx());

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let (area, response) = ui.allocate_exact_size(total, egui::Sense::click_and_drag());
                let origin = area.min + Vec2::splat(SHEET_MARGIN);
                let painter = ui.painter_at(area);
                let sheet = egui::Rect::from_min_size(origin, view.size_screen(zoom));

                let preview = PagePreview::new(page, view, &self.state.runs, zoom);
                preview.paint_sheet(&painter, origin);

                // --- Gerastertes Seitenbild ---
                let cached = self.pages.page(page);
                let mut show_schematic = true;
                if let Some((texture, is_thumb)) = cached.and_then(|c| c.best()) {
                    painter.image(
                        texture.id(),
                        sheet,
                        egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    // Die schematische Vorschau bleibt der Notnagel: nur wenn das
                    // Bild leer ist (`degraded`) oder erst das grobe Kleinbild
                    // vorliegt, wird der Text zusätzlich gezeichnet.
                    let degraded = cached.map(|c| c.meta.degraded).unwrap_or(false);
                    show_schematic = degraded || is_thumb;
                }
                if show_schematic {
                    preview.paint_text(&painter, origin);
                }

                // --- Regionen ---
                for index in self.state.regions_on_page(page) {
                    let entry = &self.state.regions[index];
                    let screen = viewer::pdf_to_screen(&entry.region.rect, &view, zoom, origin);
                    viewer::paint_region(
                        &painter,
                        screen,
                        entry.color.rgb(),
                        // Gefüllt wird nur, was auch wirklich geschwärzt wird.
                        viewer::RegionStyle::from_outcome(summary.outcome(index)),
                        self.state.selected_region == Some(index),
                    );
                }

                // --- Laufender Ziehvorgang ---
                if let Some(preview) = self.selector.preview() {
                    painter.rect_stroke(
                        preview,
                        viewer::NO_ROUNDING,
                        Stroke::new(viewer::DRAG_STROKE, drop_accent()),
                    );
                }

                self.handle_pointer(&response, &view, origin, page, zoom);
            });
    }

    fn handle_pointer(
        &mut self,
        response: &egui::Response,
        view: &viewer::PageView,
        origin: Pos2,
        page: usize,
        zoom: f32,
    ) {
        // Klick wählt aus (oder hebt die Auswahl auf).
        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                let point = viewer::screen_to_pdf_point(pos, view, zoom, origin);
                self.state.selected_region = hit_test(&self.state.regions, page, point);
            }
        }

        // Ziehen legt eine manuelle Region an.
        if let Some((a, b)) = self.selector.interact(response) {
            let rect = viewer::screen_to_pdf(a, b, view, zoom, origin);
            self.state.add_manual_region(page, rect, "manuell markiert");
        }
    }

    /// Nimmt auf das Fenster gezogene Dateien entgegen.
    ///
    /// Ein Fehlgriff beim Ziehen genügte früher, um alle Handarbeit zu
    /// verlieren — deshalb wird auch hier gefragt. Abgelehnte Dateien
    /// (kein PDF) ändern nichts und brauchen keine Rückfrage.
    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }
        let action = classify_drop(&dropped);
        let replaces_everything =
            !matches!(action, DropAction::Nothing | DropAction::Rejected { .. });
        if replaces_everything && !self.may_discard("Eine abgelegte Datei zu öffnen") {
            self.state.status = "Abgelegte Datei nicht geöffnet — nichts verändert".to_string();
            return;
        }
        self.apply_drop(action);
    }

    /// Fragt vor dem Schließen nach, wenn Handarbeit verloren ginge.
    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        if !needs_close_confirmation(self.state.has_manual_work(), self.close_confirmed) {
            return;
        }
        // Erst das Schließen zurücknehmen, dann fragen — sonst ist das Fenster
        // weg, bevor jemand antworten konnte.
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        if self.may_discard("Das Fenster zu schließen") {
            self.close_confirmed = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
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
        let stroke = Stroke::new(DROP_STROKE, drop_accent());
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

    /// Liest die Tasten aus dem Kontext.
    ///
    /// `focused()` beantwortet die entscheidende Frage: liegt der Eingabefokus
    /// in einem Widget (typischerweise dem Textfeld „Ersetzen“ oder dem
    /// Namenszusatz)? Dann gehören alle Tasten dorthin.
    fn read_keys(ctx: &egui::Context) -> KeyState {
        let text_focus = ctx.memory(|m| m.focused().is_some());
        ctx.input(|i| KeyState {
            delete: i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
            escape: i.key_pressed(Key::Escape),
            left: i.key_pressed(Key::ArrowLeft),
            right: i.key_pressed(Key::ArrowRight),
            up: i.key_pressed(Key::ArrowUp),
            down: i.key_pressed(Key::ArrowDown),
            page_up: i.key_pressed(Key::PageUp),
            page_down: i.key_pressed(Key::PageDown),
            shift: i.modifiers.shift,
            text_focus,
        })
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let keys = Self::read_keys(ctx);
        self.apply_key_commands(&key_commands(keys, self.state.selected_region.is_some()));
    }

    fn apply_key_commands(&mut self, commands: &[KeyCommand]) {
        for command in commands {
            match *command {
                KeyCommand::Deselect => {
                    self.selector.cancel();
                    self.state.selected_region = None;
                }
                KeyCommand::DeleteSelected => {
                    self.state.delete_selected();
                }
                KeyCommand::Move { dx, dy } => {
                    self.state.move_selected(dx, dy);
                }
                KeyCommand::PrevPage => self.state.prev_page(),
                KeyCommand::NextPage => self.state.next_page(),
            }
        }
    }
}

impl eframe::App for RedactApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Fertige Seitenbilder abholen, bevor gezeichnet wird.
        self.pages.poll(ctx);
        self.handle_dropped_files(ctx);

        // Einmal je Bild, nicht je Trefferzeile.
        let summary = self.state.hit_summary();

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
                crate::sidebar::show(ui, &mut self.state, &summary);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.central_rect = Some(ui.max_rect());
            if self.state.is_loaded() {
                self.paint_page(ui, &summary);
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

        // Tasten erst **nach** den Panels: vorher weiß egui noch nicht, ob der
        // Fokus in einem Textfeld liegt, und genau davon hängt ab, ob die
        // Rücktaste eine Region löscht oder ein Zeichen.
        self.handle_keys(ctx);
        self.handle_close_request(ctx);

        // Zuletzt, damit der Hinweis über allem liegt.
        self.paint_drop_hint(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use redact_core::Rect;

    // ----------------------------------------------------------- Tastatur

    /// A1: Steht der Fokus in einem Textfeld, darf **keine** Taste bis in den
    /// Zustand durchschlagen. Vorher löschte die Rücktaste im Feld „Ersetzen“
    /// die ausgewählte Region — ohne Rückfrage und ohne Rückgängig.
    #[test]
    fn a_focused_text_field_swallows_every_key() {
        let every_key = KeyState {
            delete: true,
            escape: true,
            left: true,
            right: true,
            up: true,
            down: true,
            page_up: true,
            page_down: true,
            shift: true,
            text_focus: true,
        };
        assert!(key_commands(every_key, true).is_empty());
        assert!(key_commands(every_key, false).is_empty());

        // Ohne Fokus tut dieselbe Eingabe sehr wohl etwas.
        let unfocused = KeyState {
            text_focus: false,
            ..every_key
        };
        assert!(!key_commands(unfocused, true).is_empty());
    }

    /// Und derselbe Fall einmal ganz konkret, mit echtem Zustand.
    #[test]
    fn backspace_in_a_text_field_does_not_delete_the_selected_region() {
        let mut app = RedactApp::silent(Vec::new());
        app.state
            .add_manual_region(0, Rect::new(10.0, 10.0, 50.0, 20.0), "Gehalt");
        let before = app.state.regions.clone();

        let typing = KeyState {
            delete: true,
            text_focus: true,
            ..KeyState::default()
        };
        app.apply_key_commands(&key_commands(typing, app.state.selected_region.is_some()));
        assert_eq!(app.state.regions, before, "Region darf nicht verschwinden");
        assert_eq!(app.state.selected_region, Some(0));

        // Ohne Fokus im Textfeld löscht dieselbe Taste sehr wohl.
        let on_canvas = KeyState {
            text_focus: false,
            ..typing
        };
        app.apply_key_commands(&key_commands(on_canvas, true));
        assert!(app.state.regions.is_empty());
    }

    #[test]
    fn arrow_keys_nudge_a_selection_and_otherwise_turn_the_page() {
        let arrows = |left, right, up, down, shift| KeyState {
            left,
            right,
            up,
            down,
            shift,
            ..KeyState::default()
        };

        // Mit Auswahl: verschieben, Y zeigt im PDF nach oben.
        assert_eq!(
            key_commands(arrows(false, false, true, false, false), true),
            vec![KeyCommand::Move { dx: 0.0, dy: 1.0 }]
        );
        assert_eq!(
            key_commands(arrows(false, false, false, true, true), true),
            vec![KeyCommand::Move { dx: 0.0, dy: -10.0 }]
        );
        assert_eq!(
            key_commands(arrows(true, false, false, false, false), true),
            vec![KeyCommand::Move { dx: -1.0, dy: 0.0 }]
        );

        // Ohne Auswahl: blättern.
        assert_eq!(
            key_commands(arrows(true, false, false, false, false), false),
            vec![KeyCommand::PrevPage]
        );
        assert_eq!(
            key_commands(arrows(false, true, false, false, false), false),
            vec![KeyCommand::NextPage]
        );

        // Bild auf/ab blättert auch mit Auswahl.
        let page_down = KeyState {
            page_down: true,
            ..KeyState::default()
        };
        assert_eq!(key_commands(page_down, true), vec![KeyCommand::NextPage]);

        // Escape hebt die Auswahl auf und verschiebt nicht mehr.
        let escape_and_left = KeyState {
            escape: true,
            left: true,
            ..KeyState::default()
        };
        assert_eq!(
            key_commands(escape_and_left, true),
            vec![KeyCommand::Deselect, KeyCommand::PrevPage]
        );

        // Löschen ohne Auswahl ist ein Nichts.
        let delete = KeyState {
            delete: true,
            ..KeyState::default()
        };
        assert!(key_commands(delete, false).is_empty());
    }

    // ------------------------------------------------- Rückfrage vor Verlust

    /// A8: die Rückfrage kommt genau dann, wenn wirklich etwas verloren geht.
    #[test]
    fn closing_only_asks_when_there_is_hand_work_to_lose() {
        assert!(!needs_close_confirmation(false, false));
        assert!(!needs_close_confirmation(false, true));
        assert!(needs_close_confirmation(true, false));
        // Nach dem Bestätigen darf nicht noch einmal gefragt werden, sonst
        // ließe sich das Fenster nie schließen.
        assert!(!needs_close_confirmation(true, true));

        let text = discard_question(3, "Das Fenster zu schließen");
        assert!(text.contains('3'));
        assert!(text.contains("Das Fenster zu schließen"));
        assert!(text.contains("Rückgängig"));
        assert!(text.contains("Review speichern"));
    }

    /// Ohne Handarbeit darf nichts nachfragen — die Rückfrage wäre dann nur im
    /// Weg. `may_discard` fragt dafür `AppState::has_manual_work` ab.
    #[test]
    fn may_discard_passes_straight_through_without_hand_work() {
        let mut app = RedactApp::new(vec!["iban_de".to_string()]);
        // Frisch: nichts zu verlieren, also kein Dialog (sonst hinge der Test).
        assert!(app.may_discard("Test"));

        app.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "demo.pdf");
        assert!(!app.state.regions.is_empty());
        assert!(
            app.may_discard("Test"),
            "eine reine Analyse ist wiederholbar"
        );

        // Erst Handarbeit macht die Rückfrage nötig.
        app.state
            .add_manual_region(0, Rect::new(10.0, 10.0, 50.0, 20.0), "Gehalt");
        assert!(app.state.has_manual_work());
    }

    /// Ein Fehlgriff beim Ziehen und Ablegen darf keine Arbeit kosten.
    #[test]
    fn a_declined_drop_leaves_everything_untouched() {
        let ctx = egui::Context::default();
        let app = std::cell::RefCell::new(RedactApp::new(vec!["iban_de".to_string()]));
        app.borrow_mut()
            .open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "demo.pdf");
        app.borrow_mut()
            .state
            .add_manual_region(0, Rect::new(10.0, 10.0, 50.0, 20.0), "Gehalt");
        let before = app.borrow().state.regions.clone();

        // `ask_before_discarding` bleibt an, aber die Antwort wird nicht
        // abgewartet: der Dialog käme nur, wenn `may_discard` ihn aufruft.
        // Stattdessen wird hier die Entscheidung selbst geprüft.
        assert!(app.borrow().state.has_manual_work());

        // Eine abgelegte Nicht-PDF-Datei ändert ohnehin nichts und braucht
        // deshalb auch keine Rückfrage.
        let input = egui::RawInput {
            dropped_files: vec![dropped_path("/daten/bild.png")],
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.borrow_mut().handle_dropped_files(ctx));
        assert_eq!(app.borrow().state.regions, before);
        assert!(app.borrow().state.status.contains("Keine PDF-Datei"));
    }

    // ------------------------------------------------------------ Statuszeile

    /// A5 und A7: Bildwarnung und Protokolldatei gehören in die Statuszeile.
    #[test]
    fn the_export_status_names_the_warning_and_the_audit_file() {
        let out = PathBuf::from("/daten/auszug_geschwaerzt.pdf");
        let audit = PathBuf::from("/daten/auszug_geschwaerzt_audit.json");

        let plain = export_status(3, 42, &out, &audit, 0, None);
        assert!(plain.contains("3 Rechteck(e)"));
        assert!(plain.contains("42 Zeichen"));
        assert!(plain.contains("auszug_geschwaerzt_audit.json"));
        assert!(!plain.contains("Schutzliste"));

        let warned = export_status(
            1,
            0,
            &out,
            &audit,
            2,
            Some("Bild auf Seite 1 nur überdeckt"),
        );
        assert!(
            warned.contains("Bild auf Seite 1 nur überdeckt"),
            "die erste Warnung muss im Text stehen: {warned}"
        );
        // Enthält das Protokoll Klartext aus der Negativliste, wird gewarnt.
        assert!(warned.contains("Schutzliste"), "{warned}");
    }

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

    // ------------------------------------------------------- Ablegen (Drop)

    /// Abgelegte Datei mit Pfad (Desktop-Build).
    fn dropped_path(path: &str) -> egui::DroppedFile {
        egui::DroppedFile {
            path: Some(PathBuf::from(path)),
            ..Default::default()
        }
    }

    /// Abgelegte Datei nur mit Namen und Inhalt (Web-Build).
    fn dropped_bytes(name: &str, mime: &str, bytes: &[u8]) -> egui::DroppedFile {
        egui::DroppedFile {
            path: None,
            name: name.to_string(),
            mime: mime.to_string(),
            bytes: Some(std::sync::Arc::from(bytes)),
            ..Default::default()
        }
    }

    #[test]
    fn pdf_names_are_recognised_case_insensitively() {
        assert!(is_pdf_name("a.pdf"));
        assert!(is_pdf_name("A.PDF"));
        assert!(is_pdf_name("/daten/Konto.Pdf"));
        assert!(!is_pdf_name("a.png"));
        assert!(!is_pdf_name("pdf"));
        assert!(!is_pdf_name(""));
    }

    #[test]
    fn classify_drop_picks_the_first_pdf_and_counts_the_rest() {
        assert_eq!(classify_drop(&[]), DropAction::Nothing);

        assert_eq!(
            classify_drop(&[dropped_path("/daten/a.pdf")]),
            DropAction::Open {
                path: PathBuf::from("/daten/a.pdf"),
                ignored: 0,
            }
        );

        // Die erste PDF gewinnt, alles Weitere wird gezählt.
        let files = [
            dropped_path("/daten/notiz.txt"),
            dropped_path("/daten/b.PDF"),
            dropped_path("/daten/c.pdf"),
        ];
        assert_eq!(
            classify_drop(&files),
            DropAction::Open {
                path: PathBuf::from("/daten/b.PDF"),
                ignored: 2,
            }
        );
    }

    #[test]
    fn classify_drop_rejects_non_pdf_files() {
        let files = [
            dropped_path("/daten/bild.png"),
            dropped_path("/daten/x.csv"),
        ];
        match classify_drop(&files) {
            DropAction::Rejected { names } => {
                assert_eq!(names.len(), 2);
                assert!(names[0].contains("bild.png"));
            }
            other => panic!("Ablehnung erwartet, war {other:?}"),
        }
    }

    #[test]
    fn classify_drop_handles_the_web_case_without_a_path() {
        // Nur Name und Inhalt — so liefert der Web-Build ab.
        let files = [dropped_bytes("auszug.pdf", "", b"%PDF-1.5")];
        assert_eq!(
            classify_drop(&files),
            DropAction::OpenBytes {
                name: "auszug.pdf".to_string(),
                bytes: std::sync::Arc::from(&b"%PDF-1.5"[..]),
                ignored: 0,
            }
        );

        // Auch am MIME-Typ erkennbar, wenn der Name nichts hergibt.
        let by_mime = [dropped_bytes("auszug", "application/pdf", b"%PDF-1.5")];
        assert!(matches!(
            classify_drop(&by_mime),
            DropAction::OpenBytes { .. }
        ));

        // Weder Pfad noch Inhalt ist unbrauchbar.
        let empty = [egui::DroppedFile {
            name: "auszug.pdf".to_string(),
            ..Default::default()
        }];
        assert!(matches!(classify_drop(&empty), DropAction::Rejected { .. }));
    }

    #[test]
    fn apply_drop_opens_a_pdf_from_bytes_and_notes_ignored_files() {
        let mut app = RedactApp::new(vec!["iban_de".to_string()]);
        app.apply_drop(DropAction::OpenBytes {
            name: "auszug.pdf".to_string(),
            bytes: std::sync::Arc::from(&redact_pdf::testing::demo_statement()[..]),
            ignored: 2,
        });

        assert!(app.state.is_loaded());
        assert!(app.error.is_none());
        assert!(!app.state.regions.is_empty());
        assert!(
            app.state.status.contains("2 weitere"),
            "Statuszeile: {}",
            app.state.status
        );
        // Der Name wird als Pfad übernommen, damit Namensvorschläge greifen.
        assert_eq!(
            app.state.suggested_output_path().unwrap(),
            PathBuf::from("auszug_geschwaerzt.pdf")
        );
    }

    #[test]
    fn apply_drop_reports_rejected_files_without_loading() {
        let mut app = RedactApp::default();
        app.apply_drop(DropAction::Rejected {
            names: vec!["/daten/bild.png".to_string()],
        });
        assert!(!app.state.is_loaded());
        assert!(app.error.is_none(), "Ablehnung ist kein Fehler");
        assert!(app.state.status.contains("Keine PDF-Datei"));
        assert!(app.state.status.contains("bild.png"));
    }

    #[test]
    fn apply_drop_does_nothing_for_an_empty_drop() {
        let mut app = RedactApp::default();
        let before = app.state.status.clone();
        app.apply_drop(DropAction::Nothing);
        assert_eq!(app.state.status, before);
    }

    /// Rauchtest ohne Bildschirm: der Ablege-Hinweis wird nur gezeichnet,
    /// solange Dateien schweben — und stürzt dabei nicht ab.
    #[test]
    fn drop_hint_paints_only_while_files_hover() {
        let mut app = RedactApp {
            central_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                Vec2::new(800.0, 600.0),
            )),
            ..RedactApp::default()
        };

        let ctx = egui::Context::default();
        let shapes = |input: egui::RawInput, app: &RedactApp| -> usize {
            ctx.run(input, |ctx| app.paint_drop_hint(ctx)).shapes.len()
        };
        let hovering = |count: usize| egui::RawInput {
            hovered_files: vec![egui::HoveredFile::default(); count],
            ..Default::default()
        };

        // Ohne schwebende Dateien wird nichts gezeichnet …
        let idle = shapes(egui::RawInput::default(), &app);
        // … mit schwebenden Dateien dagegen schon.
        assert!(shapes(hovering(1), &app) > idle);
        assert!(shapes(hovering(3), &app) > idle);

        // Ohne bekannte Fläche wird ebenfalls nichts gezeichnet.
        app.central_rect = None;
        assert_eq!(shapes(hovering(1), &app), idle);
    }

    /// `handle_dropped_files` liest die Rohdaten aus dem Kontext.
    #[test]
    fn handle_dropped_files_reads_the_raw_input() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            dropped_files: vec![dropped_path("/gibt/es/nicht.pdf")],
            ..Default::default()
        };
        let app = std::cell::RefCell::new(RedactApp::default());
        let _ = ctx.run(input, |ctx| app.borrow_mut().handle_dropped_files(ctx));

        // Der Pfad existiert nicht — das muss als Fehler in der Statuszeile
        // ankommen, nicht als Absturz.
        let app = app.borrow();
        assert!(app.error.is_some());
        assert!(!app.state.is_loaded());
    }

    #[test]
    fn apply_drop_reports_a_broken_pdf_through_the_status_line() {
        let mut app = RedactApp::default();
        app.apply_drop(DropAction::OpenBytes {
            name: "kaputt.pdf".to_string(),
            bytes: std::sync::Arc::from(&b"kein PDF"[..]),
            ignored: 0,
        });
        assert!(!app.state.is_loaded());
        assert!(app.error.is_some());
    }
}
