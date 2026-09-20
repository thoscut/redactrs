//! Fix-Runde 8 (B): der **Ersatztext eines geschwärzten Bildes** (Register #20).
//!
//! Ein getaggtes PDF stellt einer Abbildung ihre Beschreibung bei:
//! `/Figure <</Alt (…)>> BDC /Im0 Do EMC`. Das ist die Standardform der
//! Barrierefreiheit und für sich kein Befund. Steht dort aber, was auf dem Bild
//! zu **lesen** war („Kontoauszug, IBAN DE89 …“), dann überlebte die
//! Beschreibung bis zu dieser Fassung die Pixel-Schwärzung des Bildes: der
//! Abschnitt hat keine Glyphen, also galt er als „unberührt“, und der Klartext
//! blieb im Seitenstrom stehen. Gemessen wird hier ausschließlich mit dem
//! Orakel [`redact_pdf::leaks`], nicht am Bericht des Programms.
//!
//! Die Gegenrichtung ist der schwierige Teil und steht deshalb in derselben
//! Datei: ein Bild, das **keine** Schwärzung trifft, behält seine Beschreibung.
//! Ein Dokument ohne Ersatztexte ist für blinde Leser unbrauchbar.

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{
    leaks, load_from_bytes, save_to_bytes, strip_metadata, PdfRedactor, RedactionReport,
};

/// Was auf dem Bild zu lesen war — und in seiner Beschreibung steht.
const GEHEIM: &str = "Kontoauszug, IBAN DE89 3704 0044 0532 0130 00";
/// Eine Beschreibung, die nichts Schützenswertes sagt.
const HARMLOS: &str = "Firmenlogo der Musterbank";

// ---------------------------------------------------------------------------
// Gerüst
// ---------------------------------------------------------------------------

/// Ein RGB-Bild, dessen Pixel sich alle unterscheiden.
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

/// Ein Bild, dessen Pixel sich **nicht** dekodieren lassen (`/JPXDecode` über
/// Unsinn). `redact_images` fasst es nicht an und sagt das.
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

struct Bau {
    doc: Document,
    /// Das Bild bei (50,600)-(150,700) — die Schwärzung trifft es.
    getroffen: ObjectId,
    /// Das Bild bei (400,100)-(500,200) — keine Schwärzung liegt darauf.
    daneben: ObjectId,
    page_id: ObjectId,
}

/// Eine Seite mit zwei Bildern, jedes in seinem eigenen `/Figure`-Abschnitt und
/// jedes zusätzlich mit `/Alt` am Bilddictionary selbst.
fn zwei_bilder(getroffenes_bild: Stream, alt_getroffen: &str, alt_daneben: &str) -> Bau {
    let mut doc = Document::with_version("1.5");
    let mut erstes = getroffenes_bild;
    erstes
        .dict
        .set("Alt", Object::string_literal(alt_getroffen));
    let getroffen = doc.add_object(Object::Stream(erstes));
    let mut zweites = bild(20, 20);
    zweites.dict.set("Alt", Object::string_literal(alt_daneben));
    let daneben = doc.add_object(Object::Stream(zweites));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => getroffen, "Im1" => daneben },
    });
    let content = format!(
        "/Figure <</Alt ({alt_getroffen})>> BDC\nq 100 0 0 100 50 600 cm /Im0 Do Q\nEMC\n\
         /Figure <</Alt ({alt_daneben})>> BDC\nq 100 0 0 100 400 100 cm /Im1 Do Q\nEMC\n"
    );
    let (doc, page_id) = seite(doc, resources_id, content.into_bytes());
    Bau {
        doc,
        getroffen,
        daneben,
        page_id,
    }
}

/// Hängt Seite, Seitenbaum und Katalog an einen Content-Stream.
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

/// Schwärzt ohne zusätzlichen Rand, speichert und gibt die Bytes zurück —
/// gemessen wird immer an der Ausgabedatei.
fn schwaerze(doc: &mut Document, rects: &[Rect]) -> (RedactionReport, Vec<u8>) {
    schwaerze_mit(doc, rects, false)
}

/// Wie [`schwaerze`], aber mit `allow_undecodable`: ein Bild, dessen Pixel sich
/// nicht dekodieren lassen, bricht den Lauf sonst ab (und zu Recht).
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

/// Das Rechteck, das mitten im ersten Bild liegt.
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

// ---------------------------------------------------------------------------
// Der Befund
// ---------------------------------------------------------------------------

/// **Der Befund.** Das Bild verliert seine Pixel — und der Klartext, der
/// beschreibt, was darauf zu lesen war, ist danach nicht mehr in der Datei.
/// Gefragt wird das Orakel, nicht der Bericht.
///
/// Vor der Korrektur: `leaks` fand den Text sechsmal, im Seitenstrom roh und
/// dekodiert (`/Figure <</Alt(Kontoauszug, IBAN DE89 …)>> BDC`).
///
/// Mutation, die diesen Test rot macht: in `mirrors_to_clear` das
/// `over_blacked_image ||` aus der `touched`-Bedingung nehmen.
#[test]
fn ersatztext_eines_geschwaerzten_bildes_faellt() {
    let mut bau = zwei_bilder(bild(20, 20), GEHEIM, HARMLOS);
    let (report, out) = schwaerze(&mut bau.doc, &[ueber_dem_bild()]);
    assert_eq!(
        report.redacted_images, 1,
        "Vorbedingung: genau ein Bild verliert Pixel"
    );
    assert!(
        leaks(&out, GEHEIM).is_empty(),
        "der Ersatztext des geschwärzten Bildes steht noch in der Datei: {:?}",
        leaks(&out, GEHEIM)
    );
    // Beide Träger sind weg: der Spiegel im Strom und das `/Alt` am Bild.
    let doc = load_from_bytes(&out).expect("Ausgabe lädt");
    assert_eq!(alt_am_objekt(&doc, bau.getroffen), None);
}

/// **Die Gegenrichtung.** Dasselbe Dokument, dieselbe Schwärzung: das zweite
/// Bild liegt daneben und behält seine Beschreibung — im Strom **und** am
/// Bilddictionary. Eine Grenze, die gewöhnliche Barrierefreiheit abräumt, wäre
/// genauso ein Fehler wie die Lücke.
///
/// Mutation, die diesen Test rot macht: in der Seitenschleife die Prüfung
/// `placement.bounds.intersects(rect)` weglassen (jede Platzierung gilt als
/// getroffen).
#[test]
fn bild_neben_der_schwaerzung_behaelt_seinen_ersatztext() {
    let mut bau = zwei_bilder(bild(20, 20), GEHEIM, HARMLOS);
    let (_, out) = schwaerze(&mut bau.doc, &[ueber_dem_bild()]);
    assert!(
        !leaks(&out, HARMLOS).is_empty(),
        "das unberührte Bild verliert seine Beschreibung"
    );
    let doc = load_from_bytes(&out).expect("Ausgabe lädt");
    assert_eq!(
        alt_am_objekt(&doc, bau.daneben).as_deref(),
        Some(HARMLOS),
        "/Alt am unberührten Bild bleibt"
    );
    assert!(
        !leaks(&out, "/Figure <</Alt(Firmenlogo der Musterbank)>> BDC").is_empty(),
        "der Spiegel des unberührten Abschnitts bleibt im Strom"
    );
}

/// Eine Seite **ohne** Schwärzung verliert keinen Ersatztext — auch nicht den
/// eines Bildes, über dem auf einer anderen Seite geschwärzt wird.
#[test]
fn seite_ohne_schwaerzung_behaelt_alle_ersatztexte() {
    let mut bau = zwei_bilder(bild(20, 20), GEHEIM, HARMLOS);
    let (report, out) = schwaerze(&mut bau.doc, &[]);
    assert_eq!(report.image_alt_texts_cleared, 0);
    assert!(!leaks(&out, GEHEIM).is_empty());
    assert!(!leaks(&out, HARMLOS).is_empty());
}

/// **Was es kostet, und dass es dasteht.** Ein geschwärztes Bild verliert seine
/// Beschreibung auch dann, wenn sie nichts Schützenswertes sagt. Das ist die
/// sichere Richtung — aber ein Verlust an Barrierefreiheit, den niemand
/// bemerkt, ist der falsche. Der Bericht zählt ihn.
///
/// Gezählt wird **1**, nicht 2, obwohl zwei Träger fallen: das `/Alt` am
/// Bilddictionary ist schon mit den Pixeln gefallen — ein neu kodiertes Bild
/// bekommt ein neu aufgebautes Dictionary (`crate::image`). Die Zahl zählt
/// entfernte Schlüssel, nicht Absichten; der Gegenbeleg steht in
/// `unlesbares_bild_verliert_seinen_ersatztext_trotzdem`, wo das Dictionary
/// stehen bleibt und die Zahl 2 ist.
///
/// Mutation, die diesen Test rot macht: `fixes.image_alt_texts +=
/// mirror_keys_in(&record.properties)` weglassen (dann steht 0 statt 1).
#[test]
fn harmloser_ersatztext_faellt_mit_und_wird_gezaehlt() {
    let mut bau = zwei_bilder(bild(20, 20), HARMLOS, GEHEIM);
    // Geschwärzt wird jetzt über dem Bild mit der **harmlosen** Beschreibung.
    let (report, out) = schwaerze(&mut bau.doc, &[ueber_dem_bild()]);
    assert_eq!(
        report.image_alt_texts_cleared, 1,
        "der Spiegel im Strom; das /Alt am Bild fiel mit den Pixeln"
    );
    assert!(
        leaks(&out, HARMLOS).is_empty(),
        "auch die harmlose Beschreibung fällt: {:?}",
        leaks(&out, HARMLOS)
    );
    // Und das Bild daneben — hier mit dem Geheimtext — bleibt unberührt: die
    // Schwärzung lag nicht darauf, und dieser Lauf erfindet keine.
    assert!(!leaks(&out, GEHEIM).is_empty());
}

/// Ein Bild, dessen Pixel sich **nicht** dekodieren lassen, wird von
/// `redact_images` nicht angefasst (`redacted_images = 0`, mit Warnung). Der
/// Ersatztext fällt trotzdem: wer die Fläche schwärzen wollte, soll den
/// Klartext darüber nicht behalten.
///
/// Der Test hält damit fest, dass die Korrektur **nicht** davon lebt, dass
/// `crate::image` das Bilddictionary neu aufbaut — eine Zusicherung an anderer
/// Stelle, die hier gerade nicht gilt.
///
/// Mutation, die diesen Test rot macht: die Schleife
/// `for id in blacked_images { … clear_image_alternates … }` weglassen.
#[test]
fn unlesbares_bild_verliert_seinen_ersatztext_trotzdem() {
    let mut bau = zwei_bilder(unlesbares_bild(), GEHEIM, HARMLOS);
    let (report, out) = schwaerze_mit(&mut bau.doc, &[ueber_dem_bild()], true);
    assert_eq!(
        report.redacted_images, 0,
        "Vorbedingung: die Pixel bleiben stehen"
    );
    assert!(
        report.warnings.iter().any(|w| w.contains("dekodieren")),
        "und es wird gesagt: {:?}",
        report.warnings
    );
    assert_eq!(
        report.image_alt_texts_cleared, 2,
        "hier beides: der Spiegel im Strom und das /Alt am stehengebliebenen Dictionary"
    );
    assert!(
        leaks(&out, GEHEIM).is_empty(),
        "Ersatztext steht noch in der Datei: {:?}",
        leaks(&out, GEHEIM)
    );
    let doc = load_from_bytes(&out).expect("Ausgabe lädt");
    assert_eq!(alt_am_objekt(&doc, bau.getroffen), None);
    assert_eq!(alt_am_objekt(&doc, bau.daneben).as_deref(), Some(HARMLOS));
}

/// Der Spiegel steht als **eigenes Objekt** unter `/Resources /Properties`
/// (`/Figure /MC0 BDC`) — derselbe Weg wie beim Spiegel über Glyphen, nur ohne
/// Glyphen darunter.
#[test]
fn spiegel_im_eigenschaftsobjekt_faellt_mit_dem_bild() {
    let mut doc = Document::with_version("1.5");
    let im0 = doc.add_object(Object::Stream(bild(20, 20)));
    let props = doc.add_object(Object::Dictionary(
        dictionary! { "Alt" => Object::string_literal(GEHEIM) },
    ));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => im0 },
        "Properties" => dictionary! { "MC0" => props },
    });
    let (mut doc, _) = seite(
        doc,
        resources_id,
        b"/Figure /MC0 BDC\nq 100 0 0 100 50 600 cm /Im0 Do Q\nEMC\n".to_vec(),
    );
    let (report, out) = schwaerze(&mut doc, &[ueber_dem_bild()]);
    assert_eq!(report.redacted_images, 1);
    assert_eq!(report.image_alt_texts_cleared, 1);
    assert!(
        leaks(&out, GEHEIM).is_empty(),
        "Ersatztext im Eigenschaftsobjekt steht noch da: {:?}",
        leaks(&out, GEHEIM)
    );
}

/// Ein Inline-Bild (`BI … ID … EI`) unter dem Spiegel. Es hat kein eigenes
/// Objekt und wird im Strom ersetzt; der Spiegel darüber geht denselben Weg.
#[test]
fn inline_bild_unter_dem_spiegel() {
    let mut doc = Document::with_version("1.5");
    let resources_id = doc.add_object(dictionary! {});
    let mut content = Vec::new();
    content.extend_from_slice(format!("/Figure <</Alt ({GEHEIM})>> BDC\n").as_bytes());
    content.extend_from_slice(b"q 100 0 0 100 50 600 cm\n");
    content.extend_from_slice(b"BI /W 2 /H 2 /CS /G /BPC 8 ID ");
    content.extend_from_slice(&[0x00, 0xff, 0x7f, 0x30]);
    content.extend_from_slice(b" EI Q\nEMC\n");
    let (mut doc, _) = seite(doc, resources_id, content);
    let (report, out) = schwaerze(&mut doc, &[ueber_dem_bild()]);
    assert_eq!(report.redacted_images, 1, "das Inline-Bild wird ersetzt");
    assert_eq!(
        report.image_alt_texts_cleared, 1,
        "ein Inline-Bild hat kein Dictionary — nur der Spiegel fällt"
    );
    assert!(
        leaks(&out, GEHEIM).is_empty(),
        "Ersatztext über dem Inline-Bild: {:?}",
        leaks(&out, GEHEIM)
    );
}

/// Der Spiegel steht **im Form-XObject**, das Bild darunter auch. Das Formular
/// wird einmal neu geschrieben — auch dann, wenn es sonst nichts verliert
/// (kein Zeichen, kein Inline-Bild).
///
/// Mutation, die diesen Test rot macht: in der Bildung von `form_ids` das
/// `|| hits.range(record.range.clone()).next().is_some()` weglassen — dann
/// bleibt das Formular ungeschrieben und der Spiegel darin stehen.
#[test]
fn spiegel_im_form_xobject_ueber_einem_bild() {
    let mut doc = Document::with_version("1.5");
    let im0 = doc.add_object(Object::Stream(bild(20, 20)));
    let form_content =
        format!("/Figure <</Alt ({GEHEIM})>> BDC\nq 100 0 0 100 50 600 cm /Im0 Do Q\nEMC\n");
    let form = doc.add_object(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => im0 } },
            },
            form_content.into_bytes(),
        )
        .with_compression(false),
    ));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Fm0" => form },
    });
    let (mut doc, _) = seite(doc, resources_id, b"q /Fm0 Do Q\n".to_vec());
    let (report, out) = schwaerze(&mut doc, &[ueber_dem_bild()]);
    assert_eq!(report.redacted_images, 1);
    assert_eq!(report.image_alt_texts_cleared, 1);
    assert!(
        leaks(&out, GEHEIM).is_empty(),
        "Ersatztext im Formular: {:?}",
        leaks(&out, GEHEIM)
    );
}

/// Die Seite bleibt lesbar und der übrige Inhalt unverändert: der Abschnitt
/// selbst (`/Figure … BDC … EMC`) steht weiter im Strom, nur ohne seinen
/// Spiegel. Ein Betrachter, der die Auszeichnung braucht, findet sie.
#[test]
fn die_auszeichnung_bleibt_nur_der_spiegel_faellt() {
    let mut bau = zwei_bilder(bild(20, 20), GEHEIM, HARMLOS);
    let page_id = bau.page_id;
    let (_, out) = schwaerze(&mut bau.doc, &[ueber_dem_bild()]);
    let doc = load_from_bytes(&out).expect("Ausgabe lädt");
    let content = doc.get_page_content(page_id).expect("Seiteninhalt");
    let text = String::from_utf8_lossy(&content);
    assert!(text.contains("/Figure"), "die Auszeichnung bleibt: {text}");
    assert!(
        text.contains("EMC"),
        "die Klammer bleibt geschlossen: {text}"
    );
    assert!(!text.contains("Kontoauszug"), "{text}");
}

/// Ein um 45° gedrehtes Bild. Seine Fläche wird aus **allen vier** Ecken des
/// transformierten Einheitsquadrats gebildet; bei einer Drehung sind die
/// beiden anderen Ecken die äußeren. Die Schwärzung liegt genau dort — links
/// der Diagonale, mitten im Bild.
///
/// Mutation, die diesen Test rot macht: in `content::unit_square_bounds` nur
/// `(0,0)` und `(1,1)` nehmen (`corners[2..]`-Schleife weglassen).
#[test]
fn gedrehtes_bild_wird_ueber_alle_vier_ecken_gefunden() {
    let mut doc = Document::with_version("1.5");
    let im0 = doc.add_object(Object::Stream(bild(20, 20)));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => im0 },
    });
    // Drehung um 45°, Diagonalen ±70,7 um (300, 470,7).
    let content = format!(
        "/Figure <</Alt ({GEHEIM})>> BDC\n\
         q 70.7 70.7 -70.7 70.7 300 400 cm /Im0 Do Q\nEMC\n"
    );
    let (mut doc, _) = seite(doc, resources_id, content.into_bytes());
    // (265,465)-(275,475) liegt in der Raute, aber links von (300, …) — also
    // außerhalb der Hülle, die nur zwei Ecken ergäben.
    let (report, out) = schwaerze(&mut doc, &[Rect::new(265.0, 465.0, 275.0, 475.0)]);
    assert_eq!(report.redacted_images, 1, "die Pixel fallen wirklich");
    assert_eq!(report.image_alt_texts_cleared, 1);
    assert!(
        leaks(&out, GEHEIM).is_empty(),
        "Ersatztext über dem gedrehten Bild: {:?}",
        leaks(&out, GEHEIM)
    );
}

/// Der andere Träger, den die Aufgabe nennt: das `/StructElem`, das das Bild
/// beschreibt (`/Figure` mit `/Alt`, `/Pg` auf die Seite). Hier steht **kein**
/// Spiegel im Strom.
///
/// Was gilt, und was nicht gilt: die **Schwärzung allein** nimmt ihn nicht — sie
/// sieht die Struktur nicht, und sie darf sie nicht sehen, denn ein Bild, das
/// keine Schwärzung trifft, soll seine Beschreibung behalten. Genommen wird er
/// vom **Metadatenlauf**, der in der Verarbeitungskette immer läuft: mit
/// `/StructTreeRoot` fällt der ganze `/K`-Baum und wird weggeräumt (dieselbe
/// Zusage wie in `zg_r3_barrierefrei` und `zf_q1_luecken`, dort in der
/// Gegenrichtung gemessen).
///
/// Der Test hält beide Hälften fest, damit niemand die erste für die zweite
/// nimmt.
#[test]
fn ersatztext_am_strukturelement_faellt_mit_dem_metadatenlauf() {
    let mut doc = Document::with_version("1.5");
    let im0 = doc.add_object(Object::Stream(bild(20, 20)));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => im0 },
    });
    let (mut doc, page_id) = seite(
        doc,
        resources_id,
        b"/Figure <</MCID 0>> BDC\nq 100 0 0 100 50 600 cm /Im0 Do Q\nEMC\n".to_vec(),
    );
    let root = doc.new_object_id();
    let figure = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => "StructElem",
        "S" => "Figure",
        "P" => Object::Reference(root),
        "Pg" => Object::Reference(page_id),
        "K" => 0_i64,
        "Alt" => Object::string_literal(GEHEIM),
    }));
    doc.objects.insert(
        root,
        Object::Dictionary(dictionary! {
            "Type" => "StructTreeRoot",
            "K" => Object::Reference(figure),
        }),
    );
    let catalog = doc
        .trailer
        .get(b"Root")
        .and_then(|r| r.as_reference())
        .expect("Katalogverweis");
    doc.get_dictionary_mut(catalog)
        .expect("Katalog")
        .set("StructTreeRoot", Object::Reference(root));

    // Erste Hälfte: die Schwärzung allein lässt das Struktur-Element stehen.
    let (report, nur_geschwaerzt) = schwaerze(&mut doc, &[ueber_dem_bild()]);
    assert_eq!(report.redacted_images, 1);
    assert!(
        !leaks(&nur_geschwaerzt, GEHEIM).is_empty(),
        "die Schwärzung allein liest die Dokumentstruktur nicht — das ist so \
         dokumentiert und keine neue Zusage"
    );

    // Zweite Hälfte: der Metadatenlauf nimmt ihn, und der Bericht nennt die
    // Struktur.
    let metadaten = strip_metadata(&mut doc);
    assert!(metadaten.struct_tree_removed, "{:?}", metadaten.summary());
    let out = save_to_bytes(&doc).expect("Speichern");
    assert!(
        leaks(&out, GEHEIM).is_empty(),
        "Ersatztext am Struktur-Element: {:?}",
        leaks(&out, GEHEIM)
    );
}

/// Messung (ignoriert): was die Bildplatzierungen unter einem Spiegel kosten.
/// Lauf:
/// `cargo test -p redact-pdf --test zh_b_bildspiegel -- --ignored --nocapture`
///
/// Die Liste der Bildplatzierungen wächst **linear** in den Platzierungen,
/// nicht als Produkt „Klammern × Platzierungen“: jede Platzierung ist eine
/// Operation und zahlt aus demselben Aufwandskonto wie jede andere. Eine eigene
/// Decke braucht sie deshalb nicht — anders als die Zuordnung Spiegel↔Formular,
/// die als Produkt entstand (`MAX_MIRROR_FORM_PLACEMENTS`, `zg_r1_decke`).
///
/// Gemessen (Debug, ein Prozess, dieselbe Seite je einmal ohne und einmal mit
/// dem Spiegel darüber; MB sind 1024²):
///
/// | Platzierungen | ohne Spiegel | mit Spiegel |
/// |--------------:|-------------:|------------:|
/// |        10 000 |      1,205 s |     1,194 s |
/// |        50 000 |      6,096 s |     6,082 s |
/// |       100 000 |     12,095 s |    12,023 s |
///
/// Die Wanduhr ist die vergleichbare Spalte: sie gilt je Durchgang und ist mit
/// und ohne Spiegel gleich. Der Speicher wird als `VmHWM` gedruckt, also als
/// **Höchststand des Prozesses** — die Reihen bauen aufeinander auf, die Zahl
/// ist deshalb nur eine obere Schranke (gemessen 60,4 → 62,8 MB bei 10 000,
/// 524,3 → 542,1 MB bei 100 000; die Grundlast dieser Seiten steckt darin).
#[test]
#[ignore = "Messung"]
fn mess_viele_bildplatzierungen() {
    for platzierungen in [10_000usize, 50_000, 100_000] {
        for spiegel in [false, true] {
            let mut doc = Document::with_version("1.5");
            let im0 = doc.add_object(Object::Stream(bild(4, 4)));
            let resources_id = doc.add_object(dictionary! {
                "XObject" => dictionary! { "Im0" => im0 },
            });
            let mut content = String::new();
            if spiegel {
                content.push_str(&format!("/Figure <</Alt ({GEHEIM})>> BDC\n"));
            }
            for _ in 0..platzierungen {
                content.push_str("q 10 0 0 10 400 100 cm /Im0 Do Q\n");
            }
            if spiegel {
                content.push_str("EMC\n");
            }
            let (mut doc, _) = seite(doc, resources_id, content.into_bytes());
            let start = std::time::Instant::now();
            let (report, _) = schwaerze(&mut doc, &[ueber_dem_bild()]);
            println!(
                "{platzierungen} Platzierungen, Spiegel {spiegel}: {:?}, \
                 {} Warnung(en), VmHWM {:.1} MB",
                start.elapsed(),
                report.warnings.len(),
                vmhwm_mb()
            );
        }
    }
}

/// Der Höchststand des Prozesses in MB — und MB heißt hier 1024².
fn vmhwm_mb() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|rest| rest.split_whitespace().next()?.parse::<f64>().ok())
        // `/proc` schreibt KiB, auch wenn „kB“ dasteht.
        .map(|kib| kib / 1024.0)
        .unwrap_or(f64::NAN)
}
