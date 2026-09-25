//! Gegenprüfung der Fix-Runde 8, Gebiet **Oberfläche** — Gegenprüfer B,
//! Runde 9. Linse: **die Gegenrichtung der Rückfrage vor dem Schließen.**
//!
//! Fix-Runde 8 hat `needs_close_confirmation` einen zweiten Grund gegeben:
//!
//! ```text
//! (has_manual_work || export_running) && !already_confirmed
//! ```
//!
//! Begründet ist er so (`app.rs`, Doc-Kommentar): „Geschrieben wird atomar, es
//! gibt also keine halbe Datei — wohl aber **keine**, während die Statuszeile
//! ‚Export läuft‘ sagte. Wer schließt, soll wissen, was er wegwirft."
//!
//! Der Aufrufort ist `RedactApp::handle_close_request`:
//!
//! ```text
//! needs_close_confirmation(
//!     self.state.has_manual_work(),
//!     self.export_running(),
//!     self.close_confirmed,
//! )
//! ```
//!
//! **`export_check_running()` kam darin nicht vor** — und das war die schwerere
//! Hälfte. Seit der Gegenprüfung 9 steht es da, und dieser Test hält den Grund
//! fest statt der Lücke. Im Fenster „Export fertig, Urteil noch unterwegs"
//! gilt:
//!
//! * die Ausgabedatei **steht auf der Platte** und trägt, wenn eine Schwärzung
//!   danebenging, den Klartext — das Orakel [`redact_pdf::leaks`] findet ihn;
//! * das Einzige, was das noch sagen würde, ist das Urteil der Nachprüfung,
//!   und das läuft auf einem Thread;
//! * die Statuszeile sagt in diesem Moment „Nachprüfung läuft …";
//! * ein Klick auf das Kreuz beendet den Prozess **ohne ein Wort**.
//!
//! Die beiden Fälle stehen damit genau verkehrt: gefragt wird, wenn nichts
//! entsteht (Export abgebrochen = keine Datei, kein Schaden), und **nicht**
//! gefragt, wenn eine Datei mit Klartext liegen bleibt und ihr einziger Zeuge
//! wegfällt.
//!
//! Der Maßstab ist nicht der Bericht der Oberfläche: gelesen werden die
//! geschriebenen Bytes (`redact_pdf::leaks`) und das Urteil, das
//! `ExportCheckPlan::run` daraus macht.
//!
//! `RedactApp::export_to` und `RedactApp::checks` sind privat, deshalb läuft
//! hier der öffentliche Weg, den `export_to` Schritt für Schritt geht:
//! `audit_target` → `hit_summary` → `plan_export_check` → `plan_export` →
//! `run_reporting` → `ExportCheckPlan::run`.
//!
//! ```text
//! flock /tmp/redactrs-cargo.lock cargo test -p redact-gui --test zm_b_schliessen_waehrend_der_pruefung -- --nocapture
//! ```

use std::path::{Path, PathBuf};

use redact_core::{Rect, Region, Source};
use redact_gui::app::needs_close_confirmation;
use redact_gui::state::{AnnotatedRegion, AppState};
use redact_gui::Config;
use redact_pdf::testing::{build_pdf, TextItem};

const GEHEIM: &str = "GEHEIM-EINS";

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zm-b-schliessen-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Ein Dokument mit `seiten` Seiten; auf jeder steht das Geheimnis **zweimal**
/// — oben und unten.
///
/// Zwei Vorkommen sind der Punkt: wird nur das obere abgedeckt, hat die
/// Schwärzung Zeichen entfernt (also **keine** Warnung „Deck-Rechteck
/// gezeichnet, kein Zeichen entfernt"), und der Klartext steht trotzdem noch
/// in der Datei. Genau dann ist das Urteil der Nachprüfung der **einzige**
/// Zeuge.
fn vorlage(seiten: usize) -> Vec<u8> {
    let pages: Vec<Vec<TextItem>> = (0..seiten)
        .map(|p| {
            vec![
                TextItem::new(72.0, 700.0, 10.0, format!("Seite {p} oben {GEHEIM}")),
                TextItem::new(72.0, 660.0, 10.0, format!("Seite {p} unten {GEHEIM}")),
            ]
        })
        .collect();
    build_pdf(&pages)
}

/// Ein Rechteck über der Zeile bei `y` — es trifft und entfernt Zeichen.
fn ueber(y: f64) -> Rect {
    Rect::new(60.0, y - 10.0, 520.0, y + 14.0)
}

/// Der Zustand mit den Rechtecken, die `rects` nennt — jedes trägt `GEHEIM`
/// als gefundenen Text, wie ein Treffer der Analyse.
fn geladen(dir: &Path, bytes: &[u8], rects: &[Rect]) -> AppState {
    let input = dir.join("eingang.pdf");
    std::fs::write(&input, bytes).unwrap();
    let mut state = AppState::with_config(Config {
        no_patterns: true,
        ..Config::default()
    });
    state.load_bytes(bytes, Some(input)).unwrap();
    for rect in rects {
        state.regions.push(AnnotatedRegion::new(Region::new(
            0,
            *rect,
            Some(GEHEIM.to_string()),
            Source::Manual {
                reason: "zm-b".into(),
            },
        )));
    }
    state
}

/// Exportiert auf dem öffentlichen Weg, den `RedactApp::export_to` Schritt für
/// Schritt geht, und gibt `(Warnungen des Exports, Urteil der Nachprüfung)`
/// zurück.
fn exportieren(state: &AppState, out: &Path) -> (Vec<String>, redact_gui::state::ExportCheck) {
    let audit = state.audit_target(out);
    let summary = state.hit_summary();
    let pruefplan = state.plan_export_check(&summary);
    let plan = state.plan_export(out, Some(&audit)).expect("Plan");
    let (geschrieben, ergebnis) = plan.run_reporting();
    assert!(geschrieben, "die Datei muss entstanden sein");
    let outcome = ergebnis.expect("der Lauf muss glücken");
    (outcome.warnings, pruefplan.run(out))
}

// ===========================================================================
// Das Material: eine Datei mit Klartext, deren einziger Zeuge noch unterwegs
// ist — und eine Oberfläche, die dabei ohne Rückfrage zugeht
// ===========================================================================

/// **BRICHT.** Der Zustand „Export fertig, Urteil noch unterwegs" ist für
/// `needs_close_confirmation` kein Grund zu fragen.
///
/// Aufgebaut wird genau die Lage, in der die Oberfläche steht, wenn
/// `poll_exports` den Export abgeholt und `start_export_check` den Prüf-Thread
/// gestartet hat: die Datei ist geschrieben, `export_running()` ist wieder
/// `false`, `export_check_running()` ist `true`. Handarbeit gibt es keine —
/// die Schwärzung kommt hier aus einer Analyse und nicht aus der Maus, und
/// selbst wenn: `has_manual_work` ist nicht die Frage.
///
/// Der Lauf zeigt drei Dinge:
///
/// 1. das Orakel findet den Klartext in den **geschriebenen** Bytes;
/// 2. das Urteil der Nachprüfung ist der Satz, der das sagt — und es ist ein
///    Satz, den bis dahin niemand gesehen hat (die Warnungen des Exports
///    selbst stehen daneben und werden mitgedruckt, damit die Frage „ist das
///    Urteil überhaupt der einzige Zeuge?" am Lauf und nicht an einer
///    Behauptung hängt);
/// 3. die Entscheidung fragt in genau diesem Fenster — was sie vor der
///    Gegenprüfung 9 nicht tat: `needs_close_confirmation(false, false,
///    false)` war `false`, das Fenster ging zu, und der Prozess nahm den
///    Prüf-Thread mit.
///
/// Jetzt lautet sie `(has_manual_work || export_running || check_running) &&
/// !already_confirmed`, und am Aufrufort steht `self.export_check_running()`
/// mit darin. Der Satz der Rückfrage ist ein anderer als
/// `ABANDON_EXPORT_QUESTION`: dort entsteht keine Datei, hier liegt eine und
/// niemand hat sie nachgesehen (`ABANDON_CHECK_QUESTION`).
///
/// Mutation, die diesen Test rot macht: `check_running` aus
/// `needs_close_confirmation` streichen.
#[test]
fn zm_b_schliessen_waehrend_die_nachpruefung_laeuft_fragt() {
    let dir = tmp("urteil-unterwegs");
    let bytes = vorlage(1);
    let out_leck = dir.join("leck.pdf");
    let out_sauber = dir.join("sauber.pdf");

    // Fall A: nur das obere Vorkommen ist abgedeckt — die Datei leckt.
    let leckt = geladen(&dir, &bytes, &[ueber(700.0)]);
    let (warn_leck, urteil_leck) = exportieren(&leckt, &out_leck);

    // Fall B: beide Vorkommen abgedeckt — dieselbe Vorlage, saubere Ausgabe.
    let sauber = geladen(&dir, &bytes, &[ueber(700.0), ueber(660.0)]);
    let (warn_sauber, urteil_sauber) = exportieren(&sauber, &out_sauber);

    // 1 — das Orakel über die geschriebenen Bytes, in beiden Fällen.
    let bytes_leck = std::fs::read(&out_leck).unwrap();
    let bytes_sauber = std::fs::read(&out_sauber).unwrap();
    let funde = redact_pdf::leaks(&bytes_leck, GEHEIM);
    println!(
        "Orakel leck.pdf:   {} Fundstelle(n) von {GEHEIM}",
        funde.len()
    );
    println!(
        "Orakel sauber.pdf: {} Fundstelle(n) von {GEHEIM}",
        redact_pdf::leaks(&bytes_sauber, GEHEIM).len()
    );
    assert!(!funde.is_empty(), "leck.pdf muss lecken");
    assert!(
        redact_pdf::leaks(&bytes_sauber, GEHEIM).is_empty(),
        "sauber.pdf darf nicht lecken — sonst unterscheidet dieser Test nichts"
    );

    // 2 — die Warnungen des **Exports** sind in beiden Fällen dieselben. Sie
    //     können also nicht der Zeuge sein, der Leck von Nicht-Leck trennt.
    println!("Warnungen des Exports (leck):   {warn_leck:?}");
    println!("Warnungen des Exports (sauber): {warn_sauber:?}");
    assert_eq!(
        warn_leck, warn_sauber,
        "wären sie verschieden, sagte schon der Export etwas über das Leck"
    );

    // 3 — nur das Urteil der Nachprüfung trennt die beiden Dateien.
    println!("Urteil leck.pdf:   {}", urteil_leck.sentence());
    println!("Urteil sauber.pdf: {}", urteil_sauber.sentence());
    let warnung = urteil_leck.warning();
    println!("Warnung des Urteils (leck): {warnung:?}");
    assert!(
        warnung.is_some(),
        "das Urteil muss das Leck melden, sonst ist es kein Zeuge"
    );
    assert!(
        urteil_sauber.warning().is_none(),
        "die saubere Datei hat keine Warnung: {:?}",
        urteil_sauber.warning()
    );

    // 4 — und die Entscheidung der Oberfläche in genau diesem Fenster.
    // Wörtlich der Aufruf aus `RedactApp::handle_close_request`, mit den
    // Werten, die dort gelten: keine Handarbeit (die Treffer kommen aus der
    // Analyse, nicht aus der Maus), kein laufender Export (`poll_exports` hat
    // ihn abgeholt und `start_export_check` die Prüfung angestoßen), noch
    // nicht bestätigt.
    let gefragt = needs_close_confirmation(
        /* has_manual_work    */ false, /* export_running     */ false,
        /* check_running      */ true, /* already_confirmed  */ false,
    );
    println!("needs_close_confirmation(false, false, true, false) = {gefragt}");
    assert!(
        gefragt,
        "leck.pdf liegt mit Klartext auf der Platte, ihr einziger Zeuge ist noch unterwegs — \
         und das Fenster geht ohne ein Wort zu. Richtig wäre: solange ein Urteil aussteht \
         (export_check_running()), muss gefragt werden; gefragt wird bisher nur für den \
         laufenden Export, also für den Fall, in dem GAR KEINE Datei entsteht."
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Die Breite des Fensters, in dem das gilt — gemessen, nicht geschätzt.
///
/// Das Urteil ist so lange unterwegs, wie `ExportCheckPlan::run` über der
/// geschriebenen Datei braucht. Gemessen wird das an einer Vorlage in der
/// Größenordnung, für die die Doku selbst Zahlen nennt (305 Seiten, rund 1 s
/// bei 1 000 Begriffen), und daneben der gewöhnliche Fall mit den Begriffen,
/// die eine Analyse wirklich hergibt.
///
/// Ist das Fenster Sekunden breit, ist ein Klick auf das Kreuz darin kein
/// ausgedachter Fall: die Statuszeile sagt genau in dieser Zeit „Nachprüfung
/// läuft …", und wer fertig ist, schließt.
#[test]
#[ignore = "Messung: wie lange ein Urteil unterwegs ist"]
fn zm_b_mess_wie_breit_das_fenster_ist() {
    let dir = tmp("fensterbreite");
    let seiten = 305;
    let bytes = vorlage(seiten);
    let state = geladen(&dir, &bytes, &[ueber(700.0)]);
    let out = dir.join("out.pdf");

    let audit = state.audit_target(&out);
    let summary = state.hit_summary();
    let pruefplan = state.plan_export_check(&summary);
    let plan = state.plan_export(&out, Some(&audit)).expect("Plan");
    let (geschrieben, ergebnis) = plan.run_reporting();
    assert!(geschrieben);
    ergebnis.expect("Lauf");
    println!(
        "Vorlage {seiten} Seiten ({} kB), Ausgabe {} kB, Begriffe im Plan: {}",
        bytes.len() / 1024,
        std::fs::metadata(&out).unwrap().len() / 1024,
        pruefplan.needles.len()
    );

    let t0 = std::time::Instant::now();
    let urteil = pruefplan.clone().run(&out);
    let gewoehnlich = t0.elapsed();
    println!(
        "Urteil mit {} Begriffen: {gewoehnlich:?} — {}",
        pruefplan.needles.len(),
        urteil.sentence()
    );

    // Und am oberen Rand der Decke, wie in der Doku von `start_export_check`.
    let viele: Vec<String> = (0..redact_core::MAX_CHECK_NEEDLES)
        .map(|i| format!("GEHEIM-{i:04}"))
        .collect();
    let voll = redact_gui::state::ExportCheckPlan {
        kept_forms: vec![false; viele.len()],
        kept_literal: vec![false; viele.len()],
        needles: viele,
        ..redact_gui::state::ExportCheckPlan::default()
    };
    let anzahl = voll.needles.len();
    let t0 = std::time::Instant::now();
    let _ = voll.run(&out);
    let am_rand = t0.elapsed();
    println!("Urteil mit {anzahl} Begriffen (Decke): {am_rand:?}");
    println!(
        "So lange sagt die Statuszeile „Nachprüfung läuft …\" — und so lange geht das Fenster \
         ohne Rückfrage zu."
    );

    std::fs::remove_dir_all(&dir).ok();
}
