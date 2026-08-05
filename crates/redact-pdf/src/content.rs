//! Interpreter für PDF-Content-Streams.
//!
//! Der Scanner läuft den Content-Stream einer Seite durch, führt den
//! Grafik- und Textzustand nach (CTM, `Tm`, `Tf`, `Tc`, `Tw`, `Tz`, `Ts`, `TL`)
//! und berechnet für **jedes einzelne Zeichen** eine Bounding-Box im
//! User-Space.
//!
//! Das Ergebnis (`ShowRecord`) wird von zwei Seiten genutzt:
//!
//! * `extract.rs` baut daraus Textzeilen mit zeichengenauen Koordinaten,
//! * `redact.rs` schreibt daraus den Content-Stream neu und entfernt gezielt
//!   einzelne Glyphen.
//!
//! Form-XObjects (`Do`) werden rekursiv mitverarbeitet — dort steht in vielen
//! generierten PDFs der eigentliche Text.
//!
//! ## Senken (`ContentSink`)
//!
//! Der Interpreter kennt zwei Abnehmer: [`ScanResult`] (nur Text) und den
//! Zeichenoperationen-Sammler aus [`crate::ops`] (Text **und** Grafik). Beide
//! bekommen ihre Daten über dieselbe Durchlaufschleife — insbesondere die
//! Glyphenmathematik (`Tm`, `Trm`, Vorschub) steht nur einmal im Code, in
//! [`show_text`]. Damit können Renderer und Schwärzung nicht auseinanderlaufen.
//!
//! Grafikzustand (Farben, Linien, Clip) wird nur nachgeführt, wenn die Senke
//! über [`ContentSink::wants_graphics`] danach fragt; für die reine
//! Textextraktion kostet der Ausbau also nichts.

use std::collections::{BTreeMap, HashSet};

use lopdf::content::Operation;
use lopdf::{Dictionary, Document, Object, ObjectId};
use redact_core::{Point, Rect, RedactError, Result};

use crate::font::{as_f64, fonts_from_resources, FontInfo};
use crate::matrix::Matrix;
use crate::ops::{PathSeg, Rgb, Stroke};

/// Maximale Rekursionstiefe für verschachtelte Form-XObjects.
///
/// Begrenzt die **Tiefe**, nicht die **Breite** — und die Breite ist die
/// gefährlichere Größe. Sieben Ebenen, in denen jedes Form-XObject dasselbe
/// Unterobjekt *n*-mal zeichnet, ergeben n⁷ Durchläufe: bei n = 8 sind das
/// über zwei Millionen, aus einer Datei von 2 368 Byte, die jede einzelne
/// hier dokumentierte Grenze einhält. Dagegen hilft keine Tiefengrenze,
/// sondern nur das Aufwandskonto [`Budget`].
const MAX_FORM_DEPTH: usize = 8;

/// Grundausstattung des Aufwandskontos: so viele Zeichenoperationen darf ein
/// Seiten-Scan immer auswerten, unabhängig davon, wie groß das Dokument ist.
///
/// Reichlich bemessen, weil hier echte Gestaltung hineinfällt: ein
/// Tabellenraster, das dieselbe Zelle hundertmal zeichnet, ein Formular mit
/// vielen platzierten Bausteinen. Die dichteste gemessene Seite eines
/// 500-seitigen Kontoauszugs braucht rund 200 Operationen — Faktor 5 000.
const BASE_OPERATIONS: usize = 1_000_000;

/// Wie oft der Interpreter denselben Strom im Mittel durchlaufen darf.
///
/// Der springende Punkt bei einer Fächerung ist nicht die Menge an Inhalt,
/// sondern die **Vervielfachung**: aus 400 Byte Zeichenanweisungen werden
/// zwei Millionen Durchläufe. Deshalb wächst das Konto mit dem Inhalt, den
/// die Datei tatsächlich mitbringt — jeder Strom, der zum ersten Mal
/// dekodiert wird, bringt seine Operationen mal diesem Faktor als Guthaben
/// ein.
///
/// Damit steht diese Grenze nicht quer zu `--max-parsed-mb`: wer das Budget
/// für geparste Streams anhebt, hebt das Aufwandskonto automatisch mit an,
/// weil mehr Inhalt auch mehr Guthaben bedeutet. Eine Fächerung profitiert
/// davon nicht — sie bringt ja gerade keinen zusätzlichen Inhalt mit.
const MAX_AMPLIFICATION: usize = 16;

/// Wie viele Glyphen ein einzelner Seiten-Scan liefern darf.
///
/// Anders als das Operationskonto eine **feste Decke**, und zwar mit Absicht:
/// dies ist die eigentliche Speichergröße der Textextraktion. Jede Glyphe wird
/// als [`GlyphItem`] gehalten (Originalbytes, Text, Kasten, Grundlinie) und
/// von [`crate::extract`] noch einmal als `Glyph` kopiert. Gemessen (Release,
/// ein Seiteninhalt knapp unter dem Parse-Budget): 14,6 Mio. Glyphen auf
/// **einer** Seite → 9 306 MB Spitzenspeicher, also rund 640 Byte je Glyphe.
/// Ein Budget, das mit der erlaubten Dateigröße mitwüchse, wüchse hier also
/// in den zweistelligen Gigabytebereich — genau das soll nicht passieren.
///
/// Eine Million Zeichen auf einer Seite ist keine Seite mehr. Eine dichte
/// A4-Textseite trägt 3 000–6 000 Zeichen; die dichteste gemessene Seite eines
/// 500-seitigen Kontoauszugs 6 000. Faktor 160 Luft.
const MAX_GLYPHS_PER_SCAN: usize = 1_000_000;

/// Aufwandskonto eines Seiten-Scans.
///
/// Die dokumentierten Grenzen für Eingabedateien messen **Bytes**. Was der
/// Speicher wirklich kostet, sind aber *Interpretationen*: eine Datei von
/// 2 368 Byte hält jede Byte-Grenze ein und erzeugt trotzdem zwei Millionen
/// Durchläufe durch dieselben acht Ströme (siehe [`MAX_FORM_DEPTH`]). Deshalb
/// zählt dieses Konto das, was tatsächlich anfällt, und wächst mit dem
/// Inhalt, den die Datei mitbringt — nicht mit dem, was sie daraus macht.
///
/// Ist es leer, wird der Scan **abgebrochen und die Datei abgelehnt** — nicht
/// gewarnt. Eine Seite, deren Text nur zum Teil durchsucht wurde, darf nicht
/// als Erfolg enden: der ungeprüfte Rest ist genau der, in dem das Geheimnis
/// stehen kann.
#[derive(Debug)]
struct Budget {
    /// Verbleibendes Guthaben an Zeichenoperationen.
    operations: usize,
    glyphs: usize,
    /// Ströme, deren Inhalt schon einmal Guthaben eingebracht hat. Ein
    /// zweites Mal zählt er nicht — sonst finanzierte die Fächerung sich
    /// selbst.
    credited: HashSet<ObjectId>,
    /// Type3-Schriften, deren Glyphprozeduren schon untersucht wurden — je
    /// Strom und Ressourcenname.
    ///
    /// Ohne diese Sperre kostete **jedes** `Tj` einen vollen Durchlauf durch
    /// alle `/CharProcs` der Schrift. Genau daraus bestünde die nächste
    /// Vervielfachung: eine Schrift mit tausend Glyphprozeduren, tausendmal
    /// gesetzt. Was danach doch untersucht wird, zahlt regulär vom Konto.
    looked_at_type3: HashSet<(StreamKey, Vec<u8>)>,
    /// Begründung, sobald etwas aufgebraucht ist.
    exceeded: Option<String>,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            operations: BASE_OPERATIONS,
            glyphs: MAX_GLYPHS_PER_SCAN,
            credited: HashSet::new(),
            looked_at_type3: HashSet::new(),
            exceeded: None,
        }
    }
}

impl Budget {
    /// Schreibt den Inhalt eines Stroms gut — einmal je Strom.
    ///
    /// `id` ist `None` für den Seitenstrom selbst; der wird ohnehin nur einmal
    /// dekodiert.
    fn credit(&mut self, id: Option<ObjectId>, operations: usize) {
        if let Some(id) = id {
            if !self.credited.insert(id) {
                return;
            }
        }
        self.operations = self
            .operations
            .saturating_add(operations.saturating_mul(MAX_AMPLIFICATION));
    }

    /// `true` beim **ersten** Blick auf diese Type3-Schrift in diesem Strom.
    ///
    /// Siehe [`Budget::looked_at_type3`]: die Untersuchung selbst kostet, sie
    /// darf nur nicht bei jedem einzelnen `Tj` neu anfallen.
    fn first_look_at_type3(&mut self, stream: StreamKey, font: &[u8]) -> bool {
        self.looked_at_type3.insert((stream, font.to_vec()))
    }

    /// Verbucht eine Operation. `false` heißt: sofort aussteigen.
    fn operation(&mut self) -> bool {
        if self.exceeded.is_some() {
            return false;
        }
        match self.operations.checked_sub(1) {
            Some(rest) => {
                self.operations = rest;
                true
            }
            None => {
                self.exceeded = Some(format!(
                    "Der Seiteninhalt wird um mehr als das {MAX_AMPLIFICATION}-fache \
                     vervielfacht: es sind mehr Zeichenoperationen auszuwerten, als der \
                     Inhalt der Datei hergibt. So etwas entsteht nicht durch einen langen \
                     Text, sondern dadurch, dass wenige Form-XObjects einander vielfach \
                     zeichnen — aus wenigen Kilobyte werden Millionen Durchläufe. Die \
                     Datei wird abgelehnt."
                ));
                false
            }
        }
    }

    /// Verbucht eine Glyphe. `false` heißt: sofort aussteigen.
    fn glyph(&mut self) -> bool {
        if self.exceeded.is_some() {
            return false;
        }
        match self.glyphs.checked_sub(1) {
            Some(rest) => {
                self.glyphs = rest;
                true
            }
            None => {
                self.exceeded = Some(format!(
                    "Eine Seite dieses Dokuments setzt mehr als {MAX_GLYPHS_PER_SCAN} \
                     Zeichen. Eine dichte Textseite trägt einige tausend; diese Menge \
                     entsteht nur, wenn derselbe Text vielfach gezeichnet wird. Beim \
                     Vermessen der Zeichen würde daraus ein zweistelliges Gigabyte \
                     Arbeitsspeicher. Die Datei wird abgelehnt."
                ));
                false
            }
        }
    }

    /// Der Befund, falls das Konto gerissen wurde.
    fn result(&self) -> Result<()> {
        match &self.exceeded {
            Some(message) => Err(RedactError::Pdf(message.clone())),
            None => Ok(()),
        }
    }
}

/// Ab wie vielen **unlesbaren** Zeichen ein Font ohne `/ToUnicode` gemeldet
/// wird — unabhängig davon, wie klein ihr Anteil ist.
///
/// Vorher zählte diese Schwelle die *insgesamt* gesetzten Zeichen: unter vier
/// blieb es still. Eine Seite mit genau drei unlesbaren Glyphen warnte deshalb
/// nicht — ausgerechnet der Fall, der wehtut. Drei unlesbare Zeichen können
/// genau die Ziffern sein, auf die es ankommt; der Rest einer Kontonummer ist
/// nicht weniger schutzbedürftig, weil er kurz ist. Gezählt wird jetzt das
/// Unlesbare selbst.
///
/// Bei **einem** einzelnen Zeichen bleibt es still, sofern der Font sonst
/// lesbar ist (siehe [`UNREADABLE_RATIO`]): ein Aufzählungspunkt oder ein
/// Logo-Dingbat ist Gestaltung, kein Text. Dieser Fall ist häufig genug, dass
/// eine Warnung darüber die echten Befunde zudecken würde.
const UNREADABLE_MIN_GLYPHS: usize = 2;

/// Ab welchem Anteil schon ein **einzelnes** unlesbares Zeichen gemeldet wird.
///
/// Vorher war der Anteil das alleinige Maß, mit 0,3. Das ist die falsche
/// Größe: Der Anteil misst, wie typisch das Problem im Font ist, nicht wie
/// viel Text dadurch ungeprüft bleibt — 29 % eines Fonts mit 1000 Zeichen sind
/// 290 unlesbare Zeichen, und die blieben unerwähnt. Über die Menge
/// entscheidet jetzt [`UNREADABLE_MIN_GLYPHS`].
///
/// Der Anteil hat nur noch eine Aufgabe: Ein Font, der überhaupt kaum Text
/// setzt (bis zu 20 Zeichen), fällt schon mit einem einzigen unlesbaren
/// Zeichen auf — dort trägt dieses eine Zeichen Gewicht, während dieselbe
/// Glyphe in einem seitenfüllenden Font Beiwerk ist.
const UNREADABLE_RATIO: f64 = 0.05;

/// Aus welchem Stream ein Datensatz stammt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StreamKey {
    /// Der (ggf. zusammengesetzte) Content-Stream der Seite.
    Page,
    /// Ein Form-XObject.
    Form(ObjectId),
}

/// Lage einer Glyphe auf ihrer Grundlinie — bereits im User-Space, also nach
/// `Tm`, `Tz`, `Ts` und CTM.
///
/// Die Extraktion darf keine achsenparallele Leserichtung unterstellen: bei
/// gedrehtem Text läuft die Grundlinie schräg oder senkrecht, und eine
/// Gruppierung nach `origin.y` zerlegt jede Zeile in Einzelzeichen. Ebenso
/// wenig darf sie den Zeichenabstand aus den Kästen ableiten — eine gesetzte
/// Laufweite (`Tc`) steckt bereits im [`GlyphItem::displacement`] und wäre
/// sonst nicht von einer echten Wortlücke zu unterscheiden.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Baseline {
    /// Einheitsvektor der Schreibrichtung im User-Space.
    pub direction: Point,
    /// Vorschub bis zur nächsten Glyphe, entlang [`Baseline::direction`].
    /// Enthält Glyphenbreite, `Tc`, `Tw` und `Tz`.
    pub advance: f64,
    /// Höhe des Glyphenkastens senkrecht zur Grundlinie.
    pub height: f64,
    /// Breite des Leerzeichens dieses Fonts, entlang der Schreibrichtung.
    /// `0.0`, wenn der Font keine brauchbare Auskunft gibt.
    pub space_width: f64,
}

impl Default for Baseline {
    fn default() -> Self {
        Self {
            direction: Point::new(1.0, 0.0),
            advance: 0.0,
            height: 0.0,
            space_width: 0.0,
        }
    }
}

/// Ein einzelnes gesetztes Zeichen.
#[derive(Debug, Clone)]
pub struct GlyphItem {
    /// Originalbytes des Zeichencodes (für das Neuschreiben des Streams).
    pub bytes: Vec<u8>,
    /// Dekodierter Text (kann bei Ligaturen mehrere Zeichen umfassen).
    pub text: String,
    /// Bounding-Box im User-Space.
    pub rect: Rect,
    /// Ursprung (Grundlinie, linke Kante) im User-Space.
    pub origin: Point,
    /// Vorschub im Textraum vor Anwendung von `Tm`/CTM.
    pub displacement: f64,
    /// Grundlinien-Geometrie im User-Space.
    pub baseline: Baseline,
}

/// Bestandteil einer Text-Ausgabe-Operation.
#[derive(Debug, Clone)]
pub enum ShowItem {
    Glyph(GlyphItem),
    /// Zahlenwert aus einem `TJ`-Array (Kerning).
    Adjust(f64),
}

/// Eine Text-Ausgabe-Operation mit allen berechneten Glyphen.
#[derive(Debug, Clone)]
pub struct ShowRecord {
    pub stream: StreamKey,
    /// Index der Operation im dekodierten Stream.
    pub op_index: usize,
    pub operator: String,
    /// Originaloperanden der Operation (für `'` und `"` gebraucht).
    pub operands: Vec<Object>,
    pub font_size: f64,
    pub h_scale: f64,
    pub items: Vec<ShowItem>,
}

impl ShowRecord {
    pub fn glyphs(&self) -> impl Iterator<Item = &GlyphItem> {
        self.items.iter().filter_map(|i| match i {
            ShowItem::Glyph(g) => Some(g),
            ShowItem::Adjust(_) => None,
        })
    }
}

/// Schlüssel einer Eigenschaftsliste, die den Text darunter **spiegeln**.
///
/// * `/ActualText` — der kanonische Ersatz für die Glyphen des Abschnitts
///   (PDF 32000-1, 14.9.4). Word, InDesign und jeder PDF/UA-Erzeuger schreiben
///   ihn routinemäßig, etwa für Ligaturen und Sonderzeichen.
/// * `/Alt` — die Beschreibung für Hilfsmittel (14.9.3); bei `/Figure` steht
///   dort regelmäßig genau der Text, den das Bild zeigt.
/// * `/E` — die ausgeschriebene Form einer Abkürzung (14.9.5).
///
/// Alle drei geben wieder, was die Glyphen sagen; `pdftotext` bevorzugt in der
/// Voreinstellung sogar den Spiegel. Verschwinden die Glyphen, muss er mit.
pub const MIRROR_KEYS: [&[u8]; 3] = [b"ActualText", b"Alt", b"E"];

/// Ein `BDC`/`DP`, dessen Eigenschaftsliste einen Textspiegel trägt.
///
/// `shows` nennt die Textoperationen, die dieser Spiegel wiedergibt — die
/// Marked-Content-Klammer kennt ihre Glyphen. Damit kann die Schwärzung genau
/// die Frage beantworten, auf die es ankommt: *ist von diesem Abschnitt etwas
/// entfernt worden?*
#[derive(Debug, Clone)]
pub struct MarkedTextRecord {
    pub stream: StreamKey,
    /// Index der `BDC`/`DP`-Operation im dekodierten Strom.
    pub op_index: usize,
    /// Die aufgelöste Eigenschaftsliste — gleich, ob sie inline im Strom stand
    /// oder über `/Resources /Properties` erreichbar war.
    pub properties: Dictionary,
    /// Objekt-Id der Eigenschaftsliste, falls sie ein **eigenes** Objekt ist
    /// (`/Properties /MC0 12 0 R`). Sonst `None`: dann steht die Liste inline
    /// im Strom oder direkt im `/Properties`-Dictionary, und der Spiegel ist
    /// nur über die Operation selbst zu erreichen.
    pub property_id: Option<ObjectId>,
    /// Indizes der Textoperationen im Geltungsbereich (siehe
    /// [`scan_marked_text`]).
    pub shows: Vec<usize>,
}

/// Ergebnis eines Seiten-Scans.
#[derive(Debug, Default)]
pub struct ScanResult {
    pub shows: Vec<ShowRecord>,
    /// Marked-Content-Abschnitte mit Textspiegel.
    pub marked: Vec<MarkedTextRecord>,
    /// Wie oft ein Form-XObject auf dieser Seite gezeichnet wurde.
    pub form_placements: BTreeMap<ObjectId, usize>,
    /// Form-XObjects, die in einem der gelesenen Ressourcenverzeichnisse
    /// **stehen** — samt ihrem Ressourcennamen. Wer hier steht und nicht in
    /// [`ScanResult::form_placements`], wurde nie gezeichnet und deshalb auch
    /// nie gelesen.
    pub declared_forms: BTreeMap<ObjectId, Vec<u8>>,
    /// Befunde, die den Nutzer erreichen müssen — allen voran Fonts, deren
    /// Text sich nicht dekodieren lässt. Aus solchem Text kann die Analyse
    /// nichts erkennen; ohne Warnung hielte man die Datei für sauber.
    pub warnings: Vec<String>,
    /// Welche Spiegel schon in `marked` stehen. Als Menge geführt, nicht durch
    /// Durchsuchen der Liste: eine getaggte Seite bringt leicht Tausende
    /// Abschnitte mit, und ein mehrfach platziertes Formular liefert sie
    /// mehrfach.
    seen_marked: HashSet<(StreamKey, usize)>,
}

impl ContentSink for ScanResult {
    fn show(&mut self, record: ShowRecord) {
        self.shows.push(record);
    }

    fn marked(&mut self, record: MarkedTextRecord) {
        // Ein mehrfach platziertes Form-XObject wird mehrfach durchlaufen; sein
        // Strom wird aber nur **einmal** neu geschrieben. Derselbe Spiegel darf
        // deshalb nicht mehrfach in der Liste stehen.
        if !self.seen_marked.insert((record.stream, record.op_index)) {
            return;
        }
        self.marked.push(record);
    }

    fn form(&mut self, id: ObjectId) {
        *self.form_placements.entry(id).or_insert(0) += 1;
    }

    fn declares_form(&mut self, id: ObjectId, name: &[u8]) {
        self.declared_forms
            .entry(id)
            .or_insert_with(|| name.to_vec());
    }

    fn warn(&mut self, message: String) {
        if !self.warnings.contains(&message) {
            self.warnings.push(message);
        }
    }
}

// ---------------------------------------------------------------------------
// Senke
// ---------------------------------------------------------------------------

/// Umgebung eines Sink-Aufrufs: Dokument, Ressourcen und Herkunft.
pub struct SinkContext<'a> {
    pub doc: &'a Document,
    pub resources: Option<&'a Dictionary>,
    pub stream: StreamKey,
    pub op_index: usize,
}

/// Eine einzelne Glyphe, exakt so positioniert wie in [`GlyphItem`].
pub struct GlyphEvent<'a> {
    /// Ressourcenname aus dem letzten `Tf` (Schlüssel in `/Resources /Font`).
    pub font_name: &'a [u8],
    /// Die **bereits geparsten** Metriken dieses Fonts.
    ///
    /// Der Interpreter lädt zu Beginn jeder Ressourcenebene alle Fonts; wer
    /// sie hier noch einmal aus dem Dictionary läse, parste jedes
    /// `/ToUnicode`, jede `/W`-Liste und jede `cmap` ein zweites Mal.
    pub font: &'a FontInfo,
    pub code: u32,
    pub text: &'a str,
    /// Text-Rendering-Matrix: bildet Text-Space (1 Einheit = Schriftgröße)
    /// auf den User-Space ab. Enthält Schriftgröße, `Tz`, `Ts`, `Tm` und CTM.
    pub trm: Matrix,
    pub fill: Rgb,
    pub fill_alpha: f32,
    /// PDF-Textrendermodus aus `Tr` (3 = unsichtbar).
    pub render_mode: u8,
    pub clip: Option<usize>,
}

/// Ein fertig gemalter Pfad; die Punkte liegen bereits im User-Space.
pub struct PathEvent<'a> {
    pub segments: &'a [PathSeg],
    pub fill: Option<Rgb>,
    pub stroke: Option<Stroke>,
    pub even_odd: bool,
    pub fill_alpha: f32,
    pub clip: Option<usize>,
}

/// Ein platziertes Bild (XObject oder Inline-Bild).
pub struct ImageEvent<'a> {
    /// Name des XObjects; `None` bei einem Inline-Bild.
    pub name: Option<&'a [u8]>,
    /// Inline-Bild: Dictionary und (noch gefilterte) Rohdaten.
    pub inline: Option<(&'a Dictionary, &'a [u8])>,
    /// Bildet das Einheitsquadrat auf die Zielfläche im User-Space ab.
    pub ctm: Matrix,
    /// Aktuelle Füllfarbe — bei `/ImageMask` wird damit gemalt.
    pub fill: Rgb,
    pub fill_alpha: f32,
    pub clip: Option<usize>,
}

/// Abnehmer der Interpreter-Ereignisse.
///
/// Alle Methoden haben eine leere Standardimplementierung, eine Senke nimmt
/// sich also genau das, was sie braucht.
pub trait ContentSink {
    /// Nur wenn `true`, werden Farben, Pfade, Clips und Bilder ausgewertet.
    fn wants_graphics(&self) -> bool {
        false
    }
    /// Eine abgeschlossene Text-Ausgabe-Operation.
    fn show(&mut self, _record: ShowRecord) {}
    /// Ein Marked-Content-Abschnitt mit Textspiegel (`/ActualText`, `/Alt`,
    /// `/E`).
    fn marked(&mut self, _record: MarkedTextRecord) {}
    /// Eine einzelne Glyphe (nur bei `wants_graphics`).
    fn glyph(&mut self, _cx: &SinkContext, _event: &GlyphEvent) {}
    /// Ein gemalter Pfad (nur bei `wants_graphics`).
    fn path(&mut self, _cx: &SinkContext, _event: &PathEvent) {}
    /// Ein platziertes Bild (nur bei `wants_graphics`).
    fn image(&mut self, _cx: &SinkContext, _event: &ImageEvent) {}
    /// Ein neuer Clip-Pfad; der Rückgabewert identifiziert ihn für spätere
    /// Ereignisse.
    fn clip(&mut self, _cx: &SinkContext, _segments: &[PathSeg], _even_odd: bool) -> Option<usize> {
        None
    }
    /// Ein Form-XObject wurde platziert.
    fn form(&mut self, _id: ObjectId) {}
    /// Ein Form-XObject **steht in den Ressourcen** eines gelesenen Stroms.
    ///
    /// Das ist nicht dasselbe wie [`ContentSink::form`]: dort wird gezeichnet,
    /// hier nur angeboten. Die Differenz beider Mengen ist genau der Text, den
    /// der Interpreter nie betritt — siehe
    /// [`crate::extract::PdfExtractor::extract_with_warnings`].
    fn declares_form(&mut self, _id: ObjectId, _name: &[u8]) {}
    /// Ein Befund, der den Nutzer erreichen muss (siehe [`ScanResult::warnings`]).
    fn warn(&mut self, _message: String) {}
}

#[derive(Debug, Clone)]
struct TextState {
    font: Option<FontInfo>,
    /// Ressourcenname des zuletzt gesetzten Fonts.
    font_name: Vec<u8>,
    font_size: f64,
    char_spacing: f64,
    word_spacing: f64,
    h_scale: f64,
    leading: f64,
    rise: f64,
    render_mode: u8,
}

impl Default for TextState {
    fn default() -> Self {
        Self {
            font: None,
            font_name: Vec::new(),
            font_size: 0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            h_scale: 1.0,
            leading: 0.0,
            rise: 0.0,
            render_mode: 0,
        }
    }
}

#[derive(Debug, Clone)]
struct GraphicsState {
    ctm: Matrix,
    text: TextState,
    fill: Rgb,
    stroke: Rgb,
    fill_space: ColorSpace,
    stroke_space: ColorSpace,
    fill_alpha: f32,
    stroke_alpha: f32,
    line_width: f64,
    line_cap: u8,
    line_join: u8,
    dash: Vec<f64>,
    dash_phase: f64,
    clip: Option<usize>,
}

impl GraphicsState {
    fn new(ctm: Matrix) -> Self {
        Self {
            ctm,
            text: TextState::default(),
            fill: Rgb::BLACK,
            stroke: Rgb::BLACK,
            fill_space: ColorSpace::Gray,
            stroke_space: ColorSpace::Gray,
            fill_alpha: 1.0,
            stroke_alpha: 1.0,
            line_width: 1.0,
            line_cap: 0,
            line_join: 0,
            dash: Vec::new(),
            dash_phase: 0.0,
            clip: None,
        }
    }

    /// Strichbeschreibung im User-Space: Breite und Strichelung werden mit der
    /// CTM skaliert, weil der Renderer nur noch User-Space-Werte sieht.
    fn stroke_style(&self) -> Stroke {
        let scale = self.ctm.scale_hint();
        Stroke {
            color: self.stroke,
            width: self.line_width * scale,
            cap: self.line_cap,
            join: self.line_join,
            dash: self.dash.iter().map(|d| d * scale).collect(),
            dash_phase: self.dash_phase * scale,
            alpha: self.stroke_alpha,
        }
    }
}

// ---------------------------------------------------------------------------
// Farbräume
// ---------------------------------------------------------------------------

/// Unterstützte Farbräume.
///
/// Genähert werden bewusst:
///
/// * `ICCBased` → nach Komponentenzahl (`/N` 1/3/4) als Grau/RGB/CMYK,
/// * `CalRGB`/`CalGray` → wie die Device-Varianten,
/// * `Lab` → nur die Helligkeit `L` als Grauwert,
/// * `Separation`/`DeviceN` → Tinte 1.0 bedeutet Schwarz (`1 - max(tint)`),
/// * `Pattern` → mittleres Grau, damit gemusterte Flächen sichtbar bleiben.
///
/// Alles andere wird zu Schwarz.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ColorSpace {
    Gray,
    Rgb,
    Cmyk,
    /// `L` aus dem Lab-Raum (0..100) als Grauwert.
    Lab,
    Indexed {
        base: Box<ColorSpace>,
        lookup: Vec<u8>,
    },
    /// Eine oder mehrere Tinten (`Separation`, `DeviceN`).
    Tint(usize),
    Pattern,
    Unknown,
}

impl ColorSpace {
    /// Anzahl der Farbkomponenten je Bildpunkt.
    pub(crate) fn components(&self) -> usize {
        match self {
            ColorSpace::Gray | ColorSpace::Indexed { .. } => 1,
            ColorSpace::Rgb | ColorSpace::Lab => 3,
            ColorSpace::Cmyk => 4,
            ColorSpace::Tint(n) => *n,
            ColorSpace::Pattern | ColorSpace::Unknown => 1,
        }
    }

    /// Rechnet Komponentenwerte in RGB um. Bei `Indexed` ist `values[0]` der
    /// rohe Index in die Palette, sonst liegen alle Werte in 0.0..=1.0.
    pub(crate) fn to_rgb(&self, values: &[f64]) -> Rgb {
        let get = |i: usize| values.get(i).copied().unwrap_or(0.0);
        match self {
            ColorSpace::Gray => Rgb::gray(get(0)),
            ColorSpace::Rgb => Rgb::new(get(0), get(1), get(2)),
            ColorSpace::Cmyk => cmyk_to_rgb(get(0), get(1), get(2), get(3)),
            ColorSpace::Lab => Rgb::gray((get(0) / 100.0).clamp(0.0, 1.0)),
            ColorSpace::Indexed { base, lookup } => {
                let n = base.components();
                let index = get(0).max(0.0) as usize;
                let start = index * n;
                if start + n > lookup.len() {
                    return Rgb::BLACK;
                }
                let comps: Vec<f64> = lookup[start..start + n]
                    .iter()
                    .map(|b| *b as f64 / 255.0)
                    .collect();
                // Lab-Paletten sind selten; die Helligkeit wird auf 0..100 zurückskaliert.
                if matches!(**base, ColorSpace::Lab) {
                    return base.to_rgb(&[comps[0] * 100.0]);
                }
                base.to_rgb(&comps)
            }
            ColorSpace::Tint(n) => {
                let tint = (0..*n).map(get).fold(0.0f64, f64::max);
                Rgb::gray(1.0 - tint.clamp(0.0, 1.0))
            }
            ColorSpace::Pattern => Rgb::gray(0.5),
            ColorSpace::Unknown => Rgb::BLACK,
        }
    }

    /// Startfarbe eines Farbraums nach `cs`/`CS` (PDF 32000-1, 8.6.3).
    fn initial_color(&self) -> Rgb {
        match self {
            ColorSpace::Pattern => Rgb::gray(0.5),
            ColorSpace::Indexed { .. } => self.to_rgb(&[0.0]),
            _ => Rgb::BLACK,
        }
    }

    /// Löst ein Farbraum-Objekt auf — entweder ein Device-Name, ein Verweis in
    /// `/Resources /ColorSpace` oder ein Array wie `[/Indexed …]`.
    pub(crate) fn resolve(doc: &Document, resources: Option<&Dictionary>, obj: &Object) -> Self {
        Self::resolve_depth(doc, resources, obj, 0)
    }

    fn resolve_depth(
        doc: &Document,
        resources: Option<&Dictionary>,
        obj: &Object,
        depth: usize,
    ) -> Self {
        if depth > 8 {
            return ColorSpace::Unknown;
        }
        let resolved = doc.dereference(obj).map(|(_, o)| o).unwrap_or(obj);
        match resolved {
            Object::Name(name) => match name.as_slice() {
                b"DeviceGray" | b"G" | b"CalGray" => ColorSpace::Gray,
                b"DeviceRGB" | b"RGB" | b"CalRGB" => ColorSpace::Rgb,
                b"DeviceCMYK" | b"CMYK" => ColorSpace::Cmyk,
                b"Pattern" => ColorSpace::Pattern,
                b"Indexed" | b"I" => ColorSpace::Unknown,
                other => {
                    // Benannter Farbraum aus den Ressourcen.
                    let entry = resources
                        .and_then(|r| r.get(b"ColorSpace").ok())
                        .and_then(|o| doc.dereference(o).ok())
                        .and_then(|(_, o)| o.as_dict().ok())
                        .and_then(|d| d.get(other).ok());
                    match entry {
                        Some(entry) => Self::resolve_depth(doc, resources, entry, depth + 1),
                        None => ColorSpace::Unknown,
                    }
                }
            },
            Object::Array(items) => Self::resolve_array(doc, resources, items, depth),
            _ => ColorSpace::Unknown,
        }
    }

    fn resolve_array(
        doc: &Document,
        resources: Option<&Dictionary>,
        items: &[Object],
        depth: usize,
    ) -> Self {
        let Some(family) = items.first().and_then(|o| o.as_name().ok()) else {
            return ColorSpace::Unknown;
        };
        match family {
            b"ICCBased" => {
                let n = items
                    .get(1)
                    .and_then(|o| doc.dereference(o).ok())
                    .and_then(|(_, o)| o.as_stream().ok().map(|s| s.dict.clone()))
                    .and_then(|d| d.get(b"N").ok().and_then(as_f64))
                    .unwrap_or(3.0) as i64;
                match n {
                    1 => ColorSpace::Gray,
                    4 => ColorSpace::Cmyk,
                    _ => ColorSpace::Rgb,
                }
            }
            b"CalGray" => ColorSpace::Gray,
            b"CalRGB" => ColorSpace::Rgb,
            b"Lab" => ColorSpace::Lab,
            b"Indexed" | b"I" => {
                let base = items
                    .get(1)
                    .map(|o| Self::resolve_depth(doc, resources, o, depth + 1))
                    .unwrap_or(ColorSpace::Unknown);
                let lookup = items
                    .get(3)
                    .and_then(|o| doc.dereference(o).ok())
                    .and_then(|(_, o)| match o {
                        Object::String(bytes, _) => Some(bytes.clone()),
                        Object::Stream(stream) => stream
                            .decompressed_content()
                            .or_else(|_| stream.get_plain_content())
                            .ok(),
                        _ => None,
                    })
                    .unwrap_or_default();
                ColorSpace::Indexed {
                    base: Box::new(base),
                    lookup,
                }
            }
            b"Separation" => ColorSpace::Tint(1),
            b"DeviceN" => {
                let n = items
                    .get(1)
                    .and_then(|o| doc.dereference(o).ok())
                    .and_then(|(_, o)| o.as_array().ok().map(|a| a.len()))
                    .unwrap_or(1);
                ColorSpace::Tint(n.max(1))
            }
            b"Pattern" => ColorSpace::Pattern,
            b"DeviceGray" | b"G" => ColorSpace::Gray,
            b"DeviceRGB" | b"RGB" => ColorSpace::Rgb,
            b"DeviceCMYK" | b"CMYK" => ColorSpace::Cmyk,
            _ => ColorSpace::Unknown,
        }
    }
}

/// Standardumrechnung CMYK → RGB (PDF 32000-1, 10.4.2).
pub(crate) fn cmyk_to_rgb(c: f64, m: f64, y: f64, k: f64) -> Rgb {
    Rgb::new(
        (1.0 - c) * (1.0 - k),
        (1.0 - m) * (1.0 - k),
        (1.0 - y) * (1.0 - k),
    )
}

/// Scannt den Content-Stream einer Seite inklusive Form-XObjects und der
/// Erscheinungsströme (`/AP`) ihrer Annotationen.
///
/// Gibt `Err` zurück, sobald der Seiteninhalt **nicht vollständig durchsucht
/// werden konnte** — ein Strom, der sich nicht zerlegen lässt, oder einer, der
/// das Aufwandskonto [`Budget`] sprengt. Eine solche Seite darf nicht als
/// Erfolg durchgehen: „0 Schwärzungen, Rückgabewert 0“ liest sich wie
/// „nichts gefunden, also sauber“, und genau das wäre es dann nicht.
pub fn scan_page(doc: &Document, page_id: ObjectId) -> Result<ScanResult> {
    let content_data = doc
        .get_page_content(page_id)
        .map_err(|e| RedactError::Pdf(format!("Content-Stream nicht lesbar: {e}")))?;
    // Nicht `lopdf::content::Content::decode`: dessen Parser kennt kein
    // `BI … ID … EI`. Die Binärdaten hinter dem `ID` bringen ihn aus dem Tritt,
    // der Rest des Streams geht verloren — Text hinter einem Inline-Bild wäre
    // für die Analyse unsichtbar und könnte nie geschwärzt werden.
    // [`crate::ops::decode_content`] schneidet die Bilder vorher heraus.
    let decoded = crate::ops::decode_content_checked(&content_data);

    // Ein Abschnitt, der nur bis zur Hälfte zerlegt wurde, fiel früher lautlos
    // unter den Tisch — nicht der ganze Strom, deshalb greift die Prüfung
    // darunter nicht, und mit ihm verschwand jeder Text, der dahinter stand.
    // Derselbe Fehler, nur kleiner und deshalb noch schwerer zu bemerken.
    if !decoded.truncated.is_empty() {
        return Err(RedactError::Pdf(format!(
            "Ein Teil des Seiteninhalts ließ sich nicht in Operationen zerlegen: in {} \
             Teilstück(en) von zusammen {} Byte bricht die Zerlegung ab, alles dahinter \
             fehlt. Dieser Text wurde nicht durchsucht und kann deshalb nicht geschwärzt \
             worden sein; beim Neuschreiben der Seite ginge er zudem ersatzlos verloren. \
             Die Datei wird abgelehnt, statt eine halbe Seite als geschwärzt auszugeben.",
            decoded.truncated.len(),
            decoded.affected_bytes(),
        )));
    }
    let operations = decoded.operations;

    // Früher eine Warnung, jetzt ein Abbruch. Der Unterschied ist der
    // Rückgabewert: eine Warnung auf stderr macht aus einem Lauf, der den Text
    // dieser Seite nachweislich nie gesehen hat, trotzdem eine Datei, die im
    // Stapelbetrieb als „verarbeitet“ zählt. Gemessen an einer 1 122 Byte
    // großen Datei: „Schwärzungen: 0“, Rückgabewert 0 — und die Kontonummer
    // stand unverändert in der Ausgabe.
    if operations.is_empty() && has_tokens(&content_data) {
        return Err(RedactError::Pdf(
            "Der Content-Stream dieser Seite ließ sich nicht in Operationen zerlegen; \
             ihr Text wurde nicht durchsucht und kann deshalb nicht geschwärzt worden \
             sein. Die Datei wird abgelehnt, statt eine ungeprüfte Seite als geschwärzt \
             auszugeben."
                .into(),
        ));
    }

    let resources = page_resources(doc, page_id);
    let mut result = ScanResult::default();
    let mut budget = Budget::default();
    budget.credit(None, operations.len());
    scan_with_budget(
        doc,
        &operations,
        StreamKey::Page,
        resources.as_ref(),
        Matrix::IDENTITY,
        &mut budget,
        &mut result,
    );
    scan_annotations(doc, page_id, resources.as_ref(), &mut budget, &mut result);
    budget.result()?;
    Ok(result)
}

/// Enthält der Stream überhaupt etwas anderes als Leerraum?
fn has_tokens(data: &[u8]) -> bool {
    data.iter().any(|b| !b.is_ascii_whitespace())
}

/// Zählt je Font, wie viel des dekodierten Textes unbrauchbar ist.
///
/// Ein Identity-H-Subset ohne `/ToUnicode` lässt sich nicht dekodieren: die
/// CIDs sind reine Glyphnummern. Der Identity-Rückfall in
/// [`crate::encoding::CharMap::text_for`] macht daraus Steuerzeichen, die
/// Analyse findet nichts, und die Schwärzung meldet Erfolg — an einer Datei,
/// in der alles stehen geblieben ist. Genau dieser Fall muss laut werden.
#[derive(Debug, Default)]
struct FontDecodeStats {
    /// (Ressourcenname, `/BaseFont`) → (Zeichen gesamt, davon unlesbar)
    per_font: BTreeMap<(Vec<u8>, String), (usize, usize)>,
}

impl FontDecodeStats {
    fn record(&mut self, font_name: &[u8], font: &FontInfo, text: &str) {
        // Fonts mit /ToUnicode sagen selbst, was ihre Codes bedeuten.
        if font.charmap.has_to_unicode() {
            return;
        }
        let entry = self
            .per_font
            .entry((font_name.to_vec(), font.base_font.clone()))
            .or_insert((0, 0));
        entry.0 += 1;
        if text.chars().any(|c| c == crate::encoding::REPLACEMENT) {
            entry.1 += 1;
        }
    }

    fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        for ((resource, base_font), (total, unreadable)) in &self.per_font {
            if *unreadable == 0 {
                continue;
            }
            let enough = *unreadable >= UNREADABLE_MIN_GLYPHS
                || (*unreadable as f64) >= *total as f64 * UNREADABLE_RATIO;
            if !enough {
                continue;
            }
            let name = if base_font.is_empty() {
                String::from_utf8_lossy(resource).into_owned()
            } else {
                base_font.clone()
            };
            out.push(format!(
                "Font „{name}“ hat kein /ToUnicode; sein Text lässt sich nicht \
                 dekodieren. Muster können darin nicht erkannt werden — diese \
                 Seite wurde möglicherweise nicht vollständig geschwärzt."
            ));
        }
        out
    }
}

/// Führt einen bereits dekodierten Operationsstrom durch den Interpreter.
///
/// Damit können Aufrufer den Stream selbst dekodieren (z. B. um Inline-Bilder
/// vorher herauszutrennen) und trotzdem exakt dieselbe Zustandsführung
/// benutzen wie [`scan_page`].
///
/// Gibt `Err` zurück, wenn das Aufwandskonto [`Budget`] gerissen wurde — der
/// Durchlauf ist dann unvollständig, und was der Interpreter nicht gesehen
/// hat, kann auch nicht geschwärzt werden.
pub fn interpret(
    doc: &Document,
    operations: &[Operation],
    stream: StreamKey,
    resources: Option<&Dictionary>,
    initial_ctm: Matrix,
    sink: &mut dyn ContentSink,
) -> Result<()> {
    let mut budget = Budget::default();
    budget.credit(None, operations.len());
    scan_with_budget(
        doc,
        operations,
        stream,
        resources,
        initial_ctm,
        &mut budget,
        sink,
    );
    budget.result()
}

/// Der gemeinsame Kern von [`interpret`] und [`scan_page`] — mit einem
/// Aufwandskonto, das über mehrere Ströme derselben Seite hinweg gilt.
fn scan_with_budget(
    doc: &Document,
    operations: &[Operation],
    stream: StreamKey,
    resources: Option<&Dictionary>,
    initial_ctm: Matrix,
    budget: &mut Budget,
    sink: &mut dyn ContentSink,
) {
    let fonts = fonts_from_resources(doc, resources);
    let mut visiting = HashSet::new();
    let mut declared: HashSet<StreamKey> = HashSet::new();
    let mut stats = FontDecodeStats::default();
    scan_operations(
        doc,
        operations,
        stream,
        resources,
        &fonts,
        initial_ctm,
        0,
        &mut visiting,
        &mut declared,
        &mut stats,
        budget,
        sink,
    );
    for warning in stats.warnings() {
        sink.warn(warning);
    }
}

/// Wie viele `/Parent`-Schritte im Seitenbaum verfolgt werden.
///
/// Ein Seitenbaum ist selten tiefer als eine Handvoll Ebenen; die Grenze
/// begrenzt nur den Aufwand bei absichtlich entarteten Dateien. Zyklen fängt
/// ohnehin die Besuchsmenge ab.
const MAX_PAGE_TREE_DEPTH: usize = 64;

/// Sammelt das (ggf. geerbte) `/Resources`-Dictionary einer Seite.
///
/// Die Vererbungskette wird **selbst** abgelaufen und nicht
/// `lopdf::Document::get_page_resources` überlassen: das liefert die geerbten
/// Ressourcen nur als Liste von Objekt-Ids. Steht `/Resources` am
/// `/Pages`-Knoten als *direktes* Dictionary — nach PDF 32000-1, Tabelle 30
/// völlig regulär —, hat es keine Objekt-Id und fällt aus dem Ergebnis. Die
/// Seite sah dann so aus, als hätte sie **gar keine** Ressourcen: ein
/// `/XObject`, das ihr Strom mit `Do` zeichnet, war nicht auflösbar, sein Text
/// wurde nie gelesen und konnte nie geschwärzt werden. Gemessen an einer
/// zweiseitigen Datei: „Treffer 0, Rückgabewert 0“, und die IBAN stand
/// unverändert in der Ausgabe.
pub fn page_resources(doc: &Document, page_id: ObjectId) -> Option<Dictionary> {
    // Von der Seite aufwärts sammeln …
    let mut chain: Vec<Dictionary> = Vec::new();
    let mut seen: HashSet<ObjectId> = HashSet::new();
    let mut current = Some(page_id);
    while let Some(id) = current {
        if seen.len() >= MAX_PAGE_TREE_DEPTH || !seen.insert(id) {
            break;
        }
        let Ok(node) = doc.get_dictionary(id) else {
            break;
        };
        if let Some(resources) = node
            .get(b"Resources")
            .ok()
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| o.as_dict().ok())
        {
            chain.push(resources.clone());
        }
        current = match node.get(b"Parent") {
            Ok(Object::Reference(parent)) => Some(*parent),
            _ => None,
        };
    }

    // … und von oben nach unten mischen, damit die seiteneigenen Ressourcen
    // die geerbten überschreiben.
    let mut merged = Dictionary::new();
    for resources in chain.iter().rev() {
        merge_resources(&mut merged, resources);
    }
    Some(merged)
}

/// Setzt dieser Strom überhaupt Text?
///
/// Ein Formular ohne Textoperator ist eine Zeichnung — ein Logo, ein Rahmen,
/// eine Schraffur. Dass es niemand zeichnet, ist dann kein Befund, und eine
/// Meldung darüber wäre nur Rauschen (vgl. das Kachelmuster in
/// [`scan_tiling_pattern`]).
pub fn stream_shows_text(doc: &Document, id: ObjectId) -> bool {
    let Ok(stream) = doc.get_object(id).and_then(|o| o.as_stream()) else {
        return false;
    };
    let Ok(data) = stream
        .decompressed_content()
        .or_else(|_| stream.get_plain_content())
    else {
        // Nicht lesbar heißt nicht harmlos: hier kann Text stehen.
        return true;
    };
    crate::ops::decode_content_checked(&data)
        .operations
        .iter()
        .any(|op| matches!(op.operator.as_str(), "Tj" | "TJ" | "'" | "\""))
}

// ---------------------------------------------------------------------------
// Annotationen
// ---------------------------------------------------------------------------

/// Zieht die Erscheinungsströme (`/AP`) der Seitenannotationen mit in die
/// Extraktion.
///
/// Eine `/FreeText`-Annotation trägt ihren sichtbaren Text nicht im
/// Seiten-Content-Stream, sondern in einem eigenen Form-XObject unter
/// `/AP /N`. Wer nur den Seitenstrom liest, sieht davon nichts: ein Muster
/// kann dort nichts treffen, und was nicht getroffen wird, wird auch nicht
/// geschwärzt. Die Ströme werden deshalb wie Form-XObjects durchlaufen —
/// transformiert mit der Abbildung aus PDF 32000-1, 12.5.5 (Algorithmus 8.1),
/// damit die Glyphen dort liegen, wo die Annotation auf der Seite steht.
///
/// Die Datensätze tragen [`StreamKey::Form`] mit der Objekt-Id des
/// Erscheinungsstroms; die Schwärzung kann sie damit genauso neu schreiben wie
/// ein gewöhnliches Form-XObject.
fn scan_annotations(
    doc: &Document,
    page_id: ObjectId,
    page_resources: Option<&Dictionary>,
    budget: &mut Budget,
    sink: &mut dyn ContentSink,
) {
    let annots = doc
        .get_dictionary(page_id)
        .ok()
        .and_then(|d| d.get(b"Annots").ok())
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_array().ok())
        .cloned()
        .unwrap_or_default();

    // Ein Strom, der von zwei Annotationen (oder zwei Zuständen) benutzt wird,
    // wird nur einmal gelesen — sonst stünde derselbe Text doppelt im Ergebnis.
    let mut seen: HashSet<ObjectId> = HashSet::new();
    for annot in &annots {
        let Some(dict) = doc
            .dereference(annot)
            .ok()
            .and_then(|(_, o)| o.as_dict().ok())
        else {
            continue;
        };
        let rect = dict
            .get(b"Rect")
            .ok()
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| annot_rect(o));

        let appearance = dict
            .get(b"AP")
            .ok()
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| o.as_dict().ok());
        // Der Zustand, den ein Betrachter zeigt (`/AS`), zuerst — die übrigen
        // danach. Siehe [`appearance_streams`]: sie fallen nicht aus der
        // Analyse, sie bekommen nur ihre eigene Druckschicht.
        let selected = dict
            .get(b"AS")
            .ok()
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| o.as_name().ok().map(|n| n.to_vec()));
        let mut streams = Vec::new();
        if let Some(appearance) = appearance {
            for key in [b"N".as_slice(), b"D".as_slice(), b"R".as_slice()] {
                if let Ok(value) = appearance.get(key) {
                    streams.extend(appearance_streams(doc, value, selected.as_deref()));
                }
            }
            // Alles Übrige — ein `/AP` darf weitere Schlüssel tragen, und was
            // in der Datei steht, wird gelesen.
            for (key, value) in appearance.iter() {
                if matches!(key.as_slice(), b"N" | b"D" | b"R") {
                    continue;
                }
                streams.extend(appearance_streams(doc, value, selected.as_deref()));
            }
        }

        // Ohne Erscheinungsstrom bleibt nur der Klartext in `/Contents` — der
        // hat keine Glyphengeometrie, kann also weder verortet noch geschwärzt
        // werden. Verschwiegen werden darf er trotzdem nicht.
        if streams.is_empty() {
            if annot_has_text(doc, dict) {
                sink.warn(
                    "Eine Annotation trägt Text in /Contents, hat aber keinen lesbaren \
                     Erscheinungsstrom (/AP). Dieser Text wurde nicht durchsucht und \
                     kann deshalb nicht geschwärzt worden sein."
                        .to_string(),
                );
            }
            continue;
        }
        for id in streams {
            if !seen.insert(id) {
                continue;
            }
            scan_appearance(doc, id, rect, page_resources, budget, sink);
        }
    }
}

/// Trägt die Annotation überhaupt Text in `/Contents` (oder `/RC`)?
fn annot_has_text(doc: &Document, dict: &Dictionary) -> bool {
    [b"Contents".as_slice(), b"RC".as_slice()]
        .iter()
        .any(|key| {
            dict.get(key)
                .ok()
                .and_then(|o| doc.dereference(o).ok())
                .is_some_and(|(_, o)| match o {
                    Object::String(bytes, _) => bytes.iter().any(|b| !b.is_ascii_whitespace()),
                    _ => false,
                })
        })
}

/// Objekt-Ids aller Ströme unter einem `/AP`-Eintrag.
///
/// Der Eintrag ist entweder direkt ein Strom oder ein Dictionary von
/// Erscheinungszuständen (`/Off`, `/On`, …). Es werden **alle** Zustände
/// gelesen: was in irgendeinem Zustand steht, steht in der Datei. Jede
/// Checkbox und jedes Radio-Feld bringt so zwei Textflüsse mit, die
/// deckungsgleich auf demselben `/Rect` liegen.
///
/// Sie deshalb wegzulassen wäre falsch — sie *sind* Text, den ein Betrachter
/// unter Umständen zeigt. Sie dürfen sich aber auch nicht gegenseitig
/// unlesbar machen: genau dafür zerlegt [`crate::extract::PdfExtractor`] eine
/// Zeile in Druckschichten. Hier wird nur die Reihenfolge festgelegt — der
/// durch `/AS` benannte, also sichtbare Zustand zuerst.
fn appearance_streams(doc: &Document, value: &Object, selected: Option<&[u8]>) -> Vec<ObjectId> {
    let Ok((id, resolved)) = doc.dereference(value) else {
        return Vec::new();
    };
    match resolved {
        Object::Stream(_) => id.into_iter().collect(),
        Object::Dictionary(states) => {
            let mut out: Vec<ObjectId> = Vec::new();
            let mut rest: Vec<ObjectId> = Vec::new();
            for (name, state) in states.iter() {
                let Ok((Some(id), Object::Stream(_))) = doc.dereference(state) else {
                    continue;
                };
                if selected.is_some_and(|s| s == name.as_slice()) {
                    out.push(id);
                } else {
                    rest.push(id);
                }
            }
            out.append(&mut rest);
            out
        }
        _ => Vec::new(),
    }
}

fn scan_appearance(
    doc: &Document,
    id: ObjectId,
    rect: Option<Rect>,
    page_resources: Option<&Dictionary>,
    budget: &mut Budget,
    sink: &mut dyn ContentSink,
) {
    let Ok(stream) = doc.get_object(id).and_then(|o| o.as_stream()) else {
        return;
    };
    let Ok(data) = stream
        .decompressed_content()
        .or_else(|_| stream.get_plain_content())
    else {
        sink.warn(format!(
            "Der Erscheinungsstrom einer Annotation (Objekt {} {}) ließ sich nicht \
             dekodieren; sein Text wurde nicht durchsucht und kann deshalb nicht \
             geschwärzt worden sein.",
            id.0, id.1
        ));
        return;
    };
    let decoded = crate::ops::decode_content_checked(&data);
    if !decoded.truncated.is_empty() {
        sink.warn(format!(
            "Ein Teil des Erscheinungsstroms einer Annotation (Objekt {} {}) ließ sich nicht \
             in Operationen zerlegen; ab der Bruchstelle fehlt alles Weitere ({} Byte \
             betroffen). Dieser Text wurde nicht durchsucht und kann deshalb nicht \
             geschwärzt worden sein.",
            id.0,
            id.1,
            decoded.affected_bytes()
        ));
    }
    let operations = decoded.operations;
    if operations.is_empty() {
        return;
    }
    budget.credit(Some(id), operations.len());

    let matrix = stream
        .dict
        .get(b"Matrix")
        .ok()
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_array().ok())
        .and_then(|a| matrix_from(a))
        .unwrap_or(Matrix::IDENTITY);
    let bbox = stream
        .dict
        .get(b"BBox")
        .ok()
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| annot_rect(o));
    let resources = stream
        .dict
        .get(b"Resources")
        .ok()
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok())
        .cloned()
        .or_else(|| page_resources.cloned());

    scan_with_budget(
        doc,
        &operations,
        StreamKey::Form(id),
        resources.as_ref(),
        appearance_matrix(&matrix, bbox, rect),
        budget,
        sink,
    );
}

/// Abbildung des Erscheinungsstroms auf `/Rect` (PDF 32000-1, 12.5.5,
/// Algorithmus 8.1): die mit `/Matrix` transformierte `/BBox` wird auf das
/// Annotationsrechteck geschoben und skaliert.
fn appearance_matrix(matrix: &Matrix, bbox: Option<Rect>, rect: Option<Rect>) -> Matrix {
    let (Some(bbox), Some(rect)) = (bbox, rect) else {
        return *matrix;
    };
    let corners = [
        matrix.apply(bbox.ll.x, bbox.ll.y),
        matrix.apply(bbox.ur.x, bbox.ll.y),
        matrix.apply(bbox.ur.x, bbox.ur.y),
        matrix.apply(bbox.ll.x, bbox.ur.y),
    ];
    let mut min = Point::new(f64::MAX, f64::MAX);
    let mut max = Point::new(f64::MIN, f64::MIN);
    for c in corners {
        min.x = min.x.min(c.x);
        min.y = min.y.min(c.y);
        max.x = max.x.max(c.x);
        max.y = max.y.max(c.y);
    }
    // Entartete Kästen werden nur verschoben, nicht skaliert.
    let sx = if max.x - min.x > 1e-9 {
        rect.width() / (max.x - min.x)
    } else {
        1.0
    };
    let sy = if max.y - min.y > 1e-9 {
        rect.height() / (max.y - min.y)
    } else {
        1.0
    };
    let fit = Matrix::translate(-min.x, -min.y)
        .mul(&Matrix::scale(sx, sy))
        .mul(&Matrix::translate(rect.ll.x, rect.ll.y));
    matrix.mul(&fit)
}

/// Rechteck aus einem PDF-Array `[x0 y0 x1 y1]`; die Ecken werden normalisiert.
fn annot_rect(obj: &Object) -> Option<Rect> {
    let array = obj.as_array().ok()?;
    let v: Vec<f64> = array.iter().take(4).filter_map(as_f64).collect();
    if v.len() < 4 {
        return None;
    }
    Some(Rect::new(v[0], v[1], v[2], v[3]))
}

fn merge_resources(target: &mut Dictionary, source: &Dictionary) {
    for (key, value) in source.iter() {
        match (
            target.get(key).ok().and_then(|o| o.as_dict().ok()).cloned(),
            value.as_dict().ok(),
        ) {
            (Some(mut existing), Some(incoming)) => {
                for (k, v) in incoming.iter() {
                    existing.set(k.to_vec(), v.clone());
                }
                target.set(key.to_vec(), Object::Dictionary(existing));
            }
            _ => target.set(key.to_vec(), value.clone()),
        }
    }
}

// ---------------------------------------------------------------------------
// Marked Content: Textspiegel
// ---------------------------------------------------------------------------

/// Sammelt die Textspiegel eines Stroms und ordnet jedem die Textoperationen
/// zu, die er wiedergibt.
///
/// **Warum ein eigener kleiner Durchlauf und nicht die Zustandsschleife?**
/// Welche Operation in welcher Marked-Content-Klammer steht, ist rein
/// syntaktisch — es hängt weder an CTM noch an `Tm`, Font oder Farbe. In
/// [`scan_operations`] müsste die Klammerstruktur trotzdem neben `q`/`Q`, der
/// Form-Rekursion und dem Textzustand mitgeführt werden, obwohl sie mit alldem
/// nichts zu tun hat. Hier steht sie an einer Stelle und ist Zeile für Zeile
/// nachlesbar. Auseinanderlaufen können die beiden nicht: verglichen werden am
/// Ende nur Operationsindizes, und die vergibt in beiden Fällen dieselbe
/// Aufzählung über dasselbe `operations`.
///
/// **Geltungsbereich.**
/// * `BDC … EMC` ist eine Klammer: der Bereich sind die Textoperationen
///   dazwischen. Eine nicht geschlossene Klammer reicht bis zum Stromende — die
///   sichere Richtung.
/// * `DP` ist ein *Punkt* ohne Klammer und hat damit keine eigenen Glyphen.
///   Genommen wird deshalb der nächstliegende Bereich, der einer ist: die
///   umschließende Marked-Content-Klammer, sonst das umschließende Textobjekt
///   (`BT … ET`), sonst der ganze Strom. Ein Punkt beschreibt die Stelle, an
///   der er steht; wird dort etwas entfernt, ist auch seine Beschreibung
///   falsch.
fn scan_marked_text(
    doc: &Document,
    operations: &[Operation],
    stream: StreamKey,
    resources: Option<&Dictionary>,
    sink: &mut dyn ContentSink,
) {
    let shows: Vec<usize> = operations
        .iter()
        .enumerate()
        .filter(|(_, op)| matches!(op.operator.as_str(), "Tj" | "TJ" | "'" | "\""))
        .map(|(index, _)| index)
        .collect();

    // Offene Klammern bzw. das offene Textobjekt, jeweils als Operationsindex.
    let mut open: Vec<usize> = Vec::new();
    let mut text_object: Option<usize> = None;
    // Klammer/Textobjekt → Bereich der eingeschlossenen Operationen.
    let mut ranges: BTreeMap<usize, std::ops::Range<usize>> = BTreeMap::new();
    // Gefundene Spiegel an Klammern und an Punkten; ein Punkt merkt sich
    // zusätzlich, worin er steht.
    let mut brackets: Vec<(usize, Dictionary, Option<ObjectId>)> = Vec::new();
    type PointMirror = (
        usize,
        Dictionary,
        Option<ObjectId>,
        Option<usize>,
        Option<usize>,
    );
    let mut points: Vec<PointMirror> = Vec::new();

    for (index, op) in operations.iter().enumerate() {
        match op.operator.as_str() {
            "BDC" | "BMC" => {
                open.push(index);
                if let Some((list, id)) = mirror_property_list(doc, resources, &op.operands) {
                    brackets.push((index, list, id));
                }
            }
            "EMC" => {
                if let Some(start) = open.pop() {
                    ranges.insert(start, start + 1..index);
                }
            }
            "BT" => text_object = Some(index),
            "ET" => {
                if let Some(start) = text_object.take() {
                    ranges.insert(start, start + 1..index);
                }
            }
            "DP" => {
                if let Some((list, id)) = mirror_property_list(doc, resources, &op.operands) {
                    points.push((index, list, id, open.last().copied(), text_object));
                }
            }
            _ => {}
        }
    }
    // Unabgeschlossen: bis zum Ende des Stroms.
    for start in open.into_iter().chain(text_object) {
        ranges.insert(start, start + 1..operations.len());
    }

    let in_range = |range: &std::ops::Range<usize>| -> Vec<usize> {
        shows
            .iter()
            .copied()
            .filter(|index| range.contains(index))
            .collect()
    };

    // Eine Klammer bringt ihren Bereich selbst mit.
    for (op_index, properties, property_id) in brackets {
        let range = ranges
            .get(&op_index)
            .cloned()
            .unwrap_or(op_index + 1..operations.len());
        sink.marked(MarkedTextRecord {
            stream,
            op_index,
            properties,
            property_id,
            shows: in_range(&range),
        });
    }
    // Ein Punkt erbt den Bereich, in dem er steht.
    for (op_index, properties, property_id, bracket, text_object) in points {
        let range = bracket
            .or(text_object)
            .and_then(|start| ranges.get(&start).cloned())
            .unwrap_or(0..operations.len());
        sink.marked(MarkedTextRecord {
            stream,
            op_index,
            properties,
            property_id,
            shows: in_range(&range),
        });
    }
}

/// Trägt die Eigenschaftsliste eines `BDC`/`DP` einen Textspiegel?
///
/// Liefert die aufgelöste Liste und — falls sie ein eigenes Objekt ist — deren
/// Objekt-Id. `None`, wenn kein Textschlüssel darin steht: dann gibt es nichts
/// zu tun, und ein `/MCID`- oder `/OC`-Eintrag bleibt unangetastet.
fn mirror_property_list(
    doc: &Document,
    resources: Option<&Dictionary>,
    operands: &[Object],
) -> Option<(Dictionary, Option<ObjectId>)> {
    match operands.get(1)? {
        // `/Span <</ActualText (…)>> BDC`
        Object::Dictionary(dict) => has_mirror_key(doc, dict).then(|| (dict.clone(), None)),
        // `/Span /MC0 BDC` — die Liste steht in `/Resources /Properties`.
        Object::Name(name) => {
            let entry = resources
                .and_then(|r| r.get(b"Properties").ok())
                .and_then(|o| doc.dereference(o).ok())
                .and_then(|(_, o)| o.as_dict().ok())
                .and_then(|d| d.get(name.as_slice()).ok())?;
            let (id, resolved) = doc.dereference(entry).ok()?;
            let dict = resolved.as_dict().ok()?;
            has_mirror_key(doc, dict).then(|| (dict.clone(), id))
        }
        _ => None,
    }
}

/// Steht in dieser Liste überhaupt ein nicht leerer Textspiegel?
///
/// Aufgelöst wird auch ein indirekter Verweis: eine Eigenschaftsliste, die als
/// eigenes Objekt in der Datei steht, darf ihren `/ActualText` seinerseits als
/// Verweis führen. Wer hier nur auf `Object::String` prüfte, hielte genau diese
/// Datei für unauffällig.
fn has_mirror_key(doc: &Document, dict: &Dictionary) -> bool {
    MIRROR_KEYS.iter().any(|key| {
        matches!(
            dict.get(key).ok().and_then(|o| doc.dereference(o).ok()),
            Some((_, Object::String(bytes, _))) if bytes.iter().any(|b| !b.is_ascii_whitespace())
        )
    })
}

/// Meldet der Senke jedes Form-XObject, das in diesem Ressourcenverzeichnis
/// **steht** — unabhängig davon, ob der Strom es je zeichnet.
///
/// Nur die Objekt-Id wird angefasst, nicht der Strom: die Prüfung, ob dort
/// überhaupt Text steht, wäre hier zu teuer (sie liefe für jedes Formular auf
/// jeder Ebene) und wird erst fällig, wenn feststeht, dass niemand es
/// gezeichnet hat.
fn declare_forms(doc: &Document, resources: Option<&Dictionary>, sink: &mut dyn ContentSink) {
    let Some(xobjects) = resources
        .and_then(|r| r.get(b"XObject").ok())
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok())
    else {
        return;
    };
    for (name, value) in xobjects.iter() {
        let Ok((Some(id), Object::Stream(stream))) = doc.dereference(value) else {
            continue;
        };
        if stream.dict.get(b"Subtype").and_then(|o| o.as_name()).ok() == Some(b"Form".as_slice()) {
            sink.declares_form(id, name);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn scan_operations(
    doc: &Document,
    operations: &[Operation],
    stream: StreamKey,
    resources: Option<&Dictionary>,
    fonts: &BTreeMap<Vec<u8>, FontInfo>,
    initial_ctm: Matrix,
    depth: usize,
    visiting: &mut HashSet<ObjectId>,
    declared: &mut HashSet<StreamKey>,
    stats: &mut FontDecodeStats,
    budget: &mut Budget,
    sink: &mut dyn ContentSink,
) {
    scan_marked_text(doc, operations, stream, resources, sink);
    // Einmal je Strom, nicht einmal je Platzierung: ein zwanzigmal
    // gezeichnetes Formular bietet zwanzigmal dieselben Ressourcen an, und die
    // Liste durchzugehen kostet jedes Mal.
    if declared.insert(stream) {
        declare_forms(doc, resources, sink);
    }
    let graphics = sink.wants_graphics();
    let mut state = GraphicsState::new(initial_ctm);
    let mut stack: Vec<GraphicsState> = Vec::new();
    // Textmatrix und Zeilenmatrix
    let mut tm = Matrix::IDENTITY;
    let mut tlm = Matrix::IDENTITY;
    // Pfadaufbau: alle Punkte werden sofort in den User-Space gerechnet.
    let mut path: Vec<PathSeg> = Vec::new();
    let mut current = Point::new(0.0, 0.0);
    let mut subpath_start = Point::new(0.0, 0.0);
    // `W`/`W*` merken sich nur die Absicht; wirksam wird der Clip erst nach
    // der folgenden Maloperation.
    let mut pending_clip: Option<bool> = None;

    for (op_index, op) in operations.iter().enumerate() {
        // Jede Interpretation kostet — auch die im hundertsten Durchlauf
        // desselben Form-XObjects. Ohne diese Zeile begrenzt
        // [`MAX_FORM_DEPTH`] nur die Tiefe, und die Breite bleibt frei.
        if !budget.operation() {
            return;
        }
        let cx = SinkContext {
            doc,
            resources,
            stream,
            op_index,
        };
        match op.operator.as_str() {
            "q" => stack.push(state.clone()),
            "Q" => {
                if let Some(prev) = stack.pop() {
                    state = prev;
                }
            }
            "cm" => {
                if let Some(m) = matrix_from(&op.operands) {
                    state.ctm = m.mul(&state.ctm);
                }
            }
            "BT" => {
                tm = Matrix::IDENTITY;
                tlm = Matrix::IDENTITY;
            }
            "ET" => {}
            "Tf" => {
                if let Some(Object::Name(name)) = op.operands.first() {
                    state.text.font = fonts.get(name.as_slice()).cloned();
                    state.text.font_name = name.clone();
                }
                state.text.font_size = op.operands.get(1).and_then(as_f64).unwrap_or(0.0);
            }
            "Tr" => {
                state.text.render_mode =
                    op.operands.first().and_then(as_f64).unwrap_or(0.0).max(0.0) as u8
            }
            "Tc" => state.text.char_spacing = op.operands.first().and_then(as_f64).unwrap_or(0.0),
            "Tw" => state.text.word_spacing = op.operands.first().and_then(as_f64).unwrap_or(0.0),
            "Tz" => {
                state.text.h_scale = op.operands.first().and_then(as_f64).unwrap_or(100.0) / 100.0
            }
            "TL" => state.text.leading = op.operands.first().and_then(as_f64).unwrap_or(0.0),
            "Ts" => state.text.rise = op.operands.first().and_then(as_f64).unwrap_or(0.0),
            "Td" => {
                let tx = op.operands.first().and_then(as_f64).unwrap_or(0.0);
                let ty = op.operands.get(1).and_then(as_f64).unwrap_or(0.0);
                tlm = Matrix::translate(tx, ty).mul(&tlm);
                tm = tlm;
            }
            "TD" => {
                let tx = op.operands.first().and_then(as_f64).unwrap_or(0.0);
                let ty = op.operands.get(1).and_then(as_f64).unwrap_or(0.0);
                state.text.leading = -ty;
                tlm = Matrix::translate(tx, ty).mul(&tlm);
                tm = tlm;
            }
            "Tm" => {
                if let Some(m) = matrix_from(&op.operands) {
                    tlm = m;
                    tm = m;
                }
            }
            "T*" => {
                tlm = Matrix::translate(0.0, -state.text.leading).mul(&tlm);
                tm = tlm;
            }
            "Tj" | "TJ" | "'" | "\"" => {
                // Die Operatoren ' und " beginnen eine neue Zeile.
                if op.operator == "'" || op.operator == "\"" {
                    if op.operator == "\"" {
                        state.text.word_spacing = op
                            .operands
                            .first()
                            .and_then(as_f64)
                            .unwrap_or(state.text.word_spacing);
                        state.text.char_spacing = op
                            .operands
                            .get(1)
                            .and_then(as_f64)
                            .unwrap_or(state.text.char_spacing);
                    }
                    tlm = Matrix::translate(0.0, -state.text.leading).mul(&tlm);
                    tm = tlm;
                }
                let record = show_text(
                    &op.operands,
                    &state,
                    &mut tm,
                    &cx,
                    &op.operator,
                    stats,
                    budget,
                    sink,
                    graphics,
                );
                if let Some(record) = record {
                    // Eine Type3-Glyphe *ist* ein Content-Stream. Ob darin
                    // Text steht, entscheidet sich erst, wenn sie wirklich
                    // gesetzt wird — deshalb hier und nicht schon beim `Tf`.
                    warn_about_text_in_charprocs(
                        doc,
                        resources,
                        stream,
                        &state.text.font_name,
                        budget,
                        sink,
                    );
                    sink.show(record);
                }
            }
            // --- Pfadaufbau -------------------------------------------------
            "m" if graphics => {
                if let Some(p) = point_at(&op.operands, 0, &state.ctm) {
                    current = p;
                    subpath_start = p;
                    path.push(PathSeg::MoveTo(p));
                }
            }
            "l" if graphics => {
                if let Some(p) = point_at(&op.operands, 0, &state.ctm) {
                    current = p;
                    path.push(PathSeg::LineTo(p));
                }
            }
            "c" if graphics => {
                if let (Some(p1), Some(p2), Some(p3)) = (
                    point_at(&op.operands, 0, &state.ctm),
                    point_at(&op.operands, 2, &state.ctm),
                    point_at(&op.operands, 4, &state.ctm),
                ) {
                    current = p3;
                    path.push(PathSeg::CubicTo(p1, p2, p3));
                }
            }
            "v" if graphics => {
                // Erster Kontrollpunkt ist der aktuelle Punkt.
                if let (Some(p2), Some(p3)) = (
                    point_at(&op.operands, 0, &state.ctm),
                    point_at(&op.operands, 2, &state.ctm),
                ) {
                    path.push(PathSeg::CubicTo(current, p2, p3));
                    current = p3;
                }
            }
            "y" if graphics => {
                // Zweiter Kontrollpunkt ist der Endpunkt.
                if let (Some(p1), Some(p3)) = (
                    point_at(&op.operands, 0, &state.ctm),
                    point_at(&op.operands, 2, &state.ctm),
                ) {
                    path.push(PathSeg::CubicTo(p1, p3, p3));
                    current = p3;
                }
            }
            "h" if graphics => {
                path.push(PathSeg::Close);
                current = subpath_start;
            }
            "re" if graphics => {
                if let Some(rect) = rect_path(&op.operands, &state.ctm) {
                    subpath_start = match rect[0] {
                        PathSeg::MoveTo(p) => p,
                        _ => subpath_start,
                    };
                    current = subpath_start;
                    path.extend(rect);
                }
            }
            // --- Clipping ---------------------------------------------------
            "W" if graphics => pending_clip = Some(false),
            "W*" if graphics => pending_clip = Some(true),
            // --- Malen ------------------------------------------------------
            "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "n" if graphics => {
                let operator = op.operator.as_str();
                if matches!(operator, "s" | "b" | "b*") {
                    path.push(PathSeg::Close);
                }
                let fills = matches!(operator, "f" | "F" | "f*" | "B" | "B*" | "b" | "b*");
                let strokes = matches!(operator, "S" | "s" | "B" | "B*" | "b" | "b*");
                let even_odd = matches!(operator, "f*" | "B*" | "b*");
                if !path.is_empty() && (fills || strokes) {
                    sink.path(
                        &cx,
                        &PathEvent {
                            segments: &path,
                            fill: fills.then_some(state.fill),
                            stroke: strokes.then(|| state.stroke_style()),
                            even_odd,
                            fill_alpha: state.fill_alpha,
                            clip: state.clip,
                        },
                    );
                }
                // Der Clip gilt erst *nach* dieser Operation.
                if let Some(clip_even_odd) = pending_clip.take() {
                    if !path.is_empty() {
                        state.clip = sink.clip(&cx, &path, clip_even_odd);
                    }
                }
                path.clear();
            }
            // --- Farbe ------------------------------------------------------
            "g" | "G" if graphics => {
                let v = op.operands.first().and_then(as_f64).unwrap_or(0.0);
                set_color(&mut state, &op.operator, ColorSpace::Gray, Rgb::gray(v));
            }
            "rg" | "RG" if graphics => {
                let rgb = Rgb::new(
                    op.operands.first().and_then(as_f64).unwrap_or(0.0),
                    op.operands.get(1).and_then(as_f64).unwrap_or(0.0),
                    op.operands.get(2).and_then(as_f64).unwrap_or(0.0),
                );
                set_color(&mut state, &op.operator, ColorSpace::Rgb, rgb);
            }
            "k" | "K" if graphics => {
                let rgb = cmyk_to_rgb(
                    op.operands.first().and_then(as_f64).unwrap_or(0.0),
                    op.operands.get(1).and_then(as_f64).unwrap_or(0.0),
                    op.operands.get(2).and_then(as_f64).unwrap_or(0.0),
                    op.operands.get(3).and_then(as_f64).unwrap_or(0.0),
                );
                set_color(&mut state, &op.operator, ColorSpace::Cmyk, rgb);
            }
            "cs" | "CS" if graphics => {
                let space = op
                    .operands
                    .first()
                    .map(|o| ColorSpace::resolve(doc, resources, o))
                    .unwrap_or(ColorSpace::Unknown);
                let color = space.initial_color();
                set_color(&mut state, &op.operator, space, color);
            }
            "sc" | "scn" | "SC" | "SCN" => {
                // Ein Namensoperand benennt ein Muster. Kachelmuster sind
                // eigene Content-Streams — dort kann Text stehen, den sonst
                // niemand zu Gesicht bekommt. Das gilt auch für die reine
                // Textextraktion, deshalb steht dieser Zweig **vor** der
                // Grafikschranke.
                if let Some(Object::Name(pattern)) =
                    op.operands.iter().find(|o| matches!(o, Object::Name(_)))
                {
                    scan_tiling_pattern(
                        doc,
                        resources,
                        pattern,
                        initial_ctm,
                        depth,
                        visiting,
                        declared,
                        stats,
                        budget,
                        sink,
                    );
                }
                if !graphics {
                    continue;
                }
                let stroking = op.operator.starts_with('S');
                let space = if stroking {
                    state.stroke_space.clone()
                } else {
                    state.fill_space.clone()
                };
                let values: Vec<f64> = op.operands.iter().filter_map(as_f64).collect();
                let color = if values.is_empty() {
                    // Nur ein Musternamen — Muster werden als mittleres Grau genähert.
                    Rgb::gray(0.5)
                } else {
                    space.to_rgb(&values)
                };
                if stroking {
                    state.stroke = color;
                } else {
                    state.fill = color;
                }
            }
            // --- Linienzustand ----------------------------------------------
            "w" if graphics => {
                state.line_width = op.operands.first().and_then(as_f64).unwrap_or(1.0).max(0.0)
            }
            "J" if graphics => {
                state.line_cap = op.operands.first().and_then(as_f64).unwrap_or(0.0).max(0.0) as u8
            }
            "j" if graphics => {
                state.line_join = op.operands.first().and_then(as_f64).unwrap_or(0.0).max(0.0) as u8
            }
            "M" if graphics => {}
            "d" if graphics => {
                state.dash = op
                    .operands
                    .first()
                    .and_then(|o| o.as_array().ok())
                    .map(|a| a.iter().filter_map(as_f64).filter(|v| *v >= 0.0).collect())
                    .unwrap_or_default();
                // Eine Strichelung aus lauter Nullen bedeutet „durchgezogen“.
                if state.dash.iter().all(|d| *d <= 0.0) {
                    state.dash.clear();
                }
                state.dash_phase = op.operands.get(1).and_then(as_f64).unwrap_or(0.0);
            }
            "gs" => {
                if graphics {
                    apply_ext_gstate(doc, resources, &op.operands, &mut state);
                }
                // Eine weiche Maske mit Gruppen-Form trägt einen **eigenen**
                // Inhaltsstrom, den kein `Do` je erreicht. Er wird hier
                // betreten — unabhängig davon, ob die Senke Grafik will: Text
                // ist Text, gleich in welcher Rolle er in der Datei steht.
                scan_soft_mask(
                    doc,
                    resources,
                    &op.operands,
                    &state,
                    depth,
                    visiting,
                    declared,
                    stats,
                    budget,
                    sink,
                );
            }
            // --- Inline-Bild ------------------------------------------------
            // Der Operationsstrom kommt von `ops::decode_content`, das
            // `BI … ID … EI` zu einer Operation mit Dictionary und Rohdaten
            // zusammenfasst.
            "BI" if graphics => {
                if let (Some(Object::Dictionary(dict)), Some(Object::String(data, _))) =
                    (op.operands.first(), op.operands.get(1))
                {
                    sink.image(
                        &cx,
                        &ImageEvent {
                            name: None,
                            inline: Some((dict, data)),
                            ctm: state.ctm,
                            fill: state.fill,
                            fill_alpha: state.fill_alpha,
                            clip: state.clip,
                        },
                    );
                }
            }
            "Do" => {
                let Some(Object::Name(name)) = op.operands.first() else {
                    continue;
                };
                match load_xobject(doc, resources, name, budget) {
                    // Kein solcher Eintrag: es wird nichts gezeichnet, also
                    // versteckt sich hier auch nichts.
                    XObjectEntry::Missing => {}
                    XObjectEntry::Image => {
                        if graphics {
                            sink.image(
                                &cx,
                                &ImageEvent {
                                    name: Some(name),
                                    inline: None,
                                    ctm: state.ctm,
                                    fill: state.fill,
                                    fill_alpha: state.fill_alpha,
                                    clip: state.clip,
                                },
                            );
                        }
                    }
                    // Vorhanden, aber nicht lesbar. Früher ein stilles
                    // `continue`: der Text darin fehlte in der Analyse, die
                    // Schwärzung meldete „nichts gefunden“, und die Datei galt
                    // als sauber.
                    XObjectEntry::Unusable(message) => sink.warn(message),
                    XObjectEntry::Form(form_id, form_dict, form_ops) => {
                        if depth >= MAX_FORM_DEPTH {
                            sink.warn(format!(
                                "Form-XObject „{}“ ist tiefer als {MAX_FORM_DEPTH} Ebenen \
                                 verschachtelt; ab dort wurde nicht weitergelesen. Text in \
                                 den tieferen Ebenen wurde nicht durchsucht und kann \
                                 deshalb nicht geschwärzt worden sein.",
                                String::from_utf8_lossy(name)
                            ));
                            continue;
                        }
                        sink.form(form_id);
                        if !visiting.insert(form_id) {
                            continue; // Zyklus
                        }
                        let form_matrix = form_dict
                            .get(b"Matrix")
                            .ok()
                            .and_then(|o| o.as_array().ok())
                            .and_then(|a| matrix_from(a))
                            .unwrap_or(Matrix::IDENTITY);
                        let form_resources = form_dict
                            .get(b"Resources")
                            .ok()
                            .and_then(|o| doc.dereference(o).ok())
                            .and_then(|(_, o)| o.as_dict().ok())
                            .cloned()
                            .or_else(|| resources.cloned());
                        let form_fonts = fonts_from_resources(doc, form_resources.as_ref());
                        scan_operations(
                            doc,
                            &form_ops,
                            StreamKey::Form(form_id),
                            form_resources.as_ref(),
                            &form_fonts,
                            form_matrix.mul(&state.ctm),
                            depth + 1,
                            visiting,
                            declared,
                            stats,
                            budget,
                            sink,
                        );
                        visiting.remove(&form_id);
                    }
                }
            }
            // `sh` (Schattierungen) und alles Unbekannte werden übergangen.
            _ => {}
        }
    }
}

/// Setzt Füll- bzw. Strichfarbe; Großbuchstaben-Operatoren betreffen den Strich.
fn set_color(state: &mut GraphicsState, operator: &str, space: ColorSpace, color: Rgb) {
    if operator.chars().next().is_some_and(|c| c.is_uppercase()) {
        state.stroke_space = space;
        state.stroke = color;
    } else {
        state.fill_space = space;
        state.fill = color;
    }
}

/// Das `/ExtGState`-Dictionary hinter einem `gs`-Operanden.
fn ext_gstate_dict(
    doc: &Document,
    resources: Option<&Dictionary>,
    operands: &[Object],
) -> Option<Dictionary> {
    let Some(Object::Name(name)) = operands.first() else {
        return None;
    };
    resources
        .and_then(|r| r.get(b"ExtGState").ok())
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok())
        .and_then(|d| d.get(name.as_slice()).ok())
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok().cloned())
}

/// Betritt die Gruppen-Form einer weichen Maske (`/ExtGState /SMask /G`).
///
/// Eine weiche Maske ist ein vollwertiges Form-XObject mit eigenem
/// Ressourcenverzeichnis und eigenem Text. Sie wird nicht mit `Do` gezeichnet,
/// sondern über die Grafikzustands-Parameter gesetzt — der Interpreter kam
/// deshalb nie hinein. Ihr Text stand ungelesen in der Datei: „Treffer 0,
/// Rückgabewert 0“, während `pdftotext` die IBAN im Klartext las.
///
/// Gelesen wird im Koordinatensystem, das beim `gs` gilt (PDF 32000-1,
/// 11.6.5.2), also mit der CTM dieses Augenblicks. Die Datensätze tragen
/// [`StreamKey::Form`] mit der Objekt-Id der Gruppe; die Schwärzung schreibt
/// sie damit wie jedes andere Formular neu.
#[allow(clippy::too_many_arguments)]
fn scan_soft_mask(
    doc: &Document,
    resources: Option<&Dictionary>,
    operands: &[Object],
    state: &GraphicsState,
    depth: usize,
    visiting: &mut HashSet<ObjectId>,
    declared: &mut HashSet<StreamKey>,
    stats: &mut FontDecodeStats,
    budget: &mut Budget,
    sink: &mut dyn ContentSink,
) {
    let Some(gstate) = ext_gstate_dict(doc, resources, operands) else {
        return;
    };
    // `/SMask /None` schaltet die Maske ab und ist ein Name, kein Dictionary.
    let Some(mask) = gstate
        .get(b"SMask")
        .ok()
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok().cloned())
    else {
        return;
    };
    let Ok(group) = mask.get(b"G") else {
        return;
    };
    let Some(group_id) = group.as_reference().ok() else {
        // Eine Gruppe ohne eigene Objekt-Id ließe sich nicht neu schreiben —
        // was darin steht, bliebe stehen. Das muss gesagt werden.
        sink.warn(
            "Eine weiche Maske (/ExtGState /SMask /G) trägt ihre Gruppen-Form nicht als \
             eigenes Objekt; sie wurde nicht durchsucht und kann deshalb nicht \
             geschwärzt worden sein."
                .to_string(),
        );
        return;
    };
    if depth >= MAX_FORM_DEPTH {
        sink.warn(format!(
            "Die Gruppen-Form einer weichen Maske (Objekt {} {}) liegt tiefer als \
             {MAX_FORM_DEPTH} Ebenen; ab dort wurde nicht weitergelesen. Ihr Text wurde \
             nicht durchsucht und kann deshalb nicht geschwärzt worden sein.",
            group_id.0, group_id.1
        ));
        return;
    }
    let Ok(stream) = doc.get_object(group_id).and_then(|o| o.as_stream()) else {
        sink.warn(format!(
            "Die Gruppen-Form einer weichen Maske (Objekt {} {}) ist kein lesbarer Strom; \
             ihr Text wurde nicht durchsucht und kann deshalb nicht geschwärzt worden sein.",
            group_id.0, group_id.1
        ));
        return;
    };
    let Ok(data) = stream
        .decompressed_content()
        .or_else(|_| stream.get_plain_content())
    else {
        sink.warn(format!(
            "Die Gruppen-Form einer weichen Maske (Objekt {} {}) ließ sich nicht \
             dekodieren; ihr Text wurde nicht durchsucht und kann deshalb nicht \
             geschwärzt worden sein.",
            group_id.0, group_id.1
        ));
        return;
    };
    let decoded = crate::ops::decode_content_checked(&data);
    if !decoded.truncated.is_empty() {
        sink.warn(format!(
            "Ein Teil der Gruppen-Form einer weichen Maske (Objekt {} {}) ließ sich nicht \
             in Operationen zerlegen ({} Byte betroffen). Dieser Text wurde nicht \
             durchsucht und kann deshalb nicht geschwärzt worden sein.",
            group_id.0,
            group_id.1,
            decoded.affected_bytes()
        ));
    }
    let operations = decoded.operations;
    if operations.is_empty() {
        return;
    }
    budget.credit(Some(group_id), operations.len());
    if !visiting.insert(group_id) {
        return; // Zyklus
    }
    let matrix = stream
        .dict
        .get(b"Matrix")
        .ok()
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_array().ok())
        .and_then(|a| matrix_from(a))
        .unwrap_or(Matrix::IDENTITY);
    let group_resources = stream
        .dict
        .get(b"Resources")
        .ok()
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok())
        .cloned()
        .or_else(|| resources.cloned());
    let group_fonts = fonts_from_resources(doc, group_resources.as_ref());
    // Die Maske ist ein platzierter Strom wie ein Formular; sie muss auch so
    // gezählt werden, sonst hielte die Ressourcenprüfung sie für ungezeichnet.
    sink.form(group_id);
    scan_operations(
        doc,
        &operations,
        StreamKey::Form(group_id),
        group_resources.as_ref(),
        &group_fonts,
        matrix.mul(&state.ctm),
        depth + 1,
        visiting,
        declared,
        stats,
        budget,
        sink,
    );
    visiting.remove(&group_id);
}

/// Übernimmt die für das Zeichnen relevanten Einträge aus einem `/ExtGState`.
fn apply_ext_gstate(
    doc: &Document,
    resources: Option<&Dictionary>,
    operands: &[Object],
    state: &mut GraphicsState,
) {
    let Some(dict) = ext_gstate_dict(doc, resources, operands) else {
        return;
    };
    if let Some(lw) = dict.get(b"LW").ok().and_then(as_f64) {
        state.line_width = lw.max(0.0);
    }
    if let Some(ca) = dict.get(b"ca").ok().and_then(as_f64) {
        state.fill_alpha = ca.clamp(0.0, 1.0) as f32;
    }
    if let Some(ca) = dict.get(b"CA").ok().and_then(as_f64) {
        state.stroke_alpha = ca.clamp(0.0, 1.0) as f32;
    }
    if let Some(lc) = dict.get(b"LC").ok().and_then(as_f64) {
        state.line_cap = lc.max(0.0) as u8;
    }
    if let Some(lj) = dict.get(b"LJ").ok().and_then(as_f64) {
        state.line_join = lj.max(0.0) as u8;
    }
    if let Some(d) = dict.get(b"D").ok().and_then(|o| o.as_array().ok()) {
        if let Some(array) = d.first().and_then(|o| o.as_array().ok()) {
            state.dash = array
                .iter()
                .filter_map(as_f64)
                .filter(|v| *v >= 0.0)
                .collect();
            if state.dash.iter().all(|v| *v <= 0.0) {
                state.dash.clear();
            }
        }
        state.dash_phase = d.get(1).and_then(as_f64).unwrap_or(0.0);
    }
}

/// Punkt aus zwei Operanden, direkt in den User-Space transformiert.
fn point_at(operands: &[Object], index: usize, ctm: &Matrix) -> Option<Point> {
    let x = operands.get(index).and_then(as_f64)?;
    let y = operands.get(index + 1).and_then(as_f64)?;
    Some(ctm.apply(x, y))
}

/// `re`: ein Rechteck als geschlossener Teilpfad im User-Space.
fn rect_path(operands: &[Object], ctm: &Matrix) -> Option<Vec<PathSeg>> {
    let x = operands.first().and_then(as_f64)?;
    let y = operands.get(1).and_then(as_f64)?;
    let w = operands.get(2).and_then(as_f64)?;
    let h = operands.get(3).and_then(as_f64)?;
    Some(vec![
        PathSeg::MoveTo(ctm.apply(x, y)),
        PathSeg::LineTo(ctm.apply(x + w, y)),
        PathSeg::LineTo(ctm.apply(x + w, y + h)),
        PathSeg::LineTo(ctm.apply(x, y + h)),
        PathSeg::Close,
    ])
}

/// Was hinter einem `Do`-Namen in den Ressourcen steckt.
enum XObjectEntry {
    Image,
    /// Form-XObject: Objekt-Id, Dictionary und dekodierter Operationsstrom.
    Form(ObjectId, Dictionary, Vec<Operation>),
    /// Vorhanden, aber nicht auswertbar — mit fertiger Begründung für die
    /// Warnung. Was hier steht, wird nicht durchsucht; das muss der Nutzer
    /// erfahren.
    Unusable(String),
    /// Kein solcher Eintrag in den Ressourcen.
    Missing,
}

/// Löst einen `Do`-Namen auf und dekodiert bei einem Form-XObject gleich den
/// Inhalt.
///
/// Jeder Weg, auf dem hier nichts Brauchbares herauskommt, wird benannt statt
/// verschwiegen: ein Form-XObject ohne `/Subtype` oder mit einem Filter, den
/// niemand dekodieren kann, versteckt seinen Text sonst lautlos.
fn load_xobject(
    doc: &Document,
    resources: Option<&Dictionary>,
    name: &[u8],
    budget: &mut Budget,
) -> XObjectEntry {
    let label = String::from_utf8_lossy(name).into_owned();
    let entry = resources
        .and_then(|r| r.get(b"XObject").ok())
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok())
        .and_then(|d| d.get(name).ok());
    let Some(entry) = entry else {
        return XObjectEntry::Missing;
    };
    let Ok((id, resolved)) = doc.dereference(entry) else {
        return XObjectEntry::Unusable(format!(
            "XObject „{label}“ verweist auf ein Objekt, das es nicht gibt; sein \
             Inhalt wurde nicht durchsucht."
        ));
    };
    let Ok(stream) = resolved.as_stream() else {
        return XObjectEntry::Unusable(format!(
            "XObject „{label}“ ist kein Stream; sein Inhalt wurde nicht durchsucht."
        ));
    };
    let subtype = stream
        .dict
        .get(b"Subtype")
        .and_then(Object::as_name)
        .ok()
        .map(|n| n.to_vec());
    match subtype.as_deref() {
        Some(b"Image") => XObjectEntry::Image,
        Some(b"Form") => {
            let Some(id) = id else {
                return XObjectEntry::Unusable(format!(
                    "Form-XObject „{label}“ ist kein eigenständiges Objekt; sein Text \
                     wurde nicht durchsucht und kann deshalb nicht geschwärzt worden sein."
                ));
            };
            let Ok(data) = stream
                .decompressed_content()
                .or_else(|_| stream.get_plain_content())
            else {
                return XObjectEntry::Unusable(format!(
                    "Form-XObject „{label}“ ließ sich nicht dekodieren (unbekannter oder \
                     defekter Filter); sein Text wurde nicht durchsucht und kann deshalb \
                     nicht geschwärzt worden sein."
                ));
            };
            let decoded = crate::ops::decode_content_checked(&data);
            if !decoded.truncated.is_empty() {
                return XObjectEntry::Unusable(format!(
                    "Ein Teil des Form-XObjects „{label}“ ließ sich nicht in Operationen \
                     zerlegen; ab der Bruchstelle fehlt alles Weitere ({} Byte betroffen). \
                     Dieser Text wurde nicht durchsucht und kann deshalb nicht geschwärzt \
                     worden sein.",
                    decoded.affected_bytes()
                ));
            }
            let operations = decoded.operations;
            // Der Inhalt dieses Stroms bringt einmal Guthaben ein; jede
            // weitere Platzierung zehrt nur noch davon.
            budget.credit(Some(id), operations.len());
            if operations.is_empty() && has_tokens(&data) {
                return XObjectEntry::Unusable(format!(
                    "Der Inhalt des Form-XObjects „{label}“ ließ sich nicht in Operationen \
                     zerlegen; sein Text wurde nicht durchsucht und kann deshalb nicht \
                     geschwärzt worden sein."
                ));
            }
            XObjectEntry::Form(id, stream.dict.clone(), operations)
        }
        // Ohne `/Subtype /Form` steigt der Interpreter aus — und stünde dann
        // vor genau dem Text, den er hätte finden sollen.
        _ => XObjectEntry::Unusable(format!(
            "XObject „{label}“ hat kein bekanntes /Subtype (weder /Form noch /Image); \
             sein Inhalt wurde nicht durchsucht. Steht dort Text, blieb er ungeschwärzt."
        )),
    }
}

/// Durchläuft ein Kachelmuster (`/PatternType 1`) — dessen Content-Stream kann
/// Text enthalten.
///
/// Das Muster wird gekachelt gemalt; durchlaufen wird nur die Kachel im
/// Ursprung des Musterraums. Für die Schwärzung reicht das: gefunden wird der
/// Text an der Stelle dieser einen Kachel, entfernt wird er aus dem
/// Musterstrom — und damit aus **allen** Kacheln. Dass die weiteren Kacheln
/// nicht einzeln vermessen werden, sagt die Warnung.
#[allow(clippy::too_many_arguments)]
fn scan_tiling_pattern(
    doc: &Document,
    resources: Option<&Dictionary>,
    name: &[u8],
    base_ctm: Matrix,
    depth: usize,
    visiting: &mut HashSet<ObjectId>,
    declared: &mut HashSet<StreamKey>,
    stats: &mut FontDecodeStats,
    budget: &mut Budget,
    sink: &mut dyn ContentSink,
) {
    let label = String::from_utf8_lossy(name).into_owned();
    let entry = resources
        .and_then(|r| r.get(b"Pattern").ok())
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok())
        .and_then(|d| d.get(name).ok());
    let Some(entry) = entry else {
        return;
    };
    let Ok((id, resolved)) = doc.dereference(entry) else {
        return;
    };
    // Ein Schattierungsmuster (`/PatternType 2`) ist ein Dictionary ohne
    // Content-Stream; dort steht kein Text.
    let Ok(stream) = resolved.as_stream() else {
        return;
    };
    if stream.dict.get(b"PatternType").ok().and_then(as_f64) == Some(2.0) {
        return;
    }
    let Ok(data) = stream
        .decompressed_content()
        .or_else(|_| stream.get_plain_content())
    else {
        sink.warn(format!(
            "Kachelmuster „{label}“ ließ sich nicht dekodieren; sein Inhalt wurde nicht \
             durchsucht. Steht dort Text, blieb er ungeschwärzt."
        ));
        return;
    };
    let decoded = crate::ops::decode_content_checked(&data);
    if !decoded.truncated.is_empty() {
        sink.warn(format!(
            "Ein Teil des Kachelmusters „{label}“ ließ sich nicht in Operationen zerlegen; \
             ab der Bruchstelle fehlt alles Weitere ({} Byte betroffen). Dieser Text wurde \
             nicht durchsucht und kann deshalb nicht geschwärzt worden sein.",
            decoded.affected_bytes()
        ));
    }
    let operations = decoded.operations;
    // Ein Muster ohne Textoperator ist ein Schraffur- oder Logomuster: kein
    // Befund, keine Meldung.
    if !operations
        .iter()
        .any(|op| matches!(op.operator.as_str(), "Tj" | "TJ" | "'" | "\""))
    {
        return;
    }
    if depth >= MAX_FORM_DEPTH {
        sink.warn(format!(
            "Kachelmuster „{label}“ liegt tiefer als {MAX_FORM_DEPTH} Ebenen \
             verschachtelt; sein Text wurde nicht durchsucht und kann deshalb nicht \
             geschwärzt worden sein."
        ));
        return;
    }
    let Some(id) = id else {
        sink.warn(format!(
            "Kachelmuster „{label}“ ist kein eigenständiges Objekt; sein Text wurde \
             nicht durchsucht und kann deshalb nicht geschwärzt worden sein."
        ));
        return;
    };
    // Einmal je Dokumentobjekt: ein zweiter Durchlauf brächte nur denselben
    // Text ein zweites Mal (und bei Zyklen gar keinen).
    if !visiting.insert(id) {
        return;
    }
    budget.credit(Some(id), operations.len());

    sink.warn(format!(
        "Kachelmuster „{label}“ enthält Text. Er wird an der Stelle der ersten Kachel \
         gesucht und beim Schwärzen aus dem Muster entfernt — die übrigen Kacheln \
         werden dabei nicht einzeln vermessen. Bitte das Ergebnis dort prüfen."
    ));

    // Der Musterraum hängt am Ausgangszustand des Streams, nicht an der CTM
    // zum Zeitpunkt des `scn` (PDF 32000-1, 8.7.3.1).
    let matrix = stream
        .dict
        .get(b"Matrix")
        .ok()
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_array().ok())
        .and_then(|a| matrix_from(a))
        .unwrap_or(Matrix::IDENTITY);
    let pattern_resources = stream
        .dict
        .get(b"Resources")
        .ok()
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok())
        .cloned()
        .or_else(|| resources.cloned());
    let fonts = fonts_from_resources(doc, pattern_resources.as_ref());
    scan_operations(
        doc,
        &operations,
        StreamKey::Form(id),
        pattern_resources.as_ref(),
        &fonts,
        matrix.mul(&base_ctm),
        depth + 1,
        visiting,
        declared,
        stats,
        budget,
        sink,
    );
}

/// Meldet Type3-Glyphprozeduren, die selbst Text setzen.
///
/// Ein Type3-Font hat kein Fontprogramm: jede Glyphe ist ein Content-Stream
/// unter `/CharProcs`. Üblicherweise malt der nur — dann ist alles in Ordnung
/// und hier passiert nichts. Er darf aber auch `BT … Tj` enthalten, also mit
/// einem *anderen* Font Klartext setzen. Dann geht die Schwärzung ins Leere,
/// ohne es zu merken: der Zeichencode verschwindet sauber aus dem Seitenstrom,
/// die Glyphe ist danach unsichtbar — und der Klartext steht weiter im
/// Prozedurstrom, wo `leaks` ihn findet.
///
/// ## Warum nur eine Warnung, statt `/CharProcs` mitzudurchsuchen
///
/// Durchsuchen hieße hier: rekursiv interpretieren **und** anschließend
/// umschreiben. Beides ist an dieser Stelle schlechter als eine ehrliche
/// Meldung:
///
/// * Eine Glyphprozedur gehört keiner Stelle auf der Seite, sondern **allen**
///   Vorkommen ihres Zeichencodes — im ganzen Dokument. Was man dort entfernt,
///   entfernt man überall; was man dort stehen lässt, bleibt überall stehen.
///   Dieselbe Unschärfe wie beim Kachelmuster, nur ohne dessen Nutzen: sichtbar
///   ist der Text ohnehin schon weg, weil der Zeichencode aus dem Seitenstrom
///   verschwindet.
/// * Eine neue Rekursion muss gegen das Aufwandskonto [`Budget`] gerechnet
///   werden. Eine Schrift mit vielen Glyphprozeduren, vielfach gesetzt, wäre
///   sonst genau der nächste Weg an der Grenze vorbei — und dieser Weg wäre
///   billiger als jeder bisherige, weil er nicht einmal ein Form-XObject
///   braucht. Der Blick hierher zahlt deshalb je untersuchter Operation und
///   findet je Strom und Schrift genau einmal statt.
///
/// Was bleibt, ist die Meldung: der Befund heißt dann nicht mehr „angewendet“,
/// und der Nutzer weiß, wo er nachsehen muss.
fn warn_about_text_in_charprocs(
    doc: &Document,
    resources: Option<&Dictionary>,
    stream: StreamKey,
    font_name: &[u8],
    budget: &mut Budget,
    sink: &mut dyn ContentSink,
) {
    if font_name.is_empty() || !budget.first_look_at_type3(stream, font_name) {
        return;
    }
    let font = resources
        .and_then(|r| r.get(b"Font").ok())
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok())
        .and_then(|d| d.get(font_name).ok())
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok().cloned());
    let Some(font) = font else {
        return;
    };
    if font.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Type3".as_slice()) {
        return;
    }
    let procs = font
        .get(b"CharProcs")
        .ok()
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok().cloned());
    let Some(procs) = procs else {
        return;
    };

    let label = String::from_utf8_lossy(font_name).into_owned();
    let mut with_text: Vec<String> = Vec::new();
    let mut unreadable = 0usize;
    for (name, value) in procs.iter() {
        let Ok((id, resolved)) = doc.dereference(value) else {
            unreadable += 1;
            continue;
        };
        let Ok(proc_stream) = resolved.as_stream() else {
            unreadable += 1;
            continue;
        };
        let Ok(data) = proc_stream
            .decompressed_content()
            .or_else(|_| proc_stream.get_plain_content())
        else {
            unreadable += 1;
            continue;
        };
        let decoded = crate::ops::decode_content_checked(&data);
        if !decoded.truncated.is_empty() {
            unreadable += 1;
        }
        // Der Inhalt bringt einmal Guthaben ein — wie jeder andere Strom auch.
        budget.credit(id, decoded.operations.len());
        let mut sets_text = false;
        for op in &decoded.operations {
            // Jede geprüfte Operation kostet. Ist das Konto leer, endet der
            // Scan ohnehin; dann wird hier nichts mehr behauptet.
            if !budget.operation() {
                return;
            }
            if matches!(op.operator.as_str(), "Tj" | "TJ" | "'" | "\"") {
                sets_text = true;
            }
        }
        if sets_text {
            with_text.push(String::from_utf8_lossy(name).into_owned());
        }
    }

    if unreadable > 0 {
        sink.warn(format!(
            "Von der Type3-Schrift „{label}“ ließen sich {unreadable} Glyphprozedur(en) \
             (/CharProcs) nicht vollständig lesen. Steht dort Text, wurde er nicht \
             durchsucht und kann deshalb nicht geschwärzt worden sein."
        ));
    }
    if with_text.is_empty() {
        return;
    }
    with_text.sort();
    sink.warn(format!(
        "Die Type3-Schrift „{label}“ setzt in {} ihrer Glyphprozeduren selbst Text \
         (/CharProcs: {}). Diese Prozeduren sind eigene Ströme und wurden nicht \
         durchsucht: eine Schwärzung entfernt zwar den Zeichencode aus dem Seiteninhalt \
         — die Glyphe ist danach nicht mehr zu sehen —, der Klartext bleibt aber im \
         Prozedurstrom stehen und ist in der Datei weiter zu finden. Bitte das Ergebnis \
         dort prüfen.",
        with_text.len(),
        with_text.join(", ")
    ));
}

/// Berechnet die Glyphen einer Text-Ausgabe-Operation und schreibt `tm` fort.
///
/// **Einzige Stelle** im Programm, an der Textmatrix, Rendering-Matrix und
/// Vorschub berechnet werden. Sowohl der [`ShowRecord`] für Extraktion und
/// Schwärzung als auch das Glyph-Ereignis für den Renderer entstehen hier aus
/// denselben Zwischenwerten.
#[allow(clippy::too_many_arguments)]
fn show_text(
    operands: &[Object],
    state: &GraphicsState,
    tm: &mut Matrix,
    cx: &SinkContext,
    operator: &str,
    stats: &mut FontDecodeStats,
    budget: &mut Budget,
    sink: &mut dyn ContentSink,
    emit_glyphs: bool,
) -> Option<ShowRecord> {
    let font = state.text.font.clone().unwrap_or_default();
    let ts = &state.text;

    // Die Textargumente stehen bei ' und " nicht an erster Stelle.
    let arg = match operator {
        "\"" => operands.get(2)?,
        "'" => operands.first()?,
        _ => operands.first()?,
    };

    let mut elements: Vec<Object> = Vec::new();
    match arg {
        Object::Array(items) => elements.extend(items.iter().cloned()),
        other => elements.push(other.clone()),
    }

    // Die Leerzeichenbreite des Fonts ist der Maßstab für „echte Lücke“.
    let space_width = font.width(32, " ");

    let mut items = Vec::new();
    for element in &elements {
        match element {
            Object::String(bytes, _) => {
                for (code, text, nbytes) in font.charmap.decode(bytes) {
                    // Innerhalb der Schleife, nicht erst danach: eine einzige
                    // `Tj`-Operation kann Millionen Zeichen tragen, und jedes
                    // davon wird als [`GlyphItem`] gehalten.
                    if !budget.glyph() {
                        break;
                    }
                    stats.record(&ts.font_name, &font, &text);
                    let w0 = font.width(code, &text);
                    let is_space = nbytes == 1 && code == 32;
                    let displacement = (w0 * ts.font_size
                        + ts.char_spacing
                        + if is_space { ts.word_spacing } else { 0.0 })
                        * ts.h_scale;

                    let param = Matrix::new(
                        ts.font_size * ts.h_scale,
                        0.0,
                        0.0,
                        ts.font_size,
                        0.0,
                        ts.rise,
                    );
                    // Textraum → User-Space; daraus stammt die Schreibrichtung.
                    let text_to_user = tm.mul(&state.ctm);
                    let trm = param.mul(&text_to_user);
                    let rect = glyph_rect(&trm, w0, font.ascent, font.descent);
                    let origin = trm.apply(0.0, 0.0);
                    let baseline = baseline_of(
                        &text_to_user,
                        displacement,
                        (font.ascent - font.descent) * ts.font_size,
                        space_width * ts.font_size * ts.h_scale,
                    );

                    if emit_glyphs {
                        sink.glyph(
                            cx,
                            &GlyphEvent {
                                font_name: &ts.font_name,
                                font: &font,
                                code,
                                text: &text,
                                trm,
                                fill: state.fill,
                                fill_alpha: state.fill_alpha,
                                render_mode: ts.render_mode,
                                clip: state.clip,
                            },
                        );
                    }

                    // Ligaturen (ein Code → mehrere Zeichen) werden zeichenweise
                    // aufgeteilt, damit Text und Glyphen 1:1 zusammenpassen.
                    let char_count = text.chars().count().max(1);
                    if char_count == 1 {
                        items.push(ShowItem::Glyph(GlyphItem {
                            bytes: raw_code_bytes(bytes, &font, code, nbytes),
                            text,
                            rect,
                            origin,
                            displacement,
                            baseline,
                        }));
                    } else {
                        let bytes_for_code = raw_code_bytes(bytes, &font, code, nbytes);
                        let width = rect.width() / char_count as f64;
                        for (i, ch) in text.chars().enumerate() {
                            let sub = Rect::new(
                                rect.ll.x + width * i as f64,
                                rect.ll.y,
                                rect.ll.x + width * (i + 1) as f64,
                                rect.ur.y,
                            );
                            items.push(ShowItem::Glyph(GlyphItem {
                                // Nur das erste Teilzeichen trägt die Originalbytes,
                                // damit der Code beim Neuschreiben nicht dupliziert wird.
                                bytes: if i == 0 {
                                    bytes_for_code.clone()
                                } else {
                                    Vec::new()
                                },
                                text: ch.to_string(),
                                rect: sub,
                                origin,
                                displacement: if i == 0 { displacement } else { 0.0 },
                                baseline: Baseline {
                                    advance: if i == 0 { baseline.advance } else { 0.0 },
                                    ..baseline
                                },
                            }));
                        }
                    }

                    *tm = Matrix::translate(displacement, 0.0).mul(tm);
                }
            }
            Object::Integer(_) | Object::Real(_) => {
                let adj = as_f64(element).unwrap_or(0.0);
                let tx = -adj / 1000.0 * ts.font_size * ts.h_scale;
                *tm = Matrix::translate(tx, 0.0).mul(tm);
                items.push(ShowItem::Adjust(adj));
            }
            _ => {}
        }
    }

    Some(ShowRecord {
        stream: cx.stream,
        op_index: cx.op_index,
        operator: operator.to_string(),
        operands: operands.to_vec(),
        font_size: ts.font_size,
        h_scale: ts.h_scale,
        items,
    })
}

/// Liefert die Originalbytes eines Codes.
fn raw_code_bytes(_source: &[u8], _font: &FontInfo, code: u32, nbytes: usize) -> Vec<u8> {
    match nbytes {
        2 => vec![(code >> 8) as u8, (code & 0xFF) as u8],
        _ => vec![(code & 0xFF) as u8],
    }
}

/// Rechnet die Grundlinien-Geometrie einer Glyphe in den User-Space.
///
/// `text_to_user` ist `Tm × CTM`, bildet also den Textraum ab. Die
/// Schreibrichtung ist das Bild der Textraum-x-Achse, die Zeilenhöhe wird
/// senkrecht dazu gemessen — nur so bleibt beides bei gedrehtem Text richtig.
fn baseline_of(
    text_to_user: &Matrix,
    displacement: f64,
    em_height: f64,
    space_width: f64,
) -> Baseline {
    let along = (text_to_user.a, text_to_user.b);
    let scale_x = along.0.hypot(along.1);
    let scale_y = text_to_user.c.hypot(text_to_user.d);
    if scale_x < 1e-12 {
        return Baseline {
            height: em_height.abs() * scale_y,
            ..Baseline::default()
        };
    }
    Baseline {
        direction: Point::new(along.0 / scale_x, along.1 / scale_x),
        advance: displacement * scale_x,
        height: em_height.abs() * scale_y,
        space_width: space_width * scale_x,
    }
}

/// Bounding-Box eines Glyphen: die vier Ecken werden transformiert, daraus
/// wird die achsenparallele Hülle gebildet (funktioniert auch bei Rotation).
fn glyph_rect(trm: &Matrix, width: f64, ascent: f64, descent: f64) -> Rect {
    let corners = [
        trm.apply(0.0, descent),
        trm.apply(width, descent),
        trm.apply(width, ascent),
        trm.apply(0.0, ascent),
    ];
    let mut min = Point::new(f64::MAX, f64::MAX);
    let mut max = Point::new(f64::MIN, f64::MIN);
    for c in corners {
        min.x = min.x.min(c.x);
        min.y = min.y.min(c.y);
        max.x = max.x.max(c.x);
        max.y = max.y.max(c.y);
    }
    Rect { ll: min, ur: max }
}

fn matrix_from(operands: &[Object]) -> Option<Matrix> {
    if operands.len() < 6 {
        return None;
    }
    Some(Matrix::new(
        as_f64(&operands[0])?,
        as_f64(&operands[1])?,
        as_f64(&operands[2])?,
        as_f64(&operands[3])?,
        as_f64(&operands[4])?,
        as_f64(&operands[5])?,
    ))
}

/// Ermittelt die CTM am Ende des Streams auf Stapel-Ebene 0 sowie die Zahl
/// nicht geschlossener `q`-Operationen. Beides wird gebraucht, um die
/// Deck-Rechtecke anschließend im unveränderten User-Space zu zeichnen.
pub fn trailing_state(operations: &[Operation]) -> (Matrix, usize) {
    let mut base_ctm = Matrix::IDENTITY;
    let mut depth = 0usize;
    for op in operations {
        match op.operator.as_str() {
            "q" => depth += 1,
            "Q" => depth = depth.saturating_sub(1),
            "cm" if depth == 0 => {
                if let Some(m) = matrix_from(&op.operands) {
                    base_ctm = m.mul(&base_ctm);
                }
            }
            _ => {}
        }
    }
    (base_ctm, depth)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::content::Content;
    use lopdf::{dictionary, Stream, StringFormat};

    fn ops(src: &[u8]) -> Vec<Operation> {
        Content::decode(src).unwrap().operations
    }

    /// Baut ein einseitiges PDF um ein beliebiges Font-Dictionary (`/F1`).
    ///
    /// Das Font-Dictionary wird erst gebaut, wenn das Dokument existiert —
    /// so kann es auf eigene Objekte (etwa eine `/ToUnicode`-CMap) verweisen.
    fn page_with_font(
        font: impl FnOnce(&mut Document) -> Dictionary,
        content: Vec<u8>,
    ) -> (Document, ObjectId) {
        let mut doc = Document::with_version("1.5");
        let font = font(&mut doc);
        let font_id = doc.add_object(font);
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let content_id = doc.add_object(Stream::new(dictionary! {}, content));
        let pages_id = doc.new_object_id();
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1_i64,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        (doc, page_id)
    }

    /// Identity-H-Subset, dessen CIDs bei 1 durchnummeriert sind.
    fn identity_subset_font(with_to_unicode: bool) -> impl FnOnce(&mut Document) -> Dictionary {
        move |doc: &mut Document| {
            let mut font = dictionary! {
                "Type" => "Font",
                "Subtype" => "Type0",
                "BaseFont" => "ABCDEF+Arial",
                "Encoding" => "Identity-H",
            };
            if with_to_unicode {
                let cmap = b"/CIDInit /ProcSet findresource begin
1 begincodespacerange <0000> <FFFF> endcodespacerange
1 beginbfrange <0001> <0016> <0041> endbfrange
endcmap"
                    .to_vec();
                let id = doc.add_object(Stream::new(dictionary! {}, cmap));
                font.set("ToUnicode", Object::Reference(id));
            }
            font
        }
    }

    /// `BT /F1 10 Tf … Tj ET` mit CIDs 1..=n als Zweibyte-Codes.
    fn identity_content(cids: &[u16]) -> Vec<u8> {
        let bytes: Vec<u8> = cids.iter().flat_map(|c| c.to_be_bytes()).collect();
        Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), Object::Real(10.0)]),
                Operation::new(
                    "Tm",
                    vec![
                        1.into(),
                        0.into(),
                        0.into(),
                        1.into(),
                        Object::Real(72.0),
                        Object::Real(700.0),
                    ],
                ),
                Operation::new("Tj", vec![Object::String(bytes, StringFormat::Hexadecimal)]),
                Operation::new("ET", vec![]),
            ],
        }
        .encode()
        .unwrap()
    }

    #[test]
    fn tracks_unbalanced_q_and_base_ctm() {
        let (ctm, depth) = trailing_state(&ops(b"q 1 0 0 1 5 5 cm Q 2 0 0 2 0 0 cm q q"));
        assert_eq!(depth, 2);
        assert_eq!(ctm, Matrix::scale(2.0, 2.0));
    }

    #[test]
    fn glyph_rect_is_axis_aligned_hull() {
        // 90°-Drehung: Breite und Höhe tauschen die Plätze.
        let rot = Matrix::new(0.0, 1.0, -1.0, 0.0, 0.0, 0.0);
        let trm = Matrix::scale(10.0, 10.0).mul(&rot);
        let r = glyph_rect(&trm, 0.5, 0.75, -0.25);
        assert!((r.width() - 10.0).abs() < 1e-9);
        assert!((r.height() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn appearance_is_fitted_into_the_annotation_rect() {
        // `/BBox` 100 × 50, um 90° gedreht (Hülle also 50 × 100), soll in ein
        // `/Rect` von 100 × 200 passen: Faktor 2 in beiden Achsen. Ohne diese
        // Abbildung läge der Annotationstext im Ursprung des Formularraums —
        // also irgendwo, nur nicht dort, wo er zu sehen ist.
        let rotate = Matrix::new(0.0, 1.0, -1.0, 0.0, 0.0, 0.0);
        let m = appearance_matrix(
            &rotate,
            Some(Rect::new(0.0, 0.0, 100.0, 50.0)),
            Some(Rect::new(10.0, 20.0, 110.0, 220.0)),
        );
        let a = m.apply(0.0, 0.0);
        let b = m.apply(100.0, 50.0);
        assert!(
            (a.x - 110.0).abs() < 1e-9 && (a.y - 20.0).abs() < 1e-9,
            "{a:?}"
        );
        assert!(
            (b.x - 10.0).abs() < 1e-9 && (b.y - 220.0).abs() < 1e-9,
            "{b:?}"
        );
    }

    #[test]
    fn an_appearance_without_bbox_keeps_its_own_matrix() {
        let m = Matrix::translate(5.0, 7.0);
        assert_eq!(
            appearance_matrix(&m, None, Some(Rect::new(0.0, 0.0, 10.0, 10.0))),
            m
        );
    }

    // -----------------------------------------------------------------------
    // K3 — Identity-H ohne /ToUnicode darf nicht still Müll liefern
    // -----------------------------------------------------------------------

    #[test]
    fn identity_font_without_to_unicode_warns_loudly() {
        let cids: Vec<u16> = (1..=22).collect();
        let (doc, page_id) = page_with_font(identity_subset_font(false), identity_content(&cids));
        let scan = scan_page(&doc, page_id).expect("Scan");

        // Vorbedingung: der dekodierte Text ist tatsächlich unbrauchbar.
        let text: String = scan
            .shows
            .iter()
            .flat_map(|s| s.glyphs())
            .map(|g| g.text.as_str())
            .collect();
        assert!(
            text.chars().filter(|c| *c == '\u{FFFD}').count() * 2 > text.chars().count(),
            "Testdaten taugen nicht, der Text ist lesbar: {text:?}"
        );

        assert!(
            scan.warnings.iter().any(|w| w.contains("ToUnicode")),
            "keine Warnung trotz undekodierbarem Font: {:?}",
            scan.warnings
        );
    }

    #[test]
    fn identity_font_with_to_unicode_stays_quiet() {
        let cids: Vec<u16> = (1..=22).collect();
        let (doc, page_id) = page_with_font(identity_subset_font(true), identity_content(&cids));
        let scan = scan_page(&doc, page_id).expect("Scan");
        assert!(
            scan.warnings.is_empty(),
            "unerwartete Warnung: {:?}",
            scan.warnings
        );
    }

    #[test]
    fn an_explicit_dw_drives_the_pen_not_the_font_name() {
        // Type0-Font mit `/DW 600` und `/ToUnicode`: die Namensschätzung
        // („Helvetica“, Ziffern 0,556) darf den Vorschub nicht bestimmen,
        // sonst läuft der Stift pro Zeichen um 0,44 pt voraus und die
        // x-Sortierung der Extraktion vertauscht Glyphen.
        let font = |doc: &mut Document| {
            let cmap = b"/CIDInit /ProcSet findresource begin
1 begincodespacerange <0000> <FFFF> endcodespacerange
1 beginbfrange <0001> <000A> <0030> endbfrange
endcmap"
                .to_vec();
            let to_unicode = doc.add_object(Stream::new(dictionary! {}, cmap));
            let descendant = doc.add_object(dictionary! {
                "Type" => "Font",
                "Subtype" => "CIDFontType2",
                "BaseFont" => "ABCDEF+Helvetica",
                "DW" => 600,
            });
            dictionary! {
                "Type" => "Font",
                "Subtype" => "Type0",
                "BaseFont" => "ABCDEF+Helvetica",
                "Encoding" => "Identity-H",
                "DescendantFonts" => vec![Object::Reference(descendant)],
                "ToUnicode" => Object::Reference(to_unicode),
            }
        };
        let (doc, page_id) = page_with_font(font, identity_content(&[1, 2, 3, 4]));
        let scan = scan_page(&doc, page_id).expect("Scan");
        let glyphs: Vec<_> = scan.shows.iter().flat_map(|s| s.glyphs()).collect();
        assert_eq!(glyphs.len(), 4);
        assert_eq!(
            glyphs.iter().map(|g| g.text.as_str()).collect::<String>(),
            "0123"
        );
        for (i, g) in glyphs.iter().enumerate() {
            // 10 pt × 0,6 em = 6,0 pt je Zeichen.
            assert!(
                (g.origin.x - (72.0 + 6.0 * i as f64)).abs() < 1e-6,
                "Glyphe {i} steht bei {}, erwartet {}",
                g.origin.x,
                72.0 + 6.0 * i as f64
            );
        }
    }

    #[test]
    fn a_plain_win_ansi_font_produces_no_warning() {
        let font = |_: &mut Document| {
            dictionary! {
                "Type" => "Font",
                "Subtype" => "Type1",
                "BaseFont" => "Helvetica",
                "Encoding" => "WinAnsiEncoding",
            }
        };
        let content = b"BT /F1 10 Tf 1 0 0 1 72 700 Tm (Kontonummer 4711000) Tj ET".to_vec();
        let (doc, page_id) = page_with_font(font, content);
        let scan = scan_page(&doc, page_id).expect("Scan");
        assert!(
            scan.warnings.is_empty(),
            "unerwartete Warnung: {:?}",
            scan.warnings
        );
    }

    // -----------------------------------------------------------------------
    // Geltungsbereich der Textspiegel
    // -----------------------------------------------------------------------

    /// Scannt einen Rohstrom ohne Fonts und liefert die Spiegel mit ihrem
    /// Geltungsbereich.
    fn mirrors(src: &[u8]) -> Vec<(usize, Vec<usize>)> {
        let doc = Document::with_version("1.5");
        let mut result = ScanResult::default();
        scan_marked_text(&doc, &ops(src), StreamKey::Page, None, &mut result);
        result
            .marked
            .iter()
            .map(|m| (m.op_index, m.shows.clone()))
            .collect()
    }

    #[test]
    fn a_bracket_owns_exactly_the_glyphs_it_encloses() {
        // 0:BT 1:Tj 2:BDC 3:Tj 4:EMC 5:Tj 6:ET
        let found = mirrors(b"BT (a) Tj /Span <</ActualText (b)>> BDC (b) Tj EMC (c) Tj ET");
        assert_eq!(found, vec![(2, vec![3])]);
    }

    #[test]
    fn a_bracket_without_a_mirror_is_not_reported() {
        let found = mirrors(b"BT /Span <</MCID 0>> BDC (b) Tj EMC ET");
        assert!(found.is_empty(), "{found:?}");
    }

    /// Ein `/ActualText`, das nur aus Leerraum besteht, spiegelt nichts.
    #[test]
    fn an_empty_mirror_is_not_reported() {
        let found = mirrors(b"BT /Span <</ActualText ( )>> BDC (b) Tj EMC ET");
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_point_inherits_the_enclosing_bracket() {
        // 0:BT 1:BDC 2:Tj 3:DP 4:Tj 5:EMC 6:Tj 7:ET
        let found = mirrors(
            b"BT /Span <</MCID 0>> BDC (a) Tj /Span <</ActualText (x)>> DP (b) Tj EMC (c) Tj ET",
        );
        assert_eq!(found, vec![(3, vec![2, 4])]);
    }

    /// Ohne umschließende Klammer gilt das Textobjekt — und zwar ganz, auch
    /// was **vor** dem Punkt gesetzt wurde: ein Punkt beschreibt die Stelle, an
    /// der er steht, nicht einen Bereich dahinter.
    #[test]
    fn a_point_falls_back_to_the_text_object() {
        // 0:BT 1:Tj 2:DP 3:Tj 4:ET 5:BT 6:Tj 7:ET
        let found = mirrors(b"BT (a) Tj /Span <</ActualText (x)>> DP (b) Tj ET BT (c) Tj ET");
        assert_eq!(found, vec![(2, vec![1, 3])]);
    }

    /// Außerhalb jedes Textobjekts bleibt nur der ganze Strom. Das ist die
    /// sichere Richtung: lieber ein Spiegel zu viel entfernt als einer, der
    /// weiterhin das Geheimnis nennt.
    #[test]
    fn a_point_outside_any_text_object_covers_the_whole_stream() {
        let found = mirrors(b"/Span <</ActualText (x)>> DP BT (a) Tj ET BT (b) Tj ET");
        assert_eq!(found, vec![(0, vec![2, 5])]);
    }

    /// Eine nicht geschlossene Klammer reicht bis zum Stromende.
    #[test]
    fn an_unclosed_bracket_reaches_to_the_end() {
        let found = mirrors(b"BT /Span <</ActualText (x)>> BDC (a) Tj (b) Tj ET");
        assert_eq!(found, vec![(1, vec![2, 3])]);
    }
}
