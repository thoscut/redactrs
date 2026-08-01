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
pub mod font;
pub mod glyphnames;
pub mod image;
pub mod matrix;
pub mod meta;
pub mod ops;
pub mod redact;
pub mod testing;

pub use audit_bytes::leaks;
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
