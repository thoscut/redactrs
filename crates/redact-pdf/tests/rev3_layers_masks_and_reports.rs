//! Die fünf Befunde der dritten Prüfrunde — je Befund der Fall, der ihn zeigt,
//! und die Gegenprobe, die er nicht aufreißen darf.
//!
//! ## Warum diese Datei
//!
//! Zwei der Befunde waren **Regressionen**: die Schichtwahl der Extraktion
//! zerriss eine IBAN, die v0.3.0 noch ganz gefunden hatte, und eine Bildmaske
//! deckte wieder auf, was die Eingabe verbirgt. Beide sind an einer Stelle
//! entstanden, an der eine neue Regel den Fall, für den sie gebaut wurde,
//! richtig behandelt — und den Nachbarfall daneben nicht.
//!
//! Deshalb steht hier zu jedem Befund **beides**: der Fall, der schiefging,
//! und der Fall, für den die Regel ursprünglich da war. Ein Test, der nur den
//! ersten prüft, lädt zur nächsten Regression in die Gegenrichtung ein.
//!
//! Gemessen wird, wo es geht, am Ergebnis und nicht am Zwischenstand: die
//! Schichttests an der extrahierten Zeile *und* an `redact_pdf::leaks` der
//! geschriebenen Datei, die Maskentests am dekodierten Bildpunkt der Ausgabe.

use lopdf::{dictionary, Dictionary, Document, Object, Stream, StringFormat};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{
    load_from_bytes, page_ops, save_to_bytes, DrawOp, PdfExtractor, PdfRedactor, RasterImage,
    RedactionReport,
};

const IBAN: &str = "DE89 3704 0044 0532 0130 00";

// ---------------------------------------------------------------------------
// Gerüst
// ---------------------------------------------------------------------------

/// Eine Seite mit Helvetica/WinAnsi als `/F1` und rohem Content-Stream.
///
/// Roh, weil die Befunde an *Positionen* hängen: `Tm` auf hundertstel Punkt,
/// `Ts` mitten in der Zeile, dieselbe Zeichenkette zweimal um 0,3 pt versetzt.
/// Ein Erzeuger, der die Operationen selbst zusammensetzt, verstellte genau
/// das, worum es geht.
fn page_with(content: &str) -> Document {
    let mut doc = Document::with_version("1.5");
    let font_id = doc.add_object(helvetica());
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
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
    doc
}

fn helvetica() -> Dictionary {
    dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    }
}

/// Breite einer Zeichenkette in Helvetica — **mit denselben Metriken**, die
/// die Extraktion benutzt.
///
/// Abgeschriebene AFM-Zahlen wären eine zweite Quelle für dieselbe Wahrheit;
/// liefe eine der beiden weg, prüfte der Test eine Anordnung, die er gar nicht
/// meint.
fn width(text: &str, size: f64) -> f64 {
    let doc = Document::with_version("1.5");
    let info = redact_pdf::font::font_from_dict(&doc, &helvetica());
    text.chars()
        .map(|c| info.width(c as u32, &c.to_string()) * size)
        .sum()
}

fn lines_of(doc: &Document) -> Vec<String> {
    PdfExtractor::new()
        .extract(doc)
        .expect("Extraktion")
        .into_iter()
        .map(|run| run.text)
        .collect()
}

fn blackout(page: usize, rect: Rect) -> Redaction {
    Redaction::new(
        Region::new(
            page,
            rect,
            None,
            Source::Manual {
                reason: "Prüfrunde 3".into(),
            },
        ),
        Action::Blackout,
    )
}

// ---------------------------------------------------------------------------
// Befund 1 — die Schichtwahl darf die IBAN nicht zerreißen
// ---------------------------------------------------------------------------

/// Der gewöhnliche Tabellenfall: ein zu langer Empfängername ragt in die
/// Wertspalte, die IBAN steht in zwei `Tj`.
///
/// `overhang` ist, wie weit das Label über das Ende des **ersten**
/// IBAN-Stücks hinausreicht; `dy` der Grundlinienversatz der beiden
/// Tabellenzellen (verschiedene vertikale Ausrichtung, noch innerhalb der
/// Zeilentoleranz).
fn table_row(overhang: f64, dy: f64) -> String {
    let label = "Empfaenger: Mueller Handels GmbH";
    let first = "DE89 3704 0044 ";
    let second = "0532 0130 00";
    // Wertspalte so legen, dass das Label genau `overhang` Punkt hinter das
    // Ende des ersten Stücks reicht.
    let start = 60.0 + width(label, 10.0) - width(first, 10.0) - overhang;
    format!(
        "BT\n/F1 10 Tf\n\
         1 0 0 1 60 {:.4} Tm\n({label}) Tj\n\
         1 0 0 1 {:.4} 700 Tm\n({first}) Tj\n\
         1 0 0 1 {:.4} 700 Tm\n({second}) Tj\nET\n",
        700.0 + dy,
        start,
        start + width(first, 10.0),
    )
}

#[test]
fn a_label_that_overhangs_the_value_column_keeps_the_iban_whole() {
    // Der gemessene Fall: 2 pt Grundlinienversatz, 15 pt Überstand. Vorher
    // stand „…GmbH 0532 0130 00“ in der einen und „DE89 3704 0044“ in der
    // anderen Zeile — kein Muster griff mehr, der Lauf meldete 0 Treffer.
    let doc = page_with(&table_row(15.0, 2.0));
    let lines = lines_of(&doc);
    assert!(
        lines.iter().any(|l| l.contains(IBAN)),
        "die IBAN ist zerrissen: {lines:?}"
    );
}

#[test]
fn the_whole_measured_window_keeps_the_iban_whole() {
    // Das Fenster aus dem Befund: Grundlinienversatz 0–4 pt mal Überstand
    // 0–40 pt. Ein einzelner Punkt daraus wäre Zufall; erst die Fläche zeigt,
    // dass die Regel und nicht die Rundung entscheidet.
    for dy in [0.0, 0.5, 1.0, 1.5, 2.0, 3.0, 4.0] {
        for overhang in [0.0, 5.0, 10.0, 15.0, 20.0, 25.0, 30.0, 35.0, 40.0] {
            let doc = page_with(&table_row(overhang, dy));
            let lines = lines_of(&doc);
            assert!(
                lines.iter().any(|l| l.contains(IBAN)),
                "dy={dy} Überstand={overhang}: die IBAN ist zerrissen: {lines:?}"
            );
        }
    }
}

#[test]
fn the_run_that_is_really_redacted_leaves_no_leak() {
    // Dieselbe Anordnung, aber am Ergebnis gemessen: nicht „die Zeile sieht
    // gut aus“, sondern „die IBAN steht nicht mehr in der Datei“.
    let mut doc = page_with(&table_row(15.0, 2.0));
    let line = lines_of(&doc)
        .into_iter()
        .find(|l| l.contains(IBAN))
        .expect("die IBAN steht in einer Zeile");
    assert!(line.contains(IBAN));

    // Das Rechteck über die ganze Wertspalte — so, wie es ein Treffer des
    // Musters `iban_de` liefern würde.
    let rect = Rect::new(120.0, 696.0, 280.0, 712.0);
    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(&mut doc, &[blackout(0, rect)])
        .expect("Schwärzung");
    assert!(
        report.removed_glyphs >= IBAN.len(),
        "es wurden nur {} Zeichen entfernt",
        report.removed_glyphs
    );
    let bytes = save_to_bytes(&doc).expect("Speichern");
    assert!(
        redact_pdf::leaks(&bytes, IBAN).is_empty(),
        "die IBAN steht noch in der Datei"
    );
}

#[test]
fn a_hair_of_overlap_does_not_send_a_run_to_a_far_away_layer() {
    // Der Fall, an dem die naive Fassung der Korrektur („anschließend gewinnt
    // immer“) zerbrach: zwei aufeinanderfolgende `Tj` überlappen sich um 0,5 pt
    // (gerundete Metriken eines Erzeugers), während eine 28 pt zurückliegende
    // Schicht formal „davor“ endet. Wer nur „davor“ prüft, schickt die
    // Fortsetzung der IBAN in die Schicht des Fett-Imitats.
    let name = "Auftraggeber";
    let head = "DE89 3704 0044 ";
    let tail = "0532 0130 00";
    let x_head = 60.0 + width(name, 10.0) * 0.16;
    // 0,5 pt *vor* dem rechnerischen Ende des ersten Stücks.
    let x_tail = x_head + width(head, 10.0) - 0.5;
    let content = format!(
        "BT\n/F1 10 Tf\n\
         1 0 0 1 60 700 Tm\n({name}) Tj\n\
         1 0 0 1 60.3000 700 Tm\n({name}) Tj\n\
         1 0 0 1 {x_head:.4} 700 Tm\n({head}) Tj\n\
         1 0 0 1 {x_tail:.4} 700 Tm\n({tail}) Tj\nET\n"
    );
    let doc = page_with(&content);
    let lines = lines_of(&doc);
    assert!(
        lines.iter().any(|l| l.contains(IBAN)),
        "die Fortsetzung ist in die falsche Schicht gewandert: {lines:?}"
    );
}

#[test]
fn fake_bold_still_becomes_two_layers() {
    // Die Gegenprobe: der Fall, für den die Schichten überhaupt gebaut wurden.
    // Dieselbe Zeile zweimal, um 0,3 pt versetzt. Ohne Schichten verschränkte
    // das Verschmelzen sie zeichenweise („DDEE8899 …“) und kein Muster griffe.
    let content = format!(
        "BT\n/F1 10 Tf\n\
         1 0 0 1 60 700 Tm\n(IBAN: {IBAN}) Tj\n\
         1 0 0 1 60.3000 700 Tm\n(IBAN: {IBAN}) Tj\nET\n"
    );
    let doc = page_with(&content);
    let lines = lines_of(&doc);
    assert_eq!(
        lines.len(),
        2,
        "das Fett-Imitat muss zwei Schichten ergeben: {lines:?}"
    );
    assert!(
        lines.iter().all(|l| l.contains(IBAN)),
        "beide Schichten müssen die IBAN ganz enthalten: {lines:?}"
    );
}

#[test]
fn an_overprint_of_the_whole_line_still_becomes_two_layers() {
    // Und derselbe Fall ohne jeden Versatz: exakt übereinander gedruckt.
    let content = format!(
        "BT\n/F1 10 Tf\n1 0 0 1 60 700 Tm\n({IBAN}) Tj\nET\n\
         BT\n/F1 10 Tf\n1 0 0 1 60 700 Tm\n({IBAN}) Tj\nET\n"
    );
    let doc = page_with(&content);
    assert_eq!(lines_of(&doc), vec![IBAN.to_string(), IBAN.to_string()]);
}

// ---------------------------------------------------------------------------
// Befund 2 — die befolgte Maske ist die, die in die Ausgabe muss
// ---------------------------------------------------------------------------

/// Eine Seite 300×300 mit einem Bild bei (100,100)-(180,180).
const IMAGE_PLACEMENT: &str = "q 80 0 0 80 100 100 cm /Im0 Do Q\n";

/// Die rechte Bildhälfte — sie berührt das Bild, ohne die untere (verborgene)
/// Hälfte zu treffen.
fn touches_the_image() -> Rect {
    Rect::new(141.0, 141.0, 179.0, 179.0)
}

fn image_stream(width: u32, height: u32, extra: Dictionary, data: Vec<u8>) -> Stream {
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

fn page_with_image(streams: Vec<(&str, Stream)>) -> (Document, Vec<Object>) {
    let mut doc = Document::with_version("1.5");
    let mut names = Dictionary::new();
    let mut ids = Vec::new();
    for (name, stream) in streams {
        let id = doc.add_object(Object::Stream(stream));
        ids.push(Object::Reference(id));
        if !name.is_empty() {
            names.set(name.as_bytes().to_vec(), Object::Reference(id));
        }
    }
    let resources_id = doc.add_object(dictionary! { "XObject" => names });
    let content_id = doc.add_object(Stream::new(
        dictionary! {},
        IMAGE_PLACEMENT.as_bytes().to_vec(),
    ));
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
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    (doc, ids)
}

fn first_image(doc: &Document) -> RasterImage {
    let ops = page_ops(doc, 0).expect("Seitenoperationen");
    ops.ops
        .iter()
        .find_map(|op| match op {
            DrawOp::Image { image, .. } => Some(ops.images[*image].clone()),
            _ => None,
        })
        .expect("ein Bild")
}

fn alpha_at(image: &RasterImage, x: u32, y: u32) -> u8 {
    image.rgba[((y * image.width + x) * 4 + 3) as usize]
}

fn roundtrip(doc: &mut Document, rect: Rect) -> (RedactionReport, Document) {
    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(doc, &[blackout(0, rect)])
        .expect("Schwärzung");
    let bytes = save_to_bytes(doc).expect("Speichern");
    (report, load_from_bytes(&bytes).expect("Laden"))
}

/// 8×8 RGB: oben grau, unten ein Schwarz-Weiß-Muster — das „Geheimnis“, das
/// die Maske verbirgt.
fn grey_over_pattern() -> Vec<u8> {
    let mut data = Vec::new();
    for y in 0..8u32 {
        for x in 0..8u32 {
            if y < 4 {
                data.extend_from_slice(&[200, 200, 200]);
            } else if (x + y) % 2 == 0 {
                data.extend_from_slice(&[0, 0, 0]);
            } else {
                data.extend_from_slice(&[255, 255, 255]);
            }
        }
    }
    data
}

/// Baut das regelwidrige, aber anstandslos gerenderte Bild: `/SMask` verbirgt
/// die untere Hälfte, ein `/Mask`-Stencil daneben verbirgt nichts.
fn image_with_smask_and_mask() -> Document {
    // /SMask: oben deckend, unten völlig durchsichtig.
    let mut alpha = vec![255u8; 8 * 4];
    alpha.extend(std::iter::repeat_n(0u8, 8 * 4));
    let smask = image_stream(
        8,
        8,
        dictionary! {
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 8_i64,
        },
        alpha,
    );
    // /Mask: alle Bits 0 — bei `/Decode [0 1]` malt die Null, das Stencil
    // verbirgt also gar nichts.
    let stencil = image_stream(
        8,
        8,
        dictionary! {
            "ImageMask" => true,
            "BitsPerComponent" => 1_i64,
            "Decode" => vec![Object::Integer(0), Object::Integer(1)],
        },
        vec![0x00; 8],
    );

    let mut doc = Document::with_version("1.5");
    let smask_id = doc.add_object(Object::Stream(smask));
    let stencil_id = doc.add_object(Object::Stream(stencil));
    let image = image_stream(
        8,
        8,
        dictionary! {
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8_i64,
            "SMask" => Object::Reference(smask_id),
            "Mask" => Object::Reference(stencil_id),
        },
        grey_over_pattern(),
    );
    let image_id = doc.add_object(Object::Stream(image));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => Object::Reference(image_id) },
    });
    let content_id = doc.add_object(Stream::new(
        dictionary! {},
        IMAGE_PLACEMENT.as_bytes().to_vec(),
    ));
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
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    doc
}

/// Das Bild-Dictionary der Ausgabe — dort steht, welche Maske überlebt hat.
fn image_dict_of(doc: &Document) -> Dictionary {
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
    let entry = xobjects.get(b"Im0").expect("Bild");
    doc.dereference(entry)
        .expect("auflösbar")
        .1
        .as_stream()
        .expect("Strom")
        .dict
        .clone()
}

#[test]
fn an_smask_beside_a_mask_keeps_hiding_what_it_hid() {
    // Der Befund: `apply_soft_mask` rechnet das `/SMask` in den Alphakanal
    // (richtig — daran halten sich die Betrachter), `mask_plan` sah davon
    // nichts und ließ das `/Mask` mitschreiben. `encode_xobject` stieg
    // daraufhin vor dem Alphakanal aus: das Bild behielt die Maske, an die
    // sich keiner hält, und verlor die, an die sich alle halten.
    let mut doc = image_with_smask_and_mask();
    let before = first_image(&doc);
    for x in 0..8 {
        assert_eq!(alpha_at(&before, x, 6), 0, "die Eingabe verbirgt unten");
    }

    // Es genügt, dass **irgendeine** Schwärzung das Bild berührt; die
    // verborgene Stelle muss sie nicht treffen.
    let (_, after) = roundtrip(&mut doc, touches_the_image());
    let out = first_image(&after);
    for x in 0..8 {
        assert_eq!(
            alpha_at(&out, x, 6),
            0,
            "der verborgene Bildpunkt ({x},6) ist sichtbar geworden"
        );
    }
    let dict = image_dict_of(&after);
    assert!(
        dict.get(b"SMask").is_ok(),
        "die befolgte Maske fehlt in der Ausgabe: {dict:?}"
    );
    assert!(
        dict.get(b"Mask").is_err(),
        "die *nicht* befolgte Maske steht in der Ausgabe und verdrängt die andere: {dict:?}"
    );
}

#[test]
fn a_stencil_mask_on_its_own_is_still_carried_unchanged() {
    // Die Gegenprobe: ohne `/SMask` daneben ist der `/Mask`-Strom die befolgte
    // Maske. Er steht neben dem Bild, beschreibt es im Einheitsquadrat und muss
    // unverändert mitgeschrieben werden — ihn in eine Alphaebene zu rechnen
    // bräche ihn auf die Auflösung des Bildes herunter.
    let stencil = image_stream(
        8,
        8,
        dictionary! {
            "ImageMask" => true,
            "BitsPerComponent" => 1_i64,
            "Decode" => vec![Object::Integer(0), Object::Integer(1)],
        },
        // Untere Hälfte 1 = malt nicht: dort ist das Bild unsichtbar.
        vec![0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF],
    );
    let mut doc = Document::with_version("1.5");
    let stencil_id = doc.add_object(Object::Stream(stencil));
    let image = image_stream(
        8,
        8,
        dictionary! {
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8_i64,
            "Mask" => Object::Reference(stencil_id),
        },
        grey_over_pattern(),
    );
    let image_id = doc.add_object(Object::Stream(image));
    let resources_id = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Im0" => Object::Reference(image_id) },
    });
    let content_id = doc.add_object(Stream::new(
        dictionary! {},
        IMAGE_PLACEMENT.as_bytes().to_vec(),
    ));
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
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);

    let (_, after) = roundtrip(&mut doc, touches_the_image());
    let dict = image_dict_of(&after);
    assert!(
        dict.get(b"Mask").is_ok(),
        "der Stencil-Strom ist verlorengegangen: {dict:?}"
    );
    let out = first_image(&after);
    for x in 0..8 {
        assert_eq!(
            alpha_at(&out, x, 6),
            0,
            "der verborgene Bildpunkt ({x},6) ist sichtbar geworden"
        );
    }
}

// ---------------------------------------------------------------------------
// Befund 3 — die Härtung war schärfer als nötig
// ---------------------------------------------------------------------------

/// Ein gültiges 8×8-JPEG in **dunklem Blau** — kein Abtastwert kommt dem
/// Schlüssel „Weiß ist durchsichtig“ auch nur nahe.
const JPEG_DARK: &str = concat!(
    "ffd8ffe000104a46494600010100000100010000ffdb0043000302020302020303030304030304",
    "050805050404050a070706080c0a0c0c0b0a0b0b0d0e12100d0e110e0b0b101610111314151515",
    "0c0f171816141812141514ffdb00430103040405040509050509140d0b0d141414141414141414",
    "141414141414141414141414141414141414141414141414141414141414141414141414141414",
    "1414ffc00011080008000803012200021101031101ffc4001f0000010501010101010100000000",
    "000000000102030405060708090a0bffc400b5100002010303020403050504040000017d010203",
    "00041105122131410613516107227114328191a1082342b1c11552d1f02433627282090a161718",
    "191a25262728292a3435363738393a434445464748494a535455565758595a636465666768696a",
    "737475767778797a838485868788898a92939495969798999aa2a3a4a5a6a7a8a9aab2b3b4b5b6",
    "b7b8b9bac2c3c4c5c6c7c8c9cad2d3d4d5d6d7d8d9dae1e2e3e4e5e6e7e8e9eaf1f2f3f4f5f6f7",
    "f8f9faffc4001f0100030101010101010101010000000000000102030405060708090a0bffc400",
    "b51100020102040403040705040400010277000102031104052131061241510761711322328108",
    "144291a1b1c109233352f0156272d10a162434e125f11718191a262728292a35363738393a4344",
    "45464748494a535455565758595a636465666768696a737475767778797a82838485868788898a",
    "92939495969798999aa2a3a4a5a6a7a8a9aab2b3b4b5b6b7b8b9bac2c3c4c5c6c7c8c9cad2d3d4",
    "d5d6d7d8d9dae2e3e4e5e6e7e8e9eaf2f3f4f5f6f7f8f9faffda000c03010002110311003f00f8",
    "528a28afd2cf9a3fffd9",
);

fn unhex(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    (0..bytes.len() / 2)
        .map(|i| u8::from_str_radix(std::str::from_utf8(&bytes[i * 2..i * 2 + 2]).unwrap(), 16))
        .collect::<std::result::Result<Vec<u8>, _>>()
        .expect("Hex")
}

fn jpeg_page(mask: Option<Vec<Object>>, data: Vec<u8>) -> Document {
    let mut extra = dictionary! {
        "ColorSpace" => "DeviceRGB",
        "BitsPerComponent" => 8_i64,
        "Filter" => "DCTDecode",
    };
    if let Some(mask) = mask {
        extra.set("Mask", Object::Array(mask));
    }
    let (doc, _) = page_with_image(vec![("Im0", image_stream(8, 8, extra, data))]);
    doc
}

fn white_is_transparent() -> Vec<Object> {
    vec![
        Object::Integer(250),
        Object::Integer(255),
        Object::Integer(250),
        Object::Integer(255),
        Object::Integer(250),
        Object::Integer(255),
    ]
}

fn redaction_result(doc: &mut Document) -> std::result::Result<RedactionReport, String> {
    PdfRedactor::with_padding(0.0)
        .apply_with_report(doc, &[blackout(0, touches_the_image())])
        .map_err(|e| e.to_string())
}

#[test]
fn a_colour_key_that_hits_nothing_lets_the_run_finish() {
    // JPEG mit Farbschlüssel ist zulässiges PDF (Logos mit „Weiß ist
    // durchsichtig“). Vorher endete der Lauf hier mit Rückgabewert 1 und
    // **ohne Ausgabedatei** — noch bevor irgendetwas dekodiert war, also ohne
    // zu wissen, ob der Schlüssel überhaupt einen Bildpunkt trifft. In diesem
    // Bild ist kein einziger Abtastwert auch nur in der Nähe von Weiß.
    let mut doc = jpeg_page(Some(white_is_transparent()), unhex(JPEG_DARK));
    let report = redaction_result(&mut doc).expect("der Lauf muss durchlaufen");
    assert_eq!(report.redacted_images, 1);
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("Farbschlüssel") && w.contains("verbirgt nichts")),
        "das stillschweigende Fallenlassen wäre der falsche Weg: {:?}",
        report.warnings
    );
}

#[test]
fn a_colour_key_that_hits_pixels_still_stops_the_run() {
    // Die Gegenprobe — und die Grenze der Lockerung. Derselbe Schlüssel, aber
    // ein Bild, dessen Werte hineinfallen: dann verbirgt die Maske wirklich
    // etwas, und Abbrechen ist die ehrliche Antwort.
    //
    // Getestet wird über die *untere* Grenze des Schlüssels hinweg mit dem
    // Band, das dem verlustbehafteten Decoder zugestanden wird: 250−24 = 226.
    // Das dunkle Blau des Bildes liegt weit darunter, ein weißes gar nicht.
    let key = vec![
        Object::Integer(0),
        Object::Integer(255),
        Object::Integer(0),
        Object::Integer(255),
        Object::Integer(0),
        Object::Integer(255),
    ];
    let mut doc = jpeg_page(Some(key), unhex(JPEG_DARK));
    let message = redaction_result(&mut doc).expect_err("der Lauf muss abbrechen");
    assert!(
        message.contains("Farbschlüssel") && message.contains("DCTDecode"),
        "die Meldung nennt den Grund nicht: {message}"
    );
    assert!(
        message.contains("--allow-undecodable-images"),
        "der Fluchtweg wirkt, wird aber nicht genannt: {message}"
    );
}

#[test]
fn the_escape_hatch_really_works_for_a_colour_key() {
    // Und er wirkt auch: mit dem Schalter entsteht eine Ausgabedatei, das Bild
    // bleibt ungeschwärzt, und *das* wird gesagt.
    let key = vec![
        Object::Integer(0),
        Object::Integer(255),
        Object::Integer(0),
        Object::Integer(255),
        Object::Integer(0),
        Object::Integer(255),
    ];
    let mut doc = jpeg_page(Some(key), unhex(JPEG_DARK));
    let report = PdfRedactor::with_padding(0.0)
        .allowing_undecodable_images(true)
        .apply_with_report(&mut doc, &[blackout(0, touches_the_image())])
        .expect("mit dem Schalter läuft es durch");
    assert_eq!(report.redacted_images, 0, "das Bild bleibt ungeschwärzt");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("Farbschlüssel") && w.contains("ungeschwärzt")),
        "der Verzicht muss gesagt werden: {:?}",
        report.warnings
    );
}

#[test]
fn a_lossless_colour_key_is_untouched_by_the_loosening() {
    // Die dritte Gegenprobe: ein Farbschlüssel auf **verlustfreien** Daten
    // wird wie bisher in den Alphakanal gerechnet und nicht mitgeschrieben —
    // dort gibt es nichts zu prüfen und nichts zu lockern.
    let mut data = Vec::new();
    for y in 0..8u32 {
        for x in 0..8u32 {
            if y >= 4 {
                data.extend_from_slice(&[255, 255, 255]);
            } else if x < 4 {
                data.extend_from_slice(&[200, 10, 10]);
            } else {
                data.extend_from_slice(&[10, 10, 200]);
            }
        }
    }
    let (mut doc, _) = page_with_image(vec![(
        "Im0",
        image_stream(
            8,
            8,
            dictionary! {
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8_i64,
                "Mask" => Object::Array(white_is_transparent()),
            },
            data,
        ),
    )]);
    let (_, after) = roundtrip(&mut doc, touches_the_image());
    let dict = image_dict_of(&after);
    assert!(
        dict.get(b"Mask").is_err(),
        "ein Farbschlüssel darf nicht mitgeschrieben werden: {dict:?}"
    );
    let out = first_image(&after);
    for x in 0..8 {
        assert_eq!(
            alpha_at(&out, x, 6),
            0,
            "der Farbschlüssel muss im Alphakanal stehen ({x},6)"
        );
    }
}

// ---------------------------------------------------------------------------
// Befund 4 — ein Erscheinungsstrom ist ein platzierter Strom
// ---------------------------------------------------------------------------

/// Ein Erscheinungsstrom mit `/BBox [0 0 330 20]` und dem gegebenen Text.
fn appearance(doc: &mut Document, font: Object, text: &str) -> Object {
    let id = doc.add_object(Object::Stream(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 330.into(), 20.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        },
        format!("BT /F1 10 Tf 2 6 Td ({text}) Tj ET").into_bytes(),
    )));
    Object::Reference(id)
}

#[test]
fn an_appearance_stream_that_is_also_a_resource_is_not_called_undrawn() {
    // Erzeuger legen einen Erscheinungsstrom gern zusätzlich in `/Resources
    // /XObject` (Stempel, Logos). Vorher galt er damit als „deklariert, aber
    // nie gezeichnet“: der Lauf meldete, sein Text sei **nicht durchsucht**
    // worden, und setzte Rückgabewert 3 — obwohl er durchsucht *und*
    // geschwärzt wurde. Das ist die Umkehrung der Wahrheit an genau der
    // Stelle, an der Rückgabewert 3 etwas bedeuten soll.
    let mut doc = Document::with_version("1.5");
    let font_id = doc.add_object(helvetica());
    let font = Object::Reference(font_id);
    let ap = appearance(&mut doc, font.clone(), &format!("IBAN: {IBAN}"));
    let annot_id = doc.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "FreeText",
        "Rect" => vec![72.into(), 680.into(), 402.into(), 700.into()],
        "F" => 4_i64,
        "AP" => dictionary! { "N" => ap.clone() },
    });
    let content_id = doc.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 10 Tf 72 730 Td (Kontoauszug) Tj ET\n".to_vec(),
    ));
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => font },
            // Hier steht er ein zweites Mal.
            "XObject" => dictionary! { "Ap0" => ap },
        },
        "Annots" => vec![Object::Reference(annot_id)],
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

    let (runs, warnings) = PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("Extraktion");
    assert!(
        runs.iter().any(|r| r.text.contains(IBAN)),
        "der Text des Erscheinungsstroms wurde sehr wohl gelesen: {:?}",
        runs.iter().map(|r| &r.text).collect::<Vec<_>>()
    );
    assert!(
        !warnings.iter().any(|w| w.contains("nirgends gezeichnet")),
        "gelesener Text darf nicht als ungelesen gemeldet werden: {warnings:?}"
    );
}

#[test]
fn a_shared_appearance_stream_on_two_pages_is_reported() {
    // Zwei Widgets auf zwei Seiten teilen sich **einen** `/AP /N`-Strom.
    // Geschwärzt wird nur Seite 1 — Seite 2 ändert sich mit. Das ist die
    // sichere Richtung und bleibt es; ungesagt ist es eine Überraschung in
    // einer Datei, die danach weitergegeben wird. Der Modulkopf von
    // `redact_pdf::redact` verspricht die Meldung; vorher kam sie nie.
    let mut doc = Document::with_version("1.5");
    let font_id = doc.add_object(helvetica());
    let font = Object::Reference(font_id);
    let shared = appearance(
        &mut doc,
        font.clone(),
        &format!("Sachbearbeiter Meier   IBAN: {IBAN}"),
    );
    let resources = dictionary! { "Font" => dictionary! { "F1" => font } };
    let pages_id = doc.new_object_id();
    let mut page_ids = Vec::new();
    for (index, y) in [700_i64, 320].into_iter().enumerate() {
        let annot_id = doc.add_object(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Widget",
            "FT" => "Tx",
            "T" => Object::string_literal(format!("F{index}")),
            "Rect" => vec![72.into(), (y - 20).into(), 402.into(), y.into()],
            "F" => 4_i64,
            "AP" => dictionary! { "N" => shared.clone() },
        });
        let content_id = doc.add_object(Stream::new(
            dictionary! {},
            format!("BT /F1 10 Tf 72 780 Td (Seite {}) Tj ET\n", index + 1).into_bytes(),
        ));
        page_ids.push(Object::Reference(doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Resources" => resources.clone(),
            "Annots" => vec![Object::Reference(annot_id)],
        })));
    }
    let count = page_ids.len() as i64;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => page_ids,
            "Count" => count,
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);

    // Nur Seite 1, und nur der Bereich, in dem die IBAN steht.
    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(
            &mut doc,
            &[blackout(0, Rect::new(180.0, 682.0, 400.0, 698.0))],
        )
        .expect("Schwärzung");
    assert!(
        report.removed_glyphs > 0,
        "die Schwärzung muss überhaupt greifen"
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("die Schwärzung wirkt deshalb auch auf die anderen Seiten")),
        "die stille Mitschwärzung der zweiten Seite bleibt ungesagt: {:?}",
        report.warnings
    );
}

// ---------------------------------------------------------------------------
// Befund 5 — eine entfernte Annotation wird benannt, nicht nur gezählt
// ---------------------------------------------------------------------------

#[test]
fn a_removed_annotation_is_named_not_only_counted() {
    // `remove_annotations` löscht **jede** Annotation, deren `/Rect` einen
    // Schwärzungsbereich schneidet, samt allen Zuständen. Am Verhalten ist
    // nichts zu ändern — aber „Entfernte Annotationen: 1“ sagt nicht, dass
    // damit ein Formularfeld samt sichtbarem Text verschwunden ist.
    let mut doc = Document::with_version("1.5");
    let font_id = doc.add_object(helvetica());
    let font = Object::Reference(font_id);
    let on = appearance(&mut doc, font.clone(), &format!("IBAN: {IBAN}"));
    let off = appearance(&mut doc, font.clone(), "Nicht zutreffend");
    let annot_id = doc.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Widget",
        "FT" => "Btn",
        "T" => Object::String(b"Kontonummer".to_vec(), StringFormat::Literal),
        "Rect" => vec![72.into(), 680.into(), 402.into(), 700.into()],
        "F" => 4_i64,
        "AS" => "Off",
        "AP" => dictionary! { "N" => dictionary! { "On" => on, "Off" => off } },
    });
    let content_id = doc.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 10 Tf 72 730 Td (Kontoauszug) Tj ET\n".to_vec(),
    ));
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        "Annots" => vec![Object::Reference(annot_id)],
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

    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(
            &mut doc,
            &[blackout(0, Rect::new(180.0, 682.0, 400.0, 698.0))],
        )
        .expect("Schwärzung");
    assert_eq!(report.removed_annotations, 1);
    assert_eq!(
        report.removed_annotation_details,
        vec!["Seite 1 /Widget „Kontonummer“".to_string()],
        "der Verlust muss benannt sein, nicht nur gezählt"
    );
    // Und er darf den Rückgabewert nicht verstellen: zu viel geschwärzt ist
    // keine Deckungslücke (`redact_pipeline::coverage`). Die Aufzählung gehört
    // deshalb in ein eigenes Feld und nicht in die Warnungen.
    assert!(
        !report
            .warnings
            .iter()
            .any(|w| w.contains("Annotation") && w.contains("entfernt")),
        "das gehört nicht in die Warnliste: {:?}",
        report.warnings
    );
}

#[test]
fn an_annotation_that_stays_is_not_listed() {
    // Gegenprobe: was nicht entfernt wird, taucht auch nicht auf.
    let mut doc = Document::with_version("1.5");
    let font_id = doc.add_object(helvetica());
    let font = Object::Reference(font_id);
    let ap = appearance(&mut doc, font.clone(), "Bemerkung");
    let annot_id = doc.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "FreeText",
        // Weit weg vom Schwärzungsbereich.
        "Rect" => vec![72.into(), 100.into(), 402.into(), 120.into()],
        "F" => 4_i64,
        "AP" => dictionary! { "N" => ap },
    });
    let content_id = doc.add_object(Stream::new(
        dictionary! {},
        format!("BT /F1 10 Tf 72 700 Td (IBAN: {IBAN}) Tj ET\n").into_bytes(),
    ));
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        "Annots" => vec![Object::Reference(annot_id)],
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

    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(
            &mut doc,
            &[blackout(0, Rect::new(90.0, 696.0, 260.0, 712.0))],
        )
        .expect("Schwärzung");
    assert_eq!(report.removed_annotations, 0);
    assert!(report.removed_annotation_details.is_empty());
}
