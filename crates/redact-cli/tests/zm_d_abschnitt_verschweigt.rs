//! Gegenprüfung D der Runde 9: **drei Stellen, an denen der Abschnitt
//! „Fix-Runde 8" sich besser liest als der Baum.**
//!
//! Diese Datei ist **absichtlich ROT**. Sie behauptet nichts über ein Leck;
//! sie hält drei Widersprüche fest, die beim Lesen des Abschnitts gegen den
//! Baum auffallen und die niemand sonst hält:
//!
//! 1. Der Export läuft seit dieser Runde **auf einem eigenen Faden**
//!    (`PendingExport`, `EXPORT_RUNNING`, `EXPORT_BUSY`, `EXPORT_BROKEN` —
//!    alle neu in der Runde 8). Der Abschnitt nennt nur die **Fehler** dieses
//!    Umbaus, nicht den Umbau.
//! 2. Derselbe CHANGELOG sagt im Abschnitt der Fix-Runde 3 weiter im Präsens:
//!    „Der Export läuft weiter im Zeichentakt der Oberfläche" und kündigt
//!    `PendingExport` als eigenen Schritt an. Der Schritt ist getan, der Satz
//!    steht unverändert da — beide im selben Abschnitt
//!    `## Unveröffentlicht`, aus dem die Release-Notizen entstehen.
//! 3. Der Abschnitt sagt, die Flächenfrage werde „nicht mit Ecken" gestellt.
//!    `image.rs` sagt an zwei Stellen weiter das Gegenteil über sich selbst
//!    („geprüft an den Ecken jeder Pixelzelle", „prüft die vier Ecken *jeder*
//!    Pixelzelle") — die Doku der Datei beschreibt den Zustand **vor** der
//!    Korrektur.
//!
//! `flock /tmp/redactrs-cargo.lock cargo test -p redact-cli --test zm_d_abschnitt_verschweigt`

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("Wurzel")
        .to_path_buf()
}

fn lies(pfad: &str) -> String {
    std::fs::read_to_string(repo_root().join(pfad))
        .unwrap_or_else(|e| panic!("{pfad} lesbar: {e}"))
        .replace("\r\n", "\n")
}

/// Der ganze Abschnitt `## Unveröffentlicht` — der Schnitt der
/// Release-Notizen (`.github/workflows/release.yml`).
fn unveroeffentlicht() -> String {
    let text = lies("CHANGELOG.md");
    let marke = "\n## Unveröffentlicht";
    let von = text.find(marke).expect("der Abschnitt") + 1;
    let bis = text[von + marke.len()..]
        .find("\n## ")
        .map(|i| von + marke.len() + i)
        .unwrap_or(text.len());
    text[von..bis].to_string()
}

/// Der Abschnitt **einer** Fix-Runde, von seiner Überschrift bis zur nächsten.
fn runde(nummer: u32) -> String {
    let text = lies("CHANGELOG.md");
    let marke = format!("\n### Fix-Runde {nummer}");
    let von = text.find(&marke).expect("die Überschrift der Runde");
    let bis = text[von + marke.len()..]
        .find("\n### ")
        .map(|i| von + marke.len() + i)
        .unwrap_or(text.len());
    text[von..bis].to_string()
}

/// **ROT.** Der Export läuft jetzt auf einem eigenen Faden — der Abschnitt
/// sagt es nicht.
#[test]
fn der_abschnitt_nennt_den_export_im_eigenen_faden() {
    let app = lies("crates/redact-gui/src/app.rs");
    // Vorbedingung: der Umbau steht wirklich im Baum.
    assert!(
        app.contains("struct PendingExport") && app.contains(".name(\"redact-export\""),
        "Vorbedingung: der Export läuft auf einem eigenen Faden"
    );
    assert!(
        app.contains("EXPORT_RUNNING") && app.contains("EXPORT_BROKEN"),
        "Vorbedingung: er hat eigene Meldungen bekommen"
    );

    let block = runde(8);
    let nennt_den_umbau = block.contains("eigenen Faden, während")
        || block.contains("Export läuft jetzt")
        || block.contains("Der Export selbst läuft")
        || block.contains("nicht mehr im Zeichentakt");
    assert!(
        nennt_den_umbau,
        "Der Abschnitt der Runde 8 nennt die drei Fehler des Umbaus, aber nicht \
         den Umbau: dass der Export die Oberfläche nicht mehr anhält, dass ein \
         zweiter Export derselben Datei jetzt abgelehnt wird und dass ein \
         abgestürzter Export „es wurde keine Datei geschrieben“ meldet, steht \
         nirgends in den Release-Notizen."
    );
}

/// **ROT.** Der Satz aus der Fix-Runde 3 steht unverändert da und ist heute
/// falsch.
#[test]
fn die_aussage_ueber_den_zeichentakt_stimmt_nicht_mehr() {
    let app = lies("crates/redact-gui/src/app.rs");
    assert!(
        app.contains(".name(\"redact-export\""),
        "Vorbedingung: der Export hat einen eigenen Faden"
    );
    // Der Satz steht im Abschnitt der **Fix-Runde 3** — und damit im selben
    // `## Unveröffentlicht`, aus dem die Release-Notizen geschnitten werden.
    let block = unveroeffentlicht();
    let steht_noch = block.contains("läuft weiter im Zeichentakt der Oberfläche");
    assert!(
        !steht_noch,
        "Der Abschnitt der Fix-Runde 3 sagt im Präsens „Der Export läuft weiter im \
         Zeichentakt der Oberfläche […] Ein `PendingExport` […] kommt als \
         eigener Schritt“ — der Schritt ist in der Runde 8 getan, der Satz \
         steht unverändert im selben Abschnitt `## Unveröffentlicht`, aus dem \
         die Release-Notizen geschnitten werden."
    );
}

/// **ROT.** „nicht mit Ecken" — die Doku von `image.rs` sagt weiter das
/// Gegenteil über dieselbe Prüfung.
#[test]
fn image_rs_beschreibt_seine_pixelzelle_noch_mit_ecken() {
    let quelle = lies("crates/redact-pdf/src/image.rs");
    // Vorbedingung: die Entscheidung fällt wirklich über trennende Achsen.
    assert!(
        quelle.contains("fn cell_meets_rect") && quelle.contains("cell_meets_rect(&cell, rect)"),
        "Vorbedingung: `Work::covers` fragt die Fläche der Zelle"
    );
    let stellen: Vec<&str> = quelle
        .lines()
        .filter(|z| z.contains("Ecken jeder Pixelzelle") || z.contains("Ecken *jeder* Pixelzelle"))
        .collect();
    assert!(
        stellen.is_empty(),
        "die Doku von `image.rs` beschreibt die Prüfung noch als Eckenprüfung \
         — genau den Zustand, den diese Runde abgelöst hat:\n{}",
        stellen.join("\n")
    );
}
