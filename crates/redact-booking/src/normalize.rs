//! Normalisierung von Text für den Vergleich mit Buchungslisten-Einträgen.
//!
//! Buchungslisten enthalten Werte so, wie ein Mensch sie schreibt
//! (`DE89 3704 0044 0532 0130 00`), im PDF steht derselbe Wert aber oft mit
//! einer anderen *Menge* an Leerraum — `DE89  3704 0044 …`, gesperrt gesetzt,
//! oder ganz ohne Leerzeichen. Deshalb wird sowohl der Suchtext (Heuhaufen) als
//! auch das Muster (Nadel) vor dem Vergleich normalisiert:
//!
//! * alle Zeichen werden klein geschrieben (Vergleich ohne Groß-/Kleinschreibung),
//! * jede Folge von Leerraum wird zu genau einem Leerzeichen zusammengefasst,
//! * zerlegte Akzentbuchstaben („u" + U+0308) werden zum fertigen Zeichen
//!   („ü") zusammengesetzt — für eine kleine Tabelle, siehe `compose`.
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
        // Das zuletzt geschriebene Zeichen, falls es mit einem folgenden
        // kombinierenden Akzent verschmolzen werden darf: Byte-Position in
        // `text`, Byte-Start im Original, das Zeichen selbst.
        let mut basis: Option<(usize, usize, char)> = None;

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
                basis = None;
                continue;
            }
            in_whitespace = false;
            // Zerlegte Umlaute (NFD, „u" + U+0308) aus dem PDF mit dem
            // vorhergehenden Buchstaben zusammensetzen. Manche Erzeuger
            // schreiben Umlaute so, die Buchungsliste aber steht fast immer
            // zusammengesetzt (NFC) da — ohne diesen Schritt fand „Jürgen
            // Müller" den Namen im Dokument nicht. Das zusammengesetzte
            // Zeichen deckt im Original Buchstabe *und* Akzent ab, damit die
            // Schwärzung beide erfasst.
            if let Some((pos, base_idx, base)) = basis {
                if let Some(composed) = compose(base, ch) {
                    text.truncate(pos);
                    starts.truncate(pos);
                    ends.truncate(pos);
                    text.push(composed);
                    while starts.len() < text.len() {
                        starts.push(base_idx);
                        ends.push(char_end);
                    }
                    basis = Some((pos, base_idx, composed));
                    continue;
                }
            }
            let pos = text.len();
            let mut lower_iter = ch.to_lowercase();
            // Nur ein Zeichen, das beim Kleinschreiben eins bleibt, kann
            // Basis einer Zusammensetzung sein.
            basis = match (lower_iter.next(), lower_iter.next()) {
                (Some(l), None) => Some((pos, idx, l)),
                _ => None,
            };
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

/// Setzt einen (kleinen) Buchstaben und einen kombinierenden Akzent zu einem
/// Zeichen zusammen — eine bewusst kleine Teilmenge der Unicode-Komposition
/// (NFC), beschränkt auf die Buchstaben, die in deutschen und
/// westeuropäischen Namen vorkommen. Eine vollständige Tabelle hieße eine neue
/// Abhängigkeit; was hier fehlt, bleibt einfach unverändert stehen und wird
/// wie bisher Zeichen für Zeichen verglichen.
fn compose(base: char, mark: char) -> Option<char> {
    let composed = match (base, mark) {
        // Trema / Umlaut
        ('a', '\u{308}') => 'ä',
        ('o', '\u{308}') => 'ö',
        ('u', '\u{308}') => 'ü',
        ('e', '\u{308}') => 'ë',
        ('i', '\u{308}') => 'ï',
        ('y', '\u{308}') => 'ÿ',
        // Gravis
        ('a', '\u{300}') => 'à',
        ('e', '\u{300}') => 'è',
        ('i', '\u{300}') => 'ì',
        ('o', '\u{300}') => 'ò',
        ('u', '\u{300}') => 'ù',
        // Akut
        ('a', '\u{301}') => 'á',
        ('e', '\u{301}') => 'é',
        ('i', '\u{301}') => 'í',
        ('o', '\u{301}') => 'ó',
        ('u', '\u{301}') => 'ú',
        ('y', '\u{301}') => 'ý',
        ('c', '\u{301}') => 'ć',
        ('n', '\u{301}') => 'ń',
        ('s', '\u{301}') => 'ś',
        ('z', '\u{301}') => 'ź',
        // Zirkumflex
        ('a', '\u{302}') => 'â',
        ('e', '\u{302}') => 'ê',
        ('i', '\u{302}') => 'î',
        ('o', '\u{302}') => 'ô',
        ('u', '\u{302}') => 'û',
        // Tilde
        ('a', '\u{303}') => 'ã',
        ('n', '\u{303}') => 'ñ',
        ('o', '\u{303}') => 'õ',
        // Ring
        ('a', '\u{30A}') => 'å',
        ('u', '\u{30A}') => 'ů',
        // Hatschek
        ('c', '\u{30C}') => 'č',
        ('e', '\u{30C}') => 'ě',
        ('n', '\u{30C}') => 'ň',
        ('r', '\u{30C}') => 'ř',
        ('s', '\u{30C}') => 'š',
        ('z', '\u{30C}') => 'ž',
        // Cedille
        ('c', '\u{327}') => 'ç',
        ('s', '\u{327}') => 'ş',
        _ => return None,
    };
    Some(composed)
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
    fn composes_decomposed_umlauts_and_maps_back_over_the_mark() {
        let original = "Mu\u{308}ller";
        let n = Normalized::new(original);
        assert_eq!(n.text, "müller");
        let (s, e) = n.original_span(0, "mü".len()).unwrap();
        // „Mu" + U+0308: das „ü" deckt Buchstabe und Akzent ab.
        assert_eq!(&original[s..e], "Mu\u{308}");
        let (s, e) = n.original_span(0, n.text.len()).unwrap();
        assert_eq!((s, e), (0, original.len()));
        // Ein großer Buchstabe wird erst kleingeschrieben, dann verschmolzen.
        assert_eq!(Normalized::new("U\u{308}BER").text, "über");
        // Ein Akzent ohne passende Basis bleibt stehen.
        assert_eq!(Normalized::new(" \u{308}x").text, " \u{308}x");
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
