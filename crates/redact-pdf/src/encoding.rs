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
    /// Selbst **hergeleitete** Zuordnung, wenn der Font kein `/ToUnicode`
    /// mitbringt (cmap des eingebetteten Fontprogramms über `/CIDToGIDMap`).
    ///
    /// Bewusst getrennt von `to_unicode`: [`CharMap::has_to_unicode`] muss
    /// weiterhin „der Font sagt es selbst“ bedeuten. Eine hergeleitete
    /// Zuordnung ist oft nur teilweise; die Warnstatistik im Interpreter zählt
    /// deshalb weiter die tatsächlich unlesbaren Zeichen, statt den Font
    /// pauschal für geklärt zu halten.
    derived: BTreeMap<u32, String>,
    /// Der Code **ist** der Unicode-Codepoint (vordefinierte CMaps der Bauart
    /// `UniXXX-UCS2-H` bzw. `-UTF16-`). Dann braucht es keine Tabelle.
    code_is_unicode: bool,
    /// Ob `simple` aus einem echten Basis-Encoding stammt (WinAnsi & Co.) und
    /// nicht bloß eine leere Hülle ist.
    has_base_table: bool,
}

impl CharMap {
    pub fn one_byte(table: [Option<char>; 256]) -> Self {
        let has_base_table = table.iter().any(Option::is_some);
        Self {
            width: CodeWidth::One,
            simple: Some(Box::new(table)),
            ligatures: BTreeMap::new(),
            to_unicode: BTreeMap::new(),
            derived: BTreeMap::new(),
            code_is_unicode: false,
            has_base_table,
        }
    }

    pub fn two_byte() -> Self {
        Self {
            width: CodeWidth::Two,
            simple: None,
            ligatures: BTreeMap::new(),
            to_unicode: BTreeMap::new(),
            derived: BTreeMap::new(),
            code_is_unicode: false,
            has_base_table: false,
        }
    }

    pub fn set_to_unicode(&mut self, map: BTreeMap<u32, String>) {
        self.to_unicode = map;
    }

    /// Übernimmt eine selbst hergeleitete Code→Text-Zuordnung.
    pub fn set_derived(&mut self, map: BTreeMap<u32, String>) {
        self.derived = map;
    }

    /// Erklärt die Codes selbst für Unicode-Codepoints (UCS2-/UTF16-CMaps).
    pub fn set_code_is_unicode(&mut self, yes: bool) {
        self.code_is_unicode = yes;
    }

    /// Ob der Font selbst sagt, was seine Codes bedeuten (`/ToUnicode`).
    pub fn has_to_unicode(&self) -> bool {
        !self.to_unicode.is_empty()
    }

    /// Ob es überhaupt eine Auskunft über die Codes gibt — eigene oder
    /// hergeleitete.
    pub fn has_text_mapping(&self) -> bool {
        self.has_to_unicode() || self.code_is_unicode || !self.derived.is_empty()
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
        if let Some(s) = self.derived.get(&code) {
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
        if self.code_is_unicode {
            if let Some(c) = char::from_u32(code) {
                if !c.is_control() {
                    return c.to_string();
                }
            }
            return REPLACEMENT.to_string();
        }
        // Identity-Rückfall — aber **nur** bei einem echten Basis-Encoding.
        //
        // Bei einem Type0/Identity-H-Font ist der Code die Glyphnummer des
        // Subsets. Dass die zufällig im druckbaren ASCII-Bereich liegt, macht
        // sie nicht zu Text: aus „Kontonummer 532013000“ wird dann
        // „()*+)*,--./0123452444“ — lesbar aussehender Unsinn, in dem kein
        // Muster mehr greift. Schlimmer noch: ohne Ersatzzeichen hält die
        // Warnstatistik des Interpreters den Font für dekodiert und schweigt.
        // Der Nutzer bekäme eine „erfolgreich geschwärzte“ Datei, in der alles
        // stehen geblieben ist. Wo wir es nicht wissen, sagen wir es.
        if self.has_base_table {
            if let Some(c) = char::from_u32(code) {
                if !c.is_control() {
                    return c.to_string();
                }
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

/// Rückwärtssuche in [`win_ansi_encoding`]: Text → WinAnsi-Bytes.
///
/// Nicht darstellbare Zeichen werden zu `?`. Steht hier, weil die Tabelle hier
/// steht — Schwärzung (Ersatztext) und Testfixtures brauchen dieselbe Umkehrung.
pub fn to_win_ansi(text: &str) -> Vec<u8> {
    let table = win_ansi_encoding();
    text.chars()
        .map(|ch| {
            table
                .iter()
                .position(|entry| *entry == Some(ch))
                .map(|i| i as u8)
                .unwrap_or(b'?')
        })
        .collect()
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

/// Wie viel Speicher **eine** `/ToUnicode`-Zuordnung belegen darf.
///
/// ## Warum es diese Grenze braucht
///
/// `apply_bfrange` deckelt *einen* Bereich auf 65 536 Codes. Die **Anzahl**
/// der `bfrange`-Anweisungen war dagegen frei, und weil sich wiederholte
/// Zeilen etwa 1000:1 flate-komprimieren, kostet eine Anweisung von rund
/// 45 Byte im Dokument bis zu 65 536 Einträge. Gemessen am Release-Binary:
/// 400 Anweisungen aus einer Datei von 3 032 Byte belegten 6 775 MB, 1 000
/// Anweisungen brachten den Prozess um (SIGABRT).
///
/// Zwei Bomben, nicht eine. Die zweite zählt **Bytes** statt Einträgen: eine
/// einzige `bfrange` über 65 536 Codes mit einem langen Zielstring kopiert
/// diesen String für jeden Code. 1 131 Byte Datei ergaben so 530 MB, 1 253
/// Byte den Abbruch. Eine Grenze, die nur Einträge zählt, sieht davon nichts
/// — es bleiben 65 536 Einträge. Deshalb wird hier der Platzbedarf gezählt.
///
/// ## Warum dieser Wert
///
/// Eine **legitime** ToUnicode kann nicht beliebig groß werden: sie ordnet
/// Zeichencodes zu, und mehr als 65 536 verschiedene Codes hat kein Font.
/// Identity-H benutzt Zweibyte-Codes, und sfnt wie CFF können ohnehin nicht
/// mehr als 65 535 Glyphen führen. Ein CJK-Font mit vollem Umfang ist damit
/// der größte ehrliche Fall — gemessen 17 MB. 32 MB lassen ihn mit knapp dem
/// Doppelten an Luft durch und liegen zugleich unter dem, was das Haus für
/// eine Tabelle derselben Bauart schon zulässt ([`crate::font`],
/// `MAX_CMAP_ENTRIES` = 200 000 Einträge ≈ 52 MB).
///
/// Der Kommentar an [`crate::content`] („`/ToUnicode` kann eine Million
/// Einträge haben“) trägt für echte Dateien also **nicht**; er beschreibt
/// genau den Fall, den diese Grenze ausschließt.
pub const MAX_TO_UNICODE_BYTES: usize = 32 * 1024 * 1024;

/// Was ein Eintrag **neben** seinem Text kostet: Knoten im `BTreeMap`, der
/// eigene Heap-Block des `String`, Verschnitt des Allokators.
///
/// Nachgemessen am Release-Binary: eine Zuordnung mit 65 295 Einträgen zu je
/// 3 Byte Text belegte 17 MB, also 260 Byte je Eintrag. 256 ist die runde Zahl
/// dazu. Ohne diesen Anteil wäre die Rechnung unehrlich — bei einer echten
/// ToUnicode ist der Text 1 bis 3 Byte lang, der Rest ist Verwaltung.
const ENTRY_OVERHEAD: usize = 256;

/// Ergebnis des CMap-Parsers.
#[derive(Debug, Default)]
pub struct ToUnicode {
    /// **Nur gültig, wenn [`ToUnicode::over_limit`] falsch ist.** Ein
    /// abgebrochener Lauf hinterlässt hier ein Bruchstück, und ein Bruchstück
    /// ist die gefährlichste aller Auskünfte: es sähe aus wie „der Font sagt
    /// es selbst“, obwohl die halbe Tabelle fehlt.
    pub map: BTreeMap<u32, String>,
    /// Aus `codespacerange` abgeleitete Code-Breite, falls eindeutig.
    pub code_width: Option<CodeWidth>,
    /// Die Zuordnung hat [`MAX_TO_UNICODE_BYTES`] gesprengt; das Parsen wurde
    /// abgebrochen. Wer das ignoriert, benutzt ein Bruchstück — siehe
    /// [`ToUnicode::map`].
    pub over_limit: bool,
}

/// Nimmt einen Eintrag auf und schreibt seinen Preis fort.
///
/// `false` heißt: die Decke ist gerissen, ab hier wird nichts mehr aufgenommen.
/// Ein Code, der schon dasteht, wird trotzdem berechnet — die strengere
/// Richtung, und sie erspart eine zweite Buchführung.
fn insert_capped(
    map: &mut BTreeMap<u32, String>,
    spent: &mut usize,
    code: u32,
    text: String,
) -> bool {
    *spent = spent.saturating_add(ENTRY_OVERHEAD + text.len());
    if *spent > MAX_TO_UNICODE_BYTES {
        return false;
    }
    map.insert(code, text);
    true
}

/// Parst eine `/ToUnicode`-CMap (bfchar/bfrange/codespacerange).
///
/// Der Parser ist bewusst tolerant: unbekannte Konstrukte werden übersprungen,
/// statt die Extraktion scheitern zu lassen. **Nicht** tolerant ist er
/// gegenüber der Größe — siehe [`MAX_TO_UNICODE_BYTES`] und
/// [`ToUnicode::over_limit`].
pub fn parse_to_unicode(data: &[u8]) -> ToUnicode {
    let tokens = tokenize(data);
    let mut result = ToUnicode::default();
    let mut spent = 0usize;
    let mut i = 0;
    'cmap: while i < tokens.len() {
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
                                if !insert_capped(&mut result.map, &mut spent, code, text) {
                                    result.over_limit = true;
                                    break 'cmap;
                                }
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
                            if !apply_bfrange(&mut result.map, &mut spent, lo, hi, dst) {
                                result.over_limit = true;
                                break 'cmap;
                            }
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

/// `false` heißt: [`MAX_TO_UNICODE_BYTES`] ist erschöpft.
fn apply_bfrange(
    map: &mut BTreeMap<u32, String>,
    spent: &mut usize,
    lo: u32,
    hi: u32,
    dst: &Token,
) -> bool {
    // Schutz gegen absurde Bereiche aus kaputten Dateien.
    let hi = hi.min(lo.saturating_add(0xFFFF));
    match dst {
        Token::Array(items) => {
            for (offset, item) in items.iter().enumerate() {
                // `saturating_add`, weil `lo` aus der Datei stammt: `<FFFFFFFF>
                // <FFFFFFFF> [<0041> <0042>]` ließ den Übertrag paniken.
                let code = lo.saturating_add(offset as u32);
                if code > hi {
                    break;
                }
                if let Some(text) = token_to_text(item) {
                    if !insert_capped(map, spent, code, text) {
                        return false;
                    }
                }
            }
        }
        Token::Hex(bytes) => {
            // Fortlaufende Zuordnung: das letzte UTF-16-Wort wird hochgezählt.
            let units = hex_to_utf16(bytes);
            if units.is_empty() {
                return true;
            }
            for code in lo..=hi {
                let mut u = units.clone();
                let last = u.len() - 1;
                u[last] = u[last].wrapping_add((code - lo) as u16);
                if let Some(text) = utf16_to_string(&u) {
                    if !insert_capped(map, spent, code, text) {
                        return false;
                    }
                }
            }
        }
        Token::Name(name) => {
            if let Some(text) = glyph_name_to_text(name) {
                return insert_capped(map, spent, lo, text);
            }
        }
        _ => {}
    }
    true
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

    // -----------------------------------------------------------------------
    // Aufgabe #19 — ohne Auskunft wird nicht geraten
    // -----------------------------------------------------------------------

    #[test]
    fn two_byte_codes_without_any_mapping_stay_unreadable() {
        // Identity-H-Subset ohne /ToUnicode und ohne herleitbare Zuordnung:
        // die CIDs liegen hier zufällig im druckbaren ASCII-Bereich. Sie als
        // Text durchzureichen erzeugt lesbar aussehenden Unsinn — und damit
        // eine Schwärzung, die nichts findet und trotzdem Erfolg meldet.
        let cm = CharMap::two_byte();
        for code in [1u32, 40, 65, 0x1234] {
            assert_eq!(cm.text_for(code), REPLACEMENT.to_string(), "Code {code}");
        }
        assert!(!cm.has_text_mapping());
    }

    #[test]
    fn a_derived_mapping_decodes_but_is_not_a_to_unicode() {
        let mut cm = CharMap::two_byte();
        let mut m = BTreeMap::new();
        m.insert(1u32, "K".to_string());
        cm.set_derived(m);
        assert_eq!(cm.text_for(1), "K");
        assert_eq!(cm.text_for(2), REPLACEMENT.to_string());
        assert!(cm.has_text_mapping());
        // Hergeleitet ist nicht dasselbe wie „der Font sagt es selbst“: die
        // Warnstatistik muss die Lücken weiterhin zählen dürfen.
        assert!(!cm.has_to_unicode());
    }

    #[test]
    fn a_real_to_unicode_beats_a_derived_mapping() {
        let mut cm = CharMap::two_byte();
        let mut derived = BTreeMap::new();
        derived.insert(1u32, "X".to_string());
        cm.set_derived(derived);
        let mut real = BTreeMap::new();
        real.insert(1u32, "K".to_string());
        cm.set_to_unicode(real);
        assert_eq!(cm.text_for(1), "K");
    }

    #[test]
    fn ucs2_cmaps_treat_the_code_as_a_codepoint() {
        let mut cm = CharMap::two_byte();
        cm.set_code_is_unicode(true);
        assert_eq!(cm.text_for(0x004B), "K");
        assert_eq!(cm.text_for(0x00E4), "ä");
        assert_eq!(cm.text_for(0x0001), REPLACEMENT.to_string());
    }

    #[test]
    fn one_byte_encodings_keep_their_identity_fallback() {
        // Bei einem echten Basis-Encoding deckt die Tabelle 32..255 ab; der
        // Rückfall bleibt für den Rest erhalten und ändert nichts.
        let cm = CharMap::one_byte(win_ansi_encoding());
        assert_eq!(cm.text_for(b'A' as u32), "A");
        assert_eq!(cm.text_for(0x81), REPLACEMENT.to_string());
    }

    /// Der Test ist bestanden, wenn dieser Aufruf zurückkehrt: kaputte
    /// `/ToUnicode`-Daten dürfen nicht paniken. Eine Zusicherung über das
    /// Ergebnis gibt es hier bewusst nicht — welche Reste ein abgeschnittenes
    /// `beginbfchar` hinterlässt, ist nicht festgelegt und soll es nicht sein.
    #[test]
    fn tolerates_broken_cmap() {
        parse_to_unicode(b"beginbfchar <00 endbfchar garbage [ ] >>");
    }
}
