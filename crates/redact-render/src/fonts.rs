//! Glyph-Umrisse aus PDF-Fontprogrammen.
//!
//! Der Rasterizer braucht Füllpfade, keine Bitmaps. Dieses Modul nimmt ein
//! Fontprogramm — eingebettet aus dem PDF oder den mitgelieferten Ersatzfont —
//! und liefert pro Glyphe eine Folge von [`Seg`]-Kommandos in Font-Einheiten
//! (y zeigt nach oben, Ursprung auf der Grundlinie).
//!
//! # Unterstützte Formate
//!
//! | PDF-Eintrag                     | Format                     | Weg                       |
//! |---------------------------------|----------------------------|---------------------------|
//! | `/FontFile2`                    | TrueType (`glyf`)          | direkt                    |
//! | `/FontFile3` `/OpenType`        | OpenType (`glyf` o. `CFF `)| direkt                    |
//! | `/FontFile3` `/Type1C`, `/CIDFontType0C` | rohe CFF-Tabelle  | in `OTTO` verpackt        |
//! | —                               | OpenType-Collection (`ttcf`) | erster Font der Datei   |
//! | `/FontFile`                     | Type1 (PFB/PFA)            | **nicht** unterstützt     |
//!
//! Type1 ist in modernen PDFs selten; solche Fonts liefern hier `None`, und der
//! Aufrufer nimmt über [`FontCache::get_or_load`] automatisch den Ersatzfont.
//!
//! # Robustheit
//!
//! Jeder Einstiegspunkt gibt `Option` zurück und übersteht beliebigen Müll:
//! `skrifa` parst ausschließlich mit Bereichsprüfung, und der eigene
//! CFF-Vorparser unten benutzt durchgängig `get()`/`checked_*`.

use std::borrow::Cow;
use std::collections::HashMap;

use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::TableProvider;
use skrifa::{FontRef, GlyphId, MetadataProvider};

// ---------------------------------------------------------------------------
// Mitgelieferte Ersatzfonts (siehe Crate-Doku für Herkunft und Lizenz)
// ---------------------------------------------------------------------------

macro_rules! fallback_face {
    ($name:literal) => {
        include_bytes!(concat!("../assets/fonts/", $name, ".ttf"))
    };
}

/// Sans / Serif / Mono, jeweils Regular, Bold, Italic, Bold-Italic.
const FALLBACK_FACES: [&[u8]; 12] = [
    fallback_face!("sans-regular"),
    fallback_face!("sans-bold"),
    fallback_face!("sans-italic"),
    fallback_face!("sans-bolditalic"),
    fallback_face!("serif-regular"),
    fallback_face!("serif-bold"),
    fallback_face!("serif-italic"),
    fallback_face!("serif-bolditalic"),
    fallback_face!("mono-regular"),
    fallback_face!("mono-bold"),
    fallback_face!("mono-italic"),
    fallback_face!("mono-bolditalic"),
];

/// Voreinstellung, wenn ein Font seine Einheiten nicht verrät.
const DEFAULT_UNITS_PER_EM: u16 = 1000;

// ---------------------------------------------------------------------------
// Öffentliche Datentypen
// ---------------------------------------------------------------------------

/// Wie eine Glyphe adressiert wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GlyphKey {
    /// Direkte Glyph-ID (CID-Fonts mit Identity-Mapping).
    Gid(u16),
    /// Über ein Unicode-Zeichen (einfache Fonts, Ersatzfont).
    Char(char),
}

/// Ein einzelnes Pfadkommando.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    MoveTo(f64, f64),
    LineTo(f64, f64),
    QuadTo(f64, f64, f64, f64),
    CubicTo(f64, f64, f64, f64, f64, f64),
    Close,
}

/// Ein Umriss in Font-Einheiten (y zeigt nach oben, Ursprung auf der Grundlinie).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outline {
    pub segments: Vec<Seg>,
    pub units_per_em: f64,
}

impl Outline {
    /// Bounding-Box `(x_min, y_min, x_max, y_max)` über alle Stützpunkte.
    ///
    /// Kontrollpunkte zählen mit, die Box kann also etwas zu groß ausfallen —
    /// für Plausibilitätsprüfungen und grobes Culling reicht das.
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        let mut b: Option<(f64, f64, f64, f64)> = None;
        let mut add = |x: f64, y: f64| {
            let e = b.get_or_insert((x, y, x, y));
            e.0 = e.0.min(x);
            e.1 = e.1.min(y);
            e.2 = e.2.max(x);
            e.3 = e.3.max(y);
        };
        for seg in &self.segments {
            match *seg {
                Seg::MoveTo(x, y) | Seg::LineTo(x, y) => add(x, y),
                Seg::QuadTo(cx, cy, x, y) => {
                    add(cx, cy);
                    add(x, y);
                }
                Seg::CubicTo(cx0, cy0, cx1, cy1, x, y) => {
                    add(cx0, cy0);
                    add(cx1, cy1);
                    add(x, y);
                }
                Seg::Close => {}
            }
        }
        b
    }

    /// Anzahl der Konturen (also der `MoveTo`-Kommandos).
    pub fn contour_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|s| matches!(s, Seg::MoveTo(..)))
            .count()
    }
}

// ---------------------------------------------------------------------------
// GlyphFont
// ---------------------------------------------------------------------------

/// Ein geladenes Fontprogramm, aus dem Glyph-Umrisse geholt werden können.
///
/// Die Bytes werden mitbesessen (`Cow`, damit die eingebauten Ersatzfonts ohne
/// Kopie auskommen). Der `FontRef` wird pro Zugriff neu gebaut — das kostet nur
/// das Lesen des Tabellenverzeichnisses und erspart eine selbstbezügliche
/// Struktur.
pub struct GlyphFont {
    data: Cow<'static, [u8]>,
    /// Index innerhalb einer OpenType-Collection.
    index: u32,
    units_per_em: f64,
    glyph_count: u16,
    /// True, wenn hmtx nur vom eigenen `OTTO`-Wrapper stammt und keine echten
    /// Vorschübe enthält.
    synthetic_metrics: bool,
}

impl std::fmt::Debug for GlyphFont {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlyphFont")
            .field("bytes", &self.data.len())
            .field("index", &self.index)
            .field("units_per_em", &self.units_per_em)
            .field("glyph_count", &self.glyph_count)
            .finish()
    }
}

impl GlyphFont {
    /// Lädt ein eingebettetes Fontprogramm (TrueType, OpenType/CFF, bare CFF).
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        Self::from_cow(Cow::Owned(data.to_vec()))
    }

    /// Wie [`GlyphFont::from_bytes`], aber ohne Kopie für `'static`-Daten.
    pub fn from_static(data: &'static [u8]) -> Option<Self> {
        Self::from_cow(Cow::Borrowed(data))
    }

    fn from_cow(data: Cow<'static, [u8]>) -> Option<Self> {
        // 1. Versuch: echtes sfnt (TrueType, OpenType, Collection).
        if let Some(font) = Self::probe(data.clone(), 0, false) {
            return Some(font);
        }
        // 2. Versuch: rohe CFF-Tabelle aus /FontFile3 (Subtype /Type1C).
        let info = parse_bare_cff(&data)?;
        let wrapped = wrap_cff_in_otf(&data, info.glyph_count, info.units_per_em);
        Self::probe(Cow::Owned(wrapped), 0, true)
    }

    /// Prüft, ob `skrifa` die Bytes lesen kann und Umrisse liefert.
    fn probe(data: Cow<'static, [u8]>, index: u32, synthetic_metrics: bool) -> Option<Self> {
        let (units_per_em, glyph_count) = {
            let font = FontRef::from_index(&data, index).ok()?;
            // Ohne Umrisstabelle ist der Font für uns wertlos.
            font.outline_glyphs().format()?;
            let metrics = font.metrics(Size::unscaled(), LocationRef::default());
            let upem = if metrics.units_per_em == 0 {
                DEFAULT_UNITS_PER_EM
            } else {
                metrics.units_per_em
            };
            let count = font
                .maxp()
                .map(|m| m.num_glyphs())
                .unwrap_or(metrics.glyph_count);
            (upem, count)
        };
        Some(Self {
            data,
            index,
            units_per_em: f64::from(units_per_em),
            glyph_count,
            synthetic_metrics,
        })
    }

    /// Ersatzfont für nicht eingebettete Fonts. `serif`/`bold`/`italic`/`mono`
    /// wählen die passende Variante des mitgelieferten Fonts; `mono` hat
    /// Vorrang vor `serif`.
    pub fn fallback(serif: bool, bold: bool, italic: bool, mono: bool) -> Self {
        let family = if mono {
            2
        } else if serif {
            1
        } else {
            0
        };
        let style = usize::from(bold) + 2 * usize::from(italic);
        let bytes = FALLBACK_FACES[family * 4 + style];
        Self::from_static(bytes).unwrap_or_else(Self::empty)
    }

    /// Notnagel, falls ein mitgelieferter Font kaputt wäre: ein Font ohne
    /// Glyphen, der niemals panickt.
    fn empty() -> Self {
        Self {
            data: Cow::Borrowed(&[]),
            index: 0,
            units_per_em: f64::from(DEFAULT_UNITS_PER_EM),
            glyph_count: 0,
            synthetic_metrics: true,
        }
    }

    pub fn units_per_em(&self) -> f64 {
        self.units_per_em
    }

    /// Anzahl Glyphen.
    pub fn glyph_count(&self) -> u16 {
        self.glyph_count
    }

    fn font_ref(&self) -> Option<FontRef<'_>> {
        FontRef::from_index(&self.data, self.index).ok()
    }

    /// Löst einen [`GlyphKey`] in eine Glyph-ID auf.
    ///
    /// `Char` geht über die `cmap` des Fonts; hat er keine brauchbare (bei
    /// symbolischen Subsets üblich), gibt es `None` und der Aufrufer kann auf
    /// den Ersatzfont ausweichen.
    fn resolve(&self, font: &FontRef<'_>, key: GlyphKey) -> Option<GlyphId> {
        match key {
            GlyphKey::Gid(gid) => {
                if gid < self.glyph_count {
                    Some(GlyphId::from(gid))
                } else {
                    None
                }
            }
            GlyphKey::Char(ch) => font.charmap().map(ch),
        }
    }

    /// Umriss einer Glyphe; `None`, wenn es sie nicht gibt oder sie leer ist
    /// (z.B. das Leerzeichen).
    pub fn outline(&self, key: GlyphKey) -> Option<Outline> {
        let font = self.font_ref()?;
        let gid = self.resolve(&font, key)?;
        let glyph = font.outline_glyphs().get(gid)?;
        let mut pen = SegPen::default();
        let settings = DrawSettings::unhinted(Size::unscaled(), LocationRef::default());
        glyph.draw(settings, &mut pen).ok()?;
        if pen.segments.is_empty() {
            return None;
        }
        Some(Outline {
            segments: pen.segments,
            units_per_em: self.units_per_em,
        })
    }

    /// Vorschub in Font-Einheiten, falls der Font ihn kennt.
    ///
    /// Bei roher CFF-Eingabe gibt es keine `hmtx`-Tabelle; dann `None` — die
    /// Breiten stehen in dem Fall ohnehin im PDF (`/Widths` bzw. `/W`).
    pub fn advance(&self, key: GlyphKey) -> Option<f64> {
        if self.synthetic_metrics {
            return None;
        }
        let font = self.font_ref()?;
        let gid = self.resolve(&font, key)?;
        font.glyph_metrics(Size::unscaled(), LocationRef::default())
            .advance_width(gid)
            .map(f64::from)
    }
}

/// Sammelt die Kommandos, die `skrifa` beim Zeichnen ausgibt.
#[derive(Default)]
struct SegPen {
    segments: Vec<Seg>,
}

impl OutlinePen for SegPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.segments.push(Seg::MoveTo(f64::from(x), f64::from(y)));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.segments.push(Seg::LineTo(f64::from(x), f64::from(y)));
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.segments.push(Seg::QuadTo(
            f64::from(cx),
            f64::from(cy),
            f64::from(x),
            f64::from(y),
        ));
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.segments.push(Seg::CubicTo(
            f64::from(cx0),
            f64::from(cy0),
            f64::from(cx1),
            f64::from(cy1),
            f64::from(x),
            f64::from(y),
        ));
    }

    fn close(&mut self) {
        self.segments.push(Seg::Close);
    }
}

// ---------------------------------------------------------------------------
// Rohe CFF-Tabellen in einen OpenType-Container verpacken
// ---------------------------------------------------------------------------

/// Das Wenige, das aus einem rohen CFF-Block gelesen werden muss, um einen
/// `OTTO`-Container drumherum bauen zu können.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CffInfo {
    glyph_count: u16,
    units_per_em: u16,
}

/// Liest Glyphenzahl und Em-Größe aus einer rohen CFF-Tabelle.
///
/// `skrifa` erwartet eine vollständige Font-Datei, `/FontFile3` mit
/// `/Subtype /Type1C` liefert aber nur die nackte `CFF `-Tabelle. Um sie
/// einpacken zu können, brauchen wir `numGlyphs` (für `maxp` und `hmtx`) und
/// `unitsPerEm` (für `head`); beides steht im CFF selbst — die Glyphenzahl im
/// CharStrings-INDEX, die Em-Größe im Kehrwert von `FontMatrix[0]`.
fn parse_bare_cff(data: &[u8]) -> Option<CffInfo> {
    // Header: major, minor, hdrSize, offSize.
    if *data.first()? != 1 {
        return None;
    }
    let hdr_size = usize::from(*data.get(2)?);
    let off_size = usize::from(*data.get(3)?);
    if hdr_size < 4 || !(1..=4).contains(&off_size) {
        return None;
    }

    let mut pos = hdr_size;
    pos = cff_index_end(data, pos)?; // Name INDEX
    let (top_dicts, next) = cff_index(data, pos)?; // Top DICT INDEX
    pos = next;
    pos = cff_index_end(data, pos)?; // String INDEX
    cff_index_end(data, pos)?; // Global Subr INDEX

    let top = top_dicts.first()?;
    let top_dict = data.get(top.0..top.1)?;
    let (charstrings_off, matrix0) = cff_top_dict(top_dict)?;

    let glyph_count = cff_index_count(data, charstrings_off?)?;
    let units_per_em = matrix0
        .filter(|m| m.is_finite() && *m > 1e-9)
        .map(|m| (1.0 / m).round())
        .filter(|u| (1.0..=16384.0).contains(u))
        .map(|u| u as u16)
        .unwrap_or(DEFAULT_UNITS_PER_EM);

    Some(CffInfo {
        glyph_count,
        units_per_em,
    })
}

/// Liest einen CFF-INDEX und liefert die Bereiche seiner Einträge sowie die
/// Position dahinter.
fn cff_index(data: &[u8], pos: usize) -> Option<(Vec<(usize, usize)>, usize)> {
    let count = usize::from(be_u16(data, pos)?);
    if count == 0 {
        return Some((Vec::new(), pos.checked_add(2)?));
    }
    let off_size = usize::from(*data.get(pos.checked_add(2)?)?);
    if !(1..=4).contains(&off_size) {
        return None;
    }
    let offsets_at = pos.checked_add(3)?;
    let n = count.checked_add(1)?;
    // `data_base` ist der Nullpunkt der 1-basierten Offsets.
    let data_base = offsets_at
        .checked_add(n.checked_mul(off_size)?)?
        .checked_sub(1)?;

    let mut entries = Vec::with_capacity(count);
    let mut prev = be_offset(data, offsets_at, off_size)?;
    if prev < 1 {
        return None;
    }
    for i in 1..n {
        let cur = be_offset(
            data,
            offsets_at.checked_add(i.checked_mul(off_size)?)?,
            off_size,
        )?;
        let start = data_base.checked_add(prev)?;
        let end = data_base.checked_add(cur)?;
        if start > end || end > data.len() {
            return None;
        }
        entries.push((start, end));
        prev = cur;
    }
    Some((entries, data_base.checked_add(prev)?))
}

/// Wie [`cff_index`], aber nur die Position hinter dem INDEX.
fn cff_index_end(data: &[u8], pos: usize) -> Option<usize> {
    cff_index(data, pos).map(|(_, end)| end)
}

/// Anzahl der Einträge eines INDEX (der CharStrings-INDEX kann sehr groß sein,
/// deshalb hier ohne Aufbau der Bereichsliste).
fn cff_index_count(data: &[u8], pos: usize) -> Option<u16> {
    be_u16(data, pos)
}

/// Liefert `(CharStrings-Offset, FontMatrix[0])` aus einem Top DICT.
fn cff_top_dict(dict: &[u8]) -> Option<(Option<usize>, Option<f64>)> {
    let mut operands: Vec<f64> = Vec::new();
    let mut charstrings = None;
    let mut matrix0 = None;
    let mut i = 0usize;
    while i < dict.len() {
        let b0 = dict[i];
        match b0 {
            0..=21 => {
                // Operator; 12 leitet einen Zweibyte-Operator ein.
                let op = if b0 == 12 {
                    i += 1;
                    0x0c00 | u16::from(*dict.get(i)?)
                } else {
                    u16::from(b0)
                };
                match op {
                    17 => charstrings = operands.last().map(|v| *v as usize),
                    0x0c07 => matrix0 = operands.first().copied(),
                    _ => {}
                }
                operands.clear();
                i += 1;
            }
            28 => {
                let v = i16::from_be_bytes([*dict.get(i + 1)?, *dict.get(i + 2)?]);
                operands.push(f64::from(v));
                i += 3;
            }
            29 => {
                let v = i32::from_be_bytes([
                    *dict.get(i + 1)?,
                    *dict.get(i + 2)?,
                    *dict.get(i + 3)?,
                    *dict.get(i + 4)?,
                ]);
                operands.push(f64::from(v));
                i += 5;
            }
            30 => {
                let (value, next) = cff_real(dict, i + 1)?;
                operands.push(value);
                i = next;
            }
            32..=246 => {
                operands.push(f64::from(i32::from(b0) - 139));
                i += 1;
            }
            247..=250 => {
                let b1 = i32::from(*dict.get(i + 1)?);
                operands.push(f64::from((i32::from(b0) - 247) * 256 + b1 + 108));
                i += 2;
            }
            251..=254 => {
                let b1 = i32::from(*dict.get(i + 1)?);
                operands.push(f64::from(-(i32::from(b0) - 251) * 256 - b1 - 108));
                i += 2;
            }
            // 22..=27, 31 und 255 sind reserviert — hier ist der DICT kaputt.
            _ => return None,
        }
        // Ein Top DICT mit absurd vielen Operanden ist Müll.
        if operands.len() > 64 {
            return None;
        }
    }
    Some((charstrings, matrix0))
}

/// Dekodiert eine CFF-Realzahl (Nibble-Codierung) ab `pos`.
fn cff_real(dict: &[u8], pos: usize) -> Option<(f64, usize)> {
    let mut text = String::new();
    let mut i = pos;
    loop {
        let byte = *dict.get(i)?;
        i += 1;
        for nibble in [byte >> 4, byte & 0x0f] {
            match nibble {
                0..=9 => text.push((b'0' + nibble) as char),
                0xa => text.push('.'),
                0xb => text.push('E'),
                0xc => text.push_str("E-"),
                0xe => text.push('-'),
                0xf => return Some((text.parse().unwrap_or(0.0), i)),
                _ => {}
            }
        }
        // Realzahlen sind kurz; alles andere ist kaputt.
        if text.len() > 32 {
            return None;
        }
    }
}

/// Baut einen minimalen `OTTO`-Container um eine rohe CFF-Tabelle.
///
/// `skrifa` braucht für CFF-Umrisse `head` (Em-Größe), `maxp` (Glyphenzahl)
/// sowie `hhea`+`hmtx` (die Metrik-Quelle). Alle vier werden hier synthetisiert;
/// die Vorschübe sind null, deshalb liefert [`GlyphFont::advance`] für solche
/// Fonts `None`.
fn wrap_cff_in_otf(cff: &[u8], glyph_count: u16, units_per_em: u16) -> Vec<u8> {
    let num_glyphs = glyph_count.max(1);
    let upem = if units_per_em == 0 {
        DEFAULT_UNITS_PER_EM
    } else {
        units_per_em
    };

    let mut head = Vec::with_capacity(54);
    head.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // version
    head.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // fontRevision
    head.extend_from_slice(&0u32.to_be_bytes()); // checkSumAdjustment
    head.extend_from_slice(&0x5F0F_3CF5u32.to_be_bytes()); // magicNumber
    head.extend_from_slice(&0u16.to_be_bytes()); // flags
    head.extend_from_slice(&upem.to_be_bytes());
    head.extend_from_slice(&[0u8; 16]); // created + modified
    for _ in 0..4 {
        head.extend_from_slice(&0i16.to_be_bytes()); // xMin/yMin/xMax/yMax
    }
    head.extend_from_slice(&0u16.to_be_bytes()); // macStyle
    head.extend_from_slice(&8u16.to_be_bytes()); // lowestRecPPEM
    head.extend_from_slice(&2i16.to_be_bytes()); // fontDirectionHint
    head.extend_from_slice(&0i16.to_be_bytes()); // indexToLocFormat
    head.extend_from_slice(&0i16.to_be_bytes()); // glyphDataFormat

    let mut hhea = Vec::with_capacity(36);
    hhea.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // version
    hhea.extend_from_slice(&((upem as i32 * 4 / 5) as i16).to_be_bytes()); // ascender
    hhea.extend_from_slice(&(-(upem as i32 / 5) as i16).to_be_bytes()); // descender
    hhea.extend_from_slice(&0i16.to_be_bytes()); // lineGap
    hhea.extend_from_slice(&upem.to_be_bytes()); // advanceWidthMax
    hhea.extend_from_slice(&0i16.to_be_bytes()); // minLeftSideBearing
    hhea.extend_from_slice(&0i16.to_be_bytes()); // minRightSideBearing
    hhea.extend_from_slice(&0i16.to_be_bytes()); // xMaxExtent
    hhea.extend_from_slice(&1i16.to_be_bytes()); // caretSlopeRise
    hhea.extend_from_slice(&0i16.to_be_bytes()); // caretSlopeRun
    hhea.extend_from_slice(&0i16.to_be_bytes()); // caretOffset
    hhea.extend_from_slice(&[0u8; 8]); // reserved
    hhea.extend_from_slice(&0i16.to_be_bytes()); // metricDataFormat
    hhea.extend_from_slice(&1u16.to_be_bytes()); // numberOfHMetrics

    // Ein einziger longHorMetric (Vorschub 0), danach nur Left-Side-Bearings.
    let hmtx = vec![0u8; 4 + (usize::from(num_glyphs) - 1) * 2];

    let mut maxp = Vec::with_capacity(6);
    maxp.extend_from_slice(&0x0000_5000u32.to_be_bytes()); // Version 0.5 (CFF)
    maxp.extend_from_slice(&num_glyphs.to_be_bytes());

    // Tabellenverzeichnis muss nach Tag sortiert sein.
    let tables: [(&[u8; 4], &[u8]); 5] = [
        (b"CFF ", cff),
        (b"head", &head),
        (b"hhea", &hhea),
        (b"hmtx", &hmtx),
        (b"maxp", &maxp),
    ];

    let num_tables = tables.len() as u16;
    // entrySelector = floor(log2(numTables)), searchRange = 16 * 2^entrySelector.
    let entry_selector = 15u16 - num_tables.leading_zeros() as u16;
    let search_range = 16u16 << entry_selector;
    let range_shift = num_tables * 16 - search_range;

    let mut out = Vec::with_capacity(12 + 16 * tables.len() + cff.len() + 128);
    out.extend_from_slice(b"OTTO");
    out.extend_from_slice(&num_tables.to_be_bytes());
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&range_shift.to_be_bytes());

    let mut offset = 12 + 16 * tables.len();
    for (tag, body) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes()); // checkSum: wird nicht geprüft
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        offset += (body.len() + 3) & !3;
    }
    for (_, body) in tables {
        out.extend_from_slice(body);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    out
}

fn be_u16(data: &[u8], pos: usize) -> Option<u16> {
    Some(u16::from_be_bytes([
        *data.get(pos)?,
        *data.get(pos.checked_add(1)?)?,
    ]))
}

fn be_offset(data: &[u8], pos: usize, size: usize) -> Option<usize> {
    let mut value = 0usize;
    for i in 0..size {
        value = (value << 8) | usize::from(*data.get(pos.checked_add(i)?)?);
    }
    Some(value)
}

// ---------------------------------------------------------------------------
// FontCache
// ---------------------------------------------------------------------------

/// Cache, damit ein Umriss pro Font/Glyphe nur einmal gebaut wird.
#[derive(Default)]
pub struct FontCache {
    fonts: HashMap<String, GlyphFont>,
    outlines: HashMap<String, HashMap<GlyphKey, Option<Outline>>>,
}

impl FontCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Lädt aus den Bytes, oder liefert den passenden Ersatzfont, wenn `data`
    /// fehlt oder unbrauchbar ist. `key` identifiziert den Font eindeutig
    /// (z.B. der BaseFont-Name plus Index).
    pub fn get_or_load(
        &mut self,
        key: &str,
        data: Option<&[u8]>,
        serif: bool,
        bold: bool,
        italic: bool,
        mono: bool,
    ) -> &GlyphFont {
        if !self.fonts.contains_key(key) {
            let font = data
                .and_then(GlyphFont::from_bytes)
                .unwrap_or_else(|| GlyphFont::fallback(serif, bold, italic, mono));
            self.fonts.insert(key.to_owned(), font);
        }
        // `contains_key` oben garantiert den Eintrag.
        self.fonts.get(key).expect("gerade eingefügt")
    }

    /// Umriss aus dem Cache; baut ihn beim ersten Zugriff.
    ///
    /// Voraussetzung ist, dass [`FontCache::get_or_load`] für `key` schon
    /// aufgerufen wurde — sonst gibt es `None`.
    pub fn outline(&mut self, key: &str, glyph: GlyphKey) -> Option<Outline> {
        if let Some(cached) = self.outlines.get(key).and_then(|m| m.get(&glyph)) {
            return cached.clone();
        }
        let outline = self.fonts.get(key)?.outline(glyph);
        self.outlines
            .entry(key.to_owned())
            .or_default()
            .insert(glyph, outline.clone());
        outline
    }

    /// Anzahl geladener Fonts.
    pub fn len(&self) -> usize {
        self.fonts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fonts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cff_real_decodes_font_matrix_value() {
        // 0.001 als CFF-Realzahl: 0 . 0 0 1 <end>
        let bytes = [0x0a, 0x00, 0x1f];
        let (value, next) = cff_real(&bytes, 0).expect("Realzahl");
        assert!((value - 0.001).abs() < 1e-12, "{value}");
        assert_eq!(next, 3);
    }

    #[test]
    fn cff_real_survives_truncation() {
        assert_eq!(cff_real(&[0x0a, 0x00], 0), None);
    }

    #[test]
    fn otf_wrapper_has_sorted_table_directory() {
        let wrapped = wrap_cff_in_otf(&[1, 0, 4, 1], 3, 1000);
        assert_eq!(&wrapped[0..4], b"OTTO");
        assert_eq!(be_u16(&wrapped, 4), Some(5)); // numTables
        assert_eq!(be_u16(&wrapped, 6), Some(64)); // searchRange
        assert_eq!(be_u16(&wrapped, 8), Some(2)); // entrySelector
        assert_eq!(be_u16(&wrapped, 10), Some(16)); // rangeShift
        let mut previous = [0u8; 4];
        for i in 0..5 {
            let at = 12 + 16 * i;
            let tag: [u8; 4] = wrapped[at..at + 4].try_into().unwrap();
            assert!(tag > previous, "Tags unsortiert: {tag:?}");
            previous = tag;
        }
    }

    #[test]
    fn bare_cff_parser_rejects_garbage() {
        assert_eq!(parse_bare_cff(&[]), None);
        assert_eq!(parse_bare_cff(&[1, 0, 4]), None);
        assert_eq!(parse_bare_cff(&[9, 9, 9, 9, 9, 9]), None);
    }
}
