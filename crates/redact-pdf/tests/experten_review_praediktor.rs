//! Expertenprüfung: Prädiktoren hinter `FlateDecode` in Bildern.
//!
//! Bis zu dieser Runde rechnete `ops.rs` die PNG-Zeilenlänge als
//! `Columns * Colors * max(BPC, 8) / 8` Byte. Bei `/BitsPerComponent 1` ist
//! eine Zeile aber `ceil(Columns / 8)` Byte lang; der Zeilendekoder scheiterte,
//! und `unwrap_or(data)` nahm die **Rohdaten samt Filterbytes** als
//! Abtastwerte. Das Bild wurde geschert gelesen, die Zone schwärzte die
//! falschen Bytes, und ein Teil des Geheimnisses stand im neu kodierten Bild —
//! mit `redacted_images = 1` und ohne Warnung. Dasselbe geschah bei 8 Bit mit
//! einem einzigen Restbyte hinter der letzten Zeile. Der TIFF-Prädiktor 2
//! wurde ganz übergangen: die Differenzen galten als Bildpunkte.

use std::io::Write;

use lopdf::{dictionary, Dictionary, Document, Object, Stream};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{load_from_bytes, page_ops, save_to_bytes, DrawOp, PdfRedactor, RasterImage};

/// 27 Byte = 216 Bit: bei 1 Bit genau eine Zeile von 216 Bildpunkten.
const GEHEIM: &[u8] = b"DE89 3704 0044 0532 0130 00";

/// Die Platzierung `216 0 0 64 50 500 cm` — obere Hälfte y 532..564.
const ZONE_OBEN: (f64, f64, f64, f64) = (40.0, 532.5, 280.0, 570.0);

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

/// Eine Seite mit genau einem Bild-XObject, platziert auf 216 x 64 pt bei
/// (50, 500).
fn dokument(bild: Dictionary, daten: Vec<u8>) -> Document {
    let mut doc = Document::with_version("1.5");
    let mut dict = dictionary! { "Type" => "XObject", "Subtype" => "Image" };
    for (k, v) in bild.iter() {
        dict.set(k.clone(), v.clone());
    }
    let bild = doc.add_object(Object::Stream(
        Stream::new(dict, daten).with_compression(false),
    ));
    let res = doc.add_object(dictionary! { "XObject" => dictionary! { "Im0" => bild } });
    let content = doc.add_object(Stream::new(
        dictionary! {},
        b"q 216 0 0 64 50 500 cm /Im0 Do Q\n".to_vec(),
    ));
    let pages_id = doc.new_object_id();
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => content,
        "Resources" => res,
        "MediaBox" => vec![0.into(), 0.into(), 600.into(), 842.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1_i64,
        }),
    );
    let cat = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", cat);
    doc
}

/// 64 Zeilen, die oberen 32 tragen `geheim`, die unteren `fuell`; vor jeder
/// Zeile das PNG-Filterbyte „None“.
fn png_zeilen(geheim: &[u8], fuell: u8) -> Vec<u8> {
    let mut roh = Vec::new();
    for y in 0..64 {
        roh.push(0u8);
        if y < 32 {
            roh.extend_from_slice(geheim);
        } else {
            roh.extend(std::iter::repeat_n(fuell, geheim.len()));
        }
    }
    roh
}

fn schwaerze_oben(doc: &mut Document) -> redact_core::Result<redact_pdf::RedactionReport> {
    let (x0, y0, x1, y1) = ZONE_OBEN;
    PdfRedactor::with_padding(0.0).apply_with_report(
        doc,
        &[Redaction::new(
            Region::new(
                0,
                Rect::new(x0, y0, x1, y1),
                None,
                Source::Manual {
                    reason: "Prädiktor".into(),
                },
            ),
            Action::Blackout,
        )],
    )
}

/// Das erste Bild der Seite, wie der Interpreter es liest.
fn erstes_bild(doc: &Document) -> RasterImage {
    let ops = page_ops(doc, 0).unwrap();
    ops.ops
        .iter()
        .find_map(|op| match op {
            DrawOp::Image { image, .. } => Some(ops.images[*image].clone()),
            _ => None,
        })
        .expect("kein Bild auf der Seite")
}

/// Bildpunkte zurück zu Bits gepackt (hell = 1), Zeile für Zeile — so, wie sie
/// bei 1 Bit im Strom standen.
fn als_bits(bild: &RasterImage) -> Vec<u8> {
    let mut out = Vec::new();
    for zeile in bild.rgba.chunks_exact(bild.width as usize * 4) {
        for gruppe in zeile.chunks(32) {
            let mut b = 0u8;
            for (i, p) in gruppe.chunks_exact(4).enumerate() {
                if p[0] > 127 {
                    b |= 1 << (7 - i);
                }
            }
            out.push(b);
        }
    }
    out
}

fn grauwerte(bild: &RasterImage) -> Vec<u8> {
    bild.rgba.chunks_exact(4).map(|p| p[0]).collect()
}

fn treffer(daten: &[u8]) -> usize {
    daten.windows(GEHEIM.len()).filter(|w| *w == GEHEIM).count()
}

fn bitonal_png() -> Document {
    dokument(
        dictionary! {
            "Width" => 216_i64, "Height" => 64_i64,
            "ColorSpace" => "DeviceGray", "BitsPerComponent" => 1_i64,
            "Filter" => "FlateDecode",
            "DecodeParms" => dictionary! {
                "Predictor" => 15_i64, "Columns" => 216_i64,
                "Colors" => 1_i64, "BitsPerComponent" => 1_i64,
            },
        },
        deflate(&png_zeilen(GEHEIM, 0xFF)),
    )
}

/// Ein bitonaler Scan (1 Bit, PNG-Prädiktor — die gewöhnliche Ausgabe von
/// `img2pdf`) wird ungeschert gelesen: jede Geheimniszeile steht vor der
/// Schwärzung genau einmal da.
#[test]
fn ein_bitonales_png_praediktorbild_wird_ungeschert_gelesen() {
    let doc = bitonal_png();
    let bild = erstes_bild(&doc);
    assert_eq!((bild.width, bild.height), (216, 64));
    assert!(!bild.placeholder);
    assert_eq!(treffer(&als_bits(&bild)), 32);
}

/// Nach der Schwärzung der oberen Hälfte steht keine Geheimniszeile mehr in
/// den Bildpunkten der Ausgabe.
#[test]
fn die_schwaerzung_eines_bitonalen_png_praediktorbilds_trifft_das_geheimnis() {
    let mut doc = bitonal_png();
    let bericht = schwaerze_oben(&mut doc).expect("Lauf endet mit Ausgabe");
    assert_eq!(bericht.redacted_images, 1);
    let aus = load_from_bytes(&save_to_bytes(&doc).unwrap()).unwrap();
    let bild = erstes_bild(&aus);
    assert_eq!(
        treffer(&als_bits(&bild)),
        0,
        "Geheimnis steht noch in der Ausgabe; Warnungen {:?}",
        bericht.warnings
    );
    // Die untere Hälfte ist unberührt hell.
    assert!(grauwerte(&bild)[216 * 40..].iter().all(|&g| g > 127));
}

/// Ein Restbyte hinter der letzten Prädiktorzeile (Betrachter übergehen es)
/// lässt das 8-Bit-Bild nicht mehr in die Rohdaten samt Filterbytes fallen.
#[test]
fn ein_restbyte_hinter_der_letzten_png_zeile_schert_das_bild_nicht() {
    let mut roh = png_zeilen(GEHEIM, 0xFF);
    roh.push(0x0A);
    let mut doc = dokument(
        dictionary! {
            "Width" => 27_i64, "Height" => 64_i64,
            "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8_i64,
            "Filter" => "FlateDecode",
            "DecodeParms" => dictionary! { "Predictor" => 15_i64, "Columns" => 27_i64 },
        },
        deflate(&roh),
    );
    assert_eq!(treffer(&grauwerte(&erstes_bild(&doc))), 32);
    schwaerze_oben(&mut doc).expect("Lauf endet mit Ausgabe");
    let aus = load_from_bytes(&save_to_bytes(&doc).unwrap()).unwrap();
    assert_eq!(treffer(&grauwerte(&erstes_bild(&aus))), 0);
}

/// Alle fünf PNG-Zeilenfilter (None, Sub, Up, Average, Paeth) bei 8 Bit —
/// Average ist bei `lopdf::filters::png::decode_frame` falsch gerechnet.
#[test]
fn alle_png_zeilenfilter_werden_richtig_zurueckgerechnet() {
    // Zeilen mit wechselnden Werten, damit jeder Filter etwas zu tun hat.
    let zeilen: Vec<Vec<u8>> = (0..64u8)
        .map(|y| {
            (0..27u8)
                .map(|x| x.wrapping_mul(37).wrapping_add(y.wrapping_mul(11)))
                .collect()
        })
        .collect();
    let mut roh = Vec::new();
    let mut vorher = vec![0u8; 27];
    for (y, zeile) in zeilen.iter().enumerate() {
        let art = (y % 5) as u8;
        roh.push(art);
        for i in 0..27 {
            let links = if i > 0 { zeile[i - 1] } else { 0 };
            let oben = vorher[i];
            let oben_links = if i > 0 { vorher[i - 1] } else { 0 };
            let vorhersage = match art {
                0 => 0,
                1 => links,
                2 => oben,
                3 => ((u16::from(links) + u16::from(oben)) / 2) as u8,
                _ => {
                    let p = i16::from(links) + i16::from(oben) - i16::from(oben_links);
                    let (pa, pb, pc) = (
                        (p - i16::from(links)).abs(),
                        (p - i16::from(oben)).abs(),
                        (p - i16::from(oben_links)).abs(),
                    );
                    if pa <= pb && pa <= pc {
                        links
                    } else if pb <= pc {
                        oben
                    } else {
                        oben_links
                    }
                }
            };
            roh.push(zeile[i].wrapping_sub(vorhersage));
        }
        vorher.clone_from(zeile);
    }
    let doc = dokument(
        dictionary! {
            "Width" => 27_i64, "Height" => 64_i64,
            "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8_i64,
            "Filter" => "FlateDecode",
            "DecodeParms" => dictionary! { "Predictor" => 15_i64, "Columns" => 27_i64 },
        },
        deflate(&roh),
    );
    assert_eq!(grauwerte(&erstes_bild(&doc)), zeilen.concat());
}

/// Ein PNG-Zeilenfilterbyte jenseits von 0–4 ergibt keine Abtastwerte. Der
/// Lauf lehnt ab, statt die Rohdaten zu schwärzen und still auszuliefern.
#[test]
fn ein_ungueltiger_png_zeilenfilter_endet_in_einer_ablehnung() {
    let mut roh = png_zeilen(GEHEIM, 0xFF);
    roh[28 * 5] = 9;
    let mut doc = dokument(
        dictionary! {
            "Width" => 27_i64, "Height" => 64_i64,
            "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8_i64,
            "Filter" => "FlateDecode",
            "DecodeParms" => dictionary! { "Predictor" => 15_i64, "Columns" => 27_i64 },
        },
        deflate(&roh),
    );
    assert!(erstes_bild(&doc).placeholder);
    let fehler = schwaerze_oben(&mut doc).expect_err("ein ungelesenes Bild unter der Zone");
    assert!(
        fehler.to_string().contains("Zeilenfilter"),
        "Grund fehlt in der Meldung: {fehler}"
    );
}

/// TIFF-Prädiktor 2 bei 8 Bit Graustufen: jede Zeile als waagerechte
/// Differenzen, die erste Spalte absolut.
fn tiff_differenzen(zeilen: &[Vec<u8>], farben: usize) -> Vec<u8> {
    let mut roh = Vec::new();
    for zeile in zeilen {
        for (i, &wert) in zeile.iter().enumerate() {
            let links = if i >= farben { zeile[i - farben] } else { 0 };
            roh.push(wert.wrapping_sub(links));
        }
    }
    roh
}

fn tiff_grau() -> Document {
    let zeilen: Vec<Vec<u8>> = (0..64)
        .map(|y| {
            if y < 32 {
                GEHEIM.to_vec()
            } else {
                vec![0xFF; 27]
            }
        })
        .collect();
    dokument(
        dictionary! {
            "Width" => 27_i64, "Height" => 64_i64,
            "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8_i64,
            "Filter" => "FlateDecode",
            "DecodeParms" => dictionary! { "Predictor" => 2_i64, "Columns" => 27_i64 },
        },
        deflate(&tiff_differenzen(&zeilen, 1)),
    )
}

/// Der TIFF-Prädiktor wird zurückgerechnet: das Bild zeigt die Werte, nicht
/// die Differenzen.
#[test]
fn der_tiff_praediktor_2_wird_bei_8_bit_zurueckgerechnet() {
    let bild = erstes_bild(&tiff_grau());
    assert!(!bild.placeholder);
    assert_eq!(treffer(&grauwerte(&bild)), 32);
}

/// … und die Schwärzung trifft deshalb, was der Betrachter zeigt.
#[test]
fn die_schwaerzung_eines_tiff_praediktorbilds_trifft_das_geheimnis() {
    let mut doc = tiff_grau();
    let bericht = schwaerze_oben(&mut doc).expect("Lauf endet mit Ausgabe");
    assert_eq!(bericht.redacted_images, 1);
    let aus = load_from_bytes(&save_to_bytes(&doc).unwrap()).unwrap();
    let grau = grauwerte(&erstes_bild(&aus));
    assert_eq!(treffer(&grau), 0);
    assert!(grau[27 * 40..].iter().all(|&g| g == 0xFF));
}

/// TIFF-Prädiktor 2 bei RGB: die Differenz gilt je Komponente, nicht je Byte
/// des Nachbarn.
#[test]
fn der_tiff_praediktor_2_rechnet_je_komponente() {
    let zeilen: Vec<Vec<u8>> = (0..64u8)
        .map(|y| {
            (0..216u8)
                .map(|i| i.wrapping_mul(7).wrapping_add(y))
                .collect()
        })
        .collect();
    let doc = dokument(
        dictionary! {
            "Width" => 72_i64, "Height" => 64_i64,
            "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8_i64,
            "Filter" => "FlateDecode",
            "DecodeParms" => dictionary! {
                "Predictor" => 2_i64, "Columns" => 72_i64, "Colors" => 3_i64,
            },
        },
        deflate(&tiff_differenzen(&zeilen, 3)),
    );
    let bild = erstes_bild(&doc);
    let rgb: Vec<u8> = bild
        .rgba
        .chunks_exact(4)
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect();
    assert_eq!(rgb, zeilen.concat());
}

/// TIFF-Prädiktor 2 bei 1 Bit: die Differenz ist ein XOR mit dem linken
/// Bildpunkt, Zeile für Zeile neu begonnen.
#[test]
fn der_tiff_praediktor_2_wird_bei_1_bit_zurueckgerechnet() {
    let mut roh = Vec::new();
    for y in 0..64 {
        let zeile: &[u8] = if y < 32 { GEHEIM } else { &[0xFF; 27] };
        let mut links = 0u8;
        for &byte in zeile {
            let mut kodiert = 0u8;
            for bit in (0..8).rev() {
                let wert = (byte >> bit) & 1;
                kodiert |= (wert ^ links) << bit;
                links = wert;
            }
            roh.push(kodiert);
        }
    }
    let doc = dokument(
        dictionary! {
            "Width" => 216_i64, "Height" => 64_i64,
            "ColorSpace" => "DeviceGray", "BitsPerComponent" => 1_i64,
            "Filter" => "FlateDecode",
            "DecodeParms" => dictionary! {
                "Predictor" => 2_i64, "Columns" => 216_i64, "BitsPerComponent" => 1_i64,
            },
        },
        deflate(&roh),
    );
    assert_eq!(treffer(&als_bits(&erstes_bild(&doc))), 32);
}
