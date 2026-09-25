//! Gemeinsamer Unterbau der Beispielprogramme, die die Belege in `docs/`
//! erzeugen.
//!
//! Hier steht alles, was mehr als ein Beispiel braucht: das Bild im Speicher,
//! die Ausschnitt-Rechnung, das Rendern einer PDF-Seite — und in den
//! Untermodulen die beiden Dateiformate ([`png`], [`gif`]) sowie der
//! Konsolensatz ([`console`]).
//!
//! # Warum das alles hier liegt und nicht in `src/`
//!
//! Kein Byte davon gehört ins ausgelieferte Binary. Ein PNG- und ein
//! GIF-Schreiber sind Werkzeuge zum Erzeugen von Belegen, nicht Bestandteil
//! eines Programms, das Bankunterlagen schwärzt. Als `examples/` werden sie
//! mitkompiliert und mitgeprüft (`cargo clippy --all-targets`), landen aber
//! niemals in `redact-rs`.
//!
//! # Warum kein Crate von der Stange
//!
//! `redact-render` bindet `tiny-skia` ohne `png-format` ein und hat keine
//! Bildbibliothek; `image`, `png` oder `gif` als Abhängigkeit aufzunehmen,
//! hieße, die Lieferkette des Programms für Belegbilder zu verbreitern. Die
//! beiden Schreiber unten kosten zusammen ein paar hundert Zeilen und
//! benutzen für Deflate `lopdf::Stream::compress` — also den Kompressor, der
//! wegen der PDF-Streams ohnehin im Baum steckt.

// Jedes Beispiel benutzt nur einen Teil dieses Moduls; ohne diese Zeile
// meldete `-D warnings` in jedem Beispiel die jeweils ungenutzte Hälfte als
// toten Code. Das ist der übliche Preis für geteilten Beispielcode.
#![allow(dead_code)]

pub mod console;
pub mod gif;
pub mod png;

use std::error::Error;
use std::path::Path;

use redact_render::{PageRenderer, RenderOptions, RenderedPage};

/// Rand in Pixeln, der beim Zuschneiden um den Inhalt stehen bleibt.
pub const CROP_MARGIN: u32 = 16;

pub type Fehler = Box<dyn Error>;

// ---------------------------------------------------------------------------
// Bild
// ---------------------------------------------------------------------------

/// Ein Bild im Speicher: RGBA8, ohne Vormultiplikation — dasselbe Format, das
/// [`RenderedPage::rgba`] liefert.
#[derive(Clone, PartialEq, Eq)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Canvas {
    pub fn new(width: u32, height: u32, fill: [u8; 4]) -> Self {
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        for _ in 0..(width as usize * height as usize) {
            rgba.extend_from_slice(&fill);
        }
        Self {
            width,
            height,
            rgba,
        }
    }

    pub fn from_page(page: &RenderedPage) -> Self {
        Self {
            width: page.width,
            height: page.height,
            rgba: page.rgba.clone(),
        }
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        if x >= self.width || y >= self.height {
            return [255, 255, 255, 255];
        }
        let at = ((y as usize) * (self.width as usize) + x as usize) * 4;
        [
            self.rgba[at],
            self.rgba[at + 1],
            self.rgba[at + 2],
            self.rgba[at + 3],
        ]
    }

    pub fn set(&mut self, x: u32, y: u32, pixel: [u8; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let at = ((y as usize) * (self.width as usize) + x as usize) * 4;
        self.rgba[at..at + 4].copy_from_slice(&pixel);
    }

    /// Anteil der Pixel, die nicht reinweiß sind (0.0 … 1.0).
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

    /// Anteil der Pixel, die von `background` abweichen (0.0 … 1.0).
    ///
    /// Die allgemeine Form von [`Canvas::non_white_ratio`] — die Konsole hat
    /// keinen weißen Hintergrund, und „leer“ heißt dort „überall
    /// Hintergrundfarbe“.
    pub fn ink_ratio(&self, background: [u8; 4]) -> f64 {
        let total = self.rgba.len() / 4;
        if total == 0 {
            return 0.0;
        }
        let hits = self
            .rgba
            .chunks_exact(4)
            .filter(|p| p[0] != background[0] || p[1] != background[1] || p[2] != background[2])
            .count();
        hits as f64 / total as f64
    }

    pub fn is_gray(&self) -> bool {
        self.rgba
            .chunks_exact(4)
            .all(|p| p[0] == p[1] && p[1] == p[2])
    }

    /// Kopie des Ausschnitts; außerhalb liegende Pixel werden weiß.
    pub fn crop(&self, window: &Window) -> Self {
        let mut out = Vec::with_capacity(window.width as usize * window.height as usize * 4);
        for y in 0..window.height {
            for x in 0..window.width {
                out.extend_from_slice(&self.pixel(window.x + x, window.y + y));
            }
        }
        Self {
            width: window.width,
            height: window.height,
            rgba: out,
        }
    }

    /// Kleinstes Rechteck, in dem sich `self` und `other` unterscheiden.
    ///
    /// `None`, wenn die Bilder gleich sind. Panickt nicht bei verschiedenen
    /// Größen, sondern meldet das ganze Bild als geändert — der Aufrufer
    /// bekommt dann eben ein volles Einzelbild statt eines Ausschnitts.
    pub fn diff_window(&self, other: &Canvas) -> Option<Window> {
        if self.width != other.width || self.height != other.height {
            return Some(Window {
                x: 0,
                y: 0,
                width: self.width,
                height: self.height,
            });
        }
        let (mut left, mut top) = (u32::MAX, u32::MAX);
        let (mut right, mut bottom) = (0u32, 0u32);
        let stride = self.width as usize * 4;
        for y in 0..self.height {
            let row = y as usize * stride;
            if self.rgba[row..row + stride] == other.rgba[row..row + stride] {
                continue;
            }
            for x in 0..self.width {
                let at = row + x as usize * 4;
                if self.rgba[at..at + 4] != other.rgba[at..at + 4] {
                    left = left.min(x);
                    top = top.min(y);
                    right = right.max(x);
                    bottom = bottom.max(y);
                }
            }
        }
        if left > right || top > bottom {
            return None;
        }
        Some(Window {
            x: left,
            y: top,
            width: right - left + 1,
            height: bottom - top + 1,
        })
    }
}

impl std::fmt::Debug for Canvas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Canvas")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Ausschnitt
// ---------------------------------------------------------------------------

/// Ein Bildausschnitt in Pixeln.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Kleinster Ausschnitt, der den Inhalt **aller** übergebenen Bilder enthält.
///
/// # Warum über alle zusammen und nicht je Bild
///
/// Getrennt zugeschnittene Bilder wären ein stiller Betrug: das geschwärzte
/// Blatt hat weniger Inhalt, sein Ausschnitt fiele enger aus, und im Vergleich
/// sähe man dann Unterschiede in der Vergrößerung statt Unterschiede in der
/// Schwärzung. Ein gemeinsamer Ausschnitt kann das nicht. Für eine Animation
/// gilt dasselbe noch schärfer — ein wandernder Ausschnitt sähe aus wie eine
/// Kamerafahrt, die nie stattgefunden hat.
///
/// Fällt auf das volle Bild zurück, wenn nirgends etwas steht.
pub fn shared_window<'a>(images: impl Iterator<Item = &'a Canvas>) -> Window {
    let (mut left, mut top) = (u32::MAX, u32::MAX);
    let (mut right, mut bottom) = (0u32, 0u32);
    let (mut full_width, mut full_height) = (0u32, 0u32);

    for image in images {
        full_width = full_width.max(image.width);
        full_height = full_height.max(image.height);
        for y in 0..image.height {
            for x in 0..image.width {
                let pixel = image.pixel(x, y);
                if pixel[0] == 255 && pixel[1] == 255 && pixel[2] == 255 {
                    continue;
                }
                left = left.min(x);
                top = top.min(y);
                right = right.max(x);
                bottom = bottom.max(y);
            }
        }
    }

    if left > right || top > bottom {
        return Window {
            x: 0,
            y: 0,
            width: full_width,
            height: full_height,
        };
    }

    let x = left.saturating_sub(CROP_MARGIN);
    let y = top.saturating_sub(CROP_MARGIN);
    let right = (right + CROP_MARGIN).min(full_width.saturating_sub(1));
    let bottom = (bottom + CROP_MARGIN).min(full_height.saturating_sub(1));
    Window {
        x,
        y,
        width: right - x + 1,
        height: bottom - y + 1,
    }
}

// ---------------------------------------------------------------------------
// Seiten rendern
// ---------------------------------------------------------------------------

/// Rendert Seite `page_index` (0-basiert) aus der PDF-Datei `input`.
///
/// Gibt außerdem aus, was der Renderer über die Seite zu sagen hatte —
/// `degraded` und `warnings` werden nicht verschluckt.
pub fn render_page(
    renderer: &mut PageRenderer,
    input: &Path,
    page_index: usize,
    width: u32,
) -> Result<Canvas, Fehler> {
    let doc = redact_pdf::document::load(input)?;
    let opts = RenderOptions {
        width,
        ..RenderOptions::default()
    };
    let page = renderer.render(&doc, page_index, &opts);

    println!(
        "  {}: Seite {} → {}x{}, {} Zeichenoperationen, {:.2} % nicht weiß{}",
        input.display(),
        page_index + 1,
        page.width,
        page.height,
        page.drawn_ops,
        page.non_white_ratio() * 100.0,
        if page.degraded { "  ⚠ NOTNAGEL" } else { "" }
    );
    for warning in &page.warnings {
        println!("    Warnung: {warning}");
    }

    refuse_empty(input, page_index, &page)?;
    Ok(Canvas::from_page(&page))
}

/// Bricht ab, wenn das Bild nichts zeigt.
///
/// `render` liefert nie `Err`: kann es eine Seite nicht lesen, entsteht ein
/// weißes Blatt mit `degraded = true`. Für die Vorschau im Programm ist das
/// die richtige Antwort — für ein Bild, das als Beleg ins Repository geht,
/// nicht. Genau dieser Fehler ist hier schon einmal passiert: die Vorschau
/// zeigte über Monate leere Seiten, weil niemand hinsah.
fn refuse_empty(input: &Path, page_index: usize, page: &RenderedPage) -> Result<(), Fehler> {
    if page.degraded {
        return Err(format!(
            "{}: Seite {} war nicht lesbar, das Bild zeigt nur den Hintergrund. \
             Ein leeres Blatt als Beleg zu schreiben wäre schlimmer als kein Beleg.",
            input.display(),
            page_index + 1
        )
        .into());
    }
    if page.non_white_ratio() == 0.0 {
        return Err(format!(
            "{}: Seite {} ist vollständig weiß ({} Zeichenoperationen). \
             Entweder ist die Seite leer oder der Renderer hat nichts getroffen — \
             in beiden Fällen taugt das Bild nicht als Beleg.",
            input.display(),
            page_index + 1,
            page.drawn_ops
        )
        .into());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Kleinkram
// ---------------------------------------------------------------------------

/// Schreibt `bytes` nach `path` und meldet, was entstanden ist.
pub fn write_out(path: &Path, bytes: &[u8], what: &str) -> Result<(), Fehler> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)?;
    println!(
        "geschrieben: {} ({what}, {} Byte)",
        path.display(),
        bytes.len()
    );
    Ok(())
}

/// Holt den Wert hinter einem Schalter.
pub fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, Fehler> {
    args.next()
        .ok_or_else(|| format!("{flag} braucht einen Wert").into())
}
