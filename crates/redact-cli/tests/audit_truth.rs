//! Das Audit-Log darf nur bescheinigen, was tatsächlich passiert ist
//! (Aufgabe #30).
//!
//! Reproduktion des Defekts am laufenden Binary: mit `--padding=-100`
//! schrumpft jedes gefundene Rechteck zu einem leeren Bereich. Die Schwärzung
//! überspringt solche Regionen — kein Zeichen wird entfernt, kein Rechteck
//! gezeichnet, die IBAN steht unverändert in der Ausgabe. Das Log meldete
//! trotzdem eine Schwärzung und `metadata_stripped: true`, weil beides hart
//! verdrahtet war.
//!
//! Die Tests hier messen beides gegeneinander: was in der Ausgabedatei steht
//! (mit [`redact_pdf::leaks`], nicht mit dem eigenen Extraktor) und was das Log
//! darüber behauptet.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use lopdf::{dictionary, Object, StringFormat};

const IBAN: &str = "DE89 3704 0044 0532 0130 00";

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

/// Eigenes Verzeichnis je Test, damit parallele Läufe sich nicht stören.
fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("redact-audit-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("Binary startbar")
}

#[track_caller]
fn succeeds(out: &Output) -> String {
    assert!(
        out.status.success(),
        "Lauf fehlgeschlagen: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn demo(dir: &Path) -> PathBuf {
    let path = dir.join("kontoauszug.pdf");
    std::fs::write(&path, redact_pdf::testing::demo_statement()).unwrap();
    path
}

fn read_log(path: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).expect("Audit-Log ist JSON")
}

fn leaks_in(path: &Path, needle: &str) -> Vec<String> {
    redact_pdf::leaks(&std::fs::read(path).expect("Ausgabedatei lesbar"), needle)
}

fn usize_at(log: &serde_json::Value, path: &[&str]) -> usize {
    let mut node = log;
    for key in path {
        node = &node[*key];
    }
    node.as_u64()
        .unwrap_or_else(|| panic!("{path:?} ist keine Zahl: {node}")) as usize
}

// ---------------------------------------------------------------------------
// #30 — negatives Padding
// ---------------------------------------------------------------------------

#[test]
fn a_degenerate_padding_is_not_logged_as_a_redaction() {
    let dir = workdir("padding");
    let input = demo(&dir);
    let output = dir.join("out.pdf");
    let audit = dir.join("audit.json");

    let stdout = succeeds(&run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--patterns",
        "iban_de",
        "--padding=-100",
        "--audit-log",
        audit.to_str().unwrap(),
    ]));

    // Beobachtung zuerst: die IBAN steht unverändert in der Ausgabe.
    assert!(
        !leaks_in(&output, IBAN).is_empty(),
        "Testaufbau stimmt nicht mehr: mit --padding=-100 sollte gar nichts \
         entfernt worden sein"
    );

    let log = read_log(&audit);
    let requested = usize_at(&log, &["effect", "requested"]);
    assert!(requested > 0, "die Analyse hat keine IBAN gefunden");
    assert_eq!(
        usize_at(&log, &["effect", "applied"]),
        0,
        "das Log verbucht wirkungslose Regionen als angewendet: {log}"
    );
    assert_eq!(usize_at(&log, &["effect", "degenerate"]), requested);
    assert_eq!(usize_at(&log, &["effect", "removed_glyphs"]), 0);
    assert_eq!(usize_at(&log, &["effect", "drawn_rects"]), 0);

    // Jeder einzelne Eintrag ist als wirkungslos gekennzeichnet.
    for entry in log["redactions"].as_array().unwrap() {
        assert_eq!(entry["effect"], serde_json::json!("degenerate"), "{entry}");
        // Und das Log nennt das Rechteck, das wirklich gewirkt hat.
        assert!(
            entry["effective_rect"]["ur"]["x"].as_f64().unwrap()
                <= entry["effective_rect"]["ll"]["x"].as_f64().unwrap(),
            "effective_rect ist nicht leer: {entry}"
        );
    }

    // Und es warnt — auf stderr wie im Log.
    let warnings = log["warnings"].as_array().expect("Warnungen im Log");
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str().unwrap().contains("leer")),
        "keine Warnung zur entarteten Region: {warnings:?}"
    );
    assert!(
        stdout.contains("davon wirkungslos"),
        "die Zusammenfassung verschweigt die wirkungslosen Regionen:\n{stdout}"
    );
}

#[test]
fn a_normal_run_is_logged_as_applied() {
    // Gegenprobe: ohne krummes Padding muss dasselbe Log Erfolg bescheinigen —
    // sonst hätte der Test oben nur die Warnung getestet, nicht die Messung.
    let dir = workdir("normal");
    let input = demo(&dir);
    let output = dir.join("out.pdf");
    let audit = dir.join("audit.json");

    succeeds(&run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--patterns",
        "iban_de",
        "--audit-log",
        audit.to_str().unwrap(),
    ]));

    assert!(
        leaks_in(&output, IBAN).is_empty(),
        "die IBAN steht noch in der Ausgabe: {:?}",
        leaks_in(&output, IBAN)
    );

    let log = read_log(&audit);
    let requested = usize_at(&log, &["effect", "requested"]);
    assert_eq!(usize_at(&log, &["effect", "applied"]), requested);
    assert_eq!(usize_at(&log, &["effect", "degenerate"]), 0);
    assert!(usize_at(&log, &["effect", "removed_glyphs"]) >= IBAN.len());
    assert_eq!(usize_at(&log, &["effect", "drawn_rects"]), requested);
    for entry in log["redactions"].as_array().unwrap() {
        assert_eq!(entry["effect"], serde_json::json!("applied"), "{entry}");
    }
    let warnings = log["warnings"].as_array().cloned().unwrap_or_default();
    assert!(warnings.is_empty(), "unerwartete Warnung: {warnings:?}");
}

#[test]
fn metadata_stripped_is_measured_not_asserted() {
    let dir = workdir("metadata");
    let input = dir.join("ohne-metadaten.pdf");
    let output = dir.join("out.pdf");
    let audit = dir.join("audit.json");

    // Eine Datei, aus der es nichts zu entfernen gibt: `strip_metadata` läuft,
    // findet aber nichts. Das Log darf dann keine Entfernung behaupten.
    let mut doc =
        redact_pdf::load_from_bytes(&redact_pdf::testing::demo_statement()).expect("ladbar");
    doc.trailer.remove(b"Info");
    std::fs::write(&input, redact_pdf::save_to_bytes(&doc).unwrap()).unwrap();

    succeeds(&run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--patterns",
        "iban_de",
        "--audit-log",
        audit.to_str().unwrap(),
    ]));

    let log = read_log(&audit);
    assert_eq!(
        log["metadata_stripped"],
        serde_json::json!(false),
        "das Log behauptet eine Entfernung, die es nicht gab: {}",
        log["metadata"]
    );
    let summary = log["metadata"]["summary"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(summary.is_empty(), "{summary:?}");
}

// ---------------------------------------------------------------------------
// #4 — Restdaten tauchen im Log auf
// ---------------------------------------------------------------------------

#[test]
fn residual_data_removals_reach_the_audit_log() {
    let dir = workdir("restdaten");
    let input = dir.join("formular.pdf");
    let output = dir.join("out.pdf");
    let audit = dir.join("audit.json");

    let mut doc =
        redact_pdf::load_from_bytes(&redact_pdf::testing::demo_statement()).expect("ladbar");
    let attachment = doc.add_object(Object::Stream(lopdf::Stream::new(
        lopdf::Dictionary::new(),
        format!("Kontoauszug\nIBAN: {IBAN}\n").into_bytes(),
    )));
    let filespec = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => "Filespec",
        "F" => Object::string_literal("anhang.txt"),
        "EF" => dictionary! { "F" => Object::Reference(attachment) },
    }));
    let field = doc.add_object(Object::Dictionary(dictionary! {
        "FT" => "Tx",
        "T" => Object::string_literal("iban"),
        "V" => Object::String(utf16be(IBAN), StringFormat::Literal),
    }));
    let names = doc.add_object(Object::Dictionary(dictionary! {
        "EmbeddedFiles" => dictionary! {
            "Names" => vec![Object::string_literal("anhang"), Object::Reference(filespec)],
        },
        "JavaScript" => dictionary! {
            "Names" => vec![
                Object::string_literal("init"),
                Object::Dictionary(dictionary! {
                    "S" => "JavaScript",
                    "JS" => Object::string_literal(format!("var iban='{IBAN}';")),
                }),
            ],
        },
    }));
    let catalog_id = match doc.trailer.get(b"Root").unwrap() {
        Object::Reference(id) => *id,
        other => panic!("kein Katalog: {other:?}"),
    };
    let catalog = doc.get_dictionary_mut(catalog_id).unwrap();
    catalog.set("Names", Object::Reference(names));
    catalog.set(
        "AcroForm",
        Object::Dictionary(dictionary! {
            "Fields" => vec![Object::Reference(field)],
            "XFA" => Object::string_literal(format!("<xfa>{IBAN}</xfa>")),
        }),
    );
    std::fs::write(&input, redact_pdf::save_to_bytes(&doc).unwrap()).unwrap();
    assert!(
        !redact_pdf::leaks(&std::fs::read(&input).unwrap(), IBAN).is_empty(),
        "Testdaten taugen nicht"
    );

    succeeds(&run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--patterns",
        "iban_de",
        "--audit-log",
        audit.to_str().unwrap(),
    ]));

    let hits = leaks_in(&output, IBAN);
    assert!(
        hits.is_empty(),
        "„{IBAN}“ steht noch {} mal in der Ausgabe:\n{}",
        hits.len(),
        hits.join("\n")
    );

    // Und jede Entfernung steht im Log — sonst wäre sie nicht auditierbar.
    let log = read_log(&audit);
    assert_eq!(log["metadata_stripped"], serde_json::json!(true));
    assert_eq!(log["metadata"]["acroform"], serde_json::json!(true));
    assert_eq!(log["metadata"]["xfa"], serde_json::json!(true));
    assert_eq!(usize_at(&log, &["metadata", "field_values"]), 1);
    assert_eq!(usize_at(&log, &["metadata", "embedded_files"]), 1);
    assert_eq!(usize_at(&log, &["metadata", "javascript"]), 1);
    assert_eq!(usize_at(&log, &["metadata", "names"]), 1);
}

// ---------------------------------------------------------------------------
// Stille Abbrüche des Interpreters
// ---------------------------------------------------------------------------

/// Ein XObject ohne `/Subtype` bringt den Interpreter zum Aussteigen. Dort
/// steht Text, den die Analyse nie sieht — ohne Warnung liest der Nutzer
/// „0 Schwärzungen“ und hält die Datei für sauber. Die Warnung entsteht in
/// `redact-pdf`; hier wird geprüft, dass sie bis zum Nutzer **und** ins
/// Audit-Log durchkommt.
#[test]
fn a_silent_dead_end_in_the_interpreter_reaches_the_user_and_the_log() {
    let dir = workdir("stiller-abbruch");
    let input = dir.join("xobject.pdf");
    let output = dir.join("out.pdf");
    let audit = dir.join("audit.json");

    let mut doc =
        redact_pdf::load_from_bytes(&redact_pdf::testing::demo_statement()).expect("ladbar");
    let page_id = *doc.get_pages().values().next().unwrap();

    let xobject = doc.add_object(Object::Stream(
        lopdf::Stream::new(
            dictionary! {
                // Kein /Subtype — genau der Fall.
                "Type" => "XObject",
                "BBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            },
            b"BT /F1 10 Tf 72 500 Td (Zweitschrift) Tj ET\n".to_vec(),
        )
        .with_compression(false),
    ));
    let resources = match doc.get_dictionary(page_id).unwrap().get(b"Resources") {
        Ok(Object::Reference(id)) => *id,
        other => panic!("unerwartete /Resources: {other:?}"),
    };
    doc.get_dictionary_mut(resources)
        .unwrap()
        .set("XObject", dictionary! { "X0" => xobject });

    let mut content = doc.get_page_content(page_id).expect("Content lesbar");
    content.extend_from_slice(b"\nq /X0 Do Q\n");
    let content_id = doc.add_object(Object::Stream(lopdf::Stream::new(
        lopdf::Dictionary::new(),
        content,
    )));
    doc.get_dictionary_mut(page_id)
        .unwrap()
        .set("Contents", Object::Reference(content_id));
    std::fs::write(&input, redact_pdf::save_to_bytes(&doc).unwrap()).unwrap();

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--patterns",
        "iban_de",
        "--audit-log",
        audit.to_str().unwrap(),
    ]);
    succeeds(&out);

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("XObject") && stderr.contains("Subtype"),
        "der stille Abbruch wird dem Nutzer verschwiegen:\n{stderr}"
    );

    let log = read_log(&audit);
    let warnings = log["warnings"].as_array().cloned().unwrap_or_default();
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str().unwrap_or_default().contains("XObject")),
        "die Warnung fehlt im Audit-Log: {warnings:?}"
    );
    // Und jede Warnung steht genau einmal drin.
    let mut seen: Vec<&str> = warnings.iter().filter_map(|w| w.as_str()).collect();
    let before = seen.len();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(before, seen.len(), "doppelte Warnung im Log: {warnings:?}");
}

fn utf16be(text: &str) -> Vec<u8> {
    let mut out = vec![0xfe, 0xff];
    out.extend(text.encode_utf16().flat_map(|u| u.to_be_bytes()));
    out
}
