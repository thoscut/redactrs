//! Audit-Log: was wurde geschwärzt, was wurde bewusst *nicht* geschwärzt.
//!
//! Das Log ist der Nachweis für Dritte. Es enthält deshalb SHA-256 beider
//! Dateien — damit lässt sich später belegen, dass genau diese Eingabe zu
//! genau dieser Ausgabe geführt hat.
//!
//! ## Absicht ist nicht Ergebnis
//!
//! Das Log hat lange die *Absicht* protokolliert: jede geplante Schwärzung
//! erschien als Eintrag, und `metadata_stripped` stand hart auf `true`. Mit
//! `--padding=-100` schrumpft jedes Rechteck jedoch zu einem leeren Bereich;
//! die Schwärzung entfernt dann kein einziges Zeichen — das Log bescheinigte
//! trotzdem eine ausgeführte Schwärzung und entfernte Metadaten. Ein Nachweis,
//! der Nicht-Getanes bescheinigt, ist schlimmer als gar keiner.
//!
//! Deshalb hält das Log jetzt Gemessenes fest:
//!
//! * [`AuditEntry::effective_rect`] — das Rechteck **nach** `--padding`, also
//!   das, was tatsächlich gewirkt hat, plus [`AuditEntry::effect`] mit dem
//!   Befund je Region,
//! * [`EffectRecord`] — wie viele Zeichen, Deck-Rechtecke, Annotationen und
//!   Bilder die Schwärzung wirklich angefasst hat,
//! * [`MetadataRecord`] — was der Metadatenlauf wirklich entfernt hat.
//!
//! Was sich nicht messen lässt, steht auch nicht drin: eine Zuordnung
//! „Region → entfernte Zeichen“ liefert [`redact_pdf::RedactionReport`] nicht,
//! deshalb nennt das Log Zeichenzahlen nur als Gesamtsumme.
//!
//! ## Eine Fassung für beide Programme
//!
//! Dieses Modul lag früher in `redact-cli`; die grafische Oberfläche hatte
//! daneben ihr eigenes, von Hand gebautes JSON — mit leeren Prüfsummen, hart
//! verdrahtetem `metadata_stripped: true` und ohne jede Wirkungsmessung. Es
//! gibt jetzt nur noch dieses hier.

use std::path::Path;

use redact_core::{BlockedRegion, Rect, Redaction, Result};
use redact_pdf::document::{write_file, WriteOptions};
use redact_pdf::{MetadataReport, RedactionReport};
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
    /// Hat der Metadatenlauf tatsächlich etwas entfernt?
    ///
    /// Bewusst kein „wurde ausgeführt“: ausgeführt wird er immer. `false`
    /// heißt, dass die Eingabedatei nichts zu entfernen hatte. Die
    /// Einzelheiten stehen in [`AuditLog::metadata`].
    pub metadata_stripped: bool,
    /// Was der Metadatenlauf entfernt hat — gezählt, nicht behauptet.
    pub metadata: MetadataRecord,
    /// Was die Schwärzung bewirkt hat — gemessen, nicht beabsichtigt.
    pub effect: EffectRecord,
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
    /// Seitennummer, **0-basiert** — dieselbe Zählweise wie in der
    /// Review-Datei, in `--manual-regions` und im ganzen Werkzeug.
    ///
    /// Früher zählte allein das Audit-Log ab 1. Damit bedeutete `page` in
    /// `review.json` und in `audit.json` nicht dasselbe, obwohl der Workflow
    /// dazu einlädt, beide Dateien nebeneinander zu legen: eine aus dem Log
    /// abgeschriebene Seitenzahl landete in einer Regionsdatei eine Seite zu
    /// weit hinten. Es gilt jetzt eine Regel: **JSON zählt ab 0, Fließtext
    /// (Konsole, Oberfläche) sagt „Seite 1“.**
    pub page: usize,
    /// Das gefundene Rechteck, so wie die Analyse es geliefert hat.
    pub rect: Rect,
    /// Dasselbe Rechteck nach `--padding` — das ist es, was gewirkt hat.
    pub effective_rect: Rect,
    /// Befund für diese Region.
    pub effect: EntryEffect,
    pub action: redact_core::Action,
    pub reason: String,
    pub source: String,
}

/// Was aus einer geplanten Schwärzung geworden ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryEffect {
    /// Das Rechteck war nach `--padding` noch gültig; die Schwärzung lief.
    Applied,
    /// Das Rechteck ist nach `--padding` leer. Eine solche Region wird von
    /// [`redact_pdf::PdfRedactor`] übersprungen: kein Zeichen entfernt, kein
    /// Rechteck gezeichnet. Sie darf nicht als Erfolg gelten.
    Degenerate,
}

impl EntryEffect {
    /// Bewertet eine Region gegen das Padding, mit dem gerechnet wurde.
    ///
    /// Der Test ist derselbe, den [`redact_pdf::PdfRedactor`] anlegt:
    /// `rect.expanded(padding)` und dann `is_empty()`.
    pub fn of(rect: Rect, padding: f64) -> Self {
        if rect.expanded(padding).is_empty() {
            Self::Degenerate
        } else {
            Self::Applied
        }
    }
}

/// Was der Lauf tatsächlich bewirkt hat.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EffectRecord {
    /// Rand, der auf jedes Rechteck gerechnet wurde (`--padding`).
    pub padding: f64,
    /// Geplante Schwärzungen.
    pub requested: usize,
    /// Davon mit gültigem Rechteck.
    pub applied: usize,
    /// Davon entartet (leeres Rechteck) — wirkungslos.
    pub degenerate: usize,
    /// Tatsächlich aus den Content-Streams entfernte Zeichen.
    pub removed_glyphs: usize,
    /// Tatsächlich gezeichnete Deck-Rechtecke.
    pub drawn_rects: usize,
    /// Tatsächlich entfernte Annotationen.
    pub removed_annotations: usize,
    /// Bilder, deren Pixel überschrieben wurden.
    ///
    /// Gehört ins Log, weil ein überschriebenes Bild **neu kodiert** wird:
    /// außerhalb des Schwärzungsbereichs bleibt jedes Bildpunktbyte gleich
    /// (`redact-pdf` schreibt immer verlustfrei mit Flate), die *Datei* ist
    /// aber nicht mehr dieselbe — aus einem `/DCTDecode`-Bild wird
    /// `/FlateDecode`, und das ist deutlich größer.
    pub redacted_images: usize,
    /// Davon Kopien, die angelegt wurden, weil dasselbe Bild mehrfach im
    /// Dokument benutzt wird.
    pub copied_images: usize,
}

/// Was der Metadatenlauf entfernt hat — die Zahlen aus [`MetadataReport`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataRecord {
    pub info: bool,
    pub xmp: bool,
    pub piece_info: usize,
    pub struct_tree: bool,
    pub names: usize,
    pub embedded_files: usize,
    pub javascript: usize,
    pub acroform: bool,
    pub xfa: bool,
    pub field_values: usize,
    pub file_attachments: usize,
    pub open_action: bool,
    pub additional_actions: usize,
    pub optional_content: bool,
    /// Dieselbe Information in Klartext, für Menschen.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub summary: Vec<String>,
}

impl From<&MetadataReport> for MetadataRecord {
    fn from(report: &MetadataReport) -> Self {
        Self {
            info: report.info_removed,
            xmp: report.xmp_removed,
            piece_info: report.piece_info_removed,
            struct_tree: report.struct_tree_removed,
            names: report.names_removed,
            embedded_files: report.embedded_files_removed,
            javascript: report.javascript_removed,
            acroform: report.acroform_removed,
            xfa: report.xfa_removed,
            field_values: report.field_values_cleared,
            file_attachments: report.file_attachments_removed,
            open_action: report.open_action_removed,
            additional_actions: report.additional_actions_removed,
            optional_content: report.optional_content_removed,
            summary: report.summary(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockedEntry {
    /// 0-basiert, siehe [`AuditEntry::page`].
    pub page: usize,
    pub pattern: String,
    pub booking_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
}

/// Die gemessenen Ergebnisse eines Laufs, so wie sie ins Log gehören.
#[derive(Debug, Clone, Copy)]
pub struct Applied<'a> {
    /// Rand, mit dem geschwärzt wurde.
    pub padding: f64,
    /// Bericht der Schwärzung.
    pub redaction: &'a RedactionReport,
    /// Bericht des Metadatenlaufs.
    pub metadata: &'a MetadataReport,
}

impl AuditLog {
    /// Baut das Log aus dem, was der Lauf gemessen hat.
    ///
    /// `input_sha` wird **übergeben** und nicht aus der Datei nachgerechnet:
    /// die Prüfsumme gehört zu den Bytes, die tatsächlich verarbeitet wurden.
    /// Wer sie nach dem Lauf neu läse, bekäme im ungünstigen Fall die einer
    /// inzwischen geänderten Datei — und die grafische Oberfläche kann ein
    /// Dokument verarbeiten, das gar nicht (mehr) auf der Platte liegt.
    pub fn build(
        input: &Path,
        input_sha: &str,
        output: &Path,
        redactions: &[Redaction],
        blocked: &[BlockedRegion],
        applied: Applied<'_>,
        warnings: &[String],
    ) -> Result<Self> {
        let entries: Vec<AuditEntry> = redactions
            .iter()
            .map(|r| AuditEntry {
                page: r.region.page,
                rect: r.region.rect,
                effective_rect: r.region.rect.expanded(applied.padding),
                effect: EntryEffect::of(r.region.rect, applied.padding),
                action: r.action.clone(),
                reason: r.reason.clone(),
                source: r.region.source_kind().to_string(),
            })
            .collect();
        let degenerate = entries
            .iter()
            .filter(|e| e.effect == EntryEffect::Degenerate)
            .count();

        Ok(Self {
            timestamp: timestamp(),
            tool: ToolInfo {
                name: "redact-rs".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            input: FileInfo {
                path: input.display().to_string(),
                sha256: input_sha.to_string(),
            },
            output: FileInfo {
                path: output.display().to_string(),
                sha256: sha256_file(output)?,
            },
            effect: EffectRecord {
                padding: applied.padding,
                requested: entries.len(),
                applied: entries.len() - degenerate,
                degenerate,
                removed_glyphs: applied.redaction.removed_glyphs,
                drawn_rects: applied.redaction.drawn_rects,
                removed_annotations: applied.redaction.removed_annotations,
                redacted_images: applied.redaction.redacted_images,
                copied_images: applied.redaction.copied_images,
            },
            redactions: entries,
            blocked_by_negative_list: blocked
                .iter()
                .map(|b| BlockedEntry {
                    page: b.page,
                    pattern: b.pattern.clone(),
                    booking_id: b.booking_id.clone(),
                    blocked_reason: b.blocked_reason.clone(),
                })
                .collect(),
            metadata_stripped: applied.metadata.anything_removed(),
            metadata: MetadataRecord::from(applied.metadata),
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

/// Warnungen, die sich aus dem *Ergebnis* eines Laufs ergeben.
///
/// Drei Fälle, die stillschweigend als Erfolg durchgingen:
///
/// 1. Eine Region, die nach `--padding` leer ist. Sie wird übersprungen — die
///    Zusammenfassung meldete trotzdem „Schwärzungen: 1“.
/// 2. Ein Lauf, in dem keine einzige Region ein Zeichen entfernt hat. Das ist
///    nicht zwingend falsch (ein Bild lässt sich nur überdecken), aber es
///    gehört gesagt.
/// 3. Überschriebene Bilder. Sie werden neu kodiert — pixelgenau verlustfrei,
///    aber nicht mehr in der ursprünglichen Kodierung. Wer eine deutlich
///    größere Ausgabedatei vorfindet, soll wissen, woher sie kommt.
pub fn effect_warnings(
    redactions: &[Redaction],
    padding: f64,
    report: &RedactionReport,
) -> Vec<String> {
    let mut warnings = Vec::new();
    let degenerate = redactions
        .iter()
        .filter(|r| EntryEffect::of(r.region.rect, padding) == EntryEffect::Degenerate)
        .count();

    if degenerate > 0 {
        warnings.push(format!(
            "{degenerate} von {} Schwärzungen haben nach --padding={padding} ein leeres \
             Rechteck und konnten nichts entfernen. Der Text steht unverändert in der \
             Ausgabe. Ein negatives Padding verkleinert jeden Bereich.",
            redactions.len()
        ));
    }
    let effective = redactions.len() - degenerate;
    if effective > 0 && report.removed_glyphs == 0 && report.redacted_images == 0 {
        warnings.push(format!(
            "Kein einziges Zeichen wurde aus dem Content-Stream entfernt, obwohl \
             {effective} Schwärzung(en) ein gültiges Rechteck hatten. Entweder steht \
             an diesen Stellen kein Text (etwa ein Rasterbild), oder die Bereiche \
             treffen daneben."
        ));
    }
    if report.redacted_images > 0 {
        let mut text = format!(
            "{} Bild(er) überschrieben: die Bildpunkte im Schwärzungsbereich sind \
             wirklich weg. Das Bild wird dafür neu kodiert — außerhalb des Bereichs \
             bleibt jeder Bildpunkt unverändert (verlustfrei), die Datei ist danach \
             aber nicht mehr bitgleich und wird meist deutlich größer (aus JPEG wird \
             ein Flate-Bild).",
            report.redacted_images
        );
        if report.copied_images > 0 {
            text.push_str(&format!(
                " {} Kopie(n) angelegt, weil dasselbe Bild mehrfach benutzt wird.",
                report.copied_images
            ));
        }
        warnings.push(text);
    }
    warnings
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

    fn iban_redaction() -> Redaction {
        Redaction::new(
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
        )
    }

    fn stripped_info() -> MetadataReport {
        MetadataReport {
            info_removed: true,
            ..Default::default()
        }
    }

    fn build(
        redactions: &[Redaction],
        padding: f64,
        report: &RedactionReport,
        metadata: &MetadataReport,
    ) -> (AuditLog, std::path::PathBuf) {
        let dir = tempdir();
        let input = dir.join("in.pdf");
        let output = dir.join("out.pdf");
        std::fs::write(&input, b"a").unwrap();
        std::fs::write(&output, b"b").unwrap();
        let warnings = effect_warnings(redactions, padding, report);
        let log = AuditLog::build(
            &input,
            &sha256_bytes(b"a"),
            &output,
            redactions,
            &[],
            Applied {
                padding,
                redaction: report,
                metadata,
            },
            &warnings,
        )
        .unwrap();
        (log, dir)
    }

    #[test]
    fn sha256_matches_known_value() {
        assert_eq!(
            sha256_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn audit_entries_use_zero_based_pages_like_every_other_file() {
        let report = RedactionReport {
            removed_glyphs: 4,
            drawn_rects: 1,
            ..Default::default()
        };
        let (log, dir) = build(&[iban_redaction()], 1.0, &report, &stripped_info());
        assert_eq!(log.redactions[0].page, 0);
        assert_eq!(log.redactions[0].source, "auto");
        assert!(log.redactions[0].reason.contains("iban_de"));
        assert!(log.metadata_stripped);
        assert_eq!(log.metadata.summary, vec!["/Info-Dictionary".to_string()]);

        let json = serde_json::to_string(&log).unwrap();
        assert!(json.contains("\"action\":\"blackout\""));
        std::fs::remove_dir_all(dir).ok();
    }

    /// Beide Prüfsummen stehen im Log — ein Nachweis ohne sie bezeugt nichts.
    #[test]
    fn both_checksums_are_recorded() {
        let (log, dir) = build(
            &[iban_redaction()],
            1.0,
            &RedactionReport::default(),
            &stripped_info(),
        );
        assert_eq!(log.input.sha256, sha256_bytes(b"a"));
        assert_eq!(log.output.sha256, sha256_bytes(b"b"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn metadata_stripped_reflects_the_actual_run() {
        // Eine Datei ohne Metadaten: das Log darf keine Entfernung behaupten.
        let (log, dir) = build(
            &[iban_redaction()],
            1.0,
            &RedactionReport::default(),
            &MetadataReport::default(),
        );
        assert!(!log.metadata_stripped);
        assert!(log.metadata.summary.is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_degenerate_region_is_never_counted_as_a_redaction() {
        // Der reproduzierte Fall: --padding=-100 lässt vom Rechteck nichts
        // übrig. Die Schwärzung wird übersprungen, das Log muss das sagen.
        let (log, dir) = build(
            &[iban_redaction()],
            -100.0,
            &RedactionReport::default(),
            &stripped_info(),
        );
        assert_eq!(log.effect.requested, 1);
        assert_eq!(log.effect.applied, 0);
        assert_eq!(log.effect.degenerate, 1);
        assert_eq!(log.effect.removed_glyphs, 0);
        assert_eq!(log.effect.drawn_rects, 0);
        assert_eq!(log.redactions[0].effect, EntryEffect::Degenerate);
        assert!(log.redactions[0].effective_rect.is_empty());
        assert!(
            log.warnings.iter().any(|w| w.contains("leeres")),
            "{:?}",
            log.warnings
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_run_without_a_single_removed_glyph_is_flagged() {
        let warnings = effect_warnings(&[iban_redaction()], 1.0, &RedactionReport::default());
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("Kein einziges Zeichen"),
            "{warnings:?}"
        );
    }

    #[test]
    fn an_effective_run_produces_no_warning() {
        let report = RedactionReport {
            removed_glyphs: 26,
            drawn_rects: 1,
            ..Default::default()
        };
        assert!(effect_warnings(&[iban_redaction()], 1.0, &report).is_empty());
    }

    /// Ein überschriebenes Bild ist kein Randdetail: das Bild wird neu
    /// kodiert und ist danach nicht mehr das Original.
    #[test]
    fn overwritten_images_are_reported_and_logged() {
        let report = RedactionReport {
            drawn_rects: 1,
            redacted_images: 2,
            copied_images: 1,
            ..Default::default()
        };
        let warnings = effect_warnings(&[iban_redaction()], 1.0, &report);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("2 Bild(er) überschrieben"),
            "{warnings:?}"
        );
        assert!(warnings[0].contains("verlustfrei"), "{warnings:?}");
        // Und keine Behauptung, die nicht stimmt: außerhalb der Schwärzung
        // bleibt jeder Bildpunkt gleich.
        assert!(!warnings[0].contains("verlustbehaftet"), "{warnings:?}");
        assert!(warnings[0].contains("1 Kopie(n)"), "{warnings:?}");

        let (log, dir) = build(&[iban_redaction()], 1.0, &report, &stripped_info());
        assert_eq!(log.effect.redacted_images, 2);
        assert_eq!(log.effect.copied_images, 1);
        std::fs::remove_dir_all(dir).ok();
    }

    fn tempdir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("redact-audit-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
