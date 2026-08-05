//! Geometrie im PDF-User-Space (Punkt = 1/72 Zoll, Y-Achse zeigt nach oben).

use serde::{Deserialize, Serialize};

/// Koordinaten im PDF-User-Space (Punkt = 1/72 Zoll)
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// Achsenparalleles Rechteck. `ll` ist immer links-unten, `ur` rechts-oben.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub ll: Point, // lower-left
    pub ur: Point, // upper-right
}

impl Rect {
    /// Erzeugt ein normalisiertes Rechteck aus zwei beliebigen Eckpunkten.
    ///
    /// **Unbrauchbar hinein heißt unbrauchbar heraus.** `f64::min` und
    /// `f64::max` schlucken einen NaN-Operanden: aus `Rect::new(NaN, 0, 10, 10)`
    /// würde damit `ll.x == ur.x == 10`, also ein Punkt — und `--padding 1`
    /// machte daraus ein 2 x 2 Punkt großes Deck-Rechteck an einer Stelle, von
    /// der die Hälfte der Angabe unbrauchbar war. Das wäre eine stillschweigend
    /// **falsch platzierte** Schwärzung, gemeldet als „gezeichnet“.
    ///
    /// Deshalb bleibt NaN hier stehen und [`Rect::is_empty`] fängt das ganze
    /// Rechteck.
    ///
    /// Der Unterschied zu [`Rect::union`], die einen unbrauchbaren Operanden
    /// bewusst übergeht: dort ist die Hülle über **viele** Zeichen gemeint, und
    /// ein Zeichen unbekannter Lage darf die bekannten nicht mitreißen. Hier
    /// sind es die **zwei Ecken desselben** Rechtecks — ist eine davon
    /// unbrauchbar, ist es das Rechteck.
    pub fn from_corners(a: Point, b: Point) -> Self {
        Self {
            ll: Point::new(kleineres(a.x, b.x), kleineres(a.y, b.y)),
            ur: Point::new(groesseres(a.x, b.x), groesseres(a.y, b.y)),
        }
    }

    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self::from_corners(Point::new(x0, y0), Point::new(x1, y1))
    }

    /// Stellt sicher, dass `ll <= ur` gilt (JSON-Importe sind nicht vertrauenswürdig).
    pub fn normalized(&self) -> Self {
        Self::from_corners(self.ll, self.ur)
    }

    pub fn width(&self) -> f64 {
        self.ur.x - self.ll.x
    }

    pub fn height(&self) -> f64 {
        self.ur.y - self.ll.y
    }

    /// Sind alle vier Koordinaten endlich?
    ///
    /// Ein Rechteck, für das das nicht gilt, bezeichnet **keinen Bereich der
    /// Ebene**: NaN vergleicht sich mit nichts, und ±∞ hat keine ausrechenbare
    /// Breite (`inf - inf` ist NaN). Alles, was dieser Typ über Lage und Größe
    /// aussagt, ist für ein solches Rechteck gegenstandslos — siehe die Regel
    /// am Modulkopf von `redact-core/tests/unbrauchbare_koordinaten.rs`.
    ///
    /// Solche Koordinaten sind nicht theoretisch. Sie entstehen aus fremden
    /// PDFs (eine `cm`-Matrix mit einem Wert jenseits von `f32` wird beim
    /// Multiplizieren zu ∞ und mit einem Faktor 0 zu NaN), aus Review-Dateien
    /// und aus jedem `f64`, den der Aufrufende hereingibt — `--padding nan`
    /// genügt, und [`Rect::expanded`] macht daraus ein NaN-Rechteck.
    pub fn is_usable(&self) -> bool {
        self.ll.x.is_finite()
            && self.ll.y.is_finite()
            && self.ur.x.is_finite()
            && self.ur.y.is_finite()
    }

    /// Hat dieses Rechteck keine auswertbare Fläche?
    ///
    /// Das ist der Entartungsfilter: wer hier hängen bleibt, wird nicht
    /// gezeichnet (`redact_pdf::PdfRedactor`), bekommt keine Bildzone
    /// (`redact_pdf::image`) und gilt als wirkungslos
    /// (`redact_pipeline::EntryEffect::Degenerate`).
    ///
    /// **Unbrauchbare Koordinaten gehören ausdrücklich dazu.** Geschrieben als
    /// `width() <= 0.0` kamen sie durch, weil jeder Vergleich mit NaN falsch
    /// ist — ein Bereich mit NaN-Koordinaten galt damit als gültig, wurde als
    /// `NaN NaN NaN NaN re` in den Content-Stream geschrieben (kein Betrachter
    /// zeichnet das) und in der Zusammenfassung als „Deck-Rechteck gezeichnet“
    /// gemeldet. Hier wird die *Sicherheits*frage entschieden, die
    /// [`Rect::covered_fraction`] nicht entscheiden kann: was sich nicht
    /// auswerten lässt, wird nicht ausgewertet und auch nicht als Erfolg
    /// verbucht.
    pub fn is_empty(&self) -> bool {
        !self.is_usable() || self.width() <= 0.0 || self.height() <= 0.0
    }

    pub fn center(&self) -> Point {
        Point::new((self.ll.x + self.ur.x) / 2.0, (self.ll.y + self.ur.y) / 2.0)
    }

    /// Kleinstes Rechteck, das beide Rechtecke enthält.
    ///
    /// **Ein unbrauchbarer Operand wird stillschweigend übergangen** —
    /// `f64::min`/`max` schlucken NaN, die Hülle bleibt also die des anderen.
    /// Das ist hier die richtige Richtung und bleibt so: [`bounding_box`] und
    /// [`GlyphCursor`] bauen daraus die Hülle über *Zeichen*, und ein Zeichen,
    /// dessen Lage unbekannt ist, ließe sich ohnehin nicht überdecken. Würde
    /// die Unbrauchbarkeit stattdessen weitergereicht, wäre die Hülle über die
    /// ganze Zeile unbrauchbar — dann bliebe auch der Text stehen, dessen Lage
    /// bekannt ist.
    pub fn union(&self, other: &Rect) -> Rect {
        Rect {
            ll: Point::new(self.ll.x.min(other.ll.x), self.ll.y.min(other.ll.y)),
            ur: Point::new(self.ur.x.max(other.ur.x), self.ur.y.max(other.ur.y)),
        }
    }

    /// Vergrößert das Rechteck in alle Richtungen um `pad`.
    pub fn expanded(&self, pad: f64) -> Rect {
        Rect {
            ll: Point::new(self.ll.x - pad, self.ll.y - pad),
            ur: Point::new(self.ur.x + pad, self.ur.y + pad),
        }
    }

    /// Liegt `p` in diesem Rechteck (Rand eingeschlossen)?
    ///
    /// Für ein Rechteck ohne brauchbare Koordinaten immer `false`. Bei NaN kam
    /// das schon vorher heraus (jeder Vergleich mit NaN ist falsch); ±∞
    /// enthielte sonst jeden Punkt.
    pub fn contains(&self, p: Point) -> bool {
        self.is_usable()
            && p.x >= self.ll.x
            && p.x <= self.ur.x
            && p.y >= self.ll.y
            && p.y <= self.ur.y
    }

    /// Überlappen sich die beiden Rechtecke (Berührung zählt nicht)?
    ///
    /// Ein Rechteck ohne brauchbare Koordinaten überlappt nichts.
    pub fn intersects(&self, other: &Rect) -> bool {
        self.is_usable()
            && other.is_usable()
            && self.ll.x < other.ur.x
            && other.ll.x < self.ur.x
            && self.ll.y < other.ur.y
            && other.ll.y < self.ur.y
    }

    /// Fläche — `0.0` für alles, was [`Rect::is_empty`] als leer ansieht.
    pub fn area(&self) -> f64 {
        if !self.is_usable() {
            return 0.0;
        }
        (self.width().max(0.0)) * (self.height().max(0.0))
    }

    /// Fläche der Schnittmenge.
    ///
    /// # Warum die Prüfung ganz vorn steht
    ///
    /// `f64::min` und `f64::max` **schlucken** einen NaN-Operanden und liefern
    /// den anderen zurück. Ohne die Prüfung wird aus
    /// `self.ur.x.min(NaN) - self.ll.x.max(NaN)` genau `self.width()`, und die
    /// Rechnung meldet als Schnittfläche die **volle Fläche von `self`** —
    /// ein Rechteck mit unbrauchbaren Koordinaten schnitte damit alles.
    pub fn intersection_area(&self, other: &Rect) -> f64 {
        if !self.is_usable() || !other.is_usable() {
            return 0.0;
        }
        let w = (self.ur.x.min(other.ur.x) - self.ll.x.max(other.ll.x)).max(0.0);
        let h = (self.ur.y.min(other.ur.y) - self.ll.y.max(other.ll.y)).max(0.0);
        w * h
    }

    /// Anteil von `self`, der von `other` überdeckt wird (0.0 … 1.0).
    ///
    /// # Unbrauchbare Koordinaten sind `0.0`
    ///
    /// Nicht, weil das die sicherere Antwort wäre — das ist es nicht überall.
    /// Die Aufrufer ziehen in verschiedene Richtungen: in
    /// [`crate::resolve_conflicts`] blockiert „überdeckt alles“ eine Schwärzung
    /// (ein **Leck**), in `redact_pdf::hidden_flags` entfernte es den Text der
    /// ganzen Seite (**Datenverlust**). Eine Zahl kann diesen Streit nicht
    /// schlichten, und sie soll es auch nicht: hier wird **gemessen**, nicht
    /// entschieden.
    ///
    /// Gemessen ist `0.0` die richtige Antwort. Ein Rechteck ohne brauchbare
    /// Koordinaten bezeichnet keinen Bereich der Ebene, und die leere Menge
    /// überdeckt 0 % von allem. Die frühere `1.0` war kein Sicherheitsurteil,
    /// sondern ein Rechenfehler (siehe [`Rect::intersection_area`]).
    ///
    /// Entschieden wird die Sicherheitsfrage eine Ebene höher, von
    /// [`Rect::is_empty`]: was sich nicht auswerten lässt, wird gar nicht erst
    /// ausgewertet, nicht gezeichnet und als wirkungslos gemeldet.
    ///
    /// # Geprüft wird das Ergebnis, nicht die Eingabe
    ///
    /// Eine Prüfung auf unbrauchbare Eingaben stünde hier vergeblich:
    /// [`Rect::area`] und [`Rect::intersection_area`] beantworten sie bereits,
    /// und beide sind darauf geprüft. Was sie **nicht** abfangen, ist der
    /// Überlauf: ein Rechteck mit endlichen Koordinaten kann eine unendliche
    /// Fläche haben (Kantenlänge 1e308), und `∞ / ∞` ist NaN. Dann käme aus
    /// einer Funktion, die „0.0 … 1.0“ verspricht, eine Zahl, die weder größer
    /// noch kleiner als irgendeine Schwelle ist.
    ///
    /// Deshalb steht die Prüfung am Ergebnis. Sie fängt beide Wege dorthin —
    /// den über die Eingabe und den über die Rechnung — und ist die einzige,
    /// die sich nicht auf eine Zusicherung einer anderen Funktion verlässt.
    pub fn covered_fraction(&self, other: &Rect) -> f64 {
        let a = self.area();
        if a <= f64::EPSILON {
            // Entartete Rechtecke (z.B. Leerzeichen ohne Höhe): Mittelpunkt prüfen.
            return if other.contains(self.center()) {
                1.0
            } else {
                0.0
            };
        }
        let anteil = self.intersection_area(other) / a;
        if anteil.is_finite() {
            anteil
        } else {
            0.0
        }
    }
}

/// Ein einzelnes gesetztes Zeichen mit seiner Bounding-Box im User-Space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Glyph {
    pub ch: char,
    pub rect: Rect,
}

/// Ein zusammenhängender Text-Abschnitt einer Seite (Zeile oder Text-Run).
///
/// `glyphs` ist zeichenweise deckungsgleich mit `text.chars()`, dadurch lässt
/// sich für jeden Treffer eines Regex die exakte Bounding-Box berechnen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextRun {
    pub page: usize,
    pub text: String,
    pub glyphs: Vec<Glyph>,
    pub rect: Rect,
}

impl TextRun {
    pub fn new(page: usize, glyphs: Vec<Glyph>) -> Self {
        let text: String = glyphs.iter().map(|g| g.ch).collect();
        let rect = bounding_box(glyphs.iter().map(|g| &g.rect))
            .unwrap_or_else(|| Rect::new(0.0, 0.0, 0.0, 0.0));
        Self {
            page,
            text,
            glyphs,
            rect,
        }
    }

    /// Bounding-Box für einen Byte-Bereich von [`TextRun::text`].
    ///
    /// Gibt `None` zurück, wenn der Bereich leer ist oder außerhalb liegt.
    ///
    /// **Kostet einen Durchlauf durch den ganzen Lauf.** Wer viele Bereiche
    /// *desselben* Laufs braucht — das tut jeder, der Regex-Treffer in
    /// Rechtecke übersetzt —, nimmt [`TextRun::glyph_cursor`]; sonst wird aus
    /// M Treffern in einem Lauf aus G Glyphen ein Aufwand von G·M/2.
    pub fn rect_for_byte_range(&self, start: usize, end: usize) -> Option<Rect> {
        self.glyph_cursor().rect_for_byte_range(start, end)
    }

    /// Ein Zeiger, der für **mehrere** Byte-Bereiche desselben Laufs nur
    /// einmal durch die Glyphen wandert. Siehe [`GlyphCursor`].
    pub fn glyph_cursor(&self) -> GlyphCursor<'_> {
        GlyphCursor {
            glyphs: &self.glyphs,
            index: 0,
            byte: 0,
        }
    }
}

/// Ein Zeiger, der durch die Glyphen eines [`TextRun`] wandert und dabei
/// mitzählt, bei welchem Byte er steht.
///
/// # Warum es ihn gibt
///
/// [`TextRun::rect_for_byte_range`] zählt Byte für Byte von Glyphe 0 los. Für
/// *einen* Bereich ist das richtig und billig. Der Pattern-Matcher fragt aber
/// für **jeden** Treffer einer Zeile, und die Zeile hat auf einem
/// maschinell gesetzten Auszug schnell Hunderttausende Glyphen: bei M Treffern
/// in G Glyphen sind das G·M/2 Schritte. Gemessen an einer Zeile mit lauter
/// IBANs: 16 000 Treffer 1,2 s, 32 000 Treffer 6,1 s — Verdopplung der Eingabe,
/// Verfünffachung der Zeit. Dieselbe Zeichenzahl auf 13 784 Zeilen verteilt
/// kostete 0,5 s; es lag also an der Zeilen*länge*, nicht an der Glyphenzahl.
///
/// Der Zeiger macht daraus einen Durchlauf: aufsteigende Bereiche kosten
/// zusammen O(G) statt O(G·M).
///
/// # Warum er trotzdem nichts voraussetzt
///
/// `captures_iter` von `fancy-regex` liefert die Gesamttreffer (Gruppe 0)
/// aufsteigend und überschneidungsfrei — es sucht ab dem Ende des vorigen
/// Treffers weiter. Geschwärzt wird aber nicht der Gesamttreffer, sondern die
/// Gruppe `target`, und die darf in einem Look-around stehen. Dann liegt sie
/// außerhalb des Gesamttreffers, und die Bereiche laufen rückwärts. Ein
/// Muster, das das wirklich tut, steht in
/// `redact-patterns/tests/rect_cursor.rs`; über `--patterns-config` kann es
/// jeder mitbringen.
///
/// Deshalb setzt der Zeiger nichts voraus, sondern **prüft**: geht ein Bereich
/// hinter den Zeiger zurück, spult er auf Anfang. Das kostet dann genau so
/// viel wie vorher — und liefert dasselbe Rechteck. Eine schnelle falsche
/// Koordinate wäre hier das schlechteste Ergebnis: sie schwärzt den falschen
/// Text und lässt den richtigen stehen.
#[derive(Debug, Clone)]
pub struct GlyphCursor<'a> {
    glyphs: &'a [Glyph],
    /// Index des nächsten noch nicht gelesenen Glyphs.
    index: usize,
    /// Byte-Offset dieses Glyphs in [`TextRun::text`].
    byte: usize,
}

impl GlyphCursor<'_> {
    /// Bounding-Box für einen Byte-Bereich — Zeichen für Zeichen dasselbe wie
    /// [`TextRun::rect_for_byte_range`], nur ohne den Neuanfang bei Glyphe 0.
    ///
    /// Ein Glyph zählt genau dann dazu, wenn sein **Startbyte** in
    /// `start..end` liegt. Beginnt `start` mitten in einem Mehrbyte-Zeichen,
    /// gehört dieses Zeichen also nicht dazu — dieselbe Regel wie vorher.
    pub fn rect_for_byte_range(&mut self, start: usize, end: usize) -> Option<Rect> {
        if start >= end {
            return None;
        }
        // Rücklauf: der Bereich liegt vor dem Zeiger. Kommt bei den gelieferten
        // Mustern nicht vor, ist aber bei eigenen Mustern möglich (siehe oben).
        if self.byte > start {
            self.index = 0;
            self.byte = 0;
        }
        // Vorspulen bis zum ersten Glyph, dessen Startbyte nicht mehr vor
        // `start` liegt.
        while self.byte < start {
            let glyph = self.glyphs.get(self.index)?;
            self.byte += glyph.ch.len_utf8();
            self.index += 1;
        }
        let mut acc: Option<Rect> = None;
        while self.byte < end {
            let Some(glyph) = self.glyphs.get(self.index) else {
                break;
            };
            acc = Some(match acc {
                Some(r) => r.union(&glyph.rect),
                None => glyph.rect,
            });
            self.byte += glyph.ch.len_utf8();
            self.index += 1;
        }
        acc
    }
}

/// `f64::min`, aber ohne NaN zu schlucken — siehe [`Rect::from_corners`].
fn kleineres(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}

/// `f64::max`, aber ohne NaN zu schlucken — siehe [`Rect::from_corners`].
fn groesseres(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

/// Bounding-Box über eine Menge von Rechtecken.
pub fn bounding_box<'a, I: IntoIterator<Item = &'a Rect>>(rects: I) -> Option<Rect> {
    let mut it = rects.into_iter();
    let first = *it.next()?;
    Some(it.fold(first, |acc, r| acc.union(r)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_corners() {
        let r = Rect::from_corners(Point::new(10.0, 20.0), Point::new(0.0, 5.0));
        assert_eq!(r.ll, Point::new(0.0, 5.0));
        assert_eq!(r.ur, Point::new(10.0, 20.0));
    }

    #[test]
    fn intersection_and_coverage() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 5.0, 15.0, 15.0);
        assert!(a.intersects(&b));
        assert_eq!(a.intersection_area(&b), 25.0);
        assert_eq!(a.covered_fraction(&b), 0.25);
        let touching = Rect::new(10.0, 0.0, 20.0, 10.0);
        assert!(!a.intersects(&touching));
    }

    #[test]
    fn byte_range_bbox_covers_only_selected_glyphs() {
        let glyphs = vec![
            Glyph {
                ch: 'A',
                rect: Rect::new(0.0, 0.0, 5.0, 10.0),
            },
            Glyph {
                ch: 'B',
                rect: Rect::new(5.0, 0.0, 10.0, 10.0),
            },
            Glyph {
                ch: 'C',
                rect: Rect::new(10.0, 0.0, 15.0, 10.0),
            },
        ];
        let run = TextRun::new(0, glyphs);
        assert_eq!(run.text, "ABC");
        let r = run.rect_for_byte_range(1, 3).unwrap();
        assert_eq!(r, Rect::new(5.0, 0.0, 15.0, 10.0));
        assert!(run.rect_for_byte_range(2, 2).is_none());
    }

    #[test]
    fn byte_range_handles_multibyte_chars() {
        let glyphs = vec![
            Glyph {
                ch: 'ä',
                rect: Rect::new(0.0, 0.0, 5.0, 10.0),
            },
            Glyph {
                ch: 'x',
                rect: Rect::new(5.0, 0.0, 10.0, 10.0),
            },
        ];
        let run = TextRun::new(0, glyphs);
        // 'ä' belegt zwei Bytes, 'x' beginnt daher bei Byte 2.
        assert_eq!(
            run.rect_for_byte_range(2, 3).unwrap(),
            Rect::new(5.0, 0.0, 10.0, 10.0)
        );
    }

    // ---------------------------------------------------------------------
    // Der Gleichheitsnachweis für [`GlyphCursor`].
    //
    // Der Zeiger ersetzt eine Funktion, die bestimmt, **wo** das schwarze
    // Rechteck landet. Eine schnelle falsche Koordinate schwärzt den falschen
    // Text und lässt den richtigen stehen — deshalb steht hier nicht „ungefähr
    // gleich", sondern der Bit-Vergleich gegen die Fassung von vorher.
    // ---------------------------------------------------------------------

    /// **Der Stand von vorher, Zeile für Zeile.**
    ///
    /// Nicht [`TextRun::rect_for_byte_range`] aufrufen — die geht seit der
    /// Änderung selbst durch den Zeiger und wäre als Vergleichsmaßstab
    /// wertlos.
    fn referenz(run: &TextRun, start: usize, end: usize) -> Option<Rect> {
        if start >= end {
            return None;
        }
        let mut byte = 0usize;
        let mut acc: Option<Rect> = None;
        for glyph in &run.glyphs {
            let len = glyph.ch.len_utf8();
            if byte >= start && byte < end {
                acc = Some(match acc {
                    Some(r) => r.union(&glyph.rect),
                    None => glyph.rect,
                });
            }
            byte += len;
            if byte >= end {
                break;
            }
        }
        acc
    }

    /// Bitmuster statt `f64`-Vergleich: `==` auf `f64` sagt bei `-0.0` und
    /// `0.0` „gleich" und bei `NaN` „ungleich". Beides wäre hier die falsche
    /// Auskunft — verlangt ist *dasselbe Rechteck*, nicht *ein gleichwertiges*.
    fn bits(r: Option<Rect>) -> Option<[u64; 4]> {
        r.map(|r| {
            [
                r.ll.x.to_bits(),
                r.ll.y.to_bits(),
                r.ur.x.to_bits(),
                r.ur.y.to_bits(),
            ]
        })
    }

    /// Xorshift64* — reicht für Testdaten und braucht keine Abhängigkeit.
    struct Zufall(u64);

    impl Zufall {
        fn neu(saat: u64) -> Self {
            Self(saat | 1)
        }
        fn zahl(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        /// Gleichverteilt aus `0..n`.
        fn bis(&mut self, n: usize) -> usize {
            (self.zahl() % n.max(1) as u64) as usize
        }
    }

    /// Zeichen mit 1, 2, 3 und 4 Byte in UTF-8 — die Byte-Zählung ist genau
    /// die Stelle, an der ein Zeiger danebenliegen kann.
    const ZEICHEN: [char; 8] = ['a', '7', ' ', 'ä', 'ß', '€', 'ﬁ', '𝄞'];

    /// Baut einen Lauf, wie ihn `redact-pdf` liefert — einschließlich der
    /// Fälle, an denen ein Zeiger scheitern könnte.
    ///
    /// * **Ligaturen**: `redact-pdf` teilt eine Ligatur (ein Code, mehrere
    ///   Zeichen) in mehrere Glyphen auf und verteilt die Breite auf sie. Hier
    ///   sind das Gruppen aus zwei bis drei Zeichen, die sich eine Zelle
    ///   teilen — teils mit *identischem*, teils mit geteiltem Rechteck.
    /// * **Glyphen ohne Breite**: entartete Rechtecke kommen vor (Vorschub 0).
    /// * **Rücksprünge**: die Rechtecke laufen nicht durchgängig nach rechts.
    fn zufallslauf(r: &mut Zufall, glyphen: usize) -> TextRun {
        let mut g = Vec::with_capacity(glyphen);
        let mut x = 0.0f64;
        while g.len() < glyphen {
            // Ligaturgruppe aus 1..=3 Zeichen auf einer Zelle.
            let gruppe = 1 + r.bis(3);
            let breite = match r.bis(8) {
                0 => 0.0, // Glyph ohne Breite
                _ => 1.0 + (r.bis(400) as f64) / 100.0,
            };
            let teil = breite / gruppe as f64;
            for k in 0..gruppe {
                if g.len() >= glyphen {
                    break;
                }
                let ch = ZEICHEN[r.bis(ZEICHEN.len())];
                // Mal geteilte Zelle, mal identisches Rechteck für die ganze
                // Ligatur — beide Formen kommen aus dem Extraktor.
                let (x0, x1) = if r.bis(2) == 0 {
                    (x + teil * k as f64, x + teil * (k as f64 + 1.0))
                } else {
                    (x, x + breite)
                };
                let y = (r.bis(20) as f64) / 10.0;
                g.push(Glyph {
                    ch,
                    rect: Rect::new(x0, y, x1, y + 9.5),
                });
            }
            // Meist vorwärts, gelegentlich ein Rücksprung auf der Zeile.
            x += if r.bis(16) == 0 { -breite } else { breite };
        }
        TextRun::new(0, g)
    }

    /// **Der Gleichheitsnachweis.** Alter und neuer Weg über viele zufällige
    /// Läufe — bitgenau dasselbe Rechteck, und zwar auch dann, wenn die
    /// Bereiche nicht aufsteigen.
    ///
    /// Abgedeckt sind die Fälle, an denen sich ein Zeiger verzählt: Treffer am
    /// Anfang, am Ende, unmittelbar hintereinander, mit Mehrbyte-Zeichen, in
    /// Ligaturgruppen und an Byte-Grenzen mitten in einem Zeichen.
    #[test]
    fn der_zeiger_liefert_bitgenau_dieselben_rechtecke() {
        let mut r = Zufall::neu(0x5EED_1234_ABCD_0001);
        let mut geprueft = 0usize;
        let mut nicht_leer = 0usize;

        for lauf_nr in 0..300 {
            let glyphen = 1 + r.bis(60);
            let run = zufallslauf(&mut r, glyphen);
            let n = run.text.len();

            // (a) Alle Bereiche an Byte-Grenzen — auch die mitten in einem
            //     Mehrbyte-Zeichen, auch die leeren, auch die außerhalb.
            let mut zeiger = run.glyph_cursor();
            let mut voriger = 0usize;
            for start in 0..=n + 2 {
                for end in start..=n + 2 {
                    // Der Zeiger wird absichtlich mal aufsteigend, mal
                    // springend benutzt.
                    if start < voriger {
                        zeiger = run.glyph_cursor();
                    }
                    voriger = start;
                    let erwartet = referenz(&run, start, end);
                    let bekommen = zeiger.rect_for_byte_range(start, end);
                    assert_eq!(
                        bits(erwartet),
                        bits(bekommen),
                        "Lauf {lauf_nr} ({:?}), Bereich {start}..{end}",
                        run.text
                    );
                    geprueft += 1;
                    nicht_leer += usize::from(bekommen.is_some());
                }
            }

            // (b) Eine aufsteigende, überschneidungsfreie Folge — der Fall, für
            //     den der Zeiger gebaut ist: *ein* Zeiger für alle Bereiche.
            let mut zeiger = run.glyph_cursor();
            let mut pos = 0usize;
            while pos < n {
                let start = pos + r.bis(3);
                let end = (start + 1 + r.bis(8)).min(n + 1);
                let erwartet = referenz(&run, start, end);
                let bekommen = zeiger.rect_for_byte_range(start, end);
                assert_eq!(
                    bits(erwartet),
                    bits(bekommen),
                    "Lauf {lauf_nr} ({:?}), aufsteigend {start}..{end}",
                    run.text
                );
                geprueft += 1;
                nicht_leer += usize::from(bekommen.is_some());
                pos = end;
            }

            // (c) Und dieselbe Folge rückwärts — hier muss der Zeiger
            //     zurückspulen, sonst käme die falsche Koordinate heraus.
            let mut zeiger = run.glyph_cursor();
            let mut bereiche: Vec<(usize, usize)> = Vec::new();
            let mut pos = 0usize;
            while pos < n {
                let end = (pos + 1 + r.bis(6)).min(n);
                bereiche.push((pos, end));
                pos = end;
            }
            for (start, end) in bereiche.iter().rev() {
                let erwartet = referenz(&run, *start, *end);
                let bekommen = zeiger.rect_for_byte_range(*start, *end);
                assert_eq!(
                    bits(erwartet),
                    bits(bekommen),
                    "Lauf {lauf_nr} ({:?}), absteigend {start}..{end}",
                    run.text
                );
                geprueft += 1;
                nicht_leer += usize::from(bekommen.is_some());
            }
        }

        // Gegenprobe: der Vergleich hat wirklich Rechtecke gesehen und nicht
        // nur lauter `None` gegeneinandergehalten.
        assert!(geprueft > 100_000, "nur {geprueft} Vergleiche");
        assert!(
            nicht_leer * 3 > geprueft,
            "nur {nicht_leer} von {geprueft} Vergleichen lieferten ein Rechteck"
        );
        println!("{geprueft} Vergleiche, davon {nicht_leer} mit Rechteck — alle bitgenau gleich");
    }

    /// Die benannten Randfälle, ausgeschrieben statt gewürfelt.
    #[test]
    fn der_zeiger_trifft_anfang_ende_und_nachbarn() {
        // „ﬁ" (3 Byte) als Ligatur in zwei Glyphen mit **derselben** Zelle,
        // dazwischen Mehrbyte-Zeichen.
        let run = TextRun::new(
            0,
            vec![
                Glyph {
                    ch: 'A',
                    rect: Rect::new(0.0, 0.0, 5.0, 10.0),
                },
                Glyph {
                    ch: 'ä',
                    rect: Rect::new(5.0, 0.0, 11.0, 10.0),
                },
                Glyph {
                    ch: '€',
                    rect: Rect::new(11.0, 0.0, 20.0, 10.0),
                },
                Glyph {
                    ch: 'f',
                    rect: Rect::new(20.0, 0.0, 26.0, 10.0),
                },
                Glyph {
                    ch: 'i',
                    rect: Rect::new(20.0, 0.0, 26.0, 10.0),
                },
                Glyph {
                    ch: '𝄞',
                    rect: Rect::new(26.0, 0.0, 40.0, 10.0),
                },
            ],
        );
        // Byte-Grenzen: A=0, ä=1..3, €=3..6, f=6, i=7, 𝄞=8..12
        assert_eq!(run.text.len(), 12);

        let faelle: &[(usize, usize)] = &[
            (0, 1),   // ganz am Anfang
            (1, 3),   // Zweibyte-Zeichen
            (3, 6),   // Dreibyte-Zeichen
            (6, 8),   // Ligatur, beide Hälften
            (6, 7),   // nur die erste Hälfte
            (8, 12),  // ganz am Ende, Vierbyte-Zeichen
            (0, 12),  // alles
            (2, 6),   // Start mitten im 'ä'
            (7, 9),   // Ende mitten im '𝄞'
            (12, 13), // hinter dem Text
        ];
        // Ein einziger Zeiger für alle Bereiche — sie kommen aufsteigend.
        let mut zeiger = run.glyph_cursor();
        for (start, end) in faelle {
            assert_eq!(
                bits(referenz(&run, *start, *end)),
                bits(zeiger.rect_for_byte_range(*start, *end)),
                "Bereich {start}..{end}"
            );
        }
        // Unmittelbar hintereinanderliegende Bereiche, lückenlos.
        let mut zeiger = run.glyph_cursor();
        for (start, end) in [(0, 1), (1, 3), (3, 6), (6, 7), (7, 8), (8, 12)] {
            assert_eq!(
                bits(referenz(&run, start, end)),
                bits(zeiger.rect_for_byte_range(start, end)),
                "lückenlos {start}..{end}"
            );
        }
        // Und die Aussage, um die es geht: die Ligaturhälfte 'i' allein liefert
        // die Zelle der ganzen Ligatur, nicht die des Nachbarn.
        let mut zeiger = run.glyph_cursor();
        assert_eq!(
            zeiger.rect_for_byte_range(7, 8),
            Some(Rect::new(20.0, 0.0, 26.0, 10.0))
        );
    }

    /// [`TextRun::rect_for_byte_range`] geht jetzt durch den Zeiger — sie muss
    /// deshalb weiterhin *ohne* Zustand auskommen: zwei Aufrufe in beliebiger
    /// Reihenfolge liefern dasselbe wie zwei Aufrufe in umgekehrter.
    #[test]
    fn die_alte_funktion_bleibt_zustandslos() {
        let mut r = Zufall::neu(0xC0FF_EE00_1234_5678);
        for _ in 0..100 {
            let glyphen = 1 + r.bis(40);
            let run = zufallslauf(&mut r, glyphen);
            let n = run.text.len();
            for _ in 0..20 {
                let start = r.bis(n + 2);
                let end = start + r.bis(6);
                assert_eq!(
                    bits(run.rect_for_byte_range(start, end)),
                    bits(referenz(&run, start, end)),
                    "{:?} {start}..{end}",
                    run.text
                );
            }
        }
    }
}
