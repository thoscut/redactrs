//! Die automatische Erkennung abschalten — ganz und je Muster.
//!
//! ## Was hier gemessen wird und womit
//!
//! Ein abgeschaltetes Muster ist die eine Einstellung, die eine Datei
//! **unvollständiger** macht, ohne dass man es der Datei ansieht. Die Tests
//! prüfen deshalb drei Dinge, und zwar mit drei verschiedenen Messgeräten:
//!
//! * **Was steht noch drin?** Ausschließlich [`redact_pdf::leaks`]. Der eigene
//!   Extraktor wäre ein Zirkelschluss (wovor er blind ist, wird nicht
//!   geschwärzt und ist auch für ihn unsichtbar), und `pdftotext | grep` gibt
//!   nachweislich falsche Entwarnung — beides steht in `README.md` und im
//!   Modulkommentar von `redact_pdf::audit_bytes`.
//! * **Wie viele Treffer?** Die Zahl aus `--json` (`candidates`).
//! * **Steht die Abschaltung im Nachweis?** Das Feld `patterns` im Audit-Log
//!   und der Satz in `warnings`.
//!
//! Und die Gegenprobe gehört überall dazu: derselbe Lauf **mit** dem Muster
//! muss dasselbe Geheimnis entfernen. Ein Test, der nur zeigt, dass etwas
//! stehen bleibt, ließe eine Fassung durchgehen, die überhaupt nichts mehr
//! schwärzt.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_redact-rs")
}

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("redact-muster-aus-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Ein Lauf mit einer bestimmten Einstellungsdatei (oder ganz ohne).
///
/// Ohne Datei zeigt ein Pfad ins Leere: sonst redete die Einstellungsdatei des
/// Entwicklungsrechners in die Prüfung hinein.
fn run_with(settings: Option<&Path>, args: &[&str]) -> Output {
    let mut command = Command::new(bin());
    command.args(args).env_remove("REDACT_RS_PASSWORD");
    match settings {
        None => command.env("REDACT_RS_CONFIG", "/nicht/vorhanden.yaml"),
        Some(path) => command.env("REDACT_RS_CONFIG", path),
    };
    command.output().expect("Binary startbar")
}

fn run(args: &[&str]) -> Output {
    run_with(None, args)
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[track_caller]
fn succeeds(out: &Output) {
    assert!(out.status.success(), "Lauf fehlgeschlagen: {}", stderr(out));
}

fn demo(dir: &Path) -> PathBuf {
    let path = dir.join("kontoauszug.pdf");
    std::fs::write(&path, redact_pdf::testing::demo_statement()).unwrap();
    path
}

/// **Das ehrliche Messgerät.** Sucht auf allen Ebenen der Datei.
fn leaks(path: &Path, needle: &str) -> Vec<String> {
    let bytes = std::fs::read(path).expect("Ausgabedatei lesbar");
    redact_pdf::leaks(&bytes, needle)
}

#[track_caller]
fn assert_gone(path: &Path, needle: &str) {
    let hits = leaks(path, needle);
    assert!(
        hits.is_empty(),
        "„{needle}“ steht noch {} mal in {}:\n{}",
        hits.len(),
        path.display(),
        hits.join("\n")
    );
}

#[track_caller]
fn assert_still_there(path: &Path, needle: &str) {
    assert!(
        !leaks(path, needle).is_empty(),
        "„{needle}“ ist aus {} verschwunden — dann misst dieser Test nichts",
        path.display()
    );
}

/// Die Zusammenfassung eines Laufs als JSON.
fn summary(out: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(out)).expect("--json liefert JSON")
}

fn audit(path: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(path).expect("Audit-Log lesbar"))
        .expect("Audit-Log ist JSON")
}

/// Geheimnisse aus dem Demo-Auszug, je Muster eines.
const IBAN: &str = "DE89 3704 0044 0532 0130 00";
const EMAIL: &str = "max.mustermann@example.org";
const BIC: &str = "COBADEFFXXX";

// ---------------------------------------------------------------------------
// 1. Ganz aus
// ---------------------------------------------------------------------------

/// **Alles an gegen alles aus** — mit den Trefferzahlen und dem Nachweis, dass
/// die Geheimnisse im zweiten Lauf wirklich noch dastehen.
#[test]
fn everything_on_versus_everything_off() {
    let dir = workdir("ganz-aus");
    let input = demo(&dir);

    // --- alles an
    let an_pdf = dir.join("an.pdf");
    let an = run(&[
        input.to_str().unwrap(),
        "-o",
        an_pdf.to_str().unwrap(),
        "--json",
    ]);
    succeeds(&an);
    let an_json = summary(&an);
    let an_treffer = an_json["candidates"].as_u64().unwrap();
    assert!(
        an_treffer >= 5,
        "der Demo-Auszug gibt mehr her: {an_treffer}"
    );
    assert_gone(&an_pdf, IBAN);
    assert_gone(&an_pdf, EMAIL);
    assert_gone(&an_pdf, BIC);
    assert!(
        an_json["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|w| !w.as_str().unwrap().contains("Automatische Erkennung:")),
        "ohne Abschaltung gehört da keine Ansage hin: {an_json}"
    );

    // --- alles aus: es gibt schlicht nichts zu schwärzen, also auch keine
    // Ausgabe. Das ist die richtige Antwort — eine unveränderte Kopie mit
    // Erfolgsmeldung wäre das Gegenteil.
    let aus_pdf = dir.join("aus.pdf");
    let aus = run(&[
        input.to_str().unwrap(),
        "-o",
        aus_pdf.to_str().unwrap(),
        "--no-patterns",
        "--json",
    ]);
    succeeds(&aus);
    let aus_json = summary(&aus);
    assert_eq!(
        aus_json["candidates"].as_u64().unwrap(),
        0,
        "„aus“ muss null Treffer heißen"
    );
    assert_eq!(aus_json["redactions"].as_u64().unwrap(), 0);

    // Und die Datei ist so ungeschwärzt, wie sie aussieht.
    assert_still_there(&aus_pdf, IBAN);
    assert_still_there(&aus_pdf, EMAIL);
    assert_still_there(&aus_pdf, BIC);

    // Der Unterschied darf nicht stillschweigend bleiben.
    let ansage = aus_json["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|w| {
            let w = w.as_str().unwrap();
            w.contains("Automatische Erkennung:").then_some(w)
        })
        .unwrap_or_else(|| panic!("keine Ansage in der Zusammenfassung: {aus_json}"));
    assert!(ansage.contains("--no-patterns"), "{ansage}");

    std::fs::remove_dir_all(&dir).ok();
}

/// Der Mensch am Terminal sieht es auch ohne `--json` — **auf stdout**, neben
/// der Trefferzahl. `redact-rs … > bericht.txt` behielte sonst nur die
/// harmlose Hälfte.
#[test]
fn the_summary_on_the_console_says_it_too() {
    let dir = workdir("konsole");
    let input = demo(&dir);
    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("aus.pdf").to_str().unwrap(),
        "--no-patterns",
    ]);
    succeeds(&out);
    let text = stdout(&out);
    assert!(
        text.contains("Automatische Erkennung: abgeschaltet"),
        "stdout schweigt:\n{text}"
    );
    // Und der Rückgabewert bleibt 0: es ist eine Anweisung, kein Befund an der
    // Datei. Alles andere machte den Wert 3 für die Fälle wertlos, für die es
    // ihn gibt (siehe `redact_pipeline::coverage`).
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// 2. Einzeln aus
// ---------------------------------------------------------------------------

/// **Ein Muster aus, die übrigen an** — und zwar an genau einem Geheimnis
/// gemessen: die E-Mail-Adresse bleibt stehen, IBAN und BIC verschwinden.
#[test]
fn switching_off_one_pattern_only_spares_its_own_hits() {
    let dir = workdir("einzeln");
    let input = demo(&dir);

    let alle = run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("alle.pdf").to_str().unwrap(),
        "--json",
    ]);
    succeeds(&alle);
    let alle_treffer = summary(&alle)["candidates"].as_u64().unwrap();

    let ohne_pdf = dir.join("ohne_email.pdf");
    let ohne = run(&[
        input.to_str().unwrap(),
        "-o",
        ohne_pdf.to_str().unwrap(),
        "--disable-pattern",
        "email",
        "--json",
    ]);
    succeeds(&ohne);
    let ohne_treffer = summary(&ohne)["candidates"].as_u64().unwrap();
    assert!(
        ohne_treffer < alle_treffer,
        "abgeschaltet und trotzdem gleich viele Treffer: {ohne_treffer} von {alle_treffer}"
    );

    // Das eine Geheimnis steht noch da …
    assert_still_there(&ohne_pdf, EMAIL);
    // … und alle anderen sind weg. Ohne diese zweite Hälfte wäre auch eine
    // Fassung grün, die gar nichts mehr schwärzt.
    assert_gone(&ohne_pdf, IBAN);
    assert_gone(&ohne_pdf, BIC);
    // Gegenprobe: mit dem Muster ist auch die Adresse weg.
    assert_gone(&dir.join("alle.pdf"), EMAIL);

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// 3. Der Nachweis
// ---------------------------------------------------------------------------

/// Das Audit-Log **beider** Läufe: die Abschaltung steht darin — und dass
/// nichts abgeschaltet war, steht auch darin.
#[test]
fn the_audit_log_records_what_was_switched_off() {
    let dir = workdir("protokoll");
    let input = demo(&dir);

    // Lauf 1: alles an.
    let an_log = dir.join("an_audit.json");
    succeeds(&run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("an.pdf").to_str().unwrap(),
        "--audit-log",
        an_log.to_str().unwrap(),
    ]));
    let an = audit(&an_log);
    assert_eq!(
        an["patterns"],
        serde_json::json!({ "all_disabled": false, "disabled": [] }),
        "„nichts abgeschaltet“ gehört genauso bezeugt: {an}"
    );

    // Lauf 2: ein Muster aus.
    let einzeln_log = dir.join("einzeln_audit.json");
    succeeds(&run(&[
        input.to_str().unwrap(),
        "-o",
        dir.join("einzeln.pdf").to_str().unwrap(),
        "--disable-pattern",
        "email,bic",
        "--audit-log",
        einzeln_log.to_str().unwrap(),
    ]));
    let einzeln = audit(&einzeln_log);
    assert_eq!(
        einzeln["patterns"],
        serde_json::json!({ "all_disabled": false, "disabled": ["email", "bic"] }),
        "{einzeln}"
    );
    // Zweimal dieselbe Aussage, einmal für Maschinen und einmal für Menschen.
    assert!(
        einzeln["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("2 Muster abgeschaltet")),
        "{einzeln}"
    );

    // Lauf 3: ganz aus. Dafür braucht es eine Region von Hand, sonst gäbe es
    // nichts zu schwärzen und damit kein Log.
    let regionen = dir.join("regionen.json");
    std::fs::write(
        &regionen,
        r#"[{"page":0,
             "rect":{"ll":{"x":70.0,"y":730.0},"ur":{"x":300.0,"y":745.0}},
             "text":null,
             "source":{"manual":{"reason":"IBAN-Zeile"}}}]"#,
    )
    .unwrap();
    let aus_pdf = dir.join("aus.pdf");
    let aus_log = dir.join("aus_audit.json");
    succeeds(&run(&[
        input.to_str().unwrap(),
        "-o",
        aus_pdf.to_str().unwrap(),
        "--no-patterns",
        "--manual-regions",
        regionen.to_str().unwrap(),
        "--audit-log",
        aus_log.to_str().unwrap(),
    ]));
    let aus = audit(&aus_log);
    assert_eq!(aus["patterns"]["all_disabled"], serde_json::json!(true));
    assert!(
        aus["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("--no-patterns")),
        "{aus}"
    );
    // Die Handregion hat gewirkt — das Log bezeugt eine Schwärzung, und die
    // IBAN ist wirklich weg. Sonst bezeugte es eine Abschaltung an einer Datei,
    // an der ohnehin nichts geschah.
    assert_eq!(aus["effect"]["applied"], serde_json::json!(1), "{aus}");
    assert_gone(&aus_pdf, IBAN);
    // Und was kein Mensch markiert hat, steht noch da.
    assert_still_there(&aus_pdf, EMAIL);

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// 4. Der Tippfehler
// ---------------------------------------------------------------------------

/// **Ein unbekannter Name ist ein Bedienfehler (Rückgabewert 2)** und nennt die
/// gültigen Namen.
///
/// Stillschweigend übergangen sähe `--disable-pattern iban` aus wie eine
/// Abschaltung und wäre keine — die Ausgabe wäre anders als erwartet und
/// niemand erführe warum.
#[test]
fn an_unknown_pattern_name_ends_the_run_with_exit_2() {
    let dir = workdir("tippfehler");
    let input = demo(&dir);
    let out_pdf = dir.join("out.pdf");

    let out = run(&[
        input.to_str().unwrap(),
        "-o",
        out_pdf.to_str().unwrap(),
        "--disable-pattern",
        "iban",
    ]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "Bedienfehler ist 2, nicht {:?}\n{}",
        out.status.code(),
        stderr(&out)
    );
    let message = stderr(&out);
    assert!(message.contains("iban"), "{message}");
    for id in ["iban_de", "bic", "email", "date_de"] {
        assert!(id_is_named(&message, id), "{id} fehlt in: {message}");
    }
    assert!(
        !out_pdf.exists(),
        "bei einem Bedienfehler darf keine Ausgabe entstehen"
    );

    // Und mit dem richtigen Namen läuft derselbe Aufruf durch.
    succeeds(&run(&[
        input.to_str().unwrap(),
        "-o",
        out_pdf.to_str().unwrap(),
        "--disable-pattern",
        "iban_de",
    ]));
    assert_still_there(&out_pdf, IBAN);

    std::fs::remove_dir_all(&dir).ok();
}

fn id_is_named(message: &str, id: &str) -> bool {
    message.contains(id)
}

// ---------------------------------------------------------------------------
// 5. Die Einstellungsdatei
// ---------------------------------------------------------------------------

/// Die Voreinstellung darf in der Datei stehen — und die Kommandozeile muss
/// sie widerrufen können.
///
/// Der zweite Teil ist der wichtige: eine Abschaltung, die sich nicht mehr
/// zurücknehmen lässt, wäre genau die stille falsche Entwarnung, gegen die
/// diese Funktion gebaut ist. Deshalb steht `no_patterns` auch **nicht** in der
/// Datei — das prüft `redact_pipeline::settings`.
#[test]
fn the_settings_file_may_hold_the_default_and_the_command_line_may_revoke_it() {
    let dir = workdir("einstellungen");
    let input = demo(&dir);
    let settings = dir.join("settings.yaml");
    std::fs::write(&settings, "disabled_patterns: [email]\n").unwrap();

    // Aus der Datei: die Adresse bleibt stehen.
    let aus_datei = dir.join("ausDatei.pdf");
    succeeds(&run_with(
        Some(&settings),
        &[input.to_str().unwrap(), "-o", aus_datei.to_str().unwrap()],
    ));
    assert_still_there(&aus_datei, EMAIL);
    assert_gone(&aus_datei, IBAN);

    // Widerrufen: `--disable-pattern` mit einem anderen Namen ersetzt die
    // Liste der Datei, die Adresse wird wieder geschwärzt.
    let widerrufen = dir.join("widerrufen.pdf");
    succeeds(&run_with(
        Some(&settings),
        &[
            input.to_str().unwrap(),
            "-o",
            widerrufen.to_str().unwrap(),
            "--disable-pattern",
            "date_de",
        ],
    ));
    assert_gone(&widerrufen, EMAIL);

    std::fs::remove_dir_all(&dir).ok();
}
