//! Gegenprüfung A, Runde 9 — Masken, Inline-Bilder über Seitengrenzen und
//! indirekte Bildmaße.
//!
//! Alle Aussagen werden an den Bildpunkten der **Ausgabedatei** gemessen
//! (`page_ops` über die geladene Ausgabe), nicht am Bericht. Für Stencil-Masken
//! zählt der Alphakanal: ein Bildpunkt, der nicht mehr malt, hat Alpha 0.

use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{load_from_bytes, page_ops, save_to_bytes, DrawOp, PdfRedactor, RedactionReport};

fn seiten(mut doc: Document, seiten: Vec<(ObjectId, Vec<u8>)>) -> (Document, Vec<ObjectId>) {
    let pages_id = doc.new_object_id();
    let mut ids = Vec::new();
    for (resources_id, content) in seiten {
        let content_id = doc.add_object(Stream::new(dictionary! {}, content));
        ids.push(doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 842.into(), 842.into()],
        }));
    }
    let count = ids.len() as i64;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => ids.iter().map(|id| Object::Reference(*id)).collect::<Vec<_>>(),
            "Count" => count,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    (doc, ids)
}

fn schwaerzung(page: usize, rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            page,
            rect,
            None,
            Source::Manual {
                reason: "Gegenprüfung A, Runde 9".into(),
            },
        ),
        Action::Blackout,
    )
}

fn schwaerze(
    doc: &mut Document,
    list: &[Redaction],
    zugestaendnis: bool,
) -> (RedactionReport, Vec<u8>) {
    let report = PdfRedactor::with_padding(0.0)
        .allowing_undecodable_images(zugestaendnis)
        .apply_with_report(doc, list)
        .expect("Schwärzung läuft");
    let bytes = save_to_bytes(doc).expect("Speichern");
    (report, bytes)
}

/// Alle Bildpunkte, die die Ausgabeseite zeigt: `(Spalte, Zeile, RGBA)`.
fn gezeigt(bytes: &[u8], seite: usize) -> Vec<(usize, usize, [u8; 4])> {
    let doc = load_from_bytes(bytes).expect("Ausgabe lädt");
    let ops = page_ops(&doc, seite).expect("page_ops läuft");
    let mut out = Vec::new();
    for op in &ops.ops {
        let DrawOp::Image { image, .. } = op else {
            continue;
        };
        let raster = &ops.images[*image];
        for y in 0..raster.height as usize {
            for x in 0..raster.width as usize {
                let off = (y * raster.width as usize + x) * 4;
                out.push((
                    x,
                    y,
                    [
                        raster.rgba[off],
                        raster.rgba[off + 1],
                        raster.rgba[off + 2],
                        raster.rgba[off + 3],
                    ],
                ));
            }
        }
    }
    out
}

/// 8 x 8 RGB, jeder Bildpunkt an seinem Wert erkennbar, keiner schwarz.
fn rgb_bild_bytes() -> Vec<u8> {
    let mut data = Vec::with_capacity(8 * 8 * 3);
    for y in 0..8u32 {
        for x in 0..8u32 {
            data.extend_from_slice(&[(x * 16 + 1) as u8, (y * 16 + 1) as u8, 0x40]);
        }
    }
    data
}

fn eingabewert(x: usize, y: usize) -> [u8; 3] {
    [(x * 16 + 1) as u8, (y * 16 + 1) as u8, 0x40]
}

fn gefallene(bytes: &[u8], seite: usize) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = gezeigt(bytes, seite)
        .into_iter()
        .filter(|(x, y, farbe)| [farbe[0], farbe[1], farbe[2]] != eingabewert(*x, *y))
        .map(|(x, y, _)| (x, y))
        .collect();
    out.sort();
    out
}

// ===========================================================================
// Indirekte Bildmaße
// ===========================================================================

/// **Versucht zu widerlegen, nicht geschafft.** `/Width` und `/Height` als
/// **indirekte** Verweise — regelkonform (PDF 32000-1, 7.3.10) und der Grund,
/// aus dem `image.rs` nicht `lopdf::Document::get_page_images` benutzt.
///
/// Gefragt: fällt trotzdem genau die richtige Zelle, oder scheitert das
/// Dekodieren still (dann wäre `redacted_images` 0 und niemand hätte es gesagt)?
#[test]
fn indirekte_bildmasse_werden_geschwaerzt() {
    let mut doc = Document::with_version("1.5");
    let breite_id = doc.add_object(Object::Integer(8));
    let hoehe_id = doc.add_object(Object::Integer(8));
    let bild_id = doc.add_object(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => Object::Reference(breite_id),
                "Height" => Object::Reference(hoehe_id),
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8_i64,
            },
            rgb_bild_bytes(),
        )
        .with_compression(false),
    ));
    let res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let (mut doc, _) = seiten(doc, vec![(res, b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec())]);
    let (report, out) = schwaerze(
        &mut doc,
        &[schwaerzung(0, Rect::new(63.0, 613.0, 74.0, 637.0))],
        false,
    );
    assert_eq!(
        report.redacted_images, 1,
        "ein Bild mit indirektem /Width muss geschwärzt werden — {:?}",
        report.warnings
    );
    assert_eq!(
        gefallene(&out, 0),
        vec![(1, 5), (1, 6)],
        "und zwar genau die Zelle unter der Zone"
    );
}

// ===========================================================================
// Inline-Bild in einem Formular über zwei Seiten (Phase 3)
// ===========================================================================

/// **Versucht zu widerlegen, nicht geschafft.** Ein Inline-Bild steht in einem
/// Form-XObject, das **zwei** Seiten zeichnen. Es wird einmal geschrieben und
/// muss die Bereiche **beider** Seiten tragen (`redact_images`, Phase 3): Seite 1
/// schwärzt Spalte 1, Seite 2 Spalte 3.
///
/// Fiele nur der Bereich einer Seite, stünde auf der anderen ein
/// ungeschwärztes Bild unter einem schwarzen Rechteck — genau der Ausgang, den
/// der Modulkopf ausschließt.
#[test]
fn inline_bild_im_formular_traegt_die_bereiche_beider_seiten() {
    let mut doc = Document::with_version("1.5");
    let mut form_inhalt = b"q 100 0 0 100 0 0 cm\nBI /W 8 /H 8 /CS /RGB /BPC 8 ID ".to_vec();
    form_inhalt.extend_from_slice(&rgb_bild_bytes());
    form_inhalt.extend_from_slice(b"\nEI\nQ\n");
    let form_id = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Resources" => dictionary! {},
        },
        form_inhalt,
    ));
    let res1 = doc.add_object(dictionary! { "XObject" => dictionary! { "Fm0" => form_id } });
    let res2 = doc.add_object(dictionary! { "XObject" => dictionary! { "Fm0" => form_id } });
    let inhalt = b"q 1 0 0 1 50 600 cm /Fm0 Do Q\n".to_vec();
    let (mut doc, _) = seiten(doc, vec![(res1, inhalt.clone()), (res2, inhalt)]);
    let (report, out) = schwaerze(
        &mut doc,
        &[
            schwaerzung(0, Rect::new(63.0, 613.0, 74.0, 637.0)),
            schwaerzung(1, Rect::new(88.0, 613.0, 99.0, 637.0)),
        ],
        false,
    );
    assert_eq!(
        report.redacted_images, 1,
        "ein Inline-Bild, einmal geschrieben — {:?}",
        report.warnings
    );
    let erwartet = vec![(1, 5), (1, 6), (3, 5), (3, 6)];
    assert_eq!(
        gefallene(&out, 0),
        erwartet,
        "Seite 1 trägt beide Bereiche (das Formular ist geteilt)"
    );
    assert_eq!(
        gefallene(&out, 1),
        erwartet,
        "Seite 2 ebenso — sonst stünde dort ein ungeschwärztes Bild unter einem \
         schwarzen Rechteck"
    );
}

// ===========================================================================
// Stencil-Masken (/ImageMask)
// ===========================================================================

/// Ein 8 x 8 Stencil, dessen Bits alle `bit` sind.
fn stencil(bit: bool, decode: Option<Vec<Object>>) -> Stream {
    let byte = if bit { 0xFFu8 } else { 0x00 };
    let mut dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Image",
        "Width" => 8_i64,
        "Height" => 8_i64,
        "ImageMask" => true,
        "BitsPerComponent" => 1_i64,
    };
    if let Some(decode) = decode {
        dict.set("Decode", Object::Array(decode));
    }
    Stream::new(dict, vec![byte; 8]).with_compression(false)
}

/// **Versucht zu widerlegen, nicht geschafft.** Eine Stencil-Maske, die überall
/// malt (`/Decode` weggelassen, alle Bits 0). Unter der Zone darf sie nicht mehr
/// malen — dort hat der Bildpunkt Alpha 0 —, überall sonst weiter.
#[test]
fn stencil_malt_unter_der_zone_nicht_mehr() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(stencil(false, None)));
    let res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let (mut doc, _) = seiten(doc, vec![(res, b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec())]);
    let (report, out) = schwaerze(
        &mut doc,
        &[schwaerzung(0, Rect::new(63.0, 613.0, 74.0, 637.0))],
        false,
    );
    assert_eq!(report.redacted_images, 1, "{:?}", report.warnings);
    let stumm: Vec<(usize, usize)> = gezeigt(&out, 0)
        .into_iter()
        .filter(|(_, _, rgba)| rgba[3] < 128)
        .map(|(x, y, _)| (x, y))
        .collect();
    assert_eq!(
        stumm,
        vec![(1, 5), (1, 6)],
        "genau die Zelle unter der Zone malt nicht mehr"
    );
}

/// **Versucht zu widerlegen, nicht geschafft.** Dieselbe Maske **invertiert**:
/// `/Decode [1 0]`, alle Bits 1 — sie malt damit ebenfalls überall.
/// `encode_xobject` schreibt die Ausgabe immer mit `/Decode [0 1]`; hielte sich
/// das Dekodieren nicht an das `/Decode` der Eingabe, verschwände die ganze
/// Maske aus der Ausgabe (oder erschiene invertiert).
#[test]
fn invertierter_stencil_behaelt_seine_form() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(stencil(
        true,
        Some(vec![Object::Integer(1), Object::Integer(0)]),
    )));
    let res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let (mut doc, _) = seiten(doc, vec![(res, b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec())]);
    let (report, out) = schwaerze(
        &mut doc,
        &[schwaerzung(0, Rect::new(63.0, 613.0, 74.0, 637.0))],
        false,
    );
    assert_eq!(report.redacted_images, 1, "{:?}", report.warnings);
    let stumm: Vec<(usize, usize)> = gezeigt(&out, 0)
        .into_iter()
        .filter(|(_, _, rgba)| rgba[3] < 128)
        .map(|(x, y, _)| (x, y))
        .collect();
    assert_eq!(
        stumm,
        vec![(1, 5), (1, 6)],
        "genau die Zelle unter der Zone malt nicht mehr — die übrigen 62 malen \
         weiter, das /Decode [1 0] der Eingabe ist also befolgt"
    );
}

// ===========================================================================
// Eine Maske, die das Neukodieren nicht überstünde
// ===========================================================================

/// Ein Bild mit einer Farbschlüssel-Maske, deren Länge nicht zum Farbraum passt
/// (`/Mask [0 0 0]` bei DeviceRGB, erwartet wären sechs Einträge) — für
/// `ops::mask_plan` `Unsupported`.
fn bild_mit_unbrauchbarer_maske(alt: &str) -> Stream {
    Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 8_i64,
            "Height" => 8_i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8_i64,
            "Mask" => vec![0.into(), 0.into(), 0.into()],
            "Alt" => Object::string_literal(alt),
        },
        rgb_bild_bytes(),
    )
    .with_compression(false)
}

/// **Versucht zu widerlegen, nicht geschafft.** Eine nicht übertragbare Maske
/// bricht den Lauf ab — das ist die Zusage des Modulkopfs. Gefragt ist hier die
/// **Gegenrichtung**: bricht sie auch dann ab, wenn die Schwärzung das Bild
/// nachweislich **nicht** berührt? Das wäre ein Fehlalarm wie der der ersten
/// Runde, eine Ebene weiter hinten (der Kandidatenfilter steht **vor**
/// `mask_to_carry`).
#[test]
fn unbrauchbare_maske_neben_der_zone_bricht_nicht_ab() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_mit_unbrauchbarer_maske("Scan")));
    let res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let (mut doc, _) = seiten(doc, vec![(res, b"q 100 0 0 100 400 600 cm /Im0 Do Q\n".to_vec())]);
    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(&mut doc, &[schwaerzung(0, Rect::new(40.0, 590.0, 160.0, 710.0))])
        .expect("der Lauf darf nicht an einem Bild scheitern, das die Schwärzung nicht berührt");
    assert_eq!(report.redacted_images, 0, "{:?}", report.warnings);
}

/// **Versucht zu widerlegen, nicht geschafft.** Dieselbe Maske, jetzt **unter**
/// der Zone und mit `--allow-undecodable-images`: hier ist grob entschieden, und
/// es muss gesagt werden. `note_undecided` vermerkt die getroffene Platzierung
/// und trägt das Bild in `undecodable_images` ein — daran räumt `redact.rs` das
/// `/Alt` am Bilddictionary.
#[test]
fn unbrauchbare_maske_unter_der_zone_warnt_und_raeumt_den_ersatztext() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_mit_unbrauchbarer_maske("Scan")));
    let res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let (mut doc, _) = seiten(doc, vec![(res, b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec())]);
    let (report, out) = schwaerze(
        &mut doc,
        &[schwaerzung(0, Rect::new(63.0, 613.0, 74.0, 637.0))],
        true,
    );
    assert!(
        report.warnings.iter().any(|w| w.contains("Farbschlüssel")),
        "die Maske wird genannt: {:?}",
        report.warnings
    );
    // Die Bildpunkte bleiben — das ist der Preis des Zugeständnisses, und er
    // steht in der Warnung.
    assert!(
        gefallene(&out, 0).is_empty(),
        "ungeschwärzt, wie die Warnung sagt"
    );
    let aus = load_from_bytes(&out).expect("Ausgabe lädt");
    let alt = match aus.get_object(bild_id).expect("Bild") {
        Object::Stream(s) => s.dict.get(b"Alt").ok().cloned(),
        _ => None,
    };
    assert_eq!(
        alt, None,
        "der Ersatztext am Bilddictionary ist geräumt — sonst stünde neben den \
         gebliebenen Bildpunkten auch noch ihr Klartext"
    );
}
