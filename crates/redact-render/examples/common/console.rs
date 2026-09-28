//! Ein Konsolenbild, gesetzt mit dem Font-Rasterizer dieses Programms.
//!
//! [`redact_render::fonts`] liefert Glyph-Umrisse, `tiny-skia` füllt sie —
//! dieselben zwei Bausteine, mit denen auch die PDF-Vorschau entsteht. Es ist
//! also kein Bildschirmfoto einer fremden Konsole, sondern derselbe Satzweg
//! wie bei den Seitenbildern.
//!
//! # Das feste Zeichenraster
//!
//! Zeichenbreite und Zeilenhöhe werden auf ganze Pixel gerundet, und jedes
//! Zeichen sitzt auf einem Rasterpunkt. Zwei Gründe:
//!
//! * Eine Glyphe muss dann nur **einmal** gerastert werden und lässt sich an
//!   jeder Stelle unverändert einsetzen — das ist schnell und liefert bei
//!   jedem Lauf dasselbe Bild.
//! * Für die Animation zählt, dass sich zwischen zwei Einzelbildern so wenig
//!   wie möglich ändert. Auf einem festen Raster ist die Änderung genau das
//!   neu getippte Zeichen.
//!
//! # Fehlende Zeichen sind ein Abbruchgrund
//!
//! Wenn ein Zeichen der mitgeschnittenen Ausgabe im Font fehlt, zeigt das Bild
//! **anderen Text als der Lauf gedruckt hat**. Eine Animation, die das
//! stillschweigend überspringt, ist wertlos. [`Console::write`] bricht
//! deshalb ab und nennt das Zeichen.

use std::collections::HashMap;

use redact_render::fonts::{GlyphFont, GlyphKey, Seg};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};

use super::{Canvas, Fehler};

/// Ein Konsolenbild, das nur wachsen kann.
///
/// Geschrieben wird immer ans Ende; es gibt kein Löschen, kein Scrollen und
/// keine Cursor-Bewegung. Mehr braucht ein Mitschnitt nicht, und alles
/// darüber hinaus wäre Bühnenbild.
pub struct Console {
    canvas: Canvas,
    font: GlyphFont,
    size: f64,
    cell: u32,
    line_height: u32,
    margin_x: u32,
    margin_y: u32,
    baseline: u32,
    cols: usize,
    rows: usize,
    col: usize,
    row: usize,
    pub background: [u8; 4],
    foreground: [u8; 4],
    glyphs: HashMap<char, Glyph>,
}

/// Eine einmal gerasterte Glyphe: Deckungsgrade plus Versatz zum Setzpunkt.
struct Glyph {
    width: usize,
    height: usize,
    offset_x: i64,
    offset_y: i64,
    coverage: Vec<u8>,
}

impl Console {
    /// `cols`/`rows` in Zeichen, `size` in Pixeln je Geviert.
    pub fn new(
        cols: usize,
        rows: usize,
        size: f64,
        background: [u8; 4],
        foreground: [u8; 4],
    ) -> Result<Self, Fehler> {
        let font = GlyphFont::fallback(false, false, false, true);
        let upem = font.units_per_em();
        if upem <= 0.0 {
            return Err("der mitgelieferte Monospace-Font meldet keine Geviertgröße".into());
        }
        // Alle Zeichen einer Monospace-Schrift haben denselben Vorschub; „M“
        // ist nur der Vertreter, an dem er abgelesen wird.
        let advance = font
            .advance(GlyphKey::Char('M'))
            .ok_or("der mitgelieferte Monospace-Font kennt kein „M“")?;
        let cell = ((advance * size / upem).round() as u32).max(1);
        let line_height = ((size * 1.36).round() as u32).max(1);
        let (margin_x, margin_y) = (cell.saturating_mul(2), line_height);
        let baseline = (size * 0.80).round() as u32;

        let width = margin_x * 2 + cell * cols as u32;
        let height = margin_y * 2 + line_height * rows as u32;

        Ok(Self {
            canvas: Canvas::new(width, height, background),
            font,
            size,
            cell,
            line_height,
            margin_x,
            margin_y,
            baseline,
            cols,
            rows,
            col: 0,
            row: 0,
            background,
            foreground,
            glyphs: HashMap::new(),
        })
    }

    pub fn width(&self) -> u32 {
        self.canvas.width
    }

    pub fn height(&self) -> u32 {
        self.canvas.height
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    /// Der aktuelle Bildinhalt.
    pub fn snapshot(&self) -> Canvas {
        self.canvas.clone()
    }

    /// Beginnt eine neue Zeile.
    ///
    /// Darf über die letzte Zeile hinauslaufen — der Zeilenumbruch **hinter**
    /// der letzten Zeile braucht keinen Platz mehr. Erst der Versuch, dort
    /// noch etwas zu schreiben, ist ein Fehler (siehe [`Console::put`]).
    pub fn newline(&mut self) -> Result<(), Fehler> {
        self.col = 0;
        self.row += 1;
        Ok(())
    }

    /// Schreibt einen Text; `\n` beginnt eine Zeile, zu lange Zeilen brechen um.
    ///
    /// Der Umbruch ist derselbe, den eine Konsole macht: harte Trennung am
    /// Rand, kein Wort wird verschoben und keines verschluckt.
    pub fn write(&mut self, text: &str) -> Result<(), Fehler> {
        for ch in text.chars() {
            if ch == '\n' {
                self.newline()?;
                continue;
            }
            if ch == '\r' {
                continue;
            }
            if self.col >= self.cols {
                self.newline()?;
            }
            self.put(ch)?;
        }
        Ok(())
    }

    fn put(&mut self, ch: char) -> Result<(), Fehler> {
        if self.row >= self.rows {
            return Err(format!(
                "die Konsole ist voll ({} Zeilen), es soll aber noch {ch:?} geschrieben \
                 werden. Entweder braucht sie mehr Zeilen oder der Mitschnitt ist länger \
                 geworden — gekürzt wird er nicht.",
                self.rows
            )
            .into());
        }
        if !self.glyphs.contains_key(&ch) {
            let glyph = self.rasterize(ch)?;
            self.glyphs.insert(ch, glyph);
        }
        let pen_x = i64::from(self.margin_x + self.cell * self.col as u32);
        let pen_y = i64::from(self.margin_y + self.line_height * self.row as u32 + self.baseline);
        // `glyphs` wird hier nur gelesen; die Ausleihe endet vor dem Blit.
        let glyph = self.glyphs.get(&ch).expect("gerade eingefügt");
        let (gw, gh) = (glyph.width, glyph.height);
        let (ox, oy) = (glyph.offset_x, glyph.offset_y);
        let coverage = glyph.coverage.clone();

        for y in 0..gh {
            for x in 0..gw {
                let alpha = coverage[y * gw + x];
                if alpha == 0 {
                    continue;
                }
                let tx = pen_x + ox + x as i64;
                let ty = pen_y + oy + y as i64;
                if tx < 0 || ty < 0 {
                    continue;
                }
                let (tx, ty) = (tx as u32, ty as u32);
                if tx >= self.canvas.width || ty >= self.canvas.height {
                    continue;
                }
                let under = self.canvas.pixel(tx, ty);
                let mut over = [0u8; 4];
                for c in 0..3 {
                    let a = u32::from(alpha);
                    let value = u32::from(under[c]) * (255 - a) + u32::from(self.foreground[c]) * a;
                    over[c] = ((value + 127) / 255) as u8;
                }
                over[3] = 255;
                self.canvas.set(tx, ty, over);
            }
        }

        self.col += 1;
        Ok(())
    }

    /// Rastert eine Glyphe einmalig in eine Deckungskarte.
    fn rasterize(&self, ch: char) -> Result<Glyph, Fehler> {
        let empty = Glyph {
            width: 0,
            height: 0,
            offset_x: 0,
            offset_y: 0,
            coverage: Vec::new(),
        };
        if ch == ' ' {
            return Ok(empty);
        }
        let outline = self.font.outline(GlyphKey::Char(ch)).ok_or_else(|| {
            format!(
                "der mitgelieferte Monospace-Font hat keine Glyphe für {ch:?} (U+{:04X}). \
                 Das Zeichen steht in der mitgeschnittenen Ausgabe; es wegzulassen hieße, \
                 im Bild anderen Text zu zeigen als der Lauf gedruckt hat.",
                ch as u32
            )
        })?;
        let Some((x_min, y_min, x_max, y_max)) = outline.bounds() else {
            return Ok(empty);
        };
        let scale = self.size / outline.units_per_em;

        // Gerätekoordinaten relativ zum Setzpunkt, y zeigt nach unten.
        let left = (x_min * scale).floor() as i64 - 1;
        let top = (-y_max * scale).floor() as i64 - 1;
        let right = (x_max * scale).ceil() as i64 + 1;
        let bottom = (-y_min * scale).ceil() as i64 + 1;
        let width = (right - left).max(1) as usize;
        let height = (bottom - top).max(1) as usize;

        let Some(mut pixmap) = Pixmap::new(width as u32, height as u32) else {
            return Ok(empty);
        };
        let mut builder =
            PathBuilder::with_capacity(outline.segments.len() + 1, outline.segments.len() * 2 + 2);
        let to_x = |x: f64| (x * scale - left as f64) as f32;
        let to_y = |y: f64| (-y * scale - top as f64) as f32;
        let mut open = false;
        for segment in &outline.segments {
            match *segment {
                Seg::MoveTo(x, y) => {
                    builder.move_to(to_x(x), to_y(y));
                    open = true;
                }
                Seg::LineTo(x, y) => {
                    if open {
                        builder.line_to(to_x(x), to_y(y));
                    } else {
                        builder.move_to(to_x(x), to_y(y));
                        open = true;
                    }
                }
                Seg::QuadTo(cx, cy, x, y) => {
                    if !open {
                        builder.move_to(to_x(cx), to_y(cy));
                        open = true;
                    }
                    builder.quad_to(to_x(cx), to_y(cy), to_x(x), to_y(y));
                }
                Seg::CubicTo(cx0, cy0, cx1, cy1, x, y) => {
                    if !open {
                        builder.move_to(to_x(cx0), to_y(cy0));
                        open = true;
                    }
                    builder.cubic_to(to_x(cx0), to_y(cy0), to_x(cx1), to_y(cy1), to_x(x), to_y(y));
                }
                Seg::Close => builder.close(),
            }
        }
        let Some(path) = builder.finish() else {
            return Ok(empty);
        };

        let mut paint = Paint::default();
        paint.set_color_rgba8(255, 255, 255, 255);
        paint.anti_alias = true;
        pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );

        let coverage = pixmap.pixels().iter().map(|p| p.alpha()).collect();
        Ok(Glyph {
            width,
            height,
            offset_x: left,
            offset_y: top,
            coverage,
        })
    }
}
