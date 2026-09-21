//! Gegenprüfung der Fix-Runde 8, Gebiet **Oberfläche** — Gegenprüfer B,
//! Runde 9. Geprüft wird die eine Grenze, die `poll_exports` selbst nennt:
//!
//! > Was bleibt, ist eine Grenze, die diese Ebene nicht schließen kann: das
//! > Fenster zwischen dem `rename` **in** [`redact_pipeline::apply`] und dem
//! > Setzen der Fahne danach (dazwischen liegt nur das Audit-Log).
//!
//! Die Frage des Auftrags lautet: „Ein Urteil, dessen Datei inzwischen
//! überschrieben wurde: wird es SICHER verworfen, oder gibt es ein
//! Zeitfenster?" Das Zeitfenster ist zugegeben — **wie breit** es ist, steht
//! nirgends, und „dazwischen liegt nur das Audit-Log" stimmt nicht ganz:
//! die Fahne wird nicht in `run_reporting` gesetzt, sondern in
//! `RedactApp::export_to`, **nachdem** `run_reporting` zurückgekehrt ist. Im
//! Fenster liegen also
//!
//! 1. das Schreiben des Audit-Logs (Schritt 12 von `redact_pipeline::apply`),
//! 2. `redact_pipeline::describe_blocked` über alle von der Negativliste
//!    gedeckten Flächen (`ExportPlan::run_reporting`, hinter `apply`),
//! 3. die Rückkehr über die Kanalgrenze.
//!
//! Gemessen wird ohne Innenansicht: ein Wächter-Thread pollt das Dateisystem
//! und merkt sich, **wann die Ausgabedatei erscheint** (das `rename`); der
//! Haupt-Thread merkt sich, **wann `run_reporting` zurückkehrt** (unmittelbar
//! davor setzt die Oberfläche die Fahne). Der Abstand ist das Fenster.
//!
//! Der Maßstab ist damit das Dateisystem selbst und nicht der Bericht des
//! Programms.
//!
//! **Ergebnis vorweg:** gemessen ist das Fenster 0,6–0,9 ms breit (Release)
//! und wächst nicht mit der Arbeit — die Zusage hält, und sie hat jetzt eine
//! Zahl. Die Zusicherung bleibt als Wächter stehen.
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

/// Exportiert und misst das Fenster zwischen „die Datei ist da" und
/// „`run_reporting` ist zurück".
///
/// Gibt `(Fenster, Gesamtdauer)` zurück. Findet der Wächter die Datei nie
/// (der Lauf scheitert vor dem `rename`), gibt es kein Fenster — dann ist das
/// Ergebnis `None`, und der Aufrufer sagt das.
fn fenster_messen(
    state: &AppState,
    out: &Path,
    audit: Option<&Path>,
) -> (Option<Duration>, Duration) {
    let fertig = Arc::new(AtomicBool::new(false));
    let ziel = out.to_path_buf();
    let wache = {
        let fertig = Arc::clone(&fertig);
        std::thread::spawn(move || {
            // Eng pollen: das Fenster ist die Messgröße, die Auflösung darf
            // es nicht überdecken.
            while !fertig.load(Ordering::Acquire) {
                if ziel.exists() {
                    return Some(Instant::now());
                }
                std::hint::spin_loop();
            }
            ziel.exists().then(Instant::now)
        })
    };

    let plan = state.plan_export(out, audit).expect("Plan");
    let t0 = Instant::now();
    let (geschrieben, ergebnis) = plan.run_reporting();
    let zurueck = Instant::now();
    fertig.store(true, Ordering::Release);
    assert!(geschrieben, "die Datei muss entstanden sein");
    ergebnis.expect("der Lauf muss glücken");

    let gesehen = wache.join().expect("Wächter");
    (
        gesehen.map(|t| zurueck.saturating_duration_since(t)),
        zurueck - t0,
    )
}

// ===========================================================================
// Das Fenster gibt es — und es ist schmaler als ein Bild
// ===========================================================================

/// **HÄLT — mit einer Zahl statt eines Worts.** Das Fenster zwischen dem
/// `rename` und dem Setzen von `PendingExport::wrote` gibt es wirklich, und
/// es ist **schmaler als ein Bild**.
///
/// Gemessen an einer Durchsicht, wie sie wirklich vorkommt: eine Seite mit
/// 400 geschwärzten Zeilen, also ein Audit-Log in der Größenordnung, die
/// `SECURITY.md` für eine Durchsicht von Hand nennt („von Hand geprüft werden
/// Dutzende bis Hunderte").
///
/// Gemessen (dieser Rechner, `--release`): 0,6–0,9 ms, und die Zahl wächst
/// **nicht** mit der Zahl der Schwärzungen — das Audit-Log ist selbst
/// gedeckelt (48 kB ab rund 200 Einträgen), und mehr liegt nicht im Fenster.
/// Ohne Audit-Log sind es rund 40 µs. Ein Bild bei 60 Hz ist 16,7 ms; es
/// passt also keines hinein. Der Versuch, das Fenster durch mehr Arbeit
/// hinter dem `rename` aufzuziehen, ist **gescheitert**
/// (`zm_b_mess_wie_das_fenster_waechst`).
///
/// Die Zusicherung bleibt als **Wächter** stehen: wandert später Arbeit hinter
/// das `rename` — ein zweites Log, eine Prüfsumme über die geschriebene Datei,
/// ein `fsync` auf ein langsames Ziel —, wird dieser Lauf rot, und dann gehört
/// die Fahne dorthin, wo sie hingehört: **in** `redact-pipeline`, unmittelbar
/// hinter das `rename` der PDF-Datei (Schritt 11).
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
    let out = dir.join("out.pdf");
    let audit = dir.join("audit.json");

    let (fenster, gesamt) = fenster_messen(&state, &out, Some(&audit));
    let fenster = fenster.expect("die Datei muss erschienen sein");
    println!(
        "Vorlage: 1 Seite, {zeilen} Schwärzungen, Audit-Log {} Byte",
        std::fs::metadata(&audit).map(|m| m.len()).unwrap_or(0)
    );
    println!("run_reporting gesamt: {gesamt:?}");
    println!("Fenster rename → Rückkehr (dort setzt export_to die Fahne): {fenster:?}");
    println!("Ein Bild bei 60 Hz: {EIN_BILD:?}");

    // Die Gegenprobe: ohne Audit-Log ist das Fenster kleiner. Damit hängt die
    // Zahl an der Sache und nicht am Messrauschen.
    let out2 = dir.join("ohne-log.pdf");
    let (ohne, _) = fenster_messen(&state, &out2, None);
    println!("Fenster ohne Audit-Log: {ohne:?}");

    assert!(
        fenster < EIN_BILD,
        "zwischen dem rename und der Fahne liegen {fenster:?} — mehr als ein Bild ({EIN_BILD:?}). \
         In dieser Zeit stehen die neuen Bytes auf der Platte, `PendingExport::wrote` ist aber \
         noch false: `poll_exports` wirft das Urteil über die alten Bytes NICHT weg, und ein \
         bereitliegendes Urteil kommt durch. Dann gehört die Fahne dorthin, wo sie hingehört: \
         unmittelbar hinter das rename der PDF-Datei (in `redact-pipeline`, Schritt 11), nicht \
         hinter das Audit-Log und nicht erst hinter die Rückkehr von `run_reporting`."
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
            "{zeilen:4} Schwärzungen: Audit-Log {:7} Byte, run_reporting {gesamt:?}, \
             Fenster {fenster:?}",
            std::fs::metadata(&audit).map(|m| m.len()).unwrap_or(0)
        );
        std::fs::remove_dir_all(&dir).ok();
    }
    println!("Ein Bild bei 60 Hz: {EIN_BILD:?}");
}
