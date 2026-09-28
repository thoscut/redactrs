//! Gegenprüfung der **Nachbesserung** zu Register #19, Linse **Widerlegen** —
//! Agent „zj_c".
//!
//! Der Einwand der letzten Runde war: der Schnitt in `export_to` wirft das
//! Leck-Urteil einer Ausgabedatei weg, obwohl kein Byte entsteht („volle
//! Platte, `EACCES`, Panik") — das Leck bleibt stumm. Die Nachbesserung hängt
//! den Schnitt an `PendingExport::wrote`, und diese Fahne ist
//! `outcome.output.is_some()`, ausdrücklich **nicht** `result.is_ok()`. Der
//! Bericht begründet das wörtlich so:
//!
//! > `redact_pipeline::apply` schreibt die PDF in Schritt 11 und das Audit-Log
//! > erst in Schritt 12, ein `Err` kann also hinter fertigen Bytes kommen […]
//! > Damit ist die Fahne in BEIDEN Richtungen exakt.
//!
//! Genau dieser Fall ist hier geprüft — und er ist der Fall, in dem die
//! Oberfläche **kein Urteil** mehr abgibt. `app.rs` behandelt ihn in
//! `finish_export` so:
//!
//! ```text
//! match result {
//!     Ok(outcome) => { … self.start_export_check(prefix, check, …); }
//!     Err(e) => self.report(Err(e)),
//! }
//! ```
//!
//! und `report` setzt nur `self.error` und die Statuszeile. Also:
//!
//! * `wrote == true` ⇒ `poll_exports` streicht das Urteil über die **alten**
//!   Bytes (richtig, es gibt sie nicht mehr);
//! * `result == Err` ⇒ **keine** neue Nachprüfung, **keine** Warnung, kein
//!   `note_export_warnings` (das steht im `Ok`-Zweig) — über die **neuen**
//!   Bytes sagt niemand etwas.
//!
//! „Volle Platte" ist derselbe Fall wie im Einwand, nur einen Schritt später:
//! die PDF passt noch, das Log nicht. Diese Datei belegt die Kette mit
//! öffentlichem Weg und dem Leck-Orakel (`redact_pdf::leaks`).
//!
//! Auslöser des Log-Fehlers ist hier ein **gewöhnlicher, langer Dateiname**
//! für das Log: `redact_pdf::write_file` legt eine temporäre Datei
//! `.<name>.redact-<pid>-<n>.tmp` daneben an, und die ist um rund zwanzig
//! Zeichen länger als der Name selbst. Ein Logname von 240 Zeichen ist damit
//! anlegbar, seine Temp-Datei nicht (`NAME_MAX` = 255). Der Lauf scheitert
//! also genau dort, wo eine volle Platte oder ein nicht beschreibbares
//! Logverzeichnis ihn treffen würde: **hinter** der geschriebenen PDF. Der
//! Test als Ganzes braucht keine besonderen Rechte und läuft auch als `root`.
//!
//! `flock /tmp/redactrs-cargo.lock cargo test -p redact-gui --test zj_c_leck_ohne_urteil`

use std::path::{Path, PathBuf};

use redact_core::{Rect, Region, Source};
use redact_gui::state::{AnnotatedRegion, AppState};
use redact_gui::Config;
use redact_pdf::testing::{build_pdf, TextItem};

const GEHEIM: &str = "GEHEIM-EINS";

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zj-c-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn ein_geheimnis() -> Vec<u8> {
    build_pdf(&[vec![TextItem::new(
        72.0,
        700.0,
        10.0,
        "Zeile A GEHEIM-EINS",
    )]])
}

/// Ein Rechteck, unter dem nichts liegt: der Lauf gilt als geschwärzt, der
/// Klartext bleibt in der Ausgabe — das Leck, das die Nachprüfung melden soll.
fn daneben() -> Rect {
    Rect::new(430.0, 20.0, 440.0, 40.0)
}

/// Ein zweites Rechteck, unter dem nichts liegt — an einer **anderen** Stelle,
/// damit die Ausgabe des zweiten Exports auch Byte für Byte eine andere ist.
fn daneben_anders() -> Rect {
    Rect::new(380.0, 25.0, 402.0, 47.0)
}

/// Ein Rechteck über der Zeile: es trifft, die Ausgabe ist sauber.
fn ueber() -> Rect {
    Rect::new(60.0, 690.0, 520.0, 714.0)
}

fn geladen(dir: &Path, config: Config, rect: Rect) -> AppState {
    let bytes = ein_geheimnis();
    let input = dir.join("eins.pdf");
    std::fs::write(&input, &bytes).unwrap();
    let mut state = AppState::with_config(config);
    state.load_bytes(&bytes, Some(input)).unwrap();
    state.regions.push(AnnotatedRegion::new(Region::new(
        0,
        rect,
        Some(GEHEIM.to_string()),
        Source::Manual {
            reason: "zj-c".into(),
        },
    )));
    state
}

/// Die Dateiidentität — `rename` legt eine neue an, und genau daran hängt die
/// Aussage „die Bytes von vorhin gibt es nicht mehr".
///
/// Steht unter `cfg`, weil die Inode-Nummer eine Einrichtung genau dieses
/// Systems ist: `std::os::unix` gibt es auf Windows nicht, und dort bricht das
/// nicht zur Laufzeit, sondern schon den **Bau** — der dortige CI-Job fährt
/// `cargo clippy --workspace --all-targets`. Genau daran war er in dieser
/// Schleife fünfmal rot; diese Datei war der fünfte Fall, und zwar ausgerechnet
/// ein Beleg der Gegenprüfung, die diese Klasse jagt.
///
/// Auf anderen Systemen bleibt das Glied ungeprüft — [`ersetzt`] sagt es dann
/// auf `stderr`, statt still durchzulaufen.
#[cfg(unix)]
fn inode(path: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).unwrap().ino()
}

/// Hat `rename` die Datei wirklich ersetzt? Auf Unix an der Inode-Nummer
/// entschieden; woanders nicht entscheidbar, und dann wird es gesagt.
fn ersetzt(path: &Path, vorher: Option<u64>) -> bool {
    #[cfg(unix)]
    {
        match vorher {
            Some(alt) => inode(path) != alt,
            None => false,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (path, vorher);
        eprintln!(
            "ohne Inode-Nummer ist nicht entscheidbar, ob `rename` die Datei ersetzt hat \
             — dieses Glied der Beweiskette bleibt hier ungeprüft"
        );
        true
    }
}

/// Die Inode-Nummer, wo es eine gibt.
fn inode_falls_moeglich(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        Some(inode(path))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

fn leckt(bytes: &[u8]) -> bool {
    !redact_pdf::leaks(bytes, GEHEIM).is_empty()
}

/// Ein Logname, den es geben darf und dessen Temp-Datei zu lang ist.
fn langer_logname(dir: &Path) -> PathBuf {
    let name = format!("{}.json", "l".repeat(235));
    assert_eq!(name.len(), 240, "der Name selbst bleibt unter NAME_MAX");
    dir.join(name)
}

/// Der erste Export: eine Ausgabe, die **leckt**, mit gewöhnlichem Log.
/// Danach steht `out` auf der Platte und das Orakel findet den Klartext.
fn erster_export_mit_leck(dir: &Path, out: &Path) {
    let state = geladen(dir, Config::default(), daneben());
    let audit = state.audit_target(out);
    let outcome = state
        .plan_export(out, Some(&audit))
        .expect("Plan")
        .run()
        .expect("erster Export");
    assert_eq!(outcome.drawn_rects, 1, "das Rechteck wurde gemalt");
    assert!(
        leckt(&std::fs::read(out).unwrap()),
        "die Vorlage muss lecken — sonst prüft dieser Test nichts"
    );
}

// ===========================================================================
// 1 — Bytes geschrieben, Lauf gescheitert: die Fahne steht, das Urteil fehlt
// ===========================================================================

/// **Widerlegung.** Alle Glieder der Kette, mit öffentlichem Weg:
///
/// 1. `out.pdf` trägt nach dem zweiten Export **neue** Bytes und leckt
///    (Orakel);
/// 2. `run_reporting` gibt dazu `(true, Err(…))` zurück — die Fahne, an der
///    `poll_exports` das Urteil über die alten Bytes streicht, **und** das
///    `Err`, das `finish_export` in `report(Err(…))` schickt;
/// 3. das Urteil, das niemand mehr holt, wäre da gewesen: derselbe
///    `ExportCheckPlan`, den `export_to` vor dem Thread fasst, findet den
///    Klartext in den neuen Bytes und sagt „NOCH in der Ausgabe".
///
/// Ergebnis: das Leck ist wieder stumm — nicht weil ein Urteil über
/// unveränderte Bytes weggeworfen wurde (das ist geschlossen), sondern weil
/// für die **geschriebenen** Bytes keines entsteht.
#[test]
fn zj_c_bytes_geschrieben_lauf_gescheitert_kein_urteil() {
    let dir = tmp("kein-urteil");
    let out = dir.join("out.pdf");
    erster_export_mit_leck(&dir, &out);
    let vorher = std::fs::read(&out).unwrap();
    let inode_vorher = inode_falls_moeglich(&out);

    // Zweiter Export **derselben** Datei, Log nicht schreibbar.
    let log = langer_logname(&dir);
    let state = geladen(
        &dir,
        Config {
            audit_log: Some(log.clone()),
            ..Config::default()
        },
        daneben_anders(),
    );
    // Genau der Pfad, den `export_to` benutzt (`audit_target`).
    assert_eq!(state.audit_target(&out), log);

    let plan = state
        .plan_export(&out, Some(&log))
        .expect("plan_export lehnt nicht ab — der Export läuft an");
    let (wrote, result) = plan.run_reporting();
    let fehler = result
        .expect_err("das Log kann nicht geschrieben werden")
        .to_string();
    println!("Fehler des Laufs: {fehler}");
    println!("Fahne `wrote`: {wrote}");

    // Glied 1: die Datei trägt NEUE Bytes und leckt.
    let nachher = std::fs::read(&out).unwrap();
    assert!(
        ersetzt(&out, inode_vorher),
        "die Datei muss ersetzt worden sein (rename) — sonst prüft dieser Test etwas anderes"
    );
    assert_ne!(
        nachher, vorher,
        "und sie trägt andere Bytes: die Ausgabe des zweiten Exports"
    );
    assert!(!log.exists(), "das Log darf es nicht geben");
    assert!(leckt(&nachher), "das Orakel: die neuen Bytes lecken");

    // Glied 2: die Fahne steht (Schnitt in `poll_exports` fällt) **und** der
    // Lauf ist ein `Err` (`finish_export` → `report`, keine Nachprüfung).
    assert!(
        wrote,
        "die Fahne muss stehen, sonst urteilte die alte Prüfung über neue Bytes"
    );

    // Glied 3: das Urteil wäre da gewesen.
    let urteil = state.plan_export_check(&state.hit_summary()).run(&out);
    let warnung = urteil
        .warning()
        .expect("die Nachprüfung der neuen Bytes hätte gewarnt");
    println!("Das Urteil, das die Oberfläche nie holt: {warnung}");
    assert!(
        warnung.contains("NOCH in der Ausgabe"),
        "unerwartetes Urteil: {warnung}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

// ===========================================================================
// 2 — Dieselbe Stelle, andere Richtung: die alte Leckwarnung bleibt stehen
// ===========================================================================

/// Die Gegenrichtung derselben Lücke (Fehlalarm statt Leck): der zweite Export
/// deckt **richtig** ab, schreibt saubere Bytes und scheitert am Log. Weil
/// `note_export_warnings` (und damit `forget_warnings_of`) nur im `Ok`-Zweig
/// von `finish_export` steht, bleibt die Leckwarnung des ersten Exports über
/// Bytes stehen, die es nicht mehr gibt.
///
/// Belegt wird hier die Voraussetzung mit dem Orakel: die Datei ist nach dem
/// gescheiterten Lauf **sauber**, und der Lauf meldet trotzdem `Err` bei
/// gesetzter Fahne.
#[test]
fn zj_c_sauber_geschrieben_lauf_gescheitert_warnung_bleibt() {
    let dir = tmp("warnung-bleibt");
    let out = dir.join("out.pdf");
    erster_export_mit_leck(&dir, &out);

    let log = langer_logname(&dir);
    let state = geladen(
        &dir,
        Config {
            audit_log: Some(log.clone()),
            ..Config::default()
        },
        ueber(),
    );
    let (wrote, result) = state
        .plan_export(&out, Some(&log))
        .expect("Plan")
        .run_reporting();
    assert!(result.is_err(), "das Log muss scheitern");
    assert!(wrote, "geschrieben wurde trotzdem");

    let nachher = std::fs::read(&out).unwrap();
    assert!(
        !leckt(&nachher),
        "die neuen Bytes sind sauber — die alte Leckwarnung ist gegenstandslos"
    );
    let urteil = state.plan_export_check(&state.hit_summary()).run(&out);
    println!("Urteil über die neuen Bytes: {}", urteil.status_line());
    assert!(
        urteil.warning().is_none(),
        "sauber heißt: keine Warnung — {:?}",
        urteil.warning()
    );

    std::fs::remove_dir_all(&dir).ok();
}

// ===========================================================================
// 3 — Gegenprobe: mit gewöhnlichem Log geht derselbe Weg glatt durch
// ===========================================================================

/// Damit die Läufe oben nicht an irgendetwas anderem hängen: derselbe zweite
/// Export mit einem **gewöhnlichen** Lognamen schreibt Datei und Log und endet
/// als `Ok` — dort startet die Oberfläche ihre Nachprüfung.
#[test]
fn zj_c_gegenprobe_gewoehnliches_log_geht_durch() {
    let dir = tmp("gegenprobe");
    let out = dir.join("out.pdf");
    erster_export_mit_leck(&dir, &out);

    let log = dir.join("gewoehnlich.json");
    let state = geladen(
        &dir,
        Config {
            audit_log: Some(log.clone()),
            ..Config::default()
        },
        daneben(),
    );
    let (wrote, result) = state
        .plan_export(&out, Some(&log))
        .expect("Plan")
        .run_reporting();
    let outcome = result.expect("gewöhnlicher Log-Pfad: der Lauf geht durch");
    assert!(wrote);
    assert_eq!(
        outcome.audit_log.as_deref(),
        Some(log.display().to_string().as_str())
    );
    assert!(log.exists(), "das Log steht");

    std::fs::remove_dir_all(&dir).ok();
}
