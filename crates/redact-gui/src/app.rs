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
//!
//! Es sind **sechs** Wege: „Analysieren“ (und damit auch „Buchungsliste laden“)
//! wirft zwar keine gezogenen Rechtecke weg, aber jede Abwahl, jede je Treffer
//! gewählte Schwärzungsart und ein geladenes Review — und fragte als einziger
//! nicht. Siehe [`RedactApp::analyze`]. Der sechste ist das Umschalten der
//! automatischen Erkennung ([`RedactApp::apply_pattern_toggle`]): es rechnet
//! dieselbe Liste neu und geht deshalb durch dieselbe Frage.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};

use egui::{Color32, Key, Pos2, RichText, Stroke, Vec2};
use redact_core::ReviewFile;

use crate::render::PageCache;
use crate::selector::{hit_handle, hit_test, HandleDrag, PointerFrame, RectangleSelector};
use crate::state::{
    AppState, ExportCheck, ExportCheckPlan, HitSummary, RegionColor, MAX_ZOOM, MIN_ZOOM,
};
use crate::theme::Theme;
use crate::toolbar::{self, ToolAction, ToolContext, ToolItem};
use crate::viewer::{self, PagePreview};
use redact_pipeline::Config;

/// Was im Hauptbereich steht, solange nichts geladen ist.
///
/// Ein leerer grauer Bereich sagt nichts; dieser Satz nennt beide Wege, die
/// zum Ziel führen.
pub const EMPTY_DOCUMENT_HINT: &str = "Noch kein PDF geladen — öffnen oder hierher ziehen";

/// Was quer über einem Blatt steht, auf dem der Rasterizer nichts gezeichnet
/// hat.
///
/// Siehe [`crate::render::PageCache::nothing_drawn`]: `degraded` läuft von
/// einem fremden PDF aus praktisch nie an, ein unlesbarer Inhaltsstrom endet
/// als **gewöhnliche leere Seite**. Ohne diesen Satz sieht sie aus wie eine
/// geprüfte Leerseite, während die Kopfzeile ihre Zahlen ohne Vorbehalt nennt.
///
/// Der Wortlaut muss für beide Fälle stimmen — unlesbar **und** wirklich leer:
/// gezeichnet wurde in beiden Fällen nichts, und in beiden Fällen hat die
/// Analyse hier nichts gesehen.
pub const NOTHING_DRAWN_NOTICE: &str = "Auf dieser Seite wurde nichts dargestellt.\n\
     Sie ist entweder leer, oder ihr Inhalt war nicht lesbar —\n\
     was hier steht, wurde nicht durchsucht.";

/// Zeichen neben der Seitenzahl in der Miniaturspalte, wenn auf einer Seite
/// nichts gezeichnet wurde.
pub const NOTHING_DRAWN_MARK: &str = "⚠";

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
// Ausgegraute Bedienelemente und der Tabulator
// ---------------------------------------------------------------------------

/// Ein Knopf, der ausgegraut werden darf, ohne die Tabulatorkette zu
/// zerschneiden.
///
/// **Das Problem.** „Ausgegraut statt weggelassen“ ist die erklärte Regel
/// dieser Leiste ([`toolbar::is_enabled`]) — und sie machte die Leiste vorwärts
/// unbedienbar. egui 0.29 meldet ein abgeschaltetes Widget zuerst als
/// fokusinteressiert und nimmt ihm den Fokus im selben Aufruf wieder weg
/// (`Ui::add_enabled` → `Response::interact` mit einem `disabled`-Ui). Der
/// Tastendruck ist damit verbraucht: der Fokus liegt danach **nirgends**, und
/// der nächste Tabulator beginnt wieder ganz vorn. Alles hinter dem ersten
/// grauen Knopf ist vorwärts unerreichbar.
///
/// Im Alltag heißt das: „Wiederholen“ ist grau, solange nichts zurückgenommen
/// wurde — also sind Kleiner, Größer, Passend, 100 %, Zurück und Vor mit dem
/// Tabulator nicht zu erreichen. (Umschalt+Tab kommt durch; das steht nirgends,
/// und „bedienbar, wenn man rückwärts geht“ ist keine Antwort.)
///
/// **Die Abhilfe.** Ein abgeschalteter Knopf bekommt [`egui::Sense::hover`]
/// statt [`egui::Sense::click`]. Damit ist er nicht mehr fokussierbar, der
/// Tabulator geht an ihm vorbei zum nächsten benutzbaren Element, und der
/// Tastendruck ist nicht verbraucht. Optisch ändert sich nichts: `add_enabled`
/// zeichnet ihn weiterhin grau, und die Sprechblase mit dem Grund bleibt.
fn greyable_button(label: String, enabled: bool) -> egui::Button<'static> {
    egui::Button::new(label).sense(if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    })
}

/// Die Knopfreihe der Symbolleiste — ohne Zoomregler und Themenschalter.
///
/// Eigene Funktion und nicht bloß eine Schleife in
/// [`RedactApp::top_bar`], damit die Reihe **ohne** den Rest des Fensters
/// gezeichnet werden kann: woran der Tabulator hängen bleibt, entscheidet sich
/// hier und nirgends sonst, und ein Test, der die Reihe nachbaut, prüft seinen
/// eigenen Nachbau statt der Leiste.
///
/// Gibt zurück, welcher Knopf gedrückt wurde.
fn tool_row(ui: &mut egui::Ui, context: &ToolContext) -> Option<ToolAction> {
    let mut clicked = None;
    for item in toolbar::items() {
        match item {
            ToolItem::Separator => {
                ui.separator();
            }
            ToolItem::Button(button) => {
                let enabled = toolbar::is_enabled(button.action, context);
                // Ein grauer Knopf sagt „geht gerade nicht“; **warum** steht in
                // der Sprechblase. Für „Analysieren“ ist der Grund einer, den
                // der Nutzer selbst gesetzt hat und selbst zurücknehmen kann —
                // der gehört genannt.
                //
                // Ausdrücklich an `can_find_anything` und nicht an `!enabled`:
                // ohne Dokument ist derselbe Knopf auch grau, aber aus einem
                // ganz anderen Grund.
                let hint = if button.action == ToolAction::Analyze
                    && context.loaded
                    && !context.can_find_anything
                {
                    toolbar::ANALYZE_OFF_HINT
                } else {
                    button.hint
                };
                if ui
                    .add_enabled(enabled, greyable_button(button.label(), enabled))
                    .on_hover_text(hint)
                    .clicked()
                {
                    clicked = Some(button.action);
                }
            }
        }
    }
    clicked
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
    /// Strg+R — Rechteck anlegen, siehe [`KeyCommand::AddRegion`].
    pub key_r: bool,
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
    /// Ausgewählte Region in der Größe ändern: die linke untere Ecke bleibt
    /// stehen, die rechte obere wandert (Strg+Pfeil).
    ///
    /// Das Gegenstück zum Eckgriff. Ohne es bliebe der Tastenweg zu einem
    /// eigenen Rechteck auf eine feste Größe festgelegt — und eine Anschrift
    /// hat keine feste Größe.
    Resize {
        dx: f64,
        dy: f64,
    },
    /// Rechteck fester Größe in der Mitte der aktuellen Seite anlegen (Strg+R).
    ///
    /// Siehe [`crate::toolbar::ToolAction::AddRegion`]: der einzige Weg zu
    /// einem eigenen Rechteck, der ohne Zeigegerät auskommt.
    AddRegion,
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

    let mut commands = Vec::new();

    // Esc, Entf, Bild auf/ab, Pos1 und Ende gelten **unabhängig von der
    // Steuerungstaste** — so, wie es die Tastentabelle ohne Vorbehalt
    // verspricht und wie Umschalt+Bild ab schon immer geblättert hat. Vorher
    // kehrte der Strg-Zweig unten vorzeitig zurück, und mit gehaltener Strg
    // taten diese sechs Tasten nichts.
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

    if keys.ctrl {
        // Tastenkürzel mit Steuerungstaste stehen für sich: Strg+Z ist
        // Rückgängig und nicht zusätzlich irgendein Buchstabe im Blätterwerk.
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
        if keys.key_r {
            commands.push(KeyCommand::AddRegion);
        }
        // Strg+Pfeil ändert die Größe der Auswahl — dieselbe Schrittweite wie
        // beim Schieben, mit Umschalt dieselbe große. Ohne Auswahl gibt es
        // nichts zu ändern; ein Blättern wäre hier die falsche Antwort, denn
        // dafür genügt der Pfeil allein.
        if selected {
            if keys.left {
                commands.push(KeyCommand::Resize { dx: -step, dy: 0.0 });
            }
            if keys.right {
                commands.push(KeyCommand::Resize { dx: step, dy: 0.0 });
            }
            if keys.up {
                commands.push(KeyCommand::Resize { dx: 0.0, dy: step });
            }
            if keys.down {
                commands.push(KeyCommand::Resize { dx: 0.0, dy: -step });
            }
        }
    } else if selected {
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

/// Der Satz hinter der Meldung „zu groß“ für eine im Dialog gewählte
/// Review-Datei.
///
/// Anders formuliert als der Satz der Kommandozeile: hier gibt es keinen
/// Schalter, auf den man zeigen könnte, sondern einen Dateidialog — und der
/// wahrscheinlichste Fall ist deshalb nicht die zu große Review-Datei, sondern
/// die versehentlich angeklickte PDF- oder Archivdatei daneben.
const REVIEW_LIMIT_HINT: &str = "Für eine Review-Datei ist das eine feste Grenze und keine \
     Einstellung: gemessen sind rund 600 Byte je geprüfter Stelle, 16 MB fassen also gut \
     25 000 — von Hand geprüft werden Dutzende bis Hunderte. Gewählt war vermutlich nicht \
     die Review-Datei, sondern eine PDF- oder Archivdatei daneben.";

/// Liest eine Review-Datei als Text — mit der Obergrenze **vor** dem ersten
/// gelesenen Byte.
///
/// Der Dateidialog ist an dieser Stelle kein Schutz, sondern nur eine
/// Eingabemaske: er liefert einen *Namen*, und was hinter dem Namen liegt —
/// eine dünn belegte Riesendatei, eine benannte Pipe, ein Gerät — bestimmt
/// nicht, wer geklickt hat. [`redact_core::read_limited`] fragt deshalb auch
/// hier zuerst und liest dann.
///
/// [`redact_core::RedactError::Parse`] wie auf der Kommandozeile
/// (`redact_pipeline::read_aux_text`): daneben liefert `ReviewFile::from_json`
/// dieselbe Fehlerart, und für die Bedienende ist „die Datei taugt nicht“ eine
/// Aussage. Vorher kam hier ein nackter `Io`-Fehler heraus — **ohne den
/// Dateinamen**.
fn read_review_text(path: &std::path::Path) -> redact_core::Result<String> {
    let bytes = redact_core::read_limited(path, redact_core::MAX_AUX_FILE_BYTES, REVIEW_LIMIT_HINT)
        .map_err(redact_core::RedactError::Parse)?;
    String::from_utf8(bytes).map_err(|_| {
        redact_core::RedactError::Parse(format!(
            "{}: keine UTF-8-Datei. Eine Review-Datei ist JSON, also Text.",
            redact_core::safe_path(path)
        ))
    })
}

/// Die Überschrift des Hinweisfensters beim Laden einer Review-Datei.
///
/// Die alte Überschrift lautete immer „Review-Datei passt nicht zum Dokument“.
/// Solange nur die Prüfsumme scheitern konnte, stimmte das; seit die Datei
/// auch „zu groß“ oder „keine gewöhnliche Datei“ sein kann, stimmt es nicht
/// mehr — und eine Überschrift, die am Text darunter vorbeiredet, schickt die
/// Suche in die falsche Richtung. Die Zugehörigkeit meldet
/// [`redact_pipeline::check_review_identity`] als
/// [`redact_core::RedactError::Config`]; alles andere ist ein Fehler an der
/// Datei selbst.
pub fn review_error_title(error: &redact_core::RedactError) -> &'static str {
    match error {
        redact_core::RedactError::Config(_) => "Review-Datei passt nicht zum Dokument",
        _ => "Review-Datei nicht lesbar",
    }
}

/// Meldung nach einem geglückten Speichern der Review-Datei.
///
/// Die Datei enthält die gefundenen Geheimnisse im Klartext (dieselben, die im
/// Audit-Log stehen können) — wer sie weitergibt, gibt sie mit weiter. Das
/// gehört in dieselbe Zeile wie der Erfolg.
pub fn review_saved_status(entries: usize, path: &std::path::Path) -> String {
    format!(
        "Review gespeichert: {entries} Eintrag/Einträge → {} \
         (enthält die gefundenen Texte im Klartext)",
        path.display()
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

/// Wer die Rückfrage vor Datenverlust beantwortet.
///
/// Ein `rfd`-Dialog blockiert und braucht ein Fenster; im Test gibt es beides
/// nicht. Früher stand hier ein `bool` „fragen ja/nein“ — damit ließ sich nur
/// der Fall „Nutzerin sagt Ja“ prüfen, und ob ein Weg überhaupt fragt, blieb
/// ungeprüft. Mit [`Ask::Answer`] steht die Antwort fest, **die Frage wird aber
/// gestellt**: ein Weg, der `may_discard` gar nicht erst aufruft, fällt damit
/// auf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    /// Im Betrieb: ein Fenster fragt nach.
    User,
    /// Nur im Test: diese Antwort gilt, ohne Fenster.
    ///
    /// Ausdrücklich `#[cfg(test)]`: im ausgelieferten Programm gibt es diesen
    /// Weg nicht, eine Rückfrage lässt sich dort nicht wegkonfigurieren.
    #[cfg(test)]
    Answer(bool),
}

/// Eine Nachprüfung, die gerade auf ihrem Thread läuft.
///
/// Siehe [`RedactApp::start_export_check`].
struct PendingCheck {
    /// Die Exportmeldung, vor die das Ergebnis tritt — sie nennt die Datei,
    /// damit der Satz auch dann noch stimmt, wenn inzwischen ein anderes
    /// Dokument offen ist.
    prefix: String,
    /// Der Name der geschriebenen Datei — vor jede Warnung, die das Urteil
    /// hinterlässt: nach einem zweiten Export wäre ein nackter Satz sonst
    /// keiner Datei mehr zuzuordnen (Befund G5-A4).
    file: String,
    /// Die **Kennung** dieser Prüfung und ihrer Warnungen: der aufgelöste
    /// Pfad der Ausgabe ([`file_key`]). Zwei Dateien gleichen Namens in
    /// verschiedenen Ordnern sind zwei Prüfungen; ein zweiter Export
    /// **derselben** Datei macht die ältere gegenstandslos
    /// ([`RedactApp::start_export_check`]) — gleich, wie ihr Pfad geschrieben
    /// ist.
    key: PathBuf,
    result: Receiver<ExportCheck>,
}

/// Der Dateiname für die Warnung — der ganze Pfad steht in der Exportmeldung.
fn file_name_of(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Die Kennung einer Ausgabedatei: der **aufgelöste** Pfad.
///
/// Gemeint ist die Datei, nicht ihre Schreibweise. `./a.pdf` und `a.pdf`,
/// `ordner/../ordner/out.pdf` und `ordner/out.pdf`, ein Symlink und sein Ziel
/// sind für einen Vergleich Zeichen für Zeichen verschieden und für das
/// Dateisystem dasselbe. Daran hingen zwei Fehler (Befunde R4-4 und R4-5):
/// die ältere Nachprüfung lief weiter und bewertete die **neuen** Bytes mit
/// dem **alten** Plan — in der gefährlichen Richtung eine Entwarnung unter dem
/// Präfix des ersten Exports —, und die Warnungen einer Datei wurden am
/// bloßen **Dateinamen** gehalten, sodass ein sauberer Export nach
/// `y/a.pdf` die Leckwarnung über `x/a.pdf` mitnahm.
///
/// [`std::fs::canonicalize`] fragt dafür das Dateisystem (es löst `.`, `..`
/// und Symlinks auf und verlangt, dass es die Datei gibt). Schlägt es fehl —
/// die Datei wurde inzwischen gelöscht, ein Verzeichnis darüber ist nicht
/// lesbar —, gilt der Pfad selbst: dann ist die Kennung wieder so grob wie
/// vorher, aber nie falsch verschmolzen.
fn file_key(path: &std::path::Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Streicht das **erste** Vorkommen dieses Satzes aus der Liste.
///
/// Zwei Ausgabedateien gleichen Namens in verschiedenen Ordnern schreiben
/// denselben Text („a.pdf: …“); jede hält ihren eigenen Eintrag
/// ([`WarnedFile`]), und gestrichen wird genau einer.
fn remove_first(list: &mut Vec<String>, entry: &str) {
    if let Some(at) = list.iter().position(|held| held == entry) {
        list.remove(at);
    }
}

/// Die Warnungen **einer** Ausgabedatei.
///
/// Gehalten wird am aufgelösten Pfad ([`file_key`]), angezeigt wird der
/// Dateiname: der Pfad ist die Kennung, der Name ist der Text. Bis
/// Fix-Runde 6 war beides dasselbe — und ein Export nach `y/a.pdf` strich die
/// Warnungen von `x/a.pdf` (Befund R4-5).
struct WarnedFile {
    key: PathBuf,
    /// Die Sätze dieser Datei, wörtlich wie in [`AppState::warnings`].
    entries: Vec<String>,
}

/// Fordert beim Fallenlassen ein Neuzeichnen an — auch auf dem Weg einer
/// Panik.
///
/// Der Prüf-Thread ([`RedactApp::start_export_check`]) rief
/// `request_repaint` als **letzte Anweisung** auf. Starb er davor, ruhte die
/// Oberfläche weiter: ohne Bild kein [`RedactApp::poll_export_checks`], ohne
/// Abholen kein `Disconnected`, und die Statuszeile blieb auf
/// [`EXPORT_CHECK_RUNNING`] stehen — der Satz „abgebrochen (interner
/// Fehler)“ war zwar geschrieben, kam aber nie an. Als Wächter läuft es
/// beim Abwickeln mit.
///
/// `None` heißt: es gab noch kein Bild (Tests ohne Bildschirm) — dann gibt
/// es auch nichts anzustoßen.
struct RepaintOnDrop(Option<egui::Context>);

impl Drop for RepaintOnDrop {
    fn drop(&mut self) {
        if let Some(ctx) = &self.0 {
            ctx.request_repaint();
        }
    }
}

/// Testhaken: hält den Prüf-Thread an, bis der Test ihn freigibt.
///
/// Ohne ihn ließ sich nicht prüfen, **wann** das Neuzeichnen angefordert
/// wird. Beide Tests aus Fix-Runde 5 (`zb_p5d5_*`) blieben grün, wenn man
/// [`RepaintOnDrop`] wieder außerhalb des Threads fallen ließ: dann wird das
/// Neuzeichnen beim **Start** angefordert statt am **Ende**, und niemand
/// merkt es — genau die Fehlerklasse „ein Test bleibt ohne seine Korrektur
/// grün“ (Befund Q4-5). Mit dem Haken steht der Thread still, und der Test
/// kann feststellen, dass bis dahin **nichts** angefordert wurde.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct CheckGate {
    /// `(der Thread ist angekommen, der Test hat freigegeben)`.
    state: std::sync::Mutex<(bool, bool)>,
    signal: std::sync::Condvar,
}

#[cfg(test)]
impl CheckGate {
    /// Aus dem Prüf-Thread: melden und warten.
    fn arrive_and_wait(&self) {
        let mut state = self.state.lock().expect("Gatter");
        state.0 = true;
        self.signal.notify_all();
        while !state.1 {
            state = self.signal.wait(state).expect("Gatter");
        }
    }

    /// Aus dem Test: warten, bis der Thread wirklich läuft.
    pub(crate) fn wait_until_arrived(&self) {
        let mut state = self.state.lock().expect("Gatter");
        while !state.0 {
            state = self.signal.wait(state).expect("Gatter");
        }
    }

    /// Aus dem Test: den Thread laufen lassen.
    pub(crate) fn release(&self) {
        self.state.lock().expect("Gatter").1 = true;
        self.signal.notify_all();
    }
}

/// Was die Statuszeile zeigt, solange die Nachprüfung läuft.
pub const EXPORT_CHECK_RUNNING: &str = "Nachprüfung läuft …";

/// Für so viele Ausgabedateien werden Export- und Prüfwarnungen gehalten.
///
/// Sie ersetzen einander nicht mehr ([`RedactApp::note_export_warnings`]),
/// also braucht es eine Grenze: wer im selben Dokument dreißigmal exportiert,
/// soll keine dreißig Warnungen vor sich haben. Die älteste Datei fällt
/// heraus, die zehn jüngsten bleiben — und beim Dokumentwechsel ist die Liste
/// ohnehin weg.
pub const MAX_WARNED_FILES: usize = 10;

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
    /// Laufende Nachprüfungen nach dem Export — je eine je Ausgabedatei.
    ///
    /// Eine Liste, kein einzelner Platz: wer zweimal kurz nacheinander
    /// exportiert, bekommt **beide** Urteile. Ein verworfener Empfänger hieße,
    /// dass ein Leck in der ersten Datei nie gemeldet würde.
    checks: Vec<PendingCheck>,
    /// Ausgabedateien, deren Warnungen gerade gehalten werden — jüngste
    /// zuletzt. Siehe [`RedactApp::note_export_warnings`] und
    /// [`MAX_WARNED_FILES`].
    warned_files: Vec<WarnedFile>,
    /// Der egui-Kontext des letzten Bildes — damit ein Thread, der fertig ist,
    /// ein Neuzeichnen anstoßen kann, auch wenn die Oberfläche gerade ruht.
    /// `None`, solange noch kein Bild gezeichnet wurde (Tests ohne Bildschirm).
    ui_ctx: Option<egui::Context>,
    /// Helles oder dunkles Thema.
    pub theme: Theme,
    /// Zuletzt an egui übergebenes Thema — damit `set_visuals` nur bei einer
    /// Änderung läuft und nicht in jedem Bild.
    applied_theme: Option<Theme>,
    /// Seite des vorigen Bildes; wechselt sie, rollt die Miniaturspalte mit.
    shown_page: Option<usize>,
    /// Wurde das Schließen des Fensters bereits bestätigt?
    close_confirmed: bool,
    /// Wer die Rückfrage vor Datenverlust beantwortet.
    ask: Ask,
    /// Fokusstand über die Bildgrenze hinweg — siehe [`crate::focus`].
    text_focus: crate::focus::TextFieldFocus,
    /// Eingabefeld der Passwortabfrage.
    ///
    /// Steht hier und nicht im [`AppState`]: es ist der halb getippte Text
    /// eines Eingabefelds, kein Zustand des Dokuments. Nach jedem Versuch wird
    /// es geleert, damit das Passwort nicht länger im Speicher steht als nötig.
    password_input: String,
    /// Testhaken: lässt die Nachprüfung auf ihrem Thread paniken, damit der
    /// Weg „Thread ohne Ergebnis verschwunden“ einen Test hat (Befund G5-A3).
    #[cfg(test)]
    force_panic_in_check: bool,
    /// Testhaken: hält den Prüf-Thread an, siehe [`CheckGate`].
    #[cfg(test)]
    hold_check: Option<std::sync::Arc<CheckGate>>,
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
            checks: Vec::new(),
            warned_files: Vec::new(),
            ui_ctx: None,
            theme,
            applied_theme: None,
            shown_page: None,
            close_confirmed: false,
            ask: Ask::User,
            text_focus: crate::focus::TextFieldFocus::default(),
            password_input: String::new(),
            #[cfg(test)]
            force_panic_in_check: false,
            #[cfg(test)]
            hold_check: None,
        }
    }

    /// Rückfragen ohne Fenster, Antwort „Ja“ — für Tests ohne Bildschirm.
    #[cfg(test)]
    fn silent(config: Config) -> Self {
        Self {
            ask: Ask::Answer(true),
            ..Self::new(config)
        }
    }

    /// Rückfragen ohne Fenster, Antwort „Nein“ — für Tests ohne Bildschirm.
    ///
    /// Damit lässt sich prüfen, ob ein Weg vor dem Wegwerfen überhaupt fragt:
    /// wer `may_discard` nicht aufruft, ändert hier trotzdem etwas.
    #[cfg(test)]
    fn refusing(config: Config) -> Self {
        Self {
            ask: Ask::Answer(false),
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
        #[cfg(test)]
        if let Ask::Answer(answer) = self.ask {
            return answer;
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
    ///
    /// **Fragt vorher.** Eine erneute Analyse ersetzt alle automatisch
    /// gefundenen Einträge: jede Abwahl und jede je Treffer gewählte
    /// Schwärzungsart ist danach weg, und ein geladenes Review ebenso — den Weg
    /// hierher nimmt auch „Buchungsliste laden“. Von Hand gezogene Rechtecke
    /// bleiben zwar erhalten ([`AppState::analyze`]), die *Entscheidungen* an
    /// den übrigen Treffern nicht. Die vier anderen Wege zum selben Verlust
    /// (Öffnen, Ablegen, Review laden, Schließen) fragen über
    /// [`RedactApp::may_discard`] — dieser hier tat es nicht und meldete
    /// hinterher zufrieden die Zahl der Treffer.
    ///
    /// Der Verlaufseintrag aus [`AppState::analyze`] bleibt: er ist die
    /// Entschärfung (Strg+Z holt den Stand zurück), nicht der Ersatz für die
    /// Frage.
    /// Gibt zurück, ob die Analyse wirklich gelaufen ist.
    pub fn analyze(&mut self) -> bool {
        if !self.state.is_loaded() {
            self.state.status = "Erst ein PDF öffnen".to_string();
            return false;
        }
        if !self.may_discard("Neu zu analysieren") {
            self.state.status = "Analyse abgebrochen — nichts verändert".to_string();
            return false;
        }
        let result = self.state.analyze().map(|_| ());
        let ran = result.is_ok();
        self.report(result);
        ran
    }

    /// Schaltet die automatische Erkennung um und rechnet die Treffer neu.
    ///
    /// **Der sechste Weg zum Datenverlust** — und er geht durch dieselbe
    /// Rückfrage wie die fünf anderen ([`RedactApp::may_discard`]). Umschalten
    /// heißt, die Trefferliste neu aufzubauen: jede Abwahl und jede je Treffer
    /// gewählte Schwärzungsart ist danach weg, ein geladenes Review ebenso.
    ///
    /// **Was bleibt**, und das ist der Punkt: von Hand gezogene Rechtecke
    /// überstehen es (darum kümmert sich [`AppState::analyze`]), und die
    /// Schutzeinträge der Buchungsliste werden aus derselben Liste neu gefunden
    /// — die steht in der Konfiguration und wird hier nicht angefasst.
    ///
    /// Sagt die Nutzerin „nein“, bleibt auch das Kästchen, wie es war: die
    /// Seitenleiste ändert nichts selbst, sie meldet nur den Wunsch (siehe
    /// [`crate::sidebar::PatternToggle`]).
    fn apply_pattern_toggle(&mut self, toggle: crate::sidebar::PatternToggle) {
        use crate::sidebar::PatternToggle;

        let what = match &toggle {
            PatternToggle::All(true) => "Die automatische Erkennung einzuschalten",
            PatternToggle::All(false) => "Die automatische Erkennung abzuschalten",
            PatternToggle::One { .. } => "Ein Muster umzuschalten",
        };
        if !self.may_discard(what) {
            self.state.status = "Umschalten abgebrochen — nichts verändert".to_string();
            return;
        }

        match toggle {
            PatternToggle::All(on) => self.state.set_patterns_enabled(on),
            PatternToggle::One { id, on } => self.state.set_pattern_enabled(&id, on),
        }

        // Ohne Dokument gibt es nichts neu zu rechnen; die Einstellung gilt
        // trotzdem und wirkt beim nächsten Öffnen.
        if !self.state.is_loaded() {
            self.state.status = self
                .state
                .detection_notice()
                .unwrap_or_else(|| "Automatische Erkennung: alle Muster an".to_string());
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
        let summary = self.state.hit_summary();
        match self.state.export(&out, Some(&audit)) {
            Ok(outcome) => {
                self.error = None;
                // Erst jetzt gibt es die Datei — vorher könnte `file_key`
                // sie nicht auflösen.
                let key = file_key(&out);
                self.note_export_warnings(&key, &file_name_of(&out), &outcome.warnings);
                let prefix = export_status(
                    outcome.drawn_rects,
                    outcome.removed_glyphs,
                    &out,
                    &audit,
                    blocked,
                    outcome.warnings.first().map(String::as_str),
                );
                // Die Nachprüfung über die geschriebenen Bytes — der Weg, den
                // die Kommandozeile als `--check-leaks` bekommen hat, hier
                // ohne Konsole und ohne getippte Geheimnisse. Siehe
                // [`AppState::plan_export_check`].
                let plan = self.state.plan_export_check(&summary);
                self.start_export_check(prefix, plan, out, key);
            }
            Err(e) => self.report(Err(e)),
        }
    }

    /// Trägt die Warnungen eines geglückten Exports ein — **ohne** die des
    /// vorigen zu löschen.
    ///
    /// Vorher stand hier `self.state.warnings = outcome.warnings`: jeder
    /// geglückte Export warf die Warnung des vorigen weg. Damit war genau der
    /// Fall entwertet, für den Fix-Runde 4 den Dateinamen vor die Warnung
    /// gesetzt hat ([`RedactApp::finish_export_check`]) — „welche Datei ist
    /// gemeint?“ stellt sich nur, wenn mehr als eine dasteht. Der Doc-Kommentar
    /// von `finish_export_check` versprach „bleiben, bis das nächste Dokument
    /// kommt“; gehalten hat es nur, solange keine zweite Datei geschrieben
    /// wurde (`zb_g5a4` überlebte, weil dort beide Prüfungen noch liefen).
    ///
    /// Gehalten wird **je Ausgabedatei**, am Dateinamen als Präfix: ein
    /// zweiter Export derselben Datei ersetzt seine eigenen Warnungen (sonst
    /// sammelten sich Wiederholungen), und mehr als [`MAX_WARNED_FILES`]
    /// Dateien werden nicht gehalten. Beim **Dokumentwechsel** verschwinden
    /// sie: `AppState::load_bytes` setzt die Liste auf die Warnungen des neuen
    /// Dokuments. Die Namensliste selbst wird dabei nicht geleert — sie ist
    /// durch [`MAX_WARNED_FILES`] gedeckelt, und ein Name aus dem vorigen
    /// Dokument fällt als ältester zuerst heraus, ohne je eine Warnung mehr
    /// oder weniger zu bewirken.
    ///
    /// Die Warnungen des **Dokuments** ([`AppState::extract_warnings`]) stehen
    /// schon in der Liste — sie kamen beim Laden und gehören nicht der Datei,
    /// die gerade geschrieben wurde. Sie werden hier übergangen, sonst stünden
    /// sie nach jedem Export ein zweites Mal da.
    fn note_export_warnings(&mut self, key: &std::path::Path, file: &str, warnings: &[String]) {
        self.forget_warnings_of(key);
        for warning in warnings {
            if self.state.extract_warnings.contains(warning) {
                continue;
            }
            let entry = format!("{file}: {warning}");
            self.state.warnings.push(entry.clone());
            self.hold_warning(key, entry);
        }
        // Auch ohne Warnung ist diese Datei die jüngste — sonst fiele sie als
        // älteste heraus, während ihr Urteil noch unterwegs ist.
        self.remember_warnings_of(key);
    }

    /// Nimmt eine Ausgabedatei in die gehaltenen auf (als jüngste); die
    /// älteste fällt samt ihren Warnungen heraus, wenn es mehr als
    /// [`MAX_WARNED_FILES`] werden.
    fn remember_warnings_of(&mut self, key: &std::path::Path) -> &mut WarnedFile {
        match self.warned_files.iter().position(|held| held.key == key) {
            Some(at) => {
                let held = self.warned_files.remove(at);
                self.warned_files.push(held);
            }
            None => self.warned_files.push(WarnedFile {
                key: key.to_path_buf(),
                entries: Vec::new(),
            }),
        }
        while self.warned_files.len() > MAX_WARNED_FILES {
            let oldest = self.warned_files.remove(0);
            for entry in &oldest.entries {
                remove_first(&mut self.state.warnings, entry);
            }
        }
        self.warned_files.last_mut().expect("gerade eingetragen")
    }

    /// Hängt einen Satz an die Warnungen dieser Ausgabedatei (nur die
    /// Buchhaltung — in der Anzeige steht er schon).
    fn hold_warning(&mut self, key: &std::path::Path, entry: String) {
        self.remember_warnings_of(key).entries.push(entry);
    }

    /// Streicht die Warnungen einer Ausgabedatei aus der Liste.
    ///
    /// Gestrichen wird, was **diese** Datei eingetragen hat, nicht was so
    /// aussieht: der Schlüssel ist ihr aufgelöster Pfad ([`file_key`]), der
    /// Text trägt nur den Dateinamen.
    fn forget_warnings_of(&mut self, key: &std::path::Path) {
        let Some(at) = self.warned_files.iter().position(|held| held.key == key) else {
            return;
        };
        let held = self.warned_files.remove(at);
        for entry in &held.entries {
            remove_first(&mut self.state.warnings, entry);
        }
    }

    /// Lässt die Nachprüfung auf einem eigenen Thread laufen.
    ///
    /// Vorher lief sie im Zeichentakt: 305 Seiten mit 200 Begriffen hielten
    /// das Fenster sekundenlang an, eine 5-MB-Datei minutenlang. Jetzt geht
    /// nur der Plan (reine Daten) auf den Thread; das Ergebnis kommt über
    /// einen Kanal zurück und wird in [`RedactApp::poll_export_checks`]
    /// abgeholt — dasselbe Muster wie beim Rastern ([`crate::render`]).
    /// Solange steht [`EXPORT_CHECK_RUNNING`] in der Statuszeile.
    ///
    /// Lässt sich kein Thread starten, läuft die Prüfung an Ort und Stelle:
    /// langsam ist besser als gar nicht — ohne sie stünde da eine
    /// Erfolgsmeldung ohne Nachprüfung.
    ///
    /// ## Ein zweiter Export derselben Datei beendet die ältere Prüfung
    ///
    /// [`ExportCheckPlan::run`] liest die Datei **zum Prüfzeitpunkt**, nicht
    /// zum Exportzeitpunkt. Wird dieselbe Datei ein zweites Mal geschrieben,
    /// während die erste Prüfung noch läuft, bewertet die ältere die **neuen**
    /// Bytes mit dem **alten** Plan — und trägt ihr Urteil unter dem Präfix
    /// des ersten Exports ein. Beides ist falsch, und die gefährliche
    /// Richtung ist die Entwarnung: Die erste Ausgabe leckte, die zweite
    /// nicht, und die Zeile meldet über den ersten Export „steht nicht mehr
    /// in der Ausgabe“.
    ///
    /// Über die Bytes der ersten Ausgabe **kann** niemand mehr etwas sagen —
    /// es gibt sie nicht mehr, die Datei trägt jetzt die zweite. Die ältere
    /// Prüfung wird deshalb hier fallen gelassen (ihr Thread läuft aus, sein
    /// `send` findet niemanden mehr); die neue prüft dieselbe Datei und trägt
    /// ihr Urteil unter dem Präfix ein, zu dem es gehört. Verschwiegen wird
    /// nichts: Die Warnungen der ersten Ausgabe hat `note_export_warnings`
    /// bereits durch die der zweiten ersetzt, denn sie hängen an derselben
    /// Ausgabedatei ([`file_key`]).
    ///
    /// **Fallen gelassen heißt nicht abgebrochen** — und das bleibt so.
    /// Der Thread der älteren Prüfung liest die Datei zu Ende, durchsucht sie,
    /// sein `send` findet niemanden mehr, und sein [`RepaintOnDrop`] fordert
    /// am Ende trotzdem ein Neuzeichnen an (eines zu viel, das egui mit dem
    /// nächsten zusammenlegt). Bei N schnellen Exporten derselben Datei laufen
    /// also N−1 vollständige Suchen umsonst weiter. Gemessen (Release,
    /// Testprozess, `zg_r4_app_tests::zg_r4_2_mess_was_eine_fallengelassene_pruefung_kostet`):
    /// an einer 300-seitigen Ausgabe (96 kB) kostet ein Lauf am oberen Rand
    /// der Decke (1 000 Begriffe) **0,11 s**; fünf davon gleichzeitig sind
    /// fünf Threads, nach 0,12 s steht das Urteil, nach 0,17 s sind alle aus,
    /// und die Speicherspitze steigt um **19 MB** (rund 4 MB je Lauf). An der
    /// größeren Vorlage aus `zb_mess_nachpruefung_je_begriff_gegen_einen_durchgang`
    /// (305 Seiten, 1 079 kB) kostet ein Lauf rund **1 s**.
    ///
    /// Ein Abbruchsignal (`AtomicBool`, das die Suche ab und zu liest) ließe
    /// sich **hier** nicht einlösen: die Suche ist ein einziger Aufruf von
    /// [`redact_pdf::leaks_many_within`], und eine Fahne, die nur davor
    /// gelesen wird, spart nichts — der Thread ist dort Mikrosekunden alt.
    /// Es bräuchte die Fahne **in** `redact-pdf`; solange eine fallen
    /// gelassene Prüfung eine Sekunde eines Kerns und ein paar MB kostet, ist
    /// das den Umbau nicht wert.
    fn start_export_check(
        &mut self,
        prefix: String,
        plan: ExportCheckPlan,
        out: PathBuf,
        key: PathBuf,
    ) {
        // Dieselbe Datei, ältere Prüfung: sie urteilt sonst über Bytes, die
        // es nicht mehr gibt. Verglichen wird die **Datei** ([`file_key`]),
        // nicht die Schreibweise des Pfades.
        self.checks.retain(|pending| pending.key != key);
        let (sender, receiver) = std::sync::mpsc::channel();
        let repaint = RepaintOnDrop(self.ui_ctx.clone());
        let file = file_name_of(&out);
        #[cfg(test)]
        let force_panic = self.force_panic_in_check;
        #[cfg(test)]
        let gate = self.hold_check.clone();
        let spawned = std::thread::Builder::new()
            .name("redact-export-check".to_string())
            .spawn({
                let plan = plan.clone();
                let out = out.clone();
                move || {
                    // Das Neuzeichnen hängt am **Ende des Threads**, nicht an
                    // seinem Erfolg: stirbt er unterwegs, holt niemand mehr
                    // etwas ab, wenn niemand zeichnet — und die Statuszeile
                    // bliebe für immer auf „Nachprüfung läuft …“. Diese Zeile
                    // zieht den Wächter in den Thread (sonst fiele er schon
                    // hier draußen); fallen gelassen wird er dort — auch beim
                    // Abwickeln einer Panik.
                    let _repaint = repaint;
                    // Der Haken steht **hinter** dem Wächter: ein Test hält
                    // den Thread hier an und stellt fest, dass bis dahin kein
                    // Neuzeichnen angefordert ist (Befund Q4-5).
                    #[cfg(test)]
                    if let Some(gate) = &gate {
                        gate.arrive_and_wait();
                    }
                    #[cfg(test)]
                    assert!(!force_panic, "Testhaken: die Nachprüfung panikt");
                    let check = plan.run(&out);
                    // Ein `Err` heißt: niemand wartet mehr — dann gibt es auch
                    // niemanden, dem man das sagen müsste.
                    let _ = sender.send(check);
                }
            });
        match spawned {
            Ok(_) => {
                self.state.status = format!("{prefix}  ·  {EXPORT_CHECK_RUNNING}");
                self.checks.push(PendingCheck {
                    prefix,
                    file,
                    key,
                    result: receiver,
                });
            }
            Err(_) => self.finish_export_check(&prefix, &file, &key, plan.run(&out)),
        }
    }

    /// Holt fertige Nachprüfungen ab — einmal je Bild, vor dem Zeichnen.
    ///
    /// Gibt zurück, wie viele Urteile angekommen sind.
    pub fn poll_export_checks(&mut self) -> usize {
        let mut done = Vec::new();
        self.checks
            .retain(|pending| match pending.result.try_recv() {
                Ok(check) => {
                    done.push((
                        pending.prefix.clone(),
                        pending.file.clone(),
                        pending.key.clone(),
                        Some(check),
                    ));
                    false
                }
                Err(TryRecvError::Empty) => true,
                // Der Thread ist ohne Ergebnis verschwunden (Panik). Schweigen
                // wäre eine Entwarnung, die keine ist.
                Err(TryRecvError::Disconnected) => {
                    done.push((
                        pending.prefix.clone(),
                        pending.file.clone(),
                        pending.key.clone(),
                        None,
                    ));
                    false
                }
            });
        let count = done.len();
        for (prefix, file, key, check) in done {
            match check {
                Some(check) => self.finish_export_check(&prefix, &file, &key, check),
                None => {
                    let sentence = "Nachprüfung: abgebrochen (interner Fehler) — es wurde \
                                    nichts nachgeprüft.";
                    // Auch in die Warnungen: die Statuszeile überschreibt
                    // die nächste Aktion, und ein Export ohne Nachprüfung
                    // darf nicht so aussehen wie einer mit.
                    self.note_check_warning(&key, &file, sentence);
                    self.state.status = format!("{prefix}  ·  {sentence}");
                }
            }
        }
        count
    }

    /// Trägt ein Urteil in Statuszeile und Warnungen ein.
    ///
    /// In die Warnungen geht, was [`ExportCheck::warning`] hergibt — ein
    /// Fund, eine unvollständige Antwort (mit dem Grund je Stelle), nicht gesuchte
    /// Texte jenseits der Decke — mit dem Dateinamen davor. Die Statuszeile
    /// ist flüchtig; die Warnungen bleiben, bis das nächste Dokument kommt.
    fn finish_export_check(
        &mut self,
        prefix: &str,
        file: &str,
        key: &std::path::Path,
        check: ExportCheck,
    ) {
        if let Some(warning) = check.warning() {
            // Ganz nach vorn: die Statuszeile zeigt nur die **erste**
            // Warnung, und keine andere ist wichtiger als diese.
            self.note_check_warning(key, file, &warning);
        }
        // Die Statuszeile trägt den **gedeckelten** Satz
        // ([`ExportCheck::status_line`]); die ganze Fassung steht in der
        // Warnung, die gerade eingetragen wurde.
        self.state.status = format!("{prefix}  ·  {}", check.status_line());
    }

    /// Das Urteil einer Nachprüfung in die Warnungen — ganz nach vorn, mit
    /// dem Dateinamen davor, und je Datei nur einmal (zwei Exporte derselben
    /// Datei kurz hintereinander tragen dasselbe Urteil).
    fn note_check_warning(&mut self, key: &std::path::Path, file: &str, warning: &str) {
        let entry = format!("{file}: {warning}");
        let held = self.remember_warnings_of(key);
        if held.entries.iter().any(|kept| kept == &entry) {
            return;
        }
        held.entries.push(entry.clone());
        self.state.warnings.insert(0, entry);
    }

    /// Läuft gerade eine Nachprüfung?
    pub fn export_check_running(&self) -> bool {
        !self.checks.is_empty()
    }

    /// Wartet auf alle laufenden Nachprüfungen und trägt ihre Urteile ein.
    ///
    /// Nur für Tests: die Oberfläche wartet nie — sie holt je Bild ab.
    #[cfg(test)]
    pub fn wait_for_export_checks(&mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while self.export_check_running() {
            assert!(
                std::time::Instant::now() < deadline,
                "die Nachprüfung kommt nicht zum Ende"
            );
            if self.poll_export_checks() == 0 {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
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
            can_find_anything: self.state.analysis_can_find_anything(),
        }
    }

    /// Die Symbolleiste: Symbol **und** Text je Knopf, dahinter der
    /// Zoomregler und der Themenschalter.
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let context = self.tool_context();
        let mut clicked: Option<ToolAction> = None;

        ui.horizontal_wrapped(|ui| {
            clicked = tool_row(ui, &context);

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
            ToolAction::Analyze => {
                self.analyze();
            }
            // Über dieselbe Stelle wie Strg+R, aus demselben Grund wie bei
            // Rückgängig: Knopf und Kürzel müssen dasselbe tun.
            ToolAction::AddRegion => self.apply_key_commands(&[KeyCommand::AddRegion]),
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
            // Die Liste gilt nur, wenn die Analyse auch läuft: wird die
            // Rückfrage abgelehnt, stünde sonst eine Buchungsliste in der
            // Konfiguration, von der im Bild nichts zu sehen ist.
            let previous = self.state.config.booking_list.replace(path);
            if !self.analyze() {
                self.state.config.booking_list = previous;
            }
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
            self.save_review_to(&path);
        }
    }

    /// Schreibt die Review-Datei und **sagt es**.
    ///
    /// [`RedactApp::report`] setzt bei `Ok(())` nur `error = None` und lässt die
    /// Statuszeile stehen: der Knopf arbeitete und schwieg. Ausgerechnet hier —
    /// die Review-Datei ist der Rettungsanker, auf den die Rückfrage vor
    /// Datenverlust ausdrücklich verweist ([`discard_question`]).
    pub fn save_review_to(&mut self, path: &std::path::Path) {
        match self.state.save_review_file(path) {
            Ok(()) => {
                self.error = None;
                self.state.status = review_saved_status(self.state.regions.len(), path);
            }
            Err(e) => self.report(Err(e)),
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
    ///
    /// ## Erst fragen, dann lesen
    ///
    /// Gelesen wird über [`read_review_text`] und damit über
    /// [`redact_core::read_limited`]. Hier stand ein `std::fs::read_to_string`
    /// auf einen im Dateidialog gewählten Pfad — derselbe Fehler wie hinter
    /// `--apply-review` auf der Kommandozeile, nur ohne Kommandozeile: eine
    /// dünn belegte Datei mit 6 GB Nennlänge (4 kB auf der Platte) belegte 6 GB
    /// Arbeitsspeicher, eine benannte Pipe ließ die Oberfläche stehen. Dass ein
    /// Mensch die Datei im Dialog aussucht, ist dabei kein Schutz: ausgesucht
    /// wird ein *Name*, und was hinter dem Namen liegt, bestimmt nicht er.
    pub fn load_review_file(&mut self, path: &std::path::Path) {
        let result = read_review_text(path)
            .and_then(|data| ReviewFile::from_json(&data))
            .and_then(|review| self.state.apply_review_file(review));
        if let Err(error) = &result {
            let message = error.to_string();
            if self.ask == Ask::User {
                rfd::MessageDialog::new()
                    .set_level(rfd::MessageLevel::Error)
                    // Der Titel darf nicht mehr behaupten, es liege an der
                    // Zugehörigkeit: seit der Grenze kommt hier auch „zu groß“
                    // und „keine gewöhnliche Datei“ an, und ein Titel, der am
                    // Text vorbeiredet, schickt die Suche in die falsche
                    // Richtung.
                    .set_title(review_error_title(error))
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
                        // Feste Kennung: nur so weiß [`RedactApp::read_keys`],
                        // dass der Fokus in einem **Textfeld** liegt.
                        .id(crate::focus::id(crate::focus::PASSWORD))
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

                // --- „Hier wurde nichts gezeichnet“ ---
                //
                // Quer über das Blatt und nicht in die Statuszeile: ein
                // reinweißes Blatt sieht aus wie eine geprüfte, leere Seite,
                // und wer es dafür hält, blättert weiter.
                if self.pages.nothing_drawn(page) {
                    viewer::paint_blank_notice(&painter, sheet, NOTHING_DRAWN_NOTICE);
                }

                // --- Regionen ---
                // Wie weit die Schwärzung wirklich reicht: `--padding`
                // vergrößert jeden Balken auf jeder Seite. Das war im Bild
                // nicht zu sehen — der Nachbartext, der mitverschwindet, auch
                // nicht.
                let padding = viewer::padding_screen(self.state.config.padding, zoom);
                for index in self.state.regions_on_page(page) {
                    let entry = &self.state.regions[index];
                    let screen = viewer::pdf_to_screen(&entry.region.rect, &view, zoom, origin);
                    if summary.outcome(index).is_redacted() {
                        viewer::paint_padding(&painter, screen, entry.color.rgb(), padding);
                    }
                    viewer::paint_region(
                        &painter,
                        screen,
                        entry.color.rgb(),
                        // Gefüllt wird nur, was auch wirklich geschwärzt wird.
                        viewer::RegionStyle::from_outcome(summary.outcome(index)),
                        self.state.selected_region == Some(index),
                    );
                }

                self.handle_pointer(&response, &view, origin, page, zoom);

                // --- Laufender Ziehvorgang ---
                //
                // **Nach** der Maus, nicht davor: sonst zeigte das Bild des
                // Drucks noch den Stand des vorigen Bildes, und das Rechteck
                // erschiene erst ein Bild später — genau die Verzögerung, die
                // hier abgestellt werden soll. Anders als die Regionen hängt
                // die Vorschau an nichts, was weiter oben in diesem Bild schon
                // berechnet wurde (etwa `summary`), sie darf also nachrücken.
                if let Some(preview) = self.selector.preview() {
                    painter.rect_stroke(
                        preview,
                        viewer::NO_ROUNDING,
                        Stroke::new(viewer::DRAG_STROKE, drop_accent()),
                    );
                }
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
    ///
    /// Diese Reihenfolge gilt seit dem Beheben der verspäteten Rückmeldung
    /// bereits im **Druckbild** und nicht erst bei `drag_started`: der Selektor
    /// beginnt jetzt beim Druck, also muss auch der Vorrang des Griffs dort
    /// schon greifen. Ein Klick (drücken und ohne Weg wieder loslassen) läuft
    /// weiterhin über Fall 3 und legt nichts an — Fall 4 verwirft alles
    /// unterhalb von [`crate::selector::MIN_DRAG_SIZE`].
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
        if let Some(mut drag) = self.resize {
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
            // Die Kennung schützt vor der falschen **Region**, nicht vor der
            // falschen **Seite**. Bild ab, Pos1 und Ende blättern laut
            // `key_commands` immer — auch mit gedrückter Maustaste. Danach
            // zeichnet die Oberfläche eine andere Seite und ruft diese Funktion
            // mit deren `view`/`page`; der Zug rechnete weiter, und die Region
            // auf der alten Seite änderte sich unsichtbar mit — gerechnet mit
            // der Geometrie der neuen. Gemeldet wurde „Rechteck angepasst“.
            //
            // Also genauso beenden wie bei verschwundener Kennung: kleiner als
            // die Blättertasten zu sperren, und es lässt den bisher gezogenen
            // Stand stehen, statt ihn zu verwerfen.
            if self.state.regions[index].region.page != page {
                self.resize = None;
                self.selector.cancel();
                self.state.status =
                    "Zug beendet — die angefasste Region liegt auf einer anderen Seite".to_string();
                return;
            }
            // Das Druckbild fasst den Griff nur an; verändert wird erst, wenn
            // sich der Zeiger danach bewegt hat. `down` gehört dazu: unterhalb
            // der Klickschwelle meldet egui weder `dragged` noch sonst etwas,
            // die Ecke soll dem Zeiger dort aber schon folgen.
            let moving = frame.dragged || frame.drag_stopped || (frame.down && !frame.pressed);
            if let Some(pos) = frame.pos.filter(|_| moving) {
                let corner = viewer::screen_to_pdf_point(pos, view, zoom, origin);
                if !drag.moved {
                    // Der eine Schnappschuss dieses Zuges.
                    self.state.begin_manual_edit();
                    drag.moved = true;
                    self.resize = Some(drag);
                }
                // `from_corners` normalisiert: zieht man über die Gegenecke
                // hinaus, entsteht kein negatives Rechteck, sondern ein
                // gespiegeltes.
                self.state
                    .set_region_rect(index, redact_core::Rect::from_corners(drag.anchor, corner));
            }
            if frame.button_is_up() {
                // Losgelassen oder abgebrochen — der Zug ist vorbei.
                self.resize = None;
                // Die Erfolgsmeldung darf die Warnung nicht überschreiben, dass
                // dieser Zug eine geschützte Stelle zur Schwärzung gemacht hat
                // — sie ist die wichtigere der beiden Nachrichten.
                if drag.moved && self.state.status != crate::state::PROTECTION_OVERRIDDEN {
                    self.state.status = "Rechteck angepasst".to_string();
                }
            }
            return;
        }

        // 2. Beginnt der Zug auf einem Eckgriff der ausgewählten Region?
        //
        // Schon im **Druckbild**, nicht erst bei `drag_started`: sonst legte
        // der Selektor über den ersten 6 pt eine Vorschau über den Griff, die
        // beim Erkennen des Zuges wieder verschwände.
        if frame.drag_started || frame.pressed {
            if let Some(drag) = self.grab_handle(&frame, view, origin, page, zoom) {
                // Der Zug gehört jetzt dem Griff; der Selektor darf ihn nicht
                // zusätzlich als neues Rechteck sehen.
                self.selector.cancel();
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
            moved: false,
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
    /// Die entscheidende Frage ist **nicht** „hat irgendein Widget den Fokus“,
    /// sondern „liegt er in einem Textfeld“. Vorher stand hier
    /// `focused().is_some()`: ein einziger Druck auf Tabulator setzte den Fokus
    /// auf einen Knopf der Symbolleiste (`Sense::click()` ist fokussierbar),
    /// und von da an waren Entf, die Pfeiltasten und Strg+O/S/Z/Y tot. Welche
    /// Kennungen zu Textfeldern gehören, sagt [`crate::focus`].
    ///
    /// Für Escape zählt zusätzlich der Fokus des **vorigen** Bildendes: egui
    /// nimmt dem Feld den Fokus beim Escape schon in `Focus::begin_pass`, also
    /// bevor diese Zeile ihn abfragt.
    fn read_keys(&self, ctx: &egui::Context) -> KeyState {
        let in_field = crate::focus::in_text_field(ctx);
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
            key_r: i.key_pressed(Key::R),
            text_focus: self
                .text_focus
                .owns_keys(in_field, i.key_pressed(Key::Escape)),
        })
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let keys = self.read_keys(ctx);
        let commands = key_commands(keys, self.state.selected_region.is_some());
        self.apply_key_commands(&commands);
        // Am Bildende merken, wo der Fokus liegt: das nächste Bild beginnt mit
        // diesem Stand, und ein Escape räumt ihn ab, bevor er hier ankäme.
        self.text_focus.remember(crate::focus::in_text_field(ctx));
    }

    /// Beendet einen laufenden Zug am Eckgriff, bevor eine Taste die Region
    /// verändert.
    ///
    /// Vorher lief der Zug weiter: das nächste Mausbild setzte das Rechteck
    /// wieder auf Anker und Zeiger und überschrieb damit den Tastenschritt —
    /// der stand dann nur noch als zweiter Schritt im Verlauf. Beendet wird
    /// wie beim Blättern mitten im Zug: der bisher gezogene Stand bleibt
    /// stehen, der Selektor bleibt bis zum Loslassen gesperrt.
    fn end_handle_drag(&mut self) {
        if self.resize.take().is_some() {
            self.selector.cancel();
        }
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
                    // Auch das gerade aufgezogene Rechteck endet hier. Vorher
                    // wurde nur `resize` geräumt: Entf mitten im Aufziehen
                    // löschte die ausgewählte Region **und** legte beim
                    // Loslassen trotzdem noch das neue Rechteck an.
                    self.selector.cancel();
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
                    self.end_handle_drag();
                    self.state.move_selected(dx, dy);
                }
                KeyCommand::Resize { dx, dy } => {
                    self.end_handle_drag();
                    self.state.resize_selected(dx, dy);
                }
                KeyCommand::AddRegion => {
                    // Wie beim Ziehen mit der Maus: ein laufender Zug am
                    // Eckgriff und ein halb aufgezogenes Rechteck haben nach
                    // einem neuen Rechteck nichts mehr zu suchen.
                    self.selector.cancel();
                    self.resize = None;
                    if self.state.add_region_in_page_middle().is_none() {
                        self.state.status =
                            "Kein Dokument geladen — es gibt keine Seite für ein Rechteck"
                                .to_string();
                    }
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

        // Fertige Seitenbilder und Nachprüfungen abholen, bevor gezeichnet
        // wird.
        self.ui_ctx = Some(ctx.clone());
        self.pages.poll(ctx);
        self.poll_export_checks();
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
                crate::thumbnails::show(
                    ui,
                    &mut self.state,
                    &mut self.pages,
                    &summary,
                    page_changed,
                );
            });

        let toggle = egui::SidePanel::left("sidebar")
            .default_width(SIDEBAR_WIDTH)
            .width_range(SIDEBAR_MIN_WIDTH..=SIDEBAR_MAX_WIDTH)
            .show(ctx, |ui| {
                crate::sidebar::show(ui, &mut self.state, &summary)
            })
            .inner;
        // Erst zeichnen, dann umschalten: die Rückfrage öffnet ein Fenster des
        // Systems, und das gehört nicht in die Mitte eines Panels.
        if let Some(toggle) = toggle {
            self.apply_pattern_toggle(toggle);
        }

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

// Prüfrunde 5: Bedienfolgen quer durch Tastatur, Maus und Verlauf. Eigene
// Datei, weil sie mit den Prüfrunden wächst — aber **Kindmodul von `app`**,
// denn sie fährt `apply_pointer` und `resize` unmittelbar an.
#[cfg(test)]
#[path = "rev5_tests.rs"]
mod rev5_tests;

// Prüfrunde 6: Bedienung ohne Maus, der Notnagel-Pfad und die Umbauten aus
// v0.4.0. Aus demselben Grund Kindmodul von `app` wie die Runde davor.
#[cfg(test)]
#[path = "rev6_tests.rs"]
mod rev6_tests;

// Prüfrunde „Bedienung“ (Z4): die Tastaturwege aus v0.5.0/v0.6.0 und ihre
// Zusammenspiele. Kindmodul von `app` aus demselben Grund wie die Runden davor.
#[cfg(test)]
#[path = "z4_tests.rs"]
mod z4_tests;

// Prüfrunde 8: die Nachprüfung nach dem Export und die drei Ungenauigkeiten,
// die Z4 belegt hinterlassen hat.
#[cfg(test)]
#[path = "rev8_tests.rs"]
mod rev8_tests;

// Prüfrunde Z-B: die Nachprüfung auf einem eigenen Thread, die Kleinbild-Decke,
// die Trefferliste nur mit sichtbaren Zeilen und vier Kleinigkeiten an Tastatur
// und Griff. Kindmodul von `app` wie die Runden davor — `export_to`, `resize`
// und `apply_pointer` sind privat.
#[cfg(test)]
#[path = "zb_tests.rs"]
mod zb_tests;

// Gegenprüfung R4, Teil 2: der Prüf-Thread selbst — `start_export_check`,
// der Haken `CheckGate`, die Warnungsliste über mehrere Ausgabedateien.
// Kindmodul von `app` aus demselben Grund wie `zb_tests`: `export_to`,
// `checks`, `hold_check` und `ui_ctx` sind privat.
#[cfg(test)]
#[path = "zg_r4_app_tests.rs"]
mod zg_r4_app_tests;

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
    ///
    /// **Dieser Test prüft nur die reine Funktion.** `text_focus` steht hier
    /// von Hand auf `true`; wie dieses Feld zustande kommt, sieht er nicht —
    /// und genau dort saßen zwei Fehler (ein Knopf mit Fokus galt als Textfeld,
    /// und Escape kam nie mit `text_focus == true` an). Den echten Ablauf
    /// prüfen `a_tab_press_does_not_kill_every_shortcut`,
    /// `escape_in_the_replacement_field_only_leaves_the_field` und
    /// `a_really_focused_text_field_swallows_the_backspace` an einem laufenden
    /// [`egui::Context`].
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
            key_r: true,
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

    // ------------------------------- Tastatur am echten Kontext (Befund 4/5)
    //
    // Der Test oben setzt `text_focus` von Hand — genau daran ist die
    // bestehende Prüfung vorbeigelaufen: sie sieht nicht, **wie** dieses Feld
    // zustande kommt. Die folgenden Tests fahren deshalb echte Bilder eines
    // `egui::Context` ab, mit echten Ereignissen.

    /// Ein ganzes Bild ohne Fenster — dieselben Panels in derselben Reihenfolge
    /// wie in [`eframe::App::update`], samt Tastenauswertung am Ende.
    fn run_frame(
        ctx: &egui::Context,
        app: &std::cell::RefCell<RedactApp>,
        input: egui::RawInput,
    ) -> egui::FullOutput {
        ctx.run(input, |ctx| {
            let summary = app.borrow().state.hit_summary();
            egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
                app.borrow_mut().top_bar(ui);
            });
            // Wie in `update`: die Seitenleiste **meldet** eine Umschaltung,
            // ausgeführt wird sie danach. Ohne diese zwei Zeilen liefe ein Test,
            // der auf ein Kästchen klickt, ins Leere.
            let toggle = egui::SidePanel::left("sidebar")
                .show(ctx, |ui| {
                    let mut app = app.borrow_mut();
                    crate::sidebar::show(ui, &mut app.state, &summary)
                })
                .inner;
            if let Some(toggle) = toggle {
                app.borrow_mut().apply_pattern_toggle(toggle);
            }
            egui::CentralPanel::default().show(ctx, |ui| {
                if app.borrow().state.is_loaded() {
                    app.borrow_mut().paint_page(ui, &summary);
                } else {
                    empty_state(ui);
                }
            });
            app.borrow_mut().handle_keys(ctx);
        })
    }

    /// Ein Fenster in der Größe, die die Anwendung wirklich öffnet — sonst
    /// steht der Standardbereich von egui auf 10000 pt, und die untere Leiste
    /// der Seitenleiste (mit dem Feld „Ersetzen“) läge außerhalb des Bildes,
    /// also auch außerhalb jedes Klicks.
    fn base_input() -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                Vec2::new(crate::WINDOW_SIZE[0], crate::WINDOW_SIZE[1]),
            )),
            ..Default::default()
        }
    }

    /// Eingabe mit gedrückten Tasten.
    fn keys_input(keys: &[Key]) -> egui::RawInput {
        egui::RawInput {
            events: keys
                .iter()
                .map(|key| egui::Event::Key {
                    key: *key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                })
                .collect(),
            ..base_input()
        }
    }

    /// Eingabe mit gedrückter bzw. losgelassener Maustaste an einer Stelle.
    fn click_input(pos: Pos2, pressed: bool) -> egui::RawInput {
        egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..base_input()
        }
    }

    /// Eine Anwendung mit geladenem Demo-Auszug.
    fn loaded_app() -> std::cell::RefCell<RedactApp> {
        let app = std::cell::RefCell::new(RedactApp::silent(iban_only()));
        app.borrow_mut()
            .open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "demo.pdf");
        assert!(app.borrow().state.is_loaded());
        app
    }

    /// Zeichnet so lange, bis die Leisten ihre endgültige Größe haben, und
    /// gibt die Mitte des Feldes „Ersetzen“ zurück.
    ///
    /// Die ersten Bilder eines `egui::Context` sind Näherungen: Panelbreite und
    /// -höhe stehen erst fest, wenn der Inhalt einmal gemessen wurde. Wer zu
    /// früh klickt, klickt neben das Feld.
    fn settle_and_find_the_replacement_field(
        ctx: &egui::Context,
        app: &std::cell::RefCell<RedactApp>,
    ) -> Pos2 {
        let id = crate::focus::id(crate::focus::REPLACEMENT);
        // Feste Zahl statt „bis sich nichts mehr ändert“: die ersten beiden
        // Bilder liefern **zweimal dieselbe** Näherung, ein Abbruch bei
        // Gleichheit stiege also zu früh aus.
        for _ in 0..10 {
            run_frame(ctx, app, base_input());
        }
        ctx.read_response(id)
            .expect("das Feld „Ersetzen“ muss gezeichnet sein")
            .rect
            .center()
    }

    /// **Befund: die Polsterung war im Bild nicht zu sehen** — hier durch das
    /// ganze Bild geprüft: mit `--padding 6` malt die Seite mehr als ohne, und
    /// die Seitenleiste nennt die Zahl.
    #[test]
    fn the_padding_reaches_the_page_image_and_the_sidebar() {
        let shapes_with = |padding: f64| -> usize {
            let ctx = egui::Context::default();
            let app = std::cell::RefCell::new(RedactApp::silent(Config {
                padding,
                ..iban_only()
            }));
            app.borrow_mut()
                .open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "demo.pdf");
            assert!(
                app.borrow().state.hit_summary().redacted > 0,
                "ohne Schwärzung gäbe es nichts zu polstern"
            );
            let mut last = 0;
            for _ in 0..3 {
                last = run_frame(&ctx, &app, base_input()).shapes.len();
            }
            last
        };
        let without = shapes_with(0.0);
        assert!(
            shapes_with(6.0) > without,
            "--padding 6 vergrößert jeden Balken — das gehört ins Bild"
        );
        assert!(
            shapes_with(1.0) > without,
            "auch die Vorgabe 1,0 wirkt und war unsichtbar"
        );

        // Und die Zahl steht daneben, nicht nur in der Kommandozeile.
        assert!(crate::sidebar::padding_text(6.0).contains("6.0"));
        assert!(crate::sidebar::padding_text(0.0).contains("keine"));
    }

    /// **Befund: ein Druck auf Tabulator legte sämtliche Tastenkürzel lahm.**
    /// `read_keys` fragte `m.focused().is_some()` — und nach einem Tab sitzt
    /// der Fokus auf einem Knopf der Symbolleiste (`Sense::click()` ist
    /// fokussierbar). Von da an galten Entf, Pfeile und Strg+O/S/Z/Y als „gehört
    /// dem Textfeld“, ohne dass eines im Spiel war.
    #[test]
    fn a_tab_press_does_not_kill_every_shortcut() {
        let ctx = egui::Context::default();
        let app = loaded_app();
        app.borrow_mut()
            .state
            .add_manual_region(0, Rect::new(10.0, 10.0, 50.0, 20.0), "Gehalt");
        let before = app.borrow().state.regions.len();
        let selected = app.borrow().state.selected_region;
        assert!(selected.is_some());

        run_frame(&ctx, &app, base_input());
        run_frame(&ctx, &app, keys_input(&[Key::Tab]));

        // Ohne diese beiden Zusicherungen prüfte der Test nichts: der Fokus
        // muss wirklich irgendwo sitzen, und zwar in keinem Textfeld.
        assert!(
            ctx.memory(|m| m.focused()).is_some(),
            "Tab muss den Fokus auf ein Widget setzen"
        );
        assert!(
            !crate::focus::in_text_field(&ctx),
            "und dieses Widget ist kein Textfeld"
        );

        run_frame(&ctx, &app, keys_input(&[Key::Delete]));
        assert_eq!(
            app.borrow().state.regions.len(),
            before - 1,
            "nach einem Tab war Entf tot — und mit ihm die ganze Tastaturbedienung"
        );

        // Und die Kürzel mit Steuerungstaste ebenso: Strg+Z holt sie zurück.
        let mut undo = keys_input(&[Key::Z]);
        for event in &mut undo.events {
            if let egui::Event::Key { modifiers, .. } = event {
                *modifiers = egui::Modifiers::COMMAND;
            }
        }
        undo.modifiers = egui::Modifiers::COMMAND;
        run_frame(&ctx, &app, undo);
        assert_eq!(
            app.borrow().state.regions.len(),
            before,
            "Strg+Z muss nach einem Tab genauso wirken"
        );
    }

    /// **Befund: die Fokus-Sperre griff bei Escape nicht.** egui räumt den
    /// Fokus bei Escape in `Focus::begin_pass` ab — also bevor `read_keys` am
    /// Bildende fragt. Escape im Feld „Ersetzen“ verließ deshalb nicht nur das
    /// Feld, sondern hob zusätzlich die Auswahl auf.
    #[test]
    fn escape_in_the_replacement_field_only_leaves_the_field() {
        use redact_core::Action;

        let ctx = egui::Context::default();
        let app = loaded_app();
        app.borrow_mut().state.selected_region = Some(0);
        app.borrow_mut()
            .state
            .set_action(0, Action::Replace("[GEHALT]".into()));

        let pos = settle_and_find_the_replacement_field(&ctx, &app);

        // Ein echter Klick hinein — kein von Hand gesetzter Fokus.
        run_frame(&ctx, &app, click_input(pos, true));
        run_frame(&ctx, &app, click_input(pos, false));
        assert!(
            crate::focus::in_text_field(&ctx),
            "der Klick muss den Fokus ins Textfeld setzen"
        );
        assert_eq!(app.borrow().state.selected_region, Some(0));

        // Escape: es verlässt das Feld — und sonst nichts.
        run_frame(&ctx, &app, keys_input(&[Key::Escape]));
        assert!(
            !crate::focus::in_text_field(&ctx),
            "egui nimmt dem Feld den Fokus"
        );
        assert_eq!(
            app.borrow().state.selected_region,
            Some(0),
            "dieses Escape gehörte dem Feld, nicht der Auswahl"
        );

        // Gegenprobe: das **nächste** Escape gilt wieder der Fläche.
        run_frame(&ctx, &app, keys_input(&[Key::Escape]));
        assert_eq!(
            app.borrow().state.selected_region,
            None,
            "sonst käme man aus der Auswahl nie wieder heraus"
        );
    }

    /// Und die eigentliche Zusage, am echten Kontext: liegt der Fokus im
    /// Textfeld, gehört **jede** Taste dorthin — die Rücktaste löscht dann kein
    /// Rechteck.
    #[test]
    fn a_really_focused_text_field_swallows_the_backspace() {
        use redact_core::Action;

        let ctx = egui::Context::default();
        let app = loaded_app();
        app.borrow_mut().state.selected_region = Some(0);
        app.borrow_mut()
            .state
            .set_action(0, Action::Replace("[GEHALT]".into()));
        let before = app.borrow().state.regions.len();

        let pos = settle_and_find_the_replacement_field(&ctx, &app);
        run_frame(&ctx, &app, click_input(pos, true));
        run_frame(&ctx, &app, click_input(pos, false));
        assert!(crate::focus::in_text_field(&ctx));

        run_frame(&ctx, &app, keys_input(&[Key::Backspace]));
        assert_eq!(
            app.borrow().state.regions.len(),
            before,
            "die Rücktaste im Textfeld darf keine Region löschen"
        );
        assert_eq!(app.borrow().state.selected_region, Some(0));
    }

    /// Eingabe mit getipptem Text.
    fn text_input(text: &str) -> egui::RawInput {
        egui::RawInput {
            events: vec![egui::Event::Text(text.to_string())],
            ..base_input()
        }
    }

    /// **Befund: Tippen im Feld „Ersetzen“ flutete den Rückgängig-Stapel** —
    /// hier über die Oberfläche selbst, mit echten Tastenereignissen: die
    /// Seitenleiste muss den Text über
    /// [`crate::state::AppState::edit_replacement`] führen und nicht über
    /// `set_action`.
    #[test]
    fn typing_in_the_replacement_field_costs_exactly_one_undo_step() {
        use redact_core::Action;

        let ctx = egui::Context::default();
        let app = loaded_app();
        app.borrow_mut().state.selected_region = Some(0);
        app.borrow_mut()
            .state
            .set_action(0, Action::Replace("[X]".into()));

        let pos = settle_and_find_the_replacement_field(&ctx, &app);
        run_frame(&ctx, &app, click_input(pos, true));
        run_frame(&ctx, &app, click_input(pos, false));
        assert!(crate::focus::in_text_field(&ctx), "Fokus im Feld");

        let depth = app.borrow().state.history.undo_depth();
        let typed = "Kontoinhaber";
        for ch in typed.chars() {
            run_frame(&ctx, &app, text_input(&ch.to_string()));
        }

        let action = app.borrow().state.regions[0].action.clone();
        let Action::Replace(text) = action else {
            panic!("die Art muss „Ersetzen“ bleiben");
        };
        assert!(
            text.contains(typed),
            "der getippte Text muss ankommen: {text:?}"
        );
        assert_eq!(
            app.borrow().state.history.undo_depth(),
            depth + 1,
            "{} Anschläge dürfen einen Schritt kosten, nicht {}",
            typed.chars().count(),
            typed.chars().count()
        );

        // Ein Rückgängig führt zum Stand vor dem Tippen.
        app.borrow_mut().state.undo();
        assert_eq!(
            app.borrow().state.regions[0].action,
            Action::Replace("[X]".into())
        );
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

        // Strg + Pfeiltaste blättert **nicht** und verschiebt nicht — es
        // ändert seit dieser Runde die Größe der Auswahl (das Gegenstück zum
        // Eckgriff, siehe [`KeyCommand::Resize`]). Ohne Auswahl gibt es nichts
        // zu ändern, und blättern soll es ausdrücklich nicht: dafür genügt der
        // Pfeil allein. In einem Textfeld kommt es ohnehin nicht an — dort
        // greift `text_focus` schon vor diesem Zweig.
        //
        // Bis Z-B stand hier zusätzlich `delete: true` — und der Test hielt
        // fest, dass Strg+Entf **nichts** tut. Das war der Befund, nicht die
        // Absicht: die Tastentabelle verspricht Entf ohne Vorbehalt. Seither
        // gilt Entf mit und ohne Strg (siehe `zb_tests`).
        let ctrl_and_arrow = KeyState {
            ctrl: true,
            left: true,
            ..KeyState::default()
        };
        assert_eq!(
            key_commands(ctrl_and_arrow, true),
            vec![KeyCommand::Resize {
                dx: -NUDGE,
                dy: 0.0
            }]
        );
        assert!(key_commands(ctrl_and_arrow, false).is_empty());

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

    // -------------------------------- Rückmeldung ab dem Druck (Bild für Bild)

    /// Das Bild, in dem die Taste auf der Seite heruntergeht — ohne jede
    /// Bewegung und ohne `drag_started`.
    fn press_frame(press: Pos2) -> PointerFrame {
        PointerFrame {
            pressed: true,
            down: true,
            pos: Some(press),
            press_origin: Some(press),
            ..PointerFrame::default()
        }
    }

    /// Ein Bild mit gedrückter Taste unterhalb der Klickschwelle: egui meldet
    /// hier weder `drag_started` noch `dragged` noch `drag_stopped`.
    fn below_threshold_frame(press: Pos2, pos: Pos2) -> PointerFrame {
        PointerFrame {
            down: true,
            pos: Some(pos),
            press_origin: Some(press),
            ..PointerFrame::default()
        }
    }

    /// Das Bild des Loslassens nach einem bloßen Klick: die Taste ist oben,
    /// `press_origin` ist weg, und `drag_stopped` kommt nicht — egui hatte nie
    /// einen Zug erkannt.
    fn click_release_frame(pos: Pos2) -> PointerFrame {
        PointerFrame {
            pos: Some(pos),
            ..PointerFrame::default()
        }
    }

    /// **Der zweite Teil des gemeldeten Fehlers.** Das Rechteck muss vom
    /// Moment des Drückens an zu sehen sein und an der Druckstelle beginnen.
    /// Vorher fing der Selektor erst bei `drag_started` an — also nach bis zu
    /// 6 pt Mausweg, in denen nichts zu sehen war und danach ein fertiges
    /// 6-pt-Rechteck auftauchte.
    #[test]
    fn a_press_on_the_page_shows_a_rectangle_at_once_and_at_the_press_point() {
        let view = viewer::PageView::upright(offset_box());
        let mut app = RedactApp::silent(Config::default());
        let press = ORIGIN + Vec2::new(120.0, 90.0);

        app.apply_pointer(press_frame(press), false, &view, ORIGIN, 0, ZOOM);
        assert!(
            app.selector.is_active(),
            "nach dem Druckbild läuft schon ein Zug"
        );
        let preview = app.selector.preview().expect("Vorschau ab dem Druckbild");
        assert_eq!(preview.min, press, "die Ecke liegt, wo geklickt wurde");
        assert_eq!(preview.max, press);
        assert!(app.resize.is_none(), "und es ist keine Größenänderung");
        assert!(app.state.regions.is_empty(), "angelegt wird noch nichts");

        // Zwei Punkte Bewegung — immer noch unter der Schwelle.
        let two = press + Vec2::splat(2.0);
        app.apply_pointer(
            below_threshold_frame(press, two),
            false,
            &view,
            ORIGIN,
            0,
            ZOOM,
        );
        let preview = app.selector.preview().expect("Vorschau unter der Schwelle");
        assert_eq!(preview.min, press);
        assert_eq!(
            (preview.width(), preview.height()),
            (2.0, 2.0),
            "zwei Punkte zeigen zwei Punkte — nicht nichts und nicht sechs"
        );
    }

    /// Ein Klick ohne Bewegung legt **keine** Region an, sondern wählt aus.
    #[test]
    fn a_click_selects_and_never_draws_a_rectangle() {
        let view = viewer::PageView::upright(offset_box());
        let rect = Rect::new(100.0, 300.0, 260.0, 340.0);
        let (mut app, screen) = app_with_selected_region(&view, rect);
        app.state.selected_region = None;

        // Mitten in die vorhandene Region: drücken, loslassen, kein Weg dazwischen.
        let press = screen.center();
        app.apply_pointer(press_frame(press), false, &view, ORIGIN, 0, ZOOM);
        app.apply_pointer(click_release_frame(press), true, &view, ORIGIN, 0, ZOOM);

        assert_eq!(
            app.state.regions.len(),
            1,
            "ein Klick darf keine Region anlegen"
        );
        assert_eq!(
            app.state.selected_region,
            Some(0),
            "er wählt die vorhandene Region aus"
        );
        assert!(
            !app.selector.is_active() && app.selector.preview().is_none(),
            "und lässt keinen Zug offen stehen"
        );

        // Dasselbe auf leerer Fläche: keine Region, keine Auswahl, kein Zug.
        let empty = screen.left_top() + Vec2::new(-90.0, -90.0);
        app.apply_pointer(press_frame(empty), false, &view, ORIGIN, 0, ZOOM);
        app.apply_pointer(click_release_frame(empty), true, &view, ORIGIN, 0, ZOOM);
        assert_eq!(app.state.regions.len(), 1);
        assert_eq!(app.state.selected_region, None);
        assert!(app.selector.preview().is_none());
    }

    /// Ein **Druck auf einem Eckgriff** beginnt eine Größenänderung — keine
    /// Neuanlage und auch keine Vorschau über dem Griff. Die Rangfolge aus
    /// [`RedactApp::apply_pointer`] gilt also schon im Druckbild.
    #[test]
    fn pressing_a_corner_handle_starts_a_resize_instead_of_a_new_rectangle() {
        let view = viewer::PageView::upright(offset_box());
        let rect = Rect::new(100.0, 300.0, 260.0, 340.0);

        for handle in crate::selector::HANDLES {
            let (mut app, screen) = app_with_selected_region(&view, rect);
            let depth = app.state.history.undo_depth();
            let press = handle.pos(screen) + Vec2::new(3.0, -3.0);

            app.apply_pointer(press_frame(press), false, &view, ORIGIN, 0, ZOOM);

            let what = format!("{handle:?}");
            assert!(app.resize.is_some(), "{what}: der Griff ist angefasst");
            assert!(
                !app.selector.is_active() && app.selector.preview().is_none(),
                "{what}: über dem Griff darf keine Vorschau liegen"
            );
            assert_eq!(app.state.regions.len(), 1, "{what}: nichts Neues");
            assert_eq!(
                app.state.regions[0].region.rect, rect,
                "{what}: der bloße Druck verschiebt noch keine Ecke"
            );

            // Weiterziehen unterhalb der Klickschwelle: hier soll die Ecke
            // bereits folgen, und zwar der Region, nicht einem neuen Rechteck.
            let moved = press + Vec2::new(3.0, 3.0);
            app.apply_pointer(
                below_threshold_frame(press, moved),
                false,
                &view,
                ORIGIN,
                0,
                ZOOM,
            );
            let anchor =
                viewer::screen_to_pdf_point(handle.opposite().pos(screen), &view, ZOOM, ORIGIN);
            assert_rect_close(
                app.state.regions[0].region.rect,
                Rect::from_corners(
                    anchor,
                    viewer::screen_to_pdf_point(moved, &view, ZOOM, ORIGIN),
                ),
                &what,
            );
            assert_eq!(
                app.state.history.undo_depth(),
                depth + 1,
                "{what}: genau ein Schnappschuss"
            );
            assert_eq!(
                app.state.regions.len(),
                1,
                "{what}: immer noch nichts Neues"
            );
        }
    }

    /// Ein Griff, der nur angetippt und ohne Bewegung wieder losgelassen wird,
    /// ändert nichts — und hinterlässt auch keinen Rückgängig-Schritt, der
    /// nichts zurücknähme.
    #[test]
    fn merely_tapping_a_handle_changes_nothing_at_all() {
        let view = viewer::PageView::upright(offset_box());
        let rect = Rect::new(100.0, 300.0, 260.0, 340.0);
        let (mut app, screen) = app_with_selected_region(&view, rect);
        let depth = app.state.history.undo_depth();

        let press = screen.right_bottom();
        app.apply_pointer(press_frame(press), false, &view, ORIGIN, 0, ZOOM);
        assert!(app.resize.is_some());
        app.apply_pointer(click_release_frame(press), true, &view, ORIGIN, 0, ZOOM);

        assert!(app.resize.is_none(), "der Zug ist vorbei");
        assert_eq!(app.state.regions[0].region.rect, rect, "nichts verschoben");
        assert_eq!(
            app.state.history.undo_depth(),
            depth,
            "und kein leerer Schritt im Verlauf"
        );
        assert_eq!(app.state.regions.len(), 1);
    }

    /// Ein vollständiger Zug **ab dem Druckbild**, wie egui ihn liefert:
    /// Druck, ein paar Bilder unterhalb der Klickschwelle, dann `drag_started`,
    /// weiterziehen, loslassen. Am Ende steht ein Rechteck, dessen Ecke am
    /// Druckpunkt liegt.
    #[test]
    fn a_full_drag_starting_at_the_press_frame_ends_at_the_press_point() {
        let view = viewer::PageView::upright(offset_box());
        let mut app = RedactApp::silent(Config::default());
        let press = ORIGIN + Vec2::new(150.0, 120.0);
        let release = press + Vec2::new(80.0, 60.0);

        app.apply_pointer(press_frame(press), false, &view, ORIGIN, 0, ZOOM);
        assert_eq!(
            app.selector.preview().map(|r| r.min),
            Some(press),
            "schon das Druckbild zeigt die Ecke"
        );
        for step in [2.0_f32, 4.0] {
            app.apply_pointer(
                below_threshold_frame(press, press + Vec2::splat(step)),
                false,
                &view,
                ORIGIN,
                0,
                ZOOM,
            );
            let preview = app.selector.preview().expect("Vorschau unter der Schwelle");
            assert_eq!(preview.min, press);
            assert_eq!(preview.width(), step, "das Rechteck wächst mit der Maus");
        }
        app.apply_pointer(
            PointerFrame {
                drag_started: true,
                dragged: true,
                down: true,
                pos: Some(press + Vec2::splat(7.0)),
                press_origin: Some(press),
                ..PointerFrame::default()
            },
            false,
            &view,
            ORIGIN,
            0,
            ZOOM,
        );
        app.apply_pointer(
            PointerFrame {
                drag_stopped: true,
                pos: Some(release),
                ..PointerFrame::default()
            },
            false,
            &view,
            ORIGIN,
            0,
            ZOOM,
        );

        assert_eq!(app.state.regions.len(), 1, "genau ein Rechteck");
        assert_rect_close(
            app.state.regions[0].region.rect,
            Rect::from_corners(
                viewer::screen_to_pdf_point(press, &view, ZOOM, ORIGIN),
                viewer::screen_to_pdf_point(release, &view, ZOOM, ORIGIN),
            ),
            "Ecke am Druckpunkt",
        );
        assert!(app.selector.preview().is_none(), "und die Vorschau ist weg");
    }

    /// Die Ansage aus [`crate::state::PROTECTION_OVERRIDDEN`] muss den ganzen
    /// Zug überleben: „Rechteck angepasst“ beim Loslassen darf sie nicht
    /// überschreiben — sie ist die wichtigere der beiden Nachrichten.
    #[test]
    fn the_protection_warning_survives_the_end_of_the_drag() {
        let view = viewer::PageView::upright(offset_box());
        let mut app = RedactApp::silent(Config::default());
        let protector = Rect::new(100.0, 300.0, 400.0, 400.0);
        let covered = Rect::new(150.0, 320.0, 250.0, 360.0);
        app.state.regions.push(crate::state::AnnotatedRegion::new(
            redact_core::Region::new(
                0,
                protector,
                Some("Max Mustermann".into()),
                redact_core::Source::Booking {
                    booking_id: "b003".into(),
                    match_type: redact_core::MatchType::Negative,
                },
            ),
        ));
        app.state.regions.push(crate::state::AnnotatedRegion::new(
            redact_core::Region::new(
                0,
                covered,
                Some("DE89 3704 0044 0532 0130 00".into()),
                redact_core::Source::Pattern {
                    pattern_id: "iban_de".into(),
                    confidence: 0.99,
                },
            ),
        ));
        app.state.selected_region = Some(1);
        assert_eq!(app.state.hit_summary().redacted, 0, "gedeckt vom Schutz");

        let screen = viewer::pdf_to_screen(&covered, &view, ZOOM, ORIGIN);
        let press = screen.right_bottom();
        drag(
            &mut app,
            &view,
            press,
            &[press + Vec2::new(4.0, 4.0), press + Vec2::new(9.0, 9.0)],
        );

        assert_eq!(
            app.state.hit_summary().redacted,
            1,
            "der angefasste Treffer überstimmt den Schutz"
        );
        assert_eq!(
            app.state.status,
            crate::state::PROTECTION_OVERRIDDEN,
            "und das muss am Ende des Zuges noch dastehen"
        );
    }

    // ------------------------------------ Abbruch mitten im Zug (Escape/Entf)

    /// **Die Kehrseite von „Rechteck ab dem Druck sichtbar“.** Escape
    /// unterhalb der Klickschwelle räumte nur auf; egui reichte den
    /// `drag_started` desselben Tastendrucks danach nach, der Selektor begann
    /// einen **zweiten** Zug, und beim Loslassen lag eine zusätzliche manuelle
    /// Region da. Gemessene Folge: Druck auf den Eckgriff → Escape → jetzt
    /// erst `drag_started` → ziehen → loslassen.
    #[test]
    fn escape_below_the_click_threshold_holds_until_the_button_is_up() {
        let view = viewer::PageView::upright(offset_box());
        let rect = Rect::new(100.0, 300.0, 260.0, 340.0);
        let (mut app, screen) = app_with_selected_region(&view, rect);

        let press = screen.right_bottom();
        app.apply_pointer(press_frame(press), false, &view, ORIGIN, 0, ZOOM);
        assert!(app.resize.is_some(), "der Griff ist angefasst");

        // Escape — derselbe Weg wie die Taste im Fenster.
        app.apply_key_commands(&[KeyCommand::Deselect]);
        assert!(app.resize.is_none() && !app.selector.is_active());

        // Und **jetzt** erst meldet egui den Zug, mit derselben Taste unten.
        app.apply_pointer(
            dragging_frame(press, press + Vec2::new(40.0, 30.0), true),
            false,
            &view,
            ORIGIN,
            0,
            ZOOM,
        );
        assert!(
            !app.selector.is_active(),
            "ein Abbruch darf keinen zweiten Zug beginnen"
        );
        app.apply_pointer(
            PointerFrame {
                drag_stopped: true,
                pos: Some(press + Vec2::new(40.0, 30.0)),
                ..PointerFrame::default()
            },
            false,
            &view,
            ORIGIN,
            0,
            ZOOM,
        );

        assert_eq!(
            app.state.regions.len(),
            1,
            "beim Loslassen darf kein zusätzliches Rechteck entstehen"
        );
        assert_eq!(app.state.regions[0].region.rect, rect, "und keins wandern");

        // Gegenprobe: der **nächste** Tastendruck zeichnet wieder ganz normal —
        // sonst wäre die Sperre eine neue Sackgasse.
        let start = ORIGIN + Vec2::new(300.0, 400.0);
        app.apply_pointer(press_frame(start), false, &view, ORIGIN, 0, ZOOM);
        assert!(
            app.selector.is_active(),
            "die Sperre gilt nur bis zum Loslassen"
        );
        app.apply_pointer(
            PointerFrame {
                drag_stopped: true,
                pos: Some(start + Vec2::new(60.0, 40.0)),
                ..PointerFrame::default()
            },
            false,
            &view,
            ORIGIN,
            0,
            ZOOM,
        );
        assert_eq!(app.state.regions.len(), 2, "das nächste Rechteck entsteht");
    }

    /// Dasselbe für Entf: mitten im Aufziehen gedrückt, löschte es die
    /// ausgewählte Region **und** legte beim Loslassen trotzdem noch das
    /// aufgezogene Rechteck an — [`RedactApp::apply_key_commands`] räumte nur
    /// `resize`, nicht den Selektor.
    #[test]
    fn delete_while_drawing_does_not_still_produce_the_rectangle() {
        let view = viewer::PageView::upright(offset_box());
        let rect = Rect::new(100.0, 300.0, 260.0, 340.0);
        let (mut app, _screen) = app_with_selected_region(&view, rect);

        // Auf leerer Fläche aufziehen — die vorhandene Region bleibt ausgewählt.
        let press = ORIGIN + Vec2::new(320.0, 420.0);
        app.apply_pointer(press_frame(press), false, &view, ORIGIN, 0, ZOOM);
        app.apply_pointer(
            dragging_frame(press, press + Vec2::new(50.0, 40.0), true),
            false,
            &view,
            ORIGIN,
            0,
            ZOOM,
        );
        assert!(app.selector.is_active());

        app.apply_key_commands(&[KeyCommand::DeleteSelected]);
        assert!(app.state.regions.is_empty(), "die Auswahl ist gelöscht");

        app.apply_pointer(
            PointerFrame {
                drag_stopped: true,
                pos: Some(press + Vec2::new(50.0, 40.0)),
                ..PointerFrame::default()
            },
            false,
            &view,
            ORIGIN,
            0,
            ZOOM,
        );
        assert!(
            app.state.regions.is_empty(),
            "ein abgebrochenes Aufziehen darf beim Loslassen nichts anlegen"
        );
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
                    crate::thumbnails::show(ui, state, pages, &summary, false);
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

    /// **Befund: „Analysieren“ warf jede Auswahlentscheidung weg, ohne zu
    /// fragen.** Gemessen: Treffer 0 abgewählt, Treffer 1 auf „Weiß“ →
    /// `hand_made_count() == 2`; nach einem Druck auf „Analysieren“ war beides
    /// zurückgesetzt, und die Statuszeile meldete zufrieden die Trefferzahl.
    /// Die vier anderen Wege zum selben Verlust fragen nach — dieser eine
    /// nicht.
    #[test]
    fn analysing_again_asks_before_it_throws_the_decisions_away() {
        use redact_core::Action;

        // Die Nutzerin antwortet „Nein“ — gefragt werden muss sie trotzdem.
        let mut app = RedactApp::refusing(iban_only());
        app.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "demo.pdf");
        assert!(
            app.state.regions.len() >= 2,
            "der Demo-Auszug muss mehrere Treffer haben"
        );

        assert!(app.state.set_enabled(0, false), "Treffer 0 abwählen");
        assert!(
            app.state.set_action(1, Action::Whiteout),
            "Treffer 1 auf Weiß"
        );
        assert_eq!(app.state.hand_made_count(), 2);
        let before = app.state.regions.clone();

        assert!(!app.analyze(), "abgelehnt heißt: nicht gelaufen");
        assert_eq!(
            app.state.regions, before,
            "eine abgelehnte Rückfrage darf nichts wegwerfen"
        );
        assert_eq!(app.state.hand_made_count(), 2);
        assert!(
            app.state.status.contains("abgebrochen"),
            "und sie muss es sagen: {}",
            app.state.status
        );

        // Gegenprobe: mit „Ja“ läuft die Analyse und setzt beides zurück —
        // sonst prüfte der Test nur, dass nie analysiert wird.
        app.ask = Ask::Answer(true);
        assert!(app.analyze());
        assert_eq!(app.state.hand_made_count(), 0);
        assert!(app.state.status.starts_with("Analyse:"));
    }

    // ------------------------- Abschaltbare automatische Funde (Aufgabe #81)

    /// Eine Buchungsliste, die „Max Mustermann“ schützt.
    fn protecting_list(dir: &Path) -> PathBuf {
        let path = dir.join("liste.csv");
        std::fs::write(
            &path,
            "id,list_type,pattern\nb003,negative,\"Max Mustermann\"\n",
        )
        .unwrap();
        path
    }

    /// Wie viele Regionen aus einem Muster stammen.
    fn automatic(app: &RedactApp) -> usize {
        app.state
            .regions
            .iter()
            .filter(|a| matches!(a.region.source, redact_core::Source::Pattern { .. }))
            .count()
    }

    /// **Die Auflage (1) in der Oberfläche: ganz aus heißt kein automatischer
    /// Treffer — und Handarbeit und Schutzmarken überstehen es.**
    ///
    /// Das ist die eigentliche Prüfung dieses Schalters: er rechnet die
    /// Trefferliste neu, und dabei darf weder ein selbst gezogenes Rechteck
    /// noch ein Schutzeintrag der Buchungsliste verlorengehen.
    #[test]
    fn switching_the_detection_off_keeps_hand_drawn_regions_and_protections() {
        let dir = temp_dir("erkennung-aus");
        let mut app = RedactApp::silent(Config {
            booking_list: Some(protecting_list(&dir)),
            ..Config::default()
        });
        app.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "demo.pdf");

        app.state
            .add_manual_region(0, Rect::new(70.0, 745.0, 250.0, 760.0), "Anschrift");
        let vorher_automatisch = automatic(&app);
        let vorher_schutz = app.state.hit_summary().protecting;
        assert!(vorher_automatisch >= 3, "{vorher_automatisch}");
        assert!(vorher_schutz > 0, "die Schutzmarke fehlt schon vorher");

        app.apply_pattern_toggle(crate::sidebar::PatternToggle::All(false));

        assert_eq!(automatic(&app), 0, "es darf kein Muster mehr laufen");
        assert_eq!(
            app.state
                .regions
                .iter()
                .filter(|a| matches!(a.region.source, redact_core::Source::Manual { .. }))
                .count(),
            1,
            "das selbst gezogene Rechteck ist weg"
        );
        assert_eq!(
            app.state.hit_summary().protecting,
            vorher_schutz,
            "die Schutzmarke der Buchungsliste ist weg"
        );
        assert!(app.state.detection_notice().is_some());

        // Und zurück: dieselben Treffer wie vorher.
        app.apply_pattern_toggle(crate::sidebar::PatternToggle::All(true));
        assert_eq!(automatic(&app), vorher_automatisch);
        assert_eq!(app.state.detection_notice(), None);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// **Die Auflage (2) in der Oberfläche: ein Muster aus, die übrigen an.**
    #[test]
    fn switching_off_one_pattern_leaves_the_others_running() {
        let mut app = RedactApp::silent(Config::default());
        app.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "demo.pdf");
        let ids = |app: &RedactApp| -> Vec<String> {
            let mut ids: Vec<String> = app
                .state
                .regions
                .iter()
                .filter_map(|a| match &a.region.source {
                    redact_core::Source::Pattern { pattern_id, .. } => Some(pattern_id.clone()),
                    _ => None,
                })
                .collect();
            ids.sort();
            ids.dedup();
            ids
        };
        let alle = ids(&app);
        assert!(alle.contains(&"email".to_string()), "{alle:?}");

        app.apply_pattern_toggle(crate::sidebar::PatternToggle::One {
            id: "email".to_string(),
            on: false,
        });

        let übrig = ids(&app);
        assert!(!übrig.contains(&"email".to_string()), "{übrig:?}");
        assert_eq!(
            übrig,
            alle.iter()
                .filter(|id| *id != "email")
                .cloned()
                .collect::<Vec<_>>()
        );
        assert!(!app.state.pattern_enabled("email"));
        assert!(app.state.pattern_enabled("iban_de"));
    }

    /// Und die Rückfrage: wird sie abgelehnt, bleibt **auch das Kästchen**
    /// stehen. Ein Schalter, der sich schon umgestellt hat, während die Liste
    /// noch die alte ist, wäre die schlimmere Lüge.
    #[test]
    fn a_refused_toggle_changes_neither_the_hits_nor_the_switch() {
        let mut app = RedactApp::refusing(iban_only());
        app.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "demo.pdf");
        app.state
            .add_manual_region(0, Rect::new(70.0, 745.0, 250.0, 760.0), "Anschrift");
        let vorher = app.state.regions.clone();
        assert!(app.state.has_manual_work());

        app.apply_pattern_toggle(crate::sidebar::PatternToggle::All(false));
        assert_eq!(app.state.regions, vorher, "nichts durfte sich ändern");
        assert!(app.state.patterns_enabled(), "der Schalter steht weiter an");
        assert!(app.state.detection_notice().is_none());
        assert!(
            app.state.status.contains("abgebrochen"),
            "und es gehört gesagt: {}",
            app.state.status
        );

        // Gegenprobe mit „Ja“ — sonst prüfte der Test nur, dass nie etwas
        // geschieht.
        app.ask = Ask::Answer(true);
        app.apply_pattern_toggle(crate::sidebar::PatternToggle::All(false));
        assert!(!app.state.patterns_enabled());
        assert_eq!(automatic(&app), 0);
    }

    /// **Die Kopfzeile darf niemals zweideutig sein.**
    ///
    /// Der Befund, um den es geht: bei abgeschalteter Automatik stand dort
    /// „0 Treffer · 0 werden geschwärzt“ — Wort für Wort dasselbe wie bei einem
    /// Dokument, in dem wirklich nichts steht. Genau dieselbe Zeile für „nichts
    /// gefunden“ und „nicht gesucht“.
    #[test]
    fn the_headline_never_reads_the_same_for_found_nothing_and_did_not_look() {
        let mut app = RedactApp::silent(Config::default());
        app.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "demo.pdf");
        let gesucht = app.state.hit_summary().headline();
        assert!(!gesucht.contains("AUS"), "{gesucht}");

        app.apply_pattern_toggle(crate::sidebar::PatternToggle::All(false));
        let nicht_gesucht = app.state.hit_summary().headline();
        assert_ne!(gesucht, nicht_gesucht);
        assert!(
            nicht_gesucht.contains("AUS") && nicht_gesucht.contains("nicht gesucht"),
            "die Kopfzeile verschweigt die Abschaltung: {nicht_gesucht}"
        );
        // Und der Beweis, dass die Zeile ohne diesen Zusatz mehrdeutig wäre:
        // die Zahlen sind dieselben wie bei einem leeren Dokument.
        let leer = AppState::new().hit_summary().headline();
        assert!(
            leer.starts_with("0 Treffer · 0 werden geschwärzt"),
            "{leer}"
        );
        assert!(!nicht_gesucht.starts_with("0 Treffer"), "{nicht_gesucht}");

        // Einzeln abgeschaltet: die Zahl steht daneben, denn hier ist die
        // Trefferzahl nicht null und trotzdem unvollständig.
        app.apply_pattern_toggle(crate::sidebar::PatternToggle::All(true));
        app.apply_pattern_toggle(crate::sidebar::PatternToggle::One {
            id: "email".to_string(),
            on: false,
        });
        let teilweise = app.state.hit_summary().headline();
        assert!(teilweise.contains("1 Muster abgeschaltet"), "{teilweise}");
    }

    /// „Analysieren“ ist grau, wenn es nichts zu finden gäbe — und **nicht**,
    /// solange eine Buchungsliste dahintersteht.
    #[test]
    fn the_analyse_button_says_when_it_could_not_find_anything() {
        let dir = temp_dir("knopf");
        let mut app = RedactApp::silent(Config::default());
        app.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "demo.pdf");
        assert!(toolbar::is_enabled(
            ToolAction::Analyze,
            &app.tool_context()
        ));

        app.apply_pattern_toggle(crate::sidebar::PatternToggle::All(false));
        assert!(
            !toolbar::is_enabled(ToolAction::Analyze, &app.tool_context()),
            "ohne Muster und ohne Liste findet der Knopf nichts"
        );

        // Mit Buchungsliste ist er wieder benutzbar: die Analyse hat dann eine
        // Quelle, auch ganz ohne Muster.
        app.state.config.booking_list = Some(protecting_list(&dir));
        assert!(toolbar::is_enabled(
            ToolAction::Analyze,
            &app.tool_context()
        ));
        assert!(app.state.analysis_can_find_anything());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Der zweite Weg zu „nichts läuft mehr“: jedes Kästchen einzeln
    /// abwählen. Er führt zu demselben Zustand wie der große Schalter und
    /// gehört genauso behandelt — sonst bliebe ein Knopf benutzbar, der
    /// nachweislich eine leere Liste erzeugt.
    #[test]
    fn unticking_every_single_pattern_counts_as_switched_off_too() {
        let mut app = RedactApp::silent(Config::default());
        app.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "demo.pdf");

        let ids: Vec<String> = app
            .state
            .pattern_states()
            .iter()
            .map(|def| def.id.clone())
            .collect();
        assert!(ids.len() > 5, "{ids:?}");
        for id in &ids {
            app.apply_pattern_toggle(crate::sidebar::PatternToggle::One {
                id: id.clone(),
                on: false,
            });
        }

        assert_eq!(automatic(&app), 0);
        assert!(
            !app.state.analysis_can_find_anything(),
            "alle Kästchen aus ist dasselbe wie der große Schalter aus"
        );
        assert!(!toolbar::is_enabled(
            ToolAction::Analyze,
            &app.tool_context()
        ));
        // Der große Schalter steht dabei weiter auf „an“ — die Kopfzeile darf
        // deshalb nicht „AUS“ behaupten, sondern muss die Zahl nennen.
        assert!(app.state.patterns_enabled());
        let headline = app.state.hit_summary().headline();
        assert!(
            headline.contains(&format!("{} Muster abgeschaltet", ids.len())),
            "{headline}"
        );

        // Und eines wieder an: alles kommt zurück.
        app.apply_pattern_toggle(crate::sidebar::PatternToggle::One {
            id: "iban_de".to_string(),
            on: true,
        });
        assert!(app.state.analysis_can_find_anything());
        assert!(automatic(&app) > 0);
    }

    /// Ohne geladenes Dokument gilt die Einstellung trotzdem — sie wirkt beim
    /// nächsten Öffnen, und die Statuszeile sagt es.
    #[test]
    fn the_switch_works_before_a_document_is_open() {
        let mut app = RedactApp::silent(Config::default());
        app.apply_pattern_toggle(crate::sidebar::PatternToggle::All(false));
        assert!(!app.state.patterns_enabled());
        assert!(
            app.state.status.contains(redact_pipeline::DETECTION_NOTICE),
            "{}",
            app.state.status
        );

        app.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "demo.pdf");
        assert_eq!(automatic(&app), 0, "die Einstellung galt beim Öffnen nicht");
    }

    /// Auch ein geladenes Review geht diesen Weg — „Buchungsliste laden“ ruft
    /// dieselbe Analyse. Und wird sie abgelehnt, darf auch die Buchungsliste
    /// nicht heimlich in der Konfiguration stehen bleiben.
    #[test]
    fn a_refused_analysis_keeps_a_loaded_review_and_forgets_the_booking_list() {
        let dir = temp_dir("refused-analysis");
        let mut author = RedactApp::silent(iban_only());
        author.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "demo.pdf");
        author
            .state
            .add_manual_region(0, Rect::new(11.0, 12.0, 33.0, 44.0), "aus der Durchsicht");
        author.state.set_enabled(0, false);
        let review = dir.join("durchsicht.json");
        std::fs::write(&review, author.state.to_review_file().to_json().unwrap()).unwrap();

        let mut app = RedactApp::refusing(iban_only());
        app.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "demo.pdf");
        app.load_review_file(&review);
        assert!(app.error.is_none(), "{:?}", app.error);
        let loaded = app.state.regions.clone();
        assert!(app.state.has_manual_work(), "das Review trägt Handarbeit");

        // Der Weg über „Buchungsliste laden“ landet in derselben Analyse. Die
        // Liste ist ausdrücklich brauchbar: eine kaputte Datei ließe die
        // Analyse ohnehin scheitern, und der Test prüfte dann nichts.
        let list = dir.join("liste.csv");
        std::fs::write(
            &list,
            "id,list_type,pattern\nb001,positive,Musterfirma GmbH\n",
        )
        .unwrap();
        let previous = app.state.config.booking_list.replace(list);
        if !app.analyze() {
            app.state.config.booking_list = previous;
        }
        assert_eq!(app.state.regions, loaded, "das Review bleibt stehen");
        assert_eq!(
            app.state.config.booking_list, None,
            "eine Liste ohne Analyse wäre eine Einstellung ohne Wirkung"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// **Befund: „Review speichern“ meldete keinen Erfolg.** `report(Ok(()))`
    /// setzt nur `error = None` und lässt die Statuszeile stehen — gemessen:
    /// Status vorher und nachher identisch, die Datei lag aber da.
    #[test]
    fn saving_a_review_says_that_it_worked() {
        let dir = temp_dir("review-status");
        let mut app = RedactApp::silent(iban_only());
        app.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "demo.pdf");
        let before = app.state.status.clone();

        let path = dir.join("durchsicht.json");
        app.save_review_to(&path);
        assert!(path.exists(), "die Datei muss da sein");
        assert!(app.error.is_none(), "{:?}", app.error);
        assert_ne!(
            app.state.status, before,
            "ein Knopf, der arbeitet, darf nicht schweigen"
        );
        assert!(
            app.state.status.contains("gespeichert") && app.state.status.contains("durchsicht"),
            "{}",
            app.state.status
        );
        assert!(
            app.state.status.contains("Klartext"),
            "die Datei trägt die gefundenen Texte offen: {}",
            app.state.status
        );

        // Gegenprobe: ein Fehlschlag steht weiterhin rot in der Zeile — hier
        // ein Ziel, das schon ein Verzeichnis ist.
        app.save_review_to(&dir);
        assert!(app.error.is_some(), "{}", app.state.status);

        std::fs::remove_dir_all(&dir).ok();
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

    // --------------------------- Review-Dateien: die Grenze vor dem Lesen

    /// **Die Auflage:** eine dünn belegte Riesendatei wird abgelehnt, **bevor**
    /// sie gelesen wird — und die Ablehnung erreicht die Oberfläche.
    ///
    /// Das war der fünfte Weg derselben Art, den die Sicherheitsprüfung nicht
    /// gefunden hat: `load_review_file` las mit `std::fs::read_to_string`, was
    /// im Dateidialog angeklickt worden war. Der Dialog ist dabei kein Schutz —
    /// er liefert einen Namen, nicht eine Zusicherung über das, was hinter ihm
    /// liegt. Eine Datei mit 6 GB Nennlänge (4 kB auf der Platte) belegte 6 GB
    /// Arbeitsspeicher, und die Oberfläche stand so lange still.
    ///
    /// Geprüft wird beides: dass die Grenze *vor* dem Lesen greift — zu sehen
    /// daran, dass die Meldung die Größe nennt, die sie nur aus der Angabe des
    /// Dateisystems haben kann — und dass die Meldung in der Statuszeile
    /// **und** in `error` ankommt, also weder als Absturz noch als stilles
    /// Nichts.
    #[test]
    fn a_sparse_giant_review_file_is_refused_and_the_window_says_so() {
        let dir = temp_dir("riesig-review");
        let path = dir.join("riesig.json");
        let file = std::fs::File::create(&path).unwrap();
        // Dünn belegt: 6 GB Nennlänge, 0 Byte geschrieben.
        file.set_len(6 * 1024 * 1024 * 1024).unwrap();
        drop(file);

        let mut app = RedactApp::silent(iban_only());
        app.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "a.pdf");
        let before = app.state.regions.clone();

        app.load_review_file(&path);

        let error = app.error.clone().expect("die Datei muss abgelehnt werden");
        assert!(error.contains("riesig.json"), "{error}");
        assert!(error.contains("6144 MB"), "die Größe fehlt: {error}");
        assert!(error.contains("16 MB"), "die Grenze fehlt: {error}");
        assert!(error.contains("feste Grenze"), "der Hinweis fehlt: {error}");
        // In der Oberfläche sichtbar: die Statuszeile trägt denselben Text.
        assert_eq!(app.state.status, error, "die Statuszeile schweigt");
        assert_eq!(
            app.state.regions, before,
            "es darf nichts übernommen werden"
        );
        // Und das Hinweisfenster verspricht nicht das Falsche: es liegt nicht
        // an der Zugehörigkeit zum Dokument.
        assert_eq!(
            review_error_title(&redact_core::RedactError::Parse(error)),
            "Review-Datei nicht lesbar"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// **Die Auflage:** eine benannte Pipe lässt die Oberfläche nicht stehen.
    ///
    /// `#[cfg(unix)]`, weil es unter Windows kein `mkfifo` und keine benannte
    /// Pipe im Dateisystem gibt, die man im Dateidialog anklicken könnte — der
    /// Angriffsweg existiert dort nicht, und ein Test, der ihn nachstellen
    /// wollte, scheiterte schon am Anlegen.
    ///
    /// Der Test kommt ohne Schreiber am anderen Ende aus, und das ist gerade
    /// der Punkt: schon das *Öffnen* einer Pipe ohne Schreiber blockiert
    /// endlos. Gearbeitet wird deshalb in einem eigenen Faden mit
    /// Zeitschranke — griffe die Grenze nicht, hinge sonst der Testlauf selbst,
    /// und ein hängender Test ist schlimmer als ein roter.
    #[cfg(unix)]
    #[test]
    fn a_named_pipe_chosen_in_the_dialog_does_not_freeze_the_window() {
        let dir = temp_dir("pipe-review");
        let path = dir.join("pipe.json");
        let ok = std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .expect("mkfifo startbar");
        assert!(ok.success(), "mkfifo ist fehlgeschlagen");

        // `RedactApp` bleibt in seinem Faden — nur die Meldung kommt zurück.
        let (sender, receiver) = std::sync::mpsc::channel();
        let pfad = path.clone();
        std::thread::spawn(move || {
            let mut app = RedactApp::silent(iban_only());
            app.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "a.pdf");
            app.load_review_file(&pfad);
            let _ = sender.send((app.error.clone(), app.state.status.clone()));
        });

        let (error, status) = receiver
            .recv_timeout(std::time::Duration::from_secs(20))
            .expect("die Oberfläche hängt an der Pipe, statt sie abzulehnen");

        let error = error.expect("eine Pipe muss abgelehnt werden");
        assert!(error.contains("gewöhnliche Datei"), "{error}");
        assert!(error.contains("pipe.json"), "{error}");
        assert_eq!(status, error, "die Statuszeile schweigt");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Die Gegenprobe: eine gewöhnliche Review-Datei geht weiterhin durch.
    ///
    /// Ohne diesen Test wäre der Weg nur zugemauert statt abgesichert. Der
    /// Umweg über die Platte ist dabei Absicht — geprüft wird genau der Weg,
    /// den der Dateidialog nimmt.
    #[test]
    fn an_ordinary_review_file_still_loads() {
        let dir = temp_dir("legitim-review");
        let mut app = RedactApp::silent(iban_only());
        app.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "a.pdf");
        app.state
            .add_manual_region(0, Rect::new(10.0, 10.0, 50.0, 20.0), "Gehalt");
        let path = dir.join("review.json");
        std::fs::write(&path, app.state.to_review_file().to_json().unwrap()).unwrap();
        // Verglichen werden die Rechtecke, nicht die `RegionId`s: die werden
        // beim Laden neu vergeben, und das ist richtig so.
        let erwartet: Vec<_> = app.state.regions.iter().map(|r| r.region.clone()).collect();

        app.state.regions.clear();
        app.load_review_file(&path);

        assert!(app.error.is_none(), "{:?}", app.error);
        let geladen: Vec<_> = app.state.regions.iter().map(|r| r.region.clone()).collect();
        assert_eq!(geladen, erwartet);
        assert!(
            app.state.status.contains("Review übernommen"),
            "{}",
            app.state.status
        );

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
        app.wait_for_export_checks();

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
        plain.wait_for_export_checks();
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
