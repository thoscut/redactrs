//! Kern-Traits: die Nahtstellen zwischen den Crates.

use std::path::Path;

use crate::error::Result;
use crate::geometry::TextRun;
use crate::model::{BookingEntry, Redaction, Region};

/// Extrahiert Text-Regionen aus einem PDF.
pub trait Extractor: Send + Sync {
    fn extract(&self, doc: &lopdf::Document) -> Result<Vec<Region>>;

    /// Zeichengenaue Text-Runs (Zeilen) — Basis für Pattern- und Buchungs-Matching.
    ///
    /// Die Default-Implementierung liefert nichts; `redact-pdf` überschreibt sie.
    fn extract_runs(&self, _doc: &lopdf::Document) -> Result<Vec<TextRun>> {
        Ok(Vec::new())
    }
}

/// Analysiert Text-Runs und erzeugt Treffer-Regionen.
pub trait Analyzer: Send + Sync {
    fn analyze(&self, runs: &[TextRun]) -> Result<Vec<Region>>;
}

/// Wendet Schwärzungen auf das PDF an (mutiert das Document).
pub trait Redactor: Send + Sync {
    fn apply(&self, doc: &mut lopdf::Document, redactions: &[Redaction]) -> Result<()>;
}

/// Rendert das finale PDF.
pub trait Renderer: Send + Sync {
    fn render(&self, doc: &lopdf::Document, path: &Path) -> Result<()>;
}

/// Lädt und parsed eine Buchungsliste.
pub trait BookingLoader: Send + Sync {
    fn load(&self, path: &Path) -> Result<Vec<BookingEntry>>;
}
