//! Konfliktauflösung: die Negativliste gewinnt immer.

use std::cmp::Ordering;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::geometry::Rect;
use crate::model::{Region, Source};

/// Ein durch die Negativliste blockierter Treffer (für das Audit-Log).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockedRegion {
    pub page: usize,
    pub rect: Rect,
    /// Text der blockierenden Negativlisten-Region.
    pub pattern: String,
    pub booking_id: String,
    /// Beschreibung des Treffers, der dadurch verhindert wurde.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
}

/// Ergebnis der Konfliktauflösung.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Resolution {
    /// Regionen, die geschwärzt werden sollen.
    pub redact: Vec<Region>,
    /// Treffer, die durch die Negativliste verhindert wurden.
    pub blocked: Vec<BlockedRegion>,
}

/// Ab welchem Überdeckungsgrad eine Negativregion einen Treffer blockiert.
///
/// Sie entscheidet, ob eine Schwärzung unterbleibt, und darf sich deshalb
/// nicht unbemerkt verschieben. Nach oben hält sie
/// `negativliste_blockiert_ab_sechzig_prozent`, nach unten die Zusicherung
/// direkt darunter — unter 0,5 übersetzt diese Datei nicht mehr.
const BLOCK_COVERAGE_THRESHOLD: f64 = 0.5;

/// Ab welchem Überdeckungsgrad eine Region als „vollständig in einer anderen
/// enthalten“ gilt — und deshalb **weggeworfen** wird.
///
/// Das ist die teuerste Schwelle des Programms: Wer sie senkt, wirft
/// Schwärzungen weg, die nur teilweise von einer anderen abgedeckt sind. Der
/// nicht abgedeckte Rest bliebe dann sichtbar im Dokument stehen. 0,999 heißt
/// „bis auf Rundungsreste deckungsgleich“; alles darunter ist ein Datenleck.
/// Auch sie ist von beiden Seiten durch Tests gehalten
/// (`eine_zu_neunundneunzig_prozent_ueberdeckte_region_bleibt` gegen
/// `eine_praktisch_deckungsgleiche_region_faellt_weg`).
const CONTAINMENT_THRESHOLD: f64 = 0.999;

// Beide Schwellen müssen über der Hälfte liegen, sonst wird die Suche im
// [`RectGrid`] unvollständig — siehe dort die Begründung zum Mittelpunkt.
// Bewusst ein Compile-Fehler und keine Laufzeitprüfung: wer die Schwelle
// senkt, soll nicht erst in einem geschwärzten PDF davon erfahren.
const _: () = assert!(BLOCK_COVERAGE_THRESHOLD >= 0.5);
const _: () = assert!(CONTAINMENT_THRESHOLD >= 0.5);

/// Trennt Negativlisten-Treffer ab und entfernt alle Regionen, die von ihnen
/// überdeckt werden.
///
/// Regeln (siehe Konzept §5.3):
/// * Ein Treffer in der Negativliste blockiert eine Schwärzung — auch wenn ein
///   Pattern oder die Positivliste ebenfalls trifft.
/// * Manuelle Regionen sind eine bewusste Nutzerentscheidung und werden **nicht**
///   blockiert; sie überstimmen die Negativliste.
///
/// Blockiert ein Treffer, dann gewinnt der **erste** passende Negativeintrag in
/// Eingabereihenfolge; sein Text und seine Buchungs-ID landen im Audit-Log.
///
/// ## Laufzeit
///
/// Die Negativeinträge lagen früher nur nach Seite vorsortiert vor, und
/// innerhalb einer Seite lief jeder Kandidat gegen jeden Eintrag. Auf einem
/// Kontoauszug stehen aber alle Treffer *einer* Seite — die Vorsortierung
/// bringt dort nichts, und übrig bleibt ein Produkt: 50 000 Kandidaten gegen
/// 50 000 Negativeinträge sind 2,5 Milliarden Rechteckvergleiche (gemessen:
/// 10,4 s). Jede Seite bekommt deshalb ein [`RectGrid`], in dem nur noch die
/// Nachbarschaft des Kandidaten geprüft wird.
pub fn resolve_conflicts(regions: Vec<Region>) -> Resolution {
    let (negatives, candidates): (Vec<Region>, Vec<Region>) =
        regions.into_iter().partition(|r| r.is_blocking());

    // Ein Gitter je Seite: nur Negativeinträge derselben Seite kommen
    // überhaupt in Frage, und innerhalb der Seite nur die Nachbarschaft.
    let mut by_page: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, neg) in negatives.iter().enumerate() {
        by_page.entry(neg.page).or_default().push(i);
    }
    let mut grids: HashMap<usize, RectGrid> = HashMap::with_capacity(by_page.len());
    for (page, indices) in &by_page {
        let mut grid = RectGrid::new(indices.iter().map(|&i| &negatives[i].rect));
        for &i in indices {
            grid.insert(i, &negatives[i].rect);
        }
        grids.insert(*page, grid);
    }

    let mut result = Resolution::default();

    for cand in candidates {
        // Manuelle Regionen sind immer stärker als die Negativliste.
        let manual = matches!(cand.source, Source::Manual { .. });
        let blocker = if manual {
            None
        } else {
            grids.get(&cand.page).and_then(|grid| {
                grid.candidates_covering(&cand.rect)
                    .filter(|&i| {
                        cand.rect.covered_fraction(&negatives[i].rect) >= BLOCK_COVERAGE_THRESHOLD
                    })
                    // Das Gitter liefert die Nachbarschaft in beliebiger
                    // Reihenfolge; maßgeblich ist wie früher der erste
                    // passende Eintrag in Eingabereihenfolge.
                    .min()
            })
        };

        match blocker {
            Some(i) => {
                let neg = &negatives[i];
                result.blocked.push(BlockedRegion {
                    page: cand.page,
                    rect: cand.rect,
                    pattern: neg.text.clone().unwrap_or_default(),
                    booking_id: match &neg.source {
                        Source::Booking { booking_id, .. } => booking_id.clone(),
                        _ => String::new(),
                    },
                    blocked_reason: Some(cand.reason()),
                });
            }
            None => result.redact.push(cand),
        }
    }

    dedup(&mut result.redact);
    result
}

/// Entfernt exakte Duplikate und Regionen, die vollständig in einer anderen
/// Region derselben Seite und derselben Herkunft enthalten sind.
///
/// ## Warum nicht jeder gegen jeden
///
/// Früher lief hier jede Region gegen jede bereits behaltene. Bei der
/// CLI-Obergrenze von 100 000 Kandidaten sind das fünf Milliarden
/// Rechteckvergleiche; gemessen wurden 132,6 Sekunden für eine 212-kB-Datei.
///
/// ## Warum auch der Streifenzug nicht reichte
///
/// Der erste Versuch war ein Streifenzug: nach linker Kante sortieren und alle
/// behaltenen Regionen mitführen, die noch in den Streifen hineinragen. Das
/// hilft nur, wenn die Rechtecke in x auseinanderliegen. Auf einem Kontoauszug
/// bilden IBAN, Kontonummer und Betrag aber jeweils eine **Spalte**: gleiche
/// x-Spanne, verschiedene y. Aus dem Streifen fällt dann nie etwas heraus, die
/// mitgeführte Menge wächst auf n, und das quadratische Verhalten ist zurück
/// (gemessen: 66,7 s für 100 000 Treffer einer Spalte). Der frühere Test hat
/// das nicht bemerkt, weil er ein Gitter erzeugte — die eine Anordnung, die
/// dem Streifenzug nicht wehtut.
///
/// Statt eines Streifens deshalb ein [`RectGrid`] über beide Achsen: für jede
/// Region werden nur noch die Regionen ihrer Gitterzelle geprüft.
///
/// ## Reihenfolge
///
/// Sortiert wird weiterhin nach Seite und linker Kante (bei Gleichstand
/// entscheidet die Eingabereihenfolge): enthält eine Region eine andere
/// vollständig, dann liegt ihre linke Kante nicht weiter rechts — sie ist also
/// vorher an der Reihe und überlebt.
///
/// Nebenwirkung, bewusst in Kauf genommen: die ursprüngliche Fassung hing an
/// der Eingabereihenfolge. Kam die kleinere Region zuerst, überlebten beide.
/// Jetzt fällt die enthaltene immer weg. Das ist ungefährlich — die
/// umschließende Region hat dieselbe Herkunft und wird ohnehin geschwärzt —
/// und es macht das Ergebnis unabhängig davon, in welcher Reihenfolge die
/// Treffer anfallen.
fn dedup(regions: &mut Vec<Region>) {
    if regions.len() < 2 {
        return;
    }

    let mut order: Vec<usize> = (0..regions.len()).collect();
    order.sort_by(|&a, &b| {
        let (ra, rb) = (&regions[a], &regions[b]);
        ra.page.cmp(&rb.page).then_with(|| {
            ra.rect
                .ll
                .x
                .partial_cmp(&rb.rect.ll.x)
                .unwrap_or(Ordering::Equal)
                // Bei gleicher Kante entscheidet die Eingabereihenfolge, damit
                // von zwei deckungsgleichen Treffern der erste bleibt.
                .then(a.cmp(&b))
        })
    });

    let mut redundant = vec![false; regions.len()];
    // Enthält nur die *behaltenen* Regionen der gerade bearbeiteten Seite.
    let mut grid = RectGrid::new(regions.iter().map(|r| &r.rect));
    let mut page = None;

    for &i in &order {
        let region = &regions[i];
        if page != Some(region.page) {
            // `order` ist nach Seite sortiert, die Seiten kommen also am
            // Stück. Ohne das Leeren könnte eine Region der Vorseite eine
            // Region dieser Seite schlucken.
            page = Some(region.page);
            grid.clear();
        }

        let covered = grid.candidates_covering(&region.rect).any(|k| {
            regions[k].source == region.source
                && region.rect.covered_fraction(&regions[k].rect) >= CONTAINMENT_THRESHOLD
        });
        if covered {
            redundant[i] = true;
        } else {
            grid.insert(i, &region.rect);
        }
    }

    let mut keep: Vec<Region> = Vec::with_capacity(regions.len());
    for (i, region) in regions.drain(..).enumerate() {
        if !redundant[i] {
            keep.push(region);
        }
    }
    *regions = keep;
}

/// Höchstens ein Punkt (1/72 Zoll) Kantenlänge — ein Gitter aus Zellen der
/// Breite 0 hätte unendlich viele davon.
const MIN_CELL: f64 = 1.0;

/// Wie viele Zellen ein Rechteck höchstens belegen darf. Was darüber liegt,
/// ist gegenüber der Nachbarschaft so groß, dass Eintragen teurer wäre als
/// Mitprüfen; es wandert nach [`RectGrid::everywhere`].
const MAX_CELLS_PER_RECT: f64 = 64.0;

/// Ein grobes Gitter über eine Seite: jedes eingetragene Rechteck steht in
/// allen Zellen, die es überdeckt.
///
/// ## Zwei Fragen, zwei Abfragen
///
/// * [`RectGrid::candidates_covering`] beantwortet „welches eingetragene
///   Rechteck überdeckt dieses zu mindestens der Hälfte?“ und sieht dafür
///   **nur die Zelle des Mittelpunkts** an — Begründung gleich darunter.
/// * [`RectGrid::touching_into`] beantwortet „welches eingetragene Rechteck
///   **berührt** dieses?“ und muss dafür **alle** Zellen absuchen, die das
///   abgefragte Rechteck belegt. Die Mittelpunktsregel trägt für Berührung
///   nicht: ein Zeichen, das nur mit seiner rechten Kante in einen
///   Schwärzungsbereich ragt, hat seinen Mittelpunkt weit außerhalb.
///
/// Beide teilen sich Zellenmaß, Eintragung und die Behandlung unbrauchbarer
/// Koordinaten; nur die Zellenauswahl der Abfrage unterscheidet sich. Die
/// Zusicherungen `const _: () = assert!(… >= 0.5)` weiter oben gehören allein
/// zur ersten Abfrage und werden von der zweiten weder gebraucht noch berührt.
///
/// ## Warum die erste Abfrage nur eine einzige Zelle ansehen muss
///
/// Gefragt ist dort immer dasselbe: Gibt es ein eingetragenes Rechteck, das
/// ein gegebenes Rechteck zu mindestens `t` überdeckt? Für `t >= 0.5` genügt
/// es, die Zelle des **Mittelpunkts** abzusuchen.
///
/// Beweis in einem Satz: Läge der Mittelpunkt von `self` nicht in `other`,
/// dann endete `other` vor der Mitte einer der beiden Achsen, die Schnittmenge
/// läge ganz in einer Hälfte von `self` und wäre höchstens halb so groß —
/// Überdeckung unter 0,5. Wer also mindestens die Hälfte überdeckt, enthält
/// zwingend den Mittelpunkt, steht damit in dessen Zelle und wird gefunden.
///
/// Genau daran hängen die beiden `const _: () = assert!(… >= 0.5)` weiter
/// oben: Eine kleinere Schwelle würde diese Suche unvollständig machen — und
/// zwar lautlos, weil ein nicht gefundener Negativeintrag einfach nicht
/// blockiert. Deshalb bricht der Bau, statt still falsch zu rechnen.
///
/// Der Beweis gilt in exakter Arithmetik. Auf genau der Schwelle 0,5 bleibt
/// eine Haaresbreite: liegt der Mittelpunkt außerhalb, ist die wahre
/// Überdeckung echt kleiner als 0,5, der berechnete Quotient könnte aber auf
/// 0,5 aufrunden. Dazu müssten sich die Flächen um weniger als ein
/// Gleitkomma-ULP unterscheiden — auf einer PDF-Seite kein darstellbarer
/// Unterschied. Und die Folge wäre, dass geschwärzt statt blockiert wird,
/// nicht umgekehrt.
///
/// ## Was das Gitter nicht rettet
///
/// Liegen sehr viele Rechtecke in derselben Zelle **und** überdeckt keines das
/// andere zu 99,9 %, dann durchsucht jede Abfrage sie alle — quadratisch wie
/// zuvor, plus der Aufwand fürs Eintragen.
///
/// Gemessen, je 40 000 Treffer auf einer Seite, `--release`, bestes von drei
/// Läufen; „vorher“ ist der Streifenzug:
///
/// | Anordnung                                             | vorher   | nachher  |
/// |-------------------------------------------------------|---------:|---------:|
/// | Spalte (gleiche x-Spanne, verschiedene y)              |  9,757 s |  0,057 s |
/// | Gitter (in x nebeneinander)                            |  0,093 s |  0,047 s |
/// | gemischt (90 % Schnipsel, 10 % seitenbreite Rechtecke)  |  5,238 s |  0,046 s |
/// | deckungsgleich (n-mal dasselbe Rechteck)                |  0,047 s |  0,040 s |
/// | Kreuz (gleicher Mittelpunkt, breit-flach/schmal-hoch)   |  0,418 s |  0,548 s |
/// | Haufen (gleich groß, um 0,02 pt gegeneinander versetzt) | 15,658 s | 17,085 s |
///
/// Die letzten beiden Zeilen sind der ehrliche Rest — dort ist die neue
/// Fassung sogar 9 bis 31 % **langsamer**, weil das Eintragen ins Gitter
/// bezahlt, aber nichts einspart.
///
/// Der **Haufen** ist der teuerste Fall: n gleich große Rechtecke, jeweils um
/// Bruchteile eines Punktes versetzt. Sie überdecken einander zu 99,8 % —
/// knapp zu wenig, um wegzufallen —, haben fast denselben Mittelpunkt und
/// liegen deshalb alle in derselben Zelle. Konstruieren lässt sich das; ein
/// Kontoauszug erzeugt es nicht: dort sind die Treffer entweder
/// deckungsgleich (dann fällt bis auf einen alles weg) oder um mindestens eine
/// Zeilenhöhe versetzt (dann trennt sie das Gitter).
pub struct RectGrid {
    origin_x: f64,
    origin_y: f64,
    cell_w: f64,
    cell_h: f64,
    cells: HashMap<(i64, i64), Vec<usize>>,
    /// Rechtecke, die zu viele Zellen belegen würden oder deren Koordinaten
    /// unbrauchbar sind (NaN, unendlich). Sie werden bei jeder Abfrage
    /// mitgeprüft — das ist immer korrekt und nur dann teuer, wenn es viele
    /// sind.
    everywhere: Vec<usize>,
    /// Alle eingetragenen Indizes in Eintragungsreihenfolge. Gebraucht wird
    /// die Liste nur als Rückfallebene von [`RectGrid::touching_into`]: ein
    /// abgefragtes Rechteck, das selbst zu viele Zellen belegt, wird gegen
    /// alles geprüft statt gegen eine Nachbarschaft. Ein `usize` je Eintrag
    /// neben den bis zu 64 Zelleneinträgen desselben Rechtecks.
    inserted: Vec<usize>,
}

/// Nur endliche Koordinaten lassen sich auf Zellen abbilden.
fn usable(rect: &Rect) -> bool {
    rect.ll.x.is_finite() && rect.ll.y.is_finite() && rect.ur.x.is_finite() && rect.ur.y.is_finite()
}

impl RectGrid {
    /// Legt die Zellengröße anhand der Rechtecke fest, die später hineinsollen.
    ///
    /// Maßstab ist der Mittelwert der Kantenlängen: Zellen kleiner als die
    /// typische Region würden jede Region über viele Zellen verteilen, Zellen
    /// deutlich größer würden alles in dieselbe Zelle legen. Der Mittelwert
    /// wächst mit, wenn viele große Rechtecke dabei sind, und lässt sich von
    /// einem einzelnen Ausreißer kaum verschieben.
    pub fn new<'a>(rects: impl Iterator<Item = &'a Rect>) -> Self {
        let mut count = 0usize;
        let mut sum_w = 0.0;
        let mut sum_h = 0.0;
        let mut origin_x = f64::INFINITY;
        let mut origin_y = f64::INFINITY;
        for rect in rects {
            if !usable(rect) {
                continue;
            }
            count += 1;
            sum_w += rect.width().abs();
            sum_h += rect.height().abs();
            origin_x = origin_x.min(rect.ll.x);
            origin_y = origin_y.min(rect.ll.y);
        }
        let mean = |sum: f64| {
            if count == 0 {
                MIN_CELL
            } else {
                (sum / count as f64).max(MIN_CELL)
            }
        };
        Self {
            origin_x: if origin_x.is_finite() { origin_x } else { 0.0 },
            origin_y: if origin_y.is_finite() { origin_y } else { 0.0 },
            cell_w: mean(sum_w),
            cell_h: mean(sum_h),
            cells: HashMap::new(),
            everywhere: Vec::new(),
            inserted: Vec::new(),
        }
    }

    /// Leert den Inhalt, behält aber die Zellengröße (Seitenwechsel).
    pub fn clear(&mut self) {
        self.cells.clear();
        self.everywhere.clear();
        self.inserted.clear();
    }

    /// Zellenkoordinaten eines Punktes, noch als `f64`. Absichtlich nicht
    /// gleich `as i64`: die Umwandlung sättigt an den `i64`-Grenzen und würde
    /// die Spanne eines sehr weit außen liegenden Rechtecks zu klein ausweisen.
    /// Gesättigte Indizes selbst sind harmlos — dann teilen sich weit entfernte
    /// Rechtecke eine Zelle, das gibt mehr Kandidaten, nie weniger.
    fn floor_cell(&self, x: f64, y: f64) -> (f64, f64) {
        (
            ((x - self.origin_x) / self.cell_w).floor(),
            ((y - self.origin_y) / self.cell_h).floor(),
        )
    }

    pub fn insert(&mut self, idx: usize, rect: &Rect) {
        self.inserted.push(idx);
        if !usable(rect) {
            self.everywhere.push(idx);
            return;
        }
        // Bewusst über min/max statt über `ll`/`ur`: `Region::new`
        // normalisiert zwar, aber die Felder sind öffentlich. Ein verdrehtes
        // Rechteck darf höchstens zu viele Zellen belegen, nie zu wenige.
        let (lx, hx) = (rect.ll.x.min(rect.ur.x), rect.ll.x.max(rect.ur.x));
        let (ly, hy) = (rect.ll.y.min(rect.ur.y), rect.ll.y.max(rect.ur.y));
        let (x0, y0) = self.floor_cell(lx, ly);
        let (x1, y1) = self.floor_cell(hx, hy);
        // Beide Faktoren sind mindestens 1 (lx <= hx, ly <= hy), das Produkt
        // also nie NaN — überläuft es nach unendlich, greift genau diese
        // Schranke.
        if (x1 - x0 + 1.0) * (y1 - y0 + 1.0) > MAX_CELLS_PER_RECT {
            self.everywhere.push(idx);
            return;
        }
        for x in x0 as i64..=x1 as i64 {
            for y in y0 as i64..=y1 as i64 {
                self.cells.entry((x, y)).or_default().push(idx);
            }
        }
    }

    /// Alle Einträge, die `rect` zu mindestens der Hälfte überdecken *könnten*
    /// — ohne Wiederholungen, aber ungeordnet. Wer es genauer braucht, prüft
    /// die gelieferten Kandidaten selbst nach.
    ///
    /// **Nur für Schwellen ab 0,5 vollständig** (siehe Mittelpunktsbeweis am
    /// Typ). Wer nach bloßer *Berührung* fragt, nimmt [`RectGrid::touching_into`].
    pub fn candidates_covering<'a>(&'a self, rect: &Rect) -> impl Iterator<Item = usize> + 'a {
        const NONE: &[usize] = &[];
        let center = rect.center();
        let cell = if center.x.is_finite() && center.y.is_finite() {
            let (x, y) = self.floor_cell(center.x, center.y);
            self.cells
                .get(&(x as i64, y as i64))
                .map_or(NONE, Vec::as_slice)
        } else {
            // Ein Rechteck mit unbrauchbaren Koordinaten hat keine Fläche im
            // Sinne von `covered_fraction`; keine Prüfung könnte zutreffen.
            NONE
        };
        cell.iter().chain(self.everywhere.iter()).copied()
    }

    /// Alle Einträge, die `rect` **berühren** könnten — ohne Wiederholungen,
    /// aber ungeordnet, geschrieben nach `out`.
    ///
    /// `out` wird zuvor geleert und ist als wiederverwendeter Puffer gedacht:
    /// diese Abfrage läuft je Zeichen einer Seite, nicht je Seite.
    ///
    /// ## Warum hier alle Zellen abgesucht werden
    ///
    /// Berührung heißt: die Rechtecke haben mindestens einen Punkt `p`
    /// gemeinsam (Kante eingeschlossen). Beide belegen dann die Zelle von `p`
    /// — das eingetragene, weil [`RectGrid::insert`] es in **alle** von ihm
    /// belegten Zellen schreibt, und das abgefragte, weil hier ebenfalls über
    /// alle belegten Zellen gelaufen wird. Der Mittelpunktsbeweis von
    /// [`RectGrid::candidates_covering`] wird dafür weder gebraucht noch
    /// abgeschwächt: er gilt nur für Überdeckung ab der Hälfte, und ein bloß
    /// mit der Kante hineinragendes Rechteck erfüllt das gerade nicht.
    ///
    /// Belegt das **abgefragte** Rechteck selbst zu viele Zellen (ein
    /// riesenhaftes Zeichen bei winzigen Bereichen), wäre das Absuchen teurer
    /// als das Prüfen: dann wird alles Eingetragene geliefert. Das ist immer
    /// korrekt, nur langsamer.
    pub fn touching_into(&self, rect: &Rect, out: &mut Vec<usize>) {
        out.clear();
        if !usable(rect) {
            // Ein Rechteck mit unbrauchbaren Koordinaten berührt nichts:
            // jeder Vergleich mit NaN ist falsch. Die Einträge aus
            // `everywhere` bleiben trotzdem dabei — dort steht auch, was
            // selbst unbrauchbare Koordinaten hat.
            out.extend_from_slice(&self.everywhere);
            return;
        }
        let (lx, hx) = (rect.ll.x.min(rect.ur.x), rect.ll.x.max(rect.ur.x));
        let (ly, hy) = (rect.ll.y.min(rect.ur.y), rect.ll.y.max(rect.ur.y));
        let (x0, y0) = self.floor_cell(lx, ly);
        let (x1, y1) = self.floor_cell(hx, hy);
        if (x1 - x0 + 1.0) * (y1 - y0 + 1.0) > MAX_CELLS_PER_RECT {
            // `inserted` enthält jeden Eintrag genau einmal, `everywhere` ist
            // eine Teilmenge davon — hier ist nichts zu entdoppeln.
            out.extend_from_slice(&self.inserted);
            return;
        }
        let mut cells = 0usize;
        for x in x0 as i64..=x1 as i64 {
            for y in y0 as i64..=y1 as i64 {
                if let Some(cell) = self.cells.get(&(x, y)) {
                    out.extend_from_slice(cell);
                    cells += 1;
                }
            }
        }
        // Ein Eintrag kann in mehreren der abgesuchten Zellen stehen — aber
        // nur, wenn es überhaupt mehrere waren. Der Normalfall ist eine
        // einzige Zelle, und dann wäre Sortieren reine Arbeit ohne Wirkung.
        if cells > 1 {
            out.sort_unstable();
            out.dedup();
        }
        // Was in `everywhere` steht, steht in keiner Zelle.
        out.extend_from_slice(&self.everywhere);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{MatchType, Region};

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

    #[test]
    fn negative_list_blocks_overlapping_hit() {
        let regions = vec![
            pattern_region(0, Rect::new(10.0, 10.0, 50.0, 20.0)),
            negative_region(0, Rect::new(0.0, 0.0, 100.0, 30.0)),
        ];
        let res = resolve_conflicts(regions);
        assert!(res.redact.is_empty());
        assert_eq!(res.blocked.len(), 1);
        assert_eq!(res.blocked[0].booking_id, "b003");
    }

    #[test]
    fn negative_list_on_other_page_does_not_block() {
        let regions = vec![
            pattern_region(1, Rect::new(10.0, 10.0, 50.0, 20.0)),
            negative_region(0, Rect::new(0.0, 0.0, 100.0, 30.0)),
        ];
        let res = resolve_conflicts(regions);
        assert_eq!(res.redact.len(), 1);
        assert!(res.blocked.is_empty());
    }

    #[test]
    fn manual_region_overrides_negative_list() {
        let manual = Region::new(
            0,
            Rect::new(10.0, 10.0, 50.0, 20.0),
            None,
            Source::Manual {
                reason: "Gehalt".into(),
            },
        );
        let res = resolve_conflicts(vec![
            manual,
            negative_region(0, Rect::new(0.0, 0.0, 100.0, 30.0)),
        ]);
        assert_eq!(res.redact.len(), 1);
        assert!(res.blocked.is_empty());
    }

    #[test]
    fn barely_touching_negative_does_not_block() {
        // Nur 20% Überdeckung → Treffer bleibt bestehen.
        let regions = vec![
            pattern_region(0, Rect::new(0.0, 0.0, 100.0, 10.0)),
            negative_region(0, Rect::new(80.0, 0.0, 200.0, 10.0)),
        ];
        let res = resolve_conflicts(regions);
        assert_eq!(res.redact.len(), 1);
    }

    // ---------------------------------------------------------------------
    // Die beiden Schwellen. Vorher waren sie nur grob eingerahmt: 20 % gegen
    // „ganz enthalten“ bzw. „deckungsgleich“ gegen „gar nicht überlappend“.
    // Man konnte BLOCK_COVERAGE_THRESHOLD auf 0,99 und CONTAINMENT_THRESHOLD
    // auf 0,5 setzen, ohne dass ein einziger Test rot wurde — obwohl das eine
    // Schwärzungen verhindert und das andere Schwärzungen wegwirft.
    // ---------------------------------------------------------------------

    #[test]
    fn negativliste_blockiert_ab_sechzig_prozent() {
        // 60 % des Treffers liegen unter dem Negativeintrag → blockiert.
        // Hält BLOCK_COVERAGE_THRESHOLD nach oben (fängt z.B. 0,99).
        let hit = Rect::new(0.0, 0.0, 100.0, 10.0);
        let neg = Rect::new(40.0, 0.0, 200.0, 10.0);
        assert!((hit.covered_fraction(&neg) - 0.6).abs() < 1e-9);

        let res = resolve_conflicts(vec![pattern_region(0, hit), negative_region(0, neg)]);
        assert!(
            res.redact.is_empty(),
            "60 % überdeckt: der Treffer muss blockiert sein"
        );
        assert_eq!(res.blocked.len(), 1);
    }

    #[test]
    fn negativliste_blockiert_bei_vierzig_prozent_noch_nicht() {
        // Gegenprobe zu den 60 %. Nach unten hält die Schwelle in erster Linie
        // die `const`-Zusicherung oben: alles unter 0,5 übersetzt gar nicht.
        // Dieser Test hält die Stelle zusätzlich fest — allein könnte er es
        // nicht, denn unter 0,5 wäre auch die Gittersuche unvollständig und
        // die Frage würde nie gestellt.
        let hit = Rect::new(0.0, 0.0, 100.0, 10.0);
        let neg = Rect::new(60.0, 0.0, 200.0, 10.0);
        assert!((hit.covered_fraction(&neg) - 0.4).abs() < 1e-9);

        let res = resolve_conflicts(vec![pattern_region(0, hit), negative_region(0, neg)]);
        assert_eq!(
            res.redact.len(),
            1,
            "40 % überdeckt: der Treffer muss bestehen bleiben"
        );
        assert!(res.blocked.is_empty());
    }

    #[test]
    fn eine_zu_sechzig_prozent_ueberdeckte_region_bleibt() {
        // Zwei Treffer derselben Herkunft, der zweite ragt zu 40 % heraus.
        // Würde er wegfallen, bliebe dieser Teil ungeschwärzt stehen.
        // Hält CONTAINMENT_THRESHOLD nach unten (fängt z.B. 0,5).
        let big = Rect::new(0.0, 0.0, 100.0, 10.0);
        let partly = Rect::new(40.0, 0.0, 140.0, 10.0);
        assert!((partly.covered_fraction(&big) - 0.6).abs() < 1e-9);

        let res = resolve_conflicts(vec![pattern_region(0, big), pattern_region(0, partly)]);
        assert_eq!(
            res.redact.len(),
            2,
            "nur teilweise überdeckt: beide bleiben"
        );
    }

    #[test]
    fn eine_zu_neunundneunzig_prozent_ueberdeckte_region_bleibt() {
        // Ein Prozent Fläche steht heraus — bei einer Schwärzung ist das der
        // Unterschied zwischen „unleserlich“ und „lesbarer Rest“.
        // Hält CONTAINMENT_THRESHOLD scharf nach unten (fängt z.B. 0,99).
        let big = Rect::new(0.0, 0.0, 1000.0, 10.0);
        let sticking_out = Rect::new(0.0, 0.0, 1010.0, 10.0);
        let covered = sticking_out.covered_fraction(&big);
        assert!((covered - 0.990099).abs() < 1e-5, "{covered}");

        // Beide beginnen bei x=0, die Eingabereihenfolge entscheidet also über
        // die Bearbeitungsreihenfolge — hier zuerst die größere.
        let res = resolve_conflicts(vec![
            pattern_region(0, big),
            pattern_region(0, sticking_out),
        ]);
        assert_eq!(res.redact.len(), 2, "99 % ist nicht ganz");
    }

    #[test]
    fn eine_praktisch_deckungsgleiche_region_faellt_weg() {
        // Gegenprobe: 99,95 % Überdeckung ist Rundungsrest, nicht Inhalt.
        // Hält CONTAINMENT_THRESHOLD nach oben (fängt z.B. 0,99999).
        let big = Rect::new(0.0, 0.0, 1000.0, 10.0);
        let rounding = Rect::new(0.0, 0.0, 1000.5, 10.0);
        let covered = rounding.covered_fraction(&big);
        assert!((covered - 0.9995).abs() < 1e-6, "{covered}");

        let res = resolve_conflicts(vec![pattern_region(0, big), pattern_region(0, rounding)]);
        assert_eq!(res.redact.len(), 1);
        assert_eq!(res.redact[0].rect, big, "die zuerst bearbeitete bleibt");
    }

    #[test]
    fn duplicates_are_removed() {
        let r = Rect::new(10.0, 10.0, 50.0, 20.0);
        let res = resolve_conflicts(vec![pattern_region(0, r), pattern_region(0, r)]);
        assert_eq!(res.redact.len(), 1);
    }

    #[test]
    fn a_contained_region_falls_away_in_either_order() {
        let big = Rect::new(0.0, 0.0, 100.0, 20.0);
        let small = Rect::new(10.0, 5.0, 30.0, 15.0);

        // Beide Reihenfolgen müssen dasselbe ergeben — genau das konnte die
        // frühere Fassung nicht: kam die kleine zuerst, blieben beide stehen.
        for pair in [[big, small], [small, big]] {
            let res =
                resolve_conflicts(vec![pattern_region(0, pair[0]), pattern_region(0, pair[1])]);
            assert_eq!(res.redact.len(), 1, "Reihenfolge {pair:?}");
            assert_eq!(res.redact[0].rect, big, "die größere bleibt");
        }
    }

    #[test]
    fn a_neighbour_that_merely_touches_is_kept() {
        // Grenzfall: das zweite Rechteck beginnt genau dort, wo das erste
        // endet. Es ist nicht enthalten und muss bleiben.
        let res = resolve_conflicts(vec![
            pattern_region(0, Rect::new(0.0, 0.0, 50.0, 10.0)),
            pattern_region(0, Rect::new(50.0, 0.0, 100.0, 10.0)),
        ]);
        assert_eq!(res.redact.len(), 2);
    }

    #[test]
    fn regions_of_different_origin_do_not_swallow_each_other() {
        let big = Rect::new(0.0, 0.0, 100.0, 20.0);
        let small = Rect::new(10.0, 5.0, 30.0, 15.0);
        let manual = Region::new(
            0,
            small,
            None,
            Source::Manual {
                reason: "von Hand".into(),
            },
        );
        let res = resolve_conflicts(vec![pattern_region(0, big), manual]);
        assert_eq!(res.redact.len(), 2, "andere Herkunft, andere Begründung");
    }

    #[test]
    fn eine_region_ohne_flaeche_faellt_nur_weg_wenn_ihr_mittelpunkt_drin_liegt() {
        // Entartete Rechtecke (Leerzeichen ohne Höhe) beantwortet
        // `covered_fraction` über den Mittelpunkt. Das Gitter muss sie
        // trotzdem finden.
        let big = pattern_region(0, Rect::new(0.0, 0.0, 100.0, 10.0));
        let inside = pattern_region(0, Rect::new(40.0, 5.0, 60.0, 5.0));
        let outside = pattern_region(0, Rect::new(40.0, 50.0, 60.0, 50.0));
        let res = resolve_conflicts(vec![big, inside, outside.clone()]);
        assert_eq!(res.redact.len(), 2);
        assert_eq!(res.redact[1].rect, outside.rect);
    }

    // ---------------------------------------------------------------------
    // Laufzeit
    // ---------------------------------------------------------------------

    /// Beide Anordnungen, die auf einer Seite wirklich vorkommen — und beide
    /// müssen schnell sein.
    ///
    /// Das **Gitter** (Treffer liegen in x nebeneinander) war die einzige
    /// Anordnung, die der frühere Test geprüft hat; es ist genau die, die dem
    /// alten Streifenzug nicht wehtat.
    ///
    /// Die **Spalte** (gleiche x-Spanne, verschiedene y) ist die Anordnung,
    /// die eine IBAN- oder Kontonummernspalte auf einem Kontoauszug erzeugt.
    /// Für sie war der Streifenzug weiterhin quadratisch: gemessen 66,7 s für
    /// 100 000 Treffer, 8,4 s für 40 000.
    ///
    /// Bewusst großzügige Schranke: sie soll eine Rückkehr des quadratischen
    /// Verhaltens fangen, nicht auf langsamer Hardware grundlos rot werden.
    /// Quadratisch wären es hier Minuten.
    #[test]
    fn zwanzigtausend_treffer_je_anordnung_bleiben_weit_unter_einer_sekunde() {
        let n = 20_000;
        let gitter: Vec<Region> = (0..n)
            .map(|i| {
                let x = (i % 100) as f64 * 6.0;
                let y = (i / 100) as f64 * 12.0;
                pattern_region(0, Rect::new(x, y, x + 5.0, y + 10.0))
            })
            .collect();
        // Gleiche x-Spanne, nur y unterscheidet sich.
        let spalte: Vec<Region> = (0..n)
            .map(|i| {
                let y = i as f64 * 12.0;
                pattern_region(0, Rect::new(100.0, y, 220.0, y + 10.0))
            })
            .collect();

        for (name, regions) in [("Gitter", gitter), ("Spalte", spalte)] {
            let start = std::time::Instant::now();
            let res = resolve_conflicts(regions);
            let elapsed = start.elapsed();

            assert_eq!(
                res.redact.len(),
                n,
                "{name}: keine überlappt, keine fällt weg"
            );
            assert!(
                elapsed < std::time::Duration::from_secs(5),
                "{name}: Konfliktauflösung brauchte {elapsed:?} — das riecht wieder nach O(n²)"
            );
        }
    }

    /// Dieselbe Frage für die Negativliste: 20 000 Negativeinträge und 20 000
    /// Kandidaten auf einer Seite liefen früher gegeneinander — die
    /// Vorsortierung nach Seite half nicht, weil beides auf derselben Seite
    /// steht. Mit dem Paarvergleich dauert das hier rund 15 s.
    #[test]
    fn zwanzigtausend_negativeintraege_gegen_zwanzigtausend_treffer_bleiben_schnell() {
        let n = 20_000;
        let mut regions: Vec<Region> = (0..n)
            .map(|i| {
                let y = i as f64 * 12.0;
                negative_region(0, Rect::new(300.0, y, 420.0, y + 10.0))
            })
            .collect();
        regions.extend((0..n).map(|i| {
            let y = i as f64 * 12.0;
            pattern_region(0, Rect::new(100.0, y, 220.0, y + 10.0))
        }));

        let start = std::time::Instant::now();
        let res = resolve_conflicts(regions);
        let elapsed = start.elapsed();

        assert_eq!(res.redact.len(), n, "keiner der Treffer wird überdeckt");
        assert!(res.blocked.is_empty());
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "Negativliste brauchte {elapsed:?} — das riecht wieder nach O(n²)"
        );
    }

    // ---------------------------------------------------------------------
    // Gleichheitsnachweis gegen eine unabhängige, absichtlich langsame
    // Referenz. Das Gitter darf schneller sein — anders entscheiden darf es
    // nicht, denn `dedup` bestimmt, welche Schwärzungen wegfallen.
    // ---------------------------------------------------------------------

    /// Die Regeln aus der Dokumentation, ohne Gitter und ohne Streifen: jeder
    /// gegen jeden.
    fn referenz(regions: Vec<Region>) -> Resolution {
        let (negatives, candidates): (Vec<Region>, Vec<Region>) =
            regions.into_iter().partition(|r| r.is_blocking());

        let mut result = Resolution::default();
        'outer: for cand in candidates {
            if !matches!(cand.source, Source::Manual { .. }) {
                for neg in negatives.iter().filter(|n| n.page == cand.page) {
                    if cand.rect.covered_fraction(&neg.rect) >= BLOCK_COVERAGE_THRESHOLD {
                        result.blocked.push(BlockedRegion {
                            page: cand.page,
                            rect: cand.rect,
                            pattern: neg.text.clone().unwrap_or_default(),
                            booking_id: match &neg.source {
                                Source::Booking { booking_id, .. } => booking_id.clone(),
                                _ => String::new(),
                            },
                            blocked_reason: Some(cand.reason()),
                        });
                        continue 'outer;
                    }
                }
            }
            result.redact.push(cand);
        }

        let keep = &mut result.redact;
        let mut order: Vec<usize> = (0..keep.len()).collect();
        order.sort_by(|&a, &b| {
            keep[a].page.cmp(&keep[b].page).then_with(|| {
                keep[a]
                    .rect
                    .ll
                    .x
                    .partial_cmp(&keep[b].rect.ll.x)
                    .unwrap_or(Ordering::Equal)
                    .then(a.cmp(&b))
            })
        });
        let mut redundant = vec![false; keep.len()];
        let mut survivors: Vec<usize> = Vec::new();
        for &i in &order {
            let covered = survivors.iter().any(|&k| {
                keep[k].page == keep[i].page
                    && keep[k].source == keep[i].source
                    && keep[i].rect.covered_fraction(&keep[k].rect) >= CONTAINMENT_THRESHOLD
            });
            if covered {
                redundant[i] = true;
            } else {
                survivors.push(i);
            }
        }
        let mut out: Vec<Region> = Vec::new();
        for (i, region) in keep.drain(..).enumerate() {
            if !redundant[i] {
                out.push(region);
            }
        }
        result.redact = out;
        result
    }

    /// xorshift64* — fester Startwert, damit ein Fehlschlag reproduzierbar ist.
    struct Rng(u64);

    impl Rng {
        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next_u64() % n
        }
        fn coord(&mut self, steps: u64, scale: f64) -> f64 {
            self.below(steps) as f64 * scale
        }
    }

    /// Eine zufällige Anordnung. `mode` steuert, wie bösartig sie ist —
    /// enthalten sind alle Formen, die dem Gitter oder dem früheren
    /// Streifenzug wehtun.
    fn zufallsfall(rng: &mut Rng, n: usize, mode: u64) -> Vec<Region> {
        (0..n)
            .map(|_| {
                let page = rng.below(3) as usize;
                let (x, y, w, h) = match mode {
                    // Spalte: gleiche x-Spanne.
                    0 => (100.0, rng.coord(40, 5.0), 120.0, 10.0),
                    // Zeile: gleiche y-Spanne.
                    1 => (rng.coord(40, 5.0), 50.0, 120.0, 10.0),
                    // Alles im selben kleinen Rechteck: beide Achsen entartet.
                    2 => (rng.coord(4, 1.0), rng.coord(4, 1.0), 8.0, 8.0),
                    // Verschachtelt um denselben Mittelpunkt.
                    3 => {
                        let k = rng.coord(8, 1.0) + 1.0;
                        (50.0 - k, 50.0 - k, 2.0 * k, 2.0 * k)
                    }
                    // Kreuzform: breit-flach gegen schmal-hoch, gleicher
                    // Mittelpunkt — der Fall, den auch das Gitter nicht rettet.
                    4 => {
                        let k = rng.coord(8, 1.0) + 1.0;
                        (50.0 - k, 50.0 - 9.0 / k, 2.0 * k, 18.0 / k)
                    }
                    // Entartet: Nullfläche (Leerzeichen ohne Höhe).
                    5 => (
                        rng.coord(10, 2.0),
                        rng.coord(10, 2.0),
                        rng.coord(2, 5.0),
                        0.0,
                    ),
                    // Stark unterschiedliche Größen, breit gestreut.
                    _ => (
                        rng.coord(50, 4.0),
                        rng.coord(50, 4.0),
                        1.0 + rng.coord(20, 3.0),
                        1.0 + rng.coord(20, 3.0),
                    ),
                };
                let rect = Rect::new(x, y, x + w, y + h);
                match rng.below(10) {
                    0..=5 => pattern_region(page, rect),
                    6 => Region::new(
                        page,
                        rect,
                        Some("x".into()),
                        Source::Pattern {
                            pattern_id: "konto_nr".into(),
                            confidence: 0.8,
                        },
                    ),
                    7 => Region::new(
                        page,
                        rect,
                        None,
                        Source::Manual {
                            reason: "von Hand".into(),
                        },
                    ),
                    8 => Region::new(
                        page,
                        rect,
                        Some("pos".into()),
                        Source::Booking {
                            booking_id: "b001".into(),
                            match_type: MatchType::Positive,
                        },
                    ),
                    _ => negative_region(page, rect),
                }
            })
            .collect()
    }

    #[test]
    fn gitter_entscheidet_wie_die_referenz() {
        let mut rng = Rng(0x5EED_1234_ABCD_0001);
        for case in 0..400 {
            let n = 1 + rng.below(120) as usize;
            let mode = rng.below(8);
            let regions = zufallsfall(&mut rng, n, mode);

            let erwartet = referenz(regions.clone());
            let bekommen = resolve_conflicts(regions);

            assert_eq!(
                erwartet.redact, bekommen.redact,
                "Fall {case} (n={n}, mode={mode}): andere Schwärzungen als die Referenz"
            );
            assert_eq!(
                erwartet.blocked, bekommen.blocked,
                "Fall {case} (n={n}, mode={mode}): andere Blockierungen als die Referenz"
            );
        }
    }

    /// Die Berührungsabfrage darf nichts verlieren.
    ///
    /// Gegenprobe über alle Paare zufälliger Anordnungen: `touching_into` muss
    /// jedes Rechteck liefern, das sich mit dem abgefragten berührt — Kante
    /// eingeschlossen. Das ist die Eigenschaft, an der `redact_pdf::redact`
    /// hängt: dort entscheidet danach `covered_fraction` bzw. der Mittelpunkt,
    /// und beides setzt Berührung voraus.
    ///
    /// Bewusst **nicht** gegen `candidates_covering` geprüft: die
    /// Mittelpunktsregel gilt nur ab halber Überdeckung, für Berührung wäre
    /// sie unvollständig — genau deshalb gibt es zwei Abfragen.
    #[test]
    fn die_beruehrungsabfrage_verliert_nichts() {
        fn beruehrt(a: &Rect, b: &Rect) -> bool {
            a.ll.x <= b.ur.x && b.ll.x <= a.ur.x && a.ll.y <= b.ur.y && b.ll.y <= a.ur.y
        }

        let mut rng = Rng(0xB00C_1234_5678_9ABD);
        let mut out = Vec::new();
        for fall in 0..200 {
            let n = 1 + rng.below(40) as usize;
            // Mal grobe, mal feine Rechtecke: das Zellenmaß richtet sich nach
            // dem Eingetragenen, die Abfrage darf davon unabhängig sein.
            let scale = if fall % 3 == 0 { 0.5 } else { 7.0 };
            let rects: Vec<Rect> = (0..n)
                .map(|_| {
                    let x = rng.coord(40, scale);
                    let y = rng.coord(40, scale);
                    Rect::new(
                        x,
                        y,
                        x + rng.coord(6, scale) + 1.0,
                        y + rng.coord(6, scale) + 1.0,
                    )
                })
                .collect();
            let mut grid = RectGrid::new(rects.iter());
            for (i, rect) in rects.iter().enumerate() {
                grid.insert(i, rect);
            }

            for _ in 0..40 {
                let x = rng.coord(60, 1.0);
                let y = rng.coord(60, 1.0);
                // Auch entartete Abfragen (Breite oder Höhe 0) kommen vor —
                // ein Leerzeichen hat keine Fläche.
                let frage = Rect::new(x, y, x + rng.coord(4, 1.0), y + rng.coord(4, 1.0));
                grid.touching_into(&frage, &mut out);

                let mut geliefert: Vec<usize> = out
                    .iter()
                    .copied()
                    .filter(|&i| beruehrt(&rects[i], &frage))
                    .collect();
                geliefert.sort_unstable();
                let erwartet: Vec<usize> =
                    (0..n).filter(|&i| beruehrt(&rects[i], &frage)).collect();
                assert_eq!(geliefert, erwartet, "Fall {fall}, Abfrage {frage:?}");

                let mut ohne_wiederholung = out.clone();
                ohne_wiederholung.sort_unstable();
                ohne_wiederholung.dedup();
                assert_eq!(
                    ohne_wiederholung.len(),
                    out.len(),
                    "Fall {fall}: die Vorauswahl liefert Wiederholungen"
                );
            }
        }
    }
}
