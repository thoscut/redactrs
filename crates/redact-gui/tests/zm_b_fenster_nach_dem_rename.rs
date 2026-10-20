//! Gegenprüfung der Fix-Runde 8, Gebiet **Oberfläche** — Gegenprüfer B,
//! Runde 9, umgebaut mit der Korrektur nach 0.7.1. Geprüft wird die eine
//! Grenze, die `poll_exports` bis dahin selbst nannte:
//!
//! > Was bleibt, ist eine Grenze, die diese Ebene nicht schließen kann: das
//! > Fenster zwischen dem `rename` **in** [`redact_pipeline::apply`] und dem
//! > Setzen der Fahne danach (dazwischen liegt nur das Audit-Log).
//!
//! Die Frage des Auftrags lautet: „Ein Urteil, dessen Datei inzwischen
//! überschrieben wurde: wird es SICHER verworfen, oder gibt es ein
//! Zeitfenster?"
//!
//! **Bis 0.7.1** setzte `RedactApp::export_to` die Fahne, **nachdem**
//! `run_reporting` zurückgekehrt war. Im Fenster lagen
//!
//! 1. das Schreiben des Audit-Logs (Schritt 12 von `redact_pipeline::apply`),
//! 2. `redact_pipeline::describe_blocked` über alle von der Negativliste
//!    gedeckten Flächen (`ExportPlan::run_reporting`, hinter `apply`),
//! 3. die Rückkehr über die Kanalgrenze.
//!
//! Gemessen war das 0,6–0,9 ms (Release) — auf dem geteilten Windows-Läufer
//! aber auch 22,6 ms (CI-Lauf 127), mehr als ein Bild. Der Wächter in diesem
//! Test riss, und beim Release 0.7.1 wurde er übergangen.
//!
//! **Jetzt** setzt `redact_pipeline::apply` die Fahne selbst, unmittelbar
//! hinter dem `rename` der PDF-Datei (Schritt 11), und die Oberfläche reicht
//! sie über `ExportPlan::run_signalling` hinein. Im Fenster liegt nur noch die
//! Rückkehr aus `PdfRenderer::render` — das Audit-Log liegt **dahinter**.
//!
//! Gemessen wird ohne Innenansicht: ein Wächter-Thread pollt das Dateisystem
//! und die Fahne und merkt sich, **wann die Ausgabedatei erscheint** (das
//! `rename`) und **wann die Fahne steht**. Der Abstand ist das Fenster. Der
//! Maßstab ist damit das Dateisystem selbst und nicht der Bericht des
//! Programms.
//!
//! **Warum die Breite zählt:** in diesem Fenster liegen die neuen Bytes schon
//! auf der Platte, `PendingExport::wrote` ist aber noch `false`. Zeichnet die
//! Oberfläche jetzt ein Bild, so wirft `poll_exports` nichts weg, und ein
//! bereitliegendes Urteil einer **älteren** Nachprüfung derselben Datei wird
//! abgeholt und angezeigt — ein Urteil über Bytes, die es nicht mehr gibt, in
//! der gefährlichen Richtung eine Entwarnung. Ein Bild bei 60 Hz ist 16,7 ms;
//! ist das Fenster breiter, passt mindestens eines hinein.
//!
//! ```text
//! flock /tmp/redactrs-cargo.lock cargo test -p redact-gui --test zm_b_fenster_nach_dem_rename -- --nocapture
//! flock /tmp/redactrs-cargo.lock cargo test -p redact-gui --release --test zm_b_fenster_nach_dem_rename -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use redact_core::{Rect, Region, Source};
use redact_gui::state::{AnnotatedRegion, AppState};
use redact_gui::Config;
use redact_pdf::testing::{build_pdf, TextItem};

/// Ein Bild bei 60 Hz.
const EIN_BILD: Duration = Duration::from_micros(16_667);

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zm-b-fenster-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Eine Seite mit `zeilen` Zeilen, jede mit einem eigenen Geheimnis.
fn vorlage(seiten: usize, zeilen: usize) -> Vec<u8> {
    let pages: Vec<Vec<TextItem>> = (0..seiten)
        .map(|p| {
            (0..zeilen)
                .map(|z| {
                    TextItem::new(
                        72.0,
                        740.0 - 8.0 * z as f64,
                        6.0,
                        format!("S{p} Z{z} GEHEIM-{p:03}-{z:03} Buchung Betrag Gegenkonto"),
                    )
                })
                .collect()
        })
        .collect();
    build_pdf(&pages)
}

/// Ein Zustand mit einer Schwärzung je Zeile auf Seite 0 — so wird das
/// Audit-Log groß, und groß ist es bei einer wirklichen Durchsicht auch.
fn geladen(dir: &Path, bytes: &[u8], zeilen: usize) -> AppState {
    let input = dir.join("eingang.pdf");
    std::fs::write(&input, bytes).unwrap();
    let mut state = AppState::with_config(Config {
        no_patterns: true,
        ..Config::default()
    });
    state.load_bytes(bytes, Some(input)).unwrap();
    for z in 0..zeilen {
        let y = 740.0 - 8.0 * z as f64;
        state.regions.push(AnnotatedRegion::new(Region::new(
            0,
            Rect::new(60.0, y - 2.0, 520.0, y + 8.0),
            Some(format!("GEHEIM-000-{z:03}")),
            Source::Manual {
                reason: "zm-b".into(),
            },
        )));
    }
    state
}

/// Exportiert und misst das Fenster zwischen „die Datei ist da" und „die
/// Fahne steht".
///
/// Gibt `(Fenster, Gesamtdauer)` zurück. Findet der Wächter die Datei nie
/// (der Lauf scheitert vor dem `rename`), gibt es kein Fenster — dann ist das
/// Ergebnis `None`, und der Aufrufer sagt das.
fn fenster_messen(
    state: &AppState,
    out: &Path,
    audit: Option<&Path>,
) -> (Option<Duration>, Duration) {
    let fahne = Arc::new(AtomicBool::new(false));
    let fertig = Arc::new(AtomicBool::new(false));
    let ziel = out.to_path_buf();
    let wache = {
        let fahne = Arc::clone(&fahne);
        let fertig = Arc::clone(&fertig);
        std::thread::spawn(move || {
            // Eng pollen: das Fenster ist die Messgröße, die Auflösung darf
            // es nicht überdecken. Ein Thread für beides, damit kein zweiter
            // Wächter dem Export den Kern wegnimmt.
            let mut datei: Option<Instant> = None;
            let mut gesetzt: Option<Instant> = None;
            loop {
                let ende = fertig.load(Ordering::Acquire);
                if datei.is_none() && ziel.exists() {
                    datei = Some(Instant::now());
                }
                if gesetzt.is_none() && fahne.load(Ordering::Acquire) {
                    gesetzt = Some(Instant::now());
                }
                if ende || (datei.is_some() && gesetzt.is_some()) {
                    return (datei, gesetzt);
                }
                std::hint::spin_loop();
            }
        })
    };

    let plan = state.plan_export(out, audit).expect("Plan");
    let t0 = Instant::now();
    let ergebnis = plan.run_signalling(&fahne);
    let zurueck = Instant::now();
    fertig.store(true, Ordering::Release);
    assert!(
        fahne.load(Ordering::Acquire),
        "die Datei muss entstanden sein"
    );
    ergebnis.expect("der Lauf muss glücken");

    let (datei, gesetzt) = wache.join().expect("Wächter");
    let gesetzt = gesetzt.expect("die Fahne stand, also hat der Wächter sie gesehen");
    (
        datei.map(|t| gesetzt.saturating_duration_since(t)),
        zurueck - t0,
    )
}

// ===========================================================================
// Das Fenster gibt es — und es ist schmaler als ein Bild
// ===========================================================================

/// **HÄLT — und jetzt aus dem richtigen Grund.** Das Fenster zwischen dem
/// `rename` und dem Setzen von `PendingExport::wrote` ist **schmaler als ein
/// Bild**, weil die Fahne dort steht, wo dieser Wächter sie hinhaben wollte:
/// **in** `redact-pipeline`, unmittelbar hinter dem `rename` der PDF-Datei
/// (Schritt 11), vor dem Audit-Log.
///
/// Gemessen an einer Durchsicht, wie sie wirklich vorkommt: eine Seite mit
/// 400 geschwärzten Zeilen, also ein Audit-Log in der Größenordnung, die
/// `SECURITY.md` für eine Durchsicht von Hand nennt („von Hand geprüft werden
/// Dutzende bis Hunderte").
///
/// Vorher (Fahne hinter der Rückkehr von `run_reporting`): 0,6–0,9 ms auf
/// einem freien Rechner, 22,6 ms auf dem geteilten Windows-Läufer. Jetzt liegt
/// das Audit-Log nicht mehr im Fenster, und die Gegenprobe ohne Audit-Log
/// ergibt dieselbe Größenordnung — das Fenster hängt nicht mehr an der Arbeit
/// hinter dem `rename`.
///
/// Die Zusicherung bleibt als **Wächter** stehen: wandert Arbeit zwischen das
/// `rename` und die Fahne — die Fahne zurück hinter das Audit-Log, ein
/// `fsync` auf das Verzeichnis vor der Fahne —, wird dieser Lauf rot.
///
/// Anmerkung zur Messung: gemessen wird von **außen** (das Dateisystem sagt,
/// wann die Datei da ist). Das ist eine Obergrenze für die Sichtbarkeit des
/// `rename`, keine Innenansicht — gerade deshalb ist es die Zahl, die zählt:
/// so früh kann ein anderer Leser die neuen Bytes sehen.
#[test]
fn zm_b_zwischen_rename_und_fahne_passt_ein_bild() {
    let dir = tmp("breite");
    let zeilen = 400;
    let bytes = vorlage(1, zeilen);
    let state = geladen(&dir, &bytes, zeilen);
    let audit = dir.join("audit.json");

    // Fünf Messungen, das **Minimum** zählt: gefragt ist, ob das Fenster
    // schmaler als ein Bild sein *kann* — eine Eigenschaft der Stelle, an der
    // die Fahne gesetzt wird —, nicht, ob der geteilte CI-Läufer in jedem
    // einzelnen Lauf frei ist. Ein einzelner Lauf lag dort bei 22,6 ms (Lauf
    // 127, Windows) und riss den Wächter, ohne dass sich am Code etwas
    // geändert hatte.
    let mut fenster = Duration::MAX;
    let mut gesamt = Duration::ZERO;
    for i in 0..5 {
        let out_i = dir.join(format!("out-{i}.pdf"));
        let audit_i = dir.join(format!("audit-{i}.json"));
        let (f, g) = fenster_messen(&state, &out_i, Some(&audit_i));
        let f = f.expect("die Datei muss erschienen sein");
        fenster = fenster.min(f);
        gesamt = g;
        if i == 0 {
            std::fs::copy(&audit_i, &audit).ok();
        }
    }
    println!(
        "Vorlage: 1 Seite, {zeilen} Schwärzungen, Audit-Log {} Byte",
        std::fs::metadata(&audit).map(|m| m.len()).unwrap_or(0)
    );
    println!("run_signalling gesamt: {gesamt:?}");
    println!("Fenster rename → Fahne (gesetzt in redact_pipeline::apply): {fenster:?}");
    println!("Ein Bild bei 60 Hz: {EIN_BILD:?}");

    // Die Gegenprobe: ohne Audit-Log ist das Fenster nicht kleiner — das Log
    // liegt hinter der Fahne, nicht mehr im Fenster.
    let out2 = dir.join("ohne-log.pdf");
    let (ohne, _) = fenster_messen(&state, &out2, None);
    println!("Fenster ohne Audit-Log: {ohne:?}");

    assert!(
        fenster < EIN_BILD,
        "zwischen dem rename und der Fahne liegen {fenster:?} — mehr als ein Bild ({EIN_BILD:?}). \
         In dieser Zeit stehen die neuen Bytes auf der Platte, `PendingExport::wrote` ist aber \
         noch false: `poll_exports` wirft das Urteil über die alten Bytes NICHT weg, und ein \
         bereitliegendes Urteil kommt durch. Die Fahne gehört unmittelbar hinter das rename \
         der PDF-Datei (`redact_pipeline::apply`, Schritt 11) — liegt dort wieder Arbeit \
         dazwischen?"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Messung: wie das Fenster mit der Zahl der Schwärzungen wächst.
#[test]
#[ignore = "Messung: die Breite des Fensters zwischen rename und Fahne"]
fn zm_b_mess_wie_das_fenster_waechst() {
    for zeilen in [1usize, 50, 200, 400, 800] {
        let dir = tmp(&format!("wachstum-{zeilen}"));
        let bytes = vorlage(1, zeilen);
        let state = geladen(&dir, &bytes, zeilen);
        let out = dir.join("out.pdf");
        let audit = dir.join("audit.json");
        let (fenster, gesamt) = fenster_messen(&state, &out, Some(&audit));
        println!(
            "{zeilen:4} Schwärzungen: Audit-Log {:7} Byte, run_signalling {gesamt:?}, \
             Fenster {fenster:?}",
            std::fs::metadata(&audit).map(|m| m.len()).unwrap_or(0)
        );
        std::fs::remove_dir_all(&dir).ok();
    }
    println!("Ein Bild bei 60 Hz: {EIN_BILD:?}");
}
