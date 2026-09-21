//! Nachbesserung zu Fix-Runde 8, Register #19: **wann** der Schnitt fällt,
//! der ein Urteil über die alten Bytes wegwirft.
//!
//! Der Umbau der letzten Runde hat den Schnitt aus `start_export_check` an den
//! **Anfang** von [`RedactApp::export_to`] gezogen — also an den *Wunsch*, zu
//! schreiben, statt an die geschriebenen Bytes. Zwei Gegenprüfer haben
//! unabhängig belegt, dass das ein stilles Leck ist: wird nie ein Byte
//! geschrieben (ein Symlink als Ziel, volle Platte, `EACCES`, eine Panik auf
//! dem Thread), dann liegt die Datei unverändert da, das Leck-Orakel
//! ([`redact_pdf::leaks`]) findet den Klartext — und in Statuszeile und
//! Warnungen steht kein Wort davon, weil das Urteil beim Klick weggeworfen
//! wurde.
//!
//! Die Anforderungen stehen wirklich gegeneinander, und ein Zurücknehmen wäre
//! kein Fix: der asynchrone Export hat das Fenster erst aufgemacht, in dem die
//! alte Prüfung die **neuen** Bytes lesen kann. Der Ausweg ist die dritte
//! Stelle: nicht der Klick und nicht das abgeholte Ergebnis, sondern der
//! Export-Thread selbst sagt, **dass er geschrieben hat**
//! (`PendingExport::wrote`, gesetzt aus `ExportPlan::run_reporting` heraus
//! und **vor** dem `send`). [`RedactApp::poll_exports`] räumt daran die
//! Urteile über die alten Bytes weg — und weil
//! [`RedactApp::poll_export_checks`] `poll_exports` als erstes ruft, ist der
//! Schnitt immer vor dem Abholen eines Urteils.
//!
//! Geprüft wird hier fünf Mal:
//!
//! 1. Ziel ist ein Symlink — kein Byte entsteht, das Leck-Urteil bleibt.
//! 2. Der Export stirbt auf seinem Thread — kein Byte entsteht, das
//!    Leck-Urteil bleibt.
//! 3. Neue Bytes **sind** da: dann fällt der Schnitt, und zwar noch bevor das
//!    bereitliegende Urteil über die alten Bytes abgeholt wird.
//! 4. Ein Klick mitten im Bild sieht den fertigen Export (Fehlalarm
//!    [`EXPORT_BUSY`], der eine Handlung verlor).
//! 5. Die Warnung eines gescheiterten Exports hängt an derselben Kennung wie
//!    die eines geglückten — sonst steht „es wurde keine Datei geschrieben“
//!    neben der Erfolgsmeldung. Dazu die Kennung selbst: drei Schreibweisen
//!    einer Datei, eine Kennung.
//!
//! Kindmodul von `app` wie `zh_c_export_tests`: `export_to`, `exports`,
//! `checks`, die Haken und `ui_ctx` sind privat.
//!
//! `cargo test -p redact-gui --lib zh2_c`

use super::*;

use std::sync::Arc;
use std::time::{Duration, Instant};

use redact_core::{Rect, Region, Source};
use redact_pdf::testing::{build_pdf, TextItem};

use crate::state::AnnotatedRegion;

// --------------------------------------------------------------- Hilfsmittel

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "zh2-c-{tag}-{}-{:?}",
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
            reason: "zh2-c".into(),
        },
    ))
}

/// Ein Dokument mit einem Geheimnis auf Seite 1.
fn ein_geheimnis() -> Vec<u8> {
    build_pdf(&[vec![TextItem::new(
        72.0,
        700.0,
        10.0,
        "Zeile A GEHEIM-EINS",
    )]])
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

/// Ein Rechteck, unter dem nichts liegt: die Zeile gilt als geschwärzt und
/// bleibt trotzdem stehen — das Leck, das die Nachprüfung finden muss.
fn daneben() -> Rect {
    Rect::new(430.0, 20.0, 440.0, 40.0)
}

fn app_with(bytes: &[u8]) -> RedactApp {
    let mut app = RedactApp::silent(Config {
        no_patterns: true,
        ..Config::default()
    });
    app.open_bytes_and_analyze(bytes, "eins.pdf");
    assert!(app.state.is_loaded());
    app
}

/// Wartet mit Frist darauf, dass der Thread am Gatter ankommt.
///
/// **Mit** Frist, weil die Gegenprobe sonst hängt statt rot zu werden.
fn warte_aufs_gatter(gate: &Arc<CheckGate>, was: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !gate.arrived() {
        assert!(
            Instant::now() < deadline,
            "{was}: am Gatter kam niemand an"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Der erste Export: eine Ausgabe, die **leckt** (das Rechteck geht daneben),
/// und deren Nachprüfung am Haken hängt — ihr Urteil liegt also noch nicht
/// vor.
///
/// Gibt `(app, Prüfhaken)` zurück. Danach gilt: `out` steht auf der Platte,
/// enthält den Klartext, und `app.checks` zählt 1.
fn erster_export_mit_leck(app: &mut RedactApp, out: &std::path::Path) -> Arc<CheckGate> {
    let pruefhaken = Arc::new(CheckGate::default());
    app.hold_check = Some(pruefhaken.clone());
    app.state
        .regions
        .push(text_region(0, daneben(), "GEHEIM-EINS"));
    app.export_to(out.to_path_buf());
    app.wait_for_export();
    warte_aufs_gatter(&pruefhaken, "die Nachprüfung des ersten Exports");
    assert_eq!(app.checks.len(), 1, "die erste Prüfung muss laufen");
    let bytes = std::fs::read(out).unwrap();
    assert!(
        !redact_pdf::leaks(&bytes, "GEHEIM-EINS").is_empty(),
        "die Vorlage muss lecken — sonst prüft dieser Test nichts"
    );
    // Der nächste Export soll seine Prüfung nicht am Haken hängen lassen.
    app.hold_check = None;
    pruefhaken
}

fn leckwarnung(app: &RedactApp) -> Option<&String> {
    app.state
        .warnings
        .iter()
        .find(|w| w.contains("NOCH in der Ausgabe"))
}

// ===========================================================================
// 1 — Ziel ist ein Symlink: kein Byte, also kein Schnitt
// ===========================================================================

/// **Der Befund, wörtlich.** `file_key` ist `std::fs::canonicalize` — es
/// **folgt** Symlinks. Der Schnitt am Anfang von `export_to` warf damit das
/// Urteil der *verlinkten* Datei weg; danach lehnte
/// [`redact_pdf::document::check_target`] den Link ab und schrieb kein Byte.
/// Ergebnis: `out.pdf` lag unverändert da, das Orakel fand den Klartext, und
/// niemand sagte es.
///
/// Jetzt hängt der Schnitt an den geschriebenen Bytes. Hier entstehen keine —
/// also bleibt das Urteil, und es ist ein Urteil über genau die Bytes, die auf
/// der Platte stehen.
///
/// Steht unter `cfg(unix)`: unter Windows verlangt `symlink` Sonderrechte, und
/// ein Test, der ohne sie panickt, bricht den dortigen Lauf.
#[test]
#[cfg(unix)]
fn zh2_c_ein_symlink_als_ziel_nimmt_dem_leck_nicht_die_stimme() {
    let dir = tmp("symlink-ziel");
    let out = dir.join("out.pdf");
    let mut app = app_with(&ein_geheimnis());
    let pruefhaken = erster_export_mit_leck(&mut app, &out);

    // Zweites Ziel: ein Symlink auf **dieselbe** Datei.
    let link = dir.join("link.pdf");
    std::os::unix::fs::symlink(&out, &link).unwrap();
    assert_eq!(
        std::fs::canonicalize(&link).unwrap(),
        std::fs::canonicalize(&out).unwrap(),
        "der Link zeigt auf die Ausgabedatei"
    );

    let vorher = std::fs::read(&out).unwrap();
    app.export_to(link.clone());
    app.wait_for_export();
    println!("Statuszeile nach dem Export auf den Link: {}", app.state.status);

    // redact-rs schreibt nicht durch Links hindurch — kein Byte hat sich
    // geändert.
    assert_eq!(
        std::fs::read(&out).unwrap(),
        vorher,
        "durch den Link darf nichts geschrieben werden"
    );

    // Und weil kein Byte entstand, gilt das Urteil über die Bytes weiter.
    pruefhaken.release();
    app.wait_for_export_checks();
    println!("Warnungen: {:?}", app.state.warnings);
    let bytes = std::fs::read(&out).unwrap();
    assert!(
        !redact_pdf::leaks(&bytes, "GEHEIM-EINS").is_empty(),
        "das Orakel: die Datei leckt wirklich"
    );
    assert!(
        leckwarnung(&app).is_some(),
        "das Leck steht in der Datei und muss in den Warnungen stehen: {:?}",
        app.state.warnings
    );

    std::fs::remove_dir_all(&dir).ok();
}

// ===========================================================================
// 2 — Der Export stirbt auf seinem Thread: kein Byte, also kein Schnitt
// ===========================================================================

/// Dieselbe Klasse ohne Symlink: der zweite Export **derselben** Datei
/// schreibt nichts (hier eine Panik auf dem Thread; volle Platte und `EACCES`
/// enden genauso).
///
/// Der einzige Satz war „es wurde keine Datei geschrieben“ — über diesen
/// Export wahr, über die Datei auf der Platte irreführend: dort stand
/// weiterhin der Klartext des ersten Exports, und sein Urteil war weg.
#[test]
fn zh2_c_ein_gescheiterter_zweiter_export_nimmt_dem_leck_nicht_die_stimme() {
    let dir = tmp("panik-zweiter");
    let out = dir.join("out.pdf");
    let mut app = app_with(&ein_geheimnis());
    let pruefhaken = erster_export_mit_leck(&mut app, &out);

    let vorher = std::fs::read(&out).unwrap();
    app.force_panic_in_export = true;
    app.export_to(out.clone());
    app.wait_for_export();
    assert_eq!(app.state.status, EXPORT_BROKEN, "{}", app.state.status);
    assert_eq!(
        std::fs::read(&out).unwrap(),
        vorher,
        "geschrieben wurde nichts"
    );

    pruefhaken.release();
    app.wait_for_export_checks();
    println!("Warnungen: {:?}", app.state.warnings);
    let bytes = std::fs::read(&out).unwrap();
    assert!(
        !redact_pdf::leaks(&bytes, "GEHEIM-EINS").is_empty(),
        "das Orakel: die Datei leckt wirklich"
    );
    assert!(
        leckwarnung(&app).is_some(),
        "das Leck steht in der Datei und muss in den Warnungen stehen: {:?}",
        app.state.warnings
    );
    // Beides gehört dazu: der gescheiterte Export **und** das Leck.
    assert!(
        app.state.warnings.iter().any(|w| w.contains("abgebrochen")),
        "{:?}",
        app.state.warnings
    );

    std::fs::remove_dir_all(&dir).ok();
}

// ===========================================================================
// 3 — Neue Bytes: der Schnitt fällt, und zwar vor dem nächsten Urteil
// ===========================================================================

/// Die Gegenrichtung, und der Grund, warum der Schnitt überhaupt gebraucht
/// wird: sobald neue Bytes da sind, ist ein bereitliegendes Urteil über die
/// **alten** gegenstandslos — und in der gefährlichen Richtung eine
/// Entwarnung über eine Ausgabe, die niemand geprüft hat.
///
/// Aufgebaut mit dem Haken hinter dem Schreiben: der Export-Thread hat
/// geschrieben und seine Fahne gesetzt, sein Ergebnis liegt aber noch **nicht**
/// im Kanal. Genau in diesem Fenster wird gepollt. Das Urteil über die alten
/// Bytes ist von Hand eingesetzt und liegt fertig im Kanal — es kann also
/// nichts anderes als der Schnitt sein, was es zurückhält.
///
/// Mutation (der Schnitt an der Fahne in `poll_exports` entfernt): die
/// Entwarnung des alten Urteils steht in der Statuszeile — rot.
#[test]
fn zh2_c_neue_bytes_werfen_das_urteil_ueber_die_alten_weg() {
    const ALT: &str = "Export: der erste Lauf";
    let dir = tmp("neue-bytes");
    let out = dir.join("out.pdf");
    let vorhaken = Arc::new(CheckGate::default());
    let nachhaken = Arc::new(CheckGate::default());

    let mut app = app_with(&ein_geheimnis());
    app.state
        .regions
        .push(text_region(0, ueber(700.0), "GEHEIM-EINS"));

    // Ein erster Export ganz durch — danach gibt es die Datei, und damit die
    // Kennung, unter der ein Urteil über **ihre** Bytes hängen würde.
    app.export_to(out.clone());
    app.wait_for_export_checks();
    assert!(out.exists());
    let kennung = writing_key(&out);

    // Der zweite Export: angehalten **vor** dem Schreiben.
    app.hold_export = Some(vorhaken.clone());
    app.hold_export_after_write = Some(nachhaken.clone());
    app.export_to(out.clone());
    warte_aufs_gatter(&vorhaken, "der zweite Export vor dem Schreiben");

    // Jetzt — nach dem Klick, vor dem ersten neuen Byte — liegt ein Urteil
    // über die Bytes auf der Platte bereit: eine Entwarnung.
    let (sender, receiver) = std::sync::mpsc::channel();
    sender
        .send(ExportCheck {
            checked: 1,
            ..ExportCheck::default()
        })
        .unwrap();
    app.checks.push(PendingCheck {
        prefix: ALT.to_string(),
        file: "out.pdf".to_string(),
        key: kennung,
        result: receiver,
    });
    assert_eq!(
        app.checks.len(),
        1,
        "solange kein Byte geschrieben ist, gilt dieses Urteil"
    );

    // Geschrieben ist geschrieben — aber das Ergebnis des Exports liegt noch
    // nicht im Kanal. Genau hier wird gepollt.
    vorhaken.release();
    warte_aufs_gatter(&nachhaken, "der zweite Export nach dem Schreiben");
    assert_eq!(
        app.poll_export_checks(),
        0,
        "in diesem Bild darf kein Urteil herauskommen"
    );
    println!("Statuszeile im Fenster: {}", app.state.status);
    assert!(
        app.checks.is_empty(),
        "das Urteil gehört zu Bytes, die es nicht mehr gibt"
    );
    assert!(
        !app.state.status.contains(ALT),
        "eine Entwarnung über die alten Bytes: {}",
        app.state.status
    );
    assert!(
        !app.state.warnings.iter().any(|w| w.contains("out.pdf:")
            && w.contains("nicht mehr in der Ausgabe")),
        "{:?}",
        app.state.warnings
    );

    nachhaken.release();
    app.wait_for_export_checks();
    let status = app.state.status.clone();
    println!("Statuszeile am Ende: {status}");
    assert!(status.starts_with("Export:"), "{status}");
    assert!(status.contains("Nachprüfung:"), "{status}");
    assert!(!status.contains(ALT), "{status}");

    std::fs::remove_dir_all(&dir).ok();
}

// ===========================================================================
// 4 — Ein Klick mitten im Bild sieht den fertigen Export
// ===========================================================================

/// **Fehlalarm [`EXPORT_BUSY`], und mit ihm ging eine Handlung verloren.**
///
/// `exports` wurde ausschließlich von [`RedactApp::poll_exports`] abgeräumt,
/// und das läuft genau einmal je Bild am **Anfang** von
/// `eframe::App::update`. Der Klick läuft **mitten** im Bild, und zwischen
/// beidem steht der blockierende Speichern-Dialog: der vorige Export war
/// längst fertig, sein Ergebnis lag im Kanal — und der zweite Export wurde
/// trotzdem abgelehnt und **nicht angelegt**. Ein Bild später überschrieb die
/// Erfolgsmeldung des ersten Exports samt sauberem Urteil die Ablehnung: der
/// Nutzerin stand eine Entwarnung vor der Nase, während die Datei den alten
/// Stand trug.
///
/// Nachgewiesen mit dem Orakel: `GEHEIM-ZWEI`, das erst der zweite Klick
/// abdeckt, steht danach weiter in der Datei.
///
/// Dass der Export wirklich fertig ist, sagt sein eigener Wächter
/// ([`RepaintOnDrop`]) — er wird als **letztes** im Thread fallen gelassen,
/// also nach dem `send`.
#[test]
fn zh2_c_ein_klick_mitten_im_bild_sieht_den_fertigen_export() {
    let dir = tmp("mitten-im-bild");
    let out = dir.join("out.pdf");
    let ctx = egui::Context::default();

    let mut app = app_with(&zwei_geheimnisse());
    app.ui_ctx = Some(ctx.clone());
    app.state
        .regions
        .push(text_region(0, ueber(700.0), "GEHEIM-EINS"));

    // Klick 1 — und dann **kein Bild**: nichts wird abgeholt.
    app.export_to(out.clone());
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ctx.has_requested_repaint() {
        assert!(
            Instant::now() < deadline,
            "der Export-Thread kommt nicht zum Ende"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        app.exports.len(),
        1,
        "abgeräumt wird erst beim Abholen — und das ist noch nicht passiert"
    );

    // Klick 2, mitten im Bild: auch GEHEIM-ZWEI wird abgedeckt.
    app.state
        .regions
        .push(text_region(0, ueber(660.0), "GEHEIM-ZWEI"));
    app.export_to(out.clone());
    println!("Statuszeile nach Klick 2: {}", app.state.status);
    assert_ne!(
        app.state.status, EXPORT_BUSY,
        "der erste Export ist fertig — seine Datei steht vollständig da"
    );

    app.wait_for_export_checks();
    let status = app.state.status.clone();
    println!("Statuszeile am Ende: {status}");
    println!("Warnungen: {:?}", app.state.warnings);

    // Das Orakel, nicht der Bericht des Programms.
    let bytes = std::fs::read(&out).unwrap();
    assert!(
        redact_pdf::leaks(&bytes, "GEHEIM-ZWEI").is_empty(),
        "GEHEIM-ZWEI deckt Klick 2 ab — die Datei trägt sonst den alten Stand"
    );
    assert!(
        redact_pdf::leaks(&bytes, "GEHEIM-EINS").is_empty(),
        "GEHEIM-EINS deckt schon Klick 1 ab"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Und die dritte Schreibweise derselben Datei: **ohne** Verzeichnis.
///
/// `out.pdf` hat als Elternteil den leeren Pfad, `./out.pdf` den Pfad `.` —
/// dieselbe Datei, und ohne den Zweig für den nackten Dateinamen zwei
/// Kennungen. `redact_pdf::document::check_target` setzt an derselben Stelle
/// genauso `"."` ein.
///
/// Mutation (den Zweig `(None, Some(name))` in [`writing_key`] entfernt, also
/// zurück auf `_ => path.to_path_buf()`): zwei Kennungen für eine Datei — rot.
#[test]
fn zh2_c_ohne_verzeichnis_ist_es_dieselbe_kennung() {
    assert_eq!(
        writing_key(std::path::Path::new("out.pdf")),
        writing_key(std::path::Path::new("./out.pdf")),
        "eine Datei, zwei Schreibweisen"
    );
}

// ===========================================================================
// 5 — Die Kennung der Warnung: gescheitert und geglückt müssen sich treffen
// ===========================================================================

/// **Die Unsymmetrie, die für die Belegt-Kennung schon gesehen war.**
///
/// Die Abbruchwarnung eines gescheiterten Exports hing an `file_key(&out)`,
/// und `file_key` kann den Pfad nicht auflösen, solange es die Datei nicht
/// gibt. Der nächste, **geglückte** Export derselben Datei trug unter dem
/// aufgelösten Pfad ein und strich sie darum nicht: „es wurde keine Datei
/// geschrieben“ stand neben der Erfolgsmeldung und dem sauberen Urteil.
///
/// Geschrieben wird der Pfad hier zweimal verschieden — einmal mit `..`
/// darin. Nur `.` fällt beim Vergleich von Pfaden weg, `..` nicht; für das
/// Dateisystem ist es dieselbe Datei.
///
/// Mutation (die Kennung der Abbruchwarnung zurück auf `file_key(&out)`):
/// die Warnung bleibt neben der Erfolgsmeldung stehen — rot.
#[test]
fn zh2_c_die_abbruchwarnung_verschwindet_beim_naechsten_geglueckten_export() {
    let dir = tmp("kennung");
    let ordner = dir.join("ordner");
    std::fs::create_dir_all(&ordner).unwrap();
    let gerade = ordner.join("out.pdf");
    let umweg = ordner.join("..").join("ordner").join("out.pdf");
    assert_ne!(gerade, umweg, "zwei Schreibweisen, eine Datei");

    let mut app = app_with(&ein_geheimnis());
    app.state
        .regions
        .push(text_region(0, ueber(700.0), "GEHEIM-EINS"));

    // Erst der gescheiterte Export — unter dem Pfad mit `..`.
    app.force_panic_in_export = true;
    app.export_to(umweg.clone());
    app.wait_for_export();
    assert_eq!(app.state.status, EXPORT_BROKEN, "{}", app.state.status);
    assert!(
        app.state.warnings.iter().any(|w| w.contains("abgebrochen")),
        "{:?}",
        app.state.warnings
    );
    assert!(!gerade.exists(), "geschrieben wurde nichts");

    // Dann der geglückte — dieselbe Datei, gerade geschrieben.
    app.force_panic_in_export = false;
    app.export_to(gerade.clone());
    app.wait_for_export_checks();
    println!("Statuszeile: {}", app.state.status);
    println!("Warnungen: {:?}", app.state.warnings);
    assert!(gerade.exists(), "jetzt steht die Datei da");
    assert!(
        !app.state.warnings.iter().any(|w| w.contains("abgebrochen")),
        "„es wurde keine Datei geschrieben“ neben der Erfolgsmeldung: {:?}",
        app.state.warnings
    );

    std::fs::remove_dir_all(&dir).ok();
}
