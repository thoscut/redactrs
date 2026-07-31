//! Anwendungszustand der GUI — bewusst **ohne** egui-Typen.
//!
//! Alles, was fachlich interessant ist (Laden, Analysieren, Regionen ändern,
//! Konfliktauflösung, Export, Review-Austausch), lebt hier als gewöhnliche
//! Methode und ist damit ohne Fenster und ohne Grafikkontext testbar. Die
//! Module [`crate::app`], [`crate::sidebar`] und [`crate::viewer`] rufen
//! ausschließlich diese Methoden auf und halten selbst keinen Zustand, der
//! über einen Frame hinaus Bedeutung hätte.
//!
//! Einzige Ausnahme von „ohne egui“ ist [`crate::viewer::PageView`] — reine
//! Geometrie (MediaBox plus `/Rotate`), kein Fenster, keine Grafik. Sie liegt
//! im Sichtmodul, weil sie dort gebraucht und geprüft wird.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use redact_booking::{BookingMatcher, CsvBookingLoader};
use redact_core::{
    output_path_with_suffix, resolve_conflicts, sibling_path, Action, BlockedRegion, BookingLoader,
    Extractor, MatchType, Rect, RedactError, Redaction, Region, Renderer, Result, ReviewFile,
    ReviewInput, Source, TextRun, AUDIT_SUFFIX, DEFAULT_OUTPUT_SUFFIX, REVIEW_SUFFIX,
};
use redact_patterns::PatternMatcher;
use redact_pdf::{
    load_from_bytes, page_boxes, strip_metadata, PdfExtractor, PdfRedactor, PdfRenderer,
    RedactionReport,
};

use crate::viewer::{normalize_rotation, PageView};

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

/// Kategorie einer Region in der Trefferliste und im Seitenbild.
///
/// Die Beschriftungen sagen, **was mit dem Treffer passiert**, nicht woher er
/// technisch stammt. „Buchung negativ“ hieß früher die dritte Kategorie — auf
/// einem Kontoauszug liest sich „negativ“ wie eine Soll-Buchung, gemeint war
/// aber das genaue Gegenteil: dieser Text ist geschützt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionColor {
    /// Regex-Treffer (blau).
    AutoPattern,
    /// Positivlisten-Treffer (grün) — soll geschwärzt werden.
    AutoBookingPos,
    /// Negativlisten-Treffer (rot) — wird nie geschwärzt.
    AutoBookingNeg,
    /// Von Hand gezogenes Rechteck (orange).
    Manual,
}

/// Alle Kategorien in fester Reihenfolge (Legende, Tests).
pub const REGION_COLORS: [RegionColor; 4] = [
    RegionColor::AutoPattern,
    RegionColor::AutoBookingPos,
    RegionColor::AutoBookingNeg,
    RegionColor::Manual,
];

impl RegionColor {
    /// Leitet die Kategorie aus der Herkunft der Region ab.
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
    ///
    /// Das Orange der manuellen Regionen war früher `(240, 150, 30)` und kam im
    /// hellen Thema auf 2,18:1 gegen den Bereichshintergrund und 2,31:1 gegen
    /// das weiße Blatt — unter den 3:1, die für grafische Elemente das Minimum
    /// sind. Das Grün lag mit 2,90:1 ebenfalls darunter. Beide sind jetzt
    /// dunkler; alle vier Töne erreichen gegen weißes Blatt, hellen und dunklen
    /// Bereichshintergrund mindestens 3:1 (siehe Test unten).
    pub fn rgb(self) -> (u8, u8, u8) {
        match self {
            RegionColor::AutoPattern => (60, 130, 246),
            RegionColor::AutoBookingPos => (21, 128, 61),
            RegionColor::AutoBookingNeg => (220, 60, 60),
            RegionColor::Manual => (176, 88, 0),
        }
    }

    /// Zeichen vor dem Treffer.
    ///
    /// Vier **verschiedene** Zeichen, nicht nur vier Farben: Grün und Rot
    /// unterscheiden sich bei einer Rot-Grün-Sehschwäche kaum, die Kategorie
    /// muss aber auch dann ablesbar bleiben. Alle vier stammen aus demselben
    /// Unicode-Block „Geometric Shapes“ wie das bisher schon benutzte `●`.
    pub fn marker(self) -> &'static str {
        match self {
            RegionColor::AutoPattern => "●",
            RegionColor::AutoBookingPos => "◆",
            RegionColor::AutoBookingNeg => "■",
            RegionColor::Manual => "▲",
        }
    }

    /// Kurzbezeichnung für die Legende — ergebnisbezogen, ohne Fachjargon.
    pub fn label(self) -> &'static str {
        match self {
            RegionColor::AutoPattern => "Muster gefunden",
            RegionColor::AutoBookingPos => "Liste: schwärzen",
            RegionColor::AutoBookingNeg => "Liste: schützen",
            RegionColor::Manual => "selbst gezeichnet",
        }
    }
}

// ------------------------------------------------------- Klartextbeschreibung

/// Beschreibung eines Treffers in der Sprache der Zielgruppe.
///
/// Die alte Fassung zeigte `pattern: konto_nr (confidence 0.40)`: ein englisches
/// Schlüsselwort, eine interne ID und eine Zahl, die niemand ohne Kenntnis der
/// Erkennungsregeln deuten kann. Stattdessen wird jetzt die `description` des
/// Musters gezeigt („Kontonummer (Heuristik, 6–10 Ziffern)“).
pub fn plain_description(source: &Source) -> String {
    match source {
        Source::Pattern { pattern_id, .. } => pattern_description(pattern_id),
        Source::Booking {
            match_type: MatchType::Positive,
            ..
        } => "Aus Ihrer Liste: soll geschwärzt werden".to_string(),
        Source::Booking {
            match_type: MatchType::Negative,
            ..
        } => "Aus Ihrer Liste: darf nicht geschwärzt werden".to_string(),
        Source::Manual { reason } => match reason.trim() {
            "" => "Selbst gezeichnet".to_string(),
            other => format!("Selbst gezeichnet: {other}"),
        },
    }
}

/// Beschreibung eines eingebauten Musters; sonst wenigstens dessen Namen.
fn pattern_description(id: &str) -> String {
    static TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        redact_patterns::builtin_patterns()
            .into_iter()
            .filter(|p| !p.description.trim().is_empty())
            .map(|p| (p.id, p.description))
            .collect()
    });
    match table.get(id) {
        Some(description) => description.clone(),
        // Aus einer Konfigurationsdatei nachgeladene Muster kennt die
        // Oberfläche nicht — dann bleibt nur die ID.
        None => format!("Muster „{id}“"),
    }
}

// ------------------------------------------------------------ Trefferbilanz

/// Was am Ende mit einem Treffer geschieht.
///
/// `resolve_conflicts` verwirft blockierte und doppelte Treffer. Ohne diese
/// Unterscheidung zeigte die Seitenleiste sie weiter angehakt und gefüllt — der
/// Eindruck, sie würden geschwärzt, war schlicht falsch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitOutcome {
    /// Wird geschwärzt.
    Redacted,
    /// Schützt Text (Negativliste) und wird selbst nie geschwärzt.
    Protecting,
    /// Vom Nutzer abgewählt.
    Disabled,
    /// Durch einen Eintrag der Negativliste verhindert.
    Blocked,
    /// Doppelt bzw. vollständig in einem anderen Treffer enthalten.
    Duplicate,
}

impl HitOutcome {
    /// Wird dieser Treffer beim Export tatsächlich geschwärzt?
    pub fn is_redacted(self) -> bool {
        self == HitOutcome::Redacted
    }

    /// Kurzer Zusatz hinter der Trefferbeschriftung.
    ///
    /// Für geschützte Einträge steht hier ein **Wort** statt des früheren
    /// Durchstreichens: durchgestrichen liest sich wie „gestrichen, entfernt“ —
    /// gemeint ist das Gegenteil.
    pub fn note(self) -> &'static str {
        match self {
            HitOutcome::Redacted => "",
            HitOutcome::Protecting => "geschützt",
            HitOutcome::Disabled => "abgewählt",
            HitOutcome::Blocked => "geschützt durch Ihre Liste",
            HitOutcome::Duplicate => "doppelt",
        }
    }
}

/// Ergebnis einer Konfliktauflösung, aufbereitet für die Anzeige.
#[derive(Debug, Clone, PartialEq)]
pub struct HitSummary {
    /// Je Eintrag in [`AppState::regions`] — gleiche Reihenfolge, gleiche Länge.
    pub outcomes: Vec<HitOutcome>,
    /// Anzahl der Treffer insgesamt.
    pub total: usize,
    /// Anzahl derer, die wirklich geschwärzt werden.
    pub redacted: usize,
}

impl HitSummary {
    pub fn outcome(&self, index: usize) -> HitOutcome {
        self.outcomes
            .get(index)
            .copied()
            .unwrap_or(HitOutcome::Duplicate)
    }

    /// Die eine Zahl, auf die es ankommt — als Satz.
    pub fn headline(&self) -> String {
        format!(
            "{} Treffer · {} werden geschwärzt",
            self.total, self.redacted
        )
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

    /// Hat die Nutzerin an diesem Eintrag etwas geändert?
    ///
    /// Grundlage für die Rückfrage, bevor Regionen weggeworfen werden.
    pub fn is_hand_made(&self) -> bool {
        // `enabled == is_blocking()` heißt: der Schalter steht **anders**, als
        // ihn `AnnotatedRegion::new` gesetzt hätte.
        matches!(self.region.source, Source::Manual { .. })
            || self.enabled == self.region.is_blocking()
            || self.action != Action::Blackout
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

    /// Beschreibung in Klartext — siehe [`plain_description`].
    pub fn description(&self) -> String {
        plain_description(&self.region.source)
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
    /// Das geladene Dokument.
    ///
    /// Hinter einem [`Arc`], weil der Rasterizer in [`crate::render`] auf einem
    /// eigenen Thread darauf zugreift. Ohne den `Arc` müsste für jede Vorschau
    /// eine vollständige Kopie des Dokuments angelegt werden.
    pub document: Option<Arc<lopdf::Document>>,
    /// MediaBox je Seite.
    pub page_boxes: Vec<Rect>,
    /// `/Rotate` je Seite (0/90/180/270), inklusive Vererbung vom Seitenbaum.
    pub rotations: Vec<i64>,
    pub runs: Vec<TextRun>,
    pub current_page: usize,
    pub zoom: f32,
    pub regions: Vec<AnnotatedRegion>,
    pub selected_region: Option<usize>,
    pub booking_path: Option<PathBuf>,
    /// Namenszusatz für die vorgeschlagene Ausgabedatei.
    ///
    /// Aus `kontoauszug.pdf` wird mit dem Standardwert
    /// `kontoauszug_geschwaerzt.pdf`. Bewusst Zustand und keine Konstante — der
    /// Zusatz ist in der Oberfläche änderbar und soll später auch aus einer
    /// Einstellungsdatei bzw. von `--output-suffix` kommen können.
    pub output_suffix: String,
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
            rotations: Vec::new(),
            runs: Vec::new(),
            current_page: 0,
            zoom: 1.0,
            regions: Vec::new(),
            selected_region: None,
            booking_path: None,
            output_suffix: DEFAULT_OUTPUT_SUFFIX.to_string(),
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
        self.rotations = page_rotations(&doc);
        self.runs = runs;
        self.document = Some(Arc::new(doc));
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

    /// Drehung der angegebenen Seite (0, solange nichts geladen ist).
    pub fn rotation(&self, page: usize) -> i64 {
        self.rotations.get(page).copied().unwrap_or(0)
    }

    /// Geometrie einer Seite für die Koordinatenumrechnung.
    ///
    /// Enthält `/Rotate`; ohne das lägen die Schwärzungsrechtecke auf gedrehten
    /// Seiten an der falschen Stelle.
    pub fn page_view(&self, page: usize) -> PageView {
        PageView::new(
            self.page_boxes
                .get(page)
                .copied()
                .unwrap_or(DEFAULT_PAGE_BOX),
            self.rotation(page),
        )
    }

    /// Geometrie der aktuellen Seite.
    pub fn current_page_view(&self) -> PageView {
        self.page_view(self.current_page)
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

        let summary = self.hit_summary();
        let total = summary.total;
        self.status = format!("Analyse: {}", summary.headline());
        Ok(total)
    }

    // ------------------------------------------------------- Regionen ändern

    /// Legt eine manuelle Region an, aktiviert sie und wählt sie aus.
    /// Gibt den Index der neuen Region zurück.
    pub fn add_manual_region(
        &mut self,
        page: usize,
        rect: Rect,
        reason: impl Into<String>,
    ) -> usize {
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

    /// Bilanz für die Anzeige: was passiert mit welchem Treffer?
    ///
    /// Einmal je Bild berechnen und weiterreichen — [`resolve_conflicts`] ist
    /// nicht teuer, aber quadratisch in der Trefferzahl.
    ///
    /// Die Zuordnung geschieht der Reihe nach: `resolve_conflicts` behält bei
    /// Duplikaten das **erste** Vorkommen, also findet auch hier das erste
    /// Vorkommen seinen Eintrag im Ergebnis, das zweite nicht mehr.
    pub fn hit_summary(&self) -> HitSummary {
        let resolution = self.resolution();
        let mut redact: Vec<Option<&Region>> = resolution.redact.iter().map(Some).collect();
        let mut blocked: Vec<Option<&BlockedRegion>> =
            resolution.blocked.iter().map(Some).collect();

        let outcomes: Vec<HitOutcome> = self
            .regions
            .iter()
            .map(|entry| {
                if entry.is_blocking() {
                    return HitOutcome::Protecting;
                }
                if !entry.enabled {
                    return HitOutcome::Disabled;
                }
                if let Some(slot) = redact
                    .iter_mut()
                    .find(|slot| slot.is_some_and(|r| *r == entry.region))
                {
                    *slot = None;
                    return HitOutcome::Redacted;
                }
                if let Some(slot) = blocked.iter_mut().find(|slot| {
                    slot.is_some_and(|b| b.page == entry.region.page && b.rect == entry.region.rect)
                }) {
                    *slot = None;
                    return HitOutcome::Blocked;
                }
                HitOutcome::Duplicate
            })
            .collect();

        HitSummary {
            total: outcomes.len(),
            redacted: resolution.redact.len(),
            outcomes,
        }
    }

    /// Steckt in den Regionen Handarbeit, die beim Verwerfen verloren ginge?
    ///
    /// Wahr, sobald ein Rechteck selbst gezogen, ein Treffer abgewählt oder
    /// dessen Schwärzungsart geändert wurde. Eine frisch gelaufene Analyse
    /// allein zählt **nicht** — die ist mit einem Klick wiederhergestellt.
    pub fn has_manual_work(&self) -> bool {
        self.regions.iter().any(AnnotatedRegion::is_hand_made)
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

    // ------------------------------------------------------- Dateinamen

    /// Vorschlag für die Ausgabedatei: **neben dem Original**, mit
    /// [`AppState::output_suffix`] am Dateinamen-Stamm.
    ///
    /// `None`, solange kein Dokument geladen ist. Der Vorschlag ist nie mit dem
    /// Eingabepfad identisch — dafür sorgt [`output_path_with_suffix`], das bei
    /// leerem Zusatz auf den Standard zurückfällt.
    pub fn suggested_output_path(&self) -> Option<PathBuf> {
        self.pdf_path
            .as_ref()
            .map(|input| output_path_with_suffix(input, &self.output_suffix))
    }

    /// Vorschlag für die Review-Datei: neben dem Original, `…_review.json`.
    pub fn suggested_review_path(&self) -> Option<PathBuf> {
        self.pdf_path
            .as_ref()
            .map(|input| sibling_path(input, REVIEW_SUFFIX, "json"))
    }

    /// Audit-Log zu einer Ausgabedatei: gleiches Verzeichnis, gleicher Stamm,
    /// Zusatz `_audit`, Endung `.json`.
    ///
    /// Das Log gehört immer neben die Datei, die es beschreibt — wandert die
    /// Ausgabe in ein anderes Verzeichnis, wandert das Log mit.
    pub fn audit_path_for(out: &Path) -> PathBuf {
        sibling_path(out, AUDIT_SUFFIX, "json")
    }

    /// Vorschlag für das Audit-Log zur vorgeschlagenen Ausgabedatei.
    pub fn suggested_audit_path(&self) -> Option<PathBuf> {
        self.suggested_output_path()
            .map(|out| Self::audit_path_for(&out))
    }

    /// Verzeichnis, in dem Dateidialoge starten sollen.
    pub fn dialog_directory(&self) -> Option<PathBuf> {
        self.pdf_path
            .as_ref()
            .and_then(|p| p.parent())
            .filter(|d| !d.as_os_str().is_empty())
            .map(|d| d.to_path_buf())
    }

    // ---------------------------------------------------------------- Export

    /// Zeigt `out` auf die geladene Originaldatei?
    ///
    /// Erst der reine Pfadvergleich (greift auch, wenn die Zieldatei noch gar
    /// nicht existiert), dann — falls beide Pfade auflösbar sind — der Vergleich
    /// der aufgelösten Pfade. Damit fallen auch `./auszug.pdf`, Symlinks und
    /// `../ordner/auszug.pdf` auf.
    pub fn targets_the_input(&self, out: &Path) -> bool {
        let Some(input) = self.pdf_path.as_deref() else {
            return false;
        };
        if out == input {
            return true;
        }
        match (std::fs::canonicalize(out), std::fs::canonicalize(input)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    }

    /// Schwärzt eine Kopie des Dokuments, entfernt die Metadaten und schreibt
    /// das Ergebnis nach `out`.
    ///
    /// Die Reihenfolge (schwärzen → Metadaten strippen → schreiben) ist exakt
    /// dieselbe wie in der CLI-Pipeline; bei gleicher Regionenmenge entsteht
    /// dieselbe Datei.
    ///
    /// Ist `audit` gesetzt, wird zusätzlich ein JSON-Audit-Log geschrieben.
    ///
    /// Zwei Fälle werden **abgelehnt, bevor irgendetwas geschrieben wird**:
    ///
    /// * `out` zeigt auf die Originaldatei — der Dateidialog lässt das zu, und
    ///   ein Klick auf „Überschreiben“ hätte das ungeschwärzte Original
    ///   vernichtet;
    /// * es ist nichts ausgewählt. Vorher entstand eine unveränderte Kopie
    ///   namens `…_geschwaerzt.pdf` samt Erfolgsmeldung — eine Datei, die
    ///   aussieht wie ein Ergebnis und keines ist.
    pub fn export(&self, out: &Path, audit: Option<&Path>) -> Result<RedactionReport> {
        let doc = self
            .document
            .as_ref()
            .ok_or_else(|| RedactError::Pdf("Kein Dokument geladen".into()))?;

        if self.targets_the_input(out) {
            return Err(RedactError::Config(
                "Das ist die Originaldatei — bitte einen anderen Namen wählen.".into(),
            ));
        }

        let redactions = self.enabled_redactions();
        if redactions.is_empty() {
            return Err(RedactError::Config(
                "Nichts ausgewählt — es würde nichts geschwärzt.".into(),
            ));
        }

        let mut copy = (**doc).clone();
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

// --------------------------------------------------------------- Seitendrehung

/// `/Rotate` jeder Seite, 0-basiert und inklusive Vererbung vom Seitenbaum.
///
/// `redact-pdf` liefert die MediaBoxen, aber keine Drehungen; der Rasterizer
/// meldet die Drehung erst mit dem fertigen Bild. Die Oberfläche braucht sie
/// aber **sofort**, sonst säßen die Schwärzungsrechtecke bis zum Eintreffen des
/// ersten Bildes an der falschen Stelle. Deshalb hier noch einmal, mit derselben
/// Vererbungslogik wie `redact_pdf::page_box`.
pub fn page_rotations(doc: &lopdf::Document) -> Vec<i64> {
    doc.get_pages()
        .values()
        .map(|id| page_rotation(doc, *id))
        .collect()
}

fn page_rotation(doc: &lopdf::Document, page_id: lopdf::ObjectId) -> i64 {
    let mut current = Some(page_id);
    let mut depth = 0;
    while let Some(id) = current {
        // Gegen im Kreis zeigende /Parent-Ketten in kaputten Dateien.
        if depth > 32 {
            break;
        }
        depth += 1;
        let Ok(dict) = doc.get_dictionary(id) else {
            break;
        };
        if let Some(value) = dict
            .get(b"Rotate")
            .ok()
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| o.as_i64().ok())
        {
            return normalize_rotation(value);
        }
        current = match dict.get(b"Parent") {
            Ok(lopdf::Object::Reference(parent)) => Some(*parent),
            _ => None,
        };
    }
    0
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
        state.regions.push(AnnotatedRegion::new(negative_region(
            0,
            Rect::new(0.0, 0.0, 10.0, 10.0),
        )));
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
        state.regions.push(AnnotatedRegion::new(negative_region(
            0,
            Rect::new(0.0, 0.0, 10.0, 10.0),
        )));
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
        state.regions.push(AnnotatedRegion::new(pattern_region(
            0,
            Rect::new(0.0, 0.0, 10.0, 10.0),
        )));
        assert!(state.regions[0].enabled);
        assert!(state.toggle_enabled(0));
        assert!(!state.regions[0].enabled);
        assert!(state.toggle_enabled(0));
        assert!(state.regions[0].enabled);
    }

    #[test]
    fn enabled_redactions_respect_negative_list() {
        let mut state = AppState::new();
        state.regions.push(AnnotatedRegion::new(pattern_region(
            0,
            Rect::new(10.0, 10.0, 50.0, 20.0),
        )));
        state.regions.push(AnnotatedRegion::new(negative_region(
            0,
            Rect::new(0.0, 0.0, 100.0, 30.0),
        )));

        // Der Pattern-Treffer wird zu 100 % von der Negativregion überdeckt.
        assert!(state.enabled_redactions().is_empty());
        assert_eq!(state.blocked_regions().len(), 1);
        assert_eq!(state.blocked_regions()[0].booking_id, "b003");

        // Eine manuelle Region an derselben Stelle überstimmt die Negativliste.
        state.add_manual_region(0, Rect::new(10.0, 10.0, 50.0, 20.0), "bewusst");
        let redactions = state.enabled_redactions();
        assert_eq!(redactions.len(), 1);
        assert!(matches!(redactions[0].region.source, Source::Manual { .. }));
    }

    #[test]
    fn disabled_regions_are_not_exported() {
        let mut state = AppState::new();
        state.regions.push(AnnotatedRegion::new(pattern_region(
            0,
            Rect::new(10.0, 10.0, 50.0, 20.0),
        )));
        assert_eq!(state.enabled_redactions().len(), 1);
        assert!(state.set_enabled(0, false));
        assert!(state.enabled_redactions().is_empty());
    }

    #[test]
    fn action_is_carried_into_redactions() {
        let mut state = AppState::new();
        state.regions.push(AnnotatedRegion::new(pattern_region(
            0,
            Rect::new(10.0, 10.0, 50.0, 20.0),
        )));
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
        let text: String = runs
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !text.contains("DE89"),
            "IBAN steht noch im Dokument: {text}"
        );
        assert!(!text.contains("DE02"));
        // Nicht getroffener Text bleibt erhalten.
        assert!(text.contains("Musterbank"));

        // Audit-Log ist gültiges JSON in der vereinbarten Form.
        let log: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&audit).unwrap()).unwrap();
        assert_eq!(log["metadata_stripped"], serde_json::json!(true));
        assert!(!log["redactions"].as_array().unwrap().is_empty());
        assert_eq!(log["redactions"][0]["page"], serde_json::json!(1));
        assert_eq!(log["input"]["sha256"], serde_json::json!(""));
        assert!(log["timestamp"].as_str().unwrap().ends_with('Z'));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Vergleicht den Export mit einer **hier von Hand nachgebauten** Kette aus
    /// denselben Bausteinen (schwärzen → Metadaten strippen → schreiben).
    ///
    /// Ausdrücklich **kein** Vergleich mit `redact-cli`: dieses Crate hängt
    /// nicht von der CLI ab und ruft deren Pipeline nicht auf. Der Test zeigt
    /// also, dass `export` genau diese drei Schritte in dieser Reihenfolge und
    /// mit der Vorgabe-Polsterung ausführt — nicht mehr. Weicht die CLI
    /// irgendwann ab, merkt das nur ein Test, der beide wirklich ausführt.
    #[test]
    fn export_matches_a_hand_built_pipeline_of_the_same_steps() {
        let mut state = loaded_state();
        state.analyze(&["iban_de".to_string()], None).unwrap();
        state.add_manual_region(1, Rect::new(70.0, 700.0, 250.0, 715.0), "Adresse");

        let dir = temp_dir("identical");
        let gui_out = dir.join("gui.pdf");
        state.export(&gui_out, None).unwrap();

        let reference = dir.join("reference.pdf");
        let mut doc = redact_pdf::load_from_bytes(&redact_pdf::testing::demo_statement()).unwrap();
        PdfRedactor::with_padding(1.0)
            .apply_with_report(&mut doc, &state.enabled_redactions())
            .unwrap();
        strip_metadata(&mut doc);
        PdfRenderer::new().render(&doc, &reference).unwrap();

        assert_eq!(
            std::fs::read(&gui_out).unwrap(),
            std::fs::read(&reference).unwrap(),
            "Export muss Byte für Byte dem nachgebauten Ablauf entsprechen"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn export_without_document_fails() {
        let state = AppState::new();
        let dir = temp_dir("noexport");
        assert!(state.export(&dir.join("x.pdf"), None).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A4: ohne ausgewählte Schwärzung entstand bisher eine unveränderte Kopie
    /// mit Erfolgsmeldung. Jetzt wird abgelehnt — und **keine Datei angelegt**.
    #[test]
    fn export_refuses_when_nothing_would_be_redacted() {
        let dir = temp_dir("nothing");
        let out = dir.join("leer.pdf");

        // Gar keine Treffer.
        let state = loaded_state();
        let error = state.export(&out, None).unwrap_err().to_string();
        assert!(error.contains("Nichts ausgewählt"), "{error}");
        assert!(!out.exists(), "es darf keine Datei entstanden sein");

        // Treffer vorhanden, aber alle abgewählt.
        let mut state = loaded_state();
        state.analyze(&["iban_de".to_string()], None).unwrap();
        assert!(!state.regions.is_empty());
        for index in 0..state.regions.len() {
            state.set_enabled(index, false);
        }
        assert!(state.export(&out, None).is_err());
        assert!(!out.exists());

        // Auch das Audit-Log wird dann nicht geschrieben.
        let audit = dir.join("leer_audit.json");
        assert!(state.export(&out, Some(&audit)).is_err());
        assert!(!audit.exists());

        // Mit einer aktiven Schwärzung geht es durch.
        state.set_enabled(0, true);
        assert!(state.export(&out, None).is_ok());
        assert!(out.exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// A6: der Speichern-Dialog lässt das Original als Ziel zu. Der Export
    /// nicht.
    #[test]
    fn export_refuses_to_overwrite_the_original() {
        let dir = temp_dir("overwrite");
        let input = dir.join("auszug.pdf");
        std::fs::write(&input, redact_pdf::testing::demo_statement()).unwrap();

        let mut state = AppState::new();
        state.load_document(&input).unwrap();
        state.analyze(&["iban_de".to_string()], None).unwrap();
        assert!(!state.regions.is_empty());

        let before = std::fs::read(&input).unwrap();
        let error = state.export(&input, None).unwrap_err().to_string();
        assert!(error.contains("Originaldatei"), "{error}");
        assert_eq!(
            std::fs::read(&input).unwrap(),
            before,
            "das Original muss unangetastet bleiben"
        );

        // Auch über einen Umweg im Pfad.
        let detour = dir.join("unterordner").join("..").join("auszug.pdf");
        std::fs::create_dir_all(dir.join("unterordner")).unwrap();
        assert!(state.targets_the_input(&detour));
        assert!(state.export(&detour, None).is_err());

        // Ein anderer Name ist erlaubt.
        assert!(state
            .export(&dir.join("auszug_geschwaerzt.pdf"), None)
            .is_ok());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn export_strips_metadata() {
        let mut state = loaded_state();
        state.analyze(&["iban_de".to_string()], None).unwrap();
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
        assert_eq!(
            restored.to_review_file().items,
            state.to_review_file().items
        );
    }

    #[test]
    fn review_file_never_enables_a_negative_hit() {
        let mut state = AppState::new();
        state.regions.push(AnnotatedRegion::new(negative_region(
            0,
            Rect::new(0.0, 0.0, 10.0, 10.0),
        )));
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
    fn suggested_output_path_sits_next_to_the_input() {
        let mut state = AppState::new();
        assert_eq!(state.output_suffix, DEFAULT_OUTPUT_SUFFIX);
        // Ohne Dokument gibt es keinen Vorschlag.
        assert_eq!(state.suggested_output_path(), None);
        assert_eq!(state.suggested_review_path(), None);
        assert_eq!(state.suggested_audit_path(), None);
        assert_eq!(state.dialog_directory(), None);

        state
            .load_bytes(
                &redact_pdf::testing::demo_statement(),
                Some(PathBuf::from("/daten/kontoauszug.pdf")),
            )
            .unwrap();

        assert_eq!(
            state.suggested_output_path().unwrap(),
            PathBuf::from("/daten/kontoauszug_geschwaerzt.pdf")
        );
        assert_eq!(
            state.suggested_review_path().unwrap(),
            PathBuf::from("/daten/kontoauszug_review.json")
        );
        assert_eq!(
            state.suggested_audit_path().unwrap(),
            PathBuf::from("/daten/kontoauszug_geschwaerzt_audit.json")
        );
        assert_eq!(state.dialog_directory().unwrap(), PathBuf::from("/daten"));
    }

    #[test]
    fn suggested_output_path_honours_a_custom_suffix() {
        let mut state = AppState::new();
        state
            .load_bytes(
                &redact_pdf::testing::demo_statement(),
                Some(PathBuf::from("/daten/kontoauszug.pdf")),
            )
            .unwrap();

        state.output_suffix = "_anonym".to_string();
        assert_eq!(
            state.suggested_output_path().unwrap(),
            PathBuf::from("/daten/kontoauszug_anonym.pdf")
        );

        // Leerer Zusatz fällt auf den Standard zurück, damit das Original
        // niemals überschrieben wird.
        state.output_suffix = String::new();
        assert_eq!(
            state.suggested_output_path().unwrap(),
            PathBuf::from("/daten/kontoauszug_geschwaerzt.pdf")
        );
    }

    #[test]
    fn suggested_output_path_handles_a_missing_extension() {
        let mut state = AppState::new();
        state
            .load_bytes(
                &redact_pdf::testing::demo_statement(),
                Some(PathBuf::from("/daten/kontoauszug")),
            )
            .unwrap();
        assert_eq!(
            state.suggested_output_path().unwrap(),
            PathBuf::from("/daten/kontoauszug_geschwaerzt.pdf")
        );
    }

    #[test]
    fn suggested_output_path_never_equals_the_input() {
        let inputs = [
            "/daten/kontoauszug.pdf",
            "kontoauszug.pdf",
            "/daten/ohne_endung",
            "/daten/.pdf",
        ];
        let suffixes = ["", "   ", "_geschwaerzt", "_x", "_anonym"];
        for input in inputs {
            let mut state = AppState::new();
            state
                .load_bytes(
                    &redact_pdf::testing::demo_statement(),
                    Some(PathBuf::from(input)),
                )
                .unwrap();
            for suffix in suffixes {
                state.output_suffix = suffix.to_string();
                let out = state.suggested_output_path().unwrap();
                assert_ne!(
                    out,
                    PathBuf::from(input),
                    "Vorschlag darf die Eingabe nicht überschreiben ({input}, {suffix:?})"
                );
            }
        }
    }

    #[test]
    fn audit_path_follows_the_chosen_output() {
        // Wählt die Nutzerin ein anderes Verzeichnis, wandert das Log mit.
        assert_eq!(
            AppState::audit_path_for(Path::new("/woanders/final.pdf")),
            PathBuf::from("/woanders/final_audit.json")
        );
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
        let redaction = Redaction::new(
            pattern_region(0, Rect::new(1.0, 2.0, 3.0, 4.0)),
            Action::Blackout,
        );
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
        assert_eq!(
            log["redactions"][0]["action"],
            serde_json::json!("blackout")
        );
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

    /// Farbe allein reicht nicht: bei einer Rot-Grün-Sehschwäche sind
    /// „Liste: schwärzen“ und „Liste: schützen“ sonst nicht zu trennen.
    #[test]
    fn every_category_is_distinguishable_without_colour() {
        for (i, a) in REGION_COLORS.iter().enumerate() {
            for b in REGION_COLORS.iter().skip(i + 1) {
                assert_ne!(a.marker(), b.marker(), "{a:?} und {b:?} teilen ein Zeichen");
                assert_ne!(
                    a.label(),
                    b.label(),
                    "{a:?} und {b:?} teilen eine Beschriftung"
                );
                assert_ne!(a.rgb(), b.rgb(), "{a:?} und {b:?} teilen eine Farbe");
            }
        }
        // Die Beschriftungen dürfen kein Entwicklervokabular mehr enthalten.
        for color in REGION_COLORS {
            let label = color.label().to_lowercase();
            assert!(!label.contains("pattern"), "{label}");
            assert!(!label.contains("negativ"), "{label}");
            assert!(!label.contains("positiv"), "{label}");
        }
    }

    /// Relative Leuchtdichte nach WCAG 2.1.
    fn luminance((r, g, b): (u8, u8, u8)) -> f64 {
        let channel = |v: u8| {
            let c = v as f64 / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
    }

    fn contrast(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
        let (x, y) = (luminance(a), luminance(b));
        let (hi, lo) = if x > y { (x, y) } else { (y, x) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// Grafische Elemente brauchen nach WCAG 1.4.11 mindestens 3:1.
    #[test]
    fn region_colours_reach_the_graphic_contrast_minimum() {
        // Weißes Blatt, heller und dunkler Bereichshintergrund von egui.
        let backgrounds = [(255, 255, 255), (248, 248, 248), (27, 27, 27)];
        for color in REGION_COLORS {
            for background in backgrounds {
                let ratio = contrast(color.rgb(), background);
                assert!(
                    ratio >= 3.0,
                    "{color:?} erreicht gegen {background:?} nur {ratio:.2}:1"
                );
            }
        }
        // Das alte Orange scheiterte genau daran — Beleg, dass der Test greift.
        assert!(contrast((240, 150, 30), (255, 255, 255)) < 3.0);
    }

    #[test]
    fn descriptions_are_written_for_bank_customers() {
        let konto = plain_description(&Source::Pattern {
            pattern_id: "konto_nr".into(),
            confidence: 0.4,
        });
        assert_eq!(konto, "Kontonummer (Heuristik, 6–10 Ziffern)");
        // Weder interne ID noch die bedeutungslose Zahl.
        assert!(!konto.contains("konto_nr"));
        assert!(!konto.contains("0.4"));
        assert!(!konto.contains("confidence"));

        assert_eq!(
            plain_description(&Source::Booking {
                booking_id: "b1".into(),
                match_type: MatchType::Positive
            }),
            "Aus Ihrer Liste: soll geschwärzt werden"
        );
        assert_eq!(
            plain_description(&Source::Booking {
                booking_id: "b1".into(),
                match_type: MatchType::Negative
            }),
            "Aus Ihrer Liste: darf nicht geschwärzt werden"
        );
        assert_eq!(
            plain_description(&Source::Manual {
                reason: "Gehalt".into()
            }),
            "Selbst gezeichnet: Gehalt"
        );
        assert_eq!(
            plain_description(&Source::Manual { reason: " ".into() }),
            "Selbst gezeichnet"
        );
        // Unbekanntes Muster: wenigstens die ID, aber kein „confidence“.
        let unknown = plain_description(&Source::Pattern {
            pattern_id: "aus_config".into(),
            confidence: 0.1,
        });
        assert!(unknown.contains("aus_config"));
        assert!(!unknown.contains("confidence"));
    }

    /// A3: „Treffer (N)“ zählte auch das, was nie geschwärzt wird.
    #[test]
    fn hit_summary_separates_found_from_actually_redacted() {
        let mut state = AppState::new();
        // 1 — wird geschwärzt.
        state.regions.push(AnnotatedRegion::new(pattern_region(
            0,
            Rect::new(200.0, 200.0, 260.0, 210.0),
        )));
        // 2 — abgewählt.
        state.regions.push(AnnotatedRegion::new(pattern_region(
            0,
            Rect::new(300.0, 300.0, 360.0, 310.0),
        )));
        state.set_enabled(1, false);
        // 3 — durch die Negativliste blockiert.
        state.regions.push(AnnotatedRegion::new(pattern_region(
            0,
            Rect::new(10.0, 10.0, 50.0, 20.0),
        )));
        // 4 — die blockierende Negativregion selbst.
        state.regions.push(AnnotatedRegion::new(negative_region(
            0,
            Rect::new(0.0, 0.0, 100.0, 30.0),
        )));
        // 5 — Duplikat von 1.
        state.regions.push(AnnotatedRegion::new(pattern_region(
            0,
            Rect::new(200.0, 200.0, 260.0, 210.0),
        )));

        let summary = state.hit_summary();
        assert_eq!(summary.total, 5);
        assert_eq!(summary.redacted, 1);
        assert_eq!(summary.redacted, state.enabled_redactions().len());
        assert_eq!(
            summary.outcomes,
            vec![
                HitOutcome::Redacted,
                HitOutcome::Disabled,
                HitOutcome::Blocked,
                HitOutcome::Protecting,
                HitOutcome::Duplicate,
            ]
        );
        assert_eq!(summary.headline(), "5 Treffer · 1 werden geschwärzt");
        // Genau ein Eintrag wird gefüllt gezeichnet.
        assert_eq!(
            summary.outcomes.iter().filter(|o| o.is_redacted()).count(),
            1
        );
        // „geschützt“ statt Durchstreichen.
        assert_eq!(HitOutcome::Protecting.note(), "geschützt");
    }

    #[test]
    fn hit_summary_is_empty_without_regions() {
        let summary = AppState::new().hit_summary();
        assert_eq!(summary.total, 0);
        assert_eq!(summary.redacted, 0);
        assert_eq!(summary.headline(), "0 Treffer · 0 werden geschwärzt");
    }

    /// A8: vor dem Wegwerfen von Handarbeit muss nachgefragt werden — aber nur
    /// dann, sonst ist die Rückfrage bloß lästig.
    #[test]
    fn has_manual_work_only_reacts_to_real_hand_work() {
        // Leerer Zustand: nichts zu verlieren.
        assert!(!AppState::new().has_manual_work());

        // Eine reine Analyse ist mit einem Klick wiederholbar.
        let mut state = loaded_state();
        state.analyze(&["iban_de".to_string()], None).unwrap();
        assert!(!state.regions.is_empty());
        assert!(!state.has_manual_work());

        // Abwählen ist Handarbeit …
        state.set_enabled(0, false);
        assert!(state.has_manual_work());
        state.set_enabled(0, true);
        assert!(!state.has_manual_work());

        // … eine geänderte Schwärzungsart auch …
        state.set_action(0, Action::Whiteout);
        assert!(state.has_manual_work());
        state.set_action(0, Action::Blackout);
        assert!(!state.has_manual_work());

        // … und ein selbst gezogenes Rechteck sowieso.
        state.add_manual_region(0, Rect::new(10.0, 10.0, 40.0, 20.0), "Gehalt");
        assert!(state.has_manual_work());
        state.selected_region = Some(state.regions.len() - 1);
        assert!(state.delete_selected());
        assert!(!state.has_manual_work());

        // Ein Negativlisten-Treffer ist ausgeschaltet — das ist sein Normalfall
        // und keine Handarbeit.
        let mut state = AppState::new();
        state.regions.push(AnnotatedRegion::new(negative_region(
            0,
            Rect::new(0.0, 0.0, 10.0, 10.0),
        )));
        assert!(!state.has_manual_work());
    }

    #[test]
    fn page_view_carries_the_rotation_of_each_page() {
        let mut state = AppState::new();
        // Ohne Dokument: A4 und ungedreht.
        assert_eq!(state.current_page_view().rotate, 0);
        assert_eq!(state.current_page_view().media_box, DEFAULT_PAGE_BOX);

        state
            .load_bytes(
                &rotated_demo(&[90, 270]),
                Some(PathBuf::from("gedreht.pdf")),
            )
            .unwrap();
        assert_eq!(state.rotations, vec![90, 270]);
        assert_eq!(state.page_view(0).rotate, 90);
        assert_eq!(state.page_view(1).rotate, 270);
        // Der Anzeigeraum ist bei 90° quer.
        let display = state.page_view(0).display_box();
        assert!((display.width() - 842.0).abs() < 0.01);
        assert!((display.height() - 595.0).abs() < 0.01);

        // Unbekannte Seite → 0, kein Absturz.
        assert_eq!(state.page_view(99).rotate, 0);
    }

    #[test]
    fn odd_and_inherited_rotations_are_normalized() {
        // Krumme Werte gelten als „nicht gedreht“ …
        let mut state = AppState::new();
        state.load_bytes(&rotated_demo(&[45, -90]), None).unwrap();
        assert_eq!(state.rotations, vec![0, 270]);

        // … und ein am /Pages-Knoten gesetzter Wert wird vererbt.
        let mut doc = redact_pdf::load_from_bytes(&redact_pdf::testing::demo_statement()).unwrap();
        let pages_id = doc
            .catalog()
            .unwrap()
            .get(b"Pages")
            .unwrap()
            .as_reference()
            .unwrap();
        doc.get_object_mut(pages_id)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Rotate", 180_i64);
        assert_eq!(page_rotations(&doc), vec![180, 180]);
    }

    /// Demo-PDF mit `/Rotate` je Seite.
    fn rotated_demo(rotations: &[i64]) -> Vec<u8> {
        let mut doc = redact_pdf::load_from_bytes(&redact_pdf::testing::demo_statement()).unwrap();
        let ids: Vec<lopdf::ObjectId> = doc.get_pages().values().copied().collect();
        for (id, rotate) in ids.iter().zip(rotations) {
            doc.get_object_mut(*id)
                .unwrap()
                .as_dict_mut()
                .unwrap()
                .set("Rotate", *rotate);
        }
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
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
