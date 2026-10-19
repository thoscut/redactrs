//! Zwei Arbeitsansichten: Werkbank und Fokus.
//!
//! Beide zeigen dieselben Bedienelemente und rufen dieselben Funktionen —
//! die Knöpfe kommen in beiden aus [`crate::toolbar`], die Trefferliste aus
//! [`crate::sidebar`], die Miniaturen aus [`crate::thumbnails`]. Verschieden
//! ist nur, **wo** sie stehen:
//!
//! * **Werkbank** — feste Leisten: oben die Dokument-Aktionen, links die
//!   Miniaturen, rechts der Inspektor mit Treffern und Auswahl, über der Seite
//!   die Ansichtsleiste. Alles auf einen Blick, für lange Prüfsitzungen.
//! * **Fokus** — die Seite füllt das Fenster; Datei, Treffer, Miniaturen und
//!   Werkzeuge schweben als Karten darüber. Für den schnellen Durchgang und
//!   kleine Bildschirme.
//!
//! Die Statuszeile bleibt in beiden Ansichten eine feste Leiste: dort stehen
//! Warnungen zum Export, und die dürfen nicht hinter einer Karte verschwinden.

/// Die Arbeitsansicht.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Layout {
    #[default]
    Workbench,
    Focus,
}

/// Beide Ansichten — für Tests.
pub const LAYOUTS: [Layout; 2] = [Layout::Workbench, Layout::Focus];

impl Layout {
    /// Die Ansicht zum Namen aus der Einstellungsdatei (`werkbank`, `fokus`).
    ///
    /// Unbekannte Namen lehnt schon `Settings::from_yaml` ab; hier bleibt nur
    /// der Rückfall auf die Werkbank.
    pub fn from_name(name: &str) -> Self {
        match name {
            "fokus" => Layout::Focus,
            _ => Layout::Workbench,
        }
    }

    /// Die jeweils andere Ansicht.
    pub fn toggled(self) -> Self {
        match self {
            Layout::Workbench => Layout::Focus,
            Layout::Focus => Layout::Workbench,
        }
    }

    /// Anzeigename.
    pub fn label(self) -> &'static str {
        match self {
            Layout::Workbench => "Werkbank",
            Layout::Focus => "Fokus",
        }
    }

    /// Beschriftung des Umschalters: wohin der Klick führt — wie beim Thema.
    pub fn switch_text(self) -> &'static str {
        self.toggled().label()
    }

    /// Symbol des Umschalters.
    pub fn switch_icon(self) -> &'static str {
        match self.toggled() {
            Layout::Workbench => "☰",
            Layout::Focus => "⛶",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_from_the_settings_file_picks_the_layout() {
        assert_eq!(Layout::from_name("werkbank"), Layout::Workbench);
        assert_eq!(Layout::from_name("fokus"), Layout::Focus);
        // Jeder Name, den die Einstellungsdatei erlaubt, kommt an.
        for name in redact_pipeline::settings::LAYOUTS {
            let layout = Layout::from_name(name);
            assert_eq!(layout.label().to_lowercase(), name);
        }
    }

    #[test]
    fn toggling_twice_returns_to_the_start() {
        for layout in LAYOUTS {
            assert_ne!(layout.toggled(), layout);
            assert_eq!(layout.toggled().toggled(), layout);
            assert_eq!(layout.switch_text(), layout.toggled().label());
        }
    }

    /// Die Symbole stehen allein auf den Knöpfen — fehlte eines in der
    /// Schrift, zeigte der Knopf ein leeres Kästchen.
    #[test]
    fn every_switch_icon_is_in_the_font() {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        let font = egui::FontId::proportional(14.0);
        let icons = LAYOUTS
            .iter()
            .map(|l| l.switch_icon())
            .chain(crate::theme::THEMES.iter().map(|t| t.switch_icon()));
        for icon in icons {
            assert!(
                ctx.fonts(|fonts| fonts.has_glyphs(&font, icon)),
                "„{icon}“ fehlt in der Schrift"
            );
        }
    }
}
