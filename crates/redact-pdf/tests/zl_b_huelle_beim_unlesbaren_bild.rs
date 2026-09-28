//! Gegenprüfung zu Register #20, Runde III: **bei einem unlesbaren Bild
//! entscheidet die Korrektur jetzt an der HÜLLE der Platzierung — vorher an
//! ihrer FLÄCHE.**
//!
//! Runde 1 war der Befund „die Hülle eines gedrehten Bildes ist nicht seine
//! Fläche": in den leeren Ecken der Hülle liegt kein Bildpunkt, und eine
//! Schwärzung dort darf niemandem etwas nehmen. Runde III schließt das für das
//! *lesbare* Bild (die Bildpunkte werden gezählt). Für das **unlesbare** Bild
//! bleibt eine grobe Stelle — und die ist von der Fläche auf die Hülle
//! gewandert:
//!
//! * vorher (HEAD) hing `blacked_images` an
//!   `ImagePlacement::covers(rect)` — dem **Viereck** der Platzierung;
//! * jetzt hängt es an `ImageOutcome::undecodable_images`, und dort landet eine
//!   Platzierung, sobald `ctm_bounds(&ctm).intersects(&zone.rect)` gilt — das
//!   ist die **Hülle**.
//!
//! Für diese Entscheidung ist das Viereck kostenlos zu haben: ob ein Rechteck
//! die Fläche einer Platzierung überlappt, ist eine reine Geometriefrage und
//! braucht kein Dekodieren. Überlappt sie nicht, liegt **kein** Bildpunkt
//! dieses Bildes unter der Schwärzung — lesbar oder nicht. Der Ersatztext fällt
//! hier also ohne einen einzigen Bildpunkt, der ihn rechtfertigt, und das ist
//! genau der Befund aus Runde 1, nur einen Schritt weiter hinten.
//!
//! Gemessen wird an der geladenen Ausgabedatei und am Leck-Orakel.

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, PdfRedactor, RedactionReport};

const SPIEGEL: &str = "Kontoauszug, IBAN DE89 3704 0044 0532 0130 00";
const BILD_ALT: &str = "Scan des Kontoauszugs";

/// Ein Bild, dessen Pixel sich **nicht** dekodieren lassen (JPEG-2000-Rumpf).
fn unlesbares_bild(alt: &str) -> Stream {
    Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 32_i64,
            "Height" => 32_i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8_i64,
            "Filter" => "JPXDecode",
            "Alt" => Object::string_literal(alt),
        },
        b"kein JPEG-2000".to_vec(),
    )
    .with_compression(false)
}

fn eine_seite(mut doc: Document, resources_id: ObjectId, content: Vec<u8>) -> (Document, ObjectId) {
    let content_id = doc.add_object(Stream::new(dictionary! {}, content));
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 842.into(), 842.into()],
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
                reason: "Gegenprüfung Runde III".into(),
            },
        ),
        Action::Blackout,
    )
}

fn schwaerze(doc: &mut Document, rect: Rect) -> (RedactionReport, Vec<u8>) {
    let report = PdfRedactor::with_padding(0.0)
        .allowing_undecodable_images(true)
        .apply_with_report(doc, &[schwaerzung(rect)])
        .expect("mit dem Zugeständnis läuft der Lauf durch");
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

/// Das um 45° gedrehte unlesbare Bild unter einem `/Figure`-Spiegel.
///
/// Kantenlänge 100, gedreht um 45°: die Ecken des Vierecks liegen bei
/// (300,400), (370.71,470.71), (300,541.42), (229.29,470.71). Seine **Hülle**
/// ist (229.29,400)-(370.71,541.42) — die linke untere Ecke der Hülle ist leer.
fn gedrehtes_unlesbares_bild() -> (Document, ObjectId) {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(unlesbares_bild(BILD_ALT)));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => bild_id },
    });
    let content = format!(
        "/Figure <</Alt ({SPIEGEL})>> BDC\n\
         q 70.7107 70.7107 -70.7107 70.7107 300 400 cm /Im0 Do Q\nEMC\n"
    );
    let (doc, _) = eine_seite(doc, resources_id, content.into_bytes());
    (doc, bild_id)
}

/// **Der Befund.** Die Schwärzung liegt in der leeren Ecke der Hülle, weit
/// außerhalb des Vierecks: bei y = 422 reicht das Viereck nur bis x = 278, das
/// Rechteck endet bei x = 252. Kein Bildpunkt dieses Bildes liegt unter der
/// Schwärzung — das ist ohne Dekodieren zu sehen, und `covers` sieht es.
///
/// Trotzdem fallen `/Alt` am Bilddictionary **und** der Spiegel des
/// `/Figure`-Abschnitts. Auf HEAD (vor dieser Runde) blieben beide stehen, weil
/// `blacked_images` an der Fläche hing.
#[test]
fn leere_huellenecke_nimmt_dem_unlesbaren_bild_seinen_ersatztext() {
    let (mut doc, bild_id) = gedrehtes_unlesbares_bild();
    let vorher = match doc.get_object(bild_id).expect("Bild") {
        Object::Stream(s) => s.content.clone(),
        other => panic!("kein Strom: {other:?}"),
    };
    let (report, out) = schwaerze(&mut doc, Rect::new(232.0, 402.0, 252.0, 422.0));

    // Vorbedingung: es fällt wirklich kein Bildpunkt.
    assert_eq!(
        report.redacted_images, 0,
        "Vorbedingung: kein Bild wird überschrieben — {:?}",
        report.warnings
    );
    let nachher = match doc.get_object(bild_id).expect("Bild") {
        Object::Stream(s) => s.content.clone(),
        other => panic!("kein Strom: {other:?}"),
    };
    assert_eq!(
        nachher, vorher,
        "Vorbedingung: die Bildpunkte sind unverändert"
    );

    let aus = load_from_bytes(&out).expect("Ausgabe lädt");
    assert_eq!(
        alt_am_objekt(&aus, bild_id).as_deref(),
        Some(BILD_ALT),
        "das unversehrte Bild verliert seine Beschreibung, obwohl die Schwärzung \
         sein Viereck nicht berührt — entschieden an der Hülle. Bericht: \
         image_alt_texts_cleared = {}, Warnungen = {:?}",
        report.image_alt_texts_cleared,
        report.warnings
    );
    assert!(
        !leaks(&out, SPIEGEL).is_empty(),
        "und der Spiegel über dem unversehrten Bild fällt mit"
    );
    assert_eq!(
        report.image_alt_texts_cleared, 0,
        "nichts fällt, also zählt nichts"
    );
}

/// Die Gegenprobe, damit der Test darüber nicht „nimmt nie etwas weg" prüft:
/// dasselbe unlesbare Bild, die Schwärzung **mitten in der Raute**. Dort darf
/// grob entschieden werden — niemand weiß, was unter ihr lag —, und es wird
/// gesagt.
#[test]
fn mitten_in_der_raute_darf_das_unlesbare_bild_alles_verlieren() {
    let (mut doc, bild_id) = gedrehtes_unlesbares_bild();
    let (report, out) = schwaerze(&mut doc, Rect::new(290.0, 460.0, 310.0, 480.0));

    assert_eq!(report.redacted_images, 0, "unlesbar: die Pixel bleiben");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("nicht dekodieren")),
        "und es wird gesagt: {:?}",
        report.warnings
    );
    assert!(
        leaks(&out, SPIEGEL).is_empty(),
        "der Spiegel fällt: {:?}",
        leaks(&out, SPIEGEL)
    );
    assert!(
        leaks(&out, BILD_ALT).is_empty(),
        "und der Ersatztext am stehengebliebenen Dictionary: {:?}",
        leaks(&out, BILD_ALT)
    );
    let aus = load_from_bytes(&out).expect("Ausgabe lädt");
    assert_eq!(alt_am_objekt(&aus, bild_id), None);
}

/// Die geometrische Kontrolle, damit der Befund oben nicht auf einer Behauptung
/// über die Lage steht: **dasselbe Viereck, dasselbe Rechteck, ein lesbares
/// Bild.** Dann zählt `crate::image` die Bildpunkte selbst — mit der großzügigen
/// Flächenprüfung dieser Runde, bei der Berührung schon als Treffer gilt — und
/// findet keinen. Das Rechteck liegt also wirklich außerhalb des Vierecks:
/// entlang der Kante (300,400)→(229.29,470.71) gilt x + y = 700, im Rechteck ist
/// x + y höchstens 252 + 422 = 674.
///
/// Ein lesbares Bild behält hier alles (das ist
/// `zk_b::leere_huellenecke_nimmt_niemandem_etwas`). Nur das **unlesbare**
/// verliert seine Beschreibung — an derselben Stelle, an derselben Geometrie.
#[test]
fn dieselbe_lage_mit_lesbarem_bild_verliert_keinen_bildpunkt() {
    let mut doc = Document::with_version("1.5");
    let mut data = Vec::with_capacity(32 * 32 * 3);
    for y in 0..32u32 {
        for x in 0..32u32 {
            data.extend_from_slice(&[(x * 7 + 1) as u8, (y * 5 + 3) as u8, 0x20]);
        }
    }
    let mut stream = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 32_i64,
            "Height" => 32_i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8_i64,
            "Alt" => Object::string_literal(BILD_ALT),
        },
        data,
    );
    let _ = stream.compress();
    let bild_id = doc.add_object(Object::Stream(stream));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => bild_id },
    });
    let content = format!(
        "/Figure <</Alt ({SPIEGEL})>> BDC\n\
         q 70.7107 70.7107 -70.7107 70.7107 300 400 cm /Im0 Do Q\nEMC\n"
    );
    let (mut doc, _) = eine_seite(doc, resources_id, content.into_bytes());
    let (report, out) = schwaerze(&mut doc, Rect::new(232.0, 402.0, 252.0, 422.0));

    assert_eq!(
        report.redacted_images, 0,
        "kein einziger Bildpunkt liegt unter dem Rechteck — die Lage ist damit \
         belegt, nicht behauptet: {:?}",
        report.warnings
    );
    assert_eq!(report.image_alt_texts_cleared, 0);
    let aus = load_from_bytes(&out).expect("Ausgabe lädt");
    assert_eq!(alt_am_objekt(&aus, bild_id).as_deref(), Some(BILD_ALT));
    assert!(!leaks(&out, SPIEGEL).is_empty());
}
