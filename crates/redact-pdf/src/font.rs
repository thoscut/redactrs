//! Font-Metriken: Breiten, Höhen und Code→Zeichen-Zuordnung.
//!
//! Ohne Breiten gibt es keine korrekten Bounding-Boxen — und ohne korrekte
//! Bounding-Boxen schwärzt man an der falschen Stelle. Deshalb wird hier
//! einiges an Aufwand getrieben:
//!
//! * `/Widths` einfacher Fonts, `/W`+`/DW` bei CID-Fonts, `/MissingWidth`
//! * Metriken der 14 Standard-Fonts als Fallback (Helvetica/Times/Courier)
//! * `/ToUnicode` hat Vorrang, danach `/Encoding` + `/Differences`

use std::collections::BTreeMap;

use lopdf::{Dictionary, Document, Object};

use crate::encoding::{
    mac_roman_encoding, parse_to_unicode, standard_encoding, win_ansi_encoding, CharMap, CodeWidth,
};

/// Standardwerte, wenn der FontDescriptor nichts hergibt (Helvetica-nah).
const DEFAULT_ASCENT: f64 = 0.75;
const DEFAULT_DESCENT: f64 = -0.22;
const DEFAULT_WIDTH: f64 = 0.5;

#[derive(Debug, Clone)]
pub struct FontInfo {
    pub base_font: String,
    pub charmap: CharMap,
    /// Breiten je Code in Text-Space-Einheiten (also bereits durch 1000 geteilt).
    widths: BTreeMap<u32, f64>,
    default_width: f64,
    /// Ob `default_width` aus einem im Dokument **erklärten** `/DW` bzw.
    /// `/MissingWidth` stammt (und nicht bloß aus dem Vorgabewert).
    explicit_default_width: bool,
    /// Oberkante über der Grundlinie, in Text-Space-Einheiten.
    pub ascent: f64,
    /// Unterkante unter der Grundlinie (negativ).
    pub descent: f64,
}

impl Default for FontInfo {
    fn default() -> Self {
        Self {
            base_font: String::new(),
            charmap: CharMap::one_byte(win_ansi_encoding()),
            widths: BTreeMap::new(),
            default_width: DEFAULT_WIDTH,
            explicit_default_width: false,
            ascent: DEFAULT_ASCENT,
            descent: DEFAULT_DESCENT,
        }
    }
}

impl FontInfo {
    /// Breite eines Glyphen in Text-Space-Einheiten (1.0 = Schriftgröße).
    ///
    /// Reihenfolge: `/Widths` bzw. `/W` — dann eine im Dokument **erklärte**
    /// Vorgabebreite (`/DW`, `/MissingWidth`) — dann erst die Schätzung aus
    /// dem Fontnamen. Die Namensschätzung darf nicht gewinnen: sie liefert
    /// für ASCII 32..126 immer einen Wert und würde ein `/DW 600` still
    /// verdrängen. Der Stift liefe dann pro Zeichen um 0,044 em vor, und die
    /// x-Sortierung der Extraktion vertauscht Glyphen über getrennt
    /// positionierte Runs hinweg.
    pub fn width(&self, code: u32, text: &str) -> f64 {
        if let Some(w) = self.widths.get(&code) {
            return *w;
        }
        if self.explicit_default_width {
            return self.default_width;
        }
        if let Some(w) = standard_font_width(&self.base_font, text) {
            return w;
        }
        self.default_width
    }

    pub fn code_width(&self) -> CodeWidth {
        self.charmap.width
    }

    /// Alle bekannten Breiten (Code → Text-Space-Einheiten).
    ///
    /// Wird vom Zeichenoperationen-Strom gebraucht, damit der Renderer
    /// dieselben Vorschübe benutzt wie die Extraktion.
    pub fn width_map(&self) -> &BTreeMap<u32, f64> {
        &self.widths
    }

    /// Breite für Codes, die weder in `/Widths` noch in den Standardmetriken
    /// stehen.
    pub fn fallback_width(&self) -> f64 {
        self.default_width
    }
}

/// Lädt alle Fonts aus einem `/Resources`-Dictionary.
pub fn fonts_from_resources(
    doc: &Document,
    resources: Option<&Dictionary>,
) -> BTreeMap<Vec<u8>, FontInfo> {
    let mut out = BTreeMap::new();
    let Some(resources) = resources else {
        return out;
    };
    let Ok(font_dict) = resolve_dict(doc, resources.get(b"Font").ok()) else {
        return out;
    };
    for (name, obj) in font_dict.iter() {
        let Ok(dict) = resolve_dict(doc, Some(obj)) else {
            continue;
        };
        out.insert(name.to_vec(), load_font(doc, dict));
    }
    out
}

/// Lädt die Metriken eines einzelnen Font-Dictionaries.
///
/// Gleicher Code wie in [`fonts_from_resources`], nur für einen einzelnen
/// Font — der Zeichenoperationen-Strom löst Fonts einzeln (und zwischenge-
/// speichert) auf.
pub fn font_from_dict(doc: &Document, font: &Dictionary) -> FontInfo {
    load_font(doc, font)
}

fn resolve_dict<'a>(doc: &'a Document, obj: Option<&'a Object>) -> Result<&'a Dictionary, ()> {
    let obj = obj.ok_or(())?;
    let obj = doc.dereference(obj).map(|(_, o)| o).map_err(|_| ())?;
    obj.as_dict().map_err(|_| ())
}

fn deref<'a>(doc: &'a Document, obj: Option<&'a Object>) -> Option<&'a Object> {
    let obj = obj?;
    doc.dereference(obj).map(|(_, o)| o).ok()
}

fn load_font(doc: &Document, font: &Dictionary) -> FontInfo {
    let base_font = font
        .get(b"BaseFont")
        .and_then(Object::as_name)
        .map(|n| String::from_utf8_lossy(n).into_owned())
        .unwrap_or_default();
    let subtype = font
        .get(b"Subtype")
        .and_then(Object::as_name)
        .map(|n| n.to_vec())
        .unwrap_or_default();

    let mut info = FontInfo {
        base_font,
        ..Default::default()
    };

    if subtype == b"Type0" {
        load_type0(doc, font, &mut info);
    } else {
        load_simple(doc, font, &mut info);
    }

    // /ToUnicode gilt für alle Font-Typen und hat Vorrang.
    if let Some(Object::Stream(stream)) = deref(doc, font.get(b"ToUnicode").ok()) {
        if let Ok(data) = stream
            .decompressed_content()
            .or_else(|_| stream.get_plain_content())
        {
            let parsed = parse_to_unicode(&data);
            if !parsed.map.is_empty() {
                info.charmap.set_to_unicode(parsed.map);
            }
        }
    }

    info
}

fn load_simple(doc: &Document, font: &Dictionary, info: &mut FontInfo) {
    // --- Encoding ---
    let mut table = win_ansi_encoding();
    let mut differences: Vec<(u8, String)> = Vec::new();
    match deref(doc, font.get(b"Encoding").ok()) {
        Some(Object::Name(name)) => table = base_table(name),
        Some(Object::Dictionary(enc)) => {
            if let Ok(base) = enc.get(b"BaseEncoding").and_then(Object::as_name) {
                table = base_table(base);
            }
            if let Some(Object::Array(items)) = deref(doc, enc.get(b"Differences").ok()) {
                let mut code: i64 = 0;
                for item in items {
                    match item {
                        Object::Integer(n) => code = *n,
                        Object::Real(n) => code = *n as i64,
                        Object::Name(name) => {
                            if (0..256).contains(&code) {
                                differences
                                    .push((code as u8, String::from_utf8_lossy(name).into_owned()));
                            }
                            code += 1;
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
    let mut charmap = CharMap::one_byte(table);
    charmap.apply_differences(&differences);
    info.charmap = charmap;

    // --- Breiten ---
    let first_char = font
        .get(b"FirstChar")
        .and_then(Object::as_i64)
        .unwrap_or(0)
        .max(0) as u32;
    // Type3-Fonts geben Breiten im Glyph-Space an; /FontMatrix skaliert sie.
    let scale = font
        .get(b"FontMatrix")
        .and_then(Object::as_array)
        .ok()
        .and_then(|m| m.first().and_then(as_f64))
        .map(|a| a * 1000.0)
        .unwrap_or(1.0);
    if let Some(Object::Array(widths)) = deref(doc, font.get(b"Widths").ok()) {
        for (i, w) in widths.iter().enumerate() {
            if let Some(w) = deref(doc, Some(w)).and_then(as_f64) {
                info.widths
                    .insert(first_char + i as u32, w * scale / 1000.0);
            }
        }
    }

    load_descriptor(doc, font.get(b"FontDescriptor").ok(), info);
}

fn load_type0(doc: &Document, font: &Dictionary, info: &mut FontInfo) {
    // CID-Fonts benutzen praktisch immer Zweibyte-Codes (Identity-H & Co.).
    let mut charmap = CharMap::two_byte();
    if let Some(Object::Stream(stream)) = deref(doc, font.get(b"Encoding").ok()) {
        if let Ok(data) = stream
            .decompressed_content()
            .or_else(|_| stream.get_plain_content())
        {
            if let Some(width) = parse_to_unicode(&data).code_width {
                if width == CodeWidth::One {
                    charmap = CharMap::one_byte([None; 256]);
                }
            }
        }
    }
    info.charmap = charmap;

    let Some(descendants) = deref(doc, font.get(b"DescendantFonts").ok()) else {
        return;
    };
    let Ok(array) = descendants.as_array() else {
        return;
    };
    let Some(cid_font) = array.first().and_then(|o| deref(doc, Some(o))) else {
        return;
    };
    let Ok(cid_font) = cid_font.as_dict() else {
        return;
    };

    match cid_font.get(b"DW").ok().and_then(as_f64) {
        Some(dw) => {
            info.default_width = dw / 1000.0;
            info.explicit_default_width = true;
        }
        // Ohne `/DW` gilt laut PDF 32000-1 (9.7.4.3) 1000.
        None => info.default_width = 1.0,
    }

    if let Some(Object::Array(w)) = deref(doc, cid_font.get(b"W").ok()) {
        parse_cid_widths(doc, w, &mut info.widths);
    }

    load_descriptor(doc, cid_font.get(b"FontDescriptor").ok(), info);
}

/// `/W` hat die Form `[ c [w1 w2 …]  cFirst cLast w  … ]`.
fn parse_cid_widths(doc: &Document, w: &[Object], out: &mut BTreeMap<u32, f64>) {
    let mut i = 0;
    while i < w.len() {
        let Some(first) = deref(doc, w.get(i)).and_then(as_f64) else {
            i += 1;
            continue;
        };
        match deref(doc, w.get(i + 1)) {
            Some(Object::Array(list)) => {
                for (k, item) in list.iter().enumerate() {
                    if let Some(width) = deref(doc, Some(item)).and_then(as_f64) {
                        out.insert(first as u32 + k as u32, width / 1000.0);
                    }
                }
                i += 2;
            }
            Some(_) => {
                let last = deref(doc, w.get(i + 1)).and_then(as_f64).unwrap_or(first);
                let width = deref(doc, w.get(i + 2)).and_then(as_f64).unwrap_or(0.0);
                // Gegen absurde Bereiche aus kaputten Dateien absichern.
                let last = last.min(first + 65535.0);
                if last >= first {
                    for code in (first as u32)..=(last as u32) {
                        out.insert(code, width / 1000.0);
                    }
                }
                i += 3;
            }
            None => break,
        }
    }
}

fn load_descriptor(doc: &Document, descriptor: Option<&Object>, info: &mut FontInfo) {
    let Some(desc) = deref(doc, descriptor).and_then(|o| o.as_dict().ok()) else {
        return;
    };
    if let Some(a) = desc.get(b"Ascent").ok().and_then(as_f64) {
        if a > 0.0 {
            info.ascent = a / 1000.0;
        }
    }
    if let Some(d) = desc.get(b"Descent").ok().and_then(as_f64) {
        if d < 0.0 {
            info.descent = d / 1000.0;
        }
    }
    if let Some(mw) = desc.get(b"MissingWidth").ok().and_then(as_f64) {
        info.default_width = mw / 1000.0;
        info.explicit_default_width = true;
    }
}

fn base_table(name: &[u8]) -> [Option<char>; 256] {
    match name {
        b"WinAnsiEncoding" => win_ansi_encoding(),
        b"MacRomanEncoding" => mac_roman_encoding(),
        b"StandardEncoding" | b"MacExpertEncoding" => standard_encoding(),
        _ => win_ansi_encoding(),
    }
}

fn as_f64(obj: &Object) -> Option<f64> {
    match obj {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(*r as f64),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Metriken der Standard-14-Fonts (Fallback ohne /Widths)
// ---------------------------------------------------------------------------

/// Breiten für ASCII 32..126 in 1/1000 em.
const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667,
    611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500,
    222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

const HELVETICA_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667,
    611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 333, 278, 333, 584, 556, 333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556,
    278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

const TIMES: [u16; 95] = [
    250, 333, 408, 500, 500, 833, 778, 180, 333, 333, 500, 564, 250, 333, 250, 278, 500, 500, 500,
    500, 500, 500, 500, 500, 500, 500, 278, 278, 564, 564, 564, 444, 921, 722, 667, 667, 722, 611,
    556, 722, 722, 333, 389, 722, 611, 889, 722, 722, 556, 722, 667, 556, 611, 722, 722, 944, 722,
    722, 611, 333, 278, 333, 469, 500, 333, 444, 500, 444, 500, 444, 333, 500, 500, 278, 278, 500,
    278, 778, 500, 500, 500, 500, 333, 389, 278, 500, 500, 722, 500, 500, 444, 480, 200, 480, 541,
];

const TIMES_BOLD: [u16; 95] = [
    250, 333, 555, 500, 500, 1000, 833, 278, 333, 333, 500, 570, 250, 333, 250, 278, 500, 500, 500,
    500, 500, 500, 500, 500, 500, 500, 333, 333, 570, 570, 570, 500, 930, 722, 667, 722, 722, 667,
    611, 778, 778, 389, 500, 778, 667, 944, 722, 778, 611, 778, 722, 556, 667, 722, 722, 1000, 722,
    722, 667, 333, 278, 333, 581, 500, 333, 500, 556, 444, 556, 444, 333, 500, 556, 278, 333, 556,
    278, 833, 556, 500, 556, 556, 444, 389, 333, 556, 500, 722, 500, 500, 444, 394, 220, 394, 520,
];

/// Breite eines Zeichens in einem der Standard-14-Fonts.
fn standard_font_width(base_font: &str, text: &str) -> Option<f64> {
    let ch = text.chars().next()?;
    // Subset-Präfix („ABCDEF+Helvetica“) abschneiden.
    let name = base_font.rsplit('+').next().unwrap_or(base_font);
    let lower = name.to_ascii_lowercase();

    if lower.contains("courier") || lower.contains("mono") {
        return Some(0.6);
    }

    let bold = lower.contains("bold") || lower.contains("black") || lower.contains("heavy");
    let serif = lower.contains("times")
        || lower.contains("serif")
        || lower.contains("roman")
        || lower.contains("georgia")
        || lower.contains("garamond")
        || lower.contains("book");
    let table: &[u16; 95] = match (serif, bold) {
        (true, true) => &TIMES_BOLD,
        (true, false) => &TIMES,
        (false, true) => &HELVETICA_BOLD,
        (false, false) => &HELVETICA,
    };

    let idx = ch as u32;
    if (32..127).contains(&idx) {
        return Some(table[(idx - 32) as usize] as f64 / 1000.0);
    }
    // Latin-1-Buchstaben verhalten sich näherungsweise wie ihre ASCII-Basis.
    if idx >= 160 {
        return Some(if bold { 0.58 } else { 0.55 });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    #[test]
    fn standard_widths_are_plausible() {
        assert_eq!(standard_font_width("Helvetica", "M"), Some(0.833));
        assert_eq!(standard_font_width("Courier", "i"), Some(0.6));
        assert_eq!(standard_font_width("ABCDEF+Times-Bold", " "), Some(0.25));
        assert_eq!(standard_font_width("Helvetica", "€"), Some(0.55));
    }

    #[test]
    fn font_falls_back_to_standard_metrics() {
        let info = FontInfo {
            base_font: "Helvetica".into(),
            ..Default::default()
        };
        assert_eq!(info.width(b'M' as u32, "M"), 0.833);
    }

    #[test]
    fn explicit_widths_win() {
        let mut info = FontInfo {
            base_font: "Helvetica".into(),
            ..Default::default()
        };
        info.widths.insert(77, 0.9);
        assert_eq!(info.width(77, "M"), 0.9);
    }

    // -----------------------------------------------------------------------
    // K4 — im Dokument erklärte Vorgabebreiten schlagen die Namensschätzung
    // -----------------------------------------------------------------------

    #[test]
    fn explicit_dw_beats_the_standard_metrics_guess() {
        // Type0-Font mit `/DW 600`, aber ohne `/W`: die Ziffern sind 0,6 breit,
        // nicht 0,556 wie bei Helvetica.
        let mut doc = Document::new();
        let descendant = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "CIDFontType2",
            "BaseFont" => "ABCDEF+Helvetica",
            "DW" => 600,
        });
        let font = dictionary! {
            "Type" => "Font",
            "Subtype" => "Type0",
            "BaseFont" => "ABCDEF+Helvetica",
            "Encoding" => "Identity-H",
            "DescendantFonts" => vec![Object::Reference(descendant)],
        };
        let info = font_from_dict(&doc, &font);
        assert_eq!(info.width(b'4' as u32, "4"), 0.6);
    }

    #[test]
    fn missing_width_beats_the_standard_metrics_guess() {
        let mut doc = Document::new();
        let descriptor = doc.add_object(dictionary! {
            "Type" => "FontDescriptor",
            "FontName" => "ABCDEF+Helvetica",
            "MissingWidth" => 600,
        });
        let font = dictionary! {
            "Type" => "Font",
            "Subtype" => "TrueType",
            "BaseFont" => "ABCDEF+Helvetica",
            "FontDescriptor" => Object::Reference(descriptor),
        };
        let info = font_from_dict(&doc, &font);
        assert_eq!(info.width(b'4' as u32, "4"), 0.6);
    }

    #[test]
    fn widths_array_still_wins_over_an_explicit_default() {
        let mut doc = Document::new();
        let descriptor = doc.add_object(dictionary! {
            "Type" => "FontDescriptor",
            "MissingWidth" => 600,
        });
        let font = dictionary! {
            "Type" => "Font",
            "Subtype" => "TrueType",
            "BaseFont" => "Helvetica",
            "FirstChar" => 52_i64,
            "Widths" => vec![Object::Integer(900)],
            "FontDescriptor" => Object::Reference(descriptor),
        };
        let info = font_from_dict(&doc, &font);
        assert_eq!(info.width(b'4' as u32, "4"), 0.9);
    }

    #[test]
    fn without_an_explicit_default_the_standard_metrics_still_apply() {
        let doc = Document::new();
        let font = dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        };
        let info = font_from_dict(&doc, &font);
        assert_eq!(info.width(b'4' as u32, "4"), 0.556);
    }

    #[test]
    fn cid_width_array_ranges() {
        let doc = Document::new();
        let mut out = BTreeMap::new();
        let w = vec![
            Object::Integer(1),
            Object::Array(vec![Object::Integer(500), Object::Integer(600)]),
            Object::Integer(10),
            Object::Integer(12),
            Object::Integer(700),
        ];
        parse_cid_widths(&doc, &w, &mut out);
        assert_eq!(out.get(&1), Some(&0.5));
        assert_eq!(out.get(&2), Some(&0.6));
        assert_eq!(out.get(&10), Some(&0.7));
        assert_eq!(out.get(&12), Some(&0.7));
        assert_eq!(out.get(&13), None);
    }
}
