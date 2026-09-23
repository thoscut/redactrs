//! Interpreter für PDF-Content-Streams.
//!
//! Der Scanner läuft den Content-Stream einer Seite durch, führt den
//! Grafik- und Textzustand nach (CTM, `Tm`, `Tf`, `Tc`, `Tw`, `Tz`, `Ts`, `TL`)
//! und berechnet für **jedes einzelne Zeichen** eine Bounding-Box im
//! User-Space.
//!
//! Das Ergebnis (`ShowRecord`) wird von zwei Seiten genutzt:
//!
//! * `extract.rs` baut daraus Textzeilen mit zeichengenauen Koordinaten,
//! * `redact.rs` schreibt daraus den Content-Stream neu und entfernt gezielt
//!   einzelne Glyphen.
//!
//! Form-XObjects (`Do`) werden rekursiv mitverarbeitet — dort steht in vielen
//! generierten PDFs der eigentliche Text.
//!
//! ## Senken (`ContentSink`)
//!
//! Der Interpreter kennt zwei Abnehmer: [`ScanResult`] (nur Text) und den
//! Zeichenoperationen-Sammler aus [`crate::ops`] (Text **und** Grafik). Beide
//! bekommen ihre Daten über dieselbe Durchlaufschleife — insbesondere die
//! Glyphenmathematik (`Tm`, `Trm`, Vorschub) steht nur einmal im Code, in
//! [`show_text`]. Damit können Renderer und Schwärzung nicht auseinanderlaufen.
//!
//! Grafikzustand (Farben, Linien, Clip) wird nur nachgeführt, wenn die Senke
//! über [`ContentSink::wants_graphics`] danach fragt; für die reine
//! Textextraktion kostet der Ausbau also nichts.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::rc::Rc;

use lopdf::content::Operation;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use redact_core::{Point, Rect, RedactError, Result};

use crate::font::{as_f64, font_from_dict, FontInfo};
use crate::matrix::Matrix;
use crate::ops::{PathSeg, Rgb, Stroke};

/// Maximale Rekursionstiefe für verschachtelte Form-XObjects.
///
/// Begrenzt die **Tiefe**, nicht die **Breite** — und die Breite ist die
/// gefährlichere Größe. Sieben Ebenen, in denen jedes Form-XObject dasselbe
/// Unterobjekt *n*-mal zeichnet, ergeben n⁷ Durchläufe: bei n = 8 sind das
/// über zwei Millionen, aus einer Datei von 2 368 Byte, die jede einzelne
/// hier dokumentierte Grenze einhält. Dagegen hilft keine Tiefengrenze,
/// sondern nur das Aufwandskonto [`Budget`].
const MAX_FORM_DEPTH: usize = 8;

/// Grundausstattung des Aufwandskontos: so viele Zeichenoperationen darf ein
/// Seiten-Scan immer auswerten, unabhängig davon, wie groß das Dokument ist.
///
/// Reichlich bemessen, weil hier echte Gestaltung hineinfällt: ein
/// Tabellenraster, das dieselbe Zelle hundertmal zeichnet, ein Formular mit
/// vielen platzierten Bausteinen. Die dichteste gemessene Seite eines
/// 500-seitigen Kontoauszugs braucht rund 200 Operationen — Faktor 5 000.
const BASE_OPERATIONS: usize = 1_000_000;

/// Wie oft der Interpreter denselben Strom im Mittel durchlaufen darf.
///
/// Der springende Punkt bei einer Fächerung ist nicht die Menge an Inhalt,
/// sondern die **Vervielfachung**: aus 400 Byte Zeichenanweisungen werden
/// zwei Millionen Durchläufe. Deshalb wächst das Konto mit dem Inhalt, den
/// die Datei tatsächlich mitbringt — jeder Strom, der zum ersten Mal
/// dekodiert wird, bringt seine Operationen mal diesem Faktor als Guthaben
/// ein.
///
/// Damit steht diese Grenze nicht quer zu `--max-parsed-mb`: wer das Budget
/// für geparste Streams anhebt, hebt das Aufwandskonto automatisch mit an,
/// weil mehr Inhalt auch mehr Guthaben bedeutet. Eine Fächerung profitiert
/// davon nicht — sie bringt ja gerade keinen zusätzlichen Inhalt mit.
const MAX_AMPLIFICATION: usize = 16;

/// Wie viele Glyphen ein einzelner Seiten-Scan liefern darf.
///
/// Anders als das Operationskonto eine **feste Decke**, und zwar mit Absicht:
/// dies ist die eigentliche Speichergröße der Textextraktion. Jede Glyphe wird
/// als [`GlyphItem`] gehalten (Originalbytes, Text, Kasten, Grundlinie) und
/// von [`crate::extract`] noch einmal als `Glyph` kopiert. Gemessen (Release,
/// ein Seiteninhalt knapp unter dem Parse-Budget): 14,6 Mio. Glyphen auf
/// **einer** Seite → 9 306 MB Spitzenspeicher, also rund 640 Byte je Glyphe.
/// Ein Budget, das mit der erlaubten Dateigröße mitwüchse, wüchse hier also
/// in den zweistelligen Gigabytebereich — genau das soll nicht passieren.
///
/// Eine Million Zeichen auf einer Seite ist keine Seite mehr. Eine dichte
/// A4-Textseite trägt 3 000–6 000 Zeichen; die dichteste gemessene Seite eines
/// 500-seitigen Kontoauszugs 6 000. Faktor 160 Luft.
const MAX_GLYPHS_PER_SCAN: usize = 1_000_000;

/// Wie viele Zeichenoperationen der Strom-Zwischenspeicher insgesamt behalten
/// darf.
///
/// Der Zwischenspeicher tauscht Rechenzeit gegen Arbeitsspeicher; ohne Decke
/// wäre das nur die andere Erschöpfung. Gemessen an einer Datei von 74 kB mit
/// 200 verschiedenen Formularen zu je 75 kB Inhalt (1,7 Mio. Operationen,
/// jedes Formular genau einmal gezeichnet): ohne Decke 968 MB Spitzenspeicher
/// gegenüber 13 MB vorher. Eine dekodierte `Operation` kostet gemessen rund
/// 570 Byte — Operator, Operandenvektor und der reichlich große
/// `lopdf::Object`.
///
/// 100 000 Operationen sind danach rund 55 MB. Die Zahl ist so gewählt, dass
/// sie **weit über** jedem echten Fall liegt (ein Formular eines Kontoauszugs
/// trägt einige hundert Operationen) und zugleich weit unter dem, was ohne
/// Decke möglich wäre.
///
/// Gezählt werden **nicht nur** Operationen: auch die
/// `/Resources`-Verzeichnisse (siehe [`stream_cost`] und
/// [`Budget::resource_dicts`]). Ein Strom mit *null* Operationen zählte sonst
/// *null* und wäre damit gratis. Gemessen an Formularen mit leerem Rumpf und je
/// 800 Ressourceneinträgen: der Scan legte 12,8 kB je Strom an — über das
/// hinaus, was das Dokument selbst belegt —, und zwar linear ohne Ende
/// (2 000 Ströme 25,5 MB, 5 000 Ströme 62,6 MB), während der Zähler die ganze
/// Zeit „0“ ablas.
///
/// **Wo genau das anfällt, entscheidet, wie es zu zählen ist.** Ein
/// Verzeichnis mit **eigener Objekt-Id** wird geteilt: *n* Formulare, die es
/// erben, kosten es einmal, und es wiegt hier einmal. Ein direkt im Stromdict
/// eingebettetes gehört genau einem Strom und wiegt bei dessen Eintrag. Wer
/// beides gleich behandelte, zählte entweder dasselbe *n*-mal (falsche Zahl)
/// oder eine echte *n*-fache Kopie einmal (die Lücke, aus der 1 037 MB aus
/// einer 7,6-MB-Datei wurden).
///
/// Wichtig: gerade der gefährliche Fall braucht kaum Platz. Ein
/// Zwischenspeicher zahlt sich nur aus, wenn **derselbe** Strom mehrfach
/// gezeichnet wird — und dann ist es *ein* Eintrag, gleich wie oft. Die
/// Dateien, die die Decke reißen, sind die mit vielen **verschiedenen**
/// Strömen, und die gewinnen ohnehin nichts.
///
/// Ist sie erreicht, wird **nicht verdrängt**, sondern nur nichts mehr
/// aufgenommen: was schon drin ist, bleibt gültig, alles Weitere wird wie
/// vorher je Platzierung neu ausgepackt. Der schlechteste Fall ist damit die
/// Laufzeit von vorher — nie ein falsches Ergebnis.
const MAX_CACHED_OPERATIONS: usize = 100_000;

/// Wie viele **Tabelleneinträge** an Schriften der Zwischenspeicher insgesamt
/// behalten darf.
///
/// Die frühere Decke zählte *Verzeichnisse* (16), und das war die falsche
/// Einheit: gemessen belegte **ein** Verzeichnis mit 40 Namen auf dieselbe
/// schwere Schrift 279 MB, bei 300 Namen wären es rund 2 GB — allesamt weit
/// unter der Decke. Und selbst innerhalb der Decke sind 16 Verzeichnisse mit je
/// ein paar schweren Schriften mehrere hundert MB. Die Begründung, 16 sei „rund
/// das Doppelte dessen, was der Interpreter ohnehin hält“, trug außerdem nur
/// für ein achtfach geschachteltes Dokument; auf einer **flachen** Seite hält
/// der Interpreter genau eines.
///
/// Gezählt wird jetzt, was wirklich Platz kostet: die Einträge der geladenen
/// Tabellen, siehe [`FontInfo::weight`]. Jede Schrift wird dabei **genau
/// einmal** berechnet — der Zwischenspeicher liegt auf der Objekt-Id des
/// Schriftobjekts, nicht auf dem Ressourcennamen (siehe
/// [`Budget::font_object`]). Zweihundert Namen auf dasselbe Objekt kosten
/// deshalb einmal Tabelle und zweihundertmal einen Zeiger.
///
/// 400 000 Einträge sind nach der Abschätzung in [`FontInfo::weight`] rund
/// 40 MB. Zum Vergleich: die größte ehrliche Schrift ist eine CJK-Schrift mit
/// vollem Umfang, also höchstens 65 536 Einträge
/// ([`crate::encoding::MAX_TO_UNICODE_BYTES`]) — sechs davon gleichzeitig
/// gemerkt passen noch hinein, und eine Seite eines Kontoauszugs trägt eine
/// Handvoll Schriften mit je ein paar hundert Einträgen.
///
/// Ist sie erreicht, wird wie beim Strom-Zwischenspeicher **nicht verdrängt**,
/// sondern nur nichts mehr aufgenommen; was darüber liegt, wird je Platzierung
/// neu geladen wie vor der Änderung.
const MAX_CACHED_FONT_ENTRIES: usize = 400_000;

/// Wie viele Schrift-Tabelleneinträge ein Seiten-Scan insgesamt aufmachen
/// darf, bevor die Datei **abgelehnt** wird.
///
/// [`MAX_CACHED_FONT_ENTRIES`] ist eine weiche Decke: sie sagt nur, was
/// *gemerkt* wird. Was ein Verzeichnis geladen hat, bleibt daneben trotzdem
/// vollständig lebendig — [`load_font_map`](Budget::load_font_map) legt jede
/// Schrift des Verzeichnisses in dieselbe [`FontMap`], und die hält der
/// Interpreter, solange er in diesem Strom ist. Die weiche Decke sieht das
/// nicht.
///
/// Gemessen (zählender Allokator, Release, je Schrift eine `/ToUnicode` über
/// den vollen Bereich von 65 536 Einträgen, alle in **einem** `/Resources`;
/// gesetzt wird nur mit der ersten):
///
/// | Schriften | Datei    | Spitze  |
/// |----------:|---------:|--------:|
/// |         1 |   0,9 kB |  4,0 MB |
/// |        10 |   4,9 kB | 39,8 MB |
/// |        50 |  22,5 kB | 198,9 MB |
/// |       100 |  44,8 kB | 397,7 MB |
///
/// Linear, rund 4 MB je Schrift, bei rund 450 Byte Dateizuwachs je Schrift —
/// **Faktor 9 000**, und ohne jede Decke. Eine Datei von einem Megabyte käme
/// so auf neun Gigabyte.
///
/// Warum **ablehnen** und nicht stillschweigend weniger laden: eine Schrift,
/// die nicht geladen ist, hat keine `/ToUnicode`-Zuordnung, und ihr Text wird
/// dann falsch oder gar nicht dekodiert. Für ein Werkzeug, das Geheimnisse
/// suchen soll, ist „ich habe den Text nicht gelesen“ kein zulässiges
/// Zwischenergebnis — das ist dieselbe Begründung wie bei
/// [`crate::document::Limits`].
///
/// Warum diese Zahl: sie muss über dem liegen, was ein ehrliches Dokument
/// braucht, und darunter, was die Maschine umwirft. Eine Million Einträge sind
/// nach der Abschätzung in [`FontInfo::weight`] rund 60 MB und entsprechen
/// **fünfzehn** CJK-Schriften vollen Umfangs auf einer Seite. Eine Seite mit
/// fünf bis zehn Schriften ist normal; fünfzehn *volle* CJK-Schriften ist es
/// nicht. Der Test `rev4_interpreter_ceilings::die_schriftendecke_haelt`
/// (sieben mal 65 536 = 458 752) liegt ausdrücklich darunter und läuft
/// weiterhin durch.
///
/// Gezählt wird je **Schriftobjekt genau einmal** (siehe
/// [`Budget::charged_fonts`]) — dieselbe Schrift unter zweihundert Namen oder
/// in zwanzig Verzeichnissen kostet einmal.
const MAX_FONT_ENTRIES_PER_SCAN: usize = 1_000_000;

/// Wie viele **Zuordnungen zwischen einem Textspiegel und dem, was unter ihm
/// steht**, ein Seiten-Scan insgesamt führen darf.
///
/// Gezählt wird `Σ|record.forms|` bzw. `Σ|record.shows|` — an **drei**
/// Stellen, an denen diese Zuordnungen entstehen, und zwar je Stelle einmal:
///
/// * beim **Aufbau** in [`scan_marked_text`]: jede Spiegel-Klammer nimmt jedes
///   `Do` in ihrem Bereich auf. `B` verschachtelte `BDC`-Klammern über `D`
///   Platzierungen ergeben `B × D` Paare — aus einer Datei, die dafür keinen
///   Inhalt mitbringen muss;
/// * ebenda für die **Textoperationen**: dieselbe Produktstruktur, nur die
///   andere Liste (`B × S`). Sie stand bis Fix-Runde 7 unter keiner Schranke —
///   6 000 Klammern über 6 000 `Tj` aus 263 kB ergaben 36 Mio. Zuordnungen,
///   und der Extraktor lief darüber 31,1 s (Debug, eigener Prozess);
/// * beim **Aufklappen** in [`ScanResult::close_forms`]: zeichnet ein Formular
///   im Bereich seinerseits Formulare, kommt jede dieser Platzierungen dazu.
///
/// Drei Zähler, eine Zahl: sie messen verschiedene Arbeit (Klammern × `Do`,
/// Klammern × `Tj`, und der Baum der Formulare darunter), und keiner soll den
/// anderen aufbrauchen. Zusammen tragen sie höchstens 300 000 Zuordnungen,
/// also rund 24 MB.
///
/// Bis Fix-Runde 6 zählte nur die zweite Stelle, und ihre Zahl war lokal in
/// der Schließung. Der Aufbau lief ganz ohne Schranke und vor der ersten
/// gezählten Operation.
/// Gemessen (Debug, je eigener Prozess, `zf_q3_kombinatorik::mess_ein_fall`):
///
/// | Datei (Klammern × `Do`) | vorher | nachher |
/// |---|---|---|
/// | 92 kB (2 000 × 2 000)  | 4,71 s / 268 MB    | 0,26 s / 22 MB |
/// | 184 kB (4 000 × 4 000) | 20,6 s / 1 036 MB  | 0,41 s / 30 MB |
/// | 276 kB (6 000 × 6 000) | 41,7 s / 2 306 MB  | 0,56 s / 38 MB |
///
/// (Debug, je eigener Prozess, `VmHWM`; die dritte Zeile vorher ist die
/// Messung des Gegenprüfers, die ersten beiden nachgestellt — 4,4 s / 268 MB
/// und 17,5 s / 1 036 MB bei ihm.) Dieselbe Struktur **ohne** `/ActualText`
/// braucht 0,15 s / 14 MB: der ganze Unterschied lag an dieser einen Liste.
/// Der Fall, in dem die alte Decke zu spät griff (138 kB, 9 Mio.
/// Grundplatzierungen unter den Spiegeln: 16,6 s, 601 MB, Rückgabewert 3),
/// braucht jetzt 0,56 s und 33 MB.
///
/// **Platzierung, nicht Formular.** Dasselbe Formular zweimal unter einem
/// Spiegel zeichnet zweimal und zählt zweimal (siehe
/// [`ScanResult::close_forms`]).
///
/// 100 000 sind die Decke. Kosten der Schließung, gemessen (Debug,
/// `ze_p2_spiegel::mess_zehntausend_platzierungen`, dieselbe Seite je einmal
/// mit und einmal ohne den Spiegel darüber):
///
/// | Platzierungen | ohne Spiegel | mit Spiegel | Mehrspeicher |
/// |--------------:|-------------:|------------:|-------------:|
/// |        10 000 |      0,31 s  |     0,30 s  |      0,6 MB  |
/// |        20 000 |      0,60 s  |     0,64 s  |      2,8 MB  |
/// |110 000 (Decke)|      3,29 s  |     3,53 s  |      8,5 MB  |
///
/// Ein Paar kostet rund 80 Byte, an der Decke also unter 10 MB. Wird sie
/// erreicht, **und nur dann, wenn dabei wirklich etwas weggefallen ist**,
/// bekommt die Seite eine Warnung: die Glyphenzahl unter den letzten Spiegeln
/// ist dann unvollständig, und das muss dastehen, statt still einen falschen
/// Vergleich zu ergeben. Genau `MAX_MIRROR_FORM_PLACEMENTS` Zuordnungen gehen
/// dagegen auf und bleiben still (Befund Q3-1b).
///
/// **Die Decke gilt je Seiten-Scan**, wie jede andere in [`Budget`]: über ein
/// Dokument summiert sich also Seitenzahl × 300 000 Zuordnungen. Dokumentweit
/// zu zählen hieße, dieselbe Seite je nach ihren Nachbarn zu warnen oder
/// nicht — die Meldung wäre nicht mehr eine Aussage über diese Seite, und
/// [`scan_page`] ist öffentlich und seitenweise.
///
/// Die frühere Begründung dafür — „die Kosten bleiben gedeckelt, weil jede
/// Seite ihren eigenen Inhalt mitbringen muss“ — war **falsch**: `/Contents`
/// darf auf denselben Strom zeigen wie die Nachbarseite (PDF 32000-1,
/// Tabelle 30), und 1 000 Seiten an einem Strom von 9 kB zahlten jede die
/// volle Decke. Was daraus wurde, entschied nicht der Scan, sondern der
/// Redaktor, der die Spiegel jeder Seite bis zum Ende festhielt: gemessen
/// 224 752 Byte Eingabe, 6 439 MB Spitze (Debug). Gedeckelt wird das jetzt
/// dort, wo es anfällt — `crate::redact::MAX_DEFERRED_MIRRORS`; der Scan bleibt
/// seitenweise. Beleg:
/// `zg_r1_decke::jede_seite_zahlt_die_volle_decke_aus_einem_geteilten_strom`.
const MAX_MIRROR_FORM_PLACEMENTS: usize = 100_000;

/// Schriften eines Ressourcenverzeichnisses: Ressourcenname → Metriken.
///
/// Der Wert ist ein [`Rc`] und kein [`FontInfo`]: dieselbe Schrift steht oft
/// unter mehreren Namen und in mehreren Verzeichnissen, und der Grafikzustand
/// des Interpreters führt sie bei jedem `Tf` und jedem `q` mit. Als Wert
/// gehalten war das der teuerste Einzelposten des Scanners — gemessen 3,8 ms
/// je `Tf` bei einer Schrift mit 125 000 CMap-Einträgen, also linear in
/// Operationen × Tabellengröße, bei einem Aufwandskonto, das 1 000 000
/// Operationen zulässt.
type FontMap = BTreeMap<Vec<u8>, Rc<FontInfo>>;

/// Was ein Seiten-Scan an **Vorarbeit** gekostet hat.
///
/// Beides sind Arbeiten, die *vor* der ersten gezählten Zeichenoperation
/// anfallen und deshalb von keiner Schranke des Aufwandskontos gesehen
/// werden: einen Strom auszupacken und zu zerlegen, und ein
/// Ressourcenverzeichnis in Schriftmetriken zu übersetzen. Genau daraus
/// bestand die Vervielfachung durch mehrfach platzierte Form-XObjects — je
/// Platzierung einmal, obwohl das Ergebnis jedes Mal dasselbe ist.
///
/// Die Zahlen sind **deterministisch**: sie hängen an der Datei, nicht an der
/// Maschine. Ein Test kann damit die Aufwandsschranke festhalten, ohne eine
/// Uhr zu befragen.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ScanEffort {
    /// Wie oft ein Strom ausgepackt und in Operationen zerlegt wurde.
    ///
    /// Ohne den Seitenstrom selbst — der wird ohnehin nur einmal dekodiert.
    /// Ein hundertmal platziertes Formular zählt hier **einmal**.
    pub decoded_streams: usize,
    /// Wie oft ein `/Resources`-Verzeichnis in Schriftmetriken übersetzt
    /// wurde (`/ToUnicode`, `/W`, `/Widths`, eingebettete `cmap`).
    ///
    /// Nicht dasselbe wie „wie viele Schriften geparst wurden“: dieselbe
    /// Schrift unter zwanzig Namen wird **einmal** geparst (siehe
    /// [`Budget::font_object`]), das Verzeichnis aber einmal übersetzt.
    pub loaded_font_maps: usize,
    /// Wie oft ein **Schriftobjekt** wirklich geparst wurde.
    ///
    /// Das ist die Zahl, an der die Arbeit hängt: `/ToUnicode` zu zerlegen
    /// kostet Zeit *und* Platz, und dasselbe Objekt zweimal zu zerlegen kostet
    /// beides doppelt. Ein Verzeichnis, das dieselbe Schrift unter dreihundert
    /// Namen führt, muss hier **eins** liefern; die Datei wächst dafür um zehn
    /// Byte je Name.
    pub parsed_fonts: usize,
    /// Wie oft die `/XObject`-Liste eines Ressourcenverzeichnisses durchgegangen
    /// wurde, um der Senke die darin stehenden Formulare anzubieten.
    ///
    /// Die Zahl hängt an den **Verzeichnissen** der Datei, nicht an den
    /// Platzierungen und nicht an den Strömen: *n* Formulare, die sich ein
    /// Verzeichnis teilen, ergeben eine Durchsicht, nicht *n*. Siehe
    /// [`DeclarationKey`].
    pub declared_resources: usize,
    /// Was der Zähler des Strom-Zwischenspeichers am Ende des Scans ablas —
    /// die Zahl, die gegen [`MAX_CACHED_OPERATIONS`] steht.
    ///
    /// Hier steht sie, damit ein Test sie **lesen** kann: eine Decke, deren
    /// Zähler nicht misst, was sie begrenzen soll, sieht von außen genauso aus
    /// wie eine, die hält. Genau das war der Fehler, den diese Zahl sichtbar
    /// macht — 50 000 Ströme lasen 50 000 ab und belegten 1 011 MB.
    pub retained_weight: usize,
}

/// Woher ein `/Resources`-Verzeichnis stammt — der Schlüssel des
/// Schriften-Zwischenspeichers.
///
/// Beide Fälle benennen das Verzeichnis **eindeutig innerhalb dieses
/// Dokuments**, und mehr braucht es nicht: [`fonts_from_resources`] liest
/// ausschließlich aus dem Verzeichnis und den Schriftobjekten, an denen es
/// hängt. Weder CTM noch Textzustand noch der Fundort gehen ein.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(clippy::enum_variant_names)]
enum ResourceKey {
    /// Eigenes Objekt (`/Resources 7 0 R`) — mehrere Ströme können sich
    /// dasselbe teilen, und dann teilen sie sich auch den Eintrag.
    Object(ObjectId),
    /// Direkt im Dictionary des Stroms — gehört genau diesem einen Strom.
    InStream(ObjectId),
}

/// Ein platzierbarer Strom — Form-XObject, Gruppen-Form einer weichen Maske
/// oder Kachelmuster —, **einmal** ausgepackt und zerlegt.
///
/// Alles hier drin ist eine reine Funktion des Stromobjekts: Auspacken und
/// Zerlegen kennen weder den Fundort noch den Grafik- oder Textzustand. Was
/// vom Zusammenhang abhängt, steht deshalb bewusst **nicht** hier:
///
/// * die **Beschriftung** (der Ressourcenname, unter dem der Strom gefunden
///   wurde) — sie geht nur in Warnungen ein und wird je Platzierung gebildet,
/// * die **CTM** — sie entsteht je Platzierung aus `/Matrix` und dem
///   Grafikzustand des Augenblicks,
/// * die **geerbten Ressourcen**: bringt der Strom kein eigenes
///   `/Resources` mit, gelten die des Aufrufers. Genau dafür steht hier
///   `None` statt einer Kopie — sonst wäre der Zwischenspeicher der stille
///   Fehler, vor dem er bewahren soll,
/// * **Tiefe**, Zyklusprüfung und Textzustand — die führt der Interpreter
///   ohnehin je Durchlauf.
#[derive(Debug)]
struct PlacedStream {
    /// `None`: der Strom ließ sich gar nicht auspacken (defekter oder
    /// unbekannter Filter).
    content: Option<crate::ops::DecodedContent>,
    /// Stand in den ausgepackten Bytes überhaupt etwas anderes als Leerraum?
    has_tokens: bool,
    /// Das **eigene** `/Resources` des Stroms, bereits aufgelöst. `None`
    /// heißt: er bringt keins mit und erbt das des Aufrufers.
    ///
    /// Ein [`Rc`] und keine eigene Kopie: *n* Formulare, die per Referenz
    /// dasselbe Verzeichnis erben, hielten sonst *n* vollständige Kopien
    /// davon — siehe [`Budget::resource_dicts`]. Am Vererbungsverhalten
    /// ändert das nichts: entschieden wird weiterhin allein daran, **ob**
    /// hier etwas steht, und das steht genau dann, wenn der Strom ein eigenes
    /// `/Resources` mitbringt.
    resources: Option<Rc<Dictionary>>,
    /// Steht [`PlacedStream::resources`] im geteilten Zwischenspeicher?
    ///
    /// `true` heißt: das Verzeichnis ist dort einmal abgelegt und einmal
    /// verbucht; dieser Strom hält nur einen Zeiger darauf und kostet nichts
    /// extra. `false` heißt: die Kopie gehört diesem [`PlacedStream`] allein
    /// und wiegt deshalb bei [`stream_cost`] voll mit.
    shared_resources: bool,
    /// Unter welchem Schlüssel die Schriften zu [`PlacedStream::resources`]
    /// im Zwischenspeicher stehen — belegt genau dann, wenn jenes belegt ist.
    ///
    /// Hier steht der **Schlüssel** und nicht die Schriften selbst: sonst
    /// hinge deren Lebensdauer am Strom-Zwischenspeicher, und dessen Decke
    /// zählt Operationen, nicht Schriften. Eine Datei mit dreihundert kleinen
    /// Formularen, die alle dieselben schweren Schriften aufzählen, käme so
    /// unter jeder Operationsdecke durch und belegte trotzdem ein Gigabyte
    /// (gemessen). Über den Schlüssel entscheidet allein
    /// [`MAX_CACHED_FONT_MAPS`], wie viel liegen bleibt.
    font_key: Option<ResourceKey>,
}

impl PlacedStream {
    /// Die Operationen, oder eine leere Liste, wenn nichts zu holen war.
    fn operations(&self) -> &[Operation] {
        match &self.content {
            Some(content) => &content.operations,
            None => &[],
        }
    }

    /// Ließ sich der Strom überhaupt auspacken?
    fn decodable(&self) -> bool {
        self.content.is_some()
    }

    /// Ging beim Zerlegen etwas verloren?
    fn truncated(&self) -> bool {
        self.content
            .as_ref()
            .is_some_and(|c| !c.truncated.is_empty())
    }

    /// Größe der Teilstücke, in denen etwas fehlt.
    fn affected_bytes(&self) -> usize {
        self.content.as_ref().map_or(0, |c| c.affected_bytes())
    }
}

/// Was ein gemerkter [`PlacedStream`] gegen [`MAX_CACHED_OPERATIONS`] zählt.
///
/// Die Operationen — und das `/Resources`-Verzeichnis genau dann, wenn dieser
/// Eintrag es **allein** hält. Steht es im geteilten Zwischenspeicher
/// ([`Budget::resource_dicts`]), ist es dort schon einmal verbucht; es hier ein
/// zweites Mal zu zählen wäre keine Vorsicht, sondern eine falsche Zahl: der
/// Strom hält davon nur einen Zeiger.
fn stream_cost(placed: &PlacedStream) -> usize {
    let resources = match (&placed.resources, placed.shared_resources) {
        (Some(dict), false) => dictionary_objects(dict),
        _ => 0,
    };
    placed.operations().len().saturating_add(resources)
}

/// Wie schwer ein Dictionary wiegt, Verschachtelung mitgezählt.
///
/// Die Einheit ist der **Eintrag**: ein Dictionary- oder Array-Platz zählt
/// eins, wie eine Zeichenoperation. Beide sind in derselben Größenordnung
/// (`lopdf::Object` allein ist 120 Byte), und daran ist
/// [`MAX_CACHED_OPERATIONS`] geeicht.
///
/// ## Warum Zeichenketten nach ihrer Länge zählen
///
/// Eine PDF-Zeichenkette ist der eine Wert, dessen Platzbedarf **nicht** an der
/// Zahl der Einträge hängt: `/Junk (AAA…)` ist *ein* Eintrag und kann ein
/// Megabyte wiegen. Gezählt wird deshalb ihre Länge in Byte — großzügig
/// gerechnet (ein Byte zählt wie ein ganzer Eintrag), aber in der richtigen
/// Richtung: eine Decke, die die schwerste Sorte Wert als „eins“ liest, ist
/// keine. Dasselbe für den Rumpf eines Streams, falls einer direkt in einem
/// Verzeichnis steht.
///
/// In einem echten `/Resources` kommen Zeichenketten praktisch nicht vor — es
/// besteht aus Namen und Referenzen. Die großzügige Rechnung kostet also nichts
/// und greift nur da, wo jemand sie ausnutzen wollte.
///
/// Iterativ und nicht rekursiv: das Verzeichnis stammt aus der Datei, und eine
/// Schachtelungstiefe daraus darf nicht zum Stapelüberlauf werden. Gezählt wird
/// nur bis [`MAX_CACHED_OPERATIONS`] — mehr braucht die Entscheidung
/// „behalten oder nicht“ nicht zu wissen.
fn dictionary_objects(dict: &Dictionary) -> usize {
    let mut count = dict.len();
    let mut todo: Vec<&Object> = dict.iter().map(|(_, value)| value).collect();
    while let Some(object) = todo.pop() {
        if count >= MAX_CACHED_OPERATIONS {
            break;
        }
        match object {
            Object::Dictionary(inner) => {
                count = count.saturating_add(inner.len());
                todo.extend(inner.iter().map(|(_, value)| value));
            }
            Object::Array(items) => {
                count = count.saturating_add(items.len());
                todo.extend(items.iter());
            }
            Object::String(bytes, _) => count = count.saturating_add(bytes.len()),
            Object::Stream(stream) => {
                count = count
                    .saturating_add(stream.content.len())
                    .saturating_add(stream.dict.len());
                todo.extend(stream.dict.iter().map(|(_, value)| value));
            }
            _ => {}
        }
    }
    count
}

/// Woran die Sperre für [`declare_forms`] hängt.
///
/// Am **Verzeichnis**, nicht am Strom: die Arbeit ist, die `/XObject`-Liste
/// durchzugehen und jeden Eintrag aufzulösen, und die hängt allein am
/// Verzeichnis. *n* Formulare ohne eigenes `/Resources`, die dasselbe
/// `/XObject` mit *n* Einträgen erben, ergaben mit einer Sperre je Strom *n*²
/// Auflösungen — gemessen 4,65 s für 4 000 Formulare aus einer Datei von
/// 564 kB und 21,9 s für 8 000 aus 1,1 MB, gegen 0,038 s bzw. 0,087 s danach.
/// Quadratisch, aus einer Datei, die jede dokumentierte Grenze einhält.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum DeclarationKey {
    /// Die `/XObject`-Liste ist ein eigenes Objekt — dann teilen sich alle
    /// Ströme, die sie erben, auch die Sperre.
    XObjects(ObjectId),
    /// Direkt im Verzeichnis eingebettet: dann gehört sie zu genau diesem
    /// Strom, und der Strom ist der Ersatzschlüssel.
    InStream(StreamKey),
}

/// Ein Eintrag des Strom-Zwischenspeichers.
///
/// Der Eintrag wird für **jeden** einmal ausgepackten Strom angelegt, auch
/// wenn sein Rumpf nicht behalten wird: an ihm hängt die Buchführung des
/// Aufwandskontos, und die muss unabhängig davon stimmen, wie viel Platz
/// gerade noch da war.
#[derive(Debug)]
struct CachedStream {
    /// Der Rumpf — `None`, sobald [`MAX_CACHED_OPERATIONS`] erreicht war.
    /// Dann wird bei jeder Platzierung neu ausgepackt, wie vor der Änderung.
    body: Option<Rc<PlacedStream>>,
    /// Hat dieser Strom schon einmal Guthaben eingebracht? Ein zweites Mal
    /// zählt er nicht — sonst finanzierte die Fächerung sich selbst.
    credited: bool,
}

/// Aufwandskonto eines Seiten-Scans.
///
/// Die dokumentierten Grenzen für Eingabedateien messen **Bytes**. Was der
/// Speicher wirklich kostet, sind aber *Interpretationen*: eine Datei von
/// 2 368 Byte hält jede Byte-Grenze ein und erzeugt trotzdem zwei Millionen
/// Durchläufe durch dieselben acht Ströme (siehe [`MAX_FORM_DEPTH`]). Deshalb
/// zählt dieses Konto das, was tatsächlich anfällt, und wächst mit dem
/// Inhalt, den die Datei mitbringt — nicht mit dem, was sie daraus macht.
///
/// Ist es leer, wird der Scan **abgebrochen und die Datei abgelehnt** — nicht
/// gewarnt. Eine Seite, deren Text nur zum Teil durchsucht wurde, darf nicht
/// als Erfolg enden: der ungeprüfte Rest ist genau der, in dem das Geheimnis
/// stehen kann.
///
/// ## Warum der Zwischenspeicher hier steht
///
/// Das Konto zählte schon immer, **welcher** Strom seinen Inhalt bereits
/// gutgeschrieben bekommen hat. Es fehlte nur das Ergebnis: derselbe Strom
/// wurde je Platzierung erneut ausgepackt und zerlegt, und sein
/// Schriftenverzeichnis erneut geladen — Arbeit, die vor der ersten gezählten
/// Operation anfällt und deshalb von keiner Schranke gesehen wurde. Seit hier
/// das Ergebnis steht statt nur des Häkchens, kostet die zweite Platzierung
/// eines Formulars nichts mehr als das Nachschlagen.
#[derive(Debug)]
struct Budget {
    /// Verbleibendes Guthaben an Zeichenoperationen.
    operations: usize,
    glyphs: usize,
    /// Einmal ausgepackte und zerlegte Ströme, je Objekt-Id — samt der
    /// Auskunft, ob der Strom schon Guthaben eingebracht hat.
    streams: HashMap<ObjectId, CachedStream>,
    /// Einmal aufgelöste `/Resources`-Verzeichnisse, je **Ressourcenobjekt**.
    ///
    /// Das ist die Ebene, auf der der Platz wirklich anfällt. Vorher hielt
    /// **jeder** [`PlacedStream`] eine vollständige eigene Kopie seines
    /// aufgelösten Verzeichnisses — auch dann, wenn tausend Formulare per
    /// Referenz dasselbe Objekt erben. Gemessen an 50 000 Formularen, die sich
    /// ein Verzeichnis mit einer 16-kB-Zeichenkette teilen: **1 037 MB**
    /// Spitzenspeicher aus einer Datei von 7,6 MB, Faktor 136 — bei einem
    /// Zähler, der dabei 50 000 von 100 000 ablas. Die Decke sah den Platz
    /// nicht, weil er gar nicht in ihrer Einheit anfiel.
    ///
    /// Geteilt wird nur, was eine **eigene Objekt-Id** hat: nur dort gibt es
    /// die Vervielfachung (ein Objekt, viele Ströme). Ein direkt im Stromdict
    /// eingebettetes `/Resources` gehört ohnehin genau einem Strom und wiegt
    /// deshalb weiter bei [`stream_cost`] mit.
    ///
    /// Auch dieser Zwischenspeicher steht unter [`MAX_CACHED_OPERATIONS`];
    /// was nicht mehr hineinpasst, wird wie vorher je Platzierung kopiert —
    /// und der zugehörige Strom wird dann **nicht** gemerkt, sonst bliebe die
    /// Kopie doch liegen. Damit ist zu jedem Zeitpunkt höchstens eine solche
    /// Kopie je Schachtelungsebene am Leben.
    ///
    /// An der Vererbung ändert das nichts: der Schlüssel ist die Objekt-Id des
    /// Verzeichnisses, nicht der Fundort und nicht der Ressourcenname. Zwei
    /// Umgebungen mit verschiedenen Verzeichnissen haben verschiedene Ids und
    /// bekommen weiterhin verschiedene Antworten.
    resource_dicts: HashMap<ObjectId, Rc<Dictionary>>,
    /// Einmal geladene Schriftenverzeichnisse, je Ressourcenobjekt.
    fonts: HashMap<ResourceKey, Rc<FontMap>>,
    /// Einmal geparste Schriften, je **Schriftobjekt**.
    ///
    /// Das ist die Ebene, auf der das Parsen wirklich anfällt. Ein
    /// Ressourcenverzeichnis nennt oft dieselbe Schrift mehrfach, und
    /// verschiedene Verzeichnisse nennen erst recht dieselben Schriften;
    /// gemessen kostete jeder dieser Namen vorher eine vollständige eigene
    /// Kopie der Tabellen.
    font_objects: HashMap<ObjectId, Rc<FontInfo>>,
    /// Ressourcenverzeichnisse, deren `/XObject`-Liste schon angeboten wurde —
    /// siehe [`declare_forms`].
    declared_forms: HashSet<DeclarationKey>,
    /// Wie viele Operationen in [`Budget::streams`] liegen — die Decke ist
    /// [`MAX_CACHED_OPERATIONS`].
    cached_operations: usize,
    /// Wie viele Schrift-Tabelleneinträge der Zwischenspeicher festhält — die
    /// Decke ist [`MAX_CACHED_FONT_ENTRIES`].
    cached_font_entries: usize,
    /// Wie viele Tabelleneinträge dieser Scan **insgesamt** aufgemacht hat —
    /// die harte Decke ist [`MAX_FONT_ENTRIES_PER_SCAN`].
    seen_font_entries: usize,
    /// Welche Schriftobjekte darauf schon gebucht sind. Ohne diese Menge
    /// zählte ein Verzeichnis mit derselben Schrift unter zweihundert Namen
    /// zweihundertmal — und über der weichen Decke zählte jede erneute
    /// Ladung noch einmal.
    charged_fonts: HashSet<ObjectId>,
    /// Die Vorarbeit, die wirklich anfiel — siehe [`ScanEffort`].
    effort: ScanEffort,
    /// Type3-Schriften, deren Glyphprozeduren schon untersucht wurden — je
    /// Strom und Ressourcenname.
    ///
    /// Ohne diese Sperre kostete **jedes** `Tj` einen vollen Durchlauf durch
    /// alle `/CharProcs` der Schrift. Genau daraus bestünde die nächste
    /// Vervielfachung: eine Schrift mit tausend Glyphprozeduren, tausendmal
    /// gesetzt. Was danach doch untersucht wird, zahlt regulär vom Konto.
    looked_at_type3: HashSet<(StreamKey, Vec<u8>)>,
    /// Ströme, deren Textspiegel schon eingesammelt sind — je Strom **und
    /// Ressourcenumgebung**, siehe [`scan_marked_text`].
    ///
    /// Die Klammerstruktur ist rein syntaktisch und bei jeder Platzierung
    /// dieselbe; die Senke wirft die Wiederholung ohnehin weg
    /// ([`ScanResult::marked`] entdoppelt über `(Strom, Operationsindex)`) —
    /// nur bezahlt hat sie bis Fix-Runde 6 niemand, und die Decke
    /// [`MAX_MIRROR_FORM_PLACEMENTS`] hätte sie mitgezählt: ein zwanzigmal
    /// platziertes Formular mit fünftausend Spiegel-Paaren hätte sie erreicht,
    /// obwohl der Datensatz am Ende fünftausend Paare führt.
    ///
    /// **Rein syntaktisch ist der Durchlauf aber nicht.** `/Span /MC0 BDC`
    /// nennt die Eigenschaftsliste beim Ressourcennamen, und `/Fm0 Do`
    /// ebenso; beide lösen sich in den Ressourcen der **Platzierung** auf. Ein
    /// Form-XObject ohne eigenes `/Resources` erbt die des Aufrufers (PDF
    /// 32000-1, 8.10.1) und sieht unter zwei Platzierungen zwei verschiedene
    /// `/Properties`. Der Schlüssel trägt deshalb den Eigentümer der
    /// wirksamen Ressourcen mit: dasselbe Formular unter zwei Umgebungen wird
    /// zweimal abgelaufen, dasselbe Formular zwanzigmal unter derselben
    /// Umgebung weiterhin einmal. Bis Fix-Runde 6 stand hier nur der Strom —
    /// der zweite Spiegel blieb dann mit seinem Klartext stehen (Befund R1-2).
    scanned_marked: HashSet<(StreamKey, Option<ObjectId>)>,
    /// Verbleibende Spiegel-Formular-Paare, die [`scan_marked_text`] noch
    /// **aufnehmen** darf — die Decke ist [`MAX_MIRROR_FORM_PLACEMENTS`].
    ///
    /// Steht **hier** und nicht in [`scan_marked_text`] selbst, weil die
    /// Funktion je Strom aufgerufen wird und die Decke für den ganzen
    /// Seiten-Scan gilt: eine Seite mit hundert Strömen darf nicht hundertmal
    /// so viel aufnehmen wie eine mit einem.
    mirror_pairs: usize,
    /// Dasselbe für das **Aufklappen** in [`ScanResult::close_forms`].
    ///
    /// Ein eigener Zähler, weil es eine eigene Arbeit ist: der Aufbau wächst
    /// mit Klammern × `Do` des Stroms, das Aufklappen mit dem Baum der
    /// Formulare darunter. Jede der beiden trägt für sich höchstens
    /// [`MAX_MIRROR_FORM_PLACEMENTS`] Paare bei; zusammen also höchstens das
    /// Doppelte, rund 16 MB.
    mirror_expansions: usize,
    /// Verbleibende Spiegel-Text-Zuordnungen, die [`scan_marked_text`] noch
    /// aufnehmen darf — die Decke ist ebenfalls
    /// [`MAX_MIRROR_FORM_PLACEMENTS`].
    ///
    /// Dieselbe Produktstruktur wie beim Aufbau der Formularliste, nur die
    /// andere Liste: `B` verschachtelte Klammern über `S` Textoperationen
    /// ergeben `B × S` Einträge in [`MarkedTextRecord::shows`]. Bis
    /// Fix-Runde 7 stand davor keine Schranke — gemessen 6 000 × 6 000 aus
    /// einer Datei von 263 kB: 36 Mio. Zuordnungen, `scan_page` 0,72 s /
    /// 315 MB, der **Extraktor** 31,1 s, der Redaktor 588 MB.
    mirror_shows: usize,
    /// Wurde an einer der beiden Formular-Decken etwas weggelassen?
    mirror_forms_cut: bool,
    /// Wurde an der Text-Decke etwas weggelassen?
    mirror_shows_cut: bool,
    /// Begründung, sobald etwas aufgebraucht ist.
    exceeded: Option<String>,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            operations: BASE_OPERATIONS,
            glyphs: MAX_GLYPHS_PER_SCAN,
            streams: HashMap::new(),
            resource_dicts: HashMap::new(),
            fonts: HashMap::new(),
            font_objects: HashMap::new(),
            declared_forms: HashSet::new(),
            cached_operations: 0,
            cached_font_entries: 0,
            seen_font_entries: 0,
            charged_fonts: HashSet::new(),
            effort: ScanEffort::default(),
            looked_at_type3: HashSet::new(),
            scanned_marked: HashSet::new(),
            mirror_pairs: MAX_MIRROR_FORM_PLACEMENTS,
            mirror_expansions: MAX_MIRROR_FORM_PLACEMENTS,
            mirror_shows: MAX_MIRROR_FORM_PLACEMENTS,
            mirror_forms_cut: false,
            mirror_shows_cut: false,
            exceeded: None,
        }
    }
}

impl Budget {
    /// Packt einen platzierbaren Strom aus, zerlegt ihn und lädt seine
    /// Schriften — **einmal je Objekt-Id**.
    ///
    /// Jede weitere Platzierung bekommt dasselbe Ergebnis zurück. Warum das
    /// zulässig ist und was deshalb *nicht* darin steht: [`PlacedStream`].
    fn stream(&mut self, doc: &Document, id: ObjectId, stream: &Stream) -> Rc<PlacedStream> {
        if let Some(entry) = self.streams.get(&id) {
            return match &entry.body {
                Some(body) => Rc::clone(body),
                // Über der Decke: auspacken wie vor der Änderung.
                None => Rc::new(self.load_stream(doc, Some(id), stream)),
            };
        }
        let body = Rc::new(self.load_stream(doc, Some(id), stream));
        let cost = stream_cost(&body);
        let keep = self.cached_operations.saturating_add(cost) <= MAX_CACHED_OPERATIONS;
        if keep {
            self.cached_operations += cost;
        }
        self.streams.insert(
            id,
            CachedStream {
                body: keep.then(|| Rc::clone(&body)),
                credited: false,
            },
        );
        body
    }

    /// Packt einen Strom aus, zerlegt ihn und lädt seine Schriften — ohne den
    /// Strom selbst zu merken.
    ///
    /// `id` ist `None` für einen Strom ohne eigene Objekt-Id; ein solcher ist
    /// nicht zu merken (und nicht neu zu schreiben), und auch seine Schriften
    /// bekommen dann keinen Schlüssel.
    fn load_stream(
        &mut self,
        doc: &Document,
        id: Option<ObjectId>,
        stream: &Stream,
    ) -> PlacedStream {
        self.effort.decoded_streams += 1;
        let (content, has_tokens) = match crate::filters::decoded_content(doc, stream) {
            Some(data) => (
                Some(crate::ops::decode_content_checked(&data)),
                has_tokens(&data),
            ),
            None => (None, false),
        };
        // `/Resources` wird mitsamt seiner Objekt-Id aufgelöst: teilen sich
        // mehrere Ströme dasselbe Verzeichnis, teilen sie sich auch dessen
        // Schriften — und seit [`Budget::resource_dicts`] auch das Verzeichnis
        // selbst statt je Strom einer eigenen Kopie.
        let (resource_id, resources, shared_resources) = match stream
            .dict
            .get(b"Resources")
            .ok()
            .and_then(|o| doc.dereference(o).ok())
        {
            Some((resource_id, object)) => match object.as_dict() {
                Ok(dict) => {
                    let (rc, shared) = self.resource_dict(resource_id, dict);
                    (resource_id, Some(rc), shared)
                }
                Err(_) => (None, None, false),
            },
            None => (None, None, false),
        };
        let font_key = resources.as_ref().and(
            resource_id
                .map(ResourceKey::Object)
                .or(id.map(ResourceKey::InStream)),
        );
        PlacedStream {
            content,
            has_tokens,
            resources,
            shared_resources,
            font_key,
        }
    }

    /// Das aufgelöste `/Resources` eines Stroms — **einmal je Objekt-Id**.
    ///
    /// Der zweite Rückgabewert sagt, ob das Ergebnis geteilt ist. `false`
    /// heißt: diese Kopie gehört dem Aufrufer allein und wiegt bei
    /// [`stream_cost`] mit — entweder weil das Verzeichnis gar keine eigene
    /// Objekt-Id hat (direkt im Stromdict eingebettet, dann gibt es nichts zu
    /// teilen), oder weil [`MAX_CACHED_OPERATIONS`] voll ist.
    ///
    /// Der zweite Fall ist der wichtige: ist die Decke voll, wird der Strom
    /// ohnehin nicht gemerkt, und die Kopie stirbt mit der Platzierung. So
    /// bleibt auch ein Verzeichnis, das für sich allein schon über der Decke
    /// liegt, harmlos — es wird je Platzierung neu kopiert (Laufzeit wie
    /// vorher), aber nie *n*-mal gleichzeitig gehalten.
    fn resource_dict(&mut self, id: Option<ObjectId>, dict: &Dictionary) -> (Rc<Dictionary>, bool) {
        let Some(id) = id else {
            return (Rc::new(dict.clone()), false);
        };
        if let Some(shared) = self.resource_dicts.get(&id) {
            return (Rc::clone(shared), true);
        }
        let cost = dictionary_objects(dict);
        if self.cached_operations.saturating_add(cost) > MAX_CACHED_OPERATIONS {
            return (Rc::new(dict.clone()), false);
        }
        self.cached_operations += cost;
        let shared = Rc::new(dict.clone());
        self.resource_dicts.insert(id, Rc::clone(&shared));
        (shared, true)
    }

    /// Die Schriften zum **eigenen** `/Resources` eines Stroms.
    ///
    /// `None` heißt: der Strom bringt keins mit; dann gelten unverändert die
    /// des Aufrufers. Das ist die eine Stelle, an der die Vererbung
    /// entschieden wird — und sie wird bei **jeder** Platzierung neu
    /// entschieden, nicht einmal beim Auspacken.
    fn fonts_of(&mut self, doc: &Document, placed: &PlacedStream) -> Option<Rc<FontMap>> {
        let resources: &Dictionary = placed.resources.as_deref()?;
        Some(match placed.font_key {
            Some(key) => self.font_map(doc, key, resources),
            // Weder das Verzeichnis noch der Strom hat eine Objekt-Id: es
            // gibt nichts, worunter sich das merken ließe.
            None => Rc::new(self.load_font_map(doc, Some(resources)).0),
        })
    }

    /// Lädt die Schriften eines `/Resources`-Verzeichnisses — einmal je
    /// Verzeichnis.
    ///
    /// Siehe [`ResourceKey`] für die Begründung des Schlüssels und
    /// [`MAX_CACHED_FONT_ENTRIES`] für die Decke. Über der Decke wird geladen
    /// wie bisher, nur eben nicht behalten.
    fn font_map(
        &mut self,
        doc: &Document,
        key: ResourceKey,
        resources: &Dictionary,
    ) -> Rc<FontMap> {
        if let Some(fonts) = self.fonts.get(&key) {
            return Rc::clone(fonts);
        }
        let (fonts, retained) = self.load_font_map(doc, Some(resources));
        let fonts = Rc::new(fonts);
        // Gemerkt wird nur, was den Tabellen **nichts** hinzufügt: jede Schrift
        // darin liegt bereits in [`Budget::font_objects`] und ist dort gezählt.
        // Sonst hielte dieses Verzeichnis Tabellen fest, die die Decke gerade
        // abgelehnt hat — der Zwischenspeicher wäre die Hintertür an seiner
        // eigenen Decke vorbei. Der Preis hier ist nur die Namensliste.
        let names = fonts.len();
        if retained && self.cached_font_entries.saturating_add(names) <= MAX_CACHED_FONT_ENTRIES {
            self.cached_font_entries += names;
            self.fonts.insert(key, Rc::clone(&fonts));
        }
        fonts
    }

    /// Übersetzt ein `/Resources`-Verzeichnis in Schriftmetriken und zählt den
    /// Vorgang mit — die eine Stelle, an der das im Interpreter geschieht.
    ///
    /// Der zweite Rückgabewert sagt, ob **jede** Schrift des Verzeichnisses
    /// gemerkt werden konnte; nur dann darf das Verzeichnis selbst gemerkt
    /// werden (siehe [`Budget::font_map`]).
    fn load_font_map(&mut self, doc: &Document, resources: Option<&Dictionary>) -> (FontMap, bool) {
        self.effort.loaded_font_maps += 1;
        let mut out = FontMap::new();
        let mut retained = true;
        let Some(font_dict) = crate::font::font_dictionary(doc, resources) else {
            return (out, retained);
        };
        for (name, object) in font_dict.iter() {
            // Über der harten Decke wird nicht weitergeladen: jede weitere
            // Schrift bliebe bis zum Ende des Stroms lebendig, und das ist
            // genau das, was die Decke verhindern soll.
            if self.exceeded.is_some() {
                break;
            }
            let Ok((id, resolved)) = doc.dereference(object) else {
                continue;
            };
            let Ok(dict) = resolved.as_dict() else {
                continue;
            };
            let (info, kept) = self.font_object(doc, id, dict);
            retained &= kept;
            out.insert(name.to_vec(), info);
        }
        (out, retained)
    }

    /// Eine geparste Schrift — **einmal je Schriftobjekt**.
    ///
    /// Das Ergebnis von [`font_from_dict`] hängt allein am Schriftobjekt und an
    /// dem, was daran hängt (`/ToUnicode`, `/W`, `/Widths`, eingebettete
    /// `cmap`). Weder Fundort noch Ressourcenname noch Grafikzustand gehen
    /// ein — dieselbe Objekt-Id ergibt also immer dieselbe [`FontInfo`], und
    /// sie mehrfach zu parsen ist reine Verschwendung. Gemessen: ein
    /// Verzeichnis, das dieselbe schwere Schrift unter 40 Namen führt, belegte
    /// vorher 279 MB und brauchte 2,38 s — der Zuwachs der Datei dafür betrug
    /// 10 Byte je Name.
    ///
    /// `false` im zweiten Rückgabewert heißt: diese Schrift ist **nicht**
    /// gemerkt (keine Objekt-Id, oder [`MAX_CACHED_FONT_ENTRIES`] ist voll).
    fn font_object(
        &mut self,
        doc: &Document,
        id: Option<ObjectId>,
        dict: &Dictionary,
    ) -> (Rc<FontInfo>, bool) {
        if let Some(id) = id {
            if let Some(info) = self.font_objects.get(&id) {
                return (Rc::clone(info), true);
            }
        }
        self.effort.parsed_fonts += 1;
        let info = Rc::new(font_from_dict(doc, dict));
        let cost = info.weight();
        // Die harte Decke: je Schriftobjekt einmal, über den ganzen Scan.
        // Sie steht neben der weichen — was hier gebucht wird, bleibt in der
        // [`FontMap`] des Verzeichnisses lebendig, gleich ob es der
        // Zwischenspeicher unten annimmt oder nicht.
        let erstmals = match id {
            Some(id) => self.charged_fonts.insert(id),
            // Ohne Objekt-Id gibt es nichts wiederzuerkennen; im Zweifel
            // zählen, das ist die strengere Richtung.
            None => true,
        };
        if erstmals {
            self.seen_font_entries = self.seen_font_entries.saturating_add(cost);
            if self.seen_font_entries > MAX_FONT_ENTRIES_PER_SCAN {
                self.exceeded = Some(format!(
                    "Eine Seite dieses Dokuments lädt mehr als \
                     {MAX_FONT_ENTRIES_PER_SCAN} Schrift-Tabelleneinträge. Alle Schriften \
                     eines Ressourcenverzeichnisses sind gleichzeitig im Speicher; \
                     gemessen kostet eine Schrift mit voller `/ToUnicode`-Zuordnung rund \
                     4 MB, hundert davon aus einer Datei von 45 kB also rund 400 MB. Eine \
                     Seite trägt normalerweise eine Handvoll Schriften; diese Menge \
                     entsteht nicht durch ein echtes Dokument. Die Datei wird abgelehnt, \
                     statt den Text mit halb geladenen Schriften zu durchsuchen."
                ));
            }
        }
        let room = self.cached_font_entries.saturating_add(cost) <= MAX_CACHED_FONT_ENTRIES;
        match id {
            Some(id) if room => {
                self.cached_font_entries += cost;
                self.font_objects.insert(id, Rc::clone(&info));
                (info, true)
            }
            _ => (info, false),
        }
    }

    /// `true` beim **ersten** Angebot der `/XObject`-Liste dieses
    /// Verzeichnisses — siehe [`declare_forms`].
    fn first_declaration(&mut self, key: DeclarationKey) -> bool {
        self.declared_forms.insert(key)
    }

    /// Schreibt den Inhalt eines Stroms gut — einmal je Strom.
    ///
    /// `id` ist `None` für den Seitenstrom selbst; der wird ohnehin nur einmal
    /// dekodiert. Ein Strom mit Id muss vorher durch [`Budget::stream`]
    /// gegangen sein; ist er das nicht, wird im Zweifel **nicht**
    /// gutgeschrieben — die strengere Richtung.
    fn credit(&mut self, id: Option<ObjectId>, operations: usize) {
        if let Some(id) = id {
            match self.streams.get_mut(&id) {
                Some(entry) if !entry.credited => entry.credited = true,
                _ => return,
            }
        }
        self.operations = self
            .operations
            .saturating_add(operations.saturating_mul(MAX_AMPLIFICATION));
    }

    /// `true` beim **ersten** Blick auf diese Type3-Schrift in diesem Strom.
    ///
    /// Siehe [`Budget::looked_at_type3`]: die Untersuchung selbst kostet, sie
    /// darf nur nicht bei jedem einzelnen `Tj` neu anfallen.
    fn first_look_at_type3(&mut self, stream: StreamKey, font: &[u8]) -> bool {
        self.looked_at_type3.insert((stream, font.to_vec()))
    }

    /// `true` beim **ersten** Spiegel-Durchlauf über diesen Strom in dieser
    /// Ressourcenumgebung — siehe [`Budget::scanned_marked`].
    fn first_marked_scan(&mut self, stream: StreamKey, owner: Option<ObjectId>) -> bool {
        self.scanned_marked.insert((stream, owner))
    }

    /// Bucht bis zu `want` Spiegel-Formular-Paare beim **Aufbau** der Liste
    /// und liefert, wie viele davon bewilligt sind.
    ///
    /// `B` verschachtelte `BDC`-Klammern über `D` Platzierungen ergeben
    /// `B × D` Paare, und die entstehen, bevor die Schließung überhaupt
    /// gefragt wird. Vorher hing an dieser Stelle keine Schranke — gemessen
    /// 2 306 MB und 41,7 s aus einer Datei von 276 kB mit neun Objekten.
    fn mirror_pairs(&mut self, want: usize) -> usize {
        let granted = want.min(self.mirror_pairs);
        self.mirror_pairs -= granted;
        if granted < want {
            self.mirror_forms_cut = true;
        }
        granted
    }

    /// Bucht bis zu `want` Spiegel-Text-Zuordnungen beim Aufbau von
    /// [`MarkedTextRecord::shows`] und liefert, wie viele bewilligt sind.
    ///
    /// Eigener Zähler und eigene Flagge: es ist eine eigene Liste mit eigener
    /// Folge. Fällt hier etwas weg, fehlen einem Spiegel **Glyphen** — die
    /// Schwärzung sieht dann nicht mehr, ob er berührt ist, und der Vergleich
    /// in `crate::extract` vergleicht gegen zu wenig. Beides muss dastehen,
    /// und beides steht in einer eigenen Warnung (siehe
    /// [`ScanResult::close_forms`]).
    fn mirror_shows(&mut self, want: usize) -> usize {
        let granted = want.min(self.mirror_shows);
        self.mirror_shows -= granted;
        if granted < want {
            self.mirror_shows_cut = true;
        }
        granted
    }

    /// Bucht **eine Aufklappung** in [`ScanResult::close_forms`]. `false`
    /// heißt: die Decke ist erreicht, diese Kante entfällt — und genau dann
    /// steht auch die Flagge.
    fn mirror_expansion(&mut self) -> bool {
        match self.mirror_expansions.checked_sub(1) {
            Some(rest) => {
                self.mirror_expansions = rest;
                true
            }
            None => {
                self.mirror_forms_cut = true;
                false
            }
        }
    }

    /// Ist an einer der Formular-Decken wirklich etwas weggefallen?
    fn mirror_forms_cut(&self) -> bool {
        self.mirror_forms_cut
    }

    /// Ist an der Text-Decke wirklich etwas weggefallen?
    fn mirror_shows_cut(&self) -> bool {
        self.mirror_shows_cut
    }

    /// Verbucht eine Operation. `false` heißt: sofort aussteigen.
    fn operation(&mut self) -> bool {
        if self.exceeded.is_some() {
            return false;
        }
        match self.operations.checked_sub(1) {
            Some(rest) => {
                self.operations = rest;
                true
            }
            None => {
                self.exceeded = Some(format!(
                    "Der Seiteninhalt wird um mehr als das {MAX_AMPLIFICATION}-fache \
                     vervielfacht: es sind mehr Zeichenoperationen auszuwerten, als der \
                     Inhalt der Datei hergibt. So etwas entsteht nicht durch einen langen \
                     Text, sondern dadurch, dass wenige Form-XObjects einander vielfach \
                     zeichnen — aus wenigen Kilobyte werden Millionen Durchläufe. Die \
                     Datei wird abgelehnt."
                ));
                false
            }
        }
    }

    /// Verbucht eine Glyphe. `false` heißt: sofort aussteigen.
    fn glyph(&mut self) -> bool {
        if self.exceeded.is_some() {
            return false;
        }
        match self.glyphs.checked_sub(1) {
            Some(rest) => {
                self.glyphs = rest;
                true
            }
            None => {
                self.exceeded = Some(format!(
                    "Eine Seite dieses Dokuments setzt mehr als {MAX_GLYPHS_PER_SCAN} \
                     Zeichen. Eine dichte Textseite trägt einige tausend; diese Menge \
                     entsteht nur, wenn derselbe Text vielfach gezeichnet wird. Beim \
                     Vermessen der Zeichen würde daraus ein zweistelliges Gigabyte \
                     Arbeitsspeicher. Die Datei wird abgelehnt."
                ));
                false
            }
        }
    }

    /// Der Befund, falls das Konto gerissen wurde.
    fn result(&self) -> Result<()> {
        match &self.exceeded {
            Some(message) => Err(RedactError::Pdf(message.clone())),
            None => Ok(()),
        }
    }
}

/// Ab wie vielen **unlesbaren** Zeichen ein Font ohne `/ToUnicode` gemeldet
/// wird — unabhängig davon, wie klein ihr Anteil ist.
///
/// Vorher zählte diese Schwelle die *insgesamt* gesetzten Zeichen: unter vier
/// blieb es still. Eine Seite mit genau drei unlesbaren Glyphen warnte deshalb
/// nicht — ausgerechnet der Fall, der wehtut. Drei unlesbare Zeichen können
/// genau die Ziffern sein, auf die es ankommt; der Rest einer Kontonummer ist
/// nicht weniger schutzbedürftig, weil er kurz ist. Gezählt wird jetzt das
/// Unlesbare selbst.
///
/// Bei **einem** einzelnen Zeichen bleibt es still, sofern der Font sonst
/// lesbar ist (siehe [`UNREADABLE_RATIO`]): ein Aufzählungspunkt oder ein
/// Logo-Dingbat ist Gestaltung, kein Text. Dieser Fall ist häufig genug, dass
/// eine Warnung darüber die echten Befunde zudecken würde.
const UNREADABLE_MIN_GLYPHS: usize = 2;

/// Ab welchem Anteil schon ein **einzelnes** unlesbares Zeichen gemeldet wird.
///
/// Vorher war der Anteil das alleinige Maß, mit 0,3. Das ist die falsche
/// Größe: Der Anteil misst, wie typisch das Problem im Font ist, nicht wie
/// viel Text dadurch ungeprüft bleibt — 29 % eines Fonts mit 1000 Zeichen sind
/// 290 unlesbare Zeichen, und die blieben unerwähnt. Über die Menge
/// entscheidet jetzt [`UNREADABLE_MIN_GLYPHS`].
///
/// Der Anteil hat nur noch eine Aufgabe: Ein Font, der überhaupt kaum Text
/// setzt (bis zu 20 Zeichen), fällt schon mit einem einzigen unlesbaren
/// Zeichen auf — dort trägt dieses eine Zeichen Gewicht, während dieselbe
/// Glyphe in einem seitenfüllenden Font Beiwerk ist.
const UNREADABLE_RATIO: f64 = 0.05;

/// Aus welchem Stream ein Datensatz stammt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StreamKey {
    /// Der (ggf. zusammengesetzte) Content-Stream der Seite.
    Page,
    /// Ein Form-XObject.
    Form(ObjectId),
}

/// Lage einer Glyphe auf ihrer Grundlinie — bereits im User-Space, also nach
/// `Tm`, `Tz`, `Ts` und CTM.
///
/// Die Extraktion darf keine achsenparallele Leserichtung unterstellen: bei
/// gedrehtem Text läuft die Grundlinie schräg oder senkrecht, und eine
/// Gruppierung nach `origin.y` zerlegt jede Zeile in Einzelzeichen. Ebenso
/// wenig darf sie den Zeichenabstand aus den Kästen ableiten — eine gesetzte
/// Laufweite (`Tc`) steckt bereits im [`GlyphItem::displacement`] und wäre
/// sonst nicht von einer echten Wortlücke zu unterscheiden.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Baseline {
    /// Einheitsvektor der Schreibrichtung im User-Space.
    pub direction: Point,
    /// Vorschub bis zur nächsten Glyphe, entlang [`Baseline::direction`].
    /// Enthält Glyphenbreite, `Tc`, `Tw` und `Tz`.
    pub advance: f64,
    /// Höhe des Glyphenkastens senkrecht zur Grundlinie.
    pub height: f64,
    /// Breite des Leerzeichens dieses Fonts, entlang der Schreibrichtung.
    /// `0.0`, wenn der Font keine brauchbare Auskunft gibt.
    pub space_width: f64,
}

impl Default for Baseline {
    fn default() -> Self {
        Self {
            direction: Point::new(1.0, 0.0),
            advance: 0.0,
            height: 0.0,
            space_width: 0.0,
        }
    }
}

/// Ein einzelnes gesetztes Zeichen.
#[derive(Debug, Clone)]
pub struct GlyphItem {
    /// Originalbytes des Zeichencodes (für das Neuschreiben des Streams).
    pub bytes: Vec<u8>,
    /// Dekodierter Text (kann bei Ligaturen mehrere Zeichen umfassen).
    pub text: String,
    /// Bounding-Box im User-Space.
    pub rect: Rect,
    /// Ursprung (Grundlinie, linke Kante) im User-Space.
    pub origin: Point,
    /// Vorschub im Textraum vor Anwendung von `Tm`/CTM.
    pub displacement: f64,
    /// Grundlinien-Geometrie im User-Space.
    pub baseline: Baseline,
}

/// Bestandteil einer Text-Ausgabe-Operation.
#[derive(Debug, Clone)]
pub enum ShowItem {
    Glyph(GlyphItem),
    /// Zahlenwert aus einem `TJ`-Array (Kerning).
    Adjust(f64),
}

/// Eine Text-Ausgabe-Operation mit allen berechneten Glyphen.
#[derive(Debug, Clone)]
pub struct ShowRecord {
    pub stream: StreamKey,
    /// Index der Operation im dekodierten Stream.
    pub op_index: usize,
    pub operator: String,
    /// Originaloperanden der Operation (für `'` und `"` gebraucht).
    pub operands: Vec<Object>,
    pub font_size: f64,
    pub h_scale: f64,
    pub items: Vec<ShowItem>,
}

impl ShowRecord {
    pub fn glyphs(&self) -> impl Iterator<Item = &GlyphItem> {
        self.items.iter().filter_map(|i| match i {
            ShowItem::Glyph(g) => Some(g),
            ShowItem::Adjust(_) => None,
        })
    }
}

/// Schlüssel einer Eigenschaftsliste, die den Text darunter **spiegeln** —
/// drei Schlüssel, zwei Rollen.
///
/// * `/ActualText` — der **Ersatz** für die Glyphen des Abschnitts
///   (PDF 32000-1, 14.9.4): ein Betrachter kopiert ihn *statt* der Glyphen.
///   Er muss ihnen gleichen; Word, InDesign und jeder PDF/UA-Erzeuger
///   schreiben ihn routinemäßig, etwa für Ligaturen und Sonderzeichen.
/// * `/Alt` — die **Beschreibung** für Hilfsmittel (14.9.3); bei `/Figure`
///   steht dort, was das Bild zeigt. Sie darf von den Glyphen abweichen —
///   ein Bild hat keine.
/// * `/E` — die **ausgeschriebene Form** einer Abkürzung (14.9.5): „z. B.“
///   trägt `/E (zum Beispiel)`. Sie weicht von den Glyphen ab, das ist ihr
///   Zweck.
///
/// Alle drei werden **gelesen** (als eigener Textlauf über dem Kasten der
/// Glyphen, siehe `crate::extract`) und mit den Glyphen **geleert** (siehe
/// `crate::redact::mirrors_to_clear`): `pdftotext` bevorzugt in der
/// Voreinstellung sogar den Spiegel, und verschwinden die Glyphen, muss er
/// mit. Als **Widerspruch gemeldet** wird nur der Ersatz — ein `/Alt` oder
/// `/E`, der etwas anderes sagt als die Glyphen, ist die Normalform, kein
/// Befund.
pub const MIRROR_KEYS: [&[u8]; 3] = [b"ActualText", b"Alt", b"E"];

/// Ein `BDC`/`DP`, dessen Eigenschaftsliste einen Textspiegel trägt.
///
/// `shows` nennt die Textoperationen, die dieser Spiegel wiedergibt — die
/// Marked-Content-Klammer kennt ihre Glyphen. Damit kann die Schwärzung genau
/// die Frage beantworten, auf die es ankommt: *ist von diesem Abschnitt etwas
/// entfernt worden?*
#[derive(Debug, Clone)]
pub struct MarkedTextRecord {
    pub stream: StreamKey,
    /// Index der `BDC`/`DP`-Operation im dekodierten Strom.
    pub op_index: usize,
    /// Die aufgelöste Eigenschaftsliste — gleich, ob sie inline im Strom stand
    /// oder über `/Resources /Properties` erreichbar war.
    pub properties: Dictionary,
    /// Objekt-Id der Eigenschaftsliste, falls sie ein **eigenes** Objekt ist
    /// (`/Properties /MC0 12 0 R`). Sonst `None`: dann steht die Liste inline
    /// im Strom oder **direkt** in einem `/Properties`-Dictionary.
    pub property_id: Option<ObjectId>,
    /// Der Ressourcenname der Eigenschaftsliste (`/Span /MC0 BDC` → `MC0`),
    /// falls die Operation sie über `/Resources /Properties` benannt hat.
    ///
    /// `None` heißt: die Liste stand als Dictionary **im Strom** und existiert
    /// nirgendwo sonst; sie wird beim Neuschreiben des Stroms ersetzt und ist
    /// damit erledigt.
    ///
    /// Steht hier ein Name und `property_id` ist `None`, dann steht der
    /// Klartext des Spiegels **im Ressourcenverzeichnis** — die Operation neu
    /// zu schreiben genügt dann nicht, das Verzeichnis muss mit
    /// ([`property_list_homes`]). Bis Fix-Runde 6 blieb er dort stehen;
    /// `leaks` fand ihn, ohne Warnung, mit Rückgabewert 0 (Register #34,
    /// Befund Q3-5).
    pub property_name: Option<Vec<u8>>,
    /// Das Objekt, dessen `/Resources` beim Lesen dieses Abschnitts **galten**:
    /// die Seite, oder das Form-XObject bzw. der Musterstrom, der ein eigenes
    /// `/Resources` mitbringt.
    ///
    /// Nicht dasselbe wie `stream`. Ein Form-XObject ohne eigenes
    /// `/Resources` erbt die des Aufrufers (PDF 32000-1, 8.10.1); dann steht
    /// hier die **platzierende Seite**, in deren `/Properties` sich
    /// `property_name` aufgelöst hat. Wer stattdessen ab dem Formular suchte,
    /// fand nichts und ließ den Klartext im Verzeichnis der Seite stehen —
    /// ohne Warnung, mit Rückgabewert 0 (Befund R1-1).
    ///
    /// `None` nur aus [`interpret`]: dort gibt es keinen Seitenkontext, und
    /// keine seiner Senken liest Spiegel.
    pub property_owner: Option<ObjectId>,
    /// Indizes der Textoperationen im Geltungsbereich (siehe
    /// [`scan_marked_text`]).
    pub shows: Vec<usize>,
    /// Die Form-XObjects im Geltungsbereich: je `Do` der **Pfad** dorthin
    /// und die Objekt-Id des Formulars. Der Pfad ist die Folge der
    /// `Do`-Indizes von diesem Strom bis zum Formular — `[7]` für ein `Do` an
    /// Index 7 dieses Stroms, `[7, 2]` für das Formular, das jenes an seinem
    /// Index 2 zeichnet. Der Spiegel gilt auch für deren Glyphen — ein
    /// `/Span <</ActualText …>> BDC /Fm0 Do EMC` ist die Form, in der ein
    /// Erzeuger einen Textbaustein beschriftet, und ohne diesen Eintrag
    /// stünden „0 Glyphen“ unter einem Spiegel, der welche hat.
    ///
    /// Nach [`scan_page`] **transitiv geschlossen**: ein Formular, das das
    /// Formular hier zeichnet, steht mit dem um seinen `Do`-Index verlängerten
    /// Pfad ebenfalls darin. Der Pfad ordnet die Glyphen aller Formulare in
    /// Stromreihenfolge (`crate::extract`); wer nur wissen will, *welche*
    /// Formulare betroffen sind, liest die Id. Direkt aus
    /// [`scan_marked_text`] enthält die Liste nur die eigene Ebene.
    pub forms: Vec<(Vec<usize>, ObjectId)>,
    /// Der Bereich der eingeschlossenen Operationen im dekodierten Strom —
    /// `start..end`, dieselbe Spanne, aus der [`MarkedTextRecord::shows`] und
    /// [`MarkedTextRecord::forms`] gesiebt sind.
    ///
    /// Bewusst die **Spanne** und nicht eine Liste der Bildplatzierungen
    /// darin: ein Bild ist kein Text, es hat keine Glyphen, die einem Spiegel
    /// zuzuordnen wären — gefragt wird nur, *ob* eine geschwärzte
    /// Bildplatzierung in diesem Abschnitt liegt. Als Liste wäre das wieder
    /// das Produkt „Klammern × Platzierungen“ mit eigener Decke
    /// (`MAX_MIRROR_FORM_PLACEMENTS`); als Spanne kostet es zwei `usize` je
    /// Abschnitt. Wer die Platzierungen braucht, liest [`ScanResult::images`]
    /// und fragt hier nach.
    pub range: std::ops::Range<usize>,
}

/// Ein platziertes Bild, so weit die Schwärzung es braucht: **wo** im Strom
/// es steht und **welche Fläche** es einnimmt.
///
/// Kein Bildinhalt, keine Objekt-Id-Pflicht, kein Dekodieren — das macht
/// [`crate::image`]. Hier zählt nur die Zuordnung „diese Platzierung liegt in
/// jenem Marked-Content-Abschnitt und unter jener Schwärzung“.
#[derive(Debug, Clone, PartialEq)]
pub struct ImagePlacement {
    /// Der Strom, in dem die Platzierung steht (Seite oder Form-XObject).
    pub stream: StreamKey,
    /// Index der `Do`- bzw. `BI`-Operation im dekodierten Strom.
    pub op_index: usize,
    /// Objekt-Id des Bild-XObjects; `None` bei einem Inline-Bild, das kein
    /// eigenes Objekt hat.
    pub id: Option<ObjectId>,
    /// Die Hülle der Zielfläche im User-Space — das Einheitsquadrat des
    /// Bildes durch die CTM (PDF 32000-1, 8.9.5.2).
    ///
    /// **Die Hülle ist nicht die Fläche.** Bei einer gedrehten CTM ist sie
    /// größer als das Bild, und in ihren Ecken liegt kein Bildpunkt. Wer
    /// fragen will, ob eine Schwärzung das Bild trifft, nimmt deshalb
    /// [`ImagePlacement::covers`] und nicht dieses Rechteck; hier steht nur
    /// die billige Vorauswahl.
    pub bounds: Rect,
    /// Die vier Ecken der Zielfläche im User-Space, im Umlauf
    /// `(0,0) (1,0) (1,1) (0,1)` des Einheitsquadrats.
    pub quad: [Point; 4],
}

impl ImagePlacement {
    /// Trifft `rect` die **Fläche** dieser Platzierung?
    ///
    /// Nicht ihre Hülle: bei einem um 45° gedrehten Bild ist die Hülle doppelt
    /// so groß wie das Bild, und in ihren vier Ecken steht kein Bildpunkt,
    /// sondern gewöhnlich Text. Eine Schwärzung dort nimmt dem Bild keinen
    /// Bildpunkt — [`crate::image`] fasst es nicht an, `redacted_images`
    /// bleibt 0, und es gibt keine Warnung. Wer an der Hülle entschied, nahm
    /// dem unversehrten Bild trotzdem seinen Ersatztext und dem Abschnitt
    /// darüber seinen Spiegel: Barrierefreiheit weg, kein Bildpunkt gewonnen,
    /// kein Wort darüber.
    ///
    /// Gefragt wird deshalb das **konvexe Viereck** gegen das achsenparallele
    /// Rechteck, über die trennenden Achsen: die beiden Achsen des Rechtecks
    /// (das ist genau die Hüllenfrage) und die vier Kantennormalen des
    /// Vierecks.
    ///
    /// **Was diese Frage nicht ist: die Wahrheit über die Bildpunkte.** Die
    /// kennt nur [`crate::image`] und gibt sie dort heraus
    /// ([`crate::image::ImageOutcome::page_image_hits`]); über den Ersatztext
    /// eines Bildes wird deshalb dort entschieden und nicht hier.
    ///
    /// Hier stand einmal die Zusicherung, wo `filled > 0` gelte, liege eine
    /// Zellecke im Rechteck und damit auch das Viereck darin — ein Bild, das
    /// Bildpunkte verliert, werde also nie übersehen. **Sie war falsch und ist
    /// gestrichen.** Diese Frage vergleicht streng (Berührung zählt nicht),
    /// [`crate::image`] füllt mit dem Rand eingeschlossen; wo eine Zellecke
    /// genau auf dem Rand des Rechtecks liegt, fiel der Bildpunkt und diese
    /// Frage verneinte (`zj_b_flaeche_gegen_bildpunkt`). In der anderen
    /// Richtung blieb ebenfalls ein Rest: ein Rechteck, das die Fläche um
    /// weniger als eine Pixelzelle überlappt, gilt hier als Treffer.
    ///
    /// Die Frage bleibt, weil sie billig und ohne Dekodieren zu haben ist —
    /// als Auskunft über eine Platzierung, nicht als Ersatz für die Wahrheit.
    ///
    /// Ein entartetes Viereck (die CTM ist nicht umkehrbar, oder eine
    /// Koordinate ist unbrauchbar) trifft nichts: `crate::image` kann dort
    /// nicht einmal rückwärts rechnen und füllt keinen Bildpunkt.
    pub fn covers(&self, rect: &Rect) -> bool {
        // Die beiden Achsen des Rechtecks — und zugleich der billige
        // Vorfilter, der die meisten Platzierungen hier schon verlässt.
        if !self.bounds.intersects(rect) {
            return false;
        }
        let corners = [
            rect.ll,
            Point::new(rect.ur.x, rect.ll.y),
            rect.ur,
            Point::new(rect.ll.x, rect.ur.y),
        ];
        for index in 0..4 {
            let from = self.quad[index];
            let to = self.quad[(index + 1) % 4];
            // Normale der Kante; bei einer entarteten Kante ist sie (0,0).
            let axis = Point::new(from.y - to.y, to.x - from.x);
            if !axis.x.is_finite() || !axis.y.is_finite() {
                return false;
            }
            if axis.x == 0.0 && axis.y == 0.0 {
                return false;
            }
            let (qmin, qmax) = span(&self.quad, axis);
            let (rmin, rmax) = span(&corners, axis);
            // Wie [`Rect::intersects`]: Berührung zählt nicht. Ein NaN in der
            // Projektion lässt beide Vergleiche falsch werden — deshalb steht
            // die Entscheidung positiv formuliert.
            if !(qmax > rmin && rmax > qmin) {
                return false;
            }
        }
        true
    }
}

/// Projektion von vier Punkten auf eine Achse — kleinster und größter Wert.
///
/// Auch [`crate::image`] fragt so: dort wird die Fläche **einer Pixelzelle**
/// gegen ein Schwärzungsrechteck geprüft. Dieselbe Frage, dieselbe Antwort —
/// zwei Fassungen davon wären zwei Gelegenheiten, verschieden zu antworten.
pub(crate) fn span(points: &[Point; 4], axis: Point) -> (f64, f64) {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for point in points {
        let value = point.x * axis.x + point.y * axis.y;
        if value < min {
            min = value;
        }
        if value > max {
            max = value;
        }
    }
    (min, max)
}

/// Ergebnis eines Seiten-Scans.
#[derive(Debug, Default)]
pub struct ScanResult {
    pub shows: Vec<ShowRecord>,
    /// Marked-Content-Abschnitte mit Textspiegel.
    pub marked: Vec<MarkedTextRecord>,
    /// **Jede** Bildplatzierung dieser Seite — je Platzierung ihr Fundort im
    /// Strom und ihre Fläche im User-Space.
    ///
    /// Gebraucht werden zwei Fragen. Liegt in diesem Marked-Content-Abschnitt
    /// ein Bild, dessen Pixel eine Schwärzung überschreibt? Dann ist der
    /// Spiegel darüber (`/Alt` bei `/Figure`: „Kontoauszug, IBAN …“) genauso
    /// falsch wie ein Spiegel über verschwundenen Glyphen (Register #20). Und:
    /// welchem Bild-XObject gehört die Platzierung? Daran hängt der Ersatztext
    /// am Bilddictionary selbst.
    ///
    /// **Ohne Filter, und warum der frühere falsch war.** Die Liste wurde
    /// einmal nur „in Strömen, über denen ein Textspiegel steht“ gefüllt. Das
    /// verfehlte beide Fragen: der Spiegel steht regelmäßig in einem anderen
    /// Strom als das Bild (`… BDC /Fm0 Do EMC` im Seitenstrom, `/Im0 Do` in
    /// `Fm0`), und die zweite Frage stellt sich auch in einer Datei ganz ohne
    /// Marked Content. Gefüllt wird beim Durchlauf, also **je Platzierung**:
    /// ein zwanzigmal gezeichnetes Formular bringt seine Bilder zwanzigmal
    /// mit, jedes Mal mit seiner eigenen CTM. Genau so muss es sein — ein Bild
    /// kann an einer Stelle geschwärzt werden und an einer anderen nicht.
    ///
    /// **Keine eigene Decke, und warum keine nötig ist.** Die Liste wächst
    /// *linear* in den Platzierungen — eine je `Do`/`BI` —, und jede davon ist
    /// eine Operation, die aus demselben Aufwandskonto zahlt wie jede andere
    /// (`Budget::operation`). Das unterscheidet sie von der Zuordnung
    /// Spiegel↔Formular, die als **Produkt** „Klammern × Platzierungen“
    /// entstand und deshalb `MAX_MIRROR_FORM_PLACEMENTS` braucht. Gemessen an
    /// 10 000/50 000/100 000 Bildplatzierungen unter einem Spiegel: dieselbe
    /// Wanduhr wie ohne den Spiegel
    /// (`zh_b_bildspiegel::mess_viele_bildplatzierungen`).
    pub images: Vec<ImagePlacement>,
    /// Wie oft ein Form-XObject auf dieser Seite gezeichnet wurde.
    pub form_placements: BTreeMap<ObjectId, usize>,
    /// Form-XObjects, die in einem der gelesenen Ressourcenverzeichnisse
    /// **stehen** — samt ihrem Ressourcennamen. Wer hier steht und nicht in
    /// [`ScanResult::form_placements`], wurde nie gezeichnet und deshalb auch
    /// nie gelesen.
    pub declared_forms: BTreeMap<ObjectId, Vec<u8>>,
    /// Befunde, die den Nutzer erreichen müssen — allen voran Fonts, deren
    /// Text sich nicht dekodieren lässt. Aus solchem Text kann die Analyse
    /// nichts erkennen; ohne Warnung hielte man die Datei für sauber.
    pub warnings: Vec<String>,
    /// Was der Scan an Vorarbeit gekostet hat — siehe [`ScanEffort`].
    ///
    /// Wird von [`scan_page`] gefüllt; über [`interpret`] bleibt sie leer,
    /// weil dort der Aufrufer die Senke stellt.
    pub effort: ScanEffort,
    /// Welche Spiegel schon in `marked` stehen. Als Menge geführt, nicht durch
    /// Durchsuchen der Liste: eine getaggte Seite bringt leicht Tausende
    /// Abschnitte mit, und ein mehrfach platziertes Formular liefert sie
    /// mehrfach.
    /// Schlüssel: Strom, Operation **und Herkunft der Eigenschaftsliste**
    /// (Eigentümer der Ressourcen, Objekt-Id der Liste). Bis zur
    /// Spur-A-Runde 1 fehlte die Herkunft (Register #65).
    seen_marked: HashSet<(StreamKey, usize, Option<ObjectId>, Option<ObjectId>)>,
    /// Dasselbe für [`ScanResult::warnings`].
    ///
    /// Die Entdopplung war schon immer zugesagt; sie lief nur über
    /// `warnings.contains(…)`, also über die ganze bisherige Liste je neuer
    /// Warnung. Gemessen an derselben Datei, beide Fassungen abwechselnd:
    ///
    /// | Warnungen | mit Liste | mit Menge | Anteil |
    /// |----------:|----------:|----------:|-------:|
    /// |     2 000 |   0,028 s |   0,025 s |   11 % |
    /// |     8 000 |   0,157 s |   0,108 s |   31 % |
    /// |    32 000 |   3,592 s |   0,444 s |   88 % |
    ///
    /// Die rechte Spalte ist linear, die linke vervierfacht sich. Am
    /// Verhalten ändert sich nichts: dieselben Warnungen, dieselbe
    /// Reihenfolge, jede genau einmal.
    seen_warnings: HashSet<String>,
    /// Formular → die Formulare, die es selbst zeichnet, je mit dem Index
    /// des `Do` in seinem Strom, in `Do`-Reihenfolge. Gefüllt über
    /// [`ContentSink::form_within`], verbraucht von [`ScanResult::close_forms`].
    ///
    /// Eine **Menge**, weil ein mehrfach platziertes Formular mehrfach
    /// durchlaufen wird und seine Kanten dabei jedes Mal meldet: der Strom
    /// eines Formulars zeichnet an einem Index aber genau einmal, und die
    /// Schließung zählt Platzierungen. Stünde `(Index, Formular)` hier
    /// zweimal, zählten die Glyphen dahinter doppelt. Nach `Index` sortiert,
    /// also in Stromreihenfolge.
    nested_forms: BTreeMap<ObjectId, BTreeSet<(usize, ObjectId)>>,
}

impl ScanResult {
    /// Schließt [`MarkedTextRecord::forms`] transitiv über
    /// [`ScanResult::nested_forms`]: zeichnet ein Formular im Geltungsbereich
    /// eines Spiegels seinerseits Formulare, gehören deren Glyphen zu
    /// demselben Spiegel. Jede **Platzierung** steht danach mit dem Pfad der
    /// `Do`-Indizes darin, über den sie erreicht wurde.
    ///
    /// **Platzierung, nicht Formular.** Steht dasselbe Formular zweimal unter
    /// einem Spiegel (`… BDC /Fm0 Do /Fm0 Do EMC`, oder ein Formular, das ein
    /// inneres zweimal zeichnet), zeigt der Betrachter seine Glyphen zweimal,
    /// und der Spiegel schreibt sie zweimal aus. Eine Menge über Objekt-Ids
    /// warf hier bis Fix-Runde 5 die zweite Platzierung weg; die Glyphen
    /// zählten halb, und ein deckungsgleicher Spiegel („AlphaAlpha“ über
    /// zweimal „Alpha“) galt als Widerspruch — eine Deckungslücke und damit
    /// Rückgabewert 3 an gewöhnlichem Material. Perfide daran war, dass die
    /// Schließung nur läuft, wenn *irgendwo* im Dokument ein Formular ein
    /// Formular zeichnet: dieselbe Seite warnte je nach unbeteiligtem Beiwerk
    /// oder nicht.
    ///
    /// **Was die Menge stattdessen leistet.** Ein Zyklus (ein Formular, das
    /// sich selbst zeichnet) darf nicht in eine Endlosschleife laufen. Dagegen
    /// steht hier die **Kette der Vorfahren** dieses Pfades, nicht der ganze
    /// Datensatz: sie beendet den Zyklus genauso, lässt aber dasselbe Formular
    /// an zwei verschiedenen Stellen zu. Dazu kommt die Tiefe
    /// [`MAX_FORM_DEPTH`] — tiefer hat der Interpreter selbst nicht gelesen,
    /// und was er nicht gelesen hat, hat hier keine Glyphen beizutragen — und
    /// die Decke [`MAX_MIRROR_FORM_PLACEMENTS`] gegen die Fächerung.
    fn close_forms(&mut self, budget: &mut Budget) {
        // Auch ohne ein verschachteltes Formular kann beim **Aufbau** der
        // Listen schon etwas an der Decke weggefallen sein; die Warnung unten
        // gilt deshalb beiden Stellen — sie zählen dieselbe Größe.
        if !self.nested_forms.is_empty() {
            self.expand_forms(budget);
        }
        if budget.mirror_forms_cut() {
            self.warn(format!(
                "Unter den Textspiegeln dieser Seite stehen mehr als \
                 {MAX_MIRROR_FORM_PLACEMENTS} Zuordnungen zwischen einem Spiegel und \
                 einer Formularplatzierung; ab dort wurden die Glyphen den Spiegeln \
                 nicht mehr zugeordnet. Der Vergleich zwischen Spiegel und Glyphen ist \
                 für die letzten Abschnitte deshalb unvollständig."
            ));
        }
        if budget.mirror_shows_cut() {
            self.warn(format!(
                "Unter den Textspiegeln dieser Seite stehen mehr als \
                 {MAX_MIRROR_FORM_PLACEMENTS} Zuordnungen zwischen einem Spiegel und \
                 einer Textoperation; ab dort wurden die Textoperationen den Spiegeln \
                 nicht mehr zugeordnet. Für die letzten Abschnitte ist deshalb weder \
                 der Vergleich zwischen Spiegel und Glyphen vollständig noch die \
                 Frage entscheidbar, ob eine Schwärzung sie berührt — ein Spiegel \
                 darüber kann stehen bleiben."
            ));
        }
    }

    /// Der Tiefensuchlauf von [`ScanResult::close_forms`].
    fn expand_forms(&mut self, budget: &mut Budget) {
        for record in &mut self.marked {
            let mut closed: Vec<(Vec<usize>, ObjectId)> = Vec::new();
            for (path, id) in std::mem::take(&mut record.forms) {
                // Tiefensuche in `Do`-Reihenfolge. Ein Eintrag im Stapel ist
                // der ganze Weg als (Do-Index, Formular): daraus kommen der
                // Pfad, das aktuelle Formular und die Vorfahren, die den
                // Zyklus beenden.
                let mut stack: Vec<Vec<(usize, ObjectId)>> = Vec::new();
                // `scan_marked_text` liefert einen Pfad der Länge eins (das
                // `Do` in diesem Strom); längere kämen nur aus einer schon
                // geschlossenen Liste, und deren Vorfahren sind hier nicht
                // mehr bekannt — dann steht `id` für die ganze Kette, was den
                // Zyklusschutz höchstens strenger macht.
                stack.push(path.into_iter().map(|at| (at, id)).collect());
                while let Some(way) = stack.pop() {
                    let Some(&(_, id)) = way.last() else {
                        continue;
                    };
                    closed.push((way.iter().map(|(at, _)| *at).collect(), id));
                    if way.len() >= MAX_FORM_DEPTH {
                        continue;
                    }
                    let Some(children) = self.nested_forms.get(&id) else {
                        continue;
                    };
                    for (at, child) in children.iter().rev() {
                        if way.iter().any(|(_, up)| up == child) {
                            continue; // Zyklus
                        }
                        // Gefragt wird erst hier, wo eine Kante wirklich
                        // aufzuklappen ist. Stand die Frage davor — beim
                        // Blatt, das gar keine Kinder hat —, meldete genau
                        // `MAX_MIRROR_FORM_PLACEMENTS` Aufklappungen einen
                        // Verlust, obwohl nichts verloren ging (Befund
                        // Q3-1b; `decke_99999.pdf` 0, `decke_100000.pdf` 3).
                        if !budget.mirror_expansion() {
                            break;
                        }
                        let mut deeper = way.clone();
                        deeper.push((*at, *child));
                        stack.push(deeper);
                    }
                }
            }
            record.forms = closed;
        }
    }
}

impl ContentSink for ScanResult {
    /// Die Bilder ja, den Grafikzustand nein — siehe
    /// [`ContentSink::wants_images`].
    fn wants_images(&self) -> bool {
        true
    }

    /// Jede Bildplatzierung — auch in einem Strom ohne Textspiegel darüber.
    ///
    /// Der frühere Filter „nur in Strömen mit Spiegel“ sparte nichts Nennbares
    /// und war zweimal falsch. Erstens steht der Spiegel oft in einem
    /// **anderen** Strom als das Bild: `/Figure <</Alt …>> BDC /Fm0 Do EMC` im
    /// Seitenstrom, `/Im0 Do` in `Fm0` — die Platzierung fiel durch den Filter,
    /// der Spiegel blieb mit dem Klartext stehen. Zweitens braucht der
    /// Ersatztext **am Bilddictionary** die Liste auch in einer Datei ganz ohne
    /// Marked Content; eine Seite ohne `BDC` lieferte nichts, und das `/Alt`
    /// eines unlesbaren Bildes blieb stehen.
    fn image(&mut self, cx: &SinkContext, event: &ImageEvent) {
        let id = event
            .name
            .and_then(|name| image_id_of(cx.doc, cx.resources, name));
        self.images.push(ImagePlacement {
            stream: cx.stream,
            op_index: cx.op_index,
            id,
            bounds: unit_square_bounds(&event.ctm),
            quad: unit_square_quad(&event.ctm),
        });
    }

    fn form_within(&mut self, parent: StreamKey, at: usize, id: ObjectId) {
        let StreamKey::Form(parent) = parent else {
            return;
        };
        self.nested_forms
            .entry(parent)
            .or_default()
            .insert((at, id));
    }

    fn show(&mut self, record: ShowRecord) {
        self.shows.push(record);
    }

    fn marked(&mut self, record: MarkedTextRecord) {
        // Ein mehrfach platziertes Form-XObject wird mehrfach durchlaufen; sein
        // Strom wird aber nur **einmal** neu geschrieben. Derselbe Spiegel darf
        // deshalb nicht mehrfach in der Liste stehen.
        //
        // „Derselbe“ heißt: dieselbe Operation **und** dieselbe
        // Eigenschaftsliste. Ein Formular ohne eigenes `/Resources` löst
        // `/MC0` unter jeder Umgebung neu auf — zeichnet es Fm0 mit einem
        // Spiegel und danach die Seite mit einem anderen, sind das zwei
        // Listen an zwei Fundorten. Wer nur nach (Strom, Operation) prüfte,
        // verwarf die zweite: ihr Klartext blieb im Verzeichnis der Seite,
        // ohne Warnung, mit Rückgabewert 0 (Spur-A-Runde 1, Register #65).
        if !self.seen_marked.insert((
            record.stream,
            record.op_index,
            record.property_owner,
            record.property_id,
        )) {
            return;
        }
        self.marked.push(record);
    }

    fn form(&mut self, id: ObjectId) {
        *self.form_placements.entry(id).or_insert(0) += 1;
    }

    fn declares_form(&mut self, id: ObjectId, name: &[u8]) {
        self.declared_forms
            .entry(id)
            .or_insert_with(|| name.to_vec());
    }

    fn warn(&mut self, message: String) {
        if self.seen_warnings.insert(message.clone()) {
            self.warnings.push(message);
        }
    }
}

// ---------------------------------------------------------------------------
// Senke
// ---------------------------------------------------------------------------

/// Umgebung eines Sink-Aufrufs: Dokument, Ressourcen und Herkunft.
pub struct SinkContext<'a> {
    pub doc: &'a Document,
    pub resources: Option<&'a Dictionary>,
    pub stream: StreamKey,
    pub op_index: usize,
}

/// Eine einzelne Glyphe, exakt so positioniert wie in [`GlyphItem`].
pub struct GlyphEvent<'a> {
    /// Ressourcenname aus dem letzten `Tf` (Schlüssel in `/Resources /Font`).
    pub font_name: &'a [u8],
    /// Die **bereits geparsten** Metriken dieses Fonts.
    ///
    /// Der Interpreter lädt zu Beginn jeder Ressourcenebene alle Fonts; wer
    /// sie hier noch einmal aus dem Dictionary läse, parste jedes
    /// `/ToUnicode`, jede `/W`-Liste und jede `cmap` ein zweites Mal.
    pub font: &'a FontInfo,
    pub code: u32,
    pub text: &'a str,
    /// Text-Rendering-Matrix: bildet Text-Space (1 Einheit = Schriftgröße)
    /// auf den User-Space ab. Enthält Schriftgröße, `Tz`, `Ts`, `Tm` und CTM.
    pub trm: Matrix,
    pub fill: Rgb,
    pub fill_alpha: f32,
    /// PDF-Textrendermodus aus `Tr` (3 = unsichtbar).
    pub render_mode: u8,
    pub clip: Option<usize>,
}

/// Ein fertig gemalter Pfad; die Punkte liegen bereits im User-Space.
pub struct PathEvent<'a> {
    pub segments: &'a [PathSeg],
    pub fill: Option<Rgb>,
    pub stroke: Option<Stroke>,
    pub even_odd: bool,
    pub fill_alpha: f32,
    pub clip: Option<usize>,
}

/// Ein platziertes Bild (XObject oder Inline-Bild).
pub struct ImageEvent<'a> {
    /// Name des XObjects; `None` bei einem Inline-Bild.
    pub name: Option<&'a [u8]>,
    /// Inline-Bild: Dictionary und (noch gefilterte) Rohdaten.
    pub inline: Option<(&'a Dictionary, &'a [u8])>,
    /// Bildet das Einheitsquadrat auf die Zielfläche im User-Space ab.
    pub ctm: Matrix,
    /// Aktuelle Füllfarbe — bei `/ImageMask` wird damit gemalt.
    pub fill: Rgb,
    pub fill_alpha: f32,
    pub clip: Option<usize>,
}

/// Abnehmer der Interpreter-Ereignisse.
///
/// Alle Methoden haben eine leere Standardimplementierung, eine Senke nimmt
/// sich also genau das, was sie braucht.
pub trait ContentSink {
    /// Nur wenn `true`, werden Farben, Pfade, Clips und Bilder ausgewertet.
    fn wants_graphics(&self) -> bool {
        false
    }
    /// Nur wenn `true`, werden **Bildplatzierungen** gemeldet
    /// ([`ContentSink::image`]).
    ///
    /// Eigene Frage, weil eine Senke die Bilder brauchen kann, ohne den
    /// vollen Grafikzustand zu wollen: die CTM führt der Interpreter ohnehin
    /// nach (`q`, `Q`, `cm` stehen außerhalb jeder Abfrage), Farben, Pfade und
    /// Clips nicht. Die Schwärzung braucht genau die Fläche eines Bildes und
    /// nichts weiter — siehe [`ScanResult::images`]. Voreinstellung ist
    /// [`ContentSink::wants_graphics`]: wer Grafik will, bekommt die Bilder
    /// wie bisher, und für jede andere Senke ändert sich nichts.
    ///
    /// Was eine Senke, die nur hier `true` sagt, **nicht** bekommt: gültige
    /// `fill`, `fill_alpha` und `clip` im [`ImageEvent`] — der Farb- und
    /// Clip-Zustand wird dann nicht nachgeführt. Wer sie liest, fragt nach
    /// `wants_graphics`.
    fn wants_images(&self) -> bool {
        self.wants_graphics()
    }
    /// Eine abgeschlossene Text-Ausgabe-Operation.
    fn show(&mut self, _record: ShowRecord) {}
    /// Ein Marked-Content-Abschnitt mit Textspiegel (`/ActualText`, `/Alt`,
    /// `/E`).
    fn marked(&mut self, _record: MarkedTextRecord) {}
    /// Eine einzelne Glyphe (nur bei `wants_graphics`).
    fn glyph(&mut self, _cx: &SinkContext, _event: &GlyphEvent) {}
    /// Ein gemalter Pfad (nur bei `wants_graphics`).
    fn path(&mut self, _cx: &SinkContext, _event: &PathEvent) {}
    /// Ein platziertes Bild (nur bei `wants_graphics`).
    fn image(&mut self, _cx: &SinkContext, _event: &ImageEvent) {}
    /// Ein neuer Clip-Pfad; der Rückgabewert identifiziert ihn für spätere
    /// Ereignisse.
    fn clip(&mut self, _cx: &SinkContext, _segments: &[PathSeg], _even_odd: bool) -> Option<usize> {
        None
    }
    /// Ein Form-XObject wurde platziert.
    fn form(&mut self, _id: ObjectId) {}
    /// Ein Form-XObject `id` wurde **aus dem Strom `parent`** heraus
    /// platziert, durch das `Do` an Index `at` dieses Stroms — Formular im
    /// Formular, wenn `parent` selbst eines ist.
    ///
    /// Nur die Verschachtelung, nicht die Platzierung: die zählt
    /// [`ContentSink::form`]. Gebraucht wird sie, um den Geltungsbereich eines
    /// Textspiegels ([`MarkedTextRecord::forms`]) über Formulargrenzen hinweg
    /// zu schließen — mit `at`, damit die Glyphen des inneren Formulars an
    /// der Stelle seines `Do` in die Glyphenfolge des äußeren fallen.
    fn form_within(&mut self, _parent: StreamKey, _at: usize, _id: ObjectId) {}
    /// Ein Form-XObject **steht in den Ressourcen** eines gelesenen Stroms.
    ///
    /// Das ist nicht dasselbe wie [`ContentSink::form`]: dort wird gezeichnet,
    /// hier nur angeboten. Die Differenz beider Mengen ist genau der Text, den
    /// der Interpreter nie betritt — siehe
    /// [`crate::extract::PdfExtractor::extract_with_warnings`].
    fn declares_form(&mut self, _id: ObjectId, _name: &[u8]) {}
    /// Ein Befund, der den Nutzer erreichen muss (siehe [`ScanResult::warnings`]).
    fn warn(&mut self, _message: String) {}
}

#[derive(Debug, Clone)]
struct TextState {
    /// Die Metriken des zuletzt gesetzten Fonts — **geteilt**, nicht kopiert.
    ///
    /// Der Grafikzustand wird bei jedem `q` kopiert und bei jedem `Tf` neu
    /// gesetzt; stünde hier ein [`FontInfo`] als Wert, kopierte jedes `q` und
    /// jedes `Tf` die vollständige `/ToUnicode`-Zuordnung mit. Gemessen an
    /// einer Schrift mit 125 000 Einträgen: 3,8 ms je `Tf`, 4,1 ms je `q Q`,
    /// linear wachsend — und das Aufwandskonto lässt 1 000 000 Operationen zu.
    /// Es ist genau der Fall, den der Zwischenspeicher aus v0.4.0 **nicht**
    /// abdeckt: der verhindert das erneute Parsen, nicht das Kopieren.
    font: Option<Rc<FontInfo>>,
    /// Ressourcenname des zuletzt gesetzten Fonts.
    font_name: Vec<u8>,
    font_size: f64,
    char_spacing: f64,
    word_spacing: f64,
    h_scale: f64,
    leading: f64,
    rise: f64,
    render_mode: u8,
}

impl Default for TextState {
    fn default() -> Self {
        Self {
            font: None,
            font_name: Vec::new(),
            font_size: 0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            h_scale: 1.0,
            leading: 0.0,
            rise: 0.0,
            render_mode: 0,
        }
    }
}

#[derive(Debug, Clone)]
struct GraphicsState {
    ctm: Matrix,
    text: TextState,
    fill: Rgb,
    stroke: Rgb,
    fill_space: ColorSpace,
    stroke_space: ColorSpace,
    fill_alpha: f32,
    stroke_alpha: f32,
    line_width: f64,
    line_cap: u8,
    line_join: u8,
    dash: Vec<f64>,
    dash_phase: f64,
    clip: Option<usize>,
}

impl GraphicsState {
    fn new(ctm: Matrix) -> Self {
        Self {
            ctm,
            text: TextState::default(),
            fill: Rgb::BLACK,
            stroke: Rgb::BLACK,
            fill_space: ColorSpace::Gray,
            stroke_space: ColorSpace::Gray,
            fill_alpha: 1.0,
            stroke_alpha: 1.0,
            line_width: 1.0,
            line_cap: 0,
            line_join: 0,
            dash: Vec::new(),
            dash_phase: 0.0,
            clip: None,
        }
    }

    /// Strichbeschreibung im User-Space: Breite und Strichelung werden mit der
    /// CTM skaliert, weil der Renderer nur noch User-Space-Werte sieht.
    fn stroke_style(&self) -> Stroke {
        let scale = self.ctm.scale_hint();
        Stroke {
            color: self.stroke,
            width: self.line_width * scale,
            cap: self.line_cap,
            join: self.line_join,
            dash: self.dash.iter().map(|d| d * scale).collect(),
            dash_phase: self.dash_phase * scale,
            alpha: self.stroke_alpha,
        }
    }
}

// ---------------------------------------------------------------------------
// Farbräume
// ---------------------------------------------------------------------------

/// Unterstützte Farbräume.
///
/// Genähert werden bewusst:
///
/// * `ICCBased` → nach Komponentenzahl (`/N` 1/3/4) als Grau/RGB/CMYK,
/// * `CalRGB`/`CalGray` → wie die Device-Varianten,
/// * `Lab` → nur die Helligkeit `L` als Grauwert,
/// * `Separation`/`DeviceN` → Tinte 1.0 bedeutet Schwarz (`1 - max(tint)`),
/// * `Pattern` → mittleres Grau, damit gemusterte Flächen sichtbar bleiben.
///
/// Alles andere wird zu Schwarz.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ColorSpace {
    Gray,
    Rgb,
    Cmyk,
    /// `L` aus dem Lab-Raum (0..100) als Grauwert.
    Lab,
    Indexed {
        base: Box<ColorSpace>,
        lookup: Vec<u8>,
    },
    /// Eine oder mehrere Tinten (`Separation`, `DeviceN`).
    Tint(usize),
    Pattern,
    Unknown,
}

impl ColorSpace {
    /// Anzahl der Farbkomponenten je Bildpunkt.
    pub(crate) fn components(&self) -> usize {
        match self {
            ColorSpace::Gray | ColorSpace::Indexed { .. } => 1,
            ColorSpace::Rgb | ColorSpace::Lab => 3,
            ColorSpace::Cmyk => 4,
            ColorSpace::Tint(n) => *n,
            ColorSpace::Pattern | ColorSpace::Unknown => 1,
        }
    }

    /// Rechnet Komponentenwerte in RGB um. Bei `Indexed` ist `values[0]` der
    /// rohe Index in die Palette, sonst liegen alle Werte in 0.0..=1.0.
    pub(crate) fn to_rgb(&self, values: &[f64]) -> Rgb {
        let get = |i: usize| values.get(i).copied().unwrap_or(0.0);
        match self {
            ColorSpace::Gray => Rgb::gray(get(0)),
            ColorSpace::Rgb => Rgb::new(get(0), get(1), get(2)),
            ColorSpace::Cmyk => cmyk_to_rgb(get(0), get(1), get(2), get(3)),
            ColorSpace::Lab => Rgb::gray((get(0) / 100.0).clamp(0.0, 1.0)),
            ColorSpace::Indexed { base, lookup } => {
                let n = base.components();
                let index = get(0).max(0.0) as usize;
                let start = index * n;
                if start + n > lookup.len() {
                    return Rgb::BLACK;
                }
                let comps: Vec<f64> = lookup[start..start + n]
                    .iter()
                    .map(|b| *b as f64 / 255.0)
                    .collect();
                // Lab-Paletten sind selten; die Helligkeit wird auf 0..100 zurückskaliert.
                if matches!(**base, ColorSpace::Lab) {
                    return base.to_rgb(&[comps[0] * 100.0]);
                }
                base.to_rgb(&comps)
            }
            ColorSpace::Tint(n) => {
                let tint = (0..*n).map(get).fold(0.0f64, f64::max);
                Rgb::gray(1.0 - tint.clamp(0.0, 1.0))
            }
            ColorSpace::Pattern => Rgb::gray(0.5),
            ColorSpace::Unknown => Rgb::BLACK,
        }
    }

    /// Startfarbe eines Farbraums nach `cs`/`CS` (PDF 32000-1, 8.6.3).
    fn initial_color(&self) -> Rgb {
        match self {
            ColorSpace::Pattern => Rgb::gray(0.5),
            ColorSpace::Indexed { .. } => self.to_rgb(&[0.0]),
            _ => Rgb::BLACK,
        }
    }

    /// Löst ein Farbraum-Objekt auf — entweder ein Device-Name, ein Verweis in
    /// `/Resources /ColorSpace` oder ein Array wie `[/Indexed …]`.
    pub(crate) fn resolve(doc: &Document, resources: Option<&Dictionary>, obj: &Object) -> Self {
        Self::resolve_depth(doc, resources, obj, 0)
    }

    fn resolve_depth(
        doc: &Document,
        resources: Option<&Dictionary>,
        obj: &Object,
        depth: usize,
    ) -> Self {
        if depth > 8 {
            return ColorSpace::Unknown;
        }
        let resolved = doc.dereference(obj).map(|(_, o)| o).unwrap_or(obj);
        match resolved {
            Object::Name(name) => match name.as_slice() {
                b"DeviceGray" | b"G" | b"CalGray" => ColorSpace::Gray,
                b"DeviceRGB" | b"RGB" | b"CalRGB" => ColorSpace::Rgb,
                b"DeviceCMYK" | b"CMYK" => ColorSpace::Cmyk,
                b"Pattern" => ColorSpace::Pattern,
                b"Indexed" | b"I" => ColorSpace::Unknown,
                other => {
                    // Benannter Farbraum aus den Ressourcen.
                    let entry = resources
                        .and_then(|r| r.get(b"ColorSpace").ok())
                        .and_then(|o| doc.dereference(o).ok())
                        .and_then(|(_, o)| o.as_dict().ok())
                        .and_then(|d| d.get(other).ok());
                    match entry {
                        Some(entry) => Self::resolve_depth(doc, resources, entry, depth + 1),
                        None => ColorSpace::Unknown,
                    }
                }
            },
            Object::Array(items) => Self::resolve_array(doc, resources, items, depth),
            _ => ColorSpace::Unknown,
        }
    }

    fn resolve_array(
        doc: &Document,
        resources: Option<&Dictionary>,
        items: &[Object],
        depth: usize,
    ) -> Self {
        let Some(family) = items.first().and_then(|o| o.as_name().ok()) else {
            return ColorSpace::Unknown;
        };
        match family {
            b"ICCBased" => {
                let n = items
                    .get(1)
                    .and_then(|o| doc.dereference(o).ok())
                    .and_then(|(_, o)| o.as_stream().ok().map(|s| s.dict.clone()))
                    .and_then(|d| d.get(b"N").ok().and_then(as_f64))
                    .unwrap_or(3.0) as i64;
                match n {
                    1 => ColorSpace::Gray,
                    4 => ColorSpace::Cmyk,
                    _ => ColorSpace::Rgb,
                }
            }
            b"CalGray" => ColorSpace::Gray,
            b"CalRGB" => ColorSpace::Rgb,
            b"Lab" => ColorSpace::Lab,
            b"Indexed" | b"I" => {
                let base = items
                    .get(1)
                    .map(|o| Self::resolve_depth(doc, resources, o, depth + 1))
                    .unwrap_or(ColorSpace::Unknown);
                let lookup = items
                    .get(3)
                    .and_then(|o| doc.dereference(o).ok())
                    .and_then(|(_, o)| match o {
                        Object::String(bytes, _) => Some(bytes.clone()),
                        Object::Stream(stream) => stream
                            .decompressed_content()
                            .or_else(|_| stream.get_plain_content())
                            .ok(),
                        _ => None,
                    })
                    .unwrap_or_default();
                ColorSpace::Indexed {
                    base: Box::new(base),
                    lookup,
                }
            }
            b"Separation" => ColorSpace::Tint(1),
            b"DeviceN" => {
                let n = items
                    .get(1)
                    .and_then(|o| doc.dereference(o).ok())
                    .and_then(|(_, o)| o.as_array().ok().map(|a| a.len()))
                    .unwrap_or(1);
                ColorSpace::Tint(n.max(1))
            }
            b"Pattern" => ColorSpace::Pattern,
            b"DeviceGray" | b"G" => ColorSpace::Gray,
            b"DeviceRGB" | b"RGB" => ColorSpace::Rgb,
            b"DeviceCMYK" | b"CMYK" => ColorSpace::Cmyk,
            _ => ColorSpace::Unknown,
        }
    }
}

/// Standardumrechnung CMYK → RGB (PDF 32000-1, 10.4.2).
pub(crate) fn cmyk_to_rgb(c: f64, m: f64, y: f64, k: f64) -> Rgb {
    Rgb::new(
        (1.0 - c) * (1.0 - k),
        (1.0 - m) * (1.0 - k),
        (1.0 - y) * (1.0 - k),
    )
}

/// Scannt den Content-Stream einer Seite inklusive Form-XObjects und der
/// Erscheinungsströme (`/AP`) ihrer Annotationen.
///
/// Gibt `Err` zurück, sobald der Seiteninhalt **nicht vollständig durchsucht
/// werden konnte** — ein Strom, der sich nicht zerlegen lässt, oder einer, der
/// das Aufwandskonto [`Budget`] sprengt. Eine solche Seite darf nicht als
/// Erfolg durchgehen: „0 Schwärzungen, Rückgabewert 0“ liest sich wie
/// „nichts gefunden, also sauber“, und genau das wäre es dann nicht.
pub fn scan_page(doc: &Document, page_id: ObjectId) -> Result<ScanResult> {
    // Über [`crate::filters`], nicht `Document::get_page_content`: der eigene
    // Dekoder kennt `ASCIIHexDecode` und `RunLengthDecode`, `lopdf` nicht.
    let content_data = crate::filters::page_content(doc, page_id);
    // Nicht `lopdf::content::Content::decode`: dessen Parser kennt kein
    // `BI … ID … EI`. Die Binärdaten hinter dem `ID` bringen ihn aus dem Tritt,
    // der Rest des Streams geht verloren — Text hinter einem Inline-Bild wäre
    // für die Analyse unsichtbar und könnte nie geschwärzt werden.
    // [`crate::ops::decode_content`] schneidet die Bilder vorher heraus.
    let decoded = crate::ops::decode_content_checked(&content_data);

    // Ein Abschnitt, der nur bis zur Hälfte zerlegt wurde, fiel früher lautlos
    // unter den Tisch — nicht der ganze Strom, deshalb greift die Prüfung
    // darunter nicht, und mit ihm verschwand jeder Text, der dahinter stand.
    // Derselbe Fehler, nur kleiner und deshalb noch schwerer zu bemerken.
    if !decoded.truncated.is_empty() {
        return Err(RedactError::Pdf(format!(
            "Ein Teil des Seiteninhalts ließ sich nicht in Operationen zerlegen: in {} \
             Teilstück(en) von zusammen {} Byte bricht die Zerlegung ab, alles dahinter \
             fehlt. Dieser Text wurde nicht durchsucht und kann deshalb nicht geschwärzt \
             worden sein; beim Neuschreiben der Seite ginge er zudem ersatzlos verloren. \
             Die Datei wird abgelehnt, statt eine halbe Seite als geschwärzt auszugeben.",
            decoded.truncated.len(),
            decoded.affected_bytes(),
        )));
    }
    let operations = decoded.operations;

    // Früher eine Warnung, jetzt ein Abbruch. Der Unterschied ist der
    // Rückgabewert: eine Warnung auf stderr macht aus einem Lauf, der den Text
    // dieser Seite nachweislich nie gesehen hat, trotzdem eine Datei, die im
    // Stapelbetrieb als „verarbeitet“ zählt. Gemessen an einer 1 122 Byte
    // großen Datei: „Schwärzungen: 0“, Rückgabewert 0 — und die Kontonummer
    // stand unverändert in der Ausgabe.
    if operations.is_empty() && has_tokens(&content_data) {
        return Err(RedactError::Pdf(
            "Der Content-Stream dieser Seite ließ sich nicht in Operationen zerlegen; \
             ihr Text wurde nicht durchsucht und kann deshalb nicht geschwärzt worden \
             sein. Die Datei wird abgelehnt, statt eine ungeprüfte Seite als geschwärzt \
             auszugeben."
                .into(),
        ));
    }

    let resources = page_resources(doc, page_id);
    let mut result = ScanResult::default();
    let mut budget = Budget::default();
    budget.credit(None, operations.len());
    scan_with_budget(
        doc,
        &operations,
        StreamKey::Page,
        resources.as_ref(),
        true,
        Some(page_id),
        None,
        Matrix::IDENTITY,
        &mut budget,
        &mut result,
    );
    scan_annotations(doc, page_id, resources.as_ref(), &mut budget, &mut result);
    budget.result()?;
    result.close_forms(&mut budget);
    result.effort = budget.effort;
    result.effort.retained_weight = budget.cached_operations;
    Ok(result)
}

/// Enthält der Stream überhaupt etwas anderes als Leerraum?
fn has_tokens(data: &[u8]) -> bool {
    data.iter().any(|b| !b.is_ascii_whitespace())
}

/// Zählt je Font, wie viel des dekodierten Textes unbrauchbar ist.
///
/// Ein Identity-H-Subset ohne `/ToUnicode` lässt sich nicht dekodieren: die
/// CIDs sind reine Glyphnummern. Der Identity-Rückfall in
/// [`crate::encoding::CharMap::text_for`] macht daraus Steuerzeichen, die
/// Analyse findet nichts, und die Schwärzung meldet Erfolg — an einer Datei,
/// in der alles stehen geblieben ist. Genau dieser Fall muss laut werden.
#[derive(Debug, Default)]
struct FontDecodeStats {
    /// (Ressourcenname, `/BaseFont`) → (Zeichen gesamt, davon unlesbar)
    per_font: BTreeMap<(Vec<u8>, String), (usize, usize)>,
    /// Fonts **mit** `/ToUnicode`: was ihre Codes ergeben — siehe
    /// [`UniformMapping`].
    mapped: BTreeMap<(Vec<u8>, String), UniformMapping>,
}

/// Ob ein `/ToUnicode` alle benutzten Codes auf denselben Text abbildet.
///
/// Ein `/ToUnicode` ist eine Behauptung des Erzeugers, keine Eigenschaft der
/// Glyphen: der Betrachter zeichnet, was im Font steht, und kopiert, was die
/// CMap sagt. Gemessen an einer Datei, deren CMap jeden Code auf „x“ legt:
/// der Betrachter zeigte die IBAN, die Analyse las „xxxxxxxx“, fand nichts
/// und endete mit Rückgabewert 0. Eine CMap, die viele verschiedene Codes auf
/// **ein** Zeichen legt, ist sichtbar unplausibel — ein Font hat für ein
/// Zeichen ein, zwei, vielleicht drei Glyphvarianten, nicht acht.
///
/// Nur diese eine Form wird erkannt. Eine CMap, die die Zeichen *vertauscht*,
/// ist von einer richtigen nicht zu unterscheiden, ohne die Glyphen selbst zu
/// lesen.
#[derive(Debug, Default)]
struct UniformMapping {
    codes: HashSet<u32>,
    text: String,
    uniform: bool,
}

/// Ab so vielen verschiedenen Codes auf denselben Text gilt ein `/ToUnicode`
/// als unplausibel. Die Zahl zählt **Codes** (verschiedene Glyphen), nicht
/// Zeichen im Strom: ein Wort mit hundert „x“ ist ein Code.
const UNIFORM_MAPPING_MIN_CODES: usize = 8;

impl FontDecodeStats {
    fn record(&mut self, font_name: &[u8], font: &FontInfo, code: u32, text: &str) {
        // Fonts mit /ToUnicode sagen selbst, was ihre Codes bedeuten — ob
        // glaubhaft, hält [`UniformMapping`] fest. Leerraum zählt nicht mit:
        // dass mehrere Codes ein Leerzeichen ergeben, ist gewöhnlich.
        if font.charmap.has_to_unicode() {
            if text.trim().is_empty() || text.contains(crate::encoding::REPLACEMENT) {
                return;
            }
            let entry = self
                .mapped
                .entry((font_name.to_vec(), font.base_font.clone()))
                .or_insert_with(|| UniformMapping {
                    codes: HashSet::new(),
                    text: text.to_string(),
                    uniform: true,
                });
            if entry.codes.insert(code) && entry.text != text {
                entry.uniform = false;
            }
            return;
        }
        let entry = self
            .per_font
            .entry((font_name.to_vec(), font.base_font.clone()))
            .or_insert((0, 0));
        entry.0 += 1;
        if text.chars().any(|c| c == crate::encoding::REPLACEMENT) {
            entry.1 += 1;
        }
    }

    fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        for ((resource, base_font), (total, unreadable)) in &self.per_font {
            if *unreadable == 0 {
                continue;
            }
            let enough = *unreadable >= UNREADABLE_MIN_GLYPHS
                || (*unreadable as f64) >= *total as f64 * UNREADABLE_RATIO;
            if !enough {
                continue;
            }
            let name = if base_font.is_empty() {
                String::from_utf8_lossy(resource).into_owned()
            } else {
                base_font.clone()
            };
            out.push(format!(
                "Font „{name}“ hat kein /ToUnicode; sein Text lässt sich nicht \
                 dekodieren. Muster können darin nicht erkannt werden — diese \
                 Seite wurde möglicherweise nicht vollständig geschwärzt."
            ));
        }
        for ((resource, base_font), mapping) in &self.mapped {
            if !mapping.uniform || mapping.codes.len() < UNIFORM_MAPPING_MIN_CODES {
                continue;
            }
            let name = if base_font.is_empty() {
                String::from_utf8_lossy(resource).into_owned()
            } else {
                base_font.clone()
            };
            out.push(format!(
                "Font „{name}“ hat ein /ToUnicode, das {} verschiedene Zeichencodes auf \
                 denselben Text „{}“ abbildet. Das ist nicht glaubhaft: der Betrachter \
                 zeichnet die Glyphen des Fonts, die Analyse liest nur, was das \
                 /ToUnicode behauptet. Muster können darin nicht erkannt werden — diese \
                 Seite wurde möglicherweise nicht vollständig geschwärzt.",
                mapping.codes.len(),
                mapping.text
            ));
        }
        out
    }
}

/// Führt einen bereits dekodierten Operationsstrom durch den Interpreter.
///
/// Damit können Aufrufer den Stream selbst dekodieren (z. B. um Inline-Bilder
/// vorher herauszutrennen) und trotzdem exakt dieselbe Zustandsführung
/// benutzen wie [`scan_page`].
///
/// Gibt `Err` zurück, wenn das Aufwandskonto [`Budget`] gerissen wurde — der
/// Durchlauf ist dann unvollständig, und was der Interpreter nicht gesehen
/// hat, kann auch nicht geschwärzt werden.
pub fn interpret(
    doc: &Document,
    operations: &[Operation],
    stream: StreamKey,
    resources: Option<&Dictionary>,
    initial_ctm: Matrix,
    sink: &mut dyn ContentSink,
) -> Result<()> {
    let mut budget = Budget::default();
    budget.credit(None, operations.len());
    // Ohne Seitenkontext: `interpret` bekommt die Ressourcen fertig gemischt
    // und kennt die Seite nicht. Keine seiner Senken liest Textspiegel, und
    // `property_owner` bleibt entsprechend leer.
    scan_with_budget(
        doc,
        operations,
        stream,
        resources,
        true,
        None,
        None,
        initial_ctm,
        &mut budget,
        sink,
    );
    budget.result()
}

/// Der gemeinsame Kern von [`interpret`] und [`scan_page`] — mit einem
/// Aufwandskonto, das über mehrere Ströme derselben Seite hinweg gilt.
///
/// `fonts` sind die bereits geladenen Schriften zu `resources`, falls der
/// Aufrufer sie schon hat; `None` heißt „bitte laden“. Ein
/// Erscheinungsstrom, an dem zweihundert Annotationen hängen, bringt seine
/// eigenen Schriften genau einmal mit — sie hier erneut zu laden wäre
/// dieselbe Vervielfachung wie beim Form-XObject.
///
/// `own_resources` sagt dasselbe für die `/XObject`-Liste: `true` heißt, dieser
/// Strom bringt `resources` selbst mit und muss die darin stehenden Formulare
/// anbieten. Ein Erscheinungsstrom, der die Ressourcen der Seite erbt, gibt
/// hier `false` — die Seite hat sie bereits angeboten. Siehe
/// [`DeclarationKey`].
#[allow(clippy::too_many_arguments)]
fn scan_with_budget(
    doc: &Document,
    operations: &[Operation],
    stream: StreamKey,
    resources: Option<&Dictionary>,
    own_resources: bool,
    owner: Option<ObjectId>,
    fonts: Option<&FontMap>,
    initial_ctm: Matrix,
    budget: &mut Budget,
    sink: &mut dyn ContentSink,
) {
    let loaded;
    let fonts = match fonts {
        Some(fonts) => fonts,
        None => {
            loaded = budget.load_font_map(doc, resources).0;
            &loaded
        }
    };
    let mut visiting = HashSet::new();
    let mut stats = FontDecodeStats::default();
    scan_operations(
        doc,
        operations,
        stream,
        resources,
        own_resources,
        owner,
        fonts,
        initial_ctm,
        0,
        &mut visiting,
        &mut stats,
        budget,
        sink,
    );
    for warning in stats.warnings() {
        sink.warn(warning);
    }
}

/// Wie viele `/Parent`-Schritte im Seitenbaum verfolgt werden.
///
/// Ein Seitenbaum ist selten tiefer als eine Handvoll Ebenen; die Grenze
/// begrenzt nur den Aufwand bei absichtlich entarteten Dateien. Zyklen fängt
/// ohnehin die Besuchsmenge ab.
const MAX_PAGE_TREE_DEPTH: usize = 64;

/// Sammelt das (ggf. geerbte) `/Resources`-Dictionary einer Seite.
///
/// Die Vererbungskette wird **selbst** abgelaufen und nicht
/// `lopdf::Document::get_page_resources` überlassen: das liefert die geerbten
/// Ressourcen nur als Liste von Objekt-Ids. Steht `/Resources` am
/// `/Pages`-Knoten als *direktes* Dictionary — nach PDF 32000-1, Tabelle 30
/// völlig regulär —, hat es keine Objekt-Id und fällt aus dem Ergebnis. Die
/// Seite sah dann so aus, als hätte sie **gar keine** Ressourcen: ein
/// `/XObject`, das ihr Strom mit `Do` zeichnet, war nicht auflösbar, sein Text
/// wurde nie gelesen und konnte nie geschwärzt werden. Gemessen an einer
/// zweiseitigen Datei: „Treffer 0, Rückgabewert 0“, und die IBAN stand
/// unverändert in der Ausgabe.
pub fn page_resources(doc: &Document, page_id: ObjectId) -> Option<Dictionary> {
    // Von der Seite aufwärts sammeln …
    let mut chain: Vec<Dictionary> = Vec::new();
    let mut seen: HashSet<ObjectId> = HashSet::new();
    let mut current = Some(page_id);
    while let Some(id) = current {
        if seen.len() >= MAX_PAGE_TREE_DEPTH || !seen.insert(id) {
            break;
        }
        let Ok(node) = doc.get_dictionary(id) else {
            break;
        };
        if let Some(resources) = node
            .get(b"Resources")
            .ok()
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| o.as_dict().ok())
        {
            chain.push(resources.clone());
        }
        current = match node.get(b"Parent") {
            Ok(Object::Reference(parent)) => Some(*parent),
            _ => None,
        };
    }

    // … und von oben nach unten mischen, damit die seiteneigenen Ressourcen
    // die geerbten überschreiben.
    let mut merged = Dictionary::new();
    for resources in chain.iter().rev() {
        merge_resources(&mut merged, resources);
    }
    Some(merged)
}

/// Setzt dieser Strom überhaupt Text?
///
/// Ein Formular ohne Textoperator ist eine Zeichnung — ein Logo, ein Rahmen,
/// eine Schraffur. Dass es niemand zeichnet, ist dann kein Befund, und eine
/// Meldung darüber wäre nur Rauschen (vgl. das Kachelmuster in
/// [`scan_tiling_pattern`]).
pub fn stream_shows_text(doc: &Document, id: ObjectId) -> bool {
    let Ok(stream) = doc.get_object(id).and_then(|o| o.as_stream()) else {
        return false;
    };
    let Ok(data) = stream
        .decompressed_content()
        .or_else(|_| stream.get_plain_content())
    else {
        // Nicht lesbar heißt nicht harmlos: hier kann Text stehen.
        return true;
    };
    crate::ops::decode_content_checked(&data)
        .operations
        .iter()
        .any(|op| matches!(op.operator.as_str(), "Tj" | "TJ" | "'" | "\""))
}

// ---------------------------------------------------------------------------
// Annotationen
// ---------------------------------------------------------------------------

/// Zieht die Erscheinungsströme (`/AP`) der Seitenannotationen mit in die
/// Extraktion.
///
/// Eine `/FreeText`-Annotation trägt ihren sichtbaren Text nicht im
/// Seiten-Content-Stream, sondern in einem eigenen Form-XObject unter
/// `/AP /N`. Wer nur den Seitenstrom liest, sieht davon nichts: ein Muster
/// kann dort nichts treffen, und was nicht getroffen wird, wird auch nicht
/// geschwärzt. Die Ströme werden deshalb wie Form-XObjects durchlaufen —
/// transformiert mit der Abbildung aus PDF 32000-1, 12.5.5 (Algorithmus 8.1),
/// damit die Glyphen dort liegen, wo die Annotation auf der Seite steht.
///
/// Die Datensätze tragen [`StreamKey::Form`] mit der Objekt-Id des
/// Erscheinungsstroms; die Schwärzung kann sie damit genauso neu schreiben wie
/// ein gewöhnliches Form-XObject.
/// Wie weit eine Annotation über `/Popup`, `/IRT` und (vom Popup aus)
/// `/Parent` weitere Annotationen erreichen darf, deren Erscheinungsströme
/// dann mitgelesen werden. Gewöhnliche Dateien brauchen eine Stufe; eine
/// Antwortkette, deren Glieder alle in `/Annots` stehen, kostet über
/// `visited` ohnehin nichts Weiteres.
const MAX_ANNOTATION_LINKS: usize = 16;

fn scan_annotations(
    doc: &Document,
    page_id: ObjectId,
    page_resources: Option<&Dictionary>,
    budget: &mut Budget,
    sink: &mut dyn ContentSink,
) {
    let annots = doc
        .get_dictionary(page_id)
        .ok()
        .and_then(|d| d.get(b"Annots").ok())
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_array().ok())
        .cloned()
        .unwrap_or_default();

    // Ein Strom, der von zwei Annotationen (oder zwei Zuständen) benutzt wird,
    // wird nur einmal gelesen — sonst stünde derselbe Text doppelt im Ergebnis.
    let mut seen: HashSet<ObjectId> = HashSet::new();
    // Die Annotationen der Seite — und die, die eine von ihnen am Leben hält,
    // ohne dass sie selbst in `/Annots` stünde: die Elternnotiz eines
    // `/Popup` (`/Parent`), die Annotation, auf die eine Antwort zeigt
    // (`/IRT`), das `/Popup` selbst. Ein Werkzeug, das die Notiz aus
    // `/Annots` streicht und das Popup vergisst, hinterlässt so einen
    // Erscheinungsstrom, den `strip_metadata` über dieselben Schlüssel
    // erreicht (dort fällt `/Contents`), den hier aber niemand las: sein
    // Text stand nach dem Lauf in der Datei, ohne Warnung (Register #71).
    // Gelesen wird er mit dem `/Rect` seiner Annotation — wie jeder andere.
    let mut queue: VecDeque<(Object, usize)> =
        annots.iter().map(|annot| (annot.clone(), 0)).collect();
    let mut visited: HashSet<ObjectId> = HashSet::new();
    while let Some((annot, depth)) = queue.pop_front() {
        let Some((id, dict)) = doc
            .dereference(&annot)
            .ok()
            .and_then(|(id, o)| o.as_dict().ok().map(|d| (id, d)))
        else {
            continue;
        };
        if id.is_some_and(|id| !visited.insert(id)) {
            continue;
        }
        if depth < MAX_ANNOTATION_LINKS {
            let is_popup = dict
                .get(b"Subtype")
                .ok()
                .and_then(|o| doc.dereference(o).ok())
                .is_some_and(|(_, o)| o.as_name().is_ok_and(|n| n == b"Popup"));
            // `/Parent` nur vom Popup aus: das `/Parent` eines Widgets ist
            // ein Formularfeld, und dessen `/Kids` stehen auf anderen Seiten.
            let links: &[&[u8]] = if is_popup {
                &[b"Popup", b"IRT", b"Parent"]
            } else {
                &[b"Popup", b"IRT"]
            };
            for key in links {
                if let Ok(value) = dict.get(key) {
                    queue.push_back((value.clone(), depth + 1));
                }
            }
        }
        let rect = dict
            .get(b"Rect")
            .ok()
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| annot_rect(o));

        let appearance = dict
            .get(b"AP")
            .ok()
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| o.as_dict().ok());
        // Der Zustand, den ein Betrachter zeigt (`/AS`), zuerst — die übrigen
        // danach. Siehe [`appearance_streams`]: sie fallen nicht aus der
        // Analyse, sie bekommen nur ihre eigene Druckschicht.
        let selected = dict
            .get(b"AS")
            .ok()
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| o.as_name().ok().map(|n| n.to_vec()));
        let mut streams = Vec::new();
        if let Some(appearance) = appearance {
            for key in [b"N".as_slice(), b"D".as_slice(), b"R".as_slice()] {
                if let Ok(value) = appearance.get(key) {
                    streams.extend(appearance_streams(doc, value, selected.as_deref()));
                }
            }
            // Alles Übrige — ein `/AP` darf weitere Schlüssel tragen, und was
            // in der Datei steht, wird gelesen.
            for (key, value) in appearance.iter() {
                if matches!(key.as_slice(), b"N" | b"D" | b"R") {
                    continue;
                }
                streams.extend(appearance_streams(doc, value, selected.as_deref()));
            }
        }
        // Die Symbole eines Druckknopfs (`/MK /I`, `/RI`, `/IX`, Tabelle 189)
        // sind Form-XObjects wie ein Erscheinungsstrom und dürfen Text
        // zeichnen; ein Betrachter, der die Erscheinung neu aufbaut, malt sie
        // in das `/Rect` des Widgets. Bis zur Spur-A-Runde 1 las sie niemand
        // (Register #71).
        if let Some(mk) = dict
            .get(b"MK")
            .ok()
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| o.as_dict().ok())
        {
            for key in [b"I".as_slice(), b"RI".as_slice(), b"IX".as_slice()] {
                if let Ok(value) = mk.get(key) {
                    streams.extend(appearance_streams(doc, value, None));
                }
            }
        }

        // Ohne Erscheinungsstrom bleibt nur der Klartext in `/Contents` (und
        // den übrigen Textschlüsseln, siehe `crate::meta::ANNOTATION_TEXT_KEYS`)
        // — der hat keine Glyphengeometrie, kann also nicht verortet und
        // deshalb nicht *anteilig* geschwärzt werden. Entfernt wird er
        // trotzdem: `crate::meta::strip_metadata` nimmt jeder Annotation genau
        // diese Schlüssel, als Ganzes. Die Warnung sagt das — und nicht mehr
        // „nicht durchsucht“, was `redact_pipeline::coverage` zu Recht als
        // Deckungslücke (Rückgabewert 3) las, während `--check-leaks` an der
        // Ausgabe nichts fand (Befund G2-7).
        if streams.is_empty() {
            if annot_has_text(doc, dict) {
                sink.warn(
                    "Eine Annotation trägt Text (/Contents oder einen der Schlüssel /RC, /T, \
                     /Subj, /TU, /TM), hat aber keinen lesbaren Erscheinungsstrom (/AP). \
                     Dieser Text hat keine Glyphen und wird deshalb nicht anteilig \
                     geschwärzt; er wird mit den Metadaten als Ganzes entfernt \
                     (strip_metadata, in der Verarbeitungskette immer)."
                        .to_string(),
                );
            }
            continue;
        }
        for id in streams {
            if !seen.insert(id) {
                continue;
            }
            scan_appearance(doc, id, page_id, rect, page_resources, budget, sink);
        }
    }
}

/// Trägt die Annotation überhaupt Text — in einem der Schlüssel, die
/// `strip_metadata` entfernt? Dieselbe Liste an beiden Stellen: was hier
/// gemeldet wird, ist genau das, was dort fällt.
fn annot_has_text(doc: &Document, dict: &Dictionary) -> bool {
    crate::meta::ANNOTATION_TEXT_KEYS.iter().any(|key| {
        dict.get(key)
            .ok()
            .and_then(|o| doc.dereference(o).ok())
            .is_some_and(|(_, o)| match o {
                Object::String(bytes, _) => bytes.iter().any(|b| !b.is_ascii_whitespace()),
                _ => false,
            })
    })
}

/// Objekt-Ids aller Ströme unter einem `/AP`-Eintrag.
///
/// Der Eintrag ist entweder direkt ein Strom oder ein Dictionary von
/// Erscheinungszuständen (`/Off`, `/On`, …). Es werden **alle** Zustände
/// gelesen: was in irgendeinem Zustand steht, steht in der Datei. Jede
/// Checkbox und jedes Radio-Feld bringt so zwei Textflüsse mit, die
/// deckungsgleich auf demselben `/Rect` liegen.
///
/// Sie deshalb wegzulassen wäre falsch — sie *sind* Text, den ein Betrachter
/// unter Umständen zeigt. Sie dürfen sich aber auch nicht gegenseitig
/// unlesbar machen: genau dafür zerlegt [`crate::extract::PdfExtractor`] eine
/// Zeile in Druckschichten. Hier wird nur die Reihenfolge festgelegt — der
/// durch `/AS` benannte, also sichtbare Zustand zuerst.
fn appearance_streams(doc: &Document, value: &Object, selected: Option<&[u8]>) -> Vec<ObjectId> {
    let Ok((id, resolved)) = doc.dereference(value) else {
        return Vec::new();
    };
    match resolved {
        Object::Stream(_) => id.into_iter().collect(),
        Object::Dictionary(states) => {
            let mut out: Vec<ObjectId> = Vec::new();
            let mut rest: Vec<ObjectId> = Vec::new();
            for (name, state) in states.iter() {
                let Ok((Some(id), Object::Stream(_))) = doc.dereference(state) else {
                    continue;
                };
                if selected.is_some_and(|s| s == name.as_slice()) {
                    out.push(id);
                } else {
                    rest.push(id);
                }
            }
            out.append(&mut rest);
            out
        }
        _ => Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn scan_appearance(
    doc: &Document,
    id: ObjectId,
    page_id: ObjectId,
    rect: Option<Rect>,
    page_resources: Option<&Dictionary>,
    budget: &mut Budget,
    sink: &mut dyn ContentSink,
) {
    let Ok(stream) = doc.get_object(id).and_then(|o| o.as_stream()) else {
        return;
    };
    // Ein Erscheinungsstrom ist ein platzierter Strom wie ein Formular; er muss
    // auch so gezählt werden. Zwei Dinge hängen daran, und beide gingen ohne
    // diese Zeile schief:
    //
    // * Die Ressourcenprüfung ([`crate::extract::PdfExtractor`]) hielte ihn für
    //   ungezeichnet, sobald er zusätzlich in einem `/Resources /XObject`
    //   steht — was Erzeuger für Stempel und Logos regelmäßig tun. Der Lauf
    //   meldete dann „sein Text wurde nicht durchsucht“ und setzte
    //   Rückgabewert 3, obwohl er durchsucht *und* geschwärzt wurde. Das ist
    //   die Umkehrung der Wahrheit an genau der Stelle, an der Rückgabewert 3
    //   etwas bedeuten soll.
    // * [`crate::redact::warn_about_shared_form`] feuerte nie für einen
    //   Erscheinungsstrom, den sich zwei Widgets auf zwei Seiten teilen —
    //   obwohl der Modulkopf von `crate::redact` genau das verspricht. Die
    //   zweite Seite ändert sich beim Schwärzen mit; ungesagt ist das eine
    //   Überraschung in einer Datei, die danach weitergegeben wird.
    //
    // Vor den Lesbarkeitsprüfungen: *dass* er platziert ist, steht in der
    // Datei, gleichgültig ob wir ihn lesen konnten. Ließ er sich nicht
    // dekodieren, sagt das die Warnung darunter — und die sagt es richtig.
    sink.form(id);
    // Mehrere Annotationen dürfen sich denselben Erscheinungsstrom teilen;
    // ausgepackt wird er trotzdem nur einmal.
    let appearance = budget.stream(doc, id, stream);
    if !appearance.decodable() {
        sink.warn(format!(
            "Der Erscheinungsstrom einer Annotation (Objekt {} {}) ließ sich nicht \
             dekodieren; sein Text wurde nicht durchsucht und kann deshalb nicht \
             geschwärzt worden sein.",
            id.0, id.1
        ));
        return;
    }
    if appearance.truncated() {
        sink.warn(format!(
            "Ein Teil des Erscheinungsstroms einer Annotation (Objekt {} {}) ließ sich nicht \
             in Operationen zerlegen; ab der Bruchstelle fehlt alles Weitere ({} Byte \
             betroffen). Dieser Text wurde nicht durchsucht und kann deshalb nicht \
             geschwärzt worden sein.",
            id.0,
            id.1,
            appearance.affected_bytes()
        ));
    }
    if appearance.operations().is_empty() {
        return;
    }
    budget.credit(Some(id), appearance.operations().len());

    let matrix = stream
        .dict
        .get(b"Matrix")
        .ok()
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_array().ok())
        .and_then(|a| matrix_from(a))
        .unwrap_or(Matrix::IDENTITY);
    let bbox = stream
        .dict
        .get(b"BBox")
        .ok()
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| annot_rect(o));
    // Bringt der Erscheinungsstrom eigene Ressourcen mit, gelten sie samt
    // seiner schon geladenen Schriften; sonst die der Seite.
    let own_fonts = budget.fonts_of(doc, &appearance);
    let (resources, own_resources, fonts) = match (&appearance.resources, &own_fonts) {
        (Some(own), Some(own_fonts)) => (Some(Rc::clone(own)), true, Some(&**own_fonts)),
        // Die Ressourcen der Seite hat der Seitenstrom schon angeboten.
        _ => (page_resources.map(|d| Rc::new(d.clone())), false, None),
    };
    let owner = if own_resources { id } else { page_id };

    scan_with_budget(
        doc,
        appearance.operations(),
        StreamKey::Form(id),
        resources.as_deref(),
        own_resources,
        Some(owner),
        fonts,
        appearance_matrix(&matrix, bbox, rect),
        budget,
        sink,
    );
}

/// Abbildung des Erscheinungsstroms auf `/Rect` (PDF 32000-1, 12.5.5,
/// Algorithmus 8.1): die mit `/Matrix` transformierte `/BBox` wird auf das
/// Annotationsrechteck geschoben und skaliert.
fn appearance_matrix(matrix: &Matrix, bbox: Option<Rect>, rect: Option<Rect>) -> Matrix {
    let (Some(bbox), Some(rect)) = (bbox, rect) else {
        return *matrix;
    };
    let corners = [
        matrix.apply(bbox.ll.x, bbox.ll.y),
        matrix.apply(bbox.ur.x, bbox.ll.y),
        matrix.apply(bbox.ur.x, bbox.ur.y),
        matrix.apply(bbox.ll.x, bbox.ur.y),
    ];
    let mut min = Point::new(f64::MAX, f64::MAX);
    let mut max = Point::new(f64::MIN, f64::MIN);
    for c in corners {
        min.x = min.x.min(c.x);
        min.y = min.y.min(c.y);
        max.x = max.x.max(c.x);
        max.y = max.y.max(c.y);
    }
    // Entartete Kästen werden nur verschoben, nicht skaliert.
    let sx = if max.x - min.x > 1e-9 {
        rect.width() / (max.x - min.x)
    } else {
        1.0
    };
    let sy = if max.y - min.y > 1e-9 {
        rect.height() / (max.y - min.y)
    } else {
        1.0
    };
    let fit = Matrix::translate(-min.x, -min.y)
        .mul(&Matrix::scale(sx, sy))
        .mul(&Matrix::translate(rect.ll.x, rect.ll.y));
    matrix.mul(&fit)
}

/// Rechteck aus einem PDF-Array `[x0 y0 x1 y1]`; die Ecken werden normalisiert.
fn annot_rect(obj: &Object) -> Option<Rect> {
    let array = obj.as_array().ok()?;
    let v: Vec<f64> = array.iter().take(4).filter_map(as_f64).collect();
    if v.len() < 4 {
        return None;
    }
    Some(Rect::new(v[0], v[1], v[2], v[3]))
}

fn merge_resources(target: &mut Dictionary, source: &Dictionary) {
    for (key, value) in source.iter() {
        match (
            target.get(key).ok().and_then(|o| o.as_dict().ok()).cloned(),
            value.as_dict().ok(),
        ) {
            (Some(mut existing), Some(incoming)) => {
                for (k, v) in incoming.iter() {
                    existing.set(k.to_vec(), v.clone());
                }
                target.set(key.to_vec(), Object::Dictionary(existing));
            }
            _ => target.set(key.to_vec(), value.clone()),
        }
    }
}

// ---------------------------------------------------------------------------
// Marked Content: Textspiegel
// ---------------------------------------------------------------------------

/// Sammelt die Textspiegel eines Stroms und ordnet jedem die Textoperationen
/// zu, die er wiedergibt.
///
/// **Warum ein eigener kleiner Durchlauf und nicht die Zustandsschleife?**
/// Welche Operation in welcher Marked-Content-Klammer steht, ist rein
/// syntaktisch — es hängt weder an CTM noch an `Tm`, Font oder Farbe. In
/// [`scan_operations`] müsste die Klammerstruktur trotzdem neben `q`/`Q`, der
/// Form-Rekursion und dem Textzustand mitgeführt werden, obwohl sie mit alldem
/// nichts zu tun hat. Hier steht sie an einer Stelle und ist Zeile für Zeile
/// nachlesbar. Auseinanderlaufen können die beiden nicht: verglichen werden am
/// Ende nur Operationsindizes, und die vergibt in beiden Fällen dieselbe
/// Aufzählung über dasselbe `operations`.
///
/// **Geltungsbereich.**
/// * `BDC … EMC` ist eine Klammer: der Bereich sind die Textoperationen
///   dazwischen. Eine nicht geschlossene Klammer reicht bis zum Stromende — die
///   sichere Richtung.
/// * `DP` ist ein *Punkt* ohne Klammer und hat damit keine eigenen Glyphen.
///   Genommen wird deshalb der nächstliegende Bereich, der einer ist: die
///   umschließende Marked-Content-Klammer, sonst das umschließende Textobjekt
///   (`BT … ET`), sonst der ganze Strom. Ein Punkt beschreibt die Stelle, an
///   der er steht; wird dort etwas entfernt, ist auch seine Beschreibung
///   falsch.
/// * Ein `Do` im Bereich zieht das **Form-XObject** dahinter mit hinein
///   ([`MarkedTextRecord::forms`]): dessen Glyphen stehen in einem anderen
///   Strom, gehören aber zu diesem Spiegel. Aufgelöst wird nur die Objekt-Id
///   — kein Rumpf, kein Aufwandskonto; ob das Formular lesbar ist, entscheidet
///   [`load_xobject`] an derselben Stelle, und ein unlesbares hat keine
///   Glyphen, die hier fehlen könnten.
#[allow(clippy::too_many_arguments)]
fn scan_marked_text(
    doc: &Document,
    operations: &[Operation],
    stream: StreamKey,
    resources: Option<&Dictionary>,
    owner: Option<ObjectId>,
    budget: &mut Budget,
    sink: &mut dyn ContentSink,
) {
    let shows: Vec<usize> = operations
        .iter()
        .enumerate()
        .filter(|(_, op)| matches!(op.operator.as_str(), "Tj" | "TJ" | "'" | "\""))
        .map(|(index, _)| index)
        .collect();
    // Die Ströme, deren Glyphen unter einem Spiegel dieses Stroms stehen
    // können: Form-XObjects am `Do` — und **Kachelmuster** am `scn`. Ein
    // Muster ist ein eigener Strom mit eigenem Text; wird es unter
    // `/Span <</ActualText …>> BDC` als Füllung gesetzt, beschreibt der
    // Spiegel dessen Glyphen. Bis zur Spur-A-Runde 1 stand hier nur das
    // `Do`: die Glyphen fielen aus dem Musterstrom, der Spiegel im Seitenstrom
    // blieb stehen — `--check-leaks` fand ihn (Register #66).
    let dos: Vec<(usize, ObjectId)> = operations
        .iter()
        .enumerate()
        .filter_map(|(index, op)| match op.operator.as_str() {
            "Do" => match op.operands.first() {
                Some(Object::Name(name)) => Some((index, form_id_of(doc, resources, name)?)),
                _ => None,
            },
            "sc" | "scn" | "SC" | "SCN" => op.operands.iter().find_map(|o| match o {
                Object::Name(name) => {
                    tiling_pattern_id_of(doc, resources, name).map(|id| (index, id))
                }
                _ => None,
            }),
            _ => None,
        })
        .collect();

    // Offene Klammern bzw. das offene Textobjekt, jeweils als Operationsindex.
    let mut open: Vec<usize> = Vec::new();
    let mut text_object: Option<usize> = None;
    // Klammer/Textobjekt → Bereich der eingeschlossenen Operationen.
    let mut ranges: BTreeMap<usize, std::ops::Range<usize>> = BTreeMap::new();
    // Gefundene Spiegel an Klammern und an Punkten; ein Punkt merkt sich
    // zusätzlich, worin er steht.
    // Operationsindex, Liste, Objekt-Id der Liste, Ressourcenname der Liste.
    type BracketMirror = (usize, Dictionary, Option<ObjectId>, Option<Vec<u8>>);
    let mut brackets: Vec<BracketMirror> = Vec::new();
    // Dasselbe, plus umschließende Klammer und umschließendes Textobjekt.
    type PointMirror = (
        usize,
        Dictionary,
        Option<ObjectId>,
        Option<Vec<u8>>,
        Option<usize>,
        Option<usize>,
    );
    let mut points: Vec<PointMirror> = Vec::new();

    for (index, op) in operations.iter().enumerate() {
        match op.operator.as_str() {
            "BDC" | "BMC" => {
                open.push(index);
                if let Some((list, id, name)) = mirror_property_list(doc, resources, &op.operands) {
                    brackets.push((index, list, id, name));
                }
            }
            "EMC" => {
                if let Some(start) = open.pop() {
                    ranges.insert(start, start + 1..index);
                }
            }
            "BT" => text_object = Some(index),
            "ET" => {
                if let Some(start) = text_object.take() {
                    ranges.insert(start, start + 1..index);
                }
            }
            "DP" => {
                if let Some((list, id, name)) = mirror_property_list(doc, resources, &op.operands) {
                    points.push((index, list, id, name, open.last().copied(), text_object));
                }
            }
            _ => {}
        }
    }
    // Unabgeschlossen: bis zum Ende des Stroms.
    for start in open.into_iter().chain(text_object) {
        ranges.insert(start, start + 1..operations.len());
    }

    // `shows` und `dos` stehen nach Operationsindex sortiert; der Bereich einer
    // Klammer ist darin ein zusammenhängendes Stück. Die binäre Suche findet
    // es in O(log n) — die frühere Filterung ging für **jede** Klammer die
    // ganze Liste ab, und das ist bei verschachtelten Klammern das Produkt aus
    // beidem.
    let slice_in = |list: &[usize], range: &std::ops::Range<usize>| -> (usize, usize) {
        (
            list.partition_point(|index| *index < range.start),
            list.partition_point(|index| *index < range.end),
        )
    };
    // Dieselbe Decke, dieselbe Bauart wie unten bei `forms_in`: `B`
    // verschachtelte Klammern über `S` Textoperationen ergeben `B × S`
    // Einträge, und auch die entstehen alle hier — vor jeder Schleife, die das
    // Aufwandskonto belastet. Bis Fix-Runde 7 stand davor nichts (gemessen:
    // 6 000 × 6 000 aus 263 kB = 36 Mio. Zuordnungen, Extraktor 31,1 s).
    let in_range = |budget: &mut Budget, range: &std::ops::Range<usize>| -> Vec<usize> {
        let (from, to) = slice_in(&shows, range);
        let granted = budget.mirror_shows(to - from);
        shows[from..from + granted].to_vec()
    };
    let do_indices: Vec<usize> = dos.iter().map(|(index, _)| *index).collect();
    // Die Decke zählt hier mit: `B` verschachtelte Klammern über `D`
    // Platzierungen ergeben `B × D` Paare, und die entstehen alle an dieser
    // Stelle — vor jeder Schleife, die das Aufwandskonto belastet, und bis
    // Fix-Runde 6 unter keiner Schranke (gemessen: 2 306 MB und 41,7 s aus
    // einer Datei von 276 kB).
    let forms_in =
        |budget: &mut Budget, range: &std::ops::Range<usize>| -> Vec<(Vec<usize>, ObjectId)> {
            let (from, to) = slice_in(&do_indices, range);
            let granted = budget.mirror_pairs(to - from);
            dos[from..from + granted]
                .iter()
                .map(|(index, id)| (vec![*index], *id))
                .collect()
        };

    // Eine Klammer bringt ihren Bereich selbst mit.
    for (op_index, properties, property_id, property_name) in brackets {
        let range = ranges
            .get(&op_index)
            .cloned()
            .unwrap_or(op_index + 1..operations.len());
        sink.marked(MarkedTextRecord {
            stream,
            op_index,
            properties,
            property_id,
            property_name,
            property_owner: owner,
            shows: in_range(budget, &range),
            forms: forms_in(budget, &range),
            range,
        });
    }
    // Ein Punkt erbt den Bereich, in dem er steht.
    for (op_index, properties, property_id, property_name, bracket, text_object) in points {
        let range = bracket
            .or(text_object)
            .and_then(|start| ranges.get(&start).cloned())
            .unwrap_or(0..operations.len());
        sink.marked(MarkedTextRecord {
            stream,
            op_index,
            properties,
            property_id,
            property_name,
            property_owner: owner,
            shows: in_range(budget, &range),
            forms: forms_in(budget, &range),
            range,
        });
    }
}

/// Die Objekt-Id des Form-XObjects hinter einem `Do`-Namen — nur die Id.
///
/// Der Rumpf bleibt unangetastet: [`load_xobject`] holt ihn ohnehin, mit
/// Aufwandskonto und Warnung. Hier zählt nur, *welches* Objekt in den
/// Geltungsbereich eines Spiegels fällt. `None` für Bilder, fehlende Einträge
/// und alles, was kein eigenständiger Strom ist — ein Formular ohne eigene
/// Objekt-Id liest auch der Interpreter nicht.
fn form_id_of(doc: &Document, resources: Option<&Dictionary>, name: &[u8]) -> Option<ObjectId> {
    let entry = resources?.get(b"XObject").ok()?;
    let (_, xobjects) = doc.dereference(entry).ok()?;
    let (id, object) = doc
        .dereference(xobjects.as_dict().ok()?.get(name).ok()?)
        .ok()?;
    let stream = object.as_stream().ok()?;
    (crate::ops::xobject_subtype(doc, &stream.dict) == Some(b"Form".as_slice())).then_some(id?)
}

/// Die Objekt-Id des **Kachelmusters** hinter einem `scn`-Namen.
///
/// `None` für ein Schattierungsmuster (`/PatternType 2` — ein Dictionary ohne
/// Strom, dort steht kein Text), für einen fehlenden Eintrag und für ein
/// Muster, das kein eigenes Objekt ist (dann liest es
/// [`scan_tiling_pattern`] auch nicht).
fn tiling_pattern_id_of(
    doc: &Document,
    resources: Option<&Dictionary>,
    name: &[u8],
) -> Option<ObjectId> {
    let entry = resources?.get(b"Pattern").ok()?;
    let (_, patterns) = doc.dereference(entry).ok()?;
    let (id, object) = doc
        .dereference(patterns.as_dict().ok()?.get(name).ok()?)
        .ok()?;
    let stream = object.as_stream().ok()?;
    if stream.dict.get(b"PatternType").ok().and_then(as_f64) == Some(2.0) {
        return None;
    }
    id
}

/// Die Objekt-Id des **Bild**-XObjects hinter einem `Do`-Namen.
///
/// Gegenstück zu [`form_id_of`], dieselbe Auflösung, andere `/Subtype`-Probe.
/// `None` für ein Inline-Bild (es hat kein eigenes Objekt), für einen
/// fehlenden Eintrag und für alles, was kein Bild ist.
fn image_id_of(doc: &Document, resources: Option<&Dictionary>, name: &[u8]) -> Option<ObjectId> {
    let entry = resources?.get(b"XObject").ok()?;
    let (_, xobjects) = doc.dereference(entry).ok()?;
    let (id, object) = doc
        .dereference(xobjects.as_dict().ok()?.get(name).ok()?)
        .ok()?;
    let stream = object.as_stream().ok()?;
    (crate::ops::xobject_subtype(doc, &stream.dict) == Some(b"Image".as_slice())).then_some(id?)
}

/// Die Hülle des transformierten Einheitsquadrats.
///
/// Ein Bild wird immer in `(0,0)-(1,1)` gezeichnet und von der CTM auf seine
/// Zielfläche gebracht (PDF 32000-1, 8.9.5.2). Genommen werden alle **vier**
/// Ecken, nicht zwei: bei einer gedrehten oder gescherten CTM sind die beiden
/// anderen die äußeren.
fn unit_square_bounds(ctm: &Matrix) -> Rect {
    let corners = unit_square_quad(ctm);
    let mut bounds = Rect::from_corners(corners[0], corners[1]);
    for corner in &corners[2..] {
        bounds = bounds.union(&Rect::from_corners(*corner, *corner));
    }
    bounds
}

/// Die vier Ecken des transformierten Einheitsquadrats, **im Umlauf**.
///
/// Die Reihenfolge ist `(0,0) (1,0) (1,1) (0,1)` und nicht die Aufzählung des
/// Quadrats: [`ImagePlacement::covers`] läuft die Kanten des Vierecks ab, und
/// eine über Kreuz verbundene Ecke ergäbe kein Viereck.
fn unit_square_quad(ctm: &Matrix) -> [Point; 4] {
    [
        ctm.apply(0.0, 0.0),
        ctm.apply(1.0, 0.0),
        ctm.apply(1.0, 1.0),
        ctm.apply(0.0, 1.0),
    ]
}

/// Trägt die Eigenschaftsliste eines `BDC`/`DP` einen Textspiegel?
///
/// Liefert die aufgelöste Liste und — falls sie ein eigenes Objekt ist — deren
/// Objekt-Id. `None`, wenn kein Textschlüssel darin steht: dann gibt es nichts
/// zu tun, und ein `/MCID`- oder `/OC`-Eintrag bleibt unangetastet.
fn mirror_property_list(
    doc: &Document,
    resources: Option<&Dictionary>,
    operands: &[Object],
) -> Option<(Dictionary, Option<ObjectId>, Option<Vec<u8>>)> {
    match operands.get(1)? {
        // `/Span <</ActualText (…)>> BDC`
        Object::Dictionary(dict) => has_mirror_key(doc, dict).then(|| (dict.clone(), None, None)),
        // `/Span /MC0 BDC` — die Liste steht in `/Resources /Properties`.
        Object::Name(name) => {
            let entry = resources
                .and_then(|r| r.get(b"Properties").ok())
                .and_then(|o| doc.dereference(o).ok())
                .and_then(|(_, o)| o.as_dict().ok())
                .and_then(|d| d.get(name.as_slice()).ok())?;
            let (id, resolved) = doc.dereference(entry).ok()?;
            let dict = resolved.as_dict().ok()?;
            has_mirror_key(doc, dict).then(|| (dict.clone(), id, Some(name.clone())))
        }
        _ => None,
    }
}

/// Wo eine **direkt** stehende Eigenschaftsliste in der Datei zu finden ist.
///
/// Nicht jede Eigenschaftsliste ist ein eigenes Objekt. Steht sie direkt in
/// einem `/Properties`-Dictionary, hat sie keine Objekt-Id, und der Spiegel
/// darin ist trotzdem Klartext in der Datei — an genau einer Stelle, die sich
/// benennen lässt: das Objekt, in dem sie steckt, und der Weg dorthin.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct MirrorHome {
    /// Das Objekt, das die Liste (mittelbar) enthält.
    pub(crate) object: ObjectId,
    /// Der Weg von diesem Objekt zur Liste, Schlüssel für Schlüssel — etwa
    /// `[/Resources, /Properties, /MC0]` oder nur `[/MC0]`.
    pub(crate) path: Vec<Vec<u8>>,
}

/// Die Ressourcenverzeichnisse im Geltungsbereich von `owner`, von innen nach
/// außen — je mit dem Objekt, in dem sie stecken, und dem Weg dorthin.
///
/// `owner` ist die Seite oder das Form-XObject. Für eine Seite läuft die
/// Vererbungskette `/Parent` mit, denn `/Resources` darf am `/Pages`-Knoten
/// stehen (PDF 32000-1, Tabelle 30) — dieselbe Kette wie in
/// [`page_resources`], nur behält diese hier die Objekt-Ids.
fn resource_homes(doc: &Document, owner: ObjectId) -> Vec<(ObjectId, Vec<Vec<u8>>)> {
    let mut out = Vec::new();
    let mut seen: HashSet<ObjectId> = HashSet::new();
    let mut current = Some(owner);
    while let Some(id) = current {
        if seen.len() >= MAX_PAGE_TREE_DEPTH || !seen.insert(id) {
            break;
        }
        let node = match doc.get_object(id) {
            Ok(Object::Dictionary(dict)) => dict,
            // Ein Form-XObject: sein `/Resources` steht im Stromdictionary.
            Ok(Object::Stream(stream)) => &stream.dict,
            _ => break,
        };
        match node.get(b"Resources") {
            Ok(Object::Reference(target)) => out.push((*target, Vec::new())),
            Ok(Object::Dictionary(_)) => out.push((id, vec![b"Resources".to_vec()])),
            _ => {}
        }
        current = match node.get(b"Parent") {
            Ok(Object::Reference(parent)) => Some(*parent),
            _ => None,
        };
    }
    out
}

/// Folgt einem Weg aus **direkten** Schlüsseln innerhalb eines Objekts.
fn dict_at<'a>(doc: &'a Document, object: ObjectId, path: &[Vec<u8>]) -> Option<&'a Dictionary> {
    let mut dict = match doc.get_object(object).ok()? {
        Object::Dictionary(dict) => dict,
        Object::Stream(stream) => &stream.dict,
        _ => return None,
    };
    for key in path {
        dict = dict.get(key).ok()?.as_dict().ok()?;
    }
    Some(dict)
}

/// **Alle** Stellen, an denen die über `/Resources /Properties /<name>`
/// erreichbare Eigenschaftsliste von `owner` in der Datei steht — für den
/// Fall, dass sie **kein eigenes Objekt** ist.
///
/// Leer heißt: es gibt nichts im Dokument zu bereinigen (die Liste ist überall
/// ein eigenes Objekt, oder der Name löst sich gar nicht auf).
///
/// Gesucht wird die ganze Kette von innen nach außen, in der Reihenfolge, in
/// der [`merge_resources`] die Verzeichnisse übereinanderlegt. Das innerste,
/// das den Namen führt, ist das **wirksame** — aber nicht das einzige, in dem
/// der Klartext steht. Steht `/Properties /MC0` zugleich in den
/// Seitenressourcen und noch einmal im `/Pages`-Knoten darüber, dann trägt die
/// überschattete äußere Kopie dieselbe Zeichenkette; sie nur deshalb stehen zu
/// lassen, weil kein Betrachter sie benutzt, hieße das Geheimnis in der Datei
/// zu lassen. `leaks` sucht Bytes, nicht Wirksamkeit — und fand sie bis
/// Fix-Runde 7 ohne Warnung (Befund R1-5). Geräumt wird deshalb **jeder**
/// Fundort der Kette.
///
/// **Geteilte Verzeichnisse.** Trifft der Weg ein `/Properties`- oder
/// `/Resources`-Objekt, das mehrere Seiten oder Formulare benutzen, wirkt die
/// Bereinigung auf alle. Das ist dieselbe Richtung, in die schon eine geteilte
/// Liste mit eigener Objekt-Id wirkt (siehe `crate::redact::mirrors_to_clear`):
/// ein Spiegel, der zu viel verliert, kostet die Vorlesefunktion; einer, der
/// stehen bleibt, kostet das Geheimnis. Dieselbe Abwägung gilt für die
/// überschattete Kopie.
pub(crate) fn property_list_homes(doc: &Document, owner: ObjectId, name: &[u8]) -> Vec<MirrorHome> {
    let mut out = Vec::new();
    for (object, prefix) in resource_homes(doc, owner) {
        let Some(resources) = dict_at(doc, object, &prefix) else {
            continue;
        };
        match resources.get(b"Properties") {
            // Eigenes `/Properties`-Objekt: die Liste steht darin.
            Ok(Object::Reference(properties)) => {
                // Ein Verweis auf die Liste selbst: sie ist ein eigenes Objekt
                // und wird dort bereinigt (`property_id`). Nicht vorhanden:
                // hier nichts zu tun.
                if let Ok(Ok(Object::Dictionary(_))) =
                    doc.get_dictionary(*properties).map(|d| d.get(name))
                {
                    out.push(MirrorHome {
                        object: *properties,
                        path: vec![name.to_vec()],
                    });
                }
            }
            // Direkt im Verzeichnis: dann steht die Liste im Objekt, das
            // dieses Verzeichnis trägt.
            Ok(Object::Dictionary(properties)) => {
                if let Ok(Object::Dictionary(_)) = properties.get(name) {
                    let mut path = prefix;
                    path.push(b"Properties".to_vec());
                    path.push(name.to_vec());
                    out.push(MirrorHome { object, path });
                }
            }
            _ => continue,
        }
    }
    out
}

/// Steht in dieser Liste überhaupt ein nicht leerer Textspiegel?
///
/// Aufgelöst wird auch ein indirekter Verweis: eine Eigenschaftsliste, die als
/// eigenes Objekt in der Datei steht, darf ihren `/ActualText` seinerseits als
/// Verweis führen. Wer hier nur auf `Object::String` prüfte, hielte genau diese
/// Datei für unauffällig.
fn has_mirror_key(doc: &Document, dict: &Dictionary) -> bool {
    MIRROR_KEYS.iter().any(|key| {
        matches!(
            dict.get(key).ok().and_then(|o| doc.dereference(o).ok()),
            Some((_, Object::String(bytes, _))) if bytes.iter().any(|b| !b.is_ascii_whitespace())
        )
    })
}

/// Meldet der Senke jedes Form-XObject, das in diesem Ressourcenverzeichnis
/// **steht** — unabhängig davon, ob der Strom es je zeichnet.
///
/// Nur die Objekt-Id wird angefasst, nicht der Strom: die Prüfung, ob dort
/// überhaupt Text steht, wäre hier zu teuer (sie liefe für jedes Formular auf
/// jeder Ebene) und wird erst fällig, wenn feststeht, dass niemand es
/// gezeichnet hat.
///
/// **Einmal je Verzeichnis**, nicht je Strom — siehe [`DeclarationKey`]. Die
/// `/XObject`-Liste wird dafür ohnehin aufgelöst, und dabei fällt ihre
/// Objekt-Id an; die Sperre kostet also nichts extra.
fn declare_forms(
    doc: &Document,
    resources: Option<&Dictionary>,
    stream: StreamKey,
    budget: &mut Budget,
    sink: &mut dyn ContentSink,
) {
    let Some((id, object)) = resources
        .and_then(|r| r.get(b"XObject").ok())
        .and_then(|o| doc.dereference(o).ok())
    else {
        return;
    };
    let Ok(xobjects) = object.as_dict() else {
        return;
    };
    let key = match id {
        Some(id) => DeclarationKey::XObjects(id),
        None => DeclarationKey::InStream(stream),
    };
    if !budget.first_declaration(key) {
        return;
    }
    budget.effort.declared_resources += 1;
    for (name, value) in xobjects.iter() {
        let Ok((Some(id), Object::Stream(stream))) = doc.dereference(value) else {
            continue;
        };
        if stream.dict.get(b"Subtype").and_then(|o| o.as_name()).ok() == Some(b"Form".as_slice()) {
            sink.declares_form(id, name);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn scan_operations(
    doc: &Document,
    operations: &[Operation],
    stream: StreamKey,
    resources: Option<&Dictionary>,
    // `own_resources`: bringt **dieser** Strom das `/Resources` selbst mit?
    // `false` heißt, es ist unverändert das des Aufrufers — dann hat der
    // Aufrufer seine `/XObject`-Liste bereits angeboten, und sie ein zweites
    // Mal durchzugehen wäre genau die Vervielfachung, gegen die
    // [`DeclarationKey`] steht.
    own_resources: bool,
    // Wem `resources` gehört: die Seite, oder der Strom, der sie selbst
    // mitbringt. Läuft mit `own_resources` mit und steht am Ende in
    // [`MarkedTextRecord::property_owner`] — dort wird aus ihm der Fundort
    // einer Eigenschaftsliste, die kein eigenes Objekt ist.
    owner: Option<ObjectId>,
    fonts: &FontMap,
    initial_ctm: Matrix,
    depth: usize,
    visiting: &mut HashSet<ObjectId>,
    stats: &mut FontDecodeStats,
    budget: &mut Budget,
    sink: &mut dyn ContentSink,
) {
    // Einmal je Strom, und **vor** der Schleife darunter gebucht. Der
    // Durchlauf geht den ganzen Strom ab; er lief bisher bei jeder Platzierung
    // desselben Formulars erneut, ganz außerhalb des Aufwandskontos, und ein
    // aufgebrauchtes Konto hielt ihn nicht auf — die erste Zeile der Schleife
    // kommt erst danach. Dass einmal genügt, liegt an ihm selbst: er ist rein
    // syntaktisch (siehe [`scan_marked_text`]) und liefert jedes Mal dieselben
    // Abschnitte, die die Senke dann verwirft.
    if budget.first_marked_scan(stream, owner) {
        if !budget.operation() {
            return;
        }
        scan_marked_text(doc, operations, stream, resources, owner, budget, sink);
    }
    // Einmal je Verzeichnis, nicht einmal je Platzierung und nicht einmal je
    // Strom: ein zwanzigmal gezeichnetes Formular bietet zwanzigmal dieselben
    // Ressourcen an, und zwanzig Formulare, die sich dasselbe Verzeichnis
    // teilen, ebenfalls.
    if own_resources {
        declare_forms(doc, resources, stream, budget, sink);
    }
    let graphics = sink.wants_graphics();
    // Bilder können auch ohne den vollen Grafikzustand gebraucht werden —
    // siehe [`ContentSink::wants_images`].
    let images = graphics || sink.wants_images();
    let mut state = GraphicsState::new(initial_ctm);
    let mut stack: Vec<GraphicsState> = Vec::new();
    // Textmatrix und Zeilenmatrix
    let mut tm = Matrix::IDENTITY;
    let mut tlm = Matrix::IDENTITY;
    // Pfadaufbau: alle Punkte werden sofort in den User-Space gerechnet.
    let mut path: Vec<PathSeg> = Vec::new();
    let mut current = Point::new(0.0, 0.0);
    let mut subpath_start = Point::new(0.0, 0.0);
    // `W`/`W*` merken sich nur die Absicht; wirksam wird der Clip erst nach
    // der folgenden Maloperation.
    let mut pending_clip: Option<bool> = None;

    for (op_index, op) in operations.iter().enumerate() {
        // Jede Interpretation kostet — auch die im hundertsten Durchlauf
        // desselben Form-XObjects. Ohne diese Zeile begrenzt
        // [`MAX_FORM_DEPTH`] nur die Tiefe, und die Breite bleibt frei.
        if !budget.operation() {
            return;
        }
        let cx = SinkContext {
            doc,
            resources,
            stream,
            op_index,
        };
        match op.operator.as_str() {
            "q" => stack.push(state.clone()),
            "Q" => {
                if let Some(prev) = stack.pop() {
                    state = prev;
                }
            }
            "cm" => {
                if let Some(m) = matrix_from(&op.operands) {
                    state.ctm = m.mul(&state.ctm);
                }
            }
            "BT" => {
                tm = Matrix::IDENTITY;
                tlm = Matrix::IDENTITY;
            }
            "ET" => {}
            "Tf" => {
                if let Some(Object::Name(name)) = op.operands.first() {
                    state.text.font = fonts.get(name.as_slice()).cloned();
                    state.text.font_name = name.clone();
                }
                state.text.font_size = op.operands.get(1).and_then(as_f64).unwrap_or(0.0);
            }
            "Tr" => {
                state.text.render_mode =
                    op.operands.first().and_then(as_f64).unwrap_or(0.0).max(0.0) as u8
            }
            "Tc" => state.text.char_spacing = op.operands.first().and_then(as_f64).unwrap_or(0.0),
            "Tw" => state.text.word_spacing = op.operands.first().and_then(as_f64).unwrap_or(0.0),
            "Tz" => {
                state.text.h_scale = op.operands.first().and_then(as_f64).unwrap_or(100.0) / 100.0
            }
            "TL" => state.text.leading = op.operands.first().and_then(as_f64).unwrap_or(0.0),
            "Ts" => state.text.rise = op.operands.first().and_then(as_f64).unwrap_or(0.0),
            "Td" => {
                let tx = op.operands.first().and_then(as_f64).unwrap_or(0.0);
                let ty = op.operands.get(1).and_then(as_f64).unwrap_or(0.0);
                tlm = Matrix::translate(tx, ty).mul(&tlm);
                tm = tlm;
            }
            "TD" => {
                let tx = op.operands.first().and_then(as_f64).unwrap_or(0.0);
                let ty = op.operands.get(1).and_then(as_f64).unwrap_or(0.0);
                state.text.leading = -ty;
                tlm = Matrix::translate(tx, ty).mul(&tlm);
                tm = tlm;
            }
            "Tm" => {
                if let Some(m) = matrix_from(&op.operands) {
                    tlm = m;
                    tm = m;
                }
            }
            "T*" => {
                tlm = Matrix::translate(0.0, -state.text.leading).mul(&tlm);
                tm = tlm;
            }
            "Tj" | "TJ" | "'" | "\"" => {
                // Die Operatoren ' und " beginnen eine neue Zeile.
                if op.operator == "'" || op.operator == "\"" {
                    if op.operator == "\"" {
                        state.text.word_spacing = op
                            .operands
                            .first()
                            .and_then(as_f64)
                            .unwrap_or(state.text.word_spacing);
                        state.text.char_spacing = op
                            .operands
                            .get(1)
                            .and_then(as_f64)
                            .unwrap_or(state.text.char_spacing);
                    }
                    tlm = Matrix::translate(0.0, -state.text.leading).mul(&tlm);
                    tm = tlm;
                }
                let record = show_text(
                    &op.operands,
                    &state,
                    &mut tm,
                    &cx,
                    &op.operator,
                    stats,
                    budget,
                    sink,
                    graphics,
                );
                if let Some(record) = record {
                    // Eine Type3-Glyphe *ist* ein Content-Stream. Ob darin
                    // Text steht, entscheidet sich erst, wenn sie wirklich
                    // gesetzt wird — deshalb hier und nicht schon beim `Tf`.
                    warn_about_text_in_charprocs(
                        doc,
                        resources,
                        stream,
                        &state.text.font_name,
                        budget,
                        sink,
                    );
                    sink.show(record);
                }
            }
            // --- Pfadaufbau -------------------------------------------------
            "m" if graphics => {
                if let Some(p) = point_at(&op.operands, 0, &state.ctm) {
                    current = p;
                    subpath_start = p;
                    path.push(PathSeg::MoveTo(p));
                }
            }
            "l" if graphics => {
                if let Some(p) = point_at(&op.operands, 0, &state.ctm) {
                    current = p;
                    path.push(PathSeg::LineTo(p));
                }
            }
            "c" if graphics => {
                if let (Some(p1), Some(p2), Some(p3)) = (
                    point_at(&op.operands, 0, &state.ctm),
                    point_at(&op.operands, 2, &state.ctm),
                    point_at(&op.operands, 4, &state.ctm),
                ) {
                    current = p3;
                    path.push(PathSeg::CubicTo(p1, p2, p3));
                }
            }
            "v" if graphics => {
                // Erster Kontrollpunkt ist der aktuelle Punkt.
                if let (Some(p2), Some(p3)) = (
                    point_at(&op.operands, 0, &state.ctm),
                    point_at(&op.operands, 2, &state.ctm),
                ) {
                    path.push(PathSeg::CubicTo(current, p2, p3));
                    current = p3;
                }
            }
            "y" if graphics => {
                // Zweiter Kontrollpunkt ist der Endpunkt.
                if let (Some(p1), Some(p3)) = (
                    point_at(&op.operands, 0, &state.ctm),
                    point_at(&op.operands, 2, &state.ctm),
                ) {
                    path.push(PathSeg::CubicTo(p1, p3, p3));
                    current = p3;
                }
            }
            "h" if graphics => {
                path.push(PathSeg::Close);
                current = subpath_start;
            }
            "re" if graphics => {
                if let Some(rect) = rect_path(&op.operands, &state.ctm) {
                    subpath_start = match rect[0] {
                        PathSeg::MoveTo(p) => p,
                        _ => subpath_start,
                    };
                    current = subpath_start;
                    path.extend(rect);
                }
            }
            // --- Clipping ---------------------------------------------------
            "W" if graphics => pending_clip = Some(false),
            "W*" if graphics => pending_clip = Some(true),
            // --- Malen ------------------------------------------------------
            "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "n" if graphics => {
                let operator = op.operator.as_str();
                if matches!(operator, "s" | "b" | "b*") {
                    path.push(PathSeg::Close);
                }
                let fills = matches!(operator, "f" | "F" | "f*" | "B" | "B*" | "b" | "b*");
                let strokes = matches!(operator, "S" | "s" | "B" | "B*" | "b" | "b*");
                let even_odd = matches!(operator, "f*" | "B*" | "b*");
                if !path.is_empty() && (fills || strokes) {
                    sink.path(
                        &cx,
                        &PathEvent {
                            segments: &path,
                            fill: fills.then_some(state.fill),
                            stroke: strokes.then(|| state.stroke_style()),
                            even_odd,
                            fill_alpha: state.fill_alpha,
                            clip: state.clip,
                        },
                    );
                }
                // Der Clip gilt erst *nach* dieser Operation.
                if let Some(clip_even_odd) = pending_clip.take() {
                    if !path.is_empty() {
                        state.clip = sink.clip(&cx, &path, clip_even_odd);
                    }
                }
                path.clear();
            }
            // --- Farbe ------------------------------------------------------
            "g" | "G" if graphics => {
                let v = op.operands.first().and_then(as_f64).unwrap_or(0.0);
                set_color(&mut state, &op.operator, ColorSpace::Gray, Rgb::gray(v));
            }
            "rg" | "RG" if graphics => {
                let rgb = Rgb::new(
                    op.operands.first().and_then(as_f64).unwrap_or(0.0),
                    op.operands.get(1).and_then(as_f64).unwrap_or(0.0),
                    op.operands.get(2).and_then(as_f64).unwrap_or(0.0),
                );
                set_color(&mut state, &op.operator, ColorSpace::Rgb, rgb);
            }
            "k" | "K" if graphics => {
                let rgb = cmyk_to_rgb(
                    op.operands.first().and_then(as_f64).unwrap_or(0.0),
                    op.operands.get(1).and_then(as_f64).unwrap_or(0.0),
                    op.operands.get(2).and_then(as_f64).unwrap_or(0.0),
                    op.operands.get(3).and_then(as_f64).unwrap_or(0.0),
                );
                set_color(&mut state, &op.operator, ColorSpace::Cmyk, rgb);
            }
            "cs" | "CS" if graphics => {
                let space = op
                    .operands
                    .first()
                    .map(|o| ColorSpace::resolve(doc, resources, o))
                    .unwrap_or(ColorSpace::Unknown);
                let color = space.initial_color();
                set_color(&mut state, &op.operator, space, color);
            }
            "sc" | "scn" | "SC" | "SCN" => {
                // Ein Namensoperand benennt ein Muster. Kachelmuster sind
                // eigene Content-Streams — dort kann Text stehen, den sonst
                // niemand zu Gesicht bekommt. Das gilt auch für die reine
                // Textextraktion, deshalb steht dieser Zweig **vor** der
                // Grafikschranke.
                if let Some(Object::Name(pattern)) =
                    op.operands.iter().find(|o| matches!(o, Object::Name(_)))
                {
                    scan_tiling_pattern(
                        doc,
                        stream,
                        op_index,
                        resources,
                        owner,
                        fonts,
                        pattern,
                        initial_ctm,
                        depth,
                        visiting,
                        stats,
                        budget,
                        sink,
                    );
                }
                if !graphics {
                    continue;
                }
                let stroking = op.operator.starts_with('S');
                let space = if stroking {
                    state.stroke_space.clone()
                } else {
                    state.fill_space.clone()
                };
                let values: Vec<f64> = op.operands.iter().filter_map(as_f64).collect();
                let color = if values.is_empty() {
                    // Nur ein Musternamen — Muster werden als mittleres Grau genähert.
                    Rgb::gray(0.5)
                } else {
                    space.to_rgb(&values)
                };
                if stroking {
                    state.stroke = color;
                } else {
                    state.fill = color;
                }
            }
            // --- Linienzustand ----------------------------------------------
            "w" if graphics => {
                state.line_width = op.operands.first().and_then(as_f64).unwrap_or(1.0).max(0.0)
            }
            "J" if graphics => {
                state.line_cap = op.operands.first().and_then(as_f64).unwrap_or(0.0).max(0.0) as u8
            }
            "j" if graphics => {
                state.line_join = op.operands.first().and_then(as_f64).unwrap_or(0.0).max(0.0) as u8
            }
            "M" if graphics => {}
            "d" if graphics => {
                state.dash = op
                    .operands
                    .first()
                    .and_then(|o| o.as_array().ok())
                    .map(|a| a.iter().filter_map(as_f64).filter(|v| *v >= 0.0).collect())
                    .unwrap_or_default();
                // Eine Strichelung aus lauter Nullen bedeutet „durchgezogen“.
                if state.dash.iter().all(|d| *d <= 0.0) {
                    state.dash.clear();
                }
                state.dash_phase = op.operands.get(1).and_then(as_f64).unwrap_or(0.0);
            }
            "gs" => {
                if graphics {
                    apply_ext_gstate(doc, resources, &op.operands, &mut state);
                }
                // Eine weiche Maske mit Gruppen-Form trägt einen **eigenen**
                // Inhaltsstrom, den kein `Do` je erreicht. Er wird hier
                // betreten — unabhängig davon, ob die Senke Grafik will: Text
                // ist Text, gleich in welcher Rolle er in der Datei steht.
                scan_soft_mask(
                    doc,
                    resources,
                    owner,
                    fonts,
                    &op.operands,
                    &state,
                    depth,
                    visiting,
                    stats,
                    budget,
                    sink,
                );
            }
            // --- Inline-Bild ------------------------------------------------
            // Der Operationsstrom kommt von `ops::decode_content`, das
            // `BI … ID … EI` zu einer Operation mit Dictionary und Rohdaten
            // zusammenfasst.
            "BI" if images => {
                if let (Some(Object::Dictionary(dict)), Some(Object::String(data, _))) =
                    (op.operands.first(), op.operands.get(1))
                {
                    sink.image(
                        &cx,
                        &ImageEvent {
                            name: None,
                            inline: Some((dict, data)),
                            ctm: state.ctm,
                            fill: state.fill,
                            fill_alpha: state.fill_alpha,
                            clip: state.clip,
                        },
                    );
                }
            }
            "Do" => {
                let Some(Object::Name(name)) = op.operands.first() else {
                    continue;
                };
                match load_xobject(doc, resources, name, budget) {
                    // Kein solcher Eintrag: es wird nichts gezeichnet, also
                    // versteckt sich hier auch nichts.
                    XObjectEntry::Missing => {}
                    XObjectEntry::Image => {
                        if images {
                            sink.image(
                                &cx,
                                &ImageEvent {
                                    name: Some(name),
                                    inline: None,
                                    ctm: state.ctm,
                                    fill: state.fill,
                                    fill_alpha: state.fill_alpha,
                                    clip: state.clip,
                                },
                            );
                        }
                    }
                    // Vorhanden, aber nicht lesbar. Früher ein stilles
                    // `continue`: der Text darin fehlte in der Analyse, die
                    // Schwärzung meldete „nichts gefunden“, und die Datei galt
                    // als sauber.
                    XObjectEntry::Unusable(message) => sink.warn(message),
                    XObjectEntry::Form(form_id, form_matrix, form) => {
                        if depth >= MAX_FORM_DEPTH {
                            sink.warn(format!(
                                "Form-XObject „{}“ ist tiefer als {MAX_FORM_DEPTH} Ebenen \
                                 verschachtelt; ab dort wurde nicht weitergelesen. Text in \
                                 den tieferen Ebenen wurde nicht durchsucht und kann \
                                 deshalb nicht geschwärzt worden sein.",
                                String::from_utf8_lossy(name)
                            ));
                            continue;
                        }
                        sink.form(form_id);
                        sink.form_within(stream, op_index, form_id);
                        if !visiting.insert(form_id) {
                            continue; // Zyklus
                        }
                        // Ohne eigenes `/Resources` gelten die des Aufrufers —
                        // und damit auch dessen bereits geladene Schriften.
                        // Genau hier hängt das Ergebnis am Zusammenhang, und
                        // genau hier wird deshalb nichts gemerkt.
                        let own_fonts = budget.fonts_of(doc, &form);
                        let (form_resources, form_own, form_fonts) =
                            match (&form.resources, &own_fonts) {
                                (Some(own), Some(own_fonts)) => (Some(&**own), true, &**own_fonts),
                                _ => (resources, false, fonts),
                            };
                        // Ohne eigenes `/Resources` bleibt der Aufrufer der
                        // Eigentümer — die Namen lösen sich dort auf.
                        let form_owner = if form_own { Some(form_id) } else { owner };
                        scan_operations(
                            doc,
                            form.operations(),
                            StreamKey::Form(form_id),
                            form_resources,
                            form_own,
                            form_owner,
                            form_fonts,
                            form_matrix.mul(&state.ctm),
                            depth + 1,
                            visiting,
                            stats,
                            budget,
                            sink,
                        );
                        visiting.remove(&form_id);
                    }
                }
            }
            // `sh` (Schattierungen) und alles Unbekannte werden übergangen.
            _ => {}
        }
    }
}

/// Setzt Füll- bzw. Strichfarbe; Großbuchstaben-Operatoren betreffen den Strich.
fn set_color(state: &mut GraphicsState, operator: &str, space: ColorSpace, color: Rgb) {
    if operator.chars().next().is_some_and(|c| c.is_uppercase()) {
        state.stroke_space = space;
        state.stroke = color;
    } else {
        state.fill_space = space;
        state.fill = color;
    }
}

/// Das `/ExtGState`-Dictionary hinter einem `gs`-Operanden.
fn ext_gstate_dict(
    doc: &Document,
    resources: Option<&Dictionary>,
    operands: &[Object],
) -> Option<Dictionary> {
    let Some(Object::Name(name)) = operands.first() else {
        return None;
    };
    resources
        .and_then(|r| r.get(b"ExtGState").ok())
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok())
        .and_then(|d| d.get(name.as_slice()).ok())
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok().cloned())
}

/// Betritt die Gruppen-Form einer weichen Maske (`/ExtGState /SMask /G`).
///
/// Eine weiche Maske ist ein vollwertiges Form-XObject mit eigenem
/// Ressourcenverzeichnis und eigenem Text. Sie wird nicht mit `Do` gezeichnet,
/// sondern über die Grafikzustands-Parameter gesetzt — der Interpreter kam
/// deshalb nie hinein. Ihr Text stand ungelesen in der Datei: „Treffer 0,
/// Rückgabewert 0“, während `pdftotext` die IBAN im Klartext las.
///
/// Gelesen wird im Koordinatensystem, das beim `gs` gilt (PDF 32000-1,
/// 11.6.5.2), also mit der CTM dieses Augenblicks. Die Datensätze tragen
/// [`StreamKey::Form`] mit der Objekt-Id der Gruppe; die Schwärzung schreibt
/// sie damit wie jedes andere Formular neu.
#[allow(clippy::too_many_arguments)]
fn scan_soft_mask(
    doc: &Document,
    resources: Option<&Dictionary>,
    owner: Option<ObjectId>,
    fonts: &FontMap,
    operands: &[Object],
    state: &GraphicsState,
    depth: usize,
    visiting: &mut HashSet<ObjectId>,
    stats: &mut FontDecodeStats,
    budget: &mut Budget,
    sink: &mut dyn ContentSink,
) {
    let Some(gstate) = ext_gstate_dict(doc, resources, operands) else {
        return;
    };
    // `/SMask /None` schaltet die Maske ab und ist ein Name, kein Dictionary.
    let Some(mask) = gstate
        .get(b"SMask")
        .ok()
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok().cloned())
    else {
        return;
    };
    let Ok(group) = mask.get(b"G") else {
        return;
    };
    let Some(group_id) = group.as_reference().ok() else {
        // Eine Gruppe ohne eigene Objekt-Id ließe sich nicht neu schreiben —
        // was darin steht, bliebe stehen. Das muss gesagt werden.
        sink.warn(
            "Eine weiche Maske (/ExtGState /SMask /G) trägt ihre Gruppen-Form nicht als \
             eigenes Objekt; sie wurde nicht durchsucht und kann deshalb nicht \
             geschwärzt worden sein."
                .to_string(),
        );
        return;
    };
    if depth >= MAX_FORM_DEPTH {
        sink.warn(format!(
            "Die Gruppen-Form einer weichen Maske (Objekt {} {}) liegt tiefer als \
             {MAX_FORM_DEPTH} Ebenen; ab dort wurde nicht weitergelesen. Ihr Text wurde \
             nicht durchsucht und kann deshalb nicht geschwärzt worden sein.",
            group_id.0, group_id.1
        ));
        return;
    }
    let Ok(stream) = doc.get_object(group_id).and_then(|o| o.as_stream()) else {
        sink.warn(format!(
            "Die Gruppen-Form einer weichen Maske (Objekt {} {}) ist kein lesbarer Strom; \
             ihr Text wurde nicht durchsucht und kann deshalb nicht geschwärzt worden sein.",
            group_id.0, group_id.1
        ));
        return;
    };
    // Eine Maske kann an jedem einzelnen `gs` hängen; ausgepackt wird sie
    // trotzdem nur einmal.
    let group = budget.stream(doc, group_id, stream);
    if !group.decodable() {
        sink.warn(format!(
            "Die Gruppen-Form einer weichen Maske (Objekt {} {}) ließ sich nicht \
             dekodieren; ihr Text wurde nicht durchsucht und kann deshalb nicht \
             geschwärzt worden sein.",
            group_id.0, group_id.1
        ));
        return;
    }
    if group.truncated() {
        sink.warn(format!(
            "Ein Teil der Gruppen-Form einer weichen Maske (Objekt {} {}) ließ sich nicht \
             in Operationen zerlegen ({} Byte betroffen). Dieser Text wurde nicht \
             durchsucht und kann deshalb nicht geschwärzt worden sein.",
            group_id.0,
            group_id.1,
            group.affected_bytes()
        ));
    }
    if group.operations().is_empty() {
        return;
    }
    budget.credit(Some(group_id), group.operations().len());
    if !visiting.insert(group_id) {
        return; // Zyklus
    }
    let matrix = stream
        .dict
        .get(b"Matrix")
        .ok()
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_array().ok())
        .and_then(|a| matrix_from(a))
        .unwrap_or(Matrix::IDENTITY);
    // Ohne eigenes `/Resources` gelten die des Aufrufers — samt seiner
    // bereits geladenen Schriften.
    let own_fonts = budget.fonts_of(doc, &group);
    let (group_resources, group_own, group_fonts) = match (&group.resources, &own_fonts) {
        (Some(own), Some(own_fonts)) => (Some(&**own), true, &**own_fonts),
        _ => (resources, false, fonts),
    };
    let group_owner = if group_own { Some(group_id) } else { owner };
    // Die Maske ist ein platzierter Strom wie ein Formular; sie muss auch so
    // gezählt werden, sonst hielte die Ressourcenprüfung sie für ungezeichnet.
    sink.form(group_id);
    scan_operations(
        doc,
        group.operations(),
        StreamKey::Form(group_id),
        group_resources,
        group_own,
        group_owner,
        group_fonts,
        matrix.mul(&state.ctm),
        depth + 1,
        visiting,
        stats,
        budget,
        sink,
    );
    visiting.remove(&group_id);
}

/// Übernimmt die für das Zeichnen relevanten Einträge aus einem `/ExtGState`.
fn apply_ext_gstate(
    doc: &Document,
    resources: Option<&Dictionary>,
    operands: &[Object],
    state: &mut GraphicsState,
) {
    let Some(dict) = ext_gstate_dict(doc, resources, operands) else {
        return;
    };
    if let Some(lw) = dict.get(b"LW").ok().and_then(as_f64) {
        state.line_width = lw.max(0.0);
    }
    if let Some(ca) = dict.get(b"ca").ok().and_then(as_f64) {
        state.fill_alpha = ca.clamp(0.0, 1.0) as f32;
    }
    if let Some(ca) = dict.get(b"CA").ok().and_then(as_f64) {
        state.stroke_alpha = ca.clamp(0.0, 1.0) as f32;
    }
    if let Some(lc) = dict.get(b"LC").ok().and_then(as_f64) {
        state.line_cap = lc.max(0.0) as u8;
    }
    if let Some(lj) = dict.get(b"LJ").ok().and_then(as_f64) {
        state.line_join = lj.max(0.0) as u8;
    }
    if let Some(d) = dict.get(b"D").ok().and_then(|o| o.as_array().ok()) {
        if let Some(array) = d.first().and_then(|o| o.as_array().ok()) {
            state.dash = array
                .iter()
                .filter_map(as_f64)
                .filter(|v| *v >= 0.0)
                .collect();
            if state.dash.iter().all(|v| *v <= 0.0) {
                state.dash.clear();
            }
        }
        state.dash_phase = d.get(1).and_then(as_f64).unwrap_or(0.0);
    }
}

/// Punkt aus zwei Operanden, direkt in den User-Space transformiert.
fn point_at(operands: &[Object], index: usize, ctm: &Matrix) -> Option<Point> {
    let x = operands.get(index).and_then(as_f64)?;
    let y = operands.get(index + 1).and_then(as_f64)?;
    Some(ctm.apply(x, y))
}

/// `re`: ein Rechteck als geschlossener Teilpfad im User-Space.
fn rect_path(operands: &[Object], ctm: &Matrix) -> Option<Vec<PathSeg>> {
    let x = operands.first().and_then(as_f64)?;
    let y = operands.get(1).and_then(as_f64)?;
    let w = operands.get(2).and_then(as_f64)?;
    let h = operands.get(3).and_then(as_f64)?;
    Some(vec![
        PathSeg::MoveTo(ctm.apply(x, y)),
        PathSeg::LineTo(ctm.apply(x + w, y)),
        PathSeg::LineTo(ctm.apply(x + w, y + h)),
        PathSeg::LineTo(ctm.apply(x, y + h)),
        PathSeg::Close,
    ])
}

/// Was hinter einem `Do`-Namen in den Ressourcen steckt.
enum XObjectEntry {
    Image,
    /// Form-XObject: Objekt-Id, `/Matrix` und der einmal ermittelte Rumpf.
    Form(ObjectId, Matrix, Rc<PlacedStream>),
    /// Vorhanden, aber nicht auswertbar — mit fertiger Begründung für die
    /// Warnung. Was hier steht, wird nicht durchsucht; das muss der Nutzer
    /// erfahren.
    Unusable(String),
    /// Kein solcher Eintrag in den Ressourcen.
    Missing,
}

/// Löst einen `Do`-Namen auf und holt bei einem Form-XObject gleich den Rumpf.
///
/// Jeder Weg, auf dem hier nichts Brauchbares herauskommt, wird benannt statt
/// verschwiegen: ein Form-XObject ohne `/Subtype` oder mit einem Filter, den
/// niemand dekodieren kann, versteckt seinen Text sonst lautlos.
///
/// Die **Beschriftung** (`label`) wird bei jeder Platzierung frisch gebildet:
/// dasselbe Objekt kann unter zwei Ressourcennamen stehen, und dann muss die
/// Warnung den Namen nennen, unter dem es hier steht — nicht den, unter dem
/// es zuerst gefunden wurde. Zwischengespeichert wird deshalb nur, was am
/// Objekt hängt (siehe [`FormBody`]).
fn load_xobject(
    doc: &Document,
    resources: Option<&Dictionary>,
    name: &[u8],
    budget: &mut Budget,
) -> XObjectEntry {
    let label = String::from_utf8_lossy(name).into_owned();
    let entry = resources
        .and_then(|r| r.get(b"XObject").ok())
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok())
        .and_then(|d| d.get(name).ok());
    let Some(entry) = entry else {
        return XObjectEntry::Missing;
    };
    let Ok((id, resolved)) = doc.dereference(entry) else {
        return XObjectEntry::Unusable(format!(
            "XObject „{label}“ verweist auf ein Objekt, das es nicht gibt; sein \
             Inhalt wurde nicht durchsucht."
        ));
    };
    let Ok(stream) = resolved.as_stream() else {
        return XObjectEntry::Unusable(format!(
            "XObject „{label}“ ist kein Stream; sein Inhalt wurde nicht durchsucht."
        ));
    };
    match crate::ops::xobject_subtype(doc, &stream.dict) {
        Some(b"Image") => XObjectEntry::Image,
        Some(b"Form") => {
            let Some(id) = id else {
                return XObjectEntry::Unusable(format!(
                    "Form-XObject „{label}“ ist kein eigenständiges Objekt; sein Text \
                     wurde nicht durchsucht und kann deshalb nicht geschwärzt worden sein."
                ));
            };
            let form = budget.stream(doc, id, stream);
            if !form.decodable() {
                return XObjectEntry::Unusable(format!(
                    "Form-XObject „{label}“ ließ sich nicht dekodieren (unbekannter oder \
                     defekter Filter); sein Text wurde nicht durchsucht und kann deshalb \
                     nicht geschwärzt worden sein."
                ));
            }
            if form.truncated() {
                return XObjectEntry::Unusable(format!(
                    "Ein Teil des Form-XObjects „{label}“ ließ sich nicht in Operationen \
                     zerlegen; ab der Bruchstelle fehlt alles Weitere ({} Byte betroffen). \
                     Dieser Text wurde nicht durchsucht und kann deshalb nicht geschwärzt \
                     worden sein.",
                    form.affected_bytes()
                ));
            }
            // Der Inhalt dieses Stroms bringt einmal Guthaben ein; jede
            // weitere Platzierung zehrt nur noch davon.
            budget.credit(Some(id), form.operations().len());
            if form.operations().is_empty() && form.has_tokens {
                return XObjectEntry::Unusable(format!(
                    "Der Inhalt des Form-XObjects „{label}“ ließ sich nicht in Operationen \
                     zerlegen; sein Text wurde nicht durchsucht und kann deshalb nicht \
                     geschwärzt worden sein."
                ));
            }
            let matrix = stream
                .dict
                .get(b"Matrix")
                .ok()
                .and_then(|o| o.as_array().ok())
                .and_then(|a| matrix_from(a))
                .unwrap_or(Matrix::IDENTITY);
            XObjectEntry::Form(id, matrix, form)
        }
        // Ohne `/Subtype /Form` steigt der Interpreter aus — und stünde dann
        // vor genau dem Text, den er hätte finden sollen.
        _ => XObjectEntry::Unusable(format!(
            "XObject „{label}“ hat kein bekanntes /Subtype (weder /Form noch /Image); \
             sein Inhalt wurde nicht durchsucht. Steht dort Text, blieb er ungeschwärzt."
        )),
    }
}

/// Durchläuft ein Kachelmuster (`/PatternType 1`) — dessen Content-Stream kann
/// Text enthalten.
///
/// Das Muster wird gekachelt gemalt; durchlaufen wird nur die Kachel im
/// Ursprung des Musterraums. Für die Schwärzung reicht das: gefunden wird der
/// Text an der Stelle dieser einen Kachel, entfernt wird er aus dem
/// Musterstrom — und damit aus **allen** Kacheln. Dass die weiteren Kacheln
/// nicht einzeln vermessen werden, sagt die Warnung.
#[allow(clippy::too_many_arguments)]
fn scan_tiling_pattern(
    doc: &Document,
    // Der Strom, in dem das `scn` steht, und dessen Index — damit ein Spiegel
    // eines **äußeren** Stroms auch die Glyphen dieses Musters umfasst
    // ([`ContentSink::form_within`], Register #66).
    parent: StreamKey,
    op_index: usize,
    resources: Option<&Dictionary>,
    owner: Option<ObjectId>,
    fonts: &FontMap,
    name: &[u8],
    base_ctm: Matrix,
    depth: usize,
    visiting: &mut HashSet<ObjectId>,
    stats: &mut FontDecodeStats,
    budget: &mut Budget,
    sink: &mut dyn ContentSink,
) {
    let label = String::from_utf8_lossy(name).into_owned();
    let entry = resources
        .and_then(|r| r.get(b"Pattern").ok())
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok())
        .and_then(|d| d.get(name).ok());
    let Some(entry) = entry else {
        return;
    };
    let Ok((id, resolved)) = doc.dereference(entry) else {
        return;
    };
    // Ein Schattierungsmuster (`/PatternType 2`) ist ein Dictionary ohne
    // Content-Stream; dort steht kein Text.
    let Ok(stream) = resolved.as_stream() else {
        return;
    };
    if stream.dict.get(b"PatternType").ok().and_then(as_f64) == Some(2.0) {
        return;
    }
    // Ein Muster kann an jedem einzelnen `scn` hängen; ausgepackt wird es
    // trotzdem nur einmal.
    let pattern = match id {
        Some(id) => budget.stream(doc, id, stream),
        None => Rc::new(budget.load_stream(doc, None, stream)),
    };
    if !pattern.decodable() {
        sink.warn(format!(
            "Kachelmuster „{label}“ ließ sich nicht dekodieren; sein Inhalt wurde nicht \
             durchsucht. Steht dort Text, blieb er ungeschwärzt."
        ));
        return;
    }
    if pattern.truncated() {
        sink.warn(format!(
            "Ein Teil des Kachelmusters „{label}“ ließ sich nicht in Operationen zerlegen; \
             ab der Bruchstelle fehlt alles Weitere ({} Byte betroffen). Dieser Text wurde \
             nicht durchsucht und kann deshalb nicht geschwärzt worden sein.",
            pattern.affected_bytes()
        ));
    }
    // Ein Muster, dessen Strom weder Text setzt noch etwas platziert (`Do`,
    // `BI`), ist ein Schraffur- oder Logomuster aus Pfaden: kein Befund, keine
    // Meldung. Bis zur Spur-A-Runde 1 entschied hier allein der Textoperator —
    // ein Bild im Muster ohne Text bekam der Bildsammler so nie zu sehen, und
    // ein Formular im Muster (mit Text darin) niemand (Register #75).
    let setzt_text = pattern
        .operations()
        .iter()
        .any(|op| matches!(op.operator.as_str(), "Tj" | "TJ" | "'" | "\""));
    let platziert = pattern
        .operations()
        .iter()
        .any(|op| matches!(op.operator.as_str(), "Do" | "BI"));
    if !setzt_text && !platziert {
        return;
    }
    if depth >= MAX_FORM_DEPTH {
        sink.warn(format!(
            "Kachelmuster „{label}“ liegt tiefer als {MAX_FORM_DEPTH} Ebenen \
             verschachtelt; sein Text wurde nicht durchsucht und kann deshalb nicht \
             geschwärzt worden sein."
        ));
        return;
    }
    let Some(id) = id else {
        sink.warn(format!(
            "Kachelmuster „{label}“ ist kein eigenständiges Objekt; sein Text wurde \
             nicht durchsucht und kann deshalb nicht geschwärzt worden sein."
        ));
        return;
    };
    // Wie ein Formular im Formular: der Spiegel eines äußeren Stroms über
    // dem `scn` gilt auch für die Glyphen des Musters. Vor der Zyklusprobe,
    // weil die Verschachtelung auch beim zweiten Setzen desselben Musters
    // gilt (siehe die `Do`-Stelle in [`scan_operations`]).
    sink.form_within(parent, op_index, id);
    // Einmal je Dokumentobjekt: ein zweiter Durchlauf brächte nur denselben
    // Text ein zweites Mal (und bei Zyklen gar keinen).
    if !visiting.insert(id) {
        return;
    }
    budget.credit(Some(id), pattern.operations().len());

    let inhalt = match (setzt_text, platziert) {
        (true, false) => "enthält Text",
        (false, _) => "platziert Bilder oder Formulare",
        (true, true) => "enthält Text und platziert Bilder oder Formulare",
    };
    sink.warn(format!(
        "Kachelmuster „{label}“ {inhalt}. Was darin steht, wird an der Stelle der ersten \
         Kachel gesucht und beim Schwärzen aus dem Muster entfernt — die übrigen Kacheln \
         werden dabei nicht einzeln vermessen. Bitte das Ergebnis dort prüfen."
    ));

    // Der Musterraum hängt am Ausgangszustand des Streams, nicht an der CTM
    // zum Zeitpunkt des `scn` (PDF 32000-1, 8.7.3.1).
    let matrix = stream
        .dict
        .get(b"Matrix")
        .ok()
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_array().ok())
        .and_then(|a| matrix_from(a))
        .unwrap_or(Matrix::IDENTITY);
    // Ohne eigenes `/Resources` gelten die des Aufrufers — samt seiner
    // bereits geladenen Schriften.
    let own_fonts = budget.fonts_of(doc, &pattern);
    let (pattern_resources, pattern_own, pattern_fonts) = match (&pattern.resources, &own_fonts) {
        (Some(own), Some(own_fonts)) => (Some(&**own), true, &**own_fonts),
        _ => (resources, false, fonts),
    };
    let pattern_owner = if pattern_own { Some(id) } else { owner };
    scan_operations(
        doc,
        pattern.operations(),
        StreamKey::Form(id),
        pattern_resources,
        pattern_own,
        pattern_owner,
        pattern_fonts,
        matrix.mul(&base_ctm),
        depth + 1,
        visiting,
        stats,
        budget,
        sink,
    );
}

/// Meldet Type3-Glyphprozeduren, die selbst Text setzen.
///
/// Ein Type3-Font hat kein Fontprogramm: jede Glyphe ist ein Content-Stream
/// unter `/CharProcs`. Üblicherweise malt der nur — dann ist alles in Ordnung
/// und hier passiert nichts. Er darf aber auch `BT … Tj` enthalten, also mit
/// einem *anderen* Font Klartext setzen. Dann geht die Schwärzung ins Leere,
/// ohne es zu merken: der Zeichencode verschwindet sauber aus dem Seitenstrom,
/// die Glyphe ist danach unsichtbar — und der Klartext steht weiter im
/// Prozedurstrom, wo `leaks` ihn findet.
///
/// ## Warum nur eine Warnung, statt `/CharProcs` mitzudurchsuchen
///
/// Durchsuchen hieße hier: rekursiv interpretieren **und** anschließend
/// umschreiben. Beides ist an dieser Stelle schlechter als eine ehrliche
/// Meldung:
///
/// * Eine Glyphprozedur gehört keiner Stelle auf der Seite, sondern **allen**
///   Vorkommen ihres Zeichencodes — im ganzen Dokument. Was man dort entfernt,
///   entfernt man überall; was man dort stehen lässt, bleibt überall stehen.
///   Dieselbe Unschärfe wie beim Kachelmuster, nur ohne dessen Nutzen: sichtbar
///   ist der Text ohnehin schon weg, weil der Zeichencode aus dem Seitenstrom
///   verschwindet.
/// * Eine neue Rekursion muss gegen das Aufwandskonto [`Budget`] gerechnet
///   werden. Eine Schrift mit vielen Glyphprozeduren, vielfach gesetzt, wäre
///   sonst genau der nächste Weg an der Grenze vorbei — und dieser Weg wäre
///   billiger als jeder bisherige, weil er nicht einmal ein Form-XObject
///   braucht. Der Blick hierher zahlt deshalb je untersuchter Operation und
///   findet je Strom und Schrift genau einmal statt.
///
/// Was bleibt, ist die Meldung: der Befund heißt dann nicht mehr „angewendet“,
/// und der Nutzer weiß, wo er nachsehen muss.
fn warn_about_text_in_charprocs(
    doc: &Document,
    resources: Option<&Dictionary>,
    stream: StreamKey,
    font_name: &[u8],
    budget: &mut Budget,
    sink: &mut dyn ContentSink,
) {
    if font_name.is_empty() || !budget.first_look_at_type3(stream, font_name) {
        return;
    }
    let font = resources
        .and_then(|r| r.get(b"Font").ok())
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok())
        .and_then(|d| d.get(font_name).ok())
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok().cloned());
    let Some(font) = font else {
        return;
    };
    if font.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Type3".as_slice()) {
        return;
    }
    let procs = font
        .get(b"CharProcs")
        .ok()
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok().cloned());
    let Some(procs) = procs else {
        return;
    };

    let label = String::from_utf8_lossy(font_name).into_owned();
    let mut with_text: Vec<String> = Vec::new();
    let mut unreadable = 0usize;
    for (name, value) in procs.iter() {
        let Ok((id, resolved)) = doc.dereference(value) else {
            unreadable += 1;
            continue;
        };
        let Ok(proc_stream) = resolved.as_stream() else {
            unreadable += 1;
            continue;
        };
        let decoded = match id {
            Some(id) => budget.stream(doc, id, proc_stream),
            None => Rc::new(budget.load_stream(doc, None, proc_stream)),
        };
        if !decoded.decodable() {
            unreadable += 1;
            continue;
        }
        if decoded.truncated() {
            unreadable += 1;
        }
        // Der Inhalt bringt einmal Guthaben ein — wie jeder andere Strom auch.
        budget.credit(id, decoded.operations().len());
        let mut sets_text = false;
        for op in decoded.operations() {
            // Jede geprüfte Operation kostet. Ist das Konto leer, endet der
            // Scan ohnehin; dann wird hier nichts mehr behauptet.
            if !budget.operation() {
                return;
            }
            if matches!(op.operator.as_str(), "Tj" | "TJ" | "'" | "\"") {
                sets_text = true;
            }
        }
        if sets_text {
            with_text.push(String::from_utf8_lossy(name).into_owned());
        }
    }

    if unreadable > 0 {
        sink.warn(format!(
            "Von der Type3-Schrift „{label}“ ließen sich {unreadable} Glyphprozedur(en) \
             (/CharProcs) nicht vollständig lesen. Steht dort Text, wurde er nicht \
             durchsucht und kann deshalb nicht geschwärzt worden sein."
        ));
    }
    if with_text.is_empty() {
        return;
    }
    with_text.sort();
    sink.warn(format!(
        "Die Type3-Schrift „{label}“ setzt in {} ihrer Glyphprozeduren selbst Text \
         (/CharProcs: {}). Diese Prozeduren sind eigene Ströme und wurden nicht \
         durchsucht: eine Schwärzung entfernt zwar den Zeichencode aus dem Seiteninhalt \
         — die Glyphe ist danach nicht mehr zu sehen —, der Klartext bleibt aber im \
         Prozedurstrom stehen und ist in der Datei weiter zu finden. Bitte das Ergebnis \
         dort prüfen.",
        with_text.len(),
        with_text.join(", ")
    ));
}

/// Berechnet die Glyphen einer Text-Ausgabe-Operation und schreibt `tm` fort.
///
/// **Einzige Stelle** im Programm, an der Textmatrix, Rendering-Matrix und
/// Vorschub berechnet werden. Sowohl der [`ShowRecord`] für Extraktion und
/// Schwärzung als auch das Glyph-Ereignis für den Renderer entstehen hier aus
/// denselben Zwischenwerten.
#[allow(clippy::too_many_arguments)]
fn show_text(
    operands: &[Object],
    state: &GraphicsState,
    tm: &mut Matrix,
    cx: &SinkContext,
    operator: &str,
    stats: &mut FontDecodeStats,
    budget: &mut Budget,
    sink: &mut dyn ContentSink,
    emit_glyphs: bool,
) -> Option<ShowRecord> {
    // Geliehen statt geklont: `Tj` kann Millionen Zeichen tragen, aber die
    // Schrift ist dieselbe. Der Ersatz-Font entsteht nur, wenn gar kein `Tf`
    // dastand — dann ist er auch billig.
    let fallback;
    let font: &FontInfo = match state.text.font.as_deref() {
        Some(font) => font,
        None => {
            fallback = FontInfo::default();
            &fallback
        }
    };
    let ts = &state.text;

    // Die Textargumente stehen bei ' und " nicht an erster Stelle.
    let arg = match operator {
        "\"" => operands.get(2)?,
        "'" => operands.first()?,
        _ => operands.first()?,
    };

    let mut elements: Vec<Object> = Vec::new();
    match arg {
        Object::Array(items) => elements.extend(items.iter().cloned()),
        other => elements.push(other.clone()),
    }

    // Die Leerzeichenbreite des Fonts ist der Maßstab für „echte Lücke“.
    let space_width = font.width(32, " ");

    let mut items = Vec::new();
    for element in &elements {
        match element {
            Object::String(bytes, _) => {
                for (code, text, nbytes) in font.charmap.decode(bytes) {
                    // Innerhalb der Schleife, nicht erst danach: eine einzige
                    // `Tj`-Operation kann Millionen Zeichen tragen, und jedes
                    // davon wird als [`GlyphItem`] gehalten.
                    if !budget.glyph() {
                        break;
                    }
                    stats.record(&ts.font_name, font, code, &text);
                    let w0 = font.width(code, &text);
                    let is_space = nbytes == 1 && code == 32;
                    let displacement = (w0 * ts.font_size
                        + ts.char_spacing
                        + if is_space { ts.word_spacing } else { 0.0 })
                        * ts.h_scale;

                    let param = Matrix::new(
                        ts.font_size * ts.h_scale,
                        0.0,
                        0.0,
                        ts.font_size,
                        0.0,
                        ts.rise,
                    );
                    // Textraum → User-Space; daraus stammt die Schreibrichtung.
                    let text_to_user = tm.mul(&state.ctm);
                    let trm = param.mul(&text_to_user);
                    let rect = glyph_rect(&trm, w0, font.ascent, font.descent);
                    let origin = trm.apply(0.0, 0.0);
                    let baseline = baseline_of(
                        &text_to_user,
                        displacement,
                        (font.ascent - font.descent) * ts.font_size,
                        space_width * ts.font_size * ts.h_scale,
                    );

                    if emit_glyphs {
                        sink.glyph(
                            cx,
                            &GlyphEvent {
                                font_name: &ts.font_name,
                                font,
                                code,
                                text: &text,
                                trm,
                                fill: state.fill,
                                fill_alpha: state.fill_alpha,
                                render_mode: ts.render_mode,
                                clip: state.clip,
                            },
                        );
                    }

                    // Ligaturen (ein Code → mehrere Zeichen) werden zeichenweise
                    // aufgeteilt, damit Text und Glyphen 1:1 zusammenpassen.
                    let char_count = text.chars().count().max(1);
                    if char_count == 1 {
                        items.push(ShowItem::Glyph(GlyphItem {
                            bytes: raw_code_bytes(bytes, font, code, nbytes),
                            text,
                            rect,
                            origin,
                            displacement,
                            baseline,
                        }));
                    } else {
                        let bytes_for_code = raw_code_bytes(bytes, font, code, nbytes);
                        let width = rect.width() / char_count as f64;
                        for (i, ch) in text.chars().enumerate() {
                            let sub = Rect::new(
                                rect.ll.x + width * i as f64,
                                rect.ll.y,
                                rect.ll.x + width * (i + 1) as f64,
                                rect.ur.y,
                            );
                            items.push(ShowItem::Glyph(GlyphItem {
                                // Nur das erste Teilzeichen trägt die Originalbytes,
                                // damit der Code beim Neuschreiben nicht dupliziert wird.
                                bytes: if i == 0 {
                                    bytes_for_code.clone()
                                } else {
                                    Vec::new()
                                },
                                text: ch.to_string(),
                                rect: sub,
                                origin,
                                displacement: if i == 0 { displacement } else { 0.0 },
                                baseline: Baseline {
                                    advance: if i == 0 { baseline.advance } else { 0.0 },
                                    ..baseline
                                },
                            }));
                        }
                    }

                    *tm = Matrix::translate(displacement, 0.0).mul(tm);
                }
            }
            Object::Integer(_) | Object::Real(_) => {
                let adj = as_f64(element).unwrap_or(0.0);
                let tx = -adj / 1000.0 * ts.font_size * ts.h_scale;
                *tm = Matrix::translate(tx, 0.0).mul(tm);
                items.push(ShowItem::Adjust(adj));
            }
            _ => {}
        }
    }

    Some(ShowRecord {
        stream: cx.stream,
        op_index: cx.op_index,
        operator: operator.to_string(),
        operands: operands.to_vec(),
        font_size: ts.font_size,
        h_scale: ts.h_scale,
        items,
    })
}

/// Liefert die Originalbytes eines Codes.
fn raw_code_bytes(_source: &[u8], _font: &FontInfo, code: u32, nbytes: usize) -> Vec<u8> {
    match nbytes {
        2 => vec![(code >> 8) as u8, (code & 0xFF) as u8],
        _ => vec![(code & 0xFF) as u8],
    }
}

/// Rechnet die Grundlinien-Geometrie einer Glyphe in den User-Space.
///
/// `text_to_user` ist `Tm × CTM`, bildet also den Textraum ab. Die
/// Schreibrichtung ist das Bild der Textraum-x-Achse, die Zeilenhöhe wird
/// senkrecht dazu gemessen — nur so bleibt beides bei gedrehtem Text richtig.
fn baseline_of(
    text_to_user: &Matrix,
    displacement: f64,
    em_height: f64,
    space_width: f64,
) -> Baseline {
    let along = (text_to_user.a, text_to_user.b);
    let scale_x = along.0.hypot(along.1);
    let scale_y = text_to_user.c.hypot(text_to_user.d);
    if scale_x < 1e-12 {
        return Baseline {
            height: em_height.abs() * scale_y,
            ..Baseline::default()
        };
    }
    Baseline {
        direction: Point::new(along.0 / scale_x, along.1 / scale_x),
        advance: displacement * scale_x,
        height: em_height.abs() * scale_y,
        space_width: space_width * scale_x,
    }
}

/// Bounding-Box eines Glyphen: die vier Ecken werden transformiert, daraus
/// wird die achsenparallele Hülle gebildet (funktioniert auch bei Rotation).
fn glyph_rect(trm: &Matrix, width: f64, ascent: f64, descent: f64) -> Rect {
    let corners = [
        trm.apply(0.0, descent),
        trm.apply(width, descent),
        trm.apply(width, ascent),
        trm.apply(0.0, ascent),
    ];
    let mut min = Point::new(f64::MAX, f64::MAX);
    let mut max = Point::new(f64::MIN, f64::MIN);
    for c in corners {
        min.x = min.x.min(c.x);
        min.y = min.y.min(c.y);
        max.x = max.x.max(c.x);
        max.y = max.y.max(c.y);
    }
    Rect { ll: min, ur: max }
}

fn matrix_from(operands: &[Object]) -> Option<Matrix> {
    if operands.len() < 6 {
        return None;
    }
    Some(Matrix::new(
        as_f64(&operands[0])?,
        as_f64(&operands[1])?,
        as_f64(&operands[2])?,
        as_f64(&operands[3])?,
        as_f64(&operands[4])?,
        as_f64(&operands[5])?,
    ))
}

/// Ermittelt die CTM am Ende des Streams auf Stapel-Ebene 0 sowie die Zahl
/// nicht geschlossener `q`-Operationen. Beides wird gebraucht, um die
/// Deck-Rechtecke anschließend im unveränderten User-Space zu zeichnen.
pub fn trailing_state(operations: &[Operation]) -> (Matrix, usize) {
    let mut base_ctm = Matrix::IDENTITY;
    let mut depth = 0usize;
    for op in operations {
        match op.operator.as_str() {
            "q" => depth += 1,
            "Q" => depth = depth.saturating_sub(1),
            "cm" if depth == 0 => {
                if let Some(m) = matrix_from(&op.operands) {
                    base_ctm = m.mul(&base_ctm);
                }
            }
            _ => {}
        }
    }
    (base_ctm, depth)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::content::Content;
    use lopdf::{dictionary, Stream, StringFormat};

    fn ops(src: &[u8]) -> Vec<Operation> {
        Content::decode(src).unwrap().operations
    }

    /// Baut ein einseitiges PDF um ein beliebiges Font-Dictionary (`/F1`).
    ///
    /// Das Font-Dictionary wird erst gebaut, wenn das Dokument existiert —
    /// so kann es auf eigene Objekte (etwa eine `/ToUnicode`-CMap) verweisen.
    fn page_with_font(
        font: impl FnOnce(&mut Document) -> Dictionary,
        content: Vec<u8>,
    ) -> (Document, ObjectId) {
        let mut doc = Document::with_version("1.5");
        let font = font(&mut doc);
        let font_id = doc.add_object(font);
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let content_id = doc.add_object(Stream::new(dictionary! {}, content));
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
        (doc, page_id)
    }

    /// Identity-H-Subset, dessen CIDs bei 1 durchnummeriert sind.
    fn identity_subset_font(with_to_unicode: bool) -> impl FnOnce(&mut Document) -> Dictionary {
        move |doc: &mut Document| {
            let mut font = dictionary! {
                "Type" => "Font",
                "Subtype" => "Type0",
                "BaseFont" => "ABCDEF+Arial",
                "Encoding" => "Identity-H",
            };
            if with_to_unicode {
                let cmap = b"/CIDInit /ProcSet findresource begin
1 begincodespacerange <0000> <FFFF> endcodespacerange
1 beginbfrange <0001> <0016> <0041> endbfrange
endcmap"
                    .to_vec();
                let id = doc.add_object(Stream::new(dictionary! {}, cmap));
                font.set("ToUnicode", Object::Reference(id));
            }
            font
        }
    }

    /// `BT /F1 10 Tf … Tj ET` mit CIDs 1..=n als Zweibyte-Codes.
    fn identity_content(cids: &[u16]) -> Vec<u8> {
        let bytes: Vec<u8> = cids.iter().flat_map(|c| c.to_be_bytes()).collect();
        Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), Object::Real(10.0)]),
                Operation::new(
                    "Tm",
                    vec![
                        1.into(),
                        0.into(),
                        0.into(),
                        1.into(),
                        Object::Real(72.0),
                        Object::Real(700.0),
                    ],
                ),
                Operation::new("Tj", vec![Object::String(bytes, StringFormat::Hexadecimal)]),
                Operation::new("ET", vec![]),
            ],
        }
        .encode()
        .unwrap()
    }

    #[test]
    fn tracks_unbalanced_q_and_base_ctm() {
        let (ctm, depth) = trailing_state(&ops(b"q 1 0 0 1 5 5 cm Q 2 0 0 2 0 0 cm q q"));
        assert_eq!(depth, 2);
        assert_eq!(ctm, Matrix::scale(2.0, 2.0));
    }

    #[test]
    fn glyph_rect_is_axis_aligned_hull() {
        // 90°-Drehung: Breite und Höhe tauschen die Plätze.
        let rot = Matrix::new(0.0, 1.0, -1.0, 0.0, 0.0, 0.0);
        let trm = Matrix::scale(10.0, 10.0).mul(&rot);
        let r = glyph_rect(&trm, 0.5, 0.75, -0.25);
        assert!((r.width() - 10.0).abs() < 1e-9);
        assert!((r.height() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn appearance_is_fitted_into_the_annotation_rect() {
        // `/BBox` 100 × 50, um 90° gedreht (Hülle also 50 × 100), soll in ein
        // `/Rect` von 100 × 200 passen: Faktor 2 in beiden Achsen. Ohne diese
        // Abbildung läge der Annotationstext im Ursprung des Formularraums —
        // also irgendwo, nur nicht dort, wo er zu sehen ist.
        let rotate = Matrix::new(0.0, 1.0, -1.0, 0.0, 0.0, 0.0);
        let m = appearance_matrix(
            &rotate,
            Some(Rect::new(0.0, 0.0, 100.0, 50.0)),
            Some(Rect::new(10.0, 20.0, 110.0, 220.0)),
        );
        let a = m.apply(0.0, 0.0);
        let b = m.apply(100.0, 50.0);
        assert!(
            (a.x - 110.0).abs() < 1e-9 && (a.y - 20.0).abs() < 1e-9,
            "{a:?}"
        );
        assert!(
            (b.x - 10.0).abs() < 1e-9 && (b.y - 220.0).abs() < 1e-9,
            "{b:?}"
        );
    }

    #[test]
    fn an_appearance_without_bbox_keeps_its_own_matrix() {
        let m = Matrix::translate(5.0, 7.0);
        assert_eq!(
            appearance_matrix(&m, None, Some(Rect::new(0.0, 0.0, 10.0, 10.0))),
            m
        );
    }

    // -----------------------------------------------------------------------
    // K3 — Identity-H ohne /ToUnicode darf nicht still Müll liefern
    // -----------------------------------------------------------------------

    #[test]
    fn identity_font_without_to_unicode_warns_loudly() {
        let cids: Vec<u16> = (1..=22).collect();
        let (doc, page_id) = page_with_font(identity_subset_font(false), identity_content(&cids));
        let scan = scan_page(&doc, page_id).expect("Scan");

        // Vorbedingung: der dekodierte Text ist tatsächlich unbrauchbar.
        let text: String = scan
            .shows
            .iter()
            .flat_map(|s| s.glyphs())
            .map(|g| g.text.as_str())
            .collect();
        assert!(
            text.chars().filter(|c| *c == '\u{FFFD}').count() * 2 > text.chars().count(),
            "Testdaten taugen nicht, der Text ist lesbar: {text:?}"
        );

        assert!(
            scan.warnings.iter().any(|w| w.contains("ToUnicode")),
            "keine Warnung trotz undekodierbarem Font: {:?}",
            scan.warnings
        );
    }

    #[test]
    fn identity_font_with_to_unicode_stays_quiet() {
        let cids: Vec<u16> = (1..=22).collect();
        let (doc, page_id) = page_with_font(identity_subset_font(true), identity_content(&cids));
        let scan = scan_page(&doc, page_id).expect("Scan");
        assert!(
            scan.warnings.is_empty(),
            "unerwartete Warnung: {:?}",
            scan.warnings
        );
    }

    #[test]
    fn an_explicit_dw_drives_the_pen_not_the_font_name() {
        // Type0-Font mit `/DW 600` und `/ToUnicode`: die Namensschätzung
        // („Helvetica“, Ziffern 0,556) darf den Vorschub nicht bestimmen,
        // sonst läuft der Stift pro Zeichen um 0,44 pt voraus und die
        // x-Sortierung der Extraktion vertauscht Glyphen.
        let font = |doc: &mut Document| {
            let cmap = b"/CIDInit /ProcSet findresource begin
1 begincodespacerange <0000> <FFFF> endcodespacerange
1 beginbfrange <0001> <000A> <0030> endbfrange
endcmap"
                .to_vec();
            let to_unicode = doc.add_object(Stream::new(dictionary! {}, cmap));
            let descendant = doc.add_object(dictionary! {
                "Type" => "Font",
                "Subtype" => "CIDFontType2",
                "BaseFont" => "ABCDEF+Helvetica",
                "DW" => 600,
            });
            dictionary! {
                "Type" => "Font",
                "Subtype" => "Type0",
                "BaseFont" => "ABCDEF+Helvetica",
                "Encoding" => "Identity-H",
                "DescendantFonts" => vec![Object::Reference(descendant)],
                "ToUnicode" => Object::Reference(to_unicode),
            }
        };
        let (doc, page_id) = page_with_font(font, identity_content(&[1, 2, 3, 4]));
        let scan = scan_page(&doc, page_id).expect("Scan");
        let glyphs: Vec<_> = scan.shows.iter().flat_map(|s| s.glyphs()).collect();
        assert_eq!(glyphs.len(), 4);
        assert_eq!(
            glyphs.iter().map(|g| g.text.as_str()).collect::<String>(),
            "0123"
        );
        for (i, g) in glyphs.iter().enumerate() {
            // 10 pt × 0,6 em = 6,0 pt je Zeichen.
            assert!(
                (g.origin.x - (72.0 + 6.0 * i as f64)).abs() < 1e-6,
                "Glyphe {i} steht bei {}, erwartet {}",
                g.origin.x,
                72.0 + 6.0 * i as f64
            );
        }
    }

    #[test]
    fn a_plain_win_ansi_font_produces_no_warning() {
        let font = |_: &mut Document| {
            dictionary! {
                "Type" => "Font",
                "Subtype" => "Type1",
                "BaseFont" => "Helvetica",
                "Encoding" => "WinAnsiEncoding",
            }
        };
        let content = b"BT /F1 10 Tf 1 0 0 1 72 700 Tm (Kontonummer 4711000) Tj ET".to_vec();
        let (doc, page_id) = page_with_font(font, content);
        let scan = scan_page(&doc, page_id).expect("Scan");
        assert!(
            scan.warnings.is_empty(),
            "unerwartete Warnung: {:?}",
            scan.warnings
        );
    }

    // -----------------------------------------------------------------------
    // Geltungsbereich der Textspiegel
    // -----------------------------------------------------------------------

    /// Scannt einen Rohstrom ohne Fonts und liefert die Spiegel mit ihrem
    /// Geltungsbereich.
    fn mirrors(src: &[u8]) -> Vec<(usize, Vec<usize>)> {
        let doc = Document::with_version("1.5");
        let mut result = ScanResult::default();
        scan_marked_text(
            &doc,
            &ops(src),
            StreamKey::Page,
            None,
            None,
            &mut Budget::default(),
            &mut result,
        );
        result
            .marked
            .iter()
            .map(|m| (m.op_index, m.shows.clone()))
            .collect()
    }

    #[test]
    fn a_bracket_owns_exactly_the_glyphs_it_encloses() {
        // 0:BT 1:Tj 2:BDC 3:Tj 4:EMC 5:Tj 6:ET
        let found = mirrors(b"BT (a) Tj /Span <</ActualText (b)>> BDC (b) Tj EMC (c) Tj ET");
        assert_eq!(found, vec![(2, vec![3])]);
    }

    #[test]
    fn a_bracket_without_a_mirror_is_not_reported() {
        let found = mirrors(b"BT /Span <</MCID 0>> BDC (b) Tj EMC ET");
        assert!(found.is_empty(), "{found:?}");
    }

    /// Ein `/ActualText`, das nur aus Leerraum besteht, spiegelt nichts.
    #[test]
    fn an_empty_mirror_is_not_reported() {
        let found = mirrors(b"BT /Span <</ActualText ( )>> BDC (b) Tj EMC ET");
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_point_inherits_the_enclosing_bracket() {
        // 0:BT 1:BDC 2:Tj 3:DP 4:Tj 5:EMC 6:Tj 7:ET
        let found = mirrors(
            b"BT /Span <</MCID 0>> BDC (a) Tj /Span <</ActualText (x)>> DP (b) Tj EMC (c) Tj ET",
        );
        assert_eq!(found, vec![(3, vec![2, 4])]);
    }

    /// Ohne umschließende Klammer gilt das Textobjekt — und zwar ganz, auch
    /// was **vor** dem Punkt gesetzt wurde: ein Punkt beschreibt die Stelle, an
    /// der er steht, nicht einen Bereich dahinter.
    #[test]
    fn a_point_falls_back_to_the_text_object() {
        // 0:BT 1:Tj 2:DP 3:Tj 4:ET 5:BT 6:Tj 7:ET
        let found = mirrors(b"BT (a) Tj /Span <</ActualText (x)>> DP (b) Tj ET BT (c) Tj ET");
        assert_eq!(found, vec![(2, vec![1, 3])]);
    }

    /// Außerhalb jedes Textobjekts bleibt nur der ganze Strom. Das ist die
    /// sichere Richtung: lieber ein Spiegel zu viel entfernt als einer, der
    /// weiterhin das Geheimnis nennt.
    #[test]
    fn a_point_outside_any_text_object_covers_the_whole_stream() {
        let found = mirrors(b"/Span <</ActualText (x)>> DP BT (a) Tj ET BT (b) Tj ET");
        assert_eq!(found, vec![(0, vec![2, 5])]);
    }

    /// Eine nicht geschlossene Klammer reicht bis zum Stromende.
    #[test]
    fn an_unclosed_bracket_reaches_to_the_end() {
        let found = mirrors(b"BT /Span <</ActualText (x)>> BDC (a) Tj (b) Tj ET");
        assert_eq!(found, vec![(1, vec![2, 3])]);
    }
}
