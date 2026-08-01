//! Helles und dunkles Thema.
//!
//! Zwei Zeilen egui-Vorgabe und ein Umschalter — mehr braucht es nicht. Der
//! einzige Punkt, an dem hier wirklich etwas zu entscheiden war: die vier
//! Trefferfarben ([`crate::state::RegionColor`]) sind in **beiden** Themen
//! dieselben. Zwei Farbsätze zu pflegen hieße, den Kontrast zweimal
//! nachzurechnen und zweimal falsch machen zu können; stattdessen sind die
//! Farben so gewählt, dass sie über hellem **und** dunklem Grund die 3:1 aus
//! WCAG 1.4.11 erreichen. Geprüft wird das gegen genau die Flächen, auf denen
//! sie wirklich liegen — siehe [`Theme::backgrounds`] und den Test
//! `region_colours_reach_the_graphic_contrast_minimum` in [`crate::state`].

/// Das Papier ist in beiden Themen weiß: eine PDF-Seite wird nicht eingefärbt,
/// sonst stimmte die Vorschau nicht mehr mit dem Ausdruck überein.
pub const PAPER: (u8, u8, u8) = (255, 255, 255);

/// Helles oder dunkles Thema.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Light,
    Dark,
}

impl Theme {
    /// Das Thema zum Namen aus der Einstellungsdatei (`hell`, `dunkel`).
    ///
    /// Unbekannte Namen kommen hier nicht an — `Settings::from_yaml` lehnt sie
    /// beim Lesen der Datei ab, mit einer Meldung, die die erlaubten Werte
    /// nennt. Hier bleibt deshalb nur der Rückfall auf das helle Thema.
    pub fn from_name(name: &str) -> Self {
        match name {
            "dunkel" => Theme::Dark,
            _ => Theme::Light,
        }
    }

    /// Das jeweils andere Thema.
    pub fn toggled(self) -> Self {
        match self {
            Theme::Light => Theme::Dark,
            Theme::Dark => Theme::Light,
        }
    }

    /// Die egui-Vorgabe zum Thema.
    pub fn visuals(self) -> egui::Visuals {
        match self {
            Theme::Light => egui::Visuals::light(),
            Theme::Dark => egui::Visuals::dark(),
        }
    }

    /// Name des Themas.
    pub fn label(self) -> &'static str {
        match self {
            Theme::Light => "Hell",
            Theme::Dark => "Dunkel",
        }
    }

    /// Beschriftung des Umschalters: **wohin** der Klick führt, nicht wo man
    /// gerade ist. „Dunkel“ auf dem Knopf heißt „schaltet auf dunkel“.
    pub fn switch_text(self) -> &'static str {
        self.toggled().label()
    }

    /// Symbol des Umschalters — Sonne für hell, Mond für dunkel.
    pub fn switch_icon(self) -> &'static str {
        match self.toggled() {
            Theme::Light => "☀",
            Theme::Dark => "🌙",
        }
    }

    /// Die Flächen, auf denen Trefferfarben in diesem Thema liegen können.
    ///
    /// Aus [`Theme::visuals`] entnommen statt von Hand notiert — sonst prüfte
    /// der Kontrasttest Farben, die egui gar nicht benutzt. `faint_bg_color`
    /// fehlt bewusst: das ist ein additiver, halbdurchsichtiger Wert und keine
    /// Fläche, gegen die sich ein Kontrast rechnen ließe.
    pub fn backgrounds(self) -> Vec<(u8, u8, u8)> {
        let visuals = self.visuals();
        let mut all = vec![PAPER];
        for color in [
            visuals.panel_fill,
            visuals.window_fill,
            visuals.extreme_bg_color,
            visuals.widgets.noninteractive.bg_fill,
        ] {
            let rgb = (color.r(), color.g(), color.b());
            if !all.contains(&rgb) {
                all.push(rgb);
            }
        }
        all
    }
}

/// Beide Themen — für Tests und für die Anzeige.
pub const THEMES: [Theme; 2] = [Theme::Light, Theme::Dark];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggling_twice_returns_to_the_start() {
        for theme in THEMES {
            assert_ne!(theme.toggled(), theme);
            assert_eq!(theme.toggled().toggled(), theme);
        }
    }

    /// Der Name aus der Einstellungsdatei kommt am richtigen Thema an.
    #[test]
    fn the_name_from_the_settings_file_picks_the_theme() {
        assert_eq!(Theme::from_name("hell"), Theme::Light);
        assert_eq!(Theme::from_name("dunkel"), Theme::Dark);
        // Beide erlaubten Werte der Einstellungsdatei sind hier bekannt —
        // sonst hieße „dunkel“ in der Datei am Ende doch „hell“ im Fenster.
        for name in redact_pipeline::settings::THEMES {
            assert_eq!(Theme::from_name(name).label().to_lowercase(), name);
        }
    }

    /// Der Umschalter muss sagen, wohin er führt — steht dort der aktuelle
    /// Zustand, klickt man ihn in die falsche Richtung.
    #[test]
    fn the_switch_names_the_other_theme() {
        assert_eq!(Theme::Light.label(), "Hell");
        assert_eq!(Theme::Light.switch_text(), "Dunkel");
        assert_eq!(Theme::Dark.switch_text(), "Hell");
        assert_ne!(Theme::Light.switch_icon(), Theme::Dark.switch_icon());
    }

    /// Ein Symbol, das die Schrift nicht kennt, erscheint als leeres Kästchen
    /// — beim Themenschalter wäre das besonders ärgerlich, weil er sonst
    /// nichts als das Symbol und ein Wort trägt.
    #[test]
    fn the_switch_icons_have_glyphs() {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        let font = egui::FontId::proportional(14.0);
        ctx.fonts(|fonts| {
            for theme in THEMES {
                assert!(
                    fonts.has_glyphs(&font, theme.switch_icon()),
                    "{theme:?}: Symbol {:?} fehlt in der Standardschrift",
                    theme.switch_icon()
                );
            }
        });
    }

    /// Hell und dunkel müssen sich auch wirklich unterscheiden — und das
    /// dunkle Thema muss dunkler sein.
    #[test]
    fn the_two_themes_have_different_backgrounds() {
        let light = Theme::Light.visuals().panel_fill;
        let dark = Theme::Dark.visuals().panel_fill;
        assert_ne!(light, dark);
        let sum = |c: egui::Color32| c.r() as u32 + c.g() as u32 + c.b() as u32;
        assert!(sum(dark) < sum(light), "„Dunkel“ muss dunkler sein");
    }

    /// Auf dem Papier liegt in beiden Themen dieselbe weiße Fläche — die Seite
    /// selbst wird nicht eingefärbt.
    #[test]
    fn every_theme_lists_the_white_sheet_and_its_own_panel() {
        for theme in THEMES {
            let backgrounds = theme.backgrounds();
            assert!(backgrounds.contains(&PAPER), "{theme:?}: Papier fehlt");
            let panel = theme.visuals().panel_fill;
            assert!(
                backgrounds.contains(&(panel.r(), panel.g(), panel.b())),
                "{theme:?}: Bereichshintergrund fehlt"
            );
            assert!(backgrounds.len() >= 2);
        }
    }
}
