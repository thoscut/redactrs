//! # redact-core
//!
//! Domänenmodell und Fehlertypen für redact-rs.
//!
//! Dieses Crate hat bewusst keine Logik zum Parsen von PDFs oder zum Matchen von
//! Text — es definiert nur die gemeinsame Sprache, die alle anderen Crates
//! (`redact-pdf`, `redact-patterns`, `redact-booking`, `redact-cli`,
//! `redact-gui`) sprechen.

#![forbid(unsafe_code)]

pub mod conflict;
pub mod display;
pub mod error;
pub mod geometry;
pub mod model;
pub mod naming;
pub mod read;
pub mod review;

pub use conflict::{resolve_conflicts, BlockedRegion, Resolution};
pub use display::{safe_path, safe_text};
pub use error::{RedactError, Result};
pub use geometry::{bounding_box, Glyph, Point, Rect, TextRun};
pub use model::{
    Action, BookingEntry, ListType, MatchType, Redaction, Region, Source, DEFAULT_REPLACEMENT,
};
pub use naming::{
    check_output_suffix, output_path_with_suffix, sibling_path, AUDIT_SUFFIX,
    DEFAULT_OUTPUT_SUFFIX, REVIEW_SUFFIX,
};
pub use read::{read_limited, MAX_AUX_FILE_BYTES, MAX_CHECK_NEEDLES};
pub use review::{ReviewFile, ReviewInput, ReviewItem, REVIEW_FORMAT_VERSION};

/// Re-Export, damit abhängige Crates dieselbe lopdf-Version verwenden.
pub use lopdf;
