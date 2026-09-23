//! Spur-A-Runde 1, Prüfer A — Klassen, die nicht auf der Probenliste stehen,
//! am **Bildobjekt selbst**: eine Maske, die unter der Schwärzung ihre Bits
//! behält; ein gewöhnlicher Filter, der den Lauf beendet; ein Wächter, der an
//! der Zahlenart hängt.
//!
//! 1. **Stencil-`/Mask` unter der Zone.** `image.rs` schreibt einen
//!    `/Mask`-Strom **unverändert** mit (`Carry::Keep`, Modulkopf: „bleibt
//!    dabei in voller Auflösung“). Die Maske trägt aber selbst Bildpunkte — sie
//!    ist ein Bitmuster in der Auflösung des Maskenbildes, und ein Bitmuster
//!    kann eine Form sein: den Umriss einer Zeile. Unter der Zone werden die
//!    Farbwerte des Bildes gefüllt; die Bits der Maske, die sagen, **wo** das
//!    Bild malt, bleiben, wie sie waren.
//! 2. **`LZWDecode`.** Ein Filter aus PDF 1.0, den ältere Distiller, `tiff2pdf`
//!    und Ghostscript ohne Flate schreiben. `filters.rs` dekodiert ihn für
//!    Content-Streams (`weezl`), `ops.rs::apply_filters` kennt ihn für Bilder
//!    nicht: `Payload::Unsupported` → Platzhalter → ohne
//!    `--allow-undecodable-images` endet der Lauf mit Fehler und ohne
//!    Ausgabedatei.
//! 3. **`/Width` als Real.** `image.rs::declared_pixels` liest `as_i64` und
//!    zählt 0, wenn die Zahl als Real geschrieben ist; `ops.rs::dict_int`
//!    nimmt Real an. Die Budgetprüfung (`--max-image-mb`) reserviert dann 0
//!    Byte, das Bild wird trotzdem ausgepackt.
//!
//! Maßstab: `redact_pdf::leaks` an den Ausgabebytes, die dekodierten Bildpunkte
//! der Ausgabe (`page_ops`), und der Rückgabewert von
//! `PdfRedactor::apply_with_report` bzw. `image::redact_images`. Die Tests,
//! die einen Befund festhalten, sind **absichtlich rot**.
//!
//! `flock /tmp/redactrs-cargo.lock cargo test -p redact-pdf --test zo_a_maske_und_filter`

mod common;

use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::image::{redact_images, ImageOptions};
use redact_pdf::{leaks, load_from_bytes, page_ops, save_to_bytes, DrawOp, PdfRedactor};

/// Steht buchstäblich in den Bildpunkten: 27 Zeichen = 9 RGB-Bildpunkte, oder
/// 27 Byte = 216 Maskenbits.
const GEHEIM: &str = "DE89 3704 0044 0532 0130 00";

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
                reason: "Spur A, Runde 1, Prüfer A".into(),
            },
        ),
        Action::Blackout,
    )
}

/// Die Zone, die die Platzierung `100 0 0 100 50 600 cm` vollständig deckt.
fn zone() -> Rect {
    Rect::new(40.0, 590.0, 160.0, 710.0)
}

/// Alphawerte der Bildpunkte, die Seite 0 der Ausgabe zeigt, in Bildreihenfolge.
fn gezeigte_alpha(bytes: &[u8]) -> Vec<u8> {
    let doc = load_from_bytes(bytes).expect("Ausgabe lädt");
    let ops = page_ops(&doc, 0).expect("page_ops läuft");
    let mut out = Vec::new();
    for op in &ops.ops {
        let DrawOp::Image { image, .. } = op else {
            continue;
        };
        let raster = &ops.images[*image];
        assert!(!raster.placeholder, "Ausgabebild ist dekodierbar");
        out.extend(raster.rgba.chunks_exact(4).map(|p| p[3]));
    }
    out
}

// ===========================================================================
// 1. Stencil-/Mask unter der Zone
// ===========================================================================

/// **Stilles Leck.** Ein 216 x 4 RGB-Bild (einfarbig, unkomprimiert) mit einem
/// `/Mask`-Stencil derselben Größe, dessen **vier Zeilen** je die 27 Bytes von
/// `GEHEIM` als Bits tragen: das Bitmuster *ist* die Form, die das Bild malt.
/// Die Zone deckt die ganze Platzierung.
///
/// Erwartung: unter der Zone fallen alle Bildpunkte — auch die Bits der Maske
/// (die Form ist ein Bildinhalt) — oder der Lauf sagt, dass die Maske steht.
/// Befund: die Maske wird byteweise unverändert mitgeschrieben, das Orakel
/// findet `GEHEIM` im Maskenstrom der Ausgabe, die gezeigte Alphaebene ist
/// dieselbe wie vorher, keine Warnung. Vermutung: `image.rs::encode_xobject`,
/// Zweig `if let Some(mask) = &work.mask { dict.set("Mask", …) }`, gespeist von
/// `mask_to_carry` → `MaskPlan::Keep`.
#[test]
fn stencil_maske_behaelt_unter_der_zone_ihre_bits() {
    let breite = 216u32;
    let hoehe = 4u32;
    let mut maske = Vec::new();
    for _ in 0..hoehe {
        maske.extend_from_slice(GEHEIM.as_bytes());
    }
    assert_eq!(maske.len(), (breite / 8 * hoehe) as usize);
    let mut doc = Document::with_version("1.5");
    let maske_id = doc.add_object(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => i64::from(breite),
                "Height" => i64::from(hoehe),
                "ImageMask" => true,
                "BitsPerComponent" => 1_i64,
            },
            maske,
        )
        .with_compression(false),
    ));
    let bild_id = doc.add_object(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => i64::from(breite),
                "Height" => i64::from(hoehe),
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8_i64,
                "Mask" => maske_id,
            },
            vec![0x60u8; (breite * hoehe * 3) as usize],
        )
        .with_compression(false),
    ));
    let res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let (mut doc, _ids) = seiten(
        doc,
        vec![(res, b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec())],
    );

    let vorher = save_to_bytes(&doc).expect("Speichern");
    assert!(
        !leaks(&vorher, GEHEIM).is_empty(),
        "Vorbedingung: das Orakel findet die Maskenbits in der Eingabe"
    );
    let alpha_vorher = gezeigte_alpha(&vorher);
    assert!(
        alpha_vorher.contains(&0) && alpha_vorher.contains(&255),
        "Vorbedingung: die Maske versteckt einen Teil und zeigt einen Teil"
    );

    let report = PdfRedactor::with_padding(0.0)
        .apply_with_report(&mut doc, &[schwaerzung(0, zone())])
        .expect("läuft");
    let out = save_to_bytes(&doc).expect("Speichern");
    assert_eq!(report.redacted_images, 1, "{:?}", report.warnings);

    let funde = leaks(&out, GEHEIM);
    let alpha_nachher = gezeigte_alpha(&out);
    let gewarnt = report.warnings.iter().any(|w| w.contains("Mask"));
    assert!(
        (funde.is_empty() && alpha_nachher != alpha_vorher) || gewarnt,
        "die Stencil-Maske steht unter der Zone unverändert: Orakel {funde:?}; \
         Alphaebene der Ausgabe {} der Eingabe; Warnungen {:?}",
        if alpha_nachher == alpha_vorher {
            "gleich"
        } else {
            "verschieden von"
        },
        report.warnings
    );
}

// ===========================================================================
// 2. LZWDecode
// ===========================================================================

fn lzw_bild_mit_klartext() -> Stream {
    let (breite, hoehe) = (9u32, 4u32);
    let mut roh = Vec::with_capacity((breite * hoehe * 3) as usize);
    for y in 0..hoehe {
        if y == 1 {
            roh.extend_from_slice(GEHEIM.as_bytes());
            continue;
        }
        for x in 0..breite {
            roh.extend_from_slice(&[(x * 9 + 1) as u8, (y * 5 + 2) as u8, 0x30]);
        }
    }
    Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => i64::from(breite),
            "Height" => i64::from(hoehe),
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8_i64,
            "Filter" => "LZWDecode",
        },
        common::lzw_encode(&roh),
    )
    .with_compression(false)
}

/// **Abgelehnte gewöhnliche Datei.** Ein LZW-gepacktes RGB-Bild unter der
/// Zone. `filters.rs` kann LZW (für Content-Streams, über `weezl`), der
/// Bilddekoder nicht. Erwartung: der Lauf endet mit Ausgabedatei und
/// geschwärzten Bildpunkten. Befund: `Err` („Filter LZWDecode wird nicht
/// unterstützt“), keine Ausgabedatei. Vermutung: `ops.rs::apply_filters`,
/// Zweig `other => Payload::Unsupported`.
#[test]
#[ignore = "offen: Register #78 LZW-Bild beendet den Lauf — Spur-A-Runde 1, Beleg absichtlich rot"]
fn lzw_bild_unter_der_zone_beendet_den_lauf() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(lzw_bild_mit_klartext()));
    let res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let (mut doc, _ids) = seiten(
        doc,
        vec![(res, b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec())],
    );

    // Vorbedingung: das Orakel entpackt LZW und findet den Klartext — die
    // Datei ist also nichts Exotisches, sie ist für den Leck-Detektor gewöhnlich.
    let vorher = save_to_bytes(&doc).expect("Speichern");
    assert!(
        !leaks(&vorher, GEHEIM).is_empty(),
        "Vorbedingung: das Orakel liest LZW und findet die Klartext-Bildpunkte"
    );

    let ergebnis =
        PdfRedactor::with_padding(0.0).apply_with_report(&mut doc, &[schwaerzung(0, zone())]);
    let report = match ergebnis {
        Ok(report) => report,
        Err(e) => {
            panic!("ein LZW-Bild ist gewöhnliches Material; der Lauf endete ohne Ausgabedatei: {e}")
        }
    };
    let out = save_to_bytes(&doc).expect("Speichern");
    assert_eq!(report.redacted_images, 1, "{:?}", report.warnings);
    assert!(leaks(&out, GEHEIM).is_empty());
}

/// Der genannte Fluchtweg: mit `--allow-undecodable-images` läuft der Lauf
/// durch — das Bild bleibt ungeschwärzt, **und das wird gesagt**. Grün; hält
/// fest, dass der Rest der Klasse nicht still ist.
#[test]
fn lzw_bild_mit_zugestaendnis_bleibt_stehen_und_wird_gemeldet() {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(lzw_bild_mit_klartext()));
    let res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let (mut doc, _ids) = seiten(
        doc,
        vec![(res, b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec())],
    );
    let report = PdfRedactor::with_padding(0.0)
        .allowing_undecodable_images(true)
        .apply_with_report(&mut doc, &[schwaerzung(0, zone())])
        .expect("mit Zugeständnis läuft es durch");
    let out = save_to_bytes(&doc).expect("Speichern");
    assert!(
        !leaks(&out, GEHEIM).is_empty(),
        "das Bild bleibt ungeschwärzt"
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("LZWDecode") && w.contains("nicht dekodieren")),
        "und das wird gesagt: {:?}",
        report.warnings
    );
}

// ===========================================================================
// 3. /Width als Real: Budget umgangen
// ===========================================================================

fn bild_100x100(width: Object, height: Object) -> Stream {
    Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => width,
            "Height" => height,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8_i64,
        },
        vec![0x77u8; 100 * 100 * 3],
    )
    .with_compression(false)
}

fn lauf_mit_budget(width: Object, height: Object) -> redact_core::Result<usize> {
    let mut doc = Document::with_version("1.5");
    let bild_id = doc.add_object(Object::Stream(bild_100x100(width, height)));
    let res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild_id } });
    let (mut doc, _ids) = seiten(
        doc,
        vec![(res, b"q 100 0 0 100 50 600 cm /Im0 Do Q\n".to_vec())],
    );
    // 1 KB Budget; das Bild bräuchte 100 * 100 * 4 = 40 000 Byte.
    let options = ImageOptions {
        allow_undecodable: false,
        max_decoded_bytes: 1024,
    };
    redact_images(&mut doc, &[schwaerzung(0, zone())], 0.0, &options).map(|o| o.redacted_images)
}

/// Kontrolle: mit ganzzahligem `/Width` greift die Grenze.
#[test]
fn budget_greift_bei_ganzzahliger_breite() {
    let ergebnis = lauf_mit_budget(Object::Integer(100), Object::Integer(100));
    assert!(
        matches!(&ergebnis, Err(e) if e.to_string().contains("überschreitet das die Grenze")),
        "erwartet: Abbruch an der Grenze, tatsächlich {ergebnis:?}"
    );
}

/// **Wächter an der Zahlenart.** Dasselbe Bild, `/Width 100.0 /Height 100.0`
/// (Real). `declared_pixels` zählt 0, `reserve(0)` geht durch, `decode_image`
/// packt 40 000 Byte aus — unter einer Grenze von 1 024 Byte. Erwartung: derselbe
/// Abbruch wie bei der ganzen Zahl. Befund: `Ok`, Bild geschwärzt, Grenze
/// umgangen. Die harte Decke `MAX_IMAGE_PIXELS` (40 Mio.) bleibt; umgangen
/// wird die **wählbare** Grenze `--max-image-mb`. Vermutung:
/// `image.rs::declared_pixels` (`as_i64`) gegen `ops.rs::dict_int` (nimmt Real).
#[test]
fn budget_wird_von_reeller_breite_umgangen() {
    let ergebnis = lauf_mit_budget(Object::Real(100.0), Object::Real(100.0));
    assert!(
        matches!(&ergebnis, Err(e) if e.to_string().contains("überschreitet das die Grenze")),
        "erwartet: Abbruch an der Grenze wie bei der ganzen Zahl, tatsächlich {ergebnis:?}"
    );
}
