//! # redact-pdf
//!
//! PDF-Verarbeitung für redact-rs: Text extrahieren, Text **wirklich**
//! entfernen, Metadaten strippen, Ergebnis schreiben.
//!
//! ## Warum eigener Interpreter?
//!
//! `lopdf::Document::extract_text` liefert nur eine Zeichenkette — ohne
//! Koordinaten. Für eine Schwärzung braucht man aber beides: den Text (um zu
//! entscheiden, *was* geschwärzt wird) und die Position jedes einzelnen
//! Zeichens (um zu wissen, *wo*). Deshalb interpretiert [`content`] den
//! Content-Stream selbst und führt Grafik- und Textzustand mit.
//!
//! ## Ablauf
//!
//! ```text
//!  PDF ──load──► Document ──scan_page──► ShowRecord[] ──► TextRun[] (Zeilen)
//!                    │                        │
//!                    │                        └──► Analyse (Patterns/Buchungen)
//!                    │                                     │
//!                    └───────── PdfRedactor::apply ◄───────┘
//!                                    │
//!                              strip_metadata ──► PdfRenderer::render ──► PDF
//! ```

#![forbid(unsafe_code)]

pub mod audit_bytes;
pub mod content;
pub mod document;
pub mod encoding;
pub mod extract;
pub mod filters;
pub mod font;
pub mod glyphnames;
pub mod image;
pub mod matrix;
pub mod meta;
pub mod ops;
pub mod redact;
pub mod testing;

pub use audit_bytes::{leaks, leaks_many, leaks_many_within, squeeze, LeakCheck};
pub use content::{
    interpret, scan_page, ContentSink, GlyphEvent, GlyphItem, ImageEvent, MarkedTextRecord,
    PathEvent, ScanResult, ShowItem, ShowRecord, SinkContext, StreamKey, MIRROR_KEYS,
};
pub use document::{
    load, load_from_bytes, page_box, page_boxes, page_count, save_to_bytes, validate, PdfRenderer,
};
pub use extract::PdfExtractor;
pub use matrix::Matrix;
pub use meta::{strip_metadata, MetadataReport};
pub use ops::{
    page_ops, ClipRef, CodeToGid, DrawOp, FontKind, FontProgram, PageOps, PathSeg, RasterImage,
    Rgb, Stroke,
};
pub use redact::{PdfRedactor, RedactionReport};

/// Ein Name oder Text **aus der Eingabedatei**, so aufbereitet, dass er in
/// einer Meldung stehen darf.
///
/// # Warum
///
/// Ob eine Warnung eine Deckungslücke ist (Rückgabewert 3, „NICHT GEPRÜFT“)
/// oder bloß eine Mitteilung, entscheidet
/// `redact_pipeline::coverage::is_coverage_gap` an festen Textmarken wie
/// „enthalten Rasterbilder“. Die Namen in der Meldung — XObject, Font,
/// Filter, Feldname — stammen aber aus der Datei, und die ist nicht
/// vertrauenswürdig: ein XObject `/enthalten#20Rasterbilder` ohne `/Subtype`
/// trug die Marke in seine eigene Lückenmeldung hinein. Die Datei lief mit
/// Rückgabewert 0 durch, die IBAN darin stand unverändert in der Ausgabe
/// (Experten-Review, Befund CLI-1).
///
/// Jede Marke enthält ein gewöhnliches Leerzeichen (der Test
/// `jede_marke_enthaelt_ein_leerzeichen` in `redact-pipeline` hält das fest).
/// Hier wird deshalb jeder Leerraum und jedes Steuerzeichen zu einem
/// geschützten Leerzeichen (U+00A0): am Bildschirm sieht der Name gleich aus,
/// aber keine Marke passt mehr hinein.
pub fn label(bytes: &[u8]) -> String {
    label_text(&String::from_utf8_lossy(bytes))
}

/// [`label`] für Text, der schon dekodiert ist (etwa ein Feldname `/T`).
pub fn label_text(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_whitespace() || c.is_control() {
                '\u{a0}'
            } else {
                c
            }
        })
        .collect()
}
