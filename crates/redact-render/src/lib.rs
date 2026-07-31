//! Rasterisierung von PDF-Seiten für die Vorschau.
//!
//! Diese Datei bleibt bewusst schlank: sie deklariert nur die Untermodule und
//! exportiert deren öffentliche Typen erneut.
//!
//! # Mitgelieferte Ersatzfonts
//!
//! Für die 14 Standard-PDF-Fonts (Helvetica, Times, Courier & Co.), die
//! praktisch nie eingebettet sind, liegen in `assets/fonts/` zwölf Schnitte
//! bei. Sie sind aus den **Liberation Fonts** abgeleitet (metrisch kompatibel
//! zu Arial/Times New Roman/Courier New), auf einen lateinischen Zeichenvorrat
//! reduziert und — wie es Klausel 3 der Lizenz verlangt — in „Redact Sans“,
//! „Redact Serif“ und „Redact Mono“ umbenannt, damit kein Reserved Font Name
//! einer Modified Version verwendet wird.
//!
//! Lizenz: **SIL Open Font License 1.1**, vollständiger Text in
//! `assets/fonts/LICENSE-OFL.txt`. Zusammen belegen die zwölf Dateien
//! 329 400 Bytes (rund 322 KiB) im Binary.
//!
//! Erzeugt wurden sie je Schnitt mit
//!
//! ```text
//! pyftsubset Liberation<Familie>-<Schnitt>.ttf \
//!   --unicodes=U+0020-007E,U+00A0-017F,U+0192,U+02C6-02DD,\
//!              U+2000-206F,U+20AC,U+2122,U+2212,U+FB01-FB02,U+FFFD \
//!   --layout-features= --no-hinting --glyph-names --notdef-outline \
//!   --drop-tables+=GSUB,GPOS,GDEF,DSIG,kern,morx,gasp,FFTM
//! ```
//!
//! gefolgt vom Umschreiben der `name`-Tabelle auf die neuen Familiennamen.

#![forbid(unsafe_code)]

pub mod fonts;
pub mod raster;

pub use fonts::{FontCache, GlyphFont, GlyphKey, Outline, Seg};
pub use raster::{PageRenderer, RenderOptions, RenderedPage};
