//! Textextraktion: aus Glyphen werden Zeilen.
//!
//! Ein PDF kennt keine Zeilen — nur einzeln positionierte Glyphen. Für das
//! Pattern-Matching ist die Zeile aber die entscheidende Einheit: eine IBAN
//! wird in der Praxis über mehrere `Tj`-Operationen verteilt ausgegeben
//! (`DE89 `, `3704 `, `0044 …`). Wer nur einzelne Text-Runs betrachtet, findet
//! sie nicht. Deshalb werden hier alle Glyphen einer Seite eingesammelt,
//! nach Grundlinie gruppiert und zu Zeilen zusammengesetzt.

use lopdf::Document;
use redact_core::{Extractor, Glyph, Rect, Result, TextRun};

use crate::content::{scan_page, GlyphItem};

/// Extrahiert Textzeilen mit zeichengenauen Koordinaten.
#[derive(Debug, Clone)]
pub struct PdfExtractor {
    /// Toleranz (in Punkt), innerhalb derer Glyphen zur selben Zeile zählen.
    pub baseline_tolerance: f64,
    /// Ab welchem Anteil der mittleren Zeichenbreite eine Lücke als
    /// Leerzeichen gilt.
    pub space_ratio: f64,
}

impl Default for PdfExtractor {
    fn default() -> Self {
        Self {
            baseline_tolerance: 2.0,
            space_ratio: 0.28,
        }
    }
}

impl PdfExtractor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Extrahiert die Zeilen einer einzelnen Seite (0-basiert).
    pub fn extract_page(&self, doc: &Document, page_index: usize) -> Result<Vec<TextRun>> {
        let pages = doc.get_pages();
        let Some((_, page_id)) = pages.iter().nth(page_index) else {
            return Ok(Vec::new());
        };
        let scan = scan_page(doc, *page_id)?;
        let glyphs: Vec<GlyphItem> = scan
            .shows
            .iter()
            .flat_map(|s| s.glyphs().cloned())
            .collect();
        Ok(self.build_lines(page_index, glyphs))
    }

    /// Setzt aus einzelnen Glyphen Zeilen zusammen.
    fn build_lines(&self, page: usize, mut glyphs: Vec<GlyphItem>) -> Vec<TextRun> {
        // Codes ohne Textzuordnung fliegen raus, Ersatzzeichen bleiben erhalten:
        // sie halten die Position und verhindern falsche Zusammenschreibung.
        glyphs.retain(|g| !g.text.is_empty());
        if glyphs.is_empty() {
            return Vec::new();
        }

        // Leserichtung herstellen: von oben nach unten, dann von links nach rechts.
        // Die Y-Quantisierung fängt kleine Grundlinien-Schwankungen ab.
        let tol = self.baseline_tolerance.max(0.1);
        glyphs.sort_by(|a, b| {
            let ay = -(a.origin.y / tol).round();
            let by = -(b.origin.y / tol).round();
            ay.partial_cmp(&by)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(
                    a.origin
                        .x
                        .partial_cmp(&b.origin.x)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
        });

        let mut lines: Vec<Vec<GlyphItem>> = Vec::new();
        let mut current: Vec<GlyphItem> = Vec::new();
        let mut current_y = glyphs[0].origin.y;

        for g in glyphs {
            let height = g.rect.height().abs().max(1.0);
            let line_tol = (height * 0.45).max(tol);
            if current.is_empty() || (g.origin.y - current_y).abs() <= line_tol {
                if current.is_empty() {
                    current_y = g.origin.y;
                }
                current.push(g);
            } else {
                lines.push(std::mem::take(&mut current));
                current_y = g.origin.y;
                current.push(g);
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }

        lines
            .into_iter()
            .filter_map(|line| self.assemble_line(page, line))
            .collect()
    }

    /// Baut eine Zeile: Glyphen in Leserichtung, Lücken werden zu Leerzeichen.
    fn assemble_line(&self, page: usize, items: Vec<GlyphItem>) -> Option<TextRun> {
        if items.is_empty() {
            return None;
        }
        let avg_width: f64 = {
            let widths: Vec<f64> = items
                .iter()
                .map(|g| g.rect.width().abs())
                .filter(|w| *w > 0.0)
                .collect();
            if widths.is_empty() {
                1.0
            } else {
                widths.iter().sum::<f64>() / widths.len() as f64
            }
        };
        let gap_threshold = (avg_width * self.space_ratio).max(0.5);

        let mut glyphs: Vec<Glyph> = Vec::with_capacity(items.len() + 8);
        let mut prev_right: Option<f64> = None;
        let mut prev_y = items[0].rect;

        for item in &items {
            if let Some(right) = prev_right {
                let gap = item.rect.ll.x - right;
                let last_is_space = glyphs.last().map(|g| g.ch == ' ').unwrap_or(true);
                if gap > gap_threshold && !last_is_space {
                    glyphs.push(Glyph {
                        ch: ' ',
                        rect: Rect::new(right, prev_y.ll.y, item.rect.ll.x, prev_y.ur.y),
                    });
                }
            }
            for ch in item.text.chars() {
                glyphs.push(Glyph {
                    ch,
                    rect: item.rect,
                });
            }
            prev_right = Some(item.rect.ur.x);
            prev_y = item.rect;
        }

        // Führende/abschließende Leerzeichen entfernen.
        while glyphs.first().map(|g| g.ch == ' ').unwrap_or(false) {
            glyphs.remove(0);
        }
        while glyphs.last().map(|g| g.ch == ' ').unwrap_or(false) {
            glyphs.pop();
        }
        if glyphs.is_empty() {
            return None;
        }

        Some(TextRun::new(page, glyphs))
    }
}

impl Extractor for PdfExtractor {
    fn extract(&self, doc: &Document) -> Result<Vec<TextRun>> {
        let mut out = Vec::new();
        for (index, (_, page_id)) in doc.get_pages().iter().enumerate() {
            let scan = scan_page(doc, *page_id)?;
            let glyphs: Vec<GlyphItem> = scan
                .shows
                .iter()
                .flat_map(|s| s.glyphs().cloned())
                .collect();
            out.extend(self.build_lines(index, glyphs));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use redact_core::Point;

    fn glyph(text: &str, x: f64, y: f64, w: f64) -> GlyphItem {
        GlyphItem {
            bytes: vec![b'x'],
            text: text.to_string(),
            rect: Rect::new(x, y, x + w, y + 10.0),
            origin: Point::new(x, y),
            displacement: w,
        }
    }

    #[test]
    fn groups_glyphs_into_lines() {
        let e = PdfExtractor::new();
        let glyphs = vec![
            glyph("A", 0.0, 100.0, 5.0),
            glyph("B", 5.0, 100.0, 5.0),
            glyph("C", 0.0, 80.0, 5.0),
        ];
        let lines = e.build_lines(0, glyphs);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "AB");
        assert_eq!(lines[1].text, "C");
    }

    #[test]
    fn inserts_space_for_large_gaps() {
        let e = PdfExtractor::new();
        let glyphs = vec![
            glyph("D", 0.0, 100.0, 5.0),
            glyph("E", 5.0, 100.0, 5.0),
            glyph("8", 40.0, 100.0, 5.0),
        ];
        let lines = e.build_lines(0, glyphs);
        assert_eq!(lines[0].text, "DE 8");
        // Die Glyph-Liste muss zeichenweise deckungsgleich bleiben.
        assert_eq!(lines[0].glyphs.len(), lines[0].text.chars().count());
    }

    #[test]
    fn sorts_out_of_order_glyphs_left_to_right() {
        let e = PdfExtractor::new();
        let glyphs = vec![glyph("Z", 20.0, 100.0, 5.0), glyph("A", 0.0, 100.0, 5.0)];
        let lines = e.build_lines(0, glyphs);
        assert!(lines[0].text.starts_with('A'));
    }

    #[test]
    fn ligature_glyph_keeps_char_alignment() {
        let e = PdfExtractor::new();
        let glyphs = vec![glyph("fi", 0.0, 100.0, 8.0), glyph("x", 8.0, 100.0, 5.0)];
        let lines = e.build_lines(0, glyphs);
        assert_eq!(lines[0].text, "fix");
        assert_eq!(lines[0].glyphs.len(), 3);
    }

    #[test]
    fn empty_input_yields_no_lines() {
        assert!(PdfExtractor::new().build_lines(0, vec![]).is_empty());
    }
}
