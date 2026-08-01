//! Linke Spalte: Seitenliste, Trefferliste und Details zur Auswahl.
//!
//! Das Modul enthält keinerlei Fachlogik — jeder Klick ruft eine Methode von
//! [`AppState`] auf. Was ein Treffer bedeutet und ob er am Ende wirklich
//! geschwärzt wird, entscheidet [`AppState::hit_summary`]; hier wird das nur
//! angezeigt.
//!
//! Die Seitenauswahl steckt seit den Miniaturansichten in
//! [`crate::thumbnails`] — zwei Seitenlisten nebeneinander sind eine zu viel,
//! und ein Bild sagt mehr als eine Zahl.
//!
//! ## Was hier bewusst anders ist als früher
//!
//! * Die Überschrift nennt **beide** Zahlen: gefundene Treffer und die, die
//!   tatsächlich geschwärzt werden. Nur die zweite ist für das Ergebnis
//!   relevant, und genau die fehlte.
//! * Treffer, die `resolve_conflicts` verwirft (blockiert, doppelt), werden
//!   ausgegraut und durchgestrichen. Vorher standen sie angehakt, farbig und
//!   gefüllt in der Liste — als würden sie geschwärzt.
//! * Geschützte Einträge (Negativliste) werden **nicht** durchgestrichen.
//!   Durchgestrichen liest sich wie „entfernt“; gemeint ist das Gegenteil,
//!   deshalb steht dort jetzt das Wort „geschützt“.
//! * Der Detailbereich klebt unten am Panel statt am Ende der Liste. Bei 150
//!   Treffern war er sonst nur nach langem Scrollen erreichbar.

use egui::{Color32, RichText};
use redact_core::Action;

use crate::state::{AppState, HitOutcome, HitSummary, RegionColor, REGION_COLORS};

/// Breite des Eingabefelds für den Namenszusatz.
///
/// Ausdrücklich `f32` — siehe die Anmerkung in [`crate::viewer`] zu
/// `float_literal_f32_fallback`.
const SUFFIX_FIELD_WIDTH: f32 = 120.0;

/// Farbe einer Region als egui-Farbe.
pub fn dot_color(color: RegionColor) -> Color32 {
    let (r, g, b) = color.rgb();
    Color32::from_rgb(r, g, b)
}

/// Zeichnet die gesamte Seitenleiste.
///
/// `summary` wird einmal je Bild von [`crate::app`] berechnet und
/// hereingereicht — die Konfliktauflösung soll nicht je Trefferzeile laufen.
pub fn show(ui: &mut egui::Ui, state: &mut AppState, summary: &HitSummary) {
    output_name(ui, state);
    ui.separator();

    ui.heading("Treffer");
    ui.label(RichText::new(summary.headline()).strong());
    legend(ui);
    padding_note(ui, state.config.padding);
    ui.separator();

    // Erst der Detailbereich am unteren Rand, dann die Liste in den Rest —
    // sonst schöbe die Liste die Details aus dem sichtbaren Bereich.
    egui::TopBottomPanel::bottom("hit_details")
        .resizable(false)
        .show_inside(ui, |ui| {
            ui.add_space(BAR_PADDING);
            details(ui, state, summary);
            ui.add_space(BAR_PADDING);
        });

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            hits(ui, state, summary);
        });

    // Sobald das Ersatzfeld den Fokus nicht (mehr) hat — oder gar nicht
    // gezeichnet wurde —, ist die Tippsitzung vorbei: der nächste Anschlag
    // gehört dann zu einem neuen Schritt im Verlauf. Eine Zeile an genau einer
    // Stelle statt einer Fallunterscheidung an jedem Ausgang von `details`.
    if !ui.memory(|m| m.has_focus(crate::focus::id(crate::focus::REPLACEMENT))) {
        state.end_replacement_edit();
    }
}

/// Vertikale Luft um den Detailbereich.
const BAR_PADDING: f32 = 4.0;

/// Namenszusatz der Ausgabedatei samt Vorschau des Ergebnisses.
fn output_name(ui: &mut egui::Ui, state: &mut AppState) {
    // Steht der Ausgabepfad durch `-o` fest, bewirkt der Zusatz nichts. Ein
    // Eingabefeld, an dem sichtbar nichts hängt, ist schlimmer als ein
    // abgeschaltetes: hier wird es abgeschaltet **und** gesagt, warum.
    let fixed = state.output_name_is_fixed();
    ui.horizontal(|ui| {
        ui.label("Namenszusatz");
        ui.add_enabled(
            !fixed,
            egui::TextEdit::singleline(&mut state.config.output_suffix)
                // Feste Kennung — siehe [`crate::focus`]: daran erkennt die
                // Tastenauswertung ein Textfeld.
                .id(crate::focus::id(crate::focus::OUTPUT_SUFFIX))
                .desired_width(SUFFIX_FIELD_WIDTH)
                .hint_text(redact_core::DEFAULT_OUTPUT_SUFFIX),
        )
        .on_hover_text(if fixed {
            OUTPUT_FIXED_HINT
        } else {
            "Wird an den Dateinamen der Ausgabe angehängt. \
             Leer bedeutet: Standardzusatz, damit das Original nie überschrieben wird."
        });
    });

    let suggestion = state
        .suggested_output_path()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "— kein Dokument geladen —".to_string());
    ui.label(RichText::new(suggestion).small().weak());
    if fixed {
        ui.label(RichText::new(OUTPUT_FIXED_NOTE).small().weak());
    }
}

/// Sprechblase am abgeschalteten Feld „Namenszusatz“.
pub const OUTPUT_FIXED_HINT: &str = "Der Ausgabepfad wurde beim Aufruf genannt (-o/--output). \
     Solange er gilt, hat der Namenszusatz keine Wirkung — im Speichern-Dialog \
     lässt sich der Name ändern, und das nächste geöffnete Dokument hebt ihn auf.";

/// Zeile unter dem Vorschlag, solange `-o` gilt.
pub const OUTPUT_FIXED_NOTE: &str = "Name aus -o — der Zusatz gilt hier nicht.";

/// Satz zur Polsterung — sie steht in Zahlen nirgends, wirkt aber auf jeden
/// Balken.
pub fn padding_text(padding: f64) -> String {
    if padding == 0.0 {
        return "Polsterung: keine — geschwärzt wird genau das Rechteck.".to_string();
    }
    format!(
        "Polsterung: {padding:+.1} pt rundum — der schwarze Balken wird auf jeder \
         Seite so viel größer als das Rechteck."
    )
}

/// Die Polsterung als Zeile unter der Legende.
///
/// Im Seitenbild ist sie als gestrichelter Rahmen zu sehen
/// ([`crate::viewer::paint_padding`]); hier steht die Zahl dazu. Vorher war
/// beides unsichtbar: `--padding 6` ließ Nachbartext mitverschwinden, ohne dass
/// die Oberfläche es andeutete.
fn padding_note(ui: &mut egui::Ui, padding: f64) {
    ui.label(RichText::new(padding_text(padding)).small().weak())
        .on_hover_text(
            "Kommt von --padding (Vorgabe 1,0). Der gestrichelte Rahmen im \
             Seitenbild zeigt, was wirklich schwarz wird.",
        );
}

fn legend(ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        for color in REGION_COLORS {
            ui.label(RichText::new(color.marker()).color(dot_color(color)));
            ui.label(RichText::new(color.label()).small().weak());
        }
    });
}

fn hits(ui: &mut egui::Ui, state: &mut AppState, summary: &HitSummary) {
    if state.regions.is_empty() {
        ui.label(RichText::new("Noch keine Treffer — „Analysieren“ oder Rechteck ziehen.").weak());
        return;
    }

    let mut toggle: Option<usize> = None;
    let mut select: Option<usize> = None;

    for index in 0..state.regions.len() {
        let entry = &state.regions[index];
        let blocking = entry.is_blocking();
        let selected = state.selected_region == Some(index);
        let mut enabled = entry.enabled;
        let color = entry.color;
        let label = entry.label();
        let outcome = summary.outcome(index);
        let tooltip = format!("{}\n{}", entry.description(), outcome_tooltip(outcome));

        ui.horizontal(|ui| {
            // Negativlisten-Treffer sind nicht schaltbar.
            let checkbox = ui.add_enabled(!blocking, egui::Checkbox::new(&mut enabled, ""));
            if checkbox.changed() {
                toggle = Some(index);
            }
            ui.label(RichText::new(color.marker()).color(dot_color(color)));

            let mut text = RichText::new(label);
            match outcome {
                // Wird geschwärzt: normal und in seiner Farbe.
                HitOutcome::Redacted => {}
                // Schützt Text — kein Durchstreichen, das hieße „gestrichen“.
                HitOutcome::Protecting => text = text.color(dot_color(color)),
                // Verworfen bzw. abgewählt: ausgegraut und durchgestrichen.
                HitOutcome::Disabled | HitOutcome::Blocked | HitOutcome::Duplicate => {
                    text = text.weak().strikethrough();
                }
            }
            if ui
                .selectable_label(selected, text)
                .on_hover_text(&tooltip)
                .clicked()
            {
                select = Some(index);
            }

            let note = outcome.note();
            if !note.is_empty() {
                ui.label(RichText::new(note).small().weak());
            }
        });
    }

    if let Some(index) = toggle {
        state.toggle_enabled(index);
    }
    if let Some(index) = select {
        let page = state.regions[index].region.page;
        state.selected_region = Some(index);
        state.set_page(page);
    }
}

/// Ein ganzer Satz zum Ergebnis — für die Sprechblase.
pub fn outcome_tooltip(outcome: HitOutcome) -> &'static str {
    match outcome {
        HitOutcome::Redacted => "Diese Stelle wird beim Export geschwärzt.",
        HitOutcome::Protecting => {
            "Diese Stelle steht auf Ihrer Liste der zu schützenden Texte und \
             wird nicht geschwärzt. Sie verhindert außerdem Schwärzungen darunter."
        }
        HitOutcome::Disabled => "Abgewählt — wird nicht geschwärzt.",
        HitOutcome::Blocked => {
            "Wird nicht geschwärzt: ein Eintrag Ihrer Liste schützt diese Stelle."
        }
        HitOutcome::Duplicate => {
            "Wird nicht eigens geschwärzt — ein anderer Treffer deckt dieselbe Stelle bereits ab."
        }
    }
}

/// Detailbereich für die ausgewählte Region.
fn details(ui: &mut egui::Ui, state: &mut AppState, summary: &HitSummary) {
    let Some(index) = state.selected_region else {
        ui.label(RichText::new("Keine Region ausgewählt.").weak());
        return;
    };
    let Some(entry) = state.regions.get(index) else {
        return;
    };

    let rect = entry.region.rect;
    let page = entry.region.page;
    let description = entry.description();
    let blocking = entry.is_blocking();
    let outcome = summary.outcome(index);
    let mut action = entry.action.clone();

    ui.label(RichText::new("Auswahl").strong());
    ui.label(format!("Seite {}", page + 1));
    ui.label(description);
    ui.label(
        RichText::new(format!(
            "x {:.1} … {:.1}   y {:.1} … {:.1}",
            rect.ll.x, rect.ur.x, rect.ll.y, rect.ur.y
        ))
        .small()
        .weak(),
    );

    if blocking {
        ui.label(
            RichText::new(outcome_tooltip(HitOutcome::Protecting))
                .small()
                .color(dot_color(RegionColor::AutoBookingNeg)),
        );
        return;
    }

    if !outcome.is_redacted() {
        ui.label(RichText::new(outcome_tooltip(outcome)).small().weak());
    }

    let mut changed = false;
    // Der Vorschlag für den Ersatztext kommt aus der Konfiguration
    // (`--replace-with`), sonst aus [`crate::state::DEFAULT_REPLACEMENT`] —
    // hier stand fest das englische „[REDACTED]“.
    let default_replacement = state.default_replacement();
    egui::ComboBox::from_label("Aktion")
        .selected_text(action_label(&action))
        .show_ui(ui, |ui| {
            for candidate in [
                Action::Blackout,
                Action::Whiteout,
                Action::Replace(default_replacement.clone()),
            ] {
                let text = action_label(&candidate);
                let is_selected =
                    std::mem::discriminant(&action) == std::mem::discriminant(&candidate);
                if ui.selectable_label(is_selected, text).clicked() && !is_selected {
                    action = candidate;
                    changed = true;
                }
            }
        });

    // Der Ersatztext geht **nicht** über `set_action`: sonst legte jeder
    // Tastendruck einen Verlaufsschritt an und ein Ersatztext von 50 Zeichen
    // schöbe den ganzen Stapel hinaus. Siehe
    // [`crate::state::AppState::edit_replacement`].
    let mut typed: Option<String> = None;
    if let Action::Replace(text) = &mut action {
        let mut buffer = text.clone();
        let field = ui.add(
            egui::TextEdit::singleline(&mut buffer)
                // Feste Kennung — daran erkennt die Tastenauswertung ein
                // Textfeld, siehe [`crate::focus`].
                .id(crate::focus::id(crate::focus::REPLACEMENT)),
        );
        if field.changed() {
            typed = Some(buffer.clone());
        }
        *text = buffer;
    }
    if changed {
        state.set_action(index, action);
    } else if let Some(text) = typed {
        state.edit_replacement(index, text);
    }

    if ui.button("Region löschen").clicked() {
        state.delete_selected();
    }
}

/// Beschriftung einer Schwärzungsart.
pub fn action_label(action: &Action) -> &'static str {
    match action {
        Action::Blackout => "Schwarz",
        Action::Whiteout => "Weiß",
        Action::Replace(_) => "Ersetzen",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Jedes Ergebnis braucht eine Erklärung, und die muss auf Deutsch
    /// erkennbar machen, ob geschwärzt wird oder nicht.
    #[test]
    fn every_outcome_explains_itself() {
        let outcomes = [
            HitOutcome::Redacted,
            HitOutcome::Protecting,
            HitOutcome::Disabled,
            HitOutcome::Blocked,
            HitOutcome::Duplicate,
        ];
        for outcome in outcomes {
            let text = outcome_tooltip(outcome);
            assert!(text.len() > 20, "{outcome:?}: {text}");
            assert_eq!(
                outcome.is_redacted(),
                !text.contains("nicht"),
                "{outcome:?}: {text}"
            );
        }
        // Nur „wird geschwärzt“ bekommt keinen Zusatz in der Zeile.
        assert!(HitOutcome::Redacted.note().is_empty());
        for outcome in outcomes.iter().filter(|o| !o.is_redacted()) {
            assert!(!outcome.note().is_empty(), "{outcome:?}");
        }
    }

    /// **Befund: mit `-o` war das Feld „Namenszusatz“ wirkungslos** — und sah
    /// aus wie jedes andere Eingabefeld. Hier am echten Kontext geprüft: das
    /// Feld ist abgeschaltet, und der Grund steht daneben.
    #[test]
    fn the_suffix_field_is_switched_off_while_an_output_path_is_given() {
        use std::cell::RefCell;
        use std::path::PathBuf;

        let field = crate::focus::id(crate::focus::OUTPUT_SUFFIX);

        let draw = |state: RefCell<AppState>| -> egui::Context {
            let ctx = egui::Context::default();
            for _ in 0..3 {
                let _ = ctx.run(egui::RawInput::default(), |ctx| {
                    egui::SidePanel::left("sidebar").show(ctx, |ui| {
                        let summary = state.borrow().hit_summary();
                        show(ui, &mut state.borrow_mut(), &summary);
                    });
                });
            }
            ctx
        };

        // Ohne `-o`: das Feld ist benutzbar.
        let mut plain = AppState::new();
        plain
            .load_bytes(&redact_pdf::testing::demo_statement(), None)
            .unwrap();
        let ctx = draw(RefCell::new(plain));
        assert!(
            ctx.read_response(field).expect("Feld gezeichnet").enabled(),
            "ohne -o gehört das Feld bedienbar"
        );

        // Mit `-o`: abgeschaltet — es bewirkte nichts.
        let mut fixed = AppState::with_config(redact_pipeline::Config {
            output: Some(PathBuf::from("/ziel/fertig.pdf")),
            ..redact_pipeline::Config::default()
        });
        fixed
            .load_bytes(&redact_pdf::testing::demo_statement(), None)
            .unwrap();
        assert!(fixed.output_name_is_fixed());
        let ctx = draw(RefCell::new(fixed));
        assert!(
            !ctx.read_response(field).expect("Feld gezeichnet").enabled(),
            "ein Feld, das sichtbar nichts bewirkt, gehört abgeschaltet"
        );
        assert!(
            OUTPUT_FIXED_HINT.contains("-o"),
            "und der Grund gehört dazu"
        );
        assert!(OUTPUT_FIXED_NOTE.contains("-o"));
    }

    #[test]
    fn dot_colors_differ_per_category() {
        let colors: Vec<Color32> = REGION_COLORS.iter().copied().map(dot_color).collect();
        for (i, a) in colors.iter().enumerate() {
            for b in colors.iter().skip(i + 1) {
                assert_ne!(a, b, "Farben müssen unterscheidbar sein");
            }
        }
    }

    /// Rauchtest ohne Bildschirm: die Seitenleiste muss sich mit leerem und
    /// mit gefülltem Zustand zeichnen lassen, ohne zu panicken.
    #[test]
    fn sidebar_renders_empty_and_populated() {
        use std::cell::RefCell;

        use redact_core::{MatchType, Rect, Region, Source};

        use crate::state::AnnotatedRegion;

        let empty = RefCell::new(AppState::new());
        egui::__run_test_ui(|ui| {
            let summary = empty.borrow().hit_summary();
            show(ui, &mut empty.borrow_mut(), &summary);
        });

        let mut populated = AppState::new();
        populated
            .load_bytes(&redact_pdf::testing::demo_statement(), None)
            .unwrap();
        populated.analyze().unwrap();
        populated.regions.push(AnnotatedRegion::new(Region::new(
            0,
            Rect::new(0.0, 0.0, 50.0, 12.0),
            Some("Max Mustermann".into()),
            Source::Booking {
                booking_id: "b003".into(),
                match_type: MatchType::Negative,
            },
        )));
        populated.add_manual_region(1, Rect::new(70.0, 700.0, 200.0, 715.0), "Adresse");
        populated.set_action(0, Action::Replace("[IBAN]".into()));

        let populated = RefCell::new(populated);
        egui::__run_test_ui(|ui| {
            let summary = populated.borrow().hit_summary();
            show(ui, &mut populated.borrow_mut(), &summary);
        });
        // Der Negativ-Eintrag bleibt aus, egal wie oft gezeichnet wird.
        assert!(populated
            .borrow()
            .regions
            .iter()
            .filter(|a| a.is_blocking())
            .all(|a| !a.enabled));
    }
}
