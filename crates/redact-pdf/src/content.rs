//! Interpreter für PDF-Content-Streams.
//!
//! Der Scanner läuft den Content-Stream einer Seite durch, führt den
//! Grafik- und Textzustand nach (CTM, `Tm`, `Tf`, `Tc`, `Tw`, `Tz`, `Ts`, `TL`)
//! und berechnet für **jedes einzelne Zeichen** eine Bounding-Box im
//! User-Space.
//!
//! Das Ergebnis (`ShowRecord`) wird von zwei Seiten genutzt:
//!
//! * `extract.rs` baut daraus Textzeilen mit zeichengenauen Koordinaten,
//! * `redact.rs` schreibt daraus den Content-Stream neu und entfernt gezielt
//!   einzelne Glyphen.
//!
//! Form-XObjects (`Do`) werden rekursiv mitverarbeitet — dort steht in vielen
//! generierten PDFs der eigentliche Text.

use std::collections::{BTreeMap, HashSet};

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId};
use redact_core::{Point, Rect, RedactError, Result};

use crate::font::{fonts_from_resources, FontInfo};
use crate::matrix::Matrix;

/// Maximale Rekursionstiefe für verschachtelte Form-XObjects.
const MAX_FORM_DEPTH: usize = 8;

/// Aus welchem Stream ein Datensatz stammt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StreamKey {
    /// Der (ggf. zusammengesetzte) Content-Stream der Seite.
    Page,
    /// Ein Form-XObject.
    Form(ObjectId),
}

/// Ein einzelnes gesetztes Zeichen.
#[derive(Debug, Clone)]
pub struct GlyphItem {
    /// Originalbytes des Zeichencodes (für das Neuschreiben des Streams).
    pub bytes: Vec<u8>,
    /// Dekodierter Text (kann bei Ligaturen mehrere Zeichen umfassen).
    pub text: String,
    /// Bounding-Box im User-Space.
    pub rect: Rect,
    /// Ursprung (Grundlinie, linke Kante) im User-Space.
    pub origin: Point,
    /// Vorschub im Textraum vor Anwendung von `Tm`/CTM.
    pub displacement: f64,
}

/// Bestandteil einer Text-Ausgabe-Operation.
#[derive(Debug, Clone)]
pub enum ShowItem {
    Glyph(GlyphItem),
    /// Zahlenwert aus einem `TJ`-Array (Kerning).
    Adjust(f64),
}

/// Eine Text-Ausgabe-Operation mit allen berechneten Glyphen.
#[derive(Debug, Clone)]
pub struct ShowRecord {
    pub stream: StreamKey,
    /// Index der Operation im dekodierten Stream.
    pub op_index: usize,
    pub operator: String,
    /// Originaloperanden der Operation (für `'` und `"` gebraucht).
    pub operands: Vec<Object>,
    pub font_size: f64,
    pub h_scale: f64,
    pub items: Vec<ShowItem>,
}

impl ShowRecord {
    pub fn glyphs(&self) -> impl Iterator<Item = &GlyphItem> {
        self.items.iter().filter_map(|i| match i {
            ShowItem::Glyph(g) => Some(g),
            ShowItem::Adjust(_) => None,
        })
    }
}

/// Ergebnis eines Seiten-Scans.
#[derive(Debug, Default)]
pub struct ScanResult {
    pub shows: Vec<ShowRecord>,
    /// Wie oft ein Form-XObject auf dieser Seite gezeichnet wurde.
    pub form_placements: BTreeMap<ObjectId, usize>,
}

#[derive(Debug, Clone)]
struct TextState {
    font: Option<FontInfo>,
    font_size: f64,
    char_spacing: f64,
    word_spacing: f64,
    h_scale: f64,
    leading: f64,
    rise: f64,
}

impl Default for TextState {
    fn default() -> Self {
        Self {
            font: None,
            font_size: 0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            h_scale: 1.0,
            leading: 0.0,
            rise: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
struct GraphicsState {
    ctm: Matrix,
    text: TextState,
}

/// Scannt den Content-Stream einer Seite inklusive Form-XObjects.
pub fn scan_page(doc: &Document, page_id: ObjectId) -> Result<ScanResult> {
    let content_data = doc
        .get_page_content(page_id)
        .map_err(|e| RedactError::Pdf(format!("Content-Stream nicht lesbar: {e}")))?;
    let content = Content::decode(&content_data)
        .map_err(|e| RedactError::Pdf(format!("Content-Stream nicht dekodierbar: {e}")))?;

    let resources = page_resources(doc, page_id);
    let fonts = fonts_from_resources(doc, resources.as_ref());

    let mut result = ScanResult::default();
    let mut visiting = HashSet::new();
    scan_operations(
        doc,
        &content.operations,
        StreamKey::Page,
        resources.as_ref(),
        &fonts,
        Matrix::IDENTITY,
        0,
        &mut visiting,
        &mut result,
    );
    Ok(result)
}

/// Sammelt das (ggf. geerbte) `/Resources`-Dictionary einer Seite.
pub fn page_resources(doc: &Document, page_id: ObjectId) -> Option<Dictionary> {
    let (dict, ids) = doc.get_page_resources(page_id).ok()?;
    let mut merged = Dictionary::new();
    // Geerbte Ressourcen zuerst, damit die seiteneigenen sie überschreiben.
    for id in ids {
        if let Ok(d) = doc.get_dictionary(id) {
            merge_resources(&mut merged, d);
        }
    }
    if let Some(d) = dict {
        merge_resources(&mut merged, d);
    }
    Some(merged)
}

fn merge_resources(target: &mut Dictionary, source: &Dictionary) {
    for (key, value) in source.iter() {
        match (
            target.get(key).ok().and_then(|o| o.as_dict().ok()).cloned(),
            value.as_dict().ok(),
        ) {
            (Some(mut existing), Some(incoming)) => {
                for (k, v) in incoming.iter() {
                    existing.set(k.to_vec(), v.clone());
                }
                target.set(key.to_vec(), Object::Dictionary(existing));
            }
            _ => target.set(key.to_vec(), value.clone()),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn scan_operations(
    doc: &Document,
    operations: &[Operation],
    stream: StreamKey,
    resources: Option<&Dictionary>,
    fonts: &BTreeMap<Vec<u8>, FontInfo>,
    initial_ctm: Matrix,
    depth: usize,
    visiting: &mut HashSet<ObjectId>,
    out: &mut ScanResult,
) {
    let mut state = GraphicsState {
        ctm: initial_ctm,
        text: TextState::default(),
    };
    let mut stack: Vec<GraphicsState> = Vec::new();
    // Textmatrix und Zeilenmatrix
    let mut tm = Matrix::IDENTITY;
    let mut tlm = Matrix::IDENTITY;

    for (op_index, op) in operations.iter().enumerate() {
        match op.operator.as_str() {
            "q" => stack.push(state.clone()),
            "Q" => {
                if let Some(prev) = stack.pop() {
                    state = prev;
                }
            }
            "cm" => {
                if let Some(m) = matrix_from(&op.operands) {
                    state.ctm = m.mul(&state.ctm);
                }
            }
            "BT" => {
                tm = Matrix::IDENTITY;
                tlm = Matrix::IDENTITY;
            }
            "ET" => {}
            "Tf" => {
                if let Some(Object::Name(name)) = op.operands.first() {
                    state.text.font = fonts.get(name.as_slice()).cloned();
                }
                state.text.font_size = op.operands.get(1).and_then(num).unwrap_or(0.0);
            }
            "Tc" => state.text.char_spacing = op.operands.first().and_then(num).unwrap_or(0.0),
            "Tw" => state.text.word_spacing = op.operands.first().and_then(num).unwrap_or(0.0),
            "Tz" => state.text.h_scale = op.operands.first().and_then(num).unwrap_or(100.0) / 100.0,
            "TL" => state.text.leading = op.operands.first().and_then(num).unwrap_or(0.0),
            "Ts" => state.text.rise = op.operands.first().and_then(num).unwrap_or(0.0),
            "Td" => {
                let tx = op.operands.first().and_then(num).unwrap_or(0.0);
                let ty = op.operands.get(1).and_then(num).unwrap_or(0.0);
                tlm = Matrix::translate(tx, ty).mul(&tlm);
                tm = tlm;
            }
            "TD" => {
                let tx = op.operands.first().and_then(num).unwrap_or(0.0);
                let ty = op.operands.get(1).and_then(num).unwrap_or(0.0);
                state.text.leading = -ty;
                tlm = Matrix::translate(tx, ty).mul(&tlm);
                tm = tlm;
            }
            "Tm" => {
                if let Some(m) = matrix_from(&op.operands) {
                    tlm = m;
                    tm = m;
                }
            }
            "T*" => {
                tlm = Matrix::translate(0.0, -state.text.leading).mul(&tlm);
                tm = tlm;
            }
            "Tj" | "TJ" | "'" | "\"" => {
                // Die Operatoren ' und " beginnen eine neue Zeile.
                if op.operator == "'" || op.operator == "\"" {
                    if op.operator == "\"" {
                        state.text.word_spacing = op
                            .operands
                            .first()
                            .and_then(num)
                            .unwrap_or(state.text.word_spacing);
                        state.text.char_spacing = op
                            .operands
                            .get(1)
                            .and_then(num)
                            .unwrap_or(state.text.char_spacing);
                    }
                    tlm = Matrix::translate(0.0, -state.text.leading).mul(&tlm);
                    tm = tlm;
                }
                let record = show_text(
                    &op.operands,
                    &state,
                    &mut tm,
                    stream,
                    op_index,
                    &op.operator,
                );
                if let Some(record) = record {
                    out.shows.push(record);
                }
            }
            "Do" => {
                if depth >= MAX_FORM_DEPTH {
                    continue;
                }
                let Some(Object::Name(name)) = op.operands.first() else {
                    continue;
                };
                let Some((form_id, form_dict, form_ops)) = load_form(doc, resources, name) else {
                    continue;
                };
                *out.form_placements.entry(form_id).or_insert(0) += 1;
                if !visiting.insert(form_id) {
                    continue; // Zyklus
                }
                let form_matrix = form_dict
                    .get(b"Matrix")
                    .ok()
                    .and_then(|o| o.as_array().ok())
                    .and_then(|a| matrix_from(a))
                    .unwrap_or(Matrix::IDENTITY);
                let form_resources = form_dict
                    .get(b"Resources")
                    .ok()
                    .and_then(|o| doc.dereference(o).ok())
                    .and_then(|(_, o)| o.as_dict().ok())
                    .cloned()
                    .or_else(|| resources.cloned());
                let form_fonts = fonts_from_resources(doc, form_resources.as_ref());
                scan_operations(
                    doc,
                    &form_ops,
                    StreamKey::Form(form_id),
                    form_resources.as_ref(),
                    &form_fonts,
                    form_matrix.mul(&state.ctm),
                    depth + 1,
                    visiting,
                    out,
                );
                visiting.remove(&form_id);
            }
            _ => {}
        }
    }
}

fn load_form(
    doc: &Document,
    resources: Option<&Dictionary>,
    name: &[u8],
) -> Option<(ObjectId, Dictionary, Vec<Operation>)> {
    let xobjects = resources?.get(b"XObject").ok()?;
    let (_, xobjects) = doc.dereference(xobjects).ok()?;
    let entry = xobjects.as_dict().ok()?.get(name).ok()?;
    let id = match entry {
        Object::Reference(id) => *id,
        _ => return None,
    };
    let stream = doc.get_object(id).ok()?.as_stream().ok()?;
    if stream.dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Form") {
        return None;
    }
    let data = stream
        .decompressed_content()
        .or_else(|_| stream.get_plain_content())
        .ok()?;
    let content = Content::decode(&data).ok()?;
    Some((id, stream.dict.clone(), content.operations))
}

/// Berechnet die Glyphen einer Text-Ausgabe-Operation und schreibt `tm` fort.
fn show_text(
    operands: &[Object],
    state: &GraphicsState,
    tm: &mut Matrix,
    stream: StreamKey,
    op_index: usize,
    operator: &str,
) -> Option<ShowRecord> {
    let font = state.text.font.clone().unwrap_or_default();
    let ts = &state.text;

    // Die Textargumente stehen bei ' und " nicht an erster Stelle.
    let arg = match operator {
        "\"" => operands.get(2)?,
        "'" => operands.first()?,
        _ => operands.first()?,
    };

    let mut elements: Vec<Object> = Vec::new();
    match arg {
        Object::Array(items) => elements.extend(items.iter().cloned()),
        other => elements.push(other.clone()),
    }

    let mut items = Vec::new();
    for element in &elements {
        match element {
            Object::String(bytes, _) => {
                for (code, text, nbytes) in font.charmap.decode(bytes) {
                    let w0 = font.width(code, &text);
                    let is_space = nbytes == 1 && code == 32;
                    let displacement = (w0 * ts.font_size
                        + ts.char_spacing
                        + if is_space { ts.word_spacing } else { 0.0 })
                        * ts.h_scale;

                    let param = Matrix::new(
                        ts.font_size * ts.h_scale,
                        0.0,
                        0.0,
                        ts.font_size,
                        0.0,
                        ts.rise,
                    );
                    let trm = param.mul(&tm.mul(&state.ctm));
                    let rect = glyph_rect(&trm, w0, font.ascent, font.descent);
                    let origin = trm.apply(0.0, 0.0);

                    // Ligaturen (ein Code → mehrere Zeichen) werden zeichenweise
                    // aufgeteilt, damit Text und Glyphen 1:1 zusammenpassen.
                    let char_count = text.chars().count().max(1);
                    if char_count == 1 {
                        items.push(ShowItem::Glyph(GlyphItem {
                            bytes: raw_code_bytes(bytes, &font, code, nbytes),
                            text,
                            rect,
                            origin,
                            displacement,
                        }));
                    } else {
                        let bytes_for_code = raw_code_bytes(bytes, &font, code, nbytes);
                        let width = rect.width() / char_count as f64;
                        for (i, ch) in text.chars().enumerate() {
                            let sub = Rect::new(
                                rect.ll.x + width * i as f64,
                                rect.ll.y,
                                rect.ll.x + width * (i + 1) as f64,
                                rect.ur.y,
                            );
                            items.push(ShowItem::Glyph(GlyphItem {
                                // Nur das erste Teilzeichen trägt die Originalbytes,
                                // damit der Code beim Neuschreiben nicht dupliziert wird.
                                bytes: if i == 0 {
                                    bytes_for_code.clone()
                                } else {
                                    Vec::new()
                                },
                                text: ch.to_string(),
                                rect: sub,
                                origin,
                                displacement: if i == 0 { displacement } else { 0.0 },
                            }));
                        }
                    }

                    *tm = Matrix::translate(displacement, 0.0).mul(tm);
                }
            }
            Object::Integer(_) | Object::Real(_) => {
                let adj = num(element).unwrap_or(0.0);
                let tx = -adj / 1000.0 * ts.font_size * ts.h_scale;
                *tm = Matrix::translate(tx, 0.0).mul(tm);
                items.push(ShowItem::Adjust(adj));
            }
            _ => {}
        }
    }

    Some(ShowRecord {
        stream,
        op_index,
        operator: operator.to_string(),
        operands: operands.to_vec(),
        font_size: ts.font_size,
        h_scale: ts.h_scale,
        items,
    })
}

/// Liefert die Originalbytes eines Codes.
fn raw_code_bytes(_source: &[u8], _font: &FontInfo, code: u32, nbytes: usize) -> Vec<u8> {
    match nbytes {
        2 => vec![(code >> 8) as u8, (code & 0xFF) as u8],
        _ => vec![(code & 0xFF) as u8],
    }
}

/// Bounding-Box eines Glyphen: die vier Ecken werden transformiert, daraus
/// wird die achsenparallele Hülle gebildet (funktioniert auch bei Rotation).
fn glyph_rect(trm: &Matrix, width: f64, ascent: f64, descent: f64) -> Rect {
    let corners = [
        trm.apply(0.0, descent),
        trm.apply(width, descent),
        trm.apply(width, ascent),
        trm.apply(0.0, ascent),
    ];
    let mut min = Point::new(f64::MAX, f64::MAX);
    let mut max = Point::new(f64::MIN, f64::MIN);
    for c in corners {
        min.x = min.x.min(c.x);
        min.y = min.y.min(c.y);
        max.x = max.x.max(c.x);
        max.y = max.y.max(c.y);
    }
    Rect { ll: min, ur: max }
}

fn matrix_from(operands: &[Object]) -> Option<Matrix> {
    if operands.len() < 6 {
        return None;
    }
    Some(Matrix::new(
        num(&operands[0])?,
        num(&operands[1])?,
        num(&operands[2])?,
        num(&operands[3])?,
        num(&operands[4])?,
        num(&operands[5])?,
    ))
}

fn num(obj: &Object) -> Option<f64> {
    match obj {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(*r as f64),
        _ => None,
    }
}

/// Ermittelt die CTM am Ende des Streams auf Stapel-Ebene 0 sowie die Zahl
/// nicht geschlossener `q`-Operationen. Beides wird gebraucht, um die
/// Deck-Rechtecke anschließend im unveränderten User-Space zu zeichnen.
pub fn trailing_state(operations: &[Operation]) -> (Matrix, usize) {
    let mut base_ctm = Matrix::IDENTITY;
    let mut depth = 0usize;
    for op in operations {
        match op.operator.as_str() {
            "q" => depth += 1,
            "Q" => depth = depth.saturating_sub(1),
            "cm" if depth == 0 => {
                if let Some(m) = matrix_from(&op.operands) {
                    base_ctm = m.mul(&base_ctm);
                }
            }
            _ => {}
        }
    }
    (base_ctm, depth)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ops(src: &[u8]) -> Vec<Operation> {
        Content::decode(src).unwrap().operations
    }

    #[test]
    fn tracks_unbalanced_q_and_base_ctm() {
        let (ctm, depth) = trailing_state(&ops(b"q 1 0 0 1 5 5 cm Q 2 0 0 2 0 0 cm q q"));
        assert_eq!(depth, 2);
        assert_eq!(ctm, Matrix::scale(2.0, 2.0));
    }

    #[test]
    fn glyph_rect_is_axis_aligned_hull() {
        // 90°-Drehung: Breite und Höhe tauschen die Plätze.
        let rot = Matrix::new(0.0, 1.0, -1.0, 0.0, 0.0, 0.0);
        let trm = Matrix::scale(10.0, 10.0).mul(&rot);
        let r = glyph_rect(&trm, 0.5, 0.75, -0.25);
        assert!((r.width() - 10.0).abs() < 1e-9);
        assert!((r.height() - 5.0).abs() < 1e-9);
    }
}
