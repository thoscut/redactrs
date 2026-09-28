//! Gegenprüfung Q4 (Fix-Runde 5), Punkt 5: laufen Kommandozeile und
//! Oberfläche nach dem Umbau von `meta.rs` und `redact.rs` noch gleich?
//!
//! `cli_and_gui_agree.rs` misst am Beispiel-Kontoauszug, `ze_p5_gleichlauf.rs`
//! an einer Datei mit Formular, Annotationen und Lesezeichen. Hier steht eine
//! **neue**, reichere Vorlage:
//!
//! * ein Formularbaum mit /Parent-Kette über drei Ebenen (`kunde.konto.iban`),
//!   dessen Blatt **zweimal** platziert ist (Seite 1 und Seite 3),
//! * ein Erscheinungsstrom, der ein zweites Formular **zweimal** zeichnet
//!   (der „Spiegel über einem mehrfach platzierten Formular“), und dasselbe
//!   Formular noch einmal direkt aus dem Seitenverzeichnis von Seite 3,
//! * eine Notiz mit /Popup und eine Antwort mit /IRT, dazu /RC und /MK,
//! * ein Lesezeichenbaum über zwei Ebenen,
//! * ein Bild auf einer Seite, deren Inhaltsstrom **gepackt** ist (alle
//!   Seitenströme sind FlateDecode).
//!
//! Verglichen wird byteweise, dazu das Audit-Log Feld für Feld.

#![cfg(feature = "gui")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use redact_gui::AppState;
use redact_pipeline::Config;

const IBAN: &str = "DE89 3704 0044 0532 0130 00";
const IBAN3: &str = "DE02 1203 0000 0000 2020 51";

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zf-q4-{}-{name}", std::process::id()));
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

/// Die Schrift-Id der ersten Seite (dort steht `F1`).
fn font_of_first_page(doc: &Document) -> ObjectId {
    let page_id = *doc.get_pages().get(&1).unwrap();
    let resources = doc
        .get_object(page_id)
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Resources")
        .unwrap()
        .as_reference()
        .expect("Resources als Verweis");
    *doc.get_object(resources)
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
        .unwrap()
}

/// Die reiche Vorlage.
fn rich_pdf() -> Vec<u8> {
    use redact_pdf::testing::{build_pdf, TextItem};
    let base = build_pdf(&[
        vec![
            TextItem::new(72.0, 700.0, 10.0, format!("IBAN: {IBAN}")),
            TextItem::new(72.0, 660.0, 10.0, "Max Mustermann"),
        ],
        vec![TextItem::new(72.0, 700.0, 10.0, "Anlage 1")],
        vec![TextItem::new(
            72.0,
            700.0,
            10.0,
            format!("Zweitkonto {IBAN3}"),
        )],
    ]);
    let mut doc = Document::load_mem(&base).expect("Vorlage ladbar");
    let font_id = font_of_first_page(&doc);
    let pages: Vec<ObjectId> = (1..=3).map(|n| *doc.get_pages().get(&n).unwrap()).collect();

    // --- Ein Formular, das ein zweites zweimal zeichnet (der Spiegel).
    let ap_resources = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let inner = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 300.into(), 20.into()],
            "Resources" => Object::Reference(ap_resources),
        },
        format!("BT /F1 10 Tf 2 5 Td ({IBAN}) Tj ET").into_bytes(),
    ));
    let outer_resources = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Inner" => Object::Reference(inner) },
    });
    let outer = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 300.into(), 50.into()],
            "Resources" => Object::Reference(outer_resources),
        },
        // Zweimal dasselbe Formular, an zwei Stellen.
        b"q 1 0 0 1 0 25 cm /Inner Do Q q 1 0 0 1 0 0 cm /Inner Do Q".to_vec(),
    ));

    // --- Formularbaum: kunde -> konto -> iban, das Blatt zweimal platziert.
    let root_field = doc.new_object_id();
    let mid_field = doc.new_object_id();
    let widget1 = doc.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Widget",
        "Parent" => Object::Reference(mid_field),
        "Rect" => vec![72.into(), 600.into(), 372.into(), 650.into()],
        "P" => Object::Reference(pages[0]),
        "MK" => dictionary! { "CA" => Object::string_literal(IBAN) },
        "AP" => dictionary! { "N" => Object::Reference(outer) },
    });
    let widget2 = doc.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Widget",
        "Parent" => Object::Reference(mid_field),
        "Rect" => vec![72.into(), 500.into(), 372.into(), 550.into()],
        "P" => Object::Reference(pages[2]),
        "AP" => dictionary! { "N" => Object::Reference(outer) },
    });
    doc.objects.insert(
        mid_field,
        Object::Dictionary(dictionary! {
            "T" => Object::string_literal("konto"),
            "Parent" => Object::Reference(root_field),
            "FT" => "Tx",
            "V" => Object::string_literal(IBAN),
            "DV" => Object::string_literal(IBAN),
            "Kids" => vec![Object::Reference(widget1), Object::Reference(widget2)],
        }),
    );
    doc.objects.insert(
        root_field,
        Object::Dictionary(dictionary! {
            "T" => Object::string_literal("kunde"),
            "RV" => Object::string_literal(format!("<p>{IBAN}</p>")),
            "Kids" => vec![Object::Reference(mid_field)],
        }),
    );

    // --- Notiz mit /Popup, Antwort mit /IRT, dazu /RC.
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
        "RC" => Object::string_literal(format!("<body>{IBAN}</body>")),
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
    doc.get_object_mut(pages[0])
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set(
            "Annots",
            vec![
                Object::Reference(widget1),
                Object::Reference(note),
                Object::Reference(popup),
                Object::Reference(reply),
            ],
        );
    doc.get_object_mut(pages[2])
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Annots", vec![Object::Reference(widget2)]);

    // --- Ein Bild auf Seite 2, und dasselbe Formular direkt auf Seite 3.
    let image = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 4,
            "Height" => 4,
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 8,
        },
        (0u8..16).collect::<Vec<u8>>(),
    ));
    let page2_resources = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Bild" => Object::Reference(image) },
    });
    doc.get_object_mut(pages[1])
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Resources", Object::Reference(page2_resources));
    let page3_resources = doc.add_object(dictionary! {
        "XObject" => dictionary! { "Spiegel" => Object::Reference(outer) },
        "Font" => dictionary! { "F1" => font_id },
    });
    doc.get_object_mut(pages[2])
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Resources", Object::Reference(page3_resources));

    // --- Lesezeichen über zwei Ebenen.
    let catalog_id = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let outlines_id = doc.new_object_id();
    let leaf = doc.add_object(dictionary! {
        "Title" => Object::string_literal(format!("Beleg {IBAN}")),
        "Parent" => Object::Reference(outlines_id),
    });
    let top = doc.add_object(dictionary! {
        "Title" => Object::string_literal(format!("Kunde {IBAN}")),
        "Parent" => Object::Reference(outlines_id),
        "First" => Object::Reference(leaf),
        "Last" => Object::Reference(leaf),
        "Count" => 1,
    });
    doc.get_object_mut(leaf)
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
            "Fields" => vec![Object::Reference(root_field)],
            "DA" => Object::string_literal("/F1 10 Tf 0 g"),
            "NeedAppearances" => true,
        },
    );

    // --- Jeder Seitenstrom gepackt: „Bilder auf gefilterten Seiten“.
    let stream_ids: Vec<ObjectId> = doc
        .objects
        .iter()
        .filter(|(_, o)| o.as_stream().is_ok())
        .map(|(id, _)| *id)
        .collect();
    for id in stream_ids {
        let stream = doc.get_object_mut(id).unwrap().as_stream_mut().unwrap();
        if stream.dict.get(b"Subtype").is_err() {
            stream.compress().expect("packbar");
        }
    }

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

/// Beide Wege an der reichen Vorlage: dieselben Bytes, dasselbe Log.
#[test]
fn zf_q4_beide_wege_an_der_reichen_vorlage() {
    let dir = workdir("gleichlauf");
    let input = dir.join("reich.pdf");
    std::fs::write(&input, rich_pdf()).unwrap();

    // Die Vorlage trägt den Klartext wirklich überall.
    let raw = std::fs::read(&input).unwrap();
    println!(
        "Vorlage: {} Byte, {} Fundstellen",
        raw.len(),
        redact_pdf::leaks(&raw, IBAN).len()
    );
    for what in ["Popup", "Notiz", "Antwort", "Beleg", "Kunde"] {
        assert!(
            !redact_pdf::leaks(&raw, &format!("{what} {IBAN}")).is_empty(),
            "„{what}“ fehlt in der Vorlage"
        );
    }
    assert!(!redact_pdf::leaks(&raw, IBAN3).is_empty());

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

    // Und der Vergleich sieht etwas.
    let out_doc = Document::load_mem(&cli_bytes).expect("Ausgabe ladbar");
    assert_eq!(out_doc.get_pages().len(), 3, "Seiten verloren");
    for (name, needle) in [("Seite 1", IBAN), ("Seite 3", IBAN3)] {
        let rest = redact_pdf::leaks(&cli_bytes, needle);
        assert!(rest.is_empty(), "{name}: Klartext in der Ausgabe: {rest:?}");
    }
    // Gegenprobe: der Vergleich sieht einen Unterschied, wenn es einen gibt.
    // Dieselbe Vorlage mit der alten fest verdrahteten Polsterung 1,0.
    let anders = dir.join("anders.pdf");
    let anders_audit = dir.join("anders.json");
    let anders_bytes = export_through_the_window(
        Config {
            padding: 1.0,
            ..config(&input)
        },
        &anders,
        &anders_audit,
    );
    assert_ne!(
        cli_bytes, anders_bytes,
        "der byteweise Vergleich sieht nichts — er wäre wertlos"
    );

    println!(
        "Ausgabe: {} Byte, {} Zeichen entfernt, {} Annotation(en) entfernt",
        cli_bytes.len(),
        cli_log["effect"]["removed_glyphs"],
        cli_log["effect"]["removed_annotations"]
    );
    assert!(
        cli_log["effect"]["removed_glyphs"].as_u64().unwrap() > 0,
        "{cli_log}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Und dieselbe Vorlage über `--check-leaks`: die Kommandozeile bestätigt,
/// was die Nachprüfung der Oberfläche über dieselbe Datei sagt.
#[test]
fn zf_q4_check_leaks_und_die_oberflaeche_urteilen_gleich() {
    let dir = workdir("check-leaks");
    let input = dir.join("reich.pdf");
    std::fs::write(&input, rich_pdf()).unwrap();
    let out = dir.join("out.pdf");
    let audit = dir.join("out.json");
    let bytes = export_through_the_window(config(&input), &out, &audit);
    assert!(!bytes.is_empty());

    // Die Oberfläche über die geschriebene Datei.
    let mut state = AppState::with_config(config(&input));
    state.load_document(&input).unwrap();
    state.analyze().unwrap();
    let summary = state.hit_summary();
    let check = state.check_export(&out, &summary);
    println!("Oberfläche: {}", check.sentence());

    // Gegenprobe: an der **Vorlage** findet dieselbe Prüfung beides (rc 3) —
    // die Messung sieht also etwas.
    let vorher = run(&[
        input.to_str().unwrap(),
        "--check-leaks",
        IBAN,
        "--check-leaks",
        IBAN3,
    ]);
    assert_eq!(
        vorher.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&vorher.stdout)
    );

    // Die Kommandozeile mit denselben Begriffen.
    let probe = run(&[
        out.to_str().unwrap(),
        "--check-leaks",
        IBAN,
        "--check-leaks",
        IBAN3,
    ]);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&probe.stdout),
        String::from_utf8_lossy(&probe.stderr)
    );
    println!("Kommandozeile ({:?}): {text}", probe.status.code());
    assert_eq!(
        probe.status.code(),
        Some(0),
        "die Kommandozeile findet etwas, was die Oberfläche nicht sagt: {text}"
    );
    assert!(!check.found_leak(), "{}", check.sentence());

    std::fs::remove_dir_all(&dir).ok();
}
