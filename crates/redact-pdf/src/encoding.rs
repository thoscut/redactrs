//! Zeichensatz-Behandlung: Basis-Encodings, `/Differences` und `/ToUnicode`.
//!
//! lopdf exportiert seine internen Encoding-Tabellen nicht, und für die
//! Bounding-Box-Berechnung brauchen wir ohnehin eine *zeichenweise* Zuordnung
//! (Code → Zeichen), nicht nur „String am Stück dekodieren“. Deshalb bringt
//! dieses Modul seine eigenen, kompakten Tabellen mit.

use std::collections::BTreeMap;

use crate::glyphnames::glyph_name_to_text;

/// Ersatzzeichen für Codes, die sich nicht auflösen lassen. Es wird bewusst
/// eingesetzt, damit die Zeichenanzahl (und damit die Glyph-Indizes) stimmt.
pub const REPLACEMENT: char = '\u{FFFD}';

/// Wie viele Bytes ein Zeichencode belegt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeWidth {
    One,
    Two,
}

impl CodeWidth {
    pub fn bytes(self) -> usize {
        match self {
            CodeWidth::One => 1,
            CodeWidth::Two => 2,
        }
    }
}

/// Zuordnung Zeichencode → Text.
#[derive(Debug, Clone)]
pub struct CharMap {
    pub width: CodeWidth,
    /// Basis-Tabelle für Einbyte-Encodings (Index = Code).
    simple: Option<Box<[Option<char>; 256]>>,
    /// `/Differences`-Einträge, die für mehr als ein Zeichen stehen
    /// (Ligaturen wie `uni00660069`). Sie passen nicht in `simple`.
    ligatures: BTreeMap<u32, String>,
    /// Aus `/ToUnicode` gewonnene Zuordnung; hat Vorrang vor `simple`.
    to_unicode: BTreeMap<u32, String>,
}

impl CharMap {
    pub fn one_byte(table: [Option<char>; 256]) -> Self {
        Self {
            width: CodeWidth::One,
            simple: Some(Box::new(table)),
            ligatures: BTreeMap::new(),
            to_unicode: BTreeMap::new(),
        }
    }

    pub fn two_byte() -> Self {
        Self {
            width: CodeWidth::Two,
            simple: None,
            ligatures: BTreeMap::new(),
            to_unicode: BTreeMap::new(),
        }
    }

    pub fn set_to_unicode(&mut self, map: BTreeMap<u32, String>) {
        self.to_unicode = map;
    }

    pub fn has_to_unicode(&self) -> bool {
        !self.to_unicode.is_empty()
    }

    /// Wendet `/Differences` auf die Basis-Tabelle an.
    ///
    /// Nur **aufgelöste** Namen werden übernommen. Subset-Fonts führen ihre
    /// Glyphen häufig als `g42`, `cid17` oder `.notdef` auf; solche Namen
    /// sagen nichts über das Zeichen aus. Sie durften früher die gültige
    /// Zuordnung der Basistabelle überschreiben — der Code wurde damit
    /// unlesbar, obwohl WinAnsi ihn kannte.
    pub fn apply_differences(&mut self, diffs: &[(u8, String)]) {
        let Some(table) = self.simple.as_mut() else {
            return;
        };
        for (code, name) in diffs {
            let Some(text) = glyph_name_to_text(name) else {
                continue;
            };
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => {
                    table[*code as usize] = Some(c);
                    self.ligatures.remove(&(*code as u32));
                }
                (Some(_), Some(_)) => {
                    // Ligatur: mehrere Zeichen passen nicht in die Tabelle.
                    self.ligatures.insert(*code as u32, text);
                }
                _ => {}
            }
        }
    }

    /// Text zu einem einzelnen Code.
    pub fn text_for(&self, code: u32) -> String {
        if let Some(s) = self.to_unicode.get(&code) {
            if !s.is_empty() {
                return s.clone();
            }
        }
        if let Some(s) = self.ligatures.get(&code) {
            return s.clone();
        }
        if let Some(table) = &self.simple {
            if code < 256 {
                if let Some(c) = table[code as usize] {
                    return c.to_string();
                }
            }
        }
        // Identity-Fallback: viele Subset-Fonts ohne ToUnicode benutzen
        // ASCII-kompatible Codes.
        if let Some(c) = char::from_u32(code) {
            if !c.is_control() {
                return c.to_string();
            }
        }
        REPLACEMENT.to_string()
    }

    /// Zerlegt eine PDF-Zeichenkette in (Code, Text, Byte-Anzahl).
    pub fn decode(&self, bytes: &[u8]) -> Vec<(u32, String, usize)> {
        let mut out = Vec::with_capacity(bytes.len());
        match self.width {
            CodeWidth::One => {
                for &b in bytes {
                    let code = b as u32;
                    out.push((code, self.text_for(code), 1));
                }
            }
            CodeWidth::Two => {
                let mut i = 0;
                while i + 1 < bytes.len() {
                    let code = ((bytes[i] as u32) << 8) | bytes[i + 1] as u32;
                    out.push((code, self.text_for(code), 2));
                    i += 2;
                }
                if i < bytes.len() {
                    // Ungerade Byte-Zahl: letztes Byte als Einzelcode behandeln.
                    let code = bytes[i] as u32;
                    out.push((code, self.text_for(code), 1));
                }
            }
        }
        out
    }
}

/// WinAnsiEncoding (im Wesentlichen CP1252).
pub fn win_ansi_encoding() -> [Option<char>; 256] {
    let mut t = [None; 256];
    for code in 32u32..127 {
        t[code as usize] = char::from_u32(code);
    }
    // 0x80–0x9F: CP1252-Sonderbelegung
    const HIGH: [char; 32] = [
        '\u{20AC}', '\u{FFFD}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}',
        '\u{2021}', '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{FFFD}',
        '\u{017D}', '\u{FFFD}', '\u{FFFD}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}',
        '\u{2022}', '\u{2013}', '\u{2014}', '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}',
        '\u{0153}', '\u{FFFD}', '\u{017E}', '\u{0178}',
    ];
    for (i, c) in HIGH.iter().enumerate() {
        if *c != '\u{FFFD}' {
            t[128 + i] = Some(*c);
        }
    }
    // 0xA0–0xFF entspricht Latin-1.
    for code in 160u32..256 {
        t[code as usize] = char::from_u32(code);
    }
    // Adobe-Sonderfälle
    t[0xA0] = Some(' ');
    t[0xAD] = Some('-');
    t
}

/// MacRomanEncoding.
pub fn mac_roman_encoding() -> [Option<char>; 256] {
    let mut t = [None; 256];
    for code in 32u32..127 {
        t[code as usize] = char::from_u32(code);
    }
    const HIGH: &str = concat!(
        "ÄÅÇÉÑÖÜáàâäãåçéè",
        "êëíìîïñóòôöõúùûü",
        "†°¢£§•¶ß®©™´¨≠ÆØ",
        "∞±≤≥¥µ∂∑∏π∫ªºΩæø",
        "¿¡¬√ƒ≈∆«»… ÀÃÕŒœ",
        "–—“”‘’÷◊ÿŸ⁄€‹›ﬁﬂ",
        "‡·‚„‰ÂÊÁËÈÍÎÏÌÓÔ",
        "\u{F8FF}ÒÚÛÙıˆ˜¯˘˙˚¸˝˛ˇ",
    );
    for (i, c) in HIGH.chars().enumerate() {
        t[128 + i] = Some(c);
    }
    t
}

/// StandardEncoding (Adobe). Weicht bei den Anführungszeichen von ASCII ab.
pub fn standard_encoding() -> [Option<char>; 256] {
    let mut t = [None; 256];
    for code in 32u32..127 {
        t[code as usize] = char::from_u32(code);
    }
    t[0x27] = Some('\u{2019}'); // quoteright
    t[0x60] = Some('\u{2018}'); // quoteleft
    for (code, c) in [
        (0xA1, '¡'),
        (0xA2, '¢'),
        (0xA3, '£'),
        (0xA4, '\u{2044}'),
        (0xA5, '¥'),
        (0xA6, 'ƒ'),
        (0xA7, '§'),
        (0xA8, '¤'),
        (0xA9, '\''),
        (0xAA, '\u{201C}'),
        (0xAB, '«'),
        (0xAC, '\u{2039}'),
        (0xAD, '\u{203A}'),
        (0xAE, '\u{FB01}'),
        (0xAF, '\u{FB02}'),
        (0xB1, '\u{2013}'),
        (0xB2, '\u{2020}'),
        (0xB3, '\u{2021}'),
        (0xB4, '·'),
        (0xB6, '¶'),
        (0xB7, '\u{2022}'),
        (0xB8, '\u{201A}'),
        (0xB9, '\u{201E}'),
        (0xBA, '\u{201D}'),
        (0xBB, '»'),
        (0xBC, '\u{2026}'),
        (0xBD, '\u{2030}'),
        (0xBF, '¿'),
        (0xC1, '`'),
        (0xC2, '´'),
        (0xC3, 'ˆ'),
        (0xC4, '˜'),
        (0xC5, '¯'),
        (0xC6, '˘'),
        (0xC7, '˙'),
        (0xC8, '¨'),
        (0xCA, '˚'),
        (0xCB, '¸'),
        (0xCD, '˝'),
        (0xCE, '˛'),
        (0xCF, 'ˇ'),
        (0xD0, '\u{2014}'),
        (0xE1, 'Æ'),
        (0xE3, 'ª'),
        (0xE8, 'Ł'),
        (0xE9, 'Ø'),
        (0xEA, 'Œ'),
        (0xEB, 'º'),
        (0xF1, 'æ'),
        (0xF5, 'ı'),
        (0xF8, 'ł'),
        (0xF9, 'ø'),
        (0xFA, 'œ'),
        (0xFB, 'ß'),
    ] {
        t[code] = Some(c);
    }
    t
}

// ---------------------------------------------------------------------------
// ToUnicode-CMap
// ---------------------------------------------------------------------------

/// Ergebnis des CMap-Parsers.
#[derive(Debug, Default)]
pub struct ToUnicode {
    pub map: BTreeMap<u32, String>,
    /// Aus `codespacerange` abgeleitete Code-Breite, falls eindeutig.
    pub code_width: Option<CodeWidth>,
}

/// Parst eine `/ToUnicode`-CMap (bfchar/bfrange/codespacerange).
///
/// Der Parser ist bewusst tolerant: unbekannte Konstrukte werden übersprungen,
/// statt die Extraktion scheitern zu lassen.
pub fn parse_to_unicode(data: &[u8]) -> ToUnicode {
    let tokens = tokenize(data);
    let mut result = ToUnicode::default();
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Keyword(k) if k == "begincodespacerange" => {
                i += 1;
                let mut widths = Vec::new();
                while i < tokens.len() && !tokens[i].is_keyword("endcodespacerange") {
                    if let Token::Hex(h) = &tokens[i] {
                        widths.push(h.len());
                    }
                    i += 1;
                }
                if !widths.is_empty() && widths.iter().all(|w| *w == widths[0]) {
                    result.code_width = match widths[0] {
                        1 => Some(CodeWidth::One),
                        2 => Some(CodeWidth::Two),
                        _ => None,
                    };
                }
            }
            Token::Keyword(k) if k == "beginbfchar" => {
                i += 1;
                while i + 1 < tokens.len() && !tokens[i].is_keyword("endbfchar") {
                    let (src, dst) = (&tokens[i], &tokens[i + 1]);
                    if let (Token::Hex(s), dst) = (src, dst) {
                        if let Some(code) = hex_to_u32(s) {
                            if let Some(text) = token_to_text(dst) {
                                result.map.insert(code, text);
                            }
                        }
                    }
                    i += 2;
                }
            }
            Token::Keyword(k) if k == "beginbfrange" => {
                i += 1;
                while i + 2 < tokens.len() && !tokens[i].is_keyword("endbfrange") {
                    let lo = &tokens[i];
                    let hi = &tokens[i + 1];
                    let dst = &tokens[i + 2];
                    if let (Token::Hex(l), Token::Hex(h)) = (lo, hi) {
                        if let (Some(lo), Some(hi)) = (hex_to_u32(l), hex_to_u32(h)) {
                            apply_bfrange(&mut result.map, lo, hi, dst);
                        }
                    }
                    i += 3;
                }
            }
            _ => {}
        }
        i += 1;
    }
    result
}

fn apply_bfrange(map: &mut BTreeMap<u32, String>, lo: u32, hi: u32, dst: &Token) {
    // Schutz gegen absurde Bereiche aus kaputten Dateien.
    let hi = hi.min(lo.saturating_add(0xFFFF));
    match dst {
        Token::Array(items) => {
            for (offset, item) in items.iter().enumerate() {
                let code = lo + offset as u32;
                if code > hi {
                    break;
                }
                if let Some(text) = token_to_text(item) {
                    map.insert(code, text);
                }
            }
        }
        Token::Hex(bytes) => {
            // Fortlaufende Zuordnung: das letzte UTF-16-Wort wird hochgezählt.
            let units = hex_to_utf16(bytes);
            if units.is_empty() {
                return;
            }
            for code in lo..=hi {
                let mut u = units.clone();
                let last = u.len() - 1;
                u[last] = u[last].wrapping_add((code - lo) as u16);
                if let Some(text) = utf16_to_string(&u) {
                    map.insert(code, text);
                }
            }
        }
        Token::Name(name) => {
            if let Some(text) = glyph_name_to_text(name) {
                map.insert(lo, text);
            }
        }
        _ => {}
    }
}

fn token_to_text(t: &Token) -> Option<String> {
    match t {
        Token::Hex(bytes) => utf16_to_string(&hex_to_utf16(bytes)),
        Token::Name(name) => glyph_name_to_text(name),
        _ => None,
    }
}

fn hex_to_u32(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() || bytes.len() > 4 {
        return None;
    }
    Some(bytes.iter().fold(0u32, |acc, b| (acc << 8) | *b as u32))
}

fn hex_to_utf16(bytes: &[u8]) -> Vec<u16> {
    bytes
        .chunks(2)
        .map(|c| {
            if c.len() == 2 {
                ((c[0] as u16) << 8) | c[1] as u16
            } else {
                c[0] as u16
            }
        })
        .collect()
}

fn utf16_to_string(units: &[u16]) -> Option<String> {
    if units.is_empty() {
        return None;
    }
    let s = String::from_utf16_lossy(units);
    let s: String = s.chars().filter(|c| *c != '\0').collect();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Hex(Vec<u8>),
    Name(String),
    Number(f64),
    Keyword(String),
    Array(Vec<Token>),
}

impl Token {
    fn is_keyword(&self, kw: &str) -> bool {
        matches!(self, Token::Keyword(k) if k == kw)
    }
}

fn tokenize(data: &[u8]) -> Vec<Token> {
    let mut out = Vec::new();
    let mut stack: Vec<Vec<Token>> = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        match b {
            b'%' => {
                while i < data.len() && data[i] != b'\n' && data[i] != b'\r' {
                    i += 1;
                }
            }
            b'<' => {
                if data.get(i + 1) == Some(&b'<') {
                    // Dictionary-Anfang: für uns uninteressant.
                    i += 2;
                    continue;
                }
                let start = i + 1;
                let mut end = start;
                while end < data.len() && data[end] != b'>' {
                    end += 1;
                }
                push(
                    &mut out,
                    &mut stack,
                    Token::Hex(parse_hex(&data[start..end])),
                );
                i = end + 1;
            }
            b'>' => {
                i += 1;
                if data.get(i) == Some(&b'>') {
                    i += 1;
                }
            }
            b'[' => {
                stack.push(Vec::new());
                i += 1;
            }
            b']' => {
                if let Some(items) = stack.pop() {
                    push(&mut out, &mut stack, Token::Array(items));
                }
                i += 1;
            }
            b'/' => {
                let start = i + 1;
                let mut end = start;
                while end < data.len() && !is_delimiter(data[end]) {
                    end += 1;
                }
                let name = String::from_utf8_lossy(&data[start..end]).into_owned();
                push(&mut out, &mut stack, Token::Name(name));
                i = end;
            }
            b if b.is_ascii_whitespace() => i += 1,
            _ => {
                let start = i;
                let mut end = i;
                while end < data.len() && !is_delimiter(data[end]) {
                    end += 1;
                }
                if end == start {
                    i += 1;
                    continue;
                }
                let word = String::from_utf8_lossy(&data[start..end]).into_owned();
                let token = match word.parse::<f64>() {
                    Ok(n) => Token::Number(n),
                    Err(_) => Token::Keyword(word),
                };
                push(&mut out, &mut stack, token);
                i = end;
            }
        }
    }
    out
}

fn push(out: &mut Vec<Token>, stack: &mut [Vec<Token>], token: Token) {
    match stack.last_mut() {
        Some(top) => top.push(token),
        None => out.push(token),
    }
}

fn is_delimiter(b: u8) -> bool {
    b.is_ascii_whitespace()
        || matches!(
            b,
            b'/' | b'[' | b']' | b'<' | b'>' | b'(' | b')' | b'{' | b'}' | b'%'
        )
}

fn parse_hex(data: &[u8]) -> Vec<u8> {
    let digits: Vec<u8> = data
        .iter()
        .filter(|b| b.is_ascii_hexdigit())
        .map(|b| (*b as char).to_digit(16).unwrap_or(0) as u8)
        .collect();
    let mut out = Vec::with_capacity(digits.len().div_ceil(2));
    let mut chunks = digits.chunks(2);
    for c in &mut chunks {
        if c.len() == 2 {
            out.push((c[0] << 4) | c[1]);
        } else {
            // Ungerade Anzahl: mit 0 auffüllen (PDF-Konvention).
            out.push(c[0] << 4);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn win_ansi_covers_german_and_euro() {
        let t = win_ansi_encoding();
        assert_eq!(t[b'A' as usize], Some('A'));
        assert_eq!(t[0xE4], Some('ä'));
        assert_eq!(t[0xDF], Some('ß'));
        assert_eq!(t[0x80], Some('€'));
    }

    #[test]
    fn differences_override_base_table() {
        let mut cm = CharMap::one_byte(win_ansi_encoding());
        cm.apply_differences(&[(65, "germandbls".to_string())]);
        assert_eq!(cm.text_for(65), "ß");
    }

    // -----------------------------------------------------------------------
    // K8 — `/Differences` darf gültige Zuordnungen nicht löschen
    // -----------------------------------------------------------------------

    #[test]
    fn unresolvable_difference_name_keeps_the_base_mapping() {
        // Subset-Fonts führen ihre Glyphen oft als `g42`/`cid17` auf. Solche
        // Namen sagen nichts über das Zeichen aus — die Basistabelle bleibt
        // die bessere Auskunft.
        let mut cm = CharMap::one_byte(win_ansi_encoding());
        cm.apply_differences(&[
            (0x80, "g42".to_string()),
            (0x92, "cid17".to_string()),
            (0xE4, ".notdef".to_string()),
        ]);
        assert_eq!(cm.text_for(0x80), "€");
        assert_eq!(cm.text_for(0x92), "\u{2019}");
        assert_eq!(cm.text_for(0xE4), "ä");
    }

    #[test]
    fn ligature_difference_name_keeps_both_characters() {
        let mut cm = CharMap::one_byte(win_ansi_encoding());
        cm.apply_differences(&[(1, "uni00660069".to_string())]);
        assert_eq!(cm.text_for(1), "fi");
    }

    #[test]
    fn bfrange_with_a_ligature_name_keeps_both_characters() {
        let r = parse_to_unicode(b"1 beginbfrange <0001> <0001> /uni00660069 endbfrange");
        assert_eq!(r.map.get(&1).map(String::as_str), Some("fi"));
    }

    #[test]
    fn parses_bfchar() {
        let cmap = b"/CIDInit /ProcSet findresource begin
1 begincodespacerange <0000> <FFFF> endcodespacerange
2 beginbfchar
<0003> <0020>
<0024> <0041>
endbfchar
endcmap";
        let r = parse_to_unicode(cmap);
        assert_eq!(r.code_width, Some(CodeWidth::Two));
        assert_eq!(r.map.get(&3).map(String::as_str), Some(" "));
        assert_eq!(r.map.get(&0x24).map(String::as_str), Some("A"));
    }

    #[test]
    fn parses_bfrange_incremental_and_array() {
        let cmap = b"3 beginbfrange
<0010> <0012> <0041>
<0020> <0021> [<0058> <0059>]
endbfrange";
        let r = parse_to_unicode(cmap);
        assert_eq!(r.map.get(&0x10).map(String::as_str), Some("A"));
        assert_eq!(r.map.get(&0x11).map(String::as_str), Some("B"));
        assert_eq!(r.map.get(&0x12).map(String::as_str), Some("C"));
        assert_eq!(r.map.get(&0x20).map(String::as_str), Some("X"));
        assert_eq!(r.map.get(&0x21).map(String::as_str), Some("Y"));
    }

    #[test]
    fn parses_ligature_target() {
        let cmap = b"1 beginbfchar <0001> <00660069> endbfchar";
        let r = parse_to_unicode(cmap);
        assert_eq!(r.map.get(&1).map(String::as_str), Some("fi"));
    }

    #[test]
    fn decode_two_byte_codes() {
        let mut cm = CharMap::two_byte();
        let mut m = BTreeMap::new();
        m.insert(0x0024, "A".to_string());
        m.insert(0x0025, "B".to_string());
        cm.set_to_unicode(m);
        let decoded = cm.decode(&[0x00, 0x24, 0x00, 0x25]);
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].1, "A");
        assert_eq!(decoded[1].1, "B");
        assert_eq!(decoded[1].2, 2);
    }

    #[test]
    fn tolerates_broken_cmap() {
        let r = parse_to_unicode(b"beginbfchar <00 endbfchar garbage [ ] >>");
        assert!(r.map.is_empty() || !r.map.is_empty()); // darf nur nicht paniken
    }
}
