//! Bilder **wirklich** schwärzen: die Pixel werden ersetzt, nicht übermalt.
//!
//! Ein schwarzes Rechteck über einem gescannten Kontoauszug ist keine
//! Schwärzung — die Pixel mit der IBAN bleiben im Bild-XObject stehen und sind
//! mit jedem Werkzeug wieder freizulegen. Deshalb wird hier für jede
//! Schwärzung ermittelt, welche Bilder sie schneidet, das Bild dekodiert, der
//! Bereich über die inverse Bild-CTM in Pixelkoordinaten umgerechnet und dort
//! gefüllt.
//!
//! ## Ablauf
//!
//! ```text
//!  Seite ──interpret──► Platzierungen (Name/Inline, CTM, Stream)
//!                            │
//!            ops::page_ops ──┴──► dieselben Bilder, aber dekodiert (RGBA8)
//!                            │
//!                       Pixel füllen  (inverse CTM je Platzierung)
//!                            │
//!                    neu kodieren (Flate) ──► XObject ersetzen/kopieren
//! ```
//!
//! ## Entscheidungen
//!
//! * **Dekodiert wird nicht neu.** [`crate::ops`] kann bereits Flate, DCT,
//!   Bitmasken, `/SMask` und alle Bittiefen. [`crate::ops::page_ops`] liefert
//!   die Bilder einer Seite als RGBA8 in genau der Reihenfolge, in der der
//!   Interpreter sie meldet — deshalb genügt hier ein zweiter, sehr schlanker
//!   Durchlauf, der nur Name, CTM und Herkunft einsammelt und beides paart.
//!   Stimmen die beiden Läufe nicht überein, wird abgebrochen statt geraten.
//! * **Neu kodiert wird immer verlustfrei (Flate).** Ein `/DCTDecode`-Bild
//!   wird dabei zu `/FlateDecode`. Das ist gewollt: JPEG neu zu kodieren wäre
//!   verlustbehaftet, und die DCT-Blöcke am Rand der Schwärzung könnten Reste
//!   der ursprünglichen Pixel zurücktragen. Die Datei wird größer — das ist
//!   der Preis dafür, dass die Schwärzung hält.
//! * **Mehrfach benutzte Bilder werden kopiert.** Wird dasselbe XObject von
//!   mehreren Seiten gezeichnet, bekommt die geschwärzte Seite eine eigene
//!   Kopie; die unbeteiligten Vorkommen bleiben unverändert.
//! * **Nicht dekodierbar heißt Abbruch.** JPX oder CCITT lassen sich hier
//!   nicht öffnen; dann so zu tun, als wäre geschwärzt worden, wäre der
//!   gefährlichste aller Ausgänge. [`ImageOptions::allow_undecodable`] hebt das
//!   auf — dann bleibt es bei einer Warnung und einem übermalten Bild.

use std::collections::{BTreeMap, BTreeSet};

use lopdf::content::Operation;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use redact_core::{Rect, RedactError, Redaction, Result};

use crate::content::{ContentSink, ImageEvent, SinkContext, StreamKey};
use crate::matrix::Matrix;
use crate::ops::DrawOp;

/// Wie tief die Suche nach Bildern in verschachtelte Form-XObjects steigt.
const MAX_RESOURCE_DEPTH: usize = 8;

// ---------------------------------------------------------------------------
// Schnittstelle
// ---------------------------------------------------------------------------

/// Stellschrauben der Bild-Schwärzung.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImageOptions {
    /// Nicht dekodierbare Bilder (`JPXDecode`, `CCITTFaxDecode`, defekte
    /// Streams) durchgehen lassen, statt abzubrechen.
    ///
    /// **Unsicher.** Das Bild bleibt dann unverändert in der Datei; die
    /// Schwärzung liegt nur obenauf und ist rückgängig zu machen. Gedacht für
    /// Aufrufer, die das bewusst in Kauf nehmen — die Kommandozeile bietet es
    /// (noch) nicht an.
    pub allow_undecodable: bool,
}

/// Was die Bild-Schwärzung getan hat.
#[derive(Debug, Clone, Default)]
pub struct ImageOutcome {
    /// Anzahl Bilder, deren Pixel überschrieben wurden.
    pub redacted_images: usize,
    /// Davon: Bilder, die kopiert werden mussten, weil sie mehrfach benutzt
    /// werden.
    pub copied_images: usize,
    /// Summe der gefüllten Pixel. Überlappen sich zwei Schwärzungsbereiche in
    /// einem Bild, zählt der gemeinsame Teil je Bereich einmal.
    pub filled_pixels: u64,
    pub warnings: Vec<String>,
    /// Ersatzoperationen für Inline-Bilder, je Strom und Index im dekodierten
    /// Operationsstrom. Inline-Bilder stehen im Content-Stream selbst — sie
    /// können erst beim Neuschreiben des Stroms eingesetzt werden (siehe
    /// [`crate::redact`]).
    pub inline_replacements: BTreeMap<InlineTarget, BTreeMap<usize, Operation>>,
}

/// In welchem Strom ein zu ersetzendes Inline-Bild steht.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum InlineTarget {
    /// Der Content-Stream dieser Seite.
    Page(ObjectId),
    /// Der Strom dieses Form-XObjects. Wird das Formular von mehreren Seiten
    /// gezeichnet, wirkt die Schwärzung dort ebenfalls — dieselbe bewusste
    /// Über-Schwärzung wie beim Text in Form-XObjects.
    Form(ObjectId),
}

/// Schwärzt die Pixel aller Bilder, die von einer Schwärzung geschnitten werden.
///
/// `padding` ist derselbe Rand, den [`crate::redact::PdfRedactor`] auf die
/// Deck-Rechtecke legt — Bild und Rechteck sollen sich decken.
///
/// Verändert das Dokument (Bild-XObjects und ggf. `/Resources`), rührt aber
/// keinen Content-Stream an: die Inline-Bilder kommen als
/// [`ImageOutcome::inline_replacements`] zurück.
pub fn redact_images(
    doc: &mut Document,
    redactions: &[Redaction],
    padding: f64,
    options: &ImageOptions,
) -> Result<ImageOutcome> {
    let mut outcome = ImageOutcome::default();
    let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    if pages.is_empty() || redactions.is_empty() {
        return Ok(outcome);
    }

    // Phase 1 — Bestandsaufnahme über *alle* Seiten. Erst danach ist bekannt,
    // ob ein Bild nur an einer Stelle benutzt wird (dann darf es überschrieben
    // werden) oder mehrfach (dann muss kopiert werden).
    let mut per_page: Vec<Vec<Placement>> = Vec::with_capacity(pages.len());
    let mut image_pages: BTreeMap<ObjectId, BTreeSet<usize>> = BTreeMap::new();
    let mut form_pages: BTreeMap<ObjectId, BTreeSet<usize>> = BTreeMap::new();
    for (index, page_id) in pages.iter().enumerate() {
        let (placements, forms) = scan_page_images(doc, *page_id);
        for placement in &placements {
            if let Target::XObject { id: Some(id), .. } = &placement.target {
                image_pages.entry(*id).or_default().insert(index);
            }
        }
        for form in forms {
            form_pages.entry(form).or_default().insert(index);
        }
        per_page.push(placements);
    }

    // Phase 2 — Pixel füllen. Das Dokument bleibt dabei unangetastet, damit
    // alle Messungen auf demselben Stand beruhen. Die Arbeitspuffer sind
    // seitenübergreifend: ein Inline-Bild in einem Form-XObject kann von zwei
    // Seiten aus geschwärzt werden und darf dann nicht zweimal geschrieben
    // werden, sondern einmal mit beiden Bereichen.
    let mut works: BTreeMap<Key, Work> = BTreeMap::new();
    for (index, page_id) in pages.iter().enumerate() {
        let zones = zones_for_page(redactions, index, padding);
        let placements = &per_page[index];
        if zones.is_empty() || placements.is_empty() {
            continue;
        }
        // Grobtest ohne Dekodieren: berührt überhaupt eine Zone eine Bildfläche?
        let touched = placements
            .iter()
            .any(|p| zones.iter().any(|z| ctm_bounds(&p.ctm).intersects(&z.rect)));
        if !touched {
            continue;
        }

        fill_page(
            doc,
            index,
            *page_id,
            placements,
            &zones,
            options,
            &mut works,
            &mut outcome,
        )?;
    }

    // Phase 3 — schreiben.
    for (key, work) in works {
        if work.filled == 0 {
            continue;
        }
        outcome.filled_pixels += work.filled;
        match key {
            Key::Inline(target, op_index) => {
                let (dict, data) = encode_inline(&work)?;
                outcome
                    .inline_replacements
                    .entry(target)
                    .or_default()
                    .insert(
                        op_index,
                        Operation::new(
                            "BI",
                            vec![
                                Object::Dictionary(dict),
                                Object::String(data, StringFormat::Literal),
                            ],
                        ),
                    );
                outcome.redacted_images += 1;
            }
            Key::XObject(_, id) => {
                let shared = image_pages.get(&id).map(BTreeSet::len).unwrap_or(1) > 1;
                // Kopieren geht nur, wenn sich der Verweis isolieren lässt:
                // in den Seitenressourcen immer, in einem Form-XObject nur,
                // wenn dieses Formular allein von dieser Seite benutzt wird.
                let isolable = work.streams.iter().all(|stream| match stream {
                    StreamKey::Page => true,
                    StreamKey::Form(form) => form_pages.get(form).map(BTreeSet::len) == Some(1),
                });
                let stream = build_stream(doc, encode_xobject(&work));
                if !shared {
                    doc.objects.insert(id, Object::Stream(stream));
                } else if isolable {
                    let new_id = doc.add_object(Object::Stream(stream));
                    for source in &work.streams {
                        match source {
                            StreamKey::Page => repoint_page(doc, work.page_id, &work.name, new_id)?,
                            StreamKey::Form(form) => {
                                if !repoint_form(doc, *form, &work.name, new_id)? {
                                    repoint_page(doc, work.page_id, &work.name, new_id)?;
                                }
                            }
                        }
                    }
                    outcome.copied_images += 1;
                } else {
                    // Letzter Ausweg: überschreiben. Lieber zu viel
                    // geschwärzt als eine Datei, in der die Pixel bleiben.
                    doc.objects.insert(id, Object::Stream(stream));
                    outcome.warnings.push(format!(
                        "Bild /{} steckt in einem Form-XObject, das mehrere Seiten benutzen. \
                         Es wurde überschrieben — die Schwärzung wirkt deshalb auch auf die \
                         anderen Seiten.",
                        String::from_utf8_lossy(&work.name)
                    ));
                }
                outcome.redacted_images += 1;
            }
        }
    }

    Ok(outcome)
}

/// Enthält die Seite Rasterbilder — auch in Form-XObjects und als Inline-Bild?
///
/// Ersatz für `lopdf::Document::get_page_images`, das an einem indirekten
/// `/Width` scheitert, Form-XObjects gar nicht betritt und Inline-Bilder nicht
/// kennt. Hier wird nichts dekodiert: die Frage ist nur, ob es überhaupt ein
/// Bild gibt.
pub fn page_has_images(doc: &Document, page_id: ObjectId) -> bool {
    let resources = crate::content::page_resources(doc, page_id);
    let mut seen = BTreeSet::new();
    if resources_have_image(doc, resources.as_ref(), &mut seen, 0) {
        return true;
    }
    // Inline-Bilder stehen nur im Strom. Der Vorfilter auf die zwei Bytes
    // spart bei jeder bildlosen Seite das vollständige Parsen; entschieden
    // wird trotzdem am dekodierten Strom, damit ein `(BI)` in einer
    // Zeichenkette keinen Fehlalarm auslöst.
    let Ok(data) = doc.get_page_content(page_id) else {
        return false;
    };
    if !data.windows(2).any(|pair| pair == b"BI") {
        return false;
    }
    crate::ops::decode_content(&data)
        .iter()
        .any(|op| op.operator == "BI")
}

fn resources_have_image(
    doc: &Document,
    resources: Option<&Dictionary>,
    seen: &mut BTreeSet<ObjectId>,
    depth: usize,
) -> bool {
    if depth > MAX_RESOURCE_DEPTH {
        return false;
    }
    let Some(resources) = resources else {
        return false;
    };
    let Some(xobjects) = resources
        .get(b"XObject")
        .ok()
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok())
    else {
        return false;
    };
    for (_, entry) in xobjects.iter() {
        if let Object::Reference(id) = entry {
            if !seen.insert(*id) {
                continue;
            }
        }
        let Ok((_, resolved)) = doc.dereference(entry) else {
            continue;
        };
        let Ok(stream) = resolved.as_stream() else {
            continue;
        };
        match stream.dict.get(b"Subtype").and_then(Object::as_name) {
            Ok(b"Image") => return true,
            Ok(b"Form") => {
                let inner = stream
                    .dict
                    .get(b"Resources")
                    .ok()
                    .and_then(|o| doc.dereference(o).ok())
                    .and_then(|(_, o)| o.as_dict().ok())
                    .cloned();
                if resources_have_image(doc, inner.as_ref(), seen, depth + 1) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Platzierungen einsammeln
// ---------------------------------------------------------------------------

/// Worauf eine Bildplatzierung zeigt.
#[derive(Debug, Clone)]
enum Target {
    XObject {
        /// `None`, wenn das Bild kein eigenständiges Objekt ist — dann lässt es
        /// sich nicht ersetzen.
        id: Option<ObjectId>,
        name: Vec<u8>,
        is_mask: bool,
        filters: String,
    },
    Inline {
        is_mask: bool,
        filters: String,
    },
}

#[derive(Debug, Clone)]
struct Placement {
    stream: StreamKey,
    op_index: usize,
    ctm: Matrix,
    target: Target,
}

impl Placement {
    fn label(&self, page_index: usize) -> String {
        match &self.target {
            Target::XObject { name, .. } => format!(
                "Bild /{} auf Seite {}",
                String::from_utf8_lossy(name),
                page_index + 1
            ),
            Target::Inline { .. } => format!("Inline-Bild auf Seite {}", page_index + 1),
        }
    }

    fn filters(&self) -> &str {
        match &self.target {
            Target::XObject { filters, .. } | Target::Inline { filters, .. } => filters,
        }
    }
}

#[derive(Default)]
struct Collector {
    placements: Vec<Placement>,
    forms: BTreeSet<ObjectId>,
}

impl ContentSink for Collector {
    fn wants_graphics(&self) -> bool {
        true
    }

    fn form(&mut self, id: ObjectId) {
        self.forms.insert(id);
    }

    fn image(&mut self, cx: &SinkContext, event: &ImageEvent) {
        // Dieselben Abbruchbedingungen wie in `ops::OpsCollector::image` —
        // sonst geraten die beiden Läufe außer Takt.
        let target = match event.inline {
            Some((dict, _)) => Target::Inline {
                is_mask: dict_flag(dict, b"ImageMask", b"IM"),
                filters: filter_label(dict),
            },
            None => {
                let Some(name) = event.name else {
                    return;
                };
                let Some((id, dict)) = image_xobject(cx.doc, cx.resources, name) else {
                    return;
                };
                Target::XObject {
                    id,
                    name: name.to_vec(),
                    is_mask: dict_flag(&dict, b"ImageMask", b"IM"),
                    filters: filter_label(&dict),
                }
            }
        };
        self.placements.push(Placement {
            stream: cx.stream,
            op_index: cx.op_index,
            ctm: event.ctm,
            target,
        });
    }
}

fn scan_page_images(doc: &Document, page_id: ObjectId) -> (Vec<Placement>, BTreeSet<ObjectId>) {
    let Ok(data) = doc.get_page_content(page_id) else {
        return (Vec::new(), BTreeSet::new());
    };
    let operations = crate::ops::decode_content(&data);
    let resources = crate::content::page_resources(doc, page_id);
    let mut collector = Collector::default();
    crate::content::interpret(
        doc,
        &operations,
        StreamKey::Page,
        resources.as_ref(),
        Matrix::IDENTITY,
        &mut collector,
    );
    (collector.placements, collector.forms)
}

/// Bild-XObject aus den Ressourcen — Gegenstück zu `ops::image_xobject`, aber
/// ohne den Stream festzuhalten (hier wird nur das Dictionary gebraucht).
fn image_xobject(
    doc: &Document,
    resources: Option<&Dictionary>,
    name: &[u8],
) -> Option<(Option<ObjectId>, Dictionary)> {
    let xobjects = resources?.get(b"XObject").ok()?;
    let (_, xobjects) = doc.dereference(xobjects).ok()?;
    let entry = xobjects.as_dict().ok()?.get(name).ok()?;
    let id = match entry {
        Object::Reference(id) => Some(*id),
        _ => None,
    };
    let (_, resolved) = doc.dereference(entry).ok()?;
    let stream = resolved.as_stream().ok()?;
    if stream.dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Image") {
        return None;
    }
    Some((id, stream.dict.clone()))
}

fn dict_flag(dict: &Dictionary, long: &[u8], short: &[u8]) -> bool {
    dict.get(long)
        .or_else(|_| dict.get(short))
        .ok()
        .and_then(|o| o.as_bool().ok())
        .unwrap_or(false)
}

/// Die Filterkette als Text — nur für Fehlermeldungen.
fn filter_label(dict: &Dictionary) -> String {
    let Ok(filter) = dict.get(b"Filter").or_else(|_| dict.get(b"F")) else {
        return "ohne Filter".to_string();
    };
    let names: Vec<String> = match filter {
        Object::Name(name) => vec![String::from_utf8_lossy(name).into_owned()],
        Object::Array(items) => items
            .iter()
            .filter_map(|o| o.as_name().ok())
            .map(|n| String::from_utf8_lossy(n).into_owned())
            .collect(),
        _ => Vec::new(),
    };
    if names.is_empty() {
        "ohne Filter".to_string()
    } else {
        names.join(", ")
    }
}

// ---------------------------------------------------------------------------
// Pixel füllen
// ---------------------------------------------------------------------------

/// Ein Schwärzungsbereich mit der Farbe, die dort hineingehört.
#[derive(Debug, Clone, Copy)]
struct Zone {
    rect: Rect,
    color: [u8; 3],
}

fn zones_for_page(redactions: &[Redaction], page_index: usize, padding: f64) -> Vec<Zone> {
    redactions
        .iter()
        .filter(|r| r.region.page == page_index)
        .filter_map(|r| {
            let rect = r.region.rect.expanded(padding);
            if rect.is_empty() {
                return None;
            }
            let (red, green, blue) = r.action.fill_color().unwrap_or((0.0, 0.0, 0.0));
            let to_u8 = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            Some(Zone {
                rect,
                color: [to_u8(red), to_u8(green), to_u8(blue)],
            })
        })
        .collect()
}

/// Welches Objekt am Ende geschrieben wird.
///
/// Ein Bild-XObject wird je Seite getrennt geführt, weil es für eine Seite
/// kopiert werden kann. Ein Inline-Bild gehört dagegen zu genau einem Strom:
/// steht es in einem Form-XObject, das zwei Seiten zeichnen, muss es *einmal*
/// mit den Bereichen beider Seiten geschrieben werden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    XObject(usize, ObjectId),
    /// Strom und Index der `BI`-Operation darin.
    Inline(InlineTarget, usize),
}

/// Ein Bild in Arbeit: dekodierte Pixel plus alles, was zum Zurückschreiben
/// nötig ist.
struct Work {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    is_mask: bool,
    name: Vec<u8>,
    /// Seite, in deren Ressourcen eine Kopie verankert wird.
    page_id: ObjectId,
    /// Aus welchen Streams heraus das Bild gezeichnet wird (für die Kopie).
    streams: BTreeSet<StreamKey>,
    filled: u64,
}

#[allow(clippy::too_many_arguments)]
fn fill_page(
    doc: &Document,
    page_index: usize,
    page_id: ObjectId,
    placements: &[Placement],
    zones: &[Zone],
    options: &ImageOptions,
    works: &mut BTreeMap<Key, Work>,
    outcome: &mut ImageOutcome,
) -> Result<()> {
    let page = crate::ops::page_ops(doc, page_index)?;
    let drawn: Vec<(usize, Matrix)> = page
        .ops
        .iter()
        .filter_map(|op| match op {
            DrawOp::Image { image, ctm, .. } => Some((*image, *ctm)),
            _ => None,
        })
        .collect();
    // Die Paarung beruht darauf, dass beide Läufe denselben Interpreter mit
    // denselben Abbruchbedingungen benutzen. Weicht sie ab, ist unklar, welches
    // Bild zu welcher Platzierung gehört — dann lieber abbrechen als das
    // falsche Bild schwärzen.
    if drawn.len() != placements.len() {
        return Err(RedactError::Pdf(format!(
            "Seite {}: {} Bildplatzierungen, aber {} dekodierte Bilder — die Zuordnung ist \
             nicht eindeutig, die Schwärzung wird abgebrochen.",
            page_index + 1,
            placements.len(),
            drawn.len()
        )));
    }

    for (placement, (image_index, ctm)) in placements.iter().zip(drawn) {
        if !same_matrix(&placement.ctm, &ctm) {
            return Err(RedactError::Pdf(format!(
                "Seite {}: die Bild-CTM der beiden Durchläufe stimmt nicht überein — die \
                 Schwärzung wird abgebrochen.",
                page_index + 1
            )));
        }
        let bounds = ctm_bounds(&ctm);
        let touching: Vec<Zone> = zones
            .iter()
            .filter(|z| bounds.intersects(&z.rect))
            .copied()
            .collect();
        if touching.is_empty() {
            continue;
        }

        let raster = &page.images[image_index];
        if raster.placeholder {
            let message = format!(
                "{} lässt sich nicht dekodieren (Filter: {}). Die Schwärzung läge nur \
                 darüber; die Pixel blieben in der Datei.",
                placement.label(page_index),
                placement.filters()
            );
            if options.allow_undecodable {
                outcome.warnings.push(message);
                continue;
            }
            return Err(RedactError::Pdf(message));
        }

        let (key, is_mask) = match &placement.target {
            Target::XObject {
                id: Some(id),
                is_mask,
                ..
            } => (Key::XObject(page_index, *id), *is_mask),
            Target::XObject { id: None, .. } => {
                let message = format!(
                    "{} ist kein eigenständiges Objekt und kann nicht ersetzt werden.",
                    placement.label(page_index)
                );
                if options.allow_undecodable {
                    outcome.warnings.push(message);
                    continue;
                }
                return Err(RedactError::Pdf(message));
            }
            Target::Inline { is_mask, .. } => {
                let target = match placement.stream {
                    StreamKey::Page => InlineTarget::Page(page_id),
                    StreamKey::Form(form) => InlineTarget::Form(form),
                };
                (Key::Inline(target, placement.op_index), *is_mask)
            }
        };

        let name = match &placement.target {
            Target::XObject { name, .. } => name.clone(),
            Target::Inline { .. } => Vec::new(),
        };
        let work = works.entry(key).or_insert_with(|| Work {
            width: raster.width,
            height: raster.height,
            rgba: raster.rgba.clone(),
            is_mask,
            name,
            page_id,
            streams: BTreeSet::new(),
            filled: 0,
        });
        work.streams.insert(placement.stream);
        work.fill(&ctm, &touching);
    }
    Ok(())
}

impl Work {
    /// Füllt alle Pixel, deren Fläche eine der Zonen berührt.
    fn fill(&mut self, ctm: &Matrix, zones: &[Zone]) {
        let Some(inverse) = ctm.invert() else {
            return;
        };
        for zone in zones {
            let Some((x0, x1, y0, y1)) = self.pixel_bounds(&inverse, &zone.rect) else {
                continue;
            };
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if !self.covers(ctm, x, y, &zone.rect) {
                        continue;
                    }
                    let offset = (y * self.width as usize + x) * 4;
                    if self.is_mask {
                        // Eine Stencil-Maske trägt nur die *Form* — sie malt in
                        // der jeweils gesetzten Füllfarbe. Der Bereich malt
                        // künftig nicht mehr; damit ist die Form weg. Sichtbar
                        // deckt ihn das Rechteck ab, das die Schwärzung ohnehin
                        // zeichnet.
                        self.rgba[offset + 3] = 0;
                    } else {
                        self.rgba[offset] = zone.color[0];
                        self.rgba[offset + 1] = zone.color[1];
                        self.rgba[offset + 2] = zone.color[2];
                        self.rgba[offset + 3] = 255;
                    }
                    self.filled += 1;
                }
            }
        }
    }

    /// Grober Pixelbereich, in dem die Zone liegen kann (inklusive Rändern).
    fn pixel_bounds(&self, inverse: &Matrix, rect: &Rect) -> Option<(usize, usize, usize, usize)> {
        let corners = [
            (rect.ll.x, rect.ll.y),
            (rect.ur.x, rect.ll.y),
            (rect.ll.x, rect.ur.y),
            (rect.ur.x, rect.ur.y),
        ];
        let (mut umin, mut umax) = (f64::INFINITY, f64::NEG_INFINITY);
        let (mut vmin, mut vmax) = (f64::INFINITY, f64::NEG_INFINITY);
        for (x, y) in corners {
            let p = inverse.apply(x, y);
            if !p.x.is_finite() || !p.y.is_finite() {
                return None;
            }
            umin = umin.min(p.x);
            umax = umax.max(p.x);
            vmin = vmin.min(p.y);
            vmax = vmax.max(p.y);
        }
        let w = self.width as f64;
        let h = self.height as f64;
        // Bildzeile 0 liegt oben: v = 1 entspricht y = 0.
        let x0 = (umin * w).floor() - 1.0;
        let x1 = (umax * w).ceil() + 1.0;
        let y0 = ((1.0 - vmax) * h).floor() - 1.0;
        let y1 = ((1.0 - vmin) * h).ceil() + 1.0;
        if x1 < 0.0 || y1 < 0.0 || x0 > w - 1.0 || y0 > h - 1.0 {
            return None;
        }
        Some((
            x0.max(0.0) as usize,
            (x1.min(w - 1.0)).max(0.0) as usize,
            y0.max(0.0) as usize,
            (y1.min(h - 1.0)).max(0.0) as usize,
        ))
    }

    /// Berührt die Fläche des Pixels `(x, y)` das Rechteck?
    ///
    /// Geprüft werden die vier Ecken der Pixelzelle im User-Space. Damit stimmt
    /// die Zuordnung auch bei gedrehter oder gescherter CTM, und im Zweifel
    /// wird ein Pixel zu viel geschwärzt statt eines zu wenig.
    fn covers(&self, ctm: &Matrix, x: usize, y: usize, rect: &Rect) -> bool {
        let w = self.width as f64;
        let h = self.height as f64;
        let u0 = x as f64 / w;
        let u1 = (x + 1) as f64 / w;
        let v0 = 1.0 - (y + 1) as f64 / h;
        let v1 = 1.0 - y as f64 / h;
        [(u0, v0), (u1, v0), (u0, v1), (u1, v1)]
            .iter()
            .any(|(u, v)| rect.contains(ctm.apply(*u, *v)))
    }
}

/// Umschließendes Rechteck der Zielfläche einer Bild-CTM.
fn ctm_bounds(ctm: &Matrix) -> Rect {
    let points = [
        ctm.apply(0.0, 0.0),
        ctm.apply(1.0, 0.0),
        ctm.apply(0.0, 1.0),
        ctm.apply(1.0, 1.0),
    ];
    let mut rect = Rect::from_corners(points[0], points[1]);
    for p in &points[2..] {
        rect = rect.union(&Rect::from_corners(*p, *p));
    }
    rect
}

fn same_matrix(a: &Matrix, b: &Matrix) -> bool {
    let close = |x: f64, y: f64| (x - y).abs() <= 1e-9 * (1.0 + x.abs().max(y.abs()));
    close(a.a, b.a)
        && close(a.b, b.b)
        && close(a.c, b.c)
        && close(a.d, b.d)
        && close(a.e, b.e)
        && close(a.f, b.f)
}

// ---------------------------------------------------------------------------
// Neu kodieren
// ---------------------------------------------------------------------------

/// Ein fertig kodiertes Bild, noch ohne Objektnummern.
struct Encoded {
    dict: Dictionary,
    data: Vec<u8>,
    /// Alphakanal als eigenes `/SMask`-Bild, falls das Original eines hatte.
    smask: Option<(Dictionary, Vec<u8>)>,
}

/// Baut aus den bearbeiteten Pixeln ein Bild-XObject.
///
/// Immer `FlateDecode`, also verlustfrei. Ein ursprüngliches `/DCTDecode`-Bild
/// wird dabei zu Flate — siehe Modulkopf.
fn encode_xobject(work: &Work) -> Encoded {
    let mut dict = Dictionary::new();
    dict.set("Type", Object::Name(b"XObject".to_vec()));
    dict.set("Subtype", Object::Name(b"Image".to_vec()));
    dict.set("Width", Object::Integer(i64::from(work.width)));
    dict.set("Height", Object::Integer(i64::from(work.height)));

    if work.is_mask {
        dict.set("ImageMask", Object::Boolean(true));
        dict.set("BitsPerComponent", Object::Integer(1));
        // Ausdrücklich der Standard: die Null malt.
        dict.set(
            "Decode",
            Object::Array(vec![Object::Integer(0), Object::Integer(1)]),
        );
        return Encoded {
            dict,
            data: mask_bits(work),
            smask: None,
        };
    }

    dict.set("BitsPerComponent", Object::Integer(8));
    let (space, data) = samples(work);
    dict.set("ColorSpace", Object::Name(space.to_vec()));

    let transparent = work.rgba.chunks_exact(4).any(|p| p[3] != 255);
    let smask = transparent.then(|| {
        let mut mask = Dictionary::new();
        mask.set("Type", Object::Name(b"XObject".to_vec()));
        mask.set("Subtype", Object::Name(b"Image".to_vec()));
        mask.set("Width", Object::Integer(i64::from(work.width)));
        mask.set("Height", Object::Integer(i64::from(work.height)));
        mask.set("ColorSpace", Object::Name(b"DeviceGray".to_vec()));
        mask.set("BitsPerComponent", Object::Integer(8));
        let alpha: Vec<u8> = work.rgba.chunks_exact(4).map(|p| p[3]).collect();
        (mask, alpha)
    });

    Encoded { dict, data, smask }
}

/// Graustufen, wenn alle Pixel unbunt sind — sonst RGB. Spart bei Scans zwei
/// Drittel der Daten.
fn samples(work: &Work) -> (&'static [u8], Vec<u8>) {
    if work
        .rgba
        .chunks_exact(4)
        .all(|p| p[0] == p[1] && p[1] == p[2])
    {
        let data = work.rgba.chunks_exact(4).map(|p| p[0]).collect();
        (b"DeviceGray", data)
    } else {
        let mut data = Vec::with_capacity(work.rgba.len() / 4 * 3);
        for p in work.rgba.chunks_exact(4) {
            data.extend_from_slice(&p[..3]);
        }
        (b"DeviceRGB", data)
    }
}

/// Stencil-Maske zurück in Bits: Alpha > 127 heißt „malt“, und bei `/Decode
/// [0 1]` malt die Null.
fn mask_bits(work: &Work) -> Vec<u8> {
    let width = work.width as usize;
    let height = work.height as usize;
    let stride = width.div_ceil(8);
    let mut data = vec![0xFFu8; stride * height];
    for y in 0..height {
        for x in 0..width {
            if work.rgba[(y * width + x) * 4 + 3] > 127 {
                data[y * stride + x / 8] &= !(1u8 << (7 - (x % 8)));
            }
        }
    }
    data
}

/// Legt die Streams an und liefert das fertige Bild-XObject.
fn build_stream(doc: &mut Document, encoded: Encoded) -> Stream {
    let Encoded {
        mut dict,
        data,
        smask,
    } = encoded;
    if let Some((mask_dict, mask_data)) = smask {
        let mut mask = Stream::new(mask_dict, mask_data);
        let _ = mask.compress();
        let mask_id = doc.add_object(Object::Stream(mask));
        dict.set("SMask", Object::Reference(mask_id));
    }
    let mut stream = Stream::new(dict, data);
    let _ = stream.compress();
    stream
}

/// Baut ein Inline-Bild neu auf: `BI`-Dictionary und Nutzdaten.
///
/// Die Daten werden hexkodiert (`/AHx`) über den Flate-Strom gelegt. Das
/// verdoppelt zwar die Länge, aber ein Inline-Bild endet an einem `EI` — und
/// binäre Nutzdaten können ein solches Paar zufällig enthalten. Zusätzlich
/// steht die Länge in `/L`.
fn encode_inline(work: &Work) -> Result<(Dictionary, Vec<u8>)> {
    let mut dict = Dictionary::new();
    dict.set("W", Object::Integer(i64::from(work.width)));
    dict.set("H", Object::Integer(i64::from(work.height)));
    let raw = if work.is_mask {
        dict.set("IM", Object::Boolean(true));
        dict.set("BPC", Object::Integer(1));
        dict.set(
            "D",
            Object::Array(vec![Object::Integer(0), Object::Integer(1)]),
        );
        mask_bits(work)
    } else {
        let (space, data) = samples(work);
        dict.set("BPC", Object::Integer(8));
        dict.set(
            "CS",
            Object::Name(if space == b"DeviceGray" {
                b"G".to_vec()
            } else {
                b"RGB".to_vec()
            }),
        );
        data
    };

    let payload = to_ascii_hex(&deflate(&raw)?);
    dict.set(
        "F",
        Object::Array(vec![
            Object::Name(b"AHx".to_vec()),
            Object::Name(b"Fl".to_vec()),
        ]),
    );
    dict.set("L", Object::Integer(payload.len() as i64));
    Ok((dict, payload))
}

fn deflate(data: &[u8]) -> Result<Vec<u8>> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(data)
        .and_then(|_| encoder.finish())
        .map_err(|e| RedactError::Pdf(format!("Bilddaten nicht komprimierbar: {e}")))
}

fn to_ascii_hex(data: &[u8]) -> Vec<u8> {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = Vec::with_capacity(data.len() * 2 + 1);
    for byte in data {
        out.push(DIGITS[(byte >> 4) as usize]);
        out.push(DIGITS[(byte & 0x0F) as usize]);
    }
    out.push(b'>');
    out
}

// ---------------------------------------------------------------------------
// Verweise umbiegen
// ---------------------------------------------------------------------------

/// Setzt `/Resources /XObject /name` der Seite auf die Kopie.
///
/// Die (ggf. geerbten) Ressourcen werden dabei als eigenes Dictionary an der
/// Seite verankert — sonst würde die Änderung auch die Nachbarseiten treffen,
/// die sich dasselbe Ressourcen-Objekt teilen.
fn repoint_page(
    doc: &mut Document,
    page_id: ObjectId,
    name: &[u8],
    new_id: ObjectId,
) -> Result<()> {
    let mut resources = crate::content::page_resources(doc, page_id).unwrap_or_default();
    let mut xobjects = resources
        .get(b"XObject")
        .ok()
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok())
        .cloned()
        .unwrap_or_default();
    xobjects.set(name.to_vec(), Object::Reference(new_id));
    resources.set("XObject", Object::Dictionary(xobjects));
    doc.get_dictionary_mut(page_id)
        .map_err(|e| RedactError::Pdf(format!("Seite nicht lesbar: {e}")))?
        .set("Resources", Object::Dictionary(resources));
    Ok(())
}

/// Dasselbe im `/Resources` eines Form-XObjects.
///
/// Liefert `false`, wenn das Formular gar keine eigenen Ressourcen hat — dann
/// benutzt es die der Seite, und dort muss umgebogen werden.
fn repoint_form(
    doc: &mut Document,
    form_id: ObjectId,
    name: &[u8],
    new_id: ObjectId,
) -> Result<bool> {
    let Some(mut resources) = doc
        .get_object(form_id)
        .ok()
        .and_then(|o| o.as_stream().ok())
        .and_then(|s| s.dict.get(b"Resources").ok().cloned())
        .and_then(|o| doc.dereference(&o).ok().map(|(_, r)| r.clone()))
        .and_then(|o| o.as_dict().ok().cloned())
    else {
        return Ok(false);
    };
    let mut xobjects = resources
        .get(b"XObject")
        .ok()
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok())
        .cloned()
        .unwrap_or_default();
    xobjects.set(name.to_vec(), Object::Reference(new_id));
    resources.set("XObject", Object::Dictionary(xobjects));
    if let Ok(Object::Stream(stream)) = doc.get_object_mut(form_id) {
        stream.dict.set("Resources", Object::Dictionary(resources));
        return Ok(true);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_hex_is_terminated() {
        assert_eq!(to_ascii_hex(&[0x00, 0xAB, 0xFF]), b"00ABFF>".to_vec());
    }

    #[test]
    fn deflate_roundtrips() {
        let data: Vec<u8> = (0u8..=255).cycle().take(5000).collect();
        let packed = deflate(&data).unwrap();
        let mut out = Vec::new();
        use std::io::Read;
        flate2::read::ZlibDecoder::new(packed.as_slice())
            .read_to_end(&mut out)
            .unwrap();
        assert_eq!(out, data);
    }

    #[test]
    fn unit_square_bounds() {
        let rect = ctm_bounds(&Matrix::new(100.0, 0.0, 0.0, 50.0, 10.0, 20.0));
        assert_eq!(rect, Rect::new(10.0, 20.0, 110.0, 70.0));
        // Auch bei 90°-Drehung muss die Fläche stimmen.
        let rotated = ctm_bounds(&Matrix::new(0.0, 100.0, -100.0, 0.0, 150.0, 600.0));
        assert_eq!(rotated, Rect::new(50.0, 600.0, 150.0, 700.0));
    }

    /// Ein Graustufenbild wird als Graustufenbild zurückgeschrieben — bei
    /// Scans macht das den Unterschied zwischen 1 und 3 Byte je Pixel.
    #[test]
    fn gray_pixels_stay_gray() {
        let work = Work {
            width: 2,
            height: 1,
            rgba: vec![7, 7, 7, 255, 200, 200, 200, 255],
            is_mask: false,
            name: Vec::new(),
            page_id: (1, 0),
            streams: BTreeSet::new(),
            filled: 0,
        };
        let (space, data) = samples(&work);
        assert_eq!(space, b"DeviceGray");
        assert_eq!(data, vec![7, 200]);
    }

    #[test]
    fn mask_bits_round_trip() {
        // Pixel 0 malt (Alpha 255), Pixel 1 nicht.
        let work = Work {
            width: 2,
            height: 1,
            rgba: vec![0, 0, 0, 255, 0, 0, 0, 0],
            is_mask: true,
            name: Vec::new(),
            page_id: (1, 0),
            streams: BTreeSet::new(),
            filled: 0,
        };
        // Bit 0 = 0 (malt), Bit 1 = 1 (malt nicht), Rest Füllbits.
        assert_eq!(mask_bits(&work), vec![0b0111_1111]);
    }
}
