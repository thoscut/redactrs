//! Die Probenliste (`PRUEFLISTE.md`) hängt an den Dateien, die sie nennt.
//!
//! Die Liste ist der Gegenstand, an dem die Fix-Runden enden: jede Zeile
//! grün und mutationsfest, zwei Runden hintereinander. Sie nennt je Klasse
//! die Belegdateien — und eine Belegdatei, die es nicht mehr gibt, macht die
//! Zeile zu Prosa. Dieser Test liest die Tabelle und verlangt, dass jede
//! genannte Datei existiert und jede Zeile mindestens eine nennt.
//!
//! Mutationsnachweis: eine Belegdatei in der Tabelle umbenennen →
//! [`jede_belegdatei_der_probenliste_existiert`] rot; die Erkennung selbst
//! steht in [`eine_fehlende_datei_wird_erkannt`] an einer Zeile, die es nur
//! hier gibt.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("Wurzel")
        .to_path_buf()
}

/// Die Belegdateien einer Tabellenzeile: jedes `…`-Stück der letzten Spalte,
/// das wie ein Pfad im Baum aussieht.
fn belege_der_zeile(zeile: &str) -> Vec<String> {
    let letzte = zeile.trim_end_matches('|').rsplit('|').next().unwrap_or("");
    letzte
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|t| t.starts_with("crates/") || t.starts_with("scripts/"))
        .map(str::to_string)
        .collect()
}

/// Die Datenzeilen der Tabelle: nach der Kopfzeile und der Trennzeile.
fn zeilen(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|l| l.starts_with('|'))
        .skip(2)
        .collect()
}

fn fehlende(wurzel: &Path, text: &str) -> Vec<String> {
    let mut aus = Vec::new();
    for zeile in zeilen(text) {
        let belege = belege_der_zeile(zeile);
        if belege.is_empty() {
            aus.push(format!("ohne Belegdatei: {}", zeile.trim()));
        }
        for beleg in belege {
            if !wurzel.join(&beleg).is_file() {
                aus.push(format!("fehlt im Baum: {beleg}"));
            }
        }
    }
    aus
}

#[test]
fn jede_belegdatei_der_probenliste_existiert() {
    let wurzel = repo_root();
    let text = std::fs::read_to_string(wurzel.join("PRUEFLISTE.md")).expect("PRUEFLISTE.md");
    assert!(
        !zeilen(&text).is_empty(),
        "PRUEFLISTE.md hat keine Tabellenzeile — die Liste ist leer oder anders geformt"
    );
    let fehlt = fehlende(&wurzel, &text);
    assert!(
        fehlt.is_empty(),
        "die Probenliste nennt Belege, die es so nicht gibt:\n  {}",
        fehlt.join("\n  ")
    );
}

#[test]
fn eine_fehlende_datei_wird_erkannt() {
    let tabelle = "| Klasse | Spur | erstmals | Beleg |\n|---|---|---|---|\n\
                   | erfunden | A | R0 | `crates/redact-cli/tests/belege.rs`, `crates/nirgendwo/tests/gibt_es_nicht.rs` |\n\
                   | ohne | B | R0 | nur Prosa |\n";
    let fehlt = fehlende(&repo_root(), tabelle);
    assert_eq!(fehlt.len(), 2, "{fehlt:?}");
    assert!(fehlt[0].contains("gibt_es_nicht.rs"), "{fehlt:?}");
    assert!(fehlt[1].starts_with("ohne Belegdatei"), "{fehlt:?}");
}
