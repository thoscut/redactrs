//! Font-Metriken: Breiten, Höhen und Code→Zeichen-Zuordnung.
//!
//! Ohne Breiten gibt es keine korrekten Bounding-Boxen — und ohne korrekte
//! Bounding-Boxen schwärzt man an der falschen Stelle. Deshalb wird hier
//! einiges an Aufwand getrieben:
//!
//! * `/Widths` einfacher Fonts, `/W`+`/DW` bei CID-Fonts, `/MissingWidth`
//! * Metriken der 14 Standard-Fonts — als **letzte** Rückfallebene
//! * `/ToUnicode` hat Vorrang, danach `/Encoding` + `/Differences`
//! * Type0 ohne `/ToUnicode`: Zuordnung aus der `cmap` des eingebetteten
//!   Fontprogramms plus `/CIDToGIDMap` herleiten, sonst aus dem Namen der
//!   vordefinierten CMap

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
/// Font. Einstieg für Tests, die einen Font ohne Seite drumherum prüfen.
pub fn font_from_dict(doc: &Document, font: &Dictionary) -> FontInfo {
    load_font(doc, font)
}

fn resolve_dict<'a>(doc: &'a Document, obj: Option<&'a Object>) -> Result<&'a Dictionary, ()> {
    let obj = obj.ok_or(())?;
    let obj = doc.dereference(obj).map(|(_, o)| o).map_err(|_| ())?;
    obj.as_dict().map_err(|_| ())
}

/// Löst eine Referenz auf; die eine Fassung für das ganze Crate.
pub(crate) fn deref<'a>(doc: &'a Document, obj: Option<&'a Object>) -> Option<&'a Object> {
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

    // Ohne `/ToUnicode` bleibt bei Type0 sonst nur der Identity-Rückfall, und
    // der macht aus den CIDs eines Subsets (ab 1 durchnummeriert) lauter
    // Steuerzeichen. Vorher wird deshalb alles ausgereizt, was das Dokument
    // sonst noch hergibt.
    if subtype == b"Type0" && !info.charmap.has_to_unicode() {
        derive_cid_to_unicode(doc, font, &mut info);
    }

    info
}

// ---------------------------------------------------------------------------
// Type0 ohne /ToUnicode: Zuordnung herleiten statt raten
// ---------------------------------------------------------------------------

/// Versucht, für einen Type0-Font ohne `/ToUnicode` doch noch eine
/// Code→Text-Zuordnung zu gewinnen.
///
/// Zwei Wege, in dieser Reihenfolge:
///
/// 1. Eine **vordefinierte CMap** der Bauart `UniXXX-UCS2-H`/`-UTF16-`: dort
///    *ist* der Code der Unicode-Codepoint, es braucht keine Tabelle.
/// 2. Die **cmap des eingebetteten Fontprogramms** (`/FontFile2`, oder
///    `/FontFile3` mit vollständigem sfnt) rückwärts gelesen: sie ordnet
///    Unicode → Glyph-ID zu, `/CIDToGIDMap` ordnet CID → Glyph-ID zu. Beides
///    zusammen ergibt CID → Unicode.
///
/// Bleibt beides erfolglos, wird nichts gesetzt — dann greift der Rückfall,
/// und die Warnstatistik des Interpreters schlägt an.
fn derive_cid_to_unicode(doc: &Document, font: &Dictionary, info: &mut FontInfo) {
    if let Some(Object::Name(name)) = deref(doc, font.get(b"Encoding").ok()) {
        if predefined_cmap_is_unicode(name) {
            info.charmap.set_code_is_unicode(true);
            return;
        }
    }

    let Some(cid_font) = deref(doc, font.get(b"DescendantFonts").ok())
        .and_then(|o| o.as_array().ok())
        .and_then(|a| a.first())
        .and_then(|o| deref(doc, Some(o)))
        .and_then(|o| o.as_dict().ok())
    else {
        return;
    };

    let Some(data) = embedded_sfnt(doc, cid_font) else {
        return;
    };
    let gid_to_unicode = sfnt_gid_to_unicode(&data);
    if gid_to_unicode.is_empty() {
        return;
    }

    let cid_to_gid = cid_to_gid_map(doc, cid_font);
    let mut derived = BTreeMap::new();
    match cid_to_gid {
        // `/CIDToGIDMap /Identity` (oder fehlend): CID ist die Glyph-ID.
        None => {
            for (gid, text) in &gid_to_unicode {
                derived.insert(*gid as u32, text.clone());
            }
        }
        Some(table) => {
            for (cid, gid) in table.iter().enumerate() {
                if let Some(text) = gid_to_unicode.get(gid) {
                    derived.insert(cid as u32, text.clone());
                }
            }
        }
    }
    if !derived.is_empty() {
        info.charmap.set_derived(derived);
    }
}

/// Vordefinierte CMaps, deren Codes bereits Unicode sind.
///
/// Adobe benennt sie einheitlich `<Registry>-UCS2-H` bzw. `-UTF16-H`
/// (z. B. `UniGB-UCS2-H`, `UniJIS-UTF16-V`). Die CJK-CMaps mit
/// zeichensatzeigenen Codes (`90ms-RKSJ-H` & Co.) fallen bewusst **nicht**
/// darunter: dort wäre der Code eben *kein* Codepoint.
fn predefined_cmap_is_unicode(name: &[u8]) -> bool {
    let name = String::from_utf8_lossy(name);
    (name.contains("-UCS2") || name.contains("-UTF16")) && name.starts_with("Uni")
}

/// `/CIDToGIDMap`: `None` bedeutet Identity, sonst CID → Glyph-ID.
fn cid_to_gid_map(doc: &Document, cid_font: &Dictionary) -> Option<Vec<u16>> {
    let Some(Object::Stream(stream)) = deref(doc, cid_font.get(b"CIDToGIDMap").ok()) else {
        return None;
    };
    let data = stream
        .decompressed_content()
        .or_else(|_| stream.get_plain_content())
        .ok()?;
    Some(
        data.chunks_exact(2)
            .map(|c| ((c[0] as u16) << 8) | c[1] as u16)
            .collect(),
    )
}

/// Holt ein eingebettetes sfnt-Fontprogramm aus dem `/FontDescriptor`.
///
/// Nur sfnt (TrueType/OpenType) trägt eine `cmap`-Tabelle; ein blankes CFF
/// aus `/FontFile3 /Subtype /CIDFontType0C` bringt keine mit.
fn embedded_sfnt(doc: &Document, cid_font: &Dictionary) -> Option<Vec<u8>> {
    let descriptor = deref(doc, cid_font.get(b"FontDescriptor").ok())?
        .as_dict()
        .ok()?;
    for key in [&b"FontFile2"[..], &b"FontFile3"[..], &b"FontFile"[..]] {
        let Some(Object::Stream(stream)) = deref(doc, descriptor.get(key).ok()) else {
            continue;
        };
        let Ok(data) = stream
            .decompressed_content()
            .or_else(|_| stream.get_plain_content())
        else {
            continue;
        };
        if is_sfnt(&data) {
            return Some(data);
        }
    }
    None
}

fn is_sfnt(data: &[u8]) -> bool {
    data.len() >= 12
        && (data.starts_with(b"OTTO")
            || data.starts_with(&[0x00, 0x01, 0x00, 0x00])
            || data.starts_with(b"true")
            || data.starts_with(b"ttcf"))
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
        // Sobald `/Widths` da ist, hat der Font seine Metrik selbst erklärt.
        // Für Codes außerhalb des Bereichs gilt dann `/MissingWidth` — laut
        // PDF 32000-1 (Tabelle 122) mit Vorgabe 0 —, nicht die Schätzung aus
        // dem Fontnamen. Sonst gewönne die Standard-14-Tabelle gegen die
        // Erklärung des Dokuments, und der Stift liefe voraus.
        if !info.widths.is_empty() {
            info.default_width = 0.0;
            info.explicit_default_width = true;
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

    // Bei CID-Fonts ist die Vorgabebreite **immer** erklärt: entweder durch
    // `/DW` oder durch den Normwert 1000 aus PDF 32000-1 (9.7.4.3). Die
    // Schätzung aus dem Fontnamen darf hier nie zum Zug kommen — sie liefert
    // für 32..126 immer einen Wert und verdrängte damit still die Norm.
    info.default_width = cid_font
        .get(b"DW")
        .ok()
        .and_then(as_f64)
        .map(|dw| dw / 1000.0)
        .unwrap_or(1.0);
    info.explicit_default_width = true;

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

/// Zahl aus einem PDF-Objekt; die eine Fassung für das ganze Crate.
pub(crate) fn as_f64(obj: &Object) -> Option<f64> {
    match obj {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(*r as f64),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// sfnt-`cmap`: Unicode → Glyph-ID, rückwärts gelesen
// ---------------------------------------------------------------------------

/// Kappt absurde Tabellen aus kaputten oder feindlichen Dateien.
const MAX_CMAP_ENTRIES: usize = 200_000;

/// Liest die `cmap` eines sfnt-Fonts und dreht sie um: Glyph-ID → Text.
///
/// Mehrere Codepoints können auf dieselbe Glyphe zeigen; es gewinnt der
/// kleinste, weil das in der Praxis der „echte“ Buchstabe ist und die
/// Doppelbelegungen in den Bereichen für Ligaturen und die private Zone
/// liegen.
fn sfnt_gid_to_unicode(data: &[u8]) -> BTreeMap<u16, String> {
    let mut out = BTreeMap::new();
    let Some(table) = sfnt_table(data, b"cmap") else {
        return out;
    };
    for (code, gid) in parse_cmap(table) {
        if gid == 0 {
            continue;
        }
        let Some(ch) = char::from_u32(code) else {
            continue;
        };
        if ch.is_control() {
            continue;
        }
        out.entry(gid).or_insert_with(|| ch.to_string());
    }
    out
}

/// Sucht eine Tabelle im sfnt-Verzeichnis. Bei `ttcf` zählt der erste Font.
fn sfnt_table<'a>(data: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
    let mut base = 0usize;
    if data.starts_with(b"ttcf") {
        base = be_u32(data, 12)? as usize;
    }
    let num_tables = be_u16(data, base + 4)? as usize;
    for i in 0..num_tables {
        let rec = base + 12 + i * 16;
        let name = data.get(rec..rec + 4)?;
        if name == tag {
            let offset = be_u32(data, rec + 8)? as usize;
            let length = be_u32(data, rec + 12)? as usize;
            return data.get(offset..offset.checked_add(length)?);
        }
    }
    None
}

/// Wählt die beste Unterabelle der `cmap` und liest sie aus.
fn parse_cmap(table: &[u8]) -> BTreeMap<u32, u16> {
    let Some(count) = be_u16(table, 2) else {
        return BTreeMap::new();
    };
    // Je höher, desto lieber: volles Unicode vor BMP, BMP vor Symbol.
    let mut best: Option<(u8, usize, bool)> = None;
    for i in 0..count as usize {
        let rec = 4 + i * 8;
        let (Some(platform), Some(encoding), Some(offset)) = (
            be_u16(table, rec),
            be_u16(table, rec + 2),
            be_u32(table, rec + 4),
        ) else {
            continue;
        };
        let (rank, symbol) = match (platform, encoding) {
            (3, 10) | (0, 4) | (0, 6) => (4, false),
            (3, 1) | (0, 3) => (3, false),
            (0, 0) | (0, 1) | (0, 2) => (2, false),
            // (3,0) ist die Symbol-Belegung: Codes liegen in F000..F0FF.
            (3, 0) => (1, true),
            (1, 0) => (0, false),
            _ => continue,
        };
        if best.is_none_or(|(r, _, _)| rank > r) {
            best = Some((rank, offset as usize, symbol));
        }
    }
    let Some((_, offset, symbol)) = best else {
        return BTreeMap::new();
    };
    let Some(sub) = table.get(offset..) else {
        return BTreeMap::new();
    };
    let mut map = parse_cmap_subtable(sub);
    if symbol {
        // Symbol-cmaps führen ASCII unter F020..F0FF; für die Textsuche ist
        // das untere Byte die brauchbare Auskunft.
        map = map
            .into_iter()
            .map(|(code, gid)| {
                if (0xF000..=0xF0FF).contains(&code) {
                    (code & 0xFF, gid)
                } else {
                    (code, gid)
                }
            })
            .collect();
    }
    map
}

fn parse_cmap_subtable(sub: &[u8]) -> BTreeMap<u32, u16> {
    let mut map = BTreeMap::new();
    let Some(format) = be_u16(sub, 0) else {
        return map;
    };
    match format {
        0 => {
            for code in 0u32..256 {
                if let Some(&gid) = sub.get(6 + code as usize) {
                    if gid != 0 {
                        map.insert(code, gid as u16);
                    }
                }
            }
        }
        4 => parse_cmap_format4(sub, &mut map),
        6 => {
            let (Some(first), Some(count)) = (be_u16(sub, 6), be_u16(sub, 8)) else {
                return map;
            };
            for i in 0..count as usize {
                if let Some(gid) = be_u16(sub, 10 + i * 2) {
                    if gid != 0 {
                        map.insert(first as u32 + i as u32, gid);
                    }
                }
            }
        }
        12 => {
            let Some(groups) = be_u32(sub, 12) else {
                return map;
            };
            for i in 0..(groups as usize).min(MAX_CMAP_ENTRIES) {
                let rec = 16 + i * 12;
                let (Some(start), Some(end), Some(gid)) =
                    (be_u32(sub, rec), be_u32(sub, rec + 4), be_u32(sub, rec + 8))
                else {
                    break;
                };
                if end < start || end - start > MAX_CMAP_ENTRIES as u32 {
                    continue;
                }
                for code in start..=end {
                    let g = gid + (code - start);
                    if g != 0 && g <= u16::MAX as u32 {
                        map.insert(code, g as u16);
                    }
                }
                if map.len() > MAX_CMAP_ENTRIES {
                    break;
                }
            }
        }
        _ => {}
    }
    map
}

/// Format 4: segmentierte Bereiche mit `idDelta`/`idRangeOffset`.
fn parse_cmap_format4(sub: &[u8], map: &mut BTreeMap<u32, u16>) {
    let Some(seg_x2) = be_u16(sub, 6) else {
        return;
    };
    let segs = seg_x2 as usize / 2;
    let ends = 14;
    let starts = ends + seg_x2 as usize + 2;
    let deltas = starts + seg_x2 as usize;
    let ranges = deltas + seg_x2 as usize;
    for s in 0..segs {
        let (Some(end), Some(start), Some(delta), Some(range_offset)) = (
            be_u16(sub, ends + s * 2),
            be_u16(sub, starts + s * 2),
            be_u16(sub, deltas + s * 2),
            be_u16(sub, ranges + s * 2),
        ) else {
            return;
        };
        if start > end || start == 0xFFFF {
            continue;
        }
        for code in start..=end {
            let gid = if range_offset == 0 {
                code.wrapping_add(delta)
            } else {
                // Der Offset zählt ab der Position *dieses* idRangeOffset.
                let at = ranges + s * 2 + range_offset as usize + (code - start) as usize * 2;
                match be_u16(sub, at) {
                    Some(0) | None => continue,
                    Some(g) => g.wrapping_add(delta),
                }
            };
            if gid != 0 {
                map.insert(code as u32, gid);
            }
            if map.len() > MAX_CMAP_ENTRIES {
                return;
            }
        }
    }
}

fn be_u16(data: &[u8], at: usize) -> Option<u16> {
    let b = data.get(at..at.checked_add(2)?)?;
    Some(((b[0] as u16) << 8) | b[1] as u16)
}

fn be_u32(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
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

    // -----------------------------------------------------------------------
    // Aufgabe #19 — Zuordnung herleiten statt raten
    // -----------------------------------------------------------------------

    #[test]
    fn predefined_unicode_cmaps_are_recognised() {
        for name in [
            &b"UniGB-UCS2-H"[..],
            b"UniJIS-UCS2-HW-V",
            b"UniKS-UTF16-H",
            b"UniCNS-UCS2-V",
        ] {
            assert!(predefined_cmap_is_unicode(name), "{name:?}");
        }
        // Identity und die zeichensatzeigenen CJK-CMaps gehören *nicht* dazu:
        // dort ist der Code kein Codepoint.
        for name in [
            &b"Identity-H"[..],
            b"Identity-V",
            b"90ms-RKSJ-H",
            b"GBK-EUC-H",
            b"",
        ] {
            assert!(!predefined_cmap_is_unicode(name), "{name:?}");
        }
    }

    /// Format-4-`cmap` mit einem Segment je Zeichen, in einem sfnt verpackt.
    fn sfnt(pairs: &[(char, u16)]) -> Vec<u8> {
        let mut segs: Vec<(u16, u16)> = pairs.iter().map(|(c, g)| (*c as u16, *g)).collect();
        segs.sort_unstable();
        segs.push((0xFFFF, 1));
        let mut sub: Vec<u8> = Vec::new();
        let push = |v: &mut Vec<u8>, n: u16| v.extend_from_slice(&n.to_be_bytes());
        push(&mut sub, 4);
        push(&mut sub, 0);
        push(&mut sub, 0);
        push(&mut sub, segs.len() as u16 * 2);
        push(&mut sub, 2);
        push(&mut sub, 0);
        push(&mut sub, 0);
        for (code, _) in &segs {
            push(&mut sub, *code);
        }
        push(&mut sub, 0);
        for (code, _) in &segs {
            push(&mut sub, *code);
        }
        for (code, gid) in &segs {
            push(&mut sub, gid.wrapping_sub(*code));
        }
        for _ in &segs {
            push(&mut sub, 0);
        }
        let len = sub.len() as u16;
        sub[2..4].copy_from_slice(&len.to_be_bytes());

        let mut cmap: Vec<u8> = Vec::new();
        push(&mut cmap, 0);
        push(&mut cmap, 1);
        push(&mut cmap, 3);
        push(&mut cmap, 1);
        cmap.extend_from_slice(&12u32.to_be_bytes());
        cmap.extend_from_slice(&sub);

        let mut out: Vec<u8> = vec![0x00, 0x01, 0x00, 0x00];
        push(&mut out, 1);
        push(&mut out, 16);
        push(&mut out, 0);
        push(&mut out, 0);
        out.extend_from_slice(b"cmap");
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&28u32.to_be_bytes());
        out.extend_from_slice(&(cmap.len() as u32).to_be_bytes());
        out.extend_from_slice(&cmap);
        out
    }

    #[test]
    fn sfnt_cmap_is_read_backwards() {
        let data = sfnt(&[('K', 1), ('o', 2), ('n', 3), ('ä', 40), ('€', 300)]);
        let map = sfnt_gid_to_unicode(&data);
        assert_eq!(map.get(&1).map(String::as_str), Some("K"));
        assert_eq!(map.get(&2).map(String::as_str), Some("o"));
        assert_eq!(map.get(&3).map(String::as_str), Some("n"));
        assert_eq!(map.get(&40).map(String::as_str), Some("ä"));
        assert_eq!(map.get(&300).map(String::as_str), Some("€"));
        assert_eq!(map.get(&999), None);
    }

    #[test]
    fn truncated_font_programs_do_not_panic() {
        let data = sfnt(&[('A', 1), ('B', 2)]);
        for cut in 0..data.len() {
            let _ = sfnt_gid_to_unicode(&data[..cut]);
        }
        for junk in [&b""[..], b"OTTO", b"\x00\x01\x00\x00", b"nonsense"] {
            let _ = sfnt_gid_to_unicode(junk);
        }
    }

    #[test]
    fn cmap_format_12_and_6_are_understood() {
        // Format 12: eine Gruppe 0x41..0x43 → GID 7..9.
        let mut sub: Vec<u8> = Vec::new();
        sub.extend_from_slice(&12u16.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes());
        sub.extend_from_slice(&0u32.to_be_bytes());
        sub.extend_from_slice(&0u32.to_be_bytes());
        sub.extend_from_slice(&1u32.to_be_bytes());
        sub.extend_from_slice(&0x41u32.to_be_bytes());
        sub.extend_from_slice(&0x43u32.to_be_bytes());
        sub.extend_from_slice(&7u32.to_be_bytes());
        let map = parse_cmap_subtable(&sub);
        assert_eq!(map.get(&0x41), Some(&7));
        assert_eq!(map.get(&0x43), Some(&9));

        // Format 6: firstCode 0x30, zwei Einträge.
        let mut sub: Vec<u8> = Vec::new();
        sub.extend_from_slice(&6u16.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes());
        sub.extend_from_slice(&0x30u16.to_be_bytes());
        sub.extend_from_slice(&2u16.to_be_bytes());
        sub.extend_from_slice(&11u16.to_be_bytes());
        sub.extend_from_slice(&12u16.to_be_bytes());
        let map = parse_cmap_subtable(&sub);
        assert_eq!(map.get(&0x30), Some(&11));
        assert_eq!(map.get(&0x31), Some(&12));
    }
}
