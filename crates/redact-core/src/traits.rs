//! Kern-Traits: die Nahtstellen zwischen den Crates.

use std::path::Path;

use crate::error::Result;
use crate::geometry::TextRun;
use crate::model::{BookingEntry, Redaction, Region};

/// Extrahiert Text aus einem PDF.
///
/// **Abweichung vom Ursprungskonzept:** Der Trait liefert `TextRun`s statt
/// `Region`s. Eine `Region` trägt zwingend eine `Source` — die steht bei der
/// reinen Extraktion aber noch gar nicht fest, sie entsteht erst durch die
/// Analyse. `TextRun` transportiert zusätzlich die zeichengenauen Glyph-Boxen,
/// ohne die sich für einen Regex-Treffer *innerhalb* einer Zeile keine exakte
/// Bounding-Box berechnen ließe.
pub trait Extractor: Send + Sync {
    fn extract(&self, doc: &lopdf::Document) -> Result<Vec<TextRun>>;
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
