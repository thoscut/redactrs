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

Bereich: `git log v0.6.0..HEAD`.

Neun Fix-Runden seit 0.6.0, jede eine Gegenprüfung der vorigen — an Code
und Doku mit demselben Maßstab. Zuletzt (Runde 9) fällt ein stilles Leck im
Bild, eine Grenze, die eine gewöhnliche Datei ablehnte, und eine Rückfrage, die
beim falschen der beiden Fäden stand — denn **jede** Zahl der beiden jüngsten
Abschnitte dieser Datei ist an einen Lauf, eine Konstante oder eine
aufgezeichnete Messung gebunden, nicht nur die, an die jemand dachte, und seit
dieser Runde auch jede Zusage, die in Fettschrift steht.

Der Maßstab für eine Messzahl, ausgeschrieben — die pauschale Zusage, die hier
stand („jede Angabe hier stammt aus einem Lauf des gebauten Binaries“), war
keine: alle drei neuen Messzahlen der Runde 7 stammten aus einem Testprozess,
und kein Lauf des Binaries stand dahinter. Statt ihrer gilt:

* Eine Messzahl stammt aus einem Lauf des **gebauten Binaries**
  (`target/release/redact-rs`); Zeit und Spitzenspeicher kommen von
  `/usr/bin/time -v`, MB heißt 1024 Byte zum Quadrat.
* Zeigt die Kommandozeile die Größe nicht, weil sie im Innern eines Bausteins
  liegt oder der Oberfläche gehört, dann sagt **der Satz selbst**, dass im
  Testprozess gemessen wurde, und nennt das Profil. Ein zweiter Weg, kein
  Schlupfloch: still eine Debug-Zahl als Binary-Zahl auszugeben ist keiner.
* Eine Zahl, die den Zustand **vor** einer Korrektur beziffert, ist danach
  nicht mehr messbar. Sie sagt deshalb, dass sie den alten Zustand
  beschreibt, und nennt den Stand, an dem er liegt — nachvollziehbar durch
  Auschecken, auch wenn nicht wiederholbar.
* Was sich ableiten lässt, wird abgeleitet und nicht abgeschrieben.

Wo eine Zahl keinen dieser Wege geht, gehört sie gestrichen und nicht
verschoben. `crates/redact-cli/tests/belege.rs` hält die Regel samt Läufen;
`die_zahlen_der_doku_sind_gebunden` und
`jede_zahl_der_letzten_runden_ist_gebunden` prüfen sie.

### Spur-A-Runde 2: die Varianten der eigenen Korrekturen

Die zweite Runde unter demselben Mandat, auf dem Stand `2edf407`, wieder mit
einem Prüfer je Gebiet. Die Punkte stehen hier, sobald sie behoben
sind, jeder in seinem eigenen Commit.

* **⚠ Sicherheit: die Bilddecke galt den Maßen des Dictionaries, der
  JPEG-Dekoder belegte die Maße aus dem JPEG** (Register #101, Prüfer A;
  Dienstverweigerung). `--max-image-mb` und die harte Pixeldecke prüfen
  `/Width` × `/Height`; `decode_jpeg` las die Maße erst beim Dekodieren aus
  dem SOF-Kopf und belegte sie ohne Frage. Am Stand `2edf407` belegte ein
  Bild mit 100 × 100 im Dictionary über einem JPEG mit 6000 × 6000 am
  gebauten Binary 262 MB Spitze (`/usr/bin/time -v`), bei `--max-image-mb 8`,
  mit Rückgabewert 0 — ein größeres JPEG entsprechend mehr. Jetzt liest der
  Dekoder zuerst den Kopf; ein JPEG mit mehr Bildpunkten, als sein
  Dictionary angibt, gilt als nicht dekodierbar und nennt den Grund. Eines,
  das kleiner ist, bleibt erlaubt. Belege: `zp_a_jpeg_groesser_als_angegeben`
  (der große Fall und die Gegenproben). Mutationsnachweis: Kopf ungeprüft →
  der große Fall rot.

* **⚠ Sicherheit: ein Eintrag `null` im eigenen Verzeichnis hielt die Suche
  nach dem Spiegel an** (Register #86, Prüfer C; Variante der Korrektur #67).
  Nennt das eigene `/Properties` eines Formulars den Namen mit dem Wert
  `null` — direkt oder als Verweis ins Leere —, ist der Eintrag nach der Norm
  wie ein fehlender; Poppler und MuPDF suchen in der Seite weiter und geben
  deren Spiegel aus. Hier galt er als vorhanden, die Suche brach ab, und der
  Spiegel mit dem Geheimnis blieb in der Seite stehen, ohne Warnung. Jetzt
  zählt ein solcher Eintrag als fehlend. Beleg:
  `zo_c_spiegel_umgebungen::c3_null_eintrag_im_eigenen_verzeichnis_gilt_als_fehlend`.
  Mutationsnachweis: `null` wieder als vorhanden → rot.

* **⚠ Sicherheit: unter der Erscheinung einer Annotation lag keine Seite**
  (Register #85, Prüfer C; Variante der Korrektur #67). Bringt ein
  Erscheinungsstrom — ein `/AP`, ein Druckknopf-Symbol unter `/MK` — eigene
  Ressourcen ohne `/Properties` mit, löst Poppler `/MC0` in den Ressourcen der
  Seite auf und gibt deren Spiegel als Text der Glyphen aus. Der Scan der
  Erscheinung reichte keine äußere Umgebung weiter; der Spiegel blieb mit
  dem Geheimnis in der Seite stehen, ohne Warnung. Jetzt liegt die Seite als
  äußere Umgebung unter jeder Erscheinung mit eigenen Ressourcen. Beleg:
  `zo_c_spiegel_umgebungen::c3_erscheinung_mit_eigenen_ressourcen_ohne_properties_name_aus_der_seite`.
  Mutationsnachweis: keine Umgebung unter der Erscheinung → rot.

* **⚠ Sicherheit: eine Schrift, ein Formular, ein Grafikzustand oder ein
  Muster, deren Name nur beim Aufrufer stand, galt als nicht gezeichnet**
  (Register #88, Prüfer C; neue Klasse). Ein Formular mit eigenem
  `/Resources` benutzt einen Namen aus `/Font`, `/XObject`, `/ExtGState` oder
  `/Pattern`, der nur in den Ressourcen der Seite steht. Die Norm sieht das
  nicht vor; Poppler löst den Namen trotzdem in der Kette der Aufrufer auf
  und zeichnet. Der Scan sah nichts: keinen Text, keine Schwärzung, und auch
  `--check-leaks` fand das Geheimnis weder im Eingang noch in der Ausgabe.
  Jetzt liest der Scan einen solchen Strom unter einer zusammengesetzten
  Sicht — die Kategorien der Aufrufer, das eigene Verzeichnis darüber; ein
  eigener Eintrag geht vor, wie in Poppler. `/Properties` folgt weiter der
  Regel aus #67. Belege: `zp_c_ressourcen_beim_aufrufer` (je Kategorie ein
  Fall, das Orakel am Eingang, und die Grenze „eigener Eintrag geht vor“).
  Mutationsnachweis: keine zusammengesetzte Sicht → jeder Fall rot; der
  Aufrufer geht vor → die Grenze rot.

* **⚠ Sicherheit: die Vorprüfung las weniger als die Leser nach ihr**
  (Register #89, Prüfer E; Dienstverweigerung, die Zeile #64/#83 der
  Probenliste). `--max-decompressed-mb` gilt für das, was die Vorprüfung aus
  den Rohbytes auspackt. Sie las weniger als `lopdf` und der Schreibpfad:
  eine Filterkette mit Verweis buchte sie roh; von Flate las sie
  nur zlib und gab beim ersten Fehler auf, wo die anderen das Teilergebnis
  behalten und auf rohes Deflate zurückfallen; und den Schlüssel `/Filter`
  suchte sie als Bytefolge — mit `#xx` im Namen, als zweiter Eintrag oder
  hinter einem `/Filter` in einem inneren Dictionary kam die Kette an ihr
  vorbei. Am Stand `f6c5c68` brauchte eine Datei von 1 MB mit 1 GiB Nullen
  hinter `/Filter 6 0 R` am gebauten Binary 2,1 GB Spitze
  (`/usr/bin/time -v`), bei `--max-decompressed-mb 64`, und endete mit
  Rückgabewert 1 und der falschen Ursache, der Inhaltsstrom lasse sich nicht
  zerlegen; rohes Deflate und der Schlüssel mit `#xx` ebenso. Jetzt liest die
  Vorprüfung das Dictionary wie `lopdf`, zählt von Flate jedes Byte, das der
  Dekoder liefert, und bucht eine Kette mit Verweis nach dem Laden mit der
  aufgelösten Kette gegen dasselbe Budget. Belege:
  `zp_e_vorpruefung_als_schranke` (jede Form der Bombe und eine gewöhnliche
  Kette mit Verweis), `zp_e_teilergebnis_zaehlt`, die Einheitstests der Kette
  in `document.rs`. Mutationsnachweis: je Teil zurückgenommen → sein Fall rot.

* **⚠ Sicherheit: gebucht wurde die Ausgabe des letzten Glieds, nicht die
  Arbeit der Kette** (Register #99, Prüfer D; Dienstverweigerung, die Zeile
  #64/#83 der Probenliste). Die Vorprüfung, das Orakel und der Dekoder der
  Filterketten ließen jedes Glied für sich bis an die Grenze entpacken und
  buchten, was das letzte ausgab. Ein schrumpfendes letztes Glied verbarg
  alles davor: `[/FlateDecode /FlateDecode /ASCIIHexDecode]` über Nullen, die
  ASCIIHex als Leerraum überliest, buchte nichts. Am Stand `9bda3b6` brauchte
  eine Datei von 72 KB mit 200 solcher Ströme zu je 60 MiB Nullen am gebauten
  Binary 3 min 52 s (`/usr/bin/time -v`), bei `--max-decompressed-mb 64`, und
  endete mit Rückgabewert 0; `--check-leaks` lief nach 400 s noch. Jetzt gilt
  die Grenze für die ganze Kette — jedes Glied bekommt, was die davor übrig
  ließen —, und Vorprüfung wie Orakel buchen die Summe der Ausgaben aller
  Glieder; dieselbe Datei fällt in gut einer Sekunde am Budget. Belege:
  `zp_d_arbeit_der_kette` (Vorprüfung, Orakel, die Grenze der Kette, eine
  gewöhnliche Kette als Gegenprobe). Mutationsnachweis: je Leser
  zurückgenommen → sein Fall rot.

* **Eine Seite mit einem Kommentar vor einer Leerzeile wurde abgelehnt**
  (Register #103, beim Befund #98 gefunden; abgelehnte gewöhnliche Datei).
  Der Zerleger von `lopdf` bricht an einem Kommentar mit einer Leerzeile
  dahinter ab (`… ET`, `% Kopf`, Leerzeile, `BT …`), und ebenso an NUL und
  Seitenvorschub als Leerraum, die die Norm dazuzählt. Die Seite galt als
  nicht zerlegbar, und die Schwärzung brach mit „ließ sich nicht in
  Operationen zerlegen“ ab, obwohl `pdftotext` sie vollständig liest.
  Aufgefallen bei der Korrektur zu #98: der Prüfling in `zd_orakel_budget`
  trug genau so eine Seite, und das Orakel überging sie bis dahin stumm.
  Jetzt schreibt der Zerleger Kommentare und diesen Leerraum außerhalb
  literaler Zeichenketten vor dem zweiten Versuch in Leerzeichen um; was
  keine Syntax ist — ein verirrtes `]`, ein doppeltes Minuszeichen —, bleibt
  ein Bruch. Der Prüfling in `inline_image_boundaries`, der ein Nullbyte
  als Beispiel für Unzerlegbares nahm, trägt dafür jetzt eine verirrte
  Klammer. Belege: `zp_d_kommentar_und_leerraum`. Mutationsnachweis: kein
  Umschreiben → die Fälle rot; Zeichenketten nicht ausgenommen → das
  Prozentzeichen im Text rot.

* **⚠ Sicherheit: das Orakel übersah Stellen, ohne es zu sagen**
  (Register #98, Prüfer D; stilles Leck des Orakels, neue Klassen). Eine
  Seite, die der Interpreter nicht zerlegen kann — ein verirrtes `]`, ein
  doppeltes Minuszeichen, geschweifte Klammern, bis zum Punkt davor auch ein
  Seitenvorschub oder ein Nullbyte als Leerraum —, fehlte in der Sicht des
  Schriftdekoders ohne Meldung. Ein Strom, den der Lader anders übernahm, als er in den Rohbytes
  steht — eine falsche `/Length`, ein versetzter Querverweis —, fehlte in
  den Objektsichten ebenso. `pdftotext` las in beiden Fällen das Geheimnis
  in einer Schrift mit eigener Kodierung; `--check-leaks` meldete „nicht
  gefunden“. Jetzt steht jede abgelehnte Seite und jedes verlesene Objekt
  als eigene Zeile unter NICHT GEPRÜFT. Eine Seite, deren Inhaltsstrom
  hinter einem Bildfilter oder einem unbekannten Filter steht, bekommt keine
  zweite Zeile: den unbekannten nennt schon die Objektsicht, der Bildfilter
  ist der benannte blinde Fleck. Verglichen wird nur der Block des
  aktuellen Objekts: Altrevisionen, freigegebene Ströme und Objekt-Ströme,
  die der Lader entpackt ablegt, bleiben still. `LeakCheck::unchecked` trägt
  damit mehr gedeckelte Zähler als zuvor, und die Tabelle der Gründe in
  `SECURITY.md` ist um beide länger. Belege:
  `zp_d_seite_und_lader` (beide Fälle und die Gegenproben), dazu die
  Tabelle, gebunden in `belege.rs`. Mutationsnachweis: je
  Teil zurückgenommen → sein Fall rot; die Prüfung, dass hinter den
  geladenen Bytes `endstream` folgt, ist verteidigend und bleibt unter ihrer
  Rücknahme grün — `lopdf` übernimmt einen Strom nur so.

* **⚠ Sicherheit: ein Wort auf „stream“ verschluckte in der Rohsicht einen
  echten Strom** (Register #96, Prüfer D; stilles Leck des Orakels, neue
  Klasse). Die Rohsicht findet Ströme an den Bytes `stream` … `endstream`,
  unabhängig von der Querverweistabelle — so liest sie die Altrevisionen
  eines inkrementellen Updates, die keine andere Sicht erreicht. Ob `stream`
  dort ein Schlüsselwort war, prüfte sie nicht: ein Titel „Protokoll
  Livestream“ oder eine Schrift `/BitstreamVeraSans` öffnete einen Block bis
  zum nächsten `endstream`, der Flate-Strom einer Altrevision darin wurde nie
  entpackt, und `--check-leaks` meldete „nicht gefunden“. Jetzt steht das
  Schlüsselwort nach Leerraum oder einem Trennzeichen und vor dem
  Zeilenende. Benannte Lücke: ein Wort `stream` am Zeilenende eines Literals
  sieht weiter aus wie das Schlüsselwort. Der Entwurf des Belegs war ein
  Scheintest — Flate legte den kurzen Klartext ungepackt ab, und die
  Rohsicht fand das Geheimnis ohne jeden Strom; der Prüfling packt jetzt
  wirklich und prüft das. Belege: `zp_d_scheinstrom`,
  `audit_bytes::tests::only_the_stream_keyword_opens_a_raw_block`.
  Mutationsnachweis: jede Hälfte der Prüfung zurückgenommen → ihr Fall rot.

* **⚠ Sicherheit: die Rohsicht las das Dictionary einer Altrevision
  schlecht, und eine gescheiterte Kette meldete sie nie** (Register #95,
  Prüfer D; stilles Leck des Orakels, die Zeile #80 der Probenliste trat
  weiter auf). Die Rohsicht liest `/Filter` und `/DecodeParms` einer
  Altrevision aus den Rohbytes und trennte die Wörter dort nur an Leerraum:
  `/Filter[/ASCII85Decode/FlateDecode]`, wie iText und `lopdf` es
  schreiben, war ein einziger Name; `/DecodeParms<</Predictor 12/Columns
  8>>` ein Prädiktor ohne Zahl, `/DecodeParms 9 0 R` gar keiner; hinter
  `stream \r\n` begann der Strom beim Leerzeichen. Das Geheimnis kam als
  „nicht gefunden“ zurück — und `/FooDecode` an einer Altrevision stand in
  keiner Zeile. Jetzt liest die Rohsicht mit dem Wortzerleger der
  Vorprüfung, löst einen Verweis aus den Rohbytes auf (die Definition, die
  dem Strom am nächsten steht) und meldet eine gescheiterte Kette mit dem
  Wortlaut der Objektsicht — dort, wo keine Objektsicht denselben Strom
  gelesen hat. Weil die Rohsicht damit zum ersten Mal Ketten liest, die
  `lopdf` geschrieben hat, stimmten alte Belege nicht mehr: der zu
  ASCII85 am genau passenden Budget hielt seit #64 nichts mehr (die
  Vorprüfung lehnte die Datei ab, die Objektsicht lief nie) und misst jetzt
  die Rohsicht; der zu vielen kleinen Strömen findet das Geheimnis jetzt in
  der Rohsicht und nennt den Strom, der ihr Budget sprengt. Belege:
  `zp_d_rohes_dictionary`, `ze_p1_befunde`, `ze_p1_budget_und_filter`.
  Mutationsnachweis: je Teil zurückgenommen → sein Fall rot.

* **⚠ Sicherheit: UTF-8 mit BOM und WinAnsi im Inhaltsstrom las das Orakel
  nicht** (Register #97, Prüfer D; stilles Leck des Orakels, die Zeile #81
  der Probenliste trat weiter auf). Eine Zeichenkette in UTF-8 mit BOM, wie
  PDF 2.0 sie erlaubt, las sich als PDFDocEncoding — „Grüße“ als Zeichensalat.
  Eine Zeichenkette in einem Inhaltsstrom mit einer Standardschrift ist
  WinAnsi: das Byte für `€` ist dort ein anderes als in PDFDocEncoding, die
  Verkettung las das Euro als Aufzählungspunkt, und die Bytesuche kannte
  keine WinAnsi-Form des Begriffs. Beides fiel vor allem in der Altrevision
  auf, die nur die Rohsicht liest; „nicht gefunden“ ohne jede Meldung. Jetzt
  liest der Zeichenkettendekoder UTF-8 am BOM, die Bytesuche sucht einen
  Begriff mit solchen Zeichen auch in WinAnsi (roh und als Hex-String), und
  die Verkettung liest dieselben Zeichenketten ein zweites Mal als WinAnsi —
  nur, wo ein Byte darin anders gelesen würde. README und `--help` sagen es.
  Belege: `zp_d_bom_und_winansi`. Mutationsnachweis: je Teil zurückgenommen →
  sein Fall rot; die zweite Lesart auch ohne Unterschied → die Gegenprobe rot.

* **Die Rohsicht arbeitete je Block, nicht je Byte** (Register #100, Prüfer
  D; Dienstverweigerung, neue Klasse). Jeder rohe Strom suchte seinen
  Objektkopf über ein festes Fenster rückwärts und sein Dictionary noch
  einmal, und er baute für den blinden Entpackversuch jedes Mal neue Dekoder.
  Eine Datei aus lauter `stream`/`endstream` kostete so je Block das ganze
  Fenster, und der Lauf wuchs mit Blöcken mal Fenster statt mit der Datei.
  Dieselbe Klasse stand im Vergleich mit dem Lader aus dem Befund zu
  verlesenen Strömen: ob zwischen Querverweis und Block ein `endobj` steht,
  suchte er je Objekt im ganzen Bereich. Jetzt endet die Rückwärtssuche am
  Ende des vorigen Blocks, die Dekoder werden einmal gebaut und je Block
  zurückgesetzt, und die `endobj`-Stellen stehen einmal in einem
  Verzeichnis. Nebenbei heißt ein Block ohne eigenen Kopf nicht mehr wie das
  Objekt davor und erbt dessen Dictionary nicht. Die Laufzeit selbst ist an
  keinen Test gebunden — Zeitmessungen flattern auf dem Windows-Läufer —,
  gebunden ist, was sich daran beobachten lässt. Belege:
  `zp_d_rueckwaertssuche`,
  `audit_bytes::tests::the_header_search_stops_at_the_previous_block`,
  `audit_bytes::tests::blind_inflate_of_plain_text_yields_nothing` (der
  zurückgesetzte Dekoder liefert, was der alte Weg lieferte). Mutationsnachweis:
  je Teil zurückgenommen → sein Fall rot.

* **⚠ Sicherheit: das Erscheinungsbild eines Widgets, das keine Seite zeigt,
  blieb stehen** (Register #90, Prüfer B; stilles Leck, die Zeile #71 der
  Probenliste trat weiter auf). Ein Widget, das nur in den `/Kids` seines
  Formularfelds hängt, nicht in `/Annots` einer Seite, zeichnet kein
  Betrachter. Der Trägerlauf erreichte es über ein Geschwister auf der Seite
  (`/Parent`, `/Kids`), nahm ihm die Texte und hielt es damit am Leben; sein
  `/AP` las niemand, denn die Analyse liest, was eine Seite zeigt. Der
  gezeichnete Feldwert stand nach dem Lauf in der Datei, ohne Warnung. Jetzt
  steht vor dem Trägerlauf fest, welche Träger eine Seite zeigt; ein Träger
  außerhalb davon verliert sein `/AP` und die Symbole in seinem `/MK` — ein
  `/MK` als eigenes Objekt nur, wenn kein gezeigtes Widget es benutzt.
  Belege: `zp_b_nicht_gezeichnete_annotation`. Mutationsnachweis: je Teil
  zurückgenommen → sein Fall rot, auch die Gegenproben (direkt eingebettete
  Widgets, `/Annots` als eigenes Objekt, ein geteiltes `/MK`).

* **⚠ Sicherheit: das Ende einer langen Antwortkette behielt sein
  Erscheinungsbild** (Register #91, Prüfer B; stilles Leck, die Zeile #71
  der Probenliste trat weiter auf). Die Analyse folgt von einer
  Seitenannotation aus `/Popup` und `/IRT` nur bis zu einer festen Stufe;
  der Trägerlauf ging bis zum Ende der Kette und hielt sie am Leben, ohne
  Meldung. Das `/AP` am Ende stand nach dem Lauf in der Datei. Die Regel aus
  dem Punkt davor schließt es: ein Glied, das in keinem `/Annots` steht,
  zeichnet niemand, und es verliert sein Erscheinungsbild — die Stufengrenze
  der Analyse bleibt, und was hinter ihr liegt, hat kein Bild mehr. Ein
  eigener Commit mit eigenem Beleg, weil es ein eigener Befund war:
  `zp_b_nicht_gezeichnete_annotation`, der Fall der langen Antwortkette.
  Mutationsnachweis: die Regel zurückgenommen → dieser Fall rot.

* **Ein `/Annots`, das sich Seiten teilen, kostete Seiten mal Annotationen**
  (Register #94, Prüfer B; Dienstverweigerung, neue Klasse). Nach der Norm
  steht eine Annotation im `/Annots` genau einer Seite. Teilten sich viele
  Seiten dasselbe Array, las die Analyse jede Annotation auf jeder Seite neu,
  der Metadatenlauf gab jeder Seite eine eigene Kopie des Arrays, und der
  Redaktor fragte je Seite jede Annotation nach ihrem Rechteck und kopierte
  dafür ihr Dictionary. Kein Konto sah es, denn das Aufwandskonto beginnt je
  Seite neu. Jetzt führt die Analyse ein Buch über das ganze Dokument: eine
  Annotation, die eine frühere Seite unter derselben Ressourcenumgebung
  gelesen hat, wird nicht noch einmal gelesen; ihr Text steht einmal im
  Ergebnis, auf der ersten Seite, die sie zeigt. Unter einer anderen
  Umgebung wird sie neu gelesen, weil ein Erscheinungsstrom ohne eigene
  Ressourcen die der Seite benutzt und dort anderen Text zeigen kann. Diese
  Wiederholungen haben eine Decke, darüber wird die Datei abgelehnt. Eine
  abgelehnte Seite trägt nichts ins Buch ein. Metadatenlauf und Redaktor
  bereinigen ein geteiltes Array an seinem Objekt, einmal; alle Seiten zeigen
  danach weiter dasselbe Array, ohne toten Verweis. Benannte Lücke: der
  Hinweis, dass eine Schwärzung in einem geteilten Formular auch andere
  Seiten trifft, zählt die übrigen Seiten einer geteilten Annotation nicht
  mit. Dieselbe Vervielfachung trifft einen Inhaltsstrom oder ein Formular,
  das sich Seiten teilen; das ist ein eigener Befund (Register #106). Belege:
  `zp_b_geteiltes_annots`, `content::tests::die_wiederholungsdecke_haelt_genau`,
  `content::tests::ein_geteiltes_array_kostet_seine_eintraege_einmal`.
  Mutationsnachweis: je Teil zurückgenommen → sein Fall rot; was nur Arbeit
  spart, zeigt die Messung im Release.

* **⚠ Sicherheit: die Einstellungen einer RichMedia-Annotation hielten ihre
  Dateien** (Register #92, erster Teil, Prüfer B; stilles Leck, die Zeile
  der Metadaten-Träger trat weiter auf). Eine `/RichMedia`-Annotation trägt
  neben `/RichMediaContent` ein `/RichMediaSettings`; unter `/Activation`
  zeigt jede Instanz einer `/Configuration` mit `/Asset` auf dieselben
  Filespecs, die `/Assets` nennt, und `/Scripts` auf weitere. Der
  Metadatenlauf nahm nur den Inhalt, die eingebettete Datei blieb über die
  Einstellungen erreichbar und stand nach dem Lauf in der Ausgabe, ohne
  Warnung. Jetzt fällt `/RichMediaSettings` als Ganzes wie `/RichMediaContent`;
  README und der Bericht nennen es. Belege: `zp_b_richmedia_einstellungen`.
  Mutationsnachweis: der Schlüssel aus der Liste genommen → beide Fälle rot.

### Spur-A-Runde 1: die Probenliste hält, und sie war nicht vollständig

Die erste Runde unter dem Mandat aus `CONTRIBUTING.md` („prüfe, ob eine Zeile
der Probenliste noch auftritt, und suche nach einer Klasse, die nicht auf ihr
steht“). Fünf Prüfer, nur Schwärzung, je ein Gebiet: Bild, Metadaten, Spiegel
über Formular, Orakel und Filterkette, Decken.

**Die erste Frage: nein.** Alle acht Zeilen der Spur A in
[`PRUEFLISTE.md`](PRUEFLISTE.md) sind mit eigenen Läufen grün — nicht durch
Lesen der Belegdateien, sondern mit neuem Material am ehrlichen Orakel. Keine
Klasse, die schon auf der Liste stand, tritt noch auf.

**Die zweite Frage: ja, und zwar 19 Mal** (Register #64 bis #82). Die Liste
war eine Abwesenheitsbehauptung mit anderem Namen: 13 stille Lecks am
Schwärzungscode, drei stille Lecks am Orakel selbst, drei Dienstverweigerungen
und eine abgelehnte gewöhnliche Datei. Die Belege der Prüfer liegen als
`zo_*`-Dateien im Baum; jeder noch rote Test trägt `#[ignore]` mit seiner
Registernummer und wird mit seiner Korrektur scharf — ein Befund, ein Commit,
CI nach jedem Push. Die Runde zählt nicht gegen das Ende der Spur A; sie hat
den Umfang sichtbar gemacht. Was geschlossen ist, steht darunter, jeweils mit
seiner neuen Zeile in der Probenliste.

* **⚠ Sicherheit: eine Seite außerhalb des Seitenbaums ging mit ihrem ganzen
  Inhalt ungeschwärzt in die Ausgabe** (Register #69, Prüfer B). Eine
  gelöschte Seite, die ein stehen gebliebener Verweis hält — das `/Dest
  [Seite /Fit]` eines Links oder das `/P` einer Annotation —, hängt nicht mehr
  in `/Kids`; `get_pages` kennt sie nicht, sie geht durch keine Schwärzung
  und keine Warnung, und das Aufräumen behält sie, weil sie erreichbar ist.
  Am gebauten Binary: Rückgabewert 0, `--check-leaks` an der Ausgabe
  Rückgabewert 3 mit dem Rohstrom der alten Seite. Jetzt verliert jede Seite,
  die nicht im Baum hängt, ihren Inhalt — `/Contents`, `/Annots`,
  `/Resources`, `/Thumb`, `/AA`, `/B`, `/VP`, `/PresSteps` —, wer immer sie
  hält; der Verweis führt danach auf eine leere Seite, und der Bericht nennt
  sie („Seite außerhalb des Seitenbaums geleert“). Geleert statt gekappt,
  weil die Halter nicht abschließend aufzählbar sind. Beleg:
  `zo_b_traeger::b_verwaiste_seite_hinter_dest_oder_p_bleibt_samt_inhalt`
  (beide Halter). Mutationsnachweis: `empty_orphan_pages` gibt null zurück,
  ohne zu leeren → rot.

* **⚠ Sicherheit: das Vorschaubild `/Thumb` einer Seite überlebte den Lauf
  mit den Bildpunkten von vorher** (Register #74, Prüfer A). Ein `/Thumb` ist
  ein Raster der Seite (Tabelle 30) — ein Bild-XObject, das kein `Do`
  zeichnet und darum am Bildlauf vorbeiging; `/Thumb` kam in keiner Datei des
  Crates vor. Nach der Schwärzung des gezeichneten Bildes fand das Orakel die
  Klartext-Bildpunkte im Vorschaubild, ohne Warnung. Jetzt fällt `/Thumb` an
  jeder Seite im Metadatenlauf und steht im Bericht („Vorschaubild
  (/Thumb)“) — ein Vorschaubild einer geschwärzten Seite wäre ohnehin falsch.
  Beleg: `zo_a_bild_ohne_zeichner::vorschaubild_der_seite_ueberlebt_den_lauf_ohne_warnung`.
  Mutationsnachweis: die `take`-Zeile für `/Thumb` entfernt → rot.

* **⚠ Sicherheit: 13 Schlüssel an Katalog, Seiten und Objekten trugen
  Klartext durch den Metadatenlauf** (Register #70, Prüfer B). Der Lauf
  kannte am Katalog, an der Seite und an den übrigen Objekten je eine feste
  Liste; alles andere hielt das Aufräumen am Leben, weil es erreichbar war.
  Was das im Feld heißt: `/Perms` hält das Signatur-Dictionary jeder von
  Acrobat zertifizierten Datei ein zweites Mal — der Feldwert fiel,
  `/Reason`, `/Location`, `/ContactInfo` und `/Name` blieben; `/DSS` trägt
  Zertifikate mit Unterzeichnernamen; `/PageLabels` das Präfix, das jeder
  Betrachter zeigt; `/OutputIntents` die Texte jeder PDF/A- und PDF/X-Datei;
  `/DPartRoot` die PDF/VT-Metadaten des Kontoauszugdrucks mit Name und
  Konto des Empfängers; `/Threads` und `/B`, `/Collection`, `/URI /Base`,
  `/VP`, `/PresSteps`, `/Ref` (ein Referenz-XObject hält eine eingebettete
  Datei), `/OPI`, und `/SV`, `/Lock` am Signaturfeld. Alle mit Rückgabewert
  0 und leerem Bericht. Jetzt fallen sie — die Präfixe und Texte in
  `/PageLabels` und `/OutputIntents` einzeln, damit Nummerierung und
  Farbprofil bleiben, alles andere ganz — und der Bericht nennt sie
  („Beiwerk mit Klartext“). Belege: dreizehn `b_*`-Tests in
  `zo_b_traeger`, jeder mit seinem Träger; Mutationsnachweis: die
  Katalogschleife entfernt → sechs davon rot.

* **⚠ Sicherheit: die Stencil-Maske eines geschwärzten Bildes behielt unter
  der Zone ihre Bits — und die Bits sind die Form** (Register #77, Prüfer A).
  Ein `/Mask`-Strom wurde beim Neukodieren unverändert mitgeschrieben, weil
  er das Bild in eigener Auflösung beschreibt; der Modulkopf nannte das einen
  Schutz. Er übersah, dass die Maske selbst Bildinhalt ist: ein Textumriss als
  Stencil trägt den Text, auch wenn darunter jede Farbe schwarz ist. Das
  Orakel fand den Suchbegriff im Maskenstrom der Ausgabe, die gezeigte
  Alphaebene war dieselbe wie vorher, keine Warnung. Jetzt wird die Maske nur
  dann unverändert übernommen, wenn kein Bildpunkt gefallen ist; sonst geht
  die Alphaebene hinaus, die `Work::fill` unter der Zone auf undurchsichtig
  setzt — außerhalb der Zone dieselbe Maske, darunter die Schwärzung, als
  `/SMask` in Bildauflösung. Der Preis, dass die Maske ihre eigene Auflösung
  verliert, fällt nur bei einem Bild an, das wirklich geschwärzt wurde. Eine
  Maske, die sich nicht dekodieren ließ, kann nicht zur Alphaebene werden;
  sie bleibt als `/Mask` stehen (sonst würden die verdeckten Bildpunkte
  sichtbar), und der Lauf warnt, dass ihre Bits unter der Schwärzung stehen
  bleiben — vorher ging sie stumm mit. Beleg:
  `zo_a_maske_und_filter::stencil_maske_behaelt_unter_der_zone_ihre_bits`
  (Orakel und Alphaebene). Mutationsnachweis: die Bedingung `filled == 0`
  entfernt → rot.

* **⚠ Sicherheit: ein Formular unter zwei Umgebungen, beide mit Spiegel —
  nur der erste Fundort wurde geleert** (Register #65, Prüfer C). Ein
  Form-XObject ohne eigenes `/Resources` löst `/MC0` unter jeder Umgebung neu
  auf: zeichnet es erst ein äußeres Formular mit einem Spiegel und danach die
  Seite mit einem anderen (oder zwei Seiten mit je eigenem `/Properties`),
  sind das zwei Listen an zwei Fundorten. Die Sammelstelle des Scans hielt je
  Strom und Operation nur den **ersten** Datensatz und verwarf den zweiten —
  dessen Klartext blieb im Verzeichnis der Seite, ohne Warnung, mit
  Rückgabewert 0; `--check-leaks` an der Ausgabe fand ihn. Jetzt gehört die
  Herkunft der Liste zum Schlüssel (Eigentümer der Ressourcen und Objekt-Id
  der Liste), in `Collector::seen_marked` wie über Seiten hinweg in
  `form_marked_seen`; ein wirklich mehrfach platziertes Formular kommt weiter
  nur einmal. Belege:
  `zo_c_spiegel_umgebungen::zwei_umgebungen_beide_mit_spiegel_auf_einer_seite`
  und `…_auf_zwei_seiten` (sieben Ausprägungen, darunter die stille, in der
  beide Spiegel wortgleich mit den Glyphen sind). Mutationsnachweis: Herkunft
  aus dem Schlüssel des Scans entfernt → der Ein-Seiten-Beleg rot.

* **Eine reelle Breite umging die Bilddecke** (Register #79, Prüfer A).
  `/Width 100.0` ist nach PDF 32000-1 eine Breite; die Vorprüfung des
  Bildlaufs las eine reelle Zahl aber als null und ließ ein Bild unter jeder
  Decke durch, während der Dekoder es in voller Größe auspackte. Die harte
  Pixeldecke stand, umgangen war `--max-image-mb`. Jetzt liest die Vorprüfung
  dieselbe Zahlenart wie der Dekoder, eine reelle Zahl aufgerundet. Beleg:
  `zo_a_maske_und_filter::budget_wird_von_reeller_breite_umgangen` neben der
  ganzzahligen Kontrolle. Mutationsnachweis: reelle Zahl wieder als null
  gelesen → rot.

* **Eine Kette aus Verweisobjekten hielt den Metadatenlauf für Minuten
  an** (Register #72, Prüfer B). Die Verweiskarte `Chains::of` hält vor jeder
  Änderung fest, wo eine Kette aus Objekten endet, die nichts als ein Verweis
  sind (`4 0 obj 5 0 R`) — damit der Bericht das richtige Objekt zählt. Sie
  lief dafür von **jedem** Glied aus bis zum Ende, Aufwand n²/2. Am Stand
  `456d669` brauchte `strip_metadata` an einer Kette aus 8 000 Gliedern 33 s,
  an 20 000 Gliedern 223 s im Testprozess (Debug); die Datei dazu ist 936 kB
  groß und liegt unter jeder Grenze des Laders. Jetzt übernimmt ein Glied das
  Ende, das ein früheres schon gefunden hat, und der ganze gelaufene Pfad
  bekommt es eingetragen, Ringe eingeschlossen — jedes Glied wird genau einmal
  gelaufen. Dieselben 20 000 Glieder laufen im Testprozess (Debug) jetzt in
  0,08 s; die Decke des Belegs liegt bei 5 s, in beiden Richtungen der Kette.
  Beleg: `zo_b_traeger::c_verweiskette_kostet_nicht_quadratisch`.
  Mutationsnachweis: die Übernahme bekannter Enden entfernt → rot (die
  absteigende Kette).

* **⚠ Sicherheit: nach dem Kopieren eines geteilten Bildes blieb das
  Original mit unversehrten Bildpunkten in der Datei — hinter einem Namen,
  den niemand zeichnet** (Register #76, Prüfer A). Hängt ein Bild an mehreren
  Seiten, wird es je Seite kopiert und jeder getroffene Name auf die Kopie
  umgebogen (`Fate::Copy`). Das Original bleibt erreichbar, wo kein `Do` es
  nennt: im geerbten `/Resources` des `/Pages`-Knotens, unter einem
  überzähligen Eintrag der Seite. `prune_unreachable` sah es als erreichbar,
  und das Orakel fand in der Ausgabe die Klartext-Bildpunkte, ohne Warnung.
  Die Verweise sind nicht abschließend aufzählbar; der Gegenstand ist es: ein
  Original, das auf jeder Seite, die es zeichnet, kopiert wurde und dessen
  Platzierungen dort alle auf die Kopie zeigen, wird durch ein leeres Bild
  ersetzt (ein Bildpunkt Schwarz) und im Bericht gezählt
  (`retired_originals`). Zeigt eine Platzierung noch das Original — keine
  Zone lag auf ihr —, bleibt es. Belege:
  `zo_a_bild_ohne_zeichner::geerbte_ressourcen_halten_das_original_nach_der_kopie`
  und `…::ueberzaehliger_name_haelt_das_original_nach_der_kopie`, Gegenprobe
  `…::ueberzaehliger_name_ist_harmlos_wenn_ueberschrieben_wird`.
  Mutationsnachweis: die Nachlese entfernt → beide rot.

* **⚠ Sicherheit: ein Bild in einem Kachelmuster ohne Text blieb
  ungeschwärzt und ungemeldet** (Register #75, Prüfer A). Der Musterlauf
  stieg vor dem Durchlaufen aus, wenn der Musterstrom keinen Textoperator
  hatte („Schraffur- oder Logomuster“) — und sah dabei nur die Operatoren des
  Musterstroms selbst. Ein `Do` darin galt als Schraffur: ein Bild im Muster
  bekam der Bildsammler nie zu sehen, ein Formular im Muster (mit Text oder
  Bild darin) niemand. Der Bericht zählte null geschwärzte Bilder, keine
  Warnung, das Orakel fand die Klartext-Bildpunkte in der Ausgabe. Jetzt
  steigt der Lauf nur aus einem Muster aus, das weder Text setzt noch etwas
  platziert; die Musterwarnung sagt, was sie an der ersten Kachel vermessen
  hat („enthält Text“, „platziert Bilder oder Formulare“). Belege:
  `zo_a_bild_ohne_zeichner::bild_im_kachelmuster_ohne_text_faellt` (Bild
  direkt im Muster) und `…::bild_im_formular_im_kachelmuster_faellt` (Bild im
  Formular im Muster). Mutationsnachweis: Ausstieg wieder allein am
  Textoperator → beide rot.

* **Ein Spiegel über einem Kachelmuster mit Text blieb stehen** (Register
  #66, Prüfer C; kein stilles Leck — `--check-leaks` fand ihn, Rückgabewert
  3). `/Span <</ActualText (…)>> BDC … /P0 scn … re f … EMC` im Seitenstrom:
  die Glyphen fielen aus dem Musterstrom, der Spiegel im Seitenstrom nicht.
  Der Geltungsbereich eines Spiegels kannte nur die Formulare am `Do`; ein
  Muster wird am `scn` gesetzt, und sein Strom war ihm fremd. Dasselbe über
  ein Formular, das mit dem Muster füllt. Jetzt zählt ein Kachelmuster am
  `scn` wie ein Formular am `Do` — in der Spiegelliste des Stroms und als
  Verschachtelung nach außen (`form_within`), und fällt unter ihm ein
  Zeichen, fällt der Spiegel. Belege:
  `zo_c_spiegel_umgebungen::spiegel_ueber_kachelmuster_mit_text` (drei
  Ausprägungen) und `…::spiegel_ueber_formular_das_mit_kachelmuster_fuellt`.
  Mutationsnachweis: Muster aus der Spiegelliste → der erste rot;
  `form_within` entfernt → der zweite rot.

* **⚠ Sicherheit: zwei Erscheinungsströme, die niemand las — das Symbol
  eines Druckknopfs und das `/AP` einer Annotation, die nur ihr Popup oder
  eine Antwort am Leben hält** (Register #71, Prüfer B). Der Lauf las die
  Erscheinungsströme (`/AP`) der Annotationen in `/Annots` — und nur dort.
  Das Symbol eines Druckknopfs (`/MK /I`, `/RI`, `/IX`, Tabelle 189) ist ein
  Form-XObject wie ein Erscheinungsstrom und darf Text zeichnen; es stand
  nach dem Lauf unverändert in der Datei. Und eine Notiz, die ein Werkzeug
  aus `/Annots` gestrichen, deren Popup es aber vergessen hat, erreichte der
  Metadatenlauf über `/Parent` und nahm ihr `/Contents` — ihren
  Erscheinungsstrom las niemand; dieselbe Form über `/IRT`. Beides ohne
  Warnung, das Orakel fand den Text in der Ausgabe. Jetzt liest der Lauf die
  Symbole wie die Erscheinungsströme, abgebildet in das `/Rect` des Widgets,
  und folgt von jeder Annotation `/Popup` und `/IRT` und vom Popup aus
  `/Parent` — die Analyse findet den Text dort, eine Mustersuche trifft ihn,
  und die Schwärzung aus dem Fund entfernt ihn wie bei jedem `/AP`. Belege:
  `zo_b_traeger::b_mk_icon_mit_text_faellt` (drei Schlüssel) und
  `…::b_ap_einer_nur_ueber_popup_oder_irt_gehaltenen_annotation_faellt`
  (beide Wege), je: die Analyse sieht den Text, die Schwärzung aus dem Fund
  lässt nichts stehen. Mutationsnachweis: Symbole nicht gelesen → der erste
  rot; Verfolgung entfernt → der zweite rot.

* **Die Spiegel in den Formularen aller Seiten wurden bis zum Ende des Laufs
  gehalten, ohne Decke** (Register #68, Prüfer C). Ein Spiegel in einem
  Form-XObject wird erst geleert, wenn das Formular neu geschrieben wird —
  nach der Seitenschleife. Bis dahin hielt der Lauf den Datensatz, wie der
  Scan ihn liefert: je Platzierung im Geltungsbereich einen Pfad auf dem
  Haufen, für jede Seite. Die Decke je Seite fing das nicht (sie zählt nur
  eine Seite), die Decke der zurückgestellten Spiegel auch nicht (sie gilt nur
  dem Seitenstrom). Am Stand `e016a2f` brauchte der Redaktor an je Seite
  einem eigenen Formular mit 100 × 999 Paaren im Testprozess (Debug, eigener
  Prozess) für 10 Seiten 78 MB und für 100 Seiten 637 MB bei 9,8 s — aus rund
  10 kB Datei je Seite, ohne Schwärzung, ohne Warnung. Jetzt wird die
  schlanke Fassung gehalten (je Formular eine Id, keine Pfade — mehr fragt
  die späte Entscheidung nicht), und ein Konto über den ganzen Lauf deckelt
  sie bei 100 000 Einträgen; darüber wird nicht mehr gehalten, und der Bericht
  sagt es. Dieselben 10 Seiten brauchen im Testprozess (Debug, eigener
  Prozess) jetzt 20 MB, 100 Seiten 31 MB. Belege:
  `zo_c_spiegel_umgebungen::c4_spiegel_in_formularen_kosten_je_seite_wenig`
  (im Testprozess, Debug, als Kindprozess: 100 Seiten unter 200 MB) und
  `…::c4_decke_der_gehaltenen_formularspiegel_wird_gesagt`.
  Mutationsnachweis: Datensatz ungekürzt gehalten → der erste rot; Decke
  entfernt → der zweite rot.

* **Ein LZW-gepacktes Bild unter der Zone beendete den Lauf ohne
  Ausgabedatei** (Register #78, Prüfer A; abgelehnte gewöhnliche Datei).
  `LZWDecode` ist ein Filter aus PDF 1.0, den ältere Distiller, `tiff2pdf`
  und Ghostscript ohne Flate schreiben. Das Orakel und der Textlauf
  entpacken ihn (`filters.rs`, `weezl`); der Bilddekoder kannte ihn nicht,
  gab einen Platzhalter zurück, und ohne `--allow-undecodable-images` endete
  der Lauf mit Fehler — mit Zugeständnis blieb das Bild ungeschwärzt stehen,
  gesagt, aber für eine gewöhnliche Datei. Jetzt entpackt der Bilddekoder LZW
  über denselben Dekoder, mit Prädiktor, und mit einer Grenze aus `/Width` ×
  `/Height`: was das Bild laut Dictionary nicht braucht, wird nicht entpackt
  (ein Strom darüber gilt als nicht dekodierbar und wird so gemeldet).
  Belege: `zo_a_maske_und_filter::lzw_bild_unter_der_zone_faellt` (vorher
  absichtlich rot), `…::lzw_bild_mit_zugestaendnis_faellt_ebenso` (band das
  alte Verhalten) und `…::lzw_bild_ueber_der_grenze_wird_nicht_entpackt`.
  Mutationsnachweis: LZW wieder nicht unterstützt → alle drei rot; Grenze
  entfernt → der dritte rot.

* **⚠ Sicherheit: die Entpackgrenze galt nicht für eine Filterkette, deren
  Glied die Vorprüfung nicht auspackte** (Register #64, Prüfer E;
  Dienstverweigerung). Die Vorprüfung des Laders buchte eine Kette mit einem
  Glied, das sie nicht auspacken konnte (`RunLengthDecode`, `ASCIIHexDecode`),
  **ganz** roh — auch das Flate-Glied davor —, und der Schreibpfad entpackte
  die Kette danach ohne Grenze. Am Stand `14d03c7` stand `/Filter
  [/FlateDecode /RunLengthDecode]` über einem RunLength-Strom, der sich auf
  2,4 GB aufbläst, als 39 KB in der Datei; am gebauten Binary brauchte der
  Lauf 3,8 GB Spitze (`/usr/bin/time -v`), und unter einer Adressraumgrenze
  von 1,5 GiB starb er mit Signal statt mit einer Budgetmeldung. Jetzt packt
  die Vorprüfung jede Kette aus, deren Glieder `filters.rs` begrenzt entpacken
  kann — dieselben Dekoder wie der Schreibpfad, Glied für Glied gegen das
  Budget, ohne `lopdf` —, und die Bombe fällt dort mit Rückgabewert 1 und
  der Budgetmeldung. Die Rohgrößen-Grenze der Altlast-Filter gilt weiter nur
  `LZWDecode` und `ASCII85Decode`. Für das Orakel heißt das: eine solche
  Datei lehnt die Vorprüfung als Ganzes ab (`unchecked` nennt den
  Objektgraphen mit dem Budget), wo sie vorher den einen Strom nannte —
  `zd_orakel_budget`, `ze_p1_budget_und_filter`, `zg_r2_decke`,
  `zf_q2_teildekoder` und `ze_p4_check_leaks_grenzen` sind darauf
  umgestellt. Die Objektsicht läuft seither nur noch an einem Strom ans
  Budget, den die Vorprüfung roh bucht — einer Kette mit einem Bildfilter
  oder einem unbekannten Filter am Ende; die Kommandozeile zeigt die Zeile
  über den nicht gelaufenen Schriftdekoder deshalb an keiner der alten
  Proben mehr. Belege:
  `zo_e_kettenbombe_schreibpfad::die_entpackgrenze_gilt_auch_auf_dem_schreibpfad`
  (Linux, Adressraumgrenze über `ulimit -v`, vorher absichtlich rot).
  Mutationsnachweis: `RunLengthDecode` wieder roh gebucht → rot (vom
  Speicherlimit getötet).

* **⚠ Sicherheit: das Orakel las PDFDocEncoding als Latin-1 — ein `€`, ein
  Gedankenstrich, ein typografischer Apostroph im Suchbegriff wurden nicht
  gefunden, ohne Meldung** (Register #81, Prüfer D). PDF 32000-1, Anhang D.2
  weicht im Block 0x80–0xA0 und bei den Akzenten 0x18–0x1F von Latin-1 ab:
  dort stehen `•`, `–`, `—`, `…`, die typografischen Anführungszeichen, `™`,
  `ﬁ`, `ﬂ` und `€` (0xA0 — nicht das geschützte Leerzeichen). Der Dekoder
  des Orakels las jedes Byte als Latin-1; `--check-leaks "Betrag 5 €"` an
  einer Datei, die `(Betrag 5 \240)` trägt, sagte „nicht gefunden“ mit
  Rückgabewert 0 — auch an der Ausgabedatei des Schwärzungslaufs, wenn der
  Träger ein `/ActualText` ist, das der Lauf nicht räumt. Jetzt liest
  `decode_pdf_string` die Abweichungen nach Anhang D.2; dieselbe Funktion
  liest die Zeichenketten der Analyse und den Spiegelvergleich des
  Redaktors. Belege:
  `zo_d_altgeneration_und_kodierung::zo_d4_pdfdoc_zeichen_stilles_leck` (je
  Begriff roh und oktal maskiert — die maskierte Form trifft keine Rohsicht,
  nur den Dekoder) und
  `zo_d_orakel_am_binary::zo_d12_pdfdoc_in_der_ausgabedatei_stilles_leck`
  (am gebauten Binary, an der Ausgabedatei); beide vorher absichtlich rot.
  Mutationsnachweis: jedes Byte wieder Latin-1 → beide rot.

* **⚠ Sicherheit: das Orakel sah eine Altgeneration nur, wenn sie roh oder
  mit reinem Flate geschrieben war** (Register #80, Prüfer D). Ein
  inkrementelles Update lässt die Vorgängerfassung eines Objekts in der
  Datei; die neue Querverweistabelle nennt sie nicht mehr, also steht sie in
  keiner Objektsicht. Die Rohsicht der Ströme versuchte an jedem Block nur
  zlib und rohes Deflate: unter `/LZWDecode`, `/ASCII85Decode`, ASCIIHex mit
  Zeilenumbrüchen, Flate mit PNG-Prädiktor oder `[/ASCII85Decode
  /FlateDecode]` (Distiller) kein Fund, keine Meldung. Und die Rohsicht der
  Datei verglich nur Bytes: eine Zeichenkette der Altgeneration mit oktalen
  Escapes, als UTF-16BE so maskiert wie pdfTeX es schreibt, als Hex-String
  mit Leerraum oder mit Zeilenfortsetzung — kein Fund, keine Meldung. Jetzt
  entpackt die Rohsicht jeden Block über die Filterkette seines eigenen
  Dictionaries, mit demselben Dekoder wie die Objektsicht (`/Filter` und
  `/DecodeParms` aus den Rohbytes gelesen, ein Glied, das kein Filtername
  ist, hält die Kette an wie dort; der blinde zlib-Versuch bleibt für Blöcke
  ohne lesbaren Kopf), und ein zweiter Gang der Rohsicht liest jedes
  Zeichenketten-Literal außerhalb der Ströme dekodiert. In `docs/pruefung.txt`
  (neu erzeugt mit `make-preview.sh`) nennt die Rohsicht für den Namen seither
  auch die dekodierten Zeichenketten aus `/Title` und `/Author`. Belege:
  `zo_d_altgeneration_und_kodierung::zo_d2_altgeneration_stroeme_stilles_leck`,
  `…::zo_d3_altgeneration_zeichenketten_stilles_leck` und
  `zo_d_orakel_am_binary::zo_d11_altgeneration_am_binary_stilles_leck`, alle
  vorher absichtlich rot; `…::zo_d0_probenliste_filterkette_bleibt_gruen`
  hält fest, dass die Rohsicht an `[null /ASCII85Decode]` stehen bleibt wie
  die Objektsicht. Mutationsnachweis: Kette nicht gelesen → der erste und
  der dritte rot; Literale nicht gelesen → der zweite und der dritte rot.

* **⚠ Sicherheit: die Entpackgrenze galt nicht für den Vorspann einer Kette,
  die an einem Bildfilter endet** (Register #83, Nachtrag zu #64;
  Dienstverweigerung). Die Vorprüfung packte seit #64 jede Kette aus, deren
  Glieder alle auspackbar sind, und buchte jede andere weiter ganz roh —
  auch `[/FlateDecode /DCTDecode]`, ein JPEG, noch einmal mit Flate gepackt.
  Steht im Flate-Glied eine Bombe, entpackte der Bilddekoder des
  Schreibpfads sie ohne Grenze, um an die JPEG-Bytes zu kommen. Am Stand
  `72711d0` brauchte eine Datei von 3 MB mit 3 GiB Nullen im Flate-Glied am
  gebauten Binary 3,1 GB Spitze (`/usr/bin/time -v`), bei
  `--max-decompressed-mb 64`, und endete mit Rückgabewert 1 und der falschen
  Ursache „JPEG nicht dekodierbar“. Jetzt packt die Vorprüfung den
  auspackbaren Vorspann jeder Kette begrenzt aus und bucht ihn gegen das
  ganze Budget; die Bombe fällt dort mit der Budgetmeldung. Die
  Rohgrößen-Grenze der Altlast-Filter gilt dem Vorspann nicht:
  `[/ASCII85Decode /DCTDecode]` lief vorher roh durch und soll nicht erst
  jetzt fallen. Die Zeile der Probenliste zu #64 trat damit noch auf; sie
  trägt jetzt beide Belege. Für das Orakel heißt das: eine Datei, die die
  Vorprüfung mit demselben Budget durchlässt, bringt die Objektsicht nur noch
  dort ans Budget, wo die beiden Dekoder aus einem Strom verschieden viel
  holen; `zf_q2_teildekoder` hält seither die Ablehnung fest. Belege:
  `zo_e_vorspann_vor_bildfilter::ein_flate_vorspann_vor_dctdecode_faellt_am_budget`,
  `…::ein_echtes_jpeg_hinter_flate_bleibt_durchlaessig` und die Gegenprobe
  zur Altlast-Grenze in derselben Datei. Mutationsnachweis: Vorspann wieder
  roh gebucht → der erste rot; Altlast-Grenze auch am Vorspann → die
  Gegenprobe rot.

* **Abgelehnte gewöhnliche Datei: ein großes ASCII85-Bild fiel an einer
  Grenze, die nichts mehr schützte** (Register #82, Prüfer D). Die
  Vorprüfung lehnte jede Kette mit `LZWDecode` oder `ASCII85Decode` über einer
  festen Rohgröße ab — aus der Zeit, als `lopdf` diese Filter ohne Grenze
  auspackte. Seit #64 entpackt sie die Vorprüfung selbst, begrenzt auf das
  Budget; die Grenze lehnte nur noch Dateien ab, etwa ein Bild unter
  `/ASCII85Decode`, wie es Distiller mit ASCII-Ausgabe schreibt, und
  `--max-decompressed-mb` half nicht, weil sie eine Konstante war. Sie ist
  gefallen; die Datei wird geschwärzt. Ein LZW- oder ASCII85-Strom, der mehr
  entpackt, als das Budget hergibt, fällt weiter — jetzt mit der
  Budgetmeldung. Der zweite Teil des Befunds (TIFF-Prädiktor über
  Seitentext: das Orakel findet nichts) bleibt notiert: kein Erzeuger
  schreibt Seiteninhalt so. Beleg: `zo_d_orakel_am_binary`, der Test zum
  Befund D5 (bis dahin absichtlich rot und ignoriert). Mutationsnachweis:
  die Grenze wieder eingesetzt → rot.

* **⚠ Sicherheit: ein Spiegel, den Poppler findet, blieb stehen, wenn das
  Formular eigene Ressourcen ohne `/Properties` mitbringt** (Register #67,
  Prüfer C; mit Vorbehalt, weil die Datei die Norm verlässt). `/Span /MC0
  BDC` in einem Form-XObject mit eigenem `/Resources` löst sich nach PDF
  32000-1 nur dort auf; Poppler sucht trotzdem die Kette der Aufrufer hinauf
  und gibt den Spiegel aus der Seite als Text der Glyphen aus. Der Scan fand
  keinen Datensatz, die Liste in der Seite blieb mit dem Geheimnis stehen —
  ohne Warnung, mit Rückgabewert 0; `--check-leaks` an der Ausgabe fand sie.
  Jetzt löst sich ein Name, den das eigene Verzeichnis nicht kennt, in den
  Umgebungen der Aufrufer auf, von innen nach außen, und der Datensatz trägt
  den Eigentümer des Verzeichnisses, in dem er stand. Damit hängt das
  Ergebnis wieder an der Umgebung: der Spiegel-Scan eines solchen Stroms
  läuft je Umgebung, nicht je Strom — die Frage, ob ein Strom solche Namen
  trägt, wird je Strom einmal gestellt. Belege:
  `zo_c_spiegel_umgebungen::formular_mit_eigenen_ressourcen_ohne_properties_name_aus_der_seite`
  (bis dahin absichtlich rot und ignoriert) und Proben mit mehreren
  Umgebungen, auf verschiedenen Seiten und auf derselben. Mutationsnachweis: keine
  Auflösung außerhalb → der erste rot; Scan je Strom statt je Umgebung → die
  Probe auf einer Seite rot.

### Nach der Runde 9: ein Prüfer, der Windows heißt, und das Gate auf der Platte

Zwei Befunde außerhalb einer Gegenprüfung, jeder in seinem eigenen Commit —
der erste am Programm, der zweite am Werkzeug. Beide haben dieselbe Wurzel:
das lokale Gate führt keinen Windows-Test aus, und es passte nicht mehr auf
die Platte.

* **⚠ Sicherheit: auf einem Dateisystem ohne Groß-/Kleinschreibung gab der
  Kollisionsschutz der Oberfläche einer Datei zwei Kennungen.** Gefunden hat
  es der Windows-Job der CI mit
  `zm_b_verschiedene_dateien_verschiedene_kennungen` aus der Runde 9: der Test
  schreibt `Auszug.pdf` und `auszug.pdf`, liest beide zurück, und auf NTFS ist
  es dieselbe Datei — `writing_key` gab aber für jede Schreibweise eine eigene
  Kennung. Zwei gleichzeitige Exporte auf diese Datei hätten sich nicht mit
  `EXPORT_BUSY` abgewiesen, sondern beide geschrieben. Keine Regression: der
  Stand `0f0b0f7` kanonisierte genauso nur das Verzeichnis. Der Satz der
  Fix-Runde 8 unten, zwei Schreibwege auf dieselbe Datei fielen zusammen, galt
  nur auf Dateisystemen **mit** Groß-/Kleinschreibung; er ist dort ergänzt.

  Die Umkehrung wäre genauso falsch, und stiller: eine gemeinsame Kennung für
  zwei *verschiedene* Dateien lehnte nicht nur den zweiten Export ab — sie
  würfe in `start_export_check` die laufende Leckprüfung der ersten Datei weg
  und striche in `forget_warnings_of` deren Warnungen. NTFS lässt seit
  Windows 10 je Verzeichnis Groß/Klein zu, und ein VFAT-Stick, eine
  SMB-Freigabe oder ext4 mit `casefold` falten unter Linux. Deshalb rät
  `writing_key` nicht nach Plattform, sondern **misst das Verzeichnis**
  (`gemessene_schreibweise`): am nächsten vorhandenen Vorfahren des Ziels ein
  Eintrag mit einem ASCII-Buchstaben, dessen Schreibweise gekippt, und
  nachgesehen, ob die Platte darunter dieselbe Datei zeigt
  (`document::same_file`, jetzt öffentlich). Nichts wird angelegt, geprüft
  werden höchstens acht Einträge, und ohne Antwort — leeres Verzeichnis, keine
  Buchstaben, kein Leserecht — gilt die Voreinstellung der Plattform als
  Notnagel. Die Faltung selbst (`gefalteter_name`) ist rein und nimmt die
  Antwort als Parameter; so ist die Windows-Hälfte auf Linux gebunden
  (`zn_b_schreibweise_tests`), und die Messung wird gegen dasselbe Orakel
  geprüft wie im CI-Test: was die Platte sagt, muss sie sagen. Der
  Windows-Läufer der CI ist damit der einzige, der den positiven Zweig der
  Messung sieht. Offen, als Register: ein NTFS-Verzeichnis mit gesetzter
  Case-Flagge hat in dieser CI keinen Läufer, und für den noch nicht
  angelegten Rest eines Verzeichnispfads behält `resolved_dir` die getippte
  Schreibweise. `writing_key` ist öffentlich, und die Belege rufen das
  Original — der Nachbau in `zm_b_kennung_der_ausgabedatei` samt seinem
  Anker-Test ist weg.

* **Das Gate passte nicht mehr auf die Platte — jedes Testbinary trug die
  volle Debug-Information aller Abhängigkeiten.** Der Prüfrechner hat eine
  feste Plattenzuteilung; `cargo test --workspace` brach darin zweimal mit „No
  space left on device“ ab, einmal mitten in einen Schreibvorgang. Gemessen,
  nicht geraten: `target/` belegte vorher 19 GB, und ein GUI-Testbinary
  (`zm_b_gleichzeitige_exporte`) war vorher 240,7 MB groß, davon 219,4 MB in
  `.debug_*`-Sektionen — 91 %. Die Workspace-`Cargo.toml` hatte nur
  `[profile.release]`; der Knopf war unbenutzt. Jetzt steht dort
  `[profile.dev] debug = "line-tables-only"` — `dev`, nicht `test`, weil
  `cargo test` die Abhängigkeiten mit `dev` baut und `test` von `dev` erbt.
  Dasselbe Binary ist an den gebauten Binaries danach 78,0 MB groß, davon
  58,3 MB `.debug_*`, und `target/` belegt nach `cargo clean` und dem ganzen
  Gate 8,0 GB. Backtraces nennen weiter Datei:Zeile.

  Und eine Rücknahme: der Commit `1282b16` nannte den Knopf als Hebel, ließ
  ihn aber liegen, weil er „gebundene Debug-Messzahlen verschieben“ könne. Das
  war falsch. `debug` steuert nur die DWARF-Ausgabe; `opt-level`,
  `debug-assertions` und `overflow-checks` bleiben, `.debug_*`-Sektionen sind
  nicht `SHF_ALLOC` und nie im Arbeitsspeicher, und kein Test liest einen
  Backtrace. Keine gebundene Zeit- oder Speicherzahl hängt an der Stufe.
  `zn_a_debug_info_stufe` bindet beides: die Zeile im Manifest und die
  Wirkung — die Größe des Binaries, in dem der Test selbst läuft, gegen eine
  Decke. In `CONTRIBUTING.md` steht dazu die Regel, die aus beiden Befunden
  dieses Abschnitts folgt: ein Befund, ein Commit, und nach jedem Push wird
  der Windows-Job angesehen.

* **Die Schleife hat jetzt einen Gegenstand, an dem sie endet.** Die
  Fix-Runden hatten drei Aufgaben in einer Runde — Schwärzung, Prüfapparat,
  Doku —, und die konvergieren verschieden; die Runde 9 hatte alle drei in
  einem Commit. Jetzt sind es drei Spuren, und nur die Schwärzung zählt gegen
  das Ende. Das Ende selbst ist keine Abwesenheitsbehauptung mehr („nichts
  gefunden“), sondern eine Checkliste: [`PRUEFLISTE.md`](PRUEFLISTE.md) nennt
  jede Fehlerklasse, die in dieser Schleife aufgetreten ist, mit der Runde
  ihres ersten Auftretens und den Belegdateien, die sie festhalten;
  `zn_c_probenliste` prüft, dass es jede genannte Datei gibt. Die Bedingung,
  das Mandat der letzten Runden und die Regeln zur Arbeitsweise stehen in
  `CONTRIBUTING.md`, „Drei Spuren, eine Probenliste, ein Ende“.

### Fix-Runde 9: was die Gegenprüfung der Runde 8 noch fand

Vier Gegenprüfer lasen die Korrekturen der Runde 8 mit eigenem Material gegen.
Was die Runde 8 getragen hat, steht zuerst, weil es den Rest einordnet: jede
ihrer Korrekturen wurde einzeln zurückgenommen, und jedes Mal wurde der
richtige Test rot; jede Zahl des geprüften Blocks wurde einzeln geändert, und
keine blieb unbemerkt. Was sie nicht getragen hat, steht hier.

* **⚠ Sicherheit: ein zweiter Name desselben Bildes zeigte ungeschwärzte
  Bildpunkte mitten im Schwärzungsrechteck.** Die Runde 8 hat den *Spiegel*
  daran ausgerichtet, dass `repoint_page` genau einen Namen setzt — die
  *Bildpunkte* nicht. Lag ein Bildobjekt auf einer Seite unter zwei Namen unter
  der Schwärzung, wurde nur der erste umgebogen; der zweite zeigte weiter das
  unversehrte Original. Das Programm meldete Erfolg, und keine Warnung stand
  daneben. Gefunden nicht am Bericht, sondern an den Bildpunkten der
  Ausgabedatei. Jetzt trägt die Arbeit die Menge der getroffenen
  **(Strom, Name)**-Paare, es wird jedes davon umgebogen, und der Vermerk fällt
  **nach** dem Schreiben — denn wohin die Bildpunkte kommen, weiß nur der, der
  sie geschrieben hat.

* **⚠ Sicherheit und Dienstverweigerung: es waren drei Flächenfragen, nicht
  eine.** Der Vorfilter der Seite und der Sammler der Inline-Bilder fragten die
  **Hülle** einer Platzierung mit strengen Vergleichen, das Füllen ihre
  **Pixelzelle** mit Berührung als Treffer. Am Rand gingen die Antworten
  auseinander, und zwar in beide Richtungen: eine Seite, deren Platzierung eine
  Zone nur berührte, wurde ganz übersprungen — still, ohne Warnung, mit den
  Bildpunkten in der Datei; und ein Inline-Bild, dessen Fläche eine Zone nur
  berührte, verlor seine Rohdaten und wurde trotzdem aufgenommen, worauf der
  Lauf mit einem Fehler und **ohne Ausgabedatei** endete. Eine Grenze, die eine
  gewöhnliche Datei ablehnt, ist hier genauso ein Fehler wie eine Lücke. Jetzt
  fragen alle drei dasselbe Viereck; die Hülle ist im Bildlauf abgelöst.

* **⚠ Sicherheit: wer das Fenster schloss, während die Nachprüfung lief, wurde
  nicht gefragt.** Die Rückfrage kannte den laufenden **Export** — dort
  entsteht noch gar keine Datei — und nicht die laufende **Nachprüfung**, wo
  die Ausgabedatei schon auf der Platte liegt und das Urteil ihr einziger Zeuge
  ist. Die Fälle standen damit verkehrt herum. Jetzt wird auch dann gefragt,
  mit einem eigenen Satz: verloren geht nicht die Datei, sondern das Urteil
  über sie.

* **Eine Kennung je Export, und zwar vor und nach dem Anlegen des Ordners
  dieselbe.** Die Kennung löste das Verzeichnis auf — nur gibt es das beim
  Klick noch nicht, wenn der Ordner erst beim Schreiben entsteht. Derselbe
  Klick ergab dann zwei Kennungen, und „es wurde keine Datei geschrieben" blieb
  neben der Erfolgsmeldung stehen. Jetzt wird erst `.`/`..` lexikalisch
  weggerechnet und dann der längste vorhandene Teil aufgelöst; der Abschluss
  des Exports bekommt die Kennung des Klicks gereicht, statt eine zweite zu
  rechnen.

* **Eine Zusage in Fettschrift ist eine Zahl.** Der Wächter über die Zahlen der
  Doku ließ Ordnungszahlen und „ein/eine/einen" mit Absicht aus — im Fließtext
  sind sie ein Platz in einer Liste oder der unbestimmte Artikel. Genau dort
  standen aber die Zusagen der Runde 8: „als **erste** Anweisung", „genau
  **einen** Namen", „**eine** Stelle". Jede ließ sich umdrehen, ohne dass ein
  Test es merkte. Jetzt zählt ein Zahlwort in einem `**…**`-Lauf als Zahl; das
  Sternchen unterscheidet den Artikel von der Behauptung. Dazu: „null" und
  „eins" gehören in die Reihe, und eine zusammengesetzte Zahl verlangt ein
  Zahlwort vor der Endung — „zweihundert" ja, „Jahrhundert" nein.

* **Zwei Helfer der Doku-Wächter waren zu grob.** Die Satztrennung schnitt
  nicht hinter „… Rückgabewert 3. Dieselbe Datei …", weil vor dem Punkt eine
  Ziffer stand; zwei Sätze verschmolzen, und ein Weg im ersten deckte eine Zahl
  im zweiten. Und der Schnitt des Abschnitts endete an der ersten Zeile, die
  mit `## ` beginnt — auch mitten in einem Codeblock, was den geprüften Bereich
  still verkürzt hätte.

* **Eine Lockerung aus der Runde 8, zurückgenommen.** Der Wächter über die
  Herkunft einer Messzahl hatte sechs neue Merkmale bekommen; keines kam im
  geprüften Block vor, und eines war falsch: eine Kommandozeile im Satz zu
  **erwähnen** ist nicht dasselbe, wie die Zahl von dort zu haben. Sie sind
  gestrichen. Was bleibt, steht jetzt einzeln mit dem Satz, der es auslöst, und
  mit dem, der es nicht auslösen darf.

* **Die Doku beschrieb sich im Zustand vor der Korrektur.** `image.rs` sagte an
  zwei Stellen weiter, die Prüfung hänge an den *Ecken* jeder Pixelzelle. Und
  der Abschnitt der Fix-Runde 3 sagte im Präsens, der Export laufe weiter im
  Zeichentakt der Oberfläche, und kündigte den Umbau als kommenden Schritt an —
  im selben Abschnitt `## Unveröffentlicht`, aus dem die Release-Notizen
  geschnitten werden. Der Schritt ist in der Runde 8 getan.

* **Drei Belegdateien der Prüfer hielten Kopien der Helfer, die sie prüfen.**
  Eine Kopie prüft ihr eigenes Abbild, sobald das Original sich bewegt: sie
  blieb rot, nachdem das Original schon stimmte, und hätte ebenso gut grün
  bleiben können, während es falsch ist — dieselbe Scheintest-Klasse, gegen die
  dieses Projekt seit der Runde 3 arbeitet, diesmal auf der Prüferseite. Wo
  eine Kopie bleiben muss (der Helfer ist privat), hängt sie jetzt an einem
  Test, der den Quelltext des Originals liest.

### Fix-Runde 8: was die Gegenprüfung der Runde 7 noch fand

Fünf Gegenprüfer lasen die Korrekturen der Runde 7 mit eigenem Material gegen.
Was blieb: ein stilles Leck an der Oberfläche, ein Bildbefund, der zwischen Leck
und Fehlalarm hin und her ging, bis er dreimal gedreht war, zwei Wächter über der
Doku, die ihre eigene Regel nicht hielten, und eine Menge, die dezimal gerechnet
dastand, wo derselbe Abschnitt die Einheit binär festlegt.

* **Der Export selbst läuft auf einem eigenen Faden.** Bis hierher hielt er die
  Oberfläche an, solange er schrieb; jetzt nicht mehr. Dazu gehören eine
  gesperrte Bedienung, während er läuft, die Ablehnung eines **zweiten**
  Exports derselben Datei, und eine Meldung für den Fall, dass der Faden
  abstürzt: „es wurde keine Datei geschrieben" — denn geschrieben wird am Ende
  in einem Zug, es gibt also keine halbe Datei, wohl aber keine. Die drei
  Punkte darunter sind die **Fehler** dieses Umbaus, gefunden von der
  Gegenprüfung; hier steht der Umbau selbst, ohne den sie nichts bedeuten.

* **⚠ Sicherheit: die Oberfläche zeigte nach einem Export das Urteil über die
  vorigen Bytes.** Die Nachprüfung nach dem Export steht seit der Runde 4 in
  einem eigenen Faden, ihr Ergebnis kommt über einen Kanal zurück. Wer in der
  Zwischenzeit ein zweites Mal exportierte, sah das Urteil des ersten Laufs über
  der neuen Datei — „keine Fundstellen“ über Bytes, die niemand gelesen hatte.
  Jetzt räumt `poll_exports` als **erste** Anweisung von `export_to` die Urteile
  über alte Bytes weg. Die Kennung eines Exports ist dabei nicht mehr der Pfad,
  sondern das aufgelöste Verzeichnis samt unverändertem Dateinamen
  (`writing_key`) — sie folgt keinem Symlink, und zwei Schreibwege auf dieselbe
  Datei fallen zusammen. Das galt auf einem Dateisystem mit
  Groß-/Kleinschreibung; ohne sie waren es bis zum Befund des Windows-Jobs
  nach der Runde 9 zwei Kennungen (siehe oben). Und ein fertiger Export meldet
  nicht mehr
  „beschäftigt“: das Merkmal dafür ist, dass die Ausgabedatei entstanden ist,
  nicht dass der Lauf fehlerfrei endete.

* **⚠ Sicherheit: der Ersatztext über einem Bild hing an einer Schätzung, und
  die Schätzung war in beide Richtungen falsch.** Ob ein Spiegel über einer
  Bildplatzierung fällt, entschied die Frage „liegt das Rechteck auf der Fläche“
  — nicht die Frage, ob dort Bildpunkte gefallen sind. Beides ging schief. Bei
  einem gedrehten Bild ist die **Hülle** größer als das Bild, und in ihren Ecken
  liegt gar kein Bildpunkt: eine Schwärzung dort nahm einem unversehrten Bild
  seinen Ersatztext, ohne dass ein Byte des Bildes sich änderte (Fehlalarm). Und
  die Prüfung je Pixelzelle sah nur deren **Ecken**: ein Schwärzungsrechteck
  ganz zwischen den Gitterlinien eines groben, groß gezogenen Bildes traf keine,
  die Bildpunkte blieben in der Datei, und es gab keine Warnung (Leck). Jetzt
  entscheidet **eine** Flächenfrage, gestellt mit trennenden Achsen und nicht
  mit Ecken (Berührung zählt als Treffer, ein `NaN` ebenfalls); der
  Kandidatenfilter fragt das **Viereck** der Platzierung statt ihrer Hülle; und
  `crate::image` berichtet je Platzierung, ob dort Bildpunkte fielen, statt den
  Aufrufer schätzen zu lassen. Die Korrektur hat drei Anläufe gebraucht: der
  erste schloss das Leck und öffnete den Fehlalarm, der zweite schloss den
  Fehlalarm und öffnete das Leck wieder. Der Grund steht als Lehre daneben — die
  Wahrheit über gefallene Bildpunkte liegt in `image.rs`, und wer sie eine
  Ebene weiter vorn nachbildet, bildet sie falsch nach.

* **⚠ Sicherheit: ein zweiter Name desselben Bildes behielt seine Bildpunkte und
  verlor trotzdem seinen Spiegel.** Vermerkt wurde nach Objekt-Id, umgebogen
  wird nach Name: hängt ein Bild an mehreren Seiten, entsteht eine Kopie, und
  `repoint_page` setzt genau **einen** Namen. Zeichnete dieselbe Seite dasselbe
  Objekt unter einem zweiten Namen, zeigte der weiter das unversehrte Original —
  und verlor doch seinen Ersatztext. Fehlalarm und Leck in einer Datei: der
  Spiegel weg, die Bildpunkte sichtbar. Jetzt beantwortet **eine** Stelle die
  Frage, wohin die geschwärzten Bildpunkte kommen, und die beiden, die sie
  brauchen — der, der schreibt, und der, der vermerkt —, halten sich an dieselbe
  Antwort.

* **Ein Bild, das sich nicht dekodieren lässt, nimmt nur der getroffenen
  Platzierung den Ersatztext.** Dort fällt kein Bildpunkt; das Objekt wird weder
  überschrieben noch kopiert. Eine unberührte zweite Platzierung desselben
  Bildes zeigt danach buchstäblich dasselbe wie vorher, ihr Spiegel ist wahr,
  und sein Verlust hätte keinen Gegenwert. Das `/Alt` am **Bilddictionary**
  fällt weiter für alle Platzierungen — es hängt an der Objekt-Id und ist nicht
  je Platzierung zu haben; diese Über-Schwärzung steht neben einer Warnung und
  ist der Preis von `--allow-undecodable-images`.

* **Der Ort einer Fundstelle ist maschinenlesbar geworden.** Bis hierher war er
  nur Satz, und die Oberfläche konnte „gewollt stehen geblieben“ nicht von
  „Schwärzung danebengegangen“ unterscheiden. Jetzt kommt zu jeder Fundstelle
  ein Ort: die Sicht, die Seite (wo eine Sicht eine kennt) und die Objekt-Id (wo
  sie eine kennt) — samt ihrer Herkunft, damit niemand eine geratene Seite für
  eine gelesene nimmt.

* **Sechs Teststellen behaupteten den Ausgang eines Programms, das gar nicht
  gelaufen war.** Wo eine Prüfung eine benannte Pipe braucht, war der Ausweg
  „diese Umgebung kann das nicht“ nur für einen Startfehler vorgesehen; ein
  `mkfifo`, das startet und mit einem Fehler endet (seccomp, ein
  BusyBox-Wrapper, ein Dateisystem ohne Pipes), lief in eine Behauptung. Jetzt
  übergeht jede der sechs Stellen den Fall und schreibt eine Zeile auf die
  Fehlerausgabe. `cfg(unix)` sagt, dass es benannte Pipes **gibt** — nicht, dass
  dieser Rechner sie anlegen lässt.

* **Die Plattformregel konnte sich selbst aushebeln.** Sie verlangt, dass jede
  Stelle, die eine Einrichtung des Systems nennt, unter einem `cfg` steht oder
  ihr Ergebnis in derselben Anweisung als möglicherweise fehlend behandelt; was
  offen ist, steht namentlich in einer Liste, und die muss genau aufgehen. Nur:
  eine Verletzung, die den **Build** für Windows bricht, durfte in dieser Liste
  geparkt werden. Dann war der Regeltest grün, während
  `cargo clippy --target x86_64-pc-windows-gnu` an derselben Zeile mit `E0433`
  abbrach — die Liste sagte „bekannt“, und niemand sagte „rot“. Solche
  Verletzungen gehören jetzt nicht mehr in die Liste, sondern behoben. Außerdem
  verglich die Zuordnung einer Stelle zu ihrem Listeneintrag Pfade mit dem
  Trennzeichen des Wirtssystems: unter Windows fand sie ihren eigenen Eintrag
  nicht.

* **Zwei Wächter über der Doku hielten ihre eigene Regel nicht.** Der eine liest
  jeden Satz des geprüften Blocks und verlangt, dass eine Zeit- oder
  Speicherzahl sagt, woher sie kommt — er nahm dafür aber ein **Profil**
  („Release“) als Ort an und hätte damit genau das durchgelassen, was die Regel
  ausschließt: eine im Testprozess erhobene Zahl als Zahl des gebauten Binaries.
  Der andere prüft die Gegenrichtung, dass im Block keine Zahl unbedeckt
  dasteht — seine Wortliste endete bei „zwölf“ und hatte „siebzehn“ von Hand
  nachgetragen, wuchs also dort, wo jemand hinsah. „dreizehn“, „zwanzig“,
  „hundert“, „tausend“ und „Dutzend“ waren für ihn keine Zahlen. Die Reihe steht
  jetzt vollständig da, und die `…mal`-Formen entstehen aus ihr statt aus einer
  zweiten Liste.

* **Eine Menge stand dezimal gerechnet da, wo derselbe Abschnitt die Einheit
  binär festlegt.** Wie viel jedes Muster der alten Suche an einer Datei mit
  einem 64-MiB-Strom durchläuft, stand als Dezimalwert — derselbe Fehler, den
  der Block seiner Nachbarzahl ausdrücklich anschreibt. Die Zahl wird jetzt aus
  der Zahl der Blöcke und der Stromgröße **abgeleitet** und nicht mehr
  abgeschrieben, und beide Stellen, die sie nennen, werden gegeneinander
  gehalten.

### Fix-Runde 7: was die Gegenprüfung der Runde 6 noch fand

Fünf Gegenprüfer lasen die Korrekturen der Runde 6 mit eigenem Material gegen.
**Vier stille Lecks** blieben: ein Form-XObject ohne eigenes `/Resources`,
dasselbe Formular unter zwei Grafikumgebungen, ein `/Filter`-Wert, der gar kein
Name ist, und sechs Klartextträger am Beiwerk einer Annotation. Dazu zwei
Dienstverweigerungen — `BDC`-Klammern × Textoperationen ohne jede Decke, und eine
Decke, die je Seite statt je Dokument zählt (1 000 Seiten aus einer Datei von
224 752 Byte belegten beim Schwärzen 6 288 MB, ungedeckelt am Stand `308ef38`) —,
acht Befunde an der Oberfläche und die Erkenntnis, dass von 76 einzeln mutierten
Zahlen der Doku weiter 58 grün blieben.

* **Jetzt ist jede Zahl gebunden, nicht nur die, an die jemand dachte.** Die
  Fix-Runde 6 hat die Messzahlen der Doku an einen Testdatensatz gebunden — und
  die Gegenprüfung mutierte danach 76 Stellen einzeln: 18 wurden rot, **58
  blieben grün**. Der Test prüfte nämlich, dass jeder Satz **seiner Liste** in
  der Doku steht, und sagte nichts darüber, ob die Liste vollständig ist.
  `jede_zahl_der_letzten_runden_ist_gebunden` dreht die Frage um: er schneidet
  den Block der beiden jüngsten Fix-Runden aus dieser Datei, markiert, was die
  gebundenen Sätze davon abdecken, und verlangt, dass **keine** Zahl übrig
  bleibt. Was keine Messzahl ist — die Nummer einer Runde, ein Dateiname, die
  Nummer einer Norm —, steht mit Begründung in einer Ausnahmeliste, wie bei den
  Warnungen in `coverage.rs`. Dazu kommen Zahlen aus dem Code statt aus der
  Abschrift: die Decke der Spiegel-Zuordnungen, die Tiefe der Objektsicht, die
  Zahl der benannten Stellen und die Zahl der Filterketten stehen nur noch an
  je einer Stelle.
* **Sechs Zahlen in der Doku waren falsch, alle nachgemessen.** „0,56 s und
  38 MB“ für die entschärfte Spiegel-Bombe stammten aus dem **Testprozess**
  (Debug), der nur den Extraktor fährt, nicht aus einem Lauf des Binaries; am
  gebauten Binary sind es 0,15–0,18 s und 40 MB (Release) bzw. 1,58–1,68 s und
  52 MB (Debug),
  beide mit Rückgabewert 3. Dieselbe Datei hat **neun** Objekte, nicht elf. Die README
  versprach „bis zu 10 000 Muster“ für 1 000 Begriffe und zählte im selben Satz
  elf auf — `Probe::new` legt bis zu zwölf je Begriff an, also 12 000.
  `SECURITY.md` begründete die Decke von drei genannten Stellen mit „51 Zeilen“,
  während der Quelltext daneben 154 ausrechnet. Über einer Tabelle mit sechs
  Filterketten stand „fünf Ketten“, und die sechste — der Bildfilter **am
  Anfang** der Kette, gerade der Fall, den die Fix-Runde 6 neu zusagte — hing an
  keinem Lauf; jetzt fährt `zg_r5_filterketten` jede Zeile durch das Binary —
  und dieser Lauf widerlegte die Zusage sofort (siehe den Eintrag zum Bildfilter
  am Kettenanfang). Und
  der Spitzenspeicher des Orakels stand in Dezimal-MB, obwohl dasselbe Dokument
  MB als 1024² Byte festlegt. Dazu zwei Sätze, die der Befund derselben Runde
  widerlegt hatte und die trotzdem stehen geblieben waren: der Spitzenbedarf
  einer pfadlastigen Seite sei „der des `Operation`-Vektors und von
  `--max-parsed-mb` gedeckelt“, und eine Aufzählung im Metadaten-Abschnitt, die
  nach einem Punkt kleingeschrieben weiterlief.
* **Die Kopie der Deckenwarnung in `redact-pipeline` hing an nichts.**
  `coverage.rs` hält den Wortlaut jeder Warnung noch einmal, um ihre Einordnung
  festzuhalten — die Gegenprüfung änderte in der Kopie `100000` in `200000`, und
  kein Test wurde rot. Er konnte es auch nicht: `is_coverage_gap` antwortet für
  jede **nicht** gelistete Warnung „Deckungslücke“, also auch für eine
  verstümmelte Kopie. Aus den Kopien sind Schablonen geworden: die festen Teile
  müssen wörtlich im Quelltext von `redact-pdf` stehen, und eine Zahl wie
  `MAX_FORM_DEPTH` wird von dort gelesen statt abgeschrieben. Die Kopien selbst
  hielten der Prüfung stand; neu ist, dass es jemand prüft.
* **Die Regel gegen Plattformzusagen hielt nur halb.** Neun Proben, einzeln an
  den Baum gehängt: ein Fehlalarm (`"/proc/self/status"` in einem
  **Blockkommentar**) und sechs Lücken — ein fremdes `.ok()` acht Zeilen weiter
  genügte als Ausweg, `Path::new("/proc")` ohne Schrägstrich fiel durch,
  ebenso `"C:\Windows\…"`, `"/etc/localtime"`, ein `std::os::unix`-API ohne
  `cfg` und `Command::new("mkfifo")`. Der Ausweg zählt jetzt nur in **derselben
  Anweisung**, Kommentare sind wirklich ausgenommen, und ein fremdes Programm
  ist eine Einrichtung des Systems: `#[cfg(unix)]` sagt nichts über den `PATH`,
  und auf einem schlanken Unix-Bild ohne `util-linux` panickt ein Test, statt
  sich mit einem Hinweis zu begnügen. Neun Stellen im Baum verletzten die
  geschärfte Regel; **drei davon hätten den Windows-Lauf gebrochen und sind
  behoben**: zweimal ein Unix-API **ohne** `cfg` in einem
  `#[cfg(test)]`-Modul der Oberfläche — ein `symlink` — das bricht nicht erst zur Laufzeit,
  sondern schon den Bau, und der dortige CI-Job fährt `cargo clippy
  --workspace --all-targets` und `cargo test --workspace` — und einmal ein
  `/proc/self/status` mit `.expect(…)` ebendort, genau der Laufzeitfehler, an
  dem der Windows-Job schon viermal rot war. Der Symlink
  steht jetzt unter `#[cfg(unix)]`, der Messhelfer samt seinem Test unter
  `#[cfg(target_os = "linux")]`. Die sechs `mkfifo`-Stellen bleiben als
  benannte Altlast in der Liste: sie hängen an `cfg(unix)`, scheitern aber am
  `PATH`, und das bricht kein Windows, sondern ein schlankes Unix-Bild ohne
  `util-linux`.
* **`--check-leaks` zählte Zeilen, nicht Stellen.** „52 Stelle(n) nicht geprüft“
  stand unter 50 einzeln genannten Zeilen, einer Summenzeile „7 weitere“ und
  einer über 57 Ströme — die Zahl war kleiner als das, was darüber stand. Gezählt
  wird jetzt `LeakCheck::unchecked_places`, und der Satz sagt, dass eine
  Summenzeile den Rest zusammenfasst, wenn es sehr viele sind.

* **Ein Formular ohne eigenes `/Resources` ließ seinen Spiegel stehen.** Ein
  Form-XObject darf das Ressourcenverzeichnis der Seite erben. Tat es das,
  wurde der Textspiegel darüber gelesen, im Strom geleert und gemeldet — der
  Klartext blieb aber im Verzeichnis der **Seite** stehen, wo niemand ihn
  suchte: Rückgabewert null beim Schwärzen, danach findet `--check-leaks` die
  IBAN. Geleert wird jetzt am Fundort statt am vermuteten Ort, und zwar in
  jeder Lage, die die Gegenprüfung baute: geerbte Liste über mehrere
  `/Pages`-Ebenen, direkt in der Seite stehende Liste, geteiltes
  `/Resources`-Objekt (dort nur der getroffene Eintrag, die Nachbarseite
  bleibt unberührt), `/Properties` als Verweis, Kachelmuster mit eigenem
  Verzeichnis, und eine Zweitfassung, die einmal als Verweis und einmal direkt
  steht — beide fallen. Dasselbe Formular unter mehreren Grafikumgebungen wird
  jetzt in jeder abgelaufen; dass nur die erste geprüft wurde, war eine
  Regression aus der Fix-Runde 6, die den Scan je Strom statt je Platzierung
  laufen ließ und die Umgebung dabei vergaß.
* **Zwei Wege zur Dienstverweigerung, beide gedeckelt.** `BDC`-Klammern über
  Textoperationen zählten gegen **keine** Decke: 6 000 Klammern über 6 000
  `Tj` aus einer Datei von 263 475 Byte ergaben 36 000 000 Zuordnungen. Und
  die Decke galt je Seiten-Scan, obwohl mehrere Seiten auf denselben
  `/Contents`-Strom zeigen dürfen — jede Seite zahlte sie voll aus, und der
  Redaktor hielt die Spiegel aller Seiten bis zum Ende fest. Drei Zähler
  tragen jetzt zusammen eine gemeinsame Decke je Seiten-Scan, keiner
  verbraucht den anderen; und weil die Kosten nicht im Scan, sondern beim
  Festhalten der zurückgestellten Spiegel anfielen, steht die dokumentweite
  Decke dort (`MAX_DEFERRED_MIRRORS`).
  Zurückgestellt wird außerdem nur noch, was die späte Frage braucht — die
  Objekt-Ids der Formulare, einmal je Formular statt je Platzierung, ohne
  deren Pfade und ohne die Eigenschaftsliste, die den Spiegeltext trägt. Der
  Scan bleibt seitenweise, weil `scan_page` öffentlich und seitenweise ist:
  dokumentweit zu zählen hieße, dieselbe Seite je nach ihren Nachbarn zu
  warnen oder nicht.

  Nachgemessen am Baum dieser Runde: mit der Decke bleibt dieselbe Datei bei
  100 000 Zuordnungen und einer Warnung; `scan_page` braucht dafür 0,30–0,31 s
  und die Spitze liegt bei 36 MB — gemessen im Testprozess (Debug), weil
  `scan_page` im Extraktor liegt und die Kommandozeile es nicht herausgibt.
  Am gebauten Binary (`target/release/redact-rs`, Release) kostet dieselbe
  Struktur als Datei von 263 699 Byte 0,11–0,13 s und 35 MB Spitze, und der
  Lauf endet mit Rückgabewert 3 und sieben ungeprüften Stellen; die Decke
  selbst steht dabei auf der Konsole. Und mit ihr kosten die 1 000 Seiten aus
  224 752 Byte am gebauten Binary 24,7–24,9 s und 86 MB statt 6 288 MB, bei
  genau 100 000 zurückgestellten Abschnitten — der dokumentweiten Decke, an
  der es noch still bleibt. Diese Spitze trägt Extraktor und Redaktor
  zusammen, weil die Kommandozeile die beiden nicht trennt.

  Die Zeit und die Spitze am Binary kommen von `/usr/bin/time -v`
  (`Maximum resident set size`), das Material von
  `zg_r1_decke::schreibt_material` über `R1_OUT` — die Datei der zweiten
  Messung ist dabei bytegleich zu der jener Messung. Die 6 288 MB dagegen
  beschreiben den ungedeckelten Stand `308ef38` und sind an diesem Baum nicht
  mehr zu messen.
* **Ein `/Filter`-Wert, der gar kein Name ist, machte das Orakel stumm.**
  `null`, eine Zahl, eine Zeichenkette, ein Verweis ins Leere — für `lopdf`
  ist das alles „ungefiltert", und das Orakel gab über LZW-gepacktem Klartext
  eine Entwarnung. Die kleinste Änderung, die das schließt: ein Glied, das
  sich nicht zu einem Namen auflöst, steht als **leerer** Name in der Kette.
  Ein leerer Name ist nie ein bekannter Filter, also bleibt die Kette dort
  stehen wie an jedem anderen unbekannten Namen — die Stelle kommt in die
  Liste „nicht geprüft", und der Rückgabewert sagt es. `/Filter null` dagegen
  ist gar kein Filter und bleibt es.
* **Der Bildfilter am Anfang einer Kette war ein Leck, keine Ausnahme.** Die
  Fix-Runde 6 hatte zugesagt, die Bildfilter-Ausnahme gelte am
  stehengebliebenen Glied. Der Lauf jeder Tabellenzeile durch das Binary
  widerlegte das sofort: ein Packfilter **hinter** einem Bildfilter wurde
  stillgeschwiegen, obwohl die Rohsicht den gepackten Text findet. Gemeldet
  wird jetzt, wenn ein Bildfilter vor einem Packfilter steht; allein am Ende
  der Kette bleibt er der benannte blinde Fleck.
* **Klartextträger, die den Metadatenlauf überstanden**, keiner davon benannt:
  ein `/FileAttachment`, das nur ein `/Popup` oder eine Antwort (`/IRT`) am
  Leben hielt; eine zugeordnete Datei (`/AF`) am Katalog, an der Seite oder an
  einer direkt eingebetteten Annotation — der Weg, über den ZUGFeRD seine
  Rechnung einbettet; XMP an einem Bild-XObject; `/PieceInfo` an einem
  Form-XObject; eine dreidimensionale Annotation mit ihrem Startskript; und
  das `/RO` einer `/Redact`-Annotation. Alle fallen jetzt, und zwar so, dass
  die Datei gültig bleibt: die Annotation behält ihren Schlüssel, die
  Vermessung ihr Erscheinungsbild bytegleich, nur der Klartext ist weg.
  Gemessen am ehrlichen Orakel, nicht am Bericht des Programms.
* **Der Bericht meldete Entfernungen, die keine waren.** Gebucht wurde die
  erste statt der aufgelösten Objekt-Id, wodurch eine Verweiskette eine
  Entfernung über einen Text meldete, den `--check-leaks` danach noch fand;
  und ein Ebenenname als Verweis fiel, während die Konsole „nichts zu
  entfernen" druckte. Gezählt wird jetzt am aufgeräumten Dokument: gefallene
  Schlüssel auf dasselbe Objekt zählen je Schlüssel, ein zweiter Halter
  verhindert die Meldung, und was entfernt aber nicht sicher zuzuordnen ist,
  bleibt bewusst zu klein — das ist die erlaubte Richtung.
* **Die Oberfläche verschenkte eine sichere Entwarnung und gab einen harten
  Fehlalarm.** Verschwundene Schreibweisen wurden nicht gezählt, obwohl
  „nirgends mehr gefunden" die belastbarste Aussage ist, die diese Prüfung
  treffen kann; sie steht jetzt als eigene Zahl im Satz. Eine Trefferzeile zu
  **löschen** (Entf) ist derselbe Wunsch wie sie abzuwählen — die Nachprüfung
  hielt es für ein Leck und meldete den Text als noch in der Ausgabe stehend.
  Gemerkt wird die Löschung jetzt an der Kennung der Region und nicht an ihrem
  Text, damit ein Rückgängig sie wirklich aufhebt. Und die Statuszeile war mit
  den echten Stellen eines Laufs 1 005 Zeichen lang — gemessen im Testprozess
  (Debug), weil die Statuszeile der Oberfläche gehört und am gebauten
  `redact-rs` nicht entsteht; eine Zeichenzahl hängt anders als eine Zeit nicht
  am Profil. Länger als jede Zeile, die noch gelesen wird, war sie, weil die
  Fix-Runde 6 nur einen Bestandteil gekürzt
  hatte: gekürzt wird jetzt die ganze Zeile, hinten — die sichere Richtung,
  denn der Fund steht vorn — auf höchstens 400 Zeichen, und was nicht mehr
  hineinpasst, steht vollständig in den Warnungen.
* **Der Prüfer der Plattformregel war selbst plattformabhängig.** Das lokale
  Tor war grün, die CI rot — und zwar nur auf Windows, nur im Testschritt, und
  ausgerechnet an `keine_systemeinrichtung_ohne_cfg_oder_ohne_ausweg`. Er
  meldete alle verbliebenen `mkfifo`-Stellen als **neue** Verletzung, obwohl sie
  namentlich in seiner Altlast-Liste stehen. Der Grund: `Path::display`
  schreibt unter Windows Backslashes, die Liste nennt ihre Dateien mit
  Schrägstrichen, und verglichen wurde ohne Vereinheitlichung. Der Prüfer machte
  damit genau die Annahme, die er allen anderen verbietet — und das ist der
  Grund, aus dem ein Tor, das Windows nur übersetzt statt es zu fahren, keine
  Zusage über Windows machen kann. Vereinheitlicht wird jetzt vor dem
  Vergleich, in einer eigenen Funktion, die der Prüfer und der neue Test
  `ein_pfad_mit_backslashes_findet_seine_altlast` gemeinsam benutzen; der Test
  baut die Windows-Schreibweise selbst und läuft deshalb auf jedem System.
  Mutation (die Vereinheitlichung entfernt): genau dieser Test rot, kein
  anderer.
* **Der Ort der Messung, nachgeholt — und eine Zahl, die nicht hielt.** Die
  Gegenprüfung hielt dieser Runde vor, dass alle drei neuen Messzahlen aus
  einem Testprozess (Debug) stammten und der Lauf am gebauten Binary fehlte.
  Er ist jetzt da: die beiden Dienstverweigerungen sind am
  `target/release/redact-rs` nachgemessen, mit `/usr/bin/time -v` für die
  Spitze und `zg_r1_decke::schreibt_material` für das Material.

  Dabei fiel eine der drei: für den Redaktor hinter der dokumentweiten Decke
  standen 45,5–45,6 s und 38 MB, zwei neue Läufe desselben Befehls im
  Testprozess (Debug) gaben aber 45,001 s und 39 404 kB sowie 45,289 s und
  39 384 kB — beide Zeiten unter der Spanne, beide Spitzen darüber. Ein Zehntel
  Spanne über eine Messung dieser Länge auf einer geteilten Maschine ist keine
  Zusage; sie ist gestrichen,
  nicht verschoben, und an ihre Stelle tritt der Lauf am Binary. Die anderen
  hielten, beide Male auf die Stelle genau.

  Zwei Zahlen bleiben im Testprozess, und der Satz sagt es jetzt selbst samt
  Profil: `scan_page` liegt im Extraktor, und die Statuszeile gehört der
  Oberfläche, die keine Kommandozeile hat. Das ist der zweite erlaubte Weg,
  kein Schlupfloch — der Maßstab steht im Vorspann dieses Abschnitts.

  Nicht hier stehen die Zahlen, mit denen die Gegenprüfung die Klartextträger
  am Metadatenlauf nachwies: sie stehen weiter in der Doku am Quelltext von
  `meta.rs`, und sie sind dort fast alle mit „Gemessen (vor dieser Änderung)"
  überschrieben — Trefferzahlen und Rückgabewerte des Zustands **vor** der
  Korrektur, also nach der Regel des Vorspanns keine Zusage über heute. Was
  vom Metadatenlauf den Zustand **danach** beschreibt, ist gebunden und steht
  im Abschnitt der Runde, die es gemessen hat.

  Und nicht hier stehen die Zahlen des **ungedeckelten** Zustands: der
  Extraktor brauchte 31,1 s, der Redaktor 105 s, und 3 320 MB gingen allein
  auf die Eigenschaftslisten. Sie beschreiben den ungedeckelten Stand
  `308ef38` und sind an diesem Baum nicht mehr zu messen — prüfbar durch
  Auschecken, nicht durch Wiederholen. Der Regeltest dieser
  Runde verlangt für jede Zahl im Block einen gebundenen Satz, und er hat
  recht: was hier als Zahl steht, ist eine Zusage.
* **Offen und unerklärt: ein Test des Tores flattert.** Im ersten Gate-Lauf
  dieser Runde fiel der Test, der verlangt, dass eine neue Analyse die
  gelöschten Zeilen vergisst — die Aussage wäre, dass eine Löschung aus dem
  vorigen Analyselauf im neuen einer danebengegangenen Schwärzung die Warnung
  nimmt. In allen Läufen danach, einer davon über den ganzen Arbeitsraum, ist
  er grün; die Korrektur, die er verlangt, steht nachweislich im Code
  (`deleted_hits.clear()` unbedingt in `analyze`, dazu der Abgleich über die
  Kennung der Region). Geprüft und verworfen wurden: eine Dateikollision
  (`kept_literal` entsteht rein im Speicher aus einer lokalen `AppState`), die
  Löscherinnerung (beim Fund nachweislich leer) und `set_region_rect` (ist
  deterministisch). Die Ursache ist **unbekannt**. Der Test bleibt scharf und
  unverändert — er hat in der Sache recht —, und der Punkt steht hier, weil
  ein Tor, das ohne Codeänderung zufällig rot wird, als Tor genauso wertlos
  ist wie eines, das zufällig grün bleibt.

### Fix-Runde 6: was die Gegenprüfung der Runde 5 noch fand

Fünf Gegenprüfer lasen die Korrekturen der Runde 5 mit eigenem Material gegen.
**Drei schwere Befunde** blieben: eine getaggte Seite von 276 kB, die 41,7 s
und 2,3 GB Arbeitsspeicher kostete, ohne dass eine Decke griff (gemessen im
Testprozess, Profil Debug, am Stand `fedcabe`, dem letzten Baum vor den
Korrekturen der Runde 6 — heute nicht mehr zu messen); ein unbekannter
Filtername **an erster Stelle** einer Kette, der das Leck-Orakel stumm machte
(„nicht gefunden“, Rückgabewert 0, über einen Strom, den keine Sicht gelesen
hatte); und ein wörtlich gleicher abgewählter Text, der eine danebengegangene
Schwärzung aus der Suche nahm. Dazu vier Klartextlecks an Nachbarn von
Annotationen, vier Fundorte statt einem bei der direkten Eigenschaftsliste,
zwei falsche Alarme — und die Erkenntnis, dass **keine einzige neue Zahl der
letzten Runde gebunden war**: siebzehn Messwerte in README, `SECURITY.md` und
`CHANGELOG.md` ließen sich in einem Lauf mutieren, ohne dass ein Test rot
wurde.

* **Die Messzahlen der Doku sind gebunden.** Die Gegenprüfung mutierte
  siebzehn Zahlen und Sätze in einem Lauf — jede Zeitangabe, jede MB-Zahl,
  jede ausgeschriebene Zahl, den ganzen Gründe-Satz — und **kein einziger Test
  wurde rot**. Die Runde 5 hatte zwei ungebundene „1 000“ geschlossen und dabei
  sieben neue Zahlen ungebunden angelegt. Jetzt steht jede Messzahl **einmal** im Testdatensatz
  (`belege.rs`, `messwerte`), und der Satz der Doku wird daraus gebaut: wer die
  Zahl in der Doku ändert, findet den Satz nicht mehr; wer sie im Test ändert,
  ebenso. Was sich ableiten lässt, wird abgeleitet und nicht abgeschrieben —
  das Verhältnis 1,21 aus 6,27 s und 5,20 s, die MB aus den gemessenen kB, die
  Grenze 16 777 216 aus `redact_core::MAX_AUX_FILE_BYTES`, „10 Zeichen im
  Spiegel, 5 in den Glyphen“ aus dem Spiegeltext `AlphaAlpha`, „bis zu drei
  Stellen beim Namen“ aus `MAX_NAMED_PLACES` im Quelltext der Oberfläche, und
  die Zahl der Kodierungen aus einem Lauf des gebauten Binaries.
* **`--help` widersprach sich selbst.** Drei Absätze über dem Block „Drei
  Fälle“ stand im selben Hilfetext weiter „Rückgabewert: `0`, wenn keiner der
  Begriffe gefunden wurde, `3`, wenn mindestens einer noch dasteht“ — die zwei
  Fälle, die die Runde 5 gerade abgeschafft hatte. Wer die Hilfe von oben nach
  unten liest, findet zuerst die falsche Fassung; ein Lauf über eine Datei mit
  einem Objekt auf Ebene 33 sagt „nicht gefunden“ und endet mit 3. Der Absatz
  nennt jetzt die Bedingung für `0`, und
  `der_dritte_fall_des_rueckgabewerts_drei_steht_ueberall` verbietet die alte
  Fassung wörtlich. Dieselbe Bedingung fehlte in der Rückgabewert-Tabelle der
  README.
* **`SECURITY.md` zählte drei Gründe für „nicht geprüft“ auf, das Orakel kennt
  fünf** — und die beiden fehlenden waren gerade die, die die Runde 5
  hinzugefügt hatte (ein unbekannter Filtername; der Schriftdekoder, der nach
  einem übersprungenen Strom gar nicht erst läuft). Eine Aufzählung, die
  weniger nennt, als es gibt, liest sich wie eine vollständige. Alle fünf
  stehen jetzt mit ihrem Wortlaut in einer Tabelle; ein Test hält jeden gegen
  den Quelltext des Orakels, drei davon zusätzlich gegen einen Lauf des
  gebauten Binaries (der vierte in `zf_q5_unbekannter_filter.rs`; der fünfte
  gehört der Oberfläche, weil `check::run` die Datei vorher selbst lädt).
* **UTF-16LE fehlte in beiden Kodierungslisten.** `--help` und die README
  nannten „UTF-8, Latin-1/PDFDoc, UTF-16BE und als Hex-String“, obwohl die
  Runde 5 UTF-16LE eingebaut hatte — vier Namen für neun Muster. Gesucht wird
  in **neun** Byte-Kodierungen (zehn mit Umlaut, sieben jenseits von
  Latin-1); die Zahl kommt jetzt aus einem Lauf, der jede Fassung in eine
  Datei legt und die gemeldeten Namen zählt. Und „12 000 Muster (1 000
  Begriffe × 12 Kodierungen)“ hat nie gestimmt: so viele **Kodierungen** gibt
  es nicht. Nach `Probe::new` sind es 10 Muster je Begriff (neun Bytefassungen
  und der dekodierte Text), 11 mit einer Fassung ohne Leerraum und 12 mit
  Umlaut **und** Leerraum. Die alte Messung selbst bleibt gültig — gemessen
  wurde die Zahl der Einzelsuchen, nicht die der Kodierungen.
* **Die Zusage über unbekannte Filternamen gilt jetzt wirklich.**
  `SECURITY.md` versprach: „Ein Filtername, den das Programm gar nicht kennt,
  steht sehr wohl in `NICHT GEPRÜFT`.“ Das galt nur, wenn vorher schon ein
  Filter gelaufen war: `/Filter /FooDecode` allein kam als „nicht gefunden“
  mit Rückgabewert 0 zurück, `/Filter [/FlateDecode /FooDecode]` mit 3 —
  dieselbe unlesbare Stelle, und die Meldung hing allein an der Position. Der
  Code meldet sie jetzt an jeder Stelle der Kette; die Doku sagt dazu, was
  gemessen ist: sechs Ketten, vier mit Meldung und Rückgabewert 3, zwei
  (Bildfilter allein und am Kettenende) ohne Meldung und mit 0 — sonst käme
  jede Datei mit einem Foto als unvollständig geprüft zurück.
* **Drei Zusagen, die zu viel versprachen.** Der Satz zu `ptrace` stand
  unqualifiziert unter einer Tabelle, die Windows und BSD getrennt ausweist —
  der Nebeneffekt gehört zu `prctl`, also zu Linux; auf den übrigen
  Unix-Systemen hat `setrlimit(RLIMIT_CORE, 0)` ihn nicht. Die README sagte
  über ungeprüfte Stellen „nennt sie beim Namen samt Grund“, während
  `MAX_NAMED_PLACES = 3` höchstens drei nennt und den Rest zählt. Und die
  Bombentabelle mischte Einheiten: derselbe Messwert stand im Fließtext als
  „2 172 628 kB ≈ 2,1 GiB“ (durch 1024²) und in der Tabelle als „2 173 MB“
  (durch 1000), beide Umrechnungen aus derselben Spitze eines Laufs am gebauten
  Binary (Release). MB heißt in diesem Projekt 1024² Byte; die Tabelle nennt
  jetzt **2 122 MB** und **3 145 MB** und sagt, aus welcher Zahl sie das
  rechnet: aus den kB-Spitzen, die ein Lauf am gebauten Binary (Release)
  ausgewiesen hat.

* **Eine getaggte Seite konnte den Rechner blockieren.** Verschachtelte
  `BDC`-Klammern mit Textspiegel über denselben `Do` ließen die Zuordnung
  Spiegel→Formular als Produkt wachsen: eine Datei von 276 kB mit **neun**
  Objekten belegte 2 306 MB und lief 41,7 s, ohne Warnung und ohne dass irgendeine
  Decke griff — die Liste entstand vor der ersten gezählten Zeichenoperation;
  gemessen im Testprozess, Profil Debug, am Stand `fedcabe`, dem letzten Baum
  vor den Korrekturen der Runde 6 — heute nicht mehr zu messen.
  Der Aufbau ist jetzt gedeckelt und läuft einmal je Strom statt je Platzierung:
  dieselbe Datei am gebauten Binary **0,15–0,18 s und 40 MB** (Release,
  Rückgabewert 3); im Testprozess (Debug), der nur den Extraktor fährt, 0,56 s
  und 38 MB; ohne `/ActualText` brauchte sie immer 0,15 s. Die Decke unter den
  Textspiegeln zählt jetzt
  **Zuordnungen zwischen einem Spiegel und einer Formularplatzierung**:
  höchstens 100 000 beim Aufbau und 100 000 beim Aufklappen, je Seiten-Scan
  (zusammen rund 16 MB). Wird sie erreicht **und dabei etwas weggelassen**, sagt
  eine Warnung, dass der Vergleich für die letzten Abschnitte unvollständig ist.
* **Und sie meldete einen Verlust, obwohl keiner eintrat.** Genau 100 000
  Aufklappungen gingen auf und lieferten trotzdem Rückgabewert 3 (`decke_99999.pdf`
  0, `decke_100000.pdf` 3) — dieselbe Klasse wie der ASCII85-Fall der Runde 5.
  Gefragt wird jetzt erst dort, wo eine Kante wirklich übersprungen wird.
* **Ein Textspiegel im Ressourcenverzeichnis blieb stehen — an vier Orten.**
  Eine Eigenschaftsliste, die direkt (ohne eigene Objekt-Id) in `/Resources
  /Properties` steht, verlor ihren `/ActualText` nur im Strom; im Verzeichnis
  stand der Klartext weiter in der Datei, ohne Warnung und mit Rückgabewert 0.
  Betroffen waren die Seite, ein Form-XObject, ein vom Seitenbaum geerbtes
  Verzeichnis und ein geteiltes `/Properties`-Objekt. Alle vier werden jetzt an
  ihrem Fundort bereinigt (`property_list_home` läuft die Ressourcenkette samt
  `/Parent`-Vererbung ab; ein geteiltes Verzeichnis wirkt auf beide Nutzer).
* **`--check-leaks` schwieg über einen Strom, dessen erster Filter unbekannt
  war.** `/Filter /FooDecode` kam als „nicht gefunden“ mit Rückgabewert 0 zurück,
  dieselbe Datei als `/Filter [/FlateDecode /FooDecode]` mit 3 — die Meldung hing
  allein an der Position. Jetzt entscheidet der Filter, an dem die Kette stehen
  blieb: ein Bildfilter bleibt der benannte blinde Fleck, jeder andere
  unbekannte Name steht in der `NICHT GEPRÜFT`-Liste, auch als erstes Glied.
  (Dass der Bildfilter das **an jeder Stelle** der Kette tat, war zu viel
  versprochen — siehe Fix-Runde 7.) Das Orakel klonte außerdem die Rohbytes eines Stroms, bevor es den
  ersten Filter kannte, und warf den Klon bei einem unbekannten Filter wieder weg:
  gemessen im Testprozess (Release, 64-MiB-Strom, `/DCTDecode`, Budget 512 MiB)
  **205 MB vorher, 138 MB nachher**, dort in Dezimal-MB gezählt; das „vorher“
  ist am Stand `fedcabe`, dem letzten Baum vor den Korrekturen der Runde 6,
  nachgemessen. Die alte Kostenzahl „1 000 Begriffe 65,7 s“ ist widerlegt und durch
  eine nachstellbare Rechnung ersetzt, gemessen im Testprozess (Release): 6
  Muster je Begriff über 256 MB je Durchgang — vier Blöcke à 64 MiB, MB wie
  überall 1024² Byte —, `memmem` 9,9 GB/s → 0,163 s je Begriff, rund **163 s
  für 1 000 Begriffe als untere Schranke**; heute 5,01 s (1 Begriff) gegen
  5,99 s (1 000).
* **Vier Klartextlecks an Annotationsnachbarn.** Der Dateiname einer
  Movie-Annotation (`/Movie /F`), die Maßangaben einer Vermessung (`/Measure`,
  mit Text in `/R`, `/U`, `/RT`, `/RD`, `/PS`, `/SS`) und eine eingebettete
  Datei unter `/RichMediaContent /Assets` blieben in der Ausgabe stehen; alle drei
  Beiwerk-Dictionaries fallen jetzt als Ganzes, gezeichnet wird von ihnen nichts.
  Und `/Alt` und `/ActualText` werden an **jedem** Dictionary geleert, das der
  Trägerlauf erreicht — auch an einem, das kein Träger ist: ein `/StructElem`,
  auf das das `/IRT` einer Annotation zeigt, verwaist **nicht** mit
  `/StructTreeRoot`, der Verweis hielt es samt Klartext am Leben. Gemessen: eine
  Datei mit der IBAN auf der Seite und in `/Movie /F` endete mit „Schwärzungen:
  1“ und Rückgabewert 0, während `--check-leaks` an der Ausgabe **6 Fundstellen**
  fand — jetzt 0.
* **Die Zahlen im Audit-Log melden keine Entfernung mehr, die `--check-leaks`
  findet — jetzt für jede Zahl, nicht nur für die vier Nutzlast-Zähler.** Stand
  hinter einem entfernten Schlüssel ein Verweis, zählt er nur, wenn das Objekt
  dahinter nach dem Aufräumen wirklich fehlt: „1 Lesezeichen entfernt“ über
  einen `/Title 4 0 R`, den ein zweiter Halter am Leben hielt, war eine
  Falschmeldung. Kosten gemessen im Testprozess (Release) am ungünstigsten
  Material (200 000 Annotationen mit `/Contents` als Verweis): 767–791 ms statt
  746 ms, kein zusätzlicher Speicher.
* **Oberfläche: „zählen deshalb nicht als Leck“ war die falsche Aussage.** Steht
  ein geschwärzter Text **wörtlich** auch in einer bewusst stehen gelassenen
  Zeile, wird er nicht gesucht — die Statuszeile verkaufte das als Ergebnis und
  hinterließ keine Warnung, auch wenn die Schwärzung danebenging und der Text auf
  ihrer eigenen Seite noch stand. Jetzt sagt sie, dass sie über diese Texte
  **nichts** sagt, und die Warnung bleibt. Eine Zuordnung des Fundes zu Seite und
  Rechteck wurde geprüft und verworfen: das Orakel nennt den Ort nur im Text der
  Fundmeldung — bleibt als Vertrag für die nächste Runde stehen.
* **Drei weitere Sätze der Oberfläche.** Ließ sich die geschriebene Datei nicht
  zurücklesen, nannte die bleibende Warnung die Decke von 1 000 Begriffen als
  Grund statt der Wahrheit — sie nennt jetzt denselben Grund wie die Statuszeile.
  Die Statuszeile begann mit der Entwarnung und brachte den Vorbehalt danach;
  jetzt führt der Fund, ohne Fund stehen die Vorbehalte vorn, und die genannten
  ungeprüften Stellen sind auf Ort und Grund gekürzt (804 → 411 Zeichen bei drei
  Stellen). Und wurde dieselbe Datei ein zweites Mal exportiert, während die
  erste Nachprüfung noch lief, bewertete diese die **neuen** Bytes mit dem
  **alten** Plan unter dem Präfix des ersten Exports — die ältere Prüfung endet
  jetzt mit dem zweiten Export. Der Wächter, der das Neuzeichnen anfordert, ist
  jetzt belegt: ein Test hält den Prüf-Thread an und verlangt, dass vor seinem
  Ende **kein** Bild angefordert wird (vorher blieb die Mutation unbemerkt).

### Fix-Runde 5: was die Gegenprüfung der Runde 4 noch fand

Fünf Gegenprüfer lasen die Korrekturen der Runde 4 mit eigenem Material gegen.
**Vier stille Lecks** blieben: ein unbekanntes Filterglied nahm dem Orakel die
ganze Kette; die Objektsicht brach ab Verschachtelungstiefe 33 stumm ab;
eingebettete Annotationsteile und tiefe Feldwerte behielten ihren Klartext; und
die Oberfläche suchte nur die erste Schreibweise. Dazu zwei falsche Alarme und
neun Stellen, an denen die Doku mehr oder anderes sagte als der Code.

* **Der Rückgabewert 3 hat drei Bedeutungen, nicht zwei.** Seit der Runde 4
  endet `--check-leaks` **auch ohne Fund** mit 3, wenn eine Stelle ungeprüft
  blieb — das stand in `main.rs`, aber `--help` sagte weiter „Zwei Fälle“,
  `SECURITY.md` „hat **zwei** Bedeutungen“ und die README-Tabelle nannte nur
  den Fund. Wer danach ein Skript baute, hielt `3` ohne Fund für unmöglich.
  Alle drei Texte nennen jetzt den dritten Fall samt seinen beiden Ursachen
  (Entpackgrenze, Verschachtelungstiefe), und
  `der_dritte_fall_des_rueckgabewerts_drei_steht_ueberall` hält sie zusammen —
  einschließlich des `--help`-Zitats in `SECURITY.md`, das jetzt Zeile für Zeile
  gegen den echten Hilfetext geprüft wird.
* **Die Bombentabelle war nicht reproduzierbar** (siehe oben, Runde 4): 345 MB
  und 882 MB für ein entpacktes GiB. Neu gemessen und mit dem Verfahren
  beschrieben.
* **Der Spitzenspeicher hängt am größten Einzelstrom, nicht am Budget.**
  `SECURITY.md` nannte „1024 MB“ und ließ offen, was das für den Bedarf heißt.
  Gemessen an einer 1 020 KiB großen Datei mit einem 1-GiB-Strom, mit den
  **Vorgabewerten**: `--check-leaks` endet mit Rückgabewert 0 und einem `VmHWM`
  von 2 172 628 kB = 2 122 MB ≈ 2,1 GiB — dieselbe Spitze wie beim Schwärzen
  (2 172 312 kB = 2 121 MB), also rund das Doppelte des größten Einzelstroms. Steht in der
  Grenzentabelle.
* **Ein Kostensatz, der den abgeschafften Zustand beschrieb.** `--help`, README
  und die Fehlermeldung in `check.rs` sagten „Jeder Begriff kostet einen
  Vergleich über die ganze Datei“ — seit Runde 4 kostet 1 000 Begriffe kaum mehr
  als einer. Die Decke von 1 000 bleibt, aber mit ihrem heutigen Grund und
  dessen Messung: der Automat wächst linear mit der Liste (1,0 MB bei 1 000
  Begriffen, 7,1 MB bei 10 000, 68 MB bei 100 000, **675 MB** bei einer Million,
  dazu 25 s allein für seinen Bau), und jeder Begriff bekommt eine eigene Zeile
  im Bericht.
* **`docs/pruefung.txt` war nur zur Hälfte gebunden.** `tests/belege.rs` verglich
  die `GEFUNDEN`/`nicht gefunden`-Zeilen, den Rückgabewert und die Fassung — die
  Dateigrößen und die Fundstellenzeilen nicht. Die Mutationen „1862“→„1863“ und
  „(Objekt 4 0)“→„(Objekt 9 0)“ blieben grün, obwohl `check-preview.py` in
  seinem Kopf zusagte, `belege.rs` halte das fest. Jetzt wird der **ganze
  Berichtsblock** verglichen; beide Mutationen sind rot.
* **Die Zahl der Prüfungen in `check-preview.py` ist gebunden.** Die README
  nannte „36 Prüfungen“, das Skript meldete 54.
  `die_zahl_der_pruefungen_steht_im_readme` liest die letzte Zeile des
  Skriptlaufs und verlangt genau dieses `N` in der README.
* **Zwei ungebundene „1 000“ in diesem Verlauf** — eine davon ausgerechnet in dem
  Satz, der die Bindung ankündigt — liest jetzt
  `the_needle_ceiling_is_one_number_in_code_help_and_docs` mit.
* **Reste.** `crates/redact-cli/Cargo.toml` sagte noch „Windows hat kein
  Gegenstück“ (es gibt eines, `WerAddExcludedApplication`, nur ist es nicht
  umgesetzt und steht nicht in libc). `--help` nannte `redact_pdf::leaks` statt
  `leaks_many_within`. Die 16-MB-Zeile in `SECURITY.md` liest sich jetzt als
  das, was sie ist — eine Grenze der **Vorprüfung** für Ketten, die nicht reines
  Flate sind (nachgemessen: 17 MB `ASCII85Decode` → Rückgabewert 1; dieselben
  17 MB als `ASCIIHexDecode` oder `RunLengthDecode` laufen durch, weil die
  Vorprüfung sie gar nicht auspackt). Und der Satz zu `ptrace` sagt jetzt
  „unter Linux“.
* **Der Rat am Ende einer unvollständigen Prüfung war falsch geworden.**
  „mehr davon packt `--max-decompressed-mb` aus“ half gegen die
  Verschachtelungstiefe nichts. Der Satz nennt jetzt beide Ursachen und sagt,
  welche der Schalter erreicht (`ze_p4_check_leaks_grenzen::tiefe_33_…`, jetzt
  scharf statt `#[ignore]`).

* **Das Leck-Orakel verlor den entzifferbaren Anfang einer Filterkette.** War
  ein späteres Glied unbekannt, warf es die ganze Kette weg: Klartext im
  Flate-Teil von `[/ASCIIHexDecode /FlateDecode /DCTDecode]` wurde nicht mehr
  gefunden — das Orakel fand damit **weniger** als vor der letzten Runde, und
  an ihm messen alle anderen Tests. Es dekodiert jetzt so weit, wie es kommt,
  und nennt in der Fundstelle den Filter, an dem es stehen blieb. Der Schwärzer
  bleibt streng: einen halb dekodierten Strom liest er nie als Seiteninhalt
  (`ze_p2_seitenschleife::halb_dekodierter_strom_wird_nie_seiteninhalt`).
* **Die Objektsicht bricht bei Verschachtelungstiefe 32 ab — und sagt es
  jetzt.** Ein oktal maskierter Text in 33 verschachtelten Arrays bekam
  „nicht gefunden“ und Rückgabewert 0, obwohl keine Sicht ihn gelesen hatte;
  bei 32 Ebenen wurde derselbe Text gefunden. Der Abbruch steht jetzt mit
  Objektpfad in der `NICHT GEPRÜFT`-Liste (Rückgabewert 3). Die Grenze bleibt
  bei 32, gemessen: Tiefe 99 bei Breite 1 000 kostet 35,4 ms gegen 57,9 ms
  bei einer Grenze von 100 — bezahlbar, aber ohne Nutzen.
* **`ASCII85Decode` lehnte einen Strom ab, der exakt ins Restbudget passte.**
  Die Prüfung unterstellte jeder Fünfergruppe vier Ausgabebytes; die letzte
  liefert eins bis drei. Ergebnis: Rückgabewert 3 („nicht geprüft“) an
  harmlosem Material, und die Datei verlor zusätzlich die Schriftdekoder-Sicht.
  Alle fünf Filter nehmen jetzt nachweislich genau die Grenze an.
* **`--check-leaks` findet UTF-16LE-Bytes auch innerhalb eines Stroms**, nicht
  nur in einem Zeichenketten-Objekt. Kosten unter der Messstreuung (8 MiB
  entpackt, 200 Begriffe: 31,2–32,2 ms vorher, 29,3–34,4 ms nachher).
* **Drei Klartexte an Annotationen, die der Graphlauf der letzten Runde nicht
  erreichte.** Ein direkt eingebettetes `/Popup <</Contents (…)>>`, ein inline
  stehendes Widget in einem `/Kids`-Array und jeder Feldwert jenseits von 32
  Ebenen blieben in der Ausgabe stehen — `--check-leaks` fand sie, der Lauf
  meldete 0. Der Grund war jedes Mal derselbe: verfolgt wurden nur *Verweise*,
  und die Feldwerte liefen in zwei eigenen, tiefenbegrenzten Vorläufen. Jetzt
  bereinigt **ein** besuchsgeführter Lauf ohne Tiefengrenze alles, was eine
  Annotation erreichbar hält, eingebettete Dictionaries eingeschlossen; `/V`,
  `/DV` und `/RV` fallen darin mit. Gemessen (Release): 100 000 Felder 0,40 s,
  eine `/Parent`-Kette aus 1 000 000 Feldern 1,65 s.
* **Die Zahlen im Audit-Log melden nur noch Entferntes.** `outlines_removed`
  zählte vor dem Aufräumen: ein Lesezeichen, das noch an zweiter Stelle hing,
  überlebte samt `/Title`, gemeldet wurde trotzdem 1. Anhänge, JavaScript,
  `/XFA` und Lesezeichen werden jetzt **nach** `prune_unreachable` gezählt
  (Träger *und* Teilbaum müssen weg sein), jedes Lesezeichen verliert
  zusätzlich seinen `/Title` an Ort und Stelle, und ein Schlüssel mit Wert
  `null` bewegt keinen Zähler mehr (PDF 32000-1, 7.3.9).
* **Die Erreichbarkeitsprüfung riet nicht mehr.** Sie brach bei 64 Ebenen ab
  und wertete alles darunter als unerreichbar — ein XObject hinter 70
  verschachtelten Arrays wurde gelöscht. Sie läuft jetzt ohne Tiefengrenze;
  eine Prüfung, die abbricht, muss im Zweifel „erreichbar“ sagen.
* **Der Trailer der Ausgabe trägt nur noch `/Root`, `/Info`, `/Encrypt`, `/ID`
  und `/Size`.** Ein Objekt unter einem selbstgebauten Trailerschlüssel
  (`<< /Zusatz 7 0 R >>`) überlebte bisher jedes Aufräumen, weil der ganze
  Trailer Wurzel der Erreichbarkeitsprüfung ist.
* **Ein Textspiegel über einem mehrfach platzierten Formular galt als
  Widerspruch.** `/Span <</ActualText (AlphaAlpha)>> BDC /Fm0 Do /Fm0 Do EMC`:
  die Schließung entdoppelte über Objekt-Ids und zählte die Glyphen halb —
  „10 Zeichen im Spiegel, 5 in den Glyphen“, Rückgabewert 3 an gewöhnlichem
  getaggtem Material, und zwar nur dann, wenn *irgendwo* im Dokument ein
  Formular ein Formular zeichnete. Gezählt werden jetzt Platzierungen; Zyklen
  beendet die Kette der Vorfahren. Neue Decke: höchstens 100 000
  Formularplatzierungen unter den Spiegeln einer Seite (Mehrkosten gemessen:
  8,5 MB, unter 0,3 s); wird sie erreicht, sagt eine Warnung, dass der
  Vergleich für die letzten Abschnitte unvollständig ist. (Die Fix-Runde 6 hat
  diese Decke noch einmal umgestellt — sie zählt seither Zuordnungen zwischen
  Spiegel und Platzierung, siehe oben.)
* **Die Oberfläche suchte nur eine von zwei Schreibweisen.** Standen dieselbe
  Zeichenfolge mit und ohne Leerzeichen in zwei geschwärzten Zeilen, wurde nur
  die erste gesucht: die Statuszeile meldete „1 gesuchte(r) Text steht nicht
  mehr in der Ausgabe“ über eine Datei, in der die IBAN noch **siebenmal**
  stand. Jetzt wird jede Schreibweise gesucht; die Decke von 1 000 Begriffen
  zählt Schreibweisen, weil jede ein eigenes Muster im Automaten ist.
* **Und ein abgewählter Text deckte zu viel.** Er nahm jede Schreibweise seiner
  Normalform aus der Suche — samt einer Schwärzung, deren Rechteck daneben
  ging. Die Entscheidung fällt jetzt **am Fund**: wer wörtlich in der Ausgabe
  steht, ist ein Leck; trifft nur die Fassung ohne Leerraum eines bewusst
  stehen gelassenen Textes, ist es keins. Der Fehlalarm, den die vorige Runde
  abgestellt hat, bleibt abgestellt.
* **Die Nachprüfung behauptet keine Ursache mehr.** Sie sagte „N Stelle(n)
  wurden nicht geprüft (Entpackgrenze)“, auch wenn das Budget voll war und die
  Tiefengrenze zugeschlagen hatte. Jetzt zählt sie die Stellen und gibt deren
  eigenen Grund wieder — höchstens drei beim Namen, der Rest gezählt.
* **Zwei Kleinigkeiten der Oberfläche.** Die Warnung eines Exports überlebt den
  nächsten (gehalten je Ausgabedatei, höchstens zehn, bis zum nächsten
  Dokument) — bisher ersetzte jeder geglückte Export die Liste, und bei zwei
  Ausgabedateien war das Urteil über die erste weg. Und stirbt der Prüf-Thread,
  wird das Neuzeichnen aus einem Drop-Wächter angefordert; bisher blieb die
  Statuszeile auf „Nachprüfung läuft …“ stehen.
* **Der Windows-Job der CI war rot — wieder durch einen Test.** Die
  Speichermessung der Bombentests las auf jedem Ziel `/proc/self/status`. Unter
  Linux misst sie unverändert scharf; auf anderen Zielen prüfen dieselben Tests
  Frist, Fund und die Liste der ungeprüften Stellen, werden also nicht
  bedingungslos grün. Dass das lokale Tor für Windows nur Clippy fährt und
  nicht die Tests, bleibt die Lücke, durch die so etwas kommt.

### Fix-Runde 4: was die Gegenprüfung der Runde 3 noch fand

Fünf Gegenprüfer lasen die Korrekturen der Runde 3 mit eigenem Material
gegen. Sieben Klassen blieben: der Textspiegel über einem geteilten Formular
wurde nicht geleert; ein Bild auf einer Hex-Seite überlebte; ein `/Filter`
als Verweis wurde nicht aufgelöst; Annotationstexte jenseits `/Annots`
blieben; das Orakel der Nachprüfung entpackte ohne Budget, mit Kosten
Begriffe × Bytes; die Nachprüfung verglich wörtlich, suchte aber gequetscht;
und Doku, die mehr sagte als der Code. Diese Runde schließt sie.

* **`--check-leaks` hat ein Budget — und sagt, was es nicht gesehen hat.**
  Die Suche packte jeden Strom aus, den sie fand, ohne Grenze; die
  Vorprüfung davor sah nur, was das Dictionary als Flate ausweist. Jetzt
  läuft sie über `redact_pdf::leaks_many_within` mit demselben Budget wie
  `--max-decompressed-mb` (Vorgabe 1024 MB) — dieselbe Einheit, die Summe
  der entpackten Bytes, je Sicht der Suche (Rohsicht, Objektsicht) einmal.
  Ein Strom, der das Restbudget sprengte, wird nicht entpackt und steht als
  `NICHT GEPRÜFT: …` in der Ausgabe — auch unter
  `--quiet` —, und der Lauf endet **mit Rückgabewert 3 auch ohne Fund**:
  „Ergebnis: 1 Stelle(n) nicht geprüft — die Antwort ist unvollständig.“ Mit
  Fund stehen beide Sätze da. Gemessen am gebauten Binary
  (`check_leaks_names_what_the_budget_left_unchecked_and_returns_3`): ein
  Strom ohne `/Filter`, dessen Bytes 4 MiB Nullen als Flate sind (4 kB auf
  der Platte), mit `--max-decompressed-mb 1` → 3 und die Stelle; ohne
  Schalter → 0. Die andere Tür bleibt zu: ein als Flate ausgewiesener Strom
  über dem Budget fällt wie beim Schwärzen schon in der Vorprüfung — 1, nie 0
  (`a_stream_over_the_budget_is_refused_before_the_search`). `--help` zu
  `--check-leaks` und `--max-decompressed-mb`, README, `SECURITY.md`
  (Grenzentabelle) nennen das Budget.
* **Die Decke von 1 000 Begriffen war an drei weiteren Stellen ungebunden.**
  README „Nachprüfung nach dem Export“ und zwei Sätze zur Oberfläche in
  diesem Verlauf trugen die Zahl als Literal, und
  `the_needle_ceiling_is_one_number_in_code_help_and_docs` las sie nicht.
  Jetzt liest er auch diese drei, aus der Konstante formatiert; der
  Gegenprüfer-Test `zc_g4_decke_doku.rs` ist darin aufgegangen. README-Satz
  auf „1 500“ gesetzt: rot.
* **Doku gegen Code, sieben Stellen.** Die README sagte „unter macOS tut der
  Aufruf nichts“ — er setzt `setrlimit(RLIMIT_CORE, 0)` (`dumpable.rs`,
  `SECURITY.md`, dieser Verlauf sagten es längst). „Unter Windows gibt es
  kein Gegenstück“ (README, `SECURITY.md`, `dumpable.rs`, zweimal hier) war
  die falsche Begründung für die richtige Aussage: WER kennt
  `WerAddExcludedApplication`; dieses Programm ruft es nicht auf — die
  Zusage lautet jetzt ehrlich „nicht umgesetzt“. `SECURITY.md` versprach
  eine Bereichsprüfung für „Feldwerte“ — es ist genau eine, die
  Seitennummer. Die Messung zur Seitennummer nannte „Debug/Release“ aus dem
  Test — der läuft nur im Dev-Profil; der Release ist von Hand gemessen, und
  das steht jetzt so. Der Satz „die Warnung zum geteilten Formular weist
  darauf hin“ (Spiegel im Seitenstrom) versprach mehr, als die Warnung sagt —
  sie nennt Seiten, keinen Spiegel; den Fall selbst schließt diese Runde
  (Spiegel/Formulare, unten). „6,3 s bei 305 Seiten“ für den Export hat
  keinen Beleg im Baum und ist so gekennzeichnet. „Drei Dinge … und zwar
  immer“ in der Statuszeile der Oberfläche: die Rechteckzahl erscheint nur,
  wenn sie größer als null ist.
* **`scripts/make-preview.sh` löst ein relatives `CARGO_TARGET_DIR` gegen
  das Verzeichnis des Aufrufers auf, nicht gegen das Repository.** Die
  Fassung der Runde 3 nahm `$repo_root`: `cd scripts &&
  CARGO_TARGET_DIR=../target ./make-preview.sh` baute damit nach
  `<repo>/../target`. Jetzt wird der Wert einmal gegen `$PWD` aufgelöst und
  absolut exportiert — cargo und das Skript meinen denselben Ort. Gemessen:
  derselbe Aufruf baut nach `<repo>/target`, `git status docs/` bleibt leer.
* **Ein Textspiegel über einem Formular blieb stehen — an zwei Stellen.**
  Liegt der Spiegel *im* Formular und die Glyphen in einem inneren
  (`/Span <</ActualText …>> BDC /Fm1 Do EMC`), wurde er gelesen und gemeldet,
  aber nie geleert: das äußere Formular hatte keinen eigenen Plan und wurde
  deshalb nicht neu geschrieben. Und wird dasselbe Formular von zwei Seiten
  benutzt, aber nur auf der zweiten geschwärzt, verlor es dort seine Glyphen,
  während der Spiegel auf der ersten Seite stehen blieb — mit Rückgabewert 0,
  weil die Warnung zum geteilten Formular als „zu viel geschwärzt“ einsortiert
  ist. Gemessen: `leaks` fand die IBAN in der Ausgabe an 8 Stellen. Beides ist
  behoben. Der Bestand der Formulare, die neu geschrieben werden, umfasst jetzt
  auch die, die einen Spiegel über einem getroffenen Formular tragen; und die
  Seiten werden erst geschrieben, **nachdem** alle Formularpläne feststehen —
  vorher liefen Seiten- und Formularschleife nacheinander, und was die zweite
  entschied, erreichte die erste nicht mehr. Nach der Korrektur bleibt kein
  Spiegel stehen (`leaks`: 0 Fundstellen); die „bekannte Grenze“, die der
  Verlauf der Runde 3 hier noch nannte, gibt es nicht mehr.
* **Und die Gegenrichtung: ein Formular, das erst ein inneres zeichnet, gab
  eine falsche Warnung.** `/Fm1 Do (0044 …) Tj` unter einem deckungsgleichen
  Spiegel meldete „27 Zeichen im Spiegel, 27 in den Glyphen“ — die Glyphen des
  inneren Formulars standen in der falschen Reihenfolge. Die Glyphenfolge unter
  einem Spiegel folgt jetzt dem Pfad der `Do`-Aufrufe, beliebig tief.
* **Eine Annotation mit Text, aber ohne Erscheinungsstrom, war keine
  Deckungslücke.** Sie endete mit Rückgabewert 3 („wurde nicht durchsucht und
  kann deshalb nicht geschwärzt worden sein“) — obwohl derselbe Lauf ihren
  Text mit den Metadaten entfernt und `--check-leaks` danach 0 meldete. Die
  Warnung sagt jetzt, was wirklich geschieht: kein anteiliges Schwärzen, weil
  es keine Glyphen gibt, sondern Entfernen als Ganzes. Sie zählt nicht mehr als
  Lücke; Rückgabewert 0, und `leaks` an der Ausgabe ist leer
  (`an_annotation_without_appearance_stream_is_no_longer_a_gap`). Stellvertreter
  für den Rückgabewert 3 in `tests/incomplete.rs` ist jetzt ein XObject ohne
  bekanntes `/Subtype` — ein Fall, der wirklich ungelesen bleibt.
* **Das Leck-Orakel sucht alle Begriffe in einem Durchgang.** Bisher lief je
  Begriff eine eigene Teilstringsuche über dieselben entpackten Bytes; die
  Kosten waren Begriffe × Bytes. Jetzt trägt ein Automat (`aho-corasick`,
  liegt über `regex` ohnehin im Graphen) alle Begriffe in allen Kodierungen —
  UTF-8, UTF-16BE/LE, Hex groß und klein, Verkettung, ohne Leerraum — und
  läuft einmal je Datenblock. Gemessen (Release, 64-MiB-Datei mit nicht
  komprimierbarem Bildstrom, `zd_mess_1000_begriffe_kosten_wie_einer`):
  1 Begriff **5,20 s**, 1 000 Begriffe **6,27 s** — Verhältnis 1,21, in der
  Fix-Runde 5 nachgemessen. Vorher war es Begriffe × Bytes; die alte Fassung
  ist nicht mehr im Baum, ihr Kostengesetz aber nachstellbar: **12 000
  Einzelsuchen** mit `memmem` über 64 MiB kosten **89,2 s**, zwölf davon
  0,08 s; derselbe Durchgang mit einem Automaten kostet 0,28 s. (Die 12 000
  waren damals als „1 000 Begriffe × 12 Kodierungen“ gerechnet; es sind in
  Wahrheit 10 Muster je Begriff — siehe Fix-Runde 6. Gemessen wurde die Zahl
  der Einzelsuchen, am Kostengesetz ändert die Richtigstellung nichts.) Die Fundstellen sind Zeichen
  für Zeichen dieselben (`positions_agree_with_the_naive_search`).
* **Und es hat ein Budget: `leaks_many_within`.** Das Orakel packte jeden
  Strom aus, den es fand — die Grenze `--max-decompressed-mb` galt nur dem
  Schwärzen. Jetzt bekommt jede Sicht dasselbe Budget, es wird **beim
  Entpacken** eingehalten (`take`, nicht hinterher gemessen) und die
  Vorprüfung des Laders läuft mit derselben Zahl, damit lopdf keinen
  Objektstrom unbegrenzt auspackt. Ein Strom über dem Restbudget wird
  übersprungen und in `unchecked` benannt (Objekt und Grund); seine gepackten
  Bytes werden roh trotzdem durchsucht. In der Fix-Runde 5 nachgemessen
  (`redact-rs <bombe> --check-leaks XX`, Spitze über
  `getrusage(RUSAGE_CHILDREN).ru_maxrss`, 1 GiB Nullen, 1 044 089 bzw.
  1 044 192 Byte gepackt): mit `--max-decompressed-mb 16` lehnt die Vorprüfung
  beide Formen nach 0,02 s bei 24 MB ab (Rückgabewert 1); mit
  `--max-decompressed-mb 4096` — dem Lauf ohne wirksame Grenze — steigt die
  Spitze auf **2 122 MB** (Seiteninhalt) bzw. **3 145 MB** (`/ObjStm`), je rund
  18 s (2 172 628 bzw. 3 220 164 kB; MB heißt hier wie überall 1024² Byte —
  die Fix-Runde 6 hat diese beiden Zahlen von der Zehnerteilung auf die
  Einheit des Dokuments gebracht). Hier standen bis dahin 345 MB und 882 MB; das konnte nicht stimmen, ein
  wirklich entpacktes GiB liegt danach im Speicher.
* **Filterketten: ein Verweis ist eine Schreibweise, kein Grund zur Absage.**
  `/Filter 5 0 R`, `/DecodeParms` als Verweis und Werte *im*
  Parameter-Dictionary (`/Columns 8 0 R`, `/Predictor 12 0 R`) wurden nicht
  aufgelöst: die Datei galt als nicht zerlegbar (Rückgabewert 1) oder wurde
  still ohne Prädiktor dekodiert. Alle drei Ebenen werden jetzt aufgelöst.
  Dazu bekommen LZW, ASCII85 und RunLength dieselbe Größengrenze wie Flate
  (LZW über `weezl`, die Bibliothek, die lopdf ohnehin mitbringt).
* **Ein Bild auf einer ASCIIHex- oder RunLength-kodierten Seite wurde nicht
  überschrieben.** Die Bildsuche las den Seiteninhalt über lopdf, das diese
  beiden Filter nicht kennt: 0 geschwärzte Bilder, keine Warnung, und die
  Pixel blieben unter dem Deckrechteck. Sie benutzt jetzt denselben Dekoder
  wie der Interpreter.
* **Klartext an Annotationen: die Bereinigung folgt jetzt dem Graphen.**
  Geleert wurde nur der `/Annots`-Eintrag selbst; ein `/Popup`, die
  `/Parent`-Kette (Radiogruppen, Felder mit mehreren Widgets), `/Kids` und
  `/IRT` behielten ihre Texte. Jetzt läuft die Bereinigung über alles, was
  eine Annotation erreichbar hält, mit Besuchsmenge gegen Zyklen. Neu in der
  Liste: `/Opt` (Auswahltexte), `/OverlayText`, `/NM`, `/DS` und die
  Beschriftungen im `/MK`; neu bei den Aktionen `/PA`. Ein Verweis *im*
  `/Dest`-Feld muss auf eine Seite führen, sonst fällt das Ziel. **Nicht**
  entfernt wird `/DA` — Pflichtschlüssel und Operatorfolge, kein Menschentext;
  als benannte Lücke in `SECURITY.md`.
* **Zwei Kleinigkeiten am selben Lauf.** Das Löschen eines Schlüssels nahm das
  referenzierte Objekt mit — bei einer Datei, die `/Contents 4 0 R` mit einem
  Lesezeichen teilt, verlor die Ausgabe damit ihre Seite; jetzt löscht es nur
  den Schlüssel, und `prune_unreachable` räumt auf. Und die Lesezeichen wurden
  nur 32 Ebenen tief gezählt (33 statt 42 gemeldet, entfernt wurden sie
  trotzdem) — die Tiefengrenze ist weg, der Deckel ist die Besuchsmenge:
  100 000 Ebenen in 370 ms.
* **Das Audit-Log führt vier Zahlen, die es bisher nur als Fließtext kannte:**
  `metadata.outlines_removed`, `annotation_actions_removed`,
  `annotation_texts_cleared` und `optional_content_names_cleared`. Additiv mit
  Vorgabewert 0, also ohne Schemabruch; ein Test am Binary vergleicht sie mit
  dem, was der Lauf wirklich entfernt hat.
* **Die Nachprüfung der Oberfläche entscheidet auf derselben Normalform, auf
  der sie sucht.** Sie verglich wörtlich, die Suche vergleicht auch ohne
  Leerraum: dieselbe IBAN als „DE89 3704 …“ geschwärzt und als „DE893704…“
  abgewählt galt als Leck — ein Fehlalarm über eine Datei, die genau so
  gewollt war. Ein Teilstring bleibt ein eigener Text und wird weiter
  gemeldet. Die Nachprüfung läuft außerdem mit dem Entpackbudget des Ladens
  und sagt, wie viele Stellen sie deshalb nicht geprüft hat.
* **Und sie sagt es dort, wo es stehen bleibt.** Nicht gesuchte Texte jenseits
  der Decke, eine unvollständige Antwort und ein im Hintergrund abgebrochener
  Prüflauf standen nur in der Statuszeile, die die nächste Aktion überschreibt;
  sie stehen jetzt auch in den Warnungen, jede mit dem Namen der Ausgabedatei
  davor. Der Abbruchzweig hatte keinen Test — man konnte ihn streichen, und
  alle 306 Tests blieben grün; ein Haken im Testbau erzwingt ihn jetzt.

### Fix-Runde 3: was die Gegenprüfung der letzten Runde noch fand

Fünf Prüfer hatten die Korrekturen der vorigen Runde mit eigenem Material
gegengelesen. Die Befunde hielten; die **Korrekturen** ließen Lücken. Diese
Runde schließt sie. Drei Klassen kehren wieder: eine Decke zählt die falsche
Einheit, eine Zusicherung wird an die nächste Stelle mitgenommen, wo sie nicht
gilt, und ein Test bleibt ohne seine Korrektur grün.

* **Lesezeichen und Annotationen tragen Klartext — und der ging bis 0.6.0
  mit.** Nachgetragen: die Korrektur kam in der vorigen Fix-Runde, der Eintrag
  dazu fehlte. Der `/Title` eines Lesezeichens („Kontoauszug DE89 …“), die
  Aktionen `/A` und `/AA` einer Annotation (`/URI` mit `mailto:…?subject=DE89 …`,
  `/F` bei `/GoToR` und `/Launch`, `/JS`), ein benanntes `/Dest` und die
  Kommentartexte `/Contents`, `/RC`, `/T`, `/Subj` überlebten die Schwärzung
  mit Rückgabewert 0 — gemessen, `--check-leaks` fand sie alle. Jetzt fällt
  `/Outlines` ganz, und an jeder verbliebenen Annotation fallen Aktionen und
  Klartexte; ein ausdrückliches Ziel (`[Seite /XYZ x y z]`, Zahlen und
  Verweise, kein Text) bleibt. Die Zusammenfassung nennt es: „Lesezeichen
  (/Outlines)“, „Aktionen oder benannte Ziele an Annotationen (/A, /AA,
  /Dest)“, „Kommentartexte an Annotationen (/Contents, /RC, /T, /Subj, /TU,
  /TM)“. Preis: Gliederung und Verweise ins Netz oder in andere Dateien
  funktionieren danach nicht mehr — die sichere Richtung, dieselbe wie bei
  `/Names` und `/AcroForm`. Tests: `crates/redact-pdf/tests/zb_klartext_traeger.rs`
  (samt einem zirkulären Lesezeichenbaum, der enden muss).
* **Annotationen: `/TU` und `/TM` fielen nicht.** Beide stehen nur an
  Formularfeldern (PDF 32000-1, 12.7.3.1): `/TU` ist der alternative Feldname,
  den der Betrachter als **Tooltip** zeigt, `/TM` der Exportname — beide frei
  wählbarer Text, den Formulargeneratoren mit der Beschriftung füllen
  („Konto von Max Mustermann“). Gemessen: die IBAN im Tooltip überlebte
  `strip_metadata`, und `leaks` fand sie in der geschriebenen Datei. Jetzt
  fallen beide mit den übrigen Kommentartexten; `/T` ist an einem
  Widget der Feldname, nicht der Verfasser. Dazu: ein `/Dest`, das als
  Verweis geschrieben ist (`/Dest 12 0 R`), wird aufgelöst und über sein
  **Feld** entschieden — ein ausdrückliches Ziel bleibt jetzt auch in dieser
  Schreibweise, eine Zeichenkette dahinter fällt weiterhin. Vorher fiel jeder
  Verweis, obwohl der Quelltext das Gegenteil versprach. Tests:
  `zb_annotationstexte.rs`; Mutation `TU` aus dem Array genommen: rot.
* **Textspiegel: drei Schlüssel, zwei Rollen.** `/ActualText`, `/Alt` und
  `/E` werden weiterhin gelesen und mit den Glyphen geleert; als Widerspruch
  **gemeldet** wird nur noch `/ActualText` — nach PDF 32000-1 (14.9.4) ist es
  der *Ersatz* der Glyphen und muss ihnen gleichen, `/Alt` (14.9.3)
  *beschreibt* und `/E` (14.9.5) *schreibt aus*, beide dürfen abweichen. Ein
  `/Figure <</Alt …>> BDC /Im0 Do EMC` (die Standardform der
  Barrierefreiheit) und ein `/E` über einer Abkürzung sind keine Befunde mehr
  — vorher Rückgabewert 3 an einer gewöhnlichen getaggten Datei, also eine
  Grenze, die gewöhnliche Dateien ablehnt. Ein `/ActualText` ohne Glyphen
  darunter wird weiterhin gemeldet: das Werkzeug kann ihn nicht schwärzen,
  also muss es das sagen.
* **Textspiegel über einer Formulargrenze.** Bei
  `/Span <</ActualText …>> BDC /Fm0 Do EMC` liegen die Glyphen im Formular,
  der Spiegel im Seitenstrom; die Prüfung sah „0 Glyphen“, behauptete in der
  Warnung einen Suchlauf, der nicht stattfand, und ließ den Spiegel beim
  Schwärzen stehen. Jetzt zählen die Glyphen des Formulars zum Spiegel — in
  Stromreihenfolge, an der Stelle des `Do`, auch Formular im Formular:
  deckungsgleich bleibt still, ein lügender Spiegel wird gelesen, gemeldet
  **und** beim Schwärzen der Formularglyphen geleert. Die Warnung nennt den
  Suchlauf nur, wenn er stattfand. Tests in `zb_spiegel_luegt.rs`; Mutation „Formen des Abschnitts leer“: rot, Mutation
  „Formen beim Leeren nicht mitzählen“: rot.
* **`--check-leaks`, Sicht 7: kein quadratischer Rückfall mehr.** Schlug die
  Extraktion an *einer* Seite fehl, las der Rückfall jede Seite einzeln — und
  baute je Seite den Seitenbaum neu (quadratisch; „8,7 s je Seite“ im
  Kommentar war die Zeit des ganzen Dokuments). `PdfExtractor::extract_lenient`
  liest jetzt in **einem** Durchgang und überspringt nur die abgelehnte Seite,
  mit ihrer Nummer in der Warnung. `extract_page` entfällt (API-Bruch in
  `redact-pdf`; die unbedingte Seitenschleife lässt sich damit nicht mehr
  schreiben). Gemessen mit zählendem Allokator: 3 200 Seiten fordern je Seite
  1,02× so viel Speicher an wie 50 Seiten — vorher 3,58×
  (`zb_rueckfall_linear.rs`).
* **Filterketten: der `/DecodeParms`-Eintrag gehört zum Filter, nicht zur
  Kette.** Der eigene Dekoder nimmt die vorderen Filter einer Kette selbst
  und reicht den Rest an `lopdf` — bisher mit dem **ungekürzten**
  `/DecodeParms`-Array, dessen Index dann nicht mehr zum Restfilter passte,
  und ohne Verweise aufzulösen. `[/ASCIIHexDecode /FlateDecode]` mit
  Prädiktor und ein `/DecodeParms 5 0 R` wurden still ohne Prädiktor dekodiert
  — die Sicht sah Rauschen, nicht den Text. Jetzt wird der Eintrag des ersten
  Restfilters aufgelöst (Liste, Eintrag, Verweise) und `lopdf` bekommt genau
  diesen als einzelnes Dictionary — so, wie es ihn liest.
* **Eine Seitennummer über 4 294 967 295 wird beim Lesen abgelehnt.** `lopdf`
  nummeriert Seiten als `u32`; `redact_core::model::MAX_PAGE_INDEX` bindet
  `Region.page` und `BlockedRegion.page` in Review-Dateien und hinter
  `--manual-regions` daran (Rückgabewert 1, „… ist keine Seitenzahl“; eine Ausgabedatei
  entsteht nicht). Vorher: `"page": 18446744073709551615` ließ den
  Debug-Build mit Rückgabewert 101 abstürzen (`p + 1` im Audit-Log); der
  Release-Build schrieb mit Rückgabewert 0 eine Ausgabe und warnte vor
  „Seite 0“. Jede 1-basierte Seitenanzeige rechnet zusätzlich mit
  `saturating_add`, in der Kette wie in der Oberfläche. `"page": 99` in einem
  Einseiter bleibt, was es war: eine Warnung (`missing_page`), kein Fehler.
  Gemessen im Test
  (`hostile_field_values_in_a_valid_review_file_end_with_a_message_not_a_panic`,
  Dev-Profil, `--apply-review` und `--manual-regions`) und im Release-Bau von
  Hand: kein Rückgabewert 101, keine Ausgabedatei. Der Test selbst läuft nur
  im Dev-Profil; die Release-Messung steht nicht im Baum.
* **Die Nachprüfung der Oberfläche deckelt Begriffe, nicht Bytes.** Die Decke
  davor rechnete Begriffe × Dateibytes auf der Platte gegen 2 GiB — die
  falsche Einheit: gesucht wird in den *entpackten* Strömen. Gemessen ließ sie
  an derselben Datei, gepackt, 11 683 statt 1 916 Begriffe zu, bei gleichen
  Kosten je Begriff (≈ 0,9 ms): rund 11 s statt der versprochenen 2 s. Jetzt
  gilt dieselbe Zahl wie für `--check-leaks` (`redact_core::MAX_CHECK_NEEDLES`,
  1 000); die Bytes deckelt weiterhin `--max-decompressed-mb`. Was über der
  Decke liegt, nennt die Statuszeile mit der Zahl. — Ein Text in zwei
  geschwärzten und einer abgewählten Zeile galt als Leck; jetzt fällt je Text
  **eine** Entscheidung, gleich wie viele Zeilen ihn tragen (Mutation
  `seen`-Menge entfernt: rot). — Seitenzahlen aus einer Review-Datei laufen in
  den Anzeigen der Oberfläche nicht mehr über.
* **`za_objektspeicher_gerechnet` rechnete mit Literalen.** „2·160 + 3·160“
  stand als Zahl im Test; `OBJEKT_BYTES`/`ARRAY_BYTES` waren privat, und bei
  155 im Code blieb der Test grün und druckte 800, wo der Code 775 rechnet.
  Beide Konstanten sind jetzt `pub`, der Test rechnet aus ihnen; `OBJEKT_BYTES
  = 155`: rot.

* **Der Windows-Job der CI war rot — durch einen Test, nicht durch das
  Programm.** `the_process_is_no_longer_dumpable` verlangte auf jedem System
  `Disabled`; unter Windows liefert `deny_core_dumps` ehrlich `Unavailable`
  (siehe „Kein Kernabzug“ oben), und genau das hielt der Test für einen
  Fehler. Er erwartet jetzt je Ziel, was die Funktion dort liefern *kann*:
  Linux streng `Disabled` samt Gegenprobe beim Kernel (`PR_GET_DUMPABLE`),
  übrige Unix „nicht `Failed`“ (`Unavailable` unter einem Syscall-Filter ist
  legitim), Windows genau `Unavailable`. Die Funktion selbst ist unverändert.
  Der Windows-Job ist per `workflow_call` das Tor vor dem Release; solange er
  rot war, gab es keines. Dahinter lag noch ein zweiter Stolperstein desselben
  Laufs: ein `mut` in `tests/cli.rs`, das nur der Unix-Zweig (Symlink,
  Hardlink) braucht und das `clippy -D warnings` für das Windows-Ziel als
  überflüssig ablehnte — jetzt an `cfg(not(unix))` gebunden erlaubt.
* **Die Decke von 1 000 Begriffen ist eine Zahl, nicht fünf.** Sie lag als
  `MAX_NEEDLES` in der Kommandozeile und als Literal in `--help`, README,
  SECURITY.md und diesem Verlauf; der Test dazu war aus der Konstante
  abgeleitet und hätte jeden Wert durchgewinkt. Jetzt heißt sie
  `redact_core::MAX_CHECK_NEEDLES`, die Oberfläche deckelt ihre Nachprüfung
  damit (siehe oben), und `the_needle_ceiling_is_one_number_in_code_help_and_docs`
  liest Hilfetext und die drei Dokumente und verlangt an jeder Stelle die
  Zahl aus der Konstante — samt der zitierten Fehlermeldung. Konstante auf
  500 gesetzt: rot. Zahl in der README geändert: rot.
* **`docs/pruefung.txt` zählte Fundstellen aus einer älteren Fassung.** Die
  Belege waren vor der siebten Sicht von `--check-leaks` erzeugt; seither
  findet der Lauf je Begriff eine Fundstelle mehr, und die Datei sagte es
  nicht. Sie ist neu erzeugt, und **`crates/redact-cli/tests/belege.rs`**
  hält sie am gebauten Binary fest: `--write-demo`, schwärzen, `--check-leaks`
  mit den vier Begriffen aus `scripts/make-preview.sh` — die
  `GEFUNDEN (N …)`/`nicht gefunden:`-Zeilen und die Rückgabewerte müssen
  denen in `pruefung.txt` gleichen. `scripts/check-preview.py` prüft
  zusätzlich, dass `docs/vorher-nachher.md` aus `pruefung.txt` wortgleich
  zitiert. Eine geänderte Zahl an einer der drei Stellen: rot.
* **`scripts/make-preview.sh` löst ein relatives `CARGO_TARGET_DIR` auf.**
  `CARGO_TARGET_DIR=target ./scripts/make-preview.sh` baute nach
  `<repo>/target` und suchte die Programme dann von einem Wegwerfverzeichnis
  aus unter `target/` — Abbruch mit 127. Ein relativer Wert wird jetzt gegen
  das Repository aufgelöst, bevor das Skript das Verzeichnis wechselt.
* **Doku.** Die Nachprüfung der Oberfläche heißt `leaks_many`, nicht `leaks`;
  ihre Decke ist oben in der richtigen Einheit beschrieben; „über vier
  Stunden“ für eine Million Begriffe war der Wert vor `memmem` — heute sind es
  über eine Stunde, und die Messreihe, aus der das folgt, steht an
  `redact_core::MAX_CHECK_NEEDLES` (die Zahl je Begriff stand hier ohne Weg und
  ist gestrichen, nicht verschoben); `SECURITY.md` sagt an der
  Grenzentabelle, dass MB dort wie überall 1024² Byte heißt und MiB dieselbe
  Einheit ist; README und `SECURITY.md` nennen die zwei Rollen der drei
  Spiegel-Schlüssel (`/ActualText` muss den Glyphen gleichen, `/Alt` und `/E`
  dürfen abweichen) und den `/Alt` eines Bildes als blinden Fleck.
* **Nicht in dieser Runde: der Export selbst in den Hintergrund.** Der Export
  lief damals weiter im Zeichentakt der Oberfläche; nur die Nachprüfung danach
  war im Hintergrund. Ein `PendingExport` mit gesperrter Bedienung,
  Fehlerkanal und verketteter Nachprüfung berührt dieselben Dateien, an denen
  diese Runde die Decke und die Doppelentscheidung korrigiert — das kam als
  eigener Schritt, nicht nebenbei.

  **Nachgetragen in der Fix-Runde 9:** dieser Schritt ist in der Runde 8 getan,
  und der Satz stand trotzdem noch im Präsens da — im selben Abschnitt
  `## Unveröffentlicht`, aus dem die Release-Notizen geschnitten werden. Was
  heute im Zeichentakt bleibt, ist nur noch der Abzug; die Zahlen stehen im
  Abschnitt der Runde 9.
* **Bewusst offen: der `/Alt` eines Bildes.** Beschreibt ein getaggtes PDF
  ein Bild mit `/Figure <</Alt (…)>> BDC /Im0 Do EMC` und steht in der
  Beschreibung, was auf dem Bild zu lesen ist, überlebt sie die
  Pixel-Schwärzung des Bildes. Die Analyse liest den `/Alt` eines Bildes so
  wenig wie dessen Pixel — derselbe blinde Fleck, jetzt benannt statt
  verschwiegen; `--check-leaks` sieht ihn im Rohstrom.

### Die CI lässt Clippy jetzt auch unter Windows laufen

`cargo clippy --workspace --all-targets -- -D warnings` war für das
Windows-Ziel rot (`variants Disabled and Failed are never constructed` in
`dumpable.rs`: dort entsteht ohne `prctl`/`setrlimit` nur `Unavailable`), und
niemand sah es, weil der Windows-Job nur baute und testete. Das Enum behält
seine drei Zustände — sie sind die Wahrheit über Linux und die übrigen
Unix-Systeme —, die Ausnahme ist an `cfg(not(unix))` gebunden, und der
Windows-Job fährt jetzt denselben Clippy-Schritt wie der Linux-Job. Lokal:
`cargo clippy --workspace --all-targets --locked --target x86_64-pc-windows-gnu -- -D warnings`
(nur das Target, kein Linker nötig).

Dazu drei Kleinigkeiten mit Verhalten: `.gitignore` deckt jetzt auch
`Kontoauszug_geschwaerzt.PDF` (die Endung wird von der Eingabe übernommen,
und `git` vergleicht schreibweisenempfindlich); `scripts/make-preview.sh`
liest `CARGO_TARGET_DIR` auch für den Pfad zu den gebauten Programmen, statt
nach dem Bau an anderer Stelle mit 127 abzubrechen; und `--help` rückt die
Kommentarzeilen „Grafische Oberfläche“ und „Beispieldatei …“ wie alle anderen
ein und nennt alle sechs Schlüssel der Einstellungsdatei sowie die Decke von
1 000 Begriffen je `--check-leaks`-Lauf.

### ⚠ Die README beschrieb ein Audit-Log, das es so nicht mehr gibt

Seit 0.6.0 hat `effect` einen **fünften** Befund, `off_page`. Die README nannte
weiter „vier Befunde“ und die Gleichung
`applied + covered + degenerate + missing_page = requested`. **An dieser
Gleichung rechnet ein Prüfer nach, ob er alle wirkungslosen Regionen gesehen
hat** — und sie ging nicht mehr auf. Gemessen an der Demo mit drei manuellen
Regionen (eine wirksam, eine neben dem Blatt, eine auf einer Seite, die es
nicht gibt): `requested 3, applied 1, covered 0, degenerate 0, missing_page 1,
off_page 1`, also `1 + 0 + 0 + 1 = 2 ≠ 3`. Genau eine wirkungslose Region wäre
unbemerkt geblieben.

`off_page` kam in README **und** `SECURITY.md` null mal vor. Beide nennen ihn
jetzt: die Befundtabelle hat eine fünfte Zeile, die Gleichung fünf Summanden,
der Beispiel-Auszug das Feld, und `SECURITY.md` warnt ausdrücklich davor, die
vierteilige Fassung aus 0.5.0 weiterzubenutzen.

### ⚠ `SECURITY.md` kannte `--check-leaks` nicht

Der Schalter kam dort null mal vor. Der Abschnitt zum Rückgabewert 3 nannte nur
die Stapelbedeutung („verarbeitet, aber nicht vollständig geprüft“) — wer ihn
in ein Skript übernahm, deutete einen **Leckfund** als bloß unvollständige
Prüfung. Der `--help`-Text des Binaries nennt seit 0.4.0 beide Fälle; jetzt tut
es `SECURITY.md` auch, mit einem eigenen Abschnitt zum Schalter.

Dort stand außerdem „Verbindlich ist `redact_pdf::leaks`“ — das ist die
Bibliotheksfunktion und damit gerade nicht das, was jemand ohne Quelltext
bedienen kann. Der Satz nennt jetzt beide Wege und sagt, dass es dieselbe
Funktion ist.

### ⚠ Kein Kernabzug mehr — und die Zusage zu `unsafe` ist enger geworden

`redact-rs` schaltet Kernabzüge als **allererste** Anweisung in `main` ab
(`prctl(PR_SET_DUMPABLE, 0)`). Ohne das schrieb der Kernel bei einem Absturz
den gesamten Arbeitsspeicher weg — samt Klartext des Dokuments und, bei einer
verschlüsselten Datei, dem Passwort.

Am **gebauten Binary** nachgemessen (`core_pattern = core`,
`ulimit -c unlimited`, Prozess mit `SIGABRT` beendet): ein `sleep` als
Kontrolle hinterlässt im selben Aufruf eine 454 656 Byte große `core`-Datei,
`redact-rs` **keine**; die Shell meldet nur noch `Aborted` statt
`Aborted (core dumped)`, bei unverändertem Rückgabewert 134.

**Die Zusage `#![forbid(unsafe_code)]` gilt jetzt für sieben statt acht
Crates.** `redact-cli` steht unter `#![deny(unsafe_code)]` mit **einer**
benannten Ausnahme an der `prctl`-Funktion. `SECURITY.md` sagte weiter „in
allen acht“ und gab dazu ein `grep`, das seither eine Datei weniger findet —
beides ist berichtigt, und die Ausnahme wird dort jetzt **beziffert** (drei
`unsafe`-Blöcke in einer Datei: `prctl` für Linux, `setrlimit` für die übrigen
Unix-Systeme — je Bau entsteht nur einer davon — und die Gegenprobe im Test,
die zurückliest, ob der Kernel den Zustand übernommen hat) statt behauptet.
Gezählt wird das Attribut `#[allow(unsafe_code)]`, nicht das Schlüsselwort:
ein `grep` nach `unsafe {` traf auch seine eigene Erklärung im Modulkommentar.

Ausgeschrieben steht dort auch, **was der Schutz nicht leistet**: unter Windows
ist nichts umgesetzt (das Mittel dort, `WerAddExcludedApplication`, wird nicht
aufgerufen; ein Abbild aus einem fremden Prozess per `MiniDumpWriteDump`
bliebe ohnehin) — ausgerechnet die Plattform der Zielgruppe —, und unter macOS, BSD und
den übrigen Unix-Systemen gibt es nur das schwächere Mittel
`setrlimit(RLIMIT_CORE, 0)`: eine Grenze, kein Verbot. Die Funktion liefert
drei Antworten statt `true`/`false`; „hier gibt es kein Mittel“ ist keine
Warnung, „der Aufruf schlug fehl“ schon.
Dazu der Nebeneffekt — **unter Linux**, denn nur dort läuft
`prctl(PR_SET_DUMPABLE, 0)`: der Prozess ist danach für `ptrace` durch denselben
Benutzer unerreichbar, `gdb` und `strace` brauchen `root`. Auf den übrigen
Unix-Systemen setzt derselbe Aufruf nur `RLIMIT_CORE` und lässt `ptrace`
unberührt.

### Die Oberfläche prüft nach dem Export selbst nach

Neu: nach jedem Export liest die Oberfläche die geschriebenen Bytes zurück und
sucht darin mit `redact_pdf::leaks_many` die Texte, die sie gerade geschwärzt hat.
Das ist die **stärkere Fassung** von `--check-leaks`, weil die Oberfläche
etwas hat, was der Kommandozeilennutzer nicht hat: sie kennt die Suchbegriffe
schon (in jeder geschwärzten Zeile steht der gefundene Text) und muss sie
weder tippen lassen noch in Prozessliste und Shell-Historie schreiben. Für die
Zielgruppe, die per Doppelklick arbeitet, war die Nachprüfung bis hierher gar
nicht erreichbar.

Drei Dinge stehen in der Zeile, die dabei entsteht — die ersten beiden immer,
das dritte, sobald es mindestens ein solches Rechteck gibt:

* das Ergebnis — bei einem Fund zusätzlich ganz vorn in den Warnungen;
* **derselbe Vorbehalt wie in der Kommandozeile**: geprüft ist *diese Liste*,
  nicht die Datei;
* **die Zahl der Rechtecke ohne bekannten Text**, wenn sie größer als null
  ist. Ein selbst gezogenes Rechteck hat keinen; darüber kann die Prüfung
  nichts sagen, und dort bleibt es bei der Sichtprüfung. Verschwiegen wäre
  die neue Anzeige an einem Dokument mit lauter Handregionen selbst eine
  falsche Entwarnung.

Gesucht werden höchstens 1 000 verschiedene Texte — dieselbe Decke wie bei
`--check-leaks`, dieselbe Konstante (`redact_core::MAX_CHECK_NEEDLES`) und
dieselbe Einheit: **Begriffe**, nicht Bytes. (Eine Zwischenfassung deckelte
das Produkt aus Begriffen und Dateibytes; die Kosten hängen aber an den
*entpackten* Streambytes, und die kennt niemand vor dem Lauf. Die sind durch
`--max-decompressed-mb` gedeckelt, die Begriffe hier.) Was über der Decke
liegt, wird gesagt und nicht verschwiegen.

### Drei Ungenauigkeiten der Oberfläche

* **Eine deckungsgleiche Handregion konnte den Platz des wirklich blockierten
  Mustertreffers verbrauchen.** `BlockedRegion` trägt nur Seite und Rechteck,
  nicht die Herkunft; zweimal Strg+R legt zwei buchstäblich gleiche Regionen
  an. Lag dort ein von der Negativliste gedeckter Mustertreffer derselben
  Fläche, stand an einem **selbst gezogenen** Rechteck „geschützt durch Ihre
  Liste“ (falsch — Handregionen überstimmen die Liste) und der wirklich
  blockierte Treffer hieß „doppelt“. Der Zeiger fragt jetzt zusätzlich, ob die
  Zeile überhaupt blockierbar ist. **Der Export war nie betroffen.**
* **Die Absage bei Strg+Pfeil sprach vom Verschieben.** Wer die Größe ändern
  wollte und dabei auf der falschen Seite stand, las „Nicht verschoben …“ und
  suchte den Fehler an der falschen Stelle. Jetzt nennt sie das Verb, um das es
  ging.
* **Dieselbe Absage riet, zu Seite 8 zu blättern** — bei einem zweiseitigen
  Dokument. Eine Zeile auf einer Seite, die es nicht gibt, kann nur eine
  Review- oder Regionsdatei mitbringen; die Absage nennt jetzt die Seitenzahl
  des Dokuments und einen gangbaren Ausweg.

### Doku: drei weitere Stellen, an denen sie hinter dem Code stand

* **Die Tastentabelle** kannte weder `Strg+R` noch `Strg+Pfeil`, und
  `Umschalt+Tab` stand nirgends in der Datei. Der Abschnitt „Rechtecke
  ziehen …“ sagte weiter „Ein neuer Bereich entsteht durch Aufziehen mit der
  Maus“ — für den Nutzer, für den v0.5.0 den mauslosen Weg gebaut hat, war das
  die falsche Auskunft. Beide Wege stehen jetzt nebeneinander, samt dem Knopf
  „🔲 Rechteck“, den die README überhaupt nicht kannte.
* **Die neuen Decken standen nirgends.** `MAX_TO_UNICODE_BYTES` (32 MB),
  `MAX_CACHED_FONT_ENTRIES` (400 000 Tabelleneinträge) und die
  MediaBox-Heilung („A4 angenommen“) kamen in README, `SECURITY.md` und dieser
  Datei je null mal vor — obwohl der Kopf zu 0.6.0 die Decken zur Hauptsache
  der Fassung erklärt. Die Grenzentabelle der README hat jetzt zwei weitere
  feste Grenzen, jede mit dem Lauf, aus dem die Angabe stammt: eine 1 064 Byte
  kleine Datei mit drei `bfrange`-Blöcken über je 65 536 Codes (rund 50 MB)
  reißt die erste Decke und endet mit „NICHT GEPRÜFT“ und Rückgabewert 3;
  dieselbe Datei mit einem Block (rund 16,8 MB) läuft mit 0 durch.
* **„Erfasst sind alle Änderungen“** nannte sechs; `history.record` steht an
  zehn Stellen. Es fehlten Größe ändern, Schwärzungsart, Ersatztext — und
  **Analysieren**. Die Liste ist jetzt vollständig und sagt gemessen, was
  Analysieren wirklich wegwirft: selbst gezogene Rechtecke trägt es hinüber,
  die Entscheidungen an den Mustertreffern nicht.

### Der Pfad des Build-Rechners ist jetzt vollständig draußen

Der Eintrag zu 0.5.0 hielt fest, dass im glibc-Artefakt weiterhin **drei**
Pfade des Build-Rechners stehen — Build-Skript-Ausgaben unter `OUT_DIR`, die
der eine `--remap-path-prefix` nicht erfasst — und dass die Behebung im Bauweg
aussteht. Sie ist erledigt: `RUSTFLAGS` trägt jetzt einen **zweiten** Prefix
für `${CARGO_TARGET_DIR:-$PWD/target}`.

**Gemessen, bevor die Zeile geschrieben wurde**, und mit einem auffälligen
Ersatzpfad, damit das Ergebnis nicht zu verwechseln ist:

```console
$ RUSTFLAGS="--remap-path-prefix=$CARGO_HOME/registry/src=/cargo-registry \
             --remap-path-prefix=$PWD/target=/ZWEITER-PREFIX" \
    cargo build --release --locked -p redact-cli
$ strings -a target/release/redact-rs | grep -c /home/user/redactrs
0
$ strings -a target/release/redact-rs | grep ZWEITER-PREFIX | sort -u
/ZWEITER-PREFIX/release/build/glutin_egl_sys-…/out/egl_bindings.rs
/ZWEITER-PREFIX/release/build/glutin_glx_sys-…/out/glx_bindings.rs
/ZWEITER-PREFIX/release/build/glutin_glx_sys-…/out/glx_extra_bindings.rs
```

Vorher standen an genau diesen drei Stellen dieselben Zeilen mit dem echten
Pfad des Arbeitsbaums — belegt am selben Baum mit nur einem Prefix
(`grep -c /home/user/redactrs` ⇒ **3**). Auch `redact-rs-gui` ist danach bei 0.

**Was hier nicht steht:** eine Zusage über das *veröffentlichte* Artefakt.
Gemessen ist ein lokaler Bau mit derselben Toolchain (1.94.1) und denselben
Schaltern; ob der Release-Job dasselbe liefert, zeigt erst der nächste Lauf.
Genau diese Zusage war im Eintrag zu 0.4.0 schon einmal zu weit gefasst —
deshalb diesmal die Trennung zwischen „gemessen“ und „zugesagt“.

---

### Und am Code: die Fehlerklasse, die diese Runde beim Namen nennt

Vierter und letzter Durchgang der vereinbarten Schleife. Die Fehlerklasse
dieser Runde steht schon im Eintrag zu 0.6.0 zwischen den Zeilen, hier wird
sie ausgesprochen: **eine Zusicherung, die an ihrer Ursprungsstelle stimmt,
wird an der nächsten stillschweigend mitgenommen.** Dreimal dasselbe Muster —
und dreimal war die Abhilfe, die Zahl dort zu prüfen, wo sie *neu entsteht*,
nicht dort, wo sie hereinkommt.

### ⚠ Kein Kernabzug mehr

Stürzte der Lauf ab, schrieb der Kernel den ganzen Arbeitsspeicher auf die
Platte — mit dem Klartext des Dokuments und, bei einer verschlüsselten Datei,
dem Passwort. Der Absturz ließ sich über eine präparierte Eingabedatei
auslösen.

Gemessen am laufenden Programm (`core_pattern = core`, `ulimit -c unlimited`,
eine IBAN im Speicher, dann Absturz):

| | Abzugsdatei | die IBAN darin |
|---|---|---|
| 0.6.0 | 487 424 Byte | **3 Fundstellen** |
| 0.7.0 | keine | — |

`prctl(PR_SET_DUMPABLE, 0)` als erste Anweisung beider Binärziele. Zwei
Dinge, die dazugehören und nicht im Kleingedruckten stehen sollen:

* **Unter Windows ist nichts umgesetzt** — das Mittel dort
  (`WerAddExcludedApplication`) ruft das Programm nicht auf, und einem Abbild
  aus einem fremden Prozess entzöge es sich ohnehin nicht. Diese Absicherung
  schützt Linux (`prctl`) und, schwächer, macOS-artige Systeme
  (`setrlimit(RLIMIT_CORE, 0)`), also nicht die Plattform, auf der die
  meisten Nutzer dieses Werkzeugs sitzen.
* Der Prozess ist danach auch für `ptrace` durch denselben Benutzer
  unerreichbar. Für ein Werkzeug, das Kontoauszüge im Speicher hält, ist das
  die richtige Richtung; wer mit `gdb` an einem Fehler arbeitet, braucht
  dafür `root`.

Damit steht `redact-cli` unter `#![deny(unsafe_code)]` statt `forbid`, mit
**einer** benannten Ausnahme an genau der Funktion, die den `prctl`-Aufruf
enthält. Die übrigen sieben Crates bleiben unter `forbid`. `libc` lag über
`lopdf → getrandom` ohnehin in jedem Bau; es kommt keine Abhängigkeit hinzu.

### ⚠ NaN und Überlauf: ein Rechteck ohne brauchbare Koordinaten

`--padding nan` genügte, um **jede** Schwärzung wirkungslos zu machen — und
der Lauf meldete trotzdem „Deck-Rechteck gezeichnet". In der Ausgabe stand
dann `NaN NaN NaN NaN re f`. Ein NaN-Rechteck überdeckte rechnerisch *alles*
(`intersection_area` lieferte die volle Fläche), und im `dedup` verschluckte
es damit den echten IBAN-Treffer.

Schwerer: aus **endlichen** Koordinaten konnte `∞ − ∞` entstehen. Der
Wächter des Rechteckgitters prüfte `spanne > 64`, und das ist für `NaN`
falsch — die Schleife lief danach bis `i64::MAX`. Gemessen an einem
präparierten PDF: **vorher Abbruch nach 90 s, jetzt 0,01 s bei 9,1 MB.**

Die Regel steht jetzt einmal (`Rect::is_usable`), und dort, wo eine Zahl neu
entsteht, wird sie neu geprüft — am **Ergebnis**, nicht an der Eingabe.

### ⚠ Der Speicher, den eine Datei vor jedem Scan kostet

`--max-parsed-mb` zählte nur die entpackten Ströme. Der Rest der Datei —
Objekte, Verzeichnisse, Namen — kostet gemessen **197 bis 274 Byte je
Dictionary-Eintrag**, unabhängig von seiner Schreibweise. Eine dicht
geschriebene 44-MB-Datei ergab damit ein 1 804 MB großes Dokument, und der
Lauf endete mit Rückgabewert 0.

| Datei | 0.6.0 | 0.7.0 |
|---|---|---|
| 44,3 MB, dichte Verzeichnisse | Exit 0, **3 756 MB**, 12,6 s | Exit 1, 50 MB, 0,35 s |
| 49,9 MB, Verzeichnisse | Exit 0, 3 197 MB, 11,3 s | Exit 1, 56 MB, 0,44 s |
| 81,7 MB, nur Zahlen | Exit 0, 670 MB, 2,6 s | Exit 1, 89 MB, 0,61 s |

Dieselbe Bombe in einem *komprimierten* Objekt-Strom wurde seit jeher
abgelehnt; der Unterschied war allein, ob sie komprimiert war — und das
sucht sich ein Angreifer als Erstes aus. Gegenprobe an gewöhnlichen Dateien:
ein 2 000-Seiter mit 100 000 Treffern braucht 11 MB des 16-MB-Budgets, ein
300-seitiger Scan mit 600 MB Bilddaten 1 MB.

**Und die zweite Decke in der zweiten Einheit.** Die Rumpfbuchung allein
reichte nicht: eine Datei aus 7 968 000 **leeren Arrays**, 16 761 888 Byte und
damit knapp *unter* dem Budget, lief durch — mit 5,6 GB Spitzenspeicher.
Denn der Aufblähfaktor hängt nicht an der Dateigröße, sondern an der
**Form**: gemessen 274-facher Unterschied bei identischer Größe, je nachdem,
ob die Bytes Zeichenkette, Dictionary oder leeres Array sind. Ein Faktor lässt
sich daraus nicht schätzen — deshalb wird der Objektspeicher jetzt beim Lauf
über die Rohbytes **gerechnet** (je Objekt, je Array) und bei
`max_parsed_bytes × 60`, in der Vorgabe **960 MB**, abgelehnt. Dass die
Rechnung nie unter dem wirklich belegten Speicher liegt, hält ein eigener
Test fest (im engsten Fall 2 % darüber). Dieselbe Datei: **vorher rc 0 und
5,6 GB, jetzt rc 1 und 21 MB in 0,1 s.**

### Die Nachprüfung entpackt und parst die Datei nur noch einmal

`--check-leaks` packte je Suchbegriff **die ganze Datei neu aus** und parste
den Objektgraphen neu. Diese Arbeit hängt an der Datei, nicht am Begriff, und
fällt jetzt einmal an. Der Vergleich selbst läuft weiter je Begriff über die
ganze Datei; oberhalb der Tabelle wächst die Laufzeit damit weiter mit der
Zahl der Begriffe.

| 792-kB-Datei | 0.6.0 | 0.7.0 |
|---|---|---|
| 1 Begriff | 0,13 s | 0,11 s |
| 10 Begriffe | 0,88 s | 0,25 s |
| 40 Begriffe | — | 0,69 s |

`redact_pdf::leaks(bytes, begriff)` bleibt unverändert und benutzt intern
denselben Durchgang. Neu ist eine Obergrenze von 1 000 Begriffen: die
Byte-Grenze allein ließ rund eine Million Zeilen zu, was über eine Stunde
Laufzeit ergäbe (rund 4 ms je Begriff, gemessen an einer 898-kB-Datei mit
420 Seiten — die Messung steht an `redact_core::MAX_CHECK_NEEDLES`) — von
außen nicht von einem Hänger zu unterscheiden.

### Der Beleg, den man ansehen kann

`docs/vorher-nachher.md` zeigt dieselbe Seite vor und nach dem Lauf,
gerendert vom Rasterizer dieses Programms, daneben die vollständigen
`--check-leaks`-Läufe. Erzeugt von `./scripts/make-preview.sh`, bitgleich
reproduzierbar.

Bewusst nicht geschönt: „Kontoinhaber: Max Mustermann" steht im rechten Bild
**unverändert da**, weil es für Namen kein Muster gibt. Und `pdftotext`
steht dort als *Warnung* — mit Rückgabewert 1 („kein Treffer") direkt neben
dem Lauf, der in derselben Datei noch etwas findet.

### An der Fassungsgeschichte selbst

Der Commit-Verlauf wurde einmalig umgeschrieben: 67 Nachrichten trugen eine
Sitzungs-URL des Werkzeugs, mit dem sie entstanden sind. Sie sind entfernt;
die `Co-Authored-By`-Zeilen bleiben, weil sie eine wahre Angabe sind.

**Folge, die genannt gehört:** jeder Commit-SHA hat sich geändert, auch die
der Tags `v0.1.0` bis `v0.6.0`. Der *Inhalt* ist an jedem Tag byteidentisch
geblieben (nachgeprüft am Baum-Hash), die veröffentlichten Artefakte und
Release-Notizen sind unberührt. Die Build-Provenienz der Releases 0.4.0 bis
0.6.0 nennt aber weiterhin die **alten** SHAs; wer sie gegen den heutigen
Verlauf prüft, findet dort eine Abweichung, die keine inhaltliche ist.

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
