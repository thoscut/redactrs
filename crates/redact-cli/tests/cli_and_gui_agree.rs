//! CLI und GUI müssen dasselbe tun — gemessen, nicht behauptet (Aufgabe 5).
//!
//! ## Was hier vorher stand
//!
//! Der Vorgänger dieses Tests lebte in `redact-gui` und hieß
//! `export_matches_a_hand_built_pipeline_of_the_same_steps`. Er verglich den
//! Export **mit einer daneben von Hand aufgeschriebenen Kette** aus denselben
//! Bausteinen. Damit konnte er nie fehlschlagen: wich die Oberfläche von der
//! Kommandozeile ab, wich die Handarbeit im Test genauso ab. Genau das ist
//! passiert — Polsterung fest auf 1,0, `PdfRenderer::new()` statt
//! `with_options`, ein selbst zusammengesetztes Audit-Log mit leeren
//! Prüfsummen —, und kein Test hat gemuckt.
//!
//! ## Was hier jetzt steht
//!
//! Beide Wege laufen wirklich:
//!
//! * das gebaute Binary `redact-rs` als eigener Prozess,
//! * die Oberfläche über `redact_gui::AppState` (laden → analysieren →
//!   exportieren), also derselbe Code, den die Knöpfe aufrufen.
//!
//! Verglichen wird die Ausgabedatei **Byte für Byte** und das Audit-Log Feld
//! für Feld (bis auf Zeitstempel und Dateinamen, die zwangsläufig
//! auseinandergehen).
//!
//! Und weil ein Vergleich, der nichts sieht, wertlos ist, macht der Test die
//! Gegenprobe gleich selbst: derselbe Lauf mit der alten fest verdrahteten
//! Polsterung 1,0 **muss** einen Unterschied ergeben.

#![cfg(feature = "gui")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use redact_gui::AppState;
use redact_pipeline::Config;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("redact-parity-{}-{name}", std::process::id()));
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
fn succeeds(out: &Output) {
    assert!(
        out.status.success(),
        "Lauf fehlgeschlagen: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn demo(dir: &Path) -> PathBuf {
    let path = dir.join("kontoauszug.pdf");
    std::fs::write(&path, redact_pdf::testing::demo_statement()).unwrap();
    path
}

/// Die Einstellungen des Laufs — für beide Wege dieselben.
fn config(input: &Path, padding: f64) -> Config {
    Config {
        input: input.to_path_buf(),
        patterns: vec!["iban_de".to_string()],
        padding,
        ..Config::default()
    }
}

/// Der Weg durch die Oberfläche: laden, analysieren, exportieren.
fn export_through_the_window(config: Config, out: &Path, audit: Option<&Path>) -> Vec<u8> {
    let mut state = AppState::with_config(config);
    state
        .load_document(&state.config.input.clone())
        .expect("PDF ladbar");
    state.analyze().expect("Analyse läuft");
    assert!(!state.regions.is_empty(), "die Analyse fand nichts");
    state.export(out, audit).expect("Export läuft");
    std::fs::read(out).expect("Ausgabedatei lesbar")
}

/// Das Audit-Log ohne die Felder, die zwangsläufig verschieden sind.
///
/// Der Zeitstempel wird zweimal genommen (die Läufe liegen Sekunden
/// auseinander), und die Pfade müssen verschieden sein — beide Wege schreiben
/// ja in unterschiedliche Dateien. Alles andere, insbesondere **beide
/// Prüfsummen**, die gemessene Wirkung und die Metadatenbilanz, wird
/// verglichen.
fn comparable_log(path: &Path) -> serde_json::Value {
    let mut log: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("Audit-Log lesbar"))
            .expect("Audit-Log ist JSON");
    log["timestamp"] = serde_json::json!("<zeit>");
    log["output"]["path"] = serde_json::json!("<ausgabe>");
    log
}

/// Beide Wege, ein Ergebnis.
#[test]
fn the_binary_and_the_window_produce_the_same_file_and_the_same_log() {
    let dir = workdir("same");
    let input = demo(&dir);

    // --- Weg 1: das Binary, als eigener Prozess.
    let cli_out = dir.join("cli.pdf");
    let cli_audit = dir.join("cli_audit.json");
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

    // --- Weg 2: die Oberfläche, mit denselben Einstellungen.
    let gui_out = dir.join("gui.pdf");
    let gui_audit = dir.join("gui_audit.json");
    let gui_bytes = export_through_the_window(config(&input, 3.0), &gui_out, Some(&gui_audit));

    assert_eq!(
        cli_bytes,
        gui_bytes,
        "Kommandozeile und Oberfläche schreiben verschiedene Dateien \
         ({} vs. {} Byte)",
        cli_bytes.len(),
        gui_bytes.len()
    );

    let cli_log = comparable_log(&cli_audit);
    let gui_log = comparable_log(&gui_audit);
    assert_eq!(
        cli_log,
        gui_log,
        "die Audit-Logs gehen auseinander:\nCLI: {}\nGUI: {}",
        serde_json::to_string_pretty(&cli_log).unwrap(),
        serde_json::to_string_pretty(&gui_log).unwrap()
    );

    // Und das Log bezeugt wirklich etwas: beide Prüfsummen sind da.
    assert_eq!(cli_log["input"]["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(cli_log["output"]["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(cli_log["effect"]["padding"], serde_json::json!(3.0));

    std::fs::remove_dir_all(&dir).ok();
}

/// **Gegenprobe.** Weicht einer der beiden Wege ab, muss der Vergleich oben
/// anschlagen — sonst prüft er nichts.
///
/// Nachgestellt wird der Unterschied, der tatsächlich bestand: die Oberfläche
/// polsterte mit fest verdrahteten 1,0, während `--padding` an der
/// Fenstergrenze endete. Genau dieser Fall muss zu verschiedenen Bytes führen.
#[test]
fn a_deviating_window_would_be_caught() {
    let dir = workdir("deviating");
    let input = demo(&dir);

    let cli_out = dir.join("cli.pdf");
    succeeds(&run(&[
        input.to_str().unwrap(),
        "-o",
        cli_out.to_str().unwrap(),
        "--patterns",
        "iban_de",
        "--padding",
        "3",
    ]));
    let cli_bytes = std::fs::read(&cli_out).unwrap();

    // Der alte Zustand: die Oberfläche rechnet mit 1,0, egal was gefordert war.
    let old = export_through_the_window(config(&input, 1.0), &dir.join("alt.pdf"), None);
    assert_ne!(
        cli_bytes, old,
        "der Vergleich würde eine fest verdrahtete Polsterung nicht bemerken"
    );

    // Mit der geforderten Polsterung sind sie wieder gleich — der Unterschied
    // lag also am Wert und nicht am Vergleich.
    let now = export_through_the_window(config(&input, 3.0), &dir.join("neu.pdf"), None);
    assert_eq!(cli_bytes, now);

    std::fs::remove_dir_all(&dir).ok();
}

/// Auch die Review-Datei ist zwischen beiden Wegen austauschbar — und beide
/// legen sie mit denselben Rechten an.
#[test]
fn both_ways_write_the_same_review_file() {
    let dir = workdir("review");
    let input = demo(&dir);

    let cli_review = dir.join("cli_review.json");
    succeeds(&run(&[
        input.to_str().unwrap(),
        "--review",
        "--review-out",
        cli_review.to_str().unwrap(),
        "--patterns",
        "iban_de",
    ]));

    let mut state = AppState::with_config(config(&input, 1.0));
    state.load_document(&input).unwrap();
    state.analyze().unwrap();
    let gui_review = dir.join("gui_review.json");
    state.save_review_file(&gui_review).unwrap();

    let normalize = |path: &Path| -> serde_json::Value {
        let mut value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        // Die Oberfläche schreibt jeden gefundenen Eintrag samt `enabled`; die
        // Kommandozeile schreibt dieselbe Liste. Der Dateiname des Reviews
        // steht nicht drin, der des Dokuments schon — und der ist gleich.
        value["input"]["sha256"] = serde_json::json!("<sha>");
        value
    };
    assert_eq!(normalize(&cli_review), normalize(&gui_review));
    // Die Prüfsumme ist in beiden Dateien dieselbe und nicht leer.
    let cli_sha =
        serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&cli_review).unwrap())
            .unwrap()["input"]["sha256"]
            .as_str()
            .unwrap()
            .to_string();
    let gui_sha =
        serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&gui_review).unwrap())
            .unwrap()["input"]["sha256"]
            .as_str()
            .unwrap()
            .to_string();
    assert_eq!(cli_sha.len(), 64);
    assert_eq!(cli_sha, gui_sha);

    // Klartext in beiden Dateien, also 0600 in beiden Fällen — die Oberfläche
    // hat sie früher mit `std::fs::write` und damit 0644 angelegt.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [&cli_review, &gui_review] {
            let mode = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "{} steht auf {mode:o}", path.display());
        }
    }

    std::fs::remove_dir_all(&dir).ok();
}
