//! Gegenprüfung Fix-Runde 6 (R3): das **Beiwerk** einer Annotation, das
//! Klartext trägt, nachdem `/Movie`, `/Measure` und `/RichMediaContent` als
//! ganze Schlüssel fallen.
//!
//! Zwei Richtungen:
//!
//! * **Gegenrichtung** — eine Movie-Annotation ohne `/Movie` ist regelwidrig
//!   (Pflichtschlüssel). Die Ausgabe muss trotzdem laden, ihren Seitentext
//!   behalten, und eine Vermessung behält ihr `/AP` (dort steht die
//!   Beschriftung, die gezeichnet wird).
//! * **Weitere Träger** — was noch neben einer Annotation Klartext führt und
//!   den Lauf übersteht: ein Dateianhang, den sein eigenes `/Popup` (über
//!   `/Parent`) oder eine Antwort (`/IRT`) am Leben hält; eine zugeordnete
//!   Datei (`/AF`, PDF 2.0, 14.13 — der ZUGFeRD-Weg); XMP an einem
//!   Bild-XObject; `/PieceInfo` an einem Form-XObject; `/3DD /OnInstantiate`
//!   und `/3DV /XN` an einer 3D-Annotation; `/RO` einer Redact-Annotation.
//!
//! Orakel: `leaks` an den geschriebenen Bytes; dazu der `MetadataReport`, ob
//! er die Entfernung behauptet, die nicht stattfand.
//!
//! Die Proben, die einen offenen Befund zeigen, sind `#[ignore]` und rot —
//! sie sind der Beleg, keine Zusicherung (Muster wie `zf_q1_luecken`).

mod common;

use common::{page, Doc, SECRET};
use lopdf::{dictionary, Object, ObjectId, Stream};
use redact_pdf::{
    leaks, load_from_bytes, save_to_bytes, strip_metadata, MetadataReport, PdfExtractor,
};

fn strip(bytes: &[u8]) -> (MetadataReport, Vec<u8>) {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    let report = strip_metadata(&mut doc);
    (report, save_to_bytes(&doc).expect("Speichern"))
}

fn text_of(bytes: &[u8]) -> String {
    let doc = load_from_bytes(bytes).expect("ladbar");
    PdfExtractor::new()
        .extract(&doc)
        .expect("Text lesbar")
        .iter()
        .map(|r| r.text.clone())
        .collect::<Vec<_>>()
        .join("|")
}

/// Die Probe trägt das Geheimnis vorher; nach dem Lauf steht es nicht mehr
/// in der Datei.
#[track_caller]
fn muss_fallen(bytes: &[u8], was: &str) -> MetadataReport {
    assert!(
        !leaks(bytes, SECRET).is_empty(),
        "{was}: die Probe muss das Geheimnis vorher tragen"
    );
    let (report, out) = strip(bytes);
    let hits = leaks(&out, SECRET);
    assert!(
        hits.is_empty(),
        "{was}: nach dem Metadatenlauf steht der Klartext noch in der Datei \
         (Bericht: {:?}):\n{}",
        report.summary(),
        hits.join("\n")
    );
    report
}

/// Ein Dateianhang als Annotation: Stream, Filespec, Annotation.
fn dateianhang(d: &mut Doc) -> (ObjectId, ObjectId) {
    let strom = d.add(Object::Stream(Stream::new(
        dictionary! { "Type" => "EmbeddedFile" },
        format!("Kontoauszug\nIBAN {SECRET}\n").into_bytes(),
    )));
    let filespec = d.add(Object::Dictionary(dictionary! {
        "Type" => "Filespec",
        "F" => Object::string_literal("kontoauszug.txt"),
        "UF" => Object::string_literal("kontoauszug.txt"),
        "EF" => dictionary! { "F" => Object::Reference(strom) },
    }));
    let a = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "FileAttachment",
        "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
        "FS" => Object::Reference(filespec),
        "Contents" => Object::string_literal("Anhang"),
        "T" => Object::string_literal("Max"),
        "Name" => "Paperclip",
    }));
    (a, filespec)
}

// ---------------------------------------------------------------------------
// A) Gegenrichtung: der Torso lädt, der Seitentext bleibt, das /AP bleibt
// ---------------------------------------------------------------------------

/// Eine Movie-Annotation ohne `/Movie` — die Ausgabe lädt, der Seitentext
/// ist unverändert, die Annotation steht noch (ohne ihr Beiwerk).
#[test]
fn movie_annotation_ohne_movie_laedt_und_behaelt_den_seitentext() {
    let mut d: Doc = page(&["Rechnung 4711 ueber 120,00 EUR"]);
    let a = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Movie",
        "Rect" => vec![10.into(), 10.into(), 130.into(), 100.into()],
        "T" => Object::string_literal("Film"),
        "Movie" => dictionary! {
            "F" => Object::string_literal(format!("Kontoauszug {SECRET}.mov")),
            "Aspect" => vec![320.into(), 240.into()],
        },
        "A" => dictionary! { "ShowControls" => true },
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(a)]));
    let vorher = d.finish();
    let report = muss_fallen(&vorher, "/Movie");
    let (_, out) = strip(&vorher);
    assert_eq!(text_of(&out), text_of(&vorher), "Seitentext unverändert");
    let doc = load_from_bytes(&out).expect("Ausgabe lädt");
    assert_eq!(doc.get_pages().len(), 1);
    let annot = doc.get_dictionary(a).expect("Annotation steht noch");
    assert!(annot.get(b"Movie").is_err(), "/Movie ist gefallen");
    assert_eq!(annot.get(b"Subtype").unwrap().as_name().unwrap(), b"Movie");
    // Der Bericht nennt die Entfernung: /Movie und /T sind zwei Klartexte.
    assert_eq!(report.annotation_texts_cleared, 2, "{:?}", report.summary());
}

/// Eine Vermessung (PolyLine mit `/Measure`) behält ihr `/AP` — die
/// gezeichnete Beschriftung — bytegleich; nur `/Measure` fällt.
#[test]
fn vermessung_behaelt_ihr_ap_bytegleich() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let font_id = d.font_id;
    let ap_bytes = b"BT /F1 8 Tf 2 2 Td (12,5 cm) Tj ET\n".to_vec();
    let ap = d.add(Object::Stream(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 120.into(), 20.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => Object::Reference(font_id) } },
        },
        ap_bytes.clone(),
    )));
    let a = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "PolyLine",
        "Rect" => vec![10.into(), 10.into(), 130.into(), 30.into()],
        "Vertices" => vec![10.into(), 10.into(), 130.into(), 30.into()],
        "AP" => dictionary! { "N" => Object::Reference(ap) },
        "Measure" => dictionary! {
            "Type" => "Measure",
            "R" => Object::string_literal(format!("1 zu {SECRET}")),
            "X" => Object::Array(vec![Object::Dictionary(dictionary! {
                "U" => Object::string_literal("cm"),
                "C" => Object::Real(1.0),
            })]),
        },
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(a)]));
    let vorher = d.finish();
    muss_fallen(&vorher, "/Measure /R");
    let (_, out) = strip(&vorher);
    let doc = load_from_bytes(&out).expect("Ausgabe lädt");
    let annot = doc.get_dictionary(a).expect("Annotation steht noch");
    assert!(annot.get(b"Measure").is_err(), "/Measure ist gefallen");
    let ap_out = doc
        .get_object(ap)
        .and_then(|o| o.as_stream())
        .expect("/AP-Strom steht noch");
    assert_eq!(ap_out.content, ap_bytes, "/AP bytegleich");
    assert!(
        !leaks(&out, "12,5 cm").is_empty(),
        "die Beschriftung im /AP bleibt (sie wird gezeichnet)"
    );
}

// ---------------------------------------------------------------------------
// B) Weitere Träger mit Klartext
// ---------------------------------------------------------------------------

/// Ein Dateianhang mit **Popup** — so schreibt Acrobat jeden Kommentar. Das
/// Popup steht in `/Annots` und hält über `/Parent` die Annotation samt
/// `/FS` und eingebetteter Datei am Leben.
#[test]
#[ignore = "Befund R3: rot bis zur Korrektur"]
fn dateianhang_mit_popup_verliert_seine_datei() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let (a, _) = dateianhang(&mut d);
    let popup = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Popup",
        "Rect" => vec![100.into(), 100.into(), 300.into(), 200.into()],
        "Parent" => Object::Reference(a),
        "Open" => false,
    }));
    d.doc
        .get_dictionary_mut(a)
        .unwrap()
        .set("Popup", Object::Reference(popup));
    d.page_dict_set(
        "Annots",
        Object::Array(vec![Object::Reference(a), Object::Reference(popup)]),
    );
    muss_fallen(&d.finish(), "/FileAttachment mit /Popup");
}

/// Ein Dateianhang mit **Antwort** (`/IRT`) — „Antworten“ auf einen
/// Dateianhang-Kommentar. Die Antwort hält die Annotation samt Datei.
#[test]
#[ignore = "Befund R3: rot bis zur Korrektur"]
fn dateianhang_mit_antwort_verliert_seine_datei() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let (a, _) = dateianhang(&mut d);
    let antwort = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Text",
        "Rect" => vec![40.into(), 10.into(), 60.into(), 30.into()],
        "IRT" => Object::Reference(a),
        "Contents" => Object::string_literal("Antwort"),
    }));
    d.page_dict_set(
        "Annots",
        Object::Array(vec![Object::Reference(a), Object::Reference(antwort)]),
    );
    muss_fallen(&d.finish(), "/FileAttachment mit /IRT-Antwort");
}

/// Ein Dateianhang, den nichts anderes hält, fällt samt Datei — und wird
/// gezählt. (Gegenprobe zu den beiden Fällen darüber.)
#[test]
fn dateianhang_ohne_zweiten_halter_faellt_samt_datei() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let (a, _) = dateianhang(&mut d);
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(a)]));
    let report = muss_fallen(&d.finish(), "/FileAttachment allein");
    assert_eq!(report.file_attachments_removed, 1);
}

/// Der Bericht darf für die beiden gehaltenen Anhänge keine Entfernung
/// melden — das hält er. (Die Datei bleibt trotzdem: siehe die rot
/// markierten Proben.)
#[test]
fn gehaltener_dateianhang_wird_nicht_als_entfernt_gemeldet() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let (a, filespec) = dateianhang(&mut d);
    let popup = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Popup",
        "Rect" => vec![100.into(), 100.into(), 300.into(), 200.into()],
        "Parent" => Object::Reference(a),
    }));
    d.page_dict_set(
        "Annots",
        Object::Array(vec![Object::Reference(a), Object::Reference(popup)]),
    );
    let (report, out) = strip(&d.finish());
    assert_eq!(report.file_attachments_removed, 0, "{:?}", report.summary());
    let doc = load_from_bytes(&out).expect("Ausgabe lädt");
    assert!(
        doc.objects.contains_key(&a),
        "die Annotation steht noch (Popup /Parent)"
    );
    assert!(
        doc.objects.contains_key(&filespec),
        "der Filespec steht noch"
    );
}

/// Eine **zugeordnete Datei** am Katalog (`/AF`, PDF 2.0, 14.13) — genau so
/// hängt eine ZUGFeRD-/Factur-X-Rechnung ihre XML-Fassung an, zusätzlich zu
/// `/Names /EmbeddedFiles`. `/Names` fällt, `/AF` hält die Datei.
#[test]
#[ignore = "Befund R3: rot bis zur Korrektur"]
fn zugeordnete_datei_am_katalog_bleibt() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let strom = d.add(Object::Stream(Stream::new(
        dictionary! { "Type" => "EmbeddedFile", "Subtype" => "text/xml" },
        format!("<rsm:CrossIndustryInvoice><ram:IBANID>{SECRET}</ram:IBANID></rsm:CrossIndustryInvoice>").into_bytes(),
    )));
    let filespec = d.add(Object::Dictionary(dictionary! {
        "Type" => "Filespec",
        "F" => Object::string_literal("factur-x.xml"),
        "UF" => Object::string_literal("factur-x.xml"),
        "AFRelationship" => "Alternative",
        "EF" => dictionary! { "F" => Object::Reference(strom) },
    }));
    let blatt = d.add(Object::Dictionary(dictionary! {
        "Names" => vec![Object::string_literal("factur-x.xml"), Object::Reference(filespec)],
    }));
    let names = d.add(Object::Dictionary(
        dictionary! { "EmbeddedFiles" => Object::Reference(blatt) },
    ));
    d.catalog_set("Names", Object::Reference(names));
    d.catalog_set("AF", Object::Array(vec![Object::Reference(filespec)]));
    muss_fallen(&d.finish(), "/AF am Katalog");
}

/// Dieselbe zugeordnete Datei an der **Seite** und an einer **Annotation**.
#[test]
#[ignore = "Befund R3: rot bis zur Korrektur"]
fn zugeordnete_datei_an_seite_und_annotation_bleibt() {
    for ort in ["Seite", "Annotation"] {
        let mut d: Doc = page(&["Rechnung 4711"]);
        let strom = d.add(Object::Stream(Stream::new(
            dictionary! { "Type" => "EmbeddedFile" },
            format!("IBAN {SECRET}").into_bytes(),
        )));
        let filespec = d.add(Object::Dictionary(dictionary! {
            "Type" => "Filespec",
            "F" => Object::string_literal("daten.txt"),
            "EF" => dictionary! { "F" => Object::Reference(strom) },
        }));
        match ort {
            "Seite" => d.page_dict_set("AF", Object::Array(vec![Object::Reference(filespec)])),
            _ => {
                let a = d.add(Object::Dictionary(dictionary! {
                    "Type" => "Annot",
                    "Subtype" => "Text",
                    "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
                    "Contents" => Object::string_literal("Notiz"),
                    "AF" => Object::Array(vec![Object::Reference(filespec)]),
                }));
                d.page_dict_set("Annots", Object::Array(vec![Object::Reference(a)]));
            }
        }
        muss_fallen(&d.finish(), &format!("/AF an der {ort}"));
    }
}

/// XMP an einem **Bild-XObject** — so exportiert InDesign platzierte Fotos
/// mit ihren Metadaten (`dc:description`, Kamerabesitzer, Standort).
#[test]
#[ignore = "Befund R3: rot bis zur Korrektur"]
fn xmp_an_einem_bild_xobject_bleibt() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let xmp = d.add(Object::Stream(Stream::new(
        dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
        format!("<x:xmpmeta><dc:description>Kontoauszug {SECRET}</dc:description></x:xmpmeta>")
            .into_bytes(),
    )));
    let bild = d.add(Object::Stream(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 1,
            "Height" => 1,
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 8,
            "Metadata" => Object::Reference(xmp),
        },
        vec![0x80],
    )));
    let resources_id = d.resources_id;
    d.doc
        .get_dictionary_mut(resources_id)
        .unwrap()
        .set("XObject", dictionary! { "Im1" => Object::Reference(bild) });
    muss_fallen(&d.finish(), "/Metadata an einem Bild-XObject");
}

/// `/PieceInfo` an einem **Form-XObject** (Tabelle 95) — Illustrator legt
/// dort seine privaten Daten ab.
#[test]
#[ignore = "Befund R3: rot bis zur Korrektur"]
fn pieceinfo_an_einem_form_xobject_bleibt() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let form = d.add(Object::Stream(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()],
            "PieceInfo" => dictionary! {
                "Illustrator" => dictionary! {
                    "LastModified" => Object::string_literal("D:20240101"),
                    "Private" => Object::string_literal(format!("Kontoauszug {SECRET}")),
                },
            },
        },
        b"0 0 10 10 re f\n".to_vec(),
    )));
    let resources_id = d.resources_id;
    d.doc
        .get_dictionary_mut(resources_id)
        .unwrap()
        .set("XObject", dictionary! { "Fm1" => Object::Reference(form) });
    muss_fallen(&d.finish(), "/PieceInfo an einem Form-XObject");
}

/// Eine 3D-Annotation: `/3DD` ist ein Strom mit `/OnInstantiate` (ein
/// JavaScript-Strom, Tabelle 300) und `/3DV` eine Ansicht mit `/XN`, dem
/// frei wählbaren Anzeigenamen (Tabelle 304).
#[test]
#[ignore = "Befund R3: rot bis zur Korrektur"]
fn dreid_annotation_behaelt_javascript_und_ansichtsnamen() {
    for (was, key) in [("JavaScript", "js"), ("Ansichtsname", "xn")] {
        let mut d: Doc = page(&["Rechnung 4711"]);
        let js = d.add(Object::Stream(Stream::new(
            dictionary! {},
            if key == "js" {
                format!("app.alert(\"{SECRET}\");")
            } else {
                "1;".to_string()
            }
            .into_bytes(),
        )));
        let daten = d.add(Object::Stream(Stream::new(
            dictionary! {
                "Type" => "3D",
                "Subtype" => "U3D",
                "OnInstantiate" => Object::Reference(js),
            },
            b"U3D\0\0\0\0".to_vec(),
        )));
        let a = d.add(Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "3D",
            "Rect" => vec![10.into(), 10.into(), 130.into(), 100.into()],
            "3DD" => Object::Reference(daten),
            "3DV" => dictionary! {
                "Type" => "3DView",
                "XN" => Object::string_literal(if key == "xn" { format!("Ansicht {SECRET}") } else { "Ansicht".into() }),
                "IN" => Object::string_literal("v1"),
                "MS" => "M",
            },
        }));
        d.page_dict_set("Annots", Object::Array(vec![Object::Reference(a)]));
        muss_fallen(&d.finish(), &format!("/3D {was}"));
    }
}

/// Eine Redact-Annotation: `/OverlayText` fällt schon; `/RO` ist ein
/// Form-XObject, das über die Stelle gelegt wird — mit Text darin.
#[test]
#[ignore = "Befund R3: rot bis zur Korrektur"]
fn redact_annotation_behaelt_ihr_ro() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let font_id = d.font_id;
    let ro = d.add(Object::Stream(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 200.into(), 20.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => Object::Reference(font_id) } },
        },
        format!("BT /F1 8 Tf 2 2 Td ({SECRET}) Tj ET\n").into_bytes(),
    )));
    let a = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Redact",
        "Rect" => vec![10.into(), 10.into(), 210.into(), 30.into()],
        "QuadPoints" => vec![10.into(), 30.into(), 210.into(), 30.into(), 10.into(), 10.into(), 210.into(), 10.into()],
        "OverlayText" => Object::string_literal("GESCHWAERZT"),
        "RO" => Object::Reference(ro),
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(a)]));
    muss_fallen(&d.finish(), "/Redact /RO");
}

/// Gegenprobe: das Rendition-Beiwerk einer Screen-Annotation hängt an `/A`
/// und fällt mit ihm — samt `/MediaClip /N`, `/D /F` und `/Alt`.
#[test]
fn screen_annotation_verliert_ihre_rendition_mit_der_aktion() {
    let mut d: Doc = page(&["Rechnung 4711"]);
    let a = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Screen",
        "Rect" => vec![10.into(), 10.into(), 130.into(), 100.into()],
        "T" => Object::string_literal("Bildschirm"),
        "A" => dictionary! {
            "S" => "Rendition",
            "OP" => 0,
            "R" => dictionary! {
                "Type" => "Rendition",
                "S" => "MR",
                "N" => Object::string_literal(format!("Film {SECRET}")),
                "C" => dictionary! {
                    "Type" => "MediaClip",
                    "S" => "MCD",
                    "N" => Object::string_literal(format!("Clip {SECRET}")),
                    "CT" => Object::string_literal("video/mp4"),
                    "D" => dictionary! { "Type" => "Filespec", "F" => Object::string_literal(format!("{SECRET}.mp4")) },
                    "Alt" => vec![Object::string_literal(""), Object::string_literal(format!("Alt {SECRET}"))],
                },
            },
        },
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(a)]));
    let report = muss_fallen(&d.finish(), "/Screen /A /R");
    assert_eq!(report.annotation_actions_removed, 1);
}

// ---------------------------------------------------------------------------
// Werkzeug: die Proben als Dateien, für den Lauf über die Kommandozeile
// ---------------------------------------------------------------------------

/// Schreibt die Proben nach `KORPUS_DIR` — für `redact-rs … -f` und
/// `--check-leaks`, `pdfinfo`, `pdftotext`, `pdftoppm` außerhalb dieses Tests.
#[test]
#[ignore = "Werkzeug, keine Prüfung"]
fn korpus_schreiben() {
    let dir = std::env::var("KORPUS_DIR").expect("KORPUS_DIR setzen");
    std::fs::create_dir_all(&dir).expect("Verzeichnis");
    let mut faelle: Vec<(&str, Vec<u8>)> = Vec::new();

    // Movie mit /Movie und einer IBAN auf der Seite (die fällt).
    {
        let mut d: Doc = page(&["Kontoinhaber: Max Mustermann", &format!("IBAN: {SECRET}")]);
        let a = d.add(Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Movie",
            "Rect" => vec![300.into(), 500.into(), 420.into(), 590.into()],
            "T" => Object::string_literal("Film"),
            "Movie" => dictionary! {
                "F" => Object::string_literal(format!("Kontoauszug {SECRET}.mov")),
                "Aspect" => vec![320.into(), 240.into()],
            },
        }));
        d.page_dict_set("Annots", Object::Array(vec![Object::Reference(a)]));
        faelle.push(("movie", d.finish()));
    }
    // Vermessung mit /AP.
    {
        let mut d: Doc = page(&["Kontoinhaber: Max Mustermann", &format!("IBAN: {SECRET}")]);
        let font_id = d.font_id;
        let ap = d.add(Object::Stream(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 200.into(), 30.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => Object::Reference(font_id) } },
            },
            b"0 0 1 RG 2 w 5 15 m 195 15 l S BT /F1 12 Tf 60 18 Td (12,5 cm) Tj ET\n".to_vec(),
        )));
        let a = d.add(Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "PolyLine",
            "Rect" => vec![300.into(), 400.into(), 500.into(), 430.into()],
            "Vertices" => vec![305.into(), 415.into(), 495.into(), 415.into()],
            "AP" => dictionary! { "N" => Object::Reference(ap) },
            "Measure" => dictionary! {
                "Type" => "Measure",
                "R" => Object::string_literal(format!("1 zu {SECRET}")),
                "X" => Object::Array(vec![Object::Dictionary(dictionary! {
                    "U" => Object::string_literal("cm"),
                    "C" => Object::Real(1.0),
                })]),
            },
        }));
        d.page_dict_set("Annots", Object::Array(vec![Object::Reference(a)]));
        faelle.push(("vermessung", d.finish()));
    }
    // Dateianhang mit Popup.
    {
        let mut d: Doc = page(&["Kontoinhaber: Max Mustermann", &format!("IBAN: {SECRET}")]);
        let (a, _) = dateianhang(&mut d);
        let popup = d.add(Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Popup",
            "Rect" => vec![100.into(), 100.into(), 300.into(), 200.into()],
            "Parent" => Object::Reference(a),
            "Open" => false,
        }));
        d.doc
            .get_dictionary_mut(a)
            .unwrap()
            .set("Popup", Object::Reference(popup));
        d.page_dict_set(
            "Annots",
            Object::Array(vec![Object::Reference(a), Object::Reference(popup)]),
        );
        faelle.push(("anhang_popup", d.finish()));
    }
    // ZUGFeRD-artig: /Names /EmbeddedFiles + /AF am Katalog.
    {
        let mut d: Doc = page(&["Kontoinhaber: Max Mustermann", &format!("IBAN: {SECRET}")]);
        let strom = d.add(Object::Stream(Stream::new(
            dictionary! { "Type" => "EmbeddedFile", "Subtype" => "text/xml" },
            format!("<rsm:CrossIndustryInvoice><ram:IBANID>{SECRET}</ram:IBANID></rsm:CrossIndustryInvoice>").into_bytes(),
        )));
        let filespec = d.add(Object::Dictionary(dictionary! {
            "Type" => "Filespec",
            "F" => Object::string_literal("factur-x.xml"),
            "UF" => Object::string_literal("factur-x.xml"),
            "AFRelationship" => "Alternative",
            "EF" => dictionary! { "F" => Object::Reference(strom) },
        }));
        let blatt = d.add(Object::Dictionary(dictionary! {
            "Names" => vec![Object::string_literal("factur-x.xml"), Object::Reference(filespec)],
        }));
        let names = d.add(Object::Dictionary(
            dictionary! { "EmbeddedFiles" => Object::Reference(blatt) },
        ));
        d.catalog_set("Names", Object::Reference(names));
        d.catalog_set("AF", Object::Array(vec![Object::Reference(filespec)]));
        faelle.push(("zugferd", d.finish()));
    }
    // XMP am Bild-XObject.
    {
        let mut d: Doc = page(&["Kontoinhaber: Max Mustermann", &format!("IBAN: {SECRET}")]);
        let xmp = d.add(Object::Stream(Stream::new(
            dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
            format!("<x:xmpmeta><dc:description>Kontoauszug {SECRET}</dc:description></x:xmpmeta>")
                .into_bytes(),
        )));
        let bild = d.add(Object::Stream(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 1,
                "Height" => 1,
                "ColorSpace" => "DeviceGray",
                "BitsPerComponent" => 8,
                "Metadata" => Object::Reference(xmp),
            },
            vec![0x80],
        )));
        let resources_id = d.resources_id;
        d.doc
            .get_dictionary_mut(resources_id)
            .unwrap()
            .set("XObject", dictionary! { "Im1" => Object::Reference(bild) });
        faelle.push(("bild_xmp", d.finish()));
    }
    // Ein Ebenenname als **Verweis** (`/OCG /Name 12 0 R`): er verschwindet
    // aus der Ausgabe, der Bericht meldet ihn nicht — die Konsole sagt
    // „Metadaten: nichts zu entfernen“ über eine Datei, aus der gerade ein
    // Ebenenname entfernt wurde.
    {
        let mut d: Doc = page(&[]);
        let name = d.add(Object::string_literal(format!("Ebene {SECRET}")));
        let ocg = d.add(Object::Dictionary(dictionary! {
            "Type" => "OCG",
            "Name" => Object::Reference(name),
        }));
        let resources_id = d.resources_id;
        d.doc.get_dictionary_mut(resources_id).unwrap().set(
            "Properties",
            dictionary! { "oc1" => Object::Reference(ocg) },
        );
        d.set_content(
            b"/OC /oc1 BDC BT /F1 12 Tf 72 700 Td (Kontoinhaber: Max Mustermann) Tj ET EMC\n",
        );
        faelle.push(("ebenenname_verweis", d.finish()));
    }

    for (name, bytes) in faelle {
        std::fs::write(format!("{dir}/{name}.pdf"), &bytes).expect("schreibbar");
        println!(
            "{name}: Fundstellen in der Probe: {}",
            leaks(&bytes, SECRET).len()
        );
    }
}
