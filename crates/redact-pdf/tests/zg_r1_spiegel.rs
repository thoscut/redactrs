//! Gegenprüfung R1 (Fix-Runde 6): findet der umgebaute Spiegel-Aufbau noch
//! alles, und räumt `property_list_home` am richtigen Ort?
//!
//! Zwei Zusicherungen aus `94cc2ce` stehen hier auf dem Prüfstand:
//!
//! * `scan_marked_text` läuft seit Fix-Runde 6 **einmal je Strom**
//!   (`Budget::first_marked_scan`), begründet damit, der Durchlauf sei „rein
//!   syntaktisch“. Das gilt für die Klammerstruktur — nicht für die Auflösung
//!   von `/Span /MC0 BDC`: die hängt an den Ressourcen der **Platzierung**.
//!   Ein Formular ohne eigenes `/Resources` (PDF 32000-1, 8.10.1: für alte
//!   Dateien erlaubt; der Interpreter nimmt dann die des Aufrufers) sieht bei
//!   jeder Platzierung andere.
//! * `property_list_home(doc, owner, name)` sucht ab `owner`. Für ein
//!   Formular heißt das: nur in dessen eigenem `/Resources` und dessen
//!   (nicht vorhandenem) `/Parent` — nicht in den Ressourcen der Seite, aus
//!   denen der Name in Wirklichkeit aufgelöst wurde.
//!
//! Orakel überall: nach der Pipeline ist `leaks` leer; an ehrlichem Material
//! gibt es keine Spiegelwarnung.

mod common;

use common::{page, text_ops, Doc, SECRET};
use lopdf::{dictionary, Dictionary, Object, ObjectId, Stream};
use redact_core::{Action, Redaction, Region, Source, TextRun};
use redact_pdf::{
    leaks, load_from_bytes, save_to_bytes, strip_metadata, PdfExtractor, PdfRedactor,
};

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

fn text_at(y: i32, text: &str) -> String {
    format!("BT /F1 10 Tf 72 {y} Td ({}) Tj ET\n", escape(text))
}

fn analyse(bytes: &[u8]) -> (Vec<TextRun>, Vec<String>) {
    let doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("Extraktion")
}

fn mirror_warnings(warnings: &[String]) -> Vec<&String> {
    warnings
        .iter()
        .filter(|w| w.contains("Textspiegel"))
        .collect()
}

fn redaction_for(run: &TextRun, needle: &str) -> Option<Redaction> {
    let pos = run.text.find(needle)?;
    let rect = run.rect_for_byte_range(pos, pos + needle.len())?;
    Some(Redaction::new(
        Region::new(
            run.page,
            rect,
            Some(needle.to_string()),
            Source::Pattern {
                pattern_id: "iban_de".into(),
                confidence: 1.0,
            },
        ),
        Action::Blackout,
    ))
}

fn redactions_for(runs: &[TextRun], needle: &str) -> Vec<Redaction> {
    runs.iter()
        .filter_map(|run| redaction_for(run, needle))
        .collect()
}

fn pipeline(bytes: &[u8], redactions: &[Redaction]) -> (Vec<u8>, Vec<String>) {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, redactions)
        .expect("Schwärzung");
    strip_metadata(&mut doc);
    (save_to_bytes(&doc).expect("Speichern"), report.warnings)
}

/// Schwärzt jede Fundstelle von `SECRET` und liefert (Lecks, Warnungen).
fn nach_der_pipeline(bytes: &[u8]) -> (Vec<String>, Vec<String>) {
    let (runs, _) = analyse(bytes);
    let redactions = redactions_for(&runs, SECRET);
    assert!(!redactions.is_empty(), "nichts zu schwärzen");
    let (out, warnings) = pipeline(bytes, &redactions);
    (leaks(&out, SECRET), warnings)
}

/// Trägt `name → target` in `/XObject` des Ressourcenverzeichnisses `holder` ein.
fn link(d: &mut Doc, holder: ObjectId, name: &str, target: ObjectId) {
    let holder = d.doc.get_dictionary_mut(holder).expect("Ressourcen");
    let mut xobjects = holder
        .get(b"XObject")
        .and_then(|o| o.as_dict())
        .cloned()
        .unwrap_or_default();
    xobjects.set(name, target);
    holder.set("XObject", xobjects);
}

fn set_properties(d: &mut Doc, holder: ObjectId, properties: Object) {
    d.doc
        .get_dictionary_mut(holder)
        .expect("Ressourcen")
        .set("Properties", properties);
}

/// Ein Form-XObject **ohne** `/Resources` — die Ressourcen des Aufrufers
/// gelten (PDF 32000-1, 8.10.1, alte Dateien; `content::scan_operations`
/// nimmt dann `resources` des Aufrufers).
fn add_bare_form(d: &mut Doc, holder: ObjectId, name: &str, body: &str) -> ObjectId {
    let form_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
            },
            body.as_bytes().to_vec(),
        )
        .with_compression(false),
    ));
    link(d, holder, name, form_id);
    form_id
}

/// Ein Form-XObject mit eigenem Ressourcenobjekt. Liefert (Formular, Ressourcen).
fn add_form(d: &mut Doc, holder: ObjectId, name: &str, body: &str) -> (ObjectId, ObjectId) {
    let font_id = d.font_id;
    let form_resources = d.add(Object::Dictionary(
        dictionary! { "Font" => dictionary! { "F1" => font_id } },
    ));
    let form_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
                "Resources" => form_resources,
            },
            body.as_bytes().to_vec(),
        )
        .with_compression(false),
    ));
    link(d, holder, name, form_id);
    (form_id, form_resources)
}

fn mirror_list(text: &str) -> Dictionary {
    dictionary! { "ActualText" => Object::string_literal(text.to_string()) }
}

fn lie() -> String {
    format!("Zahlung an {SECRET}")
}

// ---------------------------------------------------------------------------
// 1. Formular ohne eigene Ressourcen: der Name löst sich in der Seite auf
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Liste {
    /// `/Properties <</MC0 <<…>> >>` direkt in den Seitenressourcen.
    Direkt,
    /// `/Properties 20 0 R`, darin `/MC0 <<…>>` direkt.
    PropertiesAlsVerweis,
    /// `/Properties <</MC0 12 0 R>>` — die Liste ist ein eigenes Objekt.
    AlsObjekt,
}

/// `Fm0` hat kein `/Resources` und setzt `/Span /MC0 BDC … EMC` um das
/// Geheimnis; `/MC0` steht in den Ressourcen der **Seite**.
fn formular_ohne_ressourcen(liste: Liste, spiegel: &str) -> Vec<u8> {
    let mut d = page(&[]);
    let res = d.resources_id;
    let list = mirror_list(spiegel);
    let properties = match liste {
        Liste::Direkt => Object::Dictionary(dictionary! { "MC0" => Object::Dictionary(list) }),
        Liste::PropertiesAlsVerweis => {
            let id = d.add(Object::Dictionary(
                dictionary! { "MC0" => Object::Dictionary(list) },
            ));
            Object::Reference(id)
        }
        Liste::AlsObjekt => {
            let id = d.add(Object::Dictionary(list));
            Object::Dictionary(dictionary! { "MC0" => Object::Reference(id) })
        }
    };
    set_properties(&mut d, res, properties);
    let body = format!("/Span /MC0 BDC\n{}EMC\n", text_at(600, SECRET));
    add_bare_form(&mut d, res, "Fm0", &body);
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"q /Fm0 Do Q\n");
    d.set_content(&raw);
    d.finish()
}

/// Gegenprobe: ist die Liste ein eigenes Objekt, trägt `property_id`, und
/// `clear_mirror_object` räumt unabhängig vom Fundort.
#[test]
fn formular_ohne_ressourcen_liste_als_objekt_wird_geleert() {
    let (found, warnings) = nach_der_pipeline(&formular_ohne_ressourcen(Liste::AlsObjekt, &lie()));
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

/// **Befund R1-1.** Der Spiegel wird beim Scan gefunden (über die Ressourcen
/// des Aufrufers), das Formular wird neu geschrieben — aber
/// `property_list_home(doc, form_id, "MC0")` sucht nur ab dem Formular und
/// findet die Liste in den Seitenressourcen nicht. Der Klartext bleibt dort
/// stehen; `leaks` findet ihn, ohne Warnung.
#[test]
fn formular_ohne_ressourcen_direkte_liste_in_der_seite_wird_geleert() {
    let mut offen = Vec::new();
    for liste in [Liste::Direkt, Liste::PropertiesAlsVerweis] {
        let (found, warnings) = nach_der_pipeline(&formular_ohne_ressourcen(liste, &lie()));
        if !found.is_empty() {
            offen.push(format!("{liste:?}: {found:?} (Warnungen {warnings:?})"));
        }
    }
    assert!(offen.is_empty(), "{}", offen.join("\n"));
}

// ---------------------------------------------------------------------------
// 2. Dasselbe Formular in zwei Ressourcen-Umgebungen: einmal je Strom?
// ---------------------------------------------------------------------------

/// `Fm1` (ohne `/Resources`) trägt `/Span /MC0 BDC (Geheimnis) Tj EMC`.
///
/// * Die **Seite** führt `/MC0 → 12 0 R` mit dem Spiegel (eigenes Objekt,
///   damit der Fundort keine Rolle spielt: gefunden ⇒ `clear_mirror_object`).
/// * `Fm0` hat eigene Ressourcen mit `/MC0 <</MCID 1>>` — **ohne** Spiegel —
///   und zeichnet `Fm1` versetzt.
///
/// Die Seite zeichnet beide. `spiegel_zuerst` steuert, welche Platzierung
/// von `Fm1` der Interpreter zuerst sieht.
fn zwei_umgebungen(spiegel_zuerst: bool) -> Vec<u8> {
    let mut d = page(&[]);
    let res = d.resources_id;
    let list_id = d.add(Object::Dictionary(mirror_list(&lie())));
    set_properties(
        &mut d,
        res,
        Object::Dictionary(dictionary! { "MC0" => Object::Reference(list_id) }),
    );
    let inner = add_bare_form(
        &mut d,
        res,
        "Fm1",
        &format!("/Span /MC0 BDC\n{}EMC\n", text_at(600, SECRET)),
    );
    let (_, outer_res) = add_form(&mut d, res, "Fm0", "q 1 0 0 1 0 -200 cm /Fm1 Do Q\n");
    link(&mut d, outer_res, "Fm1", inner);
    set_properties(
        &mut d,
        outer_res,
        Object::Dictionary(dictionary! {
            "MC0" => Object::Dictionary(dictionary! { "MCID" => 1_i64 })
        }),
    );
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    if spiegel_zuerst {
        raw.extend_from_slice(b"q /Fm1 Do Q\nq /Fm0 Do Q\n");
    } else {
        raw.extend_from_slice(b"q /Fm0 Do Q\nq /Fm1 Do Q\n");
    }
    d.set_content(&raw);
    d.finish()
}

/// Sieht der Interpreter zuerst die Platzierung mit dem Spiegel, wird er
/// gefunden und geleert.
#[test]
fn zwei_umgebungen_spiegel_zuerst_wird_geleert() {
    let (found, warnings) = nach_der_pipeline(&zwei_umgebungen(true));
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

/// **Befund R1-2.** Dieselbe Datei, nur die Reihenfolge der beiden `Do`
/// vertauscht: die erste Platzierung von `Fm1` läuft unter `Fm0`, wo `/MC0`
/// keinen Spiegel trägt; `first_marked_scan` ist damit verbraucht, die
/// zweite Platzierung unter der Seite wird nicht mehr abgelaufen. Der Spiegel
/// (Objekt `12 0`) bleibt mit dem Geheimnis stehen. Bis Fix-Runde 6 lief der
/// Durchlauf je Platzierung und fand ihn.
#[test]
fn zwei_umgebungen_spiegel_zuletzt_wird_auch_geleert() {
    let (found, warnings) = nach_der_pipeline(&zwei_umgebungen(false));
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

// ---------------------------------------------------------------------------
// 3. Zwei Platzierungen mit verschiedenen `cm`: Glyphenkästen
// ---------------------------------------------------------------------------

/// `Fm0` trägt Spiegel und Glyphen; die Seite zeichnet es bei (72, 600) und
/// bei (272, 300).
fn zweimal_mit_verschiedenem_cm(mirror: &str) -> Vec<u8> {
    let mut d = page(&[]);
    let res = d.resources_id;
    let body = format!(
        "/Span <</ActualText ({})>> BDC\n{}EMC\n",
        escape(mirror),
        text_at(0, SECRET)
    );
    add_form(&mut d, res, "Fm0", &body);
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"q 1 0 0 1 0 600 cm /Fm0 Do Q\n");
    raw.extend_from_slice(b"q 1 0 0 1 200 300 cm /Fm0 Do Q\n");
    d.set_content(&raw);
    d.finish()
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn zweite_platzierung_hat_ihren_eigenen_kasten_und_wird_getroffen() {
    let bytes = zweimal_mit_verschiedenem_cm(SECRET);
    let (runs, warnings) = analyse(&bytes);
    assert!(
        mirror_warnings(&warnings).is_empty(),
        "ehrlicher Spiegel: {:?}",
        mirror_warnings(&warnings)
    );
    let secret_runs: Vec<&TextRun> = runs.iter().filter(|r| r.text.contains(SECRET)).collect();
    assert_eq!(secret_runs.len(), 2, "{runs:?}");
    assert!(
        secret_runs
            .iter()
            .any(|r| near(r.rect.ll.x, 72.0, 1.0) && near(r.rect.ll.y, 600.0, 5.0)),
        "erste Platzierung fehlt: {secret_runs:?}"
    );
    let second: Vec<&TextRun> = secret_runs
        .iter()
        .copied()
        .filter(|r| near(r.rect.ll.x, 272.0, 1.0) && near(r.rect.ll.y, 300.0, 5.0))
        .collect();
    assert_eq!(second.len(), 1, "zweite Platzierung fehlt: {secret_runs:?}");

    // Nur die zweite Platzierung wird geschwärzt.
    let redactions: Vec<Redaction> = second
        .iter()
        .filter_map(|run| redaction_for(run, SECRET))
        .collect();
    let (out, _) = pipeline(&bytes, &redactions);
    let found = leaks(&out, SECRET);
    assert!(found.is_empty(), "{found:?}");
}

// ---------------------------------------------------------------------------
// 4. Einmal im Spiegel, einmal außerhalb; einmal /ActualText, einmal /Alt
// ---------------------------------------------------------------------------

fn im_und_ausserhalb(mirror: &str) -> Vec<u8> {
    let mut d = page(&[]);
    let res = d.resources_id;
    add_form(&mut d, res, "Fm0", &text_at(600, SECRET));
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(
        format!(
            "/Span <</ActualText ({})>> BDC\nq /Fm0 Do Q\nEMC\nq 1 0 0 1 0 -100 cm /Fm0 Do Q\n",
            escape(mirror)
        )
        .as_bytes(),
    );
    d.set_content(&raw);
    d.finish()
}

#[test]
fn im_und_ausserhalb_ehrlich_warnt_nicht() {
    let (_, warnings) = analyse(&im_und_ausserhalb(SECRET));
    assert!(
        mirror_warnings(&warnings).is_empty(),
        "{:?}",
        mirror_warnings(&warnings)
    );
}

#[test]
fn im_und_ausserhalb_luegender_spiegel_faellt() {
    let (found, warnings) = nach_der_pipeline(&im_und_ausserhalb(&lie()));
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

fn actualtext_und_alt(mirror: &str) -> Vec<u8> {
    let mut d = page(&[]);
    let res = d.resources_id;
    add_form(&mut d, res, "Fm0", &text_at(600, SECRET));
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(
        format!(
            "/Span <</ActualText ({m})>> BDC\nq /Fm0 Do Q\nEMC\n\
             /Figure <</Alt ({m})>> BDC\nq 1 0 0 1 0 -100 cm /Fm0 Do Q\nEMC\n",
            m = escape(mirror)
        )
        .as_bytes(),
    );
    d.set_content(&raw);
    d.finish()
}

#[test]
fn actualtext_und_alt_ehrlich_warnt_nicht() {
    let (_, warnings) = analyse(&actualtext_und_alt(SECRET));
    assert!(
        mirror_warnings(&warnings).is_empty(),
        "{:?}",
        mirror_warnings(&warnings)
    );
}

#[test]
fn actualtext_und_alt_luegend_fallen_beide() {
    let (found, warnings) = nach_der_pipeline(&actualtext_und_alt(&lie()));
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

// ---------------------------------------------------------------------------
// 5. `property_list_home`: Vererbung, geteilte Verzeichnisse, Verweise
// ---------------------------------------------------------------------------

/// `/Resources` steht als **direktes** Dictionary am Wurzelknoten; dazwischen
/// liegt ein zweiter `/Pages`-Knoten ohne Ressourcen.
fn vererbung_ueber_zwei_ebenen() -> Vec<u8> {
    let mut d = page(&[]);
    let res = d.resources_id;
    set_properties(
        &mut d,
        res,
        Object::Dictionary(dictionary! { "MC0" => Object::Dictionary(mirror_list(&lie())) }),
    );
    let resources = d.doc.get_dictionary(res).expect("Ressourcen").clone();
    let root = d.pages_id;
    let page_id = d.page_id;
    let mid = d.add(Object::Dictionary(dictionary! {
        "Type" => "Pages",
        "Parent" => root,
        "Kids" => vec![Object::Reference(page_id)],
        "Count" => 1_i64,
    }));
    let root_dict = d.doc.get_dictionary_mut(root).expect("Wurzel");
    root_dict.set("Kids", vec![Object::Reference(mid)]);
    root_dict.set("Resources", Object::Dictionary(resources));
    let page_dict = d.doc.get_dictionary_mut(page_id).expect("Seite");
    page_dict.set("Parent", Object::Reference(mid));
    page_dict.remove(b"Resources");
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(format!("/Span /MC0 BDC\n{}EMC\n", text_at(600, SECRET)).as_bytes());
    d.set_content(&raw);
    d.finish()
}

#[test]
fn geerbt_ueber_zwei_pages_ebenen_wird_geleert() {
    let (found, warnings) = nach_der_pipeline(&vererbung_ueber_zwei_ebenen());
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

/// Zwei Seiten teilen sich **ein** `/Resources`-Objekt mit `/Properties`
/// direkt darin: `/MC0` (Geheimnis, nur Seite 1) und `/MC1` (harmlos, nur
/// Seite 2).
fn geteiltes_verzeichnis() -> Vec<u8> {
    let mut d = page(&[]);
    let res = d.resources_id;
    set_properties(
        &mut d,
        res,
        Object::Dictionary(dictionary! {
            "MC0" => Object::Dictionary(mirror_list(&lie())),
            "MC1" => Object::Dictionary(mirror_list("Gruss")),
        }),
    );
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(format!("/Span /MC0 BDC\n{}EMC\n", text_at(600, SECRET)).as_bytes());
    d.set_content(&raw);

    let mut second = text_ops(&["Seite zwei ohne Geheimnis"]);
    second.extend_from_slice(format!("/Span /MC1 BDC\n{}EMC\n", text_at(600, "Gruss")).as_bytes());
    let content2 = d.add(Object::Stream(
        Stream::new(dictionary! {}, second).with_compression(false),
    ));
    let pages_id = d.pages_id;
    let page2 = d.add(Object::Dictionary(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content2,
        "Resources" => res,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    }));
    let page1 = d.page_id;
    let root = d.doc.get_dictionary_mut(pages_id).expect("Wurzel");
    root.set(
        "Kids",
        vec![Object::Reference(page1), Object::Reference(page2)],
    );
    root.set("Count", 2_i64);
    d.finish()
}

#[test]
fn geteiltes_resources_objekt_leert_mc0_und_laesst_seite_zwei_unberuehrt() {
    let bytes = geteiltes_verzeichnis();
    let (found, warnings) = nach_der_pipeline(&bytes);
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");

    let (runs, _) = analyse(&bytes);
    let redactions = redactions_for(&runs, SECRET);
    let (out, _) = pipeline(&bytes, &redactions);
    // Vergleich gegen den Strom der **Eingabe**, wie `lopdf` ihn liefert:
    // `get_page_content` hängt an jeden Teilstrom ein `\n` an, ein Vergleich
    // mit den rohen Bytes wäre schon deshalb ungleich.
    let vorher = load_from_bytes(&bytes).expect("Eingabe ladbar");
    let vor_pages: Vec<ObjectId> = vorher.get_pages().values().copied().collect();
    let second_vorher = vorher
        .get_page_content(vor_pages[1])
        .expect("Inhalt Seite 2 vorher");
    assert!(
        String::from_utf8_lossy(&second_vorher).contains("MC1"),
        "Material stimmt nicht"
    );
    let doc = load_from_bytes(&out).expect("Ausgabe ladbar");
    let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    assert_eq!(pages.len(), 2);
    let second_after = doc.get_page_content(pages[1]).expect("Inhalt Seite 2");
    assert_eq!(
        second_after, second_vorher,
        "der Strom der zweiten Seite hat sich verändert"
    );
    let resources = doc
        .get_dictionary(pages[1])
        .and_then(|p| p.get(b"Resources"))
        .and_then(|r| doc.dereference(r))
        .map(|(_, o)| o.clone())
        .expect("Ressourcen der zweiten Seite");
    let props = resources
        .as_dict()
        .and_then(|r| r.get(b"Properties"))
        .and_then(|p| p.as_dict())
        .cloned()
        .expect("Properties");
    let mc1 = props
        .get(b"MC1")
        .and_then(|o| o.as_dict())
        .expect("MC1 steht noch");
    assert!(
        mc1.get(b"ActualText").is_ok(),
        "der harmlose Spiegel /MC1 hat seinen Text verloren: {props:?}"
    );
    let mc0 = props
        .get(b"MC0")
        .and_then(|o| o.as_dict())
        .expect("MC0 steht noch");
    assert!(mc0.get(b"ActualText").is_err(), "MC0 trägt noch: {mc0:?}");
}

/// Ein Formular mit eigenen Ressourcen, dessen `/Properties` ein **Verweis**
/// auf ein Objekt ist, in dem die Liste direkt steht.
fn formular_mit_properties_verweis() -> Vec<u8> {
    let mut d = page(&[]);
    let res = d.resources_id;
    let body = format!("/Span /MC0 BDC\n{}EMC\n", text_at(600, SECRET));
    let (_, form_res) = add_form(&mut d, res, "Fm0", &body);
    let props_id = d.add(Object::Dictionary(
        dictionary! { "MC0" => Object::Dictionary(mirror_list(&lie())) },
    ));
    set_properties(&mut d, form_res, Object::Reference(props_id));
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"q /Fm0 Do Q\n");
    d.set_content(&raw);
    d.finish()
}

#[test]
fn formular_mit_properties_als_verweis_wird_geleert() {
    let (found, warnings) = nach_der_pipeline(&formular_mit_properties_verweis());
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

/// Dieselbe Liste steht **zweimal direkt**: in den Ressourcen der Seite und —
/// überschattet — in denen des `/Pages`-Knotens. Der Scan sieht die innere
/// (`merge_resources`: innen überschreibt außen), `property_list_home` räumt
/// die innere; die äußere Kopie trägt das Geheimnis weiter. Ob das eine
/// Fundstelle ist, entscheidet `leaks` — es sucht Bytes, nicht Wirksamkeit.
fn schattenkopie_im_seitenbaum() -> Vec<u8> {
    let mut d = page(&[]);
    let res = d.resources_id;
    set_properties(
        &mut d,
        res,
        Object::Dictionary(dictionary! { "MC0" => Object::Dictionary(mirror_list(&lie())) }),
    );
    let pages_id = d.pages_id;
    d.doc.get_dictionary_mut(pages_id).expect("Wurzel").set(
        "Resources",
        Object::Dictionary(dictionary! {
            "Properties" => dictionary! { "MC0" => Object::Dictionary(mirror_list(&lie())) },
        }),
    );
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(format!("/Span /MC0 BDC\n{}EMC\n", text_at(600, SECRET)).as_bytes());
    d.set_content(&raw);
    d.finish()
}

#[test]
fn schattenkopie_im_seitenbaum_bleibt_stehen_oder_nicht() {
    let (found, warnings) = nach_der_pipeline(&schattenkopie_im_seitenbaum());
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

// ---------------------------------------------------------------------------
// 6. Kachelmuster mit eigenem `/Properties`
// ---------------------------------------------------------------------------

/// Ein Kachelmuster (`/PatternType 1`) mit eigenem Ressourcenverzeichnis, in
/// dem `/Properties /MC0` **direkt** steht. Der Interpreter liest den
/// Musterstrom als `StreamKey::Form(id)`; die Schwärzung muss ihn wie ein
/// Formular behandeln — auch beim Aufräumen der Eigenschaftsliste.
fn kachelmuster_mit_eigenem_properties() -> Vec<u8> {
    let mut d = page(&[]);
    let res = d.resources_id;
    let font_id = d.font_id;
    let pattern_res = d.add(Object::Dictionary(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
        "Properties" => dictionary! { "MC0" => Object::Dictionary(mirror_list(&lie())) },
    }));
    let body = format!("/Span /MC0 BDC\n{}EMC\n", text_at(20, SECRET));
    let pattern_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "Pattern",
                "PatternType" => 1_i64,
                "PaintType" => 1_i64,
                "TilingType" => 1_i64,
                "BBox" => vec![0.into(), 0.into(), 400.into(), 100.into()],
                "XStep" => 400_i64,
                "YStep" => 100_i64,
                "Resources" => pattern_res,
            },
            body.as_bytes().to_vec(),
        )
        .with_compression(false),
    ));
    let holder = d.doc.get_dictionary_mut(res).expect("Ressourcen");
    holder.set("Pattern", dictionary! { "P0" => pattern_id });
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(b"q /Pattern cs /P0 scn 72 500 400 100 re f Q\n");
    d.set_content(&raw);
    d.finish()
}

#[test]
fn kachelmuster_mit_eigenem_properties_wird_geleert() {
    let (found, warnings) = nach_der_pipeline(&kachelmuster_mit_eigenem_properties());
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

// ---------------------------------------------------------------------------
// 7. Dieselbe Liste als Verweis **und** direkt an einer zweiten Stelle
// ---------------------------------------------------------------------------

/// Die Seite benennt `/MC0` als **Verweis** auf ein eigenes Listenobjekt;
/// dasselbe Listendictionary steht ein zweites Mal **direkt** in den
/// Ressourcen eines Formulars, das denselben Namen benutzt. Beide Abschnitte
/// verlieren Glyphen, also müssen beide Fundorte fallen — der eine über
/// `property_id`, der andere über `property_list_home`.
fn verweis_und_direkte_zweitfassung() -> Vec<u8> {
    let mut d = page(&[]);
    let res = d.resources_id;
    let list_id = d.add(Object::Dictionary(mirror_list(&lie())));
    set_properties(
        &mut d,
        res,
        Object::Dictionary(dictionary! { "MC0" => Object::Reference(list_id) }),
    );
    let body = format!("/Span /MC0 BDC\n{}EMC\n", text_at(400, SECRET));
    let (_, form_res) = add_form(&mut d, res, "Fm0", &body);
    set_properties(
        &mut d,
        form_res,
        Object::Dictionary(dictionary! { "MC0" => Object::Dictionary(mirror_list(&lie())) }),
    );
    let mut raw = text_ops(&["Kontoinhaber Max Mustermann"]);
    raw.extend_from_slice(format!("/Span /MC0 BDC\n{}EMC\n", text_at(600, SECRET)).as_bytes());
    raw.extend_from_slice(b"q /Fm0 Do Q\n");
    d.set_content(&raw);
    d.finish()
}

#[test]
fn verweis_und_direkte_zweitfassung_fallen_beide() {
    let (found, warnings) = nach_der_pipeline(&verweis_und_direkte_zweitfassung());
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}

/// Schreibt das Material der Befunde nach `R1_OUT`, für den Lauf über die
/// Kommandozeile (`redact-rs … --check-leaks "DE89 …"`).
#[test]
#[ignore = "Material"]
fn schreibt_material() {
    let Ok(dir) = std::env::var("R1_OUT") else {
        return;
    };
    for (name, bytes) in [
        (
            "r1_formular_ohne_ressourcen.pdf",
            formular_ohne_ressourcen(Liste::Direkt, &lie()),
        ),
        ("r1_zwei_umgebungen.pdf", zwei_umgebungen(false)),
        (
            "r1_formular_ohne_ressourcen_still.pdf",
            formular_ohne_ressourcen(Liste::Direkt, SECRET),
        ),
        ("r1_schattenkopie.pdf", schattenkopie_im_seitenbaum()),
    ] {
        std::fs::write(format!("{dir}/{name}"), &bytes).expect("schreibbar");
        println!("{dir}/{name}: {} B", bytes.len());
    }
}

/// Dieselbe Lücke, aber **still**: der Spiegel nennt wortgleich, was die
/// Glyphen darunter setzen. Dann gibt es keine „sagt etwas anderes“-Warnung —
/// die Schwärzung meldet „Schwärzungen: 1“ und Rückgabewert 0, und der
/// Klartext steht weiter im Ressourcenverzeichnis der Seite.
#[test]
fn formular_ohne_ressourcen_stiller_spiegel_wird_geleert() {
    let (found, warnings) = nach_der_pipeline(&formular_ohne_ressourcen(Liste::Direkt, SECRET));
    assert!(
        mirror_warnings(&warnings).is_empty(),
        "unerwartete Spiegelwarnung: {:?}",
        mirror_warnings(&warnings)
    );
    assert!(found.is_empty(), "Lecks {found:?}, Warnungen {warnings:?}");
}
