//! Geometrie im PDF-User-Space (Punkt = 1/72 Zoll, Y-Achse zeigt nach oben).

use serde::{Deserialize, Serialize};

/// Koordinaten im PDF-User-Space (Punkt = 1/72 Zoll)
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// Achsenparalleles Rechteck. `ll` ist immer links-unten, `ur` rechts-oben.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub ll: Point, // lower-left
    pub ur: Point, // upper-right
}

impl Rect {
    /// Erzeugt ein normalisiertes Rechteck aus zwei beliebigen Eckpunkten.
    pub fn from_corners(a: Point, b: Point) -> Self {
        Self {
            ll: Point::new(a.x.min(b.x), a.y.min(b.y)),
            ur: Point::new(a.x.max(b.x), a.y.max(b.y)),
        }
    }

    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self::from_corners(Point::new(x0, y0), Point::new(x1, y1))
    }

    /// Stellt sicher, dass `ll <= ur` gilt (JSON-Importe sind nicht vertrauenswürdig).
    pub fn normalized(&self) -> Self {
        Self::from_corners(self.ll, self.ur)
    }

    pub fn width(&self) -> f64 {
        self.ur.x - self.ll.x
    }

    pub fn height(&self) -> f64 {
        self.ur.y - self.ll.y
    }

    pub fn is_empty(&self) -> bool {
        self.width() <= 0.0 || self.height() <= 0.0
    }

    pub fn center(&self) -> Point {
        Point::new(
            (self.ll.x + self.ur.x) / 2.0,
            (self.ll.y + self.ur.y) / 2.0,
        )
    }

    /// Kleinstes Rechteck, das beide Rechtecke enthält.
    pub fn union(&self, other: &Rect) -> Rect {
        Rect {
            ll: Point::new(self.ll.x.min(other.ll.x), self.ll.y.min(other.ll.y)),
            ur: Point::new(self.ur.x.max(other.ur.x), self.ur.y.max(other.ur.y)),
        }
    }

    /// Vergrößert das Rechteck in alle Richtungen um `pad`.
    pub fn expanded(&self, pad: f64) -> Rect {
        Rect {
            ll: Point::new(self.ll.x - pad, self.ll.y - pad),
            ur: Point::new(self.ur.x + pad, self.ur.y + pad),
        }
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.ll.x && p.x <= self.ur.x && p.y >= self.ll.y && p.y <= self.ur.y
    }

    /// Überlappen sich die beiden Rechtecke (Berührung zählt nicht)?
    pub fn intersects(&self, other: &Rect) -> bool {
        self.ll.x < other.ur.x
            && other.ll.x < self.ur.x
            && self.ll.y < other.ur.y
            && other.ll.y < self.ur.y
    }

    pub fn area(&self) -> f64 {
        (self.width().max(0.0)) * (self.height().max(0.0))
    }

    /// Fläche der Schnittmenge.
    pub fn intersection_area(&self, other: &Rect) -> f64 {
        let w = (self.ur.x.min(other.ur.x) - self.ll.x.max(other.ll.x)).max(0.0);
        let h = (self.ur.y.min(other.ur.y) - self.ll.y.max(other.ll.y)).max(0.0);
        w * h
    }

    /// Anteil von `self`, der von `other` überdeckt wird (0.0 … 1.0).
    pub fn covered_fraction(&self, other: &Rect) -> f64 {
        let a = self.area();
        if a <= f64::EPSILON {
            // Entartete Rechtecke (z.B. Leerzeichen ohne Höhe): Mittelpunkt prüfen.
            return if other.contains(self.center()) { 1.0 } else { 0.0 };
        }
        self.intersection_area(other) / a
    }
}

/// Ein einzelnes gesetztes Zeichen mit seiner Bounding-Box im User-Space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Glyph {
    pub ch: char,
    pub rect: Rect,
}

/// Ein zusammenhängender Text-Abschnitt einer Seite (Zeile oder Text-Run).
///
/// `glyphs` ist zeichenweise deckungsgleich mit `text.chars()`, dadurch lässt
/// sich für jeden Treffer eines Regex die exakte Bounding-Box berechnen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextRun {
    pub page: usize,
    pub text: String,
    pub glyphs: Vec<Glyph>,
    pub rect: Rect,
}

impl TextRun {
    pub fn new(page: usize, glyphs: Vec<Glyph>) -> Self {
        let text: String = glyphs.iter().map(|g| g.ch).collect();
        let rect = bounding_box(glyphs.iter().map(|g| &g.rect))
            .unwrap_or_else(|| Rect::new(0.0, 0.0, 0.0, 0.0));
        Self {
            page,
            text,
            glyphs,
            rect,
        }
    }

    /// Bounding-Box für einen Byte-Bereich von [`TextRun::text`].
    ///
    /// Gibt `None` zurück, wenn der Bereich leer ist oder außerhalb liegt.
    pub fn rect_for_byte_range(&self, start: usize, end: usize) -> Option<Rect> {
        if start >= end {
            return None;
        }
        let mut byte = 0usize;
        let mut acc: Option<Rect> = None;
        for glyph in &self.glyphs {
            let len = glyph.ch.len_utf8();
            if byte >= start && byte < end {
                acc = Some(match acc {
                    Some(r) => r.union(&glyph.rect),
                    None => glyph.rect,
                });
            }
            byte += len;
            if byte >= end {
                break;
            }
        }
        acc
    }
}

/// Bounding-Box über eine Menge von Rechtecken.
pub fn bounding_box<'a, I: IntoIterator<Item = &'a Rect>>(rects: I) -> Option<Rect> {
    let mut it = rects.into_iter();
    let first = *it.next()?;
    Some(it.fold(first, |acc, r| acc.union(r)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_corners() {
        let r = Rect::from_corners(Point::new(10.0, 20.0), Point::new(0.0, 5.0));
        assert_eq!(r.ll, Point::new(0.0, 5.0));
        assert_eq!(r.ur, Point::new(10.0, 20.0));
    }

    #[test]
    fn intersection_and_coverage() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 5.0, 15.0, 15.0);
        assert!(a.intersects(&b));
        assert_eq!(a.intersection_area(&b), 25.0);
        assert_eq!(a.covered_fraction(&b), 0.25);
        let touching = Rect::new(10.0, 0.0, 20.0, 10.0);
        assert!(!a.intersects(&touching));
    }

    #[test]
    fn byte_range_bbox_covers_only_selected_glyphs() {
        let glyphs = vec![
            Glyph {
                ch: 'A',
                rect: Rect::new(0.0, 0.0, 5.0, 10.0),
            },
            Glyph {
                ch: 'B',
                rect: Rect::new(5.0, 0.0, 10.0, 10.0),
            },
            Glyph {
                ch: 'C',
                rect: Rect::new(10.0, 0.0, 15.0, 10.0),
            },
        ];
        let run = TextRun::new(0, glyphs);
        assert_eq!(run.text, "ABC");
        let r = run.rect_for_byte_range(1, 3).unwrap();
        assert_eq!(r, Rect::new(5.0, 0.0, 15.0, 10.0));
        assert!(run.rect_for_byte_range(2, 2).is_none());
    }

    #[test]
    fn byte_range_handles_multibyte_chars() {
        let glyphs = vec![
            Glyph {
                ch: 'ä',
                rect: Rect::new(0.0, 0.0, 5.0, 10.0),
            },
            Glyph {
                ch: 'x',
                rect: Rect::new(5.0, 0.0, 10.0, 10.0),
            },
        ];
        let run = TextRun::new(0, glyphs);
        // 'ä' belegt zwei Bytes, 'x' beginnt daher bei Byte 2.
        assert_eq!(run.rect_for_byte_range(2, 3).unwrap(), Rect::new(5.0, 0.0, 10.0, 10.0));
    }
}
