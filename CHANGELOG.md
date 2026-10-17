# Änderungsverlauf

Alle nennenswerten Änderungen an redact-rs, neueste zuerst.

**Wer eine ältere Fassung einsetzt, liest die Abschnitte dazwischen.** Dieses
Werkzeug schwärzt Bankunterlagen; seine Sicherheitszusagen haben sich zwischen
den Fassungen messbar verschoben. Einträge, bei denen eine ältere Fassung
Geheimnisse in der Ausgabe stehen ließ oder ohne Warnung Erfolg meldete, sind
mit **⚠ Sicherheit** gekennzeichnet — sie sind der Grund zu aktualisieren.

Die Fassungsnummern folgen [Semantic Versioning](https://semver.org/lang/de/).
Solange die erste Stelle `0` ist, kann sich Verhalten zwischen zwei mittleren
Stellen ändern; die dafür wichtigen Punkte stehen jeweils unter *Geändert*.

Grundlage jedes Eintrags ist ein Commit in diesem Repository — nachlesbar mit
`git log <von>..<bis>`. Die Bereiche stehen unter der jeweiligen Überschrift.
Wo der Weg zu einer Fassung über viele Prüfrunden ging, steht hier die
Zusammenfassung und der vollständige Verlauf daneben unter `docs/` — für 0.7.0
in [`docs/pruefverlauf-0.7.0.md`](docs/pruefverlauf-0.7.0.md).

> **Diese Datei erzeugt die Release-Notizen.** `.github/workflows/release.yml`
> schneidet beim Veröffentlichen den Abschnitt der gebauten Version heraus
> (`## <version>`, bis zur nächsten `## `-Überschrift; führende und
> abschließende Leerzeilen sowie `---`-Trenner fallen weg). Beim Release ist
> „Unveröffentlicht“ deshalb in `## <version> — <datum>` umzubenennen; sonst
> findet der Job nichts und meldet eine Warnung statt der
> Verhaltensänderungen. Und es darf nur **einen** Abschnitt
> „Unveröffentlicht“ geben: zwei Überschriften gleichen Namens hießen zwei
> Abschnitte, von denen der Job nur den ersten nähme. Nachgeprüft mit dem
> `awk`/`sed` aus release.yml: für 0.3.0, 0.2.0 und 0.1.0 findet er heute
> 95, 81 bzw. 32 Zeilen.

---

## Unveröffentlicht

### Geändert

* **Die Oberfläche ist überarbeitet** — gleiche Funktionen, gleiche Tasten,
  ruhigeres Bild. Ein eigener Farbsatz für hell und dunkel mit einer
  Akzentfarbe für Auswahl und Fokus; die Seite liegt mit weichem Schatten auf
  einer eigenen, etwas dunkleren Fläche und steht waagerecht in der Mitte.
  Blättern und Zoom sind aus der oberen Leiste in eine Ansichtsleiste über der
  Seite gewandert: die obere Leiste brach bei gewöhnlicher Fensterbreite in
  eine zweite Zeile um. „Exportieren“ ist als Hauptknopf hervorgehoben, der
  Zoomregler zeigt Prozent statt einer Dezimalzahl. Die Seitenleiste hat
  Zwischenüberschriften, eine zweispaltige Legende und eine größere kleine
  Schrift; die Miniaturen zeigen die Zahl der Schwärzungen als neutrale
  Plakette. Der leere Hauptbereich ist eine Ablagefläche mit einem Knopf
  „PDF öffnen …“; ohne Dokument fehlen Miniaturspalte und Auswahlbereich. Der
  Knopf, der den Zoom zurücksetzt, heißt „1:1“ statt ein zweites Mal
  „100 %“, und die Statuszeile wiederholt Seite und Zoom nicht mehr.
  Die vier Trefferfarben sind unverändert, und ihr Kontrast ist weiter gegen
  jede Fläche geprüft, auf der sie liegen können — jetzt auch gegen die Fläche
  um das Blatt.
* **Barrierefreiheit der Oberfläche.** Zurückgenommene Schrift (Hinweise,
  Dateiname, Polsterung, Koordinaten) erreicht jetzt 4,5:1 statt rund 2,5:1;
  Rahmen von Häkchen- und Textfeldern 3:1; der Tastaturfokus ist ein breiter
  Rand in der Akzentfarbe. Alle drei Werte sind getestet.
* **Behoben: die Formen der Trefferarten fehlten.** Kreis, Raute und Dreieck
  neben den Treffern kennt die Standardschrift nicht — sie erschienen als leere
  Kästchen, und die Trefferarten waren nur noch an der Farbe zu
  unterscheiden. Sie werden jetzt gemalt.
* Trefferliste: ganze Zeile anklickbar und hervorgehoben, Seitenzahl in eigener
  Spalte, Werte in fester Zeichenbreite. Auswahlbereich mit Kopfzeile und
  „Aktion“ vor der Liste; Griffe an der Auswahl größer und leichter zu fassen.

---

## 0.7.1 — 2026-10-16

Bereich: `git log v0.7.0..v0.7.1`.

Ein Sicherheitsfix (Register #73) und die Release-Notizen zu 0.7.0 in lesbarer
Form. Wer 0.7.0 einsetzt, aktualisiert wegen #73.

### Sicherheit

* **⚠ Sicherheit: Zeichenketten mit festem Wertebereich tragen keinen
  Klartext mehr durch den Lauf** (Register #73, Spur-A-Runde 1; stilles
  Leck, bis hierher als „Einordnung offen“ mit `#[ignore]` geführt). Die
  Norm sieht an einigen Stellen Zeichenketten vor, die niemand mit einem
  Geheimnis füllt — und die doch jeden Text tragen: `/Lang` am Katalog und
  an einer Annotation, die Zeitstempel `/M` und `/CreationDate`, der Zustand
  `/State` einer Prüfnotiz, die Ansicht `/ExData /3DV /XN`,
  `/FontDescriptor /FontFamily`, `/CIDSystemInfo /Registry` und das
  `/ID`-Paar des Trailers. Alle standen nach dem Lauf wortwörtlich in der
  Ausgabe, ohne Warnung und mit leerem Bericht. Jetzt bleibt ein Wert nur
  **im** Wertebereich der Norm: `/Lang` als Sprachangabe (`de-DE`,
  `zh-Hans-CN`, `es-419`), `/M` und `/CreationDate` als Datum nach 7.9.4,
  `/State` und `/StateModel` aus Tabelle 172, `/Registry` und `/Ordering`
  aus der Adobe-Registry (sonst ersetzt, beide sind Pflicht).
  `/FontFamily` und `/ExData` fallen ganz. `/ID` bekommt eine neue
  Kennung: MD5 der geschriebenen Datei, beide Hälften gleich — dieselbe
  Eingabe ergibt weiter dieselbe Ausgabe. Was fällt, steht im Bericht (am
  Katalog und an Objekten als Beiwerk, an Annotationen als Kommentartext).
  Belege: `zo_b_traeger::b_randfaelle_normstrings` (jetzt ohne
  `#[ignore]`), die Gegenprobe `b_normstrings_im_wertebereich_bleiben`,
  `b_normstrings_melden_was_fiel`, `b_id_ist_neu_und_bestimmt`,
  `b_id_platzhalter_im_strom_wird_nicht_ueberschrieben` und die
  Wertebereiche in `meta::tests`. Mutationsnachweis: jede Korrektur
  einzeln zurückgenommen → je ihr Fall rot.

### Doku

* Der Abschnitt zu 0.7.0 ist zu Release-Notizen zusammengefasst (gegliedert
  nach Sicherheit, Dienstverweigerung, Neu, Geändert, Behoben). Der
  vollständige Verlauf der Prüfrunden steht wortgleich in
  [`docs/pruefverlauf-0.7.0.md`](docs/pruefverlauf-0.7.0.md) und liegt jedem
  Archiv unter `docs/` bei.

---

## 0.7.0 — 2026-09-25

Bereich: `git log v0.6.0..v0.7.0`.

0.7.0 ist das Ergebnis von neun Fix-Runden und zwei Spur-A-Runden (siehe
`CONTRIBUTING.md`), jede eine Gegenprüfung der vorigen mit eigenem Material.
Der Schwerpunkt sind **stille Lecks**: Stellen, an denen eine ältere Fassung
Klartext in der Ausgabe stehen ließ, mit Rückgabewert 0 und ohne Warnung —
oder an denen `--check-leaks` „nicht gefunden“ meldete, obwohl der Text noch
dastand. **Wer mit einer älteren Fassung Bankunterlagen geschwärzt hat, liest
zuerst den Abschnitt „Sicherheit“** und prüft betroffene Ausgaben mit 0.7.0
nach.

Hier steht, was sich für Nutzer ändert, nach Gebieten zusammengefasst. Jeder
Befund mit Registernummer, Messung, Beleg und Mutationsnachweis steht im
vollständigen Prüfverlauf: [`docs/pruefverlauf-0.7.0.md`](docs/pruefverlauf-0.7.0.md).

### ⚠ Sicherheit: Klartext, der den Lauf überlebte

**Metadaten, Annotationen und Beiwerk.** Der Metadatenlauf arbeitete mit
festen Sperrlisten; was nicht darauf stand, blieb erreichbar und damit in der
Datei.

* Katalog, Seitenbaum und Seite behalten nur noch, was auf einer
  **Erlaubnisliste** steht — auch Schlüssel, die keine Norm kennt, fallen.
  Vorher überlebten unter anderem `/Perms` (die Signaturangaben `/Reason`,
  `/Location`, `/ContactInfo`, `/Name`), `/DSS`, `/DPartRoot` (PDF/VT-Daten des
  Kontoauszugdrucks), `/SpiderInfo`, `/Legal`, `/Requirements`, `/Threads`,
  `/B`, `/Collection`, `/URI /Base`, `/VP`, `/PresSteps`, `/Ref`, `/OPI`,
  `/SV` und `/Lock` am Signaturfeld. `/PageLabels` und `/OutputIntents`
  verlieren nur ihre Texte (Präfixe in jeder Tiefe des Zahlenbaums), damit
  Nummerierung und Farbprofil bleiben. Der Bericht nennt das „Beiwerk mit
  Klartext“.
* Jedes Ressourcenverzeichnis behält nur noch die Arten, unter denen ein
  Inhaltsstrom nachschlägt (`/ExtGState`, `/ColorSpace`, `/Pattern`,
  `/Shading`, `/XObject`, `/Font`, `/ProcSet`, `/Properties`).
* Der Trailer der Ausgabe trägt nur noch `/Root`, `/Info`, `/Encrypt`, `/ID`
  und `/Size`.
* **Lesezeichen** (`/Outlines`) fallen ganz. An Annotationen fallen Aktionen
  (`/A`, `/AA`, `/PA`), benannte Ziele und die Texte `/Contents`, `/RC`, `/T`,
  `/Subj`, `/TU` (Tooltip), `/TM`, `/Opt`, `/OverlayText`, `/NM`, `/DS` und die
  Beschriftungen in `/MK`; ein ausdrückliches Ziel auf eine Seite bleibt.
  Preis: Gliederung und Verweise ins Netz oder in andere Dateien
  funktionieren danach nicht mehr. `/DA` bleibt als benannte Lücke stehen.
* Die Bereinigung folgt jetzt dem ganzen Graphen, den eine Annotation
  erreichbar hält — `/Popup`, `/Parent`, `/Kids`, `/IRT`, direkt eingebettete
  Dictionaries, Feldwerte `/V`, `/DV`, `/RV` in jeder Tiefe — und erkennt
  eine Annotation auch an ihrer Form, wenn `/Type` normwidrig `/Annotation`
  oder `/Widget` lautet.
* Weitere Träger, die jetzt fallen: `/Movie /F`, `/Measure`,
  `/RichMediaContent /Assets` und `/RichMediaSettings`, ein `/FileAttachment`,
  das nur ein Popup oder eine Antwort hielt, zugeordnete Dateien `/AF` (der
  Weg, über den ZUGFeRD seine Rechnung einbettet), XMP an Bildern,
  `/PieceInfo` an Formularen, 3D-Annotationen samt Startskript und `/3DU`, das
  `/RO` einer `/Redact`-Annotation, das Vorschaubild `/Thumb` jeder Seite.
  `/Alt` und `/ActualText` werden an jedem erreichten Dictionary geleert.
* Ein Widget, das keine Seite zeigt (nur in den `/Kids` seines Felds), und
  das Ende einer langen Antwortkette verlieren ihr Erscheinungsbild `/AP` und
  die Symbole in `/MK`. Die Symbole eines Druckknopfs werden jetzt wie ein
  `/AP` gelesen und geschwärzt.
* Eine Seite außerhalb des Seitenbaums (gelöscht, aber von einem Link oder
  einer Annotation noch gehalten) ging mit ihrem ganzen Inhalt ungeschwärzt
  in die Ausgabe. Sie wird jetzt geleert, und der Bericht nennt sie.

**Bilder.**

* Ein zweiter Name desselben Bildes auf derselben Seite zeigte die
  unversehrten Bildpunkte mitten im Schwärzungsrechteck. Jetzt wird jeder
  getroffene Name umgebogen.
* Nach dem Kopieren eines Bildes, das an mehreren Seiten hängt, blieb das
  Original mit den Klartext-Bildpunkten in der Datei (hinter geerbten oder
  überzähligen Ressourcen). Es wird jetzt durch ein leeres Bild ersetzt und
  gezählt (`retired_originals`).
* Die Stencil-Maske eines geschwärzten Bildes behielt unter der Zone ihre
  Bits — und die Bits sind die Form des Texts. Die Maske geht jetzt als
  `/SMask` in Bildauflösung hinaus, unter der Zone geschwärzt; eine Maske, die
  sich nicht dekodieren lässt, bleibt mit Warnung stehen.
* Ein Bild in einem Kachelmuster ohne Text, ein Bild auf einer ASCIIHex- oder
  RunLength-kodierten Seite und ein Schwärzungsrechteck zwischen den
  Gitterlinien eines groben, groß gezogenen Bildes blieben ungeschwärzt.
  Eine Seite, deren Bild eine Zone nur am Rand berührte, wurde ganz
  übersprungen. Alle Flächenfragen stellen jetzt dieselbe Frage an das
  Viereck der Platzierung.
* Bewusst offen: der `/Alt` eines Bildes (`/Figure <</Alt …>> BDC`)
  überlebt die Schwärzung der Bildpunkte; `--check-leaks` sieht ihn.

**Textspiegel (`/ActualText`, `/Alt`, `/E`).** Ein Spiegel trägt den Text der
Glyphen ein zweites Mal; blieb er stehen, stand das Geheimnis in der Datei.
Jetzt wird er am Fundort geleert, und zwar auch

* über einer Formulargrenze, in einem Formular über einem inneren Formular,
  in einem Formular, das sich Seiten teilen, und über einem Kachelmuster;
* in einem Formular ohne eigenes `/Resources` und in einem mit eigenen
  Ressourcen, aber ohne `/Properties` (Poppler sucht dann beim Aufrufer — das
  tut der Scan jetzt auch), bei einem `null`-Eintrag im eigenen Verzeichnis
  und unter dem Erscheinungsbild einer Annotation;
* in einer Eigenschaftsliste, die direkt im Verzeichnis steht (Seite,
  Formular, geerbt vom Seitenbaum, geteiltes Objekt), unter mehreren
  Grafikumgebungen desselben Formulars und in einer gleichnamigen,
  überschatteten Liste beim Aufrufer.

Und ein Formular, das eine Schrift, ein Bild, einen Grafikzustand oder ein
Muster nur aus den Ressourcen seines Aufrufers benutzt, sah der Scan gar
nicht: keine Schwärzung, und auch `--check-leaks` fand nichts. Er liest es
jetzt, wie Poppler es zeichnet.

**`--check-leaks` meldete „nicht gefunden“, obwohl der Text dastand.**

* Kodierungen: PDFDocEncoding wurde als Latin-1 gelesen (`€`, Gedankenstrich,
  typografische Anführungszeichen wurden nicht gefunden); UTF-8 mit BOM,
  WinAnsi in Inhaltsströmen und UTF-16LE innerhalb von Strömen fehlten.
* Altrevisionen eines inkrementellen Updates las die Rohsicht nur roh oder mit
  reinem Flate, ihre Dictionaries schlecht, und maskierte Zeichenketten gar
  nicht; ein Wort wie „Livestream“ verschluckte einen echten Strom. Jetzt
  entpackt sie jede Filterkette und liest jedes Literal dekodiert.
* Ein unbekannter oder unlesbarer Filter an beliebiger Stelle der Kette, ein
  Packfilter hinter einem Bildfilter, eine Seite, die sich nicht zerlegen
  lässt, ein Strom, den der Lader anders übernahm, als er in der Datei steht,
  und der Abbruch der Objektsicht ab Verschachtelungstiefe 33 stehen jetzt
  unter **NICHT GEPRÜFT** (Rückgabewert 3). Ein Bildfilter allein oder am Ende
  der Kette bleibt der benannte blinde Fleck.
* Eine Kette mit unbekanntem späterem Glied wird so weit dekodiert, wie es
  geht, statt ganz verworfen.

**Oberfläche.**

* Nach einem zweiten Export zeigte die Nachprüfung das Urteil über die
  **vorigen** Bytes („keine Fundstellen“ über eine Datei, die niemand gelesen
  hatte).
* Wer das Fenster schloss, während die Nachprüfung lief, wurde nicht gefragt;
  jetzt schon — verloren ginge das Urteil über die Datei.
* Ein wörtlich gleicher, bewusst stehen gelassener Text nahm eine
  danebengegangene Schwärzung aus der Suche; von zwei Schreibweisen wurde nur
  die erste gesucht. Entschieden wird jetzt am Fund, jede Schreibweise wird
  gesucht.
* Auf einem Dateisystem ohne Groß-/Kleinschreibung (NTFS, VFAT, SMB, ext4 mit
  `casefold`) bekam eine Datei zwei Kennungen, und zwei gleichzeitige Exporte
  auf dieselbe Datei wurden nicht abgewiesen. Die Kennung misst jetzt das
  Verzeichnis.

**Weitere.**

* `--padding nan` machte **jede** Schwärzung wirkungslos und meldete trotzdem
  „Deck-Rechteck gezeichnet“. Koordinaten werden jetzt am Ergebnis geprüft
  (`Rect::is_usable`).
* **Kein Kernabzug mehr:** stürzte der Lauf ab, schrieb der Kernel den
  Arbeitsspeicher samt Klartext und Passwort auf die Platte. `redact-rs`
  schaltet das unter Linux als erste Anweisung ab (`prctl`), auf den übrigen
  Unix-Systemen schwächer über `setrlimit(RLIMIT_CORE, 0)`. **Unter Windows
  ist nichts umgesetzt.** Nebeneffekt unter Linux: `gdb` und `strace`
  brauchen `root`. `redact-cli` steht dafür unter `#![deny(unsafe_code)]` mit
  einer benannten Ausnahme; `#![forbid(unsafe_code)]` gilt für sieben statt
  acht Crates.

### ⚠ Sicherheit: Dienstverweigerung

Präparierte, oft sehr kleine Dateien konnten Minuten Laufzeit und Gigabyte an
Speicher kosten, ohne dass eine Grenze griff — teils mit Rückgabewert 0.
Jetzt fallen sie mit einer Budgetmeldung.

* **Entpackbomben in Filterketten:** `--max-decompressed-mb` galt nicht für
  Ketten mit einem Glied, das die Vorprüfung nicht auspackte, nicht für den
  Vorspann vor einem Bildfilter, nicht für Ketten als Verweis, rohes Deflate
  oder einen Schlüssel mit `#xx`, und gebucht wurde nur die Ausgabe des
  letzten Glieds. Jetzt gilt das Budget für die ganze Kette, in Vorprüfung,
  Schreibpfad und Orakel gleich.
* **Bilder:** ein JPEG mit mehr Bildpunkten, als sein Dictionary angibt, und
  eine reelle Breite (`/Width 100.0`) umgingen `--max-image-mb`.
* **Objektspeicher:** `--max-parsed-mb` zählte nur die entpackten Ströme; eine
  dicht geschriebene unkomprimierte Datei belegte ein Vielfaches ihrer Größe.
  Der Objektspeicher wird jetzt beim Lesen gerechnet und ab dem 60-Fachen von
  `--max-parsed-mb` (Vorgabe 960 MB) abgelehnt.
* **Vervielfachung über Seiten:** ein `/Annots`-Array, ein Inhaltsstrom oder
  ein Formular, das sich viele Seiten teilen, kostete Seiten × Inhalt.
  Abgerechnet wird jetzt über das ganze Dokument; jeder Strom bringt sein
  Guthaben einmal ein.
* **Getaggte Seiten:** verschachtelte `BDC`-Klammern über Formularen und
  Textoperationen ließen die Zuordnung Spiegel→Platzierung ohne Decke wachsen;
  jetzt gedeckelt, mit Warnung, wenn dabei etwas weggelassen wird.
* **Quadratische Läufe:** eine Kette aus Verweisobjekten, die Spiegel in den
  Formularen aller Seiten, die Rohsicht je Block und ein NaN im
  Rechteckgitter (Abbruch erst nach 90 s) laufen jetzt linear oder sind
  gedeckelt.

### Neu

* **Die Oberfläche prüft nach jedem Export selbst nach.** Sie liest die
  geschriebenen Bytes zurück und sucht die Texte, die sie gerade geschwärzt
  hat (`redact_pdf::leaks_many`). Die Zeile dazu nennt das Ergebnis, den
  Vorbehalt, dass nur diese Liste geprüft ist, und die Zahl der selbst
  gezogenen Rechtecke ohne bekannten Text, über die sie nichts sagen kann.
  Gesucht werden höchstens 1 000 verschiedene Texte.
* **Der Export läuft auf einem eigenen Faden:** die Oberfläche bleibt
  bedienbar, ein zweiter Export derselben Datei wird abgewiesen, und stürzt
  der Faden ab, heißt es „es wurde keine Datei geschrieben“.
* **Der Ort einer Fundstelle ist maschinenlesbar:** Sicht, Seite und
  Objekt-Id, jeweils mit ihrer Herkunft.
* **`--check-leaks` hat ein Budget** — dasselbe wie `--max-decompressed-mb` —
  und nennt, was es deshalb nicht gesehen hat, als `NICHT GEPRÜFT`, auch
  unter `--quiet`.
* **`--check-leaks` sucht alle Begriffe in einem Durchgang** und entpackt und
  parst die Datei nur noch einmal: 1 000 Begriffe kosten kaum mehr als einer.
* **LZW-gepackte Bilder** werden dekodiert und geschwärzt, statt den Lauf
  ohne Ausgabedatei zu beenden.
* **Audit-Log:** vier neue Zahlen, `metadata.outlines_removed`,
  `annotation_actions_removed`, `annotation_texts_cleared` und
  `optional_content_names_cleared` (additiv, Vorgabe 0, kein Schemabruch).
* **`docs/vorher-nachher.md`** zeigt dieselbe Seite vor und nach dem Lauf,
  daneben die vollständigen `--check-leaks`-Läufe — bitgleich reproduzierbar
  mit `./scripts/make-preview.sh`.

### Geändert

* **Rückgabewert 3 von `--check-leaks` hat drei Bedeutungen:** ein Fund, oder
  — auch ohne Fund — mindestens eine Stelle, die nicht geprüft werden konnte
  (Entpackbudget, Verschachtelungstiefe, unlesbarer Filter, unzerlegbare
  Seite). Wer `0` als „sauber“ liest, muss `3` ohne Fund mitbehandeln.
* **Höchstens 1 000 Begriffe je `--check-leaks`-Lauf**
  (`redact_core::MAX_CHECK_NEEDLES`); die Oberfläche deckelt ihre Nachprüfung
  mit derselben Zahl, gezählt in Schreibweisen.
* **Nur `/ActualText` wird als Widerspruch gemeldet.** `/Alt` und `/E` dürfen
  von den Glyphen abweichen; eine gewöhnliche getaggte Datei endet damit nicht
  mehr mit 3. Eine Annotation mit Text, aber ohne Erscheinungsstrom, gilt
  nicht mehr als Deckungslücke (ihr Text fällt mit den Metadaten).
* **Eine Datei, die sehr viele Seiten mit demselben Inhalt füllt, wird
  abgelehnt** (Vervielfachung, siehe oben); einen Schalter dafür gibt es
  nicht.
* Eine Filterkette über dem Entpackbudget lehnt die Vorprüfung als Ganzes ab
  (Rückgabewert 1), auch unter `--check-leaks`.
* Die feste Rohgrößen-Grenze für `LZWDecode` und `ASCII85Decode` ist gefallen:
  große ASCII85-Bilder, wie Distiller sie schreibt, werden geschwärzt statt
  abgelehnt; mehr als das Budget entpackt keiner.
* Eine Seitennummer über 4 294 967 295 in einer Review-Datei oder unter
  `--manual-regions` wird mit Rückgabewert 1 abgelehnt (vorher Absturz im
  Debug-Build, im Release eine Ausgabe mit Warnung über „Seite 0“).
* Der Text einer Annotation, die sich Seiten teilen, steht einmal im
  Ergebnis, auf der ersten Seite, die sie zeigt.
* Die Statuszeile der Oberfläche führt mit dem Fund und ist auf höchstens
  400 Zeichen gekürzt; der Rest steht in den Warnungen. Die Warnungen eines
  Exports bleiben je Ausgabedatei stehen (höchstens zehn), Abbrüche und
  unvollständige Prüfungen stehen dort auch.
* API von `redact-pdf`: `extract_page` entfällt, `PdfExtractor::extract_lenient`
  liest in einem Durchgang; `writing_key` und `document::same_file` sind
  öffentlich, `LeakCheck::unchecked` trägt mehr Zähler.
* MB heißt in allen Angaben 1024² Byte.

### Behoben

Gewöhnliche Dateien, die abgelehnt wurden, und falsche Alarme:

* Eine Seite mit einem Kommentar vor einer Leerzeile (oder NUL und
  Seitenvorschub als Leerraum) galt als nicht zerlegbar.
* Ein Inline-Bild, das eine Zone nur berührte, beendete den Lauf ohne
  Ausgabedatei. Ein ASCII85-Strom, der exakt ins Restbudget passte, wurde
  abgelehnt.
* `/Filter`, `/DecodeParms` und Werte darin als Verweis wurden nicht
  aufgelöst; die Datei galt als nicht zerlegbar oder wurde ohne Prädiktor
  dekodiert.
* Das Löschen eines Schlüssels nahm das referenzierte Objekt mit — bei einem
  `/Contents`, das ein Lesezeichen teilte, verlor die Ausgabe ihre Seite. Die
  Erreichbarkeitsprüfung löschte ein XObject hinter 70 verschachtelten Arrays.
* Bei einem gedrehten Bild nahm eine Schwärzung in den Ecken seiner Hülle
  einem unversehrten Bild den Ersatztext; ein nicht dekodierbares Bild nimmt
  ihn nur noch der getroffenen Platzierung.
* Ein Textspiegel über einem mehrfach platzierten Formular und ein Formular,
  das erst ein inneres zeichnet, ergaben falsche Widersprüche; genau
  erreichte Decken meldeten einen Verlust, der nicht eintrat.
* Der Bericht meldete Entfernungen, die keine waren: gezählt wird jetzt am
  aufgeräumten Dokument, nach `prune_unreachable`.
* Oberfläche: eine abgewählte Zeile ohne Leerraum galt als Leck; eine
  gelöschte Trefferzeile ebenso; verschwundene Schreibweisen wurden nicht als
  Entwarnung gezählt; eine deckungsgleiche Handregion hieß „geschützt durch
  Ihre Liste“; die Absagen bei Strg+Pfeil sprachen vom Verschieben und rieten
  zu Seiten, die es nicht gibt.
* `--help` widersprach sich beim Rückgabewert und nannte UTF-16LE nicht;
  `.gitignore` deckt `Kontoauszug_geschwaerzt.PDF`; `scripts/make-preview.sh`
  löst ein relatives `CARGO_TARGET_DIR` gegen das Aufrufverzeichnis auf.

### Doku

* **⚠ Die README beschrieb das Audit-Log falsch.** Seit 0.6.0 hat `effect`
  einen fünften Befund, `off_page`; die Gleichung, an der ein Prüfer die
  wirkungslosen Regionen nachrechnet, lautet
  `applied + covered + degenerate + missing_page + off_page = requested`. Wer
  die vierteilige Fassung aus 0.5.0 benutzt, übersieht eine Region.
* **⚠ `SECURITY.md` kannte `--check-leaks` nicht** und nannte für den
  Rückgabewert 3 nur die Stapelbedeutung; ein Leckfund ließ sich so als bloß
  unvollständige Prüfung deuten. Jetzt mit eigenem Abschnitt und der Tabelle
  aller Gründe für „nicht geprüft“.
* README und `SECURITY.md` nennen die Grenzen vollständig
  (`MAX_TO_UNICODE_BYTES`, `MAX_CACHED_FONT_ENTRIES`, die MediaBox-Heilung
  „A4 angenommen“, das Budget von `--check-leaks`), die Tastentabelle kennt
  `Strg+R`, `Strg+Pfeil` und `Umschalt+Tab`, und die Liste der Änderungen,
  die Rückgängig erfasst, ist vollständig.
* Jede Messzahl in README, `SECURITY.md` und im Prüfverlauf ist an einen Lauf
  oder eine Konstante gebunden; ein Test wird rot, wenn sie sich ändert.

### Bau und Mitwirkung

* Der Pfad des Build-Rechners steht nicht mehr im Binary (ein zweiter
  `--remap-path-prefix` für das Zielverzeichnis), gemessen an einem lokalen
  Bau.
* Die CI fährt Clippy auch für Windows; das Dev-Profil baut mit
  `debug = "line-tables-only"`.
* Die Arbeitsweise mit drei Spuren und der Probenliste
  [`PRUEFLISTE.md`](PRUEFLISTE.md) steht in `CONTRIBUTING.md`.

### An der Fassungsgeschichte selbst

Der Commit-Verlauf wurde einmalig umgeschrieben: 67 Nachrichten trugen eine
Sitzungs-URL des Werkzeugs, mit dem sie entstanden sind. **Jeder Commit-SHA
hat sich geändert, auch die der Tags `v0.1.0` bis `v0.6.0`.** Der Inhalt ist
an jedem Tag byteidentisch geblieben (nachgeprüft am Baum-Hash), die
veröffentlichten Artefakte und Release-Notizen sind unberührt. Die
Build-Provenienz der Releases 0.4.0 bis 0.6.0 nennt aber weiterhin die
**alten** SHAs.

---

## 0.6.0 — 2026-08-05

Bereich: `git log v0.5.0..v0.6.0`.

Dritter Durchgang. Er hat abgearbeitet, was die zweite Pruefung belegt
hinterlassen hatte — und dabei ist dreimal dieselbe Fehlerklasse
aufgetaucht: **eine Decke, die die falsche Einheit zaehlt.**

### ⚠ Drei Schutzdecken, die kein Test hielt

Ein Pruefer hatte sie alle drei auf „unbegrenzt" gesetzt: 1 034 Tests
blieben gruen. Gemessen schalten die Mutationen mehrere hundert Megabyte
Schutz und eine Quadratik ab. Sie haengen jetzt an deterministischen
Zaehlern statt an einer Uhr.

* Die Schriftendecke zaehlte **Verzeichnisse** — vierzig Namen in *einem*
  Verzeichnis kosteten 279 MB bei Zaehlerstand 1. Sie zaehlt jetzt
  Tabelleneintraege.
* Die Operationsdecke sah den vollstaendigen `/Resources`-Klon nicht, den
  der Zwischenspeicher daneben hielt: Stroeme mit null Operationen wogen
  null und wurden deshalb ausnahmslos behalten.
* Der Test fuer die Schichtdecke prueste gegen die Konstante, die er
  absichern sollte — wer sie anhob, hob die Schranke mit an.

### Leistung, jeweils an einer Messreihe belegt

* Die Schwaerzung legte je Bereich einen Bitvektor ueber **alle** Zeichen
  der Textoperation an und behielt ihn: 227 kB Eingabe kosteten 38,2 s und
  1 860 MB, jetzt 0,213 s und 92 MB.
* Die Vorauswahl der Bereiche war ein Streifen ueber **eine** Achse — bei
  einer Spalte, wie sie ein Kontoauszug erzeugt, lieferte sie exakt das
  volle Produkt und siebte damit nichts aus.
* Die Schrift lag als **Wert** im Grafikzustand und wurde bei jeder
  Fontwahl vollstaendig geklont. Der Zwischenspeicher aus 0.4.0
  verhinderte das erneute Parsen, nicht das Klonen; der teure Fall hatte
  sich nur verschoben.
* Dieselbe Schrift wird jetzt je Seitendurchlauf einmal geparst, gleich
  unter wie vielen Namen sie steht: 300 Namen auf dasselbe Objekt kosteten
  rund 18 s und etwa 2 GB, jetzt 0,077 s und 54 MB.
* In der Oberflaeche fielen zwei Rechnungen von 1,6 s und 26,7 s auf 0,109
  und 0,144 s. Ein Schluessel war dafuer nicht noetig — aber die
  Reihenfolge, auf die sich das stuetzt, war bisher nur eine
  Implementierungseigenschaft und ist jetzt als Test zugesagt.

### Geaendert

* **Kommandozeile und Oberflaeche sagen bei einem Rechteck neben der Seite
  dasselbe.** Die CLI meldete es als Schwaerzung und warnte erst hinterher;
  jetzt faellt der Befund auch dort **vor** der Messung, mit denselben
  Worten und einer eigenen Zahl in der Zusammenfassung.

### An dieser Datei selbst

* **Die Release-Notizen von 0.5.0 sind unvollstaendig, und zwar durch einen
  Ablauffehler.** Waehrend 0.5.0 vorbereitet wurde, trug ein Beitrag seine
  Punkte — darunter `--check-leaks` — unter „Unveroeffentlicht" ein, weil der
  Abschnitt `## 0.5.0` zu dem Zeitpunkt schon geschrieben war. Der Job
  schneidet beim Veroeffentlichen nur den Abschnitt der gebauten Version
  heraus; die Punkte sind also **ausgeliefert**, standen aber nicht in den
  Notizen. Sie sind hier unter 0.5.0 nachgetragen.

  Die Lehre daraus steht im Kopf dieser Datei: wer waehrend einer laufenden
  Freigabe etwas eintraegt, traegt es in den Abschnitt der Version ein, die
  gerade gebaut wird — nicht darueber. Ein Rechenweg, der das erzwingt, waere
  besser als eine Regel; solange es ihn nicht gibt, gehoert der Abgleich
  zwischen `## <version>` und `git log` in den Freigabeweg.

---

## 0.5.0 — 2026-08-05

Bereich: `git log v0.4.0..v0.5.0`.

Zweiter vollstaendiger Pruefdurchgang. Er hat zuerst gegen die eigenen
Aenderungen aus 0.4.0 gearbeitet — mit einem zweiten Abzug von 0.3.0 und
einer randomisierten Differenzsuche ueber 1 300 Anordnungen, denn nur so
laesst sich „Regression" von „war schon immer so" trennen.

### ⚠ Zwei Regressionen aus 0.4.0

* **Die Schichtwahl zerriss eine IBAN.** Der Code waehlte die Schicht mit
  dem groessten erreichten Wert, obwohl der Kommentar „am dichtesten davor"
  sagte — an einer gewoehnlichen Tabelle mit einem zu langen
  Empfaengernamen wanderte das erste IBAN-Stueck dadurch in eine Schicht
  und das zweite zurueck in die andere. 0.3.0 fand die IBAN, 0.4.0 meldete
  null Treffer bei Rueckgabewert 0. Ueber 3 900 zufaellige Anordnungen
  belegt: kein einziger Fall, den 0.3.0 fand und 0.5.0 verliert.
* **Eine Bildmaske deckte wieder auf, was verborgen war.** Wo `/SMask` und
  `/Mask` nebeneinander stehen, befolgte die eine Stelle das `/SMask` und
  die andere schrieb das `/Mask` mit. Es gibt jetzt eine einzige
  Entscheidungsstelle, aus der sich beide ableiten.

### ⚠ Wege, den Rechner lahmzulegen

* **Eine ToUnicode-Tabelle aus 6 kB ergab einen Abbruch**, aus 3 kB fast
  7 GB. Die Decke zaehlt Bytes und nicht Eintraege — eine zweite Bombe in
  derselben Funktion (ein sehr langer Zielstring) waere sonst durchgekommen.
* Die Hilfsdatei-Leser, die Bildmasken-Rekursion und vier superlineare
  Stellen sind geschlossen; die Konfliktaufloesung faellt von 74,6 s auf
  0,21 s bei 100 000 Kandidaten, die Trefferkoordinaten von 42,3 s auf
  0,078 s bei 32 000 Treffern.

### Neu

* **`--check-leaks <TEXT>` prueft eine fertige Datei auf Restdaten** — im
  ausgelieferten Binary, ohne Quelltext und ohne Netz. Die Anleitung dazu
  verlangte bisher eine Rust-Toolchain und einen Klon des Repositories. Die
  Gegenprobe: bei einer IBAN in einem komprimierten Objektstrom geben
  `pdftotext`, `strings` und `grep` uebereinstimmend Entwarnung.
* **Die Oberflaeche ist ohne Maus benutzbar.** Ein Rechteck liess sich nur
  mit der Maus erzeugen — und auf einem Kontoauszug sind Anschrift,
  Kontonummer und Kontoinhabername genau die Stellen, die von Hand gezogen
  werden muessen. `Strg+R` legt eines an, `Strg+Pfeil` aendert die Groesse.
  Ausserdem zerschnitt ein einziges ausgegrautes Bedienelement die
  Tabulator-Kette.
* **Eine entartete Seitengroesse brachte Oberflaeche und Kommandozeile
  auseinander**: die IBAN blieb im Fenster-Export stehen, waehrend die CLI
  mit derselben Einstellung beide Seiten schwaerzte.


### Nachgetragen

> Die folgenden Punkte sind **mit 0.5.0 ausgeliefert**, standen beim
> Veroeffentlichen aber noch unter „Unveroeffentlicht" und fehlen deshalb in
> den Release-Notizen von 0.5.0 auf GitHub. Hier stehen sie an der richtigen
> Stelle. Der Ablauf, der das verhindert, ist in 0.6.0 vermerkt.

### Neu

* **Die Nachprüfung steckt jetzt im ausgelieferten Binary:
  `--check-leaks <TEXT>`.** Der Schalter beantwortet die Frage, um die es bei
  einer Schwärzung am Ende geht — *steht dieser Text noch in der Datei?* — und
  sucht dafür auf allen Ebenen, auf denen ein Geheimnis überleben kann: rohe
  Dateibytes, jeder `stream … endstream`-Block (auch Flate-dekomprimiert),
  jedes Stream-Objekt dekodiert, die Objekte in `/ObjStm`-Containern, jedes
  Zeichenketten-Objekt unter jedem Schlüssel — in UTF-8, Latin-1/PDFDoc,
  UTF-16BE und als Hex-String.

  **Neu ist nicht die Suche, sondern ihre Erreichbarkeit.** `redact_pdf::leaks`
  gab es schon; der Schalter reicht sie durch und baut nichts nach. Bis
  einschließlich 0.4.0 war sie aber nur als Bibliotheksfunktion zu haben, und
  die README verwies dafür auf `cargo add --path …/crates/redact-pdf`. **Im
  Release-Archiv liegt kein Quelltext** (nachgesehen:
  `tar -tzf … | grep -c crates/` ⇒ 0), das Binary hatte kein entsprechendes
  Unterkommando, und der Verweis in der ausgelieferten README zeigte ins
  Leere. Wer nur das Release hatte — also die Zielgruppe —, konnte die
  wichtigste Kontrolle dieses Werkzeugs nicht ausführen, es sei denn mit
  Rust-Toolchain, Netzzugang zu crates.io und einem Klon eines privaten
  Repositories. Für ein Werkzeug, dessen erstes Versprechen „keine Cloud, keine
  Netzverbindung“ lautet, war das ein Bruch.

  Drei Festlegungen dazu:

  * **Rückgabewert `3` für einen Fund**, `0` für „keiner der Begriffe steht
    noch darin“. Ein Fund ist weder ein Verarbeitungsfehler (`1`) noch ein
    Bedienfehler (`2`): der Lauf ist gelungen, das *Ergebnis* ist es nicht.
    `0` wäre gefährlich — `redact-rs out.pdf --check-leaks "$IBAN" && versenden`
    verschickte die Datei mit der IBAN darin.
  * **Die Suchbegriffe sind Geheimnisse.** Auf der Kommandozeile stehen sie in
    der Prozessliste und in der Shell-Historie; `--check-leaks -` liest sie
    zeilenweise von der Standardeingabe
    (`redact-rs out.pdf --check-leaks - < begriffe.txt`) und nimmt keinen der
    beiden Wege. Nachgemessen mit `ps -o args=` während des Laufs.
  * **„Nichts gefunden“ wird nicht zur neuen falschen Entwarnung.** Jeder
    saubere Lauf sagt auf stdout dazu, dass damit genau diese Liste geprüft ist
    und sonst nichts.

  Ein Schalter des Schwärzens neben `--check-leaks` (`-o`, `--review`,
  `--patterns`, `--audit-log`, `--action` …) wird abgelehnt (`2`), statt
  wirkungslos mitzulaufen. Eine **verschlüsselte** Datei wird abgelehnt statt
  durchsucht: darin stehen die Zeichenketten verschlüsselt, eine Bytesuche
  fände auch dann nichts, wenn das Geheimnis noch darin steht. Die Eingabedatei
  geht durch dieselbe Typ- und Größenprüfung wie jede andere — eine benannte
  Pipe hält den Lauf nicht an.

### Behoben

* **Sieben relative Verweise in der ausgelieferten README zeigten ins Leere**,
  darunter der auf `crates/redact-pdf/src/audit_bytes.rs` — also genau der auf
  die Funktion, die den Abschnitt „Prüfen, ob die Schwärzung gewirkt hat“
  tragen sollte. Betroffen waren außerdem `crates/redact-pdf/tests/known_leaks.rs`,
  `crates/redact-pdf/tests/marked_content.rs`,
  `crates/redact-cli/tests/cli_and_gui_agree.rs`,
  `.github/workflows/release.yml`, `.github/workflows/ci.yml` und die Anleitung
  `cargo add --path /pfad/zu/redactrs/crates/redact-pdf`.

  Die Dateien sind weiterhin beim Namen genannt, aber nicht mehr verlinkt; der
  Abschnitt „Prüfen, ob die Schwärzung gewirkt hat“ ist neu geschrieben und
  **allein mit dem Release durchführbar**. Dass im Archiv kein Quelltext liegt,
  steht jetzt bei den Archivinhalten.

* **Die Aussage zu `--remap-path-prefix` im Eintrag zu 0.4.0 war zu weit
  gefasst.** Im ausgelieferten glibc-Binary stehen weiterhin drei Pfade des
  Build-Rechners — Build-Skript-Ausgaben unter `OUT_DIR`, die der Schalter
  nicht erfasst. musl und beide Windows-Artefakte sind sauber. Die Zahlen und
  die Erklärung stehen dort; die Behebung im Bauweg steht aus
  (`.github/workflows/`, z. B. `RUSTFLAGS` um ein zweites
  `--remap-path-prefix` für `$CARGO_TARGET_DIR` ergänzen oder die
  Debug-Informationen des Release-Baus abschalten).

### Geändert

* **Die Binärgrößen stehen nicht mehr als Zahlenreihe in der README.** Sie
  waren als v0.3.0-Messung ausgewiesen — ehrlich, aber mit v0.4.0 überholt.
  Die Akzeptanztabelle nennt jetzt nur noch das größte Artefakt und die
  Grenze; die Zahlen der Fassung, die jemand tatsächlich heruntergeladen hat,
  misst eine Zeile (`stat -c '%s %n' redact-rs`). Für v0.4.0 nachgemessen und
  gegen `SHA256SUMS-BINARIES` geprüft: `redact-rs.exe` 10 766 336,
  `redact-rs-gui.exe` 9 999 360, Linux/glibc 15 041 568, Linux/musl
  5 055 416 Byte — alle vier gewachsen, alle vier weit unter 30 MB.

---

## 0.4.0 — 2026-08-05

Bereich: `git log 9aa4808..v0.4.0`.

Diese Fassung ist das Ergebnis einer vollständigen Expertenprüfung aus fünf
Blickwinkeln — Bedrohung für den ausführenden Rechner, Kernversprechen der
Schwärzung, Bedienung, Doku und Freigabeweg, Leistung und Einfachheit. Jeder
Befund darin ist an einem echten Lauf belegt; die Zahlen unten sind gemessen,
nicht geschätzt.

### Neu

* **Die automatische Erkennung lässt sich abschalten — ganz und je Muster.**
  `--no-patterns` gab es schon; dazu kommen `--disable-pattern <ID>`
  (mehrfach oder kommagetrennt), der Schlüssel `disabled_patterns` in der
  Einstellungsdatei, ein Häkchen „Automatisch suchen“ samt aufklappbarer
  Musterliste in der Oberfläche und das Feld `patterns` im Audit-Log.

  **Der wichtigste Teil ist nicht der neue Schalter, sondern die Ansage.** In
  0.3.0 war `--no-patterns` **vollständig stumm**: kein Satz in der
  Zusammenfassung, keine Warnung auf stderr, kein Feld im Audit-Log
  (`detection_notice` und `PatternRecord` gibt es dort noch nicht — nachgesehen
  an `9aa4808`). Eine mit `--no-patterns` erzeugte Datei war von einer
  vollständig geprüften nicht zu unterscheiden — nicht am Ergebnis, nicht am
  Rückgabewert, nicht am Nachweis. „0 Treffer“ heißt dort nicht *nichts
  gefunden*, sondern *nicht gesucht*. **Wer 0.3.0 mit `--no-patterns`
  eingesetzt hat, sieht den Ergebnissen das nicht an und muss es aus dem
  Aufruf rekonstruieren.**

  Ab dieser Fassung sagt es jeder der drei Wege ausdrücklich, und alle drei
  speisen sich aus derselben Angabe:

  * die Zusammenfassung auf stdout, **direkt unter der Trefferzahl** — nicht
    nur auf stderr, denn `redact-rs … > bericht.txt` behielte sonst genau die
    harmlose Hälfte: `Automatische Erkennung: abgeschaltet (--no-patterns). …`
    bzw. `Automatische Erkennung: 1 Muster abgeschaltet (konto_nr). …`;
  * die Kopfzeile der Trefferliste in der Oberfläche
    (`Automatische Suche AUS — nicht gesucht, nur von Hand: …`), dazu ein Satz
    in Warnfarbe unter dem Schalter;
  * das Audit-Log als Feld `patterns`, das **auch dann** dasteht, wenn nichts
    abgeschaltet war (`{"all_disabled": false, "disabled": []}`) — ein Feld,
    das nur im Ausnahmefall erschiene, machte ein Log mit abgeschalteter
    Erkennung ununterscheidbar von einem Log einer älteren Fassung.

  Der **Rückgabewert bleibt 0**. Eine abgeschaltete Erkennung ist eine
  Anweisung des Aufrufenden und keine Deckungslücke: die Analyse hat das
  Dokument vollständig gelesen und auf Geheiß nach weniger gesucht. Spränge
  die 3 auch hier an, wäre sie für die Fälle wertlos, für die es sie gibt.
  Wer im Skript darauf prüfen will, liest `patterns` aus dem Audit-Log.

  Ein **unbekannter Musternamen beendet den Lauf mit Rückgabewert 2** und einer
  Meldung, die alle gültigen nennt; abgeschaltet und geschwärzt wird dann
  nichts. Ein stillschweigend übergangenes `--disable-pattern iban` sähe aus
  wie eine Abschaltung und wäre keine. `no_patterns` gibt es in der
  Einstellungsdatei bewusst **nicht** — der Schalter hat kein Gegenstück, aus
  einer Datei heraus wäre er nicht mehr zu widerrufen; `disabled_patterns` ist
  widerrufbar, weil eine Liste auf der Kommandozeile die aus der Datei ersetzt.

### ⚠ Sicherheit

* **Zwei weitere Stellen, an denen der Interpreter nichts sah, sind jetzt
  laut.** Beide endeten vorher mit „Treffer 0, Rückgabewert 0“ — dem
  schlimmsten Ergebnis, weil es sich wie „geprüft und sauber“ liest. Beide
  setzen jetzt den Rückgabewert **3** und stehen mit `NICHT GEPRÜFT` auf
  stderr:

  * ein **Form-XObject, das in `/Resources` steht, aber nirgends gezeichnet
    wird** — sein Text wurde nicht durchsucht. Ein Formular ohne Text bleibt
    unerwähnt, und eines, das nur eine andere Seite zeichnet, gilt nicht als
    Lücke; sonst spränge der Wert bei geteilten Ressourcen ständig an;
  * eine **weiche Maske (`/ExtGState /SMask /G`), die sich nicht lesen lässt** —
    keine eigene Objekt-Id, kein lesbarer Strom, nicht dekodierbar, zu tief
    verschachtelt oder nur teilweise zerlegbar.

  Eine weiche Maske, die sich **lesen** lässt, wird seit dieser Runde betreten
  und ihr Text mitgeschwärzt: sie ist ein vollwertiges Form-XObject mit eigenem
  Text, das kein `Do` je erreicht — der Interpreter kam bisher nie hinein,
  während `pdftotext` die IBAN im Klartext las.
* **Übereinander gedruckter Text wird wieder lesbar zusammengesetzt.**
  Fett-Imitat, Schlagschatten, Rückkern in einer `TJ`-Operation und mehrere
  Erscheinungsströme derselben Annotation verschränkten die Zeilenbildung
  zeichenweise zu `IIBBAANN::  DDEE8899 …` — kein Muster traf mehr, und der
  Lauf endete mit „Treffer 0“.
* **Was die Eingabe versteckt, bleibt in der Ausgabe versteckt.** Beim
  Schwärzen wird ein betroffenes Bild neu kodiert und sein Dictionary neu
  aufgebaut; eine dabei verlorene Maske (`/SMask`, `/Mask` als Stencil-Strom,
  `/Mask` als Farbschlüssel) dreht das Kernversprechen um — gerade eine Stelle,
  die die Eingabe unsichtbar macht, ist das Muster einer bereits mit einem
  anderen Werkzeug geschwärzten Stelle. Ein Stencil-Strom wird jetzt unverändert
  mitgeschrieben, ein Farbschlüssel in den Alphakanal gerechnet; was sich nicht
  sicher übertragen lässt, beendet den Lauf, statt still zu vereinfachen.
  Zwei Bilder, die sich gegenseitig als `/SMask` nennen, führten in eine
  endlose Rekursion.
* **Die Hilfsdateien haben eine feste Größengrenze.** Buchungsliste
  (`--booking-list`), Review-Datei (`--apply-review`) und Regionsliste
  (`--manual-regions`) **16 MB**, Musterkonfiguration (`--patterns-config`)
  **1 MB**; beide fest, ohne Schalter. Vorher gingen sie durch
  `std::fs::read`/`read_to_string`, und das legt einen Puffer in **Dateigröße**
  an, bevor irgendetwas geprüft ist: wie viel Arbeitsspeicher ein Lauf belegt,
  stand damit in der Datei. Nachgemessen an einer dünn belegten 6-GB-Datei
  hinter `--manual-regions`: vorher Exit 1 nach **24,0 s** bei **6 150 MB**,
  jetzt Exit 1 nach **0,00 s** bei **6,3 MB**.

  Das ist **keine** Vertrauensfrage — diese Dateien bringt der Bedienende
  selbst mit. Der wahrscheinlichste Weg dorthin ist kein Angriff, sondern ein
  vertippter Pfad: `--manual-regions` auf den 700-MB-Scan statt auf die
  JSON-Datei. Eine benannte Pipe oder ein Gerät wird abgelehnt, bevor die Größe
  überhaupt zur Sprache kommt (Länge 0, liefert endlos). Die Meldungen nennen
  Nennlänge, Grenze und die wahrscheinlichere Ursache.

### Geändert

* **Ein Release entsteht nur noch aus einem Stand, dessen CI grün ist.** Der
  Freigabeweg hat den Code vorher nie geprüft: `verify` sah Versionsnummer und
  Tag an, gebaut wurde mit einem einzigen `cargo build --release` — kein Test,
  kein Clippy, kein fmt, kein `cargo deny`. So ist v0.3.0 entstanden: der Lauf
  „Publish Release" (`30740996889`, Commit `9aa4808`) endete erfolgreich,
  während die CI-Läufe desselben Commits (`30740996865`, `30741004317`) beide
  fehlschlugen. `release.yml` ruft jetzt `ci.yml` als wiederverwendbaren
  Workflow auf und macht ihn zur Vorbedingung für Bauen und Veröffentlichen.
* **Ein Push auf einen Arbeitsbranch veröffentlicht nichts mehr** (`04da92c`).
  `publish-release.yml` hörte auf `main`, `master` **und** `claude/**`; v0.3.0
  wurde deshalb für denselben Commit zweimal ausgelöst. Der Lauf des
  Arbeitsbranches war 14 Sekunden schneller, legte den Release als
  *Vorabversion* an — so steht v0.3.0 bis heute auf GitHub — und der Lauf von
  `main` scheiterte danach am bereits vorhandenen Tag.
* **Veröffentlicht wird nur, was auf dem Default-Branch angekommen ist.** Für
  den Datei-Weg (`.release-version`) galt das schon; ein gepushter Tag
  (`git tag v0.3.1 <arbeitsstand>`) und `workflow_dispatch` mit beliebigem
  `ref` kannten dagegen gar keinen Branch. `verify` prüft die Abstammung jetzt
  für alle Wege.
* **Der geprüfte Commit ist der gebaute Commit.** `verify` löst den Ref genau
  einmal zu einer Commit-ID auf; CI-Tor, Bauauftrag und Veröffentlichung
  bekommen diese ID. Vorher hat jeder Job den Ref selbst aufgelöst — bei einem
  Branch-Ref konnten das zwei verschiedene Commits sein.
* **Vorabversionen erkennt der Release an der Versionsnummer.** `v0.4.0-rc.1`
  wird als Vorabversion gekennzeichnet, auch auf dem Tag-Weg, auf dem der
  bisherige Schalter leer blieb.
* Der Pfad des Build-Rechners steht nicht mehr in dem Teil der ausgelieferten
  Binaries, den `--remap-path-prefix` erfasst — das ist der übersetzte
  Quelltext aller Crates. **Nicht erfasst sind die Ausgaben der Build-Skripte
  (`OUT_DIR`)**, und dort steht er weiterhin.

  Nachgemessen an den ausgelieferten v0.4.0-Artefakten
  (`strings -a -n 8 <binary> | grep -c /home/runner`):

  | Artefakt | Fundstellen |
  |---|---|
  | Linux/glibc `redact-rs` | **3** |
  | Linux/musl `redact-rs` | 0 |
  | `redact-rs.exe` | 0 |
  | `redact-rs-gui.exe` | 0 |

  Alle drei liegen unter
  `/home/runner/work/redactrs/redactrs/target/x86_64-unknown-linux-gnu/release/build/`
  und heißen `glutin_glx_sys-…/out/glx_extra_bindings.rs`,
  `glutin_glx_sys-…/out/glx_bindings.rs` und
  `glutin_egl_sys-…/out/egl_bindings.rs` — Dateien, die die Build-Skripte von
  `glutin_glx_sys` bzw. `glutin_egl_sys` (über `gl_generator`) zur Bauzeit in
  `OUT_DIR` erzeugen. `--remap-path-prefix` wirkt auf die Pfade, mit denen
  *Cargo* `rustc` aufruft, nicht auf einen absoluten Pfad, den ein Build-Skript
  selbst in `OUT_DIR` hineinschreibt. musl und beide Windows-Artefakte sind
  sauber, weil sie ohne Oberfläche (`--no-default-features`) bzw. ohne die
  X11-/EGL-Anbindung gebaut werden.

  **Was das heißt und was nicht:** preisgegeben ist der Pfad eines
  GitHub-Runners (`/home/runner/work/redactrs/redactrs`) — kein Geheimnis,
  aber auch nicht das, was der Eintrag zugesagt hat. Wer aus einem privaten
  Arbeitsbaum selbst baut, trägt seinen eigenen Pfad in dieses Artefakt.

  **Hier stand: „steht nicht mehr in den ausgelieferten Binaries“** — das war
  für das glibc-Artefakt falsch. Korrigiert am 2026-08-05, nachdem es an den
  Artefakten nachgemessen wurde; die Behebung selbst gehört in den Bauweg
  (siehe „Unveröffentlicht“).

  Was die Prüfsummen in `SHA256SUMS-BINARIES` belegen und was nicht, sagen die
  Release-Notizen jetzt ausdrücklich; die frühere Formulierung („Wer nachbauen
  will, vergleicht diesen Hash") war eine Zusage ohne Deckung.
* Es gibt diesen Änderungsverlauf. Bis einschließlich v0.3.0 stand
  ausschließlich in `git log`, was sich zwischen zwei Fassungen geändert hat.

### Behoben

* Zwei Stellen im Freigabeweg, die nie erreicht wurden: `grep -v` auf
  `.release-version` endet mit 1, wenn nur Kommentare übrig bleiben — unter
  `set -euo pipefail` starb der Schritt an dieser Stelle wortlos, statt die
  vorgesehene Meldung auszugeben. Und der Zweig, der einen Release außerhalb
  des Default-Branches als Vorabversion gekennzeichnet hätte, konnte seit
  `04da92c` nicht mehr greifen; er ist entfallen.
* `SECURITY.md`: die Tabelle zu `unsafe` in Abhängigkeiten nennt jetzt auch
  die Crates, die im reinen CLI-Bau Angreiferdaten sehen (`encoding_rs`,
  `aes`, `aho-corasick`, `memchr`, `simd-adler32`), und sagt, dass ein Bau
  ohne Oberfläche die Fläche verkleinert, aber nicht beseitigt. Alle Zahlen
  neu gemessen.
* `SECURITY.md`: die Stapel-Zusammenfassung in einem Beispiel stammte aus
  einer Fassung mit zwei Zahlen; das Programm gibt seit v0.3.0 drei aus.
* `README.md`, fünf Angaben, die als **Messung** dastanden und keine mehr
  waren — jede ist ersetzt durch eine, die für diesen Baum nachgefahren wurde:
  * dieselbe Stapel-Zusammenfassung mit zwei Zahlen wie in `SECURITY.md`; dazu
    fehlten in den Beispielen die Fortschrittszeilen auf stderr;
  * die Binärgrößen („Windows 7,4 MB / 7 395 328 Byte“, „Linux 13 MB“). An den
    **ausgelieferten** v0.3.0-Artefakten nachgemessen — gegen
    `SHA256SUMS-BINARIES` geprüft — sind es 10,6 MB (`redact-rs.exe`), 9,8 MB
    (`redact-rs-gui.exe`), 14,9 MB (Linux/glibc) und 4,9 MB (Linux/musl). Das
    Akzeptanzkriterium (< 30 MB) hält weiter, mit Abstand;
  * „1350 Schwärzungen in 77 ms“ für das 10-Seiten-Kriterium. 1350 Treffer
    stimmen; die Zeit für den ganzen Prozess ist **0,16 s** (bestes von fünf
    Läufen, Release);
  * die Zahl der Fälle in `cli_and_gui_agree.rs` stand an einer Stelle auf
    fünf, an der anderen auf sieben. Es sind sieben (`ok. 7 passed`);
  * „82×42 Bildpunkte“ für die gepolsterte Bildregion — die daneben genannten
    3696 geänderten Bildpunkte sind 84×44. Der Rasterrand wird nach **außen**
    gerundet, und das ist die sichere Richtung.
* `README.md`: „die Bildpunkte außerhalb der Schwärzung sind nachweislich
  unverändert, auch beim JPEG-Fall“ war zu stark. Für ein Flate-Bild stimmt es
  exakt (nachgemessen: null geänderte Bildpunkte außerhalb). Bei einem JPEG gilt
  es nur gegenüber dem **eigenen** Dekodat: gegen Pillow gemessen weichen 8231
  der 16 304 äußeren Bildpunkte ab, fast alle um höchstens 2 je Kanal. Das ist
  Dekoder-Rauschen und kein zurückgetragener Inhalt — aber wer es wörtlich
  nachprüfen will, braucht denselben Dekoder.
* `README.md`, Release-Rezept: es empfahl `printf '0.2.0\n' > .release-version`
  und nannte einen Tag als gleichwertige Alternative. Das erste warf den
  Kommentarkopf der Datei weg, der sagt, dass eine Änderung an ihr
  veröffentlicht; das zweite trifft nicht mehr zu, seit beide Wege durch
  dieselben zwei Vorbedingungen gehen (Default-Branch, grüne CI).
* `README.md`: der Änderungsverlauf war nirgends verlinkt, das musl-Archiv
  nirgends erwähnt, und die Liste des Archivinhalts kannte `CHANGELOG.md`
  noch nicht.

### ⚠ Bedienung — Wege, auf denen eine IBAN stehenbleiben konnte

* **Mit den Pfeiltasten ließ sich ein Treffer neben das Blatt schieben, und
  die Kopfzeile versprach ihn weiter.** `move_selected` war die einzige
  Rechteckänderung, die weder die Klemmung noch den gemeinsamen Schreibweg
  durchlief. Nach etwa sechzig Anschlägen lag das Rechteck komplett außerhalb
  der Seite — unsichtbar, weil der Maler dort abschneidet —, während die
  Kopfzeile unverändert „2 werden geschwärzt“ sagte und der Export ohne Fehler
  durchlief. Die Warnung kam erst danach. Über den Eckgriff war derselbe Weg
  längst geklemmt.

  Geklemmt wird jetzt **versetzend**, nicht schneidend: reines Beschneiden
  hätte den Balken am Blattrand bei jedem weiteren Anschlag schrumpfen lassen —
  genau die Richtung, die eine IBAN wieder hervorkommen lässt.

* **Ein mit den Pfeiltasten korrigierter Treffer galt nicht als Handarbeit.**
  „Analysieren“ oder eine Buchungsliste warf ihn deshalb **ohne Rückfrage**
  weg, während die vier anderen Wege zum selben Verlust fragen.

* **Regionen aus einer Review-Datei, die neben der Seite liegen, zählten als
  „wird geschwärzt“.** Sie tragen jetzt ein eigenes Wort in Liste und
  Kopfzeile und gehen weder in die Konfliktauflösung noch in den Export. Die
  Aussage steht damit *vor* der Arbeit statt als Warnung danach. Geklemmt wird
  hier bewusst **nicht**: eine Review-Datei ist die Angabe der Nutzerin.

* **Die Zahl an der Miniaturansicht zählte Zeilen statt Schwärzungen.** Eine
  Seite, auf der ein Schutzeintrag alles blockiert und drei Handrechtecke
  abgewählt sind, zeigte 5, obwohl dort nichts geschwärzt wird — und die
  Miniaturspalte ist der Ort, an dem man am Ende prüft, ob nichts stehen
  geblieben ist.

* Ein Seitenwechsel mitten im Zug am Eckgriff veränderte blind die Region auf
  der alten Seite, gerechnet mit der Geometrie der neuen. Bei zwei
  deckungsgleichen Zeilen gewann außerdem die Schwärzungsart der *abgewählten*
  — *ob* geschwärzt wurde stimmte, *wie* nicht. Und fünfzig Tastenanschläge
  leerten den gesamten Rückgängig-Stapel.

### Leistung

Vier Stellen wuchsen schneller als die Eingabe. Alle vier sind an
Messreihen belegt, und für jede ist nachgewiesen, dass sich am **Ergebnis**
nichts ändert — bei einem Werkzeug, das entscheidet, welche Schwärzung
wegfällt, wiegt das schwerer als die Geschwindigkeit.

* **Die Konfliktauflösung war weiterhin quadratisch — bei genau der Anordnung,
  die ein Kontoauszug erzeugt.** Der Streifenzug lief über die x-Achse;
  Treffer mit derselben x-Spanne, die sich nur in y unterscheiden — eine IBAN
  auf jeder Zeile, also eine Spalte — fielen nie aus dem Streifen. 100 000
  Kandidaten kosteten **74,6 s**, jetzt 0,21 s. Der vorhandene Test wählte
  ausgerechnet die Anordnung, die nicht wehtut; er prüft jetzt beide.

  Die Suche stützt sich auf ein Gitter und darf sich nur deshalb auf die Zelle
  des Mittelpunkts beschränken, weil beide Schwellen bei mindestens 50 %
  liegen. Diese Kopplung ist als **Compile-Fehler** verankert, nicht als
  Kommentar: wer eine Schwelle senkt, macht die Suche lautlos unvollständig.

* **Ein oft platziertes Form-XObject kostete 4,4 ms Rechenzeit je zusätzlichem
  Dateibyte.** Strom und Schriftverzeichnis wurden bei *jeder* Platzierung neu
  ausgepackt; eine Platzierung sind sechs Byte in der Datei. 1 600
  Platzierungen in einer 231-kB-Datei brauchten **126,5 s** — ohne Abbruch,
  ohne Warnung, Rückgabewert 0, obwohl die README ausdrücklich Schutz gegen
  diese Vervielfachung verspricht. Jetzt 0,65 s. Dieselbe Vervielfachung lag
  bei weichen Masken, Kachelmustern und Erscheinungsströmen.

* **Die Schwärzung wuchs quadratisch im Seiteninhalt.** Für jedes Rechteck
  wurde eine Markierung über *alle* Zeichen der Textoperation angelegt, und
  beide Größen wachsen mit dem Inhalt, weil Treffer aus Text entstehen. 25 600
  Kandidaten auf einer Seite: **23,3 s**, jetzt 2,9 s und genau linear.

* **Die Miniaturansichten kosteten je Einzelbild O(Seiten × Regionen).** Bei
  1 600 Seiten und 96 000 Regionen 743 ms je Bild, jetzt 0,8 ms.

### Prüfgrundlage

* **Zwei Schwellen, die entscheiden, ob eine Schwärzung verworfen wird, hielt
  kein Test.** Beide ließen sich auf einen falschen Wert setzen, ohne dass
  einer von 640 Tests anschlug.
* **Ein Test schlug auf grünem Code in einem von fünf Läufen fehl** — und
  verdeckte dabei die halbe Suite, weil `cargo test` nach dem ersten roten
  Testbinary abbricht.
* Die Prüfung der Aufwandsschranken zählt jetzt ausgepackte Ströme und
  geladene Schriftverzeichnisse statt Sekunden — deterministisch statt
  lastabhängig.

### Aufgeräumt

Eine Zusicherung, die nichts zusicherte; eine Hülle um ein einzelnes Feld;
ein `.max()`, das zwei Wahrheiten versöhnte, die nachweislich nie
auseinandergehen; zwei Funktionen ohne Aufrufer; und zwei öffentlich
einstellbare Felder, die keiner der 35 Aufrufer je gesetzt hat. Fünf Kopien
der Frage „ist das ein Bild?“ sind zu einer geworden — die abweichende von
ihnen löste eine indirekte Referenz auf und war unerreichbar; jetzt löst die
eine Fassung sie überall auf, was einen echten Fall von „Deckungslücke
gemeldet“ zu „Bild geschwärzt“ bewegt.

---

## 0.3.0 — 2026-08-02

Vier unabhängige Prüfrunden nach 0.2.0. Der überwiegende Teil der Befunde war
nicht „fehlt noch", sondern „meldet Erfolg und tut es nicht".
Bereich: `git log v0.2.0..v0.3.0`.

### ⚠ Sicherheit

* **`/ActualText` überlebte die Schwärzung.** Ein Kontoauszug aus Word oder
  InDesign trug die geschwärzte IBAN weiterhin im Klartext: die Glyphen waren
  korrekt aus dem `TJ` entfernt, das Deck-Rechteck saß richtig — und
  `pdftotext` gab die IBAN in der Voreinstellung vollständig zurück. Das
  Programm meldete „2 Schwärzungen, 40 Zeichen entfernt", Exit 0, keine
  Warnung. `/Span <</ActualText …>> BDC`, `/Alt` und `/E` schreibt jeder
  PDF/UA-Erzeuger routinemäßig. Betroffen waren alle sechs gemessenen
  Varianten. **Wer v0.2.0 mit getaggten PDFs eingesetzt hat, sollte die
  Ergebnisse nachprüfen.**
* **Verschlüsselte Eingaben umgingen die Ressourcengrenzen vollständig.** Nach
  dem Entschlüsseln lief nur noch `validate`, keine Prüfung der jetzt
  entschlüsselten Streams. Am Release-Binary gemessen: eine verschlüsselte
  Dekompressionsbombe von 33 kB kostete 342 s und 15 446 MB und endete im
  globalen OOM-Killer; eine verschlüsselte Verschachtelungsbombe von 1,3 kB
  endete mit Exit 0, geschriebener Ausgabedatei und vier nachweisbaren Lecks.
  Die Grenzen greifen jetzt hinter der Entschlüsselung.
* **Zwei weitere Wege in den OOM-Killer sind zu.** Ein `/Image`-Teilstring im
  Stream-Dictionary hebelte beide Vorprüfungen aus (130 999 Byte → SIGABRT bei
  3 961 MB; 1 122 Byte → Exit 0 mit Geheimnis in der Ausgabe), und eine
  Fächerung über Form-XObjects ebenso (2 274 Byte → SIGABRT bei 4 044 MB).
  Über Budget und Tiefenprüfung entscheidet jetzt der ausgepackte Inhalt statt
  des vom Angreifer beschreibbaren Dictionaries, und ein Aufwandskonto zählt
  Vervielfachung statt Bytes.
* **`lopdf` 0.34 → 0.42: RUSTSEC-2026-0187 ist geschlossen** (Stack Overflow
  beim Parsen tief verschachtelter Objektstrukturen — genau das, was dieses
  Werkzeug tut). Die befristete Ausnahme in `deny.toml` ist ersatzlos
  entfallen. `tests/nesting_bomb.rs` belegt, dass die Bibliothek selbst
  repariert ist und nicht nur die eigene Vorprüfung greift.
* **`--manual-regions` umging die Identitätsprüfung von `--apply-review`.**
* **Das Audit-Log bescheinigte Schwärzungen, die nicht stattgefunden hatten.**
  Eine Regionsdatei mit den Seitenzahlen 1, 2, 3 für ein dreiseitiges Dokument
  — der naheliegende Fehler eines Menschen, der ab 1 zählt — ergab Exit 0,
  keine Warnung, drei Einträge „applied" und die IBAN der ersten Seite
  unverändert in der Ausgabe.
* **Eine Seite, deren Content-Stream sich in keine Operation zerlegen ließ,**
  endete mit Exit 0 und „0 Schwärzungen", während das Geheimnis unverändert in
  der Ausgabe stand. Jetzt ist das ein Fehler.
* **In der Oberfläche verschob `Entf` während eines Zuges eine unbeteiligte
  Region auf das Ziehrechteck** — deren Fläche wurde danach nicht mehr
  geschwärzt. Der Zug merkte sich einen Index in die Regionsliste statt einer
  Kennung.

### Geändert

* **Neuer Rückgabewert 3 für weiche Warnungen.** Vier Fälle heißen wörtlich
  „wurde nicht durchsucht und kann deshalb nicht geschwärzt worden sein" und
  endeten trotzdem mit 0; im Stapel zählten solche Dateien als verarbeitet.
  Die 2 bleibt der Bedienfehler („mach es anders"), die 3 richtet sich an die
  Ergebnisprüfung („schau selbst nach"). **Für Skripte ist das die wichtigste
  Änderung dieser Fassung.**
* Die Stapel-Zusammenfassung nennt drei Zahlen statt zwei: vollständig
  geprüft, verarbeitet (aber nicht vollständig geprüft), fehlgeschlagen. Keine
  Datei zählt in zweien mit.
* Das Audit-Log unterscheidet je Region vier Befunde statt alles „applied" zu
  nennen.
* Audit-Log und Review-Datei melden Seitenzahlen einheitlich; die frühere
  Warnung, man dürfe eine Seitenzahl nicht von der einen in die andere Datei
  übernehmen, ist gegenstandslos.
* Zwei stille Verhaltensänderungen in `lopdf` 0.42 abgefangen: `/Prev`
  verschwindet beim Laden aus dem Trailer (die Warnung zu inkrementellen
  Revisionen wäre spurlos entfallen), und die Bibliothek begrenzt sich selbst
  auf Verschachtelungstiefe 100 und lässt zu tiefe Objekte kommentarlos weg —
  für die Tiefen 101 bis 128 wäre ein neues Fenster für stillen Objektverlust
  entstanden.

### Behoben

* Ein Inline-Bild mit ` EI ` in den Nutzdaten löschte den Rest der Seite.
  `lopdf::Content::decode` liefert dabei kein `Err`, sondern bricht am ersten
  unverständlichen Byte ab und gibt den Rest lautlos auf. Die Bildgrenze wird
  jetzt geprüft statt geglaubt.
* `konto_nr` backtrackte quadratisch: 1 600 Buchstaben genügten, um das
  Regex-Budget zu reißen (8 000 Zeichen: 1 473,8 ms → 10,3 ms).
* Die Oberfläche hielt die Ressourcengrenzen nicht: `load_document` las mit
  `std::fs::read`, also ohne Größengrenze — eine Sparse-Datei mit scheinbar
  6 GB kostete 6 149 MB, bevor überhaupt feststand, ob es ein PDF ist.
* Die Oberfläche prüfte eine Review-Datei pauschal statt gegen das geladene
  Dokument und lehnte damit auch passende Dateien ab.
* Das Rechteck beginnt am Druckpunkt und ist ab dem Druck sichtbar, nicht erst
  nach 6 pt Mausbewegung; die Eckgriffe sind keine Attrappen mehr.
* `--action blackout --replace-with X` verlor X, sobald in der Oberfläche auf
  „Ersetzen" umgestellt wurde. Der Ersatztext hat jetzt eine Quelle statt
  zweier.
* „Analysieren" fragt, bevor es Auswahlentscheidungen verwirft; ein Druck auf
  Tabulator legt nicht mehr sämtliche Tastenkürzel lahm.
* `--action`, `--apply-review`, `--audit-log`, `--review-out` und die
  Ressourcengrenzen gelten auch im Fenster.
* Ein Release war zeitweise unmöglich: `verify` konnte `.release-version`
  nicht mehr lesen, seit die Datei einen Kommentarkopf trägt.

---

## 0.2.0 — 2026-08-01

Der Kern hat sich an mehreren Stellen von „meldet Erfolg" zu „tut es wirklich"
bewegt. Bereich: `git log v0.1.0..v0.2.0`.

### ⚠ Sicherheit

* **Gescannte Dokumente wurden nicht wirklich geschwärzt.** Das schwarze
  Rechteck lag nur obenauf; die Bildpunkte mit der IBAN blieben in der Datei.
  Pixel unter einer Schwärzung werden jetzt überschrieben — auch in
  Form-XObjects und bei gedrehten oder skalierten Platzierungen.
* **Gesperrt gesetzte IBANs wurden nicht erkannt.** Ab `Tc` > 1,4 pt steht
  zwischen jedem Zeichen ein Leerzeichen — genau so setzen Banken Kontofelder.
* **Ein Subset-Font ohne `/ToUnicode` lieferte lesbar aussehenden Unsinn ohne
  jede Warnung**; die Kommandozeile endete mit 0, und der Nutzer hielt die
  Datei für sauber. Die Warnschwelle für unlesbare Glyphen zählte außerdem den
  Anteil am Font statt das Unlesbare selbst: 29 % eines 1000-Zeichen-Fonts
  sind 290 unlesbare Zeichen — und es blieb still.
* **Jede Review-Datei aus der Oberfläche war auf jedes beliebige PDF
  anwendbar** (leeres `sha256`-Feld wurde stillschweigend durchgewunken) —
  Rechtecke an den falschen Stellen, ohne Hinweis.
* **Das Audit-Log bescheinigte Schwärzungen, die nicht stattgefunden hatten.**
* **Der Standardlauf schwärzte zu 75 % Fehltreffer** (jetzt 0 %, gemessen an
  echten Auszugszeilen).
* **Eine 92-kB-Datei brachte den Speicher auf 5,5 GB, eine 183-kB-Datei brach
  mit SIGABRT ab** — obwohl `SECURITY.md` ausdrücklich „kontrollierter Abbruch
  statt Speicherfehler" zusagte. Die Bildschwärzung dekodierte jedes Bild der
  Seite statt nur der betroffenen, klonte den Puffer und hielt die Arbeitsliste
  über alle Seiten. 5 508 MB → 187 MB. Betroffen war nicht nur ein Angreifer:
  ein 40-seitiger A4-Scan hielt 34 MB je Seite bis zum Schluss fest.
* **Ein `/OCG`, das eine Seite über `/Resources /Properties` referenziert,
  überlebte das Entfernen von `/OCProperties` samt seinem `/Name`** — heißt
  eine Ebene „Ebene Mustermann", stand der Name hinterher noch in der Datei.
* **Eine Ligatur konnte lesbar bleiben:** deckte der Bereich genau das zweite
  Teilzeichen ab, schrieb `rebuild_show` den Originalcode wieder heraus.

### Geändert

* Die zugesagte MSRV steht auf dem gemessenen Wert **1.88** statt auf der nie
  geprüften 1.75. Der MSRV-Job ist keine Notiz mehr, sondern eine Zusicherung.
* CLI und Oberfläche teilen sich die Verarbeitungskette wirklich
  (`crates/redact-pipeline`), statt es nur im Kommentar zu behaupten. Damit
  wirkt `--padding` auch in der Oberfläche, der Export überschreibt die
  Eingabedatei nicht mehr stillschweigend, und Audit-Log wie Review-Datei
  entstehen mit 0600 statt 0644, mit Symlink- und dev/ino-Prüfung und atomarem
  Umbenennen.
* Neue Obergrenze `--max-image-mb` (Vorgabe 256 MB), geprüft **vor** dem
  Auspacken. `--max-decompressed-mb` half hier nie — es verbucht die Rohbytes
  des Streams, nicht die dekodierten Bildpunkte.
* Die Meldung „(neu kodiert, verlustbehaftet)" ist weg: außerhalb der
  Schwärzung ändert sich kein Bildpunkt, auch bei JPEG-Quelle.
* Die Buchungsliste verspricht nicht mehr, über Zeilenumbrüche zu treffen.
* Das Windows-Archiv enthält zusätzlich `redact-rs-gui.exe` ohne
  Konsolenfenster.
* Release-Notizen versprechen eine Herkunftsaussage nur dort, wo GitHub sie
  auch erzeugt: für private Repositories unter einem Benutzerkonto gibt es
  keine, und das steht jetzt so darin.

### Neu

* Muster für die SEPA-Gläubiger-Identifikationsnummer (`DE98ZZZ09999999999`),
  standardmäßig aktiv. Sie steht auf jeder Lastschriftzeile und ist bei
  Einzelunternehmern unmittelbar personenbezogen.
* Echte Seitendarstellung in der Oberfläche.

### Behoben

* `replace_page_content` war quadratisch in der Seitenzahl
  (8 000 Seiten: 35,01 s → 0,83 s).
* `dict_int` dereferenzierte nicht: ein Bild mit indirektem `/Width` ließ die
  Schwärzung abbrechen. Sieben weitere Stellen ohne Dereferenzierung
  mitbehoben.
* Die Fehlermeldung bei zu großen Bildern nannte den Filter als Ursache, statt
  die gerissene Pixelgrenze.

### Geprüft und widerlegt

Zwei gemeldete Befunde waren keine — die Gegenproben bleiben als Tests stehen:
manuelle Regionen entfernen **nicht** die falschen Zeichen (geprüft mit engem
Zeilenabstand, 90° gedreht, im Form-XObject, über mehrere `Tj`, mit
`TJ`-Kerning, `Tw`, `Tz 50` und Lücken in `/Widths`), und gedrehter Text wird
sehr wohl extrahiert (bei 0/90/180/270° entstehen aus zwei gesetzten Zeilen
genau zwei).

---

## 0.1.0 — 2026-07-31

Erste Fassung. Bereich: `git log v0.1.0`.

### Neu

* `redact-core`: Domänenmodell (Region, Redaction, BookingEntry),
  zeichengenaue Text-Runs, Konfliktauflösung mit Vorrang der Negativliste,
  Review-JSON-Format.
* `redact-pdf`: eigener Content-Stream-Interpreter, der für jedes Zeichen eine
  Bounding-Box im User-Space berechnet (inklusive Form-XObjects);
  Font-Metriken aus `/Widths`, CID-`/W`, Standard-14-Fallback, `/ToUnicode`
  und `/Differences`. Geschwärzt wird durch **Entfernen** aus dem Content-
  Stream, der entfallende Vorschub wird als `TJ`-Kerning ersetzt, darüber das
  Deck-Rechteck. Metadaten werden gestrippt, Annotationen im
  Schwärzungsbereich gelöscht, verschlüsselte oder kaputte PDFs abgelehnt.
* `redact-patterns`: elf eingebaute Muster mit IBAN-, BIC- und Luhn-Prüfung,
  konfigurierbar über YAML/JSON.
* `redact-booking`: CSV-Buchungslisten mit Vorrang der Negativliste.
* `redact-cli`: Kommandozeile mit Review-Modus, `--apply-review`, Audit-Log
  und `--json`. Review-Dateien tragen den SHA-256 der Eingabe; ein Anwenden
  auf ein anderes Dokument wird abgelehnt. Das Audit-Log führt die SHA-256 von
  Ein- und Ausgabe sowie die blockierten Treffer.
* `redact-gui`: egui-Oberfläche mit Seitenvorschau, Rechtecken per Maus,
  abwählbaren Treffern, gesperrten Negativlisten-Treffern und Drag & Drop.
  Der Export benutzt dieselbe Pipeline wie die Kommandozeile.
* Ohne `-o` entsteht das Ergebnis neben der Eingabe, mit Namenszusatz
  (`kontoauszug.pdf` → `kontoauszug_geschwaerzt.pdf`); `--force` überschreibt,
  ohne `--force` bricht der Lauf ab.
* Build- und Release-Infrastruktur: CI mit fmt, Clippy (`-D warnings`) und
  Tests unter Linux und Windows; Release-Workflow für
  `x86_64-pc-windows-msvc` und `x86_64-unknown-linux-gnu` mit `SHA256SUMS`.
  Ein Push von `.release-version` löst die Veröffentlichung aus — in
  abgeschotteten Umgebungen ist das der einzige Weg, der immer funktioniert.
