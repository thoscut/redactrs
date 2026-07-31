//! Audit-Log: was wurde geschwärzt, was wurde bewusst *nicht* geschwärzt.
//!
//! Das Log ist der Nachweis für Dritte. Es enthält deshalb SHA-256 beider
//! Dateien — damit lässt sich später belegen, dass genau diese Eingabe zu
//! genau dieser Ausgabe geführt hat.

use std::path::Path;

use redact_core::{BlockedRegion, Rect, Redaction, Result};
use redact_pdf::document::{write_file, WriteOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLog {
    pub timestamp: String,
    pub tool: ToolInfo,
    pub input: FileInfo,
    pub output: FileInfo,
    pub redactions: Vec<AuditEntry>,
    pub blocked_by_negative_list: Vec<BlockedEntry>,
    pub metadata_stripped: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// 1-basierte Seitennummer (im Log menschenlesbar, intern 0-basiert).
    pub page: usize,
    pub rect: Rect,
    pub action: redact_core::Action,
    pub reason: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockedEntry {
    pub page: usize,
    pub pattern: String,
    pub booking_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
}

impl AuditLog {
    pub fn build(
        input: &Path,
        output: &Path,
        redactions: &[Redaction],
        blocked: &[BlockedRegion],
        warnings: &[String],
    ) -> Result<Self> {
        Ok(Self {
            timestamp: timestamp(),
            tool: ToolInfo {
                name: "redact-rs".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            input: FileInfo {
                path: input.display().to_string(),
                sha256: sha256_file(input)?,
            },
            output: FileInfo {
                path: output.display().to_string(),
                sha256: sha256_file(output)?,
            },
            redactions: redactions
                .iter()
                .map(|r| AuditEntry {
                    page: r.region.page + 1,
                    rect: r.region.rect,
                    action: r.action.clone(),
                    reason: r.reason.clone(),
                    source: r.region.source_kind().to_string(),
                })
                .collect(),
            blocked_by_negative_list: blocked
                .iter()
                .map(|b| BlockedEntry {
                    page: b.page + 1,
                    pattern: b.pattern.clone(),
                    booking_id: b.booking_id.clone(),
                    blocked_reason: b.blocked_reason.clone(),
                })
                .collect(),
            metadata_stripped: true,
            warnings: warnings.to_vec(),
        })
    }

    /// Schreibt das Log über den zentralen Schreibpfad.
    ///
    /// Das Log nennt jede gefundene Stelle im Klartext — es gehört deshalb
    /// unter Unix mit Modus 0600 angelegt und darf keine fremde Datei
    /// überschreiben. Beides steckt in `options`.
    pub fn write(&self, path: &Path, options: &WriteOptions) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        write_file(path, json.as_bytes(), options)
    }
}

/// SHA-256 einer Datei als Hex-String.
pub fn sha256_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(sha256_bytes(&bytes))
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn timestamp() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use redact_core::{Action, Redaction, Region, Source};

    #[test]
    fn sha256_matches_known_value() {
        assert_eq!(
            sha256_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn audit_entries_use_one_based_pages() {
        let dir = tempdir();
        let input = dir.join("in.pdf");
        let output = dir.join("out.pdf");
        std::fs::write(&input, b"a").unwrap();
        std::fs::write(&output, b"b").unwrap();

        let redaction = Redaction::new(
            Region::new(
                0,
                Rect::new(1.0, 2.0, 3.0, 4.0),
                Some("DE89".into()),
                Source::Pattern {
                    pattern_id: "iban_de".into(),
                    confidence: 0.99,
                },
            ),
            Action::Blackout,
        );
        let log = AuditLog::build(&input, &output, &[redaction], &[], &[]).unwrap();
        assert_eq!(log.redactions[0].page, 1);
        assert_eq!(log.redactions[0].source, "auto");
        assert!(log.redactions[0].reason.contains("iban_de"));
        assert!(log.metadata_stripped);

        let json = serde_json::to_string(&log).unwrap();
        assert!(json.contains("\"action\":\"blackout\""));
        std::fs::remove_dir_all(dir).ok();
    }

    fn tempdir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("redact-audit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
