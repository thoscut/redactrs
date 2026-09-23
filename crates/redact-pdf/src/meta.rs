//! Metadaten und Restdaten entfernen.
//!
//! Ein geschwärztes PDF nützt wenig, wenn im `/Info`-Dictionary noch
//! „Kontoauszug_Mustermann_DE89…“ als Titel steht — oder wenn derselbe Text im
//! Wert eines Formularfeldes, in einem Dateianhang oder in einem
//! JavaScript-Schnipsel weiterlebt. Der Content-Stream ist nur *eine* von
//! mehreren Stellen, an denen ein Geheimnis in einer PDF-Datei steht.
//!
//! ## Was tatsächlich entfernt wird
//!
//! Aus dem **Trailer**:
//!
//! * das komplette `/Info`-Dictionary. Jeder andere Schlüssel als `/Root`,
//!   `/Info`, `/Encrypt`, `/ID` und `/Size` fällt beim Schreiben
//!   ([`crate::document::save_to_bytes`]): der Trailer ist eine Wurzel der
//!   Erreichbarkeitsprüfung, und ein Objekt unter einem erfundenen Schlüssel
//!   (`<< /Zusatz 7 0 R >>`) überlebte sonst jedes Aufräumen.
//!
//! Aus dem **Katalog**:
//!
//! * der XMP-Metadatenstrom `/Metadata`,
//! * `/PieceInfo` (anwendungsspezifische Zusatzdaten),
//! * die Dokumentstruktur — `/StructTreeRoot` samt `/MarkInfo`; der `/K`-Baum
//!   darunter (mit `/ActualText` und `/Alt`, die den Seitentext spiegeln)
//!   verwaist damit **in aller Regel** — aber nicht immer: zeigt ein `/IRT`
//!   einer Annotation auf ein Struktur-Element, hält dieser Verweis es über
//!   das Aufräumen hinweg am Leben. Der Trägerlauf läuft `/IRT` ohnehin ab
//!   und nimmt jedem Dictionary, das er dabei erreicht, `/Alt` und
//!   `/ActualText` (siehe unten). Was **kein** Weg von einer Annotation
//!   erreicht, bleibt unberührt — und genau das ist die Grenze: hält ein
//!   Objekt außerhalb dieses Laufs ein Struktur-Element am Leben (`<< /Zusatz
//!   7 0 R >>` an einer Seite), steht sein `/Alt` weiter in der Datei
//!   (Beleg: `zf_q1_korpus::ein_strukturelement_ohne_annotation_bleibt_unberuehrt`),
//! * der `/Names`-Baum. Er trägt benannte Ziele, **JavaScript**
//!   (`/Names /JavaScript`) und **eingebettete Dateien**
//!   (`/Names /EmbeddedFiles`) — allesamt Texttransporte. Ebenso `/Dests`, der
//!   alte, gleichwertige Weg zu benannten Zielen. Preis: benannte Sprünge
//!   innerhalb des Dokuments funktionieren danach nicht mehr. Das ist die
//!   sichere Richtung,
//! * `/AcroForm`. Zusätzlich werden in **jedem** Feld des Formularbaums die
//!   Werte `/V`, `/DV` und `/RV` gelöscht — ein Widget kann über `/Annots`
//!   erreichbar bleiben, und `/V` trägt dort genau den Text, der aus dem
//!   Content-Stream entfernt wurde. „Jedes“ heißt jedes: der Feldbaum ist
//!   eine Wurzel desselben besuchsgeführten Laufs, der die Annotationen
//!   bereinigt — ohne Tiefengrenze. Gemessen (vor dieser Änderung): ein `/V`
//!   in der Mitte einer 70 Felder langen `/Parent`-Kette blieb stehen, weil
//!   zwei getrennte Vorläufe bei 32 Ebenen abbrachen. Gemessen (danach):
//!   100 000 Felder in 0,40 s, eine Kette aus 1 000 000 Feldern in 1,65 s
//!   (Release, +40 MiB über dem Dokument). `/XFA` (ein vollständiger zweiter
//!   Formulardatensatz als XML) fällt mit dem `/AcroForm`-Dictionary,
//! * `/OpenAction` und `/AA` — Aktionen, die beim Öffnen bzw. bei Ereignissen
//!   laufen. Beide dürfen `/S /JavaScript` mit beliebigem Quelltext sein,
//! * `/OCProperties` — die Verwaltung optionaler Inhalte („Ebenen“),
//! * `/AF` — die zugeordneten Dateien (PDF 2.0, 14.13). So hängt eine
//!   ZUGFeRD-/Factur-X-Rechnung ihre XML-Fassung an: **zusätzlich** zum
//!   `/Names`-Baum, und der eine Halter allein zu entfernen nahm die Datei
//!   nicht mit. Gemessen (vor dieser Änderung): `--check-leaks` fand die
//!   IBAN nach dem Schwärzen an drei Stellen, Rückgabewert 3.
//!
//! Aus **jedem verbliebenen `/OCG`**:
//!
//! * `/Name` (auf einen leeren String gesetzt — das Feld ist Pflicht) und
//!   `/Usage`. `/OCProperties` zu löschen genügt nicht: eine Seite hält ein
//!   `/OCG` über `/Resources /Properties` am Leben, und der Ebenenname ist
//!   frei wählbarer Text.
//!
//! * `/Outlines` — die Lesezeichen. Jeder Eintrag trägt einen `/Title`, und
//!   der ist frei wählbarer Text: „Kontoauszug DE89 …“ ist ein Lesezeichen,
//!   wie es jeder Erzeuger schreibt. Gemessen (vor dieser Änderung): eine Datei mit der
//!   IBAN allein im Lesezeichen endete mit 0 Treffern, Rückgabewert 0, und
//!   `--check-leaks` fand sie. Preis: die Gliederung im Betrachter ist weg.
//!   Zusätzlich verliert **jeder Eintrag** seinen `/Title` und seine
//!   Aktionen an Ort und Stelle: den Baum abzuhängen genügt nicht, wenn noch
//!   etwas anderes einen Eintrag hält — der überlebt das Aufräumen sonst
//!   samt Titel, und der Bericht meldete ihn trotzdem als entfernt.
//!
//! Aus **jeder Seite**:
//!
//! * `/Metadata` (seitenweites XMP), `/PieceInfo`, `/StructParents`, `/AA`,
//!   `/AF`,
//! * Annotationen vom Typ `/FileAttachment` — ein Dateianhang hängt nicht nur
//!   im `/Names`-Baum, er kann auch direkt an einer Seite kleben. Aus
//!   `/Annots` gestrichen zu werden genügt ihm nicht: sein `/Popup` hält ihn
//!   über `/Parent` am Leben (so schreibt Acrobat jeden Kommentar), eine
//!   Antwort über `/IRT` genauso. Deshalb fallen `/FS` und `/AF` am Objekt
//!   selbst. Gemessen (vor dieser Änderung): eine Datei mit der IBAN in der
//!   eingebetteten Datei endete mit „2 Kommentartexte“ und Rückgabewert 0,
//!   während `--check-leaks` an der Ausgabe drei Fundstellen fand.
//!
//! Aus **jedem übrigen Objekt**:
//!
//! * `/Metadata` und `/PieceInfo` — beide hängen nicht nur am Katalog und an
//!   den Seiten: XMP klebt an jedem **Bild-XObject**, das ein Layout-Programm
//!   platziert hat (mit `dc:description`, Kamerabesitzer, Aufnahmeort), und
//!   `/PieceInfo` (Tabelle 95) an jedem **Form-XObject**, das ein
//!   Zeichenprogramm geschrieben hat. Ein Strom ist immer ein eigenes Objekt;
//!   ein Durchgang durch die Objekte erreicht sie alle,
//! * `/AF` — aus demselben Grund.
//!
//! Aus **jeder verbliebenen Annotation** — und aus allem, was sie erreichbar
//! hält: ihrem `/Popup`, der `/Parent`-Kette nach oben (das Feld einer
//! Radiogruppe, das Feld hinter mehreren Widgets), `/Kids` nach unten und
//! `/IRT` (Antwortkette). Ein Träger, den nur `/AcroForm` erreichbar machte,
//! fällt mit dem Formular; einen, den ein Widget in `/Annots` über `/Parent`
//! hält, überlebt es — und der trägt `/T`, `/TU`, `/Opt` und `/AA` genauso
//! wie das Widget selbst. Führt ihn keine Seite in `/Annots`, verliert er
//! außerdem sein Erscheinungsbild: niemand zeichnet es, und niemand liest es
//! (Register #90, siehe `shown_carriers`). Gemessen (vor dieser Änderung): `/Contents` eines
//! nur über `/Popup` erreichbaren Popups, `/T` und `/TU` am Elternfeld,
//! `/Opt` am Feld und am Widget, `/AA /K /JS` am Elternfeld, `/MK /CA`,
//! `/OverlayText`, `/PA`, `/NM` und `/DS` an der Annotation — alle mit
//! Rückgabewert 0 überlebt. Entfernt wird an jedem erreichten Dictionary:
//!
//! * die Aktionen `/A`, `/AA` und `/PA` (die URI-Aktion eines Links) und ein
//!   benanntes `/Dest`. Ein Link fern des Textes trägt seinen Klartext in
//!   `/URI` (`mailto:…?subject=DE89 …`), in `/F` (`/GoToR`, `/Launch`:
//!   `Kontoauszug_DE89….pdf`) oder in `/JS`; ein benanntes Ziel ist eine
//!   Zeichenkette. Gemessen: alle drei Formen überlebten die Schwärzung mit
//!   Rückgabewert 0. Ein `/Dest` als Feld (`[Seite /XYZ x y z]`) bleibt — es
//!   trägt Zahlen und Verweise, keinen Text. Entschieden wird über das
//!   **Feld**, elementweise aufgelöst: ein `/Dest 12 0 R` wird aufgelöst, und
//!   ein Verweis *im* Feld muss auf ein Seitenobjekt (`/Type /Page`) führen —
//!   `[12 0 R /XYZ …]` mit einer Zeichenkette in Objekt 12 ist kein Ziel,
//!   sondern ein Versteck. Und eine Seite, auf die so ein Ziel (oder das
//!   `/P` einer Annotation) führt, die aber **nicht im Seitenbaum** hängt,
//!   trug ihren ganzen Content-Stream ungeschwärzt in die Ausgabe — der
//!   Schwärzungslauf kennt nur die Seiten des Baums (Spur-A-Runde 1,
//!   Register #69). Sie wird jetzt geleert, siehe `empty_orphan_pages`.
//!   Preis: Verweise ins Netz und in andere Dateien funktionieren danach
//!   nicht mehr,
//! * die Klartexte `/Contents`, `/RC`, `/T` (Verfasser; an einem Feld der
//!   Feldname) und `/Subj`, dazu `/TU` (der alternative Feldname — das ist
//!   der Tooltip, den der Betrachter beim Überfahren zeigt), `/TM` (der
//!   Exportname; PDF 32000-1, 12.7.3.1, Tabelle 220), `/Opt` (die
//!   Auswahltexte einer Liste, Tabelle 231), `/OverlayText` (Redact,
//!   Tabelle 195), `/NM` (der Annotationsname), `/DS` (die Stilangabe eines
//!   FreeText) und die Beschriftungen `/MK /CA`, `/MK /RC`, `/MK /AC`
//!   (Tabelle 189). Gemessen: eine Notiz mit Symbol-Erscheinungsstrom trug
//!   die IBAN in `/Contents`, ein `/T` und ein `/RC` neben einem
//!   Erscheinungsstrom ebenso — alle mit Rückgabewert 0. Was eine Annotation
//!   **zeichnet** (`/AP`), geht wie Seitentext durch die Schwärzung und
//!   bleibt; was sie daneben als Klartext mitführt, hat keine
//!   Glyphengeometrie und kann nicht anteilig geschwärzt werden. Das ist
//!   dieselbe Entscheidung wie bei den Feldwerten — und dieselbe, die
//!   Acrobats „Dokument bereinigen“ trifft.
//! * der Spiegeltext `/Alt` und `/ActualText` — an **jedem** erreichten
//!   Dictionary, auch an einem, das kein Träger ist (ein `/StructElem` hinter
//!   `/IRT`). Beides ist frei wählbarer Text, der gewöhnlich genau das
//!   spiegelt, was gerade aus dem Strom verschwunden ist.
//! * das Beiwerk `/Movie`, `/Measure`, `/RichMediaContent`,
//!   `/RichMediaSettings`, `/3DD`, `/3DV`, `/3DU` und `/RO` — als Ganzes, siehe
//!   [`ANNOTATION_PLATE_KEYS`]. Gemessen (vor
//!   dieser Änderung): der Dateiname in `/Movie /F`, die
//!   Einheitenbeschriftung in `/Measure /X[0] /U`, eine eingebettete Datei
//!   unter `/RichMediaContent /Assets`, der JavaScript-Strom unter
//!   `/3DD /OnInstantiate`, der Ansichtsname `/3DV /XN` und der Text im
//!   Form-XObject `/RO` einer Redact-Annotation überlebten alle mit
//!   Rückgabewert 0 („Metadaten: nichts zu entfernen“), während
//!   `--check-leaks` sie fand,
//! * die Dateiverweise `/AF` und `/FS` ([`FILE_SPEC_KEYS`]) — beide zeigen
//!   auf ein Filespec mit Dateinamen und eingebetteter Datei.
//!
//! **Benannte Lücke:** `/DA` (Default Appearance) bleibt. Es ist bei FreeText
//! und Widgets Pflicht und eine Operatorfolge (`/Helv 12 Tf 0 g`), keine
//! Zeichenkette für Menschen — wer eine IBAN als Schriftnamen in `/DA`
//! schreibt, kommt durch. Ein Dictionary mit einem fremden `/Type` (Seite,
//! Seitenbaum) ist kein Träger und wird weder bereinigt noch abgelaufen: eine
//! kaputte `/Parent`-Kette, die auf die Seite führt, darf ihr nicht
//! `/Contents` nehmen.
//!
//! ## Was hier bewusst *nicht* passiert
//!
//! * Objekte werden nie einzeln gelöscht — nur Schlüssel. Was dadurch
//!   unerreichbar wird, räumt [`crate::document::prune_unreachable`] am Ende
//!   dieses Laufs weg (und noch einmal beim Speichern): `lopdf` schreibt
//!   alles, was in `doc.objects` steht — Erreichbarkeit interessiert den
//!   Writer nicht. Gemessen (vor dieser Änderung): das Löschen je Schlüssel
//!   traf auch geteilte Objekte — `/Contents 4 0 R` an einer Notiz auf den
//!   Seiteninhalt nahm der Seite ihren Inhalt, ein `/Outlines /First` auf die
//!   Seite löschte die Seite („enthält keine Seiten“).
//! * Der Inhalt einer Ebene wird nicht angerührt. Entfernt wird ihr *Name*,
//!   nicht der Text, den sie zeichnet — der geht denselben Weg wie jeder
//!   andere Seiteninhalt durch [`crate::redact`].
//! * Annotationen außerhalb eines Schwärzungsbereichs bleiben stehen (das
//!   entscheidet [`crate::redact`]), ihre Appearance-Streams also auch.

use std::collections::{BTreeMap, BTreeSet};

use lopdf::{Dictionary, Document, Object, ObjectId, StringFormat};

/// Welche Metadaten und Restdaten entfernt wurden.
///
/// Jeder Eintrag ist ein *gemessenes* Ergebnis, kein Vorsatz — das Audit-Log
/// übernimmt die Zahlen unverändert. „Entfernt“ heißt dabei zweierlei, und
/// welches von beidem, hängt daran, was der Zähler behauptet:
///
/// * **Ein Schlüssel ist weg** — `info_removed`, `xmp_removed`,
///   `piece_info_removed`, `struct_tree_removed`, `names_removed`,
///   `acroform_removed`, `open_action_removed`,
///   `additional_actions_removed`, `optional_content_removed`,
///   `file_attachments_removed`, `file_specs_removed`,
///   `annotation_actions_removed`,
///   `annotation_texts_cleared`, `field_values_cleared`,
///   `optional_content_names_cleared`. Der Schlüssel steht so in der
///   Ausgabe: nicht mehr da. Ein Schlüssel mit dem Wert `null` zählt nicht
///   (PDF 32000-1, 7.3.9: gleich einem fehlenden Schlüssel). Und **stand
///   dort ein Verweis**, zählt er nur, wenn das Objekt dahinter nach dem
///   Aufräumen wirklich fehlt: `/Contents 4 0 R` an einer Notiz, das noch
///   jemand anders hält, war sonst „1 Kommentartext entfernt“ über einen
///   Text, den `--check-leaks` unverändert fand (siehe [`Tally`]).
/// * **Die Nutzlast ist weg** — `embedded_files_removed`,
///   `javascript_removed`, `xfa_removed`, `outlines_removed`. Diese vier
///   behaupten, ein *Inhalt* sei aus der Datei verschwunden; gezählt wird
///   deshalb erst **nach** `prune_unreachable` und nur, was dann wirklich
///   fehlt. Hält ein zweiter Verweis den Anhang, den XFA-Datensatz oder das
///   Lesezeichen am Leben, meldet der Bericht ihn nicht als entfernt.
///   Beim Lesezeichen genügt, dass sein Text fiel — der Eintrag darf als
///   leeres Gerüst stehen bleiben; sein `/Title` muss dafür aber **wirklich**
///   weg sein: ein `/Title 4 0 R`, den ein zweiter Halter am Leben hält,
///   macht den Eintrag zum Leck, nicht zur Entfernung.
///
/// Der Maßstab ist `--check-leaks` an der geschriebenen Datei: keine Zahl
/// hier darf eine Entfernung melden, die dort noch zu finden ist. Der Satz
/// gilt für **jede** Zahl, nicht nur für die vier Nutzlast-Zähler. Die
/// Gegenrichtung ist er nicht: eine Zahl darf zu klein sein — eine
/// `/FileAttachment`-Annotation ohne Datei etwa zählt nicht mit.
///
/// Gemessen wird am **aufgelösten** Objekt: hinter `/Contents 4 0 R` kann
/// Objekt 4 selbst nur `5 0 R` sein. Fällt dann 4 und hält ein zweiter
/// Verweis die Zeichenkette 5, ist nichts entfernt (siehe [`Chains`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetadataReport {
    pub info_removed: bool,
    pub xmp_removed: bool,
    pub piece_info_removed: usize,
    pub struct_tree_removed: bool,
    /// `/Names`-Bäume (benannte Ziele, JavaScript, eingebettete Dateien).
    pub names_removed: usize,
    /// Einträge unterhalb von `/Names /EmbeddedFiles`, deren Teilbaum nach
    /// dem Aufräumen vollständig aus der Datei verschwunden ist.
    pub embedded_files_removed: usize,
    /// Einträge unterhalb von `/Names /JavaScript` — gezählt wie
    /// [`Self::embedded_files_removed`].
    pub javascript_removed: usize,
    /// `/AcroForm` aus dem Katalog entfernt.
    pub acroform_removed: bool,
    /// `/XFA` war vorhanden und ist nach dem Lauf nicht mehr in der Datei
    /// (es fällt mit dem `/AcroForm`-Dictionary — es sei denn, ein zweiter
    /// Verweis hält den Datensatz).
    pub xfa_removed: bool,
    /// Gelöschte Feldwerte (`/V`, `/DV`, `/RV`) über alle Felder und Widgets,
    /// die von einer Seite oder vom Formularbaum aus erreichbar sind — ohne
    /// Tiefengrenze.
    pub field_values_cleared: usize,
    /// Dateianhänge: `/FileAttachment`-Annotationen, die aus der Seite
    /// gestrichen wurden **und** deren Datei (`/FS`, `/AF`) danach nicht mehr
    /// in der Datei steht — je Annotation einer.
    ///
    /// Aus `/Annots` gestrichen werden sie alle; gezählt wird nur, wessen
    /// Nutzlast wirklich fiel. Ein Anhang, dessen Filespec ein zweiter
    /// Verweis am Leben hält, zählt nicht (`--check-leaks` findet ihn), und
    /// eine `/FileAttachment`-Annotation ohne Datei zählt nicht mit — sie
    /// trug nichts.
    pub file_attachments_removed: usize,
    /// Dateiverweise, die **nicht** an einer Seitenannotation hingen: `/AF`
    /// (zugeordnete Datei, PDF 2.0, 14.13) am Katalog, an einer Seite, an
    /// einem XObject oder an einem Träger, und ein `/FS` an einem Träger,
    /// den keine Seite in `/Annots` führte.
    ///
    /// `/AF` ist der Weg, auf dem eine ZUGFeRD-/Factur-X-Rechnung ihre
    /// XML-Fassung anhängt: `/Names /EmbeddedFiles` zu entfernen nahm ihr
    /// bisher nur den einen von zwei Haltern.
    pub file_specs_removed: usize,
    pub open_action_removed: bool,
    /// `/AA`-Dictionaries aus Katalog und Seiten.
    pub additional_actions_removed: usize,
    pub optional_content_removed: bool,
    /// Ebenennamen (`/OCG /Name`), die aus der Datei entfernt wurden.
    ///
    /// `/OCProperties` zu löschen genügt nicht: eine Seite kann dasselbe
    /// `/OCG` über `/Resources /Properties` weiter referenzieren, und dann
    /// bleibt sein `/Name` in der Datei stehen — frei wählbarer Text, der
    /// denselben Klartext tragen kann, der gerade aus dem Strom entfernt
    /// wurde.
    pub optional_content_names_cleared: usize,
    /// Lesezeichen (`/Outlines`-Einträge), die nach dem Lauf keinen Text mehr
    /// tragen: der Eintrag steht nicht mehr in der Datei, oder sein `/Title`
    /// (und seine Aktionen) fielen an Ort und Stelle — und nichts davon hängt
    /// noch als eigenes Objekt in der Datei.
    pub outlines_removed: usize,
    /// `/A`, `/AA`, `/PA` und benannte `/Dest` an Annotationen und an
    /// allem, was sie erreichbar halten (`/Popup`, `/Parent`, `/Kids`,
    /// `/IRT`).
    pub annotation_actions_removed: usize,
    /// Klartexte an Annotationen und erreichbaren Feldern — je Schlüssel
    /// einer: `/Contents`, `/RC`, `/T`, `/Subj`, `/TU`, `/TM`, `/Opt`,
    /// `/OverlayText`, `/NM`, `/DS` sowie `/CA`, `/RC`, `/AC` in `/MK`,
    /// dazu `/Alt` und `/ActualText` an jedem erreichten Dictionary und das
    /// Beiwerk `/Movie`, `/Measure`, `/RichMediaContent`,
    /// `/RichMediaSettings`, `/3DD`, `/3DV`, `/3DU`, `/RO` sowie `/SV` (Seed-Value eines Signaturfelds: `/Reasons`,
    /// `/LegalAttestation`) und `/Lock` (`/Fields`).
    pub annotation_texts_cleared: usize,
    /// Seiten **außerhalb des Seitenbaums**, die ihren Inhalt verloren haben.
    ///
    /// Eine Seite, die nicht in `/Kids` hängt — gelöscht, aber von einem
    /// stehen gebliebenen Verweis gehalten (`/Dest [7 0 R /Fit]` eines Links,
    /// `/P` einer Annotation) —, kennt `get_pages` nicht: sie geht durch keine
    /// Schwärzung, keine Warnung, und `prune_unreachable` behält sie, weil sie
    /// erreichbar ist. Gefunden hat das die Spur-A-Runde 1 (Register #69): ihr
    /// Content-Stream stand ungeschwärzt in der Ausgabe. Jetzt verliert jede
    /// solche Seite `/Contents`, `/Annots`, `/Resources` und ihr Beiwerk — der
    /// Verweis führt danach auf eine leere Seite.
    pub orphan_pages_emptied: usize,
    /// Vorschaubilder (`/Thumb`, PDF 32000-1 Tabelle 30) — je Seite eines.
    ///
    /// Ein Raster der Seite, wie sie **vor** der Schwärzung aussah: ein
    /// Bild-XObject, das kein `Do` zeichnet und darum am Bildlauf vorbeiging.
    /// Die Spur-A-Runde 1 fand die Klartext-Bildpunkte darin nach dem Lauf
    /// unverändert wieder (Register #74). Ein Vorschaubild einer geschwärzten
    /// Seite ist ohnehin falsch; es fällt, und der Betrachter rechnet sich
    /// eines aus dem Inhalt aus.
    pub thumbnails_removed: usize,
    /// Beiwerk an Katalog, Seiten und Objekten, das Klartext tragen kann und
    /// bis zur Spur-A-Runde 1 (Register #70) stehen blieb — je Schlüssel
    /// einer: am Katalog `/Perms` (hält das Signatur-Dictionary samt
    /// `/Reason`, `/Location`, `/ContactInfo`), `/DSS` (Zertifikate mit
    /// Unterzeichnernamen), `/Threads` (Artikel mit `/I /Title`),
    /// `/Collection` (Portfolio-Schema), `/URI` (`/Base`), `/DPartRoot`
    /// (PDF/VT-Metadaten `/DPM` — im Kontoauszugdruck Name und Konto des
    /// Empfängers), die Präfixe `/P` in `/PageLabels` und die Texte `/Info`,
    /// `/OutputCondition`, `/RegistryName` in `/OutputIntents` (am Katalog
    /// und an der Seite); an Seiten `/B`, `/VP`, `/PresSteps`, `/DPart`; an
    /// Objekten `/Ref` (ein Referenz-XObject hält eine eingebettete Datei)
    /// und `/OPI`. Dazu seit der Spur-A-Runde 2 jeder Schlüssel an Katalog,
    /// Seitenbaum und Seite, der nicht auf der Erlaubnisliste steht
    /// ([`CATALOG_KEEP`], [`PAGE_KEEP`], [`PAGE_TREE_KEEP`]) — etwa
    /// `/SpiderInfo`, `/Legal`, `/Requirements` (Register #92).
    pub beiwerk_removed: usize,
}

impl MetadataReport {
    pub fn anything_removed(&self) -> bool {
        !self.summary().is_empty()
    }

    /// Klartextzeilen für Konsole und Audit-Log — eine je tatsächlich
    /// durchgeführter Entfernung.
    pub fn summary(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut flag = |cond: bool, text: &str| {
            if cond {
                out.push(text.to_string());
            }
        };
        flag(self.info_removed, "/Info-Dictionary");
        flag(self.xmp_removed, "XMP-Metadaten (/Metadata)");
        flag(
            self.struct_tree_removed,
            "Dokumentstruktur (/StructTreeRoot)",
        );
        flag(self.acroform_removed, "Formulardefinition (/AcroForm)");
        flag(self.xfa_removed, "XFA-Formulardaten (/XFA)");
        flag(self.open_action_removed, "Öffnen-Aktion (/OpenAction)");
        flag(self.optional_content_removed, "Ebenen (/OCProperties)");

        let mut count = |n: usize, singular: &str, plural: &str| {
            if n == 1 {
                out.push(format!("1 {singular}"));
            } else if n > 1 {
                out.push(format!("{n} {plural}"));
            }
        };
        count(self.piece_info_removed, "/PieceInfo", "/PieceInfo-Blöcke");
        count(self.names_removed, "/Names-Baum", "/Names-Bäume");
        count(
            self.embedded_files_removed,
            "eingebettete Datei",
            "eingebettete Dateien",
        );
        count(
            self.javascript_removed,
            "JavaScript-Eintrag",
            "JavaScript-Einträge",
        );
        count(self.field_values_cleared, "Feldwert", "Feldwerte");
        count(
            self.file_attachments_removed,
            "Dateianhang-Annotation",
            "Dateianhang-Annotationen",
        );
        count(
            self.file_specs_removed,
            "Dateiverweis (/AF, /FS)",
            "Dateiverweise (/AF, /FS)",
        );
        count(
            self.additional_actions_removed,
            "Ereignisaktion (/AA)",
            "Ereignisaktionen (/AA)",
        );
        count(
            self.optional_content_names_cleared,
            "Ebenenname (/OCG /Name)",
            "Ebenennamen (/OCG /Name)",
        );
        count(
            self.outlines_removed,
            "Lesezeichen (/Outlines)",
            "Lesezeichen (/Outlines)",
        );
        count(
            self.annotation_actions_removed,
            "Aktion oder benanntes Ziel an einer Annotation (/A, /AA, /PA, /Dest)",
            "Aktionen oder benannte Ziele an Annotationen (/A, /AA, /PA, /Dest)",
        );
        count(
            self.annotation_texts_cleared,
            "Kommentartext an einer Annotation (/Contents, /RC, /T, /Subj, /TU, /TM, /Opt, /OverlayText, /NM, /DS, /MK, /Alt, /ActualText, /Movie, /Measure, /RichMediaContent, /RichMediaSettings, /3DD, /3DV, /3DU, /RO, /SV, /Lock)",
            "Kommentartexte an Annotationen (/Contents, /RC, /T, /Subj, /TU, /TM, /Opt, /OverlayText, /NM, /DS, /MK, /Alt, /ActualText, /Movie, /Measure, /RichMediaContent, /RichMediaSettings, /3DD, /3DV, /3DU, /RO, /SV, /Lock)",
        );
        count(
            self.beiwerk_removed,
            "Beiwerk mit Klartext (/Perms, /DSS, /Threads, /B, /Collection, /URI, /DPartRoot, /DPart, /PageLabels-Präfix, /OutputIntents-Text, /VP, /PresSteps, /Ref, /OPI, Schlüssel außerhalb der Erlaubnisliste)",
            "Beiwerk mit Klartext (/Perms, /DSS, /Threads, /B, /Collection, /URI, /DPartRoot, /DPart, /PageLabels-Präfix, /OutputIntents-Text, /VP, /PresSteps, /Ref, /OPI, Schlüssel außerhalb der Erlaubnisliste)",
        );
        count(
            self.thumbnails_removed,
            "Vorschaubild (/Thumb)",
            "Vorschaubilder (/Thumb)",
        );
        count(
            self.orphan_pages_emptied,
            "Seite außerhalb des Seitenbaums geleert",
            "Seiten außerhalb des Seitenbaums geleert",
        );
        out
    }
}

/// Entfernt alle Dokument-Metadaten und die im Modulkommentar genannten
/// Restdaten.
pub fn strip_metadata(doc: &mut Document) -> MetadataReport {
    let mut report = MetadataReport::default();

    // Jede Entfernung wird gebucht, nicht sofort gezählt: was hinter einem
    // Verweis steht, fällt erst mit `prune_unreachable` — und nur, wenn es
    // niemand sonst hält (siehe [`Tally`]). Gezählt wird deshalb ganz am
    // Ende, an der aufgeräumten Datei.
    let mut info = Tally::default();
    let mut xmp = Tally::default();
    let mut piece_info = Tally::default();
    let mut thumbs = Tally::default();
    let mut beiwerk = Tally::default();
    let mut struct_tree = Tally::default();
    let mut names = Tally::default();
    let mut acroform = Tally::default();
    let mut open_action = Tally::default();
    let mut additional_actions = Tally::default();
    let mut optional_content = Tally::default();
    let mut file_specs = Tally::default();
    let mut attachments: Vec<Tally> = Vec::new();

    // Die Verweisketten des geladenen Dokuments — vor jeder Änderung, danach
    // sind sie nicht mehr zu sehen (siehe [`Chains`]).
    let chains = Chains::of(doc);

    // --- Trailer /Info ---
    info.book(doc.trailer.remove(b"Info"));

    let catalog_id = doc.trailer.get(b"Root").ok().and_then(|o| match o {
        Object::Reference(id) => Some(*id),
        _ => None,
    });
    let page_ids: Vec<ObjectId> = doc.get_pages().values().copied().collect();

    // --- Erst vermessen, dann verändern ---------------------------------
    //
    // Was in `/Names` steckt, welche Felder am Formular hängen und welche
    // Einträge der Lesezeichenbaum trägt, lässt sich nur solange feststellen,
    // wie die Referenzen noch stehen. Gezählt wird deshalb hier — *gemeldet*
    // erst nach dem Aufräumen, und nur, was dann wirklich fehlt (siehe
    // [`Payload`]).
    let mut embedded_files = Payload::default();
    let mut javascript = Payload::default();
    let mut xfa = Payload::default();
    let mut form_fields: Option<Object> = None;
    if let Some(catalog_id) = catalog_id {
        if let Ok(catalog) = doc.get_dictionary(catalog_id) {
            if let Some(names) = value_of(catalog, b"Names") {
                let holder = names.as_reference().ok();
                if let Some(dict) = resolve(doc, names).and_then(|o| o.as_dict().ok()) {
                    embedded_files = Payload::of(doc, dict, b"EmbeddedFiles", holder);
                    javascript = Payload::of(doc, dict, b"JavaScript", holder);
                }
            }
            if let Some(acroform) = value_of(catalog, b"AcroForm") {
                let holder = acroform.as_reference().ok();
                if let Some(dict) = resolve(doc, acroform).and_then(|o| o.as_dict().ok()) {
                    xfa = Payload::of(doc, dict, b"XFA", holder);
                    // Der Feldbaum wird abgelaufen wie die Annotationen einer
                    // Seite — mit derselben Besuchsmenge, ohne Tiefengrenze.
                    form_fields = value_of(dict, b"Fields").cloned();
                }
            }
        }
    }
    // Lesezeichen: die Einträge stehen fest, solange der Baum steht.
    let outline_items = catalog_id
        .and_then(|id| doc.get_dictionary(id).ok())
        .and_then(|catalog| value_of(catalog, b"Outlines"))
        .map(|outlines| collect_outline_items(doc, outlines))
        .unwrap_or_default();

    // --- Katalog --------------------------------------------------------
    if let Some(catalog_id) = catalog_id {
        if let Ok(catalog) = doc.get_dictionary_mut(catalog_id) {
            take(catalog, b"Metadata", &mut xmp);
            take(catalog, b"PieceInfo", &mut piece_info);
            take(catalog, b"StructTreeRoot", &mut struct_tree);
            catalog.remove(b"MarkInfo");
            // `/Names` — benannte Ziele, JavaScript, eingebettete Dateien.
            take(catalog, b"Names", &mut names);
            // `/Dests` ist der alte, gleichwertige Weg zu benannten Zielen.
            take(catalog, b"Dests", &mut names);
            if !take(catalog, b"AcroForm", &mut acroform) {
                // Ohne `/AcroForm` gibt es auch kein `/XFA`.
                xfa = Payload::default();
            }
            take(catalog, b"OpenAction", &mut open_action);
            take(catalog, b"AA", &mut additional_actions);
            take(catalog, b"OCProperties", &mut optional_content);
            catalog.remove(b"Outlines");
            // Beiwerk mit Klartext, bis zur Spur-A-Runde 1 übersehen
            // (Register #70). `/Perms` hält das Signatur-Dictionary ein
            // zweites Mal — der Feldwert fällt, der Halter hier nicht.
            for key in CATALOG_BEIWERK_KEYS {
                take(catalog, key, &mut beiwerk);
            }
            // Und alles, was die Aufzählung nicht kennt (Register #92).
            take_unlisted(catalog, &CATALOG_KEEP, &mut beiwerk);
        }
        clean_page_labels(doc, catalog_id, &mut beiwerk);
        clean_output_intents(doc, catalog_id, &mut beiwerk);
        for node in page_tree_nodes(doc, catalog_id, &page_ids) {
            if let Ok(node) = doc.get_dictionary_mut(node) {
                take_unlisted(node, &PAGE_TREE_KEEP, &mut beiwerk);
            }
        }
    }

    // --- Träger: Annotationen, Felder, Feldwerte ------------------------
    //
    // Die Besuchsmenge gilt für das ganze Dokument: ein Feld, dessen Widgets
    // auf zwei Seiten liegen, wird einmal bereinigt und einmal gezählt. Der
    // Feldbaum des Formulars ist eine zweite Wurzel desselben Laufs — ein
    // Feld, das nur dort hängt, hat dieselben Klartexte und denselben Wert.
    let mut visited: BTreeSet<ObjectId> = BTreeSet::new();
    let mut cleaned = Cleaned::default();
    // Was eine Seite zeigt, steht fest, bevor der erste Träger bereinigt
    // wird — sonst hinge es an der Reihenfolge der Seiten, ob ein Widget,
    // das eine andere Seite über `/Parent` und `/Kids` erreicht, als gezeigt
    // gilt (Register #90).
    let shown = shown_carriers(doc, &page_ids);
    let mut captions: BTreeMap<ObjectId, bool> = BTreeMap::new();
    // Ein `/Annots`, das sich Seiten teilen, wird einmal an seinem Objekt
    // bereinigt (Register #94).
    let mut attachment_arrays: BTreeSet<ObjectId> = BTreeSet::new();
    for page_id in &page_ids {
        attachments.extend(remove_file_attachments(
            doc,
            *page_id,
            &mut attachment_arrays,
        ));
        if let Ok(page) = doc.get_dictionary_mut(*page_id) {
            take(page, b"PieceInfo", &mut piece_info);
            page.remove(b"StructParents");
            take(page, b"Metadata", &mut xmp);
            take(page, b"AA", &mut additional_actions);
            // Das Vorschaubild zeigt die Seite von vorher (Register #74).
            take(page, b"Thumb", &mut thumbs);
            for key in PAGE_BEIWERK_KEYS {
                take(page, key, &mut beiwerk);
            }
            // Und alles, was die Aufzählung nicht kennt (Register #92).
            take_unlisted(page, &PAGE_KEEP, &mut beiwerk);
        }
        clean_output_intents(doc, *page_id, &mut beiwerk);
        cleaned += clean_annotations(doc, *page_id, &mut visited, &shown, &mut captions);
    }
    if let Some(mut fields) = form_fields {
        // Der Feldbaum zuletzt: was von einer Seite aus erreichbar ist, ist
        // dann schon bereinigt und gezählt. Übrig bleiben die Felder, die nur
        // das Formular erreichbar hielt — sie fallen mit ihm und stehen im
        // Bericht unter `/AcroForm`, nicht noch einmal als Annotationstext.
        // Gezählt wird von ihnen allein der Feldwert: `/V` ist der Text, der
        // gerade aus dem Content-Stream entfernt wurde, und der Zähler dafür
        // war schon immer der Feldwert-Zähler. Zurückgeschrieben wird der
        // Wurzelwert nicht — er hängt am `/AcroForm`-Dictionary, und dessen
        // Schlüssel ist gerade gefallen.
        cleaned.values +=
            clean_carriers(doc, &mut fields, false, &mut visited, &shown, &mut captions).values;
    }
    // Ein `/MK` als eigenes Objekt — von mehreren Widgets geteilt: einmal
    // bereinigt, nach allen Wurzeln. Seine Symbole verliert es nur, wenn kein
    // gezeigtes Widget es benutzt.
    for (mk, benutzt_gezeigt) in captions {
        if let Ok(dict) = doc.get_dictionary_mut(mk) {
            clean_captions(dict, &mut cleaned.texts);
            if !benutzt_gezeigt {
                for key in ICON_KEYS {
                    take(dict, key, &mut cleaned.texts);
                }
            }
        }
    }

    // --- Seiten außerhalb des Seitenbaums --------------------------------
    //
    // Vor dem Durchgang über alle Objekte, damit auch das Beiwerk einer
    // solchen Seite (`/Metadata`, `/PieceInfo`) noch mitgezählt wird.
    report.orphan_pages_emptied = empty_orphan_pages(doc, &page_ids);

    // --- Metadaten an jedem übrigen Objekt -------------------------------
    //
    // XMP und `/PieceInfo` hängen nicht nur am Katalog und an den Seiten,
    // und `/AF` ist der zweite Halter jeder eingebetteten Datei.
    clear_object_metadata(
        doc,
        &mut xmp,
        &mut piece_info,
        &mut file_specs,
        &mut beiwerk,
    );

    // --- Lesezeichen ----------------------------------------------------
    //
    // Der Baum hängt nur noch am gefallenen `/Outlines`; ein Eintrag, den
    // zusätzlich jemand anders hält, überlebt das Aufräumen aber — samt
    // `/Title`. Deshalb wird jeder Eintrag bereinigt wie ein Träger.
    let outlines_cleared = clear_outline_items(doc, &outline_items);

    // --- Aufräumen ------------------------------------------------------
    //
    // Bis hierher sind nur Schlüssel gefallen. Alles, was dadurch niemand
    // mehr referenziert (das `/Info`-Objekt, der `/Names`-Baum, jeder
    // Lesezeichen-Eintrag, ein `/RC`-Strom), fällt jetzt — und ein geteiltes
    // Objekt (`/Contents 4 0 R` auf den Seiteninhalt) bleibt, weil die Seite
    // es weiterhin hält.
    crate::document::prune_unreachable(doc);

    // --- Ebenennamen ----------------------------------------------------
    //
    // Erst *nach* dem Aufräumen: was mit `/OCProperties` verschwunden ist,
    // wird hier weder angefasst noch gezählt. Was übrig bleibt, ist genau der
    // Fall, den dieses Modul bis Aufgabe #57 offen gelassen hat.
    let ocg_names = clear_optional_content_names(doc);

    // Stand ein Ebenenname als eigenes Objekt hinter `/Name 12 0 R`, hält
    // ihn nach dem Ersetzen niemand mehr: ein zweiter Aufräumlauf nimmt ihn
    // mit — und macht die Entfernung damit zählbar. Ohne ihn meldete der
    // Bericht 0, die Zusammenfassung blieb leer, und die Konsole sagte
    // „Metadaten: nichts zu entfernen“ über eine Datei, aus der gerade ein
    // Ebenenname entfernt wurde. Gelaufen wird er nur in diesem Fall; ein
    // Name, der direkt im `/OCG` stand, ist ohnehin sofort weg.
    if !ocg_names.refs.is_empty() {
        crate::document::prune_unreachable(doc);
    }

    // --- Jetzt erst zählen ----------------------------------------------
    //
    // Bis hierher stand nur fest, was *vorhatte* zu verschwinden. Was ein
    // zweiter Halter weiterhin erreichbar macht, steht noch in der Datei und
    // darf nicht als entfernt gemeldet werden.
    report.info_removed = info.settled(doc, &chains) > 0;
    report.xmp_removed = xmp.settled(doc, &chains) > 0;
    report.piece_info_removed = piece_info.settled(doc, &chains);
    report.thumbnails_removed = thumbs.settled(doc, &chains);
    report.beiwerk_removed = beiwerk.settled(doc, &chains);
    report.struct_tree_removed = struct_tree.settled(doc, &chains) > 0;
    report.names_removed = names.settled(doc, &chains);
    report.acroform_removed = acroform.settled(doc, &chains) > 0;
    report.open_action_removed = open_action.settled(doc, &chains) > 0;
    report.additional_actions_removed = additional_actions.settled(doc, &chains);
    report.optional_content_removed = optional_content.settled(doc, &chains) > 0;
    // Ein Dateianhang zählt, wenn seine Datei nach dem Aufräumen fehlt —
    // je Annotation einer, auch wenn sie `/FS` und `/AF` zugleich trug.
    report.file_attachments_removed = attachments
        .iter()
        .filter(|one| one.settled(doc, &chains) > 0)
        .count();
    file_specs += cleaned.files;
    report.file_specs_removed = file_specs.settled(doc, &chains);
    report.annotation_actions_removed = cleaned.actions.settled(doc, &chains);
    report.annotation_texts_cleared = cleaned.texts.settled(doc, &chains);
    report.field_values_cleared = cleaned.values.settled(doc, &chains);
    // Die Ebenennamen sind *ersetzt*, nicht gelöscht, und danach wird nicht
    // mehr aufgeräumt: ein `/Name 12 0 R`, den noch jemand hält, steht weiter
    // in der Datei und zählt deshalb nicht (`settled` sieht das Objekt).
    report.optional_content_names_cleared = ocg_names.settled(doc, &chains);
    report.embedded_files_removed = embedded_files.removed(doc);
    report.javascript_removed = javascript.removed(doc);
    report.xfa_removed = xfa.removed(doc) > 0;
    // Ein Lesezeichen gilt als entfernt, wenn nichts von dem, was es trug,
    // noch in der Datei steht — ein `/Title 4 0 R`, den ein zweiter Halter
    // am Leben hält, macht den Eintrag zum Leck, nicht zur Entfernung.
    report.outlines_removed = outline_items
        .iter()
        .filter(|id| match outlines_cleared.get(*id) {
            Some(cleared) => cleared.alive(doc, &chains) == 0,
            None => !doc.objects.contains_key(*id),
        })
        .count();

    report
}

/// Leert den Klartextnamen jedes verbliebenen `/OCG`-Dictionaries.
///
/// Warum überhaupt: `/OCProperties` aus dem Katalog zu entfernen macht die
/// Ebenenverwaltung unerreichbar, nicht aber die Ebenen selbst. Eine Seite,
/// die ein `/OCG` über `/Resources /Properties` benutzt (so wird `/BDC /OC`
/// aufgelöst), hält es weiterhin am Leben — samt `/Name`, und der ist frei
/// wählbarer Text: „Ebene Mustermann“ ist ein Ebenenname, wie ihn jedes
/// Layout-Programm schreibt.
///
/// Warum leeren statt löschen: `/Name` ist bei `/OCG` ein Pflichtfeld. Ein
/// leerer String hält die Datei regelkonform und trägt nichts mehr.
///
/// Gesucht wird in **allen** Objekten und rekursiv auch in direkt
/// eingebetteten Dictionaries — ein `/OCG` muss kein indirektes Objekt sein,
/// und über welchen Weg es erreichbar ist, spielt für den Klartext keine
/// Rolle. `/Usage` fällt mit: dort steht unter `/CreatorInfo` ebenfalls frei
/// wählbarer Text.
fn clear_optional_content_names(doc: &mut Document) -> Tally {
    let ids: Vec<ObjectId> = doc.objects.keys().copied().collect();
    let mut cleared = Tally::default();
    for id in ids {
        if let Some(object) = doc.objects.get_mut(&id) {
            clear_ocg_names(object, &mut cleared);
        }
    }
    cleared
}

/// Läuft einen *direkten* Objektbaum ohne Rekursion und ohne Tiefengrenze ab.
/// Eine Grenze hätte einen Ebenennamen hinter genügend Verschachtelung
/// stillschweigend stehen lassen.
fn clear_ocg_names(object: &mut Object, cleared: &mut Tally) {
    let mut stack: Vec<&mut Object> = vec![object];
    while let Some(item) = stack.pop() {
        let dict = match item {
            Object::Dictionary(dict) => dict,
            Object::Stream(stream) => &mut stream.dict,
            Object::Array(items) => {
                stack.extend(items.iter_mut());
                continue;
            }
            _ => continue,
        };
        if dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"OCG") {
            let name = value_of(dict, b"Name").cloned();
            let has_text = name
                .as_ref()
                .is_some_and(|name| !matches!(name, Object::String(bytes, _) if bytes.is_empty()));
            if has_text {
                dict.set("Name", Object::String(Vec::new(), StringFormat::Literal));
                cleared.book(name);
            }
            dict.remove(b"Usage");
        }
        stack.extend(dict.iter_mut().map(|(_, value)| value));
    }
}

// ---------------------------------------------------------------------------
// Hilfsfunktionen
// ---------------------------------------------------------------------------

/// Sammelt die Einträge eines Lesezeichenbaums.
///
/// Gelaufen wird `/First` → `/Next` je Ebene und `/First` in die Tiefe, mit
/// Besuchsmenge: ein Baum, der auf sich selbst zeigt, wäre sonst endlos.
/// Gedeckelt ist der Lauf allein durch die Besuchsmenge — jede Id wird
/// höchstens einmal angefasst, und mehr Ids als Objekte gibt es nicht. Eine
/// Tiefengrenze gab es hier einmal; sie brach mit `break` ab und ließ die
/// Geschwister der oberen Ebenen ungezählt (33 statt 42). Der Stapel wächst
/// je Eintrag um höchstens zwei Einträge, Rekursion gibt es nicht — 100 000
/// Stufen tief enden in Millisekunden.
fn collect_outline_items(doc: &Document, root: &Object) -> Vec<ObjectId> {
    let mut seen: BTreeSet<ObjectId> = BTreeSet::new();
    if let Object::Reference(id) = root {
        seen.insert(*id);
    }
    let mut items: Vec<ObjectId> = Vec::new();
    let Some(first) = resolve(doc, root)
        .and_then(|o| o.as_dict().ok())
        .and_then(|d| value_of(d, b"First"))
    else {
        return items;
    };
    let mut stack: Vec<Object> = vec![first.clone()];
    while let Some(item) = stack.pop() {
        let Object::Reference(id) = item else {
            continue;
        };
        if !seen.insert(id) {
            continue;
        }
        let Ok(dict) = doc.get_dictionary(id) else {
            continue;
        };
        items.push(id);
        for key in [b"Next".as_slice(), b"First".as_slice()] {
            if let Some(link) = value_of(dict, key) {
                stack.push(link.clone());
            }
        }
    }
    items
}

/// Nimmt jedem Lesezeichen seinen `/Title` und seine Aktionen.
///
/// Warum überhaupt: gewöhnlich fällt der ganze Baum mit dem
/// `/Outlines`-Schlüssel des Katalogs und wird weggeräumt. Ein Eintrag, den
/// noch etwas anderes hält, überlebt das aber — und `/Title` ist frei
/// wählbarer Text („Kontoauszug DE89 …“). Ein Zähler, der solche Einträge
/// als entfernt meldet, meldet etwas, das nicht geschah.
///
/// Rückgabe: je Eintrag, an dem etwas fiel, die Buchung — erst nach dem
/// Aufräumen steht fest, ob ein `/Title 4 0 R` wirklich mit fiel.
fn clear_outline_items(doc: &mut Document, items: &[ObjectId]) -> BTreeMap<ObjectId, Cleaned> {
    let mut cleared: BTreeMap<ObjectId, Cleaned> = BTreeMap::new();
    for id in items {
        // Erst lesen (das Ziel entscheidet sich am unveränderten Dokument),
        // dann schreiben.
        let Ok(dict) = doc.get_dictionary(*id) else {
            continue;
        };
        let keep_dest = keeps_destination(doc, dict);
        let Ok(dict) = doc.get_dictionary_mut(*id) else {
            continue;
        };
        // `/Title` ist der Klartext des Eintrags; alles Weitere ist dieselbe
        // Sorte Aktion wie an einer Annotation. Gezählt wird der Eintrag im
        // Lesezeichen-Zähler, nicht zusätzlich bei den Annotationen.
        let mut cleaned = Cleaned::default();
        take(dict, b"Title", &mut cleaned.texts);
        cleaned += clean_carrier(dict, keep_dest);
        if cleaned.any() {
            cleared.insert(*id, cleaned);
        }
    }
    cleared
}

/// Nimmt jeder Annotation der Seite — und allem, was sie erreichbar hält —
/// ihre Aktionen, Klartexte und Feldwerte.
///
/// Eine Annotation steht gewöhnlich als eigenes Objekt in `/Annots`; ein
/// direkt eingebettetes Dictionary wird an Ort und Stelle bereinigt und
/// zurückgeschrieben. Von jeder Annotation aus werden
/// [`ANNOTATION_LINK_KEYS`] verfolgt: das `/Popup` hängt sonst nur an der
/// Notiz, das Elternfeld eines Widgets nur an dessen `/Parent`, und dessen
/// `/Kids` halten die Geschwister-Widgets. `visited` endet Zyklen
/// (`/Parent` ↔ `/Kids`, `/Popup` ↔ `/Parent`) und sorgt dafür, dass ein
/// geteiltes Feld einmal gezählt wird.
fn clean_annotations(
    doc: &mut Document,
    page_id: ObjectId,
    visited: &mut BTreeSet<ObjectId>,
    shown: &BTreeSet<ObjectId>,
    captions: &mut BTreeMap<ObjectId, bool>,
) -> Cleaned {
    let Some(mut annots) = doc
        .get_dictionary(page_id)
        .ok()
        .and_then(|page| value_of(page, b"Annots"))
        .cloned()
    else {
        return Cleaned::default();
    };
    let cleaned = clean_carriers(doc, &mut annots, true, visited, shown, captions);
    // Nur wenn im *direkten* Wert etwas fiel, muss er zurück an die Seite —
    // ein `/Annots 9 0 R` wird an seinem eigenen Objekt bereinigt.
    if cleaned.any() && !matches!(annots, Object::Reference(_)) {
        if let Ok(page) = doc.get_dictionary_mut(page_id) {
            page.set("Annots", annots);
        }
    }
    cleaned
}

/// Bereinigt eine Trägerwurzel — das `/Annots`-Feld einer Seite oder das
/// `/Fields`-Feld des Formulars — und alles, was von dort aus erreichbar ist.
///
/// `root` wird an Ort und Stelle verändert: eingebettete Dictionaries werden
/// darin bereinigt, Verweise wandern auf den Stapel und werden an ihren
/// eigenen Objekten bereinigt. Wer den Wurzelwert behalten will, schreibt ihn
/// danach zurück.
///
/// `root_shown`: zeigt eine Seite die Träger direkt in `root`? Für `/Annots`
/// ja, für `/Fields` nein. Ein Träger, den keine Seite zeigt, verliert sein
/// Erscheinungsbild (Register #90, siehe [`shown_carriers`]).
fn clean_carriers(
    doc: &mut Document,
    root: &mut Object,
    root_shown: bool,
    visited: &mut BTreeSet<ObjectId>,
    shown: &BTreeSet<ObjectId>,
    captions: &mut BTreeMap<ObjectId, bool>,
) -> Cleaned {
    let mut stack: Vec<ObjectId> = Vec::new();
    let mut cleaned = clean_embedded(doc, root, root_shown, &mut stack, captions);
    cleaned += drain_carriers(doc, &mut stack, visited, shown, captions);
    cleaned
}

/// Die Träger, die eine Seite zeigt: jedes Objekt, das in `/Annots` einer
/// Seite steht, und jedes `/Annots`-Array, das als eigenes Objekt dasteht
/// (seine direkten Einträge zeigt die Seite).
///
/// Ein Träger außerhalb davon wird von keinem Betrachter gezeichnet — ein
/// Widget, das nur in den `/Kids` seines Felds hängt, das Ende einer
/// Antwortkette, weiter weg, als `crate::content` von der Seite aus geht.
/// Bis zur Spur-A-Runde 2 hielt der Trägerlauf solche Träger über `/Parent`,
/// `/Kids` und `/IRT` am Leben und nahm ihnen die Texte, ließ aber ihr
/// `/AP` stehen, das niemand las: der gezeichnete Feldwert stand nach dem
/// Lauf in der Datei, ohne Warnung (Register #90, #91). Jetzt verliert ein
/// solcher Träger sein Erscheinungsbild — was niemand zeigt, braucht keins,
/// und was bliebe, könnte niemand schwärzen.
fn shown_carriers(doc: &Document, page_ids: &[ObjectId]) -> BTreeSet<ObjectId> {
    let mut shown = BTreeSet::new();
    for page_id in page_ids {
        let Some(annots) = doc
            .get_dictionary(*page_id)
            .ok()
            .and_then(|page| page.get(b"Annots").ok())
        else {
            continue;
        };
        let array = match annots {
            // Ein Array, das sich Seiten teilen, einmal — nicht Seiten ×
            // Einträge (Register #94).
            Object::Reference(id) => {
                if !shown.insert(*id) {
                    continue;
                }
                doc.get_object(*id).ok().and_then(|o| o.as_array().ok())
            }
            Object::Array(items) => Some(items),
            _ => None,
        };
        for item in array.into_iter().flatten() {
            if let Object::Reference(id) = item {
                shown.insert(*id);
            }
        }
    }
    shown
}

/// Arbeitet den Stapel ab: jedes erreichte Objekt wird als Träger bereinigt,
/// seine Verweise kommen wieder auf den Stapel.
fn drain_carriers(
    doc: &mut Document,
    stack: &mut Vec<ObjectId>,
    visited: &mut BTreeSet<ObjectId>,
    shown: &BTreeSet<ObjectId>,
    captions: &mut BTreeMap<ObjectId, bool>,
) -> Cleaned {
    let mut cleaned = Cleaned::default();
    loop {
        let Some(id) = stack.pop() else {
            return cleaned;
        };
        if !visited.insert(id) {
            continue;
        }
        // Was sich nur am unveränderten Dokument entscheiden lässt — die Id
        // hinter einer Verweiskette —, kommt zuerst; danach wird an einer
        // Kopie gearbeitet, und nur die kommt zurück ins Dokument.
        let Some((id, mut object)) = ({
            let reference = Object::Reference(id);
            match doc.dereference(&reference) {
                // `/Kids 5 0 R` — das Feld selbst als eigenes Objekt.
                Ok((resolved, object @ Object::Array(_))) => {
                    Some((resolved.unwrap_or(id), object.clone()))
                }
                // Ein Dictionary — ob Träger oder nicht, entscheidet
                // `clean_embedded`: ein Nicht-Träger verliert nur seinen
                // Spiegeltext (`/Alt`, `/ActualText`) und wird nicht
                // weiterverfolgt.
                Ok((resolved, object @ Object::Dictionary(_))) => {
                    Some((resolved.unwrap_or(id), object.clone()))
                }
                _ => None,
            }
        }) else {
            continue;
        };
        let step = clean_embedded(doc, &mut object, shown.contains(&id), stack, captions);
        if step.any() {
            doc.objects.insert(id, object);
        }
        cleaned += step;
    }
}

/// Bereinigt einen *direkten* Objektbaum: jedes eingebettete Träger-
/// Dictionary an Ort und Stelle, jeder Verweis auf den Stapel.
///
/// Ohne Rekursion und ohne Tiefengrenze — der Stapel hält die noch offenen
/// Zweige, und ein direkter Baum ist endlich (er wurde als Teil *eines*
/// Objekts geladen, Zyklen kann er nicht haben).
///
/// `root_shown`: zeigt eine Seite `root` — ist es ein Array, dessen direkte
/// Einträge? Alles, was über [`ANNOTATION_LINK_KEYS`] erreicht wird, zeigt
/// keine Seite als Teil von `root`; ein Verweis entscheidet an seinem Objekt.
fn clean_embedded(
    doc: &Document,
    root: &mut Object,
    root_shown: bool,
    stack: &mut Vec<ObjectId>,
    captions: &mut BTreeMap<ObjectId, bool>,
) -> Cleaned {
    let mut cleaned = Cleaned::default();
    let mut open: Vec<(&mut Object, bool)> = match root {
        Object::Array(items) => items.iter_mut().map(|item| (item, root_shown)).collect(),
        other => vec![(other, root_shown)],
    };
    while let Some((item, shown)) = open.pop() {
        match item {
            Object::Reference(id) => stack.push(*id),
            Object::Array(items) => open.extend(items.iter_mut().map(|item| (item, false))),
            Object::Dictionary(dict) => {
                if !is_carrier(dict) {
                    // Kein Träger — aber **erreicht**. Ein `/StructElem`
                    // hinter `/IRT` verwaist nicht: der Verweis hält es über
                    // `prune_unreachable` hinweg am Leben, samt `/Alt`.
                    // Weiter läuft der Lauf hier nicht: eine kaputte
                    // `/Parent`-Kette, die auf die Seite führt, darf ihr
                    // weder `/Contents` noch `/Kids` nehmen.
                    clear_alternates(dict, &mut cleaned.texts);
                    continue;
                }
                let keep_dest = keeps_destination(doc, dict);
                if let Ok(Object::Reference(mk)) = dict.get(b"MK") {
                    *captions.entry(*mk).or_insert(false) |= shown;
                }
                cleaned += clean_carrier(dict, keep_dest);
                if !shown {
                    take(dict, b"AP", &mut cleaned.texts);
                    if let Ok(Object::Dictionary(mk)) = dict.get_mut(b"MK") {
                        for key in ICON_KEYS {
                            take(mk, key, &mut cleaned.texts);
                        }
                    }
                }
                for (key, value) in dict.iter_mut() {
                    if ANNOTATION_LINK_KEYS.contains(&key.as_slice()) {
                        open.push((value, false));
                    }
                }
            }
            _ => {}
        }
    }
    cleaned
}

/// Die Schlüssel, unter denen eine Annotation Klartext neben ihrem
/// Erscheinungsbild führt.
///
/// `/Contents`, `/RC`, `/T` und `/Subj` sind die Kommentartexte (PDF 32000-1,
/// 12.5.2 und 12.5.6.2); an einem Feld ist `/T` der Feldname. `/TU` und
/// `/TM` stehen an Formularfeldern (12.7.3.1, Tabelle 220): `/TU` ist der
/// alternative Feldname, den der Betrachter als **Tooltip** zeigt, `/TM` der
/// Exportname beim Absenden. Beide sind frei wählbarer Text und werden von
/// Formulargeneratoren mit dem Beschriftungstext gefüllt — „Konto von Max
/// Mustermann“ ist ein Tooltip, wie ihn jeder Editor schreibt. `/Opt` sind
/// die Auswahltexte einer Liste oder die Exportwerte einer Radiogruppe
/// (Tabelle 231), `/OverlayText` der Text, den eine Redact-Annotation über
/// die Stelle schreibt (Tabelle 195), `/NM` der frei wählbare Name der
/// Annotation (Tabelle 164) und `/DS` die Stilangabe eines FreeText
/// (Tabelle 174) — alle vier Zeichenketten ohne Glyphengeometrie.
pub(crate) const ANNOTATION_TEXT_KEYS: [&[u8]; 10] = [
    b"Contents",
    b"RC",
    b"T",
    b"Subj",
    b"TU",
    b"TM",
    b"Opt",
    b"OverlayText",
    b"NM",
    b"DS",
];

/// Aktionen an einer Annotation oder einem Feld: `/A` und `/AA`
/// (Tabelle 164) und `/PA`, die URI-Aktion eines Links (Tabelle 173).
const ANNOTATION_ACTION_KEYS: [&[u8]; 3] = [b"A", b"AA", b"PA"];

/// Die Beschriftungen im `/MK`-Dictionary eines Widgets (Tabelle 189):
/// normal, beim Überfahren, beim Drücken.
const CAPTION_KEYS: [&[u8]; 3] = [b"CA", b"RC", b"AC"];

/// Die Symbole im `/MK`-Dictionary eines Druckknopfs (Tabelle 189) — Form-
/// XObjects, die Text zeichnen dürfen. Ein Widget, das keine Seite zeigt,
/// verliert sie mit seinem `/AP` (Register #90).
const ICON_KEYS: [&[u8]; 3] = [b"I", b"RI", b"IX"];

/// Die Schlüssel, über die eine Annotation weitere Träger erreichbar hält:
/// ihr `/Popup` (Tabelle 170), das Elternfeld (`/Parent`, Tabelle 220), die
/// Kindfelder und Geschwister-Widgets (`/Kids`) und die Annotation, auf die
/// sie antwortet (`/IRT`, Tabelle 170).
const ANNOTATION_LINK_KEYS: [&[u8]; 4] = [b"Popup", b"Parent", b"Kids", b"IRT"];

/// Die beiden Schlüssel, unter denen ein Dictionary den Seitentext
/// *spiegelt*: `/Alt` (die Beschreibung für Menschen) und `/ActualText` (der
/// Ersatztext beim Kopieren), PDF 32000-1, 14.9.3 und 14.9.4.
///
/// Sie stehen nicht in [`ANNOTATION_TEXT_KEYS`], weil sie auch an Objekten
/// hängen, die keine Träger sind — an einem `/StructElem` etwa —, und weil
/// `crate::content` diese Liste als „Text, den die Annotation neben ihrem
/// Erscheinungsbild führt“ liest.
///
/// **Und deshalb steht der `/Alt` eines Bildes hier nicht.** Er gehört weder in
/// diese Liste noch in [`ANNOTATION_TEXT_KEYS`]: ein Bild ist kein Träger, es
/// wird von diesem Lauf nicht erreicht, und in der Liste, die den
/// Spiegelvergleich von `crate::content` steuert, erzeugte er strukturelle
/// Fehlalarme (Befund #14: „/E, /Alt am Bild“). Dieser Lauf kennt auch keine
/// Schwärzungsrechtecke und könnte das Bild, über dem geschwärzt wurde, nicht
/// von dem Firmenlogo daneben unterscheiden — ein Dokument ohne Ersatztexte ist
/// für blinde Leser unbrauchbar. Genommen wird er dort, wo die Pixel fallen:
/// `crate::redact::clear_image_alternates` und der Spiegel darüber in
/// `crate::redact::mirrors_to_clear` (Register #20, Beleg
/// `zh_b_bildspiegel.rs`).
const ALTERNATE_TEXT_KEYS: [&[u8]; 2] = [b"Alt", b"ActualText"];

/// Beiwerk einer Annotation, das eigenen Klartext führt und nichts zeichnet —
/// es fällt als Ganzes.
///
/// * `/Movie` (12.5.6.17, Tabelle 293/294): `/F` ist der Dateiname des Films
///   und Pflichtschlüssel — „Kontoauszug DE89 ….mov“ ist ein Dateiname, wie
///   ihn jeder schreibt. Ohne `/F` wäre der Rest (`/Aspect`, `/Rotate`,
///   `/Poster`) ein regelwidriger Torso; der Film selbst steckt ohnehin nicht
///   im Erscheinungsstrom.
/// * `/Measure` (12.9, Tabelle 198–200): die Maßangaben einer Vermessung.
///   Text steht dort nicht nur in `/X[i] /U` (die Einheit), sondern in `/R`
///   (das Maßstabsverhältnis) und in jedem `/RT`, `/RD`, `/PS`, `/SS` jeder
///   Zahlenformatierung — und dasselbe noch einmal in `/Y`, `/D`, `/A` und
///   `/T`. Nur `/X[i] /U` zu leeren hieße, vier weitere gleichartige Lecks
///   stehen zu lassen. Gezeichnet wird davon nichts: die Beschriftung einer
///   Vermessung steht im `/AP` und geht dort denselben Weg wie Seitentext.
/// * `/RichMediaContent` (13.7, Tabelle 328): der `/Assets`-Namensbaum ist
///   der dritte Weg, auf dem eine **eingebettete Datei** in einer PDF-Datei
///   steckt — neben `/Names /EmbeddedFiles` und der
///   `/FileAttachment`-Annotation, die beide schon fallen.
/// * `/RichMediaSettings` (13.7.2, Tabelle 332): unter `/Activation` hält
///   jede Instanz einer `/Configuration` mit `/Asset` dieselben Filespecs,
///   die `/Assets` nennt, und `/Scripts` eine Liste weiterer — so schreiben
///   es Acrobat und media9. Mit `/RichMediaContent` allein blieb die Datei
///   über die Einstellungen erreichbar (Register #92).
/// * `/3DD` und `/3DV` (13.6.2, Tabelle 298): der Strom des 3D-Modells trägt
///   unter `/OnInstantiate` einen **JavaScript-Strom** (Tabelle 300), die
///   Ansicht unter `/XN` ihren frei wählbaren Anzeigenamen (Tabelle 304).
///   Gezeichnet wird vor dem Aktivieren nur das `/AP`; das Modell selbst ist
///   dieselbe Sorte Nutzlast wie ein Film.
/// * `/RO` (12.5.6.23, Tabelle 195): das Form-XObject, das eine
///   Redact-Annotation über die Stelle legt, **nachdem** die Schwärzung
///   angewendet wurde. Vorher zeichnet es nichts, und kein Lauf liest es:
///   `crate::content` liest das `/AP` einer Annotation, nicht ihr `/RO` —
///   der Text darin ginge durch jede Schwärzung und jede Zählung hindurch.
///
/// **Was das kostet und was es nicht kostet:** die Annotation bleibt stehen
/// und verliert einen Pflichtschlüssel — eine Movie-Annotation ohne `/Movie`
/// ist ein Torso, und poppler sagt das an ihr („Syntax Error: Bad Annot
/// Movie“). Das ist der Preis, *nicht* das Ziel: nur das Beiwerk ganz zu
/// nehmen statt das eine Textfeld darin hinterlässt keinen Torso **im
/// Beiwerk** (ein `/Movie` ohne `/F`, ein `/Measure` ohne `/R`) und lässt
/// keines der gleichartigen Nebenfelder stehen. Gezeichnet wird von alldem
/// nichts, kein Betrachter bricht daran ab, und der Seitentext bleibt
/// unverändert (Beleg:
/// `zg_r3_beiwerk::movie_annotation_ohne_movie_laedt_und_behaelt_den_seitentext`).
const ANNOTATION_PLATE_KEYS: [&[u8]; 10] = [
    b"Movie",
    b"Measure",
    b"RichMediaContent",
    b"RichMediaSettings",
    b"3DD",
    b"3DV",
    // Die Einheiten einer 3D-Annotation (PDF 2.0, 13.6.2): `/TU`, `/UU`,
    // `/DU` sind frei wählbare Namen (Register #92).
    b"3DU",
    b"RO",
    // Seed-Value und Sperre eines Signaturfelds (Tabellen 233/234): `/SV
    // /Reasons`, `/SV /LegalAttestation`, `/Lock /Fields` (Register #70).
    b"SV",
    b"Lock",
];

/// Dateiverweise an einem Träger: `/AF` (zugeordnete Datei, PDF 2.0, 14.13 —
/// der Weg, auf dem eine ZUGFeRD-/Factur-X-Rechnung ihre XML-Fassung anhängt)
/// und `/FS` (die Datei einer `/FileAttachment`-Annotation, Tabelle 184).
///
/// Beide zeigen auf ein Filespec-Dictionary mit Dateinamen (`/F`, `/UF`,
/// `/Desc`) und der eingebetteten Datei selbst (`/EF`). Der `/Names`-Baum zu
/// entfernen genügt nicht: `/AF` ist ein zweiter, gleichwertiger Halter, und
/// eine Annotation, die ihr `/Popup` oder eine `/IRT`-Antwort am Leben hält,
/// nimmt ihr `/FS` mit über das Aufräumen.
const FILE_SPEC_KEYS: [&[u8]; 2] = [b"AF", b"FS"];

/// Ist dieses Dictionary eine Annotation oder ein Formularfeld?
///
/// Beide tragen entweder kein `/Type` (Felder, und Annotationen dürfen es
/// weglassen) oder `/Type /Annot`. Alles andere — eine Seite, der
/// Seitenbaum, ein Katalog — ist kein Träger: eine kaputte `/Parent`-Kette,
/// die auf die Seite führt, darf ihr weder `/Contents` noch `/Kids` nehmen.
fn is_carrier(dict: &Dictionary) -> bool {
    match value_of(dict, b"Type") {
        Some(Object::Name(name)) => name == b"Annot",
        Some(_) => false,
        // Kein `/Type` — und ein `/Type null` ist keines (7.3.9).
        None => true,
    }
}

/// Darf das `/Dest` dieser Annotation stehen bleiben? Nur, wenn es eines
/// trägt **und** dieses — nach Auflösung — ein ausdrückliches Ziel ist.
fn keeps_destination(doc: &Document, dict: &Dictionary) -> bool {
    value_of(dict, b"Dest").is_some_and(|dest| is_explicit_destination(doc, dest))
}

/// Was ein Lauf tatsächlich entfernt hat.
///
/// Jede Zahl zählt **entfernte Schlüssel** an Objekten, die so in der
/// Ausgabe stehen — nicht Absichten. Ein Schlüssel mit dem Wert `null` zählt
/// nicht mit: PDF 32000-1, 7.3.9 setzt ihn einem fehlenden Schlüssel gleich
/// (siehe [`take`]). Und ein Schlüssel, dessen **Verweis** fiel, zählt erst,
/// wenn das Objekt dahinter nach dem Aufräumen wirklich fehlt (siehe
/// [`Tally`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Cleaned {
    /// `/A`, `/AA`, `/PA` und ein benanntes `/Dest`.
    actions: Tally,
    /// Klartexte ([`ANNOTATION_TEXT_KEYS`], [`ALTERNATE_TEXT_KEYS`],
    /// [`ANNOTATION_PLATE_KEYS`] und die Beschriftungen in `/MK`).
    texts: Tally,
    /// Feldwerte `/V`, `/DV`, `/RV`.
    values: Tally,
    /// Dateiverweise ([`FILE_SPEC_KEYS`]).
    files: Tally,
}

impl Cleaned {
    fn any(&self) -> bool {
        self.actions.any() || self.texts.any() || self.values.any() || self.files.any()
    }

    /// Wie viele der gebuchten Verweise stehen noch in der Datei?
    fn alive(&self, doc: &Document, chains: &Chains) -> usize {
        self.actions.alive(doc, chains)
            + self.texts.alive(doc, chains)
            + self.values.alive(doc, chains)
            + self.files.alive(doc, chains)
    }
}

impl std::ops::AddAssign for Cleaned {
    fn add_assign(&mut self, other: Self) {
        self.actions += other.actions;
        self.texts += other.texts;
        self.values += other.values;
        self.files += other.files;
    }
}

/// Die drei Schlüssel, an denen ein Formularfeld seinen Wert trägt
/// (PDF 32000-1, 12.7.3.1, Tabelle 220 und 12.7.3.4).
const FIELD_VALUE_KEYS: [&[u8]; 3] = [b"V", b"DV", b"RV"];

/// Nimmt einer Annotation oder einem Feld Aktionen, Klartexte und Feldwerte.
/// `keep_dest` ist vorher am unveränderten Dokument bestimmt (siehe
/// [`keeps_destination`]).
///
/// `/V`, `/DV` und `/RV` fallen hier statt in einem eigenen, tiefenbegrenzten
/// Vorlauf: ein Feld trägt seinen Wert genau dort, wo es auch seinen
/// Feldnamen trägt, und dieser Lauf erreicht jedes Feld, das die Datei
/// erreichbar hält — auch das siebzigste einer `/Parent`-Kette.
fn clean_carrier(dict: &mut Dictionary, keep_dest: bool) -> Cleaned {
    let mut cleaned = Cleaned::default();
    for key in ANNOTATION_ACTION_KEYS {
        take(dict, key, &mut cleaned.actions);
    }
    if !keep_dest {
        take(dict, b"Dest", &mut cleaned.actions);
    }
    for key in ANNOTATION_TEXT_KEYS {
        take(dict, key, &mut cleaned.texts);
    }
    for key in ANNOTATION_PLATE_KEYS {
        take(dict, key, &mut cleaned.texts);
    }
    clear_alternates(dict, &mut cleaned.texts);
    for key in FILE_SPEC_KEYS {
        take_file_spec(dict, key, &mut cleaned.files);
    }
    for key in FIELD_VALUE_KEYS {
        take(dict, key, &mut cleaned.values);
    }
    if let Ok(Object::Dictionary(mk)) = dict.get_mut(b"MK") {
        clean_captions(mk, &mut cleaned.texts);
    }
    cleaned
}

/// Leert die Beschriftungen eines `/MK`-Dictionaries.
fn clean_captions(mk: &mut Dictionary, texts: &mut Tally) {
    for key in CAPTION_KEYS {
        take(mk, key, texts);
    }
}

/// Nimmt einem erreichten Dictionary seinen Spiegeltext.
///
/// `/Alt` und `/ActualText` stehen an Struktur-Elementen (PDF 32000-1,
/// 14.9.3 und 14.9.4) und an markierten Abschnitten: `/Alt` beschreibt eine
/// Abbildung für Menschen, `/ActualText` ersetzt den Text beim Kopieren.
/// Beides ist frei wählbarer Klartext und spiegelt gewöhnlich genau das, was
/// gerade aus dem Content-Stream verschwunden ist.
///
/// Angefasst wird nur, was dieser Lauf **erreicht** — also von einer
/// Annotation aus über [`ANNOTATION_LINK_KEYS`]. Der `/K`-Baum unter
/// `/StructTreeRoot` fällt sonst mit dem Katalogschlüssel und wird
/// weggeräumt; ein `/StructElem`, das keine Annotation erreicht, bleibt
/// unberührt (siehe `zf_q1_luecken`).
fn clear_alternates(dict: &mut Dictionary, texts: &mut Tally) {
    for key in ALTERNATE_TEXT_KEYS {
        take(dict, key, texts);
    }
}

/// Ein `/Dest` ohne Text: ein Feld aus Seitenverweis, Zahlen und einem der
/// Anzeigenamen aus PDF 32000-1, Tabelle 151 (`[Seite /XYZ x y z]`).
///
/// Ein Verweis wird zuerst **aufgelöst** — das Feld selbst wie jedes seiner
/// Elemente: `/Dest 12 0 R` ist, was in Objekt 12 steht, und `[12 0 R /XYZ …]`
/// ist nur dann ein Ziel, wenn Objekt 12 eine Seite (`/Type /Page`) ist.
/// Entschieden wird über den Inhalt, nicht über die Schreibweise; ein
/// Erzeuger, der jedes Ziel als eigenes Objekt ablegt, verliert seine Sprünge
/// sonst grundlos — und einer, der eine Zeichenkette hinter den Verweis im
/// Feld legt, kommt nicht durch.
///
/// Alles andere — eine Zeichenkette (benanntes Ziel), ein Name als Ziel,
/// ein Feld mit einem fremden Namen darin — kann Text tragen und fällt; ein
/// Verweis ins Leere fällt mit, er führt ohnehin nirgendwohin.
fn is_explicit_destination(doc: &Document, dest: &Object) -> bool {
    const FIT: [&[u8]; 8] = [
        b"XYZ", b"Fit", b"FitH", b"FitV", b"FitR", b"FitB", b"FitBH", b"FitBV",
    ];
    let Some(Object::Array(items)) = resolve(doc, dest) else {
        return false;
    };
    items.iter().all(|item| match resolve(doc, item) {
        Some(Object::Integer(_) | Object::Real(_) | Object::Null) => true,
        Some(Object::Name(name)) => FIT.contains(&name.as_slice()),
        // Eine Seite ist immer ein eigenes Objekt — ein direkt eingebettetes
        // Dictionary im Zielfeld ist keine.
        Some(Object::Dictionary(dict)) => {
            matches!(item, Object::Reference(_))
                && dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"Page")
        }
        _ => false,
    })
}

/// Entfernt `key` und bucht die Entfernung in `into`. Rückgabe: stand dort
/// ein Wert? Das Objekt dahinter bleibt stehen — ob es noch jemand hält,
/// entscheidet `prune_unreachable`, nicht diese Stelle.
///
/// Ein Schlüssel mit dem Wert `null` zählt **nicht**: PDF 32000-1, 7.3.9
/// setzt ihn einem fehlenden Schlüssel gleich. Entfernt wird er trotzdem —
/// er trägt nichts, und ohne ihn ist die Datei um eine Merkwürdigkeit ärmer.
fn take(dict: &mut Dictionary, key: &[u8], into: &mut Tally) -> bool {
    into.book(dict.remove(key))
}

/// Entfernt einen Dateiverweis und bucht ihn **je Filespec**.
///
/// `/AF` ist ein *Array* von Filespecs (PDF 2.0, 14.13). Für [`Tally`] wäre
/// ein Array ein direkter Wert — mit dem Schlüssel weg, also gezählt. Das
/// stimmt für das Array, nicht für die Dateien darin: hält ein zweiter
/// Verweis den Filespec, steht die eingebettete Datei weiter in der Ausgabe,
/// und der Bericht meldete eine Entfernung, die `--check-leaks` findet.
/// Gebucht wird deshalb jedes Element; `/FS` (ein einzelner Filespec) geht
/// unverändert durch.
fn take_file_spec(dict: &mut Dictionary, key: &[u8], into: &mut Tally) -> bool {
    match dict.remove(key) {
        Some(Object::Array(items)) => {
            let mut any = false;
            for item in items {
                any |= into.book(Some(item));
            }
            any
        }
        removed => into.book(removed),
    }
}

/// Ein Zähler, der erst **nach** dem Aufräumen feststeht.
///
/// Stand der Wert eines entfernten Schlüssels **direkt** im Dictionary, ist
/// er mit dem Schlüssel weg — das steht sofort fest. Stand dort ein
/// **Verweis**, ist erst nach [`crate::document::prune_unreachable`] klar, ob
/// das Objekt dahinter wirklich fiel: hält es noch jemand anders
/// (`/Contents 4 0 R`, ein `/Title`, den zwei Lesezeichen teilen), steht sein
/// Text weiter in der Datei. Ein Zähler, der ihn dann als entfernt meldet,
/// meldet etwas, das nicht geschah — und `MetadataReport` verspricht das
/// Gegenteil.
///
/// Kosten: ein `ObjectId` (8 Byte) je entfernten Schlüssel mit Verweiswert,
/// dazu am Ende ein Nachschlagen je gemerkter Id. Der Lauf über den
/// Objektgraphen bleibt derselbe.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Tally {
    /// Entfernungen, deren Wert kein Verweis war.
    direct: usize,
    /// Die Objekte hinter entfernten Verweisen.
    refs: Vec<ObjectId>,
}

impl Tally {
    /// Bucht, was `Dictionary::remove` zurückgab. Rückgabe: stand dort
    /// überhaupt etwas?
    fn book(&mut self, removed: Option<Object>) -> bool {
        match removed {
            None | Some(Object::Null) => false,
            Some(Object::Reference(id)) => {
                self.refs.push(id);
                true
            }
            Some(_) => {
                self.direct += 1;
                true
            }
        }
    }

    /// Ist überhaupt etwas gefallen?
    fn any(&self) -> bool {
        self.direct > 0 || !self.refs.is_empty()
    }

    /// Wie viele der gebuchten Entfernungen sind nach dem Aufräumen wirklich
    /// welche?
    fn settled(&self, doc: &Document, chains: &Chains) -> usize {
        self.direct + self.refs.len() - self.alive(doc, chains)
    }

    /// Wie viele der gebuchten Verweise stehen noch in der Datei?
    ///
    /// Gefragt wird nach dem **Ziel** der Kette, nicht nach der gebuchten Id:
    /// hinter `/Contents 4 0 R` kann Objekt 4 selbst nur `5 0 R` sein, und
    /// der Text steht in 5. Fällt 4 und bleibt 5, ist nichts entfernt
    /// (siehe [`Chains`]).
    fn alive(&self, doc: &Document, chains: &Chains) -> usize {
        self.refs
            .iter()
            .filter(|id| doc.objects.contains_key(&chains.target(**id)))
            .count()
    }
}

/// Verweisketten des **geladenen** Dokuments: welches Objekt steht am Ende
/// einer Kette aus Objekten, die selbst nichts als ein Verweis sind?
///
/// `4 0 obj 5 0 R endobj` ist eine solche Kette. Ein Zähler, der sich die
/// erste Id merkt, misst dann das falsche Objekt: fällt 4 (niemand hält es)
/// und hält ein zweiter Verweis die Zeichenkette 5, meldet der Bericht eine
/// Entfernung, die `--check-leaks` unverändert findet — genau das, was
/// [`MetadataReport`] ausschließt.
///
/// Gebaut wird die Karte **vor** jeder Änderung: danach ist Objekt 4
/// vielleicht weggeräumt, und die Kette nicht mehr zu sehen. Kosten: ein
/// Durchgang durch `doc.objects` und ein Eintrag je Objekt, das nur ein
/// Verweis ist — in gewöhnlichen Dateien keiner.
#[derive(Debug, Clone, Default)]
struct Chains(BTreeMap<ObjectId, ObjectId>);

impl Chains {
    /// Kosten: **linear** in der Zahl der Objekte. Jedes Glied wird genau
    /// einmal gelaufen — wer ein schon aufgelöstes Glied trifft, übernimmt
    /// dessen Ende, und der ganze gelaufene Pfad bekommt es eingetragen.
    /// Bis zur Spur-A-Runde 1 lief jedes Glied seine Kette bis zum Ende
    /// (n²/2): eine 365-kB-Datei mit 8 000 Gliedern brauchte 33 s, 20 000
    /// Glieder (936 kB) 223 s im Testprozess, Debug, am Stand `456d669`
    /// (Register #72; die Zahlen bindet `belege.rs`).
    fn of(doc: &Document) -> Self {
        let mut map: BTreeMap<ObjectId, ObjectId> = BTreeMap::new();
        // Glieder, deren Kette in einem Ring endet — auch die gelten als
        // erledigt, sonst liefe jeder Ring je Glied noch einmal.
        let mut done: BTreeSet<ObjectId> = BTreeSet::new();
        for (id, object) in &doc.objects {
            if !matches!(object, Object::Reference(_)) || done.contains(id) {
                continue;
            }
            // Der Kette folgen, bis ein Objekt kein Verweis mehr ist, ein
            // schon bekanntes Ende auftaucht, oder der Pfad sich schließt
            // (`4 0 obj 5 0 R`, `5 0 obj 4 0 R`).
            let mut path: Vec<ObjectId> = vec![*id];
            let mut seen: BTreeSet<ObjectId> = BTreeSet::from([*id]);
            let mut at = *id;
            let end = loop {
                if let Some(end) = map.get(&at) {
                    break *end;
                }
                match doc.objects.get(&at) {
                    Some(Object::Reference(next)) => {
                        if !seen.insert(*next) {
                            // Ring: das zuletzt erreichte Glied ist das Ende.
                            break at;
                        }
                        at = *next;
                        path.push(at);
                    }
                    _ => break at,
                }
            };
            for link in path {
                done.insert(link);
                if link != end {
                    map.insert(link, end);
                }
            }
        }
        Self(map)
    }

    /// Das Objekt am Ende der Kette — oder die Id selbst, wenn sie keine ist.
    fn target(&self, id: ObjectId) -> ObjectId {
        self.0.get(&id).copied().unwrap_or(id)
    }
}

impl std::ops::AddAssign for Tally {
    fn add_assign(&mut self, other: Self) {
        self.direct += other.direct;
        self.refs.extend(other.refs);
    }
}

/// Der Wert hinter `key` — `None`, wenn der Schlüssel fehlt **oder** `null`
/// ist (PDF 32000-1, 7.3.9: beides ist dasselbe).
fn value_of<'a>(dict: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    match dict.get(key) {
        Ok(Object::Null) | Err(_) => None,
        Ok(object) => Some(object),
    }
}

fn resolve<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a Object> {
    doc.dereference(object).ok().map(|(_, o)| o)
}

/// Eine Nutzlast, die mit einem Schlüssel fallen soll — und die Objekte, an
/// denen sich nach dem Aufräumen nachsehen lässt, ob sie wirklich fiel.
///
/// Warum das nötig ist: `strip_metadata` entfernt Schlüssel, keine Objekte.
/// Was dadurch unerreichbar wird, räumt `prune_unreachable` weg — was noch
/// jemand anders hält, bleibt stehen. Ein Zähler, der vor dem Aufräumen
/// gezählt hat, meldet in diesem Fall eine Entfernung, die nicht stattfand;
/// `MetadataReport` verspricht das Gegenteil.
///
/// `count` ist die Anzahl der Einträge, `holder` das Objekt, das den
/// Schlüssel trug (`None`: der Schlüssel stand direkt im Katalog und fällt
/// mit ihm), `ids` sind alle Objekte des Teilbaums darunter. Gemeldet wird
/// nur, wenn **nichts** davon mehr in der Datei steht.
#[derive(Debug, Clone, Default)]
struct Payload {
    count: usize,
    holder: Option<ObjectId>,
    ids: BTreeSet<ObjectId>,
}

impl Payload {
    /// Vermisst `dict[key]`; `holder` ist die Id von `dict`, falls es ein
    /// eigenes Objekt ist.
    fn of(doc: &Document, dict: &Dictionary, key: &[u8], holder: Option<ObjectId>) -> Self {
        let Some(node) = value_of(dict, key) else {
            return Self::default();
        };
        Self {
            // Ein vorhandener, aber leerer Teilbaum zählt als eins — auch er
            // verschwindet und gehört ins Audit-Log.
            count: count_name_tree(doc, node).max(1),
            holder,
            ids: subtree_ids(doc, node),
        }
    }

    /// Wie viele Einträge sind nach dem Aufräumen wirklich verschwunden?
    fn removed(&self, doc: &Document) -> usize {
        let gone = |id: &ObjectId| !doc.objects.contains_key(id);
        if self.holder.as_ref().is_none_or(gone) && self.ids.iter().all(gone) {
            self.count
        } else {
            0
        }
    }
}

/// Zählt die Blätter eines Namensbaums (`/Names`-Array bzw. `/Kids`).
///
/// Ohne Tiefengrenze und ohne Rekursion: der Stapel hält die offenen Knoten,
/// die Besuchsmenge endet Zyklen. Eine Grenze hätte hier zu *wenig* gezählt.
fn count_name_tree(doc: &Document, root: &Object) -> usize {
    let mut count = 0usize;
    let mut seen: BTreeSet<ObjectId> = BTreeSet::new();
    let mut stack: Vec<&Object> = vec![root];
    while let Some(node) = stack.pop() {
        if let Object::Reference(id) = node {
            if !seen.insert(*id) {
                continue;
            }
        }
        let Some(dict) = resolve(doc, node).and_then(|o| o.as_dict().ok()) else {
            continue;
        };
        if let Some(items) = value_of(dict, b"Names")
            .and_then(|o| resolve(doc, o))
            .and_then(|o| o.as_array().ok())
        {
            // Ein Namensbaum speichert Paare: Name, Wert, Name, Wert …
            count += items.len() / 2;
        }
        if let Some(kids) = value_of(dict, b"Kids")
            .and_then(|o| resolve(doc, o))
            .and_then(|o| o.as_array().ok())
        {
            stack.extend(kids.iter());
        }
    }
    count
}

/// Alle Objekte, die von `root` aus erreichbar sind — die Menge, die
/// verschwinden muss, damit eine Nutzlast als entfernt gilt.
fn subtree_ids(doc: &Document, root: &Object) -> BTreeSet<ObjectId> {
    let mut seen: BTreeSet<ObjectId> = BTreeSet::new();
    let mut stack: Vec<&Object> = vec![root];
    while let Some(object) = stack.pop() {
        match object {
            Object::Reference(id) => {
                if seen.insert(*id) {
                    if let Some(target) = doc.objects.get(id) {
                        stack.push(target);
                    }
                }
            }
            Object::Array(items) => stack.extend(items.iter()),
            Object::Dictionary(dict) => stack.extend(dict.iter().map(|(_, value)| value)),
            Object::Stream(stream) => stack.extend(stream.dict.iter().map(|(_, value)| value)),
            _ => {}
        }
    }
    seen
}

/// Entfernt Annotationen vom Typ `/FileAttachment` aus einer Seite — und
/// nimmt jeder von ihnen ihre Datei.
///
/// Aus `/Annots` zu streichen genügt nicht: das `/Popup` einer Annotation
/// (die Form, in der Acrobat jeden Kommentar schreibt) hält sie über
/// `/Parent` am Leben, eine Antwort über `/IRT` genauso — samt `/FS`,
/// Filespec und eingebetteter Datei. Deshalb fallen [`FILE_SPEC_KEYS`] am
/// Objekt selbst; was danach niemand mehr hält, räumt `prune_unreachable`
/// weg.
///
/// Rückgabe: je gefundenem Anhang eine Buchung. Gezählt wird er erst nach
/// dem Aufräumen und nur, wenn seine Datei dann wirklich fehlt — hält sie
/// ein zweiter Verweis, steht sie weiter in der Datei.
///
/// Ein `/Annots` als eigenes Objekt wird an diesem Objekt bereinigt, einmal
/// je Dokument (`arrays`). Bis zur Spur-A-Runde 2 bekam jede Seite, die es
/// teilte, eine eigene Kopie ohne die Anhänge: Seiten × Einträge Arbeit und
/// ebenso viele Verweise in der Ausgabe (Register #94).
fn remove_file_attachments(
    doc: &mut Document,
    page_id: ObjectId,
    arrays: &mut BTreeSet<ObjectId>,
) -> Vec<Tally> {
    let shared = match doc
        .get_dictionary(page_id)
        .ok()
        .and_then(|page| page.get(b"Annots").ok())
    {
        Some(Object::Reference(id)) => Some(*id),
        _ => None,
    };
    if let Some(id) = shared {
        if !arrays.insert(id) {
            return Vec::new();
        }
    }
    let Some(items) = doc
        .get_dictionary(page_id)
        .ok()
        .and_then(|page| page.get(b"Annots").ok())
        .and_then(|annots| resolve(doc, annots))
        .and_then(|o| o.as_array().ok())
        .cloned()
    else {
        return Vec::new();
    };

    let mut kept = Vec::with_capacity(items.len());
    let mut removed: Vec<Tally> = Vec::new();
    for item in items {
        // Die Id hinter dem Eintrag steht nur am unveränderten Dokument
        // fest; danach wird geschrieben.
        let target = match doc.dereference(&item) {
            Ok((resolved, Object::Dictionary(dict))) if matches!(dict.get(b"Subtype"), Ok(Object::Name(n)) if n == b"FileAttachment") => {
                Some(resolved)
            }
            _ => None,
        };
        let Some(resolved) = target else {
            kept.push(item);
            continue;
        };
        let mut one = Tally::default();
        match resolved {
            // Ein eigenes Objekt: es überlebt, wenn ein `/Popup` oder ein
            // `/IRT` darauf zeigt — die Datei darf es nicht mitnehmen.
            Some(id) => {
                if let Ok(dict) = doc.get_dictionary_mut(id) {
                    for key in FILE_SPEC_KEYS {
                        take_file_spec(dict, key, &mut one);
                    }
                }
            }
            // Direkt im Array: das Dictionary fällt mit dem Eintrag, seine
            // Dateiverweise werden trotzdem gebucht — ob das Filespec
            // dahinter wirklich fällt, entscheidet erst das Aufräumen.
            None => {
                if let Ok(dict) = item.as_dict() {
                    let mut copy = dict.clone();
                    for key in FILE_SPEC_KEYS {
                        take_file_spec(&mut copy, key, &mut one);
                    }
                }
            }
        }
        removed.push(one);
    }

    if !removed.is_empty() {
        // Am eigenen Objekt, wenn es ein Array ist; eine Verweiskette
        // (`/Annots 5 0 R`, dort `6 0 R`) bekommt wie vorher die Seite.
        match shared.and_then(|id| match doc.get_object_mut(id) {
            Ok(Object::Array(entries)) => Some(entries),
            _ => None,
        }) {
            Some(entries) => *entries = kept,
            None => {
                if let Ok(page) = doc.get_dictionary_mut(page_id) {
                    page.set("Annots", Object::Array(kept));
                }
            }
        }
    }
    removed
}

/// Nimmt **jedem** Objekt der Datei seine Metadaten-Anhängsel: den
/// Katalogschlüssel mit Klartext, die bis zur Spur-A-Runde 1 stehen blieben
/// (Register #70). Jeder fällt ganz:
///
/// * `/Perms` — hält das Signatur-Dictionary (Tabelle 258) ein zweites Mal;
///   `/V` am Feld fiel, `/Reason`, `/Location`, `/ContactInfo`, `/Name` und
///   `/Reference … /Msg` überlebten. Jedes von Acrobat zertifizierte
///   Dokument hat es. Eine Signatur ist nach der Schwärzung ohnehin
///   ungültig.
/// * `/DSS` — Zertifikatsströme mit Unterzeichnernamen (PAdES-LTV).
/// * `/Threads` — Artikel mit `/I` (`/Title`, `/Author`, `/Subject`); die
///   Perlen hängen zusätzlich an der Seite unter `/B`.
/// * `/Collection` — Portfolio-Schema (`/Schema … /N`, `/D`, `/Folders
///   /Name /Desc`); die Dateien selbst fielen schon mit `/Names`.
/// * `/URI` — `/Base` (`https://…/kunden/<Konto>/`).
/// * `/DPartRoot` — PDF/VT-Dokumentteile mit `/DPM` (PDF 2.0, 14.12); im
///   Kontoauszugdruck stehen dort Name, Adresse und Konto des Empfängers.
///   Die Seite hält ihren Teil zusätzlich unter `/DPart`.
const CATALOG_BEIWERK_KEYS: [&[u8]; 6] = [
    b"Perms",
    b"DSS",
    b"Threads",
    b"Collection",
    b"URI",
    b"DPartRoot",
];

/// Seitenschlüssel mit Klartext (Register #70): `/B` (Artikelperlen), `/VP`
/// (Viewports, Tabelle 260 — `/Name` und `/Measure /R`, Geo-PDF), `/PresSteps`
/// (Navigationsknoten mit Aktionen `/NA`, `/PA` außerhalb von `/AA`), `/DPart`
/// (siehe `/DPartRoot`).
const PAGE_BEIWERK_KEYS: [&[u8]; 4] = [b"B", b"VP", b"PresSteps", b"DPart"];

/// Objektschlüssel mit Klartext (Register #70): `/Ref` an einem
/// Form-XObject (Referenz-XObject, 8.10.4 — `/F` ist ein Filespec mit `/EF`,
/// der vierte Weg für eine eingebettete Datei neben `/Names`, `/FS`, `/AF`)
/// und `/OPI` an einem Bild (Tabelle 397: `/F`, `/Comments`).
const OBJECT_BEIWERK_KEYS: [&[u8]; 2] = [b"Ref", b"OPI"];

/// Was ein Katalog behalten darf (ISO 32000-2, Tabelle 29) — alles andere
/// fällt als Beiwerk, auch ein Schlüssel, den keine Norm kennt.
///
/// Bis zur Spur-A-Runde 2 nahm der Lauf eine **Sperrliste**; was nicht auf
/// ihr stand, blieb. Prüfer B fand `/SpiderInfo` (Web Capture: die
/// abgerufene Adresse und die gesendeten Formulardaten), `/Legal` mit
/// `/Attestation` und `/Requirements` mit Text — jede Runde fände weitere
/// (Register #92). Was hier steht, braucht die Anzeige oder wird an anderer
/// Stelle bereinigt: `/PageLabels` und `/OutputIntents` verlieren ihre Texte
/// ([`clean_page_labels`], [`clean_output_intents`]), `/AF` fällt mit
/// [`clear_object_metadata`] und wird dort je Filespec gezählt.
const CATALOG_KEEP: [&[u8]; 12] = [
    b"Type",
    b"Version",
    b"Extensions",
    b"Pages",
    b"PageLabels",
    b"OutputIntents",
    b"ViewerPreferences",
    b"PageLayout",
    b"PageMode",
    b"Lang",
    b"NeedsRendering",
    b"AF",
];

/// Was eine Seite behalten darf (ISO 32000-2, Tabelle 31): Aufbau,
/// Seitenrahmen, Inhalt, Darstellung. `/Annots` wird als Träger bereinigt,
/// `/OutputIntents` (PDF 2.0) verliert seine Texte wie am Katalog, `/AF`
/// fällt mit [`clear_object_metadata`]. Alles andere fällt als Beiwerk
/// (Register #92).
const PAGE_KEEP: [&[u8]; 21] = [
    b"Type",
    b"Parent",
    b"Resources",
    b"MediaBox",
    b"CropBox",
    b"BleedBox",
    b"TrimBox",
    b"ArtBox",
    b"BoxColorInfo",
    b"Contents",
    b"Rotate",
    b"Group",
    b"Dur",
    b"Trans",
    b"Annots",
    b"Tabs",
    b"UserUnit",
    b"PZ",
    b"TemplateInstantiated",
    b"OutputIntents",
    b"AF",
];

/// Was ein Knoten des Seitenbaums behalten darf (Tabelle 30): der Aufbau
/// und die vererbbaren Seitenattribute.
const PAGE_TREE_KEEP: [&[u8]; 9] = [
    b"Type",
    b"Kids",
    b"Count",
    b"Parent",
    b"Resources",
    b"MediaBox",
    b"CropBox",
    b"Rotate",
    b"AF",
];

/// Nimmt `dict` jeden Schlüssel, der nicht in `keep` steht, und bucht ihn.
fn take_unlisted(dict: &mut Dictionary, keep: &[&[u8]], into: &mut Tally) {
    let fremd: Vec<Vec<u8>> = dict
        .iter()
        .map(|(key, _)| key.clone())
        .filter(|key| !keep.contains(&key.as_slice()))
        .collect();
    for key in fremd {
        take(dict, &key, into);
    }
}

/// Die Knoten des Seitenbaums unter `/Pages` — ohne die Seiten selbst.
fn page_tree_nodes(doc: &Document, catalog_id: ObjectId, pages: &[ObjectId]) -> Vec<ObjectId> {
    let mut out = Vec::new();
    let mut seen: BTreeSet<ObjectId> = pages.iter().copied().collect();
    let mut stack: Vec<ObjectId> = doc
        .get_dictionary(catalog_id)
        .ok()
        .and_then(|c| c.get(b"Pages").ok())
        .and_then(|o| o.as_reference().ok())
        .into_iter()
        .collect();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Ok(node) = doc.get_dictionary(id) else {
            continue;
        };
        let Some(Object::Array(kids)) = value_of(node, b"Kids") else {
            continue;
        };
        out.push(id);
        stack.extend(kids.iter().filter_map(|k| k.as_reference().ok()));
    }
    out
}

/// Nimmt den Seitenbeschriftungen ihr Präfix `/P` — den einzigen Klartext im
/// Zahlenbaum `/PageLabels` (Tabelle 159). `/S` und `/St` bleiben: der
/// Betrachter zählt weiter römisch oder arabisch, nur ohne
/// „Kontoauszug <Konto> – “ davor.
///
/// Jeder Wert in `/Nums` wird bearbeitet, als eigenes Objekt wie direkt im
/// Feld, und jeder Knoten unter `/Kids`, gleich wie tief. Bis zur
/// Spur-A-Runde 2 endete die Schleife am ersten direkten Wert, und ein Baum
/// endete still an einer festen Stufe; ein Präfix dahinter stand nach dem
/// Lauf in der Datei (Register #92). Knoten als eigene Objekte laufen über
/// einen Stapel mit `seen` gegen Zyklen; ein direkt eingebetteter Knoten
/// (regelwidrig, `/Kids` verlangt Verweise) wird in seinem Elternknoten
/// bereinigt, nicht an dessen Stelle geschrieben.
fn clean_page_labels(doc: &mut Document, catalog_id: ObjectId, beiwerk: &mut Tally) {
    let Some(root) = doc
        .get_dictionary(catalog_id)
        .ok()
        .and_then(|c| c.get(b"PageLabels").ok().cloned())
    else {
        return;
    };
    let mut nodes: Vec<ObjectId> = Vec::new();
    let mut labels: Vec<ObjectId> = Vec::new();
    match root {
        Object::Reference(id) => nodes.push(id),
        Object::Dictionary(mut dict) => {
            clean_label_node(&mut dict, &mut nodes, &mut labels, beiwerk);
            if let Ok(catalog) = doc.get_dictionary_mut(catalog_id) {
                catalog.set("PageLabels", Object::Dictionary(dict));
            }
        }
        _ => return,
    }
    let mut seen: BTreeSet<ObjectId> = BTreeSet::new();
    while let Some(id) = nodes.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Ok(mut dict) = doc.get_dictionary(id).cloned() else {
            continue;
        };
        clean_label_node(&mut dict, &mut nodes, &mut labels, beiwerk);
        doc.objects.insert(id, Object::Dictionary(dict));
    }
    for id in labels {
        if let Ok(label) = doc.get_dictionary_mut(id) {
            take(label, b"P", beiwerk);
        }
    }
}

/// Ein Knoten des Zahlenbaums `/PageLabels`: nimmt jedem direkten
/// Beschriftungs-Dictionary in `/Nums` sein `/P` und steigt in direkt
/// eingebettete `/Kids` ab; was als eigenes Objekt dasteht, geht auf `nodes`
/// (Knoten) oder `labels` (Beschriftungen).
fn clean_label_node(
    node: &mut Dictionary,
    nodes: &mut Vec<ObjectId>,
    labels: &mut Vec<ObjectId>,
    beiwerk: &mut Tally,
) {
    if let Ok(Object::Array(nums)) = node.get_mut(b"Nums") {
        // Die Werte des Zahlenbaums: jedes zweite Element.
        for value in nums.iter_mut().skip(1).step_by(2) {
            match value {
                Object::Reference(id) => labels.push(*id),
                Object::Dictionary(label) => {
                    take(label, b"P", beiwerk);
                }
                _ => {}
            }
        }
    }
    if let Ok(Object::Array(kids)) = node.get_mut(b"Kids") {
        for kid in kids.iter_mut() {
            match kid {
                Object::Reference(id) => nodes.push(*id),
                Object::Dictionary(inner) => clean_label_node(inner, nodes, labels, beiwerk),
                _ => {}
            }
        }
    }
}

/// Nimmt jedem `/OutputIntents`-Eintrag (Tabelle 365) seine Texte `/Info`,
/// `/OutputCondition` und `/RegistryName`. `/OutputConditionIdentifier` und
/// `/DestOutputProfile` bleiben — sie machen die Datei PDF/A oder PDF/X, und
/// der Bezeichner ist ein Normname (`sRGB IEC61966-2.1`), kein Freitext.
///
/// `holder` ist der Katalog oder eine Seite: PDF 2.0 erlaubt
/// `/OutputIntents` auch dort (14.11.5); bis zur Spur-A-Runde 2 bereinigte
/// der Lauf nur den Katalog (Register #92).
fn clean_output_intents(doc: &mut Document, holder: ObjectId, beiwerk: &mut Tally) {
    const TEXT_KEYS: [&[u8]; 3] = [b"Info", b"OutputCondition", b"RegistryName"];
    let Some(intents) = doc
        .get_dictionary(holder)
        .ok()
        .and_then(|c| c.get(b"OutputIntents").ok().cloned())
    else {
        return;
    };
    let array_id = match &intents {
        Object::Reference(id) => Some(*id),
        _ => None,
    };
    let Some(Object::Array(items)) = resolve(doc, &intents).cloned() else {
        return;
    };
    let mut direct = items.clone();
    for (i, item) in items.iter().enumerate() {
        match item {
            Object::Reference(id) => {
                if let Ok(intent) = doc.get_dictionary_mut(*id) {
                    for key in TEXT_KEYS {
                        take(intent, key, beiwerk);
                    }
                }
            }
            Object::Dictionary(_) => {
                if let Object::Dictionary(intent) = &mut direct[i] {
                    for key in TEXT_KEYS {
                        take(intent, key, beiwerk);
                    }
                }
            }
            _ => {}
        }
    }
    if direct != items {
        match array_id {
            Some(id) => {
                doc.objects.insert(id, Object::Array(direct));
            }
            None => {
                if let Ok(holder) = doc.get_dictionary_mut(holder) {
                    holder.set("OutputIntents", Object::Array(direct));
                }
            }
        }
    }
}

/// Leert jede Seite, die **nicht im Seitenbaum** hängt.
///
/// `/Type /Page` ohne Platz in `/Kids`: eine gelöschte Seite, die ein
/// stehen gebliebener Verweis hält — das `/Dest` eines Links, das `/P` einer
/// Annotation, oder irgendein anderer Schlüssel, den niemand kennt. Der
/// Schwärzungslauf sieht nur die Seiten des Baums (`get_pages`); diese hier
/// ginge mit ihrem ganzen Content-Stream ungeschwärzt in die Ausgabe, und
/// `prune_unreachable` behielte sie, weil sie erreichbar ist (Register #69).
///
/// Es wird nicht der Halter gekappt, sondern die Seite geleert: welcher
/// Schlüssel sie hält, ist nicht abschließend aufzählbar, und ohne Inhalt
/// trägt sie nichts mehr, wer immer sie hält. Was fällt: `/Contents`,
/// `/Annots`, `/Resources`, `/Thumb`, `/AA`, `/B`, `/VP`, `/PresSteps` —
/// alles, was Text, Bilder oder Aktionen tragen kann. `/MediaBox` und
/// `/Parent` bleiben, damit ein Betrachter, der dem Verweis folgt, eine
/// leere Seite bekommt und keinen Fehler. Rückgabe: geleerte Seiten.
fn empty_orphan_pages(doc: &mut Document, tree: &[ObjectId]) -> usize {
    const CONTENT_KEYS: [&[u8]; 8] = [
        b"Contents",
        b"Annots",
        b"Resources",
        b"Thumb",
        b"AA",
        b"B",
        b"VP",
        b"PresSteps",
    ];
    let tree: BTreeSet<ObjectId> = tree.iter().copied().collect();
    let mut emptied = 0usize;
    let ids: Vec<ObjectId> = doc.objects.keys().copied().collect();
    for id in ids {
        if tree.contains(&id) {
            continue;
        }
        let Some(Object::Dictionary(dict)) = doc.objects.get_mut(&id) else {
            continue;
        };
        if dict.get(b"Type").and_then(Object::as_name).ok() != Some(b"Page") {
            continue;
        }
        let mut lost = false;
        for key in CONTENT_KEYS {
            lost |= dict.remove(key).is_some();
        }
        if lost {
            emptied += 1;
        }
    }
    emptied
}

/// XMP-Strom `/Metadata`, die Privatdaten `/PieceInfo` und die zugeordnete
/// Datei `/AF`.
///
/// Katalog und Seiten sind hier schon durch; gemeint ist der Rest — ein
/// **Bild-XObject** mit XMP (so exportiert ein Layout-Programm platzierte
/// Fotos samt `dc:description`, Kamerabesitzer und Aufnahmeort), ein
/// **Form-XObject** mit `/PieceInfo` (Tabelle 95: die Privatdaten des
/// Zeichenprogramms) und jedes Objekt mit `/AF`. Ein Strom ist immer ein
/// eigenes Objekt; ein Durchgang durch `doc.objects` erreicht sie alle.
///
/// Dass diese drei Schlüssel nichts zeichnen und nirgends Pflicht sind,
/// macht das Entfernen gefahrlos: `/Metadata` (14.3.2) und `/PieceInfo`
/// (14.5) sind Beiwerk, `/AF` (14.13) ein zweiter Halter für eine Datei, die
/// ohnehin fallen soll.
fn clear_object_metadata(
    doc: &mut Document,
    xmp: &mut Tally,
    piece_info: &mut Tally,
    files: &mut Tally,
    beiwerk: &mut Tally,
) {
    let ids: Vec<ObjectId> = doc.objects.keys().copied().collect();
    for id in ids {
        let Some(object) = doc.objects.get_mut(&id) else {
            continue;
        };
        let dict = match object {
            Object::Dictionary(dict) => dict,
            Object::Stream(stream) => &mut stream.dict,
            _ => continue,
        };
        take(dict, b"Metadata", xmp);
        take(dict, b"PieceInfo", piece_info);
        take_file_spec(dict, b"AF", files);
        for key in OBJECT_BEIWERK_KEYS {
            take(dict, key, beiwerk);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{dictionary, Dictionary, Stream, StringFormat};

    fn doc_with_info() -> Document {
        let mut doc = Document::with_version("1.5");
        let info_id = doc.add_object(dictionary! {
            "Title" => Object::string_literal("Kontoauszug Mustermann"),
            "Author" => Object::string_literal("Max Mustermann"),
        });
        let pages_id = doc.new_object_id();
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        doc.trailer.set("Info", info_id);
        doc
    }

    #[test]
    fn removes_info_dictionary() {
        let mut doc = doc_with_info();
        let report = strip_metadata(&mut doc);
        assert!(report.info_removed);
        assert!(doc.trailer.get(b"Info").is_err());
        // Auch das Objekt selbst ist weg — nicht nur die Referenz.
        let bytes = {
            let mut buf = Vec::new();
            doc.save_to(&mut buf).unwrap();
            buf
        };
        assert!(!String::from_utf8_lossy(&bytes).contains("Mustermann"));
    }

    #[test]
    fn removes_xmp_metadata() {
        let mut doc = doc_with_info();
        let xmp = doc.add_object(Object::Stream(lopdf::Stream::new(
            Dictionary::new(),
            b"<x:xmpmeta>geheim</x:xmpmeta>".to_vec(),
        )));
        let catalog_id = catalog_of(&doc);
        doc.get_dictionary_mut(catalog_id)
            .unwrap()
            .set("Metadata", Object::Reference(xmp));

        let report = strip_metadata(&mut doc);
        assert!(report.xmp_removed);
        assert!(!doc.objects.contains_key(&xmp));
    }

    #[test]
    fn is_idempotent() {
        let mut doc = doc_with_info();
        strip_metadata(&mut doc);
        let second = strip_metadata(&mut doc);
        assert!(!second.anything_removed(), "{:?}", second.summary());
    }

    const SECRET: &str = "DE89 3704 0044 0532 0130 00";

    fn catalog_of(doc: &Document) -> ObjectId {
        match doc.trailer.get(b"Root").unwrap() {
            Object::Reference(id) => *id,
            _ => unreachable!(),
        }
    }

    fn set_catalog(doc: &mut Document, key: &str, value: Object) {
        let catalog_id = catalog_of(doc);
        doc.get_dictionary_mut(catalog_id).unwrap().set(key, value);
    }

    #[track_caller]
    fn assert_no_leak(doc: &Document, what: &str) {
        let bytes = crate::document::save_to_bytes(doc).unwrap();
        let hits = crate::leaks(&bytes, SECRET);
        assert!(hits.is_empty(), "{what}: {hits:?}");
    }

    #[test]
    fn page_metadata_object_is_deleted_not_just_dereferenced() {
        let mut doc = doc_with_info();
        let xmp = doc.add_object(Object::Stream(lopdf::Stream::new(
            dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
            format!("<x:xmpmeta><dc:title>Kontoauszug {SECRET}</dc:title></x:xmpmeta>")
                .into_bytes(),
        )));
        let page_id = *doc.get_pages().values().next().unwrap();
        doc.get_dictionary_mut(page_id)
            .unwrap()
            .set("Metadata", Object::Reference(xmp));

        let report = strip_metadata(&mut doc);
        assert!(report.xmp_removed);
        assert!(
            !doc.objects.contains_key(&xmp),
            "das Seiten-XMP steht weiterhin als verwaistes Objekt in der Datei"
        );
    }

    #[test]
    fn names_tree_is_removed_as_the_module_documentation_promises() {
        let mut doc = doc_with_info();
        let names = doc.add_object(Object::Dictionary(dictionary! {
            "Dests" => dictionary! {
                "Names" => vec![
                    Object::string_literal("konto"),
                    Object::string_literal(SECRET),
                ],
            },
        }));
        set_catalog(&mut doc, "Names", Object::Reference(names));

        let report = strip_metadata(&mut doc);
        assert_eq!(report.names_removed, 1);
        assert!(!doc.objects.contains_key(&names));
        let catalog_id = catalog_of(&doc);
        assert!(doc
            .get_dictionary(catalog_id)
            .unwrap()
            .get(b"Names")
            .is_err());

        assert_no_leak(&doc, "/Names-Baum");
    }

    #[test]
    fn old_style_dests_dictionary_is_removed_too() {
        let mut doc = doc_with_info();
        let dests = doc.add_object(Object::Dictionary(dictionary! {
            "konto" => Object::string_literal(SECRET),
        }));
        set_catalog(&mut doc, "Dests", Object::Reference(dests));

        let report = strip_metadata(&mut doc);
        assert_eq!(report.names_removed, 1);
        assert!(!doc.objects.contains_key(&dests));
    }

    // -----------------------------------------------------------------------
    // Restdaten: Formulare, Anhänge, JavaScript, Aktionen, Ebenen
    // -----------------------------------------------------------------------

    #[test]
    fn acroform_field_values_are_cleared_and_counted() {
        let mut doc = doc_with_info();
        let field = doc.add_object(Object::Dictionary(dictionary! {
            "FT" => "Tx",
            "T" => Object::string_literal("iban"),
            "V" => Object::String(utf16be(SECRET), StringFormat::Literal),
            "DV" => Object::string_literal(SECRET),
        }));
        set_catalog(
            &mut doc,
            "AcroForm",
            Object::Dictionary(dictionary! {
                "Fields" => vec![Object::Reference(field)],
                "XFA" => Object::string_literal(format!("<xfa>{SECRET}</xfa>")),
            }),
        );

        let report = strip_metadata(&mut doc);
        assert!(report.acroform_removed);
        assert!(report.xfa_removed);
        assert_eq!(report.field_values_cleared, 2);
        assert_no_leak(&doc, "AcroForm /V, /DV und /XFA");
    }

    #[test]
    fn a_widget_reachable_through_annots_loses_its_value_too() {
        // Der harte Fall: das Feld hängt zusätzlich an der Seite. `/AcroForm`
        // zu entfernen genügt hier gerade *nicht* — das Widget bleibt über
        // `/Annots` erreichbar.
        let mut doc = doc_with_info();
        let field = doc.add_object(Object::Dictionary(dictionary! {
            "FT" => "Tx",
            "V" => Object::string_literal(SECRET),
        }));
        let widget = doc.add_object(Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Widget",
            "Parent" => Object::Reference(field),
            "Rect" => vec![0.into(), 0.into(), 100.into(), 20.into()],
        }));
        let page_id = *doc.get_pages().values().next().unwrap();
        doc.get_dictionary_mut(page_id)
            .unwrap()
            .set("Annots", Object::Array(vec![Object::Reference(widget)]));
        set_catalog(
            &mut doc,
            "AcroForm",
            Object::Dictionary(dictionary! {
                "Fields" => vec![Object::Reference(field)],
            }),
        );

        let report = strip_metadata(&mut doc);
        assert_eq!(report.field_values_cleared, 1);
        assert_no_leak(&doc, "Widget über /Annots");
    }

    #[test]
    fn embedded_files_and_javascript_are_counted_before_the_names_tree_goes() {
        let mut doc = doc_with_info();
        let attachment = doc.add_object(Object::Stream(Stream::new(
            Dictionary::new(),
            format!("Kontoauszug\nIBAN {SECRET}\n").into_bytes(),
        )));
        let filespec = doc.add_object(Object::Dictionary(dictionary! {
            "Type" => "Filespec",
            "F" => Object::string_literal("kontoauszug.txt"),
            "EF" => dictionary! { "F" => Object::Reference(attachment) },
        }));
        let names = doc.add_object(Object::Dictionary(dictionary! {
            "EmbeddedFiles" => dictionary! {
                "Names" => vec![
                    Object::string_literal("anhang"),
                    Object::Reference(filespec),
                ],
            },
            "JavaScript" => dictionary! {
                "Names" => vec![
                    Object::string_literal("start"),
                    Object::Dictionary(dictionary! {
                        "S" => "JavaScript",
                        "JS" => Object::string_literal(format!("var iban = '{SECRET}';")),
                    }),
                ],
            },
        }));
        set_catalog(&mut doc, "Names", Object::Reference(names));

        let report = strip_metadata(&mut doc);
        assert_eq!(report.embedded_files_removed, 1);
        assert_eq!(report.javascript_removed, 1);
        assert_no_leak(&doc, "/EmbeddedFiles und /JavaScript");
    }

    #[test]
    fn open_action_and_additional_actions_are_removed() {
        let mut doc = doc_with_info();
        let action = doc.add_object(Object::Dictionary(dictionary! {
            "S" => "JavaScript",
            "JS" => Object::string_literal(format!("app.alert('{SECRET}');")),
        }));
        set_catalog(&mut doc, "OpenAction", Object::Reference(action));
        set_catalog(
            &mut doc,
            "AA",
            Object::Dictionary(dictionary! {
                "WC" => dictionary! {
                    "S" => "JavaScript",
                    "JS" => Object::string_literal(format!("// {SECRET}")),
                },
            }),
        );
        let page_id = *doc.get_pages().values().next().unwrap();
        doc.get_dictionary_mut(page_id).unwrap().set(
            "AA",
            Object::Dictionary(dictionary! {
                "O" => dictionary! {
                    "S" => "JavaScript",
                    "JS" => Object::string_literal(format!("// Seite {SECRET}")),
                },
            }),
        );

        let report = strip_metadata(&mut doc);
        assert!(report.open_action_removed);
        assert_eq!(report.additional_actions_removed, 2);
        assert_no_leak(&doc, "/OpenAction und /AA");
    }

    #[test]
    fn optional_content_properties_are_removed() {
        let mut doc = doc_with_info();
        let ocg = doc.add_object(Object::Dictionary(dictionary! {
            "Type" => "OCG",
            "Name" => Object::string_literal(format!("Ebene {SECRET}")),
        }));
        set_catalog(
            &mut doc,
            "OCProperties",
            Object::Dictionary(dictionary! {
                "OCGs" => vec![Object::Reference(ocg)],
                "D" => dictionary! { "ON" => vec![Object::Reference(ocg)] },
            }),
        );

        let report = strip_metadata(&mut doc);
        assert!(report.optional_content_removed);
        assert_no_leak(&doc, "/OCProperties");
    }

    #[test]
    fn file_attachment_annotations_are_removed_from_pages() {
        let mut doc = doc_with_info();
        let attachment = doc.add_object(Object::Stream(Stream::new(
            Dictionary::new(),
            format!("IBAN {SECRET}").into_bytes(),
        )));
        let annot = doc.add_object(Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "FileAttachment",
            "Rect" => vec![0.into(), 0.into(), 20.into(), 20.into()],
            "FS" => dictionary! {
                "Type" => "Filespec",
                "F" => Object::string_literal("anhang.txt"),
                "EF" => dictionary! { "F" => Object::Reference(attachment) },
            },
        }));
        let other = doc.add_object(Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Square",
            "Rect" => vec![0.into(), 0.into(), 20.into(), 20.into()],
        }));
        let page_id = *doc.get_pages().values().next().unwrap();
        doc.get_dictionary_mut(page_id).unwrap().set(
            "Annots",
            Object::Array(vec![Object::Reference(annot), Object::Reference(other)]),
        );

        let report = strip_metadata(&mut doc);
        assert_eq!(report.file_attachments_removed, 1);
        // Die unbeteiligte Annotation bleibt stehen.
        assert!(doc.objects.contains_key(&other));
        assert_no_leak(&doc, "/FileAttachment-Annotation");
    }

    /// Eine kaputte `/Parent`-Kette, die auf die Seite führt: die Seite ist
    /// kein Träger und behält `/Contents` — und über ihr `/Parent` (den
    /// Seitenbaum) läuft die Bereinigung nicht weiter.
    #[test]
    fn a_parent_chain_onto_the_page_tree_is_not_cleaned() {
        let mut doc = doc_with_info();
        let page_id = *doc.get_pages().values().next().unwrap();
        let content = doc.add_object(Object::Stream(Stream::new(
            Dictionary::new(),
            b"BT (Seitentext) Tj ET".to_vec(),
        )));
        let widget = doc.add_object(Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Widget",
            "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()],
            "T" => Object::string_literal("feld"),
            "Parent" => Object::Reference(page_id),
        }));
        let page = doc.get_dictionary_mut(page_id).unwrap();
        page.set("Contents", Object::Reference(content));
        page.set("Annots", Object::Array(vec![Object::Reference(widget)]));

        let report = strip_metadata(&mut doc);
        assert_eq!(report.annotation_texts_cleared, 1, "nur /T am Widget");
        let page = doc.get_dictionary(page_id).unwrap();
        assert_eq!(
            page.get(b"Contents").ok(),
            Some(&Object::Reference(content))
        );
        assert!(doc.objects.contains_key(&content));
        assert_eq!(doc.get_pages().len(), 1);
    }

    /// Zwei Widgets teilen sich ein Elternfeld: dessen Texte fallen einmal
    /// und werden einmal gezählt — der Bericht nennt, was fiel, nicht, wie
    /// oft es erreicht wurde. Und `/Parent` ↔ `/Kids` ist ein Zyklus, der
    /// enden muss.
    #[test]
    fn a_shared_parent_field_is_cleaned_and_counted_once() {
        let mut doc = doc_with_info();
        let field = doc.new_object_id();
        let widgets: Vec<ObjectId> = (0..2)
            .map(|_| {
                doc.add_object(Object::Dictionary(dictionary! {
                    "Type" => "Annot",
                    "Subtype" => "Widget",
                    "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()],
                    "Parent" => Object::Reference(field),
                }))
            })
            .collect();
        doc.objects.insert(
            field,
            Object::Dictionary(dictionary! {
                "FT" => "Btn",
                "T" => Object::string_literal("gruppe"),
                "TU" => Object::string_literal(format!("Konto {SECRET}")),
                "Kids" => widgets.iter().map(|id| Object::Reference(*id)).collect::<Vec<_>>(),
            }),
        );
        let page_id = *doc.get_pages().values().next().unwrap();
        doc.get_dictionary_mut(page_id).unwrap().set(
            "Annots",
            Object::Array(widgets.iter().map(|id| Object::Reference(*id)).collect()),
        );

        let report = strip_metadata(&mut doc);
        assert_eq!(report.annotation_texts_cleared, 2, "/T und /TU, je einmal");
        assert!(!doc.get_dictionary(field).unwrap().has(b"TU"));
        assert_no_leak(&doc, "/TU am geteilten Elternfeld");
    }

    #[test]
    fn summary_lists_exactly_what_was_removed() {
        let mut doc = doc_with_info();
        let report = strip_metadata(&mut doc);
        assert_eq!(report.summary(), vec!["/Info-Dictionary".to_string()]);
    }

    fn utf16be(text: &str) -> Vec<u8> {
        let mut out = vec![0xfe, 0xff];
        out.extend(text.encode_utf16().flat_map(|u| u.to_be_bytes()));
        out
    }
}
