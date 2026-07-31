//! Linke Spalte: Seitenliste, Trefferliste und Details zur Auswahl.
//!
//! Das Modul enthält keinerlei Fachlogik — jeder Klick ruft eine Methode von
//! [`AppState`] auf. Farbige Punkte markieren die Herkunft eines Treffers,
//! Negativlisten-Einträge werden durchgestrichen und lassen sich nicht
//! einschalten.

use egui::{Color32, RichText};
use redact_core::Action;

use crate::state::{AppState, RegionColor};

/// Zeichen für den Farbpunkt vor einem Treffer.
///
/// Bewusst `●` (U+25CF) statt eines farbigen Emoji: die eingebauten
/// egui-Schriften decken die Emoji-Blöcke nur lückenhaft ab, ein eingefärbter
/// Kreis wird dagegen garantiert dargestellt — und die Farbe ist genau die
/// Information, um die es geht.
pub const COLOR_DOT: &str = "●";

/// Breite des Eingabefelds für den Namenszusatz.
///
/// Ausdrücklich `f32` — siehe die Anmerkung in [`crate::viewer`] zu
/// `float_literal_f32_fallback`.
const SUFFIX_FIELD_WIDTH: f32 = 120.0;

/// Höhe der Seitenliste.
const PAGE_LIST_HEIGHT: f32 = 64.0;

/// Farbe einer Region als egui-Farbe.
pub fn dot_color(color: RegionColor) -> Color32 {
    let (r, g, b) = color.rgb();
    Color32::from_rgb(r, g, b)
}

/// Zeichnet die gesamte Seitenleiste.
pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    output_name(ui, state);
    ui.separator();

    ui.heading("Seiten");
    pages(ui, state);
    ui.separator();

    ui.horizontal(|ui| {
        ui.heading("Treffer");
        ui.label(RichText::new(format!("({})", state.regions.len())).weak());
    });
    legend(ui);
    ui.separator();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            hits(ui, state);
        });
}

/// Namenszusatz der Ausgabedatei samt Vorschau des Ergebnisses.
fn output_name(ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        ui.label("Namenszusatz");
        ui.add(
            egui::TextEdit::singleline(&mut state.output_suffix)
                .desired_width(SUFFIX_FIELD_WIDTH)
                .hint_text(redact_core::DEFAULT_OUTPUT_SUFFIX),
        )
        .on_hover_text(
            "Wird an den Dateinamen der Ausgabe angehängt. \
             Leer bedeutet: Standardzusatz, damit das Original nie überschrieben wird.",
        );
    });

    let suggestion = state
        .suggested_output_path()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "— kein Dokument geladen —".to_string());
    ui.label(RichText::new(suggestion).small().weak());
}

fn pages(ui: &mut egui::Ui, state: &mut AppState) {
    let count = state.page_count();
    if count == 0 {
        ui.label(RichText::new("kein Dokument").weak());
        return;
    }
    egui::ScrollArea::horizontal()
        .id_salt("page_list")
        .max_height(PAGE_LIST_HEIGHT)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for page in 0..count {
                    let hits = state.regions_on_page(page).len();
                    let label = if hits > 0 {
                        format!("{} ({hits})", page + 1)
                    } else {
                        format!("{}", page + 1)
                    };
                    let selected = page == state.current_page;
                    if ui.selectable_label(selected, label).clicked() {
                        state.set_page(page);
                    }
                }
            });
        });
}

fn legend(ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        for color in [
            RegionColor::AutoPattern,
            RegionColor::AutoBookingPos,
            RegionColor::AutoBookingNeg,
            RegionColor::Manual,
        ] {
            ui.label(RichText::new(COLOR_DOT).color(dot_color(color)));
            ui.label(RichText::new(color.label()).small().weak());
        }
    });
}

fn hits(ui: &mut egui::Ui, state: &mut AppState) {
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
        let was_enabled = entry.enabled;
        let color = entry.color;
        let label = entry.label();
        let tooltip = entry.region.reason();

        ui.horizontal(|ui| {
            // Negativlisten-Treffer sind nicht schaltbar.
            let checkbox = ui.add_enabled(!blocking, egui::Checkbox::new(&mut enabled, ""));
            if checkbox.changed() {
                toggle = Some(index);
            }
            ui.label(RichText::new(COLOR_DOT).color(dot_color(color)));

            let mut text = RichText::new(label);
            if blocking {
                text = text.strikethrough().color(dot_color(color));
            } else if !was_enabled {
                text = text.weak();
            }
            if ui
                .selectable_label(selected, text)
                .on_hover_text(tooltip)
                .clicked()
            {
                select = Some(index);
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

    ui.separator();
    details(ui, state);
}

/// Detailbereich für die ausgewählte Region.
fn details(ui: &mut egui::Ui, state: &mut AppState) {
    let Some(index) = state.selected_region else {
        ui.label(RichText::new("Keine Region ausgewählt.").weak());
        return;
    };
    let Some(entry) = state.regions.get(index) else {
        return;
    };

    let rect = entry.region.rect;
    let page = entry.region.page;
    let reason = entry.region.reason();
    let blocking = entry.is_blocking();
    let mut action = entry.action.clone();

    ui.label(RichText::new("Auswahl").strong());
    ui.label(format!("Seite {}", page + 1));
    ui.label(reason);
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
            RichText::new("Negativliste — wird nie geschwärzt und blockiert Treffer darunter.")
                .small()
                .color(dot_color(RegionColor::AutoBookingNeg)),
        );
        return;
    }

    let mut changed = false;
    egui::ComboBox::from_label("Aktion")
        .selected_text(action_label(&action))
        .show_ui(ui, |ui| {
            for candidate in [
                Action::Blackout,
                Action::Whiteout,
                Action::Replace("[REDACTED]".to_string()),
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

    if let Action::Replace(text) = &mut action {
        if ui.text_edit_singleline(text).changed() {
            changed = true;
        }
    }
    if changed {
        state.set_action(index, action);
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

    #[test]
    fn every_action_has_a_label() {
        assert_eq!(action_label(&Action::Blackout), "Schwarz");
        assert_eq!(action_label(&Action::Whiteout), "Weiß");
        assert_eq!(action_label(&Action::Replace("x".into())), "Ersetzen");
    }

    #[test]
    fn dot_colors_differ_per_category() {
        let colors = [
            dot_color(RegionColor::AutoPattern),
            dot_color(RegionColor::AutoBookingPos),
            dot_color(RegionColor::AutoBookingNeg),
            dot_color(RegionColor::Manual),
        ];
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
        egui::__run_test_ui(|ui| show(ui, &mut empty.borrow_mut()));

        let mut populated = AppState::new();
        populated
            .load_bytes(&redact_pdf::testing::demo_statement(), None)
            .unwrap();
        populated.analyze(&["iban_de".to_string()], None).unwrap();
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
        egui::__run_test_ui(|ui| show(ui, &mut populated.borrow_mut()));
        // Der Negativ-Eintrag bleibt aus, egal wie oft gezeichnet wird.
        assert!(populated
            .borrow()
            .regions
            .iter()
            .filter(|a| a.is_blocking())
            .all(|a| !a.enabled));
    }
}
