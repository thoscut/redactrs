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

/// Eine gewünschte Umschaltung der automatischen Erkennung.
///
/// Die Seitenleiste ändert den Zustand **nicht selbst**: beim Umschalten wird
/// die Trefferliste neu aufgebaut, und ob dabei Arbeit weggeworfen werden darf,
/// entscheidet die Rückfrage in [`crate::app::RedactApp`]. Ein hier direkt
/// gesetztes Kästchen ließe sich nach einem „Nein“ nicht mehr zurücknehmen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternToggle {
    /// Automatische Erkennung ganz (`true` = an).
    All(bool),
    /// Ein einzelnes Muster.
    One { id: String, on: bool },
}

/// Zeichnet die gesamte Seitenleiste.
///
/// `summary` wird einmal je Bild von [`crate::app`] berechnet und
/// hereingereicht — die Konfliktauflösung soll nicht je Trefferzeile laufen.
///
/// Rückgabe ist der Wunsch, die automatische Erkennung umzuschalten; ausgeführt
/// wird er von [`crate::app::RedactApp::apply_pattern_toggle`].
#[must_use]
pub fn show(
    ui: &mut egui::Ui,
    state: &mut AppState,
    summary: &HitSummary,
) -> Option<PatternToggle> {
    ui.add_space(SECTION_GAP);
    crate::app::section_title(ui, "Ausgabe");
    output_name(ui, state);
    ui.add_space(SECTION_GAP);

    crate::app::section_title(ui, "Treffer");
    ui.label(
        RichText::new(summary.headline())
            .strong()
            .size(15.0)
            .color(ui.visuals().strong_text_color()),
    );
    let toggle = detection(ui, state);
    // Legende und Polsterung erklären, was auf einer Seite zu sehen ist —
    // ohne Dokument gibt es nichts zu erklären.
    if state.is_loaded() {
        ui.add_space(2.0);
        legend(ui);
        padding_note(ui, state.config.padding);
    }
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

    hits(ui, state, summary);

    // Sobald das Ersatzfeld den Fokus nicht (mehr) hat — oder gar nicht
    // gezeichnet wurde —, ist die Tippsitzung vorbei: der nächste Anschlag
    // gehört dann zu einem neuen Schritt im Verlauf. Eine Zeile an genau einer
    // Stelle statt einer Fallunterscheidung an jedem Ausgang von `details`.
    if !ui.memory(|m| m.has_focus(crate::focus::id(crate::focus::REPLACEMENT))) {
        state.end_replacement_edit();
    }

    toggle
}

/// Sprechblase am Schalter für die automatische Erkennung.
pub const DETECTION_HINT: &str = "Aus heißt: kein einziger automatischer Treffer — \
     geschwärzt wird nur, was Sie selbst ziehen (oder was aus einer Buchungsliste kommt). \
     Entspricht --no-patterns auf der Kommandozeile und steht so auch im Protokoll.";

/// Überschrift der aufklappbaren Musterliste.
pub const PATTERN_LIST_TITLE: &str = "Muster einzeln";

/// Der Schalter für die automatische Erkennung — ganz und je Muster.
///
/// ## Warum er hier steht und nicht in der oberen Leiste
///
/// Er gehört zu den Treffern: er entscheidet, welche Zeilen darunter überhaupt
/// entstehen. Und er muss **an derselben Stelle sichtbar sein wie die Zahl, die
/// er erklärt** — „0 Treffer“ heißt mit abgeschalteter Erkennung nicht „nichts
/// gefunden“, sondern „nicht gesucht“. Deshalb steht die Ansage aus
/// [`AppState::detection_notice`] unmittelbar unter der Kopfzeile, in
/// Warnfarbe, und nicht bloß als Häkchenzustand.
fn detection(ui: &mut egui::Ui, state: &AppState) -> Option<PatternToggle> {
    let mut wish = None;

    let mut on = state.patterns_enabled();
    if crate::app::accent_checkbox(ui, &mut on, "Automatisch suchen")
        .on_hover_text(DETECTION_HINT)
        .changed()
    {
        wish = Some(PatternToggle::All(on));
    }

    // Der Zustand muss ohne Klick und ohne Sprechblase zu sehen sein: wer eine
    // so entstandene Datei für vollständig geprüft hält, hat sie nicht geprüft.
    if let Some(notice) = state.detection_notice() {
        ui.label(RichText::new(notice).small().color(warning_color(ui)));
    }

    // Die Liste ist zugeklappt, solange nichts abgeschaltet ist — offen sonst:
    // eine Abschaltung soll man sehen, ohne danach zu suchen.
    egui::CollapsingHeader::new(PATTERN_LIST_TITLE)
        .default_open(state.disabled_pattern_count() > 0)
        .show(ui, |ui| {
            // Bei „alles aus“ ändert ein einzelnes Häkchen nichts — es bleibt
            // trotzdem stehen (nicht ausgeblendet), damit die gemerkte Auswahl
            // sichtbar ist. Schaltbar ist es dann nicht, und aus demselben
            // Grund wie bei den Schutzeinträgen steht dort ein
            // [`inert_checkbox`]: ein abgeschaltetes Kästchen verschluckt den
            // Tabulator, und dahinter liegt die ganze Trefferliste.
            let switch_on = state.patterns_enabled();
            ui.add_enabled_ui(switch_on, |ui| {
                for def in state.pattern_states() {
                    if !switch_on {
                        ui.add(inert_checkbox(def.enabled, &def.id))
                            .on_hover_text(&def.description);
                        continue;
                    }
                    let mut single = def.enabled;
                    if crate::app::accent_checkbox(ui, &mut single, def.id.as_str())
                        .on_hover_text(&def.description)
                        .changed()
                    {
                        wish = Some(PatternToggle::One {
                            id: def.id.clone(),
                            on: single,
                        });
                    }
                }
                if state.pattern_states().is_empty() {
                    ui.label(
                        RichText::new("— keine Muster in diesem Lauf —")
                            .color(crate::theme::muted(ui)),
                    );
                }
            });
        });

    wish
}

/// Die Farbe für „hier stimmt etwas nicht ganz“ — dieselbe wie in der
/// Statuszeile für Warnungen.
fn warning_color(ui: &egui::Ui) -> Color32 {
    ui.visuals().warn_fg_color
}

/// Vertikale Luft um den Detailbereich.
const BAR_PADDING: f32 = 8.0;

/// Luft vor einer Zwischenüberschrift.
const SECTION_GAP: f32 = 6.0;

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
                // **Zusätzlich** zu `add_enabled`, nicht statt seiner: ein
                // abgeschaltetes Widget verschluckt in egui 0.29 den
                // Tabulator (siehe `crate::app::greyable_button`), und dieses
                // Feld steht **ganz oben** in der Spalte. Ein Aufruf
                // `redact-rs --gui -o ziel.pdf` machte damit keine einzige
                // Trefferzeile vorwärts ertabbar — die Liste, um die es in
                // dieser Spalte geht. `interactive(false)` nimmt dem Feld die
                // Fokussierbarkeit; grau gezeichnet wird es weiterhin.
                .interactive(!fixed)
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

    // Ein unbrauchbarer Zusatz (Pfadtrenner, `..`, Steuerzeichen) wird beim
    // Export ohnehin abgelehnt. Ihn erst dort zu melden hiesse: der Vorschlag
    // darunter zeigt einen entschärften Namen, alles sieht in Ordnung aus, und
    // der Fehler kommt erst nach dem Klick. Deshalb sofort, an Ort und Stelle.
    if let Err(e) = redact_core::check_output_suffix(&state.config.output_suffix) {
        let colour = ui.visuals().error_fg_color;
        ui.label(RichText::new(e.to_string()).small().color(colour));
        return;
    }

    let suggestion = state
        .suggested_output_path()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "— kein Dokument geladen —".to_string());
    ui.label(
        RichText::new(suggestion)
            .small()
            .color(crate::theme::muted(ui)),
    );
    if fixed {
        ui.label(
            RichText::new(OUTPUT_FIXED_NOTE)
                .small()
                .color(crate::theme::muted(ui)),
        );
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
    ui.label(
        RichText::new(padding_text(padding))
            .small()
            .color(crate::theme::muted(ui)),
    )
    .on_hover_text(
        "Kommt von --padding (Vorgabe 1,0). Der gestrichelte Rahmen im \
             Seitenbild zeigt, was wirklich schwarz wird.",
    );
}

/// Die vier Trefferfarben, zwei je Zeile — ein Zeichen und sein Wort bleiben
/// zusammen, statt mitten im Paar umzubrechen.
fn legend(ui: &mut egui::Ui) {
    egui::Grid::new("legend")
        .num_columns(2)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            for (i, color) in REGION_COLORS.into_iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    marker(ui, color);
                    ui.label(RichText::new(color.label()).small());
                });
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
}

/// Die Form, die eine Trefferart neben ihrer Farbe trägt.
///
/// Farbe allein reicht nicht (WCAG 1.4.1): Blau, Grün, Rot und Orange liegen
/// bei einer Rot-Grün-Sehschwäche dicht beieinander. Gemalt statt als
/// Schriftzeichen — die Standardschriften von egui kennen `●`, `◆` und `▲`
/// nicht, sie erschienen als leere Kästchen und alle vier Arten sahen bis auf
/// die Farbe gleich aus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerShape {
    Circle,
    Diamond,
    Square,
    Triangle,
}

/// Die Form zur Trefferart — dieselbe Zuordnung wie
/// [`RegionColor::marker`].
pub fn marker_shape(color: RegionColor) -> MarkerShape {
    match color {
        RegionColor::AutoPattern => MarkerShape::Circle,
        RegionColor::AutoBookingPos => MarkerShape::Diamond,
        RegionColor::AutoBookingNeg => MarkerShape::Square,
        RegionColor::Manual => MarkerShape::Triangle,
    }
}

/// Kantenlänge des Platzes für ein Zeichen der Trefferart.
const MARKER_SIZE: f32 = 14.0;

/// Malt das Zeichen der Trefferart in einen Platz von [`MARKER_SIZE`].
pub fn marker(ui: &mut egui::Ui, color: RegionColor) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::Vec2::splat(MARKER_SIZE), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        paint_marker(ui.painter(), rect.center(), color);
    }
    response.on_hover_text(color.label())
}

/// Malt das Zeichen der Trefferart um `center`.
pub fn paint_marker(painter: &egui::Painter, center: egui::Pos2, color: RegionColor) {
    use egui::{pos2, Shape, Stroke};
    let fill = dot_color(color);
    let (x, y) = (center.x, center.y);
    match marker_shape(color) {
        MarkerShape::Circle => {
            painter.circle_filled(center, 4.5, fill);
        }
        MarkerShape::Square => {
            painter.rect_filled(
                egui::Rect::from_center_size(center, egui::Vec2::splat(8.5)),
                1.0,
                fill,
            );
        }
        MarkerShape::Diamond => {
            painter.add(Shape::convex_polygon(
                vec![
                    pos2(x, y - 5.5),
                    pos2(x + 5.5, y),
                    pos2(x, y + 5.5),
                    pos2(x - 5.5, y),
                ],
                fill,
                Stroke::NONE,
            ));
        }
        MarkerShape::Triangle => {
            painter.add(Shape::convex_polygon(
                vec![
                    pos2(x, y - 5.0),
                    pos2(x + 5.5, y + 4.5),
                    pos2(x - 5.5, y + 4.5),
                ],
                fill,
                Stroke::NONE,
            ));
        }
    }
}

/// Höhe einer Trefferzeile ohne Zwischenraum.
///
/// Jede Zeile ist so hoch wie das Kästchen: `interact_size.y`, das größte
/// Element darin. Sie muss **fest** sein, weil die Liste nur die sichtbaren
/// Zeilen aufbaut ([`egui::ScrollArea::show_rows`]) und die Höhe der übrigen
/// aus dieser Zahl rechnet. Dass sie stimmt, prüft ein Test gegen die
/// gezeichnete Zeile — mit und ohne Zusatz, mit echtem und trägem Kästchen.
pub fn hit_row_height(ui: &egui::Ui) -> f32 {
    ui.spacing().interact_size.y + HIT_ROW_EXTRA
}

/// Was eine Trefferzeile höher ist als ein Bedienelement — Luft, damit die
/// Zeilen als Zeilen lesbar sind und die ganze Zeile ein bequemes Ziel ist.
const HIT_ROW_EXTRA: f32 = 4.0;
/// Breite der Spalte mit der Seitenzahl („S.12“).
const PAGE_COLUMN: f32 = 34.0;

/// Die Trefferliste — nur die **sichtbaren** Zeilen werden aufgebaut.
///
/// Vorher lief `for index in 0..regions.len()` durch die ganze Liste, in
/// jedem Bild: 10 000 Zeilen kosteten 87 ms je Bild, 50 000 Zeilen 527 ms —
/// und `--max-candidates` erlaubt 100 000. Egui zeichnet unsichtbare Widgets
/// zwar nicht, baut aber jeden Text auf. [`egui::ScrollArea::show_rows`]
/// überspringt, was außerhalb des Fensters liegt.
fn hits(ui: &mut egui::Ui, state: &mut AppState, summary: &HitSummary) {
    if state.regions.is_empty() {
        ui.label(
            RichText::new("Noch keine Treffer — „Analysieren“ oder Rechteck ziehen.")
                .color(crate::theme::muted(ui)),
        );
        return;
    }

    let mut toggle: Option<usize> = None;
    let mut select: Option<usize> = None;

    let row_height = hit_row_height(ui);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, row_height, state.regions.len(), |ui, rows| {
            for index in rows {
                hit_row(ui, state, summary, index, &mut toggle, &mut select);
            }
        });

    if let Some(index) = toggle {
        state.toggle_enabled(index);
    }
    if let Some(index) = select {
        let page = state.regions[index].region.page;
        state.selected_region = Some(index);
        state.set_page(page);
    }
}

/// Eine Zeile der Trefferliste; gibt die Antwort der ganzen Zeile zurück,
/// damit ein Test ihre Höhe gegen [`hit_row_height`] halten kann.
pub(crate) fn hit_row(
    ui: &mut egui::Ui,
    state: &AppState,
    summary: &HitSummary,
    index: usize,
    toggle: &mut Option<usize>,
    select: &mut Option<usize>,
) -> egui::Response {
    let entry = &state.regions[index];
    let blocking = entry.is_blocking();
    let selected = state.selected_region == Some(index);
    let mut enabled = entry.enabled;
    let color = entry.color;
    let label = entry.label();
    let outcome = summary.outcome(index);
    let tooltip = format!("{}\n{}", entry.description(), outcome_tooltip(outcome));

    // Die ganze Zeile ist das Ziel: ein Klick irgendwo darauf wählt den
    // Treffer, und die Hervorhebung reicht über die volle Breite. Die Fläche
    // wird **vor** dem Häkchenfeld angelegt — in egui liegt, was später kommt,
    // obenauf, und das Häkchenfeld muss seine Klicks selbst bekommen.
    let size = egui::vec2(ui.available_width(), hit_row_height(ui));
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let row = ui.interact(rect, ui.id().with(("hit_row", index)), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if selected {
            Some(ui.visuals().selection.bg_fill)
        } else if row.hovered() {
            Some(ui.visuals().faint_bg_color)
        } else {
            None
        };
        if let Some(fill) = fill {
            ui.painter()
                .rect_filled(rect, crate::theme::WIDGET_ROUNDING, fill);
        }
        if row.has_focus() {
            ui.painter().rect_stroke(
                rect.shrink(1.0),
                crate::theme::WIDGET_ROUNDING,
                ui.visuals().widgets.active.bg_stroke,
            );
        }
    }

    let text_color = if selected {
        ui.visuals().selection.stroke.color
    } else {
        ui.visuals().strong_text_color()
    };
    let (page_text, value) = label
        .split_once(' ')
        .map(|(page, value)| (page.to_string(), value.to_string()))
        .unwrap_or_else(|| (String::new(), label.clone()));

    ui.allocate_new_ui(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(6.0, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            // Negativlisten-Treffer sind nicht schaltbar. Für sie steht hier
            // ein Platzhalter statt eines abgeschalteten Kästchens: ein
            // abgeschaltetes Widget verschluckt in egui 0.29 den Tabulator
            // (siehe [`crate::app::greyable_button`]), und dieses Kästchen ist
            // das **erste** Bedienelement jeder Trefferzeile. Eine
            // Buchungsliste mit einem Schutzeintrag an erster Stelle machte
            // damit keine einzige Trefferzeile vorwärts ertabbar.
            if blocking {
                ui.add_enabled(false, inert_checkbox(enabled, ""));
            } else {
                let checkbox = crate::app::accent_checkbox(ui, &mut enabled, "");
                if checkbox.changed() {
                    *toggle = Some(index);
                }
            }
            marker(ui, color);

            let (page_rect, _) = ui
                .allocate_exact_size(egui::vec2(PAGE_COLUMN, rect.height()), egui::Sense::hover());
            ui.painter().text(
                page_rect.left_center(),
                egui::Align2::LEFT_CENTER,
                &page_text,
                egui::TextStyle::Small.resolve(ui.style()),
                crate::theme::muted(ui),
            );

            // Feste Zeichenbreite: IBANs und Kontonummern stehen untereinander.
            let mut text = RichText::new(value).monospace().color(text_color);
            match outcome {
                // Wird geschwärzt: normal.
                HitOutcome::Redacted => {}
                // Schützt Text — kein Durchstreichen, das hieße „gestrichen“.
                HitOutcome::Protecting => text = text.color(dot_color(color)),
                // Verworfen bzw. abgewählt: gedämpft und durchgestrichen.
                HitOutcome::Disabled
                | HitOutcome::Blocked
                | HitOutcome::Duplicate
                | HitOutcome::OffPage
                | HitOutcome::MissingPage => {
                    text = text.color(crate::theme::muted(ui)).strikethrough();
                }
            }

            let note = outcome.note();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !note.is_empty() {
                    ui.label(RichText::new(note).small().color(crate::theme::muted(ui)));
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(text).truncate().selectable(false));
                });
            });
        },
    );

    // Die Zeile meldet sich mit ihrem Inhalt — für Sprechblase und Tastatur.
    row.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, &label)
    });
    let row = row.on_hover_text(&tooltip);
    if row.clicked() {
        *select = Some(index);
    }
    row
}

/// Ein Kästchen, das nur aussieht wie eins — nicht anklickbar, nicht
/// fokussierbar, und deshalb kein Hindernis für den Tabulator.
///
/// Gezeichnet wird Zug um Zug wie [`egui::Checkbox`] im abgeschalteten
/// Zustand: dieselben Maße (`icon_width`, `interact_size`), dieselben
/// Rechtecke (`icon_rectangles`) und ausdrücklich die `inactive`-Farben, die
/// egui einem abgeschalteten Kästchen gibt. Die Ausgrauung selbst kommt
/// weiterhin von [`egui::Ui::add_enabled`], das den Maler abblendet. Optisch
/// ist der Unterschied zum bisherigen Stand also keiner.
///
/// Der Unterschied liegt in [`egui::Sense::hover`]: ohne Klickabsicht ist das
/// Element nicht fokussierbar, der Tabulator geht daran vorbei zur nächsten
/// Zeile, und der Tastendruck ist nicht verbraucht.
fn inert_checkbox(checked: bool, text: &str) -> impl egui::Widget + '_ {
    move |ui: &mut egui::Ui| {
        let (icon_width, icon_spacing) = {
            let spacing = ui.spacing();
            (spacing.icon_width, spacing.icon_spacing)
        };
        let interact_height = ui.spacing().interact_size.y;
        let (galley, mut size) = if text.is_empty() {
            (None, egui::vec2(icon_width, 0.0))
        } else {
            let extra = egui::vec2(icon_width + icon_spacing, 0.0);
            let wrap_width = ui.available_width() - extra.x;
            let galley = egui::WidgetText::from(text).into_galley(
                ui,
                None,
                wrap_width,
                egui::TextStyle::Button,
            );
            let size = (extra + galley.size()).max(ui.spacing().interact_size);
            (Some(galley), size)
        };
        size = size.max(egui::Vec2::splat(interact_height));
        size.y = size.y.max(icon_width);

        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
        // Für Hilfsmittel bleibt es ein Kästchen — es ist nur keines, das man
        // umschalten könnte.
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Checkbox,
                false,
                checked,
                galley.as_ref().map_or("", |g| g.text()),
            )
        });
        if ui.is_rect_visible(rect) {
            let visuals = ui.visuals().widgets.inactive;
            let (small, big) = ui.spacing().icon_rectangles(rect);
            ui.painter().add(egui::epaint::RectShape::new(
                big.expand(visuals.expansion),
                visuals.rounding,
                visuals.bg_fill,
                visuals.bg_stroke,
            ));
            if checked {
                ui.painter().add(egui::Shape::line(
                    vec![
                        egui::pos2(small.left(), small.center().y),
                        egui::pos2(small.center().x, small.bottom()),
                        egui::pos2(small.right(), small.top()),
                    ],
                    visuals.fg_stroke,
                ));
            }
            if let Some(galley) = galley {
                let at = egui::pos2(
                    rect.min.x + icon_width + icon_spacing,
                    rect.center().y - 0.5 * galley.size().y,
                );
                ui.painter().galley(at, galley, visuals.text_color());
            }
        }
        response
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
        HitOutcome::OffPage => {
            "Dieses Rechteck liegt vollständig neben dem Blatt und wird nicht geschwärzt — \
             es kann dort kein Zeichen treffen."
        }
        HitOutcome::MissingPage => {
            "Dieses Rechteck nennt eine Seite, die es in diesem Dokument nicht gibt. \
             Es wird nicht geschwärzt — zu berichtigen ist die Seitenzahl, nicht die Lage."
        }
    }
}

/// Detailbereich für die ausgewählte Region.
fn details(ui: &mut egui::Ui, state: &mut AppState, summary: &HitSummary) {
    let Some(index) = state.selected_region else {
        crate::app::section_title(ui, "Auswahl");
        ui.label(
            RichText::new("Keine Region ausgewählt — in der Liste oder auf der Seite anklicken.")
                .color(crate::theme::muted(ui)),
        );
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

    crate::app::section_title(ui, "Auswahl");
    let color = entry.color;
    let value = entry
        .label()
        .split_once(' ')
        .map(|(_, value)| value.to_string())
        .unwrap_or_default();
    ui.horizontal(|ui| {
        marker(ui, color);
        ui.add(
            egui::Label::new(
                RichText::new(value)
                    .monospace()
                    .size(14.5)
                    .color(ui.visuals().strong_text_color()),
            )
            .truncate(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // `saturating_add`, weil die Seitennummer aus fremder Hand kommt
            // (Review-Datei, `--manual-regions`): bei `usize::MAX` liefe die
            // 1-basierte Anzeige im Debug-Build über und löste eine Panic aus;
            // im Release-Build stünde „Seite 0" da. Die erste Verteidigung ist
            // `redact_core::model::MAX_PAGE_INDEX` an der Deserialisierung;
            // diese hier gilt für jeden Aufrufer, der `Region` selbst baut.
            ui.label(
                RichText::new(format!("Seite {}", page.saturating_add(1)))
                    .color(crate::theme::muted(ui)),
            );
        });
    });
    ui.label(description);
    ui.label(
        RichText::new(format!(
            "x {:.1} … {:.1}   y {:.1} … {:.1}",
            rect.ll.x, rect.ur.x, rect.ll.y, rect.ur.y
        ))
        .small()
        .color(crate::theme::muted(ui)),
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
        ui.label(
            RichText::new(outcome_tooltip(outcome))
                .small()
                .color(crate::theme::muted(ui)),
        );
    }

    let mut changed = false;
    // Der Vorschlag für den Ersatztext kommt aus der Konfiguration
    // (`--replace-with`), sonst aus [`redact_core::DEFAULT_REPLACEMENT`] —
    // hier stand fest das englische „[REDACTED]“.
    let default_replacement = state.default_replacement();
    let mut typed: Option<String> = None;
    ui.add_space(4.0);
    egui::Grid::new("selection_details")
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            ui.label("Aktion");
            egui::ComboBox::from_id_salt("region_action")
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
            ui.end_row();

            // Der Ersatztext geht **nicht** über `set_action`: sonst legte jeder
            // Tastendruck einen Verlaufsschritt an und ein Ersatztext von 50
            // Zeichen schöbe den ganzen Stapel hinaus. Siehe
            // [`crate::state::AppState::edit_replacement`].
            if let Action::Replace(text) = &mut action {
                ui.label("Ersatztext");
                let mut buffer = text.clone();
                let field = ui.add(
                    egui::TextEdit::singleline(&mut buffer)
                        // Feste Kennung — daran erkennt die Tastenauswertung
                        // ein Textfeld, siehe [`crate::focus`].
                        .id(crate::focus::id(crate::focus::REPLACEMENT)),
                );
                if field.changed() {
                    typed = Some(buffer.clone());
                }
                *text = buffer;
                ui.end_row();
            }
        });
    if changed {
        state.set_action(index, action);
    } else if let Some(text) = typed {
        state.edit_replacement(index, text);
    }

    ui.add_space(8.0);
    let danger = ui.visuals().error_fg_color;
    if ui
        .add_sized(
            egui::vec2(ui.available_width(), ui.spacing().interact_size.y + 4.0),
            egui::Button::new(RichText::new("Region löschen").color(danger)),
        )
        .on_hover_text("Entfernt diese Region aus der Liste (Entf) — rückgängig mit Strg+Z")
        .clicked()
    {
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
    use std::ops::DerefMut;

    use super::*;

    /// Jede Trefferart hat ihre eigene Form — Farbe allein reicht nicht
    /// (WCAG 1.4.1) —, und die Form wird gemalt, nicht als Schriftzeichen
    /// gesetzt: die Standardschriften von egui kennen `●`, `◆` und `▲` nicht.
    #[test]
    fn every_hit_kind_has_its_own_painted_shape() {
        for (i, a) in REGION_COLORS.iter().enumerate() {
            for b in &REGION_COLORS[i + 1..] {
                assert_ne!(marker_shape(*a), marker_shape(*b), "{a:?} und {b:?}");
            }
        }
        // Belegt, warum nicht mehr das Zeichen aus `RegionColor::marker`
        // gesetzt wird: mindestens eines fehlt in der Schrift.
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        let font = egui::FontId::proportional(14.0);
        let missing = ctx.fonts(|fonts| {
            REGION_COLORS
                .iter()
                .any(|c| !fonts.has_glyphs(&font, c.marker()))
        });
        assert!(missing, "alle Zeichen vorhanden — dann ginge auch Schrift");
        // Und gemalt wird tatsächlich etwas.
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                for color in REGION_COLORS {
                    marker(ui, color);
                }
            });
        });
        assert!(output.shapes.len() >= REGION_COLORS.len());
    }

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
            HitOutcome::OffPage,
            HitOutcome::MissingPage,
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

    // ------------------------- Abschaltbare automatische Funde (Aufgabe #81)

    /// Aller Text, den ein Bild wirklich auf den Schirm gemalt hat.
    ///
    /// Über die gezeichneten Formen und nicht über den Zustand: die Frage
    /// lautet „steht es da?“, und die beantwortet nur das, was gemalt wurde.
    fn painted_text(output: &egui::FullOutput) -> String {
        fn walk(shape: &egui::Shape, into: &mut String) {
            match shape {
                egui::Shape::Text(text) => {
                    into.push_str(text.galley.text());
                    into.push('\n');
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, into);
                    }
                }
                _ => {}
            }
        }
        let mut text = String::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut text);
        }
        text
    }

    /// Zeichnet die Seitenleiste und gibt zurück, was dabei zu lesen war.
    fn draw(state: &mut AppState) -> String {
        use std::cell::RefCell;

        let ctx = egui::Context::default();
        let state = RefCell::new(state);
        let mut painted = String::new();
        // Dreimal: egui braucht ein Bild, bis Panelmaße und aufklappbare
        // Bereiche stehen.
        for _ in 0..3 {
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                egui::SidePanel::left("sidebar").show(ctx, |ui| {
                    let summary = state.borrow().hit_summary();
                    let _ = show(ui, state.borrow_mut().deref_mut(), &summary);
                });
            });
            painted = painted_text(&output);
        }
        painted
    }

    /// **Die Auflage (3): der Zustand muss an der Oberfläche zu sehen sein.**
    ///
    /// Nicht in einer Sprechblase, nicht erst nach einem Klick — als Text auf
    /// dem Schirm, neben der Zahl, die er erklärt. „0 Treffer“ heißt mit
    /// abgeschalteter Erkennung nicht „nichts gefunden“, sondern „nicht
    /// gesucht“, und das ist der Unterschied zwischen einer geprüften und einer
    /// ungeprüften Datei.
    #[test]
    fn a_switched_off_detection_is_written_on_the_screen() {
        // Alles an: der Schalter ist da, eine Warnung nicht.
        let mut state = AppState::new();
        state
            .load_bytes(&redact_pdf::testing::demo_statement(), None)
            .unwrap();
        let painted = draw(&mut state);
        assert!(painted.contains("Automatisch suchen"), "{painted}");
        assert!(
            !painted.contains(redact_pipeline::DETECTION_NOTICE),
            "ohne Abschaltung gehört da keine Warnung hin:\n{painted}"
        );

        // Ganz aus: der Satz steht da, wörtlich derselbe wie im Audit-Log.
        state.set_patterns_enabled(false);
        let painted = draw(&mut state);
        let notice = state.detection_notice().expect("es ist etwas abgeschaltet");
        assert!(
            painted.contains(&notice),
            "die Abschaltung steht nirgends auf dem Schirm:\n{painted}"
        );
        assert!(notice.contains("--no-patterns"), "{notice}");

        // Einzeln aus: ebenso, mit dem Namen des Musters.
        state.set_patterns_enabled(true);
        state.set_pattern_enabled("bic", false);
        let painted = draw(&mut state);
        assert!(
            painted.contains(&state.detection_notice().unwrap()),
            "{painted}"
        );
        assert!(painted.contains("bic"), "{painted}");
    }

    /// Die Kästchen zeigen den **tatsächlichen** Zustand, nicht die Vorgabe.
    #[test]
    fn the_pattern_list_shows_the_state_of_this_run() {
        let mut state = AppState::new();
        state
            .load_bytes(&redact_pdf::testing::demo_statement(), None)
            .unwrap();
        // Aufgeklappt ist die Liste nur, wenn etwas abgeschaltet ist — dann
        // müssen die Namen zu sehen sein.
        state.set_pattern_enabled("date_de", false);
        let painted = draw(&mut state);
        assert!(painted.contains(PATTERN_LIST_TITLE), "{painted}");
        assert!(painted.contains("iban_de"), "{painted}");
        assert!(painted.contains("date_de"), "{painted}");
        assert!(state.pattern_enabled("iban_de"));
        assert!(!state.pattern_enabled("date_de"));
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
                        let _ = show(ui, &mut state.borrow_mut(), &summary);
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
            let _ = show(ui, &mut empty.borrow_mut(), &summary);
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
            let _ = show(ui, &mut populated.borrow_mut(), &summary);
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
