//! Prüfrunde Oberfläche (0.8.1): wem die Tasten am **Zoomregler** gehören.
//!
//! Der Regler der Ansichtsleiste (`RedactApp::view_bar`) hat ein eintippbares
//! Zahlenfeld — ein `egui::TextEdit` mit einer Kennung, die egui vergibt und
//! die deshalb nicht in [`crate::focus::TEXT_FIELDS`] stand. Gemessen vor der
//! Korrektur:
//!
//! * Rücktaste beim Korrigieren der Zoomzahl **löschte das ausgewählte
//!   Rechteck**, Pfeil links schob es um 1 pt;
//! * Pfeil rechts auf der fokussierten Schiene zoomte **und** schob das
//!   Rechteck (ohne Auswahl: zoomte und blätterte).
//!
//! Kindmodul von `app`: `view_bar` und `handle_keys` sind privat.
//!
//! `cargo test -p redact-gui --lib zq_ui`

use super::*;

use redact_core::Rect;

/// Zeichnet die Ansichtsleiste und liest danach die Tasten — dieselbe
/// Reihenfolge wie in `update` (Panels zuerst, Tasten zuletzt).
fn frame(ctx: &egui::Context, app: &mut RedactApp, events: Vec<egui::Event>) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            Pos2::ZERO,
            Vec2::new(crate::WINDOW_SIZE[0], crate::WINDOW_SIZE[1]),
        )),
        events,
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| {
        egui::TopBottomPanel::top("view_bar").show(ctx, |ui| app.view_bar(ui));
        app.handle_keys(ctx);
    });
}

fn key(key: Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn press(ctx: &egui::Context, app: &mut RedactApp, k: Key) {
    frame(ctx, app, vec![key(k, egui::Modifiers::NONE)]);
}

/// Ein Dokument mit zwei Seiten und einem ausgewählten Rechteck.
fn app_with_selection() -> RedactApp {
    let mut app = RedactApp::silent(Config {
        no_patterns: true,
        ..Config::default()
    });
    app.open_bytes_and_analyze(&redact_pdf::testing::demo_statement(), "demo.pdf");
    assert!(app.state.page_count() >= 2);
    app.state
        .add_manual_region(0, Rect::new(100.0, 100.0, 200.0, 140.0), "Gehalt");
    assert!(app.state.selected_region.is_some());
    app
}

fn focused_text_edit(ctx: &egui::Context) -> bool {
    ctx.memory(|m| m.focused())
        .is_some_and(|id| egui::text_edit::TextEditState::load(ctx, id).is_some())
}

/// Tab, bis der Fokus im Zahlenfeld des Reglers steht — das einzige
/// Textfeld der Ansichtsleiste.
fn tab_into_the_zoom_value(ctx: &egui::Context, app: &mut RedactApp) {
    frame(ctx, app, vec![]);
    for _ in 0..20 {
        press(ctx, app, Key::Tab);
        if focused_text_edit(ctx) {
            frame(ctx, app, vec![]);
            return;
        }
    }
    panic!("das Zahlenfeld des Zoomreglers ist per Tab nicht erreichbar");
}

#[test]
fn zq_ui_backspace_in_the_zoom_value_keeps_the_rectangle() {
    let ctx = egui::Context::default();
    let mut app = app_with_selection();
    let regions = app.state.regions.len();
    let rect = app.state.selected().unwrap().region.rect;

    tab_into_the_zoom_value(&ctx, &mut app);
    assert!(
        crate::focus::in_text_field(&ctx),
        "das Feld zählt als Textfeld"
    );

    for k in [Key::Backspace, Key::Delete, Key::ArrowLeft, Key::ArrowRight] {
        press(&ctx, &mut app, k);
    }
    assert_eq!(app.state.regions.len(), regions, "Rechteck gelöscht");
    assert_eq!(
        app.state.selected().unwrap().region.rect,
        rect,
        "verschoben"
    );

    // Das Feld nimmt die Tasten wirklich: Zahl eintippen, Enter.
    for _ in 0..6 {
        press(&ctx, &mut app, Key::Backspace);
    }
    frame(&ctx, &mut app, vec![egui::Event::Text("200".into())]);
    press(&ctx, &mut app, Key::Enter);
    frame(&ctx, &mut app, vec![]);
    assert!(
        (app.state.zoom - 2.0).abs() < 1e-6,
        "Zoom {}",
        app.state.zoom
    );
    assert_eq!(app.state.regions.len(), regions);
}

#[test]
fn zq_ui_an_arrow_on_the_zoom_slider_only_zooms() {
    let ctx = egui::Context::default();
    let mut app = app_with_selection();
    let rect = app.state.selected().unwrap().region.rect;
    tab_into_the_zoom_value(&ctx, &mut app);
    // Eins zurück: die Schiene des Reglers.
    frame(&ctx, &mut app, vec![key(Key::Tab, egui::Modifiers::SHIFT)]);
    frame(&ctx, &mut app, vec![]);
    assert!(
        !focused_text_edit(&ctx),
        "Fokus muss auf der Schiene stehen"
    );

    let zoom = app.state.zoom;
    press(&ctx, &mut app, Key::ArrowRight);
    assert!(app.state.zoom > zoom, "der Regler nimmt den Pfeil");
    assert_eq!(
        app.state.selected().unwrap().region.rect,
        rect,
        "verschoben"
    );

    // Ohne Auswahl: zoomen, nicht blättern.
    app.state.selected_region = None;
    let page = app.state.current_page;
    press(&ctx, &mut app, Key::ArrowRight);
    assert_eq!(app.state.current_page, page, "geblättert");

    // Gegenprobe: die Sperre gilt nur, solange der Regler den Fokus hat.
    ctx.memory_mut(|m| m.stop_text_input());
    if let Some(id) = ctx.memory(|m| m.focused()) {
        ctx.memory_mut(|m| m.surrender_focus(id));
    }
    frame(&ctx, &mut app, vec![]);
    press(&ctx, &mut app, Key::ArrowRight);
    assert_eq!(
        app.state.current_page,
        page + 1,
        "Pfeil ohne Fokus blättert"
    );
}
