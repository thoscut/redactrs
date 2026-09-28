//! Gegenprobe I-B unter der Linse **falscher Alarm**: nimmt die Korrektur zu
//! Register #20 (Ersatztext über einem geschwärzten Bild) gewöhnlichem Material
//! etwas weg, das es braucht?
//!
//! Die Korrektur räumt zwei Dinge ab, sobald die **Fläche** einer
//! Bildplatzierung ein Schwärzungsrechteck schneidet:
//!
//! 1. den Textspiegel des Marked-Content-Abschnitts, in dem die Platzierung
//!    liegt (`crate::redact::mirrors_to_clear`), und
//! 2. `/Alt` und `/ActualText` am Bilddictionary selbst
//!    (`crate::redact::clear_image_alternates`).
//!
//! Punkt 2 arbeitet auf einer **Objekt-Id** — und eine Objekt-Id ist nicht
//! dasselbe wie eine Platzierung. Ein Bild, das mehrere Seiten benutzen (das
//! Briefkopflogo ist der Normalfall), wird von `crate::image` **kopiert**: die
//! geschwärzten Pixel gehen in ein neues Objekt, das alte Objekt behält seine
//! Pixel und bedient weiter die Seiten ohne Schwärzung
//! (`crate::image::write_work`, Zweig `isolable`). `clear_image_alternates`
//! bekommt die **alte** Id.
//!
//! Gemessen wird an der Ausgabedatei und am Leck-Orakel, nicht am Bericht.

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, PdfRedactor, RedactionReport};

/// Die Beschreibung des Logos — gewöhnliches Material, nichts Schützenswertes.
const LOGO: &str = "Firmenlogo der Musterbank";
/// Der Text, den die Schwärzung auf Seite 1 treffen soll.
const ZIEL: &str = "Geheim";

// ---------------------------------------------------------------------------
// Gerüst
// ---------------------------------------------------------------------------

/// Ein FlateDecode-RGB-Bild.
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

/// Ein **gültiges** Baseline-JPEG, 64 x 64, ein Kanal, gleichmäßig grau.
///
/// Eigene Huffman-Tabellen mit je einem Code der Länge 1: DC-Symbol 0
/// (Differenz 0, keine Zusatzbits) und AC-Symbol 0 (EOB). Je Block also die
/// zwei Bits `00`; 8 x 8 Blöcke ergeben genau 16 Nullbytes, damit auch kein
/// `FF`-Stuffing. Alle DC-Koeffizienten 0 heißt nach der Pegelverschiebung
/// Grauwert 128 — ein Scan, wie ihn `zune-jpeg` dekodiert.
fn jpeg_grau_64() -> Vec<u8> {
    let mut out: Vec<u8> = vec![0xFF, 0xD8]; // SOI
                                             // DQT: Präzision 8 Bit, Tabelle 0, alle Werte 1.
    out.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
    out.extend(std::iter::repeat_n(0x01u8, 64));
    // SOF0: 8 Bit, 64 x 64, ein Kanal, Abtastung 1x1, Quantisierungstabelle 0.
    out.extend_from_slice(&[
        0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x40, 0x00, 0x40, 0x01, 0x01, 0x11, 0x00,
    ]);
    // DHT DC (Tc=0, Th=0): ein Code der Länge 1 für Symbol 0x00.
    out.extend_from_slice(&[0xFF, 0xC4, 0x00, 0x14, 0x00, 0x01]);
    out.extend(std::iter::repeat_n(0x00u8, 15));
    out.push(0x00);
    // DHT AC (Tc=1, Th=0): ein Code der Länge 1 für Symbol 0x00 (EOB).
    out.extend_from_slice(&[0xFF, 0xC4, 0x00, 0x14, 0x10, 0x01]);
    out.extend(std::iter::repeat_n(0x00u8, 15));
    out.push(0x00);
    // SOS: ein Kanal, DC-Tabelle 0, AC-Tabelle 0, Ss=0, Se=63, Ah/Al=0.
    out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);
    // 64 Blöcke x 2 Bit = 16 Byte, alle Bits 0.
    out.extend(std::iter::repeat_n(0x00u8, 16));
    out.extend_from_slice(&[0xFF, 0xD9]); // EOI
    out
}

/// Ein Scan-Bild hinter `/DCTDecode`.
fn scanbild() -> Stream {
    Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 64_i64,
            "Height" => 64_i64,
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 8_i64,
            "Filter" => "DCTDecode",
        },
        jpeg_grau_64(),
    )
    .with_compression(false)
}

/// Eine Schrift, damit der Text im Strom auch Glyphen hat.
fn schrift(doc: &mut Document) -> ObjectId {
    doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    })
}

/// Baut ein Dokument aus je Seite (Ressourcen-Id, Inhalt) und gibt die
/// Seiten-Ids in Reihenfolge zurück.
fn dokument(mut doc: Document, seiten: Vec<(ObjectId, Vec<u8>)>) -> (Document, Vec<ObjectId>) {
    let pages_id = doc.new_object_id();
    let mut ids = Vec::new();
    for (resources_id, content) in seiten {
        let content_id = doc.add_object(Stream::new(dictionary! {}, content));
        ids.push(doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        }));
    }
    let kids: Vec<Object> = ids.iter().map(|id| Object::Reference(*id)).collect();
    let count = kids.len() as i64;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => count,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    (doc, ids)
}

/// **Der gewöhnliche Fall.** Ein Logo, das auf beiden Seiten im Briefkopf
/// steht: **ein** Bild-XObject, von beiden Seiten aus benutzt, jede Seite mit
/// eigenem `/Resources`. Auf Seite 1 steht der Text, der geschwärzt wird, und
/// er steht im Briefkopf — das `--padding` der Schwärzung reicht damit an das
/// Logo. Auf Seite 2 wird nichts geschwärzt.
fn zwei_seiten_ein_logo(logo: Stream) -> (Document, Vec<ObjectId>, ObjectId) {
    let mut doc = Document::with_version("1.5");
    let mut logo = logo;
    logo.dict.set("Alt", Object::string_literal(LOGO));
    let logo_id = doc.add_object(Object::Stream(logo));
    let font = schrift(&mut doc);
    let res = |doc: &mut Document| {
        doc.add_object(dictionary! {
            "XObject" => dictionary! { "Logo" => logo_id },
            "Font" => dictionary! { "F1" => font },
        })
    };
    let res1 = res(&mut doc);
    let res2 = res(&mut doc);
    // Das Logo bei (50,700)-(150,800), der Zieltext direkt daneben.
    let seite1 = format!(
        "/Figure <</Alt ({LOGO})>> BDC\nq 100 0 0 100 50 700 cm /Logo Do Q\nEMC\n\
         BT /F1 12 Tf 200 740 Td ({ZIEL}) Tj ET\n"
    );
    let seite2 = format!(
        "/Figure <</Alt ({LOGO})>> BDC\nq 100 0 0 100 50 700 cm /Logo Do Q\nEMC\n\
         BT /F1 12 Tf 200 740 Td (Seite zwei bleibt ganz) Tj ET\n"
    );
    let (doc, ids) = dokument(
        doc,
        vec![(res1, seite1.into_bytes()), (res2, seite2.into_bytes())],
    );
    (doc, ids, logo_id)
}

fn schwaerzung(page: usize, rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            page,
            rect,
            None,
            Source::Manual {
                reason: "Gegenprobe".into(),
            },
        ),
        Action::Blackout,
    )
}

fn schwaerze(doc: &mut Document, list: &[Redaction]) -> (RedactionReport, Vec<u8>) {
    let report = PdfRedactor::with_padding(0.0)
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

/// Der Hinweis „diese Seite enthält Rasterbilder, ohne OCR wird ihr Inhalt
/// nicht gelesen" steht in jedem Lauf mit einem Bild und hat nichts mit der
/// hier geprüften Korrektur zu tun. Alles andere wäre eine neue Warnung.
fn nur_ocr_hinweis(report: &RedactionReport) -> bool {
    report
        .warnings
        .iter()
        .all(|w| w.contains("enthalten Rasterbilder"))
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
// Der Befund
// ---------------------------------------------------------------------------

/// **Der Befund.** Das Logo steht auf Seite 1 und Seite 2. Geschwärzt wird nur
/// auf Seite 1. `crate::image` legt dafür eine **Kopie** an: Seite 1 zeigt auf
/// das neue Objekt mit den geschwärzten Pixeln, Seite 2 weiter auf das alte —
/// dessen Pixel unverändert sind.
///
/// `clear_image_alternates` nimmt dem **alten** Objekt sein `/Alt`. Seite 2
/// verliert damit die Beschreibung eines Bildes, das nichts verloren hat: kein
/// Pixel, keine Schwärzung, keine Seite mit einer Schwärzung. Genau die
/// Gegenrichtung, die `zh_b_bildspiegel::bild_neben_der_schwaerzung_behaelt_\
/// seinen_ersatztext` für ein *zweites* Bild zusichert — nur hier für dasselbe.
#[test]
fn geteiltes_logo_behaelt_seinen_ersatztext_auf_der_unberuehrten_seite() {
    let (mut doc, seiten, logo_id) = zwei_seiten_ein_logo(bild(32, 32));
    // Das Rechteck liegt mitten im Logo auf Seite 1.
    let (report, out) = schwaerze(
        &mut doc,
        &[schwaerzung(0, Rect::new(60.0, 710.0, 90.0, 740.0))],
    );
    assert_eq!(
        report.redacted_images, 1,
        "Vorbedingung: ein Bild verliert Pixel"
    );
    assert_eq!(
        report.copied_images, 1,
        "Vorbedingung: das geteilte Bild wird kopiert, das alte Objekt bleibt für Seite 2"
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
        "das Bild, das Seite 2 bedient, hat keine Schwärzung gesehen und muss \
         seine Beschreibung behalten — `image_alt_texts_cleared` war {}",
        report.image_alt_texts_cleared
    );
    assert!(
        !leaks(&out, LOGO).is_empty(),
        "die Beschreibung des unberührten Bildes ist ganz aus der Datei verschwunden"
    );
}

/// Dieselbe Datei, andere Richtung: die Pixel auf Seite 2 bleiben, dort ist
/// nichts geschwärzt. Das ist die Vorbedingung dafür, dass der Verlust des
/// `/Alt` oben ein Verlust **ohne Gegenwert** ist.
#[test]
fn geteiltes_logo_behaelt_auf_seite_zwei_seine_pixel() {
    let (mut doc, seiten, logo_id) = zwei_seiten_ein_logo(bild(32, 32));
    let vorher = match doc.get_object(logo_id).expect("Bild") {
        Object::Stream(stream) => stream.content.clone(),
        other => panic!("kein Strom: {other:?}"),
    };
    let (_, out) = schwaerze(
        &mut doc,
        &[schwaerzung(0, Rect::new(60.0, 710.0, 90.0, 740.0))],
    );
    let aus = load_from_bytes(&out).expect("Ausgabe lädt");
    let auf_seite2 = logo_der_seite(&aus, seiten[1]);
    let nachher = match aus.get_object(auf_seite2).expect("Bild") {
        Object::Stream(stream) => stream.content.clone(),
        other => panic!("kein Strom: {other:?}"),
    };
    assert_eq!(
        vorher, nachher,
        "die Pixel des Bildes auf Seite 2 sind unverändert"
    );
}

// ---------------------------------------------------------------------------
// Die Gegenrichtung: gewöhnliches Material bleibt ganz
// ---------------------------------------------------------------------------

/// Ein gewöhnliches getaggtes Dokument, eine Schwärzung **weit weg** vom Bild:
/// der Spiegel bleibt, das `/Alt` am Bild bleibt, der Zähler bleibt 0, und das
/// Bild behält seine Pixel.
#[test]
fn getaggtes_dokument_mit_schwaerzung_neben_dem_bild_verliert_nichts() {
    let (mut doc, _seiten, logo_id) = zwei_seiten_ein_logo(bild(32, 32));
    // (200,740) ist die Textzeile, das Logo liegt bei (50,700)-(150,800).
    let (report, out) = schwaerze(
        &mut doc,
        &[schwaerzung(0, Rect::new(198.0, 736.0, 260.0, 754.0))],
    );
    assert!(
        report.removed_glyphs > 0,
        "Vorbedingung: die Schwärzung trifft Glyphen"
    );
    assert_eq!(
        report.redacted_images, 0,
        "kein Bild wird angefasst: {:?}",
        report.warnings
    );
    assert_eq!(
        report.image_alt_texts_cleared, 0,
        "kein Ersatztext fällt: {:?}",
        report.warnings
    );
    assert!(
        nur_ocr_hinweis(&report),
        "keine Warnung außer dem OCR-Hinweis: {:?}",
        report.warnings
    );
    let aus = load_from_bytes(&out).expect("Ausgabe lädt");
    assert_eq!(alt_am_objekt(&aus, logo_id).as_deref(), Some(LOGO));
    assert!(
        !leaks(&out, &format!("/Figure <</Alt({LOGO})>> BDC")).is_empty(),
        "der Spiegel bleibt im Strom"
    );
    assert!(leaks(&out, ZIEL).is_empty(), "der Zieltext ist weg");
}

/// Ein Scan hinter `/DCTDecode` — der gewöhnlichste Bildfall überhaupt. Die
/// Schwärzung liegt neben dem Bild: der Lauf darf nicht abbrechen, nicht warnen
/// und dem Scan nichts nehmen.
#[test]
fn scan_mit_dctdecode_ohne_treffer_bleibt_unberuehrt() {
    let mut doc = Document::with_version("1.5");
    let mut scan = scanbild();
    scan.dict.set("Alt", Object::string_literal(LOGO));
    let scan_id = doc.add_object(Object::Stream(scan));
    let font = schrift(&mut doc);
    let res = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => scan_id },
        "Font" => dictionary! { "F1" => font },
    });
    let content = format!(
        "/Figure <</Alt ({LOGO})>> BDC\nq 200 0 0 200 50 500 cm /Im0 Do Q\nEMC\n\
         BT /F1 12 Tf 60 200 Td ({ZIEL}) Tj ET\n"
    );
    let (mut doc, _) = dokument(doc, vec![(res, content.into_bytes())]);
    let (report, out) = schwaerze(
        &mut doc,
        &[schwaerzung(0, Rect::new(58.0, 196.0, 120.0, 214.0))],
    );
    assert!(report.removed_glyphs > 0, "Vorbedingung: Glyphen fallen");
    assert_eq!(report.redacted_images, 0);
    assert_eq!(report.image_alt_texts_cleared, 0);
    assert!(
        nur_ocr_hinweis(&report),
        "keine Warnung außer dem OCR-Hinweis: {:?}",
        report.warnings
    );
    let aus = load_from_bytes(&out).expect("Ausgabe lädt");
    assert_eq!(alt_am_objekt(&aus, scan_id).as_deref(), Some(LOGO));
}

/// Derselbe Scan, jetzt **mit** Treffer: er muss sich dekodieren lassen (sonst
/// prüft der Test oben nichts Echtes), seine Pixel verlieren und seinen
/// Ersatztext abgeben. Ohne diesen Test wäre `jpeg_grau_64` vielleicht gar kein
/// JPEG, und `scan_mit_dctdecode_ohne_treffer_bleibt_unberuehrt` grün aus dem
/// falschen Grund.
#[test]
fn scan_mit_dctdecode_und_treffer_wird_wirklich_dekodiert() {
    let mut doc = Document::with_version("1.5");
    let mut scan = scanbild();
    scan.dict.set("Alt", Object::string_literal(LOGO));
    let scan_id = doc.add_object(Object::Stream(scan));
    let font = schrift(&mut doc);
    let res = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => scan_id },
        "Font" => dictionary! { "F1" => font },
    });
    let content = format!(
        "/Figure <</Alt ({LOGO})>> BDC\nq 200 0 0 200 50 500 cm /Im0 Do Q\nEMC\n\
         BT /F1 12 Tf 60 200 Td ({ZIEL}) Tj ET\n"
    );
    let (mut doc, _) = dokument(doc, vec![(res, content.into_bytes())]);
    let (report, out) = schwaerze(
        &mut doc,
        &[schwaerzung(0, Rect::new(100.0, 550.0, 160.0, 610.0))],
    );
    assert_eq!(
        report.redacted_images, 1,
        "das DCTDecode-Bild muss dekodiert und überschrieben werden: {:?}",
        report.warnings
    );
    assert!(
        nur_ocr_hinweis(&report),
        "keine Warnung außer dem OCR-Hinweis: {:?}",
        report.warnings
    );
    assert!(leaks(&out, LOGO).is_empty(), "{:?}", leaks(&out, LOGO));
}

// ---------------------------------------------------------------------------
// Material für den Lauf am gebauten Binär
// ---------------------------------------------------------------------------

/// Schreibt die Korpora dieses Moduls als PDF-Dateien, damit derselbe Fall am
/// gebauten Binär gefahren werden kann. Läuft nur mit gesetztem `ZI_B_OUT`.
#[test]
fn material_schreiben() {
    let Ok(dir) = std::env::var("ZI_B_OUT") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("Verzeichnis");

    let (doc, _, _) = zwei_seiten_ein_logo(bild(32, 32));
    std::fs::write(
        dir.join("geteiltes_logo.pdf"),
        save_to_bytes(&doc).expect("Speichern"),
    )
    .expect("schreiben");

    let mut doc = Document::with_version("1.5");
    let mut scan = scanbild();
    scan.dict.set("Alt", Object::string_literal(LOGO));
    let scan_id = doc.add_object(Object::Stream(scan));
    let font = schrift(&mut doc);
    let res = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => scan_id },
        "Font" => dictionary! { "F1" => font },
    });
    let content = format!(
        "/Figure <</Alt ({LOGO})>> BDC\nq 200 0 0 200 50 500 cm /Im0 Do Q\nEMC\n\
         BT /F1 12 Tf 60 200 Td ({ZIEL}) Tj ET\n"
    );
    let (doc, _) = dokument(doc, vec![(res, content.into_bytes())]);
    std::fs::write(
        dir.join("scan_dct.pdf"),
        save_to_bytes(&doc).expect("Speichern"),
    )
    .expect("schreiben");

    // Das gedrehte Bild samt Text in der leeren Ecke seiner Hülle.
    let mut doc = Document::with_version("1.5");
    let mut gedreht = bild(32, 32);
    gedreht.dict.set("Alt", Object::string_literal(LOGO));
    let bild_id = doc.add_object(Object::Stream(gedreht));
    let font = schrift(&mut doc);
    let res = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => bild_id },
        "Font" => dictionary! { "F1" => font },
    });
    let content = format!(
        "/Figure <</Alt ({LOGO})>> BDC\n\
         q 70.7107 70.7107 -70.7107 70.7107 300 400 cm /Im0 Do Q\nEMC\n\
         BT /F1 12 Tf 233 406 Td ({ZIEL}) Tj ET\n"
    );
    let (doc, _) = dokument(doc, vec![(res, content.into_bytes())]);
    std::fs::write(
        dir.join("gedreht.pdf"),
        save_to_bytes(&doc).expect("Speichern"),
    )
    .expect("schreiben");
}

// ---------------------------------------------------------------------------
// Die Hülle ist nicht die Fläche
// ---------------------------------------------------------------------------

/// **Der Befund.** Ein um 45° gedrehtes Bild — eine Abbildung, ein Stempel, ein
/// schräg gesetztes Logo. Seine **Hülle** (das achsenparallele Rechteck über
/// alle vier Ecken) ist doppelt so groß wie seine Fläche; in den vier Ecken der
/// Hülle liegt gar kein Bild, und dort steht in einem gewöhnlichen Dokument
/// Text.
///
/// Wird dieser Text geschwärzt, dann gilt für `crate::image` richtigerweise:
/// kein Bildpunkt wird getroffen (`covers` prüft die Ecken der Pixelzelle),
/// das Bild wird nicht angefasst, `redacted_images` bleibt 0 und es gibt keine
/// Warnung. Die neue Vorauswahl in `redact.rs` fragt aber nur die **Hülle** —
/// und nimmt dem unversehrten Bild darauf `/Alt` weg und dem `/Figure`-Abschnitt
/// darüber seinen Spiegel.
///
/// Das ist der falsche Alarm: Barrierefreiheit weg, kein Pixel gewonnen, keine
/// Warnung, und der Bericht sagt „Überschriebene Bilder: 0“ neben
/// „Ersatztexte: 2".
///
/// **Lauf:** `cargo test -p redact-pdf --test zi_b_falschalarm_bild -- --ignored
/// gedrehtes_bild_verliert`. `#[ignore]` nach Hausbrauch (vgl.
/// `zi_a_ort_gegenprobe.rs`), damit ein offener Befund das Gate nicht rot
/// färbt — der Befund steht im Bericht.
#[test]
#[ignore = "offener Befund: die Hülle eines gedrehten Bildes ist nicht seine Fläche — /Alt und Spiegel fallen, obwohl kein Bildpunkt fällt"]
fn gedrehtes_bild_verliert_seinen_ersatztext_an_der_leeren_huellenecke() {
    let mut doc = Document::with_version("1.5");
    let mut gedreht = bild(32, 32);
    gedreht.dict.set("Alt", Object::string_literal(LOGO));
    let bild_id = doc.add_object(Object::Stream(gedreht));
    let font = schrift(&mut doc);
    let res = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => bild_id },
        "Font" => dictionary! { "F1" => font },
    });
    // 45° gedreht, Kantenlänge 100: die Ecken liegen bei (300,400), (370.7,470.7),
    // (229.3,470.7) und (300,541.4). Die Hülle ist (229.3,400)-(370.7,541.4);
    // ihre linke untere Ecke ist leer.
    let content = format!(
        "/Figure <</Alt ({LOGO})>> BDC\n\
         q 70.7107 70.7107 -70.7107 70.7107 300 400 cm /Im0 Do Q\nEMC\n\
         BT /F1 12 Tf 233 406 Td ({ZIEL}) Tj ET\n"
    );
    let (mut doc, _) = dokument(doc, vec![(res, content.into_bytes())]);
    // (232,402)-(252,422): in der Hülle, außerhalb der Raute.
    let (report, out) = schwaerze(
        &mut doc,
        &[schwaerzung(0, Rect::new(232.0, 402.0, 252.0, 422.0))],
    );
    assert_eq!(
        report.redacted_images, 0,
        "Vorbedingung: kein Bildpunkt wird getroffen — {:?}",
        report.warnings
    );
    assert!(
        nur_ocr_hinweis(&report),
        "keine Warnung außer dem OCR-Hinweis: {:?}",
        report.warnings
    );
    let aus = load_from_bytes(&out).expect("Ausgabe lädt");
    println!(
        "Bericht: redacted_images={} copied_images={} image_alt_texts_cleared={} \
         removed_glyphs={} warnings={:?}",
        report.redacted_images,
        report.copied_images,
        report.image_alt_texts_cleared,
        report.removed_glyphs,
        report.warnings
    );
    println!(
        "/Alt am Bildobjekt: {:?}; Spiegel im Strom: {} Fundstelle(n)",
        alt_am_objekt(&aus, bild_id),
        leaks(&out, &format!("/Figure <</Alt({LOGO})>> BDC")).len()
    );
    assert_eq!(
        alt_am_objekt(&aus, bild_id).as_deref(),
        Some(LOGO),
        "das unversehrte gedrehte Bild verliert sein /Alt; \
         image_alt_texts_cleared = {}",
        report.image_alt_texts_cleared
    );
    assert!(
        !leaks(&out, &format!("/Figure <</Alt({LOGO})>> BDC")).is_empty(),
        "der Spiegel über dem unversehrten Bild ist weg; \
         image_alt_texts_cleared = {}",
        report.image_alt_texts_cleared
    );
}

/// Die Gegenrichtung zum Test darüber, damit er nicht aus dem falschen Grund
/// grün wird: dasselbe gedrehte Bild, das Rechteck jetzt **mitten** in der
/// Raute. Dann fallen Pixel, und `/Alt` und Spiegel gehören mit ihnen.
#[test]
fn gedrehtes_bild_mitten_getroffen_verliert_pixel_und_ersatztext() {
    let mut doc = Document::with_version("1.5");
    let mut gedreht = bild(32, 32);
    gedreht.dict.set("Alt", Object::string_literal(LOGO));
    let bild_id = doc.add_object(Object::Stream(gedreht));
    let res = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => bild_id },
    });
    let content = format!(
        "/Figure <</Alt ({LOGO})>> BDC\n\
         q 70.7107 70.7107 -70.7107 70.7107 300 400 cm /Im0 Do Q\nEMC\n"
    );
    let (mut doc, _) = dokument(doc, vec![(res, content.into_bytes())]);
    // Der Mittelpunkt der Raute liegt bei (300, 470.7).
    let (report, out) = schwaerze(
        &mut doc,
        &[schwaerzung(0, Rect::new(285.0, 455.0, 315.0, 485.0))],
    );
    assert_eq!(
        report.redacted_images, 1,
        "Vorbedingung: Pixel fallen — {:?}",
        report.warnings
    );
    assert!(leaks(&out, LOGO).is_empty(), "{:?}", leaks(&out, LOGO));
}
