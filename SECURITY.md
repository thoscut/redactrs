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
* **`#![forbid(unsafe_code)]` in sieben der acht eigenen Crates**
  (`redact-core`, `redact-pdf`, `redact-patterns`, `redact-booking`,
  `redact-pipeline`, `redact-render`, `redact-gui`). Der achte, `redact-cli`,
  steht unter `#![deny(unsafe_code)]` — mit **einer** benannten und begründeten
  Ausnahme, siehe [Kein Kernabzug](#kein-kernabzug-dieses-prozesses). Der
  Compiler setzt beides durch; es ist keine Absichtserklärung.

  **Bis 0.6.0 standen alle acht unter `forbid`.** Wer diese Zusage aus einer
  älteren Fassung kennt, liest hier die Änderung und nicht den alten Satz.

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
  crates/redact-gui/src/main.rs
  ```

  Acht Dateien — sieben Crates plus das zweite Binärziel von `redact-gui`.
  `redact-cli` fehlt in dieser Liste, und genau deshalb steht daneben die
  zweite Frage, die die Ausnahme **beziffert** statt sie zu behaupten:

  ```bash
  $ grep -rl '#\[allow(unsafe_code)\]' crates/*/src --include='*.rs'
  crates/redact-cli/src/dumpable.rs
  $ grep -c '#\[allow(unsafe_code)\]' crates/redact-cli/src/dumpable.rs
  3
  ```

  **Eine** Datei, **drei** Fundstellen, und die Zahl gehört genau so
  hingeschrieben: der `prctl`-Aufruf für Linux, der `setrlimit`-Aufruf für die
  übrigen Unix-Systeme (im Bau eines Binaries entsteht immer nur einer von
  beiden), und die Gegenprobe im `#[cfg(test)]`-Modul, die zurückliest, ob der
  Kernel den Zustand wirklich übernommen hat. Jede trägt ein einzelnes
  `#[allow(unsafe_code)]` an der Stelle — nicht an der Datei und nicht am
  Crate; gezählt wird deshalb genau dieses Attribut. (Ein `grep` nach dem
  Schlüsselwort `unsafe` zählte auch die Erklärung im Modulkommentar mit und
  lieferte eine Zahl zu viel.) Wächst diese Ausgabe, ist die Zusage verletzt,
  und das ist dann an einer Zahl abzulesen und nicht an einer Meinung.
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

### Kein Kernabzug dieses Prozesses

Alles oben schützt das Passwort auf dem Weg *aus* dem Programm heraus. Es
schützt nicht davor, dass der **Kernel den ganzen Arbeitsspeicher wegschreibt**,
wenn der Prozess abstürzt — und darin steht dann der Klartext des Dokuments
und, bei einer verschlüsselten Datei, das Passwort. Ein Kernabzug liegt dort,
wohin `kernel.core_pattern` zeigt: unter systemd im Journal, sonst im
Arbeitsverzeichnis. Er ist länger lesbar, als der Prozess lief.

Seit dieser Fassung schaltet `redact-rs` das als **allererste** Anweisung in
`main` ab (`prctl(PR_SET_DUMPABLE, 0)`, `crates/redact-cli/src/dumpable.rs`);
scheitert der Aufruf, sagt der Lauf es auf stderr, statt den Schutz nur
anzunehmen. Die Begründung samt Messreihe steht im Modulkommentar dort.

Nachgemessen am **gebauten Binary** (`core_pattern = core`,
`ulimit -c unlimited`, derselbe Shell-Aufruf für beide Zeilen; der Prozess
wird mit `SIGABRT` beendet):

| Prozess | Abzugsdatei |
|---|---|
| `sleep` (Kontrolle — die Umgebung schreibt also wirklich Abzüge) | `core`, 454 656 Byte |
| `redact-rs … --check-leaks -` | **keine** |

Die Shell meldet dabei nur noch `Aborted` statt `Aborted (core dumped)`, und
der Rückgabewert bleibt 134 — der Prozess wurde also wirklich von `SIGABRT`
getötet und ist nicht etwa vorher sauber ausgestiegen.

**Was das nicht abdeckt — bitte genau lesen:**

* **Windows: nichts umgesetzt.** Ein Mittel gäbe es —
  `WerAddExcludedApplication` nimmt den Prozess aus der
  Windows-Fehlerberichterstattung (WER) und damit aus deren Abbildern —, aber
  dieses Programm ruft es nicht auf. Ein Abbild, das ein *anderer* Prozess
  zieht (`MiniDumpWriteDump`, ein Debugger), bliebe davon ohnehin unberührt.
  Das ist ausgerechnet die Plattform der Zielgruppe; die Zusage lautet dort
  schlicht: keine.
* **macOS, BSD und die übrigen Unix-Systeme: das schwächere Mittel.** Dort
  gibt es kein `prctl`; die Funktion setzt `setrlimit(RLIMIT_CORE, 0)` — eine
  Grenze, kein Verbot. Wer den Prozess mit angehobener Grenze startet, ändert
  daran nichts (sie wird hier gesetzt, nicht geerbt), aber ein `core_pattern`,
  das an ein Programm weiterreicht, kann sie je nach System übergehen.

  | System | Mittel | Wie fest |
  |---|---|---|
  | Linux | `prctl(PR_SET_DUMPABLE, 0)` | Der Kernel schreibt gar nichts, `root` eingeschlossen. |
  | macOS, BSD, übrige Unix | `setrlimit(RLIMIT_CORE, 0)` | Schwächer: eine Grenze, kein Verbot. |
  | Windows | **nicht umgesetzt** | `WerAddExcludedApplication` wird nicht aufgerufen; ein Abbild aus einem fremden Prozess bliebe ohnehin. |

  Die Funktion liefert drei Antworten statt `true`/`false`: „abgeschaltet“,
  „hier ist nichts abgeschaltet worden“ (Windows: nicht umgesetzt; oder ein
  Kern, der die Option ablehnt)
  und „das Mittel gibt es, der Aufruf schlug fehl“ — nur die letzte ist eine
  Warnung wert (`crates/redact-cli/src/dumpable.rs`).
* **Ein Abzug, den ein *anderes* Programm zieht**, etwa ein Debugger mit
  `root`-Rechten.

Und der **Nebeneffekt — er gehört zu `prctl`, also zu Linux**: dort ist der
Prozess danach auch für `ptrace` durch denselben Benutzer unerreichbar, und
`/proc/<pid>/` gehört `root`. Für ein Werkzeug, das Kontoauszüge im Speicher
hält, ist das die richtige Richtung — ein anderes Programm desselben Benutzers
kann den Klartext nicht mehr mitlesen. Wer dort mit `gdb` oder `strace` an
einem Fehler arbeitet, braucht `root` oder einen eigenen Bau ohne diese Zeile.
**Auf macOS, BSD und den übrigen Unix-Systemen gibt es diesen Nebeneffekt
nicht**: `setrlimit(RLIMIT_CORE, 0)` begrenzt den Abzug und sonst nichts — ein
Debugger desselben Benutzers kommt weiterhin an den Prozess. Unter Windows ist
ohnehin nichts umgesetzt (siehe Tabelle darüber).

### Die Grenzen gelten auch hinter der Entschlüsselung

Sie greifen nur **zweistufig**, weil die Vorprüfung `prescan` über Rohbytes
läuft und an einer verschlüsselten Datei nur die Hälfte sehen kann:

| | verschlüsselt? | auf den Rohbytes messbar |
|---|---|---|
| Objektstruktur (`<<`, `[`, Namen, Zahlen) | nein | ja |
| Zeichenketten und **Streams** | ja | nein — es ist Rauschen |

* **Vor** der Entschlüsselung läuft `prescan` wie immer. Was dort zu sehen ist,
  ist geprüft: eine Datei mit 200 000 offenen `[` in einem gewöhnlichen Objekt
  fällt schon hier durch, verschlüsselt oder nicht. Dasselbe gilt seit der
  Buchung des Rumpfs für eine Dictionary-Bombe: die Objektstruktur wird nicht
  verschlüsselt, das Parse-Budget greift also schon im ersten Durchgang.
  Nachgemessen an einer 44-MB-Dictionary-Bombe mit `/Encrypt` im Trailer — sie
  wird mit **und** ohne Passwort mit derselben Budgetmeldung abgelehnt.
* **Nach** der Entschlüsselung läuft dieselbe Prüfung ein zweites Mal, jetzt auf
  dem entschlüsselten Dokument (`redact_pipeline::check_limits_after_decryption`).
  Dazu wird das Dokument serialisiert — die Streams stehen darin als das, was
  sie sind: komprimiert, aber nicht mehr verschlüsselt. Es misst also
  derselbe Code mit denselben Grenzen und denselben Meldungen wie bei einer
  unverschlüsselten Datei.

Erst *nach* dem Laden zu prüfen ist nur deshalb vertretbar, weil das Laden
selbst billig ist: `lopdf` legt Streams als Rohbytes ab und packt sie nicht aus.
Der teure Teil — die Zerlegung des Seiteninhalts in Operationen, je nach Form
des Stroms 62 bis rund 100 Byte Arbeitsspeicher je Byte Stream **für den
`Operation`-Vektor allein** (siehe „Dekompressionsbomben“) — kommt erst danach.
Der Spitzenbedarf eines Laufs liegt darüber; für die einzelne Textseite deutlich,
siehe „Was die Zahl 62–100 nicht ist“.

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

**MB heißt hier wie überall in diesem Projekt 1024² Byte** — 16 MB sind
16 777 216 Byte, so rechnet es auch der Abschnitt „Wo gewöhnlicher Text an
die Decke stößt“ vor. Wo eine Messung weiter unten MiB schreibt, ist dieselbe
Einheit gemeint, nur ausdrücklich; die Schalter (`--max-input-mb` und die
übrigen) nehmen dieselben Vielfachen von 1024².

| Grenze | Vorgabe | Stellschraube |
|--------|---------|---------------|
| **Größe der Eingabedatei** | **512 MB** | **`--max-input-mb`** |
| Verschachtelungstiefe (`[`, `<<`) | 100 | fest |
| dito, in binär aussehender Nutzlast | 256 | fest |
| entpackte Bytes über **alle** Streams | 1024 MB | `--max-decompressed-mb` |
| davon: alles, woraus PDF-**Syntax** wird — geparste Streams **und** der Rumpf der Datei | 16 MB | `--max-parsed-mb` |
| dito, in der zweiten Einheit: **gerechneter Objektspeicher** | ein Vielfaches des Byte-Budgets | `--max-parsed-mb` |
| Trefferkandidaten je Datei | 100 000 | `--max-candidates` |
| Bildpunkte **je Bild** (Dekodieren) | 40 000 000 | fest |
| gleichzeitig gehaltene **dekodierte** Bildbytes | 256 MB | `--max-image-mb` |
| **Zeichen, die eine Seite setzen darf** | **1 000 000** | fest |
| **Zeichenoperationen je Seiten-Scan** (Aufwandskonto) | **1 000 000 + 16× Inhalt** | fest |
| Größe der Einstellungsdatei | 1 MB | fest |
| **Buchungsliste** (`--booking-list`) | **16 MB** | **fest** |
| **Review-Datei** (`--apply-review`) | **16 MB** | **fest** |
| **Regionsliste** (`--manual-regions`) | **16 MB** | **fest** |
| **Musterkonfiguration** (`--patterns-config`) | **1 MB** | **fest** |
| **Suchbegriffe je `--check-leaks`-Lauf** | **1 000** | **fest** |
| **Budget der Nachprüfung** — Summe der entpackten Bytes je Sicht von `--check-leaks`; ein Strom, der sie sprengte, wird nicht entpackt: `NICHT GEPRÜFT`, Rückgabewert 3 | **1024 MB** | **`--max-decompressed-mb`** (dieselbe Zahl) |

Alle Grenzen, die dem **Eingabe-PDF** gelten, gelten für verschlüsselte Dateien
genauso — siehe „Die Grenzen gelten auch hinter der Entschlüsselung“.

Die Zeilen „Zeichen, die eine Seite setzen darf“ und „Zeichenoperationen je
Seiten-Scan“ messen keine Bytes, und das ist ihr Zweck; sie stehen weiter unten
unter „Wenn Bytes die falsche Größe sind“.

**Was die Vorprüfung auspackt.** Auspacken muss sie, was zu PDF-Syntax werden
kann; das sind `FlateDecode`, `LZWDecode`, `ASCII85Decode` — und seit der
Spur-A-Runde 1 (Register #64) auch `ASCIIHexDecode` und `RunLengthDecode`,
jedes Glied begrenzt auf das Entpackbudget. Eine Grenze der **Rohgröße** gibt
es dabei nicht mehr: bis zur Spur-A-Runde 1 lehnte die Vorprüfung jede Kette
mit `LZWDecode` oder `ASCII85Decode` über 16 MB ab — nachgemessen an einem
17-MB-`ASCII85Decode`-Strom: `Stream mit Altlast-Filter (ASCII85Decode) ist mit
17 825 795 Bytes zu groß (Grenze 16 777 216 Bytes)`, Rückgabewert 1. Die Grenze
stammte aus der Zeit, als `lopdf` diese Filter ohne Grenze auspackte; seit #64
entpackt die Vorprüfung sie selbst, und die Grenze lehnte nur noch ein großes
ASCII85-Bild ab, wie es Distiller mit ASCII-Ausgabe schreibt (Register #82).
Die Bildfilter packt die Vorprüfung **gar nicht** aus, sie zählt ihre Rohbytes.
Ein 17-MB-`ASCIIHexDecode`-Strom lief vor der Runde roh durch (Rückgabewert 0,
nachgemessen) und wird jetzt entpackt gezählt. Was **vor** einem Bildfilter
oder einem unbekannten Filter steht — der Flate-Vorspann von `[/FlateDecode
/DCTDecode]` —, packt sie seit Register #83 aus und zählt es gegen das ganze
Budget; bis dahin buchte sie die ganze Kette roh, und der Bilddekoder des
Schreibpfads entpackte den Vorspann ohne Grenze. Ausgepackt werden die Bildfilter erst beim Schwärzen und in der
Nachprüfung — und dort gegen `--max-decompressed-mb`
(nachgemessen: ein `RunLengthDecode`- und ein `ASCIIHexDecode`-Seiteninhalt mit
demselben Geheimnis werden von `--check-leaks` gefunden, letzterer ausdrücklich
als `<Stream, dekodiert: ASCIIHexDecode>`).

**Der Spitzenspeicher hängt am größten Einzelstrom, nicht am Budget.** „1024 MB“
ist die *Summe* der entpackten Bytes über alle Ströme, nicht der Bedarf. Wer
den Bedarf schätzen will, sieht auf den **größten einzelnen Strom**: gemessen
liegt die Spitze bei rund dem **Doppelten** seiner entpackten Größe (die
entpackten Bytes liegen mehr als einmal gleichzeitig im Speicher).
Nachgemessen an einer 1 020 KiB großen Datei mit einem einzigen
Flate-Strom über 1 GiB Nullen, **mit den Vorgabewerten** (`--max-decompressed-mb
1024`): `redact-rs bomb.pdf --check-leaks XX` endet mit Rückgabewert **0** — der
Strom passt ja ins Budget — und mit einem `VmHWM` von 2 172 628 kB, also
**2 122 MB** ≈ 2,1 GiB (18 s; gemessen über
`getrusage(RUSAGE_CHILDREN).ru_maxrss`, dieselbe Zahl, die
`/proc/<pid>/status` als `VmHWM` führt; `kB` heißt dort 1024 Byte, und die
MB-Angaben hier sind daraus durch 1024 geteilt, nicht durch 1000). Das
Schwärzen derselben Datei erreicht dieselbe Spitze (2 172 312 kB = 2 121 MB) —
die Nachprüfung ist hier nicht sparsamer als der Hauptweg. Derselbe Strom als
`/ObjStm` verpackt kommt mit den Vorgabewerten gar nicht durch (Rückgabewert 1,
`VmHWM` 1 056 688 kB = 1 032 MB); wer ihm mit `--max-decompressed-mb 4096` Luft
gibt, misst 3 220 164 kB = **3 145 MB** ≈ 3,1 GiB — also rund das
**Dreifache**, weil ein Objektstrom zusätzlich geparst wird. Wer den Bedarf drücken will, senkt
`--max-decompressed-mb`: mit 16 MB bleibt derselbe Lauf bei 24 MB (Rückgabewert
1, die Vorprüfung lehnt die Datei ab).

**Und `--max-decompressed-mb` deckelt, was *entpackt* wird, nicht die Größe des
Prozesses.** Zwei gemessene Stellen (Release, `VmHWM` des Kindprozesses,
64-MiB-Strom, Budget 512 MiB): Das Orakel klonte die Rohbytes eines Stroms,
bevor es den ersten Filter überhaupt kannte, und warf den Klon bei einem
unbekannten Filter wieder weg — `/DCTDecode` kostete dadurch 205 MB, seit der
Fix-Runde 6 sind es 138 MB, so viel wie ohne `/Filter`. Und ein Strom mit
`/FlateDecode`, dessen Bytes sich als roher Deflate-Strom aufblasen lassen,
kommt auf 621 MB: der Rückfall auf rohes Deflate ist die Nachbildung von
`lopdf`, und das Budget deckelt das Entpackte, nicht den Prozess.

**Diese drei Zahlen zählen MB als 10⁶ Byte** — so gibt sie die Messung in
`crates/redact-pdf/tests/zf_q2_teildekoder.rs` aus, anders als der Rest dieses
Dokuments; dass 205 − 138 = 67 genau der 64-MiB-Strom ist, verrät die Einheit.
Gemessen ist außerdem der **Testprozess**, also `leaks_many_within` allein. Am
gebauten Binary liegt die Spitze über derselben Datei bei 325 388 kB = 318 MB
(1024²) — mit und ohne `/DCTDecode` gleich —, weil es die Datei zusätzlich lädt
und durch die Schwärzung schickt.

### Was `--max-parsed-mb` zählt — zwei Klassen, nicht eine

Das Parse-Budget verbucht **beides**, was `lopdf` zu `Object`-Werten macht:

1. die **Streams, die geparst werden** — Seiteninhalt und Objekt-Streams. Ob ein
   Stream hierher zählt, entscheidet sein *ausgepackter Inhalt*, nicht sein
   Dictionary: sieht er wie PDF-Syntax aus statt wie Nutzlast, gilt das enge
   Budget.
2. den **Rumpf der Datei selbst** — Objektköpfe, Dictionaries, Arrays,
   Querverweistabelle, Trailer; alles, was nicht Stream-Nutzlast ist.

**Auch ein Bild kann unter Klasse 1 fallen.** Hier stand einmal „ein Bild bleibt
Nutzlast und zählt nur gegen `--max-decompressed-mb`“ — das widerspricht dem
Satz zwei Zeilen darüber, denn es entscheidet der *ausgepackte Inhalt*, und ein
Bild bringt seinen selbst mit. Nachgemessen an vier einseitigen PDFs mit
demselben FlateDecode-Graustufenbild (4472 × 4472 Bildpunkte, 19 998 784 Byte
entpackt), die sich nur im Wertebereich der Bildpunkte unterscheiden:

| Bildpunkte | Datei | Ergebnis mit den Vorgaben |
|---|---:|---|
| **0x30–0x70** (dunkler Scan) | 103 199 Byte | **Exit 1** — am *Parse*-Budget, nicht am Dekompressionsbudget |
| 0x40–0x90 | 108 586 Byte | Exit 0 |
| 0xC0–0xFF (heller Scan) | 82 679 Byte | Exit 0 |
| 0x00–0xFF | 103 604 Byte | Exit 0 |

Dieselbe abgelehnte Datei läuft mit `--max-parsed-mb 64` durch (Exit 0, 32 MiB
Spitzenspeicher).

Der Unterschied ist kein Zufall: 0x30 bis 0x70 liegt vollständig im druckbaren
ASCII-Bereich (0x20–0x7E), die anderen drei Bereiche nicht. Ein dunkler
Graustufen-Scan sieht ausgepackt aus wie PDF-Syntax und wird deshalb wie welche
verbucht. Das ist Absicht — das Dictionary eines Streams gehört dem, der die
Datei baut, und `/Subtype /Image` wäre damit kein Beleg, sondern eine Einladung.
Der Preis dafür steht in der ersten Zeile: ein gewöhnlicher Scan, dessen
Bildpunkte dort landen, wird abgelehnt und braucht `--max-parsed-mb`.

(Die erste Zeile lässt die beiden Klammer-Bytes `0x5B` und `0x5D` aus. Nimmt man
sie hinzu, lehnt der Lauf mit den Vorgaben ebenfalls am Parse-Budget ab, mit
`--max-parsed-mb 64` dann aber an der **Verschachtelungstiefe**: `[` und `]` in
den Bildpunkten liest die Vorprüfung wie einen Stapel offener Arrays. Beide
Ablehnungen sind richtig; für die Frage dieses Abschnitts — welches Budget ein
Bild trifft — ist die Tiefengrenze nur im Weg.)

**Klasse 2 fehlte früher, und das war eine Lücke.** Eine Datei aus lauter
unkomprimierten Dictionaries hat keine Streams — sie sah vom Budget nichts und
lief mit Rückgabewert 0 durch. Dieselbe Datei *komprimiert*, also als
Objekt-Stream, wurde seit jeher abgelehnt. Der Unterschied war allein, ob die
Bombe gepackt war, und das sucht ein Angreifer sich als Erstes aus.

Nachgemessen an dieser Fassung (Release, `/usr/bin/time -f %M`, `--no-patterns`,
eine Datei aus Dictionaries mit je 600 Einträgen, **kein** Stream darin):

| Rumpf der Datei | `--max-parsed-mb` | Ergebnis |
|---|---|---|
| 44,3 MB | 16 (Vorgabe) | Exit 1, 47 MiB — abgelehnt, und zwar wegen des Rumpfs |
| 44,3 MB | 64 (von Hand angehoben) | Exit 0, **rund 2 520 MiB** |

Die zweite Zeile sagt, was diese Datei kostet, wenn man sie durchlässt: **2,5 GB
aus 44 MB.** Vor der Buchung des Rumpfs war das kein Sonderfall mit angehobenem
Budget, sondern der Normalfall — die Datei hat keine Streams, das Budget sah sie
nicht, und der Lauf endete mit Rückgabewert 0.

### Eine Byte-Grenze kann diese Klasse nicht allein decken

Hier stand einmal: „ein voll ausgeschöpftes Vorgabebudget kostet rund 1,3 GB,
der ungünstigste Fall bleibt also der Content-Stream“. Der Satz war falsch, und
er war es aus dem Grund, der in diesem Projekt immer wieder auftaucht — **die
Decke zählte die falsche Einheit**. Ein Byte-Budget unterstellt einen
Aufblähfaktor; den gibt es nicht. Er hängt an der *Form* der Syntax, nicht an
ihrer Länge. Fünf Dateien **gleicher Größe** (je knapp 16,0 MiB reiner Rumpf,
kein Stream), die sich nur darin unterscheiden, was zwischen den Klammern steht:

| Was zwischen den Klammern steht | Datei | Ergebnis mit den Vorgaben |
|---|---:|---|
| `[]` — 8 212 000 leere Arrays | 16 759 951 Byte | **Exit 1** (Objektdecke) |
| `0` — 8 212 000 Zahlen | 16 759 951 Byte | **Exit 1** (Objektdecke) |
| `/a` — 5 511 000 Namen | 16 758 210 Byte | Exit 0, **1 638 MiB** |
| `<<>>` — 4 147 000 leere Dictionaries | 16 757 286 Byte | Exit 0, 985 MiB |
| `<< /abc 0 … >>` — je 600 Einträge | 16 771 547 Byte | Exit 0, 960 MiB |

Gleiche Dateigröße, und trotzdem liegen allein die drei angenommenen Zeilen um
den Faktor 1,7 auseinander — die beiden abgelehnten kämen ohne die zweite Decke
weit darüber hinaus. **Die Datei aus lauter leeren Arrays war der Fall, der das
aufdeckte:** `[]` sind zwei Byte in der Datei und gemessen 632 Byte im
Arbeitsspeicher. Sie blieb damit unter *jedem* Größenbudget und belegte
trotzdem Gigabytes — vor der Objektdecke gemessen **5 832 MiB bei Rückgabewert
0**, also rund 365 Byte je Dateibyte statt der 80, die aus „1 330 MB aus 16 MB“
folgen würden.

Deshalb hängt an `--max-parsed-mb` heute **eine zweite Decke in einer zweiten
Einheit**: neben den Dateibytes wird der *gerechnete* Speicher der Objekte
gedeckelt, die daraus entstehen (`OBJEKT_BYTES`, `ARRAY_BYTES` und
`OBJEKTSPEICHER_JE_BUDGETBYTE` in `crates/redact-pdf/src/document.rs`). Beide
Decken bewegen sich mit demselben Schalter; eine zweite Schraube wäre eine
zweite Stelle, an der die Zahlen auseinanderlaufen können. Die gerechnete Zahl
ist dabei *nicht* der Spitzenspeicher — die Zeile mit den Namen zeigt es: sie
kommt gerechnet unter der Decke durch und belegt gemessen 1 638 MiB.

**Gewöhnliche Dateien merken davon nichts** — aber nicht so weit weg, wie hier
einmal stand. Die Tabelle nannte früher nur den *Rumpf* und kam damit auf 0,55 %
und 2,90 %, obwohl der Abschnitt darüber zwei Klassen desselben Budgets
eingeführt hatte: die geparsten Streams fehlten in der Rechnung. Gemessen wird
deshalb, was das Budget wirklich sieht — das **kleinste `--max-parsed-mb`, mit
dem die Datei noch durchläuft** (Bisektion, `--no-patterns`):

| Datei | Größe | kleinstes `--max-parsed-mb` | Anteil am 16-MB-Budget |
|---|---:|---:|---:|
| Beispieldatei aus `--write-demo`, 2 Seiten | 1 862 Byte | 1 | ≤ 6 % |
| 10 Seiten Text (`--example gen10`) | 59 824 Byte | 1 | ≤ 6 % |
| 200 Seiten A4-Scan, 300 dpi, JPEG je Seite | 55 106 045 Byte | 1 | ≤ 6 % |
| 500 Seiten Text, unkomprimiert | 2 998 855 Byte | 3 | 19 % |
| 500 Seiten Text, geschwärzt (also komprimiert) | 535 588 Byte | 5 | 31 % |
| 2 000 Seiten Text, unkomprimiert | 12 058 857 Byte | 12 | 75 % |
| 2 000 Seiten Text, geschwärzt | 1 575 711 Byte | 14 | **88 %** |

Der Schalter nimmt nur ganze Megabyte; „1“ heißt also „1 oder weniger, feiner
lässt sich mit ihm nicht messen“. **Alle** diese Dateien laufen mit den Vorgaben
durch (Rückgabewert 0, nachgemessen).

Zwei Zeilen sind lehrreich. Der **Scan**: 55 MB Datei und trotzdem 1 MB Budget —
seine Bilder sind DCT-Nutzlast und zählen nur gegen `--max-decompressed-mb`. Und
die **geschwärzten** Textdateien brauchen *mehr* Budget als die unkomprimierten
Originale, aus denen sie entstanden, obwohl sie ein Fünftel bis ein Achtel deren
Größe haben: das Budget sieht den ausgepackten Inhalt, und die Schwärzung legt
Deck-Rechtecke obendrauf. Wer vom Dateigewicht auf das Budget schließt, schließt
in beide Richtungen falsch.

Ein geschwärztes 2 000-Seiten-Dokument liegt damit bei 88 % des Vorgabebudgets —
das ist gewöhnlich, aber es ist keine Reserve mehr. Wo dieser Text an die Decke
stößt, steht im nächsten Abschnitt. Die Gegenprobe im Testlauf hält
`crates/redact-pdf/tests/rumpf_im_parse_budget.rs` fest
(`gewoehnliche_dokumente_gehen_mit_der_vorgabe_durch`).

### Wo gewöhnlicher Text an die Decke stößt

Damit gibt es eine Verfügbarkeitsschwelle, die vor der Buchung des Rumpfs keine
war, und sie gehört genannt: **eine Textdatei, deren Rumpf und Seiteninhalte
zusammen 16 MiB überschreiten, wird abgelehnt** — auch wenn nichts daran böse
gemeint ist. Bei unkomprimierten Seiteninhalten ist das praktisch die
Dateigröße, denn dann zählt jedes Byte in eine der beiden Klassen.

Nachgemessen an Dateien in der Bauart von `--example gen10` (45 Textzeilen je
Seite, unkomprimierte Inhaltsströme), einmal mit `/MediaBox` und `/Resources` je
Seite und einmal von `/Pages` geerbt:

| Aufbau | Seiten | Datei | Ergebnis mit den Vorgaben |
|---|---:|---:|---|
| `/MediaBox` und `/Resources` je Seite | 2 779 | 16 775 702 Byte | Exit 0 |
| dito | **2 780** | 16 781 757 Byte | **Exit 1** |
| beides von `/Pages` geerbt | 2 796 | 16 772 427 Byte | Exit 0 |
| dito | **2 797** | 16 778 444 Byte | **Exit 1** |

16 MiB sind 16 777 216 Byte — in beiden Zeilenpaaren liegt die Grenze genau
dazwischen.

**Woran die Zahl wirklich hängt.** Nicht an den Seiten — an den Bytes. Dieselben
Seiten mit geerbtem `/MediaBox` und `/Resources` sparen 46 Byte je Seitendict,
und schon passen mehr Seiten in dieselbe Decke. Wer die Schwelle als
Seitenzahl liest, liest sie falsch; maßgeblich ist, was die Datei an Syntax
mitbringt.

**Der Ausweg steht auf der Kommandozeile:** `--max-parsed-mb` höher setzen. Er
hebt beide Decken zugleich, und er wirkt linear — wer ihn verdoppelt, lässt
doppelt so viel Syntax und doppelt so viel gerechneten Objektspeicher zu.

```console
$ redact-rs 2780seiten.pdf -o out.pdf --max-parsed-mb 32 --no-patterns
$ echo $?
0
```

Gemessen: Exit 0 bei 583 MiB Spitzenspeicher. Wer den Schalter anhebt, sollte
wissen, wofür — die Tabellen oben sagen, was eine Datei dieser Größe im
ungünstigen Fall kosten kann.

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

**Und nach der Größe der Wertebereich — für die Seitennummer.** Von den
Feldwerten aus Review- und Regionsdateien wird genau einer dem Wertebereich
nach geprüft: die Seitennummer, höchstens `u32::MAX` (so nummeriert `lopdf`
Seiten; `redact_core::model::MAX_PAGE_INDEX`); alles darüber beendet den Lauf
mit Rückgabewert 1; eine Ausgabedatei entsteht nicht. Gemessen im Test
(`hostile_field_values_in_a_valid_review_file_end_with_a_message_not_a_panic`,
Dev-Profil, `--apply-review` und `--manual-regions`) und im Release-Bau von
Hand: kein Rückgabewert 101, keine Ausgabedatei. Die übrigen Feldwerte —
Koordinaten, Texte — bekommen hier keine Bereichsprüfung; unbrauchbare
Koordinaten (NaN, Unendlich, Überlauf) fängt die Rechteckbildung dort, wo die
Zahl neu entsteht (`Rect::is_usable`). Jede 1-basierte Seitenanzeige rechnet
zusätzlich sättigend — ein zweiter Zaun hinter dem ersten.

Die Vorprüfung (`redact_pdf::document::prescan`) läuft über die **Rohbytes** der
Datei, bevor `lopdf` sie zu sehen bekommt, und schließt die ausgepackten Streams
und den Rumpf mit ein — siehe „Was `--max-parsed-mb` zählt“. Sie muss davor
laufen: der Stapelüberlauf beendet den Prozess, bevor irgendein Fehlerwert
entstehen könnte. Die Buchung des Rumpfs belegt dabei selbst keinen Speicher —
sie zählt Bytes, und die Ablehnung kommt vor `Document::load_mem`, also vor der
einzigen Stelle, an der aus diesen Bytes wirklich Speicher wird.

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
`--max-decompressed-mb` und `--max-parsed-mb` verbuchen die *entpackten
Streambytes* — das, was der PDF-Filter ausgibt (dazu bei `--max-parsed-mb` der
Rumpf der Datei). **Nicht** die Rohbytes: eine 82-kB-Datei mit einem
`/FlateDecode`-Bild von 20 MB entpackter Nutzlast fällt an
`--max-decompressed-mb 8` durch, obwohl sie 82 kB groß ist (nachgemessen,
Exit 1). Ein Bild, das geschwärzt wird, muss aber nach RGBA8 ausgepackt werden —
4 Byte je Bildpunkt. Bei einem gewöhnlichen Schwarzweiß-Scan (`/DeviceGray`,
`/BitsPerComponent 1`) liegt zwischen beidem der **Faktor 32**; die
Streambyte-Grenzen greifen dort also nicht. Siehe „Speicherbedarf der
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

#### Rückgabewert 3 hat **drei** Bedeutungen

Das ist die Stelle, an der ein Skript falsch gebaut wird. Wer den Absatz
darüber allein liest, hält 3 für „verarbeitet, aber nicht vollständig geprüft“
— und deutet dann einen **Leckfund** als eine bloß unvollständige Prüfung. Der
`--help`-Text des Binaries nennt alle drei Fälle; hier standen bis zur
Fix-Runde 5 nur zwei, und der dritte fehlte auch dort, wo er am meisten wehtut:
`--check-leaks` endet **auch ohne Fund** mit 3, wenn eine Stelle ungeprüft
blieb.

```console
$ redact-rs --help | sed -n '/^  3  /,$p'
  3  Der Lauf ist gelungen, das Ergebnis ist es nicht — sieh hin.
     Drei Fälle:
     • Verarbeitet, aber nicht vollständig geprüft. …
     • --check-leaks hat mindestens einen Begriff in der Datei gefunden.
       Ein Fund ist kein Verarbeitungsfehler (das wäre 1) und kein
       Bedienfehler (das wäre 2): die Suche lief vollständig, die Antwort
       lautet „ja, es steht noch drin“.
     • --check-leaks konnte eine Stelle NICHT PRÜFEN — und das auch ohne
       einen einzigen Fund. Ein Strom, der das Restbudget von
       --max-decompressed-mb sprengte, wurde nicht entpackt; über ihn sagt
       „nicht gefunden“ nichts. …
```

Alle drei Fälle heißen „sieh hin“, und keiner darf als Erfolg durchgehen.
Wer sie im Skript **unterscheiden** muss, unterscheidet sie am Aufruf und nicht
am Rückgabewert: `--check-leaks` schwärzt nicht und schließt jeden
Schwärzungsschalter aus, ein schwärzender Lauf prüft keine Begriffe. Ein
Aufruf, eine Frage, eine Antwort. Zum Schalter selbst siehe
[`--check-leaks`: die Nachprüfung ohne Quelltext](#--check-leaks-die-nachprüfung-ohne-quelltext).

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
patterns, disabled_patterns, min_confidence, padding, theme. (Der Inhalt der
Datei wird hier nicht wiedergegeben — REDACT_RS_CONFIG und die Vorgabestelle
können auf eine beliebige fremde Datei zeigen.)
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

**Speicher ja, Zeit nein.** Die Speicherzahlen sind reproduzierbar: eine
Nachmessung auf derselben Maschine trifft sie aufs Megabyte. Die **Laufzeiten
sind es nicht** — sie hängen an Last, Übersetzer und Maschine und schwanken
gemessen um ein Fünftel nach unten (13,3 s wurden bei der Nachprüfung zu 11,0 s,
2,9 s zu 2,4 s), ohne dass sich am Speicher etwas geändert hätte. Sie stehen
hier als Größenordnung — „Sekunden, nicht Minuten“ —, nicht als Zusicherung.
Wer eine dieser Sekundenzahlen nicht reproduziert, hat deshalb noch keinen
Befund; wer eine Speicherzahl nicht reproduziert, sehr wohl.

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
gespeichert) und ein sehr viel engeres für alles, woraus PDF-Syntax wird: die
geparsten Streams **und** den Rumpf der Datei (siehe „Was `--max-parsed-mb`
zählt“).

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
im ungünstigsten hier gemessenen Fall.**

**Was hier einmal falsch stand.** „Bei der Vorgabe folgt daraus eine Obergrenze
von etwa 1,6 GB, und die zweite Klasse desselben Budgets — der Rumpf — bleibt
darunter; maßgeblich bleibt also der Content-Stream.“ Beides war falsch. Der
Rumpf bleibt nicht darunter: eine Datei aus lauter leeren Arrays kam bei
Rückgabewert 0 auf 5 832 MiB (siehe „Eine Byte-Grenze kann diese Klasse nicht
allein decken“), also auf mehr als das Dreifache. Und „Obergrenze“ war das
falsche Wort für eine Zahl, die aus einem unterstellten Faktor folgt — die
Faktoren in dieser Tabelle stehen für die Formen, die *hier* gemessen wurden,
und nicht für die ungünstigste, die es gibt. Eine Obergrenze gibt heute die
zweite Decke in der zweiten Einheit, der gerechnete Objektspeicher; die
Byte-Zahlen dieses Abschnitts sind Messwerte, keine Zusicherung.

Wer das gegen eine Maschine mit wenig Arbeitsspeicher absichern will, setzt
`--max-parsed-mb` herunter; der Wert wirkt linear und bewegt beide Decken.

**Und die Verstärkungstabellen dieses Abschnitts sind älter als die
Objektdecke.** Die Zeilen ab 16 MB Strom lassen sich mit den Vorgaben heute gar
nicht mehr erreichen — sie wurden an einer Fassung gemessen, die nur Bytes
zählte. Nachgemessen an der heutigen, `0 0 0 rg` unkomprimiert und
`--no-patterns`:

| Strom | Ergebnis mit den Vorgaben |
|---|---|
| 8 MB | Exit 0 |
| 16 MB | **Exit 1** (Objektdecke) |
| 15,0 MB (15 729 051 Byte Datei) | **Exit 1** (Objektdecke); mit `--max-parsed-mb 64`: Exit 0, 1 123 MiB |

Die Richtung des Fehlers in den alten Tabellen ist damit die harmlose: sie
versprechen mehr Durchlass, als der Code heute gibt. Die Faktoren selbst
(Byte je Byte) gelten unverändert für das, was durchkommt.

**Und die Nachprüfung war bis 0.6.0 die offene Flanke desselben Bildes.**
`--check-leaks` und die Nachprüfung der Oberfläche packten jeden Strom aus, den
sie fanden; die Grenze `--max-decompressed-mb` galt nur dem Schwärzen. Seit
dieser Fassung gilt sie beiden, und zwar **beim Entpacken** (der Leser bekommt
das Restbudget, es wird nicht hinterher gemessen), samt einer Vorprüfung mit
derselben Zahl, damit `lopdf` keinen Objektstrom unbegrenzt auspackt.
Nachgemessen an 1 GiB Nullen (1 044 089 Byte in der Datei als Seiteninhalt,
1 044 192 Byte als `/ObjStm`), je einmal mit `--max-decompressed-mb 16` und
einmal mit `--max-decompressed-mb 4096` — letzteres steht für „ohne die
Grenze“, denn 4096 MB deckt das ganze GiB. Aufruf jeweils
`redact-rs <bombe> --check-leaks XX`; der Spitzenspeicher ist
`getrusage(RUSAGE_CHILDREN).ru_maxrss` des Kindprozesses, dieselbe Zahl, die
`/proc/<pid>/status` als `VmHWM` führt:

Der Spitzenspeicher steht hier wie überall in diesem Dokument in **MB =
1024² Byte**; `VmHWM` meldet KiB, geteilt wird also durch 1024, nicht durch
1000. (Bis zur Fix-Runde 6 rechnete diese Tabelle durch 1000 und der Fließtext
darüber durch 1024² — dieselbe Messung stand als „2 173 MB“ und als „2,1 GiB“
da.)

| Form der Bombe | mit 16 MB Budget | ohne die Grenze (4096 MB) |
|---|---|---|
| als Seiteninhalt | 24 MB, 0,02 s, **Exit 1** | 2 122 MB, 18 s, Exit 0 |
| als Objektstrom (`/ObjStm`) | 24 MB, 0,02 s, **Exit 1** | 3 145 MB, 18 s, Exit 0 |

Hier standen bis zur Fix-Runde 5 „345 MB“ und „882 MB“ für die rechte Spalte. Das konnte
nicht stimmen: ein GiB, das wirklich entpackt wird, liegt danach im Speicher,
und weniger als 1 074 MB kann eine Spitze dann nicht sein. Die Zahlen oben sind
über je drei Läufe stabil auf sechs Stellen (2 172 628 kB bzw. 3 220 164 kB,
also 2 122 MB bzw. 3 145 MB); die Zeiten hängen an der Maschine und sind nur
zur Größenordnung genannt.

Ein Strom über dem Restbudget wird übersprungen und **benannt** — die Antwort
lautet dann „nicht geprüft“, nicht „nicht gefunden“ (Rückgabewert 3, siehe
README, „Prüfen, ob die Schwärzung gewirkt hat“). Seine gepackten Bytes
durchsucht die Rohsicht trotzdem.

### Was die Zahl 62–100 nicht ist

Sie ist der Preis des **`Operation`-Vektors**, nicht der Spitzenbedarf eines
Laufs. Wo Text auf der Seite steht, kommen die `ShowRecord`s und `GlyphItem`s
des Interpreters und die `TextRun`/`Glyph`-Liste des Extraktors dazu — und die
wiegen mehr als die Operationen, aus denen sie entstehen. Für die **einzelne
Seite** ist die Zahl damit um das Vier- bis Sechsfache zu klein.

Nachgemessen (Release, `/usr/bin/time -f %M`, `--no-patterns`, unkomprimierte
Ströme, je 20 000 Textzeilen zu 36 Zeichen pro Seite):

| Aufbau | geparster Strom | Spitzenspeicher | Byte je Byte |
|---|---:|---:|---:|
| 1 Seite × 20 000 Zeilen | 1,448 MB | 492 MB | **340** |
| 2 Seiten × 20 000 Zeilen | 2,895 MB | 531 MB | 183 |
| 4 Seiten × 20 000 Zeilen | 5,791 MB | 598 MB | 103 |
| 8 Seiten × 27 000 Zeilen | 15,635 MB | 969 MB | 62 |

Über mehrere Seiten fällt das Verhältnis, weil der Arbeitssatz einer Seite
freigegeben wird, bevor die nächste beginnt. Die letzte Zeile ist der
ungünstigste Textfall, der überhaupt durchkommt: acht Seiten mit je 972 000
Zeichen, also je dicht unter der Glyphengrenze, und zusammen dicht unter
`--max-parsed-mb`. Sie landet auf 62. Auf **Dokumentebene** passt die Zahl
62–100 also; für die **einzelne Seite** passt sie nicht.

Und für eine Textseite zieht die wirksame Obergrenze ohnehin nicht
`--max-parsed-mb`, sondern die Deckelung auf **eine Million Zeichen je Seite**
(`MAX_GLYPHS_PER_SCAN` in `crates/redact-pdf/src/content.rs`). Sie greift schon
bei rund 2 MB Seiteninhalt, also weit vor dem 16-MB-Budget:

| Eine Seite | geparster Strom | Ergebnis |
|---|---:|---|
| 972 000 Zeichen | 1,954 MB | Exit 0, **658 MB** |
| 1 044 000 Zeichen | 2,099 MB | Exit 1 (Glyphengrenze), 421 MB |

**Eine pfadlastige Seite umgeht diese Deckelung** — das Glyphenbudget zählt
Zeichen, und `re`/`l`/`c` setzen keine. Aus einem Pfad bleibt zwar nichts liegen
(der Seiten-Scan hält Textoperationen, Marked Content und Formularplatzierungen,
keine Pfaddaten). Der Spitzenbedarf ist deshalb aber **nicht** der des
`Operation`-Vektors allein: was der Scan an Marked Content und
Formularplatzierungen hält, wuchs bis zur Fix-Runde 6 als Produkt und war von
`--max-parsed-mb` nicht gedeckelt (nächster Absatz). Für die Pfaddaten selbst
gilt der Satz — sie stehen im `Operation`-Vektor und darauf zählt
`--max-parsed-mb`.

Was der Seiten-Scan an **Marked Content und Formularplatzierungen** hält, ist
seit dieser Fassung zusätzlich je Seiten-Scan gedeckelt: höchstens 100 000
Zuordnungen zwischen einem Textspiegel und einer Formularplatzierung beim
Aufbau der Liste und ebenso viele beim Aufklappen, zusammen rund 16 MB. Vorher
war dieser Teil von **keiner** Grenze gedeckelt und wuchs als Produkt aus
`BDC`-Klammern und `Do`-Aufrufen: eine Datei von 276 kB machte daraus 2 306 MB
Spitzenspeicher und 41,7 s (Faktor 8 400), ohne Warnung und ohne dass eine
Decke griff. Nachgemessen,
jeweils eine Seite dicht unter dem 16-MB-Budget:

| Eine Seite | geparster Strom | Spitzenspeicher | Byte je Byte |
|---|---:|---:|---:|
| nur Pfade, `x y 40 10 re f` | 15,79 MB | 1 183 MB | 75 |
| nur Pfade, `m … l … c S` | 15,89 MB | 1 139 MB | 72 |
| 972 000 Zeichen **und** Pfade | 15,10 MB | 1 308 MB | 87 |

Der schlimmste hier gemessene Einzelseitenfall bleibt damit unter den 1,6 GB,
die aus der 100-Byte-Zeile oben folgen. Die Deckelung wird umgangen, die
Obergrenze nicht.

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

Vervierfachung bei Verdopplung — das Verhalten war zum Zeitpunkt dieser Messung
**quadratisch**. Eine knappe Megabyte-Datei genügte damit, um die Maschine eine
Stunde zu beschäftigen.

Die Tabelle ist ein Befund von damals und keine Zusage von heute: wie schnell
die Konfliktauflösung im Einzelnen ist, hängt am jeweiligen Verfahren in
`crates/redact-core/src/conflict.rs` und ändert sich mit ihm. Was sich **nicht**
ändert, ist die Form der Frage — welche Regionen überlappen einander, ist eine
Frage über *Paare*, und die Zahl der Paare wächst schneller als die Zahl der
Treffer. Ein Verfahren kann den Regelfall gut treffen und in einer ungünstigen
Anordnung trotzdem entarten, etwa wenn sehr viele Treffer dieselbe Spalte
belegen (eine IBAN auf jeder Zeile).

Deshalb begrenzt die Kette unabhängig davon die Zahl der Trefferkandidaten
(`--max-candidates`, Vorgabe 100 000); jenseits der Grenze endet der Lauf mit
Exit 2, statt beliebig lange zu rechnen.

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
* **`LZWDecode`.** Solche Streams packte bis zur Spur-A-Runde 1 `lopdf` aus,
  nicht die Vorprüfung, ohne vorab begrenzten Speicher; seither entpackt sie
  die Vorprüfung selbst, begrenzt auf das Entpackbudget (Register #64). Die
  Rohgrößen-Grenze von 16 MB, die daneben stand, ist gefallen (Register
  #82): sie schützte nichts mehr. `LZWDecode` ist ein Filter aus der Zeit vor
  PDF 1.4 und kommt in heutigen Dateien praktisch nicht mehr vor.
* **Rechenzeit unterhalb der Grenzen.** Bis zu 100 000 Trefferkandidaten werden
  ohne weitere Frage aufgelöst. Wie lange das dauert, hängt nicht nur an ihrer
  Zahl, sondern an ihrer **Anordnung**: viele Treffer in derselben Spalte sind
  teurer als dieselbe Zahl über die Fläche verteilt. Eine Sekundenzusage steht
  hier deshalb bewusst nicht. Wer eine braucht, setzt `--max-candidates`
  herunter; der Schalter wirkt vor der Auflösung.
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
    bewusst auf. Gezählt werden die Maße des Bild-Dictionaries — bei
    `DCTDecode` zusätzlich die im Kopf des JPEG: ein JPEG mit mehr
    Bildpunkten, als sein Dictionary angibt, gilt seit Register #101 als
    nicht dekodierbar. Vorher belegte der Dekoder die Maße des JPEG, ohne
    dass eine der beiden Grenzen sie sah.
  * **256 MB gleichzeitig gehaltene dekodierte Bildbytes** (`--max-image-mb`).

  **Richtigstellung.** Bis einschließlich Aufgabe #58 stand hier allein die
  Grenze je Bild — und las sich, als wäre damit der Speicherbedarf gedeckelt.
  Das war die gefährlichere Hälfte der Wahrheit: eine Grenze *je Bild* sagt
  nichts über die *Summe*. Gemessen an 1-Bit-Graustufenbildern (die gewöhnliche
  Kodierung eines Schwarzweiß-Scans) brachte eine **92-kB-Datei** mit 20 Bildern
  den Prozess auf **5 508 MB**, eine 183-kB-Datei mit 40 Bildern auf SIGABRT
  („memory allocation of 144000000 bytes failed“, Exit 134) — trotz gesetzter
  `--max-decompressed-mb` und `--max-parsed-mb`, denn die zählen den Stream, wie
  der PDF-Filter ihn ausgibt (1 Bit je Bildpunkt), nicht die RGBA8-Fassung. Und
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
`missing_page` geführt und nicht als Schwärzung verbucht, eine Region neben dem
Blatt seit 0.6.0 als `off_page`. **Beide gehören in die Nachrechnung**:
`applied + covered + degenerate + missing_page + off_page = requested`. Wer die
vierteilige Fassung aus 0.5.0 weiterbenutzt, kommt bei einem Log mit
`off_page > 0` auf eine zu kleine Summe und übersieht genau die Regionen, die
nichts bewirkt haben.

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

### Der Textspiegel eines Bildes (`/Alt`)

Ein getaggtes PDF darf Glyphen einen Spiegeltext beistellen — drei Schlüssel
an einem Marked-Content-Abschnitt, `MIRROR_KEYS` in
`crates/redact-pdf/src/content.rs`. Sie haben **zwei Rollen**: `/ActualText`
(PDF 32000-1, 14.9.4) ist der *Ersatz* der Glyphen und muss ihnen gleichen;
`/Alt` (14.9.3) *beschreibt*, `/E` (14.9.5) *schreibt aus* — beide dürfen von
den Glyphen abweichen. Gelesen und mit den Glyphen geleert werden alle drei;
gewarnt wird bei einem Widerspruch nur für `/ActualText`.

Das lässt eine benannte Lücke: der **`/Alt` eines Bildes**. Ein
`/Figure <</Alt (…)>> BDC /Im0 Do EMC` hat keine Glyphen darunter, ist die
Standardform der Barrierefreiheit und kein Befund. Steht in der Beschreibung
aber, was auf dem Bild zu lesen ist, überlebt sie die Pixel-Schwärzung des
Bildes: die Analyse liest den `/Alt` eines Bildes so wenig wie dessen Pixel —
derselbe blinde Fleck wie bei gescannten Seiten (README, „Gescannte
Dokumente“). `--check-leaks` sieht ihn: der Text steht als Klartext im
Seitenstrom, und die Rohsichten der Nachprüfung lesen den Strom, nicht die
Struktur.

Ein Spiegel über einem **Formular**, das erst auf einer anderen Seite
getroffen wird, fällt mit: die Seiten werden erst geschrieben, wenn alle
Formularpläne feststehen. Die Warnung zum geteilten Formular nennt weiterhin
die Seiten, auf denen das Formular steht; ein Spiegel bleibt auf keiner davon
stehen (gemessen mit `leaks`: 0 Fundstellen).

### Namen, die sich nur beim Aufrufer auflösen

Ein Formular mit eigenem `/Resources` darf nach der Norm nur Namen aus diesem
Verzeichnis benutzen. Poppler sucht einen fehlenden Namen trotzdem in den
Ressourcen der Aufrufer bis hinauf zur Seite und zeichnet, was es dort
findet. Der Scan folgt dem: kennt das eigene Verzeichnis eine benutzte
Schrift, ein Formular, einen Grafikzustand, ein Muster oder eine
Schattierung nicht, liest er den Strom unter den Kategorien der Aufrufer, mit
dem eigenen Verzeichnis darüber. Ein eigener Eintrag geht vor. Für
Eigenschaftslisten (`/Properties`) gilt dieselbe Suche, Name für Name. Die
Nachprüfung liest dieselbe Sicht. Belege: `zp_c_ressourcen_beim_aufrufer`,
`zo_c_spiegel_umgebungen`.

### Ein Struktur-Element, das keine Annotation erreicht

Der Metadatenlauf erreicht Struktur-Elemente nur über die Annotationen einer
Seite (`/Popup`, `/Parent`, `/Kids`, `/IRT`) und leert dort `/Alt` und
`/ActualText`. Ein `/StructElem`, das kein solcher Weg erreicht und das ein
Objekt außerhalb dieses Laufs am Leben hält, behält sein `/Alt`. Gewöhnlich
fällt der ganze `/K`-Baum mit `/StructTreeRoot` und wird weggeräumt; Beleg für
beide Richtungen: `zf_q1_luecken` und
`zf_q1_korpus::ein_strukturelement_ohne_annotation_bleibt_unberuehrt`.

### `/DA` an Annotationen bleibt stehen

Die Klartexte einer Annotation und alles, was sie erreichbar hält, werden mit
den Metadaten entfernt (README, Tabelle unter „Sicherheit“). **Eine benannte
Lücke bleibt:** `/DA` (Default Appearance) an FreeText/Widgets bleibt stehen —
Pflichtschlüssel und Operatorfolge, kein Menschentext; eine IBAN als
Schriftname in `/DA` überlebt. `--check-leaks` sieht sie: `/DA` ist eine
Zeichenkette unter einem Schlüssel, und die Objektsicht liest jede.

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

## `--check-leaks`: die Nachprüfung ohne Quelltext

Der Abschnitt „Sicherheitslücken melden“ am Ende dieser Datei nennt
`redact_pdf::leaks` als verbindliches Messgerät. Das ist eine
**Bibliotheksfunktion** — sie setzt eine Rust-Toolchain, Netzzugang zu
crates.io und einen Klon des Repositories voraus. Im Release-Archiv liegt kein
Quelltext. Für ein Werkzeug, dessen erstes Versprechen „keine Cloud, keine
Netzverbindung“ lautet, war die Kontrolle damit ausgerechnet für die Gruppe
unerreichbar, für die sie gedacht ist.

Seit 0.4.0 steckt dieselbe Funktion im ausgelieferten Binary:

```console
$ redact-rs geschwaerzt.pdf --check-leaks "DE89 3704 0044 0532 0130 00"
Geprüft: geschwaerzt.pdf (1332 Byte)
  nicht gefunden: DE89 3704 0044 0532 0130 00

Ergebnis: der Suchbegriff steht nicht mehr in der Datei.
Das heißt NICHT, dass in der Datei nichts mehr steht. Geprüft wurde genau diese
Liste. …
$ echo $?
0
```

Drei Punkte, die zur Aussage gehören:

* **Die Suchbegriffe sind Geheimnisse.** Auf der Kommandozeile stehen sie in
  der Prozessliste (`ps`) und in der Shell-Historie — dasselbe Problem wie beim
  Passwort. `--check-leaks -` liest sie zeilenweise von der Standardeingabe:
  `redact-rs geschwaerzt.pdf --check-leaks - < begriffe.txt`.
* **Ein Fund ist Rückgabewert 3**, siehe oben — **und eine nicht geprüfte
  Stelle auch.** Die Suche entpackt in Summe höchstens `--max-decompressed-mb`
  (Vorgabe 1024 MB, dieselbe Zahl wie in der Grenzentabelle; je Sicht der
  Suche einmal). Ein Strom, der das Restbudget sprengte, wird nicht entpackt
  und steht als `NICHT GEPRÜFT: …` in der Ausgabe, und der Lauf endet
  auch ohne Fund mit 3, nie mit 0: „nicht gefunden“ in einem Strom, der nie
  aufgemacht wurde, ist keine Aussage.
* **Kein Freibrief.** Geprüft ist die angegebene Liste, nicht die Datei. Ein
  zweiter Name, eine weitere Kontonummer, eine Schreibweise mit anderen
  Leerzeichen, Text in einem Rasterbild — nichts davon ist damit geprüft.

Die Oberfläche macht dieselbe Prüfung seit 0.6.0 nach jedem Export von selbst:
sie kennt die Suchbegriffe bereits (in jeder geschwärzten Zeile steht der
gefundene Text) und muss sie deshalb weder tippen lassen noch in eine
Prozessliste schreiben. Sie nennt denselben Vorbehalt und sagt zusätzlich, wie
viele Rechtecke **ohne** bekannten Text dabei waren — für die kann sie nichts
sagen, und dort bleibt es bei der Sichtprüfung.

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
der der geschwärzte Text noch zu finden ist**. Für die zweite Sorte reicht
`redact-rs <datei> --check-leaks "<text>"` — der Schalter des ausgelieferten
Binaries, kein Quelltext und keine Toolchain nötig (siehe
[oben](#--check-leaks-die-nachprüfung-ohne-quelltext)).

Nachtrag: `strings` ist dafür nur die schnelle Vorstufe und darf nicht als
Entwarnung gelesen werden — ein Flate-komprimierter Objektstrom (`/ObjStm`) ist
für eine reine Rohbyte-Suche unsichtbar, ebenso eine Zeichenkette in UTF-16BE
oder als Hex-String. `pdftotext` taugt erst recht nicht als Nachweis; die
Messung dazu steht im README unter „Prüfen, ob die Schwärzung gewirkt hat“.
Verbindlich ist `redact_pdf::leaks` — und **genau die** Funktion steckt hinter
`--check-leaks` und (als `leaks_many`, dieselben Sichten) hinter der
Nachprüfung der Oberfläche. Wer keine
Rust-Toolchain hat, nimmt den Schalter; der Bibliotheksaufruf ist derselbe
Maßstab, nur für den, der das Repository ohnehin gebaut hat.

### Was die Nachprüfung der Oberfläche zusichert — und was nicht

Sie entscheidet **am Fund**: ein wörtlicher Rest ist ein Leck. Trifft nur die
Fassung ohne Leerraum und trägt eine bewusst stehen gelassene Zeile dieselbe
Zeichenfolge, zählt der Fund nicht — die Statuszeile sagt, wie viele Texte das
betrifft. Ein Text, der **wörtlich** auch in einer bewusst stehen gelassenen
Zeile steht, wird nicht gesucht: ein Fund wäre von dieser Zeile nicht zu
unterscheiden. Die Nachprüfung behauptet darüber nichts — sie zählt diese Texte,
sagt in der Statuszeile **und** in der bleibenden Warnung, dass sie über sie
nichts weiß, und für sie bleibt die Sichtprüfung. (Bis zur Fix-Runde 6 hieß es
„zählen deshalb nicht als Leck“, ohne Warnung — auch wenn die Schwärzung
danebengegangen war.)

Sie nennt außerdem jede ungeprüfte Stelle mit **ihrem eigenen** Grund und nimmt
keine Ursache an — in der Statuszeile höchstens drei beim Namen, der Rest
gezählt („… und N weitere“); `MAX_NAMED_PLACES = 3` in
`crates/redact-gui/src/state.rs`, denn `LeakCheck::unchecked` darf bis zu
**154** Zeilen tragen, und so viele liest in einer Statuszeile niemand. Die 154
sind abgeleitet, nicht gemessen: die Decke `MAX_UNCHECKED = 50` einzeln
genannter Stellen plus Summenzeile gilt je **Zähler**, und davon gibt es drei
(zu große Ströme der Rohsicht, dieselben der Objektsicht, Stellen aus anderem
Grund), dazu die Zeile über Sicht 7: 3 × 51 + 1. Gemessen wurden 52 Zeilen aus
60 zu großen Strömen
(`zf_q4_tests::zf_q4_3_die_zahl_der_ungepruefeten_stellen_sprengt_die_zusage`).
Die Kommandozeile schreibt jede Stelle als eigene `NICHT GEPRÜFT:`-Zeile und
zählt im Ergebnissatz **Stellen**, nicht Zeilen.

**Fünf Gründe gibt es, nicht drei** — so viele kennt
`redact_pdf::leaks_many_within` heute. Bis zur Fix-Runde 6 zählte dieser
Abschnitt drei auf; die beiden fehlenden waren gerade die, die die Fix-Runde 5
hinzugefügt hatte:

| Grund | Wortlaut in der Meldung |
|---|---|
| Entpackgrenze | `nicht entpackt — N Byte gepackt, entpackt mehr als die verbleibenden …` |
| Vorprüfung des Laders abgelehnt | `Objektgraph (Sichten 3–7) nicht durchsucht — die Vorprüfung des Laders lehnt die Datei ab: …` |
| Verschachtelungstiefe des Objektgraphen | `nicht durchsucht — Verschachtelungstiefe 32 erreicht` |
| Filtername, den das Programm nicht kennt | `nur bis Filter N von M dekodiert` bzw. `gar nicht dekodiert — /FooDecode ist hier kein bekannter Filter` |
| Schriftdekoder nicht gelaufen | `Sicht 7 (Schriftdekoder) nicht gelaufen: N Strom/Ströme wurden nicht entpackt` |

Die letzte Zeile ist eine Folge der ersten: bleibt auch nur ein Strom
ungepackt, läuft Sicht 7 gar nicht erst, weil der Schriftdekoder ohne eigene
Grenze entpackt. Seit Register #83 bucht die Vorprüfung jeden Strom
mindestens so groß, wie die Objektsicht ihn mit demselben Dekoder entpackt;
eine Datei, die sie mit demselben Budget durchlässt, erreicht die Zeile
„nicht entpackt“ der Objektsicht und die über Sicht 7 nur noch, wo die zwei
Dekoder aus einem Strom verschieden viel holen. Beide bleiben als Rückfall;
die Entpackgrenze der **Rohsicht** — ein zlib-Strom ohne `/Filter`, den die
Vorprüfung roh zählt — ist die Zeile, die man an einer gewöhnlichen
Kommandozeile noch sieht.

### Interpreter und Orakel lesen verschieden

Der Interpreter nimmt einen halb dekodierten Strom **nie** als Seiteninhalt:
eine Filterkette mit unbekanntem Glied bricht mit Warnung ab. Das Orakel
durchsucht den entzifferbaren Anfang trotzdem — es soll finden, was sichtbar
ist, nicht schwärzen. Festgehalten in
`ze_p2_seitenschleife::halb_dekodierter_strom_wird_nie_seiteninhalt`.

**Was ein sauberer Lauf nicht ausschließt.** Text hinter einem Bildfilter
(`/DCTDecode`, `/JPXDecode`, `/CCITTFaxDecode`, `/JBIG2Decode`) — **am Ende der
Filterkette**: ein benannter blinder Fleck und **keine** `NICHT
GEPRÜFT`-Zeile, sonst käme jede Datei mit einem Foto als unvollständig geprüft
zurück. Geht die Kette hinter dem Bildfilter **weiter**, ist das etwas anderes:
dort hat keine Sicht gelesen, und seit der Fix-Runde 7 steht die Stelle in der
`NICHT GEPRÜFT`-Liste (Rückgabewert 3). Bis dahin schwieg der Lauf auch da. Ein Filtername, den das Programm gar nicht kennt, steht sehr
wohl darin — **an jeder Stelle der Kette**, auch als erstes Glied. Bis zur
Fix-Runde 6 galt das nur, wenn vorher schon ein Filter gelaufen war:
`/Filter /FooDecode` allein kam als „nicht gefunden“ mit Rückgabewert 0 zurück,
`/Filter [/FlateDecode /FooDecode]` mit 3 — dieselbe unlesbare Stelle, und die
Meldung hing allein an der Position. Am gebauten Binary nachgemessen, sechs
Ketten — jede Zeile aus einem Lauf
(`zg_r5_filterketten::jede_zeile_der_filterkettentabelle_stammt_aus_einem_lauf`;
die ersten fünf zusätzlich in
`zf_q5_unbekannter_filter::die_zusage_ueber_unbekannte_filter_gilt_an_jeder_stelle_der_kette`):

| Filterkette | Meldung | Rückgabewert |
|---|---|---|
| `/FooDecode` | `gar nicht dekodiert — /FooDecode ist hier kein bekannter Filter (Glied 1 von 1)` | 3 |
| `[/FooDecode /FlateDecode]` | `… (Glied 1 von 2)` | 3 |
| `[/FlateDecode /FooDecode]` | `nur bis Filter 1 von 2 dekodiert — /FooDecode ist hier kein bekannter Filter` | 3 |
| `/DCTDecode` | keine | 0 |
| `[/FlateDecode /DCTDecode]` | keine | 0 |
| `[/DCTDecode /ASCII85Decode]` | `gar nicht dekodiert — /DCTDecode ist ein Bildfilter und wird nicht dekodiert, aber die Kette geht dahinter weiter (Glied 1 von 2)` | 3 |

Ebenso benannt: ein Textspiegel in einer direkt in
`/Resources /Properties` stehenden Eigenschaftsliste bleibt im
Ressourcenverzeichnis stehen (Beleg:
`ze_p2_spiegel::befund_direkte_eigenschaftsliste_behaelt_ihren_spiegel`).

Es gibt keine Prämie und keine zugesicherte Frist — dies ist ein kleines
Projekt. Eingehende Meldungen werden aber beantwortet.
