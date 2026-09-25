//! Gegenprüfung R4 (Fix-Runde 6), Punkt 4: **laufen Kommandozeile und
//! Oberfläche nach Runde 6 noch gleich?**
//!
//! `cli_and_gui_agree.rs` misst am Beispiel-Kontoauszug, `ze_p5_gleichlauf.rs`
//! an Formular/Annotationen/Lesezeichen, `zf_q4_gleichlauf.rs` an einer
//! reicheren Vorlage. Keine dieser Vorlagen trifft die Stellen, die
//! Fix-Runde 6 angefasst hat. Hier steht eine Vorlage, die **genau** sie
//! trifft:
//!
//! * eine **direkt** stehende Eigenschaftsliste (`/Properties /MC0` mit
//!   `/ActualText`) in **geerbten** Ressourcen — die Seite hat keine eigenen,
//!   sie hängen am `/Pages`-Knoten (Runde 6: „an vier Orten … alle vier
//!   werden am Fundort bereinigt“),
//! * ein Formular, das ein zweites **zweimal** zeichnet, und das unter
//!   verschachtelten BDC-Klammern (die Zuordnung Spiegel→Formular, die in
//!   Runde 6 gedeckelt wurde),
//! * eine **Movie-Annotation** mit `/F` (eines der vier Klartextlecks an
//!   Annotationsnachbarn),
//! * ein Strom mit einem **unbekannten Filter an erster Stelle**
//!   (`/Filter [/FooDecode /FlateDecode]` und `/Filter /FooDecode`) — der
//!   Fall, der `--check-leaks` stumm machte.
//!
//! Verglichen wird byteweise, dazu das Audit-Log Feld für Feld, dazu das
//! Urteil beider Orakel (CLI `--check-leaks`, Oberfläche
//! `AppState::check_export`) über **dieselbe** Ausgabe.

#![cfg(feature = "gui")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use redact_gui::AppState;
use redact_pipeline::Config;

const IBAN: &str = "DE89 3704 0044 0532 0130 00";
const IBAN3: &str = "DE02 1203 0000 0000 2020 51";

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zg-r4-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .env("REDACT_RS_CONFIG", "/nicht/vorhanden.yaml")
        .env_remove("REDACT_RS_PASSWORD")
        .stdin(Stdio::null())
        .output()
        .expect("Binary")
}

/// Geschrieben wurde die Datei — Rückgabewert 0 oder 3 („verarbeitet, aber
/// nicht vollständig geprüft“). Die Vorlage trägt einen Textspiegel, dessen
/// Fassung von den Glyphen abweicht; das ist eine 3 und kein Fehler.
#[track_caller]
fn wrote_the_file(out: &Output) {
    let code = out.status.code();
    assert!(
        code == Some(0) || code == Some(3),
        "Lauf fehlgeschlagen ({code:?}): {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

/// Die Schrift-Id der ersten Seite.
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

/// Die Vorlage, die die Änderungen der Runde 6 trifft.
fn runde6_pdf() -> Vec<u8> {
    use redact_pdf::testing::{build_pdf, TextItem};
    let base = build_pdf(&[
        vec![TextItem::new(
            72.0,
            700.0,
            10.0,
            "Kontoinhaber Max Mustermann",
        )],
        vec![TextItem::new(
            72.0,
            700.0,
            10.0,
            format!("Zweitkonto {IBAN3}"),
        )],
    ]);
    let mut doc = Document::load_mem(&base).expect("Vorlage ladbar");
    let font_id = font_of_first_page(&doc);
    let pages: Vec<ObjectId> = (1..=2).map(|n| *doc.get_pages().get(&n).unwrap()).collect();
    let page1_resources = doc
        .get_object(pages[0])
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Resources")
        .unwrap()
        .as_reference()
        .unwrap();

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
        format!("BT /F1 10 Tf 2 5 Td ({}) Tj ET", escape(IBAN)).into_bytes(),
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
        // Zweimal dasselbe Formular, jedes Mal unter einer eigenen
        // BDC-Klammer — der Fall, den Runde 6 gedeckelt hat.
        b"/Span /MC0 BDC q 1 0 0 1 0 25 cm /Inner Do Q EMC \
          /Span /MC0 BDC q 1 0 0 1 0 0 cm /Inner Do Q EMC"
            .to_vec(),
    ));

    // --- Die direkt stehende Eigenschaftsliste, in **geerbten** Ressourcen.
    {
        let resources = doc
            .get_dictionary_mut(page1_resources)
            .expect("Ressourcen der Seite");
        let mut props = Dictionary::new();
        props.set(
            "MC0",
            Object::Dictionary(dictionary! {
                "ActualText" => Object::string_literal(format!("Zahlung an {IBAN}")),
            }),
        );
        resources.set("Properties", props);
        resources.set(
            "XObject",
            dictionary! { "Spiegel" => Object::Reference(outer) },
        );
    }
    // Der Inhalt von Seite 1: sichtbarer Text, der Spiegel, und der
    // geschützte Abschnitt mit der Eigenschaftsliste.
    let content = format!(
        "BT /F1 10 Tf 1 0 0 1 72 700 Tm (Kontoinhaber Max Mustermann) Tj ET\n\
         /Span /MC0 BDC\n\
         BT /F1 10 Tf 1 0 0 1 72 660 Tm ({}) Tj ET\n\
         EMC\n\
         q 1 0 0 1 72 500 cm /Spiegel Do Q\n",
        escape(IBAN)
    );
    let content_id = doc
        .get_object(pages[0])
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Contents")
        .unwrap()
        .as_reference()
        .unwrap();
    doc.objects.insert(
        content_id,
        Object::Stream(Stream::new(dictionary! {}, content.into_bytes()).with_compression(false)),
    );
    // Geerbt: die Ressourcen hängen am Seitenbaum, die Seite hat keine.
    let pages_id = doc
        .get_object(pages[0])
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Parent")
        .unwrap()
        .as_reference()
        .unwrap();
    doc.get_dictionary_mut(pages_id)
        .expect("Seitenbaum")
        .set("Resources", Object::Reference(page1_resources));
    doc.get_object_mut(pages[0])
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .remove(b"Resources");

    // --- Die Movie-Annotation mit /F, dazu ein Widget mit dem Spiegel als
    // Erscheinungsstrom (das Formular zweimal platziert).
    let movie = doc.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Movie",
        "Rect" => vec![400.into(), 600.into(), 460.into(), 640.into()],
        "T" => Object::string_literal(format!("Film {IBAN}")),
        "Movie" => dictionary! {
            "F" => Object::string_literal(format!("/tmp/{IBAN}.mov")),
            "Aspect" => vec![320.into(), 240.into()],
        },
    });
    let widget = doc.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Widget",
        "FT" => "Tx",
        "T" => Object::string_literal("konto"),
        "V" => Object::string_literal(IBAN),
        "Rect" => vec![72.into(), 480.into(), 372.into(), 530.into()],
        "P" => Object::Reference(pages[0]),
        "AP" => dictionary! { "N" => Object::Reference(outer) },
    });
    doc.get_object_mut(pages[0])
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set(
            "Annots",
            vec![Object::Reference(movie), Object::Reference(widget)],
        );
    let catalog_id = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();

    // --- Zwei Ströme mit unbekanntem Filter an **erster** Stelle: einmal
    // allein, einmal vor einem bekannten. Der Inhalt ist verschleiert
    // (XOR 0x5A), also weder roh noch als Zeichenkette lesbar.
    let opaque: Vec<u8> = format!("Anlage {IBAN}").bytes().map(|b| b ^ 0x5A).collect();
    let allein = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "EmbeddedFile",
            "Filter" => "FooDecode",
        },
        opaque.clone(),
    ));
    let vorne = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "EmbeddedFile",
            "Filter" => vec!["FooDecode".into(), "FlateDecode".into()],
        },
        opaque,
    ));
    let catalog = doc
        .get_object_mut(catalog_id)
        .unwrap()
        .as_dict_mut()
        .unwrap();
    catalog.set(
        "Names",
        dictionary! {
            "EmbeddedFiles" => dictionary! {
                "Names" => vec![
                    Object::string_literal("allein"),
                    Object::Reference(allein),
                    Object::string_literal("vorne"),
                    Object::Reference(vorne),
                ],
            },
        },
    );
    catalog.set(
        "AcroForm",
        dictionary! {
            "Fields" => vec![Object::Reference(widget)],
            "DA" => Object::string_literal("/F1 10 Tf 0 g"),
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

fn export_through_the_window(config: Config, out: &Path, audit: &Path) -> (Vec<u8>, AppState) {
    let mut state = AppState::with_config(config);
    state
        .load_document(&state.config.input.clone())
        .expect("PDF ladbar");
    state.analyze().expect("Analyse");
    assert!(!state.regions.is_empty(), "die Analyse fand nichts");
    state.export(out, Some(audit)).expect("Export");
    (std::fs::read(out).expect("Ausgabe lesbar"), state)
}

fn comparable_log(path: &Path) -> serde_json::Value {
    let mut log: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("Audit-Log lesbar")).unwrap();
    log["timestamp"] = serde_json::json!("<zeit>");
    log["output"]["path"] = serde_json::json!("<ausgabe>");
    log
}

/// Beide Wege an der Vorlage der Runde 6: dieselben Bytes, dasselbe Log.
#[test]
fn zg_r4_4_beide_wege_an_der_vorlage_der_runde_6() {
    let dir = workdir("gleichlauf");
    let input = dir.join("runde6.pdf");
    std::fs::write(&input, runde6_pdf()).unwrap();

    // Die Vorlage trägt den Klartext wirklich an allen vier Stellen.
    let raw = std::fs::read(&input).unwrap();
    println!(
        "Vorlage: {} Byte, {} Fundstellen für die IBAN",
        raw.len(),
        redact_pdf::leaks(&raw, IBAN).len()
    );
    for what in [
        format!("Zahlung an {IBAN}"),
        format!("Film {IBAN}"),
        format!("/tmp/{IBAN}.mov"),
    ] {
        assert!(
            !redact_pdf::leaks(&raw, &what).is_empty(),
            "„{what}“ fehlt in der Vorlage"
        );
    }

    let cli_out = dir.join("cli.pdf");
    let cli_audit = dir.join("cli.json");
    let cli = run(&[
        input.to_str().unwrap(),
        "-o",
        cli_out.to_str().unwrap(),
        "--patterns",
        "iban_de",
        "--padding",
        "3",
        "--audit-log",
        cli_audit.to_str().unwrap(),
    ]);
    println!("CLI: {}", String::from_utf8_lossy(&cli.stdout));
    println!("CLI (stderr): {}", String::from_utf8_lossy(&cli.stderr));
    wrote_the_file(&cli);
    let cli_bytes = std::fs::read(&cli_out).unwrap();

    let gui_out = dir.join("gui.pdf");
    let gui_audit = dir.join("gui.json");
    let (gui_bytes, state) = export_through_the_window(config(&input), &gui_out, &gui_audit);

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
    println!(
        "Audit-Log: {}",
        serde_json::to_string_pretty(&cli_log).unwrap()
    );

    // Und der Vergleich sieht etwas: die vier Stellen sind bereinigt.
    for what in [
        IBAN.to_string(),
        format!("Zahlung an {IBAN}"),
        format!("Film {IBAN}"),
        format!("/tmp/{IBAN}.mov"),
    ] {
        let rest = redact_pdf::leaks(&cli_bytes, &what);
        assert!(
            rest.is_empty(),
            "Klartext in der Ausgabe ({what}): {rest:?}"
        );
    }

    // --- Die Orakel: beide über dieselbe Ausgabe.
    let check = state.check_export(&gui_out, &state.hit_summary());
    println!("Oberfläche über die Ausgabe: {}", check.sentence());
    println!("Warnung:                     {:?}", check.warning());
    let cli_check = run(&[
        cli_out.to_str().unwrap(),
        "--check-leaks",
        IBAN,
        "--check-leaks",
        IBAN3,
    ]);
    let cli_text = String::from_utf8_lossy(&cli_check.stdout).into_owned();
    println!(
        "CLI --check-leaks über die Ausgabe (rc {:?}):\n{cli_text}",
        cli_check.status.code()
    );
    assert!(
        !check.found_leak(),
        "die Oberfläche meldet ein Leck, die Kommandozeile nicht: {}",
        check.sentence()
    );
    assert!(
        !cli_text.contains("STEHT NOCH"),
        "die Kommandozeile meldet ein Leck, die Oberfläche nicht: {cli_text}"
    );
    assert_eq!(
        check.incomplete(),
        cli_text.contains("NICHT GEPRÜFT"),
        "die Orakel sind sich über die ungeprüften Stellen uneins:\n           Oberfläche: {:?}\n  CLI: {cli_text}",
        check.unchecked
    );

    // --- Und über die **Vorlage**: dort steht der unbekannte Filter noch
    // (`strip_metadata` nimmt die eingebetteten Dateien aus der Ausgabe
    // heraus). Der Fall, der `--check-leaks` bis Runde 6 stumm machte.
    let vorlage_cli = run(&[input.to_str().unwrap(), "--check-leaks", "Anlage"]);
    let vorlage_text = String::from_utf8_lossy(&vorlage_cli.stdout).into_owned();
    println!(
        "CLI --check-leaks über die Vorlage (rc {:?}):\n{vorlage_text}",
        vorlage_cli.status.code()
    );
    let vorlage_gui = redact_gui::state::ExportCheckPlan {
        needles: vec!["Anlage".to_string()],
        kept_forms: vec![false],
        ..redact_gui::state::ExportCheckPlan::default()
    }
    .run(&input);
    println!("Oberfläche über die Vorlage: {}", vorlage_gui.sentence());
    assert_eq!(
        vorlage_cli.status.code(),
        Some(3),
        "unbekannter Filter an erster Stelle, aber kein „nicht geprüft“: {vorlage_text}"
    );
    assert!(vorlage_text.contains("FooDecode"), "{vorlage_text}");
    assert!(
        vorlage_gui.incomplete(),
        "die Oberfläche nennt die ungeprüfte Stelle nicht: {}",
        vorlage_gui.sentence()
    );
    let stellen = vorlage_gui.unchecked.join("\n  ");
    println!("ungeprüfte Stellen der Oberfläche:\n  {stellen}");
    assert!(
        stellen.contains("FooDecode"),
        "der unbekannte Filter fehlt: {stellen}"
    );
    assert_eq!(
        vorlage_text.matches("FooDecode").count(),
        stellen.matches("FooDecode").count(),
        "verschieden viele Stellen mit unbekanntem Filter:\n  CLI: {vorlage_text}\n           Oberfläche: {stellen}"
    );
}

/// Die Gegenprobe zum Vergleich selbst: mit einer anderen Polsterung gehen
/// die Bytes auseinander — der Vergleich oben sieht also wirklich etwas.
#[test]
fn zg_r4_4_der_vergleich_sieht_einen_unterschied() {
    let dir = workdir("gegenprobe");
    let input = dir.join("runde6.pdf");
    std::fs::write(&input, runde6_pdf()).unwrap();
    let (drei, _) = export_through_the_window(
        config(&input),
        &dir.join("drei.pdf"),
        &dir.join("drei.json"),
    );
    let (eins, _) = export_through_the_window(
        Config {
            padding: 1.0,
            ..config(&input)
        },
        &dir.join("eins.pdf"),
        &dir.join("eins.json"),
    );
    assert_ne!(drei, eins, "der Vergleich sieht keinen Unterschied");
}
