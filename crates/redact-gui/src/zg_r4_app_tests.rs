//! Gegenprüfung R4 (Fix-Runde 6), Teil 2: der Prüf-Thread —
//! `start_export_check` beendet ältere Prüfungen derselben Datei, der
//! `CheckGate`, die Warnungsliste über mehrere Ausgabedateien.
//!
//! Kindmodul von `app` wie `zb_tests`: `export_to`, `checks`, `hold_check`
//! und `ui_ctx` sind privat. Eingehängt wird es mit
//! `#[cfg(test)] mod zg_r4_app_tests;` am Ende von `app.rs`.
//!
//! `cargo test -p redact-gui zg_r4_2`
//!
//! Tests mit `#[ignore = "Befund …"]` tragen die **richtige** Erwartung und
//! sind bis zur Korrektur rot; der grüne Test daneben hält fest, was der Code
//! heute wirklich tut.

use super::*;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use redact_core::{Rect, Region, Source};
use redact_pdf::testing::{build_pdf, TextItem};

use crate::state::AnnotatedRegion;

// --------------------------------------------------------------- Hilfsmittel

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "zg-r4-app-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn text_region(page: usize, rect: Rect, text: &str) -> AnnotatedRegion {
    AnnotatedRegion::new(Region::new(
        page,
        rect,
        Some(text.to_string()),
        Source::Manual {
            reason: "R4".into(),
        },
    ))
}

/// Ein Dokument mit zwei Geheimnissen auf Seite 1.
fn zwei_geheimnisse() -> Vec<u8> {
    build_pdf(&[vec![
        TextItem::new(72.0, 700.0, 10.0, "Zeile A GEHEIM-EINS"),
        TextItem::new(72.0, 660.0, 10.0, "Zeile B GEHEIM-ZWEI"),
    ]])
}

/// Ein Rechteck über der Zeile bei `y` — trifft.
fn ueber(y: f64) -> Rect {
    Rect::new(60.0, y - 10.0, 520.0, y + 14.0)
}

/// Ein Rechteck, unter dem nichts liegt — die Zeile gilt als geschwärzt und
/// bleibt trotzdem stehen.
fn daneben() -> Rect {
    Rect::new(430.0, 20.0, 440.0, 40.0)
}

fn app_with(bytes: &[u8]) -> RedactApp {
    let mut app = RedactApp::silent(Config {
        no_patterns: true,
        ..Config::default()
    });
    app.open_bytes_and_analyze(bytes, "zwei.pdf");
    assert!(app.state.is_loaded());
    app
}

/// Holt alle Urteile ab und zählt sie; die Statuszeile nach **jedem** Abholen
/// kommt mit zurück.
fn collect_verdicts(app: &mut RedactApp) -> (usize, Vec<String>) {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut count = 0;
    let mut statuses = Vec::new();
    while app.export_check_running() {
        assert!(
            Instant::now() < deadline,
            "die Nachprüfung kommt nicht zum Ende"
        );
        let got = app.poll_export_checks();
        if got > 0 {
            count += got;
            statuses.push(app.state.status.clone());
        } else {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    (count, statuses)
}

// ===========================================================================
// Die Kennung: `pending.out != out`
// ===========================================================================

/// Die Lage von `zb_q4d6_ein_zweiter_export_derselben_datei_beendet_die_alte_pruefung`,
/// nur heißt die Datei beim zweiten Mal anders — über einen Symlink auf
/// denselben Ordner. Erster Export sauber, zweiter leck.
///
/// Gibt `(app, gate, erster Pfad, zweiter Pfad)` zurück; beide Threads
/// hängen am Haken.
fn zweimal_dieselbe_datei_unter_zwei_namen(
    tag: &str,
) -> (RedactApp, Arc<CheckGate>, PathBuf, PathBuf) {
    let dir = tmp(tag);
    let real = dir.join("ordner");
    std::fs::create_dir_all(&real).unwrap();
    let link = dir.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let first = real.join("out.pdf");
    let second = link.join("out.pdf");

    let gate = Arc::new(CheckGate::default());
    let mut app = app_with(&zwei_geheimnisse());
    app.hold_check = Some(gate.clone());

    // Erster Export: Zeile A, Rechteck trifft — sauber.
    app.state
        .regions
        .push(text_region(0, ueber(700.0), "GEHEIM-EINS"));
    app.export_to(first.clone());
    gate.wait_until_arrived();
    assert_eq!(app.checks.len(), 1);

    // Zweiter Export in **dieselbe Datei** unter dem anderen Namen: dazu
    // Zeile B, Rechteck daneben — die neue Ausgabe leckt.
    app.state
        .regions
        .push(text_region(0, daneben(), "GEHEIM-ZWEI"));
    app.export_to(second.clone());
    assert_eq!(
        std::fs::canonicalize(&first).unwrap(),
        std::fs::canonicalize(&second).unwrap(),
        "beide Pfade sind dieselbe Datei"
    );
    (app, gate, first, second)
}

/// **So verhält es sich heute.** `start_export_check` vergleicht Pfade
/// (`pending.out != out`), nicht Dateien: die ältere Prüfung bleibt am
/// Leben, liest die **neuen** Bytes mit dem **alten** Plan (nur
/// GEHEIM-EINS, und das ist in der neuen Datei geschwärzt) und meldet über
/// den ersten Export eine Entwarnung — genau der Fall, den Befund Q4-6 für
/// gleich geschriebene Pfade abgestellt hat.
#[test]
fn zg_r4_2_dieselbe_datei_unter_anderem_namen_haelt_die_alte_pruefung_am_leben_heute() {
    let (mut app, gate, first, second) = zweimal_dieselbe_datei_unter_zwei_namen("symlink-heute");
    println!("erster Pfad:  {}", first.display());
    println!("zweiter Pfad: {}", second.display());
    println!(
        "laufende Prüfungen nach dem zweiten Export: {}",
        app.checks.len()
    );
    assert_eq!(app.checks.len(), 2, "heute: beide laufen");

    gate.release();
    let (urteile, statuses) = collect_verdicts(&mut app);
    for status in &statuses {
        println!("Statuszeile: {status}");
    }
    println!("Warnungen: {:?}", app.state.warnings);
    assert_eq!(urteile, 2, "heute: zwei Urteile über eine Datei");
    // Die Datei leckt wirklich.
    let bytes = std::fs::read(&first).unwrap();
    assert!(!redact_pdf::leaks(&bytes, "GEHEIM-ZWEI").is_empty());
    assert!(redact_pdf::leaks(&bytes, "GEHEIM-EINS").is_empty());
    // Eines der beiden Urteile ist die Entwarnung über den ersten Export —
    // die Zeile trägt den ersten Pfad und „stehen nicht mehr in der Ausgabe“.
    let entwarnung = statuses.iter().any(|s| {
        s.contains(&first.display().to_string()) && s.contains("stehen nicht mehr in der Ausgabe")
    });
    // Kommen beide Urteile in **einem** Abholen an, zeigt die Statuszeile nur
    // das letzte — dann steht die Entwarnung nicht in `statuses`, aber sie
    // wurde gefällt (zwei Urteile, eine Datei). Beides ist der Befund.
    println!("Entwarnung über den ersten Export sichtbar: {entwarnung}");
}

/// **Befund R4 (die richtige Erwartung):** ein zweiter Export in dieselbe
/// Datei beendet die ältere Prüfung — auch wenn der Pfad anders geschrieben
/// ist. Sonst urteilt die ältere über fremde Bytes (siehe Doc-Kommentar von
/// `start_export_check`: „Beides ist falsch, und die gefährliche Richtung ist
/// die Entwarnung“).
#[test]
#[ignore = "Befund R4: die Kennung einer Prüfung ist der Pfad, nicht die Datei — Symlink/`..` halten die ältere Prüfung am Leben"]
fn zg_r4_2_dieselbe_datei_unter_anderem_namen_muesste_die_alte_pruefung_beenden() {
    let (mut app, gate, _, _) = zweimal_dieselbe_datei_unter_zwei_namen("symlink-soll");
    assert_eq!(
        app.checks.len(),
        1,
        "die ältere Prüfung urteilt über Bytes, die es nicht mehr gibt"
    );
    gate.release();
    let (urteile, _) = collect_verdicts(&mut app);
    assert_eq!(urteile, 1, "ein Export, ein Urteil");
}

/// Dasselbe mit `..` statt Symlink: `ordner/../ordner/out.pdf` ist für den
/// Vergleich ein anderer Pfad (nur `.` wird normalisiert, `..` nicht).
#[test]
fn zg_r4_2_ein_punkt_punkt_im_pfad_haelt_die_alte_pruefung_am_leben_heute() {
    let dir = tmp("dotdot");
    let real = dir.join("ordner");
    std::fs::create_dir_all(&real).unwrap();
    let first = real.join("out.pdf");
    let second = real.join("..").join("ordner").join("out.pdf");
    assert_ne!(first, second);

    let gate = Arc::new(CheckGate::default());
    let mut app = app_with(&zwei_geheimnisse());
    app.hold_check = Some(gate.clone());
    app.state
        .regions
        .push(text_region(0, ueber(700.0), "GEHEIM-EINS"));
    app.export_to(first.clone());
    gate.wait_until_arrived();
    app.export_to(second.clone());
    println!("laufende Prüfungen: {}", app.checks.len());
    assert_eq!(app.checks.len(), 2, "heute: beide laufen");
    gate.release();
    let (urteile, _) = collect_verdicts(&mut app);
    assert_eq!(urteile, 2);
}

/// A, B, A und dreimal A in Millisekunden — mit **gleich** geschriebenen
/// Pfaden hält die Korrektur: nach A, B, A laufen B und das zweite A; nach
/// A, A, A läuft eines. Die fallen gelassenen Threads laufen aus, ihr
/// `send` findet niemanden, und das Neuzeichnen wird trotzdem angefordert.
#[test]
fn zg_r4_2_a_b_a_und_dreimal_a() {
    let dir = tmp("aba");
    let a = dir.join("a.pdf");
    let b = dir.join("b.pdf");
    let ctx = egui::Context::default();
    let gate = Arc::new(CheckGate::default());
    let mut app = app_with(&zwei_geheimnisse());
    app.ui_ctx = Some(ctx.clone());
    app.hold_check = Some(gate.clone());
    app.state
        .regions
        .push(text_region(0, ueber(700.0), "GEHEIM-EINS"));

    app.export_to(a.clone());
    gate.wait_until_arrived();
    app.export_to(b.clone());
    app.export_to(a.clone());
    let outs: Vec<&Path> = app.checks.iter().map(|c| c.out.as_path()).collect();
    println!("nach A, B, A: {outs:?}");
    assert_eq!(outs, vec![b.as_path(), a.as_path()]);

    app.export_to(a.clone());
    app.export_to(a.clone());
    let outs: Vec<&Path> = app.checks.iter().map(|c| c.out.as_path()).collect();
    println!("nach A, B, A, A, A: {outs:?}");
    assert_eq!(outs, vec![b.as_path(), a.as_path()]);
    assert!(!ctx.has_requested_repaint(), "alle hängen noch am Haken");

    gate.release();
    let (urteile, statuses) = collect_verdicts(&mut app);
    assert_eq!(urteile, 2, "B und das letzte A");
    for status in &statuses {
        println!("Statuszeile: {status}");
    }
    assert!(ctx.has_requested_repaint());
    // Die drei fallen gelassenen Threads: laufen aus, ohne Spur — nicht
    // beweisbar außer dadurch, dass nichts hängt und nichts panikt.
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(app.poll_export_checks(), 0);
}

// ===========================================================================
// Die Warnungen hängen am Dateinamen, die Prüfungen am Pfad
// ===========================================================================

/// **So verhält es sich heute.** `x/a.pdf` leckt, die Warnung steht. Dann
/// ein sauberer Export nach `y/a.pdf` — ein anderer Ordner, dieselbe
/// Datei**namen**: `note_export_warnings` streicht alles unter „a.pdf: “,
/// also auch die Leckwarnung über `x/a.pdf`. Die Datei ist unverändert und
/// leckt weiter; ihre Warnung ist weg.
#[test]
fn zg_r4_2_gleicher_name_anderer_ordner_loescht_die_leckwarnung_heute() {
    let dir = tmp("name");
    let x = dir.join("x");
    let y = dir.join("y");
    std::fs::create_dir_all(&x).unwrap();
    std::fs::create_dir_all(&y).unwrap();
    let mut app = app_with(&zwei_geheimnisse());

    // x/a.pdf: Rechteck daneben, die Ausgabe leckt.
    app.state
        .regions
        .push(text_region(0, daneben(), "GEHEIM-EINS"));
    app.export_to(x.join("a.pdf"));
    app.wait_for_export_checks();
    println!("nach x/a.pdf: {:?}", app.state.warnings);
    assert!(
        app.state
            .warnings
            .iter()
            .any(|w| w.starts_with("a.pdf: ") && w.contains("NOCH in der Ausgabe")),
        "{:?}",
        app.state.warnings
    );

    // y/a.pdf: Rechteck trifft, die Ausgabe ist sauber.
    assert!(app.state.set_region_rect(0, ueber(700.0)));
    app.export_to(y.join("a.pdf"));
    println!("direkt nach y/a.pdf: {:?}", app.state.warnings);
    app.wait_for_export_checks();
    println!("nach y/a.pdf: {:?}", app.state.warnings);
    println!("Statuszeile: {}", app.state.status);
    // x/a.pdf leckt nach wie vor.
    let bytes = std::fs::read(x.join("a.pdf")).unwrap();
    assert!(!redact_pdf::leaks(&bytes, "GEHEIM-EINS").is_empty());
    assert!(
        !app.state
            .warnings
            .iter()
            .any(|w| w.contains("NOCH in der Ausgabe")),
        "heute: die Warnung über x/a.pdf ist weg — {:?}",
        app.state.warnings
    );
}

/// **Befund R4 (die richtige Erwartung):** die Warnung über eine Datei
/// verschwindet nur, wenn **diese** Datei neu geschrieben wird oder das
/// Dokument wechselt — nicht, weil eine andere Datei gleichen Namens in
/// einem anderen Ordner geschrieben wurde.
#[test]
#[ignore = "Befund R4: Warnungen werden am Dateinamen gehalten, Prüfungen am Pfad — ein gleichnamiger Export in einen anderen Ordner löscht die Leckwarnung"]
fn zg_r4_2_gleicher_name_anderer_ordner_muesste_die_leckwarnung_behalten() {
    let dir = tmp("name-soll");
    let x = dir.join("x");
    let y = dir.join("y");
    std::fs::create_dir_all(&x).unwrap();
    std::fs::create_dir_all(&y).unwrap();
    let mut app = app_with(&zwei_geheimnisse());
    app.state
        .regions
        .push(text_region(0, daneben(), "GEHEIM-EINS"));
    app.export_to(x.join("a.pdf"));
    app.wait_for_export_checks();
    assert!(app.state.set_region_rect(0, ueber(700.0)));
    app.export_to(y.join("a.pdf"));
    app.wait_for_export_checks();
    assert!(
        app.state
            .warnings
            .iter()
            .any(|w| w.contains("NOCH in der Ausgabe")),
        "x/a.pdf leckt weiter, die Warnung ist weg: {:?}",
        app.state.warnings
    );
}

// ===========================================================================
// Die Warnung „sagt nichts“ kommt, geht und kommt wieder — richtig
// ===========================================================================

/// Der Weg der Warnung über nicht gesuchte Texte durch mehrere Exporte:
/// sie steht nach dem Export mit abgewählter Zeile, verschwindet mit dem
/// nächsten Export derselben Datei ohne sie, kommt mit ihr wieder — und
/// steht dann **einmal**, nicht zweimal. Beim Dokumentwechsel ist sie weg.
#[test]
fn zg_r4_1_die_warnung_ueber_nicht_gesuchte_texte_kommt_und_geht_mit_dem_export() {
    let dir = tmp("kommt-geht");
    let out = dir.join("out.pdf");
    let doc = build_pdf(&[vec![
        TextItem::new(72.0, 700.0, 10.0, "Betrag"),
        TextItem::new(72.0, 660.0, 10.0, "Betrag"),
    ]]);
    let mut app = app_with(&doc);
    app.state
        .regions
        .push(text_region(0, ueber(700.0), "Betrag"));
    app.state
        .regions
        .push(text_region(0, ueber(660.0), "Betrag"));
    assert!(app.state.set_enabled(1, false));
    let sagt_nichts = |app: &RedactApp| {
        app.state
            .warnings
            .iter()
            .filter(|w| w.starts_with("out.pdf: ") && w.contains("sagt sie nichts"))
            .count()
    };

    app.export_to(out.clone());
    app.wait_for_export_checks();
    println!("1. Export (abgewählt): {:?}", app.state.warnings);
    assert_eq!(sagt_nichts(&app), 1);

    assert!(app.state.set_enabled(1, true));
    app.export_to(out.clone());
    app.wait_for_export_checks();
    println!("2. Export (beide geschwärzt): {:?}", app.state.warnings);
    assert_eq!(
        sagt_nichts(&app),
        0,
        "die Warnung gehört zum vorigen Export"
    );

    assert!(app.state.set_enabled(1, false));
    app.export_to(out.clone());
    app.wait_for_export_checks();
    app.export_to(out.clone());
    app.wait_for_export_checks();
    println!("3.+4. Export (abgewählt): {:?}", app.state.warnings);
    assert_eq!(sagt_nichts(&app), 1, "einmal, nicht zweimal");

    app.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "demo.pdf");
    println!("Dokumentwechsel: {:?}", app.state.warnings);
    assert_eq!(sagt_nichts(&app), 0);
}

// ===========================================================================
// Der Haken hält
// ===========================================================================

/// Der [`CheckGate`] hält den Thread wirklich an — und nicht nur „meldet
/// ihn an“. Beide Tests aus Fix-Runde 6 (`zb_q4d5`, `zb_q4d6`) bleiben
/// grün, wenn `arrive_and_wait` sofort zurückkehrt (Mutation
/// `state.0 = true; state.1 = true;`): dann ist die Prüfung längst fertig,
/// wenn der Test nachsieht, und niemand merkt, dass der Haken nichts hält.
///
/// Mutation (`arrive_and_wait` kehrt sofort zurück): rot.
#[test]
fn zg_r4_2_der_haken_haelt_den_thread_wirklich() {
    let dir = tmp("haken");
    let out = dir.join("out.pdf");
    let gate = Arc::new(CheckGate::default());
    let mut app = app_with(&zwei_geheimnisse());
    app.hold_check = Some(gate.clone());
    app.state
        .regions
        .push(text_region(0, ueber(700.0), "GEHEIM-EINS"));
    app.export_to(out.clone());
    gate.wait_until_arrived();
    // Eine Prüfung über zwei Zeilen dauert Millisekunden; 300 davon reichen,
    // damit ein nicht haltender Haken auffällt.
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        app.poll_export_checks(),
        0,
        "der Thread ist am Haken vorbei — er hält nicht"
    );
    assert!(app.export_check_running());
    assert!(
        app.state.status.contains(EXPORT_CHECK_RUNNING),
        "{}",
        app.state.status
    );
    gate.release();
    app.wait_for_export_checks();
    assert!(
        app.state.status.contains("Nachprüfung:"),
        "{}",
        app.state.status
    );
}
