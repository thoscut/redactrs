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
manuelle Regionen (`--manual-regions`) und Review-Dateien (`--apply-review`)
kommen vom Bedienenden. Sie werden auf Plausibilität geprüft, aber nicht als
Angriffsfläche behandelt. Wer eine fremde Musterdatei einspielt, spielt eine
fremde Konfiguration ein — dieselbe Vertrauensstufe wie ein Kommandozeilen-
Argument.

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
* **`#![forbid(unsafe_code)]` in allen eigenen Crates** (`redact-core`,
  `redact-pdf`, `redact-patterns`, `redact-booking`, `redact-render`,
  `redact-gui`, `redact-cli`). Der Compiler setzt das durch; es ist keine
  Absichtserklärung.
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

---

## Grenzen für Eingabedateien

| Grenze | Vorgabe | Stellschraube |
|--------|---------|---------------|
| Verschachtelungstiefe (`[`, `<<`) | 128 | fest |
| dito, in binär aussehender Nutzlast | 256 | fest |
| entpackte Bytes über **alle** Streams | 1024 MB | `--max-decompressed-mb` |
| davon: Streams, die geparst werden | 16 MB | `--max-parsed-mb` |
| Trefferkandidaten je Datei | 100 000 | `--max-candidates` |
| Rohgröße eines LZW-/ASCII85-Streams | 16 MB | fest |

Die Vorprüfung (`redact_pdf::document::prescan`) läuft über die **Rohbytes**,
bevor `lopdf` die Datei zu sehen bekommt, und schließt die ausgepackten Streams
mit ein. Sie muss davor laufen: der Stapelüberlauf beendet den Prozess, bevor
irgendein Fehlerwert entstehen könnte.

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

Die Schwelle in `lopdf 0.34` liegt bei etwa 500–2 000 Ebenen (Debug-Build) bzw.
5 000–10 000 (Release). Die Grenze 128 liegt weit darunter und weit über allem,
was in echten Dokumenten vorkommt.

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
werden. 16 MB × 62 ≈ 1 GB ist die Obergrenze, die daraus folgt.

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
Diese Crates sehen Bytes aus dem Eingabe-PDF und enthalten `unsafe`
(Vorkommen im jeweiligen `src/`):

| Crate | `unsafe` | sieht Angreiferdaten |
|---|---|---|
| `bytemuck` 1.25 | 368 | ja — unter `skrifa` und `tiny-skia` |
| `tiny-skia` 0.12 | 152 | ja — rastert Pfade und Koordinaten aus dem PDF (Vorschau) |
| `zune-jpeg` 0.5 | 85 | ja — dekodiert eingebettete JPEGs |
| `flate2` 1.1 | 36 | ja — packt jeden komprimierten Stream aus |
| `eframe` 0.29 | 21 | mittelbar — nur in der grafischen Oberfläche |
| `skrifa` 0.33 / `read-fonts` 0.31 | 0 (selbst) | ja — liest eingebettete Schriften, stützt sich auf `bytemuck` |

Ein Speicherfehler in einer dieser Bibliotheken ist ein Speicherfehler in
redact-rs. Wer ein PDF aus wirklich unbekannter Quelle verarbeitet, sollte das
in einer Sandbox tun (Container, `bwrap`, eigenes Benutzerkonto) — und wer nur
die Kommandozeile braucht, baut ohne die Oberfläche:
`cargo build --release -p redact-cli --no-default-features`.

Anmerkung zur Einordnung: `lopdf 0.34` selbst enthält **kein** `unsafe` — und
stürzt trotzdem ab. „Sicheres Rust“ schützt vor Speicherfehlern, nicht vor
unbegrenzter Rekursion und nicht vor unbegrenztem Speicherverbrauch.

### Dienstverweigerung

Die oben gemessenen Fälle sind begrenzt. Nicht begrenzt sind:

* **Andere Wege in die Rekursion.** Die Vorprüfung zählt `[` und `<<`. Findet
  jemand einen anderen Pfad in `lopdf`, der tief rekursiert, greift sie nicht.
  Die eigentliche Schwachstelle RUSTSEC-2026-0187 besteht weiter; sie ist nur
  nicht mehr erreichbar — siehe „Warum lopdf noch auf 0.34 steht".
* **`LZWDecode`.** Solche Streams packt `lopdf` aus, nicht die Vorprüfung. Sie
  werden deshalb bis zu einer Rohgröße von 16 MB an `lopdf` durchgereicht;
  wieviel Speicher der Dekoder dabei belegt, ist nicht vorab begrenzt. Über
  16 MB Rohgröße wird die Datei abgelehnt. `LZWDecode` ist ein Filter aus der
  Zeit vor PDF 1.4 und kommt in heutigen Dateien praktisch nicht mehr vor.
* **Rechenzeit unterhalb der Grenzen.** 100 000 Trefferkandidaten kosten
  rund 17 s. Das ist gewollt großzügig; wer engere Zusagen braucht, setzt
  `--max-candidates` herunter.
* **Sehr große Bilder.** Streams mit `/Subtype /Image` werden nicht auf
  Klammertiefe untersucht — hier ist das belegbar unbedenklich, weil `lopdf`
  sie gar nicht auspackt. Ihre entpackte Größe zählt aber gegen das große
  Budget.

### Die Zusicherungen oben gelten für die Kommandozeile, nicht für die Oberfläche

Nachtrag zur Dokumentationsprüfung (Stand: dieser Commit). Zwei der Punkte unter
„Was zugesichert wird“ beschreiben den Weg durch `redact-cli`. Die grafische
Oberfläche ist ein eigenständiger Ablauf — `redact-gui` hat keine Abhängigkeit
auf `redact-cli` — und weicht davon ab:

* **Der eine Schreibpfad ist nicht der einzige.** Das geschwärzte PDF geht auch
  in der Oberfläche durch `write_file`. Audit-Log und Review-Datei nicht: sie
  entstehen über `std::fs::write` (`crates/redact-gui/src/state.rs:1023` bzw.
  `crates/redact-gui/src/app.rs:781`). Damit fehlen für diese beiden Dateien
  der Modus `0600`, die Symlink-Prüfung, die Kanonisierung des Zielpfads und
  die `create_new`+`rename`-Sequenz. Sie entstehen mit den Vorgaberechten des
  Kontos — und in beiden steht Klartext (siehe unten).
* **Die Grenzen für Eingabedateien sind dort nicht einstellbar.** `--max-…`
  sind Argumente der Kommandozeile. Die Oberfläche lädt über
  `redact_pdf::load_from_bytes`, also mit `Limits::default()`: die Vorprüfung
  läuft, aber mit den fest eingebauten Werten aus der Tabelle oben.

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

### Die Prüfsummen-Sperre der Review-Datei greift nur bei gesetzter Prüfsumme

`--apply-review` vergleicht den SHA-256 der Eingabe mit `input.sha256` der
Review-Datei und bricht bei Abweichung ab. Ist das Feld jedoch **leer**, kehrt
die Prüfung ohne Befund zurück (`crates/redact-cli/src/pipeline.rs:250`) — die
Datei wird dann auf jedes beliebige Dokument angewendet, und die Rechtecke
landen an falscher Stelle. Eine von Hand erstellte oder zusammenkopierte
Review-Datei muss die Prüfsumme also mitführen.

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

### Warum `lopdf` noch auf 0.34 steht

Der Sprung auf 0.42 (dort ist RUSTSEC-2026-0187 behoben) ist technisch klein,
lag aber außerhalb dessen, was dieser Arbeitsschritt anfassen durfte. Zu
ändern wären:

* `crates/redact-pdf/src/audit_bytes.rs` — `Dictionary::type_is` entfällt
  (`get_type()` liefert jetzt `&[u8]` statt `&str`), und `Stream::filters()`
  liefert `Vec<&[u8]>` statt `Vec<String>`, was `manual_decode` durchschlägt.
* `crates/redact-pdf/src/document.rs` — `Object::type_name()` liefert `&[u8]`.

Auf 0.44 kommen `content.rs`, `ops.rs` und `redact.rs` hinzu
(`Document::get_page_content()` liefert `Vec<u8>` statt `Result<Vec<u8>>`).

Zwei Verhaltensänderungen sind vor dem Umstieg zu prüfen: `lopdf 0.42` behält
kein `/Prev` mehr im Trailer (das nutzt `has_incremental_history`, um auf
Vorversionen hinzuweisen), und es liest eingebettete Bilder anders, wodurch zwei
Kanarienvogel-Tests in `crates/redact-pdf/tests/known_leaks.rs` anschlagen —
dort vermutlich zum Guten.

Bis dahin ist die Vorprüfung die Absicherung, und der Eintrag in `deny.toml`
bleibt stehen. Er ist befristet, nicht dauerhaft.

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
