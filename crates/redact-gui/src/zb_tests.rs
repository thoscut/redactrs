//! Prüfrunde Z-B — **die Oberfläche darf nie auf eine Datei warten.**
//!
//! Kindmodul von [`crate::app`], damit `export_to`, `resize`, `apply_pointer`
//! und `handle_keys` unmittelbar erreichbar sind.
//!
//! Sieben Befunde, jeder mit einem Test, der ohne die Korrektur rot ist:
//!
//! 1. Die Nachprüfung nach dem Export lief im Zeichentakt — 305 Seiten mit
//!    200 Begriffen hielten das Fenster an. Jetzt läuft sie auf einem eigenen
//!    Thread ([`RedactApp::start_export_check`]), in **einem** Durchgang
//!    ([`redact_pdf::leaks_many`]).
//! 2. Kleinbilder wurden nie verworfen. Jetzt gilt eine Byte-Decke
//!    ([`crate::render::MAX_THUMB_BYTES`]), und die Miniaturspalte fordert
//!    nur an, was sichtbar ist.
//! 3. Die Trefferliste baute jede Zeile in jedem Bild. Jetzt nur die
//!    sichtbaren ([`egui::ScrollArea::show_rows`]).
//! 4. Die Decke der Nachprüfung zählte erst Begriffe (200), dann Begriffe ×
//!    Dateibytes auf der Platte — die falsche Einheit, die Kosten hängen an
//!    den **entpackten** Streambytes, und die kennt vor dem Lauf niemand.
//!    Jetzt zählt sie Begriffe mit derselben Zahl wie `--check-leaks`
//!    ([`redact_core::MAX_CHECK_NEEDLES`]); die Bytes deckelt
//!    `--max-decompressed-mb`.
//! 5. Ein abgewählter Text galt als Leck. Jetzt als Entscheidung
//!    ([`crate::state::ExportCheck::kept`]).
//! 6. Eine Pfeiltaste während eines Griff-Zugs wurde vom Zug überschrieben.
//!    Jetzt beendet sie ihn ([`RedactApp::end_handle_drag`]).
//! 7. Mit gehaltener Strg taten Esc, Entf, Bild auf/ab, Pos1 und Ende nichts.
//!    Jetzt gelten sie mit und ohne Strg ([`key_commands`]).
//!
//! Belege über die geschriebene Datei gehen ausschließlich über
//! [`redact_pdf::leaks`] — das ehrliche Orakel, nie der eigene Extraktor.
//! Die Messungen (`zb_mess_*`) sind `#[ignore]` und laufen mit
//! `cargo test -p redact-gui zb_mess -- --ignored --nocapture`.

use super::*;

use std::cell::RefCell;
use std::sync::Arc;
use std::time::{Duration, Instant};

use redact_core::{MatchType, Rect, Region, Source};
use redact_pdf::testing::{build_pdf, TextItem};

use crate::render::{MAX_THUMB_BYTES, THUMB_WIDTH};
use crate::state::{selection_on_missing_page, AnnotatedRegion, HitOutcome, NudgeKind};
use crate::viewer::PageView;
use redact_core::MAX_CHECK_NEEDLES;

// --------------------------------------------------------------- Hilfsmittel

fn iban_only() -> Config {
    Config {
        patterns: vec!["iban_de".to_string()],
        ..Config::default()
    }
}

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "zb-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn demo_app() -> RedactApp {
    let mut app = RedactApp::silent(iban_only());
    app.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "demo.pdf");
    assert!(app.state.is_loaded());
    app
}

/// Ein Dokument mit `pages` einfachen Seiten, jede mit `lines` Zeilen; Zeile
/// `l` auf Seite `p` trägt `terms[(p * lines + l) % terms.len()]`.
fn many_pages(pages: usize, lines: usize, terms: &[String]) -> Vec<u8> {
    let sheets: Vec<Vec<TextItem>> = (0..pages)
        .map(|p| {
            (0..lines)
                .map(|l| {
                    let term = &terms[(p * lines + l) % terms.len()];
                    TextItem::new(
                        72.0,
                        800.0 - l as f64 * 18.0,
                        10.0,
                        format!("Zeile {l} auf Seite {p}: {term} Betrag 12,34 EUR"),
                    )
                })
                .collect()
        })
        .collect();
    build_pdf(&sheets)
}

fn terms(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("Begriff{i:04}Xy")).collect()
}

/// Eine Zeile mit bekanntem Text, wie sie aus einer Review-Datei käme.
fn text_region(page: usize, rect: Rect, text: &str) -> AnnotatedRegion {
    AnnotatedRegion::new(Region::new(
        page,
        rect,
        Some(text.to_string()),
        Source::Manual {
            reason: "Z-B".into(),
        },
    ))
}

fn keys(f: impl FnOnce(&mut KeyState)) -> KeyState {
    let mut k = KeyState::default();
    f(&mut k);
    k
}

fn press(app: &mut RedactApp, k: KeyState) {
    let commands = key_commands(k, app.state.selected_region.is_some());
    app.apply_key_commands(&commands);
}

const ORIGIN: Pos2 = Pos2::new(30.0, 40.0);
const ZOOM: f32 = 1.0;

fn press_frame(p: Pos2) -> PointerFrame {
    PointerFrame {
        pressed: true,
        down: true,
        pos: Some(p),
        press_origin: Some(p),
        ..PointerFrame::default()
    }
}

fn dragging_frame(press: Pos2, pos: Pos2) -> PointerFrame {
    PointerFrame {
        dragged: true,
        down: true,
        pos: Some(pos),
        press_origin: Some(press),
        ..PointerFrame::default()
    }
}

fn release_frame(pos: Pos2) -> PointerFrame {
    PointerFrame {
        drag_stopped: true,
        pos: Some(pos),
        ..PointerFrame::default()
    }
}

/// Holt Ergebnisse ab, bis `ready` gilt.
fn wait_for(cache: &mut PageCache, ctx: &egui::Context, ready: impl Fn(&PageCache) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while !ready(cache) {
        assert!(Instant::now() < deadline, "Rendern dauert zu lang");
        cache.poll(ctx);
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Bytes aller gehaltenen Kleinbilder, gerechnet aus den Texturen selbst —
/// derselbe Zähler funktioniert vor und nach der Decke.
fn thumb_texture_bytes(cache: &PageCache, pages: usize) -> usize {
    (0..pages)
        .filter_map(|p| cache.thumb(p))
        .map(|t| {
            let [w, h] = t.size();
            w * h * 4
        })
        .sum()
}

fn screen(width: f32, height: f32) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            Pos2::ZERO,
            Vec2::new(width, height),
        )),
        ..Default::default()
    }
}

// ===========================================================================
// 1 — Die Nachprüfung läuft nicht im Zeichentakt
// ===========================================================================

/// **Befund 1, behoben.** `export_to` kehrt zurück, **bevor** die Datei
/// durchsucht ist: die Statuszeile sagt „läuft“, das Urteil kommt über den
/// Kanal. Vorher stand das Urteil sofort da — und das Fenster so lange still.
#[test]
fn zb1_die_nachpruefung_haelt_die_oberflaeche_nicht_an() {
    let out = tmp("zb1").join("out.pdf");
    let mut app = demo_app();

    app.export_to(out.clone());
    app.wait_for_export();

    // Kein Abholen dazwischen: was hier steht, hat `export_to` selbst gesetzt.
    assert!(
        app.export_check_running(),
        "die Prüfung muss unterwegs sein, nicht fertig"
    );
    let running = app.state.status.clone();
    assert!(
        running.contains(EXPORT_CHECK_RUNNING),
        "Statuszeile während der Prüfung: {running}"
    );
    assert!(
        !running.contains("Nachprüfung:"),
        "ein Urteil vor dem Lauf: {running}"
    );
    assert!(running.starts_with("Export:"), "{running}");

    app.wait_for_export_checks();
    assert!(!app.export_check_running());
    let verdict = app.state.status.clone();
    assert!(!verdict.contains(EXPORT_CHECK_RUNNING), "{verdict}");
    assert!(
        verdict.contains("Nachprüfung: 2 gesuchte Text(e) stehen nicht mehr in der Ausgabe."),
        "Urteil: {verdict}"
    );
    assert!(
        verdict.starts_with("Export:"),
        "die Exportmeldung bleibt: {verdict}"
    );
    assert!(verdict.contains("Geprüft ist genau diese Liste, nicht die Datei."));

    // Das Orakel bestätigt das Urteil.
    let bytes = std::fs::read(&out).unwrap();
    assert!(redact_pdf::leaks(&bytes, "DE89 3704 0044 0532 0130 00").is_empty());
    assert!(redact_pdf::leaks(&bytes, "DE02 1203 0000 0000 2020 51").is_empty());
}

/// Ein Fund kommt auch dann an, wenn inzwischen ein anderes Dokument offen
/// ist — und er nennt die Datei, um die es geht. Ein verworfenes Urteil wäre
/// die gefährlichste Form von Schweigen.
#[test]
fn zb1_ein_leck_kommt_an_auch_wenn_inzwischen_etwas_anderes_offen_ist() {
    let out = tmp("zb1-leck").join("geschwaerzt.pdf");
    let mut app = demo_app();
    app.state.regions.push(text_region(
        0,
        Rect::new(40.0, 40.0, 120.0, 60.0),
        "Musterbank",
    ));
    let index = app.state.regions.len() - 1;
    assert_eq!(app.state.hit_summary().outcome(index), HitOutcome::Redacted);

    app.export_to(out.clone());
    app.wait_for_export();
    assert!(app.export_check_running());

    // Dazwischen: ein anderes Dokument.
    app.open_bytes_and_analyze(&redact_pdf::testing::minimal_pdf("Etwas anderes"), "b.pdf");
    assert!(
        app.export_check_running(),
        "das Öffnen verwirft kein Urteil"
    );

    app.wait_for_export_checks();
    let status = app.state.status.clone();
    assert!(
        status.contains("geschwaerzt.pdf"),
        "nennt die Datei: {status}"
    );
    assert!(
        status.contains("steht NOCH in der Ausgabe"),
        "der Fund muss dastehen: {status}"
    );
    assert!(
        app.state
            .warnings
            .first()
            .is_some_and(|w| w.contains("NOCH")),
        "ganz vorn in den Warnungen: {:?}",
        app.state.warnings
    );
    assert!(!redact_pdf::leaks(&std::fs::read(&out).unwrap(), "Musterbank").is_empty());
}

/// Der eine Durchgang sagt dasselbe wie die Suche je Begriff — geprüft am
/// Orakel selbst, nicht an der Oberfläche.
#[test]
fn zb1_ein_durchgang_urteilt_wie_die_suche_je_begriff() {
    let out = tmp("zb1-orakel").join("out.pdf");
    let mut app = demo_app();
    app.state.regions.push(text_region(
        0,
        Rect::new(40.0, 40.0, 120.0, 60.0),
        "Musterbank",
    ));
    app.state.regions.push(text_region(
        1,
        Rect::new(40.0, 40.0, 120.0, 60.0),
        "Seite 2",
    ));
    let summary = app.state.hit_summary();
    app.state.export(&out, None).expect("Export");

    let plan = app.state.plan_export_check(&summary);
    assert_eq!(plan.needles.len(), 4, "{plan:?}");
    let check = plan.clone().run(&out);
    assert_eq!(check.checked, 4);
    assert_eq!(check.skipped, 0);

    let bytes = std::fs::read(&out).unwrap();
    for needle in &plan.needles {
        let single = !redact_pdf::leaks(&bytes, needle).is_empty();
        assert_eq!(
            check.leaking.contains(needle),
            single,
            "„{needle}“: ein Durchgang {} , je Begriff {single}",
            check.leaking.contains(needle)
        );
    }
    assert_eq!(check.leaking, vec!["Musterbank", "Seite 2"]);
}

// ===========================================================================
// 2 — Kleinbilder bleiben unter einer Byte-Decke
// ===========================================================================

/// **Befund 2, behoben.** Über der Decke fällt das am längsten nicht
/// berührte Kleinbild weg; berühren schützt. Vorher blieb jedes liegen.
#[test]
fn zb2_kleinbilder_bleiben_unter_der_decke_und_das_aelteste_faellt_zuerst() {
    let ctx = egui::Context::default();
    let doc = redact_pdf::load_from_bytes(&many_pages(6, 3, &terms(3))).unwrap();
    let mut cache = PageCache::new();
    cache.set_document(Arc::new(doc));

    assert!(cache.request_thumb(0, &ctx));
    wait_for(&mut cache, &ctx, |c| c.has_thumb(0));
    let one = cache.thumb_bytes();
    assert!(one > 0);
    assert_eq!(one, thumb_texture_bytes(&cache, 6));

    // Platz für genau zwei.
    cache.set_thumb_budget(2 * one);
    for page in 1..6 {
        assert!(cache.request_thumb(page, &ctx));
        wait_for(&mut cache, &ctx, |c| c.has_thumb(page));
        assert!(
            cache.thumb_bytes() <= 2 * one,
            "nach Seite {page}: {} Bytes über der Decke {}",
            cache.thumb_bytes(),
            2 * one
        );
    }
    assert_eq!(cache.thumb_count(), 2);
    assert!(
        cache.has_thumb(4) && cache.has_thumb(5),
        "die jüngsten bleiben"
    );
    assert!(
        !cache.has_thumb(0) && !cache.has_thumb(1),
        "die ältesten sind weg"
    );
    assert_eq!(cache.thumb_bytes(), thumb_texture_bytes(&cache, 6));

    // Berühren macht Seite 4 jung — dann fällt Seite 5, nicht 4.
    cache.touch_thumb(4);
    assert!(cache.request_thumb(0, &ctx));
    wait_for(&mut cache, &ctx, |c| c.has_thumb(0));
    assert!(cache.has_thumb(4), "berührt bleibt");
    assert!(!cache.has_thumb(5), "unberührt fällt");
    assert_eq!(cache.thumb_count(), 2);

    // Was weg ist, wird auf Wunsch neu gerechnet — kein Loch für immer.
    assert!(cache.request_thumb(5, &ctx));
    wait_for(&mut cache, &ctx, |c| c.has_thumb(5));

    cache.reset();
    assert_eq!(cache.thumb_bytes(), 0);
}

/// Die angezeigte Seite behält ihr Kleinbild — es überbrückt die Wartezeit
/// auf das Vollbild —, auch wenn die Decke sonst voll ist.
#[test]
fn zb2_die_angezeigte_seite_behaelt_ihr_kleinbild() {
    let ctx = egui::Context::default();
    let doc = redact_pdf::load_from_bytes(&many_pages(4, 3, &terms(3))).unwrap();
    let view = PageView::upright(redact_pdf::page_boxes(&doc)[0]);
    let mut cache = PageCache::new();
    cache.set_document(Arc::new(doc));

    // Seite 0 ist die angezeigte Seite.
    cache.request(0, &view, 1.0, &ctx);
    wait_for(&mut cache, &ctx, |c| c.has_thumb(0) && !c.is_busy());
    let one = cache.thumb_bytes();
    cache.set_thumb_budget(one + 1);

    for page in 1..4 {
        assert!(cache.request_thumb(page, &ctx));
        wait_for(&mut cache, &ctx, |c| c.has_thumb(page));
        assert!(
            cache.has_thumb(0),
            "nach Seite {page}: die angezeigte Seite hat ihr Kleinbild verloren"
        );
        assert!(cache.thumb_count() <= 2, "{}", cache.thumb_count());
    }
    assert!(cache.has_thumb(3));
    assert!(!cache.has_thumb(1) && !cache.has_thumb(2));
}

/// Die andere Richtung: ein gewöhnliches Dokument verliert **kein**
/// Kleinbild. 64 MiB sind gut 200 A4-Seiten; das Demo-PDF behält beide.
#[test]
fn zb2_gewoehnliche_dokumente_verlieren_kein_kleinbild() {
    let a4 = THUMB_WIDTH as usize * (THUMB_WIDTH as f64 * 842.0 / 595.0) as usize * 4;
    assert!(
        MAX_THUMB_BYTES / a4 >= 200,
        "{} A4-Kleinbilder passen unter die Decke — zu wenig",
        MAX_THUMB_BYTES / a4
    );

    let ctx = egui::Context::default();
    let doc = redact_pdf::load_from_bytes(&redact_pdf::testing::demo_statement()).unwrap();
    let mut cache = PageCache::new();
    cache.set_document(Arc::new(doc));
    for page in 0..2 {
        assert!(cache.request_thumb(page, &ctx));
        wait_for(&mut cache, &ctx, |c| c.has_thumb(page));
    }
    assert_eq!(cache.thumb_count(), 2);
    assert!(cache.thumb_bytes() < MAX_THUMB_BYTES);
}

/// Die Miniaturspalte fordert nur **sichtbare** Kleinbilder an. Sonst liefe
/// sie mit der Decke im Kreis: anfordern, verwerfen, anfordern — und hätte
/// vorher jede der 100 Seiten geholt.
#[test]
fn zb2_die_spalte_fordert_nur_sichtbare_kleinbilder_an() {
    let pages = 100;
    let mut state = AppState::new();
    state
        .load_bytes(&many_pages(pages, 3, &terms(3)), None)
        .unwrap();
    let mut cache = PageCache::new();
    cache.set_document(state.document.clone().unwrap());
    let summary = state.hit_summary();

    let state = RefCell::new(state);
    let cache = RefCell::new(cache);
    let ctx = egui::Context::default();
    // Ein niedriges Fenster: etwa vier Kleinbilder sind zu sehen.
    let input = screen(600.0, 520.0);

    for frame in 0..40 {
        let _ = ctx.run(input.clone(), |ctx| {
            egui::SidePanel::left("thumbnails")
                .default_width(crate::thumbnails::PANEL_WIDTH)
                .show(ctx, |ui| {
                    crate::thumbnails::show(
                        ui,
                        &mut state.borrow_mut(),
                        &mut cache.borrow_mut(),
                        &summary,
                        frame == 0,
                    );
                });
        });
        // Wie im Betrieb: das angeforderte Bild kommt, dann das nächste Bild.
        let mut c = cache.borrow_mut();
        wait_for(&mut c, &ctx, |c| !c.is_busy());
    }

    let cache = cache.borrow();
    assert!(
        cache.thumb_count() <= 12,
        "{} Kleinbilder nach 40 Bildern — die Spalte holt Unsichtbares",
        cache.thumb_count()
    );
    assert!(cache.has_thumb(0), "die erste Seite ist sichtbar und da");
    assert!(
        !cache.has_thumb(60),
        "Seite 61 ist nicht zu sehen und nicht geholt"
    );
}

// ===========================================================================
// 3 — Die Trefferliste baut nur die sichtbaren Zeilen
// ===========================================================================

/// 10 000 Zeilen mit verschiedenen Texten, ein Bild, ein 700 pt hohes
/// Fenster: nur die sichtbaren Texte werden gesetzt.
fn many_rows_state(n: usize) -> (AppState, HitSummary) {
    let mut state = AppState::with_config(iban_only());
    state
        .load_bytes(&redact_pdf::testing::demo_statement(), None)
        .unwrap();
    for i in 0..n {
        state.regions.push(text_region(
            0,
            Rect::new(72.0, 100.0, 300.0, 112.0),
            &format!("Text Nummer {i}"),
        ));
    }
    // Die Bilanz von Hand — die Konfliktauflösung über 50 000 Zeilen ist eine
    // andere Frage als der Aufbau der Liste.
    let summary = HitSummary {
        outcomes: vec![HitOutcome::Redacted; n],
        found: n,
        protecting: 0,
        redacted: n,
        off_page: 0,
        missing_page: 0,
        automatic: true,
        detection_switch: true,
        disabled_patterns: 0,
    };
    (state, summary)
}

fn draw_sidebar(ctx: &egui::Context, state: &RefCell<AppState>, summary: &HitSummary) {
    let _ = ctx.run(screen(1280.0, 700.0), |ctx| {
        egui::SidePanel::left("sidebar")
            .default_width(SIDEBAR_WIDTH)
            .show(ctx, |ui| {
                let _ = crate::sidebar::show(ui, &mut state.borrow_mut(), summary);
            });
    });
}

/// **Befund 3, behoben.** Gezählt werden die gesetzten Texte (Galleys) —
/// vorher einer je Zeile, 10 000 und mehr; jetzt nur die im Fenster.
#[test]
fn zb3_die_trefferliste_baut_nur_die_sichtbaren_zeilen() {
    let (state, summary) = many_rows_state(10_000);
    let state = RefCell::new(state);
    let ctx = egui::Context::default();
    // Zwei Bilder: das erste misst die Leisten, das zweite zählt.
    draw_sidebar(&ctx, &state, &summary);
    draw_sidebar(&ctx, &state, &summary);
    let galleys = ctx.fonts(|f| f.num_galleys_in_cache());
    assert!(
        galleys < 1_000,
        "{galleys} gesetzte Texte für ein 700 pt hohes Fenster — die Liste baut alles"
    );
    assert!(galleys > 20, "{galleys}: es wurde gar nichts gezeichnet");
}

/// Die feste Zeilenhöhe, mit der `show_rows` rechnet, ist die wirkliche —
/// für jede Zeilenart: schlicht, mit Zusatz („abgewählt“), mit trägem
/// Kästchen (Schutzeintrag). Stimmte sie nicht, lägen Zeilen übereinander
/// oder es blieben Lücken.
#[test]
fn zb3_jede_zeilenart_ist_so_hoch_wie_angesagt() {
    let mut state = AppState::new();
    state
        .regions
        .push(text_region(0, Rect::new(0.0, 0.0, 10.0, 10.0), "schlicht"));
    state
        .regions
        .push(text_region(0, Rect::new(0.0, 0.0, 10.0, 10.0), "abgewählt"));
    state.regions.push(AnnotatedRegion::new(Region::new(
        0,
        Rect::new(0.0, 0.0, 10.0, 10.0),
        Some("geschützt".into()),
        Source::Booking {
            booking_id: "b1".into(),
            match_type: MatchType::Negative,
        },
    )));
    assert!(state.regions[2].is_blocking());
    let summary = HitSummary {
        outcomes: vec![
            HitOutcome::Redacted,
            HitOutcome::Disabled,
            HitOutcome::Protecting,
        ],
        found: 2,
        protecting: 1,
        redacted: 1,
        off_page: 0,
        missing_page: 0,
        automatic: true,
        detection_switch: true,
        disabled_patterns: 0,
    };
    egui::__run_test_ui(|ui| {
        let expected = crate::sidebar::hit_row_height(ui);
        for index in 0..3 {
            let (mut toggle, mut select) = (None, None);
            let row =
                crate::sidebar::hit_row(ui, &state, &summary, index, &mut toggle, &mut select);
            assert_eq!(
                row.rect.height(),
                expected,
                "Zeile {index} ist {} pt hoch, angesagt sind {expected}",
                row.rect.height()
            );
        }
    });
}

// ===========================================================================
// 4 — Die Nachprüfung deckelt Begriffe wie die Kommandozeile
// ===========================================================================

/// **Befund 4, zum zweiten Mal entschieden.** Die erste Korrektur rechnete
/// Begriffe × Dateibytes auf der Platte gegen 2 GiB — die falsche Einheit
/// (siehe [`crate::state::ExportCheckPlan::run`]). Jetzt gilt dieselbe Decke
/// wie für `--check-leaks`, in Begriffen. Ohne die Korrektur (`.min(limit)`
/// weg) würden alle Begriffe gesucht und `skipped` bliebe 0.
#[test]
fn zb4_die_nachpruefung_deckelt_begriffe_wie_die_kommandozeile() {
    let out = tmp("zb4").join("out.pdf");
    let app = demo_app();
    let summary = app.state.hit_summary();
    app.state.export(&out, None).expect("Export");
    let mut plan = app.state.plan_export_check(&summary);
    let own = plan.needles.len();
    assert!((1..MAX_CHECK_NEEDLES).contains(&own), "{plan:?}");

    // Einen Begriff mehr, als die Decke zulässt: genau einer bleibt liegen.
    for i in own..=MAX_CHECK_NEEDLES {
        plan.needles.push(format!("weiterer Begriff {i}"));
    }
    assert_eq!(plan.needles.len(), MAX_CHECK_NEEDLES + 1);
    let capped = plan.clone().run(&out);
    assert_eq!(capped.checked, MAX_CHECK_NEEDLES, "{capped:?}");
    assert_eq!(capped.skipped, 1, "{capped:?}");
    assert_eq!(capped.limit, MAX_CHECK_NEEDLES);
    let sentence = capped.sentence();
    assert!(
        sentence.contains("1 weitere Text(e) wurden nicht gesucht"),
        "{sentence}"
    );
    assert!(
        sentence.contains(&format!(
            "höchstens {MAX_CHECK_NEEDLES} Begriffe je Nachprüfung (dieselbe Decke wie \
             --check-leaks)"
        )),
        "{sentence}"
    );
    assert!(
        sentence.contains("Trefferliste weiter hinten"),
        "{sentence}"
    );
    // Die eigenen Begriffe stehen vorn und wurden gesucht — und stehen nicht
    // mehr in der Datei.
    assert!(capped.leaking.is_empty(), "{capped:?}");

    // Mit tieferer Decke: 4 von 10, wie der Test der ersten Korrektur.
    plan.needles.truncate(10);
    let tight = plan.clone().run_within(&out, 4);
    assert_eq!(tight.checked, 4, "{tight:?}");
    assert_eq!(tight.skipped, 6, "{tight:?}");
    assert_eq!(tight.limit, 4);
    assert!(
        tight
            .sentence()
            .contains("6 weitere Text(e) wurden nicht gesucht"),
        "{}",
        tight.sentence()
    );
    assert!(
        tight.sentence().contains("höchstens 4 Begriffe"),
        "{}",
        tight.sentence()
    );

    // Die andere Richtung: 10 Begriffe an einer kleinen Datei werden alle
    // gesucht — die Decke hängt nicht mehr an der Dateigröße.
    let real = plan.run(&out);
    assert_eq!(real.checked, 10, "{real:?}");
    assert_eq!(real.skipped, 0);
    assert!(
        !real.sentence().contains("nicht gesucht"),
        "{}",
        real.sentence()
    );
}

/// Die Decke ist an der Kommandozeile und in der Oberfläche **eine** Zahl —
/// der Satz der Oberfläche nennt sie mit derselben Konstante, die
/// `--check-leaks` anwendet.
#[test]
fn zb4_die_decke_der_oberflaeche_ist_die_der_kommandozeile() {
    let check = crate::state::ExportCheck {
        checked: MAX_CHECK_NEEDLES,
        skipped: 3,
        limit: MAX_CHECK_NEEDLES,
        ..Default::default()
    };
    let sentence = check.sentence();
    assert!(
        sentence.contains(&format!("höchstens {MAX_CHECK_NEEDLES} Begriffe")),
        "{sentence}"
    );
    assert!(sentence.contains("--check-leaks"), "{sentence}");
    // Die alte Einheit ist aus dem Satz verschwunden.
    assert!(!sentence.contains("kB"), "{sentence}");
    assert!(!sentence.contains("Prüfbudget"), "{sentence}");
}

// ===========================================================================
// 5 — Ein abgewählter Text ist eine Entscheidung, kein Leck
// ===========================================================================

/// **Befund 5, behoben.** Dieselbe IBAN auf zwei Seiten, eine abgewählt:
/// die Nachprüfung meldete „1 … steht NOCH in der Ausgabe — darf so nicht
/// weitergegeben werden“ über eine Datei, die genau so gewollt war. Jetzt
/// sagt sie, dass dieser Text nicht gesucht wurde, und warum.
#[test]
fn zb5_ein_abgewaehlter_text_ist_eine_entscheidung_kein_leck() {
    let out = tmp("zb5").join("out.pdf");
    let same = "DE89 3704 0044 0532 0130 00";
    let other = "DE02 1203 0000 0000 2020 51";
    let pdf = build_pdf(&[
        vec![TextItem::new(72.0, 700.0, 10.0, format!("IBAN: {same}"))],
        vec![TextItem::new(72.0, 700.0, 10.0, format!("IBAN: {same}"))],
        vec![TextItem::new(72.0, 700.0, 10.0, format!("IBAN: {other}"))],
    ]);
    let mut app = RedactApp::silent(iban_only());
    app.open_bytes_and_analyze(&pdf, "drei.pdf");
    assert_eq!(app.state.regions.len(), 3, "{:?}", app.state.regions);
    assert_eq!(app.state.regions[0].region.text.as_deref(), Some(same));
    assert_eq!(app.state.regions[1].region.text.as_deref(), Some(same));

    // Die Zeile auf Seite 2 bleibt bewusst stehen.
    assert!(app.state.set_enabled(1, false));
    let summary = app.state.hit_summary();
    assert_eq!(summary.outcome(1), HitOutcome::Disabled);

    let plan = app.state.plan_export_check(&summary);
    // Seit Fix-Runde 7 stehen **beide** Texte in `needles`; gedeckt ist nur
    // der Fund der abgewählten Zeile.
    assert_eq!(
        plan.needles,
        vec![same.to_string(), other.to_string()],
        "{plan:?}"
    );
    assert_eq!(plan.kept_literal, vec![true, false], "{plan:?}");

    app.export_to(out.clone());
    app.wait_for_export();
    app.wait_for_export_checks();
    let status = app.state.status.clone();
    assert!(
        !status.contains("steht NOCH in der Ausgabe"),
        "die Entscheidung gilt als Leck: {status}"
    );
    // Der Vorbehalt steht **vor** der Entwarnung (Befund Q4-3).
    assert!(
        status.contains("1 gesuchte Text(e) stehen nicht mehr in der Ausgabe."),
        "{status}"
    );
    assert!(
        status.contains(
            "1 Text(e) stehen wörtlich auch in einer abgewählten, gelöschten oder geschützten Zeile: \
             über sie sagt diese Prüfung nichts"
        ),
        "{status}"
    );
    // Seit Befund Q4-1 bleibt dieser Vorbehalt auch in den Warnungen stehen:
    // die Statuszeile überschreibt die nächste Aktion, und „nicht gesucht“
    // ist keine Entwarnung.
    assert_eq!(app.state.warnings.len(), 1, "{:?}", app.state.warnings);
    assert!(
        app.state.warnings[0].contains("über 1 Text(e) sagt sie nichts"),
        "{:?}",
        app.state.warnings
    );

    // Das Orakel: die abgewählte IBAN steht wirklich noch da, die andere nicht.
    let bytes = std::fs::read(&out).unwrap();
    assert!(!redact_pdf::leaks(&bytes, same).is_empty());
    assert!(redact_pdf::leaks(&bytes, other).is_empty());
}

/// **Befund 5, die zweite Hälfte.** Derselbe Text in **zwei** geschwärzten
/// Zeilen und einer abgewählten: das `retain` der ersten Korrektur strich
/// ihn nach der ersten geschwärzten Zeile aus den stehen gelassenen, und die
/// zweite Zeile trug ihn in `needles` — derselbe Text in `needles` **und**
/// `kept`, und die Nachprüfung meldete ein Leck über eine Datei, die genau so
/// gewollt war. Jetzt fällt je Text eine Entscheidung, gleich wie viele
/// Zeilen ihn tragen.
#[test]
fn zb5_zweimal_geschwaerzt_einmal_abgewaehlt_ist_kein_leck() {
    let out = tmp("zb5-zweimal").join("out.pdf");
    let same = "DE89 3704 0044 0532 0130 00";
    let pdf = build_pdf(&[
        vec![TextItem::new(72.0, 700.0, 10.0, format!("IBAN: {same}"))],
        vec![TextItem::new(72.0, 700.0, 10.0, format!("IBAN: {same}"))],
        vec![TextItem::new(72.0, 700.0, 10.0, format!("IBAN: {same}"))],
    ]);
    let mut app = RedactApp::silent(iban_only());
    app.open_bytes_and_analyze(&pdf, "drei.pdf");
    assert_eq!(app.state.regions.len(), 3, "{:?}", app.state.regions);
    for entry in &app.state.regions {
        assert_eq!(entry.region.text.as_deref(), Some(same));
    }

    // Die Zeile auf Seite 2 bleibt bewusst stehen; Seite 1 und 3 werden
    // geschwärzt.
    assert!(app.state.set_enabled(1, false));
    let summary = app.state.hit_summary();
    assert_eq!(summary.outcome(0), HitOutcome::Redacted);
    assert_eq!(summary.outcome(1), HitOutcome::Disabled);
    assert_eq!(summary.outcome(2), HitOutcome::Redacted);

    let plan = app.state.plan_export_check(&summary);
    // Seit Fix-Runde 7 steht der Text in `needles` — gedeckt ist nur der
    // **Fund**.
    assert_eq!(plan.kept_literal, vec![true], "{plan:?}");

    app.export_to(out.clone());
    app.wait_for_export();
    app.wait_for_export_checks();
    let status = app.state.status.clone();
    assert!(
        !status.contains("steht NOCH"),
        "die Entscheidung gilt als Leck: {status}"
    );
    // Gesucht wurde er (Fix-Runde 7), gefunden auch — die abgewählte Zeile
    // steht ja noch da. Verschwunden ist damit keiner.
    assert!(
        status.contains("0 gesuchte Text(e) stehen nicht mehr in der Ausgabe."),
        "{status}"
    );
    assert!(
        status
            .contains("1 Text(e) stehen wörtlich auch in einer abgewählten, gelöschten oder geschützten Zeile"),
        "{status}"
    );

    // Das Orakel: die abgewählte Zeile steht wirklich noch da — bewusst.
    let bytes = std::fs::read(&out).unwrap();
    assert!(!redact_pdf::leaks(&bytes, same).is_empty());
}

/// Nur stehen gelassene Texte: dann wurde nichts gesucht, und der Satz sagt
/// nicht „keine geschwärzte Zeile mit bekanntem Text“ — die gibt es ja.
#[test]
fn zb5_nur_stehen_gelassene_texte_heisst_nichts_gesucht_und_sagt_warum() {
    let check = crate::state::ExportCheck {
        checked: 0,
        kept: 2,
        ..Default::default()
    };
    let sentence = check.sentence();
    assert!(
        sentence.starts_with("Nachprüfung: es wurde nichts gesucht."),
        "{sentence}"
    );
    assert!(
        sentence.contains("2 Text(e) decken sich mit einer abgewählten"),
        "{sentence}"
    );
    assert!(!sentence.contains("keine geschwärzte Zeile"), "{sentence}");
    assert!(!check.found_leak());
}

// ===========================================================================
// 6 — Eine Pfeiltaste beendet den Griff-Zug
// ===========================================================================

/// **Befund 6, behoben.** Griff gefasst, gezogen, Pfeil rechts: der Schritt
/// gilt, das nächste Mausbild setzt ihn nicht zurück, und im Verlauf stehen
/// genau zwei Schritte — der Zug und die Taste.
#[test]
fn zb6_eine_pfeiltaste_beendet_den_griff_zug_statt_von_ihm_ueberschrieben_zu_werden() {
    let mut app = demo_app();
    app.state.selected_region = Some(0);
    let before = app.state.regions[0].region.rect;
    let view = app.state.page_view(0);
    let corner = viewer::pdf_to_screen(&before, &view, ZOOM, ORIGIN).left_top();

    app.apply_pointer(press_frame(corner), false, &view, ORIGIN, 0, ZOOM);
    assert!(app.resize.is_some(), "der Griff ist gefasst");
    let target = corner - Vec2::new(20.0, 20.0);
    app.apply_pointer(
        dragging_frame(corner, target),
        false,
        &view,
        ORIGIN,
        0,
        ZOOM,
    );
    let dragged = app.state.regions[0].region.rect;
    assert_ne!(dragged, before, "der Zug hat gewirkt");

    // Pfeil rechts, Maustaste noch unten.
    press(&mut app, keys(|k| k.right = true));
    assert!(app.resize.is_none(), "die Taste beendet den Zug");
    let stepped = app.state.regions[0].region.rect;
    assert!(
        (stepped.ll.x - (dragged.ll.x + NUDGE)).abs() < 1e-9,
        "{stepped:?}"
    );
    assert!(
        (stepped.ur.x - (dragged.ur.x + NUDGE)).abs() < 1e-9,
        "{stepped:?}"
    );

    // Das nächste Mausbild — die Taste ist ja noch unten — ändert nichts mehr.
    let further = target - Vec2::new(5.0, 5.0);
    app.apply_pointer(
        dragging_frame(corner, further),
        false,
        &view,
        ORIGIN,
        0,
        ZOOM,
    );
    assert_eq!(
        app.state.regions[0].region.rect, stepped,
        "der Zug hat den Tastenschritt überschrieben"
    );
    app.apply_pointer(release_frame(further), false, &view, ORIGIN, 0, ZOOM);
    assert_eq!(app.state.regions[0].region.rect, stepped);
    assert_eq!(
        app.state.regions.len(),
        2,
        "kein neues Rechteck aus dem Rest des Zugs"
    );

    // Verlauf: Taste zurück, dann Zug zurück.
    app.state.undo();
    assert_eq!(app.state.regions[0].region.rect, dragged);
    app.state.undo();
    assert_eq!(app.state.regions[0].region.rect, before);
}

// ===========================================================================
// 7 — Strg ändert nichts an Esc, Entf, Bild auf/ab, Pos1 und Ende
// ===========================================================================

/// **Befund 7, entschieden.** Die Tastentabelle verspricht die sechs Tasten
/// ohne Vorbehalt; jetzt hält der Code das — wie Umschalt+Bild ab schon
/// vorher blätterte. Strg+Pfeil bleibt, was es war.
#[test]
fn zb7_strg_aendert_nichts_an_esc_entf_bild_pos1_und_ende() {
    type Set = fn(&mut KeyState);
    let cases: [(Set, KeyCommand); 6] = [
        (|k| k.escape = true, KeyCommand::Deselect),
        (|k| k.delete = true, KeyCommand::DeleteSelected),
        (|k| k.page_up = true, KeyCommand::PrevPage),
        (|k| k.page_down = true, KeyCommand::NextPage),
        (|k| k.home = true, KeyCommand::FirstPage),
        (|k| k.end = true, KeyCommand::LastPage),
    ];
    for (set, expected) in cases {
        let plain = keys(set);
        let with_ctrl = keys(|k| {
            set(k);
            k.ctrl = true;
        });
        assert_eq!(key_commands(plain, true), vec![expected], "ohne Strg");
        assert_eq!(key_commands(with_ctrl, true), vec![expected], "mit Strg");
    }

    // Unverändert: Strg+Pfeil ohne Auswahl tut nichts, mit Auswahl ändert es
    // die Größe; Umschalt+Bild ab blättert.
    assert!(key_commands(
        keys(|k| {
            k.ctrl = true;
            k.left = true;
        }),
        false
    )
    .is_empty());
    assert_eq!(
        key_commands(
            keys(|k| {
                k.ctrl = true;
                k.left = true;
            }),
            true
        ),
        vec![KeyCommand::Resize {
            dx: -NUDGE,
            dy: 0.0
        }]
    );
    assert_eq!(
        key_commands(
            keys(|k| {
                k.shift = true;
                k.page_down = true;
            }),
            false
        ),
        vec![KeyCommand::NextPage]
    );
}

/// Dasselbe am echten Kontext: Strg+Bild ab blättert das Demo-PDF auf
/// Seite 2 — die Modifikatoren kommen aus egui, nicht aus einem `KeyState`
/// von Hand.
#[test]
fn zb7_strg_bild_ab_blaettert_am_echten_kontext() {
    let app = RefCell::new(demo_app());
    assert_eq!(app.borrow().state.current_page, 0);
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        events: vec![egui::Event::Key {
            key: Key::PageDown,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }],
        modifiers: egui::Modifiers::COMMAND,
        ..screen(1280.0, 860.0)
    };
    let _ = ctx.run(input, |ctx| {
        app.borrow_mut().handle_keys(ctx);
    });
    assert_eq!(
        app.borrow().state.current_page,
        1,
        "Strg+Bild ab hat nicht geblättert"
    );
}

// ===========================================================================
// #16 — Seitenzahlen aus fremder Hand laufen in der Anzeige nicht über
// ===========================================================================

/// `Region.page` kommt aus einer Review- oder Regionsdatei; `usize::MAX`
/// steht dort, wenn jemand es hineinschreibt. Die 1-basierte Anzeige rechnet
/// `+ 1`, und im Debug-Build ist das eine Panic — ausgerechnet in der Absage,
/// die erklären soll, dass es die Seite nicht gibt, und beim Malen der
/// Trefferliste. `saturating_add` hält die Zeile lesbar; mit `+ 1` fällt der
/// Test um.
#[test]
fn absurd_page_numbers_do_not_panic_in_labels() {
    let huge = "18446744073709551615";

    let sentence = selection_on_missing_page(usize::MAX, 1, NudgeKind::Move);
    assert!(sentence.contains(huge), "{sentence}");
    assert!(sentence.contains("die es in diesem Dokument"), "{sentence}");

    let sentence = crate::state::selection_on_other_page(usize::MAX, NudgeKind::Resize);
    assert!(sentence.contains(huge), "{sentence}");

    let entry = text_region(
        usize::MAX,
        Rect::new(72.0, 700.0, 300.0, 712.0),
        "DE89 3704 0044 0532 0130 00",
    );
    let label = entry.label();
    assert!(label.starts_with(&format!("S.{huge} ")), "{label}");
}

// ===========================================================================
// Fix-Runde 4, Gegenprüfung G5: was die Nachprüfung sagt, muss stehen
// bleiben — und die Datei nennen
// ===========================================================================

/// Lauter verschiedene kleine Rechtecke in einer leeren Ecke — deckungsgleiche
/// gleicher Herkunft gälten sonst als „doppelt“, nicht als geschwärzt.
fn distinct_corner(i: usize) -> Rect {
    let x = 400.0 + (i % 50) as f64 * 2.0;
    let y = 20.0 + (i / 50) as f64 * 2.0;
    Rect::new(x, y, x + 1.5, y + 1.5)
}

/// **Befund G5-A1.** „N weitere Text(e) wurden nicht gesucht“ stand nur in
/// der Statuszeile, und die nächste Aktion überschreibt sie. Jetzt steht es
/// zusätzlich vorn in den Warnungen — wie ein Fund, mit eigenem Satz, mit
/// dem Namen der Datei. Mutation (`skipped`-Zweig in `warning()` weg): rot.
#[test]
fn zb_g5a1_nicht_gesuchte_texte_stehen_in_den_warnungen() {
    let out = tmp("g5a1").join("geschwaerzt.pdf");
    let mut app = demo_app();
    // Einen Begriff mehr, als die Decke zulässt — jeder in einer eigenen
    // Zeile, keiner steht in der Datei.
    for (i, term) in terms(MAX_CHECK_NEEDLES + 1).iter().enumerate() {
        app.state
            .regions
            .push(text_region(0, distinct_corner(i), term));
    }
    let summary = app.state.hit_summary();
    let plan = app.state.plan_export_check(&summary);
    assert!(
        plan.needles.len() > MAX_CHECK_NEEDLES,
        "{}",
        plan.needles.len()
    );

    app.export_to(out.clone());
    app.wait_for_export();
    app.wait_for_export_checks();
    let status = app.state.status.clone();
    assert!(!status.contains("steht NOCH"), "{status}");
    assert!(status.contains("wurden nicht gesucht"), "{status}");

    let warning = app
        .state
        .warnings
        .first()
        .cloned()
        .unwrap_or_else(|| panic!("keine Warnung: {:?}", app.state.warnings));
    assert!(warning.starts_with("geschwaerzt.pdf: "), "{warning}");
    assert!(
        warning.contains("Nachprüfung unvollständig"),
        "eigener Satz: {warning}"
    );
    assert!(warning.contains("wurden nicht gesucht"), "{warning}");
    assert!(
        warning.contains(&format!("höchstens {MAX_CHECK_NEEDLES} Begriffe")),
        "{warning}"
    );
    assert!(!warning.contains("steht NOCH"), "{warning}");

    // Die nächste Aktion überschreibt die Statuszeile — die Warnung bleibt.
    app.state.status = "etwas anderes".to_string();
    assert_eq!(app.state.warnings.first(), Some(&warning));
}

/// **Befund G5-A3.** Verschwindet der Prüf-Thread ohne Ergebnis (Panik),
/// sagt die Oberfläche „abgebrochen (interner Fehler)“ — bisher ohne Test:
/// die Zeile `done.push((prefix, None))` konnte fallen, und 306 Tests blieben
/// grün. Hier lässt ein Testhaken die Prüfung im Thread paniken. Mutation
/// (die Zeile weg): der Eintrag verschwindet stumm, die Statuszeile bleibt
/// bei „läuft“ — rot.
#[test]
fn zb_g5a3_eine_panik_im_pruefthread_wird_gesagt_und_nicht_verschwiegen() {
    let out = tmp("g5a3").join("geschwaerzt.pdf");
    let mut app = demo_app();
    app.force_panic_in_check = true;
    app.export_to(out.clone());
    app.wait_for_export();
    assert!(app.export_check_running());
    app.wait_for_export_checks();
    assert!(!app.export_check_running());
    let status = app.state.status.clone();
    assert!(
        status.contains("Nachprüfung: abgebrochen (interner Fehler)"),
        "{status}"
    );
    assert!(!status.contains(EXPORT_CHECK_RUNNING), "{status}");
    assert!(status.starts_with("Export:"), "{status}");
    // Auch in den Warnungen, mit der Datei — die Statuszeile ist flüchtig.
    let warning = app
        .state
        .warnings
        .first()
        .cloned()
        .unwrap_or_else(|| panic!("keine Warnung: {:?}", app.state.warnings));
    assert!(warning.starts_with("geschwaerzt.pdf: "), "{warning}");
    assert!(
        warning.contains("abgebrochen (interner Fehler)"),
        "{warning}"
    );

    // Ohne den Haken kommt das Urteil wie immer.
    app.force_panic_in_check = false;
    app.export_to(out.clone());
    app.wait_for_export();
    app.wait_for_export_checks();
    assert!(
        app.state
            .status
            .contains("stehen nicht mehr in der Ausgabe"),
        "{}",
        app.state.status
    );
}

/// **Befund G5-A4.** Ein Fund ging als nackter Satz in die Warnungen; nach
/// einem zweiten Export war nicht mehr zu sagen, welche Datei gemeint ist.
/// Jetzt steht der Dateiname davor. Zwei Exporte, beide mit Leck, beide
/// unterwegs, bevor das erste Urteil kommt: zwei Warnungen, jede mit ihrer
/// Datei. Mutation (Präfix weg): rot.
#[test]
fn zb_g5a4_die_warnung_nennt_die_datei() {
    let dir = tmp("g5a4");
    let first = dir.join("erste.pdf");
    let second = dir.join("zweite.pdf");
    let mut app = demo_app();
    app.state.regions.push(text_region(
        0,
        Rect::new(40.0, 40.0, 120.0, 60.0),
        "Musterbank",
    ));
    let index = app.state.regions.len() - 1;
    assert_eq!(app.state.hit_summary().outcome(index), HitOutcome::Redacted);

    app.export_to(first.clone());
    app.wait_for_export();
    app.export_to(second.clone());
    app.wait_for_export();
    app.wait_for_export_checks();

    let warnings = app.state.warnings.clone();
    let leaks: Vec<&String> = warnings
        .iter()
        .filter(|w| w.contains("steht NOCH in der Ausgabe"))
        .collect();
    assert_eq!(leaks.len(), 2, "{warnings:?}");
    assert!(
        leaks
            .iter()
            .any(|w| w.starts_with("erste.pdf: Nachprüfung:")),
        "{warnings:?}"
    );
    assert!(
        leaks
            .iter()
            .any(|w| w.starts_with("zweite.pdf: Nachprüfung:")),
        "{warnings:?}"
    );
    // Der Satz selbst ist unverändert der der Statuszeile.
    assert!(
        app.state.status.contains("steht NOCH in der Ausgabe"),
        "{}",
        app.state.status
    );
}

// ===========================================================================
// P5-D3 — die Warnung des vorigen Exports bleibt stehen
// ===========================================================================

/// Ein Dokument mit einem Leck, das jeder Export stehen lässt: das Rechteck
/// liegt neben dem Text.
fn leaking_app() -> RedactApp {
    let mut app = demo_app();
    app.state.regions.push(text_region(
        0,
        Rect::new(40.0, 40.0, 120.0, 60.0),
        "Musterbank",
    ));
    let index = app.state.regions.len() - 1;
    assert_eq!(app.state.hit_summary().outcome(index), HitOutcome::Redacted);
    app
}

fn leak_warnings(app: &RedactApp) -> Vec<String> {
    app.state
        .warnings
        .iter()
        .filter(|w| w.contains("steht NOCH in der Ausgabe"))
        .cloned()
        .collect()
}

/// **Befund P5-D3.** `export_to` setzte `self.state.warnings =
/// outcome.warnings` bei **jedem** geglückten Export — die Warnung des
/// vorigen war damit weg. Genau der Fall, für den Fix-Runde 4 den Dateinamen
/// eingeführt hat: zwei Ausgabedateien, zwei Urteile, und man muss sehen
/// können, welches zu welcher gehört. `zb_g5a4_die_warnung_nennt_die_datei`
/// überlebte nur, weil dort **beide** Prüfungen noch liefen, als der zweite
/// Export begann. Hier ist das erste Urteil schon da — vorher: rot.
/// Mutation (`note_export_warnings` → `self.state.warnings = …`): rot.
#[test]
fn zb_p5d3_die_warnung_des_ersten_exports_ueberlebt_den_zweiten() {
    let dir = tmp("p5d3-zwei");
    let mut app = leaking_app();

    app.export_to(dir.join("erste.pdf"));
    app.wait_for_export();
    app.wait_for_export_checks();
    assert_eq!(leak_warnings(&app).len(), 1, "{:?}", app.state.warnings);

    // Erst wenn das erste Urteil steht, der zweite Export.
    app.export_to(dir.join("zweite.pdf"));
    app.wait_for_export();
    app.wait_for_export_checks();

    let leaks = leak_warnings(&app);
    assert_eq!(leaks.len(), 2, "{:?}", app.state.warnings);
    assert!(
        leaks.iter().any(|w| w.starts_with("erste.pdf: ")),
        "die erste Warnung ist weg: {:?}",
        app.state.warnings
    );
    assert!(
        leaks.iter().any(|w| w.starts_with("zweite.pdf: ")),
        "{:?}",
        app.state.warnings
    );
}

/// Die Gegenrichtung, drei Hälften: derselbe Dateiname sammelt sich nicht
/// (dreimal exportiert, eine Warnung), ein Export, der das Leck **schließt**,
/// nimmt die alte Warnung mit (sonst stünde eine Warnung über eine Datei, die
/// es nicht mehr gibt), und mehr als [`MAX_WARNED_FILES`] Dateien werden
/// nicht gehalten — die älteste fällt heraus.
/// Mutation (`forget_warnings_of` in `note_export_warnings` weg): die alte
/// Warnung bleibt über der geschwärzten Datei stehen, rot. Mutation (die
/// `while`-Schleife in `remember_warnings_of` weg): 11 statt 10, rot.
#[test]
fn zb_p5d3_dieselbe_datei_sammelt_sich_nicht_und_die_decke_haelt() {
    let dir = tmp("p5d3-decke");
    let mut app = leaking_app();

    for _ in 0..3 {
        app.export_to(dir.join("gleich.pdf"));
        app.wait_for_export();
        app.wait_for_export_checks();
    }
    assert_eq!(leak_warnings(&app).len(), 1, "{:?}", app.state.warnings);

    // Und zwei Exporte derselben Datei **kurz hintereinander**: beide tragen
    // denselben Satz, und der steht einmal da.
    //
    // Der Satz „beide unterwegs, bevor das erste Urteil kommt“ stand hier
    // früher und stimmte schon vorher nicht: seit Befund Q4-6 lässt der zweite
    // Export die ältere Prüfung derselben Datei fallen, es ist also immer
    // höchstens eine unterwegs. Seit Fix-Runde 8 schreibt der Export auf einem
    // eigenen Thread, und der zweite Klick auf **dieselbe** Datei wird
    // abgelehnt, solange der erste schreibt ([`EXPORT_BUSY`]) — deshalb wartet
    // dieser Teil jetzt ausdrücklich auf jeden Export. Geprüft ist damit, was
    // hier wirklich zu prüfen ist: zwei Exporte derselben Datei lassen genau
    // eine Warnung stehen.
    app.export_to(dir.join("gleich.pdf"));
    app.wait_for_export();
    app.export_to(dir.join("gleich.pdf"));
    app.wait_for_export();
    app.wait_for_export_checks();
    assert_eq!(leak_warnings(&app).len(), 1, "{:?}", app.state.warnings);

    // Dieselbe Datei noch einmal, diesmal ohne das Leck: die alte Warnung
    // gehört weg — sie spricht über eine Datei, die es nicht mehr gibt.
    let leaking = app.state.regions.pop().expect("das leckende Rechteck");
    app.export_to(dir.join("gleich.pdf"));
    app.wait_for_export();
    app.wait_for_export_checks();
    assert!(
        leak_warnings(&app).is_empty(),
        "die alte Warnung steht über der neuen Datei: {:?}",
        app.state.warnings
    );
    app.state.regions.push(leaking);

    for i in 0..MAX_WARNED_FILES + 1 {
        app.export_to(dir.join(format!("datei{i}.pdf")));
        app.wait_for_export();
        app.wait_for_export_checks();
    }
    let leaks = leak_warnings(&app);
    assert_eq!(leaks.len(), MAX_WARNED_FILES, "{:?}", app.state.warnings);
    assert!(
        !leaks.iter().any(|w| w.starts_with("datei0.pdf: ")),
        "die älteste Datei muss herausfallen: {:?}",
        app.state.warnings
    );
    assert!(
        leaks
            .iter()
            .any(|w| w.starts_with(&format!("datei{}.pdf: ", MAX_WARNED_FILES))),
        "{:?}",
        app.state.warnings
    );
}

/// Und die Zusage des Doc-Kommentars von `finish_export_check`: die
/// Warnungen bleiben, **bis das nächste Dokument kommt**. Danach nicht mehr —
/// und die Decke zählt im neuen Dokument wieder zehn Dateien voll aus.
/// Mutation (`note_export_warnings` → `self.state.warnings = …`): rot.
#[test]
fn zb_p5d3_der_dokumentwechsel_raeumt_die_warnungen_weg() {
    let dir = tmp("p5d3-wechsel");
    let mut app = leaking_app();
    app.export_to(dir.join("erste.pdf"));
    app.wait_for_export();
    app.wait_for_export_checks();
    assert_eq!(leak_warnings(&app).len(), 1, "{:?}", app.state.warnings);

    app.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "zweites.pdf");
    assert!(
        app.state.warnings.is_empty(),
        "das nächste Dokument räumt sie weg: {:?}",
        app.state.warnings
    );

    // Und die Namensliste ist mit weg: die Decke zählt wieder bei null.
    for i in 0..MAX_WARNED_FILES {
        app.state.regions.push(text_region(
            0,
            Rect::new(40.0, 40.0, 120.0, 60.0),
            "Musterbank",
        ));
        app.export_to(dir.join(format!("neu{i}.pdf")));
        app.wait_for_export();
        app.wait_for_export_checks();
        app.state.regions.pop();
    }
    assert_eq!(
        leak_warnings(&app).len(),
        MAX_WARNED_FILES,
        "{:?}",
        app.state.warnings
    );
}

// ===========================================================================
// P5-D5 — das Neuzeichnen hängt am Ende des Threads, nicht an seinem Erfolg
// ===========================================================================

/// **Befund P5-D5.** `request_repaint` stand als **letzte** Anweisung im
/// Prüf-Thread. Starb er davor (Panik), ruhte die Oberfläche weiter: ohne
/// Bild kein `poll_export_checks`, ohne Abholen kein `Disconnected` — und
/// die Statuszeile blieb auf „Nachprüfung läuft …“, obwohl der Satz
/// „abgebrochen (interner Fehler)“ längst bereitlag. Jetzt fordert ein
/// Wächter das Neuzeichnen beim Fallenlassen an, also auch beim Abwickeln
/// der Panik — der Wächter wird in den Thread **hineingezogen**
/// (`let _repaint = repaint;`) und dort beim Abwickeln fallen gelassen.
/// Mutation (der Wächter zurück zu `Option<egui::Context>` mit
/// `ctx.request_repaint()` als letzter Anweisung): rot.
#[test]
fn zb_p5d5_eine_panik_im_pruefthread_fordert_trotzdem_ein_neuzeichnen() {
    let out = tmp("p5d5-panik").join("geschwaerzt.pdf");
    let ctx = egui::Context::default();
    assert!(!ctx.has_requested_repaint(), "frischer Kontext, nichts an");

    let mut app = demo_app();
    app.ui_ctx = Some(ctx.clone());
    app.force_panic_in_check = true;
    app.export_to(out);
    app.wait_for_export();
    app.wait_for_export_checks();

    assert!(
        ctx.has_requested_repaint(),
        "ohne Neuzeichnen holt niemand das Urteil ab — die Statuszeile bliebe auf „läuft“"
    );
    assert!(
        app.state.status.contains("abgebrochen (interner Fehler)"),
        "{}",
        app.state.status
    );
}

/// Die Gegenrichtung: der geglückte Lauf fordert es genauso an — der
/// Wächter ersetzt den Aufruf am Ende, er kommt nicht zu ihm hinzu.
#[test]
fn zb_p5d5_auch_der_geglueckte_lauf_fordert_ein_neuzeichnen() {
    let out = tmp("p5d5-gut").join("geschwaerzt.pdf");
    let ctx = egui::Context::default();
    assert!(!ctx.has_requested_repaint());

    let mut app = demo_app();
    app.ui_ctx = Some(ctx.clone());
    app.export_to(out);
    app.wait_for_export();
    app.wait_for_export_checks();

    assert!(ctx.has_requested_repaint());
    assert!(
        app.state.status.contains("Nachprüfung:"),
        "{}",
        app.state.status
    );
}

// ===========================================================================
// Q4-5 / Q4-6 — wann das Neuzeichnen kommt, und wem ein Urteil gehört
// ===========================================================================

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

/// **Befund Q4-5.** Das Neuzeichnen wird am **Ende** des Prüf-Threads
/// angefordert, nicht bei seinem Start.
///
/// Die beiden Tests aus Fix-Runde 5 (`zb_p5d5_*`) prüfen nur, **dass**
/// angefordert wurde — beide blieben grün, wenn man `let _repaint = repaint;`
/// aus dem Thread nimmt. Dann fällt der Wächter noch in
/// `start_export_check` auf dem Oberflächen-Thread, das Neuzeichnen kommt
/// beim Start, und der Fehler von P5-D5 wäre zurück (stirbt der Thread
/// später, holt niemand mehr ein Urteil ab). Hier steht der Thread mit dem
/// [`CheckGate`] still, und bis dahin darf **nichts** angefordert sein.
///
/// Mutation (`let _repaint = repaint;` entfernt): rot.
#[test]
fn zb_q4d5_das_neuzeichnen_kommt_erst_am_ende_des_pruefthreads() {
    let out = tmp("q4d5").join("out.pdf");
    // **Zwei Kontexte, weil es zwei Threads sind.** Seit Fix-Runde 8 läuft
    // auch der Export auf einem Thread, und der fordert am Ende zu Recht ein
    // Neuzeichnen an — ohne das holte niemand sein Ergebnis ab. Ein einziger
    // Kontext könnte hinterher nicht mehr sagen, **wer** angefordert hat, und
    // die Aussage dieses Tests gilt dem **Prüf**-Thread. Der Export bekommt
    // deshalb seinen eigenen Kontext; der zweite wird eingehängt, bevor die
    // Nachprüfung überhaupt entsteht (sie entsteht in `poll_exports`, also
    // erst im `wait_for_export` darunter).
    let export_ctx = egui::Context::default();
    let ctx = egui::Context::default();
    assert!(!ctx.has_requested_repaint(), "frischer Kontext, nichts an");

    let gate = Arc::new(crate::app::CheckGate::default());
    let mut app = demo_app();
    app.ui_ctx = Some(export_ctx.clone());
    app.hold_check = Some(gate.clone());
    app.export_to(out);
    app.ui_ctx = Some(ctx.clone());
    app.wait_for_export();

    // Der Thread läuft wirklich — und hängt vor der Suche.
    gate.wait_until_arrived();
    assert!(app.export_check_running());
    assert!(
        !ctx.has_requested_repaint(),
        "das Neuzeichnen wurde schon beim Start angefordert — der Wächter \
         liegt nicht im Thread"
    );

    gate.release();
    app.wait_for_export_checks();
    // **Warten, nicht annehmen.** Der Wächter ([`RepaintOnDrop`]) fällt erst
    // am Ende des Threads, also **nach** dem `send` des Urteils: der
    // Oberflächen-Faden kann das Urteil längst abgeholt haben, während der
    // Thread noch abwickelt. Die Annahme „Urteil da, also Neuzeichnen
    // angefordert“ ist deshalb ein Wettlauf — gemessen: einmal rot in 60
    // vollständigen Läufen der GUI-Sammlung unter Last (Fix-Runde 8,
    // Reproduktionsschleife zu Register #50), mit genau dieser Meldung. Die
    // Aussage des Tests bleibt dieselbe; die Mutation
    // („`let _repaint = repaint;` aus dem Thread genommen“) fällt weiter auf
    // die Prüfung **vor** `release` zurück und ist damit weiter rot.
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ctx.has_requested_repaint() {
        assert!(
            Instant::now() < deadline,
            "am Ende des Threads muss es angefordert sein"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        app.state.status.contains("Nachprüfung:"),
        "{}",
        app.state.status
    );
}

/// **Befund Q4-6.** Ein zweiter Export **derselben** Datei beendet die
/// ältere Prüfung.
///
/// [`ExportCheckPlan::run`] liest die Datei zum Prüfzeitpunkt. Läuft die
/// erste Prüfung noch, wenn die zweite Ausgabe geschrieben ist, bewertet sie
/// die **neuen** Bytes mit dem **alten** Plan — und trägt das Urteil unter
/// dem Präfix des ersten Exports ein. Hier ist die erste Ausgabe sauber und
/// die zweite leck: die alte Prüfung (Begriffe des ersten Exports) fände in
/// den neuen Bytes nichts und meldete über den ersten Export eine
/// Entwarnung, die für keine der beiden Dateien stimmt.
///
/// Mutation (`self.checks.retain(…)` in `start_export_check` entfernt): rot,
/// zwei Urteile statt einem.
#[test]
fn zb_q4d6_ein_zweiter_export_derselben_datei_beendet_die_alte_pruefung() {
    let dir = tmp("q4d6-zweimal");
    let out = dir.join("out.pdf");
    let gate = Arc::new(crate::app::CheckGate::default());

    let mut app = RedactApp::silent(Config {
        no_patterns: true,
        ..Config::default()
    });
    app.open_bytes_and_analyze(&zwei_geheimnisse(), "zwei.pdf");
    app.hold_check = Some(gate.clone());

    // Erster Export: nur Zeile A, Rechteck trifft — die Ausgabe ist sauber.
    app.state
        .regions
        .push(text_region(0, ueber(700.0), "GEHEIM-EINS"));
    let erster = app.state.plan_export_check(&app.state.hit_summary());
    assert_eq!(erster.needles, vec!["GEHEIM-EINS".to_string()]);
    app.export_to(out.clone());
    app.wait_for_export();
    gate.wait_until_arrived();
    assert_eq!(app.checks.len(), 1);

    // Zweiter Export in **dieselbe** Datei: dazu Zeile B, deren Rechteck
    // danebengeht — die neue Ausgabe leckt.
    app.state
        .regions
        .push(text_region(0, daneben(), "GEHEIM-ZWEI"));
    let zweiter = app.state.plan_export_check(&app.state.hit_summary());
    assert_eq!(
        zweiter.needles,
        vec!["GEHEIM-EINS".to_string(), "GEHEIM-ZWEI".to_string()]
    );
    app.export_to(out.clone());
    app.wait_for_export();
    assert_eq!(
        app.checks.len(),
        1,
        "die ältere Prüfung urteilt über Bytes, die es nicht mehr gibt"
    );

    gate.release();
    let urteile = {
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut count = 0;
        while app.export_check_running() {
            assert!(
                Instant::now() < deadline,
                "die Nachprüfung kommt nicht zum Ende"
            );
            count += app.poll_export_checks();
        }
        count
    };
    assert_eq!(urteile, 1, "ein Export, ein Urteil");
    let status = app.state.status.clone();
    println!("Statuszeile: {status}");
    assert!(
        status.contains("1 von 2 gesuchten Text(en) steht NOCH in der Ausgabe"),
        "das Urteil gehört zum zweiten Export: {status}"
    );
    // Und das Orakel: die neue Ausgabe leckt wirklich.
    let bytes = std::fs::read(&out).unwrap();
    assert!(!redact_pdf::leaks(&bytes, "GEHEIM-ZWEI").is_empty());
    assert!(redact_pdf::leaks(&bytes, "GEHEIM-EINS").is_empty());
}

/// **Befund Q4-6, der zweite Fall.** Wird mitten in einer laufenden Prüfung
/// ein **anderes Dokument** geöffnet, kommt ihr Urteil trotzdem an — und
/// bleibt zuzuordnen.
///
/// Fallenlassen wäre falsch: die geschriebene Datei gibt es weiter, und ihr
/// Leck verschwindet nicht dadurch, dass jemand ein anderes Dokument
/// aufmacht. Zuzuordnen ist es über den **Dateinamen**, den Statuszeile
/// (Präfix des Exports) und Warnung mitführen — das neue Dokument heißt
/// anders und hat mit dem Urteil nichts zu tun.
///
/// Mutation (`file` aus `note_check_warning` weg): rot.
#[test]
fn zb_q4d6_ein_dokumentwechsel_verliert_das_urteil_nicht() {
    let out = tmp("q4d6-wechsel").join("erste.pdf");
    let gate = Arc::new(crate::app::CheckGate::default());

    let mut app = RedactApp::silent(Config {
        no_patterns: true,
        ..Config::default()
    });
    app.open_bytes_and_analyze(&zwei_geheimnisse(), "zwei.pdf");
    app.hold_check = Some(gate.clone());
    // Rechteck daneben: die Ausgabe leckt.
    app.state
        .regions
        .push(text_region(0, daneben(), "GEHEIM-EINS"));
    app.export_to(out.clone());
    app.wait_for_export();
    gate.wait_until_arrived();

    // Mitten in der Prüfung ein anderes Dokument.
    app.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "demo.pdf");
    assert!(app.export_check_running(), "die Prüfung läuft weiter");
    assert!(app.state.warnings.is_empty(), "{:?}", app.state.warnings);

    gate.release();
    app.wait_for_export_checks();
    let status = app.state.status.clone();
    println!("Statuszeile: {status}");
    assert!(status.contains("steht NOCH in der Ausgabe"), "{status}");
    assert!(
        status.contains("erste.pdf"),
        "die Datei steht im Satz: {status}"
    );
    assert_eq!(app.state.warnings.len(), 1, "{:?}", app.state.warnings);
    assert!(
        app.state.warnings[0].starts_with("erste.pdf: "),
        "{:?}",
        app.state.warnings
    );
}

// ===========================================================================
// Messungen — `cargo test -p redact-gui zb_mess -- --ignored --nocapture`
// ===========================================================================

/// Befund 1: Suche je Begriff gegen einen Durchgang, und wie lange
/// `export_to` die Oberfläche hält — vorher bis zum Urteil, jetzt bis zum
/// Start des Threads. 305 Seiten, 200 Begriffe, wie beim Gegenprüfer.
#[test]
#[ignore]
fn zb_mess_nachpruefung_je_begriff_gegen_einen_durchgang() {
    let terms = terms(200);
    let pdf = many_pages(305, 40, &terms);
    println!(
        "Datei: {} Seiten, {} Begriffe, {} kB",
        305,
        terms.len(),
        pdf.len() / 1024
    );

    let mut app = RedactApp::silent(Config {
        patterns: Vec::new(),
        ..Config::default()
    });
    app.open_bytes_and_analyze(&pdf, "gross.pdf");
    for (i, term) in terms.iter().enumerate() {
        // Ein Rechteck je Begriff auf der Seite, auf der er zuerst steht.
        let page = i / 40;
        let line = i % 40;
        let y = 800.0 - line as f64 * 18.0;
        app.state.regions.push(text_region(
            page,
            Rect::new(72.0, y - 2.0, 400.0, y + 10.0),
            term,
        ));
    }
    let summary = app.state.hit_summary();
    assert_eq!(summary.redacted, 200);
    let out = tmp("zb-mess-1").join("out.pdf");

    let t = Instant::now();
    app.export_to(out.clone());
    app.wait_for_export();
    let export_returns = t.elapsed();
    let t = Instant::now();
    app.wait_for_export_checks();
    let verdict_arrives = t.elapsed();
    println!(
        "export_to kehrt zurück nach {export_returns:?}; Urteil nach weiteren {verdict_arrives:?}"
    );
    println!("Statuszeile: {}", app.state.status);

    let bytes = std::fs::read(&out).unwrap();
    let needles: Vec<&str> = terms.iter().map(String::as_str).collect();
    let t = Instant::now();
    let one_pass = redact_pdf::leaks_many(&bytes, &needles);
    let one_pass_time = t.elapsed();
    let t = Instant::now();
    let per_needle: Vec<Vec<String>> = needles
        .iter()
        .map(|n| redact_pdf::leaks(&bytes, n))
        .collect();
    let per_needle_time = t.elapsed();
    assert_eq!(one_pass, per_needle);
    println!(
        "je Begriff (vorher): {per_needle_time:?}; ein Durchgang (jetzt): {one_pass_time:?}; Verhältnis {:.1}",
        per_needle_time.as_secs_f64() / one_pass_time.as_secs_f64()
    );

    // Befund 4 (Fix-Runde 3): die Zahl für den Doc-Kommentar an
    // `ExportCheckPlan::run` — ein Durchgang mit 1, 200 und
    // `MAX_CHECK_NEEDLES` Begriffen an derselben Datei. Jeder Wert ist der
    // schnellste von drei Läufen, damit ein Nachbar auf der Maschine nicht
    // die Messung schreibt.
    let padded = self::terms(MAX_CHECK_NEEDLES); // `terms` ist oben die Liste
    let fastest = |n: usize| {
        let needles: Vec<&str> = padded[..n].iter().map(String::as_str).collect();
        (0..3)
            .map(|_| {
                let t = Instant::now();
                let hits = redact_pdf::leaks_many(&bytes, &needles);
                assert_eq!(hits.len(), n);
                t.elapsed()
            })
            .min()
            .unwrap()
    };
    let one = fastest(1);
    let two_hundred = fastest(200);
    let ceiling = fastest(MAX_CHECK_NEEDLES);
    println!(
        "ein Durchgang, {} kB: 1 Begriff {one:?}; 200 Begriffe {two_hundred:?}; \
         {MAX_CHECK_NEEDLES} Begriffe {ceiling:?}; je Begriff über dem Sockel {:.2} ms",
        bytes.len() / 1024,
        (ceiling.as_secs_f64() - one.as_secs_f64()) * 1000.0 / (MAX_CHECK_NEEDLES - 1) as f64
    );

    // Die alte Decke (Begriffe × Dateibytes ≤ 2 GiB) ließ an 100 kB rund
    // 20 000 Begriffe zu und versprach dafür „≈ 2 s“. Gemessen an einer
    // Datei dieser Größe, einmal, weil ein Lauf reicht, um die Zusage zu
    // prüfen.
    let small = many_pages(30, 40, &self::terms(50));
    let many: Vec<String> = self::terms(20_000);
    let needles: Vec<&str> = many.iter().map(String::as_str).collect();
    let allowed = (2u64 * 1024 * 1024 * 1024) / small.len() as u64;
    let t = Instant::now();
    let hits = redact_pdf::leaks_many(&small, &needles);
    let elapsed = t.elapsed();
    assert_eq!(hits.len(), needles.len());
    println!(
        "alte Decke: {} kB ließen {allowed} Begriffe zu; 20 000 Begriffe brauchen {elapsed:?} \
         (versprochen waren ≈ 2 s)",
        small.len() / 1024
    );

    // Dieselbe Datei mit gepackten Strömen: auf der Platte ein Bruchteil, die
    // Arbeit dieselbe — denn gesucht wird in den **entpackten** Bytes. Die
    // alte Decke hätte an der gepackten Datei entsprechend mehr Begriffe
    // zugelassen, bei gleichen Kosten je Begriff.
    let mut doc = redact_pdf::load_from_bytes(&pdf).unwrap();
    for (_, object) in doc.objects.iter_mut() {
        if let lopdf::Object::Stream(stream) = object {
            let _ = stream.compress();
        }
    }
    let packed = redact_pdf::save_to_bytes(&doc).unwrap();
    let needles: Vec<&str> = padded.iter().map(String::as_str).collect();
    let time = |bytes: &[u8]| {
        (0..3)
            .map(|_| {
                let t = Instant::now();
                let hits = redact_pdf::leaks_many(bytes, &needles);
                assert_eq!(hits.len(), needles.len());
                t.elapsed()
            })
            .min()
            .unwrap()
    };
    let plain_time = time(&pdf);
    let packed_time = time(&packed);
    let budget = 2u64 * 1024 * 1024 * 1024;
    println!(
        "gepackt: {} kB statt {} kB (Faktor {:.1}); {MAX_CHECK_NEEDLES} Begriffe: {packed_time:?} \
         gepackt gegen {plain_time:?} ungepackt; die alte Decke ließ gepackt {} statt {} Begriffe zu",
        packed.len() / 1024,
        pdf.len() / 1024,
        pdf.len() as f64 / packed.len() as f64,
        budget / packed.len() as u64,
        budget / pdf.len() as u64
    );
}

/// Befund 2: 300 Seiten Kleinbilder — Bytes vorher (nie verworfen) und jetzt.
#[test]
#[ignore]
fn zb_mess_kleinbilder_300_seiten() {
    let pages = 300;
    let ctx = egui::Context::default();
    let doc = redact_pdf::load_from_bytes(&many_pages(pages, 3, &terms(3))).unwrap();
    let mut cache = PageCache::new();
    cache.set_document(Arc::new(doc));
    let t = Instant::now();
    for page in 0..pages {
        assert!(cache.request_thumb(page, &ctx));
        wait_for(&mut cache, &ctx, |c| c.has_thumb(page));
    }
    let held = (0..pages).filter(|p| cache.has_thumb(*p)).count();
    let bytes = thumb_texture_bytes(&cache, pages);
    println!(
        "{pages} Seiten in {:?}: {held} Kleinbilder gehalten, {} MB (Decke {} MB)",
        t.elapsed(),
        bytes / 1_000_000,
        MAX_THUMB_BYTES / 1_000_000
    );
}

/// Befund 3: Zeit je Bild für 500, 10 000 und 50 000 Trefferzeilen.
#[test]
#[ignore]
fn zb_mess_trefferliste_je_bild() {
    for n in [500usize, 10_000, 50_000] {
        let (state, summary) = many_rows_state(n);
        let state = RefCell::new(state);
        let ctx = egui::Context::default();
        draw_sidebar(&ctx, &state, &summary);
        let mut best = Duration::MAX;
        for _ in 0..5 {
            let t = Instant::now();
            draw_sidebar(&ctx, &state, &summary);
            best = best.min(t.elapsed());
        }
        let galleys = ctx.fonts(|f| f.num_galleys_in_cache());
        println!("{n} Zeilen: {best:?} je Bild (bestes von 5), {galleys} gesetzte Texte");
    }
}
