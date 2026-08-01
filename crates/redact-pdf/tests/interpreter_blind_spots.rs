//! Blinde Flecken des Content-Interpreters — und der Beweis, dass sie zu sind.
//!
//! Gemessen wird nicht mit dem eigenen Extraktor (das wäre ein Zirkelschluss:
//! wovor er blind ist, sieht auch der Test nicht), sondern mit
//! [`redact_pdf::leaks`]: dem Detektor, der die fertige Datei auf allen Ebenen
//! durchsucht.
//!
//! Zwei Sorten Zusicherung:
//!
//! * **Geschlossen** — der Text wird gefunden *und* die geschwärzte Datei
//!   enthält das Geheimnis nirgends mehr (Inline-Bild, Kachelmuster,
//!   Annotation).
//! * **Gemeldet** — der Interpreter kommt nicht weiter (XObject ohne
//!   `/Subtype`, unbekannter Filter, zu tiefe Verschachtelung) und sagt es.
//!   Ein stilles `continue` an diesen Stellen hieße: „0 Schwärzungen“, Exit 0,
//!   und der Nutzer hält eine Datei für sauber, in der alles stehen geblieben
//!   ist.

mod common;

use common::SECRET;
use lopdf::{dictionary, Document, Object, Stream};
use redact_core::{Action, Extractor, Redaction, Region, Source, TextRun};
use redact_pdf::{
    content::scan_page, leaks, load_from_bytes, save_to_bytes, strip_metadata, PdfExtractor,
    PdfRedactor, ScanResult,
};

// ---------------------------------------------------------------------------
// Werkzeug
// ---------------------------------------------------------------------------

fn scan(bytes: &[u8]) -> ScanResult {
    let doc = load_from_bytes(bytes).expect("PDF ladbar");
    let page_id = *doc.get_pages().values().next().expect("eine Seite");
    scan_page(&doc, page_id).expect("Scan")
}

fn extract(bytes: &[u8]) -> Vec<TextRun> {
    let doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfExtractor::new().extract(&doc).expect("Extraktion")
}

fn extracted_text(bytes: &[u8]) -> String {
    extract(bytes)
        .iter()
        .map(|r| r.text.as_str())
        .collect::<Vec<_>>()
        .join("|")
}

/// Die vollständige Verarbeitung, so wie das Werkzeug sie fährt.
fn pipeline(bytes: &[u8], redactions: &[Redaction]) -> Vec<u8> {
    let mut doc = load_from_bytes(bytes).expect("PDF ladbar");
    PdfRedactor::new()
        .apply_with_report(&mut doc, redactions)
        .expect("Schwärzung");
    strip_metadata(&mut doc);
    save_to_bytes(&doc).expect("Speichern")
}

/// Schwärzung für ein gefundenes Textstück; `None`, wenn die Analyse es gar
/// nicht sieht.
fn redaction_for(runs: &[TextRun], needle: &str) -> Option<Redaction> {
    runs.iter().find_map(|run| {
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
}

#[track_caller]
fn assert_no_leak(bytes: &[u8], what: &str) {
    let hits = leaks(bytes, SECRET);
    assert!(
        hits.is_empty(),
        "{what}: „{SECRET}“ steht noch {} mal in der Ausgabe:\n{}",
        hits.len(),
        hits.join("\n")
    );
}

/// Vorbedingung jedes Szenarios: das Geheimnis steht wirklich in der Datei.
#[track_caller]
fn assert_precondition_leaks(bytes: &[u8], what: &str) {
    assert!(
        !leaks(bytes, SECRET).is_empty(),
        "Testdaten taugen nicht ({what}): das Geheimnis steht gar nicht in der Datei"
    );
}

#[track_caller]
fn assert_warns(warnings: &[String], fragment: &str) {
    assert!(
        warnings.iter().any(|w| w.contains(fragment)),
        "keine Warnung mit „{fragment}“; gemeldet wurde: {warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Bausteine
// ---------------------------------------------------------------------------

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

/// Ein Content-Stream, der genau eine Zeile setzt.
fn text_at(x: f64, y: f64, text: &str) -> Vec<u8> {
    format!("BT\n/F1 10 Tf\n{x} {y} Td\n({}) Tj\nET\n", escape(text)).into_bytes()
}

fn int_array(values: &[i64]) -> Vec<Object> {
    values.iter().map(|v| Object::Integer(*v)).collect()
}

fn set_resource(doc: &mut Document, resources_id: lopdf::ObjectId, key: &str, value: Object) {
    doc.get_dictionary_mut(resources_id)
        .expect("Resources")
        .set(key, value);
}

// ---------------------------------------------------------------------------
// #25 — Inline-Bild vor weiterem Text
// ---------------------------------------------------------------------------

#[test]
fn text_behind_an_inline_image_is_extracted_and_redacted() {
    let pdf = common::inline_image_before_text(SECRET);
    assert_precondition_leaks(&pdf, "Inline-Bild");

    // 1. Der Lesepfad kommt am Inline-Bild vorbei.
    let runs = extract(&pdf);
    let text = runs
        .iter()
        .map(|r| r.text.as_str())
        .collect::<Vec<_>>()
        .join("|");
    assert!(
        text.contains(SECRET),
        "Text hinter dem Inline-Bild bleibt unsichtbar: {text:?}"
    );

    // 2. Und was gefunden wird, wird auch entfernt — ohne den unbeteiligten
    //    Text davor mitzureißen.
    let redaction = redaction_for(&runs, SECRET).expect("IBAN gefunden");
    let out = pipeline(&pdf, &[redaction]);
    assert_no_leak(&out, "Text hinter Inline-Bild");
    assert!(
        extracted_text(&out).contains("Kontoinhaber"),
        "unbeteiligter Text ging beim Neuschreiben verloren: {:?}",
        extracted_text(&out)
    );
}

/// Auch im Form-XObject: ein Inline-Bild **im Formular** darf den Rest des
/// Formularstroms nicht verschlucken.
#[test]
fn text_behind_an_inline_image_inside_a_form_is_extracted() {
    let mut d = common::page(&["Kontoinhaber: Max Mustermann"]);
    let font_id = d.font_id;

    let mut form_content = Vec::new();
    form_content.extend_from_slice(b"q 20 0 0 20 300 780 cm\n");
    form_content.extend_from_slice(b"BI /W 2 /H 2 /CS /G /BPC 8 ID ");
    form_content.extend_from_slice(&[0x00, 0xff, 0x7f, 0x30]);
    form_content.extend_from_slice(b" EI Q\n");
    form_content.extend_from_slice(&text_at(72.0, 640.0, &format!("IBAN: {SECRET}")));

    let form_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => int_array(&[0, 0, 595, 842]),
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            },
            form_content,
        )
        .with_compression(false),
    ));
    let resources_id = d.resources_id;
    set_resource(
        &mut d.doc,
        resources_id,
        "XObject",
        Object::Dictionary(dictionary! { "Fm0" => form_id }),
    );
    let mut raw = common::text_ops(&["Kontoinhaber: Max Mustermann"]);
    raw.extend_from_slice(b"q /Fm0 Do Q\n");
    d.set_content(&raw);
    let pdf = d.finish();

    assert_precondition_leaks(&pdf, "Inline-Bild im Form-XObject");
    let runs = extract(&pdf);
    let redaction = redaction_for(&runs, SECRET).expect("IBAN hinter dem Inline-Bild im Formular");
    let out = pipeline(&pdf, &[redaction]);
    assert_no_leak(&out, "Text hinter Inline-Bild im Form-XObject");
}

// ---------------------------------------------------------------------------
// #29a — Form-XObject ohne /Subtype
// ---------------------------------------------------------------------------

fn form_without_subtype(secret: &str) -> Vec<u8> {
    let mut d = common::page(&["Kontoinhaber: Max Mustermann"]);
    let font_id = d.font_id;
    let form_id = d.add(Object::Stream(
        Stream::new(
            // Bewusst ohne /Subtype: der Interpreter stieg hier lautlos aus.
            dictionary! {
                "Type" => "XObject",
                "BBox" => int_array(&[0, 0, 595, 842]),
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            },
            text_at(72.0, 640.0, &format!("IBAN: {secret}")),
        )
        .with_compression(false),
    ));
    let resources_id = d.resources_id;
    set_resource(
        &mut d.doc,
        resources_id,
        "XObject",
        Object::Dictionary(dictionary! { "Fm0" => form_id }),
    );
    let mut raw = common::text_ops(&["Kontoinhaber: Max Mustermann"]);
    raw.extend_from_slice(b"q /Fm0 Do Q\n");
    d.set_content(&raw);
    d.finish()
}

#[test]
fn a_form_without_subtype_is_reported_not_swallowed() {
    let pdf = form_without_subtype(SECRET);
    assert_precondition_leaks(&pdf, "XObject ohne /Subtype");

    let scan = scan(&pdf);
    // Belegt, dass der Fleck echt ist: der Text darin wird nicht gesehen.
    let seen: String = scan
        .shows
        .iter()
        .flat_map(|s| s.glyphs())
        .map(|g| g.text.as_str())
        .collect();
    assert!(
        !seen.contains(SECRET),
        "Testdaten taugen nicht: der Inhalt wird doch gelesen"
    );
    assert_warns(&scan.warnings, "/Subtype");
    assert_warns(&scan.warnings, "Fm0");
}

// ---------------------------------------------------------------------------
// #29b — Form-XObject mit nicht dekodierbarem Filter
// ---------------------------------------------------------------------------

fn form_with_unsupported_filter(secret: &str) -> Vec<u8> {
    let mut d = common::page(&["Kontoinhaber: Max Mustermann"]);
    let font_id = d.font_id;
    // `ASCIIHexDecode` kann `lopdf` beim Auspacken nicht — ein Filter, an dem
    // der Interpreter tatsächlich hängen bleibt.
    let encoded = common::ascii_hex_encode(&text_at(72.0, 640.0, &format!("IBAN: {secret}")));
    let form_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "Filter" => Object::Name(b"ASCIIHexDecode".to_vec()),
                "BBox" => int_array(&[0, 0, 595, 842]),
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            },
            encoded,
        )
        .with_compression(false),
    ));
    let resources_id = d.resources_id;
    set_resource(
        &mut d.doc,
        resources_id,
        "XObject",
        Object::Dictionary(dictionary! { "Fm0" => form_id }),
    );
    let mut raw = common::text_ops(&["Kontoinhaber: Max Mustermann"]);
    raw.extend_from_slice(b"q /Fm0 Do Q\n");
    d.set_content(&raw);
    d.finish()
}

#[test]
fn an_undecodable_form_filter_is_reported_not_swallowed() {
    let pdf = form_with_unsupported_filter(SECRET);
    assert_precondition_leaks(&pdf, "Form mit ASCIIHexDecode");

    let scan = scan(&pdf);
    assert_warns(&scan.warnings, "dekodieren");
    assert_warns(&scan.warnings, "Fm0");
}

// ---------------------------------------------------------------------------
// #29c — Text in einem Kachelmuster
// ---------------------------------------------------------------------------

fn tiling_pattern_with_text(secret: &str) -> Vec<u8> {
    let mut d = common::page(&["Kontoinhaber: Max Mustermann"]);
    let font_id = d.font_id;
    let pattern_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "Pattern",
                "PatternType" => 1_i64,
                "PaintType" => 1_i64,
                "TilingType" => 1_i64,
                "BBox" => int_array(&[0, 0, 300, 40]),
                "XStep" => 300_i64,
                "YStep" => 40_i64,
                // Der Musterraum sitzt auf der Seite bei (72|600).
                "Matrix" => int_array(&[1, 0, 0, 1, 72, 600]),
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            },
            text_at(0.0, 0.0, &format!("IBAN: {secret}")),
        )
        .with_compression(false),
    ));
    let resources_id = d.resources_id;
    set_resource(
        &mut d.doc,
        resources_id,
        "Pattern",
        Object::Dictionary(dictionary! { "P1" => pattern_id }),
    );
    let mut raw = common::text_ops(&["Kontoinhaber: Max Mustermann"]);
    raw.extend_from_slice(b"q /Pattern cs /P1 scn 60 590 400 60 re f Q\n");
    d.set_content(&raw);
    d.finish()
}

#[test]
fn text_in_a_tiling_pattern_is_found_reported_and_redacted() {
    let pdf = tiling_pattern_with_text(SECRET);
    assert_precondition_leaks(&pdf, "Kachelmuster");

    // 1. Der Musterstrom wird durchlaufen — an der Stelle der ersten Kachel.
    let runs = extract(&pdf);
    let hit = runs
        .iter()
        .find(|r| r.text.contains(SECRET))
        .expect("Text im Kachelmuster wird gefunden");
    assert!(
        (hit.rect.ll.x - 72.0).abs() < 2.0 && (hit.rect.ll.y - 600.0).abs() < 4.0,
        "Kachel falsch verortet: {:?}",
        hit.rect
    );

    // 2. Trotzdem wird gewarnt: die weiteren Kacheln werden nicht vermessen.
    assert_warns(&scan(&pdf).warnings, "Kachelmuster");

    // 3. Und der Text verschwindet wirklich aus dem Musterstrom.
    let redaction = redaction_for(&runs, SECRET).expect("IBAN im Kachelmuster");
    let out = pipeline(&pdf, &[redaction]);
    assert_no_leak(&out, "Text im Kachelmuster");
}

// ---------------------------------------------------------------------------
// #29d — Verschachtelung tiefer als die Grenze
// ---------------------------------------------------------------------------

/// `levels` ineinandergeschachtelte Form-XObjects; jede Ebene setzt eine
/// Marke „Ebene N“, die innerste zusätzlich das Geheimnis.
fn nested_forms(secret: &str, levels: usize) -> Vec<u8> {
    let mut d = common::page(&["Kontoinhaber: Max Mustermann"]);
    let font_id = d.font_id;

    let mut inner: Option<lopdf::ObjectId> = None;
    for level in (1..=levels).rev() {
        let mut content = text_at(72.0, 700.0 - 12.0 * level as f64, &format!("Ebene {level}"));
        if level == levels {
            content.extend_from_slice(&text_at(300.0, 400.0, &format!("IBAN: {secret}")));
        }
        let mut resources = dictionary! { "Font" => dictionary! { "F1" => font_id } };
        if let Some(child) = inner {
            resources.set("XObject", dictionary! { "Fm" => child });
            content.extend_from_slice(b"q /Fm Do Q\n");
        }
        let stream = Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => int_array(&[0, 0, 595, 842]),
                "Resources" => resources,
            },
            content,
        )
        .with_compression(false);
        inner = Some(d.add(Object::Stream(stream)));
    }

    let resources_id = d.resources_id;
    set_resource(
        &mut d.doc,
        resources_id,
        "XObject",
        Object::Dictionary(dictionary! { "Fm0" => inner.expect("mindestens eine Ebene") }),
    );
    let mut raw = common::text_ops(&["Kontoinhaber: Max Mustermann"]);
    raw.extend_from_slice(b"q /Fm0 Do Q\n");
    d.set_content(&raw);
    d.finish()
}

#[test]
fn nesting_beyond_the_limit_is_reported_not_swallowed() {
    let pdf = nested_forms(SECRET, 12);
    assert_precondition_leaks(&pdf, "12-fache Verschachtelung");

    let scan = scan(&pdf);
    let seen: String = scan
        .shows
        .iter()
        .flat_map(|s| s.glyphs())
        .map(|g| g.text.as_str())
        .collect();

    // Bis zur Grenze wird gelesen …
    assert!(
        seen.contains("Ebene 8"),
        "die Ebenen bis zur Grenze fehlen: {seen:?}"
    );
    // … dahinter nicht mehr — und genau das wird gemeldet.
    assert!(
        !seen.contains(SECRET),
        "Testdaten taugen nicht: die tiefste Ebene wird doch gelesen"
    );
    assert_warns(&scan.warnings, "verschachtelt");
}

#[test]
fn nesting_within_the_limit_stays_quiet() {
    let scan = scan(&nested_forms(SECRET, 4));
    assert!(
        scan.warnings.is_empty(),
        "unerwartete Warnung: {:?}",
        scan.warnings
    );
}

// ---------------------------------------------------------------------------
// #29 — der Weg der Warnungen nach draußen
// ---------------------------------------------------------------------------

#[test]
fn warnings_reach_the_extractor_api() {
    // `extract_with_warnings` ist der Weg, auf dem Aufrufer außerhalb dieses
    // Crates die Befunde bekommen. Was der Interpreter meldet, muss dort
    // ankommen — sonst versandet es und die CLI meldet nur „0 Schwärzungen“.
    let doc = load_from_bytes(&form_without_subtype(SECRET)).expect("PDF ladbar");
    let (_runs, warnings) = PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("Extraktion");
    assert_warns(&warnings, "/Subtype");

    let doc = load_from_bytes(&nested_forms(SECRET, 12)).expect("PDF ladbar");
    let (_runs, warnings) = PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("Extraktion");
    assert_warns(&warnings, "verschachtelt");
}

// ---------------------------------------------------------------------------
// #31 — Text in Annotationen (/FreeText, /Contents, /AP)
// ---------------------------------------------------------------------------

fn free_text_annotation(secret: &str) -> Vec<u8> {
    let mut d = common::page(&["Kontoinhaber: Max Mustermann"]);
    let font_id = d.font_id;
    let ap_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => int_array(&[0, 0, 240, 20]),
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            },
            text_at(0.0, 4.0, &format!("IBAN: {secret}")),
        )
        .with_compression(false),
    ));
    let annot_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "FreeText",
        "Rect" => int_array(&[300, 400, 540, 420]),
        "F" => 4_i64,
        "Contents" => Object::string_literal(format!("IBAN: {secret}")),
        "AP" => dictionary! { "N" => ap_id },
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(annot_id)]));
    d.finish()
}

#[test]
fn free_text_annotation_is_extracted_at_its_place_and_redacted() {
    let pdf = free_text_annotation(SECRET);
    assert_precondition_leaks(&pdf, "/FreeText-Annotation");

    // 1. Der Text der Annotation wird überhaupt gesehen …
    let runs = extract(&pdf);
    let hit = runs
        .iter()
        .find(|r| r.text.contains(SECRET))
        .expect("Text der /FreeText-Annotation wird gefunden");

    // 2. … und zwar dort, wo die Annotation auf der Seite steht (/Rect mit der
    //    Abbildung aus Algorithmus 8.1, nicht im Ursprung des Formularraums).
    assert!(
        hit.rect.ll.x >= 299.0
            && hit.rect.ur.x <= 541.0
            && hit.rect.ll.y >= 399.0
            && hit.rect.ur.y <= 421.0,
        "Annotationstext liegt außerhalb von /Rect: {:?}",
        hit.rect
    );

    // 3. Ein Treffer dort schwärzt auch wirklich — samt /Contents.
    let redaction = redaction_for(&runs, SECRET).expect("IBAN in der Annotation");
    let out = pipeline(&pdf, &[redaction]);
    assert_no_leak(&out, "/FreeText-Annotation");
}

#[test]
fn an_appearance_state_dictionary_is_walked_too() {
    // `/AP /N` darf statt eines Stroms ein Dictionary von Zuständen sein.
    let mut d = common::page(&["Kontoinhaber: Max Mustermann"]);
    let font_id = d.font_id;
    let on_id = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => int_array(&[0, 0, 240, 20]),
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            },
            text_at(0.0, 4.0, &format!("IBAN: {SECRET}")),
        )
        .with_compression(false),
    ));
    let annot_id = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Widget",
        "Rect" => int_array(&[300, 400, 540, 420]),
        "AS" => Object::Name(b"On".to_vec()),
        "AP" => dictionary! { "N" => dictionary! { "On" => on_id } },
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(annot_id)]));
    let pdf = d.finish();

    assert!(
        extracted_text(&pdf).contains(SECRET),
        "Zustands-Dictionary unter /AP /N wird nicht gelesen: {:?}",
        extracted_text(&pdf)
    );
}

#[test]
fn a_page_without_annotations_gains_nothing() {
    // Gegenprobe: der neue Weg darf auf gewöhnlichen Seiten nichts verändern.
    let pdf = common::page(&["Kontoinhaber: Max Mustermann", &format!("IBAN: {SECRET}")]).finish();
    let scan = scan(&pdf);
    assert!(
        scan.warnings.is_empty(),
        "unerwartete Warnung: {:?}",
        scan.warnings
    );
    assert_eq!(
        scan.shows.iter().flat_map(|s| s.glyphs()).count(),
        "Kontoinhaber: Max Mustermann".len() + format!("IBAN: {SECRET}").len()
    );
}
