//! Was die Eingabe **versteckt**, bleibt in der Ausgabe versteckt.
//!
//! Ein Bild kann Bildpunkte auf drei Arten unsichtbar machen: `/SMask`
//! (Alphaebene), `/Mask` als Stencil-Strom und `/Mask` als Farbschlüssel-Array
//! (PDF 32000-1, 8.9.6.4). Beim Schwärzen wird das Bild neu kodiert und sein
//! Dictionary neu aufgebaut — was dabei nicht ausdrücklich mitgeschrieben wird,
//! ist weg. Eine verlorene Maske dreht das Kernversprechen um: die Stelle, die
//! die Eingabe unsichtbar macht — **das Muster einer bereits mit einem anderen
//! Werkzeug geschwärzten Stelle** — stünde in der Ausgabe wieder sichtbar da.
//!
//! Gemessen wird deshalb an der geschriebenen Datei, Bildpunkt für Bildpunkt:
//! Farbe *und* Alpha. Zusätzlich prüft ein Teil der Tests das Dictionary der
//! Ausgabe, weil dort die Entscheidung fällt (`/Mask` mitschreiben gegen
//! `/SMask` neu bauen).
//!
//! Die zweite Hälfte der Datei ist eine Rückversicherung: die Bildfälle, die
//! heute nachweislich stimmen (gespiegelte CTM, `/Decode [1 0]`, 1 Bit,
//! `Indexed`, `DeviceCMYK`, `/SMask` mit Teiltransparenz), noch einmal mit
//! gezählten Bildpunkten — damit die Maskenänderung sie nicht unbemerkt
//! beschädigt.

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{
    load_from_bytes, page_ops, save_to_bytes, DrawOp, PdfRedactor, RasterImage, RedactionReport,
};

// ---------------------------------------------------------------------------
// Gerüst
// ---------------------------------------------------------------------------

/// Eine Seite 300×300, ein Bild bei (100,100)-(180,180): jeder der 8×8
/// Bildpunkte ist genau 10 Punkt groß.
const PLACE: &str = "q 80 0 0 80 100 100 cm /Im0 Do Q\n";

/// Die rechte Bildhälfte, ohne die Ränder zu berühren: Spalten 4..7.
fn right_half() -> Rect {
    Rect::new(141.0, 101.0, 179.0, 179.0)
}

fn build(xobjects: Vec<(&str, Stream)>, content: &str) -> Document {
    let mut doc = Document::with_version("1.5");
    let mut names = Dictionary::new();
    for (name, stream) in xobjects {
        let id = doc.add_object(Object::Stream(stream));
        names.set(name.as_bytes().to_vec(), Object::Reference(id));
    }
    let resources_id = doc.add_object(dictionary! { "XObject" => names });
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 300.into(), 300.into()],
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
    doc
}

/// Ein Bild-XObject ohne Filter — die Nutzdaten bleiben so, wie sie gemeint
/// sind, und ein Test kann sie im Zweifel in der Datei wiederfinden.
fn image(width: u32, height: u32, extra: Dictionary, data: Vec<u8>) -> Stream {
    let mut dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Image",
        "Width" => i64::from(width),
        "Height" => i64::from(height),
    };
    for (key, value) in extra.iter() {
        dict.set(key.clone(), value.clone());
    }
    Stream::new(dict, data).with_compression(false)
}

/// 8×8: linke Hälfte kräftig rot, rechte Hälfte blau.
fn red_left_blue_right() -> Vec<u8> {
    let mut data = Vec::new();
    for _ in 0..8 {
        for x in 0..8 {
            data.extend_from_slice(if x < 4 {
                &[200, 10, 10]
            } else {
                &[10, 10, 200]
            });
        }
    }
    data
}

fn blackout(rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            0,
            rect,
            None,
            Source::Manual {
                reason: "Maskentest".into(),
            },
        ),
        Action::Blackout,
    )
}

/// Schwärzt ohne Rand, speichert und lädt neu — gemessen wird an der Ausgabe.
fn roundtrip(doc: &mut Document, rect: Rect) -> (RedactionReport, Document) {
    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(doc, &[blackout(rect)])
        .expect("Schwärzung");
    let bytes = save_to_bytes(doc).expect("Speichern");
    (report, load_from_bytes(&bytes).expect("Laden"))
}

fn images_of(doc: &Document) -> Vec<RasterImage> {
    let ops = page_ops(doc, 0).expect("Seitenoperationen");
    ops.ops
        .iter()
        .filter_map(|op| match op {
            DrawOp::Image { image, .. } => Some(ops.images[*image].clone()),
            _ => None,
        })
        .collect()
}

fn first_image(doc: &Document) -> RasterImage {
    images_of(doc).into_iter().next().expect("ein Bild")
}

fn pixel(image: &RasterImage, x: u32, y: u32) -> [u8; 4] {
    let offset = ((y * image.width + x) * 4) as usize;
    [
        image.rgba[offset],
        image.rgba[offset + 1],
        image.rgba[offset + 2],
        image.rgba[offset + 3],
    ]
}

/// Eine Bildzeile als Text: Farbe und Alpha je Bildpunkt.
fn row(image: &RasterImage, y: u32) -> String {
    (0..image.width)
        .map(|x| {
            let p = pixel(image, x, y);
            format!("({},{},{})a{}", p[0], p[1], p[2], p[3])
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Die Objektnummer eines Bildes aus den Seitenressourcen.
fn image_id(doc: &Document, name: &[u8]) -> ObjectId {
    let page_id = *doc.get_pages().values().next().expect("Seite");
    let resources = doc
        .get_dictionary(page_id)
        .and_then(|d| d.get(b"Resources"))
        .and_then(|o| doc.dereference(o).map(|(_, r)| r))
        .and_then(|o| o.as_dict())
        .expect("Ressourcen");
    let xobjects = resources
        .get(b"XObject")
        .and_then(|o| doc.dereference(o).map(|(_, r)| r))
        .and_then(|o| o.as_dict())
        .expect("XObjects");
    match xobjects.get(name).expect("Bild") {
        Object::Reference(id) => *id,
        other => panic!("kein Verweis: {other:?}"),
    }
}

/// Das Bild-Dictionary aus den Seitenressourcen — dort steht, ob `/Mask` oder
/// `/SMask` in der Ausgabe gelandet ist.
fn image_dict(doc: &Document, name: &[u8]) -> Dictionary {
    let page_id = *doc.get_pages().values().next().expect("Seite");
    let resources = doc
        .get_dictionary(page_id)
        .and_then(|d| d.get(b"Resources"))
        .and_then(|o| doc.dereference(o).map(|(_, r)| r))
        .and_then(|o| o.as_dict())
        .expect("Ressourcen");
    let xobjects = resources
        .get(b"XObject")
        .and_then(|o| doc.dereference(o).map(|(_, r)| r))
        .and_then(|o| o.as_dict())
        .expect("XObjects");
    let entry = xobjects.get(name).expect("Bild");
    doc.dereference(entry)
        .expect("auflösbar")
        .1
        .as_stream()
        .expect("Strom")
        .dict
        .clone()
}

fn redaction_error(doc: &mut Document, rect: Rect) -> String {
    PdfRedactor::with_padding(0.0)
        .apply_with_report(doc, &[blackout(rect)])
        .expect_err("die Schwärzung hätte abbrechen müssen")
        .to_string()
}

// ---------------------------------------------------------------------------
// /Mask als Stencil-Strom
// ---------------------------------------------------------------------------

/// Linke Hälfte rot und durch `/Mask` unsichtbar, rechte Hälfte blau.
///
/// Abtastwert 1 heißt „maskiert“ (PDF 32000-1, 8.9.6.4); `0xf0` versteckt also
/// die linken vier Spalten.
fn image_with_stencil_mask() -> Document {
    let mut doc = build(
        vec![(
            "Im0",
            image(
                8,
                8,
                dictionary! {
                    "ColorSpace" => "DeviceRGB",
                    "BitsPerComponent" => 8_i64,
                },
                red_left_blue_right(),
            ),
        )],
        PLACE,
    );
    let mask_id = doc.add_object(Object::Stream(image(
        8,
        8,
        dictionary! {
            "ImageMask" => true,
            "BitsPerComponent" => 1_i64,
        },
        vec![0xf0; 8],
    )));
    let id = image_id(&doc, b"Im0");
    if let Ok(Object::Stream(stream)) = doc.get_object_mut(id) {
        stream.dict.set("Mask", Object::Reference(mask_id));
    }
    doc
}

/// Die Polarität, an der alles hängt: `/Mask`-Abtastwert 1 versteckt.
///
/// Ohne diesen Test ist jede Aussage über die Ausgabe wertlos — wer die Eingabe
/// falsch herum liest, schreibt zwangsläufig das Falsche zurück.
#[test]
fn a_stencil_mask_hides_the_samples_it_marks() {
    let doc = image_with_stencil_mask();
    let before = first_image(&doc);
    for y in 0..8 {
        for x in 0..4 {
            assert_eq!(
                pixel(&before, x, y)[3],
                0,
                "Spalte {x} müsste versteckt sein: {}",
                row(&before, y)
            );
        }
        for x in 4..8 {
            assert_eq!(
                pixel(&before, x, y)[3],
                255,
                "Spalte {x} müsste sichtbar sein: {}",
                row(&before, y)
            );
        }
    }
}

/// Der Kern von Befund 3: die versteckte Hälfte bleibt versteckt.
#[test]
fn a_stencil_mask_is_written_back_unchanged() {
    let mut doc = image_with_stencil_mask();
    let before = first_image(&doc);
    let (report, out) = roundtrip(&mut doc, right_half());
    assert_eq!(report.redacted_images, 1);

    let after = first_image(&out);
    for y in 0..8 {
        for x in 0..4 {
            assert_eq!(
                pixel(&after, x, y),
                pixel(&before, x, y),
                "versteckter Bildpunkt ({x},{y}) hat sich verändert\nvorher : {}\nnachher: {}",
                row(&before, y),
                row(&after, y),
            );
        }
        for x in 4..8 {
            assert_eq!(
                pixel(&after, x, y),
                [0, 0, 0, 255],
                "geschwärzter Bildpunkt ({x},{y}) ist nicht schwarz und sichtbar: {}",
                row(&after, y),
            );
        }
    }

    // Und zwar dadurch, dass die Maske selbst mitgeschrieben wird — nicht durch
    // eine nachgebaute Alphaebene: der Stencil-Strom steht in voller Auflösung
    // neben dem Bild, eine Alphaebene wäre auf 8×8 heruntergebrochen.
    let dict = image_dict(&out, b"Im0");
    let mask = dict.get(b"Mask").expect("/Mask fehlt in der Ausgabe");
    let (_, resolved) = out.dereference(mask).expect("auflösbar");
    let mask_stream = resolved.as_stream().expect("Strom");
    assert_eq!(
        mask_stream
            .dict
            .get(b"ImageMask")
            .and_then(Object::as_bool)
            .ok(),
        Some(true)
    );
    assert!(
        dict.get(b"SMask").is_err(),
        "/Mask und /SMask nebeneinander sind regelwidrig: {dict:?}"
    );
}

/// Auch das Umgekehrte muss stimmen: die Bildpunkte sind wirklich fort.
#[test]
fn the_redacted_samples_are_gone_from_the_file_even_under_a_mask() {
    let mut doc = image_with_stencil_mask();
    let (_, out) = roundtrip(&mut doc, Rect::new(101.0, 101.0, 139.0, 179.0));
    let after = first_image(&out);
    // Geschwärzt wurde diesmal die **versteckte** Hälfte. Ihre Farbwerte müssen
    // trotzdem weg sein — sonst genügte ein Entfernen der Maske, um sie zu
    // sehen.
    for y in 0..8 {
        for x in 0..4 {
            let p = pixel(&after, x, y);
            assert_eq!(
                [p[0], p[1], p[2]],
                [0, 0, 0],
                "versteckter Bildpunkt ({x},{y}) trägt noch seine Farbe: {}",
                row(&after, y)
            );
        }
    }
}

/// Der Grund, aus dem `/Mask` mitgeschrieben und nicht umgerechnet wird: eine
/// Maske, die wir **gar nicht lesen können**, kommt trotzdem unbeschadet
/// durch.
///
/// Ein Stencil neben einem eingescannten Auszug ist typischerweise
/// `CCITTFaxDecode` — genau das, was hier nicht aufgeht. Würde die Maske in
/// eine Alphaebene umgerechnet, bliebe diese mangels Daten überall deckend, das
/// `/SMask` entfiele „zu Recht“ und die verdeckten Bildpunkte stünden sichtbar
/// in der Ausgabe.
#[test]
fn a_mask_we_cannot_decode_is_still_carried_over() {
    let mut doc = image_with_stencil_mask();
    let mask_id = match image_dict(&doc, b"Im0").get(b"Mask") {
        Ok(Object::Reference(id)) => *id,
        other => panic!("kein Maskenverweis: {other:?}"),
    };
    if let Ok(Object::Stream(stream)) = doc.get_object_mut(mask_id) {
        stream
            .dict
            .set("Filter", Object::Name(b"CCITTFaxDecode".to_vec()));
    }

    let (report, out) = roundtrip(&mut doc, right_half());
    assert_eq!(report.redacted_images, 1, "das Bild selbst ist lesbar");
    let dict = image_dict(&out, b"Im0");
    let mask = dict.get(b"Mask").expect("/Mask fehlt in der Ausgabe");
    let (_, resolved) = out.dereference(mask).expect("auflösbar");
    assert_eq!(
        resolved
            .as_stream()
            .expect("Strom")
            .dict
            .get(b"Filter")
            .and_then(Object::as_name)
            .ok(),
        Some(&b"CCITTFaxDecode"[..]),
        "die Maske wurde angefasst, statt unverändert mitzukommen"
    );
    assert!(dict.get(b"SMask").is_err(), "{dict:?}");
}

// ---------------------------------------------------------------------------
// /Mask als Farbschlüssel-Array
// ---------------------------------------------------------------------------

fn image_with_colour_key(mask: Vec<i64>, data: Vec<u8>) -> Document {
    build(
        vec![(
            "Im0",
            image(
                8,
                8,
                dictionary! {
                    "ColorSpace" => "DeviceRGB",
                    "BitsPerComponent" => 8_i64,
                    "Mask" => mask.into_iter().map(Object::Integer).collect::<Vec<_>>(),
                },
                data,
            ),
        )],
        PLACE,
    )
}

/// Ein Farbschlüssel benennt **Abtastwerte**. Gelesen werden muss er, solange
/// es sie gibt.
#[test]
fn a_colour_key_hides_the_samples_it_names() {
    let doc = image_with_colour_key(vec![200, 200, 10, 10, 10, 10], red_left_blue_right());
    let before = first_image(&doc);
    for y in 0..8 {
        for x in 0..4 {
            assert_eq!(pixel(&before, x, y)[3], 0, "Zeile {y}: {}", row(&before, y));
        }
        for x in 4..8 {
            assert_eq!(
                pixel(&before, x, y)[3],
                255,
                "Zeile {y}: {}",
                row(&before, y)
            );
        }
    }
}

/// Der Farbschlüssel wird **nicht** übernommen, sondern als Alphaebene
/// zurückgeschrieben — sonst träfe er in der Ausgabe andere Bildpunkte.
#[test]
fn a_colour_key_becomes_an_alpha_channel() {
    let mut doc = image_with_colour_key(vec![200, 200, 10, 10, 10, 10], red_left_blue_right());
    let before = first_image(&doc);
    let (report, out) = roundtrip(&mut doc, right_half());
    assert_eq!(report.redacted_images, 1);

    let after = first_image(&out);
    for y in 0..8 {
        for x in 0..4 {
            assert_eq!(
                pixel(&after, x, y),
                pixel(&before, x, y),
                "versteckter Bildpunkt ({x},{y}) hat sich verändert\nvorher : {}\nnachher: {}",
                row(&before, y),
                row(&after, y),
            );
        }
        for x in 4..8 {
            assert_eq!(
                pixel(&after, x, y),
                [0, 0, 0, 255],
                "geschwärzter Bildpunkt ({x},{y}): {}",
                row(&after, y)
            );
        }
    }

    let dict = image_dict(&out, b"Im0");
    assert!(
        dict.get(b"Mask").is_err(),
        "der Farbschlüssel der Eingabe steht noch in der Ausgabe: {dict:?}"
    );
    assert!(
        dict.get(b"SMask").is_ok(),
        "die Maske ist ersatzlos entfallen: {dict:?}"
    );
}

/// Der teure Fall: der Schlüssel benennt genau die Farbe, mit der geschwärzt
/// wird. Unverändert übernommen machte er die frisch geschwärzten Bildpunkte
/// wieder durchsichtig — die Schwärzung wäre ein Loch im Bild.
#[test]
fn a_colour_key_that_names_black_does_not_dissolve_the_redaction() {
    let mut data = Vec::new();
    for _ in 0..8 {
        for x in 0..8 {
            // Links schwarz (und damit vom Schlüssel versteckt), rechts weiß.
            data.extend_from_slice(if x < 4 { &[0, 0, 0] } else { &[255, 255, 255] });
        }
    }
    let mut doc = image_with_colour_key(vec![0, 0, 0, 0, 0, 0], data);
    let (_, out) = roundtrip(&mut doc, right_half());
    let after = first_image(&out);
    for y in 0..8 {
        for x in 4..8 {
            assert_eq!(
                pixel(&after, x, y),
                [0, 0, 0, 255],
                "der geschwärzte Bildpunkt ({x},{y}) ist durchsichtig geworden: {}",
                row(&after, y)
            );
        }
        for x in 0..4 {
            assert_eq!(
                pixel(&after, x, y)[3],
                0,
                "der versteckte Bildpunkt ({x},{y}) ist sichtbar geworden: {}",
                row(&after, y)
            );
        }
    }
}

/// Ein Farbschlüssel auf einem verlustbehaftet kodierten Bild lässt sich nicht
/// sicher lesen — dann wird abgebrochen statt genähert.
#[test]
fn a_colour_key_on_a_lossy_image_stops_the_run() {
    let mut doc = build(
        vec![(
            "Im0",
            image(
                8,
                8,
                dictionary! {
                    "ColorSpace" => "DeviceRGB",
                    "BitsPerComponent" => 8_i64,
                    "Filter" => "DCTDecode",
                    "Mask" => vec![
                        Object::Integer(200), Object::Integer(200),
                        Object::Integer(10), Object::Integer(10),
                        Object::Integer(10), Object::Integer(10),
                    ],
                },
                b"kein echtes JPEG".to_vec(),
            ),
        )],
        PLACE,
    );
    let message = redaction_error(&mut doc, right_half());
    assert!(
        message.contains("Farbschlüssel") && message.contains("DCTDecode"),
        "die Meldung nennt den Grund nicht: {message}"
    );
}

/// Ein Schlüssel, dessen Länge nicht zum Farbraum passt: unklar, was er meint.
#[test]
fn a_malformed_colour_key_stops_the_run() {
    // Zwei Einträge, aber DeviceRGB hat drei Komponenten.
    let mut doc = image_with_colour_key(vec![200, 200], red_left_blue_right());
    let message = redaction_error(&mut doc, right_half());
    assert!(
        message.contains("Farbschlüssel") && message.contains("Komponente"),
        "die Meldung nennt den Grund nicht: {message}"
    );
}

// ---------------------------------------------------------------------------
// /SMask
// ---------------------------------------------------------------------------

fn image_with_smask(alpha: Vec<u8>, extra_mask: Dictionary) -> Document {
    let mut doc = build(
        vec![(
            "Im0",
            image(
                8,
                8,
                dictionary! {
                    "ColorSpace" => "DeviceRGB",
                    "BitsPerComponent" => 8_i64,
                },
                red_left_blue_right(),
            ),
        )],
        PLACE,
    );
    let mut mask_dict = dictionary! {
        "ColorSpace" => "DeviceGray",
        "BitsPerComponent" => 8_i64,
    };
    for (key, value) in extra_mask.iter() {
        mask_dict.set(key.clone(), value.clone());
    }
    let mask_id = doc.add_object(Object::Stream(image(8, 8, mask_dict, alpha)));
    let id = image_id(&doc, b"Im0");
    if let Ok(Object::Stream(stream)) = doc.get_object_mut(id) {
        stream.dict.set("SMask", Object::Reference(mask_id));
    }
    doc
}

/// Teiltransparenz überlebt als neues `/SMask`-Objekt — der Fall, der heute
/// stimmt und weiter stimmen muss.
#[test]
fn a_soft_mask_with_real_transparency_is_written_back() {
    let mut alpha = vec![255u8; 64];
    for y in 0..8 {
        alpha[y * 8] = 0; // Spalte 0 unsichtbar
        alpha[y * 8 + 1] = 128; // Spalte 1 halbdurchsichtig
    }
    let mut doc = image_with_smask(alpha, Dictionary::new());
    let before = first_image(&doc);
    assert_eq!(pixel(&before, 1, 0)[3], 128);

    let (_, out) = roundtrip(&mut doc, right_half());
    let after = first_image(&out);
    for y in 0..8 {
        assert_eq!(pixel(&after, 0, y)[3], 0, "Zeile {y}: {}", row(&after, y));
        assert_eq!(pixel(&after, 1, y)[3], 128, "Zeile {y}: {}", row(&after, y));
    }
    assert!(image_dict(&out, b"Im0").get(b"SMask").is_ok());
}

/// Ist nach der Schwärzung alles deckend, entfällt die Alphaebene zu Recht.
#[test]
fn a_soft_mask_that_is_fully_opaque_afterwards_is_dropped() {
    let mut alpha = vec![255u8; 64];
    for y in 0..8 {
        for x in 4..8 {
            alpha[y * 8 + x] = 0; // genau die Hälfte, die geschwärzt wird
        }
    }
    let mut doc = image_with_smask(alpha, Dictionary::new());
    let (_, out) = roundtrip(&mut doc, right_half());
    let after = first_image(&out);
    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(pixel(&after, x, y)[3], 255, "Zeile {y}: {}", row(&after, y));
        }
    }
    assert!(
        image_dict(&out, b"Im0").get(b"SMask").is_err(),
        "die Alphaebene ist überflüssig und dürfte nicht mehr dastehen"
    );
}

/// Eine Alphaebene, die wir nicht lesen können, dürfen wir nicht neu bauen —
/// sie entfiele stillschweigend und die verdeckten Bildpunkte würden sichtbar.
#[test]
fn an_unreadable_soft_mask_stops_the_run() {
    let mut doc = image_with_smask(vec![0; 64], dictionary! { "Filter" => "JPXDecode" });
    let message = redaction_error(&mut doc, right_half());
    assert!(
        message.contains("/SMask"),
        "die Meldung nennt die Alphaebene nicht: {message}"
    );
}

/// Zwei Bilder, die sich gegenseitig als Alphaebene nennen, dürfen den Prozess
/// nicht in eine endlose Rekursion schicken.
#[test]
fn two_images_that_mask_each_other_terminate() {
    let mut doc = build(
        vec![(
            "Im0",
            image(
                8,
                8,
                dictionary! { "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8_i64 },
                vec![128; 64],
            ),
        )],
        PLACE,
    );
    let first = image_id(&doc, b"Im0");
    let second = doc.add_object(Object::Stream(image(
        8,
        8,
        dictionary! {
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 8_i64,
            "SMask" => Object::Reference(first),
        },
        vec![64; 64],
    )));
    if let Ok(Object::Stream(stream)) = doc.get_object_mut(first) {
        stream.dict.set("SMask", Object::Reference(second));
    }
    // Ohne Riegel endet das im Stapelüberlauf, und zwar schon beim Anzeigen.
    let images = images_of(&doc);
    assert_eq!(images.len(), 1);
}

// ---------------------------------------------------------------------------
// Was heute stimmt und weiter stimmen muss
// ---------------------------------------------------------------------------

/// Vergleicht Bildpunkt für Bildpunkt: innerhalb der Spalten `black` schwarz,
/// außerhalb unverändert. Die Randspalten bleiben ausgespart — dort entscheidet
/// die Geometrie, und diese Tests messen die Farbtreue, nicht die Kante.
#[track_caller]
fn assert_columns_blacked(
    before: &RasterImage,
    after: &RasterImage,
    black: (u32, u32),
    what: &str,
) {
    assert_eq!((before.width, before.height), (after.width, after.height));
    for y in 0..after.height {
        for x in 0..after.width {
            if (black.0..=black.1).contains(&x) {
                assert_eq!(
                    pixel(after, x, y),
                    [0, 0, 0, 255],
                    "{what}: ({x},{y}) nicht geschwärzt\n{}",
                    row(after, y)
                );
            } else if x + 1 < black.0 || x > black.1 + 1 {
                assert_eq!(
                    pixel(after, x, y),
                    pixel(before, x, y),
                    "{what}: ({x},{y}) außerhalb verändert\nvorher : {}\nnachher: {}",
                    row(before, y),
                    row(after, y)
                );
            }
        }
    }
}

/// Gespiegelte CTM (negatives x): die Schwärzung muss die *gespiegelten*
/// Spalten treffen.
#[test]
fn a_mirrored_image_hits_the_mirrored_columns() {
    let mut doc = build(
        vec![(
            "Im0",
            image(
                8,
                8,
                dictionary! { "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8_i64 },
                red_left_blue_right(),
            ),
        )],
        "q -80 0 0 80 180 100 cm /Im0 Do Q\n",
    );
    let before = first_image(&doc);
    let (report, out) = roundtrip(&mut doc, right_half());
    assert_eq!(report.redacted_images, 1);
    // Gespiegelt liegt Spalte 0 rechts: die rechte Seitenhälfte trifft 0..3.
    assert_columns_blacked(&before, &first_image(&out), (0, 3), "gespiegelt");
}

/// `/Decode [1 0]` auf Graustufen: beim Lesen angewandt, beim Schreiben
/// fallengelassen — der Bildpunkt behält seinen sichtbaren Wert.
#[test]
fn an_inverted_decode_array_is_read_and_dropped_correctly() {
    let data: Vec<u8> = (0..64u8).map(|v| v * 4).collect();
    let mut doc = build(
        vec![(
            "Im0",
            image(
                8,
                8,
                dictionary! {
                    "ColorSpace" => "DeviceGray",
                    "BitsPerComponent" => 8_i64,
                    "Decode" => vec![Object::Integer(1), Object::Integer(0)],
                },
                data,
            ),
        )],
        PLACE,
    );
    let before = first_image(&doc);
    assert_eq!(pixel(&before, 0, 0), [255, 255, 255, 255], "invertiert?");
    let (_, out) = roundtrip(&mut doc, right_half());
    let after = first_image(&out);
    assert_columns_blacked(&before, &after, (4, 7), "/Decode [1 0]");
    let dict = image_dict(&out, b"Im0");
    assert!(
        dict.get(b"Decode").is_err(),
        "das Decode-Array gilt für die alten Abtastwerte: {dict:?}"
    );
}

/// 1 Bit je Bildpunkt, Graustufen.
#[test]
fn a_one_bit_image_keeps_its_untouched_columns() {
    // Zeilenweise 0b1010_1010: abwechselnd schwarz und weiß.
    let mut doc = build(
        vec![(
            "Im0",
            image(
                8,
                8,
                dictionary! { "ColorSpace" => "DeviceGray", "BitsPerComponent" => 1_i64 },
                vec![0b1010_1010; 8],
            ),
        )],
        PLACE,
    );
    let before = first_image(&doc);
    assert_eq!(pixel(&before, 0, 0), [255, 255, 255, 255]);
    assert_eq!(pixel(&before, 1, 0), [0, 0, 0, 255]);
    let (_, out) = roundtrip(&mut doc, right_half());
    assert_columns_blacked(&before, &first_image(&out), (4, 7), "1 Bit");
}

/// `Indexed` mit Palette: die ungeschwärzten Spalten behalten ihre Farbe.
#[test]
fn an_indexed_image_keeps_its_untouched_columns() {
    let palette = vec![200, 10, 10, 10, 10, 200, 0, 200, 0, 240, 240, 0];
    let mut indices = Vec::new();
    for _ in 0..8 {
        for x in 0..8u8 {
            indices.push(x % 4);
        }
    }
    let mut doc = build(
        vec![(
            "Im0",
            image(
                8,
                8,
                dictionary! {
                    "ColorSpace" => vec![
                        Object::Name(b"Indexed".to_vec()),
                        Object::Name(b"DeviceRGB".to_vec()),
                        Object::Integer(3),
                        Object::String(palette, StringFormat::Hexadecimal),
                    ],
                    "BitsPerComponent" => 8_i64,
                },
                indices,
            ),
        )],
        PLACE,
    );
    let before = first_image(&doc);
    assert_eq!(pixel(&before, 0, 0), [200, 10, 10, 255], "Palette gelesen?");
    let (_, out) = roundtrip(&mut doc, right_half());
    assert_columns_blacked(&before, &first_image(&out), (4, 7), "Indexed");
}

/// `DeviceCMYK`.
#[test]
fn a_cmyk_image_keeps_its_untouched_columns() {
    let mut data = Vec::new();
    for _ in 0..8 {
        for x in 0..8u8 {
            data.extend_from_slice(&[x * 30, 255 - x * 30, 40, 10]);
        }
    }
    let mut doc = build(
        vec![(
            "Im0",
            image(
                8,
                8,
                dictionary! { "ColorSpace" => "DeviceCMYK", "BitsPerComponent" => 8_i64 },
                data,
            ),
        )],
        PLACE,
    );
    let before = first_image(&doc);
    let (_, out) = roundtrip(&mut doc, right_half());
    assert_columns_blacked(&before, &first_image(&out), (4, 7), "DeviceCMYK");
}

/// Eine Stencil-Maske, die überall malt: nur die geschwärzte Hälfte hört auf zu
/// malen — der Prüfstein für die Polarität, denn hier ist *das Bild selbst* die
/// Maske und `/Mask` gar nicht im Spiel.
#[test]
fn a_stencil_image_stops_painting_only_where_it_was_redacted() {
    let mut doc = build(
        vec![(
            "Im0",
            image(
                8,
                8,
                dictionary! { "ImageMask" => true, "BitsPerComponent" => 1_i64 },
                vec![0x00; 8],
            ),
        )],
        "q 1 0 0 rg 80 0 0 80 100 100 cm /Im0 Do Q\n",
    );
    let before = first_image(&doc);
    for x in 0..8 {
        assert_eq!(pixel(&before, x, 0)[3], 255, "Vorbedingung: malt überall");
    }
    let (_, out) = roundtrip(&mut doc, right_half());
    let after = first_image(&out);
    for y in 0..8 {
        for x in 0..4 {
            assert_eq!(pixel(&after, x, y)[3], 255, "Zeile {y}: {}", row(&after, y));
        }
        for x in 4..8 {
            assert_eq!(pixel(&after, x, y)[3], 0, "Zeile {y}: {}", row(&after, y));
        }
    }
}
