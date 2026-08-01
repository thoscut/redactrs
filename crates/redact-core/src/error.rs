//! Fehler-Typen für alle redact-rs Crates.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum RedactError {
    #[error("PDF-Fehler: {0}")]
    Pdf(String),
    #[error("Pattern-Fehler: {0}")]
    Pattern(String),
    #[error("Buchungslisten-Fehler: {0}")]
    Booking(String),
    #[error("IO-Fehler: {0}")]
    Io(#[from] std::io::Error),
    #[error("Parse-Fehler: {0}")]
    Parse(String),
    #[error("Konfigurationsfehler: {0}")]
    Config(String),
}

impl From<lopdf::Error> for RedactError {
    fn from(e: lopdf::Error) -> Self {
        RedactError::Pdf(e.to_string())
    }
}

impl From<serde_json::Error> for RedactError {
    fn from(e: serde_json::Error) -> Self {
        RedactError::Parse(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, RedactError>;
