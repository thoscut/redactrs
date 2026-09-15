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
    /// Warnungen heute erzeugen — als **Schablone**, nicht als Abschrift.
    ///
    /// # Warum eine Schablone und keine Kopie
    ///
    /// Hier standen bis zur Fix-Runde 7 ausgeschriebene Kopien, zeichengleich
    /// mit `redact-pdf` und **durch nichts gesichert**: die Gegenprüfung
    /// änderte `100000` in `200000` und kein Test wurde rot. Er konnte es auch
    /// nicht — [`is_coverage_gap`] antwortet für jede nicht gelistete Warnung
    /// `true`, also auch für eine verstümmelte Kopie. Geprüft wurde damit die
    /// Voreinstellung und nicht die Einordnung.
    ///
    /// Jeder Eintrag ist deshalb jetzt:
    ///
    /// 1. **die Zeichenkette, wie sie im Quelltext steht** — mit ihren
    ///    `{}`-Stellen. Die festen Teile dazwischen (die *Marken*) müssen
    ///    wörtlich in `redact-pdf` bzw. [`crate::audit`] vorkommen; läuft der
    ///    Wortlaut dort weg, fällt es **hier** auf.
    /// 2. **die Werte der `{}`-Stellen**, aus denen der Satz gebaut wird, den
    ///    [`is_coverage_gap`] dann einsortiert. Eine Stelle in GROSSBUCHSTABEN
    ///    (`{MAX_FORM_DEPTH}`) ist keine Einsetzung, sondern eine **Konstante**:
    ///    ihr Wert wird aus dem Quelltext gelesen. Damit ist keine Zahl hier
    ///    mehr abgeschrieben.
    ///
    /// `redact-cli/tests/incomplete.rs` prüft einen Fall stellvertretend am
    /// ganzen Weg bis zum Rückgabewert.
    ///
    /// Mutationsnachweis: ein Wort einer Marke geändert → `jede_schablone_
    /// steht_so_in_redact_pdf` rot; `{MAX_MIRROR_FORM_PLACEMENTS}` durch
    /// `100000` ersetzt → derselbe Test rot (die Marke „mehr als 100000
    /// Zuordnungen“ steht so nirgends).
    const WORTLAUT: &[(bool, &str, &[&str])] = &[
        // ---------------------------------------------- Deckungslücken
        (
            true,
            "Unter den Textspiegeln dieser Seite stehen mehr als \
             {MAX_MIRROR_FORM_PLACEMENTS} Zuordnungen zwischen einem Spiegel und \
             einer Formularplatzierung; ab dort wurden die Glyphen den Spiegeln \
             nicht mehr zugeordnet. Der Vergleich zwischen Spiegel und Glyphen ist \
             für die letzten Abschnitte deshalb unvollständig.",
            &[],
        ),
        (
            true,
            "Der Erscheinungsstrom einer Annotation (Objekt {} {}) ließ sich nicht \
             dekodieren; sein Text wurde nicht durchsucht und kann deshalb nicht \
             geschwärzt worden sein.",
            &["12", "0"],
        ),
        (
            true,
            "Form-XObject „{}“ ist tiefer als {MAX_FORM_DEPTH} Ebenen verschachtelt; \
             ab dort wurde nicht weitergelesen. Text in den tieferen Ebenen wurde \
             nicht durchsucht und kann deshalb nicht geschwärzt worden sein.",
            &["Fm0"],
        ),
        (
            true,
            "XObject „{label}“ hat kein bekanntes /Subtype (weder /Form noch \
             /Image); sein Inhalt wurde nicht durchsucht. Steht dort Text, blieb er \
             ungeschwärzt.",
            &["Fm0"],
        ),
        (
            true,
            "Form-XObject „{label}“ ließ sich nicht dekodieren (unbekannter oder \
             defekter Filter); sein Text wurde nicht durchsucht und kann deshalb \
             nicht geschwärzt worden sein.",
            &["Fm0"],
        ),
        (
            true,
            "Kachelmuster „{label}“ liegt tiefer als {MAX_FORM_DEPTH} Ebenen \
             verschachtelt; sein Text wurde nicht durchsucht und kann deshalb nicht \
             geschwärzt worden sein.",
            &["P0"],
        ),
        (
            true,
            "Kachelmuster „{label}“ enthält Text. Er wird an der Stelle der ersten \
             Kachel gesucht und beim Schwärzen aus dem Muster entfernt — die übrigen \
             Kacheln werden dabei nicht einzeln vermessen. Bitte das Ergebnis dort \
             prüfen.",
            &["P0"],
        ),
        (
            true,
            "Font „{name}“ hat kein /ToUnicode; sein Text lässt sich nicht \
             dekodieren. Muster können darin nicht erkannt werden — diese Seite \
             wurde möglicherweise nicht vollständig geschwärzt.",
            &["ABCDEF+Arial"],
        ),
        (
            true,
            "{} lässt sich nicht dekodieren ({reason}). Die Schwärzung läge nur \
             darüber; die Pixel blieben in der Datei.",
            &["Bild /Im0 auf Seite 1", "Filter: JPXDecode"],
        ),
        (
            true,
            "{} ist kein eigenständiges Objekt und kann nicht ersetzt werden.",
            &["Bild /Im0 auf Seite 1"],
        ),
        // ------------------------------------------ keine Deckungslücken
        (
            false,
            "Eine Annotation trägt Text (/Contents oder einen der Schlüssel /RC, /T, \
             /Subj, /TU, /TM), hat aber keinen lesbaren Erscheinungsstrom (/AP). \
             Dieser Text hat keine Glyphen und wird deshalb nicht anteilig \
             geschwärzt; er wird mit den Metadaten als Ganzes entfernt \
             (strip_metadata, in der Verarbeitungskette immer).",
            &[],
        ),
        (
            false,
            "{with_images} von {} Seite(n) enthalten Rasterbilder. Geschwärzte \
             Bereiche werden im Bild selbst überschrieben; gelesen wird der \
             Bildinhalt aber nicht — Text *in* einem Bild (Scan, Foto) findet die \
             Analyse ohne OCR nicht.",
            &["1", "3"],
        ),
        (
            false,
            "{} Bild(er) überschrieben: die Bildpunkte im Schwärzungsbereich sind \
             wirklich weg. Das Bild wird dafür neu kodiert — außerhalb des Bereichs \
             bleibt jeder Bildpunkt unverändert (verlustfrei), die Datei ist danach \
             aber nicht mehr bitgleich und wird meist deutlich größer (aus JPEG wird \
             ein Flate-Bild).",
            &["2"],
        ),
        (
            false,
            "Bild /{} steckt in einem Form-XObject, das mehrere Seiten benutzen. Es \
             wurde überschrieben — die Schwärzung wirkt deshalb auch auf die anderen \
             Seiten.",
            &["Im0"],
        ),
        (
            false,
            "Die Eingabedatei besteht aus mehreren inkrementellen Revisionen \
             (/Prev). Frühere Fassungen können Text enthalten, den eine spätere \
             Revision nur überschrieben hat — etwa eine bereits in einem anderen \
             Werkzeug vorgenommene Schwärzung. Die Ausgabe wird als eine einzige \
             Revision ohne Vorgeschichte geschrieben; prüfen Sie das Ergebnis \
             trotzdem.",
            &[],
        ),
        (
            false,
            "Die Eigenschaftsliste einer Marked-Content-Auszeichnung enthielt neben \
             dem Textspiegel indirekte Verweise ({}). Eine Liste, die inline im \
             Strom steht, darf keine enthalten (PDF 32000-1, 14.6.2); sie sind \
             deshalb mit entfallen. Bitte prüfen, ob die Datei dadurch anders \
             aussieht.",
            &["5 0 R"],
        ),
        (
            false,
            "{} von {total} Schwärzung(en) haben ein Deck-Rechteck gezeichnet, aber \
             kein einziges Zeichen aus dem Content-Stream entfernt. Wo gar kein Text \
             steht (Grafik, Rasterbild), ist das richtig; treffen die Koordinaten \
             dagegen daneben, bleibt der Text darunter lesbar und per \
             Copy-&-Paste zu holen.",
            &["1", "3"],
        ),
        (
            false,
            "{} von {total} Schwärzungen haben nach --padding={} ein leeres Rechteck \
             und konnten nichts entfernen. Der Text steht unverändert in der \
             Ausgabe. Ein negatives Padding verkleinert jeden Bereich.",
            &["1", "3", "-100"],
        ),
        (
            false,
            "{} von {total} Schwärzung(en) liegen auf einer Seite, die es in diesem \
             Dokument nicht gibt (Seite {list}; das Dokument hat {} Seite(n)). Dort \
             wurde nichts entfernt und nichts überdeckt — der Text steht unverändert \
             in der Ausgabe. Häufigste Ursache ist die Zählweise: in JSON ist die \
             erste Seite „page“: 0, die letzte also {}.",
            &["1", "3", "27", "3", "2"],
        ),
        (
            false,
            "{} von {total} Schwärzung(en) liegen vollständig neben der Seite, auf \
             der sie stehen sollen (Seite {list}). Dort kann kein Zeichen liegen und \
             kein Deck-Rechteck sichtbar werden — der Text der Seite steht \
             unverändert in der Ausgabe. Häufigste Ursache sind Koordinaten aus \
             einer Review- oder Regionsdatei, die zu einem anders großen Blatt \
             gehören.",
            &["1", "3", "1"],
        ),
    ];

    /// Die Wurzel des Repositorys.
    fn wurzel() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// Der Quelltext, der die Warnungen **erzeugt**: `redact-pdf` und die
    /// übrige Verarbeitungskette — ohne diese Datei, die sie nur einsortiert.
    ///
    /// Zeilenfortsetzungen im Zeichenkettenliteral (`\` am Zeilenende) werden
    /// aufgelöst, danach wird jeder Leerraum zu einem Leerzeichen: so steht
    /// der Wortlaut da, wie ihn `format!` erzeugt.
    fn quelltext() -> String {
        fn sammle(verzeichnis: &std::path::Path, aus: &mut String) {
            let Ok(eintraege) = std::fs::read_dir(verzeichnis) else {
                return;
            };
            for eintrag in eintraege.flatten() {
                let pfad = eintrag.path();
                if pfad.is_dir() {
                    sammle(&pfad, aus);
                } else if pfad.extension().is_some_and(|e| e == "rs")
                    && pfad.file_name().is_some_and(|n| n != "coverage.rs")
                {
                    aus.push_str(&std::fs::read_to_string(&pfad).unwrap_or_default());
                    aus.push('\n');
                }
            }
        }
        let mut roh = String::new();
        sammle(&wurzel().join("crates/redact-pdf/src"), &mut roh);
        sammle(&wurzel().join("crates/redact-pipeline/src"), &mut roh);
        assert!(
            roh.len() > 100_000,
            "der Quelltext von redact-pdf wurde nicht gefunden ({} Byte)",
            roh.len()
        );
        // `… \`<Zeilenumbruch><Einrückung>` ist eine Fortsetzung, kein Leerraum.
        let mut ohne_fortsetzung = String::with_capacity(roh.len());
        let mut zeichen = roh.chars().peekable();
        while let Some(c) = zeichen.next() {
            if c == '\\' && zeichen.peek() == Some(&'\n') {
                zeichen.next();
                while zeichen.peek().is_some_and(|z| *z == ' ' || *z == '\t') {
                    zeichen.next();
                }
                continue;
            }
            ohne_fortsetzung.push(c);
        }
        ohne_fortsetzung
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Eine Schablone zerlegt: die festen **Marken** und die Namen der
    /// `{}`-Stellen dazwischen.
    fn zerlege(schablone: &str) -> (Vec<String>, Vec<String>) {
        let geglaettet = schablone.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut marken = Vec::new();
        let mut stellen = Vec::new();
        let mut rest = geglaettet.as_str();
        while let Some(auf) = rest.find('{') {
            let zu = auf
                + rest[auf..]
                    .find('}')
                    .expect("`{` ohne `}` in der Schablone");
            marken.push(rest[..auf].to_string());
            stellen.push(rest[auf + 1..zu].to_string());
            rest = &rest[zu + 1..];
        }
        marken.push(rest.to_string());
        (marken, stellen)
    }

    /// Ist diese `{}`-Stelle eine **Konstante** (GROSSBUCHSTABEN) und keine
    /// Einsetzung? `{}` — die leere Stelle — ist keine.
    fn ist_konstante(stelle: &str) -> bool {
        !stelle.is_empty() && stelle.chars().all(|c| c.is_ascii_uppercase() || c == '_')
    }

    /// Der Wert einer Konstanten aus dem Quelltext: `const NAME: usize = 8;`
    /// oder `= 100_000;`.
    fn konstante(quelle: &str, name: &str) -> String {
        let muster = format!("const {name}: usize = ");
        let ab = quelle
            .find(&muster)
            .unwrap_or_else(|| panic!("`{muster}…` steht nicht im Quelltext von redact-pdf"))
            + muster.len();
        let wert: String = quelle[ab..]
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '_')
            .filter(|c| *c != '_')
            .collect();
        assert!(!wert.is_empty(), "`{muster}` steht ohne Zahl im Quelltext");
        wert
    }

    /// **Die Bindung.** Jede Marke jeder Schablone steht wörtlich im
    /// Quelltext, der die Warnung erzeugt.
    ///
    /// Damit ist die Kopie keine mehr: wer die Meldung in `redact-pdf`
    /// umschreibt, wird hier rot — und wer sie hier verstümmelt, ebenso.
    #[test]
    fn jede_schablone_steht_so_in_redact_pdf() {
        let quelle = quelltext();
        let mut fehlend: Vec<String> = Vec::new();
        for (_, schablone, _) in WORTLAUT {
            let (marken, _) = zerlege(schablone);
            assert!(
                marken.iter().any(|m| m.trim().chars().count() >= 30),
                "Schablone ohne belastbare Marke: {schablone}"
            );
            for marke in &marken {
                let marke = marke.trim();
                // Ganz kurze Stücke zwischen zwei `{}` binden nichts und
                // kämen überall vor.
                if marke.chars().count() < 10 {
                    continue;
                }
                if !quelle.contains(marke) {
                    fehlend.push(format!("„{marke}“"));
                }
            }
        }
        // Die Ausnahmeliste besteht selbst aus Marken — auch sie muss den
        // Wortlaut treffen, sonst fällt eine Ausnahme still aus.
        for (marke, _) in NOT_A_COVERAGE_GAP {
            // Diese eine wird in *dieser* Datei erzeugt (`crate::DETECTION_NOTICE`).
            if *marke == crate::DETECTION_NOTICE {
                continue;
            }
            if !quelle.contains(*marke) {
                fehlend.push(format!("Ausnahme „{marke}“"));
            }
        }
        assert!(
            fehlend.is_empty(),
            "{} Marke(n) stehen so nicht mehr im Quelltext von redact-pdf — die \
             Einordnung hier gilt einem Wortlaut, den es nicht gibt:\n{}",
            fehlend.len(),
            fehlend.join("\n")
        );
    }

    /// Jede Warnung ist so einsortiert, wie sie gemeint ist — gebaut aus der
    /// Schablone, mit den Konstanten aus dem Quelltext.
    #[test]
    fn every_known_warning_is_sorted_the_way_it_is_meant() {
        let quelle = quelltext();
        for (lücke, schablone, werte) in WORTLAUT {
            let (marken, stellen) = zerlege(schablone);
            let frei = stellen.iter().filter(|s| !ist_konstante(s)).count();
            assert_eq!(
                frei,
                werte.len(),
                "{frei} einzusetzende Stelle(n), aber {} Wert(e): {schablone}",
                werte.len()
            );
            let mut text = String::new();
            let mut naechster = 0usize;
            for (i, marke) in marken.iter().enumerate() {
                text.push_str(marke);
                let Some(stelle) = stellen.get(i) else {
                    continue;
                };
                if ist_konstante(stelle) {
                    text.push_str(&konstante(&quelle, stelle));
                } else {
                    text.push_str(werte[naechster]);
                    naechster += 1;
                }
            }
            assert_eq!(
                is_coverage_gap(&text),
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
