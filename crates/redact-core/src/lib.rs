//! # redact-core
//!
//! Domänenmodell, Traits und Fehlertypen für redact-rs.
//!
//! Dieses Crate hat bewusst keine Logik zum Parsen von PDFs oder zum Matchen von
//! Text — es definiert nur die gemeinsame Sprache, die alle anderen Crates
//! (`redact-pdf`, `redact-patterns`, `redact-booking`, `redact-cli`,
//! `redact-gui`) sprechen.

#![forbid(unsafe_code)]

pub mod conflict;
pub mod error;
pub mod geometry;
pub mod model;
pub mod review;
pub mod traits;

pub use conflict::{negative_rects, resolve_conflicts, BlockedRegion, Resolution};
pub use error::{RedactError, Result};
pub use geometry::{bounding_box, Glyph, Point, Rect, TextRun};
pub use model::{Action, BookingEntry, ListType, MatchType, Redaction, Region, Source};
pub use review::{ReviewFile, ReviewInput, ReviewItem, REVIEW_FORMAT_VERSION};
pub use traits::{Analyzer, BookingLoader, Extractor, Redactor, Renderer};

/// Re-Export, damit abhängige Crates dieselbe lopdf-Version verwenden.
pub use lopdf;
