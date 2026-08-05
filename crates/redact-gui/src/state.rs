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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use redact_core::{
    output_path_with_suffix, resolve_conflicts, sibling_path, Action, BlockedRegion, MatchType,
    Rect, RedactError, Redaction, Region, Result, ReviewFile, ReviewInput, Source, TextRun,
    AUDIT_SUFFIX, REVIEW_SUFFIX,
};
use redact_patterns::PatternDef;
use redact_pdf::{document::sane_page_boxes, PdfExtractor};
use redact_pipeline::{sha256_bytes, Config, Outcome, ReviewIdentity, Secret};

use crate::history::History;
use crate::viewer::{normalize_rotation, PageView};

/// A4 als Rückfallwert, wenn noch kein Dokument geladen ist.
pub const DEFAULT_PAGE_BOX: Rect = Rect {
    ll: redact_core::Point { x: 0.0, y: 0.0 },
    ur: redact_core::Point {
        x: 595.276,
        y: 841.89,
    },
};

/// Statuszeile, wenn ein von der Schutzliste gedeckter Treffer durch eine
/// Handanpassung zur Schwärzung wird.
///
/// Siehe [`AppState::set_region_rect`]: die Wirkung kehrt sich um, und zwar
/// schon bei einem Punkt Verbreiterung. Farbe, Beschriftung und Zahl ändern
/// sich sichtbar mit — der *Grund* stand nirgends.
pub const PROTECTION_OVERRIDDEN: &str =
    "Achtung: Dieser Treffer war durch Ihre Schutzliste gedeckt. Von Hand angepasst \
     überstimmt er sie und wird jetzt geschwärzt — Strg+Z nimmt es zurück.";

/// Statuszeile, wenn ein Pfeiltastendruck eine Region auf einer Seite träfe,
/// die gerade niemand sieht.
///
/// Siehe [`AppState::move_selected`] und [`AppState::resize_selected`]. Der
/// Satz nennt beide Auswege — zu der Seite blättern oder die Auswahl aufheben
/// —, weil sonst nur „es passiert nichts“ übrig bliebe. `page` ist 0-basiert.
pub fn selection_on_other_page(page: usize) -> String {
    format!(
        "Nicht verschoben — die ausgewählte Region liegt auf Seite {}, gezeigt wird \
         eine andere. Zu ihr blättern, oder mit Esc die Auswahl aufheben (dann \
         blättern die Pfeiltasten wieder).",
        page + 1
    )
}

/// Größe eines mit der Tastatur angelegten Rechtecks in Punkt (Breite, Höhe).
///
/// Grob eine Anschriftzeile auf A4 — groß genug, um es auf dem Blatt zu
/// finden, klein genug, um nicht die halbe Seite zu verdecken. Siehe
/// [`AppState::add_region_in_page_middle`].
pub const NEW_REGION_SIZE: (f64, f64) = (200.0, 40.0);

/// Kleinste Kantenlänge, auf die sich ein Rechteck mit der Tastatur schrumpfen
/// lässt.
pub const MIN_REGION_EXTENT: f64 = 2.0;

/// Statuszeile nach [`AppState::add_region_in_page_middle`].
///
/// Nennt **beides**: wo das Rechteck liegt und wie es weitergeht. Ein neu
/// angelegtes Rechteck, das nur „angelegt“ meldet, lässt den Tastaturnutzer
/// vor der Frage stehen, wie er es dorthin bekommt, wo es hingehört.
pub fn new_region_hint(page: usize) -> String {
    format!(
        "Rechteck in der Mitte von Seite {} angelegt und ausgewählt. Pfeiltasten \
         schieben es (mit Umschalt 10 pt), Strg+Pfeil ändert seine Größe, Entf \
         löscht es, Strg+Z nimmt es zurück.",
        page + 1
    )
}

/// Warnung zu einer Seite, deren MediaBox unbrauchbar war.
///
/// Ohne sie sähe die geheilte Seite aus wie jede andere — und was das Fenster
/// zeigt, wäre nicht das, was in der Datei steht. `page` ist 0-basiert.
pub fn healed_page_warning(page: usize) -> String {
    format!(
        "Seite {} nennt eine unbrauchbare Seitengröße; gerechnet und gezeichnet \
         wird mit A4. Prüfen Sie dort besonders genau, ob die Rechtecke sitzen.",
        page + 1
    )
}

/// Vorgabe für den Ersatztext bei [`Action::Replace`].
///
/// **Dieselbe Zeichenkette wie auf der Kommandozeile** (`--replace-with`,
/// Vorgabe in `crates/redact-cli/src/cli.rs`). In der Oberfläche stand hier
/// fest `"[REDACTED]"` — englisch in einer deutschen Oberfläche und anders als
/// das, was ein Lauf ohne `--gui` schreibt. Eine gemeinsame Konstante in
/// Es gibt sie inzwischen in `redact-core` — das ist der richtige Ort, weil
/// Kommandozeile und Oberfläche denselben Wert brauchen und zwei Literale
/// zwangsläufig auseinanderlaufen.
pub use redact_core::DEFAULT_REPLACEMENT;

/// Kleinster und größter erlaubter Zoomfaktor.
pub const MIN_ZOOM: f32 = 0.25;
/// Siehe [`MIN_ZOOM`].
pub const MAX_ZOOM: f32 = 4.0;
/// Faktor je Druck auf „Größer“ bzw. „Kleiner“.
///
/// Multiplikativ, nicht additiv: bei 0,25 ist ein Schritt von 0,05 kaum zu
/// sehen, bei 4,0 ein Sprung.
pub const ZOOM_STEP: f32 = 1.25;

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
    /// Liegt vollständig neben dem Blatt und kann kein Zeichen treffen.
    ///
    /// Selbst gezogen entsteht so etwas nicht ([`AppState::add_manual_region`]
    /// klemmt), über einen Eckgriff auch nicht ([`AppState::set_region_rect`]),
    /// über die Pfeiltasten seit dieser Runde ebenfalls nicht
    /// ([`AppState::move_selected`]). Eine **Review-Datei** kann solche
    /// Rechtecke aber weiterhin mitbringen: sie ist eine Liste von Koordinaten
    /// und wird nicht beschnitten, weil das die Angabe der Nutzerin
    /// stillschweigend veränderte. Also wird sie stattdessen **angesagt** — vor
    /// dem Export und nicht erst als Warnung danach.
    OffPage,
    /// Nennt eine Seite, die es in diesem Dokument nicht gibt.
    ///
    /// Der Nachbarfall von [`HitOutcome::OffPage`], und aus demselben Grund
    /// hier: er kann kein Zeichen entfernen. Er hat trotzdem eine **eigene**
    /// Zeile, weil er etwas anderes zu tun gibt — bei `OffPage` stimmen die
    /// Koordinaten nicht, hier die Seitenzahl. „Liegt neben der Seite“ schickte
    /// den Leser an den Rand von Seite 8, die es gar nicht gibt.
    ///
    /// Entstehen kann das nur über eine Review-Datei oder `--manual-regions`;
    /// die Oberfläche selbst legt kein Rechteck auf einer Seite an, die sie
    /// nicht anzeigen kann.
    MissingPage,
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
            HitOutcome::OffPage => "liegt neben der Seite",
            HitOutcome::MissingPage => "Seite gibt es nicht",
        }
    }
}

/// Ergebnis einer Konfliktauflösung, aufbereitet für die Anzeige.
#[derive(Debug, Clone, PartialEq)]
pub struct HitSummary {
    /// Je Eintrag in [`AppState::regions`] — gleiche Reihenfolge, gleiche Länge.
    pub outcomes: Vec<HitOutcome>,
    /// Anzahl der **Funde**: alles, was geschwärzt werden könnte.
    ///
    /// Schutzeinträge der Negativliste zählen hier **nicht** mit. Sie sind
    /// keine Funde, sondern das Gegenteil — und sie standen trotzdem in der
    /// Zahl vor dem Wort „Treffer“: „2 Treffer · 1 werden geschwärzt“, wobei
    /// der erste gar kein Fund war.
    pub found: usize,
    /// Anzahl der Schutzeinträge (Negativliste).
    pub protecting: usize,
    /// Anzahl derer, die wirklich geschwärzt werden.
    pub redacted: usize,
    /// Anzahl der Einträge, deren Rechteck vollständig neben dem Blatt liegt.
    ///
    /// Sie stecken **nicht** in [`HitSummary::redacted`] — sie können kein
    /// Zeichen entfernen. Siehe [`HitOutcome::OffPage`].
    pub off_page: usize,
    /// Anzahl der Einträge auf einer Seite, die es im Dokument nicht gibt.
    ///
    /// Ebenfalls nicht in [`HitSummary::redacted`], und aus demselben Grund.
    /// Eine eigene Zahl, weil sie etwas anderes zu tun gibt als
    /// [`HitSummary::off_page`] — siehe [`HitOutcome::MissingPage`].
    pub missing_page: usize,
    /// Hat dieser Lauf überhaupt automatisch gesucht?
    ///
    /// **Der Grund, warum das Feld hier steht** und nicht bloß am Schalter in
    /// der Seitenleiste: ohne es lautet [`HitSummary::headline`] bei
    /// abgeschalteter Erkennung „0 Treffer · 0 werden geschwärzt“ — Wort für
    /// Wort dieselbe Zeile wie bei einem Dokument, in dem wirklich nichts
    /// steht. Das ist der eine Satz dieses Programms, der niemals zweideutig
    /// sein darf.
    ///
    /// Es hängt an [`AppState::any_pattern_runs`] und **nicht** am
    /// Hauptschalter: zu „es wurde nicht gesucht“ führen zwei Wege, und der
    /// zweite — jedes einzelne Kästchen abwählen — ließ die Vorwarnung sonst
    /// verschwinden, obwohl genauso wenig gesucht wurde.
    pub automatic: bool,
    /// Steht der Hauptschalter „Automatisch suchen“ auf an?
    ///
    /// Nur dafür da, den **Grund** in [`HitSummary::headline`] richtig zu
    /// benennen: „Automatische Suche AUS“ neben einem gesetzten Häkchen
    /// schickte den Leser an den falschen Schalter.
    pub detection_switch: bool,
    /// Wie viele Muster einzeln abgeschaltet sind.
    pub disabled_patterns: usize,
}

impl HitSummary {
    pub fn outcome(&self, index: usize) -> HitOutcome {
        self.outcomes
            .get(index)
            .copied()
            .unwrap_or(HitOutcome::Duplicate)
    }

    /// Alle Zeilen der Liste — Funde **und** Schutzeinträge.
    pub fn rows(&self) -> usize {
        self.outcomes.len()
    }

    /// Die Zahlen, auf die es ankommt — als Satz.
    ///
    /// Schutzeinträge bekommen einen eigenen Platz, statt die Trefferzahl zu
    /// erhöhen: sie verhindern Schwärzungen, sie sind keine.
    ///
    /// ## Warum die Abschaltung **vor** den Zahlen steht
    ///
    /// Weil sie sie umdeutet. „0 Treffer“ heißt bei eingeschalteter Erkennung
    /// „nichts gefunden“ und bei abgeschalteter „nicht gesucht“ — dieselben
    /// Zeichen, die entgegengesetzte Aussage. Ein Zusatz hinter den Zahlen
    /// käme zu spät: gelesen wird die Zeile von links, und eine Zahl, die man
    /// schon falsch verstanden hat, liest man nicht noch einmal.
    pub fn headline(&self) -> String {
        let mut text = String::new();
        if !self.automatic {
            // Derselbe Vorbehalt, aber am richtigen Bedienelement: der
            // Hauptschalter und die Kästchen darunter führen beide hierher,
            // und wer den falschen genannt bekommt, sucht am falschen Ort.
            text.push_str(if self.detection_switch {
                "Kein Muster läuft — nicht gesucht, nur von Hand: "
            } else {
                "Automatische Suche AUS — nicht gesucht, nur von Hand: "
            });
        }
        text.push_str(&format!(
            "{} Treffer · {} werden geschwärzt",
            self.found, self.redacted
        ));
        if self.protecting > 0 {
            text.push_str(&format!(" · {} geschützt", self.protecting));
        }
        // Vor den Mustern und direkt hinter den Zahlen, die es berichtigt: wer
        // „3 Treffer · 2 werden geschwärzt“ liest, soll im selben Atemzug
        // erfahren, warum aus dem dritten nichts wird.
        if self.off_page > 0 {
            text.push_str(&format!(" · {} neben der Seite", self.off_page));
        }
        // Und direkt daneben der Nachbarfall: eine Seitenzahl, die es nicht
        // gibt. Auch das gehört **vor** den Export und nicht als Warnung
        // danach.
        if self.missing_page > 0 {
            text.push_str(&format!(
                " · {} auf einer Seite, die es nicht gibt",
                self.missing_page
            ));
        }
        // Bei „ganz aus“ ist die Zahl der einzeln abgeschalteten Muster
        // gegenstandslos — es läuft ohnehin keines. Am **Hauptschalter**
        // gemessen und nicht an [`HitSummary::automatic`]: steht der Schalter
        // auf „an“ und sind trotzdem alle Kästchen leer, ist genau diese Zahl
        // die Auskunft, die weiterhilft.
        if self.detection_switch && self.disabled_patterns > 0 {
            text.push_str(&format!(
                " · {} Muster abgeschaltet",
                self.disabled_patterns
            ));
        }
        text
    }
}

/// Unverwechselbare Kennung einer Trefferzeile.
///
/// **Warum es sie gibt**: ein Index in [`AppState::regions`] bezeichnet einen
/// *Platz* in der Liste, keine Sache. Wird die Liste kürzer oder ausgetauscht
/// (löschen, rückgängig, Review laden, neu analysieren), zeigt derselbe Index
/// plötzlich auf eine **andere** Region. Was über mehrere Bilder hinweg gemerkt
/// wird — der laufende Zug am Eckgriff, siehe [`crate::selector::HandleDrag`] —
/// muss deshalb eine Kennung merken und keinen Index. Ein Index, der ins Leere
/// zeigt, fällt auf; ein Index, der auf die falsche Region zeigt, nicht.
///
/// Die Kennung wird beim Anlegen vergeben und beim Kopieren mitgenommen: ein
/// Schnappschuss des Verlaufs enthält dieselben Regionen, nicht bloß gleich
/// aussehende.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RegionId(u64);

impl RegionId {
    /// Die nächste freie Kennung.
    ///
    /// Ein Zähler über das ganze Programm: er muss nur eindeutig sein, nicht
    /// klein und nicht lückenlos. Bei einer Vergabe je Region reicht `u64` für
    /// jede vorstellbare Sitzung.
    fn next() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

/// Eine Region mitsamt ihrem Zustand in der Oberfläche.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotatedRegion {
    /// Kennung dieser Zeile — siehe [`RegionId`].
    pub id: RegionId,
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
    /// Erzeugt einen Eintrag mit den Vorgabewerten (Negativtreffer aus,
    /// Schwärzungsart [`Action::Blackout`]).
    ///
    /// In der Oberfläche wird stattdessen [`AnnotatedRegion::with_action`]
    /// gerufen: dort gilt die Schwärzungsart aus der [`Config`], also das, was
    /// `--action`/`--replace-with` gesetzt haben.
    pub fn new(region: Region) -> Self {
        Self::with_action(region, Action::Blackout)
    }

    /// Wie [`AnnotatedRegion::new`], aber mit vorgegebener Schwärzungsart.
    pub fn with_action(region: Region, action: Action) -> Self {
        let color = RegionColor::from_source(&region.source);
        let enabled = !region.is_blocking();
        Self {
            id: RegionId::next(),
            region,
            enabled,
            color,
            action,
        }
    }

    /// Blockiert dieser Eintrag andere Treffer (Negativliste)?
    pub fn is_blocking(&self) -> bool {
        self.region.is_blocking()
    }

    /// Hat die Nutzerin an diesem Eintrag etwas geändert?
    ///
    /// Grundlage für die Rückfrage, bevor Regionen weggeworfen werden.
    ///
    /// `default_action` ist die Schwärzungsart, mit der ein frischer Eintrag
    /// angelegt worden **wäre** — also die aus der [`Config`]. Verglichen wird
    /// gegen sie und nicht fest gegen [`Action::Blackout`]: mit
    /// `--action replace` trüge sonst jeder unangetastete Treffer das Merkmal
    /// „von Hand geändert“, und die Rückfrage käme bei jedem Öffnen.
    pub fn is_hand_made(&self, default_action: &Action) -> bool {
        // `enabled == is_blocking()` heißt: der Schalter steht **anders**, als
        // ihn `AnnotatedRegion::with_action` gesetzt hätte.
        matches!(self.region.source, Source::Manual { .. })
            || self.enabled == self.region.is_blocking()
            || self.action != *default_action
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

// ------------------------------------------------------------ Identität
//
// Siehe [`review_identity`]: eine Review-Datei ist eine Liste von Rechtecken
// ohne jeden Bezug zum Inhalt. Landet sie auf einem anderen Dokument, sitzen
// die Schwärzungen an falschen Stellen — und die Geheimnisse bleiben stehen.

/// SHA-256 als Hex-Zeichenkette.
///
/// Nur noch ein Name für [`redact_pipeline::sha256_bytes`] — dieselbe
/// Rechnung, dieselbe Fassung, ein Aufruf. Vorher stand hier eine zweite
/// Implementierung, deren Übereinstimmung mit der der CLI ein Test *behaupten*
/// musste.
pub fn sha256_hex(bytes: &[u8]) -> String {
    sha256_bytes(bytes)
}

/// Ein Dokument, das auf ein Passwort wartet.
///
/// Die Bytes werden gehalten statt der Pfad: abgelegte Dateien (Ziehen und
/// Ablegen im Web-Build) haben gar keinen Pfad, und eine Datei zwischen zwei
/// Versuchen erneut zu lesen hieße, womöglich eine *andere* Datei zu öffnen.
pub struct PendingDocument {
    bytes: Vec<u8>,
    path: Option<PathBuf>,
}

/// `Debug` von Hand: die Rohbytes eines PDFs gehören in keine Meldung.
impl std::fmt::Debug for PendingDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingDocument")
            .field("bytes", &format!("{} Byte", self.bytes.len()))
            .field("path", &self.path)
            .finish()
    }
}

/// Die Muster einer Konfiguration mit ihrem Zustand — oder nichts.
///
/// Eine unbrauchbare `--patterns-config` liefert hier eine leere Liste statt
/// eines Fehlers: die Seitenleiste hat keinen Platz für eine Fehlermeldung, und
/// dieselbe Datei bringt die Analyse ohnehin mit Meldung zum Stehen (siehe
/// [`AppState::analyze`]). Ein leeres Kästchenfeld ist dort das ehrlichere
/// Bild als eine erfundene Vorgabeliste.
fn pattern_states_of(config: &Config) -> Vec<PatternDef> {
    redact_pipeline::pattern_states(config).unwrap_or_default()
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
    /// SHA-256 der geladenen Datei; leer, solange nichts geladen ist.
    ///
    /// Identität des Dokuments — sie steht in der Review-Datei und wird beim
    /// Anwenden verglichen (siehe [`review_identity`]).
    pub input_sha256: String,
    /// MediaBox je Seite — **geprüft**, siehe [`redact_pdf::document::sane_box`].
    ///
    /// Steht in der Datei eine unbrauchbare Angabe (`[0 0 0 0]`, negativ, NaN,
    /// absurd groß), steht hier A4 — dieselbe Größe, die der Rasterizer für
    /// diese Seite zeichnet. Die Rohangabe zu übernehmen hieße, dass die
    /// Oberfläche eine Seite für einen Punkt groß hält, jedes Rechteck darauf
    /// als „liegt neben der Seite“ aussortiert und die Datei ungeschwärzt
    /// durchgeht. Welche Seiten so geheilt wurden, steht in
    /// [`AppState::healed_pages`].
    pub page_boxes: Vec<Rect>,
    /// Seiten (0-basiert), deren MediaBox unbrauchbar war und durch A4 ersetzt
    /// wurde.
    ///
    /// **Die Warnung ist der wichtigere Teil der Heilung**: eine so geheilte
    /// Seite sieht sonst aus wie jede andere.
    pub healed_pages: Vec<usize>,
    /// `/Rotate` je Seite (0/90/180/270), inklusive Vererbung vom Seitenbaum.
    pub rotations: Vec<i64>,
    pub runs: Vec<TextRun>,
    pub current_page: usize,
    pub zoom: f32,
    pub regions: Vec<AnnotatedRegion>,
    pub selected_region: Option<usize>,
    /// Die Einstellungen des Laufs — **dieselbe** Struktur, mit der die
    /// Kommandozeile arbeitet.
    ///
    /// Sie kommt von dort (`redact-rs --gui …`) und wird hier weiterbenutzt:
    /// Muster, Buchungsliste, `--min-confidence`, `--manual-regions`,
    /// `--padding`, die Ladegrenzen. Vorher hatte die Oberfläche eine Handvoll
    /// eigener Felder und kannte den Rest schlicht nicht.
    ///
    /// [`Config::input`] wird beim Öffnen eines Dokuments gesetzt; der
    /// Namenszusatz [`Config::output_suffix`] ist in der Seitenleiste änderbar.
    pub config: Config,
    /// Statuszeile.
    pub status: String,
    /// Warnungen des Extraktors zum geladenen Dokument.
    ///
    /// Getrennt von [`AppState::warnings`], weil sie zum **Dokument** gehören
    /// und in jeden Export mitgehen müssen: „auf dieser Seite konnten wir
    /// nichts lesen“ ist die einzige Stelle, an der übersehener Text sichtbar
    /// wird. Die Kommandozeile stellt sie genauso an den Anfang ihrer
    /// Warnungsliste.
    pub extract_warnings: Vec<String>,
    pub warnings: Vec<String>,
    /// Schnappschüsse für Rückgängig/Wiederholen.
    pub history: History,
    /// Die Muster dieses Laufs samt ihrem tatsächlichen Zustand.
    ///
    /// Grundlage der Kästchen in der Seitenleiste. Steht hier zwischengelegt
    /// und wird **nicht** in jedem Bild neu bestimmt: dabei würden alle
    /// regulären Ausdrücke übersetzt. Aufgefrischt wird bei jeder Änderung an
    /// [`Config::no_patterns`] bzw. [`Config::disabled_patterns`], und die
    /// gehen ausschließlich über [`AppState::set_patterns_enabled`] und
    /// [`AppState::set_pattern_enabled`].
    pattern_states: Vec<PatternDef>,
    /// Ein verschlüsseltes Dokument, das auf sein Passwort wartet.
    ///
    /// Solange das gesetzt ist, zeigt [`crate::app`] die Passwortabfrage.
    pending: Option<PendingDocument>,
    /// Region, deren Ersatztext gerade getippt wird.
    ///
    /// Siehe [`AppState::edit_replacement`]: eine Tippsitzung ist **ein**
    /// Schritt im Verlauf, nicht einer je Anschlag.
    replacing: Option<RegionId>,
    /// Region, die gerade mit den Pfeiltasten geschoben wird.
    ///
    /// Dasselbe Muster wie [`AppState::replacing`] und aus demselben Grund:
    /// eine Schiebe-Sitzung ist **ein** Schritt im Verlauf, nicht einer je
    /// Anschlag. Siehe [`AppState::move_selected`].
    nudging: Option<RegionId>,
}

impl Default for AppState {
    fn default() -> Self {
        let config = Config::default();
        let pattern_states = pattern_states_of(&config);
        Self {
            pattern_states,
            pdf_path: None,
            document: None,
            input_sha256: String::new(),
            page_boxes: Vec::new(),
            healed_pages: Vec::new(),
            rotations: Vec::new(),
            runs: Vec::new(),
            current_page: 0,
            zoom: 1.0,
            regions: Vec::new(),
            selected_region: None,
            config,
            status: "Kein Dokument geladen".to_string(),
            extract_warnings: Vec::new(),
            warnings: Vec::new(),
            history: History::new(),
            pending: None,
            replacing: None,
            nudging: None,
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Zustand mit den Einstellungen eines Kommandozeilenaufrufs.
    pub fn with_config(config: Config) -> Self {
        let mut state = Self {
            config,
            ..Self::default()
        };
        // `--patterns`, `--patterns-config` und `--disable-pattern` bestimmen,
        // welche Kästchen die Seitenleiste zeigt und wie sie stehen.
        state.refresh_pattern_states();
        state
    }

    // ------------------------------------------- Automatische Erkennung

    /// Sucht dieser Lauf überhaupt automatisch?
    pub fn patterns_enabled(&self) -> bool {
        !self.config.no_patterns
    }

    /// Schaltet die automatische Erkennung ganz an oder aus.
    ///
    /// Die einzeln abgeschalteten Muster bleiben dabei gemerkt: wer alles
    /// abschaltet und später wieder einschaltet, findet seine Auswahl vor und
    /// nicht die Vorgabe.
    ///
    /// **Rechnet die Trefferliste nicht selbst neu** — das tut
    /// [`crate::app::RedactApp`], nachdem es gefragt hat, ob dabei Arbeit
    /// verlorengehen darf.
    pub fn set_patterns_enabled(&mut self, on: bool) {
        self.config.no_patterns = !on;
        self.refresh_pattern_states();
    }

    /// Läuft dieses eine Muster in diesem Lauf?
    ///
    /// Antwortet aus [`AppState::pattern_states`], also aus dem *tatsächlichen*
    /// Zustand: Vorgabe, `--patterns`, `--patterns-config` und die
    /// Abschaltliste sind darin schon verrechnet. Ein unbekannter Name ist
    /// `false` — er läuft ja auch nicht.
    pub fn pattern_enabled(&self, id: &str) -> bool {
        self.pattern_states
            .iter()
            .any(|def| def.id == id && def.enabled)
    }

    /// Schaltet ein einzelnes Muster an oder aus.
    ///
    /// Wie [`AppState::set_patterns_enabled`]: die Liste wird geändert, die
    /// Treffer rechnet der Aufrufer neu.
    pub fn set_pattern_enabled(&mut self, id: &str, on: bool) {
        self.config.disabled_patterns.retain(|d| d.trim() != id);
        if !on {
            self.config.disabled_patterns.push(id.to_string());
        }
        self.refresh_pattern_states();
    }

    /// Die Muster dieses Laufs mit ihrem Zustand — für die Kästchen.
    pub fn pattern_states(&self) -> &[PatternDef] {
        &self.pattern_states
    }

    /// Kann „Analysieren“ überhaupt etwas finden?
    ///
    /// Drei Quellen speisen die Analyse (siehe
    /// [`redact_pipeline::collect_regions_for`]): die Muster, die Buchungsliste
    /// und eine Regionsdatei aus `--manual-regions`. Ist die automatische
    /// Erkennung aus und steht keine der beiden anderen dahinter, liefert der
    /// Knopf eine **leere** Liste — und wirft dafür jede Abwahl und jede je
    /// Treffer gewählte Schwärzungsart weg. Er gehört dann abgeschaltet, mit
    /// dem Grund daneben ([`crate::toolbar::ANALYZE_OFF_HINT`]).
    ///
    /// Ausdrücklich **nicht** „automatische Erkennung an?“: mit einer
    /// Buchungsliste findet die Analyse auch ohne jedes Muster etwas, und ein
    /// Knopf, der dann grau wäre, log in die andere Richtung.
    pub fn analysis_can_find_anything(&self) -> bool {
        self.any_pattern_runs()
            || self.config.booking_list.is_some()
            || self.config.manual_regions.is_some()
    }

    /// Läuft in diesem Lauf überhaupt noch ein Muster?
    ///
    /// Beide Wege zu „nein“ zählen: der Schalter „alles aus“ und das
    /// Abwählen jedes einzelnen Kästchens. Nur den ersten zu prüfen ließe die
    /// zweite, ebenso erreichbare Hälfte des Falls offen.
    ///
    /// Eine **leere** Zustandsliste heißt „unbekannt“ und nicht „nichts läuft“:
    /// dorthin führt eine unbrauchbare `--patterns-config` (siehe
    /// [`pattern_states_of`]). Im Zweifel bleibt der Knopf benutzbar, damit die
    /// echte Fehlermeldung erscheint statt einer erfundenen.
    fn any_pattern_runs(&self) -> bool {
        self.patterns_enabled()
            && (self.pattern_states.is_empty() || self.pattern_states.iter().any(|d| d.enabled))
    }

    /// Wie viele Muster in diesem Lauf abgeschaltet sind.
    pub fn disabled_pattern_count(&self) -> usize {
        self.config.disabled_pattern_ids().len()
    }

    /// Die Ansage über abgeschaltete Erkennung — **derselbe Satz**, den die
    /// Kommandozeile ausgibt und der im Audit-Log steht.
    pub fn detection_notice(&self) -> Option<String> {
        redact_pipeline::detection_notice(&self.config)
    }

    fn refresh_pattern_states(&mut self) {
        self.pattern_states = pattern_states_of(&self.config);
    }

    // ---------------------------------------------------------------- Laden

    /// Lädt ein PDF von der Platte, extrahiert die Text-Runs und setzt Seite
    /// und Auswahl zurück.
    pub fn load_document(&mut self, path: &Path) -> Result<()> {
        // Über `read_input` statt `std::fs::read`: sonst gälte die
        // Größengrenze der Eingabedatei nur auf der Kommandozeile. Eine
        // Sparse-Datei mit scheinbar 6 GB kostete hier 6 149 MB, bevor
        // überhaupt feststand, ob es ein PDF ist.
        let bytes = redact_pipeline::read_input(path, self.config.max_input_bytes)?;
        self.load_bytes(&bytes, Some(path.to_path_buf()))
            .map_err(|e| match e {
                RedactError::Pdf(msg) => RedactError::Pdf(format!("{}: {msg}", path.display())),
                other => other,
            })
    }

    /// Wie [`AppState::load_document`], aber aus dem Speicher. Es werden keine
    /// temporären Dateien angelegt.
    ///
    /// Geladen wird mit denselben Grenzen (`--max-decompressed-mb`,
    /// `--max-parsed-mb`) und mit demselben Extraktoraufruf wie in der
    /// Kommandozeile: `extract_with_warnings` statt `extract`. Der Interpreter
    /// bricht an mehreren Stellen still ab; dort steht Text, den die Analyse
    /// nicht sieht. Wer das nicht erfährt, hält eine Datei mit „0 Treffer“ für
    /// sauber — die Oberfläche hat diese Warnungen bisher weggeworfen.
    pub fn load_bytes(&mut self, bytes: &[u8], path: Option<PathBuf>) -> Result<()> {
        // `load_document` statt `load_from_bytes_with_limits`: **dieselbe**
        // Ladefunktion wie in `redact_pipeline::run`, damit ein Passwort aus
        // `Config` hier genauso wirkt wie auf der Kommandozeile.
        let doc = match redact_pipeline::load_document(bytes, &self.config) {
            Ok(doc) => doc,
            Err(e) => {
                // Verschlüsselt und (noch) kein passendes Passwort: das
                // Dokument wartet, statt verloren zu gehen — die Oberfläche
                // fragt danach und ruft dann `unlock`.
                if redact_pipeline::password_required(&e) {
                    self.pending = Some(PendingDocument {
                        bytes: bytes.to_vec(),
                        path,
                    });
                }
                return Err(e);
            }
        };
        self.pending = None;
        let (runs, mut warnings) = PdfExtractor::new().extract_with_warnings(&doc)?;
        // **Geprüfte** Seitengrößen, aus derselben Quelle wie die des
        // Rasterizers. Über `page_boxes` kam bisher die Rohangabe herein; eine
        // Seite mit `/MediaBox [0 0 0 0]` war damit im Fenster einen Punkt
        // groß, `clamp_to_page` ließ von jedem Rechteck darauf nichts übrig,
        // und der Export schwärzte dort nichts — ohne ein Wort, während
        // `redact_pipeline::run` mit derselben `Config` beide Seiten schwärzte.
        let checked = sane_page_boxes(&doc);
        self.healed_pages = checked
            .iter()
            .enumerate()
            .filter(|(_, box_)| box_.is_replaced())
            .map(|(page, _)| page)
            .collect();
        self.page_boxes = checked.iter().map(|box_| box_.rect).collect();
        // Vor die Warnungen des Extraktors: eine unbrauchbare Seitengröße
        // erklärt, warum eine Seite anders aussieht, als sie in der Datei
        // steht — das gehört zuerst gelesen.
        for page in self.healed_pages.iter().rev() {
            warnings.insert(0, healed_page_warning(*page));
        }
        self.rotations = page_rotations(&doc);
        self.runs = runs;
        self.document = Some(Arc::new(doc));
        // Über die Bytes, nicht über das geparste Dokument: die Review-Datei
        // soll die Datei benennen, die die Nutzerin geöffnet hat.
        self.input_sha256 = sha256_hex(bytes);
        // `-o` galt **einem** Dokument. Wird ein anderes geöffnet, ist der Pfad
        // von dort der falsche Vorschlag: die Oberfläche schlug nach dem
        // zweiten PDF weiter den Ausgabenamen des ersten vor, und der
        // Namenszusatz blieb dabei wirkungslos. Für das erste geöffnete
        // Dokument — das aus der Kommandozeile oder das erste aus dem Dialog —
        // gilt er weiter, ebenso beim erneuten Öffnen derselben Datei.
        let switching = self.pdf_path.is_some() && self.pdf_path.as_deref() != path.as_deref();
        if switching {
            self.config.output = None;
        }
        self.config.input = path.clone().unwrap_or_default();
        self.pdf_path = path;
        self.current_page = 0;
        self.selected_region = None;
        self.regions.clear();
        self.end_edit_sessions();
        self.extract_warnings = warnings.clone();
        self.warnings = warnings;
        // Der Verlauf gehörte zum vorigen Dokument.
        self.history.clear();
        self.status = format!(
            "{} Seite(n), {} Textabschnitte geladen",
            self.page_boxes.len(),
            self.runs.len()
        );
        Ok(())
    }

    /// Wartet ein Dokument auf sein Passwort?
    pub fn needs_password(&self) -> bool {
        self.pending.is_some()
    }

    /// Name des wartenden Dokuments — für die Frage im Fenster.
    pub fn pending_name(&self) -> String {
        match self.pending.as_ref().and_then(|p| p.path.as_ref()) {
            Some(path) => path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
            None => "Das Dokument".to_string(),
        }
    }

    /// Zweiter Anlauf mit dem eingegebenen Passwort.
    ///
    /// Passt es nicht, wartet dasselbe Dokument weiter (die Abfrage bleibt
    /// stehen) und das falsche Passwort wird **nicht** behalten — sonst
    /// scheiterte auch der nächste Versuch daran.
    pub fn unlock(&mut self, password: &str) -> Result<()> {
        let Some(pending) = self.pending.take() else {
            return Err(RedactError::Config(
                "Kein Dokument wartet auf ein Passwort".into(),
            ));
        };
        self.config.password = Some(Secret::new(password));
        let result = self.load_bytes(&pending.bytes, pending.path);
        if result.is_err() {
            self.config.password = None;
        }
        result
    }

    /// Die Passwortabfrage abbrechen: das Dokument bleibt ungeöffnet.
    pub fn cancel_password(&mut self) {
        self.pending = None;
        self.config.password = None;
        self.status = "Verschlüsseltes Dokument nicht geöffnet".to_string();
    }

    pub fn is_loaded(&self) -> bool {
        self.document.is_some()
    }

    pub fn page_count(&self) -> usize {
        self.page_boxes.len()
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

    /// Erste Seite (Pos1).
    pub fn first_page(&mut self) {
        self.current_page = 0;
    }

    /// Letzte Seite (Ende).
    pub fn last_page(&mut self) {
        self.current_page = self.page_count().saturating_sub(1);
    }

    /// Steht die Anzeige auf der ersten Seite?
    pub fn is_first_page(&self) -> bool {
        self.current_page == 0
    }

    /// Steht die Anzeige auf der letzten Seite (oder ist nichts geladen)?
    pub fn is_last_page(&self) -> bool {
        self.current_page + 1 >= self.page_count()
    }

    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    }

    /// Eine Stufe größer.
    pub fn zoom_in(&mut self) {
        self.set_zoom(self.zoom * ZOOM_STEP);
    }

    /// Eine Stufe kleiner.
    pub fn zoom_out(&mut self) {
        self.set_zoom(self.zoom / ZOOM_STEP);
    }

    /// Originalgröße (100 %).
    pub fn zoom_reset(&mut self) {
        self.set_zoom(1.0);
    }

    pub fn can_zoom_in(&self) -> bool {
        self.zoom < MAX_ZOOM
    }

    pub fn can_zoom_out(&self) -> bool {
        self.zoom > MIN_ZOOM
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

    /// Wo steht die Region mit dieser Kennung gerade?
    ///
    /// `None`, wenn es sie nicht mehr gibt — genau die Auskunft, die ein über
    /// mehrere Bilder laufender Ziehvorgang braucht (siehe [`RegionId`]).
    pub fn index_of(&self, id: RegionId) -> Option<usize> {
        self.regions.iter().position(|a| a.id == id)
    }

    /// Kennung der Region an diesem Platz.
    pub fn id_at(&self, index: usize) -> Option<RegionId> {
        self.regions.get(index).map(|a| a.id)
    }

    /// Ein Eintrag mit der Schwärzungsart dieses Laufs.
    ///
    /// **Der einzige Weg**, auf dem in der Oberfläche neue Einträge entstehen:
    /// so gilt `--action`/`--replace-with` hier wie auf der Kommandozeile.
    /// Vorher stand in [`AnnotatedRegion::new`] fest [`Action::Blackout`], und
    /// `redact-rs --gui --action replace` schwärzte schwarz.
    fn annotate(&self, region: Region) -> AnnotatedRegion {
        AnnotatedRegion::with_action(region, self.config.action.clone())
    }

    // -------------------------------------------------------------- Analyse

    /// Führt die Analyse über die extrahierten Text-Runs aus — **die** Analyse,
    /// [`redact_pipeline::collect_regions_for`].
    ///
    /// Damit gelten hier dieselben Regeln wie auf der Kommandozeile:
    /// `--manual-regions`, die Buchungsliste, `--patterns-config`,
    /// `--no-patterns`, `--min-confidence` und die Obergrenze
    /// `--max-candidates`. Vorher stand hier ein eigener Aufruf von
    /// `PatternMatcher::new`, der von alldem nur die Muster-IDs kannte.
    ///
    /// Bereits von Hand gezogene Regionen bleiben erhalten — eine erneute
    /// Analyse darf Nutzerarbeit nicht wegwerfen. Alle automatisch gefundenen
    /// Regionen werden dagegen ersetzt.
    ///
    /// Gibt die Gesamtzahl der Regionen zurück.
    pub fn analyze(&mut self) -> Result<usize> {
        // Erst rechnen, dann den Verlauf anfassen: scheitert die Analyse,
        // bleibt der Stapel unberührt.
        // Mit der Prüfsumme des geladenen Dokuments statt ohne: nur so kann die
        // Kette eine Review-Datei hinter `--manual-regions` gegen *dieses*
        // Dokument prüfen. Ohne sie gilt die Herkunft als unbekannt, und eine
        // solche Datei würde ohne `--allow-unverified-review` pauschal
        // abgelehnt — sicher, aber strenger als nötig und ohne erkennbaren
        // Grund für den Nutzer.
        let found =
            redact_pipeline::collect_regions_for(&self.config, &self.runs, &self.input_sha256)?;

        // Was aus `--manual-regions` kommt, steht schon in `found`; ohne den
        // zweiten Test stünde es nach jeder Analyse ein weiteres Mal in der
        // Liste.
        let manual: Vec<AnnotatedRegion> = self
            .regions
            .iter()
            .filter(|a| matches!(a.region.source, Source::Manual { .. }))
            .filter(|a| !found.contains(&a.region))
            .cloned()
            .collect();

        let annotated: Vec<AnnotatedRegion> = found.into_iter().map(|r| self.annotate(r)).collect();
        // Die Trefferliste wird ausgetauscht — eine laufende Tipp- oder
        // Schiebe-Sitzung gehört zum alten Stand.
        self.end_edit_sessions();
        self.history.record(&self.regions);
        self.regions = annotated;
        self.regions.extend(manual);
        self.selected_region = None;

        let summary = self.hit_summary();
        let found = summary.found;
        self.status = format!("Analyse: {}", summary.headline());
        Ok(found)
    }

    // ------------------------------------------------------- Regionen ändern

    /// Das Blatt dieser Seite, sofern ein Dokument geladen ist.
    ///
    /// **Ohne** Rückfall auf A4: wo es keine Seite gibt, gibt es auch nichts zu
    /// beschneiden. Ein Rückfall meldete hier ein Blatt, das im Dokument nicht
    /// vorkommt, und [`AppState::clamp_to_page`] beschnitte darauf.
    pub fn page_box(&self, page: usize) -> Option<Rect> {
        self.page_boxes.get(page).copied()
    }

    /// Beschneidet ein Rechteck auf das Blatt.
    ///
    /// `None`, wenn davon nichts übrig bleibt. Ohne geladenes Dokument bleibt
    /// das Rechteck, wie es ist.
    ///
    /// **Warum das nötig ist**: die Interaktionsfläche im Hauptbereich ist
    /// [`crate::app`]s Blattrand breiter als das Blatt selbst, bei kleinem Zoom
    /// entspricht das etlichen Punkten im User-Space. Ein dort gezogenes
    /// Rechteck lag *neben* der Seite, wurde in der Kopfzeile aber als „wird
    /// geschwärzt“ mitgezählt — gemessen: Rechteck bei x 700…760 auf einer
    /// 595 pt breiten Seite, Kopfzeile „1 werden geschwärzt“, nach dem Export
    /// `removed_glyphs = 0`. Die Wahrheit kam erst hinterher als Warnung.
    pub fn clamp_to_page(&self, page: usize, rect: Rect) -> Option<Rect> {
        let Some(sheet) = self.page_box(page) else {
            return Some(rect);
        };
        let rect = rect.normalized();
        let sheet = sheet.normalized();
        // Erst rechnen, dann bauen: `Rect::new` dreht verkehrte Ecken um, ein
        // leerer Schnitt sähe danach wie ein gültiges Rechteck aus.
        let (x0, x1) = (rect.ll.x.max(sheet.ll.x), rect.ur.x.min(sheet.ur.x));
        let (y0, y1) = (rect.ll.y.max(sheet.ll.y), rect.ur.y.min(sheet.ur.y));
        (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1, y1))
    }

    /// Legt eine manuelle Region an, aktiviert sie und wählt sie aus.
    ///
    /// Das Rechteck wird auf das Blatt beschnitten
    /// ([`AppState::clamp_to_page`]); liegt es ganz daneben, entsteht **keine**
    /// Region — eine, die nichts überdecken kann, hätte in der Trefferliste nur
    /// eine Zahl aufgebläht, die etwas anderes verspricht.
    ///
    /// Gibt den Index der neuen Region zurück, `None`, wenn keine entstand.
    pub fn add_manual_region(
        &mut self,
        page: usize,
        rect: Rect,
        reason: impl Into<String>,
    ) -> Option<usize> {
        let Some(rect) = self.clamp_to_page(page, rect) else {
            self.status = format!(
                "Rechteck liegt außerhalb von Seite {} — nichts angelegt",
                page + 1
            );
            return None;
        };
        let region = Region::new(
            page,
            rect,
            None,
            Source::Manual {
                reason: reason.into(),
            },
        );
        let entry = self.annotate(region);
        self.end_edit_sessions();
        self.history.record(&self.regions);
        self.regions.push(entry);
        let index = self.regions.len() - 1;
        self.selected_region = Some(index);
        self.status = format!("Manuelle Region auf Seite {} angelegt", page + 1);
        Some(index)
    }

    /// Legt ein Rechteck fester Größe in der **Mitte der aktuellen Seite** an,
    /// wählt es aus und sagt in der Statuszeile, wie es weitergeht.
    ///
    /// `None`, solange kein Dokument geladen ist — dann gibt es keine Seite,
    /// auf die es gehörte.
    ///
    /// ## Warum es diesen Weg gibt
    ///
    /// Er ist der einzige, der ohne Zeigegerät zu einem eigenen Rechteck
    /// führt; siehe [`crate::toolbar::ToolAction::AddRegion`].
    ///
    /// ## Warum die Mitte
    ///
    /// Weil jede andere Wahl geraten wäre. Eine freie Stelle zu suchen hieße,
    /// eine Vorstellung davon zu haben, was „frei“ ist — und das Ergebnis
    /// wäre von Seite zu Seite ein anderes, ohne dass man es vorhersagen
    /// könnte. Die Mitte ist immer dieselbe Stelle, und sie liegt immer im
    /// Blatt.
    ///
    /// Der Einwand dagegen ist berechtigt: auf einem schon vollen Blatt ist
    /// ein neues Rechteck in der Mitte schwer zu finden. Dagegen steht
    /// dreierlei, und alles davon war ohnehin schon da: es ist **ausgewählt**
    /// und wird deshalb mit dickem Rand und Eckgriffen gezeichnet
    /// ([`crate::viewer::SELECTED_STROKE`]), im Detailbereich der Seitenleiste
    /// stehen seine Koordinaten, und die Statuszeile nennt Seite und
    /// Weiterweg. Eine eigene Suchlogik dafür wäre mehr Zustand als der Fall
    /// wert ist.
    ///
    /// ## Größe
    ///
    /// [`NEW_REGION_SIZE`] — grob eine Anschriftzeile. Auf sehr kleinen
    /// Blättern höchstens die halbe Seitenkante, damit noch zu sehen ist, dass
    /// es ein Rechteck **auf** der Seite ist und nicht die Seite selbst.
    /// Ändern lässt es sich danach mit Strg+Pfeil
    /// ([`AppState::resize_selected`]).
    pub fn add_region_in_page_middle(&mut self) -> Option<usize> {
        let page = self.current_page;
        let sheet = self.page_box(page)?.normalized();
        let width = NEW_REGION_SIZE.0.min(sheet.width() / 2.0);
        let height = NEW_REGION_SIZE.1.min(sheet.height() / 2.0);
        let (cx, cy) = (
            (sheet.ll.x + sheet.ur.x) / 2.0,
            (sheet.ll.y + sheet.ur.y) / 2.0,
        );
        let index = self.add_manual_region(
            page,
            Rect::new(
                cx - width / 2.0,
                cy - height / 2.0,
                cx + width / 2.0,
                cy + height / 2.0,
            ),
            "mit der Tastatur angelegt",
        )?;
        self.status = new_region_hint(page);
        Some(index)
    }

    /// Ändert die Größe der ausgewählten Region: die **linke untere** Ecke
    /// bleibt stehen, die rechte obere wandert um `dx`/`dy`.
    ///
    /// Das Gegenstück zum Eckgriff, für die Tastatur. Ohne sie wäre der
    /// Tastenweg zu einem eigenen Rechteck eine halbe Sache: eine feste Größe,
    /// die sich nur verschieben lässt, deckt keine Anschrift ab — die ist
    /// mehrzeilig, und wie breit sie ist, weiß nur, wer die Seite sieht.
    ///
    /// Der Weg geht durch [`AppState::set_region_rect`], also durch dieselbe
    /// Funktion wie Maus und Pfeiltasten: Beschneiden auf das Blatt, Wechsel
    /// zu [`Source::Manual`] und die Ansage bei überstimmtem Schutz gelten hier
    /// genauso. **Beschnitten** wird hier richtigerweise (anders als beim
    /// Schieben, siehe [`AppState::slide_onto_page`]): wer die Kante zieht,
    /// will das Rechteck ändern, und am Blattrand ist Schluss.
    ///
    /// Kleiner als [`MIN_REGION_EXTENT`] wird es nicht — ein Rechteck von null
    /// Fläche wäre eine Zeile in der Liste, die nichts überdeckt.
    ///
    /// Verlauf und Seitenprüfung wie bei [`AppState::move_selected`]; beide
    /// teilen sich die Sitzung, weil beides dieselbe Handbewegung an derselben
    /// Region ist.
    pub fn resize_selected(&mut self, dx: f64, dy: f64) -> bool {
        let Some(index) = self.selected_region else {
            return false;
        };
        let Some(entry) = self.regions.get(index) else {
            return false;
        };
        if entry.region.page != self.current_page {
            self.status = selection_on_other_page(entry.region.page);
            return false;
        }
        let (id, page, rect) = (entry.id, entry.region.page, entry.region.rect.normalized());
        let target = Rect::new(
            rect.ll.x,
            rect.ll.y,
            (rect.ur.x + dx).max(rect.ll.x + MIN_REGION_EXTENT),
            (rect.ur.y + dy).max(rect.ll.y + MIN_REGION_EXTENT),
        );
        // Wie beim Schieben: was nichts ändert oder abgelehnt würde, kostet
        // keinen Schritt „Rückgängig“.
        if target == rect || self.clamp_to_page(page, target).is_none() {
            return true;
        }
        if self.nudging != Some(id) {
            self.end_edit_sessions();
            self.history.record(&self.regions);
            self.nudging = Some(id);
        }
        self.set_region_rect(index, target)
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
        self.end_edit_sessions();
        self.history.record(&self.regions);
        self.regions.remove(index);
        self.selected_region = None;
        self.status = "Region gelöscht".to_string();
        true
    }

    /// Schiebt ein Rechteck so weit zurück, dass es wieder ganz auf dem Blatt
    /// liegt — **ohne** seine Größe zu ändern.
    ///
    /// Der Unterschied zu [`AppState::clamp_to_page`] ist der Unterschied
    /// zwischen Schieben und Ziehen. Wer eine Ecke zieht, will das Rechteck
    /// ändern; dort ist Beschneiden richtig. Wer mit den Pfeiltasten schiebt,
    /// will es *versetzen* — würde dabei beschnitten, schrumpfte der schwarze
    /// Balken am Blattrand bei jedem weiteren Anschlag, und ein Stück der
    /// IBAN käme darunter hervor. Also stößt das Rechteck am Rand an und
    /// bleibt ganz.
    ///
    /// Ohne geladenes Dokument gibt es keine Seite und nichts zu schieben.
    /// Ist das Rechteck größer als das Blatt, wird es an der unteren linken
    /// Ecke ausgerichtet; den Überhang schneidet danach
    /// [`AppState::set_region_rect`] weg.
    fn slide_onto_page(&self, page: usize, rect: Rect) -> Rect {
        let Some(sheet) = self.page_box(page) else {
            return rect;
        };
        let (sheet, rect) = (sheet.normalized(), rect.normalized());
        let shift = |low: f64, high: f64, edge_low: f64, edge_high: f64| {
            if low < edge_low {
                edge_low - low
            } else if high > edge_high {
                edge_high - high
            } else {
                0.0
            }
        };
        let dx = shift(rect.ll.x, rect.ur.x, sheet.ll.x, sheet.ur.x);
        let dy = shift(rect.ll.y, rect.ur.y, sheet.ll.y, sheet.ur.y);
        Rect::new(
            rect.ll.x + dx,
            rect.ll.y + dy,
            rect.ur.x + dx,
            rect.ur.y + dy,
        )
    }

    /// Verschiebt die ausgewählte Region um `dx`/`dy` im PDF-User-Space
    /// (Y zeigt nach oben). `false`, wenn nichts ausgewählt war.
    ///
    /// **Der Weg geht über [`AppState::set_region_rect`]** — dieselbe Funktion,
    /// die der Eckgriff benutzt. Das war er lange nicht, und daran hingen drei
    /// Befunde auf einmal:
    ///
    /// * *Neben das Blatt geschoben.* Hundert Anschläge auf Umschalt+Pfeil
    ///   links legten das Rechteck vollständig neben die Seite. Zu sehen war
    ///   davon nichts (`ui.painter_at` schneidet weg), die Kopfzeile zählte es
    ///   weiter als „wird geschwärzt“, und der Export meldete den Fehlschlag
    ///   erst hinterher als Warnung. Jetzt stößt es am Blattrand an
    ///   ([`AppState::slide_onto_page`]).
    /// * *Keine Handarbeit.* Ein so korrigierter Treffer blieb
    ///   [`Source::Pattern`]; [`AnnotatedRegion::is_hand_made`] sah die
    ///   Korrektur nicht, und „Analysieren“ warf sie **ohne Rückfrage** weg —
    ///   während die vier anderen Wege zum selben Verlust fragten. Der
    ///   Quellwechsel steckt in `set_region_rect`.
    /// * *Ansage bei überstimmtem Schutz.* Auch [`PROTECTION_OVERRIDDEN`]
    ///   stand nur im Ziehweg.
    ///
    /// Der Verlauf bekommt **einen** Schnappschuss je Schiebe-Sitzung, nicht
    /// einen je Anschlag: fünfzig Antipper — mit Tastenwiederholung etwa eine
    /// Sekunde — schoben sonst bei [`crate::HISTORY_LIMIT`] = 50 jeden älteren
    /// Stand hinaus, auch den vor einem versehentlichen Löschen. Dasselbe
    /// Muster wie beim Tippen im Ersatzfeld ([`AppState::edit_replacement`])
    /// und beim Zug am Eckgriff ([`AppState::begin_manual_edit`]); beendet
    /// wird die Sitzung von [`AppState::end_edit_sessions`].
    ///
    /// ## Nur auf der Seite, die gezeigt wird
    ///
    /// Dieselbe Absicherung, die der Zug am **Eckgriff** seit v0.4.0 hat
    /// (`crate::app::RedactApp::apply_pointer`, Fall 1: „Zug beendet — die
    /// angefasste Region liegt auf einer anderen Seite“). Sie fehlte hier, und
    /// der Weg dorthin ist einer, den man von selbst geht: Trefferzeile
    /// anklicken (die Auswahl bleibt), Bild ab, dann Pfeil links für „eine
    /// Seite zurück“. Geblättert wird dabei nicht — [`crate::app::key_commands`]
    /// macht aus jedem Pfeil eine Verschiebung, sobald *irgendetwas*
    /// ausgewählt ist —, sondern der Balken auf der **nicht gezeigten** Seite
    /// wandert. Zehn Anschläge sind 100 pt; gemessen stand die IBAN danach
    /// halb überdeckt und wieder lesbar in der exportierten Datei, während die
    /// Kopfzeile unverändert „2 werden geschwärzt“ versprach.
    ///
    /// Die Alternative — die Auswahl beim Blättern aufheben — nimmt die
    /// gewollte Arbeitsweise „Zeile anklicken, Seite springt mit“
    /// (`crate::sidebar`) kaputt. Also stattdessen: nichts tun und **sagen**,
    /// warum ([`selection_on_other_page`]).
    pub fn move_selected(&mut self, dx: f64, dy: f64) -> bool {
        let Some(index) = self.selected_region else {
            return false;
        };
        let Some(entry) = self.regions.get(index) else {
            return false;
        };
        if entry.region.page != self.current_page {
            self.status = selection_on_other_page(entry.region.page);
            return false;
        }
        let (id, page, rect) = (entry.id, entry.region.page, entry.region.rect);
        let target = self.slide_onto_page(
            page,
            Rect::new(
                rect.ll.x + dx,
                rect.ll.y + dy,
                rect.ur.x + dx,
                rect.ur.y + dy,
            ),
        );
        // Am Blattrand angekommen: nichts ändert sich, also gehört auch nichts
        // in den Verlauf. Sonst kostete jeder weitere Anschlag gegen den Rand
        // einen Schritt „Rückgängig“, der sichtbar nichts zurücknimmt.
        if target == rect {
            return true;
        }
        // `set_region_rect` beschneidet noch einmal und lehnt ab, was dabei
        // leer wird. Erst fragen, dann den Verlauf anfassen — ein abgelehnter
        // Anschlag darf keinen Schritt „Rückgängig“ kosten.
        if self.clamp_to_page(page, target).is_none() {
            return true;
        }
        // Der eine Schnappschuss dieser Sitzung.
        if self.nudging != Some(id) {
            self.end_edit_sessions();
            self.history.record(&self.regions);
            self.nudging = Some(id);
        }
        self.set_region_rect(index, target)
    }

    /// Beginnt eine Änderung, die über mehrere Bilder läuft.
    ///
    /// Legt **einmal** den Stand davor im Verlauf ab. Ein Ziehvorgang am
    /// Eckgriff ändert das Rechteck in jedem Bild; käme jedes davon in den
    /// Stapel, stünden nach einer Sekunde Ziehen sechzig Schritte darin und
    /// „Rückgängig“ führte nicht mehr zum Stand vor der Korrektur, sondern
    /// einen Mauszuck weit zurück.
    pub fn begin_manual_edit(&mut self) {
        self.end_edit_sessions();
        self.history.record(&self.regions);
    }

    /// Setzt das Rechteck einer Region — **ohne** Verlaufseintrag.
    ///
    /// Für den laufenden Ziehvorgang gedacht; den einen Schnappschuss legt
    /// [`AppState::begin_manual_edit`] zu dessen Beginn an.
    ///
    /// **Ein von Hand geändertes Rechteck ist nicht mehr das, was das Muster
    /// gefunden hat.** Der Treffer wird deshalb zu einer manuellen Region:
    /// Farbe, Beschreibung und Audit-Log sagen dann „selbst gezeichnet“, die
    /// Rückfrage vor Datenverlust zählt ihn mit, und eine erneute Analyse wirft
    /// die Korrektur nicht weg (sie ersetzt nur die automatisch gefundenen
    /// Regionen). Der ursprüngliche Fund taucht nach einer erneuten Analyse
    /// wieder auf; liegt er ganz im korrigierten Rechteck, weist ihn
    /// `resolve_conflicts` als „doppelt“ aus.
    ///
    /// **Ausnahme sind schützende Treffer der Negativliste**: aus ihnen darf
    /// durch ein Ziehen niemals eine Schwärzung werden — das kehrte ihre
    /// Bedeutung um. Sie behalten Herkunft und Wirkung, nur ihr Rechteck ändert
    /// sich. Der Preis dieser Ausnahme: [`AnnotatedRegion::is_hand_made`] sieht
    /// ihnen die Änderung nicht an, ein allein daran geändertes Dokument gilt
    /// also als unbearbeitet. Ein eigenes Merkmal dafür wäre mehr Zustand, als
    /// dieser Fall wert ist; rückgängig machen lässt sich die Änderung
    /// trotzdem.
    ///
    /// **Ein *blockierter* Treffer verliert dabei seinen Schutz** — und das
    /// bleibt so, mit Begründung. Die Ausnahme oben gilt den *schützenden*
    /// Einträgen, nicht den geschützten. Wird ein Treffer, den die Schutzliste
    /// deckt, von Hand angefasst, wird er zur manuellen Region, und manuelle
    /// Regionen überstimmen die Schutzliste (das entscheidet
    /// `resolve_conflicts`, siehe [`AppState::enabled_redactions`]). Aus
    /// `[Protecting, Blocked]` wird also `[Protecting, Redacted]` — schon bei
    /// einem Punkt Verbreiterung.
    ///
    /// Das ist kein Datenverlust (es wird *mehr* geschwärzt, nicht weniger),
    /// und es ist die einzige Lesart, die zum Rest passt: ein von Hand
    /// gezogenes Rechteck an derselben Stelle überstimmt die Schutzliste
    /// genauso. Den Schutz zu erhalten hieße, dass dasselbe Rechteck je nach
    /// Entstehungsweg verschieden wirkt. Was fehlte, war die **Ansage**: Farbe,
    /// Beschriftung und Zahl änderten sich zwar mit, die Umkehrung der Wirkung
    /// stand aber nirgends. Deshalb setzt dieser Fall [`PROTECTION_OVERRIDDEN`]
    /// in die Statuszeile; Strg+Z nimmt die Anpassung zurück.
    pub fn set_region_rect(&mut self, index: usize, rect: Rect) -> bool {
        let Some(page) = self.regions.get(index).map(|a| a.region.page) else {
            return false;
        };
        // Dieselbe Grenze wie beim Anlegen: über den Blattrand hinaus lässt
        // sich eine Ecke zwar ziehen, das Rechteck endet aber am Blatt. Sonst
        // stünde eine Region in der Liste, deren Fläche gar nicht auf der Seite
        // liegt — gezählt als „wird geschwärzt“, ohne ein Zeichen zu treffen.
        let Some(rect) = self.clamp_to_page(page, rect) else {
            return false;
        };
        let entry = &self.regions[index];
        let stays =
            entry.region.is_blocking() || matches!(entry.region.source, Source::Manual { .. });
        // **Vor** der Änderung fragen: hinterher ist der Eintrag manuell und
        // damit ohnehin nicht mehr blockiert. Die Konfliktauflösung läuft dabei
        // höchstens einmal je Ziehvorgang — ab dem zweiten Bild ist `stays`
        // wahr.
        let was_protected = !stays && self.hit_summary().outcome(index) == HitOutcome::Blocked;

        let entry = &mut self.regions[index];
        entry.region.rect = rect;
        if !stays {
            entry.region.source = Source::Manual {
                reason: "Treffer von Hand angepasst".to_string(),
            };
            entry.color = RegionColor::from_source(&entry.region.source);
        }
        if was_protected {
            self.status = PROTECTION_OVERRIDDEN.to_string();
        }
        true
    }

    /// Ändert den Aktivzustand einer Region.
    ///
    /// Eine blockierende (Negativlisten-)Region lässt sich **nicht**
    /// einschalten — das würde die Semantik der Negativliste aushebeln. In dem
    /// Fall wird `false` zurückgegeben und nichts geändert.
    pub fn set_enabled(&mut self, index: usize, enabled: bool) -> bool {
        let Some(entry) = self.regions.get(index) else {
            return false;
        };
        if enabled && entry.is_blocking() {
            return false;
        }
        // Nur echte Änderungen kommen in den Verlauf — sonst kostete ein
        // Rückgängig mehrere Klicks, bevor sichtbar etwas passiert.
        if entry.enabled != enabled {
            self.end_edit_sessions();
            self.history.record(&self.regions);
            self.regions[index].enabled = enabled;
        }
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
        let Some(entry) = self.regions.get(index) else {
            return false;
        };
        if entry.action != action {
            // Eine neue Art beendet die laufenden Sitzungen: der nächste
            // Anschlag gehört dann zu einem neuen Schritt.
            self.end_edit_sessions();
            self.history.record(&self.regions);
            self.regions[index].action = action;
        }
        true
    }

    /// Vorschlag für den Ersatztext.
    ///
    /// Der aus `--replace-with` (er steckt in [`Config::action`], sobald
    /// `--action replace` gilt), sonst [`DEFAULT_REPLACEMENT`]. Die Oberfläche
    /// hatte hier fest `"[REDACTED]"` stehen: englisch in einer deutschen
    /// Oberfläche, anders als die Vorgabe der Kommandozeile, und
    /// `--replace-with` blieb wirkungslos.
    pub fn default_replacement(&self) -> String {
        // Über `Config::replacement()`, nicht über `action` allein: sonst ginge
        // `--replace-with X` bei `--action blackout` verloren, und wer hier auf
        // „Ersetzen" umstellt, bekäme die Vorgabe statt seines Textes.
        self.config.replacement().to_string()
    }

    /// Ändert den Ersatztext einer Region — **ein** Verlaufseintrag je
    /// Tippsitzung.
    ///
    /// Jede Textänderung legte bisher einen Schnappschuss ab: gemessen 19
    /// Tastendrücke → 20 Schritte. Ein Ersatztext von rund 50 Zeichen schob
    /// damit bei einer Grenze von [`crate::HISTORY_LIMIT`] **jeden** älteren
    /// Stand hinaus — auch den vor einem versehentlichen Löschen. Dasselbe
    /// Problem hatte der Zug am Eckgriff, und es wird hier genauso gelöst wie
    /// dort ([`AppState::begin_manual_edit`]): der Schnappschuss entsteht
    /// einmal, beim ersten Anschlag.
    ///
    /// Die Sitzung endet mit [`AppState::end_replacement_edit`] — die
    /// Seitenleiste ruft das, sobald das Feld den Fokus nicht mehr hat.
    pub fn edit_replacement(&mut self, index: usize, text: impl Into<String>) -> bool {
        let text = text.into();
        let Some(entry) = self.regions.get(index) else {
            return false;
        };
        let Action::Replace(current) = &entry.action else {
            return false;
        };
        if *current == text {
            return true;
        }
        let id = entry.id;
        if self.replacing != Some(id) {
            self.end_edit_sessions();
            self.history.record(&self.regions);
            self.replacing = Some(id);
        }
        self.regions[index].action = Action::Replace(text);
        true
    }

    /// Beendet eine Tippsitzung im Ersatzfeld.
    pub fn end_replacement_edit(&mut self) {
        self.end_edit_sessions();
    }

    /// Läuft gerade eine Tippsitzung (nur für Tests und Erklärungen)?
    pub fn is_editing_replacement(&self) -> bool {
        self.replacing.is_some()
    }

    /// Läuft gerade eine Schiebe-Sitzung (nur für Tests und Erklärungen)?
    pub fn is_nudging(&self) -> bool {
        self.nudging.is_some()
    }

    /// Beendet beide Sitzungen, die mehrere Bedienschritte zu **einem**
    /// Verlaufsschritt zusammenfassen: das Tippen im Ersatzfeld
    /// ([`AppState::edit_replacement`]) und das Schieben mit den Pfeiltasten
    /// ([`AppState::move_selected`]).
    ///
    /// Ruft, wer selbst einen Schnappschuss ablegt. Ohne das flösse eine
    /// dazwischenliegende Änderung — löschen, abwählen, Art umstellen — in den
    /// laufenden Schritt hinein, und ein Rückgängig führte an ihr vorbei.
    fn end_edit_sessions(&mut self) {
        self.replacing = None;
        self.nudging = None;
    }

    // ------------------------------------------- Rückgängig / Wiederholen

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Stellt den Stand vor der letzten Änderung wieder her.
    ///
    /// Die Auswahl wird dabei aufgehoben: nach einem Schritt zurück kann der
    /// Eintrag, auf den der Index zeigte, verschwunden oder ein anderer sein.
    pub fn undo(&mut self) -> bool {
        self.end_edit_sessions();
        match self.history.undo(&self.regions) {
            Some(previous) => {
                self.regions = previous;
                self.selected_region = None;
                self.status = "Rückgängig".to_string();
                true
            }
            None => {
                self.status = "Nichts mehr rückgängig zu machen".to_string();
                false
            }
        }
    }

    /// Nimmt ein Rückgängig zurück.
    pub fn redo(&mut self) -> bool {
        self.end_edit_sessions();
        match self.history.redo(&self.regions) {
            Some(next) => {
                self.regions = next;
                self.selected_region = None;
                self.status = "Wiederhergestellt".to_string();
                true
            }
            None => {
                self.status = "Nichts mehr wiederherzustellen".to_string();
                false
            }
        }
    }

    // --------------------------------------------------- Konfliktauflösung

    /// Zählt dieser Eintrag bei der Konfliktauflösung mit?
    ///
    /// Alle aktivierten Treffer **plus** sämtliche Negativlisten-Treffer.
    /// Letztere sind nie `enabled` (sie werden ja nicht geschwärzt), müssen
    /// aber trotzdem als Blocker mitgegeben werden.
    ///
    /// **Diese eine Vorauswahl gilt überall.** Sie stand früher nur in
    /// [`AppState::conflict_input`]; [`AppState::enabled_redactions`] suchte
    /// die Schwärzungsart dagegen über *alle* Zeilen. Bei zwei deckungsgleichen
    /// Einträgen — so entstehen sie beim Laden einer Review-Datei mit doppeltem
    /// Eintrag — gewann damit die Art des **abgewählten**: die Oberfläche zeigte
    /// „Ersetzen [IBAN]“, exportiert wurde `Blackout`.
    ///
    /// Seit dieser Runde gibt es die zweite Stelle gar nicht mehr:
    /// [`AppState::enabled_redactions`] liest die Entscheidung aus
    /// [`AppState::hit_summary`], und die trifft sie je Zeile mit genau diesen
    /// beiden Prüfungen (`is_blocking` ⇒ `Protecting`, `!enabled` ⇒
    /// `Disabled`). Die Vorauswahl steht damit noch an **einer** Stelle —
    /// hier, für [`AppState::conflict_input`].
    fn counts_for_resolution(entry: &AnnotatedRegion) -> bool {
        entry.is_blocking() || entry.enabled
    }

    /// Regionen, die in die Konfliktauflösung gehen.
    ///
    /// Ohne die, die vollständig neben ihrem Blatt liegen: sie können kein
    /// Zeichen entfernen, und mitgezählt behaupteten sie das Gegenteil. Siehe
    /// [`HitOutcome::OffPage`].
    fn conflict_input(&self) -> Vec<Region> {
        self.regions
            .iter()
            .filter(|a| Self::counts_for_resolution(a))
            .filter(|a| !self.is_off_page(&a.region))
            .map(|a| a.region.clone())
            .collect()
    }

    /// Liegt diese Region vollständig neben ihrem Blatt?
    ///
    /// Dieselbe Rechnung wie [`AppState::clamp_to_page`] — was dort nichts
    /// übrig lässt, kann auch nichts schwärzen.
    ///
    /// **Eine Seite, die es im Dokument nicht gibt, zählt genauso.** Das war
    /// nicht so: die Antwort lautete dort `false`, weil die Frage „liegt es
    /// neben dem Blatt?“ ohne Blatt nicht zu beantworten sei, und der Export
    /// meldete den Fall hinterher als `missing_page_redactions`. Die
    /// Begründung trägt nur, solange **kein Dokument geladen** ist; ist eines
    /// da, ist die Frage sehr wohl zu beantworten, und die Antwort lautet: das
    /// Rechteck kann kein Zeichen treffen. Eine Review-Datei mit einem Eintrag
    /// auf Seite 8 eines zweiseitigen Dokuments ließ die Kopfzeile sonst „2
    /// werden geschwärzt“ versprechen, ohne Vorbehalt, und die Wahrheit kam
    /// als Warnung **nach** dem Export — wortwörtlich der Fehler, den
    /// [`HitOutcome::OffPage`] für den Nachbarfall abgestellt hat.
    pub fn is_off_page(&self, region: &Region) -> bool {
        match self.page_box(region.page) {
            Some(_) => self.clamp_to_page(region.page, region.rect).is_none(),
            // Ohne geladenes Dokument gibt es überhaupt keine Seiten; dann ist
            // die Frage wirklich offen und die Region bleibt stehen.
            None => self.has_document(),
        }
    }

    /// Ist ein Dokument geladen?
    pub fn has_document(&self) -> bool {
        self.document.is_some()
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
    /// ## Warum zwei Zeiger und keine Suche
    ///
    /// Die Zuordnung geschieht **der Reihe nach**, und sie darf das, weil
    /// [`resolve_conflicts`] beide Listen in Eingabereihenfolge zurückgibt:
    /// `redact` und `blocked` sind Teilfolgen der Kandidaten, in derselben
    /// Ordnung. Genau diese Zusage hält
    /// [`crate::state::rev7_tests::resolve_conflicts_haelt_die_eingabereihenfolge`]
    /// — ohne sie wäre jeder Umbau von `dedup` ein stiller Datenfehler in der
    /// Anzeige, und deshalb steht sie als Test da und nicht als Bemerkung.
    ///
    /// Vorher wurde für **jede** Zeile die passende Stelle im Ergebnis
    /// *gesucht*: n Zeilen gegen n Ergebnisplätze, also n²/2 Vergleiche ganzer
    /// [`Region`]-Werte. Bei 96 000 Regionen kostete allein diese Bilanz 1,6 s
    /// — und sie läuft einmal je Bild, bei jedem Zug am Eckgriff und nach
    /// jeder Analyse. Mit zwei Zeigern ist es **ein** Vergleich je Zeile
    /// (gemessen 81 ms), bei sonst gleichem Ergebnis: verglichen wird dasselbe
    /// wie zuvor, nur an genau einer Stelle statt in einer Schleife.
    ///
    /// Läuft ein Zeiger doch einmal daneben — weil die Reihenfolgezusage
    /// gefallen ist —, dann fällt die Zeile auf [`HitOutcome::Duplicate`]
    /// zurück, nicht auf „wird geschwärzt“. Der Test oben ist trotzdem der
    /// Wächter: die vorsichtige Richtung ist kein Ersatz für die Zusage.
    pub fn hit_summary(&self) -> HitSummary {
        let resolution = self.resolution();
        // Zeigt auf den nächsten noch nicht vergebenen Platz im Ergebnis.
        let mut next_redact = 0usize;
        let mut next_blocked = 0usize;

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
                // Vor der Suche im Ergebnis: neben dem Blatt liegende
                // Rechtecke sind gar nicht erst hineingegangen und fielen
                // sonst als „doppelt“ heraus — ein falscher Grund für das
                // richtige Ergebnis.
                //
                // **Hinter** den beiden Prüfungen davor: ein Schutzeintrag
                // bleibt ein Schutzeintrag (sonst zählte ihn `found` plötzlich
                // als Fund), und ein abgewählter bleibt abgewählt — das ist
                // die Entscheidung der Nutzerin und der nähere Grund.
                if self.is_off_page(&entry.region) {
                    // Zwei Gründe, dasselbe Ergebnis — aber verschiedene
                    // Abhilfen: fehlt die Seite, ist die Seitenzahl falsch;
                    // sonst die Koordinaten.
                    return match self.page_box(entry.region.page) {
                        Some(_) => HitOutcome::OffPage,
                        None => HitOutcome::MissingPage,
                    };
                }
                // Genau **ein** Vergleich, mit demselben Gleichheitsbegriff
                // wie zuvor: der Platz, der dieser Zeile zusteht, ist der
                // vorderste noch freie.
                if resolution
                    .redact
                    .get(next_redact)
                    .is_some_and(|r| *r == entry.region)
                {
                    next_redact += 1;
                    return HitOutcome::Redacted;
                }
                if resolution
                    .blocked
                    .get(next_blocked)
                    .is_some_and(|b| b.page == entry.region.page && b.rect == entry.region.rect)
                {
                    next_blocked += 1;
                    return HitOutcome::Blocked;
                }
                HitOutcome::Duplicate
            })
            .collect();

        let count = |wanted: HitOutcome| outcomes.iter().filter(|o| **o == wanted).count();
        let protecting = count(HitOutcome::Protecting);
        HitSummary {
            found: outcomes.len() - protecting,
            protecting,
            // **Eine** Quelle, nicht zwei. `found`, `protecting` und
            // `off_page` kamen schon immer aus `outcomes`, `redacted` allein
            // aus `resolution.redact.len()` — und beide werden nebeneinander
            // gelesen: die Kopfzeile nahm `redacted`, Miniaturspalte und
            // Betrachter nehmen `outcomes`. Zwei Quellen für dieselbe Zahl
            // können nur auseinanderlaufen; abweichen darf hier nichts.
            redacted: count(HitOutcome::Redacted),
            off_page: count(HitOutcome::OffPage),
            missing_page: count(HitOutcome::MissingPage),
            outcomes,
            // **Nicht** `patterns_enabled()`: das ist nur der Hauptschalter.
            // Wer das letzte laufende Muster einzeln abwählt, sucht ebenso
            // wenig — und die Kopfzeile verlor dabei ihre Vorwarnung und sagte
            // „0 Treffer · 0 werden geschwärzt“, denselben Satz wie bei einem
            // Dokument, in dem wirklich nichts steht. `any_pattern_runs` zählt
            // ausdrücklich beide Wege.
            automatic: self.any_pattern_runs(),
            detection_switch: self.patterns_enabled(),
            disabled_patterns: self.disabled_pattern_count(),
        }
    }

    /// Wie viele Schwärzungen je Seite wirklich passieren.
    ///
    /// Der Index ist die Seitenzahl, die Länge [`AppState::page_count`].
    ///
    /// **Einmal je Bild, nicht einmal je Miniaturansicht.** Die Spalte links
    /// rief früher für jedes Kleinbild `regions_on_page`, und das lief über
    /// *alle* Regionen und legte dafür einen `Vec<usize>` an — nur um dessen
    /// Länge zu nehmen. Bei 1 600 Seiten und 96 000 Regionen (beides noch
    /// unter `--max-candidates`) kostete allein diese Zahl 0,50 s je Bild:
    /// zwei Bilder je Sekunde, ohne dass irgendetwas gezeichnet worden wäre.
    /// Hier ist es ein Durchlauf über die Regionen für die ganze Spalte.
    pub fn redactions_per_page(&self, summary: &HitSummary) -> Vec<usize> {
        let mut counts = vec![0usize; self.page_count()];
        for (index, entry) in self.regions.iter().enumerate() {
            if summary.outcome(index).is_redacted() {
                if let Some(slot) = counts.get_mut(entry.region.page) {
                    *slot += 1;
                }
            }
        }
        counts
    }

    /// Steckt in den Regionen Handarbeit, die beim Verwerfen verloren ginge?
    ///
    /// Wahr, sobald ein Rechteck selbst gezogen, ein Treffer abgewählt oder
    /// dessen Schwärzungsart geändert wurde. Eine frisch gelaufene Analyse
    /// allein zählt **nicht** — die ist mit einem Klick wiederhergestellt.
    pub fn has_manual_work(&self) -> bool {
        self.regions
            .iter()
            .any(|a| a.is_hand_made(&self.config.action))
    }

    /// Einträge, an denen von Hand gearbeitet wurde (für die Rückfrage).
    pub fn hand_made_count(&self) -> usize {
        self.regions
            .iter()
            .filter(|a| a.is_hand_made(&self.config.action))
            .count()
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
    ///
    /// **Die Schwärzungsart kommt von der Zeile selbst.** Sie wurde früher
    /// gesucht: für jede Region der Auflösung ein Durchlauf durch alle Zeilen,
    /// bis eine gleich war — n²/2 Vergleiche ganzer [`Region`]-Werte samt
    /// ihrem `Option<String>`, gemessen 25,0 s bei 96 000 Regionen. Die Suche
    /// beantwortete dabei genau die Frage, die [`AppState::hit_summary`]
    /// ohnehin schon für **jede** Zeile beantwortet hat: wird sie geschwärzt?
    ///
    /// Also wird sie hier gelesen statt zum zweiten Mal gestellt. Damit fallen
    /// weg: die Suche, der Filter über [`AppState::counts_for_resolution`] an
    /// dieser Stelle (`hit_summary` prüft dasselbe schon, nur je Zeile
    /// einmal), der Rückfall auf [`Config::action`] für eine Region ohne Zeile
    /// — den es nicht geben kann, weil jede Region der Auflösung aus einer
    /// Zeile stammt — und **ein ganzer Lauf von `resolve_conflicts`**.
    ///
    /// Die Reihenfolge bleibt dieselbe: `outcomes` steht in Zeilenreihenfolge,
    /// und `resolution.redact` ist deren Teilfolge (siehe `hit_summary`).
    ///
    /// Was die frühere Fassung damit behielt: bei zwei deckungsgleichen Zeilen
    /// entschied vor Befund B5 die **erste** — auch wenn sie abgewählt war und
    /// die andere eine andere Art trug. Jetzt entscheidet die Zeile, der die
    /// Bilanz die Schwärzung zugeschrieben hat, und das ist dieselbe.
    pub fn enabled_redactions(&self) -> Vec<Redaction> {
        let summary = self.hit_summary();
        self.regions
            .iter()
            .enumerate()
            .filter(|(index, _)| summary.outcome(*index).is_redacted())
            .map(|(_, entry)| Redaction::new(entry.region.clone(), entry.action.clone()))
            .collect()
    }

    // ------------------------------------------------------- Dateinamen

    /// Vorschlag für die Ausgabedatei: der Pfad aus `-o`, sonst **neben dem
    /// Original**, mit dem Namenszusatz aus [`Config::output_suffix`] am
    /// Dateinamen-Stamm.
    ///
    /// `None`, solange kein Dokument geladen ist. Der abgeleitete Vorschlag ist
    /// nie mit dem Eingabepfad identisch — dafür sorgt
    /// [`output_path_with_suffix`], das bei leerem Zusatz auf den Standard
    /// zurückfällt. Ein von Hand genanntes `-o` darf dagegen alles sein; dass
    /// dabei nicht das Original überschrieben wird, entscheidet
    /// [`AppState::export`] und nicht dieser Vorschlag.
    pub fn suggested_output_path(&self) -> Option<PathBuf> {
        // Wurde ein Ausgabepfad genannt (`-o`), ist er der Vorschlag — auch in
        // der Oberfläche. Sie fragt trotzdem nach: geschrieben wird erst nach
        // einem Klick, und der Dialog steht dann auf dem gewünschten Namen.
        if let Some(out) = &self.config.output {
            return Some(out.clone());
        }
        self.pdf_path
            .as_ref()
            .map(|input| output_path_with_suffix(input, &self.config.output_suffix))
    }

    /// Steht der Ausgabename fest, weil `-o` ihn genannt hat?
    ///
    /// Dann hat der Namenszusatz keine Wirkung — und die Seitenleiste schaltet
    /// das Feld ab und sagt warum, statt ein Eingabefeld anzubieten, an dem
    /// sichtbar nichts hängt.
    pub fn output_name_is_fixed(&self) -> bool {
        self.config.output.is_some()
    }

    /// Vorschlag für die Review-Datei: der Pfad aus `--review-out`, sonst neben
    /// dem Original, `…_review.json`.
    ///
    /// `--review-out` kam in der Oberfläche vorher überhaupt nicht vor — genau
    /// der Fehlertyp, der bei `--action`, `--apply-review` und `--audit-log`
    /// schon behoben wurde: ein Schalter, den die Kommandozeile annimmt und der
    /// hinter `--gui` still verschwindet. Die Regel ist dieselbe wie in
    /// `redact_pipeline::run` (dort `review_target`).
    pub fn suggested_review_path(&self) -> Option<PathBuf> {
        if let Some(out) = &self.config.review_out {
            return Some(out.clone());
        }
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

    /// Wohin das Audit-Log dieses Exports gehört.
    ///
    /// `--audit-log` hat Vorrang: wer den Pfad auf der Kommandozeile nennt,
    /// bekommt ihn auch, wenn er die Oberfläche über `--gui` dazuschaltet.
    /// Vorher wurde der Name hier **immer** aus dem der Ausgabedatei abgeleitet,
    /// und `--audit-log` blieb in der Oberfläche wirkungslos (Befund #67).
    /// Ohne den Schalter bleibt es bei [`AppState::audit_path_for`] — das Log
    /// gehört dann neben die Datei, die es beschreibt.
    pub fn audit_target(&self, out: &Path) -> PathBuf {
        self.config
            .audit_log
            .clone()
            .unwrap_or_else(|| Self::audit_path_for(out))
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

    /// Einstellungen für einen Export nach `out`.
    ///
    /// `force` steht auf `true`: der Speichern-Dialog des Systems hat das
    /// Überschreiben bereits abgefragt, eine zweite Rückfrage im Schreibpfad
    /// wäre eine Sackgasse. Das ist der einzige bewusste Unterschied im
    /// Schreibverhalten gegenüber der Kommandozeile, wo `--force` die Antwort
    /// ist. Alles andere — die Eingabedatei bleibt geschützt, kein Symlink,
    /// kein halb geschriebenes Ziel, Modus 0600 für Log und Review — gilt
    /// hier wie dort.
    pub fn export_config(&self, out: &Path, audit: Option<&Path>) -> Config {
        Config {
            output: Some(out.to_path_buf()),
            audit_log: audit.map(Path::to_path_buf),
            force: true,
            // `redact-rs --gui --review` hat die Oberfläche geöffnet, um die
            // Treffer anzusehen. Wer hier auf „Exportieren“ drückt, will eine
            // geschwärzte Datei — der Review-Modus der Kommandozeile ist
            // damit erledigt.
            review: false,
            ..self.config.clone()
        }
    }

    /// Schwärzt eine Kopie des Dokuments, entfernt die Metadaten und schreibt
    /// das Ergebnis nach `out`.
    ///
    /// Die Arbeit macht [`redact_pipeline::apply`] — **derselbe** Aufruf, den
    /// auch [`redact_pipeline::run`] für die Kommandozeile ausführt. Bei
    /// gleicher Regionenmenge und gleicher [`Config`] entsteht dieselbe Datei,
    /// Byte für Byte, und dasselbe Audit-Log. Vorher standen hier drei von Hand
    /// nachgebaute Zeilen mit fest verdrahteter Polsterung 1,0, stillem
    /// Überschreiben und einem selbst zusammengesetzten Log.
    ///
    /// Ist `audit` gesetzt, wird zusätzlich ein Audit-Log geschrieben — mit
    /// beiden Prüfsummen, gemessener Wirkung und Modus 0600.
    ///
    /// Zwei Fälle werden **abgelehnt, bevor irgendetwas geschrieben wird**:
    ///
    /// * `out` zeigt auf die Originaldatei — der Dateidialog lässt das zu, und
    ///   ein Klick auf „Überschreiben“ hätte das ungeschwärzte Original
    ///   vernichtet;
    /// * es ist nichts ausgewählt. Vorher entstand eine unveränderte Kopie
    ///   namens `…_geschwaerzt.pdf` samt Erfolgsmeldung — eine Datei, die
    ///   aussieht wie ein Ergebnis und keines ist.
    pub fn export(&self, out: &Path, audit: Option<&Path>) -> Result<Outcome> {
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

        let config = self.export_config(out, audit);
        let mut outcome = Outcome {
            input: config.input.display().to_string(),
            input_sha256: self.input_sha256.clone(),
            pages: self.page_count(),
            text_runs: self.runs.len(),
            candidates: self.regions.len(),
            // Die Warnungen des Extraktors gehören mit ins Log — genau wie in
            // `redact_pipeline::run`, wo sie ebenfalls vor der Schwärzung
            // eingetragen werden.
            warnings: self.extract_warnings.clone(),
            ..Default::default()
        };

        let mut copy = (**doc).clone();
        let blocked = self.blocked_regions();
        redact_pipeline::apply(&mut copy, &redactions, &blocked, &config, &mut outcome)?;
        outcome.blocked_details = redact_pipeline::describe_blocked(&blocked);
        Ok(outcome)
    }

    // ---------------------------------------------------------------- Review

    /// Schreibt den aktuellen Zustand in eine Review-Datei.
    ///
    /// `input.sha256` trägt die Prüfsumme des geladenen Dokuments. Früher blieb
    /// das Feld leer, weil dieses Crate keine `sha2`-Abhängigkeit hatte — mit
    /// der Folge, dass **jede** aus der GUI stammende Review-Datei auf jedes
    /// beliebige PDF angewendet werden konnte, in der GUI wie in der CLI (die
    /// eine leere Prüfsumme überspringt). Die Rechtecke säßen dann an falschen
    /// Stellen, und die Geheimnisse blieben stehen.
    pub fn to_review_file(&self) -> ReviewFile {
        let input = ReviewInput {
            path: self
                .pdf_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            sha256: self.input_sha256.clone(),
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

    /// Schreibt die Review-Datei — über den **einen** Schreibpfad.
    ///
    /// In der Datei stehen die gefundenen Geheimnisse im Klartext. Sie
    /// entstand hier lange mit `std::fs::write`: Modus 0644, durch einen
    /// Symlink hindurch, ohne atomares Umbenennen. Jetzt gilt dasselbe wie für
    /// `--review-out`: 0600, kein Symlink, ein Zug, und die Eingabedatei kann
    /// nicht getroffen werden.
    pub fn save_review_file(&self, path: &Path) -> Result<()> {
        let config = Config {
            force: true,
            ..self.config.clone()
        };
        redact_pipeline::write_review_file(path, &self.to_review_file(), &config)
    }

    /// Übernimmt eine Review-Datei (z.B. aus `redact-rs --review-out`).
    ///
    /// **Prüft zuerst die Identität.** Eine Review-Datei sagt nur „schwärze
    /// bei diesen Koordinaten“ — auf ein anderes Dokument angewendet liegen
    /// die Rechtecke auf beliebigen Stellen, das Ergebnis sieht geschwärzt aus
    /// und ist es nicht. Stimmen die Prüfsummen nicht überein, wird die Datei
    /// deshalb **abgelehnt** und nichts verändert.
    pub fn apply_review_file(&mut self, review: ReviewFile) -> Result<()> {
        let identity = redact_pipeline::check_review_identity(
            &review,
            &self.input_sha256,
            self.config.allow_unverified_review,
        )?;

        self.end_edit_sessions();
        self.history.record(&self.regions);
        self.regions = review
            .items
            .into_iter()
            .map(|item| {
                // Die Schwärzungsart steht in der Datei; die Vorgabe aus der
                // Konfiguration käme hier zu spät und würde sie überschreiben.
                let mut entry = AnnotatedRegion::new(item.region);
                // Ein Negativlisten-Treffer bleibt aus, egal was in der Datei steht.
                entry.enabled = item.enabled && !entry.is_blocking();
                entry.action = item.action;
                entry
            })
            .collect();
        self.selected_region = None;
        self.status = match identity {
            ReviewIdentity::Matches => format!(
                "Review übernommen: {} Einträge (Prüfsumme stimmt)",
                self.regions.len()
            ),
            // Hierher kommt nur, wer die Prüfung ausdrücklich abgeschaltet hat
            // (`--allow-unverified-review`). Dann gehört wenigstens gesagt,
            // dass die Zugehörigkeit zum Dokument niemand geprüft hat.
            _ => format!(
                "Review übernommen: {} Einträge — ohne Prüfsumme, \
                 Zugehörigkeit zum Dokument ungeprüft",
                self.regions.len()
            ),
        };
        Ok(())
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

// Prüfrunde 7: die beiden Quadratiken in der Trefferbilanz und die
// Reihenfolgezusage, auf der ihre Beseitigung steht. Eigene Datei, aber
// **Kindmodul von `state`** — sie fährt `resolution`, `is_off_page` und die
// Bilanz unmittelbar an.
#[cfg(test)]
#[path = "rev7_tests.rs"]
pub mod rev7_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use redact_core::{MatchType, Point};
    use redact_pipeline::review_identity;

    /// Einstellungen, wie sie `redact-rs --gui --patterns iban_de` erzeugt.
    fn iban_only() -> Config {
        Config {
            patterns: vec!["iban_de".to_string()],
            ..Config::default()
        }
    }

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
        // Nur die IBAN-Muster, damit die Tests nicht an anderen Treffern hängen.
        let mut state = AppState::with_config(iban_only());
        state
            .load_bytes(
                &redact_pdf::testing::demo_statement(),
                Some(PathBuf::from("demo.pdf")),
            )
            .expect("Demo-PDF ladbar");
        state
    }

    // ------------------------------------------------------ Verschlüsselung

    use redact_pipeline::testing::{ENCRYPTED_PDF, ENCRYPTED_PDF_IBAN, ENCRYPTED_PDF_PASSWORD};

    /// Ein verschlüsseltes Dokument geht nicht verloren, es wartet.
    #[test]
    fn an_encrypted_document_waits_for_its_password() {
        let mut state = AppState::with_config(iban_only());
        let error = state
            .load_bytes(ENCRYPTED_PDF, Some(PathBuf::from("auszug.pdf")))
            .expect_err("ohne Passwort darf nicht geladen werden");
        assert!(error.to_string().contains("verschlüsselt"), "{error}");
        assert!(state.needs_password(), "die Abfrage kommt gar nicht");
        assert!(!state.is_loaded());
        assert_eq!(state.pending_name(), "auszug.pdf");
    }

    /// Mit dem richtigen Passwort ist das Dokument wirklich da — Seiten, Text
    /// und Treffer inbegriffen.
    #[test]
    fn the_right_password_opens_the_waiting_document() {
        let mut state = AppState::with_config(iban_only());
        let _ = state.load_bytes(ENCRYPTED_PDF, Some(PathBuf::from("auszug.pdf")));

        state
            .unlock(ENCRYPTED_PDF_PASSWORD)
            .expect("richtiges Passwort");
        assert!(!state.needs_password());
        assert!(state.is_loaded());
        assert_eq!(state.page_count(), 1);
        let text: String = state.runs.iter().map(|r| r.text.clone()).collect();
        assert!(text.contains(ENCRYPTED_PDF_IBAN), "kein Text: {text:?}");
        assert_eq!(state.analyze().unwrap(), 1);
    }

    /// Ein falsches Passwort lässt die Frage stehen — und wird nicht behalten,
    /// sonst scheiterte auch der nächste Versuch daran.
    #[test]
    fn a_wrong_password_keeps_the_question_open_and_is_forgotten() {
        let mut state = AppState::with_config(iban_only());
        let _ = state.load_bytes(ENCRYPTED_PDF, None);

        let error = state.unlock("falsch").expect_err("falsches Passwort");
        assert!(!error.to_string().contains("falsch"), "{error}");
        assert!(state.needs_password(), "die Abfrage ist zugefallen");
        assert!(
            state.config.password.is_none(),
            "das falsche Passwort blieb hängen"
        );

        // Und der zweite Versuch geht durch.
        state
            .unlock(ENCRYPTED_PDF_PASSWORD)
            .expect("zweiter Versuch");
        assert!(state.is_loaded());
    }

    #[test]
    fn cancelling_the_question_leaves_the_document_closed() {
        let mut state = AppState::with_config(iban_only());
        let _ = state.load_bytes(ENCRYPTED_PDF, None);
        state.cancel_password();
        assert!(!state.needs_password());
        assert!(!state.is_loaded());
        assert!(state.config.password.is_none());
        assert!(state.unlock("egal").is_err(), "ohne Wartendes kein Versuch");
    }

    /// Das Passwort steht in keinem Text, den irgendetwas ausgeben könnte —
    /// der Zustand leitet `Debug` ab, und das reicht für einen Panik-Text.
    #[test]
    fn the_state_never_prints_the_password() {
        let mut state = AppState::with_config(iban_only());
        let _ = state.load_bytes(ENCRYPTED_PDF, Some(PathBuf::from("auszug.pdf")));
        state.unlock(ENCRYPTED_PDF_PASSWORD).unwrap();
        let dump = format!("{state:?}");
        assert!(!dump.contains(ENCRYPTED_PDF_PASSWORD), "{dump}");
        assert!(!state.status.contains(ENCRYPTED_PDF_PASSWORD));
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
        state.analyze().expect("Analyse läuft");
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
        state.analyze().unwrap();
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
        assert_eq!(index, Some(0));
        assert_eq!(state.selected_region, Some(0));
        assert!(state.regions[0].enabled);
        assert_eq!(state.regions[0].color, RegionColor::Manual);

        // Verschoben wird nur auf der Seite, die gezeigt wird — die Region
        // liegt auf Seite 2 (siehe [`AppState::move_selected`]). Beim Klick
        // auf eine Trefferzeile springt die Seite von selbst mit; hier von
        // Hand.
        state.current_page = 1;
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

    /// Ein von Hand aufgezogener Muster-Treffer ist nicht mehr das, was das
    /// Muster gefunden hat — er wird zur manuellen Region. Ein schützender
    /// Treffer der Negativliste behält dagegen seine Herkunft: aus Schutz darf
    /// durch ein Ziehen keine Schwärzung werden.
    #[test]
    fn resizing_turns_a_pattern_hit_into_a_manual_region_but_never_a_protecting_one() {
        let mut state = AppState::new();
        state.regions.push(AnnotatedRegion::new(pattern_region(
            0,
            Rect::new(10.0, 10.0, 50.0, 20.0),
        )));
        state.regions.push(AnnotatedRegion::new(negative_region(
            0,
            Rect::new(60.0, 10.0, 90.0, 20.0),
        )));

        let bigger = Rect::new(5.0, 5.0, 60.0, 30.0);
        assert!(state.set_region_rect(0, bigger));
        assert_eq!(state.regions[0].region.rect, bigger);
        assert!(matches!(
            state.regions[0].region.source,
            Source::Manual { .. }
        ));
        assert_eq!(state.regions[0].color, RegionColor::Manual);
        assert!(state.regions[0].enabled, "geschwärzt wird weiter");
        assert!(state.regions[0].is_hand_made(&state.config.action));

        let wider = Rect::new(55.0, 5.0, 95.0, 30.0);
        assert!(state.set_region_rect(1, wider));
        assert_eq!(state.regions[1].region.rect, wider);
        assert!(
            state.regions[1].is_blocking(),
            "der Schutz darf nicht verlorengehen"
        );
        assert_eq!(state.regions[1].color, RegionColor::AutoBookingNeg);
        assert!(!state.regions[1].enabled);

        // Ohne Region passiert nichts.
        assert!(!state.set_region_rect(7, bigger));
    }

    /// **Befund: ein geschützter Treffer verliert seinen Schutz, sobald man
    /// sein Rechteck anfasst.** Gemessen: Verbreiterung um **einen** Punkt
    /// macht aus `[Protecting, Blocked]` ein `[Protecting, Redacted]`.
    ///
    /// Die Entscheidung ist, das **so zu lassen** (Begründung an
    /// [`AppState::set_region_rect`]: ein von Hand gezogenes Rechteck an
    /// derselben Stelle überstimmt die Schutzliste ebenso, und geschwärzt wird
    /// mehr statt weniger) — aber es **anzusagen**. Dieser Test hält beides
    /// fest: die Wirkung und die Ansage.
    #[test]
    fn touching_a_protected_hit_says_that_it_now_overrides_the_protection() {
        let mut state = AppState::new();
        // Der Schutzeintrag …
        state.regions.push(AnnotatedRegion::new(negative_region(
            0,
            Rect::new(0.0, 0.0, 100.0, 30.0),
        )));
        // … und ein Fund, den er deckt.
        state.regions.push(AnnotatedRegion::new(pattern_region(
            0,
            Rect::new(10.0, 10.0, 50.0, 20.0),
        )));
        assert_eq!(
            state.hit_summary().outcomes,
            vec![HitOutcome::Protecting, HitOutcome::Blocked]
        );
        assert_eq!(state.hit_summary().redacted, 0);
        let status_before = state.status.clone();

        // Ein Punkt breiter.
        assert!(state.set_region_rect(1, Rect::new(10.0, 10.0, 51.0, 20.0)));

        assert_eq!(
            state.hit_summary().outcomes,
            vec![HitOutcome::Protecting, HitOutcome::Redacted],
            "so ist es, und so bleibt es"
        );
        assert_eq!(state.hit_summary().redacted, 1);
        // Sichtbar ist es auch: Farbe und Herkunft ändern sich mit.
        assert_eq!(state.regions[1].color, RegionColor::Manual);
        // Und gesagt wird es jetzt ebenfalls.
        assert_ne!(state.status, status_before);
        assert_eq!(state.status, PROTECTION_OVERRIDDEN);
        assert!(state.status.contains("Schutzliste"));
        assert!(state.status.contains("Strg+Z"));

        // Nur einmal je Zug: das nächste Bild desselben Ziehvorgangs meldet
        // nichts mehr, denn der Eintrag ist längst manuell.
        state.status = "läuft".to_string();
        assert!(state.set_region_rect(1, Rect::new(10.0, 10.0, 52.0, 20.0)));
        assert_eq!(state.status, "läuft");

        // Ein Treffer, den nichts schützt, löst die Warnung nicht aus.
        state.regions.push(AnnotatedRegion::new(pattern_region(
            0,
            Rect::new(300.0, 300.0, 360.0, 310.0),
        )));
        state.status = "unberührt".to_string();
        assert!(state.set_region_rect(2, Rect::new(300.0, 300.0, 361.0, 310.0)));
        assert_eq!(state.status, "unberührt");
    }

    /// Der Schnappschuss gehört an den Anfang eines Ziehvorgangs, nicht in
    /// jedes Bild: `set_region_rect` legt selbst keinen an.
    #[test]
    fn only_begin_manual_edit_writes_to_the_history() {
        let mut state = AppState::new();
        state.add_manual_region(0, Rect::new(0.0, 0.0, 10.0, 10.0), "test");
        let depth = state.history.undo_depth();

        state.begin_manual_edit();
        for step in 1..=30 {
            state.set_region_rect(0, Rect::new(0.0, 0.0, 10.0 + step as f64, 10.0));
        }
        assert_eq!(state.history.undo_depth(), depth + 1);

        assert!(state.undo());
        assert_eq!(
            state.regions[0].region.rect,
            Rect::new(0.0, 0.0, 10.0, 10.0)
        );
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

    /// **Befund #67.** `--action`/`--replace-with` galt in der Oberfläche
    /// nicht: `AnnotatedRegion::new` trug fest `Action::Blackout` ein, und
    /// `redact-rs --gui --action replace --replace-with "[IBAN]"` schwärzte
    /// schwarz.
    #[test]
    fn the_action_from_the_configuration_reaches_every_new_region() {
        let replace = Action::Replace("[IBAN]".to_string());
        let mut state = AppState::with_config(Config {
            action: replace.clone(),
            ..iban_only()
        });
        state
            .load_bytes(
                &redact_pdf::testing::demo_statement(),
                Some(PathBuf::from("demo.pdf")),
            )
            .unwrap();
        state.analyze().unwrap();
        assert!(!state.regions.is_empty(), "die Analyse fand nichts");

        // Jeder Fund trägt die Art aus der Konfiguration — sichtbar in der
        // Trefferliste, nicht bloß im Export.
        assert!(
            state.regions.iter().all(|a| a.action == replace),
            "die Treffer stehen auf {:?}",
            state.regions.iter().map(|a| &a.action).collect::<Vec<_>>()
        );
        // Ein von Hand gezogenes Rechteck ebenso.
        state.add_manual_region(0, Rect::new(10.0, 10.0, 50.0, 20.0), "Gehalt");
        assert_eq!(state.regions.last().unwrap().action, replace);

        // Und sie kommt bis in die Schwärzungen.
        let redactions = state.enabled_redactions();
        assert!(!redactions.is_empty());
        assert!(
            redactions.iter().all(|r| r.action == replace),
            "in den Schwärzungen steht etwas anderes"
        );
    }

    /// Ein unangetasteter Treffer ist **keine** Handarbeit, auch wenn der Lauf
    /// auf `--action replace` steht — sonst käme die Rückfrage „von Hand
    /// bearbeitete Schwärzungen verwerfen?“ bei jedem Öffnen.
    #[test]
    fn the_configured_action_alone_is_not_hand_work() {
        let mut state = AppState::with_config(Config {
            action: Action::Whiteout,
            ..iban_only()
        });
        state
            .load_bytes(
                &redact_pdf::testing::demo_statement(),
                Some(PathBuf::from("demo.pdf")),
            )
            .unwrap();
        state.analyze().unwrap();
        assert!(!state.has_manual_work(), "die Analyse allein zählt nicht");
        assert_eq!(state.hand_made_count(), 0);

        // Wer die Art *ändert*, hat dagegen Hand angelegt.
        assert!(state.set_action(0, Action::Blackout));
        assert!(state.has_manual_work());
        assert_eq!(state.hand_made_count(), 1);
    }

    /// Die Schwärzungsart steht am Ende auch in der Datei — gemessen an den
    /// Bytes, nicht am Feld.
    #[test]
    fn the_configured_action_changes_the_written_file() {
        let dir = temp_dir("action");
        let export_with = |action: Action, name: &str| {
            let mut state = AppState::with_config(Config {
                action,
                ..iban_only()
            });
            state
                .load_bytes(
                    &redact_pdf::testing::demo_statement(),
                    Some(PathBuf::from("demo.pdf")),
                )
                .unwrap();
            state.analyze().unwrap();
            let out = dir.join(name);
            state.export(&out, None).unwrap();
            std::fs::read(&out).unwrap()
        };

        let black = export_with(Action::Blackout, "schwarz.pdf");
        let replaced = export_with(Action::Replace("[IBAN]".to_string()), "ersetzt.pdf");
        assert_ne!(
            black, replaced,
            "--action muss in der geschriebenen Datei ankommen"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn export_removes_the_text_from_the_pdf() {
        let mut state = loaded_state();
        state.analyze().unwrap();
        assert!(!state.regions.is_empty());

        let dir = temp_dir("export");
        let out = dir.join("out.pdf");
        let audit = dir.join("audit.json");
        let outcome = state.export(&out, Some(&audit)).expect("Export läuft");
        assert!(outcome.removed_glyphs > 0);
        assert!(outcome.drawn_rects > 0);

        // Ergebnis erneut extrahieren: die IBAN darf nicht mehr auftauchen.
        let bytes = std::fs::read(&out).unwrap();
        let doc = redact_pdf::load_from_bytes(&bytes).unwrap();
        let (runs, _) = PdfExtractor::new().extract_with_warnings(&doc).unwrap();
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

        // Audit-Log: dasselbe, was die Kommandozeile schreibt.
        //
        // Vorher stand hier ein von Hand zusammengesetztes JSON mit **leeren**
        // Prüfsummen und hart verdrahtetem `metadata_stripped: true`. Ein
        // Nachweis ohne Prüfsummen bezeugt nichts, und die Metadatenzeile war
        // schlicht gelogen.
        let log: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&audit).unwrap()).unwrap();
        assert!(!log["redactions"].as_array().unwrap().is_empty());
        // Das Audit-Log zählt Seiten 0-basiert wie jede andere Datei auch
        // (`AuditEntry::page`); der erste Treffer des Demo-Auszugs steht auf
        // der ersten Seite.
        assert_eq!(log["redactions"][0]["page"], serde_json::json!(0));
        assert_eq!(
            log["input"]["sha256"].as_str().unwrap(),
            state.input_sha256,
            "das Log muss die Eingabe benennen können"
        );
        assert_eq!(log["output"]["sha256"].as_str().unwrap().len(), 64);
        // Gemessen, nicht behauptet: das Demo-PDF bringt ein /Info-Dictionary
        // mit, also *wurde* etwas entfernt — und das Log zählt es auf.
        assert_eq!(log["metadata_stripped"], serde_json::json!(true));
        assert!(!log["metadata"]["summary"].as_array().unwrap().is_empty());
        assert_eq!(log["effect"]["padding"], serde_json::json!(1.0));
        assert_eq!(
            log["effect"]["removed_glyphs"],
            serde_json::json!(outcome.removed_glyphs)
        );

        // Klartext, deshalb nur für die Eigentümerin lesbar (0600).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&audit).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "Audit-Log stand auf {mode:o}");
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Der Export benutzt **die** Polsterung aus der Konfiguration.
    ///
    /// Hier stand bis Aufgabe 5 ein Test, der die Kette daneben noch einmal von
    /// Hand aufschrieb und deshalb nie fehlschlagen konnte. Der echte Vergleich
    /// — dieselbe Datei aus dem `redact-rs`-Binary und aus dieser Oberfläche,
    /// Byte für Byte — steht in `redact-cli/tests/cli_and_gui_agree.rs`.
    ///
    /// Was hier bleibt, ist der Befund, der die beiden früher trennte: die
    /// Oberfläche polsterte mit fest verdrahteten 1,0 und war für `--padding`
    /// unerreichbar.
    #[test]
    fn export_uses_the_padding_from_the_configuration() {
        let dir = temp_dir("padding");

        let export_with = |padding: f64, name: &str| {
            let mut state = AppState::with_config(Config {
                padding,
                ..iban_only()
            });
            state
                .load_bytes(
                    &redact_pdf::testing::demo_statement(),
                    Some(PathBuf::from("demo.pdf")),
                )
                .unwrap();
            state.analyze().unwrap();
            let out = dir.join(name);
            let outcome = state.export(&out, None).unwrap();
            (std::fs::read(&out).unwrap(), outcome)
        };

        let (thin, _) = export_with(1.0, "thin.pdf");
        let (fat, _) = export_with(6.0, "fat.pdf");
        assert_ne!(thin, fat, "--padding muss in der Oberfläche ankommen");

        // Und der entartete Fall wird auch hier gemeldet statt als Erfolg
        // durchgereicht.
        let (_, degenerate) = export_with(-100.0, "leer.pdf");
        assert_eq!(degenerate.effective_redactions, 0);
        assert!(degenerate.degenerate_redactions > 0);
        assert!(
            degenerate.warnings.iter().any(|w| w.contains("leeres")),
            "{:?}",
            degenerate.warnings
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
        state.analyze().unwrap();
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
        state.analyze().unwrap();
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
        state.analyze().unwrap();
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
        state.analyze().unwrap();
        state.add_manual_region(1, Rect::new(70.0, 700.0, 200.0, 715.0), "Adresse");
        state.set_enabled(0, false);
        state.set_action(1, Action::Whiteout);

        let json = state.to_review_file().to_json().unwrap();
        let parsed = ReviewFile::from_json(&json).unwrap();

        // Dasselbe Dokument, deshalb dieselbe Prüfsumme — die Datei passt.
        let mut restored = loaded_state();
        restored.apply_review_file(parsed).expect("Review passt");
        assert!(
            restored.status.contains("Prüfsumme stimmt"),
            "{}",
            restored.status
        );

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
        // Ohne geladenes Dokument gibt es keine Prüfsumme zu vergleichen; hier
        // geht es um den Negativtreffer, deshalb die Prüfung ausdrücklich aus.
        let mut state = AppState::with_config(Config {
            allow_unverified_review: true,
            ..Config::default()
        });
        state.regions.push(AnnotatedRegion::new(negative_region(
            0,
            Rect::new(0.0, 0.0, 10.0, 10.0),
        )));
        let mut review = state.to_review_file();
        // Von Hand manipulierte Datei: `enabled` steht auf true.
        review.items[0].enabled = true;
        state.apply_review_file(review).unwrap();
        assert!(!state.regions[0].enabled);
        assert!(state.enabled_redactions().is_empty());
    }

    // ------------------------------------------------- Identität (Aufgabe 36)

    /// Der bekannte Wert aus dem SHA-256-Standard.
    ///
    /// Dass beide Programme dieselbe Rechnung benutzen, muss dieser Test nicht
    /// mehr behaupten: [`sha256_hex`] *ist* [`redact_pipeline::sha256_bytes`].
    #[test]
    fn sha256_matches_the_known_value_of_the_cli() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(sha256_hex(b"").len(), 64);
        assert_ne!(sha256_hex(b"a"), sha256_hex(b"b"));
    }

    /// **Die Review-Datei trägt die Prüfsumme ihres Dokuments.** Ohne sie
    /// ließe sie sich auf jedes beliebige PDF anwenden — die Rechtecke lägen
    /// dann an willkürlichen Stellen.
    #[test]
    fn review_input_names_pages_path_and_the_checksum_of_the_document() {
        let state = loaded_state();
        let review = state.to_review_file();
        assert_eq!(review.input.pages, 2);
        assert_eq!(review.input.path, "demo.pdf");
        assert_eq!(review.input.sha256.len(), 64, "{}", review.input.sha256);
        assert_eq!(
            review.input.sha256,
            sha256_hex(&redact_pdf::testing::demo_statement())
        );

        // Ohne Dokument gibt es nichts zu prüfen und nichts zu behaupten.
        assert_eq!(AppState::new().to_review_file().input.sha256, "");
    }

    #[test]
    fn review_identity_only_objects_when_both_sides_are_known() {
        let a = sha256_hex(b"Dokument A");
        let b = sha256_hex(b"Dokument B");
        assert_eq!(review_identity(&a, &a), ReviewIdentity::Matches);
        // Groß-/Kleinschreibung der Hexziffern darf nicht entscheiden.
        assert_eq!(
            review_identity(&a.to_uppercase(), &a),
            ReviewIdentity::Matches
        );
        assert_eq!(review_identity(&a, &b), ReviewIdentity::Mismatch);
        // Alte Dateien ohne Prüfsumme bzw. kein Dokument geladen.
        assert_eq!(review_identity("", &a), ReviewIdentity::Unchecked);
        assert_eq!(review_identity(&a, ""), ReviewIdentity::Unchecked);
        assert_eq!(review_identity("", ""), ReviewIdentity::Unchecked);
    }

    /// **Aufgabe 36.** Eine Review-Datei zu einem *anderen* Dokument wird
    /// abgelehnt, und zwar bevor irgendetwas am Zustand geändert ist. Vorher
    /// wurde sie stillschweigend angewendet: die Rechtecke saßen dann an den
    /// Koordinaten des fremden Dokuments, das Ergebnis sah geschwärzt aus und
    /// die Geheimnisse standen weiter da.
    #[test]
    fn a_review_file_for_a_different_document_is_refused() {
        // Review zu Dokument A, erstellt auf dem gedrehten Demo-PDF.
        let mut origin = AppState::new();
        origin
            .load_bytes(&rotated_demo(&[90, 90]), Some(PathBuf::from("anderes.pdf")))
            .unwrap();
        origin.add_manual_region(0, Rect::new(10.0, 10.0, 50.0, 20.0), "Gehalt");
        let review = origin.to_review_file();
        assert!(!review.input.sha256.is_empty());

        // Dokument B ist ein anderes.
        let mut target = loaded_state();
        target.analyze().unwrap();
        let before = target.regions.clone();
        let history_before = target.history.undo_depth();
        assert_ne!(target.input_sha256, origin.input_sha256);

        let error = target
            .apply_review_file(review.clone())
            .expect_err("fremde Review-Datei muss abgelehnt werden")
            .to_string();
        assert!(error.contains("anderen Dokument"), "{error}");
        // Die Meldung nennt beide Prüfsummen und die gemeinte Datei.
        assert!(error.contains(&origin.input_sha256[..12]), "{error}");
        assert!(error.contains(&target.input_sha256[..12]), "{error}");
        assert!(error.contains("anderes.pdf"), "{error}");
        // Und es wurde nichts angefasst — auch kein Schnappschuss abgelegt.
        assert_eq!(target.regions, before);
        assert_eq!(target.history.undo_depth(), history_before);

        // Zum richtigen Dokument geht dieselbe Datei durch.
        let mut right = AppState::new();
        right
            .load_bytes(&rotated_demo(&[90, 90]), Some(PathBuf::from("anderes.pdf")))
            .unwrap();
        right.apply_review_file(review).expect("passt");
        assert_eq!(right.regions.len(), 1);
    }

    /// **Aufgabe 54.** Eine Review-Datei ohne Prüfsumme wurde stillschweigend
    /// angewendet — in beiden Programmen. Damit genügte ein von Hand
    /// eingetragenes `"sha256": ""`, um die Identitätsprüfung vollständig
    /// auszuhebeln. Jetzt wird sie abgelehnt, und der Weg daran vorbei ist ein
    /// ausdrücklicher Schalter.
    #[test]
    fn a_review_file_without_a_checksum_is_refused_unless_allowed() {
        let mut state = loaded_state();
        let mut review = state.to_review_file();
        review.input.sha256 = String::new();
        review.items.clear();

        let before = state.regions.clone();
        let error = state
            .apply_review_file(review.clone())
            .expect_err("ohne Prüfsumme muss abgelehnt werden")
            .to_string();
        assert!(error.contains("keine Prüfsumme"), "{error}");
        assert!(error.contains("--allow-unverified-review"), "{error}");
        assert_eq!(
            state.regions, before,
            "es darf nichts verändert worden sein"
        );

        // Mit dem Schalter geht sie durch — die Statuszeile sagt trotzdem,
        // dass niemand die Zugehörigkeit geprüft hat.
        state.config.allow_unverified_review = true;
        state.apply_review_file(review).expect("wird angewendet");
        assert!(state.status.contains("ungeprüft"), "{}", state.status);
    }

    #[test]
    fn suggested_output_path_sits_next_to_the_input() {
        let mut state = AppState::new();
        assert_eq!(
            state.config.output_suffix,
            redact_core::DEFAULT_OUTPUT_SUFFIX
        );
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

        state.config.output_suffix = "_anonym".to_string();
        assert_eq!(
            state.suggested_output_path().unwrap(),
            PathBuf::from("/daten/kontoauszug_anonym.pdf")
        );

        // Leerer Zusatz fällt auf den Standard zurück, damit das Original
        // niemals überschrieben wird.
        state.config.output_suffix = String::new();
        assert_eq!(
            state.suggested_output_path().unwrap(),
            PathBuf::from("/daten/kontoauszug_geschwaerzt.pdf")
        );
    }

    /// Wurde `-o` genannt, steht der Speichern-Dialog auf diesem Namen — der
    /// Schalter endet nicht an der Fenstergrenze, er wird nur (sichtbar)
    /// bestätigt statt stillschweigend ausgeführt.
    #[test]
    fn a_given_output_path_is_what_the_dialog_proposes() {
        let mut state = AppState::with_config(Config {
            output: Some(PathBuf::from("/ziel/fertig.pdf")),
            ..Config::default()
        });
        state
            .load_bytes(
                &redact_pdf::testing::demo_statement(),
                Some(PathBuf::from("/daten/kontoauszug.pdf")),
            )
            .unwrap();
        assert_eq!(
            state.suggested_output_path().unwrap(),
            PathBuf::from("/ziel/fertig.pdf")
        );
        // Ohne `-o` bleibt es beim Namen neben dem Original.
        state.config.output = None;
        assert_eq!(
            state.suggested_output_path().unwrap(),
            PathBuf::from("/daten/kontoauszug_geschwaerzt.pdf")
        );
    }

    /// **Befund: `-o` klebte am Dokumentwechsel.** `config.output` wurde beim
    /// Laden nie zurückgesetzt — nach dem Öffnen eines zweiten PDF schlug der
    /// Dialog weiter den Ausgabenamen des **ersten** vor. Und solange er galt,
    /// war das Feld „Namenszusatz“ ohne jede Wirkung.
    #[test]
    fn a_given_output_path_does_not_follow_the_next_document() {
        let mut state = AppState::with_config(Config {
            input: PathBuf::from("/daten/erstes.pdf"),
            output: Some(PathBuf::from("/ziel/fertig.pdf")),
            ..Config::default()
        });
        let demo = redact_pdf::testing::demo_statement();

        // Das Dokument der Kommandozeile: `-o` gilt.
        state
            .load_bytes(&demo, Some(PathBuf::from("/daten/erstes.pdf")))
            .unwrap();
        assert!(state.output_name_is_fixed());
        assert_eq!(
            state.suggested_output_path().unwrap(),
            PathBuf::from("/ziel/fertig.pdf")
        );
        // Solange er gilt, bewirkt der Zusatz nichts — deshalb ist das Feld
        // abgeschaltet.
        state.config.output_suffix = "_test".to_string();
        assert_eq!(
            state.suggested_output_path().unwrap(),
            PathBuf::from("/ziel/fertig.pdf")
        );

        // Dasselbe Dokument noch einmal: `-o` bleibt.
        state
            .load_bytes(&demo, Some(PathBuf::from("/daten/erstes.pdf")))
            .unwrap();
        assert!(state.output_name_is_fixed());

        // Ein **anderes** Dokument: der Vorschlag folgt jetzt ihm.
        state
            .load_bytes(&demo, Some(PathBuf::from("/daten/zweites.pdf")))
            .unwrap();
        assert!(!state.output_name_is_fixed(), "-o gehörte zum ersten PDF");
        assert_eq!(
            state.suggested_output_path().unwrap(),
            PathBuf::from("/daten/zweites_test.pdf"),
            "und der Namenszusatz wirkt wieder"
        );
    }

    /// **Befund: Rechtecke neben der Seite wurden als „wird geschwärzt“
    /// gezählt.** Die Interaktionsfläche ist breiter als das Blatt; gemessen:
    /// Rechteck bei x 700…760 auf einer 595 pt breiten Seite, Kopfzeile „1
    /// werden geschwärzt“, nach dem Export `removed_glyphs = 0`, `covered = 1`.
    #[test]
    fn a_rectangle_beside_the_sheet_is_not_counted_as_a_redaction() {
        let mut state = loaded_state();
        let sheet = state.page_box(0).expect("Seite 0");
        assert!(sheet.ur.x < 600.0, "Demo-Auszug ist A4: {sheet:?}");

        // Ganz daneben: es entsteht nichts, und die Zeile sagt es.
        let outside = Rect::new(700.0, 400.0, 760.0, 420.0);
        assert_eq!(state.add_manual_region(0, outside, "daneben"), None);
        assert!(state.regions.is_empty(), "{:?}", state.regions);
        assert!(
            state.status.contains("außerhalb"),
            "und die Zeile sagt es sofort, nicht erst nach dem Export: {}",
            state.status
        );
        assert_eq!(state.hit_summary().redacted, 0);

        // Halb daneben: was auf dem Blatt liegt, bleibt — der Rest wird
        // abgeschnitten.
        let half = Rect::new(sheet.ur.x - 40.0, 400.0, sheet.ur.x + 100.0, 420.0);
        let index = state.add_manual_region(0, half, "halb daneben").unwrap();
        let kept = state.regions[index].region.rect;
        assert_eq!(kept.ur.x, sheet.ur.x, "am Blattrand ist Schluss");
        assert_eq!(kept.ll.x, sheet.ur.x - 40.0);
        assert_eq!(state.hit_summary().redacted, 1);

        // Und auch ein Zug am Eckgriff kommt nicht über das Blatt hinaus.
        assert!(state.set_region_rect(index, Rect::new(500.0, 400.0, 900.0, 420.0)));
        assert_eq!(state.regions[index].region.rect.ur.x, sheet.ur.x);
        // Ganz hinausgezogen ändert gar nichts.
        let before = state.regions[index].region.rect;
        assert!(!state.set_region_rect(index, Rect::new(800.0, 400.0, 900.0, 420.0)));
        assert_eq!(state.regions[index].region.rect, before);
    }

    /// Ohne geladenes Dokument gibt es kein Blatt — dann wird auch nichts
    /// beschnitten (sonst hinge das Verhalten an einem geratenen A4).
    #[test]
    fn without_a_document_nothing_is_clipped() {
        let mut state = AppState::new();
        assert_eq!(state.page_box(0), None);
        let far_out = Rect::new(5000.0, 5000.0, 5100.0, 5100.0);
        assert_eq!(state.clamp_to_page(0, far_out), Some(far_out));
        assert_eq!(state.add_manual_region(0, far_out, "ohne Blatt"), Some(0));
    }

    /// **Befund: `--review-out` galt in der Oberfläche nicht.** Kein einziges
    /// Vorkommen in `redact-gui` — derselbe Fehlertyp wie die drei Schalter,
    /// die schon still wegfielen. Die Regel ist die von
    /// `redact_pipeline::run`: genannter Pfad, sonst neben dem Original.
    #[test]
    fn review_out_from_the_command_line_is_what_the_dialog_proposes() {
        let mut state = AppState::with_config(Config {
            review_out: Some(PathBuf::from("/ziel/durchsicht.json")),
            ..Config::default()
        });
        state
            .load_bytes(
                &redact_pdf::testing::demo_statement(),
                Some(PathBuf::from("/daten/kontoauszug.pdf")),
            )
            .unwrap();
        assert_eq!(
            state.suggested_review_path().unwrap(),
            PathBuf::from("/ziel/durchsicht.json")
        );

        // Ohne den Schalter bleibt es beim Namen neben dem Original — genau
        // wie `sibling_path` ihn auch auf der Kommandozeile bildet.
        state.config.review_out = None;
        assert_eq!(
            state.suggested_review_path().unwrap(),
            PathBuf::from("/daten/kontoauszug_review.json")
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
                state.config.output_suffix = suffix.to_string();
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

    /// Die Zoomknöpfe der Symbolleiste gehen stufenweise und laufen an den
    /// Anschlag, statt darüber hinaus.
    #[test]
    fn zoom_steps_stay_inside_the_limits() {
        let mut state = AppState::new();
        assert_eq!(state.zoom, 1.0);
        state.zoom_in();
        assert!((state.zoom - ZOOM_STEP).abs() < 1e-6, "{}", state.zoom);
        state.zoom_out();
        assert!((state.zoom - 1.0).abs() < 1e-6, "{}", state.zoom);

        for _ in 0..50 {
            state.zoom_in();
        }
        assert_eq!(state.zoom, MAX_ZOOM);
        assert!(!state.can_zoom_in());
        assert!(state.can_zoom_out());

        for _ in 0..50 {
            state.zoom_out();
        }
        assert_eq!(state.zoom, MIN_ZOOM);
        assert!(!state.can_zoom_out());
        assert!(state.can_zoom_in());

        state.zoom_reset();
        assert_eq!(state.zoom, 1.0);
    }

    /// Pos1 und Ende springen an die Enden des Dokuments.
    #[test]
    fn home_and_end_jump_to_the_first_and_last_page() {
        let mut state = loaded_state();
        assert!(state.is_first_page());
        assert!(!state.is_last_page());

        state.last_page();
        assert_eq!(state.current_page, 1);
        assert!(state.is_last_page());
        assert!(!state.is_first_page());

        state.first_page();
        assert_eq!(state.current_page, 0);

        // Ohne Dokument gibt es keine Seite — und keinen Absturz.
        let mut empty = AppState::new();
        empty.last_page();
        assert_eq!(empty.current_page, 0);
        assert!(empty.is_first_page());
        assert!(empty.is_last_page());
    }

    // ------------------------------------------ Rückgängig / Wiederholen

    /// Jede Änderung an der Trefferliste ist zurücknehmbar — und ein
    /// Rückgängig ist selbst wieder zurücknehmbar.
    #[test]
    fn every_kind_of_edit_can_be_undone_and_redone() {
        let mut state = loaded_state();
        assert!(
            !state.can_undo(),
            "frisch geladen gibt es nichts zurückzunehmen"
        );
        assert!(!state.can_redo());
        assert!(!state.undo(), "ohne Verlauf passiert nichts");

        // 1) Analyse.
        state.analyze().unwrap();
        let after_analysis = state.regions.clone();
        assert!(!after_analysis.is_empty());
        assert!(state.can_undo());

        // 2) Rechteck von Hand.
        state.add_manual_region(0, Rect::new(10.0, 10.0, 50.0, 20.0), "Gehalt");
        // 3) Abwählen.
        assert!(state.set_enabled(0, false));
        // 4) Schwärzungsart.
        assert!(state.set_action(0, Action::Whiteout));
        // 5) Verschieben.
        state.selected_region = Some(0);
        assert!(state.move_selected(3.0, 0.0));
        // 6) Löschen.
        assert!(state.delete_selected());
        let after_all_edits = state.regions.clone();

        // Sechs Schritte zurück landen wieder beim Ergebnis der Analyse.
        for _ in 0..5 {
            assert!(state.undo());
        }
        assert_eq!(state.regions, after_analysis);
        assert_eq!(state.selected_region, None, "die Auswahl wird aufgehoben");

        // Noch einer: vor der Analyse war die Liste leer.
        assert!(state.undo());
        assert!(state.regions.is_empty());
        assert!(!state.can_undo());

        // Und wieder vor bis ganz nach hinten.
        for _ in 0..6 {
            assert!(state.redo());
        }
        assert!(!state.can_redo());
        assert_eq!(state.regions, after_all_edits);
    }

    /// Nach einer neuen Änderung darf das Wiederholen nicht in einen Zweig
    /// führen, den es nicht mehr gibt — und der Stapel ist begrenzt.
    #[test]
    fn redo_expires_after_a_new_change_and_the_stack_is_bounded() {
        use crate::history::HISTORY_LIMIT;

        let mut state = AppState::new();
        state.add_manual_region(0, Rect::new(0.0, 0.0, 10.0, 10.0), "eins");
        assert!(state.undo());
        assert!(state.can_redo());

        state.add_manual_region(0, Rect::new(20.0, 20.0, 30.0, 30.0), "zwei");
        assert!(!state.can_redo(), "Wiederholen muss verfallen sein");
        assert!(!state.redo());

        // Mehr Änderungen als der Stapel fasst.
        let mut state = AppState::new();
        for i in 0..(HISTORY_LIMIT + 20) {
            state.add_manual_region(0, Rect::new(i as f64, 0.0, i as f64 + 1.0, 1.0), "viele");
        }
        assert_eq!(state.history.undo_depth(), HISTORY_LIMIT);
        while state.undo() {}
        // 70 Rechtecke, 50 aufbewahrte Schritte → 20 bleiben stehen.
        assert_eq!(state.regions.len(), 20);
    }

    /// Ein neues Dokument bringt einen neuen Verlauf mit.
    #[test]
    fn loading_a_document_clears_the_history() {
        let mut state = loaded_state();
        state.add_manual_region(0, Rect::new(0.0, 0.0, 10.0, 10.0), "Gehalt");
        assert!(state.can_undo());
        state
            .load_bytes(&redact_pdf::testing::demo_statement(), None)
            .unwrap();
        assert!(!state.can_undo());
        assert!(!state.can_redo());
    }

    /// **Befund: Tippen im Feld „Ersetzen“ flutete den Rückgängig-Stapel.**
    /// Jede Textänderung legte einen Schnappschuss ab — gemessen: 19
    /// Tastendrücke → 20 Schritte. Ein Ersatztext von rund 50 Zeichen schob
    /// damit bei einer Grenze von 50 **jeden** älteren Stand hinaus, auch den
    /// vor einem versehentlichen Löschen.
    #[test]
    fn typing_a_replacement_is_one_step_in_the_history_not_one_per_key() {
        let mut state = loaded_state();
        state.analyze().unwrap();
        state.set_action(0, Action::Replace("[X]".into()));
        let depth = state.history.undo_depth();

        // 19 Anschläge, wie gemessen.
        let text = "Musterfirma GmbH XY";
        assert_eq!(text.chars().count(), 19);
        let mut typed = String::new();
        for ch in text.chars() {
            typed.push(ch);
            assert!(state.edit_replacement(0, typed.clone()));
        }
        assert_eq!(state.regions[0].action, Action::Replace(text.to_string()));
        assert_eq!(
            state.history.undo_depth(),
            depth + 1,
            "eine Tippsitzung ist ein Schritt — vorher waren es 20"
        );

        // Und Rückgängig führt zum Stand **vor** dem Tippen, nicht einen
        // Buchstaben zurück.
        state.undo();
        assert_eq!(state.regions[0].action, Action::Replace("[X]".into()));

        // Nach dem Verlassen des Feldes beginnt eine neue Sitzung.
        state.redo();
        assert!(!state.is_editing_replacement());
        state.edit_replacement(0, "abc");
        assert!(state.is_editing_replacement());
        let depth = state.history.undo_depth();
        state.end_replacement_edit();
        state.edit_replacement(0, "abcd");
        assert_eq!(
            state.history.undo_depth(),
            depth + 1,
            "eine neue Sitzung ist ein neuer Schritt"
        );
    }

    /// Und die eigentliche Folge des Befundes: der ältere Stand überlebt das
    /// Tippen eines langen Ersatztextes.
    #[test]
    fn a_long_replacement_no_longer_pushes_the_whole_history_out() {
        let mut state = loaded_state();
        state.analyze().unwrap();
        let before_the_mistake = state.regions.clone();

        // Das versehentliche Löschen, das man gleich zurückholen möchte.
        state.selected_region = Some(0);
        state.delete_selected();
        state.set_action(0, Action::Replace(String::new()));

        // Ein Ersatztext mit mehr Zeichen als der Stapel Plätze hat.
        let long: String = std::iter::repeat_n('x', crate::HISTORY_LIMIT + 5).collect();
        let mut typed = String::new();
        for ch in long.chars() {
            typed.push(ch);
            state.edit_replacement(0, typed.clone());
        }

        // Zurück: Ersatztext, dann die Art, dann das Löschen.
        while state.can_undo() {
            state.undo();
            if state.regions == before_the_mistake {
                return;
            }
        }
        panic!("der Stand vor dem Löschen ist aus dem Verlauf gefallen");
    }

    /// **Befund: Oberfläche und Kommandozeile schlugen verschiedene Ersatztexte
    /// vor.** In der Seitenleiste stand fest `"[REDACTED]"` — englisch in einer
    /// deutschen Oberfläche und ein anderer Text als der Vorgabewert von
    /// `--replace-with`; der Schalter selbst blieb wirkungslos.
    #[test]
    fn the_replacement_default_matches_the_command_line() {
        // Ohne Angabe: dieselbe Zeichenkette wie die clap-Vorgabe von
        // `--replace-with` (crates/redact-cli/src/cli.rs, `default_value`).
        assert_eq!(DEFAULT_REPLACEMENT, "[GESCHWÄRZT]");
        assert!(!DEFAULT_REPLACEMENT.contains("REDACTED"));
        assert_eq!(AppState::new().default_replacement(), DEFAULT_REPLACEMENT);

        // Mit `--action replace --replace-with "[IBAN]"` gilt genau das.
        let state = AppState::with_config(Config {
            action: Action::Replace("[IBAN]".into()),
            ..Config::default()
        });
        assert_eq!(state.default_replacement(), "[IBAN]");

        // Auch `--action whiteout` lässt den Vorschlag deutsch bleiben.
        let state = AppState::with_config(Config {
            action: Action::Whiteout,
            ..Config::default()
        });
        assert_eq!(state.default_replacement(), DEFAULT_REPLACEMENT);

        // Und der zweite Teil desselben Befunds: `--replace-with` gilt auch
        // dann, wenn die Schwärzungsart etwas anderes ist. Wer die Datei mit
        // `--action blackout --replace-with "[IBAN]"` öffnet und dann in der
        // Trefferliste auf „Ersetzen" umstellt, meint seinen Text — nicht die
        // Vorgabe. Vorher las `default_replacement` nur `action` und verlor ihn.
        let state = AppState::with_config(Config {
            action: Action::Blackout,
            replace_with: "[IBAN]".into(),
            ..Config::default()
        });
        assert_eq!(state.default_replacement(), "[IBAN]");
    }

    /// Was nichts ändert, gehört nicht in den Verlauf: sonst klickt man
    /// dreimal Rückgängig, bevor überhaupt etwas passiert.
    #[test]
    fn unchanged_values_do_not_fill_the_history() {
        let mut state = AppState::new();
        state.regions.push(AnnotatedRegion::new(pattern_region(
            0,
            Rect::new(0.0, 0.0, 10.0, 10.0),
        )));
        assert!(state.set_enabled(0, true), "war schon an");
        assert!(state.set_action(0, Action::Blackout), "war schon schwarz");
        assert!(!state.can_undo());

        // Eine echte Änderung dagegen schon.
        assert!(state.set_enabled(0, false));
        assert!(state.can_undo());
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

    /// Grafische Elemente brauchen nach WCAG 1.4.11 mindestens 3:1 — **in
    /// beiden Themen**.
    ///
    /// Die Flächen kommen nicht mehr von Hand notiert, sondern aus
    /// [`crate::theme::Theme::backgrounds`], also aus den Werten, die egui
    /// wirklich malt: weißes Blatt, Bereichs- und Fensterfüllung sowie der
    /// „extreme“ Hintergrund (Textfelder, Listen). Ein Wechsel des Themas
    /// darf keine der vier Trefferfarben unsichtbar machen.
    #[test]
    fn region_colours_reach_the_graphic_contrast_minimum() {
        use crate::theme::{Theme, THEMES};

        for theme in THEMES {
            for background in theme.backgrounds() {
                for color in REGION_COLORS {
                    let ratio = contrast(color.rgb(), background);
                    assert!(
                        ratio >= 3.0,
                        "{color:?} erreicht im Thema {theme:?} gegen {background:?} \
                         nur {ratio:.2}:1"
                    );
                }
            }
        }

        // Die beiden Themen müssen sich überhaupt unterscheiden, sonst prüfte
        // die Schleife oben zweimal dasselbe.
        assert_ne!(Theme::Light.backgrounds(), Theme::Dark.backgrounds());

        // Das alte Orange scheiterte genau daran — Beleg, dass der Test greift.
        assert!(contrast((240, 150, 30), (255, 255, 255)) < 3.0);
        // Und ein dunkles Blau bestünde die Prüfung gegen weißes Papier
        // mühelos, verschwände aber im dunklen Thema. Dass dieser Fall
        // auffällt, ist der ganze Zweck der Erweiterung auf beide Themen.
        let navy = (30, 40, 90);
        assert!(contrast(navy, crate::theme::PAPER) >= 3.0);
        assert!(Theme::Dark
            .backgrounds()
            .iter()
            .any(|bg| contrast(navy, *bg) < 3.0));
    }

    #[test]
    fn descriptions_are_written_for_bank_customers() {
        let konto = plain_description(&Source::Pattern {
            pattern_id: "konto_nr".into(),
            confidence: 0.4,
        });
        // Der genaue Wortlaut gehört `redact-patterns`; hier zählt, dass die
        // **Beschreibung** des Musters gezeigt wird und nicht dessen ID.
        assert!(konto.starts_with("Kontonummer"), "{konto}");
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
        assert_eq!(summary.rows(), 5, "fünf Zeilen stehen in der Liste");
        assert_eq!(summary.found, 4, "der Schutzeintrag ist kein Fund");
        assert_eq!(summary.protecting, 1);
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
        assert_eq!(
            summary.headline(),
            "4 Treffer · 1 werden geschwärzt · 1 geschützt"
        );
        // Genau ein Eintrag wird gefüllt gezeichnet.
        assert_eq!(
            summary.outcomes.iter().filter(|o| o.is_redacted()).count(),
            1
        );
        // „geschützt“ statt Durchstreichen.
        assert_eq!(HitOutcome::Protecting.note(), "geschützt");
    }

    /// **Befund: die Kopfzeile zählte Schutzeinträge als „Treffer“.** Gemessen:
    /// „2 Treffer · 1 werden geschwärzt“ bei einem Musterfund und einem Eintrag
    /// der Negativliste — der erste war gar kein Fund.
    #[test]
    fn the_headline_does_not_count_protection_as_a_hit() {
        let mut state = AppState::new();
        // Der Schutzeintrag steht zuerst — genau die gemessene Reihenfolge.
        state.regions.push(AnnotatedRegion::new(negative_region(
            0,
            Rect::new(0.0, 0.0, 100.0, 30.0),
        )));
        state.regions.push(AnnotatedRegion::new(pattern_region(
            0,
            Rect::new(200.0, 200.0, 260.0, 210.0),
        )));

        let summary = state.hit_summary();
        assert_eq!(summary.rows(), 2, "beide Zeilen stehen in der Liste");
        assert_eq!(summary.found, 1, "aber nur einer ist ein Fund");
        assert_eq!(summary.protecting, 1);
        assert_eq!(
            summary.headline(),
            "1 Treffer · 1 werden geschwärzt · 1 geschützt"
        );

        // Ohne Schutzeintrag bleibt die Zeile so kurz wie bisher.
        state.regions.remove(0);
        assert_eq!(
            state.hit_summary().headline(),
            "1 Treffer · 1 werden geschwärzt"
        );
    }

    #[test]
    fn hit_summary_is_empty_without_regions() {
        let summary = AppState::new().hit_summary();
        assert_eq!(summary.found, 0);
        assert_eq!(summary.protecting, 0);
        assert_eq!(summary.rows(), 0);
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
        state.analyze().unwrap();
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
