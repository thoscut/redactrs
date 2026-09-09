//! Gegenprüfung g4: die Decke `MAX_CHECK_NEEDLES` steht in README und
//! CHANGELOG an mehr Stellen, als `the_needle_ceiling_is_one_number_in_code_help_and_docs`
//! bindet — der Abschnitt „Nachprüfung nach dem Export“ (README) und die
//! beiden Sätze zur Oberfläche im CHANGELOG. Mutation README-Zeile
//! „höchstens 1 000 Begriffe je Nachprüfung“ → „1 500“: der bestehende Test
//! blieb grün, dieser wird rot.

use redact_core::MAX_CHECK_NEEDLES;

fn mit_tausendertrenner(n: usize) -> String {
    let ziffern = n.to_string();
    let mut aus = String::new();
    for (i, z) in ziffern.chars().enumerate() {
        if i > 0 && (ziffern.len() - i).is_multiple_of(3) {
            aus.push(' ');
        }
        aus.push(z);
    }
    aus
}

fn glatt(name: &str) -> String {
    let wurzel = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(wurzel.join(name))
        .unwrap_or_else(|e| panic!("{name} lesbar: {e}"))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn die_decke_der_nachpruefung_steht_auch_in_der_oberflaechen_doku_als_konstante() {
    let formatiert = mit_tausendertrenner(MAX_CHECK_NEEDLES);
    let readme = glatt("README.md");
    let changelog = glatt("CHANGELOG.md");
    for (datei, text, satz) in [
        (
            "README.md",
            &readme,
            format!("Gesucht werden höchstens {formatiert} Begriffe je Nachprüfung"),
        ),
        (
            "CHANGELOG.md",
            &changelog,
            format!("Gesucht werden höchstens {formatiert} verschiedene Texte"),
        ),
        (
            "CHANGELOG.md",
            &changelog,
            format!("(`redact_core::MAX_CHECK_NEEDLES`, {formatiert})"),
        ),
    ] {
        assert!(text.contains(&satz), "{datei} ohne „{satz}“");
    }
}
