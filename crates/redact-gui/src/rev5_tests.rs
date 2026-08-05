//! Prüfrunde 5 — Bedienfolgen, die zu einer *nicht* geschwärzten IBAN führen.
//!
//! Kindmodul von [`crate::app`], damit die privaten Wege (`apply_pointer`,
//! `RedactApp::silent`/`refusing`, `resize`) erreichbar sind.
//!
//! Jeder Test hier nagelt einen Befund fest: er ist gegen die reparierte Stelle
//! grün und gegen die alte rot. Die Gegenproben stehen jeweils im selben Test —
//! sie zeigen, dass die Behauptung wirklich an der geänderten Zeile hängt und
//! nicht an einer Nebensache.

use super::*;

use redact_core::{MatchType, Rect, Region, Source};

use crate::state::{AnnotatedRegion, HitOutcome};

// --------------------------------------------------------------- Hilfsmittel

fn iban_only() -> Config {
    Config {
        patterns: vec!["iban_de".to_string()],
        ..Config::default()
    }
}

/// Eine Anwendung mit geladenem Demo-Kontoauszug und gelaufener Analyse.
/// Rückfragen beantwortet sich `Ask::Answer(yes)` selbst.
fn loaded(yes: bool) -> RedactApp {
    let mut app = if yes {
        RedactApp::silent(iban_only())
    } else {
        RedactApp::refusing(iban_only())
    };
    app.state
        .load_bytes(
            &redact_pdf::testing::demo_statement(),
            Some(PathBuf::from("demo.pdf")),
        )
        .expect("Demo-PDF ladbar");
    app.state.analyze().expect("Analyse läuft");
    app
}

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rev5-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Tastendruck, wie ihn `read_keys` liefern würde.
fn keys(f: impl FnOnce(&mut KeyState)) -> KeyState {
    let mut k = KeyState::default();
    f(&mut k);
    k
}

/// Eine Taste durch die **ganze** Kette schicken: Übersetzung + Ausführung.
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

// ===========================================================================
// B1 — Pfeiltasten sind auf das Blatt geklemmt
// ===========================================================================

/// **Befund B1, behoben.** [`AppState::move_selected`] verschob das Rechteck
/// ohne jede Grenze: hundert Anschläge auf Umschalt+Pfeil links legten es
/// vollständig neben eine 595 pt breite Seite. Zu sehen war davon nichts
/// (`ui.painter_at` schneidet weg), die Kopfzeile zählte es weiter als „wird
/// geschwärzt“, und der Export meldete den Fehlschlag erst hinterher als
/// Warnung.
///
/// Jetzt stößt das Rechteck am Blattrand an — **ohne dabei zu schrumpfen**.
/// Das ist der Unterschied zwischen Schieben und Ziehen: schnitte das Schieben
/// wie der Eckgriff, würde der schwarze Balken am Rand mit jedem weiteren
/// Anschlag schmaler, und ein Stück der IBAN käme darunter hervor.
#[test]
fn b1_arrow_keys_stop_at_the_edge_of_the_sheet_instead_of_leaving_it() {
    let mut app = loaded(true);
    // Zwei IBAN-Treffer: Seite 1 und Seite 2.
    assert_eq!(app.state.regions.len(), 2);
    app.state.selected_region = Some(0);

    let sheet = app.state.page_box(0).expect("Seite 0");
    let before = app.state.regions[0].region.rect;
    assert!(before.ur.x <= sheet.ur.x && before.ll.x >= sheet.ll.x);

    // 100× Umschalt+Pfeil links = 1000 pt nach links gewollt.
    for _ in 0..100 {
        press(
            &mut app,
            keys(|k| {
                k.left = true;
                k.shift = true;
            }),
        );
    }

    let after = app.state.regions[0].region.rect;
    assert_eq!(
        after.ll.x, sheet.ll.x,
        "das Rechteck steht am linken Blattrand: {after:?} vs. Blatt {sheet:?}"
    );
    assert!(
        (after.width() - before.width()).abs() < 1e-9,
        "und es ist dabei nicht geschrumpft: {} → {}",
        before.width(),
        after.width()
    );

    // Dasselbe nach unten: 100× Umschalt+Pfeil ab.
    for _ in 0..100 {
        press(
            &mut app,
            keys(|k| {
                k.down = true;
                k.shift = true;
            }),
        );
    }
    let corner = app.state.regions[0].region.rect;
    assert_eq!(corner.ll.y, sheet.ll.y, "und am unteren Rand: {corner:?}");
    assert!((corner.height() - before.height()).abs() < 1e-9);

    // Die Kopfzeile verspricht damit nichts Unhaltbares mehr: was sie zählt,
    // liegt auf dem Blatt.
    let summary = app.state.hit_summary();
    assert_eq!(summary.outcome(0), HitOutcome::Redacted);
    assert_eq!(summary.redacted, 2);
    assert_eq!(summary.off_page, 0);
    assert!(
        summary.headline().contains("2 werden geschwärzt"),
        "Kopfzeile: {}",
        summary.headline()
    );

    // Und das, was gezählt wird, ist im Seitenbild auch zu sehen — geprüft an
    // der Fläche, die `paint_page` mit `ui.painter_at(area)` beschneidet.
    let view = app.state.page_view(0);
    let sheet_screen = egui::Rect::from_min_size(ORIGIN, view.size_screen(ZOOM));
    let region_screen = viewer::pdf_to_screen(&corner, &view, ZOOM, ORIGIN);
    assert!(
        sheet_screen.contains_rect(region_screen),
        "die gezählte Fläche liegt ganz auf dem gezeichneten Blatt: \
         {region_screen:?} vs. Blatt {sheet_screen:?}"
    );

    // Gegenprobe: kleine Schritte innerhalb des Blattes werden **nicht**
    // angefasst — geklemmt wird nur am Rand.
    let mut free = loaded(true);
    free.state.selected_region = Some(0);
    let start = free.state.regions[0].region.rect;
    press(
        &mut free,
        keys(|k| {
            k.right = true;
            k.shift = true;
        }),
    );
    let moved = free.state.regions[0].region.rect;
    assert!(
        (moved.ll.x - start.ll.x - crate::app::NUDGE_FAST).abs() < 1e-9,
        "ein Anschlag mitten auf dem Blatt verschiebt um genau {} pt: {start:?} → {moved:?}",
        crate::app::NUDGE_FAST
    );

    // Gegenprobe 2: derselbe Weg über einen Eckgriff **ist** ebenfalls
    // geklemmt — dort allerdings beschneidend, weil Ziehen etwas anderes ist
    // als Schieben.
    let view = free.state.page_view(0);
    let screen = viewer::pdf_to_screen(&moved, &view, ZOOM, ORIGIN);
    let grip = screen.left_top();
    free.apply_pointer(press_frame(grip), false, &view, ORIGIN, 0, ZOOM);
    free.apply_pointer(
        dragging_frame(grip, grip - Vec2::new(5000.0, 0.0)),
        false,
        &view,
        ORIGIN,
        0,
        ZOOM,
    );
    free.apply_pointer(
        release_frame(grip - Vec2::new(5000.0, 0.0)),
        false,
        &view,
        ORIGIN,
        0,
        ZOOM,
    );
    let clamped = free.state.regions[0].region.rect;
    assert!(
        clamped.ll.x >= sheet.ll.x - 0.001,
        "der Eckgriff wird geklemmt: {clamped:?}"
    );
}

/// **Befund B1, die Gegenrichtung.** Die Klemmung darf den *gewöhnlichen* Fall
/// nicht anfassen: mitten auf dem Blatt ist ein Anschlag genau ein Anschlag,
/// hin und zurück landet exakt am Ausgangspunkt, und der Export wirkt
/// unverändert. Eine zu grobe Klemmung — etwa eine, die jedes Rechteck an den
/// Blattursprung zöge — fiele hier auf.
#[test]
fn b1_a_small_nudge_lands_exactly_where_it_started() {
    let mut app = loaded(true);
    app.state.selected_region = Some(0);
    let before = app.state.regions[0].region.rect;
    // Ein kleiner Schritt nach oben und wieder zurück: die Korrektur, die eine
    // Nutzerin wirklich macht.
    press(&mut app, keys(|k| k.up = true));
    let up = app.state.regions[0].region.rect;
    assert!(
        (up.ll.y - before.ll.y - crate::app::NUDGE).abs() < 1e-9,
        "genau {} pt nach oben: {before:?} → {up:?}",
        crate::app::NUDGE
    );
    press(&mut app, keys(|k| k.down = true));
    assert_eq!(
        app.state.regions[0].region.rect, before,
        "und wieder genau zurück"
    );

    let dir = tmp("b1-effect");
    let outcome = app
        .state
        .export(&dir.join("out.pdf"), None)
        .expect("Export läuft");
    assert_eq!(outcome.redactions, 2);
    assert_eq!(
        outcome.effective_redactions, 2,
        "beide Schwärzungen treffen Text"
    );
    assert!(
        !outcome
            .warnings
            .iter()
            .any(|w| w.contains("kein einziges Zeichen")),
        "Warnungen: {:?}",
        outcome.warnings
    );
}

// ===========================================================================
// B2 — Ein geschobener Treffer ist Handarbeit
// ===========================================================================

/// **Befund B2, behoben.** Ein mit den Pfeiltasten verschobener Treffer ist
/// jetzt Handarbeit ([`AnnotatedRegion::is_hand_made`]) — „Analysieren“ fragt
/// vorher nach, statt die Korrektur stillschweigend wegzuwerfen. Es war der
/// einzige der fünf Wege zum selben Verlust, der nicht fragte.
///
/// Gegenprobe im selben Test: dieselbe Korrektur über den Eckgriff verhält
/// sich gleich — das war schon vorher so und muss so bleiben.
#[test]
fn b2_a_nudged_hit_is_hand_work_so_analysis_asks_first() {
    // (1) Pfeiltasten.
    let mut nudged = loaded(false); // jede Rückfrage endet mit „Nein“
    nudged.state.selected_region = Some(0);
    let original = nudged.state.regions[0].region.rect;
    for _ in 0..3 {
        press(
            &mut nudged,
            keys(|k| {
                k.up = true;
                k.shift = true;
            }),
        );
    }
    let corrected = nudged.state.regions[0].region.rect;
    assert_ne!(corrected, original, "die Korrektur ist angekommen");
    assert!(
        matches!(nudged.state.regions[0].region.source, Source::Manual { .. }),
        "ein von Hand verschobenes Rechteck ist nicht mehr das, was das Muster fand"
    );
    assert!(nudged.state.has_manual_work());
    assert_eq!(nudged.state.hand_made_count(), 1);

    // Analysieren wird abgelehnt, weil `Ask::Answer(false)` die Rückfrage mit
    // „Nein“ beantwortet — und die Korrektur steht noch.
    assert!(
        !nudged.analyze(),
        "vor dem Verwerfen wird gefragt, hier mit Nein beantwortet"
    );
    assert_eq!(nudged.state.regions[0].region.rect, corrected);

    // Mit „Ja“ überlebt sie sogar: eine erneute Analyse ersetzt nur die
    // automatisch gefundenen Regionen.
    let mut keeping = loaded(true);
    keeping.state.selected_region = Some(0);
    press(
        &mut keeping,
        keys(|k| {
            k.up = true;
            k.shift = true;
        }),
    );
    let kept = keeping.state.regions[0].region.rect;
    assert!(keeping.analyze());
    assert!(
        keeping
            .state
            .regions
            .iter()
            .any(|a| a.region.rect == kept && matches!(a.region.source, Source::Manual { .. })),
        "die von Hand geschobene Region hat die Analyse überlebt"
    );

    // (2) Gegenprobe: dieselbe Korrektur über den Eckgriff.
    let mut dragged = loaded(false);
    dragged.state.selected_region = Some(0);
    let view = dragged.state.page_view(0);
    let screen = viewer::pdf_to_screen(&original, &view, ZOOM, ORIGIN);
    let corner = screen.left_top();
    let target = corner - Vec2::new(0.0, 3.0);
    dragged.apply_pointer(press_frame(corner), false, &view, ORIGIN, 0, ZOOM);
    dragged.apply_pointer(
        dragging_frame(corner, target),
        false,
        &view,
        ORIGIN,
        0,
        ZOOM,
    );
    dragged.apply_pointer(release_frame(target), false, &view, ORIGIN, 0, ZOOM);
    assert_ne!(dragged.state.regions[0].region.rect, original);
    assert!(dragged.state.has_manual_work());
    assert!(!dragged.analyze());
}

/// **Befund B2, Anhang.** Was für den Eckgriff gilt, gilt jetzt auch für die
/// Pfeiltasten: wird ein durch die Schutzliste gedeckter Treffer angefasst,
/// überstimmt er die Schutzliste — und **das wird angesagt**
/// ([`crate::state::PROTECTION_OVERRIDDEN`]). Vorher änderte sich die Wirkung
/// beim Schieben genauso, nur stand es nirgends.
#[test]
fn b2_nudging_a_protected_hit_announces_that_it_overrides_the_protection() {
    let mut app = loaded(true);
    let hit = app.state.regions[0].region.rect;
    app.state.regions.push(AnnotatedRegion::new(Region::new(
        0,
        Rect::new(
            hit.ll.x - 2.0,
            hit.ll.y - 2.0,
            hit.ur.x + 2.0,
            hit.ur.y + 2.0,
        ),
        Some("Max Mustermann".into()),
        Source::Booking {
            booking_id: "b003".into(),
            match_type: MatchType::Negative,
        },
    )));
    assert_eq!(app.state.hit_summary().outcome(0), HitOutcome::Blocked);

    app.state.selected_region = Some(0);
    press(&mut app, keys(|k| k.right = true));

    assert_eq!(app.state.hit_summary().outcome(0), HitOutcome::Redacted);
    assert_eq!(app.state.status, crate::state::PROTECTION_OVERRIDDEN);

    // Strg+Z nimmt es zurück.
    press(
        &mut app,
        keys(|k| {
            k.ctrl = true;
            k.key_z = true;
        }),
    );
    assert_eq!(app.state.hit_summary().outcome(0), HitOutcome::Blocked);
}

// ===========================================================================
// B3 — Eine Schiebe-Sitzung ist ein Verlaufsschritt
// ===========================================================================

/// **Befund B3, behoben.** Jeder einzelne Pfeiltastendruck legte einen eigenen
/// Schnappschuss ab. Bei [`crate::HISTORY_LIMIT`] = 50 genügten 50 Antipper —
/// mit Tastenwiederholung rund eine Sekunde —, um **jeden** älteren Stand
/// hinauszuschieben, auch den vor einem versehentlichen Löschen.
///
/// Jetzt ist eine Schiebe-Sitzung **ein** Schritt, genau wie eine Tippsitzung
/// im Ersatzfeld ([`AppState::edit_replacement`]) und ein Zug am Eckgriff
/// ([`AppState::begin_manual_edit`]).
#[test]
fn b3_fifty_arrow_taps_are_one_step_in_the_history() {
    let mut app = loaded(true);

    // Ein zweites, von Hand gezogenes Rechteck — das ist die Arbeit, die
    // beinahe verloren ging.
    let hits = app.state.regions.len();
    app.state
        .add_manual_region(0, Rect::new(60.0, 60.0, 200.0, 80.0), "Adresse");
    assert_eq!(app.state.regions.len(), hits + 1);

    // Versehentlich gelöscht.
    app.state.selected_region = Some(hits);
    assert!(app.state.delete_selected());
    assert_eq!(app.state.regions.len(), hits);

    // Der Fehler fällt nicht sofort auf; erst wird der verbliebene Treffer mit
    // den Pfeiltasten zurechtgerückt.
    app.state.selected_region = Some(0);
    let before = app.state.history.undo_depth();
    for _ in 0..crate::HISTORY_LIMIT {
        press(&mut app, keys(|k| k.right = true));
    }
    assert_eq!(
        app.state.history.undo_depth(),
        before + 1,
        "fünfzig Anschläge auf dieselbe Region sind ein Schritt"
    );

    // Und der Stand vor dem Löschen ist wieder erreichbar.
    let mut best = 0usize;
    while app.state.can_undo() {
        app.state.undo();
        best = best.max(app.state.regions.len());
    }
    assert_eq!(
        best,
        hits + 1,
        "das versehentlich gelöschte Rechteck ist über den Stapel wieder erreichbar"
    );
}

/// **Befund B3, Kehrseite.** Eine Sitzung darf nicht *zu* weit reichen: was
/// zwischen zwei Anschlägen passiert, muss sie beenden. Sonst zöge ein
/// Rückgängig an der dazwischenliegenden Änderung vorbei.
#[test]
fn b3_anything_in_between_starts_a_new_nudge_session() {
    let mut app = loaded(true);
    app.state.selected_region = Some(0);

    press(&mut app, keys(|k| k.right = true));
    assert!(app.state.is_nudging());
    let after_first = app.state.history.undo_depth();

    // Dazwischen etwas anderes: den zweiten Treffer abwählen.
    assert!(app.state.set_enabled(1, false));
    assert!(!app.state.is_nudging(), "das beendet die Schiebe-Sitzung");
    let after_toggle = app.state.history.undo_depth();
    assert_eq!(after_toggle, after_first + 1);

    press(&mut app, keys(|k| k.right = true));
    assert_eq!(
        app.state.history.undo_depth(),
        after_toggle + 1,
        "der nächste Anschlag ist ein neuer Schritt"
    );

    // Ein Schritt zurück nimmt nur das Schieben zurück; das Abwählen steht
    // noch.
    app.state.undo();
    assert!(!app.state.regions[1].enabled);

    // Eine andere Region zu schieben ist ebenfalls eine neue Sitzung.
    let mut other = loaded(true);
    other.state.selected_region = Some(0);
    press(&mut other, keys(|k| k.right = true));
    let depth = other.state.history.undo_depth();
    other.state.selected_region = Some(1);
    press(&mut other, keys(|k| k.right = true));
    assert_eq!(other.state.history.undo_depth(), depth + 1);
}

/// **Befund B3, Nachbarschaft.** Am Blattrand angekommen ändert ein weiterer
/// Anschlag nichts — dann gehört auch nichts in den Verlauf. Sonst kostete
/// jeder Anschlag gegen den Rand ein „Rückgängig“, das sichtbar nichts
/// zurücknimmt.
#[test]
fn b3_pressing_against_the_edge_costs_no_history_step() {
    let mut app = loaded(true);
    app.state.selected_region = Some(0);
    for _ in 0..100 {
        press(
            &mut app,
            keys(|k| {
                k.left = true;
                k.shift = true;
            }),
        );
    }
    let at_the_wall = app.state.regions[0].region.rect;
    // Erst die laufende Sitzung beenden — sonst wäre die Aussage trivial: eine
    // laufende Sitzung legt ohnehin keinen zweiten Schnappschuss ab. Geprüft
    // werden soll, dass ein Anschlag gegen den Rand auch **ohne** Sitzung
    // keinen Schritt kostet. (`end_replacement_edit` ist der Weg, den die
    // Seitenleiste in jedem Bild ohne Feldfokus geht.)
    app.state.end_replacement_edit();
    assert!(!app.state.is_nudging());
    let depth = app.state.history.undo_depth();
    for _ in 0..10 {
        press(
            &mut app,
            keys(|k| {
                k.left = true;
                k.shift = true;
            }),
        );
    }
    assert_eq!(app.state.regions[0].region.rect, at_the_wall);
    assert_eq!(
        app.state.history.undo_depth(),
        depth,
        "gegen den Rand drücken legt keinen Schnappschuss ab"
    );
}

// ===========================================================================
// B4 — Seitenwechsel mitten im Zug am Eckgriff
// ===========================================================================

/// **Befund B4, behoben.** Bild ab / Pos1 / Ende blättern **immer**, auch mit
/// gedrückter Maustaste. Ein laufender Zug am Eckgriff lief danach weiter:
/// `apply_pointer` bekam die Geometrie der **neuen** Seite, die Region lag aber
/// auf der alten und war nicht mehr zu sehen — sie änderte sich blind mit, und
/// die Statuszeile meldete „Rechteck angepasst“.
///
/// Die [`crate::selector::HandleDrag`]-Kennung schützte vor der falschen
/// Region, nicht vor der falschen **Seite**. Jetzt endet der Zug genauso wie
/// bei verschwundener Kennung.
#[test]
fn b4_turning_the_page_during_a_handle_drag_ends_it() {
    let mut app = loaded(true);
    assert_eq!(app.state.page_count(), 2, "das Demo-PDF hat zwei Seiten");

    app.state.selected_region = Some(0);
    assert_eq!(app.state.regions[0].region.page, 0);
    let before = app.state.regions[0].region.rect;

    let view0 = app.state.page_view(0);
    let screen = viewer::pdf_to_screen(&before, &view0, ZOOM, ORIGIN);
    let corner = screen.left_top();

    // Griff anfassen.
    app.apply_pointer(press_frame(corner), false, &view0, ORIGIN, 0, ZOOM);
    assert!(app.resize.is_some(), "der Griff ist gefasst");

    // Bild ab — die Taste wirkt, obwohl die Maustaste unten ist.
    let commands = key_commands(keys(|k| k.page_down = true), true);
    assert_eq!(commands, vec![KeyCommand::NextPage]);
    app.apply_key_commands(&commands);
    assert_eq!(app.state.current_page, 1, "die Anzeige steht auf Seite 2");

    // Weiterziehen; die Oberfläche zeichnet jetzt Seite 2 und ruft
    // `apply_pointer` mit deren Geometrie.
    let view1 = app.state.page_view(1);
    let target = corner + Vec2::new(120.0, 90.0);
    app.apply_pointer(
        dragging_frame(corner, target),
        false,
        &view1,
        ORIGIN,
        1,
        ZOOM,
    );

    assert!(app.resize.is_none(), "der Zug ist beendet");
    assert_eq!(
        app.state.regions[0].region.rect, before,
        "die unsichtbare Region auf Seite 1 ist unverändert"
    );
    assert!(
        app.state.status.contains("anderen Seite"),
        "und es steht in der Statuszeile: {}",
        app.state.status
    );

    // Auch spätere Mausbilder schreiben nichts mehr.
    app.apply_pointer(release_frame(target), false, &view1, ORIGIN, 1, ZOOM);
    assert_eq!(app.state.regions[0].region.rect, before);
    assert_ne!(app.state.status, "Rechteck angepasst");

    // Gegenprobe: **ohne** Seitenwechsel läuft derselbe Zug ganz normal durch.
    let mut fine = loaded(true);
    fine.state.selected_region = Some(0);
    let view = fine.state.page_view(0);
    let screen = viewer::pdf_to_screen(&before, &view, ZOOM, ORIGIN);
    let corner = screen.left_top();
    let target = corner - Vec2::new(6.0, 6.0);
    fine.apply_pointer(press_frame(corner), false, &view, ORIGIN, 0, ZOOM);
    fine.apply_pointer(
        dragging_frame(corner, target),
        false,
        &view,
        ORIGIN,
        0,
        ZOOM,
    );
    fine.apply_pointer(release_frame(target), false, &view, ORIGIN, 0, ZOOM);
    assert_ne!(fine.state.regions[0].region.rect, before);
    assert_eq!(fine.state.status, "Rechteck angepasst");
}

// ===========================================================================
// B5 — Zwei gleiche Regionen, verschiedene Schwärzungsart
// ===========================================================================

/// **Befund B5, behoben.** [`AppState::enabled_redactions`] suchte die
/// Schwärzungsart über `find(|a| a.region == region)` — also über die **erste**
/// gleiche Zeile, nicht über die, die die Konfliktauflösung behalten hat. Sind
/// zwei Einträge deckungsgleich und trägt der abgewählte eine andere Art,
/// exportierte die Oberfläche die Art des abgewählten. *Ob* geschwärzt wird,
/// stimmte; *wie*, nicht.
///
/// Jetzt gilt für beide dieselbe Vorauswahl wie in `conflict_input`.
#[test]
fn b5_the_action_of_the_selected_twin_wins_not_the_deselected_one() {
    let mut app = loaded(true);
    let region = app.state.regions[0].region.clone();
    let twin = app.state.regions.len();

    // Zwilling: derselbe Bereich, dieselbe Herkunft — so entsteht er beim
    // Laden einer Review-Datei mit doppeltem Eintrag.
    app.state.regions.push(AnnotatedRegion::new(region.clone()));

    // Zeile 1 wird abgewählt und trägt „Schwarz“; der Zwilling bleibt an und
    // soll ersetzt werden.
    assert!(app.state.set_enabled(0, false));
    assert!(app
        .state
        .set_action(twin, redact_core::Action::Replace("[IBAN]".into())));

    let summary = app.state.hit_summary();
    assert_eq!(summary.outcome(0), HitOutcome::Disabled);
    assert_eq!(
        summary.outcome(twin),
        HitOutcome::Redacted,
        "die Oberfläche zeigt: der Zwilling wird geschwärzt"
    );

    let shown = app.state.regions[twin].action.clone();
    let exported = app
        .state
        .enabled_redactions()
        .into_iter()
        .find(|r| r.region == region)
        .expect("die Stelle wird geschwärzt");
    assert_eq!(
        exported.action, shown,
        "exportiert wird, was in der Zeile steht, die geschwärzt wird"
    );
    assert_eq!(shown, redact_core::Action::Replace("[IBAN]".into()));

    // Gegenprobe: ist der **erste** der beiden derjenige, der geschwärzt wird,
    // gewinnt selbstverständlich seine Art.
    let mut other = loaded(true);
    let region = other.state.regions[0].region.clone();
    let twin = other.state.regions.len();
    other
        .state
        .regions
        .push(AnnotatedRegion::new(region.clone()));
    assert!(other.state.set_enabled(twin, false));
    assert!(other.state.set_action(0, redact_core::Action::Whiteout));
    let exported = other
        .state
        .enabled_redactions()
        .into_iter()
        .find(|r| r.region == region)
        .expect("die Stelle wird geschwärzt");
    assert_eq!(exported.action, redact_core::Action::Whiteout);
}

// ===========================================================================
// B6 — Die Zahl an der Miniaturansicht
// ===========================================================================

/// Alle Texte, die ein Bild wirklich malt.
fn painted_texts(output: &egui::FullOutput) -> Vec<String> {
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.text().to_string()),
            egui::Shape::Vec(shapes) => {
                for s in shapes {
                    walk(s, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in &output.shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

/// Ein Bild der Miniaturspalte.
fn thumbnail_frame(app: &std::cell::RefCell<RedactApp>) -> Vec<String> {
    let ctx = egui::Context::default();
    let mut texts = Vec::new();
    for _ in 0..3 {
        let summary = app.borrow().state.hit_summary();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::SidePanel::left("thumbnails").show(ctx, |ui| {
                let mut app = app.borrow_mut();
                let RedactApp { state, pages, .. } = &mut *app;
                crate::thumbnails::show(ui, state, pages, &summary, false);
            });
        });
        texts = painted_texts(&output);
    }
    texts
}

/// **Befund B6, behoben.** Die Zahl neben einer Miniaturansicht kam aus
/// [`AppState::regions_on_page`] und zählte **jede** Zeile der Seite mit:
/// abgewählte, blockierte, doppelte und Schutzeinträge. Ihre Sprechblase hieß
/// „Treffer auf dieser Seite“.
///
/// Genau dieser Fehler war in der Kopfzeile der Seitenleiste schon abgestellt
/// („Schutzeinträge sind keine Funde“) — in der Miniaturspalte stand er noch,
/// und dort ist er gefährlicher: sie ist die Übersicht, mit der man am Ende
/// durchgeht, ob auf keiner Seite etwas stehen geblieben ist.
///
/// Aufbau so, dass die Zahl nicht mit einer Seitenzahl zu verwechseln ist: das
/// Dokument hat zwei Seiten, die falsche Zahl an Seite 1 wäre die **5**, die
/// richtige in der Gegenprobe die **4**.
#[test]
fn b6_the_thumbnail_badge_counts_redactions_not_rows() {
    let mut app = loaded(true);
    let hit = app.state.regions[0].region.rect;

    // Ein Schutzeintrag deckt den IBAN-Treffer auf Seite 1 vollständig …
    app.state.regions.push(AnnotatedRegion::new(Region::new(
        0,
        Rect::new(
            hit.ll.x - 2.0,
            hit.ll.y - 2.0,
            hit.ur.x + 2.0,
            hit.ur.y + 2.0,
        ),
        Some("Max Mustermann".into()),
        Source::Booking {
            booking_id: "b003".into(),
            match_type: MatchType::Negative,
        },
    )));
    // … und drei von Hand gezogene, aber wieder abgewählte Rechtecke.
    for i in 0..3 {
        let y = 200.0 + 30.0 * i as f64;
        let index = app
            .state
            .add_manual_region(0, Rect::new(100.0, y, 200.0, y + 12.0), "Adresse")
            .expect("angelegt");
        assert!(app.state.set_enabled(index, false));
    }

    // Auf Seite 1 wird damit **nichts** geschwärzt — bei fünf Zeilen.
    let summary = app.state.hit_summary();
    assert_eq!(app.state.regions_on_page(0).len(), 5);
    assert_eq!(summary.outcome(0), HitOutcome::Blocked);
    assert_eq!(app.state.redactions_per_page(&summary), vec![0, 1]);

    let texts = thumbnail_frame(&std::cell::RefCell::new(app));
    assert!(
        !texts.contains(&"5".to_string()),
        "an Seite 1 darf keine 5 stehen — gemalt wurde: {texts:?}"
    );

    // Gegenprobe: sind dieselben drei Rechtecke angehakt und fehlt der
    // Schutzeintrag, steht an Seite 1 die 4 — drei Handrechtecke plus der
    // IBAN-Treffer.
    let mut counting = loaded(true);
    for i in 0..3 {
        let y = 200.0 + 30.0 * i as f64;
        counting
            .state
            .add_manual_region(0, Rect::new(100.0, y, 200.0, y + 12.0), "Adresse")
            .expect("angelegt");
    }
    let summary = counting.state.hit_summary();
    assert_eq!(counting.state.redactions_per_page(&summary), vec![4, 1]);
    let texts = thumbnail_frame(&std::cell::RefCell::new(counting));
    assert!(
        texts.contains(&"4".to_string()),
        "die 4 gehört an Seite 1 — gemalt wurde: {texts:?}"
    );
}

// ===========================================================================
// B7 — Rechtecke neben dem Blatt aus einer Review-Datei
// ===========================================================================

/// **Befund B7, behoben.** [`AppState::apply_review_file`] klemmt nicht — und
/// das soll es auch nicht: eine Review-Datei ist die Angabe der Nutzerin, und
/// sie stillschweigend zu verändern wäre schlimmer als das Problem. Gefehlt hat
/// die **Ansage**: ein Rechteck, das vollständig neben der Seite liegt, wurde
/// als „wird geschwärzt“ gezählt, und die Wahrheit kam erst nach dem Export als
/// Warnung.
///
/// Jetzt gibt es [`HitOutcome::OffPage`]: solche Einträge zählen nicht mehr als
/// Schwärzung, stehen mit eigenem Wort in der Liste und in der Kopfzeile, und
/// sie gehen gar nicht erst in die Konfliktauflösung.
#[test]
fn b7_off_page_regions_from_a_review_file_are_named_not_counted() {
    let mut app = loaded(true);
    let sha = app.state.input_sha256.clone();
    let sheet = app.state.page_box(0).unwrap();

    let real = app.state.regions[0].region.clone();
    assert!(matches!(real.source, Source::Pattern { .. }));
    let off_page = Region::new(
        0,
        Rect::new(sheet.ur.x + 50.0, 100.0, sheet.ur.x + 250.0, 120.0),
        Some("DE00 0000 0000 0000 0000 00".into()),
        Source::Manual {
            reason: "neben dem Blatt".into(),
        },
    );

    let review = redact_core::ReviewFile::new(
        redact_core::ReviewInput {
            path: "demo.pdf".into(),
            sha256: sha,
            pages: 2,
        },
        vec![real, off_page],
        Vec::new(),
    );
    app.state.apply_review_file(review).expect("passt");

    let summary = app.state.hit_summary();
    assert_eq!(summary.outcome(0), HitOutcome::Redacted);
    assert_eq!(summary.outcome(1), HitOutcome::OffPage);
    assert_eq!(summary.redacted, 1);
    assert_eq!(summary.off_page, 1);
    assert!(
        summary.headline().contains("1 werden geschwärzt")
            && summary.headline().contains("1 neben der Seite"),
        "Kopfzeile: {}",
        summary.headline()
    );
    assert!(!HitOutcome::OffPage.note().is_empty());

    // Und der Export verspricht nichts anderes: das Rechteck neben dem Blatt
    // wird gar nicht erst mitgeschickt, also fehlt auch die nachträgliche
    // Warnung „kein einziges Zeichen“.
    let dir = tmp("b7");
    let outcome = app
        .state
        .export(&dir.join("out.pdf"), None)
        .expect("Export läuft");
    assert_eq!(outcome.redactions, 1);
    assert_eq!(outcome.effective_redactions, 1);
    assert!(
        !outcome
            .warnings
            .iter()
            .any(|w| w.contains("kein einziges Zeichen")),
        "Warnungen: {:?}",
        outcome.warnings
    );

    // Gegenprobe: dasselbe Rechteck selbst gezogen entsteht gar nicht erst.
    let mut drawn = loaded(true);
    let index = drawn.state.add_manual_region(
        0,
        Rect::new(sheet.ur.x + 50.0, 100.0, sheet.ur.x + 250.0, 120.0),
        "neben dem Blatt",
    );
    assert_eq!(index, None, "selbst gezogen wird geklemmt bzw. verworfen");

    // Gegenprobe 2: ein Rechteck, das die Seite nur **teilweise** verlässt, ist
    // keine Off-Page-Region — es trifft ja Text.
    let mut partly = loaded(true);
    let index = partly
        .state
        .add_manual_region(
            0,
            Rect::new(sheet.ur.x - 20.0, 100.0, sheet.ur.x + 250.0, 120.0),
            "halb daneben",
        )
        .expect("angelegt");
    assert_eq!(
        partly.state.hit_summary().outcome(index),
        HitOutcome::Redacted
    );
}

// ===========================================================================
// Entartete Zustände
// ===========================================================================

/// Ohne geladenes Dokument klemmt `clamp_to_page` nichts — dort gibt es keine
/// Seite. Ein von Hand gezogenes Rechteck entsteht trotzdem, und weil es keine
/// Seite gibt, gilt es auch nicht als „neben der Seite“.
#[test]
fn e1_without_a_document_nothing_is_clamped() {
    let mut state = crate::state::AppState::new();
    assert_eq!(state.page_count(), 0);
    let index = state.add_manual_region(0, Rect::new(9000.0, 9000.0, 9100.0, 9100.0), "x");
    assert_eq!(
        index,
        Some(0),
        "ohne Dokument bleibt das Rechteck, wie es ist"
    );
    let summary = state.hit_summary();
    assert_eq!(summary.redacted, 1);
    assert_eq!(summary.off_page, 0);
    assert!(
        state.export(&PathBuf::from("/dev/null"), None).is_err(),
        "der Export scheitert wenigstens am fehlenden Dokument"
    );

    // Und die Pfeiltasten schieben dort, ohne sich an einer Seite zu stoßen.
    state.selected_region = Some(0);
    let before = state.regions[0].region.rect;
    assert!(state.move_selected(-5.0, 0.0));
    assert_eq!(state.regions[0].region.rect.ll.x, before.ll.x - 5.0);
}

/// Ein entartetes Rechteck (Breite 0) lässt sich über den Eckgriff herstellen.
/// `clamp_to_page` verwirft es — die Region bleibt, wie sie war.
#[test]
fn e2_a_degenerate_rectangle_is_refused_by_clamp() {
    let mut app = loaded(true);
    let before = app.state.regions[0].region.rect;
    // Auf die Gegenecke ziehen ⇒ Breite und Höhe null.
    let ok = app.state.set_region_rect(
        0,
        Rect::new(before.ll.x, before.ll.y, before.ll.x, before.ll.y),
    );
    assert!(!ok, "ein leeres Rechteck wird abgelehnt");
    assert_eq!(app.state.regions[0].region.rect, before);
    assert!(
        matches!(app.state.regions[0].region.source, Source::Pattern { .. }),
        "und der Treffer bleibt, was er war"
    );
}

// ===========================================================================
// Was gehalten hat — und weiter halten muss
// ===========================================================================

/// Der Zug am Eckgriff überlebt Umsortierung und Löschen **nicht** als falscher
/// Index: die [`crate::state::RegionId`] findet entweder dieselbe Region oder
/// gar keine. Hier: eine Region **vor** der gezogenen wird gelöscht, alle
/// Indizes rutschen — der Zug bleibt trotzdem auf seiner Region.
#[test]
fn ok_a_handle_drag_follows_its_id_when_the_list_is_renumbered() {
    let mut app = loaded(true);
    // Zwei Regionen auf Seite 1: der IBAN-Treffer und ein Handrechteck.
    let drawn = app
        .state
        .add_manual_region(0, Rect::new(300.0, 300.0, 400.0, 320.0), "Adresse")
        .expect("angelegt");
    let target_id = app.state.id_at(drawn).unwrap();
    let rect = app.state.regions[drawn].region.rect;

    let view = app.state.page_view(0);
    let screen = viewer::pdf_to_screen(&rect, &view, ZOOM, ORIGIN);
    let corner = screen.left_top();
    app.apply_pointer(press_frame(corner), false, &view, ORIGIN, 0, ZOOM);
    assert_eq!(app.resize.map(|d| d.region), Some(target_id));

    // Mitten im Zug fällt eine Region **davor** weg (Trefferliste, Entf).
    app.state.selected_region = Some(0);
    assert!(app.state.delete_selected());
    let now = app.state.index_of(target_id).expect("noch da");
    assert_ne!(now, drawn, "die Indizes haben sich verschoben");

    let target = corner - Vec2::new(20.0, 20.0);
    app.apply_pointer(
        dragging_frame(corner, target),
        false,
        &view,
        ORIGIN,
        0,
        ZOOM,
    );
    app.apply_pointer(release_frame(target), false, &view, ORIGIN, 0, ZOOM);
    assert_ne!(
        app.state.regions[now].region.rect, rect,
        "die gezogene Region hat sich geändert"
    );
    assert_eq!(app.state.regions.len(), 2);
}

/// Eine erneute Analyse tauscht alle automatischen Einträge aus — die alten
/// Kennungen sind damit weg, und ein laufender Zug endet, statt auf einen
/// fremden Treffer zu springen.
#[test]
fn ok_a_new_analysis_ends_a_running_handle_drag() {
    let mut app = loaded(true);
    app.state.selected_region = Some(0);
    let rect = app.state.regions[0].region.rect;
    let view = app.state.page_view(0);
    let screen = viewer::pdf_to_screen(&rect, &view, ZOOM, ORIGIN);
    let corner = screen.left_top();
    app.apply_pointer(press_frame(corner), false, &view, ORIGIN, 0, ZOOM);
    assert!(app.resize.is_some());

    assert!(app.analyze(), "Analyse läuft (Rückfrage: Ja)");

    let target = corner - Vec2::new(30.0, 30.0);
    app.apply_pointer(
        dragging_frame(corner, target),
        false,
        &view,
        ORIGIN,
        0,
        ZOOM,
    );
    assert!(app.resize.is_none(), "der Zug ist beendet");
    assert_eq!(app.state.regions[0].region.rect, rect, "nichts verrutscht");
    assert!(app.state.status.contains("gibt es nicht mehr"));
}

/// Strg+Z mitten im Zug beendet ihn, statt den zurückgenommenen Stand sofort
/// wieder zu überschreiben.
#[test]
fn ok_undo_during_a_handle_drag_ends_it() {
    let mut app = loaded(true);
    app.state.selected_region = Some(0);
    let rect = app.state.regions[0].region.rect;
    let view = app.state.page_view(0);
    let screen = viewer::pdf_to_screen(&rect, &view, ZOOM, ORIGIN);
    let corner = screen.left_top();
    app.apply_pointer(press_frame(corner), false, &view, ORIGIN, 0, ZOOM);
    let moved = corner - Vec2::new(10.0, 10.0);
    app.apply_pointer(dragging_frame(corner, moved), false, &view, ORIGIN, 0, ZOOM);
    assert_ne!(app.state.regions[0].region.rect, rect);

    press(
        &mut app,
        keys(|k| {
            k.ctrl = true;
            k.key_z = true;
        }),
    );
    assert!(app.resize.is_none());
    assert_eq!(app.state.regions[0].region.rect, rect);

    // Weitere Mausbilder ändern nichts mehr.
    let further = corner - Vec2::new(60.0, 60.0);
    app.apply_pointer(
        dragging_frame(corner, further),
        false,
        &view,
        ORIGIN,
        0,
        ZOOM,
    );
    app.apply_pointer(release_frame(further), false, &view, ORIGIN, 0, ZOOM);
    assert_eq!(app.state.regions[0].region.rect, rect);
}

/// Ein Review, das der Analyse entspricht, löst die Verwerfen-Rückfrage nicht
/// aus — weil `has_manual_work()` den *aktuellen* Stand betrachtet und dort
/// keine Handarbeit steckt. Das ist geprüft und in Ordnung: es geht nichts
/// verloren, was nicht ein Klick auf „Analysieren“ zurückholt.
#[test]
fn ok_a_review_matching_the_analysis_does_not_ask() {
    let mut app = loaded(false); // jede Rückfrage würde mit „Nein“ enden
    assert!(
        !app.state.has_manual_work(),
        "eine frische Analyse ist keine Handarbeit"
    );

    let dir = tmp("review-match");
    let path = dir.join("review.json");
    app.state.save_review_file(&path).unwrap();

    assert!(app.may_discard("Ein Review zu laden"));
    let before = app.state.regions.len();
    app.load_review_file(&path);
    assert_eq!(app.state.regions.len(), before);

    // Gegenprobe: mit Handarbeit wird sehr wohl gefragt.
    let mut with_work = loaded(false);
    with_work
        .state
        .add_manual_region(0, Rect::new(60.0, 60.0, 200.0, 80.0), "Adresse");
    assert!(with_work.state.has_manual_work());
    assert!(!with_work.may_discard("Ein Review zu laden"));
}

/// Ein durch die Schutzliste gedeckter Treffer wird durch einen Zug am Eckgriff
/// zur manuellen Region und überstimmt damit die Schutzliste. Die Statuszeile
/// sagt es ([`crate::state::PROTECTION_OVERRIDDEN`]), und Strg+Z nimmt es
/// zurück. Geprüft und ausdrücklich so gewollt.
#[test]
fn ok_touching_a_protected_hit_overrides_its_protection_and_says_so() {
    let mut app = loaded(true);
    let hit = app.state.regions[0].region.rect;

    // Eine Schutzregion, die den Treffer vollständig deckt.
    app.state.regions.push(AnnotatedRegion::new(Region::new(
        0,
        Rect::new(
            hit.ll.x - 2.0,
            hit.ll.y - 2.0,
            hit.ur.x + 2.0,
            hit.ur.y + 2.0,
        ),
        Some("Max Mustermann".into()),
        Source::Booking {
            booking_id: "b003".into(),
            match_type: MatchType::Negative,
        },
    )));

    let summary = app.state.hit_summary();
    assert_eq!(summary.outcome(0), HitOutcome::Blocked);
    let protected_count = summary.redacted;

    // Denselben Treffer am Eckgriff einen Punkt breiter ziehen.
    app.state.selected_region = Some(0);
    let view = app.state.page_view(0);
    let screen = viewer::pdf_to_screen(&hit, &view, ZOOM, ORIGIN);
    let corner = screen.left_top();
    let target = corner - Vec2::new(1.0, 0.0);
    app.apply_pointer(press_frame(corner), false, &view, ORIGIN, 0, ZOOM);
    app.apply_pointer(
        dragging_frame(corner, target),
        false,
        &view,
        ORIGIN,
        0,
        ZOOM,
    );
    app.apply_pointer(release_frame(target), false, &view, ORIGIN, 0, ZOOM);

    let after = app.state.hit_summary();
    assert_eq!(after.outcome(0), HitOutcome::Redacted);
    assert_eq!(
        after.redacted,
        protected_count + 1,
        "aus geschützt wird geschwärzt"
    );
    assert_eq!(
        app.state.status,
        crate::state::PROTECTION_OVERRIDDEN,
        "und es steht in der Statuszeile"
    );

    // Strg+Z nimmt es zurück.
    press(
        &mut app,
        keys(|k| {
            k.ctrl = true;
            k.key_z = true;
        }),
    );
    assert_eq!(app.state.hit_summary().outcome(0), HitOutcome::Blocked);
}

/// Eine Review-Datei zu einem **anderen** Dokument wird abgelehnt, und der
/// bisherige Stand bleibt unangetastet.
#[test]
fn ok_a_foreign_review_file_changes_nothing() {
    let mut app = loaded(true);
    let before = app.state.regions.clone();
    let review = redact_core::ReviewFile::new(
        redact_core::ReviewInput {
            path: "fremd.pdf".into(),
            sha256: "0".repeat(64),
            pages: 1,
        },
        vec![Region::new(
            0,
            Rect::new(0.0, 0.0, 100.0, 20.0),
            None,
            Source::Manual {
                reason: "fremd".into(),
            },
        )],
        Vec::new(),
    );
    assert!(app.state.apply_review_file(review).is_err());
    assert_eq!(app.state.regions, before, "nichts verändert");
    assert!(!app.state.can_undo() || app.state.regions == before);
}

/// Ein Export ohne einzige Schwärzung wird abgelehnt, statt eine unveränderte
/// Kopie mit Erfolgsmeldung zu hinterlassen.
#[test]
fn ok_an_export_without_any_redaction_is_refused() {
    let mut app = loaded(true);
    for i in 0..app.state.regions.len() {
        app.state.set_enabled(i, false);
    }
    assert_eq!(app.state.hit_summary().redacted, 0);
    let dir = tmp("ok-export");
    let out = dir.join("leer.pdf");
    let err = app.state.export(&out, None).expect_err("muss scheitern");
    assert!(err.to_string().contains("Nichts ausgewählt"), "{err}");
    assert!(!out.exists(), "und es darf keine Datei entstehen");
}

/// Ein Export, in dem **nur** Rechtecke neben der Seite übrig sind, wird
/// genauso abgelehnt: die Kopfzeile sagt „0 werden geschwärzt“, und dann darf
/// keine Datei entstehen, die aussieht wie ein Ergebnis.
#[test]
fn ok_an_export_with_only_off_page_rectangles_is_refused() {
    let mut app = loaded(true);
    let sha = app.state.input_sha256.clone();
    let sheet = app.state.page_box(0).unwrap();
    let off_page = Region::new(
        0,
        Rect::new(sheet.ur.x + 50.0, 100.0, sheet.ur.x + 250.0, 120.0),
        None,
        Source::Manual {
            reason: "neben dem Blatt".into(),
        },
    );
    let review = redact_core::ReviewFile::new(
        redact_core::ReviewInput {
            path: "demo.pdf".into(),
            sha256: sha,
            pages: 2,
        },
        vec![off_page],
        Vec::new(),
    );
    app.state.apply_review_file(review).expect("passt");

    assert_eq!(app.state.hit_summary().redacted, 0);
    let dir = tmp("ok-offpage-export");
    let out = dir.join("leer.pdf");
    let err = app.state.export(&out, None).expect_err("muss scheitern");
    assert!(err.to_string().contains("Nichts ausgewählt"), "{err}");
    assert!(!out.exists());
}

/// Der Export auf die Originaldatei wird abgelehnt.
#[test]
fn ok_exporting_onto_the_original_is_refused() {
    let dir = tmp("ok-original");
    let original = dir.join("auszug.pdf");
    std::fs::write(&original, redact_pdf::testing::demo_statement()).unwrap();
    let mut app = RedactApp::silent(iban_only());
    app.open_and_analyze(original.clone());
    assert!(app.state.is_loaded());
    let err = app
        .state
        .export(&original, None)
        .expect_err("muss scheitern");
    assert!(err.to_string().contains("Originaldatei"), "{err}");
    // Die Datei ist unverändert.
    assert_eq!(
        std::fs::read(&original).unwrap(),
        redact_pdf::testing::demo_statement()
    );
}

/// Ein PDF ohne Seiten wird beim Laden mit klarem Satz abgelehnt — es kommt gar
/// nicht erst in einen Zustand, in dem Rechtecke auf einer nicht vorhandenen
/// Seite entstünden.
#[test]
fn ok_a_pdf_without_pages_is_refused_at_load() {
    use lopdf::{dictionary, Document, Object};

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => Object::Array(vec![]),
            "Count" => 0,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();

    let mut app = RedactApp::silent(iban_only());
    let error = app
        .state
        .load_bytes(&bytes, Some(PathBuf::from("leer.pdf")))
        .expect_err("ein Dokument ohne Seiten gehört abgelehnt");
    assert!(error.to_string().contains("keine Seiten"), "{error}");
    assert!(!app.state.is_loaded());
    assert_eq!(app.state.page_count(), 0);
}
