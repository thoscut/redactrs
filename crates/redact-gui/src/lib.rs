//! # redact-gui
//!
//! Grafische Oberfläche zum Prüfen und Zeichnen von Schwärzungen — egui/eframe,
//! reines Rust, keine nativen Zusatzbibliotheken.
//!
//! ## Aufbau
//!
//! ```text
//!  state.rs      Zustand + Fachlogik (ohne egui, vollständig unit-getestet)
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
//!   exportiert. Liegt der Fokus in einem Textfeld, gehören **alle** Tasten
//!   dorthin — siehe [`key_commands`].
//!
//! ## Review-Dateien gehören zu genau einem Dokument
//!
//! Eine Review-Datei ist eine Liste von Rechtecken ohne Bezug zum Inhalt. Auf
//! ein anderes PDF angewendet läge jedes Rechteck an einer beliebigen Stelle:
//! das Ergebnis sähe geschwärzt aus, und die Geheimnisse stünden noch da.
//! Deshalb schreibt [`AppState::to_review_file`] die SHA-256-Prüfsumme des
//! Eingabedokuments mit, und [`AppState::apply_review_file`] lehnt eine Datei
//! mit abweichender Prüfsumme ab ([`review_identity`]) — dieselbe Regel, die
//! die CLI mit `--apply-review` anwendet.
//!
//! ## Beispiel
//!
//! ```no_run
//! # use std::path::PathBuf;
//! redact_gui::run(Some(PathBuf::from("auszug.pdf")), None, vec![])?;
//! # Ok::<(), redact_core::RedactError>(())
//! ```

#![forbid(unsafe_code)]

pub mod app;
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
pub use history::{History, HISTORY_LIMIT};
pub use render::{PageCache, PageMeta};
pub use selector::{hit_test, RectangleSelector};
pub use state::{
    review_identity, sha256_hex, AnnotatedRegion, AppState, HitOutcome, HitSummary, RegionColor,
    ReviewIdentity,
};
pub use theme::Theme;
pub use toolbar::{ToolAction, ToolButton, ToolContext, ToolItem};
pub use viewer::{pdf_to_screen, screen_to_pdf, PagePreview, PageView, RegionStyle};

use std::path::PathBuf;

use redact_core::{RedactError, Result};

/// Fensterbreite und -höhe beim Start.
pub const WINDOW_SIZE: [f32; 2] = [1280.0, 860.0];
/// Fenstertitel.
pub const WINDOW_TITLE: &str = "redact-rs";

/// Startet die grafische Oberfläche.
///
/// Ist `pdf` gesetzt, wird die Datei geladen und sofort analysiert; ein Fehler
/// dabei beendet das Programm **nicht**, sondern erscheint in der Statuszeile.
/// `booking` ist eine optionale Buchungsliste (CSV), `patterns` die Auswahl der
/// Pattern-IDs (leer = alle eingebauten).
///
/// Diese Signatur ist die Nahtstelle zur CLI (`redact-rs --gui`) und darf sich
/// nicht ändern.
pub fn run(pdf: Option<PathBuf>, booking: Option<PathBuf>, patterns: Vec<String>) -> Result<()> {
    let mut app = RedactApp::new(patterns);
    app.state.booking_path = booking;
    if let Some(path) = pdf {
        app.open_and_analyze(path);
    }

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
