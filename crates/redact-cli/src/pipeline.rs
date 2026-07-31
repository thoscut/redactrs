//! Die Verarbeitungskette — identisch für CLI und GUI.
//!
//! ```text
//! laden → Text extrahieren → Analyse (manuell + Buchungen + Patterns)
//!       → Konflikte auflösen → [Review] → schwärzen → Metadaten strippen
//!       → schreiben → Audit-Log
//! ```

use std::path::{Path, PathBuf};

use redact_booking::{BookingMatcher, CsvBookingLoader};
use redact_core::{
    resolve_conflicts, Action, BlockedRegion, BookingLoader, Extractor, RedactError, Redaction,
    Region, Renderer, Result, ReviewFile, ReviewInput, TextRun,
};
use redact_patterns::PatternMatcher;
use redact_pdf::{PdfExtractor, PdfRedactor, PdfRenderer};

use crate::audit::{sha256_bytes, AuditLog};

/// Alle Einstellungen eines Laufs.
#[derive(Debug, Clone)]
pub struct Config {
    pub input: PathBuf,
    pub output: Option<PathBuf>,
    pub patterns: Vec<String>,
    pub no_patterns: bool,
    pub patterns_config: Option<PathBuf>,
    pub booking_list: Option<PathBuf>,
    pub manual_regions: Option<PathBuf>,
    pub review: bool,
    pub review_out: Option<PathBuf>,
    pub apply_review: Option<PathBuf>,
    pub audit_log: Option<PathBuf>,
    pub action: Action,
    pub padding: f64,
}

/// Ergebnis eines Laufs — Grundlage für die Ausgabe auf der Konsole.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Outcome {
    pub input: String,
    pub output: Option<String>,
    pub review_out: Option<String>,
    pub pages: usize,
    pub text_runs: usize,
    pub candidates: usize,
    pub redactions: usize,
    pub blocked: usize,
    pub removed_glyphs: usize,
    pub removed_annotations: usize,
    pub audit_log: Option<String>,
    /// Klartextbeschreibung der von der Negativliste blockierten Treffer.
    pub blocked_details: Vec<String>,
    pub warnings: Vec<String>,
}

/// Führt einen kompletten Lauf aus.
pub fn run(config: &Config) -> Result<Outcome> {
    let mut outcome = Outcome {
        input: config.input.display().to_string(),
        ..Default::default()
    };

    // 1. PDF laden (streng geprüft, keine Reparaturversuche).
    let mut doc = redact_pdf::load(&config.input)?;
    outcome.pages = redact_pdf::page_count(&doc);

    // 2./3./4./5. Analyse — oder eine bereits geprüfte Review-Datei.
    let (redactions, blocked) = match &config.apply_review {
        Some(path) => {
            let data = std::fs::read_to_string(path)?;
            let review = ReviewFile::from_json(&data)?;
            verify_review_matches_input(&review, &config.input)?;
            let blocked = review.blocked_by_negative_list.clone();
            outcome.blocked_details = describe_blocked(&blocked);
            (review.redactions(), blocked)
        }
        None => {
            let runs = PdfExtractor::new().extract(&doc)?;
            outcome.text_runs = runs.len();

            let candidates = collect_regions(config, &runs)?;
            outcome.candidates = candidates.len();

            let resolution = resolve_conflicts(candidates);
            outcome.blocked = resolution.blocked.len();
            outcome.blocked_details = describe_blocked(&resolution.blocked);

            // 7. Review-Modus: nur exportieren.
            if config.review {
                let path = review_target(config);
                let review = ReviewFile::new(
                    ReviewInput {
                        path: config.input.display().to_string(),
                        sha256: sha256_bytes(&std::fs::read(&config.input)?),
                        pages: outcome.pages,
                    },
                    resolution.redact,
                    resolution.blocked,
                );
                write_json(&path, &review.to_json()?)?;
                outcome.review_out = Some(path.display().to_string());
                return Ok(outcome);
            }

            let redactions = resolution
                .redact
                .into_iter()
                .map(|region| Redaction::new(region, config.action.clone()))
                .collect();
            (redactions, resolution.blocked)
        }
    };

    outcome.redactions = redactions.len();
    outcome.blocked = outcome.blocked.max(blocked.len());

    let Some(output) = &config.output else {
        return Err(RedactError::Config(
            "keine Ausgabedatei angegeben — bitte -o/--output oder --review benutzen".into(),
        ));
    };
    if output == &config.input {
        return Err(RedactError::Config(
            "Ausgabe- und Eingabedatei dürfen nicht identisch sein".into(),
        ));
    }

    // 9. Schwärzung anwenden.
    let report =
        PdfRedactor::with_padding(config.padding).apply_with_report(&mut doc, &redactions)?;
    outcome.removed_glyphs = report.removed_glyphs;
    outcome.removed_annotations = report.removed_annotations;
    outcome.warnings = report.warnings.clone();

    // 10. Metadaten strippen.
    redact_pdf::strip_metadata(&mut doc);

    // 11. Ausgabe schreiben.
    PdfRenderer::new().render(&doc, output)?;
    outcome.output = Some(output.display().to_string());

    // 12. Audit-Log.
    if let Some(path) = &config.audit_log {
        let log = AuditLog::build(
            &config.input,
            output,
            &redactions,
            &blocked,
            &report.warnings,
        )?;
        log.write(path)?;
        outcome.audit_log = Some(path.display().to_string());
    }

    Ok(outcome)
}

/// Schritte 3–5: manuelle Regionen, Buchungsliste, Patterns.
fn collect_regions(config: &Config, runs: &[TextRun]) -> Result<Vec<Region>> {
    let mut regions = Vec::new();

    // 3. Manuelle Regionen (haben Vorrang und werden nie blockiert).
    if let Some(path) = &config.manual_regions {
        regions.extend(load_manual_regions(path)?);
    }

    // 4. Buchungslisten-Matching (Negativ- und Positivtreffer).
    if let Some(path) = &config.booking_list {
        let entries = CsvBookingLoader.load(path)?;
        let matcher = BookingMatcher::new(entries)?;
        regions.extend(matcher.find_matches(runs)?);
    }

    // 5. Pattern-Matching.
    if !config.no_patterns {
        let matcher = match &config.patterns_config {
            Some(path) => PatternMatcher::from_config_file(path)?,
            None => PatternMatcher::new(&config.patterns)?,
        };
        regions.extend(matcher.find_matches(runs)?);
    }

    Ok(regions)
}

/// Lädt manuelle Regionen aus einer JSON-Datei.
///
/// Akzeptiert sowohl ein nacktes Array von Regionen (Format aus dem Konzept)
/// als auch eine Review-Datei — so kann man beide Formate durchreichen.
pub fn load_manual_regions(path: &Path) -> Result<Vec<Region>> {
    let data = std::fs::read_to_string(path)?;
    if let Ok(regions) = serde_json::from_str::<Vec<Region>>(&data) {
        return Ok(regions.into_iter().map(normalize_region).collect());
    }
    match ReviewFile::from_json(&data) {
        Ok(review) => Ok(review
            .items
            .into_iter()
            .filter(|i| i.enabled)
            .map(|i| normalize_region(i.region))
            .collect()),
        Err(e) => Err(RedactError::Parse(format!(
            "{}: weder eine Regionsliste noch eine Review-Datei ({e})",
            path.display()
        ))),
    }
}

fn normalize_region(region: Region) -> Region {
    Region::new(region.page, region.rect, region.text, region.source)
}

/// Stellt sicher, dass die Review-Datei zur Eingabedatei gehört.
fn verify_review_matches_input(review: &ReviewFile, input: &Path) -> Result<()> {
    if review.input.sha256.is_empty() {
        return Ok(());
    }
    let actual = sha256_bytes(&std::fs::read(input)?);
    if actual != review.input.sha256 {
        return Err(RedactError::Parse(format!(
            "Review-Datei wurde für eine andere Eingabe erstellt \
             (erwartet {}, tatsächlich {})",
            &review.input.sha256[..review.input.sha256.len().min(12)],
            &actual[..12]
        )));
    }
    Ok(())
}

fn review_target(config: &Config) -> PathBuf {
    config.review_out.clone().unwrap_or_else(|| {
        let mut path = config.input.clone();
        path.set_extension("review.json");
        path
    })
}

fn write_json(path: &Path, json: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(path, json)?;
    Ok(())
}

/// Zusätzlich verfügbare Blocker-Informationen für die Ausgabe.
pub fn describe_blocked(blocked: &[BlockedRegion]) -> Vec<String> {
    blocked
        .iter()
        .map(|b| {
            format!(
                "Seite {}: „{}“ (Buchung {}) blockiert {}",
                b.page + 1,
                b.pattern,
                b.booking_id,
                b.blocked_reason.as_deref().unwrap_or("einen Treffer")
            )
        })
        .collect()
}
