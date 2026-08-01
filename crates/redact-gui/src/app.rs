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
use crate::selector::{hit_handle, hit_test, HandleDrag, PointerFrame, RectangleSelector};
use crate::state::{AppState, HitSummary, RegionColor, MAX_ZOOM, MIN_ZOOM};
use crate::theme::Theme;
use crate::toolbar::{self, ToolAction, ToolContext, ToolItem};
use crate::viewer::{self, PagePreview};
use redact_pipeline::Config;

/// Was im Hauptbereich steht, solange nichts geladen ist.
///
/// Ein leerer grauer Bereich sagt nichts; dieser Satz nennt beide Wege, die
/// zum Ziel führen.
pub const EMPTY_DOCUMENT_HINT: &str = "Noch kein PDF geladen — öffnen oder hierher ziehen";

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
/// Grober Platzbedarf der Ränder für „Passend“: Trefferliste, Miniaturspalte,
/// obere Leiste und Statuszeile. Wird das zu klein geschätzt, ragt die Seite
/// nach dem Einpassen aus dem Fenster.
const CHROME_SIZE: Vec2 = Vec2::new(360.0 + crate::thumbnails::PANEL_WIDTH, 140.0);

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
    pub home: bool,
    pub end: bool,
    pub shift: bool,
    /// Steuerungstaste (unter macOS die Befehlstaste).
    pub ctrl: bool,
    pub key_o: bool,
    pub key_s: bool,
    pub key_z: bool,
    pub key_y: bool,
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
    FirstPage,
    LastPage,
    Undo,
    Redo,
    /// Strg+O — Dateidialog „PDF öffnen“.
    Open,
    /// Strg+S — Dateidialog „Geschwärztes PDF speichern“.
    Export,
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

    // Tastenkürzel mit Steuerungstaste stehen für sich: Strg+Z ist Rückgängig
    // und nicht zusätzlich irgendein Buchstabe im Blätterwerk.
    if keys.ctrl {
        let mut commands = Vec::new();
        if keys.key_o {
            commands.push(KeyCommand::Open);
        }
        if keys.key_s {
            commands.push(KeyCommand::Export);
        }
        if keys.key_z {
            commands.push(KeyCommand::Undo);
        }
        if keys.key_y {
            commands.push(KeyCommand::Redo);
        }
        return commands;
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
    // Bild auf/ab und Pos1/Ende blättern immer, auch mit ausgewählter Region.
    if keys.page_up {
        commands.push(KeyCommand::PrevPage);
    }
    if keys.page_down {
        commands.push(KeyCommand::NextPage);
    }
    if keys.home {
        commands.push(KeyCommand::FirstPage);
    }
    if keys.end {
        commands.push(KeyCommand::LastPage);
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

/// Der leere Hauptbereich: großer Satz, darunter der zweite Weg.
fn empty_state(ui: &mut egui::Ui) {
    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() / 3.0);
            ui.label(RichText::new("🗁").size(48.0).weak());
            ui.add_space(8.0);
            ui.label(RichText::new(EMPTY_DOCUMENT_HINT).size(20.0));
            ui.add_space(4.0);
            ui.label(
                RichText::new("„🗁 Öffnen“ in der Leiste oben, Strg+O — oder eine PDF-Datei in dieses Fenster ziehen.")
                    .weak(),
            );
        });
    });
}

/// Zustand der Oberfläche.
pub struct RedactApp {
    pub state: AppState,
    selector: RectangleSelector,
    /// Läuft gerade eine Größenänderung an einem Eckgriff?
    resize: Option<HandleDrag>,
    /// Letzte Fehlermeldung; wird als roter Text in der Statuszeile gezeigt.
    error: Option<String>,
    /// Fläche des Hauptbereichs im letzten Frame — Grundlage für den
    /// Ablege-Hinweis, der über allem liegt.
    central_rect: Option<egui::Rect>,
    /// Gerasterte Seitenbilder; rechnet auf einem eigenen Thread.
    pages: PageCache,
    /// Helles oder dunkles Thema.
    pub theme: Theme,
    /// Zuletzt an egui übergebenes Thema — damit `set_visuals` nur bei einer
    /// Änderung läuft und nicht in jedem Bild.
    applied_theme: Option<Theme>,
    /// Seite des vorigen Bildes; wechselt sie, rollt die Miniaturspalte mit.
    shown_page: Option<usize>,
    /// Wurde das Schließen des Fensters bereits bestätigt?
    close_confirmed: bool,
    /// Rückfragen unterdrücken (nur für Tests ohne Bildschirm).
    ///
    /// Ein `rfd`-Dialog blockiert und braucht ein Fenster; im Test gibt es
    /// beides nicht.
    ask_before_discarding: bool,
    /// Eingabefeld der Passwortabfrage.
    ///
    /// Steht hier und nicht im [`AppState`]: es ist der halb getippte Text
    /// eines Eingabefelds, kein Zustand des Dokuments. Nach jedem Versuch wird
    /// es geleert, damit das Passwort nicht länger im Speicher steht als nötig.
    password_input: String,
}

impl Default for RedactApp {
    fn default() -> Self {
        Self::new(Config::default())
    }
}

impl RedactApp {
    /// Baut die Oberfläche mit den Einstellungen eines Aufrufs.
    ///
    /// Es ist dieselbe [`Config`], die `redact_pipeline::run` bekäme — Muster,
    /// Buchungsliste, Schwellwert, Polsterung, Ladegrenzen. Vorher reichte die
    /// Nahtstelle nur die Muster-IDs durch, und der Rest der Kommandozeile
    /// endete an der Fenstergrenze.
    pub fn new(config: Config) -> Self {
        // Das Thema kommt aus derselben `Config` wie alles andere; dort hat es
        // die Einstellungsdatei hineingelegt.
        let theme = Theme::from_name(&config.theme);
        Self {
            state: AppState::with_config(config),
            selector: RectangleSelector::new(),
            resize: None,
            error: None,
            central_rect: None,
            pages: PageCache::new(),
            theme,
            applied_theme: None,
            shown_page: None,
            close_confirmed: false,
            ask_before_discarding: true,
            password_input: String::new(),
        }
    }

    /// Ohne Rückfragen — für Tests ohne Bildschirm.
    #[cfg(test)]
    fn silent(config: Config) -> Self {
        Self {
            ask_before_discarding: false,
            ..Self::new(config)
        }
    }

    /// Was gerade im Passwortfeld steht (nur für Tests).
    #[cfg(test)]
    fn set_password_input(&mut self, password: &str) {
        self.password_input = password.to_string();
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
        let count = self.state.hand_made_count();
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

    /// Alles, was aus der Konfiguration **beim Start** folgt.
    ///
    /// Das ist die Stelle, an der `redact-rs --gui …` und `redact-rs …`
    /// auseinanderlaufen können, deshalb steht sie hier als gewöhnliche Methode
    /// und nicht in [`crate::run`] zwischen Fenster und Ereignisschleife: so ist
    /// sie ohne Bildschirm prüfbar.
    ///
    /// Reihenfolge wie in `redact_pipeline::run`: Dokument laden, analysieren —
    /// und wenn `--apply-review` gesetzt ist, tritt die Review-Datei an die
    /// Stelle der Analyse. Ihre Zugehörigkeit zum Dokument prüft dabei dieselbe
    /// Funktion wie auf der Kommandozeile
    /// ([`redact_pipeline::check_review_identity`], über
    /// [`crate::state::AppState::apply_review_file`]).
    pub fn open_startup_document(&mut self) {
        let input = self.state.config.input.clone();
        if !input.as_os_str().is_empty() {
            self.open_and_analyze(input);
        }

        let Some(review) = self.state.config.apply_review.clone() else {
            return;
        };
        if !self.state.is_loaded() {
            // Ohne Dokument gibt es nichts, worauf ein Review passen könnte —
            // und stillschweigend übergehen darf die Oberfläche den Schalter
            // nicht.
            self.report(Err(redact_core::RedactError::Config(
                "--apply-review braucht ein Dokument: bitte zuerst ein PDF öffnen.".into(),
            )));
            return;
        }
        self.load_review_file(&review);
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
        let result = self.state.analyze().map(|_| ());
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
        let analyzed = self.state.analyze().map(|_| ());
        self.report(analyzed);
    }

    /// Versucht, das wartende Dokument mit dem eingegebenen Passwort zu öffnen.
    ///
    /// Ohne egui, damit der Ablauf ohne Bildschirm prüfbar ist: das
    /// Eingabefeld wird in jedem Fall geleert (auch bei falschem Passwort —
    /// es soll nicht stehen bleiben), und geglückt geht es denselben Weg wie
    /// jedes andere Öffnen, also samt Rasterizer und Analyse.
    pub fn submit_password(&mut self) {
        let password = std::mem::take(&mut self.password_input);
        let unlocked = self.state.unlock(&password);
        self.after_loading(unlocked);
    }

    /// Die Passwortabfrage abbrechen.
    pub fn cancel_password(&mut self) {
        self.password_input.clear();
        self.state.cancel_password();
        self.error = None;
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
        let audit = self.state.audit_target(&out);
        let blocked = self.state.blocked_regions().len();
        match self.state.export(&out, Some(&audit)) {
            Ok(outcome) => {
                self.error = None;
                self.state.warnings = outcome.warnings.clone();
                self.state.status = export_status(
                    outcome.drawn_rects,
                    outcome.removed_glyphs,
                    &out,
                    &audit,
                    blocked,
                    outcome.warnings.first().map(String::as_str),
                );
            }
            Err(e) => self.report(Err(e)),
        }
    }

    // ------------------------------------------------------------- Zeichnen

    /// Zustand, von dem abhängt, welche Knöpfe benutzbar sind.
    fn tool_context(&self) -> ToolContext {
        ToolContext {
            loaded: self.state.is_loaded(),
            can_undo: self.state.can_undo(),
            can_redo: self.state.can_redo(),
            first_page: self.state.is_first_page(),
            last_page: self.state.is_last_page(),
            can_zoom_in: self.state.can_zoom_in(),
            can_zoom_out: self.state.can_zoom_out(),
        }
    }

    /// Die Symbolleiste: Symbol **und** Text je Knopf, dahinter der
    /// Zoomregler und der Themenschalter.
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let context = self.tool_context();
        let mut clicked: Option<ToolAction> = None;

        ui.horizontal_wrapped(|ui| {
            for item in toolbar::items() {
                match item {
                    ToolItem::Separator => {
                        ui.separator();
                    }
                    ToolItem::Button(button) => {
                        let enabled = toolbar::is_enabled(button.action, &context);
                        if ui
                            .add_enabled(enabled, egui::Button::new(button.label()))
                            .on_hover_text(button.hint)
                            .clicked()
                        {
                            clicked = Some(button.action);
                        }
                    }
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

            // Der Themenschalter gehört ans Ende: er ist Einstellung, nicht
            // Arbeitsschritt.
            ui.separator();
            if ui
                .button(format!(
                    "{} {}",
                    self.theme.switch_icon(),
                    self.theme.switch_text()
                ))
                .on_hover_text(format!(
                    "Zur {}-Darstellung wechseln (aktuell: {})",
                    self.theme.switch_text(),
                    self.theme.label()
                ))
                .clicked()
            {
                clicked = Some(ToolAction::ToggleTheme);
            }
        });

        if let Some(action) = clicked {
            self.apply_tool_action(action, ui.ctx());
        }
    }

    /// Führt aus, was ein Knopf der Symbolleiste bedeutet.
    ///
    /// Dieselbe Stelle bedient auch die Tastenkürzel — „Öffnen“ soll über
    /// Strg+O genau dasselbe tun wie über den Knopf.
    fn apply_tool_action(&mut self, action: ToolAction, ctx: &egui::Context) {
        match action {
            ToolAction::Open => self.open_dialog(),
            ToolAction::Analyze => self.analyze(),
            ToolAction::Booking => self.booking_dialog(),
            ToolAction::Export => self.export_dialog(),
            ToolAction::ReviewSave => self.review_save_dialog(),
            ToolAction::ReviewLoad => self.review_load_dialog(),
            // Über dieselbe Stelle wie Strg+Z/Strg+Y — dort endet auch ein
            // laufender Zug am Eckgriff, und das darf nicht davon abhängen, ob
            // der Knopf oder das Kürzel benutzt wurde.
            ToolAction::Undo => self.apply_key_commands(&[KeyCommand::Undo]),
            ToolAction::Redo => self.apply_key_commands(&[KeyCommand::Redo]),
            ToolAction::ZoomOut => self.state.zoom_out(),
            ToolAction::ZoomIn => self.state.zoom_in(),
            ToolAction::ZoomFit => {
                let view = self.state.current_page_view();
                let available = ctx.screen_rect().size() - CHROME_SIZE;
                self.state.set_zoom(viewer::fit_zoom(available, &view));
            }
            ToolAction::ZoomReset => self.state.zoom_reset(),
            ToolAction::PrevPage => self.state.prev_page(),
            ToolAction::NextPage => self.state.next_page(),
            ToolAction::ToggleTheme => self.theme = self.theme.toggled(),
        }
    }

    // -------------------------------------------------------------- Dialoge

    fn open_dialog(&mut self) {
        if !self.may_discard("Ein anderes PDF zu öffnen") {
            return;
        }
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("PDF", &["pdf"])
            .set_title("PDF öffnen")
            .pick_file()
        {
            self.open_and_analyze(path);
        }
    }

    fn booking_dialog(&mut self) {
        let mut dialog = rfd::FileDialog::new()
            .add_filter("CSV", &["csv"])
            .set_title("Buchungsliste laden");
        if let Some(dir) = self.state.dialog_directory() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.state.config.booking_list = Some(path);
            self.analyze();
        }
    }

    fn export_dialog(&mut self) {
        if !self.state.is_loaded() {
            self.state.status = "Erst ein PDF öffnen".to_string();
            return;
        }
        // Vorgabe: neben dem Original, Stamm + Namenszusatz.
        let dialog = save_dialog(
            "Geschwärztes PDF speichern",
            "PDF",
            &["pdf"],
            self.state.dialog_directory(),
            self.state.suggested_output_path(),
            "geschwaerzt.pdf",
        );
        if let Some(path) = dialog.save_file() {
            self.export_to(path);
        }
    }

    fn review_save_dialog(&mut self) {
        let dialog = save_dialog(
            "Review-Datei speichern",
            "JSON",
            &["json"],
            self.state.dialog_directory(),
            self.state.suggested_review_path(),
            "review.json",
        );
        if let Some(path) = dialog.save_file() {
            let result = self.state.save_review_file(&path);
            self.report(result);
        }
    }

    fn review_load_dialog(&mut self) {
        if !self.may_discard("Ein Review zu laden ersetzt die Trefferliste und") {
            return;
        }
        let mut dialog = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .set_title("Review-Datei laden");
        if let Some(dir) = self.state.dialog_directory() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.load_review_file(&path);
        }
    }

    /// Liest eine Review-Datei und übernimmt sie — **wenn** sie zum geladenen
    /// Dokument gehört.
    ///
    /// Passt die Prüfsumme nicht, meldet [`AppState::apply_review_file`] einen
    /// Fehler; der landet rot in der Statuszeile und **zusätzlich** in einem
    /// Hinweisfenster. Eine Zeile am unteren Rand ginge hier zu leicht unter:
    /// wer eine fremde Review-Datei anwendet, bekommt ein Ergebnis, das
    /// geschwärzt aussieht und keines ist.
    pub fn load_review_file(&mut self, path: &std::path::Path) {
        let result = std::fs::read_to_string(path)
            .map_err(redact_core::RedactError::from)
            .and_then(|data| ReviewFile::from_json(&data))
            .and_then(|review| self.state.apply_review_file(review));
        if let Err(error) = &result {
            let message = error.to_string();
            if self.ask_before_discarding {
                rfd::MessageDialog::new()
                    .set_level(rfd::MessageLevel::Error)
                    .set_title("Review-Datei passt nicht zum Dokument")
                    .set_description(&message)
                    .set_buttons(rfd::MessageButtons::Ok)
                    .show();
            }
        }
        self.report(result);
    }

    /// Die Passwortabfrage für ein verschlüsseltes Dokument.
    ///
    /// Bewusst ein eigenes Fenster und kein `rfd`-Dialog: `rfd` kann keine
    /// verdeckte Eingabe, das Passwort stünde also im Klartext auf dem Schirm.
    ///
    /// Die Entscheidungen stecken in [`RedactApp::submit_password`] und
    /// [`RedactApp::cancel_password`] — hier steht nur das Fenster, damit der
    /// Ablauf ohne Bildschirm geprüft werden kann.
    fn password_dialog(&mut self, ctx: &egui::Context) {
        if !self.state.needs_password() {
            return;
        }
        let name = self.state.pending_name();
        let error = self.error.clone();
        // Herausnehmen und zurücklegen: sonst wäre `self` zweimal geliehen.
        let mut input = std::mem::take(&mut self.password_input);
        let (mut submit, mut cancel) = (false, false);

        egui::Window::new("Passwort erforderlich")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!("„{name}“ ist verschlüsselt."));
                ui.label(
                    RichText::new(
                        "Das Passwort dient nur zum Öffnen. Es wird nirgends \
                         gespeichert und steht in keiner Ausgabedatei, in \
                         keinem Audit-Log und in keiner Meldung.",
                    )
                    .weak(),
                );
                ui.add_space(BAR_PADDING);

                let field = ui.add(
                    egui::TextEdit::singleline(&mut input)
                        .password(true)
                        .hint_text("Passwort"),
                );
                // Nur greifen, wenn gerade nichts anderes den Fokus hat —
                // sonst risse das Feld ihn den Knöpfen bei jedem Bild weg.
                if ui.memory(|m| m.focused().is_none()) {
                    field.request_focus();
                }
                submit = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));

                ui.add_space(BAR_PADDING);
                ui.horizontal(|ui| {
                    submit |= ui.button("Öffnen").clicked();
                    cancel = ui.button("Abbrechen").clicked();
                });

                if let Some(message) = &error {
                    ui.label(RichText::new(message).color(Color32::from_rgb(220, 60, 60)));
                }
            });

        self.password_input = input;
        if submit {
            self.submit_password();
        } else if cancel {
            self.cancel_password();
        }
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
        self.apply_pointer(
            PointerFrame::from_response(response),
            response.clicked(),
            view,
            origin,
            page,
            zoom,
        );
        self.show_handle_cursor(response, view, origin, page, zoom);
    }

    /// Das Bildschirmrechteck der ausgewählten Region, sofern sie auf dieser
    /// Seite liegt.
    fn selected_screen_rect(
        &self,
        view: &viewer::PageView,
        origin: Pos2,
        page: usize,
        zoom: f32,
    ) -> Option<(usize, egui::Rect)> {
        let index = self.state.selected_region?;
        let entry = self.state.regions.get(index)?;
        (entry.region.page == page).then(|| {
            (
                index,
                viewer::pdf_to_screen(&entry.region.rect, view, zoom, origin),
            )
        })
    }

    /// Alles, was die Maus im Seitenbild bewirkt — als reine Zustandsänderung
    /// ohne `Response`, damit die **Reihenfolge der Fälle** ohne Bildschirm
    /// prüfbar ist.
    ///
    /// Und die Reihenfolge ist hier die eigentliche Aussage: ein Zug, der auf
    /// einem Eckgriff der ausgewählten Region beginnt, ändert diese Region.
    /// Erst wenn kein Griff im Spiel ist, legt ein Zug ein neues Rechteck an.
    /// Ohne diesen Vorrang entstünde bei jedem Korrekturversuch ein zweites
    /// Rechteck über dem ersten.
    fn apply_pointer(
        &mut self,
        frame: PointerFrame,
        clicked: bool,
        view: &viewer::PageView,
        origin: Pos2,
        page: usize,
        zoom: f32,
    ) {
        // 1. Eine laufende Größenänderung hat Vorrang vor allem anderen.
        if let Some(drag) = self.resize {
            // Die Region wird über ihre Kennung gesucht, nicht über einen
            // gemerkten Platz in der Liste: zwischen zwei Bildern kann gelöscht,
            // zurückgenommen oder ein Review geladen worden sein. Ist sie weg,
            // endet der Zug — er darf auf keinen Fall auf die Region
            // weiterlaufen, die jetzt an ihrer Stelle steht.
            let Some(index) = self.state.index_of(drag.region) else {
                self.resize = None;
                self.selector.cancel();
                self.state.status =
                    "Zug beendet — die angefasste Region gibt es nicht mehr".to_string();
                return;
            };
            if let Some(pos) = frame.pos.filter(|_| frame.dragged || frame.drag_stopped) {
                let corner = viewer::screen_to_pdf_point(pos, view, zoom, origin);
                // `from_corners` normalisiert: zieht man über die Gegenecke
                // hinaus, entsteht kein negatives Rechteck, sondern ein
                // gespiegeltes.
                self.state
                    .set_region_rect(index, redact_core::Rect::from_corners(drag.anchor, corner));
            }
            if !frame.dragged {
                // Losgelassen oder abgebrochen — der Zug ist vorbei.
                self.resize = None;
                self.state.status = "Rechteck angepasst".to_string();
            }
            return;
        }

        // 2. Beginnt der Zug auf einem Eckgriff der ausgewählten Region?
        if frame.drag_started {
            if let Some(drag) = self.grab_handle(&frame, view, origin, page, zoom) {
                // Der Zug gehört jetzt dem Griff; der Selektor darf ihn nicht
                // zusätzlich als neues Rechteck sehen.
                self.selector.cancel();
                self.state.begin_manual_edit();
                self.resize = Some(drag);
                // Das erste Bild gleich mitnehmen, sonst hinkt die Anzeige.
                self.apply_pointer(frame, false, view, origin, page, zoom);
                return;
            }
        }

        // 3. Klick wählt aus (oder hebt die Auswahl auf).
        if clicked {
            if let Some(pos) = frame.pos {
                // Ein Klick auf einen Griff der ausgewählten Region behält die
                // Auswahl: sonst verschwänden die Griffe genau dann, wenn man
                // sie anfasst.
                let on_handle = self
                    .selected_screen_rect(view, origin, page, zoom)
                    .and_then(|(_, rect)| hit_handle(rect, pos))
                    .is_some();
                if !on_handle {
                    let point = viewer::screen_to_pdf_point(pos, view, zoom, origin);
                    self.state.selected_region = hit_test(&self.state.regions, page, point);
                }
            }
        }

        // 4. Ziehen legt eine manuelle Region an.
        if let Some((a, b)) = self.selector.step(frame) {
            let rect = viewer::screen_to_pdf(a, b, view, zoom, origin);
            self.state.add_manual_region(page, rect, "manuell markiert");
        }
    }

    /// Liegt der **Druckpunkt** auf einem Eckgriff der ausgewählten Region?
    ///
    /// Der festgehaltene Punkt ist die gegenüberliegende **Bildschirmecke**,
    /// in den User-Space zurückgerechnet. Damit stimmt die Zuordnung für jede
    /// `/Rotate`-Stellung, ohne dass irgendwo eine Tabelle „welche
    /// Bildschirmecke ist welche PDF-Ecke“ gepflegt werden müsste.
    fn grab_handle(
        &self,
        frame: &PointerFrame,
        view: &viewer::PageView,
        origin: Pos2,
        page: usize,
        zoom: f32,
    ) -> Option<HandleDrag> {
        let press = frame.press_point()?;
        let (index, screen) = self.selected_screen_rect(view, origin, page, zoom)?;
        let handle = hit_handle(screen, press)?;
        let anchor = viewer::screen_to_pdf_point(handle.opposite().pos(screen), view, zoom, origin);
        Some(HandleDrag {
            region: self.state.id_at(index)?,
            anchor,
        })
    }

    /// Zeigt am Mauszeiger, dass ein Griff angefasst werden kann.
    fn show_handle_cursor(
        &self,
        response: &egui::Response,
        view: &viewer::PageView,
        origin: Pos2,
        page: usize,
        zoom: f32,
    ) {
        let Some(pos) = response
            .hover_pos()
            .or_else(|| response.interact_pointer_pos())
        else {
            return;
        };
        let Some((_, screen)) = self.selected_screen_rect(view, origin, page, zoom) else {
            return;
        };
        if let Some(handle) = hit_handle(screen, pos) {
            response.ctx.set_cursor_icon(handle.cursor_icon());
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
            home: i.key_pressed(Key::Home),
            end: i.key_pressed(Key::End),
            shift: i.modifiers.shift,
            // `command` ist unter macOS die Befehlstaste, sonst Strg — das
            // erwartet man dort so.
            ctrl: i.modifiers.command,
            key_o: i.key_pressed(Key::O),
            key_s: i.key_pressed(Key::S),
            key_z: i.key_pressed(Key::Z),
            key_y: i.key_pressed(Key::Y),
            text_focus,
        })
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let keys = Self::read_keys(ctx);
        let commands = key_commands(keys, self.state.selected_region.is_some());
        self.apply_key_commands(&commands);
    }

    fn apply_key_commands(&mut self, commands: &[KeyCommand]) {
        for command in commands {
            match *command {
                KeyCommand::Deselect => {
                    self.selector.cancel();
                    // Auch eine laufende Größenänderung endet hier — sonst
                    // folgte das Rechteck weiter der Maus, obwohl seine Region
                    // gar nicht mehr ausgewählt ist. Zurückgesetzt wird nichts:
                    // dafür gibt es Rückgängig.
                    self.resize = None;
                    self.state.selected_region = None;
                }
                KeyCommand::DeleteSelected => {
                    // Wie bei `Deselect`: gelöscht wird die Region, an der
                    // womöglich gerade gezogen wird. Die Kennung im
                    // [`HandleDrag`] verhinderte zwar schon, dass der Zug auf
                    // eine fremde Region überspringt — aber ein Zug ohne Ziel
                    // hat nichts mehr zu suchen, und die Statuszeile soll nicht
                    // beim nächsten Mausbild noch „Rechteck angepasst“ melden.
                    self.resize = None;
                    self.state.delete_selected();
                }
                KeyCommand::Move { dx, dy } => {
                    self.state.move_selected(dx, dy);
                }
                KeyCommand::PrevPage => self.state.prev_page(),
                KeyCommand::NextPage => self.state.next_page(),
                KeyCommand::FirstPage => self.state.first_page(),
                KeyCommand::LastPage => self.state.last_page(),
                KeyCommand::Undo => {
                    // Rückgängig tauscht die **ganze** Trefferliste aus und hebt
                    // die Auswahl auf. Ein Zug, der weiterliefe, schriebe den
                    // gerade zurückgenommenen Stand sofort wieder — die Kennung
                    // findet die Region ja wieder. Also endet er hier.
                    self.resize = None;
                    self.state.undo();
                    self.error = None;
                }
                KeyCommand::Redo => {
                    // Dasselbe in der anderen Richtung.
                    self.resize = None;
                    self.state.redo();
                    self.error = None;
                }
                KeyCommand::Open => self.open_dialog(),
                KeyCommand::Export => self.export_dialog(),
            }
        }
    }
}

impl eframe::App for RedactApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Thema nur bei Änderung setzen — `set_visuals` kopiert den ganzen Stil.
        if self.applied_theme != Some(self.theme) {
            ctx.set_visuals(self.theme.visuals());
            self.applied_theme = Some(self.theme);
        }

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

        // Miniaturansichten ganz links; sie leben von denselben Kleinbildern,
        // die der Hauptbereich ohnehin anfordert.
        let page_changed = self.shown_page != Some(self.state.current_page);
        self.shown_page = Some(self.state.current_page);
        egui::SidePanel::left("thumbnails")
            .default_width(crate::thumbnails::PANEL_WIDTH)
            .width_range(crate::thumbnails::PANEL_MIN_WIDTH..=crate::thumbnails::PANEL_MAX_WIDTH)
            .show(ctx, |ui| {
                crate::thumbnails::show(ui, &mut self.state, &mut self.pages, page_changed);
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
                empty_state(ui);
            }
        });

        // Die Passwortabfrage liegt über den Panels und vor der Tastatur: sie
        // hat ein Textfeld, und solange sie offen ist, gehören alle Tasten
        // dorthin.
        self.password_dialog(ctx);

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

    use std::path::Path;

    use redact_core::Rect;

    /// Einstellungen, wie sie `redact-rs --gui --patterns iban_de` erzeugt.
    fn iban_only() -> Config {
        Config {
            patterns: vec!["iban_de".to_string()],
            ..Config::default()
        }
    }

    // ----------------------------------------------------------- Tastatur

    /// A1: Steht der Fokus in einem Textfeld, darf **keine** Taste bis in den
    /// Zustand durchschlagen. Vorher löschte die Rücktaste im Feld „Ersetzen“
    /// die ausgewählte Region — ohne Rückfrage und ohne Rückgängig.
    #[test]
    fn a_focused_text_field_swallows_every_key() {
        // Wirklich jede Taste, auch die neuen Kürzel: Strg+Z im Textfeld
        // gehört dem Textfeld.
        let every_key = KeyState {
            delete: true,
            escape: true,
            left: true,
            right: true,
            up: true,
            down: true,
            page_up: true,
            page_down: true,
            home: true,
            end: true,
            shift: true,
            ctrl: true,
            key_o: true,
            key_s: true,
            key_z: true,
            key_y: true,
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
        // Und ohne Steuerungstaste ebenfalls.
        let without_ctrl = KeyState {
            ctrl: false,
            ..unfocused
        };
        assert!(!key_commands(without_ctrl, true).is_empty());
    }

    /// Und derselbe Fall einmal ganz konkret, mit echtem Zustand.
    #[test]
    fn backspace_in_a_text_field_does_not_delete_the_selected_region() {
        let mut app = RedactApp::silent(Config::default());
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

    /// Pos1 und Ende blättern an die Enden — auch mit ausgewählter Region.
    #[test]
    fn home_and_end_always_jump_to_the_first_and_last_page() {
        let home = KeyState {
            home: true,
            ..KeyState::default()
        };
        let end = KeyState {
            end: true,
            ..KeyState::default()
        };
        assert_eq!(key_commands(home, false), vec![KeyCommand::FirstPage]);
        assert_eq!(key_commands(end, true), vec![KeyCommand::LastPage]);
    }

    /// Die Kürzel mit Steuerungstaste stehen für sich: Strg+Z ist Rückgängig
    /// und blättert nicht nebenbei.
    #[test]
    fn control_shortcuts_are_exclusive() {
        let with_ctrl = |o, s, z, y| KeyState {
            ctrl: true,
            key_o: o,
            key_s: s,
            key_z: z,
            key_y: y,
            ..KeyState::default()
        };
        assert_eq!(
            key_commands(with_ctrl(true, false, false, false), false),
            vec![KeyCommand::Open]
        );
        assert_eq!(
            key_commands(with_ctrl(false, true, false, false), false),
            vec![KeyCommand::Export]
        );
        assert_eq!(
            key_commands(with_ctrl(false, false, true, false), true),
            vec![KeyCommand::Undo]
        );
        assert_eq!(
            key_commands(with_ctrl(false, false, false, true), true),
            vec![KeyCommand::Redo]
        );

        // Strg + Pfeiltaste verschiebt nichts und blättert nicht — das Kürzel
        // gehört dem Betriebssystem bzw. der Textnavigation.
        let ctrl_and_arrow = KeyState {
            ctrl: true,
            left: true,
            delete: true,
            ..KeyState::default()
        };
        assert!(key_commands(ctrl_and_arrow, true).is_empty());

        // Ohne Steuerungstaste sind O, S, Z und Y gewöhnliche Buchstaben.
        let letters = KeyState {
            key_o: true,
            key_s: true,
            key_z: true,
            key_y: true,
            ..KeyState::default()
        };
        assert!(key_commands(letters, true).is_empty());
    }

    /// Strg+Z und Strg+Y wirken auf den echten Zustand.
    #[test]
    fn undo_and_redo_run_through_the_keyboard() {
        let mut app = RedactApp::silent(Config::default());
        app.state
            .add_manual_region(0, Rect::new(10.0, 10.0, 50.0, 20.0), "Gehalt");
        assert_eq!(app.state.regions.len(), 1);

        let ctrl_z = KeyState {
            ctrl: true,
            key_z: true,
            ..KeyState::default()
        };
        app.apply_key_commands(&key_commands(ctrl_z, false));
        assert!(app.state.regions.is_empty(), "Strg+Z nimmt zurück");

        let ctrl_y = KeyState {
            ctrl: true,
            key_y: true,
            ..KeyState::default()
        };
        app.apply_key_commands(&key_commands(ctrl_y, false));
        assert_eq!(app.state.regions.len(), 1, "Strg+Y stellt wieder her");
    }

    // ------------------------------------------------------ Maus und Eckgriffe

    /// MediaBox, deren linke untere Ecke **nicht** im Ursprung liegt.
    fn offset_box() -> Rect {
        Rect::new(10.0, 20.0, 605.0, 862.0)
    }

    /// Linke obere Ecke des Blatts auf dem Bildschirm.
    const ORIGIN: Pos2 = Pos2::new(37.0, 91.0);
    const ZOOM: f32 = 1.5;

    /// Eine Anwendung mit genau einer — ausgewählten — Region und deren
    /// Bildschirmrechteck.
    fn app_with_selected_region(view: &viewer::PageView, rect: Rect) -> (RedactApp, egui::Rect) {
        let mut app = RedactApp::silent(Config::default());
        app.state.add_manual_region(0, rect, "test");
        assert_eq!(app.state.selected_region, Some(0));
        (app, viewer::pdf_to_screen(&rect, view, ZOOM, ORIGIN))
    }

    /// Fährt einen vollständigen Ziehvorgang ab: drücken bei `press`, über die
    /// Punkte in `path` ziehen, am letzten loslassen.
    ///
    /// Bildet nach, was egui wirklich liefert: `drag_started` kommt **nach**
    /// dem Druck (der Zeiger ist dann schon weiter), und beim Loslassen ist
    /// `press_origin` bereits wieder `None`.
    fn drag(app: &mut RedactApp, view: &viewer::PageView, press: Pos2, path: &[Pos2]) {
        let (last, rest) = path.split_last().expect("mindestens ein Punkt");
        assert!(!rest.is_empty(), "Ziehen braucht mindestens zwei Punkte");
        for (i, pos) in rest.iter().enumerate() {
            app.apply_pointer(
                PointerFrame {
                    drag_started: i == 0,
                    dragged: true,
                    pos: Some(*pos),
                    press_origin: Some(press),
                    ..PointerFrame::default()
                },
                false,
                view,
                ORIGIN,
                0,
                ZOOM,
            );
        }
        app.apply_pointer(
            PointerFrame {
                drag_stopped: true,
                pos: Some(*last),
                ..PointerFrame::default()
            },
            false,
            view,
            ORIGIN,
            0,
            ZOOM,
        );
    }

    fn assert_rect_close(got: Rect, want: Rect, what: &str) {
        let close = |a: f64, b: f64| (a - b).abs() < 0.01;
        assert!(
            close(got.ll.x, want.ll.x)
                && close(got.ll.y, want.ll.y)
                && close(got.ur.x, want.ur.x)
                && close(got.ur.y, want.ur.y),
            "{what}: {got:?} != {want:?}"
        );
    }

    /// Ist `point` eine der vier Ecken von `rect`?
    fn is_corner_of(rect: Rect, point: redact_core::Point) -> bool {
        let close = |a: f64, b: f64| (a - b).abs() < 0.01;
        (close(point.x, rect.ll.x) || close(point.x, rect.ur.x))
            && (close(point.y, rect.ll.y) || close(point.y, rect.ur.y))
    }

    /// **Fehler 2.** An einem Eckgriff der ausgewählten Region zu ziehen ändert
    /// diese Region, statt eine neue anzulegen — die gegenüberliegende
    /// **Bildschirm**ecke bleibt dabei stehen. Geprüft für alle vier Griffe,
    /// alle vier `/Rotate`-Werte und eine MediaBox mit Ursprung ≠ (0,0): auf
    /// einer gedrehten Seite gehört zu „links oben auf dem Bildschirm“ jedes
    /// Mal eine andere Ecke im User-Space.
    #[test]
    fn dragging_a_corner_handle_resizes_the_region_for_every_rotation() {
        let rect = Rect::new(100.0, 300.0, 260.0, 340.0);
        for rotate in [0_i64, 90, 180, 270] {
            let view = viewer::PageView::new(offset_box(), rotate);
            for handle in crate::selector::HANDLES {
                let (mut app, screen) = app_with_selected_region(&view, rect);
                let corner = handle.pos(screen);
                // Ein paar Punkte neben der Ecke gedrückt — die Trefferzone ist
                // absichtlich größer als das gezeichnete Quadrat.
                let press = corner + Vec2::new(3.0, -3.0);
                let target = corner + Vec2::new(37.0, 23.0);
                drag(
                    &mut app,
                    &view,
                    press,
                    &[press + Vec2::new(7.0, 7.0), target],
                );

                let what = format!("rot={rotate} {handle:?}");
                assert!(app.resize.is_none(), "{what}: Zug muss beendet sein");
                assert_eq!(
                    app.state.regions.len(),
                    1,
                    "{what}: es darf kein zweites Rechteck entstehen"
                );

                let fixed =
                    viewer::screen_to_pdf_point(handle.opposite().pos(screen), &view, ZOOM, ORIGIN);
                let moved = viewer::screen_to_pdf_point(target, &view, ZOOM, ORIGIN);
                assert!(
                    is_corner_of(rect, fixed),
                    "{what}: der festgehaltene Punkt {fixed:?} ist keine Ecke von {rect:?}"
                );
                assert_rect_close(
                    app.state.regions[0].region.rect,
                    Rect::from_corners(fixed, moved),
                    &what,
                );
            }
        }
    }

    /// Gegenprobe: **ohne** Auswahl gibt es keine Griffe, und genau derselbe
    /// Zug legt wieder ein neues Rechteck an. Das ist die Aussage „Griff
    /// schlägt Neuanlage“ — ohne diese Gegenprobe bewiese der Test oben nur,
    /// dass irgendetwas nichts anlegt.
    #[test]
    fn the_same_drag_draws_a_new_rectangle_when_nothing_is_selected() {
        let view = viewer::PageView::upright(offset_box());
        let rect = Rect::new(100.0, 300.0, 260.0, 340.0);
        let (mut app, screen) = app_with_selected_region(&view, rect);
        app.state.selected_region = None;

        let press = screen.left_top() + Vec2::new(3.0, -3.0);
        let target = press + Vec2::new(60.0, 40.0);
        drag(
            &mut app,
            &view,
            press,
            &[press + Vec2::new(7.0, 7.0), target],
        );

        assert_eq!(app.state.regions.len(), 2, "hier entsteht ein Rechteck");
        assert_eq!(
            app.state.regions[0].region.rect, rect,
            "die alte Region bleibt unverändert"
        );
    }

    /// Zieht man über die Gegenecke hinaus, wird das Rechteck normalisiert —
    /// negative Breiten oder Höhen darf es nicht geben.
    #[test]
    fn dragging_past_the_opposite_corner_normalizes_the_rectangle() {
        let view = viewer::PageView::upright(offset_box());
        let rect = Rect::new(100.0, 300.0, 260.0, 340.0);
        let (mut app, screen) = app_with_selected_region(&view, rect);

        // Am Griff links oben weit über die Ecke rechts unten hinausziehen.
        let press = screen.left_top();
        let target = screen.right_bottom() + Vec2::new(60.0, 45.0);
        drag(
            &mut app,
            &view,
            press,
            &[press + Vec2::new(7.0, 7.0), target],
        );

        let got = app.state.regions[0].region.rect;
        assert!(got.width() > 0.0 && got.height() > 0.0, "{got:?}");
        assert!(got.ll.x < got.ur.x && got.ll.y < got.ur.y, "{got:?}");
        // Die feste Ecke (rechts unten auf dem Bildschirm) ist jetzt die linke
        // obere des neuen Rechtecks.
        let fixed = viewer::screen_to_pdf_point(screen.right_bottom(), &view, ZOOM, ORIGIN);
        let moved = viewer::screen_to_pdf_point(target, &view, ZOOM, ORIGIN);
        assert_rect_close(got, Rect::from_corners(fixed, moved), "gespiegelt");
        assert!(
            moved.x > fixed.x && moved.y < fixed.y,
            "wirklich darüber hinaus"
        );
    }

    /// Ein Ziehvorgang ist **ein** Schritt im Rückgängig-Stapel, nicht einer je
    /// Bild — sonst führte „Rückgängig“ nach einer Sekunde Ziehen bloß einen
    /// Mauszuck weit zurück.
    #[test]
    fn one_handle_drag_is_exactly_one_undo_step() {
        let view = viewer::PageView::upright(offset_box());
        let rect = Rect::new(100.0, 300.0, 260.0, 340.0);
        let (mut app, screen) = app_with_selected_region(&view, rect);
        let before = app.state.history.undo_depth();

        // Zwanzig Bilder Mausbewegung.
        let press = screen.right_bottom();
        let path: Vec<Pos2> = (1..=20)
            .map(|i| press + Vec2::new(i as f32 * 3.0, i as f32 * 2.0))
            .collect();
        drag(&mut app, &view, press, &path);

        assert_eq!(
            app.state.history.undo_depth(),
            before + 1,
            "je Ziehvorgang genau ein Schnappschuss"
        );
        assert_ne!(
            app.state.regions[0].region.rect, rect,
            "das Rechteck muss sich geändert haben"
        );
        assert!(app.state.undo());
        assert_rect_close(
            app.state.regions[0].region.rect,
            rect,
            "ein Rückgängig führt zum Stand vor der Korrektur",
        );
    }

    // ------------------------------ Eingriffe mitten im Zug (Befund #65)

    /// Ein einzelnes Mausbild mitten im Zug.
    fn dragging_frame(press: Pos2, pos: Pos2, started: bool) -> PointerFrame {
        PointerFrame {
            drag_started: started,
            dragged: true,
            pos: Some(pos),
            press_origin: Some(press),
            ..PointerFrame::default()
        }
    }

    /// Zwei Regionen auf Seite 0: eine von Hand gezogene (ausgewählt, an ihrem
    /// Griff wird gleich gezogen) und ein **unbeteiligter** Musterfund.
    ///
    /// Der Musterfund ist mit Absicht keine manuelle Region: `set_region_rect`
    /// macht aus einem Treffer, den man anfasst, eine Handarbeit — an ihm ist
    /// also auch zu sehen, ob der Zug ihn überhaupt berührt hat.
    fn app_with_a_bystander(view: &viewer::PageView) -> (RedactApp, egui::Rect, Rect) {
        let mine = Rect::new(100.0, 300.0, 260.0, 340.0);
        let bystander = Rect::new(300.0, 500.0, 460.0, 540.0);
        let mut app = RedactApp::silent(Config::default());
        app.state.add_manual_region(0, mine, "meins");
        app.state.regions.push(crate::state::AnnotatedRegion::new(
            redact_core::Region::new(
                0,
                bystander,
                Some("DE89 3704 0044 0532 0130 00".into()),
                redact_core::Source::Pattern {
                    pattern_id: "iban_de".into(),
                    confidence: 0.99,
                },
            ),
        ));
        app.state.selected_region = Some(0);
        (
            app,
            viewer::pdf_to_screen(&mine, view, ZOOM, ORIGIN),
            bystander,
        )
    }

    /// Der unbeteiligte Musterfund ist unangetastet: Rechteck, Herkunft und
    /// Farbe wie angelegt.
    #[track_caller]
    fn assert_bystander_untouched(app: &RedactApp, index: usize, rect: Rect) {
        let entry = &app.state.regions[index];
        assert_eq!(
            entry.region.rect, rect,
            "die unbeteiligte Region ist auf das Ziehrechteck gesprungen — \
             ihre Fläche würde nicht mehr geschwärzt"
        );
        assert!(
            matches!(entry.region.source, redact_core::Source::Pattern { .. }),
            "aus dem Musterfund wurde eine Handarbeit: {:?}",
            entry.region.source
        );
        assert_eq!(entry.color, RegionColor::AutoPattern);
    }

    /// **Befund #65.** Entf mitten im Zug am Eckgriff traf die falsche Region:
    /// gemerkt war ein *Index*, und nach dem Löschen zeigte der auf die
    /// nachrückende Region. Sie sprang auf das Ziehrechteck, ihre eigentliche
    /// Fläche blieb ungeschwärzt — ohne dass irgendetwas es gesagt hätte.
    #[test]
    fn deleting_during_a_handle_drag_leaves_every_other_region_alone() {
        let view = viewer::PageView::upright(offset_box());
        let (mut app, screen, bystander) = app_with_a_bystander(&view);

        // Zug am Griff unten rechts der ausgewählten Region.
        let press = screen.right_bottom();
        app.apply_pointer(
            dragging_frame(press, press + Vec2::new(7.0, 7.0), true),
            false,
            &view,
            ORIGIN,
            0,
            ZOOM,
        );
        assert!(app.resize.is_some(), "der Zug am Griff muss laufen");

        // Entf, mitten im Zug.
        let delete = KeyState {
            delete: true,
            ..KeyState::default()
        };
        app.apply_key_commands(&key_commands(delete, true));
        assert_eq!(app.state.regions.len(), 1, "die Auswahl ist gelöscht");
        assert!(app.resize.is_none(), "ein Zug ohne Ziel muss enden");

        // Weiterziehen — genau hier sprang früher die fremde Region mit.
        app.apply_pointer(
            dragging_frame(press, press + Vec2::new(120.0, 80.0), false),
            false,
            &view,
            ORIGIN,
            0,
            ZOOM,
        );
        assert_bystander_untouched(&app, 0, bystander);
        assert_eq!(app.state.regions.len(), 1, "es entsteht auch nichts Neues");
    }

    /// Der Zwillingsfall: Rückgängig mitten im Zug. Die Region gibt es danach
    /// noch, der Zug fände sie also wieder — und schriebe den gerade
    /// zurückgenommenen Stand sofort erneut. Deshalb endet er.
    #[test]
    fn undo_during_a_handle_drag_ends_it_instead_of_writing_the_drag_back() {
        let view = viewer::PageView::upright(offset_box());
        let rect = Rect::new(100.0, 300.0, 260.0, 340.0);
        let (mut app, screen) = app_with_selected_region(&view, rect);

        let press = screen.right_bottom();
        app.apply_pointer(
            dragging_frame(press, press + Vec2::new(30.0, 20.0), true),
            false,
            &view,
            ORIGIN,
            0,
            ZOOM,
        );
        assert!(app.resize.is_some());
        assert_ne!(
            app.state.regions[0].region.rect, rect,
            "der Zug hat das Rechteck geändert"
        );

        let ctrl_z = KeyState {
            ctrl: true,
            key_z: true,
            ..KeyState::default()
        };
        app.apply_key_commands(&key_commands(ctrl_z, true));
        assert!(app.resize.is_none(), "Rückgängig beendet den Zug");
        assert_eq!(
            app.state.regions[0].region.rect, rect,
            "Rückgängig führt zum Stand vor der Korrektur"
        );

        // Und die Maus bewegt sich weiter.
        app.apply_pointer(
            dragging_frame(press, press + Vec2::new(90.0, 60.0), false),
            false,
            &view,
            ORIGIN,
            0,
            ZOOM,
        );
        assert_eq!(
            app.state.regions[0].region.rect, rect,
            "der Zug lief nach dem Rückgängig weiter und hob es wieder auf"
        );
    }

    /// Die Absicherung, die **nicht** davon abhängt, dass jemand an den
    /// laufenden Zug denkt.
    ///
    /// Der Knopf „Region löschen“ in der Seitenleiste ruft
    /// [`AppState::delete_selected`] und kommt an `RedactApp::resize` gar nicht
    /// heran (`sidebar.rs`) — genauso wenig wie eine erneute Analyse oder ein
    /// geladenes Review, die die Liste austauschen. Dass der Zug trotzdem nicht
    /// auf die nachrückende Region springt, liegt allein an der Kennung im
    /// [`HandleDrag`]: sie zeigt entweder auf dieselbe Region oder auf keine.
    #[test]
    fn a_list_change_the_drag_never_heard_about_cannot_redirect_it() {
        let view = viewer::PageView::upright(offset_box());
        let (mut app, screen, bystander) = app_with_a_bystander(&view);

        let press = screen.right_bottom();
        app.apply_pointer(
            dragging_frame(press, press + Vec2::new(7.0, 7.0), true),
            false,
            &view,
            ORIGIN,
            0,
            ZOOM,
        );

        // Wie der Knopf in der Seitenleiste: die Liste wird kürzer, vom Zug
        // weiß niemand etwas.
        app.state.delete_selected();
        assert!(app.resize.is_some(), "der Zug läuft ahnungslos weiter");

        app.apply_pointer(
            dragging_frame(press, press + Vec2::new(120.0, 80.0), false),
            false,
            &view,
            ORIGIN,
            0,
            ZOOM,
        );
        assert_bystander_untouched(&app, 0, bystander);
        assert!(app.resize.is_none(), "der Zug beendet sich selbst");
        assert!(
            app.state.status.contains("gibt es nicht mehr"),
            "und sagt es: {}",
            app.state.status
        );
    }

    /// Ein Klick auf einen Griff behält die Auswahl — sonst verschwänden die
    /// Griffe genau dann, wenn man sie anfasst.
    #[test]
    fn clicking_a_handle_keeps_the_selection() {
        let view = viewer::PageView::upright(offset_box());
        let rect = Rect::new(100.0, 300.0, 260.0, 340.0);
        let (mut app, screen) = app_with_selected_region(&view, rect);

        let click = |app: &mut RedactApp, pos: Pos2| {
            app.apply_pointer(
                PointerFrame {
                    pos: Some(pos),
                    ..PointerFrame::default()
                },
                true,
                &view,
                ORIGIN,
                0,
                ZOOM,
            );
        };

        // Knapp außerhalb der Region, aber auf dem Griff.
        click(&mut app, screen.left_top() + Vec2::new(-4.0, -4.0));
        assert_eq!(app.state.selected_region, Some(0));

        // Weit daneben hebt die Auswahl dagegen auf.
        click(&mut app, screen.left_top() + Vec2::new(-80.0, -80.0));
        assert_eq!(app.state.selected_region, None);
    }

    // ------------------------------------------------------- Symbolleiste

    /// Die Knöpfe der Leiste wirken auf den Zustand — hier ohne Fenster, über
    /// dieselbe Stelle, die auch der Klick benutzt.
    #[test]
    fn toolbar_actions_change_the_state() {
        let ctx = egui::Context::default();
        let mut app = RedactApp::silent(iban_only());
        app.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "demo.pdf");
        assert!(app.state.is_loaded());

        // Blättern.
        app.apply_tool_action(ToolAction::NextPage, &ctx);
        assert_eq!(app.state.current_page, 1);
        app.apply_tool_action(ToolAction::PrevPage, &ctx);
        assert_eq!(app.state.current_page, 0);

        // Zoom.
        app.apply_tool_action(ToolAction::ZoomIn, &ctx);
        assert!(app.state.zoom > 1.0);
        app.apply_tool_action(ToolAction::ZoomReset, &ctx);
        assert_eq!(app.state.zoom, 1.0);
        app.apply_tool_action(ToolAction::ZoomOut, &ctx);
        assert!(app.state.zoom < 1.0);

        // Thema.
        let before = app.theme;
        app.apply_tool_action(ToolAction::ToggleTheme, &ctx);
        assert_ne!(app.theme, before);
        app.apply_tool_action(ToolAction::ToggleTheme, &ctx);
        assert_eq!(app.theme, before);

        // Rückgängig — die Analyse beim Öffnen ist der erste Schritt.
        let found = app.state.regions.clone();
        assert!(!found.is_empty());
        app.apply_tool_action(ToolAction::Undo, &ctx);
        assert!(app.state.regions.is_empty());
        app.apply_tool_action(ToolAction::Redo, &ctx);
        assert_eq!(app.state.regions, found);
    }

    /// Der Zustand der Leiste folgt dem Zustand der Anwendung.
    #[test]
    fn the_toolbar_context_mirrors_the_application() {
        let mut app = RedactApp::silent(iban_only());
        let empty = app.tool_context();
        assert!(!empty.loaded);
        assert!(!empty.can_undo);
        assert!(empty.first_page && empty.last_page);
        assert!(!toolbar::is_enabled(ToolAction::Export, &empty));

        app.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "demo.pdf");
        let loaded = app.tool_context();
        assert!(loaded.loaded);
        assert!(loaded.can_undo, "die Analyse ist zurücknehmbar");
        assert!(loaded.first_page && !loaded.last_page);
        assert!(toolbar::is_enabled(ToolAction::Export, &loaded));
        assert!(toolbar::is_enabled(ToolAction::NextPage, &loaded));
        assert!(!toolbar::is_enabled(ToolAction::PrevPage, &loaded));

        app.state.last_page();
        let last = app.tool_context();
        assert!(!last.first_page && last.last_page);
    }

    // ------------------------------------------------------- Leerer Zustand

    /// Der leere Hauptbereich muss beide Wege nennen.
    #[test]
    fn the_empty_state_names_both_ways_in() {
        assert!(EMPTY_DOCUMENT_HINT.contains("Noch kein PDF geladen"));
        assert!(EMPTY_DOCUMENT_HINT.contains("öffnen"));
        assert!(EMPTY_DOCUMENT_HINT.contains("ziehen"));
    }

    /// Rauchtest ohne Bildschirm: ein ganzes Bild zeichnen, leer und mit
    /// Dokument, in beiden Themen.
    #[test]
    fn a_whole_frame_draws_in_both_themes() {
        let ctx = egui::Context::default();
        let app = std::cell::RefCell::new(RedactApp::silent(iban_only()));

        for theme in crate::theme::THEMES {
            app.borrow_mut().theme = theme;
            // `eframe::Frame` lässt sich ohne Fenster nicht bauen — deshalb
            // werden die Panels hier einzeln über dieselben Bausteine
            // gezeichnet wie in `update`.
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                ctx.set_visuals(theme.visuals());
                let summary = app.borrow().state.hit_summary();
                egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
                    app.borrow_mut().top_bar(ui);
                });
                egui::SidePanel::left("thumbnails").show(ctx, |ui| {
                    let mut app = app.borrow_mut();
                    let RedactApp { state, pages, .. } = &mut *app;
                    crate::thumbnails::show(ui, state, pages, false);
                });
                egui::CentralPanel::default().show(ctx, |ui| {
                    if app.borrow().state.is_loaded() {
                        app.borrow_mut().paint_page(ui, &summary);
                    } else {
                        empty_state(ui);
                    }
                });
            });
            assert!(!output.shapes.is_empty(), "{theme:?}: nichts gezeichnet");

            if !app.borrow().state.is_loaded() {
                app.borrow_mut()
                    .open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "demo.pdf");
            }
        }
        assert!(app.borrow().state.is_loaded());
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
        let mut app = RedactApp::new(iban_only());
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
        let app = std::cell::RefCell::new(RedactApp::new(iban_only()));
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

    // ------------------------------------------------- Review-Dateien (36)

    /// **Aufgabe 36, der ganze Weg.** Eine in der GUI gespeicherte
    /// Review-Datei trägt die Prüfsumme ihres Dokuments; beim Laden zu einem
    /// anderen Dokument wird sie abgelehnt, statt die Rechtecke an falsche
    /// Stellen zu setzen.
    #[test]
    fn a_review_file_from_another_document_is_refused_on_load() {
        let dir = std::env::temp_dir().join(format!(
            "redact-gui-review-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        // Dokument A: Review speichern, wie es der Knopf „Review speichern“ tut.
        let mut a = RedactApp::silent(iban_only());
        a.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "a.pdf");
        a.state
            .add_manual_region(0, Rect::new(10.0, 10.0, 50.0, 20.0), "Gehalt");
        let path = dir.join("a_review.json");
        std::fs::write(&path, a.state.to_review_file().to_json().unwrap()).unwrap();
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains(&crate::state::sha256_hex(&redact_pdf::testing::demo_statement())[..12]),
            "die Prüfsumme muss in der Datei stehen"
        );

        // Dokument B ist ein anderes — ein einzelnes leeres Blatt genügt.
        let mut b = RedactApp::silent(iban_only());
        b.open_bytes_and_analyze(&one_page_pdf(), "b.pdf");
        assert!(b.state.is_loaded());
        let before = b.state.regions.clone();

        b.load_review_file(&path);
        let error = b.error.clone().expect("die Datei muss abgelehnt werden");
        assert!(error.contains("anderen Dokument"), "{error}");
        assert!(error.contains("a.pdf"), "{error}");
        assert_eq!(b.state.regions, before, "es darf nichts übernommen werden");

        // Zum eigenen Dokument geht dieselbe Datei durch.
        a.state.regions.clear();
        a.load_review_file(&path);
        assert!(a.error.is_none(), "{:?}", a.error);
        assert!(!a.state.regions.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Ein Arbeitsverzeichnis je Test.
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "redact-gui-app-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Legt den Demo-Kontoauszug als Datei ab.
    fn demo_file(dir: &Path) -> PathBuf {
        let path = dir.join("kontoauszug.pdf");
        std::fs::write(&path, redact_pdf::testing::demo_statement()).unwrap();
        path
    }

    /// **Befund #67.** `--apply-review` galt in der Oberfläche nicht: sie lud
    /// und analysierte, die genannte Datei blieb liegen. Auf der Kommandozeile
    /// tritt sie an die Stelle der Analyse — hier jetzt auch.
    #[test]
    fn apply_review_from_the_command_line_is_applied_on_start() {
        let dir = temp_dir("apply-review");
        let input = demo_file(&dir);
        let review = dir.join("durchsicht.json");

        // Eine Review-Datei, die sich von der Analyse unterscheidet: ein
        // zusätzliches, von Hand gezogenes Rechteck.
        let marker = Rect::new(11.0, 12.0, 33.0, 44.0);
        let mut author = RedactApp::silent(Config {
            input: input.clone(),
            ..iban_only()
        });
        author.open_startup_document();
        let found = author.state.regions.len();
        assert!(found > 0, "die Analyse muss etwas finden");
        author
            .state
            .add_manual_region(0, marker, "aus der Durchsicht");
        std::fs::write(&review, author.state.to_review_file().to_json().unwrap()).unwrap();

        // Und jetzt derselbe Start mit `--apply-review`.
        let mut app = RedactApp::silent(Config {
            input: input.clone(),
            apply_review: Some(review.clone()),
            ..iban_only()
        });
        app.open_startup_document();

        assert!(app.error.is_none(), "{:?}", app.error);
        assert_eq!(
            app.state.regions.len(),
            found + 1,
            "die Review-Datei ist nicht angekommen — es steht nur die Analyse da"
        );
        assert!(
            app.state.regions.iter().any(|a| a.region.rect == marker),
            "das Rechteck aus der Review-Datei fehlt"
        );
        assert!(
            app.state.status.contains("Review übernommen"),
            "und es steht in der Statuszeile: {}",
            app.state.status
        );

        // Gegenprobe: ohne den Schalter bleibt es bei der Analyse.
        let mut without = RedactApp::silent(Config {
            input: input.clone(),
            ..iban_only()
        });
        without.open_startup_document();
        assert_eq!(without.state.regions.len(), found);

        // Und eine Review-Datei ohne Dokument wird nicht stillschweigend
        // verschluckt.
        let mut alone = RedactApp::silent(Config {
            apply_review: Some(review),
            ..iban_only()
        });
        alone.open_startup_document();
        let error = alone.error.clone().expect("das muss auffallen");
        assert!(error.contains("--apply-review"), "{error}");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// **Befund #67.** `--audit-log` galt in der Oberfläche nicht: der Pfad
    /// wurde immer aus dem Namen der Ausgabedatei abgeleitet.
    #[test]
    fn the_audit_log_goes_where_the_command_line_said() {
        let dir = temp_dir("audit-log");
        let input = demo_file(&dir);
        let wanted = dir.join("protokoll.json");
        let out = dir.join("geschwaerzt.pdf");

        let mut app = RedactApp::silent(Config {
            input: input.clone(),
            audit_log: Some(wanted.clone()),
            ..iban_only()
        });
        app.open_startup_document();
        app.export_to(out.clone());

        assert!(app.error.is_none(), "{:?}", app.error);
        assert!(
            wanted.exists(),
            "das Log steht nicht, wo --audit-log es hinhaben wollte"
        );
        let derived = AppState::audit_path_for(&out);
        assert!(
            !derived.exists(),
            "stattdessen entstand der abgeleitete Name {}",
            derived.display()
        );
        assert!(
            app.state.status.contains("protokoll.json"),
            "die Statuszeile nennt den falschen Pfad: {}",
            app.state.status
        );

        // Gegenprobe: ohne den Schalter ist der abgeleitete Name richtig.
        let second = dir.join("zweite.pdf");
        let mut plain = RedactApp::silent(Config {
            input,
            ..iban_only()
        });
        plain.open_startup_document();
        plain.export_to(second.clone());
        assert!(plain.error.is_none(), "{:?}", plain.error);
        assert!(AppState::audit_path_for(&second).exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Ein zweites, anderes PDF für den Test oben.
    fn one_page_pdf() -> Vec<u8> {
        let mut doc = redact_pdf::load_from_bytes(&redact_pdf::testing::demo_statement()).unwrap();
        // Irgendeine Änderung, die den Inhalt verschiebt — es geht nur darum,
        // dass eine andere Datei entsteht.
        doc.version = "1.6".to_string();
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        assert_ne!(bytes, redact_pdf::testing::demo_statement());
        bytes
    }

    // ---------------------------------------------------------- Passwort

    /// Das Passwort geht denselben Weg wie jedes Öffnen — samt Analyse.
    #[test]
    fn submitting_the_password_opens_and_analyses_the_document() {
        use redact_pipeline::testing::{ENCRYPTED_PDF, ENCRYPTED_PDF_PASSWORD};

        let mut app = RedactApp::silent(iban_only());
        app.open_bytes_and_analyze(ENCRYPTED_PDF, "auszug.pdf");
        assert!(app.state.needs_password(), "die Abfrage kommt nicht");
        assert!(!app.state.is_loaded());

        app.set_password_input(ENCRYPTED_PDF_PASSWORD);
        app.submit_password();

        assert!(!app.state.needs_password());
        assert!(app.state.is_loaded());
        assert_eq!(app.state.regions.len(), 1, "es wurde nicht analysiert");
        assert!(app.error.is_none(), "{:?}", app.error);
        // Das Eingabefeld ist leer, das Passwort steht nirgends im Zustand.
        assert!(app.password_input.is_empty());
        assert!(!format!("{:?}", app.state).contains(ENCRYPTED_PDF_PASSWORD));
    }

    /// Nach einem falschen Passwort bleibt die Frage stehen, das Feld ist leer
    /// und die Meldung nennt das Passwort nicht.
    #[test]
    fn a_wrong_password_leaves_the_question_open() {
        use redact_pipeline::testing::ENCRYPTED_PDF;

        let mut app = RedactApp::silent(iban_only());
        app.open_bytes_and_analyze(ENCRYPTED_PDF, "auszug.pdf");
        app.set_password_input("falsch-4711");
        app.submit_password();

        assert!(app.state.needs_password(), "die Abfrage ist zugefallen");
        assert!(app.password_input.is_empty());
        let message = app.error.clone().expect("eine Meldung muss erscheinen");
        assert!(!message.contains("falsch-4711"), "{message}");

        // Abbrechen schließt sie und lässt nichts stehen.
        app.cancel_password();
        assert!(!app.state.needs_password());
        assert!(app.error.is_none());
    }

    /// Das Thema kommt aus der `Config` — dort hat es die Einstellungsdatei
    /// hineingelegt.
    #[test]
    fn the_theme_comes_from_the_configuration() {
        for (name, expected) in [("hell", Theme::Light), ("dunkel", Theme::Dark)] {
            let app = RedactApp::new(Config {
                theme: name.to_string(),
                ..Config::default()
            });
            assert_eq!(app.theme, expected, "Thema „{name}“");
        }
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
        let mut app = RedactApp::new(iban_only());
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
        let mut app = RedactApp::new(iban_only());
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
