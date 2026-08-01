//! Kern-Traits: die Nahtstellen zwischen den Crates.

use crate::error::Result;
use crate::geometry::TextRun;

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
