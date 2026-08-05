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
//!   Befund je Region und [`AuditEntry::removed_glyphs`] mit der Zahl der
//!   Zeichen, die **diese** Region entfernt hat,
//! * [`EffectRecord`] — wie viele Zeichen, Deck-Rechtecke, Annotationen und
//!   Bilder die Schwärzung wirklich angefasst hat,
//! * [`MetadataRecord`] — was der Metadatenlauf wirklich entfernt hat.
//!
//! ## Der Befund je Region kommt aus der Messung
//!
//! Er wurde lange **allein am Rechteck** gefällt: „nach `--padding` nicht
//! leer“ hieß `applied`. Die Seite kam in der Bewertung gar nicht vor. Eine
//! Region auf `"page": 3` in einem dreiseitigen Dokument — die naheliegende
//! Verwechslung eines Menschen, der ab 1 zählt — wurde von der Schwärzung nie
//! angefasst, es gibt diese Seite nicht, und das Log bescheinigte trotzdem
//! `applied`. Ebenso eine Region, deren Koordinaten danebenliegen: gültiges
//! Rechteck, kein getroffenes Zeichen, `applied`.
//!
//! Die Wahrheit liefert [`redact_pdf::RedactionReport::per_redaction`] — die
//! Zahl der Zeichen, die jede einzelne Region entfernt hat. [`Effects`] wertet
//! sie zusammen mit der Seitenzahl des Dokuments aus und ist die **eine**
//! Quelle für Log, Zusammenfassung und Warnungen.
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
    /// Wonach dieser Lauf **nicht** gesucht hat.
    ///
    /// `#[serde(default)]`, damit ein vor dieser Fassung geschriebenes Log
    /// weiterhin einlesbar bleibt. Geschrieben wird das Feld **immer**, auch
    /// wenn nichts abgeschaltet war — siehe [`PatternRecord`].
    #[serde(default)]
    pub patterns: PatternRecord,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Welche automatische Erkennung dieser Lauf **nicht** gemacht hat.
///
/// ## Warum das Feld immer geschrieben wird
///
/// „Nichts abgeschaltet“ ist die Aussage, auf die sich ein Prüfer verlassen
/// können muss — und sie muss dastehen. Ein Feld, das nur im Ausnahmefall
/// erscheint, macht ein Log mit abgeschalteter Erkennung ununterscheidbar von
/// einem Log, das eine ältere Fassung des Werkzeugs geschrieben hat: in beiden
/// fehlt es. Ein Nachweis, dem man ansieht, dass er nichts verschweigt, ist der
/// halbe Zweck der Datei.
///
/// Derselbe Sachverhalt steht zusätzlich als Satz in [`AuditLog::warnings`]
/// (siehe [`crate::detection_notice`]) — einmal für Maschinen, einmal für
/// Menschen; gebaut werden beide aus derselben [`crate::Config`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatternRecord {
    /// Die automatische Erkennung war ganz abgeschaltet (`--no-patterns`).
    pub all_disabled: bool,
    /// Einzeln abgeschaltete Muster (`--disable-pattern`), in der angegebenen
    /// Reihenfolge, getrimmt und ohne Dubletten.
    #[serde(default)]
    pub disabled: Vec<String>,
}

impl PatternRecord {
    /// Was die Einstellungen dieses Laufs über die Erkennung sagen.
    pub fn of(config: &crate::Config) -> Self {
        Self {
            all_disabled: config.no_patterns,
            disabled: config
                .disabled_pattern_ids()
                .into_iter()
                .map(str::to_string)
                .collect(),
        }
    }

    /// Ist überhaupt etwas abgeschaltet?
    pub fn anything_disabled(&self) -> bool {
        self.all_disabled || !self.disabled.is_empty()
    }
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
    /// Zeichen, die **diese** Region aus dem Content-Stream entfernt hat.
    ///
    /// Aus [`redact_pdf::RedactionReport::per_redaction`], also gemessen.
    /// Überlappende Regionen werden jede für sich gezählt; die Summe über alle
    /// Einträge kann deshalb größer sein als [`EffectRecord::removed_glyphs`].
    pub removed_glyphs: usize,
    pub action: redact_core::Action,
    pub reason: String,
    pub source: String,
}

/// Was aus einer geplanten Schwärzung geworden ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryEffect {
    /// Die Region hat mindestens ein Zeichen aus dem Content-Stream entfernt.
    /// Das ist der einzige Befund, der eine ausgeführte Schwärzung bezeugt.
    Applied,
    /// Gültiges Rechteck auf einer vorhandenen Seite, Deck-Rechteck gezeichnet
    /// — aber kein einziges Zeichen getroffen.
    ///
    /// **Nicht dasselbe wie [`EntryEffect::MissingPage`], und mit Absicht kein
    /// Fehler:** über einer Grafik oder einem Rasterbild steht kein Text, der
    /// zu entfernen wäre; die Bildpunkte darunter werden von
    /// [`redact_pdf::PdfRedactor`] trotzdem überschrieben, die Schwärzung
    /// wirkt also. Dieselbe Null entsteht aber auch, wenn die Koordinaten
    /// danebenliegen — und dann steht der Text lesbar in der Ausgabe. Was von
    /// beidem zutrifft, kann das Programm nicht entscheiden: es kennt nur die
    /// Zeichen, die es *gefunden* hat. Deshalb ein eigener Befund, der gesagt
    /// und nicht als `applied` verbucht wird.
    Covered,
    /// Das Rechteck ist nach `--padding` leer. Eine solche Region wird von
    /// [`redact_pdf::PdfRedactor`] übersprungen: kein Zeichen entfernt, kein
    /// Rechteck gezeichnet. Sie darf nicht als Erfolg gelten.
    Degenerate,
    /// Die Region nennt eine Seite, die es im Dokument nicht gibt.
    ///
    /// Hier ist überhaupt nichts geschehen — kein Zeichen entfernt, kein
    /// Rechteck gezeichnet, keine Annotation entfernt. Der häufigste Weg
    /// dorthin ist die verwechselte Zählweise: in jeder JSON-Datei ist die
    /// erste Seite `0`, nur der Fließtext sagt „Seite 1“.
    MissingPage,
    /// Das Rechteck liegt **vollständig neben dem Blatt** seiner Seite.
    ///
    /// Der Nachbarfall von [`EntryEffect::MissingPage`]: die Seite gibt es, die
    /// Koordinaten treffen sie nur nicht. Kein Zeichen kann darunter liegen,
    /// und was außerhalb der MediaBox gezeichnet wird, sieht kein Betrachter —
    /// die Schwärzung ist wirkungslos, bevor irgendetwas gemessen wurde.
    ///
    /// ## Warum das ein eigener Befund ist
    ///
    /// **Es war einer — aber nur in der Oberfläche.** Dort heißt er
    /// `HitOutcome::OffPage`, wird *vor* dem Export gesagt und aus der Zahl
    /// „n werden geschwärzt“ herausgerechnet. Die Kommandozeile kannte ihn
    /// nicht: dasselbe Rechteck ging dort als geplante Schwärzung durch, wurde
    /// als [`EntryEffect::Covered`] verbucht („Deck-Rechteck gezeichnet, kein
    /// Zeichen entfernt“) und kam als Warnung **hinterher**. Genau die
    /// Divergenz zwischen beiden Programmen, die dieses Crate abstellen soll.
    ///
    /// Und `Covered` war dabei sachlich falsch: dort ist kein Deck-Rechteck
    /// gezeichnet worden, das jemand sähe. `Covered` heißt „über einer Grafik
    /// steht eben kein Text“ und ist häufig richtig; diesen Fall darunter zu
    /// mischen machte den häufigen Befund unbrauchbar für den seltenen.
    ///
    /// ## Warum er **vor** der Messung entschieden wird
    ///
    /// Weil er sich vor der Messung entscheiden lässt: er hängt an den
    /// Koordinaten und an der MediaBox, nicht am Ergebnis. Deshalb steht er in
    /// [`EntryEffect::of`] noch vor der Frage nach den entfernten Zeichen —
    /// dieselbe Reihenfolge, in der die Oberfläche ihn stellt.
    OffPage,
}

impl EntryEffect {
    /// Bewertet eine Region gegen das, was gemessen wurde.
    ///
    /// `sheets` sind die geprüften MediaBoxen des verarbeiteten Dokuments (eine
    /// je Seite, siehe [`redact_pdf::document::sane_page_boxes`]);
    /// `removed_glyphs` ist der Eintrag dieser Region in
    /// [`redact_pdf::RedactionReport::per_redaction`].
    ///
    /// Die Reihenfolge der Prüfungen ist Absicht:
    ///
    /// 1. eine Seite, die es nicht gibt, macht jede weitere Frage
    ///    gegenstandslos — dort ist kein Rechteck zu klein und keines daneben,
    ///    dort geschieht gar nichts;
    /// 2. ein Rechteck, von dem `--padding` nichts übrig lässt, wird von
    ///    [`redact_pdf::PdfRedactor`] übersprungen — auch das entscheidet sich
    ///    ohne Messung;
    /// 3. ein Rechteck neben dem Blatt kann kein Zeichen treffen. **Vor** der
    ///    Frage nach den entfernten Zeichen, weil das der Grund ist und nicht
    ///    die Folge: „kein Zeichen entfernt“ wäre hier eine richtige Zahl mit
    ///    der falschen Erklärung.
    ///
    /// Die ersten drei brauchen den Bericht der Schwärzung überhaupt nicht —
    /// sie stünden auch fest, bevor irgendetwas geschwärzt wurde. Genau
    /// deshalb kann die Oberfläche dieselbe Auskunft **vor** dem Export geben.
    pub fn of(redaction: &Redaction, padding: f64, sheets: &[Rect], removed_glyphs: usize) -> Self {
        let Some(sheet) = sheets.get(redaction.region.page) else {
            return Self::MissingPage;
        };
        if redaction.region.rect.expanded(padding).is_empty() {
            return Self::Degenerate;
        }
        if beside_the_sheet(&redaction.region.rect, sheet) {
            return Self::OffPage;
        }
        if removed_glyphs > 0 {
            Self::Applied
        } else {
            Self::Covered
        }
    }
}

/// Liegt `rect` vollständig neben `sheet`?
///
/// **Dieselbe Rechnung wie `redact_gui::AppState::clamp_to_page`**, und das ist
/// der Punkt: was dort nichts übrig lässt, kann auch hier nichts schwärzen.
/// Beide Programme müssen zu demselben Rechteck dasselbe sagen, sonst ist die
/// Divergenz nur an eine andere Stelle gewandert.
///
/// Gerechnet wird auf dem **ungepolsterten** Rechteck: `--padding` vergrößert
/// oder verkleinert die Fläche, verschiebt sie aber nicht auf die Seite. Ein
/// Rechteck, das erst durch eine großzügige Polsterung das Blatt berührt,
/// träfe dort ohnehin keinen Text, den die Analyse gefunden hätte.
fn beside_the_sheet(rect: &Rect, sheet: &Rect) -> bool {
    let rect = rect.normalized();
    let sheet = sheet.normalized();
    let (x0, x1) = (rect.ll.x.max(sheet.ll.x), rect.ur.x.min(sheet.ur.x));
    let (y0, y1) = (rect.ll.y.max(sheet.ll.y), rect.ur.y.min(sheet.ur.y));
    !(x1 > x0 && y1 > y0)
}

/// Der Befund je Region samt Summen — die eine Quelle für Log, Zusammenfassung
/// und Warnungen.
///
/// Einmal gerechnet und dann herumgereicht, damit Nachweis (`audit.json`),
/// Konsolenausgabe und Warnungen nicht auseinanderlaufen können: vorher
/// bewertete jede der drei Stellen für sich, und alle drei bewerteten falsch.
#[derive(Debug, Clone, Default)]
pub struct Effects {
    /// Befund je übergebener Schwärzung, in derselben Reihenfolge.
    pub per_entry: Vec<EntryEffect>,
    /// Entfernte Zeichen je übergebener Schwärzung (gemessen).
    pub removed_glyphs: Vec<usize>,
    /// Rand, mit dem gerechnet wurde.
    pub padding: f64,
    /// Seitenzahl des Dokuments.
    pub pages: usize,
    /// Zahl der Regionen mit [`EntryEffect::Applied`].
    pub applied: usize,
    /// Zahl der Regionen mit [`EntryEffect::Covered`].
    pub covered: usize,
    /// Zahl der Regionen mit [`EntryEffect::Degenerate`].
    pub degenerate: usize,
    /// Zahl der Regionen mit [`EntryEffect::MissingPage`].
    pub missing_page: usize,
    /// Zahl der Regionen mit [`EntryEffect::OffPage`].
    pub off_page: usize,
    /// Die angesprochenen, aber nicht vorhandenen Seiten — 0-basiert,
    /// aufsteigend, ohne Dubletten.
    pub missing_pages: Vec<usize>,
    /// Die Seiten, neben denen ein Rechteck lag — 0-basiert, aufsteigend, ohne
    /// Dubletten.
    ///
    /// Aus demselben Grund wie [`Effects::missing_pages`]: die Zahl allein
    /// sagt nicht, wo nachzusehen ist.
    pub off_page_pages: Vec<usize>,
}

impl Effects {
    /// Bewertet jede Schwärzung gegen den Bericht der Schwärzung.
    ///
    /// `report.per_redaction` ist index-gleich mit `redactions`; fehlt ein
    /// Eintrag (das kann nur ein Programmfehler sein), wird `0` angenommen —
    /// die vorsichtige Richtung, denn `0` heißt „nichts nachgewiesen“.
    pub fn measure(
        redactions: &[Redaction],
        padding: f64,
        sheets: &[Rect],
        report: &RedactionReport,
    ) -> Self {
        let removed_glyphs: Vec<usize> = (0..redactions.len())
            .map(|i| report.per_redaction.get(i).copied().unwrap_or(0))
            .collect();
        let per_entry: Vec<EntryEffect> = redactions
            .iter()
            .zip(&removed_glyphs)
            .map(|(r, removed)| EntryEffect::of(r, padding, sheets, *removed))
            .collect();

        let mut effects = Self {
            padding,
            pages: sheets.len(),
            ..Self::default()
        };
        for (redaction, effect) in redactions.iter().zip(&per_entry) {
            match effect {
                EntryEffect::Applied => effects.applied += 1,
                EntryEffect::Covered => effects.covered += 1,
                EntryEffect::Degenerate => effects.degenerate += 1,
                EntryEffect::OffPage => {
                    effects.off_page += 1;
                    let page = redaction.region.page;
                    if let Err(at) = effects.off_page_pages.binary_search(&page) {
                        effects.off_page_pages.insert(at, page);
                    }
                }
                EntryEffect::MissingPage => {
                    effects.missing_page += 1;
                    let page = redaction.region.page;
                    if let Err(at) = effects.missing_pages.binary_search(&page) {
                        effects.missing_pages.insert(at, page);
                    }
                }
            }
        }
        effects.per_entry = per_entry;
        effects.removed_glyphs = removed_glyphs;
        effects
    }

    /// Geplante Schwärzungen.
    pub fn requested(&self) -> usize {
        self.per_entry.len()
    }

    /// Regionen, bei denen nachweislich gar nichts geschehen ist.
    ///
    /// [`Effects::off_page`] gehört dazu und nicht zu [`Effects::covered`]:
    /// neben dem Blatt wird kein Deck-Rechteck sichtbar und kein Zeichen
    /// getroffen. Das ist dieselbe Einordnung, die die Oberfläche mit
    /// `HitOutcome::OffPage` trifft — dort fällt der Fall aus „n werden
    /// geschwärzt“ heraus.
    pub fn ineffective(&self) -> usize {
        self.degenerate + self.missing_page + self.off_page
    }

    /// Warnungen, die sich aus dem *Ergebnis* eines Laufs ergeben.
    ///
    /// Fünf Fälle, die stillschweigend als Erfolg durchgingen:
    ///
    /// 1. Eine Region auf einer Seite, die es nicht gibt. Sie wird nie
    ///    angefasst; die Zusammenfassung meldete trotzdem „Schwärzungen: 1“.
    /// 2. Eine Region, die nach `--padding` leer ist. Sie wird übersprungen.
    /// 3. Eine Region, die vollständig **neben** ihrem Blatt liegt. Sie kann
    ///    kein Zeichen treffen, und was außerhalb der MediaBox gezeichnet
    ///    wird, sieht niemand. Bis zu dieser Runde lief der Fall unter Nummer
    ///    4 mit — mit deren Wortlaut („Deck-Rechteck gezeichnet“), der hier
    ///    schlicht nicht stimmt. Siehe [`EntryEffect::OffPage`].
    /// 4. Eine Region, die kein Zeichen entfernt hat. Das kann richtig sein
    ///    (Grafik, Rasterbild), ist es aber nicht zwingend — und der
    ///    Unterschied entscheidet, ob das Geheimnis noch dasteht.
    /// 5. Überschriebene Bilder. Sie werden neu kodiert — pixelgenau
    ///    verlustfrei, aber nicht mehr in der ursprünglichen Kodierung. Wer
    ///    eine deutlich größere Ausgabedatei vorfindet, soll wissen, woher sie
    ///    kommt.
    pub fn warnings(&self, report: &RedactionReport) -> Vec<String> {
        let mut warnings = Vec::new();
        let total = self.requested();

        if self.missing_page > 0 {
            // Das `+ 1` ist die einzige erlaubte Umrechnung: Fließtext sagt
            // „Seite 1“, JSON zählt ab 0 (siehe [`AuditEntry::page`]).
            let list = self
                .missing_pages
                .iter()
                .map(|p| (p + 1).to_string())
                .collect::<Vec<_>>()
                .join(", ");
            warnings.push(format!(
                "{} von {total} Schwärzung(en) liegen auf einer Seite, die es in diesem \
                 Dokument nicht gibt (Seite {list}; das Dokument hat {} Seite(n)). Dort \
                 wurde nichts entfernt und nichts überdeckt — der Text steht unverändert \
                 in der Ausgabe. Häufigste Ursache ist die Zählweise: in JSON ist die \
                 erste Seite „page“: 0, die letzte also {}.",
                self.missing_page,
                self.pages,
                self.pages.saturating_sub(1)
            ));
        }
        if self.degenerate > 0 {
            warnings.push(format!(
                "{} von {total} Schwärzungen haben nach --padding={} ein leeres \
                 Rechteck und konnten nichts entfernen. Der Text steht unverändert in der \
                 Ausgabe. Ein negatives Padding verkleinert jeden Bereich.",
                self.degenerate, self.padding
            ));
        }
        // Vor dem Nachbarn darunter und mit eigenem Wortlaut: „Deck-Rechteck
        // gezeichnet, aber kein Zeichen entfernt“ wäre hier eine richtige Zahl
        // mit der falschen Erklärung — neben dem Blatt ist überhaupt nichts
        // gezeichnet worden, was jemand sähe. Dieselben Worte wie in der
        // Oberfläche („neben der Seite“), damit beide Wege wiedererkennbar
        // dasselbe sagen.
        if self.off_page > 0 {
            let list = self
                .off_page_pages
                .iter()
                .map(|p| (p + 1).to_string())
                .collect::<Vec<_>>()
                .join(", ");
            warnings.push(format!(
                "{} von {total} Schwärzung(en) liegen vollständig neben der Seite, auf \
                 der sie stehen sollen (Seite {list}). Dort kann kein Zeichen liegen und \
                 kein Deck-Rechteck sichtbar werden — der Text der Seite steht \
                 unverändert in der Ausgabe. Häufigste Ursache sind Koordinaten aus \
                 einer Review- oder Regionsdatei, die zu einem anders großen Blatt \
                 gehören.",
                self.off_page
            ));
        }
        if self.covered > 0 {
            let mut text = format!(
                "{} von {total} Schwärzung(en) haben ein Deck-Rechteck gezeichnet, aber \
                 kein einziges Zeichen aus dem Content-Stream entfernt. Wo gar kein Text \
                 steht (Grafik, Rasterbild), ist das richtig; treffen die Koordinaten \
                 dagegen daneben, bleibt der Text darunter lesbar und per Copy-&-Paste \
                 zu holen.",
                self.covered
            );
            if report.redacted_images > 0 {
                text.push_str(&format!(
                    " In diesem Lauf wurden {} Bild(er) in ihren Bildpunkten \
                     überschrieben — über einem Rasterbild ist dieser Befund der \
                     Normalfall.",
                    report.redacted_images
                ));
            }
            warnings.push(text);
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
}

/// Was der Lauf tatsächlich bewirkt hat.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EffectRecord {
    /// Rand, der auf jedes Rechteck gerechnet wurde (`--padding`).
    pub padding: f64,
    /// Seitenzahl des verarbeiteten Dokuments.
    ///
    /// Steht hier, weil `missing_page` sonst nicht nachprüfbar wäre: erst
    /// zusammen mit dieser Zahl lässt sich sehen, dass eine genannte Seite
    /// wirklich außerhalb liegt.
    pub pages: usize,
    /// Geplante Schwärzungen.
    pub requested: usize,
    /// Davon mit nachgewiesener Wirkung: mindestens ein Zeichen entfernt.
    pub applied: usize,
    /// Davon nur überdeckt: gültiges Rechteck, aber kein Zeichen getroffen.
    pub covered: usize,
    /// Davon entartet (leeres Rechteck) — wirkungslos.
    pub degenerate: usize,
    /// Davon auf einer Seite, die es nicht gibt — wirkungslos.
    pub missing_page: usize,
    /// Welche Seiten das waren (0-basiert).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_pages: Vec<usize>,
    /// Davon vollständig neben ihrem Blatt — wirkungslos.
    ///
    /// Siehe [`EntryEffect::OffPage`]. `#[serde(default)]`, damit ein Log aus
    /// einer früheren Fassung weiter lesbar bleibt.
    #[serde(default)]
    pub off_page: usize,
    /// Welche Seiten das waren (0-basiert).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub off_page_pages: Vec<usize>,
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

/// Die Ergebnisse eines Laufs samt der Ansage, wonach überhaupt gesucht wurde
/// — so wie beides ins Log gehört.
#[derive(Debug, Clone, Copy)]
pub struct Applied<'a> {
    /// Der Befund je Region — bereits gemessen, hier nicht noch einmal
    /// bewertet. Trägt auch `padding` und die Seitenzahl.
    pub effects: &'a Effects,
    /// Bericht der Schwärzung.
    pub redaction: &'a RedactionReport,
    /// Bericht des Metadatenlaufs.
    pub metadata: &'a MetadataReport,
    /// Was an automatischer Erkennung abgeschaltet war.
    ///
    /// Das einzige Feld hier, das keine Messung ist — es steht trotzdem in
    /// diesem Bündel, weil es zur selben Frage gehört wie die anderen drei:
    /// *was ist wirklich geschehen?* Ein eigener Parameter an
    /// [`AuditLog::build`] wäre der achte gewesen.
    pub patterns: &'a PatternRecord,
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
        let effects = applied.effects;
        let entries: Vec<AuditEntry> = redactions
            .iter()
            .enumerate()
            .map(|(index, r)| AuditEntry {
                page: r.region.page,
                rect: r.region.rect,
                effective_rect: r.region.rect.expanded(effects.padding),
                // Übernommen, nicht neu bewertet: beides kommt aus [`Effects`],
                // und das ist die einzige Stelle, die misst.
                effect: effects
                    .per_entry
                    .get(index)
                    .copied()
                    .unwrap_or(EntryEffect::Covered),
                removed_glyphs: effects.removed_glyphs.get(index).copied().unwrap_or(0),
                action: r.action.clone(),
                reason: r.reason.clone(),
                source: r.region.source_kind().to_string(),
            })
            .collect();

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
                padding: effects.padding,
                pages: effects.pages,
                requested: entries.len(),
                applied: effects.applied,
                covered: effects.covered,
                degenerate: effects.degenerate,
                missing_page: effects.missing_page,
                missing_pages: effects.missing_pages.clone(),
                off_page: effects.off_page,
                off_page_pages: effects.off_page_pages.clone(),
                removed_glyphs: applied.redaction.removed_glyphs,
                drawn_rects: applied.redaction.drawn_rects,
                removed_annotations: applied.redaction.removed_annotations,
                redacted_images: applied.redaction.redacted_images,
                copied_images: applied.redaction.copied_images,
            },
            patterns: applied.patterns.clone(),
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

    /// `n` A4-Blätter — die Blattgrößen, gegen die die Bewertung rechnet.
    ///
    /// Die Rechtecke der Testregionen liegen alle innerhalb davon; wer eines
    /// danebenlegen will, nimmt Koordinaten jenseits von A4.
    fn sheets(n: usize) -> Vec<Rect> {
        vec![Rect::new(0.0, 0.0, 595.0, 842.0); n]
    }

    fn stripped_info() -> MetadataReport {
        MetadataReport {
            info_removed: true,
            ..Default::default()
        }
    }

    /// Ein Bericht, in dem die Wirkung je Region wirklich drinsteht.
    ///
    /// `RedactionReport::default()` von Hand aufzufüllen war der bequeme Weg —
    /// und genau der, auf dem die Wahrheit verlorenging: `per_redaction` blieb
    /// leer, und niemandem fiel auf, dass es niemand las.
    fn report_with(per_redaction: &[usize]) -> RedactionReport {
        RedactionReport {
            removed_glyphs: per_redaction.iter().sum(),
            drawn_rects: per_redaction.len(),
            per_redaction: per_redaction.to_vec(),
            ..Default::default()
        }
    }

    fn build(
        redactions: &[Redaction],
        padding: f64,
        pages: usize,
        report: &RedactionReport,
        metadata: &MetadataReport,
    ) -> (AuditLog, std::path::PathBuf) {
        build_with(
            redactions,
            padding,
            pages,
            report,
            metadata,
            &crate::Config::default(),
        )
    }

    fn build_with(
        redactions: &[Redaction],
        padding: f64,
        pages: usize,
        report: &RedactionReport,
        metadata: &MetadataReport,
        config: &crate::Config,
    ) -> (AuditLog, std::path::PathBuf) {
        let dir = tempdir();
        let input = dir.join("in.pdf");
        let output = dir.join("out.pdf");
        std::fs::write(&input, b"a").unwrap();
        std::fs::write(&output, b"b").unwrap();
        let effects = Effects::measure(redactions, padding, &sheets(pages), report);
        let warnings = effects.warnings(report);
        let log = AuditLog::build(
            &input,
            &sha256_bytes(b"a"),
            &output,
            redactions,
            &[],
            Applied {
                effects: &effects,
                redaction: report,
                metadata,
                patterns: &PatternRecord::of(config),
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
        let (log, dir) = build(
            &[iban_redaction()],
            1.0,
            1,
            &report_with(&[4]),
            &stripped_info(),
        );
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
            1,
            &report_with(&[4]),
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
            1,
            &report_with(&[4]),
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
            1,
            &RedactionReport {
                per_redaction: vec![0],
                ..Default::default()
            },
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

    // ----------------------------------------------------------- Aufgabe #64

    /// Der Kern des Befunds: die Seite kam in der Bewertung gar nicht vor.
    ///
    /// Eine Region auf Seite 3 eines dreiseitigen Dokuments (0, 1, 2) wird von
    /// der Schwärzung nie angefasst. Das Log bescheinigte trotzdem `applied`.
    #[test]
    fn a_region_on_a_page_that_does_not_exist_is_not_applied() {
        let mut region = iban_redaction();
        region.region.page = 3;
        let (log, dir) = build(
            &[region],
            1.0,
            3,
            &RedactionReport {
                per_redaction: vec![0],
                ..Default::default()
            },
            &stripped_info(),
        );
        assert_eq!(log.effect.requested, 1);
        assert_eq!(log.effect.applied, 0);
        assert_eq!(log.effect.missing_page, 1);
        assert_eq!(log.effect.missing_pages, vec![3]);
        assert_eq!(log.effect.pages, 3);
        assert_eq!(log.redactions[0].effect, EntryEffect::MissingPage);
        assert_eq!(log.redactions[0].removed_glyphs, 0);

        // Und der Lauf sagt es — mit der Seitenzahl im Fließtext, also ab 1.
        let warning = log
            .warnings
            .iter()
            .find(|w| w.contains("nicht gibt"))
            .unwrap_or_else(|| panic!("keine Warnung: {:?}", log.warnings));
        assert!(warning.contains("Seite 4"), "{warning}");
        assert!(warning.contains("3 Seite(n)"), "{warning}");
        std::fs::remove_dir_all(dir).ok();
    }

    /// Gültiges Rechteck, vorhandene Seite, kein getroffenes Zeichen: das ist
    /// **nicht** `applied` und **nicht** dasselbe wie eine fehlende Seite.
    #[test]
    fn a_region_that_removed_nothing_is_covered_not_applied() {
        let (log, dir) = build(
            &[iban_redaction()],
            1.0,
            1,
            &RedactionReport {
                drawn_rects: 1,
                per_redaction: vec![0],
                ..Default::default()
            },
            &stripped_info(),
        );
        assert_eq!(log.effect.applied, 0);
        assert_eq!(log.effect.covered, 1);
        assert_eq!(log.effect.missing_page, 0);
        assert_eq!(log.effect.degenerate, 0);
        assert_eq!(log.redactions[0].effect, EntryEffect::Covered);
        assert!(
            log.warnings
                .iter()
                .any(|w| w.contains("kein einziges Zeichen")),
            "{:?}",
            log.warnings
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// Die fünf Befunde sind fünf verschiedene, und die Summe stimmt.
    #[test]
    fn every_region_is_judged_on_its_own_measurement() {
        let applied = iban_redaction();
        let covered = iban_redaction();
        let mut missing = iban_redaction();
        missing.region.page = 7;
        // Weit rechts neben dem A4-Blatt aus `sheets`.
        let mut beside = iban_redaction();
        beside.region.rect = Rect::new(900.0, 2.0, 950.0, 4.0);
        let redactions = [applied, covered, missing, beside];

        let effects = Effects::measure(
            &redactions,
            1.0,
            &sheets(2),
            &RedactionReport {
                removed_glyphs: 22,
                drawn_rects: 2,
                per_redaction: vec![22, 0, 0, 0],
                ..Default::default()
            },
        );
        assert_eq!(
            effects.per_entry,
            vec![
                EntryEffect::Applied,
                EntryEffect::Covered,
                EntryEffect::MissingPage,
                EntryEffect::OffPage,
            ]
        );
        assert_eq!(effects.applied, 1);
        assert_eq!(effects.covered, 1);
        assert_eq!(effects.missing_page, 1);
        assert_eq!(effects.off_page, 1);
        assert_eq!(effects.ineffective(), 2);
        assert_eq!(
            effects.applied
                + effects.covered
                + effects.degenerate
                + effects.missing_page
                + effects.off_page,
            effects.requested()
        );
        assert_eq!(effects.removed_glyphs, vec![22, 0, 0, 0]);
    }

    /// **Ein Rechteck neben dem Blatt ist nicht „überdeckt“.**
    ///
    /// Der Befund, den nur die Oberfläche kannte. Bis zu dieser Runde lief er
    /// hier als [`EntryEffect::Covered`] mit — mit der Erklärung „Deck-Rechteck
    /// gezeichnet, aber kein Zeichen entfernt“, die über einer Grafik der
    /// Normalfall ist und hier schlicht nicht stimmt.
    ///
    /// Beides steht als Zusicherung da: der Befund **und** dass es nicht der
    /// alte ist. Nimmt man die `OffPage`-Prüfung aus [`EntryEffect::of`]
    /// heraus, wird dieser Test rot.
    #[test]
    fn a_rect_beside_the_sheet_is_off_page_and_not_covered() {
        let mut beside = iban_redaction();
        beside.region.rect = Rect::new(900.0, 100.0, 950.0, 120.0);
        let effects = Effects::measure(&[beside], 1.0, &sheets(1), &report_with(&[0]));

        assert_eq!(effects.per_entry, vec![EntryEffect::OffPage]);
        assert_eq!(effects.off_page, 1);
        assert_eq!(effects.covered, 0, "der Fall läuft wieder als „überdeckt“");
        assert_eq!(effects.applied, 0);
        assert_eq!(effects.off_page_pages, vec![0]);
        assert_eq!(effects.ineffective(), 1);

        // Die Warnung nennt den Fall und die Seite — im Fließtext ab 1.
        let warnings = effects.warnings(&report_with(&[0]));
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("neben der Seite, auf der sie stehen sollen"),
            "{warnings:?}"
        );
        assert!(warnings[0].contains("(Seite 1)"), "{warnings:?}");
        assert!(
            !warnings[0].contains("Deck-Rechteck gezeichnet, aber"),
            "der falsche Satz ist zurück: {warnings:?}"
        );
        // Und die Einordnung: eine Angabe des Nutzers, keine Deckungslücke.
        assert!(!crate::is_coverage_gap(&warnings[0]), "{warnings:?}");
    }

    /// Ein Rechteck, das das Blatt **berührt**, ist keins daneben.
    ///
    /// Die Gegenprobe zum Test darüber: ohne sie wäre er auch dann grün, wenn
    /// jedes Rechteck als „neben der Seite“ gälte. Geprüft wird die Kante — ein
    /// Punkt Überlappung genügt, genau wie in
    /// `redact_gui::AppState::clamp_to_page`.
    #[test]
    fn a_rect_that_still_touches_the_sheet_is_not_off_page() {
        // Das Blatt endet bei x = 595; dieses Rechteck ragt hinein.
        let mut kante = iban_redaction();
        kante.region.rect = Rect::new(594.0, 100.0, 650.0, 120.0);
        let effects = Effects::measure(&[kante], 1.0, &sheets(1), &report_with(&[3]));
        assert_eq!(effects.per_entry, vec![EntryEffect::Applied]);
        assert_eq!(effects.off_page, 0);

        // Und genau einen Punkt weiter draußen ist es doch daneben.
        let mut daneben = iban_redaction();
        daneben.region.rect = Rect::new(595.0, 100.0, 650.0, 120.0);
        let effects = Effects::measure(&[daneben], 1.0, &sheets(1), &report_with(&[0]));
        assert_eq!(effects.per_entry, vec![EntryEffect::OffPage]);
    }

    /// Die Reihenfolge der Prüfungen: eine Seite, die es nicht gibt, schlägt
    /// „neben dem Blatt“ — dort gibt es gar kein Blatt, neben dem etwas läge.
    #[test]
    fn a_missing_page_wins_over_a_rect_beside_the_sheet() {
        let mut region = iban_redaction();
        region.region.page = 9;
        region.region.rect = Rect::new(900.0, 100.0, 950.0, 120.0);
        let effects = Effects::measure(&[region], 1.0, &sheets(1), &report_with(&[0]));
        assert_eq!(effects.per_entry, vec![EntryEffect::MissingPage]);
        assert_eq!(effects.off_page, 0);
    }

    /// Und `--padding`, das ein Rechteck leert, schlägt beides: dort wird
    /// überhaupt nichts angefasst.
    #[test]
    fn a_degenerate_rect_wins_over_a_rect_beside_the_sheet() {
        let mut region = iban_redaction();
        region.region.rect = Rect::new(900.0, 100.0, 950.0, 120.0);
        let effects = Effects::measure(&[region], -100.0, &sheets(1), &report_with(&[0]));
        assert_eq!(effects.per_entry, vec![EntryEffect::Degenerate]);
        assert_eq!(effects.off_page, 0);
    }

    /// Eine Seite, die es nicht gibt, schlägt jede weitere Bewertung: dort ist
    /// kein Rechteck zu klein, dort geschieht überhaupt nichts.
    #[test]
    fn a_missing_page_wins_over_a_degenerate_rect() {
        let mut region = iban_redaction();
        region.region.page = 9;
        let effects = Effects::measure(&[region], -100.0, &sheets(1), &RedactionReport::default());
        assert_eq!(effects.per_entry, vec![EntryEffect::MissingPage]);
        assert_eq!(effects.degenerate, 0);
    }

    #[test]
    fn an_effective_run_produces_no_warning() {
        let effects = Effects::measure(&[iban_redaction()], 1.0, &sheets(1), &report_with(&[26]));
        assert!(effects.warnings(&report_with(&[26])).is_empty());
    }

    /// Ein überschriebenes Bild ist kein Randdetail: das Bild wird neu
    /// kodiert und ist danach nicht mehr das Original.
    #[test]
    fn overwritten_images_are_reported_and_logged() {
        let report = RedactionReport {
            drawn_rects: 1,
            redacted_images: 2,
            copied_images: 1,
            per_redaction: vec![0],
            ..Default::default()
        };
        let effects = Effects::measure(&[iban_redaction()], 1.0, &sheets(1), &report);
        let warnings = effects.warnings(&report);
        // Zwei Befunde: die Region hat kein Zeichen entfernt (über einem Bild
        // der Normalfall — und genau das steht auch dabei), und das Bild wurde
        // neu kodiert.
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings[0].contains("Normalfall"), "{warnings:?}");
        assert!(
            warnings[1].contains("2 Bild(er) überschrieben"),
            "{warnings:?}"
        );
        assert!(warnings[1].contains("verlustfrei"), "{warnings:?}");
        // Und keine Behauptung, die nicht stimmt: außerhalb der Schwärzung
        // bleibt jeder Bildpunkt gleich.
        assert!(!warnings[1].contains("verlustbehaftet"), "{warnings:?}");
        assert!(warnings[1].contains("1 Kopie(n)"), "{warnings:?}");

        let (log, dir) = build(&[iban_redaction()], 1.0, 1, &report, &stripped_info());
        assert_eq!(log.effect.redacted_images, 2);
        assert_eq!(log.effect.copied_images, 1);
        std::fs::remove_dir_all(dir).ok();
    }

    // ------------------------------------- Abschaltbare automatische Funde

    /// Das Log sagt, wonach der Lauf **nicht** gesucht hat — und es sagt es
    /// auch dann, wenn nichts abgeschaltet war.
    #[test]
    fn the_log_records_what_was_switched_off() {
        // Fall 1: alles an. Das Feld steht trotzdem da und ist leer.
        let (log, dir) = build(
            &[iban_redaction()],
            1.0,
            1,
            &report_with(&[4]),
            &stripped_info(),
        );
        assert!(!log.patterns.anything_disabled());
        let json = serde_json::to_string(&log).unwrap();
        assert!(
            json.contains("\"patterns\":{\"all_disabled\":false,\"disabled\":[]}"),
            "das Feld fehlt im Log: {json}"
        );
        std::fs::remove_dir_all(dir).ok();

        // Fall 2: einzeln abgeschaltet.
        let config = crate::Config {
            disabled_patterns: vec!["date_de".into(), " bic ".into(), String::new()],
            ..crate::Config::default()
        };
        let (log, dir) = build_with(
            &[iban_redaction()],
            1.0,
            1,
            &report_with(&[4]),
            &stripped_info(),
            &config,
        );
        assert!(!log.patterns.all_disabled);
        assert_eq!(log.patterns.disabled, vec!["date_de", "bic"]);
        std::fs::remove_dir_all(dir).ok();

        // Fall 3: ganz aus.
        let config = crate::Config {
            no_patterns: true,
            ..crate::Config::default()
        };
        let (log, dir) = build_with(
            &[iban_redaction()],
            1.0,
            1,
            &report_with(&[4]),
            &stripped_info(),
            &config,
        );
        assert!(log.patterns.all_disabled);
        assert!(log.patterns.anything_disabled());
        std::fs::remove_dir_all(dir).ok();
    }

    /// Ein Log aus einer Fassung ohne dieses Feld bleibt lesbar — sonst wäre
    /// jede ältere Datei mit einem Schlag unbrauchbar.
    #[test]
    fn an_older_log_without_the_field_still_parses() {
        let (log, dir) = build(
            &[iban_redaction()],
            1.0,
            1,
            &report_with(&[4]),
            &stripped_info(),
        );
        let mut value: serde_json::Value = serde_json::to_value(&log).unwrap();
        value.as_object_mut().unwrap().remove("patterns");
        let old: AuditLog = serde_json::from_value(value).expect("altes Log bleibt lesbar");
        assert_eq!(old.patterns, PatternRecord::default());
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
