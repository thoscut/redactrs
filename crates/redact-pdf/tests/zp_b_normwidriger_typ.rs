//! Register #93 (Prüfer B-6, Spur-A-Runde 2): die Trägererkennung hing an
//! `/Type`.
//!
//! Eine Annotation trägt `/Type /Annot` oder gar kein `/Type`; ein
//! Formularfeld trägt keines. Erzeuger schreiben trotzdem `/Type
//! /Annotation` oder `/Type /Widget` — normwidrig, aber jeder Betrachter
//! zeigt die Annotation, ihren `/Contents` und das Feld mit `/T` und `/V`.
//! Der Metadatenlauf sah darin keinen Träger und ließ die Klartexte stehen.
//!
//! Jetzt gilt als Träger auch ein Dictionary mit anderem `/Type`, das die
//! Form einer Annotation (`/Subtype` und `/Rect`) oder eines Felds (`/FT`)
//! hat — außer Seite, Seitenbaum und Katalog: eine kaputte `/Parent`-Kette,
//! die dort ankommt, darf der Seite nichts nehmen.
//!
//! Orakel ist [`redact_pdf::leaks`] an den geschriebenen Bytes.

mod common;

use common::{page, Doc, SECRET};
use lopdf::{dictionary, Object, ObjectId};
use redact_pdf::{leaks, load_from_bytes, save_to_bytes, strip_metadata, MetadataReport};

fn geheim() -> Object {
    Object::string_literal(format!("Kunde {SECRET}"))
}

fn rect() -> Object {
    vec![10.into(), 10.into(), 30.into(), 30.into()].into()
}

fn lauf(d: &Doc) -> (Vec<String>, MetadataReport, lopdf::Document) {
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
fn muss_fallen(d: &Doc, was: &str) {
    let (hits, report, _) = lauf(d);
    assert!(
        hits.is_empty(),
        "{was}: STILLES LECK — der Klartext steht nach dem Lauf in der Ausgabe.\n  \
         Bericht: {:?}\n  Fundstellen:\n{}",
        report.summary(),
        hits.join("\n")
    );
    assert!(
        report.annotation_texts_cleared + report.field_values_cleared > 0,
        "{was}: der Bericht muss die Entfernung nennen: {:?}",
        report.summary()
    );
}

fn auf_die_seite(d: &mut Doc, annots: Vec<ObjectId>) {
    d.page_dict_set(
        "Annots",
        Object::Array(annots.into_iter().map(Object::Reference).collect()),
    );
}

/// `/Type /Annotation` statt `/Type /Annot`.
#[test]
fn eine_annotation_mit_type_annotation_verliert_ihren_kommentar() {
    let mut d = page(&["Hallo"]);
    let notiz = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annotation",
        "Subtype" => "Text",
        "Rect" => rect(),
        "Contents" => geheim(),
        "T" => geheim(),
    }));
    auf_die_seite(&mut d, vec![notiz]);
    muss_fallen(&d, "/Type /Annotation");
}

/// `/Type /Widget`: Feld und Widget in einem, im `/Annots` der Seite und in
/// `/Fields` des Formulars.
#[test]
fn ein_feld_mit_type_widget_verliert_name_und_wert() {
    let mut d = page(&["Hallo"]);
    let feld = d.add(Object::Dictionary(dictionary! {
        "Type" => "Widget",
        "Subtype" => "Widget",
        "FT" => "Tx",
        "Rect" => rect(),
        "T" => geheim(),
        "TU" => geheim(),
        "V" => geheim(),
    }));
    auf_die_seite(&mut d, vec![feld]);
    let acroform = d.add(Object::Dictionary(dictionary! {
        "Fields" => vec![Object::Reference(feld)],
    }));
    d.catalog_set("AcroForm", Object::Reference(acroform));
    muss_fallen(&d, "/Type /Widget");
}

/// Ein Feld nur in `/Fields`, ohne Widget auf der Seite, mit fremdem
/// `/Type` — die Form (`/FT`) entscheidet.
#[test]
fn ein_feld_mit_fremdem_type_nur_im_feldbaum_verliert_seinen_wert() {
    let mut d = page(&["Hallo"]);
    let feld = d.add(Object::Dictionary(dictionary! {
        "Type" => "Field",
        "FT" => "Tx",
        "T" => geheim(),
        "V" => geheim(),
    }));
    let widget = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Widget",
        "Rect" => rect(),
        "Parent" => Object::Reference(feld),
    }));
    d.doc
        .get_dictionary_mut(feld)
        .expect("Feld")
        .set("Kids", vec![Object::Reference(widget)]);
    auf_die_seite(&mut d, vec![widget]);
    muss_fallen(&d, "Feld mit /Type /Field über /Parent");
}

/// Gegenprobe: eine `/Parent`-Kette, die auf die Seite führt, nimmt der
/// Seite nichts, und ein Struktur-Element bleibt kein Träger.
#[test]
fn seite_und_strukturelement_bleiben_keine_traeger() {
    let mut d = page(&["Hallo"]);
    let page_id = d.page_id;
    let element = d.add(Object::Dictionary(dictionary! {
        "Type" => "StructElem",
        "S" => "P",
        "T" => Object::string_literal("Absatztitel"),
        "K" => 0,
    }));
    let notiz = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annotation",
        "Subtype" => "Text",
        "Rect" => rect(),
        "Contents" => geheim(),
        "Parent" => Object::Reference(page_id),
        "IRT" => Object::Reference(element),
    }));
    auf_die_seite(&mut d, vec![notiz]);
    let (hits, _, doc) = lauf(&d);
    assert!(hits.is_empty(), "{}", hits.join("\n"));
    let seite_id = *doc.get_pages().values().next().expect("Seite");
    let seite = doc.get_dictionary(seite_id).expect("Seite");
    for key in [&b"Contents"[..], b"Resources", b"MediaBox", b"Parent"] {
        assert!(
            seite.get(key).is_ok(),
            "/{} fehlt an der Seite",
            String::from_utf8_lossy(key)
        );
    }
    let element = doc
        .get_dictionary(element)
        .expect("Struktur-Element bleibt");
    assert!(
        element.get(b"K").is_ok() && element.get(b"S").is_ok(),
        "ein Struktur-Element ist kein Träger: {element:?}"
    );
}

/// Gegenprobe für den Ausschluss von Seite, Seitenbaum und Katalog: die
/// Seiten werden nacheinander bereinigt. Eine Notiz auf Seite 1, deren
/// `/Parent` auf Seite 2 zeigt, erreicht Seite 2, **bevor** deren
/// Erlaubnisliste ein normwidriges `/Subtype` und `/Rect` nimmt — nach der
/// Form allein wäre Seite 2 ein Träger und verlöre ihren Inhalt.
#[test]
fn eine_parent_kette_auf_eine_spaetere_seite_nimmt_ihr_nichts() {
    let mut d = page(&["Hallo"]);
    let (pages_id, resources_id, page_id) = (d.pages_id, d.resources_id, d.page_id);
    let inhalt = d.add(Object::Stream(lopdf::Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 72 700 Td (Seite zwei) Tj ET\n".to_vec(),
    )));
    let seite2 = d.add(Object::Dictionary(dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference(pages_id),
        "Contents" => Object::Reference(inhalt),
        "Resources" => Object::Reference(resources_id),
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Subtype" => "Seite",
        "Rect" => rect(),
    }));
    let pages = d.doc.get_dictionary_mut(pages_id).expect("Seitenbaum");
    pages.set(
        "Kids",
        vec![Object::Reference(page_id), Object::Reference(seite2)],
    );
    pages.set("Count", 2);
    let notiz = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annotation",
        "Subtype" => "Text",
        "Rect" => rect(),
        "Contents" => geheim(),
        "Parent" => Object::Reference(seite2),
    }));
    auf_die_seite(&mut d, vec![notiz]);
    let (hits, _, doc) = lauf(&d);
    assert!(hits.is_empty(), "{}", hits.join("\n"));
    let seiten: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    assert_eq!(seiten.len(), 2, "beide Seiten bleiben");
    let zweite = doc.get_dictionary(seiten[1]).expect("Seite 2");
    assert!(
        zweite.get(b"Contents").is_ok(),
        "Seite 2 hat ihren Inhalt verloren: {zweite:?}"
    );
}
