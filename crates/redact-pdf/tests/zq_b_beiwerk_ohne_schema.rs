//! Beiwerk **unterhalb** der Arten mit Schema (Prüfrunde „Metadaten“,
//! Spur A).
//!
//! Seit der Prüfrunde 2 beschränkt `meta.rs::restrict_to_shapes` Schrift,
//! Grafikzustand, Bild, Formular … auf die Schlüssel ihrer Normtabelle. Was
//! **darunter** ohne eigenes Schema hing, behielt jeden Schlüssel: das
//! ICC-Profil eines Farbraums, die Attribute eines `/DeviceN`, ein Halbton
//! unter `/HT` (samt seinem Normschlüssel `/HalftoneName`, frei wählbarer
//! Text), `/BS`, `/BE`, `/MK` und `/AP` einer Annotation, die Ströme eines
//! Schriftprogramms und einer CMap, ein Eintrag in `/Alternates` eines
//! Bildes. Gemessen am Binary vor der Korrektur: Schwärzung mit Rückgabewert
//! 0, „Metadaten: nichts zu entfernen“, und `--check-leaks` fand den Text an
//! drei solchen Stellen.
//!
//! Orakel ist [`redact_pdf::leaks`] an den geschriebenen Bytes. Der letzte
//! Test ist die Gegenprobe: was an denselben Stellen zeichnet, bleibt.
//!
//! `cargo test -p redact-pdf --test zq_b_beiwerk_ohne_schema`

mod common;

use common::{page, Doc, SECRET};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, strip_metadata, MetadataReport};

fn geheim() -> Object {
    Object::string_literal(format!("Kunde {SECRET}"))
}

fn lauf(d: &Doc) -> (Vec<String>, MetadataReport, Document) {
    let bytes = d.finish();
    assert!(
        !leaks(&bytes, SECRET).is_empty(),
        "die Probe muss das Geheimnis vorher tragen"
    );
    let mut doc = load_from_bytes(&bytes).expect("PDF ladbar");
    let report = strip_metadata(&mut doc);
    let out = save_to_bytes(&doc).expect("Speichern");
    let doc = load_from_bytes(&out).expect("Ausgabe ladbar");
    (leaks(&out, SECRET), report, doc)
}

#[track_caller]
fn muss_fallen(d: &Doc, was: &str) -> Document {
    let (hits, report, doc) = lauf(d);
    assert!(
        hits.is_empty(),
        "{was}: STILLES LECK — der Klartext steht nach dem Lauf in der Ausgabe.\n  \
         Bericht: {:?}\n  Fundstellen:\n{}",
        report.summary(),
        hits.join("\n")
    );
    assert!(
        report.beiwerk_removed > 0,
        "{was}: der Bericht muss die Entfernung als Beiwerk nennen: {:?}",
        report.summary()
    );
    doc
}

fn res_set(d: &mut Doc, key: &str, value: Object) {
    let id = d.resources_id;
    d.doc
        .get_dictionary_mut(id)
        .expect("Ressourcen")
        .set(key, value);
}

fn mit_inhalt(d: &mut Doc, extra: &[u8]) {
    let mut c = common::text_ops(&["Rechnung 4711"]);
    c.extend_from_slice(extra);
    d.set_content(&c);
}

fn strom(dict: Dictionary, data: &[u8]) -> Object {
    Object::Stream(Stream::new(dict, data.to_vec()).with_compression(false))
}

fn icc(d: &mut Doc, n: i64, extra: Option<Object>) -> ObjectId {
    let alternate = if n == 1 { "DeviceGray" } else { "DeviceRGB" };
    let mut dict = dictionary! { "N" => n, "Alternate" => alternate };
    if let Some(extra) = extra {
        dict.set("Kommentar", extra);
    }
    d.add(strom(dict, &[0u8; 16]))
}

fn icc_farbraum(id: ObjectId) -> Object {
    vec![Object::Name(b"ICCBased".to_vec()), Object::Reference(id)].into()
}

fn annot(d: &mut Doc, a: Dictionary) -> ObjectId {
    let id = d.add(Object::Dictionary(a));
    d.page_dict_set("Annots", vec![Object::Reference(id)].into());
    id
}

fn kaestchen(d: &mut Doc) -> ObjectId {
    d.add(strom(
        dictionary! { "Type" => "XObject", "Subtype" => "Form",
        "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()] },
        b"0 0 10 10 re f",
    ))
}

fn halbton(name: Object) -> Dictionary {
    dictionary! {
        "Type" => "Halftone", "HalftoneType" => 1, "Frequency" => 60,
        "Angle" => 45, "SpotFunction" => "Round", "HalftoneName" => name,
    }
}

fn dict_at(doc: &Document, id: ObjectId) -> &Dictionary {
    match doc.get_object(id).expect("Objekt") {
        Object::Dictionary(d) => d,
        Object::Stream(s) => &s.dict,
        other => panic!("kein Dictionary: {other:?}"),
    }
}

fn sub<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> &'a Dictionary {
    let value = dict.get(key).expect("Schlüssel");
    let (_, value) = doc.dereference(value).expect("auflösbar");
    match value {
        Object::Dictionary(d) => d,
        Object::Stream(s) => &s.dict,
        other => panic!("kein Dictionary unter {key:?}: {other:?}"),
    }
}

// ===========================================================================
// Farbräume
// ===========================================================================

#[test]
fn a_foreign_key_on_an_icc_profile_stream_is_removed() {
    let mut d = page(&["Rechnung 4711"]);
    let id = icc(&mut d, 1, Some(geheim()));
    res_set(
        &mut d,
        "ColorSpace",
        Object::Dictionary(dictionary! { "CS0" => icc_farbraum(id) }),
    );
    mit_inhalt(&mut d, b"/CS0 cs 0.5 sc 10 10 50 50 re f\n");
    muss_fallen(&d, "ICCBased-Strom unter /Resources /ColorSpace");
}

#[test]
fn a_foreign_key_on_the_icc_profile_of_a_page_group_is_removed() {
    let mut d = page(&["Rechnung 4711"]);
    let id = icc(&mut d, 3, Some(geheim()));
    d.page_dict_set(
        "Group",
        Object::Dictionary(dictionary! {
            "Type" => "Group", "S" => "Transparency", "CS" => icc_farbraum(id),
        }),
    );
    muss_fallen(&d, "ICCBased-Strom unter der Seitengruppe /CS");
}

#[test]
fn an_icc_profile_behind_a_separation_alternate_is_reached() {
    let mut d = page(&["Rechnung 4711"]);
    let id = icc(&mut d, 1, Some(geheim()));
    let tint = d.add(Object::Dictionary(dictionary! {
        "FunctionType" => 2, "Domain" => vec![0.into(), 1.into()], "N" => 1,
    }));
    res_set(
        &mut d,
        "ColorSpace",
        Object::Dictionary(dictionary! {
            "CS0" => vec![
                Object::Name(b"Separation".to_vec()),
                Object::Name(b"Spot".to_vec()),
                icc_farbraum(id),
                Object::Reference(tint),
            ],
        }),
    );
    mit_inhalt(&mut d, b"/CS0 cs 0.5 sc 10 10 50 50 re f\n");
    muss_fallen(&d, "ICC-Profil als Alternativraum einer Separation");
}

#[test]
fn a_foreign_key_in_devicen_attributes_is_removed() {
    let mut d = page(&["Rechnung 4711"]);
    let tint = d.add(strom(
        dictionary! { "FunctionType" => 4, "Domain" => vec![0.into(), 1.into()],
        "Range" => vec![0.into(), 1.into()] },
        b"{ }",
    ));
    res_set(
        &mut d,
        "ColorSpace",
        Object::Dictionary(dictionary! {
            "CS0" => vec![
                Object::Name(b"DeviceN".to_vec()),
                vec![Object::Name(b"Spot".to_vec())].into(),
                Object::Name(b"DeviceGray".to_vec()),
                Object::Reference(tint),
                Object::Dictionary(dictionary! { "Subtype" => "DeviceN", "Zusatz" => geheim() }),
            ],
        }),
    );
    mit_inhalt(&mut d, b"/CS0 cs 0.5 sc 10 10 50 50 re f\n");
    muss_fallen(&d, "DeviceN-Attribute");
}

#[test]
fn a_foreign_key_on_the_icc_profile_of_an_image_is_removed() {
    let mut d = page(&["Rechnung 4711"]);
    let id = icc(&mut d, 1, Some(geheim()));
    let bild = d.add(strom(
        dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => 1, "Height" => 1,
        "ColorSpace" => icc_farbraum(id), "BitsPerComponent" => 8 },
        &[0x80],
    ));
    res_set(
        &mut d,
        "XObject",
        Object::Dictionary(dictionary! { "Im0" => bild }),
    );
    mit_inhalt(&mut d, b"q 10 0 0 10 400 400 cm /Im0 Do Q\n");
    muss_fallen(&d, "ICC-Profil im /ColorSpace eines Bildes");
}

// ===========================================================================
// Halbtöne
// ===========================================================================

fn mit_halbton(d: &mut Doc, ht: Object) {
    res_set(
        d,
        "ExtGState",
        Object::Dictionary(dictionary! {
            "GS0" => dictionary! { "Type" => "ExtGState", "HT" => ht },
        }),
    );
    mit_inhalt(d, b"/GS0 gs\n");
}

#[test]
fn the_name_of_a_halftone_is_removed() {
    let mut d = page(&["Rechnung 4711"]);
    mit_halbton(&mut d, Object::Dictionary(halbton(geheim())));
    muss_fallen(&d, "/HalftoneName unter /HT");
}

#[test]
fn a_foreign_key_on_an_indirect_halftone_without_type_is_removed() {
    let mut d = page(&["Rechnung 4711"]);
    let ht = d.add(Object::Dictionary(dictionary! {
        "HalftoneType" => 1, "Frequency" => 60, "Angle" => 45,
        "SpotFunction" => "Round", "Zusatz" => geheim(),
    }));
    mit_halbton(&mut d, Object::Reference(ht));
    muss_fallen(
        &d,
        "fremder Schlüssel an einem indirekten Halbton ohne /Type",
    );
}

#[test]
fn a_type5_halftone_loses_its_text_and_keeps_its_colorants() {
    let mut d = page(&["Rechnung 4711"]);
    let cyan = d.add(Object::Dictionary(halbton(geheim())));
    mit_halbton(
        &mut d,
        Object::Dictionary(dictionary! {
            "Type" => "Halftone", "HalftoneType" => 5,
            "HalftoneName" => geheim(),
            "Zusatz" => geheim(),
            "Cyan" => cyan,
            "Default" => halbton(Object::string_literal("x")),
        }),
    );
    let doc = muss_fallen(&d, "Halbton vom Typ 5");
    let res = dict_at(&doc, d.resources_id);
    let gs = sub(&doc, sub(&doc, res, b"ExtGState"), b"GS0");
    let ht = sub(&doc, gs, b"HT");
    assert!(
        ht.get(b"Cyan").is_ok() && ht.get(b"Default").is_ok(),
        "{ht:?}"
    );
    assert!(ht.get(b"HalftoneType").is_ok(), "{ht:?}");
    assert!(sub(&doc, ht, b"Cyan").get(b"Frequency").is_ok());
    assert!(sub(&doc, ht, b"Default").get(b"HalftoneName").is_err());
}

// ===========================================================================
// Unterverzeichnisse einer Annotation
// ===========================================================================

#[test]
fn a_foreign_key_under_the_border_style_of_an_annotation_is_removed() {
    let mut d = page(&["Rechnung 4711"]);
    annot(
        &mut d,
        dictionary! {
            "Type" => "Annot", "Subtype" => "Square",
            "Rect" => vec![300.into(), 300.into(), 400.into(), 400.into()],
            "BS" => dictionary! { "W" => 1, "S" => "S", "Zusatz" => geheim() },
            "BE" => dictionary! { "S" => "C", "Zusatz" => geheim() },
        },
    );
    muss_fallen(&d, "Annotation /BS und /BE");
}

#[test]
fn a_foreign_key_under_mk_of_a_widget_without_type_is_removed() {
    let mut d = page(&["Rechnung 4711"]);
    annot(
        &mut d,
        dictionary! {
            "Subtype" => "Widget", "FT" => "Btn",
            "Rect" => vec![300.into(), 300.into(), 400.into(), 400.into()],
            "MK" => dictionary! {
                "BG" => vec![1.into()],
                "IF" => dictionary! { "SW" => "A", "Zusatz" => geheim() },
                "Zusatz" => geheim(),
            },
        },
    );
    muss_fallen(&d, "Widget /MK und /MK /IF");
}

#[test]
fn a_string_among_the_appearance_states_is_removed() {
    let mut d = page(&["Rechnung 4711"]);
    let on = kaestchen(&mut d);
    let text = d.add(geheim());
    annot(
        &mut d,
        dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "FT" => "Btn",
            "Rect" => vec![300.into(), 300.into(), 310.into(), 310.into()],
            "AS" => "On",
            "AP" => dictionary! {
                "N" => dictionary! { "On" => on, "Zusatz" => geheim() },
                "D" => dictionary! { "On" => on, "Verweis" => text },
            },
        },
    );
    muss_fallen(&d, "Zustandsverzeichnis unter /AP (direkt und als Verweis)");
}

#[test]
fn a_foreign_key_under_ap_is_removed() {
    let mut d = page(&["Rechnung 4711"]);
    let on = kaestchen(&mut d);
    annot(
        &mut d,
        dictionary! {
            "Type" => "Annot", "Subtype" => "Square",
            "Rect" => vec![300.into(), 300.into(), 310.into(), 310.into()],
            "AP" => dictionary! { "N" => on, "Zusatz" => geheim() },
        },
    );
    muss_fallen(&d, "fremder Schlüssel unter /AP");
}

#[test]
fn an_untyped_appearance_stream_is_held_to_the_form_schema() {
    let mut d = page(&["Rechnung 4711"]);
    let on = d.add(strom(
        dictionary! { "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()],
        "Zusatz" => geheim() },
        b"0 0 10 10 re f",
    ));
    annot(
        &mut d,
        dictionary! {
            "Type" => "Annot", "Subtype" => "Square",
            "Rect" => vec![300.into(), 300.into(), 310.into(), 310.into()],
            "AP" => dictionary! { "N" => on },
        },
    );
    let doc = muss_fallen(&d, "Erscheinungsstrom ohne /Subtype");
    assert!(dict_at(&doc, on).get(b"BBox").is_ok(), "die /BBox bleibt");
}

// ===========================================================================
// Schriftströme und Bildalternativen
// ===========================================================================

#[test]
fn a_foreign_key_on_a_font_program_stream_is_removed() {
    let mut d = page(&["Rechnung 4711"]);
    let ff = d.add(strom(
        dictionary! { "Length1" => 4, "Zusatz" => geheim() },
        b"\0\0\0\0",
    ));
    let fid = d.font_id;
    let fd = d.add(Object::Dictionary(dictionary! {
        "Type" => "FontDescriptor", "FontName" => "Helvetica", "Flags" => 32,
        "FontFile2" => ff,
    }));
    d.doc
        .get_dictionary_mut(fid)
        .expect("Schrift")
        .set("FontDescriptor", fd);
    let doc = muss_fallen(&d, "/FontFile2-Strom");
    assert!(dict_at(&doc, ff).get(b"Length1").is_ok(), "/Length1 bleibt");
}

#[test]
fn a_foreign_key_on_a_to_unicode_stream_is_removed() {
    let mut d = page(&["Rechnung 4711"]);
    let tu = d.add(strom(
        dictionary! { "Zusatz" => geheim() },
        b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
          1 begincodespacerange <00> <FF> endcodespacerange\nendcmap end end\n",
    ));
    let fid = d.font_id;
    d.doc
        .get_dictionary_mut(fid)
        .expect("Schrift")
        .set("ToUnicode", tu);
    muss_fallen(&d, "/ToUnicode-Strom");
}

#[test]
fn a_foreign_key_on_the_encoding_cmap_of_a_type0_font_is_removed() {
    let mut d = page(&["Rechnung 4711"]);
    let cmap = d.add(strom(
        dictionary! { "Type" => "CMap", "CMapName" => "Eigen", "Zusatz" => geheim(),
        "CIDSystemInfo" => dictionary! { "Registry" => Object::string_literal("Adobe"),
            "Ordering" => Object::string_literal("Identity"), "Supplement" => 0,
            "Zusatz" => geheim() } },
        b"begincmap endcmap",
    ));
    let cid = d.add(Object::Dictionary(dictionary! {
        "Type" => "Font", "Subtype" => "CIDFontType2", "BaseFont" => "Eigen",
        "CIDSystemInfo" => dictionary! { "Registry" => Object::string_literal("Adobe"),
            "Ordering" => Object::string_literal("Identity"), "Supplement" => 0 },
    }));
    let t0 = d.add(Object::Dictionary(dictionary! {
        "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "Eigen",
        "Encoding" => cmap, "DescendantFonts" => vec![Object::Reference(cid)],
    }));
    let f1 = d.font_id;
    res_set(
        &mut d,
        "Font",
        Object::Dictionary(dictionary! { "F1" => f1, "F2" => t0 }),
    );
    let doc = muss_fallen(&d, "/Encoding-CMap einer Type0-Schrift");
    assert!(
        dict_at(&doc, cmap).get(b"CMapName").is_ok(),
        "/CMapName bleibt"
    );
}

#[test]
fn a_foreign_key_in_an_alternates_entry_of_an_image_is_removed() {
    let mut d = page(&["Rechnung 4711"]);
    let bild = |doc: &mut Doc| {
        doc.add(strom(
            dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => 1,
            "Height" => 1, "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8 },
            &[0x80],
        ))
    };
    let alt = bild(&mut d);
    let haupt = bild(&mut d);
    d.doc
        .get_object_mut(haupt)
        .and_then(Object::as_stream_mut)
        .expect("Bild")
        .dict
        .set(
            "Alternates",
            vec![Object::Dictionary(
                dictionary! { "Image" => alt, "DefaultForPrinting" => true, "Zusatz" => geheim() },
            )],
        );
    res_set(
        &mut d,
        "XObject",
        Object::Dictionary(dictionary! { "Im0" => haupt }),
    );
    mit_inhalt(&mut d, b"q 10 0 0 10 400 400 cm /Im0 Do Q\n");
    let doc = muss_fallen(&d, "Eintrag in /Alternates");
    assert!(doc.get_object(alt).is_ok(), "das Alternativbild bleibt");
}

// ===========================================================================
// Gegenprobe: was zeichnet, bleibt
// ===========================================================================

/// Eine Datei mit den Stellen oben, aber ohne einen einzigen fremden
/// Schlüssel: der Lauf nimmt nichts als Beiwerk, und jeder zeichnende
/// Schlüssel steht noch da.
#[test]
fn nothing_that_draws_is_removed_at_these_places() {
    let mut d = page(&["Rechnung 4711"]);
    let icc_id = icc(&mut d, 3, None);
    let tint = d.add(Object::Dictionary(dictionary! {
        "FunctionType" => 2, "Domain" => vec![0.into(), 1.into()], "N" => 1,
    }));
    let lookup = d.add(strom(dictionary! {}, &[0, 0, 0, 255, 255, 255]));
    let separation: Object = vec![
        Object::Name(b"Separation".to_vec()),
        Object::Name(b"Spot".to_vec()),
        Object::Name(b"DeviceGray".to_vec()),
        Object::Reference(tint),
    ]
    .into();
    res_set(
        &mut d,
        "ColorSpace",
        Object::Dictionary(dictionary! {
            "CS0" => icc_farbraum(icc_id),
            "CS1" => vec![
                Object::Name(b"DeviceN".to_vec()),
                vec![Object::Name(b"Spot".to_vec())].into(),
                icc_farbraum(icc_id),
                Object::Reference(tint),
                Object::Dictionary(dictionary! {
                    "Subtype" => "NChannel",
                    "Colorants" => dictionary! { "Spot" => separation },
                    "Process" => dictionary! { "ColorSpace" => "DeviceCMYK",
                        "Components" => vec![Object::Name(b"Cyan".to_vec())] },
                }),
            ],
            "CS2" => vec![
                Object::Name(b"Indexed".to_vec()),
                Object::Name(b"DeviceGray".to_vec()),
                1.into(),
                Object::Reference(lookup),
            ],
            "CS3" => vec![
                Object::Name(b"CalRGB".to_vec()),
                Object::Dictionary(dictionary! {
                    "WhitePoint" => vec![1.into(), 1.into(), 1.into()],
                    "Gamma" => vec![2.into(), 2.into(), 2.into()],
                }),
            ],
        }),
    );
    res_set(
        &mut d,
        "ExtGState",
        Object::Dictionary(dictionary! {
            "GS0" => dictionary! { "HT" => dictionary! {
                "Type" => "Halftone", "HalftoneType" => 5,
                "Default" => dictionary! { "HalftoneType" => 1, "Frequency" => 60,
                    "Angle" => 45, "SpotFunction" => "Round", "AccurateScreens" => true },
            } },
        }),
    );
    let on = kaestchen(&mut d);
    let off = kaestchen(&mut d);
    let widget = annot(
        &mut d,
        dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "FT" => "Btn",
            "Rect" => vec![300.into(), 300.into(), 310.into(), 310.into()],
            "AS" => "On",
            "BS" => dictionary! { "Type" => "Border", "W" => 2, "S" => "D",
                "D" => vec![3.into()] },
            "MK" => dictionary! { "BG" => vec![1.into()], "BC" => vec![0.into()],
                "IF" => dictionary! { "SW" => "A", "S" => "P" } },
            "AP" => dictionary! { "N" => dictionary! { "On" => on, "Off" => off },
                "D" => on },
        },
    );
    mit_inhalt(
        &mut d,
        b"/CS0 cs 0.5 0.5 0.5 sc /CS1 cs 1 sc /GS0 gs 10 10 50 50 re f\n",
    );

    let bytes = d.finish();
    let mut doc = load_from_bytes(&bytes).expect("PDF ladbar");
    let report = strip_metadata(&mut doc);
    assert_eq!(report.beiwerk_removed, 0, "{:?}", report.summary());
    let out = save_to_bytes(&doc).expect("Speichern");
    let doc = load_from_bytes(&out).expect("Ausgabe ladbar");

    let icc = dict_at(&doc, icc_id);
    for key in [&b"N"[..], b"Alternate", b"Length"] {
        assert!(icc.get(key).is_ok(), "ICC-Profil verlor {key:?}");
    }
    assert!(doc.get_object(lookup).is_ok(), "die Farbtabelle bleibt");
    let res = dict_at(&doc, d.resources_id);
    let spaces = sub(&doc, res, b"ColorSpace");
    let Ok(Object::Array(devicen)) = spaces.get(b"CS1") else {
        panic!("CS1");
    };
    let Object::Dictionary(attrs) = &devicen[4] else {
        panic!("Attribute");
    };
    for key in [&b"Subtype"[..], b"Colorants", b"Process"] {
        assert!(attrs.get(key).is_ok(), "DeviceN-Attribute verloren {key:?}");
    }
    let ht = sub(&doc, sub(&doc, sub(&doc, res, b"ExtGState"), b"GS0"), b"HT");
    let default = sub(&doc, ht, b"Default");
    for key in [
        &b"Frequency"[..],
        b"Angle",
        b"SpotFunction",
        b"AccurateScreens",
    ] {
        assert!(default.get(key).is_ok(), "Halbton verlor {key:?}");
    }
    let w = dict_at(&doc, widget);
    assert!(sub(&doc, w, b"BS").get(b"D").is_ok());
    assert!(sub(&doc, w, b"MK").get(b"BG").is_ok());
    assert!(sub(&doc, sub(&doc, w, b"MK"), b"IF").get(b"S").is_ok());
    let ap = sub(&doc, w, b"AP");
    let states = sub(&doc, ap, b"N");
    assert!(
        states.get(b"On").is_ok() && states.get(b"Off").is_ok(),
        "{states:?}"
    );
    assert!(ap.get(b"D").is_ok());
    assert!(dict_at(&doc, on).get(b"BBox").is_ok());
}
