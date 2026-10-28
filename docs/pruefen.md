<a id="pruefen"></a>

# Prüfen, ob die Schwärzung gewirkt hat

## `pdftotext … | grep …` taugt dafür nicht

Diese Anleitung stand früher an dieser Stelle. Sie ist entfernt worden, weil sie
falsche Entwarnung gibt — nachgemessen an der eigenen Demo-Ausgabe:

```console
$ redact-rs --write-demo kontoauszug.pdf
$ redact-rs kontoauszug.pdf --patterns iban_de,bic,email
$ pdftotext kontoauszug_geschwaerzt.pdf - | grep DE89
$ echo $?
1                        # „kein Treffer“ — sieht sauber aus
```

In derselben Datei stehen zu diesem Zeitpunkt noch:

```console
$ pdftotext kontoauszug_geschwaerzt.pdf -
…
Kontoinhaber: Max Mustermann
…
Kontonummer: 532013000
…
Telefon: +49 30 123456789
Steuer-ID: 12345678901
```

Zwei getrennte Fehler stecken darin:

1. **`grep` prüft genau eine Zeichenkette.** Was kein Muster erkannt hat —
   Namen grundsätzlich, hier zusätzlich Kontonummer, Telefonnummer und
   Steuer-ID, weil `--patterns iban_de,bic,email` sie ausgeschlossen hat —
   steht unangetastet in der Datei und wird von einem `grep DE89` nie berührt.
   (Ohne `--patterns` greifen Kontonummer, Telefon und Steuer-ID sehr wohl;
   der Name bleibt trotzdem stehen.)
2. **`pdftotext` sieht nicht alles.** Es rendert Seitentext. Ein PDF kann
   dieselbe Zeichenkette an mehreren Stellen tragen, die dabei nicht vorkommen.
   Nachgemessen mit `poppler 24.02.0` an je einer winzigen Datei pro Versteck:

   | Geheimnis versteckt in … | `pdftotext` | `strings` | `--check-leaks` |
   |---|---|---|---|
   | `/ActualText` eines Struktur-Elements | **nicht gefunden** | gefunden | gefunden |
   | `/ActualText` als Marked-Content im Seiteninhalt | gefunden | gefunden | gefunden |
   | Erscheinungsstrom (`/AP`) einer Annotation | gefunden | gefunden | gefunden |
   | Formularfeld-Wert `/V` | **nicht gefunden** | gefunden | gefunden |
   | verwaistes Objekt (von nirgends referenziert) | **nicht gefunden** | gefunden | gefunden |
   | Objektstrom `/ObjStm` (Flate-komprimiert) | **nicht gefunden** | **nicht gefunden** | gefunden |

   Auch `strings` reicht also nicht: ein komprimierter Objektstrom ist für eine
   reine Rohbyte-Suche unsichtbar. Und beide Werkzeuge scheitern zusätzlich an
   der Kodierung — dieselbe IBAN kann als UTF-16BE oder als Hex-String
   `<44453839…>` in der Datei stehen.

   Die Tabelle misst, was die drei Werkzeuge *finden* — nicht, was redact-rs
   schwärzt. Beide `/ActualText`-Zeilen werden inzwischen mitgeschwärzt, siehe
   [Der Textspiegel im Seiteninhalt](grenzen.md#der-textspiegel-im-seiteninhalt).

## `--check-leaks`: die Prüfung steckt im Programm

**Ein Aufruf, kein zweites Werkzeug, kein Netzzugang.** Der Schalter nimmt die
Datei und die Texte, von denen niemand mehr etwas sehen soll:

```console
$ redact-rs kontoauszug_geschwaerzt.pdf \
      --check-leaks "DE89 3704 0044 0532 0130 00" \
      --check-leaks "Max Mustermann" \
      --check-leaks "532013000"
Geprüft: kontoauszug_geschwaerzt.pdf (1304 Byte)
  nicht gefunden: DE89 3704 0044 0532 0130 00
  GEFUNDEN (6 Fundstelle(n)): Max Mustermann
      Rohdaten-Stream @0x1e7 (inflate) [Inhalt, UTF-8/ASCII]: …2 750 Tm.(Kontoinhaber: Max Mustermann) Tj./F1 10 Tf.1 0 0 1 7…
      Rohdaten-Stream @0x1e7 (inflate) [Zeichenketten-Verkettung]: …ontoauszugKontoinhaber: Max MustermannIBAN:BIC:Kontonummer: 53…
      Objekt 11 0 <Stream, lopdf-dekodiert> [Inhalt, UTF-8/ASCII]: …2 750 Tm.(Kontoinhaber: Max Mustermann) Tj./F1 10 Tf.1 0 0 1 7…
      …
  GEFUNDEN (4 Fundstelle(n)): 532013000
      Rohdaten-Stream @0x1e7 (inflate) [Inhalt, UTF-8/ASCII]: …72 705 Tm.(Kontonummer: 532013000) Tj./F1 10 Tf.1 0 0 1 7…
      …

Ergebnis: 2 von 3 Suchbegriffen stehen noch in der Datei. Diese Datei ist nicht geschwärzt — sie darf so nicht weitergegeben werden. (Rückgabewert 3.)
$ echo $?
3
```

Gesucht wird auf allen Ebenen, auf denen ein Geheimnis überleben kann: rohe
Dateibytes, jeder roh gefundene `stream … endstream`-Block (auch
Flate-dekomprimiert, also inklusive Altrevisionen), jedes Stream-Objekt des
Objektgraphen dekodiert, die Objekte in `/ObjStm`-Containern und **jedes**
Zeichenketten-Objekt unter jedem Schlüssel — jeweils in **bis zu neun
Byte-Kodierungen**: UTF-8/ASCII, Latin-1/PDFDoc, UTF-16BE und UTF-16LE, dazu
jede der drei Bytefassungen (Latin-1, UTF-16BE, UTF-16LE) als Hex-String in
Groß- und in Kleinschreibung. Neun sind es für einen Begriff aus reinem ASCII
mit Buchstaben — dort fällt Latin-1 mit UTF-8 zusammen —, zehn mit Umlaut und
sieben mit einem Zeichen jenseits von Latin-1. Fassungen, die auf dieselben
Bytes fallen, werden nur einmal gesucht: bei einer IBAN aus Ziffern und `DE`
ist der Hex-String in Groß- und in Kleinschreibung dieselbe Bytefolge, dort
sind es sechs. Ein Begriff mit einem Zeichen, das WinAnsi anders ablegt als
Latin-1 (`€`, `–`, `„`), wird zusätzlich in WinAnsi gesucht, roh und als
Hex-String — so stehen Zeichenketten in einem Inhaltsstrom mit einer
Standardschrift. Im Zweifel meldet die Prüfung zu viel: dieselbe
Fundstelle erscheint einmal je Sichtweise. Ein Fehlalarm wird untersucht, ein
übersehenes Leck wird ausgeliefert.

Die Suche selbst ist dieselbe Funktion, die die Tests von `redact-pdf` als
Messgerät benutzen (`redact_pdf::leaks`, in
`crates/redact-pdf/src/audit_bytes.rs`); der Schalter reicht sie durch und baut
nichts nach.

### Der Rückgabewert

| Wert | heißt |
|---|---|
| `0` | Keiner der Begriffe steht noch in der Datei — **und** jede Stelle konnte geprüft werden. **Kein Freibrief** — siehe unten. |
| `3` | Mindestens einer steht noch darin — **oder** eine Stelle konnte nicht geprüft werden (`NICHT GEPRÜFT: …`, sieben mögliche Gründe, siehe unten), auch ohne einen einzigen Fund. Der Lauf ist gelungen, das *Ergebnis* nicht. |
| `1` | Verarbeitungsfehler: die Datei ist keine PDF-Datei, nicht lesbar, zu groß oder verschlüsselt. |
| `2` | Bedienfehler: kein Suchbegriff, mehr als eine Datei, oder ein Schalter, der nicht dazugehört. |

Ein Fund ist bewusst **kein** `1`: die Datei wurde gelesen, die Suche lief
vollständig durch, die Antwort steht fest — sie lautet nur „ja, es steht noch
drin“. Ein Skript unterscheidet damit drei Fälle, ohne die Ausgabe zu lesen:

```bash
redact-rs out.pdf --check-leaks "$IBAN"
case $? in
  0) echo "im Rahmen der geprüften Liste sauber" ;;
  3) echo "LECK — Datei zurückhalten"; exit 1 ;;
  *) echo "Prüfung ist gar nicht gelaufen"; exit 1 ;;
esac
```

### Die Suchbegriffe sind Geheimnisse

Auf der Kommandozeile stehen sie in der Prozessliste (`ps`) und in der
Shell-Historie und sind damit für andere Konten auf derselben Maschine lesbar —
dasselbe Problem wie beim Passwort, und dieselbe Antwort: **`-` liest sie
zeilenweise von der Standardeingabe.**

```bash
redact-rs geschwaerzt.pdf --check-leaks - < begriffe.txt
printf '%s\n' "$IBAN" "$NAME" | redact-rs geschwaerzt.pdf --check-leaks -
```

Nachgemessen: während des zweiten Laufs zeigt `ps -o args=` genau
`redact-rs geschwaerzt.pdf --check-leaks -` und keinen der Begriffe. Leere
Zeilen werden übergangen, Windows-Zeilenenden auch; ein Komma trennt **nicht**
(„Mustermann, Max“ ist ein Begriff, nicht zwei).

**Die Ausgabe der Prüfung ist dagegen nicht geheimnisfrei zu bekommen** und
gehört behandelt wie das Original: die geprüfte Liste steht darin (sonst ließe
sich das Ergebnis nicht nachprüfen), und jeder Fund zeigt seine Umgebung aus
der Datei (sonst wäre er nicht zu finden).

### Was `--check-leaks` nicht tut

Es schwärzt nicht und schreibt nichts. Deshalb lehnt es jeden Schalter des
Schwärzens ab, statt ihn wirkungslos mitlaufen zu lassen — `-o`, `--review`,
`--audit-log`, `--patterns`, `--action`, `--padding` und die übrigen enden mit
Rückgabewert `2`. Der Weg ist **zwei Aufrufe**, und das ist kein Umweg:

```bash
redact-rs auszug.pdf -o geschwaerzt.pdf
redact-rs geschwaerzt.pdf --check-leaks - < begriffe.txt
```

So prüft der zweite Aufruf nachweislich *die geschriebene Datei* und nicht
einen Zwischenstand im Speicher — was eine Kontrolle erst zu einer macht.

Erlaubt bleiben `--quiet` (ein sauberer Lauf schweigt dann ganz — nur der
Rückgabewert antwortet; **ein Fund wird trotzdem gemeldet**, denn er ist die
Nachricht, wegen der dieser Schalter existiert) und die Grenzen
`--max-input-mb`, `--max-decompressed-mb`, `--max-parsed-mb`; die greifen beim
Lesen und Vorprüfen jeder fremden Datei. Eine **verschlüsselte** Datei wird
abgelehnt statt durchsucht: darin stehen die Zeichenketten verschlüsselt, eine
Bytesuche fände auch dann nichts, wenn das Geheimnis noch darin steht — und
„nichts gefunden“ wäre hier die falscheste aller Antworten.

**Die Suche hat ein Budget, und sie sagt, was nicht hineinpasste.** Mehr als
`--max-decompressed-mb` (Vorgabe 1024 MB) packt sie in Summe nicht aus —
dieselbe Einheit wie beim Schwärzen, je Sicht der Suche (Rohsicht,
Objektsicht) einmal. Ein Strom, der das Restbudget sprengte, wird nicht
entpackt; er ist weder Fund noch „nicht gefunden“: er steht als
`NICHT GEPRÜFT: …` in der Ausgabe, und der Lauf endet **mit Rückgabewert `3`
auch ohne Fund** — „Ergebnis: 1 Stelle(n) nicht geprüft — die Antwort ist
unvollständig.“ Gezählt werden **Stellen**, nicht Zeilen: sind es sehr viele,
nennt die Liste die ersten einzeln und fasst den Rest in einer Summenzeile
zusammen, die Zahl im Ergebnissatz bleibt aber die volle.
Ein `--check-leaks … && versenden` verschickt so keine Datei,
deren größter Strom nie aufgemacht wurde. Wer die Stelle prüfen will, hebt den
Schalter. (Eine Datei, deren als Flate ausgewiesene Ströme *in Summe* über dem
Budget liegen, kommt gar nicht so weit: die Vorprüfung lehnt sie wie beim
Schwärzen mit Rückgabewert `1` ab, bevor irgendetwas ausgepackt wird.)

**Höchstens 1 000 Begriffe je Aufruf.** Nicht der Zeit wegen: seit Fix-Runde 4
trägt ein Automat alle Begriffe in allen Kodierungen und läuft **einmal** je
Datenblock — 1 000 Begriffe kosten kaum mehr als einer (nachgemessen an einer
64-MiB-Datei: 5,20 s für einen, 6,27 s für 1 000, Verhältnis 1,21). Die Decke
gilt dem Speicher: der Automat wächst linear mit der Liste, nachgemessen
1,0 MB für 1 000 Begriffe (bis zu 12 000 Muster: bis zu zehn Byte-Kodierungen
und der dekodierte Text je Begriff, dazu die Fassung ohne Leerraum, wo der
Begriff welchen trägt — neun Kodierungen sind es ohne Umlaut, zehn mit),
7,1 MB für 10 000, 68 MB für
100 000 und **675 MB** für eine Million — dazu 25 s allein für seinen Bau,
bevor ein Byte der Datei gelesen ist. Und jeder Begriff bekommt eine eigene
Zeile im Bericht. Der 1 001. Begriff endet deshalb mit Rückgabewert
`2`, bevor die Datei gelesen wird (nachgemessen: `seq 1 1001 | sed 's/^/x/' |
redact-rs beispiel.pdf --check-leaks -` meldet „mit 1001 Suchbegriffen; mehr
als 1000 nimmt der Lauf nicht an“; mit 1 000 Zeilen läuft derselbe Aufruf
durch). Wer mehr hat, teilt die Liste und ruft mehrmals auf — jeder Lauf
meldet für sich.

## Die Gegenprobe

**Erst sie gibt dem Ergebnis seinen Wert.** Dieselbe Demo, diesmal ohne
`--patterns`, also mit den Vorgabemustern — geprüft gegen alle fünf Werte:

```console
$ redact-rs kontoauszug.pdf -o std.pdf
$ redact-rs std.pdf \
      --check-leaks "DE89 3704 0044 0532 0130 00" \
      --check-leaks "532013000" \
      --check-leaks "+49 30 123456789" \
      --check-leaks "12345678901" \
      --check-leaks "Max Mustermann"
Geprüft: std.pdf (1332 Byte)
  nicht gefunden: DE89 3704 0044 0532 0130 00
  nicht gefunden: 532013000
  nicht gefunden: +49 30 123456789
  nicht gefunden: 12345678901
  GEFUNDEN (6 Fundstelle(n)): Max Mustermann
      …

Ergebnis: 1 von 5 Suchbegriffen steht noch in der Datei. …
$ echo $?
3
```

IBAN, Kontonummer, Telefonnummer und Steuer-ID sind restlos weg — auf allen
Ebenen, auf denen gesucht wird, nicht nur dort, wo `pdftotext` hinsieht. Übrig
ist der **Name**, und zwar genau so oft wie vorher: für Namen gibt es kein
Muster, und niemand hat ihn markiert. Das ist keine Schwäche der Prüfung,
sondern ihr Ergebnis.

**Und die Gegenprobe zur Gegenprobe** — der Fall, in dem `pdftotext` *und*
`strings` *und* `grep` auf der Rohdatei übereinstimmend Entwarnung geben: eine
IBAN in einem Flate-komprimierten Objektstrom (nachgemessen an einer 767 Byte
großen Datei, poppler 24.02.0):

```console
$ pdftotext objstm.pdf - | grep -F "DE89 3704 0044 0532 0130 00" ; echo $?
1
$ strings -a objstm.pdf | grep -F "DE89 3704 0044 0532 0130 00" ; echo $?
1
$ grep -c -F "DE89 3704 0044 0532 0130 00" objstm.pdf ; echo $?
0
1
$ redact-rs objstm.pdf --check-leaks "DE89 3704 0044 0532 0130 00" ; echo $?
Geprüft: objstm.pdf (767 Byte)
  GEFUNDEN (10 Fundstelle(n)): DE89 3704 0044 0532 0130 00
      Rohdaten-Stream @0x1e8 (inflate) [Inhalt, UTF-8/ASCII]: …6 0 (DE89 3704 0044 0532 0130 00)…
      Objekt 6 0 [Zeichenkette, literal]: …DE89 3704 0044 0532 0130 00…
      Objekt 7 0 <ObjStm> → Objekt 6 0 [Zeichenkette, literal]: …DE89 3704 0044 0532 0130 00…
      …

Ergebnis: der Suchbegriff steht noch in der Datei. Diese Datei ist nicht geschwärzt — sie darf so nicht weitergegeben werden. (Rückgabewert 3.)
3
```

Dieser Fall steht als Test im Baum
(`a_secret_in_a_compressed_object_stream_is_found` in
`crates/redact-cli/tests/check_leaks.rs`) — samt der Zusicherung, dass das
Geheimnis wirklich nicht im Klartext in der Datei steht.

## Und wogegen prüft man?

`--check-leaks` beantwortet die Frage „steht *dieser* Text noch in der Datei?“.
Die Liste der Suchbegriffe muss also von Hand entstehen: die Werte aus dem
Original, die nicht nach draußen dürfen. Zwei brauchbare Quellen dafür:

* das Feld `text` jedes Eintrags in `review.json` (dort steht genau das, was das
  Werkzeug gefunden hat) — und dazu die Werte, die dort **fehlen**;
* eine Sichtprüfung der Originalseiten.

**„Nichts gefunden“ ist deshalb kein Freibrief**, und das Werkzeug sagt es nach
jedem sauberen Lauf selbst:

```console
$ redact-rs std.pdf --check-leaks "DE89 3704 0044 0532 0130 00"
Geprüft: std.pdf (1332 Byte)
  nicht gefunden: DE89 3704 0044 0532 0130 00

Ergebnis: der Suchbegriff steht nicht mehr in der Datei.
Das heißt NICHT, dass in der Datei nichts mehr steht. Geprüft wurde genau diese
Liste. Was nicht darin steht — ein zweiter Name, eine weitere Kontonummer, eine
Schreibweise mit anderen Leerzeichen, Text in einem Rasterbild —, ist damit
nicht geprüft. Die Liste zu schreiben bleibt Handarbeit, und die Sichtprüfung
des Ergebnisses ersetzt sie nicht.
$ echo $?
0
```

Der Satz steht auf **stdout** und nicht auf stderr, damit
`redact-rs … --check-leaks … > bericht.txt` nicht genau die Hälfte behält, die
wie eine Freigabe aussieht.

Das ersetzt die Sichtprüfung des Ergebnisses nicht. Die Prüfung beweist, dass
eine bekannte Zeichenkette weg ist. Dass nichts *Unbekanntes* stehen geblieben
ist, kann sie nicht beweisen.

## „0 Schwärzungen“ ist kein Freibrief

Meldet ein Lauf `Schwärzungen: 0`, kann das heißen: in der Datei steht nichts
Schützenswertes. Es kann genauso heißen: der Text steckt in einem Rasterbild,
die Schrift benutzt eine Kodierung, die der Extraktor nicht auflösen konnte, das
Muster passt auf diese Schreibweise nicht, oder die Negativliste hat alles
blockiert. Siehe auch den gleichnamigen Abschnitt in [`SECURITY.md`](../SECURITY.md).

Für einen Teil dieser Fälle gibt es inzwischen ein Signal: Stellen, die die
Analyse nicht durchsuchen **konnte**, stehen auf stderr mit `NICHT GEPRÜFT` und
setzen den Rückgabewert auf `3`. Das deckt längst nicht alles ab — ein Muster,
das schlicht nicht passt, merkt niemand, und ein Rasterbild zählt bewusst nicht
dazu —, aber es macht einen Teil der stillen Fälle laut.
