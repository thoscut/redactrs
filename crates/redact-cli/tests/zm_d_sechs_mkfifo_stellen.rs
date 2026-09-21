//! Gegenprüfung D der Runde 9 zum CHANGELOG-Abschnitt **Fix-Runde 8**, Satz:
//!
//! > Sechs Teststellen behaupteten den Ausgang eines Programms, das gar nicht
//! > gelaufen war. […] Jetzt übergeht jede der sechs Stellen den Fall und
//! > schreibt eine Zeile auf die Fehlerausgabe.
//!
//! Hier wird **gezählt** und **jede** der sechs Stellen geprüft — auch die
//! sechste (`crates/redact-gui/src/app.rs`), die in
//! `zf_q5_plattformzusagen::BEHOBENE_MKFIFO` fehlt: jene Liste hält nur fünf.
//! Ein Rückfall ausgerechnet an der sechsten Stelle fiele dort niemandem auf.
//!
//! Der **Lauf** dazu steht nicht im Text, sondern im Bericht: mit einer
//! `mkfifo`-Attrappe im `PATH` (Ausgang 127) laufen alle sechs Tests grün
//! durch und sagen auf `stderr`, was sie deshalb nicht geprüft haben.
//!
//! `flock /tmp/redactrs-cargo.lock cargo test -p redact-cli --test zm_d_sechs_mkfifo_stellen`

use std::path::{Path, PathBuf};

/// Die sechs Dateien, die `mkfifo` starten — Stand `af55c66`.
const SECHS: &[&str] = &[
    "crates/redact-booking/tests/loader_tests.rs",
    "crates/redact-cli/tests/check_leaks.rs",
    "crates/redact-cli/tests/hardening.rs",
    "crates/redact-core/src/read.rs",
    "crates/redact-gui/src/app.rs",
    "crates/redact-patterns/tests/config_limits.rs",
];

fn repo_root() -> PathBuf {
    let hier = Path::new(env!("CARGO_MANIFEST_DIR"));
    hier.parent()
        .and_then(Path::parent)
        .expect("crates/<paket>/..")
        .to_path_buf()
}

fn rust_dateien(dir: &Path, aus: &mut Vec<PathBuf>) {
    let Ok(eintraege) = std::fs::read_dir(dir) else {
        return;
    };
    for eintrag in eintraege.flatten() {
        let pfad = eintrag.path();
        if pfad.is_dir() {
            if pfad.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            rust_dateien(&pfad, aus);
        } else if pfad.extension().is_some_and(|e| e == "rs") {
            aus.push(pfad);
        }
    }
}

/// Eine Zeile, die `mkfifo` wirklich **startet** — kein Kommentar, kein
/// Schnipsel in einer Zeichenkette (dort steht `\"mkfifo\"`).
fn startet_mkfifo(zeile: &str) -> bool {
    let s = zeile.trim_start();
    if s.starts_with("//") || s.starts_with("*") {
        return false;
    }
    s.contains("Command::new(\"mkfifo\")")
}

/// **Es sind genau sechs, und keine mehr.** Wächst die Zahl, ist eine neue
/// Stelle ungeprüft.
#[test]
fn es_sind_genau_sechs_stellen() {
    let wurzel = repo_root();
    let mut dateien = Vec::new();
    rust_dateien(&wurzel.join("crates"), &mut dateien);
    assert!(dateien.len() > 50, "der Baum wurde nicht gefunden");

    let mut gefunden: Vec<String> = Vec::new();
    for datei in &dateien {
        let text = std::fs::read_to_string(datei).expect("lesbar");
        let treffer = text.lines().filter(|z| startet_mkfifo(z)).count();
        if treffer > 0 {
            let relativ = datei
                .strip_prefix(&wurzel)
                .unwrap_or(datei)
                .display()
                .to_string()
                .replace('\\', "/");
            for _ in 0..treffer {
                gefunden.push(relativ.clone());
            }
        }
    }
    gefunden.sort();
    eprintln!("Stellen, die mkfifo starten: {gefunden:#?}");
    assert_eq!(
        gefunden,
        SECHS.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        "genau sechs Stellen, jede genau einmal"
    );
}

/// **Jede der sechs trägt den Ausweg und sagt ihn** — die sechste
/// eingeschlossen.
#[test]
fn jede_der_sechs_uebergeht_den_fall_und_sagt_es() {
    for datei in SECHS {
        let text = std::fs::read_to_string(repo_root().join(datei)).expect("lesbar");
        assert!(
            text.contains("match std::process::Command::new(\"mkfifo\")")
                || text.contains("match Command::new(\"mkfifo\")"),
            "{datei}: kein `match` über dem Startergebnis"
        );
        assert!(
            text.contains("Ok(status) if status.success() => None"),
            "{datei}: der Ausgang wird nicht als möglicher Fehlschlag behandelt"
        );
        assert!(
            text.contains("mkfifo endete mit"),
            "{datei}: ein Ausgang außer null wird nicht als Ausweg behandelt"
        );
        assert!(
            text.contains("kein mkfifo im Pfad"),
            "{datei}: der Startfehler wird nicht benannt"
        );
        assert!(
            text.contains("keine benannte Pipe angelegt"),
            "{datei}: der Übersprung sagt nicht, was ungeprüft bleibt"
        );
        assert!(
            !text.contains(".expect(\"mkfifo startbar\")"),
            "{datei}: Rückfall auf die Behauptung"
        );
        // Und hinter dem Ausweg keine Behauptung über den Ausgang.
        let zeilen: Vec<&str> = text.lines().collect();
        for (i, zeile) in zeilen.iter().enumerate() {
            if !startet_mkfifo(zeile) {
                continue;
            }
            for (nr, spaeter) in zeilen[i..(i + 12).min(zeilen.len())].iter().enumerate() {
                assert!(
                    !(spaeter.contains("assert") && spaeter.contains("success")),
                    "{datei}:{}: behauptet doch wieder den Ausgang: {}",
                    i + nr + 1,
                    spaeter.trim()
                );
            }
        }
    }
}
