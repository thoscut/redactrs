//! Prüfrunde 6 — Bedienung ohne Maus, der Notnagel-Pfad und die Umbauten aus
//! v0.4.0.
//!
//! Kindmodul von [`crate::app`], damit `apply_pointer`, `tool_row`,
//! `apply_tool_action` und `RedactApp::silent` erreichbar sind.
//!
//! ## Woher diese Datei kommt
//!
//! Sie ist als Befundsammlung entstanden: jeder Test hielt fest, dass die
//! Oberfläche von v0.4.0 an dieser Stelle etwas Falsches tut. In dieser Runde
//! sind die Stellen behoben, und die Tests halten jetzt das Gegenteil fest —
//! **mit** der Beschreibung dessen, was vorher geschah, damit ein
//! fehlschlagender Test nicht nur „ungleich“ sagt, sondern auch, was
//! zurückgekommen ist.
//!
//! Zwei Belege gehen ausdrücklich über die **geschriebene Datei** und über
//! [`redact_pdf::leaks`] — nicht über den eigenen Extraktor: der sieht dieselbe
//! Datei mit denselben Augen wie der Code, der sie geschrieben hat, und ein
//! Rest, der nur in einem `/ObjStm`, einem Anmerkungsfeld oder in TJ-Stücken
//! steht, entginge ihm.

use super::*;

use redact_core::{Action, MatchType, Rect, Region, Source};

use crate::state::{AnnotatedRegion, HitOutcome};

// --------------------------------------------------------------- Hilfsmittel

fn iban_only() -> Config {
    Config {
        patterns: vec!["iban_de".to_string()],
        ..Config::default()
    }
}

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
        "rev6-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn keys(f: impl FnOnce(&mut KeyState)) -> KeyState {
    let mut k = KeyState::default();
    f(&mut k);
    k
}

/// Eine Taste durch die ganze Kette schicken: Übersetzung + Ausführung.
fn press(app: &mut RedactApp, k: KeyState) {
    let commands = key_commands(k, app.state.selected_region.is_some());
    app.apply_key_commands(&commands);
}

/// Der Text, den ein geschriebenes PDF noch hergibt — für Meldungen.
fn text_of(path: &std::path::Path) -> String {
    let doc = lopdf::Document::load(path).expect("Ausgabe ladbar");
    let (runs, _) = redact_pdf::PdfExtractor::new()
        .extract_with_warnings(&doc)
        .expect("lesbar");
    runs.iter()
        .map(|r| r.text.clone())
        .collect::<Vec<_>>()
        .join(" | ")
}

/// **Der** Beleg für „die IBAN ist weg“: [`redact_pdf::leaks`] über die
/// geschriebenen Bytes.
///
/// Sucht in allem, was in der Datei steht — Rohbytes, entpackte Streams,
/// Objekt-Streams, jedes Zeichenkettenobjekt, beide Kodierungen, und innerhalb
/// von Streams zusätzlich die Verkettung aller Literale. Der eigene Extraktor
/// taugt dafür nicht: er sieht die Datei genauso, wie der Schreibpfad sie
/// gemeint hat.
fn assert_gone(path: &std::path::Path, needle: &str) {
    let bytes = std::fs::read(path).expect("Ausgabe lesbar");
    let hits = redact_pdf::leaks(&bytes, needle);
    assert!(
        hits.is_empty(),
        "„{needle}“ steht noch in {}: {hits:#?}",
        path.display()
    );
}

/// Die Gegenprobe dazu — der Prüfer muss auch anschlagen können.
fn assert_present(path: &std::path::Path, needle: &str) {
    let bytes = std::fs::read(path).expect("Ausgabe lesbar");
    assert!(
        !redact_pdf::leaks(&bytes, needle).is_empty(),
        "„{needle}“ fehlt in {} — dann prüft `assert_gone` nichts",
        path.display()
    );
}

fn tab(shift: bool) -> egui::Event {
    egui::Event::Key {
        key: egui::Key::Tab,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            shift,
            ..Default::default()
        },
    }
}

fn space() -> egui::Event {
    egui::Event::Key {
        key: egui::Key::Space,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::default(),
    }
}

// ===========================================================================
// A1 — Ohne Maus ein Rechteck anlegen
// ===========================================================================

/// **Befund A1.** Die Oberfläche kannte keinen Weg, ein Schwärzungsrechteck
/// ohne Zeigegerät zu erzeugen. [`AppState::add_manual_region`] wurde an
/// **genau einer** Stelle gerufen — in [`RedactApp::apply_pointer`], Fall 4,
/// aus einem [`PointerFrame`]. Weder [`key_commands`] noch
/// [`crate::toolbar::ToolAction`] noch die Seitenleiste hatten ein Gegenstück.
///
/// Das war nicht bloß eine fehlende Bequemlichkeit. Auf einem Kontoauszug sind
/// Anschrift, Kontonummer und Name des Kontoinhabers genau die Stellen, die
/// kein Muster zuverlässig findet; sie **müssen** von Hand gezogen werden. Wer
/// keine Maus benutzen kann, konnte diese Datei also nicht vollständig
/// schwärzen — er konnte nur an dem herumschieben, was die Analyse ohnehin
/// gefunden hatte.
///
/// Seit dieser Runde gibt es **einen** solchen Weg, und er hat zwei Eingänge,
/// die dasselbe tun: Strg+R und den Knopf „Rechteck“.
#[test]
fn a1_a_rectangle_can_be_created_without_a_mouse() {
    let mut app = RedactApp::silent(iban_only());
    app.state
        .load_bytes(
            &redact_pdf::testing::demo_statement(),
            Some(PathBuf::from("demo.pdf")),
        )
        .expect("Demo-PDF ladbar");
    app.apply_pattern_toggle(crate::sidebar::PatternToggle::All(false));
    assert!(app.state.regions.is_empty(), "Ausgangslage: keine Treffer");

    // --- Weg 1: das Tastenkürzel ---
    press(
        &mut app,
        keys(|k| {
            k.ctrl = true;
            k.key_r = true;
        }),
    );
    assert_eq!(app.state.regions.len(), 1, "Strg+R legt ein Rechteck an");
    let index = app.state.selected_region.expect("und wählt es aus");
    assert_eq!(index, 0);
    let entry = &app.state.regions[0];
    assert_eq!(entry.region.page, app.state.current_page);
    assert!(matches!(entry.region.source, Source::Manual { .. }));
    assert!(entry.enabled, "und es zählt sofort als Schwärzung");

    // In der Mitte der Seite, in der vorgesehenen Größe.
    let sheet = app.state.page_box(0).expect("Seite 0").normalized();
    let rect = entry.region.rect;
    assert!(
        ((rect.ll.x + rect.ur.x) / 2.0 - (sheet.ll.x + sheet.ur.x) / 2.0).abs() < 1e-6,
        "waagerecht mittig: {rect:?} auf {sheet:?}"
    );
    assert!(
        ((rect.ll.y + rect.ur.y) / 2.0 - (sheet.ll.y + sheet.ur.y) / 2.0).abs() < 1e-6,
        "senkrecht mittig: {rect:?} auf {sheet:?}"
    );
    assert!((rect.width() - crate::state::NEW_REGION_SIZE.0).abs() < 1e-6);
    assert!((rect.height() - crate::state::NEW_REGION_SIZE.1).abs() < 1e-6);

    // Die Statuszeile sagt, wie es weitergeht — sonst steht der
    // Tastaturbenutzer vor einem Rechteck und keiner Auskunft.
    let status = app.state.status.clone();
    for word in ["Pfeiltasten", "Strg+Pfeil", "Entf", "Strg+Z"] {
        assert!(status.contains(word), "Statuszeile: {status}");
    }

    // --- Weg 2: der Knopf, auf einer anderen Seite ---
    let ctx = egui::Context::default();
    app.state.set_page(1);
    app.apply_tool_action(ToolAction::AddRegion, &ctx);
    assert_eq!(app.state.regions.len(), 2);
    assert_eq!(
        app.state.regions[1].region.page, 1,
        "der Knopf legt es auf der gezeigten Seite an"
    );

    // --- Und die Größe hängt nicht mehr an der Maus ---
    let before = app.state.regions[1].region.rect;
    press(
        &mut app,
        keys(|k| {
            k.ctrl = true;
            k.right = true;
            k.shift = true;
        }),
    );
    press(
        &mut app,
        keys(|k| {
            k.ctrl = true;
            k.up = true;
            k.shift = true;
        }),
    );
    let after = app.state.regions[1].region.rect;
    assert!(
        (after.width() - before.width() - 10.0).abs() < 1e-6,
        "{after:?}"
    );
    assert!(
        (after.height() - before.height() - 10.0).abs() < 1e-6,
        "{after:?}"
    );
    assert_eq!(
        (after.ll.x, after.ll.y),
        (before.ll.x, before.ll.y),
        "die linke untere Ecke bleibt stehen"
    );
    // Und wieder kleiner.
    press(
        &mut app,
        keys(|k| {
            k.ctrl = true;
            k.left = true;
            k.shift = true;
        }),
    );
    assert!((app.state.regions[1].region.rect.width() - before.width()).abs() < 1e-6);

    // --- Was weiterhin nichts anlegt ---
    //
    // Der Weg soll **einer** sein und kein Seiteneffekt jeder zweiten Taste.
    let mut quiet = RedactApp::silent(iban_only());
    quiet
        .state
        .load_bytes(&redact_pdf::testing::demo_statement(), None)
        .expect("ladbar");
    quiet.apply_pattern_toggle(crate::sidebar::PatternToggle::All(false));
    let every_other_key = [
        keys(|k| k.delete = true),
        keys(|k| k.escape = true),
        keys(|k| k.left = true),
        keys(|k| k.right = true),
        keys(|k| k.up = true),
        keys(|k| k.down = true),
        keys(|k| k.page_up = true),
        keys(|k| k.page_down = true),
        keys(|k| k.home = true),
        keys(|k| k.end = true),
        keys(|k| {
            k.shift = true;
            k.left = true;
        }),
        keys(|k| {
            k.ctrl = true;
            k.key_z = true;
        }),
        keys(|k| {
            k.ctrl = true;
            k.key_y = true;
        }),
        // Strg+Pfeil ändert eine **vorhandene** Auswahl; ohne Auswahl tut es
        // nichts und legt erst recht nichts an.
        keys(|k| {
            k.ctrl = true;
            k.right = true;
        }),
    ];
    for k in every_other_key {
        let commands: Vec<KeyCommand> = key_commands(k, quiet.state.selected_region.is_some())
            .into_iter()
            .filter(|c| !matches!(c, KeyCommand::Open | KeyCommand::Export))
            .collect();
        quiet.apply_key_commands(&commands);
        assert!(
            quiet.state.regions.is_empty(),
            "Tastenfolge {k:?} hat etwas angelegt"
        );
    }
    for button in crate::toolbar::buttons() {
        // Die fünf Knöpfe mit Dateidialog bleiben draußen — sie laden oder
        // schreiben Dateien und brauchen ein Fenster.
        if matches!(
            button.action,
            ToolAction::Open
                | ToolAction::Export
                | ToolAction::Booking
                | ToolAction::ReviewSave
                | ToolAction::ReviewLoad
                | ToolAction::AddRegion
        ) {
            continue;
        }
        quiet.apply_tool_action(button.action, &ctx);
        assert!(
            quiet.state.regions.is_empty(),
            "„{}“ hat etwas angelegt",
            button.text
        );
    }

    // Ohne Dokument gibt es keine Seite — und deshalb auch kein Rechteck,
    // sondern einen Satz dazu.
    let mut empty = RedactApp::silent(iban_only());
    press(
        &mut empty,
        keys(|k| {
            k.ctrl = true;
            k.key_r = true;
        }),
    );
    assert!(empty.state.regions.is_empty());
    assert!(
        empty.state.status.contains("Kein Dokument"),
        "{}",
        empty.state.status
    );
}

/// Die Gegenrichtung, damit der Befund nicht größer klingt, als er ist:
/// **auswählen, verschieben, löschen und die Art umstellen** gehen ohne Maus.
#[test]
fn a2_selecting_moving_deleting_and_retyping_do_work_without_a_mouse() {
    let mut app = loaded(true);
    assert_eq!(app.state.regions.len(), 2);

    // Auswählen über die Trefferliste: Tabulator bis zur Zeile, dann Leertaste.
    let selected = select_first_hit_by_keyboard(loaded(true));
    assert_eq!(
        selected,
        Some(0),
        "die erste Trefferzeile ist per Tabulator und Leertaste erreichbar"
    );

    // Verschieben.
    app.state.selected_region = Some(0);
    let before = app.state.regions[0].region.rect;
    press(
        &mut app,
        keys(|k| {
            k.right = true;
            k.shift = true;
        }),
    );
    assert!(app.state.regions[0].region.rect.ll.x > before.ll.x);

    // Art umstellen — über dieselbe Methode, die die Auswahlliste ruft.
    assert!(app.state.set_action(0, Action::Whiteout));
    assert_eq!(app.state.regions[0].action, Action::Whiteout);

    // Löschen.
    press(&mut app, keys(|k| k.delete = true));
    assert_eq!(app.state.regions.len(), 1);
}

/// Tabulator durch die Seitenleiste, bis eine Trefferzeile ausgewählt ist.
fn select_first_hit_by_keyboard(app: RedactApp) -> Option<usize> {
    hits_reachable_by_keyboard(app, 20).first().copied()
}

/// Alle Trefferzeilen, die der Tabulator vorwärts erreicht — in der
/// Reihenfolge, in der er sie erreicht.
///
/// Ein Druck auf den Tabulator, ein Druck auf die Leertaste, und dann
/// nachsehen, welche Zeile ausgewählt ist. Das ist genau der Weg, den jemand
/// ohne Zeigegerät geht.
fn hits_reachable_by_keyboard(app: RedactApp, steps: usize) -> Vec<usize> {
    use std::cell::RefCell;
    let app = RefCell::new(app);
    let ctx = egui::Context::default();
    let draw = |events: Vec<egui::Event>| {
        let summary = app.borrow().state.hit_summary();
        let input = egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(1400.0, 1400.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::SidePanel::right("sidebar")
                .exact_width(500.0)
                .show(ctx, |ui| {
                    let _ = crate::sidebar::show(ui, &mut app.borrow_mut().state, &summary);
                });
        });
    };
    draw(Vec::new());
    let mut seen: Vec<usize> = Vec::new();
    for _ in 0..steps {
        draw(vec![tab(false)]);
        draw(vec![space()]);
        if let Some(index) = app.borrow().state.selected_region {
            if !seen.contains(&index) {
                seen.push(index);
            }
        }
    }
    seen
}

// ===========================================================================
// A3 — Der Tabulator kommt an einem ausgegrauten Bedienelement vorbei
// ===========================================================================

/// Die Kette, die der Tabulator vorwärts durch die **echte** Knopfreihe läuft.
///
/// Gezeichnet wird [`crate::app::tool_row`] — dieselbe Funktion, die
/// [`RedactApp::top_bar`] benutzt. Ein Nachbau an dieser Stelle prüfte den
/// Nachbau; genau daran ist die erste Fassung dieses Tests vorbeigelaufen.
///
/// Erkannt wird der Fokus daran, was ein Druck auf die **Leertaste** auslöst —
/// egui aktiviert damit das fokussierte Bedienelement. Das ist zugleich die
/// Frage, auf die es ankommt: nicht „wo liegt eine Kennung“, sondern „kommt
/// man mit der Tastatur an diesen Knopf heran“. Ein Eintrag „—“ heißt: der
/// Tabulator hat nirgendwo etwas Benutzbares getroffen.
fn toolbar_tab_chain(context: crate::toolbar::ToolContext, steps: usize) -> Vec<String> {
    let ctx = egui::Context::default();
    let draw = |events: Vec<egui::Event>| -> Option<ToolAction> {
        let mut hit = None;
        let input = egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(2000.0, 800.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::TopBottomPanel::top("bar").show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    hit = tool_row(ui, &context);
                });
            });
        });
        hit
    };
    // Erst zeichnen, dann tabben: vor dem ersten Bild kennt egui die Widgets
    // noch nicht.
    let _ = draw(Vec::new());
    let mut chain = Vec::new();
    for _ in 0..steps {
        let _ = draw(vec![tab(false)]);
        let action = draw(vec![space()]);
        chain.push(match action {
            Some(action) => crate::toolbar::buttons()
                .iter()
                .find(|b| b.action == action)
                .map(|b| b.text.to_string())
                .expect("jede Aktion gehört zu einem Knopf"),
            None => "—".to_string(),
        });
    }
    chain
}

/// **Befund A3.** „Ausgegraut statt weggelassen“ ist die erklärte Regel dieser
/// Leiste ([`crate::toolbar::is_enabled`]) — und sie zerschnitt die
/// Tabulatorkette. Ein abgeschalteter Knopf verschluckte den Tastendruck: egui
/// meldet ihn erst als fokusinteressiert und nimmt ihm den Fokus im selben
/// Atemzug wieder weg. Der Fokus war danach **nirgends**, und der nächste
/// Tabulator begann wieder ganz vorn.
///
/// Folge im Normalbetrieb: „Wiederholen“ ist grau, solange nichts
/// zurückgenommen wurde — also war **alles dahinter** vorwärts unerreichbar:
/// Kleiner, Größer, Passend, 100 %, Zurück, Vor.
///
/// Behoben durch [`crate::app::greyable_button`]: ohne Klickabsicht ist ein
/// abgeschalteter Knopf nicht fokussierbar, der Tabulator geht an ihm vorbei.
#[test]
fn a3_a_greyed_out_button_no_longer_cuts_the_forward_tab_chain() {
    let everything = crate::toolbar::ToolContext {
        loaded: true,
        can_undo: true,
        can_redo: true,
        first_page: false,
        last_page: false,
        can_zoom_in: true,
        can_zoom_out: true,
        can_find_anything: true,
    };
    let count = crate::toolbar::buttons().len();
    // Ohne grauen Knopf ist jeder erreichbar — das ging vorher auch schon.
    let full = toolbar_tab_chain(everything, count + 2);
    for button in crate::toolbar::buttons() {
        assert!(
            full.contains(&button.text.to_string()),
            "ohne graue Knöpfe muss „{}“ erreichbar sein — Kette: {full:?}",
            button.text
        );
    }

    // Und jetzt der Alltag: „Wiederholen“ ist grau, sonst nichts.
    let one_grey = crate::toolbar::ToolContext {
        can_redo: false,
        ..everything
    };
    let chain = toolbar_tab_chain(one_grey, count + 2);
    assert!(
        !chain.contains(&"Wiederholen".to_string()),
        "der graue Knopf bekommt keinen Fokus: {chain:?}"
    );
    for behind in ["Kleiner", "Größer", "Passend", "100 %", "Zurück", "Vor"] {
        assert!(
            chain.contains(&behind.to_string()),
            "„{behind}“ steht hinter dem grauen Knopf und muss trotzdem \
             erreichbar sein — Kette: {chain:?}"
        );
    }

    // Der schlimmste Fall, den die Leiste hergibt: nur „Öffnen“ ist benutzbar.
    // Auch dann darf der Tabulator nicht ins Leere laufen.
    let nothing = crate::toolbar::ToolContext {
        can_find_anything: true,
        ..Default::default()
    };
    let chain = toolbar_tab_chain(nothing, 3);
    assert!(chain.contains(&"Öffnen".to_string()), "{chain:?}");
    for button in crate::toolbar::buttons() {
        if button.action == ToolAction::Open {
            continue;
        }
        assert!(
            !chain.contains(&button.text.to_string()),
            "„{}“ ist grau und darf keinen Fokus bekommen: {chain:?}",
            button.text
        );
    }
}

/// Dasselbe in der Seitenleiste — und dort traf es die Trefferliste.
///
/// Zwei Zustände, die die Kommandozeile ganz gewöhnlich herstellt, schalten
/// **das erste** Bedienelement der Spalte ab:
///
/// * `redact-rs --gui -o ziel.pdf` — dann ist „Namenszusatz“ abgeschaltet
///   ([`crate::sidebar::OUTPUT_FIXED_HINT`]), und es steht ganz oben;
/// * eine Buchungsliste mit Schutzeinträgen — deren Kästchen ist abgeschaltet
///   ([`AppState::set_enabled`]).
///
/// In beiden Fällen war **keine einzige Trefferzeile** mit dem Tabulator
/// vorwärts auszuwählen. Behoben durch `TextEdit::interactive(!fixed)` und
/// durch einen nicht fokussierbaren Platzhalter statt eines abgeschalteten
/// Kästchens.
#[test]
fn a3b_a_greyed_out_field_no_longer_blocks_the_hit_list() {
    assert_eq!(
        select_first_hit_by_keyboard(loaded(true)),
        Some(0),
        "im Normalfall ist die Trefferliste erreichbar"
    );

    // 1. `-o` schaltet „Namenszusatz“ ab.
    let mut fixed = loaded(true);
    fixed.state.config.output = Some(PathBuf::from("/tmp/ziel.pdf"));
    assert!(fixed.state.output_name_is_fixed());
    assert_eq!(
        select_first_hit_by_keyboard(fixed),
        Some(0),
        "auch mit -o muss die Trefferliste per Tabulator erreichbar sein"
    );

    // 2. Ein Schutzeintrag an erster Stelle schaltet dessen Kästchen ab.
    let mut blocked = loaded(true);
    let hit = blocked.state.regions[0].region.rect;
    blocked.state.regions.insert(
        0,
        AnnotatedRegion::new(Region::new(
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
        )),
    );
    assert!(blocked.state.regions[0].is_blocking());
    let reachable = hits_reachable_by_keyboard(blocked, 20);
    assert!(
        reachable.contains(&1) && reachable.contains(&2),
        "hinter einem Schutzeintrag müssen die echten Trefferzeilen erreichbar \
         sein — erreicht: {reachable:?}"
    );

    // 3. Beides zusammen — und zusätzlich der Hauptschalter aus, der die
    //    Musterkästchen abschaltet.
    let mut both = loaded(true);
    both.state.config.output = Some(PathBuf::from("/tmp/ziel.pdf"));
    both.apply_pattern_toggle(crate::sidebar::PatternToggle::All(false));
    both.state.regions.push(AnnotatedRegion::new(Region::new(
        0,
        Rect::new(100.0, 100.0, 200.0, 120.0),
        None,
        Source::Manual {
            reason: "von Hand".into(),
        },
    )));
    assert_eq!(
        select_first_hit_by_keyboard(both),
        Some(0),
        "auch mit abgeschalteten Musterkästchen bleibt die Liste erreichbar"
    );
}

// ===========================================================================
// B1 — Pfeiltasten und die Seite, die niemand sieht
// ===========================================================================

/// **Befund B1.** Der Zug am Eckgriff wird beendet, sobald geblättert wird —
/// [`RedactApp::apply_pointer`], Fall 1: „Zug beendet — die angefasste Region
/// liegt auf einer anderen Seite“. Für die **Pfeiltasten** gab es diese
/// Absicherung nicht.
///
/// Das Blättern hebt die Auswahl nicht auf ([`AppState::next_page`] fasst sie
/// nicht an), und [`key_commands`] macht aus einem Pfeil eine Verschiebung,
/// sobald *irgendetwas* ausgewählt ist. Wer also auf Seite 2 blätterte und dort
/// mit Pfeil links „eine Seite zurück“ wollte, verschob stattdessen den Balken
/// auf Seite 1 — den er nicht sah, weil `ui.painter_at` die andere Seite
/// zeichnet. Nichts sagte es: die Statuszeile trug weiter die Meldung der
/// Analyse.
///
/// Gemessen war: zehn Anschläge Umschalt+Pfeil links schoben den Balken 100 pt
/// nach links, und die IBAN stand danach **lesbar** in der exportierten Datei.
/// Jetzt bewegt sich nichts, und die Statuszeile sagt warum.
#[test]
fn b1_arrow_keys_do_not_move_a_region_on_a_page_that_is_not_shown() {
    let mut app = loaded(true);
    app.state.selected_region = Some(0);
    assert_eq!(app.state.regions[0].region.page, 0);
    let before = app.state.regions[0].region.rect;

    // Bild ab — jetzt zeigt das Fenster Seite 2.
    press(&mut app, keys(|k| k.page_down = true));
    assert_eq!(app.state.current_page, 1);
    assert_eq!(
        app.state.selected_region,
        Some(0),
        "die Auswahl bleibt auf der Region der ersten Seite — \
         „Zeile anklicken, Seite springt mit“ soll weiter gehen"
    );

    // Zehnmal „eine Seite zurück“ versuchen.
    for _ in 0..10 {
        press(
            &mut app,
            keys(|k| {
                k.left = true;
                k.shift = true;
            }),
        );
    }

    assert_eq!(
        app.state.regions[0].region.rect, before,
        "der Balken auf Seite 1 hat sich nicht bewegt"
    );
    assert!(
        matches!(app.state.regions[0].region.source, Source::Pattern { .. }),
        "und aus dem Mustertreffer ist auch nicht stillschweigend Handarbeit \
         geworden: {:?}",
        app.state.regions[0].region.source
    );
    // Und es steht dort, statt nur nicht zu passieren.
    assert_eq!(
        app.state.status,
        crate::state::selection_on_other_page(0, crate::state::NudgeKind::Move)
    );
    assert!(app.state.status.contains("Seite 1"), "{}", app.state.status);
    assert!(app.state.status.contains("Esc"), "{}", app.state.status);

    // Dasselbe gilt für die Größenänderung — sie geht durch dieselbe Prüfung.
    press(
        &mut app,
        keys(|k| {
            k.ctrl = true;
            k.right = true;
        }),
    );
    assert_eq!(app.state.regions[0].region.rect, before);

    // Der Export ist damit vollständig: beide IBANs sind weg.
    let out = tmp("b1").join("out.pdf");
    app.state.export(&out, None).expect("Export läuft");
    assert_gone(&out, "0532 0130 00");
    assert_gone(&out, "DE89");
    assert_gone(&out, "DE02");
    // Der Prüfer schlägt auch an: derselbe Weg findet Text, der stehen bleibt.
    assert_present(&out, "Kontoauszug");

    // Gegenprobe: auf der Seite, die gezeigt wird, schiebt derselbe Anschlag
    // nach wie vor.
    let mut same_page = loaded(true);
    same_page.state.selected_region = Some(0);
    let start = same_page.state.regions[0].region.rect;
    for _ in 0..10 {
        press(
            &mut same_page,
            keys(|k| {
                k.left = true;
                k.shift = true;
            }),
        );
    }
    assert_eq!(same_page.state.current_page, 0);
    assert!(same_page.state.regions[0].region.rect.ll.x < start.ll.x);

    // Und der Weg, den die Seitenleiste geht — Zeile anklicken, Seite springt
    // mit —, funktioniert unverändert.
    let mut via_list = loaded(true);
    via_list.state.set_page(1);
    let page = via_list.state.regions[1].region.page;
    via_list.state.selected_region = Some(1);
    via_list.state.set_page(page);
    let start = via_list.state.regions[1].region.rect;
    press(
        &mut via_list,
        keys(|k| {
            k.left = true;
            k.shift = true;
        }),
    );
    assert!(via_list.state.regions[1].region.rect.ll.x < start.ll.x);
}

// ===========================================================================
// B2 — Entartete MediaBox: Oberfläche und Kommandozeile gehen denselben Weg
// ===========================================================================

/// Ein Kontoauszug, dessen zweite Seite eine unbrauchbare MediaBox trägt.
fn statement_with_a_degenerate_second_page() -> Vec<u8> {
    let mut doc =
        lopdf::Document::load_mem(&redact_pdf::testing::demo_statement()).expect("Demo-PDF ladbar");
    let page2 = *doc.get_pages().values().nth(1).expect("zwei Seiten");
    doc.get_dictionary_mut(page2).expect("Seitendict").set(
        "MediaBox",
        lopdf::Object::Array(vec![0.into(), 0.into(), 0.into(), 0.into()]),
    );
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("speicherbar");
    out
}

/// **Befund B2.** `redact_render::raster::sane_box` fing entartete MediaBoxen
/// ab und nahm A4 an — der Rasterizer zeichnete die Seite also. `redact_pdf`s
/// `page_box` tat das **nicht**, und [`AppState::page_boxes`] kam von dort.
///
/// Die Folgen griffen ineinander:
///
/// * [`PageView::size_screen`] lieferte 1 × 1 Punkt — die Seite war im Fenster
///   nicht zu sehen, obwohl der Rasterizer ein Bild dazu geliefert hatte;
/// * [`AppState::clamp_to_page`] ließ von jedem Rechteck dieser Seite nichts
///   übrig, also galt der IBAN-Treffer als [`HitOutcome::OffPage`] und ging gar
///   nicht erst in [`AppState::enabled_redactions`];
/// * der Export meldete **keine** Warnung — die Stelle war ja aussortiert.
///
/// Ergebnis: die Oberfläche schrieb eine Datei, in der die IBAN der zweiten
/// Seite unverändert stand, während `redact_pipeline::run` mit **derselben**
/// [`Config`] beide Seiten schwärzte. Genau der Punkt, an dem die Zusage „die
/// Oberfläche bekommt dieselbe Config und benutzt sie auch“ brach.
///
/// Behoben, indem die Prüfung an **eine** Stelle gehoben wurde
/// ([`redact_pdf::document::sane_box`]) und beide Seiten dort fragen — und
/// indem die geheilte Seite **gesagt** wird.
#[test]
fn b2_a_degenerate_media_box_redacts_like_the_command_line_and_says_so() {
    let bytes = statement_with_a_degenerate_second_page();
    let dir = tmp("b2");
    let input = dir.join("kaputt.pdf");
    std::fs::write(&input, &bytes).expect("schreibbar");

    // --- Die Oberfläche ---
    let mut app = RedactApp::silent(iban_only());
    app.state
        .load_bytes(&bytes, Some(input.clone()))
        .expect("ladbar");
    app.state.analyze().expect("Analyse");

    let summary = app.state.hit_summary();
    assert_eq!(summary.outcomes.len(), 2, "beide IBANs werden gefunden");
    assert_eq!(summary.outcome(0), HitOutcome::Redacted);
    assert_eq!(
        summary.outcome(1),
        HitOutcome::Redacted,
        "und der Treffer auf Seite 2 fällt nicht mehr heraus"
    );
    assert_eq!(summary.off_page, 0);
    assert!(summary.headline().contains("2 werden geschwärzt"));

    // Die Seite ist im Fenster A4 groß — dieselbe Größe, die der Rasterizer
    // zeichnet.
    assert_eq!(app.state.page_box(1), Some(redact_pdf::document::A4));
    let view = app.state.page_view(1);
    assert!(
        view.size_screen(1.0).x > 500.0,
        "{:?}",
        view.size_screen(1.0)
    );

    // Und es steht dabei: eine geheilte Seite darf nicht aussehen wie jede
    // andere.
    assert_eq!(app.state.healed_pages, vec![1]);
    let notice = crate::state::healed_page_warning(1);
    assert!(
        app.state.warnings.contains(&notice),
        "keine Warnung zur geheilten Seite: {:?}",
        app.state.warnings
    );
    assert!(
        notice.contains("Seite 2") && notice.contains("A4"),
        "{notice}"
    );

    let gui_out = dir.join("gui.pdf");
    let outcome = app.state.export(&gui_out, None).expect("Export läuft");
    assert_eq!(outcome.drawn_rects, 2);
    assert_gone(&gui_out, "DE02 1203 0000 0000 2020 51");
    assert_gone(&gui_out, "DE89");
    assert_present(&gui_out, "Kontoauszug");

    // --- Dieselbe Datei, dieselbe Config, über die Kommandozeile ---
    let cli_out = dir.join("cli.pdf");
    let outcome = redact_pipeline::run(&Config {
        input,
        output: Some(cli_out.clone()),
        force: true,
        ..iban_only()
    })
    .expect("Kommandozeile läuft");
    assert_eq!(outcome.drawn_rects, 2, "die Kette schwärzt beide Seiten");
    assert_gone(&cli_out, "DE02");

    // Die Zusage in Zahlen: beide Wege schwärzen dasselbe.
    assert_eq!(
        app.state.enabled_redactions().len(),
        outcome.drawn_rects,
        "Oberfläche und Kommandozeile schwärzen gleich viel"
    );

    // Gegenprobe: eine **gesunde** Datei löst die Warnung nicht aus.
    let mut sound = RedactApp::silent(iban_only());
    sound
        .state
        .load_bytes(&redact_pdf::testing::demo_statement(), None)
        .expect("ladbar");
    assert!(sound.state.healed_pages.is_empty());
    assert!(
        sound.state.warnings.is_empty(),
        "{:?}",
        sound.state.warnings
    );
}

/// Und die Regel selbst, an ihrer einen Stelle.
#[test]
fn b2b_the_media_box_check_lives_in_exactly_one_place() {
    use redact_pdf::document::{sane_box, A4};

    let good = Rect::new(0.0, 0.0, 200.0, 300.0);
    let checked = sane_box(good);
    assert_eq!(checked.rect, good);
    assert!(!checked.is_replaced());
    assert_eq!(checked.warning(), None);

    for bad in [
        Rect::new(0.0, 0.0, 0.0, 0.0),
        Rect::new(0.0, 0.0, -0.5, -0.5),
        Rect::new(0.0, 0.0, 1.0e9, 1.0e9),
        Rect::new(0.0, 0.0, f64::NAN, 10.0),
    ] {
        let checked = sane_box(bad);
        assert_eq!(checked.rect, A4, "{bad:?}");
        assert!(checked.is_replaced(), "{bad:?}");
        assert!(checked.warning().is_some(), "{bad:?}");
    }

    // Der Rasterizer meldet für dieselbe Seite denselben Satz — er fragt
    // dieselbe Funktion.
    let bytes = statement_with_a_degenerate_second_page();
    let doc = lopdf::Document::load_mem(&bytes).expect("ladbar");
    let rendered = redact_render::PageRenderer::new().render(
        &doc,
        1,
        &redact_render::RenderOptions::default(),
    );
    assert!(!rendered.degraded);
    assert!(rendered.drawn_ops > 0);
    assert!(
        rendered
            .warnings
            .iter()
            .any(|w| w.contains("Unbrauchbare MediaBox")),
        "{:?}",
        rendered.warnings
    );
    // Und die Oberfläche kommt auf dieselbe Fläche wie das Bild.
    let mut app = RedactApp::silent(iban_only());
    app.state.load_bytes(&bytes, None).expect("ladbar");
    assert_eq!(app.state.page_view(1).display_box(), rendered.page_box);
}

// ===========================================================================
// B3 — Eine Region auf einer Seite, die es nicht gibt
// ===========================================================================

/// **Befund B3.** [`HitOutcome::OffPage`] ist in v0.4.0 dafür entstanden, dass
/// ein Rechteck neben dem Blatt nicht länger als „wird geschwärzt“ gezählt
/// wird — „**angesagt — vor dem Export und nicht erst als Warnung danach**“.
/// Der Nachbarfall war übrig geblieben: ein Rechteck auf einer Seite, die es im
/// Dokument gar nicht gibt.
///
/// [`AppState::is_off_page`] verlangte eine bekannte Seite und antwortete sonst
/// `false`. Die Kopfzeile versprach dann eine Schwärzung mehr, als der Export
/// ausführen konnte, die Miniaturspalte zeigte die Zahl nirgends — und die
/// Wahrheit kam als Warnung **nach** dem Schreiben, in genau der Form, die für
/// den Nachbarfall abgeschafft wurde.
///
/// Jetzt hat der Fall eine eigene Antwort ([`HitOutcome::MissingPage`]): er
/// zählt nicht mit, er steht in der Kopfzeile, und er nennt die Seitenzahl als
/// das, was zu berichtigen ist.
#[test]
fn b3_a_region_on_a_page_that_does_not_exist_is_announced_before_the_export() {
    let mut app = loaded(true);
    let sha = app.state.input_sha256.clone();
    let real = app.state.regions[0].region.clone();
    let ghost = Region::new(
        7,
        Rect::new(100.0, 100.0, 200.0, 120.0),
        Some("DE00 0000 0000 0000 0000 00".into()),
        Source::Manual {
            reason: "Seite 8 gibt es nicht".into(),
        },
    );
    let review = redact_core::ReviewFile::new(
        redact_core::ReviewInput {
            path: "demo.pdf".into(),
            sha256: sha,
            pages: 2,
        },
        vec![real, ghost],
        Vec::new(),
    );
    app.state
        .apply_review_file(review)
        .expect("Prüfsumme passt");

    let summary = app.state.hit_summary();
    assert_eq!(
        summary.outcome(1),
        HitOutcome::MissingPage,
        "die Oberfläche behauptet nicht mehr, das Rechteck werde geschwärzt"
    );
    assert_eq!(summary.missing_page, 1);
    assert_eq!(
        summary.off_page, 0,
        "und nennt es nicht „neben der Seite“ — dort ist die Lage falsch, \
         hier die Seitenzahl"
    );
    let headline = summary.headline();
    assert!(headline.contains("1 werden geschwärzt"), "{headline}");
    assert!(
        headline.contains("1 auf einer Seite, die es nicht gibt"),
        "{headline}"
    );
    // Die Zeile in der Liste sagt dasselbe, und die Sprechblase nennt die
    // Abhilfe.
    assert_eq!(HitOutcome::MissingPage.note(), "Seite gibt es nicht");
    assert!(crate::sidebar::outcome_tooltip(HitOutcome::MissingPage).contains("Seitenzahl"));
    assert_eq!(app.state.redactions_per_page(&summary), vec![1, 0]);

    // Und der Export tut genau das, was vorher angesagt wurde.
    let out = tmp("b3").join("out.pdf");
    let outcome = app.state.export(&out, None).expect("Export läuft");
    assert_eq!(outcome.drawn_rects, 1);

    // Ohne geladenes Dokument bleibt die Frage offen — dann ist sie auch nicht
    // zu beantworten, und ein Rechteck aus einer Regionsdatei darf nicht
    // vorschnell abgeschrieben werden.
    let mut blank = AppState::new();
    assert!(!blank.has_document());
    blank.regions.push(AnnotatedRegion::new(Region::new(
        7,
        Rect::new(100.0, 100.0, 200.0, 120.0),
        None,
        Source::Manual {
            reason: "noch kein Dokument".into(),
        },
    )));
    blank.regions[0].enabled = true;
    assert_eq!(blank.hit_summary().outcome(0), HitOutcome::Redacted);

    // Gegenprobe: dasselbe Rechteck **neben** einer vorhandenen Seite bleibt
    // `OffPage` — die beiden Fälle werden nicht verwechselt.
    let mut beside = loaded(true);
    let sheet = beside.state.page_box(0).unwrap();
    let index = beside.state.regions.len();
    beside.state.regions.push(AnnotatedRegion::new(Region::new(
        0,
        Rect::new(sheet.ur.x + 50.0, 100.0, sheet.ur.x + 250.0, 120.0),
        None,
        Source::Manual {
            reason: "neben dem Blatt".into(),
        },
    )));
    beside.state.regions[index].enabled = true;
    let summary = beside.state.hit_summary();
    assert_eq!(summary.outcome(index), HitOutcome::OffPage);
    assert!(summary.headline().contains("1 neben der Seite"));
    assert_eq!(summary.missing_page, 0);
}

// ===========================================================================
// B4 — „Analysieren“ ist grau und rät jetzt das Richtige
// ===========================================================================

/// **Befund B4.** Es gibt zwei Wege zu „kein Muster läuft“: den Schalter
/// „Automatisch suchen“ und das Abwählen jedes einzelnen Kästchens
/// ([`AppState::any_pattern_runs`] zählt beide, ausdrücklich).
/// [`crate::toolbar::ANALYZE_OFF_HINT`] kannte aber nur den ersten und riet,
/// ein Häkchen zu setzen, das in diesem Zustand längst gesetzt ist. Und
/// [`HitSummary::automatic`] hing ebenfalls nur am Schalter: die Kopfzeile
/// verlor ihre Vorwarnung und sagte „0 Treffer · 0 werden geschwärzt“ —
/// derselbe Satz wie bei einem Dokument, in dem wirklich nichts steht.
#[test]
fn b4_the_hint_and_the_headline_name_both_ways_to_switch_detection_off() {
    let mut app = loaded(true);
    // Der einzige Musterlauf dieses Aufrufs wird einzeln abgewählt.
    app.apply_pattern_toggle(crate::sidebar::PatternToggle::One {
        id: "iban_de".into(),
        on: false,
    });

    assert!(
        !app.state.analysis_can_find_anything(),
        "es gibt nichts mehr zu finden — „Analysieren“ ist grau"
    );
    assert!(!toolbar::is_enabled(
        ToolAction::Analyze,
        &app.tool_context()
    ));
    // Der Rat daneben trifft jetzt zu — er nennt **beide** Wege.
    assert!(app.state.patterns_enabled(), "das Häkchen ist gesetzt");
    assert!(
        toolbar::ANALYZE_OFF_HINT.contains("Automatisch suchen"),
        "{}",
        toolbar::ANALYZE_OFF_HINT
    );
    assert!(
        toolbar::ANALYZE_OFF_HINT.contains(crate::sidebar::PATTERN_LIST_TITLE),
        "die Liste, in der die Kästchen wirklich stehen, muss im Hinweis \
         vorkommen: {}",
        toolbar::ANALYZE_OFF_HINT
    );

    // Und die Kopfzeile behält ihre Vorwarnung — vor den Zahlen, die sie
    // umdeutet.
    let summary = app.state.hit_summary();
    assert!(!summary.automatic, "es läuft kein Muster");
    assert!(
        summary.detection_switch,
        "der Schalter steht trotzdem auf an"
    );
    let headline = summary.headline();
    assert!(
        headline.starts_with("Kein Muster läuft — nicht gesucht, nur von Hand:"),
        "Kopfzeile: {headline}"
    );
    assert!(
        headline.contains("0 Treffer · 0 werden geschwärzt"),
        "{headline}"
    );
    // Und die Zahl, die weiterhilft: wie viele Kästchen wieder anzukreuzen sind.
    assert!(headline.contains("1 Muster abgeschaltet"), "{headline}");

    // Gegenprobe 1: über den Schalter steht der andere Grund vorn — dort ist
    // das Häkchen wirklich der richtige Ort.
    let mut switched = loaded(true);
    switched.apply_pattern_toggle(crate::sidebar::PatternToggle::All(false));
    let headline = switched.state.hit_summary().headline();
    assert!(headline.starts_with("Automatische Suche AUS"), "{headline}");
    // Bei „ganz aus“ ist die Zahl der einzelnen Kästchen gegenstandslos.
    assert!(!headline.contains("Muster abgeschaltet"), "{headline}");

    // Gegenprobe 2: läuft ein Muster, gibt es keinen Vorbehalt.
    let plain = loaded(true);
    let summary = plain.state.hit_summary();
    assert!(summary.automatic && summary.detection_switch);
    assert!(summary
        .headline()
        .starts_with("2 Treffer · 2 werden geschwärzt"));
}

// ===========================================================================
// B5 — Ein Blatt ohne ein Wort sagt jetzt, dass es keines ist
// ===========================================================================

/// **Befund B5.** Der Auftrag lautete, eine Seite zu bauen, an der der
/// Rasterizer scheitert. Fünf Versuche — `/Contents` als fehlende Referenz,
/// als Zahl, als Dictionary, als Array mit Loch, als FlateDecode ohne Deflate
/// — führen **nicht** zu `RenderedPage::degraded`. Der Grund liegt in
/// `lopdf::Document::get_page_content`: es schluckt jeden Fehler und liefert
/// `Ok(vec![])`. `redact_pdf::ops::page_ops` gelingt damit, mit null
/// Operationen, und der Rasterizer meldet eine **gewöhnliche, leere** Seite.
///
/// Die schematische Vorschau springt also nicht ein — sie hat auch nichts zu
/// zeigen, denn der Extraktor findet auf derselben Seite ebenfalls nichts. Und
/// niemand sagte etwas: `extract_warnings` leer, der Rasterizer ohne Warnung,
/// die Kopfzeile nannte ihre Zahlen ohne jeden Vorbehalt. Der Nutzer sah ein
/// weißes Blatt und durfte daraus schließen, die Seite sei leer.
///
/// Der Notnagel-Pfad bleibt, wie er ist — er ist von hier aus nicht erreichbar,
/// und ihn erreichbar zu machen hieße, `lopdf` zu ändern. Stattdessen wird die
/// Auskunft benutzt, die schon bereitlag und nirgends gelesen wurde:
/// [`crate::render::PageMeta::drawn_ops`].
#[test]
fn b5_a_page_where_nothing_was_drawn_says_so() {
    let mut doc =
        lopdf::Document::load_mem(&redact_pdf::testing::demo_statement()).expect("Demo-PDF ladbar");
    let page2 = *doc.get_pages().values().nth(1).expect("zwei Seiten");
    doc.get_dictionary_mut(page2)
        .expect("Seitendict")
        .set("Contents", lopdf::Object::Reference((9999, 0)));
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("speicherbar");

    // Der Rasterizer hält die Seite unverändert für in Ordnung — und leer.
    // Daran ändert diese Runde nichts; das ist die Lage, mit der die
    // Oberfläche umgehen muss.
    let doc = lopdf::Document::load_mem(&bytes).expect("ladbar");
    let page = redact_render::PageRenderer::new().render(
        &doc,
        1,
        &redact_render::RenderOptions::default(),
    );
    assert!(
        !page.degraded,
        "der Notnagel-Pfad läuft gar nicht an — er ist von hier aus nicht erreichbar"
    );
    assert_eq!(page.drawn_ops, 0, "gezeichnet wird nichts");
    assert!(
        page.warnings.is_empty(),
        "und gesagt wird dort nichts: {:?}",
        page.warnings
    );
    assert_eq!(page.non_white_ratio(), 0.0, "das Bild ist reinweiß");

    // Die Oberfläche sagt es trotzdem — über `drawn_ops`.
    let mut cache = crate::render::PageCache::new();
    let ctx = egui::Context::default();
    cache.set_document(std::sync::Arc::new(
        lopdf::Document::load_mem(&bytes).expect("ladbar"),
    ));
    let view = crate::viewer::PageView::new(redact_pdf::document::A4, 0);
    cache.request(1, &view, 1.0, &ctx);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while cache.is_busy() && std::time::Instant::now() < deadline {
        cache.poll(&ctx);
    }
    assert!(!cache.is_busy(), "der Rasterizer ist nicht fertig geworden");
    assert!(
        cache.nothing_drawn(1),
        "auf Seite 2 wurde nichts gezeichnet — das muss die Oberfläche wissen"
    );
    assert!(
        !cache.nothing_drawn(0),
        "auf Seite 1 dagegen sehr wohl — sonst stünde der Satz überall"
    );
    // Und der Satz, der dann quer über dem Blatt steht, sagt beides: dass hier
    // nichts dargestellt wurde und dass hier nichts durchsucht wurde.
    for word in ["nichts dargestellt", "nicht durchsucht"] {
        assert!(
            crate::app::NOTHING_DRAWN_NOTICE.contains(word),
            "{}",
            crate::app::NOTHING_DRAWN_NOTICE
        );
    }
    assert!(!crate::app::NOTHING_DRAWN_MARK.is_empty());

    // Solange noch kein Bild vorliegt, wird nichts behauptet.
    let fresh = crate::render::PageCache::new();
    assert!(!fresh.nothing_drawn(0));
}

// ===========================================================================
// Geprüft und in Ordnung — die Liste, gegen die gegengeprüft wird
// ===========================================================================

/// Die im Auftrag genannte Folge — Muster abschalten, analysieren, wieder
/// einschalten, exportieren — täuscht **nicht**: die Datei und der Satz
/// darüber stimmen in jedem Schritt überein.
#[test]
fn ok_switching_detection_off_and_on_again_keeps_word_and_file_together() {
    let mut app = loaded(true);
    assert!(app
        .state
        .hit_summary()
        .headline()
        .starts_with("2 Treffer · 2 werden geschwärzt"));

    app.apply_pattern_toggle(crate::sidebar::PatternToggle::All(false));
    assert!(app.state.regions.is_empty());
    let headline = app.state.hit_summary().headline();
    assert!(headline.starts_with("Automatische Suche AUS"), "{headline}");
    // Und der Export verweigert sich, statt eine „geprüfte“ Kopie zu liefern.
    let dir = tmp("ok1");
    assert!(app.state.export(&dir.join("leer.pdf"), None).is_err());

    app.apply_pattern_toggle(crate::sidebar::PatternToggle::All(true));
    assert_eq!(app.state.regions.len(), 2);
    let headline = app.state.hit_summary().headline();
    assert!(
        headline.starts_with("2 Treffer · 2 werden geschwärzt"),
        "{headline}"
    );

    let out = dir.join("voll.pdf");
    app.state.export(&out, None).expect("Export läuft");
    let text = text_of(&out);
    assert!(!text.contains("DE89"), "{text}");
    assert!(!text.contains("DE02"), "{text}");
    assert_gone(&out, "DE89");
    assert_gone(&out, "DE02");
}

/// Eine erneute Analyse lässt ein Rechteck neben dem Blatt weiterhin als
/// „neben der Seite“ stehen — es wird weder gezählt noch stillschweigend
/// beschnitten.
#[test]
fn ok_off_page_survives_a_second_analysis_without_being_counted() {
    let mut app = loaded(true);
    let sheet = app.state.page_box(0).unwrap();
    let index = app.state.regions.len();
    app.state.regions.push(AnnotatedRegion::new(Region::new(
        0,
        Rect::new(sheet.ur.x + 50.0, 100.0, sheet.ur.x + 250.0, 120.0),
        None,
        Source::Manual {
            reason: "neben dem Blatt".into(),
        },
    )));
    app.state.regions[index].enabled = true;
    assert_eq!(app.state.hit_summary().outcome(index), HitOutcome::OffPage);

    app.state.analyze().expect("Analyse");
    let summary = app.state.hit_summary();
    let still = app
        .state
        .regions
        .iter()
        .position(|a| a.region.rect.ll.x > sheet.ur.x)
        .expect("die Region hat die Analyse überlebt");
    assert_eq!(summary.outcome(still), HitOutcome::OffPage);
    assert_eq!(summary.redacted, 2, "gezählt werden nur die beiden echten");
    assert!(summary.headline().contains("1 neben der Seite"));
}

/// Eine Schiebe-Sitzung ist **ein** Verlaufsschritt, und ein Löschen danach
/// ein eigener — auch über einen Seitenwechsel hinweg.
#[test]
fn ok_one_nudging_session_is_one_step_and_deleting_starts_a_new_one() {
    let mut app = loaded(true);
    app.state.selected_region = Some(0);
    let start = app.state.regions[0].region.rect;

    for _ in 0..25 {
        press(&mut app, keys(|k| k.left = true));
    }
    assert!(app.state.is_nudging());
    press(&mut app, keys(|k| k.page_down = true));
    press(&mut app, keys(|k| k.delete = true));
    assert_eq!(app.state.regions.len(), 1);
    assert!(!app.state.is_nudging());

    // Erster Schritt zurück: die Region ist wieder da, verschoben.
    assert!(app.state.undo());
    assert_eq!(app.state.regions.len(), 2);
    assert!(app.state.regions[0].region.rect.ll.x < start.ll.x);
    // Zweiter Schritt: alle 25 Anschläge auf einmal zurück.
    assert!(app.state.undo());
    assert_eq!(app.state.regions[0].region.rect, start);
}

/// Auch eine Größenänderung mit der Tastatur ist **eine** Sitzung — und sie
/// gehört zur selben wie das Schieben: es ist dieselbe Handbewegung an
/// derselben Region.
#[test]
fn ok_one_keyboard_resize_session_is_one_step() {
    let mut app = loaded(true);
    app.state.selected_region = Some(0);
    let start = app.state.regions[0].region.rect;
    let depth = app.state.history.undo_depth();

    for _ in 0..25 {
        press(
            &mut app,
            keys(|k| {
                k.ctrl = true;
                k.right = true;
            }),
        );
    }
    assert!(app.state.regions[0].region.rect.width() > start.width());
    assert_eq!(
        app.state.history.undo_depth(),
        depth + 1,
        "25 Anschläge sind ein Schritt"
    );
    assert!(app.state.undo());
    assert_eq!(app.state.regions[0].region.rect, start);
}

/// Ein geschützter Treffer, der mit den Pfeiltasten angefasst wird, verliert
/// den Schutz — sagt es — und Strg+Z nimmt beides zurück.
#[test]
fn ok_nudging_a_protected_hit_says_so_and_undo_takes_it_back() {
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
    assert_eq!(app.state.status, crate::state::PROTECTION_OVERRIDDEN);
    assert_eq!(app.state.hit_summary().outcome(0), HitOutcome::Redacted);

    press(
        &mut app,
        keys(|k| {
            k.ctrl = true;
            k.key_z = true;
        }),
    );
    assert_eq!(app.state.regions[0].region.rect, hit);
    assert_eq!(app.state.hit_summary().outcome(0), HitOutcome::Blocked);
}
