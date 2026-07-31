//! Textextraktion: aus Glyphen werden Zeilen.
//!
//! Ein PDF kennt keine Zeilen — nur einzeln positionierte Glyphen. Für das
//! Pattern-Matching ist die Zeile aber die entscheidende Einheit: eine IBAN
//! wird in der Praxis über mehrere `Tj`-Operationen verteilt ausgegeben
//! (`DE89 `, `3704 `, `0044 …`). Wer nur einzelne Text-Runs betrachtet, findet
//! sie nicht. Deshalb werden hier alle Glyphen einer Seite eingesammelt,
//! nach Grundlinie gruppiert und zu Zeilen zusammengesetzt.

use lopdf::{Document, ObjectId};
use redact_core::{Extractor, Glyph, Rect, Result, TextRun};

use crate::content::{scan_page, GlyphItem};

/// Auflösung der Richtungs-Einteilung in Grad. Glyphen mit gleicher gerundeter
/// Grundlinienrichtung kommen in dieselbe Zeile; ein 90°-Block bleibt also von
/// waagerechtem Text getrennt.
const DIRECTION_BUCKET_DEGREES: f64 = 1.0;

/// Extrahiert Textzeilen mit zeichengenauen Koordinaten.
#[derive(Debug, Clone)]
pub struct PdfExtractor {
    /// Toleranz (in Punkt), innerhalb derer Glyphen zur selben Zeile zählen.
    pub baseline_tolerance: f64,
    /// Ab welchem Anteil der **Leerzeichenbreite des Fonts** eine über den
    /// natürlichen Vorschub hinausgehende Lücke als Leerzeichen gilt.
    pub space_ratio: f64,
}

impl Default for PdfExtractor {
    fn default() -> Self {
        Self {
            baseline_tolerance: 2.0,
            space_ratio: 0.5,
        }
    }
}

impl PdfExtractor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Extrahiert die Zeilen einer einzelnen Seite (0-basiert).
    pub fn extract_page(&self, doc: &Document, page_index: usize) -> Result<Vec<TextRun>> {
        let pages = doc.get_pages();
        let Some((_, page_id)) = pages.iter().nth(page_index) else {
            return Ok(Vec::new());
        };
        let scan = scan_page(doc, *page_id)?;
        let glyphs: Vec<GlyphItem> = scan
            .shows
            .iter()
            .flat_map(|s| s.glyphs().cloned())
            .collect();
        Ok(self.build_lines(page_index, glyphs))
    }

    /// Wie [`Extractor::extract`], liefert aber zusätzlich die Warnungen des
    /// Interpreters.
    ///
    /// Wichtig ist vor allem der Fall „Font ohne `/ToUnicode`“: dort steht
    /// zwar Text auf der Seite, er lässt sich aber nicht dekodieren. Die
    /// Analyse findet dann nichts, die Schwärzung meldet Erfolg — und der
    /// Nutzer hält eine Datei für sauber, in der alles stehen geblieben ist.
    /// Diese Warnungen dürfen deshalb nicht im Extraktor versanden.
    pub fn extract_with_warnings(&self, doc: &Document) -> Result<(Vec<TextRun>, Vec<String>)> {
        let mut runs = Vec::new();
        let mut warnings: Vec<String> = Vec::new();
        for (index, (_, page_id)) in doc.get_pages().iter().enumerate() {
            let scan = scan_page(doc, *page_id)?;
            for warning in &scan.warnings {
                if !warnings.contains(warning) {
                    warnings.push(warning.clone());
                }
            }
            let glyphs: Vec<GlyphItem> = scan
                .shows
                .iter()
                .flat_map(|s| s.glyphs().cloned())
                .collect();
            runs.extend(self.build_lines(index, glyphs));
        }
        Ok((runs, warnings))
    }

    /// Setzt aus einzelnen Glyphen Zeilen zusammen.
    ///
    /// Gruppiert wird **entlang der Grundlinie**, nicht entlang der
    /// User-Space-Y-Achse: bei gedrehtem Text wandert der Zeilenursprung sonst
    /// in `y`, und jede einzelne Glyphe landet in einer eigenen „Zeile“ —
    /// womit kein Muster mehr über mehr als ein Zeichen greift.
    fn build_lines(&self, page: usize, mut glyphs: Vec<GlyphItem>) -> Vec<TextRun> {
        // Codes ohne Textzuordnung fliegen raus, Ersatzzeichen bleiben erhalten:
        // sie halten die Position und verhindern falsche Zusammenschreibung.
        glyphs.retain(|g| !g.text.is_empty());
        if glyphs.is_empty() {
            return Vec::new();
        }

        // Leserichtung herstellen: Zeilen in Vorschubrichtung, innerhalb der
        // Zeile in Schreibrichtung. Bei waagerechtem Text ist das genau
        // „von oben nach unten, dann von links nach rechts“. Die
        // Quantisierung des Zeilenabstands fängt kleine
        // Grundlinien-Schwankungen ab.
        let tol = self.baseline_tolerance.max(0.1);
        glyphs.sort_by(|a, b| {
            sort_key(a, tol)
                .partial_cmp(&sort_key(b, tol))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut lines: Vec<Vec<GlyphItem>> = Vec::new();
        let mut current: Vec<GlyphItem> = Vec::new();
        // Bezugswerte der laufenden Zeile. Die Toleranz gehört zur **Zeile**,
        // nicht zur gerade betrachteten Glyphe — sonst entscheidet die
        // Reihenfolge über die Gruppierung, und eine große Überschrift zieht
        // die kleine Zeile darüber in sich hinein.
        let mut line_direction = 0i64;
        let mut line_across = 0.0f64;
        let mut line_tol = tol;

        for g in glyphs {
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
            current.push(g);
        }
        if !current.is_empty() {
            lines.push(current);
        }

        lines
            .into_iter()
            .filter_map(|line| self.assemble_line(page, line))
            .collect()
    }

    /// Baut eine Zeile: Glyphen in Leserichtung, Lücken werden zu Leerzeichen.
    ///
    /// Als Lücke zählt nur, was **über den ohnehin gesetzten Vorschub
    /// hinausgeht**. Eine Laufweite (`Tc`) steckt bereits im Vorschub jeder
    /// Glyphe; würde man wie früher den Abstand der Glyphenkästen messen,
    /// stünde ab `Tc > 1,4 pt` zwischen jedem Zeichen ein Leerzeichen — und
    /// eine gesperrt gesetzte IBAN wäre nicht mehr zu erkennen.
    fn assemble_line(&self, page: usize, items: Vec<GlyphItem>) -> Option<TextRun> {
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
                let _ = (extra, pitch);
                let avg: f64 =
                    items.iter().map(|g| g.rect.width().abs()).sum::<f64>() / items.len() as f64;
                let threshold = (avg * 0.28).max(0.5);
                let extra = item.rect.ll.x - p.rect.ur.x;
                let pitch = 0.0;
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

impl Extractor for PdfExtractor {
    fn extract(&self, doc: &Document) -> Result<Vec<TextRun>> {
        Ok(self.extract_with_warnings(doc)?.0)
    }
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
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
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
fn sort_key(g: &GlyphItem, tol: f64) -> (i64, f64, f64) {
    (
        direction_bucket(g),
        -(across_of(g) / tol).round(),
        along_of(g),
    )
}

// ---------------------------------------------------------------------------
// Warnungen
// ---------------------------------------------------------------------------

/// Alle Font-Warnungen eines Dokuments, ohne Wiederholungen.
///
/// Gedacht für Aufrufer, die den Text nicht selbst brauchen — etwa die
/// Schwärzung, die sie in ihren Bericht übernimmt.
pub fn font_warnings(doc: &Document) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for (_, page_id) in doc.get_pages() {
        collect_warnings(doc, page_id, &mut out)?;
    }
    Ok(out)
}

fn collect_warnings(doc: &Document, page_id: ObjectId, out: &mut Vec<String>) -> Result<()> {
    for warning in scan_page(doc, page_id)?.warnings {
        if !out.contains(&warning) {
            out.push(warning);
        }
    }
    Ok(())
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
        let e = PdfExtractor::new();
        let glyphs = vec![
            glyph("A", 0.0, 100.0, 5.0),
            glyph("B", 5.0, 100.0, 5.0),
            glyph("C", 0.0, 80.0, 5.0),
        ];
        let lines = e.build_lines(0, glyphs);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "AB");
        assert_eq!(lines[1].text, "C");
    }

    #[test]
    fn inserts_space_for_large_gaps() {
        let e = PdfExtractor::new();
        let glyphs = vec![
            glyph("D", 0.0, 100.0, 5.0),
            glyph("E", 5.0, 100.0, 5.0),
            glyph("8", 40.0, 100.0, 5.0),
        ];
        let lines = e.build_lines(0, glyphs);
        assert_eq!(lines[0].text, "DE 8");
        // Die Glyph-Liste muss zeichenweise deckungsgleich bleiben.
        assert_eq!(lines[0].glyphs.len(), lines[0].text.chars().count());
    }

    #[test]
    fn sorts_out_of_order_glyphs_left_to_right() {
        let e = PdfExtractor::new();
        let glyphs = vec![glyph("Z", 20.0, 100.0, 5.0), glyph("A", 0.0, 100.0, 5.0)];
        let lines = e.build_lines(0, glyphs);
        assert!(lines[0].text.starts_with('A'));
    }

    #[test]
    fn ligature_glyph_keeps_char_alignment() {
        let e = PdfExtractor::new();
        let glyphs = vec![glyph("fi", 0.0, 100.0, 8.0), glyph("x", 8.0, 100.0, 5.0)];
        let lines = e.build_lines(0, glyphs);
        assert_eq!(lines[0].text, "fix");
        assert_eq!(lines[0].glyphs.len(), 3);
    }

    #[test]
    fn empty_input_yields_no_lines() {
        assert!(PdfExtractor::new().build_lines(0, vec![]).is_empty());
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
}
