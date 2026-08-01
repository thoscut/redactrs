//! Stapelverarbeitung am gebauten Binary.
//!
//! Die beiden Eigenschaften, auf die es bei einem Werkzeug ankommt, dem man
//! Kontoauszüge anvertraut:
//!
//! * **Eine kaputte Datei bricht den Stapel nicht ab.** Sie wird gemeldet, der
//!   Rest läuft weiter, und der Rückgabewert sagt am Ende, ob alles gut ging.
//! * **Kein Ergebnis überschreibt ein anderes.** Zwei gleichnamige Dateien aus
//!   verschiedenen Verzeichnissen ergeben zwei Ergebnisse, jedes neben seiner
//!   Eingabe.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("redact-batch-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .env("REDACT_RS_CONFIG", "/nicht/vorhanden.yaml")
        .env_remove("REDACT_RS_PASSWORD")
        .output()
        .expect("Binary startbar")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

const IBAN: &str = "DE89 3704 0044 0532 0130 00";

/// Ein einseitiges PDF mit einer Kennzeichnung und der IBAN.
fn pdf_with(dir: &Path, name: &str, marker: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(
        &path,
        redact_pdf::testing::minimal_pdf(&format!("{marker} IBAN {IBAN}")),
    )
    .unwrap();
    path
}

fn visible_text(path: &Path) -> String {
    let doc = redact_pdf::load(path).expect("PDF ladbar");
    redact_pdf::PdfExtractor::new()
        .extract(&doc)
        .expect("Extraktion")
        .iter()
        .map(|run| run.text.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

#[track_caller]
fn assert_redacted(path: &Path) {
    assert!(path.exists(), "Ergebnis fehlt: {}", path.display());
    let leaks = redact_pdf::leaks(&std::fs::read(path).unwrap(), "DE89");
    assert!(leaks.is_empty(), "{}: IBAN steht noch da", path.display());
}

/// Mehrere Dateien: je eine Ausgabe daneben, eine Zusammenfassung am Ende.
#[test]
fn several_files_each_get_their_own_result() {
    let dir = workdir("mehrere");
    let a = pdf_with(&dir, "a.pdf", "Konto A");
    let b = pdf_with(&dir, "b.pdf", "Konto B");

    let out = run(&[
        a.to_str().unwrap(),
        b.to_str().unwrap(),
        "--patterns",
        "iban_de",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));

    assert_redacted(&dir.join("a_geschwaerzt.pdf"));
    assert_redacted(&dir.join("b_geschwaerzt.pdf"));

    let summary = stdout(&out);
    assert!(summary.contains("2 Datei(en)"), "{summary}");
    assert!(summary.contains("0 fehlgeschlagen"), "{summary}");

    std::fs::remove_dir_all(&dir).ok();
}

/// Ein Verzeichnis wird zum Stapel — ohne die Ergebnisse eines früheren Laufs.
#[test]
fn a_directory_is_processed_and_a_second_run_does_not_cascade() {
    let dir = workdir("verzeichnis");
    pdf_with(&dir, "eins.pdf", "Konto 1");
    pdf_with(&dir, "zwei.pdf", "Konto 2");
    std::fs::write(dir.join("notiz.txt"), b"kein PDF").unwrap();

    let out = run(&[dir.to_str().unwrap(), "--patterns", "iban_de"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_redacted(&dir.join("eins_geschwaerzt.pdf"));
    assert_redacted(&dir.join("zwei_geschwaerzt.pdf"));

    // Ein zweiter Lauf über dasselbe Verzeichnis darf nicht die Ergebnisse
    // des ersten noch einmal schwärzen.
    let again = run(&[dir.to_str().unwrap(), "--patterns", "iban_de", "--force"]);
    assert!(again.status.success(), "{}", stderr(&again));
    assert!(
        !dir.join("eins_geschwaerzt_geschwaerzt.pdf").exists(),
        "der zweite Lauf hat sein eigenes Ergebnis noch einmal genommen"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// **Die Kernforderung:** eine kaputte Datei mittendrin reißt nichts mit.
#[test]
fn a_broken_file_is_reported_and_the_rest_of_the_batch_runs() {
    let dir = workdir("kaputt");
    let a = pdf_with(&dir, "a.pdf", "Konto A");
    let kaputt = dir.join("b_kaputt.pdf");
    std::fs::write(&kaputt, b"das ist ganz sicher kein PDF").unwrap();
    let c = pdf_with(&dir, "c.pdf", "Konto C");

    let out = run(&[
        a.to_str().unwrap(),
        kaputt.to_str().unwrap(),
        c.to_str().unwrap(),
        "--patterns",
        "iban_de",
    ]);

    // Die heilen Dateien sind fertig — beide, auch die *nach* der kaputten.
    assert_redacted(&dir.join("a_geschwaerzt.pdf"));
    assert_redacted(&dir.join("c_geschwaerzt.pdf"));
    assert!(
        !dir.join("b_kaputt_geschwaerzt.pdf").exists(),
        "für die kaputte Datei entstand ein Ergebnis"
    );

    // Sie wird gemeldet …
    let message = stderr(&out);
    assert!(message.contains("b_kaputt.pdf"), "{message}");

    // … in der Zusammenfassung gezählt …
    //
    // Seit #79 steht dort nicht mehr „2 verarbeitet“, sondern „2 vollständig
    // geprüft“: eine Datei, deren Text nur zum Teil gelesen werden konnte, ist
    // zwar verarbeitet, aber nicht geprüft — und wurde vorher genau hier
    // mitgezählt. Diese beiden hier sind unauffällig, also stehen sie in der
    // ersten Zahl und die mittlere ist 0.
    let summary = stdout(&out);
    assert!(summary.contains("3 Datei(en)"), "{summary}");
    assert!(summary.contains("2 vollständig geprüft"), "{summary}");
    assert!(
        summary.contains("0 verarbeitet (aber nicht vollständig geprüft)"),
        "{summary}"
    );
    assert!(summary.contains("1 fehlgeschlagen"), "{summary}");

    // … und der Rückgabewert sagt, dass nicht alles gut ging.
    assert!(
        !out.status.success(),
        "der Rückgabewert behauptet, alles sei gut gegangen"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Zwei gleichnamige Dateien aus verschiedenen Verzeichnissen überschreiben
/// einander nicht — jedes Ergebnis liegt neben seiner eigenen Eingabe und
/// trägt auch wirklich deren Inhalt.
#[test]
fn two_inputs_of_the_same_name_do_not_overwrite_each_other() {
    let root = workdir("gleichnamig");
    let a_dir = root.join("januar");
    let b_dir = root.join("februar");
    std::fs::create_dir_all(&a_dir).unwrap();
    std::fs::create_dir_all(&b_dir).unwrap();

    let a = pdf_with(&a_dir, "auszug.pdf", "Konto JANUAR");
    let b = pdf_with(&b_dir, "auszug.pdf", "Konto FEBRUAR");

    let out = run(&[
        a.to_str().unwrap(),
        b.to_str().unwrap(),
        "--patterns",
        "iban_de",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));

    let a_out = a_dir.join("auszug_geschwaerzt.pdf");
    let b_out = b_dir.join("auszug_geschwaerzt.pdf");
    assert_redacted(&a_out);
    assert_redacted(&b_out);

    // Und jedes Ergebnis gehört zu seiner Eingabe — nicht zweimal dasselbe.
    assert!(
        visible_text(&a_out).contains("JANUAR"),
        "das Januar-Ergebnis enthält den falschen Auszug"
    );
    assert!(
        visible_text(&b_out).contains("FEBRUAR"),
        "das Februar-Ergebnis enthält den falschen Auszug"
    );

    std::fs::remove_dir_all(&root).ok();
}

/// Ein Ziel für viele Dateien wäre genau das Überschreiben, das nicht
/// passieren darf — deshalb abgelehnt, bevor irgendetwas geschrieben wird.
#[test]
fn a_single_target_is_refused_for_a_batch() {
    let dir = workdir("einziel");
    let a = pdf_with(&dir, "a.pdf", "Konto A");
    let b = pdf_with(&dir, "b.pdf", "Konto B");

    for (switch, value) in [
        ("-o", "alles.pdf"),
        ("--audit-log", "audit.json"),
        ("--review-out", "review.json"),
    ] {
        let target = dir.join(value);
        let out = run(&[
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            switch,
            target.to_str().unwrap(),
        ]);
        assert_eq!(out.status.code(), Some(2), "{switch}: {}", stderr(&out));
        assert!(!target.exists(), "{switch} hat trotzdem geschrieben");
        assert!(!dir.join("a_geschwaerzt.pdf").exists());
    }

    std::fs::remove_dir_all(&dir).ok();
}

/// Mit `--json` kommt eine maschinenlesbare Liste — auch über die
/// gescheiterten Dateien.
#[test]
fn json_lists_every_file_including_the_failures() {
    let dir = workdir("json");
    let a = pdf_with(&dir, "a.pdf", "Konto A");
    let kaputt = dir.join("b.pdf");
    std::fs::write(&kaputt, b"kein PDF").unwrap();

    let out = run(&[
        a.to_str().unwrap(),
        kaputt.to_str().unwrap(),
        "--patterns",
        "iban_de",
        "--json",
    ]);
    let entries: Vec<serde_json::Value> =
        serde_json::from_str(&stdout(&out)).expect("JSON auf stdout");
    assert_eq!(entries.len(), 2);
    assert!(entries[0]["outcome"]["output"].is_string());
    assert!(entries[0]["error"].is_null());
    assert!(entries[1]["outcome"].is_null());
    assert!(
        entries[1]["error"].as_str().unwrap().contains("PDF"),
        "{}",
        entries[1]
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Eine Datei bleibt eine Datei: derselbe Ablauf und dieselbe Ausgabe wie
/// bisher, keine Stapel-Zusammenfassung.
#[test]
fn a_single_file_behaves_exactly_as_before() {
    let dir = workdir("einzeln");
    let a = pdf_with(&dir, "a.pdf", "Konto A");

    let out = run(&[a.to_str().unwrap(), "--patterns", "iban_de"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("Eingabe:"), "{text}");
    assert!(!text.contains("Datei(en):"), "{text}");

    std::fs::remove_dir_all(&dir).ok();
}
