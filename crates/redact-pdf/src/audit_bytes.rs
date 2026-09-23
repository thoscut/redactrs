//! Leak-Detektor: sucht eine Zeichenkette in **allem**, was in einer PDF-Datei
//! steht — nicht nur im Seiteninhalt.
//!
//! ## Warum dieses Modul existiert
//!
//! Die End-to-End-Tests haben bislang zwei blinde Orakel benutzt:
//!
//! * `doc.get_page_content(page)` — sieht nur den Content-Stream der Seite.
//!   Form-XObjects, Annotation-Appearances, Metadaten, Struct-Tree-Strings,
//!   verwaiste Objekte und Objekt-Streams kommen darin schlicht nicht vor.
//! * den eigenen Extraktor — ein Zirkelschluss: wovor der Extraktor blind ist,
//!   das wird nicht geschwärzt und ist damit auch für den Test unsichtbar.
//!
//! [`leaks`] ist das ehrliche Messgerät: es kennt keine „Seitenlogik“, sondern
//! durchsucht die Datei auf allen Ebenen, auf denen ein Geheimnis überleben
//! kann.
//!
//! ## Was durchsucht wird
//!
//! 1. die **rohen Dateibytes** (unkomprimierte Reste, Historie inkrementeller
//!    Updates, verwaiste Objekte),
//! 2. **jeder roh gefundene `stream … endstream`-Block**, zusätzlich
//!    Flate-dekomprimiert — damit auch komprimierte Altrevisionen sichtbar
//!    werden, die im Objektgraph der neuesten Revision gar nicht auftauchen,
//! 3. **jedes Stream-Objekt** des Objektgraphen, dekodiert über
//!    [`crate::filters`] (Flate, LZW, ASCII85, ASCIIHex, RunLength, mit
//!    Prädiktor) — derselbe Dekoder, den der Schwärzer benutzt, nur
//!    nachsichtiger: bricht die Filterkette an einem unbekannten Glied ab,
//!    wird durchsucht, was **bis dahin** entpackt war
//!    ([`crate::filters::decoded_prefix_within`]), und die Fundstelle sagt,
//!    wo es stehen blieb; bleibt gar nichts übrig (schon das erste Glied
//!    unbekannt), sind die Rohbytes aus Sicht 2 die Rückfallebene und der
//!    Abbruch steht in [`LeakCheck::unchecked`],
//! 4. **Objekte in Objekt-Streams** (`/ObjStm`) — komprimierte Container, die
//!    eine reine Rohbyte-Suche nicht sehen kann,
//! 5. **alle Zeichenketten-Objekte** im gesamten Objektgraph, egal unter
//!    welchem Schlüssel (`/Contents`, `/V`, `/ActualText`, `/Alt`, `/TU`,
//!    `/T`, `/Info`-Werte, `/Names` …), inklusive Trailer,
//! 6. innerhalb von Streams zusätzlich die **Verkettung aller
//!    Zeichenketten-Literale** — damit wird Text auch dann gefunden, wenn er
//!    per `TJ` in Bruchstücke zerlegt ist,
//! 7. **jede Seite, wie der eigene Schriftdekoder sie liest**: Glyphencodes
//!    über `/ToUnicode`, `/Differences` und Standardkodierungen in Zeichen
//!    übersetzt, zu Zeilen gesetzt ([`scan_decoded_text`]).
//!
//! Beide PDF-String-Kodierungen werden berücksichtigt: PDFDocEncoding/Latin-1
//! **und** UTF-16 (BE wie LE, mit und ohne BOM). Ebenso beide Syntaxen:
//! literal `(DE89…)` und hexadezimal `<44453839…>`.
//!
//! ## Warum Sichtweise 7 kein Zirkelschluss ist
//!
//! Die Sichten 1–6 vergleichen **Bytes**. Bei einer eingebetteten
//! Teilmengen-Schrift stehen im Strom aber Glyphnummern oder umgelenkte Codes
//! (`<01020304>Tj`), keine Zeichen — und das ist die Datei aus Word,
//! LibreOffice und Chrome, also der Regelfall. Gemessen an einem
//! LibreOffice-Writer-24.2-Export (TrueType-Teilmenge `BAAAAA+LiberationSerif`,
//! Codes ab `01`) und an einem PyMuPDF-Export (Type0/Identity-H, DejaVuSans)
//! meldete `--check-leaks` an der **ungeschwärzten** Datei „keiner der 4
//! Suchbegriffe steht noch in der Datei“, Rückgabewert 0.
//!
//! Sicht 7 wäre **allein** genau der Zirkelschluss von oben. Sie steht
//! deshalb **neben** den Bytesichten, nicht an ihrer Stelle: die Bytesichten
//! finden, was der Dekoder nicht liest (Metadaten, verwaiste Objekte, Text in
//! einem Strom, den kein `Do` erreicht, Rohbytes einer alten Revision); der
//! Dekoder findet, was die Bytesichten nicht lesen (Glyphencodes).
//!
//! ## Benannte blinde Flecken
//!
//! Was **keine** der sieben Sichten sieht — und was deshalb auch ein sauberer
//! Lauf nicht ausschließt:
//!
//! * **Ein lügendes `/ToUnicode`.** Die Zuordnung ist eine Behauptung der
//!   Datei; der Dekoder glaubt ihr. Bildet sie jeden Code auf „x“ ab, liest
//!   Sicht 7 „xxxx“, die Bytesichten sehen Glyphnummern, und der Text auf
//!   dem Papier bleibt unsichtbar (Kanarienvogel in
//!   `tests/zb_orakel_schriftdekoder.rs`).
//! * **Eine Schrift ohne brauchbare Zuordnung** — kein `/ToUnicode`, keine
//!   `cmap` im Fontprogramm. Der Interpreter warnt beim Schwärzen darüber;
//!   `leaks` gibt nur Fundstellen zurück und kann die Warnung nicht
//!   weiterreichen.
//! * **Text in einem Rasterbild** und **Glyphen als Pfade** (Umrisse statt
//!   Schrift): dort gibt es keine Codes, die man übersetzen könnte. Ein
//!   Strom hinter `/DCTDecode`, `/JPXDecode`, `/CCITTFaxDecode` oder
//!   `/JBIG2Decode` fällt darunter — allein wie am **Ende** einer Kette. Er
//!   erzeugt deshalb **keinen** Eintrag in [`LeakCheck::unchecked`]: sonst
//!   käme jede Datei mit einem Foto als „unvollständig geprüft“ zurück, und
//!   eine Grenze, die gewöhnliche Dateien abweist, ist genauso ein Fehler wie
//!   eine Lücke.
//!
//!   Steht hinter dem Bildfilter aber noch ein Glied, trägt diese Begründung
//!   nicht: die Reihenfolge im `/Filter`-Array ist die Dekodierreihenfolge
//!   (PDF 32000-1, 7.4.1), und die Ausgabe eines Bildfilters sind
//!   Abtastwerte — kein Erzeuger hängt dahinter noch einen Filter. Dort liegt
//!   also kein Bild, sondern ein Glied, das niemand angewandt hat:
//!   `[/DCTDecode /FlateDecode]` deckte so einen Filter zu, den dieses
//!   Programm **kennt**, und verkaufte den Bildfilternamen als meldungsfreie
//!   Zone für beliebige Bytes (Befund R2-A, behoben in Fix-Runde 7). Die
//!   Ausnahme gilt seitdem nur für den Bildfilter als **letztes** Glied;
//!   `[/ASCII85Decode /DCTDecode]` — die Distiller-Kette, auf die sie zielt —
//!   schweigt unverändert.
//!
//!   Ein Filtername, den niemand kennt, ist etwas anderes und steht sehr wohl
//!   in `unchecked` — seit Fix-Runde 6 auch dann, wenn er das **erste** Glied
//!   ist (`/Filter /FooDecode` kam vorher als „nicht gefunden“ mit
//!   Rückgabewert 0 zurück, `/Filter [/FlateDecode /FooDecode]` mit 3;
//!   Befund Q2-1/Q5). Ebenso ein Glied, das gar **kein Name** ist
//!   (`[/LZWDecode null]`, ein Verweis ins Leere, eine Zahl, eine
//!   Zeichenkette): bis Fix-Runde 6 verwarf `filters::filter_names` dafür die
//!   ganze Kette, und der Strom las sich wie einer ohne `/Filter` — kein
//!   Fund, keine Meldung, Rückgabewert 0, obwohl ein bekannter Filter daneben
//!   stand (Befund R2-C).
//! * **Was tiefer liegt als [`MAX_DEPTH`]** Ebenen im Objektgraphen. Der
//!   Lader lässt 100 zu, diese Sicht läuft 32 — der Abbruch steht seit
//!   Fix-Runde 5 in [`LeakCheck::unchecked`] mit dem Objektpfad. Vorher war
//!   er still: ein oktal maskierter Text in 33 verschachtelten Arrays kam an
//!   der Kommandozeile als „nicht gefunden“ mit Rückgabewert 0 zurück
//!   (Befund P4-2).
//!
//! ## Kosten und Budget
//!
//! Die Arbeit hängt an der Datei, nicht an den Begriffen: jeder Datenblock
//! wird **einmal** entpackt und **einmal** durchlaufen — ein
//! Aho-Corasick-Automat über alle Muster aller Begriffe ([`Matcher`]).
//! Bis Fix-Runde 4 lief je Begriff und Kodierung eine eigene `memmem`-Suche
//! über jeden Block; das kostete Begriffe × Bytes. Nachgemessen in
//! Fix-Runde 6 (`tests/zd_orakel_budget.rs::zd_mess_die_alte_suche_je_muster`,
//! Release): an einer Datei mit einem 64-MiB-Strom läuft jedes Muster über
//! **256 MB** — vier Blöcke à 64 MiB (Rohdatei, roher Stromblock gepackt und
//! entpackt, derselbe Strom über den Objektgraphen dekodiert), und MB ist hier
//! wie überall 1024² Byte. (Bis Fix-Runde 8 stand hier „268 MB“: dieselbe
//! Menge dezimal gerechnet, also der Fehler, den derselbe Modulkopf ein
//! Stück weiter unten für „205 und 138 MB“ schon einmal geradezieht.)
//!
//! Was die Zeit angeht, hängt jede dieser Zahlen an der Maschine, auf der sie
//! entstanden ist — deshalb steht dabei, welcher Lauf sie geliefert hat:
//!
//! | Lauf | `memmem` | alte Suche, 1 Begriff | linear auf 1 000 | heute, 1 000 |
//! |---|---|---|---|---|
//! | Fix-Runde 6 | 9,9 GB/s | 0,163 s | rund 163 s | 5,99 s (gegen 5,01 s mit einem, Verhältnis 1,20) |
//! | Fix-Runde 8, geteilte Maschine | 12,3 GB/s | 0,131 s | rund 131 s | 6,74 s (gegen 5,78 s, Verhältnis 1,17) |
//!
//! Die hochgerechnete Zahl ist eine **untere** Schranke: die
//! Zeichenketten-Verkettung und die Textsichten kommen darauf. Beide Läufe
//! liegen weit über den früher hier genannten „65,7 s“ — die sind damit
//! falsch, und daran ändert die Maschine nichts; die Größenordnung des
//! CHANGELOG (rund 300 s) passt. Die Sekundenzahlen sind **aufgeschriebene
//! Läufe**, keine Zusage: zugesichert und geprüft ist die Schranke „1 000
//! Begriffe unter dem Vierfachen von einem“
//! (`tests/zd_orakel_budget.rs::zd_mess_1000_begriffe_kosten_wie_einer`), und
//! die gilt auch auf einer anders schnellen Maschine.
//!
//! Entpackt wird nur bis zu einem Budget ([`leaks_many_within`]); was das
//! Budget nicht deckt, steht in [`LeakCheck::unchecked`], damit „nicht
//! gefunden“ nie stillschweigend „nicht gesucht“ bedeutet. Das Budget ist
//! einer von **fünf** Gründen, die dort stehen können — die Aufzählung führt
//! [`LeakCheck`].
//!
//! ## Der Ort einer Fundstelle
//!
//! Jede Fundstelle kommt zweimal: als **Satz** (`findings`) und als **Ort**
//! ([`LeakCheck::sites`], ein [`LeakSite`] je Fund, gleiche Reihenfolge). Der
//! Satz ist für Menschen — Kommandozeile, Oberfläche, `docs/pruefung.txt` —
//! und bleibt Zeichen für Zeichen, wie er ist. Der Ort ist für Programme: er
//! nennt die Sicht, die Seite (wo eine Sicht eine kennt, also Sicht 7) und die
//! Objekt-Id (wo sie eine kennt) — samt ihrer **Herkunft**
//! ([`LeakSite::object_source`]): aus dem geladenen Dokument, oder aus dem
//! Objektkopf in den Rohbytes und dort gegen das Dokument **nicht** geprüft.
//! Die Rohsicht-Id ist eine Aussage über die Bytes, keine über das geladene
//! Dokument; an einer Datei mit inkrementellem Update fallen beide auseinander
//! (Befund ZI-A1, siehe [`LeakSite::object`]).
//!
//! Der Grund ist eine Sicherheitsfolge, kein Komfort: die Oberfläche muss
//! „gewollt stehen geblieben“ von „Schwärzung danebengegangen“ trennen. Solange
//! der Ort nur im Satz stand, konnte sie ihn bloß aus einer Meldung herauslesen
//! — und eine danebengegangene Schwärzung sah aus wie ein bewusst stehen
//! gelassener Text. Aus einem Leck wurde „keine Aussage“.
//!
//! ## Fehlerrichtung
//!
//! Im Zweifel meldet der Detektor zu viel. Ein Fehlalarm lässt einen Test laut
//! fehlschlagen und wird untersucht; ein übersehenes Leck lässt ihn still grün
//! bleiben und wird ausgeliefert. Deshalb wird derselbe Fund gerne mehrfach
//! gemeldet — einmal je Sichtweise (roh, dekodiert, als Objektfeld).

use std::collections::BTreeSet;

use aho_corasick::{AhoCorasick, MatchKind};
use lopdf::{Dictionary, Document, Object, ObjectStream, Stream, StringFormat};
use memchr::memmem;

use crate::document::{
    is_delimiter, is_whitespace, prescan, raw_dict_entry, raw_object_value, Limits,
};
use crate::extract::PdfExtractor;
use crate::filters::{self, Oversize};

/// Obergrenze für gemeldete Fundstellen — eine Fehlermeldung mit 5000 Zeilen
/// hilft niemandem.
const MAX_HITS: usize = 200;
/// Anzahl Bytes Kontext links und rechts der Fundstelle.
const CONTEXT: usize = 24;
/// Maximale Verschachtelungstiefe beim Ablaufen des Objektgraphen.
const MAX_DEPTH: usize = 32;
/// Obergrenze für einzeln genannte, nicht entpackte Ströme; danach nur noch
/// eine Summenzeile. Ist das Budget einmal aufgebraucht, träfe es sonst jeden
/// weiteren Strom der Datei.
const MAX_UNCHECKED: usize = 50;
/// Wie weit vor einem rohen `stream` nach dem Objektkopf `N G obj` gesucht
/// wird — nur für die Meldung, welcher Strom nicht entpackt wurde.
const OBJECT_HEADER_LOOKBACK: usize = 64 * 1024;

/// Sucht `needle` in ALLEM, was in der Datei steht — nicht nur im Seiteninhalt.
///
/// Rückgabe: eine Liste von Fundstellen mit Kontext. Leer heißt: die
/// Zeichenkette kommt in der Datei auf keiner der oben genannten Ebenen vor.
/// Jeder Eintrag nennt, *wo* der Fund liegt (Objekt-Id plus Feld bzw. Stream),
/// damit ein fehlschlagender Test etwas Brauchbares sagt.
pub fn leaks(pdf_bytes: &[u8], needle: &str) -> Vec<String> {
    leaks_many(pdf_bytes, std::slice::from_ref(&needle))
        .pop()
        .unwrap_or_default()
}

/// Sucht mehrere Zeichenketten in **einem** Durchgang durch die Datei.
///
/// Rückgabe: je Suchbegriff eine Liste von Fundstellen, in der Reihenfolge der
/// Eingabe. `leaks_many(b, &[x])[0] == leaks(b, x)` — dieselbe Messung, nur
/// ohne die Arbeit mehrfach zu tun.
///
/// ## Warum es diese Form gibt
///
/// [`leaks`] entpackt jeden Stream, parst den Objektgraphen und dekodiert
/// jede Zeichenkette. Diese Arbeit hängt allein an der Datei, nicht am
/// Suchbegriff. Wer `--check-leaks` mit zehn Begriffen aufruft, hat sie
/// vorher zehnmal bezahlt — die alte Suche lief **je Muster** über jeden
/// Datenblock, also linear in der Zahl der Begriffe. Hier fällt sie einmal
/// an, und seit Fix-Runde 4 auch der Vergleich: ein Automat über alle
/// Begriffe. Was das an Zeit ausmacht, steht im Modulkopf unter „Kosten und
/// Budget“ — mit dem Test, der es nachmisst.
///
/// (Bis Fix-Runde 8 standen hier „0,13 s für einen Begriff und 0,88 s für
/// zehn, an einer 792-kB-Datei“. Hinter den Zahlen stand kein Test, und sie
/// messen einen Code-Pfad, den es seit Fix-Runde 4 nicht mehr gibt —
/// nachprüfbar ist daran nichts, also stehen sie nicht mehr da. Dieselbe
/// Sorte Zahl wie die „65,7 s“, die Fix-Runde 6 gestrichen hat.)
///
/// Ohne Budget: [`leaks_many_within`] mit `u64::MAX`. Was sich nicht laden
/// oder entpacken lässt, fehlt hier stillschweigend — wer das wissen muss,
/// liest [`LeakCheck::unchecked`].
pub fn leaks_many(pdf_bytes: &[u8], needles: &[&str]) -> Vec<Vec<String>> {
    leaks_many_within(pdf_bytes, needles, u64::MAX).findings
}

/// Ergebnis von [`leaks_many_within`]: die Fundstellen je Suchbegriff und die
/// Stellen, die **nicht** durchsucht wurden.
///
/// `unchecked` leer heißt: jede Sicht ist vollständig gelaufen. Ist es nicht
/// leer, ist „nicht gefunden“ keine Aussage — der Aufrufer muss das sagen
/// (Kommandozeile: Rückgabewert 3; Oberfläche: Satz in der Statuszeile).
///
/// Die Gründe, die dieser Code kennt, sind **fünf** — jede Zeile nennt ihren
/// eigenen, der Aufrufer nimmt keinen an:
///
/// 1. **Entpackgrenze**: ein Strom ergäbe mehr als das verbleibende Budget
///    (`--max-decompressed-mb`, [`Budget::skip`]); seine gepackten Bytes
///    wurden roh durchsucht.
/// 2. **Die Vorprüfung des Laders lehnt die Datei ab** ([`prescan`]) oder
///    `lopdf` lädt sie nicht: dann fehlen die Sichten 3–7 ganz.
/// 3. **Verschachtelungstiefe [`MAX_DEPTH`] erreicht** — was tiefer im
///    Objektgraphen liegt, hat keine Sicht gelesen; die Zeile nennt den
///    Objektpfad.
/// 4. **Die Filterkette blieb an einem Glied stehen, das sich nicht anwenden
///    lässt** — „nur bis Filter N von M dekodiert“ bzw. „gar nicht
///    dekodiert“, wenn es schon das erste Glied ist. Drei Formen: ein
///    Filtername, den dieses Programm nicht kennt; ein Glied, das gar kein
///    Name ist (`[/LZWDecode null]`, ein Verweis ins Leere, eine Zahl, eine
///    Zeichenkette — Befund R2-C); und ein **Bildfilter, hinter dem die Kette
///    weitergeht** (Befund R2-A). Der Bildfilter als **letztes** Glied zählt
///    nicht dazu: benannter blinder Fleck, siehe Modulkopf.
/// 5. **Sicht 7 (Schriftdekoder) nicht gelaufen**: sie entpackt ohne eigene
///    Grenze und läuft deshalb nur, wenn Sicht 3 jeden Strom entpacken
///    konnte.
///
/// Gründe 1 und 4 sind je Sicht auf [`MAX_UNCHECKED`] Zeilen gedeckelt; was
/// darüber liegt, steht als Summenzeile („… und N weitere …“).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LeakCheck {
    /// Je Suchbegriff die Fundstellen, in der Reihenfolge der Eingabe.
    pub findings: Vec<Vec<String>>,
    /// Je Suchbegriff: hat er **wörtlich** getroffen — Zeichen für Zeichen,
    /// in irgendeiner Kodierung —, oder nur seine Fassung **ohne Leerraum**?
    ///
    /// Gleiche Länge und Reihenfolge wie `findings`. `false` bei einem
    /// Begriff ohne Fund; `false` bei einem Fund heißt: gefunden wurde nur
    /// die gequetschte Normalform (`DE893704…` für `DE89 3704 …`).
    ///
    /// Die Unterscheidung steht im Text jeder Fundstelle schon in eckigen
    /// Klammern („…, ohne Leerraum“). Hier steht sie **maschinenlesbar**,
    /// damit die Oberfläche sie nicht aus einer Meldung herauslesen muss:
    /// Sie entscheidet daran, ob ein Rest ein Leck ist oder eine bewusst
    /// stehen gelassene Schreibweise derselben Normalform (Befund P5-2).
    /// Sie kostet nichts: gezählt wird beim Melden, nicht beim Suchen.
    pub literal: Vec<bool>,
    /// Je Suchbegriff und je Fundstelle: **wo** sie liegt — maschinenlesbar,
    /// neben ihrem Text in `findings`.
    ///
    /// `sites[n][i]` gehört zu `findings[n][i]`: gleiche Länge, gleiche
    /// Reihenfolge, Eintrag für Eintrag (geprüft in
    /// `tests/zh_a_ort_maschinenlesbar.rs`).
    ///
    /// **Warum das Feld hier steht.** Bis Fix-Runde 7 stand der Ort einer
    /// Fundstelle nur in ihrem Text („Objekt 7 0/Popup/Contents“, „Seite 3“).
    /// Die Oberfläche muss aber „gewollt stehen geblieben“ von „Schwärzung
    /// danebengegangen“ trennen — und hatte dafür nur diesen Text, also nur
    /// Raten. Damit sah eine danebengegangene Schwärzung aus wie ein bewusst
    /// stehen gelassener Text, und aus einem Leck wurde „keine Aussage“
    /// (Register #41, Vertrag V5). Jetzt liegt der Ort maschinenlesbar
    /// daneben, so wie [`LeakCheck::literal`] die Schreibweise daneben legt.
    ///
    /// Der Text in `findings` ist davon **unberührt**: er ist Ausgabe
    /// (Kommandozeile, Oberfläche, `docs/pruefung.txt`), und ein zusätzliches
    /// Feld bricht keine Ausgabe.
    pub sites: Vec<Vec<LeakSite>>,
    /// Was nicht durchsucht wurde, je Stelle ein Satz (Objekt, Grund) — die
    /// fünf möglichen Gründe stehen oben am Typ.
    pub unchecked: Vec<String>,
    /// Wie viele **Stellen** ungeprüft blieben.
    ///
    /// `unchecked.len()` ist etwas anderes: die Zahl der **Zeilen**. Über
    /// [`MAX_UNCHECKED`] hinaus fasst eine Summenzeile viele Stellen zu einer
    /// Zeile zusammen, und dann ist die Zeilenzahl **kleiner** als die Zahl
    /// der Stellen: 61 nicht entpackte Ströme stehen als 50 Zeilen + eine
    /// Summenzeile („… und 11 weitere“) + die Zeile über Sicht 7 = 52 Zeilen.
    /// Wer „52 Stelle(n) nicht geprüft“ darunter schreibt, sagt eine kleinere
    /// Zahl als die Liste darüber (Befund R2-B).
    ///
    /// Gezählt wird hier jede Stelle einzeln, auch die von der Decke
    /// verschwiegene; die beiden Zeilen, die keine einzelne Stelle nennen
    /// („Sicht 7 nicht gelaufen“, „Objektgraph nicht durchsucht“) zählen als
    /// je eine. `unchecked_places == 0` heißt deshalb genau dasselbe wie
    /// `unchecked.is_empty()`.
    pub unchecked_places: usize,
}

/// Welche der sieben Sichten des Modulkopfs einen Fund hatte.
///
/// Die Sicht sagt, **auf welcher Ebene** der Text noch steht, und das ist
/// eine andere Auskunft als der Fund selbst: ein Fund in
/// [`LeakView::FontDecoder`] steht auf dem Papier, einer in
/// [`LeakView::RawFile`] in den Rohbytes (etwa einer Altrevision), einer in
/// [`LeakView::StringObject`] in einem Feld, das kein Leser zu sehen bekommt.
///
/// Die Nummern sind die des Modulkopfs; [`LeakView::number`] gibt sie aus,
/// damit eine Oberfläche „Sicht 3“ schreiben kann wie die Doku.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LeakView {
    /// Sicht 1: die rohen Dateibytes.
    RawFile,
    /// Sicht 2: ein roh gefundener `stream … endstream`-Block — gepackt wie
    /// entpackt.
    RawStream,
    /// Sicht 3: ein Stream-Objekt des Objektgraphen, roh und über
    /// [`crate::filters`] dekodiert.
    Stream,
    /// Sicht 4: ein Objekt **in** einem Objekt-Stream (`/ObjStm`).
    ObjectStream,
    /// Sicht 5: ein Zeichenketten- oder Namensobjekt des Objektgraphen,
    /// Trailer eingeschlossen.
    StringObject,
    /// Sicht 6: die Verkettung aller Zeichenketten-Literale **in** einem
    /// Datenblock — der Fund, den ein per `TJ` zerlegter Text nur so hergibt.
    StringConcat,
    /// Sicht 7: eine Seite, wie der eigene Schriftdekoder sie liest.
    FontDecoder,
}

impl LeakView {
    /// Die Nummer, unter der der Modulkopf diese Sicht führt.
    pub fn number(self) -> u8 {
        match self {
            Self::RawFile => 1,
            Self::RawStream => 2,
            Self::Stream => 3,
            Self::ObjectStream => 4,
            Self::StringObject => 5,
            Self::StringConcat => 6,
            Self::FontDecoder => 7,
        }
    }
}

/// Woher die Objekt-Id einer Fundstelle stammt — und damit, **was sie
/// zusichert**.
///
/// Zwei Quellen, zwei verschieden starke Aussagen. Ohne diese Unterscheidung
/// stand an [`LeakSite::object`] eine Zusicherung, die der Code nicht hält:
/// „Objektnummer und Generation, wie `lopdf` sie zählt“ — die Rohsichten
/// zählen aber nicht mit `lopdf`, sie lesen einen Objektkopf aus den Bytes
/// (Befund ZI-A1, `tests/zi_a_ort_gegenprobe.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ObjectSource {
    /// Aus dem **geladenen Dokument** (Sichten 3–5, und Sicht 6 auf einem
    /// Strom des Objektgraphen): `lopdf` zählt das Objekt so, und der Fund
    /// steht in genau diesem Objekt. Nur hier darf ein Aufrufer mit der Id
    /// ins geladene Dokument greifen — [`LeakSite::document_object`] gibt
    /// genau diese Fassung heraus.
    Document,
    /// Aus den **Rohbytes** gelesen (Sicht 2, und Sicht 6 auf demselben
    /// Block): der letzte Objektkopf `N G obj` bis zu 64 KiB vor dem Block.
    /// Gegen das geladene Dokument **nicht** geprüft — die Id sagt, wie diese
    /// Bytes einmal beschriftet wurden, nicht, was das geladene Dokument
    /// heute unter dieser Nummer führt.
    RawHeader,
}

/// Der Ort einer Fundstelle, maschinenlesbar — neben ihrem Text.
///
/// Gefüllt wird, was die Sicht **weiß**; geraten wird nichts. Und wo eine
/// Sicht etwas **Schwächeres** weiß, sagt der Ort dazu, was es ist
/// ([`LeakSite::object_source`]) — eine starke und eine schwache Auskunft
/// unter demselben Namen wäre dasselbe Raten, nur auf der anderen Seite.
///
/// * `page` kennt nur [`LeakView::FontDecoder`]: allein diese Sicht läuft
///   über Seiten. Die Sichten 1–6 laufen über Objekte, und ein Strom kann von
///   mehreren Seiten benutzt werden — eine Seitenzahl daneben wäre eine
///   Zusicherung, die an der nächsten Stelle nicht mehr gilt.
/// * `object` kennt jede Sicht, die ein Objekt nennt: die Sichten aus dem
///   Objektgraphen immer, die Rohsicht dann, wenn vor dem Block ein
///   Objektkopf `N G obj` steht. Sicht 1 und Sicht 7 nennen keines.
/// * `object_source` sagt, welche der beiden Quellen es war.
///
/// Was hier `None` ist, heißt also „diese Sicht weiß es nicht“ — nicht „es
/// gibt keine Seite“.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LeakSite {
    /// Welche Sicht den Fund hatte.
    pub view: LeakView,
    /// Die Seite, **1-basiert wie im Text** der Fundstelle: „Seite 3“ ist
    /// `Some(3)`. `None`, wenn die Sicht keine Seite kennt.
    pub page: Option<usize>,
    /// Objektnummer und Generation. Was das zusichert, hängt an
    /// [`LeakSite::object_source`]: „wie `lopdf` sie zählt“ gilt **nur** für
    /// [`ObjectSource::Document`]. `None`, wenn die Sicht kein Objekt kennt.
    ///
    /// Bei [`LeakView::ObjectStream`] ist es das **enthaltene** Objekt — dort
    /// steht der Text; welcher Container es trägt, sagt der Meldungstext.
    /// (Quelle ist trotzdem [`ObjectSource::Document`]: das enthaltene Objekt
    /// steht mit dieser Id im geladenen Dokument.)
    ///
    /// # Warum `Some(id)` allein nichts heißt
    ///
    /// Die Rohsichten übernehmen die Id aus dem Objektkopf **in den
    /// Rohbytes** und fragen das geladene Dokument nicht. Das ist eine wahre
    /// Aussage über die Bytes und eine falsche über das Dokument, sobald
    /// beides auseinanderfällt — an **gewöhnlichen** Dateien:
    ///
    /// * **inkrementelles Update** (jedes „Speichern“ schreibt eines): der
    ///   Klartext liegt in der alten Revision, dieselbe Id trägt im geladenen
    ///   Dokument das neue, geschwärzte Objekt. Wer der Id folgt, liest
    ///   „geschwärzt, also gewollt stehen geblieben“ — genau die
    ///   Verwechslung, gegen die dieses Feld eingeführt wurde
    ///   (Befund ZI-A1; Beleg `tests/zi_a_ort_gegenprobe.rs::zi_a1`,
    ///   Vertrag `tests/zh2_a_ort_herkunft.rs`).
    /// * **eingebettetes PDF ohne Filter** (PDF/A-3, ZUGFeRD): der Kopf
    ///   gehört zur Objektzählung des **inneren** Dokuments.
    /// * **ein Block, den die xref-Tabelle nicht mehr nennt**: die Rohsicht
    ///   findet ihn mit Absicht (daher überlebt die Historie inkrementeller
    ///   Updates überhaupt eine Prüfung) — im geladenen Dokument steht unter
    ///   der Nummer aus seinem Kopf etwas anderes oder nichts.
    ///
    /// Wer ins geladene Dokument greifen will, nimmt deshalb
    /// [`LeakSite::document_object`]; wer die Historie untersucht, nimmt die
    /// Rohsicht-Id — zusammen mit dem Dateioffset, der im Satz steht.
    ///
    /// # Warum die Id nicht geprüft und nicht weggelassen wird
    ///
    /// * **Prüfen** (Id nur nennen, wenn das geladene Dokument sie bestätigt)
    ///   hinge Sicht 2 an der Ladbarkeit der Datei. Diese Sicht ist aber
    ///   genau dafür da, davon **unabhängig** zu sein: lädt `lopdf` die Datei
    ///   nicht, ist sie die einzige Messung, die es noch gibt. Und sie
    ///   verliert eine **richtige** Id in den Lagen, um die es forensisch
    ///   geht — Altrevision, Block ohne xref-Eintrag: dort gibt es im
    ///   geladenen Dokument nichts zu bestätigen, obwohl der Kopf in den
    ///   Bytes steht.
    /// * **Weglassen** wirft dieselbe Auskunft weg, nur immer.
    /// * Falsch war nicht der Wert, sondern die **Zusicherung**. Sie steht
    ///   jetzt am Typ, schwächer und wahr, kostet keine Laufzeit und lässt
    ///   dem Aufrufer beide Fragen.
    pub object: Option<(u32, u16)>,
    /// Woher `object` kommt — `None` **genau dann**, wenn `object` `None`
    /// ist. Die beiden Felder werden zusammen gesetzt; siehe
    /// [`ObjectSource`].
    pub object_source: Option<ObjectSource>,
}

impl LeakSite {
    /// Die Objekt-Id, **wenn** sie aus dem geladenen Dokument stammt — die
    /// Fassung, mit der ein Aufrufer ins Dokument greifen darf.
    ///
    /// `None` heißt hier: entweder kennt diese Sicht kein Objekt, oder ihre
    /// Id kommt aus den Rohbytes und ist ungeprüft
    /// ([`ObjectSource::RawHeader`]). Eine Oberfläche, die „gewollt stehen
    /// geblieben“ von „Schwärzung danebengegangen“ trennen soll, fragt so —
    /// `site.object` allein würde ihr eine Altrevision als geschwärztes,
    /// gewolltes Objekt verkaufen.
    pub fn document_object(&self) -> Option<(u32, u16)> {
        match self.object_source {
            Some(ObjectSource::Document) => self.object,
            _ => None,
        }
    }
}

/// [`leaks_many`] mit einem Budget für entpackte Bytes.
///
/// `max_decompressed_bytes` ist dieselbe Zahl wie `--max-decompressed-mb`
/// der Kommandozeile (in Byte) und zählt dieselbe Einheit wie
/// [`Limits::max_decompressed_bytes`]: die **Summe** der entpackten Bytes
/// über die Ströme der Datei. Das Budget gilt **je Sicht** — die Rohsicht
/// (Sicht 2, jeder `stream … endstream`-Block) und die Objektsicht (Sichten
/// 3 und 4) entpacken die Ströme der Datei je einmal und bekommen je ein
/// volles Budget. Was der Lader der Schwärzung mit demselben Wert
/// durchlässt, prüft das Orakel damit vollständig.
///
/// Ein Strom, der das verbleibende Budget seiner Sicht sprengen würde, wird
/// **nicht** entpackt — das Entpacken selbst bricht an der Grenze ab
/// ([`filters::decoded_content_within`]), es wird nicht erst hinterher
/// gemessen — und steht in [`LeakCheck::unchecked`] mit Objekt-Id und Grund.
/// Seine gepackten Bytes werden trotzdem roh durchsucht.
///
/// Was `lopdf` beim Laden selbst entpackt (Objekt- und Querverweisströme),
/// deckt [`prescan`] mit demselben Budget, **bevor** `Document::load_mem`
/// läuft; lehnt die Vorprüfung ab, fehlen die Sichten 3–7 und `unchecked`
/// sagt es. Sicht 7 (Schriftdekoder) entpackt über den Extraktor ohne eigene
/// Grenze; sie läuft nur, wenn Sicht 3 jeden Strom entpacken konnte — dann
/// ist ihre Arbeit durch dasselbe Budget gedeckt.
///
/// # Spitzenbelegung
///
/// Gleichzeitig im Speicher stehen: die Dateibytes, das geladene Dokument
/// daneben (dessen Objektströme `lopdf` entpackt hält, durch die Vorprüfung
/// ≤ Budget) und **ein Strom in Arbeit**. Der Strom in Arbeit kostet
/// höchstens das verbleibende Budget + 1 Byte — jeder Filter misst **beim**
/// Entpacken —, dazu bei einer Kette gleichzeitig Eingabe und Ausgabe des
/// laufenden Gliedes und, während er durchsucht wird, die aus ihm gebildete
/// Zeichenketten-Verkettung ([`concat_pdf_strings`], höchstens noch einmal
/// dieselbe Größe).
///
/// Die **Rohbytes** eines Stroms werden dafür nicht kopiert: bis Fix-Runde 6
/// klonte `filters::decode_chain` sie, bevor es den ersten Filter kannte, und
/// warf den Klon bei einem unbekannten ersten Glied wieder weg. Gemessen
/// (Release, Kindprozess, `VmHWM`, 64-MiB-Strom, Budget 512 MiB,
/// `tests/zf_q2_teildekoder.rs`): ohne `/Filter` 132 MB, mit
/// `/Filter /DCTDecode` **196 MB** vorher und 132 MB nachher. (MB ist hier
/// wie überall 1024² Byte; bis Fix-Runde 6 rechnete die Messausgabe in
/// Dezimal-MB und nannte dieselben Läufe 205 und 138 MB.)
///
/// Das Budget ist eine Obergrenze für das, was **entpackt** wird, keine
/// Zusage über die Größe des Prozesses: derselbe 64-MiB-Strom als
/// `/FlateDecode`, dessen Bytes sich als roher Deflate-Strom auf gut das
/// Doppelte aufblasen lassen, kommt bei 512 MiB Budget auf 593 MB — entpackte
/// Bytes, ihre Verkettung, und zwei Sichten hintereinander. Wer das Budget
/// hochdreht, kauft Speicher, nicht nur Erlaubnis.
///
/// Gemessen (`tests/zd_orakel_budget.rs`): 1 GiB Nullen, 1 MB gepackt,
/// Budget 16 MiB → unter 100 MB, unter 2 s.
pub fn leaks_many_within(
    pdf_bytes: &[u8],
    needles: &[&str],
    max_decompressed_bytes: u64,
) -> LeakCheck {
    let mut probe = Probe::new(needles);
    if probe.needles.is_empty() {
        let (findings, literal, sites) = probe.into_hits();
        return LeakCheck {
            findings,
            literal,
            sites,
            unchecked: Vec::new(),
            unchecked_places: 0,
        };
    }

    scan_raw_file(pdf_bytes, &mut probe);
    scan_raw_string_literals(pdf_bytes, &mut probe);
    let mut raw_budget = Budget::new(max_decompressed_bytes);
    let (raw_streams, raw_gaps) = scan_raw_streams(pdf_bytes, &mut probe, &mut raw_budget);
    // Stellen zählen, bevor die Decke aus vielen Stellen eine Zeile macht.
    let mut places = raw_budget.places();
    let mut unchecked = raw_budget.into_unchecked();

    // Lässt sich die Datei nicht laden, bleiben die Rohsuchen die Messung —
    // und `unchecked` sagt, dass es nur die waren.
    let limits = Limits {
        max_decompressed_bytes,
        // Syntax-Budget und Objektdecke gehören zur Schwärzung; hier zählt
        // nur, was entpackt wird.
        max_parsed_bytes: u64::MAX,
        ..Limits::default()
    };
    let doc = match prescan(pdf_bytes, &limits).map_err(|e| e.to_string()) {
        Ok(()) => Document::load_mem(pdf_bytes).map_err(|e| e.to_string()),
        Err(e) => Err(format!("die Vorprüfung des Laders lehnt die Datei ab: {e}")),
    };
    match doc {
        Ok(doc) => {
            // Was der Lader anders las, als es in den Rohbytes steht, haben
            // die Sichten 3–7 nicht gesehen (Register #98).
            let (mut streams, covered) = misread_streams(pdf_bytes, &raw_streams, &doc);
            // Eine gescheiterte Kette der Rohsicht zählt nur, wo keine
            // Objektsicht denselben Strom gelesen hat — dort meldet sie die
            // Objektsicht selbst (Register #95). Ein verschlüsseltes Dokument
            // vergleicht niemand; dort bleibt es bei der Objektsicht.
            if let Some(covered) = covered {
                streams.extend(
                    raw_gaps
                        .into_iter()
                        .filter(|(block, _)| !covered.contains(block))
                        .map(|(_, line)| line),
                );
            }
            places += streams.len();
            push_capped(&mut unchecked, streams, "Ströme");
            let mut budget = Budget::new(max_decompressed_bytes);
            scan_object_graph(&doc, &mut probe, &mut budget);
            let skipped = budget.skipped;
            places += budget.places();
            unchecked.extend(budget.into_unchecked());
            if skipped == 0 {
                let gaps = scan_decoded_text(&doc, &mut probe);
                places += gaps.len();
                push_capped(
                    &mut unchecked,
                    gaps.into_iter()
                        .map(|gap| format!("Sicht 7 (Schriftdekoder): {gap}"))
                        .collect(),
                    "Seiten",
                );
            } else {
                // Eine ganze Sicht, die nicht lief: eine Stelle mehr, auch
                // wenn sie kein einzelnes Objekt nennt.
                places += 1;
                unchecked.push(format!(
                    "Sicht 7 (Schriftdekoder) nicht gelaufen: {skipped} Strom/Ströme \
                     wurden nicht entpackt, und der Schriftdekoder entpackt ohne \
                     eigene Grenze"
                ));
            }
        }
        Err(reason) => {
            places += 1;
            unchecked.push(format!(
                "Objektgraph (Sichten 3–7) nicht durchsucht — {reason}"
            ));
            let lines: Vec<String> = raw_gaps.into_iter().map(|(_, line)| line).collect();
            places += lines.len();
            push_capped(&mut unchecked, lines, "Ströme");
        }
    }

    let (findings, literal, sites) = probe.into_hits();
    LeakCheck {
        findings,
        literal,
        sites,
        unchecked,
        unchecked_places: places,
    }
}

// ---------------------------------------------------------------------------
// Budget
// ---------------------------------------------------------------------------

/// Was eine Sicht noch entpacken darf, und was sie deshalb ausgelassen hat.
struct Budget {
    remaining: u64,
    total: u64,
    skipped: usize,
    /// Stellen, die aus einem **anderen** Grund als dem Budget offen blieben
    /// (unbekanntes Glied einer Filterkette, Tiefengrenze) — gezählt, damit
    /// eine Datei, die davon tausende hat, nicht tausend Zeilen erzeugt.
    noted: usize,
    unchecked: Vec<String>,
}

impl Budget {
    fn new(total: u64) -> Self {
        Self {
            remaining: total,
            total,
            skipped: 0,
            noted: 0,
            unchecked: Vec::new(),
        }
    }

    /// Wie viele Bytes der nächste Strom höchstens ergeben darf.
    fn room(&self) -> usize {
        usize::try_from(self.remaining).unwrap_or(usize::MAX)
    }

    /// Bucht einen entpackten Strom.
    fn charge(&mut self, bytes: usize) {
        self.remaining = self
            .remaining
            .saturating_sub(u64::try_from(bytes).unwrap_or(u64::MAX));
    }

    /// Merkt einen Strom vor, der nicht entpackt wurde.
    fn skip(&mut self, what: &str, packed: usize) {
        self.skipped += 1;
        if self.skipped <= MAX_UNCHECKED {
            self.unchecked.push(format!(
                "{what}: nicht entpackt — {packed} Byte gepackt, entpackt mehr als \
                 die verbleibenden {} von {} Byte des Budgets (--max-decompressed-mb); \
                 die gepackten Bytes wurden roh durchsucht",
                self.remaining, self.total
            ));
        }
    }

    /// Wie viele **Stellen** diese Sicht offen ließ — auch die, die hinter
    /// der Decke [`MAX_UNCHECKED`] in einer Summenzeile verschwinden.
    ///
    /// Das ist die Zahl für [`LeakCheck::unchecked_places`]; die Zahl der
    /// Zeilen (`unchecked.len()`) ist bei voller Decke **kleiner**.
    fn places(&self) -> usize {
        self.skipped + self.noted
    }

    /// Merkt eine Stelle, die aus einem anderen Grund als dem Budget offen
    /// blieb — mit derselben Decke wie [`Self::skip`].
    fn note(&mut self, message: String) {
        self.noted += 1;
        if self.noted <= MAX_UNCHECKED {
            self.unchecked.push(message);
        }
    }

    fn into_unchecked(mut self) -> Vec<String> {
        if self.skipped > MAX_UNCHECKED {
            self.unchecked.push(format!(
                "… und {} weitere Ströme nicht entpackt",
                self.skipped - MAX_UNCHECKED
            ));
        }
        if self.noted > MAX_UNCHECKED {
            self.unchecked.push(format!(
                "… und {} weitere Stellen nicht geprüft",
                self.noted - MAX_UNCHECKED
            ));
        }
        self.unchecked
    }
}

// ---------------------------------------------------------------------------
// Suchmuster
// ---------------------------------------------------------------------------

/// Die gesuchte Zeichenkette in allen Kodierungen, in denen sie in einer
/// PDF-Datei stehen kann.
struct Needle {
    text: String,
    /// Ohne jeden Leerraum — fängt Text ab, dessen Zwischenräume im PDF nicht
    /// als Leerzeichen, sondern als Positionierung stehen.
    ///
    /// `None`, wenn die Suchzeichenkette gar keinen Leerraum enthält: dann
    /// bringt der Vergleich nichts und würde nur Fehlalarme über
    /// Fragmentgrenzen hinweg erzeugen („MODE“ + „89 EUR“ → „DE89“). Ebenso
    /// `None`, wenn nichts übrig bleibt — ein leeres Muster stünde überall.
    squeezed: Option<String>,
    /// Bytefolgen: (Beschreibung, Muster). Gesucht werden sie nicht einzeln,
    /// sondern alle zusammen im [`Matcher`] der [`Probe`].
    variants: Vec<(&'static str, Vec<u8>)>,
}

impl Needle {
    fn new(text: &str) -> Self {
        let mut variants: Vec<(&'static str, Vec<u8>)> = Vec::new();

        let utf8 = text.as_bytes().to_vec();
        variants.push(("UTF-8/ASCII", utf8.clone()));

        // Latin-1 / PDFDocEncoding: ein Byte je Zeichen.
        if text.chars().all(|c| (c as u32) < 0x100) {
            let latin1: Vec<u8> = text.chars().map(|c| c as u8).collect();
            if latin1 != utf8 {
                variants.push(("Latin-1/PDFDoc", latin1.clone()));
            }
            variants.push(("Hex-String (Latin-1, gross)", hex_ascii(&latin1, true)));
            variants.push(("Hex-String (Latin-1, klein)", hex_ascii(&latin1, false)));
        }

        let utf16: Vec<u8> = text
            .encode_utf16()
            .flat_map(|u| u.to_be_bytes())
            .collect::<Vec<u8>>();
        variants.push(("UTF-16BE", utf16.clone()));
        variants.push(("Hex-String (UTF-16BE, gross)", hex_ascii(&utf16, true)));
        variants.push(("Hex-String (UTF-16BE, klein)", hex_ascii(&utf16, false)));

        // UTF-16**LE** ist im PDF nicht vorgesehen, kommt aber vor —
        // [`decode_pdf_string`] liest den BOM `FF FE` ausdrücklich. Ein
        // Zeichenketten-**Objekt** ist damit abgedeckt; LE-Bytes **in einem
        // Strom** (oder in der Rohdatei, wo niemand parst) waren es nicht.
        // Der Preis ist drei Muster mehr je Begriff, und der Automat läuft
        // einmal je Datenblock, egal wie viele Muster er kennt: gemessen an
        // einer Datei mit 8 MiB entpacktem Strom 28,7 ms mit einem Begriff
        // und 29,6 ms mit 200 (`ze_p1_mess_kosten_je_kodierung`).
        let utf16le: Vec<u8> = text
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<u8>>();
        variants.push(("UTF-16LE", utf16le.clone()));
        variants.push(("Hex-String (UTF-16LE, gross)", hex_ascii(&utf16le, true)));
        variants.push(("Hex-String (UTF-16LE, klein)", hex_ascii(&utf16le, false)));

        variants.dedup_by(|a, b| a.1 == b.1);

        let squeezed = squeeze(text);
        Self {
            text: text.to_string(),
            squeezed: (squeezed != text && !squeezed.is_empty()).then_some(squeezed),
            variants,
        }
    }
}

/// Ein Automat über viele Muster — ein Durchgang je Datenblock, gleich wie
/// viele Begriffe gesucht werden.
///
/// Jedes Muster kennt seinen Platz: `(Begriff, Kodierung)` in der [`Probe`].
/// Gesucht wird **überlappend** (`MatchKind::Standard`,
/// `find_overlapping_iter`): „Max Mustermann“ und „Mustermann“ müssen beide
/// treffen, und derselbe Text in zwei Kodierungen ebenso.
struct Matcher {
    automaton: AhoCorasick,
    /// Je Muster-Id: (Index des Begriffs in `Probe::needles`, Index der
    /// Kodierung in `Needle::variants`).
    slots: Vec<(usize, usize)>,
    /// Je Muster-Id: seine Länge in Bytes.
    lens: Vec<usize>,
}

impl Matcher {
    fn new(patterns: Vec<((usize, usize), Vec<u8>)>) -> Self {
        let (slots, bytes): (Vec<(usize, usize)>, Vec<Vec<u8>>) = patterns.into_iter().unzip();
        let lens = bytes.iter().map(Vec::len).collect();
        let automaton = AhoCorasick::builder()
            .match_kind(MatchKind::Standard)
            .build(&bytes)
            // Scheitert nur, wenn die Muster den Adressraum des Automaten
            // sprengen — bei Suchbegriffen aus einer Kommandozeile oder
            // Oberfläche nicht erreichbar. Leise nichts zu finden wäre die
            // falsche Antwort eines Messgeräts.
            .expect("Suchbegriffe passen nicht in den Aho-Corasick-Automaten");
        Self {
            automaton,
            slots,
            lens,
        }
    }

    /// Fundstellen je Muster: überlappungsfrei **je Muster**, aufsteigend,
    /// höchstens `limit` — Stelle für Stelle dasselbe, was
    /// `memmem::find_iter(hay).take(limit)` je Muster lieferte (bis
    /// Fix-Runde 4 die Suche, `positions_agree_with_the_naive_search`).
    ///
    /// Der Automat meldet Treffer nach Endposition; bei fester Musterlänge
    /// ist das je Muster die Reihenfolge der Anfänge. Ein Treffer, der in
    /// den vorigen desselben Musters hineinragt, zählt nicht — genau wie
    /// eine Suche, die hinter dem Treffer weitersucht. Sobald jedes Muster
    /// seine Grenze erreicht hat, endet der Durchgang.
    fn positions(&self, hay: &[u8], limit: usize) -> Vec<Vec<usize>> {
        let mut out: Vec<Vec<usize>> = vec![Vec::new(); self.slots.len()];
        if limit == 0 || out.is_empty() {
            return out;
        }
        let mut done = 0usize;
        for found in self.automaton.find_overlapping_iter(hay) {
            let id = found.pattern().as_usize();
            let hits = &mut out[id];
            if hits.len() >= limit {
                continue;
            }
            if hits
                .last()
                .is_some_and(|&last| found.start() < last + self.lens[id])
            {
                continue;
            }
            hits.push(found.start());
            if hits.len() == limit {
                done += 1;
                if done == out.len() {
                    break;
                }
            }
        }
        out
    }
}

/// Alle Suchbegriffe eines Laufs samt ihren Fundstellen und Automaten.
///
/// Der Durchgang durch die Datei kennt nur noch dieses eine Bündel, deshalb
/// wird jeder Stream einmal entpackt, jede Zeichenkette einmal dekodiert und
/// jeder Block einmal durchlaufen — unabhängig davon, wie viele Begriffe
/// gesucht werden.
struct Probe {
    /// Die nicht-leeren Begriffe. Ein leerer Begriff stünde in jeder Datei;
    /// er wird nicht gesucht, behält aber unten seinen (leeren) Platz.
    needles: Vec<Needle>,
    reports: Vec<Report>,
    /// Zu jedem Eintrag oben: seine Position in der Eingabe.
    slots: Vec<usize>,
    /// Anzahl der Begriffe in der Eingabe, inklusive der leeren.
    total: usize,
    /// Sucht mindestens ein Begriff auch ohne Leerraum? Nur dann lohnt es,
    /// den Leerraum aus einem Datenblock zu entfernen.
    any_squeezed: bool,
    /// Alle Kodierungen aller Begriffe — für Bytes.
    bytes: Matcher,
    /// Je Begriff sein Text — für dekodierten Text. Muster-Id = Index in
    /// `needles`.
    text: Matcher,
    /// Die Fassungen ohne Leerraum — für Text ohne Leerraum. `slots` nennt
    /// den Begriff.
    squeezed: Matcher,
}

impl Probe {
    fn new(input: &[&str]) -> Self {
        let mut needles = Vec::new();
        let mut slots = Vec::new();
        for (slot, text) in input.iter().enumerate() {
            if text.is_empty() {
                continue;
            }
            needles.push(Needle::new(text));
            slots.push(slot);
        }
        let mut bytes = Vec::new();
        let mut text = Vec::new();
        let mut squeezed = Vec::new();
        for (n, needle) in needles.iter().enumerate() {
            for (v, (_, pattern)) in needle.variants.iter().enumerate() {
                bytes.push(((n, v), pattern.clone()));
            }
            text.push(((n, 0), needle.text.as_bytes().to_vec()));
            if let Some(s) = &needle.squeezed {
                squeezed.push(((n, 1), s.as_bytes().to_vec()));
            }
        }
        Self {
            reports: needles.iter().map(|_| Report::default()).collect(),
            any_squeezed: !squeezed.is_empty(),
            bytes: Matcher::new(bytes),
            text: Matcher::new(text),
            squeezed: Matcher::new(squeezed),
            needles,
            slots,
            total: input.len(),
        }
    }

    /// Byteweise Suche in `hay` — alle Kodierungen aller Begriffe in einem
    /// Durchgang; `site` und `how` beschreiben die Fundstelle.
    fn scan_bytes(&mut self, hay: &[u8], limit: usize, site: Site<'_>, how: &str) {
        let found = self.bytes.positions(hay, limit);
        for (id, positions) in found.iter().enumerate() {
            let (n, v) = self.bytes.slots[id];
            let variant = self.needles[n].variants[v].0;
            for &pos in positions {
                self.reports[n].hit_bytes(
                    site,
                    &format!("{how}, {variant}"),
                    hay,
                    pos,
                    self.bytes.lens[id],
                );
            }
        }
    }

    /// Erste Fundstelle je Begriff in dekodiertem Text — `text` steht für
    /// die Muster mit Leerraum (Muster-Id = Begriff), `squeezed` für die
    /// ohne. Zurück kommt je Begriff, ob der Text selbst getroffen hat.
    fn scan_text_plain(&mut self, text: &str, site: Site<'_>, how: &str) -> Vec<bool> {
        let found = self.text.positions(text.as_bytes(), 1);
        found
            .iter()
            .enumerate()
            .map(|(n, positions)| {
                let Some(&pos) = positions.first() else {
                    return false;
                };
                self.reports[n].hit_text(site, how, text, pos, self.text.lens[n], true);
                true
            })
            .collect()
    }

    /// Wie [`Self::scan_text_plain`], für die Fassung ohne Leerraum;
    /// `only` sagt, welche Begriffe überhaupt gemeldet werden sollen.
    fn scan_text_squeezed(
        &mut self,
        squeezed: &str,
        site: Site<'_>,
        how: &str,
        only: impl Fn(usize) -> bool,
    ) {
        let found = self.squeezed.positions(squeezed.as_bytes(), 1);
        for (id, positions) in found.iter().enumerate() {
            let (n, _) = self.squeezed.slots[id];
            if !only(n) {
                continue;
            }
            if let Some(&pos) = positions.first() {
                self.reports[n].hit_text(site, how, squeezed, pos, self.squeezed.lens[id], false);
            }
        }
    }

    /// Die Fundstellen, die Wörtlich-Marken und die Orte, je in der
    /// Reihenfolge der Eingabe (leere Begriffe behalten ihren Platz: keine
    /// Funde, nicht wörtlich).
    ///
    /// Orte und Fundstellen kommen Eintrag für Eintrag in derselben
    /// Reihenfolge heraus, weil [`Report::push`] beide in einem Schritt
    /// anhängt — siehe [`LeakCheck::sites`].
    fn into_hits(self) -> (Vec<Vec<String>>, Vec<bool>, Vec<Vec<LeakSite>>) {
        let mut hits = vec![Vec::new(); self.total];
        let mut literal = vec![false; self.total];
        let mut sites = vec![Vec::new(); self.total];
        for (slot, report) in self.slots.into_iter().zip(self.reports) {
            hits[slot] = report.hits;
            literal[slot] = report.literal;
            sites[slot] = report.sites;
        }
        (hits, literal, sites)
    }
}

/// Entfernt jeden Leerraum — beide Seiten eines Vergleichs werden so behandelt.
///
/// Öffentlich, damit die Oberfläche ihre Entscheidung „gesucht oder bewusst
/// stehen gelassen“ auf **derselben** Normalform trifft, auf der hier
/// gesucht wird (Befund G5-B1: „DE89 3704 …“ und „DE893704…“ sind ein Text).
pub fn squeeze(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

fn hex_ascii(bytes: &[u8], upper: bool) -> Vec<u8> {
    let digits: &[u8; 16] = if upper {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    let mut out = Vec::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(digits[(b >> 4) as usize]);
        out.push(digits[(b & 0x0f) as usize]);
    }
    out
}

// ---------------------------------------------------------------------------
// Fundstellen sammeln
// ---------------------------------------------------------------------------

/// Wo ein Fund liegt: der Text für die Meldung **und** derselbe Ort
/// maschinenlesbar.
///
/// Ein Bündel statt zweier Parameter, damit der Ort denselben Weg durch die
/// Sichten nimmt wie der Text und nicht auf halber Strecke verloren geht.
/// Was die Sicht nicht weiß, bleibt `None` — siehe [`LeakSite`].
#[derive(Clone, Copy)]
struct Site<'a> {
    text: &'a str,
    view: LeakView,
    page: Option<usize>,
    /// Id **und** Herkunft in einem Feld: so kann keine Id ohne ihre
    /// Herkunft weiterreisen (die Zusicherung von [`LeakSite::object`] hängt
    /// daran).
    object: Option<((u32, u16), ObjectSource)>,
}

impl<'a> Site<'a> {
    fn new(text: &'a str, view: LeakView) -> Self {
        Self {
            text,
            view,
            page: None,
            object: None,
        }
    }

    /// Dieselbe Stelle, tiefer im Pfad — der Ort bleibt, der Text wächst.
    fn with_text<'b>(self, text: &'b str) -> Site<'b> {
        Site {
            text,
            view: self.view,
            page: self.page,
            object: self.object,
        }
    }

    /// Ab hier liest eine andere Sicht.
    fn with_view(self, view: LeakView) -> Self {
        Self { view, ..self }
    }

    /// Eine Id aus dem geladenen Dokument.
    fn with_object(self, object: (u32, u16)) -> Self {
        Self {
            object: Some((object, ObjectSource::Document)),
            ..self
        }
    }

    /// Eine Id aus dem Objektkopf in den **Rohbytes** — ungeprüft, siehe
    /// [`ObjectSource::RawHeader`].
    fn with_raw_object(self, object: (u32, u16)) -> Self {
        Self {
            object: Some((object, ObjectSource::RawHeader)),
            ..self
        }
    }

    fn with_page(self, page: usize) -> Self {
        Self {
            page: Some(page),
            ..self
        }
    }

    /// Der Ort ohne den Meldungstext — das, was der Aufrufer bekommt.
    fn without_text(self) -> LeakSite {
        LeakSite {
            view: self.view,
            page: self.page,
            object: self.object.map(|(id, _)| id),
            object_source: self.object.map(|(_, source)| source),
        }
    }
}

#[derive(Default)]
struct Report {
    hits: Vec<String>,
    /// Zu jedem Eintrag in `hits` sein Ort — gleiche Länge, gleicher Index.
    sites: Vec<LeakSite>,
    seen: BTreeSet<String>,
    /// Hat der Begriff **wörtlich** getroffen? Siehe [`LeakCheck::literal`].
    /// Wird auch gesetzt, wenn die Meldung selbst wegfällt (Doppelung oder
    /// [`MAX_HITS`]) — getroffen hat er trotzdem.
    literal: bool,
}

impl Report {
    fn push(&mut self, message: String, site: LeakSite, literal: bool) {
        self.literal |= literal;
        if self.hits.len() < MAX_HITS && self.seen.insert(message.clone()) {
            self.hits.push(message);
            // Beides in einem Schritt: `sites[i]` gehört zu `hits[i]`, und
            // das ist die Zusicherung von [`LeakCheck::sites`].
            self.sites.push(site);
        }
    }

    /// Fund in einer Bytefolge — immer wörtlich: die Kodierungen eines
    /// Begriffs werden aus seinem Text gebildet, nicht aus der gequetschten
    /// Fassung.
    fn hit_bytes(&mut self, site: Site<'_>, how: &str, hay: &[u8], pos: usize, len: usize) {
        let ctx = printable_context(hay, pos, len);
        self.push(
            format!("{} [{how}]: …{ctx}…", site.text),
            site.without_text(),
            true,
        );
    }

    /// Fund in bereits dekodiertem Text; `literal` sagt, ob der Text selbst
    /// getroffen hat oder nur seine Fassung ohne Leerraum.
    fn hit_text(
        &mut self,
        site: Site<'_>,
        how: &str,
        hay: &str,
        pos: usize,
        len: usize,
        literal: bool,
    ) {
        let ctx = printable_context(hay.as_bytes(), pos, len);
        self.push(
            format!("{} [{how}]: …{ctx}…", site.text),
            site.without_text(),
            literal,
        );
    }
}

fn printable_context(hay: &[u8], pos: usize, len: usize) -> String {
    let start = pos.saturating_sub(CONTEXT);
    let end = (pos + len + CONTEXT).min(hay.len());
    hay[start..end]
        .iter()
        .map(|&b| {
            if (0x20..0x7f).contains(&b) {
                b as char
            } else {
                '.'
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Ebene 1+2: Rohdatei und rohe Streams
// ---------------------------------------------------------------------------

/// Sicht 1, zweiter Gang: jedes Zeichenketten-Literal in den Rohbytes
/// **außerhalb** der Stream-Blöcke, dekodiert wie ein Zeichenketten-Objekt —
/// oktale Maskierung (`\104\105…`, auch UTF-16BE so maskiert, wie pdfTeX es
/// schreibt), Hex-Strings mit Leerraum, Zeilenfortsetzung. Der Bytevergleich
/// der Rohsicht trifft keine dieser Schreibweisen, und ein Objekt der
/// Altgeneration steht in keiner Objektsicht mehr: bis zur Spur-A-Runde 1
/// war das ein stilles Leck (Register #80). Die Stream-Blöcke bleiben außen
/// vor — ihre Literale liest die Rohsicht der Ströme (Sicht 6).
fn scan_raw_string_literals(bytes: &[u8], probe: &mut Probe) {
    let blocks = raw_stream_blocks(bytes);
    let mut next_block = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        // Einen Stream-Block überspringen, sobald er beginnt.
        if let Some(&(start, payload)) = blocks.get(next_block) {
            if i >= start {
                i = start + payload.len();
                next_block += 1;
                continue;
            }
        }
        match bytes[i] {
            b'(' => {
                let (raw, next) = read_literal_string(bytes, i + 1);
                scan_raw_literal(&raw, i, probe);
                i = next;
            }
            b'<' if bytes.get(i + 1) != Some(&b'<') => {
                let (raw, next) = read_hex_string(bytes, i + 1);
                scan_raw_literal(&raw, i, probe);
                i = next;
            }
            _ => i += 1,
        }
    }
}

fn scan_raw_literal(raw: &[u8], offset: usize, probe: &mut Probe) {
    if raw.is_empty() {
        return;
    }
    // Der Ort ist die Rohdatei, wie bei der Bytesuche; wie verglichen
    // wurde, sagt das Etikett in Klammern. So liest jeder, der den Satz
    // nach „Ort [Wie]“ zerlegt, dieselbe Sicht wie aus dem Ort.
    let base = format!("Rohdatei @0x{offset:x}");
    let site = Site::new(&base, LeakView::RawFile);
    scan_text(
        &decode_pdf_string(raw),
        site,
        "Zeichenkette (dekodiert)",
        probe,
    );
}

fn scan_raw_file(bytes: &[u8], probe: &mut Probe) {
    let found = probe.bytes.positions(bytes, 8);
    for (id, positions) in found.iter().enumerate() {
        let (n, v) = probe.bytes.slots[id];
        let variant = probe.needles[n].variants[v].0;
        for &pos in positions {
            probe.reports[n].hit_bytes(
                Site::new(&format!("Rohdatei @0x{pos:x}"), LeakView::RawFile),
                variant,
                bytes,
                pos,
                probe.bytes.lens[id],
            );
        }
    }
}

/// Findet jeden `stream … endstream`-Block anhand der Rohbytes — unabhängig
/// davon, ob die xref-Tabelle das Objekt noch kennt. Genau so überlebt die
/// Historie inkrementeller Updates.
/// Liefert die Ströme mit Objektkopf und Dictionary (für den Vergleich mit
/// dem, was der Lader daraus machte, [`misread_streams`]) und je Block, an
/// dem die Kette stehen blieb, seine Zeile ([`chain_gap`]).
fn scan_raw_streams(bytes: &[u8], probe: &mut Probe, budget: &mut Budget) -> RawStreams {
    let mut kopf = Vec::new();
    let mut gaps = Vec::new();
    let mut definitions = RawDefinitions::new(bytes);
    for (offset, payload) in raw_stream_blocks(bytes) {
        let header = object_header(bytes, offset);
        let base = format!("Rohdaten-Stream @0x{offset:x}{}", object_label(header));
        // Der Objektkopf steht hier in **Rohbytes**, nicht im Objektgraphen:
        // was er nicht hergibt (kein Kopf in Reichweite, eine Zahl, die in
        // keinen `u32` passt), bleibt `None` statt geraten zu werden. Und was
        // er hergibt, reist als `ObjectSource::RawHeader` weiter — im
        // geladenen Dokument kann unter dieser Nummer etwas anderes stehen
        // (Altrevision, eingebettetes PDF, verwaistes Objekt).
        let mut site = Site::new(&base, LeakView::RawStream);
        if let Some(id) = object_id(header) {
            site = site.with_raw_object(id);
        }
        scan_blob(payload, site.with_text(&format!("{base} (roh)")), probe);
        // Zuerst über die Filterkette, die das rohe Dictionary vor dem Block
        // nennt — mit demselben Dekoder wie die Objektsicht. Bis zur
        // Spur-A-Runde 1 versuchte diese Sicht nur zlib und rohes Deflate:
        // eine Altgeneration unter `/LZWDecode`, `/ASCII85Decode`, ASCIIHex
        // mit Zeilenumbrüchen, Flate mit Prädiktor oder `[/ASCII85Decode
        // /FlateDecode]` stand in keiner Querverweistabelle mehr und damit in
        // keiner Objektsicht — kein Fund, keine Meldung (Register #80).
        // Erst wenn die Kette nichts hergibt (kein Kopf, kein `/Filter`,
        // schon das erste Glied unbekannt), bleibt der blinde Versuch mit
        // zlib und rohem Deflate: ein Block ohne Kopf, ein eingebettetes PDF.
        // Gebucht wird je Block einmal.
        let dict = raw_stream_dict(bytes, offset);
        if let (Some(id), Some(_)) = (object_id(header), dict) {
            kopf.push((id, offset));
        }
        let chain = match dict {
            Some(dict) => decode_raw_chain(dict, payload, offset, &mut definitions, budget.room()),
            None => Ok(RawChain::default()),
        };
        match chain {
            Ok(chain) => {
                // Die Arbeit der Kette, nicht die Ausgabe ihres letzten
                // Glieds (Register #99).
                budget.charge(chain.work);
                if let Some(gap) = chain.gap {
                    gaps.push((offset, format!("{base}: {gap}")));
                }
                if let Some((decoded, names)) = chain.decoded {
                    // Reines Flate ohne Prädiktor ist, was der blinde Versuch
                    // immer schon tat — und heißt in der Fundstelle weiter so.
                    let wie = if names == "FlateDecode" {
                        "inflate".to_string()
                    } else {
                        format!("dekodiert: {names}")
                    };
                    scan_blob(&decoded, site.with_text(&format!("{base} ({wie})")), probe);
                    continue;
                }
            }
            Err(Oversize) => {
                budget.skip(&base, payload.len());
                continue;
            }
        }
        match inflate_raw(payload, budget.room()) {
            Ok(Some(inflated)) => {
                budget.charge(inflated.len());
                scan_blob(
                    &inflated,
                    site.with_text(&format!("{base} (inflate)")),
                    probe,
                );
            }
            Ok(None) => {}
            Err(Oversize) => budget.skip(&base, payload.len()),
        }
    }
    (kopf, gaps)
}

/// Was [`scan_raw_streams`] für die Zeit nach dem Laden zurücklässt: die
/// Ströme mit Objektkopf und die Zeilen gescheiterter Ketten, je mit dem
/// Offset ihres Blocks.
type RawStreams = (Vec<((u32, u16), usize)>, Vec<(usize, String)>);

/// Das rohe Stream-Dictionary vor einem `stream`-Block: die Bytes zwischen
/// dem Objektkopf `N G obj` und dem Schlüsselwort. `None`, wenn kein Kopf in
/// Reichweite steht.
fn raw_stream_dict(bytes: &[u8], data_offset: usize) -> Option<&[u8]> {
    let from = data_offset.saturating_sub(OBJECT_HEADER_LOOKBACK);
    let window = &bytes[from..data_offset];
    let keyword = memmem::rfind(window, b"stream")?;
    let obj = memmem::rfind(&window[..keyword], b"obj")?;
    if window[..obj].ends_with(b"end") {
        return None;
    }
    Some(&window[obj + 3..keyword])
}

/// Was die Kette eines rohen Blocks ergab.
#[derive(Default)]
struct RawChain {
    /// Die Bytes nach dem letzten angewandten Glied und die Namen der
    /// angewandten Glieder — `None`, wenn kein Glied lief.
    decoded: Option<(Vec<u8>, String)>,
    /// Die Arbeit der Kette (Register #99).
    work: usize,
    /// Wo die Kette stehen blieb, mit Grund ([`chain_gap`]).
    gap: Option<String>,
}

/// Entpackt einen rohen Block über die Filterkette seines Dictionaries —
/// derselbe Dekoder wie in der Objektsicht, nur dass `/Filter` und
/// `/DecodeParms` aus den Rohbytes gelesen werden: die Altrevision eines
/// inkrementellen Updates steht in keinem Objektgraphen mehr.
///
/// Gelesen wird mit dem Wortzerleger aus [`crate::document`], nicht an
/// Leerraum getrennt: `/Filter[/ASCII85Decode/FlateDecode]` und
/// `/DecodeParms<</Predictor 12/Columns 5>>` ohne ein Leerzeichen sind
/// gewöhnliche Ausgabe von iText (Register #95). Ein Verweis darin wird aus
/// den Rohbytes aufgelöst ([`RawDefinitions`]).
fn decode_raw_chain(
    dict: &[u8],
    payload: &[u8],
    offset: usize,
    definitions: &mut RawDefinitions<'_>,
    limit: usize,
) -> Result<RawChain, Oversize> {
    let Some(filter) = raw_dict_entry(dict, b"Filter") else {
        return Ok(RawChain::default());
    };
    let mut stream_dict = lopdf::Dictionary::new();
    stream_dict.set("Filter", filter);
    if let Some(parms) = raw_dict_entry(dict, b"DecodeParms") {
        stream_dict.set("DecodeParms", parms);
    }
    let doc = definitions.scratch(&stream_dict, offset);
    let stream = lopdf::Stream::new(stream_dict, payload.to_vec());
    let names = filters::filter_names(&doc, &stream.dict).unwrap_or_default();
    if names.is_empty() {
        return Ok(RawChain::default());
    }
    let (data, applied, work) = filters::decoded_prefix_counted(&doc, &stream, limit)?;
    let decoded = (applied > 0).then(|| {
        let mut label = names[..applied]
            .iter()
            .map(|name| String::from_utf8_lossy(name).into_owned())
            .collect::<Vec<_>>()
            .join("+");
        if stream.dict.has(b"DecodeParms") {
            label.push_str(" mit DecodeParms");
        }
        (data, label)
    });
    Ok(RawChain {
        decoded,
        work,
        gap: chain_gap(&names, applied),
    })
}

/// So viele Bytes hinter `N G obj` liest eine Definition höchstens: ein
/// Filtername oder ein `/DecodeParms`-Dictionary ist kurz.
const RAW_DEFINITION_WINDOW: usize = 1024;

/// Die Definitionen `N G obj` in den Rohbytes — für Verweise in einem rohen
/// Stream-Dictionary (`/Filter 7 0 R`, `/DecodeParms 9 0 R`).
///
/// Der Verzeichnis entsteht erst beim ersten Verweis, in einem Durchgang über
/// die Datei. Gilt eine Nummer mehrmals (inkrementelle Updates), gilt die
/// Definition, die dem Block am nächsten steht: die Objekte einer Revision
/// stehen beieinander, und ein Erzeuger schreibt das Parameterobjekt gleich
/// vor oder hinter den Strom. Jede Definition wird einmal gelesen, bis zu
/// ihrem `endobj`, höchstens [`RAW_DEFINITION_WINDOW`] Byte; zusammen
/// höchstens so viele Bytes, wie die Datei hat. Eine gewöhnliche Datei
/// erreicht das nie — ihre Definitionen überlappen nicht —, eine Datei aus
/// lauter `N 0 obj` ohne `endobj` bleibt so linear (die Klasse aus Register
/// #100). Was danach kommt, bleibt unaufgelöst.
struct RawDefinitions<'a> {
    bytes: &'a [u8],
    index: Option<std::collections::HashMap<(u32, u16), Vec<usize>>>,
    parsed: std::collections::HashMap<usize, Object>,
    parsed_bytes: usize,
}

impl<'a> RawDefinitions<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            index: None,
            parsed: std::collections::HashMap::new(),
            parsed_bytes: 0,
        }
    }

    /// Ein Dokument, das genau die Objekte enthält, auf die `dict` (und was
    /// daraus aufgelöst wurde) verweist — je Nummer die Definition nahe
    /// `near`.
    fn scratch(&mut self, dict: &lopdf::Dictionary, near: usize) -> Document {
        let mut doc = Document::new();
        let mut queue = Vec::new();
        for (_, value) in dict.iter() {
            collect_references(value, &mut queue);
        }
        while let Some(id) = queue.pop() {
            if doc.objects.contains_key(&id) {
                continue;
            }
            if let Some(object) = self.lookup(id, near) {
                collect_references(&object, &mut queue);
                doc.objects.insert(id, object);
            }
        }
        doc
    }

    fn lookup(&mut self, id: (u32, u16), near: usize) -> Option<Object> {
        let bytes = self.bytes;
        let index = self
            .index
            .get_or_insert_with(|| raw_definition_index(bytes));
        let offsets = index.get(&id)?;
        // Aufsteigend gesammelt: die nächste ist links oder rechts von `near`.
        let right = offsets.partition_point(|&o| o < near);
        let at = [right.checked_sub(1), Some(right)]
            .into_iter()
            .flatten()
            .filter_map(|i| offsets.get(i).copied())
            .min_by_key(|o| o.abs_diff(near))?;
        if let Some(object) = self.parsed.get(&at) {
            return Some(object.clone());
        }
        let window = &bytes[at..(at + RAW_DEFINITION_WINDOW).min(bytes.len())];
        let body = memmem::find(window, b"endobj").map_or(window, |end| &window[..end]);
        if self.parsed_bytes + body.len() > bytes.len() {
            return None;
        }
        self.parsed_bytes += body.len();
        let object = raw_object_value(body);
        self.parsed.insert(at, object.clone());
        Some(object)
    }
}

/// Jede Definition `N G obj` der Datei: Nummer und Generation → die Stellen
/// hinter `obj`, aufsteigend.
fn raw_definition_index(bytes: &[u8]) -> std::collections::HashMap<(u32, u16), Vec<usize>> {
    let mut index: std::collections::HashMap<(u32, u16), Vec<usize>> =
        std::collections::HashMap::new();
    for at in memmem::find_iter(bytes, b"obj") {
        if bytes[..at].ends_with(b"end") {
            continue;
        }
        // `obj` als Wort, nicht als Anfang von `object`.
        if bytes
            .get(at + 3)
            .is_some_and(|b| !is_whitespace(*b) && !is_delimiter(*b))
        {
            continue;
        }
        let head = bytes[..at].trim_ascii_end();
        if head.len() == at {
            continue;
        }
        let (rest, generation) = trailing_number(head);
        let (_, number) = trailing_number(rest.trim_ascii_end());
        let id = number
            .and_then(|n| n.parse::<u32>().ok())
            .zip(generation.and_then(|g| g.parse::<u16>().ok()));
        if let Some(id) = id {
            index.entry(id).or_default().push(at + 3);
        }
    }
    index
}

/// Sammelt die Verweise in `object` (Listen und Dictionaries eingeschlossen).
fn collect_references(object: &Object, out: &mut Vec<(u32, u16)>) {
    match object {
        Object::Reference(id) => out.push(*id),
        Object::Array(items) => items.iter().for_each(|item| collect_references(item, out)),
        Object::Dictionary(dict) => dict
            .iter()
            .for_each(|(_, value)| collect_references(value, out)),
        _ => {}
    }
}

fn raw_stream_blocks(bytes: &[u8]) -> Vec<(usize, &[u8])> {
    let stream = memmem::Finder::new(b"stream");
    let endstream = memmem::Finder::new(b"endstream");
    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(rel) = stream.find(&bytes[i..]) {
        let start = i + rel;
        i = start + 6;
        // „endstream“ endet ebenfalls auf „stream“ — solche Treffer überspringen.
        if start >= 3 && &bytes[start - 3..start] == b"end" {
            continue;
        }
        if !is_stream_keyword(bytes, start) {
            continue;
        }
        // Hinter dem Schlüsselwort: Leerzeichen, dann das Zeilenende
        // ([`is_stream_keyword`]). Der Strom beginnt dahinter — bis zur
        // Spur-A-Runde 2 hier schon beim Leerzeichen, und ein Flate-Strom
        // hinter `stream \r\n` begann mit drei Bytes, die nicht dazugehören
        // (Register #95).
        let mut data = i;
        while matches!(bytes.get(data), Some(b' ' | b'\t')) {
            data += 1;
        }
        if bytes.get(data) == Some(&b'\r') {
            data += 1;
        }
        if bytes.get(data) == Some(&b'\n') {
            data += 1;
        }
        let Some(rel_end) = endstream.find(&bytes[data..]) else {
            break;
        };
        out.push((data, &bytes[data..data + rel_end]));
        i = data + rel_end + 9;
    }
    out
}

/// Steht an `start` das **Schlüsselwort** `stream` — oder nur die Bytes?
///
/// Das Schlüsselwort steht nach einem Trennzeichen oder Leerraum (meist
/// `>>`, das Ende des Stream-Dictionarys) und vor dem Zeilenende; davor darf
/// noch Leerraum stehen (`stream \r\n`, PDF 32000-1, 7.3.8.1 verlangt das
/// Zeilenende, manche Erzeuger schreiben ein Leerzeichen davor).
///
/// Bis zur Spur-A-Runde 2 genügten die sechs Bytes: ein Titel „Protokoll
/// Livestream“ oder eine Schrift `/BitstreamVeraSans` öffnete einen Block
/// bis zum nächsten `endstream` — und der echte Strom dahinter, etwa der
/// Flate-Strom einer Altrevision, die keine Objektsicht mehr erreicht, wurde
/// nie entpackt (Register #96). Was bleibt: ein Wort `stream` am Ende einer
/// Zeile in einem Literal (`(… Live stream` mit Zeilenumbruch) sieht aus wie
/// das Schlüsselwort und öffnet weiter einen Block — benannte Lücke.
fn is_stream_keyword(bytes: &[u8], start: usize) -> bool {
    let getrennt = start
        .checked_sub(1)
        .is_none_or(|j| is_whitespace(bytes[j]) || is_delimiter(bytes[j]));
    let hinten = bytes.get(start + 6..).unwrap_or_default();
    let leer = hinten
        .iter()
        .take_while(|b| matches!(b, b' ' | b'\t'))
        .count();
    getrennt && matches!(hinten.get(leer), Some(b'\r' | b'\n'))
}

/// Objektnummer und Generation aus dem Kopf `N G obj` vor einem rohen Strom
/// — als **Ziffernfolgen**. `None`, wenn keiner zu finden ist (Block mitten
/// in Rohdaten, Kopf weiter weg als [`OBJECT_HEADER_LOOKBACK`]).
///
/// Warum nicht gleich Zahlen: das Etikett [`object_label`] muss Zeichen für
/// Zeichen dasselbe bleiben wie bisher, auch wenn eine Ziffernfolge in keinen
/// `u32` passt. Die maschinenlesbare Fassung ([`object_id`]) gibt dann `None`
/// zurück — der Text bleibt.
fn object_header(bytes: &[u8], stream_offset: usize) -> Option<(&str, &str)> {
    let from = stream_offset.saturating_sub(OBJECT_HEADER_LOOKBACK);
    let window = &bytes[from..stream_offset];
    let pos = memmem::rfind(window, b"obj")?;
    if window[..pos].ends_with(b"end") {
        return None;
    }
    let (rest, generation) = trailing_number(window[..pos].trim_ascii_end());
    let (_, number) = trailing_number(rest.trim_ascii_end());
    match (number, generation) {
        (Some(number), Some(generation)) if rest.len() < pos => Some((number, generation)),
        _ => None,
    }
}

/// „ (Objekt N G)“ zu einem gefundenen Kopf — damit eine Meldung den Strom so
/// nennt wie die Objektsichten. Leer, wenn keiner gefunden wurde.
fn object_label(header: Option<(&str, &str)>) -> String {
    match header {
        Some((number, generation)) => format!(" (Objekt {number} {generation})"),
        None => String::new(),
    }
}

/// Derselbe Kopf maschinenlesbar für [`LeakSite::object`] — dort als
/// [`ObjectSource::RawHeader`], denn geprüft ist er nicht. `None`, wenn keiner
/// gefunden wurde **oder** eine der Ziffernfolgen nicht in ihren Zahlentyp
/// passt: dann weiß diese Sicht die Objekt-Id nicht, und eine geratene wäre
/// schlimmer als keine.
fn object_id(header: Option<(&str, &str)>) -> Option<(u32, u16)> {
    let (number, generation) = header?;
    Some((number.parse().ok()?, generation.parse().ok()?))
}

/// Die Ziffernfolge am Ende von `s` und der Rest davor.
fn trailing_number(s: &[u8]) -> (&[u8], Option<&str>) {
    let start = s
        .iter()
        .rposition(|b| !b.is_ascii_digit())
        .map_or(0, |i| i + 1);
    let digits = std::str::from_utf8(&s[start..])
        .ok()
        .filter(|d| !d.is_empty());
    (&s[..start], digits)
}

/// Zlib, sonst rohes Deflate — an jedem Block, ohne aufs Dictionary zu
/// schauen: so werden auch Altrevisionen und Blöcke ohne `/Filter` sichtbar.
/// `None`, wenn nichts dabei herauskommt.
fn inflate_raw(data: &[u8], limit: usize) -> Result<Option<Vec<u8>>, Oversize> {
    let out = filters::read_within(flate2::read::ZlibDecoder::new(data), limit)?;
    if !out.is_empty() {
        return Ok(Some(out));
    }
    let raw = filters::read_within(flate2::read::DeflateDecoder::new(data), limit)?;
    Ok((!raw.is_empty()).then_some(raw))
}

// ---------------------------------------------------------------------------
// Ebene 3–6: Objektgraph
// ---------------------------------------------------------------------------

fn scan_object_graph(doc: &Document, probe: &mut Probe, budget: &mut Budget) {
    // Der Trailer gehört zu keinem Objekt — also nennt seine Fundstelle auch
    // keines.
    let trailer = Site::new("Trailer", LeakView::StringObject);
    walk_dict(doc, &doc.trailer, trailer, probe, budget, 0);
    for (id, object) in &doc.objects {
        let path = format!("Objekt {} {}", id.0, id.1);
        let site = Site::new(&path, LeakView::StringObject).with_object((id.0, id.1));
        walk(doc, object, site, probe, budget, 0);
    }
}

/// Die Tiefengrenze ist erreicht — und das wird **gesagt**.
///
/// Der Lader lässt 100 Ebenen zu ([`Limits::max_nesting_depth`]), diese Sicht
/// läuft 32 ([`MAX_DEPTH`]). Zwischen beiden Zahlen liegt ein Streifen, in dem
/// ein Text in der Datei steht und keine Sicht ihn liest: ein oktal
/// maskiertes `\104\105…` in 33 verschachtelten Arrays fand keine Bytesuche
/// (die Maskierung), und die Objektsicht brach ab — **stillschweigend**, mit
/// Rückgabewert 0 an der Kommandozeile (Befund P4-2). Der Abbruch bleibt; nur
/// still ist er nicht mehr.
///
/// Gemeldet wird nur, was überhaupt etwas verbergen kann: ein Blatt ohne
/// Inhalt (Zahl, `null`, Verweis) hat nichts zu verbergen, und eine Meldung
/// darüber wäre bloß Lärm.
fn too_deep(path: &str, object: &Object, budget: &mut Budget) {
    let hides_something = match object {
        Object::String(..) | Object::Name(_) | Object::Stream(_) => true,
        Object::Array(items) => !items.is_empty(),
        Object::Dictionary(dict) => !dict.is_empty(),
        _ => false,
    };
    if hides_something {
        budget.note(deep_message(path));
    }
}

fn deep_message(path: &str) -> String {
    format!(
        "{path}: nicht durchsucht — Verschachtelungstiefe {MAX_DEPTH} erreicht; \
         was tiefer liegt, hat keine Sicht gelesen"
    )
}

fn walk(
    doc: &Document,
    object: &Object,
    site: Site<'_>,
    probe: &mut Probe,
    budget: &mut Budget,
    depth: usize,
) {
    let path = site.text;
    if depth > MAX_DEPTH {
        return too_deep(path, object, budget);
    }
    match object {
        Object::String(raw, format) => {
            let how = match format {
                StringFormat::Literal => "Zeichenkette, literal",
                StringFormat::Hexadecimal => "Zeichenkette, hex",
            };
            scan_string(raw, site, how, probe);
        }
        Object::Name(name) => scan_raw_bytes(name, site, "Name", probe),
        Object::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                walk(
                    doc,
                    item,
                    site.with_text(&format!("{path}[{i}]")),
                    probe,
                    budget,
                    depth + 1,
                );
            }
        }
        Object::Dictionary(dict) => walk_dict(doc, dict, site, probe, budget, depth),
        Object::Stream(stream) => {
            walk_dict(doc, &stream.dict, site, probe, budget, depth);
            scan_stream(doc, stream, site, probe, budget, depth);
        }
        _ => {}
    }
}

fn walk_dict(
    doc: &Document,
    dict: &Dictionary,
    site: Site<'_>,
    probe: &mut Probe,
    budget: &mut Budget,
    depth: usize,
) {
    // `walk` hat die Tiefe schon geprüft und gemeldet, bevor es hierher
    // verzweigt; diese Schranke ist die Absicherung, kein zweiter Melder.
    if depth > MAX_DEPTH {
        return;
    }
    let path = site.text;
    for (key, value) in dict.iter() {
        let key = String::from_utf8_lossy(key);
        walk(
            doc,
            value,
            site.with_text(&format!("{path}/{key}")),
            probe,
            budget,
            depth + 1,
        );
    }
}

/// Ein Stream: roh, dekodiert (im Budget) und — als Objekt-Stream — seine
/// enthaltenen Objekte.
fn scan_stream(
    doc: &Document,
    stream: &Stream,
    site: Site<'_>,
    probe: &mut Probe,
    budget: &mut Budget,
    depth: usize,
) {
    let path = site.text;
    // Ab hier liest Sicht 3: die Bytes des Stroms, nicht mehr sein Verzeichnis.
    let content = site.with_view(LeakView::Stream);
    scan_blob(
        &stream.content,
        content.with_text(&format!("{path} <Stream, roh>")),
        probe,
    );

    let decoded = match decode_stream(doc, stream, budget.room()) {
        Ok(view) => {
            // Der Grund zuerst: er gilt auch dann, wenn es gar keine
            // dekodierte Sicht zu durchsuchen gibt.
            if let Some(reason) = view.unchecked {
                budget.note(format!("{path} <Stream>: {reason}"));
            }
            let work = view.work;
            view.data.map(|(label, data)| {
                budget.charge(work.max(data.len()));
                scan_blob(
                    &data,
                    content.with_text(&format!("{path} <Stream, {label}>")),
                    probe,
                );
                data
            })
        }
        Err(Oversize) => {
            budget.skip(&format!("{path} <Stream>"), stream.content.len());
            return;
        }
    };

    // Objekt-Streams sind komprimierte Container: die enthaltenen Objekte
    // stehen nirgends im Klartext und entgehen jeder Rohbyte-Suche.
    // `ObjectStream::new` würde selbst entpacken — ohne Grenze; deshalb
    // bekommt es den schon entpackten Inhalt ohne `/Filter`.
    if stream.dict.has_type(b"ObjStm") {
        let mut plain = Stream::new(
            stream.dict.clone(),
            decoded.unwrap_or_else(|| stream.content.clone()),
        );
        plain.dict.remove(b"Filter");
        plain.dict.remove(b"DecodeParms");
        plain.dict.remove(b"DP");
        if let Ok(object_stream) = ObjectStream::new(&mut plain) {
            for (id, object) in &object_stream.objects {
                let inner = format!("{path} <ObjStm> → Objekt {} {}", id.0, id.1);
                // Sicht 4, und das Objekt ist das **enthaltene**: dort steht
                // der Text. Welcher Container es trägt, sagt der Text.
                let inside = site
                    .with_view(LeakView::ObjectStream)
                    .with_object((id.0, id.1));
                walk(
                    doc,
                    object,
                    inside.with_text(&inner),
                    probe,
                    budget,
                    depth + 1,
                );
            }
        }
    }
}

/// Was [`decode_stream`] über einen Strom sagt.
struct Decoded {
    /// Die dekodierte Sicht: Beschriftung und Bytes. `None`, wenn kein Glied
    /// der Kette lief — dann gibt es nichts zu durchsuchen, was die schon
    /// gelaufene Rohsicht nicht bereits gelesen hätte.
    data: Option<(String, Vec<u8>)>,
    /// Die Arbeit der Kette: die Ausgaben aller angewandten Glieder
    /// zusammen. Sie bucht das Budget, nicht die Größe von `data` — ein
    /// schrumpfendes letztes Glied verbarg sonst alles davor (Register #99).
    work: usize,
    /// Der Grund für eine Zeile in [`LeakCheck::unchecked`] — ohne den
    /// Objektpfad, den der Aufrufer davorsetzt.
    ///
    /// Gesetzt, sobald die Kette an einem Glied stehen blieb, das dieses
    /// Programm nicht anwenden kann — **an welcher Stelle auch immer**: ein
    /// Filtername, den es nicht kennt, oder ein Glied ganz ohne Namen
    /// (`[/LZWDecode null]`, ein Verweis ins Leere, eine Zahl, eine
    /// Zeichenkette — siehe [`filters::filter_names`]). Nicht gesetzt bei
    /// einem Bildfilter ([`filters::is_image_filter`]): der ist ein im
    /// Modulkopf benannter blinder Fleck, und eine Meldung darüber stünde an
    /// jeder zweiten Datei mit einem Foto.
    unchecked: Option<String>,
}

/// Die dekodierte Sicht auf einen gefilterten Stream — so weit, wie sie
/// reicht — und der Grund, falls die Kette vorher stehen blieb.
///
/// Ohne `/Filter` gibt es beides nicht: die Rohbytes sind schon durchsucht.
///
/// Bricht die Kette ab, kommt zurück, was bis dahin entpackt war:
/// `[/ASCIIHexDecode /FlateDecode /DCTDecode]` mit Klartext im Flate-Teil
/// wird so wieder gefunden. Bis Commit `f982c12` konnte das Orakel das (ein
/// eigener Dekoder, der abbrach und behielt), danach nicht mehr — siehe
/// [`filters::decoded_prefix_within`].
///
/// Bricht sie **an ihrem ersten Glied** ab, gibt es keine dekodierte Sicht:
/// die Rohbytes hat die Rohsicht gelesen, eine zweite gleichlautende Meldung
/// brächte nichts. Der **Grund** steht trotzdem — bis Fix-Runde 6 stieg diese
/// Funktion bei `applied == 0` mit `Ok(None)` aus und fragte
/// [`filters::is_image_filter`] gar nicht erst. `/Filter /FooDecode` kam
/// deshalb an der Kommandozeile als „nicht gefunden“ mit Rückgabewert 0
/// zurück, `/Filter [/FlateDecode /FooDecode]` mit Rückgabewert 3 — dieselbe
/// unlesbare Stelle, und die Meldung hing allein an der Position
/// (Befund Q2-1/Q5).
fn decode_stream(doc: &Document, stream: &Stream, room: usize) -> Result<Decoded, Oversize> {
    let names = filters::filter_names(doc, &stream.dict).unwrap_or_default();
    if names.is_empty() {
        return Ok(Decoded {
            data: None,
            work: 0,
            unchecked: None,
        });
    }
    let total = names.len();
    let (data, applied, work) = filters::decoded_prefix_counted(doc, stream, room)?;

    let unchecked = chain_gap(&names, applied);

    let chain = |names: &[Vec<u8>]| {
        names
            .iter()
            .map(|f| String::from_utf8_lossy(f))
            .collect::<Vec<_>>()
            .join("+")
    };
    let data = match applied < total {
        // Nichts entpackt: die Rohsicht ist die einzige Sicht.
        _ if applied == 0 => None,
        true => {
            let rest = if names[applied].is_empty() {
                "ein Glied ohne Filternamen".to_string()
            } else {
                format!("/{} unbekannt", String::from_utf8_lossy(&names[applied]))
            };
            Some((
                format!(
                    "dekodiert: {} — bis Filter {applied} von {total}, danach {rest}",
                    chain(&names[..applied])
                ),
                data,
            ))
        }
        false => Some((format!("dekodiert: {}", chain(&names)), data)),
    };
    Ok(Decoded {
        data,
        work,
        unchecked,
    })
}

// ---------------------------------------------------------------------------
// Vergleiche
// ---------------------------------------------------------------------------

/// Durchsucht einen (dekodierten) Datenblock: erst byteweise in allen
/// Kodierungen, dann die Verkettung aller darin enthaltenen
/// Zeichenketten-Literale.
fn scan_blob(blob: &[u8], site: Site<'_>, probe: &mut Probe) {
    scan_raw_bytes(blob, site, "Inhalt", probe);

    // Verkettung und Leerraum-Fassung hängen allein am Datenblock: einmal
    // bilden, dann von jedem Suchbegriff benutzen.
    let joined = concat_pdf_strings(blob);
    if joined.is_empty() {
        return;
    }
    // Die Verkettung ist eine eigene Sicht (6) — derselbe Ort, ein anderer
    // Blick darauf.
    let concat = site.with_view(LeakView::StringConcat);
    probe.scan_text_plain(&joined, concat, "Zeichenketten-Verkettung");
    if probe.any_squeezed {
        let squeezed = squeeze(&joined);
        probe.scan_text_squeezed(
            &squeezed,
            concat,
            "Zeichenketten-Verkettung, ohne Leerraum",
            |_| true,
        );
    }
}

fn scan_raw_bytes(hay: &[u8], site: Site<'_>, how: &str, probe: &mut Probe) {
    probe.scan_bytes(hay, 4, site, how);
}

/// Zeichenketten-Objekt: sowohl dekodiert (PDFDocEncoding **oder** UTF-16BE)
/// als auch roh vergleichen.
fn scan_string(raw: &[u8], site: Site<'_>, how: &str, probe: &mut Probe) {
    // Dekodieren hängt allein an der Zeichenkette, nicht am Suchbegriff.
    scan_text(&decode_pdf_string(raw), site, how, probe);
    scan_raw_bytes(raw, site, how, probe);
}

/// Bereits dekodierter Text: als Ganzes und, wo der Begriff Leerraum hat
/// und als Ganzes nicht traf, ohne jeden Leerraum.
fn scan_text(text: &str, site: Site<'_>, how: &str, probe: &mut Probe) {
    let hit = probe.scan_text_plain(text, site, how);
    if probe.any_squeezed {
        let squeezed = squeeze(text);
        probe.scan_text_squeezed(&squeezed, site, &format!("{how}, ohne Leerraum"), |n| {
            !hit[n]
        });
    }
}

// ---------------------------------------------------------------------------
// Ebene 7: der Text, wie der eigene Schriftdekoder ihn liest
// ---------------------------------------------------------------------------

/// Durchsucht jede Seite so, wie die Analyse sie liest: Glyphencodes über
/// `/ToUnicode`, `/Differences` und die Standardkodierungen in Zeichen
/// übersetzt, zu Zeilen zusammengesetzt (siehe [`PdfExtractor`]).
///
/// Die Sichtweisen 1–6 vergleichen Bytes. In einer eingebetteten
/// Teilmengen-Schrift — dem Regelfall aus Word, LibreOffice, Chrome — stehen
/// im Strom aber keine Zeichen, sondern Glyphnummern oder umgelenkte Codes:
/// `<01020304>Tj` für „Kont“. Keine der sechs Bytesichten kann darin eine
/// IBAN finden; gemessen an einem LibreOffice-Writer-24.2-Export meldete der
/// Detektor an der **ungeschwärzten** Datei „nicht gefunden“ für alle vier
/// Begriffe.
///
/// Diese Sicht kommt **dazu**, nicht an die Stelle der anderen. Allein wäre
/// sie der Zirkelschluss aus dem Modulkommentar: wovor der Dekoder blind ist,
/// wäre auch hier unsichtbar. Beide zusammen decken sich gegenseitig: die
/// Bytesichten finden, was der Dekoder nicht liest (Metadaten, verwaiste
/// Objekte, Rohtext in einem Strom, den kein `Do` erreicht); der Dekoder
/// findet, was die Bytesichten nicht lesen (Glyphencodes).
///
/// Eine Seite, die der Interpreter ablehnt (Aufwandskonto gerissen, Strom
/// nicht zerlegbar), fehlt in dieser Sicht — die Bytesichten haben sie
/// trotzdem durchsucht. Damit sie nicht die anderen Seiten mitnimmt, liest
/// [`PdfExtractor::extract_lenient`] in **einem** Durchgang über **einen**
/// Seitenbaum und überspringt nur die abgelehnte Seite. Der frühere
/// Rückfall — nach dem ersten Fehler Seite für Seite über eine
/// seitenweise Extraktion, die je Aufruf den Seitenbaum neu baute — war
/// quadratisch in der Seitenzahl: gemessen an einem 4 000-Seiten-Dokument
/// 8,7 s für das **ganze Dokument** gegenüber 1,3 s für
/// [`PdfExtractor::extract`]; heute misst `zb_rueckfall_linear.rs` den
/// Speicher, und der wächst linear.
///
/// Liefert je Seite, die der Interpreter ablehnte, eine Zeile: dort hat
/// diese Sicht nichts gelesen (Register #98). Ausgenommen ist eine Seite,
/// deren Inhaltsströme **alle** hinter einem Filter stehen, den hier niemand
/// auspackt ([`page_content_undecodable`]): dort konnte diese Sicht nie etwas
/// lesen, und die Stelle ist schon benannt — ein unbekannter Filter steht als
/// eigene Zeile der Objektsicht in `unchecked`, Text hinter einem Bildfilter
/// ist der benannte blinde Fleck aus dem Modulkopf. Eine zweite Zeile dafür
/// machte aus jeder solchen Datei einen Befund mehr, und aus der Zusage
/// „Bildfilter am Kettenende: keine Meldung“ (`SECURITY.md`) eine falsche.
fn scan_decoded_text(doc: &Document, probe: &mut Probe) -> Vec<String> {
    let (runs, _, gaps) = PdfExtractor::new().extract_lenient_with_gaps(doc);
    let gaps = gaps
        .into_iter()
        .filter(|(page_id, _)| !page_content_undecodable(doc, *page_id))
        .map(|(_, gap)| gap)
        .collect();
    // Die Zeilen kommen seitenweise sortiert; je Seite ein Text.
    for page_runs in runs.chunk_by(|a, b| a.page == b.page) {
        let text: String = page_runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        // Die einzige Sicht, die eine Seite kennt — und die einzige, deren
        // Fund heißt: das steht auf dem Papier.
        let page = page_runs[0].page + 1;
        scan_text(
            &text,
            Site::new(&format!("Seite {page}"), LeakView::FontDecoder).with_page(page),
            "Schriftdekoder",
            probe,
        );
    }
    gaps
}

/// Steht **jeder** Inhaltsstrom der Seite hinter einer Kette, die hier nicht
/// ganz durchläuft — ein Glied, das [`filters::is_decodable_filter`] nicht
/// kennt (Bildfilter, unbekannter Name, ein Glied ohne Namen)?
///
/// Gefragt wird nach den Namen, ausgepackt wird nichts. Eine Seite mit einem
/// lesbaren und einem unlesbaren Strom ist **nicht** unlesbar: der
/// Interpreter lehnt sie als Ganzes ab, und was im lesbaren Strom steht, hat
/// diese Sicht nicht gesehen — sie behält ihre Zeile. Eine Seite ohne
/// Inhaltsstrom auch; sie lehnt der Interpreter nie ab.
fn page_content_undecodable(doc: &Document, page_id: lopdf::ObjectId) -> bool {
    let contents = doc.get_page_contents(page_id);
    !contents.is_empty()
        && contents.into_iter().all(|id| {
            let Ok(stream) = doc.get_object(id).and_then(Object::as_stream) else {
                return false;
            };
            filters::filter_names(doc, &stream.dict)
                .unwrap_or_default()
                .iter()
                .any(|name| !filters::is_decodable_filter(name))
        })
}

/// Hängt `lines` an `unchecked` an — höchstens [`MAX_UNCHECKED`] davon,
/// der Rest als eine Summenzeile. Gezählt werden die Stellen beim Aufrufer.
fn push_capped(unchecked: &mut Vec<String>, lines: Vec<String>, was: &str) {
    let rest = lines.len().saturating_sub(MAX_UNCHECKED);
    unchecked.extend(lines.into_iter().take(MAX_UNCHECKED));
    if rest > 0 {
        unchecked.push(format!("… und {rest} weitere {was} nicht geprüft"));
    }
}

/// Ströme, die der Lader **anders** übernommen hat, als sie in den Rohbytes
/// stehen — je Objekt eine Zeile.
///
/// Verglichen wird nur, was die Querverweistabelle als gewöhnliches Objekt
/// führt: ein Strom einer Altrevision, den ein inkrementelles Update ersetzt
/// oder freigegeben hat, steht auch in den Rohbytes und ist trotzdem nicht
/// verlesen. Verglichen wird deshalb nur der Block des aktuellen Objekts
/// (der erste seiner Nummer hinter dem Offset der Querverweistabelle); er
/// gilt als richtig gelesen, wenn er mit genau den geladenen Bytes beginnt
/// und dahinter, nach Leerraum, `endstream` folgt — dann stimmte `/Length`. Sonst: das Objekt
/// fehlt, ist kein Strom, oder der Lader las eine andere Länge (eine zu
/// kurze oder zu lange `/Length`, ein versetzter Querverweis).
///
/// Bis zur Spur-A-Runde 2 fehlte ein solches Objekt stumm in den Sichten
/// 3–7: `pdftotext` las das Geheimnis, das Orakel meldete „nicht gefunden“
/// mit Rückgabewert 0 (Register #98). Ein verschlüsseltes Dokument wird
/// nicht verglichen — der Lader entschlüsselt, die Rohbytes nicht —, und
/// ebenso wenig ein Objekt-Strom, den der Lader entpackt ablegt.
fn misread_streams(
    bytes: &[u8],
    raw: &[((u32, u16), usize)],
    doc: &Document,
) -> (Vec<String>, Option<BTreeSet<usize>>) {
    if doc.is_encrypted() {
        return (Vec::new(), None);
    }
    let mut covered = BTreeSet::new();
    let mut by_id: std::collections::BTreeMap<(u32, u16), Vec<usize>> =
        std::collections::BTreeMap::new();
    for (id, offset) in raw {
        by_id.entry(*id).or_default().push(*offset);
    }
    let mut out = Vec::new();
    for (id, offsets) in by_id {
        let Some(lopdf::xref::XrefEntry::Normal { offset, generation }) =
            doc.reference_table.get(id.0)
        else {
            continue;
        };
        if *generation != id.1 {
            continue;
        }
        // Der Block des **aktuellen** Objekts: der erste dieser Nummer hinter
        // dem Offset aus der Querverweistabelle, ohne `endobj` dazwischen.
        // Ältere Blöcke derselben Nummer stehen davor (ein inkrementelles
        // Update hängt an) und sind Altrevisionen — auch dann, wenn die
        // Nummer jetzt ein Objekt ohne Strom trägt.
        let offset = *offset as usize;
        let Some(block) = offsets
            .iter()
            .copied()
            .filter(|&o| o > offset)
            .min()
            .filter(|&o| memmem::find(&bytes[offset.min(o)..o], b"endobj").is_none())
        else {
            continue;
        };
        let gelesen = match doc.objects.get(&id) {
            // Einen Objekt-Strom entpackt der Lader beim Laden und legt ihn
            // entpackt ab — seine Bytes gleichen den Rohbytes nie. Eine
            // falsche `/Length` an ihm bleibt hier ungemeldet (benannte
            // Lücke); die Objekte darin sehen die Sichten 3–7 trotzdem, so
            // weit der Lader sie fand.
            Some(Object::Stream(stream))
                if stream.dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"ObjStm") =>
            {
                true
            }
            Some(Object::Stream(stream)) => {
                // Der Rohblock beginnt hinter dem Zeilenende, auch hinter
                // `stream \r\n` ([`raw_stream_blocks`]) — dort, wo auch der
                // Lader die Bytes nimmt.
                let rest = &bytes[block..];
                rest.starts_with(&stream.content) && {
                    // Und dahinter, nach Leerraum, `endstream`: dann stimmte
                    // die Länge. (Verteidigend — `lopdf` 0.42 übernimmt einen
                    // Strom nur, wenn es so ist.)
                    let hinten = &rest[stream.content.len()..];
                    let leer = hinten
                        .iter()
                        .take_while(|b| matches!(b, b'\r' | b'\n' | b' ' | b'\t' | b'\x0c' | 0))
                        .count();
                    hinten[leer..].starts_with(b"endstream")
                }
            }
            _ => false,
        };
        if gelesen {
            covered.insert(block);
        } else {
            out.push(format!(
                "Objekt {} {}: der Lader übernahm nicht den Strom, der in den Rohbytes \
                 steht (Länge im Dictionary oder Querverweis passt nicht) — die Sichten \
                 3–7 haben ihn nicht gelesen, nur die Rohsichten",
                id.0, id.1
            ));
        }
    }
    (out, Some(covered))
}

/// Wo eine Kette stehen blieb, als Zeile für `unchecked` — oder `None`: sie
/// lief ganz durch, oder sie blieb an etwas stehen, worüber zu schweigen
/// richtig ist (ein Bildfilter als letztes Glied, der benannte blinde Fleck).
///
/// Dieselbe Regel für die Objektsicht ([`decode_stream`]) und die Rohsicht
/// ([`decode_raw_chain`]). Bis zur Spur-A-Runde 2 meldete die Rohsicht eine
/// gescheiterte Kette nie: eine Altrevision unter `/FooDecode` stand in
/// keiner Objektsicht und in keiner Zeile (Register #95).
fn chain_gap(names: &[Vec<u8>], applied: usize) -> Option<String> {
    let total = names.len();
    // Der Filter, an dem die Kette stehen blieb — falls sie stehen blieb.
    // Ob er der erste ist oder der letzte, ändert nichts daran, was hinter
    // ihm liegt: ungelesen. Nur **welcher** Filter es ist, entscheidet, ob
    // das eine Meldung wert ist.
    // Der Grund für eine Meldung — `None` heißt: die Kette lief ganz durch,
    // oder sie blieb an etwas stehen, worüber zu schweigen richtig ist.
    let grund = (applied < total).then(|| {
        let name = &names[applied];
        // Ein leeres Glied ist keines mit unbekanntem Namen, sondern eines
        // ganz **ohne** Namen: `/Filter [/LZWDecode null]`, ein Verweis ins
        // Leere, eine Zahl, eine Zeichenkette (siehe `filters::filter_names`).
        if name.is_empty() {
            return Some(
                "der Wert an dieser Stelle ist kein Filtername (Verweis ins Leere, \
                 null, Zahl oder Zeichenkette)"
                    .to_string(),
            );
        }
        if filters::is_image_filter(name) {
            // Der benannte blinde Fleck — aber nur als **letztes** Glied.
            // Die Ausgabe eines Bildfilters sind Abtastwerte; kein Erzeuger
            // hängt dahinter noch einen Filter (PDF 32000-1, 7.4.1: die
            // Reihenfolge im Array ist die Dekodierreihenfolge). Steht doch
            // eines dahinter, liegt dort kein Bild, sondern ein Glied, das
            // niemand angewandt hat — und darüber zu schweigen, verkauft
            // einen Bildfilternamen als meldungsfreie Zone (Befund R2-A).
            return (applied + 1 < total).then(|| {
                format!(
                    "/{} ist ein Bildfilter und wird nicht dekodiert, aber die Kette \
                     geht dahinter weiter",
                    String::from_utf8_lossy(name)
                )
            });
        }
        Some(format!(
            "/{} ist hier kein bekannter Filter",
            String::from_utf8_lossy(name)
        ))
    });
    match grund.flatten() {
        Some(grund) if applied == 0 => Some(format!(
            "gar nicht dekodiert — {grund} (Glied 1 von {total}); gelesen sind \
             nur die rohen, gepackten Bytes"
        )),
        Some(grund) => Some(format!(
            "nur bis Filter {applied} von {total} dekodiert — {grund}; was dahinter \
             steht, hat keine Sicht gelesen"
        )),
        None => None,
    }
}

/// PDFDocEncoding (PDF 32000-1, Anhang D.2), wo es von Latin-1 abweicht:
/// die Akzente 0x18–0x1F und der Block 0x80–0xA0 mit `•`, `–`, `—`, `…`,
/// den typografischen Anführungszeichen, `™`, `ﬁ`/`ﬂ` und `€` (0xA0 — nicht
/// das geschützte Leerzeichen). 0x9F ist unbelegt und bleibt, was Latin-1
/// daraus macht.
///
/// Bis zur Spur-A-Runde 1 las [`decode_pdf_string`] jedes Byte als Latin-1;
/// `(Betrag 5 \240)` wurde damit „Betrag 5 “ mit U+00A0, und ein
/// `--check-leaks "Betrag 5 €"` fand nichts — ohne Meldung, Rückgabewert 0
/// (Register #81).
const PDFDOC_ABWEICHUNGEN: &[(u8, char)] = &[
    (0x18, '\u{02D8}'),
    (0x19, '\u{02C7}'),
    (0x1A, '\u{02C6}'),
    (0x1B, '\u{02D9}'),
    (0x1C, '\u{02DD}'),
    (0x1D, '\u{02DB}'),
    (0x1E, '\u{02DA}'),
    (0x1F, '\u{02DC}'),
    (0x80, '\u{2022}'),
    (0x81, '\u{2020}'),
    (0x82, '\u{2021}'),
    (0x83, '\u{2026}'),
    (0x84, '\u{2014}'),
    (0x85, '\u{2013}'),
    (0x86, '\u{0192}'),
    (0x87, '\u{2044}'),
    (0x88, '\u{2039}'),
    (0x89, '\u{203A}'),
    (0x8A, '\u{2212}'),
    (0x8B, '\u{2030}'),
    (0x8C, '\u{201E}'),
    (0x8D, '\u{201C}'),
    (0x8E, '\u{201D}'),
    (0x8F, '\u{2018}'),
    (0x90, '\u{2019}'),
    (0x91, '\u{201A}'),
    (0x92, '\u{2122}'),
    (0x93, '\u{FB01}'),
    (0x94, '\u{FB02}'),
    (0x95, '\u{0141}'),
    (0x96, '\u{0152}'),
    (0x97, '\u{0160}'),
    (0x98, '\u{0178}'),
    (0x99, '\u{017D}'),
    (0x9A, '\u{0131}'),
    (0x9B, '\u{0142}'),
    (0x9C, '\u{0153}'),
    (0x9D, '\u{0161}'),
    (0x9E, '\u{017E}'),
    (0xA0, '\u{20AC}'),
];

/// Ein Byte in PDFDocEncoding als Zeichen.
fn pdfdoc_char(byte: u8) -> char {
    PDFDOC_ABWEICHUNGEN
        .iter()
        .find(|(b, _)| *b == byte)
        .map_or(byte as char, |(_, c)| *c)
}

/// Dekodiert eine PDF-Zeichenkette.
///
/// UTF-16 wird am BOM erkannt (`FE FF`, in freier Wildbahn auch `FF FE`);
/// alles andere wird als PDFDocEncoding gelesen — Latin-1 mit den
/// Abweichungen aus [`PDFDOC_ABWEICHUNGEN`].
pub fn decode_pdf_string(raw: &[u8]) -> String {
    if raw.len() >= 2 && raw[0] == 0xfe && raw[1] == 0xff {
        let units: Vec<u16> = raw[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else if raw.len() >= 2 && raw[0] == 0xff && raw[1] == 0xfe {
        let units: Vec<u16> = raw[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        raw.iter().map(|&b| pdfdoc_char(b)).collect()
    }
}

/// Verkettet alle Zeichenketten-Literale eines Blocks — literal `(…)` wie
/// hexadezimal `<…>`.
///
/// Bewusst ein eigener Mini-Lexer statt `lopdf::content::Content::decode`:
/// dessen Parser verliert bei einem Inline-Bild (`BI … ID … EI`) den Rest des
/// Streams — genau der Fehler, den dieses Modell aufdecken soll.
fn concat_pdf_strings(blob: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < blob.len() {
        match blob[i] {
            b'(' => {
                let (bytes, next) = read_literal_string(blob, i + 1);
                out.push_str(&decode_pdf_string(&bytes));
                i = next;
            }
            b'<' if blob.get(i + 1) != Some(&b'<') => {
                let (bytes, next) = read_hex_string(blob, i + 1);
                out.push_str(&decode_pdf_string(&bytes));
                i = next;
            }
            _ => i += 1,
        }
    }
    out
}

fn read_literal_string(blob: &[u8], mut i: usize) -> (Vec<u8>, usize) {
    let mut out = Vec::new();
    let mut depth = 1usize;
    while i < blob.len() {
        match blob[i] {
            b'\\' => {
                i += 1;
                let Some(&esc) = blob.get(i) else { break };
                match esc {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(0x08),
                    b'f' => out.push(0x0c),
                    b'\n' => {}
                    b'\r' => {
                        if blob.get(i + 1) == Some(&b'\n') {
                            i += 1;
                        }
                    }
                    b'0'..=b'7' => {
                        let mut value = u32::from(esc - b'0');
                        let mut taken = 1;
                        while taken < 3 {
                            match blob.get(i + 1) {
                                Some(&d @ b'0'..=b'7') => {
                                    value = value * 8 + u32::from(d - b'0');
                                    i += 1;
                                    taken += 1;
                                }
                                _ => break,
                            }
                        }
                        out.push(value as u8);
                    }
                    other => out.push(other),
                }
                i += 1;
            }
            b'(' => {
                depth += 1;
                out.push(b'(');
                i += 1;
            }
            b')' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    break;
                }
                out.push(b')');
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    (out, i)
}

fn read_hex_string(blob: &[u8], mut i: usize) -> (Vec<u8>, usize) {
    let start = i;
    let mut nibbles = Vec::new();
    while i < blob.len() && blob[i] != b'>' {
        match filters::hex_value(blob[i]) {
            Some(v) => nibbles.push(v),
            None if blob[i].is_ascii_whitespace() => {}
            // Kein Hex-String, sondern irgendein anderes `<` im Datenstrom.
            None => return (Vec::new(), start),
        }
        i += 1;
    }
    if i < blob.len() {
        i += 1;
    }
    if nibbles.len() % 2 == 1 {
        nibbles.push(0);
    }
    let bytes = nibbles.chunks(2).map(|c| (c[0] << 4) | c[1]).collect();
    (bytes, i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_nothing_in_an_empty_haystack() {
        assert!(leaks(b"", "GEHEIM").is_empty());
        assert!(leaks(b"%PDF-1.5\n", "GEHEIM").is_empty());
    }

    #[test]
    fn empty_needle_never_matches() {
        assert!(leaks(b"irgendwas", "").is_empty());
    }

    #[test]
    fn finds_plain_bytes_with_offset_and_context() {
        let hits = leaks(b"%PDF-1.5\n(Konto GEHEIM 42)\n", "GEHEIM");
        assert!(!hits.is_empty());
        assert!(hits[0].contains("Rohdatei"), "{hits:?}");
        assert!(hits[0].contains("Konto GEHEIM 42"), "{hits:?}");
    }

    #[test]
    fn finds_utf16be_encoded_text() {
        let mut data = b"%PDF-1.5\n(".to_vec();
        data.extend_from_slice(&[0xfe, 0xff]);
        data.extend("GEHEIM".encode_utf16().flat_map(|u| u.to_be_bytes()));
        data.extend_from_slice(b")\n");
        let hits = leaks(&data, "GEHEIM");
        assert!(
            hits.iter().any(|h| h.contains("UTF-16BE")),
            "UTF-16BE nicht erkannt: {hits:?}"
        );
    }

    #[test]
    fn finds_hex_string_syntax() {
        let hits = leaks(b"%PDF-1.5\n<47454845494D>\n", "GEHEIM");
        assert!(
            hits.iter().any(|h| h.contains("Hex-String")
                || h.contains("Zeichenketten-Verkettung")
                || h.contains("Inhalt")),
            "{hits:?}"
        );
    }

    /// Die alte Suche, Byte für Byte — als Maßstab für die neue.
    fn find_all_naive(hay: &[u8], pat: &[u8], limit: usize) -> Vec<usize> {
        if pat.is_empty() || hay.len() < pat.len() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut from = 0usize;
        while from + pat.len() <= hay.len() {
            match hay[from..].windows(pat.len()).position(|w| w == pat) {
                Some(rel) => {
                    out.push(from + rel);
                    if out.len() >= limit {
                        break;
                    }
                    from += rel + pat.len();
                }
                None => break,
            }
        }
        out
    }

    /// Der Automat muss Fundstelle für Fundstelle dasselbe liefern wie die
    /// naive Suche je Muster: überlappungsfrei je Muster, in Reihenfolge, an
    /// der Grenze abgeschnitten. Die Fälle: Muster länger als der Heuhaufen,
    /// Muster am Ende, überlappende Vorkommen (`aaaa` in `aaaaaaa` sind zwei,
    /// nicht vier), Grenze kleiner als die Zahl der Vorkommen — und mehrere
    /// Muster, die einander überlappen oder gleich sind, im selben Durchgang.
    #[test]
    fn positions_agree_with_the_naive_search() {
        type Case<'a> = (&'a [u8], &'a [&'a [u8]], usize);
        let cases: &[Case] = &[
            (b"", &[b"a"], 4),
            (b"ab", &[b"abc"], 4),
            (b"xxabc", &[b"abc"], 4),
            (b"aaaaaaa", &[b"aaaa"], 4),
            (b"abcabcabcabc", &[b"abc"], 2),
            (b"abcabcabcabc", &[b"abc"], 8),
            (b"a.b.c.d.e.f", &[b"."], 3),
            (b"DE89 3704 0044 DE89 3704", &[b"DE89 3704"], 8),
            // Überlappend: der eine Begriff steckt im anderen.
            (
                b"Max Mustermann und Mustermann",
                &[b"Max Mustermann", b"Mustermann", b"Max"],
                4,
            ),
            // Gleiche Muster unter zwei Ids, und eines, das nie trifft.
            (b"abab", &[b"ab", b"ab", b"zz", b"b"], 4),
            (b"aaaaaaa", &[b"aaaa", b"aa", b"a"], 2),
        ];
        for (hay, pats, limit) in cases {
            let matcher = Matcher::new(
                pats.iter()
                    .enumerate()
                    .map(|(i, p)| ((i, 0), p.to_vec()))
                    .collect(),
            );
            let expected: Vec<Vec<usize>> = pats
                .iter()
                .map(|p| find_all_naive(hay, p, *limit))
                .collect();
            assert_eq!(
                matcher.positions(hay, *limit),
                expected,
                "hay={hay:?} pats={pats:?} limit={limit}"
            );
        }
        let matcher = Matcher::new(vec![((0, 0), b"aaaa".to_vec())]);
        assert_eq!(matcher.positions(b"aaaaaaa", 4), vec![vec![0]]);
        let matcher = Matcher::new(vec![((0, 0), b"abc".to_vec())]);
        assert_eq!(matcher.positions(b"abcabcabcabc", 2), vec![vec![0, 3]]);
    }

    /// Ein Begriff aus lauter Leerraum hat keine Fassung ohne Leerraum —
    /// ein leeres Muster stünde an jeder Stelle jeder Datei.
    #[test]
    fn a_blank_needle_has_no_squeezed_form() {
        assert!(Needle::new("   ").squeezed.is_none());
        assert!(leaks(b"%PDF-1.5\n(irgendwas)\n", "   ").is_empty());
    }

    /// Der Objektkopf vor einem rohen Strom — für die Meldung, welcher
    /// Strom nicht entpackt wurde.
    #[test]
    fn raw_blocks_are_labelled_with_their_object_header() {
        let pdf = b"%PDF-1.5\n12 0 obj\n<< /Length 3 >>\nstream\nabc\nendstream\nendobj\n";
        let blocks = raw_stream_blocks(pdf);
        assert_eq!(blocks.len(), 1);
        let header = object_header(pdf, blocks[0].0);
        assert_eq!(object_label(header), " (Objekt 12 0)");
        // Derselbe Kopf maschinenlesbar — Text und Ort sagen dasselbe.
        assert_eq!(object_id(header), Some((12, 0)));
        // Ohne Kopf (Block mitten in Rohdaten, oder nur ein `endobj` davor):
        // kein Etikett, keine Erfindung.
        let loose = b"%PDF-1.5\nendobj\nstream\nabc\nendstream\n";
        let blocks = raw_stream_blocks(loose);
        let header = object_header(loose, blocks[0].0);
        assert_eq!(object_label(header), "");
        assert_eq!(object_id(header), None);
        // Eine Nummer, die in keinen `u32` passt: das Etikett bleibt, der
        // maschinenlesbare Ort sagt „weiß ich nicht“ statt zu raten.
        let huge = b"%PDF-1.5\n99999999999999 0 obj\nstream\nabc\nendstream\n";
        let blocks = raw_stream_blocks(huge);
        let header = object_header(huge, blocks[0].0);
        assert_eq!(object_label(header), " (Objekt 99999999999999 0)");
        assert_eq!(object_id(header), None);
    }

    /// Register #96: nur das Schlüsselwort öffnet einen Block. Je Fall genau
    /// ein Block, und er beginnt hinter dem echten `stream`.
    #[test]
    fn only_the_stream_keyword_opens_a_raw_block() {
        let echt = b"<< /Length 3 >>\nstream\nabc\nendstream";
        for vorher in [
            &b""[..],
            b"(Protokoll Livestream)\n",
            b"<< /BaseFont /BitstreamVeraSans >>\n",
            b"(Live stream heute)\n",
            b"(streams)\n",
            b"(Protokoll Livestream\nTeil 2)\n",
        ] {
            let pdf = [vorher, &echt[..]].concat();
            let blocks = raw_stream_blocks(&pdf);
            assert_eq!(blocks.len(), 1, "{}", String::from_utf8_lossy(vorher));
            assert_eq!(blocks[0].1, b"abc\n", "{}", String::from_utf8_lossy(vorher));
        }
        // Die Formen des Schlüsselworts: direkt hinter `>>`, mit Leerzeichen
        // vor dem Zeilenende, mit CR LF.
        for form in [
            &b"<<>>stream\nabc\nendstream"[..],
            b"<< >>\nstream \r\nabc\nendstream",
            b"<< >>\r\nstream\r\nabc\nendstream",
        ] {
            assert_eq!(
                raw_stream_blocks(form).len(),
                1,
                "{}",
                String::from_utf8_lossy(form)
            );
        }
    }

    #[test]
    fn literal_string_handles_escapes_and_nesting() {
        let (bytes, _) = read_literal_string(b"a\\(b(c)d\\101)rest", 0);
        assert_eq!(bytes, b"a(b(c)dA");
    }

    #[test]
    fn concatenation_joins_tj_fragments() {
        let blob = b"[(DE89 3704 0044 )-2(0532 0130 00)] TJ";
        let joined = concat_pdf_strings(blob);
        assert_eq!(joined, "DE89 3704 0044 0532 0130 00");
    }

    #[test]
    fn decodes_both_string_encodings() {
        assert_eq!(decode_pdf_string(b"Hallo"), "Hallo");
        let mut utf16 = vec![0xfe, 0xff];
        utf16.extend("Hallo".encode_utf16().flat_map(|u| u.to_be_bytes()));
        assert_eq!(decode_pdf_string(&utf16), "Hallo");
    }

    /// Die gesuchten Begriffe eines Laufs — Treffer und Nicht-Treffer,
    /// mit und ohne Leerraum, ein leerer dazwischen.
    const NEEDLES: &[&str] = &[
        "DE89 3704 0044 0532 0130 00",
        "Max Mustermann",
        "kommtnichtvor",
        "",
        "Kontonummer",
    ];

    /// `leaks_many` ist dieselbe Messung wie `leaks`, nur in einem Durchgang.
    ///
    /// Das ist die Zusicherung, die zählt: das ehrliche Orakel darf durch die
    /// Zusammenfassung kein einziges Leck weniger melden. Verglichen wird
    /// Fundstelle für Fundstelle, nicht bloß „auch etwas gefunden“.
    #[test]
    fn one_pass_reports_exactly_what_the_single_pass_reports() {
        let pdf = crate::testing::demo_statement();
        let many = leaks_many(&pdf, NEEDLES);
        assert_eq!(many.len(), NEEDLES.len());

        for (needle, hits) in NEEDLES.iter().zip(&many) {
            assert_eq!(
                hits,
                &leaks(&pdf, needle),
                "abweichende Fundstellen für {needle:?}"
            );
        }

        // Und der Test misst wirklich etwas: mindestens ein Begriff steht in
        // der ungeschwärzten Vorlage, sonst verglichen wir nur leere Listen.
        assert!(
            many.iter().any(|hits| !hits.is_empty()),
            "kein Begriff gefunden — dieser Test würde jede Änderung durchwinken"
        );
        assert!(
            many[2].is_empty() && many[3].is_empty(),
            "Nicht-Treffer und leerer Begriff müssen leer bleiben: {:?}",
            &many[2..4]
        );
    }

    /// `LeakCheck::literal` trennt den wörtlichen Fund vom Fund, den nur die
    /// Fassung ohne Leerraum gebracht hat.
    ///
    /// Die Oberfläche entscheidet daran, ob ein Rest ein Leck ist oder eine
    /// bewusst stehen gelassene Schreibweise derselben Normalform (Vertrag V1
    /// aus Fix-Runde 5): „DE89 3704“ und „DE893704“ sind ein Text, aber nur
    /// einer von beiden steht wörtlich in der Datei.
    #[test]
    fn literal_says_whether_the_needle_matched_verbatim() {
        // Wörtlich: die Ziffern stehen mit Leerzeichen im Strom.
        let woertlich = b"%PDF-1.5\n1 0 obj\n<< >>\nstream\n(DE89 3704 0044) Tj\nendstream\n";
        // Nur zerlegt: erst die Verkettung ohne Leerraum trifft.
        let zerlegt = b"%PDF-1.5\n1 0 obj\n<< >>\nstream\n[(DE89)-2(3704)-2(0044)] TJ\nendstream\n";
        let needles = ["DE89 3704 0044", "kommtnichtvor", ""];

        let a = leaks_many_within(woertlich, &needles, u64::MAX);
        assert!(!a.findings[0].is_empty());
        assert_eq!(a.literal, vec![true, false, false], "{:?}", a.findings[0]);

        let b = leaks_many_within(zerlegt, &needles, u64::MAX);
        assert!(
            !b.findings[0].is_empty(),
            "die gequetschte Fassung muss treffen: {:?}",
            b.findings[0]
        );
        assert_eq!(
            b.literal,
            vec![false, false, false],
            "wörtlich steht der Text dort nicht: {:?}",
            b.findings[0]
        );
        // Und die Marke sagt dasselbe wie der Meldungstext.
        assert!(b.findings[0].iter().all(|h| h.contains("ohne Leerraum")));
    }

    /// Der Durchgang darf die Begriffe nicht vermischen: jeder Bericht gehört
    /// zu genau seinem Begriff, auch wenn ein leerer dazwischensteht.
    #[test]
    fn each_report_belongs_to_its_own_needle() {
        let pdf = crate::testing::minimal_pdf("Alpha Beta");
        let hits = leaks_many(&pdf, &["Alpha", "", "Beta", "Gamma"]);
        assert_eq!(hits.len(), 4);
        assert!(hits[0].iter().all(|h| h.contains("Alpha")), "{:?}", hits[0]);
        assert!(hits[1].is_empty());
        assert!(hits[2].iter().all(|h| h.contains("Beta")), "{:?}", hits[2]);
        assert!(hits[3].is_empty());
    }

    /// Die Suche ohne Leerraum darf nicht daran hängen, welche *anderen*
    /// Begriffe im selben Lauf stehen.
    ///
    /// Der Durchgang bildet die Leerraum-Fassung eines Datenblocks einmal für
    /// alle Begriffe. Genau hier könnte die Zusammenfassung einen Begriff um
    /// seine Fundstelle bringen — deshalb wird beides geprüft: allein und in
    /// Gesellschaft eines Begriffs ohne Leerraum.
    #[test]
    fn whitespace_free_comparison_survives_the_shared_pass() {
        // Im PDF steht die IBAN in Stücken, gesucht wird sie mit Leerzeichen.
        let pdf =
            b"%PDF-1.5\n1 0 obj\n<< >>\nstream\n[(DE89)-2(3704)-2(0044)] TJ\nendstream\nendobj\n";
        let needle = "DE89 3704 0044";

        let alone = leaks_many(pdf, &[needle]);
        assert!(
            alone[0].iter().any(|h| h.contains("ohne Leerraum")),
            "ohne Leerraum nicht gefunden: {:?}",
            alone[0]
        );

        // Derselbe Begriff neben einem, der gar keinen Leerraum enthält.
        let together = leaks_many(pdf, &["Kontonummer", needle]);
        assert_eq!(
            together[1], alone[0],
            "Fundstellen hängen an der Nachbarschaft"
        );
    }

    /// Ein Durchgang statt einer je Begriff — belegt an der Zahl der
    /// entpackten Streams.
    ///
    /// Die Laufzeit selbst zu messen wäre auf einer geteilten Maschine
    /// wackelig. Gezählt wird stattdessen die Arbeit, die früher je Begriff
    /// anfiel: `Document::load_mem` ist der teuerste Einzelschritt, und die
    /// Rohbyte-Suche über die ganze Datei der zweitteuerste. Beide hängen
    /// hier nur noch an der Datei.
    #[test]
    fn the_file_is_parsed_once_no_matter_how_many_needles() {
        let pdf = crate::testing::demo_statement();
        let mut probe = Probe::new(NEEDLES);
        assert_eq!(
            probe.needles.len(),
            4,
            "der leere Begriff darf nicht gesucht werden"
        );

        // Der Objektgraph wird genau einmal abgelaufen — nachweisbar daran,
        // dass `scan_object_graph` ein `&mut Probe` mit allen Begriffen nimmt
        // und nicht je Begriff aufgerufen wird. Der Aufruf hier ist derselbe
        // wie in `leaks_many`.
        let doc = Document::load_mem(&pdf).expect("Vorlage parsebar");
        scan_object_graph(&doc, &mut probe, &mut Budget::new(u64::MAX));
        let (hits, _, _) = probe.into_hits();
        assert_eq!(hits.len(), NEEDLES.len());
        assert!(
            hits.iter().any(|h| !h.is_empty()),
            "der Objektgraph-Durchgang hat nichts gefunden"
        );
    }
}
