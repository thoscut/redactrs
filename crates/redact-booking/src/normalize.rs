//! Normalisierung von Text für den Vergleich mit Buchungslisten-Einträgen.
//!
//! Buchungslisten enthalten Werte so, wie ein Mensch sie schreibt
//! (`DE89 3704 0044 0532 0130 00`), im PDF steht derselbe Wert aber oft mit
//! einer anderen *Menge* an Leerraum — `DE89  3704 0044 …`, gesperrt gesetzt,
//! oder ganz ohne Leerzeichen. Deshalb wird sowohl der Suchtext (Heuhaufen) als
//! auch das Muster (Nadel) vor dem Vergleich normalisiert:
//!
//! * alle Zeichen werden klein geschrieben (Vergleich ohne Groß-/Kleinschreibung),
//! * jede Folge von Leerraum wird zu genau einem Leerzeichen zusammengefasst.
//!
//! ## Was das *nicht* leistet: Zeilenumbrüche
//!
//! Die Zusammenfassung von Leerraum behandelt zwar auch `\n` wie jedes andere
//! Leerraum-Zeichen — das nützt hier aber nichts, denn **im Eingabetext kommt
//! nie eines vor**. [`crate::matcher::BookingMatcher::find_matches`] bekommt
//! [`redact_core::TextRun`]s, und der Extraktor
//! (`redact_pdf::extract::PdfExtractor::build_lines`) gruppiert Glyphen entlang
//! der Grundlinie zu je einem Run pro *Zeile*; als Trennzeichen setzt
//! `assemble_line` ausschließlich `' '`. Eine über zwei Zeilen verteilte IBAN
//! ist also nicht ein Run mit `\n`, sondern **zwei getrennte Runs** — und der
//! Abgleich läuft Run für Run. Sie wird deshalb nicht gefunden.
//!
//! Festgehalten von `iban_split_across_two_extracted_lines_is_not_found` in
//! `tests/matcher_tests.rs`; nachgemessen am fertigen Programm liefert eine
//! Seite mit `IBAN: DE89 3704` / `0044 0532 0130 00` in zwei Zeilen
//! `Textzeilen: 2, Treffer gesamt: 0` — für die Buchungsliste wie für das
//! Muster `iban_de`.
//!
//! Damit ein Treffer im normalisierten Text wieder auf die exakte Bounding-Box
//! im PDF abgebildet werden kann, merkt sich [`Normalized`] für **jedes Byte**
//! des normalisierten Textes, aus welchem Original-Zeichen es entstanden ist.

/// Normalisierter Text samt Rückabbildung auf die Bytes des Originaltextes.
///
/// # Aufbau der Abbildung
///
/// `starts` und `ends` haben exakt so viele Einträge wie `text` Bytes hat.
/// Für das normalisierte Byte `i` gilt:
///
/// * `starts[i]` = Byte-Offset des Original-Zeichens, aus dem `i` entstanden ist,
/// * `ends[i]`   = exklusives Byte-Ende genau dieses Original-Zeichens.
///
/// Beide Vektoren sind nötig, weil die Abbildung nicht längentreu ist:
///
/// * Ein Original-Zeichen kann beim Kleinschreiben mehrere Bytes (oder sogar
///   mehrere Zeichen) erzeugen — dann zeigen alle diese Bytes auf dasselbe
///   Original-Zeichen.
/// * Eine ganze Folge von Leerraum-Zeichen wird zu einem einzigen Leerzeichen —
///   dieses Byte zeigt auf das **erste** Leerraum-Zeichen der Folge.
///
/// Ein Treffer `[start, end)` im normalisierten Text wird über
/// [`Normalized::original_span`] auf `[starts[start], ends[end - 1])` im
/// Originaltext zurückgerechnet. Weil die äußeren Grenzen verwendet werden,
/// deckt die Rückabbildung auch zusammengefassten Leerraum *innerhalb* des
/// Treffers vollständig ab.
#[derive(Debug, Clone)]
pub(crate) struct Normalized {
    /// Der normalisierte Text (klein geschrieben, Leerraum zusammengefasst).
    pub(crate) text: String,
    /// Byte-Start des Original-Zeichens je normalisiertem Byte.
    starts: Vec<usize>,
    /// Exklusives Byte-Ende des Original-Zeichens je normalisiertem Byte.
    ends: Vec<usize>,
}

impl Normalized {
    /// Normalisiert `input` und baut dabei die Rückabbildung auf.
    pub(crate) fn new(input: &str) -> Self {
        let mut text = String::with_capacity(input.len());
        let mut starts = Vec::with_capacity(input.len());
        let mut ends = Vec::with_capacity(input.len());
        let mut in_whitespace = false;

        for (idx, ch) in input.char_indices() {
            let char_end = idx + ch.len_utf8();
            if ch.is_whitespace() {
                // Leerraum-Folge auf ein einzelnes Leerzeichen eindampfen.
                if !in_whitespace {
                    text.push(' ');
                    starts.push(idx);
                    ends.push(char_end);
                }
                in_whitespace = true;
                continue;
            }
            in_whitespace = false;
            for lower in ch.to_lowercase() {
                text.push(lower);
            }
            // Alle neu erzeugten Bytes zeigen auf dasselbe Original-Zeichen.
            while starts.len() < text.len() {
                starts.push(idx);
                ends.push(char_end);
            }
        }

        debug_assert_eq!(starts.len(), text.len());
        debug_assert_eq!(ends.len(), text.len());
        Self { text, starts, ends }
    }

    /// Rechnet den normalisierten Bereich `[start, end)` auf einen Byte-Bereich
    /// des Originaltextes zurück.
    ///
    /// Liefert `None` für leere oder außerhalb liegende Bereiche.
    pub(crate) fn original_span(&self, start: usize, end: usize) -> Option<(usize, usize)> {
        if start >= end || end > self.starts.len() {
            return None;
        }
        Some((self.starts[start], self.ends[end - 1]))
    }
}

/// Normalisiert ein Suchmuster (Nadel).
///
/// Zusätzlich zur Normalisierung des Heuhaufens wird führender und
/// abschließender Leerraum entfernt — sonst könnte ein Muster mit Rand-Leerraum
/// niemals am Anfang oder Ende eines Text-Runs treffen.
pub(crate) fn normalize_needle(input: &str) -> String {
    Normalized::new(input).text.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Achtung beim Lesen: dass hier ein `\n` zusammengefasst wird, belegt
    /// **nicht**, dass eine über zwei Zeilen verteilte IBAN gefunden wird. Der
    /// Normalisierer bekommt einen fertigen `TextRun`, und ein Run enthält nie
    /// einen Zeilenumbruch (siehe Modulkommentar). Der Fall `\n` ist hier nur
    /// der Vollständigkeit halber mitgeprüft — die reale Lage hält
    /// `iban_split_across_two_extracted_lines_is_not_found` in
    /// `tests/matcher_tests.rs` fest.
    #[test]
    fn collapses_whitespace_and_lowercases() {
        assert_eq!(Normalized::new("DE89  3704 0044").text, "de89 3704 0044");
        assert_eq!(Normalized::new("DE89  3704\n0044").text, "de89 3704 0044");
    }

    #[test]
    fn maps_normalized_match_back_to_original_bytes() {
        let original = "Konto:  DE89  3704 x";
        let n = Normalized::new(original);
        let needle = normalize_needle(" DE89 3704 ");
        let pos = n.text.find(&needle).unwrap();
        let (s, e) = n.original_span(pos, pos + needle.len()).unwrap();
        assert_eq!(&original[s..e], "DE89  3704");
    }

    #[test]
    fn maps_multibyte_characters_correctly() {
        let original = "Müller Straße";
        let n = Normalized::new(original);
        let needle = normalize_needle("straße");
        let pos = n.text.find(&needle).unwrap();
        let (s, e) = n.original_span(pos, pos + needle.len()).unwrap();
        assert_eq!(&original[s..e], "Straße");
    }

    #[test]
    fn rejects_empty_and_out_of_range_spans() {
        let n = Normalized::new("abc");
        assert!(n.original_span(1, 1).is_none());
        assert!(n.original_span(0, 99).is_none());
    }

    #[test]
    fn leading_whitespace_does_not_shift_offsets() {
        let original = "   abc";
        let n = Normalized::new(original);
        assert_eq!(n.text, " abc");
        let pos = n.text.find("abc").unwrap();
        let (s, e) = n.original_span(pos, pos + 3).unwrap();
        assert_eq!((s, e), (3, 6));
    }
}
