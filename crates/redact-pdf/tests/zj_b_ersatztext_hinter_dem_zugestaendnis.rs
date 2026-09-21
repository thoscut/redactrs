//! Gegenprüfung J-B, zweite Linse — **Ergebnis: der Einwand hält nicht, die
//! Antwort hält.** Dieser Lauf ist ein Freispruch, kein Befund.
//!
//! Der Verdacht war: `keep_image_dicts = self.allow_undecodable_images`
//! (`redact.rs`) schiebt den FEHLALARM aus Runde I nur hinter einen Schalter.
//! Der Befund aus Runde I (`zi_b_falschalarm_bild::
//! geteiltes_logo_behaelt_seinen_ersatztext_auf_der_unberuehrten_seite`): ein
//! Logo auf zwei Seiten, geschwärzt nur auf Seite 1; `crate::image` legt eine
//! **Kopie** an, Seite 2 behält das alte Objekt mit unversehrten Pixeln, und
//! `clear_image_alternates` nahm diesem alten Objekt sein `/Alt`. Wenn das
//! Gatter nur die Betriebsart abfragt, müsste derselbe Schaden mit
//! `--allow-undecodable-images` unverändert eintreten — an einem Bild, das
//! tadellos dekodierbar ist.
//!
//! **Er tritt nicht ein**, und zwar aus einem Grund, der in keiner der
//! geänderten Doku-Stellen steht: `crate::image::redact_images` läuft **vor**
//! der Seitenschleife und hat die Ressourcen der geschwärzten Seite schon auf
//! die Kopie umgebogen (`repoint_page`). Was der Scan danach als
//! `ImagePlacement::id` liefert, ist die **Kopie** — und deren Dictionary hat
//! `crate::image` neu aufgebaut, es trägt gar kein `/Alt` mehr.
//! `clear_image_alternates` findet dort nichts, in beiden Betriebsarten.
//!
//! Beide Läufe unten sind grün; gemessen mit dem Leck-Orakel und am Objekt,
//! auf das Seite 2 nach dem Lauf zeigt. Gegenprobe zum Gatter: dieselben
//! Läufe bleiben grün, wenn man `if keep_image_dicts` in der Seitenschleife
//! weglässt (Mutation unter flock gefahren, md5 vor == nach == d07f0a0a,
//! danach wieder 4feb3b78). Für **diesen** Fall ist das Gatter also nicht
//! einmal nötig — es wirkt dort, wo das Bilddictionary stehen bleibt.

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, PdfRedactor, RedactionReport};

/// Die Beschreibung des Logos — gewöhnliches Material, nichts Schützenswertes.
const LOGO: &str = "Firmenlogo der Musterbank";

// ---------------------------------------------------------------------------
// Gerüst — dieselbe Datei wie in Runde I
// ---------------------------------------------------------------------------

/// Ein FlateDecode-RGB-Bild, mühelos dekodierbar.
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

/// Ein Logo im Briefkopf **beider** Seiten: ein Bild-XObject, zwei Seiten mit
/// eigenem `/Resources`. Geschwärzt wird nur auf Seite 1.
fn zwei_seiten_ein_logo() -> (Document, Vec<ObjectId>, ObjectId) {
    let mut doc = Document::with_version("1.5");
    let mut logo = bild(32, 32);
    logo.dict.set("Alt", Object::string_literal(LOGO));
    let logo_id = doc.add_object(Object::Stream(logo));
    let res = |doc: &mut Document| {
        doc.add_object(dictionary! {
            "XObject" => dictionary! { "Logo" => logo_id },
        })
    };
    let res1 = res(&mut doc);
    let res2 = res(&mut doc);
    let inhalt = "q 100 0 0 100 50 700 cm /Logo Do Q\n";

    let pages_id = doc.new_object_id();
    let mut ids = Vec::new();
    for resources_id in [res1, res2] {
        let content_id = doc.add_object(Stream::new(dictionary! {}, inhalt.as_bytes().to_vec()));
        ids.push(doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        }));
    }
    let kids: Vec<Object> = ids.iter().map(|id| Object::Reference(*id)).collect();
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => 2_i64,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    (doc, ids, logo_id)
}

fn schwaerzung(page: usize, rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            page,
            rect,
            None,
            Source::Manual {
                reason: "Gegenprüfung J-B".into(),
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
    let redactor = PdfRedactor {
        padding: 0.0,
        allow_undecodable_images: zugestaendnis,
        ..PdfRedactor::default()
    };
    let report = redactor
        .apply_with_report(doc, list)
        .expect("Schwärzung läuft");
    let bytes = save_to_bytes(doc).expect("Speichern");
    (report, bytes)
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

/// Auf welches Objekt zeigt `/XObject /Logo` dieser Seite **nach** dem Lauf?
fn logo_der_seite(doc: &Document, page_id: ObjectId) -> ObjectId {
    let page = doc.get_dictionary(page_id).expect("Seite");
    let (_, res) = doc
        .dereference(page.get(b"Resources").expect("Resources"))
        .expect("Resources lesen");
    let (_, xobjects) = doc
        .dereference(
            res.as_dict()
                .expect("Dict")
                .get(b"XObject")
                .expect("XObject"),
        )
        .expect("XObject lesen");
    match xobjects
        .as_dict()
        .expect("Dict")
        .get(b"Logo")
        .expect("Logo")
    {
        Object::Reference(id) => *id,
        other => panic!("kein Verweis: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Der Freispruch
// ---------------------------------------------------------------------------

/// **Der Freispruch.** In *beiden* Betriebsarten behält das unversehrte Bild
/// auf Seite 2 seine Beschreibung — auch mit `--allow-undecodable-images`,
/// wo `blacked_images` gefüllt wird.
#[test]
fn geteiltes_logo_behaelt_seinen_ersatztext_in_beiden_betriebsarten() {
    for zugestaendnis in [false, true] {
        let (mut doc, seiten, logo_id) = zwei_seiten_ein_logo();
        let (report, out) = schwaerze(
            &mut doc,
            &[schwaerzung(0, Rect::new(60.0, 710.0, 90.0, 740.0))],
            zugestaendnis,
        );
        assert_eq!(
            report.redacted_images, 1,
            "Vorbedingung: ein Bild verliert Pixel (Zugeständnis = {zugestaendnis})"
        );
        assert_eq!(
            report.copied_images, 1,
            "Vorbedingung: crate::image legt eine Kopie an"
        );

        let aus = load_from_bytes(&out).expect("Ausgabe lädt");
        let auf_seite2 = logo_der_seite(&aus, seiten[1]);
        assert_eq!(
            auf_seite2, logo_id,
            "Vorbedingung: Seite 2 zeigt weiter auf das ursprüngliche Bildobjekt"
        );
        assert_ne!(
            logo_der_seite(&aus, seiten[0]),
            logo_id,
            "Vorbedingung: Seite 1 zeigt auf die geschwärzte Kopie"
        );
        assert_eq!(
            alt_am_objekt(&aus, auf_seite2).as_deref(),
            Some(LOGO),
            "das Bild, das Seite 2 bedient, hat keine Schwärzung gesehen und behält \
             seine Beschreibung (Zugeständnis = {zugestaendnis}) — \
             image_alt_texts_cleared == {}",
            report.image_alt_texts_cleared
        );
        assert!(
            !leaks(&out, LOGO).is_empty(),
            "die Beschreibung steht noch in der Datei (Zugeständnis = {zugestaendnis})"
        );
        assert_eq!(
            report.image_alt_texts_cleared, 0,
            "und der Bericht behauptet nichts anderes (Zugeständnis = {zugestaendnis})"
        );
    }
}

/// Der Grund, festgenagelt: die Platzierung, die der Scan sieht, zeigt nach
/// `crate::image` auf die **Kopie**, nicht mehr auf das geteilte Original.
/// Daran hängt der Freispruch oben — fällt diese Zusicherung, kommt der Befund
/// aus Runde I mit dem Zugeständnis zurück.
#[test]
fn der_scan_sieht_nach_dem_bildlauf_die_kopie() {
    let (mut doc, seiten, logo_id) = zwei_seiten_ein_logo();
    let vorher = redact_pdf::scan_page(&doc, seiten[0]).expect("Scan vor dem Lauf");
    assert_eq!(
        vorher.images[0].id,
        Some(logo_id),
        "vor dem Lauf zeigt die Platzierung auf das geteilte Original"
    );
    schwaerze(
        &mut doc,
        &[schwaerzung(0, Rect::new(60.0, 710.0, 90.0, 740.0))],
        true,
    );
    let nachher = redact_pdf::scan_page(&doc, seiten[0]).expect("Scan nach dem Lauf");
    assert_ne!(
        nachher.images[0].id,
        Some(logo_id),
        "nach dem Bildlauf zeigt sie auf die Kopie — deshalb erreicht \
         clear_image_alternates das Original nie"
    );
}
