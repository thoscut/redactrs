//! # redact-gui
//!
//! Grafische Oberfläche zum Prüfen und Zeichnen von Schwärzungen — egui/eframe,
//! reines Rust, keine nativen Zusatzbibliotheken.
//!
//! ## Aufbau
//!
//! ```text
//!  state.rs    Zustand + Fachlogik (ohne egui, vollständig unit-getestet)
//!  viewer.rs   Koordinatenumrechnung PDF ↔ Bildschirm, schematische Notvorschau
//!  render.rs   Seitenbilder aus `redact-render`, im Hintergrund-Thread
//!  selector.rs Rechteck aufziehen, Treffersuche
//!  sidebar.rs  Seiten- und Trefferliste
//!  app.rs      eframe-App: Leisten, Tasten, Dialoge
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
//! ## Beispiel
//!
//! ```no_run
//! # use std::path::PathBuf;
//! redact_gui::run(Some(PathBuf::from("auszug.pdf")), None, vec![])?;
//! # Ok::<(), redact_core::RedactError>(())
//! ```

#![forbid(unsafe_code)]

pub mod app;
pub mod render;
pub mod selector;
pub mod sidebar;
pub mod state;
pub mod viewer;

pub use app::{
    classify_drop, is_pdf_name, key_commands, DropAction, KeyCommand, KeyState, RedactApp,
};
pub use render::{PageCache, PageMeta};
pub use selector::{hit_test, RectangleSelector};
pub use state::{AnnotatedRegion, AppState, HitOutcome, HitSummary, RegionColor};
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
