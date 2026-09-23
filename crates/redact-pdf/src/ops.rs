//! Vollständiger, aufgelöster Zeichenoperationen-Strom einer Seite.
//!
//! Der GUI-Renderer braucht mehr als Textzeilen: Pfade, Farben, Bilder und
//! Glyphen — und zwar in genau denselben Koordinaten, mit denen auch geschwärzt
//! wird. Deshalb entsteht [`PageOps`] **nicht** aus einem zweiten Interpreter,
//! sondern aus derselben Durchlaufschleife wie [`crate::content::scan_page`]:
//! [`OpsCollector`] ist lediglich eine andere [`ContentSink`].
//!
//! Alles ist bereits aufgelöst:
//!
//! * Pfadpunkte liegen im User-Space (CTM angewendet),
//! * Farben sind RGB (CMYK/Graustufen/Indexed sind umgerechnet),
//! * Glyphen tragen eine Matrix, die Glyph-Space direkt auf User-Space abbildet,
//! * Bilder sind entpackte RGBA8-Puffer.
//!
//! ## Was genähert oder übergangen wird
//!
//! * **Clip**: nur der zuletzt gesetzte Pfad wird referenziert; echte
//!   Schnittmengen verschachtelter Clips werden nicht gebildet.
//! * **`sh` (Schattierungen)** und Muster (`Pattern`) werden nicht gezeichnet;
//!   gemusterte Flächen bekommen mittleres Grau.
//! * **Type3-Fonts** liefern kein Fontprogramm (ihre Glyphen sind selbst
//!   Content-Streams).
//! * **Inline-Bilder** werden auf Seitenebene erkannt; in Form-XObjects nicht,
//!   weil dort die Operationsindizes zur Schwärzung passen müssen.
//! * **`LZWDecode`/`CCITTFaxDecode`/`JPXDecode`** werden nicht dekodiert,
//!   sondern durch einen Platzhalter ersetzt (siehe [`PageOps::notes`]).

use std::collections::{BTreeMap, HashMap, HashSet};

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use redact_core::{Point, Rect, RedactError, Result};

use crate::content::{
    interpret, ColorSpace, ContentSink, GlyphEvent, ImageEvent, PathEvent, SinkContext, StreamKey,
};
use crate::font::{as_f64, deref, FontInfo};
use crate::matrix::Matrix;

/// Obergrenze für dekodierte Bilder, **je Bild**.
///
/// Sie sagt nichts über die Summe: ein Dokument darf beliebig viele Bilder
/// knapp unterhalb dieser Grenze enthalten. Was gleichzeitig gehalten werden
/// darf, regelt [`crate::image::ImageOptions::max_decoded_bytes`].
pub const MAX_IMAGE_PIXELS: u64 = 40_000_000;

// ---------------------------------------------------------------------------
// Datentypen
// ---------------------------------------------------------------------------

/// Eine Farbe im Bereich 0.0..=1.0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl Rgb {
    pub const BLACK: Rgb = Rgb {
        r: 0.0,
        g: 0.0,
        b: 0.0,
    };

    pub fn new(r: f64, g: f64, b: f64) -> Self {
        Self {
            r: r.clamp(0.0, 1.0) as f32,
            g: g.clamp(0.0, 1.0) as f32,
            b: b.clamp(0.0, 1.0) as f32,
        }
    }

    pub fn gray(v: f64) -> Self {
        Self::new(v, v, v)
    }

    /// Als 8-Bit-Tripel, wie es ein Rasterpuffer braucht.
    pub fn to_u8(self) -> [u8; 3] {
        [
            (self.r * 255.0).round().clamp(0.0, 255.0) as u8,
            (self.g * 255.0).round().clamp(0.0, 255.0) as u8,
            (self.b * 255.0).round().clamp(0.0, 255.0) as u8,
        ]
    }
}

/// Ein Pfadsegment; alle Punkte liegen im User-Space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathSeg {
    MoveTo(Point),
    LineTo(Point),
    CubicTo(Point, Point, Point),
    Close,
}

/// Strichparameter, bereits in User-Space-Einheiten.
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    pub color: Rgb,
    pub width: f64,
    pub cap: u8,
    pub join: u8,
    pub dash: Vec<f64>,
    pub dash_phase: f64,
    /// Deckkraft aus `/CA` (0.0..=1.0).
    pub alpha: f32,
}

/// Index in [`PageOps::clips`]; gleiche Clip-Pfade werden nur einmal abgelegt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipRef(pub usize);

/// Eine Zeichenoperation.
#[derive(Debug, Clone)]
pub enum DrawOp {
    /// Pfad. `segments` liegen bereits im User-Space (CTM angewendet).
    Path {
        segments: Vec<PathSeg>,
        fill: Option<Rgb>,
        stroke: Option<Stroke>,
        even_odd: bool,
        /// Deckkraft der Füllung aus `/ca`.
        fill_alpha: f32,
        clip: Option<ClipRef>,
    },
    /// Eine einzelne Glyphe.
    Glyph {
        /// Index in [`PageOps::fonts`].
        font: usize,
        /// Zeichencode wie im Content-Stream (bei CID-Fonts der CID).
        code: u32,
        /// Bildet Glyph-Space (Einheiten pro Em) auf User-Space ab — enthält
        /// bereits Schriftgröße, `Tz`, `Ts`, `Tm` und CTM.
        transform: Matrix,
        fill: Rgb,
        fill_alpha: f32,
        /// PDF-Textrendermodus (3 = unsichtbar, z. B. OCR-Ebene).
        render_mode: u8,
        clip: Option<ClipRef>,
    },
    /// Bild-XObject. `ctm` bildet das Einheitsquadrat (0,0)-(1,1) auf die
    /// Zielfläche im User-Space ab (PDF-Konvention).
    Image {
        /// Index in [`PageOps::images`].
        image: usize,
        ctm: Matrix,
        /// Deckkraft aus `/ca`.
        alpha: f32,
        clip: Option<ClipRef>,
    },
}

/// Art des eingebetteten Fontprogramms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontKind {
    TrueType,
    Cff,
    Type1,
    Type3,
    Unknown,
}

/// Wie aus einem Zeichencode eine Glyph-ID wird.
#[derive(Debug, Clone, PartialEq)]
pub enum CodeToGid {
    /// Code (bzw. CID) ist bereits die Glyph-ID.
    Identity,
    /// Explizite Tabelle aus `/CIDToGIDMap`.
    Map(BTreeMap<u32, u16>),
    /// Einfache Fonts: über Glyphnamen bzw. Unicode auflösen.
    ViaCharCode,
}

/// Alles, was zum Setzen der Glyphen eines Fonts gebraucht wird.
#[derive(Debug, Clone)]
pub struct FontProgram {
    pub base_font: String,
    pub kind: FontKind,
    /// Rohdaten des eingebetteten Fontprogramms
    /// (`/FontFile`, `/FontFile2`, `/FontFile3`).
    pub data: Option<Vec<u8>>,
    /// Einheiten pro Em: 1000 für Type1/CFF, sonst aus dem Fontprogramm.
    pub units_per_em: f64,
    pub code_to_gid: CodeToGid,
    pub is_cid: bool,
    /// Breite je Code in Text-Space-Einheiten (wie `FontInfo::width`).
    pub widths: BTreeMap<u32, f64>,
    pub default_width: f64,
    /// Für nicht eingebettete Fonts: Code → Unicode, damit ein Ersatzfont
    /// benutzt werden kann.
    pub code_to_unicode: BTreeMap<u32, String>,
    /// `/FontDescriptor /Flags` (Serif, Symbolic, Italic, …).
    pub flags: u32,
}

impl FontProgram {
    /// Notnagel, wenn der Font nicht auflösbar ist — damit keine Glyphe
    /// verlorengeht.
    fn fallback(name: &[u8]) -> Self {
        Self {
            base_font: String::from_utf8_lossy(name).into_owned(),
            kind: FontKind::Unknown,
            data: None,
            units_per_em: 1000.0,
            code_to_gid: CodeToGid::ViaCharCode,
            is_cid: false,
            widths: BTreeMap::new(),
            default_width: 0.5,
            code_to_unicode: BTreeMap::new(),
            flags: 0,
        }
    }
}

/// Ein entpacktes Bild.
#[derive(Debug, Clone)]
pub struct RasterImage {
    pub width: u32,
    pub height: u32,
    /// Entpackte RGBA8-Daten (`width * height * 4`).
    pub rgba: Vec<u8>,
    /// `true`, wenn das Bild nicht dekodiert werden konnte und hier nur eine
    /// Ersatzfläche steht.
    pub placeholder: bool,
}

impl RasterImage {
    /// Einfarbige Fläche (wird als Platzhalter über die Zielfläche gezogen).
    fn solid(color: [u8; 4], placeholder: bool) -> Self {
        Self {
            width: 1,
            height: 1,
            rgba: color.to_vec(),
            placeholder,
        }
    }

    /// Ein Platzhalter für ein Bild, das gar nicht erst geöffnet werden konnte.
    pub fn placeholder() -> Self {
        Self::solid([220, 220, 220, 255], true)
    }
}

/// Alles, was zum Zeichnen einer Seite gebraucht wird — bereits aufgelöst.
#[derive(Debug, Clone)]
pub struct PageOps {
    /// Seitenindex (0-basiert).
    pub page: usize,
    pub media_box: Rect,
    /// Seitendrehung in Grad (0/90/180/270) aus `/Rotate`, inklusive Vererbung.
    /// Sie wird **nicht** angewendet — das ist Sache des Renderers.
    pub rotate: i64,
    pub fonts: Vec<FontProgram>,
    pub images: Vec<RasterImage>,
    /// Clip-Pfade, auf die [`ClipRef`] zeigt.
    pub clips: Vec<Vec<PathSeg>>,
    pub ops: Vec<DrawOp>,
    /// Hinweise auf genäherte oder übergangene Inhalte.
    pub notes: Vec<String>,
}

impl PageOps {
    fn empty(page: usize, media_box: Rect, rotate: i64) -> Self {
        Self {
            page,
            media_box,
            rotate,
            fonts: Vec::new(),
            images: Vec::new(),
            clips: Vec::new(),
            ops: Vec::new(),
            notes: Vec::new(),
        }
    }

    /// Anzahl der Glyph-Operationen (praktisch für Tests und Diagnose).
    pub fn glyph_count(&self) -> usize {
        self.ops
            .iter()
            .filter(|o| matches!(o, DrawOp::Glyph { .. }))
            .count()
    }
}

// ---------------------------------------------------------------------------
// Einstiegspunkt
// ---------------------------------------------------------------------------

/// Sammelt alle Zeichenoperationen einer Seite (0-basiert).
pub fn page_ops(doc: &Document, page_index: usize) -> Result<PageOps> {
    let pages = doc.get_pages();
    let Some((_, page_id)) = pages.iter().nth(page_index) else {
        // `saturating_add`, weil `page_index` aus fremder Hand kommt: bei
        // `usize::MAX` liefe die 1-basierte Anzeige im Debug-Build über und
        // löste eine Panic aus — ausgerechnet in dem Zweig, der einen Fehler
        // sauber melden soll.
        return Err(RedactError::Pdf(format!(
            "Seite {} existiert nicht",
            page_index.saturating_add(1)
        )));
    };
    let page_id = *page_id;

    let media_box = crate::document::page_box(doc, page_id);
    let rotate = page_rotation(doc, page_id);

    let data = crate::filters::page_content(doc, page_id);
    let operations = decode_content(&data);
    let resources = crate::content::page_resources(doc, page_id);

    let mut collector = OpsCollector {
        out: PageOps::empty(page_index, media_box, rotate),
        fonts: HashMap::new(),
        images: HashMap::new(),
        clips: HashMap::new(),
        seen_notes: HashSet::new(),
    };
    interpret(
        doc,
        &operations,
        StreamKey::Page,
        resources.as_ref(),
        Matrix::IDENTITY,
        &mut collector,
    )?;
    Ok(collector.out)
}

/// `/Rotate` inklusive Vererbung vom Seitenbaum, normiert auf 0/90/180/270.
pub fn page_rotation(doc: &Document, page_id: ObjectId) -> i64 {
    let mut current = Some(page_id);
    let mut depth = 0;
    while let Some(id) = current {
        if depth > 32 {
            break;
        }
        depth += 1;
        let Ok(dict) = doc.get_dictionary(id) else {
            break;
        };
        if let Some(value) = dict
            .get(b"Rotate")
            .ok()
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| o.as_i64().ok())
        {
            let normalized = value.rem_euclid(360);
            // Nur die vier erlaubten Werte; alles andere gilt als 0.
            return if normalized % 90 == 0 { normalized } else { 0 };
        }
        current = match dict.get(b"Parent") {
            Ok(Object::Reference(parent)) => Some(*parent),
            _ => None,
        };
    }
    0
}

// ---------------------------------------------------------------------------
// Die Senke
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum FontKey {
    /// Der Regelfall: der Font ist ein indirektes Objekt.
    Object(ObjectId),
    /// Direkt eingebettetes Font-Dictionary — nur im eigenen Stream eindeutig.
    Local(StreamKey, Vec<u8>),
}

/// Schlüssel eines Clip-Pfads: die Bitmuster seiner Koordinaten.
///
/// [`PathSeg`] trägt `f64` und hat deshalb weder `Eq` noch `Hash`. Bitgleiche
/// Pfade sind aber genau die, aus denen dieselbe Maske entsteht — als
/// Schlüssel taugen die Rohbits also, und sie umgehen dabei die zwei
/// Eigenheiten von `==` auf Fließkomma:
///
/// * `NaN == NaN` ist **falsch**. Ein Pfad mit NaN-Koordinate wäre bei der
///   alten Suche nie wiedererkannt worden — jedes `W n` hätte einen neuen
///   Eintrag und im Rasterizer eine neue Maske erzeugt.
/// * `0.0 == -0.0` ist **wahr**. Solche Pfade stehen jetzt doppelt in der
///   Liste: ein Eintrag zu viel, aber kein falsches Bild.
type ClipKey = Vec<u64>;

fn clip_key(segments: &[PathSeg]) -> ClipKey {
    let mut key = Vec::with_capacity(segments.len() * 3);
    let mut push = |tag: u64, points: &[Point]| {
        key.push(tag);
        for p in points {
            key.push(p.x.to_bits());
            key.push(p.y.to_bits());
        }
    };
    for seg in segments {
        match seg {
            PathSeg::MoveTo(p) => push(0, &[*p]),
            PathSeg::LineTo(p) => push(1, &[*p]),
            PathSeg::CubicTo(a, b, c) => push(2, &[*a, *b, *c]),
            PathSeg::Close => push(3, &[]),
        }
    }
    key
}

struct OpsCollector {
    out: PageOps,
    fonts: HashMap<FontKey, usize>,
    images: HashMap<ObjectId, usize>,
    /// Schon abgelegte Clip-Pfade — **als Menge**, nicht durch Durchsuchen der
    /// Liste.
    ///
    /// Vorher stand hier `clips.iter().position(…)`, also eine Suche über alle
    /// bisherigen Pfade je neuem Pfad: quadratisch. Gemessen (Debug, beide
    /// Fassungen abwechselnd auf derselben Datei, je ein `re W n` mit eigenem
    /// Rechteck):
    ///
    /// | Clips  | Datei   | mit Liste | mit Menge |
    /// |-------:|--------:|----------:|----------:|
    /// |  8 000 |  382 kB |    1,07 s |    0,50 s |
    /// | 16 000 |  769 kB |    3,78 s |    1,04 s |
    /// | 32 000 |  1,5 MB |   14,65 s |    2,16 s |
    /// | 64 000 |  3,1 MB |   59,73 s |    4,23 s |
    ///
    /// Links Faktor 3,5 bis 4,1 je Verdopplung, rechts 2,0. Die Nachbarn
    /// [`OpsCollector::fonts`] und [`OpsCollector::images`] machen es seit
    /// jeher so.
    clips: HashMap<ClipKey, usize>,
    /// Schon vergebene Hinweise — als Menge, aus demselben Grund wie
    /// [`OpsCollector::clips`].
    seen_notes: HashSet<String>,
}

impl OpsCollector {
    fn note(&mut self, text: String) {
        if self.seen_notes.insert(text.clone()) {
            self.out.notes.push(text);
        }
    }

    /// Liefert (und lädt bei Bedarf) den Font zu einem Ressourcennamen.
    fn font_slot(&mut self, cx: &SinkContext, name: &[u8], info: &FontInfo) -> usize {
        let entry = font_entry(cx.doc, cx.resources, name);
        let key = match &entry {
            Some((Some(id), _)) => FontKey::Object(*id),
            _ => FontKey::Local(cx.stream, name.to_vec()),
        };
        if let Some(index) = self.fonts.get(&key) {
            return *index;
        }
        let program = match &entry {
            Some((_, dict)) => load_font_program(cx.doc, dict, info),
            None => FontProgram::fallback(name),
        };
        let index = self.out.fonts.len();
        self.out.fonts.push(program);
        self.fonts.insert(key, index);
        index
    }

    fn push_image(&mut self, image: RasterImage) -> usize {
        let index = self.out.images.len();
        self.out.images.push(image);
        index
    }
}

impl ContentSink for OpsCollector {
    fn wants_graphics(&self) -> bool {
        true
    }

    fn glyph(&mut self, cx: &SinkContext, event: &GlyphEvent) {
        let font = self.font_slot(cx, event.font_name, event.font);
        let upem = self.out.fonts[font].units_per_em;
        let scale = if upem > 0.0 { 1.0 / upem } else { 0.001 };
        // Glyph-Space → Text-Space → User-Space, in einer Matrix.
        let transform = Matrix::scale(scale, scale).mul(&event.trm);
        self.out.ops.push(DrawOp::Glyph {
            font,
            code: event.code,
            transform,
            fill: event.fill,
            fill_alpha: event.fill_alpha,
            render_mode: event.render_mode,
            clip: event.clip.map(ClipRef),
        });
    }

    fn path(&mut self, _cx: &SinkContext, event: &PathEvent) {
        self.out.ops.push(DrawOp::Path {
            segments: event.segments.to_vec(),
            fill: event.fill,
            stroke: event.stroke.clone(),
            even_odd: event.even_odd,
            fill_alpha: event.fill_alpha,
            clip: event.clip.map(ClipRef),
        });
    }

    fn clip(&mut self, _cx: &SinkContext, segments: &[PathSeg], _even_odd: bool) -> Option<usize> {
        let key = clip_key(segments);
        if let Some(index) = self.clips.get(&key) {
            return Some(*index);
        }
        let index = self.out.clips.len();
        self.out.clips.push(segments.to_vec());
        self.clips.insert(key, index);
        Some(index)
    }

    fn image(&mut self, cx: &SinkContext, event: &ImageEvent) {
        let index = if let Some((dict, data)) = event.inline {
            let (image, note) = decode_image(cx.doc, cx.resources, dict, data, event.fill);
            if let Some(note) = note {
                self.note(format!("Inline-Bild: {note}"));
            }
            self.push_image(image)
        } else {
            let Some(name) = event.name else {
                return;
            };
            let Some((id, stream)) = image_xobject(cx.doc, cx.resources, name) else {
                return;
            };
            // Stencil-Masken hängen an der aktuellen Füllfarbe und dürfen
            // deshalb nur zwischengespeichert werden, wenn sie keine sind.
            let is_mask = dict_bool(cx.doc, &stream.dict, b"ImageMask", b"IM");
            let cached = id.filter(|_| !is_mask).and_then(|id| self.images.get(&id));
            match cached {
                Some(index) => *index,
                None => {
                    let (image, note) = decode_image(
                        cx.doc,
                        cx.resources,
                        &stream.dict,
                        &stream.content,
                        event.fill,
                    );
                    if let Some(note) = note {
                        let label = String::from_utf8_lossy(name).into_owned();
                        self.note(format!("Bild /{label}: {note}"));
                    }
                    let index = self.push_image(image);
                    if let Some(id) = id.filter(|_| !is_mask) {
                        self.images.insert(id, index);
                    }
                    index
                }
            }
        };
        self.out.ops.push(DrawOp::Image {
            image: index,
            ctm: event.ctm,
            alpha: event.fill_alpha,
            clip: event.clip.map(ClipRef),
        });
    }
}

// ---------------------------------------------------------------------------
// Fonts
// ---------------------------------------------------------------------------

/// Sucht ein Font-Dictionary in den Ressourcen; gibt zusätzlich seine ObjectId
/// zurück, damit gleiche Fonts nur einmal geladen werden.
fn font_entry(
    doc: &Document,
    resources: Option<&Dictionary>,
    name: &[u8],
) -> Option<(Option<ObjectId>, Dictionary)> {
    let fonts = resources?.get(b"Font").ok()?;
    let (_, fonts) = doc.dereference(fonts).ok()?;
    let entry = fonts.as_dict().ok()?.get(name).ok()?;
    let id = match entry {
        Object::Reference(id) => Some(*id),
        _ => None,
    };
    let (_, resolved) = doc.dereference(entry).ok()?;
    Some((id, resolved.as_dict().ok()?.clone()))
}

/// Baut das Renderer-Fontprogramm aus dem Dictionary und den **schon
/// vorliegenden** Metriken (siehe [`crate::content::GlyphEvent::font`]).
fn load_font_program(doc: &Document, dict: &Dictionary, info: &FontInfo) -> FontProgram {
    let subtype = deref(doc, dict.get(b"Subtype").ok())
        .and_then(|o| o.as_name().ok())
        .map(|n| n.to_vec())
        .unwrap_or_default();
    let is_cid = subtype == b"Type0";

    // Bei Type0 stehen Deskriptor und Fontprogramm im Nachkommen-Font.
    let descendant = if is_cid {
        deref(doc, dict.get(b"DescendantFonts").ok())
            .and_then(|o| o.as_array().ok())
            .and_then(|a| a.first())
            .and_then(|o| deref(doc, Some(o)))
            .and_then(|o| o.as_dict().ok())
            .cloned()
    } else {
        None
    };
    let carrier = descendant.as_ref().unwrap_or(dict);
    let descriptor = deref(doc, carrier.get(b"FontDescriptor").ok())
        .and_then(|o| o.as_dict().ok())
        .cloned();

    let (data, mut kind) = embedded_font(doc, descriptor.as_ref());
    if kind == FontKind::Unknown {
        kind = match subtype.as_slice() {
            b"TrueType" => FontKind::TrueType,
            b"Type1" | b"MMType1" => FontKind::Type1,
            b"Type3" => FontKind::Type3,
            _ => FontKind::Unknown,
        };
    }
    if subtype == b"Type3" {
        kind = FontKind::Type3;
    }

    let units_per_em = data
        .as_deref()
        .and_then(sfnt_units_per_em)
        .or_else(|| type3_units_per_em(doc, dict))
        .unwrap_or(1000.0);

    let code_to_gid = if is_cid {
        match deref(doc, carrier.get(b"CIDToGIDMap").ok()) {
            Some(Object::Stream(stream)) => stream
                .decompressed_content()
                .or_else(|_| stream.get_plain_content())
                .ok()
                .map(|bytes| CodeToGid::Map(cid_to_gid_table(&bytes)))
                .unwrap_or(CodeToGid::Identity),
            // `/Identity` oder gar nichts: CID ist die Glyph-ID. Bei
            // CIDFontType0 (CFF) ist das eine Näherung, die für Subsets stimmt.
            _ => CodeToGid::Identity,
        }
    } else {
        CodeToGid::ViaCharCode
    };

    let widths = info.width_map().clone();
    let mut code_to_unicode = BTreeMap::new();
    if !is_cid {
        for code in 0u32..256 {
            let text = info.charmap.text_for(code);
            if !text.is_empty() && !text.starts_with(crate::encoding::REPLACEMENT) {
                code_to_unicode.insert(code, text);
            }
        }
    } else if data.is_none() && info.charmap.has_to_unicode() {
        // Nicht eingebetteter CID-Font: nur so kann ein Ersatzfont helfen.
        for code in widths.keys() {
            let text = info.charmap.text_for(*code);
            if !text.is_empty() && !text.starts_with(crate::encoding::REPLACEMENT) {
                code_to_unicode.insert(*code, text);
            }
        }
    }

    let flags = descriptor
        .as_ref()
        .and_then(|d| deref(doc, d.get(b"Flags").ok()))
        .and_then(|o| o.as_i64().ok())
        .unwrap_or(0)
        .max(0) as u32;

    FontProgram {
        base_font: info.base_font.clone(),
        kind,
        data,
        units_per_em,
        code_to_gid,
        is_cid,
        widths,
        default_width: info.fallback_width(),
        code_to_unicode,
        flags,
    }
}

/// Holt das eingebettete Fontprogramm aus dem `/FontDescriptor`.
fn embedded_font(doc: &Document, descriptor: Option<&Dictionary>) -> (Option<Vec<u8>>, FontKind) {
    let Some(descriptor) = descriptor else {
        return (None, FontKind::Unknown);
    };
    for (key, kind) in [
        (&b"FontFile2"[..], FontKind::TrueType),
        (&b"FontFile3"[..], FontKind::Cff),
        (&b"FontFile"[..], FontKind::Type1),
    ] {
        let Some(Object::Stream(stream)) = deref(doc, descriptor.get(key).ok()) else {
            continue;
        };
        let Ok(data) = stream
            .decompressed_content()
            .or_else(|_| stream.get_plain_content())
        else {
            continue;
        };
        if data.is_empty() {
            continue;
        }
        // `/FontFile3 /Subtype /OpenType` ist ein vollständiges sfnt.
        let kind = if data.starts_with(b"OTTO")
            || data.starts_with(&[0x00, 0x01, 0x00, 0x00])
            || data.starts_with(b"true")
            || data.starts_with(b"ttcf")
        {
            if data.starts_with(b"OTTO") {
                FontKind::Cff
            } else {
                FontKind::TrueType
            }
        } else {
            kind
        };
        return (Some(data), kind);
    }
    (None, FontKind::Unknown)
}

/// `unitsPerEm` aus der `head`-Tabelle eines sfnt-Fonts.
fn sfnt_units_per_em(data: &[u8]) -> Option<f64> {
    if data.len() < 12 {
        return None;
    }
    let mut base = 0usize;
    if &data[0..4] == b"ttcf" {
        base = be_u32(data, 12)? as usize;
        if base + 12 > data.len() {
            return None;
        }
    } else if !(data.starts_with(&[0x00, 0x01, 0x00, 0x00])
        || data.starts_with(b"true")
        || data.starts_with(b"OTTO"))
    {
        return None;
    }
    let num_tables = be_u16(data, base + 4)? as usize;
    for i in 0..num_tables {
        let entry = base + 12 + i * 16;
        if entry + 16 > data.len() {
            return None;
        }
        if &data[entry..entry + 4] == b"head" {
            let offset = be_u32(data, entry + 8)? as usize;
            let upem = be_u16(data, offset + 18)? as f64;
            return (upem > 0.0).then_some(upem);
        }
    }
    None
}

/// Type3-Fonts geben ihren Glyph-Space über `/FontMatrix` an.
fn type3_units_per_em(doc: &Document, dict: &Dictionary) -> Option<f64> {
    let a = deref(doc, dict.get(b"FontMatrix").ok())?
        .as_array()
        .ok()?
        .first()
        .and_then(|o| deref(doc, Some(o)))
        .and_then(as_f64)?;
    (a.abs() > 1e-9).then(|| 1.0 / a.abs())
}

fn cid_to_gid_table(bytes: &[u8]) -> BTreeMap<u32, u16> {
    let mut map = BTreeMap::new();
    for (cid, pair) in bytes.chunks_exact(2).enumerate() {
        let gid = ((pair[0] as u16) << 8) | pair[1] as u16;
        if gid != 0 {
            map.insert(cid as u32, gid);
        }
    }
    map
}

// ---------------------------------------------------------------------------
// Bilder
// ---------------------------------------------------------------------------

/// Das `/Subtype` eines XObjects — die **eine** Fassung dieser Frage.
///
/// Sie stand fünfmal im Crate, eine davon abweichend: nur hier wurde eine
/// indirekte Referenz aufgelöst, und weil der Torwächter
/// [`crate::content::scan_page`] ohne Auflösung vorher aussteigt, kam diese
/// Fassung nie zum Zug. Gemessen an einer Datei mit `/Subtype 9 0 R → /Image`:
/// „NICHT GEPRÜFT“ und Rückgabewert 3, kein Bild geschwärzt.
///
/// Aufgelöst wird jetzt überall. PDF 32000-1 (7.3.10) erlaubt jedem Wert eine
/// indirekte Referenz; wer sie hier nicht auflöst, hält ein Bild für
/// „unbekanntes XObject“ und lässt seine Pixel stehen. Ein Zyklus kann daraus
/// nichts machen: `Document::dereference` bricht nach `DEREF_LIMIT` Schritten
/// ab.
pub(crate) fn xobject_subtype<'a>(doc: &'a Document, dict: &'a Dictionary) -> Option<&'a [u8]> {
    deref(doc, dict.get(b"Subtype").ok())?.as_name().ok()
}

/// Sucht ein Bild-XObject in den Ressourcen.
///
/// Gibt eine Referenz zurück — gescannte Seiten bringen zweistellige
/// Megabyte-Streams mit, die nicht je Platzierung kopiert werden dürfen.
fn image_xobject<'a>(
    doc: &'a Document,
    resources: Option<&'a Dictionary>,
    name: &[u8],
) -> Option<(Option<ObjectId>, &'a Stream)> {
    let xobjects = resources?.get(b"XObject").ok()?;
    let (_, xobjects) = doc.dereference(xobjects).ok()?;
    let entry = xobjects.as_dict().ok()?.get(name).ok()?;
    let id = match entry {
        Object::Reference(id) => Some(*id),
        _ => None,
    };
    let (_, resolved) = doc.dereference(entry).ok()?;
    let stream = resolved.as_stream().ok()?;
    if xobject_subtype(doc, &stream.dict) != Some(b"Image") {
        return None;
    }
    Some((id, stream))
}

/// Was nach dem Anwenden der Nicht-Bild-Filter übrigbleibt.
enum Payload {
    /// Rohe Abtastwerte.
    Samples(Vec<u8>),
    /// JPEG (`DCTDecode`).
    Jpeg(Vec<u8>),
    /// JPEG 2000 (`JPXDecode`) — dafür gibt es keinen brauchbaren Rust-Decoder.
    Jpx,
    /// Filter, den wir nicht können (`LZWDecode`, `CCITTFaxDecode`, …).
    Unsupported(String),
}

/// Dekodiert ein Bild in einen RGBA8-Puffer.
///
/// Schlägt irgendetwas fehl, entsteht eine einfarbige Platzhalterfläche — eine
/// leere Seite wäre genau der Fehler, den wir beheben wollen. Der zweite
/// Rückgabewert beschreibt in dem Fall, was schiefging.
///
/// Öffentlich, weil [`crate::image`] genau **ein** Bild auspacken muss und
/// nicht die ganze Seite: der Grund, aus dem es nicht ging, gehört wörtlich in
/// die Fehlermeldung — „zu groß (8000x8000)“ ist etwas anderes als der Filter.
pub fn decode_image(
    doc: &Document,
    resources: Option<&Dictionary>,
    dict: &Dictionary,
    raw: &[u8],
    fill: Rgb,
) -> (RasterImage, Option<String>) {
    decode_image_inner(doc, resources, dict, raw, fill, true)
}

/// `masks` ist beim Dekodieren **einer Maske** falsch.
///
/// Eine Maske hat nach PDF 32000-1 (8.9.5.4, 11.6.5.3) selbst keine Maske. Ohne
/// diesen Riegel führen zwei Bilder, die sich gegenseitig als `/SMask` nennen,
/// in eine endlose Rekursion — der Prozess stirbt am Stapelüberlauf, und zwar
/// schon beim bloßen Anzeigen der Seite.
fn decode_image_inner(
    doc: &Document,
    resources: Option<&Dictionary>,
    dict: &Dictionary,
    raw: &[u8],
    fill: Rgb,
    masks: bool,
) -> (RasterImage, Option<String>) {
    let width = dict_int(doc, dict, b"Width", b"W").unwrap_or(0).max(0) as u32;
    let height = dict_int(doc, dict, b"Height", b"H").unwrap_or(0).max(0) as u32;
    if width == 0 || height == 0 {
        return (
            RasterImage::solid([220, 220, 220, 255], true),
            Some("Größe fehlt".into()),
        );
    }
    if u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
        return (
            RasterImage::solid([220, 220, 220, 255], true),
            Some(format!("zu groß ({width}x{height})")),
        );
    }

    let payload = apply_filters(doc, dict, raw);
    let mask = dict_bool(doc, dict, b"ImageMask", b"IM");
    let bpc = if mask {
        1
    } else {
        dict_int(doc, dict, b"BitsPerComponent", b"BPC")
            .unwrap_or(8)
            .max(1) as usize
    };
    let decode: Option<Vec<f64>> = dict
        .get(b"Decode")
        .or_else(|_| dict.get(b"D"))
        .ok()
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| {
            o.as_array()
                .ok()
                .map(|a| a.iter().filter_map(as_f64).collect())
        });

    let mut image = match payload {
        Payload::Samples(samples) => {
            if mask {
                match stencil_to_rgba(width, height, &samples, decode.as_deref(), fill) {
                    Some(rgba) => RasterImage {
                        width,
                        height,
                        rgba,
                        placeholder: false,
                    },
                    None => {
                        return (
                            RasterImage::solid([220, 220, 220, 255], true),
                            Some("Maskendaten unvollständig".into()),
                        )
                    }
                }
            } else {
                let space = dict
                    .get(b"ColorSpace")
                    .or_else(|_| dict.get(b"CS"))
                    .ok()
                    .map(|o| ColorSpace::resolve(doc, resources, o))
                    .unwrap_or(ColorSpace::Gray);
                match samples_to_rgba(width, height, bpc, &space, decode.as_deref(), &samples) {
                    Some(mut rgba) => {
                        // Farbschlüssel-`/Mask` vergleicht **Abtastwerte**, nicht
                        // fertige Farben (PDF 32000-1, 8.9.6.4). Sie muss hier
                        // eingerechnet werden, solange `samples` noch existiert:
                        // nach dem Farbraum ist der Vergleich nicht mehr möglich
                        // (`Indexed` liefert Palettenfarben, CMYK gerechnetes
                        // RGB), und nach dem Schwärzen erst recht nicht.
                        if let Some(ranges) = masks.then(|| color_key_ranges(doc, dict)).flatten() {
                            apply_color_key(
                                &mut rgba,
                                width,
                                height,
                                bpc,
                                space.components(),
                                &samples,
                                &ranges,
                            );
                        }
                        RasterImage {
                            width,
                            height,
                            rgba,
                            placeholder: false,
                        }
                    }
                    None => {
                        return (
                            RasterImage::solid([220, 220, 220, 255], true),
                            Some("Abtastwerte unvollständig".into()),
                        )
                    }
                }
            }
        }
        Payload::Jpeg(data) => match decode_jpeg(&data, decode.as_deref()) {
            Some(image) => image,
            None => {
                return (
                    RasterImage::solid([220, 220, 220, 255], true),
                    Some("JPEG nicht dekodierbar".into()),
                )
            }
        },
        Payload::Jpx => {
            return (
                RasterImage::solid([128, 128, 128, 255], true),
                Some("JPXDecode (JPEG 2000) wird nicht dekodiert".into()),
            )
        }
        Payload::Unsupported(filter) => {
            return (
                RasterImage::solid([220, 220, 220, 255], true),
                Some(format!("Filter {filter} wird nicht unterstützt")),
            )
        }
    };

    if masks {
        if let Some(reason) = apply_soft_mask(doc, resources, dict, &mut image) {
            return (RasterImage::solid([220, 220, 220, 255], true), Some(reason));
        }
    }
    (image, None)
}

/// Wendet alle Filter an, die keine Bildkompression sind.
fn apply_filters(doc: &Document, dict: &Dictionary, raw: &[u8]) -> Payload {
    let filters = filter_names(doc, dict);
    if filters.is_empty() {
        return Payload::Samples(raw.to_vec());
    }
    let mut data = raw.to_vec();
    for (index, filter) in filters.iter().enumerate() {
        let params = filter_params(doc, dict, index);
        data = match filter.as_str() {
            "FlateDecode" | "Fl" => match inflate(&data) {
                Some(out) => apply_predictor(doc, out, params.as_ref()),
                None => return Payload::Unsupported("FlateDecode".into()),
            },
            "ASCII85Decode" | "A85" => decode_ascii85(&data),
            "ASCIIHexDecode" | "AHx" => decode_ascii_hex(&data),
            "RunLengthDecode" | "RL" => decode_run_length(&data),
            "DCTDecode" | "DCT" => return Payload::Jpeg(data),
            "JPXDecode" => return Payload::Jpx,
            other => return Payload::Unsupported(other.to_string()),
        };
    }
    Payload::Samples(data)
}

fn filter_names(doc: &Document, dict: &Dictionary) -> Vec<String> {
    let Some(filter) = deref(doc, dict.get(b"Filter").or_else(|_| dict.get(b"F")).ok()) else {
        return Vec::new();
    };
    match filter {
        Object::Name(name) => vec![String::from_utf8_lossy(name).into_owned()],
        Object::Array(items) => items
            .iter()
            .filter_map(|o| deref(doc, Some(o)))
            .filter_map(|o| o.as_name().ok())
            .map(|n| String::from_utf8_lossy(n).into_owned())
            .collect(),
        _ => Vec::new(),
    }
}

fn filter_params(doc: &Document, dict: &Dictionary, index: usize) -> Option<Dictionary> {
    let params = deref(
        doc,
        dict.get(b"DecodeParms").or_else(|_| dict.get(b"DP")).ok(),
    )?;
    match params {
        Object::Dictionary(d) if index == 0 => Some(d.clone()),
        Object::Array(items) => items
            .get(index)
            .and_then(|o| deref(doc, Some(o)))
            .and_then(|o| o.as_dict().ok())
            .cloned(),
        _ => None,
    }
}

/// PNG-Prädiktoren (`/Predictor >= 10`); der TIFF-Prädiktor 2 wird übergangen.
fn apply_predictor(doc: &Document, data: Vec<u8>, params: Option<&Dictionary>) -> Vec<u8> {
    let Some(params) = params else {
        return data;
    };
    let predictor = dict_int(doc, params, b"Predictor", b"Predictor").unwrap_or(1);
    if !(10..=15).contains(&predictor) {
        return data;
    }
    let columns = dict_int(doc, params, b"Columns", b"Columns")
        .unwrap_or(1)
        .max(1) as usize;
    let colors = dict_int(doc, params, b"Colors", b"Colors")
        .unwrap_or(1)
        .max(1) as usize;
    let bits = dict_int(doc, params, b"BitsPerComponent", b"BPC")
        .unwrap_or(8)
        .max(8) as usize;
    let bytes_per_pixel = (colors * bits / 8).max(1);
    lopdf::filters::png::decode_frame(&data, bytes_per_pixel, columns).unwrap_or(data)
}

fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    use flate2::read::{DeflateDecoder, ZlibDecoder};
    use std::io::Read;

    // Abgeschnittene Streams sind in freier Wildbahn häufig — was schon
    // dekodiert ist, wird behalten.
    let mut out = Vec::new();
    let _ = ZlibDecoder::new(data).read_to_end(&mut out);
    if !out.is_empty() {
        return Some(out);
    }
    let mut raw = Vec::new();
    let _ = DeflateDecoder::new(data).read_to_end(&mut raw);
    (!raw.is_empty()).then_some(raw)
}

fn decode_ascii_hex(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 2);
    let mut high: Option<u8> = None;
    for &byte in data {
        if byte == b'>' {
            break;
        }
        let Some(value) = (byte as char).to_digit(16) else {
            continue;
        };
        match high.take() {
            Some(h) => out.push((h << 4) | value as u8),
            None => high = Some(value as u8),
        }
    }
    if let Some(h) = high {
        out.push(h << 4);
    }
    out
}

fn decode_ascii85(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut group = [0u8; 5];
    let mut count = 0;
    let mut i = 0;
    // Ein führendes `<~` ist erlaubt.
    if data.starts_with(b"<~") {
        i = 2;
    }
    while i < data.len() {
        let byte = data[i];
        i += 1;
        if byte.is_ascii_whitespace() {
            continue;
        }
        if byte == b'~' {
            break;
        }
        if byte == b'z' && count == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        if !(b'!'..=b'u').contains(&byte) {
            break;
        }
        group[count] = byte - b'!';
        count += 1;
        if count == 5 {
            let value = group.iter().fold(0u32, |acc, d| {
                acc.wrapping_mul(85).wrapping_add(u32::from(*d))
            });
            out.extend_from_slice(&value.to_be_bytes());
            count = 0;
        }
    }
    if count > 0 {
        for slot in group.iter_mut().skip(count) {
            *slot = 84;
        }
        let value = group.iter().fold(0u32, |acc, d| {
            acc.wrapping_mul(85).wrapping_add(u32::from(*d))
        });
        out.extend_from_slice(&value.to_be_bytes()[..count - 1]);
    }
    out
}

fn decode_run_length(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let length = data[i];
        i += 1;
        match length {
            128 => break,
            0..=127 => {
                let n = length as usize + 1;
                let end = (i + n).min(data.len());
                out.extend_from_slice(&data[i..end]);
                i = end;
            }
            _ => {
                if i < data.len() {
                    let n = 257 - length as usize;
                    out.extend(std::iter::repeat_n(data[i], n));
                    i += 1;
                }
            }
        }
    }
    out
}

/// `/ImageMask`: 1 Bit je Punkt, gemalt wird in der aktuellen Füllfarbe.
fn stencil_to_rgba(
    width: u32,
    height: u32,
    samples: &[u8],
    decode: Option<&[f64]>,
    fill: Rgb,
) -> Option<Vec<u8>> {
    let stride = (width as usize).div_ceil(8);
    if samples.len() < stride * height as usize {
        return None;
    }
    // Standard `/Decode [0 1]`: die Null malt.
    let paint_on = decode.and_then(|d| d.first().copied()).unwrap_or(0.0) < 0.5;
    let [r, g, b] = fill.to_u8();
    let mut out = vec![0u8; width as usize * height as usize * 4];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let bit = (samples[y * stride + x / 8] >> (7 - (x % 8))) & 1;
            let paint = (bit == 0) == paint_on;
            let offset = (y * width as usize + x) * 4;
            if paint {
                out[offset] = r;
                out[offset + 1] = g;
                out[offset + 2] = b;
                out[offset + 3] = 255;
            }
        }
    }
    Some(out)
}

/// Rohe Abtastwerte → RGBA8.
fn samples_to_rgba(
    width: u32,
    height: u32,
    bpc: usize,
    space: &ColorSpace,
    decode: Option<&[f64]>,
    samples: &[u8],
) -> Option<Vec<u8>> {
    let comps = space.components();
    if comps == 0 || !matches!(bpc, 1 | 2 | 4 | 8 | 16) {
        return None;
    }
    let stride = (width as usize * comps * bpc).div_ceil(8);
    if samples.len() < stride * height as usize {
        return None;
    }
    let max = ((1u32 << bpc.min(16)) - 1) as f64;
    let indexed = matches!(space, ColorSpace::Indexed { .. });
    let lab = matches!(space, ColorSpace::Lab);

    let mut out = vec![0u8; width as usize * height as usize * 4];
    let mut values = vec![0.0f64; comps];
    for y in 0..height as usize {
        let row = &samples[y * stride..];
        for x in 0..width as usize {
            for (c, slot) in values.iter_mut().enumerate() {
                let raw = read_sample(row, (x * comps + c) * bpc, bpc) as f64;
                *slot = match decode.and_then(|d| Some((*d.get(2 * c)?, *d.get(2 * c + 1)?))) {
                    // Bei `Indexed` bildet `/Decode` standardmaessig auf den
                    // Indexbereich ab, die Formel stimmt also fuer beide Faelle.
                    Some((dmin, dmax)) => dmin + raw * (dmax - dmin) / max,
                    None if indexed => raw,
                    None if lab && c == 0 => raw / max * 100.0,
                    None => raw / max,
                };
            }
            let rgb = space.to_rgb(&values).to_u8();
            let offset = (y * width as usize + x) * 4;
            out[offset..offset + 3].copy_from_slice(&rgb);
            out[offset + 3] = 255;
        }
    }
    Some(out)
}

/// Liest einen Abtastwert beliebiger Bittiefe aus einer Zeile.
fn read_sample(row: &[u8], bit_offset: usize, bpc: usize) -> u32 {
    match bpc {
        8 => row.get(bit_offset / 8).copied().unwrap_or(0) as u32,
        16 => {
            let i = bit_offset / 8;
            ((row.get(i).copied().unwrap_or(0) as u32) << 8)
                | row.get(i + 1).copied().unwrap_or(0) as u32
        }
        _ => {
            let byte = row.get(bit_offset / 8).copied().unwrap_or(0) as u32;
            let shift = 8 - bpc - (bit_offset % 8);
            (byte >> shift) & ((1 << bpc) - 1)
        }
    }
}

/// JPEG über `zune-jpeg`. CMYK-JPEGs werden als Adobe-invertiert behandelt.
fn decode_jpeg(data: &[u8], decode: Option<&[f64]>) -> Option<RasterImage> {
    use zune_jpeg::zune_core::bytestream::ZCursor;
    use zune_jpeg::JpegDecoder;

    let mut decoder = JpegDecoder::new(ZCursor::new(data));
    let pixels = decoder.decode().ok()?;
    let info = decoder.info()?;
    let (width, height) = (info.width as u32, info.height as u32);
    if width == 0 || height == 0 {
        return None;
    }
    let count = width as usize * height as usize;
    let comps = pixels.len() / count.max(1);
    // `/Decode [1 0 …]` dreht die Werte um.
    let inverted = decode
        .and_then(|d| d.first().copied())
        .map(|v| v > 0.5)
        .unwrap_or(false);

    let mut rgba = vec![255u8; count * 4];
    for i in 0..count {
        let rgb = match comps {
            1 => {
                let v = pixels[i];
                let v = if inverted { 255 - v } else { v };
                [v, v, v]
            }
            3 => {
                let p = &pixels[i * 3..i * 3 + 3];
                if inverted {
                    [255 - p[0], 255 - p[1], 255 - p[2]]
                } else {
                    [p[0], p[1], p[2]]
                }
            }
            4 => {
                let p = &pixels[i * 4..i * 4 + 4];
                // Adobe speichert CMYK invertiert.
                let f = |v: u8| {
                    let x = v as f64 / 255.0;
                    if inverted {
                        x
                    } else {
                        1.0 - x
                    }
                };
                crate::content::cmyk_to_rgb(f(p[0]), f(p[1]), f(p[2]), f(p[3])).to_u8()
            }
            _ => return None,
        };
        rgba[i * 4..i * 4 + 3].copy_from_slice(&rgb);
    }
    Some(RasterImage {
        width,
        height,
        rgba,
        placeholder: false,
    })
}

/// Welche Maske eines Bildes ein Betrachter **wirklich befolgt**.
///
/// Das ist die eine Stelle, an der diese Frage entschieden wird. Sie hat zwei
/// Abnehmer, die sich nicht widersprechen dürfen: [`apply_soft_mask`] rechnet
/// die hier benannte Maske in den Alphakanal, und [`mask_plan`] entscheidet
/// daraus, was in die Ausgabe geschrieben wird. Liefen die beiden auseinander,
/// behielte das Bild eine Maske, an die sich niemand hält, und verlöre die, an
/// die sich alle halten — genau der Weg, auf dem eine Bildmaske schon einmal
/// wieder aufgedeckt hat, was verborgen war.
///
/// `/SMask` und `/Mask` nebeneinander sind nach PDF 32000-1 (Tabelle 89)
/// regelwidrig. Erzeuger schreiben es trotzdem, die Betrachter rendern es
/// anstandslos — und halten sich dabei an `/SMask`. Also gilt hier dasselbe.
enum Honoured<'a> {
    /// Weder `/SMask` noch `/Mask`.
    None,
    /// `/SMask`-Strom: Graustufen-Alpha. Hat Vorrang vor jedem `/Mask`.
    Soft(&'a Stream),
    /// `/Mask`-Strom: Stencil („die Null malt“).
    Stencil(&'a Stream),
    /// `/Mask` ist da, aber kein Strom — ein Farbschlüssel-Array oder etwas,
    /// das keine Maske sein kann. Wie damit umzugehen ist, entscheidet
    /// [`mask_plan`] am Eintrag selbst.
    OtherMask(&'a Object),
}

/// Siehe [`Honoured`].
fn honoured_mask<'a>(doc: &'a Document, dict: &'a Dictionary) -> Honoured<'a> {
    if let Some(soft) = deref(doc, dict.get(b"SMask").ok()).and_then(|o| o.as_stream().ok()) {
        return Honoured::Soft(soft);
    }
    let Ok(entry) = dict.get(b"Mask") else {
        return Honoured::None;
    };
    match deref(doc, Some(entry)).and_then(|o| o.as_stream().ok()) {
        Some(stencil) => Honoured::Stencil(stencil),
        None => Honoured::OtherMask(entry),
    }
}

/// `/SMask` (Graustufen-Alpha) bzw. `/Mask` (Stencil) auf das Bild anwenden.
///
/// Welche der beiden gilt, sagt [`honoured_mask`] — und nur die.
///
/// Der Rückgabewert nennt den Grund, wenn das Bild **nicht** so dargestellt
/// werden kann, wie die Datei es meint. Er entsteht nur für `/SMask`: dessen
/// Alphaebene wird beim Schwärzen neu aufgebaut ([`crate::image`]), und eine
/// Alphaebene, die wir nicht lesen konnten, würde dabei stillschweigend
/// entfallen — versteckte Bildpunkte stünden in der Ausgabe sichtbar da. Ein
/// `/Mask` dagegen wird beim Neukodieren unverändert übernommen; ob wir es
/// lesen konnten, ändert an der Ausgabe nichts.
fn apply_soft_mask(
    doc: &Document,
    resources: Option<&Dictionary>,
    dict: &Dictionary,
    image: &mut RasterImage,
) -> Option<String> {
    let (stream, is_stencil) = match honoured_mask(doc, dict) {
        Honoured::Soft(stream) => (stream, false),
        Honoured::Stencil(stream) => (stream, true),
        // Ein Farbschlüssel-Array steckt bereits im Alphakanal (siehe
        // `apply_color_key`) und ist hier kein Strom.
        Honoured::None | Honoured::OtherMask(_) => return None,
    };
    let (mask, note) = decode_image_inner(
        doc,
        resources,
        &stream.dict,
        &stream.content,
        Rgb::BLACK,
        false,
    );
    if mask.placeholder || mask.width == 0 || mask.height == 0 {
        if is_stencil {
            return None;
        }
        return Some(format!(
            "die Alphaebene /SMask ließ sich nicht dekodieren ({})",
            note.unwrap_or_else(|| "Grund unbekannt".into())
        ));
    }
    for y in 0..image.height as usize {
        // Nächster Nachbar — Maske und Bild dürfen unterschiedlich groß sein.
        let my = y * mask.height as usize / image.height as usize;
        for x in 0..image.width as usize {
            let mx = x * mask.width as usize / image.width as usize;
            let m = &mask.rgba[(my * mask.width as usize + mx) * 4..];
            // Bei `/Mask` (Stencil, PDF 32000-1 8.9.6.4) wird der Bildpunkt
            // gemalt, wo der Abtastwert der Maske 0 ist — und genau dort steht
            // in `mask.rgba` das Alpha 255, weil `stencil_to_rgba` dieselbe
            // Regel („die Null malt“, `/Decode` eingerechnet) anwendet. Die
            // Maske ist also **nicht** umzudrehen: was die Maske malt, ist das,
            // was vom Bild zu sehen ist.
            let alpha = if is_stencil { m[3] } else { m[0] };
            image.rgba[(y * image.width as usize + x) * 4 + 3] = alpha;
        }
    }
    None
}

/// Die Bereiche einer Farbschlüssel-`/Mask`, falls das Bild eine hat.
///
/// `/Mask` ist entweder ein Strom (Stencil) oder ein Array aus 2×n ganzen
/// Zahlen (Farbschlüssel). Nur das Array kommt hier zurück.
fn color_key_ranges(doc: &Document, dict: &Dictionary) -> Option<Vec<i64>> {
    let items = deref(doc, dict.get(b"Mask").ok())?.as_array().ok()?;
    let ranges: Vec<i64> = items
        .iter()
        .filter_map(|o| deref(doc, Some(o)))
        .filter_map(|o| match o {
            Object::Integer(i) => Some(*i),
            Object::Real(r) => Some(*r as i64),
            _ => None,
        })
        .collect();
    (ranges.len() == items.len() && !ranges.is_empty()).then_some(ranges)
}

/// Farbschlüssel-Maskierung (PDF 32000-1, 8.9.6.4).
///
/// Durchsichtig ist ein Bildpunkt, dessen **Abtastwerte** in *allen*
/// Komponenten in den angegebenen Bereich fallen. Verglichen wird vor jeder
/// Farbumrechnung und vor `/Decode`; deshalb steht das hier bei den rohen
/// Abtastwerten und nicht bei den fertigen Farben.
fn apply_color_key(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    bpc: usize,
    comps: usize,
    samples: &[u8],
    ranges: &[i64],
) {
    if comps == 0 || ranges.len() < comps * 2 {
        return;
    }
    let stride = (width as usize * comps * bpc).div_ceil(8);
    for y in 0..height as usize {
        let Some(row) = samples.get(y * stride..) else {
            return;
        };
        for x in 0..width as usize {
            let hidden = (0..comps).all(|c| {
                let value = i64::from(read_sample(row, (x * comps + c) * bpc, bpc));
                value >= ranges[2 * c] && value <= ranges[2 * c + 1]
            });
            if hidden {
                rgba[(y * width as usize + x) * 4 + 3] = 0;
            }
        }
    }
}

/// Was mit der `/Mask` eines Bildes beim **Neukodieren** geschehen muss.
///
/// [`crate::image`] baut das Bild-Dictionary neu auf. Was es dabei nicht
/// ausdrücklich übernimmt, ist in der Ausgabe weg — und eine verlorene Maske
/// macht genau die Bildpunkte sichtbar, die die Eingabe versteckt.
#[derive(Debug, Clone, PartialEq)]
pub enum MaskPlan {
    /// Kein `/Mask` (ein `/SMask` läuft über den Alphakanal).
    None,
    /// `/Mask` verweist auf einen Stencil-Strom: **unverändert übernehmen**,
    /// solange kein Bildpunkt gefallen ist.
    ///
    /// Der Strom steht neben dem Bild und beschreibt es im Einheitsquadrat,
    /// nicht im Pixelraster — er überlebt das Neukodieren des Bildes
    /// unbeschadet und in voller Auflösung. Ihn in eine Alphaebene des Bildes
    /// umzurechnen, hieße ihn auf dessen Auflösung herunterzubrechen. Fällt
    /// aber ein Bildpunkt, ist die Maske selbst Bildinhalt (ihre Bits sind
    /// die Form, die gemalt wird), und `crate::image` schreibt statt ihrer
    /// die unter der Zone geschwärzte Alphaebene als `/SMask` (Register #77).
    Keep(Object),
    /// Die Maske steckt bereits im Alphakanal; ein `/Mask` darf **nicht**
    /// übernommen werden.
    ///
    /// Zwei Fälle führen hierher:
    ///
    /// * Ein Farbschlüssel-Array. Es benennt Abtastwerte, keine Bildstellen.
    ///   Nach dem Schwärzen stehen an den geschwärzten Stellen andere Werte,
    ///   und der Farbraum kann sich beim Neukodieren ohnehin ändern: derselbe
    ///   Schlüssel träfe in der Ausgabe andere Bildpunkte — im schlimmsten
    ///   Fall die geschwärzten, die damit wieder durchsichtig würden.
    /// * Ein `/Mask` **neben** einem `/SMask`. Befolgt wird dann das `/SMask`
    ///   ([`Honoured`]), und das steht im Alphakanal. Das `/Mask` mitzuschreiben
    ///   hieße, dem Bild in der Ausgabe die einzige Maske zu geben, an die sich
    ///   kein Betrachter gehalten hat — und ihm die zu nehmen, an die sich alle
    ///   halten.
    InAlpha,
    /// Farbschlüssel auf **verlustbehaftet** kodierten Daten (`/DCTDecode`,
    /// `/JPXDecode`).
    ///
    /// Ob der Schlüssel überhaupt einen Bildpunkt trifft, ist dem Dictionary
    /// nicht anzusehen; das entscheidet [`color_key_reach`] am *dekodierten*
    /// Bild — und zwar dort, wo das Bild ohnehin ausgepackt wird und die
    /// Bildgrenze (`--max-image-mb`) bereits greift. Der Text nennt den
    /// Filter, damit die Meldung ihn nennen kann.
    ProbeColorKey(String),
    /// Diese Maske lässt sich nicht ohne stille Näherung übernehmen.
    Unsupported(String),
}

/// Siehe [`MaskPlan`].
pub fn mask_plan(doc: &Document, resources: Option<&Dictionary>, dict: &Dictionary) -> MaskPlan {
    let is_stencil_image = dict_bool(doc, dict, b"ImageMask", b"IM");
    match honoured_mask(doc, dict) {
        Honoured::None => MaskPlan::None,
        // Der Alphakanal trägt das `/SMask` — und der wird beim Neukodieren zu
        // einem frischen `/SMask`. Ein `/Mask` daneben ist regelwidrig und wird
        // von keinem Betrachter befolgt; mitgeschrieben verdrängte es das
        // `/SMask` und deckte damit genau die Bildpunkte auf, die die Eingabe
        // verbirgt.
        Honoured::Soft(_) if dict.get(b"Mask").is_ok() => MaskPlan::InAlpha,
        Honoured::Soft(_) => MaskPlan::None,
        // Ein Bild, das selbst eine Stencil-Maske ist, wird als Bitmuster
        // neu geschrieben; die Maske steckt dann im Alphakanal und damit in
        // den Bits. Ein `/Mask` daneben wäre doppelt gemoppelt.
        Honoured::Stencil(_) if is_stencil_image => MaskPlan::InAlpha,
        // Übernommen wird der **Verweis**. Ein Strom, der direkt im
        // Bild-Dictionary stünde, wäre keiner: Ströme sind eigene Objekte
        // (PDF 32000-1, 7.3.8), und mitgeschrieben ergäbe er eine Datei, die
        // kein Betrachter mehr liest.
        Honoured::Stencil(_) => match dict.get(b"Mask") {
            Ok(entry @ Object::Reference(_)) => MaskPlan::Keep(entry.clone()),
            _ => MaskPlan::Unsupported(
                "hat einen /Mask-Strom, der kein eigenes Objekt ist; er ließe sich nicht \
                 mitschreiben"
                    .into(),
            ),
        },
        Honoured::OtherMask(entry) => match deref(doc, Some(entry)) {
            Some(Object::Array(items)) => color_key_plan(doc, resources, dict, items),
            // Ein Verweis, der ins Leere zeigt: die Datei *wollte* eine Maske,
            // und ein Werkzeug, das eine beschädigte Querverweistabelle anders
            // repariert, findet sie vielleicht. Was sie versteckt, ist hier
            // nicht zu ermitteln.
            None => MaskPlan::Unsupported(
                "hat ein /Mask, dessen Verweis sich nicht auflösen lässt. Ob es Bildpunkte \
                 versteckt, ist damit nicht zu bestimmen"
                    .into(),
            ),
            // Alles andere kann in keinem regelkonformen Betrachter etwas
            // verstecken (`/Mask` ist Strom oder Array, PDF 32000-1 Tabelle 89;
            // `null` gilt nach 7.3.9 als nicht vorhanden). Es fällt deshalb weg,
            // ohne dass dabei etwas sichtbar werden könnte — und der Lauf an
            // einer Datei abzubrechen, die überall sonst unauffällig aussieht,
            // wäre der schlechtere Handel.
            Some(_) => MaskPlan::None,
        },
    }
}

/// Der Teil von [`mask_plan`], der für ein Farbschlüssel-Array zuständig ist.
fn color_key_plan(
    doc: &Document,
    resources: Option<&Dictionary>,
    dict: &Dictionary,
    items: &[Object],
) -> MaskPlan {
    if dict_bool(doc, dict, b"ImageMask", b"IM") {
        return MaskPlan::Unsupported(
            "hat eine Farbschlüssel-Maske, ist aber selbst eine Stencil-Maske (/ImageMask) und hat \
             gar keine Farbwerte"
                .into(),
        );
    }
    let comps = color_key_components(doc, resources, dict);
    if items.len() != comps * 2 {
        return MaskPlan::Unsupported(format!(
            "hat eine Farbschlüssel-Maske mit {} Einträgen, der Farbraum hat aber {comps} \
             Komponente(n) (erwartet: {}). Welche Bildpunkte die Datei versteckt, ist damit nicht \
             zu bestimmen",
            items.len(),
            comps * 2
        ));
    }
    if color_key_ranges(doc, dict).is_none() {
        return MaskPlan::Unsupported(
            "hat eine Farbschlüssel-Maske, deren Einträge keine ganzen Zahlen sind".into(),
        );
    }
    // Verlustbehaftet kodiert: der Schlüssel vergleicht Abtastwerte, und welche
    // ein JPEG-Decoder liefert, ist von Decoder zu Decoder um ein paar Stufen
    // verschieden. Ob das überhaupt eine Rolle spielt, hängt daran, ob
    // irgendein Wert in die Nähe des Schlüssels kommt — das ist am Dictionary
    // nicht zu sehen und wird deshalb am dekodierten Bild nachgesehen.
    let filters = filter_names(doc, dict);
    if let Some(lossy) = filters
        .iter()
        .find(|f| matches!(f.as_str(), "DCTDecode" | "DCT" | "JPXDecode"))
    {
        return MaskPlan::ProbeColorKey(lossy.clone());
    }
    MaskPlan::InAlpha
}

/// Zahl der Farbkomponenten, gegen die ein Farbschlüssel zu zählen ist.
fn color_key_components(
    doc: &Document,
    resources: Option<&Dictionary>,
    dict: &Dictionary,
) -> usize {
    dict.get(b"ColorSpace")
        .or_else(|_| dict.get(b"CS"))
        .ok()
        .map(|o| crate::content::ColorSpace::resolve(doc, resources, o))
        .unwrap_or(crate::content::ColorSpace::Gray)
        .components()
}

/// Wie weit ein Abtastwert neben dem Schlüsselbereich liegen darf und trotzdem
/// als „womöglich getroffen“ gilt.
///
/// Ein JPEG-Decoder liefert nicht dieselben Werte, die der Erzeuger kodiert
/// hat; zwischen zwei Decodern liegen ein paar Stufen, an harten Kanten auch
/// mehr. Das Band ist bewusst großzügig: es entscheidet nur darüber, ob wir
/// den Fall für harmlos erklären, und „harmlos“ soll die Ausnahme sein.
const COLOR_KEY_BAND: i64 = 24;

/// Ob ein Farbschlüssel im **dekodierten** Bild überhaupt etwas verbirgt.
///
/// Siehe [`MaskPlan::ProbeColorKey`]. Gearbeitet wird auf dem fertigen
/// RGBA-Puffer: dort liegen bei einem JPEG genau die Abtastwerte, die der
/// Decoder ausgegeben hat (ein- und dreikomponentig unverändert, `/Decode
/// [1 0 …]` eingerechnet). Vier Komponenten (CMYK) sind nach der
/// Farbumrechnung nicht mehr zurückzurechnen, ebenso wenig ein Bild, das gar
/// nicht dekodiert werden konnte — dort bleibt es beim ehrlichen „weiß ich
/// nicht“.
#[derive(Debug, Clone, PartialEq)]
pub enum ColorKeyReach {
    /// Kein Abtastwert liegt im Schlüsselbereich (samt Band): die Maske
    /// verbirgt nichts und darf entfallen.
    NothingHidden,
    /// So viele Bildpunkte liegen im Band.
    Hides(u64),
    /// Am dekodierten Bild nicht nachzuprüfen.
    Undecidable(String),
}

/// Siehe [`ColorKeyReach`].
pub fn color_key_reach(
    doc: &Document,
    resources: Option<&Dictionary>,
    dict: &Dictionary,
    image: &RasterImage,
) -> ColorKeyReach {
    let Some(ranges) = color_key_ranges(doc, dict) else {
        return ColorKeyReach::Undecidable("die Bereiche sind keine ganzen Zahlen".into());
    };
    if image.placeholder {
        return ColorKeyReach::Undecidable("das Bild ließ sich nicht dekodieren".into());
    }
    let comps = color_key_components(doc, resources, dict);
    if ranges.len() != comps * 2 || !matches!(comps, 1 | 3) {
        return ColorKeyReach::Undecidable(format!(
            "der Farbraum hat {comps} Komponente(n); aus den fertigen Farben sind die \
             Abtastwerte dann nicht zurückzurechnen"
        ));
    }
    let bpc = dict_int(doc, dict, b"BitsPerComponent", b"BPC").unwrap_or(8);
    if bpc != 8 {
        return ColorKeyReach::Undecidable(format!(
            "die Abtastwerte haben {bpc} Bit; der dekodierte Puffer hat 8"
        ));
    }
    // `/Decode [1 0 …]` dreht die Werte um — dieselbe Regel wie in
    // [`decode_jpeg`], sonst verglichen wir gegen die falschen Zahlen.
    let inverted = dict
        .get(b"Decode")
        .or_else(|_| dict.get(b"D"))
        .ok()
        .and_then(|o| deref(doc, Some(o)))
        .and_then(|o| o.as_array().ok())
        .and_then(|a| a.first().and_then(as_f64))
        .map(|v| v > 0.5)
        .unwrap_or(false);

    let mut hits = 0u64;
    for pixel in image.rgba.chunks_exact(4) {
        let hidden = (0..comps).all(|c| {
            let raw = i64::from(if comps == 1 { pixel[0] } else { pixel[c] });
            let raw = if inverted { 255 - raw } else { raw };
            raw >= ranges[2 * c] - COLOR_KEY_BAND && raw <= ranges[2 * c + 1] + COLOR_KEY_BAND
        });
        hits += u64::from(hidden);
    }
    if hits == 0 {
        ColorKeyReach::NothingHidden
    } else {
        ColorKeyReach::Hides(hits)
    }
}

// ---------------------------------------------------------------------------
// Content-Stream mit Inline-Bildern
// ---------------------------------------------------------------------------

/// Ergebnis von [`decode_content_checked`].
///
/// Wichtig ist das zweite Feld: lopdfs Content-Parser **bricht am ersten
/// Fehler ab und verwirft den Rest** — kommentarlos, mit `Ok`. Beim Lesen
/// heißt das „dort stand kein Text“, beim Neuschreiben „diesen Teil der Seite
/// gab es nie“; beides falsch, beides still. Wer diese Funktion benutzt, muss
/// sich dazu verhalten.
#[derive(Debug, Default)]
pub struct DecodedContent {
    pub operations: Vec<Operation>,
    /// Länge (in Bytes) jedes Teilstücks, das der Parser nur **teilweise**
    /// aufgebraucht hat. Der Rest dahinter fehlt in `operations`.
    pub truncated: Vec<usize>,
}

impl DecodedContent {
    /// Größe der Teilstücke, in denen etwas fehlt.
    pub fn affected_bytes(&self) -> usize {
        self.truncated.iter().sum()
    }
}

/// Dekodiert einen Content-Stream und fasst `BI … ID … EI` zu je einer
/// Operation `BI` mit Dictionary und Rohdaten zusammen.
///
/// lopdfs Parser kennt keine Inline-Bilder: die Binärdaten hinter `ID` bringen
/// ihn aus dem Tritt, der Rest des Streams geht verloren. Deshalb werden die
/// Blöcke vorher herausgeschnitten und die Teilstücke einzeln geparst.
///
/// **Verwirft stillschweigend, was sich nicht zerlegen lässt.** Für alles,
/// was danach eine Aussage über Vollständigkeit trifft — Suchen, Schwärzen,
/// Neuschreiben —, ist [`decode_content_checked`] die richtige Tür.
pub fn decode_content(data: &[u8]) -> Vec<Operation> {
    decode_content_checked(data).operations
}

/// Wie [`decode_content`], meldet aber die Abschnitte, die verloren gingen.
pub fn decode_content_checked(data: &[u8]) -> DecodedContent {
    let mut out = DecodedContent::default();
    let images = find_inline_images(data);
    if images.is_empty() {
        decode_chunk(data, &mut out);
        return out;
    }
    let mut pos = 0usize;
    for (start, end, dict, payload) in images {
        if start > pos {
            decode_chunk(&data[pos..start], &mut out);
        }
        out.operations.push(Operation::new(
            "BI",
            vec![
                Object::Dictionary(dict),
                Object::String(payload, StringFormat::Literal),
            ],
        ));
        pos = end;
    }
    if pos < data.len() {
        decode_chunk(&data[pos..], &mut out);
    }
    out
}

/// Zerlegt ein Teilstück zwischen zwei Inline-Bildern.
///
/// `Content::decode` liefert auch dann `Ok`, wenn es nach dem ersten
/// unverständlichen Byte aufgibt — der Rest ist dann einfach weg. Nur
/// `Content::decode_strict` sagt, ob das ganze Teilstück aufgebraucht wurde.
/// Deshalb hier zuerst streng, und erst wenn das scheitert, nachsichtig: die
/// Operationen vor dem Bruch sollen erhalten bleiben, aber der Bruch selbst
/// muss aktenkundig werden.
fn decode_chunk(chunk: &[u8], out: &mut DecodedContent) {
    if let Ok(content) = Content::decode_strict(chunk) {
        out.operations.extend(content.operations);
        return;
    }
    // Zweiter Versuch mit sauberem Abschluss: lopdf verlangt hinter einem
    // Kommentar ein Zeilenende und kennt weder NUL noch Seitenvorschub als
    // Leerraum, obwohl PDF 32000-1 (Tabelle 1) beide dazuzählt. Beides ist
    // kein Inhaltsverlust und darf keinen Fehlalarm auslösen.
    let mut tidied: Vec<u8> = chunk.to_vec();
    while tidied.last().is_some_and(|b| is_pdf_whitespace(*b)) {
        tidied.pop();
    }
    tidied.push(b'\n');
    if let Ok(content) = Content::decode_strict(&tidied) {
        out.operations.extend(content.operations);
        return;
    }
    // Jetzt ist wirklich etwas abgeschnitten. Was davor steht, wird gerettet.
    if let Ok(content) = Content::decode(chunk) {
        out.operations.extend(content.operations);
    }
    out.truncated.push(chunk.len());
}

/// Leerraum nach PDF 32000-1, Tabelle 1 — einschließlich NUL und
/// Seitenvorschub, die Rusts `is_ascii_whitespace` nicht bzw. anders sieht.
fn is_pdf_whitespace(byte: u8) -> bool {
    matches!(byte, 0x00 | 0x09 | 0x0a | 0x0c | 0x0d | 0x20)
}

type InlineImage = (usize, usize, Dictionary, Vec<u8>);

/// Findet alle Inline-Bilder: (Start von `BI`, Ende hinter `EI`, Dict, Daten).
fn find_inline_images(data: &[u8]) -> Vec<InlineImage> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < data.len() {
        match data[i] {
            b'%' => {
                while i < data.len() && data[i] != b'\n' && data[i] != b'\r' {
                    i += 1;
                }
            }
            b'(' => i = skip_literal_string(data, i),
            b'<' => {
                if data.get(i + 1) == Some(&b'<') {
                    i += 2;
                } else {
                    i += 1;
                    while i < data.len() && data[i] != b'>' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'B' if is_token(data, i, b"BI") => match parse_inline_image(data, i) {
                Some(image) => {
                    i = image.1;
                    out.push(image);
                }
                None => i += 2,
            },
            _ => i += 1,
        }
    }
    out
}

fn parse_inline_image(data: &[u8], start: usize) -> Option<InlineImage> {
    // Dictionary-Teil bis zum `ID`.
    let mut i = start + 2;
    let id_pos = loop {
        if i >= data.len() {
            return None;
        }
        match data[i] {
            b'(' => i = skip_literal_string(data, i),
            b'I' if is_token(data, i, b"ID") => break i,
            _ => i += 1,
        }
    };

    let mut source = data[start + 2..id_pos].to_vec();
    source.extend_from_slice(b" ID");
    let operands = Content::decode(&source)
        .ok()?
        .operations
        .last()
        .map(|op| op.operands.clone())?;
    let mut dict = Dictionary::new();
    let mut iter = operands.into_iter();
    while let (Some(Object::Name(key)), Some(value)) = (iter.next(), iter.next()) {
        dict.set(key, value);
    }

    // Nach `ID` folgt genau ein Trennzeichen, dann die Binärdaten.
    let mut begin = id_pos + 2;
    if data.get(begin).is_some_and(|b| b.is_ascii_whitespace()) {
        begin += 1;
    }
    // `/L` (bzw. `/Length`) gibt die Datenlänge an, wenn vorhanden.
    let declared = dict
        .get(b"L")
        .or_else(|_| dict.get(b"Length"))
        .ok()
        .and_then(|o| o.as_i64().ok())
        .filter(|l| *l >= 0)
        .map(|l| l as usize);

    // Wo die Nutzdaten enden, ist die einzige Frage, die hier zählt — an ihr
    // hängt der **ganze Rest des Stroms**. Zu früh: das Bild ist abgeschnitten
    // und dahinter steht Binärmüll, der beim Zerlegen samt Text verschwindet.
    // Zu spät: der folgende Text steckt in der Nutzlast, wird nie durchsucht
    // und wandert wortwörtlich in die Ausgabe zurück.
    //
    // Deshalb wird jede Längenangabe **geprüft**, statt ihr geglaubt zu
    // werden: an ihrem Ende muss das Token `EI` stehen. Die Reihenfolge:
    //
    // 1. `/L` bzw. `/Length` — verlässlich, *wenn* es stimmt.
    // 2. die aus `/W`, `/H`, `/BPC` und `/CS` **gerechnete** Länge. Ohne
    //    Filter ist sie exakt und schlägt jede Suche.
    // 3. erst zuletzt die Suche nach einem `EI`, hinter dem wirklich ein
    //    Operatorstrom weitergeht — binäre Nutzdaten enthalten `EI` zufällig.
    let ends_at = |len: usize| -> Option<usize> {
        let end = begin.checked_add(len)?;
        (end <= data.len() && ei_follows(data, end)).then_some(end)
    };
    let end_of_data = declared
        .and_then(ends_at)
        .or_else(|| unfiltered_payload_len(&dict).and_then(ends_at))
        .or_else(|| find_ei(data, begin))
        // Letzte Rückfallebene: eine Längenangabe, die sich nicht bestätigen
        // ließ, ist immer noch besser als gar keine Grenze — sonst wäre ein
        // Bild am Stromende ohne `EI` überhaupt kein Bild mehr.
        .or_else(|| {
            declared
                .and_then(|len| begin.checked_add(len))
                .filter(|end| *end <= data.len())
        })?;
    let payload = data.get(begin..end_of_data)?.to_vec();
    let after = find_ei_end(data, end_of_data).unwrap_or(data.len());
    Some((start, after, dict, payload))
}

/// Steht ab `pos` — nach beliebig viel Leerraum — das Token `EI`?
fn ei_follows(data: &[u8], pos: usize) -> bool {
    let mut i = pos;
    while i < data.len() && data[i].is_ascii_whitespace() {
        i += 1;
    }
    data.get(i) == Some(&b'E')
        && data.get(i + 1) == Some(&b'I')
        && data
            .get(i + 2)
            .map(|b| b.is_ascii_whitespace() || is_delimiter(*b))
            .unwrap_or(true)
}

/// Länge der Nutzdaten eines **ungefilterten** Inline-Bildes, aus seinem
/// Dictionary gerechnet.
///
/// Ohne Filter ist das keine Schätzung, sondern die Rechnung aus PDF 32000-1,
/// 8.9.5.1: je Zeile `ceil(Breite · Komponenten · Bits / 8)` Bytes, mal Höhe.
/// Sie ist der einzige Weg, der ein `EI` in den Binärdaten gar nicht erst
/// befragen muss.
///
/// `None`, sobald etwas unklar ist — ein Filter, ein Farbraum, der erst über
/// die Ressourcen aufzulösen wäre, eine fehlende Angabe. Dann entscheidet die
/// Suche, nicht eine geratene Zahl.
fn unfiltered_payload_len(dict: &Dictionary) -> Option<usize> {
    if dict.get(b"F").is_ok() || dict.get(b"Filter").is_ok() {
        return None;
    }
    let int = |long: &[u8], short: &[u8]| {
        dict.get(short)
            .or_else(|_| dict.get(long))
            .ok()
            .and_then(|o| o.as_i64().ok())
    };
    let width = int(b"Width", b"W")?;
    let height = int(b"Height", b"H")?;
    if width <= 0 || height <= 0 {
        return None;
    }
    let is_mask = dict
        .get(b"IM")
        .or_else(|_| dict.get(b"ImageMask"))
        .ok()
        .and_then(|o| o.as_bool().ok())
        .unwrap_or(false);
    let (components, bits) = if is_mask {
        // Eine Stencil-Maske hat genau ein Bit je Pixel.
        (1i64, 1i64)
    } else {
        let space = dict
            .get(b"CS")
            .or_else(|_| dict.get(b"ColorSpace"))
            .ok()
            .and_then(|o| o.as_name().ok())?;
        let components = match space {
            b"G" | b"DeviceGray" | b"CalGray" | b"I" | b"Indexed" => 1,
            b"RGB" | b"DeviceRGB" | b"CalRGB" | b"Lab" => 3,
            b"CMYK" | b"DeviceCMYK" => 4,
            // Ein benannter Farbraum aus den Ressourcen: die Komponentenzahl
            // steht hier nicht, also wird hier auch nicht gerechnet.
            _ => return None,
        };
        (components, int(b"BitsPerComponent", b"BPC").unwrap_or(8))
    };
    if !(1..=16).contains(&bits) {
        return None;
    }
    let row_bytes = width
        .checked_mul(components)?
        .checked_mul(bits)?
        .checked_add(7)?
        / 8;
    usize::try_from(row_bytes.checked_mul(height)?).ok()
}

/// Wie viele Token hinter einem `EI` geprüft werden, bevor es als echtes Ende
/// des Bildes gilt.
///
/// Acht sind genug: sobald ein Byte auftaucht, das in keinem Token vorkommen
/// kann, ist die Sache entschieden. Binärdaten schaffen selten mehr als zwei
/// oder drei Token, bevor sie sich verraten.
const EI_TAIL_TOKENS: usize = 8;

/// Wie weit hinter einem `EI` dafür höchstens gelesen wird.
///
/// Ein einzelnes Token darf nicht beliebig lang werden: eine nie geschlossene
/// Zeichenkette liefe sonst je Kandidat bis zum Stromende, und ein Strom voller
/// `EI`-Kandidaten würde quadratisch. Vier Kilobyte gültig aussehender Syntax
/// sind ohnehin Beweis genug.
const EI_TAIL_WINDOW: usize = 4096;

/// Sucht das `EI`, das den Bilddatenblock beendet.
///
/// Binäre Bilddaten können `EI` zufällig enthalten — mit Leerzeichen davor und
/// dahinter, also in genau der Form, die ein echtes Ende hat. Wer am ersten
/// Treffer stehen bleibt, schneidet das Bild ab und macht aus dem Rest des
/// Stroms Binärmüll; beim Neuschreiben ist der Text dahinter dann ersatzlos
/// weg. Ironischerweise hexkodiert [`crate::image`] beim *Schreiben* genau
/// deshalb — beim Lesen fehlte dieselbe Vorsicht.
///
/// Deshalb zählt nur ein `EI`, hinter dem etwas steht, das wirklich wie ein
/// Operatorstrom aussieht. Findet sich keines, bleibt der erste Treffer als
/// Rückfallebene: eine Grenze an der falschen Stelle ist immer noch besser
/// als gar kein Bild.
fn find_ei(data: &[u8], from: usize) -> Option<usize> {
    let mut fallback = None;
    let mut i = from;
    while i + 1 < data.len() {
        if data[i] == b'E'
            && data[i + 1] == b'I'
            && i > from
            && data[i - 1].is_ascii_whitespace()
            && data
                .get(i + 2)
                .map(|b| b.is_ascii_whitespace() || is_delimiter(*b))
                .unwrap_or(true)
        {
            if tail_looks_like_operators(data, i + 2) {
                return Some(i - 1);
            }
            fallback.get_or_insert(i - 1);
        }
        i += 1;
    }
    fallback
}

/// Geht ab `pos` ein paar Token weit und prüft, ob das noch PDF-Syntax ist.
///
/// Kein Beweis, sondern ein Filter: Binärdaten stolpern schon nach ein, zwei
/// Token über ein Byte, das in keinem Token vorkommt (Steuerzeichen, alles
/// über 0x7E). Ein `BI`/`ID` beendet die Prüfung sofort mit „ja“ — dahinter
/// beginnt das nächste Inline-Bild, dessen Daten wieder binär sein dürfen.
fn tail_looks_like_operators(data: &[u8], pos: usize) -> bool {
    let data = &data[..data.len().min(pos.saturating_add(EI_TAIL_WINDOW))];
    let mut i = pos;
    for _ in 0..EI_TAIL_TOKENS {
        while i < data.len() && data[i].is_ascii_whitespace() {
            i += 1;
        }
        // Der Strom ist zu Ende — das ist ein sauberer Abschluss.
        let Some(byte) = data.get(i).copied() else {
            return true;
        };
        match byte {
            // Name
            b'/' => {
                i += 1;
                while i < data.len() && !data[i].is_ascii_whitespace() && !is_delimiter(data[i]) {
                    i += 1;
                }
            }
            b'(' => i = skip_literal_string(data, i),
            b'<' | b'>' | b'[' | b']' | b'{' | b'}' => i += 1,
            b'%' => {
                while i < data.len() && data[i] != b'\n' && data[i] != b'\r' {
                    i += 1;
                }
            }
            b'+' | b'-' | b'.' | b'0'..=b'9' => {
                i += 1;
                while i < data.len() && matches!(data[i], b'+' | b'-' | b'.' | b'0'..=b'9') {
                    i += 1;
                }
            }
            b'\'' | b'"' => i += 1,
            b if b.is_ascii_alphabetic() => {
                let start = i;
                while i < data.len() && (data[i].is_ascii_alphanumeric() || data[i] == b'*') {
                    i += 1;
                }
                if matches!(&data[start..i], b"BI" | b"ID") {
                    return true;
                }
            }
            // Steuerzeichen, hohe Bytes, alles andere: keine Operatorsyntax.
            _ => return false,
        }
    }
    true
}

fn find_ei_end(data: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i + 1 < data.len() {
        if data[i] == b'E' && data[i + 1] == b'I' {
            return Some(i + 2);
        }
        i += 1;
    }
    None
}

fn skip_literal_string(data: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    let mut depth = 1;
    while i < data.len() {
        match data[i] {
            b'\\' => i += 1,
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    i
}

/// Steht an `pos` genau das Token `token` (auf Wortgrenzen)?
fn is_token(data: &[u8], pos: usize, token: &[u8]) -> bool {
    if !data[pos..].starts_with(token) {
        return false;
    }
    let before_ok = pos == 0 || data[pos - 1].is_ascii_whitespace() || is_delimiter(data[pos - 1]);
    let after_ok = data
        .get(pos + token.len())
        .map(|b| b.is_ascii_whitespace() || is_delimiter(*b))
        .unwrap_or(true);
    before_ok && after_ok
}

fn is_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

// ---------------------------------------------------------------------------
// Kleinkram
// ---------------------------------------------------------------------------

/// Ganzzahl aus einem Bild-Dictionary; Inline-Bilder benutzen Kurznamen.
///
/// **Mit Dereferenzierung.** `/Width 12 0 R` ist in freier Wildbahn üblich —
/// wer den Wert direkt aus dem Dictionary liest, bekommt eine `Reference`,
/// findet keine Zahl und hält das Bild für größenlos. Daraus wurde ein
/// Platzhalter, und über einem Platzhalter bricht die Bildschwärzung ab
/// ([`crate::image`]). Deshalb braucht schon das Auslesen das Dokument.
fn dict_int(doc: &Document, dict: &Dictionary, long: &[u8], short: &[u8]) -> Option<i64> {
    match deref(doc, dict.get(long).or_else(|_| dict.get(short)).ok())? {
        Object::Integer(i) => Some(*i),
        Object::Real(r) => Some(*r as i64),
        _ => None,
    }
}

/// Wahrheitswert aus einem Bild-Dictionary — ebenfalls mit Dereferenzierung.
fn dict_bool(doc: &Document, dict: &Dictionary, long: &[u8], short: &[u8]) -> bool {
    deref(doc, dict.get(long).or_else(|_| dict.get(short)).ok())
        .and_then(|o| o.as_bool().ok())
        .unwrap_or(false)
}

fn be_u16(data: &[u8], offset: usize) -> Option<u16> {
    Some(((*data.get(offset)? as u16) << 8) | *data.get(offset + 1)? as u16)
}

fn be_u32(data: &[u8], offset: usize) -> Option<u32> {
    Some(
        ((*data.get(offset)? as u32) << 24)
            | ((*data.get(offset + 1)? as u32) << 16)
            | ((*data.get(offset + 2)? as u32) << 8)
            | *data.get(offset + 3)? as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::scan_page;
    use crate::testing::{build_pdf, demo_statement, TextItem};
    use lopdf::dictionary;

    // -----------------------------------------------------------------
    // Testgeruest
    // -----------------------------------------------------------------

    /// Baut ein einseitiges PDF mit vorgegebenem Content-Stream.
    fn build_doc(
        content: &[u8],
        resources: impl FnOnce(&mut Document) -> Dictionary,
        page_extra: &[(&str, Object)],
    ) -> Document {
        let mut doc = Document::with_version("1.5");
        let resources = resources(&mut doc);
        let resources_id = doc.add_object(resources);
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
        let pages_id = doc.new_object_id();
        let mut page = dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
        };
        for (key, value) in page_extra {
            page.set(key.as_bytes().to_vec(), value.clone());
        }
        let page_id = doc.add_object(page);
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        doc
    }

    fn ops_of(content: &str) -> PageOps {
        let doc = build_doc(content.as_bytes(), |_| dictionary! {}, &[]);
        page_ops(&doc, 0).unwrap()
    }

    fn paths(page: &PageOps) -> Vec<&DrawOp> {
        page.ops
            .iter()
            .filter(|o| matches!(o, DrawOp::Path { .. }))
            .collect()
    }

    fn point(p: Point) -> (f64, f64) {
        ((p.x * 1e6).round() / 1e6, (p.y * 1e6).round() / 1e6)
    }

    fn close_to(a: Rgb, r: f32, g: f32, b: f32) -> bool {
        (a.r - r).abs() < 1e-4 && (a.g - g).abs() < 1e-4 && (a.b - b).abs() < 1e-4
    }

    fn flate(data: &[u8]) -> Vec<u8> {
        use flate2::write::ZlibEncoder;
        use flate2::Compression;
        use std::io::Write;
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    // -----------------------------------------------------------------
    // Pfade, Farben, Zustand
    // -----------------------------------------------------------------

    #[test]
    fn rectangle_fill_becomes_one_path_with_user_space_corners() {
        let page = ops_of("0.2 0.4 0.6 rg 10 20 30 40 re f");
        let ops = paths(&page);
        assert_eq!(ops.len(), 1);
        let DrawOp::Path {
            segments,
            fill,
            stroke,
            even_odd,
            ..
        } = ops[0]
        else {
            unreachable!()
        };
        assert!(stroke.is_none());
        assert!(!even_odd);
        assert!(close_to(fill.unwrap(), 0.2, 0.4, 0.6));
        assert_eq!(segments.len(), 5);
        assert_eq!(
            segments
                .iter()
                .filter_map(|s| match s {
                    PathSeg::MoveTo(p) | PathSeg::LineTo(p) => Some(point(*p)),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec![(10.0, 20.0), (40.0, 20.0), (40.0, 60.0), (10.0, 60.0)]
        );
        assert_eq!(segments.last(), Some(&PathSeg::Close));
    }

    #[test]
    fn cm_scaling_and_translation_reaches_the_path_points() {
        let page = ops_of("2 0 0 2 5 5 cm 10 10 20 20 re f");
        let DrawOp::Path { segments, .. } = paths(&page)[0] else {
            unreachable!()
        };
        assert_eq!(point_of(&segments[0]), (25.0, 25.0));
        assert_eq!(point_of(&segments[2]), (65.0, 65.0));
    }

    fn point_of(seg: &PathSeg) -> (f64, f64) {
        match seg {
            PathSeg::MoveTo(p) | PathSeg::LineTo(p) => point(*p),
            PathSeg::CubicTo(_, _, p) => point(*p),
            PathSeg::Close => (f64::NAN, f64::NAN),
        }
    }

    #[test]
    fn q_and_capital_q_restore_color_line_width_and_clip() {
        let page = ops_of(
            "q 5 w 1 0 0 rg 0 0 10 10 re W n \
             0 0 5 5 re f Q \
             0 0 5 5 re S",
        );
        let ops = paths(&page);
        assert_eq!(ops.len(), 2);

        let DrawOp::Path { fill, clip, .. } = ops[0] else {
            unreachable!()
        };
        assert!(close_to(fill.unwrap(), 1.0, 0.0, 0.0));
        assert_eq!(*clip, Some(ClipRef(0)));

        let DrawOp::Path {
            fill, stroke, clip, ..
        } = ops[1]
        else {
            unreachable!()
        };
        assert!(fill.is_none(), "S faerbt nicht");
        let stroke = stroke.as_ref().unwrap();
        assert!(
            close_to(stroke.color, 0.0, 0.0, 0.0),
            "Farbe zurueckgesetzt"
        );
        assert_eq!(stroke.width, 1.0, "Linienbreite zurueckgesetzt");
        assert_eq!(*clip, None, "Clip nach Q wieder weg");
    }

    #[test]
    fn even_odd_flag_comes_from_the_star_operators() {
        assert!(!matches!(
            paths(&ops_of("0 0 9 9 re f"))[0],
            DrawOp::Path { even_odd: true, .. }
        ));
        assert!(matches!(
            paths(&ops_of("0 0 9 9 re f*"))[0],
            DrawOp::Path { even_odd: true, .. }
        ));
        assert!(matches!(
            paths(&ops_of("0 0 9 9 re B*"))[0],
            DrawOp::Path { even_odd: true, .. }
        ));
    }

    #[test]
    fn gray_and_cmyk_are_converted_to_rgb() {
        let page = ops_of("0.5 g 0 0 1 1 re f 0 1 1 0 k 0 0 1 1 re f 0 0 0 0.5 K 0 0 1 1 re S");
        let ops = paths(&page);
        let DrawOp::Path { fill, .. } = ops[0] else {
            unreachable!()
        };
        assert!(close_to(fill.unwrap(), 0.5, 0.5, 0.5));

        let DrawOp::Path { fill, .. } = ops[1] else {
            unreachable!()
        };
        // (1-c)(1-k) mit c=0, m=y=1, k=0 ergibt reines Rot.
        assert!(close_to(fill.unwrap(), 1.0, 0.0, 0.0));

        let DrawOp::Path { stroke, .. } = ops[2] else {
            unreachable!()
        };
        assert!(close_to(stroke.as_ref().unwrap().color, 0.5, 0.5, 0.5));
    }

    #[test]
    fn indexed_and_separation_color_spaces_are_approximated() {
        let doc = build_doc(
            b"/Pal cs 1 sc 0 0 1 1 re f /Sep cs 1 scn 0 0 1 1 re f",
            |_| {
                dictionary! {
                    "ColorSpace" => dictionary! {
                        "Pal" => vec![
                            Object::Name(b"Indexed".to_vec()),
                            Object::Name(b"DeviceRGB".to_vec()),
                            Object::Integer(1),
                            Object::String(vec![0, 0, 0, 0, 0, 255], StringFormat::Literal),
                        ],
                        "Sep" => vec![
                            Object::Name(b"Separation".to_vec()),
                            Object::Name(b"Spot".to_vec()),
                            Object::Name(b"DeviceGray".to_vec()),
                        ],
                    },
                }
            },
            &[],
        );
        let page = page_ops(&doc, 0).unwrap();
        let ops = paths(&page);
        let DrawOp::Path { fill, .. } = ops[0] else {
            unreachable!()
        };
        assert!(close_to(fill.unwrap(), 0.0, 0.0, 1.0), "Index 1 = Blau");
        let DrawOp::Path { fill, .. } = ops[1] else {
            unreachable!()
        };
        assert!(
            close_to(fill.unwrap(), 0.0, 0.0, 0.0),
            "volle Tinte = Schwarz"
        );
    }

    #[test]
    fn bezier_curve_survives_as_cubic_with_its_control_points() {
        let page = ops_of("10 10 m 20 30 40 30 50 10 c S");
        let DrawOp::Path { segments, .. } = paths(&page)[0] else {
            unreachable!()
        };
        assert_eq!(segments[0], PathSeg::MoveTo(Point::new(10.0, 10.0)));
        assert_eq!(
            segments[1],
            PathSeg::CubicTo(
                Point::new(20.0, 30.0),
                Point::new(40.0, 30.0),
                Point::new(50.0, 10.0)
            )
        );
    }

    #[test]
    fn v_and_y_curves_use_the_implicit_control_points() {
        let page = ops_of("10 10 m 20 30 30 10 v 40 40 50 50 y S");
        let DrawOp::Path { segments, .. } = paths(&page)[0] else {
            unreachable!()
        };
        assert_eq!(
            segments[1],
            PathSeg::CubicTo(
                Point::new(10.0, 10.0),
                Point::new(20.0, 30.0),
                Point::new(30.0, 10.0)
            )
        );
        assert_eq!(
            segments[2],
            PathSeg::CubicTo(
                Point::new(40.0, 40.0),
                Point::new(50.0, 50.0),
                Point::new(50.0, 50.0)
            )
        );
    }

    /// Ein Clip-Pfad wird **einmal** abgelegt, auch wenn er zweimal gesetzt
    /// wird.
    ///
    /// Der zweite `W n` ist der Kern des Tests. Ohne ihn sagte er nur aus, dass
    /// aus **einem** `W n` **ein** Eintrag wird — und das gilt mit und ohne
    /// Entdopplung. Erst der zweite, gleiche Beschnitt unterscheidet die
    /// beiden Fassungen: ohne Entdopplung stünde derselbe Pfad zweimal in
    /// [`PageOps::clips`], und der Rasterizer baute für ihn eine zweite Maske
    /// in voller Bildgröße.
    #[test]
    fn clip_is_stored_once_and_referenced_by_the_following_ops() {
        let page = ops_of(
            "q 0 0 10 10 re W n 1 1 2 2 re f 1 1 2 2 re f Q \
             q 0 0 10 10 re W n 1 1 2 2 re f Q \
             1 1 2 2 re f",
        );
        assert_eq!(page.clips.len(), 1, "derselbe Pfad, zweimal gesetzt");
        assert_eq!(page.clips[0].len(), 5);
        let ops = paths(&page);
        assert!(matches!(
            ops[0],
            DrawOp::Path {
                clip: Some(ClipRef(0)),
                ..
            }
        ));
        assert!(matches!(ops[1], DrawOp::Path { clip: Some(_), .. }));
        // Der zweite Block muss auf **denselben** Eintrag zeigen.
        assert!(
            matches!(
                ops[2],
                DrawOp::Path {
                    clip: Some(ClipRef(0)),
                    ..
                }
            ),
            "der zweite, gleiche Beschnitt zeigt nicht auf denselben Eintrag: {:?}",
            ops[2]
        );
        assert!(matches!(ops[3], DrawOp::Path { clip: None, .. }));
    }

    #[test]
    fn line_width_and_dash_are_scaled_into_user_space() {
        let page = ops_of("3 0 0 3 0 0 cm 2 w [4 2] 1 d 1 J 1 j 0 0 5 5 re S");
        let DrawOp::Path { stroke, .. } = paths(&page)[0] else {
            unreachable!()
        };
        let stroke = stroke.as_ref().unwrap();
        assert_eq!(stroke.width, 6.0);
        assert_eq!(stroke.dash, vec![12.0, 6.0]);
        assert_eq!(stroke.dash_phase, 3.0);
        assert_eq!((stroke.cap, stroke.join), (1, 1));
    }

    #[test]
    fn ext_gstate_supplies_line_width_and_alpha() {
        let doc = build_doc(
            b"/GS1 gs 0 0 5 5 re B",
            |_| {
                dictionary! {
                    "ExtGState" => dictionary! {
                        "GS1" => dictionary! { "LW" => 7, "ca" => 0.25, "CA" => 0.5 },
                    },
                }
            },
            &[],
        );
        let page = page_ops(&doc, 0).unwrap();
        let DrawOp::Path {
            stroke, fill_alpha, ..
        } = paths(&page)[0]
        else {
            unreachable!()
        };
        assert!((fill_alpha - 0.25).abs() < 1e-6);
        let stroke = stroke.as_ref().unwrap();
        assert_eq!(stroke.width, 7.0);
        assert!((stroke.alpha - 0.5).abs() < 1e-6);
    }

    // -----------------------------------------------------------------
    // Text
    // -----------------------------------------------------------------

    /// Anti-Drift: die Glyph-Matrix muss auf denselben Ursprung abbilden, den
    /// `scan_page` fuer dieselbe Glyphe meldet. Waeren das zwei getrennte
    /// Rechenwege, wuerde genau hier die Schwaerzung verrutschen.
    #[test]
    fn glyph_transform_origin_matches_scan_page_exactly() {
        let bytes = demo_statement();
        let doc = crate::document::load_from_bytes(&bytes).unwrap();
        let page_id = *doc.get_pages().values().next().unwrap();

        let scan = scan_page(&doc, page_id).unwrap();
        let expected: Vec<Point> = scan
            .shows
            .iter()
            .flat_map(|s| s.glyphs())
            // Nur das erste Teilzeichen eines Codes traegt die Originalbytes.
            .filter(|g| !g.bytes.is_empty())
            .map(|g| g.origin)
            .collect();

        let page = page_ops(&doc, 0).unwrap();
        let actual: Vec<Point> = page
            .ops
            .iter()
            .filter_map(|op| match op {
                DrawOp::Glyph { transform, .. } => Some(transform.apply(0.0, 0.0)),
                _ => None,
            })
            .collect();

        assert!(!expected.is_empty());
        assert_eq!(expected.len(), actual.len());
        for (a, b) in expected.iter().zip(&actual) {
            assert_eq!(a.x, b.x, "X-Ursprung weicht ab");
            assert_eq!(a.y, b.y, "Y-Ursprung weicht ab");
        }
    }

    #[test]
    fn demo_statement_yields_a_glyph_for_every_character() {
        let bytes = demo_statement();
        let doc = crate::document::load_from_bytes(&bytes).unwrap();
        for page_index in 0..2 {
            let page = page_ops(&doc, page_index).unwrap();
            let scan_glyphs = {
                let page_id = *doc.get_pages().values().nth(page_index).unwrap();
                scan_page(&doc, page_id)
                    .unwrap()
                    .shows
                    .iter()
                    .flat_map(|s| s.glyphs())
                    .filter(|g| !g.bytes.is_empty())
                    .count()
            };
            assert!(scan_glyphs > 100);
            assert_eq!(page.glyph_count(), scan_glyphs);
            assert_eq!(page.fonts.len(), 1);
            assert_eq!(page.fonts[0].base_font, "Helvetica");
            assert_eq!(page.fonts[0].units_per_em, 1000.0);
        }
    }

    #[test]
    fn glyph_transform_maps_glyph_space_to_user_space() {
        let bytes = build_pdf(&[vec![TextItem::new(72.0, 700.0, 12.0, "A")]]);
        let doc = crate::document::load_from_bytes(&bytes).unwrap();
        let page = page_ops(&doc, 0).unwrap();
        let DrawOp::Glyph {
            transform,
            code,
            render_mode,
            fill,
            ..
        } = &page.ops[0]
        else {
            panic!("keine Glyphe");
        };
        assert_eq!(*code, u32::from(b'A'));
        assert_eq!(*render_mode, 0);
        assert!(close_to(*fill, 0.0, 0.0, 0.0));
        // 1000 Glyph-Einheiten entsprechen bei 12 pt genau der Schriftgroesse.
        let origin = transform.apply(0.0, 0.0);
        let em = transform.apply(1000.0, 0.0);
        assert!((origin.x - 72.0).abs() < 1e-9 && (origin.y - 700.0).abs() < 1e-9);
        assert!((em.x - 84.0).abs() < 1e-9);
    }

    #[test]
    fn invisible_text_keeps_its_render_mode() {
        let doc = build_doc(
            b"BT /F1 12 Tf 3 Tr 10 10 Td (Hi) Tj ET",
            |doc| {
                let font = doc.add_object(dictionary! {
                    "Type" => "Font",
                    "Subtype" => "Type1",
                    "BaseFont" => "Helvetica",
                });
                dictionary! { "Font" => dictionary! { "F1" => font } }
            },
            &[],
        );
        let page = page_ops(&doc, 0).unwrap();
        assert_eq!(page.glyph_count(), 2);
        assert!(page
            .ops
            .iter()
            .all(|o| matches!(o, DrawOp::Glyph { render_mode: 3, .. })));
    }

    #[test]
    fn form_xobject_contents_appear_with_the_form_matrix() {
        let doc = build_doc(
            b"1 0 0 1 5 5 cm /Fx1 Do",
            |doc| {
                let form = doc.add_object(Stream::new(
                    dictionary! {
                        "Type" => "XObject",
                        "Subtype" => "Form",
                        "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()],
                        "Matrix" => vec![1.into(), 0.into(), 0.into(), 1.into(), 100.into(), 100.into()],
                    },
                    b"0 0 10 10 re f".to_vec(),
                ));
                dictionary! { "XObject" => dictionary! { "Fx1" => form } }
            },
            &[],
        );
        let page = page_ops(&doc, 0).unwrap();
        let DrawOp::Path { segments, .. } = paths(&page)[0] else {
            unreachable!()
        };
        // Form-Matrix (100/100) und CTM der Seite (5/5) wirken zusammen.
        assert_eq!(point_of(&segments[0]), (105.0, 105.0));
        assert_eq!(point_of(&segments[2]), (115.0, 115.0));
    }

    // -----------------------------------------------------------------
    // Bilder
    // -----------------------------------------------------------------

    fn doc_with_image(content: &[u8], dict: Dictionary, data: Vec<u8>) -> Document {
        build_doc(
            content,
            move |doc| {
                let image = doc.add_object(Stream::new(dict, data));
                dictionary! { "XObject" => dictionary! { "Im1" => image } }
            },
            &[],
        )
    }

    #[test]
    fn image_mask_is_painted_in_the_current_fill_color() {
        let doc = doc_with_image(
            b"1 0 0 rg 100 0 0 100 10 20 cm /Im1 Do",
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 2,
                "Height" => 2,
                "ImageMask" => true,
            },
            // Zeile 0: links malen, Zeile 1: rechts malen.
            vec![0b0100_0000, 0b1000_0000],
        );
        let page = page_ops(&doc, 0).unwrap();
        assert_eq!(page.images.len(), 1);
        let image = &page.images[0];
        assert!(!image.placeholder);
        assert_eq!((image.width, image.height), (2, 2));
        assert_eq!(&image.rgba[0..4], &[255, 0, 0, 255]);
        assert_eq!(image.rgba[7], 0, "zweiter Punkt ist durchsichtig");
        assert_eq!(image.rgba[11], 0);
        assert_eq!(&image.rgba[12..16], &[255, 0, 0, 255]);

        let DrawOp::Image { ctm, image, .. } = &page.ops[0] else {
            panic!("kein Bild")
        };
        assert_eq!(*image, 0);
        assert_eq!(point(ctm.apply(0.0, 0.0)), (10.0, 20.0));
        assert_eq!(point(ctm.apply(1.0, 1.0)), (110.0, 120.0));
    }

    #[test]
    fn eight_bit_rgb_flate_image_decodes_to_the_expected_pixels() {
        let pixels = vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
        let doc = doc_with_image(
            b"100 0 0 100 0 0 cm /Im1 Do",
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 2,
                "Height" => 2,
                "BitsPerComponent" => 8,
                "ColorSpace" => "DeviceRGB",
                "Filter" => "FlateDecode",
            },
            flate(&pixels),
        );
        let page = page_ops(&doc, 0).unwrap();
        let image = &page.images[0];
        assert!(!image.placeholder);
        assert_eq!(
            image.rgba,
            vec![
                255, 0, 0, 255, //
                0, 255, 0, 255, //
                0, 0, 255, 255, //
                255, 255, 255, 255,
            ]
        );
    }

    #[test]
    fn one_bit_gray_image_decodes_black_and_white() {
        // Genau der Fall, den gescannte Seiten benutzen.
        let doc = doc_with_image(
            b"10 0 0 10 0 0 cm /Im1 Do",
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 4,
                "Height" => 1,
                "BitsPerComponent" => 1,
                "ColorSpace" => "DeviceGray",
            },
            vec![0b1010_0000],
        );
        let page = page_ops(&doc, 0).unwrap();
        let rgba = &page.images[0].rgba;
        assert_eq!(&rgba[0..4], &[255, 255, 255, 255]);
        assert_eq!(&rgba[4..8], &[0, 0, 0, 255]);
        assert_eq!(&rgba[8..12], &[255, 255, 255, 255]);
    }

    #[test]
    fn undecodable_image_yields_a_placeholder_instead_of_an_error() {
        let doc = doc_with_image(
            b"10 0 0 10 0 0 cm /Im1 Do",
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 8,
                "Height" => 8,
                "BitsPerComponent" => 8,
                "ColorSpace" => "DeviceGray",
                "Filter" => "JPXDecode",
            },
            vec![0x00, 0x01, 0x02],
        );
        let page = page_ops(&doc, 0).unwrap();
        assert_eq!(page.images.len(), 1);
        assert!(page.images[0].placeholder);
        assert_eq!(page.images[0].rgba, vec![128, 128, 128, 255]);
        assert!(page.notes.iter().any(|n| n.contains("JPXDecode")));
        assert!(matches!(page.ops[0], DrawOp::Image { .. }));
    }

    #[test]
    fn broken_samples_still_produce_a_placeholder() {
        let doc = doc_with_image(
            b"10 0 0 10 0 0 cm /Im1 Do",
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 100,
                "Height" => 100,
                "BitsPerComponent" => 8,
                "ColorSpace" => "DeviceRGB",
            },
            vec![1, 2, 3],
        );
        let page = page_ops(&doc, 0).unwrap();
        assert!(page.images[0].placeholder);
        assert!(!page.notes.is_empty());
    }

    #[test]
    fn soft_mask_becomes_the_alpha_channel() {
        let mut doc = Document::with_version("1.5");
        let smask = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 2,
                "Height" => 1,
                "BitsPerComponent" => 8,
                "ColorSpace" => "DeviceGray",
            },
            vec![0, 255],
        ));
        let image = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 2,
                "Height" => 1,
                "BitsPerComponent" => 8,
                "ColorSpace" => "DeviceRGB",
                "SMask" => smask,
            },
            vec![10, 20, 30, 40, 50, 60],
        ));
        let doc = build_doc(
            b"1 0 0 1 0 0 cm /Im1 Do",
            move |target| {
                // Objekte in das Zieldokument uebernehmen.
                for (id, object) in doc.objects.iter() {
                    target.objects.insert(*id, object.clone());
                }
                target.max_id = target.max_id.max(doc.max_id);
                dictionary! { "XObject" => dictionary! { "Im1" => image } }
            },
            &[],
        );
        let page = page_ops(&doc, 0).unwrap();
        assert_eq!(page.images[0].rgba[3], 0);
        assert_eq!(page.images[0].rgba[7], 255);
    }

    #[test]
    fn inline_image_is_decoded_and_does_not_truncate_the_stream() {
        let mut content = b"q 20 0 0 20 0 0 cm BI /W 2 /H 1 /CS /RGB /BPC 8 ID ".to_vec();
        content.extend_from_slice(&[255, 0, 0, 0, 0, 255]);
        content.extend_from_slice(b"\nEI Q 0 0 5 5 re f");

        let doc = build_doc(&content, |_| dictionary! {}, &[]);
        let page = page_ops(&doc, 0).unwrap();
        assert_eq!(page.images.len(), 1);
        assert_eq!(page.images[0].rgba, vec![255, 0, 0, 255, 0, 0, 255, 255]);
        // Der Pfad hinter dem Inline-Bild darf nicht verlorengehen.
        assert_eq!(paths(&page).len(), 1);
        assert!(matches!(page.ops[0], DrawOp::Image { .. }));
    }

    #[test]
    fn inline_image_scanner_ignores_bi_inside_strings() {
        let found = find_inline_images(b"(BI ID EI) Tj 0 0 1 1 re f");
        assert!(found.is_empty());
    }

    /// Gefilterte Nutzdaten: die Länge lässt sich nicht rechnen, und `/L`
    /// fehlt. Dann entscheidet allein, ob hinter dem `EI` wirklich ein
    /// Operatorstrom weitergeht — sonst schnitte das erste zufällige `EI`
    /// das Bild ab und machte aus dem Rest der Seite Binärmüll.
    #[test]
    fn a_filtered_inline_image_ends_at_the_ei_that_operators_follow() {
        let mut payload = vec![0x78u8, 0x9c, 0x01, 0x02];
        payload.extend_from_slice(b" EI ");
        payload.extend_from_slice(&[0x80, 0xff, 0x0e, 0x9a]);

        let mut content = b"q BI /W 2 /H 2 /CS /G /BPC 8 /F /Fl ID ".to_vec();
        content.extend_from_slice(&payload);
        content.extend_from_slice(b"\nEI\nQ 0 0 5 5 re f");

        let images = find_inline_images(&content);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].3, payload, "die Nutzdaten wurden abgeschnitten");

        let decoded = decode_content_checked(&content);
        assert!(
            decoded.truncated.is_empty(),
            "hinter dem Bild ging etwas verloren"
        );
        let operators: Vec<&str> = decoded
            .operations
            .iter()
            .map(|op| op.operator.as_str())
            .collect();
        assert_eq!(operators, ["q", "BI", "Q", "re", "f"]);
    }

    /// Ein `/L`, das der Erzeuger zu groß angegeben hat, wird nicht geglaubt:
    /// an seinem Ende steht kein `EI`, also entscheidet die gerechnete Länge.
    #[test]
    fn a_declared_length_only_counts_when_an_ei_follows_it() {
        let payload: Vec<u8> = (0u8..4).collect();
        let mut content = b"BI /W 2 /H 2 /CS /G /BPC 8 /L 40 ID ".to_vec();
        content.extend_from_slice(&payload);
        content.extend_from_slice(b"\nEI\n0 0 5 5 re f");

        let images = find_inline_images(&content);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].3, payload);
        assert_eq!(unfiltered_payload_len(&images[0].2), Some(4));
    }

    /// Eine Stencil-Maske rechnet mit einem Bit je Pixel, aufgerundet je Zeile.
    #[test]
    fn the_payload_length_of_a_stencil_mask_is_rounded_up_per_row() {
        let mut dict = Dictionary::new();
        dict.set("W", Object::Integer(9));
        dict.set("H", Object::Integer(3));
        dict.set("IM", Object::Boolean(true));
        assert_eq!(unfiltered_payload_len(&dict), Some(6));

        // Ein benannter Farbraum aus den Ressourcen: hier wird nicht geraten.
        let mut dict = Dictionary::new();
        dict.set("W", Object::Integer(4));
        dict.set("H", Object::Integer(4));
        dict.set("CS", Object::Name(b"Cs1".to_vec()));
        assert_eq!(unfiltered_payload_len(&dict), None);
    }

    /// Ein Teilstück, das der Parser nur zur Hälfte aufbraucht, wird gemeldet
    /// — `Content::decode` allein liefert dafür ein arglos aussehendes `Ok`.
    #[test]
    fn a_partially_parsed_chunk_is_reported_instead_of_dropped() {
        let decoded = decode_content_checked(b"q Q \x01\x02 0 0 5 5 re f");
        assert_eq!(decoded.truncated.len(), 1);
        assert!(decoded.affected_bytes() > 0);

        // Gegenprobe: ein abschließender Kommentar ohne Zeilenende und ein
        // Seitenvorschub als Leerraum sind kein Verlust.
        assert!(decode_content_checked(b"q Q % Schluss")
            .truncated
            .is_empty());
        assert!(decode_content_checked(b"q Q\x0c").truncated.is_empty());
    }

    // -----------------------------------------------------------------
    // Seitenattribute
    // -----------------------------------------------------------------

    #[test]
    fn rotate_is_reported_including_inheritance() {
        let doc = build_doc(b"", |_| dictionary! {}, &[("Rotate", Object::Integer(90))]);
        let page = page_ops(&doc, 0).unwrap();
        assert_eq!(page.rotate, 90);
        assert_eq!(page.page, 0);
        assert_eq!(page.media_box, Rect::new(0.0, 0.0, 200.0, 200.0));

        // Geerbt vom /Pages-Knoten und negativ angegeben.
        let mut doc = build_doc(b"", |_| dictionary! {}, &[]);
        let pages_id = *doc
            .objects
            .iter()
            .find(|(_, o)| {
                o.as_dict()
                    .map(|d| d.get(b"Type").and_then(Object::as_name).ok() == Some(b"Pages"))
                    .unwrap_or(false)
            })
            .map(|(id, _)| id)
            .unwrap();
        if let Some(Object::Dictionary(d)) = doc.objects.get_mut(&pages_id) {
            d.set("Rotate", Object::Integer(-90));
        }
        assert_eq!(page_ops(&doc, 0).unwrap().rotate, 270);
    }

    #[test]
    fn unknown_page_index_is_an_error() {
        let doc = build_doc(b"", |_| dictionary! {}, &[]);
        assert!(page_ops(&doc, 7).is_err());
    }

    // -----------------------------------------------------------------
    // Eingebettete Fonts
    // -----------------------------------------------------------------

    /// Minimaler sfnt-Font mit genau einer `head`-Tabelle.
    fn sfnt_with_units_per_em(upem: u16) -> Vec<u8> {
        let mut data = vec![0u8; 12];
        data[0..4].copy_from_slice(&[0x00, 0x01, 0x00, 0x00]);
        data[4..6].copy_from_slice(&1u16.to_be_bytes()); // numTables
        data.extend_from_slice(b"head");
        data.extend_from_slice(&[0, 0, 0, 0]); // checkSum
        data.extend_from_slice(&28u32.to_be_bytes()); // offset
        data.extend_from_slice(&54u32.to_be_bytes()); // length
        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&upem.to_be_bytes());
        data.extend_from_slice(&head);
        data
    }

    #[test]
    fn embedded_truetype_font_reports_its_units_per_em() {
        let doc = build_doc(
            b"BT /F1 10 Tf 20 30 Td (A) Tj ET",
            |doc| {
                let file =
                    doc.add_object(Stream::new(dictionary! {}, sfnt_with_units_per_em(2048)));
                let descriptor = doc.add_object(dictionary! {
                    "Type" => "FontDescriptor",
                    "FontName" => "Testfont",
                    "Flags" => 34,
                    "FontFile2" => file,
                });
                let font = doc.add_object(dictionary! {
                    "Type" => "Font",
                    "Subtype" => "TrueType",
                    "BaseFont" => "Testfont",
                    "FirstChar" => 65,
                    "Widths" => vec![Object::Integer(600)],
                    "FontDescriptor" => descriptor,
                });
                dictionary! { "Font" => dictionary! { "F1" => font } }
            },
            &[],
        );
        let page = page_ops(&doc, 0).unwrap();
        let program = &page.fonts[0];
        assert_eq!(program.kind, FontKind::TrueType);
        assert_eq!(program.units_per_em, 2048.0);
        assert_eq!(program.flags, 34);
        assert!(!program.is_cid);
        assert_eq!(program.code_to_gid, CodeToGid::ViaCharCode);
        assert_eq!(program.widths.get(&65), Some(&0.6));
        assert_eq!(
            program.code_to_unicode.get(&65).map(String::as_str),
            Some("A")
        );
        assert!(program.data.is_some());

        // Der Glyph-Space richtet sich nach den 2048 Einheiten des Fonts.
        let DrawOp::Glyph { transform, .. } = &page.ops[0] else {
            panic!("keine Glyphe")
        };
        let origin = transform.apply(0.0, 0.0);
        let em = transform.apply(2048.0, 0.0);
        assert!((origin.x - 20.0).abs() < 1e-9 && (origin.y - 30.0).abs() < 1e-9);
        assert!((em.x - 30.0).abs() < 1e-9, "2048 Einheiten = 10 pt");
    }

    #[test]
    fn cid_font_uses_two_byte_codes_and_the_cid_to_gid_table() {
        let doc = build_doc(
            b"BT /F1 12 Tf 0 0 Td <00030004> Tj ET",
            |doc| {
                let map = doc.add_object(Stream::new(
                    dictionary! {},
                    // CID 3 -> GID 7, CID 4 -> GID 9
                    vec![0, 0, 0, 0, 0, 0, 0, 7, 0, 9],
                ));
                let descriptor = doc.add_object(dictionary! {
                    "Type" => "FontDescriptor",
                    "FontName" => "CIDfont",
                    "Flags" => 4,
                });
                let descendant = doc.add_object(dictionary! {
                    "Type" => "Font",
                    "Subtype" => "CIDFontType2",
                    "BaseFont" => "CIDfont",
                    "DW" => 1000,
                    "W" => vec![
                        Object::Integer(3),
                        Object::Array(vec![Object::Integer(500), Object::Integer(500)]),
                    ],
                    "CIDToGIDMap" => map,
                    "FontDescriptor" => descriptor,
                });
                let font = doc.add_object(dictionary! {
                    "Type" => "Font",
                    "Subtype" => "Type0",
                    "BaseFont" => "CIDfont",
                    "Encoding" => "Identity-H",
                    "DescendantFonts" => vec![Object::Reference(descendant)],
                });
                dictionary! { "Font" => dictionary! { "F1" => font } }
            },
            &[],
        );
        let page = page_ops(&doc, 0).unwrap();
        let program = &page.fonts[0];
        assert!(program.is_cid);
        assert_eq!(program.default_width, 1.0);
        let CodeToGid::Map(table) = &program.code_to_gid else {
            panic!("keine CIDToGID-Tabelle")
        };
        assert_eq!(table.get(&3), Some(&7));
        assert_eq!(table.get(&4), Some(&9));

        let codes: Vec<u32> = page
            .ops
            .iter()
            .filter_map(|o| match o {
                DrawOp::Glyph { code, .. } => Some(*code),
                _ => None,
            })
            .collect();
        assert_eq!(codes, vec![3, 4]);
        // Vorschub: 500/1000 * 12 pt zwischen den beiden Glyphen.
        let DrawOp::Glyph { transform, .. } = &page.ops[1] else {
            unreachable!()
        };
        assert!((transform.apply(0.0, 0.0).x - 6.0).abs() < 1e-9);
    }

    #[test]
    fn shading_operator_does_not_break_the_stream() {
        let page = ops_of("/Sh1 sh 0 0 5 5 re f");
        assert_eq!(paths(&page).len(), 1);
    }
}
