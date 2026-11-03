//! Abgleich von Text gegen Positiv- und Negativliste.

use aho_corasick::{AhoCorasick, MatchKind};
use redact_core::{
    BookingEntry, ListType, MatchType, RedactError, Region, Result, Source, TextRun,
};

use crate::normalize::{normalize_needle, Normalized};

/// Negativlisten-Treffer werden um diesen Betrag (in pt) vergrößert, damit sie
/// überlappende Pattern-Treffer bei der Konfliktauflösung zuverlässig decken.
const NEGATIVE_PADDING_PT: f64 = 2.0;

/// Zeichen, die in einem `is_regex`-Muster echte Regex-Syntax bedeuten würden.
const REGEX_METACHARS: [char; 14] = [
    '\\', '.', '*', '+', '?', '(', ')', '[', ']', '{', '}', '|', '^', '$',
];

/// Ein für den Abgleich vorbereiteter Eintrag: Muster und Kontexte liegen
/// bereits normalisiert vor (siehe [`crate::normalize`]).
#[derive(Debug, Clone)]
struct Compiled {
    id: String,
    needle: String,
    context_before: Option<String>,
    context_after: Option<String>,
}

impl Compiled {
    /// Bereitet einen Eintrag vor bzw. lehnt ihn mit klarer Meldung ab.
    fn new(entry: &BookingEntry) -> Result<Self> {
        if entry.is_regex {
            if let Some(meta) = entry.pattern.chars().find(|c| REGEX_METACHARS.contains(c)) {
                return Err(RedactError::Booking(format!(
                    "Eintrag `{}`: Buchungslisten unterstützen keine echten regulären Ausdrücke \
                     (Metazeichen `{meta}` in `{}`). Regex-Muster bitte über `--patterns-config` \
                     definieren; `is_regex` wird hier nur für metazeichenfreie Muster als \
                     Literal-Suche akzeptiert.",
                    entry.id, entry.pattern
                )));
            }
        }
        let needle = normalize_needle(&entry.pattern);
        if needle.is_empty() {
            return Err(RedactError::Booking(format!(
                "Eintrag `{}`: leeres Muster",
                entry.id
            )));
        }
        Ok(Self {
            id: entry.id.clone(),
            needle,
            context_before: entry.context_before.as_deref().map(normalize_needle),
            context_after: entry.context_after.as_deref().map(normalize_needle),
        })
    }

    /// Prüft `context_before` gegen den Text vor und `context_after` gegen den
    /// Text nach der Fundstelle (beides normalisiert, also ohne Beachtung von
    /// Groß-/Kleinschreibung und Leerraum-Mengen).
    ///
    /// `bounds` ist [`Compiled::context_bounds`] für diesen Heuhaufen. Das ist
    /// gleichwertig mit `hay[..start].contains(before)` und
    /// `hay[end..].contains(after)`: `before` steht genau dann vor `start`, wenn
    /// sein *erstes* Vorkommen spätestens bei `start` endet; `after` steht genau
    /// dann hinter `end`, wenn sein *letztes* Vorkommen frühestens bei `end`
    /// beginnt. So kostet die Prüfung je Fundstelle O(1) statt O(Zeilenlänge).
    fn context_ok(bounds: ContextBounds, start: usize, end: usize) -> bool {
        let before_ok = match bounds.before_first_end {
            None => true,
            Some(None) => false,
            Some(Some(first_end)) => first_end <= start,
        };
        let after_ok = match bounds.after_last_start {
            None => true,
            Some(None) => false,
            Some(Some(last_start)) => last_start >= end,
        };
        before_ok && after_ok
    }

    /// Wo die Kontexte im Heuhaufen stehen: einmal je (Run, Eintrag) und nur,
    /// wenn der Eintrag im Run überhaupt eine Fundstelle hat.
    fn context_bounds(&self, hay: &str) -> ContextBounds {
        ContextBounds {
            before_first_end: self
                .context_before
                .as_ref()
                .map(|b| hay.find(b.as_str()).map(|i| i + b.len())),
            after_last_start: self.context_after.as_ref().map(|a| hay.rfind(a.as_str())),
        }
    }
}

/// Lage der Kontexte eines Eintrags in einem Heuhaufen. Äußeres `None`: der
/// Eintrag hat diesen Kontext nicht; inneres `None`: er kommt im Run nicht vor.
#[derive(Debug, Clone, Copy)]
struct ContextBounds {
    /// Ende des ersten Vorkommens von `context_before`.
    before_first_end: Option<Option<usize>>,
    /// Anfang des letzten Vorkommens von `context_after`.
    after_last_start: Option<Option<usize>>,
}

/// Gleicht Text-Runs gegen eine Buchungsliste ab.
///
/// Die Negativliste hat immer Vorrang: sie wird zuerst geprüft und ihre Treffer
/// blockieren später (in [`redact_core::resolve_conflicts`]) alle überlappenden
/// Schwärzungen.
#[derive(Debug, Clone, Default)]
pub struct BookingMatcher {
    /// Positivliste in Dateireihenfolge.
    positive: Vec<BookingEntry>,
    /// Negativliste in Dateireihenfolge.
    negative: Vec<BookingEntry>,
    /// Vorbereitete Muster, index-gleich zu `positive`.
    positive_compiled: Vec<Compiled>,
    /// Vorbereitete Muster, index-gleich zu `negative`.
    negative_compiled: Vec<Compiled>,
    /// Ein Automat über **alle** Nadeln: Muster-Id `i < negative.len()` ist
    /// `negative_compiled[i]`, darüber `positive_compiled[i - negative.len()]`.
    /// Damit wird jede Zeile einmal gelesen statt einmal je Eintrag — vorher
    /// wuchs die Laufzeit mit Einträgen × Text (siehe
    /// `tests/zq_d_skalierung.rs`). `None` nur bei leerer Liste.
    automaton: Option<AhoCorasick>,
}

impl BookingMatcher {
    /// Baut den Matcher aus geladenen Einträgen.
    ///
    /// Fehler entstehen nur durch unbrauchbare Muster (leer oder echte
    /// Regex-Syntax bei `is_regex = true`).
    pub fn new(entries: Vec<BookingEntry>) -> Result<Self> {
        let mut matcher = Self::default();
        for entry in entries {
            let compiled = Compiled::new(&entry)?;
            match entry.list_type {
                ListType::Positive => {
                    matcher.positive.push(entry);
                    matcher.positive_compiled.push(compiled);
                }
                ListType::Negative => {
                    matcher.negative.push(entry);
                    matcher.negative_compiled.push(compiled);
                }
            }
        }
        if !matcher.is_empty() {
            let needles = matcher
                .negative_compiled
                .iter()
                .chain(&matcher.positive_compiled)
                .map(|c| c.needle.as_str());
            // `Standard` + überlappende Suche: jede Fundstelle jeder Nadel,
            // auch wenn sich Nadeln gegenseitig enthalten oder überlappen. Die
            // Auswahl je Eintrag trifft danach `collect`.
            let automaton = AhoCorasick::builder()
                .match_kind(MatchKind::Standard)
                .build(needles)
                .map_err(|e| {
                    RedactError::Booking(format!("Buchungsliste lässt sich nicht vorbereiten: {e}"))
                })?;
            matcher.automaton = Some(automaton);
        }
        Ok(matcher)
    }

    /// Einträge der Positivliste (müssen geschwärzt werden).
    pub fn positive(&self) -> &[BookingEntry] {
        &self.positive
    }

    /// Einträge der Negativliste (dürfen **nicht** geschwärzt werden).
    pub fn negative(&self) -> &[BookingEntry] {
        &self.negative
    }

    /// Enthält der Matcher überhaupt Einträge?
    pub fn is_empty(&self) -> bool {
        self.positive.is_empty() && self.negative.is_empty()
    }

    /// Sucht alle Treffer: eine Region je Fundstelle mit exakter Bounding-Box.
    ///
    /// **Je Run, nicht über Runs hinweg.** Jeder `TextRun` ist eine extrahierte
    /// Zeile und wird für sich normalisiert und durchsucht; ein Muster, das im
    /// PDF über zwei Zeilen verteilt steht, trifft deshalb nicht (siehe
    /// [`crate::normalize`]).
    ///
    /// Reihenfolge (deterministisch): Runs in Eingabereihenfolge, je Run zuerst
    /// die Negativliste in Dateireihenfolge, dann die Positivliste, je Eintrag
    /// die Fundstellen von links nach rechts.
    ///
    /// Negativlisten-Treffer werden ebenfalls zurückgegeben (mit
    /// [`MatchType::Negative`]) und dienen der Konfliktauflösung als Blocker;
    /// ihr Rechteck wird um [`NEGATIVE_PADDING_PT`] pt vergrößert.
    pub fn find_matches(&self, runs: &[TextRun]) -> Result<Vec<Region>> {
        let mut regions = Vec::new();
        let Some(automaton) = &self.automaton else {
            return Ok(regions);
        };

        let mut hits: Vec<(usize, usize)> = Vec::new();
        for run in runs {
            if run.text.is_empty() {
                continue;
            }
            let hay = Normalized::new(&run.text);
            // Jede Fundstelle jeder Nadel als (Muster-Id, Start). Sortiert
            // ergibt das genau die geforderte Reihenfolge: Negativliste vor
            // Positivliste, je Liste Dateireihenfolge, je Eintrag links nach
            // rechts.
            hits.clear();
            hits.extend(
                automaton
                    .find_overlapping_iter(hay.text.as_str())
                    .map(|m| (m.pattern().as_usize(), m.start())),
            );
            hits.sort_unstable();
            self.collect(run, &hay, &hits, &mut regions);
        }
        Ok(regions)
    }

    /// Der Eintrag zur Muster-Id des Automaten.
    fn compiled(&self, id: usize) -> (&Compiled, MatchType) {
        match id.checked_sub(self.negative_compiled.len()) {
            None => (&self.negative_compiled[id], MatchType::Negative),
            Some(p) => (&self.positive_compiled[p], MatchType::Positive),
        }
    }

    /// Macht aus den sortierten Fundstellen eines Runs die Regionen.
    ///
    /// Je Eintrag wie `str::match_indices`: von links nach rechts,
    /// **überlappungsfrei** (eine Fundstelle, die in die vorige hineinreicht,
    /// zählt nicht), und eine Fundstelle mit falschem Kontext wird nur selbst
    /// verworfen — sie sperrt die folgenden nicht.
    fn collect(
        &self,
        run: &TextRun,
        hay: &Normalized,
        hits: &[(usize, usize)],
        out: &mut Vec<Region>,
    ) {
        for group in hits.chunk_by(|a, b| a.0 == b.0) {
            let (compiled, match_type) = self.compiled(group[0].0);
            let bounds = compiled.context_bounds(&hay.text);
            // Ein Zeiger je (Run, Eintrag): die Fundstellen kommen aufsteigend,
            // die Rechtecke kosten zusammen einen Durchlauf statt einen je
            // Fundstelle (siehe `redact_core::GlyphCursor`).
            let mut cursor = run.glyph_cursor();
            let mut next_free = 0;
            for &(_, norm_start) in group {
                if norm_start < next_free {
                    continue;
                }
                let norm_end = norm_start + compiled.needle.len();
                next_free = norm_end;
                if !Compiled::context_ok(bounds, norm_start, norm_end) {
                    continue;
                }
                // Normalisierte Fundstelle auf Original-Bytes zurückrechnen …
                let Some((start, end)) = hay.original_span(norm_start, norm_end) else {
                    continue;
                };
                // … und daraus die exakte Bounding-Box der Glyphen bestimmen.
                let Some(rect) = cursor.rect_for_byte_range(start, end) else {
                    continue;
                };
                let rect = match match_type {
                    MatchType::Negative => rect.expanded(NEGATIVE_PADDING_PT),
                    MatchType::Positive => rect,
                };
                out.push(Region::new(
                    run.page,
                    rect,
                    run.text.get(start..end).map(|s| s.to_string()),
                    Source::Booking {
                        booking_id: compiled.id.clone(),
                        match_type,
                    },
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Der Automat muss **genau** liefern, was die alte Suche je Eintrag
    //! lieferte: dieselben Regionen in derselben Reihenfolge.

    use super::*;
    use redact_core::{Glyph, Rect};

    /// Die frühere Suche, wörtlich: je Run, Negativliste vor Positivliste, je
    /// Eintrag `match_indices`, Kontext per `contains`, Rechteck ohne Zeiger.
    fn reference(m: &BookingMatcher, runs: &[TextRun]) -> Vec<Region> {
        let mut out = Vec::new();
        for run in runs.iter().filter(|r| !r.text.is_empty()) {
            let hay = Normalized::new(&run.text);
            for (list, match_type) in [
                (&m.negative_compiled, MatchType::Negative),
                (&m.positive_compiled, MatchType::Positive),
            ] {
                for c in list {
                    for (ns, s) in hay.text.match_indices(c.needle.as_str()) {
                        let ne = ns + s.len();
                        let before = c
                            .context_before
                            .as_ref()
                            .is_none_or(|b| hay.text[..ns].contains(b.as_str()));
                        let after = c
                            .context_after
                            .as_ref()
                            .is_none_or(|a| hay.text[ne..].contains(a.as_str()));
                        if !(before && after) {
                            continue;
                        }
                        let Some((start, end)) = hay.original_span(ns, ne) else {
                            continue;
                        };
                        let Some(rect) = run.rect_for_byte_range(start, end) else {
                            continue;
                        };
                        let rect = match match_type {
                            MatchType::Negative => rect.expanded(NEGATIVE_PADDING_PT),
                            MatchType::Positive => rect,
                        };
                        out.push(Region::new(
                            run.page,
                            rect,
                            run.text.get(start..end).map(|s| s.to_string()),
                            Source::Booking {
                                booking_id: c.id.clone(),
                                match_type,
                            },
                        ));
                    }
                }
            }
        }
        out
    }

    fn run(page: usize, text: &str) -> TextRun {
        let glyphs = text
            .chars()
            .enumerate()
            .map(|(i, ch)| Glyph {
                ch,
                rect: Rect::new(i as f64 * 5.0, 0.0, (i as f64 + 1.0) * 5.0, 10.0),
            })
            .collect();
        TextRun::new(page, glyphs)
    }

    /// Kleiner deterministischer Zufall (xorshift), damit der Test ohne
    /// weitere Abhängigkeit auskommt und jeder Lauf dasselbe prüft.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
        /// Text aus einem winzigen Alphabet: viele Überlappungen, Groß- und
        /// Kleinschreibung, Leerraum-Folgen, ein Umlaut samt zerlegter Form.
        fn text(&mut self, max: usize) -> String {
            const ZEICHEN: [&str; 9] = ["a", "A", "b", "ab", " ", "  ", "\t", "ü", "u\u{308}"];
            let n = self.below(max + 1);
            (0..n).map(|_| ZEICHEN[self.below(ZEICHEN.len())]).collect()
        }
    }

    #[test]
    fn automat_liefert_dasselbe_wie_die_suche_je_eintrag() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let mut verglichen = 0usize;
        for fall in 0..3_000 {
            let mut entries = Vec::new();
            for i in 0..1 + rng.below(6) {
                let pattern = rng.text(4);
                if normalize_needle(&pattern).is_empty() {
                    continue;
                }
                let kontext = |rng: &mut Rng| (rng.below(3) == 0).then(|| rng.text(2));
                entries.push(BookingEntry {
                    id: format!("e{i}"),
                    list_type: if rng.below(3) == 0 {
                        ListType::Negative
                    } else {
                        ListType::Positive
                    },
                    pattern,
                    context_before: kontext(&mut rng),
                    context_after: kontext(&mut rng),
                    is_regex: false,
                });
            }
            // Gelegentlich ein Eintrag doppelt: beide müssen treffen.
            if !entries.is_empty() && rng.below(4) == 0 {
                let mut doppelt = entries[rng.below(entries.len())].clone();
                doppelt.id.push_str("-doppelt");
                entries.push(doppelt);
            }
            let m = BookingMatcher::new(entries).unwrap();
            let runs: Vec<TextRun> = (0..1 + rng.below(3))
                .map(|p| run(p, &rng.text(30)))
                .collect();
            let erwartet = reference(&m, &runs);
            let bekommen = m.find_matches(&runs).unwrap();
            assert_eq!(bekommen, erwartet, "Fall {fall}: {runs:?}");
            verglichen += erwartet.len();
        }
        // Der Vergleich darf nicht leer gelaufen sein.
        assert!(verglichen > 1_000, "nur {verglichen} Treffer verglichen");
    }
}
