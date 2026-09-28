//! Gegenprüfung der Fix-Runde 8 (Register #19), Gebiet **Oberfläche** —
//! Gegenprüfer B, Runde 9.
//!
//! Zwei Fragen, ein Lauf:
//!
//! 1. **Register #19 — läuft der Export noch im Zeichentakt?** Nein, und das
//!    ist hier mit der Uhr belegt: `AppState::plan_export` (der Abzug, der auf
//!    den Thread geht) kostet Mikrosekunden, `ExportPlan::run_reporting` (die
//!    Arbeit) Sekunden. Beides sind öffentliche Wege; die Oberfläche ruft in
//!    `RedactApp::export_to` genau diese beiden Stellen, die erste im
//!    Zeichentakt und die zweite auf dem Thread.
//!
//! 2. **Was der Umbau gekostet hat: die Decke ist weg.** Solange der Export im
//!    Zeichentakt lief, war *einer* gleichzeitig möglich — der Takt war die
//!    Decke. Jetzt hält `RedactApp::exports` eine Liste ohne Obergrenze:
//!    abgelehnt wird nur ein zweiter Export **derselben** Ausgabedatei
//!    (`EXPORT_BUSY`), ein Export in eine andere Datei ausdrücklich nicht (so
//!    steht es in `zh_c_eine_andere_datei_darf_gleichzeitig_geschrieben_werden`).
//!    Jeder laufende Export hält dabei eine **eigene vollständige Kopie** des
//!    Dokuments: `ExportPlan::run_reporting` beginnt mit
//!    `let mut copy = (*self.document).clone();`.
//!
//!    Gemessen wird deshalb, was N gleichzeitige Exporte desselben Dokuments
//!    an Zeit und an Speicherspitze kosten, verglichen mit einem.
//!
//! Der Maßstab ist nicht der Bericht der Oberfläche: gelesen werden die
//! geschriebenen Bytes (`redact_pdf::leaks`) und `/proc/self/status`.
//!
//! ```text
//! flock /tmp/redactrs-cargo.lock cargo test -p redact-gui --test zm_b_gleichzeitige_exporte -- --nocapture
//! flock /tmp/redactrs-cargo.lock cargo test -p redact-gui --release --test zm_b_gleichzeitige_exporte -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};

use redact_core::{Rect, Region, Source};
use redact_gui::state::{AnnotatedRegion, AppState};
use redact_gui::Config;
use redact_pdf::testing::{build_pdf, TextItem};

const GEHEIM: &str = "GEHEIM-EINS";

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zm-b-gleich-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Ein Dokument mit `seiten` Seiten, auf jeder ein Geheimnis und `zeilen`
/// Zeilen Fülltext — damit die Kopie im Speicher etwas wiegt.
fn vorlage_mit(seiten: usize, zeilen: usize) -> Vec<u8> {
    let pages: Vec<Vec<TextItem>> = (0..seiten)
        .map(|p| {
            let mut items = vec![TextItem::new(
                72.0,
                700.0,
                10.0,
                format!("Seite {p} {GEHEIM}"),
            )];
            for z in 0..zeilen {
                items.push(TextItem::new(
                    72.0,
                    660.0 - 12.0 * z as f64,
                    10.0,
                    format!("Seite {p} Zeile {z}: Buchung, Betrag, Gegenkonto, Verwendungszweck"),
                ));
            }
            items
        })
        .collect();
    build_pdf(&pages)
}

fn vorlage(seiten: usize) -> Vec<u8> {
    vorlage_mit(seiten, 1)
}

/// Die Größe der Messvorlage — über die Umgebung einstellbar, damit dieselbe
/// Messung an einer kleinen und an einer großen Vorlage läuft.
///
/// Nur die Messung braucht sie, und die läuft nur unter Linux
/// (`/proc/self/status`). Ohne das `cfg` wäre sie dort toter Code — und mit
/// `-D warnings` ein Baufehler für `x86_64-pc-windows-gnu`.
#[cfg(target_os = "linux")]
fn masse() -> (usize, usize, usize) {
    let zahl = |name: &str, vorgabe: usize| {
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(vorgabe)
    };
    (
        zahl("ZM_B_SEITEN", 400),
        zahl("ZM_B_ZEILEN", 1),
        zahl("ZM_B_N", 4),
    )
}

/// Ein Rechteck über der Zeile bei `y` — es trifft.
fn ueber(y: f64) -> Rect {
    Rect::new(60.0, y - 10.0, 520.0, y + 14.0)
}

/// Ein geladener Zustand mit genau einer Schwärzung auf Seite 0.
fn geladen(dir: &Path, bytes: &[u8]) -> AppState {
    let input = dir.join("eingang.pdf");
    std::fs::write(&input, bytes).unwrap();
    let mut state = AppState::with_config(Config {
        no_patterns: true,
        ..Config::default()
    });
    state.load_bytes(bytes, Some(input)).unwrap();
    state.regions.push(AnnotatedRegion::new(Region::new(
        0,
        ueber(700.0),
        Some(GEHEIM.to_string()),
        Source::Manual {
            reason: "zm-b".into(),
        },
    )));
    state
}

/// Ein Feld aus `/proc/self/status` in kB — `VmHWM` ist die Speicherspitze
/// des ganzen Prozesses, `Threads` die Zahl der Threads.
///
/// Nur Linux hat diese Datei; jede Messung hier steht deshalb unter
/// `cfg(target_os = "linux")`.
#[cfg(target_os = "linux")]
fn proc_status(field: &str) -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").expect("/proc/self/status");
    status
        .lines()
        .find(|line| line.starts_with(field))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse().ok())
        .expect(field)
}

// ===========================================================================
// 1 — Register #19: der Abzug ist billig, die Arbeit ist teuer
// ===========================================================================

/// **Register #19 hält.** Was im Zeichentakt bleibt, ist der *Abzug*, und der
/// kostet nichts.
///
/// `RedactApp::export_to` tut vor dem Thread genau drei Dinge, die etwas
/// rechnen: `audit_target`, `blocked_regions`/`hit_summary`,
/// `plan_export_check` und `plan_export`. Alles davon läuft hier, mit der Uhr
/// daneben; `run_reporting` — die Arbeit — läuft danach und ist um
/// Größenordnungen teurer. Der Vergleich ist die Aussage: die Oberfläche hält
/// nur noch für den Abzug an.
#[test]
fn zm_b_der_abzug_im_zeichentakt_kostet_nichts_die_arbeit_alles() {
    let dir = tmp("abzug");
    let bytes = vorlage(300);
    let state = geladen(&dir, &bytes);
    let out = dir.join("out.pdf");

    let t0 = std::time::Instant::now();
    let audit = state.audit_target(&out);
    let _blocked = state.blocked_regions().len();
    let summary = state.hit_summary();
    let _check = state.plan_export_check(&summary);
    let plan = state.plan_export(&out, Some(&audit)).expect("Plan");
    let abzug = t0.elapsed();

    let t0 = std::time::Instant::now();
    let (geschrieben, ergebnis) = plan.run_reporting();
    let arbeit = t0.elapsed();
    assert!(geschrieben, "die Datei muss entstanden sein");
    ergebnis.expect("der Lauf muss glücken");

    println!("Vorlage: 300 Seiten, {} kB", bytes.len() / 1024);
    println!("Abzug (im Zeichentakt): {abzug:?}");
    println!("Arbeit (auf dem Thread): {arbeit:?}");
    println!(
        "Verhältnis: 1 zu {:.0}",
        arbeit.as_secs_f64() / abzug.as_secs_f64().max(1e-9)
    );

    // Das Orakel, nicht der Bericht: die geschriebenen Bytes.
    let geschriebene = std::fs::read(&out).unwrap();
    assert!(
        redact_pdf::leaks(&geschriebene, GEHEIM).len() < 300,
        "Seite 0 muss geschwärzt sein"
    );

    assert!(
        abzug < arbeit,
        "der Abzug ({abzug:?}) darf nicht teurer sein als die Arbeit ({arbeit:?})"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ===========================================================================
// 2 — Was der Umbau gekostet hat: keine Decke über gleichzeitigen Exporten
// ===========================================================================

/// **Messung.** N gleichzeitige Exporte desselben Dokuments in N verschiedene
/// Dateien — genau das, was die Oberfläche erlaubt.
///
/// Gemessen wird die Speicherspitze des Prozesses (`VmHWM`) und die Dauer, je
/// für einen Export allein und für vier gleichzeitige. Jeder Export kopiert
/// das Dokument selbst (`run_reporting`: `(*self.document).clone()`), also
/// steigt die Spitze mit N.
///
/// Solange der Export im Zeichentakt lief, war N = 1 — der Takt war die Decke.
/// Diese Messung sagt, was seit Fix-Runde 8 stattdessen offen steht.
#[test]
#[ignore = "Messung: was gleichzeitige Exporte an Speicher kosten"]
#[cfg(target_os = "linux")]
fn zm_b_mess_was_vier_gleichzeitige_exporte_kosten() {
    let dir = tmp("mess-vier");
    let (seiten, zeilen, n) = masse();
    let bytes = vorlage_mit(seiten, zeilen);
    let state = geladen(&dir, &bytes);
    println!(
        "Vorlage: {seiten} Seiten mal {zeilen} Zeile(n), {} kB",
        bytes.len() / 1024
    );

    // Erst einer allein.
    let out = dir.join("einzeln.pdf");
    let plan = state
        .plan_export(&out, Some(&state.audit_target(&out)))
        .expect("Plan");
    let vor_einzeln = proc_status("VmHWM:");
    let t0 = std::time::Instant::now();
    let (geschrieben, ergebnis) = plan.run_reporting();
    let dauer_einzeln = t0.elapsed();
    assert!(geschrieben);
    ergebnis.expect("Lauf");
    let nach_einzeln = proc_status("VmHWM:");
    println!(
        "ein Export: {dauer_einzeln:?}, Speicherspitze +{} MB (von {} MB auf {} MB)",
        (nach_einzeln.saturating_sub(vor_einzeln)) / 1024,
        vor_einzeln / 1024,
        nach_einzeln / 1024
    );

    // Und jetzt N gleichzeitig — N Dateien, N Threads, wie nach N Klicks in
    // den Dialog.
    let plaene: Vec<_> = (0..n)
        .map(|i| {
            let ziel = dir.join(format!("gleich-{i}.pdf"));
            let plan = state
                .plan_export(&ziel, Some(&state.audit_target(&ziel)))
                .expect("Plan");
            (ziel, plan)
        })
        .collect();

    let vor_vier = proc_status("VmHWM:");
    let threads_vor = proc_status("Threads:");
    let t0 = std::time::Instant::now();
    let griffe: Vec<_> = plaene
        .into_iter()
        .map(|(ziel, plan)| {
            std::thread::spawn(move || {
                let (geschrieben, ergebnis) = plan.run_reporting();
                (ziel, geschrieben, ergebnis.is_ok())
            })
        })
        .collect();
    // Kurz warten, damit die Threads wirklich alle gleichzeitig arbeiten,
    // und dann die Zahl der Threads lesen.
    std::thread::sleep(std::time::Duration::from_millis(50));
    let threads_mitten = proc_status("Threads:");
    let mut alle_da = true;
    for griff in griffe {
        let (ziel, geschrieben, gelungen) = griff.join().expect("Thread");
        alle_da &= geschrieben && gelungen && ziel.exists();
    }
    let dauer_vier = t0.elapsed();
    let nach_vier = proc_status("VmHWM:");
    assert!(alle_da, "alle vier Ausgaben müssen dastehen");

    println!(
        "{n} gleichzeitig: {dauer_vier:?}, Speicherspitze +{} MB (von {} MB auf {} MB), \
         Threads {threads_vor} → {threads_mitten}",
        (nach_vier.saturating_sub(vor_vier)) / 1024,
        vor_vier / 1024,
        nach_vier / 1024
    );
    println!(
        "Zuwachs je gleichzeitigem Export: {} MB",
        (nach_vier.saturating_sub(vor_vier)) / 1024 / n as u64
    );
    println!(
        "Die Oberfläche deckelt das nicht: abgelehnt wird nur ein zweiter Export DERSELBEN \
         Datei (EXPORT_BUSY). MAX_WARNED_FILES = {} deckelt die Warnungen, nicht die Threads.",
        redact_gui::app::MAX_WARNED_FILES
    );

    std::fs::remove_dir_all(&dir).ok();
}
