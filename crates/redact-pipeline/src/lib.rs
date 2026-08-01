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
pub mod settings;
pub mod testing;

use std::path::{Path, PathBuf};

use lopdf::{Document, LoadOptions};
use redact_booking::{BookingMatcher, CsvBookingLoader};
use redact_core::{
    output_path_with_suffix, resolve_conflicts, sibling_path, Action, BlockedRegion, RedactError,
    Redaction, Region, Result, ReviewFile, ReviewInput, TextRun, REVIEW_SUFFIX,
};
use redact_patterns::PatternMatcher;
use redact_pdf::document::{
    check_target, load_from_bytes_with_limits, validate, write_file, Limits, WriteOptions,
};
use redact_pdf::{PdfExtractor, PdfRedactor, PdfRenderer};

pub use crate::audit::{sha256_bytes, sha256_file, Applied, AuditLog, Effects, EntryEffect};
pub use crate::settings::Settings;

/// Vorgabe für `--padding`, in Punkt.
pub const DEFAULT_PADDING: f64 = 1.0;

/// Vorgabe für `--max-candidates`, siehe [`Config::max_candidates`].
pub const DEFAULT_MAX_CANDIDATES: usize = 100_000;

/// Ein Passwort, das sich nicht versehentlich ausplaudern lässt.
///
/// Der einzige Weg an den Klartext ist [`Secret::reveal`] — und den ruft genau
/// eine Stelle auf: [`load_document`], für `lopdf::Document::decrypt`. `Debug`
/// und `Display` zeigen Sterne, damit ein `{:?}` irgendwo im Programm (die
/// Oberfläche hält die [`Config`] in ihrem Zustand, und der leitet `Debug` ab)
/// das Passwort nicht in eine Meldung, ein Protokoll oder einen Panik-Text
/// schreibt. Absichtlich **nicht** `Serialize`: so kann es auch nicht
/// versehentlich in einer JSON-Datei landen.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(password: impl Into<String>) -> Self {
        Self(password.into())
    }

    /// Der Klartext. Nur für die Entschlüsselung.
    pub fn reveal(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl std::fmt::Display for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("***")
    }
}

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
    /// Passwort eines verschlüsselten Dokuments.
    ///
    /// Ohne Passwort bleibt es bei der Ablehnung aus `redact-pdf`: ein
    /// verschlüsseltes PDF wird nicht verarbeitet. Siehe [`load_document`].
    pub password: Option<Secret>,
    /// Thema der Oberfläche (`hell` oder `dunkel`).
    ///
    /// Steht hier, weil [`Config`] die *eine* Einstellungsstruktur beider
    /// Programme ist: die Einstellungsdatei hat damit genau einen Weg in die
    /// Kommandozeile **und** in die Oberfläche, und es gibt keine zweite
    /// Stelle, an der beide auseinanderlaufen könnten. Auf das Ergebnis der
    /// Schwärzung hat der Wert keinen Einfluss.
    pub theme: String,
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
            password: None,
            theme: settings::THEMES[0].to_string(),
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
    /// Davon wirksam — Regionen, die nachweislich mindestens ein Zeichen aus
    /// dem Content-Stream entfernt haben.
    ///
    /// Gemessen an [`redact_pdf::RedactionReport::per_redaction`], nicht am
    /// Rechteck geschätzt: „gültiges Rechteck“ ist kein Nachweis.
    pub effective_redactions: usize,
    /// Davon nur überdeckt: gültiges Rechteck auf einer vorhandenen Seite,
    /// Deck-Rechteck gezeichnet, aber kein Zeichen getroffen. Siehe
    /// [`audit::EntryEffect::Covered`] — richtig über einer Grafik, falsch bei
    /// danebenliegenden Koordinaten.
    pub covered_redactions: usize,
    /// Davon entartet: leeres Rechteck, also ohne jede Wirkung.
    pub degenerate_redactions: usize,
    /// Davon auf einer Seite, die es im Dokument nicht gibt — dort ist
    /// überhaupt nichts geschehen.
    pub missing_page_redactions: usize,
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

// -------------------------------------------------------- Verschlüsselung

/// Meldung, wenn das angegebene Passwort nicht passt.
///
/// Bewusst **ohne** den Fehler aus `lopdf` und selbstverständlich ohne das
/// Passwort: eine Fehlermeldung landet in Protokollen, auf Bildschirmfotos und
/// in Fehlerberichten.
const WRONG_PASSWORD: &str = "Das Dokument ließ sich mit diesem Passwort nicht \
     entschlüsseln. Passt das Passwort, oder benutzt die Datei ein \
     Verschlüsselungsverfahren, das redact-rs nicht beherrscht?";

/// Erkennt die Ablehnungen, gegen die ein Passwort hilft.
///
/// Zwei Fälle: „ist verschlüsselt, kein Passwort da“ (aus
/// `redact_pdf::document::validate`) und „Passwort passt nicht“ (von hier).
/// Die Oberfläche fragt danach, ob sie das Passwortfenster zeigt; der Test
/// `an_encrypted_document_asks_for_a_password` hält beide Fälle fest, damit
/// eine geänderte Meldung nicht stillschweigend zu „gar keine Abfrage mehr“
/// wird.
pub fn password_required(error: &RedactError) -> bool {
    match error {
        RedactError::Pdf(msg) => msg.contains("verschlüsselt") || msg.contains(WRONG_PASSWORD),
        _ => false,
    }
}

/// Lädt ein PDF aus dem Speicher — mit Passwort auch ein verschlüsseltes.
///
/// **Ohne** Passwort ändert sich nichts: `redact-pdf` lehnt verschlüsselte
/// Dateien ab, und das bleibt richtig so. Ein Passwort ist die ausdrückliche
/// Ansage „ich darf das öffnen“ und gibt genau *dieser* einen Ablehnung einen
/// zweiten Anlauf. Jede andere Ablehnung — kein PDF, zu tief verschachtelt,
/// Dekompressionsbombe, kaputter Katalog — bleibt bestehen; ein Passwort soll
/// keinen fremden Fehler übertünchen.
///
/// Der erste Anlauf hat die Vorprüfung der Rohbytes bereits bestanden (sonst
/// wäre es nicht die Verschlüsselungs-Ablehnung geworden), deshalb läuft sie
/// nicht ein zweites Mal. **Grenze:** was `prescan` an einem verschlüsselten
/// Dokument messen kann, ist wenig — die Streams lassen sich vor der
/// Entschlüsselung nicht auspacken, `--max-decompressed-mb` greift dort also
/// nicht. Siehe `SECURITY.md`.
pub fn load_document(bytes: &[u8], config: &Config) -> Result<Document> {
    let rejected = match load_from_bytes_with_limits(bytes, &config.limits) {
        Ok(doc) => return Ok(doc),
        Err(e) => e,
    };
    let Some(password) = config
        .password
        .as_ref()
        .filter(|_| password_required(&rejected))
    else {
        return Err(rejected);
    };

    let doc = Document::load_mem_with_options(bytes, LoadOptions::with_password(password.reveal()))
        .map_err(|_| RedactError::Pdf(WRONG_PASSWORD.to_string()))?;
    validate(&doc)?;
    Ok(doc)
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
    let mut doc = load_document(&bytes, config).map_err(|e| match e {
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

            // Mit Prüfsumme: eine Review-Datei hinter `--manual-regions` muss
            // zum Dokument gehören, genau wie hinter `--apply-review`.
            let candidates = collect_regions_for(config, &runs, &outcome.input_sha256)?;
            outcome.candidates = candidates.len();

            let resolution = resolve_conflicts(candidates);
            outcome.blocked = resolution.blocked.len();
            outcome.blocked_details = describe_blocked(&resolution.blocked);

            // 7. Review-Modus: nur exportieren.
            if config.review {
                let path = review_target(config);
                let mut review = ReviewFile::new(
                    ReviewInput {
                        path: config.input.display().to_string(),
                        sha256: outcome.input_sha256.clone(),
                        pages: outcome.pages,
                    },
                    resolution.redact,
                    resolution.blocked,
                );
                // `ReviewFile::new` trägt in jeden Eintrag `Action::Blackout`
                // ein — es kennt die Einstellungen des Laufs nicht. Ohne diese
                // Zeile gäbe eine mit `--action replace --replace-with "[IBAN]"`
                // erzeugte Review-Datei überall „schwarz“ an, und ein späteres
                // `--apply-review` schwärzte statt zu ersetzen: die Datei würde
                // die Absicht des Laufs falsch wiedergeben. Die Oberfläche
                // macht es an derselben Stelle genauso
                // (`AppState::to_review_file`).
                for item in &mut review.items {
                    item.action = config.action.clone();
                }
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

    // Vor der Schwärzung gelesen: danach ist es dieselbe Zahl, aber die Frage
    // „gibt es diese Seite?“ gehört zu dem Dokument, auf das die Regionen
    // gerechnet wurden.
    let pages = redact_pdf::page_count(doc);

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

    // Was die Schwärzung *nicht* bewirkt hat, gehört genauso gemeldet — und
    // zwar gemessen: `Effects` wertet `report.per_redaction` je Region aus.
    // `--padding=-100` lässt von jedem Rechteck nichts übrig, eine Region auf
    // einer nicht vorhandenen Seite wird nie angefasst, und eine Region mit
    // gültigem Rechteck kann trotzdem kein Zeichen treffen. Keiner dieser
    // Fälle ist eine ausgeführte Schwärzung.
    let effects = Effects::measure(redactions, config.padding, pages, &report);
    outcome.effective_redactions = effects.applied;
    outcome.covered_redactions = effects.covered;
    outcome.degenerate_redactions = effects.degenerate;
    outcome.missing_page_redactions = effects.missing_page;
    push_warnings(&mut outcome.warnings, effects.warnings(&report));

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
                effects: &effects,
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
///
/// **Ohne bekannte Prüfsumme.** Eine Review-Datei hinter `--manual-regions`
/// gilt damit als ungeprüft und wird abgelehnt, solange nicht
/// `--allow-unverified-review` gesetzt ist. Wer die Prüfsumme des Dokuments
/// kennt, ruft [`collect_regions_for`] und bekommt die volle Prüfung.
pub fn collect_regions(config: &Config, runs: &[TextRun]) -> Result<Vec<Region>> {
    collect_regions_for(config, runs, "")
}

/// Wie [`collect_regions`], aber mit der Prüfsumme des verarbeiteten Dokuments.
///
/// `document_sha` ist der SHA-256 der Bytes, die gerade verarbeitet werden (in
/// [`Outcome::input_sha256`] derselbe Wert). Ein leerer String heißt „nicht
/// bekannt“ und führt bei einer Review-Datei zur Ablehnung, siehe
/// [`check_review_identity`].
pub fn collect_regions_for(
    config: &Config,
    runs: &[TextRun],
    document_sha: &str,
) -> Result<Vec<Region>> {
    let mut regions = Vec::new();

    // 3. Manuelle Regionen (haben Vorrang und werden nie blockiert).
    if let Some(path) = &config.manual_regions {
        regions.extend(load_manual_regions(
            path,
            document_sha,
            config.allow_unverified_review,
        )?);
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
///
/// ## Für die Review-Datei gilt dieselbe Identitätsprüfung wie bei `--apply-review`
///
/// Sie fehlte hier, und damit ließ sich die Prüfung schlicht umgehen: dieselbe
/// Datei, die `--apply-review fremd.json` mit „gehört zu einem anderen
/// Dokument“ und Exit 2 zurückwies, ging hinter `--manual-regions` wortlos
/// durch. Die Rechtecke landeten an beliebigen Stellen, und das Audit-Log
/// bescheinigte „applied“. Eine Prüfung, die ein anderer Schalter aushebelt,
/// ist keine.
///
/// Das **nackte Array** bleibt ungeprüft: es nennt keine Herkunft und kann
/// keine nennen — es ist das Format für von Hand geschriebene Koordinaten und
/// behauptet nirgends, zu einem bestimmten Dokument zu gehören. Wer es benutzt,
/// hat die Seitenzahlen selbst gewählt; die Wirkung jeder einzelnen Region
/// steht danach im Audit-Log (siehe [`audit::EntryEffect`]).
pub fn load_manual_regions(
    path: &Path,
    document_sha: &str,
    allow_unverified: bool,
) -> Result<Vec<Region>> {
    let data = std::fs::read_to_string(path)?;
    if let Ok(regions) = serde_json::from_str::<Vec<Region>>(&data) {
        return Ok(regions.into_iter().map(normalize_region).collect());
    }
    match ReviewFile::from_json(&data) {
        Ok(review) => {
            check_review_identity(&review, document_sha, allow_unverified)?;
            Ok(review
                .items
                .into_iter()
                .filter(|i| i.enabled)
                .map(|i| normalize_region(i.region))
                .collect())
        }
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

    // ------------------------------------------------------------ Aufgabe #66

    /// `--manual-regions` war der Weg an der Identitätsprüfung vorbei: dieselbe
    /// Review-Datei, die `--apply-review` mit „gehört zu einem anderen
    /// Dokument“ ablehnt, ging hier wortlos durch.
    #[test]
    fn a_review_file_behind_manual_regions_is_checked_like_any_other() {
        let dir = std::env::temp_dir().join(format!("redact-manual-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("review.json");

        let document = sha256_bytes(b"Dokument A");
        let foreign = sha256_bytes(b"Dokument B");
        let mut review = review_with(&foreign);
        review.items = ReviewFile::new(
            ReviewInput {
                path: "auszug.pdf".into(),
                sha256: foreign.clone(),
                pages: 1,
            },
            vec![Region::new(
                0,
                Rect::new(10.0, 10.0, 20.0, 20.0),
                None,
                redact_core::Source::Manual {
                    reason: "Hand".into(),
                },
            )],
            Vec::new(),
        )
        .items;
        std::fs::write(&path, review.to_json().unwrap()).unwrap();

        let error = load_manual_regions(&path, &document, false)
            .expect_err("fremde Review-Datei muss auch hier abgelehnt werden")
            .to_string();
        assert!(error.contains("anderen Dokument"), "{error}");

        // Und der Schalter hilft ihr auch hier nicht — „ungeprüft“ ist etwas
        // anderes als „nachweislich fremd“.
        assert!(load_manual_regions(&path, &document, true).is_err());

        // Zum eigenen Dokument gehört sie und wird angewendet.
        std::fs::write(
            &path,
            serde_json::to_string(&{
                let mut own = review.clone();
                own.input.sha256 = document.clone();
                own
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            load_manual_regions(&path, &document, false).unwrap().len(),
            1
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Das nackte Regions-Array bleibt ungeprüft: es behauptet keine Herkunft.
    #[test]
    fn a_plain_region_array_needs_no_checksum() {
        let dir = std::env::temp_dir().join(format!("redact-manual-plain-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("regionen.json");
        std::fs::write(
            &path,
            r#"[{"page":0,
                 "rect":{"ll":{"x":1.0,"y":2.0},"ur":{"x":3.0,"y":4.0}},
                 "text":null,
                 "source":{"manual":{"reason":"Hand"}}}]"#,
        )
        .unwrap();
        assert_eq!(load_manual_regions(&path, "", false).unwrap().len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_matching_checksum_passes() {
        let document = sha256_bytes(b"Dokument A");
        assert_eq!(
            check_review_identity(&review_with(&document), &document, false).unwrap(),
            ReviewIdentity::Matches
        );
    }

    // ------------------------------------------------------ Verschlüsselung

    fn encrypted_config(password: Option<&str>) -> Config {
        Config {
            password: password.map(Secret::new),
            ..Config::default()
        }
    }

    /// Ohne Passwort bleibt es bei der klaren Ablehnung — und die Oberfläche
    /// erkennt sie als „hier fehlt ein Passwort“.
    #[test]
    fn an_encrypted_document_asks_for_a_password() {
        let error = load_document(testing::ENCRYPTED_PDF, &encrypted_config(None))
            .expect_err("ohne Passwort muss abgelehnt werden");
        assert!(error.to_string().contains("verschlüsselt"), "{error}");
        assert!(
            password_required(&error),
            "die Oberfläche würde gar nicht erst fragen: {error}"
        );

        // Und das falsche Passwort führt zur zweiten Abfrage, nicht zum Ende.
        let wrong = load_document(testing::ENCRYPTED_PDF, &encrypted_config(Some("falsch")))
            .expect_err("falsches Passwort muss abgelehnt werden");
        assert!(password_required(&wrong), "{wrong}");
    }

    /// Mit dem richtigen Passwort ist der Text wirklich lesbar — sonst hätte
    /// die Entschlüsselung nur nicht gemeckert.
    #[test]
    fn the_right_password_opens_the_document_and_the_text_is_readable() {
        let config = encrypted_config(Some(testing::ENCRYPTED_PDF_PASSWORD));
        let doc = load_document(testing::ENCRYPTED_PDF, &config).expect("Passwort passt");
        assert_eq!(redact_pdf::page_count(&doc), 1);
        let text = PdfExtractor::new()
            .extract(&doc)
            .unwrap()
            .iter()
            .map(|run| run.text.clone())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            text.contains(testing::ENCRYPTED_PDF_IBAN),
            "entschlüsselt, aber kein Text: {text:?}"
        );
    }

    /// Ein Passwort darf keinen anderen Fehler übertünchen.
    #[test]
    fn a_password_does_not_excuse_a_broken_file() {
        let error = load_document(b"keine PDF-Datei", &encrypted_config(Some("egal")))
            .expect_err("kaputte Datei bleibt kaputt")
            .to_string();
        assert!(error.contains("%PDF-"), "{error}");
    }

    /// Das Passwort darf in keiner Meldung und in keiner Ausgabe auftauchen —
    /// auch nicht über `Debug`, denn die Oberfläche hält die `Config` in einem
    /// Zustand, der `Debug` ableitet.
    #[test]
    fn the_password_never_shows_up_in_text() {
        const PW: &str = "streng-geheim-4711";
        let config = encrypted_config(Some(PW));
        assert!(!format!("{config:?}").contains(PW));
        assert!(!format!("{:?}", config.password).contains(PW));
        assert!(!format!("{}", config.password.as_ref().unwrap()).contains(PW));

        let error = load_document(testing::ENCRYPTED_PDF, &config)
            .expect_err("falsches Passwort")
            .to_string();
        assert!(!error.contains(PW), "{error}");
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
