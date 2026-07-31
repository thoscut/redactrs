//! End-to-End-Tests gegen das gebaute Binary.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

/// Eigenes Verzeichnis je Test, damit parallele Läufe sich nicht stören.
fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("redact-cli-{}-{name}", std::process::id()));
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

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Liefert den Inhalt aller Content-Streams als Text (auch unkomprimierte).
fn stream_text(path: &Path) -> String {
    let doc = redact_pdf::load(path).expect("PDF ladbar");
    let mut out = String::new();
    for page_id in doc.get_pages().values() {
        if let Ok(data) = doc.get_page_content(*page_id) {
            out.push_str(&String::from_utf8_lossy(&data));
        }
    }
    out
}

fn demo(dir: &Path) -> PathBuf {
    let pdf = dir.join("kontoauszug.pdf");
    let out = run(&["--write-demo", pdf.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    pdf
}

#[test]
fn lists_builtin_patterns() {
    let out = run(&["--list-patterns"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("iban_de"));
    assert!(text.contains("bic"));
}

#[test]
fn redacts_with_patterns() {
    let dir = workdir("patterns");
    let input = demo(&dir);
    let output = dir.join("out.pdf");

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--patterns",
        "iban_de,email",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let text = stream_text(&output);
    assert!(!text.contains("DE89"), "IBAN noch im Stream");
    assert!(!text.contains("example.org"), "E-Mail noch im Stream");
    assert!(text.contains("Musterbank"), "unbeteiligter Text verloren");
}

#[test]
fn negative_list_prevents_redaction() {
    let dir = workdir("negative");
    let input = demo(&dir);
    let csv = dir.join("buchungen.csv");
    std::fs::write(
        &csv,
        "id,list_type,pattern,context_before,context_after,is_regex\n\
         n001,negative,\"DE89 3704 0044 0532 0130 00\",,,\n",
    )
    .unwrap();
    let output = dir.join("out.pdf");

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--patterns",
        "iban_de",
        "--booking-list",
        csv.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout(&out).contains("blockiert"),
        "Blockade nicht gemeldet"
    );

    let text = stream_text(&output);
    assert!(text.contains("DE89"), "Negativliste hat nicht geschützt");
    // Die IBAN auf Seite 2 steht nicht auf der Negativliste und muss weg sein.
    assert!(!text.contains("DE02"), "zweite IBAN nicht geschwärzt");
}

#[test]
fn positive_list_forces_redaction_without_patterns() {
    let dir = workdir("positive");
    let input = demo(&dir);
    let csv = dir.join("buchungen.csv");
    std::fs::write(
        &csv,
        "id,list_type,pattern,context_before,context_after,is_regex\n\
         b001,positive,\"Musterfirma GmbH\",,,\n",
    )
    .unwrap();
    let output = dir.join("out.pdf");

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--no-patterns",
        "--booking-list",
        csv.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let text = stream_text(&output);
    assert!(
        !text.contains("Musterfirma"),
        "Positivtreffer nicht geschwärzt"
    );
    assert!(text.contains("DE89"), "ohne Patterns darf die IBAN bleiben");
}

#[test]
fn review_then_apply_roundtrip() {
    let dir = workdir("review");
    let input = demo(&dir);
    let review = dir.join("review.json");
    let output = dir.join("out.pdf");

    let out = run(&[
        input.to_str().unwrap(),
        "--review",
        "--review-out",
        review.to_str().unwrap(),
        "--patterns",
        "iban_de",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(review.exists());

    // Ersten Treffer abwählen — er darf danach nicht geschwärzt werden.
    let data = std::fs::read_to_string(&review).unwrap();
    let mut file: redact_core::ReviewFile = serde_json::from_str(&data).unwrap();
    assert!(
        file.items.len() >= 2,
        "es sollten zwei IBANs gefunden werden"
    );
    file.items[0].enabled = false;
    std::fs::write(&review, file.to_json().unwrap()).unwrap();

    let audit = dir.join("audit.json");
    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--apply-review",
        review.to_str().unwrap(),
        "--audit-log",
        audit.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let text = stream_text(&output);
    assert!(
        text.contains("DE89"),
        "abgewählter Treffer wurde trotzdem geschwärzt"
    );
    assert!(
        !text.contains("DE02"),
        "ausgewählter Treffer nicht geschwärzt"
    );

    let log: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&audit).unwrap()).unwrap();
    assert_eq!(log["redactions"].as_array().unwrap().len(), 1);
    assert_eq!(log["metadata_stripped"], serde_json::json!(true));
    assert_eq!(log["input"]["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(log["output"]["sha256"].as_str().unwrap().len(), 64);
}

#[test]
fn review_file_from_another_document_is_rejected() {
    let dir = workdir("mismatch");
    let input = demo(&dir);
    let other = dir.join("andere.pdf");
    std::fs::write(&other, redact_pdf::testing::minimal_pdf("nichts geheimes")).unwrap();
    let review = dir.join("review.json");

    run(&[
        input.to_str().unwrap(),
        "--review",
        "--review-out",
        review.to_str().unwrap(),
    ]);

    let out = run(&[
        other.to_str().unwrap(),
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
        "--apply-review",
        review.to_str().unwrap(),
    ]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("andere Eingabe"));
}

#[test]
fn manual_regions_are_applied() {
    let dir = workdir("manual");
    let input = demo(&dir);
    let regions = dir.join("regions.json");
    // Deckt die Zeile „Kontoinhaber: Max Mustermann“ bei y≈750 ab.
    std::fs::write(
        &regions,
        r#"[{"page":0,
             "rect":{"ll":{"x":60.0,"y":745.0},"ur":{"x":300.0,"y":760.0}},
             "text":null,
             "source":{"manual":{"reason":"Kontoinhaber"}}}]"#,
    )
    .unwrap();
    let output = dir.join("out.pdf");

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--no-patterns",
        "--manual-regions",
        regions.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = stream_text(&output);
    assert!(
        !text.contains("Max Mustermann"),
        "manuelle Region nicht angewendet"
    );
    assert!(text.contains("Musterbank"), "zu viel geschwärzt");
}

#[test]
fn rejects_broken_pdf_with_clear_message() {
    let dir = workdir("broken");
    let broken = dir.join("kaputt.pdf");
    std::fs::write(&broken, b"das ist kein PDF").unwrap();

    let out = run(&[
        broken.to_str().unwrap(),
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
    ]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("PDF-Fehler"), "unklare Meldung: {err}");
    assert!(err.contains("kaputt.pdf"), "Dateiname fehlt: {err}");
}

#[test]
fn refuses_to_overwrite_the_input() {
    let dir = workdir("overwrite");
    let input = demo(&dir);
    let out = run(&[input.to_str().unwrap(), "-o", input.to_str().unwrap()]);
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("identisch"));
}

#[test]
fn json_output_is_machine_readable() {
    let dir = workdir("json");
    let input = demo(&dir);
    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("out.pdf").to_str().unwrap(),
        "--patterns",
        "iban_de",
        "--json",
    ]);
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(value["pages"], serde_json::json!(2));
    assert_eq!(value["redactions"], serde_json::json!(2));
    assert!(value["removed_glyphs"].as_u64().unwrap() > 40);
}

#[test]
fn same_input_and_config_produce_identical_output() {
    let dir = workdir("deterministic");
    let input = demo(&dir);
    let a = dir.join("a.pdf");
    let b = dir.join("b.pdf");
    for target in [&a, &b] {
        let out = run(&[
            input.to_str().unwrap(),
            "-o",
            target.to_str().unwrap(),
            "--patterns",
            "iban_de,bic",
        ]);
        assert!(out.status.success());
    }
    assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
}
