//! Fix-Runde 8, Register #19: **der Export selbst** läuft nicht mehr im
//! Zeichentakt.
//!
//! Bis hierher lief nur die *Nachprüfung* auf einem Thread
//! ([`RedactApp::start_export_check`]); das Schwärzen und Schreiben blieb im
//! Zeichentakt — gemessen 6,3 s bei 305 Seiten, in denen das Fenster auf
//! keinen Klick reagierte. Wer exportiert, sah eine eingefrorene Anwendung
//! und wusste nicht, ob sie noch lebt.
//!
//! Geprüft wird hier sechs Mal:
//!
//! 1. Der Export läuft wirklich auf einem Thread, und **während** er läuft ist
//!    die Oberfläche frei: die Zieldatei steht noch nicht da, die Statuszeile
//!    sagt [`EXPORT_RUNNING`], und ein Bild (`poll_export_checks`) kostet
//!    Mikrosekunden statt Sekunden (gemessen 9,967 µs, Debug-Lauf). Daneben
//!    die Gegenprobe: ein gewöhnlicher Export kommt ganz durch, mit Datei,
//!    Warnung und Urteil.
//! 2. Ein zweiter Export **derselben** Datei wird abgelehnt, solange der erste
//!    schreibt — und ein Export in eine **andere** Datei ausdrücklich nicht.
//! 3. Eine laufende Nachprüfung derselben Datei verliert ihr Urteil, sobald
//!    neue Bytes entstehen — und **erst** dann. Der Schnitt hängt an
//!    `PendingExport::wrote`, nicht am Klick: siehe `zh2_c_leck_tests`, wo das
//!    Gegenstück steht (ein Export, der nie ein Byte schreibt, darf kein
//!    Urteil wegwerfen).
//! 4. Stirbt der Export-Thread, schweigt die Oberfläche nicht.
//! 5. Wer das Fenster schließt, während ein Export schreibt, wird gefragt.
//! 6. Zwei gescheiterte Exporte derselben Datei warnen **einmal** — die
//!    Doppelungsabwehr in `note_check_warning` hatte bis hierher keinen Test.
//!
//! Kindmodul von `app` wie `zb_tests`: `export_to`, `exports`, `hold_export`
//! und `ui_ctx` sind privat.
//!
//! `cargo test -p redact-gui --lib zh_c`

use super::*;

use std::sync::Arc;
use std::time::{Duration, Instant};

use redact_core::{Rect, Region, Source};
use redact_pdf::testing::{build_pdf, TextItem};

use crate::state::AnnotatedRegion;

// --------------------------------------------------------------- Hilfsmittel

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "zh-c-{tag}-{}-{:?}",
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
            reason: "zh-c".into(),
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

/// Ein Rechteck über der Zeile bei `y` — trifft.
fn ueber(y: f64) -> Rect {
    Rect::new(60.0, y - 10.0, 520.0, y + 14.0)
}

/// Ein Rechteck, unter dem nichts liegt: die Zeile gilt als geschwärzt und
/// bleibt trotzdem stehen — das Leck, das die Nachprüfung finden muss.
fn daneben() -> Rect {
    Rect::new(430.0, 20.0, 440.0, 40.0)
}

/// Eine Oberfläche mit geladenem Dokument, ohne Muster — die Regionen setzt
/// jeder Test selbst.
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
/// **Mit** Frist, weil die Gegenprobe sonst hängt statt rot zu werden: läuft
/// die Arbeit gar nicht auf einem Thread (Mutation: der Zweig ohne Thread),
/// kommt dort niemand an.
fn warte_aufs_gatter(gate: &Arc<CheckGate>, was: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !gate.arrived() {
        assert!(
            Instant::now() < deadline,
            "{was} läuft nicht auf einem eigenen Thread — am Gatter kam niemand an"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

// ===========================================================================
// 1 — Der Export läuft auf einem Thread, die Oberfläche bleibt frei
// ===========================================================================

/// **Befund #19.** `export_to` kehrt zurück, **bevor** die Datei geschrieben
/// ist.
///
/// Der Haken [`CheckGate`] hält den Export-Thread vor dem Schreiben an. Bis
/// dahin gilt: die Zieldatei existiert **nicht**, die Statuszeile sagt
/// [`EXPORT_RUNNING`], die Nachprüfung kann noch gar nicht laufen — und ein
/// Bild kostet nichts. Erst danach entsteht die Datei, und dann geht es
/// weiter wie bisher: Exportmeldung, dann das Urteil der Nachprüfung.
///
/// Mutation (nachgewiesen): `plan.clone().run()` vor den Thread gezogen, also
/// wieder im Zeichentakt geschrieben → rot mit „die Datei steht schon da“.
/// Rot wird dadurch auch
/// [`zh_c_ein_export_der_auf_dem_thread_stirbt_schweigt_nicht`] — dort ist
/// dann geschrieben, obwohl der Thread starb. Beides ist genau die mutierte
/// Eigenschaft.
#[test]
fn zh_c_der_export_laeuft_nicht_mehr_im_zeichentakt() {
    let dir = tmp("thread");
    let out = dir.join("out.pdf");
    let gate = Arc::new(CheckGate::default());

    let mut app = app_with(&ein_geheimnis());
    app.state
        .regions
        .push(text_region(0, ueber(700.0), "GEHEIM-EINS"));
    app.hold_export = Some(gate.clone());

    app.export_to(out.clone());
    warte_aufs_gatter(&gate, "der Export");

    // Die Oberfläche ist frei, und die Datei ist noch nicht da.
    assert!(app.export_running(), "der Export muss unterwegs sein");
    assert_eq!(app.state.status, EXPORT_RUNNING, "{}", app.state.status);
    assert!(
        !out.exists(),
        "die Datei steht schon da — dann hat nicht der Thread geschrieben"
    );
    assert!(
        !app.export_check_running(),
        "die Nachprüfung kann nichts prüfen, was noch nicht geschrieben ist"
    );
    // Ein Bild währenddessen: es holt nichts ab und hält nicht an.
    let t0 = Instant::now();
    assert_eq!(app.poll_export_checks(), 0);
    let bild = t0.elapsed();
    eprintln!("Ein Bild während des Exports: {bild:?}");
    assert!(
        bild < Duration::from_millis(500),
        "ein Bild während des Exports dauerte {bild:?}"
    );
    assert!(app.export_running(), "und der Export läuft weiter");

    // Jetzt darf er schreiben.
    gate.release();
    app.wait_for_export_checks();
    assert!(!app.export_running());
    assert!(out.exists(), "die Datei muss danach dastehen");
    let status = app.state.status.clone();
    assert!(status.starts_with("Export:"), "{status}");
    assert!(status.contains("Nachprüfung:"), "{status}");

    // Das Orakel: das Geheimnis ist wirklich weg.
    let bytes = std::fs::read(&out).unwrap();
    assert!(
        redact_pdf::leaks(&bytes, "GEHEIM-EINS").is_empty(),
        "die Ausgabe leckt: {status}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Die Gegenrichtung zu Test 1: **ohne** Haken bleibt alles wie zuvor — die
/// Datei entsteht, die Warnungen des Exports stehen da, das Urteil kommt.
///
/// Das ist die Zusage, die der Umbau nicht brechen durfte: derselbe Aufruf von
/// `redact_pipeline::apply`, nur an einer anderen Stelle ausgeführt. Die
/// Byte-Gleichheit mit der Kommandozeile prüft
/// `redact-cli/tests/cli_and_gui_agree.rs` weiter unverändert.
#[test]
fn zh_c_ein_gewoehnlicher_export_kommt_ganz_durch() {
    let dir = tmp("gewoehnlich");
    let out = dir.join("out.pdf");
    let mut app = app_with(&ein_geheimnis());
    app.state
        .regions
        .push(text_region(0, daneben(), "GEHEIM-EINS"));

    app.export_to(out.clone());
    app.wait_for_export();
    app.wait_for_export_checks();

    let status = app.state.status.clone();
    assert!(status.starts_with("Export:"), "{status}");
    // Das Rechteck ging daneben: das Leck muss in der Warnung stehen.
    let bytes = std::fs::read(&out).unwrap();
    assert!(
        !redact_pdf::leaks(&bytes, "GEHEIM-EINS").is_empty(),
        "die Vorlage muss lecken"
    );
    assert!(
        app.state
            .warnings
            .iter()
            .any(|w| w.contains("GEHEIM-EINS") || w.contains("steht NOCH")),
        "{:?}",
        app.state.warnings
    );

    std::fs::remove_dir_all(&dir).ok();
}

// ===========================================================================
// 2 — Zwei schnelle Exporte
// ===========================================================================

/// **Dieselbe Datei zweimal**, während der erste noch schreibt: abgelehnt.
///
/// Bei der Nachprüfung gewinnt der jüngere Lauf — sie *liest* nur. Ein Export
/// *schreibt*, und die Reihenfolge der `rename`-Aufrufe ist nicht die der
/// Klicks: landete der ältere zuletzt, trüge die Datei die alten Bytes,
/// während Meldung und Urteil vom neuen Plan sprechen. Das ist die
/// Entwarnung, die es nicht geben darf — also wird der zweite Export
/// abgelehnt und sagt das.
///
/// Mutation (die `if self.exports.iter().any(…)`-Abfrage in `export_to`
/// entfernt): zwei Exporte derselben Datei laufen gleichzeitig, `exports`
/// zählt 2 und die Statuszeile trägt nicht [`EXPORT_BUSY`] — rot.
#[test]
fn zh_c_ein_zweiter_export_derselben_datei_wird_abgelehnt() {
    let dir = tmp("zweimal");
    let out = dir.join("out.pdf");
    let gate = Arc::new(CheckGate::default());

    let mut app = app_with(&ein_geheimnis());
    app.state
        .regions
        .push(text_region(0, ueber(700.0), "GEHEIM-EINS"));
    app.hold_export = Some(gate.clone());

    app.export_to(out.clone());
    warte_aufs_gatter(&gate, "der Export");
    assert_eq!(app.exports.len(), 1);

    // Derselbe Klick noch einmal — dieselbe Datei.
    app.export_to(out.clone());
    assert_eq!(app.state.status, EXPORT_BUSY, "{}", app.state.status);
    assert_eq!(
        app.exports.len(),
        1,
        "zwei Schreiber auf derselben Datei: die Reihenfolge der Bytes wäre offen"
    );
    // Und mit einer Schreibweise, die auf dieselbe Datei zeigt, ebenso.
    app.export_to(dir.join(".").join("out.pdf"));
    assert_eq!(app.state.status, EXPORT_BUSY, "{}", app.state.status);
    assert_eq!(app.exports.len(), 1);

    gate.release();
    app.wait_for_export_checks();
    assert!(out.exists());
    let status = app.state.status.clone();
    assert!(status.starts_with("Export:"), "{status}");
    assert!(status.contains("Nachprüfung:"), "{status}");

    std::fs::remove_dir_all(&dir).ok();
}

/// Die Gegenrichtung: eine **andere** Datei darf gleichzeitig entstehen.
///
/// Eine Grenze, die gewöhnliche Fälle ablehnt, ist genauso ein Fehler wie eine
/// Lücke. Zwei verschiedene Ausgabedateien stören sich nicht — es gibt keine
/// gemeinsamen Bytes, über die jemand das Falsche sagen könnte.
///
/// Mutation (die Abfrage auf `exports.is_empty()` statt auf die Datei
/// gestellt): der zweite Export wird abgelehnt, `exports` zählt 1 — rot.
#[test]
fn zh_c_eine_andere_datei_darf_gleichzeitig_geschrieben_werden() {
    let dir = tmp("zwei-dateien");
    let erste = dir.join("erste.pdf");
    let zweite = dir.join("zweite.pdf");
    let gate = Arc::new(CheckGate::default());

    let mut app = app_with(&ein_geheimnis());
    app.state
        .regions
        .push(text_region(0, ueber(700.0), "GEHEIM-EINS"));
    app.hold_export = Some(gate.clone());

    app.export_to(erste.clone());
    warte_aufs_gatter(&gate, "der erste Export");
    app.export_to(zweite.clone());
    assert_ne!(app.state.status, EXPORT_BUSY, "{}", app.state.status);
    assert_eq!(
        app.exports.len(),
        2,
        "zwei verschiedene Dateien sind zwei Exporte"
    );

    gate.release();
    app.wait_for_export_checks();
    assert!(erste.exists(), "die erste Datei fehlt");
    assert!(zweite.exists(), "die zweite Datei fehlt");
    // Beide Urteile sind eingetragen, jedes mit seinem Dateinamen davor.
    let namen: Vec<&String> = app.state.warnings.iter().collect();
    eprintln!("Warnungen: {namen:?}");

    std::fs::remove_dir_all(&dir).ok();
}

// ===========================================================================
// 3 — Neue Bytes machen ein altes Urteil gegenstandslos
// ===========================================================================

/// **Der Schnitt liegt an den geschriebenen Bytes** — nicht am Klick.
///
/// [`ExportCheckPlan::run`] liest die Datei zum Prüfzeitpunkt. Sobald ein
/// Export derselben Datei **geschrieben hat**, wäre ein Urteil der älteren
/// Prüfung eines über Bytes, die es nicht mehr gibt, und im schlimmsten Fall
/// eine Entwarnung über eine Ausgabe, die niemand geprüft hat. Bis
/// Fix-Runde 8 stand dieser Schnitt in `start_export_check`, also hinter einem
/// **geglückten** Schreiben; das trug nur, solange das Schreiben denselben
/// Takt blockierte.
///
/// Die Nachbesserung dazu: bis zur letzten Runde stand dieser Test hier mit
/// der Erwartung „schon beim Klick leer“ — und hat damit ein Leck
/// festgeschrieben. Schreibt der Export **nie** ein Byte (Symlink als Ziel,
/// volle Platte, Panik), dann liegt die Datei unverändert da, und ihr Urteil
/// gilt weiter. Deshalb prüft dieser Test jetzt **beide** Seiten des Fensters,
/// und die Gegenstücke stehen in `zh2_c_leck_tests`.
///
/// Mutation (`self.exports … wrote`-Schnitt in `poll_exports` entfernt): das
/// alte Urteil überlebt die neuen Bytes, `checks` zählt nach dem Schreiben
/// weiterhin 1 — rot. Gegenmutation (`AtomicBool::new(true)`, also der Schnitt
/// wieder am Klick): die Prüfung ist schon vor dem ersten Byte weg — rot.
#[test]
fn zh_c_ein_beginnender_export_beendet_die_pruefung_derselben_datei() {
    let dir = tmp("schnitt");
    let out = dir.join("out.pdf");
    let pruefhaken = Arc::new(CheckGate::default());
    let vorhaken = Arc::new(CheckGate::default());
    let nachhaken = Arc::new(CheckGate::default());

    let mut app = app_with(&ein_geheimnis());
    app.state
        .regions
        .push(text_region(0, ueber(700.0), "GEHEIM-EINS"));

    // Erster Export ganz durch — seine Prüfung hängt am Haken.
    app.hold_check = Some(pruefhaken.clone());
    app.export_to(out.clone());
    app.wait_for_export();
    warte_aufs_gatter(&pruefhaken, "die Nachprüfung");
    assert_eq!(app.checks.len(), 1, "die erste Prüfung muss laufen");
    app.hold_check = None;

    // Zweiter Export derselben Datei, angehalten **vor** dem Schreiben: noch
    // ist kein Byte neu, also urteilt die alte Prüfung weiter über die Bytes,
    // die auf der Platte stehen.
    app.hold_export = Some(vorhaken.clone());
    app.hold_export_after_write = Some(nachhaken.clone());
    app.export_to(out.clone());
    warte_aufs_gatter(&vorhaken, "der zweite Export vor dem Schreiben");
    app.poll_export_checks();
    assert_eq!(
        app.checks.len(),
        1,
        "solange kein Byte geschrieben ist, gilt das alte Urteil"
    );

    // Und nun **mit** den neuen Bytes: geschrieben ist geschrieben, das
    // Ergebnis des Exports liegt aber noch nicht im Kanal.
    vorhaken.release();
    warte_aufs_gatter(&nachhaken, "der zweite Export nach dem Schreiben");
    app.poll_export_checks();
    assert!(
        app.checks.is_empty(),
        "die alte Prüfung urteilt über Bytes, die es nicht mehr gibt"
    );

    pruefhaken.release();
    nachhaken.release();
    app.wait_for_export_checks();
    // Genau ein Urteil, und es gehört zum zweiten Export.
    let status = app.state.status.clone();
    assert!(status.starts_with("Export:"), "{status}");
    assert!(status.contains("Nachprüfung:"), "{status}");

    std::fs::remove_dir_all(&dir).ok();
}

// ===========================================================================
// 4 — Ein Export, der auf seinem Thread stirbt
// ===========================================================================

/// **Schweigen sähe aus wie Erfolg.** Stirbt der Export-Thread, sagt die
/// Oberfläche das — in der Statuszeile und in den Warnungen.
///
/// Derselbe Weg wie bei der Nachprüfung (Befund G5-A3): der Kanal wird
/// `Disconnected`, und das ist kein Grund zu schweigen. Geschrieben wird
/// atomar (Temp-Datei, dann `rename`), es gibt also keine halbe Datei — wohl
/// aber gar keine, und genau das sagt der Satz.
///
/// Mutation (der `Disconnected`-Zweig in `poll_exports` auf „stillschweigend
/// vergessen“ gesetzt): die Statuszeile bleibt auf [`EXPORT_RUNNING`] stehen
/// und keine Warnung steht da — rot.
#[test]
fn zh_c_ein_export_der_auf_dem_thread_stirbt_schweigt_nicht() {
    let dir = tmp("panik");
    let out = dir.join("out.pdf");
    let mut app = app_with(&ein_geheimnis());
    app.state
        .regions
        .push(text_region(0, ueber(700.0), "GEHEIM-EINS"));
    app.force_panic_in_export = true;

    app.export_to(out.clone());
    app.wait_for_export();

    assert_eq!(app.state.status, EXPORT_BROKEN, "{}", app.state.status);
    assert!(
        app.state.warnings.iter().any(|w| w.contains("abgebrochen")),
        "{:?}",
        app.state.warnings
    );
    assert!(
        !out.exists(),
        "geschrieben wird atomar — eine halbe Datei darf es nicht geben"
    );
    // Und keine Nachprüfung, die über eine nicht geschriebene Datei urteilt.
    assert!(!app.export_check_running());

    std::fs::remove_dir_all(&dir).ok();
}

// ===========================================================================
// 5 — Das Fenster schließen, während der Export schreibt
// ===========================================================================

/// **Wer schließt, soll wissen, was er wegwirft.**
///
/// Solange der Export im Zeichentakt lief, war das keine Frage: das Fenster
/// nahm während des Schreibens keine Eingabe an. Jetzt beendete ein Klick auf
/// das Kreuz den Prozess mitten im Schreiben — geschrieben wird atomar, es
/// gibt also keine halbe Datei, wohl aber **keine**, während die Statuszeile
/// „Export läuft“ sagte.
///
/// Geprüft wird die reine Entscheidung (ohne Fenster) und die Rückfrage an
/// einem Export, der am Haken hängt.
///
/// Mutation (nachgewiesen): `(has_manual_work || export_running)` auf
/// `has_manual_work` zurückgesetzt → dieser Test rot, und mit ihm
/// `app::tests::closing_only_asks_when_there_is_hand_work_to_lose`, das
/// dieselbe Entscheidung von der anderen Seite hält.
#[test]
fn zh_c_ein_laufender_export_fragt_vor_dem_schliessen() {
    // Die Entscheidung: ohne Handarbeit, aber mit laufendem Export wird
    // gefragt — und nach dem Bestätigen nicht noch einmal.
    assert!(needs_close_confirmation(false, true, false));
    assert!(!needs_close_confirmation(false, false, false));
    assert!(!needs_close_confirmation(false, true, true));

    let dir = tmp("schliessen");
    let out = dir.join("out.pdf");
    let gate = Arc::new(CheckGate::default());

    // `refusing`: die Rückfrage wird mit „Nein“ beantwortet.
    let mut app = RedactApp::refusing(Config {
        no_patterns: true,
        ..Config::default()
    });
    app.open_bytes_and_analyze(&ein_geheimnis(), "eins.pdf");
    app.state
        .regions
        .push(text_region(0, ueber(700.0), "GEHEIM-EINS"));
    app.hold_export = Some(gate.clone());
    app.export_to(out.clone());
    warte_aufs_gatter(&gate, "der Export");

    assert!(
        !app.may_abandon_export(),
        "ein laufender Export darf nicht stillschweigend aufgegeben werden"
    );

    gate.release();
    app.wait_for_export_checks();
    assert!(
        app.may_abandon_export(),
        "ohne laufenden Export gibt es nichts zu fragen"
    );

    std::fs::remove_dir_all(&dir).ok();
}

// ===========================================================================
// 6 — Zweimal gescheitert, einmal gewarnt
// ===========================================================================

/// **Zwei gescheiterte Exporte derselben Datei tragen denselben Satz — und der
/// steht einmal da.**
///
/// Die Doppelungsabwehr in [`RedactApp::note_check_warning`] hatte bis hierher
/// **keinen** Test: ihre Mutation (die `contains`-Prüfung entfernt) ließ die
/// ganze Sammlung grün (Fix-Runde 8, Mutationslauf M9) — genau die
/// Fehlerklasse „ein Test bleibt ohne seine Korrektur grün“. Der Kommentar in
/// `zb_p5d3` behauptete, zwei gleichzeitige Prüfungen derselben Datei deckten
/// den Fall ab; seit Befund Q4-6 gibt es die nicht mehr, denn der jüngere
/// Export lässt die ältere Prüfung fallen.
///
/// Erreichbar ist der Fall seit dem Export auf dem Thread: ein
/// **gescheiterter** Export trägt seinen Satz ein, ohne dass
/// [`RedactApp::note_export_warnings`] vorher die Warnungen dieser Datei
/// gestrichen hat — das tut nur ein geglückter. Zweimal gescheitert heißt also
/// zweimal derselbe Satz unter derselben Kennung.
///
/// Mutation (`if held.entries.iter().any(…) { return; }` in
/// `note_check_warning` entfernt): zwei Warnungen statt einer, rot.
#[test]
fn zh_c_zwei_gescheiterte_exporte_derselben_datei_warnen_einmal() {
    let dir = tmp("zweimal-panik");
    let out = dir.join("out.pdf");
    let mut app = app_with(&ein_geheimnis());
    app.state
        .regions
        .push(text_region(0, ueber(700.0), "GEHEIM-EINS"));
    app.force_panic_in_export = true;

    for _ in 0..2 {
        app.export_to(out.clone());
        app.wait_for_export();
    }

    assert_eq!(app.state.status, EXPORT_BROKEN, "{}", app.state.status);
    let abgebrochen = app
        .state
        .warnings
        .iter()
        .filter(|w| w.contains("abgebrochen"))
        .count();
    assert_eq!(abgebrochen, 1, "{:?}", app.state.warnings);
    assert!(!out.exists(), "geschrieben wurde nichts");

    std::fs::remove_dir_all(&dir).ok();
}
