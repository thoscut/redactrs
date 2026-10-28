<a id="grenzen"></a>

# Was dieses Werkzeug nicht leistet

Die Liste ist bewusst ausführlich und unfreundlich. Jeder Punkt ist am Code oder
an einem Lauf belegt; wo **[Testfall]** steht, hält ein Test im Repository genau
dieses Verhalten fest. Vollständigkeit lässt sich nicht zusichern — was hier
nicht steht, ist deshalb nicht automatisch abgedeckt.

## Erkennung

* **Keine Namen, keine Adressen, keine Freitexte.** Es gibt kein eingebautes
  Muster dafür und keine Named-Entity-Erkennung. Kontoinhaber, Empfänger,
  Arbeitgeber, Verwendungszwecke bleiben stehen, solange sie nicht über die
  Buchungsliste, eine manuelle Region oder die GUI erfasst werden.
* **Kein Treffer über einen Zeilenumbruch hinweg.** Weder Muster noch
  Buchungsliste sehen mehr als eine extrahierte Textzeile auf einmal.
  Nachgemessen an einer Seite, auf der `IBAN: DE89 3704` und
  `0044 0532 0130 00` in zwei Zeilen stehen:

  ```console
  $ redact-rs wrap.pdf -o wrap_out.pdf --no-patterns --booking-list wrap.csv
  Textzeilen:         2
  Treffer gesamt:     0        # der Eintrag enthält die vollständige IBAN
  Automatische Erkennung: abgeschaltet (--no-patterns). …
  $ redact-rs wrap.pdf -o wrap_out2.pdf --patterns iban_de
  Textzeilen:         2
  Treffer gesamt:     0
  ```

  Gegenprobe, damit die Null etwas bedeutet: steht dieselbe IBAN **in einer**
  Zeile, findet `iban_de` sie (`Textzeilen: 1, Treffer gesamt: 1,
  Schwärzungen: 1`) — auch ungruppiert geschrieben.

* **Muster sind Heuristiken.** `konto_nr` und `blz` entscheiden über den
  Kontext: ohne Schlüsselwort daneben liegen sie mit 0.30 bzw. 0.25 unter der
  Schwelle von 0.50 und finden nichts — eine nackte Ziffernfolge wird also
  *nicht* geschwärzt, solange man nicht `--min-confidence` senkt. `iban_intl`,
  `amount_eur` und `date_de` sind standardmäßig **aus** und werden ohne
  `--patterns` gar nicht angewandt.
* **Fonts ohne `/ToUnicode`.** Steht der Text in einer Schrift, deren Kodierung
  sich nicht auflösen lässt, ist er für die Analyse unsichtbar — und wird
  deshalb nicht geschwärzt. Immerhin **sagt der Lauf es inzwischen**: die
  Warnungen des Interpreters erreichen die Zusammenfassung. Nachgemessen an
  einem PDF mit einem Form-XObject ohne `/Subtype`:

  ```console
  $ redact-rs nosubtype.pdf -o out.pdf --patterns iban_de
  Schwärzungen:       0
  …
  NICHT GEPRÜFT: XObject „Fx“ hat kein bekanntes /Subtype (weder /Form noch
  /Image); sein Inhalt wurde nicht durchsucht. Steht dort Text, blieb er
  ungeschwärzt.

  1 Stelle(n) in diesem Dokument wurden nicht durchsucht. …
  $ echo $?
  3
  ```

  Eine solche Meldung ist der Hinweis, dass „0 Schwärzungen“ nichts bedeutet —
  und sie ist an `NICHT GEPRÜFT` und am Rückgabewert `3` auch maschinell zu
  erkennen. Sie ersetzt die Sichtprüfung nicht.

## Gescannte Dokumente

* **Keine OCR.** Steht die sensible Information als Pixel in einem Rasterbild,
  **findet** die Analyse sie nicht. Sie muss von Hand gezogen werden — in der
  GUI mit der Maus oder über `--manual-regions`. Was gezogen wurde, wird dann
  aber auch wirklich entfernt (siehe [Bilder](bilder.md#bilder)); das ist der
  Unterschied zwischen „nicht gefunden“ und „nicht geschwärzt“.
* **Der `/Alt` eines Bildes gehört zu demselben blinden Fleck.** Ein
  getaggtes PDF darf ein Bild beschreiben (`/Figure <</Alt (…)>> BDC /Im0 Do
  EMC`); steht in der Beschreibung, was auf dem Bild zu lesen ist, überlebt sie
  die Pixel-Schwärzung. Die Analyse liest den `/Alt` eines Bildes so wenig
  wie dessen Pixel. `--check-leaks` sieht ihn im Rohstrom — der Text steht
  als Klartext im Seiteninhalt.
* **Ein reiner Scan meldet 0 Schwärzungen.** Ohne extrahierbaren Text gibt es
  keine Treffer. Die Warnung dazu erscheint aber inzwischen **auch dann**, wenn
  gar nichts geschwärzt wurde — nachgemessen:

  ```console
  $ redact-rs scan.pdf -o scan_geschwaerzt.pdf
  Eingabe:            scan.pdf
  Seiten:             1
  Schwärzungen:       0
  Entfernte Zeichen:  0
  Deck-Rechtecke:     0
  Metadaten:          nichts zu entfernen
  Ausgabe:            scan_geschwaerzt.pdf
  Warnung: 1 von 1 Seite(n) enthalten Rasterbilder. Geschwärzte Bereiche
  werden im Bild selbst überschrieben; gelesen wird der Bildinhalt aber
  nicht — Text *in* einem Bild (Scan, Foto) findet die Analyse ohne OCR nicht.
  $ echo $?
  0
  ```

  Festgehalten von `a_pure_scan_is_reported_even_without_any_redaction`
  (`crates/redact-pdf/tests/images.rs`). Die Ausgabedatei enthält trotzdem
  alles: wer ein gescanntes Dokument bearbeitet, muss die Seiten selbst
  ansehen und die Bereiche von Hand ziehen.

  Der Rückgabewert bleibt hier `0`: ein Rasterbild ist kein Loch in der
  Prüfung, sondern eine bekannte Grenze des Verfahrens, und ein Dokument mit
  Bildern ist der Normalfall. `3` ist den Stellen vorbehalten, an denen der
  Interpreter etwas **nicht durchsuchen konnte** — die beiden nächsten
  Beispiele.
* **JPEG-2000 und CCITT-Fax lassen sich nicht öffnen.** Eine Schwärzung darauf
  bricht den Lauf ab, statt nur zu überdecken — es sei denn, man erlaubt es mit
  `--allow-undecodable-images`, und dann ist die Ausgabe unsicher
  ([Details](bilder.md#bilder)).

## Bekannte Lecks in der Schwärzung

Die vier Lecks, die hier früher standen, sind **alle behoben**; zuletzt
dieses:

* **Erscheinungsströme außerhalb des Schwärzungsbereichs.** Annotationen, die
  in einen Schwärzungsbereich ragen, werden wie bisher entfernt. Ragt eine
  Annotation *nicht* hinein, bleibt sie stehen, aber ihr Erscheinungsstrom
  (`/AP`) wird nach dem Text jeder Schwärzung durchsucht — auf allen Seiten,
  Leerraum zählt dabei nicht mit. Die Zeichen, die ihn wiederholen, werden
  entfernt; der übrige Text der Annotation bleibt lesbar. Grenze: eine
  manuelle Fläche ohne Text sagt nicht, *was* geschwärzt wurde, und lässt sich
  deshalb nicht wiederfinden; Formulare *innerhalb* eines Erscheinungsstroms
  werden nicht durchsucht. (Aufgabe #26)

Dazu diese Grenzen, für die es keinen Testfall gibt:

* **Vektorgrafiken** werden nicht durchsucht — Text, der als Pfad gezeichnet
  ist, ist für die Analyse unsichtbar.
* **Eingebettete Dateien** werden nicht durchsucht. Sie werden allerdings
  **entfernt** (`/Names /EmbeddedFiles` und `/FileAttachment`-Annotationen),
  also nicht auf Inhalte geprüft, sondern samt Inhalt gelöscht.
* **Type3-Fonts** liefern kein Fontprogramm und werden nur genähert behandelt.

Die Testfälle stehen in `crates/redact-pdf/tests/known_leaks.rs`. Solange ein
Leck offen ist, trägt sein Test ein `#[ignore]` mit Aufgabennummer, damit die
Suite grün bleibt und der Defekt trotzdem dokumentiert ist; ein
**Kanarienvogel** ohne `#[ignore]` wird rot, sobald das Leck verschwindet.
Zurzeit ist keiner davon nötig. Nachstellen (im Quellbaum — im Release-Archiv
liegt kein Quelltext):

```bash
cargo test -p redact-pdf --no-default-features --test known_leaks
# 13 passed; 0 failed; 0 ignored
```

**Behoben** (die Tests laufen jetzt ohne `#[ignore]` und sind grün — insgesamt
13 bestandene Tests in `known_leaks.rs`):

| früheres Leck | Test |
|---|---|
| Inline-Bilder zerreißen den Seiteninhalt | `text_after_an_inline_image_is_still_found_by_the_extractor`, `unrelated_text_after_an_inline_image_survives_the_rewrite`, `redacting_a_page_with_an_inline_image_removes_the_secret` |
| Formularfeld-Werte (`/V`) bleiben stehen | `form_field_value_is_redacted_too` |
| verwaiste Objekte werden mitgeschrieben | `objects_unpacked_from_an_object_stream_are_not_carried_over` |
| inkrementelle Vorversionen bleiben erhalten | `incremental_history_is_dropped_when_the_file_is_rewritten` |
| Erscheinungsstrom (`/AP`) einer Annotation außerhalb der Schwärzung trägt den Text weiter | `appearance_stream_outside_the_redaction_is_also_cleaned`, `annotation_outside_the_redaction_keeps_its_other_text` |
| `/ActualText` eines Struktur-Elements spiegelt den geschwärzten Text | `struct_elem_actual_text_does_not_mirror_the_redacted_text` |
| **Textspiegel im Seiteninhalt** (`/ActualText`, `/Alt`, `/E` an einem Marked-Content-Abschnitt) überleben die Schwärzung | 9 Fälle in `crates/redact-pdf/tests/marked_content.rs` — siehe unten |

### Der Textspiegel im Seiteninhalt

Ein getaggtes PDF darf den Glyphen eines Abschnitts einen Ersatztext
beistellen: `/Span <</ActualText (DE89 …)>> BDC … EMC`. Der steht als Klartext
**im Content-Stream**, nicht in einem Objekt daneben. Die Glyphen wurden
korrekt entfernt, das Deck-Rechteck saß richtig — und `pdftotext` gab in der
Voreinstellung trotzdem die vollständige IBAN aus, weil es den Spiegel
bevorzugt.

Die drei Schlüssel `/ActualText`, `/Alt` und `/E`
(`MIRROR_KEYS` in `crates/redact-pdf/src/content.rs`) werden jetzt **geleert**,
sobald von den Glyphen darunter etwas entfernt wurde — nicht nur gefunden.

Drei Schlüssel, zwei Rollen: nach PDF 32000-1 ist `/ActualText` (14.9.4) der
*Ersatz* der Glyphen und muss ihnen gleichen; `/Alt` (14.9.3) *beschreibt*,
`/E` (14.9.5) *schreibt aus* — beide dürfen abweichen. Gelesen und geleert
werden alle drei; die Warnung „Spiegel widerspricht den Glyphen“ gibt es nur
für `/ActualText`. Ein `/Figure <</Alt …>> BDC /Im0 Do EMC` ohne Glyphen
darunter ist die Standardform der Barrierefreiheit und kein Befund — der
`/Alt` eines Bildes gehört zu demselben blinden Fleck wie dessen Pixel, siehe
[Gescannte Dokumente](#gescannte-dokumente); `--check-leaks` sieht ihn im
Rohstrom.
Gedeckt sind neun Wege, auf denen so ein Spiegel in einer Datei stehen kann:

| Fall | Test |
|---|---|
| `/Span <</ActualText (…)>> BDC` im Seitenstrom | `span_with_actual_text_in_the_page_stream` |
| `/Figure <</Alt (…)>> BDC` | `figure_with_alt_text_in_the_page_stream` |
| als Punkt-Operator `DP` statt `BDC` | `marked_content_point_with_actual_text` |
| als Hex-String `<44453839…>` | `actual_text_written_as_a_hex_string` |
| innerhalb eines Form-XObjects | `actual_text_inside_a_form_xobject` |
| über `/Resources /Properties` der Seite | `actual_text_reached_through_resources_properties` |
| dito im Form-XObject, über dessen eigene Ressourcen | `actual_text_through_properties_inside_a_form_xobject` |
| als **indirekter Verweis** auf ein eigenes Objekt | `actual_text_as_an_indirect_reference` |
| **Gegenprobe**: ein Abschnitt, den keine Schwärzung berührt, behält seinen Spiegel | `an_untouched_section_keeps_its_actual_text` |

Die letzte Zeile ist die wichtigere Hälfte: ein Durchgang, der *jeden*
`/ActualText` löscht, macht getaggte PDFs für Screenreader und PDF/UA
unbrauchbar — und niemand würde es merken, weil das Leck-Orakel dazu schweigt.
Orakel ist in allen Fällen ausschließlich `redact_pdf::leaks`, nie der eigene
Extraktor: der liest den Spiegel gar nicht und sähe deshalb nichts.

```bash
cargo test -p redact-pdf --test marked_content
# test result: ok. 9 passed
```

## Verarbeitung

* **Verschlüsselte PDFs** werden ohne Passwort abgelehnt. Mit `--password` bzw.
  `REDACT_RS_PASSWORD` werden sie entschlüsselt und normal verarbeitet — was
  dabei ungeprüft bleibt, steht in
  [`SECURITY.md`](../SECURITY.md#passwörter-verschlüsselter-pdfs). Nicht jedes
  Verfahren ist lesbar; was `lopdf` nicht beherrscht, endet mit derselben
  Meldung wie ein falsches Passwort.
* **Strukturell defekte PDFs** werden abgelehnt, nicht repariert — eine
  „reparierte“ Datei könnte Inhalte enthalten, die der Analyse entgehen.
* **Eine Seite, die sich nicht in Operationen zerlegen lässt, kostet die ganze
  Datei.** Lässt sich der Content-Stream einer Seite (oder auch nur ein
  Teilstück davon) nicht zerlegen, wird die **Datei abgelehnt** — nicht
  gewarnt. Ihr Text wurde nicht durchsucht und kann deshalb nicht geschwärzt
  worden sein; beim Neuschreiben ginge er zudem ersatzlos verloren. Vorher
  meldete derselbe Lauf „Schwärzungen: 0“ mit Rückgabewert 0, und die
  Kontonummer stand unverändert in der Ausgabe. Eine Warnung auf stderr hätte
  daraus im Stapelbetrieb trotzdem eine „verarbeitete“ Datei gemacht.
* **Kein Plugin-System.** Die Stapelverarbeitung gibt es inzwischen
  ([siehe Kommandozeile](kommandozeile.md#stapel)), sie steigt aber **nicht** in Unterverzeichnisse ab.
* **Keine unbegrenzte Größe.** Sechs Grenzen greifen, jede mit einer eigenen
  Meldung und einem Rückgabewert statt eines Speicherfehlers:

  | Was | Vorgabe | Stellschraube |
  |---|---|---|
  | Größe der Eingabedatei | 512 MB | `--max-input-mb` |
  | entpackte Bytes über alle Streams | 1024 MB | `--max-decompressed-mb` |
  | davon: alles, woraus PDF-Syntax wird — geparste Streams **und** der Rumpf der Datei | 16 MB | `--max-parsed-mb` |
  | gleichzeitig gehaltene dekodierte Bildbytes | 256 MB | `--max-image-mb` |
  | Trefferkandidaten je Datei | 100 000 | `--max-candidates` (Exit 2) |
  | Zeichen, die **eine Seite** setzen darf | 1 000 000 | fest |

  Dazu ein **Aufwandskonto** gegen die Vervielfachung durch Form-XObjects: Eine
  Datei von 2 368 Byte hält jede Byte-Grenze ein und lässt trotzdem acht
  Form-XObjects einander so oft zeichnen, dass über zwei Millionen Durchläufe
  entstehen. Dagegen hilft keine Größengrenze. Das Konto zählt deshalb, was
  wirklich anfällt, und wächst mit dem Inhalt, den die Datei *mitbringt* —
  nicht mit dem, was sie daraus macht. Ist es leer, wird die Datei abgelehnt.
  Das gilt auch über das ganze Dokument: ein Inhaltsstrom oder ein Formular,
  das viele Seiten zeigen, bringt sein Guthaben einmal ein, nicht je Seite.

  Zwei weitere Decken haben **keinen** Schalter, weil es an ihnen nichts
  einzustellen gibt — sie begrenzen, was eine Datei über den Umweg ihrer
  Schriften belegen kann:

  | Was | Grenze | Was geschieht darüber |
  |---|---|---|
  | Eine `/ToUnicode`-Zuordnung (Platzbedarf, `MAX_TO_UNICODE_BYTES`) | 32 MB | Die Zuordnung wird **verworfen**, nicht abgeschnitten: der Font gilt als einer ohne `/ToUnicode`. Bei einem Type0-Font ist das eine Deckungslücke — sie steht auf stderr und im Audit-Log, und der Lauf endet mit Rückgabewert 3. |
  | Zwischenspeicher für Schriftmetriken (Tabelleneinträge, `MAX_CACHED_FONT_ENTRIES`) | 400 000 | Nichts wird verdrängt, es wird nur nichts mehr aufgenommen. Das **Ergebnis bleibt gleich**; die betroffene Schrift wird je Platzierung neu geladen. |

  Nachgemessen am gebauten Binary (0.6.0). Eine 1 064 Byte kleine Datei mit
  einem Type0-Font, dessen `/ToUnicode` drei `bfrange`-Blöcke über je 65 536
  Codes aufspannt (rund 50 MB Platzbedarf), reißt die erste Decke:

  ```console
  $ redact-rs tou_gross.pdf -o g.pdf --no-patterns
  …
  NICHT GEPRÜFT: Font „Test“ hat kein /ToUnicode; sein Text lässt sich nicht
  dekodieren. Muster können darin nicht erkannt werden — diese Seite wurde
  möglicherweise nicht vollständig geschwärzt.
  $ echo $?
  3
  ```

  Dieselbe Datei mit **einem** solchen Block (rund 16,8 MB) läuft mit
  Rückgabewert 0 durch und dekodiert ihren Text. Für die zweite Decke: eine
  5 235 Byte kleine Datei mit acht solchen Schriften auf einer Seite (zusammen
  524 288 Tabelleneinträge, also über der Decke) belegt in der Debug-Fassung
  67 MB Spitzenspeicher, dekodiert alle acht Textzeilen und endet mit 0.

  **Eine unbrauchbare `/MediaBox`** — nicht endlich oder mit einer Kante
  außerhalb des zulässigen Bereichs, etwa `[0 0 0 0]` — führt nicht zur
  Ablehnung der Seite, sondern wird durch **A4 ersetzt** (`sane_box` in
  `crates/redact-pdf/src/document.rs`; der Rasterizer nennt das „Unbrauchbare
  MediaBox (0 x 0), A4 angenommen“). Sonst gäbe es kein Blatt, auf dem sich ein
  Rechteck beschneiden oder zeichnen ließe.

  **Die Oberfläche sagt es beim Laden** — nachgemessen an einem zweiseitigen
  Dokument, dessen zweite Seite `/MediaBox [0 0 0 0]` trägt: „Seite 2 nennt
  eine unbrauchbare Seitengröße; gerechnet und gezeichnet wird mit A4. Prüfen
  Sie dort besonders genau, ob die Rechtecke sitzen.“

  **Die Kommandozeile sagt es auch** — nachgemessen an einer einseitigen Datei
  mit `/MediaBox [0 0 0 0]`: sie findet die IBAN, schwärzt sie (28 Zeichen
  entfernt) und meldet dazu „Seite 1: Unbrauchbare MediaBox (0 x 0), A4
  angenommen“ samt dem Hinweis, dass die Wirkungsprüfung dieser Seite gegen A4
  gemessen ist und nicht gegen die Angabe der Datei. Der Lauf endet mit
  **Rückgabewert 3** — verarbeitet, aber nicht vollständig geprüft. Wer dort
  mit eigenen Koordinaten arbeitet, rechnet gegen A4, und das steht jetzt in
  der Ausgabe statt nur in der Oberfläche.

  **Auch die Hilfsdateien haben eine Grenze**, und die ist *fest* — es gibt
  keinen Schalter dafür:

  | Was | Grenze |
  |---|---|
  | Buchungsliste (`--booking-list`) | 16 MB |
  | Review-Datei (`--apply-review`) | 16 MB |
  | Regionsliste (`--manual-regions`) | 16 MB |
  | Musterkonfiguration (`--patterns-config`) | 1 MB |
  | Einstellungsdatei | 1 MB |

  Sie sind nicht gegen Angreifer gerichtet — diese Dateien bringt der Bedienende
  selbst mit —, sondern gegen den vertippten Pfad: `--manual-regions` auf einen
  700-MB-Scan statt auf die JSON-Datei legte vorher einen Puffer in Dateigröße
  an, bevor überhaupt feststand, dass es kein JSON ist (nachgemessen an einer
  dünn belegten 6-GB-Datei: **6 150 MB Spitzenspeicher nach 24 s**; heute Exit 1
  nach 0,00 s bei 6,3 MB). Eine benannte Pipe oder ein Gerät wird abgelehnt,
  bevor die Größe überhaupt zur Sprache kommt.

  Details, Messwerte und die Begründung jeder Zahl in
  [`SECURITY.md`](../SECURITY.md).

## Was der Nutzer selbst tun muss

1. **Das Ergebnis ansehen.** Jede Seite, mit den Augen. Die Zahl der
   Schwärzungen ist keine Aussage über Vollständigkeit.
2. **Gegen die eigenen Werte prüfen** — siehe
   [Prüfen, ob die Schwärzung gewirkt hat](pruefen.md#pruefen).
3. **`review.json` und `audit.json` aufräumen.** Beide liegen standardmäßig
   neben dem Original und tragen Klartext (Details unter
   [Review-Workflow und Audit-Log](review-und-audit.md)).
4. **Bei gescannten Seiten die Rechtecke selbst ziehen.** Ohne OCR *findet* das
   Werkzeug dort nichts; gezogene Bereiche werden aber wirklich aus den Pixeln
   entfernt ([Details](bilder.md#bilder)). Die Verantwortung dafür, dass jede
   schützenswerte Stelle ein Rechteck bekommen hat, liegt beim Auge des
   Nutzers.
5. **Das Original getrennt aufbewahren.** Die Ausgabe entsteht neben der
   Eingabe; eine Verwechslung beim Verschicken ist der wahrscheinlichste Fehler
   überhaupt.
