//! Bilder werden **wirklich** geschwärzt — die Pixel sind weg, nicht übermalt.
//!
//! Gemessen wird nicht am Zwischenstand, sondern am Ergebnis: das geschriebene
//! PDF wird neu geladen, das Bild erneut dekodiert und Pixel für Pixel
//! verglichen. Innerhalb der Schwärzung muss es schwarz sein, außerhalb
//! byteweise unverändert.
//!
//! Die Bildmuster sind so gewählt, dass jedes Pixel einen eigenen Wert trägt —
//! eine verrutschte Zuordnung fällt damit sofort auf.

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{
    leaks, load_from_bytes, page_ops, save_to_bytes, DrawOp, PdfRedactor, RasterImage,
    RedactionReport,
};

/// Das Geheimnis, das die Pixel eines Tests buchstäblich enthalten.
const SECRET: &str = "DE89 3704 0044 0532 0130 00";

// ---------------------------------------------------------------------------
// Gerüst
// ---------------------------------------------------------------------------

/// Baut ein Dokument mit gemeinsamen XObject-Ressourcen und je einem
/// Content-Stream pro Seite.
fn build(xobjects: Vec<(&str, Stream)>, pages: &[&str]) -> Document {
    let mut doc = Document::with_version("1.5");
    let mut names = Dictionary::new();
    for (name, stream) in xobjects {
        let id = doc.add_object(Object::Stream(stream));
        names.set(name.as_bytes().to_vec(), Object::Reference(id));
    }
    let resources_id = doc.add_object(dictionary! { "XObject" => names });

    let pages_id = doc.new_object_id();
    let mut page_ids = Vec::new();
    for content in pages {
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
        page_ids.push(doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        }));
    }
    let count = page_ids.len() as i64;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => page_ids.iter().copied().map(Object::Reference).collect::<Vec<_>>(),
            "Count" => count,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    doc
}

/// Ein RGB-Bild, dessen Pixel sich alle unterscheiden.
fn rgb_image(width: u32, height: u32) -> Stream {
    let mut data = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            data.extend_from_slice(&[(x * 11 + 3) as u8, (y * 7 + 5) as u8, 0x40]);
        }
    }
    image_stream(width, height, b"DeviceRGB", 8, data)
}

fn image_stream(width: u32, height: u32, space: &[u8], bpc: i64, data: Vec<u8>) -> Stream {
    let mut stream = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => i64::from(width),
            "Height" => i64::from(height),
            "ColorSpace" => Object::Name(space.to_vec()),
            "BitsPerComponent" => bpc,
        },
        data,
    );
    let _ = stream.compress();
    stream
}

fn blackout(page: usize, rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            page,
            rect,
            None,
            Source::Manual {
                reason: "Bildtest".into(),
            },
        ),
        Action::Blackout,
    )
}

/// Schwärzt ohne zusätzlichen Rand — sonst verschiebt `padding` die
/// Pixelgrenzen, an denen dieser Test misst.
fn redact(doc: &mut Document, redactions: &[Redaction]) -> RedactionReport {
    PdfRedactor::with_padding(0.0)
        .apply_with_report(doc, redactions)
        .expect("Schwärzung")
}

/// Schwärzt, speichert und lädt neu — gemessen wird immer an der Ausgabedatei.
fn roundtrip(doc: &mut Document, redactions: &[Redaction]) -> (RedactionReport, Document, Vec<u8>) {
    let report = redact(doc, redactions);
    let bytes = save_to_bytes(doc).expect("Speichern");
    let out = load_from_bytes(&bytes).expect("Laden");
    (report, out, bytes)
}

/// Alle auf einer Seite gezeichneten Bilder, in Zeichenreihenfolge dekodiert.
fn images_of(doc: &Document, page: usize) -> Vec<RasterImage> {
    let ops = page_ops(doc, page).expect("Seitenoperationen");
    ops.ops
        .iter()
        .filter_map(|op| match op {
            DrawOp::Image { image, .. } => Some(ops.images[*image].clone()),
            _ => None,
        })
        .collect()
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

/// Kern jeder Messung.
///
/// `black` gibt den Bereich an, der sicher *innerhalb* der Schwärzung liegt;
/// `keep` den Bereich, der sicher außerhalb liegt. Zwischen beiden bleibt
/// bewusst ein Pixel Luft: an der Kante entscheidet die Geometrie, und darüber
/// soll dieser Test keine Aussage erzwingen.
#[track_caller]
fn assert_blacked(
    before: &RasterImage,
    after: &RasterImage,
    black: (u32, u32, u32, u32),
    what: &str,
) {
    assert_eq!(before.width, after.width, "{what}: Breite geändert");
    assert_eq!(before.height, after.height, "{what}: Höhe geändert");
    let (x0, x1, y0, y1) = black;
    for y in 0..after.height {
        for x in 0..after.width {
            let inside = (x0..=x1).contains(&x) && (y0..=y1).contains(&y);
            let outside = x + 1 < x0 || x > x1 + 1 || y + 1 < y0 || y > y1 + 1;
            if inside {
                assert_eq!(
                    pixel(after, x, y),
                    [0, 0, 0, 255],
                    "{what}: Pixel ({x},{y}) im Schwärzungsbereich ist nicht schwarz"
                );
            } else if outside {
                assert_eq!(
                    pixel(after, x, y),
                    pixel(before, x, y),
                    "{what}: Pixel ({x},{y}) außerhalb wurde verändert"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Achsenparallele, gedrehte und skalierte Bild-CTM
// ---------------------------------------------------------------------------

/// Das Bild füllt (50,600)-(150,700); jedes der 20×20 Pixel ist 5 Punkt groß.
/// Geschwärzt wird (75,650)-(100,675) — Spalten 5..9, Zeilen 5..9.
#[test]
fn an_upright_image_loses_exactly_the_covered_pixels() {
    let mut doc = build(
        vec![("Im0", rgb_image(20, 20))],
        &["q 100 0 0 100 50 600 cm /Im0 Do Q\n"],
    );
    let before = images_of(&doc, 0).remove(0);
    assert!(!before.placeholder, "Vorbedingung: Bild ist dekodierbar");

    let (report, out, _) = roundtrip(
        &mut doc,
        &[blackout(0, Rect::new(75.0, 650.0, 100.0, 675.0))],
    );
    assert_eq!(report.redacted_images, 1);
    assert_eq!(
        report.copied_images, 0,
        "einmalig benutzt — keine Kopie nötig"
    );

    let after = images_of(&out, 0).remove(0);
    assert_blacked(&before, &after, (5, 9, 5, 9), "aufrechtes Bild");
}

/// `/Width`, `/Height` und `/BitsPerComponent` als **indirekte** Verweise.
///
/// In freier Wildbahn üblich, für uns lange folgenlos: das Bild dekodierte zum
/// Platzhalter und wurde nur übermalt. Seit der Bildschwärzung ist derselbe
/// Platzhalter ein Abbruch — aus einem stillen Fehler ist ein lauter geworden.
/// Ursache war das Auslesen ohne `Document::dereference`.
#[test]
fn an_image_with_indirect_size_entries_is_decoded_and_redacted() {
    let mut doc = build(Vec::new(), &["q 100 0 0 100 50 600 cm /Im0 Do Q\n"]);
    let width_id = doc.add_object(Object::Integer(20));
    let height_id = doc.add_object(Object::Integer(20));
    let bpc_id = doc.add_object(Object::Integer(8));
    let mut stream = rgb_image(20, 20);
    stream.dict.set("Width", Object::Reference(width_id));
    stream.dict.set("Height", Object::Reference(height_id));
    stream
        .dict
        .set("BitsPerComponent", Object::Reference(bpc_id));
    let image_id = doc.add_object(Object::Stream(stream));
    add_xobject(&mut doc, "Im0", image_id);

    let before = images_of(&doc, 0).remove(0);
    assert!(
        !before.placeholder,
        "indirektes /Width ließ das Bild zum Platzhalter werden"
    );
    assert_eq!((before.width, before.height), (20, 20));

    // Und die Schwärzung läuft durch, statt am Platzhalter abzubrechen.
    let (report, out, _) = roundtrip(
        &mut doc,
        &[blackout(0, Rect::new(75.0, 650.0, 100.0, 675.0))],
    );
    assert_eq!(report.redacted_images, 1);
    let after = images_of(&out, 0).remove(0);
    assert_blacked(&before, &after, (5, 9, 5, 9), "indirekte Größenangaben");
}

/// Dieselbe Schwärzung, aber das Bild ist um 90° gedreht platziert:
/// `0 100 -100 0 150 600 cm` bildet u auf y und v auf x ab. Damit liegt der
/// geschwärzte Bereich bei Spalten 10..14 statt 5..9 — wer die inverse CTM
/// nicht rechnet, trifft die falschen Pixel.
#[test]
fn a_rotated_image_is_blacked_where_the_inverse_ctm_points() {
    let mut doc = build(
        vec![("Im0", rgb_image(20, 20))],
        &["q 0 100 -100 0 150 600 cm /Im0 Do Q\n"],
    );
    let before = images_of(&doc, 0).remove(0);

    let (_, out, _) = roundtrip(
        &mut doc,
        &[blackout(0, Rect::new(75.0, 650.0, 100.0, 675.0))],
    );
    let after = images_of(&out, 0).remove(0);
    assert_blacked(&before, &after, (10, 14, 5, 9), "gedrehtes Bild");

    // Gegenprobe zum aufrechten Fall: dort wären die Spalten 5..9 schwarz.
    assert_eq!(
        pixel(&after, 7, 7),
        pixel(&before, 7, 7),
        "die Drehung wurde nicht berücksichtigt"
    );
}

/// Ungleich skaliert und verschoben: 200×50 Punkt für 40×10 Pixel.
#[test]
fn a_scaled_image_maps_the_area_per_axis() {
    let mut doc = build(
        vec![("Im0", rgb_image(40, 10))],
        &["q 200 0 0 50 100 400 cm /Im0 Do Q\n"],
    );
    let before = images_of(&doc, 0).remove(0);

    // x 150..200  ->  u 0.25..0.5   -> Spalten 10..19
    // y 425..440  ->  v 0.5..0.8    -> Zeilen  2..4
    let (_, out, _) = roundtrip(
        &mut doc,
        &[blackout(0, Rect::new(150.0, 425.0, 200.0, 440.0))],
    );
    let after = images_of(&out, 0).remove(0);
    assert_blacked(&before, &after, (10, 19, 2, 4), "skaliertes Bild");
}

// ---------------------------------------------------------------------------
// Mehrfach benutzte Bilder
// ---------------------------------------------------------------------------

/// Dasselbe XObject auf zwei Seiten, geschwärzt wird nur Seite 1. Seite 2 muss
/// das Bild unverändert behalten — deshalb wird kopiert statt überschrieben.
#[test]
fn an_image_used_on_two_pages_is_copied_not_overwritten() {
    let content = "q 100 0 0 100 50 600 cm /Im0 Do Q\n";
    let mut doc = build(vec![("Im0", rgb_image(20, 20))], &[content, content]);
    let before = images_of(&doc, 0).remove(0);

    let (report, out, _) = roundtrip(
        &mut doc,
        &[blackout(0, Rect::new(75.0, 650.0, 100.0, 675.0))],
    );
    assert_eq!(
        report.copied_images, 1,
        "das Bild hätte kopiert werden müssen"
    );

    let page1 = images_of(&out, 0).remove(0);
    assert_blacked(&before, &page1, (5, 9, 5, 9), "Seite 1");

    let page2 = images_of(&out, 1).remove(0);
    assert_eq!(
        page2.rgba, before.rgba,
        "das unbeteiligte Vorkommen auf Seite 2 wurde mitgeschwärzt"
    );
}

/// Zweimal dieselbe Platzierung auf *einer* Seite: beide Bereiche wirken auf
/// dasselbe Bild, es bleibt bei einem Objekt.
#[test]
fn two_placements_on_one_page_share_the_union() {
    let mut doc = build(
        vec![("Im0", rgb_image(20, 20))],
        &["q 100 0 0 100 50 600 cm /Im0 Do Q\nq 100 0 0 100 50 200 cm /Im0 Do Q\n"],
    );
    let before = images_of(&doc, 0).remove(0);

    // Erste Platzierung: Zeilen/Spalten 5..9. Zweite Platzierung (y 200..300):
    // y 250..275 trifft dieselben Zeilen, x 125..150 die Spalten 15..19.
    let (report, out, _) = roundtrip(
        &mut doc,
        &[
            blackout(0, Rect::new(75.0, 650.0, 100.0, 675.0)),
            blackout(0, Rect::new(125.0, 250.0, 150.0, 275.0)),
        ],
    );
    assert_eq!(report.copied_images, 0);
    let after = images_of(&out, 0).remove(0);
    assert_eq!(pixel(&after, 7, 7), [0, 0, 0, 255], "erste Platzierung");
    assert_eq!(pixel(&after, 17, 7), [0, 0, 0, 255], "zweite Platzierung");
    assert_eq!(
        pixel(&after, 2, 2),
        pixel(&before, 2, 2),
        "unbeteiligte Pixel"
    );
}

// ---------------------------------------------------------------------------
// Nicht dekodierbare Bilder
// ---------------------------------------------------------------------------

fn undecodable(filter: &str) -> Stream {
    Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 20_i64,
            "Height" => 20_i64,
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 8,
            "Filter" => Object::Name(filter.as_bytes().to_vec()),
        },
        vec![0x99; 64],
    )
}

#[test]
fn an_undecodable_image_stops_the_redaction() {
    for filter in ["JPXDecode", "CCITTFaxDecode"] {
        let mut doc = build(
            vec![("Im0", undecodable(filter))],
            &["q 100 0 0 100 50 600 cm /Im0 Do Q\n"],
        );
        let error = PdfRedactor::with_padding(0.0)
            .apply_with_report(
                &mut doc,
                &[blackout(0, Rect::new(75.0, 650.0, 100.0, 675.0))],
            )
            .expect_err("hätte fehlschlagen müssen");
        let text = error.to_string();
        assert!(
            text.contains(filter) && text.contains("nicht dekodieren"),
            "unklare Meldung für {filter}: {text}"
        );
    }
}

#[test]
fn an_undecodable_image_outside_every_redaction_is_no_problem() {
    // Das Bild liegt bei (50,600)-(150,700), geschwärzt wird ganz woanders.
    let mut doc = build(
        vec![("Im0", undecodable("JPXDecode"))],
        &["q 100 0 0 100 50 600 cm /Im0 Do Q\n"],
    );
    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(
            &mut doc,
            &[blackout(0, Rect::new(300.0, 100.0, 400.0, 200.0))],
        )
        .expect("darf nicht fehlschlagen");
    assert_eq!(report.redacted_images, 0);
}

#[test]
fn the_library_can_be_told_to_go_ahead_anyway() {
    let mut doc = build(
        vec![("Im0", undecodable("JPXDecode"))],
        &["q 100 0 0 100 50 600 cm /Im0 Do Q\n"],
    );
    let report = PdfRedactor::with_padding(0.0)
        .allowing_undecodable_images(true)
        .apply_with_report(
            &mut doc,
            &[blackout(0, Rect::new(75.0, 650.0, 100.0, 675.0))],
        )
        .expect("mit Erlaubnis darf es weitergehen");
    assert_eq!(report.redacted_images, 0);
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("JPXDecode") && w.contains("blieben in der Datei")),
        "ohne deutliche Warnung ist die Erlaubnis eine Falle: {:?}",
        report.warnings
    );
}

// ---------------------------------------------------------------------------
// Speicherbedarf (Aufgabe #58) und der wahre Grund (Aufgabe #60)
// ---------------------------------------------------------------------------

/// Ein 1-Bit-Graustufenbild mit `/FlateDecode` — die gewöhnliche Kodierung
/// eines Schwarzweiß-Scans. Zwischen Rohbytes und dekodiertem RGBA8 liegt der
/// Faktor 32; genau daran hing der Speicherbedarf.
fn bilevel_image(width: u32, height: u32) -> Stream {
    let stride = (width as usize).div_ceil(8);
    image_stream(
        width,
        height,
        b"DeviceGray",
        1,
        vec![0u8; stride * height as usize],
    )
}

/// Ein Bild, das nur *behauptet*, groß zu sein — die Nutzdaten sind ein
/// winziger Strom. Es wird gar nicht erst ausgepackt, deshalb genügt das.
fn oversized_flate_image(width: u32, height: u32) -> Stream {
    let mut stream = image_stream(1, 1, b"DeviceGray", 1, vec![0u8; 1]);
    stream.dict.set("Width", i64::from(width));
    stream.dict.set("Height", i64::from(height));
    stream
}

/// **Aufgabe #60.** Ein Bild über `MAX_IMAGE_PIXELS` lässt sich nicht öffnen —
/// aber nicht *wegen des Filters*. Die Meldung nannte trotzdem nur den Filter
/// und schickte damit jeden, der ihr folgt, in die falsche Richtung.
#[test]
fn the_message_names_the_real_reason_not_the_filter() {
    let mut doc = build(
        vec![("Im0", oversized_flate_image(8000, 8000))],
        &["q 100 0 0 100 50 600 cm /Im0 Do Q\n"],
    );
    let error = PdfRedactor::with_padding(0.0)
        .apply_with_report(
            &mut doc,
            &[blackout(0, Rect::new(75.0, 650.0, 100.0, 675.0))],
        )
        .expect_err("hätte fehlschlagen müssen");
    let text = error.to_string();
    assert!(
        text.contains("zu groß") && text.contains("8000x8000"),
        "der wahre Grund fehlt: {text}"
    );
    assert!(
        !text.contains("Filter: FlateDecode"),
        "der Filter ist nicht das Problem und darf nicht als solches dastehen: {text}"
    );
}

/// **Aufgabe #58 (a) und (c).** Der Speicherbedarf selbst ist im Test kaum zu
/// messen — die Zahl, an der er hing, schon: wie viele Bilder gleichzeitig
/// dekodiert gehalten werden. Vorher waren das alle Bilder aller berührten
/// Seiten (deshalb 5,5 GB bei 20 Bildern in einer 92-kB-Datei), heute genau
/// eines.
#[test]
fn only_one_image_is_held_decoded_at_a_time() {
    // Fünf Bilder, alle deckungsgleich unter derselben Schwärzung.
    let images: Vec<(&str, Stream)> = vec![
        ("Im0", rgb_image(40, 40)),
        ("Im1", rgb_image(40, 40)),
        ("Im2", rgb_image(40, 40)),
        ("Im3", rgb_image(40, 40)),
        ("Im4", rgb_image(40, 40)),
    ];
    let content = "q 100 0 0 100 50 600 cm /Im0 Do Q\n\
                   q 100 0 0 100 50 600 cm /Im1 Do Q\n\
                   q 100 0 0 100 50 600 cm /Im2 Do Q\n\
                   q 100 0 0 100 50 600 cm /Im3 Do Q\n\
                   q 100 0 0 100 50 600 cm /Im4 Do Q\n";
    let mut doc = build(images, &[content]);
    let report = redact(
        &mut doc,
        &[blackout(0, Rect::new(75.0, 650.0, 100.0, 675.0))],
    );
    assert_eq!(
        report.redacted_images, 5,
        "alle fünf müssen geschwärzt sein"
    );
    assert_eq!(
        report.peak_decoded_images, 1,
        "es darf immer nur ein Bild gleichzeitig dekodiert gehalten werden"
    );
    assert_eq!(
        report.peak_decoded_image_bytes,
        40 * 40 * 4,
        "gehalten werden darf nur genau ein Bildpuffer"
    );
}

/// Dieselbe Zusicherung über Seiten hinweg: 20 Seiten mit je einem Bild und je
/// einer Schwärzung häufen nichts an. Das war Ursache (c) — die Arbeitspuffer
/// wurden über *alle* Seiten gesammelt und erst am Ende geschrieben.
#[test]
fn images_are_not_accumulated_across_pages() {
    let images: Vec<(&str, Stream)> = (0..20)
        .map(|i| (IMAGE_NAMES[i], rgb_image(40, 40)))
        .collect();
    let pages: Vec<String> = (0..20)
        .map(|i| format!("q 100 0 0 100 50 600 cm /{} Do Q\n", IMAGE_NAMES[i]))
        .collect();
    let page_refs: Vec<&str> = pages.iter().map(String::as_str).collect();
    let mut doc = build(images, &page_refs);
    let redactions: Vec<Redaction> = (0..20)
        .map(|page| blackout(page, Rect::new(75.0, 650.0, 100.0, 675.0)))
        .collect();
    let report = redact(&mut doc, &redactions);
    assert_eq!(report.redacted_images, 20);
    assert_eq!(report.peak_decoded_images, 1, "{report:?}");
}

/// **Aufgabe #58 (a).** Ein Bild, das keine Schwärzung schneidet, wird gar
/// nicht erst ausgepackt. Vorher entschied der Vorfilter nur, *ob* die Seite
/// angefasst wird — 19 unbeteiligte Bilder kosteten trotzdem 2,9 GB.
#[test]
fn an_untouched_image_on_a_redacted_page_is_never_decoded() {
    let images: Vec<(&str, Stream)> = vec![("Im0", rgb_image(40, 40)), ("Im1", rgb_image(80, 80))];
    // Im0 liegt unter der Schwärzung, Im1 ganz woanders auf derselben Seite.
    let content = "q 100 0 0 100 50 600 cm /Im0 Do Q\n\
                   q 100 0 0 100 300 100 cm /Im1 Do Q\n";
    let mut doc = build(images, &[content]);
    let report = redact(
        &mut doc,
        &[blackout(0, Rect::new(75.0, 650.0, 100.0, 675.0))],
    );
    assert_eq!(report.redacted_images, 1);
    assert_eq!(
        report.peak_decoded_image_bytes,
        40 * 40 * 4,
        "das unbeteiligte 80x80-Bild wurde ausgepackt, obwohl es niemand braucht"
    );
}

/// **Aufgabe #58, die Obergrenze.** Reicht das Budget nicht, endet der Lauf
/// mit einer Meldung — nicht mit einer gescheiterten Speicheranforderung.
/// `SECURITY.md` sichert genau das zu.
#[test]
fn the_decoded_image_budget_ends_the_run_in_a_controlled_way() {
    let mut doc = build(
        vec![("Im0", bilevel_image(2000, 2000))],
        &["q 100 0 0 100 50 600 cm /Im0 Do Q\n"],
    );
    // 2000x2000 sind 16 MB dekodiert; erlaubt wird 1 MB.
    let error = PdfRedactor::with_padding(0.0)
        .with_max_decoded_image_bytes(1024 * 1024)
        .apply_with_report(
            &mut doc,
            &[blackout(0, Rect::new(75.0, 650.0, 100.0, 675.0))],
        )
        .expect_err("hätte an der Grenze abbrechen müssen");
    let text = error.to_string();
    assert!(
        text.contains("Grenze") && text.contains("max-image-mb"),
        "die Meldung muss sagen, welche Grenze griff und wie man sie ändert: {text}"
    );
}

/// Und mit ausreichendem Budget läuft dasselbe Dokument durch.
#[test]
fn the_same_document_passes_with_enough_budget() {
    let mut doc = build(
        vec![("Im0", bilevel_image(2000, 2000))],
        &["q 100 0 0 100 50 600 cm /Im0 Do Q\n"],
    );
    let report = PdfRedactor::with_padding(0.0)
        .with_max_decoded_image_bytes(64 * 1024 * 1024)
        .apply_with_report(
            &mut doc,
            &[blackout(0, Rect::new(75.0, 650.0, 100.0, 675.0))],
        )
        .expect("64 MB reichen für 16 MB Bild");
    assert_eq!(report.redacted_images, 1);
}

const IMAGE_NAMES: [&str; 20] = [
    "Im0", "Im1", "Im2", "Im3", "Im4", "Im5", "Im6", "Im7", "Im8", "Im9", "Im10", "Im11", "Im12",
    "Im13", "Im14", "Im15", "Im16", "Im17", "Im18", "Im19",
];

// ---------------------------------------------------------------------------
// Der Beweis am Dateibyte
// ---------------------------------------------------------------------------

/// Die Bildpunkte *sind* das Geheimnis: ein 8-Bit-Graustufenbild, dessen
/// Abtastwerte die ASCII-Bytes der IBAN sind. Vorher findet [`leaks`] sie im
/// (komprimierten) Stream, hinterher darf nichts mehr davon in der Datei
/// stehen. Das misst nicht unsere eigene Buchführung, sondern die Datei.
#[test]
fn pixels_that_spell_the_secret_are_gone_from_the_file() {
    let bytes = SECRET.as_bytes();
    let width = bytes.len() as u32;
    let mut data = Vec::new();
    for _ in 0..4 {
        data.extend_from_slice(bytes);
    }
    let mut doc = build(
        vec![("Im0", image_stream(width, 4, b"DeviceGray", 8, data))],
        &["q 200 0 0 50 50 600 cm /Im0 Do Q\n"],
    );

    let before = save_to_bytes(&doc).expect("Speichern");
    assert!(
        !leaks(&before, SECRET).is_empty(),
        "Vorbedingung: das Geheimnis steht in den Bilddaten"
    );

    let (report, _, after) = roundtrip(
        &mut doc,
        &[blackout(0, Rect::new(40.0, 590.0, 260.0, 660.0))],
    );
    assert_eq!(report.redacted_images, 1);
    let hits = leaks(&after, SECRET);
    assert!(
        hits.is_empty(),
        "das Geheimnis steht noch {} mal in der Datei:\n{}",
        hits.len(),
        hits.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Inline-Bilder, Stencil-Masken, Form-XObjects
// ---------------------------------------------------------------------------

/// Ein Inline-Bild steht im Content-Stream selbst; es muss beim Neuschreiben
/// der Seite ersetzt werden.
#[test]
fn an_inline_image_is_redacted_in_place() {
    let mut raw = Vec::from(&b"q 80 0 0 80 100 600 cm\n"[..]);
    raw.extend_from_slice(b"BI /W 8 /H 8 /CS /G /BPC 8 ID ");
    // Jeder Wert einmalig: 0, 4, 8, … 252.
    raw.extend_from_slice(&(0u8..64).map(|v| v * 4).collect::<Vec<u8>>());
    raw.extend_from_slice(b" EI Q\n");

    let mut doc = build(Vec::new(), &[""]);
    let page_id = *doc.get_pages().values().next().unwrap();
    set_content(&mut doc, page_id, &raw);

    let before = images_of(&doc, 0).remove(0);
    assert_eq!((before.width, before.height), (8, 8));

    // Das Bild füllt (100,600)-(180,680), ein Pixel ist 10 Punkt groß.
    // x 130..160 -> Spalten 3..5, y 620..650 -> Zeilen 3..5.
    let (report, out, _) = roundtrip(
        &mut doc,
        &[blackout(0, Rect::new(130.0, 620.0, 160.0, 650.0))],
    );
    assert_eq!(report.redacted_images, 1);

    let after = images_of(&out, 0).remove(0);
    assert_blacked(&before, &after, (3, 5, 3, 5), "Inline-Bild");

    // Und der Strom ist danach immer noch heil.
    let out_page = *out.get_pages().values().next().unwrap();
    let content = out.get_page_content(out_page).expect("Content lesbar");
    let text = String::from_utf8_lossy(&content);
    assert_eq!(
        text.matches("BI").count(),
        text.matches("EI").count(),
        "Inline-Bild ohne Abschluss:\n{text}"
    );
}

/// Eine Stencil-Maske trägt nur die *Form* der Zeichen. Nach der Schwärzung
/// darf sie im Bereich nichts mehr malen.
#[test]
fn a_stencil_mask_stops_painting_where_it_was_redacted() {
    // 8×8, alle Bits 0 — malt überall (Standard-`/Decode [0 1]`).
    let mut stencil = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 8_i64,
            "Height" => 8_i64,
            "ImageMask" => true,
            "BitsPerComponent" => 1_i64,
        },
        vec![0x00; 8],
    );
    let _ = stencil.compress();

    let mut doc = build(
        vec![("Im0", stencil)],
        &["q 1 0 0 rg 80 0 0 80 100 600 cm /Im0 Do Q\n"],
    );
    let before = images_of(&doc, 0).remove(0);
    assert_eq!(pixel(&before, 4, 4)[3], 255, "Vorbedingung: malt überall");

    let (report, out, _) = roundtrip(
        &mut doc,
        &[blackout(0, Rect::new(130.0, 620.0, 160.0, 650.0))],
    );
    assert_eq!(report.redacted_images, 1);

    let after = images_of(&out, 0).remove(0);
    for y in 3..=5u32 {
        for x in 3..=5u32 {
            assert_eq!(
                pixel(&after, x, y)[3],
                0,
                "Maskenpixel ({x},{y}) malt immer noch"
            );
        }
    }
    assert_eq!(pixel(&after, 0, 0)[3], 255, "Rest der Maske wurde zerstört");
}

/// Bild in einem Form-XObject: die CTM entsteht erst aus Form-Matrix und
/// Platzierung.
#[test]
fn an_image_inside_a_form_xobject_is_redacted() {
    let mut doc = build(vec![("Im0", rgb_image(20, 20))], &["q /Fm0 Do Q\n"]);
    let image_ref = doc
        .get_dictionary(image_resources_id(&doc))
        .unwrap()
        .get(b"XObject")
        .unwrap()
        .clone();
    let form = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Resources" => dictionary! { "XObject" => image_ref },
        },
        b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec(),
    )
    .with_compression(false);
    let form_id = doc.add_object(Object::Stream(form));
    add_xobject(&mut doc, "Fm0", form_id);

    let before = images_of(&doc, 0).remove(0);
    let (report, out, _) = roundtrip(
        &mut doc,
        &[blackout(0, Rect::new(75.0, 650.0, 100.0, 675.0))],
    );
    assert_eq!(report.redacted_images, 1);
    let after = images_of(&out, 0).remove(0);
    assert_blacked(&before, &after, (5, 9, 5, 9), "Bild im Form-XObject");
}

// ---------------------------------------------------------------------------
// Die Bild-Warnung
// ---------------------------------------------------------------------------

/// Der gescannte Kontoauszug: nur ein Bild, kein Text, also auch keine
/// Schwärzung. Genau hier muss der Hinweis kommen — vorher kam er nie, weil bei
/// null Schwärzungen früh zurückgesprungen wurde.
#[test]
fn a_pure_scan_is_reported_even_without_any_redaction() {
    let mut doc = build(
        vec![("Im0", rgb_image(20, 20))],
        &["q 500 0 0 700 50 70 cm /Im0 Do Q\n"],
    );
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &[])
        .expect("Schwärzung");
    assert_eq!(report.removed_glyphs, 0);
    assert!(
        report.warnings.iter().any(|w| w.contains("Rasterbilder")),
        "kein Hinweis auf den Scan: {:?}",
        report.warnings
    );
}

/// Zwei alte blinde Flecken: `get_page_images` betritt keine Form-XObjects und
/// scheitert an einem indirekten `/Width`.
#[test]
fn images_in_form_xobjects_and_with_indirect_width_are_found_too() {
    let mut doc = build(Vec::new(), &["q /Fm0 Do Q\n"]);
    let width_id = doc.add_object(Object::Integer(20));
    let image = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => Object::Reference(width_id),
            "Height" => 20_i64,
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 8,
        },
        vec![0x80; 400],
    );
    let image_id = doc.add_object(Object::Stream(image));
    let form = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image_id } },
        },
        b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec(),
    )
    .with_compression(false);
    let form_id = doc.add_object(Object::Stream(form));
    add_xobject(&mut doc, "Fm0", form_id);

    let page_id = *doc.get_pages().values().next().unwrap();
    assert!(
        doc.get_page_images(page_id)
            .map(|i| i.is_empty())
            .unwrap_or(true),
        "Vorbedingung: lopdf findet dieses Bild nicht"
    );
    assert!(
        redact_pdf::image::page_has_images(&doc, page_id),
        "das Bild im Form-XObject wurde übersehen"
    );

    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &[])
        .expect("Schwärzung");
    assert!(
        report.warnings.iter().any(|w| w.contains("Rasterbilder")),
        "{:?}",
        report.warnings
    );
}

/// Befund G1-C3, zweite Stelle: auch `page_has_images` liest die Seite über
/// `filters::page_content`. Auf einer `ASCIIHexDecode`-kodierten Seite fand
/// `Document::get_page_content` (kennt den Filter nicht) kein `BI`, und die
/// Warnung „enthält Rasterbilder“ blieb aus.
#[test]
fn an_inline_image_on_a_hex_encoded_page_is_seen() {
    let raw: &[u8] = b"q 10 0 0 10 50 600 cm BI /W 1 /H 1 /CS /G /BPC 8 ID \x80 EI Q\n";
    let hex: String = raw.iter().map(|b| format!("{b:02X}")).collect::<String>() + ">";
    for encoded in [false, true] {
        let mut doc = build(Vec::new(), &[""]);
        let page_id = *doc.get_pages().values().next().unwrap();
        let content_id = doc.get_page_contents(page_id)[0];
        let stream = if encoded {
            Stream::new(
                dictionary! { "Filter" => "ASCIIHexDecode" },
                hex.clone().into_bytes(),
            )
            .with_compression(false)
        } else {
            Stream::new(dictionary! {}, raw.to_vec())
        };
        doc.objects.insert(content_id, Object::Stream(stream));
        assert!(
            redact_pdf::image::page_has_images(&doc, page_id),
            "encoded={encoded}: das Inline-Bild wurde übersehen"
        );
    }
}

#[test]
fn a_page_without_images_gets_no_image_warning() {
    let mut doc = build(Vec::new(), &["q 1 0 0 1 0 0 cm Q\n"]);
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &[])
        .expect("Schwärzung");
    assert!(
        !report.warnings.iter().any(|w| w.contains("Rasterbilder")),
        "{:?}",
        report.warnings
    );
}

// ---------------------------------------------------------------------------
// Kleinkram
// ---------------------------------------------------------------------------

fn set_content(doc: &mut Document, page_id: ObjectId, raw: &[u8]) {
    let Ok(Object::Reference(id)) = doc.get_dictionary(page_id).unwrap().get(b"Contents") else {
        panic!("kein Content-Verweis");
    };
    let id = *id;
    doc.objects.insert(
        id,
        Object::Stream(Stream::new(dictionary! {}, raw.to_vec())),
    );
}

fn image_resources_id(doc: &Document) -> ObjectId {
    let page_id = *doc.get_pages().values().next().unwrap();
    match doc.get_dictionary(page_id).unwrap().get(b"Resources") {
        Ok(Object::Reference(id)) => *id,
        other => panic!("kein Ressourcen-Verweis: {other:?}"),
    }
}

fn add_xobject(doc: &mut Document, name: &str, id: ObjectId) {
    let resources_id = image_resources_id(doc);
    let mut xobjects = doc
        .get_dictionary(resources_id)
        .unwrap()
        .get(b"XObject")
        .ok()
        .and_then(|o| o.as_dict().ok())
        .cloned()
        .unwrap_or_default();
    xobjects.set(name.as_bytes().to_vec(), Object::Reference(id));
    doc.get_dictionary_mut(resources_id)
        .unwrap()
        .set("XObject", Object::Dictionary(xobjects));
}
