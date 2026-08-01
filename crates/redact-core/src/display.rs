//! Fremde Texte auf ein Terminal ausgeben, ohne es zu steuern.
//!
//! ## Warum das nötig ist
//!
//! Ein Dateiname ist Eingabe wie jede andere: unter Unix darf er jedes Byte
//! außer `/` und `NUL` enthalten, also auch `ESC [ 3 1 m` oder `ESC [ 2 K`.
//! Wird er unverändert nach stdout geschrieben, führt das Terminal die Folge
//! aus — es färbt, es löscht Zeilen, es setzt den Cursor. Damit lässt sich die
//! Zusammenfassung eines Stapellaufs optisch fälschen: eine Datei namens
//! `auszug<ESC>[2K<ESC>[A.pdf` löscht die Zeile über sich, und aus
//! „1 fehlgeschlagen" wird auf dem Bildschirm „0 fehlgeschlagen".
//!
//! Wer eine Datei schwärzt, prüft das Ergebnis an dieser Zusammenfassung. Sie
//! muss deshalb genau das zeigen, was geschehen ist — und nicht das, was der
//! Absender der Datei zeigen wollte.
//!
//! ## Was ersetzt wird
//!
//! Alles, was ein Terminal (oder ein Textbetrachter) als Steuerung liest:
//!
//! * C0-Steuerzeichen `U+0000`–`U+001F`, **einschließlich** Zeilenumbruch und
//!   Tabulator — ein Umbruch in einem Dateinamen erzeugt sonst eine ganze
//!   erfundene Zeile.
//! * `DEL` (`U+007F`) und die C1-Steuerzeichen `U+0080`–`U+009F`.
//! * Die Richtungsumschalter `U+200E`, `U+200F`, `U+202A`–`U+202E` und
//!   `U+2066`–`U+2069`. Sie steuern kein Terminal, kehren aber die Leserichtung
//!   um: `rechnung<U+202E>fdp.exe` liest sich als `rechnungexe.pdf`.
//!
//! Ersetzt wird durch die sichtbare Form `\u{1b}`. Umgekehrt eindeutig ist das
//! nicht — ein Dateiname, in dem die sieben Zeichen `\u{1b}` wirklich stehen,
//! sieht danach genauso aus. Das ist Absicht: das Ziel ist eine Ausgabe, die
//! nichts steuert, nicht eine, aus der sich der Name zurückrechnen lässt. Ein
//! zusätzlich verdoppelter Rückstrich machte jeden Windows-Pfad unlesbar und
//! wäre für den Zweck kein Gewinn.
//!
//! ## Wo es *nicht* gebraucht wird
//!
//! In JSON (`--json`, Audit-Log, Review-Datei) nicht: `serde_json` schreibt
//! Steuerzeichen selbst als `\u001b`. Nur die Ausgabe für Menschen geht hier
//! durch.

use std::path::Path;

/// Gibt `text` so zurück, dass er ein Terminal nicht steuern kann.
pub fn safe_text(text: &str) -> String {
    // Häufigster Fall zuerst: es ist nichts zu tun, und dann soll auch nichts
    // kopiert werden.
    if !text.chars().any(needs_escape) {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() + 8);
    for ch in text.chars() {
        if needs_escape(ch) {
            out.push_str(&format!("\\u{{{:x}}}", ch as u32));
        } else {
            out.push(ch);
        }
    }
    out
}

/// Wie [`safe_text`], aber für einen Pfad.
///
/// Ein Pfad, der keine gültige UTF-8-Folge ist, wird dabei zusätzlich über
/// `to_string_lossy` verlustbehaftet umgesetzt — genau wie bisher an jeder
/// Ausgabestelle auch.
pub fn safe_path(path: &Path) -> String {
    safe_text(&path.to_string_lossy())
}

/// Steuert dieses Zeichen die Anzeige, statt etwas darzustellen?
fn needs_escape(ch: char) -> bool {
    matches!(ch,
        '\u{0}'..='\u{1f}'
        | '\u{7f}'..='\u{9f}'
        | '\u{200e}' | '\u{200f}'
        | '\u{202a}'..='\u{202e}'
        | '\u{2066}'..='\u{2069}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Gewöhnliche Namen bleiben Zeichen für Zeichen, wie sie sind — auch mit
    /// Umlauten, Leerzeichen und (unter Windows) Rückstrichen.
    #[test]
    fn ordinary_names_pass_through_unchanged() {
        for name in [
            "kontoauszug.pdf",
            "Auszug März 2026.pdf",
            "/daten/übersicht_geschwaerzt.pdf",
            "C:\\Users\\Anna\\auszug.pdf",
            "名前.pdf",
        ] {
            assert_eq!(safe_text(name), name);
        }
    }

    /// Die Folge aus dem Befund: `ESC [ 3 1 m` darf nicht mehr als solche
    /// herauskommen.
    #[test]
    fn an_escape_sequence_no_longer_reaches_the_terminal() {
        let out = safe_text("a\u{1b}[31mrot\u{1b}[2K.pdf");
        assert!(!out.contains('\u{1b}'), "{out}");
        assert_eq!(out, "a\\u{1b}[31mrot\\u{1b}[2K.pdf");
    }

    /// Ein Zeilenumbruch im Namen erfände sonst eine ganze Zeile in der
    /// Zusammenfassung.
    #[test]
    fn a_line_break_in_a_name_cannot_forge_a_line() {
        let out = safe_text("auszug.pdf\n3 Datei(en): 3 verarbeitet, 0 fehlgeschlagen.");
        assert!(!out.contains('\n'), "{out}");
    }

    /// Auch der Rückwärtsleser wird sichtbar gemacht.
    #[test]
    fn the_direction_override_is_made_visible() {
        let out = safe_text("rechnung\u{202e}fdp.exe");
        assert!(!out.contains('\u{202e}'), "{out}");
        assert!(out.contains("\\u{202e}"), "{out}");
    }

    #[test]
    fn paths_go_the_same_way() {
        let path = PathBuf::from("a\u{1b}[31m.pdf");
        assert_eq!(safe_path(&path), "a\\u{1b}[31m.pdf");
    }

    /// Gegenprobe: die Prüfung schlägt nicht auf harmlose Zeichen an, die
    /// zufällig hoch liegen (etwa das Geviert oder ein geschütztes Leerzeichen).
    #[test]
    fn harmless_high_characters_are_left_alone() {
        for name in ["a\u{a0}b.pdf", "—.pdf", "\u{200b}.pdf"] {
            assert_eq!(safe_text(name), name);
        }
    }
}
