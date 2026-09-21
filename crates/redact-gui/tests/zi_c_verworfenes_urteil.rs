//! Gegenprüfung der Korrektur zu Register #19 (Export auf eigenem Thread),
//! Linse **Widerlegen** — Agent „zi_c".
//!
//! Geprüft wird nicht der Bericht, sondern der Quelltext von
//! [`redact_gui::app`]: `export_to` streicht das noch unterwegs befindliche
//! Urteil der Nachprüfung **vor** dem ersten geschriebenen Byte
//!
//! ```text
//! // Ab hier entstehen neue Bytes dieser Datei.
//! let written = file_key(&out);
//! self.checks.retain(|pending| pending.key != written);
//! ```
//!
//! und `file_key` ist `std::fs::canonicalize(path)`. Beides zusammen ergibt
//! zwei Lagen, die die Korrektur nicht deckt und die diese Datei mit
//! öffentlichem Weg und dem Leck-Orakel belegt:
//!
//! 1. Ein **Symlink** als Ziel trägt die Kennung seines Ziels (canonicalize
//!    folgt ihm) — `write_file` schreibt aber grundsätzlich nicht durch Links
//!    hindurch. Der Export scheitert also, **nachdem** das Urteil über die
//!    verlinkte Datei weggeworfen ist. Die Datei liegt weiter auf der Platte,
//!    das Orakel findet den Klartext, und niemand sagt es.
//! 2. `--audit-log` gibt **jedem** Export denselben Logpfad. Ein zweiter
//!    Export in eine *andere* Ausgabedatei ist ausdrücklich erlaubt und läuft
//!    gleichzeitig — beide schreiben dasselbe Audit-Log, beide Meldungen
//!    nennen es, und beschrieben ist am Ende nur einer der beiden Läufe.
//!
//! `flock /tmp/redactrs-cargo.lock cargo test -p redact-gui --test zi_c_verworfenes_urteil`

use std::path::{Path, PathBuf};

use redact_core::{Rect, Region, Source};
use redact_gui::state::{AnnotatedRegion, AppState};
use redact_gui::Config;
use redact_pdf::testing::{build_pdf, TextItem};

const GEHEIM: &str = "GEHEIM-EINS";

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zi-c-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn ein_geheimnis() -> Vec<u8> {
    build_pdf(&[vec![TextItem::new(72.0, 700.0, 10.0, "Zeile A GEHEIM-EINS")]])
}

/// Ein Rechteck, unter dem nichts liegt: der Lauf gilt als geschwärzt, der
/// Klartext bleibt in der Ausgabe — genau das Leck, das die Nachprüfung
/// melden soll.
fn daneben() -> Rect {
    Rect::new(430.0, 20.0, 440.0, 40.0)
}

fn geladen(dir: &Path, config: Config) -> AppState {
    let bytes = ein_geheimnis();
    let input = dir.join("eins.pdf");
    std::fs::write(&input, &bytes).unwrap();
    let mut state = AppState::with_config(config);
    state.load_bytes(&bytes, Some(input)).unwrap();
    state.regions.push(AnnotatedRegion::new(Region::new(
        0,
        daneben(),
        Some(GEHEIM.to_string()),
        Source::Manual {
            reason: "zi-c".into(),
        },
    )));
    state
}

fn leckt(path: &Path) -> bool {
    let bytes = std::fs::read(path).unwrap();
    !redact_pdf::leaks(&bytes, GEHEIM).is_empty()
}

// ===========================================================================
// 1 — Der Symlink trägt die Kennung seines Ziels, die Bytes nicht
// ===========================================================================

/// **Widerlegung, Teil 1.** Alle drei Glieder der Kette, mit öffentlichem Weg:
///
/// * die exportierte Datei `out.pdf` leckt (Orakel `redact_pdf::leaks`) — ihr
///   Urteil ist das, was nicht verloren gehen darf;
/// * `canonicalize(link.pdf) == canonicalize(out.pdf)`, also ist
///   `file_key(link.pdf) == file_key(out.pdf)`: der Schnitt in `export_to`
///   trifft das Urteil **von out.pdf**, obwohl out.pdf niemand anfasst;
/// * `plan_export(link.pdf)` geht durch (der Schnitt wird also erreicht), und
///   erst `run()` scheitert am Symlink — kein Byte geschrieben, out.pdf
///   unverändert und weiter leckend.
#[test]
fn zi_c_ein_symlink_als_ziel_traegt_die_kennung_der_verlinkten_datei() {
    let dir = tmp("symlink");
    let state = geladen(&dir, Config::default());

    let out = dir.join("out.pdf");
    let audit = AppState::audit_path_for(&out);
    let outcome = state.export(&out, Some(&audit)).expect("erster Export");
    assert_eq!(outcome.drawn_rects, 1, "das Rechteck wurde gemalt");
    assert!(out.exists());
    assert!(
        leckt(&out),
        "die Vorlage muss lecken, sonst prüft dieser Test nichts"
    );
    let vorher = std::fs::read(&out).unwrap();

    // Ein gewöhnlicher Name im selben Ordner, der auf die Ausgabe zeigt.
    let link = dir.join("link.pdf");
    std::os::unix::fs::symlink(&out, &link).unwrap();

    // Glied 2: dieselbe Kennung. Das ist wörtlich `file_key` aus `app.rs`.
    assert_eq!(
        std::fs::canonicalize(&link).unwrap(),
        std::fs::canonicalize(&out).unwrap(),
        "canonicalize folgt dem Link — der Schnitt in export_to trifft out.pdf"
    );

    // Glied 3: der Plan entsteht (also läuft `export_to` über den Schnitt
    // hinweg), das Schreiben scheitert danach.
    let audit_link = AppState::audit_path_for(&link);
    let plan = state
        .plan_export(&link, Some(&audit_link))
        .expect("plan_export lehnt einen Symlink NICHT ab — der Schnitt wird erreicht");
    let err = plan.run().expect_err("write_file schreibt nicht durch Links");
    let text = err.to_string();
    assert!(
        text.contains("symbolischer Link"),
        "unerwarteter Fehler: {text}"
    );

    // Und die Folge: out.pdf ist unangetastet und leckt weiter.
    assert_eq!(std::fs::read(&out).unwrap(), vorher, "out.pdf wurde berührt");
    assert!(leckt(&out), "out.pdf leckt weiter — ihr Urteil fehlt");
    assert!(!link.exists() || std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());

    std::fs::remove_dir_all(&dir).ok();
}

/// Gegenprobe in die **andere** Richtung: ein gewöhnliches zweites Ziel im
/// selben Ordner geht durch. Die Ablehnung oben liegt am Link, nicht daran,
/// dass hier irgendetwas grundsätzlich klemmt.
#[test]
fn zi_c_ein_gewoehnliches_zweites_ziel_geht_durch() {
    let dir = tmp("gewoehnlich");
    let state = geladen(&dir, Config::default());

    for name in ["eins_out.pdf", "zwei_out.pdf"] {
        let out = dir.join(name);
        let audit = AppState::audit_path_for(&out);
        state
            .plan_export(&out, Some(&audit))
            .expect("Plan")
            .run()
            .expect("Lauf");
        assert!(out.exists(), "{name} fehlt");
    }

    std::fs::remove_dir_all(&dir).ok();
}

// ===========================================================================
// 2 — Zwei gleichzeitige Exporte, ein Audit-Log
// ===========================================================================

/// **Widerlegung, Teil 2.** `export_to` lehnt den zweiten Export nur ab, wenn
/// er dieselbe **Ausgabedatei** schreibt (`EXPORT_BUSY`, Schlüssel
/// `writing_key(out)`). Ein Export schreibt aber **zwei** Dateien: die Ausgabe
/// und das Audit-Log. Mit `--audit-log` ist das Log für jede Ausgabe dasselbe
/// (`AppState::audit_target`), also laufen zwei erlaubte, gleichzeitige
/// Exporte auf genau die Lage zu, mit der die Korrektur den zweiten Export
/// derselben Datei begründet: die Reihenfolge der `rename`-Aufrufe ist nicht
/// die der Klicks.
#[test]
fn zi_c_zwei_gleichzeitige_exporte_teilen_ein_audit_log() {
    let dir = tmp("auditlog");
    let log = dir.join("log.json");
    let state = geladen(
        &dir,
        Config {
            audit_log: Some(log.clone()),
            ..Config::default()
        },
    );

    let b = dir.join("b.pdf");
    let c = dir.join("c.pdf");
    // Derselbe Logpfad für beide Ausgaben — das ist `audit_target`.
    assert_eq!(state.audit_target(&b), state.audit_target(&c));
    assert_eq!(state.audit_target(&b), log);

    let plan_b = state.plan_export(&b, Some(&log)).expect("Plan b");
    let plan_c = state.plan_export(&c, Some(&log)).expect("Plan c");

    // So, wie `export_to` es ausdrücklich erlaubt: gleichzeitig, weil die
    // Ausgabedateien verschieden sind.
    let hb = std::thread::spawn(move || plan_b.run());
    let hc = std::thread::spawn(move || plan_c.run());
    let ob = hb.join().unwrap().expect("Export b");
    let oc = hc.join().unwrap().expect("Export c");

    assert!(b.exists() && c.exists(), "beide Ausgaben fehlen nicht");
    // Beide Meldungen nennen dasselbe Log …
    assert_eq!(ob.audit_log, oc.audit_log);
    assert_eq!(ob.audit_log.as_deref(), Some(log.display().to_string().as_str()));

    // … beschrieben ist aber nur einer der beiden Läufe.
    let text = std::fs::read_to_string(&log).unwrap();
    let nennt_b = text.contains("b.pdf");
    let nennt_c = text.contains("c.pdf");
    eprintln!("Audit-Log nennt b.pdf: {nennt_b}, c.pdf: {nennt_c}");
    assert!(
        nennt_b != nennt_c,
        "das Log beschreibt beide oder keinen — dann ist die Lage anders als gedacht"
    );

    std::fs::remove_dir_all(&dir).ok();
}
