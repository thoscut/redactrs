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
//!  Seite ──interpret──► Platzierungen (Name/Inline, CTM, Stream, Ressourcen)
//!                            │
//!                  schneidet eine Schwärzung diese Fläche?  ── nein ──► fertig
//!                            │ ja
//!            ops::decode_image ──► genau *dieses* Bild, dekodiert (RGBA8)
//!                            │
//!                       Pixel füllen  (inverse CTM je Platzierung)
//!                            │
//!         neu kodieren (Flate), XObject ersetzen/kopieren, Puffer freigeben
//! ```
//!
//! ## Entscheidungen
//!
//! * **Es wird nur dekodiert, was eine Schwärzung wirklich schneidet.** Früher
//!   lief hier [`crate::ops::page_ops`], das *jedes* Bild der Seite nach RGBA8
//!   auspackt und alle gleichzeitig hält; 19 unbeteiligte Bilder kosteten so
//!   2,9 GB. Heute entscheidet der Schnitt zwischen Bildfläche und
//!   Schwärzungsbereich je Platzierung, und die Bildbytes stehen nur zwischen
//!   dem Dekodieren und dem Zurückschreiben *eines* Bildes im Speicher.
//! * **Ein Bild nach dem anderen.** Sobald ein Bild gefüllt ist, wird es neu
//!   kodiert, in das Dokument geschrieben und sein RGBA-Puffer freigegeben —
//!   erst dann kommt das nächste. Die einzige Ausnahme ist ein Inline-Bild in
//!   einem Form-XObject: das kann von zwei Seiten aus geschwärzt werden und
//!   muss deshalb bis zum Ende gehalten werden.
//! * **Es gibt eine harte Obergrenze.** [`ImageOptions::max_decoded_bytes`]
//!   begrenzt die Summe der gleichzeitig gehaltenen dekodierten Bildbytes.
//!   Wird sie überschritten, endet der Lauf mit einer Meldung — nicht mit
//!   einer gescheiterten Speicheranforderung. Siehe `SECURITY.md`.
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
use std::rc::Rc;

use lopdf::content::Operation;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use redact_core::{Rect, RedactError, Redaction, Result};

use crate::content::{ContentSink, ImageEvent, SinkContext, StreamKey};
use crate::matrix::Matrix;
use crate::ops::{RasterImage, Rgb};

/// Wie tief die Suche nach Bildern in verschachtelte Form-XObjects steigt.
const MAX_RESOURCE_DEPTH: usize = 8;

/// Vorgabe für [`ImageOptions::max_decoded_bytes`]: 256 MB.
///
/// Ein dekodiertes Bild kostet 4 Byte je Bildpunkt. Das größte Bild, das
/// [`crate::ops`] überhaupt auspackt, hat 40 000 000 Bildpunkte, also 160 MB —
/// die Vorgabe lässt genau eines davon zu und noch etwas Luft für ein
/// gleichzeitig gehaltenes Inline-Bild aus einem Form-XObject. Alles darüber
/// ist eine bewusste Entscheidung des Aufrufers (`--max-image-mb`).
pub const DEFAULT_MAX_DECODED_IMAGE_BYTES: u64 = 256 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Schnittstelle
// ---------------------------------------------------------------------------

/// Stellschrauben der Bild-Schwärzung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageOptions {
    /// Nicht dekodierbare Bilder (`JPXDecode`, `CCITTFaxDecode`, defekte
    /// Streams) durchgehen lassen, statt abzubrechen.
    ///
    /// **Unsicher.** Das Bild bleibt dann unverändert in der Datei; die
    /// Schwärzung liegt nur obenauf und ist rückgängig zu machen. Gedacht für
    /// Aufrufer, die das bewusst in Kauf nehmen — die Kommandozeile bietet es
    /// (noch) nicht an.
    pub allow_undecodable: bool,
    /// Obergrenze für die Summe der **gleichzeitig** gehaltenen dekodierten
    /// Bildbytes (RGBA8, 4 Byte je Bildpunkt).
    ///
    /// Geprüft wird **vor** dem Auspacken, anhand von `/Width` und `/Height`
    /// aus dem Bild-Dictionary. Reicht das Budget nicht, endet der Lauf mit
    /// einem Fehler statt mit einer gescheiterten Speicheranforderung.
    ///
    /// **Was die Grenze nicht abdeckt:** die Puffer, die *während* des
    /// Umkodierens eines einzelnen Bildes zusätzlich entstehen (entpackte
    /// Abtastwerte, die Graustufen- bzw. RGB-Bytes vor dem Deflate). Sie
    /// betragen zusammen rund das Anderthalbfache eines Bildes; der wirkliche
    /// Spitzenbedarf liegt also über dem hier genannten Wert. Die Grenze ist
    /// ein Riegel gegen das *Anhäufen* vieler Bilder, keine Zusage über den
    /// Gesamtverbrauch des Prozesses.
    pub max_decoded_bytes: u64,
}

impl Default for ImageOptions {
    fn default() -> Self {
        Self {
            allow_undecodable: false,
            max_decoded_bytes: DEFAULT_MAX_DECODED_IMAGE_BYTES,
        }
    }
}

/// Was die Bild-Schwärzung getan hat.
#[derive(Debug, Clone, Default)]
pub struct ImageOutcome {
    /// Anzahl Bilder, deren Pixel überschrieben wurden.
    pub redacted_images: usize,
    /// Davon: Bilder, die kopiert werden mussten, weil sie mehrfach benutzt
    /// werden.
    pub copied_images: usize,
    /// Höchstzahl der **gleichzeitig** dekodiert gehaltenen Bilder.
    ///
    /// Der Speicherbedarf selbst lässt sich im Test kaum messen; diese Zahl
    /// schon. Sie muss 1 sein, solange kein Inline-Bild eines Form-XObjects im
    /// Spiel ist — steigt sie, sammelt jemand wieder Bilder an.
    pub peak_decoded_images: usize,
    /// Dasselbe in Bytes (RGBA8).
    pub peak_decoded_bytes: u64,
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

    // Vorab: welche Schwärzung liegt auf welcher Seite. Einmal gebildet statt
    // je Seite aus der vollständigen Liste gefiltert.
    let zones_per_page = zones_by_page(redactions, padding);

    // Phase 1 — Bestandsaufnahme über *alle* Seiten. Erst danach ist bekannt,
    // ob ein Bild nur an einer Stelle benutzt wird (dann darf es überschrieben
    // werden) oder mehrfach (dann muss kopiert werden). Dekodiert wird hier
    // noch nichts; die Rohdaten eines Inline-Bildes werden nur dann behalten,
    // wenn eine Schwärzung seine Fläche überhaupt schneidet.
    let no_zones: Vec<Zone> = Vec::new();
    let mut per_page: Vec<Vec<Placement>> = Vec::with_capacity(pages.len());
    let mut image_pages: BTreeMap<ObjectId, BTreeSet<usize>> = BTreeMap::new();
    let mut form_pages: BTreeMap<ObjectId, BTreeSet<usize>> = BTreeMap::new();
    for (index, page_id) in pages.iter().enumerate() {
        let zones = zones_per_page.get(&index).unwrap_or(&no_zones);
        let (placements, forms) = scan_page_images(doc, *page_id, zones);
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

    // Phase 2 — füllen und **sofort** schreiben. `works` hält nur, was sich
    // nicht sofort abschließen lässt: ein Inline-Bild in einem Form-XObject
    // kann von zwei Seiten aus geschwärzt werden und muss deshalb einmal mit
    // den Bereichen beider Seiten geschrieben werden.
    let mut works: BTreeMap<Key, Work> = BTreeMap::new();
    let mut budget = Budget::new(options.max_decoded_bytes);
    for (index, page_id) in pages.iter().enumerate() {
        let zones = zones_per_page.get(&index).unwrap_or(&no_zones);
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
            zones,
            options,
            &mut works,
            &mut budget,
            &mut outcome,
            &image_pages,
            &form_pages,
        )?;
    }

    // Phase 3 — der Rest: die über Seitengrenzen gehaltenen Inline-Bilder.
    let leftovers: Vec<(Key, Work)> = std::mem::take(&mut works).into_iter().collect();
    for (key, work) in leftovers {
        budget.release(&work);
        write_work(doc, key, work, &image_pages, &form_pages, &mut outcome)?;
    }

    outcome.peak_decoded_images = budget.peak_images;
    outcome.peak_decoded_bytes = budget.peak_bytes;
    Ok(outcome)
}

/// Schreibt ein fertig gefülltes Bild in das Dokument und gibt seine Bytes frei.
///
/// Das ist der Schritt, der früher erst ganz am Ende für *alle* Bilder lief.
/// Jetzt läuft er, sobald ein Bild fertig ist — deshalb liegt zu jedem
/// Zeitpunkt höchstens ein dekodiertes Bild im Speicher.
fn write_work(
    doc: &mut Document,
    key: Key,
    work: Work,
    image_pages: &BTreeMap<ObjectId, BTreeSet<usize>>,
    form_pages: &BTreeMap<ObjectId, BTreeSet<usize>>,
    outcome: &mut ImageOutcome,
) -> Result<()> {
    if work.filled == 0 {
        return Ok(());
    }
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
    Ok(())
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
        /// `/Width` × `/Height` laut Dictionary — für die Budgetprüfung, die
        /// *vor* dem Auspacken greifen muss.
        pixels: u64,
    },
    Inline {
        is_mask: bool,
        filters: String,
        pixels: u64,
        /// Das `BI`-Dictionary. Es steht im Content-Stream und ist beim
        /// Dekodieren nicht mehr greifbar.
        dict: Dictionary,
        /// Die (noch gefilterten) Rohdaten — nur dann behalten, wenn eine
        /// Schwärzung diese Fläche überhaupt schneidet. Sonst hielte eine
        /// Datei mit vielen unbeteiligten Inline-Bildern sie alle im Speicher.
        data: Option<Vec<u8>>,
    },
}

#[derive(Debug, Clone)]
struct Placement {
    stream: StreamKey,
    op_index: usize,
    ctm: Matrix,
    /// Die Ressourcen des Stroms, in dem gezeichnet wird. Das Dekodieren
    /// braucht sie für benannte Farbräume und `/SMask`; mehrere Platzierungen
    /// desselben Stroms teilen sich eine Kopie.
    resources: Option<Rc<Dictionary>>,
    /// Füllfarbe an dieser Stelle — eine Stencil-Maske malt damit.
    fill: Rgb,
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

    fn pixels(&self) -> u64 {
        match &self.target {
            Target::XObject { pixels, .. } | Target::Inline { pixels, .. } => *pixels,
        }
    }

    fn is_mask(&self) -> bool {
        match &self.target {
            Target::XObject { is_mask, .. } | Target::Inline { is_mask, .. } => *is_mask,
        }
    }

    fn name(&self) -> Vec<u8> {
        match &self.target {
            Target::XObject { name, .. } => name.clone(),
            Target::Inline { .. } => Vec::new(),
        }
    }
}

struct Collector<'a> {
    placements: Vec<Placement>,
    forms: BTreeSet<ObjectId>,
    /// Nur Bilder, deren Fläche eine dieser Zonen schneidet, brauchen später
    /// ihre Rohdaten.
    zones: &'a [Zone],
    /// Bereits gesehene Ressourcen-Dictionaries. Ein Strom hat genau eines;
    /// zwanzig Platzierungen darin sollen es nicht zwanzigmal kopieren.
    resource_cache: Vec<Rc<Dictionary>>,
}

impl Collector<'_> {
    fn resources(&mut self, resources: Option<&Dictionary>) -> Option<Rc<Dictionary>> {
        let resources = resources?;
        if let Some(found) = self.resource_cache.iter().find(|c| c.as_ref() == resources) {
            return Some(Rc::clone(found));
        }
        let shared = Rc::new(resources.clone());
        self.resource_cache.push(Rc::clone(&shared));
        Some(shared)
    }

    fn touches_a_zone(&self, ctm: &Matrix) -> bool {
        let bounds = ctm_bounds(ctm);
        self.zones.iter().any(|z| bounds.intersects(&z.rect))
    }
}

impl ContentSink for Collector<'_> {
    fn wants_graphics(&self) -> bool {
        true
    }

    fn form(&mut self, id: ObjectId) {
        self.forms.insert(id);
    }

    fn image(&mut self, cx: &SinkContext, event: &ImageEvent) {
        // Dieselben Abbruchbedingungen wie in `ops::OpsCollector::image` —
        // sonst zählen die beiden Läufe verschieden viele Bilder.
        let target = match event.inline {
            Some((dict, data)) => Target::Inline {
                is_mask: dict_flag(dict, b"ImageMask", b"IM"),
                filters: filter_label(dict),
                pixels: declared_pixels(cx.doc, dict),
                dict: dict.clone(),
                data: self.touches_a_zone(&event.ctm).then(|| data.to_vec()),
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
                    pixels: declared_pixels(cx.doc, &dict),
                }
            }
        };
        let resources = self.resources(cx.resources);
        self.placements.push(Placement {
            stream: cx.stream,
            op_index: cx.op_index,
            ctm: event.ctm,
            resources,
            fill: event.fill,
            target,
        });
    }
}

fn scan_page_images(
    doc: &Document,
    page_id: ObjectId,
    zones: &[Zone],
) -> (Vec<Placement>, BTreeSet<ObjectId>) {
    let Ok(data) = doc.get_page_content(page_id) else {
        return (Vec::new(), BTreeSet::new());
    };
    let operations = crate::ops::decode_content(&data);
    let resources = crate::content::page_resources(doc, page_id);
    let mut collector = Collector {
        placements: Vec::new(),
        forms: BTreeSet::new(),
        zones,
        resource_cache: Vec::new(),
    };
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

/// `/Width` × `/Height` laut Dictionary, ohne irgendetwas auszupacken.
///
/// Grundlage der Budgetprüfung: sie muss entscheiden können, *bevor* der
/// Puffer angefordert wird. Fehlt eine Angabe oder ist sie unsinnig, zählt 0 —
/// das Dekodieren liefert dann ohnehin nur einen Platzhalter.
fn declared_pixels(doc: &Document, dict: &Dictionary) -> u64 {
    let value = |long: &[u8], short: &[u8]| -> u64 {
        dict.get(long)
            .or_else(|_| dict.get(short))
            .ok()
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| o.as_i64().ok())
            .unwrap_or(0)
            .max(0) as u64
    };
    value(b"Width", b"W").saturating_mul(value(b"Height", b"H"))
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

/// Alle Schwärzungsbereiche, nach Seite sortiert — einmal statt je Seite neu
/// aus der vollständigen Liste gefiltert.
fn zones_by_page(redactions: &[Redaction], padding: f64) -> BTreeMap<usize, Vec<Zone>> {
    let mut out: BTreeMap<usize, Vec<Zone>> = BTreeMap::new();
    for redaction in redactions {
        let rect = redaction.region.rect.expanded(padding);
        if rect.is_empty() {
            continue;
        }
        let (red, green, blue) = redaction.action.fill_color().unwrap_or((0.0, 0.0, 0.0));
        let to_u8 = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        out.entry(redaction.region.page).or_default().push(Zone {
            rect,
            color: [to_u8(red), to_u8(green), to_u8(blue)],
        });
    }
    out
}

/// Buchführung über die gleichzeitig gehaltenen dekodierten Bildbytes.
///
/// Der Sinn ist ein **kontrollierter** Abbruch: geprüft wird vor dem
/// Auspacken, aus `/Width` und `/Height`. Eine Datei, die mehr verlangt, endet
/// mit einer Meldung statt mit `memory allocation of … bytes failed`.
struct Budget {
    limit: u64,
    held: u64,
    held_images: usize,
    /// Höchststände — die einzige im Test messbare Zusicherung darüber, dass
    /// wirklich ein Bild nach dem anderen bearbeitet wird.
    peak_bytes: u64,
    peak_images: usize,
}

impl Budget {
    fn new(limit: u64) -> Self {
        Self {
            limit,
            held: 0,
            held_images: 0,
            peak_bytes: 0,
            peak_images: 0,
        }
    }

    /// Fordert Platz für ein Bild an. `pixels` ist die Zahl der Bildpunkte
    /// laut Dictionary; gerechnet wird mit 4 Byte je Punkt (RGBA8).
    fn reserve(&mut self, pixels: u64, what: &str) -> Result<()> {
        let needed = pixels.saturating_mul(4);
        if self.held.saturating_add(needed) > self.limit {
            let mb = |bytes: u64| bytes as f64 / (1024.0 * 1024.0);
            return Err(RedactError::Pdf(format!(
                "{what} bräuchte {:.0} MB dekodierte Bildpunkte; zusammen mit den bereits \
                 gehaltenen {:.0} MB überschreitet das die Grenze von {:.0} MB. Der Lauf \
                 wird abgebrochen, bevor die Speicheranforderung scheitert — mit \
                 --max-image-mb lässt sich die Grenze bewusst anheben.",
                mb(needed),
                mb(self.held),
                mb(self.limit),
            )));
        }
        self.held += needed;
        Ok(())
    }

    /// Nimmt zurück, was [`Budget::reserve`] veranschlagt hatte, und bucht
    /// stattdessen die tatsächliche Puffergröße.
    fn settle(&mut self, reserved_pixels: u64, actual: &Work) {
        self.cancel(reserved_pixels);
        self.held = self.held.saturating_add(actual.rgba.len() as u64);
        self.held_images += 1;
        self.peak_bytes = self.peak_bytes.max(self.held);
        self.peak_images = self.peak_images.max(self.held_images);
    }

    /// Gibt eine Reservierung zurück, aus der nichts geworden ist.
    fn cancel(&mut self, reserved_pixels: u64) {
        self.held = self.held.saturating_sub(reserved_pixels.saturating_mul(4));
    }

    fn release(&mut self, work: &Work) {
        self.held = self.held.saturating_sub(work.rgba.len() as u64);
        self.held_images = self.held_images.saturating_sub(1);
    }
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

/// Schwärzt die Bilder **einer** Seite — und zwar nur die, die eine Zone
/// wirklich schneidet.
///
/// Der Ablauf ist bewusst nach Zielobjekt gruppiert und nicht nach
/// Platzierung: dasselbe Bild kann auf einer Seite mehrfach gezeichnet sein
/// und darf trotzdem nur einmal ausgepackt werden. Ist eine Gruppe fertig,
/// wird sie sofort geschrieben und ihr Puffer freigegeben.
#[allow(clippy::too_many_arguments)]
fn fill_page(
    doc: &mut Document,
    page_index: usize,
    page_id: ObjectId,
    placements: &[Placement],
    zones: &[Zone],
    options: &ImageOptions,
    works: &mut BTreeMap<Key, Work>,
    budget: &mut Budget,
    outcome: &mut ImageOutcome,
    image_pages: &BTreeMap<ObjectId, BTreeSet<usize>>,
    form_pages: &BTreeMap<ObjectId, BTreeSet<usize>>,
) -> Result<()> {
    // 1. Zuordnen: welche Platzierung trifft welche Zonen, und auf welches
    //    Zielobjekt zeigt sie? Hier wird noch nichts ausgepackt.
    let mut order: Vec<Key> = Vec::new();
    let mut groups: BTreeMap<Key, Vec<(usize, Vec<Zone>)>> = BTreeMap::new();
    for (index, placement) in placements.iter().enumerate() {
        let bounds = ctm_bounds(&placement.ctm);
        let touching: Vec<Zone> = zones
            .iter()
            .filter(|z| bounds.intersects(&z.rect))
            .copied()
            .collect();
        if touching.is_empty() {
            continue;
        }
        let key = match &placement.target {
            Target::XObject { id: Some(id), .. } => Key::XObject(page_index, *id),
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
            Target::Inline { .. } => {
                let target = match placement.stream {
                    StreamKey::Page => InlineTarget::Page(page_id),
                    StreamKey::Form(form) => InlineTarget::Form(form),
                };
                Key::Inline(target, placement.op_index)
            }
        };
        groups
            .entry(key)
            .or_insert_with(|| {
                order.push(key);
                Vec::new()
            })
            .push((index, touching));
    }

    // 2. Ein Bild nach dem anderen: auspacken, füllen, schreiben, freigeben.
    for key in order {
        let entries = groups.remove(&key).unwrap_or_default();
        let Some((first, _)) = entries.first() else {
            continue;
        };
        let first = &placements[*first];

        // Ein Inline-Bild in einem Form-XObject kann schon von einer früheren
        // Seite her in Arbeit sein; dann wird derselbe Puffer weiterbenutzt.
        let mut work = match works.remove(&key) {
            Some(work) => work,
            None => {
                // Was [`crate::ops`] ohnehin nicht auspackt, kostet auch kein
                // Budget — und soll „zu groß“ melden statt „Grenze
                // überschritten“.
                let reserved = if first.pixels() <= crate::ops::MAX_IMAGE_PIXELS {
                    budget.reserve(first.pixels(), &first.label(page_index))?;
                    first.pixels()
                } else {
                    0
                };
                let (raster, note) = decode_placement(doc, first);
                if raster.placeholder {
                    budget.cancel(reserved);
                    // Der **echte** Grund steht in `note` — der Filter ist nur
                    // der häufigste Fall, nicht der einzige. Ein Bild über
                    // `MAX_IMAGE_PIXELS` etwa hat einen tadellosen Filter.
                    let reason = note.unwrap_or_else(|| format!("Filter: {}", first.filters()));
                    let message = format!(
                        "{} lässt sich nicht dekodieren ({reason}). Die Schwärzung läge nur \
                         darüber; die Pixel blieben in der Datei.",
                        first.label(page_index),
                    );
                    if options.allow_undecodable {
                        outcome.warnings.push(message);
                        continue;
                    }
                    return Err(RedactError::Pdf(message));
                }
                let work = Work {
                    width: raster.width,
                    height: raster.height,
                    rgba: raster.rgba,
                    is_mask: first.is_mask(),
                    name: first.name(),
                    page_id,
                    streams: BTreeSet::new(),
                    filled: 0,
                };
                budget.settle(reserved, &work);
                work
            }
        };

        for (index, touching) in &entries {
            let placement = &placements[*index];
            work.streams.insert(placement.stream);
            work.fill(&placement.ctm, touching);
        }

        // Ein Inline-Bild in einem Form-XObject wird von der nächsten Seite
        // vielleicht noch gebraucht — alles andere ist hier fertig.
        if matches!(key, Key::Inline(InlineTarget::Form(_), _)) {
            works.insert(key, work);
        } else {
            budget.release(&work);
            write_work(doc, key, work, image_pages, form_pages, outcome)?;
        }
    }
    Ok(())
}

/// Packt genau das Bild aus, auf das diese Platzierung zeigt.
///
/// Der Rückgabewert ist derselbe wie bei [`crate::ops::decode_image`]: das
/// Bild und, falls es nicht ging, der Grund dafür.
fn decode_placement(doc: &Document, placement: &Placement) -> (RasterImage, Option<String>) {
    let resources = placement.resources.as_deref();
    match &placement.target {
        Target::Inline { dict, data, .. } => {
            let Some(data) = data else {
                // Kann nicht vorkommen: die Rohdaten werden genau dann
                // behalten, wenn die Fläche eine Zone schneidet — und nur
                // dann kommt diese Funktion überhaupt her.
                return (
                    RasterImage::placeholder(),
                    Some("Rohdaten des Inline-Bildes nicht mitgeführt".into()),
                );
            };
            crate::ops::decode_image(doc, resources, dict, data, placement.fill)
        }
        Target::XObject { name, .. } => {
            let Some(stream) = image_stream(doc, resources, name) else {
                return (
                    RasterImage::placeholder(),
                    Some("das Bild-XObject ist nicht mehr auflösbar".into()),
                );
            };
            crate::ops::decode_image(
                doc,
                resources,
                &stream.dict,
                &stream.content,
                placement.fill,
            )
        }
    }
}

/// Wie [`image_xobject`], liefert aber den Strom samt Nutzdaten.
fn image_stream<'a>(
    doc: &'a Document,
    resources: Option<&'a Dictionary>,
    name: &[u8],
) -> Option<&'a Stream> {
    let xobjects = resources?.get(b"XObject").ok()?;
    let (_, xobjects) = doc.dereference(xobjects).ok()?;
    let entry = xobjects.as_dict().ok()?.get(name).ok()?;
    let (_, resolved) = doc.dereference(entry).ok()?;
    let stream = resolved.as_stream().ok()?;
    if stream.dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Image") {
        return None;
    }
    Some(stream)
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

// Der frühere `same_matrix` verglich die Bild-CTM zweier Durchläufe. Es gibt
// nur noch einen: die Platzierung trägt jetzt selbst, was zum Dekodieren nötig
// ist, und kann deshalb gar nicht mehr mit einem fremden Bild gepaart werden.

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
