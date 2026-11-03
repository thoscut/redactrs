//! Wem ein Tastendruck gehört: dem Textfeld oder der Oberfläche.
//!
//! Die Zusage lautet „liegt der Fokus in einem **Textfeld**, gehören alle
//! Tasten dorthin“ (siehe [`crate::key_commands`]). Gefragt wurde dafür
//! `egui::Memory::focused().is_some()` — und das beantwortet eine andere Frage:
//! **irgendein** Widget hat den Fokus.
//!
//! Der Unterschied ist keine Feinheit. `egui::Sense::click()` ist fokussierbar,
//! also landet der Fokus nach einem einzigen Druck auf die Tabulatortaste auf
//! einem Knopf der Symbolleiste. Von da an galt jede Taste als „gehört dem
//! Textfeld“: Entf, die Pfeiltasten und Strg+O/S/Z/Y waren tot, ohne dass ein
//! Textfeld im Spiel gewesen wäre. Erholung nur durch Klicken oder Escape —
//! Tastaturbedienung war damit praktisch nicht durchführbar.
//!
//! Deshalb tragen die drei Textfelder dieser Oberfläche feste Kennungen, und
//! gefragt wird nach **ihnen** — und zusätzlich danach, ob am fokussierten
//! Widget ein `egui::TextEdit` hängt (das Zahlenfeld des Zoomreglers hat keine
//! eigene Kennung, siehe [`in_text_field`]). Ein Widget, das kein Textfeld
//! ist, bekommt hier nichts; nimmt es die Pfeiltasten selbst (die Schiene des
//! Reglers), meldet es das mit [`claim_arrows`].
//!
//! ## Escape ist ein Sonderfall
//!
//! egui räumt den Fokus bei Escape in `Focus::begin_pass` ab — also **bevor**
//! die Anwendung am Bildende die Tasten liest. Wer dann nur den *jetzigen*
//! Fokus prüft, sieht „kein Textfeld“ und lässt Escape zusätzlich als
//! Oberflächenbefehl durchgehen: ein Escape im Feld „Ersetzen“ hob zusätzlich
//! die Auswahl auf. [`TextFieldFocus`] merkt sich deshalb den Stand vom Ende
//! des vorigen Bildes; für Escape zählt der.

use egui::{Context, Id};

/// Kennung des Feldes „Namenszusatz“ (Seitenleiste).
pub const OUTPUT_SUFFIX: &str = "redact_feld_namenszusatz";
/// Kennung des Feldes „Ersatztext“ (Seitenleiste, Aktion „Ersetzen“).
pub const REPLACEMENT: &str = "redact_feld_ersatztext";
/// Kennung des Passwortfeldes.
pub const PASSWORD: &str = "redact_feld_passwort";

/// Alle Textfelder der Oberfläche.
pub const TEXT_FIELDS: [&str; 3] = [OUTPUT_SUFFIX, REPLACEMENT, PASSWORD];

/// Die egui-Kennung eines Feldes.
pub fn id(name: &str) -> Id {
    Id::new(name)
}

/// Gehört diese Kennung einem Textfeld dieser Oberfläche?
pub fn is_text_field(focused: Option<Id>) -> bool {
    focused.is_some_and(|focused| TEXT_FIELDS.iter().any(|name| id(name) == focused))
}

/// Liegt der Fokus **jetzt** in einem Textfeld?
///
/// Die Namensliste [`TEXT_FIELDS`] allein reichte nicht: das eintippbare
/// Zahlenfeld des Zoomreglers (`egui::Slider` mit `DragValue`) ist ebenfalls
/// ein `egui::TextEdit`, trägt aber eine Kennung, die egui selbst vergibt.
/// Eine Rücktaste beim Korrigieren der Zoomzahl löschte deshalb das
/// ausgewählte Rechteck, und die Pfeiltasten verschoben es. Gefragt wird
/// darum zusätzlich, ob am fokussierten Widget ein `TextEditState` hängt —
/// den hat jedes Textfeld, und ein Knopf nie (sonst wäre die Tastatur nach
/// einem Tab wieder tot, siehe oben).
pub fn in_text_field(ctx: &Context) -> bool {
    let focused = ctx.memory(|m| m.focused());
    is_text_field(focused)
        || focused.is_some_and(|id| egui::text_edit::TextEditState::load(ctx, id).is_some())
}

/// Schlüssel, unter dem [`claim_arrows`] sein Bild ablegt.
const ARROW_CLAIM: &str = "redact_pfeile_vergeben";

/// Meldet: dieses Widget hat den Fokus und nimmt die **Pfeiltasten** selbst.
///
/// Für die Schiene des Zoomreglers. egui verstellt einen fokussierten Regler
/// mit Pfeil links/rechts; ohne diese Meldung schob derselbe Anschlag
/// zusätzlich das ausgewählte Rechteck oder blätterte. Gilt nur für das Bild,
/// in dem sie abgegeben wird — aufzurufen **vor** dem Lesen der Tasten.
pub fn claim_arrows(response: &egui::Response) {
    if response.has_focus() {
        let ctx = &response.ctx;
        let pass = ctx.cumulative_pass_nr();
        ctx.data_mut(|d| d.insert_temp(Id::new(ARROW_CLAIM), pass));
    }
}

/// Hat in **diesem** Bild ein Widget die Pfeiltasten beansprucht?
pub fn arrows_claimed(ctx: &Context) -> bool {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<u64>(Id::new(ARROW_CLAIM))) == Some(pass)
}

/// Der Fokusstand über die Bildgrenze hinweg.
///
/// Gemerkt wird der Stand am **Ende** eines Bildes — das ist derselbe Stand,
/// mit dem das nächste Bild beginnt, bevor egui ihn in `Focus::begin_pass`
/// womöglich abräumt.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextFieldFocus {
    /// Lag der Fokus am Ende des vorigen Bildes in einem Textfeld?
    before: bool,
}

impl TextFieldFocus {
    /// Gehören die Tasten dieses Bildes einem Textfeld?
    ///
    /// `escape` ist der einzige Grund für den Blick zurück: egui nimmt dem Feld
    /// den Fokus beim Escape schon zu Beginn des Bildes weg, der Tastendruck
    /// selbst gehört aber noch dorthin.
    pub fn owns_keys(&self, now: bool, escape: bool) -> bool {
        now || (escape && self.before)
    }

    /// Am Bildende merken, wo der Fokus liegt.
    pub fn remember(&mut self, now: bool) {
        self.before = now;
    }

    /// Nur für Tests und Erklärungen.
    pub fn was_in_text_field(&self) -> bool {
        self.before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ein Knopf ist kein Textfeld — genau daran ist die alte Abfrage
    /// gescheitert.
    #[test]
    fn only_the_named_fields_count_as_text_fields() {
        for name in TEXT_FIELDS {
            assert!(is_text_field(Some(id(name))), "{name}");
        }
        assert!(!is_text_field(None));
        assert!(!is_text_field(Some(Id::new("irgendein_knopf"))));
        // Auch ein Knopf der Symbolleiste, wie egui ihn benennt.
        assert!(!is_text_field(Some(Id::new("🗁 Öffnen"))));
    }

    /// Escape gehört dem Feld, aus dem es gerade herausführt — aber nur diesem
    /// einen Bild und nur dieser einen Taste.
    #[test]
    fn escape_still_belongs_to_the_field_it_just_left() {
        let mut focus = TextFieldFocus::default();
        assert!(
            !focus.owns_keys(false, true),
            "ohne Feld gehört nichts dorthin"
        );

        focus.remember(true);
        assert!(focus.was_in_text_field());
        assert!(
            focus.owns_keys(false, true),
            "Escape gehört dem Feld, dem egui den Fokus schon genommen hat"
        );
        assert!(
            !focus.owns_keys(false, false),
            "jede andere Taste richtet sich nach dem Jetzt"
        );

        // Nach dem Escape ist der Fokus weg — das nächste Bild merkt sich das.
        focus.remember(false);
        assert!(
            !focus.owns_keys(false, true),
            "und dann gilt Escape wieder der Fläche"
        );
    }
}
