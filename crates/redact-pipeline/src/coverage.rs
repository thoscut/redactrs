//! Welche Warnung heißt „das Ergebnis ist womöglich unvollständig“?
//!
//! # Warum es diese Datei gibt
//!
//! Eine Seite, deren Content-Stream sich in **keine** Operation zerlegen lässt,
//! bricht den Lauf hart ab (`redact_pdf::content::scan_page`). Der Grund steht
//! dort: „0 Schwärzungen, Rückgabewert 0“ liest sich wie „nichts gefunden, also
//! sauber“, und genau das wäre es nicht.
//!
//! Dieselbe Aussage machen aber noch ein halbes Dutzend anderer Stellen — teils
//! wortgleich, „wurde nicht durchsucht und kann deshalb nicht geschwärzt worden
//! sein“ —, und die endeten mit Rückgabewert 0. Ein Font ohne `/ToUnicode`, ein
//! Form-XObject unterhalb der Verschachtelungsgrenze, ein Kachelmuster mit
//! Text, eine Annotation ohne `/AP`: in all diesen Fällen hat die Analyse einen
//! Teil des Dokuments **nie gesehen**. Sie kann deshalb nicht sagen, ob dort
//! etwas stehen geblieben ist. Im Stapelbetrieb zählten solche Dateien als
//! Erfolg, und die Warnung ging in stderr unter.
//!
//! # Die Unterscheidung, auf die es ankommt
//!
//! Nicht jede Warnung heißt das. Ein neu kodiertes Bild, eine Datei mit
//! inkrementellen Revisionen, eine Schwärzung, die über einer Grafik kein
//! Zeichen entfernt hat — das sind Mitteilungen über das *Ergebnis*, und die
//! Analyse hat dabei alles gesehen, was es zu sehen gab. Würde der
//! Rückgabewert auch dafür anspringen, wäre er nach der zweiten Datei ein Wert,
//! den man wegdrückt; und ein Alarm, den man wegdrückt, ist keiner.
//!
//! **Deckungslücke** heißt hier deshalb genau eines: *die Analyse konnte für
//! einen Teil des Dokuments nicht einstehen.* Entweder hat sie ihn nicht
//! gelesen, oder sie hat das Ergebnis dort nicht nachgemessen.
//!
//! # Warum die Liste die Ausnahmen nennt und nicht die Fälle
//!
//! Umgekehrt wäre es bequemer, aber falsch herum. `redact-pdf` bekommt neue
//! Warnungen, sobald jemand einen weiteren blinden Fleck schließt — und ein
//! blinder Fleck ist per Bauart eine Deckungslücke. Eine Liste der *Fälle*
//! ließe jede neue Warnung stillschweigend auf Rückgabewert 0 laufen, also
//! genau in den Fehler zurückfallen, um den es hier geht.
//!
//! Deshalb: **im Zweifel eine Lücke.** Wer eine Warnung hinzufügt, die keine
//! ist, trägt sie unten mit Begründung ein. Das ist eine Zeile Arbeit an der
//! richtigen Stelle statt eines stillen Rückgabewerts 0 an der falschen.

/// Warnungen, die **keine** Deckungslücke sind — Textmarke und Begründung.
///
/// Die Textmarken sind bewusst kurz und stehen im *unveränderlichen* Teil der
/// jeweiligen Meldung (nicht in einer Zahl, nicht in einem Objektnamen).
/// Der Test `every_known_warning_is_sorted_the_way_it_is_meant` unten hält sie
/// gegen den Wortlaut, mit dem `redact-pdf` und [`crate::audit::Effects`] sie
/// heute erzeugen; `redact-cli/tests/incomplete.rs` prüft einen Fall
/// stellvertretend am ganzen Weg bis zum Rückgabewert.
pub const NOT_A_COVERAGE_GAP: &[(&str, &str)] = &[
    // --- Mitteilungen über die Ausgabedatei ---
    (
        "Bild(er) überschrieben",
        "die Bildpunkte sind wirklich weg; gemeldet wird nur, dass die Datei \
         dadurch neu kodiert und größer wird",
    ),
    (
        "die Schwärzung wirkt deshalb auch auf die anderen Seiten",
        "zu viel geschwärzt, nicht zu wenig — das Gegenteil einer Lücke",
    ),
    (
        "wird mit den Metadaten als Ganzes entfernt",
        "eine Annotation mit Text, aber ohne Erscheinungsstrom: der Text hat \
         keine Glyphen und wird nicht anteilig geschwärzt — `strip_metadata` \
         nimmt ihn als Ganzes, und das läuft in der Kette immer (Fix-Runde 4, \
         Befund G2-7: vorher Rückgabewert 3 an einer Datei, an der \
         `--check-leaks` danach 0 meldete). Gemessen: nach `strip_metadata` \
         findet `leaks` den Text nicht mehr",
    ),
    (
        "Die Maske verbirgt nichts und ist deshalb",
        "am dekodierten Bild nachgemessen: kein Abtastwert fällt in den \
         Schlüsselbereich, samt Band für den verlustbehafteten Decoder. Es wird \
         also nichts sichtbar, was die Eingabe verborgen hätte — die Analyse \
         kann für diese Datei vollständig einstehen. Ohne diesen Eintrag \
         endete ein JPEG mit Farbschlüssel-Maske mit „NICHT GEPRÜFT“ und \
         Rückgabewert 3, obwohl gemessen wurde, dass es nichts zu prüfen gibt",
    ),
    (
        "inkrementellen Revisionen",
        "die Ausgabe wird als eine einzige Revision ohne Vorgeschichte \
         geschrieben; der Hinweis gilt der Eingabedatei",
    ),
    (
        "Eigenschaftsliste einer Marked-Content-Auszeichnung",
        "betrifft das Aussehen der Ausgabe, nicht die Reichweite der Analyse",
    ),
    // --- Eine Eigenschaft des Werkzeugs, kein Befund an dieser Datei ---
    //
    // Der Grenzfall, und er ist es wert, ausgeschrieben zu werden. Wörtlich
    // *ist* „Text in einem Bild wird nicht gelesen“ eine Deckungslücke: ohne
    // OCR sieht die Analyse dort nichts. Die Meldung entsteht aber, sobald auf
    // irgendeiner Seite **irgendein** Rasterbild liegt — ein Logo im Briefkopf
    // genügt. Sie gilt damit für so gut wie jeden eingescannten Auszug, also
    // für den Regelfall dieses Werkzeugs.
    //
    // Ein Rückgabewert, der bei jedem zweiten Kontoauszug anspringt, sagt nach
    // der dritten Datei nichts mehr; er würde die Fälle mit begraben, für die
    // er gedacht ist (ein Font ohne /ToUnicode, ein ungelesenes XObject) —
    // und die sind selten und behebbar. Der Satz ist außerdem keine Aussage
    // über *diese* Datei, sondern über redact-rs: „wir können kein OCR“. Das
    // gehört in die Dokumentation und in die Warnung, nicht in einen
    // Rückgabewert, der bei jedem Lauf gleich ausfällt.
    //
    // **Nicht mitgemeint** ist der Nachbarfall: ein Bild, das sich *nicht
    // dekodieren* ließ („die Pixel blieben in der Datei“). Dort hat die
    // Schwärzung etwas versucht und es nicht geschafft — das bleibt eine
    // Lücke.
    (
        "enthalten Rasterbilder",
        "eine ständige Eigenschaft des Werkzeugs (kein OCR), keine Besonderheit \
         dieser Datei — sie träfe fast jeden eingescannten Auszug und machte den \
         Rückgabewert damit wertlos",
    ),
    // --- Eine Anweisung des Nutzers, kein Befund an dieser Datei ---
    //
    // Der zweite Grenzfall, und er ist es genauso wert, ausgeschrieben zu
    // werden. Wörtlich **ist** eine abgeschaltete Mustererkennung ein Loch in
    // der Prüfung: wonach nicht gesucht wurde, kann nicht gefunden worden sein.
    //
    // Der Unterschied zu einer Deckungslücke liegt darin, wer es entschieden
    // hat. Die Analyse hat das Dokument **vollständig gelesen** — sie hat auf
    // ausdrückliche Anweisung nach weniger gesucht. Das ist derselbe Vorgang
    // wie `--patterns iban_de` oder eine angehobene `--min-confidence`, und
    // für die springt der Rückgabewert seit jeher nicht an. Täte er es hier,
    // liefe jeder Lauf mit `--no-patterns` — der übliche Weg für rein
    // manuelles Schwärzen, siehe README — auf Rückgabewert 3, und der Wert
    // wäre für die Fälle wertlos, für die es ihn gibt.
    //
    // Verschwiegen wird deshalb nichts: der Satz steht in der Zusammenfassung,
    // in der Statuszeile der Oberfläche und als eigenes Feld `patterns` im
    // Audit-Log (siehe `crate::audit::PatternRecord`). Nur der Rückgabewert
    // bleibt der Frage „hat das Werkzeug alles gesehen?“ vorbehalten.
    (
        crate::DETECTION_NOTICE,
        "eine ausdrückliche Anweisung des Aufrufenden, keine Eigenschaft dieser \
         Datei — die Analyse hat alles gelesen und auf Geheiß nach weniger \
         gesucht; gesagt wird es in Zusammenfassung, Oberfläche und Audit-Log",
    ),
    // --- Mitteilungen über die Wirkung angeforderter Schwärzungen ---
    //
    // Diese drei sagen etwas über *Regionen, die der Nutzer angefordert hat*,
    // nicht über Text, den die Analyse nicht gesehen hat. Die Seite selbst
    // wurde vollständig durchsucht. Sie stehen außerdem in der Zusammenfassung
    // bereits mit eigenen Zählern („davon wirksam“, „davon wirkungslos“) und
    // gehören damit zu einer anderen Frage als Rückgabewert 3 — nämlich
    // „hat meine Angabe gestimmt?“ statt „hat das Werkzeug alles gesehen?“.
    (
        "haben ein Deck-Rechteck gezeichnet, aber",
        "über einer Grafik der Normalfall; die Seite war durchsucht",
    ),
    (
        "ein leeres Rechteck und konnten nichts entfernen",
        "eine Folge von --padding, also einer Angabe des Nutzers",
    ),
    (
        "liegen auf einer Seite, die es in diesem Dokument nicht gibt",
        "eine Folge der Seitenzählung in der Regionsdatei, also einer Angabe \
         des Nutzers",
    ),
    // Der Nachbar der Zeile darüber, und aus demselben Grund hier: bei
    // `missing_page` stimmt die Seitenzahl nicht, hier die Koordinaten. Beide
    // Male hat die Analyse das Dokument vollständig gelesen und ausgerechnet
    // die Stelle, an der die Angabe nicht trägt, **benannt** — samt Seite.
    //
    // Warum das trotz seiner Nähe zu einem echten Leck kein Rückgabewert 3
    // ist: die 3 beantwortet „hat das Werkzeug alles gesehen?“. Hier lautet
    // die Antwort darauf ja; offen ist „hat meine Angabe gestimmt?“, und das
    // ist eine andere Frage mit einer anderen Abhilfe (Koordinaten prüfen,
    // nicht das Ergebnis von Hand nachlesen). Beide unter eine Zahl zu legen
    // hieße, den Fällen, für die es die 3 gibt — ein Font ohne /ToUnicode, ein
    // ungelesenes XObject —, ihre Unterscheidbarkeit zu nehmen.
    //
    // Verschwiegen wird nichts: der Fall steht als eigene Zahl in der
    // Zusammenfassung („davon wirkungslos … neben der Seite“), als eigenes
    // Feld `off_page` im Audit-Log, als eigener Befund je Region
    // (`EntryEffect::OffPage`) und in der Warnung darüber mit Seitenzahl — und
    // in der Oberfläche mit **denselben Worten** vor dem Export.
    (
        "liegen vollständig neben der Seite, auf der sie stehen sollen",
        "eine Folge der Koordinaten in der Review- oder Regionsdatei, also \
         einer Angabe des Nutzers; die Seite selbst wurde vollständig \
         durchsucht und der Fall wird mit Seitenzahl benannt",
    ),
];

/// Heißt diese Warnung „für einen Teil des Dokuments kann ich nicht einstehen“?
///
/// Siehe den Modulkommentar: alles ist eine Lücke, was nicht in
/// [`NOT_A_COVERAGE_GAP`] steht.
pub fn is_coverage_gap(warning: &str) -> bool {
    !NOT_A_COVERAGE_GAP
        .iter()
        .any(|(marke, _)| warning.contains(marke))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Der Wortlaut, mit dem `redact-pdf` und [`crate::audit::Effects`] ihre
    /// Warnungen heute erzeugen.
    ///
    /// Die Texte sind Kopien — dieses Crate kann sie nicht aufrufen, ohne die
    /// PDFs zu bauen, die sie auslösen. Das tut `redact-cli/tests/incomplete.rs`
    /// für einen Fall stellvertretend, und zwar am ganzen Weg bis zum
    /// Rückgabewert. Kopien in einem Test sind hier vertretbar, weil sie **die
    /// Einordnung** festhalten und nicht die Meldung selbst: läuft der Wortlaut
    /// in `redact-pdf` weg, fällt dieser Test hier nicht auf — aber der dort.
    const WORTLAUT: &[(bool, &str)] = &[
        // ---------------------------------------------- Deckungslücken
        (
            true,
            "Der Erscheinungsstrom einer Annotation (Objekt 12 0) ließ sich nicht \
             dekodieren; sein Text wurde nicht durchsucht und kann deshalb nicht \
             geschwärzt worden sein.",
        ),
        (
            true,
            "Form-XObject „Fm0“ ist tiefer als 8 Ebenen verschachtelt; ab dort wurde \
             nicht weitergelesen. Text in den tieferen Ebenen wurde nicht durchsucht \
             und kann deshalb nicht geschwärzt worden sein.",
        ),
        (
            true,
            "XObject „Fm0“ hat kein bekanntes /Subtype (weder /Form noch /Image); sein \
             Inhalt wurde nicht durchsucht. Steht dort Text, blieb er ungeschwärzt.",
        ),
        (
            true,
            "Form-XObject „Fm0“ ließ sich nicht dekodieren (unbekannter oder defekter \
             Filter); sein Text wurde nicht durchsucht und kann deshalb nicht \
             geschwärzt worden sein.",
        ),
        (
            true,
            "Kachelmuster „P0“ liegt tiefer als 8 Ebenen verschachtelt; sein Text wurde \
             nicht durchsucht und kann deshalb nicht geschwärzt worden sein.",
        ),
        (
            true,
            "Kachelmuster „P0“ enthält Text. Er wird an der Stelle der ersten Kachel \
             gesucht und beim Schwärzen aus dem Muster entfernt — die übrigen Kacheln \
             werden dabei nicht einzeln vermessen. Bitte das Ergebnis dort prüfen.",
        ),
        (
            true,
            "Font „ABCDEF+Arial“ hat kein /ToUnicode; sein Text lässt sich nicht \
             dekodieren. Muster können darin nicht erkannt werden — diese Seite wurde \
             möglicherweise nicht vollständig geschwärzt.",
        ),
        (
            true,
            "Bild /Im0 auf Seite 1 lässt sich nicht dekodieren (Filter: JPXDecode). Die \
             Schwärzung läge nur darüber; die Pixel blieben in der Datei.",
        ),
        (
            true,
            "Bild /Im0 auf Seite 1 ist kein eigenständiges Objekt und kann nicht \
             ersetzt werden.",
        ),
        // ------------------------------------------ keine Deckungslücken
        (
            false,
            "Eine Annotation trägt Text (/Contents oder einen der Schlüssel /RC, /T, \
             /Subj, /TU, /TM), hat aber keinen lesbaren Erscheinungsstrom (/AP). \
             Dieser Text hat keine Glyphen und wird deshalb nicht anteilig \
             geschwärzt; er wird mit den Metadaten als Ganzes entfernt \
             (strip_metadata, in der Verarbeitungskette immer).",
        ),
        (
            false,
            "1 von 3 Seite(n) enthalten Rasterbilder. Geschwärzte Bereiche werden im \
             Bild selbst überschrieben; gelesen wird der Bildinhalt aber nicht — Text \
             *in* einem Bild (Scan, Foto) findet die Analyse ohne OCR nicht.",
        ),
        (
            false,
            "2 Bild(er) überschrieben: die Bildpunkte im Schwärzungsbereich sind \
             wirklich weg. Das Bild wird dafür neu kodiert — außerhalb des Bereichs \
             bleibt jeder Bildpunkt unverändert (verlustfrei), die Datei ist danach \
             aber nicht mehr bitgleich und wird meist deutlich größer (aus JPEG wird \
             ein Flate-Bild).",
        ),
        (
            false,
            "Bild /Im0 steckt in einem Form-XObject, das mehrere Seiten benutzen. Es \
             wurde überschrieben — die Schwärzung wirkt deshalb auch auf die anderen \
             Seiten.",
        ),
        (
            false,
            "Die Eingabedatei besteht aus mehreren inkrementellen Revisionen (/Prev). \
             Frühere Fassungen können Text enthalten, den eine spätere Revision nur \
             überschrieben hat — etwa eine bereits in einem anderen Werkzeug \
             vorgenommene Schwärzung. Die Ausgabe wird als eine einzige Revision ohne \
             Vorgeschichte geschrieben; prüfen Sie das Ergebnis trotzdem.",
        ),
        (
            false,
            "Die Eigenschaftsliste einer Marked-Content-Auszeichnung enthielt neben dem \
             Textspiegel indirekte Verweise (5 0 R). Eine Liste, die inline im Strom \
             steht, darf keine enthalten (PDF 32000-1, 14.6.2); sie sind deshalb mit \
             entfallen. Bitte prüfen, ob die Datei dadurch anders aussieht.",
        ),
        (
            false,
            "1 von 3 Schwärzung(en) haben ein Deck-Rechteck gezeichnet, aber kein \
             einziges Zeichen aus dem Content-Stream entfernt. Wo gar kein Text steht \
             (Grafik, Rasterbild), ist das richtig; treffen die Koordinaten dagegen \
             daneben, bleibt der Text darunter lesbar und per Copy-&-Paste zu holen.",
        ),
        (
            false,
            "1 von 3 Schwärzungen haben nach --padding=-100 ein leeres Rechteck und \
             konnten nichts entfernen. Der Text steht unverändert in der Ausgabe. Ein \
             negatives Padding verkleinert jeden Bereich.",
        ),
        (
            false,
            "1 von 3 Schwärzung(en) liegen auf einer Seite, die es in diesem Dokument \
             nicht gibt (Seite 27; das Dokument hat 3 Seite(n)). Dort wurde nichts \
             entfernt und nichts überdeckt — der Text steht unverändert in der Ausgabe.",
        ),
        (
            false,
            "1 von 3 Schwärzung(en) liegen vollständig neben der Seite, auf der sie \
             stehen sollen (Seite 1). Dort kann kein Zeichen liegen und kein \
             Deck-Rechteck sichtbar werden — der Text der Seite steht unverändert in \
             der Ausgabe. Häufigste Ursache sind Koordinaten aus einer Review- oder \
             Regionsdatei, die zu einem anders großen Blatt gehören.",
        ),
    ];

    #[test]
    fn every_known_warning_is_sorted_the_way_it_is_meant() {
        for (lücke, text) in WORTLAUT {
            assert_eq!(
                is_coverage_gap(text),
                *lücke,
                "falsch einsortiert (erwartet Deckungslücke = {lücke}): {text}"
            );
        }
    }

    /// Die Meldung über abgeschaltete Erkennung wird **hier** erzeugt und
    /// nicht abgeschrieben: beide Sätze aus [`crate::detection_notice`] müssen
    /// unter die Ausnahme fallen, sonst liefe jeder Lauf mit `--no-patterns`
    /// auf Rückgabewert 3.
    #[test]
    fn a_switched_off_detection_is_not_a_coverage_gap() {
        let aus = crate::Config {
            no_patterns: true,
            ..crate::Config::default()
        };
        let einzeln = crate::Config {
            disabled_patterns: vec!["date_de".into()],
            ..crate::Config::default()
        };
        for config in [&aus, &einzeln] {
            let text = crate::detection_notice(config).expect("es ist etwas abgeschaltet");
            assert!(
                !is_coverage_gap(&text),
                "abgeschaltete Erkennung ist eine Anweisung, keine Lücke: {text}"
            );
        }
        // Gegenprobe: ohne Abschaltung gibt es die Meldung gar nicht.
        assert_eq!(crate::detection_notice(&crate::Config::default()), None);
    }

    /// Die Voreinstellung ist „Lücke“ — eine unbekannte Warnung darf nicht
    /// stillschweigend auf Rückgabewert 0 laufen.
    #[test]
    fn an_unknown_warning_counts_as_a_gap() {
        assert!(is_coverage_gap(
            "Eine Warnung, die es zum Zeitpunkt dieses Tests noch nicht gab."
        ));
    }

    /// Jede Ausnahme braucht eine Begründung — eine leere Zeile in der Liste
    /// wäre eine stille Abschaltung.
    #[test]
    fn every_exception_carries_a_reason() {
        for (marke, grund) in NOT_A_COVERAGE_GAP {
            assert!(!marke.trim().is_empty());
            assert!(
                grund.len() > 20,
                "„{marke}“ steht ohne belastbare Begründung in der Ausnahmeliste"
            );
        }
    }
}
