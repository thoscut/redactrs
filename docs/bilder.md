<a id="bilder"></a>

# Bilder werden wirklich geschwärzt

Liegt eine Schwärzung über einem Rasterbild, wird nicht nur ein Rechteck
darüber gezeichnet — die **Pixel im Bild-XObject selbst werden überschrieben**.
Damit lässt sich ein gescanntes Dokument sicher schwärzen, ganz ohne OCR: man
zieht die Rechtecke von Hand (GUI oder `--manual-regions`), und die Bildpunkte
darunter sind hinterher weg.

Nachgemessen an einem 200×100-Bild (`/DeviceRGB`, `/FlateDecode`), das eine
Seite als Scan trägt, mit einer manuellen Region darüber:

```console
$ redact-rs scan.pdf -o scan_geschwaerzt.pdf --manual-regions bildregion.json --no-patterns
Seiten:             1
Schwärzungen:       1
  davon wirksam:      0 (Zeichen entfernt)
  davon ohne Textfund: 1 (Deck-Rechteck gezeichnet, kein Zeichen entfernt — richtig
                          über Grafik, falsch bei danebenliegenden Koordinaten)
Entfernte Zeichen:  0
Deck-Rechtecke:     1
Überschriebene Bilder: 1 (neu kodiert: außerhalb der Schwärzung verlustfrei,
                          Datei meist größer)
```

Die Zeile „ohne Textfund“ ist hier der Normalfall und kein Mangel: in einem
Rasterbild steht für die Analyse kein Text, zu entfernen gibt es also nichts —
überschrieben werden die Bildpunkte. Der Befund heißt im Audit-Log `covered`,
siehe [`effect` je Region](review-und-audit.md#befund).

Das Bild aus der Ausgabedatei erneut dekodiert und Pixel für Pixel mit dem
Original verglichen (Region 80×40 pt, mit `--padding 1.0` also 82×42 pt):
**3696 von 20 000 Pixeln geändert, alle 3696 auf Schwarz; kein einziges Pixel
außerhalb des Rechtecks verändert** — der veränderte Bereich ist genau
`x 48…131`, `y 28…71`, also **84×44 Bildpunkte**.

Dass es 84×44 sind und nicht 82×42, ist kein Rundungsfehler, sondern die
sichere Richtung: ein Rechteck von 82×42 pt liegt nicht auf dem Pixelraster,
und ein nur teilweise getroffener Randpixel wird **mit** überschrieben statt
stehen gelassen. Nach innen zu runden hieße, einen Streifen Originalbild am
Rand der Schwärzung zu behalten.

Neu kodiert wird dabei immer **verlustfrei** mit `/FlateDecode`
(`crates/redact-pdf/src/image.rs`), auch dann, wenn das Original ein JPEG war.
Nicht bitgleich ist danach die *Datei*, nicht das *Bild*. Dass die
Zusammenfassung das auch so sagt und nicht „verlustbehaftet“ schreibt, hält
`overwritten_images_are_reported_and_logged` in
`crates/redact-pipeline/src/audit.rs` fest — der Test prüft ausdrücklich auf
die **Abwesenheit** des Wortes.

## Der Preis: die Datei ist nicht mehr bitgleich

Ein überschriebenes Bild wird **verlustfrei neu kodiert — immer als
`/FlateDecode`**. Ein `/DCTDecode`-Bild (JPEG) verliert dabei seinen Filter:

Nachgemessen an demselben 200×100-Bild wie oben, einmal als JPEG (Qualität 85)
statt als Flate-Bild, mit derselben Region darüber:

```console
$ # vorher                        nachher
$ #   /Filter /DCTDecode            /Filter /FlateDecode
$ #    6 939 Stream-Bytes          48 782 Stream-Bytes
$ #   Datei 7 766 Byte             Datei 49 591 Byte
```

Das ist Absicht. JPEG neu zu kodieren wäre verlustbehaftet, und die DCT-Blöcke
am Rand der Schwärzung könnten Reste der ursprünglichen Pixel zurücktragen.
Die Datei wird dafür deutlich größer — **beim JPEG.** Bei einem Flate-Bild kann
sie auch schrumpfen: dieselbe Region über dem Flate-Bild oben ergab 57 516 →
49 031 Byte, weil eine große schwarze Fläche sich besser packen lässt als das,
was vorher dort stand. Genau deshalb sagt die Zusammenfassung „Datei **meist**
größer“ und nicht „dadurch größer“ — die längere Warnung daneben sagt es
ebenso, und eine Zusammenfassung, die mehr behauptet als die Warnung, wäre die
falsche von beiden.

Ehrlich dazugesagt: **verlustfrei heißt pixelgleich, nicht bytegleich** — und
„pixelgleich“ heißt bei einem JPEG etwas Schwächeres, als es zunächst klingt:

* **Flate-Bild → Flate-Bild: exakt.** Nachgemessen am Beispiel oben: von
  20 000 Bildpunkten sind genau die 3696 innerhalb des Rechtecks geändert und
  **null** außerhalb.
* **JPEG → Flate-Bild: pixelgleich nur zum eigenen Dekodat.** redact-rs packt
  das JPEG mit seinem Dekoder aus, überschreibt die Bildpunkte und schreibt das
  Ergebnis verlustfrei weg. Wer die Ausgabe gegen das Original hält und dabei
  einen *anderen* JPEG-Dekoder benutzt, sieht deshalb auch außerhalb der
  Schwärzung Unterschiede — JPEG-Dekodierung ist zwischen Implementierungen
  nicht bitgenau. Nachgemessen gegen Pillow: 8231 der 16 304 Bildpunkte
  außerhalb des Rechtecks weichen ab, davon 8167 um **höchstens 2** je Kanal
  (größte Abweichung 16). Innerhalb des Rechtecks sind alle 3696 schwarz. Das
  ist Dekoder-Rauschen, kein zurückgetragener Inhalt — aber wer „unverändert“
  wörtlich nachprüfen will, muss denselben Dekoder benutzen.

Das Bild-*Objekt* in der Datei ist in beiden Fällen ein anderes als im Original.
Wer Bitgleichheit gegenüber dem Original braucht, darf keine Schwärzung über ein
Bild legen.

## Nicht dekodierbare Bilder brechen den Lauf ab

JPEG-2000 (`/JPXDecode`) und Fax-Kodierung (`/CCITTFaxDecode`) kann redact-rs
nicht öffnen. Eine Schwärzung darauf ließe sich nur *überdecken* — die Pixel
blieben in der Datei. Deshalb bricht der Lauf in diesem Fall mit einem Fehler
ab, statt eine Datei zu erzeugen, deren Schwärzung nur obenauf liegt:

```console
$ redact-rs scan_jpx.pdf -o out.pdf --manual-regions regionen.json --no-patterns
Fehler: PDF-Fehler: Bild /Im0 auf Seite 1 lässt sich nicht dekodieren
(JPXDecode (JPEG 2000) wird nicht dekodiert). Die Schwärzung läge nur darüber;
die Pixel blieben in der Datei.
$ echo $?
1
```

`--allow-undecodable-images` hebt das auf. Dann entsteht eine Ausgabe, in der
das Bild **ungeschwärzt** ist und die Schwärzung nur darüberliegt; es bleibt bei
einer Meldung in der Zusammenfassung und im Audit-Log:

```console
$ redact-rs scan_jpx.pdf -o out.pdf --manual-regions regionen.json --no-patterns \
      --allow-undecodable-images
…
NICHT GEPRÜFT: Bild /Im0 auf Seite 1 lässt sich nicht dekodieren (JPXDecode
(JPEG 2000) wird nicht dekodiert). Die Schwärzung läge nur darüber; die Pixel
blieben in der Datei.

1 Stelle(n) in diesem Dokument wurden nicht durchsucht. …
$ echo $?
3
```

Der Rückgabewert ist also **`3`, nicht `0`**: es ist eine Ausgabedatei
entstanden, aber an dieser Stelle wurde nichts geprüft und nichts entfernt.

Der Schalter ist für Fälle gedacht, in denen man das bewusst in Kauf nimmt —
er macht die Ausgabe unsicher.

Liegt ein nicht dekodierbares Bild **außerhalb** jeder Schwärzung, ist es kein
Problem und der Lauf geht ohne Schalter durch.
