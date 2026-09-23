//! Spur-A-Runde 1 (nach Commit 0eb898d), Prüfer B: **Metadaten-Träger, die
//! den Lauf überstehen** — Probenlisten-Zeile 3 (`PRUEFLISTE.md`).
//!
//! Zwei Teile:
//!
//! * **A) Die Zeile selbst.** Jeder in R5/R7 genannte Träger (`/Popup`, `/AF`,
//!   `/PieceInfo`, `/Movie`, `/RichMediaContent`, `/3DD`, `/3DV`, `/RO`,
//!   `/Measure`, XMP am Bild, `/IRT` auf ein Struktur-Element, Lesezeichen
//!   mit zweitem Halter, Ebenenname, Feldwerte, Elternfeld-Texte) geht noch
//!   einmal durch den **vollen** Lauf — Schwärzung, `strip_metadata`,
//!   `save_to_bytes`, wie `redact-pipeline` ihn fährt —, nicht nur durch
//!   `strip_metadata`. Erwartung: grün.
//! * **B) Träger, die R5/R7 nicht nennen.** Dieselbe Frage an Stellen, die
//!   `meta.rs` nicht anfasst. Jeder Test formuliert das **gewünschte**
//!   Verhalten (der Klartext ist nach dem Lauf weg); wo er rot ist, ist er
//!   der Beleg für ein **stilles Leck**: der Lauf endet ohne Fehler und ohne
//!   Warnung, `leaks` findet den Suchbegriff in der Ausgabe.
//!
//! Orakel ist ausschließlich [`redact_pdf::leaks`] an den geschriebenen
//! Bytes — dieselbe Prüfung wie `redact-rs … --check-leaks`. Der Bericht des
//! Programms steht in jeder Fehlermeldung nur daneben, damit sichtbar ist,
//! dass er schweigt.
//!
//! Die Seite jeder Probe trägt das Geheimnis **auch** im Seitentext, und der
//! Lauf schwärzt diese Zeile: ein Fund in der Ausgabe kommt dann nur noch aus
//! dem Träger, und die Fundstelle nennt ihn.

mod common;

use std::time::{Duration, Instant};

use common::{page, utf16be_bom, Doc, SECRET};
use lopdf::{dictionary, Object, ObjectId, Stream, StringFormat};
use redact_core::{Action, Rect, Redaction, Region, Source};
use redact_pdf::{
    leaks, load_from_bytes, save_to_bytes, strip_metadata, MetadataReport, PdfRedactor,
    RedactionReport,
};

// ---------------------------------------------------------------------------
// Werkzeug
// ---------------------------------------------------------------------------

/// Deckt die Textzeilen ab, die [`common::page`] setzt (y = 700 und 685).
fn schwaerzung() -> Redaction {
    Redaction::new(
        Region::new(
            0,
            Rect::new(40.0, 600.0, 560.0, 760.0),
            None,
            Source::Manual {
                reason: "Spur A, Prüfer B".into(),
            },
        ),
        Action::Blackout,
    )
}

struct Lauf {
    redaction: RedactionReport,
    meta: MetadataReport,
    out: Vec<u8>,
}

/// Der volle Lauf, wie `redact-pipeline` ihn fährt: Schwärzung, Metadaten,
/// Schreiben.
fn lauf(bytes: &[u8]) -> Lauf {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    let redaction = PdfRedactor::new()
        .apply_with_report(&mut doc, &[schwaerzung()])
        .expect("Schwärzung läuft durch");
    let meta = strip_metadata(&mut doc);
    let out = save_to_bytes(&doc).expect("Speichern");
    Lauf {
        redaction,
        meta,
        out,
    }
}

/// Die Probe trägt das Geheimnis vorher; nach dem vollen Lauf steht es nicht
/// mehr in der Datei. Schlägt das fehl, nennt die Meldung die Fundstellen
/// **und** was der Lauf dazu gesagt hat — bei einem stillen Leck: nichts.
#[track_caller]
fn muss_fallen(bytes: &[u8], was: &str) -> Lauf {
    assert!(
        !leaks(bytes, SECRET).is_empty(),
        "{was}: die Probe muss das Geheimnis vorher tragen"
    );
    let l = lauf(bytes);
    let hits = leaks(&l.out, SECRET);
    assert!(
        hits.is_empty(),
        "{was}: STILLES LECK — nach dem vollen Lauf steht der Klartext noch in der \
         Ausgabe.\n  Warnungen der Schwärzung: {:?}\n  Bericht Metadaten: {:?}\n  \
         Fundstellen:\n{}",
        l.redaction.warnings,
        l.meta.summary(),
        hits.join("\n")
    );
    l
}

/// Eine Seite mit dem Geheimnis im Seitentext — die Zeile fällt im Lauf.
fn probe() -> Doc {
    page(&["Kontoinhaber: Max Mustermann", &format!("IBAN: {SECRET}")])
}

fn s(text: impl Into<String>) -> Object {
    Object::string_literal(text.into())
}

fn rect() -> Object {
    vec![10.into(), 10.into(), 30.into(), 30.into()]
        .into_iter()
        .collect::<Vec<Object>>()
        .into()
}

fn annots(d: &mut Doc, ids: &[ObjectId]) {
    d.page_dict_set(
        "Annots",
        Object::Array(ids.iter().map(|id| Object::Reference(*id)).collect()),
    );
}

/// Ein eingebetteter Datei-Strom mit dem Geheimnis samt Filespec.
fn filespec(d: &mut Doc, name: &str) -> ObjectId {
    let strom = d.add(Object::Stream(Stream::new(
        dictionary! { "Type" => "EmbeddedFile" },
        format!("Kontoauszug\nIBAN {SECRET}\n").into_bytes(),
    )));
    d.add(Object::Dictionary(dictionary! {
        "Type" => "Filespec",
        "F" => s(name),
        "UF" => s(name),
        "EF" => dictionary! { "F" => Object::Reference(strom) },
    }))
}

/// Ein Form-XObject, das das Geheimnis als Text zeichnet.
fn form_mit_text(d: &mut Doc) -> ObjectId {
    let font_id = d.font_id;
    d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 200.into(), 20.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => Object::Reference(font_id) } },
            },
            format!("BT /F1 8 Tf 2 2 Td ({SECRET}) Tj ET\n").into_bytes(),
        )
        .with_compression(false),
    ))
}

// ===========================================================================
// A) Die Probenlisten-Zeile: jeder in R5/R7 genannte Träger, voller Lauf
// ===========================================================================

#[test]
fn a_popup_haelt_dateianhang() {
    let mut d = probe();
    let fs = filespec(&mut d, "kontoauszug.txt");
    let a = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "FileAttachment", "Rect" => rect(),
        "FS" => Object::Reference(fs), "Contents" => s("Anhang"), "Name" => "Paperclip",
    }));
    let popup = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Popup", "Rect" => rect(),
        "Parent" => Object::Reference(a), "Open" => false,
    }));
    d.doc
        .get_dictionary_mut(a)
        .unwrap()
        .set("Popup", Object::Reference(popup));
    annots(&mut d, &[a, popup]);
    muss_fallen(&d.finish(), "/FileAttachment mit /Popup");
}

#[test]
fn a_irt_haelt_dateianhang() {
    let mut d = probe();
    let fs = filespec(&mut d, "kontoauszug.txt");
    let a = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "FileAttachment", "Rect" => rect(),
        "FS" => Object::Reference(fs),
    }));
    let antwort = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Text", "Rect" => rect(),
        "IRT" => Object::Reference(a), "Contents" => s("Antwort"),
    }));
    annots(&mut d, &[a, antwort]);
    muss_fallen(&d.finish(), "/FileAttachment mit /IRT");
}

#[test]
fn a_af_am_katalog_an_der_seite_und_an_der_annotation() {
    for ort in ["Katalog", "Seite", "Annotation", "XObject"] {
        let mut d = probe();
        let fs = filespec(&mut d, "factur-x.xml");
        let af = Object::Array(vec![Object::Reference(fs)]);
        match ort {
            "Katalog" => {
                let blatt = d.add(Object::Dictionary(dictionary! {
                    "Names" => vec![s("factur-x.xml"), Object::Reference(fs)],
                }));
                let names = d.add(Object::Dictionary(
                    dictionary! { "EmbeddedFiles" => Object::Reference(blatt) },
                ));
                d.catalog_set("Names", Object::Reference(names));
                d.catalog_set("AF", af);
            }
            "Seite" => d.page_dict_set("AF", af),
            "Annotation" => {
                let a = d.add(Object::Dictionary(dictionary! {
                    "Type" => "Annot", "Subtype" => "Text", "Rect" => rect(),
                    "Contents" => s("Notiz"), "AF" => af,
                }));
                annots(&mut d, &[a]);
            }
            _ => {
                let form = d.add(Object::Stream(Stream::new(
                    dictionary! {
                        "Type" => "XObject", "Subtype" => "Form",
                        "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()],
                        "AF" => af,
                    },
                    b"0 0 10 10 re f\n".to_vec(),
                )));
                let resources_id = d.resources_id;
                d.doc
                    .get_dictionary_mut(resources_id)
                    .unwrap()
                    .set("XObject", dictionary! { "Fm1" => Object::Reference(form) });
            }
        }
        muss_fallen(&d.finish(), &format!("/AF am {ort}"));
    }
}

#[test]
fn a_pieceinfo_und_xmp_an_katalog_seite_und_xobjects() {
    for ort in ["Katalog", "Seite", "Form", "Bild"] {
        let mut d = probe();
        let xmp = d.add(Object::Stream(Stream::new(
            dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
            format!("<x:xmpmeta><dc:title>Kontoauszug {SECRET}</dc:title></x:xmpmeta>")
                .into_bytes(),
        )));
        let piece: Object = dictionary! {
            "Illustrator" => dictionary! {
                "LastModified" => s("D:20240101"),
                "Private" => s(format!("Kontoauszug {SECRET}")),
            },
        }
        .into();
        match ort {
            "Katalog" => {
                d.catalog_set("Metadata", Object::Reference(xmp));
                d.catalog_set("PieceInfo", piece);
            }
            "Seite" => {
                d.page_dict_set("Metadata", Object::Reference(xmp));
                d.page_dict_set("PieceInfo", piece);
            }
            _ => {
                let xobj = if ort == "Form" {
                    d.add(Object::Stream(Stream::new(
                        dictionary! {
                            "Type" => "XObject", "Subtype" => "Form",
                            "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()],
                            "PieceInfo" => piece, "Metadata" => Object::Reference(xmp),
                        },
                        b"0 0 10 10 re f\n".to_vec(),
                    )))
                } else {
                    d.add(Object::Stream(Stream::new(
                        dictionary! {
                            "Type" => "XObject", "Subtype" => "Image",
                            "Width" => 1, "Height" => 1, "ColorSpace" => "DeviceGray",
                            "BitsPerComponent" => 8,
                            "PieceInfo" => piece, "Metadata" => Object::Reference(xmp),
                        },
                        vec![0x80],
                    )))
                };
                let resources_id = d.resources_id;
                d.doc
                    .get_dictionary_mut(resources_id)
                    .unwrap()
                    .set("XObject", dictionary! { "X1" => Object::Reference(xobj) });
            }
        }
        muss_fallen(&d.finish(), &format!("/PieceInfo und /Metadata am {ort}"));
    }
}

#[test]
fn a_beiwerk_movie_measure_richmedia_3d_ro() {
    // /Movie
    {
        let mut d = probe();
        let a = d.add(Object::Dictionary(dictionary! {
            "Type" => "Annot", "Subtype" => "Movie", "Rect" => rect(), "T" => s("Film"),
            "Movie" => dictionary! { "F" => s(format!("Kontoauszug {SECRET}.mov")), "Aspect" => vec![320.into(), 240.into()] },
        }));
        annots(&mut d, &[a]);
        muss_fallen(&d.finish(), "/Movie /F");
    }
    // /Measure
    {
        let mut d = probe();
        let a = d.add(Object::Dictionary(dictionary! {
            "Type" => "Annot", "Subtype" => "PolyLine", "Rect" => rect(),
            "Vertices" => vec![10.into(), 10.into(), 30.into(), 30.into()],
            "Measure" => dictionary! {
                "Type" => "Measure", "R" => s(format!("1 zu {SECRET}")),
                "X" => Object::Array(vec![Object::Dictionary(dictionary! { "U" => s("cm"), "C" => Object::Real(1.0) })]),
            },
        }));
        annots(&mut d, &[a]);
        muss_fallen(&d.finish(), "/Measure /R");
    }
    // /RichMediaContent /Assets
    {
        let mut d = probe();
        let fs = filespec(&mut d, "daten.txt");
        let a = d.add(Object::Dictionary(dictionary! {
            "Type" => "Annot", "Subtype" => "RichMedia", "Rect" => rect(),
            "RichMediaContent" => dictionary! {
                "Type" => "RichMediaContent",
                "Assets" => dictionary! { "Names" => vec![s("daten.txt"), Object::Reference(fs)] },
                "Configurations" => Object::Array(Vec::new()),
            },
        }));
        annots(&mut d, &[a]);
        muss_fallen(&d.finish(), "/RichMediaContent /Assets");
    }
    // /3DD /OnInstantiate und /3DV /XN
    {
        let mut d = probe();
        let js = d.add(Object::Stream(Stream::new(
            dictionary! {},
            format!("app.alert(\"{SECRET}\");").into_bytes(),
        )));
        let daten = d.add(Object::Stream(Stream::new(
            dictionary! { "Type" => "3D", "Subtype" => "U3D", "OnInstantiate" => Object::Reference(js) },
            b"U3D\0\0\0\0".to_vec(),
        )));
        let a = d.add(Object::Dictionary(dictionary! {
            "Type" => "Annot", "Subtype" => "3D", "Rect" => rect(),
            "3DD" => Object::Reference(daten),
            "3DV" => dictionary! { "Type" => "3DView", "XN" => s(format!("Ansicht {SECRET}")), "IN" => s("v1"), "MS" => "M" },
        }));
        annots(&mut d, &[a]);
        muss_fallen(&d.finish(), "/3DD und /3DV");
    }
    // /RO
    {
        let mut d = probe();
        let ro = form_mit_text(&mut d);
        let a = d.add(Object::Dictionary(dictionary! {
            "Type" => "Annot", "Subtype" => "Redact", "Rect" => rect(),
            "OverlayText" => s("GESCHWAERZT"), "RO" => Object::Reference(ro),
        }));
        annots(&mut d, &[a]);
        muss_fallen(&d.finish(), "/Redact /RO");
    }
}

#[test]
fn a_texte_am_elternfeld_popup_und_antwortkette() {
    let mut d = probe();
    let feld = d.doc.new_object_id();
    let mk = d.add(Object::Dictionary(
        dictionary! { "CA" => s(format!("Knopf {SECRET}")) },
    ));
    let widget = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "Rect" => rect(),
        "Parent" => Object::Reference(feld), "MK" => Object::Reference(mk),
        "NM" => s(format!("nm {SECRET}")), "DS" => s(format!("font: {SECRET}")),
        "PA" => dictionary! { "S" => "URI", "URI" => s(format!("mailto:x?subject={SECRET}")) },
    }));
    d.doc.objects.insert(
        feld,
        Object::Dictionary(dictionary! {
            "FT" => "Ch", "T" => s(format!("t {SECRET}")), "TU" => s(format!("tu {SECRET}")),
            "TM" => s(format!("tm {SECRET}")),
            "Opt" => vec![s(format!("opt {SECRET}"))],
            "V" => Object::String(utf16be_bom(SECRET), StringFormat::Literal),
            "DV" => s(SECRET), "RV" => s(format!("<p>{SECRET}</p>")),
            "AA" => dictionary! { "K" => dictionary! { "S" => "JavaScript", "JS" => s(format!("// {SECRET}")) } },
            "Kids" => vec![Object::Reference(widget)],
        }),
    );
    // Ein Popup in /Annots, dessen Elternnotiz nur über /Parent erreichbar ist.
    let notiz = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Text", "Rect" => rect(),
        "Contents" => s(format!("Notiz {SECRET}")), "T" => s(SECRET), "Subj" => s(SECRET),
        "RC" => s(format!("<p>{SECRET}</p>")),
    }));
    let popup = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Popup", "Rect" => rect(),
        "Parent" => Object::Reference(notiz), "Contents" => s(format!("Popup {SECRET}")),
    }));
    // Eine Antwort, deren /IRT auf ein Struktur-Element mit /Alt zeigt.
    let elem = d.add(Object::Dictionary(dictionary! {
        "Type" => "StructElem", "S" => "Figure",
        "Alt" => s(format!("Alt {SECRET}")), "ActualText" => s(SECRET),
    }));
    let antwort = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Text", "Rect" => rect(),
        "IRT" => Object::Reference(elem), "Contents" => s("Antwort"),
    }));
    annots(&mut d, &[widget, popup, antwort]);
    muss_fallen(
        &d.finish(),
        "Elternfeld, Popup-Eltern, /IRT auf /StructElem",
    );
}

#[test]
fn a_lesezeichen_mit_zweitem_halter_und_ebenenname() {
    let mut d = probe();
    let outlines = d.doc.new_object_id();
    let item = d.add(Object::Dictionary(dictionary! {
        "Title" => s(format!("Kontoauszug {SECRET}")), "Parent" => Object::Reference(outlines),
        "A" => dictionary! { "S" => "URI", "URI" => s(format!("http://x/{SECRET}")) },
    }));
    d.doc.objects.insert(
        outlines,
        Object::Dictionary(dictionary! {
            "Type" => "Outlines", "First" => Object::Reference(item), "Last" => Object::Reference(item), "Count" => 1,
        }),
    );
    d.catalog_set("Outlines", Object::Reference(outlines));
    // Der zweite Halter: ein Schlüssel an der Seite, den niemand bereinigt.
    d.page_dict_set("Zusatz", Object::Reference(item));
    // Ein Ebenenname, den nur /Resources /Properties hält.
    let name = d.add(s(format!("Ebene {SECRET}")));
    let ocg = d.add(Object::Dictionary(
        dictionary! { "Type" => "OCG", "Name" => Object::Reference(name) },
    ));
    let resources_id = d.resources_id;
    d.doc.get_dictionary_mut(resources_id).unwrap().set(
        "Properties",
        dictionary! { "oc1" => Object::Reference(ocg) },
    );
    muss_fallen(&d.finish(), "Lesezeichen mit zweitem Halter, Ebenenname");
}

#[test]
fn a_info_names_javascript_openaction_aa_xfa() {
    let mut d = probe();
    let info = d.add(Object::Dictionary(
        dictionary! { "Title" => s(format!("Kontoauszug {SECRET}")) },
    ));
    d.doc.trailer.set("Info", Object::Reference(info));
    let fs = filespec(&mut d, "anhang.txt");
    let names = d.add(Object::Dictionary(dictionary! {
        "EmbeddedFiles" => dictionary! { "Names" => vec![s("anhang"), Object::Reference(fs)] },
        "JavaScript" => dictionary! { "Names" => vec![s("start"), Object::Dictionary(dictionary! {
            "S" => "JavaScript", "JS" => s(format!("var iban = '{SECRET}';")) })] },
        "Dests" => dictionary! { "Names" => vec![s(SECRET), vec![0.into(), "Fit".into()].into()] },
    }));
    d.catalog_set("Names", Object::Reference(names));
    d.catalog_set("Dests", dictionary! { "konto" => s(SECRET) }.into());
    d.catalog_set(
        "OpenAction",
        dictionary! { "S" => "JavaScript", "JS" => s(format!("app.alert('{SECRET}');")) }.into(),
    );
    d.catalog_set(
        "AA",
        dictionary! { "WC" => dictionary! { "S" => "JavaScript", "JS" => s(format!("// {SECRET}")) } }.into(),
    );
    d.page_dict_set(
        "AA",
        dictionary! { "O" => dictionary! { "S" => "JavaScript", "JS" => s(format!("// {SECRET}")) } }.into(),
    );
    d.catalog_set(
        "AcroForm",
        dictionary! {
            "Fields" => Object::Array(Vec::new()),
            "XFA" => s(format!("<xfa>{SECRET}</xfa>")),
        }
        .into(),
    );
    d.catalog_set(
        "OCProperties",
        dictionary! { "OCGs" => Object::Array(Vec::new()), "D" => dictionary! { "Order" => vec![s(SECRET)] } }.into(),
    );
    d.catalog_set(
        "StructTreeRoot",
        dictionary! { "Type" => "StructTreeRoot", "K" => dictionary! { "Type" => "StructElem", "S" => "P", "ActualText" => s(SECRET) } }.into(),
    );
    muss_fallen(
        &d.finish(),
        "/Info, /Names, /Dests, /OpenAction, /AA, /XFA, /OCProperties, /StructTreeRoot",
    );
}

// ===========================================================================
// B) Träger, die R5/R7 nicht nennen
// ===========================================================================

/// **Signatur, die `/Perms` am Leben hält.** So zertifiziert Acrobat ein
/// Dokument: das Signaturfeld trägt `/V` → Signatur-Dictionary, und der
/// Katalog hält dasselbe Dictionary noch einmal unter `/Perms /DocMDP`
/// (PDF 32000-1, 12.8.4, Tabelle 258). `/V` fällt am Feld — der zweite Halter
/// bleibt, und mit ihm `/Reason`, `/Location`, `/ContactInfo`, `/Name`
/// (Tabelle 252): frei wählbarer Text, den der Signaturdialog erfragt.
///
/// Zweite Form: `/Perms /UR3` (Usage Rights) mit `/Reference … /TransformParams
/// /Msg` (Tabelle 255) — ebenfalls Text.
#[test]
#[ignore = "offen: Register #70 Katalog-/Seiten-/Objektschluessel — Spur-A-Runde 1, Beleg absichtlich rot"]
fn b_perms_haelt_die_signatur_samt_grund_und_ort() {
    for form in ["DocMDP", "UR3"] {
        let mut d = probe();
        let sig = d.add(Object::Dictionary(dictionary! {
            "Type" => "Sig", "Filter" => "Adobe.PPKLite", "SubFilter" => "adbe.pkcs7.detached",
            "Name" => s("Max Mustermann"),
            "Reason" => s(format!("Freigabe Kontoauszug {SECRET}")),
            "Location" => s(format!("Filiale {SECRET}")),
            "ContactInfo" => s(format!("kontakt {SECRET}")),
            "M" => s("D:20240101120000Z"),
            "ByteRange" => vec![0.into(), 0.into(), 0.into(), 0.into()],
            "Contents" => Object::String(vec![0u8; 16], StringFormat::Hexadecimal),
            "Reference" => vec![Object::Dictionary(dictionary! {
                "Type" => "SigRef", "TransformMethod" => form,
                "TransformParams" => dictionary! { "Type" => "TransformParams", "P" => 1, "V" => "1.2",
                    "Msg" => s(format!("Nutzungsrechte {SECRET}")) },
            })],
        }));
        let feld = d.add(Object::Dictionary(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "FT" => "Sig", "T" => s("Signature1"),
            "Rect" => rect(), "F" => 132, "V" => Object::Reference(sig),
        }));
        annots(&mut d, &[feld]);
        d.catalog_set(
            "AcroForm",
            dictionary! { "Fields" => vec![Object::Reference(feld)], "SigFlags" => 3 }.into(),
        );
        d.catalog_set(
            "Perms",
            dictionary! { form => Object::Reference(sig) }.into(),
        );
        muss_fallen(
            &d.finish(),
            &format!("/Perms /{form} hält das Signatur-Dictionary"),
        );
    }
}

/// **Document Security Store** (`/DSS`, PDF 2.0 12.8.4.3; PAdES-LTV): der
/// Katalog hält Zertifikate, OCSP-Antworten und CRLs als Ströme. Ein
/// Zertifikat trägt den Namen des Unterzeichners im Klartext (DER-kodierte
/// UTF8String liegen als Bytes hintereinander); ein `/VRI`-Eintrag hält
/// dieselben Ströme noch einmal. Nach einer Schwärzung ist jede Signatur
/// ohnehin gebrochen — die Ströme tragen nur noch Text.
#[test]
#[ignore = "offen: Register #70 Katalog-/Seiten-/Objektschluessel — Spur-A-Runde 1, Beleg absichtlich rot"]
fn b_dss_zertifikate_bleiben() {
    let mut d = probe();
    let cert = d.add(Object::Stream(Stream::new(dictionary! {}, {
        let mut der = vec![0x30u8, 0x82, 0x01, 0x00];
        der.extend_from_slice(format!("CN=Max Mustermann, serialNumber={SECRET}").as_bytes());
        der.extend_from_slice(&[0, 0]);
        der
    })));
    d.catalog_set(
        "DSS",
        dictionary! {
            "Type" => "DSS",
            "Certs" => vec![Object::Reference(cert)],
            "VRI" => dictionary! { "A1B2" => dictionary! { "Cert" => vec![Object::Reference(cert)] } },
        }
        .into(),
    );
    muss_fallen(&d.finish(), "/DSS /Certs");
}

/// **Seitenbeschriftungen** (`/PageLabels`, 12.4.2, Tabelle 159): der
/// Nummernbaum trägt je Bereich ein `/P` — den Präfix, den jeder Betrachter
/// in der Seitenanzeige zeigt („Anhang A-1“). Frei wählbarer Text, in keinem
/// Strom, an keiner Annotation.
#[test]
#[ignore = "offen: Register #70 Katalog-/Seiten-/Objektschluessel — Spur-A-Runde 1, Beleg absichtlich rot"]
fn b_seitenbeschriftung_praefix_bleibt() {
    let mut d = probe();
    d.catalog_set(
        "PageLabels",
        dictionary! {
            "Nums" => vec![0.into(), Object::Dictionary(dictionary! { "S" => "D", "P" => s(format!("Kontoauszug {SECRET} - ")) })],
        }
        .into(),
    );
    muss_fallen(&d.finish(), "/PageLabels /P");
}

/// **Artikel** (`/Threads`, 12.4.3): jeder Thread trägt unter `/I` ein
/// Info-Dictionary mit `/Title`, `/Author`, `/Subject`, `/Keywords` — dieselben
/// Schlüssel wie `/Info`, das fällt. Der Thread hängt am Katalog **und** über
/// den Bead (`/B` an der Seite, `/T` am Bead) an der Seite.
#[test]
#[ignore = "offen: Register #70 Katalog-/Seiten-/Objektschluessel — Spur-A-Runde 1, Beleg absichtlich rot"]
fn b_artikel_info_bleibt() {
    let mut d = probe();
    let thread = d.doc.new_object_id();
    let page_id = d.page_id;
    let bead = d.add(Object::Dictionary(dictionary! {
        "Type" => "Bead", "T" => Object::Reference(thread), "P" => Object::Reference(page_id),
        "R" => vec![72.into(), 680.into(), 300.into(), 720.into()],
    }));
    d.doc
        .get_dictionary_mut(bead)
        .unwrap()
        .set("N", Object::Reference(bead));
    d.doc
        .get_dictionary_mut(bead)
        .unwrap()
        .set("V", Object::Reference(bead));
    d.doc.objects.insert(
        thread,
        Object::Dictionary(dictionary! {
            "Type" => "Thread", "F" => Object::Reference(bead),
            "I" => dictionary! { "Title" => s(format!("Kontoauszug {SECRET}")), "Author" => s("Max Mustermann") },
        }),
    );
    d.catalog_set("Threads", vec![Object::Reference(thread)].into());
    d.page_dict_set("B", vec![Object::Reference(bead)].into());
    muss_fallen(&d.finish(), "/Threads /I /Title");
}

/// **Portfolio** (`/Collection`, 12.3.5): das Schema benennt Spalten mit
/// `/N` (Tabelle 157), `/D` nennt das anfangs gezeigte Dokument; die
/// eingebetteten Dateien selbst fallen mit `/Names`, das Schema bleibt. In
/// PDF 2.0 dazu `/Folders` mit `/Name` und `/Desc`.
#[test]
#[ignore = "offen: Register #70 Katalog-/Seiten-/Objektschluessel — Spur-A-Runde 1, Beleg absichtlich rot"]
fn b_portfolio_schema_bleibt() {
    let mut d = probe();
    d.catalog_set(
        "Collection",
        dictionary! {
            "Type" => "Collection",
            "Schema" => dictionary! { "Type" => "CollectionSchema",
                "f1" => dictionary! { "Type" => "CollectionField", "Subtype" => "S", "N" => s(format!("Konto {SECRET}")), "O" => 1 } },
            "D" => s(format!("Kontoauszug {SECRET}.pdf")),
            "Folders" => dictionary! { "Type" => "Folder", "ID" => 1, "Name" => s(format!("Ordner {SECRET}")), "Desc" => s(SECRET) },
        }
        .into(),
    );
    muss_fallen(&d.finish(), "/Collection /Schema, /D, /Folders");
}

/// **Ausgabebedingung** (`/OutputIntents`, 14.11.5, Tabelle 365): jede
/// PDF/A- und PDF/X-Datei trägt sie; `/OutputCondition`, `/Info` und
/// `/RegistryName` sind Textstrings.
#[test]
#[ignore = "offen: Register #70 Katalog-/Seiten-/Objektschluessel — Spur-A-Runde 1, Beleg absichtlich rot"]
fn b_outputintent_info_bleibt() {
    let mut d = probe();
    d.catalog_set(
        "OutputIntents",
        vec![Object::Dictionary(dictionary! {
            "Type" => "OutputIntent", "S" => "GTS_PDFA1",
            "OutputConditionIdentifier" => s("sRGB"),
            "OutputCondition" => s(format!("Druck {SECRET}")),
            "Info" => s(format!("Info {SECRET}")),
        })]
        .into(),
    );
    muss_fallen(&d.finish(), "/OutputIntents /Info");
}

/// **Basis-URI** (`/URI /Base`, 12.6.4.7, Tabelle 208) am Katalog — der
/// Pfad, gegen den jeder relative Link aufgelöst wird. Die Links fallen, die
/// Basis bleibt.
#[test]
#[ignore = "offen: Register #70 Katalog-/Seiten-/Objektschluessel — Spur-A-Runde 1, Beleg absichtlich rot"]
fn b_uri_base_bleibt() {
    let mut d = probe();
    d.catalog_set(
        "URI",
        dictionary! { "Base" => s(format!("https://bank.example/kunden/{SECRET}/")) }.into(),
    );
    muss_fallen(&d.finish(), "/URI /Base");
}

/// **Dokumentteile** (`/DPartRoot`, PDF 2.0 14.12; PDF/VT): der Baum
/// gliedert Seiten nach Empfängern, und jedes `/DPart` trägt unter `/DPM`
/// ein frei aufgebautes Metadaten-Dictionary — im PDF/VT-Druck von
/// Kontoauszügen genau Name, Adresse und Kontonummer des Empfängers. Die
/// Seite zeigt mit `/DPart` zurück.
#[test]
#[ignore = "offen: Register #70 Katalog-/Seiten-/Objektschluessel — Spur-A-Runde 1, Beleg absichtlich rot"]
fn b_dokumentteile_dpm_bleiben() {
    let mut d = probe();
    let root = d.doc.new_object_id();
    let page_id = d.page_id;
    let leaf = d.add(Object::Dictionary(dictionary! {
        "Type" => "DPart", "Parent" => Object::Reference(root),
        "Start" => Object::Reference(page_id), "End" => Object::Reference(page_id),
        "DPM" => dictionary! { "CIP4_Root" => dictionary! { "CIP4_Recipient" => dictionary! {
            "CIP4_Contact" => dictionary! { "CIP4_Person" => dictionary! { "CIP4_Firstname" => s("Max"), "CIP4_Familyname" => s("Mustermann") } },
            "Account" => s(SECRET),
        } } },
    }));
    d.doc.objects.insert(
        root,
        Object::Dictionary(dictionary! { "Type" => "DPart", "DParts" => vec![Object::Array(vec![Object::Reference(leaf)])] }),
    );
    d.catalog_set(
        "DPartRoot",
        dictionary! { "Type" => "DPartRoot", "DPartRootNode" => Object::Reference(root) }.into(),
    );
    d.page_dict_set("DPart", Object::Reference(leaf));
    muss_fallen(&d.finish(), "/DPartRoot … /DPM");
}

/// **Ansichtsfenster** an der Seite (`/VP`, 12.9, Tabelle 260): `/Name` ist
/// Text, `/Measure` dasselbe Dictionary, das an einer Annotation fällt — an
/// der Seite fällt es nicht. Geo-PDFs tragen dort `/GCS /WKT` mit Text.
#[test]
#[ignore = "offen: Register #70 Katalog-/Seiten-/Objektschluessel — Spur-A-Runde 1, Beleg absichtlich rot"]
fn b_viewport_name_und_measure_bleiben() {
    let mut d = probe();
    d.page_dict_set(
        "VP",
        vec![Object::Dictionary(dictionary! {
            "Type" => "Viewport", "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Name" => s(format!("Ansicht {SECRET}")),
            "Measure" => dictionary! { "Type" => "Measure", "R" => s(format!("1 zu {SECRET}")) },
        })]
        .into(),
    );
    muss_fallen(&d.finish(), "/VP /Name, /VP /Measure");
}

/// **Navigationsknoten** an der Seite (`/PresSteps`, 12.4.4.2, Tabelle 162):
/// `/NA` und `/PA` sind Aktionen — mit JavaScript — an einem Ort, den weder
/// `/AA` noch der Trägerlauf sieht.
#[test]
#[ignore = "offen: Register #70 Katalog-/Seiten-/Objektschluessel — Spur-A-Runde 1, Beleg absichtlich rot"]
fn b_navigationsknoten_aktion_bleibt() {
    let mut d = probe();
    d.page_dict_set(
        "PresSteps",
        dictionary! {
            "Type" => "NavNode",
            "NA" => dictionary! { "S" => "JavaScript", "JS" => s(format!("app.alert('{SECRET}');")) },
            "PA" => dictionary! { "S" => "URI", "URI" => s(format!("http://x/{SECRET}")) },
        }
        .into(),
    );
    muss_fallen(&d.finish(), "/PresSteps /NA /JS");
}

/// **Referenz-XObject** (`/Ref`, 8.10.4, Tabelle 97): ein Form-XObject, das
/// eine Seite einer **anderen Datei** importiert — `/F` ist ein Filespec, und
/// ein Filespec darf die Datei unter `/EF` einbetten. Der vierte Weg für eine
/// eingebettete Datei, neben `/Names`, `/FS` und `/AF` (alle drei fallen).
#[test]
#[ignore = "offen: Register #70 Katalog-/Seiten-/Objektschluessel — Spur-A-Runde 1, Beleg absichtlich rot"]
fn b_referenz_xobject_traegt_eine_eingebettete_datei() {
    let mut d = probe();
    let fs = filespec(&mut d, &format!("Kontoauszug {SECRET}.pdf"));
    let form = d.add(Object::Stream(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()],
            "Ref" => dictionary! { "F" => Object::Reference(fs), "Page" => 0 },
        },
        b"0 0 10 10 re f\n".to_vec(),
    )));
    let resources_id = d.resources_id;
    d.doc
        .get_dictionary_mut(resources_id)
        .unwrap()
        .set("XObject", dictionary! { "Fm1" => Object::Reference(form) });
    let mut raw = common::text_ops(&["Kontoinhaber: Max Mustermann", &format!("IBAN: {SECRET}")]);
    raw.extend_from_slice(b"q /Fm1 Do Q\n");
    d.set_content(&raw);
    muss_fallen(&d.finish(), "/Ref /F (Filespec mit /EF)");
}

/// **OPI** (`/OPI`, 14.11.7, Tabelle 397/398) am Bild-XObject: der Verweis
/// auf die Druckvorlage, mit Dateinamen (`/F`) und `/Comments`.
#[test]
#[ignore = "offen: Register #70 Katalog-/Seiten-/Objektschluessel — Spur-A-Runde 1, Beleg absichtlich rot"]
fn b_opi_dateiname_bleibt() {
    let mut d = probe();
    let bild = d.add(Object::Stream(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 1, "Height" => 1,
            "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
            "OPI" => dictionary! { "1.3" => dictionary! { "Type" => "OPI", "Version" => Object::Real(1.3),
                "F" => s(format!("Kontoauszug {SECRET}.eps")), "Comments" => s(SECRET) } },
        },
        vec![0x80],
    )));
    let resources_id = d.resources_id;
    d.doc
        .get_dictionary_mut(resources_id)
        .unwrap()
        .set("XObject", dictionary! { "Im1" => Object::Reference(bild) });
    muss_fallen(&d.finish(), "/OPI /F");
}

/// **Signaturfeld-Beiwerk** am Widget selbst: `/SV` (Seed Value, Tabelle 234)
/// mit `/Reasons` und `/LegalAttestation`, `/Lock` (Tabelle 233) mit
/// Feldnamen. Das Widget steht in `/Annots`, der Lauf erreicht es — und nimmt
/// `/V`, `/T`, `/TU`; diese beiden Schlüssel nicht.
#[test]
#[ignore = "offen: Register #70 Katalog-/Seiten-/Objektschluessel — Spur-A-Runde 1, Beleg absichtlich rot"]
fn b_seed_value_und_lock_am_signaturfeld_bleiben() {
    let mut d = probe();
    let feld = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Sig", "T" => s("Signature1"), "Rect" => rect(),
        "SV" => dictionary! { "Type" => "SV", "Ff" => 0, "Reasons" => vec![s(format!("Freigabe {SECRET}"))],
            "LegalAttestation" => vec![s(SECRET)] },
        "Lock" => dictionary! { "Type" => "SigFieldLock", "Action" => "Include", "Fields" => vec![s(format!("feld {SECRET}"))] },
    }));
    annots(&mut d, &[feld]);
    muss_fallen(&d.finish(), "/SV /Reasons, /Lock /Fields");
}

/// **Eine Seite außerhalb des Seitenbaums, gehalten von einem Verweis.** Ein
/// Link mit ausdrücklichem `/Dest [Seite /Fit]` bleibt (es trägt „nur Zahlen
/// und Verweise“); zeigt er auf eine Seite, die nicht mehr in `/Kids` steht
/// (gelöschte Seite, stehen gebliebener Link — so schreibt es jedes Werkzeug,
/// das Seiten entfernt, ohne Links zu prüfen), hält er sie über
/// `prune_unreachable` hinweg am Leben. Ihr Inhalt geht durch keine
/// Schwärzung: `get_pages()` kennt sie nicht. Zweite Form: `/P` einer
/// Annotation (Tabelle 164) auf dieselbe Seite.
#[test]
fn b_verwaiste_seite_hinter_dest_oder_p_bleibt_samt_inhalt() {
    for halter in ["Dest", "P"] {
        let mut d = probe();
        let pages_id = d.pages_id;
        let resources_id = d.resources_id;
        let inhalt = d.add(Object::Stream(Stream::new(
            dictionary! {},
            common::text_ops(&[&format!("Alte Seite: IBAN {SECRET}")]),
        )));
        let waise = d.add(Object::Dictionary(dictionary! {
            "Type" => "Page", "Parent" => Object::Reference(pages_id),
            "Contents" => Object::Reference(inhalt), "Resources" => Object::Reference(resources_id),
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        }));
        let mut link = dictionary! {
            "Type" => "Annot", "Subtype" => "Link", "Rect" => rect(),
        };
        match halter {
            "Dest" => link.set("Dest", vec![Object::Reference(waise), "Fit".into()]),
            _ => link.set("P", Object::Reference(waise)),
        }
        let a = d.add(Object::Dictionary(link));
        annots(&mut d, &[a]);
        let bytes = d.finish();
        let l = lauf(&bytes);
        let doc = load_from_bytes(&l.out).expect("Ausgabe lädt");
        assert_eq!(doc.get_pages().len(), 1, "der Seitenbaum hat eine Seite");
        let hits = leaks(&l.out, SECRET);
        assert!(
            hits.is_empty(),
            "/{halter} auf eine Seite außerhalb des Seitenbaums: STILLES LECK — ihr Inhalt \
             steht ungeschwärzt in der Ausgabe.\n  Warnungen: {:?}\n  Bericht: {:?}\n  \
             Fundstellen:\n{}",
            l.redaction.warnings,
            l.meta.summary(),
            hits.join("\n")
        );
    }
}

/// **`/MK /I`** — das Symbol eines Druckknopfs (Tabelle 189) ist ein
/// Form-XObject und darf Text zeichnen. `zf_q1_traeger_nachbarn` hält fest,
/// dass es **bleibt**; gelesen wird es nirgends: `crate::content` liest das
/// `/AP`, nicht das `/MK`. Dazu `/RI` und `/IX`.
#[test]
#[ignore = "offen: Register #71 Erscheinungsstroeme ausserhalb /Annots — Spur-A-Runde 1, Beleg absichtlich rot"]
fn b_mk_icon_mit_text_bleibt() {
    let mut d = probe();
    let icon = form_mit_text(&mut d);
    let leer = d.add(Object::Stream(Stream::new(
        dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 20.into(), 20.into()] },
        b"0.9 g 0 0 20 20 re f\n".to_vec(),
    )));
    let widget = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Btn", "Ff" => 65536, "T" => s("knopf"),
        "Rect" => rect(), "AP" => dictionary! { "N" => Object::Reference(leer) },
        "MK" => dictionary! { "I" => Object::Reference(icon), "TP" => 1 },
    }));
    annots(&mut d, &[widget]);
    muss_fallen(&d.finish(), "/MK /I mit Text");
}

/// **Der Erscheinungsstrom einer Annotation, die nur ihr `/Popup` hält.** Das
/// Popup steht in `/Annots`, seine Elternnotiz nicht mehr (ein Werkzeug hat
/// sie gestrichen, das Popup vergessen). Der Trägerlauf erreicht die Notiz
/// über `/Parent` und nimmt ihr `/Contents` — ihr `/AP` liest niemand:
/// `crate::content` läuft `/Annots` ab, nicht `/Parent`. Dieselbe Form über
/// `/IRT`.
#[test]
#[ignore = "offen: Register #71 Erscheinungsstroeme ausserhalb /Annots — Spur-A-Runde 1, Beleg absichtlich rot"]
fn b_ap_einer_nur_ueber_popup_oder_irt_gehaltenen_annotation_bleibt() {
    for weg in ["Popup", "IRT"] {
        let mut d = probe();
        let ap = form_mit_text(&mut d);
        let notiz = d.add(Object::Dictionary(dictionary! {
            "Type" => "Annot", "Subtype" => "FreeText", "Rect" => rect(),
            "Contents" => s("Notiz"), "AP" => dictionary! { "N" => Object::Reference(ap) },
        }));
        let halter = d.add(Object::Dictionary(match weg {
            "Popup" => dictionary! { "Type" => "Annot", "Subtype" => "Popup", "Rect" => rect(), "Parent" => Object::Reference(notiz) },
            _ => dictionary! { "Type" => "Annot", "Subtype" => "Text", "Rect" => rect(), "IRT" => Object::Reference(notiz), "Contents" => s("Antwort") },
        }));
        annots(&mut d, &[halter]);
        muss_fallen(
            &d.finish(),
            &format!("/AP einer nur über /{weg} gehaltenen Annotation"),
        );
    }
}

/// **Randfälle mit geringer Wahrscheinlichkeit** — Zeichenketten, die die
/// Norm vorsieht und die niemand mit einem Geheimnis füllt, die aber nach dem
/// Lauf wortwörtlich in der Datei stehen: `/Lang` (Katalog, Annotation),
/// `/M` und `/CreationDate` an einer Annotation, `/State` einer Notiz,
/// `/ExData /3DV /XN`, `/FontDescriptor /FontFamily`, `/CIDSystemInfo
/// /Registry`, das `/ID`-Paar des Trailers. Je Fall ein eigener Lauf, damit
/// die Meldung sagt, welcher es ist.
#[test]
#[ignore = "offen: Register #73 Normstrings (Einordnung offen) — Spur-A-Runde 1, Beleg absichtlich rot"]
fn b_randfaelle_normstrings() {
    type Fall<'a> = (&'a str, Box<dyn Fn(&mut Doc) + 'a>);
    let faelle: Vec<Fall> = vec![
        (
            "/Lang am Katalog",
            Box::new(|d| d.catalog_set("Lang", s(SECRET))),
        ),
        (
            "/Lang, /M, /CreationDate, /State, /ExData an einer Annotation",
            Box::new(|d| {
                let a = d.add(Object::Dictionary(dictionary! {
                    "Type" => "Annot", "Subtype" => "Text", "Rect" => rect(),
                    "Lang" => s(SECRET), "M" => s(format!("D:{SECRET}")), "CreationDate" => s(SECRET),
                    "State" => s(SECRET), "StateModel" => s("Review"),
                    "ExData" => dictionary! { "Type" => "ExData", "Subtype" => "Markup3D", "3DV" => dictionary! { "XN" => s(SECRET) } },
                }));
                annots(d, &[a]);
            }),
        ),
        (
            "/FontDescriptor /FontFamily und /CIDSystemInfo /Registry",
            Box::new(|d| {
                let font_id = d.font_id;
                let fd = d.add(Object::Dictionary(dictionary! {
                    "Type" => "FontDescriptor", "FontName" => "Helvetica", "Flags" => 32,
                    "FontBBox" => vec![0.into(), 0.into(), 1000.into(), 1000.into()],
                    "ItalicAngle" => 0, "Ascent" => 800, "Descent" => -200, "CapHeight" => 700, "StemV" => 80,
                    "FontFamily" => s(SECRET),
                }));
                let f = d.doc.get_dictionary_mut(font_id).unwrap();
                f.set("FontDescriptor", Object::Reference(fd));
                f.set("CIDSystemInfo", dictionary! { "Registry" => s(SECRET), "Ordering" => s("Identity"), "Supplement" => 0 });
            }),
        ),
        (
            "/ID im Trailer",
            Box::new(|d| {
                d.doc.trailer.set("ID", vec![s(SECRET), s(SECRET)]);
            }),
        ),
    ];
    let mut lecks = Vec::new();
    for (was, bauen) in faelle {
        let mut d = probe();
        bauen(&mut d);
        let bytes = d.finish();
        assert!(
            !leaks(&bytes, SECRET).is_empty(),
            "{was}: Probe trägt das Geheimnis"
        );
        let l = lauf(&bytes);
        let hits = leaks(&l.out, SECRET);
        if !hits.is_empty() {
            lecks.push(format!(
                "{was}: {} Fundstelle(n), Warnungen {:?}, Bericht {:?}\n    {}",
                hits.len(),
                l.redaction.warnings,
                l.meta.summary(),
                hits.join("\n    ")
            ));
        }
    }
    assert!(
        lecks.is_empty(),
        "Randfälle, die den Lauf überstehen:\n{}",
        lecks.join("\n")
    );
}

// ===========================================================================
// C) Dienstverweigerung: die Verweiskarte `Chains::of` ist quadratisch
// ===========================================================================

/// Ein Dokument mit `n` Objekten, die nichts als ein Verweis auf das nächste
/// sind (`4 0 obj 5 0 R endobj`), plus ein gewöhnlicher Rumpf.
fn verweiskette(n: u32) -> Vec<u8> {
    let d = probe();
    let base = d.finish();
    let start = d.doc.max_id + 10;
    let mut out = base;
    // Als eigene Revision anhängen — so kommt die Kette durch den Lader.
    let prev = {
        let key = b"startxref";
        let pos = out.windows(key.len()).rposition(|w| w == key).unwrap();
        let tail = &out[pos + key.len()..];
        let digits: String = tail
            .iter()
            .skip_while(|b| b.is_ascii_whitespace())
            .take_while(|b| b.is_ascii_digit())
            .map(|&b| b as char)
            .collect();
        digits
    };
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    let mut offsets = Vec::with_capacity(n as usize);
    for i in 0..n {
        let id = start + i;
        offsets.push((id, out.len()));
        let next = if i + 1 < n { start + i + 1 } else { 1 };
        out.extend_from_slice(format!("{id} 0 obj\n{next} 0 R\nendobj\n").as_bytes());
    }
    let xref_offset = out.len();
    let mut xref = String::from("xref\n0 1\n0000000000 65535 f \n");
    xref.push_str(&format!("{start} {n}\n"));
    for (_, offset) in &offsets {
        xref.push_str(&format!("{offset:010} 00000 n \n"));
    }
    let root = d.catalog_id;
    xref.push_str(&format!(
        "trailer\n<</Size {} /Root {} {} R /Prev {prev}>>\nstartxref\n{xref_offset}\n%%EOF\n",
        start + n + 1,
        root.0,
        root.1
    ));
    out.extend_from_slice(xref.as_bytes());
    out
}

fn strip_dauer(bytes: &[u8]) -> Duration {
    let mut doc = load_from_bytes(bytes).expect("ladbar");
    let t = Instant::now();
    strip_metadata(&mut doc);
    t.elapsed()
}

/// Messung (kein Test): `strip_metadata` an Ketten wachsender Länge.
#[test]
#[ignore = "Messung, keine Prüfung: cargo test … -- --ignored --nocapture"]
fn c_mess_verweiskette() {
    for n in [1_000u32, 2_000, 4_000, 8_000] {
        let bytes = verweiskette(n);
        let dauer = strip_dauer(&bytes);
        println!(
            "n = {n:>6}: strip_metadata {dauer:?} ({} kB Datei)",
            bytes.len() / 1024
        );
    }
}

/// **Decke:** 20 000 Verweisobjekte in einer Kette müssen `strip_metadata`
/// in unter 5 s durchlaufen. `Chains::of` folgt von **jedem** Verweisobjekt
/// aus der ganzen Kette bis zu ihrem Ende (`meta.rs`, `impl Chains::of`):
/// Aufwand n²/2 — bei 20 000 Objekten 2·10⁸ Schritte, jeder mit einer
/// `BTreeSet`-Einfügung. Die Datei ist 0,5 MB groß und liegt weit unter jeder
/// Grenze des Laders.
#[test]
#[ignore = "offen: Register #72 Chains::of quadratisch (224 s im Debug) — Spur-A-Runde 1, Beleg absichtlich rot"]
fn c_verweiskette_kostet_nicht_quadratisch() {
    let bytes = verweiskette(20_000);
    let dauer = strip_dauer(&bytes);
    assert!(
        dauer < Duration::from_secs(5),
        "DIENSTVERWEIGERUNG — strip_metadata braucht {dauer:?} für 20 000 verkettete \
         Verweisobjekte ({} kB); Chains::of ist quadratisch",
        bytes.len() / 1024
    );
}

// ===========================================================================
// Werkzeug: die Proben als Dateien, für den Lauf über die Kommandozeile
// ===========================================================================

/// Schreibt eine Auswahl der B-Proben nach `KORPUS_DIR` — für
/// `redact-rs x.pdf -o y.pdf --patterns iban_de` und `redact-rs y.pdf
/// --check-leaks "DE89 …"` am gebauten Binary.
#[test]
#[ignore = "Werkzeug, keine Prüfung"]
fn korpus_schreiben() {
    let dir = std::env::var("KORPUS_DIR").expect("KORPUS_DIR setzen");
    std::fs::create_dir_all(&dir).expect("Verzeichnis");
    let mut faelle: Vec<(&str, Vec<u8>)> = Vec::new();
    // /Perms /DocMDP
    {
        let mut d = probe();
        let sig = d.add(Object::Dictionary(dictionary! {
            "Type" => "Sig", "Filter" => "Adobe.PPKLite", "SubFilter" => "adbe.pkcs7.detached",
            "Name" => s("Max Mustermann"), "Reason" => s(format!("Freigabe Kontoauszug {SECRET}")),
            "Location" => s("Musterstadt"), "ContactInfo" => s("max@example.org"),
            "ByteRange" => vec![0.into(), 0.into(), 0.into(), 0.into()],
            "Contents" => Object::String(vec![0u8; 16], StringFormat::Hexadecimal),
        }));
        let feld = d.add(Object::Dictionary(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "FT" => "Sig", "T" => s("Signature1"),
            "Rect" => rect(), "F" => 132, "V" => Object::Reference(sig),
        }));
        annots(&mut d, &[feld]);
        d.catalog_set(
            "AcroForm",
            dictionary! { "Fields" => vec![Object::Reference(feld)], "SigFlags" => 3 }.into(),
        );
        d.catalog_set(
            "Perms",
            dictionary! { "DocMDP" => Object::Reference(sig) }.into(),
        );
        faelle.push(("perms_docmdp", d.finish()));
    }
    // /PageLabels
    {
        let mut d = probe();
        d.catalog_set("PageLabels", dictionary! {
            "Nums" => vec![0.into(), Object::Dictionary(dictionary! { "S" => "D", "P" => s(format!("Kontoauszug {SECRET} - ")) })],
        }.into());
        faelle.push(("pagelabels", d.finish()));
    }
    // Verwaiste Seite hinter /Dest
    {
        let mut d = probe();
        let (pages_id, resources_id) = (d.pages_id, d.resources_id);
        let inhalt = d.add(Object::Stream(Stream::new(
            dictionary! {},
            common::text_ops(&[&format!("Alte Seite: IBAN {SECRET}")]),
        )));
        let waise = d.add(Object::Dictionary(dictionary! {
            "Type" => "Page", "Parent" => Object::Reference(pages_id), "Contents" => Object::Reference(inhalt),
            "Resources" => Object::Reference(resources_id), "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        }));
        let a = d.add(Object::Dictionary(dictionary! {
            "Type" => "Annot", "Subtype" => "Link", "Rect" => rect(), "Dest" => vec![Object::Reference(waise), "Fit".into()],
        }));
        annots(&mut d, &[a]);
        faelle.push(("waise_dest", d.finish()));
    }
    // /DPartRoot
    {
        let mut d = probe();
        let root = d.doc.new_object_id();
        let page_id = d.page_id;
        let leaf = d.add(Object::Dictionary(dictionary! {
            "Type" => "DPart", "Parent" => Object::Reference(root), "Start" => Object::Reference(page_id), "End" => Object::Reference(page_id),
            "DPM" => dictionary! { "CIP4_Root" => dictionary! { "CIP4_Recipient" => dictionary! { "Account" => s(SECRET) } } },
        }));
        d.doc.objects.insert(root, Object::Dictionary(dictionary! { "Type" => "DPart", "DParts" => vec![Object::Array(vec![Object::Reference(leaf)])] }));
        d.catalog_set(
            "DPartRoot",
            dictionary! { "Type" => "DPartRoot", "DPartRootNode" => Object::Reference(root) }
                .into(),
        );
        d.page_dict_set("DPart", Object::Reference(leaf));
        faelle.push(("dpartroot", d.finish()));
    }
    // /AP einer nur über /Popup gehaltenen Annotation
    {
        let mut d = probe();
        let ap = form_mit_text(&mut d);
        let notiz = d.add(Object::Dictionary(dictionary! {
            "Type" => "Annot", "Subtype" => "FreeText", "Rect" => rect(), "Contents" => s("Notiz"),
            "AP" => dictionary! { "N" => Object::Reference(ap) },
        }));
        let popup = d.add(Object::Dictionary(dictionary! { "Type" => "Annot", "Subtype" => "Popup", "Rect" => rect(), "Parent" => Object::Reference(notiz) }));
        annots(&mut d, &[popup]);
        faelle.push(("ap_ueber_popup", d.finish()));
    }
    // Referenz-XObject mit eingebetteter Datei
    {
        let mut d = probe();
        let fs = filespec(&mut d, "alt.pdf");
        let form = d.add(Object::Stream(Stream::new(
            dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()],
                "Ref" => dictionary! { "F" => Object::Reference(fs), "Page" => 0 } },
            b"0 0 10 10 re f\n".to_vec(),
        )));
        let resources_id = d.resources_id;
        d.doc
            .get_dictionary_mut(resources_id)
            .unwrap()
            .set("XObject", dictionary! { "Fm1" => Object::Reference(form) });
        let mut raw =
            common::text_ops(&["Kontoinhaber: Max Mustermann", &format!("IBAN: {SECRET}")]);
        raw.extend_from_slice(b"q /Fm1 Do Q\n");
        d.set_content(&raw);
        faelle.push(("ref_xobject", d.finish()));
    }
    // Verweiskette (Dienstverweigerung), klein genug für einen Lauf
    faelle.push(("verweiskette_8000", verweiskette(8_000)));
    for (name, bytes) in faelle {
        std::fs::write(format!("{dir}/{name}.pdf"), &bytes).expect("schreibbar");
        println!(
            "{name}: Fundstellen in der Probe: {}",
            leaks(&bytes, SECRET).len()
        );
    }
}
