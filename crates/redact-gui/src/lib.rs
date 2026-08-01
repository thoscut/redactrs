//! # redact-gui
//!
//! Grafische Oberfläche zum Prüfen und Zeichnen von Schwärzungen — egui/eframe,
//! reines Rust, keine nativen Zusatzbibliotheken.
//!
//! ## Aufbau
//!
//! ```text
//!  state.rs      Zustand + Fachlogik (ohne egui, vollständig unit-getestet)
//!  focus.rs      Wem ein Tastendruck gehört: Textfeld oder Oberfläche
//!  history.rs    Rückgängig/Wiederholen als Schnappschuss-Stapel
//!  viewer.rs     Koordinatenumrechnung PDF ↔ Bildschirm, schematische Notvorschau
//!  render.rs     Seitenbilder aus `redact-render`, im Hintergrund-Thread
//!  selector.rs   Rechteck aufziehen, Treffersuche
//!  thumbnails.rs Miniaturansichten (nutzt die Kleinbilder aus render.rs)
//!  sidebar.rs    Trefferliste und Details
//!  toolbar.rs    Symbolleiste als Daten (Symbol + Text, Freigaberegeln)
//!  theme.rs      helles und dunkles Thema
//!  app.rs        eframe-App: Leisten, Tasten, Dialoge
//! ```
//!
//! Die Trennung ist Absicht: alle Rechnungen (Drehung, Y-Spiegelung, Zoom),
//! Zustandsübergänge (an/aus, verschieben, löschen), die Tastenbelegung und der
//! Export liegen in gewöhnlichen Funktionen, die ohne Fenster und ohne
//! Grafikkontext getestet werden. Die egui-Module rufen sie nur auf.
//!
//! ## Dateien öffnen
//!
//! Über „PDF öffnen …“ oder per Ziehen und Ablegen auf das Fenster. Was mit
//! einer Menge abgelegter Dateien geschieht, entscheidet [`classify_drop`] —
//! eine reine Funktion, damit das Verhalten (erste PDF gewinnt, Nicht-PDFs
//! werden abgelehnt) ohne Maus prüfbar ist. Bevor dabei von Hand gezogene
//! Rechtecke verloren gehen, wird gefragt.
//!
//! ## Seitendarstellung
//!
//! Der Hauptbereich zeigt das von `redact-render` gerasterte Seitenbild. Es
//! entsteht auf einem eigenen Thread ([`render`]), damit Seitenwechsel und
//! Zoomen die Oberfläche nicht anhalten. Kann eine Seite nicht rasterisiert
//! werden, springt die schematische Vorschau aus [`viewer`] ein und zeigt
//! wenigstens die Lage des Textes.
//!
//! Schwärzungsrechtecke liegen im **ungedrehten** User-Space, das Bild zeigt
//! die Seite **nach** `/Rotate`. Die Umrechnung dazwischen macht
//! [`viewer::PageView`]; ohne sie säßen die Rechtecke auf gedrehten Seiten an
//! der falschen Stelle.
//!
//! Links steht eine Spalte mit Miniaturansichten ([`thumbnails`]). Sie zeigt
//! **dieselben** Kleinbilder, die der Hauptbereich ohnehin anfordert — es wird
//! nichts doppelt gerendert.
//!
//! ## Bedienung
//!
//! * Symbolleiste ([`toolbar`]): jeder Knopf trägt Symbol **und** Wort; welche
//!   Knöpfe wann benutzbar sind, entscheidet [`toolbar::is_enabled`].
//! * Thema ([`theme`]): hell oder dunkel, umschaltbar. Die vier Trefferfarben
//!   sind in beiden Themen dieselben und erreichen überall mindestens 3:1
//!   (WCAG 1.4.11) — geprüft gegen die Flächen, die egui wirklich malt.
//! * Rückgängig/Wiederholen ([`history`]): bis zu [`HISTORY_LIMIT`]
//!   Schnappschüsse der Trefferliste, Strg+Z und Strg+Y.
//! * Tastatur: Bild auf/ab und Pos1/Ende blättern, Pfeiltasten verschieben die
//!   Auswahl (sonst blättern sie), Entf löscht, Strg+O öffnet, Strg+S
//!   exportiert. Liegt der Fokus in einem **Textfeld**, gehören **alle** Tasten
//!   dorthin — siehe [`key_commands`] und [`focus`]. „Textfeld“ heißt dabei
//!   genau das: ein Knopf mit Fokus (nach einem Druck auf Tabulator) ist keins,
//!   und Escape gehört noch dem Feld, dem egui den Fokus schon genommen hat.
//!
//! ## Review-Dateien gehören zu genau einem Dokument
//!
//! Eine Review-Datei ist eine Liste von Rechtecken ohne Bezug zum Inhalt. Auf
//! ein anderes PDF angewendet läge jedes Rechteck an einer beliebigen Stelle:
//! das Ergebnis sähe geschwärzt aus, und die Geheimnisse stünden noch da.
//! Deshalb schreibt [`AppState::to_review_file`] die SHA-256-Prüfsumme des
//! Eingabedokuments mit, und [`AppState::apply_review_file`] gibt die
//! Entscheidung an [`redact_pipeline::check_review_identity`] —
//! **buchstäblich dieselbe Funktion**, die `--apply-review` benutzt, nicht
//! bloß dieselbe Regel. Eine abweichende Prüfsumme wird abgelehnt, eine
//! fehlende ebenfalls (nur `--allow-unverified-review` kommt daran vorbei).
//!
//! ## Was die Schalter der Kommandozeile hier tun
//!
//! Die Oberfläche bekommt **dieselbe** [`Config`] wie ein Lauf ohne `--gui`,
//! und sie benutzt sie auch — Muster, Buchungsliste, Schwellwert, Polsterung,
//! Ladegrenzen, Passwort. Drei Schalter fielen dabei still unter den Tisch;
//! sie gelten jetzt (Befund #67):
//!
//! * `--action` / `--replace-with`: die Schwärzungsart eines frisch gefundenen
//!   Treffers. [`AnnotatedRegion`] trug hier fest „schwarz“, `--gui --action
//!   replace` schwärzte also schwarz. Jetzt legt
//!   [`AppState`] neue Einträge mit der Art aus der
//!   Konfiguration an — in der Trefferliste steht sie **sichtbar** und ist dort
//!   je Treffer änderbar.
//! * `--apply-review`: die Review-Datei wird beim Start angewendet
//!   ([`RedactApp::open_startup_document`]) und tritt damit an die Stelle der
//!   Analyse, genau wie in `redact_pipeline::run`. Die Prüfsumme entscheidet
//!   dabei mit derselben Funktion wie dort.
//! * `--audit-log`: der genannte Pfad gilt auch hier
//!   ([`AppState::audit_target`]); ohne ihn
//!   bleibt es beim Namen neben der Ausgabedatei.
//! * `--review-out`: der genannte Pfad ist der Vorschlag im Dialog „Review
//!   speichern“ ([`AppState::suggested_review_path`]) — dieselbe Regel wie
//!   `redact_pipeline::run`. Vorher kam der Schalter in diesem Crate überhaupt
//!   nicht vor.
//!
//! Zwei Schalter können in einem Fenster **nicht** wörtlich gelten, und das ist
//! sichtbar, nicht stillschweigend:
//!
//! * `--output`/`-o` und `--force`: geschrieben wird erst auf Knopfdruck, und
//!   der Speichern-Dialog fragt vor dem Überschreiben selbst. Der Pfad aus `-o`
//!   ist der **Vorschlag** im Dialog, den man sieht und ändern kann. Er gehört
//!   dem Dokument, für das er genannt wurde: solange er gilt, ist das Feld
//!   „Namenszusatz“ abgeschaltet (es bewirkte sonst sichtbar nichts), und beim
//!   Öffnen eines **anderen** Dokuments verfällt er
//!   ([`AppState::output_name_is_fixed`]).
//! * `--review`: die Oberfläche *ist* die Durchsicht. Wer hier „Exportieren“
//!   drückt, will eine geschwärzte Datei — siehe
//!   [`AppState::export_config`].
//!
//! ## Die Verarbeitungskette liegt woanders
//!
//! Analyse und Export dieser Oberfläche sind Aufrufe von
//! [`redact_pipeline::collect_regions`] und [`redact_pipeline::apply`]. Hier
//! stehen nur Fenster, Auswahl und Zustand. Bis Aufgabe 5 waren es rund 130
//! abgetippte Zeilen neben der CLI — und sie waren auseinandergelaufen.
//!
//! ## Beispiel
//!
//! ```no_run
//! # use std::path::PathBuf;
//! let config = redact_pipeline::Config {
//!     input: PathBuf::from("auszug.pdf"),
//!     ..redact_pipeline::Config::default()
//! };
//! redact_gui::run(config)?;
//! # Ok::<(), redact_core::RedactError>(())
//! ```

#![forbid(unsafe_code)]

pub mod app;
pub mod focus;
pub mod history;
pub mod render;
pub mod selector;
pub mod sidebar;
pub mod state;
pub mod theme;
pub mod thumbnails;
pub mod toolbar;
pub mod viewer;

pub use app::{
    classify_drop, is_pdf_name, key_commands, DropAction, KeyCommand, KeyState, RedactApp,
    EMPTY_DOCUMENT_HINT,
};
pub use focus::TextFieldFocus;
pub use history::{History, HISTORY_LIMIT};
pub use render::{PageCache, PageMeta};
pub use selector::{hit_handle, hit_test, Handle, HandleDrag, PointerFrame, RectangleSelector};
pub use state::{
    sha256_hex, AnnotatedRegion, AppState, HitOutcome, HitSummary, RegionColor, RegionId,
};
// Die Identitätsprüfung liegt jetzt in der gemeinsamen Kette; hier bleibt nur
// der gewohnte Name.
pub use redact_pipeline::{review_identity, Config, ReviewIdentity};
pub use theme::Theme;
pub use toolbar::{ToolAction, ToolButton, ToolContext, ToolItem};
pub use viewer::{pdf_to_screen, screen_to_pdf, PagePreview, PageView, RegionStyle};

use redact_core::{RedactError, Result};

/// Fensterbreite und -höhe beim Start.
pub const WINDOW_SIZE: [f32; 2] = [1280.0, 860.0];
/// Fenstertitel.
pub const WINDOW_TITLE: &str = "redact-rs";

/// Startet die grafische Oberfläche.
///
/// Die Nahtstelle zur CLI (`redact-rs --gui`) ist **die Konfiguration der
/// Verarbeitungskette selbst**: [`redact_pipeline::Config`], dieselbe Struktur,
/// die `redact_pipeline::run` bekommt. Damit gelten in der Oberfläche
/// dieselben Schalter wie auf der Kommandozeile — vorher reichte sie nur PDF,
/// Buchungsliste und Muster-IDs durch, und `--patterns-config`,
/// `--no-patterns`, `--min-confidence`, `--manual-regions` und `--padding`
/// blieben draußen.
///
/// Ist [`Config::input`] gesetzt, wird die Datei geladen und sofort
/// analysiert; ein Fehler dabei beendet das Programm **nicht**, sondern
/// erscheint in der Statuszeile. Was beim Start sonst noch aus der
/// Konfiguration folgt (`--apply-review`), macht
/// [`RedactApp::open_startup_document`] — dort steht es ohne Fenster geprüft.
pub fn run(config: Config) -> Result<()> {
    let mut app = RedactApp::new(config);
    app.open_startup_document();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(WINDOW_TITLE)
            .with_inner_size(WINDOW_SIZE),
        ..Default::default()
    };

    eframe::run_native(
        WINDOW_TITLE,
        options,
        Box::new(|_cc| Ok(Box::new(app) as Box<dyn eframe::App>)),
    )
    .map_err(|e| RedactError::Config(format!("Grafische Oberfläche nicht startbar: {e}")))
}
