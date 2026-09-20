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
//! Die Befundtests der Gegenprüfung sind seit Fix-Runde 7 scharf (kein
//! `#[ignore]` mehr); die Tests daneben halten die Gegenrichtung fest — was
//! **nicht** passieren darf, wenn die Kennung einer Prüfung die Datei ist und
//! nicht die Schreibweise ihres Pfades.

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
/// Braucht einen Symlink und steht deshalb unter `cfg`: unter Windows
/// verlangt `symlink` Sonderrechte, und ein Test, der ohne sie panickt,
/// bricht den dortigen Lauf.
#[cfg(unix)]
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
    app.wait_for_export();
    gate.wait_until_arrived();
    assert_eq!(app.checks.len(), 1);

    // Zweiter Export in **dieselbe Datei** unter dem anderen Namen: dazu
    // Zeile B, Rechteck daneben — die neue Ausgabe leckt.
    app.state
        .regions
        .push(text_region(0, daneben(), "GEHEIM-ZWEI"));
    app.export_to(second.clone());
    app.wait_for_export();
    assert_eq!(
        std::fs::canonicalize(&first).unwrap(),
        std::fs::canonicalize(&second).unwrap(),
        "beide Pfade sind dieselbe Datei"
    );
    (app, gate, first, second)
}

/// **Befund R4-4 (die richtige Erwartung, jetzt erfüllt):** ein zweiter
/// Export in dieselbe Datei beendet die ältere Prüfung — auch wenn der Pfad
/// anders geschrieben ist. Sonst urteilt die ältere über fremde Bytes (siehe
/// Doc-Kommentar von `start_export_check`: „Beides ist falsch, und die
/// gefährliche Richtung ist die Entwarnung“).
///
/// Bis Fix-Runde 6 verglich `start_export_check` Pfade (`pending.out !=
/// out`), nicht Dateien: die ältere Prüfung blieb am Leben, las die **neuen**
/// Bytes mit dem **alten** Plan (nur GEHEIM-EINS, und das ist in der neuen
/// Datei geschwärzt) und meldete über den ersten Export eine Entwarnung.
#[test]
#[cfg(unix)]
fn zg_r4_2_dieselbe_datei_unter_anderem_namen_beendet_die_alte_pruefung() {
    let (mut app, gate, first, second) = zweimal_dieselbe_datei_unter_zwei_namen("symlink-soll");
    println!("erster Pfad:  {}", first.display());
    println!("zweiter Pfad: {}", second.display());
    println!(
        "laufende Prüfungen nach dem zweiten Export: {}",
        app.checks.len()
    );
    assert_eq!(
        app.checks.len(),
        1,
        "die ältere Prüfung urteilt über Bytes, die es nicht mehr gibt"
    );
    gate.release();
    let (urteile, statuses) = collect_verdicts(&mut app);
    for status in &statuses {
        println!("Statuszeile: {status}");
    }
    println!("Warnungen: {:?}", app.state.warnings);
    assert_eq!(urteile, 1, "ein Export, ein Urteil");
    // Die Datei leckt wirklich — und genau das sagt das eine Urteil.
    let bytes = std::fs::read(&first).unwrap();
    assert!(!redact_pdf::leaks(&bytes, "GEHEIM-ZWEI").is_empty());
    assert!(redact_pdf::leaks(&bytes, "GEHEIM-EINS").is_empty());
    assert!(
        !statuses
            .iter()
            .any(|s| s.contains("stehen nicht mehr in der Ausgabe")),
        "eine Entwarnung über Bytes, die es nicht mehr gibt: {statuses:?}"
    );
    assert!(
        app.state
            .warnings
            .iter()
            .any(|w| w.contains("NOCH in der Ausgabe")),
        "{:?}",
        app.state.warnings
    );
}

/// Dasselbe mit `..` statt Symlink: `ordner/../ordner/out.pdf` ist für einen
/// Vergleich Zeichen für Zeichen ein anderer Pfad (nur `.` wird
/// normalisiert, `..` nicht) — und dieselbe Datei.
#[test]
fn zg_r4_2_ein_punkt_punkt_im_pfad_beendet_die_alte_pruefung() {
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
    app.wait_for_export();
    gate.wait_until_arrived();
    app.export_to(second.clone());
    app.wait_for_export();
    println!("laufende Prüfungen: {}", app.checks.len());
    assert_eq!(app.checks.len(), 1);
    gate.release();
    let (urteile, _) = collect_verdicts(&mut app);
    assert_eq!(urteile, 1);

    // **Die Gegenrichtung**: eine wirklich andere Datei im selben Ordner
    // beendet nichts.
    let gate = Arc::new(CheckGate::default());
    app.hold_check = Some(gate.clone());
    app.export_to(first.clone());
    app.wait_for_export();
    gate.wait_until_arrived();
    app.export_to(real.join("zweite.pdf"));
    app.wait_for_export();
    println!("zwei Dateien: {}", app.checks.len());
    assert_eq!(app.checks.len(), 2);
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
    app.wait_for_export();
    gate.wait_until_arrived();
    app.export_to(b.clone());
    app.wait_for_export();
    app.export_to(a.clone());
    app.wait_for_export();
    // Die Kennung ist der aufgelöste Pfad — hier derselbe Ordner, also nur
    // `canonicalize` über beide.
    let echt = |path: &Path| std::fs::canonicalize(path).unwrap();
    let keys: Vec<&Path> = app.checks.iter().map(|c| c.key.as_path()).collect();
    println!("nach A, B, A: {keys:?}");
    assert_eq!(keys, vec![echt(&b).as_path(), echt(&a).as_path()]);

    app.export_to(a.clone());
    app.wait_for_export();
    app.export_to(a.clone());
    app.wait_for_export();
    let keys: Vec<&Path> = app.checks.iter().map(|c| c.key.as_path()).collect();
    println!("nach A, B, A, A, A: {keys:?}");
    assert_eq!(keys, vec![echt(&b).as_path(), echt(&a).as_path()]);
    // „Alle hängen noch am Haken“ heißt: kein Urteil ist da. Gefragt wird das
    // jetzt direkt und nicht mehr am Neuzeichnen-Merker: seit Fix-Runde 8
    // schreibt auch der Export auf einem Thread, und der fordert am Ende zu
    // Recht ein Neuzeichnen an (sonst holte niemand sein Ergebnis ab). Der
    // Merker kann deshalb nicht mehr unterscheiden, wer ihn gesetzt hat —
    // `poll_export_checks() == 0` kann es.
    assert_eq!(app.poll_export_checks(), 0, "alle hängen noch am Haken");

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

/// **Die Gegenrichtung zu Befund R4-5.** `x/a.pdf` leckt, die Warnung steht.
/// Dann ein sauberer Export nach `y/a.pdf` — anderer Ordner, gleicher
/// Datei**name**: bis Fix-Runde 6 strich `note_export_warnings` alles unter
/// „a.pdf: “ und nahm die Leckwarnung über `x/a.pdf` mit, obwohl diese Datei
/// unverändert weiterleckte.
///
/// Geprüft wird hier beides: die fremde Warnung bleibt — und die **eigene**
/// verschwindet weiter, wenn dieselbe Datei sauber neu geschrieben wird
/// (sonst wäre der Fehler nur auf die andere Seite gekippt).
#[test]
fn zg_r4_2_gleicher_name_anderer_ordner_behaelt_die_leckwarnung() {
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
    app.wait_for_export();
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
    app.wait_for_export();
    println!("direkt nach y/a.pdf: {:?}", app.state.warnings);
    app.wait_for_export_checks();
    println!("nach y/a.pdf: {:?}", app.state.warnings);
    println!("Statuszeile: {}", app.state.status);
    // x/a.pdf leckt nach wie vor.
    let bytes = std::fs::read(x.join("a.pdf")).unwrap();
    assert!(!redact_pdf::leaks(&bytes, "GEHEIM-EINS").is_empty());
    assert!(
        app.state
            .warnings
            .iter()
            .any(|w| w.starts_with("a.pdf: ") && w.contains("NOCH in der Ausgabe")),
        "die Warnung über x/a.pdf ist weg — {:?}",
        app.state.warnings
    );

    // Und die **eigene** Warnung geht mit dem nächsten Export derselben
    // Datei: x/a.pdf noch einmal, diesmal sauber.
    app.export_to(x.join("a.pdf"));
    app.wait_for_export();
    app.wait_for_export_checks();
    println!("nach x/a.pdf (sauber): {:?}", app.state.warnings);
    let bytes = std::fs::read(x.join("a.pdf")).unwrap();
    assert!(redact_pdf::leaks(&bytes, "GEHEIM-EINS").is_empty());
    assert!(
        !app.state
            .warnings
            .iter()
            .any(|w| w.contains("NOCH in der Ausgabe")),
        "das geschlossene Leck nimmt seine Warnung mit — {:?}",
        app.state.warnings
    );
}

/// **Befund R4-5 (die richtige Erwartung, jetzt erfüllt):** die Warnung über
/// eine Datei verschwindet nur, wenn **diese** Datei neu geschrieben wird
/// oder das Dokument wechselt — nicht, weil eine andere Datei gleichen Namens
/// in einem anderen Ordner geschrieben wurde.
#[test]
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
    app.wait_for_export();
    app.wait_for_export_checks();
    assert!(app.state.set_region_rect(0, ueber(700.0)));
    app.export_to(y.join("a.pdf"));
    app.wait_for_export();
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
    app.wait_for_export();
    app.wait_for_export_checks();
    println!("1. Export (abgewählt): {:?}", app.state.warnings);
    assert_eq!(sagt_nichts(&app), 1);

    assert!(app.state.set_enabled(1, true));
    app.export_to(out.clone());
    app.wait_for_export();
    app.wait_for_export_checks();
    println!("2. Export (beide geschwärzt): {:?}", app.state.warnings);
    assert_eq!(
        sagt_nichts(&app),
        0,
        "die Warnung gehört zum vorigen Export"
    );

    assert!(app.state.set_enabled(1, false));
    app.export_to(out.clone());
    app.wait_for_export();
    app.wait_for_export_checks();
    app.export_to(out.clone());
    app.wait_for_export();
    app.wait_for_export_checks();
    println!("3.+4. Export (abgewählt): {:?}", app.state.warnings);
    assert_eq!(sagt_nichts(&app), 1, "einmal, nicht zweimal");

    app.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "demo.pdf");
    println!("Dokumentwechsel: {:?}", app.state.warnings);
    assert_eq!(sagt_nichts(&app), 0);
}

// ===========================================================================
// Was eine fallen gelassene Prüfung kostet (Messung)
// ===========================================================================

/// Ein Feld aus `/proc/self/status` (Linux) — `Threads:` zählt Threads,
/// `VmHWM:` die Spitze des Arbeitsspeichers in kB.
///
/// Steht unter `cfg`, weil `/proc` eine Einrichtung genau dieses Systems ist:
/// unter Windows fährt die CI `cargo test --workspace`, und dort gibt es kein
/// `/proc` — derselbe Laufzeitfehler, an dem der Job in dieser Schleife schon
/// dreimal rot war. Auf Linux bleibt die Messung streng (`expect`), woanders
/// gibt es sie nicht.
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

/// **Messung zur Frage „gehört ein Abbruchsignal hin?“**
///
/// `start_export_check` lässt die ältere Prüfung **fallen** (`checks.retain`),
/// bricht sie aber nicht ab: ihr Thread liest die Datei zu Ende, durchsucht
/// sie, sein `send` findet niemanden, und sein [`RepaintOnDrop`] fordert am
/// Ende trotzdem ein Neuzeichnen an. Bei N schnellen Exporten derselben Datei
/// laufen also N−1 vollständige Suchen umsonst weiter.
///
/// Gemessen wird, was das kostet: die Dauer **einer** Nachprüfung, die Zahl
/// der Threads unmittelbar nach fünf Exporten, die Zeit bis das letzte Urteil
/// da ist, die Zeit bis auch die fallen gelassenen Threads aus sind, und die
/// Spitze des Arbeitsspeichers (`VmHWM`) davor und danach.
///
/// `cargo test -p redact-gui --release zg_r4_2_mess -- --ignored --nocapture`
#[test]
#[ignore = "Messung: was eine fallen gelassene Nachprüfung kostet"]
#[cfg(target_os = "linux")]
fn zg_r4_2_mess_was_eine_fallengelassene_pruefung_kostet() {
    let dir = tmp("mess");
    let out = dir.join("out.pdf");
    let seiten = 300;
    let pages: Vec<Vec<TextItem>> = (0..seiten)
        .map(|p| {
            vec![
                TextItem::new(72.0, 700.0, 10.0, format!("Seite {p} GEHEIM-EINS")),
                TextItem::new(
                    72.0,
                    660.0,
                    10.0,
                    format!("Seite {p} GEHEIM-ZWEI und mehr Text, damit die Seite etwas wiegt"),
                ),
            ]
        })
        .collect();
    let doc = build_pdf(&pages);
    println!("Vorlage: {} Seiten, {} kB", seiten, doc.len() / 1024);
    let mut app = app_with(&doc);
    app.state
        .regions
        .push(text_region(0, ueber(700.0), "GEHEIM-EINS"));

    // Eine Prüfung allein — so lange läuft auch jede fallen gelassene weiter.
    let leer = proc_status("Threads:");
    let t0 = Instant::now();
    app.export_to(out.clone());
    app.wait_for_export();
    let export = t0.elapsed();
    app.wait_for_export_checks();
    let einzeln = t0.elapsed();
    println!(
        "Ausgabe: {} kB; ein Export {:?}, Export + Nachprüfung {:?}",
        std::fs::metadata(&out).unwrap().len() / 1024,
        export,
        einzeln
    );
    println!("Statuszeile: {}", app.state.status);

    // Was ein fallen gelassener Thread noch zu tun hat, am **oberen Rand der
    // Decke**: 1 000 Begriffe über derselben Datei, gemessen ohne Thread.
    let viele: Vec<String> = (0..redact_core::MAX_CHECK_NEEDLES)
        .map(|i| format!("GEHEIM-{i:04}"))
        .collect();
    let plan = ExportCheckPlan {
        kept_forms: vec![false; viele.len()],
        kept_literal: vec![false; viele.len()],
        needles: viele,
        ..ExportCheckPlan::default()
    };
    let t0 = Instant::now();
    let check = plan.clone().run(&out);
    let einer = t0.elapsed();
    println!(
        "ein Lauf mit {} Begriffen: {einer:?} ({})",
        plan.needles.len(),
        check.sentence()
    );

    // Fünf davon in Folge, jede lässt die vorige fallen — sie laufen weiter.
    let vorher = proc_status("VmHWM:");
    let t0 = Instant::now();
    for _ in 0..5 {
        app.start_export_check(
            "Messung".to_string(),
            plan.clone(),
            out.clone(),
            file_key(&out),
        );
    }
    let gestartet = t0.elapsed();
    let threads = proc_status("Threads:");
    assert_eq!(app.checks.len(), 1, "vier sind fallen gelassen");
    app.wait_for_export_checks();
    let bis_urteil = t0.elapsed();
    // Warten, bis auch die fallen gelassenen Threads aus sind.
    while proc_status("Threads:") > leer {
        std::thread::sleep(Duration::from_millis(5));
    }
    let bis_alle_aus = t0.elapsed();
    let nachher = proc_status("VmHWM:");
    println!(
        "fünf Prüfungen gestartet in {gestartet:?}; Threads danach: {threads} \
         (Grundlast {leer})\n\
         letztes Urteil nach {bis_urteil:?}; alle Threads aus nach {bis_alle_aus:?}\n\
         VmHWM {} MB → {} MB (+{} MB)",
        vorher / 1024,
        nachher / 1024,
        (nachher - vorher) / 1024
    );
    std::fs::remove_dir_all(&dir).ok();
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
    app.wait_for_export();
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
