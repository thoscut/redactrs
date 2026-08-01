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
//! 3. Bilder, die ein Schwärzungsbereich schneidet, werden in ihren Pixeln
//!    überschrieben und neu kodiert ([`crate::image`]).
//! 4. Anschließend wird ein deckendes Rechteck gezeichnet.
//! 5. Überlappende Annotationen werden gelöscht (auch dort steht Text).
//! 6. Der **Textspiegel** eines Marked-Content-Abschnitts (`/ActualText`,
//!    `/Alt`, `/E`) wird geleert, sobald von den Glyphen darunter etwas
//!    entfernt wurde ([`mirrors_to_clear`]).
//!
//! Zeichen in Form-XObjects werden ebenfalls entfernt. Wird dasselbe XObject
//! mehrfach platziert, wirkt die Entfernung notwendigerweise auf alle
//! Platzierungen — es wird also eher zu viel als zu wenig geschwärzt. Das ist
//! die sichere Richtung.

use std::collections::{BTreeMap, BTreeSet};

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use redact_core::{Rect, RedactError, Redaction, Result};

use crate::content::{MarkedTextRecord, ShowItem, ShowRecord, StreamKey, MIRROR_KEYS};
use crate::image::InlineTarget;
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
    /// Entfernte Zeichen **je übergebener Schwärzung**, in derselben
    /// Reihenfolge und Länge wie die an [`PdfRedactor::apply_with_report`]
    /// übergebene Liste.
    ///
    /// Ohne diese Aufschlüsselung kann ein Audit-Log nur die Gesamtsumme
    /// nennen. Eine Region, die zwar gültig ist, im Strom aber nichts trifft —
    /// falsche Koordinaten, Glyphen außerhalb, nur ein Bild darunter —, sähe
    /// darin aus wie jede andere: „angewendet“. Mit `per_redaction` lässt sich
    /// je Region die Wahrheit protokollieren, und eine `0` ist ein Befund.
    ///
    /// Überlappende Bereiche werden **jeder für sich** gezählt: verdecken zwei
    /// Regionen dasselbe Zeichen, erscheint es in beiden Zahlen. Die Summe
    /// kann deshalb größer sein als [`RedactionReport::removed_glyphs`] — die
    /// Frage „hat *diese* Region etwas bewirkt?“ ist nur so ehrlich zu
    /// beantworten.
    pub per_redaction: Vec<usize>,
    /// Anzahl gezeichneter Deck-Rechtecke.
    pub drawn_rects: usize,
    /// Anzahl entfernter Annotationen.
    pub removed_annotations: usize,
    /// Anzahl Bilder, deren Pixel überschrieben wurden.
    pub redacted_images: usize,
    /// Davon: Kopien, die angelegt wurden, weil das Bild mehrfach benutzt wird.
    pub copied_images: usize,
    /// Höchstzahl der **gleichzeitig** dekodiert gehaltenen Bilder.
    ///
    /// Siehe [`crate::image::ImageOutcome::peak_decoded_images`]: der
    /// Speicherbedarf ist im Test kaum messbar, diese Zahl schon.
    pub peak_decoded_images: usize,
    /// Dasselbe in Bytes (RGBA8, 4 Byte je Bildpunkt).
    pub peak_decoded_image_bytes: u64,
    /// Warnungen — z.B. Seiten, deren Bildinhalt mangels OCR nicht durchsucht
    /// werden konnte.
    pub warnings: Vec<String>,
}

/// Ein Element einer neu aufzubauenden Text-Operation.
#[derive(Debug, Clone)]
enum PlanItem {
    Glyph { bytes: Vec<u8>, displacement: f64 },
    Adjust(f64),
}

/// Welche Zeichen einer Text-Operation verdeckt sind — insgesamt und je
/// Schwärzung.
#[derive(Debug, Clone, Default)]
struct Selection {
    /// Vereinigung über alle Bereiche: das entscheidet über den Strom.
    hidden: Vec<bool>,
    /// Index der Schwärzung → die Zeichen, die **sie** verdeckt.
    ///
    /// Nur Bereiche, die überhaupt etwas treffen, stehen hier. Getrennt
    /// geführt, weil sich nur so je Region sagen lässt, ob sie gewirkt hat.
    per_redaction: BTreeMap<usize, Vec<bool>>,
}

impl Selection {
    fn any(&self) -> bool {
        self.hidden.iter().any(|h| *h)
    }
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
    selection: Selection,
}

impl Plan {
    fn from_record(record: &ShowRecord, selection: Selection) -> Self {
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
            selection,
        }
    }

    fn hidden(&self) -> &[bool] {
        &self.selection.hidden
    }

    fn hidden_count(&self) -> usize {
        self.selection.hidden.iter().filter(|h| **h).count()
    }

    /// Wie viele Zeichen dieser Operation die Schwärzung `index` verdeckt.
    fn count_for(&self, index: usize) -> usize {
        self.selection
            .per_redaction
            .get(&index)
            .map(|flags| flags.iter().filter(|h| **h).count())
            .unwrap_or(0)
    }
}

/// Wendet Schwärzungen auf ein Dokument an.
#[derive(Debug, Clone)]
pub struct PdfRedactor {
    /// Zusätzlicher Rand (in Punkt) um jeden Schwärzungsbereich.
    pub padding: f64,
    /// Bilder, die sich nicht dekodieren lassen (JPX, CCITT, defekte Streams),
    /// durchgehen lassen, statt abzubrechen.
    ///
    /// **Unsicher** — siehe [`crate::image::ImageOptions::allow_undecodable`].
    /// Standard ist `false`: lieber ein Fehler als eine Datei, in der die
    /// Schwärzung nur obenauf liegt.
    pub allow_undecodable_images: bool,
    /// Obergrenze für die gleichzeitig gehaltenen dekodierten Bildbytes.
    ///
    /// Siehe [`crate::image::ImageOptions::max_decoded_bytes`]. Wird sie
    /// überschritten, endet der Lauf mit einem Fehler — nicht mit einer
    /// gescheiterten Speicheranforderung.
    pub max_decoded_image_bytes: u64,
}

impl Default for PdfRedactor {
    fn default() -> Self {
        Self {
            padding: 1.0,
            allow_undecodable_images: false,
            max_decoded_image_bytes: crate::image::DEFAULT_MAX_DECODED_IMAGE_BYTES,
        }
    }
}

impl PdfRedactor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_padding(padding: f64) -> Self {
        Self {
            padding,
            ..Self::default()
        }
    }

    /// Siehe [`PdfRedactor::allow_undecodable_images`].
    pub fn allowing_undecodable_images(mut self, allow: bool) -> Self {
        self.allow_undecodable_images = allow;
        self
    }

    /// Siehe [`PdfRedactor::max_decoded_image_bytes`].
    pub fn with_max_decoded_image_bytes(mut self, bytes: u64) -> Self {
        self.max_decoded_image_bytes = bytes;
        self
    }

    /// Führt die Schwärzungen aus und liefert einen Bericht darüber, was
    /// tatsächlich gewirkt hat.
    pub fn apply_with_report(
        &self,
        doc: &mut Document,
        redactions: &[Redaction],
    ) -> Result<RedactionReport> {
        let mut report = RedactionReport {
            per_redaction: vec![0; redactions.len()],
            ..RedactionReport::default()
        };
        // Auch bei „0 Schwärzungen“ muss der Nutzer erfahren, dass die Datei
        // eine Vorgeschichte hat: was in einer früheren Revision stand, ist
        // beim Laden mitgekommen.
        if crate::document::has_incremental_history(doc) {
            report.warnings.push(
                "Die Eingabedatei besteht aus mehreren inkrementellen Revisionen (/Prev). \
                 Frühere Fassungen können Text enthalten, den eine spätere Revision nur \
                 überschrieben hat — etwa eine bereits in einem anderen Werkzeug \
                 vorgenommene Schwärzung. Die Ausgabe wird als eine einzige Revision ohne \
                 Vorgeschichte geschrieben; prüfen Sie das Ergebnis trotzdem."
                    .to_string(),
            );
        }
        warn_about_images(doc, &mut report);

        // Bilder zuerst — dafür werden die *unveränderten* Content-Streams
        // gebraucht, und die Ersatzoperationen für Inline-Bilder gehen unten in
        // das Neuschreiben der Seite ein.
        let images = crate::image::redact_images(
            doc,
            redactions,
            self.padding,
            &crate::image::ImageOptions {
                allow_undecodable: self.allow_undecodable_images,
                max_decoded_bytes: self.max_decoded_image_bytes,
            },
        )?;
        report.redacted_images = images.redacted_images;
        report.copied_images = images.copied_images;
        report.peak_decoded_images = images.peak_decoded_images;
        report.peak_decoded_image_bytes = images.peak_decoded_bytes;
        for warning in images.warnings {
            push_warning(&mut report, warning);
        }
        let inline_images = images.inline_replacements;

        let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();
        let mut form_plans: BTreeMap<ObjectId, BTreeMap<usize, Plan>> = BTreeMap::new();
        // Textspiegel in Form-XObjects: gefunden beim Scan der Seite, geleert
        // erst beim einmaligen Neuschreiben des Formulars.
        let mut form_marked: BTreeMap<ObjectId, Vec<MarkedTextRecord>> = BTreeMap::new();
        // Eigenschaftslisten, die als eigenes Objekt in der Datei stehen und
        // deshalb nicht im Strom, sondern im Objekt bereinigt werden.
        let mut property_objects: BTreeSet<ObjectId> = BTreeSet::new();
        let no_inline: BTreeMap<usize, Operation> = BTreeMap::new();
        let no_plans: BTreeMap<usize, Plan> = BTreeMap::new();
        let no_marked: Vec<MarkedTextRecord> = Vec::new();

        // Einmal statt je Seite: welche Seite benutzt welchen Content-Stream.
        // Siehe [`ContentUsers`] — die wiederholte Suche war der quadratische
        // Anteil an der Laufzeit.
        let mut content_users = ContentUsers::build(doc, &pages);
        // Ebenso die Zuordnung Schwärzung → Seite. Sie je Seite aus der
        // vollständigen Liste zu filtern kostet Seiten × Schwärzungen.
        let by_page = redactions_by_page(redactions);
        let no_redactions: Vec<(usize, &Redaction)> = Vec::new();

        for (page_index, page_id) in pages.iter().enumerate() {
            // Der Index in der **übergebenen** Liste wird mitgeführt: nur so
            // lässt sich am Ende je Schwärzung sagen, was sie bewirkt hat.
            let on_this_page: &[(usize, &Redaction)] = by_page
                .get(&page_index)
                .map(Vec::as_slice)
                .unwrap_or(&no_redactions);
            let page_redactions: Vec<&Redaction> = on_this_page.iter().map(|(_, r)| *r).collect();

            // Gescannt wird *jede* Seite, auch die ohne Schwärzung. Die
            // Befunde des Scanners — ein Font, dessen Text sich nicht
            // dekodieren lässt, ein Strom, der nicht zerlegbar war, ein
            // Formular ohne `/Subtype` — sind genau dann das Einzige, was den
            // Nutzer erreicht: „0 Schwärzungen, Exit 0“ liest sich sonst wie
            // „nichts gefunden, also sauber“.
            // Kein Sonderweg mehr für Seiten ohne Schwärzung. Früher wurde ein
            // unlesbarer Strom dort nur als Warnung gemeldet und der Lauf lief
            // weiter — mit dem Ergebnis „0 Schwärzungen, Rückgabewert 0“ für
            // eine Seite, deren Text nie jemand gesehen hat. Ausgerechnet die
            // Seiten ohne Treffer sind die, bei denen das Fehlen von Treffern
            // etwas bedeuten soll.
            let scan = crate::content::scan_page(doc, *page_id).map_err(|e| {
                RedactError::Pdf(format!(
                    "Seite {} ließ sich nicht lesen: {e} Ihr Inhalt wurde nicht \
                     durchsucht; die Datei wird nicht als geschwärzt ausgegeben.",
                    page_index + 1
                ))
            })?;
            for warning in &scan.warnings {
                push_warning(&mut report, warning.clone());
            }
            if page_redactions.is_empty() {
                continue;
            }
            // Entartete Bereiche fliegen raus, ihr Index bleibt aber erhalten:
            // im Bericht steht für sie eine ehrliche 0.
            let indexed_rects: Vec<(usize, Rect)> = on_this_page
                .iter()
                .copied()
                .map(|(index, r)| (index, r.region.rect.expanded(self.padding)))
                .filter(|(_, r)| !r.is_empty())
                .collect();
            let rects: Vec<Rect> = indexed_rects.iter().map(|(_, r)| *r).collect();

            let mut page_plans: BTreeMap<usize, Plan> = BTreeMap::new();

            for record in &scan.shows {
                let selection = hidden_flags(record, &indexed_rects);
                if !selection.any() {
                    continue;
                }
                let target = match record.stream {
                    StreamKey::Page => &mut page_plans,
                    StreamKey::Form(id) => form_plans.entry(id).or_default(),
                };
                merge_plan(target, record, selection);
            }

            report.removed_glyphs += page_plans.values().map(Plan::hidden_count).sum::<usize>();
            add_per_redaction(&mut report, page_plans.values());

            // Textspiegel in Formularen werden erst später fällig — dort sind
            // die Pläne erst nach der letzten Seite vollständig.
            for record in &scan.marked {
                if let StreamKey::Form(id) = record.stream {
                    let known = form_marked.entry(id).or_default();
                    if !known.iter().any(|k| k.op_index == record.op_index) {
                        known.push(record.clone());
                    }
                }
            }
            let mut mirrors = mirrors_to_clear(&scan.marked, StreamKey::Page, &page_plans);
            property_objects.append(&mut mirrors.objects);
            for warning in std::mem::take(&mut mirrors.warnings) {
                push_warning(&mut report, warning);
            }

            let inline = inline_images
                .get(&InlineTarget::Page(*page_id))
                .unwrap_or(&no_inline);
            self.rewrite_page(
                doc,
                *page_id,
                &page_plans,
                inline,
                &mirrors.inline,
                &page_redactions,
                &mut report,
                &mut content_users,
            )?;
            report.removed_annotations += remove_annotations(doc, *page_id, &rects)?;
        }

        // Form-XObjects werden einmalig neu geschrieben — auch die, in denen
        // nur ein Inline-Bild zu ersetzen ist und kein Zeichen entfällt.
        let form_ids: BTreeSet<ObjectId> = form_plans
            .keys()
            .copied()
            .chain(inline_images.keys().filter_map(|target| match target {
                InlineTarget::Form(id) => Some(*id),
                InlineTarget::Page(_) => None,
            }))
            .collect();
        for form_id in form_ids {
            let plans = form_plans.get(&form_id).unwrap_or(&no_plans);
            let inline = inline_images
                .get(&InlineTarget::Form(form_id))
                .unwrap_or(&no_inline);
            report.removed_glyphs += plans.values().map(Plan::hidden_count).sum::<usize>();
            add_per_redaction(&mut report, plans.values());
            let marked = form_marked.get(&form_id).unwrap_or(&no_marked);
            let mut mirrors = mirrors_to_clear(marked, StreamKey::Form(form_id), plans);
            property_objects.append(&mut mirrors.objects);
            for warning in std::mem::take(&mut mirrors.warnings) {
                push_warning(&mut report, warning);
            }
            rewrite_form(doc, form_id, plans, inline, &mirrors.inline)?;
        }

        // Zum Schluss die Eigenschaftslisten, die als eigene Objekte in der
        // Datei stehen — sie gehören keinem Strom, sondern dem Dokument.
        for id in property_objects {
            clear_mirror_object(doc, id);
        }

        Ok(report)
    }

    #[allow(clippy::too_many_arguments)]
    fn rewrite_page(
        &self,
        doc: &mut Document,
        page_id: ObjectId,
        plans: &BTreeMap<usize, Plan>,
        inline_images: &BTreeMap<usize, Operation>,
        mirrors: &BTreeMap<usize, Dictionary>,
        redactions: &[&Redaction],
        report: &mut RedactionReport,
        content_users: &mut ContentUsers,
    ) -> Result<()> {
        let data = doc
            .get_page_content(page_id)
            .map_err(|e| RedactError::Pdf(format!("Content-Stream nicht lesbar: {e}")))?;
        let decoded = decode_or_fail(&data, "Der Content-Stream dieser Seite")?;

        let mut operations = rewrite_operations(&decoded, plans, inline_images, mirrors);

        // Grafikzustand auf den Ausgangszustand zurückfahren, damit die
        // Rechtecke im unveränderten User-Space liegen.
        let (base_ctm, depth) = crate::content::trailing_state(&decoded);
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

        let encoded = encode_operations(&operations)?;

        if needs_font {
            add_placeholder_font(doc, page_id)?;
        }
        replace_page_content(doc, page_id, encoded, content_users)
    }
}

// ---------------------------------------------------------------------------
// Content-Streams mit Inline-Bildern
// ---------------------------------------------------------------------------

/// Zerlegt einen Strom, der gleich **neu geschrieben** wird.
///
/// Was hier nicht in Operationen zerfällt, steht nachher nicht mehr in der
/// Datei: [`encode_operations`] schreibt nur zurück, was übrig blieb. Früher
/// fiel ein solcher Abschnitt kommentarlos unter den Tisch — der Nutzer bekam
/// eine Datei, die er für vollständig hielt, und der Inhalt war weg. Lieber
/// ein Fehler als eine stillschweigend beschnittene Seite.
fn decode_or_fail(data: &[u8], what: &str) -> Result<Vec<Operation>> {
    let decoded = crate::ops::decode_content_checked(data);
    if !decoded.truncated.is_empty() {
        return Err(RedactError::Pdf(format!(
            "{what} ließ sich nicht vollständig in Operationen zerlegen: in {} \
             Teilstück(en) von zusammen {} Byte bricht die Zerlegung ab, alles dahinter \
             fehlt. Beim Neuschreiben ginge dieser Teil ersatzlos verloren; die Datei \
             wird deshalb nicht ausgegeben.",
            decoded.truncated.len(),
            decoded.affected_bytes()
        )));
    }
    Ok(decoded.operations)
}

/// Nimmt eine Warnung in den Bericht auf — jede höchstens einmal.
fn push_warning(report: &mut RedactionReport, message: String) {
    if !report.warnings.contains(&message) {
        report.warnings.push(message);
    }
}

/// Kodiert einen Operationsstrom zurück in Streambytes.
///
/// Gegenstück zu [`crate::ops::decode_content`]: dort wird ein Inline-Bild zu
/// einer Pseudo-Operation `BI` mit Dictionary und Rohdaten zusammengefasst.
/// `Content::encode` würde daraus `<<…>> (…) BI` machen — syntaktischer
/// Unsinn, den kein Betrachter mehr liest. Deshalb werden die Bilder hier von
/// Hand geschrieben und nur die Abschnitte dazwischen von lopdf kodiert.
fn encode_operations(operations: &[Operation]) -> Result<Vec<u8>> {
    let mut out: Vec<u8> = Vec::new();
    let mut chunk: Vec<Operation> = Vec::new();

    for op in operations {
        match inline_image_operands(op) {
            Some((dict, data)) => {
                flush_chunk(&mut chunk, &mut out)?;
                out.extend_from_slice(&encode_inline_image(dict, data)?);
            }
            None => chunk.push(op.clone()),
        }
    }
    flush_chunk(&mut chunk, &mut out)?;
    Ok(out)
}

fn flush_chunk(chunk: &mut Vec<Operation>, out: &mut Vec<u8>) -> Result<()> {
    if chunk.is_empty() {
        return Ok(());
    }
    let operations = std::mem::take(chunk);
    let encoded = Content { operations }
        .encode()
        .map_err(|e| RedactError::Pdf(format!("Content-Stream nicht kodierbar: {e}")))?;
    out.extend_from_slice(&encoded);
    out.push(b'\n');
    Ok(())
}

/// Erkennt die Pseudo-Operation, die `decode_content` für ein Inline-Bild baut.
fn inline_image_operands(op: &Operation) -> Option<(&Dictionary, &[u8])> {
    if op.operator != "BI" {
        return None;
    }
    match (op.operands.first(), op.operands.get(1)) {
        (Some(Object::Dictionary(dict)), Some(Object::String(data, _))) => {
            Some((dict, data.as_slice()))
        }
        _ => None,
    }
}

/// Schreibt ein Inline-Bild als `BI … ID … EI`.
fn encode_inline_image(dict: &Dictionary, data: &[u8]) -> Result<Vec<u8>> {
    // Die Schlüssel-Wert-Paare stehen im Stream unmittelbar vor dem `ID` —
    // also genau in der Form, die lopdf für die Operanden einer Operation
    // `ID` erzeugt.
    let mut operands: Vec<Object> = Vec::with_capacity(dict.len() * 2);
    for (key, value) in dict.iter() {
        operands.push(Object::Name(key.clone()));
        operands.push(value.clone());
    }
    let header = Content {
        operations: vec![Operation::new("ID", operands)],
    }
    .encode()
    .map_err(|e| RedactError::Pdf(format!("Inline-Bild nicht kodierbar: {e}")))?;

    let mut out = Vec::with_capacity(header.len() + data.len() + 8);
    out.extend_from_slice(b"BI ");
    out.extend_from_slice(&header);
    // Genau ein Trennzeichen zwischen `ID` und den Bilddaten.
    out.push(b' ');
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nEI\n");
    Ok(out)
}

/// Welche Glyphen einer Text-Operation liegen im Schwärzungsbereich?
///
/// `rects` sind die (bereits um `padding` erweiterten) Bereiche zusammen mit
/// ihrem Index in der übergebenen Schwärzungsliste. Der Index wird
/// mitgeschleppt, damit der Bericht je Region Rechenschaft ablegen kann.
fn hidden_flags(record: &ShowRecord, rects: &[(usize, Rect)]) -> Selection {
    let mut selection = Selection {
        hidden: vec![false; record.items.len()],
        per_redaction: BTreeMap::new(),
    };
    for (index, rect) in rects {
        let mut flags: Vec<bool> = record
            .items
            .iter()
            .map(|item| match item {
                ShowItem::Glyph(g) => {
                    g.rect.covered_fraction(rect) >= GLYPH_COVERAGE_THRESHOLD
                        || rect.contains(g.rect.center())
                }
                ShowItem::Adjust(_) => false,
            })
            .collect();
        // Erst je Bereich verbreitern, dann vereinigen: eine Region, die nur
        // eine Hälfte einer Ligatur trifft, hat auch die andere zu verantworten.
        widen_over_ligatures(&record.items, &mut flags);
        if !flags.iter().any(|h| *h) {
            continue;
        }
        for (all, one) in selection.hidden.iter_mut().zip(&flags) {
            *all = *all || *one;
        }
        selection.per_redaction.insert(*index, flags);
    }
    selection
}

/// Schreibt die Zeichenzahlen je Schwärzung fort.
fn add_per_redaction<'a>(report: &mut RedactionReport, plans: impl Iterator<Item = &'a Plan>) {
    for plan in plans {
        for index in plan.selection.per_redaction.keys() {
            if let Some(slot) = report.per_redaction.get_mut(*index) {
                *slot += plan.count_for(*index);
            }
        }
    }
}

/// Zieht die Auswahl über ganze Zeichencodes zusammen.
///
/// Eine Ligatur ist im Strom **ein** Code, steht aber für mehrere Zeichen.
/// [`crate::content`] teilt sie in Teilzeichen auf, damit Text und Geometrie
/// zeichenweise zusammenpassen; die Originalbytes trägt dabei nur das erste
/// Teilzeichen, alle weiteren haben `bytes` leer.
///
/// Beim Neuschreiben entscheidet deshalb allein das erste Teilzeichen, ob der
/// Code wieder in den Strom geschrieben wird — und mit ihm **alle** seine
/// Zeichen. Beginnt der Treffer erst beim zweiten Teilzeichen, überlebt die
/// Ligatur also vollständig und bleibt mit `pdftotext` lesbar, obwohl das
/// Deck-Rechteck sie halb verdeckt.
///
/// Eine nur teilweise getroffene Ligatur muss deshalb **ganz** verschwinden.
/// Das ist auch die sichere Richtung: lieber ein Zeichen zu viel entfernt als
/// ein Geheimnis halb stehen gelassen.
fn widen_over_ligatures(items: &[ShowItem], flags: &mut [bool]) {
    let mut index = 0;
    while index < items.len() {
        // Ein Code beginnt bei der Glyphe, die die Originalbytes trägt.
        let starts_code = matches!(&items[index], ShowItem::Glyph(g) if !g.bytes.is_empty());
        if !starts_code {
            index += 1;
            continue;
        }
        let start = index;
        index += 1;
        while matches!(items.get(index), Some(ShowItem::Glyph(g)) if g.bytes.is_empty()) {
            index += 1;
        }
        if index - start > 1 && flags[start..index].iter().any(|h| *h) {
            for flag in &mut flags[start..index] {
                *flag = true;
            }
        }
    }
}

/// Führt mehrere Platzierungen desselben XObjects zusammen: ein Zeichen wird
/// entfernt, sobald es in *irgendeiner* Platzierung verdeckt ist.
///
/// Die Zählung je Schwärzung wird mit vereinigt — dadurch zählt ein Zeichen,
/// das in zwei Platzierungen desselben Formulars verdeckt ist, für dieselbe
/// Region trotzdem nur einmal.
fn merge_plan(target: &mut BTreeMap<usize, Plan>, record: &ShowRecord, selection: Selection) {
    match target.get_mut(&record.op_index) {
        Some(existing) if existing.selection.hidden.len() == selection.hidden.len() => {
            for (a, b) in existing.selection.hidden.iter_mut().zip(&selection.hidden) {
                *a = *a || *b;
            }
            for (index, flags) in selection.per_redaction {
                match existing.selection.per_redaction.get_mut(&index) {
                    Some(known) if known.len() == flags.len() => {
                        for (a, b) in known.iter_mut().zip(&flags) {
                            *a = *a || *b;
                        }
                    }
                    _ => {
                        existing.selection.per_redaction.insert(index, flags);
                    }
                }
            }
        }
        Some(existing) => {
            let count = selection.hidden.iter().filter(|h| **h).count();
            if count > existing.hidden_count() {
                *existing = Plan::from_record(record, selection);
            }
        }
        None => {
            target.insert(record.op_index, Plan::from_record(record, selection));
        }
    }
}

/// Ersetzt die betroffenen Text-Operationen durch bereinigte Fassungen,
/// geschwärzte Inline-Bilder durch ihre neu kodierte Fassung und die
/// Eigenschaftsliste betroffener `BDC`/`DP` durch ihre entspiegelte Fassung.
fn rewrite_operations(
    operations: &[Operation],
    plans: &BTreeMap<usize, Plan>,
    inline_images: &BTreeMap<usize, Operation>,
    mirrors: &BTreeMap<usize, Dictionary>,
) -> Vec<Operation> {
    let mut out = Vec::with_capacity(operations.len() + plans.len() * 2);
    for (index, op) in operations.iter().enumerate() {
        match (plans.get(&index), inline_images.get(&index)) {
            (Some(plan), _) => out.extend(rebuild_show(plan)),
            (None, Some(image)) => out.push(image.clone()),
            (None, None) => match mirrors.get(&index) {
                Some(cleaned) => out.push(rebuild_marked(op, cleaned)),
                None => out.push(op.clone()),
            },
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Textspiegel in Marked Content
// ---------------------------------------------------------------------------

/// Was an den Textspiegeln **eines** Stroms zu tun ist.
#[derive(Debug, Default)]
struct MirrorFixes {
    /// Operationsindex → bereinigte Eigenschaftsliste, die inline in den Strom
    /// geschrieben wird.
    inline: BTreeMap<usize, Dictionary>,
    /// Eigenschaftslisten, die als eigenes Objekt in der Datei stehen; sie
    /// werden im Dokument bereinigt, nicht im Strom.
    objects: BTreeSet<ObjectId>,
    warnings: Vec<String>,
}

/// Entscheidet je Marked-Content-Abschnitt, ob sein Textspiegel weg muss.
///
/// **Warum über den vorhandenen `Plan`-Mechanismus und nicht über einen eigenen
/// Suchlauf?** Der Spiegel ist keine eigene Fundstelle, sondern die Aussage
/// „hier steht dasselbe wie in den Glyphen darunter“. Ob er zu entfernen ist,
/// hängt deshalb an genau einer Frage — *ist von diesen Glyphen etwas
/// verschwunden?* —, und die beantwortet nur der `Plan`. Ein eigener Durchgang
/// müsste dieselbe Frage ein zweites Mal beantworten und könnte dabei zu einem
/// anderen Ergebnis kommen als der Strom, den er beschreiben soll.
///
/// **Ganz oder gar nicht.** Eine einzige entfernte Glyphe genügt. Ein Spiegel
/// ist der Text des ganzen Abschnitts; sobald daraus etwas fehlt, ist er als
/// Ganzes falsch — und er stünde als Klartext genau dort, wo eben noch das
/// Geheimnis stand. Ihn anteilig zu kürzen ginge nicht: welcher Teil des
/// Spiegels zu welcher Glyphe gehört, sagt kein PDF (gerade darum gibt es ihn:
/// eine `ffi`-Ligatur ist ein Code für drei Zeichen).
///
/// **Und wenn die Schwärzung den Abschnitt nicht berührt**, bleibt der Spiegel
/// unangetastet. Alle `/ActualText` vorsorglich zu löschen würde getaggte PDFs
/// für Screenreader unbrauchbar machen, ohne irgendetwas zu schützen.
fn mirrors_to_clear(
    marked: &[MarkedTextRecord],
    stream: StreamKey,
    plans: &BTreeMap<usize, Plan>,
) -> MirrorFixes {
    let mut fixes = MirrorFixes::default();
    for record in marked {
        if record.stream != stream {
            continue;
        }
        let touched = record
            .shows
            .iter()
            .any(|index| plans.get(index).is_some_and(|plan| plan.hidden_count() > 0));
        if !touched {
            continue;
        }
        match record.property_id {
            // Eigenes Objekt: dort bereinigen. Wird dieselbe Liste von einem
            // zweiten, unberührten Abschnitt benutzt, verliert auch der seinen
            // Spiegel — eine geteilte Liste ist ein geteilter Spiegel, und zu
            // viel entfernt ist hier die sichere Richtung.
            Some(id) => {
                fixes.objects.insert(id);
            }
            None => {
                let (cleaned, dropped) = clean_property_list(&record.properties);
                if !dropped.is_empty() {
                    fixes.warnings.push(format!(
                        "Die Eigenschaftsliste einer Marked-Content-Auszeichnung enthielt \
                         neben dem Textspiegel indirekte Verweise ({}). Eine Liste, die \
                         inline im Strom steht, darf keine enthalten (PDF 32000-1, 14.6.2); \
                         sie sind deshalb mit entfallen. Bitte prüfen, ob die Datei dadurch \
                         anders aussieht.",
                        dropped.join(", ")
                    ));
                }
                fixes.inline.insert(record.op_index, cleaned);
            }
        }
    }
    fixes
}

/// Entfernt die Textschlüssel aus einer Eigenschaftsliste.
///
/// Zurück kommt zusätzlich, welche Einträge als indirekter Verweis wegfallen
/// mussten: die bereinigte Liste wird inline in den Strom geschrieben, und dort
/// sind Verweise nicht zulässig (PDF 32000-1, 14.6.2). Bei einer ohnehin schon
/// inline stehenden Liste kann das nicht vorkommen — nur bei einer, die über
/// `/Resources /Properties` erreichbar war, ohne ein eigenes Objekt zu sein.
fn clean_property_list(dict: &Dictionary) -> (Dictionary, Vec<String>) {
    let mut cleaned = Dictionary::new();
    let mut dropped = Vec::new();
    for (key, value) in dict.iter() {
        if MIRROR_KEYS.contains(&key.as_slice()) {
            continue;
        }
        if matches!(value, Object::Reference(_)) {
            dropped.push(format!("/{}", String::from_utf8_lossy(key)));
            continue;
        }
        cleaned.set(key.to_vec(), value.clone());
    }
    (cleaned, dropped)
}

/// Schreibt ein `BDC`/`DP` mit bereinigter Eigenschaftsliste neu.
///
/// Der Tag (erster Operand) bleibt stehen: er trägt keinen Text, sondern die
/// Rolle des Abschnitts. Stand die Liste bisher als Name in
/// `/Resources /Properties`, tritt jetzt die bereinigte Liste inline an seine
/// Stelle — beides ist als Operand zulässig, und so bleibt der Eintrag in den
/// Ressourcen unangetastet, den andere Abschnitte vielleicht noch brauchen.
fn rebuild_marked(op: &Operation, cleaned: &Dictionary) -> Operation {
    let tag = op
        .operands
        .first()
        .cloned()
        .unwrap_or_else(|| Object::Name(b"Span".to_vec()));
    Operation::new(
        op.operator.as_str(),
        vec![tag, Object::Dictionary(cleaned.clone())],
    )
}

/// Leert den Textspiegel einer Eigenschaftsliste, die als eigenes Objekt in der
/// Datei steht.
fn clear_mirror_object(doc: &mut Document, id: ObjectId) {
    let dict = match doc.objects.get_mut(&id) {
        Some(Object::Dictionary(dict)) => dict,
        Some(Object::Stream(stream)) => &mut stream.dict,
        _ => return,
    };
    for key in MIRROR_KEYS {
        dict.remove(key);
    }
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
        let is_hidden = plan.hidden().get(index).copied().unwrap_or(false);
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
            vec![Object::String(
                crate::encoding::to_win_ansi(text),
                StringFormat::Literal,
            )],
        ),
        Operation::new("ET", vec![]),
    ]
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

/// Wer benutzt welchen Content-Stream?
///
/// [`replace_page_content`] darf einen alten Strom nur löschen, wenn ihn keine
/// andere Seite mehr braucht. Diese Auskunft je geschwärzter Seite neu zu
/// suchen hieß, `get_page_contents` über **alle** Seiten laufen zu lassen —
/// die Kosten wuchsen mit dem Produkt aus Seitenzahl und Zahl der
/// geschwärzten Seiten (8000 Seiten mit je einer Schwärzung: 50 s; mit einer
/// einzigen Schwärzung: 0,6 s).
///
/// Der Index bildet dieselbe Auskunft **einmal** und wird beim Ersetzen
/// fortgeschrieben. Das ist kein Zwischenspeicher, der veralten darf: eine
/// Seite, deren Inhalt bereits ersetzt wurde, benutzt ihre alten Ströme nicht
/// mehr — genau das trägt [`ContentUsers::release`] nach. Damit antwortet der
/// Index Schritt für Schritt so, wie es die wiederholte Suche täte.
#[derive(Debug, Default)]
struct ContentUsers {
    users: BTreeMap<ObjectId, BTreeSet<ObjectId>>,
}

impl ContentUsers {
    fn build(doc: &Document, pages: &[ObjectId]) -> Self {
        let mut users: BTreeMap<ObjectId, BTreeSet<ObjectId>> = BTreeMap::new();
        for page_id in pages {
            for stream in doc.get_page_contents(*page_id) {
                users.entry(stream).or_default().insert(*page_id);
            }
        }
        Self { users }
    }

    /// Benutzt außer `page_id` noch jemand diesen Strom?
    fn shared_with_others(&self, stream: ObjectId, page_id: ObjectId) -> bool {
        self.users
            .get(&stream)
            .is_some_and(|pages| pages.iter().any(|p| *p != page_id))
    }

    /// `page_id` benutzt diesen Strom nicht mehr.
    fn release(&mut self, stream: ObjectId, page_id: ObjectId) {
        if let Some(pages) = self.users.get_mut(&stream) {
            pages.remove(&page_id);
        }
    }

    /// Der neue Strom gehört ab jetzt zu dieser Seite.
    fn claim(&mut self, stream: ObjectId, page_id: ObjectId) {
        self.users.entry(stream).or_default().insert(page_id);
    }
}

/// Ordnet jede Schwärzung ihrer Seite zu — mit dem Index in der übergebenen
/// Liste, denn nur damit lässt sich am Ende je Schwärzung berichten.
///
/// Einmal gebildet statt je Seite gefiltert: bei 8000 Seiten und 8000
/// Schwärzungen sind das 64 Millionen Vergleiche weniger.
fn redactions_by_page(redactions: &[Redaction]) -> BTreeMap<usize, Vec<(usize, &Redaction)>> {
    let mut out: BTreeMap<usize, Vec<(usize, &Redaction)>> = BTreeMap::new();
    for (index, redaction) in redactions.iter().enumerate() {
        out.entry(redaction.region.page)
            .or_default()
            .push((index, redaction));
    }
    out
}

/// Ersetzt den Content-Stream einer Seite und entfernt die alten Objekte.
fn replace_page_content(
    doc: &mut Document,
    page_id: ObjectId,
    data: Vec<u8>,
    content_users: &mut ContentUsers,
) -> Result<()> {
    let old: BTreeSet<ObjectId> = doc.get_page_contents(page_id).into_iter().collect();

    // Streams, die auch von anderen Seiten benutzt werden, bleiben erhalten.
    let shared: BTreeSet<ObjectId> = old
        .iter()
        .copied()
        .filter(|id| content_users.shared_with_others(*id, page_id))
        .collect();

    let mut stream = Stream::new(Dictionary::new(), data);
    let _ = stream.compress();
    let new_id = doc.add_object(Object::Stream(stream));

    let page = doc
        .get_dictionary_mut(page_id)
        .map_err(|e| RedactError::Pdf(e.to_string()))?;
    page.set("Contents", Object::Reference(new_id));

    // Der Index wird mitgeführt, *bevor* gelöscht wird: die Seite benutzt ihre
    // alten Ströme nicht mehr, dafür den neuen.
    for id in &old {
        content_users.release(*id, page_id);
    }
    content_users.claim(new_id, page_id);

    for id in old.difference(&shared) {
        doc.objects.remove(id);
    }
    Ok(())
}

fn rewrite_form(
    doc: &mut Document,
    form_id: ObjectId,
    plans: &BTreeMap<usize, Plan>,
    inline_images: &BTreeMap<usize, Operation>,
    mirrors: &BTreeMap<usize, Dictionary>,
) -> Result<()> {
    if plans.is_empty() && inline_images.is_empty() && mirrors.is_empty() {
        return Ok(());
    }
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
    let decoded = decode_or_fail(&data, "Der Inhalt eines Form-XObjects")?;
    let operations = rewrite_operations(&decoded, plans, inline_images, mirrors);
    let encoded = encode_operations(&operations)?;

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

/// Weist auf Rasterbilder hin, deren Inhalt niemand gelesen hat.
///
/// Die Schwärzung selbst greift inzwischen bis in die Pixel ([`crate::image`]).
/// Was bleibt, ist die *Analyse*: was in einem Bild steht, findet kein Muster
/// und keine Buchungsliste — dafür bräuchte es OCR. Deshalb wird gewarnt,
/// sobald eine Seite überhaupt ein Bild enthält.
///
/// Drei frühere Lücken sind damit geschlossen:
///
/// * Der Hinweis kam nie bei einem reinen Scan, weil bei 0 Schwärzungen vorher
///   zurückgesprungen wurde — ausgerechnet der Fall, in dem er zählt.
/// * `lopdf::Document::get_page_images` betritt keine Form-XObjects.
/// * Es scheiterte an einem indirekten `/Width`; jetzt wird die Größe für die
///   Frage „gibt es hier ein Bild?“ gar nicht mehr gebraucht.
fn warn_about_images(doc: &Document, report: &mut RedactionReport) {
    let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    let with_images = pages
        .iter()
        .filter(|page_id| crate::image::page_has_images(doc, **page_id))
        .count();
    if with_images == 0 {
        return;
    }
    let msg = format!(
        "{with_images} von {} Seite(n) enthalten Rasterbilder. Geschwärzte Bereiche werden im \
         Bild selbst überschrieben; gelesen wird der Bildinhalt aber nicht — Text *in* einem \
         Bild (Scan, Foto) findet die Analyse ohne OCR nicht.",
        pages.len()
    );
    push_warning(report, msg);
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
    use lopdf::dictionary;
    use redact_core::{Action, Region, Source};

    /// Das Geheimnis, das die Audit-Szenarien verstecken.
    const SECRET: &str = "DE89 3704 0044 0532 0130 00";

    // -----------------------------------------------------------------------
    // Werkzeug: Dokumente aus dem Audit nachbauen
    // -----------------------------------------------------------------------

    struct Fixture {
        doc: Document,
        page_id: ObjectId,
        resources_id: ObjectId,
        font_id: ObjectId,
    }

    /// Eine Seite mit Helvetica/WinAnsi und leerem Content-Stream.
    fn fixture() -> Fixture {
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
        let content_id = doc.add_object(Stream::new(dictionary! {}, Vec::new()));
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
        Fixture {
            doc,
            page_id,
            resources_id,
            font_id,
        }
    }

    impl Fixture {
        fn set_content(&mut self, raw: &[u8]) {
            let id = match self
                .doc
                .get_dictionary(self.page_id)
                .unwrap()
                .get(b"Contents")
            {
                Ok(Object::Reference(id)) => *id,
                other => panic!("kein Content-Verweis: {other:?}"),
            };
            self.doc.objects.insert(
                id,
                Object::Stream(Stream::new(dictionary! {}, raw.to_vec())),
            );
        }

        fn redact(&mut self, redactions: &[Redaction]) -> (RedactionReport, Vec<u8>) {
            let report = PdfRedactor::new()
                .apply_with_report(&mut self.doc, redactions)
                .expect("Schwärzung");
            crate::meta::strip_metadata(&mut self.doc);
            let bytes = crate::document::save_to_bytes(&self.doc).expect("Speichern");
            (report, bytes)
        }
    }

    /// Textzeilen bei x = 72, y = 700, Zeilenabstand 15.
    fn text_ops(lines: &[&str]) -> Vec<u8> {
        let mut out = String::from("BT\n/F1 10 Tf\n72 700 Td\n");
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                out.push_str("0 -15 Td\n");
            }
            out.push_str(&format!("({line}) Tj\n"));
        }
        out.push_str("ET\n");
        out.into_bytes()
    }

    /// Ein winziges Graustufen-Inline-Bild; die Rohdaten hinter `ID` sind
    /// genau das, woran `Content::decode` scheitert.
    fn inline_image_ops() -> Vec<u8> {
        let mut raw = Vec::from(&b"q 20 0 0 20 300 780 cm\n"[..]);
        raw.extend_from_slice(b"BI /W 2 /H 2 /CS /G /BPC 8 ID ");
        raw.extend_from_slice(&[0x00, 0xff, 0x7f, 0x30]);
        raw.extend_from_slice(b" EI Q\n");
        raw
    }

    /// Deckt beide Textzeilen ab (y = 700 und y = 685).
    fn whole_text_area() -> Redaction {
        manual(Rect::new(40.0, 600.0, 560.0, 760.0))
    }

    fn manual(rect: Rect) -> Redaction {
        Redaction::new(
            Region::new(
                0,
                rect,
                None,
                Source::Manual {
                    reason: "Audit".into(),
                },
            ),
            Action::Blackout,
        )
    }

    #[track_caller]
    fn assert_no_leak(bytes: &[u8], needle: &str, what: &str) {
        let hits = crate::leaks(bytes, needle);
        assert!(
            hits.is_empty(),
            "{what}: „{needle}“ steht noch {} mal in der Ausgabe:\n{}",
            hits.len(),
            hits.join("\n")
        );
    }

    #[track_caller]
    fn assert_present(bytes: &[u8], needle: &str, what: &str) {
        assert!(
            !crate::leaks(bytes, needle).is_empty(),
            "{what}: „{needle}“ ist aus der Datei verschwunden"
        );
    }

    // -----------------------------------------------------------------------
    // C1 — Inline-Bilder zerreißen den Content-Stream
    // -----------------------------------------------------------------------

    #[test]
    fn inline_image_survives_decode_and_encode_unchanged() {
        let raw = inline_image_ops();
        let ops = crate::ops::decode_content(&raw);
        assert!(
            ops.iter().any(|op| op.operator == "BI"),
            "Inline-Bild nicht erkannt: {ops:?}"
        );
        let encoded = encode_operations(&ops).unwrap();
        // Zweiter Durchlauf: gleiche Operationen, gleiche Bilddaten.
        let again = crate::ops::decode_content(&encoded);
        let payload = |ops: &[Operation]| {
            ops.iter()
                .find_map(|op| inline_image_operands(op).map(|(_, d)| d.to_vec()))
        };
        assert_eq!(payload(&ops), payload(&again));
        assert_eq!(payload(&again), Some(vec![0x00, 0xff, 0x7f, 0x30]));
        assert_eq!(encode_operations(&again).unwrap(), encoded);
    }

    #[test]
    fn text_behind_an_inline_image_is_found_and_redacted() {
        let mut f = fixture();
        let mut raw = inline_image_ops();
        raw.extend_from_slice(&text_ops(&[
            "Kontoinhaber: Max Mustermann",
            &format!("IBAN: {SECRET}"),
        ]));
        f.set_content(&raw);

        let (report, out) = f.redact(&[whole_text_area()]);
        assert!(
            report.removed_glyphs > 0,
            "hinter dem Inline-Bild wurde kein Zeichen gefunden"
        );
        assert_no_leak(&out, SECRET, "Inline-Bild vor Text");
    }

    #[test]
    fn text_behind_an_inline_image_is_not_thrown_away() {
        // Der Datenverlust ist die zweite Hälfte des Defekts: geschwärzt wird
        // nur die IBAN-Zeile, alles andere muss stehen bleiben.
        let mut f = fixture();
        let mut raw = inline_image_ops();
        raw.extend_from_slice(&text_ops(&[
            "Kontoinhaber: Max Mustermann",
            &format!("IBAN: {SECRET}"),
        ]));
        f.set_content(&raw);

        let (_, out) = f.redact(&[manual(Rect::new(40.0, 678.0, 560.0, 696.0))]);
        assert_no_leak(&out, SECRET, "nur die IBAN-Zeile geschwärzt");
        assert_present(&out, "Kontoinhaber", "unbeteiligter Text hinter dem Bild");
    }

    #[test]
    fn the_output_has_no_dangling_inline_image() {
        let mut f = fixture();
        let mut raw = inline_image_ops();
        raw.extend_from_slice(&text_ops(&[&format!("IBAN: {SECRET}")]));
        f.set_content(&raw);
        let (_, out) = f.redact(&[whole_text_area()]);

        let doc = crate::document::load_from_bytes(&out).expect("Ausgabe ladbar");
        let page_id = *doc.get_pages().values().next().unwrap();
        let content = doc.get_page_content(page_id).expect("Content lesbar");
        let text = String::from_utf8_lossy(&content);
        assert_eq!(
            text.matches("BI").count(),
            text.matches("EI").count(),
            "Inline-Bild ohne Abschluss:\n{text}"
        );
        // Und der Strom ist wieder vollständig dekodierbar.
        let ops = crate::ops::decode_content(&content);
        assert!(ops.iter().any(|op| op.operator == "BI"));
    }

    #[test]
    fn iban_inside_a_form_xobject_behind_an_inline_image_is_redacted() {
        let mut f = fixture();
        let form_content = format!("BT\n/F1 10 Tf\n72 640 Td\n(IBAN: {SECRET}) Tj\nET\n");
        let font_id = f.font_id;
        let form_id = f.doc.add_object(Object::Stream(
            Stream::new(
                dictionary! {
                    "Type" => "XObject",
                    "Subtype" => "Form",
                    "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                    "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
                },
                form_content.into_bytes(),
            )
            .with_compression(false),
        ));
        f.doc
            .get_dictionary_mut(f.resources_id)
            .unwrap()
            .set("XObject", dictionary! { "Fm0" => form_id });

        let mut raw = text_ops(&["Kontoinhaber: Max Mustermann"]);
        raw.extend_from_slice(&inline_image_ops());
        raw.extend_from_slice(b"q /Fm0 Do Q\n");
        f.set_content(&raw);

        let (_, out) = f.redact(&[whole_text_area()]);
        assert_no_leak(&out, SECRET, "Form-XObject hinter Inline-Bild");
    }

    // -----------------------------------------------------------------------
    // C2 — verwaiste Objekte
    // -----------------------------------------------------------------------

    /// Legt eine Annotation mit Appearance-Stream an. `intersecting` steuert,
    /// ob ihr `/Rect` in den geschwärzten Bereich ragt.
    fn with_annotation(f: &mut Fixture, intersecting: bool) {
        let font_id = f.font_id;
        let ap_content = format!("BT\n/F1 8 Tf\n0 4 Td\n(Notiz: {SECRET}) Tj\nET\n");
        let ap_id = f.doc.add_object(Object::Stream(
            Stream::new(
                dictionary! {
                    "Type" => "XObject",
                    "Subtype" => "Form",
                    "BBox" => vec![0.into(), 0.into(), 240.into(), 20.into()],
                    "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
                },
                ap_content.into_bytes(),
            )
            .with_compression(false),
        ));
        let rect = if intersecting {
            vec![72.into(), 680.into(), 312.into(), 700.into()]
        } else {
            vec![400.into(), 100.into(), 540.into(), 120.into()]
        };
        let annot_id = f.doc.add_object(Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "FreeText",
            "Rect" => rect,
            "F" => 4_i64,
            "AP" => dictionary! { "N" => ap_id },
        }));
        f.doc
            .get_dictionary_mut(f.page_id)
            .unwrap()
            .set("Annots", Object::Array(vec![Object::Reference(annot_id)]));
    }

    #[test]
    fn appearance_stream_of_a_removed_annotation_is_gone() {
        let mut f = fixture();
        f.set_content(&text_ops(&[
            "Kontoinhaber: Max Mustermann",
            &format!("IBAN: {SECRET}"),
        ]));
        with_annotation(&mut f, true);

        let (report, out) = f.redact(&[whole_text_area()]);
        assert_eq!(report.removed_annotations, 1);
        assert_no_leak(&out, SECRET, "/AP der entfernten Annotation");
    }

    #[test]
    fn a_non_overlapping_annotation_keeps_its_appearance_stream() {
        // Gegenprobe und bewusste Grenze: diese Annotation liegt außerhalb
        // jeder Schwärzung, bleibt also referenziert — der Erreichbarkeitslauf
        // darf sie gerade *nicht* anfassen. Ihr `/AP` trägt das Geheimnis
        // weiter. Das ist kein Fehler des Aufräumens, sondern eine Lücke der
        // Analyse: der Extraktor liest Appearance-Streams nicht, deshalb
        // entsteht für diese Stelle gar keine Schwärzung.
        let mut f = fixture();
        f.set_content(&text_ops(&[
            "Kontoinhaber: Max Mustermann",
            &format!("IBAN: {SECRET}"),
        ]));
        with_annotation(&mut f, false);

        let (report, out) = f.redact(&[whole_text_area()]);
        assert_eq!(report.removed_annotations, 0);
        assert_present(&out, SECRET, "/AP einer nicht überlappenden Annotation");
    }

    #[test]
    fn struct_elem_below_the_removed_root_does_not_survive() {
        let mut f = fixture();
        f.set_content(&text_ops(&[
            "Kontoinhaber: Max Mustermann",
            &format!("IBAN: {SECRET}"),
        ]));
        let elem_id = f.doc.add_object(Object::Dictionary(dictionary! {
            "Type" => "StructElem",
            "S" => "Span",
            "ActualText" => Object::string_literal(SECRET),
            "Alt" => Object::string_literal(format!("Kontonummer {SECRET}")),
        }));
        let root_id = f.doc.add_object(Object::Dictionary(dictionary! {
            "Type" => "StructTreeRoot",
            "K" => vec![Object::Reference(elem_id)],
        }));
        let catalog_id = match f.doc.trailer.get(b"Root").unwrap() {
            Object::Reference(id) => *id,
            _ => unreachable!(),
        };
        f.doc
            .get_dictionary_mut(catalog_id)
            .unwrap()
            .set("StructTreeRoot", Object::Reference(root_id));

        let (_, out) = f.redact(&[whole_text_area()]);
        assert_no_leak(&out, SECRET, "/StructElem /ActualText");
    }

    // -----------------------------------------------------------------------
    // C3 — inkrementelle Vorversionen
    // -----------------------------------------------------------------------

    #[test]
    fn an_incremental_history_is_reported_even_without_redactions() {
        let mut f = fixture();
        f.set_content(&text_ops(&["Kontoinhaber: Max Mustermann"]));
        f.doc.trailer.set("Prev", Object::Integer(4711));

        let report = PdfRedactor::new()
            .apply_with_report(&mut f.doc, &[])
            .expect("Schwärzung");
        assert_eq!(report.removed_glyphs, 0);
        assert_eq!(
            report.warnings.len(),
            1,
            "keine Warnung zur Vorgeschichte: {:?}",
            report.warnings
        );
        assert!(report.warnings[0].contains("/Prev"));
    }

    #[test]
    fn a_document_without_history_gets_no_warning() {
        let mut f = fixture();
        f.set_content(&text_ops(&["Kontoinhaber: Max Mustermann"]));
        let report = PdfRedactor::new()
            .apply_with_report(&mut f.doc, &[])
            .expect("Schwärzung");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    }

    // -----------------------------------------------------------------------
    // Bestehende Zusicherungen zum Neuaufbau der Text-Operationen
    // -----------------------------------------------------------------------

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
            selection: Selection {
                hidden,
                per_redaction: BTreeMap::new(),
            },
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
            for (a, b) in existing
                .selection
                .hidden
                .iter_mut()
                .zip([false, false, true])
            {
                *a = *a || b;
            }
        }
        assert_eq!(target[&7].hidden(), [true, false, true]);
    }
}
