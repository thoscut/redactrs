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
pub mod coverage;
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
    check_target, load_from_bytes_with_limits, prescan, save_to_bytes, validate, write_file,
    Limits, WriteOptions,
};
use redact_pdf::{PdfExtractor, PdfRedactor, PdfRenderer};

pub use crate::audit::{
    sha256_bytes, sha256_file, Applied, AuditLog, Effects, EntryEffect, PatternRecord,
};
pub use crate::coverage::is_coverage_gap;
pub use crate::settings::Settings;

/// Vorgabe für `--padding`, in Punkt.
pub const DEFAULT_PADDING: f64 = 1.0;

/// Vorgabe für `--max-candidates`, siehe [`Config::max_candidates`].
pub const DEFAULT_MAX_CANDIDATES: usize = 100_000;

/// Vorgabe für `--max-input-mb`: 512 MB.
///
/// ## Warum es diese Grenze überhaupt gibt
///
/// Die Eingabedatei wird in einem Stück in den Speicher gelesen — sie muss es
/// werden, denn die Prüfsumme im Audit-Log soll die der *verarbeiteten* Bytes
/// sein und nicht die einer Datei, die sich zwischendurch geändert hat. Ohne
/// Grenze ist die Größe des Speicherbedarfs damit eine Angabe der Datei.
/// Gemessen an einer dünn belegten Datei (`truncate -s 6G`, 4 kB wirklich auf
/// der Platte): Spitzenspeicher 6 149 MB, nach 20 s ein Fehler. Bei 32 GB
/// Nennlänge holt der Kernel den Prozess mit dem OOM-Killer, und den holt er
/// sich nicht immer allein.
///
/// Solange ein Mensch jede Datei einzeln aussuchte, war das theoretisch. Mit
/// der Stapelverarbeitung genügt eine Datei im Verzeichnis.
///
/// ## Warum 512 MB
///
/// Es ist eine Grenze gegen das Absurde, nicht gegen das Große. Die Vorlage
/// ist ein Kontoauszug: ein paar hundert Kilobyte, mit eingescannten Seiten
/// einige Megabyte. Selbst ein Jahrgang farbig gescannter Auszüge in 600 dpi
/// bleibt weit darunter. 512 MB lassen sich auf jeder Maschine lesen, auf der
/// die Oberfläche läuft — und sie sind zwei Zehnerpotenzen von dem entfernt,
/// was die Maschine umwirft. Wer wirklich mehr braucht, sagt es mit
/// `--max-input-mb`; das ist eine bewusste Entscheidung und keine, die eine
/// fremde Datei für den Nutzer trifft.
pub const DEFAULT_MAX_INPUT_BYTES: u64 = 512 * 1024 * 1024;

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
    /// **Keine** automatische Erkennung — geschwärzt wird nur, was von Hand
    /// gezogen oder über die Buchungsliste angegeben wurde (`--no-patterns`).
    ///
    /// Der Schalter ist die grobe Fassung von [`Config::disabled_patterns`].
    /// Beides zusammen ist erlaubt und kein Widerspruch: die Oberfläche merkt
    /// sich die einzeln abgeschalteten Muster, während alles aus ist, damit sie
    /// beim Wiedereinschalten noch da sind.
    ///
    /// Ist er gesetzt, sagt [`detection_notice`] das in jeder Ausgabe dieses
    /// Laufs — Konsole, Oberfläche, Audit-Log.
    pub no_patterns: bool,
    /// Einzeln abgeschaltete Muster (`--disable-pattern`).
    ///
    /// Die übrigen laufen weiter. Gedacht für Muster, die in einem bestimmten
    /// Dokument mehr Fehltreffer als Funde erzeugen (`date_de` ist der
    /// Regelfall), ohne deshalb die ganze Erkennung aufzugeben.
    ///
    /// **Ein unbekannter Name ist ein Bedienfehler** und beendet den Lauf; die
    /// Begründung steht bei [`disable_patterns`]. Gelesen wird die Liste über
    /// [`Config::disabled_pattern_ids`], nie roh.
    pub disabled_patterns: Vec<String>,
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
    /// Der Ersatztext dieses Laufs (`--replace-with`) — **unabhängig von
    /// [`Config::action`]**.
    ///
    /// ## Warum er nicht in `action` genügt
    ///
    /// [`Action::Replace`] trägt seinen Text selbst, und das ist richtig: die
    /// Art gehört zu *einer* Region und steht so auch in der Review-Datei. Als
    /// einziger Ort für `--replace-with` reicht das aber nicht, weil die Art
    /// des Laufs und der Ersatztext des Laufs zwei verschiedene Angaben sind.
    ///
    /// `--action blackout --replace-with "[IBAN]"` verlor `[IBAN]` schon vor
    /// dem ersten Fenster: `ActionArg::Blackout.to_action(…)` wirft das
    /// Argument weg, weil `Action::Blackout` kein Textfeld hat. Wer in der
    /// Trefferliste der Oberfläche dann auf „Ersetzen“ umstellte, bekam die
    /// Vorgabe statt seines Textes — und nichts wies darauf hin, dass der
    /// Schalter je da war.
    ///
    /// Deshalb steht der Text hier: [`Config::replacement`] ist die *eine*
    /// Antwort auf „womit wird in diesem Lauf ersetzt?“, für beide Programme.
    pub replace_with: String,
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
    /// Obergrenze für die Eingabedatei selbst, in Byte (`--max-input-mb`).
    ///
    /// Sie greift **vor** dem Lesen; siehe [`read_input`].
    pub max_input_bytes: u64,
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
            disabled_patterns: Vec::new(),
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
            replace_with: redact_core::DEFAULT_REPLACEMENT.to_string(),
            padding: DEFAULT_PADDING,
            allow_undecodable_images: false,
            max_decoded_image_bytes: redact_pdf::image::DEFAULT_MAX_DECODED_IMAGE_BYTES,
            limits: Limits::default(),
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
            max_candidates: DEFAULT_MAX_CANDIDATES,
            password: None,
            theme: settings::THEMES[0].to_string(),
        }
    }
}

impl Config {
    /// Der Ersatztext dieses Laufs — die **eine** Quelle für beide Programme.
    ///
    /// Steht in [`Config::action`] bereits ein Text (`--action replace`), gilt
    /// der; sonst der mitgeführte [`Config::replace_with`]. Beide Wege enden
    /// bei derselben Zeichenkette, wenn `--action replace --replace-with X`
    /// gesetzt war — die Fallunterscheidung ist nur dafür da, dass eine von
    /// Hand gebaute `Config` (die Oberfläche und ihre Tests bauen welche) nicht
    /// zwei Felder gleichzeitig pflegen muss.
    ///
    /// Ohne beides ist es [`redact_core::DEFAULT_REPLACEMENT`].
    pub fn replacement(&self) -> &str {
        self.action
            .replacement()
            .unwrap_or(self.replace_with.as_str())
    }

    /// Die abgeschalteten Muster dieses Laufs — die **eine** Lesart von
    /// [`Config::disabled_patterns`].
    ///
    /// Leerraum wird abgeschnitten, leere Einträge und Dubletten fallen weg:
    /// `--disable-pattern date_de,` und `--disable-pattern " date_de "` meinen
    /// dasselbe wie `--disable-pattern date_de`. Ein leerer Eintrag nennt
    /// keinen Namen, über den sich jemand täuschen könnte — anders als ein
    /// falsch geschriebener, und der wird abgelehnt (siehe
    /// [`disable_patterns`]).
    pub fn disabled_pattern_ids(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = Vec::new();
        for id in &self.disabled_patterns {
            let id = id.trim();
            if !id.is_empty() && !ids.contains(&id) {
                ids.push(id);
            }
        }
        ids
    }
}

/// Kopfzeile jeder Meldung über abgeschaltete automatische Erkennung.
///
/// Sie steht im unveränderlichen Teil beider Sätze aus [`detection_notice`] und
/// ist damit die Textmarke, an der [`crate::coverage`] die Meldung einordnet.
pub const DETECTION_NOTICE: &str = "Automatische Erkennung:";

/// Was dieser Lauf **nicht** gesucht hat — als Satz, oder `None`.
///
/// ## Warum es diese Meldung gibt
///
/// Abschaltbare Erkennung ist bequem und genau deshalb gefährlich: eine Datei,
/// die mit `--no-patterns` durchgelaufen ist, sieht in jeder Zahl aus wie eine
/// vollständig geprüfte („0 Treffer“ heißt dann nicht „nichts gefunden“,
/// sondern „nicht gesucht“). Wer sie später in die Hand bekommt, kann den
/// Unterschied an der Ausgabe nicht sehen.
///
/// Dieser Satz ist der Unterschied. Er geht denselben Weg wie jede andere
/// Warnung des Laufs — Konsole, Statuszeile der Oberfläche, `warnings` im
/// Audit-Log — und steht zusätzlich als Struktur in
/// [`crate::audit::PatternRecord`].
///
/// ## Warum er den Rückgabewert nicht anhebt
///
/// Er ist **keine** Deckungslücke (siehe [`crate::coverage`]): die Analyse hat
/// das Dokument vollständig gelesen, sie hat nur nach weniger gesucht — und das
/// auf ausdrückliche Anweisung. Ein Rückgabewert 3 bei jedem Lauf mit
/// `--no-patterns` machte den Wert für die Fälle wertlos, für die es ihn gibt
/// (ein Font ohne `/ToUnicode`, ein ungelesenes XObject).
pub fn detection_notice(config: &Config) -> Option<String> {
    if config.no_patterns {
        return Some(format!(
            "{DETECTION_NOTICE} abgeschaltet (--no-patterns). Es wurde nach keinem \
             einzigen Muster gesucht; geschwärzt ist nur, was von Hand oder über die \
             Buchungsliste angegeben war. Eine IBAN, die niemand markiert hat, steht \
             unverändert in der Ausgabe."
        ));
    }
    let off = config.disabled_pattern_ids();
    if off.is_empty() {
        return None;
    }
    Some(format!(
        "{DETECTION_NOTICE} {} Muster abgeschaltet ({}). Wonach diese Muster gesucht \
         hätten, wurde in diesem Lauf nicht gesucht — solche Stellen stehen unverändert \
         in der Ausgabe.",
        off.len(),
        off.join(", ")
    ))
}

/// Die Muster dieses Laufs mit ihrem tatsächlichen Zustand.
///
/// Genau die Liste, mit der [`collect_regions_for`] sucht: Vorgabe,
/// `--patterns`, `--patterns-config` und die Abschaltungen sind darin bereits
/// verrechnet. Die Oberfläche zeichnet daraus ihre Kästchen — ohne diese
/// Funktion müsste sie den Zustand aus vier Quellen selbst zusammenrechnen,
/// also ein zweites Mal, mit der üblichen Folge.
///
/// **Nicht billig**: dabei werden alle regulären Ausdrücke übersetzt. Die
/// Oberfläche ruft es deshalb bei einer Änderung und nicht in jedem Bild.
pub fn pattern_states(config: &Config) -> Result<Vec<redact_patterns::PatternDef>> {
    Ok(pattern_matcher(config)?.defs().to_vec())
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

impl Outcome {
    /// Die Warnungen, die heißen „für diesen Teil des Dokuments kann ich nicht
    /// einstehen“ — siehe [`crate::coverage`].
    ///
    /// Eine Teilmenge von [`Outcome::warnings`]; sie wird nicht getrennt
    /// gespeichert, damit es keine zweite Liste gibt, die von der ersten
    /// abweichen könnte.
    pub fn coverage_gaps(&self) -> Vec<&str> {
        self.warnings
            .iter()
            .filter(|w| crate::coverage::is_coverage_gap(w))
            .map(String::as_str)
            .collect()
    }

    /// Hat die Analyse das ganze Dokument gesehen?
    ///
    /// `false` heißt **nicht** „es ist etwas schiefgegangen“, sondern: der Lauf
    /// ist durchgelaufen und hat geschrieben, kann aber für einen Teil der
    /// Datei nicht sagen, ob dort etwas stehen geblieben ist. Genau das ist der
    /// Unterschied zwischen Rückgabewert 0 und 3.
    pub fn fully_inspected(&self) -> bool {
        self.coverage_gaps().is_empty()
    }
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
/// ## Die Grenzen gelten auch hier — sie greifen nur zweistufig
///
/// Die Vorprüfung [`redact_pdf::document::prescan`] misst Rohbytes. An einer
/// verschlüsselten Datei sieht sie deshalb nur die Hälfte:
///
/// | | verschlüsselt? | von `prescan` auf den Rohbytes messbar |
/// |---|---|---|
/// | Objektstruktur (`<<`, `[`, Namen, Zahlen) | nein | ja |
/// | Zeichenketten und **Streams** | ja | nein — es ist Rauschen |
///
/// Der erste Anlauf oben hat die Rohbytes also bereits geprüft, und was dort
/// zu sehen war, ist geprüft: eine Datei mit 200 000 offenen `[` in einem
/// gewöhnlichen Objekt fällt schon dort durch, verschlüsselt oder nicht.
/// Nicht zu sehen war der Inhalt der Streams — und genau dort liegen beide
/// Bomben, gegen die die Budgets gedacht sind.
///
/// Deshalb läuft die Prüfung nach der Entschlüsselung ein zweites Mal, jetzt
/// auf dem entschlüsselten Dokument (siehe [`check_limits_after_decryption`]).
/// Vorher galten `--max-decompressed-mb`, `--max-parsed-mb` und die
/// Tiefengrenze für eine verschlüsselte Datei überhaupt nicht. Gemessen an
/// einer 196 kB großen Datei mit einem 64 MB entpackenden Content-Stream:
/// ohne Passwort Ablehnung in 0,0 s; **mit** Passwort Spitzenspeicher über
/// 15 GB und Abbruch durch den OOM-Killer, mit `ulimit -v` stattdessen
/// SIGABRT. Dieselbe Datei mit 200 000-facher Verschachtelung lief mit
/// Passwort auf Rückgabewert 0 durch und schrieb eine Ausgabe, in der die
/// IBAN unverändert stand — unverschlüsselt wurde sie abgelehnt.
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
    check_limits_after_decryption(&doc, &config.limits)?;
    Ok(doc)
}

/// Wendet die Budgets und die Tiefengrenze auf ein **entschlüsseltes**
/// Dokument an.
///
/// ## Wie
///
/// Das entschlüsselte Dokument wird serialisiert und die Vorprüfung läuft über
/// diese Bytes. Das ist keine Umständlichkeit, sondern der Punkt: die Streams
/// stehen dort als das, was sie sind — komprimiert, aber nicht mehr
/// verschlüsselt —, und damit misst hier **derselbe Code mit denselben Grenzen
/// und denselben Meldungen** wie bei einer unverschlüsselten Datei. Eine
/// zweite Buchhaltung neben [`redact_pdf::document::prescan`] wäre eine
/// zweite Stelle, an der die Zahlen auseinanderlaufen könnten.
///
/// Teuer ist das nicht: serialisiert wird der komprimierte Zustand, für die
/// gemessene Bombe rund 200 kB. Das Auspacken übernimmt die Vorprüfung, und
/// die packt von vornherein nur bis zum Budget aus.
///
/// ## Wann es zu spät wäre
///
/// Erst *nach* dem Laden zu prüfen ist nur deshalb vertretbar, weil das Laden
/// selbst billig ist: `lopdf` legt Streams als Rohbytes ab und packt sie nicht
/// aus. Der teure Teil ist das, was danach kommt — die Zerlegung des
/// Seiteninhalts in Operationen (rund 60 Byte Arbeitsspeicher je Byte
/// Stream) — und der kommt erst nach dieser Prüfung.
pub fn check_limits_after_decryption(doc: &Document, limits: &Limits) -> Result<()> {
    let bytes = save_to_bytes(doc)?;
    prescan(&bytes, limits).map_err(|e| match e {
        // Der Zusatz sagt, welcher der beiden Durchgänge angeschlagen hat —
        // die Datei sah von außen harmlos aus, und das gehört in die Meldung.
        //
        // Bewusst „entschlüsselt“ und nicht „verschlüsselt“: [`password_required`]
        // sucht nach letzterem, und die Oberfläche fragte sonst wieder nach
        // einem Passwort, das längst gepasst hat.
        RedactError::Pdf(msg) => RedactError::Pdf(format!("entschlüsselt gilt weiter: {msg}")),
        other => other,
    })
}

/// Liest die Eingabedatei — mit einer Obergrenze **vor** dem ersten Byte.
///
/// ## Warum die Reihenfolge zählt
///
/// `std::fs::read` legt einen Puffer in Dateigröße an und füllt ihn. Wie groß
/// der wird, stand damit in der Datei, nicht in der Konfiguration: eine dünn
/// belegte Datei (`truncate -s 6G`) belegt 4 kB auf der Platte und 6 GB im
/// Speicher. Gemessen wurden 6 149 MB Spitzenspeicher, bevor überhaupt
/// feststand, dass es kein PDF ist.
///
/// Hier wird deshalb zuerst gefragt und dann gelesen:
///
/// 1. Es muss eine **gewöhnliche Datei** sein. Eine benannte Pipe hat die
///    Länge 0 und liefert trotzdem endlos.
/// 2. Die Länge muss unter `max_bytes` liegen.
/// 3. Gelesen wird über einen **begrenzten** Leser. Das ist kein Gürtel zum
///    Hosenträger: zwischen der Frage und dem Lesen kann die Datei wachsen,
///    und über einen `/proc`-Pfad oder ein Netzdateisystem lügt die Länge auch
///    ohne Zutun.
///
/// Die Meldung nennt die Datei beim Namen. Im Stapel nennt
/// `redact_cli::batch` sie zusätzlich schon *vorher*: wer zwanzig Dateien
/// laufen lässt, soll nicht raten müssen, an welcher es hängt.
pub fn read_input(path: &Path, max_bytes: u64) -> Result<Vec<u8>> {
    use std::io::Read;

    let name = redact_core::safe_path(path);
    let meta = std::fs::metadata(path)
        .map_err(|e| RedactError::Pdf(format!("{name}: nicht lesbar: {e}")))?;
    if !meta.is_file() {
        return Err(RedactError::Pdf(format!(
            "{name}: keine gewöhnliche Datei. redact-rs liest nur Dateien — \
             eine Pipe oder ein Gerät hätte keine Größe, an der sich eine \
             Grenze festmachen ließe."
        )));
    }
    if meta.len() > max_bytes {
        return Err(RedactError::Pdf(format!(
            "{name}: {} MB groß, erlaubt sind {} MB (--max-input-mb). \
             Die Datei wird zum Prüfen der Prüfsumme in einem Stück gelesen; \
             ohne diese Grenze bestimmte die Datei, wie viel Arbeitsspeicher \
             das Werkzeug belegt.",
            meta.len() / (1024 * 1024),
            max_bytes / (1024 * 1024)
        )));
    }

    let file = std::fs::File::open(path)
        .map_err(|e| RedactError::Pdf(format!("{name}: nicht lesbar: {e}")))?;
    // `max_bytes + 1`: so ist eine Datei, die zwischen Frage und Lesen
    // gewachsen ist, am Ergebnis zu erkennen statt am Speicherverbrauch.
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| RedactError::Pdf(format!("{name}: nicht lesbar: {e}")))?;
    if bytes.len() as u64 > max_bytes {
        return Err(RedactError::Pdf(format!(
            "{name}: die Datei ist während des Lesens über die Grenze von \
             {} MB hinaus gewachsen (--max-input-mb).",
            max_bytes / (1024 * 1024)
        )));
    }
    Ok(bytes)
}

/// Der Satz hinter der Meldung „zu groß“ für eine Review-Datei bzw. eine
/// Regionsliste: woher die Grenze kommt und was zu tun ist.
///
/// Der wahrscheinlichste Fall ist nicht die zu große Review-Datei, sondern der
/// falsche Pfad hinter dem Schalter — beide nehmen JSON, und beide stehen in
/// derselben Kommandozeile wie die Eingabe-PDF. Genau danach fragt der Satz.
const AUX_LIMIT_HINT: &str = "Für eine Review-Datei oder eine Regionsliste ist das eine feste \
     Grenze und keine Einstellung: gemessen sind rund 600 Byte je geprüfter Stelle, 16 MB \
     fassen also gut 25 000 — von Hand geprüft werden Dutzende bis Hunderte. Zeigt \
     `--apply-review` bzw. `--manual-regions` wirklich auf die JSON-Datei und nicht auf die \
     PDF-Datei?";

/// Liest eine Hilfsdatei als Text — mit der Obergrenze **vor** dem ersten
/// gelesenen Byte.
///
/// ## Warum das hier steht und nicht zweimal daneben
///
/// Zwei Aufrufer lesen dieselbe Sorte Datei: der Zweig `--apply-review` in
/// [`run`] und [`load_manual_regions`]. Beide brauchen aus den Bytes einen
/// `String`, und beide brauchen dafür dieselbe Meldung — geschrieben stünde
/// das `from_utf8` sonst zweimal da, und die zweite Fassung liefe der ersten
/// über kurz oder lang davon.
///
/// ## Erst fragen, dann lesen
///
/// Gelesen wird über [`redact_core::read_limited`], also erst, wenn feststeht,
/// dass hier eine **gewöhnliche Datei** unterhalb von
/// [`redact_core::MAX_AUX_FILE_BYTES`] liegt. Vorher stand an beiden Stellen
/// ein `std::fs::read_to_string`, und das legt einen Puffer in Dateigröße an:
/// eine dünn belegte Datei mit 6 GB Nennlänge (4 kB auf der Platte) kostete
/// hinter `--apply-review` 6 157 MB Spitzenspeicher und 20 s, hinter
/// `--manual-regions` 6 160 MB und 12 s — jedes Mal, bevor auch nur feststand,
/// dass es kein JSON ist. Eine benannte Pipe lief endlos weiter; abgebrochen
/// wurde erst durch die Zeitschranke von außen.
///
/// ## Warum [`RedactError::Parse`]
///
/// Weil daneben `ReviewFile::from_json` genau diese Fehlerart liefert: für die
/// Bedienende ist „die Datei taugt nicht“ eine Aussage, gleich ob sie zu groß
/// ist oder krummes JSON enthält, und die Kommandozeile leitet daraus ihren
/// Rückgabewert ab. Vorher kam hier ein nackter [`RedactError::Io`] heraus —
/// **ohne den Dateinamen**, weil `std::io::Error` den Pfad nicht mitführt.
fn read_aux_text(path: &Path, hint: &str) -> Result<String> {
    let bytes = redact_core::read_limited(path, redact_core::MAX_AUX_FILE_BYTES, hint)
        .map_err(RedactError::Parse)?;
    // Eigene Meldung statt der von `String::from_utf8`: die nennt die Stelle
    // im Dateiinhalt, und der Inhalt gehört hier nicht in die Ausgabe — der
    // Pfad kommt von außen und kann auf eine fremde Datei zeigen.
    String::from_utf8(bytes).map_err(|_| {
        RedactError::Parse(format!(
            "{}: keine UTF-8-Datei. Erwartet wird JSON, also Text.",
            redact_core::safe_path(path)
        ))
    })
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
    //
    // `read_input` statt `std::fs::read`: gelesen wird erst, wenn feststeht,
    // dass es eine gewöhnliche Datei unterhalb von `--max-input-mb` ist.
    let bytes = read_input(&config.input, config.max_input_bytes)?;
    outcome.input_sha256 = sha256_bytes(&bytes);
    let mut doc = load_document(&bytes, config).map_err(|e| match e {
        RedactError::Pdf(msg) => RedactError::Pdf(format!("{}: {msg}", config.input.display())),
        other => other,
    })?;
    outcome.pages = redact_pdf::page_count(&doc);

    // 2./3./4./5. Analyse — oder eine bereits geprüfte Review-Datei.
    let (redactions, blocked) = match &config.apply_review {
        Some(path) => {
            // `read_aux_text` statt `std::fs::read_to_string`: gelesen wird
            // erst, wenn feststeht, dass es eine gewöhnliche Datei unterhalb
            // von `MAX_AUX_FILE_BYTES` ist — so wie es die Eingabe-PDF eine
            // Handvoll Zeilen weiter oben längst hält.
            let data = read_aux_text(path, AUX_LIMIT_HINT)?;
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

            // Abgeschaltete Erkennung gehört auch dann gesagt, wenn der Lauf
            // gar nicht bis zum Schwärzen kommt: `--review` ist die Stelle, an
            // der jemand die Trefferliste prüft — und „0 Treffer“ heißt dort
            // ohne diesen Satz „nichts gefunden“ statt „nicht gesucht“.
            // [`apply`] trägt ihn für den gewöhnlichen Weg ein; doppelt wird er
            // nicht, `push_warnings` lässt keine Dubletten zu.
            push_warnings(
                &mut outcome.warnings,
                detection_notice(config).into_iter().collect(),
            );

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

    // Ganz vorn und für beide Programme an derselben Stelle: wonach dieser Lauf
    // **nicht** gesucht hat. Die Oberfläche kommt nur hier vorbei (sie hat kein
    // `run`), und die Reihenfolge der Warnungen muss zwischen beiden Wegen
    // gleich bleiben — `cli_and_gui_agree` vergleicht das Log Feld für Feld.
    let patterns = PatternRecord::of(config);
    push_warnings(
        &mut outcome.warnings,
        detection_notice(config).into_iter().collect(),
    );

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
                patterns: &patterns,
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

    // 5. Pattern-Matching — abschaltbar, ganz ([`Config::no_patterns`]) und je
    //    Muster ([`Config::disabled_patterns`]).
    //
    // Gebaut wird der Sucher **auch dann**, wenn ohnehin nichts gesucht wird:
    // sonst bliebe ein Tippfehler in `--disable-pattern` ausgerechnet neben
    // `--no-patterns` wirkungslos, und ein wirkungsloser Name ist genau die
    // falsche Entwarnung, gegen die dieser Schalter gedacht ist. Es kostet das
    // Übersetzen von zwölf Ausdrücken, nicht mehr.
    let matcher = pattern_matcher(config)?;
    if !config.no_patterns {
        regions.extend(matcher.find_matches(runs)?);
    }

    check_candidate_budget(config, regions.len())?;
    Ok(regions)
}

/// Baut den Muster-Sucher dieses Laufs.
///
/// **Die eine Stelle**, an der `--patterns`, `--patterns-config`,
/// `--disable-pattern` und `--min-confidence` zusammenkommen — Kommandozeile
/// und Oberfläche gehen beide hier durch.
fn pattern_matcher(config: &Config) -> Result<PatternMatcher> {
    let matcher = match &config.patterns_config {
        Some(path) => PatternMatcher::from_config_file(path)?,
        None => PatternMatcher::new(&config.patterns)?,
    };
    let matcher = disable_patterns(matcher, config)?;
    // `--min-confidence` gewinnt gegen die Schwelle aus der
    // Konfigurationsdatei: die Kommandozeile ist die spätere Anweisung.
    match config.min_confidence {
        Some(min) => matcher.with_min_confidence(min),
        None => Ok(matcher),
    }
}

/// Schaltet die in [`Config::disabled_patterns`] genannten Muster ab.
///
/// ## Warum ein unbekannter Name den Lauf beendet
///
/// `--disable-pattern iban` (statt `iban_de`) still zu übergehen hätte zwei
/// Lesarten, und beide sind schlecht: entweder hält der Aufrufende das Muster
/// für abgeschaltet, während es weiterläuft — dann schwärzt der Lauf mehr als
/// gedacht, was ärgerlich ist —, oder er hält es für abgeschaltet und es *ist*
/// eines mit ähnlichem Namen betroffen. Vor allem aber wäre eine Angabe ohne
/// Wirkung eine Angabe, deren Wirkung niemand mehr nachvollzieht. Deshalb:
/// [`RedactError::Config`], also Rückgabewert 2, mit der Liste der gültigen
/// Namen — abgeschaltet wird in diesem Fall gar nichts.
///
/// ## Wie
///
/// Die Definitionen bekommen `enabled = false` und der Sucher wird daraus neu
/// gebaut; `PatternMatcher::with_defs` übersetzt abgeschaltete Muster erst gar
/// nicht. Die Schwelle aus einer Musterkonfiguration muss dabei ausdrücklich
/// mitgenommen werden — ein neuer Sucher fängt bei der Vorgabe an, und ein
/// `--disable-pattern` hätte sonst nebenbei `min_confidence` einer YAML-Datei
/// zurückgesetzt.
fn disable_patterns(matcher: PatternMatcher, config: &Config) -> Result<PatternMatcher> {
    let off = config.disabled_pattern_ids();
    if off.is_empty() {
        return Ok(matcher);
    }

    let known = known_pattern_ids(&matcher);
    for id in &off {
        if !known.iter().any(|k| k == id) {
            return Err(RedactError::Config(format!(
                "--disable-pattern: „{}“ ist kein bekanntes Muster. Gültig sind: {}. \
                 (`redact-rs --list-patterns` zeigt sie mit Beschreibung.) Es wurde \
                 nichts abgeschaltet und nichts geschwärzt: ein übergangener Name sähe \
                 aus wie eine Abschaltung und wäre keine.",
                redact_core::safe_text(id),
                known.join(", ")
            )));
        }
    }

    let min = matcher.min_confidence();
    let defs = matcher
        .defs()
        .iter()
        .cloned()
        .map(|mut def| {
            if off.contains(&def.id.as_str()) {
                def.enabled = false;
            }
            def
        })
        .collect();
    PatternMatcher::with_defs(defs)?.with_min_confidence(min)
}

/// Die Musternamen, die dieser Lauf kennt: die eingebauten plus die aus
/// `--patterns-config`.
///
/// **Nicht** nur die gerade ausgewählten. `--patterns iban_de` engt die Auswahl
/// ein, macht `bic` aber nicht zu einem unbekannten Namen — ein Fehler an
/// dieser Stelle wäre keine Warnung vor einem Tippfehler, sondern eine vor
/// einer Doppelung.
fn known_pattern_ids(matcher: &PatternMatcher) -> Vec<String> {
    let mut ids: Vec<String> = redact_patterns::builtin_pattern_ids()
        .into_iter()
        .map(str::to_string)
        .collect();
    for def in matcher.defs() {
        if !ids.contains(&def.id) {
            ids.push(def.id.clone());
        }
    }
    ids
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
///
/// ## Erst fragen, dann lesen
///
/// Die Datei geht durch [`read_aux_text`] und damit durch
/// [`redact_core::read_limited`]: sie muss eine gewöhnliche Datei unterhalb von
/// [`redact_core::MAX_AUX_FILE_BYTES`] sein, bevor ein Byte gelesen wird.
pub fn load_manual_regions(
    path: &Path,
    document_sha: &str,
    allow_unverified: bool,
) -> Result<Vec<Region>> {
    // Erst fragen, dann lesen — siehe [`read_aux_text`]. Beide Formate, die
    // hier durchgehen, sind JSON von Hand gepflegter Größe; die Grenze greift
    // vor `serde_json`, und darauf kommt es an.
    let data = read_aux_text(path, AUX_LIMIT_HINT)?;
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
#[derive(Debug)]
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
    // Der Namenszusatz bestimmt jeden Pfad, der hier gleich geprüft wird —
    // also gehört er selbst vor die Prüfung. Die Einstellungsdatei prüft ihn
    // schon beim Lesen; hier gilt es zusätzlich für `--output-suffix`, für das
    // Feld in der Seitenleiste der Oberfläche und für jeden künftigen Weg.
    redact_core::check_output_suffix(&config.output_suffix)?;

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

    // ------------------- Befund 3: die Grenzen gelten auch entschlüsselt

    /// **Die Auflage:** die verschlüsselte Dekompressionsbombe endet mit einem
    /// klaren Fehler statt mit SIGKILL oder SIGABRT.
    ///
    /// Speicher lässt sich in einem Test nur schlecht messen — ein
    /// Prozessabbruch reißt den Testläufer mit, statt einen Wert zu liefern.
    /// Gemessen wird deshalb, was sich messen lässt: **abgelehnt oder
    /// angenommen**, und ob die Meldung die überschrittene Größe nennt. Die
    /// Speicherzahlen (vorher 3 876 MB und SIGABRT, nachher 22,8 MB und
    /// Rückgabewert 1) stehen in `SECURITY.md`.
    #[test]
    fn an_encrypted_decompression_bomb_is_refused_instead_of_eating_the_machine() {
        let config = encrypted_config(Some(testing::ENCRYPTED_PDF_PASSWORD));
        let error = load_document(testing::ENCRYPTED_BOMB_PDF, &config)
            .expect_err("die Bombe muss abgelehnt werden")
            .to_string();

        // Die Meldung nennt das Budget, das sie gerissen hat …
        assert!(error.contains("Budget"), "{error}");
        assert!(
            error.contains(&format!(
                "{}",
                config.limits.max_parsed_bytes / (1024 * 1024)
            )),
            "die Meldung nennt die Grenze nicht: {error}"
        );
        // … und sie sagt, dass es an der entschlüsselten Datei gemessen wurde.
        assert!(error.contains("entschlüsselt gilt weiter"), "{error}");
        // Aber sie darf nicht wie „hier fehlt ein Passwort“ aussehen: die
        // Oberfläche fragte sonst nach einem Passwort, das gerade gepasst hat.
        let error = load_document(testing::ENCRYPTED_BOMB_PDF, &config).unwrap_err();
        assert!(
            !password_required(&error),
            "die Oberfläche fragt nach dem Passwort statt abzubrechen: {error}"
        );
    }

    /// **Die Auflage:** die verschlüsselte Verschachtelungsbombe läuft nicht
    /// mehr mit Rückgabewert 0 durch.
    ///
    /// Vorher: Rückgabewert 0, Ausgabe geschrieben, IBAN unverändert darin.
    /// Das ist der schlimmere der beiden Fälle — ein Absturz fällt auf, eine
    /// Datei mit „0 Schwärzungen“ nicht.
    #[test]
    fn an_encrypted_nesting_bomb_no_longer_passes_as_a_clean_document() {
        let config = encrypted_config(Some(testing::ENCRYPTED_PDF_PASSWORD));
        let error = load_document(testing::ENCRYPTED_NESTING_BOMB_PDF, &config)
            .expect_err("die Verschachtelung muss auffallen")
            .to_string();
        assert!(error.contains("Verschachtelungstiefe"), "{error}");
        assert!(
            error.contains(&config.limits.max_nesting_depth.to_string()),
            "{error}"
        );
    }

    /// Die eigenen Grenzen gelten auch dahinter: wer sie hochsetzt, kommt an
    /// derselben Datei durch — und wer sie herunterzieht, an einer harmlosen
    /// nicht mehr.
    ///
    /// Ohne diese Gegenprobe könnte die Prüfung eine feste Zahl benutzen und
    /// der Test bliebe grün.
    #[test]
    fn the_check_after_decryption_uses_the_configured_limits() {
        let mut config = encrypted_config(Some(testing::ENCRYPTED_PDF_PASSWORD));

        // Großzügiger als die Bombe: sie kommt durch.
        config.limits.max_parsed_bytes = (testing::ENCRYPTED_BOMB_MB + 1) * 1024 * 1024;
        assert!(
            load_document(testing::ENCRYPTED_BOMB_PDF, &config).is_ok(),
            "mit ausreichendem Budget muss dieselbe Datei laden"
        );

        // Enger als das harmlose Prüf-PDF: auch das fällt durch.
        config.limits = Limits {
            max_parsed_bytes: 1,
            ..Limits::default()
        };
        let error = load_document(testing::ENCRYPTED_PDF, &config)
            .expect_err("mit Budget 1 kommt nichts durch")
            .to_string();
        assert!(error.contains("entschlüsselt gilt weiter"), "{error}");
    }

    /// Und die wichtigste Gegenprobe: ein gewöhnliches verschlüsseltes
    /// Dokument geht weiterhin durch. Eine Grenze, die alles ablehnt, ist
    /// keine Härtung, sondern ein Ausfall.
    #[test]
    fn an_ordinary_encrypted_document_still_opens() {
        let config = encrypted_config(Some(testing::ENCRYPTED_PDF_PASSWORD));
        let doc = load_document(testing::ENCRYPTED_PDF, &config).expect("muss weiterhin laden");
        assert_eq!(redact_pdf::page_count(&doc), 1);
        check_limits_after_decryption(&doc, &config.limits).expect("harmlos");
    }

    // ------------------------- Befund 4: Obergrenze für die Eingabedatei

    fn tempdir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("redact-eingabe-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// **Die Auflage:** die Grenze greift *vor* dem Lesen.
    ///
    /// Gemessen wird das an einer dünn belegten Datei: sie belegt 4 kB auf der
    /// Platte und meldet 6 GB Länge. Wird sie gelesen, kostet das 6 GB
    /// Arbeitsspeicher (gemessen: 6 149 MB); wird sie an ihrer *Angabe*
    /// abgelehnt, kostet es nichts. Der Unterschied ist im Test daran zu
    /// sehen, dass die Meldung die Größe nennt — dafür muss sie vor dem Lesen
    /// bekannt gewesen sein.
    #[test]
    fn a_file_larger_than_the_budget_is_refused_before_it_is_read() {
        let dir = tempdir("gross");
        let path = dir.join("riesig.pdf");
        let file = std::fs::File::create(&path).unwrap();
        // Dünn belegt: 6 GB Nennlänge, 0 Byte geschrieben.
        file.set_len(6 * 1024 * 1024 * 1024).unwrap();
        drop(file);

        let error = read_input(&path, DEFAULT_MAX_INPUT_BYTES)
            .expect_err("6 GB müssen abgelehnt werden")
            .to_string();
        assert!(error.contains("6144 MB"), "{error}");
        assert!(error.contains("512 MB"), "{error}");
        assert!(
            error.contains("riesig.pdf"),
            "die Datei wird nicht genannt: {error}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Und die Gegenprobe: eine Datei unterhalb der Grenze wird vollständig
    /// gelesen, Byte für Byte wie mit `std::fs::read`.
    #[test]
    fn a_file_within_the_budget_is_read_completely() {
        let dir = tempdir("klein");
        let path = dir.join("klein.pdf");
        let content: Vec<u8> = (0..=255u8).cycle().take(300_000).collect();
        std::fs::write(&path, &content).unwrap();

        assert_eq!(read_input(&path, DEFAULT_MAX_INPUT_BYTES).unwrap(), content);
        // Genau auf der Grenze ist noch erlaubt, ein Byte darüber nicht.
        assert!(read_input(&path, content.len() as u64).is_ok());
        assert!(read_input(&path, content.len() as u64 - 1).is_err());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Ein Verzeichnis (oder eine Pipe) hat keine Größe, an der sich eine
    /// Grenze festmachen ließe — und wird deshalb gar nicht erst gelesen.
    #[test]
    fn only_regular_files_are_read() {
        let dir = tempdir("art");
        let error = read_input(&dir, DEFAULT_MAX_INPUT_BYTES)
            .expect_err("ein Verzeichnis ist keine Eingabedatei")
            .to_string();
        assert!(error.contains("gewöhnliche Datei"), "{error}");
        std::fs::remove_dir_all(&dir).ok();
    }

    // --------------- Befund 5: Namenszusatz mit Pfadanteilen, zentral

    /// Vor dem ersten Schreibziel wird der Namenszusatz geprüft — egal, ob er
    /// aus der Einstellungsdatei, von `--output-suffix` oder aus dem Feld in
    /// der Seitenleiste kommt.
    #[test]
    fn plan_outputs_refuses_a_suffix_with_a_path_component() {
        let config = Config {
            input: PathBuf::from("/daten/b3/eins.pdf"),
            output_suffix: "/../../ziel/alle".to_string(),
            ..Config::default()
        };
        let error = plan_outputs(&config)
            .expect_err("der Zusatz muss auffallen")
            .to_string();
        assert!(error.contains("Namenszusatz"), "{error}");
        // Und es wurde dabei nichts angelegt: `check_target` legt sonst mit
        // `create_dir_all` die Zwischenverzeichnisse an.
        assert!(
            !Path::new("/daten").exists(),
            "ein Verzeichnis wurde angelegt"
        );
    }

    // ------------------------------------------------- Befund #76: Ersatztext

    /// Der Ersatztext hat **eine** Antwort, egal von welcher Seite gefragt
    /// wird.
    ///
    /// Die drei Fälle sind genau die drei, die es gibt: der Lauf ersetzt
    /// ohnehin (`action` trägt den Text), der Lauf schwärzt schwarz und hat
    /// trotzdem einen Text mitbekommen (`--action blackout --replace-with X` —
    /// der Fall, der verlorenging), und der Lauf hat gar nichts gesagt.
    #[test]
    fn the_replacement_has_one_answer_whichever_action_is_set() {
        let mit_action = Config {
            action: Action::Replace("[IBAN]".into()),
            ..Config::default()
        };
        assert_eq!(mit_action.replacement(), "[IBAN]");

        let nur_schalter = Config {
            action: Action::Blackout,
            replace_with: "[IBAN]".into(),
            ..Config::default()
        };
        assert_eq!(
            nur_schalter.replacement(),
            "[IBAN]",
            "--replace-with geht bei --action blackout verloren"
        );

        assert_eq!(
            Config::default().replacement(),
            redact_core::DEFAULT_REPLACEMENT
        );
        // Und die Vorgabe ist die aus `redact-core`, nicht eine zweite hier.
        assert_eq!(
            Config::default().replace_with,
            redact_core::DEFAULT_REPLACEMENT
        );
    }

    // ------------------------- Abschaltbare automatische Funde (Aufgabe #81)

    /// Eine Zeile, in der drei verschiedene Muster etwas finden.
    fn mixed_run() -> Vec<TextRun> {
        vec![
            text_run("IBAN DE89 3704 0044 0532 0130 00 BIC COBADEFFXXX"),
            text_run("Kontakt max.mustermann@example.org am 05.01.2026"),
        ]
    }

    /// Welche Muster in diesem Ergebnis vorkommen.
    fn hit_patterns(regions: &[Region]) -> Vec<String> {
        let mut ids: Vec<String> = regions
            .iter()
            .filter_map(|r| match &r.source {
                redact_core::Source::Pattern { pattern_id, .. } => Some(pattern_id.clone()),
                _ => None,
            })
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// **Die Auflage (1): ganz aus heißt kein einziger automatischer Treffer.**
    #[test]
    fn no_patterns_finds_nothing_at_all() {
        let runs = mixed_run();
        let alles_an = collect_regions(&Config::default(), &runs).unwrap();
        assert!(
            hit_patterns(&alles_an).len() >= 3,
            "die Gegenprobe taugt nichts: {:?}",
            hit_patterns(&alles_an)
        );

        let aus = Config {
            no_patterns: true,
            ..Config::default()
        };
        assert!(collect_regions(&aus, &runs).unwrap().is_empty());
    }

    /// **Die Auflage (2): einzeln aus nimmt genau ein Muster heraus.**
    ///
    /// Gemessen wird beides: das abgeschaltete Muster fehlt, und **alle
    /// anderen sind noch da**. Nur die erste Hälfte zu prüfen ließe eine
    /// Abschaltung durchgehen, die nebenbei den ganzen Lauf lahmlegt.
    #[test]
    fn switching_off_one_pattern_leaves_every_other_one_alone() {
        let runs = mixed_run();
        let alle = hit_patterns(&collect_regions(&Config::default(), &runs).unwrap());
        assert!(alle.contains(&"email".to_string()), "{alle:?}");

        let config = Config {
            disabled_patterns: vec!["email".to_string()],
            ..Config::default()
        };
        let übrig = hit_patterns(&collect_regions(&config, &runs).unwrap());
        assert!(!übrig.contains(&"email".to_string()), "{übrig:?}");
        assert_eq!(
            übrig,
            alle.iter()
                .filter(|id| *id != "email")
                .cloned()
                .collect::<Vec<_>>(),
            "abgeschaltet wurde mehr als das eine Muster"
        );
    }

    /// **Die Auflage (3): ein unbekannter Name ist ein Bedienfehler.**
    ///
    /// `RedactError::Config` ist der Rückgabewert 2 der Kommandozeile; die
    /// Meldung muss die gültigen Namen nennen, sonst hilft sie beim Tippfehler
    /// nicht.
    #[test]
    fn an_unknown_pattern_name_is_a_usage_error_naming_the_valid_ones() {
        let config = Config {
            // Der naheliegende Tippfehler: `iban` statt `iban_de`.
            disabled_patterns: vec!["iban".to_string()],
            ..Config::default()
        };
        let error = collect_regions(&config, &mixed_run())
            .expect_err("ein unbekannter Name darf nicht stillschweigend durchgehen");
        assert!(
            matches!(error, RedactError::Config(_)),
            "das muss ein Bedienfehler sein (Rückgabewert 2), ist aber: {error:?}"
        );
        let text = error.to_string();
        assert!(text.contains("iban"), "{text}");
        for id in redact_patterns::builtin_pattern_ids() {
            assert!(text.contains(id), "{id} fehlt in der Meldung: {text}");
        }
    }

    /// Auch neben `--no-patterns` fällt der Tippfehler auf.
    ///
    /// Sonst wäre ausgerechnet die Kombination „alles aus, dieses eine
    /// besonders“ die Stelle, an der ein Name wirkungslos verschwindet.
    #[test]
    fn an_unknown_name_is_refused_even_when_nothing_would_run_anyway() {
        let config = Config {
            no_patterns: true,
            disabled_patterns: vec!["gibt_es_nicht".to_string()],
            ..Config::default()
        };
        assert!(collect_regions(&config, &mixed_run()).is_err());
    }

    /// Ein Muster aus `--patterns-config` ist ein gültiger Name — und die
    /// Schwelle dieser Datei überlebt das Abschalten.
    ///
    /// Der zweite Teil ist der Regressionsschutz: der Sucher wird zum
    /// Abschalten neu gebaut, und ein neu gebauter Sucher fängt bei der
    /// Vorgabe-Schwelle an.
    #[test]
    fn a_pattern_from_a_config_file_can_be_switched_off_without_losing_its_threshold() {
        let dir = tempdir("musterdatei");
        let path = dir.join("patterns.yaml");
        std::fs::write(
            &path,
            "min_confidence: 0.25\npatterns:\n  - id: kundennummer\n    \
             regex: 'KdNr\\.? ?[0-9]{5}'\n    confidence: 0.3\n",
        )
        .unwrap();

        let runs = vec![text_run("KdNr 12345 und BLZ 37040044")];
        let config = Config {
            patterns_config: Some(path.clone()),
            ..Config::default()
        };
        let alle = hit_patterns(&collect_regions(&config, &runs).unwrap());
        // Beide leben nur von der abgesenkten Schwelle aus der Datei.
        assert!(alle.contains(&"kundennummer".to_string()), "{alle:?}");
        assert!(alle.contains(&"blz".to_string()), "{alle:?}");

        let config = Config {
            disabled_patterns: vec!["kundennummer".to_string()],
            ..config
        };
        let übrig = hit_patterns(&collect_regions(&config, &runs).unwrap());
        assert!(!übrig.contains(&"kundennummer".to_string()), "{übrig:?}");
        assert!(
            übrig.contains(&"blz".to_string()),
            "die Schwelle aus der Musterdatei ging beim Abschalten verloren: {übrig:?}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// `--patterns` engt die Auswahl ein, macht aber keinen Namen unbekannt.
    #[test]
    fn a_name_outside_the_selection_is_still_a_known_name() {
        let config = Config {
            patterns: vec!["iban_de".to_string()],
            disabled_patterns: vec!["bic".to_string()],
            ..Config::default()
        };
        let übrig = hit_patterns(&collect_regions(&config, &mixed_run()).unwrap());
        assert_eq!(übrig, vec!["iban_de".to_string()]);
    }

    /// Die Ansage über abgeschaltete Erkennung — beide Fälle, und der Fall
    /// „nichts abgeschaltet“ schweigt.
    #[test]
    fn the_notice_says_what_was_switched_off() {
        assert_eq!(detection_notice(&Config::default()), None);

        let ganz = detection_notice(&Config {
            no_patterns: true,
            ..Config::default()
        })
        .expect("ganz aus muss gesagt werden");
        assert!(ganz.starts_with(DETECTION_NOTICE), "{ganz}");
        assert!(ganz.contains("--no-patterns"), "{ganz}");

        let einzeln = detection_notice(&Config {
            disabled_patterns: vec!["date_de".into(), "date_de".into(), "  ".into()],
            ..Config::default()
        })
        .expect("einzeln aus muss gesagt werden");
        assert!(einzeln.starts_with(DETECTION_NOTICE), "{einzeln}");
        // Getrimmt, ohne Dubletten, ohne Leereintrag: „1 Muster“, nicht drei.
        assert!(
            einzeln.contains("1 Muster abgeschaltet (date_de)"),
            "{einzeln}"
        );
    }

    /// Die Liste für die Oberfläche zeigt den *tatsächlichen* Zustand — nicht
    /// die Vorgabe und nicht die Abschaltliste allein.
    #[test]
    fn the_pattern_states_show_what_really_runs() {
        let states = pattern_states(&Config::default()).unwrap();
        let state_of = |states: &[redact_patterns::PatternDef], id: &str| {
            states
                .iter()
                .find(|d| d.id == id)
                .unwrap_or_else(|| panic!("{id} fehlt"))
                .enabled
        };
        // Vorgabe: `iban_de` an, `date_de` aus (zu viele Fehltreffer).
        assert!(state_of(&states, "iban_de"));
        assert!(!state_of(&states, "date_de"));

        let states = pattern_states(&Config {
            disabled_patterns: vec!["iban_de".into()],
            ..Config::default()
        })
        .unwrap();
        assert!(!state_of(&states, "iban_de"));

        // `--patterns date_de` schaltet ein standardmäßig ausgeschaltetes
        // Muster ein — auch das muss die Liste zeigen.
        let states = pattern_states(&Config {
            patterns: vec!["date_de".into()],
            ..Config::default()
        })
        .unwrap();
        assert_eq!(states.len(), 1);
        assert!(state_of(&states, "date_de"));
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
