//! Rasterisierung einer PDF-Seite in ein RGBA8-Bild.
//!
//! Eingabe ist [`redact_pdf::ops::PageOps`] — ein bereits vollständig
//! aufgelöster Zeichenstrom im User-Space. Dieses Modul kümmert sich nur noch
//! um zwei Dinge:
//!
//! 1. die Abbildung User-Space → Pixel (MediaBox, `/Rotate`, Y-Spiegelung), und
//! 2. das eigentliche Malen mit `tiny-skia`.
//!
//! # Die Vorschau darf niemals ausfallen
//!
//! [`PageRenderer::render`] gibt **kein** `Result` zurück und panickt nicht.
//! Jede Stufe ist abgesichert:
//!
//! * `page_ops` läuft in [`std::panic::catch_unwind`]; schlägt es fehl (Fehler
//!   *oder* Panik), entsteht ein weißes Blatt mit `degraded = true`.
//! * Die Zeichenschleife läuft **pro Operation** in `catch_unwind`. Ein kaputter
//!   Pfad, ein unlesbarer Font oder ein defektes Bild kostet genau diese eine
//!   Operation — der Rest der Seite wird trotzdem gezeichnet.
//! * Entartete MediaBoxen (null, negativ, NaN, absurd groß) werden auf A4
//!   zurückgesetzt, es gibt nie ein Bild der Größe 0.
//!
//! # Was originalgetreu, genähert oder übergangen wird
//!
//! | Inhalt                       | Umsetzung                                   |
//! |------------------------------|---------------------------------------------|
//! | Pfade füllen/stricheln       | originalgetreu (Nonzero/Even-Odd, Cap/Join, Dash) |
//! | Strichbreite 0               | Haarlinie (dünnste darstellbare Linie)      |
//! | Glyphen                      | echte Umrisse, sonst mitgelieferter Ersatzfont |
//! | Bilder                       | bilinear, mit korrekter Bild-Y-Spiegelung   |
//! | Clip                         | genähert: nur der zuletzt gesetzte Pfad (siehe [`redact_pdf::ops`]) |
//! | Textmodus 1/5 (nur Kontur)   | genähert: flächig gefüllt                   |
//! | Textmodus 3/7 (unsichtbar)   | übersprungen (korrekt)                      |
//! | Schattierungen/Muster        | von `page_ops` bereits durch Grau ersetzt   |
//! | Weiche Masken (`/SMask`)     | nicht ausgewertet                           |

use std::collections::{HashMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};

use lopdf::Document;
use redact_core::{Point, Rect};
use redact_pdf::matrix::Matrix;
use redact_pdf::ops::{
    page_ops, page_rotation, ClipRef, CodeToGid, DrawOp, FontProgram, PageOps, PathSeg,
    RasterImage, Rgb, Stroke as PdfStroke,
};
use tiny_skia::{
    Color, FillRule, FilterQuality, LineCap, LineJoin, Mask, Paint, Path, PathBuilder, Pixmap,
    PixmapPaint, Stroke as SkStroke, StrokeDash, Transform,
};

use crate::fonts::{FontCache, GlyphFont, GlyphKey, Outline, Seg};

// ---------------------------------------------------------------------------
// Grenzwerte
// ---------------------------------------------------------------------------

/// Notnagel-Seitengröße, wenn die MediaBox unbrauchbar ist.
///
/// Steht seit dieser Runde in [`redact_pdf::document`] — zusammen mit der
/// Regel, die entscheidet, wann sie einspringt. Zwei Fassungen derselben Regel
/// sind hier schon auseinandergelaufen; siehe [`redact_pdf::document::SaneBox`].
const A4: Rect = redact_pdf::document::A4;

/// Harte Obergrenze für eine Bildkante, egal was die Optionen sagen.
const MAX_EDGE_LIMIT: u32 = 20_000;
/// Unterhalb dieser Gerätebreite wird ein Strich zur Haarlinie.
const HAIRLINE_BELOW: f64 = 0.85;

// ---------------------------------------------------------------------------
// Optionen und Ergebnis
// ---------------------------------------------------------------------------

/// Wie eine Seite gerendert werden soll.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderOptions {
    /// Zielbreite in Pixeln; die Höhe folgt aus dem Seitenverhältnis.
    pub width: u32,
    /// Obergrenze für Breite **und** Höhe in Pixeln, damit riesige Seiten nicht
    /// den Speicher sprengen.
    pub max_pixels: u32,
    /// Hintergrund (normalerweise weiß), als RGBA8 ohne Vormultiplikation.
    pub background: [u8; 4],
    /// Text zeichnen (zum Testen abschaltbar).
    pub draw_text: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            width: 1000,
            max_pixels: 4000,
            background: [255, 255, 255, 255],
            draw_text: true,
        }
    }
}

impl RenderOptions {
    /// Auf sinnvolle Werte gestutzte Kopie.
    fn sanitized(&self) -> Self {
        let max_edge = self.max_pixels.clamp(16, MAX_EDGE_LIMIT);
        Self {
            width: self.width.clamp(1, max_edge),
            max_pixels: max_edge,
            background: self.background,
            draw_text: self.draw_text,
        }
    }
}

/// Ergebnis eines Rendervorgangs — enthält IMMER ein Bild.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedPage {
    pub width: u32,
    pub height: u32,
    /// RGBA8, `width * height * 4` Bytes, ohne Vormultiplikation.
    pub rgba: Vec<u8>,
    /// Auf welche Fläche im User-Space sich das Bild bezieht (nach `/Rotate`).
    ///
    /// Die linke untere Ecke ist die der MediaBox; bei `/Rotate 90` und `270`
    /// sind Breite und Höhe getauscht, damit das Seitenverhältnis zum Bild
    /// passt.
    pub page_box: Rect,
    /// Seitendrehung, die bereits angewendet wurde (0/90/180/270).
    pub rotate: i64,
    /// Was nicht oder nur näherungsweise dargestellt werden konnte.
    pub warnings: Vec<String>,
    /// Wie viele Zeichenoperationen tatsächlich gezeichnet wurden.
    pub drawn_ops: usize,
    /// `true`, wenn nur der Notnagel-Pfad lief — das Bild zeigt dann bloß den
    /// Hintergrund.
    pub degraded: bool,
}

impl RenderedPage {
    /// Farbe eines Pixels; `None` außerhalb des Bildes.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let at = ((y as usize) * (self.width as usize) + x as usize) * 4;
        self.rgba.get(at..at + 4).map(|p| [p[0], p[1], p[2], p[3]])
    }

    /// Anteil der Pixel, die nicht reinweiß sind (0.0 … 1.0).
    ///
    /// Praktisch als grobe „ist überhaupt etwas zu sehen?“-Prüfung.
    pub fn non_white_ratio(&self) -> f64 {
        let total = self.rgba.len() / 4;
        if total == 0 {
            return 0.0;
        }
        let hits = self
            .rgba
            .chunks_exact(4)
            .filter(|p| p[0] != 255 || p[1] != 255 || p[2] != 255)
            .count();
        hits as f64 / total as f64
    }
}

// ---------------------------------------------------------------------------
// PageRenderer
// ---------------------------------------------------------------------------

/// Rasterisiert Seiten und hält dabei die geladenen Fonts vor.
#[derive(Default)]
pub struct PageRenderer {
    fonts: FontCache,
    /// Merkt sich je Font-Schlüssel, ob das eingebettete Programm wirklich
    /// lesbar war (sonst steckt hinter dem Schlüssel der Ersatzfont, und
    /// Glyph-IDs aus dem PDF wären dort bedeutungslos).
    embedded: HashMap<String, bool>,
}

impl std::fmt::Debug for PageRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageRenderer")
            .field("fonts", &self.fonts.len())
            .finish()
    }
}

impl PageRenderer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Anzahl der bislang geladenen Fontprogramme (Diagnose).
    pub fn font_count(&self) -> usize {
        self.fonts.len()
    }

    /// Rendert eine Seite (0-basiert).
    ///
    /// Gibt **niemals** `Err` zurück und panickt nie: im schlimmsten Fall
    /// entsteht ein leeres Blatt in der richtigen Größe mit `degraded = true`
    /// und einer Erklärung in `warnings`.
    pub fn render(
        &mut self,
        doc: &Document,
        page_index: usize,
        opts: &RenderOptions,
    ) -> RenderedPage {
        let opts = opts.sanitized();
        let mut warnings: Vec<String> = Vec::new();
        let mut degraded = false;

        // --- 1. Zeichenoperationen holen ------------------------------------
        let collected = catch_unwind(AssertUnwindSafe(|| page_ops(doc, page_index)));
        let ops = match collected {
            Ok(Ok(ops)) => Some(ops),
            Ok(Err(err)) => {
                push_once(&mut warnings, format!("Seite nicht lesbar: {err}"));
                degraded = true;
                None
            }
            Err(_) => {
                push_once(
                    &mut warnings,
                    "Seite nicht lesbar: Panik beim Auswerten abgefangen".to_string(),
                );
                degraded = true;
                None
            }
        };

        // --- 2. Geometrie ---------------------------------------------------
        let (raw_box, rotate) = match &ops {
            Some(ops) => (ops.media_box, ops.rotate),
            None => fallback_geometry(doc, page_index),
        };
        let media_box = sane_box(raw_box, &mut warnings);
        let geo = Geometry::new(media_box, rotate, &opts);

        // --- 3. Leinwand ----------------------------------------------------
        let Some(mut pixmap) = Pixmap::new(geo.width, geo.height) else {
            // Kann nur bei absurden Größen passieren; dann eben ein Minimalbild.
            push_once(
                &mut warnings,
                format!("Bildpuffer {}x{} nicht anlegbar", geo.width, geo.height),
            );
            return blank_page(&opts, media_box, rotate, warnings);
        };
        pixmap.fill(background_color(opts.background));

        // --- 4. Malen -------------------------------------------------------
        let mut drawn_ops = 0usize;
        if let Some(ops) = &ops {
            for note in &ops.notes {
                push_once(&mut warnings, note.clone());
            }
            let slots = self.prepare_fonts(ops);
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                let mut painter = Painter {
                    pixmap: &mut pixmap,
                    geo: &geo,
                    page: ops,
                    fonts: &mut self.fonts,
                    slots: &slots,
                    clips: vec![None; ops.clips.len()],
                    images: vec![None; ops.images.len()],
                    warned_fonts: HashSet::new(),
                    warnings: Vec::new(),
                    drawn: 0,
                    draw_text: opts.draw_text,
                };
                painter.run();
                (painter.drawn, painter.warnings)
            }));
            match outcome {
                Ok((drawn, notes)) => {
                    drawn_ops = drawn;
                    for note in notes {
                        push_once(&mut warnings, note);
                    }
                }
                Err(_) => {
                    // Die Zeichenschleife sichert jede Operation einzeln ab;
                    // hier landet nur, was darüber hinaus schiefgeht.
                    push_once(
                        &mut warnings,
                        "Zeichnen abgebrochen: Panik abgefangen".to_string(),
                    );
                    degraded = true;
                }
            }
        }

        RenderedPage {
            width: pixmap.width(),
            height: pixmap.height(),
            rgba: pixmap.take_demultiplied(),
            page_box: geo.page_box,
            rotate: geo.rotate,
            warnings,
            drawn_ops,
            degraded,
        }
    }

    /// Lädt alle Fonts der Seite einmalig und liefert je Font den Cache-Schlüssel.
    fn prepare_fonts(&mut self, ops: &PageOps) -> Vec<FontSlot> {
        ops.fonts
            .iter()
            .map(|program| {
                let key = font_key(program);
                let embedded = match self.embedded.get(&key) {
                    Some(known) => *known,
                    None => {
                        let usable = program
                            .data
                            .as_deref()
                            .map(|data| GlyphFont::from_bytes(data).is_some())
                            .unwrap_or(false);
                        self.embedded.insert(key.clone(), usable);
                        usable
                    }
                };
                let style = FontStyle::guess(program);
                self.fonts.get_or_load(
                    &key,
                    program.data.as_deref(),
                    style.serif,
                    style.bold,
                    style.italic,
                    style.mono,
                );
                // Zusätzlich immer den passenden Schnitt des mitgelieferten
                // Ersatzfonts bereithalten — siehe [`FontSlot::substitute`].
                let substitute = style.substitute_key();
                self.fonts.get_or_load(
                    &substitute,
                    None,
                    style.serif,
                    style.bold,
                    style.italic,
                    style.mono,
                );
                FontSlot {
                    key,
                    substitute,
                    embedded,
                }
            })
            .collect()
    }
}

/// Ein geladener Font der aktuellen Seite.
#[derive(Debug, Clone)]
struct FontSlot {
    /// Schlüssel im [`FontCache`].
    key: String,
    /// Schlüssel des mitgelieferten Ersatzfonts im selben Schnitt.
    ///
    /// Auch eingebettete Fonts brauchen ihn: Subsets tragen oft nur eine
    /// `cmap` der Form (1,0)/Format 6 oder gar keine, und ein roher CFF hat
    /// nie eine. `skrifa` kennt nur Unicode-`cmap`s (Format 4/12), sodass eine
    /// Suche über das Zeichen ins Leere läuft. Dann ist der Ersatzfont die
    /// bessere Wahl als die Glyph-ID zu raten.
    substitute: String,
    /// `true`, wenn das eingebettete Programm gelesen werden konnte.
    embedded: bool,
}

// ---------------------------------------------------------------------------
// Geometrie
// ---------------------------------------------------------------------------

/// Abbildung User-Space → Pixel, inklusive `/Rotate` und Y-Spiegelung.
#[derive(Debug, Clone)]
struct Geometry {
    width: u32,
    height: u32,
    /// Punkt im User-Space → Punkt im Bild.
    to_device: Matrix,
    /// Gleichmäßiger Maßstab (Pixel je Punkt) — für Strichbreiten.
    scale: f64,
    page_box: Rect,
    rotate: i64,
}

impl Geometry {
    fn new(media_box: Rect, rotate: i64, opts: &RenderOptions) -> Self {
        let rotate = normalize_rotation(rotate);
        let (x0, y0) = (media_box.ll.x, media_box.ll.y);
        let (x1, y1) = (media_box.ur.x, media_box.ur.y);
        let (w, h) = (media_box.width(), media_box.height());
        let swapped = rotate == 90 || rotate == 270;
        let (disp_w, disp_h) = if swapped { (h, w) } else { (w, h) };

        let max_edge = opts.max_pixels.max(1);
        let mut px_w = f64::from(opts.width.clamp(1, max_edge));
        let mut px_h = px_w * disp_h / disp_w;
        if px_h > f64::from(max_edge) {
            px_h = f64::from(max_edge);
            px_w = px_h * disp_w / disp_h;
        }
        let width = (px_w.round() as i64).clamp(1, i64::from(max_edge)) as u32;
        let height = (px_h.round() as i64).clamp(1, i64::from(max_edge)) as u32;
        let scale = (f64::from(width) / disp_w).min(f64::from(height) / disp_h);

        // Y-Spiegelung steckt in jedem der vier Fälle mit drin: PDF zählt y
        // nach oben, ein Bild nach unten.
        let s = scale;
        let to_device = match rotate {
            90 => Matrix::new(0.0, s, s, 0.0, -y0 * s, -x0 * s),
            180 => Matrix::new(-s, 0.0, 0.0, s, x1 * s, -y0 * s),
            270 => Matrix::new(0.0, -s, -s, 0.0, y1 * s, x1 * s),
            _ => Matrix::new(s, 0.0, 0.0, -s, -x0 * s, y1 * s),
        };
        let page_box = if swapped {
            Rect::new(x0, y0, x0 + h, y0 + w)
        } else {
            media_box
        };

        Self {
            width,
            height,
            to_device,
            scale,
            page_box,
            rotate,
        }
    }
}

fn normalize_rotation(rotate: i64) -> i64 {
    let normalized = rotate.rem_euclid(360);
    if normalized % 90 == 0 {
        normalized
    } else {
        0
    }
}

/// MediaBox auf etwas Zeichenbares stutzen.
///
/// Die Entscheidung selbst fällt in [`redact_pdf::document::sane_box`]; hier
/// wird nur noch der Satz dazu in die Warnungen dieser Seite gelegt. Vorher
/// stand die Regel an dieser Stelle allein — die Oberfläche kam über
/// `redact_pdf::page_boxes` an die **ungeprüfte** Angabe und hielt eine Seite
/// mit `/MediaBox [0 0 0 0]` für 0 × 0 Punkt groß, während der Rasterizer sie
/// gleich daneben auf A4 zeichnete.
fn sane_box(raw: Rect, warnings: &mut Vec<String>) -> Rect {
    let checked = redact_pdf::document::sane_box(raw);
    if let Some(note) = checked.warning() {
        push_once(warnings, note);
    }
    checked.rect
}

/// MediaBox und `/Rotate` direkt aus dem Dokument, wenn `page_ops` versagt hat.
fn fallback_geometry(doc: &Document, page_index: usize) -> (Rect, i64) {
    let probe = catch_unwind(AssertUnwindSafe(|| {
        let pages = doc.get_pages();
        let (_, page_id) = pages.iter().nth(page_index)?;
        Some((
            redact_pdf::page_box(doc, *page_id),
            page_rotation(doc, *page_id),
        ))
    }));
    match probe {
        Ok(Some(found)) => found,
        _ => (A4, 0),
    }
}

/// Weißes (bzw. hintergrundfarbenes) Blatt ohne jeden Inhalt.
fn blank_page(
    opts: &RenderOptions,
    media_box: Rect,
    rotate: i64,
    warnings: Vec<String>,
) -> RenderedPage {
    let geo = Geometry::new(media_box, rotate, opts);
    let (width, height) = (geo.width.max(1), geo.height.max(1));
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for _ in 0..(width as usize * height as usize) {
        rgba.extend_from_slice(&opts.background);
    }
    RenderedPage {
        width,
        height,
        rgba,
        page_box: geo.page_box,
        rotate: geo.rotate,
        warnings,
        drawn_ops: 0,
        degraded: true,
    }
}

fn background_color(rgba: [u8; 4]) -> Color {
    Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3])
}

fn push_once(warnings: &mut Vec<String>, text: String) {
    if !warnings.contains(&text) {
        warnings.push(text);
    }
}

// ---------------------------------------------------------------------------
// Fonts
// ---------------------------------------------------------------------------

/// Schnitt-Heuristik für den Ersatzfont.
#[derive(Debug, Clone, Copy)]
struct FontStyle {
    serif: bool,
    bold: bool,
    italic: bool,
    mono: bool,
}

impl FontStyle {
    /// `/FontDescriptor /Flags` schlägt den Namen; fehlen die Flags, entscheidet
    /// der BaseFont-Name (Standard-14-Fonts haben oft gar keinen Deskriptor).
    fn guess(program: &FontProgram) -> Self {
        const FIXED_PITCH: u32 = 1 << 0;
        const SERIF: u32 = 1 << 1;
        const ITALIC: u32 = 1 << 6;
        const FORCE_BOLD: u32 = 1 << 18;

        let name = program.base_font.to_ascii_lowercase();
        let has = |needles: &[&str]| needles.iter().any(|n| name.contains(n));
        Self {
            serif: program.flags & SERIF != 0
                || has(&["times", "serif", "roman", "georgia", "garamond", "book"]),
            bold: program.flags & FORCE_BOLD != 0 || has(&["bold", "black", "heavy", "semibold"]),
            italic: program.flags & ITALIC != 0 || has(&["italic", "oblique"]),
            mono: program.flags & FIXED_PITCH != 0 || has(&["courier", "mono", "consol"]),
        }
    }

    /// Cache-Schlüssel des mitgelieferten Ersatzfonts in diesem Schnitt.
    ///
    /// Es gibt nur zwölf Schnitte, deshalb reicht der Stil als Schlüssel; alle
    /// Fonts eines Dokuments teilen sich dieselben Einträge.
    fn substitute_key(&self) -> String {
        format!(
            "!substitute|{}{}{}{}",
            u8::from(self.serif),
            u8::from(self.bold),
            u8::from(self.italic),
            u8::from(self.mono)
        )
    }
}

/// Eindeutiger Cache-Schlüssel für ein Fontprogramm.
///
/// Der Cache überlebt einzelne Dokumente, deshalb reicht der Index in
/// [`PageOps::fonts`] nicht. Name, Art, Em-Größe, Länge und ein FNV-Hash der
/// Bytes zusammen sind praktisch kollisionsfrei.
fn font_key(program: &FontProgram) -> String {
    let (len, hash) = match program.data.as_deref() {
        Some(data) => (data.len(), fnv1a(data)),
        None => (0, 0),
    };
    format!(
        "{}|{:?}|{}|{}|{:016x}|{}",
        program.base_font, program.kind, program.units_per_em, len, hash, program.is_cid
    )
}

fn fnv1a(data: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in data {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

// ---------------------------------------------------------------------------
// Der Maler
// ---------------------------------------------------------------------------

struct Painter<'a> {
    pixmap: &'a mut Pixmap,
    geo: &'a Geometry,
    page: &'a PageOps,
    fonts: &'a mut FontCache,
    slots: &'a [FontSlot],
    /// Clip-Masken, beim ersten Gebrauch gebaut (`Some(None)` = nicht baubar).
    clips: Vec<Option<Option<Mask>>>,
    /// Bilder als vormultiplizierte Pixmaps, ebenfalls verzögert.
    images: Vec<Option<Option<Pixmap>>>,
    warned_fonts: HashSet<usize>,
    warnings: Vec<String>,
    drawn: usize,
    draw_text: bool,
}

impl Painter<'_> {
    fn run(&mut self) {
        // Die Referenz aus dem Feld herausziehen, damit die Schleife nicht
        // gegen die `&mut self`-Ausleihe im Rumpf steht.
        let page = self.page;
        let mut panics = 0usize;
        for op in &page.ops {
            match catch_unwind(AssertUnwindSafe(|| self.draw_op(op))) {
                Ok(true) => self.drawn += 1,
                Ok(false) => {}
                Err(_) => panics += 1,
            }
        }
        if panics > 0 {
            self.warn(format!(
                "{panics} Zeichenoperation(en) übersprungen: Panik abgefangen"
            ));
        }
    }

    fn warn(&mut self, text: String) {
        push_once(&mut self.warnings, text);
    }

    /// Zeichnet eine Operation; `true`, wenn wirklich etwas gemalt wurde.
    fn draw_op(&mut self, op: &DrawOp) -> bool {
        match op {
            DrawOp::Path {
                segments,
                fill,
                stroke,
                even_odd,
                fill_alpha,
                clip,
            } => self.draw_path(
                segments,
                *fill,
                stroke.as_ref(),
                *even_odd,
                *fill_alpha,
                *clip,
            ),
            DrawOp::Glyph {
                font,
                code,
                transform,
                fill,
                fill_alpha,
                render_mode,
                clip,
            } => {
                if !self.draw_text {
                    return false;
                }
                self.draw_glyph(
                    *font,
                    *code,
                    transform,
                    *fill,
                    *fill_alpha,
                    *render_mode,
                    *clip,
                )
            }
            DrawOp::Image {
                image,
                ctm,
                alpha,
                clip,
            } => self.draw_image(*image, ctm, *alpha, *clip),
        }
    }

    // -- Pfade ---------------------------------------------------------------

    fn draw_path(
        &mut self,
        segments: &[PathSeg],
        fill: Option<Rgb>,
        stroke: Option<&PdfStroke>,
        even_odd: bool,
        fill_alpha: f32,
        clip: Option<ClipRef>,
    ) -> bool {
        if fill.is_none() && stroke.is_none() {
            return false;
        }
        let Some(path) = device_path(segments, &self.geo.to_device) else {
            return false;
        };
        let stroke_style = stroke.map(|s| self.stroke_style(s));
        self.prepare_clip(clip);
        let mask = clip_mask(&self.clips, clip);
        let mut painted = false;

        if let Some(color) = fill {
            let paint = solid_paint(color, fill_alpha);
            let rule = if even_odd {
                FillRule::EvenOdd
            } else {
                FillRule::Winding
            };
            self.pixmap
                .fill_path(&path, &paint, rule, Transform::identity(), mask);
            painted = true;
        }
        if let (Some(stroke), Some(sk)) = (stroke, stroke_style) {
            let paint = solid_paint(stroke.color, stroke.alpha);
            self.pixmap
                .stroke_path(&path, &paint, &sk, Transform::identity(), mask);
            painted = true;
        }
        painted
    }

    /// Strichparameter aus User-Space in Gerätekoordinaten übersetzen.
    fn stroke_style(&self, stroke: &PdfStroke) -> SkStroke {
        let device_width = stroke.width.max(0.0) * self.geo.scale;
        // PDF: Breite 0 bedeutet „dünnste darstellbare Linie“. Alles unterhalb
        // eines knappen Pixels behandeln wir genauso, sonst verschwinden
        // Tabellenlinien in der Vorschau.
        let width = if device_width.is_finite() && device_width >= HAIRLINE_BELOW {
            device_width as f32
        } else {
            0.0
        };
        SkStroke {
            width,
            miter_limit: 4.0,
            line_cap: match stroke.cap {
                1 => LineCap::Round,
                2 => LineCap::Square,
                _ => LineCap::Butt,
            },
            line_join: match stroke.join {
                1 => LineJoin::Round,
                2 => LineJoin::Bevel,
                _ => LineJoin::Miter,
            },
            dash: self.dash(stroke),
        }
    }

    fn dash(&self, stroke: &PdfStroke) -> Option<StrokeDash> {
        if stroke.dash.is_empty() {
            return None;
        }
        let mut lengths: Vec<f32> = stroke
            .dash
            .iter()
            .map(|d| (d * self.geo.scale) as f32)
            .collect();
        if lengths.iter().any(|d| !d.is_finite() || *d < 0.0) {
            return None;
        }
        if lengths.iter().sum::<f32>() <= 0.0 {
            return None;
        }
        // PDF erlaubt ungerade Arrays („3“ heißt 3 an, 3 aus“).
        if lengths.len() % 2 == 1 {
            lengths.extend_from_within(..);
        }
        let phase = (stroke.dash_phase * self.geo.scale) as f32;
        StrokeDash::new(lengths, if phase.is_finite() { phase } else { 0.0 })
    }

    // -- Glyphen -------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn draw_glyph(
        &mut self,
        font: usize,
        code: u32,
        transform: &Matrix,
        fill: Rgb,
        fill_alpha: f32,
        render_mode: u8,
        clip: Option<ClipRef>,
    ) -> bool {
        // 3 = unsichtbar (OCR-Ebene), 7 = nur Clip — beide malen nichts.
        if render_mode == 3 || render_mode == 7 {
            return false;
        }
        let Some(program) = self.page.fonts.get(font) else {
            return false;
        };
        let Some(slot) = self.slots.get(font) else {
            return false;
        };
        if transform.determinant().abs() < 1e-12 {
            return false;
        }
        // Leerraum hat nie einen Umriss. Das muss VOR der Glyphensuche stehen,
        // sonst greift für ihn am Ende die Notlösung „Code = Glyph-ID“ und mitten
        // im Satz steht auf einmal ein fremdes Zeichen.
        let blank = program
            .code_to_unicode
            .get(&code)
            .is_some_and(|text| !text.is_empty() && text.chars().all(char::is_whitespace));
        if blank {
            return false;
        }
        if render_mode == 1 || render_mode == 5 {
            self.warn(format!(
                "Text nur als Kontur ({}): flächig gefüllt gezeichnet",
                program.base_font
            ));
        }

        let own = slot.key.clone();
        let substitute = slot.substitute.clone();
        let candidates = glyph_candidates(program, slot.embedded, code);
        let mut outline: Option<Outline> = None;
        for (face, candidate) in &candidates {
            let key = match face {
                Face::Own => &own,
                Face::Substitute => &substitute,
            };
            if let Some(found) = self.fonts.outline(key, *candidate) {
                outline = Some(found);
                break;
            }
        }
        let Some(outline) = outline else {
            if self.warned_fonts.insert(font) {
                let name = program.base_font.clone();
                self.warn(format!("Font {name}: einzelne Glyphen ohne Umriss"));
            }
            return false;
        };

        // `transform` rechnet in den Em-Einheiten, die das PDF nennt; der Umriss
        // kommt in denen des tatsächlich benutzten Fontprogramms. Bei Ersatz-
        // fonts sind das verschiedene Zahlen.
        let upem = if outline.units_per_em > 0.0 {
            outline.units_per_em
        } else {
            1000.0
        };
        let correction = if program.units_per_em > 0.0 {
            program.units_per_em / upem
        } else {
            1.0
        };
        let total = Matrix::scale(correction, correction)
            .mul(transform)
            .mul(&self.geo.to_device);

        let Some(path) = outline_path(&outline, &total) else {
            return false;
        };
        self.prepare_clip(clip);
        let mask = clip_mask(&self.clips, clip);
        let paint = solid_paint(fill, fill_alpha);
        self.pixmap.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            mask,
        );
        true
    }

    // -- Bilder --------------------------------------------------------------

    fn draw_image(
        &mut self,
        index: usize,
        ctm: &Matrix,
        alpha: f32,
        clip: Option<ClipRef>,
    ) -> bool {
        let Some(source) = self.page.images.get(index) else {
            return false;
        };
        if source.width == 0 || source.height == 0 {
            return false;
        }
        if ctm.determinant().abs() < 1e-12 {
            return false;
        }
        if source.placeholder {
            self.warn("Bild nicht dekodierbar: Ersatzfläche gezeichnet".to_string());
        }

        // Bildraum: Ursprung oben links, y nach unten. Erst auf das
        // Einheitsquadrat, dann per CTM in den User-Space, dann ins Bild.
        let (iw, ih) = (f64::from(source.width), f64::from(source.height));
        let to_unit = Matrix::new(1.0 / iw, 0.0, 0.0, -1.0 / ih, 0.0, 1.0);
        let total = to_unit.mul(ctm).mul(&self.geo.to_device);
        let ts = Transform::from_row(
            total.a as f32,
            total.b as f32,
            total.c as f32,
            total.d as f32,
            total.e as f32,
            total.f as f32,
        );
        if !ts.is_valid() {
            return false;
        }

        self.ensure_image(index);
        self.prepare_clip(clip);
        let paint = PixmapPaint {
            opacity: alpha.clamp(0.0, 1.0),
            blend_mode: tiny_skia::BlendMode::SourceOver,
            quality: FilterQuality::Bilinear,
        };
        let source = match self.images.get(index) {
            Some(Some(Some(pixmap))) => pixmap.as_ref(),
            _ => return false,
        };
        let mask = clip_mask(&self.clips, clip);
        self.pixmap.draw_pixmap(0, 0, source, &paint, ts, mask);
        true
    }

    fn ensure_image(&mut self, index: usize) {
        if matches!(self.images.get(index), None | Some(Some(_))) {
            return;
        }
        let built = self.page.images.get(index).and_then(to_pixmap);
        self.images[index] = Some(built);
    }

    // -- Clips ---------------------------------------------------------------

    /// Baut die Maske, falls es sie noch nicht gibt (getrennt von
    /// [`Painter::clip_mask`], damit die Ausleihen sich nicht in die Quere
    /// kommen).
    fn prepare_clip(&mut self, clip: Option<ClipRef>) {
        if let Some(ClipRef(at)) = clip {
            self.ensure_clip(at);
        }
    }

    fn ensure_clip(&mut self, at: usize) {
        if matches!(self.clips.get(at), None | Some(Some(_))) {
            return;
        }
        let segments = self.page.clips.get(at).map(Vec::as_slice).unwrap_or(&[]);
        let built = build_mask(segments, self.geo);
        if built.is_none() {
            self.warn("Clip nicht baubar: ohne Beschnitt gezeichnet".to_string());
        }
        self.clips[at] = Some(built);
    }
}

/// Maske zu einer Clip-Referenz; `None` heißt „ohne Beschnitt zeichnen“.
///
/// Freie Funktion, damit nur `clips` ausgeliehen wird und die Pixmap daneben
/// veränderbar bleibt. Setzt [`Painter::prepare_clip`] voraus.
fn clip_mask(clips: &[Option<Option<Mask>>], clip: Option<ClipRef>) -> Option<&Mask> {
    let ClipRef(at) = clip?;
    clips
        .get(at)
        .and_then(Option::as_ref)
        .and_then(Option::as_ref)
}

/// Baut die Clip-Maske; `None`, wenn der Pfad unbrauchbar ist — dann wird
/// bewusst *ohne* Beschnitt gezeichnet statt gar nicht.
fn build_mask(segments: &[PathSeg], geo: &Geometry) -> Option<Mask> {
    let path = device_path(segments, &geo.to_device)?;
    let mut mask = Mask::new(geo.width, geo.height)?;
    mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
    Some(mask)
}

/// RGBA8 (nicht vormultipliziert) → `tiny_skia::Pixmap`.
fn to_pixmap(image: &RasterImage) -> Option<Pixmap> {
    let expected = (image.width as usize).checked_mul(image.height as usize)?;
    if image.rgba.len() < expected.checked_mul(4)? {
        return None;
    }
    let mut pixmap = Pixmap::new(image.width, image.height)?;
    for (target, source) in pixmap
        .pixels_mut()
        .iter_mut()
        .zip(image.rgba.chunks_exact(4))
    {
        *target =
            tiny_skia::ColorU8::from_rgba(source[0], source[1], source[2], source[3]).premultiply();
    }
    Some(pixmap)
}

fn solid_paint<'a>(color: Rgb, alpha: f32) -> Paint<'a> {
    let [r, g, b] = color.to_u8();
    let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(r, g, b, a));
    paint.anti_alias = true;
    paint
}

/// Aus welchem Fontprogramm ein Versuch bedient wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Face {
    /// Das Programm der Seite (eingebettet, oder bei fehlender Einbettung
    /// bereits der Ersatzfont).
    Own,
    /// Der mitgelieferte Ersatzfont.
    Substitute,
}

/// Welche Glyphen-Schlüssel für einen Code in Frage kommen, in dieser Reihenfolge.
///
/// Die Reihenfolge ist der eigentliche Inhalt dieser Funktion:
///
/// 1. das eigene Programm über das Zeichen (die (3,1)-`cmap`, der Normalfall),
/// 2. das eigene Programm über den privaten Bereich 0xF000+ (symbolische
///    (3,0)-`cmap`s),
/// 3. **der Ersatzfont über das Zeichen**, und erst danach
/// 4. das eigene Programm über „Code = Glyph-ID“.
///
/// Schritt 4 hinter Schritt 3 zu stellen ist wichtig: „Code = Glyph-ID“ ist
/// nirgends in PDF 32000-1 vorgesehen, sondern reine Verzweiflung. Bei Subsets
/// von z. B. ReportLab (nur eine (1,0)-`cmap`, die `skrifa` nicht liest) trifft
/// es zuverlässig die *falsche* Glyphe — aus „Professional“ wird Buchstabensalat.
/// Der richtige Buchstabe im falschen Schnitt ist für eine Schwärzungsvorschau
/// deutlich mehr wert als die falsche Glyphe im richtigen Schnitt. Wo es kein
/// `/ToUnicode` gibt, bleibt Schritt 4 die letzte Möglichkeit.
fn glyph_candidates(program: &FontProgram, embedded: bool, code: u32) -> Vec<(Face, GlyphKey)> {
    let mut keys = Vec::with_capacity(4);
    let unicode = program
        .code_to_unicode
        .get(&code)
        .and_then(|text| text.chars().next());

    if program.is_cid {
        if embedded {
            let gid = match &program.code_to_gid {
                CodeToGid::Identity => u16::try_from(code).ok(),
                CodeToGid::Map(map) => map.get(&code).copied(),
                CodeToGid::ViaCharCode => None,
            };
            if let Some(gid) = gid {
                keys.push((Face::Own, GlyphKey::Gid(gid)));
            }
        }
        if let Some(ch) = unicode {
            keys.push((Face::Own, GlyphKey::Char(ch)));
            if embedded {
                keys.push((Face::Substitute, GlyphKey::Char(ch)));
            }
        }
        return keys;
    }

    if let Some(ch) = unicode {
        keys.push((Face::Own, GlyphKey::Char(ch)));
    }
    if embedded {
        // Symbolische Subsets bilden ihre Codes gern über eine (3,0)-cmap im
        // privaten Bereich 0xF000..0xF0FF ab.
        if code < 0x100 {
            if let Some(ch) = char::from_u32(0xF000 + code) {
                keys.push((Face::Own, GlyphKey::Char(ch)));
            }
        }
        if let Some(ch) = unicode {
            keys.push((Face::Substitute, GlyphKey::Char(ch)));
        }
        if let Ok(gid) = u16::try_from(code) {
            keys.push((Face::Own, GlyphKey::Gid(gid)));
        }
    }
    keys
}

// ---------------------------------------------------------------------------
// Pfadbau
// ---------------------------------------------------------------------------

/// Baut aus User-Space-Segmenten einen Pfad in Gerätekoordinaten.
///
/// `None`, wenn nichts Zeichenbares übrig bleibt oder eine Koordinate nicht
/// endlich ist.
fn device_path(segments: &[PathSeg], to_device: &Matrix) -> Option<Path> {
    let mut builder = PathBuilder::with_capacity(segments.len() + 1, segments.len() * 2 + 2);
    let mut open = false;
    for segment in segments {
        match segment {
            PathSeg::MoveTo(p) => {
                let (x, y) = map(to_device, *p)?;
                builder.move_to(x, y);
                open = true;
            }
            PathSeg::LineTo(p) => {
                let (x, y) = map(to_device, *p)?;
                if open {
                    builder.line_to(x, y);
                } else {
                    builder.move_to(x, y);
                    open = true;
                }
            }
            PathSeg::CubicTo(c1, c2, p) => {
                let (x1, y1) = map(to_device, *c1)?;
                let (x2, y2) = map(to_device, *c2)?;
                let (x, y) = map(to_device, *p)?;
                if !open {
                    builder.move_to(x1, y1);
                    open = true;
                }
                builder.cubic_to(x1, y1, x2, y2, x, y);
            }
            PathSeg::Close => {
                if open {
                    builder.close();
                }
            }
        }
    }
    builder.finish()
}

/// Wie [`device_path`], aber für Glyph-Umrisse (die zusätzlich Quadratiken kennen).
fn outline_path(outline: &Outline, to_device: &Matrix) -> Option<Path> {
    let mut builder =
        PathBuilder::with_capacity(outline.segments.len() + 1, outline.segments.len() * 2 + 2);
    let mut open = false;
    for segment in &outline.segments {
        match *segment {
            Seg::MoveTo(x, y) => {
                let (x, y) = map_xy(to_device, x, y)?;
                builder.move_to(x, y);
                open = true;
            }
            Seg::LineTo(x, y) => {
                let (x, y) = map_xy(to_device, x, y)?;
                if open {
                    builder.line_to(x, y);
                } else {
                    builder.move_to(x, y);
                    open = true;
                }
            }
            Seg::QuadTo(cx, cy, x, y) => {
                let (cx, cy) = map_xy(to_device, cx, cy)?;
                let (x, y) = map_xy(to_device, x, y)?;
                if !open {
                    builder.move_to(cx, cy);
                    open = true;
                }
                builder.quad_to(cx, cy, x, y);
            }
            Seg::CubicTo(cx0, cy0, cx1, cy1, x, y) => {
                let (cx0, cy0) = map_xy(to_device, cx0, cy0)?;
                let (cx1, cy1) = map_xy(to_device, cx1, cy1)?;
                let (x, y) = map_xy(to_device, x, y)?;
                if !open {
                    builder.move_to(cx0, cy0);
                    open = true;
                }
                builder.cubic_to(cx0, cy0, cx1, cy1, x, y);
            }
            Seg::Close => {
                if open {
                    builder.close();
                }
            }
        }
    }
    builder.finish()
}

fn map(to_device: &Matrix, p: Point) -> Option<(f32, f32)> {
    map_xy(to_device, p.x, p.y)
}

fn map_xy(to_device: &Matrix, x: f64, y: f64) -> Option<(f32, f32)> {
    let p = to_device.apply(x, y);
    let (x, y) = (p.x as f32, p.y as f32);
    (x.is_finite() && y.is_finite()).then_some((x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geo(rotate: i64) -> Geometry {
        Geometry::new(
            Rect::new(0.0, 0.0, 200.0, 100.0),
            rotate,
            &RenderOptions {
                width: 400,
                ..RenderOptions::default()
            },
        )
    }

    #[test]
    fn unrotated_transform_flips_y() {
        let g = geo(0);
        assert_eq!((g.width, g.height), (400, 200));
        let top_left = g.to_device.apply(0.0, 100.0);
        assert!(top_left.x.abs() < 1e-6 && top_left.y.abs() < 1e-6);
        let bottom_right = g.to_device.apply(200.0, 0.0);
        assert!((bottom_right.x - 400.0).abs() < 1e-6);
        assert!((bottom_right.y - 200.0).abs() < 1e-6);
    }

    #[test]
    fn rotation_swaps_the_canvas() {
        let g = geo(90);
        assert_eq!((g.width, g.height), (400, 800));
        // Bei 90° im Uhrzeigersinn wandert links unten nach links oben.
        let ll = g.to_device.apply(0.0, 0.0);
        assert!(ll.x.abs() < 1e-6 && ll.y.abs() < 1e-6);
        assert_eq!(g.page_box.width(), 100.0);
        assert_eq!(g.page_box.height(), 200.0);
    }

    #[test]
    fn absurd_media_box_falls_back_to_a4() {
        let mut warnings = Vec::new();
        let fixed = sane_box(Rect::new(0.0, 0.0, 0.0, 0.0), &mut warnings);
        assert_eq!(fixed, A4);
        assert_eq!(warnings.len(), 1);

        let mut warnings = Vec::new();
        let fixed = sane_box(Rect::new(0.0, 0.0, f64::NAN, 10.0), &mut warnings);
        assert_eq!(fixed, A4);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn max_pixels_caps_both_edges() {
        let g = Geometry::new(
            Rect::new(0.0, 0.0, 100.0, 10_000.0),
            0,
            &RenderOptions {
                width: 5000,
                max_pixels: 500,
                ..RenderOptions::default()
            },
        );
        assert!(
            g.width <= 500 && g.height <= 500,
            "{}x{}",
            g.width,
            g.height
        );
        assert!(g.width >= 1 && g.height >= 1);
    }

    #[test]
    fn font_keys_differ_for_different_programs() {
        let mut a = FontProgram {
            base_font: "Helvetica".into(),
            kind: redact_pdf::ops::FontKind::TrueType,
            data: Some(vec![1, 2, 3]),
            units_per_em: 1000.0,
            code_to_gid: CodeToGid::ViaCharCode,
            is_cid: false,
            widths: Default::default(),
            default_width: 0.5,
            code_to_unicode: Default::default(),
            flags: 0,
        };
        let key_a = font_key(&a);
        a.data = Some(vec![1, 2, 4]);
        assert_ne!(key_a, font_key(&a));
    }
}
