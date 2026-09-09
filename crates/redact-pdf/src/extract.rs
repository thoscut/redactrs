//! Textextraktion: aus Glyphen werden Zeilen.
//!
//! Ein PDF kennt keine Zeilen — nur einzeln positionierte Glyphen. Für das
//! Pattern-Matching ist die Zeile aber die entscheidende Einheit: eine IBAN
//! wird in der Praxis über mehrere `Tj`-Operationen verteilt ausgegeben
//! (`DE89 `, `3704 `, `0044 …`). Wer nur einzelne Text-Runs betrachtet, findet
//! sie nicht. Deshalb werden hier alle Glyphen einer Seite eingesammelt,
//! nach Grundlinie gruppiert und zu Zeilen zusammengesetzt.
//!
//! ## Warum eine Zeile mehrere Druckschichten haben kann
//!
//! Das Verschmelzen über alle Textoperationen hinweg ist gewollt — eine
//! Tabellenzeile besteht aus vielen `Tj`-Aufrufen und muss **eine** Zeile
//! ergeben. Es hat aber eine Grenze: liegen zwei Texte auf derselben
//! Grundlinie *übereinander*, verschränkt das Verschmelzen sie zeichenweise
//! (`IIBBAANN::  DDEE8899 …`) und kein Muster greift mehr. Das ist kein
//! Sonderfall, sondern das gängige Fett-Imitat (dieselbe Zeile zweimal
//! gedruckt), der Schlagschatten und jede Annotation mit mehreren
//! Erscheinungszuständen auf demselben `/Rect`.
//!
//! Die Unterscheidung ist **nicht** die Herkunft (nach `ShowRecord` zu trennen
//! zerlegte jede Tabellenzeile), sondern die Geometrie: nebeneinander gesetzter
//! Text *kachelt* die Grundlinie — jede Druckfolge beginnt dort, wo die vorige
//! aufhört —, übereinander gedruckter Text *überdeckt* sie. Die Zeile wird
//! deshalb in [`split_layers`] in Druckschichten zerlegt, und jede Schicht
//! ergibt ihre eigene Zeile.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use lopdf::{Document, Object, ObjectId};
use redact_core::{bounding_box, Glyph, Rect, Result, TextRun};

use crate::content::{
    scan_page, GlyphItem, MarkedTextRecord, ScanResult, ShowRecord, StreamKey, MIRROR_KEYS,
};

/// Auflösung der Richtungs-Einteilung in Grad. Glyphen mit gleicher gerundeter
/// Grundlinienrichtung kommen in dieselbe Zeile; ein 90°-Block bleibt also von
/// waagerechtem Text getrennt.
const DIRECTION_BUCKET_DEGREES: f64 = 1.0;

/// Anteil der *kürzeren* der beiden Druckfolgen, den eine Überlappung erreichen
/// muss, bevor sie als eigene Druckschicht gilt.
///
/// Ein Wert unterhalb davon ist Feinsatz: ein Kerningpaar, eine negative
/// Laufweite, eine Zelle, deren Text ein Stück in die nächste ragt. Ein Wert
/// darüber heißt, dass die eine Folge die andere über weite Strecken überdeckt
/// — und zwei Texte übereinander sind zwei Zeilen, keine.
const OVERPRINT_RATIO: f64 = 0.5;

/// Untergrenze derselben Prüfung, als Anteil der Leerzeichenbreite.
///
/// Sie fängt sehr kurze Druckfolgen ab (eine einzelne Glyphe), bei denen der
/// Anteil oben in den Bereich der Rundungs- und Metrikunschärfe fiele. Ohne
/// sie zerfiele eine Zeile, deren Metriken nicht exakt zu den gesetzten
/// Positionen passen, in lauter Schichten.
const OVERPRINT_FLOOR_IN_SPACES: f64 = 0.5;

/// Wie viele Druckschichten eine Zeile höchstens bekommt.
///
/// Die Zuordnung sucht linear über die bereits belegten Schichten; ohne
/// Obergrenze machte eine Datei, die tausend Textstücke an dieselbe Stelle
/// druckt, daraus quadratischen Aufwand. Ein Fett-Imitat hat zwei Schichten,
/// ein Schlagschatten zwei, eine Checkbox mit allen Zuständen eine Handvoll —
/// 64 liegt weit jenseits von allem, was ein Satzprogramm erzeugt.
const MAX_PRINT_LAYERS: usize = 64;

/// Wie viele ungezeichnete Formulare höchstens einzeln geöffnet werden.
///
/// Die Prüfung „steht da überhaupt Text?“ dekodiert einen Strom. Eine Datei
/// mit zehntausend Karteileichen im Ressourcenverzeichnis dürfte damit nicht
/// den Lauf aufhalten; jenseits der Grenze wird zusammengefasst gemeldet.
const MAX_INSPECTED_UNPLACED_FORMS: usize = 64;

/// Toleranz (in Punkt), innerhalb derer Glyphen zur selben Zeile zählen.
const BASELINE_TOLERANCE: f64 = 2.0;

/// Ab welchem Anteil der **Leerzeichenbreite des Fonts** eine über den
/// natürlichen Vorschub hinausgehende Lücke als Leerzeichen gilt.
const SPACE_RATIO: f64 = 0.5;

/// Extrahiert Textzeilen mit zeichengenauen Koordinaten.
///
/// Ohne Zustand: die beiden Maße der Zeilenbildung stehen als
/// [`BASELINE_TOLERANCE`] und [`SPACE_RATIO`] fest. Sie waren einmal
/// öffentliche Felder — verstellt hat sie nie jemand, weder ein Schalter noch
/// die Oberfläche noch ein Test, alle Konstruktionsstellen im Arbeitsbereich
/// lauten `new()` bzw. `default()`. Ein Feld, das nur einen Wert annimmt, ist
/// keine Einstellung, sondern eine Zusage, die niemand einlöst; die *Werte*
/// bleiben von den Tests gedeckt.
#[derive(Debug, Clone, Copy, Default)]
pub struct PdfExtractor;

impl PdfExtractor {
    pub fn new() -> Self {
        Self
    }

    /// Wie [`PdfExtractor::extract`], liefert aber zusätzlich die Warnungen des
    /// Interpreters.
    ///
    /// Wichtig ist vor allem der Fall „Font ohne `/ToUnicode`“: dort steht
    /// zwar Text auf der Seite, er lässt sich aber nicht dekodieren. Die
    /// Analyse findet dann nichts, die Schwärzung meldet Erfolg — und der
    /// Nutzer hält eine Datei für sauber, in der alles stehen geblieben ist.
    /// Diese Warnungen dürfen deshalb nicht im Extraktor versanden.
    ///
    /// Eine Seite, die der Interpreter ablehnt, kippt das **ganze** Dokument:
    /// die Schwärzung darf keine Datei ausgeben, deren Text sie nicht
    /// vollständig gesehen hat.
    pub fn extract_with_warnings(&self, doc: &Document) -> Result<(Vec<TextRun>, Vec<String>)> {
        self.extract_pages(doc, false)
    }

    /// Wie [`PdfExtractor::extract_with_warnings`], nur **nachsichtig**: eine
    /// Seite, die der Interpreter ablehnt, wird übersprungen und als Warnung
    /// genannt; die übrigen Seiten kommen zurück.
    ///
    /// Das ist der Weg des ehrlichen Orakels (`crate::audit_bytes`), das
    /// jede Seite so lesen will, wie der Schriftdekoder sie liest — und dem
    /// eine kaputte Seite 1 nicht die Seiten 2 bis n nehmen darf. Für die
    /// Schwärzung wäre dieselbe Nachsicht ein Leck; sie nimmt
    /// [`PdfExtractor::extract_with_warnings`].
    ///
    /// **Ein** Durchgang, **ein** Seitenbaum: früher fiel das Orakel nach
    /// einem Fehler auf eine Schleife über eine seitenweise Extraktion
    /// zurück, und die baute je Aufruf den Seitenbaum neu (`get_pages()`,
    /// eine frische `BTreeMap` über alle Seiten) — quadratisch in der
    /// Seitenzahl. Jetzt teilen sich beide Wege dieselbe Schleife in
    /// [`PdfExtractor::extract_pages`]; die Seitenschleife um einen
    /// seitenweisen Aufruf gibt es nicht mehr, und damit lässt sie sich auch
    /// nicht mehr schreiben.
    pub fn extract_lenient(&self, doc: &Document) -> (Vec<TextRun>, Vec<String>) {
        // `lenient` liefert nie `Err` — der Rückfall steht nur der
        // Signatur wegen da.
        self.extract_pages(doc, true).unwrap_or_default()
    }

    /// Die eine Seitenschleife hinter [`PdfExtractor::extract_with_warnings`]
    /// und [`PdfExtractor::extract_lenient`]; `lenient` entscheidet, was
    /// eine abgelehnte Seite bewirkt: Abbruch oder Warnung.
    fn extract_pages(&self, doc: &Document, lenient: bool) -> Result<(Vec<TextRun>, Vec<String>)> {
        let mut runs = Vec::new();
        let mut warnings: Vec<String> = Vec::new();
        // Angeboten und gezeichnet — über **alle** Seiten hinweg, siehe
        // [`unplaced_form_warnings`].
        let mut declared: BTreeMap<ObjectId, Vec<u8>> = BTreeMap::new();
        let mut placed: BTreeSet<ObjectId> = BTreeSet::new();
        for (index, (_, page_id)) in doc.get_pages().iter().enumerate() {
            let scan = match scan_page(doc, *page_id) {
                Ok(scan) => scan,
                Err(e) if lenient => {
                    warnings.push(format!(
                        "Seite {} ließ sich nicht lesen und fehlt in dieser Sicht: {e}",
                        index + 1
                    ));
                    continue;
                }
                Err(e) => return Err(e),
            };
            for warning in &scan.warnings {
                if !warnings.contains(warning) {
                    warnings.push(warning.clone());
                }
            }
            for (id, name) in &scan.declared_forms {
                declared.entry(*id).or_insert_with(|| name.clone());
            }
            placed.extend(scan.form_placements.keys().copied());
            runs.extend(Self::build_lines(index, glyph_items(&scan)));
            let (mirrors, mirror_warnings) = mirror_runs(doc, index, &scan);
            runs.extend(mirrors);
            for warning in mirror_warnings {
                if !warnings.contains(&warning) {
                    warnings.push(warning);
                }
            }
        }
        for warning in unplaced_form_warnings(doc, &declared, &placed) {
            if !warnings.contains(&warning) {
                warnings.push(warning);
            }
        }
        Ok((runs, warnings))
    }

    /// Setzt aus einzelnen Glyphen Zeilen zusammen.
    ///
    /// Gruppiert wird **entlang der Grundlinie**, nicht entlang der
    /// User-Space-Y-Achse: bei gedrehtem Text wandert der Zeilenursprung sonst
    /// in `y`, und jede einzelne Glyphe landet in einer eigenen „Zeile“ —
    /// womit kein Muster mehr über mehr als ein Zeichen greift.
    ///
    /// Jede Glyphe kommt mit der laufenden Nummer ihrer Textoperation
    /// ([`glyph_items`]). Die entscheidet **nicht** über die Zeilenbildung —
    /// sie hält nur fest, welche Glyphen in *einem Zug* gesetzt wurden, und
    /// daraus werden in [`print_runs`] die Druckfolgen.
    fn build_lines(page: usize, items: Vec<(usize, GlyphItem)>) -> Vec<TextRun> {
        // Codes ohne Textzuordnung fliegen raus, Ersatzzeichen bleiben erhalten:
        // sie halten die Position und verhindern falsche Zusammenschreibung.
        let items: Vec<(usize, GlyphItem)> = items
            .into_iter()
            .filter(|(_, g)| !g.text.is_empty())
            .collect();
        if items.is_empty() {
            return Vec::new();
        }

        // Die Druckfolgen müssen **vor** dem Sortieren bestimmt werden: sie
        // ergeben sich aus der Reihenfolge, in der die Glyphen gesetzt wurden.
        let runs = print_runs(&items);
        let mut glyphs: Vec<(usize, GlyphItem)> = items
            .into_iter()
            .zip(runs)
            .map(|((_, g), run)| (run, g))
            .collect();

        // Leserichtung herstellen: Zeilen in Vorschubrichtung, innerhalb der
        // Zeile in Schreibrichtung. Bei waagerechtem Text ist das genau
        // „von oben nach unten, dann von links nach rechts“. Die
        // Quantisierung des Zeilenabstands fängt kleine
        // Grundlinien-Schwankungen ab.
        let tol = BASELINE_TOLERANCE.max(0.1);
        glyphs.sort_by(|(_, a), (_, b)| key_cmp(&sort_key(a, tol), &sort_key(b, tol)));

        let mut lines: Vec<Vec<(usize, GlyphItem)>> = Vec::new();
        let mut current: Vec<(usize, GlyphItem)> = Vec::new();
        // Bezugswerte der laufenden Zeile. Die Toleranz gehört zur **Zeile**,
        // nicht zur gerade betrachteten Glyphe — sonst entscheidet die
        // Reihenfolge über die Gruppierung, und eine große Überschrift zieht
        // die kleine Zeile darüber in sich hinein.
        let mut line_direction = 0i64;
        let mut line_across = 0.0f64;
        let mut line_tol = tol;

        for (run, g) in glyphs {
            let direction = direction_bucket(&g);
            let across = across_of(&g);
            let same_line = !current.is_empty()
                && direction == line_direction
                && (across - line_across).abs() <= line_tol;
            if !same_line && !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            if current.is_empty() {
                line_direction = direction;
                line_across = across;
                // Höhe senkrecht zur Grundlinie; nur wenn die fehlt, muss die
                // achsenparallele Hülle herhalten.
                let height = if g.baseline.height.abs() > 1e-6 {
                    g.baseline.height.abs()
                } else {
                    g.rect.height().abs()
                };
                line_tol = (height * 0.45).max(tol);
            }
            current.push((run, g));
        }
        if !current.is_empty() {
            lines.push(current);
        }

        lines
            .into_iter()
            .flat_map(split_layers)
            .filter_map(|layer| Self::assemble_line(page, layer))
            .collect()
    }

    /// Baut eine Zeile: Glyphen in Leserichtung, Lücken werden zu Leerzeichen.
    ///
    /// Als Lücke zählt nur, was **über den ohnehin gesetzten Vorschub
    /// hinausgeht**. Eine Laufweite (`Tc`) steckt bereits im Vorschub jeder
    /// Glyphe; würde man wie früher den Abstand der Glyphenkästen messen,
    /// stünde ab `Tc > 1,4 pt` zwischen jedem Zeichen ein Leerzeichen — und
    /// eine gesperrt gesetzte IBAN wäre nicht mehr zu erkennen.
    fn assemble_line(page: usize, items: Vec<GlyphItem>) -> Option<TextRun> {
        if items.is_empty() {
            return None;
        }
        let dir = items[0].baseline.direction;
        // Ersatzmaß, falls der Font keine Leerzeichenbreite hergibt: ein
        // Leerzeichen ist grob halb so breit wie eine mittlere Glyphe.
        let assumed_space = {
            let widths: Vec<f64> = items
                .iter()
                .map(|g| g.rect.width().abs().max(g.baseline.advance.abs()))
                .filter(|w| *w > 0.0)
                .collect();
            if widths.is_empty() {
                1.0
            } else {
                widths.iter().sum::<f64>() / widths.len() as f64 * 0.5
            }
        };
        let space_width_of = |g: &GlyphItem| {
            if g.baseline.space_width.abs() > 1e-6 {
                g.baseline.space_width.abs()
            } else {
                assumed_space
            }
        };

        // Überschuss je Glyphenpaar: der zurückgelegte Weg entlang der
        // Grundlinie abzüglich dessen, was der Vorschub ohnehin erklärt.
        let extras: Vec<f64> = items
            .windows(2)
            .map(|pair| {
                let (p, next) = (&pair[0], &pair[1]);
                let travelled =
                    (next.origin.x - p.origin.x) * dir.x + (next.origin.y - p.origin.y) * dir.y;
                travelled - p.baseline.advance
            })
            .collect();
        let pitch = line_pitch(&extras, space_width_of(&items[0]));

        let mut glyphs: Vec<Glyph> = Vec::with_capacity(items.len() + 8);

        for (index, item) in items.iter().enumerate() {
            if index > 0 {
                let p = &items[index - 1];
                let extra = extras[index - 1];
                // Ein Leerzeichen liegt vor, wenn der Überschuss — nach Abzug
                // eines etwaigen Rastervorschubs — mindestens die halbe
                // Leerzeichenbreite dieses Fonts erreicht.
                let threshold = space_width_of(p) * SPACE_RATIO;
                let last_is_space = glyphs.last().map(|g| g.ch == ' ').unwrap_or(true);
                if extra - pitch > threshold && !last_is_space {
                    glyphs.push(Glyph {
                        ch: ' ',
                        rect: Rect::from_corners(p.rect.ur, item.rect.ll),
                    });
                }
            }
            for ch in item.text.chars() {
                glyphs.push(Glyph {
                    ch,
                    rect: item.rect,
                });
            }
        }

        // Führende/abschließende Leerzeichen entfernen.
        while glyphs.first().map(|g| g.ch == ' ').unwrap_or(false) {
            glyphs.remove(0);
        }
        while glyphs.last().map(|g| g.ch == ' ').unwrap_or(false) {
            glyphs.pop();
        }
        if glyphs.is_empty() {
            return None;
        }

        Some(TextRun::new(page, glyphs))
    }
}

impl PdfExtractor {
    /// Extrahiert die Textzeilen des ganzen Dokuments.
    ///
    /// Verwirft die Warnungen des Interpreters. Wer wissen will, ob eine
    /// Seite stillschweigend uebergangen wurde — und das will die Kette —,
    /// nimmt [`PdfExtractor::extract_with_warnings`].
    pub fn extract(&self, doc: &Document) -> Result<Vec<TextRun>> {
        Ok(self.extract_with_warnings(doc)?.0)
    }
}

// ---------------------------------------------------------------------------
// Druckfolgen und Druckschichten
// ---------------------------------------------------------------------------

/// Die Glyphen eines Seiten-Scans, jede mit der laufenden Nummer ihrer
/// Textoperation.
///
/// Die Nummer zählt **Platzierungen**, nicht Operationen im Strom: ein
/// zweimal gezeichnetes Form-XObject liefert seine Textoperationen zweimal,
/// und die beiden Durchgänge sind zwei verschiedene Druckvorgänge an zwei
/// Stellen der Seite.
fn glyph_items(scan: &ScanResult) -> Vec<(usize, GlyphItem)> {
    scan.shows
        .iter()
        .enumerate()
        .flat_map(|(index, show)| show.glyphs().cloned().map(move |g| (index, g)))
        .collect()
}

/// Textspiegel, die etwas **anderes** sagen als die Glyphen darunter — als
/// eigene Zeilen, samt Warnung je Abschnitt.
///
/// Ein `/ActualText` (oder `/Alt`, `/E`) ist Text, den ein Betrachter ausgibt:
/// `pdftotext` bevorzugt ihn in der Voreinstellung, Kopieren im Betrachter
/// liefert ihn. Bis 0.6.0 wurde er nur **geleert**, wenn die Schwärzung die
/// Glyphen darunter traf, und nie **gelesen**. Gemessen an zwei Dateien:
///
/// * `/Span <</ActualText (IBAN: DE89 …)>> BDC (Kontodaten folgen unten) Tj EMC`
///   — 0 Treffer, Rückgabewert 0, und Kopieren im Betrachter lieferte die
///   IBAN aus der „geschwärzten“ Datei.
/// * Dasselbe mit `/Alt` — gleiches Ergebnis.
///
/// **Deckungsgleich** heißt: nach [`fold`] gleich. Word, InDesign und jeder
/// PDF/UA-Erzeuger schreiben Spiegel routinemäßig — für Ligaturen, weiche
/// Trennstriche, Tabulatoren, Sonderzeichen. Die dürfen keine Warnung geben,
/// sonst warnt jede getaggte Datei. Ein Spiegel aus **einem** Zeichen sagt für
/// sich nichts und wird ebenfalls nicht gemeldet. Mehr Ausnahmen gibt es
/// nicht: ein `½` mit `/ActualText (1/2)` wird gemeldet, denn ohne eine
/// Tabelle aller Sonderzeichen ist es von einem `½` mit `/ActualText (IBAN …)`
/// nicht zu unterscheiden — und die Warnung sagt genau das.
///
/// **Widerspricht** der Spiegel, kann das Werkzeug nicht wissen, welche der
/// beiden Fassungen ein Betrachter zeigt. Deshalb beides: der Spiegel wird als
/// eigene Zeile durchsucht (auf dem Kasten der Glyphen darunter — trifft dort
/// ein Muster, verschwinden die Glyphen und mit ihnen der Spiegel, siehe
/// `crate::redact::mirrors_to_clear`), **und** der Lauf meldet eine
/// Deckungslücke — **nur für `/ActualText`**, denn nur der Ersatz muss den
/// Glyphen gleichen (PDF 32000-1, 14.9.4). `/Alt` beschreibt und `/E`
/// schreibt aus (14.9.3, 14.9.5); beide dürfen abweichen, werden gelesen und
/// geleert wie der Ersatz, aber nicht gemeldet — sonst wäre jedes Bild mit
/// `/Figure <</Alt …>> BDC /Im0 Do EMC`, die Standardform der
/// Barrierefreiheit, ein Befund. Ein `/ActualText` ohne Glyphen darunter hat
/// keinen Kasten, bleibt bei der Warnung — und die sagt dann ausdrücklich,
/// dass **nicht** durchsucht wurde.
///
/// **Formulargrenze.** Die Glyphen unter einem Spiegel liegen nicht immer im
/// selben Strom: `/Span <</ActualText …>> BDC /Fm0 Do EMC` setzt sie über ein
/// Form-XObject. Die Textoperationen der Formulare im Geltungsbereich
/// ([`MarkedTextRecord::forms`]) zählen deshalb mit — an der Stelle ihres
/// `Do`, in Stromreihenfolge, auch über mehrere Ebenen: der Pfad der
/// `Do`-Indizes im Datensatz ordnet ein Formular, das erst ein inneres
/// Formular zeichnet und dann eigenen Text, genau so ein (Befund G1-A3: die
/// frühere Fassung stellte die eigenen Glyphen stets voran und meldete eine
/// ehrliche Datei als Widerspruch).
///
/// Die Warnung nennt keinen Text: sie steht später im Audit-Log, und dort
/// hätte der Spiegel nichts verloren.
fn mirror_runs(doc: &Document, page: usize, scan: &ScanResult) -> (Vec<TextRun>, Vec<String>) {
    let mut runs = Vec::new();
    let mut warnings = Vec::new();
    // (Strom, Operation) → Textoperation. Ein mehrfach platziertes Formular
    // liefert dieselbe Operation mehrfach; die erste Platzierung genügt, der
    // Strom wird ohnehin nur einmal neu geschrieben.
    let mut shows: HashMap<(StreamKey, usize), &ShowRecord> = HashMap::new();
    for show in &scan.shows {
        shows.entry((show.stream, show.op_index)).or_insert(show);
    }
    // Formular → seine Textoperationen in Stromreihenfolge, je Operation
    // einmal — nur für die Formulare, die unter einem Spiegel stehen.
    let mut form_shows: HashMap<ObjectId, Vec<&ShowRecord>> = scan
        .marked
        .iter()
        .flat_map(|record| record.forms.iter().map(|(_, id)| (*id, Vec::new())))
        .collect();
    if !form_shows.is_empty() {
        for show in &scan.shows {
            if let StreamKey::Form(id) = show.stream {
                if let Some(list) = form_shows.get_mut(&id) {
                    list.push(show);
                }
            }
        }
        for list in form_shows.values_mut() {
            list.sort_by_key(|show| show.op_index);
            list.dedup_by_key(|show| show.op_index);
        }
    }
    for record in &scan.marked {
        // Glyphen in Stromreihenfolge: die eigenen Textoperationen an ihrem
        // Index, die eines Formulars an der Stelle seines `Do` — als Pfad
        // `[Do-Index, …, Operationsindex]`, damit ein inneres Formular
        // zwischen die Textoperationen des äußeren fällt, an der Stelle
        // seines `Do`. Die Pfade sind untereinander verschieden, die
        // Sortierung damit eindeutig.
        let mut parts: Vec<(Vec<usize>, &ShowRecord)> = record
            .shows
            .iter()
            .filter_map(|index| {
                shows
                    .get(&(record.stream, *index))
                    .map(|show| (vec![*index], *show))
            })
            .collect();
        for (path, id) in &record.forms {
            if let Some(list) = form_shows.get(id) {
                parts.extend(list.iter().map(|show| {
                    let mut key = path.clone();
                    key.push(show.op_index);
                    (key, *show)
                }));
            }
        }
        parts.sort_by(|(a, _), (b, _)| a.cmp(b));
        let glyphs: Vec<&GlyphItem> = parts.iter().flat_map(|(_, show)| show.glyphs()).collect();
        let beneath = fold(glyphs.iter().map(|g| g.text.as_str()));
        let rect = bounding_box(glyphs.iter().map(|g| &g.rect));
        for key in MIRROR_KEYS {
            let Some(text) = mirror_text(doc, record, key) else {
                continue;
            };
            let folded = fold([text.as_str()]);
            if folded.is_empty() || folded == beneath || folded.chars().count() == 1 {
                continue;
            }
            // Nur der Ersatz muss gleichen.
            if key == b"ActualText" {
                warnings.push(format!(
                    "Der Textspiegel (/{}) eines Marked-Content-Abschnitts auf Seite {} sagt \
                     etwas anderes als die Glyphen darunter ({} Zeichen im Spiegel, {} in den \
                     Glyphen). Welche der beiden Fassungen ein Betrachter zeigt oder kopiert, \
                     kann das Werkzeug nicht wissen.{}",
                    String::from_utf8_lossy(key),
                    page + 1,
                    folded.chars().count(),
                    beneath.chars().count(),
                    if rect.is_some() {
                        " Der Spiegel wurde zusätzlich als eigener Text durchsucht; bitte das \
                         Ergebnis dort von Hand prüfen."
                    } else {
                        " Ohne Glyphen darunter hat der Spiegel keinen Kasten: er wurde nicht \
                         durchsucht und kann von diesem Werkzeug nicht geschwärzt werden."
                    }
                ));
            }
            if let Some(rect) = rect {
                runs.push(spread(page, &text, rect));
            }
        }
    }
    (runs, warnings)
}

/// Der Spiegeltext unter `key` — auch, wenn er als Verweis in der Liste steht.
fn mirror_text(doc: &Document, record: &MarkedTextRecord, key: &[u8]) -> Option<String> {
    let value = record.properties.get(key).ok()?;
    match doc.dereference(value).ok()?.1 {
        Object::String(bytes, _) => Some(crate::audit_bytes::decode_pdf_string(bytes)),
        _ => None,
    }
}

/// Die Vergleichsform eines Textes: ohne Leerraum, ohne unsichtbare Zeichen,
/// Ligaturen aufgelöst.
///
/// Genau die Unterschiede, die ein ehrlicher Spiegel hat: `/ActualText (fi)`
/// über einer `ﬁ`-Ligatur, `<FEFF00AD>` (weicher Trennstrich) über einem
/// Bindestrich am Zeilenende, `<FEFF0009>` (Tabulator) über einem Leerraum.
fn fold<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
    let mut out = String::new();
    for part in parts {
        for c in part.chars() {
            match c {
                c if c.is_whitespace() => {}
                '\u{00AD}' | '\u{200B}'..='\u{200D}' | '\u{2060}' | '\u{FEFF}' => {}
                '\u{FB00}' => out.push_str("ff"),
                '\u{FB01}' => out.push_str("fi"),
                '\u{FB02}' => out.push_str("fl"),
                '\u{FB03}' => out.push_str("ffi"),
                '\u{FB04}' => out.push_str("ffl"),
                '\u{FB05}' | '\u{FB06}' => out.push_str("st"),
                c => out.push(c),
            }
        }
    }
    out
}

/// Verteilt einen Text gleichmäßig über einen Kasten — Zeichen für Zeichen,
/// in Schreibrichtung von links nach rechts.
///
/// Die Kästen sind erfunden; sie liegen aber alle **im** Kasten der Glyphen,
/// die der Spiegel ersetzt. Ein Treffer darin berührt deshalb immer echte
/// Glyphen, und die Schwärzung nimmt den Spiegel mit.
fn spread(page: usize, text: &str, rect: Rect) -> TextRun {
    let chars: Vec<char> = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let step = rect.width() / chars.len().max(1) as f64;
    let glyphs = chars
        .into_iter()
        .enumerate()
        .map(|(i, ch)| {
            let x0 = rect.ll.x + step * i as f64;
            Glyph {
                ch,
                rect: Rect::new(x0, rect.ll.y, x0 + step, rect.ur.y),
            }
        })
        .collect();
    TextRun::new(page, glyphs)
}

/// Meldet Form-XObjects, die in einem Ressourcenverzeichnis **stehen**, aber
/// im ganzen Dokument nie gezeichnet werden.
///
/// Der Interpreter führt aus, was ein `Do` erreicht — mehr nicht. Ein
/// Formular, das nur in `/Resources /XObject` steht, wird deshalb nie gelesen;
/// und weil es über `/Resources` erreichbar bleibt, überlebt es auch die
/// Aufräumrunde für unerreichbare Objekte. Sein Text steht unverändert in der
/// Ausgabe, und der Lauf meldete „nichts gefunden“ mit Rückgabewert 0.
///
/// ## Warum gemeldet und nicht entfernt
///
/// Der Eintrag aus `/Resources` zu streichen wäre gründlicher — das Objekt
/// würde unerreichbar und fiele beim Schreiben weg. Es wäre aber ein Eingriff
/// in eine Struktur, deren Reichweite hier gar nicht feststeht:
///
/// * Ein `/Resources`-Dictionary hängt oft am `/Pages`-Knoten oder wird von
///   mehreren Seiten **als dasselbe Objekt** benutzt.
/// * Erscheinungsströme von Annotationen greifen ersatzweise auf die
///   Ressourcen der Seite zurück.
/// * „Kein `Do` gefunden“ ist eine Aussage über *unseren* Durchlauf. Ein
///   anderer Betrachter kann denselben Namen über einen Weg erreichen, den
///   wir nicht gegangen sind.
///
/// Ein stiller Eingriff, der in einem dieser Fälle danebengeht, beschädigt
/// eine Datei, ohne dass es jemand merkt — und er beseitigt nicht einmal die
/// eigentliche Lücke, denn gelesen wurde der Text ja weiterhin nicht. Die
/// Meldung sagt dagegen genau das, was zutrifft, nennt das Objekt beim Namen
/// und setzt den Rückgabewert auf 3. Das Entfernen gehört, wenn es kommt, an
/// die Oberfläche als ausdrückliche Entscheidung des Nutzers — nicht in einen
/// Seiteneffekt der Analyse.
fn unplaced_form_warnings(
    doc: &Document,
    declared: &BTreeMap<ObjectId, Vec<u8>>,
    placed: &BTreeSet<ObjectId>,
) -> Vec<String> {
    let unplaced: Vec<(&ObjectId, &Vec<u8>)> = declared
        .iter()
        .filter(|(id, _)| !placed.contains(*id))
        .collect();
    if unplaced.is_empty() {
        return Vec::new();
    }
    if unplaced.len() > MAX_INSPECTED_UNPLACED_FORMS {
        return vec![format!(
            "{} Form-XObjects stehen in den Ressourcen, werden aber nirgends gezeichnet. \
             Ihr Inhalt wurde nicht durchsucht und kann deshalb nicht geschwärzt worden \
             sein.",
            unplaced.len()
        )];
    }
    unplaced
        .into_iter()
        .filter(|(id, _)| crate::content::stream_shows_text(doc, **id))
        .map(|(id, name)| {
            format!(
                "Das Form-XObject „{}“ (Objekt {} {}) steht in den Ressourcen, wird aber \
                 nirgends gezeichnet; sein Text wurde nicht durchsucht und kann deshalb \
                 nicht geschwärzt worden sein.",
                String::from_utf8_lossy(name),
                id.0,
                id.1
            )
        })
        .collect()
}

/// Wie weit eine Glyphe hinter den Stand ihrer Druckfolge zurückfallen darf,
/// ohne dass eine neue Folge beginnt.
///
/// Ein Kerningpaar (`[(A) 80 (V)] TJ`) springt um Bruchteile eines Punktes
/// zurück und gehört selbstverständlich zum selben Zug. Ein Rücksprung über
/// die ganze Zeichenkette ist dagegen ein zweiter Druck an derselben Stelle.
fn backstep_tolerance(g: &GlyphItem) -> f64 {
    let space = g.baseline.space_width.abs();
    let advance = g.baseline.advance.abs();
    (space.max(advance) * 0.5).max(0.1)
}

/// Ausdehnung einer Glyphe entlang der Grundlinie.
///
/// Maßgeblich ist der Vorschub — er ist die Zelle, die die Glyphe in der Zeile
/// belegt, und er stimmt genau mit dem Beginn der nächsten Glyphe überein.
/// Nur wo er fehlt (Teilzeichen einer Ligatur, Glyphen ohne Breite), muss die
/// Projektion des Glyphenkastens einspringen.
fn extent_along(g: &GlyphItem) -> f64 {
    let advance = g.baseline.advance.abs();
    if advance > 1e-9 {
        return advance;
    }
    let d = g.baseline.direction;
    g.rect.width().abs() * d.x.abs() + g.rect.height().abs() * d.y.abs()
}

/// Zerlegt die Glyphen in **Druckfolgen**: Läufe, die in einem Zug und ohne
/// Rücksprung auf die Grundlinie gesetzt wurden.
///
/// Eine neue Folge beginnt, wenn eine neue Textoperation anfängt oder wenn
/// innerhalb einer Operation weit hinter den erreichten Stand zurückgesprungen
/// wird (`[(Text) 16789 (Text)] TJ` — derselbe Text zweimal übereinander in
/// *einer* Operation).
///
/// Teilzeichen einer Ligatur tragen keinen Vorschub und sitzen auf dem
/// Ursprung ihres Codes. Sie sind Fortsetzung, nie Rücksprung — würde man sie
/// wie eigenständige Glyphen prüfen, zerfiele jede Ligatur in eine eigene
/// Schicht.
fn print_runs(items: &[(usize, GlyphItem)]) -> Vec<usize> {
    let mut out = Vec::with_capacity(items.len());
    let mut run = 0usize;
    // (Quelle der laufenden Folge, bisher erreichter Stand auf der Grundlinie)
    let mut open: Option<(usize, f64)> = None;
    for (source, g) in items {
        let start = along_of(g);
        let end = start + extent_along(g);
        let starts_new = match open {
            None => true,
            Some((src, _)) if src != *source => true,
            Some(_) if g.baseline.advance.abs() <= 1e-9 => false,
            Some((_, reached)) => start + backstep_tolerance(g) < reached,
        };
        if starts_new {
            if open.is_some() {
                run += 1;
            }
            open = Some((*source, end));
        } else if let Some((src, reached)) = open {
            open = Some((src, reached.max(end)));
        }
        out.push(run);
    }
    out
}

/// Welche der belegten Schichten eine Folge `[start, start + length)`
/// aufnimmt — oder `None`, wenn keine passt und eine neue fällig ist.
///
/// In Frage kommt eine Schicht nur, wenn sie höchstens innerhalb der Toleranz
/// überlappt (siehe [`OVERPRINT_RATIO`] und [`OVERPRINT_FLOOR_IN_SPACES`]).
/// Unter den in Frage kommenden gewinnt die, deren Text **am dichtesten** an
/// `start` endet — gemessen als Abstand `|reached − start|`, nicht als größtes
/// `reached`.
///
/// Der Unterschied ist der ganze Befund. „Das größte `reached`“ nimmt die
/// Schicht, die am weitesten *hineinragt*: ein zu langer Empfängername, der 26
/// pt in die Wertspalte reicht, schlägt damit die Schicht, in der das erste
/// IBAN-Stück lückenlos endet — und holt sich das zweite Stück. Keine der
/// beiden Zeilen enthält die IBAN dann noch ganz.
///
/// Umgekehrt darf „anschließend“ auch nicht bedingungslos gewinnen: zwei
/// aufeinanderfolgende `Tj` überlappen sich in der Praxis um Bruchteile eines
/// Punktes (gerundete Metriken), und eine 28 pt zurückliegende Schicht wäre
/// dann „davor“, aber gewiss nicht die Fortsetzung. Der Abstand entscheidet;
/// bei gleichem Abstand die Schicht, die nicht überlappt.
fn best_layer(layers: &[(f64, f64, f64)], start: f64, length: f64, space: f64) -> Option<usize> {
    let mut best: Option<(usize, f64, bool)> = None;
    for (index, (reached, last_length, last_space)) in layers.iter().enumerate() {
        let overlap = reached - start;
        if overlap > 0.0 {
            let tolerance = (OVERPRINT_RATIO * length.min(*last_length))
                .max(OVERPRINT_FLOOR_IN_SPACES * space.max(*last_space));
            if overlap > tolerance {
                continue;
            }
        }
        let distance = overlap.abs();
        let overlaps = overlap > 0.0;
        let better = match best {
            None => true,
            Some((_, best_distance, best_overlaps)) => {
                distance < best_distance
                    || (distance == best_distance && best_overlaps && !overlaps)
            }
        };
        if better {
            best = Some((index, distance, overlaps));
        }
    }
    best.map(|(index, _, _)| index)
}

/// Zerlegt eine Zeile in **Druckschichten**.
///
/// Die Zeile kommt bereits in Leserichtung sortiert; jede Glyphe trägt die
/// Nummer ihrer Druckfolge. Nebeneinander gesetzte Folgen kacheln die
/// Grundlinie und bleiben in einer Schicht — das ist die Tabellenzeile aus
/// vielen `Tj`-Aufrufen. Eine Folge, die eine bereits belegte Strecke
/// **überdeckt**, eröffnet eine neue Schicht.
///
/// Gesucht wird dabei nicht die erstbeste freie Schicht, sondern die, deren
/// Text am dichtesten davor endet ([`best_layer`] — dort steht, warum
/// „dichtesten“ und nicht „am weitesten reichend“). So findet ein Textstück,
/// das nach einem Überdruck an der ursprünglichen Stelle weitergeht (der
/// klassische Akzent-Überdruck `(Cr) Tj … (´) Tj … (dit) Tj`), zurück in seine
/// eigene Schicht.
fn split_layers(line: Vec<(usize, GlyphItem)>) -> Vec<Vec<GlyphItem>> {
    // Spanne und Leerzeichenmaß je Druckfolge.
    let mut spans: BTreeMap<usize, (f64, f64, f64)> = BTreeMap::new();
    for (run, g) in &line {
        let start = along_of(g);
        let end = start + extent_along(g);
        let space = g.baseline.space_width.abs();
        spans
            .entry(*run)
            .and_modify(|s| {
                s.0 = s.0.min(start);
                s.1 = s.1.max(end);
                s.2 = s.2.max(space);
            })
            .or_insert((start, end, space));
    }
    if spans.len() < 2 {
        return vec![line.into_iter().map(|(_, g)| g).collect()];
    }

    // Abgearbeitet wird **entlang der Grundlinie**, nicht in der Reihenfolge
    // der Zeile: die ist nach Grundlinienband und erst dann nach Lage
    // sortiert, und eine hochgestellte Ziffer (`Ts`) steht deshalb vor dem
    // Text, der links von ihr beginnt. Wer sie in dieser Reihenfolge einfüllt,
    // erklärt den Zeilenanfang zur zweiten Schicht.
    let mut order: Vec<usize> = spans.keys().copied().collect();
    order.sort_by(|a, b| spans[a].0.total_cmp(&spans[b].0).then(a.cmp(b)));

    // Belegte Schichten: (erreichter Stand, Länge der zuletzt eingefügten
    // Folge, deren Leerzeichenmaß).
    let mut layers: Vec<(f64, f64, f64)> = Vec::new();
    let mut layer_of: BTreeMap<usize, usize> = BTreeMap::new();
    for run in order {
        let (start, end, space) = spans[&run];
        let length = (end - start).max(0.0);
        let mut chosen = best_layer(&layers, start, length, space);
        // Jenseits der Obergrenze wird nicht weiter aufgefächert: die Suche
        // ist linear in der Zahl der Schichten, und eine Datei, die tausend
        // Texte an dieselbe Stelle druckt, machte daraus quadratischen
        // Aufwand. Kein Satz übereinander gedruckter Texte reicht so weit.
        if chosen.is_none() && layers.len() >= MAX_PRINT_LAYERS {
            // Die am weitesten zurückliegende Schicht — der Ausweg, wenn die
            // Obergrenze erreicht ist.
            chosen = layers
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| a.0.total_cmp(&b.0))
                .map(|(index, _)| index);
        }
        match chosen {
            Some(index) => {
                layers[index] = (layers[index].0.max(end), length, space);
                layer_of.insert(run, index);
            }
            None => {
                layer_of.insert(run, layers.len());
                layers.push((end, length, space));
            }
        }
    }
    if layers.len() < 2 {
        return vec![line.into_iter().map(|(_, g)| g).collect()];
    }

    let mut out: Vec<Vec<GlyphItem>> = vec![Vec::new(); layers.len()];
    for (run, g) in line {
        out[layer_of[&run]].push(g);
    }
    out.retain(|layer| !layer.is_empty());
    out
}

// ---------------------------------------------------------------------------
// Grundlinien-Geometrie
// ---------------------------------------------------------------------------

/// So viele Glyphenpaare müssen vorliegen, bevor ein gleichmäßiger Überschuss
/// als Satzweise (und nicht als Wortlücke) gewertet wird.
const MIN_PITCH_SAMPLES: usize = 4;

/// Anteil der Paare, der zum vorherrschenden Überschuss passen muss.
const PITCH_AGREEMENT: f64 = 0.6;

/// Bis zu wie vielen Leerzeichenbreiten ein gleichmäßiger Überschuss noch als
/// Sperrung durchgeht. Darüber liegt Spaltensatz, und der trennt wirklich.
const MAX_PITCH_IN_SPACES: f64 = 2.0;

/// Gleichmäßiger Überschuss einer Zeile — die „Satzweise“.
///
/// Formulare setzen Ziffern gern einzeln auf ein festes Raster, das breiter
/// ist als der natürliche Vorschub. Der Überschuss ist dann in der ganzen
/// Zeile derselbe: er ist Gestaltung, keine Wortlücke. Wer ihn nicht
/// herausrechnet, macht aus `4711000` ein `4 7 1 1 0 0 0` — und keine
/// Kontonummer wird mehr erkannt.
///
/// Ein Überschuss von mehr als [`MAX_PITCH_IN_SPACES`] Leerzeichenbreiten
/// bleibt unangetastet: so weit auseinander stehen Spalten, nicht Zeichen.
fn line_pitch(extras: &[f64], space_width: f64) -> f64 {
    if extras.len() < MIN_PITCH_SAMPLES {
        return 0.0;
    }
    let mut sorted: Vec<f64> = extras.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let median = sorted[sorted.len() / 2];
    if median <= 0.0 || median >= space_width * MAX_PITCH_IN_SPACES {
        return 0.0;
    }
    let band = (median * 0.25).max(0.05);
    let agreeing = extras
        .iter()
        .filter(|e| (**e - median).abs() <= band)
        .count();
    if (agreeing as f64) < extras.len() as f64 * PITCH_AGREEMENT {
        return 0.0;
    }
    median
}

/// Lage quer zur Schreibrichtung — das ist „die Zeile“.
///
/// Bei waagerechtem Text ist das schlicht `origin.y`.
fn across_of(g: &GlyphItem) -> f64 {
    let d = g.baseline.direction;
    -g.origin.x * d.y + g.origin.y * d.x
}

/// Lage entlang der Schreibrichtung — das ist „die Spalte“.
///
/// Bei waagerechtem Text ist das schlicht `origin.x`; bei 180°-Text zählt es
/// rückwärts, wodurch die Zeile in der richtigen Leserichtung entsteht.
fn along_of(g: &GlyphItem) -> f64 {
    let d = g.baseline.direction;
    g.origin.x * d.x + g.origin.y * d.y
}

/// Schreibrichtung, auf ganze Grad gerundet (0..359).
fn direction_bucket(g: &GlyphItem) -> i64 {
    let d = g.baseline.direction;
    let degrees = d.y.atan2(d.x).to_degrees().rem_euclid(360.0);
    ((degrees / DIRECTION_BUCKET_DEGREES).round() as i64) % (360 / DIRECTION_BUCKET_DEGREES as i64)
}

/// Sortierschlüssel: erst nach Schreibrichtung, dann Zeile für Zeile in
/// Vorschubrichtung, innerhalb der Zeile in Schreibrichtung.
/// Vergleicht zwei Sortierschlüssel — mit einer **totalen** Ordnung.
///
/// ## Warum nicht `partial_cmp(..).unwrap_or(Equal)`
///
/// Weil das mit `NaN` keine Ordnung ist, sondern nur so aussieht. Gegenbeispiel
/// mit drei Werten: `a = 5,0`, `n = NaN`, `b = 1,0` ergibt `a < n` (Equal aus
/// dem `unwrap_or`), `n < b` (ebenso) — und trotzdem `a > b`. Aus `a ≤ n ≤ b`
/// folgt dann nicht `a ≤ b`, die Transitivität ist verletzt.
///
/// Seit Rust 1.81 darf `slice::sort_by` das bemerken und den Prozess mit
/// „user-provided comparison function does not correctly implement a total
/// order" **abbrechen**. In einem Werkzeug, dem man fremde Dateien vorwirft,
/// ist ein Abbruch kein Schönheitsfehler: er ist ein Weg, den Lauf von außen
/// zu beenden. Isoliert nachgemessen tritt er ab **21 Elementen** auf (darunter
/// benutzt `sort_by` Einfügesortierung und prüft nicht).
///
/// `total_cmp` bildet die Ordnung der IEEE-754-Bitmuster ab: sie ist total,
/// braucht keinen Sonderfall und sortiert `NaN` ans Ende statt mittendrin.
///
/// ## Was hier **nicht** behauptet wird
///
/// Dass eine PDF-Datei diesen Abbruch auslösen *kann*, ist an dieser Stelle
/// nicht belegt. Zwei Versuche mit präparierten Dateien (verkettete
/// `cm`-Überläufe ins Unendliche, danach `0 0 0 0 0 0 cm`, jeweils mit 30
/// Glyphen) haben hier kein `NaN` erzeugt — die Koordinatenprüfungen
/// stromaufwärts fangen offenbar vorher ab. Die Korrektur steht trotzdem,
/// aus dem Grund, der diesem Durchgang seinen Namen gegeben hat: eine
/// Zusicherung, die an ihrer Ursprungsstelle gilt, wird an der nächsten gern
/// stillschweigend mitgenommen. Hier auf „kein Aufrufer kann `NaN` liefern"
/// zu bauen, wäre genau das.
fn key_cmp(a: &(i64, f64, f64), b: &(i64, f64, f64)) -> std::cmp::Ordering {
    a.0.cmp(&b.0)
        .then(a.1.total_cmp(&b.1))
        .then(a.2.total_cmp(&b.2))
}

fn sort_key(g: &GlyphItem, tol: f64) -> (i64, f64, f64) {
    (
        direction_bucket(g),
        -(across_of(g) / tol).round(),
        along_of(g),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::Baseline;
    use lopdf::{dictionary, Object, Stream};
    use redact_core::Point;

    fn glyph(text: &str, x: f64, y: f64, w: f64) -> GlyphItem {
        GlyphItem {
            bytes: vec![b'x'],
            text: text.to_string(),
            rect: Rect::new(x, y, x + w, y + 10.0),
            origin: Point::new(x, y),
            displacement: w,
            baseline: Baseline {
                direction: Point::new(1.0, 0.0),
                advance: w,
                height: 10.0,
                space_width: w * 0.5,
            },
        }
    }

    /// Glyphen einer einzigen Textoperation — die Herkunft ist für die
    /// Zeilenbildung ohnehin nicht maßgeblich, und die Tests unten prüfen die
    /// Geometrie.
    fn one_show(glyphs: Vec<GlyphItem>) -> Vec<(usize, GlyphItem)> {
        glyphs.into_iter().map(|g| (0, g)).collect()
    }

    /// Einseitiges PDF mit frei gewähltem Content-Stream; `/F1` ist Helvetica.
    fn doc_with_content(content: &str) -> Document {
        let mut doc = Document::with_version("1.5");
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
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
        doc
    }

    /// Die extrahierten Zeilentexte eines Content-Streams.
    fn lines_of(content: &str) -> Vec<String> {
        let doc = doc_with_content(content);
        PdfExtractor::new()
            .extract(&doc)
            .expect("Extraktion")
            .into_iter()
            .map(|run| run.text)
            .collect()
    }

    #[test]
    fn groups_glyphs_into_lines() {
        let glyphs = vec![
            glyph("A", 0.0, 100.0, 5.0),
            glyph("B", 5.0, 100.0, 5.0),
            glyph("C", 0.0, 80.0, 5.0),
        ];
        let lines = PdfExtractor::build_lines(0, one_show(glyphs));
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "AB");
        assert_eq!(lines[1].text, "C");
    }

    #[test]
    fn inserts_space_for_large_gaps() {
        let glyphs = vec![
            glyph("D", 0.0, 100.0, 5.0),
            glyph("E", 5.0, 100.0, 5.0),
            glyph("8", 40.0, 100.0, 5.0),
        ];
        let lines = PdfExtractor::build_lines(0, one_show(glyphs));
        assert_eq!(lines[0].text, "DE 8");
        // Die Glyph-Liste muss zeichenweise deckungsgleich bleiben.
        assert_eq!(lines[0].glyphs.len(), lines[0].text.chars().count());
    }

    #[test]
    fn sorts_out_of_order_glyphs_left_to_right() {
        let glyphs = vec![glyph("Z", 20.0, 100.0, 5.0), glyph("A", 0.0, 100.0, 5.0)];
        let lines = PdfExtractor::build_lines(0, one_show(glyphs));
        assert!(lines[0].text.starts_with('A'));
    }

    #[test]
    fn ligature_glyph_keeps_char_alignment() {
        let glyphs = vec![glyph("fi", 0.0, 100.0, 8.0), glyph("x", 8.0, 100.0, 5.0)];
        let lines = PdfExtractor::build_lines(0, one_show(glyphs));
        assert_eq!(lines[0].text, "fix");
        assert_eq!(lines[0].glyphs.len(), 3);
    }

    #[test]
    fn empty_input_yields_no_lines() {
        assert!(PdfExtractor::build_lines(0, vec![]).is_empty());
    }

    #[test]
    fn line_pitch_only_reports_a_uniform_moderate_excess() {
        // Gleichmäßiges Raster: erkannt.
        assert_eq!(line_pitch(&[1.4, 1.4, 1.4, 1.4, 1.4], 2.5), 1.4);
        // Zu wenige Paare: kein Urteil.
        assert_eq!(line_pitch(&[1.4, 1.4, 1.4], 2.5), 0.0);
        // Überwiegend bündiger Satz mit einzelnen Lücken: kein Raster.
        assert_eq!(line_pitch(&[0.0, 0.0, 0.0, 6.0, 0.0], 2.5), 0.0);
        // Uneinheitlich: kein Raster.
        assert_eq!(line_pitch(&[0.4, 1.4, 3.0, 0.9, 2.2], 2.5), 0.0);
        // Spaltensatz (über zwei Leerzeichenbreiten): unangetastet.
        assert_eq!(line_pitch(&[9.0, 9.0, 9.0, 9.0, 9.0], 2.5), 0.0);
    }

    // -----------------------------------------------------------------------
    // K1 — Laufweite (`Tc`) und Rasterpositionierung dürfen keine
    //      Leerzeichen erfinden.
    // -----------------------------------------------------------------------

    #[test]
    fn char_spacing_does_not_split_an_iban() {
        // So setzen Banken Kontofelder: gesperrte Schrift über `Tc`.
        // 9 pt Helvetica, Ziffernbreite 5,00 pt — jede Laufweite oberhalb von
        // rund 1,4 pt lag früher über der Lückenschwelle.
        for tc in ["1.5", "3.0", "6.0"] {
            let content =
                format!("BT /F1 9 Tf {tc} Tc 1 0 0 1 72 700 Tm (DE89370400440532013000) Tj ET");
            assert_eq!(
                lines_of(&content),
                vec!["DE89370400440532013000".to_string()],
                "Tc = {tc}"
            );
        }
    }

    #[test]
    fn single_glyphs_on_a_grid_stay_one_number() {
        // Formularraster: jede Ziffer einzeln auf 6,4 pt Abstand gesetzt,
        // obwohl der natürliche Vorschub nur 5,00 pt beträgt.
        let mut content = String::from("BT /F1 9 Tf 1 0 0 1 72 700 Tm ");
        for (i, ch) in "4711000".chars().enumerate() {
            if i > 0 {
                content.push_str("6.4 0 Td ");
            }
            content.push_str(&format!("({ch}) Tj "));
        }
        content.push_str("ET");
        assert_eq!(lines_of(&content), vec!["4711000".to_string()]);
    }

    #[test]
    fn a_wide_grid_still_keeps_the_number_together() {
        // Weiteres Raster (8 pt bei 5,00 pt Vorschub). Der Überschuss ist in
        // der ganzen Zeile derselbe — Gestaltung, keine Wortlücke.
        let mut content = String::from("BT /F1 9 Tf 1 0 0 1 72 700 Tm ");
        for (i, ch) in "12345678901".chars().enumerate() {
            if i > 0 {
                content.push_str("8 0 Td ");
            }
            content.push_str(&format!("({ch}) Tj "));
        }
        content.push_str("ET");
        assert_eq!(lines_of(&content), vec!["12345678901".to_string()]);
    }

    #[test]
    fn a_word_gap_inside_a_grid_is_still_a_space() {
        // Dasselbe Raster, aber an einer Stelle bleibt eine Zelle frei.
        let mut content = String::from("BT /F1 9 Tf 1 0 0 1 72 700 Tm ");
        for (i, ch) in "12345678901".chars().enumerate() {
            if i > 0 {
                let step = if i == 5 { 16.0 } else { 8.0 };
                content.push_str(&format!("{step} 0 Td "));
            }
            content.push_str(&format!("({ch}) Tj "));
        }
        content.push_str("ET");
        assert_eq!(lines_of(&content), vec!["12345 678901".to_string()]);
    }

    #[test]
    fn evenly_spaced_columns_are_still_separated() {
        // Grenze nach oben: ein gleichmäßiger Abstand von 20 pt ist kein
        // gesperrter Satz mehr, sondern eine Tabelle.
        let mut content = String::from("BT /F1 9 Tf 1 0 0 1 72 700 Tm ");
        for (i, ch) in "ABCDE".chars().enumerate() {
            if i > 0 {
                content.push_str("20 0 Td ");
            }
            content.push_str(&format!("({ch}) Tj "));
        }
        content.push_str("ET");
        assert_eq!(lines_of(&content), vec!["A B C D E".to_string()]);
    }

    #[test]
    fn single_glyphs_at_their_natural_advance_stay_one_word() {
        // Belegt korrektes Verhalten (darf nicht kaputtgehen): Einzelglyphen
        // im natürlichen Vorschub von Helvetica 10 pt.
        let widths = [7.22, 5.56, 5.56, 2.78, 5.56]; // D, o, o, f, ... nur Beispiel
        let mut content = String::from("BT /F1 10 Tf 1 0 0 1 72 700 Tm ");
        for (i, ch) in "Konto".chars().enumerate() {
            if i > 0 {
                content.push_str(&format!("{} 0 Td ", widths[i - 1]));
            }
            content.push_str(&format!("({ch}) Tj "));
        }
        content.push_str("ET");
        assert_eq!(lines_of(&content), vec!["Konto".to_string()]);
    }

    #[test]
    fn a_real_gap_still_becomes_a_space() {
        // Gegenprobe zu K1: eine echte Wortlücke muss weiterhin erkannt werden.
        let content = "BT /F1 9 Tf 1 0 0 1 72 700 Tm (IBAN:) Tj 1 0 0 1 100 700 Tm \
                       (DE89370400440532013000) Tj ET";
        assert_eq!(
            lines_of(content),
            vec!["IBAN: DE89370400440532013000".to_string()]
        );
    }

    #[test]
    fn table_columns_are_separated_by_spaces() {
        let content = "BT /F1 9 Tf 1 0 0 1 72 700 Tm (05.01.2026) Tj \
                       1 0 0 1 200 700 Tm (Ueberweisung) Tj \
                       1 0 0 1 400 700 Tm (1.234,56) Tj ET";
        assert_eq!(
            lines_of(content),
            vec!["05.01.2026 Ueberweisung 1.234,56".to_string()]
        );
    }

    #[test]
    fn negative_tj_adjustments_become_a_space() {
        // Ein negativer `TJ`-Wert schiebt nach rechts — der klassische Weg,
        // Wortabstände ohne Leerzeichen-Glyphe zu setzen.
        let content = "BT /F1 9 Tf 1 0 0 1 72 700 Tm [(Kontonummer)-400(4711000)] TJ ET";
        assert_eq!(
            lines_of(content),
            vec!["Kontonummer 4711000".to_string()],
            "negativer TJ-Vorschub"
        );
    }

    #[test]
    fn small_negative_tj_adjustments_do_not_split_a_number() {
        // Feinausgleich innerhalb einer Zahl (−40/1000 em) ist kein Wortabstand.
        let content = "BT /F1 9 Tf 1 0 0 1 72 700 Tm [(4711)-40(000)] TJ ET";
        assert_eq!(lines_of(content), vec!["4711000".to_string()]);
    }

    #[test]
    fn superscript_stays_on_its_line() {
        // `Ts` hebt die Grundlinie an; die Fußnotenziffer gehört trotzdem
        // zur Zeile.
        let content = "BT /F1 10 Tf 1 0 0 1 72 700 Tm (Kontostand) Tj \
                       3 Ts (1) Tj 0 Ts ( 4711000) Tj ET";
        let lines = lines_of(content);
        assert_eq!(
            lines.len(),
            1,
            "Hochstellung hat die Zeile zerrissen: {lines:?}"
        );
    }

    // -----------------------------------------------------------------------
    // K2 — gedrehter Text
    // -----------------------------------------------------------------------

    #[test]
    fn text_rotated_by_90_degrees_forms_one_line() {
        let content = "BT /F1 9 Tf 0 1 -1 0 300 400 Tm (DE89 3704 0044 0532 0130 00) Tj ET";
        assert_eq!(
            lines_of(content),
            vec!["DE89 3704 0044 0532 0130 00".to_string()]
        );
    }

    #[test]
    fn text_rotated_by_270_degrees_forms_one_line() {
        let content = "BT /F1 9 Tf 0 -1 1 0 300 400 Tm (DE89 3704 0044 0532 0130 00) Tj ET";
        assert_eq!(
            lines_of(content),
            vec!["DE89 3704 0044 0532 0130 00".to_string()]
        );
    }

    #[test]
    fn text_rotated_by_180_degrees_reads_forwards() {
        let content = "BT /F1 9 Tf -1 0 0 -1 300 400 Tm (Kontonummer 4711000) Tj ET";
        assert_eq!(lines_of(content), vec!["Kontonummer 4711000".to_string()]);
    }

    #[test]
    fn rotated_lines_are_kept_apart_and_in_order() {
        // Zwei um 90° gedrehte Zeilen nebeneinander (Zeilenvorschub geht in +x).
        let content = "BT /F1 9 Tf 0 1 -1 0 300 400 Tm (Kontonummer) Tj \
                       0 1 -1 0 315 400 Tm (4711000) Tj ET";
        assert_eq!(
            lines_of(content),
            vec!["Kontonummer".to_string(), "4711000".to_string()]
        );
    }

    // -----------------------------------------------------------------------
    // K8 — Zeilentoleranz gehört zur Zeile, nicht zur betrachteten Glyphe
    // -----------------------------------------------------------------------

    #[test]
    fn a_tall_heading_does_not_swallow_the_line_above_it() {
        // 9 pt bei y = 705, 16 pt bei y = 700: die kleine Zeile eröffnet die
        // Gruppe, ihre Toleranz (3,93 pt) trägt die 5 pt Abstand nicht.
        let content = "BT /F1 9 Tf 1 0 0 1 72 705 Tm (4711000) Tj \
                       /F1 16 Tf 1 0 0 1 72 700 Tm (Kontoauszug) Tj ET";
        assert_eq!(
            lines_of(content),
            vec!["4711000".to_string(), "Kontoauszug".to_string()]
        );
    }

    // -----------------------------------------------------------------------
    // Druckschichten — Grenzen der Regel
    // -----------------------------------------------------------------------

    #[test]
    fn overprinted_text_becomes_two_lines() {
        let content = "BT /F1 10 Tf 1 0 0 1 72 700 Tm (4711000) Tj ET \
                       BT /F1 10 Tf 1 0 0 1 72 700 Tm (4711000) Tj ET";
        assert_eq!(
            lines_of(content),
            vec!["4711000".to_string(), "4711000".to_string()]
        );
    }

    #[test]
    fn a_hundred_overprints_stay_within_the_layer_cap() {
        // Die Zuordnung sucht linear über die belegten Schichten. Ohne
        // Obergrenze wäre eine Datei, die hundertfach an dieselbe Stelle
        // druckt, quadratischer Aufwand — und die kann sich jeder bauen.
        //
        // Die Schranke steht hier als **feste Zahl**, nicht als
        // `MAX_PRINT_LAYERS`. Gegen die Konstante geprüft wüchse sie mit ihr
        // mit und könnte gar nicht fehlschlagen — 100 Schichten sind auch
        // dann noch „höchstens 1 000 000“. Genau das ist passiert: mit
        // `MAX_PRINT_LAYERS = 1_000_000` blieb der Test grün, während die
        // Extraktion einer Datei mit n Drucken an derselben Stelle
        // quadratisch wurde (`--release`):
        //
        //     n         Decke 64     Decke 1 000 000
        //      20 000    0,388 s      0,763 s
        //      50 000    0,906 s      3,148 s
        //     100 000    2,951 s     15,801 s
        //
        // Doppelte Eingabe, fünffache Zeit. 64 ist die Decke, die dieses
        // Programm hält; wer sie anhebt, ändert Laufzeitverhalten und soll
        // das hier begründen müssen.
        let mut content = String::new();
        for _ in 0..100 {
            content.push_str("BT /F1 10 Tf 1 0 0 1 72 700 Tm (4711000) Tj ET ");
        }
        let lines = lines_of(&content);
        assert_eq!(
            lines.len(),
            64,
            "hundert Überdrucke, Decke 64 — es müssen genau 64 Schichten sein"
        );
        // Jenseits der Grenze fallen Drucke wieder zusammen und verschränken
        // sich — die Schichten davor bleiben aber sauber, und darauf kommt es
        // an: hundert Drucke an derselben Stelle sind kein Satz, sondern ein
        // Angriff auf die Laufzeit.
        assert!(
            lines.iter().filter(|l| l.as_str() == "4711000").count() >= 2,
            "keine saubere Schicht übrig: {lines:?}"
        );
    }

    #[test]
    fn a_cell_that_slightly_overruns_the_next_stays_one_line() {
        // „Kontonummer“ ist bei 10 pt rund 61 pt breit und ragt damit 3 pt in
        // die nächste Zelle. Das ist Feinsatz, kein zweiter Druck: die Zeile
        // bleibt **eine** Zeile. (Dass sich an der Nahtstelle ein Zeichen
        // verschränkt, ist alte Kost und hat mit den Schichten nichts zu tun.)
        let content = "BT /F1 10 Tf 1 0 0 1 72 700 Tm (Kontonummer) Tj \
                       1 0 0 1 130 700 Tm (4711000) Tj ET";
        let lines = lines_of(content);
        assert_eq!(
            lines.len(),
            1,
            "eine leichte Überschneidung darf die Zeile nicht zerlegen: {lines:?}"
        );
    }

    /// Die Glyphensortierung bricht auch mit `NaN` nicht ab.
    ///
    /// `slice::sort_by` prüft ab **21 Elementen**, ob der Vergleicher eine
    /// totale Ordnung liefert, und beendet den Prozess sonst mit
    /// „user-provided comparison function does not correctly implement a
    /// total order". Ein Abbruch ist in einem Werkzeug, dem man fremde
    /// Dateien vorwirft, ein Weg, den Lauf von aussen zu beenden — deshalb
    /// steht hier eine Zahl über der Schwelle, nicht darunter.
    #[test]
    fn die_glyphensortierung_ist_eine_totale_ordnung() {
        // 25 Schlüssel, jeder dritte mit NaN in der Querlage.
        let mut keys: Vec<(i64, f64, f64)> = (0..25)
            .map(|i| {
                let across = if i % 3 == 0 {
                    f64::NAN
                } else {
                    f64::from(25 - i)
                };
                (0, across, f64::from(i))
            })
            .collect();

        keys.sort_by(key_cmp);

        // Kein Element geht verloren, und die Folge ist nach dem eigenen
        // Vergleicher aufsteigend — mehr ist bei NaN nicht zu verlangen und
        // weniger wäre keine Ordnung.
        assert_eq!(keys.len(), 25);
        assert!(
            keys.windows(2)
                .all(|w| key_cmp(&w[0], &w[1]) != std::cmp::Ordering::Greater),
            "nach dem Sortieren nicht aufsteigend: {keys:?}"
        );
        assert_eq!(
            keys.iter().filter(|k| k.1.is_nan()).count(),
            9,
            "die NaN-Schlüssel sind verschwunden"
        );
    }

    /// Gegenprobe: gewöhnliche Schlüssel ordnen wie vorher — Zeilenband vor
    /// Lage in der Zeile, Vorschubrichtung zuerst.
    #[test]
    fn gewoehnliche_schluessel_ordnen_unveraendert() {
        let mut keys = vec![(0, 2.0, 5.0), (1, 0.0, 0.0), (0, 1.0, 9.0), (0, 1.0, 3.0)];
        keys.sort_by(key_cmp);
        assert_eq!(
            keys,
            vec![(0, 1.0, 3.0), (0, 1.0, 9.0), (0, 2.0, 5.0), (1, 0.0, 0.0)]
        );
    }
}
