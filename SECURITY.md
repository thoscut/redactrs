# Sicherheit

redact-rs bekommt genau die Dateien zu sehen, die jemand nicht veröffentlicht
haben will — und es bekommt sie oft von genau der Seite, die ein Interesse
daran haben kann, dass das Werkzeug abstürzt, die Maschine lahmlegt oder an
eine falsche Stelle schreibt. Dieses Dokument sagt, wogegen das Werkzeug
gehärtet ist, wogegen nicht, und mit welchen Messungen das belegt ist.

Leitsatz: **die Software darf für das System, auf dem sie läuft, keine
Schwächung bedeuten.** Ein Werkzeug, das beim Öffnen einer präparierten Datei
den Rechner belegt oder eine Systemdatei überschreibt, ist keine Hilfe, sondern
ein zusätzliches Risiko.

---

## Bedrohungsmodell

**Nicht vertrauenswürdig — das Eingabe-PDF.**
Es stammt von außen. Es darf beliebig kaputt, absichtlich präpariert oder auf
einen Absturz des Parsers hin gebaut sein. Jede Grenze in diesem Dokument
existiert wegen dieser Annahme.

**Vertrauenswürdig — was der Nutzer selbst mitbringt.**
Musterkonfiguration (`--patterns-config`), Buchungsliste (`--booking-list`),
manuelle Regionen (`--manual-regions`), Review-Dateien (`--apply-review`) und
die Einstellungsdatei (`~/.config/redact-rs/settings.yaml`) kommen vom
Bedienenden. Sie werden auf Plausibilität geprüft, aber nicht als
Angriffsfläche behandelt. Eine fehlerhafte Einstellungsdatei beendet den Lauf
mit einer Meldung, statt stillschweigend auf die Vorgaben zurückzufallen —
sonst arbeitete das Werkzeug mit anderen Werten als der Nutzer meint,
eingestellt zu haben. Wer eine fremde Musterdatei einspielt, spielt eine
fremde Konfiguration ein — dieselbe Vertrauensstufe wie ein Kommandozeilen-
Argument.

**Eine Einschränkung dazu, und sie ist keine Vertrauensfrage.** Ihr *Inhalt*
wird nicht als feindselig behandelt, ihre *Größe* schon: alle vier gehen durch
einen begrenzten Leser (16 MB, Musterkonfiguration 1 MB — siehe „Grenzen für
Eingabedateien“). Ohne das bestimmte die Datei, wie viel Arbeitsspeicher der
Lauf belegt, und der wahrscheinlichste Weg dorthin ist kein Angriff, sondern
ein vertippter Pfad: `--manual-regions` auf den 700-MB-Scan statt auf die
JSON-Datei. Vertrauen in die Absicht ist kein Grund, den eigenen Prozess einer
Zahl aus einem Verzeichniseintrag auszuliefern.

**Kein Bestandteil des Modells.**
Ein Angreifer mit Schreibrechten im Arbeitsverzeichnis oder mit Zugriff auf den
laufenden Prozess. Gegen den hilft kein Anwendungsprogramm.

---

## Was zugesichert wird

* **Kein Netzwerk.** Es gibt keine Netz-Abhängigkeit im Programmpfad; es wird
  weder etwas nachgeladen noch gemeldet.
* **Keine temporären Dateien außerhalb des Zielverzeichnisses.** Das PDF wird
  vollständig im Speicher verarbeitet. Beim Schreiben entsteht genau eine
  temporäre Datei, und zwar **im Zielverzeichnis**; sie wird per `rename` zur
  Ausgabedatei. Das ist der Preis dafür, dass niemals eine halb geschriebene
  Datei sichtbar wird. In `/tmp` landet nichts.
* **Kein PDF-JavaScript.** `/JavaScript`, `/AA`, `/OpenAction` und Ähnliches
  werden nicht ausgeführt — es gibt keinen Interpreter dafür. Gelesen wird nur
  Struktur, Text und Grafikzustand.
* **`#![forbid(unsafe_code)]` in allen acht eigenen Crates** (`redact-core`,
  `redact-pdf`, `redact-patterns`, `redact-booking`, `redact-pipeline`,
  `redact-render`, `redact-gui`, `redact-cli`) — und in jedem der drei
  Binärziele. Der Compiler setzt das durch; es ist keine Absichtserklärung.

  ```bash
  $ grep -rl 'forbid(unsafe_code)' crates/*/src/lib.rs crates/*/src/main.rs \
        crates/*/src/bin/*.rs
  crates/redact-booking/src/lib.rs
  crates/redact-core/src/lib.rs
  crates/redact-gui/src/lib.rs
  crates/redact-patterns/src/lib.rs
  crates/redact-pdf/src/lib.rs
  crates/redact-pipeline/src/lib.rs
  crates/redact-render/src/lib.rs
  crates/redact-cli/src/main.rs          # redact-cli hat keine lib.rs
  crates/redact-gui/src/main.rs
  crates/redact-cli/src/bin/redact-rs-gui.rs
  ```
* **Kontrollierter Abbruch statt Speicherfehler.** Übersteigt eine Eingabe die
  unten genannten Grenzen, endet der Lauf mit einer Meldung und einem
  Rückgabewert — nicht mit SIGABRT und nicht mit einer gescheiterten
  Speicheranforderung.
* **Ein einziger Schreibpfad.** Alles, was das Werkzeug ablegt — geschwärztes
  PDF, Review-Datei, Audit-Log, Beispieldatei —, geht durch
  `redact_pdf::document::write_file`. Dieser Pfad
  * kanonisiert das Zielverzeichnis (Symlinks und `..` aufgelöst) und behandelt
    den Dateinamen getrennt,
  * lehnt es ab, wenn das Ziel dieselbe Datei ist wie die Eingabe — erkannt
    über Geräte- und Inode-Nummer, also auch bei `./in.pdf`, `dir/../in.pdf`,
    absolutem Pfad, Hardlink, Symlink und auf Dateisystemen ohne
    Groß-/Kleinschreibung; **auch mit `--force`**,
  * lehnt es ab, wenn das Ziel ein symbolischer Link ist,
  * überschreibt eine vorhandene Datei nur mit `--force` — das gilt jetzt auch
    für `--write-demo`, `--review-out` und `--audit-log`, die die Prüfung
    vorher schlicht übersprungen haben,
  * legt die Datei über `create_new(true)` an (`O_CREAT | O_EXCL`, folgt keinem
    Symlink) und macht sie erst durch `rename` sichtbar,
  * legt Review-Datei und Audit-Log unter Unix mit Modus `0600` an — in beiden
    stehen die *gefundenen* Geheimnisse im Klartext.
* **Die Schreibziele werden geprüft, bevor gerechnet wird.** Ein Lauf, dessen
  Audit-Log-Ziel nicht taugt, schreibt auch kein PDF.
* **Der Namenszusatz ist ein Name.** `--output-suffix` und `output_suffix` aus
  der Einstellungsdatei dürfen keinen Pfadtrenner, kein `..` und kein
  Steuerzeichen enthalten; sonst bricht der Lauf ab, bevor etwas entsteht.
  Siehe „Ein Namenszusatz ist kein Wegweiser“.
* **Nichts aus einer fremden Datei steuert die Anzeige.** Dateinamen und
  Meldungen gehen auf dem Weg nach stdout/stderr durch
  `redact_core::safe_text`. Siehe „Fremde Zeichen auf dem Terminal“.
* **Die Fehlermeldung der Einstellungsdatei gibt deren Inhalt nicht wieder.**
  `REDACT_RS_CONFIG` zeigt auf einen beliebigen Pfad. Siehe „Die
  Einstellungsdatei als Vorleser“.
* **Das Passwort eines verschlüsselten PDFs steht in keiner erzeugten Datei.**
  Weder in der Ausgabe-PDF noch im Audit-Log noch in der Review-Datei, und in
  keiner Fehlermeldung. Siehe den eigenen Abschnitt unten.

---

## Passwörter verschlüsselter PDFs

Ohne Passwort lehnt redact-rs ein verschlüsseltes PDF ab — daran hat sich
nichts geändert, und das ist die richtige Vorgabe. Mit `--password`,
`REDACT_RS_PASSWORD` oder der Abfrage in der Oberfläche wird die Datei
entschlüsselt und wie jede andere verarbeitet.

### Ein Passwort auf der Kommandozeile ist lesbar

`redact-rs auszug.pdf --password geheim` schreibt das Passwort

* in die **Prozessliste** — auf den meisten Systemen kann jedes Konto
  `ps aux` lesen, und `/proc/<pid>/cmdline` ist unter Linux für alle lesbar,
* in die **Shell-Historie** (`~/.bash_history`, `~/.zsh_history`), wo es dauerhaft
  bleibt.

Deshalb gibt es zwei Wege, die das vermeiden, und der Hilfetext nennt sie:

```sh
# Umgebungsvariable — nicht in der Prozessliste, nicht in der Historie
# (das führende Leerzeichen hält die Zeile aus der Historie, wenn
#  HISTCONTROL=ignorespace gesetzt ist)
 REDACT_RS_PASSWORD=geheim redact-rs auszug.pdf

# Oder ganz ohne Tippen: die Oberfläche fragt in einem Fenster mit
# verdeckter Eingabe
redact-rs --gui auszug.pdf
```

Rangfolge: `--password` schlägt `REDACT_RS_PASSWORD`. Ohne beides bleibt es bei
der Ablehnung.

### Wohin das Passwort **nicht** gelangt

Das Passwort liegt in `redact_pipeline::Secret`. Der Typ gibt den Klartext nur
über `reveal()` heraus — und das ruft genau eine Stelle auf, die
Entschlüsselung selbst. `Debug` und `Display` zeigen Sterne, und `Serialize`
ist absichtlich **nicht** abgeleitet: es kann also weder über ein `{:?}`
irgendwo im Programm noch über einen JSON-Export hinausrutschen. Die
Fehlermeldung bei falschem Passwort ist ein fester Satz ohne den eingegebenen
Wert.

Gemessen wird das, nicht behauptet:
`crates/redact-cli/tests/password.rs::the_password_appears_in_no_file_the_run_produces`
lässt einen vollständigen Lauf über ein wirklich verschlüsseltes PDF laufen und
durchsucht Ausgabe-PDF, Audit-Log und Review-Datei — die PDF mit
`redact_pdf::leaks`, also auf allen Ebenen inklusive entpackter Streams — sowie
stdout und stderr.

### Die Grenzen gelten auch hinter der Entschlüsselung

Sie greifen nur **zweistufig**, weil die Vorprüfung `prescan` über Rohbytes
läuft und an einer verschlüsselten Datei nur die Hälfte sehen kann:

| | verschlüsselt? | auf den Rohbytes messbar |
|---|---|---|
| Objektstruktur (`<<`, `[`, Namen, Zahlen) | nein | ja |
| Zeichenketten und **Streams** | ja | nein — es ist Rauschen |

* **Vor** der Entschlüsselung läuft `prescan` wie immer. Was dort zu sehen ist,
  ist geprüft: eine Datei mit 200 000 offenen `[` in einem gewöhnlichen Objekt
  fällt schon hier durch, verschlüsselt oder nicht.
* **Nach** der Entschlüsselung läuft dieselbe Prüfung ein zweites Mal, jetzt auf
  dem entschlüsselten Dokument (`redact_pipeline::check_limits_after_decryption`).
  Dazu wird das Dokument serialisiert — die Streams stehen darin als das, was
  sie sind: komprimiert, aber nicht mehr verschlüsselt. Es misst also
  derselbe Code mit denselben Grenzen und denselben Meldungen wie bei einer
  unverschlüsselten Datei.

Erst *nach* dem Laden zu prüfen ist nur deshalb vertretbar, weil das Laden
selbst billig ist: `lopdf` legt Streams als Rohbytes ab und packt sie nicht aus.
Der teure Teil — die Zerlegung des Seiteninhalts in Operationen, je nach Form
des Stroms 62 bis rund 100 Byte Arbeitsspeicher je Byte Stream (siehe
„Dekompressionsbomben“) — kommt erst danach.

**Richtigstellung.** Bis einschließlich dieser Fassung stand hier: *„Wer ein
verschlüsseltes PDF öffnet, gibt ihm ausdrücklich mehr Vertrauen als einem
unverschlüsselten — was insofern zusammenpasst, als man sein Passwort kennt."*
Der Satz war falsch, und er hat eine Lücke gedeckt.

Falsch ist er, weil er das Vertrauen an die falsche Stelle hängt.
Passwortgeschützte Kontoauszüge werden **samt Passwort** verschickt; das ist
der Normalfall, für den `--password` überhaupt existiert. Das Vertrauen gilt
dann dem Absender, nicht der Byte-Struktur der Datei — und der Absender ist im
Bedrohungsmodell dieses Werkzeugs ausdrücklich *nicht* vertrauenswürdig.
Gedeckt hat er, dass ein Passwort nicht nur die Entschlüsselung freischaltete,
sondern sämtliche Stream-Budgets **abschaltete**: nach
`Document::load_mem_with_options` lief nur noch `validate`. Zwei Messungen dazu
stehen unter „Verschlüsselte Bomben".

**Das Passwort im Arbeitsspeicher wird nicht überschrieben.** Es steht als
gewöhnlicher `String` im Prozess und wird beim Freigeben nicht genullt; ein
Kernabbild oder eine Auslagerungsdatei kann es enthalten. Dagegen hülfe nur
gesperrter Speicher, und das ist eine Abhängigkeit und eine Zusicherung, die
dieses Werkzeug nicht gibt.

**Nicht jede Verschlüsselung ist lesbar.** Was `lopdf` beherrscht, wird
geöffnet; alles andere (etwa zertifikatsbasierte Sicherheitshandler) endet mit
derselben Meldung wie ein falsches Passwort.

**Berechtigungen werden nicht durchgesetzt.** Ein PDF kann „Kopieren verboten“
oder „Drucken verboten“ signalisieren. redact-rs wertet diese Angaben nicht
aus — sie sind ohnehin keine Zugriffskontrolle, sondern eine Bitte an die
anzeigende Software. Wer die Datei öffnen darf, kann sie hier schwärzen.

---

## Grenzen für Eingabedateien

| Grenze | Vorgabe | Stellschraube |
|--------|---------|---------------|
| **Größe der Eingabedatei** | **512 MB** | **`--max-input-mb`** |
| Verschachtelungstiefe (`[`, `<<`) | 100 | fest |
| dito, in binär aussehender Nutzlast | 256 | fest |
| entpackte Bytes über **alle** Streams | 1024 MB | `--max-decompressed-mb` |
| davon: Streams, die geparst werden | 16 MB | `--max-parsed-mb` |
| Trefferkandidaten je Datei | 100 000 | `--max-candidates` |
| Rohgröße eines LZW-/ASCII85-Streams | 16 MB | fest |
| Bildpunkte **je Bild** (Dekodieren) | 40 000 000 | fest |
| gleichzeitig gehaltene **dekodierte** Bildbytes | 256 MB | `--max-image-mb` |
| **Zeichen, die eine Seite setzen darf** | **1 000 000** | fest |
| **Zeichenoperationen je Seiten-Scan** (Aufwandskonto) | **1 000 000 + 16× Inhalt** | fest |
| Größe der Einstellungsdatei | 1 MB | fest |
| **Buchungsliste** (`--booking-list`) | **16 MB** | **fest** |
| **Review-Datei** (`--apply-review`) | **16 MB** | **fest** |
| **Regionsliste** (`--manual-regions`) | **16 MB** | **fest** |
| **Musterkonfiguration** (`--patterns-config`) | **1 MB** | **fest** |

Alle Grenzen, die dem **Eingabe-PDF** gelten, gelten für verschlüsselte Dateien
genauso — siehe „Die Grenzen gelten auch hinter der Entschlüsselung“.

Die Zeilen „Zeichen, die eine Seite setzen darf“ und „Zeichenoperationen je
Seiten-Scan“ messen keine Bytes, und das ist ihr Zweck; sie stehen weiter unten
unter „Wenn Bytes die falsche Größe sind“.

### Die Hilfsdateien sind fest begrenzt, und zwar mit Absicht

Die letzten fünf Zeilen — Einstellungsdatei, Buchungsliste, Review-Datei,
Regionsliste, Musterkonfiguration — gelten **nicht** dem Eingabe-PDF, sondern
den Dateien, die der Bedienende selbst mitbringt. Sie stehen hier trotzdem, weil
eine Grenze nicht davon abhängen darf,
ob eine Datei *böse gemeint* ist: `std::fs::read` legt einen Puffer in
**Dateigröße** an, bevor irgendetwas geprüft wurde. Wie viel Arbeitsspeicher ein
Lauf belegt, stand damit in der Datei und nicht in der Konfiguration — und der
häufigste Weg dorthin ist kein Angriff, sondern ein vertippter Pfad:
`--manual-regions` auf einen 700-MB-Scan statt auf die JSON-Datei. Gemessen
steht das unter „Hilfsdateien ohne Grenze“.

**Warum sie fest sind.** Ein Schalter dafür hätte als einzigen Zweck, diesen
Schutz aufzuweichen. Für `--max-input-mb` gibt es einen Grund — ein 700-MB-Scan
ist eine echte Eingabe —, für eine 700-MB-Buchungsliste keinen.

**Warum 16 MB und nicht 512.** Bemessen an dem, was legitim vorkommt:
`examples/booking_list.csv` braucht 134 Byte je Eintrag (941 Byte für sieben
Einträge, nachgezählt), 16 MB fassen also über hunderttausend — mehr, als die
Kette mit `--max-candidates` (100 000) überhaupt weiterreicht. Für die
Review-Datei rechnet der Code mit rund 600 Byte je geprüfter Stelle, also gut
25 000 Stellen; von Hand geprüft werden Dutzende bis Hunderte.

**Warum die Musterkonfiguration enger liegt.** Dort ist nicht die Byte-Zahl die
teure Größe, sondern die Zahl der übersetzten regulären Ausdrücke — rund 12 kB
Arbeitsspeicher je Muster. 1 MB fasst über 26 000 Muster; `examples/patterns.yaml`
wiegt 1 155 Byte (nachgemessen). Dieselbe Grenze gilt für die Einstellungsdatei.
Die Begründungen im Einzelnen stehen bei den Konstanten selbst
(`redact_core::MAX_AUX_FILE_BYTES`, `MAX_CONFIG_BYTES` in
`crates/redact-patterns/src/matcher.rs`); die dort angegebenen
Speicher-Messreihen sind hier **nicht** nachgemessen.

**Geprüft wird in drei Schritten, und der erste ist nicht die Größe:**
es muss eine *gewöhnliche Datei* sein (eine benannte Pipe meldet Länge 0 und
liefert endlos), dann muss die Länge unter der Grenze liegen, und gelesen wird
danach trotzdem über einen begrenzten Leser — zwischen Frage und Antwort kann
eine Datei wachsen.

Die Vorprüfung (`redact_pdf::document::prescan`) läuft über die **Rohbytes**,
bevor `lopdf` die Datei zu sehen bekommt, und schließt die ausgepackten Streams
mit ein. Sie muss davor laufen: der Stapelüberlauf beendet den Prozess, bevor
irgendein Fehlerwert entstehen könnte.

Die erste Zeile steht **vor** allen anderen, und zwar wörtlich: die
Eingabedatei wird in einem Stück gelesen — die Prüfsumme im Audit-Log soll die
der *verarbeiteten* Bytes sein und nicht die einer inzwischen ausgetauschten
Datei —, und ohne diese Grenze stand damit in der Datei, wie viel
Arbeitsspeicher der Lauf belegt. `redact_pipeline::read_input` fragt deshalb
zuerst nach Art und Länge und liest dann über einen begrenzten Leser; siehe
„Unbegrenzte Eingabedatei“.

**Zur Zahl 512 MB.** Es ist eine Grenze gegen das Absurde, nicht gegen das
Große. Die Vorlage ist ein Kontoauszug: ein paar hundert Kilobyte, mit
eingescannten Seiten einige Megabyte; selbst ein Jahrgang farbig gescannter
Auszüge in 600 dpi bleibt weit darunter. 512 MB lassen sich auf jeder Maschine
lesen, auf der die Oberfläche läuft, und sind zwei Zehnerpotenzen von dem
entfernt, was die Maschine umwirft. Wer wirklich mehr braucht, sagt es mit
`--max-input-mb` — das ist dann eine bewusste Entscheidung und keine, die eine
fremde Datei für den Nutzer trifft.

Die beiden Bildzeilen sind eine **eigene** Klasse und stehen bewusst getrennt:
`--max-decompressed-mb` und `--max-parsed-mb` verbuchen die *Rohbytes* eines
Streams. Ein Bild, das geschwärzt wird, muss aber nach RGBA8 ausgepackt werden —
4 Byte je Bildpunkt. Bei einem gewöhnlichen Schwarzweiß-Scan (`/DeviceGray`,
`/BitsPerComponent 1`) liegt zwischen beidem der **Faktor 32**; die
Rohbyte-Grenzen greifen dort also nicht. Siehe „Speicherbedarf der
Bildschwärzung“ unter „Messungen“.

### Wenn Bytes die falsche Größe sind

Jede Grenze oben misst Bytes. Was der Speicher wirklich kostet, sind aber
*Interpretationen* — und die Zahl der Interpretationen hängt nicht an der
Dateigröße. Zwei Wege nutzen genau diese Lücke; beide halten jede Byte-Grenze
dieser Tabelle ein.

**Fächerung durch Form-XObjects.** Ein Form-XObject darf ein anderes zeichnen,
und zwar mehrfach. Sieben Ebenen, in denen jedes Formular dasselbe Unterobjekt
achtmal zeichnet, ergeben 8⁷ ≈ **zwei Millionen Durchläufe** — aus einer Datei
von **2 368 Byte**. Dagegen hilft keine Tiefengrenze (die Verschachtelung ist
mit 8 Ebenen harmlos) und keine Größengrenze (die Datei ist winzig). Es hilft
nur, den *Aufwand* zu zählen.

Das **Aufwandskonto** (`Budget` in `crates/redact-pdf/src/content.rs`) tut das:

* Grundausstattung **1 000 000 Zeichenoperationen** je Seiten-Scan. Reichlich
  bemessen, weil dort echte Gestaltung hineinfällt — ein Tabellenraster, das
  dieselbe Zelle hundertmal zeichnet, ein Formular mit vielen Bausteinen. Die
  dichteste gemessene Seite eines 500-seitigen Kontoauszugs braucht rund 200:
  Faktor 5 000.
* Dazu **16× die Operationen jedes Stroms**, der zum ersten Mal dekodiert wird.
  Das Konto wächst also mit dem Inhalt, den die Datei **mitbringt**, nicht mit
  dem, was sie daraus macht. Ein zweiter Durchlauf durch denselben Strom bringt
  nichts ein — sonst finanzierte die Fächerung sich selbst.

Damit steht diese Grenze nicht quer zu `--max-parsed-mb`: wer das Parse-Budget
anhebt, hebt das Aufwandskonto automatisch mit an, weil mehr Inhalt mehr
Guthaben bedeutet. Eine Fächerung profitiert davon nicht, denn sie bringt ja
gerade keinen zusätzlichen Inhalt mit.

**Glyphen je Seite.** Anders als das Operationskonto eine **feste Decke**, und
zwar mit Absicht: dies ist die eigentliche Speichergröße der Textextraktion.
Jede Glyphe wird als `GlyphItem` gehalten (Originalbytes, Text, Kasten,
Grundlinie) und beim Zusammensetzen der Zeilen noch einmal kopiert. Die Messung
dazu steht bei der Konstanten selbst (`MAX_GLYPHS_PER_SCAN` in
`crates/redact-pdf/src/content.rs`): Release, ein Seiteninhalt knapp unter dem
Parse-Budget, **14,6 Mio. Glyphen auf einer Seite → 9 306 MB**, also rund
**640 Byte je Glyphe**. Ein Budget, das mit der erlaubten Dateigröße mitwüchse,
wüchse hier in den zweistelligen Gigabytebereich.

Eine Million Zeichen auf einer Seite ist keine Seite mehr. Eine dichte
A4-Textseite trägt 3 000–6 000 Zeichen; die dichteste gemessene Seite eines
500-seitigen Kontoauszugs ebenfalls 6 000. Faktor 160 Luft.

```console
$ redact-rs viel_text.pdf -o out.pdf --no-patterns
Fehler: PDF-Fehler: Eine Seite dieses Dokuments setzt mehr als 1000000 Zeichen.
Eine dichte Textseite trägt einige tausend; diese Menge entsteht nur, wenn
derselbe Text vielfach gezeichnet wird. Beim Vermessen der Zeichen würde daraus
ein zweistelliges Gigabyte Arbeitsspeicher. Die Datei wird abgelehnt.
$ echo $?
1
```

Nachgemessen an einer 46-kB-Datei mit 15 MB Seiteninhalt aus wiederholten
`Tj`-Operationen: Exit 1 nach 2,8 s bei **1 003 MB** — die Grenze greift
innerhalb des Parse-Budgets, nicht erst danach.

**Beide brechen ab, sie warnen nicht.** Eine Seite, deren Text nur zum Teil
durchsucht wurde, darf nicht als Erfolg enden: der ungeprüfte Rest ist genau
der, in dem das Geheimnis stehen kann. „0 Schwärzungen, Rückgabewert 0“ liest
sich wie „nichts gefunden, also sauber“.

### Eine Seite, die sich nicht zerlegen lässt, kostet die ganze Datei

Aus demselben Grund wird die Datei abgelehnt, wenn `scan_page` den Seiteninhalt
nicht vollständig in Operationen zerlegen kann — und zwar in zwei Abstufungen:

* **Ein Teilstück** ließ sich nicht zerlegen. Das fiel früher lautlos unter den
  Tisch: nicht der ganze Strom, nur ein Abschnitt daraus, und mit ihm jeder
  Text darin. Beim Neuschreiben der Seite wäre er zusätzlich ersatzlos verloren
  gegangen. Die Meldung nennt heute Byte-Zahl und Anzahl der Teilstücke.
* **Der ganze Strom** ließ sich nicht zerlegen, obwohl er Token enthält. Das
  war früher eine Warnung. Der Unterschied ist der Rückgabewert: eine Warnung
  auf stderr macht aus einem Lauf, der den Text dieser Seite nachweislich nie
  gesehen hat, trotzdem eine Datei, die im Stapelbetrieb als „verarbeitet“
  zählt. Der Befund dazu steht bei der Prüfung selbst (`scan_page` in
  `crates/redact-pdf/src/content.rs`): eine **1 122 Byte** große Datei, Ausgabe
  „Schwärzungen: 0“, Rückgabewert 0 — und die Kontonummer unverändert in der
  Ausgabedatei.

Das ist zugleich die Antwort auf die dritte Zeile unter „Verschlüsselte
Bomben“, den stillen Fall: eine Datei, deren Content-Stream so tief
verschachtelt ist, dass `lopdf` ihn nicht in Operationen zerlegt, endet jetzt
mit einer Ablehnung statt mit einer Ausgabedatei voller ungeschwärzter IBAN.

---

## Wege, die nicht über den Speicher gehen

### Ein Namenszusatz ist kein Wegweiser

Ohne Ausgabedatei entsteht das Ergebnis **neben** der Eingabe, mit einem Zusatz
im Dateinamen. Der Zusatz kam ungeprüft aus `--output-suffix`, aus der
Einstellungsdatei und aus dem Feld in der Seitenleiste. Mit einem Pfadtrenner
darin war er kein Zusatz mehr:

```yaml
# ~/.config/redact-rs/settings.yaml
output_suffix: "/../../ziel/alle"
```

So sah der Lauf **vor der Korrektur** aus. Der Mitschnitt ist historisch: die
Zusammenfassung nannte damals zwei Zahlen, seit 0.3.0 sind es drei, und den
Aufruf selbst lässt das Programm heute gar nicht mehr zu (siehe unten).

```
$ redact-rs b3/ --force
b3/drei.pdf → b3/drei/../../ziel/alle.pdf (0 Schwärzung(en))
b3/eins.pdf → b3/eins/../../ziel/alle.pdf (0 Schwärzung(en))
b3/zwei.pdf → b3/zwei/../../ziel/alle.pdf (0 Schwärzung(en))

3 Datei(en): 3 verarbeitet, 0 fehlgeschlagen.
$ echo $?
0
```

In `ziel/alle.pdf` stand nur das Ergebnis der **letzten** Datei. Drei Dinge
gingen dabei schief, und der Rückgabewert meldete keines davon: der Dateistamm
war weg, alle Stapel-Ergebnisse kollidierten auf demselben Pfad (genau das, was
`batch::reject_single_target_switches` für `-o` verhindert), und geschrieben
wurde außerhalb des Eingabeverzeichnisses — die Zwischenverzeichnisse legte
`check_target` selbst mit `create_dir_all` an. Symlinkschutz und Eingabeschutz
hielten; sie sind die letzte Bremse, nicht die erste.

Geprüft wird jetzt an zwei Stellen: beim Lesen der Einstellungsdatei
(`Settings::validate`) und vor dem ersten Schreibziel
(`redact_pipeline::plan_outputs`, gilt damit für Kommandozeile, Stapel und
Oberfläche). Zusätzlich neutralisiert `redact_core::output_path_with_suffix`
Pfadtrenner zu `_` — erreicht wird das im laufenden Programm nie, es ist die
Bremse für einen künftigen Aufrufer, der beide Prüfungen vergisst.

Derselbe Aufruf heute, mit dem ausgelieferten Binary 0.3.0
(`x86_64-linux-musl`) auf drei unveränderten Kopien der Beispieldatei:

```
$ REDACT_RS_CONFIG=s.yaml redact-rs b3/ --force
Fehler: Konfigurationsfehler: s.yaml: der Namenszusatz „/../../ziel/alle“ ist keiner: er enthält einen Pfadtrenner. Der Zusatz wird an den Dateinamen der Eingabe angehängt und darf deshalb nur aus Namensbestandteilen bestehen — mit einem Pfadtrenner darin ginge der Dateistamm verloren, im Stapel schriebe jede Datei auf dasselbe Ziel, und geschrieben würde außerhalb des Eingabeverzeichnisses. Wer die Ausgabe woanders haben will, gibt sie mit -o an.
$ echo $?
2
```

Dieselbe Ablehnung kommt über `--output-suffix`; dort ist sie ein
Datei-Fehlschlag je Eingabe statt eines Abbruchs vor dem ersten Schreiben.

Und ohne den Zusatz — so sieht die Zusammenfassung eines Stapels seit 0.3.0
aus, drei Zahlen statt zweier:

```
$ redact-rs b3/ --force
[1/3] b3/drei.pdf
[2/3] b3/eins.pdf
[3/3] b3/zwei.pdf
b3/drei.pdf → b3/drei_geschwaerzt.pdf (7 Schwärzung(en))
b3/eins.pdf → b3/eins_geschwaerzt.pdf (7 Schwärzung(en))
b3/zwei.pdf → b3/zwei_geschwaerzt.pdf (7 Schwärzung(en))

3 Datei(en): 3 vollständig geprüft, 0 verarbeitet (aber nicht vollständig geprüft), 0 fehlgeschlagen.
$ echo $?
0
```

Die mittlere Zahl steht für sich, weil „verarbeitet" vorher auch für Dateien
galt, deren Text niemand gelesen hatte: zwanzig Auszüge ergaben „20
verarbeitet, 0 fehlgeschlagen" und Rückgabewert 0, obwohl in einer davon eine
Kontonummer unberührt stand. Zählt die mittlere Zahl über null und ist keine
Datei gescheitert, endet der Lauf mit Rückgabewert 3 statt 0
(`crates/redact-cli/src/batch.rs`, `exit_code`); eine gescheiterte Datei
schlägt das mit 1.

### Fremde Zeichen auf dem Terminal

Ein Dateiname darf unter Unix jedes Byte außer `/` und `NUL` enthalten, also
auch `ESC [ 3 1 m` oder `ESC [ 2 K`. Roh ausgegeben führt das Terminal die
Folge aus. `od -c` auf die Zusammenfassung eines Stapels zeigte vorher

```
b   6   /   a  033   [   3   1   m   r   o   t  033   [   2   K   .   p   d   f
```

Damit ließ sich die Zusammenfassung optisch fälschen — Zeilen löschen, den
Cursor hochfahren, aus „1 fehlgeschlagen“ ein „0 fehlgeschlagen“ machen. Wer
ein Schwärzungsergebnis an dieser Zusammenfassung prüft, prüft dann das, was
der Absender der Datei zeigen wollte.

Jeder Text, der aus einer Datei stammt, geht deshalb durch
`redact_core::safe_text`: C0-Steuerzeichen einschließlich Zeilenumbruch, `DEL`,
die C1-Zeichen und die Richtungsumschalter (`U+202A`–`U+202E`, `U+2066`–`U+2069`
— `rechnung<U+202E>fdp.exe` liest sich sonst als `rechnungexe.pdf`) werden zur
sichtbaren Form `\u{1b}`. Betroffen sind die Zusammenfassung, die
Stapelmeldungen und die eine Stelle, an der jeder Fehler herauskommt
(`main::main`). In `--json` bleibt der Name vollständig: `serde_json` schreibt
Steuerzeichen selbst als ``, und dort liest ihn ein Programm, keine Anzeige.

### Die Einstellungsdatei als Vorleser

`REDACT_RS_CONFIG` zeigt auf einen beliebigen Pfad, und an der Vorgabestelle
kann ein Symlink stehen. Zeigt einer der beiden auf eine fremde Datei, lautete
die Meldung

```
Fehler: Konfigurationsfehler: …/s4.yaml: Einstellungen nicht lesbar:
        unknown field `root:*:20501:0:99999:7::`, …
```

Keine Rechtegrenze wird dabei überschritten — das Programm läuft mit den
Rechten des Nutzers, der die Datei ohnehin lesen dürfte. Es war aber ein Weg,
beliebige Zeilen einer fremden Datei in Protokolle, Fehlerberichte und
Bildschirmfotos zu befördern, und dafür gibt es keinen Grund.

Die Meldung wird jetzt selbst gebaut (`settings::describe_yaml_error`): Art des
Fehlers, Zeile und Spalte, die erlaubten Schlüssel. Der beanstandete Schlüssel
wird nur wiedergegeben, wenn er *wie ein Schlüssel dieser Datei aussieht* —
höchstens 32 Zeichen, ASCII, beginnend mit Buchstabe oder `_`, danach
Buchstaben, Ziffern, `_` und `-`. Ein Tippfehler (`output_sufix`) ist damit
weiterhin beim Namen genannt, `root:*:20501:0:99999:7::` nicht:

```
Fehler: Konfigurationsfehler: /etc/shadow: Einstellungen nicht lesbar
(Zeile 1, Spalte 1): unbekannter Schlüssel. Erlaubt sind: output_suffix,
patterns, min_confidence, padding, theme. (Der Inhalt der Datei wird hier
nicht wiedergegeben — REDACT_RS_CONFIG und die Vorgabestelle können auf eine
beliebige fremde Datei zeigen.)
```

Dieselbe Überlegung gilt für die Größe: die Einstellungsdatei wird nur gelesen,
wenn sie eine gewöhnliche Datei unter 1 MB ist. Sonst wäre `REDACT_RS_CONFIG`
auf eine dünn belegte 6-GB-Datei derselbe Speicherfehler wie oben, nur an einer
anderen Stelle.

---

## Messungen

Alle Zahlen aus demselben Release-Build (`x86_64-unknown-linux-gnu`, 16 GB RAM),
Spitzenspeicher über `getrusage(RUSAGE_CHILDREN).ru_maxrss` des Kindprozesses —
nicht über eine Abtastschleife, die den Spitzenwert verpassen kann.

### Tiefe Verschachtelung (RUSTSEC-2026-0187)

400-kB-Datei mit 200 000 offenen `[`:

| | vorher | nachher |
|---|---|---|
| Ergebnis | **SIGABRT**, Exit 134 | Exit 1 mit Meldung |
| Laufzeit | 0,01 s | 0,00 s |
| Spitzenspeicher | 15,2 MB | 7,9 MB |

Reproduziert in drei Varianten, alle drei stürzten ab und werden jetzt alle drei
abgelehnt: als gewöhnliches Objekt, versteckt in einem Flate-komprimierten
Objekt-Stream (971 Byte Datei) und im Flate-komprimierten Seiteninhalt
(862 Byte Datei).

Die Grenze liegt bei **100** (`Limits::default().max_nesting_depth` in
`crates/redact-pdf/src/document.rs`) — dieselbe Zahl wie in der Tabelle unter
„Grenzen für Eingabedateien“, und weit über allem, was in echten Dokumenten
vorkommt.

Sie ist nicht geraten, sondern **am Verhalten der Bibliothek nachgemessen**:
`the_depth_limit_is_exactly_what_lopdf_still_parses`
(`crates/redact-pdf/tests/nesting_bomb.rs`) baut eine Datei mit genau 100 Ebenen
und eine mit 101. Bei 100 liest `lopdf` alle Objekte ein und die Vorprüfung
lässt die Datei durch; bei 101 verliert `lopdf` das Objekt stillschweigend —
und genau deshalb lehnt die Vorprüfung ab. Zieht ein `lopdf`-Update die
Schwelle um, fällt dieser Test in beide Richtungen auf: er schlägt an, wenn 100
zu hoch geworden ist, und ebenso, wenn es zu niedrig geworden ist.

### Dekompressionsbomben

Content-Stream aus wiederholtem `0 0 0 rg\n`, zlib-komprimiert:

| entpackt | Datei | vorher | nachher |
|---|---|---|---|
| 200 MB | 389 kB | Exit 0, **12 402 MB**, 51,2 s | Exit 1, 20,9 MB, 0,03 s |
| 2 GB | 3,9 MB | **SIGABRT** (Speicheranforderung gescheitert bei 12 GB), 51,4 s | Exit 1, 27,5 MB, 0,04 s |

Der Verstärkungsfaktor wurde separat gemessen und ist linear:

| entpackter Content-Stream | Spitzenspeicher |
|---|---|
| 8 MB | 500 MB |
| 16 MB | 996 MB |
| 32 MB | 1 988 MB |
| 64 MB | 3 972 MB |

Rund **62 Byte Arbeitsspeicher je Byte Content-Stream**. Der Grund ist nicht das
Auspacken, sondern das Parsen: aus jedem Operator wird eine eigene
`lopdf::content::Operation` mit eigenem Vektor. Deshalb gibt es zwei Budgets —
ein großes für alle Streams (Bilder, Schriften, eingebettete Dateien werden nur
gespeichert) und ein sehr viel engeres für die Streams, die tatsächlich geparst
werden.

**Der Faktor hängt von der Form des Stroms ab, nicht nur von seiner Größe.**
Nachgemessen an derselben Maschine (Release, `ru_maxrss` des Kindprozesses,
`--no-patterns`), jeweils ein Seiteninhalt aus einem wiederholten Baustein:

| Baustein | 15 MB Strom → Spitzenspeicher | Byte je Byte |
|---|---|---|
| `0 0 0 rg` (1 Operand) | 1 014 MB | 68 |
| `10 20 30 40 re f` (4 Operanden) | 1 064 MB | 71 |
| `q 1 0 0 1 100 700 cm Q` (6 Operanden) | 1 479 MB | 99 |
| `[1 1 1 1 1 1 1 1 1 1] 0 d` (Array mit 10 Elementen) | 1 506 MB | **100** |

Je mehr *Operanden* auf einen Operator kommen, desto teurer wird das Byte: die
62 aus der Tabelle darüber sind der günstige Fall, nicht der ungünstige.
Maßgeblich ist deshalb die letzte Zeile: **rund 100 Byte je Byte Content-Stream
im ungünstigsten hier gemessenen Fall.** Bei der Vorgabe `--max-parsed-mb 16`
folgt daraus eine Obergrenze von etwa **1,6 GB**, nicht 1 GB.

Wer das gegen eine Maschine mit wenig Arbeitsspeicher absichern will, setzt
`--max-parsed-mb` herunter; der Wert wirkt linear.

### Verschlüsselte Bomben

Dieselben beiden Bomben wie oben, nur RC4-verschlüsselt (Standard-Handler,
`/V 1 /R 2`) und mit bekanntem Passwort. „vorher“ ist der Stand, in dem nach
`Document::load_mem_with_options` nur `validate` lief.

| Datei | Aufruf | vorher | nachher |
|---|---|---|---|
| 196 kB, Content-Stream **64 MB** entpackt | ohne Passwort | Exit 1, 8,0 MB, 0,0 s | unverändert |
| dito | mit Passwort | **SIGKILL** durch den systemweiten OOM-Killer; unter `ulimit -v 4 GB` stattdessen **SIGABRT**, Exit 134, nach 10,6 s bei **3 876 MB** | Exit 1, **22,8 MB**, 0,0 s |
| 1,3 kB, **200 000** offene `[` im Content-Stream | ohne Passwort | Exit 1 (Tiefengrenze) | unverändert |
| dito | mit Passwort | **Exit 0**, Ausgabe geschrieben, IBAN unverändert darin, 8,0 MB | Exit 1, **7,9 MB**, 0,0 s |
| 902 Byte, harmloses verschlüsseltes PDF | mit Passwort | Exit 0, 7,9 MB | unverändert |
| 31 kB, Content-Stream **15 MB** (knapp unter `--max-parsed-mb`) | ohne / mit Passwort | — | Exit 0, **1 013,7 MB** / **1 013,4 MB** |

Die dritte Zeile ist der stillere und deshalb schlimmere Fall. Der Lauf endete
mit „0 Schwärzungen“ und schrieb eine Ausgabedatei; `redact_pdf::leaks` fand die
IBAN darin unverändert. `lopdf` bekommt einen so tief verschachtelten
Content-Stream nicht in Operationen zerlegt, die Analyse sieht also keinen
Text — und „0 Schwärzungen“ liest sich wie „nichts zu schwärzen“. Ein Absturz
fällt auf; das hier nicht.

Dieser Fall ist inzwischen **doppelt** zu. Zum einen greifen die Budgets auch
hinter der Entschlüsselung (die Zeile „nachher“). Zum anderen ist ein
Seiteninhalt, der sich nicht in Operationen zerlegen lässt, seither ein Grund,
die **Datei abzulehnen** — siehe „Eine Seite, die sich nicht zerlegen lässt,
kostet die ganze Datei“. Selbst wenn eine Datei künftig an beiden Budgets
vorbeikäme, endete sie nicht mehr mit einer Ausgabe voller ungeschwärzter IBAN.

Die letzte Zeile ist die eigentliche Gegenprobe: eine Datei **knapp unterhalb**
des Budgets kostet verschlüsselt auf 0,3 MB genau so viel wie unverschlüsselt.
Beide Wege messen jetzt dasselbe und lassen dasselbe durch — vorher hing an
einem Passwort, ob überhaupt gemessen wurde.

Der Verstärkungsfaktor ist ebenfalls derselbe wie ohne Verschlüsselung:
gemessen an einer verschlüsselten 13-kB-Datei mit 4 MB Content-Stream aus
wiederholtem IBAN-Text **1 189 MB** — dort trägt die Trefferverwaltung den
größeren Teil, weshalb dieser Wert deutlich über den 62–100 Byte je Byte des
reinen Parsens liegt. Bei 64 MB wären das gut 19 GB; die Maschine hat 16.

Diese Zeile ist inzwischen historisch: dieselbe Datei kommt heute gar nicht
mehr so weit. 4 MB Seiteninhalt aus wiederholtem IBAN-Text setzen rund
1,9 Mio. Zeichen auf **einer** Seite, und die Glyphengrenze (siehe „Grenzen für
Eingabedateien“) bricht den Scan bei einer Million ab — nachgemessen Exit 1 bei
**521 MB** statt 1 189 MB. Die Trefferverwaltung kann also nicht mehr in die
Größenordnung kommen, in der sie hier gemessen wurde.

Die Fälle stehen als Prüfmaterial im Baum
(`crates/redact-pipeline/src/testdata/bombe_verschluesselt.pdf` und
`bombe_verschachtelt.pdf`, 33 kB bzw. 1,3 kB) und werden von
`crates/redact-cli/tests/hardening.rs` gemessen. Der Test prüft nicht auf
Megabyte, sondern auf das, was sich in einem Testfall sauber messen lässt:
`status.code().is_some()` — ein durch ein Signal beendeter Prozess hat unter
Unix **keinen** Rückgabewert, genau daran ist ein Speicherfehler zu erkennen.

### Unbegrenzte Eingabedatei

Eine dünn belegte Datei (`truncate -s 6G`): 6 442 450 944 Byte Nennlänge, 4 kB
wirklich auf der Platte. `std::fs::read` legt einen Puffer in Dateigröße an,
bevor irgendetwas geprüft wird.

| | vorher | nachher |
|---|---|---|
| einzeln (`redact-rs riesig.pdf -o out.pdf`) | Exit 1 nach **19,9 s**, **6 149 MB** | Exit 1 nach **0,0 s**, **7,9 MB** |
| in einem Stapelverzeichnis mit zwei gesunden Dateien | Exit 1 nach **27,0 s**, **6 152 MB** | Exit 1 nach **0,1 s**, **8,8 MB** |

Linear skalierend: 32 GB Nennlänge reißen den Rechner um, und dafür braucht es
weder Rechte noch Plattenplatz.

Zum Stapel gehört die zweite Hälfte des Befunds: die Datei wurde **nicht einmal
namentlich genannt**, solange sie den Lauf aufhielt. Vor der Stapelverarbeitung
suchte immer ein Mensch die Datei aus; jetzt genügt eine Datei im Verzeichnis.
Seither steht jeder Name mit Zähler auf stderr, **bevor** die Datei geöffnet
wird (stderr, weil `--json` seine Zusammenfassung nach stdout schreibt):

```
[1/3] b3/a.pdf
[2/3] b3/riesig.pdf
FEHLGESCHLAGEN b3/riesig.pdf: PDF-Fehler: b3/riesig.pdf: 6144 MB groß,
erlaubt sind 512 MB (--max-input-mb). …
[3/3] b3/z.pdf
```

### Hilfsdateien ohne Grenze

Derselbe Befund, eine Schalterreihe weiter. Die Grenze der vorigen Messung galt
für die **Eingabe-PDF**; die vier Dateien, die der Bedienende daneben mitgibt —
Buchungsliste, Review-Datei, Regionsliste, Musterkonfiguration —, gingen durch
`std::fs::read`/`read_to_string` und hatten gar keine. Wieder eine dünn belegte
Datei (`truncate -s 6G`): 6 442 450 944 Byte Nennlänge, 4 kB wirklich auf der
Platte.

**„vorher“ ist an einem Release-Binary des Standes gemessen, in dem der Schalter
noch ungebremst las; „nachher“ am Binary desselben Arbeitsbaums, nachdem die
Grenze stand.** Beide Läufe auf derselben Maschine, `/usr/bin/time -v` des
Kindprozesses, dieselbe 6-GB-Datei, jeweils mit dem Demo-Kontoauszug als
Eingabe.

| Schalter | Grenze | vorher | nachher |
|---|---|---|---|
| `--manual-regions` | 16 MB | Exit 1 nach **24,0 s**, **6 150 MB** | Exit 1 nach **0,00 s**, **6,3 MB** |
| `--apply-review` | 16 MB | — (nicht selbst gemessen) | Exit 1 nach **0,00 s**, **5,6 MB** |
| `--booking-list` | 16 MB | — (nicht selbst gemessen) | Exit 1 nach **0,00 s**, **6,6 MB** |
| `--patterns-config` | 1 MB | — (nicht selbst gemessen) | Exit 1 nach **0,00 s**, **6,5 MB** |
| Gegenprobe: gesunder Lauf ohne Hilfsdatei | — | — | Exit 0 nach **0,01 s**, **8,8 MB** |

Die Zeile `--manual-regions` ist die einzige, für die hier ein eigener
Vorher-Wert steht — sie war beim Messen als letzte noch offen. Für die übrigen
drei lag die Grenze bereits, als gemessen wurde; die Vorher-Werte, die in den
Modulkommentaren von `crates/redact-core/src/read.rs` stehen, sind **nicht**
nachgemessen worden und deshalb hier nicht wiedergegeben. Der Mechanismus ist in
allen vier Fällen derselbe, und die 6 150 MB der ersten Zeile zeigen, was er
kostet.

Die Meldung nennt Nennlänge, Grenze und — das ist der eigentliche Zweck — die
wahrscheinlichere Ursache:

```console
$ redact-rs k.pdf -o out.pdf --manual-regions gross.json --no-patterns
Fehler: Parse-Fehler: gross.json: 6144 MB (6442450944 Byte) groß, erlaubt sind
16 MB. Für eine Review-Datei oder eine Regionsliste ist das eine feste Grenze und
keine Einstellung: gemessen sind rund 600 Byte je geprüfter Stelle, 16 MB fassen
also gut 25 000 — von Hand geprüft werden Dutzende bis Hunderte. Zeigt
`--apply-review` bzw. `--manual-regions` wirklich auf die JSON-Datei und nicht
auf die PDF-Datei?
$ echo $?
1
```

**Die Größe ist nicht die erste Frage.** Eine benannte Pipe hat die Länge 0 und
liefert trotzdem endlos; ein Zeichengerät ebenso. Beides wird abgelehnt, bevor
eine Größe überhaupt zur Sprache kommt — nachgemessen an einer `mkfifo`-Pipe,
aus der ein `yes` schrieb (Exit 1 nach **0,01 s**, **6,5 MB**), an `/dev/zero`
hinter `--manual-regions` und an einem Verzeichnis hinter `--booking-list`:

```console
$ redact-rs k.pdf -o out.pdf --booking-list pipe.csv
Fehler: Buchungslisten-Fehler: pipe.csv: keine gewöhnliche Datei. Gelesen werden
nur Dateien — eine Pipe oder ein Gerät hätte keine Größe, an der sich eine Grenze
festmachen ließe, und lieferte weiter, bis der Arbeitsspeicher voll ist.
$ echo $?
1
```

### Speicherbedarf der Bildschwärzung

Eingaben: 1-Bit-Graustufenbilder (`/DeviceGray`, `/BitsPerComponent 1`,
`/FlateDecode`) — die gewöhnliche Kodierung eines Schwarzweiß-Scans, Faktor 32
zwischen roh und dekodiert. Je Zeile eine Schwärzung. „vorher“ ist der Stand
vor Aufgabe #58.

| Eingabe | Inhalt | vorher | nachher |
|---|---|---|---|
| 23 kB | 5 Bilder 6000×6000, 1 Seite | 8,3 s, **1 387 MB** | 6,3 s, **186 MB** |
| 46 kB | 10 Bilder | 19,0 s, **2 761 MB** | 12,8 s, **186 MB** |
| 92 kB | 20 Bilder | 34,3 s, **5 508 MB** | 25,5 s, **187 MB** |
| 92 kB | 20 Bilder, **nur eines** geschwärzt | 24,1 s, **2 898 MB** | **1,4 s**, **186 MB** |
| 92 kB | dito mit `--max-decompressed-mb 128 --max-parsed-mb 1` | 25,7 s, **2 898 MB** | 1,4 s, 186 MB |
| 5,9 kB | 20 Seiten, **ein** geteiltes A4-300-dpi-Bild | 6,8 s, **705 MB** | 6,1 s, **49 MB** |
| 61 kB | 40-seitiger A4-300-dpi-Scan, je 1 Schwärzung | 13,1 s, **1 369 MB** | 12,1 s, **50 MB** |
| 183 kB | 40 Bilder, `ulimit -v 4194304` | **SIGABRT, Exit 134** | Exit 0, 51,0 s, **189 MB** |

Der Normalfall — der 40-seitige Scan — kostete rund 34 MB je Seite, die bis
zum Ende liegen blieben; ein 200-Seiten-Stapel wäre bei etwa 7 GB gelandet.
Heute ist der Bedarf von der Seitenzahl unabhängig.

Drei Ursachen, die sich multiplizierten, und was an ihre Stelle getreten ist:

1. Die Bildschwärzung ließ sich die ganze Seite über `ops::page_ops` dekodieren
   und griff sich daraus das eine Bild. 19 unbeteiligte Bilder kosteten so
   2,9 GB. Heute trägt jede Bildplatzierung selbst, was zum Dekodieren nötig
   ist, und es wird nur ausgepackt, was eine Schwärzung wirklich schneidet.
2. Der Arbeitspuffer war eine *Kopie* der dekodierten Pixel, während das
   Original noch stand — daher der Unterschied 5 508 MB (alle Bilder berührt)
   zu 2 898 MB (eines berührt). Heute wird der dekodierte Puffer verbraucht,
   nicht geklont.
3. Alle Arbeitspuffer wurden über *alle* Seiten gesammelt und erst am Ende
   geschrieben. Deshalb genügten 5,9 kB Datei für 705 MB. Heute wird jedes
   Bild geschrieben und freigegeben, bevor das nächste ausgepackt wird.

Die Zusicherung „kontrollierter Abbruch statt Speicherfehler“ ist damit auch
für Bilder eingelöst. Zu eng gesetzt sieht das so aus:

```
$ redact-rs 40bilder.pdf -o out.pdf --manual-regions r.json --max-image-mb 64
Fehler: PDF-Fehler: Bild /Im0 auf Seite 1 bräuchte 137 MB dekodierte
Bildpunkte; zusammen mit den bereits gehaltenen 0 MB überschreitet das die
Grenze von 64 MB. Der Lauf wird abgebrochen, bevor die Speicheranforderung
scheitert — mit --max-image-mb lässt sich die Grenze bewusst anheben.
$ echo $?
1
```

### Rechenzeit

212-kB-Datei, 500 Seiten × 88 Zeilen, 264 000 Treffer:

| Seiten | Treffer | Laufzeit |
|---|---|---|
| 50 | 26 400 | 1,19 s |
| 100 | 52 800 | 5,42 s |
| 200 | 105 600 | 22,4 s |
| 500 | 264 000 | 132,6 s |

Vervierfachung bei Verdopplung — das Verhalten ist **quadratisch**. Ursache ist
`dedup` in `crates/redact-core/src/conflict.rs`: jede Region wird gegen alle
bereits behaltenen geprüft. Eine knappe Megabyte-Datei genügte damit, um die
Maschine eine Stunde zu beschäftigen. Bis das dort behoben ist (Bucketing nach
Seite macht daraus O(n log n)), begrenzt die Kette die Zahl der
Trefferkandidaten; der Lauf endet dann nach 4,7 s mit Exit 2.

### Rechenzeit über die Seitenzahl (Aufgabe #59)

Eine zweite, unabhängige quadratische Stelle — im Schreibpfad, nicht in der
Konfliktauflösung. 2,6-MB-Datei, je eine Schwärzung pro Seite:

| Seiten | vorher | nachher |
|---|---|---|
| 1 000 | 0,48 s | 0,10 s |
| 2 000 | 2,11 s | 0,18 s |
| 4 000 | 8,50 s | 0,38 s |
| 8 000 | **35,01 s** | **0,83 s** |
| 8 000, aber nur **eine** Schwärzung im ganzen Dokument | 0,64 s | 0,62 s |

Wieder Vervierfachung bei Verdopplung, und die letzte Zeile zeigt woran es
hing: die Kosten wuchsen mit dem *Produkt* aus Seitenzahl und Zahl der
geschwärzten Seiten. `replace_page_content` suchte je geschwärzter Seite über
**alle** Seiten nach geteilten Content-Streams, obwohl diese Menge von der
gerade geschwärzten Seite gar nicht abhängt. Sie wird jetzt einmal gebildet
(`ContentUsers` in `crates/redact-pdf/src/redact.rs`) und beim Ersetzen
fortgeschrieben — der Index antwortet Schritt für Schritt genau so wie die
wiederholte Suche, ein geteilter Strom wird also weiterhin nur dann gelöscht,
wenn ihn keine Seite mehr benutzt. Ebenfalls einmal statt je Seite gebildet:
die Zuordnung Schwärzung → Seite.

### Rückverfolgung in Mustern

`fancy-regex 0.14` setzt in `RegexOptions::default` ein Rückverfolgungslimit von
1 000 000 (`src/lib.rs:534`). Es ist also bereits gesetzt und muss nicht
nachgezogen werden. Gemessen:

* Muster ohne Rückwärtsreferenz und ohne Lookaround — etwa `(a+)+b` oder
  `(x+x+)+y` — reicht `fancy-regex` an die lineare `regex`-Maschine durch.
  Laufzeit unter 1 µs, unabhängig von der Eingabelänge. Klassisches
  katastrophales Backtracking gibt es dort nicht.
* Muster, die die Rückverfolgungsmaschine erzwingen — etwa `(a+)+\1b` —
  erreichen das Limit nach rund 17 ms und liefern
  `Err(RuntimeError(BacktrackLimitExceeded))`.
* `redact-patterns` reicht diesen Fehler als `RedactError::Pattern` weiter und
  bricht den Lauf ab. Kein `unwrap`, keine Endlosschleife.

Musterdateien gelten ohnehin als vertrauenswürdig (siehe Bedrohungsmodell); das
Limit ist die zweite Verteidigungslinie gegen einen Tippfehler.

---

## Was ausdrücklich NICHT zugesichert wird

### `unsafe` in Abhängigkeiten, die Angreiferdaten parsen

`#![forbid(unsafe_code)]` gilt für den eigenen Code, nicht für den Unterbau.
Diese Crates sehen Bytes aus dem Eingabe-PDF und enthalten `unsafe`.

**Die erste Tabelle gilt schon für die reine Kommandozeile** — also für das
musl-Archiv des Releases, gebaut mit `--no-default-features`. Hier stand
früher eine Tabelle, in der die Oberfläche den größten Teil der Fläche
stellte; das stimmte nicht, und die Empfehlung darunter („wer nur die
Kommandozeile braucht, baut ohne die Oberfläche") las sich dadurch wie ein
Ausweg. Sie **verkleinert** die Fläche, sie beseitigt sie nicht.

| Crate | `unsafe` | wo die Angreiferdaten herkommen |
|---|---|---|
| `memchr` 2.8.3 | 340 | Substringsuche der Mustererkennung über den extrahierten PDF-Text (SIMD) |
| `encoding_rs` 0.8.35 | 271 | UTF-16BE-Dekodierung der PDF-Strings (`lopdf/src/encodings/mod.rs:13`) |
| `aho-corasick` 1.1.4 | 227 | Mehrmustersuche unter `regex-automata`/`fancy-regex`, ebenfalls über den PDF-Text |
| `aes` 0.8.4 | 110 | entschlüsselt das Eingabe-PDF (`lopdf/src/encryption/algorithms.rs`) |
| `zune-jpeg` 0.5.15 | 85 | dekodiert eingebettete JPEGs — auch ohne Oberfläche, für die Bildschwärzung |
| `regex-automata` 0.4.16 | 57 | die Regex-Maschine selbst |
| `flate2` 1.1.9 | 37 | packt jeden komprimierten Stream aus |
| `simd-adler32` 0.3.10 | 36 | Prüfsumme jedes ausgepackten Streams (unter `miniz_oxide`) |
| `miniz_oxide` 0.8.9 | 3 | der Inflate-Kern unter `flate2` |

Nur im Bau **mit** Oberfläche kommen dazu:

| Crate | `unsafe` | wo die Angreiferdaten herkommen |
|---|---|---|
| `bytemuck` 1.25.2 | 376 | unter `skrifa` und `tiny-skia` |
| `tiny-skia` 0.12.0 | 152 | rastert Pfade und Koordinaten aus dem PDF (Vorschau) |
| `eframe` 0.29.1 | 21 | mittelbar — das Fenster selbst |
| `skrifa` 0.33.2 / `read-fonts` 0.31.3 | 0 (selbst) | lesen eingebettete Schriften, stützen sich auf `bytemuck` |

Ein Speicherfehler in einer dieser Bibliotheken ist ein Speicherfehler in
redact-rs. Wer ein PDF aus wirklich unbekannter Quelle verarbeitet, sollte das
in einer Sandbox tun (Container, `bwrap`, eigenes Benutzerkonto). Der Bau ohne
Oberfläche (`cargo build --release -p redact-cli --no-default-features`) nimmt
`bytemuck`, `tiny-skia` und `eframe` heraus — die neun Crates der ersten
Tabelle bleiben.

**Nachrechnen** statt glauben; die Zahlen oben stammen aus genau diesem Lauf
(gezählt wird das Schlüsselwort `unsafe` in `src/`, Kommentare und
Zeichenketten eingeschlossen — ein grobes Maß für Fläche, keine Prüfung):

```bash
cargo tree -p redact-cli --no-default-features -e normal --prefix none --no-dedupe \
  | sed 's/ (proc-macro)//' | grep -v '^redact-' | sort -u \
  | while read -r name ver; do
      d=$(echo ~/.cargo/registry/src/*/"$name-${ver#v}")
      [ -d "$d/src" ] && printf '%6s  %s %s\n' \
        "$(grep -rho '\bunsafe\b' --include='*.rs' "$d/src" | wc -l)" "$name" "$ver"
    done | sort -rn
```

Ohne `--no-default-features` zählt derselbe Aufruf den Graphen **mit**
Oberfläche; dort stehen `linux-raw-sys`, `glow`, `rustix` und `winit` weit
oben. Sie sehen keine PDF-Bytes — sie reden mit Kernel, Fenstersystem und
Grafiktreiber — und stehen deshalb in keiner der beiden Tabellen.

Zwei Nachbarn, die nicht in die Tabellen gehören, aber genannt sein sollen:

* **`unsafe-libyaml` 0.2.11 (240 `unsafe`, transpiliertes C)** liegt unter
  `serde_yaml` und liest Einstellungs- und Musterdatei; `serde_json` 1.0.151
  (16) liest Review- und Regionsdateien. Nach dem Bedrohungsmodell sind das
  alles vertrauenswürdige Eingaben — sie stehen deshalb nicht in der Tabelle.
  Unter den Bibliotheken, die diese Dateien lesen, ist `unsafe-libyaml` mit
  Abstand die größte `unsafe`-Fläche (`serde_yaml` selbst: 60, `serde_json`:
  16, `csv`: 4). Wer Musterdateien aus fremder Hand einliest, verlässt das
  Bedrohungsmodell an dieser Stelle.
* **`ttf-parser` 0.25.1** ist nicht mehr gepflegt (RUSTSEC-2026-0192) und
  liegt im Abhängigkeitsgraphen, ist aber ausschließlich über
  `lopdf::FontData::new` erreichbar, und redact-rs ruft das nirgends auf
  (`git grep FontData -- crates/` findet nichts). Kein Angreiferpfad,
  nur toter Ballast.

Anmerkung zur Einordnung: `lopdf` selbst enthält **kein** `unsafe` — und stürzte
in 0.34 trotzdem ab (RUSTSEC-2026-0187). „Sicheres Rust“ schützt vor
Speicherfehlern, nicht vor unbegrenzter Rekursion und nicht vor unbegrenztem
Speicherverbrauch.

### Dienstverweigerung

Die oben gemessenen Fälle sind begrenzt. Nicht begrenzt sind:

* **Andere Wege in die Rekursion.** Die Vorprüfung zählt `[` und `<<`. Findet
  jemand einen anderen Pfad in `lopdf`, der tief rekursiert, greift sie nicht.
  RUSTSEC-2026-0187 selbst ist mit `lopdf 0.42` behoben (siehe unten); die
  Vorprüfung deckt seither nicht mehr eine offene Schwachstelle zu, sondern
  begrenzt den Aufwand.
* **`LZWDecode`.** Solche Streams packt `lopdf` aus, nicht die Vorprüfung. Sie
  werden deshalb bis zu einer Rohgröße von 16 MB an `lopdf` durchgereicht;
  wieviel Speicher der Dekoder dabei belegt, ist nicht vorab begrenzt. Über
  16 MB Rohgröße wird die Datei abgelehnt. `LZWDecode` ist ein Filter aus der
  Zeit vor PDF 1.4 und kommt in heutigen Dateien praktisch nicht mehr vor.
* **Rechenzeit unterhalb der Grenzen.** 100 000 Trefferkandidaten kosten
  rund 17 s. Das ist gewollt großzügig; wer engere Zusagen braucht, setzt
  `--max-candidates` herunter.
* **Sehr große Bilder.** Ihre entpackte Größe zählt gegen das große Budget.

  **Richtigstellung — das war eine Lücke, keine Feinheit.** Hier stand bis
  einschließlich dieser Fassung: *„Streams mit `/Subtype /Image` werden nicht
  auf Klammertiefe untersucht — hier ist das belegbar unbedenklich, weil
  `lopdf` sie gar nicht auspackt."* Der Satz war falsch, und er hat eine
  Umgehung gedeckt: `/Subtype /Image` an ein Stream-Dictionary zu schreiben
  kostet nichts, das Dictionary gehört dem Angreifer. Die Begründung galt für
  `lopdf 0.34`; **seit 0.36 prüft `Stream::decompressed_content` das `/Subtype`
  nicht mehr**, und `Document::get_page_content` packt einen Seiteninhalt mit
  `/Subtype /Image` ganz normal aus. Ein so beschrifteter Stream war damit ein
  Weg an der Tiefenprüfung und am engen Parse-Budget vorbei.

  Heute entscheidet über Budget und Tiefenprüfung ausschließlich der
  **ausgepackte Inhalt** (`looks_binary` in
  `crates/redact-pdf/src/document.rs`) — das einzige Kriterium, das nicht dem
  Angreifer gehört: wer PDF-Syntax unterbringen will, muss druckbare Zeichen
  schreiben. Zwei Tests halten das fest, jeweils gegen dieselbe Nutzlast unter
  sechs verschiedenen Dictionaries (`""`, `/Harmlos /Image`, `/Subtype /Image`,
  `/Length1 4711`, `/Type /Metadata`, `/Type /XRef`):
  `the_dictionary_does_not_decide_which_budget_applies` und
  `the_dictionary_does_not_switch_off_the_depth_check`.

  Nachtrag zur Bildschwärzung: Seit Schwärzungen die **Pixel** eines Bildes
  überschreiben, wird ein betroffenes Bild sehr wohl dekodiert — nach RGBA8,
  also 4 Byte je Pixel. Dagegen stehen **zwei** Grenzen, und beide werden
  gebraucht:

  * **40 000 000 Bildpunkte je Bild** (`MAX_IMAGE_PIXELS` in
    `crates/redact-pdf/src/ops.rs`), also rund 160 MB. Ein Bild darüber wird
    nicht dekodiert, sondern als Platzhalter geführt — und ein Platzhalter
    unter einer Schwärzung **bricht den Lauf ab** (dieselbe Behandlung wie
    `/JPXDecode` und `/CCITTFaxDecode`, siehe
    `crates/redact-pdf/src/image.rs`). Lieber ein Fehler als eine Datei, in der
    die Schwärzung nur obenauf liegt; `--allow-undecodable-images` hebt das
    bewusst auf.
  * **256 MB gleichzeitig gehaltene dekodierte Bildbytes** (`--max-image-mb`).

  **Richtigstellung.** Bis einschließlich Aufgabe #58 stand hier allein die
  Grenze je Bild — und las sich, als wäre damit der Speicherbedarf gedeckelt.
  Das war die gefährlichere Hälfte der Wahrheit: eine Grenze *je Bild* sagt
  nichts über die *Summe*. Gemessen an 1-Bit-Graustufenbildern (die gewöhnliche
  Kodierung eines Schwarzweiß-Scans) brachte eine **92-kB-Datei** mit 20 Bildern
  den Prozess auf **5 508 MB**, eine 183-kB-Datei mit 40 Bildern auf SIGABRT
  („memory allocation of 144000000 bytes failed“, Exit 134) — trotz gesetzter
  `--max-decompressed-mb` und `--max-parsed-mb`, denn die zählen Rohbytes. Und
  es traf nicht nur konstruierte Eingaben: ein gewöhnlicher 40-seitiger
  A4-Scan mit je einer Schwärzung brauchte 1 369 MB, ein 200-Seiten-Stapel
  entsprechend rund 7 GB. Die Zahlen vorher und nachher stehen unter
  „Speicherbedarf der Bildschwärzung“.

  Was heute gilt: es wird nur noch dekodiert, was eine Schwärzung wirklich
  schneidet, und immer nur **ein Bild zur Zeit** — geschrieben und freigegeben,
  bevor das nächste kommt. Die einzige Ausnahme ist ein Inline-Bild in einem
  Form-XObject, das von mehreren Seiten gezeichnet wird; es muss bis zum Ende
  gehalten werden und zählt gegen dasselbe Budget. Reicht das Budget nicht,
  endet der Lauf mit einer Meldung — geprüft wird **vor** dem Auspacken, anhand
  von `/Width` und `/Height`.

  Was das Budget **nicht** abdeckt: die Puffer, die *während* des Umkodierens
  eines einzelnen Bildes zusätzlich entstehen (entpackte Abtastwerte, die
  Graustufen- bzw. RGB-Bytes vor dem Deflate). Sie betragen zusammen rund das
  Anderthalbfache eines Bildes — der wirkliche Spitzenbedarf liegt also über
  dem eingestellten Wert. Bei der Vorgabe von 256 MB wurden 189 MB gemessen;
  der Grenzfall ist ein einzelnes Bild knapp unter 40 000 000 Bildpunkten, das
  mit rund 400 MB zu Buche schlägt. Die Grenze ist ein Riegel gegen das
  *Anhäufen* vieler Bilder, keine Zusage über den Gesamtverbrauch des
  Prozesses.

### Die Zusicherungen oben gelten auch für die Oberfläche

**Richtigstellung.** Bis einschließlich dieser Fassung stand hier, zwei der
Punkte unter „Was zugesichert wird“ beschrieben nur den Weg durch `redact-cli`,
und die Oberfläche weiche davon ab: sie schreibe Audit-Log und Review-Datei mit
`std::fs::write` (genannt waren `crates/redact-gui/src/state.rs:1023` und
`crates/redact-gui/src/app.rs:781`), und die `--max-…`-Grenzen wirkten dort
nicht. **Beides trifft nicht mehr zu.** Die genannten Stellen tragen heute
anderen Code; die Zusicherung ist stärker, als sie hier beschrieben war. Eine
Doku, die zu wenig verspricht, ist harmloser als eine, die zu viel verspricht —
aber sie schickt den Nutzer in den umständlicheren Weg, ohne dass es dafür einen
Grund gäbe.

Was heute gilt:

* **Der eine Schreibpfad ist der einzige — für alle drei Dateien.**
  Geschwärztes PDF, Review-Datei und Audit-Log gehen in beiden Programmen durch
  `redact_pdf::document::write_file`:
  * Review-Datei über `AppState::save_review_file` →
    `redact_pipeline::write_review_file` → `write_file` mit `secret_options`,
  * Audit-Log über `AppState::export` → `redact_pipeline::apply` →
    `AuditLog::write` → `write_file` mit `secret_options`.

  Damit gelten für beide der Modus `0600`, die Symlink-Prüfung, die
  Kanonisierung des Zielpfads, der Eingabeschutz und die
  `create_new`+`rename`-Sequenz. **Gemessen an der geschriebenen Datei**, nicht
  am Aufrufgraphen:

  | Test | misst |
  |---|---|
  | `export_removes_the_text_from_the_pdf` (`crates/redact-gui/src/state.rs`) | `assert_eq!(mode, 0o600)` auf das von der Oberfläche geschriebene Audit-Log |
  | `both_ways_write_the_same_review_file` (`crates/redact-cli/tests/cli_and_gui_agree.rs`) | `0600` auf **beide** Review-Dateien, die aus dem Binary und die aus der Oberfläche |

* **Die Grenzen für Eingabedateien gelten dort ebenfalls — und sind über
  `redact-rs --gui` auch einstellbar.** Die Oberfläche lädt nicht mehr über
  `redact_pdf::load_from_bytes` (also nicht mit `Limits::default()`), sondern
  über `redact_pipeline::load_document(bytes, &self.config)`
  (`crates/redact-gui/src/state.rs`) — dieselbe Ladefunktion, die
  `redact_pipeline::run` benutzt, mit derselben `Config`. Im Einzelnen:

  | Grenze | Weg in der Oberfläche |
  |---|---|
  | `--max-input-mb` | `AppState::load_document` liest über `redact_pipeline::read_input(path, self.config.max_input_bytes)` statt über `std::fs::read` |
  | `--max-decompressed-mb`, `--max-parsed-mb`, Tiefengrenze | `load_document` → `load_from_bytes_with_limits(bytes, &config.limits)`, und nach einer Entschlüsselung `check_limits_after_decryption` |
  | `--max-image-mb` | über `config.max_decoded_image_bytes` im Export |
  | `--max-candidates` | `AppState::analyze` → `redact_pipeline::collect_regions_for` → `check_candidate_budget` |
  | Aufwandskonto und Glyphengrenze je Seite | im Interpreter, also unterhalb beider Programme |

  `redact-rs --gui auszug.pdf --max-parsed-mb 4` wirkt damit wirklich: die
  Kommandozeile baut die `Config` und gibt sie unverändert an
  `redact_gui::run` weiter (`crates/redact-cli/src/main.rs`).

  **Was bleibt:** die Fensterfassung zum Doppelklicken (`redact-rs-gui.exe`,
  und ebenso das Entwickler-Binary `redact-gui`) hat gar keine Kommandozeile.
  Sie baut ein `Config::default()` und arbeitet deshalb immer mit den Vorgaben
  aus der Tabelle unter „Grenzen für Eingabedateien“ — die Grenzen sind dort
  nicht abgeschaltet, nur nicht verstellbar. Aus demselben Grund liest sie auch
  die Einstellungsdatei nicht: `Settings::load()` wird ausschließlich in
  `crates/redact-cli/src/main.rs` gerufen.

### Was in den beiden „privaten“ Dateien wirklich steht

Der Punkt „in beiden stehen die *gefundenen* Geheimnisse im Klartext“ trifft
für die Review-Datei uneingeschränkt zu, für das Audit-Log nur teilweise —
nachgemessen an einem Lauf über den Demo-Kontoauszug:

| | `review.json` | `audit.json` |
|---|---|---|
| gefundener Text (`"text": "DE89 …"`) | **ja**, je Treffer | nein |
| Seite und Rechteck je Treffer | ja | ja |
| Herkunft (`pattern: iban_de`, `booking: b002`) | ja | ja |
| Text der **Negativlisten**-Einträge | **ja** | **ja** (`blocked_by_negative_list[].pattern`) |
| Begründung manueller Regionen | ja | ja |

Das Audit-Log verrät den geschwärzten Wert also nicht direkt — zusammen mit dem
Original lokalisiert es aber jede Schwärzung punktgenau, und die Einträge der
Negativliste stehen wörtlich darin. Beide Dateien gehören dorthin, wo auch das
ungeschwärzte Original liegen darf, und sonst nirgendwohin.

### Die Prüfsummen-Sperre der Review-Datei gilt an jedem Schalter

`--apply-review` vergleicht den SHA-256 der verarbeiteten Bytes mit
`input.sha256` der Review-Datei und bricht bei Abweichung ab
(`crates/redact-pipeline/src/lib.rs`, `check_review_identity`). Zwei Löcher
darin sind geschlossen:

* Eine **leere** Prüfsumme kehrte ohne Befund zurück; wer `"sha256": ""` von
  Hand eintrug, hebelte die Prüfung vollständig aus. Sie wird jetzt abgelehnt.
  `--allow-unverified-review` ist der ausdrückliche Weg daran vorbei — eine
  *falsche* Prüfsumme bleibt auch damit abgelehnt.
* `--manual-regions` nimmt beide Dateiformate an und prüfte **gar nicht**.
  Dieselbe Datei, die `--apply-review` mit Exit 2 zurückwies, ging hinter dem
  anderen Schalter wortlos durch — die Rechtecke landeten an beliebigen Stellen,
  und das Audit-Log meldete „applied“. Wird dort eine Review-Datei erkannt,
  gilt jetzt dieselbe Prüfung.

Ein **nacktes Regions-Array** hinter `--manual-regions` bleibt ungeprüft: es
nennt keine Herkunft und behauptet auch keine. Es ist das Format für von Hand
geschriebene Koordinaten; wer es benutzt, wählt die Seitenzahlen selbst. Was
jede einzelne Region bewirkt hat, steht danach im Audit-Log (`effect` je
Eintrag) — eine Region auf einer nicht vorhandenen Seite wird als
`missing_page` geführt und nicht als Schwärzung verbucht.

### Die Restlücke bei der Symlink-Prüfung

Unter Unix wäre `O_NOFOLLOW` (über `OpenOptionsExt::custom_flags`) der saubere
Weg — er verlangt aber die Konstante aus `libc` und damit eine zusätzliche
Abhängigkeit, die dieser Arbeitsschritt nicht aufnehmen durfte. Stattdessen:

1. `symlink_metadata` auf das Ziel; ist es ein Link, wird abgelehnt.
2. Geschrieben wird nicht auf das Ziel, sondern auf eine frische temporäre
   Datei mit `create_new(true)`. Das ist `O_CREAT | O_EXCL` und scheitert an
   einem Symlink, auch an einem ins Leere zeigenden.
3. `rename` ersetzt das Ziel als Ganzes; ein dort liegender Symlink wird
   ersetzt, nicht durchschritten.

**Was bleibt:** zwischen Schritt 1 und Schritt 3 liegt ein Zeitfenster. Wer im
Zielverzeichnis schreiben darf, kann in diesem Fenster einen Symlink anlegen —
`rename` überschreibt dann diesen Link, nicht sein Ziel. Geschrieben wird also
nie durch den Link hindurch; was ein Angreifer erreichen kann, ist das
Verschwinden seines eigenen Links. Der frühere Zustand (`fs::write` ohne jede
Prüfung, mit TOCTOU zwischen `exists()` und `write()`) schrieb dagegen
tatsächlich in die Zieldatei des Links.

Wenn `libc` einmal ohnehin im Graphen liegt, gehört hier `O_NOFOLLOW` hin.

### `lopdf` steht auf 0.42 — RUSTSEC-2026-0187 ist behoben

Hier stand bis einschließlich dieser Fassung ein Abschnitt „Warum `lopdf` noch
auf 0.34 steht“ samt einer Liste dessen, was für den Sprung zu ändern wäre.
**Der Sprung ist gemacht.** `Cargo.toml` und `Cargo.lock` führen `lopdf 0.42.0`;
darin ist RUSTSEC-2026-0187 behoben. Die Ausnahme in `deny.toml` ist damit
gegenstandslos und entfernt — `[advisories] ignore = []`.

Was der Umstieg gekostet hat, steht heute im Code:

* `crates/redact-pdf/src/audit_bytes.rs` — Filternamen kommen als Rohbytes
  (`Vec<&[u8]>`) statt als `Vec<String>`; `Dictionary::type_is` ist entfallen.
* `crates/redact-pdf/src/document.rs` — `/Prev` bleibt im geladenen Trailer
  nicht mehr stehen, was `has_incremental_history` betrifft.

**Die Vorprüfung bleibt trotzdem.** Sie ist nicht mehr die Notbremse gegen eine
offene Schwachstelle, sondern das, was sie ohnehin sein sollte: eine Grenze für
das, was diese Anwendung an Aufwand zu treiben bereit ist. `lopdf` schützt sich
gegen unbegrenzte Rekursion; gegen eine Dekompressionsbombe, gegen ein
Aufwandskonto sprengende Form-XObject-Fächerung und gegen eine Million Glyphen
auf einer Seite schützt es nicht, und das ist auch nicht seine Aufgabe.

Eine Anmerkung, die den Sprung überdauert: `lopdf` enthält **kein** `unsafe`
und stürzte trotzdem ab. „Sicheres Rust“ schützt vor Speicherfehlern, nicht vor
unbegrenzter Rekursion und nicht vor unbegrenztem Speicherverbrauch.

---

## „0 Schwärzungen" ist kein Freibrief

`0 Schwärzungen` ist kein Freibrief, sondern ein Befund, der Prüfung verlangt.

Die Zeile kann heißen: in dieser Datei steht nichts Schützenswertes. Sie kann
genauso heißen: der Text steckt in einem Rasterbild, die Schrift benutzt eine
Kodierung, die der Extraktor nicht auflösen konnte, das Muster passt auf diese
Schreibweise nicht, oder die Negativliste hat alles blockiert. Wer die Zahl
nicht gegen eine Sichtprüfung hält, verlässt sich auf eine Heuristik, die
gerade nichts gefunden hat — das ist nicht dasselbe wie „da ist nichts".

Für alles, was das Haus verlässt, gilt der Review-Workflow
(`--review`, prüfen, `--apply-review`).

---

## Einen Fund melden

Bitte **nicht** als öffentliches Issue, solange die Lücke nicht behoben ist —
schon gar nicht mit Beispieldatei.

Weg: im Repository unter **Security → Report a vulnerability** eine private
Advisory anlegen (<https://github.com/thoscut/redactrs/security/advisories/new>).

Hilfreich sind:

* eine möglichst kleine Datei oder Kommandozeile, die den Fehler auslöst,
* Version (`redact-rs --version`), Betriebssystem, Bauart (mit oder ohne
  Oberfläche),
* was passiert und was stattdessen passieren sollte.

Besonders willkommen sind zwei Sorten Fund: **eine Datei, die den Prozess
abstürzen lässt oder den Rechner belegt**, und **eine geschwärzte Ausgabe, in
der der geschwärzte Text noch zu finden ist**. Für die zweite Sorte reicht oft
schon `redact_pdf::leaks` bzw. `strings`.

Nachtrag: `strings` ist dafür nur die schnelle Vorstufe und darf nicht als
Entwarnung gelesen werden — ein Flate-komprimierter Objektstrom (`/ObjStm`) ist
für eine reine Rohbyte-Suche unsichtbar, ebenso eine Zeichenkette in UTF-16BE
oder als Hex-String. `pdftotext` taugt erst recht nicht als Nachweis; die
Messung dazu steht im README unter „Prüfen, ob die Schwärzung gewirkt hat“.
Verbindlich ist `redact_pdf::leaks`.

Es gibt keine Prämie und keine zugesicherte Frist — dies ist ein kleines
Projekt. Eingehende Meldungen werden aber beantwortet.
