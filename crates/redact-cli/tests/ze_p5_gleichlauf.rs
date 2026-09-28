//! Gegenprüfung P5 (Fix-Runde 4), Punkt 4: Gleichlauf von Kommandozeile und
//! Oberfläche an einer Datei mit **Formular, Annotationen und Lesezeichen**.
//!
//! `cli_and_gui_agree.rs` vergleicht beide Wege nur am Beispiel-Kontoauszug
//! (`demo_statement`): reiner Seitentext, kein /AcroForm, keine /Annots, kein
//! /Outlines. Genau die drei Stellen hat Fix-Runde 4 angefasst — die Seiten
//! werden erst nach der Formularschleife geschrieben (`PendingPage`), der
//! Metadaten-Graphlauf greift auf mehr Annotationsschlüssel zu. Ändert sich
//! dabei die Schreibreihenfolge nur auf einem der beiden Wege, ginge das an
//! den alten Testfällen vorbei.
//!
//! Verglichen wird byteweise, dazu das Audit-Log Feld für Feld.

#![cfg(feature = "gui")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use redact_gui::AppState;
use redact_pipeline::Config;

const IBAN: &str = "DE89 3704 0044 0532 0130 00";
const IBAN2: &str = "DE02 1203 0000 0000 2020 51";

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ze-p5-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str]) -> Output {
    Command::new(bin()).args(args).output().expect("Binary")
}

#[track_caller]
fn succeeds(out: &Output) {
    assert!(
        out.status.success(),
        "Lauf fehlgeschlagen ({:?}): {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Die Ressourcen-Id der ersten Seite (dort steht die Schrift `F1`).
fn font_of_first_page(doc: &Document) -> ObjectId {
    let page_id = *doc.get_pages().get(&1).unwrap();
    let resources = match doc
        .get_object(page_id)
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Resources")
    {
        Ok(Object::Reference(id)) => *doc
            .get_object(*id)
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Font")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"F1")
            .unwrap()
            .as_reference()
            .as_ref()
            .unwrap(),
        other => panic!("Resources: {other:?}"),
    };
    resources
}

/// Zwei Seiten Text, dazu ein Formularfeld mit Erscheinungsstrom
/// (Textspiegel), vier Annotationen und ein Lesezeichenbaum — überall
/// derselbe Klartext.
fn rich_pdf() -> Vec<u8> {
    use redact_pdf::testing::{build_pdf, TextItem};
    let base = build_pdf(&[
        vec![
            TextItem::new(72.0, 700.0, 10.0, format!("IBAN: {IBAN}")),
            TextItem::new(72.0, 660.0, 10.0, "Max Mustermann"),
        ],
        vec![TextItem::new(72.0, 700.0, 10.0, format!("Konto {IBAN2}"))],
    ]);
    let mut doc = Document::load_mem(&base).expect("Vorlage ladbar");
    let font_id = font_of_first_page(&doc);
    let page1 = *doc.get_pages().get(&1).unwrap();

    // --- Formularfeld mit Erscheinungsstrom (der „Textspiegel“).
    let ap_resources = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let appearance = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 300.into(), 20.into()],
            "Resources" => Object::Reference(ap_resources),
        },
        format!("BT /F1 10 Tf 2 5 Td ({IBAN}) Tj ET").into_bytes(),
    ));
    let field = doc.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Widget",
        "FT" => "Tx",
        "T" => Object::string_literal("iban"),
        "V" => Object::string_literal(IBAN),
        "DV" => Object::string_literal(IBAN),
        "DA" => Object::string_literal("/F1 10 Tf 0 g"),
        "MK" => dictionary! { "CA" => Object::string_literal(IBAN) },
        "Rect" => vec![72.into(), 600.into(), 372.into(), 620.into()],
        "P" => Object::Reference(page1),
        "AP" => dictionary! { "N" => Object::Reference(appearance) },
    });

    // --- Annotationen: Notiz mit Popup, dazu eine Antwort (/IRT).
    let popup = doc.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Popup",
        "Contents" => Object::string_literal(format!("Popup {IBAN}")),
        "Rect" => vec![400.into(), 600.into(), 560.into(), 700.into()],
    });
    let note = doc.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Text",
        "Contents" => Object::string_literal(format!("Notiz {IBAN}")),
        "NM" => Object::string_literal(format!("nm-{IBAN}")),
        "Rect" => vec![400.into(), 690.into(), 420.into(), 710.into()],
        "Popup" => Object::Reference(popup),
    });
    let reply = doc.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Text",
        "Contents" => Object::string_literal(format!("Antwort {IBAN}")),
        "IRT" => Object::Reference(note),
        "Rect" => vec![430.into(), 690.into(), 450.into(), 710.into()],
    });
    doc.get_object_mut(page1)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set(
            "Annots",
            vec![
                Object::Reference(field),
                Object::Reference(note),
                Object::Reference(popup),
                Object::Reference(reply),
            ],
        );

    // --- Lesezeichen und /AcroForm im Katalog.
    let catalog_id = doc
        .trailer
        .get(b"Root")
        .unwrap()
        .as_reference()
        .expect("Katalog");
    let outlines_id = doc.new_object_id();
    let child = doc.add_object(dictionary! {
        "Title" => Object::string_literal(format!("Auszug {IBAN}")),
        "Parent" => Object::Reference(outlines_id),
    });
    let top = doc.add_object(dictionary! {
        "Title" => Object::string_literal(format!("Kunde {IBAN}")),
        "Parent" => Object::Reference(outlines_id),
        "First" => Object::Reference(child),
        "Last" => Object::Reference(child),
        "Count" => 1,
    });
    doc.get_object_mut(child)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Parent", Object::Reference(top));
    doc.objects.insert(
        outlines_id,
        Object::Dictionary(dictionary! {
            "Type" => "Outlines",
            "First" => Object::Reference(top),
            "Last" => Object::Reference(top),
            "Count" => 2,
        }),
    );
    let catalog = doc
        .get_object_mut(catalog_id)
        .unwrap()
        .as_dict_mut()
        .unwrap();
    catalog.set("Outlines", Object::Reference(outlines_id));
    catalog.set(
        "AcroForm",
        dictionary! {
            "Fields" => vec![Object::Reference(field)],
            "DA" => Object::string_literal("/F1 10 Tf 0 g"),
            "NeedAppearances" => true,
        },
    );

    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("speicherbar");
    bytes
}

fn config(input: &Path) -> Config {
    Config {
        input: input.to_path_buf(),
        patterns: vec!["iban_de".to_string()],
        padding: 3.0,
        ..Config::default()
    }
}

fn export_through_the_window(config: Config, out: &Path, audit: &Path) -> Vec<u8> {
    let mut state = AppState::with_config(config);
    state
        .load_document(&state.config.input.clone())
        .expect("PDF ladbar");
    state.analyze().expect("Analyse");
    assert!(!state.regions.is_empty(), "die Analyse fand nichts");
    state.export(out, Some(audit)).expect("Export");
    std::fs::read(out).expect("Ausgabe lesbar")
}

fn comparable_log(path: &Path) -> serde_json::Value {
    let mut log: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("Audit-Log lesbar")).unwrap();
    log["timestamp"] = serde_json::json!("<zeit>");
    log["output"]["path"] = serde_json::json!("<ausgabe>");
    log
}

/// Beide Wege an der reichen Datei: dieselben Bytes, dasselbe Log.
#[test]
fn ze_p5_beide_wege_an_formular_annotationen_und_lesezeichen() {
    let dir = workdir("gleichlauf");
    let input = dir.join("reich.pdf");
    std::fs::write(&input, rich_pdf()).unwrap();

    // Die Vorlage trägt den Klartext wirklich an allen vier Stellen.
    let raw = std::fs::read(&input).unwrap();
    let hits = redact_pdf::leaks(&raw, IBAN);
    println!("Vorlage: {} Fundstellen", hits.len());
    for what in ["Popup", "Notiz", "Antwort", "Auszug", "Kunde"] {
        assert!(
            !redact_pdf::leaks(&raw, &format!("{what} {IBAN}")).is_empty(),
            "„{what}“ fehlt in der Vorlage"
        );
    }

    let cli_out = dir.join("cli.pdf");
    let cli_audit = dir.join("cli.json");
    succeeds(&run(&[
        input.to_str().unwrap(),
        "-o",
        cli_out.to_str().unwrap(),
        "--patterns",
        "iban_de",
        "--padding",
        "3",
        "--audit-log",
        cli_audit.to_str().unwrap(),
    ]));
    let cli_bytes = std::fs::read(&cli_out).unwrap();

    let gui_out = dir.join("gui.pdf");
    let gui_audit = dir.join("gui.json");
    let gui_bytes = export_through_the_window(config(&input), &gui_out, &gui_audit);

    assert_eq!(
        cli_bytes,
        gui_bytes,
        "Kommandozeile und Oberfläche schreiben verschiedene Dateien ({} vs. {} Byte)",
        cli_bytes.len(),
        gui_bytes.len()
    );
    let cli_log = comparable_log(&cli_audit);
    assert_eq!(
        cli_log,
        comparable_log(&gui_audit),
        "die Audit-Logs gehen auseinander: {}",
        serde_json::to_string_pretty(&cli_log).unwrap()
    );

    // Und der Vergleich sieht etwas: beide Seiten sind noch da, der Klartext
    // ist überall weg (Formularspiegel, Annotationen, Lesezeichen).
    let out_doc = Document::load_mem(&cli_bytes).expect("Ausgabe ladbar");
    assert_eq!(
        out_doc.get_pages().len(),
        2,
        "die Ausgabe hat Seiten verloren"
    );
    let rest = redact_pdf::leaks(&cli_bytes, IBAN);
    assert!(rest.is_empty(), "Klartext in der Ausgabe: {rest:?}");
    let rest2 = redact_pdf::leaks(&cli_bytes, IBAN2);
    assert!(
        rest2.is_empty(),
        "Klartext (Seite 2) in der Ausgabe: {rest2:?}"
    );

    assert!(
        cli_log["effect"]["removed_glyphs"].as_u64().unwrap() > 0,
        "{cli_log}"
    );
    println!(
        "Ausgabe: {} Byte, {} Zeichen entfernt, {} Annotation(en) entfernt",
        cli_bytes.len(),
        cli_log["effect"]["removed_glyphs"],
        cli_log["effect"]["removed_annotations"]
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Dasselbe mit `--action replace`: der Ersatztext fasst Seite **und**
/// Ressourcen an, das Formular kommt hinzu.
#[test]
fn ze_p5_beide_wege_mit_ersatztext_an_derselben_datei() {
    let dir = workdir("gleichlauf-replace");
    let input = dir.join("reich.pdf");
    std::fs::write(&input, rich_pdf()).unwrap();

    let cli_out = dir.join("cli.pdf");
    let cli_audit = dir.join("cli.json");
    succeeds(&run(&[
        input.to_str().unwrap(),
        "-o",
        cli_out.to_str().unwrap(),
        "--patterns",
        "iban_de",
        "--padding",
        "3",
        "--action",
        "replace",
        "--replace-with",
        "[IBAN]",
        "--audit-log",
        cli_audit.to_str().unwrap(),
    ]));
    let cli_bytes = std::fs::read(&cli_out).unwrap();

    let gui_out = dir.join("gui.pdf");
    let gui_audit = dir.join("gui.json");
    let gui_bytes = export_through_the_window(
        Config {
            action: redact_core::Action::Replace("[IBAN]".to_string()),
            ..config(&input)
        },
        &gui_out,
        &gui_audit,
    );

    assert_eq!(
        cli_bytes,
        gui_bytes,
        "mit --action replace verschiedene Dateien ({} vs. {} Byte)",
        cli_bytes.len(),
        gui_bytes.len()
    );
    assert_eq!(comparable_log(&cli_audit), comparable_log(&gui_audit));
    assert!(!redact_pdf::leaks(&cli_bytes, "[IBAN]").is_empty());

    std::fs::remove_dir_all(&dir).ok();
}
