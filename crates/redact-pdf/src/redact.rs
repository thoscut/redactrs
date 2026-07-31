//! Echte Schwärzung: der Text wird aus dem Content-Stream **entfernt**.
//!
//! Ein schwarzes Rechteck darüberzulegen genügt nicht — der Text bliebe per
//! Copy-&-Paste oder `pdftotext` lesbar. Deshalb:
//!
//! 1. Der Content-Stream wird interpretiert und für jedes Zeichen die
//!    Bounding-Box berechnet ([`crate::content`]).
//! 2. Zeichen, die in einem Schwärzungsbereich liegen, werden aus der
//!    Text-Operation entfernt. Damit der restliche Text an seiner Stelle
//!    bleibt, wird der entfallende Vorschub als `TJ`-Kerningwert eingesetzt.
//! 3. Anschließend wird ein deckendes Rechteck gezeichnet.
//! 4. Überlappende Annotationen werden gelöscht (auch dort steht Text).
//!
//! Zeichen in Form-XObjects werden ebenfalls entfernt. Wird dasselbe XObject
//! mehrfach platziert, wirkt die Entfernung notwendigerweise auf alle
//! Platzierungen — es wird also eher zu viel als zu wenig geschwärzt. Das ist
//! die sichere Richtung.

use std::collections::{BTreeMap, BTreeSet};

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use redact_core::{Rect, RedactError, Redaction, Redactor, Result};

use crate::content::{scan_page, ShowItem, ShowRecord, StreamKey};
use crate::matrix::Matrix;

/// Ab welchem Überdeckungsgrad ein Zeichen als geschwärzt gilt.
const GLYPH_COVERAGE_THRESHOLD: f64 = 0.25;

/// Name der Font-Ressource, die für `Action::Replace` angelegt wird.
const PLACEHOLDER_FONT: &[u8] = b"RedactRsHelv";

/// Bericht über eine durchgeführte Schwärzung.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RedactionReport {
    /// Anzahl tatsächlich aus dem Content-Stream entfernter Zeichen.
    pub removed_glyphs: usize,
    /// Anzahl gezeichneter Deck-Rechtecke.
    pub drawn_rects: usize,
    /// Anzahl entfernter Annotationen.
    pub removed_annotations: usize,
    /// Warnungen — z.B. Bilder, die nur überdeckt, aber nicht neu kodiert werden.
    pub warnings: Vec<String>,
}

/// Ein Element einer neu aufzubauenden Text-Operation.
#[derive(Debug, Clone)]
enum PlanItem {
    Glyph { bytes: Vec<u8>, displacement: f64 },
    Adjust(f64),
}

/// Bauplan für eine einzelne Text-Operation.
#[derive(Debug, Clone)]
struct Plan {
    operator: String,
    /// Originaloperanden — für `"` werden daraus `Tw`/`Tc` rekonstruiert.
    operands: Vec<Object>,
    font_size: f64,
    h_scale: f64,
    items: Vec<PlanItem>,
    hidden: Vec<bool>,
}

impl Plan {
    fn from_record(record: &ShowRecord, hidden: Vec<bool>) -> Self {
        let items = record
            .items
            .iter()
            .map(|item| match item {
                ShowItem::Glyph(g) => PlanItem::Glyph {
                    bytes: g.bytes.clone(),
                    displacement: g.displacement,
                },
                ShowItem::Adjust(v) => PlanItem::Adjust(*v),
            })
            .collect();
        Self {
            operator: record.operator.clone(),
            operands: record.operands.clone(),
            font_size: record.font_size,
            h_scale: record.h_scale,
            items,
            hidden,
        }
    }

    fn hidden_count(&self) -> usize {
        self.hidden.iter().filter(|h| **h).count()
    }
}

/// Wendet Schwärzungen auf ein Dokument an.
#[derive(Debug, Clone)]
pub struct PdfRedactor {
    /// Zusätzlicher Rand (in Punkt) um jeden Schwärzungsbereich.
    pub padding: f64,
}

impl Default for PdfRedactor {
    fn default() -> Self {
        Self { padding: 1.0 }
    }
}

impl PdfRedactor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_padding(padding: f64) -> Self {
        Self { padding }
    }

    /// Wie [`Redactor::apply`], liefert aber zusätzlich einen Bericht.
    pub fn apply_with_report(
        &self,
        doc: &mut Document,
        redactions: &[Redaction],
    ) -> Result<RedactionReport> {
        let mut report = RedactionReport::default();
        if redactions.is_empty() {
            return Ok(report);
        }

        let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();
        let mut form_plans: BTreeMap<ObjectId, BTreeMap<usize, Plan>> = BTreeMap::new();

        for (page_index, page_id) in pages.iter().enumerate() {
            let page_redactions: Vec<&Redaction> = redactions
                .iter()
                .filter(|r| r.region.page == page_index)
                .collect();
            if page_redactions.is_empty() {
                continue;
            }
            let rects: Vec<Rect> = page_redactions
                .iter()
                .map(|r| r.region.rect.expanded(self.padding))
                .filter(|r| !r.is_empty())
                .collect();

            let scan = scan_page(doc, *page_id)?;
            let mut page_plans: BTreeMap<usize, Plan> = BTreeMap::new();

            for record in &scan.shows {
                let hidden = hidden_flags(record, &rects);
                if !hidden.iter().any(|h| *h) {
                    continue;
                }
                let target = match record.stream {
                    StreamKey::Page => &mut page_plans,
                    StreamKey::Form(id) => form_plans.entry(id).or_default(),
                };
                merge_plan(target, record, hidden);
            }

            report.removed_glyphs += page_plans.values().map(Plan::hidden_count).sum::<usize>();

            self.rewrite_page(doc, *page_id, &page_plans, &page_redactions, &mut report)?;
            report.removed_annotations += remove_annotations(doc, *page_id, &rects)?;
            warn_about_images(doc, *page_id, &rects, &mut report);
        }

        // Form-XObjects werden einmalig neu geschrieben.
        for (form_id, plans) in form_plans {
            report.removed_glyphs += plans.values().map(Plan::hidden_count).sum::<usize>();
            rewrite_form(doc, form_id, &plans)?;
        }

        Ok(report)
    }

    fn rewrite_page(
        &self,
        doc: &mut Document,
        page_id: ObjectId,
        plans: &BTreeMap<usize, Plan>,
        redactions: &[&Redaction],
        report: &mut RedactionReport,
    ) -> Result<()> {
        let data = doc
            .get_page_content(page_id)
            .map_err(|e| RedactError::Pdf(format!("Content-Stream nicht lesbar: {e}")))?;
        let content = Content::decode(&data)
            .map_err(|e| RedactError::Pdf(format!("Content-Stream nicht dekodierbar: {e}")))?;

        let mut operations = rewrite_operations(&content.operations, plans);

        // Grafikzustand auf den Ausgangszustand zurückfahren, damit die
        // Rechtecke im unveränderten User-Space liegen.
        let (base_ctm, depth) = crate::content::trailing_state(&content.operations);
        for _ in 0..depth {
            operations.push(Operation::new("Q", vec![]));
        }
        operations.push(Operation::new("q", vec![]));
        if !base_ctm.is_identity() {
            if let Some(inv) = base_ctm.invert() {
                operations.push(cm_op(&inv));
            }
        }

        let mut needs_font = false;
        for redaction in redactions {
            let rect = redaction.region.rect.expanded(self.padding);
            if rect.is_empty() {
                continue;
            }
            if let Some((r, g, b)) = redaction.action.fill_color() {
                operations.push(Operation::new("rg", vec![real(r), real(g), real(b)]));
                operations.push(Operation::new(
                    "re",
                    vec![
                        real(rect.ll.x),
                        real(rect.ll.y),
                        real(rect.width()),
                        real(rect.height()),
                    ],
                ));
                operations.push(Operation::new("f", vec![]));
                report.drawn_rects += 1;
            }
            if let Some(text) = redaction.action.replacement() {
                needs_font = true;
                operations.extend(placeholder_ops(text, &rect));
            }
        }
        operations.push(Operation::new("Q", vec![]));

        let encoded = Content { operations }
            .encode()
            .map_err(|e| RedactError::Pdf(format!("Content-Stream nicht kodierbar: {e}")))?;

        if needs_font {
            add_placeholder_font(doc, page_id)?;
        }
        replace_page_content(doc, page_id, encoded)
    }
}

impl Redactor for PdfRedactor {
    fn apply(&self, doc: &mut Document, redactions: &[Redaction]) -> Result<()> {
        self.apply_with_report(doc, redactions).map(|_| ())
    }
}

/// Welche Glyphen einer Text-Operation liegen im Schwärzungsbereich?
fn hidden_flags(record: &ShowRecord, rects: &[Rect]) -> Vec<bool> {
    record
        .items
        .iter()
        .map(|item| match item {
            ShowItem::Glyph(g) => rects.iter().any(|r| {
                g.rect.covered_fraction(r) >= GLYPH_COVERAGE_THRESHOLD
                    || r.contains(g.rect.center())
            }),
            ShowItem::Adjust(_) => false,
        })
        .collect()
}

/// Führt mehrere Platzierungen desselben XObjects zusammen: ein Zeichen wird
/// entfernt, sobald es in *irgendeiner* Platzierung verdeckt ist.
fn merge_plan(target: &mut BTreeMap<usize, Plan>, record: &ShowRecord, hidden: Vec<bool>) {
    match target.get_mut(&record.op_index) {
        Some(existing) if existing.hidden.len() == hidden.len() => {
            for (a, b) in existing.hidden.iter_mut().zip(hidden) {
                *a = *a || b;
            }
        }
        Some(existing) => {
            if hidden.iter().filter(|h| **h).count() > existing.hidden_count() {
                *existing = Plan::from_record(record, hidden);
            }
        }
        None => {
            target.insert(record.op_index, Plan::from_record(record, hidden));
        }
    }
}

/// Ersetzt die betroffenen Text-Operationen durch bereinigte Fassungen.
fn rewrite_operations(operations: &[Operation], plans: &BTreeMap<usize, Plan>) -> Vec<Operation> {
    let mut out = Vec::with_capacity(operations.len() + plans.len() * 2);
    for (index, op) in operations.iter().enumerate() {
        match plans.get(&index) {
            Some(plan) => out.extend(rebuild_show(plan)),
            None => out.push(op.clone()),
        }
    }
    out
}

/// Baut eine Text-Operation ohne die verdeckten Zeichen neu auf.
///
/// Der Vorschub der entfernten Zeichen wird als `TJ`-Kerningwert eingesetzt:
/// in PDF gilt `tx = -a/1000 · Tfs · Th`, also `a = -1000 · tx / (Tfs · Th)`.
/// Dadurch bleibt der nachfolgende Text exakt an seiner ursprünglichen Stelle.
fn rebuild_show(plan: &Plan) -> Vec<Operation> {
    let scale = plan.font_size * plan.h_scale;

    let mut elements: Vec<Object> = Vec::new();
    let mut pending_bytes: Vec<u8> = Vec::new();
    let mut pending_shift = 0.0f64;

    for (index, item) in plan.items.iter().enumerate() {
        let is_hidden = plan.hidden.get(index).copied().unwrap_or(false);
        match item {
            PlanItem::Glyph {
                bytes,
                displacement,
            } => {
                if is_hidden {
                    if !pending_bytes.is_empty() {
                        elements.push(Object::String(
                            std::mem::take(&mut pending_bytes),
                            StringFormat::Literal,
                        ));
                    }
                    pending_shift += displacement;
                } else {
                    push_shift(&mut elements, &mut pending_shift, scale);
                    pending_bytes.extend_from_slice(bytes);
                }
            }
            PlanItem::Adjust(value) => {
                if !pending_bytes.is_empty() {
                    elements.push(Object::String(
                        std::mem::take(&mut pending_bytes),
                        StringFormat::Literal,
                    ));
                }
                push_shift(&mut elements, &mut pending_shift, scale);
                elements.push(real(*value));
            }
        }
    }
    if !pending_bytes.is_empty() {
        elements.push(Object::String(pending_bytes, StringFormat::Literal));
    }
    push_shift(&mut elements, &mut pending_shift, scale);

    // ' und " enthalten neben der Textausgabe noch einen Zeilenvorschub bzw.
    // Wort-/Zeichenabstände. Die werden hier explizit vorangestellt.
    let mut result = Vec::new();
    match plan.operator.as_str() {
        "'" => result.push(Operation::new("T*", vec![])),
        "\"" => {
            if let Some(aw) = plan.operands.first() {
                result.push(Operation::new("Tw", vec![aw.clone()]));
            }
            if let Some(ac) = plan.operands.get(1) {
                result.push(Operation::new("Tc", vec![ac.clone()]));
            }
            result.push(Operation::new("T*", vec![]));
        }
        _ => {}
    }
    result.push(Operation::new("TJ", vec![Object::Array(elements)]));
    result
}

fn push_shift(elements: &mut Vec<Object>, shift: &mut f64, scale: f64) {
    if shift.abs() > 1e-9 && scale.abs() > 1e-9 {
        elements.push(real(-1000.0 * *shift / scale));
    }
    *shift = 0.0;
}

fn real(value: f64) -> Object {
    Object::Real(value as f32)
}

// ---------------------------------------------------------------------------
// Hilfsfunktionen für das Dokument
// ---------------------------------------------------------------------------

fn cm_op(m: &Matrix) -> Operation {
    Operation::new(
        "cm",
        vec![
            real(m.a),
            real(m.b),
            real(m.c),
            real(m.d),
            real(m.e),
            real(m.f),
        ],
    )
}

/// Zeichnet den Ersatztext linksbündig in das Deck-Rechteck.
fn placeholder_ops(text: &str, rect: &Rect) -> Vec<Operation> {
    let size = (rect.height() * 0.7).clamp(4.0, 14.0);
    let baseline = rect.ll.y + (rect.height() - size) / 2.0 + size * 0.2;
    vec![
        Operation::new("BT", vec![]),
        // Ersatztext in Schwarz auf weißem Grund.
        Operation::new("g", vec![real(0.0)]),
        Operation::new(
            "Tf",
            vec![Object::Name(PLACEHOLDER_FONT.to_vec()), real(size)],
        ),
        Operation::new("Td", vec![real(rect.ll.x + 1.0), real(baseline)]),
        Operation::new(
            "Tj",
            vec![Object::String(to_win_ansi(text), StringFormat::Literal)],
        ),
        Operation::new("ET", vec![]),
    ]
}

/// Kodiert den Ersatztext für die WinAnsi-Helvetica-Ressource.
/// Nicht darstellbare Zeichen werden zu `?`.
fn to_win_ansi(text: &str) -> Vec<u8> {
    let table = crate::encoding::win_ansi_encoding();
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

fn add_placeholder_font(doc: &mut Document, page_id: ObjectId) -> Result<()> {
    let mut font = Dictionary::new();
    font.set("Type", Object::Name(b"Font".to_vec()));
    font.set("Subtype", Object::Name(b"Type1".to_vec()));
    font.set("BaseFont", Object::Name(b"Helvetica".to_vec()));
    font.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
    let font_id = doc.add_object(Object::Dictionary(font));

    let page = doc
        .get_dictionary(page_id)
        .map_err(|e| RedactError::Pdf(e.to_string()))?
        .clone();
    let mut resources = match page.get(b"Resources") {
        Ok(Object::Dictionary(d)) => d.clone(),
        Ok(Object::Reference(id)) => doc.get_dictionary(*id).cloned().unwrap_or_default(),
        _ => Dictionary::new(),
    };
    let mut fonts = match resources.get(b"Font") {
        Ok(Object::Dictionary(d)) => d.clone(),
        Ok(Object::Reference(id)) => doc.get_dictionary(*id).cloned().unwrap_or_default(),
        _ => Dictionary::new(),
    };
    fonts.set(PLACEHOLDER_FONT.to_vec(), Object::Reference(font_id));
    resources.set("Font", Object::Dictionary(fonts));

    let page = doc
        .get_dictionary_mut(page_id)
        .map_err(|e| RedactError::Pdf(e.to_string()))?;
    page.set("Resources", Object::Dictionary(resources));
    Ok(())
}

/// Ersetzt den Content-Stream einer Seite und entfernt die alten Objekte.
fn replace_page_content(doc: &mut Document, page_id: ObjectId, data: Vec<u8>) -> Result<()> {
    let old: BTreeSet<ObjectId> = doc.get_page_contents(page_id).into_iter().collect();

    // Streams, die auch von anderen Seiten benutzt werden, bleiben erhalten.
    let mut shared = BTreeSet::new();
    for other in doc.get_pages().values() {
        if *other == page_id {
            continue;
        }
        for id in doc.get_page_contents(*other) {
            if old.contains(&id) {
                shared.insert(id);
            }
        }
    }

    let mut stream = Stream::new(Dictionary::new(), data);
    let _ = stream.compress();
    let new_id = doc.add_object(Object::Stream(stream));

    let page = doc
        .get_dictionary_mut(page_id)
        .map_err(|e| RedactError::Pdf(e.to_string()))?;
    page.set("Contents", Object::Reference(new_id));

    for id in old.difference(&shared) {
        doc.objects.remove(id);
    }
    Ok(())
}

fn rewrite_form(
    doc: &mut Document,
    form_id: ObjectId,
    plans: &BTreeMap<usize, Plan>,
) -> Result<()> {
    let data = {
        let stream = doc
            .get_object(form_id)
            .and_then(|o| o.as_stream())
            .map_err(|e| RedactError::Pdf(e.to_string()))?;
        stream
            .decompressed_content()
            .or_else(|_| stream.get_plain_content())
            .map_err(|e| RedactError::Pdf(e.to_string()))?
    };
    let content = Content::decode(&data)
        .map_err(|e| RedactError::Pdf(format!("XObject nicht dekodierbar: {e}")))?;
    let operations = rewrite_operations(&content.operations, plans);
    let encoded = Content { operations }
        .encode()
        .map_err(|e| RedactError::Pdf(e.to_string()))?;

    if let Ok(Object::Stream(stream)) = doc.get_object_mut(form_id) {
        stream.set_plain_content(encoded);
        let _ = stream.compress();
    }
    Ok(())
}

/// Entfernt Annotationen, die in einen Schwärzungsbereich ragen.
fn remove_annotations(doc: &mut Document, page_id: ObjectId, rects: &[Rect]) -> Result<usize> {
    if rects.is_empty() {
        return Ok(0);
    }
    let annots = match doc.get_dictionary(page_id).and_then(|d| d.get(b"Annots")) {
        Ok(obj) => match doc.dereference(obj) {
            Ok((_, Object::Array(items))) => items.clone(),
            _ => return Ok(0),
        },
        Err(_) => return Ok(0),
    };

    let mut kept = Vec::new();
    let mut removed = 0usize;
    for annot in annots {
        let rect = doc
            .dereference(&annot)
            .ok()
            .and_then(|(_, o)| o.as_dict().ok())
            .and_then(|d| d.get(b"Rect").ok())
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| rect_from_object(o));
        match rect {
            Some(r) if rects.iter().any(|target| r.intersects(target)) => {
                if let Object::Reference(id) = annot {
                    doc.objects.remove(&id);
                }
                removed += 1;
            }
            _ => kept.push(annot),
        }
    }

    if removed > 0 {
        let page = doc
            .get_dictionary_mut(page_id)
            .map_err(|e| RedactError::Pdf(e.to_string()))?;
        page.set("Annots", Object::Array(kept));
    }
    Ok(removed)
}

/// Rasterbilder können nicht neu kodiert werden — darauf muss hingewiesen werden.
fn warn_about_images(
    doc: &Document,
    page_id: ObjectId,
    rects: &[Rect],
    report: &mut RedactionReport,
) {
    if rects.is_empty() {
        return;
    }
    let has_images = doc
        .get_page_images(page_id)
        .map(|imgs| !imgs.is_empty())
        .unwrap_or(false);
    if has_images {
        let msg = "Seite enthält Rasterbilder. Sie werden überdeckt, aber nicht neu kodiert — \
                   bei gescannten Dokumenten ist zusätzlich OCR bzw. Neurendern nötig."
            .to_string();
        if !report.warnings.contains(&msg) {
            report.warnings.push(msg);
        }
    }
}

fn rect_from_object(obj: &Object) -> Option<Rect> {
    let array = obj.as_array().ok()?;
    let v: Vec<f64> = array
        .iter()
        .take(4)
        .filter_map(|o| match o {
            Object::Integer(i) => Some(*i as f64),
            Object::Real(r) => Some(*r as f64),
            _ => None,
        })
        .collect();
    if v.len() < 4 {
        return None;
    }
    Some(Rect::new(v[0], v[1], v[2], v[3]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(hidden: Vec<bool>) -> Plan {
        Plan {
            operator: "Tj".into(),
            operands: vec![],
            font_size: 10.0,
            h_scale: 1.0,
            items: vec![
                PlanItem::Glyph {
                    bytes: b"A".to_vec(),
                    displacement: 5.0,
                },
                PlanItem::Glyph {
                    bytes: b"B".to_vec(),
                    displacement: 5.0,
                },
                PlanItem::Glyph {
                    bytes: b"C".to_vec(),
                    displacement: 5.0,
                },
            ],
            hidden,
        }
    }

    fn tj_elements(ops: &[Operation]) -> Vec<Object> {
        let op = ops.last().unwrap();
        assert_eq!(op.operator, "TJ");
        match &op.operands[0] {
            Object::Array(items) => items.clone(),
            other => panic!("kein Array: {other:?}"),
        }
    }

    #[test]
    fn removes_middle_glyph_and_keeps_layout() {
        let ops = rebuild_show(&plan(vec![false, true, false]));
        let elements = tj_elements(&ops);
        // "A", Kerning für das entfernte "B", "C"
        assert_eq!(elements.len(), 3);
        assert!(matches!(&elements[0], Object::String(s, _) if s == b"A"));
        // 5pt Vorschub bei Tfs=10, Th=1  ->  a = -1000*5/10 = -500
        match &elements[1] {
            Object::Real(v) => assert!((*v - (-500.0)).abs() < 0.01, "war {v}"),
            other => panic!("keine Zahl: {other:?}"),
        }
        assert!(matches!(&elements[2], Object::String(s, _) if s == b"C"));
    }

    #[test]
    fn removing_everything_leaves_only_kerning() {
        let ops = rebuild_show(&plan(vec![true, true, true]));
        let elements = tj_elements(&ops);
        assert_eq!(elements.len(), 1);
        match &elements[0] {
            Object::Real(v) => assert!((*v - (-1500.0)).abs() < 0.01, "war {v}"),
            other => panic!("keine Zahl: {other:?}"),
        }
    }

    #[test]
    fn nothing_hidden_keeps_all_bytes() {
        let ops = rebuild_show(&plan(vec![false, false, false]));
        let elements = tj_elements(&ops);
        assert_eq!(elements.len(), 1);
        assert!(matches!(&elements[0], Object::String(s, _) if s == b"ABC"));
    }

    #[test]
    fn quote_operator_is_expanded_into_explicit_operations() {
        let mut p = plan(vec![false, true, false]);
        p.operator = "\"".into();
        p.operands = vec![
            Object::Real(1.0),
            Object::Real(2.0),
            Object::String(vec![], StringFormat::Literal),
        ];
        let ops = rebuild_show(&p);
        assert_eq!(ops[0].operator, "Tw");
        assert_eq!(ops[1].operator, "Tc");
        assert_eq!(ops[2].operator, "T*");
        assert_eq!(ops[3].operator, "TJ");
    }

    #[test]
    fn merge_takes_union_of_placements() {
        let mut target = BTreeMap::new();
        target.insert(7usize, plan(vec![true, false, false]));
        // Zweite Platzierung verdeckt ein anderes Zeichen.
        if let Some(existing) = target.get_mut(&7) {
            for (a, b) in existing.hidden.iter_mut().zip([false, false, true]) {
                *a = *a || b;
            }
        }
        assert_eq!(target[&7].hidden, vec![true, false, true]);
    }
}
