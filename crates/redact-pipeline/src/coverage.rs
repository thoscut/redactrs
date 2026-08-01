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
            "Eine Annotation trägt Text in /Contents, hat aber keinen lesbaren \
             Erscheinungsstrom (/AP). Dieser Text wurde nicht durchsucht und kann \
             deshalb nicht geschwärzt worden sein.",
        ),
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
