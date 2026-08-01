//! `--password`: verschlüsselte PDFs öffnen — und das Passwort nirgends
//! stehen lassen.
//!
//! Zwei Dinge werden gemessen, nicht behauptet:
//!
//! 1. Eine **wirklich** verschlüsselte Datei (aus
//!    [`redact_pipeline::testing::ENCRYPTED_PDF`]) wird ohne Passwort
//!    abgelehnt, mit Passwort geöffnet und geschwärzt.
//! 2. Das Passwort taucht in **keiner** erzeugten Datei auf — nicht in der
//!    Ausgabe-PDF, nicht im Audit-Log, nicht in der Review-Datei — und in
//!    keiner Meldung auf stdout oder stderr, auch nicht der Fehlermeldung des
//!    falschen Passworts.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use redact_pipeline::testing::{ENCRYPTED_PDF, ENCRYPTED_PDF_IBAN, ENCRYPTED_PDF_PASSWORD};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("redact-pw-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Die verschlüsselte Testdatei auf der Platte.
fn encrypted(dir: &Path) -> PathBuf {
    let path = dir.join("verschluesselt.pdf");
    std::fs::write(&path, ENCRYPTED_PDF).unwrap();
    path
}

fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        // Damit eine Einstellungsdatei des Systems den Lauf nicht verändert.
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

/// Ohne Passwort bleibt es bei der klaren Ablehnung — das ist richtig so.
#[test]
fn an_encrypted_pdf_is_still_refused_without_a_password() {
    let dir = workdir("refused");
    let input = encrypted(&dir);

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("x.pdf").to_str().unwrap(),
    ]);
    assert!(!out.status.success(), "verschlüsselt und trotzdem geöffnet");
    let message = stderr(&out);
    assert!(message.contains("verschlüsselt"), "{message}");
    assert!(
        !dir.join("x.pdf").exists(),
        "es wurde trotz Ablehnung geschrieben"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Mit Passwort wird die Datei geöffnet **und** geschwärzt.
#[test]
fn with_the_password_the_document_is_opened_and_redacted() {
    let dir = workdir("open");
    let input = encrypted(&dir);
    let output = dir.join("out.pdf");

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--password",
        ENCRYPTED_PDF_PASSWORD,
        "--patterns",
        "iban_de",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));

    // Gegenprobe zum Test „ohne Passwort abgelehnt“: hier wurde wirklich
    // gearbeitet, die IBAN ist gefunden und verschwunden.
    assert!(stdout(&out).contains("Schwärzungen:"), "{}", stdout(&out));
    let leaks = redact_pdf::leaks(&std::fs::read(&output).unwrap(), "DE89");
    assert!(leaks.is_empty(), "IBAN steht noch da: {leaks:?}");

    std::fs::remove_dir_all(&dir).ok();
}

/// Der Weg an Prozessliste und Shell-Historie vorbei.
#[test]
fn the_environment_variable_works_just_as_well() {
    let dir = workdir("env");
    let input = encrypted(&dir);
    let output = dir.join("out.pdf");

    let out = Command::new(bin())
        .args([
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
            "--patterns",
            "iban_de",
        ])
        .env("REDACT_RS_CONFIG", "/nicht/vorhanden.yaml")
        .env("REDACT_RS_PASSWORD", ENCRYPTED_PDF_PASSWORD)
        .output()
        .expect("Binary startbar");

    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(output.exists());
    let leaks = redact_pdf::leaks(&std::fs::read(&output).unwrap(), "DE89");
    assert!(leaks.is_empty(), "IBAN steht noch da: {leaks:?}");

    std::fs::remove_dir_all(&dir).ok();
}

/// Ein falsches Passwort scheitert — und verrät sich nicht.
#[test]
fn a_wrong_password_fails_without_naming_itself() {
    const WRONG: &str = "ganz-falsches-passwort-4711";
    let dir = workdir("wrong");
    let input = encrypted(&dir);

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("x.pdf").to_str().unwrap(),
        "--password",
        WRONG,
    ]);
    assert!(!out.status.success(), "falsches Passwort ging durch");
    let message = stderr(&out);
    assert!(message.contains("Passwort"), "{message}");
    assert!(
        !message.contains(WRONG),
        "das Passwort steht im Text: {message}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// **Die Auflage:** das Passwort landet in keiner erzeugten Datei.
///
/// Geprüft wird über alle drei Dateien eines vollständigen Laufs — die
/// geschwärzte PDF (auf allen Ebenen, mit [`redact_pdf::leaks`]), das
/// Audit-Log und die Review-Datei — und zusätzlich über beide Ausgabekanäle.
#[test]
fn the_password_appears_in_no_file_the_run_produces() {
    let dir = workdir("leak");
    let input = encrypted(&dir);
    let output = dir.join("out.pdf");
    let audit = dir.join("audit.json");
    let review = dir.join("review.json");

    // 1. Review-Export.
    let reviewed = run(&[
        input.to_str().unwrap(),
        "--review",
        "--review-out",
        review.to_str().unwrap(),
        "--password",
        ENCRYPTED_PDF_PASSWORD,
        "--patterns",
        "iban_de",
    ]);
    assert!(reviewed.status.success(), "{}", stderr(&reviewed));

    // 2. Schwärzen mit Audit-Log.
    let redacted = run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--audit-log",
        audit.to_str().unwrap(),
        "--password",
        ENCRYPTED_PDF_PASSWORD,
        "--patterns",
        "iban_de",
    ]);
    assert!(redacted.status.success(), "{}", stderr(&redacted));

    // Die Review-Datei bezeugt, dass wirklich analysiert wurde — sonst
    // prüfte dieser Test nur leere Dateien.
    let review_text = std::fs::read_to_string(&review).unwrap();
    assert!(
        review_text.contains(ENCRYPTED_PDF_IBAN),
        "die Analyse fand nichts, der Test prüfte nichts:\n{review_text}"
    );

    for path in [&output, &audit, &review] {
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(
            !bytes
                .windows(ENCRYPTED_PDF_PASSWORD.len())
                .any(|w| w == ENCRYPTED_PDF_PASSWORD.as_bytes()),
            "das Passwort steht in {}",
            path.display()
        );
        // Und auch nicht in irgendeinem entpackten Stream der PDF.
        assert!(
            redact_pdf::leaks(&bytes, ENCRYPTED_PDF_PASSWORD).is_empty(),
            "das Passwort steht (kodiert) in {}",
            path.display()
        );
    }

    for out in [&reviewed, &redacted] {
        assert!(!stdout(out).contains(ENCRYPTED_PDF_PASSWORD));
        assert!(!stderr(out).contains(ENCRYPTED_PDF_PASSWORD));
    }

    std::fs::remove_dir_all(&dir).ok();
}

/// Der Hilfetext muss vor der Prozessliste warnen und den anderen Weg nennen.
///
/// Die Auflage lautet „in die Hilfe **und** in SECURITY.md“; die Hilfe steht
/// hier, die Datei prüft [`the_security_notes_name_both_ways`].
#[test]
fn the_help_warns_about_the_process_list() {
    let text = stdout(&run(&["--help"]));
    assert!(text.contains("REDACT_RS_PASSWORD"), "{text}");
    assert!(text.contains("Prozessliste"), "die Warnung fehlt:\n{text}");
    assert!(text.contains("Historie"), "die Warnung fehlt:\n{text}");
}

/// Und dasselbe steht in `SECURITY.md` — die Auflage nennt beide Orte.
#[test]
fn the_security_notes_name_both_ways() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../SECURITY.md");
    let text = std::fs::read_to_string(&path).expect("SECURITY.md lesbar");
    assert!(
        text.contains("REDACT_RS_PASSWORD"),
        "SECURITY.md schweigt zur Umgebungsvariable"
    );
    assert!(
        text.contains("Prozessliste"),
        "SECURITY.md schweigt zur Prozessliste"
    );
}
