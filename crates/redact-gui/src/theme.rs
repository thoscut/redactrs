//! Helles und dunkles Thema.
//!
//! Ein eigener Farbsatz auf Schiefer-Grundlage statt der nackten
//! egui-Vorgabe, eine Akzentfarbe (Violett) für Auswahl, Fokus und den einen
//! Hauptknopf, dazu Abstände, Rundungen und Schriftgrößen, die die Leisten
//! ruhiger und die Trefferliste lesbarer machen. Alles davon steht in
//! [`Theme::visuals`] und [`Theme::apply`] — sonst nirgends.
//!
//! Das Violett ist mit Absicht keine der vier Trefferfarben
//! ([`crate::state::RegionColor`]: Blau, Grün, Rot, Orange): eine markierte
//! Zeile darf nicht aussehen wie eine Trefferart.
//!
//! Der einzige Punkt, an dem hier wirklich etwas zu entscheiden war: die vier
//! Trefferfarben sind in **beiden** Themen dieselben. Zwei Farbsätze zu
//! pflegen hieße, den Kontrast zweimal nachzurechnen und zweimal falsch machen
//! zu können; stattdessen sind die Flächen so gewählt, dass jede Trefferfarbe
//! über ihnen die 3:1 aus WCAG 1.4.11 erreicht. Geprüft wird das gegen genau
//! die Flächen, auf denen sie wirklich liegen — siehe [`Theme::backgrounds`]
//! und den Test `region_colours_reach_the_graphic_contrast_minimum` in
//! [`crate::state`].

use egui::{Color32, Rounding, Shadow, Stroke, Vec2};

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

    /// Das Thema, zu dem diese egui-Werte gehören — für Zeichenfunktionen, die
    /// nur ein `Ui` in der Hand haben.
    pub fn of(visuals: &egui::Visuals) -> Self {
        if visuals.dark_mode {
            Theme::Dark
        } else {
            Theme::Light
        }
    }

    /// Das jeweils andere Thema.
    pub fn toggled(self) -> Self {
        match self {
            Theme::Light => Theme::Dark,
            Theme::Dark => Theme::Light,
        }
    }

    /// Farben, Rundungen und Schatten des Themas.
    pub fn visuals(self) -> egui::Visuals {
        let palette = self.palette();
        let mut visuals = match self {
            Theme::Light => egui::Visuals::light(),
            Theme::Dark => egui::Visuals::dark(),
        };

        visuals.panel_fill = palette.panel;
        visuals.window_fill = palette.window;
        visuals.extreme_bg_color = palette.field;
        visuals.faint_bg_color = palette.faint;
        visuals.code_bg_color = palette.faint;
        visuals.hyperlink_color = palette.accent;
        visuals.warn_fg_color = palette.warn;
        visuals.error_fg_color = palette.error;
        visuals.selection.bg_fill = palette.selection;
        // Auch der Rand eines Textfeldes mit Fokus — deshalb 2 pt.
        visuals.selection.stroke = Stroke::new(2.0, palette.selection_text);

        visuals.window_rounding = Rounding::same(WINDOW_ROUNDING);
        visuals.menu_rounding = Rounding::same(WIDGET_ROUNDING);
        visuals.window_stroke = Stroke::new(1.0, palette.border);
        visuals.window_shadow = Shadow {
            offset: Vec2::new(0.0, 8.0),
            blur: 28.0,
            spread: 0.0,
            color: palette.shadow,
        };
        visuals.popup_shadow = Shadow {
            offset: Vec2::new(0.0, 4.0),
            blur: 12.0,
            spread: 0.0,
            color: palette.shadow,
        };
        visuals.slider_trailing_fill = true;
        visuals.striped = false;
        visuals.indent_has_left_vline = false;

        let widgets = &mut visuals.widgets;
        let rounding = Rounding::same(WIDGET_ROUNDING);

        widgets.noninteractive.bg_fill = palette.panel;
        widgets.noninteractive.weak_bg_fill = palette.panel;
        widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.divider);
        widgets.noninteractive.fg_stroke = Stroke::new(1.0, palette.text);
        widgets.noninteractive.rounding = rounding;

        // `bg_fill` tragen Häkchenfelder und die Schiene des Zoomreglers,
        // `weak_bg_fill` die Knöpfe: die Schiene muss sich vom Bereich abheben,
        // ein Knopf soll hell und ruhig bleiben.
        widgets.inactive.bg_fill = palette.rail;
        widgets.inactive.weak_bg_fill = palette.control;
        // Rahmen von Häkchenfeldern, Textfeldern und Auswahllisten: 3:1 gegen
        // jede Fläche (WCAG 1.4.11) — sonst ist ein leeres Feld nicht als Feld
        // zu erkennen. Die Knöpfe der Leisten zeichnen ihren Rahmen erst beim
        // Darüberfahren ([`crate::app::tool_buttons`]).
        widgets.inactive.bg_stroke = Stroke::new(1.0, palette.control_border);
        widgets.inactive.fg_stroke = Stroke::new(1.0, palette.text_strong);
        widgets.inactive.rounding = rounding;

        widgets.hovered.bg_fill = palette.control_hover;
        widgets.hovered.weak_bg_fill = palette.control_hover;
        widgets.hovered.bg_stroke = Stroke::new(1.0, palette.border_strong);
        widgets.hovered.fg_stroke = Stroke::new(1.5, palette.text_strong);
        widgets.hovered.rounding = rounding;
        widgets.hovered.expansion = 0.0;

        widgets.active.bg_fill = palette.control_active;
        widgets.active.weak_bg_fill = palette.control_active;
        // `active` ist auch der Zustand „hat den Tastaturfokus“: ein 2-pt-Rand
        // in der Akzentfarbe, damit man sieht, wo der Tabulator steht.
        widgets.active.bg_stroke = Stroke::new(2.0, palette.accent);
        widgets.active.fg_stroke = Stroke::new(2.0, palette.text_strong);
        widgets.active.rounding = rounding;
        widgets.active.expansion = 0.0;

        widgets.open.bg_fill = palette.control_hover;
        widgets.open.weak_bg_fill = palette.control_hover;
        widgets.open.bg_stroke = Stroke::new(1.0, palette.border_strong);
        widgets.open.fg_stroke = Stroke::new(1.0, palette.text_strong);
        widgets.open.rounding = rounding;

        visuals
    }

    /// Setzt das ganze Thema am Kontext: [`Theme::visuals`] samt Abständen
    /// und Schriftgrößen.
    ///
    /// Abstände und Schriftgrößen hängen nicht vom Thema ab, gehören aber zum
    /// selben Aufruf — `set_visuals` allein ließe sie auf der egui-Vorgabe,
    /// und die ist für eine Liste mit 150 Treffern zu gedrängt.
    pub fn apply(self, ctx: &egui::Context) {
        ctx.style_mut(|style| {
            style.visuals = self.visuals();

            let spacing = &mut style.spacing;
            spacing.item_spacing = Vec2::new(8.0, 6.0);
            spacing.button_padding = Vec2::new(8.0, 4.0);
            spacing.interact_size.y = 24.0;
            spacing.icon_width = 16.0;
            spacing.icon_width_inner = 9.0;
            spacing.icon_spacing = 6.0;
            spacing.combo_width = 140.0;
            spacing.menu_margin = egui::Margin::same(6.0);
            spacing.window_margin = egui::Margin::same(16.0);

            use egui::{FontFamily, FontId, TextStyle};
            style.text_styles = [
                (
                    TextStyle::Small,
                    FontId::new(11.5, FontFamily::Proportional),
                ),
                (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
                (
                    TextStyle::Button,
                    FontId::new(13.5, FontFamily::Proportional),
                ),
                (
                    TextStyle::Heading,
                    FontId::new(17.0, FontFamily::Proportional),
                ),
                (
                    TextStyle::Monospace,
                    FontId::new(12.5, FontFamily::Monospace),
                ),
            ]
            .into();
        });
    }

    /// Hintergrund der Fläche, auf der die Seite liegt.
    ///
    /// Eine Spur dunkler als die Leisten, damit das weiße Blatt sich abhebt —
    /// und hell genug, dass eine Trefferfarbe auch dort, neben dem Blatt,
    /// noch 3:1 erreicht (Rechtecke dürfen über den Rand ragen).
    pub fn canvas(self) -> Color32 {
        self.palette().canvas
    }

    /// Zurückgenommene Schrift: Hinweise, Vorschläge, Nebensätze.
    ///
    /// Statt egui's `weak()`: das mischt die Schriftfarbe zur Hälfte mit dem
    /// Hintergrund und landet damit bei rund 2,5:1 — für Text verlangt
    /// WCAG 1.4.3 aber 4,5:1. Diese Farbe erreicht das auf jeder Fläche, auf
    /// der Schrift steht (Test `muted_text_reaches_the_text_contrast_minimum`).
    pub fn muted(self) -> Color32 {
        self.palette().muted
    }

    /// Die Akzentfarbe — Auswahl, Fokus, der Hauptknopf.
    pub fn accent(self) -> Color32 {
        self.palette().accent
    }

    /// Schrift auf der Akzentfarbe.
    pub fn on_accent(self) -> Color32 {
        self.palette().on_accent
    }

    /// Fläche der Trefferzahl auf einer Miniatur: neutral, damit Violett
    /// allein Auswahl und Fokus bedeutet.
    pub fn badge_fill(self) -> Color32 {
        self.palette().muted
    }

    /// Schrift auf [`Self::badge_fill`].
    pub fn badge_text(self) -> Color32 {
        self.palette().panel
    }

    fn palette(self) -> Palette {
        match self {
            Theme::Light => Palette {
                panel: Color32::from_rgb(246, 247, 249),
                window: Color32::from_rgb(255, 255, 255),
                field: Color32::from_rgb(255, 255, 255),
                faint: Color32::from_rgb(238, 240, 243),
                canvas: Color32::from_rgb(234, 236, 240),
                control: Color32::from_rgb(255, 255, 255),
                rail: Color32::from_rgb(226, 229, 234),
                control_hover: Color32::from_rgb(241, 242, 245),
                control_active: Color32::from_rgb(229, 231, 236),
                divider: Color32::from_rgb(226, 229, 234),
                border: Color32::from_rgb(211, 215, 222),
                control_border: Color32::from_rgb(128, 136, 150),
                border_strong: Color32::from_rgb(170, 177, 189),
                text: Color32::from_rgb(55, 65, 81),
                text_strong: Color32::from_rgb(17, 24, 39),
                muted: Color32::from_rgb(92, 100, 112),
                accent: Color32::from_rgb(109, 40, 217),
                on_accent: Color32::WHITE,
                selection: Color32::from_rgb(233, 225, 255),
                selection_text: Color32::from_rgb(76, 29, 149),
                warn: Color32::from_rgb(161, 98, 7),
                error: Color32::from_rgb(200, 30, 30),
                shadow: Color32::from_black_alpha(36),
            },
            Theme::Dark => Palette {
                panel: Color32::from_rgb(28, 30, 34),
                window: Color32::from_rgb(32, 34, 39),
                field: Color32::from_rgb(20, 21, 24),
                faint: Color32::from_rgb(36, 39, 44),
                canvas: Color32::from_rgb(16, 17, 20),
                control: Color32::from_rgb(40, 43, 49),
                rail: Color32::from_rgb(54, 58, 66),
                control_hover: Color32::from_rgb(50, 54, 61),
                control_active: Color32::from_rgb(60, 64, 72),
                divider: Color32::from_rgb(44, 47, 53),
                border: Color32::from_rgb(56, 60, 68),
                control_border: Color32::from_rgb(104, 110, 122),
                border_strong: Color32::from_rgb(86, 92, 104),
                text: Color32::from_rgb(200, 205, 213),
                text_strong: Color32::from_rgb(236, 238, 242),
                muted: Color32::from_rgb(140, 146, 156),
                accent: Color32::from_rgb(167, 139, 250),
                on_accent: Color32::from_rgb(24, 16, 48),
                selection: Color32::from_rgb(64, 42, 120),
                selection_text: Color32::from_rgb(233, 225, 255),
                warn: Color32::from_rgb(245, 180, 70),
                error: Color32::from_rgb(248, 113, 113),
                shadow: Color32::from_black_alpha(110),
            },
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
    /// Dazu gehört die Fläche um das Blatt ([`Theme::canvas`]): ein Rechteck,
    /// das über den Seitenrand ragt, liegt dort. Der Rest ist aus
    /// [`Theme::visuals`] entnommen statt von Hand notiert — sonst prüfte
    /// der Kontrasttest Farben, die egui gar nicht benutzt. `faint_bg_color`
    /// fehlt bewusst: das ist ein additiver, halbdurchsichtiger Wert und keine
    /// Fläche, gegen die sich ein Kontrast rechnen ließe.
    pub fn backgrounds(self) -> Vec<(u8, u8, u8)> {
        let visuals = self.visuals();
        let mut all = vec![PAPER];
        for color in [
            self.canvas(),
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

/// Die zurückgenommene Schriftfarbe zum Thema dieses `Ui` — siehe
/// [`Theme::muted`].
pub fn muted(ui: &egui::Ui) -> Color32 {
    Theme::of(ui.visuals()).muted()
}

/// Eckenrundung von Knöpfen, Feldern und Menüs.
pub const WIDGET_ROUNDING: f32 = 6.0;
/// Eckenrundung von Fenstern (Passwortabfrage) und Karten.
pub const WINDOW_ROUNDING: f32 = 10.0;

/// Der Farbsatz eines Themas. Privat: nach außen gehen nur die egui-Werte
/// ([`Theme::visuals`]) und die drei Farben, die egui nicht kennt.
struct Palette {
    panel: Color32,
    window: Color32,
    field: Color32,
    faint: Color32,
    canvas: Color32,
    control: Color32,
    rail: Color32,
    control_hover: Color32,
    control_active: Color32,
    divider: Color32,
    border: Color32,
    control_border: Color32,
    border_strong: Color32,
    text: Color32,
    text_strong: Color32,
    muted: Color32,
    accent: Color32,
    on_accent: Color32,
    selection: Color32,
    selection_text: Color32,
    warn: Color32,
    error: Color32,
    shadow: Color32,
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

    /// Die Schrift auf dem Hauptknopf muss lesbar sein: 4,5:1 nach WCAG 1.4.3,
    /// in beiden Themen.
    #[test]
    fn the_primary_button_text_reads_on_the_accent() {
        fn luminance(c: Color32) -> f64 {
            let channel = |v: u8| {
                let v = v as f64 / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
        }
        for theme in THEMES {
            let (a, b) = (luminance(theme.accent()), luminance(theme.on_accent()));
            let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
            assert!(ratio >= 4.5, "{theme:?}: nur {ratio:.2}:1");
        }
    }

    /// Das Blatt muss sich von seiner Umgebung abheben — die Fläche darum ist
    /// eine andere als die der Leisten, und sie ist nicht weiß.
    #[test]
    fn the_canvas_sets_the_sheet_apart() {
        for theme in THEMES {
            assert_ne!(theme.canvas(), theme.visuals().panel_fill, "{theme:?}");
            let (r, g, b) = PAPER;
            assert_ne!(theme.canvas(), Color32::from_rgb(r, g, b), "{theme:?}");
            assert_eq!(Theme::of(&theme.visuals()), theme);
        }
    }

    /// `apply` setzt mehr als die Farben — sonst blieben Abstände und
    /// Schriftgrößen auf der gedrängten egui-Vorgabe.
    #[test]
    fn applying_the_theme_sets_colours_and_spacing() {
        for theme in THEMES {
            let ctx = egui::Context::default();
            theme.apply(&ctx);
            let style = ctx.style();
            assert_eq!(style.visuals.panel_fill, theme.visuals().panel_fill);
            assert!(style.spacing.interact_size.y >= 24.0);
            let small = &style.text_styles[&egui::TextStyle::Small];
            assert!(small.size >= 11.0, "{theme:?}: kleine Schrift zu klein");
        }
    }
    fn contrast(a: Color32, b: Color32) -> f64 {
        fn luminance(c: Color32) -> f64 {
            let channel = |v: u8| {
                let v = v as f64 / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
        }
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    /// Flächen, auf denen Schrift steht.
    fn text_surfaces(theme: Theme) -> Vec<Color32> {
        let v = theme.visuals();
        vec![
            v.panel_fill,
            v.window_fill,
            v.extreme_bg_color,
            v.faint_bg_color,
            theme.canvas(),
        ]
    }

    /// Zurückgenommene Schrift erreicht trotzdem 4,5:1 (WCAG 1.4.3) — auf jeder
    /// Fläche, auf der Schrift steht. egui's eigenes `weak()` schafft das
    /// nicht; daran hängt, dass Hinweise und Dateinamen lesbar bleiben.
    #[test]
    fn muted_text_reaches_the_text_contrast_minimum() {
        for theme in THEMES {
            for surface in text_surfaces(theme) {
                let ratio = contrast(theme.muted(), surface);
                assert!(
                    ratio >= 4.5,
                    "{theme:?}: gedämpft gegen {surface:?} nur {ratio:.2}:1"
                );
                let text = theme.visuals().text_color();
                let ratio = contrast(text, surface);
                assert!(
                    ratio >= 7.0,
                    "{theme:?}: Schrift gegen {surface:?} nur {ratio:.2}:1"
                );
            }
            // Gegenprobe: egui's `weak()` fiele durch — sonst bräuchte es
            // diese Farbe nicht.
            let v = theme.visuals();
            assert!(
                contrast(v.weak_text_color(), v.panel_fill) < 4.5,
                "{theme:?}"
            );
        }
    }

    /// Der Rand eines Bedienelements ist als Grenze erkennbar: 3:1 nach
    /// WCAG 1.4.11 gegen den Bereich, das Feld selbst und das Fenster.
    #[test]
    fn control_borders_reach_the_graphic_contrast_minimum() {
        for theme in THEMES {
            let v = theme.visuals();
            let border = v.widgets.inactive.bg_stroke.color;
            for surface in [v.panel_fill, v.window_fill, v.extreme_bg_color] {
                let ratio = contrast(border, surface);
                assert!(
                    ratio >= 3.0,
                    "{theme:?}: Rand gegen {surface:?} nur {ratio:.2}:1"
                );
            }
            // Fokus: deutlich breiter als ein gewöhnlicher Rand, in der
            // Akzentfarbe, und ebenfalls 3:1.
            assert!(v.widgets.active.bg_stroke.width >= 2.0);
            assert!(contrast(v.widgets.active.bg_stroke.color, v.panel_fill) >= 3.0);
            assert!(contrast(v.selection.stroke.color, v.extreme_bg_color) >= 3.0);
        }
    }

    /// Die Trefferzahl auf der Miniatur ist Text: 4,5:1 auf ihrer Fläche.
    #[test]
    fn the_thumbnail_badge_is_legible() {
        for theme in THEMES {
            let ratio = contrast(theme.badge_text(), theme.badge_fill());
            assert!(ratio >= 4.5, "{theme:?}: Plakette nur {ratio:.2}:1");
            assert_ne!(theme.badge_fill(), theme.accent());
        }
    }
}
