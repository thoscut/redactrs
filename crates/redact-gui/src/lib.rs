//! # redact-gui
//!
//! Grafische Oberfläche zum Prüfen und Zeichnen von Schwärzungen — egui/eframe,
//! reines Rust, keine nativen Zusatzbibliotheken.
//!
//! ## Aufbau
//!
//! ```text
//!  state.rs    Zustand + Fachlogik (ohne egui, vollständig unit-getestet)
//!  viewer.rs   Koordinatenumrechnung PDF ↔ Bildschirm, schematische Seitenvorschau
//!  selector.rs Rechteck aufziehen, Treffersuche
//!  sidebar.rs  Seiten- und Trefferliste
//!  app.rs      eframe-App: Leisten, Tasten, Dialoge
//! ```
//!
//! Die Trennung ist Absicht: alle Rechnungen (Y-Spiegelung, Zoom),
//! Zustandsübergänge (an/aus, verschieben, löschen) und der Export liegen in
//! gewöhnlichen Funktionen, die ohne Fenster und ohne Grafikkontext getestet
//! werden. Die egui-Module rufen sie nur auf.
//!
//! ## Dateien öffnen
//!
//! Über „PDF öffnen …“ oder per Ziehen und Ablegen auf das Fenster. Was mit
//! einer Menge abgelegter Dateien geschieht, entscheidet [`classify_drop`] —
//! eine reine Funktion, damit das Verhalten (erste PDF gewinnt, Nicht-PDFs
//! werden abgelehnt) ohne Maus prüfbar ist.
//!
//! ## Vorschau ohne Rasterizer
//!
//! Die Seitendarstellung rastert das PDF **nicht**. Sie zeichnet ein weißes
//! Blatt und setzt darauf die extrahierten Textzeilen an ihren echten
//! User-Space-Koordinaten neu. Das Bild ist schematisch, aber
//! koordinatentreu — genau das, was zum Platzieren von Schwärzungsrechtecken
//! gebraucht wird. Einzelheiten und Grenzen: [`viewer`].
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
pub mod selector;
pub mod sidebar;
pub mod state;
pub mod viewer;

pub use app::{classify_drop, is_pdf_name, DropAction, RedactApp};
pub use selector::{hit_test, RectangleSelector};
pub use state::{AnnotatedRegion, AppState, RegionColor};
pub use viewer::{pdf_to_screen, screen_to_pdf, PagePreview};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_constants_match_the_specification() {
        assert_eq!(WINDOW_SIZE, [1280.0_f32, 860.0_f32]);
        assert_eq!(WINDOW_TITLE, "redact-rs");
    }

    /// `run` lässt sich nicht ohne Bildschirm ausführen; hier wird nur
    /// festgehalten, dass die Signatur die von der CLI erwartete ist.
    #[test]
    fn run_has_the_signature_the_cli_expects() {
        let _: fn(Option<PathBuf>, Option<PathBuf>, Vec<String>) -> Result<()> = run;
    }
}
