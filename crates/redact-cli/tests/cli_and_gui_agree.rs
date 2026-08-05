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

/// Dieselben Einstellungen, aber mit Ersatztext statt schwarzem Balken.
fn replacing_config(input: &Path, padding: f64) -> Config {
    Config {
        action: redact_core::Action::Replace("[IBAN]".to_string()),
        ..config(input, padding)
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

/// Dieselbe Gleichheit mit `--action replace --replace-with "[IBAN]"`.
///
/// Der Vergleich oben lief nur mit der Vorgabe-Aktion und hätte einen
/// Unterschied in der Behandlung von `--action` nicht bemerkt: beide Wege
/// hätten schwarz geschwärzt und wären sich darin einig gewesen. Ein Ersatztext
/// fasst mehr an — Deck-Rechteck **und** eine neue Font-Ressource auf der Seite
/// —, ist also der schärfere Vergleich.
#[test]
fn the_binary_and_the_window_agree_on_a_replacement_too() {
    let dir = workdir("replace");
    let input = demo(&dir);

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
        "--action",
        "replace",
        "--replace-with",
        "[IBAN]",
        "--audit-log",
        cli_audit.to_str().unwrap(),
    ]));
    let cli_bytes = std::fs::read(&cli_out).unwrap();

    let gui_out = dir.join("gui.pdf");
    let gui_audit = dir.join("gui_audit.json");
    let gui_bytes =
        export_through_the_window(replacing_config(&input, 3.0), &gui_out, Some(&gui_audit));

    assert_eq!(
        cli_bytes,
        gui_bytes,
        "mit --action replace schreiben Kommandozeile und Oberfläche \
         verschiedene Dateien ({} vs. {} Byte)",
        cli_bytes.len(),
        gui_bytes.len()
    );
    let cli_log = comparable_log(&cli_audit);
    assert_eq!(cli_log, comparable_log(&gui_audit));
    // Und der Ersatztext ist wirklich angekommen — sonst verglichen wir zwei
    // schwarze Balken miteinander.
    assert_eq!(
        cli_log["redactions"][0]["action"],
        serde_json::json!({ "replace": "[IBAN]" }),
        "{cli_log}"
    );
    assert!(
        !redact_pdf::leaks(&cli_bytes, "[IBAN]").is_empty(),
        "der Ersatztext steht nicht in der Ausgabe"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Auch die Review-Datei ist mit `--action replace` zwischen beiden Wegen
/// austauschbar.
///
/// `both_ways_write_the_same_review_file` lief nur mit der Vorgabe-Aktion und
/// blieb deshalb selbst dann grün, als die Kommandozeile in jeden Eintrag
/// „blackout“ schrieb (siehe `review_file_records_the_action_of_the_run` in
/// `cli.rs`).
#[test]
fn both_ways_write_the_same_review_file_with_a_replacement() {
    let dir = workdir("review-action");
    let input = demo(&dir);

    let cli_review = dir.join("cli_review.json");
    succeeds(&run(&[
        input.to_str().unwrap(),
        "--review",
        "--review-out",
        cli_review.to_str().unwrap(),
        "--patterns",
        "iban_de",
        "--action",
        "replace",
        "--replace-with",
        "[IBAN]",
    ]));

    let review: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&cli_review).unwrap()).unwrap();
    assert!(!review["items"].as_array().unwrap().is_empty());

    // Und die Oberfläche schreibt dieselbe Datei.
    let mut state = AppState::with_config(replacing_config(&input, 1.0));
    state.load_document(&input).unwrap();
    state.analyze().unwrap();
    let gui_review = dir.join("gui_review.json");
    state.save_review_file(&gui_review).unwrap();
    let gui: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&gui_review).unwrap()).unwrap();
    assert_eq!(review["items"], gui["items"]);

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

/// Und dieselbe Gleichheit mit **abgeschalteter Erkennung**.
///
/// Der neue Schalter geht denselben Weg wie alles andere: er steht in
/// [`Config`], und beide Programme lesen ihn dort. Verglichen wird deshalb auch
/// hier Byte für Byte und Feld für Feld — samt dem neuen Log-Feld `patterns`
/// und dem Satz in `warnings`, der die Abschaltung benennt. Ohne diesen Test
/// könnte die Oberfläche die Abschaltung übergehen (mehr schwärzen) oder sie
/// nicht protokollieren (dieselbe Datei, ein schweigender Nachweis) — beides
/// bliebe im Vergleich der übrigen Tests unsichtbar.
#[test]
fn the_binary_and_the_window_agree_on_a_switched_off_pattern() {
    let dir = workdir("disabled");
    let input = demo(&dir);

    // `email` ist ein Muster, das der Demo-Auszug wirklich trifft — sonst
    // verglichen wir zwei Läufe, in denen die Abschaltung nichts bewirkt.
    let einstellungen = |input: &Path| Config {
        input: input.to_path_buf(),
        disabled_patterns: vec!["email".to_string()],
        padding: 3.0,
        ..Config::default()
    };

    let cli_out = dir.join("cli.pdf");
    let cli_audit = dir.join("cli_audit.json");
    succeeds(&run(&[
        input.to_str().unwrap(),
        "-o",
        cli_out.to_str().unwrap(),
        "--padding",
        "3",
        "--disable-pattern",
        "email",
        "--audit-log",
        cli_audit.to_str().unwrap(),
    ]));
    let cli_bytes = std::fs::read(&cli_out).unwrap();

    let gui_out = dir.join("gui.pdf");
    let gui_audit = dir.join("gui_audit.json");
    let gui_bytes = export_through_the_window(einstellungen(&input), &gui_out, Some(&gui_audit));

    assert_eq!(
        cli_bytes,
        gui_bytes,
        "mit --disable-pattern schreiben Kommandozeile und Oberfläche \
         verschiedene Dateien ({} vs. {} Byte)",
        cli_bytes.len(),
        gui_bytes.len()
    );
    let cli_log = comparable_log(&cli_audit);
    assert_eq!(cli_log, comparable_log(&gui_audit));

    // Der Nachweis sagt es — in beiden Fassungen, denn die Logs sind gleich.
    assert_eq!(
        cli_log["patterns"],
        serde_json::json!({ "all_disabled": false, "disabled": ["email"] }),
        "{cli_log}"
    );
    assert!(
        cli_log["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("Automatische Erkennung:")),
        "{cli_log}"
    );

    // **Gegenprobe.** Ohne die Abschaltung ist es eine andere Datei — der
    // Vergleich oben misst also wirklich etwas.
    let ohne = export_through_the_window(
        Config {
            disabled_patterns: Vec::new(),
            ..einstellungen(&input)
        },
        &dir.join("ohne.pdf"),
        None,
    );
    assert_ne!(
        cli_bytes, ohne,
        "eine Oberfläche, die --disable-pattern übergeht, fiele hier nicht auf"
    );
    // Und der Grund für den Unterschied steht in der Datei: die Adresse ist
    // nur im Lauf ohne Abschaltung verschwunden.
    assert!(
        !redact_pdf::leaks(&cli_bytes, "max.mustermann@example.org").is_empty(),
        "das abgeschaltete Muster hat trotzdem geschwärzt"
    );
    assert!(
        redact_pdf::leaks(&ohne, "max.mustermann@example.org").is_empty(),
        "ohne Abschaltung muss die Adresse weg sein"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// „Alles aus“ geht denselben Weg: beide Programme schwärzen dann nur, was von
/// Hand gezogen wurde — und schreiben dieselbe Datei.
#[test]
fn the_binary_and_the_window_agree_with_no_patterns_at_all() {
    let dir = workdir("no-patterns");
    let input = demo(&dir);

    // Ohne Muster braucht es eine Region von Hand, sonst gäbe es nichts zu
    // schwärzen und keine Ausgabe — auf beiden Wegen.
    let regionen = dir.join("regionen.json");
    let rect = redact_core::Rect::new(70.0, 730.0, 300.0, 745.0);
    std::fs::write(
        &regionen,
        serde_json::to_string(&vec![redact_core::Region::new(
            0,
            rect,
            None,
            redact_core::Source::Manual {
                reason: "IBAN-Zeile".to_string(),
            },
        )])
        .unwrap(),
    )
    .unwrap();

    let cli_out = dir.join("cli.pdf");
    let cli_audit = dir.join("cli_audit.json");
    succeeds(&run(&[
        input.to_str().unwrap(),
        "-o",
        cli_out.to_str().unwrap(),
        "--no-patterns",
        "--manual-regions",
        regionen.to_str().unwrap(),
        "--audit-log",
        cli_audit.to_str().unwrap(),
    ]));
    let cli_bytes = std::fs::read(&cli_out).unwrap();

    let gui_out = dir.join("gui.pdf");
    let gui_audit = dir.join("gui_audit.json");
    let gui_bytes = export_through_the_window(
        Config {
            input: input.clone(),
            no_patterns: true,
            manual_regions: Some(regionen.clone()),
            patterns: Vec::new(),
            ..Config::default()
        },
        &gui_out,
        Some(&gui_audit),
    );

    assert_eq!(cli_bytes, gui_bytes, "„alles aus“ läuft auseinander");
    let cli_log = comparable_log(&cli_audit);
    assert_eq!(cli_log, comparable_log(&gui_audit));
    assert_eq!(cli_log["patterns"]["all_disabled"], serde_json::json!(true));
    // Die Handregion hat wirklich gewirkt — sonst verglichen wir zwei
    // unveränderte Kopien.
    assert_eq!(
        cli_log["effect"]["applied"],
        serde_json::json!(1),
        "{cli_log}"
    );
    assert!(redact_pdf::leaks(&cli_bytes, "DE89 3704 0044 0532 0130 00").is_empty());
    // Und was kein Mensch markiert hat, steht in beiden Dateien noch da.
    assert!(!redact_pdf::leaks(&cli_bytes, "COBADEFFXXX").is_empty());

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
