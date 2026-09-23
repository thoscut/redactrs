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
//! * **Was die Eingabe versteckt, bleibt versteckt.** Ein Bild kann Bildpunkte
//!   unsichtbar machen: `/SMask`, ein `/Mask` als Stencil-Strom oder ein
//!   `/Mask` als Farbschlüssel. Beim Neuaufbau des Dictionarys ist weg, was
//!   nicht ausdrücklich mitkommt — und eine verlorene Maske ist die Umkehrung
//!   des Kernversprechens: gerade eine Stelle, die die Eingabe absichtlich
//!   verdeckt, ist das Muster einer bereits mit einem anderen Werkzeug
//!   geschwärzten Stelle. Deshalb: ein `/Mask`-Strom wird **unverändert
//!   mitgeschrieben** (er beschreibt das Bild im Einheitsquadrat und bleibt
//!   dabei in voller Auflösung), ein Farbschlüssel wird beim Dekodieren in den
//!   Alphakanal gerechnet und als `/SMask` neu geschrieben — er darf nicht
//!   mitkommen, weil er Abtastwerte benennt und die geschwärzten Bildpunkte
//!   sonst durchsichtig würden. Was sich so nicht sicher übertragen lässt,
//!   beendet den Lauf mit einer Meldung ([`crate::ops::MaskPlan`]).

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use lopdf::content::Operation;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use redact_core::{Point, Rect, RedactError, Redaction, Result};

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
    /// Bilder durchgehen lassen, die sich nicht schwärzen lassen, statt
    /// abzubrechen: nicht dekodierbare (`JPXDecode`, `CCITTFaxDecode`, defekte
    /// Streams) und solche mit einer Maske, die das Neukodieren nicht
    /// überstünde ([`crate::ops::MaskPlan::Unsupported`]).
    ///
    /// **Unsicher.** Das Bild bleibt dann unverändert in der Datei; die
    /// Schwärzung liegt nur obenauf und ist rückgängig zu machen. Gedacht für
    /// Aufrufer, die das bewusst in Kauf nehmen; auf der Kommandozeile heißt
    /// der Schalter `--allow-undecodable-images`.
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
    /// **Die Wahrheit über die gefallenen Bildpunkte, im Seitenstrom.** Je
    /// Seite die Operationsindizes der `Do`/`BI`, unter denen dieser Lauf
    /// wirklich Bildpunkte überschrieben hat ([`Work::filled`] ist gewachsen,
    /// geprüft an der **Fläche** jeder Pixelzelle, [`cell_meets_rect`]).
    ///
    /// Sie steht hier, weil nur diese Stelle sie kennt und weil außerhalb
    /// etwas daran hängt: der Ersatztext eines Bildes (`/Alt`,
    /// `/ActualText` — am Bilddictionary und am Marked-Content-Abschnitt
    /// darüber) überlebt die Pixel-Schwärzung und trägt oft genau, was zu
    /// sehen war. Er darf fallen, wenn die Bildpunkte gefallen sind — und
    /// sonst nicht. Zweimal wurde diese Frage draußen *geschätzt*: an der
    /// Hülle der Platzierung (zu groß — ein unversehrtes gedrehtes Bild verlor
    /// seinen Ersatztext) und am Viereck der Platzierung (zu klein an der
    /// Kante — ein Bild verlor seinen Bildpunkt und behielt seinen Spiegel).
    /// Siehe [`crate::redact`].
    ///
    /// **Was auch hier steht, ohne Wahrheit zu sein:** eine Platzierung, deren
    /// Bildpunkte sich gar nicht anfassen ließen (nicht dekodierbar, Maske
    /// nicht übertragbar, kein eigenständiges Objekt — nur mit
    /// [`ImageOptions::allow_undecodable`]). Dort weiß niemand, was fiel; dort
    /// ist grob entschieden, und dort steht eine Warnung in
    /// [`ImageOutcome::warnings`]. Grob entscheiden ist erlaubt, wenn es
    /// gesagt wird; still grob entscheiden nicht.
    pub page_image_hits: BTreeMap<ObjectId, BTreeSet<usize>>,
    /// Dasselbe je Form-XObject: die Operationsindizes in **seinem** Strom.
    ///
    /// Vollständig, sobald [`redact_images`] zurückkommt — auch über
    /// Seitengrenzen: ein Formular, dessen Bild erst von Seite 2 aus
    /// geschwärzt wird, steht hier, bevor Seite 1 gelesen wird.
    pub form_image_hits: BTreeMap<ObjectId, BTreeSet<usize>>,
    /// Bild-XObjects, deren Dictionary diesen Lauf **überlebt**, obwohl eine
    /// Schwärzung auf ihrer Fläche lag: ihre Pixel ließen sich nicht anfassen
    /// (siehe [`ImageOptions::allow_undecodable`]), also wurde das Dictionary
    /// nicht neu aufgebaut und `/Alt` und `/ActualText` stehen noch daran.
    ///
    /// Bei jedem anderen getroffenen Bild ist der Ersatztext am Dictionary mit
    /// den Pixeln gefallen — [`encode_xobject`] baut das Dictionary aus den
    /// Bildeigenschaften neu auf. Diese Liste ist deshalb genau die, die
    /// [`crate::redact`] noch eigens räumen muss.
    pub undecodable_images: BTreeSet<ObjectId>,
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
///
/// **Gibt heraus, welche Bildpunkte wirklich gefallen sind.** Nur hier ist das
/// bekannt — [`Work::covers`] prüft die **Fläche** *jeder* Pixelzelle im
/// User-Space gegen die Zone ([`cell_meets_rect`], trennende Achsen). An den
/// vier *Ecken* der Zelle hing die Frage bis zur Fix-Runde 8, und ein
/// Rechteck ganz zwischen den Gitterlinien traf dann keine. Die Antwort steht in [`ImageOutcome::page_image_hits`],
/// [`ImageOutcome::form_image_hits`] und [`ImageOutcome::undecodable_images`];
/// an ihr entscheidet [`crate::redact`] über den Ersatztext des Bildes. Weil
/// dieser Lauf *vor* der Seitenschleife steht, ist die Antwort fertig, bevor
/// irgendein Ersatztext angefasst wird — die Reihenfolge, auf die es ankommt.
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
        let (placements, forms) = scan_page_images(doc, *page_id, zones)?;
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
        //
        // **Dieselbe Frage wie überall sonst im Bildlauf**, und das ist der
        // ganze Punkt. Hier stand `ctm_bounds(&p.ctm).intersects(&z.rect)`: die
        // **Hülle**, und mit strengen Vergleichen, bei denen Berührung *nicht*
        // zählt. Eine Seite, deren Platzierung eine Zone nur berührt, wurde
        // damit ganz übersprungen — still, ohne Warnung, mit den Bildpunkten in
        // der Datei. Und ein gedrehtes Bild, dessen Hülle eine Zone schneidet,
        // obwohl kein Bildpunkt darunter liegt, kam umgekehrt herein.
        let touched = placements.iter().any(|p| {
            let quad = placement_quad(&p.ctm);
            zones.iter().any(|z| cell_meets_rect(&quad, &z.rect))
        });
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
        // Der Vermerk ist längst gesetzt: ein Inline-Bild hat keinen Namen, und
        // die Seite, auf der etwas fiel, hat ihn in ihrer eigenen Runde
        // vermerkt. Hier ist nur noch zu schreiben.
        let _ = write_work(doc, key, work, &image_pages, &form_pages, &mut outcome)?;
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
) -> Result<Option<Shown>> {
    if work.filled == 0 {
        return Ok(None);
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
            // Ein Inline-Bild steht an genau einer Operation seines Stroms und
            // hat keinen Namen: es gibt nichts umzubiegen und nichts
            // einzuschränken.
            Ok(Some(Shown::Alle))
        }
        Key::XObject(_, id) => {
            let stream = build_stream(doc, encode_xobject(&work));
            let shown = match fate(id, &work, image_pages, form_pages) {
                Fate::Overwrite => {
                    doc.objects.insert(id, Object::Stream(stream));
                    Shown::Alle
                }
                Fate::Copy => {
                    let new_id = doc.add_object(Object::Stream(stream));
                    // **Jedes** getroffene Paar, nicht das erste. Ein Name, der
                    // nicht umgebogen wird, zeigt weiter das unversehrte
                    // Original — und lag er unter der Schwärzung, stehen dort
                    // ungeschwärzte Bildpunkte mitten im Rechteck.
                    let mut erreichbar: BTreeSet<(StreamKey, Vec<u8>)> = BTreeSet::new();
                    let mut ueber_die_seite = false;
                    for (source, name) in &work.targets {
                        match source {
                            StreamKey::Page => repoint_page(doc, work.page_id, name, new_id)?,
                            StreamKey::Form(form) => {
                                if !repoint_form(doc, *form, name, new_id)? {
                                    // Das Formular hat keine eigenen Ressourcen
                                    // und benutzt die der Seite. Dort wird
                                    // umgebogen — und damit zeigt der Name auch
                                    // in jedem anderen ressourcenlosen Formular
                                    // und im Seitenstrom die Kopie. Wie weit das
                                    // reicht, ist hier nicht billig zu sagen.
                                    repoint_page(doc, work.page_id, name, new_id)?;
                                    ueber_die_seite = true;
                                }
                            }
                        }
                        erreichbar.insert((*source, name.clone()));
                    }
                    outcome.copied_images += 1;
                    if ueber_die_seite {
                        Shown::Alle
                    } else {
                        Shown::Nur(erreichbar)
                    }
                }
                Fate::OverwriteShared => {
                    // Letzter Ausweg: überschreiben. Lieber zu viel
                    // geschwärzt als eine Datei, in der die Pixel bleiben.
                    doc.objects.insert(id, Object::Stream(stream));
                    outcome.warnings.push(format!(
                        "Bild {} steckt in einem Form-XObject, das mehrere Seiten benutzen. \
                         Es wurde überschrieben — die Schwärzung wirkt deshalb auch auf die \
                         anderen Seiten.",
                        label_names(&work.targets)
                    ));
                    Shown::Alle
                }
            };
            outcome.redacted_images += 1;
            Ok(Some(shown))
        }
    }
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
    //
    // Über [`crate::filters::page_content`], nicht `Document::get_page_content`:
    // `lopdf` kennt weder `ASCIIHexDecode` noch `RunLengthDecode` und ließe
    // eine so kodierte Seite als Rohbytes stehen — ohne ein einziges `BI`.
    let data = crate::filters::page_content(doc, page_id);
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
        match crate::ops::xobject_subtype(doc, &stream.dict) {
            Some(b"Image") => return true,
            Some(b"Form") => {
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

    /// Schneidet eine Zone die Fläche dieser Platzierung?
    ///
    /// **Dieselbe Frage wie in [`fill_page`]**, und sie muss es sein. Hier
    /// stand die **Hülle** mit `Rect::intersects`, wo Berührung *nicht* zählt,
    /// während `fill_page` die Platzierung über `cell_meets_rect` aufnimmt, wo
    /// sie zählt. Für ein Inline-Bild, dessen Fläche eine Zone nur berührt,
    /// gingen die beiden Antworten auseinander: die Rohdaten wurden nicht
    /// mitgeführt (`data == None`), die Platzierung aber trotzdem aufgenommen —
    /// und [`decode_placement`] nennt genau diesen Fall „kann nicht
    /// vorkommen", liefert einen Platzhalter, und ohne
    /// `--allow-undecodable-images` **endete der Lauf mit einem Fehler und
    /// ohne Ausgabedatei**. Eine Grenze, die eine gewöhnliche Datei ablehnt,
    /// ist genauso ein Fehler wie eine Lücke. Beleg:
    /// `zm_d_bildwahrheit_gegengelesen::zwei_flaechenfragen_brechen_den_lauf_ab`.
    fn touches_a_zone(&self, ctm: &Matrix) -> bool {
        let quad = placement_quad(ctm);
        self.zones.iter().any(|z| cell_meets_rect(&quad, &z.rect))
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
) -> Result<(Vec<Placement>, BTreeSet<ObjectId>)> {
    // Derselbe Dekoder wie beim Schwärzen des Textes (`content.rs`): auf einer
    // `/ASCIIHexDecode`-kodierten Seite fand `Document::get_page_content`
    // kein `Do`, das Bild unter der Schwärzung blieb unverändert, und der
    // Bericht zählte 0 geschwärzte Bilder ohne Warnung (Befund G1-C3).
    let data = crate::filters::page_content(doc, page_id);
    let operations = crate::ops::decode_content(&data);
    let resources = crate::content::page_resources(doc, page_id);
    let mut collector = Collector {
        placements: Vec::new(),
        forms: BTreeSet::new(),
        zones,
        resource_cache: Vec::new(),
    };
    // Das Aufwandskonto des Interpreters gilt hier genauso: was der Scanner
    // nicht zu Ende lesen konnte, darf nicht als „keine Bilder gefunden“
    // durchgehen.
    crate::content::interpret(
        doc,
        &operations,
        StreamKey::Page,
        resources.as_ref(),
        Matrix::IDENTITY,
        &mut collector,
    )?;
    Ok((collector.placements, collector.forms))
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
///
/// „Gegenstück“ heißt: dieselbe Frage, dieselbe Antwort. Die Prüfung auf
/// `/Subtype` steht deshalb in [`crate::ops::xobject_subtype`] — vorher stand
/// sie fünfmal im Crate, und eine der fünf antwortete anders.
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
    if crate::ops::xobject_subtype(doc, &stream.dict) != Some(b"Image") {
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
    /// Seite, in deren Ressourcen eine Kopie verankert wird.
    page_id: ObjectId,
    /// Jede **getroffene** Platzierung als `(Strom, Name)` — und beides gehört
    /// zusammen.
    ///
    /// Ein Name allein genügt nicht: dasselbe Bildobjekt kann auf einer Seite
    /// unter **zwei** Namen liegen, und ein Strom kann denselben Namen für ein
    /// anderes Objekt führen. Umgebogen wird deshalb jedes dieser Paare
    /// ([`write_work`]). Wer nur das erste umbog, ließ unter dem zweiten Namen
    /// ungeschwärzte Bildpunkte **mitten in der Schwärzung** stehen — ein
    /// stilles Leck, gefunden an den Bildpunkten der Ausgabedatei
    /// (`zm_a_zwei_namen_ein_bild`).
    targets: BTreeSet<(StreamKey, Vec<u8>)>,
    filled: u64,
    /// Der `/Mask`-Eintrag der Eingabe, der unverändert mitgeschrieben werden
    /// muss (Stencil-Strom, siehe [`crate::ops::MaskPlan::Keep`]).
    ///
    /// `None` heißt: es gibt nichts mitzuschreiben — entweder hatte das Bild
    /// kein `/Mask`, oder die Maske steckt bereits im Alphakanal und wird von
    /// dort neu aufgebaut.
    mask: Option<Object>,
}

/// Vermerkt eine einzelne Platzierung als „hier sind Bildpunkte gefallen".
///
/// Der Vermerk geht in den Strom, in dem die Platzierung **steht** — nicht in
/// den der Seite, die sie gezeichnet hat: ein Formular wird einmal neu
/// geschrieben, und die Spiegel darin gehören ihm. Siehe
/// [`ImageOutcome::page_image_hits`].
fn note_placement(outcome: &mut ImageOutcome, page_id: ObjectId, placement: &Placement) {
    match placement.stream {
        StreamKey::Page => outcome.page_image_hits.entry(page_id).or_default(),
        StreamKey::Form(form) => outcome.form_image_hits.entry(form).or_default(),
    }
    .insert(placement.op_index);
}

/// Was mit dem Bildobjekt geschieht, wenn seine Bildpunkte gefallen sind.
///
/// Die Frage wird an **zwei** Stellen gebraucht und darf deshalb nur einmal
/// beantwortet werden: [`write_work`] schreibt danach, und [`fill_page`]
/// vermerkt danach, über welchen Platzierungen der Ersatztext fällt. Standen
/// dort zwei Antworten, fiel ein Spiegel über Bildpunkten, die unversehrt
/// sichtbar bleiben (`zl_b_pendel_beide_richtungen`) — die Fehlerklasse
/// „eine Zusage wird zur nächsten Aufrufstelle getragen, wo sie nicht gilt".
enum Fate {
    /// Das Objekt hängt an genau einer Seite: es wird überschrieben, und
    /// **jede** Platzierung dieses Objekts zeigt danach die geschwärzten
    /// Bildpunkte.
    Overwrite,
    /// Das Objekt hängt an mehreren Seiten und der Verweis lässt sich
    /// isolieren: es wird kopiert, und **jedes** getroffene `(Strom, Name)`-Paar
    /// wird auf die Kopie umgebogen ([`repoint_page`], [`repoint_form`]).
    Copy,
    /// Geteilt, aber nicht isolierbar — letzter Ausweg: überschreiben, und das
    /// mit Warnung. Auch hier zeigt jede Platzierung die geschwärzten
    /// Bildpunkte, und zwar auf allen Seiten.
    OverwriteShared,
}

/// Wo die geschwärzten Bildpunkte nach dem Schreiben **zu sehen** sind.
///
/// Das weiß erst der, der geschrieben hat: welche Paare umgebogen wurden, und
/// ob der Rückfall auf die Seitenressourcen weiter trug als das einzelne Paar.
/// Wer vorher vermerkt, rät — und genau daran hing das Pendel dieser Schleife.
enum Shown {
    /// Über **jede** Platzierung dieses Objekts auf dieser Seite. Entweder wurde
    /// das Objekt selbst überschrieben, oder ein Formular ohne eigene
    /// Ressourcen hat den Verweis der **Seite** umbiegen lassen: dann trägt die
    /// Änderung weiter als das eine Paar, und wie weit genau, ist hier nicht
    /// billig zu sagen. Über-Vermerken ist in dieser Richtung erlaubt — es
    /// nimmt einem Abschnitt seinen Ersatztext, aber es lässt nie einen
    /// Klartext stehen.
    Alle,
    /// Nur über diese `(Strom, Name)`-Paare. Alle anderen Platzierungen
    /// desselben Objekts zeigen weiter das unversehrte Original, und ihr
    /// Ersatztext ist wahr.
    Nur(BTreeSet<(StreamKey, Vec<u8>)>),
}

/// Die Namen einer Arbeit, wie eine Meldung sie nennt: „/Im0" — oder
/// „/Im0, /ImA", wenn dasselbe Bild unter mehreren Namen getroffen ist.
fn label_names(targets: &BTreeSet<(StreamKey, Vec<u8>)>) -> String {
    let mut namen: Vec<String> = targets
        .iter()
        .map(|(_, n)| format!("/{}", String::from_utf8_lossy(n)))
        .collect();
    namen.sort();
    namen.dedup();
    namen.join(", ")
}

/// Entscheidet, wohin die geschwärzten Bildpunkte kommen. Ohne Nebenwirkung:
/// die Entscheidung fällt zweimal (vermerken, schreiben) und muss beide Male
/// dieselbe sein.
fn fate(
    id: ObjectId,
    work: &Work,
    image_pages: &BTreeMap<ObjectId, BTreeSet<usize>>,
    form_pages: &BTreeMap<ObjectId, BTreeSet<usize>>,
) -> Fate {
    if image_pages.get(&id).map(BTreeSet::len).unwrap_or(1) <= 1 {
        return Fate::Overwrite;
    }
    // Kopieren geht nur, wenn sich der Verweis isolieren lässt: in den
    // Seitenressourcen immer, in einem Form-XObject nur, wenn dieses Formular
    // allein von dieser Seite benutzt wird.
    let isolable = work.targets.iter().all(|(stream, _)| match stream {
        StreamKey::Page => true,
        StreamKey::Form(form) => form_pages.get(form).map(BTreeSet::len) == Some(1),
    });
    if isolable {
        Fate::Copy
    } else {
        Fate::OverwriteShared
    }
}

/// Vermerkt jede Platzierung, die die geschwärzten Bildpunkte **zeigt** — nicht
/// nur die, unter der die Schwärzung lag, und nicht mehr als das.
///
/// Geschwärzt werden die Bildpunkte *im Objekt*. Wird dasselbe Objekt auf
/// derselben Seite ein zweites Mal gezeichnet, kommt es darauf an, wie es
/// geschrieben wurde ([`fate`]):
///
/// * **überschrieben** — dann zeigen alle Platzierungen dieses Objekts die
///   geschwärzten Bildpunkte, auch die, über der keine Zone lag. Der Spiegel
///   dort beschreibt ein Bild, das verloren hat, und trägt möglicherweise
///   genau das, was dort zu sehen war: er muss fallen. `only_name` ist `None`.
/// * **kopiert** — dann sind genau die umgebogenen `(Strom, Name)`-Paare
///   erreichbar. Ein Name desselben Objekts, der nicht getroffen war, zeigt
///   weiter das unversehrte Original; dessen Spiegel beschreibt sichtbare,
///   ungeschwärzte Bildpunkte und muss stehen bleiben. [`Shown::Nur`] trägt die
///   Paare, wie [`write_work`] sie wirklich umgebogen hat.
///
/// Fiel der Spiegel auch im zweiten Fall, widersprach sich die Ausgabedatei:
/// Spiegel weg **und** ungeschwärzte Bildpunkte auf derselben Seite sichtbar —
/// Fehlalarm und Leck in einem (`zl_b_pendel_beide_richtungen`).
///
/// Eine Platzierung auf einer **anderen** Seite ist nie gemeint: dort steht bei
/// einem geteilten Bild weiter das unversehrte Original ([`write_work`], Zweig
/// [`Fate::Copy`]). `Key::XObject` trägt den Seitenindex, `placements` ist die
/// Liste dieser Seite — beides hält das auseinander.
///
/// Ein Inline-Bild steht an genau einer Operation seines Stroms und hat keine
/// Geschwister; ein Name kommt dort nicht vor.
fn note_lost_pixels(
    outcome: &mut ImageOutcome,
    page_id: ObjectId,
    placements: &[Placement],
    key: Key,
    shown: &Shown,
) {
    match key {
        Key::Inline(InlineTarget::Page(id), op_index) => {
            outcome
                .page_image_hits
                .entry(id)
                .or_default()
                .insert(op_index);
        }
        Key::Inline(InlineTarget::Form(id), op_index) => {
            outcome
                .form_image_hits
                .entry(id)
                .or_default()
                .insert(op_index);
        }
        Key::XObject(_, id) => {
            for placement in placements {
                let Target::XObject {
                    id: Some(own),
                    name,
                    ..
                } = &placement.target
                else {
                    continue;
                };
                if *own != id {
                    continue;
                }
                // Strom und Name entscheiden nur, wenn kopiert wurde. Wurde
                // überschrieben — oder trug der Rückfall weiter als das eine
                // Paar —, zeigt jede Platzierung die geschwärzten Bildpunkte.
                if let Shown::Nur(erreichbar) = shown {
                    if !erreichbar.contains(&(placement.stream, name.clone())) {
                        continue;
                    }
                }
                note_placement(outcome, page_id, placement);
            }
        }
    }
}

/// Vermerkt ein Zielobjekt, dessen Bildpunkte sich **nicht anfassen** ließen.
///
/// Hier ist grob entschieden — und zwar in beide Richtungen ehrlich: die
/// getroffenen Platzierungen gelten als getroffen (der Ersatztext darüber
/// fällt, obwohl kein Bildpunkt fiel), und das Bilddictionary bleibt stehen
/// (sein `/Alt` muss eigens geräumt werden). Erlaubt ist das nur, weil daneben
/// eine Warnung steht; der Aufrufer, der sie erzeugt, ruft diese Funktion.
///
/// **Nur die getroffenen**, und nicht über [`note_lost_pixels`]. Dessen Frage
/// lautet „welche Platzierungen zeigen die geschwärzten Bildpunkte?" — und
/// geschwärzt ist hier keiner: das Objekt wird weder überschrieben noch
/// kopiert. Eine unberührte Platzierung desselben Bildes zeigt danach
/// buchstäblich dasselbe wie vorher; ihr Spiegel ist wahr, und sein Verlust
/// hätte keinen Gegenwert. Über der getroffenen Platzierung dagegen sollte
/// etwas verschwinden und verschwindet nicht — der Text darüber beschreibt
/// möglicherweise genau das.
fn note_undecided(
    outcome: &mut ImageOutcome,
    page_id: ObjectId,
    placements: &[Placement],
    entries: &[(usize, Vec<Zone>)],
    key: Key,
) {
    for (index, _) in entries {
        note_placement(outcome, page_id, &placements[*index]);
    }
    if let Key::XObject(_, id) = key {
        outcome.undecodable_images.insert(id);
    }
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
        // Am VIERECK, nicht an der Hülle. Hinter diesem Filter stehen die
        // note_*-Aufrufe, die entscheiden, ob der Ersatztext über einer
        // Platzierung fällt — auch in den Zweigen, die grob entscheiden dürfen
        // (unlesbares Bild, nicht übertragbare Maske, kein eigenständiges
        // Objekt). Entschied der Filter an der Hülle, verlor ein gedrehtes
        // unlesbares Bild seinen Ersatztext in der leeren Ecke: Fehlalarm der
        // ersten Runde, eine Ebene weiter hinten. Siehe [`placement_quad`].
        let quad = placement_quad(&placement.ctm);
        let touching: Vec<Zone> = zones
            .iter()
            .filter(|z| cell_meets_rect(&quad, &z.rect))
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
                    // Grob entschieden, aber gesagt: der Ersatztext über dieser
                    // Platzierung fällt, obwohl kein Bildpunkt fiel. Ein
                    // Dictionary zum Räumen gibt es hier nicht — das Bild ist
                    // gar kein eigenständiges Objekt.
                    note_placement(outcome, page_id, placement);
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
                // Eine Maske, die das Neukodieren nicht überstünde, ist ein
                // Abbruchgrund und **kein** Fall für eine Näherung: was die
                // Eingabe versteckt, stünde sonst in der Ausgabe sichtbar da.
                let carry = match mask_to_carry(doc, first) {
                    Ok(carry) => carry,
                    Err(reason) => {
                        mask_failure(options, outcome, &first.label(page_index), &reason)?;
                        note_undecided(outcome, page_id, placements, &entries, key);
                        continue;
                    }
                };
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
                // Der Farbschlüssel auf verlustbehaftet kodierten Daten wird
                // **hier** entschieden und nicht schon am Dictionary: die Frage
                // „trifft er überhaupt einen Bildpunkt?“ ist nur am dekodierten
                // Bild zu beantworten, und dekodiert wird nur an dieser Stelle
                // — hinter `budget.reserve`, also innerhalb von
                // `--max-image-mb`.
                //
                // Vor der Prüfung „ließ sich das Bild überhaupt dekodieren?“,
                // und zwar mit Absicht: ging das Dekodieren schief, ist die
                // Maske der schwerer wiegende der beiden Befunde. „Was der
                // Schlüssel versteckt, weiß ich nicht“ nennt den Grund, aus dem
                // die Datei nicht geschrieben werden darf; „Filter unbekannt“
                // nennt nur den Anlass.
                let mask = match carry {
                    Carry::Nothing => None,
                    Carry::Keep(object) => Some(object),
                    Carry::Probe(filter) => match probe_color_key(doc, first, &raster, &filter) {
                        Ok(note) => {
                            outcome
                                .warnings
                                .push(format!("{} {note}", first.label(page_index)));
                            None
                        }
                        Err(reason) => {
                            budget.cancel(reserved);
                            mask_failure(options, outcome, &first.label(page_index), &reason)?;
                            note_undecided(outcome, page_id, placements, &entries, key);
                            continue;
                        }
                    },
                };
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
                        note_undecided(outcome, page_id, placements, &entries, key);
                        continue;
                    }
                    // Der Fluchtweg wird **genannt** — aus demselben Grund wie
                    // in [`mask_failure`]: ohne den Hinweis endet der Lauf mit
                    // Rückgabewert 1 und ohne Ausgabedatei, und im Stapel fällt
                    // die Datei ersatzlos aus. Ein `/JPXDecode`-Scan ist
                    // gewöhnliches Material; wer ihn vorlegt, soll erfahren,
                    // wie es weitergeht. Angehängt und nicht eingebaut: der
                    // Wortlaut der **Warnung** steht als Schablone in
                    // `redact_pipeline::coverage` und muss dort wiederzufinden
                    // sein.
                    return Err(RedactError::Pdf(format!(
                        "{message} Mit --allow-undecodable-images läuft der Lauf trotzdem \
                         durch: das Bild bleibt dann ungeschwärzt, und die Ausgabedatei \
                         entsteht."
                    )));
                }
                let work = Work {
                    width: raster.width,
                    height: raster.height,
                    rgba: raster.rgba,
                    is_mask: first.is_mask(),
                    page_id,
                    targets: BTreeSet::new(),
                    filled: 0,
                    mask,
                };
                budget.settle(reserved, &work);
                work
            }
        };

        // Der Zähler **vor** dieser Seite: ein Inline-Bild in einem Formular
        // kann von einer früheren Seite her schon gefüllt sein, und dort wurde
        // es damals vermerkt. Gefragt ist, ob auf *dieser* Seite etwas fiel.
        let filled_before = work.filled;
        for (index, touching) in &entries {
            let placement = &placements[*index];
            work.targets.insert((placement.stream, placement.name()));
            work.fill(&placement.ctm, touching);
        }
        // **Die Wahrheit, hier und nur hier:** sind Bildpunkte gefallen? Nicht
        // „liegt das Rechteck auf der Fläche" — das ist die Schätzung, die
        // zweimal falsch war (siehe [`ImageOutcome::page_image_hits`]).
        let gefallen = work.filled > filled_before;

        // Ein Inline-Bild in einem Form-XObject wird von der nächsten Seite
        // vielleicht noch gebraucht und erst in Phase 3 geschrieben. Der
        // Vermerk gehört trotzdem in **diese** Seite — und er kann hier fallen,
        // weil ein Inline-Bild keinen Namen hat, über den das Schreiben noch
        // etwas zu entscheiden hätte.
        if matches!(key, Key::Inline(InlineTarget::Form(_), _)) {
            if gefallen {
                note_lost_pixels(outcome, page_id, placements, key, &Shown::Alle);
            }
            works.insert(key, work);
            continue;
        }

        budget.release(&work);
        // **Erst schreiben, dann vermerken.** Wohin die geschwärzten Bildpunkte
        // kommen, weiß nur der, der sie geschrieben hat: welche
        // `(Strom, Name)`-Paare umgebogen wurden, und ob der Rückfall auf die
        // Seitenressourcen weiter trug als das einzelne Paar. Wer vorher
        // vermerkt, rät — daran hing das Pendel dreier Runden.
        let shown = write_work(doc, key, work, image_pages, form_pages, outcome)?;
        if gefallen {
            if let Some(shown) = shown {
                note_lost_pixels(outcome, page_id, placements, key, &shown);
            }
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

/// Was von der `/Mask` dieses Bildes in die Ausgabe muss.
#[derive(Debug, Clone, PartialEq)]
enum Carry {
    /// Nichts mitzuschreiben: kein `/Mask`, oder die Maske steckt im
    /// Alphakanal und wird von dort neu aufgebaut.
    Nothing,
    /// Diesen `/Mask`-Verweis unverändert mitschreiben (Stencil-Strom).
    Keep(Object),
    /// Erst am **dekodierten** Bild zu entscheiden: Farbschlüssel auf
    /// verlustbehaftet kodierten Daten. Der Text ist der Filtername.
    Probe(String),
}

/// Was mit einer Maske geschieht, die sich nicht ohne stille Näherung
/// übertragen ließe.
///
/// `Ok(())` heißt „gewarnt, weiter mit dem nächsten Bild“ — das gibt es nur
/// mit `--allow-undecodable-images`. Sonst bricht der Lauf ab: was die Eingabe
/// versteckt, stünde in der Ausgabe sonst sichtbar da, und eine Datei, die
/// aussieht wie geschwärzt, ist schlimmer als keine.
///
/// Der Fluchtweg wird **genannt**. Er wirkt an genau dieser Stelle; ohne den
/// Hinweis endet der Lauf mit Rückgabewert 1 und ohne Ausgabedatei, und im
/// Stapel fällt die Datei ersatzlos aus.
fn mask_failure(
    options: &ImageOptions,
    outcome: &mut ImageOutcome,
    label: &str,
    reason: &str,
) -> Result<()> {
    let what = format!("{label} {reason}");
    if options.allow_undecodable {
        outcome.warnings.push(format!(
            "{what}. Das Bild bleibt deshalb ungeschwärzt; die Bildpunkte im \
             Schwärzungsbereich blieben in der Datei."
        ));
        return Ok(());
    }
    Err(RedactError::Pdf(format!(
        "{what}. Die Datei wird nicht als geschwärzt ausgegeben, statt die Maske \
         stillschweigend fallen zu lassen — die versteckten Bildpunkte stünden sonst sichtbar in \
         der Ausgabe. Mit --allow-undecodable-images läuft der Lauf trotzdem durch: das Bild \
         bleibt dann ungeschwärzt, und die Ausgabedatei entsteht."
    )))
}

/// Sieht am dekodierten Bild nach, ob der Farbschlüssel überhaupt etwas
/// verbirgt (siehe [`crate::ops::MaskPlan::ProbeColorKey`]).
///
/// `Ok(text)` heißt: er trifft keinen einzigen Bildpunkt, die Maske darf mit
/// diesem Hinweis entfallen. `Err(grund)` heißt: sie verbirgt etwas (oder es
/// ist nicht zu entscheiden) — dann ist Abbrechen die ehrliche Antwort.
fn probe_color_key(
    doc: &Document,
    placement: &Placement,
    raster: &crate::ops::RasterImage,
    filter: &str,
) -> std::result::Result<String, String> {
    let resources = placement.resources.as_deref();
    let Target::XObject { name, .. } = &placement.target else {
        return Err("trägt einen Farbschlüssel als Inline-Bild".into());
    };
    let Some(stream) = image_stream(doc, resources, name) else {
        return Err("ist nicht mehr auflösbar".into());
    };
    match crate::ops::color_key_reach(doc, resources, &stream.dict, raster) {
        crate::ops::ColorKeyReach::NothingHidden => Ok(format!(
            "trägt eine Farbschlüssel-Maske und ist mit {filter} kodiert. Im dekodierten Bild \
             fällt kein einziger Abtastwert in den Schlüsselbereich — auch nicht mit großzügigem \
             Band für den verlustbehafteten Decoder. Die Maske verbirgt nichts und ist deshalb \
             entfallen; alle Bildpunkte bleiben sichtbar wie in der Eingabe."
        )),
        crate::ops::ColorKeyReach::Hides(pixels) => Err(format!(
            "hat eine Farbschlüssel-Maske und ist mit {filter} kodiert. {pixels} Bildpunkt(e) \
             fallen in den Schlüsselbereich, die Maske verbirgt dort also etwas. Der Schlüssel \
             vergleicht Abtastwerte; welche ein verlustbehafteter Decoder liefert, ist von Decoder \
             zu Decoder verschieden — welche Bildpunkte die Datei genau versteckt, lässt sich \
             damit nicht sicher genug bestimmen"
        )),
        crate::ops::ColorKeyReach::Undecidable(why) => Err(format!(
            "hat eine Farbschlüssel-Maske und ist mit {filter} kodiert; ob sie Bildpunkte \
             verbirgt, ist am dekodierten Bild nicht nachzuprüfen ({why})"
        )),
    }
}

/// Was von der `/Mask` dieses Bildes in die Ausgabe muss — siehe [`Carry`].
///
/// `Err(grund)` ist eine Maske, die sich nicht ohne stille Näherung übertragen
/// ließe. Siehe [`crate::ops::MaskPlan`].
fn mask_to_carry(doc: &Document, placement: &Placement) -> std::result::Result<Carry, String> {
    let resources = placement.resources.as_deref();
    let dict = match &placement.target {
        Target::Inline { dict, .. } => {
            // Ein Inline-Bild darf nach PDF 32000-1 (8.9.7, Tabelle 93) weder
            // `/Mask` noch `/SMask` haben, und sein neu geschriebenes
            // `BI`-Dictionary könnte auch keines aufnehmen: ein Verweis auf ein
            // Objekt ist dort nicht erlaubt. Stünde trotzdem eines da, ginge es
            // beim Neuschreiben verloren.
            for key in [&b"Mask"[..], b"SMask"] {
                if dict.get(key).is_ok() {
                    return Err(format!(
                        "trägt ein /{}, das ein Inline-Bild gar nicht haben darf. Beim \
                         Neuschreiben ginge es verloren und die verdeckten Bildpunkte würden \
                         sichtbar",
                        String::from_utf8_lossy(key)
                    ));
                }
            }
            return Ok(Carry::Nothing);
        }
        Target::XObject { name, .. } => match image_stream(doc, resources, name) {
            Some(stream) => stream.dict.clone(),
            // Nicht auflösbar: das meldet `decode_placement` gleich darauf.
            None => return Ok(Carry::Nothing),
        },
    };
    match crate::ops::mask_plan(doc, resources, &dict) {
        crate::ops::MaskPlan::None | crate::ops::MaskPlan::InAlpha => Ok(Carry::Nothing),
        crate::ops::MaskPlan::Keep(object) => Ok(Carry::Keep(object)),
        crate::ops::MaskPlan::ProbeColorKey(filter) => Ok(Carry::Probe(filter)),
        crate::ops::MaskPlan::Unsupported(reason) => Err(reason),
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
    if crate::ops::xobject_subtype(doc, &stream.dict) != Some(b"Image") {
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

    /// Berührt die **Fläche** des Pixels `(x, y)` das Rechteck?
    ///
    /// Geprüft wird die Pixelzelle als Viereck im User-Space gegen das
    /// Rechteck, über die trennenden Achsen (siehe [`cell_meets_rect`]). Damit
    /// stimmt die Zuordnung auch bei gedrehter oder gescherter CTM, und im
    /// Zweifel wird ein Pixel zu viel geschwärzt statt eines zu wenig.
    ///
    /// **Früher wurden nur die vier Ecken der Zelle geprüft** (`rect.contains`
    /// je Ecke). Das ließ genau den Fall fallen, in dem das Rechteck ganz
    /// *innerhalb* einer Zelle liegt: ein grobes Bild groß gezogen — ein
    /// 2 × 2-Bild auf 100 × 100 Punkte hat 50 Punkte große Zellen — und ein
    /// kleineres Schwärzungsrechteck mitten darin. Keine Zellecke lag im
    /// Rechteck, kein Bildpunkt fiel, und es gab keine Warnung: die Schwärzung
    /// lag dort nur obenauf, und die Bildpunkte darunter blieben in der Datei.
    /// Die Zusicherung „im Zweifel ein Pixel zu viel“ galt also gerade nicht.
    fn covers(&self, ctm: &Matrix, x: usize, y: usize, rect: &Rect) -> bool {
        let w = self.width as f64;
        let h = self.height as f64;
        let u0 = x as f64 / w;
        let u1 = (x + 1) as f64 / w;
        let v0 = 1.0 - (y + 1) as f64 / h;
        let v1 = 1.0 - y as f64 / h;
        // Im Umlauf, damit die Kanten der Zelle Kanten bleiben.
        let cell = [
            ctm.apply(u0, v0),
            ctm.apply(u1, v0),
            ctm.apply(u1, v1),
            ctm.apply(u0, v1),
        ];
        cell_meets_rect(&cell, rect)
    }
}

/// Überlappen sich die Fläche einer Pixelzelle und ein Schwärzungsrechteck?
///
/// Trennende Achsen: die beiden Achsen des Rechtecks und die vier
/// Kantennormalen der Zelle. **Berührung zählt als Treffer** — anders als bei
/// [`crate::content::ImagePlacement::covers`], das dieselbe Form gegen dieselbe
/// Art Rechteck mit strengen Vergleichen prüft. Der Unterschied ist gewollt und
/// er ist die sichere Richtung: hier wird geschwärzt, dort nur gefragt. Ein
/// `NaN` in einer Projektion lässt beide Vergleiche falsch werden und gilt
/// deshalb ebenfalls als Treffer.
fn cell_meets_rect(cell: &[Point; 4], rect: &Rect) -> bool {
    let corners = [
        rect.ll,
        Point::new(rect.ur.x, rect.ll.y),
        rect.ur,
        Point::new(rect.ll.x, rect.ur.y),
    ];
    let normal = |from: Point, to: Point| Point::new(from.y - to.y, to.x - from.x);
    let axes = [
        Point::new(1.0, 0.0),
        Point::new(0.0, 1.0),
        normal(cell[0], cell[1]),
        normal(cell[1], cell[2]),
        normal(cell[2], cell[3]),
        normal(cell[3], cell[0]),
    ];
    for axis in axes {
        // Eine entartete Kante trennt nichts; sie darf die Antwort nicht
        // bestimmen.
        if !axis.x.is_finite() || !axis.y.is_finite() || (axis.x == 0.0 && axis.y == 0.0) {
            continue;
        }
        let (cmin, cmax) = crate::content::span(cell, axis);
        let (rmin, rmax) = crate::content::span(&corners, axis);
        if cmax < rmin || rmax < cmin {
            return false;
        }
    }
    true
}

/// Das **Viereck** einer Platzierung: das Einheitsquadrat durch die CTM.
///
/// Der Unterschied zur Hülle ([`ctm_bounds`]) ist der ganze Befund dieser
/// Runde. Bei einem um 45 Grad gedrehten Bild ist die Hülle doppelt so groß wie
/// das Bild, und in ihren vier Ecken liegt gar kein Bildpunkt. Wer dort
/// entscheidet, nimmt einem unversehrten Bild seinen Ersatztext — das war der
/// Fehlalarm der ersten Runde, und über den Umweg des Kandidatenfilters kam er
/// in der dritten zurück.
///
/// Die Frage kostet nichts: sie ist reine Geometrie und braucht kein Byte des
/// Bildes. Berührt das Viereck die Zone nicht, liegt **kein** Bildpunkt dieses
/// Bildes unter der Schwärzung — lesbar oder nicht. Es gibt hier also keine
/// Unwissenheit, über die man ehrlich sein müsste.
///
/// Seit der Gegenprüfung 9 ist die Hülle im Bildlauf **ganz** abgelöst:
/// `ctm_bounds` gab es an drei Stellen, und sie antworteten am Rand
/// verschieden. Der Seitenvorfilter übersprang eine Seite, deren Platzierung
/// eine Zone nur berührte; [`Collector::touches_a_zone`] warf die Rohdaten
/// eines so berührten Inline-Bildes weg, während [`fill_page`] die Platzierung
/// aufnahm — und der Lauf endete ohne Ausgabedatei. Jetzt fragen alle drei
/// dasselbe Viereck mit [`cell_meets_rect`].
fn placement_quad(ctm: &Matrix) -> [Point; 4] {
    [
        ctm.apply(0.0, 0.0),
        ctm.apply(1.0, 0.0),
        ctm.apply(1.0, 1.0),
        ctm.apply(0.0, 1.0),
    ]
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

    // Ein `/Mask` der Eingabe wird unverändert mitgeschrieben — solange
    // **kein** Bildpunkt gefallen ist. Es steht neben dem Bild, beschreibt es
    // im Einheitsquadrat und übersteht das Neukodieren deshalb in voller
    // Auflösung und mit seinen harten Kanten. Ein `/SMask` gibt es dann
    // nicht: beides nebeneinander ist regelwidrig (PDF 32000-1, Tabelle 89),
    // und die Alphaebene, die hier vorläge, wäre nichts anderes als dieselbe
    // Maske — auf die Auflösung des Bildes heruntergebrochen.
    //
    // Sind Bildpunkte gefallen, ist die Maske **selbst** Bildinhalt: ihre
    // Bits sind die Form, die das Bild malt — ein Textumriss als Stencil
    // trägt den Text, auch wenn darunter jede Farbe schwarz ist. Die
    // Spur-A-Runde 1 fand den Suchbegriff im unverändert mitgeschriebenen
    // Maskenstrom der Ausgabe (Register #77). Deshalb geht dann die
    // Alphaebene hinaus, die [`Work::fill`] unter der Zone auf undurchsichtig
    // gesetzt hat: außerhalb der Zone dieselbe Maske, unter der Zone die
    // Schwärzung — als `/SMask` in Bildauflösung. Der Preis (die Maske
    // verliert ihre eigene Auflösung) fällt nur bei einem Bild an, das
    // wirklich geschwärzt wurde.
    //
    // Dass die Alphaebene wirklich *dieselbe* Maske trägt, ist keine Annahme,
    // sondern folgt aus `crate::ops::Honoured`: `work.mask` ist genau dann
    // gesetzt, wenn der Stencil-`/Mask`-Strom die befolgte Maske ist — und
    // dann hat `apply_soft_mask` genau ihn in den Alphakanal gerechnet. Steht
    // daneben ein `/SMask`, gilt das `/SMask`, `mask_plan` liefert `InAlpha`,
    // `work.mask` bleibt leer, und der Alphakanal geht unten als neues
    // `/SMask` in die Ausgabe. Diese Verzweigung darf deshalb nie eine
    // Alphaebene wegwerfen, die etwas anderes sagt als das `/Mask`.
    if let Some(mask) = &work.mask {
        if work.filled == 0 {
            dict.set("Mask", mask.clone());
            return Encoded {
                dict,
                data,
                smask: None,
            };
        }
    }

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

    /// Das Viereck einer Platzierung — und der Unterschied zur Hülle, die es
    /// abgelöst hat.
    ///
    /// Hier stand `unit_square_bounds` und prüfte `ctm_bounds`. Die Funktion
    /// gibt es nicht mehr: sie hatte drei Aufrufstellen, die am Rand
    /// verschieden antworteten, und alle drei fragen jetzt dieses Viereck.
    #[test]
    fn placement_quad_is_the_unit_square_through_the_ctm() {
        let quad = placement_quad(&Matrix::new(100.0, 0.0, 0.0, 50.0, 10.0, 20.0));
        assert_eq!(
            quad.map(|p| (p.x, p.y)),
            [(10.0, 20.0), (110.0, 20.0), (110.0, 70.0), (10.0, 70.0)]
        );

        // Um 45 Grad gedreht: die **Hülle** dieses Vierecks wäre das Quadrat
        // (-70,-70)-(70,70) und damit doppelt so groß wie das Bild. In ihrer
        // linken Ecke liegt kein Bildpunkt — und genau dort hat die erste Runde
        // einem unversehrten Bild seinen Ersatztext genommen.
        let s = std::f64::consts::FRAC_1_SQRT_2 * 100.0;
        let schraeg = placement_quad(&Matrix::new(s, s, -s, s, 0.0, 0.0));
        let ecke = Rect::new(-69.0, -1.0, -68.0, 1.0);
        assert!(
            !cell_meets_rect(&schraeg, &ecke),
            "die leere Hüllenecke berührt das Viereck nicht"
        );
        // Die Mitte dagegen trifft.
        assert!(cell_meets_rect(&schraeg, &Rect::new(-1.0, 60.0, 1.0, 62.0)));
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
            page_id: (1, 0),
            targets: BTreeSet::new(),
            filled: 0,
            mask: None,
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
            page_id: (1, 0),
            targets: BTreeSet::new(),
            filled: 0,
            mask: None,
        };
        // Bit 0 = 0 (malt), Bit 1 = 1 (malt nicht), Rest Füllbits.
        assert_eq!(mask_bits(&work), vec![0b0111_1111]);
    }
}
