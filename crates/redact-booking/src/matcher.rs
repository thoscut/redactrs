//! Abgleich von Text gegen Positiv- und Negativliste.

use redact_core::{
    Analyzer, BookingEntry, ListType, MatchType, RedactError, Region, Result, Source, TextRun,
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

    /// Alle Fundstellen im normalisierten Heuhaufen, links nach rechts,
    /// überlappungsfrei und bereits kontext-geprüft.
    ///
    /// Schlägt die Kontextprüfung an einer Fundstelle fehl, wird nur diese
    /// Fundstelle verworfen — die Suche läuft im selben Run weiter.
    fn occurrences<'a>(&'a self, hay: &'a Normalized) -> impl Iterator<Item = (usize, usize)> + 'a {
        hay.text
            .match_indices(self.needle.as_str())
            .map(|(start, m)| (start, start + m.len()))
            .filter(|&(start, end)| self.context_ok(&hay.text, start, end))
    }

    /// Erste kontext-geprüfte Fundstelle, falls vorhanden.
    fn first_match(&self, hay: &Normalized) -> Option<(usize, usize)> {
        self.occurrences(hay).next()
    }

    /// Prüft `context_before` gegen den Text vor und `context_after` gegen den
    /// Text nach der Fundstelle (beides normalisiert, also ohne Beachtung von
    /// Groß-/Kleinschreibung und Leerraum-Mengen).
    fn context_ok(&self, hay: &str, start: usize, end: usize) -> bool {
        if let Some(before) = &self.context_before {
            if !hay[..start].contains(before.as_str()) {
                return false;
            }
        }
        if let Some(after) = &self.context_after {
            if !hay[end..].contains(after.as_str()) {
                return false;
            }
        }
        true
    }
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

    /// Konzept-API: prüft eine ganze Region anhand ihres Textes.
    ///
    /// Die Negativliste hat Vorrang. Ohne Text (`region.text == None`) gibt es
    /// keinen Treffer.
    pub fn match_region(&self, region: &Region) -> Option<Source> {
        let text = region.text.as_deref()?;
        let hay = Normalized::new(text);

        for compiled in &self.negative_compiled {
            if compiled.first_match(&hay).is_some() {
                return Some(Source::Booking {
                    booking_id: compiled.id.clone(),
                    match_type: MatchType::Negative,
                });
            }
        }
        for compiled in &self.positive_compiled {
            if compiled.first_match(&hay).is_some() {
                return Some(Source::Booking {
                    booking_id: compiled.id.clone(),
                    match_type: MatchType::Positive,
                });
            }
        }
        None
    }

    /// Präzise Variante: eine Region je Fundstelle mit exakter Bounding-Box.
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
        if self.is_empty() {
            return Ok(regions);
        }

        for run in runs {
            if run.text.is_empty() {
                continue;
            }
            let hay = Normalized::new(&run.text);
            self.collect(
                run,
                &hay,
                &self.negative_compiled,
                MatchType::Negative,
                &mut regions,
            );
            self.collect(
                run,
                &hay,
                &self.positive_compiled,
                MatchType::Positive,
                &mut regions,
            );
        }
        Ok(regions)
    }

    /// Sammelt die Treffer einer Liste für einen einzelnen Run.
    fn collect(
        &self,
        run: &TextRun,
        hay: &Normalized,
        list: &[Compiled],
        match_type: MatchType,
        out: &mut Vec<Region>,
    ) {
        for compiled in list {
            for (norm_start, norm_end) in compiled.occurrences(hay) {
                // Normalisierte Fundstelle auf Original-Bytes zurückrechnen …
                let Some((start, end)) = hay.original_span(norm_start, norm_end) else {
                    continue;
                };
                // … und daraus die exakte Bounding-Box der Glyphen bestimmen.
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
                        booking_id: compiled.id.clone(),
                        match_type,
                    },
                ));
            }
        }
    }
}

impl Analyzer for BookingMatcher {
    fn analyze(&self, runs: &[TextRun]) -> Result<Vec<Region>> {
        self.find_matches(runs)
    }
}
