//! Gegenprüfung G1 (Fix-Runde 3), Gebiet A: Textspiegel über Formularen.
//!
//! Grüne Tests belegen, was die Korrektur hält; die mit `#[ignore]`
//! markierten Tests sind **Befunde** — sie sind rot am Stand `76bdcf9` und
//! bleiben rot, bis die Ursache behoben ist (`cargo test … -- --ignored`).
//!
//! Orakel ist [`redact_pdf::leaks`] auf der gespeicherten Datei, nie der
//! Extraktor allein; die Extraktion liefert nur die Warnungen und die Läufe,
//! aus denen die Schwärzungen gebildet werden.

mod common;

use common::{page, Doc, SECRET};
use lopdf::{dictionary, Object, ObjectId, Stream};
use redact_core::{Action, Redaction, Region, Source, TextRun};
use redact_pdf::{
    leaks, load_from_bytes, save_to_bytes, strip_metadata, PdfExtractor, PdfRedactor,
};

fn analyse(bytes: &[u8]) -> (Vec<TextRun>, Vec<String>) {
    let doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("Extraktion")
}

/// Die vollständige Verarbeitung; zurück kommen Datei und Berichtwarnungen.
fn pipeline(bytes: &[u8], redactions: &[Redaction]) -> (Vec<u8>, Vec<String>) {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, redactions)
        .expect("Schwärzung");
    strip_metadata(&mut doc);
    (save_to_bytes(&doc).expect("Speichern"), report.warnings)
}

/// Schwärzungen für jeden Lauf, der `needle` trägt — wahlweise nur auf einer
/// Seite (Seitenbereich, Handauswahl).
fn redactions_for(runs: &[TextRun], needle: &str, only_page: Option<usize>) -> Vec<Redaction> {
    runs.iter()
        .filter(|run| only_page.is_none_or(|p| run.page == p))
        .filter_map(|run| {
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
        })
        .collect()
}

fn mirror_warnings(warnings: &[String]) -> Vec<&String> {
    warnings.iter().filter(|w| w.contains("Textspiegel")).collect()
}

/// Form-XObject `name` mit `body`, in `resources` eingetragen. Die Ressourcen
/// des Formulars sind ein **eigenes** Objekt, damit weitere Formulare
/// hineingehängt werden können. Liefert (Formular, dessen Ressourcen).
fn add_form(d: &mut Doc, resources: ObjectId, name: &str, body: &str) -> (ObjectId, ObjectId) {
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
    let target = d.doc.get_dictionary_mut(resources).expect("Resources");
    let mut xobjects = target
        .get(b"XObject")
        .and_then(|o| o.as_dict())
        .cloned()
        .unwrap_or_default();
    xobjects.set(name, form_id);
    target.set("XObject", xobjects);
    (form_id, form_resources)
}

fn text_form(body: &str) -> String {
    format!("BT /F1 10 Tf 72 700 Td {body} ET")
}

/// Weitere Seite mit eigenem Inhalt und den Ressourcen von Seite 1.
fn add_page(d: &mut Doc, content: &[u8]) -> ObjectId {
    let content_id = d.add(Object::Stream(Stream::new(dictionary! {}, content.to_vec())));
    let resources = d.resources_id;
    let pages_id = d.pages_id;
    let page_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    }));
    let pages = d.doc.get_dictionary_mut(pages_id).expect("Pages");
    let mut kids = pages
        .get(b"Kids")
        .and_then(|k| k.as_array())
        .cloned()
        .expect("Kids");
    kids.push(page_id.into());
    let count = kids.len() as i64;
    pages.set("Kids", kids);
    pages.set("Count", count);
    page_id
}

/// Lügender Spiegel: genau eine Warnung, ein eigener Lauf, nach der
/// Schwärzung kein Leck.
fn assert_lying_mirror_is_cleared(bytes: &[u8]) {
    let (runs, warnings) = analyse(bytes);
    assert_eq!(mirror_warnings(&warnings).len(), 1, "{warnings:?}");
    let redactions = redactions_for(&runs, SECRET, None);
    assert!(!redactions.is_empty(), "der Spiegel ist ein eigener Lauf");
    let (out, _) = pipeline(bytes, &redactions);
    let found = leaks(&out, SECRET);
    assert!(found.is_empty(), "{found:?}");
}

// ---------------------------------------------------------------------------
// Hält
// ---------------------------------------------------------------------------

/// Ein Spiegel über **zwei** Formularen (`/Fm0 Do … /Fm1 Do` in einer
/// Klammer), der lügt: beide Formulare zählen zum Geltungsbereich, der
/// Spiegel wird gelesen, gemeldet und geleert.
#[test]
fn spiegel_ueber_zwei_formularen_wird_geleert() {
    let mut d = page(&[]);
    let r = d.resources_id;
    add_form(&mut d, r, "Fm0", &text_form("(Kontodaten) Tj"));
    add_form(&mut d, r, "Fm1", &text_form("(folgen unten) Tj"));
    d.set_content(
        format!(
            "/Span << /ActualText (IBAN: {SECRET}) >> BDC /Fm0 Do q 1 0 0 1 80 0 cm /Fm1 Do Q EMC\n"
        )
        .as_bytes(),
    );
    assert_lying_mirror_is_cleared(&d.finish());
}

/// Dasselbe Formular zweimal auf einer Seite — einmal unter einem lügenden
/// Spiegel, einmal frei. Das Formular wird einmal neu geschrieben; der
/// Spiegel muss mit.
#[test]
fn formular_zweimal_platziert_einmal_im_spiegel() {
    let mut d = page(&[]);
    let r = d.resources_id;
    add_form(&mut d, r, "Fm0", &text_form("(Kontodaten folgen unten) Tj"));
    d.set_content(
        format!(
            "q 1 0 0 1 0 -100 cm /Fm0 Do Q\n/Span << /ActualText (IBAN: {SECRET}) >> BDC /Fm0 Do EMC\n"
        )
        .as_bytes(),
    );
    assert_lying_mirror_is_cleared(&d.finish());
}

/// Gegenrichtung: derselbe Aufbau mit ehrlichem Spiegel — keine Warnung,
/// kein Leck nach der Schwärzung beider Platzierungen.
#[test]
fn formular_zweimal_platziert_ehrlich_bleibt_still() {
    let mut d = page(&[]);
    let r = d.resources_id;
    add_form(&mut d, r, "Fm0", &text_form(&format!("(IBAN: {SECRET}) Tj")));
    d.set_content(
        format!(
            "q 1 0 0 1 0 -100 cm /Fm0 Do Q\n/Span << /ActualText (IBAN: {SECRET}) >> BDC /Fm0 Do EMC\n"
        )
        .as_bytes(),
    );
    let bytes = d.finish();
    let (runs, warnings) = analyse(&bytes);
    assert!(warnings.is_empty(), "{warnings:?}");
    let (out, _) = pipeline(&bytes, &redactions_for(&runs, SECRET, None));
    assert!(leaks(&out, SECRET).is_empty());
}

/// `/ActualText` als indirekter Verweis **im Formular**: die
/// Eigenschaftsliste als Objekt (`/Span /P0 BDC`), und der Wert selbst als
/// Verweis auf eine Zeichenkette. Beides wird gelesen und geleert.
#[test]
fn actualtext_als_verweis_in_einem_formular_wird_geleert() {
    for value_is_reference in [false, true] {
        let mut d = page(&[]);
        let r = d.resources_id;
        let (form_id, form_resources) = add_form(&mut d, r, "Fm0", "");
        let text = Object::string_literal(format!("IBAN: {SECRET}"));
        let value = if value_is_reference {
            Object::Reference(d.add(text))
        } else {
            text
        };
        let props = d.add(Object::Dictionary(dictionary! { "ActualText" => value }));
        d.doc
            .get_dictionary_mut(form_resources)
            .expect("Ressourcen")
            .set("Properties", dictionary! { "P0" => props });
        if let Ok(Object::Stream(s)) = d.doc.get_object_mut(form_id) {
            s.set_content(
                b"BT /F1 10 Tf 72 700 Td /Span /P0 BDC (Kontodaten folgen unten) Tj EMC ET"
                    .to_vec(),
            );
        }
        d.set_content(b"/Fm0 Do\n");
        assert_lying_mirror_is_cleared(&d.finish());
    }
}

/// Verschachtelte Klammern mit drei verschiedenen Schlüsseln, alle ehrlich
/// bzw. beschreibend: keine Warnung, das Geheimnis wird über die Glyphen
/// gefunden und entfernt.
#[test]
fn verschachtelte_klammern_mit_verschiedenen_schluesseln_bleiben_still() {
    let mut d = page(&[]);
    d.set_content(
        format!(
            "BT /F1 10 Tf 72 700 Td \
             /Span << /Alt (Kontonummer) >> BDC \
             /Span << /E (Internationale Bankkontonummer) >> BDC (IBAN) Tj EMC \
             /Span << /ActualText ({SECRET}) >> BDC ( {SECRET}) Tj EMC \
             EMC ET"
        )
        .as_bytes(),
    );
    let bytes = d.finish();
    let (runs, warnings) = analyse(&bytes);
    assert!(warnings.is_empty(), "{warnings:?}");
    let (out, _) = pipeline(&bytes, &redactions_for(&runs, SECRET, None));
    assert!(leaks(&out, SECRET).is_empty());
}

/// Spiegel mit eigenen Glyphen **und** einem Formular, in beiden
/// Reihenfolgen — die Glyphen stehen in Stromreihenfolge, der Spiegel ist
/// deckungsgleich, keine Warnung.
#[test]
fn spiegel_mit_eigenen_glyphen_und_formular_in_beiden_reihenfolgen() {
    for form_first in [false, true] {
        let mut d = page(&[]);
        let r = d.resources_id;
        let content = if form_first {
            add_form(&mut d, r, "Fm0", &text_form("(IBAN: DE89 3704) Tj"));
            format!(
                "/Span << /ActualText (IBAN: {SECRET}) >> BDC /Fm0 Do \
                 BT /F1 10 Tf 150 700 Td (0044 0532 0130 00) Tj ET EMC"
            )
        } else {
            add_form(&mut d, r, "Fm0", &text_form("(0044 0532 0130 00) Tj"));
            format!(
                "BT /F1 10 Tf 72 700 Td /Span << /ActualText (IBAN: {SECRET}) >> BDC \
                 (IBAN: DE89 3704 ) Tj ET /Fm0 Do EMC"
            )
        };
        d.set_content(content.as_bytes());
        let (_, warnings) = analyse(&d.finish());
        assert!(
            mirror_warnings(&warnings).is_empty(),
            "form_first={form_first}: {warnings:?}"
        );
    }
}

/// Gewöhnliche getaggte Datei: `/P <</MCID>>`, `/Figure <</Alt>>` über einer
/// Zeichnung (Formular ohne Text), `/Span <</ActualText>>` zweimal über
/// demselben Flate-komprimierten Formular. Keine Warnung, weder in der
/// Analyse noch im Bericht einer Schwärzung ohne Treffer.
#[test]
fn gewoehnliche_getaggte_datei_bleibt_still() {
    let mut d = page(&[]);
    let r = d.resources_id;
    add_form(&mut d, r, "Fm0", "0 0 m 100 100 l S");
    let (form_id, _) = add_form(&mut d, r, "Fm1", &text_form("(Kontoinhaber) Tj"));
    if let Ok(Object::Stream(s)) = d.doc.get_object_mut(form_id) {
        s.compress().expect("komprimierbar");
    }
    d.set_content(
        b"/P << /MCID 0 >> BDC BT /F1 10 Tf 72 700 Td (Hallo Welt) Tj ET EMC\n\
          /Figure << /Alt (Logo der Bank) >> BDC /Fm0 Do EMC\n\
          /Span << /ActualText (Kontoinhaber) >> BDC /Fm1 Do EMC\n\
          /Span << /ActualText (Kontoinhaber) >> BDC /Fm1 Do EMC\n",
    );
    let bytes = d.finish();
    let (_, warnings) = analyse(&bytes);
    assert!(warnings.is_empty(), "{warnings:?}");
    let (_, report_warnings) = pipeline(&bytes, &[]);
    assert!(report_warnings.is_empty(), "{report_warnings:?}");
}

// ---------------------------------------------------------------------------
// Befunde — rot am Stand 76bdcf9
// ---------------------------------------------------------------------------

/// **Befund G1-A1.** Ein Spiegel **im Formular** `Fm0` über `/Fm1 Do`, die
/// Glyphen in `Fm1`. Der Spiegel wird gelesen und gemeldet; die Schwärzung
/// trifft die Glyphen in `Fm1` — aber `Fm0` hat keinen eigenen Plan und
/// steht deshalb nicht in `form_ids` (`redact.rs`, Schleife über
/// `form_plans.keys()`): es wird nie neu geschrieben, sein Spiegel bleibt
/// mit dem Geheimnis stehen. Gelesen, aber nicht geleert — das neue Leck,
/// das die Doku zu `mirrors_to_clear` ausschließen wollte.
#[test]
#[ignore = "Befund G1-A1: Spiegel im Formular über innerem Formular wird nicht geleert"]
fn befund_spiegel_im_formular_ueber_innerem_formular_bleibt_stehen() {
    let mut d = page(&[]);
    let r = d.resources_id;
    let (_, outer_resources) = add_form(
        &mut d,
        r,
        "Fm0",
        &format!("/Span << /ActualText (IBAN: {SECRET}) >> BDC /Fm1 Do EMC"),
    );
    add_form(
        &mut d,
        outer_resources,
        "Fm1",
        &text_form("(Kontodaten folgen unten) Tj"),
    );
    d.set_content(b"/Fm0 Do\n");
    assert_lying_mirror_is_cleared(&d.finish());
}

/// **Befund G1-A2.** Ein Formular auf zwei Seiten, auf Seite 1 unter einem
/// **ehrlichen** Spiegel. Die Schwärzung wird nur für Seite 2 angefordert
/// (Seitenbereich, Handauswahl). Das Formular verliert seine Glyphen — auf
/// beiden Seiten —, der Spiegel auf Seite 1 bleibt mit der IBAN stehen. Die
/// einzige Warnung („wirkt deshalb auch auf die anderen Seiten“) steht in
/// `redact_pipeline::coverage::NOT_A_COVERAGE_GAP`: Rückgabewert 0, Leck.
#[test]
#[ignore = "Befund G1-A2: Spiegel auf einer nicht geschwärzten Seite überlebt die Schwärzung des geteilten Formulars"]
fn befund_geteiltes_formular_spiegel_auf_anderer_seite_bleibt_stehen() {
    let mut d = page(&[]);
    let r = d.resources_id;
    add_form(&mut d, r, "Fm0", &text_form(&format!("(IBAN: {SECRET}) Tj")));
    d.set_content(format!("/Span << /ActualText (IBAN: {SECRET}) >> BDC /Fm0 Do EMC\n").as_bytes());
    add_page(&mut d, b"/Fm0 Do\n");
    let bytes = d.finish();
    let (runs, warnings) = analyse(&bytes);
    assert!(warnings.is_empty(), "{warnings:?}");
    let redactions = redactions_for(&runs, SECRET, Some(1));
    assert_eq!(redactions.len(), 1, "nur Seite 2");
    let (out, report_warnings) = pipeline(&bytes, &redactions);
    let found = leaks(&out, SECRET);
    assert!(
        found.is_empty(),
        "Leck: {found:?}\nBerichtwarnungen: {report_warnings:?}"
    );
}

/// **Befund G1-A3 (klein).** Ein Formular, das erst ein inneres Formular
/// zeichnet und dann eigenen Text — unter einem deckungsgleichen Spiegel.
/// Die Glyphenfolge des Datensatzes stellt die eigenen Glyphen vor die des
/// inneren Formulars (dokumentierte Grenze in `extract::mirror_runs`); die
/// ehrliche Datei bekommt eine Spiegelwarnung und damit Rückgabewert 3.
#[test]
#[ignore = "Befund G1-A3: falsche Spiegelwarnung, wenn ein Formular sein inneres Formular vor dem eigenen Text zeichnet"]
fn befund_inneres_formular_vor_eigenem_text_gibt_falsche_warnung() {
    let mut d = page(&[]);
    let r = d.resources_id;
    let (_, outer_resources) = add_form(
        &mut d,
        r,
        "Fm0",
        "/Fm1 Do BT /F1 10 Tf 150 700 Td (0044 0532 0130 00) Tj ET",
    );
    add_form(
        &mut d,
        outer_resources,
        "Fm1",
        &text_form("(IBAN: DE89 3704) Tj"),
    );
    d.set_content(format!("/Span << /ActualText (IBAN: {SECRET}) >> BDC /Fm0 Do EMC\n").as_bytes());
    let (_, warnings) = analyse(&d.finish());
    assert!(
        mirror_warnings(&warnings).is_empty(),
        "ehrliche Datei: {warnings:?}"
    );
}
