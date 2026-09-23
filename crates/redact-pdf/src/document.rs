//! Laden, Prüfen und Speichern von PDF-Dateien.
//!
//! Bewusst streng: verschlüsselte oder strukturell kaputte Dateien werden
//! abgelehnt statt repariert. Eine „reparierte“ Datei könnte Inhalte enthalten,
//! die der Analyse entgehen — und damit ungeschwärzt durchrutschen.
//!
//! ## Eingaben sind nicht vertrauenswürdig
//!
//! Ein PDF kommt von dem, dessen Daten wir schwärzen sollen — also genau von
//! der Seite, die ein Interesse daran haben kann, dass das Werkzeug abstürzt
//! oder die Maschine lahmlegt. Deshalb läuft vor dem Parsen eine
//! [`prescan`]-Vorprüfung über die Rohbytes, und alles, was geschrieben wird,
//! geht durch den einen Schreibpfad [`write_file`].

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use lopdf::{Document, Object, ObjectId};
use redact_core::{Rect, RedactError, Result};

// ---------------------------------------------------------------------------
// Grenzen für nicht vertrauenswürdige Eingaben
// ---------------------------------------------------------------------------

/// Obergrenzen, mit denen fremde PDFs gelesen werden.
///
/// Die Werte sind bewusst großzügig gegenüber echten Dokumenten und trotzdem
/// weit unterhalb dessen, was die Maschine in die Knie zwingt. Wer sie ändert,
/// sollte die Messungen in `SECURITY.md` kennen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximale Verschachtelungstiefe von `[` bzw. `<<` in den Rohbytes.
    ///
    /// Historisch war das die Notbremse gegen RUSTSEC-2026-0187: `lopdf` 0.34
    /// parste unbegrenzt rekursiv, lief ab einigen hundert Ebenen (Debug) bzw.
    /// einigen tausend (Release) über den Stack und beendete den Prozess mit
    /// SIGABRT. Seit `lopdf` 0.42 begrenzt die Bibliothek sich selbst
    /// (`lopdf::reader::MAX_NESTING_DEPTH`, derzeit 100) und stürzt nicht
    /// mehr ab.
    ///
    /// Die Grenze bleibt trotzdem, und sie liegt jetzt **gleichauf** mit der
    /// von `lopdf`. Der Grund ist ein anderer als früher: `lopdf` parst
    /// nachsichtig. Ein Objekt, das seine Tiefengrenze reißt, wird nicht
    /// gemeldet, sondern **stillschweigend weggelassen** — das Dokument lädt,
    /// eine Referenz darauf zeigt danach ins Leere, und die Ausgabe wäre
    /// unauffällig kaputt. Für ein Werkzeug, das Kontoauszüge verarbeitet, ist
    /// eine Ablehnung mit Meldung die richtige Antwort darauf. Gemessen:
    /// `lopdf` 0.42 nimmt Tiefe 100 an und lässt das Objekt ab Tiefe 101
    /// fallen; genau dort greift auch diese Prüfung.
    pub max_nesting_depth: usize,
    /// Summe der **entpackten** Bytes über alle Streams der Datei.
    pub max_decompressed_bytes: u64,
    /// Alles, woraus PDF-**Syntax** wird — zwei Klassen:
    ///
    /// 1. die Streams, die geparst werden (Objekt- und Content-Streams);
    /// 2. der **Rumpf der Datei selbst**: Objektköpfe, Dictionaries, Arrays,
    ///    Querverweistabelle, Trailer — alles, was nicht Stream-Nutzlast ist.
    ///
    /// Beides ist der teure Teil. Aus einem Byte Content-Stream werden im
    /// Speicher rund 60 bis 100 Byte `lopdf::content::Operation`; aus einem
    /// Dictionary-Eintrag von 5 Byte werden **197 Byte** `lopdf::Object` samt
    /// Schlüssel und Tabellenreserve. Deshalb hat diese Klasse ein eigenes,
    /// sehr viel engeres Budget als der Rest (Bilder, Schriften, eingebettete
    /// Dateien), der nur gespeichert wird.
    ///
    /// **Klasse 2 fehlte früher.** Gezählt wurden nur Streams — eine
    /// unkomprimierte Datei aus lauter Dictionaries sah das Budget also gar
    /// nicht. Gemessen: 44,3 MB Quelle ergaben ein `Document` von 1 804 MB
    /// (Faktor 40,7) und einen Lauf mit 3 756 MB Spitzenspeicher und
    /// Rückgabewert 0. Dieselbe Bombe in einem Objekt-Stream wurde seit jeher
    /// abgelehnt; der Unterschied war allein, ob sie komprimiert war. Die
    /// Begründung im Langen steht in
    /// `redact-pdf/tests/rumpf_im_parse_budget.rs`.
    ///
    /// **Was dieses Budget allein nicht kann.** Es zählt *Dateibytes* und muss
    /// dafür einen Aufblähfaktor unterstellen. Der ist keine Konstante — er
    /// hängt an der **Form** der Syntax, nicht an ihrer Länge, und schwankt
    /// gemessen um den Faktor 274. Deshalb steht neben dieser Buchhaltung eine
    /// zweite, die den Faktor rechnet, statt ihn zu unterstellen; sie hängt am
    /// selben Schalter. Siehe [`OBJEKTSPEICHER_JE_BUDGETBYTE`].
    pub max_parsed_bytes: u64,
}

/// Wie viel **Objektspeicher** je Byte Parse-Budget zugestanden wird.
///
/// ## Das ist die Zahl, die vorher stillschweigend unterstellt wurde
///
/// [`Limits::max_parsed_bytes`] zählt Dateibytes und unterstellt damit einen
/// begrenzten Aufblähfaktor — geprüft hat ihn niemand. Genau das war die
/// Lücke: eine Datei aus reinem Rumpf, 16 761 999 Byte und damit **innerhalb**
/// des 16-MiB-Budgets, gefüllt mit leeren Arrays `[[][][]…]`, lief mit
/// Rückgabewert 0 durch und belegte dabei **5 739 MB** — über mehrere Läufe
/// 11 bis 26 s, die Wanduhr schwankt mit der Fremdlast, der Speicher nicht.
/// Dieselbe Bauart mit 21,4 MB wurde abgelehnt: die Decke griff, sie hing nur
/// an der falschen Größe. Jetzt wird dieselbe Datei bei 22 MB abgelehnt, in
/// unter 0,2 s.
///
/// Hier steht der Faktor jetzt ausdrücklich da und wird geprüft. Gemessen mit
/// zählendem Allokator an 4-MB-Dateien gleicher Größe (`Document` nach
/// `load_mem`, geteilt durch die Dateigröße):
///
/// | Form | `Document` | Byte je Dateibyte |
/// |---|---:|---:|
/// | `[[][][]…]` — leere Arrays | 1 204,7 MB | **301,6** |
/// | `[/a/a/a…]` — Namen | 300,2 MB | 75,2 |
/// | `[0 0 0…]` — Zahlen | 292,6 MB | 73,3 |
/// | `<</ab 0 …>>` — Dictionary-Einträge | 176,1 MB | 44,1 |
/// | `[<<>><<>>…]` — leere Dictionaries | 146,7 MB | 36,7 |
/// | `[1 0 R …]` — Verweise | 146,0 MB | 36,5 |
/// | eine 800-Byte-Zeichenkette | 4,6 MB | 1,1 |
///
/// 274-facher Unterschied bei identischer Dateigröße. Ein Faktor ist also
/// nichts, was sich aus der Dateigröße ablesen ließe — er hängt an der
/// **Form**. Deshalb wird er nicht geschätzt, sondern beim Lauf über die
/// Rohbytes **gerechnet**: [`OBJEKT_BYTES`] je Objekt, [`ARRAY_BYTES`] je
/// Array (siehe [`wortanfang`]). Dass diese Rechnung nie unter dem wirklich
/// belegten Speicher liegt — im engsten Fall 2 % darüber —, hält
/// `redact-pdf/tests/za_objektspeicher_gerechnet.rs` fest.
///
/// ## Warum 60
///
/// Die Decke ist `max_parsed_bytes × 60`, bei der Vorgabe also **960 MB**
/// gerechneter Objektspeicher. Sie liegt zwischen dem Teuersten, was dieser
/// Baum ausdrücklich durchlassen *will*, und dem Billigsten, was er ablehnen
/// *muss*. Alles gemessen; die Spitzenwerte sind `ru_maxrss` eines
/// Release-Laufs `redact-rs DATEI -o … -f -q`, MB heißt hier wie überall
/// 1024², und „gerechnet“ ist der Wert dieser Buchhaltung:
///
/// | Datei (Byte) | gerechnet | Spitze vorher | vorher | nachher |
/// |---|---:|---:|---|---|
/// | 13 500 654, ein Seiteninhalt mit 1,5 Mio. Operationen | 915 MB | 2 178 MB | rc 0 | **rc 0** |
/// | 16 660 959, `[<<>><<>>…]` | Byte-Budget bindet | 1 013 MB | rc 0 | **rc 0** |
/// | 16 660 891, `<</ab 0 …>>` | 988 MB | 1 383 MB | rc 0 | rc 1 |
/// | 16 661 481, `[1 0 R …]` | 1 241 MB | 711 MB | rc 0 | rc 1 |
/// | 16 660 959, `[0 0 0 …]` | 1 241 MB | 1 920 MB | rc 0 | rc 1 |
/// | 16 660 959, `[/a/a …]` | 1 241 MB | 2 404 MB | rc 0 | rc 1 |
/// | 16 761 999, `[[][][]…]` | 4 896 MB | **5 739 MB** | rc 0 | rc 1, 22 MB |
///
/// **Nach unten** hält der lange, ehrliche Seiteninhalt die Decke fest: der
/// Test `resource_bombs::a_long_honest_content_stream_is_not_mistaken_for_a_fanout`
/// verlangt ausdrücklich, dass ein einzelner Strom mit sehr vielen Operationen
/// durchläuft. Er kostet gerechnet 915 MB — jede kleinere Decke bräche ihn.
/// **Nach oben** hält ihn die Dictionary-Bombe fest, die mit 988 MB knapp
/// darüber liegt.
///
/// Die Zeile mit 711 MB zeigt den Preis der Vorsicht: `1 0 R` ist ein Objekt
/// und zählt drei Wörter, deshalb wird diese Datei abgelehnt, obwohl sie
/// gemessen unter der Decke bliebe. Ein Abzug für das `R` ließe sich mit
/// `R R R R …` dazu missbrauchen, echte Objekte wegzurechnen; Wörter dürfen
/// nur addiert werden, und eine Datei aus 2,7 Millionen Verweisen ist kein
/// Dokument.
///
/// Der Faktor gilt gegen das **Budget**, nicht gegen die Dateigröße: eine
/// Datei unter dem Budget darf einen höheren Eigenfaktor haben — der ehrliche
/// Seiteninhalt oben hat 68 — und kommt trotzdem durch.
///
/// Für gewöhnliche Dokumente ändert sich damit nichts: bei allen sechs
/// Gegenproben (`--write-demo`, 500 und 2 000 Textseiten, 200 Scanseiten mit
/// Bildern, Querverweis-Strom, 2 937 Textseiten) bindet weiterhin das
/// **Byte-Budget** und nicht diese Decke — nachgemessen in
/// `za_objektdecke::bei_gewoehnlichen_dokumenten_bindet_weiter_das_byte_budget`.
///
/// ## Wo es doch enger wird, und was das kostet
///
/// Ein **sehr dichter Seiteninhalt** stößt jetzt vor dem Byte-Budget an. Bei
/// der Dichte von `0 0 0 rg\n` (vier Wörter je neun Byte) liegt die Grenze
/// rechnerisch bei 14 155 776 Byte Strom; gemessen läuft eine Datei mit
/// 13,5 MB Strom durch und eine mit 14,0 MB nicht mehr — vorher lief die mit
/// 14,6 MB bei Rückgabewert 0 auf 2 469 MB. Das ist der Preis dafür, dass die
/// 16-MB-Datei aus leeren Arrays nicht mehr 5 739 MB belegt: beide sind
/// Syntax, und eine Decke, die nur die eine Form kennt, wäre wieder eine über
/// der falschen Menge. Wer solche Dateien wirklich verarbeiten muss, hebt
/// `--max-parsed-mb` an — mit 24 läuft die 14,6-MB-Datei wieder durch.
///
/// ## Woran die Decke damit hängt
///
/// An `--max-parsed-mb`, und zwar in beiden Einheiten. Das ist Absicht: der
/// Schalter ist die eine Stelle, an der jemand sagt „so viel Syntax lasse ich
/// zu“, und er soll nicht in der einen Einheit wirken und in der anderen nicht.
/// Eine zweite Schraube wäre eine zweite Stelle, an der die Zahlen
/// auseinanderlaufen — und für ein wirklich so gebautes Dokument gäbe es sonst
/// gar keinen Weg mehr.
///
/// ## Was die Zahl **nicht** ist
///
/// Kein Spitzenbedarf eines Laufs. Gerechnet wird, was aus der Syntax an
/// `Object`-Werten entsteht, und sonst nichts — die Tabelle oben zeigt den
/// Abstand: beim Seiteninhalt liegt der gemessene Spitzenwert beim 2,4-fachen
/// des gerechneten, beim Rumpf beim 1,2- bis 1,6-fachen. Es ist auch keine
/// Messung: die Summe hängt an der Datei, nicht an Allokator oder Zielsystem.
///
/// Die Messreihe steht in `redact-pdf/tests/za_objektdecke.rs`.
const OBJEKTSPEICHER_JE_BUDGETBYTE: u64 = 60;

/// Was ein `lopdf::Object` im geladenen Dokument kostet — der Betrag, mit dem
/// [`Prescan::walk`] jedes Wort der Syntax verbucht.
///
/// Gemessen mit zählendem Allokator: **153,6** Byte je Zahl, Name, Verweis
/// oder leerem Dictionary in einem Array; ein Dictionary-Eintrag kostet 231
/// Byte und besteht aus zwei Wörtern (Schlüssel und Wert), bekommt also
/// 2 × 160 verbucht. 160 liegt über beidem.
pub const OBJEKT_BYTES: u64 = 160;

/// Was ein **Array** zusätzlich kostet.
///
/// Ein leeres Array `[]` ist zwei Byte in der Datei und kostet gemessen
/// **632,4** Byte: sein eigener `Object`-Platz plus die Anforderung des `Vec`,
/// den `lopdf` dafür anlegt. Deshalb zählt eine öffnende `[` wie vier Objekte.
/// Genau diese Form war die Bombe, die durch das Byte-Budget lief. Beide
/// Beträge sind in `za_objektspeicher_gerechnet.rs` an die Messung gebunden.
pub const ARRAY_BYTES: u64 = 640;

impl Default for Limits {
    fn default() -> Self {
        Self {
            // Gleichauf mit `lopdf::reader::MAX_NESTING_DEPTH` (dort 100, aber
            // nicht öffentlich re-exportiert, deshalb hier als Zahl). Der Test
            // `the_depth_limit_is_exactly_what_lopdf_still_parses` misst die
            // Grenze am Verhalten der Bibliothek nach; zieht ein lopdf-Update
            // sie um, fällt der Test auf.
            max_nesting_depth: 100,
            max_decompressed_bytes: 1024 * 1024 * 1024,
            max_parsed_bytes: 16 * 1024 * 1024,
        }
    }
}

/// Lädt ein PDF von der Platte und prüft es.
pub fn load(path: &Path) -> Result<Document> {
    load_with_limits(path, &Limits::default())
}

/// Wie [`load`], aber mit eigenen Grenzen.
pub fn load_with_limits(path: &Path, limits: &Limits) -> Result<Document> {
    let bytes = std::fs::read(path)?;
    load_from_bytes_with_limits(&bytes, limits).map_err(|e| match e {
        RedactError::Pdf(msg) => RedactError::Pdf(format!("{}: {msg}", path.display())),
        other => other,
    })
}

/// Lädt ein PDF aus dem Speicher (es werden keine temporären Dateien angelegt).
pub fn load_from_bytes(bytes: &[u8]) -> Result<Document> {
    load_from_bytes_with_limits(bytes, &Limits::default())
}

/// Wie [`load_from_bytes`], aber mit eigenen Grenzen.
pub fn load_from_bytes_with_limits(bytes: &[u8], limits: &Limits) -> Result<Document> {
    if !bytes.starts_with(b"%PDF-") {
        // Manche Dateien haben ein paar Bytes Vorspann — das ist zulässig,
        // aber der Header muss in den ersten 1024 Bytes auftauchen.
        let head = &bytes[..bytes.len().min(1024)];
        if !head.windows(5).any(|w| w == b"%PDF-") {
            return Err(RedactError::Pdf(
                "keine PDF-Datei (Header %PDF- fehlt)".into(),
            ));
        }
    }

    // Vorprüfung der Rohbytes — muss *vor* `load_mem` laufen: was dort
    // durchfällt, soll den Parser gar nicht erst erreichen.
    prescan(bytes, limits)?;

    let mut doc = Document::load_mem(bytes)
        .map_err(|e| RedactError::Pdf(format!("Datei nicht lesbar: {e}")))?;
    restore_revision_markers(bytes, &mut doc);

    validate(&doc)?;
    Ok(doc)
}

// ---------------------------------------------------------------------------
// Vorprüfung der Rohbytes
// ---------------------------------------------------------------------------

/// Rohgröße, bis zu der ein Stream mit einem Nicht-Flate-Filter ausgepackt
/// wird. Darüber lehnen wir ab, statt einem fremden Dekoder ein unbegrenztes
/// Speicherbudget zu geben.
const MAX_LEGACY_STREAM_BYTES: usize = 16 * 1024 * 1024;

/// Anteil nicht druckbarer Bytes, ab dem eine Nutzlast als Binärdaten gilt.
const BINARY_RATIO: f64 = 0.10;

/// Kann dieses Byte in PDF-**Syntax** außerhalb einer Zeichenkette vorkommen?
///
/// Zwischenraum (einschließlich NUL, das PDF ausdrücklich dazuzählt) und
/// druckbares ASCII — mehr ist Syntax nicht. Steuerzeichen und Bytes über 126
/// kommen nur in Zeichenketten, in Namen (dort als `#xx` oder, bei `lopdf`,
/// auch roh) und in Stream-Nutzlast vor; Zeichenketten und Nutzlast überspringt
/// [`Prescan::walk`] ohnehin, Namen behandelt es eigens.
///
/// Fast dieselbe Menge zählt [`looks_binary`] aus — dort gilt NUL allerdings
/// als Binärzeichen. Das ist Absicht: eine Nutzlast voller Nullbytes ist keine
/// Syntax, ein einzelnes NUL zwischen zwei Klammern aber sehr wohl.
fn syntax_byte(b: u8) -> bool {
    matches!(b, 0 | 9 | 10 | 12 | 13 | 32..=126)
}

/// Wie viele Bytes vom Anfang eines Streams für die Entscheidung
/// „Nutzlast oder Syntax?“ betrachtet werden.
///
/// Dieselbe Zahl dient als Größe der Vorprobe in [`Prescan::account`]: mehr
/// auszupacken, nur um zu klassifizieren, wäre genau die Speicheranforderung,
/// die diese Vorprüfung verhindern soll.
const BINARY_SAMPLE_BYTES: u64 = 64 * 1024;

/// Tiefengrenze für Nutzlasten, die wie Binärdaten aussehen.
///
/// Auch sie werden gezählt — sonst genügte es, einen Content-Stream mit
/// Rauschen zu spicken, um die Prüfung zu umgehen.
///
/// ## Was dort gezählt wird, und warum die reine Klammertiefe es nicht war
///
/// Bis v0.6.0 zählte hier **jedes** `[`-Byte. Das war keine Messung von
/// Verschachtelung, sondern eine Irrfahrt: `]` senkt den Zähler nur bis null
/// (`saturating_sub`), also läuft er in Rauschen nach oben davon, und sein
/// Höchststand wächst mit der **Länge** der Nutzlast statt mit ihrer Struktur.
/// Die Grenze war damit in Wahrheit eine Größengrenze — und eine, die nach
/// Bytemustern ohne jede Bedeutung mal zuschlug und mal nicht.
///
/// Gemessen an dem Fall, der es aufdeckte: ein gewöhnlicher **Querverweis-
/// Strom** (`/W [1 4 2]`, wie `lopdf` selbst ihn ab PDF 1.5 schreibt) besteht
/// aus 7-Byte-Einträgen, in denen `[`- und `]`-Bytes rein zufällig vorkommen.
/// Von 128 Dateigrößen zwischen 0,25 MB und 32 MB wurden abgelehnt:
///
/// | Einträge | Nutzlast | vorher | nachher |
/// |---------:|---------:|-------:|--------:|
/// |   20 000 |  140 kB  |   0    |    0    |
/// |   50 000 |  350 kB  |   1    |    0    |
/// |  100 000 |  700 kB  |  12    |    0    |
/// |  200 000 |  1,4 MB  |  35    |    0    |
///
/// Ob eine gewöhnliche Datei durchkam, hing also daran, wo ihre Objekte
/// zufällig lagen. Das ist ein Verfügbarkeitsfehler, kein Schutz.
///
/// Jetzt misst die Zählung in Nutzlast nur noch **zusammenhängende Syntax**:
/// der Zähler fängt bei jedem Byte neu an, das in PDF-Syntax gar nicht
/// vorkommt ([`syntax_byte`]) und auch nicht in einem Namen steht (dort lässt
/// `lopdf` rohe Bytes über 126 zu). Mehr braucht es nicht, und mehr steht
/// deshalb auch nicht da — eine Verschachtelung, die `lopdf` wirklich liest,
/// ist ein zusammenhängender Lauf gültiger Syntax: nach jedem `[` muss ein
/// Objekt folgen, sonst bricht der Parser ab und verschachtelt nichts.
///
/// Belegt in `z8_verschachtelung_in_nutzlast`: sieben Tarnungen (Rauschen
/// davor, dahinter, Zahlen, Namen aus hohen Bytes, Nullbytes, Dictionaries)
/// werden weiterhin abgelehnt; 6,2 MB Zufall, druckbares Rauschen und
/// 560 gewöhnliche Querverweis-Ströme nicht mehr.
///
/// Dass dieser Wert über [`Limits::max_nesting_depth`] liegt, ist kein
/// Versehen, aber es hat einen Preis, und der ist gemessen: ein
/// Seiteninhalt mit Tiefe 150 und ein paar tausend Nullbytes am Ende sieht
/// nach Nutzlast aus, kommt hier also durch — und `lopdf` liest ihn trotzdem
/// als Seiteninhalt und lässt ihn fallen, weil seine eigene Grenze bei 100
/// liegt. Abstürzen kann es dabei seit 0.42 nicht mehr; herauskommen würde
/// aber eine Seite, deren Text nie jemand gesehen hat. Genau diesen Fall
/// fängt [`crate::content::scan_page`] ab: dort ist ein Content-Stream ohne
/// eine einzige Operation ein **Fehler**, keine Warnung. Der Test dazu ist
/// `resource_bombs::a_page_whose_content_cannot_be_decoded_ends_the_run`.
const MAX_BINARY_NESTING_DEPTH: usize = 256;

/// Prüft die Rohbytes einer Datei, *bevor* `lopdf` sie zu sehen bekommt.
///
/// Zwei Dinge werden gemessen:
///
/// 1. **Verschachtelungstiefe.** Bis `lopdf` 0.34 war das die Notbremse gegen
///    RUSTSEC-2026-0187: ein PDF mit 200 000 offenen `[` beendete den Prozess
///    mit SIGABRT, bevor irgendein Fehlerwert entstehen konnte. Seit 0.42
///    begrenzt `lopdf` seine Rekursion selbst, und der Absturz ist weg.
///    Geblieben ist ein anderer Grund: `lopdf` meldet ein zu tiefes Objekt
///    nicht, sondern lässt es **stillschweigend weg**. Diese Prüfung macht
///    daraus eine Ablehnung mit Meldung — siehe [`Limits::max_nesting_depth`].
/// 2. **Entpackte Gesamtgröße.** Ein 400-kB-PDF, dessen Content-Stream sich
///    auf 200 MB aufbläht, belegt beim Parsen zweistellige Gigabytes. Dagegen
///    hat `lopdf` nach wie vor nichts — diese Buchhaltung ist die einzige
///    Grenze, und sie ist der Grund, warum die Vorprüfung bleibt.
///
/// Untersucht werden auch die *ausgepackten* Streams — beides, Verschachtelung
/// wie Größe, lässt sich sonst trivial in einem komprimierten Objekt- oder
/// Content-Stream verstecken.
pub fn prescan(bytes: &[u8], limits: &Limits) -> Result<()> {
    let mut scan = Prescan {
        limits,
        packed: 0,
        decompressed: 0,
        parsed: 0,
        objects: 0,
    };
    scan.walk(bytes, true, false)
}

/// Beginnt an `i` ein **Wort** der PDF-Syntax?
///
/// Ein Wort ist alles, woraus `lopdf` einen `Object`-Wert macht und was nicht
/// schon an seiner öffnenden Klammer erkannt wird: eine Zahl, ein Name, ein
/// Schlüsselwort (`true`, `false`, `null`, `R`, ein Operator im Seiteninhalt).
/// Klammern zählt [`Prescan::walk`] selbst, an `[`, `<<`, `(` und `<`.
///
/// **Die Zählung darf nach oben abweichen, nie nach unten.** Deshalb hier zwei
/// bewusste Ungenauigkeiten, beide nach oben: ein Verweis `1 0 R` ist *ein*
/// Objekt und zählt drei Wörter, und ein Dictionary-Schlüssel ist gar kein
/// Objekt (`lopdf` legt ihn als `Vec<u8>` ab) und zählt eins. Wer die Zählung
/// dichter an die Wahrheit bringen will, muss aufpassen: ein Abzug für `R`
/// ließe sich mit `R R R R …` dazu missbrauchen, echte Objekte
/// wegzurechnen — Wörter dürfen nur addiert werden.
fn wortanfang(bytes: &[u8], i: usize) -> bool {
    let b = bytes[i];
    if b == b'/' {
        // Der Name selbst wird beim ersten Byte seines Rumpfes gezählt (dessen
        // Vorgänger ist dieses `/`, also ein Trennzeichen). Nur der **leere**
        // Name hat keinen Rumpf — er ist trotzdem ein Objekt und kostet ein
        // Byte in der Datei.
        return match bytes.get(i + 1) {
            Some(&n) => is_whitespace(n) || is_delimiter(n),
            None => true,
        };
    }
    if is_whitespace(b) || is_delimiter(b) {
        return false;
    }
    i == 0 || is_whitespace(bytes[i - 1]) || is_delimiter(bytes[i - 1])
}

fn check_depth(depth: usize, limit: usize) -> Result<()> {
    if depth > limit {
        return Err(RedactError::Pdf(format!(
            "Verschachtelungstiefe über {limit} — die Datei wird abgelehnt. \
             So tief verschachtelte Objektstrukturen liest der PDF-Parser nicht \
             mehr vollständig ein; er ließe das betroffene Objekt kommentarlos \
             weg. Eine solche Datei ist kein normales Dokument."
        )));
    }
    Ok(())
}

struct Prescan<'a> {
    limits: &'a Limits,
    /// Die **gepackte** Größe derselben Streams, die `decompressed` zählt —
    /// so, wie sie in der Datei stehen.
    ///
    /// Sie entscheidet nichts, sie erklärt nur: aus beiden Zahlen ergibt sich,
    /// ob die Datei sich beim Öffnen wirklich vervielfacht
    /// ([`Prescan::charge`]).
    packed: u64,
    decompressed: u64,
    parsed: u64,
    objects: u64,
}

impl Prescan<'_> {
    /// Läuft über einen Byte-Bereich und zählt die Klammertiefe.
    ///
    /// Zeichenketten, Kommentare und (in Content-Streams) eingebettete Bilder
    /// werden übersprungen — dort steht Nutzlast, keine Struktur. `streams`
    /// steuert, ob `stream … endstream` als Nutzlast behandelt wird; das gilt
    /// nur für die Datei selbst, nicht für bereits ausgepackte Streams.
    ///
    /// `binary` sagt, ob dieser Bereich wie Nutzlast aussieht. Davon hängen
    /// drei Dinge ab: die zulässige Tiefe (siehe [`MAX_BINARY_NESTING_DEPTH`]),
    /// ab wann die Tiefenzählung neu anfängt (siehe [`syntax_byte`]), und ob
    /// die Objekte dieses Bereichs überhaupt verbucht werden.
    ///
    /// ## Der Rumpf zählt mit
    ///
    /// Beim Lauf über die **Datei selbst** (`streams == true`) wird am Ende
    /// alles verbucht, was *keine* Stream-Nutzlast war: Objektköpfe,
    /// Dictionaries, Arrays, die Querverweistabelle, der Trailer. Genau das
    /// macht `lopdf` zu `Object`-Werten, und genau das fehlte dem Budget.
    ///
    /// **Warum das nötig ist — gemessen.** Ein Dictionary-Eintrag `/ab 0` kostet
    /// 5 Byte in der Datei und 197 Byte im Speicher (`size_of::<lopdf::Object>()`
    /// allein ist 120, dazu der Schlüssel als `Vec<u8>` und die Reserve der
    /// `IndexMap`). Eine unkomprimierte 44,3-MB-Datei aus lauter solchen
    /// Einträgen ergab ein `Document` von **1 804 MB — Faktor 40,7**, und zwar
    /// **bevor** irgendein Scan lief. Vom Budget sah sie nichts: es zählte nur
    /// Streams, und Streams hatte sie keine.
    ///
    /// Dieselbe Bombe in einem Objekt-Stream wurde dagegen seit jeher
    /// abgelehnt — der Objekt-Stream wird ausgepackt und als Syntax verbucht.
    /// Die Lücke war also nicht die Bauart der Bombe, sondern allein die Frage,
    /// ob sie komprimiert war. Diese Unterscheidung sucht sich ein Angreifer
    /// als Erstes aus.
    ///
    /// ## Und die **Objekte** zählen mit
    ///
    /// Die Bytes allein reichen nicht: ein leeres Array `[]` ist zwei Byte und
    /// im Speicher 632. Deshalb zählt derselbe Lauf nebenher die Wörter der
    /// Syntax und rechnet daraus, was das geladene Dokument belegen wird —
    /// [`OBJEKT_BYTES`] je Wort, [`ARRAY_BYTES`] je öffnender `[`, verbucht
    /// über [`Prescan::charge_objects`]. Gezählt wird nur außerhalb von
    /// Nutzlast (`binary == false`): woraus `lopdf` keine `Object`-Werte macht,
    /// kostet auch keine. Die Begründung samt Messreihe steht bei
    /// [`OBJEKTSPEICHER_JE_BUDGETBYTE`].
    fn walk(&mut self, bytes: &[u8], streams: bool, binary: bool) -> Result<()> {
        let limit = if binary {
            MAX_BINARY_NESTING_DEPTH
        } else {
            self.limits.max_nesting_depth
        };
        let mut i = 0usize;
        let mut depth = 0usize;
        // Anfang des äußersten Dictionaries — das ist der Kopf des Streams,
        // der gleich folgen kann.
        let mut dict_start = 0usize;
        let mut dict_end = 0usize;
        // Bytes, die als Stream-Nutzlast übersprungen wurden. Sie sind über
        // [`Prescan::account`] schon verbucht und dürfen nicht doppelt zählen.
        let mut nutzlast = 0u64;
        // Steht dieses Byte in einem Namen (`/…`)? Dort sind auch Bytes über
        // 126 zulässig — `lopdf` nimmt sie sogar roh an.
        let mut in_name = false;
        // Der gerechnete Speicher der Objekte in diesem Bereich — die Menge,
        // an der der Speicher des geladenen Dokuments wirklich hängt. Gezählt
        // wird nur, wo aus den Bytes auch wirklich Syntax wird: Nutzlast
        // (`binary`) parst `lopdf` nicht zu `Object`-Werten, und ihre Bytes
        // zählen deshalb schon heute nicht ins Parse-Budget.
        let zaehlen = !binary;
        let mut objektspeicher = 0u64;

        while i < bytes.len() {
            if binary {
                in_name = match bytes[i] {
                    b'/' => true,
                    b if is_whitespace(b) || is_delimiter(b) => false,
                    _ => in_name,
                };
            }
            match bytes[i] {
                // In Nutzlast fängt die Zählung bei jedem Byte neu an, das in
                // PDF-Syntax gar nicht vorkommen kann. Damit misst die Tiefe
                // nur noch **zusammenhängende Syntax** — und genau daraus
                // besteht eine Verschachtelung, die `lopdf` wirklich liest.
                b if binary && !in_name && !syntax_byte(b) => {
                    depth = 0;
                    i += 1;
                }
                b'%' => i = skip_to_eol(bytes, i),
                b'(' => {
                    objektspeicher += zaehlen as u64 * OBJEKT_BYTES;
                    i = skip_literal_string(bytes, i);
                }
                b'<' if bytes.get(i + 1) == Some(&b'<') => {
                    if depth == 0 {
                        dict_start = i;
                    }
                    objektspeicher += zaehlen as u64 * OBJEKT_BYTES;
                    depth += 1;
                    check_depth(depth, limit)?;
                    i += 2;
                }
                b'<' => {
                    objektspeicher += zaehlen as u64 * OBJEKT_BYTES;
                    i = skip_hex_string(bytes, i);
                }
                b'>' if bytes.get(i + 1) == Some(&b'>') => {
                    depth = depth.saturating_sub(1);
                    i += 2;
                    if depth == 0 {
                        dict_end = i;
                    }
                }
                b'[' => {
                    objektspeicher += zaehlen as u64 * ARRAY_BYTES;
                    depth += 1;
                    check_depth(depth, limit)?;
                    i += 1;
                }
                b']' => {
                    depth = depth.saturating_sub(1);
                    i += 1;
                }
                b's' if streams && depth == 0 && keyword_at(bytes, i, b"stream") => {
                    let start = payload_start(bytes, i + b"stream".len());
                    let end = find_from(bytes, b"endstream", start).unwrap_or(bytes.len());
                    let dict = if dict_end > dict_start && dict_end <= i {
                        &bytes[dict_start..dict_end]
                    } else {
                        &[][..]
                    };
                    self.account(dict, &bytes[start..end.max(start)])?;
                    nutzlast = nutzlast.saturating_add((end.max(start) - start) as u64);
                    i = end;
                }
                // Eingebettetes Bild in einem Content-Stream: zwischen `ID`
                // und `EI` stehen rohe Bilddaten, keine Syntax.
                b'B' if !streams && keyword_at(bytes, i, b"BI") => {
                    i = skip_inline_image(bytes, i);
                }
                _ => {
                    if zaehlen && wortanfang(bytes, i) {
                        objektspeicher += OBJEKT_BYTES;
                    }
                    i += 1;
                }
            }
        }
        if streams {
            // Erst hier, nicht laufend: der Lauf selbst belegt für den Rumpf
            // keinen Speicher (er zählt nur Bytes), und die Ablehnung kommt
            // immer noch vor `Document::load_mem` — also vor der einzigen
            // Stelle, an der aus diesen Bytes wirklich Speicher wird.
            let rumpf = (bytes.len() as u64).saturating_sub(nutzlast);
            self.charge_parsed(
                rumpf,
                "die zu parsenden Teile der Datei (Objektköpfe, Dictionaries, \
                 Querverweistabelle) zusammen mit den geparsten Streams",
            )?;
        }
        if zaehlen {
            // Nach dem Byte-Budget, nicht davor: reißt eine Datei beide, ist
            // die Größe die einfachere Auskunft. Und wie dort gilt — der Lauf
            // selbst belegt nichts, gebucht wird am Stück, und die Ablehnung
            // kommt immer noch vor `Document::load_mem`.
            self.charge_objects(objektspeicher)?;
        }
        Ok(())
    }

    /// Verbucht einen Stream und untersucht ihn.
    ///
    /// **Das Stream-Dictionary entscheidet hier nichts.** Es steht in der
    /// Datei, die geprüft werden soll; wer sie baut, schreibt jeden Schlüssel
    /// hinein, den er braucht. Über Budget und Tiefenprüfung entscheidet
    /// deshalb ausschließlich der *Inhalt* des ausgepackten Streams — siehe
    /// [`looks_binary`]. Aus dem Dictionary wird nur die Filterkette gelesen,
    /// und die muss stimmen, sonst ließe sich der Stream gar nicht auspacken.
    fn account(&mut self, dict: &[u8], payload: &[u8]) -> Result<()> {
        let filters = filter_names(dict);
        // Nur diese Filter kann `lopdf` auspacken. Alles andere (DCT, JPX,
        // CCITT, JBIG2, RunLength, Unbekanntes) wird nie zu PDF-Syntax und
        // kann folglich auch keine Verschachtelung verstecken.
        let decodable = filters.iter().all(|f| {
            matches!(
                f.as_slice(),
                b"FlateDecode" | b"LZWDecode" | b"ASCII85Decode"
            )
        });

        if !decodable {
            return self.charge(payload.len() as u64, payload.len() as u64, false);
        }

        let total_room = self
            .limits
            .max_decompressed_bytes
            .saturating_sub(self.decompressed);
        let parsed_room = self.limits.max_parsed_bytes.saturating_sub(self.parsed);

        // Nutzlast oder Syntax? Dafür genügt der Anfang: [`looks_binary`]
        // betrachtet ohnehin nur die ersten [`BINARY_SAMPLE_BYTES`]. Erst
        // wenn diese Frage beantwortet ist, steht fest, welches Budget gilt —
        // und damit, wie viel überhaupt ausgepackt werden darf.
        let flate_only =
            !filters.is_empty() && filters.iter().all(|f| f.as_slice() == b"FlateDecode");
        let (binary, decoded) = if flate_only {
            let probe = self.decode(&filters, payload, total_room.min(BINARY_SAMPLE_BYTES))?;
            let binary = match &probe {
                Some((data, _)) => looks_binary(data),
                None => looks_binary(payload),
            };
            // Syntax bekommt sofort das enge Budget: die Bombe fliegt auf,
            // bevor sie mehr als `max_parsed_bytes` belegt hat.
            let room = if binary {
                total_room
            } else {
                total_room.min(parsed_room)
            };
            (binary, self.decode(&filters, payload, room)?)
        } else {
            // Altlast-Filter werden nur einmal ausgepackt — ein zweiter Lauf
            // durch `lopdf` wäre bei LZW teurer als die Klassifikation wert
            // ist. Die Rohgröße begrenzt [`MAX_LEGACY_STREAM_BYTES`].
            let decoded = self.decode(&filters, payload, total_room)?;
            let binary = match &decoded {
                Some((data, _)) => looks_binary(data),
                None => looks_binary(payload),
            };
            (binary, decoded)
        };

        let data = decoded
            .as_ref()
            .map(|(d, _)| d.as_slice())
            .unwrap_or(payload);
        self.charge(payload.len() as u64, data.len() as u64, !binary)?;

        // **Jeder** auspackbare Stream wird durchlaufen, auch einer, der sich
        // als Bild ausgibt. Früher stand hier eine Ausnahme für
        // `/Subtype /Image` mit der Begründung, `lopdf` weigere sich, solche
        // Streams auszupacken. Das galt für `lopdf` 0.34 und ist seit 0.36
        // nicht mehr wahr: `Stream::decompressed_content` prüft `/Subtype`
        // nicht mehr, und `Document::get_page_content` packt einen
        // Seiteninhalt mit `/Subtype /Image` ganz normal aus. Ein Dictionary
        // ist ohnehin kein Beleg — es gehört dem Angreifer.
        self.walk(data, false, binary)
    }

    /// Packt einen Stream aus — speicherbegrenzt.
    ///
    /// `FlateDecode` läuft über einen begrenzten Leser und kann deshalb nie
    /// mehr belegen als `room`. `ASCII85Decode` schrumpft. `LZWDecode`
    /// überlassen wir `lopdf`, begrenzen dafür aber die Rohgröße.
    ///
    /// Der zweite Rückgabewert sagt, ob `room` erreicht wurde — die Nutzlast
    /// ist dann abgeschnitten und nur noch als „mindestens so groß“ zu lesen.
    fn decode(
        &self,
        filters: &[Vec<u8>],
        payload: &[u8],
        room: u64,
    ) -> Result<Option<(Vec<u8>, bool)>> {
        if filters.is_empty() {
            return Ok(None);
        }
        let legacy = filters.iter().any(|f| f.as_slice() != b"FlateDecode");
        if legacy && payload.len() > MAX_LEGACY_STREAM_BYTES {
            return Err(RedactError::Pdf(format!(
                "Stream mit Altlast-Filter ({}) ist mit {} Bytes zu groß \
                 (Grenze {} Bytes). Solche Streams werden nicht ausgepackt, \
                 weil sich ihr Speicherbedarf nicht vorab begrenzen lässt.",
                filters
                    .iter()
                    .map(|f| String::from_utf8_lossy(f).into_owned())
                    .collect::<Vec<_>>()
                    .join("+"),
                payload.len(),
                MAX_LEGACY_STREAM_BYTES
            )));
        }

        let mut data = payload.to_vec();
        let mut truncated = false;
        for filter in filters {
            data = match filter.as_slice() {
                b"FlateDecode" => match inflate_bounded(&data, room) {
                    Some((out, hit)) => {
                        truncated |= hit;
                        out
                    }
                    // Kaputter oder verschlüsselter Stream: nicht auspackbar,
                    // also wird er auch nicht geparst.
                    None => return Ok(None),
                },
                other => match lopdf_decode(other, &data) {
                    Some(out) => out,
                    None => return Ok(None),
                },
            };
        }
        Ok(Some((data, truncated)))
    }

    /// Verbucht einen Stream: `packed` seine Größe in der Datei, `size` die
    /// nach dem Auspacken.
    ///
    /// **Die Meldung nennt die Ursache, die sie belegen kann.** Sie sprach
    /// früher von einer Dekompressionsbombe — „eine kleine Datei, die sich
    /// beim Öffnen vervielfacht“ —, auch wenn sich gar nichts vervielfacht
    /// hatte: ein 2-MB-Strom **ohne** `/Filter` riss ein 1-MB-Budget mit dem
    /// Faktor 1, und `leaks_many_within` reichte den Satz wörtlich weiter
    /// (Befund R2-D, `tests/zg_r2_decke.rs`). Die Zahl stimmte, die Ursache
    /// nicht, und sie schickte den Leser eine Bombe suchen, die es nicht gibt.
    ///
    /// Gebucht wird deshalb beides, und die Begründung hängt am Verhältnis:
    /// mehr als das **Doppelte** heißt vervielfacht, alles darunter heißt
    /// schlicht „zu viel Strominhalt“. Beide Zahlen stehen in der Meldung, so
    /// dass der Leser das Verhältnis selbst nachrechnen kann.
    fn charge(&mut self, packed: u64, size: u64, syntax: bool) -> Result<()> {
        self.packed = self.packed.saturating_add(packed);
        self.decompressed = self.decompressed.saturating_add(size);
        if self.decompressed > self.limits.max_decompressed_bytes {
            let ursache = if self.decompressed > self.packed.saturating_mul(2) {
                "Das ist das Muster einer Dekompressionsbombe: eine kleine \
                 Datei, die sich beim Öffnen vervielfacht."
            } else {
                "Beim Auspacken wächst dabei fast nichts — die Datei trägt \
                 schlicht mehr Strominhalt, als das Budget zulässt."
            };
            return Err(RedactError::Pdf(format!(
                "entpackte Streams überschreiten das Budget von {} MB \
                 ({} Byte gepackt, {} Byte entpackt). {ursache}",
                self.limits.max_decompressed_bytes / (1024 * 1024),
                self.packed,
                self.decompressed
            )));
        }
        if syntax {
            self.charge_parsed(
                size,
                "die zu parsenden Streams (Seiteninhalt, Objekt-Streams)",
            )?;
        }
        Ok(())
    }

    /// Verbucht Bytes, aus denen `lopdf` PDF-Syntax macht.
    ///
    /// `woher` benennt die Klasse — sie steht in der Meldung, damit erkennbar
    /// ist, welcher Teil der Datei das Budget aufgebraucht hat.
    ///
    /// **Was die Meldung sagen darf.** Sie nannte früher „200 bis 270 Byte je
    /// Dictionary-Eintrag“ — auch dann, wenn ein Content-Stream das Budget
    /// gerissen hatte, und als wäre der Faktor eine Konstante. Er ist keine:
    /// gemessen an 4-MB-Dateien gleicher Größe reicht er von 1,1 (eine lange
    /// Zeichenkette) bis 301,6 (leere Arrays) Byte je Dateibyte. Genau deshalb
    /// gibt es daneben [`OBJEKTSPEICHER_JE_BUDGETBYTE`]; die Meldung verspricht
    /// hier nur noch, was sie halten kann.
    fn charge_parsed(&mut self, size: u64, woher: &str) -> Result<()> {
        self.parsed = self.parsed.saturating_add(size);
        if self.parsed > self.limits.max_parsed_bytes {
            return Err(RedactError::Pdf(format!(
                "{woher} überschreiten das Budget von {} MB. Beim Parsen wird \
                 daraus ein Vielfaches an Arbeitsspeicher — wie viel, hängt an \
                 der Form der Syntax, nicht an ihrer Länge: gemessen 1 Byte je \
                 Dateibyte bei einer langen Zeichenkette und 302 bei lauter \
                 leeren Arrays. Ob ein Stream hierher zählt, entscheidet sein \
                 Inhalt: sieht er wie PDF-Syntax aus statt wie Nutzlast, gilt \
                 dieses engere Budget. Ein wirklich so großes Dokument lässt \
                 sich mit --max-parsed-mb durchlassen.",
                self.limits.max_parsed_bytes / (1024 * 1024)
            )));
        }
        Ok(())
    }

    /// Verbucht den **gerechneten Speicher der Objekte** — die Menge, an der
    /// der Speicher des geladenen Dokuments wirklich hängt.
    ///
    /// Warum es diese zweite Buchhaltung neben [`Prescan::charge_parsed`]
    /// gibt, steht bei [`OBJEKTSPEICHER_JE_BUDGETBYTE`].
    fn charge_objects(&mut self, geschaetzt: u64) -> Result<()> {
        self.objects = self.objects.saturating_add(geschaetzt);
        let decke = self
            .limits
            .max_parsed_bytes
            .saturating_mul(OBJEKTSPEICHER_JE_BUDGETBYTE);
        if self.objects > decke {
            return Err(RedactError::Pdf(format!(
                "die Objekte dieser Datei belegen im Speicher gerechnet mehr \
                 als {} MB — sie wird abgelehnt. Nicht ihre Größe entscheidet \
                 das, sondern Zahl und Art der Objekte: {} Byte je Objekt, {} \
                 je Array. Ein leeres Array `[]` sind zwei Byte in der Datei \
                 und gemessen 632 im Arbeitsspeicher; eine Datei aus lauter \
                 solchen Arrays bleibt damit unter jedem Größenbudget und \
                 belegt trotzdem Gigabytes. Zugestanden sind {} Byte \
                 Objektspeicher je Byte des Budgets von --max-parsed-mb — bei \
                 einem gewöhnlichen Dokument entscheidet deshalb weiterhin \
                 dessen Größe und nicht diese Decke. Ein wirklich so gebautes \
                 Dokument lässt sich mit --max-parsed-mb durchlassen.",
                decke / (1024 * 1024),
                OBJEKT_BYTES,
                ARRAY_BYTES,
                OBJEKTSPEICHER_JE_BUDGETBYTE
            )));
        }
        Ok(())
    }
}

/// Entpackt mit `flate2` und bricht ab, sobald `limit` überschritten ist.
///
/// Der zweite Rückgabewert meldet, dass die Grenze erreicht wurde. Belegt wird
/// nie mehr als `limit + 1` Byte — deshalb kann eine Dekompressionsbombe hier
/// nichts ausrichten.
fn inflate_bounded(data: &[u8], limit: u64) -> Option<(Vec<u8>, bool)> {
    let mut out = Vec::new();
    let reader = flate2::read::ZlibDecoder::new(data);
    if reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut out)
        .is_err()
    {
        return None;
    }
    let truncated = out.len() as u64 > limit;
    Some((out, truncated))
}

/// Auspacken über `lopdf` — für die Filter, die wir nicht selbst können.
fn lopdf_decode(filter: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    let mut dict = lopdf::Dictionary::new();
    dict.set("Filter", Object::Name(filter.to_vec()));
    lopdf::Stream::new(dict, data.to_vec())
        .decompressed_content()
        .ok()
}

/// Filternamen aus den Rohbytes eines Stream-Dictionaries.
fn filter_names(dict: &[u8]) -> Vec<Vec<u8>> {
    let Some(pos) = find_from(dict, b"/Filter", 0) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut i = pos + b"/Filter".len();
    // Hinter `/Filter` steht entweder ein Name oder ein Array von Namen.
    // Beides endet spätestens am nächsten Schlüssel oder am Ende.
    while i < dict.len() {
        match dict[i] {
            b'/' => {
                let start = i + 1;
                let mut end = start;
                while end < dict.len() && !is_delimiter(dict[end]) && !is_whitespace(dict[end]) {
                    end += 1;
                }
                out.push(dict[start..end].to_vec());
                i = end;
                // Ein einzelner Name (kein Array) beendet die Liste.
                if out.len() == 1 && !dict[pos..start].contains(&b'[') {
                    break;
                }
            }
            b']' | b'>' => break,
            _ => i += 1,
        }
    }
    out
}

/// Sieht der Stream nach Nutzlast statt nach PDF-Syntax aus?
///
/// **Das einzige Kriterium, das nicht dem Angreifer gehört.** Ein Dictionary
/// lässt sich beschriften, wie man will — `/Subtype /Image`, `/Length1`,
/// `/Metadata`: alles frei wählbar, alles ohne Wirkung auf das, was `lopdf`
/// später wirklich tut. Der ausgepackte Inhalt lässt sich dagegen nicht
/// fälschen, ohne aufzuhören, das zu sein, was er vorgibt: wer PDF-Syntax
/// unterbringen will, muss druckbare Zeichen schreiben.
fn looks_binary(data: &[u8]) -> bool {
    let sample = &data[..data.len().min(BINARY_SAMPLE_BYTES as usize)];
    if sample.is_empty() {
        return false;
    }
    let odd = sample
        .iter()
        .filter(|b| !matches!(b, 9 | 10 | 12 | 13 | 32..=126))
        .count();
    odd as f64 / sample.len() as f64 > BINARY_RATIO
}

fn is_whitespace(b: u8) -> bool {
    matches!(b, 0 | 9 | 10 | 12 | 13 | 32)
}

fn is_delimiter(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

/// Steht an `i` das Schlüsselwort `kw`, sauber abgegrenzt?
fn keyword_at(bytes: &[u8], i: usize, kw: &[u8]) -> bool {
    if !bytes[i..].starts_with(kw) {
        return false;
    }
    let before_ok = i == 0 || is_whitespace(bytes[i - 1]) || is_delimiter(bytes[i - 1]);
    let after_ok = match bytes.get(i + kw.len()) {
        Some(b) => is_whitespace(*b) || is_delimiter(*b),
        None => true,
    };
    before_ok && after_ok
}

fn skip_to_eol(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i] != b'\n' && bytes[i] != b'\r' {
        i += 1;
    }
    i
}

/// Überspringt eine literale Zeichenkette `( … )` samt Escapes und
/// geschachtelten Klammern.
fn skip_literal_string(bytes: &[u8], mut i: usize) -> usize {
    i += 1;
    let mut nesting = 1usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'(' => {
                nesting += 1;
                i += 1;
            }
            b')' => {
                nesting -= 1;
                i += 1;
                if nesting == 0 {
                    return i;
                }
            }
            _ => i += 1,
        }
    }
    i
}

fn skip_hex_string(bytes: &[u8], mut i: usize) -> usize {
    i += 1;
    while i < bytes.len() && bytes[i] != b'>' {
        i += 1;
    }
    i.saturating_add(1).min(bytes.len())
}

/// Überspringt ein eingebettetes Bild (`BI … ID <Rohdaten> EI`).
fn skip_inline_image(bytes: &[u8], i: usize) -> usize {
    let Some(id) = (i..bytes.len()).find(|&p| keyword_at(bytes, p, b"ID")) else {
        return i + 2;
    };
    let data = id + 3;
    let mut p = data;
    while p < bytes.len() {
        if is_whitespace(bytes[p]) && keyword_at(bytes, p + 1, b"EI") {
            return p + 3;
        }
        p += 1;
    }
    bytes.len()
}

/// Erste Datenposition hinter dem Schlüsselwort `stream`.
fn payload_start(bytes: &[u8], mut i: usize) -> usize {
    if bytes.get(i) == Some(&b'\r') {
        i += 1;
    }
    if bytes.get(i) == Some(&b'\n') {
        i += 1;
    }
    i
}

fn find_from(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= haystack.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// Strukturelle Mindestanforderungen.
pub fn validate(doc: &Document) -> Result<()> {
    if doc.is_encrypted() {
        return Err(RedactError::Pdf(
            "Dokument ist verschlüsselt. Verschlüsselte PDFs werden nicht verarbeitet — \
             bitte vorher entschlüsseln."
                .into(),
        ));
    }
    if doc.catalog().is_err() {
        return Err(RedactError::Pdf(
            "Katalog (/Root) fehlt oder ist defekt".into(),
        ));
    }
    if doc.get_pages().is_empty() {
        return Err(RedactError::Pdf("Dokument enthält keine Seiten".into()));
    }
    Ok(())
}

/// Anzahl der Seiten.
pub fn page_count(doc: &Document) -> usize {
    doc.get_pages().len()
}

/// Ersatz-Seitengröße, wenn im Dokument keine oder eine unbrauchbare MediaBox
/// steht.
pub const A4: Rect = Rect {
    ll: redact_core::Point { x: 0.0, y: 0.0 },
    ur: redact_core::Point {
        x: 595.276,
        y: 841.89,
    },
};

/// Kleinste noch plausible Seitenkante in Punkt (≈ 0,35 mm).
pub const MIN_PAGE_EXTENT: f64 = 1.0;
/// Größte noch plausible Seitenkante in Punkt (≈ 70 m).
pub const MAX_PAGE_EXTENT: f64 = 200_000.0;

/// Eine MediaBox, nachdem sie auf Brauchbarkeit geprüft wurde.
///
/// **Warum das hier steht und nicht beim Rasterizer.** Die Prüfung gab es
/// bisher nur in `redact_render::raster`; [`page_box`] lieferte die Rohangabe
/// aus der Datei ungeprüft weiter. Damit gab es zwei Antworten auf dieselbe
/// Frage „wie groß ist diese Seite?“, und sie liefen auseinander: der
/// Rasterizer zeichnete eine Seite mit `/MediaBox [0 0 0 0]` ganz gewöhnlich
/// auf A4, während die Oberfläche sie für 0 × 0 Punkt groß hielt, jedes
/// Rechteck darauf als „liegt neben der Seite“ aussortierte und die IBAN
/// dieser Seite ungeschwärzt in die Ausgabe schrieb — dieselbe Datei, die
/// `redact_pipeline::run` mit derselben `Config` vollständig schwärzte.
///
/// Jetzt gibt es **eine** Stelle mit der Regel, und beide Seiten fragen dort.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SaneBox {
    /// Die Größe, mit der weitergerechnet werden darf.
    pub rect: Rect,
    /// Die Angabe aus der Datei, falls sie ersetzt werden musste.
    ///
    /// `Some` heißt: was hier steht, ist **nicht** die MediaBox des Dokuments,
    /// sondern [`A4`]. Das gehört gesagt — eine so geheilte Seite sieht sonst
    /// aus wie jede andere.
    pub replaced: Option<Rect>,
}

impl SaneBox {
    /// Musste die Angabe der Datei ersetzt werden?
    pub fn is_replaced(&self) -> bool {
        self.replaced.is_some()
    }

    /// Der Satz, der dazu gesagt werden muss — `None`, wenn alles in Ordnung
    /// war.
    ///
    /// Der Wortlaut ist der des Rasterizers; er stand dort seit jeher in den
    /// `warnings` einer gerenderten Seite und wird jetzt auch von der
    /// Oberfläche benutzt, damit beide Wege denselben Satz sagen.
    pub fn warning(&self) -> Option<String> {
        let raw = self.replaced?;
        Some(format!(
            "Unbrauchbare MediaBox ({} x {}), A4 angenommen",
            raw.normalized().width(),
            raw.normalized().height()
        ))
    }
}

/// Prüft eine MediaBox und ersetzt sie durch [`A4`], wenn sie unbrauchbar ist.
///
/// Unbrauchbar heißt: nicht endlich (NaN, ∞) oder eine Kante außerhalb von
/// [`MIN_PAGE_EXTENT`] … [`MAX_PAGE_EXTENT`]. Das deckt Nullgröße, negative
/// und absurd große Angaben ab.
pub fn sane_box(raw: Rect) -> SaneBox {
    let rect = raw.normalized();
    let finite = [rect.ll.x, rect.ll.y, rect.ur.x, rect.ur.y]
        .iter()
        .all(|v| v.is_finite());
    let (w, h) = (rect.width(), rect.height());
    if finite
        && (MIN_PAGE_EXTENT..=MAX_PAGE_EXTENT).contains(&w)
        && (MIN_PAGE_EXTENT..=MAX_PAGE_EXTENT).contains(&h)
    {
        return SaneBox {
            rect,
            replaced: None,
        };
    }
    SaneBox {
        rect: A4,
        replaced: Some(raw),
    }
}

/// MediaBox jeder Seite (0-basiert), inklusive Vererbung vom Seitenbaum.
///
/// **Ungeprüft** — was in der Datei steht. Wer damit rechnet oder zeichnet,
/// nimmt [`sane_page_boxes`].
pub fn page_boxes(doc: &Document) -> Vec<Rect> {
    doc.get_pages()
        .values()
        .map(|id| page_box(doc, *id))
        .collect()
}

/// Wie [`page_boxes`], aber jede Seite durch [`sane_box`] geschickt.
pub fn sane_page_boxes(doc: &Document) -> Vec<SaneBox> {
    page_boxes(doc).into_iter().map(sane_box).collect()
}

/// MediaBox einer Seite; Standard ist A4, falls nichts angegeben ist.
///
/// Liefert die Angabe **so, wie sie in der Datei steht** — auch wenn sie
/// unbrauchbar ist. Geprüft wird sie von [`sane_box`]; der Rasterizer tut das
/// unmittelbar vor dem Zeichnen, damit die Warnung an der Seite hängt, die
/// gerade gerastert wird.
pub fn page_box(doc: &Document, page_id: lopdf::ObjectId) -> Rect {
    // /MediaBox kann von /Pages geerbt werden.
    let mut current = Some(page_id);
    let mut depth = 0;
    while let Some(id) = current {
        if depth > 32 {
            break;
        }
        depth += 1;
        let Ok(dict) = doc.get_dictionary(id) else {
            break;
        };
        if let Some(rect) = dict
            .get(b"MediaBox")
            .ok()
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| rect_from(o))
        {
            return rect;
        }
        current = match dict.get(b"Parent") {
            Ok(Object::Reference(parent)) => Some(*parent),
            _ => None,
        };
    }
    A4
}

fn rect_from(obj: &Object) -> Option<Rect> {
    let array = obj.as_array().ok()?;
    let v: Vec<f64> = array
        .iter()
        .take(4)
        .filter_map(|o| match o {
            Object::Integer(i) => Some(*i as f64),
            Object::Real(r) => Some(*r as f64),
            _ => None,
        })
        .collect();
    if v.len() < 4 {
        return None;
    }
    Some(Rect::new(v[0], v[1], v[2], v[3]))
}

// ---------------------------------------------------------------------------
// Erreichbarkeit
// ---------------------------------------------------------------------------

/// Meldet, ob das Dokument aus mehreren inkrementellen Revisionen besteht.
///
/// Der Trailer der jüngsten Revision trägt in diesem Fall ein `/Prev` (Zeiger
/// auf die vorige XRef-Sektion) bzw. ein `/XRefStm` — das ist der Beleg für
/// eine Vorgeschichte. Damit er im geladenen Dokument auch wirklich steht,
/// siehe `restore_revision_markers` weiter unten.
pub fn has_incremental_history(doc: &Document) -> bool {
    doc.trailer.get(b"Prev").is_ok() || doc.trailer.get(b"XRefStm").is_ok()
}

/// Trägt `/Prev` und `/XRefStm` wieder in den Trailer ein.
///
/// ## Warum das nötig ist
///
/// Bis `lopdf` 0.34 blieb `/Prev` im geladenen Trailer stehen. Seit 0.42
/// **verbraucht** der Leser den Schlüssel beim Ablaufen der XRef-Kette
/// (`trailer.remove(b"Prev")` in `reader.rs`), und das geladene Dokument sieht
/// danach aus wie eine Datei ohne Vorgeschichte. Der Hinweis „diese Datei
/// besteht aus mehreren Revisionen“ in [`crate::PdfRedactor`] wäre damit
/// stillschweigend verschwunden — und das ist genau die Art Warnung, deren
/// Fehlen niemandem auffällt.
///
/// Deshalb wird der Marker aus den Rohbytes zurückgeholt. Er beschreibt einen
/// Offset in der *Eingabedatei* und darf in keiner Ausgabe landen;
/// [`save_to_bytes`] entfernt ihn vor dem Schreiben wieder.
///
/// [`load_from_bytes_with_limits`] ruft das selbst auf. Wer `lopdf` an dieser
/// Funktion vorbei benutzt — etwa `Document::load_mem_with_options`, um ein
/// verschlüsseltes Dokument mit Passwort zu öffnen —, muss es danach selbst
/// aufrufen, sonst fehlt die Warnung über die Vorgeschichte.
pub fn restore_revision_markers(bytes: &[u8], doc: &mut Document) {
    let Some(trailer) = newest_trailer_area(bytes) else {
        return;
    };
    for key in [&b"/Prev"[..], &b"/XRefStm"[..]] {
        let name = &key[1..];
        if doc.trailer.get(name).is_ok() {
            continue;
        }
        if let Some(offset) = integer_after(trailer, key) {
            doc.trailer.set(name, Object::Integer(offset));
        }
    }
}

/// Der Bytebereich, in dem der Trailer der jüngsten Revision steht.
///
/// `startxref` am Dateiende nennt den Offset der jüngsten XRef-Sektion. Dort
/// steht entweder eine klassische Tabelle (`xref … trailer << … >>`, die
/// Einträge dazwischen sind reine Ziffern) oder das Dictionary eines
/// XRef-Stroms. Im zweiten Fall endet der interessante Bereich am
/// Schlüsselwort `stream` — was dahinter liegt, ist Nutzlast und könnte
/// zufällig `/Prev` enthalten.
fn newest_trailer_area(bytes: &[u8]) -> Option<&[u8]> {
    let key = b"startxref";
    let pos = bytes
        .windows(key.len())
        .enumerate()
        .rfind(|(_, w)| *w == key)
        .map(|(i, _)| i)?;
    let start = usize::try_from(integer_after(&bytes[pos..], key)?).ok()?;
    if start >= bytes.len() {
        return None;
    }
    let rest = &bytes[start..];
    let end = [&b"%%EOF"[..], &b"stream"[..]]
        .iter()
        .filter_map(|needle| find_from(rest, needle, 0))
        .min()
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

/// Liest die Ganzzahl, die (nach Leerraum) hinter `key` steht.
fn integer_after(bytes: &[u8], key: &[u8]) -> Option<i64> {
    let pos = find_from(bytes, key, 0)? + key.len();
    let digits: Vec<u8> = bytes[pos..]
        .iter()
        .skip_while(|b| is_whitespace(**b))
        .take_while(|b| b.is_ascii_digit())
        .copied()
        .collect();
    std::str::from_utf8(&digits).ok()?.parse().ok()
}

/// Sammelt alle vom Trailer aus erreichbaren Objekte.
fn reachable_objects(doc: &Document) -> BTreeSet<ObjectId> {
    let mut seen: BTreeSet<ObjectId> = BTreeSet::new();
    let mut queue: Vec<ObjectId> = Vec::new();

    // Wurzeln: alles, was der Trailer referenziert — /Root, /Info, /Encrypt.
    // Der Trailer wird generisch abgelaufen, damit kein Schlüssel vergessen
    // wird, den eine künftige PDF-Version einführt.
    collect_references(
        &Object::Dictionary(doc.trailer.clone()),
        &mut seen,
        &mut queue,
    );

    // Breitensuche durch den Objektgraphen. `seen` verhindert Zyklen.
    while let Some(id) = queue.pop() {
        let Some(object) = doc.objects.get(&id) else {
            continue;
        };
        collect_references(object, &mut seen, &mut queue);
    }
    seen
}

/// Trägt alle Referenzen eines Objekts in die Arbeitsliste ein.
///
/// Durch Dictionaries, Arrays und Stream-Dictionaries — dort steckt unter
/// anderem ein `/Length`, das als indirektes Objekt vorliegen darf.
///
/// Ohne Rekursion und ohne Tiefengrenze: der eigene Stapel hält die noch
/// offenen Zweige. Eine Tiefengrenze gab es hier einmal (64 Ebenen direkter
/// Verschachtelung); alles darunter galt als **unerreichbar** und wurde von
/// [`prune_unreachable`] gelöscht — ein XObject hinter 70 verschachtelten
/// Arrays verschwand samt Bild aus der Ausgabe. Eine Erreichbarkeitsprüfung,
/// die abbricht, muss „erreichbar“ sagen; hier bricht sie gar nicht mehr ab.
/// Der Stapel wächst höchstens um die Größe des Dokuments, das ohnehin im
/// Speicher liegt.
fn collect_references(object: &Object, seen: &mut BTreeSet<ObjectId>, queue: &mut Vec<ObjectId>) {
    let mut stack: Vec<&Object> = vec![object];
    while let Some(object) = stack.pop() {
        match object {
            Object::Reference(id) => {
                if seen.insert(*id) {
                    queue.push(*id);
                }
            }
            Object::Array(items) => stack.extend(items.iter()),
            Object::Dictionary(dict) => stack.extend(dict.iter().map(|(_, value)| value)),
            Object::Stream(stream) => stack.extend(stream.dict.iter().map(|(_, value)| value)),
            _ => {}
        }
    }
}

/// Entfernt alle Objekte, die vom Trailer aus nicht mehr erreichbar sind.
///
/// `lopdf::Document::save_to` schreibt **alles**, was in `doc.objects` steht —
/// Erreichbarkeit interessiert den Writer nicht. Wer nur eine Referenz löscht
/// (Annotation ohne ihren `/AP`-Stream, `/StructTreeRoot` ohne den `/K`-Baum
/// darunter, `/Metadata` einer Seite), lässt das Objekt selbst stehen, und es
/// landet unkomprimiert und gut lesbar in der Ausgabe. Dasselbe gilt für
/// Objekte, die eine ältere Revision einer `/Prev`-Kette beigesteuert hat und
/// die niemand mehr referenziert.
///
/// Ein einziger Erreichbarkeitslauf vor dem Speichern erschlägt alle diese
/// Fälle. Rückgabe: Anzahl entfernter Objekte.
pub fn prune_unreachable(doc: &mut Document) -> usize {
    let reachable = reachable_objects(doc);
    let before = doc.objects.len();
    doc.objects.retain(|id, _| reachable.contains(id));
    before - doc.objects.len()
}

/// Die Schlüssel, die im Trailer der Ausgabe stehen dürfen (PDF 32000-1,
/// 7.5.5, Tabelle 15 — ohne `/Prev`, das eine Vorgeschichte beschreibt, die
/// die Ausgabe nicht hat).
///
/// Alles andere fällt: `reachable_objects` nimmt den **ganzen** Trailer als
/// Wurzel, ein Objekt unter einem erfundenen Schlüssel (`<< /Zusatz 7 0 R >>`)
/// überlebte damit jedes Aufräumen und stand mit seinem Klartext in der
/// Ausgabe. Dazu kommen die Reste eines XRef-Stroms (`/Type`, `/W`, `/Index`,
/// `/Filter`, `/DecodeParms`, `/Length`), die `lopdf` beim Laden im Trailer
/// ablegt und die in einer klassischen Trailer-Zeile nichts verloren haben;
/// schreibt `lopdf` selbst einen XRef-Strom, setzt es sie ohnehin neu.
const TRAILER_KEYS: [&[u8]; 5] = [b"Root", b"Info", b"Encrypt", b"ID", b"Size"];

/// Serialisiert das Dokument in den Speicher.
///
/// Vor dem Schreiben wird aufgeräumt: der Trailer behält nur die Schlüssel
/// aus [`TRAILER_KEYS`] (und verliert damit auch die Zeiger auf ältere
/// Revisionen, `/Prev` und `/XRefStm`), unerreichbare Objekte fliegen raus.
/// Die Ausgabe ist genau eine Revision — ohne Vorgeschichte und ohne
/// Karteileichen. Das Dokument des Aufrufers bleibt unverändert.
pub fn save_to_bytes(doc: &Document) -> Result<Vec<u8>> {
    let mut buffer = Vec::new();
    let mut copy = doc.clone();
    // Zuerst der Trailer, dann die Erreichbarkeit: was nur ein fremder
    // Trailerschlüssel gehalten hat, ist danach unerreichbar und fällt mit.
    // Die Ausgabe ist eine vollständige, in sich geschlossene Datei — ein
    // geerbtes `/Prev` zeigt in ihr auf einen völlig anderen Offset, im
    // Zweifel mitten in einen Content-Stream.
    let fremd: Vec<Vec<u8>> = copy
        .trailer
        .iter()
        .map(|(key, _)| key.clone())
        .filter(|key| !TRAILER_KEYS.contains(&key.as_slice()))
        .collect();
    for key in fremd {
        copy.trailer.remove(&key);
    }
    prune_unreachable(&mut copy);
    copy.save_to(&mut buffer)
        .map_err(|e| RedactError::Pdf(format!("Speichern fehlgeschlagen: {e}")))?;
    Ok(buffer)
}

// ---------------------------------------------------------------------------
// Der eine Schreibpfad
// ---------------------------------------------------------------------------

/// Wie eine Ausgabedatei angelegt wird.
///
/// Alles, was redact-rs schreibt — geschwärztes PDF, Review-Datei, Audit-Log,
/// Beispieldatei —, geht durch [`write_file`] und damit durch diese Optionen.
/// Vorher hatte jeder Schreibvorgang seine eigenen Regeln, und drei von vier
/// haben die `--force`-Prüfung schlicht übersprungen.
#[derive(Debug, Clone, Default)]
pub struct WriteOptions {
    /// Eine vorhandene Zieldatei überschreiben.
    pub force: bool,
    /// Die Datei nur für den Eigentümer lesbar anlegen (Unix: Modus 0600).
    ///
    /// Für Review-Datei und Audit-Log: dort stehen die *gefundenen*
    /// Geheimnisse im Klartext.
    pub private: bool,
    /// Dateien, die unter keinen Umständen überschrieben werden dürfen —
    /// allen voran die Eingabedatei.
    pub protect: Vec<PathBuf>,
}

impl WriteOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }

    pub fn private(mut self, private: bool) -> Self {
        self.private = private;
        self
    }

    pub fn protect(mut self, path: impl Into<PathBuf>) -> Self {
        self.protect.push(path.into());
        self
    }
}

/// Ein geprüftes Schreibziel: Verzeichnis kanonisiert, Dateiname getrennt.
#[derive(Debug, Clone)]
pub struct Target {
    /// Kanonisiertes Elternverzeichnis (Symlinks und `..` aufgelöst).
    pub dir: PathBuf,
    /// Der Dateiname für sich.
    pub name: std::ffi::OsString,
}

impl Target {
    pub fn path(&self) -> PathBuf {
        self.dir.join(&self.name)
    }
}

/// Prüft ein Schreibziel, ohne zu schreiben.
///
/// Damit kann die Verarbeitungskette *vor* der eigentlichen Arbeit abbrechen,
/// statt erst nach dem Schwärzen zu merken, dass das Ziel nicht taugt.
/// [`write_file`] prüft danach noch einmal — dazwischen kann sich die Platte
/// geändert haben.
pub fn check_target(path: &Path, options: &WriteOptions) -> Result<Target> {
    let name = path
        .file_name()
        .ok_or_else(|| {
            RedactError::Config(format!(
                "{} ist kein Dateiname, sondern ein Verzeichnispfad",
                path.display()
            ))
        })?
        .to_os_string();

    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    if !parent.exists() {
        std::fs::create_dir_all(&parent)?;
    }
    // Kanonisiert wird nur das Verzeichnis. Auf die Zieldatei selbst darf
    // `canonicalize` nicht angewendet werden: es folgt Symlinks, und genau
    // die wollen wir erkennen statt ihnen zu folgen.
    let dir = std::fs::canonicalize(&parent).map_err(|e| {
        RedactError::Config(format!(
            "Ausgabeverzeichnis {} nicht benutzbar: {e}",
            parent.display()
        ))
    })?;
    let target = Target { dir, name };
    let full = target.path();

    // Zuerst die Identitätsprüfung: „das ist deine Eingabedatei“ ist die
    // nützlichere Auskunft als „existiert bereits“, und sie gilt auch mit
    // `--force`.
    for protected in &options.protect {
        if same_file(&full, protected) {
            return Err(RedactError::Config(format!(
                "Ausgabe- und Eingabedatei sind identisch ({} ist {}). \
                 Ein Schwärzungslauf, der sein eigenes Original überschreibt, \
                 ist nicht rückgängig zu machen — auch nicht mit --force.",
                path.display(),
                protected.display()
            )));
        }
    }

    match std::fs::symlink_metadata(&full) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(RedactError::Config(format!(
                    "{} ist ein symbolischer Link. redact-rs schreibt nicht durch \
                     Links hindurch — sonst landet die Ausgabe irgendwo anders, \
                     womöglich in einer Systemdatei. Bitte ein echtes Ziel angeben.",
                    full.display()
                )));
            }
            if !meta.file_type().is_file() {
                return Err(RedactError::Config(format!(
                    "{} ist keine gewöhnliche Datei",
                    full.display()
                )));
            }
            if !options.force {
                return Err(RedactError::Config(format!(
                    "{} existiert bereits — mit --force überschreiben oder -o anders wählen",
                    full.display()
                )));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(RedactError::Config(format!(
                "{} nicht prüfbar: {e}",
                full.display()
            )))
        }
    }

    Ok(target)
}

/// Sind das zwei Namen für dieselbe Datei?
///
/// Der Pfadvergleich allein trägt nicht: `./in.pdf`, `dir/../in.pdf`, ein
/// absoluter Pfad, ein Hardlink und — auf Dateisystemen ohne
/// Groß-/Kleinschreibung — `IN.PDF` bezeichnen alle dieselbe Datei, sehen aber
/// verschieden aus. Deshalb wird zuerst über die Dateiidentität verglichen und
/// nur ersatzweise über den kanonisierten Pfad.
///
/// Öffentlich, damit es im Baum **eine** Antwort auf „dieselbe Datei?“ gibt:
/// die Oberfläche misst damit, ob ein Verzeichnis Groß- und Kleinschreibung
/// unterscheidet (`redact_gui::app::gemessene_schreibweise`).
pub fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(ma), Ok(mb)) = (std::fs::metadata(a), std::fs::metadata(b)) {
            return ma.dev() == mb.dev() && ma.ino() == mb.ino();
        }
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb || eq_ignore_case(&ca, &cb),
        _ => a == b,
    }
}

/// Vergleich ohne Rücksicht auf Groß-/Kleinschreibung — für Dateisysteme, die
/// selbst keine macht (NTFS, APFS in der Voreinstellung).
fn eq_ignore_case(a: &Path, b: &Path) -> bool {
    let (a, b) = (a.to_string_lossy(), b.to_string_lossy());
    a.len() == b.len() && a.to_lowercase() == b.to_lowercase()
}

/// Schreibt eine Datei — der einzige Weg, auf dem redact-rs etwas ablegt.
///
/// * Ziel geprüft: kein Symlink, nicht die Eingabedatei, vorhandene Datei nur
///   mit `force`.
/// * Angelegt wird eine temporäre Datei **im selben Verzeichnis** mit
///   `create_new(true)` — das ist `O_CREAT | O_EXCL` und folgt keinem Symlink.
/// * Sichtbar wird das Ergebnis erst durch `rename`, also in einem Schritt.
///   Ein abgebrochener Lauf hinterlässt keine halbe Ausgabedatei.
pub fn write_file(path: &Path, bytes: &[u8], options: &WriteOptions) -> Result<()> {
    let target = check_target(path, options)?;
    let full = target.path();

    let temp = target.dir.join(format!(
        ".{}.redact-{}-{}.tmp",
        target.name.to_string_lossy(),
        std::process::id(),
        next_temp_counter()
    ));

    let mut open = std::fs::OpenOptions::new();
    open.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Review-Datei und Audit-Log enthalten die gefundenen Geheimnisse im
        // Klartext. Sie dürfen nie mit den Vorgaberechten entstehen.
        open.mode(if options.private { 0o600 } else { 0o644 });
    }

    let write = (|| -> std::io::Result<()> {
        let mut file = open.open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, &full)
    })();

    if let Err(e) = write {
        let _ = std::fs::remove_file(&temp);
        return Err(RedactError::Io(e));
    }
    Ok(())
}

fn next_temp_counter() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Schreibt das Dokument als neue Datei.
///
/// Die Voreinstellung überschreibt ein vorhandenes Ziel (so verhält sich die
/// grafische Oberfläche, die vorher selbst fragt). Die Kommandozeile setzt
/// über [`PdfRenderer::with_options`] ihre eigenen Regeln — dort entscheidet
/// `--force`.
#[derive(Debug, Clone, Default)]
pub struct PdfRenderer {
    options: WriteOptions,
}

impl PdfRenderer {
    pub fn new() -> Self {
        Self {
            options: WriteOptions::new().force(true),
        }
    }

    pub fn with_options(options: WriteOptions) -> Self {
        Self { options }
    }

    pub fn render(&self, doc: &Document, path: &Path) -> Result<()> {
        let bytes = save_to_bytes(doc)?;
        write_file(path, &bytes, &self.options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    #[test]
    fn rejects_non_pdf() {
        let err = load_from_bytes(b"hello world").unwrap_err();
        assert!(matches!(err, RedactError::Pdf(_)));
        assert!(err.to_string().contains("%PDF-"));
    }

    // -----------------------------------------------------------------------
    // Vorprüfung der Rohbytes
    // -----------------------------------------------------------------------

    fn scan(bytes: &[u8]) -> Result<()> {
        prescan(bytes, &Limits::default())
    }

    #[test]
    fn deep_nesting_is_rejected() {
        let deep = format!("{}{}", "[".repeat(1000), "]".repeat(1000));
        let err = scan(deep.as_bytes()).unwrap_err();
        assert!(err.to_string().contains("Verschachtelungstiefe"));
        // Dictionaries zählen genauso.
        let deep = format!("{}{}", "<<".repeat(1000), ">>".repeat(1000));
        assert!(scan(deep.as_bytes()).is_err());
    }

    #[test]
    fn ordinary_nesting_passes() {
        assert!(scan(&crate::testing::demo_statement()).is_ok());
        assert!(scan(&crate::testing::minimal_pdf("Hallo Welt")).is_ok());
        let nested = format!("{}{}", "[".repeat(64), "]".repeat(64));
        assert!(scan(nested.as_bytes()).is_ok());
    }

    #[test]
    fn brackets_inside_strings_do_not_count() {
        // In einer Zeichenkette ist `[` ein Zeichen, keine Struktur. Ohne
        // diese Unterscheidung würde jedes Dokument mit eckigen Klammern im
        // Text irgendwann fälschlich abgelehnt.
        let text = format!("({}) Tj", "[".repeat(1000));
        assert!(scan(text.as_bytes()).is_ok());
        // Escapte Klammern dürfen den Überspringer nicht aus dem Tritt bringen.
        let text = format!("(a\\)b{}) Tj", "[".repeat(1000));
        assert!(scan(text.as_bytes()).is_ok());
    }

    #[test]
    fn a_comment_is_not_structure() {
        let text = format!("% {}\n", "[".repeat(1000));
        assert!(scan(text.as_bytes()).is_ok());
    }

    /// Baut ein Ein-Objekt-PDF-Fragment mit einem Stream.
    fn with_stream(dict: &str, payload: &[u8]) -> Vec<u8> {
        let mut raw = format!(
            "%PDF-1.7\n1 0 obj\n<< {dict} /Length {} >>\nstream\n",
            payload.len()
        )
        .into_bytes();
        raw.extend_from_slice(payload);
        raw.extend_from_slice(b"\nendstream\nendobj\n");
        raw
    }

    #[test]
    fn binary_stream_payload_is_not_read_as_structure() {
        // Schriftprogramme, Farbprofile und Bilddaten enthalten `[`-Bytes rein
        // zufällig. Würden sie als Struktur gezählt, wäre bei genügend großen
        // Streams der Fehlalarm garantiert.
        let payload: Vec<u8> = (0..40_000u32).map(|i| (i * 37 % 256) as u8).collect();
        assert!(scan(&with_stream("/Subtype /Image", &payload)).is_ok());
        assert!(scan(&with_stream("/Length1 4711", &payload)).is_ok());
        // Auch ohne Marker: die Notbremse erkennt Binärdaten am Byteprofil.
        assert!(scan(&with_stream("/N 3", &payload)).is_ok());
    }

    #[test]
    fn deep_nesting_in_an_uncompressed_content_stream_is_caught() {
        // Der Seiteninhalt wird geparst — dort zählt die Tiefe sehr wohl.
        let payload = format!("{}{}", "[".repeat(1000), "]".repeat(1000));
        let err = scan(&with_stream("", payload.as_bytes())).unwrap_err();
        assert!(err.to_string().contains("Verschachtelungstiefe"));
    }

    #[test]
    fn an_inline_image_does_not_confuse_the_scanner() {
        // Zwischen `ID` und `EI` stehen rohe Bilddaten. Sie dürfen weder als
        // Struktur gezählt noch den Rest des Streams verschlucken.
        let mut payload = b"BT ET q 1 0 0 1 0 0 cm BI /W 8 /H 8 /BPC 8 ID ".to_vec();
        payload.extend(std::iter::repeat_n(b'[', 1000));
        payload.extend_from_slice(b" EI Q\n[(a) 1 (b)] TJ\n");
        assert!(scan(&with_stream("", &payload)).is_ok());
    }

    #[test]
    fn the_stream_budget_is_enforced() {
        use lopdf::{dictionary, Stream};

        let mut stream = Stream::new(dictionary! {}, vec![b'x'; 4 * 1024 * 1024]);
        stream.compress().unwrap();
        let mut raw = format!(
            "%PDF-1.7\n1 0 obj\n<< /Filter /FlateDecode /Length {} >>\nstream\n",
            stream.content.len()
        )
        .into_bytes();
        raw.extend_from_slice(&stream.content);
        raw.extend_from_slice(b"\nendstream\nendobj\n");

        let tight = Limits {
            max_parsed_bytes: 1024 * 1024,
            ..Limits::default()
        };
        let err = prescan(&raw, &tight).unwrap_err();
        assert!(err.to_string().contains("Budget"), "{err}");
        // Mit dem Vorgabebudget passt derselbe Stream.
        assert!(prescan(&raw, &Limits::default()).is_ok());
    }

    #[test]
    fn filters_are_read_from_the_dictionary() {
        assert_eq!(filter_names(b"<< /Length 10 >>"), Vec::<Vec<u8>>::new());
        assert_eq!(
            filter_names(b"<< /Filter /FlateDecode /Length 10 >>"),
            vec![b"FlateDecode".to_vec()]
        );
        assert_eq!(
            filter_names(b"<< /Filter [/ASCII85Decode /FlateDecode] >>"),
            vec![b"ASCII85Decode".to_vec(), b"FlateDecode".to_vec()]
        );
    }

    #[test]
    fn binary_streams_are_recognised() {
        assert!(looks_binary(&[0u8, 1, 2, 3, 4, 5, 6, 7]));
        assert!(!looks_binary(b"BT /F1 12 Tf (Hallo) Tj ET"));
    }

    /// Ein Content-Stream bleibt Syntax, egal was im Dictionary steht.
    ///
    /// Früher entschied das Dictionary: `/Image`, `/Length1`, `/Metadata` und
    /// fünf weitere Zeichenketten genügten, um gegen das große statt gegen das
    /// enge Budget verbucht zu werden — und ein `/Image` irgendwo schaltete
    /// zusätzlich die Tiefenprüfung ab. Beides gehörte dem, der die Datei
    /// baut.
    #[test]
    fn the_dictionary_does_not_decide_which_budget_applies() {
        let payload = b"0 0 0 rg\n".repeat(4096); // 36 kB Syntax
        let limits = Limits {
            max_nesting_depth: 100,
            max_decompressed_bytes: 1024 * 1024,
            max_parsed_bytes: 16 * 1024,
        };
        for dict in [
            "",
            " /Harmlos /Image",
            " /Subtype /Image",
            " /Length1 4711",
            " /Type /Metadata",
            " /Type /XRef",
        ] {
            let raw = file_with_stream(dict, &payload);
            let error = prescan(&raw, &limits).expect_err(&format!("Dictionary „{dict}“"));
            assert!(
                error.to_string().contains("zu parsenden Streams"),
                "Dictionary „{dict}“: {error}"
            );
        }
    }

    /// Dieselbe Frage für die Tiefenprüfung.
    #[test]
    fn the_dictionary_does_not_switch_off_the_depth_check() {
        let mut payload = vec![b'['; 200];
        payload.extend(std::iter::repeat_n(b']', 200));
        for dict in ["", " /Harmlos /Image", " /Subtype /Image", " /Length1 4711"] {
            let raw = file_with_stream(dict, &payload);
            let error =
                prescan(&raw, &Limits::default()).expect_err(&format!("Dictionary „{dict}“"));
            assert!(
                error.to_string().contains("Verschachtelungstiefe"),
                "Dictionary „{dict}“: {error}"
            );
        }
    }

    /// Eine Datei mit genau einem Flate-Stream und frei wählbarem Dictionary.
    fn file_with_stream(extra_dict: &str, payload: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        encoder.write_all(payload).expect("komprimierbar");
        let packed = encoder.finish().expect("komprimierbar");
        let mut out = b"%PDF-1.7\n1 0 obj\n".to_vec();
        out.extend_from_slice(
            format!(
                "<< /Filter /FlateDecode /Length {}{extra_dict} >>\nstream\n",
                packed.len()
            )
            .as_bytes(),
        );
        out.extend_from_slice(&packed);
        out.extend_from_slice(b"\nendstream\nendobj\n%%EOF\n");
        out
    }

    // -----------------------------------------------------------------------
    // Schreibpfad
    // -----------------------------------------------------------------------

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "redact-doc-{}-{name}-{}",
            std::process::id(),
            next_temp_counter()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn writing_creates_the_file_atomically() {
        let dir = scratch("atomic");
        let target = dir.join("a.txt");
        write_file(&target, b"eins", &WriteOptions::new()).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"eins");

        // Ohne `force` bleibt das Vorhandene stehen.
        let err = write_file(&target, b"zwei", &WriteOptions::new()).unwrap_err();
        assert!(err.to_string().contains("existiert bereits"));
        assert_eq!(std::fs::read(&target).unwrap(), b"eins");

        write_file(&target, b"zwei", &WriteOptions::new().force(true)).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"zwei");

        // Keine Reste im Verzeichnis.
        let entries: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(entries.len(), 1, "Reste: {entries:?}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_protected_file_is_never_overwritten() {
        let dir = scratch("protect");
        let input = dir.join("in.pdf");
        std::fs::write(&input, b"original").unwrap();

        for alias in [
            input.clone(),
            dir.join(".").join("in.pdf"),
            dir.join("unter").join("..").join("in.pdf"),
        ] {
            std::fs::create_dir_all(dir.join("unter")).unwrap();
            let options = WriteOptions::new().force(true).protect(input.clone());
            let err = write_file(&alias, b"weg", &options).unwrap_err();
            assert!(err.to_string().contains("identisch"), "{alias:?}: {err}");
            assert_eq!(std::fs::read(&input).unwrap(), b"original");
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_refused_and_the_target_stays_untouched() {
        let dir = scratch("symlink");
        let victim = dir.join("fremd.txt");
        std::fs::write(&victim, b"fremder inhalt").unwrap();
        let link = dir.join("ziel.txt");
        std::os::unix::fs::symlink(&victim, &link).unwrap();

        for options in [WriteOptions::new(), WriteOptions::new().force(true)] {
            let err = write_file(&link, b"ueberschrieben", &options).unwrap_err();
            assert!(err.to_string().contains("symbolischer Link"), "{err}");
        }
        assert_eq!(std::fs::read(&victim).unwrap(), b"fremder inhalt");
        std::fs::remove_dir_all(dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn private_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = scratch("mode");
        let target = dir.join("geheim.json");
        write_file(&target, b"{}", &WriteOptions::new().private(true)).unwrap();
        let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn rejects_truncated_pdf() {
        assert!(load_from_bytes(b"%PDF-1.7\nnur muell").is_err());
    }

    #[test]
    fn loads_minimal_document() {
        let bytes = crate::testing::minimal_pdf("Hallo Welt");
        let doc = load_from_bytes(&bytes).unwrap();
        assert_eq!(page_count(&doc), 1);
        assert_eq!(page_boxes(&doc)[0], Rect::new(0.0, 0.0, 595.0, 842.0));
    }

    #[test]
    fn saving_is_deterministic() {
        let bytes = crate::testing::minimal_pdf("Hallo Welt");
        let doc = load_from_bytes(&bytes).unwrap();
        assert_eq!(save_to_bytes(&doc).unwrap(), save_to_bytes(&doc).unwrap());
    }

    // -----------------------------------------------------------------------
    // Erreichbarkeitslauf
    // -----------------------------------------------------------------------

    const SECRET: &str = "DE89 3704 0044 0532 0130 00";

    /// Objekte, die nur die *Dateistruktur* der Eingabe beschreiben: XRef-Strom,
    /// Objekt-Stream-Container, Linearisierungs-Dictionary. Sie hängen an der
    /// XRef-Tabelle, nicht am Objektgraphen, und `lopdf` schreibt sie beim
    /// Speichern ohnehin nicht mit. Dass der Erreichbarkeitslauf sie entfernt,
    /// kostet also nichts.
    fn is_file_structure(object: &Object) -> bool {
        matches!(
            object.type_name().ok(),
            Some(b"XRef") | Some(b"ObjStm") | Some(b"Linearized")
        )
    }

    /// Was der Erreichbarkeitslauf entfernt hat — ohne die reinen
    /// Dateistruktur-Objekte.
    fn pruned_payload(doc: &Document) -> Vec<ObjectId> {
        let mut copy = doc.clone();
        prune_unreachable(&mut copy);
        doc.objects
            .iter()
            .filter(|(id, object)| !copy.objects.contains_key(id) && !is_file_structure(object))
            .map(|(id, _)| *id)
            .collect()
    }

    fn extracted_text(bytes: &[u8]) -> Vec<String> {
        let doc = load_from_bytes(bytes).expect("ladbar");
        crate::PdfExtractor::new()
            .extract(&doc)
            .expect("Extraktion")
            .into_iter()
            .map(|run| run.text)
            .collect()
    }

    #[test]
    fn a_normal_document_loses_nothing() {
        let bytes = crate::testing::demo_statement();
        let doc = load_from_bytes(&bytes).unwrap();
        let before_pages = page_count(&doc);
        let before_boxes = page_boxes(&doc);
        let before_text = extracted_text(&bytes);

        assert_eq!(
            pruned_payload(&doc),
            Vec::<ObjectId>::new(),
            "der Erreichbarkeitslauf hat an einem sauberen Dokument Inhalt entfernt"
        );

        let after = save_to_bytes(&doc).unwrap();
        let reloaded = load_from_bytes(&after).expect("Ausgabe wieder ladbar");
        assert_eq!(page_count(&reloaded), before_pages);
        assert_eq!(page_boxes(&reloaded), before_boxes);
        assert_eq!(extracted_text(&after), before_text);
    }

    #[test]
    fn an_orphaned_object_does_not_reach_the_output() {
        let bytes = crate::testing::minimal_pdf("Kontoinhaber: Max Mustermann");
        let mut doc = load_from_bytes(&bytes).unwrap();
        doc.add_object(Object::Dictionary(dictionary! {
            "Type" => "Vergessen",
            "ActualText" => Object::string_literal(SECRET),
        }));

        // Ohne Aufräumen schreibt `lopdf` das Objekt wortwörtlich mit.
        let mut naive = Vec::new();
        doc.clone().save_to(&mut naive).unwrap();
        assert!(!crate::leaks(&naive, SECRET).is_empty());

        let cleaned = save_to_bytes(&doc).unwrap();
        assert!(
            crate::leaks(&cleaned, SECRET).is_empty(),
            "verwaistes Objekt überlebt: {:?}",
            crate::leaks(&cleaned, SECRET)
        );
        assert_eq!(page_count(&load_from_bytes(&cleaned).unwrap()), 1);
    }

    #[test]
    fn a_cycle_does_not_hang_the_traversal() {
        let bytes = crate::testing::minimal_pdf("Hallo Welt");
        let mut doc = load_from_bytes(&bytes).unwrap();
        let a = doc.new_object_id();
        let b = doc.new_object_id();
        doc.objects.insert(
            a,
            Object::Dictionary(dictionary! { "Next" => Object::Reference(b) }),
        );
        doc.objects.insert(
            b,
            Object::Dictionary(dictionary! { "Next" => Object::Reference(a) }),
        );
        // Vom Katalog aus erreichbar machen, damit der Zyklus wirklich betreten wird.
        let catalog_id = match doc.trailer.get(b"Root").unwrap() {
            Object::Reference(id) => *id,
            _ => unreachable!(),
        };
        doc.get_dictionary_mut(catalog_id)
            .unwrap()
            .set("Ring", Object::Reference(a));

        let mut copy = doc.clone();
        prune_unreachable(&mut copy);
        assert!(copy.objects.contains_key(&a) && copy.objects.contains_key(&b));
    }

    #[test]
    fn an_indirect_stream_length_keeps_its_object() {
        let bytes = crate::testing::minimal_pdf("Hallo Welt");
        let mut doc = load_from_bytes(&bytes).unwrap();
        let page_id = *doc.get_pages().values().next().unwrap();
        let content_id = doc.get_page_contents(page_id)[0];
        let length = doc
            .get_object(content_id)
            .unwrap()
            .as_stream()
            .unwrap()
            .content
            .len() as i64;
        let length_id = doc.add_object(Object::Integer(length));
        if let Ok(Object::Stream(stream)) = doc.get_object_mut(content_id) {
            stream.dict.set("Length", Object::Reference(length_id));
        }

        let mut copy = doc.clone();
        prune_unreachable(&mut copy);
        assert!(
            copy.objects.contains_key(&length_id),
            "/Length als indirektes Objekt wurde weggeräumt"
        );
    }

    // -----------------------------------------------------------------------
    // Inkrementelle Vorversionen
    // -----------------------------------------------------------------------

    /// Ein PDF mit `/Prev`-Kette: die Basisrevision trägt `secret` in einem
    /// Content-Stream, die angehängte Revision hängt die Seite auf einen
    /// **neuen** Stream um. Genau das tun Werkzeuge, die „geschwärzt“ per
    /// inkrementellem Update speichern — der alte Strom bleibt in der Datei.
    fn incremental_with_orphaned_content(secret: &str, replacement: &str) -> Vec<u8> {
        use lopdf::Stream;

        let mut doc = Document::with_version("1.5");
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let content = format!("BT\n/F1 10 Tf\n72 700 Td\n(IBAN: {secret}) Tj\nET\n");
        let content_id = doc
            .add_object(Stream::new(dictionary! {}, content.into_bytes()).with_compression(false));
        let pages_id = doc.new_object_id();
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1_i64,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);

        let mut base = Vec::new();
        doc.clone().save_to(&mut base).unwrap();

        // +1 vergibt `save_to` bereits für den XRef-Strom.
        let new_content_id = doc.max_id + 2;
        let new_content = format!("BT\n/F1 10 Tf\n72 700 Td\n(IBAN: {replacement}) Tj\nET\n");
        let mut stream_body = format!("<</Length {}>>\nstream\n", new_content.len()).into_bytes();
        stream_body.extend_from_slice(new_content.as_bytes());
        stream_body.extend_from_slice(b"\nendstream");

        let page_body = format!(
            "<</Type/Page/Parent {} 0 R/Contents {new_content_id} 0 R/Resources {} 0 R\
             /MediaBox[0 0 595 842]>>",
            pages_id.0, resources_id.0
        )
        .into_bytes();

        append_revision(
            base,
            catalog_id,
            new_content_id + 1,
            &[(new_content_id, stream_body), (page_id.0, page_body)],
        )
    }

    /// Hängt eine weitere Revision an: Objekte, klassische xref-Sektion, `/Prev`.
    fn append_revision(
        mut out: Vec<u8>,
        catalog_id: ObjectId,
        size: u32,
        objects: &[(u32, Vec<u8>)],
    ) -> Vec<u8> {
        let prev = last_startxref(&out).expect("startxref in der Basisrevision");
        if !out.ends_with(b"\n") {
            out.push(b'\n');
        }
        let mut offsets = Vec::new();
        for (id, body) in objects {
            offsets.push((*id, out.len()));
            out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_offset = out.len();
        let mut xref = String::from("xref\n0 1\n0000000000 65535 f \n");
        for (id, offset) in &offsets {
            xref.push_str(&format!("{id} 1\n{offset:010} 00000 n \n"));
        }
        xref.push_str(&format!(
            "trailer\n<</Size {size} /Root {} {} R /Prev {prev}>>\nstartxref\n{xref_offset}\n%%EOF\n",
            catalog_id.0, catalog_id.1
        ));
        out.extend_from_slice(xref.as_bytes());
        out
    }

    fn last_startxref(bytes: &[u8]) -> Option<usize> {
        let key = b"startxref";
        let pos = bytes
            .windows(key.len())
            .enumerate()
            .rfind(|(_, w)| *w == key)
            .map(|(i, _)| i)?;
        let digits: String = bytes[pos + key.len()..]
            .iter()
            .skip_while(|b| b.is_ascii_whitespace())
            .take_while(|b| b.is_ascii_digit())
            .map(|&b| b as char)
            .collect();
        digits.parse().ok()
    }

    #[test]
    fn the_base_revision_of_a_prev_chain_does_not_survive() {
        let bytes = incremental_with_orphaned_content(SECRET, "XXXX XXXX XXXX");
        assert!(
            !crate::leaks(&bytes, SECRET).is_empty(),
            "Testdaten taugen nicht: die Historie enthält das Geheimnis gar nicht"
        );
        let doc = load_from_bytes(&bytes).expect("ladbar");
        assert!(has_incremental_history(&doc));

        // Ohne Aufräumen wandert der verwaiste Basis-Stream in die Ausgabe.
        let mut naive = Vec::new();
        doc.clone().save_to(&mut naive).unwrap();
        assert!(
            !crate::leaks(&naive, SECRET).is_empty(),
            "Vorbedingung: ohne Erreichbarkeitslauf leckt die Datei"
        );

        let cleaned = save_to_bytes(&doc).unwrap();
        assert!(
            crate::leaks(&cleaned, SECRET).is_empty(),
            "Basisrevision überlebt: {:?}",
            crate::leaks(&cleaned, SECRET)
        );
        assert_eq!(extracted_text(&cleaned), vec!["IBAN: XXXX XXXX XXXX"]);
    }

    #[test]
    fn the_output_trailer_has_no_stale_prev() {
        let bytes = incremental_with_orphaned_content(SECRET, "XXXX XXXX XXXX");
        let doc = load_from_bytes(&bytes).expect("ladbar");
        let cleaned = save_to_bytes(&doc).unwrap();

        let reloaded = load_from_bytes(&cleaned).expect("Ausgabe ladbar");
        assert!(!has_incremental_history(&reloaded));
        assert!(
            !String::from_utf8_lossy(&cleaned).contains("/Prev"),
            "die Ausgabe trägt weiterhin ein /Prev"
        );
    }

    #[test]
    fn pruning_stays_deterministic() {
        let bytes = incremental_with_orphaned_content(SECRET, "XXXX XXXX XXXX");
        let doc = load_from_bytes(&bytes).unwrap();
        assert_eq!(save_to_bytes(&doc).unwrap(), save_to_bytes(&doc).unwrap());
    }

    /// `lopdf` verbraucht `/Prev` seit 0.42 beim Laden. Ohne
    /// `restore_revision_markers` verschwände der Hinweis auf die
    /// Vorgeschichte spurlos — deshalb wird hier beides gemessen: der Marker
    /// ist wieder da, **und** eine gewöhnliche Datei bekommt keinen.
    #[test]
    fn the_revision_marker_survives_loading() {
        let bytes = incremental_with_orphaned_content(SECRET, "XXXX XXXX XXXX");
        assert!(
            String::from_utf8_lossy(&bytes).contains("/Prev"),
            "Testdaten taugen nicht: die Eingabe hat gar keine Vorgeschichte"
        );

        let raw = Document::load_mem(&bytes).expect("ladbar");
        let doc = load_from_bytes(&bytes).expect("ladbar");
        assert!(
            has_incremental_history(&doc),
            "der /Prev-Marker ist beim Laden verlorengegangen (lopdf-Rohladung: \
             {:?})",
            raw.trailer.get(b"Prev").is_ok()
        );

        // Gegenprobe: eine Datei aus einer einzigen Revision wird nicht
        // fälschlich als Mehrfachrevision gemeldet.
        let single = load_from_bytes(&crate::testing::minimal_pdf("Hallo")).unwrap();
        assert!(!has_incremental_history(&single));
    }
}
