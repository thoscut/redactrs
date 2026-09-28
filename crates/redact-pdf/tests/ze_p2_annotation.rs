//! Gegenprüfung P2 (Fix-Runde 4): die Annotation ohne `/AP`.
//!
//! Seit `f982c12` ist die Warnung „Eine Annotation trägt Text …, hat aber
//! keinen lesbaren Erscheinungsstrom (/AP)“ **keine** Deckungslücke mehr:
//! `redact_pipeline::coverage::NOT_A_COVERAGE_GAP` trägt sie mit der
//! Begründung, `strip_metadata` nehme den Text als Ganzes und laufe „in der
//! Kette immer“. Diese Datei prüft die Zusage — an jedem Weg und in jeder
//! Schreibweise, in der der Text an der Annotation hängen kann.

mod common;

use common::{page, text_ops, Doc, SECRET};
use lopdf::{dictionary, Object, Stream};
use redact_core::Redaction;
use redact_pdf::{
    leaks, load_from_bytes, save_to_bytes, strip_metadata, PdfExtractor, PdfRedactor,
};

fn with_annotation(build: impl FnOnce(&mut Doc) -> Object) -> Vec<u8> {
    let mut d = page(&[]);
    d.set_content(&text_ops(&["Kontoinhaber Max Mustermann"]));
    let value = build(&mut d);
    let annot = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Text",
        "Rect" => vec![400.into(), 60.into(), 500.into(), 80.into()],
    }));
    let (key, value) = match value {
        Object::Array(items) => {
            let key = items[0].as_name().expect("Schlüssel").to_vec();
            (key, items[1].clone())
        }
        other => (b"Contents".to_vec(), other),
    };
    d.doc
        .get_dictionary_mut(annot)
        .expect("Annotation")
        .set(key, value);
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(annot)]));
    d.finish()
}

fn run(bytes: &[u8]) -> (Vec<u8>, Vec<String>) {
    let mut doc = load_from_bytes(bytes).expect("ladbar");
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &[] as &[Redaction])
        .expect("Schwärzung");
    strip_metadata(&mut doc);
    (save_to_bytes(&doc).expect("speicherbar"), report.warnings)
}

fn ap_warnings(warnings: &[String]) -> Vec<&String> {
    warnings
        .iter()
        .filter(|w| w.contains("Erscheinungsstrom (/AP)"))
        .collect()
}

/// Der Klartext direkt am Schlüssel: die Zusage hält für jeden der
/// gemeldeten Schlüssel — Warnung genau einmal, danach kein Fund.
#[test]
fn klartext_direkt_am_schluessel_faellt_mit_den_metadaten() {
    for key in ["Contents", "RC", "T", "Subj", "TU", "TM"] {
        let bytes = with_annotation(|_| {
            Object::Array(vec![
                Object::Name(key.as_bytes().to_vec()),
                Object::string_literal(format!("Notiz: {SECRET}")),
            ])
        });
        let (out, warnings) = run(&bytes);
        assert_eq!(ap_warnings(&warnings).len(), 1, "/{key}: {warnings:?}");
        let found = leaks(&out, SECRET);
        assert!(found.is_empty(), "/{key}: {found:?}");
    }
}

/// Derselbe Text, aber als **eigenes Objekt** hinter einem Verweis.
/// `annot_has_text` löst auf und meldet — `strip_metadata` nimmt den
/// Schlüssel, und das verwaiste Objekt fällt mit `prune_unreachable`.
#[test]
fn klartext_hinter_einem_verweis_faellt_ebenso() {
    let bytes = with_annotation(|d| {
        let id = d.add(Object::string_literal(format!("Notiz: {SECRET}")));
        Object::Reference(id)
    });
    let (out, warnings) = run(&bytes);
    assert_eq!(ap_warnings(&warnings).len(), 1, "{warnings:?}");
    let found = leaks(&out, SECRET);
    assert!(found.is_empty(), "{found:?}");
}

/// Ein Lauf **ohne einen einzigen Treffer** ist der Regelfall dieser
/// Warnung: 0 Schwärzungen, und der Text muss trotzdem weg sein.
#[test]
fn null_schwaerzungen_entfernen_den_annotationstext_trotzdem() {
    let bytes = with_annotation(|_| Object::string_literal(format!("Notiz: {SECRET}")));
    let mut doc = load_from_bytes(&bytes).expect("ladbar");
    let report = PdfRedactor::new()
        .apply_with_report(&mut doc, &[] as &[Redaction])
        .expect("Schwärzung");
    assert_eq!(report.removed_glyphs, 0);
    assert_eq!(report.drawn_rects, 0);
    strip_metadata(&mut doc);
    let out = save_to_bytes(&doc).expect("speicherbar");
    assert!(leaks(&out, SECRET).is_empty());
}

/// `/RC` darf laut PDF 32000-1, Tabelle 174 auch ein **Stream** sein („rich
/// text string … or a stream“). Dann greift `content::annot_has_text` nicht
/// (es prüft nur `Object::String`), also gibt es keine Warnung — aber auch
/// keinen Fund: `strip_metadata` nimmt den Schlüssel, und der verwaiste
/// Stream fällt mit `prune_unreachable`. Geprüft, weil eine Warnung, die
/// nur die eine Schreibweise kennt, sonst eine Lücke verdeckte.
#[test]
fn rc_als_stream_faellt_ohne_warnung_aber_vollstaendig() {
    let bytes = with_annotation(|d| {
        let id = d.add(Object::Stream(
            Stream::new(
                dictionary! {},
                format!("<body>Notiz: {SECRET}</body>").into_bytes(),
            )
            .with_compression(false),
        ));
        Object::Array(vec![Object::Name(b"RC".to_vec()), Object::Reference(id)])
    });
    let (out, warnings) = run(&bytes);
    let found = leaks(&out, SECRET);
    assert!(
        found.is_empty(),
        "kein Fund erwartet, aber {found:?}; Warnungen: {warnings:?}"
    );
}

/// Und die Gegenrichtung, damit die Warnung nicht überall anspringt: eine
/// Annotation **mit** Erscheinungsstrom bekommt sie nicht.
#[test]
fn annotation_mit_erscheinungsstrom_bekommt_die_warnung_nicht() {
    let mut d = page(&[]);
    d.set_content(&text_ops(&["Kontoinhaber Max Mustermann"]));
    let font_id = d.font_id;
    let ap = d.add(Object::Stream(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 100.into(), 20.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            },
            b"BT /F1 10 Tf 2 5 Td (Notiz) Tj ET".to_vec(),
        )
        .with_compression(false),
    ));
    let annot = d.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Text",
        "Rect" => vec![400.into(), 60.into(), 500.into(), 80.into()],
        "Contents" => Object::string_literal(format!("Notiz: {SECRET}")),
        "AP" => dictionary! { "N" => ap },
    }));
    d.page_dict_set("Annots", Object::Array(vec![Object::Reference(annot)]));
    let bytes = d.finish();

    let doc = load_from_bytes(&bytes).expect("ladbar");
    let (_, warnings) = PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("Extraktion");
    assert!(ap_warnings(&warnings).is_empty(), "{warnings:?}");
}

/// Material für die Läufe über die Kommandozeile (`--apply-review`,
/// `--no-patterns`, Rückgabewert). Schreibt nur mit `ZE_P2_OUT`.
#[test]
fn schreibt_material() {
    let Ok(dir) = std::env::var("ZE_P2_OUT") else {
        return;
    };
    let bytes = with_annotation(|_| Object::string_literal(format!("Notiz: {SECRET}")));
    std::fs::write(format!("{dir}/annot_ohne_ap.pdf"), bytes).expect("schreibbar");
}
