//! Die Symbolleiste — als Daten.
//!
//! Welche Knöpfe es gibt, was sie heißen und wann sie benutzbar sind, steht
//! hier in gewöhnlichen Werten. [`crate::app`] zeichnet sie nur noch und
//! führt die zurückgemeldete [`ToolAction`] aus. Das hat zwei Gründe:
//!
//! * die Beschriftungen und die „Ist der Knopf jetzt benutzbar?“-Regeln sind
//!   ohne Fenster prüfbar (siehe Tests unten);
//! * **Symbol und Text**, nie nur ein Symbol. Eine reine Icon-Leiste zwingt
//!   zum Raten; wer einmal ein PDF falsch exportiert hat, rät nicht gern.

/// Was ein Knopf auslöst.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolAction {
    Open,
    Analyze,
    /// Legt ein Rechteck fester Größe in der Mitte der aktuellen Seite an.
    ///
    /// **Der einzige Weg zu einem eigenen Rechteck, der ohne Zeigegerät
    /// auskommt.** `AppState::add_manual_region` wurde bis hierher an genau
    /// einer Stelle gerufen — aus einem `PointerFrame`. Auf einem Kontoauszug
    /// sind Anschrift, Kontonummer und der Name des Kontoinhabers aber genau
    /// die Stellen, die kein Muster zuverlässig findet: sie *müssen* von Hand
    /// gezogen werden. Wer keine Maus benutzen kann, konnte diese Datei also
    /// nicht vollständig schwärzen.
    AddRegion,
    Booking,
    Export,
    ReviewSave,
    ReviewLoad,
    Undo,
    Redo,
    ZoomOut,
    ZoomIn,
    ZoomFit,
    ZoomReset,
    PrevPage,
    NextPage,
    ToggleTheme,
}

/// Ein Knopf der Leiste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolButton {
    /// Symbol — steht **vor** dem Text, nie an seiner Stelle.
    pub icon: &'static str,
    pub text: &'static str,
    /// Sprechblase; nennt auch das Tastenkürzel.
    pub hint: &'static str,
    pub action: ToolAction,
}

impl ToolButton {
    /// Beschriftung des Knopfes: Symbol, Leerzeichen, Text.
    pub fn label(&self) -> String {
        format!("{} {}", self.icon, self.text)
    }
}

/// Knopf oder Trenner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolItem {
    Button(ToolButton),
    Separator,
}

const fn button(
    icon: &'static str,
    text: &'static str,
    hint: &'static str,
    action: ToolAction,
) -> ToolItem {
    ToolItem::Button(ToolButton {
        icon,
        text,
        hint,
        action,
    })
}

/// Die Leiste von links nach rechts: erst die Datei, dann die Trefferliste,
/// dann die Ansicht.
///
/// Die Symbole stammen aus dem Zeichenvorrat, den egui mit seinen
/// Standardschriften mitbringt — der Test `every_icon_has_a_glyph` sorgt
/// dafür, dass keines als leeres Kästchen erscheint.
pub fn items() -> Vec<ToolItem> {
    vec![
        button("🗁", "Öffnen", "PDF-Datei öffnen (Strg+O)", ToolAction::Open),
        button(
            "🔍",
            "Analysieren",
            "Muster und Buchungsliste erneut suchen",
            ToolAction::Analyze,
        ),
        button(
            "🔲",
            "Rechteck",
            "Rechteck in der Mitte der Seite anlegen und auswählen (Strg+R) — \
             danach mit den Pfeiltasten schieben und mit Strg+Pfeil in der Größe ändern",
            ToolAction::AddRegion,
        ),
        button(
            "🗐",
            "Buchungsliste",
            "CSV mit zu schwärzenden und zu schützenden Buchungen laden",
            ToolAction::Booking,
        ),
        button(
            "💾",
            "Exportieren",
            "Geschwärztes PDF speichern (Strg+S)",
            ToolAction::Export,
        ),
        ToolItem::Separator,
        button(
            "🗄",
            "Review speichern",
            "Aktuellen Stand als JSON sichern — mit Prüfsumme des Dokuments",
            ToolAction::ReviewSave,
        ),
        button(
            "📋",
            "Review laden",
            "Gesicherten Stand übernehmen (nur zum passenden Dokument)",
            ToolAction::ReviewLoad,
        ),
        ToolItem::Separator,
        button(
            "↺",
            "Rückgängig",
            "Letzte Änderung an der Trefferliste zurücknehmen (Strg+Z)",
            ToolAction::Undo,
        ),
        button(
            "↻",
            "Wiederholen",
            "Zurückgenommene Änderung erneut ausführen (Strg+Y)",
            ToolAction::Redo,
        ),
        ToolItem::Separator,
        button(
            "➖",
            "Kleiner",
            "Eine Stufe kleiner darstellen",
            ToolAction::ZoomOut,
        ),
        button(
            "➕",
            "Größer",
            "Eine Stufe größer darstellen",
            ToolAction::ZoomIn,
        ),
        button(
            "⛶",
            "Passend",
            "Ganze Seite ins Fenster einpassen",
            ToolAction::ZoomFit,
        ),
        button(
            "⟲",
            "100 %",
            "Darstellung auf Originalgröße zurücksetzen",
            ToolAction::ZoomReset,
        ),
        ToolItem::Separator,
        button(
            "⏴",
            "Zurück",
            "Eine Seite zurück (Bild auf, Pfeil links)",
            ToolAction::PrevPage,
        ),
        button(
            "⏵",
            "Vor",
            "Eine Seite vor (Bild ab, Pfeil rechts)",
            ToolAction::NextPage,
        ),
    ]
}

/// Nur die Knöpfe, ohne Trenner.
pub fn buttons() -> Vec<ToolButton> {
    items()
        .into_iter()
        .filter_map(|item| match item {
            ToolItem::Button(button) => Some(button),
            ToolItem::Separator => None,
        })
        .collect()
}

/// Sprechblase an „Analysieren“, solange es nichts zu finden gibt.
///
/// Dasselbe Muster wie [`crate::sidebar::OUTPUT_FIXED_HINT`]: der Knopf wird
/// **abgeschaltet und der Grund steht daneben**. Ein Knopf, der sich drücken
/// lässt und dann eine leere Trefferliste hinterlässt, ist die schlechtere
/// Antwort — er sähe aus wie „nichts gefunden“.
///
/// **Beide Wege zu „kein Muster läuft“ stehen darin.** Der Satz kannte nur den
/// Hauptschalter und riet, ein Häkchen zu setzen, das im zweiten Fall längst
/// gesetzt ist: wer das letzte laufende Muster einzeln abwählt, liest sonst
/// eine Anweisung, die er nicht befolgen kann.
/// [`crate::state::AppState::any_pattern_runs`] zählt beide Wege, also muss
/// der Hinweis daneben beide nennen. Die Liste heißt
/// [`crate::sidebar::PATTERN_LIST_TITLE`]; dass der Name hier wörtlich steht,
/// hält der Test `the_hint_names_both_ways_to_switch_detection_off` fest.
pub const ANALYZE_OFF_HINT: &str = "Es läuft kein Muster, und es steht weder eine \
     Buchungsliste noch eine Regionsdatei dahinter — es gäbe nichts zu finden. \
     Abhilfe: das Häkchen „Automatisch suchen“ in der Trefferliste setzen, unter \
     „Muster einzeln“ wieder mindestens ein Muster ankreuzen, oder eine \
     Buchungsliste laden.";

/// Woran hängt, ob ein Knopf benutzbar ist.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ToolContext {
    pub loaded: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub first_page: bool,
    pub last_page: bool,
    pub can_zoom_in: bool,
    pub can_zoom_out: bool,
    /// Hat „Analysieren“ überhaupt eine Quelle, aus der es schöpfen könnte?
    ///
    /// Falsch, sobald die automatische Erkennung aus ist **und** weder eine
    /// Buchungsliste noch eine Regionsdatei dahintersteht — dann liefert der
    /// Knopf eine leere Liste und wirft dafür jede Entscheidung weg. Siehe
    /// [`crate::state::AppState::analysis_can_find_anything`].
    pub can_find_anything: bool,
}

/// Darf dieser Knopf gedrückt werden?
///
/// Ausgegraut statt weggelassen: ein Knopf, der verschwindet, wirkt wie ein
/// Fehler der Anwendung; ein grauer Knopf sagt „geht jetzt gerade nicht“.
pub fn is_enabled(action: ToolAction, context: &ToolContext) -> bool {
    match action {
        // Öffnen geht immer, sonst käme man nie zu einem Dokument.
        ToolAction::Open | ToolAction::ToggleTheme => true,
        // Ein Knopf, der nachweislich nichts finden kann, gehört ausgegraut —
        // und zwar mit einem Satz daneben, der sagt warum (das macht
        // [`crate::app::RedactApp::top_bar`] mit [`ANALYZE_OFF_HINT`]). Dasselbe
        // Muster wie beim Feld „Namenszusatz“, das mit `-o` nichts bewirkt.
        ToolAction::Analyze => context.loaded && context.can_find_anything,
        ToolAction::AddRegion
        | ToolAction::Booking
        | ToolAction::Export
        | ToolAction::ReviewSave
        | ToolAction::ReviewLoad
        | ToolAction::ZoomFit
        | ToolAction::ZoomReset => context.loaded,
        ToolAction::Undo => context.can_undo,
        ToolAction::Redo => context.can_redo,
        ToolAction::ZoomIn => context.loaded && context.can_zoom_in,
        ToolAction::ZoomOut => context.loaded && context.can_zoom_out,
        ToolAction::PrevPage => context.loaded && !context.first_page,
        ToolAction::NextPage => context.loaded && !context.last_page,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Symbol **und** Text — an dieser Regel hängt die ganze Leiste.
    #[test]
    fn every_button_carries_an_icon_and_a_word() {
        for button in buttons() {
            assert!(!button.icon.is_empty(), "{button:?}: kein Symbol");
            assert!(!button.text.is_empty(), "{button:?}: kein Text");
            // Der Text ist ein Wort, kein Zeichen.
            assert!(
                button.text.chars().count() >= 3,
                "{button:?}: Text zu kurz, das ist wieder nur ein Symbol"
            );
            assert!(button.hint.len() > 10, "{button:?}: Sprechblase zu dürftig");
            assert!(button.label().starts_with(button.icon));
            assert!(button.label().ends_with(button.text));
        }
    }

    /// Keine zwei Knöpfe dürfen gleich heißen oder dieselbe Aktion auslösen.
    #[test]
    fn buttons_are_unique() {
        let all = buttons();
        for (i, a) in all.iter().enumerate() {
            for b in all.iter().skip(i + 1) {
                assert_ne!(a.action, b.action, "{a:?} und {b:?}");
                assert_ne!(a.text, b.text, "{a:?} und {b:?}");
            }
        }
        // Die Leiste ist gegliedert, nicht eine Reihe aus fünfzehn Knöpfen.
        assert!(
            items()
                .iter()
                .filter(|i| matches!(i, ToolItem::Separator))
                .count()
                >= 3
        );
    }

    /// Ein Symbol, das die Schrift nicht kennt, erscheint als leeres Kästchen.
    /// Genau das darf nicht passieren — deshalb hier gegen die echten
    /// Standardschriften von egui geprüft.
    #[test]
    fn every_icon_has_a_glyph() {
        let ctx = egui::Context::default();
        // Vor dem ersten `run` gibt es noch keine Schriften.
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        let font = egui::FontId::proportional(14.0);
        ctx.fonts(|fonts| {
            for button in buttons() {
                assert!(
                    fonts.has_glyphs(&font, button.icon),
                    "Symbol {:?} von „{}“ fehlt in der Standardschrift — \
                     es erschiene als leeres Kästchen",
                    button.icon,
                    button.text
                );
            }
            // Dieselbe Regel gilt für jedes andere Zeichen, das die Oberfläche
            // als Symbol benutzt. Das Zeichen in der Miniaturspalte steht
            // neben einem **weißen** Kleinbild; erschiene es als leeres
            // Kästchen, wäre der Hinweis nicht bloß hässlich, sondern
            // unlesbar. Siehe [`crate::render::PageCache::nothing_drawn`].
            assert!(
                fonts.has_glyphs(&font, crate::app::NOTHING_DRAWN_MARK),
                "Zeichen {:?} für „auf dieser Seite wurde nichts gezeichnet“ \
                 fehlt in der Standardschrift",
                crate::app::NOTHING_DRAWN_MARK
            );
        });
    }

    #[test]
    fn nothing_but_opening_works_without_a_document() {
        let empty = ToolContext {
            // Ausdrücklich gesetzt: sonst prüfte der Test „Analysieren“ aus
            // dem falschen Grund als abgeschaltet.
            can_find_anything: true,
            ..ToolContext::default()
        };
        for button in buttons() {
            let enabled = is_enabled(button.action, &empty);
            assert_eq!(
                enabled,
                button.action == ToolAction::Open,
                "{button:?} ohne Dokument"
            );
        }
    }

    /// „Analysieren“ ist grau, sobald es nichts zu finden gäbe — und wieder
    /// benutzbar, sobald es eine Quelle gibt.
    #[test]
    fn analysing_is_greyed_out_when_there_is_nothing_left_to_find() {
        let loaded = ToolContext {
            loaded: true,
            can_find_anything: true,
            ..ToolContext::default()
        };
        assert!(is_enabled(ToolAction::Analyze, &loaded));

        let nothing = ToolContext {
            can_find_anything: false,
            ..loaded
        };
        assert!(!is_enabled(ToolAction::Analyze, &nothing));
        // Die übrigen Knöpfe bleiben davon unberührt — abgeschaltete Muster
        // sind kein Grund, das Exportieren zu verbieten.
        assert!(is_enabled(ToolAction::Export, &nothing));
        assert!(is_enabled(ToolAction::Booking, &nothing));
        assert!(ANALYZE_OFF_HINT.contains("Automatisch suchen"));
    }

    #[test]
    fn navigation_and_history_follow_the_state() {
        let loaded = ToolContext {
            loaded: true,
            can_zoom_in: true,
            can_zoom_out: true,
            can_find_anything: true,
            ..Default::default()
        };

        // Erste Seite: „Zurück“ ist tot, „Vor“ lebt.
        let first = ToolContext {
            first_page: true,
            ..loaded
        };
        assert!(!is_enabled(ToolAction::PrevPage, &first));
        assert!(is_enabled(ToolAction::NextPage, &first));

        let last = ToolContext {
            last_page: true,
            ..loaded
        };
        assert!(is_enabled(ToolAction::PrevPage, &last));
        assert!(!is_enabled(ToolAction::NextPage, &last));

        // Ohne Verlauf sind beide Verlaufsknöpfe grau …
        assert!(!is_enabled(ToolAction::Undo, &loaded));
        assert!(!is_enabled(ToolAction::Redo, &loaded));
        // … mit Verlauf nicht mehr, und das auch ohne geladenes Dokument
        // (Rückgängig darf nie an der Datei hängen).
        let with_history = ToolContext {
            can_undo: true,
            can_redo: true,
            ..ToolContext::default()
        };
        assert!(is_enabled(ToolAction::Undo, &with_history));
        assert!(is_enabled(ToolAction::Redo, &with_history));

        // Am Zoomanschlag ist Schluss.
        let zoomed_in = ToolContext {
            can_zoom_in: false,
            ..loaded
        };
        assert!(!is_enabled(ToolAction::ZoomIn, &zoomed_in));
        assert!(is_enabled(ToolAction::ZoomOut, &zoomed_in));
    }
}
