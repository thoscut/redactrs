//! Anwendungszustand der GUI — bewusst **ohne** egui-Typen.
//!
//! Alles, was fachlich interessant ist (Laden, Analysieren, Regionen ändern,
//! Konfliktauflösung, Export, Review-Austausch), lebt hier als gewöhnliche
//! Methode und ist damit ohne Fenster und ohne Grafikkontext testbar. Die
//! Module [`crate::app`], [`crate::sidebar`] und [`crate::viewer`] rufen
//! ausschließlich diese Methoden auf und halten selbst keinen Zustand, der
//! über einen Frame hinaus Bedeutung hätte.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use redact_booking::{BookingMatcher, CsvBookingLoader};
use redact_core::{
    resolve_conflicts, Action, BlockedRegion, BookingLoader, Extractor, MatchType, RedactError,
    Rect, Redaction, Region, Renderer, Result, ReviewFile, ReviewInput, Source, TextRun,
};
use redact_patterns::PatternMatcher;
use redact_pdf::{
    load_from_bytes, page_boxes, strip_metadata, PdfExtractor, PdfRedactor, PdfRenderer,
    RedactionReport,
};

/// A4 als Rückfallwert, wenn noch kein Dokument geladen ist.
pub const DEFAULT_PAGE_BOX: Rect = Rect {
    ll: redact_core::Point { x: 0.0, y: 0.0 },
    ur: redact_core::Point {
        x: 595.276,
        y: 841.89,
    },
};

/// Kleinster und größter erlaubter Zoomfaktor.
pub const MIN_ZOOM: f32 = 0.25;
/// Siehe [`MIN_ZOOM`].
pub const MAX_ZOOM: f32 = 4.0;

/// Farbkategorie einer Region in der Trefferliste und im Seitenbild.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionColor {
    /// Regex-Treffer (blau).
    AutoPattern,
    /// Positivlisten-Treffer (grün).
    AutoBookingPos,
    /// Negativlisten-Treffer (rot) — wird nie geschwärzt.
    AutoBookingNeg,
    /// Von Hand gezogenes Rechteck (orange).
    Manual,
}

impl RegionColor {
    /// Leitet die Farbkategorie aus der Herkunft der Region ab.
    pub fn from_source(source: &Source) -> Self {
        match source {
            Source::Pattern { .. } => RegionColor::AutoPattern,
            Source::Booking {
                match_type: MatchType::Positive,
                ..
            } => RegionColor::AutoBookingPos,
            Source::Booking {
                match_type: MatchType::Negative,
                ..
            } => RegionColor::AutoBookingNeg,
            Source::Manual { .. } => RegionColor::Manual,
        }
    }

    /// RGB-Wert für die Anzeige. Bewusst kein `egui::Color32`, damit dieses
    /// Modul frei von GUI-Abhängigkeiten bleibt.
    pub fn rgb(self) -> (u8, u8, u8) {
        match self {
            RegionColor::AutoPattern => (60, 130, 246),
            RegionColor::AutoBookingPos => (34, 168, 90),
            RegionColor::AutoBookingNeg => (220, 60, 60),
            RegionColor::Manual => (240, 150, 30),
        }
    }

    /// Kurzbezeichnung für die Legende.
    pub fn label(self) -> &'static str {
        match self {
            RegionColor::AutoPattern => "Pattern",
            RegionColor::AutoBookingPos => "Buchung positiv",
            RegionColor::AutoBookingNeg => "Buchung negativ",
            RegionColor::Manual => "Manuell",
        }
    }
}

/// Eine Region mitsamt ihrem Zustand in der Oberfläche.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotatedRegion {
    pub region: Region,
    /// Wird diese Region beim Export geschwärzt?
    ///
    /// Für Negativlisten-Treffer immer `false` — sie *blockieren* Schwärzungen,
    /// statt selbst welche zu sein. Siehe [`AppState::set_enabled`].
    pub enabled: bool,
    pub color: RegionColor,
    /// Art der Schwärzung.
    ///
    /// **Ergänzung gegenüber der Konzeptskizze**: ohne dieses Feld ginge beim
    /// Round-Trip über eine Review-Datei die `action` jedes Eintrags verloren
    /// (die CLI kann `whiteout` und `replace` schreiben), und GUI und CLI wären
    /// nicht mehr austauschbar.
    pub action: Action,
}

impl AnnotatedRegion {
    /// Erzeugt einen Eintrag mit den Vorgabewerten (Negativtreffer aus).
    pub fn new(region: Region) -> Self {
        let color = RegionColor::from_source(&region.source);
        let enabled = !region.is_blocking();
        Self {
            region,
            enabled,
            color,
            action: Action::Blackout,
        }
    }

    /// Blockiert dieser Eintrag andere Treffer (Negativliste)?
    pub fn is_blocking(&self) -> bool {
        self.region.is_blocking()
    }

    /// Beschriftung für die Trefferliste.
    pub fn label(&self) -> String {
        let text = self
            .region
            .text
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .unwrap_or("(ohne Text)");
        format!("S.{} {}", self.region.page + 1, shorten(text, 34))
    }
}

/// Kürzt einen Text auf `max` Zeichen (zeichen-, nicht byteweise).
pub fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut s: String = text.chars().take(max.saturating_sub(1)).collect();
    s.push('…');
    s
}

/// Der gesamte Zustand der Anwendung.
#[derive(Debug)]
pub struct AppState {
    pub pdf_path: Option<PathBuf>,
    pub document: Option<lopdf::Document>,
    /// MediaBox je Seite.
    pub page_boxes: Vec<Rect>,
    pub runs: Vec<TextRun>,
    pub current_page: usize,
    pub zoom: f32,
    pub regions: Vec<AnnotatedRegion>,
    pub selected_region: Option<usize>,
    pub booking_path: Option<PathBuf>,
    /// Statuszeile.
    pub status: String,
    pub warnings: Vec<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            pdf_path: None,
            document: None,
            page_boxes: Vec::new(),
            runs: Vec::new(),
            current_page: 0,
            zoom: 1.0,
            regions: Vec::new(),
            selected_region: None,
            booking_path: None,
            status: "Kein Dokument geladen".to_string(),
            warnings: Vec::new(),
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    // ---------------------------------------------------------------- Laden

    /// Lädt ein PDF von der Platte, extrahiert die Text-Runs und setzt Seite
    /// und Auswahl zurück.
    pub fn load_document(&mut self, path: &Path) -> Result<()> {
        let bytes = std::fs::read(path)?;
        self.load_bytes(&bytes, Some(path.to_path_buf()))
            .map_err(|e| match e {
                RedactError::Pdf(msg) => RedactError::Pdf(format!("{}: {msg}", path.display())),
                other => other,
            })
    }

    /// Wie [`AppState::load_document`], aber aus dem Speicher. Es werden keine
    /// temporären Dateien angelegt.
    pub fn load_bytes(&mut self, bytes: &[u8], path: Option<PathBuf>) -> Result<()> {
        let doc = load_from_bytes(bytes)?;
        let runs = PdfExtractor::new().extract(&doc)?;
        self.page_boxes = page_boxes(&doc);
        self.runs = runs;
        self.document = Some(doc);
        self.pdf_path = path;
        self.current_page = 0;
        self.selected_region = None;
        self.regions.clear();
        self.warnings.clear();
        self.status = format!(
            "{} Seite(n), {} Textabschnitte geladen",
            self.page_boxes.len(),
            self.runs.len()
        );
        Ok(())
    }

    pub fn is_loaded(&self) -> bool {
        self.document.is_some()
    }

    pub fn page_count(&self) -> usize {
        self.page_boxes.len()
    }

    /// MediaBox der aktuellen Seite (A4, solange nichts geladen ist).
    pub fn current_page_box(&self) -> Rect {
        self.page_boxes
            .get(self.current_page)
            .copied()
            .unwrap_or(DEFAULT_PAGE_BOX)
    }

    /// Springt auf eine Seite; Werte außerhalb des Dokuments werden geklemmt.
    pub fn set_page(&mut self, page: usize) {
        let last = self.page_count().saturating_sub(1);
        self.current_page = page.min(last);
    }

    pub fn next_page(&mut self) {
        if self.current_page + 1 < self.page_count() {
            self.current_page += 1;
        }
    }

    pub fn prev_page(&mut self) {
        self.current_page = self.current_page.saturating_sub(1);
    }

    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    }

    /// Text-Runs der angegebenen Seite.
    pub fn runs_on_page(&self, page: usize) -> Vec<&TextRun> {
        self.runs.iter().filter(|r| r.page == page).collect()
    }

    /// Indizes aller Regionen der angegebenen Seite (in Anlagereihenfolge).
    pub fn regions_on_page(&self, page: usize) -> Vec<usize> {
        self.regions
            .iter()
            .enumerate()
            .filter(|(_, a)| a.region.page == page)
            .map(|(i, _)| i)
            .collect()
    }

    pub fn selected(&self) -> Option<&AnnotatedRegion> {
        self.selected_region.and_then(|i| self.regions.get(i))
    }

    // -------------------------------------------------------------- Analyse

    /// Führt Pattern- und (optional) Buchungslisten-Analyse über die
    /// extrahierten Text-Runs aus.
    ///
    /// Bereits von Hand gezogene Regionen bleiben erhalten — eine erneute
    /// Analyse darf Nutzerarbeit nicht wegwerfen. Alle automatisch gefundenen
    /// Regionen werden dagegen ersetzt.
    ///
    /// Gibt die Gesamtzahl der Regionen zurück.
    pub fn analyze(&mut self, pattern_ids: &[String], booking: Option<&Path>) -> Result<usize> {
        let matcher = PatternMatcher::new(pattern_ids)?;
        let mut found = matcher.find_matches(&self.runs)?;

        if let Some(path) = booking {
            let entries = CsvBookingLoader.load(path)?;
            let booking_matcher = BookingMatcher::new(entries)?;
            found.extend(booking_matcher.find_matches(&self.runs)?);
            self.booking_path = Some(path.to_path_buf());
        }

        let manual: Vec<AnnotatedRegion> = self
            .regions
            .iter()
            .filter(|a| matches!(a.region.source, Source::Manual { .. }))
            .cloned()
            .collect();

        self.regions = found.into_iter().map(AnnotatedRegion::new).collect();
        self.regions.extend(manual);
        self.selected_region = None;

        let total = self.regions.len();
        let blocked = self.regions.iter().filter(|a| a.is_blocking()).count();
        self.status = format!("Analyse: {total} Treffer ({blocked} aus der Negativliste)");
        Ok(total)
    }

    // ------------------------------------------------------- Regionen ändern

    /// Legt eine manuelle Region an, aktiviert sie und wählt sie aus.
    /// Gibt den Index der neuen Region zurück.
    pub fn add_manual_region(&mut self, page: usize, rect: Rect, reason: impl Into<String>) -> usize {
        let region = Region::new(
            page,
            rect,
            None,
            Source::Manual {
                reason: reason.into(),
            },
        );
        self.regions.push(AnnotatedRegion::new(region));
        let index = self.regions.len() - 1;
        self.selected_region = Some(index);
        self.status = format!("Manuelle Region auf Seite {} angelegt", page + 1);
        index
    }

    /// Löscht die ausgewählte Region. `false`, wenn nichts ausgewählt war.
    pub fn delete_selected(&mut self) -> bool {
        let Some(index) = self.selected_region else {
            return false;
        };
        if index >= self.regions.len() {
            self.selected_region = None;
            return false;
        }
        self.regions.remove(index);
        self.selected_region = None;
        self.status = "Region gelöscht".to_string();
        true
    }

    /// Verschiebt die ausgewählte Region um `dx`/`dy` im PDF-User-Space
    /// (Y zeigt nach oben). `false`, wenn nichts ausgewählt war.
    pub fn move_selected(&mut self, dx: f64, dy: f64) -> bool {
        let Some(index) = self.selected_region else {
            return false;
        };
        let Some(entry) = self.regions.get_mut(index) else {
            return false;
        };
        let r = entry.region.rect;
        entry.region.rect = Rect::new(r.ll.x + dx, r.ll.y + dy, r.ur.x + dx, r.ur.y + dy);
        true
    }

    /// Ändert den Aktivzustand einer Region.
    ///
    /// Eine blockierende (Negativlisten-)Region lässt sich **nicht**
    /// einschalten — das würde die Semantik der Negativliste aushebeln. In dem
    /// Fall wird `false` zurückgegeben und nichts geändert.
    pub fn set_enabled(&mut self, index: usize, enabled: bool) -> bool {
        let Some(entry) = self.regions.get_mut(index) else {
            return false;
        };
        if enabled && entry.is_blocking() {
            return false;
        }
        entry.enabled = enabled;
        true
    }

    /// Kippt den Aktivzustand. Gibt zurück, ob die Änderung angenommen wurde.
    pub fn toggle_enabled(&mut self, index: usize) -> bool {
        let Some(entry) = self.regions.get(index) else {
            return false;
        };
        let want = !entry.enabled;
        self.set_enabled(index, want)
    }

    /// Setzt die Schwärzungsart einer Region.
    pub fn set_action(&mut self, index: usize, action: Action) -> bool {
        match self.regions.get_mut(index) {
            Some(entry) => {
                entry.action = action;
                true
            }
            None => false,
        }
    }

    // --------------------------------------------------- Konfliktauflösung

    /// Regionen, die in die Konfliktauflösung gehen.
    ///
    /// Das sind alle aktivierten Treffer **plus** sämtliche Negativlisten-
    /// Treffer. Letztere sind nie `enabled` (sie werden ja nicht geschwärzt),
    /// müssen aber trotzdem als Blocker mitgegeben werden.
    fn conflict_input(&self) -> Vec<Region> {
        self.regions
            .iter()
            .filter(|a| a.is_blocking() || a.enabled)
            .map(|a| a.region.clone())
            .collect()
    }

    /// Ergebnis der Konfliktauflösung nach [`redact_core::resolve_conflicts`].
    pub fn resolution(&self) -> redact_core::Resolution {
        resolve_conflicts(self.conflict_input())
    }

    /// Durch die Negativliste verhinderte Treffer (fürs Audit-Log).
    pub fn blocked_regions(&self) -> Vec<BlockedRegion> {
        self.resolution().blocked
    }

    /// Die tatsächlich auszuführenden Schwärzungen.
    ///
    /// Negativlisten-Treffer fallen heraus, ebenso alles, was von ihnen zu
    /// mindestens 50 % überdeckt wird. Manuelle Regionen überstimmen die
    /// Negativliste (das entscheidet `resolve_conflicts`).
    pub fn enabled_redactions(&self) -> Vec<Redaction> {
        self.resolution()
            .redact
            .into_iter()
            .map(|region| {
                let action = self
                    .regions
                    .iter()
                    .find(|a| a.region == region)
                    .map(|a| a.action.clone())
                    .unwrap_or_default();
                Redaction::new(region, action)
            })
            .collect()
    }

    // ---------------------------------------------------------------- Export

    /// Schwärzt eine Kopie des Dokuments, entfernt die Metadaten und schreibt
    /// das Ergebnis nach `out`.
    ///
    /// Die Reihenfolge (schwärzen → Metadaten strippen → schreiben) ist exakt
    /// dieselbe wie in der CLI-Pipeline; bei gleicher Regionenmenge entsteht
    /// dieselbe Datei.
    ///
    /// Ist `audit` gesetzt, wird zusätzlich ein JSON-Audit-Log geschrieben.
    pub fn export(&self, out: &Path, audit: Option<&Path>) -> Result<RedactionReport> {
        let doc = self
            .document
            .as_ref()
            .ok_or_else(|| RedactError::Pdf("Kein Dokument geladen".into()))?;

        let redactions = self.enabled_redactions();
        let mut copy = doc.clone();
        let report = PdfRedactor::new().apply_with_report(&mut copy, &redactions)?;
        strip_metadata(&mut copy);
        PdfRenderer::new().render(&copy, out)?;

        if let Some(audit_path) = audit {
            let log = build_audit_log(
                self.pdf_path.as_deref(),
                out,
                &redactions,
                &self.blocked_regions(),
                &report.warnings,
                unix_seconds_now(),
            );
            let json = serde_json::to_string_pretty(&log)?;
            std::fs::write(audit_path, json)?;
        }

        Ok(report)
    }

    // ---------------------------------------------------------------- Review

    /// Schreibt den aktuellen Zustand in eine Review-Datei.
    ///
    /// **Einschränkung**: `input.sha256` bleibt leer — dieses Crate hat bewusst
    /// keine `sha2`-Abhängigkeit. Die CLI füllt das Feld; die GUI liest es und
    /// gibt es beim Speichern nicht weiter (sie kennt es nicht).
    pub fn to_review_file(&self) -> ReviewFile {
        let input = ReviewInput {
            path: self
                .pdf_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            sha256: String::new(),
            pages: self.page_count(),
        };
        let regions = self.regions.iter().map(|a| a.region.clone()).collect();
        let mut file = ReviewFile::new(input, regions, self.blocked_regions());
        for (item, entry) in file.items.iter_mut().zip(self.regions.iter()) {
            item.enabled = entry.enabled;
            item.action = entry.action.clone();
        }
        file
    }

    /// Übernimmt eine Review-Datei (z.B. aus `redact-rs --review-out`).
    pub fn apply_review_file(&mut self, review: ReviewFile) {
        self.regions = review
            .items
            .into_iter()
            .map(|item| {
                let mut entry = AnnotatedRegion::new(item.region);
                // Ein Negativlisten-Treffer bleibt aus, egal was in der Datei steht.
                entry.enabled = item.enabled && !entry.is_blocking();
                entry.action = item.action;
                entry
            })
            .collect();
        self.selected_region = None;
        self.status = format!("Review übernommen: {} Einträge", self.regions.len());
    }
}

// ------------------------------------------------------------------- Audit

/// Baut das Audit-Log als JSON-Wert.
///
/// Die Struktur ist deckungsgleich mit dem, was `redact-cli` schreibt — mit
/// zwei bewussten Unterschieden:
///
/// * `input.sha256` und `output.sha256` bleiben **leer**, weil dieses Crate
///   keine `sha2`-Abhängigkeit hat.
/// * Zusätzlich zum RFC-3339-Zeitstempel (UTC, Sekundengenauigkeit) wird
///   `timestamp_unix` mit den Sekunden seit der Epoche ausgegeben. Dieses Crate
///   hat kein `chrono`; der Zeitstempel wird aus [`SystemTime`] selbst
///   formatiert (siehe [`format_rfc3339_utc`]), und der reine Zahlenwert macht
///   das Log unabhängig von der Formatierung nachprüfbar.
pub fn build_audit_log(
    input: Option<&Path>,
    output: &Path,
    redactions: &[Redaction],
    blocked: &[BlockedRegion],
    warnings: &[String],
    unix_secs: i64,
) -> serde_json::Value {
    let entries: Vec<serde_json::Value> = redactions
        .iter()
        .map(|r| {
            serde_json::json!({
                // 1-basiert, damit das Log ohne Umrechnung lesbar ist.
                "page": r.region.page + 1,
                "rect": r.region.rect,
                "action": r.action,
                "reason": r.reason,
                "source": r.region.source_kind(),
            })
        })
        .collect();

    let blocked_entries: Vec<serde_json::Value> = blocked
        .iter()
        .map(|b| {
            serde_json::json!({
                "page": b.page + 1,
                "pattern": b.pattern,
                "booking_id": b.booking_id,
                "blocked_reason": b.blocked_reason,
            })
        })
        .collect();

    serde_json::json!({
        "timestamp": format_rfc3339_utc(unix_secs),
        "timestamp_unix": unix_secs,
        "tool": {
            "name": "redact-rs",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "input": {
            "path": input.map(|p| p.display().to_string()).unwrap_or_default(),
            "sha256": "",
        },
        "output": {
            "path": output.display().to_string(),
            "sha256": "",
        },
        "redactions": entries,
        "blocked_by_negative_list": blocked_entries,
        "metadata_stripped": true,
        "warnings": warnings,
    })
}

/// Sekunden seit der Unix-Epoche (negativ für Zeiten davor).
pub fn unix_seconds_now() -> i64 {
    let now = SystemTime::now();
    match now.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    }
}

/// Formatiert Sekunden seit der Epoche als UTC-Zeitstempel nach RFC 3339
/// (`YYYY-MM-DDTHH:MM:SSZ`).
///
/// Eigenimplementierung, weil dieses Crate kein `chrono` hat. Schaltjahre
/// werden nach dem gregorianischen Kalender behandelt; Schaltsekunden gibt es
/// in der Unix-Zeit nicht.
pub fn format_rfc3339_utc(unix_secs: i64) -> String {
    let days = unix_secs.div_euclid(86_400);
    let secs = unix_secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let (hour, minute, second) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Tage seit 1970-01-01 → (Jahr, Monat, Tag) im gregorianischen Kalender.
///
/// Algorithmus nach Howard Hinnant, „chrono-Compatible Low-Level Date
/// Algorithms“ — abhängigkeitsfrei und über den ganzen i64-Bereich korrekt.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use redact_core::{MatchType, Point};

    fn pattern_region(page: usize, rect: Rect) -> Region {
        Region::new(
            page,
            rect,
            Some("DE89 3704 0044 0532 0130 00".into()),
            Source::Pattern {
                pattern_id: "iban_de".into(),
                confidence: 0.99,
            },
        )
    }

    fn negative_region(page: usize, rect: Rect) -> Region {
        Region::new(
            page,
            rect,
            Some("Max Mustermann".into()),
            Source::Booking {
                booking_id: "b003".into(),
                match_type: MatchType::Negative,
            },
        )
    }

    fn loaded_state() -> AppState {
        let mut state = AppState::new();
        state
            .load_bytes(
                &redact_pdf::testing::demo_statement(),
                Some(PathBuf::from("demo.pdf")),
            )
            .expect("Demo-PDF ladbar");
        state
    }

    #[test]
    fn load_bytes_fills_pages_runs_and_resets_selection() {
        let mut state = AppState::new();
        state.current_page = 5;
        state.selected_region = Some(3);

        state
            .load_bytes(&redact_pdf::testing::demo_statement(), None)
            .unwrap();

        assert_eq!(state.page_count(), 2);
        assert_eq!(state.page_boxes[0], Rect::new(0.0, 0.0, 595.0, 842.0));
        assert!(!state.runs.is_empty());
        assert_eq!(state.current_page, 0);
        assert_eq!(state.selected_region, None);
        assert!(state.is_loaded());
    }

    #[test]
    fn analyze_assigns_colors_and_disables_negative_hits() {
        let mut state = loaded_state();
        // Nur die IBAN-Patterns, damit der Test nicht an anderen Treffern hängt.
        state
            .analyze(&["iban_de".to_string()], None)
            .expect("Analyse läuft");
        assert!(!state.regions.is_empty());
        assert!(state
            .regions
            .iter()
            .all(|a| a.color == RegionColor::AutoPattern && a.enabled));

        // Negativtreffer künstlich ergänzen und Farbzuordnung prüfen.
        state
            .regions
            .push(AnnotatedRegion::new(negative_region(0, Rect::new(
                0.0, 0.0, 10.0, 10.0,
            ))));
        let last = state.regions.last().unwrap();
        assert_eq!(last.color, RegionColor::AutoBookingNeg);
        assert!(!last.enabled);
    }

    #[test]
    fn analyze_keeps_manual_regions() {
        let mut state = loaded_state();
        state.add_manual_region(0, Rect::new(10.0, 10.0, 40.0, 20.0), "Gehalt");
        state.analyze(&["iban_de".to_string()], None).unwrap();
        assert_eq!(
            state
                .regions
                .iter()
                .filter(|a| a.color == RegionColor::Manual)
                .count(),
            1
        );
    }

    #[test]
    fn add_move_and_delete_manual_region() {
        let mut state = AppState::new();
        let index = state.add_manual_region(1, Rect::new(10.0, 20.0, 50.0, 40.0), "Gehalt");
        assert_eq!(index, 0);
        assert_eq!(state.selected_region, Some(0));
        assert!(state.regions[0].enabled);
        assert_eq!(state.regions[0].color, RegionColor::Manual);

        assert!(state.move_selected(5.0, -3.0));
        assert_eq!(state.regions[0].region.rect.ll, Point::new(15.0, 17.0));
        assert_eq!(state.regions[0].region.rect.ur, Point::new(55.0, 37.0));

        assert!(state.delete_selected());
        assert!(state.regions.is_empty());
        assert_eq!(state.selected_region, None);
        // Ohne Auswahl passiert nichts mehr.
        assert!(!state.delete_selected());
        assert!(!state.move_selected(1.0, 1.0));
    }

    #[test]
    fn negative_region_cannot_be_enabled() {
        let mut state = AppState::new();
        state
            .regions
            .push(AnnotatedRegion::new(negative_region(0, Rect::new(
                0.0, 0.0, 10.0, 10.0,
            ))));
        assert!(!state.regions[0].enabled);
        assert!(!state.set_enabled(0, true));
        assert!(!state.regions[0].enabled);
        assert!(!state.toggle_enabled(0));
        assert!(!state.regions[0].enabled);
        // Ausschalten ist erlaubt (ändert nichts) …
        assert!(state.set_enabled(0, false));
        // … und ein unbekannter Index scheitert.
        assert!(!state.set_enabled(99, false));
    }

    #[test]
    fn pattern_region_toggles_normally() {
        let mut state = AppState::new();
        state
            .regions
            .push(AnnotatedRegion::new(pattern_region(0, Rect::new(
                0.0, 0.0, 10.0, 10.0,
            ))));
        assert!(state.regions[0].enabled);
        assert!(state.toggle_enabled(0));
        assert!(!state.regions[0].enabled);
        assert!(state.toggle_enabled(0));
        assert!(state.regions[0].enabled);
    }

    #[test]
    fn enabled_redactions_respect_negative_list() {
        let mut state = AppState::new();
        state
            .regions
            .push(AnnotatedRegion::new(pattern_region(0, Rect::new(
                10.0, 10.0, 50.0, 20.0,
            ))));
        state
            .regions
            .push(AnnotatedRegion::new(negative_region(0, Rect::new(
                0.0, 0.0, 100.0, 30.0,
            ))));

        // Der Pattern-Treffer wird zu 100 % von der Negativregion überdeckt.
        assert!(state.enabled_redactions().is_empty());
        assert_eq!(state.blocked_regions().len(), 1);
        assert_eq!(state.blocked_regions()[0].booking_id, "b003");

        // Eine manuelle Region an derselben Stelle überstimmt die Negativliste.
        state.add_manual_region(0, Rect::new(10.0, 10.0, 50.0, 20.0), "bewusst");
        let redactions = state.enabled_redactions();
        assert_eq!(redactions.len(), 1);
        assert!(matches!(
            redactions[0].region.source,
            Source::Manual { .. }
        ));
    }

    #[test]
    fn disabled_regions_are_not_exported() {
        let mut state = AppState::new();
        state
            .regions
            .push(AnnotatedRegion::new(pattern_region(0, Rect::new(
                10.0, 10.0, 50.0, 20.0,
            ))));
        assert_eq!(state.enabled_redactions().len(), 1);
        assert!(state.set_enabled(0, false));
        assert!(state.enabled_redactions().is_empty());
    }

    #[test]
    fn action_is_carried_into_redactions() {
        let mut state = AppState::new();
        state
            .regions
            .push(AnnotatedRegion::new(pattern_region(0, Rect::new(
                10.0, 10.0, 50.0, 20.0,
            ))));
        assert!(state.set_action(0, Action::Replace("[IBAN]".into())));
        let redactions = state.enabled_redactions();
        assert_eq!(redactions[0].action, Action::Replace("[IBAN]".into()));
    }

    #[test]
    fn export_removes_the_text_from_the_pdf() {
        let mut state = loaded_state();
        state.analyze(&["iban_de".to_string()], None).unwrap();
        assert!(!state.regions.is_empty());

        let dir = temp_dir("export");
        let out = dir.join("out.pdf");
        let audit = dir.join("audit.json");
        let report = state.export(&out, Some(&audit)).expect("Export läuft");
        assert!(report.removed_glyphs > 0);
        assert!(report.drawn_rects > 0);

        // Ergebnis erneut extrahieren: die IBAN darf nicht mehr auftauchen.
        let bytes = std::fs::read(&out).unwrap();
        let doc = redact_pdf::load_from_bytes(&bytes).unwrap();
        let runs = PdfExtractor::new().extract(&doc).unwrap();
        let text: String = runs.iter().map(|r| r.text.as_str()).collect::<Vec<_>>().join("\n");
        assert!(!text.contains("DE89"), "IBAN steht noch im Dokument: {text}");
        assert!(!text.contains("DE02"));
        // Nicht getroffener Text bleibt erhalten.
        assert!(text.contains("Musterbank"));

        // Audit-Log ist gültiges JSON in der vereinbarten Form.
        let log: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&audit).unwrap()).unwrap();
        assert_eq!(log["metadata_stripped"], serde_json::json!(true));
        assert!(log["redactions"].as_array().unwrap().len() >= 1);
        assert_eq!(log["redactions"][0]["page"], serde_json::json!(1));
        assert_eq!(log["input"]["sha256"], serde_json::json!(""));
        assert!(log["timestamp"].as_str().unwrap().ends_with('Z'));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn export_without_document_fails() {
        let state = AppState::new();
        let dir = temp_dir("noexport");
        assert!(state.export(&dir.join("x.pdf"), None).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn export_strips_metadata() {
        let state = loaded_state();
        let dir = temp_dir("meta");
        let out = dir.join("out.pdf");
        state.export(&out, None).unwrap();
        let doc = redact_pdf::load_from_bytes(&std::fs::read(&out).unwrap()).unwrap();
        assert!(doc.trailer.get(b"Info").is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn review_file_roundtrip_preserves_state() {
        let mut state = loaded_state();
        state.analyze(&["iban_de".to_string()], None).unwrap();
        state.add_manual_region(1, Rect::new(70.0, 700.0, 200.0, 715.0), "Adresse");
        state.set_enabled(0, false);
        state.set_action(1, Action::Whiteout);

        let json = state.to_review_file().to_json().unwrap();
        let parsed = ReviewFile::from_json(&json).unwrap();

        let mut restored = AppState::new();
        restored.apply_review_file(parsed);

        assert_eq!(restored.regions.len(), state.regions.len());
        for (a, b) in restored.regions.iter().zip(state.regions.iter()) {
            assert_eq!(a.region, b.region);
            assert_eq!(a.enabled, b.enabled);
            assert_eq!(a.action, b.action);
            assert_eq!(a.color, b.color);
        }
        assert_eq!(restored.to_review_file().items, state.to_review_file().items);
    }

    #[test]
    fn review_file_never_enables_a_negative_hit() {
        let mut state = AppState::new();
        state
            .regions
            .push(AnnotatedRegion::new(negative_region(0, Rect::new(
                0.0, 0.0, 10.0, 10.0,
            ))));
        let mut review = state.to_review_file();
        // Von Hand manipulierte Datei: `enabled` steht auf true.
        review.items[0].enabled = true;
        state.apply_review_file(review);
        assert!(!state.regions[0].enabled);
        assert!(state.enabled_redactions().is_empty());
    }

    #[test]
    fn review_input_has_pages_and_empty_sha() {
        let state = loaded_state();
        let review = state.to_review_file();
        assert_eq!(review.input.pages, 2);
        assert_eq!(review.input.sha256, "");
        assert_eq!(review.input.path, "demo.pdf");
    }

    #[test]
    fn page_navigation_is_clamped() {
        let mut state = loaded_state();
        state.next_page();
        assert_eq!(state.current_page, 1);
        state.next_page();
        assert_eq!(state.current_page, 1);
        state.prev_page();
        assert_eq!(state.current_page, 0);
        state.prev_page();
        assert_eq!(state.current_page, 0);
        state.set_page(99);
        assert_eq!(state.current_page, 1);
    }

    #[test]
    fn zoom_is_clamped() {
        let mut state = AppState::new();
        state.set_zoom(100.0);
        assert_eq!(state.zoom, MAX_ZOOM);
        state.set_zoom(0.0);
        assert_eq!(state.zoom, MIN_ZOOM);
    }

    #[test]
    fn timestamp_formatting_matches_known_values() {
        assert_eq!(format_rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_rfc3339_utc(1_700_000_000), "2023-11-14T22:13:20Z");
        // Schaltjahr: 2024-02-29.
        assert_eq!(format_rfc3339_utc(1_709_164_800), "2024-02-29T00:00:00Z");
        // Vor der Epoche.
        assert_eq!(format_rfc3339_utc(-1), "1969-12-31T23:59:59Z");
    }

    #[test]
    fn audit_log_shape_matches_cli_format() {
        let redaction = Redaction::new(pattern_region(0, Rect::new(1.0, 2.0, 3.0, 4.0)), Action::Blackout);
        let blocked = vec![BlockedRegion {
            page: 1,
            rect: Rect::new(0.0, 0.0, 5.0, 5.0),
            pattern: "Max Mustermann".into(),
            booking_id: "b003".into(),
            blocked_reason: Some("pattern: iban_de".into()),
        }];
        let log = build_audit_log(
            Some(Path::new("in.pdf")),
            Path::new("out.pdf"),
            &[redaction],
            &blocked,
            &["Bild nur überdeckt".to_string()],
            1_700_000_000,
        );

        assert_eq!(log["timestamp"], serde_json::json!("2023-11-14T22:13:20Z"));
        assert_eq!(log["timestamp_unix"], serde_json::json!(1_700_000_000i64));
        assert_eq!(log["tool"]["name"], serde_json::json!("redact-rs"));
        assert_eq!(log["input"]["path"], serde_json::json!("in.pdf"));
        assert_eq!(log["input"]["sha256"], serde_json::json!(""));
        assert_eq!(log["output"]["path"], serde_json::json!("out.pdf"));
        assert_eq!(log["redactions"][0]["page"], serde_json::json!(1));
        assert_eq!(log["redactions"][0]["action"], serde_json::json!("blackout"));
        assert_eq!(log["redactions"][0]["source"], serde_json::json!("auto"));
        assert_eq!(
            log["redactions"][0]["rect"],
            serde_json::json!({"ll": {"x": 1.0, "y": 2.0}, "ur": {"x": 3.0, "y": 4.0}})
        );
        assert_eq!(
            log["blocked_by_negative_list"][0]["booking_id"],
            serde_json::json!("b003")
        );
        assert_eq!(
            log["blocked_by_negative_list"][0]["page"],
            serde_json::json!(2)
        );
        assert_eq!(log["metadata_stripped"], serde_json::json!(true));
        assert_eq!(log["warnings"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn shorten_keeps_short_text_and_truncates_long_text() {
        assert_eq!(shorten("kurz", 10), "kurz");
        assert_eq!(shorten("äöüäöüäöü", 4), "äöü…");
    }

    #[test]
    fn region_color_covers_every_source() {
        assert_eq!(
            RegionColor::from_source(&Source::Pattern {
                pattern_id: "x".into(),
                confidence: 1.0
            }),
            RegionColor::AutoPattern
        );
        assert_eq!(
            RegionColor::from_source(&Source::Booking {
                booking_id: "b".into(),
                match_type: MatchType::Positive
            }),
            RegionColor::AutoBookingPos
        );
        assert_eq!(
            RegionColor::from_source(&Source::Booking {
                booking_id: "b".into(),
                match_type: MatchType::Negative
            }),
            RegionColor::AutoBookingNeg
        );
        assert_eq!(
            RegionColor::from_source(&Source::Manual { reason: "r".into() }),
            RegionColor::Manual
        );
        // Jede Farbe hat eine Beschriftung und einen RGB-Wert.
        for color in [
            RegionColor::AutoPattern,
            RegionColor::AutoBookingPos,
            RegionColor::AutoBookingNeg,
            RegionColor::Manual,
        ] {
            assert!(!color.label().is_empty());
            let (r, g, b) = color.rgb();
            assert!(r as u32 + g as u32 + b as u32 > 0);
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "redact-gui-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
