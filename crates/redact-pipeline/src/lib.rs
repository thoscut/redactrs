//! Die Verarbeitungskette — identisch für CLI und GUI.
//!
//! ```text
//! laden → Text extrahieren → Analyse (manuell + Buchungen + Patterns)
//!       → Konflikte auflösen → [Review] → schwärzen → Metadaten strippen
//!       → schreiben → Audit-Log
//! ```
//!
//! ## Warum ein eigenes Crate
//!
//! Der Satz oben stand jahrelang über `redact-cli::pipeline` und war falsch:
//! `redact-gui` hatte nicht einmal eine Abhängigkeit auf die CLI, sondern rund
//! 130 abgetippte Zeilen — und die waren auseinandergelaufen (Polsterung fest
//! auf 1.0, stilles Überschreiben der Ausgabe, Audit-Log mit leeren Prüfsummen
//! und hart verdrahtetem `metadata_stripped: true`, Schreiben ohne den einen
//! Schreibpfad aus `SECURITY.md`).
//!
//! Nach `redact-core` konnte die Kette nicht: dort steht das Domänenmodell, und
//! `redact-pdf`, `redact-patterns` und `redact-booking` hängen ihrerseits daran
//! — die Kette braucht aber genau diese drei. Das wäre ein Zyklus. Also ein
//! eigenes Crate **über** ihnen, das beide Programme benutzen.
//!
//! ## Wer ruft was
//!
//! * [`run`] ist der ganze Weg von der Datei bis zum Log — das macht die
//!   Kommandozeile.
//! * [`collect_regions`] (Schritte 3–5) und [`apply`] (Schritte 9–12) sind die
//!   beiden Hälften daraus. Die Oberfläche ruft sie einzeln auf, weil zwischen
//!   ihnen die Nutzerin sitzt: an-/abwählen, Rechtecke ziehen, verschieben.
//!   Es ist derselbe Code, nicht derselbe Ablauf noch einmal.

#![forbid(unsafe_code)]

pub mod audit;

use std::path::{Path, PathBuf};

use lopdf::Document;
use redact_booking::{BookingMatcher, CsvBookingLoader};
use redact_core::{
    output_path_with_suffix, resolve_conflicts, sibling_path, Action, BlockedRegion, RedactError,
    Redaction, Region, Result, ReviewFile, ReviewInput, TextRun, REVIEW_SUFFIX,
};
use redact_patterns::PatternMatcher;
use redact_pdf::document::{
    check_target, load_from_bytes_with_limits, write_file, Limits, WriteOptions,
};
use redact_pdf::{PdfExtractor, PdfRedactor, PdfRenderer};

pub use crate::audit::{
    effect_warnings, sha256_bytes, sha256_file, Applied, AuditLog, EntryEffect,
};

/// Vorgabe für `--padding`, in Punkt.
pub const DEFAULT_PADDING: f64 = 1.0;

/// Vorgabe für `--max-candidates`, siehe [`Config::max_candidates`].
pub const DEFAULT_MAX_CANDIDATES: usize = 100_000;

/// Alle Einstellungen eines Laufs.
#[derive(Debug, Clone)]
pub struct Config {
    pub input: PathBuf,
    pub output: Option<PathBuf>,
    /// Namenszusatz, wenn keine Ausgabedatei angegeben wurde.
    pub output_suffix: String,
    /// Vorhandene Dateien überschreiben.
    pub force: bool,
    pub patterns: Vec<String>,
    pub no_patterns: bool,
    pub patterns_config: Option<PathBuf>,
    /// Mindestvertrauen; ohne Angabe gilt `redact_patterns::DEFAULT_MIN_CONFIDENCE`.
    pub min_confidence: Option<f32>,
    pub booking_list: Option<PathBuf>,
    pub manual_regions: Option<PathBuf>,
    pub review: bool,
    pub review_out: Option<PathBuf>,
    pub apply_review: Option<PathBuf>,
    /// Eine Review-Datei **ohne** Prüfsumme trotzdem anwenden.
    ///
    /// Ohne diesen Schalter wird sie abgelehnt: `"sha256": ""` von Hand
    /// eingetragen hebelte die Identitätsprüfung sonst vollständig aus.
    pub allow_unverified_review: bool,
    pub audit_log: Option<PathBuf>,
    pub action: Action,
    pub padding: f64,
    /// Bilder, die sich nicht dekodieren lassen, durchgehen lassen statt
    /// abzubrechen — **unsicher**, siehe `--allow-undecodable-images`.
    pub allow_undecodable_images: bool,
    /// Obergrenze für die gleichzeitig gehaltenen **dekodierten** Bildbytes
    /// (`--max-image-mb`).
    ///
    /// Das ist die einzige Grenze, die den Speicherbedarf der Bildschwärzung
    /// deckelt: `--max-decompressed-mb` verbucht die *Rohbytes* eines Streams,
    /// und ein 1-Bit-Scan wächst beim Auspacken nach RGBA8 um den Faktor 32.
    pub max_decoded_image_bytes: u64,
    /// Obergrenzen für die Eingabedatei (siehe `SECURITY.md`).
    pub limits: Limits,
    /// Obergrenze für die Zahl der Trefferkandidaten (Zeitbremse).
    pub max_candidates: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            input: PathBuf::new(),
            output: None,
            output_suffix: redact_core::DEFAULT_OUTPUT_SUFFIX.to_string(),
            force: false,
            patterns: Vec::new(),
            no_patterns: false,
            patterns_config: None,
            min_confidence: None,
            booking_list: None,
            manual_regions: None,
            review: false,
            review_out: None,
            apply_review: None,
            allow_unverified_review: false,
            audit_log: None,
            action: Action::Blackout,
            padding: DEFAULT_PADDING,
            allow_undecodable_images: false,
            max_decoded_image_bytes: redact_pdf::image::DEFAULT_MAX_DECODED_IMAGE_BYTES,
            limits: Limits::default(),
            max_candidates: DEFAULT_MAX_CANDIDATES,
        }
    }
}

/// Ergebnis eines Laufs — Grundlage für die Ausgabe auf der Konsole und für
/// die Statuszeile der Oberfläche.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Outcome {
    pub input: String,
    /// SHA-256 der **verarbeiteten** Bytes, nicht der Datei nach dem Lauf.
    pub input_sha256: String,
    pub output: Option<String>,
    pub review_out: Option<String>,
    pub pages: usize,
    pub text_runs: usize,
    pub candidates: usize,
    pub redactions: usize,
    /// Davon wirksam — Regionen, die nach `--padding` noch ein Rechteck haben.
    pub effective_redactions: usize,
    /// Davon entartet: leeres Rechteck, also ohne jede Wirkung.
    pub degenerate_redactions: usize,
    pub blocked: usize,
    pub removed_glyphs: usize,
    pub drawn_rects: usize,
    pub removed_annotations: usize,
    /// Bilder, deren Bildpunkte überschrieben wurden.
    ///
    /// Neu kodiert wird verlustfrei; die Datei wird dadurch größer.
    pub redacted_images: usize,
    /// Davon Kopien für mehrfach benutzte Bilder.
    pub copied_images: usize,
    /// Klartext dessen, was der Metadatenlauf wirklich entfernt hat.
    pub metadata_removed: Vec<String>,
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

    // 0. Alle Schreibziele *vor* der Arbeit prüfen.
    //
    // Sonst merkt man erst nach dem Schwärzen, dass das Ziel die Eingabedatei
    // ist — und bei `--force` wäre das Original dann schon weg. Geprüft wird
    // hier, geschrieben später; `write_file` prüft ein zweites Mal.
    plan_outputs(config)?;

    // 1. PDF laden (streng geprüft, keine Reparaturversuche).
    //
    // Die Bytes werden **einmal** gelesen: die Prüfsumme muss die der
    // verarbeiteten Bytes sein, nicht die einer inzwischen ausgetauschten
    // Datei. Die Oberfläche macht es genauso (`AppState::load_bytes`).
    let bytes = std::fs::read(&config.input)?;
    outcome.input_sha256 = sha256_bytes(&bytes);
    let mut doc = load_from_bytes_with_limits(&bytes, &config.limits).map_err(|e| match e {
        RedactError::Pdf(msg) => RedactError::Pdf(format!("{}: {msg}", config.input.display())),
        other => other,
    })?;
    outcome.pages = redact_pdf::page_count(&doc);

    // 2./3./4./5. Analyse — oder eine bereits geprüfte Review-Datei.
    let (redactions, blocked) = match &config.apply_review {
        Some(path) => {
            let data = std::fs::read_to_string(path)?;
            let review = ReviewFile::from_json(&data)?;
            check_review_identity(
                &review,
                &outcome.input_sha256,
                config.allow_unverified_review,
            )?;
            let blocked = review.blocked_by_negative_list.clone();
            outcome.blocked_details = describe_blocked(&blocked);
            (review.redactions(), blocked)
        }
        None => {
            // `extract_with_warnings` statt `extract`: der Interpreter bricht
            // an mehreren Stellen still ab (XObject ohne `/Subtype`, Form mit
            // unbekanntem Filter, zu tiefe Verschachtelung, Kachelmuster).
            // Dort steht Text, den die Analyse nicht sieht — wer das nicht
            // erfährt, hält eine Datei mit „0 Schwärzungen“ für sauber.
            let (runs, extract_warnings) = PdfExtractor::new().extract_with_warnings(&doc)?;
            push_warnings(&mut outcome.warnings, extract_warnings);
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
                        sha256: outcome.input_sha256.clone(),
                        pages: outcome.pages,
                    },
                    resolution.redact,
                    resolution.blocked,
                );
                write_review_file(&path, &review, config)?;
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

    // 9./10./11./12. — derselbe Code, den die Oberfläche für ihren Export ruft.
    apply(&mut doc, &redactions, &blocked, config, &mut outcome)?;
    Ok(outcome)
}

/// Schritte 9–12: schwärzen → Metadaten strippen → schreiben → Audit-Log.
///
/// **Die gemeinsame Hälfte.** Was zwischen Analyse und Schwärzung geschieht,
/// unterscheidet die beiden Programme (die Kommandozeile rechnet durch, die
/// Oberfläche lässt auswählen); ab hier darf es keinen Unterschied mehr geben.
/// Gleiche `redactions`, gleicher `config` ⇒ Byte für Byte dieselbe Ausgabe und
/// dasselbe Log.
///
/// `outcome` wird ergänzt, nicht ersetzt: bereits eingetragene Warnungen (etwa
/// die des Extraktors) bleiben stehen und landen mit im Log.
pub fn apply(
    doc: &mut Document,
    redactions: &[Redaction],
    blocked: &[BlockedRegion],
    config: &Config,
    outcome: &mut Outcome,
) -> Result<()> {
    outcome.redactions = redactions.len();
    outcome.blocked = outcome.blocked.max(blocked.len());

    let output = plan_outputs(config)?.output.ok_or_else(|| {
        RedactError::Config("ohne --review muss das Ausgabeziel feststehen".into())
    })?;

    // 9. Schwärzung anwenden.
    let report = PdfRedactor::with_padding(config.padding)
        .allowing_undecodable_images(config.allow_undecodable_images)
        .with_max_decoded_image_bytes(config.max_decoded_image_bytes)
        .apply_with_report(doc, redactions)?;
    outcome.removed_glyphs = report.removed_glyphs;
    outcome.drawn_rects = report.drawn_rects;
    outcome.removed_annotations = report.removed_annotations;
    outcome.redacted_images = report.redacted_images;
    outcome.copied_images = report.copied_images;
    // Anfügen, nicht ersetzen: die Warnungen des Extraktors stehen schon drin
    // und sind die einzige Stelle, an der „auf dieser Seite konnten wir nichts
    // lesen“ überhaupt sichtbar wird.
    push_warnings(&mut outcome.warnings, report.warnings.clone());

    // Was die Schwärzung *nicht* bewirkt hat, gehört genauso gemeldet.
    // `--padding=-100` etwa lässt von jedem Rechteck nichts übrig; solche
    // Regionen werden übersprungen und dürfen nicht als Erfolg zählen.
    outcome.degenerate_redactions = degenerate_count(redactions, config.padding);
    outcome.effective_redactions = redactions.len() - outcome.degenerate_redactions;
    push_warnings(
        &mut outcome.warnings,
        effect_warnings(redactions, config.padding, &report),
    );

    // 10. Metadaten strippen.
    let metadata = redact_pdf::strip_metadata(doc);
    outcome.metadata_removed = metadata.summary();

    // 11. Ausgabe schreiben.
    PdfRenderer::with_options(output_options(config)).render(doc, &output)?;
    outcome.output = Some(output.display().to_string());

    // 12. Audit-Log.
    if let Some(path) = &config.audit_log {
        let log = AuditLog::build(
            &config.input,
            &outcome.input_sha256,
            &output,
            redactions,
            blocked,
            Applied {
                padding: config.padding,
                redaction: &report,
                metadata: &metadata,
            },
            &outcome.warnings,
        )?;
        log.write(path, &secret_options(config).protect(output.clone()))?;
        outcome.audit_log = Some(path.display().to_string());
    }

    Ok(())
}

/// Hängt Warnungen an, ohne Dubletten.
///
/// Die Warnungen kommen aus mehreren Quellen — Extraktor, Schwärzung,
/// Wirkungsprüfung — und beschreiben teils denselben Befund (etwa ein
/// Rasterbild, das sowohl beim Lesen als auch beim Überdecken auffällt). Im
/// Audit-Log soll jeder Befund genau einmal stehen.
pub fn push_warnings(target: &mut Vec<String>, warnings: Vec<String>) {
    for warning in warnings {
        if !target.contains(&warning) {
            target.push(warning);
        }
    }
}

/// Zählt die Regionen, von denen `--padding` nichts übrig lässt.
///
/// Dieselbe Bewertung wie im Audit-Log — bewusst *eine* Quelle, damit
/// Zusammenfassung und Nachweis nicht auseinanderlaufen können.
fn degenerate_count(redactions: &[Redaction], padding: f64) -> usize {
    redactions
        .iter()
        .filter(|r| EntryEffect::of(r.region.rect, padding) == EntryEffect::Degenerate)
        .count()
}

/// Zeitbremse: die Konfliktauflösung wächst quadratisch mit der Trefferzahl.
///
/// Gemessen (Release, 500 Seiten × 88 Zeilen aus einer 212-kB-Datei): 264 000
/// Treffer kosten 133 s, 52 800 Treffer 4,7 s — Vervierfachung bei
/// Verdopplung. Ursache ist `dedup` in `redact-core::conflict`, das jede
/// Region gegen alle bereits behaltenen prüft. Solange das so ist, braucht die
/// Kette eine Obergrenze, sonst reicht eine knappe Megabyte-Datei, um die
/// Maschine eine Stunde zu beschäftigen.
fn check_candidate_budget(config: &Config, candidates: usize) -> Result<()> {
    if candidates > config.max_candidates {
        return Err(RedactError::Config(format!(
            "{candidates} Trefferkandidaten überschreiten die Obergrenze von {}. \
             Die Konfliktauflösung wächst quadratisch; eine solche Datei würde \
             die Maschine über Gebühr beschäftigen. Mit --max-candidates lässt \
             sich die Grenze anheben, wenn die Datei wirklich so aussieht.",
            config.max_candidates
        )));
    }
    Ok(())
}

/// Schritte 3–5: manuelle Regionen, Buchungsliste, Patterns.
///
/// Die zweite gemeinsame Hälfte: die Oberfläche ruft dieselbe Funktion, damit
/// `--patterns-config`, `--no-patterns`, `--min-confidence` und
/// `--manual-regions` auch dort gelten — vorher kannte sie nichts davon.
pub fn collect_regions(config: &Config, runs: &[TextRun]) -> Result<Vec<Region>> {
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
        let mut matcher = match &config.patterns_config {
            Some(path) => PatternMatcher::from_config_file(path)?,
            None => PatternMatcher::new(&config.patterns)?,
        };
        // `--min-confidence` gewinnt gegen die Schwelle aus der
        // Konfigurationsdatei: die Kommandozeile ist die spätere Anweisung.
        if let Some(min) = config.min_confidence {
            matcher = matcher.with_min_confidence(min)?;
        }
        regions.extend(matcher.find_matches(runs)?);
    }

    check_candidate_budget(config, regions.len())?;
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

// ---------------------------------------------------------------- Identität

/// Gehört eine Review-Datei zum verarbeiteten Dokument?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewIdentity {
    /// Prüfsummen vorhanden und gleich.
    Matches,
    /// Die Datei nennt keine Prüfsumme. Nur mit ausdrücklicher Erlaubnis.
    Unchecked,
    /// Prüfsummen vorhanden und verschieden — die Datei gehört woandershin.
    Mismatch,
}

/// Vergleicht die Prüfsumme aus der Review-Datei mit der des Dokuments.
///
/// Reine Funktion, damit die Entscheidung ohne Dateien und ohne Fenster
/// prüfbar ist.
pub fn review_identity(review_sha: &str, document_sha: &str) -> ReviewIdentity {
    if review_sha.is_empty() || document_sha.is_empty() {
        return ReviewIdentity::Unchecked;
    }
    if review_sha.eq_ignore_ascii_case(document_sha) {
        ReviewIdentity::Matches
    } else {
        ReviewIdentity::Mismatch
    }
}

/// Die ersten Stellen einer Prüfsumme — mehr braucht eine Meldung nicht.
fn short_sha(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}

/// Stellt sicher, dass eine Review-Datei zum Dokument gehört.
///
/// Eine Review-Datei sagt nur „schwärze bei diesen Koordinaten“. Auf ein
/// anderes Dokument angewendet liegen die Rechtecke an beliebigen Stellen: das
/// Ergebnis sieht geschwärzt aus und ist es nicht.
///
/// Eine **leere** Prüfsumme wurde früher stillschweigend durchgewinkt. Wer
/// `"sha256": ""` von Hand einträgt, umging die Prüfung damit vollständig —
/// und die Oberfläche schrieb vor Aufgabe 36 selbst solche Dateien. Sie wird
/// deshalb abgelehnt; `allow_unverified` (`--allow-unverified-review`) ist der
/// ausdrückliche Weg daran vorbei.
pub fn check_review_identity(
    review: &ReviewFile,
    document_sha: &str,
    allow_unverified: bool,
) -> Result<ReviewIdentity> {
    let identity = review_identity(&review.input.sha256, document_sha);
    match identity {
        ReviewIdentity::Matches => Ok(identity),
        ReviewIdentity::Mismatch => Err(RedactError::Config(format!(
            "Diese Review-Datei gehört zu einem anderen Dokument — sie wurde für \
             eine andere Eingabe erstellt (Datei: {}…, verarbeitet: {}…). Sie wurde \
             nicht angewendet, die Schwärzungen lägen an falschen Stellen. \
             Bitte „{}“ verwenden oder eine passende Review-Datei wählen.",
            short_sha(&review.input.sha256),
            short_sha(document_sha),
            match review.input.path.trim() {
                "" => "das zugehörige PDF",
                path => path,
            }
        ))),
        ReviewIdentity::Unchecked if allow_unverified => Ok(identity),
        ReviewIdentity::Unchecked => Err(RedactError::Config(format!(
            "Diese Review-Datei nennt keine Prüfsumme ihres Dokuments ({}) — es lässt \
             sich also nicht feststellen, ob sie zu dieser Eingabe gehört. Auf ein \
             fremdes Dokument angewendet lägen die Schwärzungen an beliebigen Stellen. \
             Wer das trotzdem will, sagt es ausdrücklich: --allow-unverified-review.",
            match review.input.path.trim() {
                "" => "auch kein Dateiname",
                path => path,
            }
        ))),
    }
}

// ------------------------------------------------------------- Schreibziele

fn review_target(config: &Config) -> PathBuf {
    config
        .review_out
        .clone()
        .unwrap_or_else(|| sibling_path(&config.input, REVIEW_SUFFIX, "json"))
}

/// Schreibt eine Review-Datei über den zentralen Schreibpfad.
///
/// In der Datei stehen die gefundenen Geheimnisse im Klartext — sie gehört
/// unter Unix mit Modus 0600 angelegt, ohne Symlink und in einem Zug. Die
/// Oberfläche hat das früher mit `std::fs::write` umgangen.
pub fn write_review_file(path: &Path, review: &ReviewFile, config: &Config) -> Result<()> {
    write_file(path, review.to_json()?.as_bytes(), &secret_options(config))
}

/// Bestimmt die Ausgabedatei.
///
/// Ohne `-o` wird neben der Eingabedatei gespeichert — `kontoauszug.pdf` wird
/// also zu `kontoauszug_geschwaerzt.pdf`.
pub fn output_path(config: &Config) -> PathBuf {
    match &config.output {
        Some(path) => path.clone(),
        None => output_path_with_suffix(&config.input, &config.output_suffix),
    }
}

/// Schreibregeln für die Ausgabe-PDF.
///
/// Eine vorhandene Datei wird nur mit `--force` überschrieben, damit ein
/// zweiter Lauf nicht unbemerkt ein bereits geprüftes Ergebnis ersetzt. Die
/// Eingabedatei ist immer geschützt — auch mit `--force`.
pub fn output_options(config: &Config) -> WriteOptions {
    WriteOptions::new()
        .force(config.force)
        .protect(config.input.clone())
}

/// Schreibregeln für Review-Datei und Audit-Log.
///
/// Wie die Ausgabe, zusätzlich aber `private`: in beiden Dateien stehen die
/// gefundenen Geheimnisse im Klartext. Unter Unix entstehen sie mit Modus 0600.
pub fn secret_options(config: &Config) -> WriteOptions {
    output_options(config).private(true)
}

/// Alle Schreibziele eines Laufs, vorab geprüft.
pub struct OutputPlan {
    pub output: Option<PathBuf>,
}

/// Prüft die Schreibziele, bevor gerechnet wird.
///
/// Bis hierher hatte jeder Ausgabeweg seine eigene (oder gar keine) Prüfung:
/// `--write-demo`, `--review-out` und `--audit-log` haben `--force` schlicht
/// ignoriert, und der Vergleich „Ausgabe == Eingabe“ verglich rohe Pfade und
/// war damit über `./in.pdf`, `dir/../in.pdf` oder einen Symlink zu umgehen.
pub fn plan_outputs(config: &Config) -> Result<OutputPlan> {
    if config.review {
        check_target(&review_target(config), &secret_options(config))?;
        return Ok(OutputPlan { output: None });
    }

    let output = output_path(config);
    let target = check_target(&output, &output_options(config))?;

    if let Some(path) = &config.audit_log {
        // Das Audit-Log darf weder die Eingabe noch die frisch geschriebene
        // Ausgabe treffen.
        check_target(path, &secret_options(config).protect(target.path()))?;
    }

    Ok(OutputPlan {
        output: Some(output),
    })
}

/// Zusätzlich verfügbare Blocker-Informationen für die Ausgabe.
///
/// Das `+ 1` ist Absicht und die einzige erlaubte Umrechnung: Fließtext sagt
/// „Seite 1“, JSON zählt ab 0 (siehe [`audit::AuditEntry::page`]).
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

#[cfg(test)]
mod tests {
    use super::*;
    use redact_core::{Glyph, Rect, ReviewInput};

    /// Eine Textzeile mit gleichmäßig gesetzten Zeichen.
    fn text_run(text: &str) -> TextRun {
        let glyphs = text
            .chars()
            .enumerate()
            .map(|(i, ch)| Glyph {
                ch,
                rect: Rect::new(i as f64 * 6.0, 100.0, i as f64 * 6.0 + 6.0, 110.0),
            })
            .collect();
        TextRun::new(0, glyphs)
    }

    fn review_with(sha: &str) -> ReviewFile {
        ReviewFile::new(
            ReviewInput {
                path: "auszug.pdf".into(),
                sha256: sha.to_string(),
                pages: 1,
            },
            Vec::new(),
            Vec::new(),
        )
    }

    #[test]
    fn identity_only_objects_when_both_sides_are_known() {
        let a = sha256_bytes(b"Dokument A");
        let b = sha256_bytes(b"Dokument B");
        assert_eq!(review_identity(&a, &a), ReviewIdentity::Matches);
        assert_eq!(
            review_identity(&a.to_uppercase(), &a),
            ReviewIdentity::Matches
        );
        assert_eq!(review_identity(&a, &b), ReviewIdentity::Mismatch);
        assert_eq!(review_identity("", &a), ReviewIdentity::Unchecked);
        assert_eq!(review_identity(&a, ""), ReviewIdentity::Unchecked);
    }

    /// Aufgabe 54: `"sha256": ""` hebelte die Identitätsprüfung aus — in
    /// **beiden** Programmen, weil beide stillschweigend `Ok` lieferten.
    #[test]
    fn a_review_file_without_a_checksum_is_refused_unless_allowed() {
        let document = sha256_bytes(b"Dokument A");
        let error = check_review_identity(&review_with(""), &document, false)
            .expect_err("ohne Prüfsumme muss abgelehnt werden")
            .to_string();
        assert!(error.contains("keine Prüfsumme"), "{error}");
        assert!(error.contains("--allow-unverified-review"), "{error}");

        // Nur mit ausdrücklichem Schalter geht sie durch.
        assert_eq!(
            check_review_identity(&review_with(""), &document, true).unwrap(),
            ReviewIdentity::Unchecked
        );
    }

    #[test]
    fn a_review_file_for_another_document_is_refused_even_with_the_switch() {
        let document = sha256_bytes(b"Dokument A");
        let other = sha256_bytes(b"Dokument B");
        for allow in [false, true] {
            let error = check_review_identity(&review_with(&other), &document, allow)
                .expect_err("fremde Prüfsumme muss abgelehnt werden")
                .to_string();
            // Beide Programme melden denselben Satz.
            assert!(error.contains("anderen Dokument"), "{error}");
            assert!(error.contains("andere Eingabe"), "{error}");
            assert!(error.contains("auszug.pdf"), "{error}");
        }
    }

    #[test]
    fn a_matching_checksum_passes() {
        let document = sha256_bytes(b"Dokument A");
        assert_eq!(
            check_review_identity(&review_with(&document), &document, false).unwrap(),
            ReviewIdentity::Matches
        );
    }

    #[test]
    fn the_candidate_budget_is_enforced_in_the_shared_analysis() {
        let config = Config {
            max_candidates: 0,
            ..Config::default()
        };
        let runs = vec![text_run("IBAN DE89 3704 0044 0532 0130 00")];
        let error = collect_regions(&config, &runs)
            .expect_err("die Obergrenze muss greifen")
            .to_string();
        assert!(error.contains("Obergrenze"), "{error}");
    }
}
