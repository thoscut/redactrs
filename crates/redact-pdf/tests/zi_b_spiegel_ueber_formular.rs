//! Gegenprüfung (I/B) zu Register #20: **wo der Bildspiegel weiterlebt.**
//!
//! Agent B schließt den Fall `/Figure <</Alt (…)>> BDC /Im0 Do EMC` — Spiegel
//! und Bildplatzierung im **selben** Strom. Diese Datei fragt die beiden
//! Lagen, die daneben liegen:
//!
//! 1. der Spiegel steht im **Seitenstrom**, das Bild eine Ebene tiefer im
//!    Form-XObject (`/Figure <</Alt (…)>> BDC /Fm0 Do EMC`, `/Im0 Do` in
//!    `Fm0`). Für Glyphen ist genau dieser Weg gedeckt
//!    ([`MarkedTextRecord::forms`], `touches_form_plan`, Befund G1-A1/G1-A2);
//!    für Bilder wird nur die **eigene** Spanne des Stroms gefragt.
//! 2. ein Bild, dessen Pixel stehenbleiben (`--allow-undecodable-images`), mit
//!    `/Alt` am Bilddictionary, auf einer Seite **ohne** Marked-Content — also
//!    in einem gewöhnlichen, nicht getaggten PDF. `ScanResult::images` wird nur
//!    in Strömen gefüllt, „über denen überhaupt ein Spiegel steht“; ohne
//!    `BDC` ist die Liste leer, und `blacked_images` damit auch.
//!
//! Gemessen wird ausschließlich mit dem Orakel [`redact_pdf::leaks`].

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{leaks, save_to_bytes, PdfRedactor, RedactionReport};

/// Was auf dem Bild zu lesen war — und in seiner Beschreibung steht.
const GEHEIM: &str = "Kontoauszug, IBAN DE89 3704 0044 0532 0130 00";

// ---------------------------------------------------------------------------
// Gerüst (dieselben Maße wie zh_b_bildspiegel, damit die Fälle vergleichbar sind)
// ---------------------------------------------------------------------------

fn bild(width: u32, height: u32) -> Stream {
    let mut data = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            data.extend_from_slice(&[(x * 11 + 3) as u8, (y * 7 + 5) as u8, 0x40]);
        }
    }
    let mut stream = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => i64::from(width),
            "Height" => i64::from(height),
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8_i64,
        },
        data,
    );
    let _ = stream.compress();
    stream
}

/// Ein Bild, dessen Pixel sich **nicht** dekodieren lassen.
fn unlesbares_bild() -> Stream {
    Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 20_i64,
            "Height" => 20_i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8_i64,
            "Filter" => "JPXDecode",
        },
        b"kein JPEG-2000".to_vec(),
    )
    .with_compression(false)
}

fn seite(mut doc: Document, resources_id: ObjectId, content: Vec<u8>) -> (Document, ObjectId) {
    let content_id = doc.add_object(Stream::new(dictionary! {}, content));
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
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    (doc, page_id)
}

fn schwaerzung(rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            0,
            rect,
            None,
            Source::Manual {
                reason: "Bildfläche".into(),
            },
        ),
        Action::Blackout,
    )
}

fn schwaerze_mit(
    doc: &mut Document,
    rects: &[Rect],
    allow_undecodable: bool,
) -> (RedactionReport, Vec<u8>) {
    let list: Vec<Redaction> = rects.iter().copied().map(schwaerzung).collect();
    let report = PdfRedactor::with_padding(0.0)
        .allowing_undecodable_images(allow_undecodable)
        .apply_with_report(doc, &list)
        .expect("Schwärzung läuft");
    let bytes = save_to_bytes(doc).expect("Speichern");
    (report, bytes)
}

fn schwaerze(doc: &mut Document, rects: &[Rect]) -> (RedactionReport, Vec<u8>) {
    schwaerze_mit(doc, rects, false)
}

/// Das Rechteck mitten im Bild bei (50,600)-(150,700).
fn ueber_dem_bild() -> Rect {
    Rect::new(75.0, 650.0, 100.0, 675.0)
}

fn alt_am_objekt(doc: &Document, id: ObjectId) -> Option<String> {
    let dict: &Dictionary = match doc.get_object(id).ok()? {
        Object::Stream(stream) => &stream.dict,
        Object::Dictionary(dict) => dict,
        _ => return None,
    };
    dict.get(b"Alt")
        .ok()
        .and_then(|value| value.as_str().ok())
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
}

/// Ein Form-XObject, das genau ein Bild zeichnet — bei (50,600)-(150,700).
fn formular_mit_bild(doc: &mut Document, innen: Option<ObjectId>) -> (ObjectId, ObjectId) {
    let im0 = doc.add_object(Object::Stream(bild(20, 20)));
    let inhalt = match innen {
        None => "q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_string(),
        Some(_) => "q /Fm1 Do Q\n".to_string(),
    };
    let mut resources = dictionary! { "XObject" => dictionary! { "Im0" => im0 } };
    if let Some(inner) = innen {
        resources.set("XObject", dictionary! { "Fm1" => inner });
    }
    let form = doc.add_object(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Resources" => resources,
            },
            inhalt.into_bytes(),
        )
        .with_compression(false),
    ));
    (form, im0)
}

// ---------------------------------------------------------------------------
// Lage 1: der Spiegel im Seitenstrom, das Bild im Formular
// ---------------------------------------------------------------------------

/// **Der Gegenbefund.** `/Figure <</Alt (Kontoauszug, IBAN …)>> BDC /Fm0 Do EMC`
/// im Seitenstrom, `/Im0 Do` in `Fm0`. Die Pixel fallen (`redacted_images = 1`),
/// der Klartext im Seitenstrom bleibt: `image_hits` kennt nur Platzierungen des
/// **eigenen** Stroms, und `touches_form_plan` fragt allein die Glyphenpläne
/// der Formulare, nicht ihre geschwärzten Bildflächen.
#[test]
fn spiegel_im_seitenstrom_ueber_formular_mit_bild() {
    let mut doc = Document::with_version("1.5");
    let (form, _im0) = formular_mit_bild(&mut doc, None);
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Fm0" => form },
    });
    let content = format!("/Figure <</Alt ({GEHEIM})>> BDC\nq /Fm0 Do Q\nEMC\n");
    let (mut doc, _) = seite(doc, resources_id, content.into_bytes());
    let (report, out) = schwaerze(&mut doc, &[ueber_dem_bild()]);
    assert_eq!(report.redacted_images, 1, "die Pixel fallen wirklich");
    assert!(
        leaks(&out, GEHEIM).is_empty(),
        "Spiegel im Seitenstrom über dem Formular: {:?} (gemeldet geleert: {})",
        leaks(&out, GEHEIM),
        report.image_alt_texts_cleared
    );
}

/// Dieselbe Lage eine Ebene tiefer: der Spiegel steht in `Fm0`, das Bild in
/// `Fm1`, das `Fm0` zeichnet. Für Glyphen ist das ausdrücklich gedeckt
/// (Befund G1-A1).
#[test]
fn spiegel_im_formular_ueber_innerem_formular_mit_bild() {
    let mut doc = Document::with_version("1.5");
    let (innen, _im0) = formular_mit_bild(&mut doc, None);
    let aussen_inhalt = format!("/Figure <</Alt ({GEHEIM})>> BDC\nq /Fm1 Do Q\nEMC\n");
    let aussen = doc.add_object(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Resources" => dictionary! { "XObject" => dictionary! { "Fm1" => innen } },
            },
            aussen_inhalt.into_bytes(),
        )
        .with_compression(false),
    ));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Fm0" => aussen },
    });
    let (mut doc, _) = seite(doc, resources_id, b"q /Fm0 Do Q\n".to_vec());
    let (report, out) = schwaerze(&mut doc, &[ueber_dem_bild()]);
    assert_eq!(report.redacted_images, 1, "die Pixel fallen wirklich");
    assert!(
        leaks(&out, GEHEIM).is_empty(),
        "Spiegel im äußeren Formular: {:?}",
        leaks(&out, GEHEIM)
    );
}

// ---------------------------------------------------------------------------
// Lage 2: das gewöhnliche, nicht getaggte PDF
// ---------------------------------------------------------------------------

/// **Der zweite Gegenbefund.** Ein Bild, dessen Pixel stehenbleiben, mit
/// `/Alt (Kontoauszug, IBAN …)` am Dictionary — auf einer Seite **ohne**
/// Marked-Content. `clear_image_alternates` soll laut Bericht „unabhängig
/// davon, ob die Pixel wirklich fielen“ greifen; sein Futter `blacked_images`
/// kommt aber aus `ScanResult::images`, und das wird nur in Strömen gefüllt,
/// über denen ein Textspiegel steht. Ohne `BDC` im Strom bleibt der Ersatztext.
#[test]
fn unlesbares_bild_ohne_marked_content_behaelt_seinen_ersatztext() {
    let mut doc = Document::with_version("1.5");
    let mut roh = unlesbares_bild();
    roh.dict.set("Alt", Object::string_literal(GEHEIM));
    let im0 = doc.add_object(Object::Stream(roh));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => im0 },
    });
    let (mut doc, _) = seite(
        doc,
        resources_id,
        b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec(),
    );
    let (report, out) = schwaerze_mit(&mut doc, &[ueber_dem_bild()], true);
    assert_eq!(report.redacted_images, 0, "die Pixel bleiben (unlesbar)");
    assert_eq!(
        alt_am_objekt(&doc, im0),
        None,
        "/Alt am Bilddictionary steht noch"
    );
    assert!(
        leaks(&out, GEHEIM).is_empty(),
        "Ersatztext am Bilddictionary: {:?}",
        leaks(&out, GEHEIM)
    );
}

/// Dieselbe Seite, nur mit einer beliebigen Marked-Content-Klammer irgendwo im
/// Strom — einer, die **nichts** mit dem Bild zu tun hat. Hält dieser Test und
/// der vorige nicht, dann hängt die Korrektur daran, dass die Datei getaggt
/// ist: dieselbe Datei, dasselbe Bild, dieselbe Schwärzung, anderes Ergebnis.
#[test]
fn dieselbe_seite_mit_irgendeiner_klammer() {
    let mut doc = Document::with_version("1.5");
    let mut roh = unlesbares_bild();
    roh.dict.set("Alt", Object::string_literal(GEHEIM));
    let im0 = doc.add_object(Object::Stream(roh));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => im0 },
    });
    let (mut doc, _) = seite(
        doc,
        resources_id,
        b"/Span <</ActualText (unbeteiligt)>> BDC EMC\n\
          q 100 0 0 100 50 600 cm /Im0 Do Q\n"
            .to_vec(),
    );
    let (report, out) = schwaerze_mit(&mut doc, &[ueber_dem_bild()], true);
    assert_eq!(report.redacted_images, 0, "die Pixel bleiben (unlesbar)");
    assert!(
        leaks(&out, GEHEIM).is_empty(),
        "Ersatztext am Bilddictionary: {:?}",
        leaks(&out, GEHEIM)
    );
}

// ---------------------------------------------------------------------------
// Gegenrichtung: nichts darf zu viel fallen
// ---------------------------------------------------------------------------

/// Das Formular wird gezeichnet, aber **keine** Schwärzung liegt auf seinem
/// Bild: der Spiegel im Seitenstrom bleibt. Ein Dokument ohne Ersatztexte ist
/// für blinde Leser unbrauchbar.
#[test]
fn formular_neben_der_schwaerzung_behaelt_den_spiegel() {
    let mut doc = Document::with_version("1.5");
    let (form, _im0) = formular_mit_bild(&mut doc, None);
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Fm0" => form },
    });
    let content = format!("/Figure <</Alt ({GEHEIM})>> BDC\nq /Fm0 Do Q\nEMC\n");
    let (mut doc, _) = seite(doc, resources_id, content.into_bytes());
    // Weit weg vom Bild (50,600)-(150,700).
    let (report, out) = schwaerze(&mut doc, &[Rect::new(400.0, 100.0, 450.0, 150.0)]);
    assert_eq!(report.redacted_images, 0, "kein Bild wird angefasst");
    assert!(
        !leaks(&out, GEHEIM).is_empty(),
        "der unbeteiligte Ersatztext muss bleiben"
    );
}

// ---------------------------------------------------------------------------
// Für den Lauf am gebauten Binär
// ---------------------------------------------------------------------------

/// Schreibt die beiden Gegenbeispiele als PDF-Datei, damit derselbe Fall am
/// ganzen Weg (CLI, `--check-leaks`) gefahren werden kann. Kein Urteil, nur
/// Material — deshalb `#[ignore]`.
#[test]
#[ignore = "schreibt Material in ZI_B_DIR; nur fuer den Lauf am Binaer"]
fn schreibe_gegenbeispiele() {
    let dir = std::env::var("ZI_B_DIR").expect("ZI_B_DIR setzen");
    let dir = std::path::Path::new(&dir);
    std::fs::create_dir_all(dir).expect("Verzeichnis");

    // 1) Spiegel im Seitenstrom, Bild im Formular.
    let mut doc = Document::with_version("1.5");
    let (form, _) = formular_mit_bild(&mut doc, None);
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Fm0" => form },
    });
    let content = format!("/Figure <</Alt ({GEHEIM})>> BDC\nq /Fm0 Do Q\nEMC\n");
    let (mut doc, _) = seite(doc, resources_id, content.into_bytes());
    doc.save(dir.join("spiegel_ueber_formular.pdf"))
        .expect("speichern");

    // 2) Unlesbares Bild mit /Alt, Seite ohne Marked-Content.
    let mut doc = Document::with_version("1.5");
    let mut roh = unlesbares_bild();
    roh.dict.set("Alt", Object::string_literal(GEHEIM));
    let im0 = doc.add_object(Object::Stream(roh));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => im0 },
    });
    let (mut doc, _) = seite(
        doc,
        resources_id,
        b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec(),
    );
    doc.save(dir.join("unlesbar_ohne_klammer.pdf"))
        .expect("speichern");

    // 2b) Dieselbe Seite, mit einer unbeteiligten Klammer.
    let mut doc = Document::with_version("1.5");
    let mut roh = unlesbares_bild();
    roh.dict.set("Alt", Object::string_literal(GEHEIM));
    let im0 = doc.add_object(Object::Stream(roh));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => im0 },
    });
    let (mut doc, _) = seite(
        doc,
        resources_id,
        b"/Span <</ActualText (unbeteiligt)>> BDC EMC\n\
          q 100 0 0 100 50 600 cm /Im0 Do Q\n"
            .to_vec(),
    );
    doc.save(dir.join("unlesbar_mit_klammer.pdf"))
        .expect("speichern");
}

// ---------------------------------------------------------------------------
// Lage 3: derselbe Weg wie Befund G1-A2, nur mit einem Bild statt Glyphen
// ---------------------------------------------------------------------------

/// Zwei Seiten zeichnen dasselbe Formular. Der Spiegel steht auf Seite 1, die
/// Schwärzung kommt von Seite 2. Für **Glyphen** ist genau das Befund G1-A2 und
/// gedeckt ([`DeferredMirror`]); die Bildfrage geht diesen Weg nicht mit —
/// `DeferredMirror::touched` fragt allein `form_plans`.
#[test]
fn spiegel_auf_seite_eins_bild_von_seite_zwei_geschwaerzt() {
    let mut doc = Document::with_version("1.5");
    let (form, _im0) = formular_mit_bild(&mut doc, None);
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Fm0" => form },
    });
    // Seite 1 mit Spiegel, Seite 2 ohne — beide zeichnen Fm0.
    let eins = doc.add_object(Stream::new(
        dictionary! {},
        format!("/Figure <</Alt ({GEHEIM})>> BDC\nq /Fm0 Do Q\nEMC\n").into_bytes(),
    ));
    let zwei = doc.add_object(Stream::new(dictionary! {}, b"q /Fm0 Do Q\n".to_vec()));
    let pages_id = doc.new_object_id();
    let seite_eins = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => eins,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    let seite_zwei = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => zwei,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(seite_eins), Object::Reference(seite_zwei)],
            "Count" => 2_i64,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);

    // Die Schwärzung liegt auf **Seite 2**.
    let list = vec![Redaction::new(
        Region::new(
            1,
            ueber_dem_bild(),
            None,
            Source::Manual {
                reason: "Bildfläche".into(),
            },
        ),
        Action::Blackout,
    )];
    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(&mut doc, &list)
        .expect("Schwärzung läuft");
    let out = save_to_bytes(&doc).expect("Speichern");
    assert_eq!(report.redacted_images, 1, "die Pixel fallen wirklich");
    assert!(
        leaks(&out, GEHEIM).is_empty(),
        "Spiegel auf Seite 1: {:?}",
        leaks(&out, GEHEIM)
    );
}
