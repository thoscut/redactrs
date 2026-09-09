//! Gegenprüfung Fix-Runde 3 (G2): Annotationstexte, Ziele und Lesezeichen
//! mit eigenem Material — Verweisketten, geteilte Objekte, Objekt-Streams,
//! große und tiefe Bäume.
//!
//! Zwei Sorten Tests:
//!
//! * gewöhnliche Tests (grün; ihr Mutationsnachweis steht im Bericht des
//!   Gegenprüfers),
//! * mit `#[ignore]` markierte **Befunde**: Material, an dem das Geheimnis
//!   nach `strip_metadata` + `save_to_bytes` noch in der Datei steht. Sie
//!   laufen mit `--ignored` und sind rot, bis die Lücke geschlossen ist;
//!   danach das `#[ignore]` entfernen.
//!
//! Orakel ist [`redact_pdf::leaks`] an den geschriebenen Bytes.

mod common;

use std::time::Instant;

use common::{page, utf16be_bom, Doc, SECRET};
use lopdf::{dictionary, Object, ObjectId, Stream, StringFormat};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, strip_metadata, MetadataReport};

fn strip(bytes: &[u8]) -> (MetadataReport, Vec<u8>) {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    let report = strip_metadata(&mut doc);
    (report, save_to_bytes(&doc).expect("Speichern"))
}

#[track_caller]
fn assert_gone(bytes: &[u8], what: &str) -> MetadataReport {
    assert!(
        !leaks(bytes, SECRET).is_empty(),
        "{what}: die Probe muss das Geheimnis vorher tragen, sonst misst der Test nichts"
    );
    let (report, out) = strip(bytes);
    let hits = leaks(&out, SECRET);
    assert!(
        hits.is_empty(),
        "{what}: nach der Verarbeitung steht die IBAN noch in der Datei:\n{}",
        hits.join("\n")
    );
    report
}

fn rect() -> Object {
    Object::Array(vec![400.into(), 100.into(), 500.into(), 120.into()])
}

fn explicit_destination(page_id: ObjectId) -> Object {
    Object::Array(vec![
        Object::Reference(page_id),
        Object::Name(b"XYZ".to_vec()),
        72.into(),
        700.into(),
        Object::Real(1.5),
    ])
}

fn note(contents: Object) -> lopdf::Dictionary {
    dictionary! {
        "Type" => "Annot",
        "Subtype" => "Text",
        "Rect" => rect(),
        "Contents" => contents,
    }
}

fn secret_string() -> Object {
    Object::string_literal(format!("Notiz {SECRET}"))
}

// ---------------------------------------------------------------------------
// A) Verweisketten um Annotationen herum
// ---------------------------------------------------------------------------

/// `/Annots 7 0 R` — das Feld selbst als eigenes Objekt, wie es viele
/// Erzeuger schreiben.
#[test]
fn annots_hinter_einem_verweis_werden_bereinigt() {
    let mut d = page(&["Harmloser Text"]);
    let annot_id = d.add(Object::Dictionary(note(secret_string())));
    let annots_id = d.add(Object::Array(vec![Object::Reference(annot_id)]));
    d.page_dict_set("Annots", Object::Reference(annots_id));
    let report = assert_gone(&d.finish(), "/Annots als Verweis");
    assert_eq!(report.annotation_texts_cleared, 1);
}

/// `/Annots 7 0 R`, Objekt 7 ist `8 0 R`, Objekt 8 das Feld; darin `9 0 R`,
/// Objekt 9 ist `10 0 R`, Objekt 10 die Annotation. Jeder Betrachter löst
/// solche Ketten auf.
#[test]
fn annots_und_annotation_hinter_verweisketten_werden_bereinigt() {
    let mut d = page(&["Harmloser Text"]);
    let annot_id = d.add(Object::Dictionary(note(secret_string())));
    let annot_ref = d.add(Object::Reference(annot_id));
    let annots_id = d.add(Object::Array(vec![Object::Reference(annot_ref)]));
    let annots_ref = d.add(Object::Reference(annots_id));
    d.page_dict_set("Annots", Object::Reference(annots_ref));
    let report = assert_gone(&d.finish(), "/Annots hinter Verweis auf Verweis");
    assert_eq!(report.annotation_texts_cleared, 1);
    let (_, out) = strip(&d.finish());
    let doc = load_from_bytes(&out).expect("Ausgabe ladbar");
    assert!(
        !doc.get_dictionary(annot_id).expect("Annotation").has(b"Contents"),
        "/Contents steht noch an der Annotation hinter der Kette"
    );
}

/// Ein Widget, das nur über `/Kids` eines Formularfeldes hängt und nicht in
/// `/Annots` steht, fällt mit `/AcroForm` (unerreichbar → `prune_unreachable`).
#[test]
fn widget_nur_ueber_kids_des_acroform_faellt_mit_dem_formular() {
    let mut d = page(&["Harmloser Text"]);
    let field_id = d.doc.new_object_id();
    let widget_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Widget",
        "Rect" => rect(),
        "Parent" => field_id,
        "TU" => Object::string_literal(format!("Konto von {SECRET}")),
    }));
    d.doc.objects.insert(
        field_id,
        Object::Dictionary(dictionary! {
            "FT" => "Tx",
            "T" => Object::string_literal("iban"),
            "Kids" => vec![Object::Reference(widget_id)],
        }),
    );
    d.catalog_set(
        "AcroForm",
        Object::Dictionary(dictionary! { "Fields" => vec![Object::Reference(field_id)] }),
    );
    let report = assert_gone(&d.finish(), "Widget nur über /Kids");
    assert!(report.acroform_removed);
    // Nicht in `/Annots` → nicht als Annotation gezählt; es fällt mit dem Baum.
    assert_eq!(report.annotation_texts_cleared, 0);
}

/// `/Dest 12 0 R`, Objekt 12 ist `13 0 R`, Objekt 13 das Feld — bleibt und
/// führt weiter auf das Feld.
#[test]
fn dest_hinter_zweistufigem_verweis_bleibt_und_fuehrt_auf_das_feld() {
    let mut d = page(&["Harmloser Text"]);
    let dest_id = d.add(explicit_destination(d.page_id));
    let dest_ref = d.add(Object::Reference(dest_id));
    let annot_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Link",
        "Rect" => rect(),
        "Dest" => Object::Reference(dest_ref),
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(annot_id)]));
    let (report, out) = strip(&d.finish());
    assert_eq!(report.annotation_actions_removed, 0);
    let doc = load_from_bytes(&out).expect("Ausgabe ladbar");
    let annot = doc.get_dictionary(annot_id).expect("Annotation");
    let dest = annot.get(b"Dest").expect("das Ziel bleibt");
    let (_, target) = doc.dereference(dest).expect("Ziel auflösbar");
    let items = target.as_array().expect("Feld");
    assert_eq!(items.first(), Some(&Object::Reference(doc.get_pages()[&1])));
}

/// `/Dest 12 0 R` mit `12 0 obj 12 0 R` — die Auflösung endet, das Ziel
/// fällt (es führt nirgendwohin), das Objekt verschwindet.
#[test]
fn zirkulaerer_dest_verweis_endet_und_faellt() {
    let mut d = page(&["Harmloser Text"]);
    let loop_id = d.doc.new_object_id();
    d.doc
        .objects
        .insert(loop_id, Object::Reference(loop_id));
    let annot_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Link",
        "Rect" => rect(),
        "Dest" => Object::Reference(loop_id),
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(annot_id)]));
    let (report, out) = strip(&d.finish());
    assert_eq!(report.annotation_actions_removed, 1);
    let doc = load_from_bytes(&out).expect("Ausgabe ladbar");
    assert!(!doc.get_dictionary(annot_id).expect("Annotation").has(b"Dest"));
    assert!(!doc.objects.contains_key(&loop_id));
}

/// Ein Feld, dessen erstes Element ein fremder Name statt des Seitenverweises
/// ist, fällt (ein Name kann Text tragen); eines aus lauter Anzeigenamen
/// bleibt — es trägt keinen Text.
#[test]
fn dest_feld_mit_fremdem_namen_vorn_faellt() {
    // Ein Name kennt keine Leerzeichen — gesucht wird die IBAN kompakt.
    let compact = SECRET.replace(' ', "");
    let mut d = page(&["Harmloser Text"]);
    let fremd = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Link",
        "Rect" => rect(),
        "Dest" => vec![
            Object::Name(format!("Konto_{compact}").into_bytes()),
            Object::Name(b"XYZ".to_vec()),
            0.into(), 0.into(), 0.into(),
        ],
    }));
    let nur_anzeigenamen = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Link",
        "Rect" => rect(),
        "Dest" => vec![Object::Name(b"Fit".to_vec()), Object::Name(b"XYZ".to_vec())],
    }));
    d.page_dict_set(
        "Annots",
        Object::Array(vec![Object::Reference(fremd), Object::Reference(nur_anzeigenamen)]),
    );
    let bytes = d.finish();
    assert!(!leaks(&bytes, &compact).is_empty(), "Probe trägt die IBAN nicht");
    let (report, out) = strip(&bytes);
    assert!(leaks(&out, &compact).is_empty(), "{:?}", leaks(&out, &compact));
    assert_eq!(report.annotation_actions_removed, 1);
    let doc = load_from_bytes(&out).expect("Ausgabe ladbar");
    assert!(!doc.get_dictionary(fremd).unwrap().has(b"Dest"));
    assert!(doc.get_dictionary(nur_anzeigenamen).unwrap().has(b"Dest"));
}

/// `/Contents` als Hex-String und als UTF-16BE mit BOM — beide fallen.
#[test]
fn contents_als_hex_und_als_utf16_fallen() {
    let mut d = page(&["Harmloser Text"]);
    let hex = d.add(Object::Dictionary(note(Object::String(
        format!("Notiz {SECRET}").into_bytes(),
        StringFormat::Hexadecimal,
    ))));
    let utf16 = d.add(Object::Dictionary(note(Object::String(
        utf16be_bom(&format!("Notiz {SECRET}")),
        StringFormat::Literal,
    ))));
    d.page_dict_set(
        "Annots",
        Object::Array(vec![Object::Reference(hex), Object::Reference(utf16)]),
    );
    let bytes = d.finish();
    // Beide Formen müssen vorher gefunden werden — sonst misst der Test nur eine.
    let vorher = leaks(&bytes, SECRET);
    assert!(vorher.iter().any(|h| h.contains("UTF-16")), "{vorher:?}");
    assert!(vorher.iter().any(|h| h.contains("hex")), "{vorher:?}");
    let report = assert_gone(&bytes, "/Contents hex und UTF-16BE");
    assert_eq!(report.annotation_texts_cleared, 2);
}

/// `/RC 9 0 R` auf einen Strom (Rich Text als Text-Stream, PDF 32000-1,
/// 12.5.6.2 erlaubt „text string or text stream“).
#[test]
fn rc_als_strom_faellt_samt_objekt() {
    let mut d = page(&["Harmloser Text"]);
    let rc_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {},
            format!("<body><p>Notiz {SECRET}</p></body>").into_bytes(),
        )
        .with_compression(false),
    ));
    let annot_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "FreeText",
        "Rect" => rect(),
        "RC" => Object::Reference(rc_id),
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(annot_id)]));
    let report = assert_gone(&d.finish(), "/RC als Strom");
    assert_eq!(report.annotation_texts_cleared, 1);
    let (_, out) = strip(&d.finish());
    let doc = load_from_bytes(&out).expect("Ausgabe ladbar");
    assert!(!doc.objects.contains_key(&rc_id));
}

/// Ein Aktionsobjekt, das eine Annotation (`/A 5 0 R`), eine zweite
/// Annotation und der Katalog (`/OpenAction 5 0 R`) teilen: das Objekt fällt,
/// und niemand behält einen hängenden Verweis darauf.
#[test]
fn geteilte_aktion_mit_openaction_faellt_ohne_haengenden_verweis() {
    let mut d = page(&["Harmloser Text"]);
    let action_id = d.add(Object::Dictionary(dictionary! {
        "S" => "URI",
        "URI" => Object::string_literal(format!("mailto:x@example.org?subject={SECRET}")),
    }));
    let a = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Link", "Rect" => rect(),
        "A" => Object::Reference(action_id),
    }));
    let b = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Link", "Rect" => rect(),
        "A" => Object::Reference(action_id),
    }));
    d.page_dict_set(
        "Annots",
        Object::Array(vec![Object::Reference(a), Object::Reference(b)]),
    );
    d.catalog_set("OpenAction", Object::Reference(action_id));
    let report = assert_gone(&d.finish(), "/A geteilt mit /OpenAction");
    assert!(report.open_action_removed);
    assert_eq!(report.annotation_actions_removed, 2);
    let (_, out) = strip(&d.finish());
    let doc = load_from_bytes(&out).expect("Ausgabe ladbar");
    let catalog_id = match doc.trailer.get(b"Root").unwrap() {
        Object::Reference(id) => *id,
        _ => unreachable!(),
    };
    assert!(!doc.get_dictionary(catalog_id).unwrap().has(b"OpenAction"));
    assert!(!doc.get_dictionary(a).unwrap().has(b"A"));
    assert!(!doc.get_dictionary(b).unwrap().has(b"A"));
    assert!(!doc.objects.contains_key(&action_id));
}

/// Der Bericht zählt **Schlüssel**, nicht Annotationen: eine Notiz mit vier
/// Texten meldet „4 Kommentartexte“.
#[test]
fn bericht_zaehlt_schluessel_nicht_annotationen() {
    let mut d = page(&["Harmloser Text"]);
    let annot_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Text",
        "Rect" => rect(),
        "Contents" => Object::string_literal("a"),
        "RC" => Object::string_literal("<p>a</p>"),
        "T" => Object::string_literal("Max"),
        "Subj" => Object::string_literal("Notiz"),
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(annot_id)]));
    let (report, _) = strip(&d.finish());
    assert_eq!(report.annotation_texts_cleared, 4);
    assert!(
        report
            .summary()
            .iter()
            .any(|l| l.starts_with("4 Kommentartexte an Annotationen")),
        "{:?}",
        report.summary()
    );
}

/// Gegenrichtung: eine gewöhnliche Datei mit Verweisen auf ausdrückliche
/// Ziele (direkt und als eigenes Objekt) meldet nichts und bleibt
/// navigierbar.
#[test]
fn links_auf_ausdrueckliche_ziele_bleiben_und_der_bericht_ist_leer() {
    let mut d = page(&["Harmloser Text"]);
    let dest_id = d.add(explicit_destination(d.page_id));
    let direct = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Link", "Rect" => rect(),
        "Border" => vec![0.into(), 0.into(), 0.into()],
        "Dest" => explicit_destination(d.page_id),
    }));
    let indirect = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Link", "Rect" => rect(),
        "Dest" => Object::Reference(dest_id),
    }));
    d.page_dict_set(
        "Annots",
        Object::Array(vec![Object::Reference(direct), Object::Reference(indirect)]),
    );
    let (report, out) = strip(&d.finish());
    assert!(!report.anything_removed(), "{:?}", report.summary());
    let doc = load_from_bytes(&out).expect("Ausgabe ladbar");
    let page_id = doc.get_pages()[&1];
    for id in [direct, indirect] {
        let dest = doc.get_dictionary(id).unwrap().get(b"Dest").unwrap().clone();
        let (_, target) = doc.dereference(&dest).unwrap();
        assert_eq!(
            target.as_array().unwrap().first(),
            Some(&Object::Reference(page_id)),
            "der Sprung führt nicht mehr auf die Seite"
        );
    }
}

// ---------------------------------------------------------------------------
// B) Lesezeichen
// ---------------------------------------------------------------------------

fn last_startxref(bytes: &[u8]) -> Option<usize> {
    let key = b"startxref";
    let pos = bytes
        .windows(key.len())
        .enumerate()
        .rfind(|(_, w)| *w == key)
        .map(|(i, _)| i)?;
    let digits: String = bytes[pos + key.len()..]
        .iter()
        .skip_while(|b| b.is_ascii_whitespace())
        .take_while(|b| b.is_ascii_digit())
        .map(|&b| b as char)
        .collect();
    digits.parse().ok()
}

/// Hängt eine Revision mit klassischer xref-Sektion und `/Prev` an.
fn append_revision(
    mut out: Vec<u8>,
    root: ObjectId,
    size: u32,
    objects: &[(u32, Vec<u8>)],
) -> Vec<u8> {
    let prev = last_startxref(&out).expect("startxref");
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    let mut offsets = Vec::new();
    for (id, body) in objects {
        offsets.push((*id, out.len()));
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref_offset = out.len();
    let mut xref = String::from("xref\n0 1\n0000000000 65535 f \n");
    for (id, offset) in &offsets {
        xref.push_str(&format!("{id} 1\n{offset:010} 00000 n \n"));
    }
    xref.push_str(&format!(
        "trailer\n<</Size {size} /Root {} {} R /Prev {prev}>>\nstartxref\n{xref_offset}\n%%EOF\n",
        root.0, root.1
    ));
    out.extend_from_slice(xref.as_bytes());
    out
}

/// `/Outlines` und sein Eintrag liegen in einem `/ObjStm`; der Katalog wird
/// in einer angehängten Revision auf sie umgebogen.
#[test]
fn lesezeichen_in_einem_objektstrom_fallen() {
    let d = page(&["Harmloser Text"]);
    let catalog_id = d.catalog_id;
    let pages_id = d.pages_id;
    let page_id = d.page_id;
    // +1 vergibt `save_to` selbst für den XRef-Stream.
    let objstm_id = d.doc.max_id + 2;
    let outlines_id = objstm_id + 1;
    let item_id = objstm_id + 2;
    let base = d.finish();

    let o1 = format!("<</Type/Outlines/First {item_id} 0 R/Last {item_id} 0 R/Count 1>>");
    let o2 = format!(
        "<</Title(IBAN {SECRET})/Parent {outlines_id} 0 R/Dest[{} 0 R/Fit]>>",
        page_id.0
    );
    let header = format!("{outlines_id} 0 {item_id} {} ", o1.len() + 1);
    let mut plain = header.clone().into_bytes();
    plain.extend_from_slice(o1.as_bytes());
    plain.push(b'\n');
    plain.extend_from_slice(o2.as_bytes());
    let mut body = format!(
        "<</Type/ObjStm/N 2/First {}/Length {}>>\nstream\n",
        header.len(),
        plain.len()
    )
    .into_bytes();
    body.extend_from_slice(&plain);
    body.extend_from_slice(b"\nendstream");

    let catalog = format!(
        "<</Type/Catalog/Pages {} 0 R/Outlines {outlines_id} 0 R>>",
        pages_id.0
    );
    let filler_id = item_id + 10;
    let bytes = append_revision(
        base,
        catalog_id,
        filler_id + 1,
        &[
            (objstm_id, body),
            (catalog_id.0, catalog.into_bytes()),
            (filler_id, b"<</Type/Platzhalter>>".to_vec()),
        ],
    );
    // Die Probe ist, was sie sein soll: der Eintrag ist ausgepackt erreichbar.
    let probe = load_from_bytes(&bytes).expect("ladbar");
    assert!(probe.get_dictionary((item_id, 0)).is_ok(), "Eintrag nicht ausgepackt");
    let report = assert_gone(&bytes, "/Outlines im Objekt-Stream");
    assert_eq!(report.outlines_removed, 1);
}

/// Baut einen Lesezeichenbaum mit `n` Einträgen: `flach` = eine `/Next`-Kette,
/// sonst eine `/First`-Kette in die Tiefe. Der letzte Eintrag trägt das
/// Geheimnis.
fn outline_chain(d: &mut Doc, n: usize, flach: bool) -> ObjectId {
    let outlines_id = d.doc.new_object_id();
    let ids: Vec<ObjectId> = (0..n).map(|_| d.doc.new_object_id()).collect();
    for (k, id) in ids.iter().enumerate() {
        let mut dict = dictionary! {
            "Title" => Object::string_literal(if k + 1 == n {
                format!("IBAN {SECRET}")
            } else {
                format!("Kapitel {k}")
            }),
            "Parent" => if flach || k == 0 { outlines_id } else { ids[k - 1] },
        };
        if k + 1 < n {
            dict.set(if flach { "Next" } else { "First" }, Object::Reference(ids[k + 1]));
        }
        d.doc.objects.insert(*id, Object::Dictionary(dict));
    }
    d.doc.objects.insert(
        outlines_id,
        Object::Dictionary(dictionary! {
            "Type" => "Outlines",
            "First" => ids[0],
            "Last" => ids[n - 1],
            "Count" => n as i64,
        }),
    );
    d.catalog_set("Outlines", Object::Reference(outlines_id));
    outlines_id
}

/// 100 000 Einträge in einer Kette (breit) und 100 000 in der Tiefe: beide
/// enden zügig, und nichts bleibt in der Datei.
#[test]
fn hunderttausend_lesezeichen_enden_zuegig() {
    for flach in [true, false] {
        let mut d = page(&["Harmloser Text"]);
        outline_chain(&mut d, 100_000, flach);
        let start = Instant::now();
        let report = strip_metadata(&mut d.doc);
        let strip_dauer = start.elapsed();
        let out = save_to_bytes(&d.doc).expect("Speichern");
        let gesamt = start.elapsed();
        let hits = leaks(&out, SECRET);
        println!(
            "{}: strip {:?}, mit Speichern {:?}, gezählt {}, Ausgabe {} Byte",
            if flach { "breit" } else { "tief" },
            strip_dauer,
            gesamt,
            report.outlines_removed,
            out.len()
        );
        assert!(hits.is_empty(), "{}", hits.join("\n"));
        assert!(gesamt.as_secs() < 60, "zu langsam: {gesamt:?}");
        if flach {
            assert_eq!(report.outlines_removed, 100_000);
        }
    }
}

// ---------------------------------------------------------------------------
// Befunde — rot, bis die Lücke geschlossen ist
// ---------------------------------------------------------------------------

/// Ein `/Popup`, das nur über den `/Popup`-Schlüssel der Notiz hängt (nicht
/// in `/Annots`), bleibt erreichbar und behält sein `/Contents`.
#[test]
#[ignore = "Befund G2-1: Popup außerhalb von /Annots behält /Contents"]
fn popup_nur_ueber_den_popup_schluessel_traegt_weiter() {
    let mut d = page(&["Harmloser Text"]);
    let popup_id = d.doc.new_object_id();
    let note_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Text", "Rect" => rect(),
        "Contents" => Object::string_literal("Notiz"),
        "Popup" => popup_id,
    }));
    d.doc.objects.insert(
        popup_id,
        Object::Dictionary(dictionary! {
            "Type" => "Annot", "Subtype" => "Popup", "Rect" => rect(),
            "Parent" => note_id,
            "Contents" => Object::string_literal(format!("Popup {SECRET}")),
        }),
    );
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(note_id)]));
    assert_gone(&d.finish(), "/Popup nur über /Popup erreichbar");
}

/// Ein Widget in `/Annots` mit `/Parent` auf ein Feld, das nicht selbst
/// Annotation ist (Textfeld mit mehreren Widgets, Radiogruppe): das Feld
/// bleibt über `/Parent` erreichbar, und `key` darauf bleibt stehen.
fn parent_field_with(key: &str, value: Object) -> Vec<u8> {
    let mut d = page(&["Harmloser Text"]);
    let field_id = d.doc.new_object_id();
    let widget_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "Rect" => rect(),
        "Parent" => field_id,
    }));
    let mut field = dictionary! {
        "FT" => "Tx",
        "T" => Object::string_literal("feld"),
        "Kids" => vec![Object::Reference(widget_id)],
        "V" => Object::string_literal("wert"),
    };
    field.set(key, value);
    d.doc.objects.insert(field_id, Object::Dictionary(field));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(widget_id)]));
    d.catalog_set(
        "AcroForm",
        Object::Dictionary(dictionary! { "Fields" => vec![Object::Reference(field_id)] }),
    );
    d.finish()
}

#[test]
#[ignore = "Befund G2-2: /TU am Elternfeld eines Widgets bleibt"]
fn tooltip_am_elternfeld_eines_widgets_traegt_weiter() {
    assert_gone(
        &parent_field_with("TU", Object::string_literal(format!("Konto von {SECRET}"))),
        "/TU am Elternfeld",
    );
}

#[test]
#[ignore = "Befund G2-2: /T am Elternfeld eines Widgets bleibt"]
fn feldname_am_elternfeld_eines_widgets_traegt_weiter() {
    assert_gone(
        &parent_field_with("T", Object::string_literal(format!("Konto {SECRET}"))),
        "/T am Elternfeld",
    );
}

#[test]
#[ignore = "Befund G2-3: /Opt (Auswahlliste) am Feld bleibt"]
fn auswahlliste_am_elternfeld_traegt_weiter() {
    assert_gone(
        &parent_field_with(
            "Opt",
            Object::Array(vec![Object::string_literal(SECRET), Object::string_literal("andere")]),
        ),
        "/Opt am Elternfeld",
    );
}

#[test]
#[ignore = "Befund G2-3: /AA mit JavaScript am Elternfeld bleibt"]
fn ereignisaktion_am_elternfeld_traegt_weiter() {
    assert_gone(
        &parent_field_with(
            "AA",
            Object::Dictionary(dictionary! {
                "K" => dictionary! {
                    "S" => "JavaScript",
                    "JS" => Object::string_literal(format!("if (event.value == '{SECRET}') {{}}")),
                },
            }),
        ),
        "/AA /K /JS am Elternfeld",
    );
}

/// Weitere Klartextträger **an der Annotation selbst**, die nicht in
/// `ANNOTATION_TEXT_KEYS` stehen.
fn annotation_with(subtype: &str, key: &str, value: Object) -> Vec<u8> {
    let mut d = page(&["Harmloser Text"]);
    let mut annot = dictionary! {
        "Type" => "Annot",
        "Subtype" => Object::Name(subtype.as_bytes().to_vec()),
        "Rect" => rect(),
    };
    annot.set(key, value);
    let id = d.add(Object::Dictionary(annot));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(id)]));
    d.finish()
}

#[test]
#[ignore = "Befund G2-3: /Opt an einem Auswahl-Widget bleibt"]
fn auswahlliste_am_widget_traegt_weiter() {
    let mut d = page(&["Harmloser Text"]);
    let id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "Rect" => rect(),
        "FT" => "Ch", "Ff" => 131072_i64, "T" => Object::string_literal("bank"),
        "Opt" => vec![Object::string_literal(SECRET), Object::string_literal("andere")],
        "V" => Object::string_literal(SECRET),
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(id)]));
    assert_gone(&d.finish(), "/Opt am Widget");
}

#[test]
#[ignore = "Befund G2-3: /MK /CA (Beschriftung) an einem Widget bleibt"]
fn beschriftung_mk_ca_am_widget_traegt_weiter() {
    assert_gone(
        &annotation_with(
            "Widget",
            "MK",
            Object::Dictionary(dictionary! { "CA" => Object::string_literal(format!("Konto {SECRET}")) }),
        ),
        "/MK /CA",
    );
}

#[test]
#[ignore = "Befund G2-3: /OverlayText an einer Redact-Annotation bleibt"]
fn overlaytext_einer_redact_annotation_traegt_weiter() {
    assert_gone(
        &annotation_with("Redact", "OverlayText", Object::string_literal(format!("war: {SECRET}"))),
        "/OverlayText",
    );
}

#[test]
#[ignore = "Befund G2-3: /PA (URI-Aktion eines Links, Tabelle 173) bleibt"]
fn pa_aktion_eines_links_traegt_weiter() {
    assert_gone(
        &annotation_with(
            "Link",
            "PA",
            Object::Dictionary(dictionary! {
                "S" => "URI",
                "URI" => Object::string_literal(format!("mailto:x@example.org?subject={SECRET}")),
            }),
        ),
        "/PA",
    );
}

#[test]
#[ignore = "Befund G2-3: /NM (Annotationsname) bleibt"]
fn nm_traegt_weiter() {
    assert_gone(
        &annotation_with("Text", "NM", Object::string_literal(format!("Notiz {SECRET}"))),
        "/NM",
    );
}

#[test]
#[ignore = "Befund G2-3: /DS (Default Style) bleibt"]
fn ds_traegt_weiter() {
    assert_gone(
        &annotation_with("FreeText", "DS", Object::string_literal(format!("font: {SECRET}"))),
        "/DS",
    );
}

#[test]
#[ignore = "Befund G2-3: /DA (Default Appearance) bleibt"]
fn da_traegt_weiter() {
    assert_gone(
        &annotation_with("FreeText", "DA", Object::string_literal(format!("/{SECRET} 0 Tf"))),
        "/DA",
    );
}

/// `/Dest [12 0 R /XYZ 0 0 0]`, Objekt 12 ist eine Zeichenkette: ein Verweis
/// im Feld wird nicht aufgelöst, das Feld gilt als ausdrückliches Ziel und
/// hält die Zeichenkette erreichbar.
#[test]
#[ignore = "Befund G2-4: Verweis im Zielfeld wird nicht aufgelöst"]
fn dest_feld_mit_verweis_auf_eine_zeichenkette_traegt_weiter() {
    let mut d = page(&["Harmloser Text"]);
    let string_id = d.add(Object::string_literal(format!("IBAN {SECRET}")));
    let annot_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Link", "Rect" => rect(),
        "Dest" => vec![
            Object::Reference(string_id),
            Object::Name(b"XYZ".to_vec()),
            0.into(), 0.into(), 0.into(),
        ],
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(annot_id)]));
    assert_gone(&d.finish(), "/Dest [String-Verweis /XYZ …]");
}

/// `take` löscht das referenzierte Objekt, auch wenn es geteilt ist:
/// `/Contents 4 0 R` auf den Seiteninhalt nimmt der Seite ihren Inhalt.
#[test]
#[ignore = "Befund G2-5: take() löscht geteilte Objekte (Seiteninhalt)"]
fn annotation_contents_als_verweis_auf_den_seiteninhalt_loescht_die_seite() {
    let mut d = page(&["Harmloser Text"]);
    let content_id = d.content_id;
    let annot_id = d.add(Object::Dictionary(note(Object::Reference(content_id))));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(annot_id)]));
    let (_, out) = strip(&d.finish());
    let doc = load_from_bytes(&out).expect("Ausgabe ladbar");
    let page_id = doc.get_pages()[&1];
    let contents = doc.get_dictionary(page_id).unwrap().get(b"Contents").unwrap().clone();
    assert!(
        doc.dereference(&contents).is_ok(),
        "der Seiteninhalt ist mit dem Annotationstext gelöscht worden"
    );
}

/// Ein `/Outlines /First` auf die Seite selbst (kaputte Datei): der
/// Lesezeichenlauf löscht die Seite.
#[test]
#[ignore = "Befund G2-5: Lesezeichenlauf löscht, worauf /First zeigt — auch die Seite"]
fn outlines_first_auf_die_seite_loescht_die_seite() {
    let mut d = page(&["Harmloser Text"]);
    let page_id = d.page_id;
    let outlines_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Outlines", "First" => page_id, "Last" => page_id,
    }));
    d.catalog_set("Outlines", Object::Reference(outlines_id));
    let (_, out) = strip(&d.finish());
    let doc = load_from_bytes(&out).expect("Ausgabe ladbar");
    assert_eq!(doc.get_pages().len(), 1, "die Seite ist weg");
}

/// Ein Baum tiefer als `MAX_TREE_DEPTH` (32): der Lauf bricht ab und zählt
/// die Geschwister der oberen Ebenen nicht mehr — die Objekte fallen zwar
/// (unerreichbar), aber der Bericht nennt zu wenige.
#[test]
#[ignore = "Befund G2-6: Zählung bricht bei Tiefe > 32 ab (break statt continue)"]
fn tiefer_baum_zaehlt_die_geschwister_der_oberen_ebene_nicht() {
    let mut d = page(&["Harmloser Text"]);
    let outlines_id = d.doc.new_object_id();
    let a = d.doc.new_object_id();
    let b = d.doc.new_object_id();
    // A hat einen Nachbarn B (Ebene 0) und eine 40 Stufen tiefe /First-Kette.
    let mut chain: Vec<ObjectId> = (0..40).map(|_| d.doc.new_object_id()).collect();
    for (k, id) in chain.iter().enumerate() {
        let mut dict = dictionary! { "Title" => Object::string_literal(format!("Stufe {k}")) };
        if k + 1 < chain.len() {
            dict.set("First", Object::Reference(chain[k + 1]));
        }
        d.doc.objects.insert(*id, Object::Dictionary(dict));
    }
    d.doc.objects.insert(
        a,
        Object::Dictionary(dictionary! {
            "Title" => Object::string_literal("A"), "Parent" => outlines_id,
            "Next" => b, "First" => chain[0],
        }),
    );
    d.doc.objects.insert(
        b,
        Object::Dictionary(dictionary! {
            "Title" => Object::string_literal(format!("IBAN {SECRET}")), "Parent" => outlines_id,
        }),
    );
    d.doc.objects.insert(
        outlines_id,
        Object::Dictionary(dictionary! { "Type" => "Outlines", "First" => a, "Last" => b }),
    );
    d.catalog_set("Outlines", Object::Reference(outlines_id));
    chain.clear();
    let report = assert_gone(&d.finish(), "tiefer Baum");
    assert_eq!(report.outlines_removed, 42, "2 Einträge auf Ebene 0 und 40 Stufen");
}
