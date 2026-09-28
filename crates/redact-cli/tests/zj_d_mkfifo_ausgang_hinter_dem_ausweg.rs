//! Gegenprüfung der Nachbesserung von Agent D (Runde 8), Einwand 1:
//! **der Textwächter, der die fünf behobenen `mkfifo`-Stellen tragen soll,
//! sieht genau die Form nicht, in der rustfmt den Rückfall schreibt.**
//!
//! Agent D hat den Ausweg an fünf Stellen richtig zugeschnitten — jeder
//! Ausgang außer null wird übersprungen, nicht nur `Err`. Gemessen: die
//! Stelle in `crates/redact-core/src/read.rs` endet mit einem `mkfifo`, das
//! mit 127 antwortet, grün und sagt den Grund.
//!
//! Getragen wird das für vier der fünf Stellen aber allein von einem
//! Textwächter in
//! `zf_q5_plattformzusagen::die_fuenf_behobenen_mkfifo_stellen_haben_ihren_ausweg_und_sagen_ihn`
//! (nur die fünfte fährt zusätzlich wirklich, über
//! `check_leaks::eine_mkfifo_attrappe_mit_ausgang_127_bricht_den_lauf_nicht`).
//! Dieser Wächter fordert, dass in den zwölf Zeilen nach dem Start von
//! `mkfifo` keine Zeile **gleichzeitig** `assert` und `success` enthält:
//!
//! ```text
//! !(spaeter.contains("assert") && spaeter.contains("success"))
//! ```
//!
//! Genau diese Bedingung ist blind gegen die Schreibweise, die rustfmt
//! erzwingt, sobald die Meldung eines `assert!` die Zeile zu lang macht:
//!
//! ```text
//!         assert!(                     <- "assert", kein "success"
//!             ok.success(),            <- "success", kein "assert"
//!             "mkfifo ist fehlgeschlagen — …"
//!         );
//! ```
//!
//! Nachgewiesen (Mutation in `crates/redact-core/src/read.rs`, unter einem
//! `flock` angewendet, gefahren und zurückgenommen; md5 vor == nach):
//!
//! * `rustfmt --edition 2021 --check` auf der mutierten Datei: **sauber** —
//!   die Form ist nicht gestellt, sondern die von rustfmt gewollte.
//! * `die_fuenf_behobenen_mkfifo_stellen_haben_ihren_ausweg_und_sagen_ihn`:
//!   **grün**, obwohl der Fehler wieder im Baum steht.
//! * dieselbe Testdatei mit einer `mkfifo`-Attrappe im `PATH`, die mit 127
//!   endet: `exit status: 101` statt eines grünen Übersprungs — also genau
//!   der Anlassfall des Einwands, wieder offen.
//! * Dabei lag die Zeile `ok.success(),` neun Zeilen hinter dem Start von
//!   `mkfifo`, also **innerhalb** des Fensters von zwölf. Die Fensterlänge ist
//!   nicht die Ursache; die Forderung „beides in **einer** Zeile“ ist es.
//!
//! Der Wächter hier fordert dasselbe, aber ohne diese Blindstelle: **hinter
//! dem `match`, das den Ausweg trägt, darf der Ausgang des Werkzeugs im
//! ganzen Rumpf der Funktion nicht mehr vorkommen.** Der Ausweg selbst
//! (`Ok(status) if status.success() => None`) steht *im* `match` und bleibt
//! deshalb erlaubt.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Die fünf Stellen, die Agent D behoben hat.
const BEHOBENE: &[&str] = &[
    "crates/redact-booking/tests/loader_tests.rs",
    "crates/redact-cli/tests/check_leaks.rs",
    "crates/redact-cli/tests/hardening.rs",
    "crates/redact-core/src/read.rs",
    "crates/redact-patterns/tests/config_limits.rs",
];

/// Die sechste Stelle. Sie gehört der Oberfläche und ist als **Vertrag**
/// gemeldet, nicht von dieser Prüfung geändert.
const SECHSTE: &str = "crates/redact-gui/src/app.rs";

/// Startet die Zeile das fremde Werkzeug?
fn startet_mkfifo(zeile: &str) -> bool {
    zeile.contains("Command::new") && zeile.contains("mkfifo")
}

/// Die Einrückung einer Zeile in Leerzeichen.
fn einzug(zeile: &str) -> usize {
    zeile.len() - zeile.trim_start().len()
}

/// Das Ende des Rumpfes der Funktion, in der Zeile `i` steht.
///
/// Gesucht wird über die **Einrückung**: die schließende Klammer eines
/// `fn`-Rumpfes steht genau so weit eingerückt wie das `fn` selbst. Das trägt
/// sowohl eine Funktion auf oberster Ebene als auch eine in `mod tests`.
fn rumpfende(zeilen: &[&str], i: usize) -> usize {
    let kopf = (0..=i)
        .rev()
        .find(|&k| zeilen[k].trim_start().starts_with("fn "));
    let Some(kopf) = kopf else {
        return zeilen.len();
    };
    let tiefe = einzug(zeilen[kopf]);
    let zu = format!("{}}}", " ".repeat(tiefe));
    ((kopf + 1)..zeilen.len())
        .find(|&k| zeilen[k].trim_end() == zu)
        .unwrap_or(zeilen.len())
}

/// Behauptet die Datei hinter dem Ausweg etwas über den Ausgang von `mkfifo`?
///
/// Zurück kommt die Zeilennummer (1-basiert) samt Text der ersten solchen
/// Stelle.
fn behauptung_hinter_dem_ausweg(text: &str) -> Vec<(usize, String)> {
    let zeilen: Vec<&str> = text.lines().collect();
    let mut gefunden = Vec::new();
    for (i, zeile) in zeilen.iter().enumerate() {
        if !startet_mkfifo(zeile) {
            continue;
        }
        let ende = rumpfende(&zeilen, i);
        // Das `match` trägt den Ausweg; sein Abschluss ist das erste `};`
        // hinter dem Start. Erst dahinter wird geprüft.
        let nach_dem_match = ((i + 1)..ende).find(|&k| zeilen[k].trim_end().ends_with("};"));
        let Some(nach_dem_match) = nach_dem_match else {
            gefunden.push((
                i + 1,
                "das Startergebnis geht nicht über ein `match`".to_string(),
            ));
            continue;
        };
        for (k, zeile) in zeilen
            .iter()
            .enumerate()
            .take(ende)
            .skip(nach_dem_match + 1)
        {
            if zeile.contains("success") {
                gefunden.push((k + 1, zeile.trim().to_string()));
            }
        }
    }
    gefunden
}

/// **Die Auflage.** Hinter dem Ausweg steht an den fünf behobenen Stellen
/// keine Behauptung über den Ausgang von `mkfifo` — in **keiner**
/// Schreibweise, auch nicht über mehrere Zeilen verteilt.
///
/// Heute grün. Rot wird er unter der Mutation, die der Wächter von Agent D
/// grün ließ (mehrzeiliges `assert!` auf `ok.success()`); das ist oben
/// gemessen.
#[test]
fn der_ausgang_von_mkfifo_wird_hinter_dem_ausweg_nicht_behauptet() {
    let mut schlecht: Vec<String> = Vec::new();
    for datei in BEHOBENE {
        let pfad = repo_root().join(datei);
        let text = std::fs::read_to_string(&pfad).unwrap_or_else(|e| panic!("{datei} lesbar: {e}"));
        assert!(
            text.contains("mkfifo"),
            "{datei} startet kein `mkfifo` mehr — dann gehört die Zeile hier weg"
        );
        for (nr, zeile) in behauptung_hinter_dem_ausweg(&text) {
            schlecht.push(format!("{datei}:{nr}  {zeile}"));
        }
    }
    assert!(
        schlecht.is_empty(),
        "{} Stelle(n) behaupten hinter dem Ausweg etwas über den Ausgang von \
         `mkfifo` — ein Ausgang außer null heißt: es ist keine Pipe entstanden, \
         also ist nichts zu prüfen:\n{}",
        schlecht.len(),
        schlecht.join("\n")
    );
}

/// **Vertrag, absichtlich rot:** die sechste Stelle hält dasselbe nicht.
///
/// `crates/redact-gui/src/app.rs` trägt einen Ausweg nur für „`mkfifo` lässt
/// sich nicht **starten**“ und dahinter weiter
/// `assert!(ok.success(), "mkfifo ist fehlgeschlagen")`. Ihre eigene Begründung
/// im Quelltext sagt, ein vorhandenes `mkfifo`, das scheitert, sei ein Fehler,
/// „dann gibt es die Pipe“ — das ist genau die Begründung, die Agent D an den
/// anderen fünf Stellen als sachlich falsch gestrichen hat: scheitert `mkfifo`,
/// gibt es **keine** Pipe.
///
/// Weder [`ALTLASTEN`](../zf_q5_plattformzusagen) noch `BEHOBENE_MKFIFO`
/// decken das: die Plattformregel prüft nur, **dass** ein Ausweg da ist, nicht
/// was danach mit ihm geschieht, und der Textwächter läuft nur über die fünf.
/// Die Datei gehört der Oberfläche; hier steht der Fall, damit er nicht
/// verschwindet.
#[test]
fn auch_die_sechste_stelle_darf_den_ausgang_nicht_behaupten() {
    let text = std::fs::read_to_string(repo_root().join(SECHSTE))
        .unwrap_or_else(|e| panic!("{SECHSTE} lesbar: {e}"));
    let gefunden = behauptung_hinter_dem_ausweg(&text);
    assert!(
        gefunden.is_empty(),
        "{SECHSTE} behaupten hinter dem Ausweg etwas über den Ausgang von \
         `mkfifo` — auf einem BusyBox-Bild (`Ok(exit status: 127)`) platzt der \
         Lauf dort weiter, obwohl keine Pipe entstanden ist:\n{}",
        gefunden
            .iter()
            .map(|(nr, z)| format!("{SECHSTE}:{nr}  {z}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
