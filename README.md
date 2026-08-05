# redact-rs

Ein schlankes, **lokales** CLI- und GUI-Werkzeug in Rust zum Schwärzen sensibler
Daten in PDF-Dokumenten (Bankunterlagen, Kontoauszüge, Rechnungen).

* **Manuelle Schwärzung** über Regionen (JSON oder per Maus in der GUI)
* **Automatische Schwärzung** über Regex-Muster (IBAN, BIC, Steuer-ID, …)
* **Buchungsliste** mit Positiv- und Negativliste (CSV)
* **Echte Schwärzung**: der gefundene Text wird aus dem Content-Stream
  *entfernt*, nicht nur übermalt
* **Auch in Bildern**: liegt eine Schwärzung über einem Rasterbild, werden die
  betroffenen **Pixel überschrieben**, nicht überdeckt. Gescannte Seiten lassen
  sich damit ohne OCR sicher schwärzen — die Rechtecke zieht man von Hand
  ([Details](#bilder))
* **Keine Cloud**, keine Netzverbindung, deterministische Ausgabe

> **Vor dem ersten Einsatz bitte zwei Abschnitte lesen:**
> [Prüfen, ob die Schwärzung gewirkt hat](#pruefen) und
> [Was dieses Werkzeug nicht leistet](#grenzen).
> Ein Werkzeug für Bankunterlagen ist nur so viel wert wie die Kontrolle
> dahinter — und die verbreitete Kontrolle (`pdftotext … | grep …`) gibt
> nachweislich falsche Entwarnung.

> **Wer von einer älteren Fassung kommt, liest zuerst
> [`CHANGELOG.md`](CHANGELOG.md).** Zwischen den Fassungen dieses Werkzeugs
> sind mehrfach Fälle geschlossen worden, in denen eine ältere Fassung
> Geheimnisse in der Ausgabe stehen ließ und trotzdem Erfolg meldete. Die
> betroffenen Einträge sind dort mit **⚠ Sicherheit** gekennzeichnet und nennen
> jeweils, ob bereits erzeugte Ergebnisse nachzuprüfen sind.

---

## Inhalt

1. [Schnellstart](#schnellstart)
2. [Installation](#installation)
3. [Kommandozeile](#kommandozeile)
   * [Verschlüsselte PDFs](#verschluesselte-pdfs)
   * [Mehrere Dateien auf einmal](#stapel)
   * [Einstellungsdatei](#einstellungsdatei)
   * [Automatische Funde abschalten](#automatische-funde-abschalten)
4. [Buchungsliste](#buchungsliste)
5. [Manuelle Regionen](#manuelle-regionen)
6. [Eigene Patterns](#eigene-patterns)
7. [Bilder werden wirklich geschwärzt](#bilder)
8. [Review-Workflow](#review-workflow)
9. [Audit-Log](#audit-log)
10. [Prüfen, ob die Schwärzung gewirkt hat](#pruefen)
11. [Was dieses Werkzeug nicht leistet](#grenzen)
12. [Grafische Oberfläche](#grafische-oberfläche)
    * [Rechtecke ziehen, verschieben und an den Ecken nachziehen](#rechtecke-ziehen-verschieben-und-an-den-ecken-nachziehen)
    * [Rückgängig und Wiederholen](#rückgängig-und-wiederholen)
    * [Miniaturansichten und Zoom](#miniaturansichten-und-zoom)
    * [Tastaturbedienung](#tastaturbedienung)
13. [Architektur](#architektur)
14. [Sicherheit — was zugesichert wird](#sicherheit)
15. [Abweichungen vom Ursprungskonzept](#abweichungen-vom-ursprungskonzept)
16. [Entwicklung](#entwicklung)
17. [Release bauen](#release-bauen)

Daneben: [`CHANGELOG.md`](CHANGELOG.md) — was sich zwischen zwei Fassungen
geändert hat, und was davon sicherheitsrelevant war.
[`SECURITY.md`](SECURITY.md) — Bedrohungsmodell, Grenzen für Eingabedateien,
Messungen.

---

## Schnellstart

```bash
# Beispiel-Kontoauszug erzeugen
redact-rs --write-demo kontoauszug.pdf

# Automatisch schwärzen — Ergebnis landet als kontoauszug_geschwaerzt.pdf
# direkt neben der Eingabedatei
redact-rs kontoauszug.pdf --patterns iban_de,bic,email
```

Und dann **nicht** aufhören. Der Lauf oben schränkt mit `--patterns` bewusst auf
drei Muster ein und entfernt darum nur IBAN, BIC und E-Mail-Adresse — Kontoinhaber,
Kontonummer, Telefonnummer und Steuer-ID lässt er stehen (nachgemessen:
4 Schwärzungen). Ohne `--patterns` greifen die Vorgabemuster und es werden
7 Schwärzungen; übrig bleibt dann von den schützenswerten Angaben nur der
**Name**, denn dafür gibt es kein Muster:

```console
$ redact-rs kontoauszug.pdf -o std.pdf --force
Treffer gesamt:     7
Schwärzungen:       7
$ pdftotext std.pdf -
Kontoinhaber: Max Mustermann      ← steht noch da
IBAN:
Kontonummer:
Telefon:
Steuer-ID:
```

Wie man das feststellt, steht unter
[Prüfen, ob die Schwärzung gewirkt hat](#pruefen).

## Installation

Fertige Binaries liegen unter
[Releases](https://github.com/thoscut/redactrs/releases) — **drei Archive**:

| Archiv | Inhalt | wofür |
|---|---|---|
| `redact-rs-<version>-x86_64-windows.zip` | `redact-rs.exe` + `redact-rs-gui.exe` | Windows, mit Oberfläche |
| `redact-rs-<version>-x86_64-linux.tar.gz` | `redact-rs` (glibc) | Linux, mit Oberfläche |
| `redact-rs-<version>-x86_64-linux-musl.tar.gz` | `redact-rs` (statisch, **ohne** Oberfläche) | Linux-Fassungen, auf denen der glibc-Bau nicht läuft — Debian 12, RHEL/Alma/Rocky 8 und 9, Ubuntu 22.04. Keine Systemvoraussetzungen. |

Das musl-Archiv ist die reine Kommandozeile (`--no-default-features`): kein
`--gui`, kein `redact-rs-gui`. Alles andere ist identisch.

Das Windows-Archiv enthält **zwei Programme**:

| Datei | wofür |
|---|---|
| `redact-rs-gui.exe` | Die zum **Doppelklicken**. Öffnet nur das Fenster, ohne die schwarze Konsole dahinter, und nimmt ein PDF entgegen, das man auf ihr Symbol zieht. |
| `redact-rs.exe` | Die **Konsolenfassung** mit allen Optionen — und der einzige Weg zu Ausgaben auf stdout (`--json`, `--list-patterns`). |

Zwei Dateien statt eines Schalters, weil `#![windows_subsystem = "windows"]`
das Konsolenfenster nur um den Preis *jeder* Ausgabe auf stdout/stderr abschaltet
(siehe `crates/redact-cli/src/bin/redact-rs-gui.rs`). Unter Linux gibt es diese
Trennung nicht; dort startet `redact-rs` ohne Argumente die Oberfläche.

Dazu liegt in **jedem** der drei Archive (nachgesehen in
`.github/workflows/release.yml`):

* dieser `README.md` und [`SECURITY.md`](SECURITY.md) — sonst liefen die
  Verweise in beiden Richtungen ins Leere,
* [`CHANGELOG.md`](CHANGELOG.md) — wer eine ältere Fassung ersetzt, muss ohne
  Netzzugang sehen können, welche Lecks dazwischen geschlossen wurden. **Ab der
  nächsten Fassung**; die v0.3.0-Archive enthalten ihn noch nicht (nachgesehen
  in den ausgelieferten Archiven).
* die drei Lizenztexte `LICENSE-MIT`, `LICENSE-APACHE` und `LICENSE-OFL.txt`
  (die eingebetteten Schriften stehen unter der SIL Open Font License),
* das Verzeichnis `examples/`.

**Die SHA-256-Prüfsummen liegen *nicht* im Archiv.** Sie sind eigene
Release-Dateien und müssen getrennt heruntergeladen werden — was auch der
einzige Weg ist, auf dem sie etwas beweisen: eine Prüfsumme im geprüften
Archiv prüft nichts.

| Release-Datei | Inhalt |
|---|---|
| `SHA256SUMS` | Prüfsummen der Archive |
| `SHA256SUMS-BINARIES` | Prüfsummen der entpackten Binaries |

```bash
sha256sum -c SHA256SUMS                              # Linux
Get-FileHash <datei> -Algorithm SHA256               # Windows (PowerShell)
```

Selbst bauen (benötigt **Rust 1.88** oder neuer — `rust-version` in
`Cargo.toml`):

```bash
git clone https://github.com/thoscut/redactrs
cd redactrs
cargo build --release                                    # mit GUI
cargo build --release -p redact-cli --no-default-features # nur CLI
# Binary: target/release/redact-rs
```

Das `-p redact-cli` im zweiten Aufruf ist nicht optional: `--no-default-features`
allein wirkt auf **alle** Mitglieder des Workspace, und `redact-gui` ist selbst
ein Mitglied — es würde also samt `eframe`/`egui`/`rfd` trotzdem gebaut. Erst die
Paketauswahl nimmt die Oberfläche aus dem Abhängigkeitsgraphen (dasselbe tut
`scripts/build-windows.sh --no-gui`).

`cargo build --release` erzeugt **drei** ausführbare Dateien, von denen nur die
ersten beiden ausgeliefert werden:

| Binary | Herkunft | Status |
|---|---|---|
| `redact-rs` | `crates/redact-cli/src/main.rs` | die Konsolenfassung, im Release enthalten |
| `redact-rs-gui` | `crates/redact-cli/src/bin/redact-rs-gui.rs` | die Fensterfassung, im Windows-Archiv enthalten |
| `redact-gui` | `crates/redact-gui/src/main.rs` | Nebenprodukt des GUI-Crates, **nicht** im Release und nirgends dokumentiert außer hier. Es öffnet dasselbe Fenster wie `redact-rs-gui`, kennt aber `-h`/`-V` und weist unbekannte Optionen zurück (`redact-rs-gui` übergeht sie, weil es unter Windows ohne Konsole nichts melden könnte). Beim Entwickeln praktisch, weil es ohne `redact-cli` gebaut wird. Für den Einsatz ist `redact-rs` bzw. `redact-rs-gui` gemeint. |

Voraussetzungen zum Bauen der GUI unter Linux:

```bash
sudo apt-get install libgtk-3-dev libxkbcommon-dev libwayland-dev
```

## Kommandozeile

Ohne `-o` wird **neben der Eingabedatei** gespeichert: aus `kontoauszug.pdf`
wird `kontoauszug_geschwaerzt.pdf`. Der Zusatz lässt sich mit
`--output-suffix` ändern — oder dauerhaft in der
[Einstellungsdatei](#einstellungsdatei). Eine vorhandene Datei wird nur mit `--force`
überschrieben — ein zweiter Lauf soll ein bereits geprüftes Ergebnis nicht
unbemerkt ersetzen. Für `--review` gilt dasselbe Schema
(`kontoauszug_review.json`).

Der Namenszusatz ist **ein Name**: Pfadtrenner, `..` und Steuerzeichen werden
abgelehnt, bevor irgendetwas entsteht — sonst wäre er ein Wegweiser aus dem
Eingabeverzeichnis heraus (die Begründung steht in
[`SECURITY.md`](SECURITY.md)). Geprüft wird er **auch dann, wenn `-o` gesetzt
ist** und der Zusatz gar nicht gebraucht würde: ein untaugliches
`--output-suffix` soll auffallen, wo es steht, und nicht erst im nächsten Lauf
ohne `-o`.

```console
$ redact-rs kontoauszug.pdf -o ok.pdf --output-suffix "/../boese"
Fehler: Konfigurationsfehler: der Namenszusatz „/../boese“ ist keiner: er
enthält einen Pfadtrenner. …
$ echo $?
2
```

```text
redact-rs [EINGABE.pdf | VERZEICHNIS …] [OPTIONEN]

  -o, --output <PDF>          Ausgabedatei (Standard: neben der Eingabe)
      --output-suffix <TEXT>  Namenszusatz (Standard: _geschwaerzt)
      --password <PW>         Passwort eines verschlüsselten PDFs
                              (besser: REDACT_RS_PASSWORD — siehe unten)
  -f, --force                 vorhandene Ausgabedatei überschreiben
      --patterns <IDs>        Muster, kommagetrennt (z.B. iban_de,bic)
      --no-patterns           gar keine Muster anwenden
      --disable-pattern <ID>  einzelnes Muster abschalten (mehrfach möglich)
      --patterns-config <F>   eigene Musterkonfiguration (YAML oder JSON)
      --min-confidence <WERT> Mindestvertrauen eines Treffers (Standard: 0.5)
      --booking-list <CSV>    Buchungsliste (Positiv-/Negativliste)
      --manual-regions <JSON> manuell festgelegte Regionen
      --review                nur analysieren, nichts schwärzen
      --review-out <JSON>     Zieldatei des Review-Exports
      --apply-review <JSON>   geprüfte Review-Datei anwenden
      --allow-unverified-review   Review-Datei ohne Prüfsumme trotzdem anwenden
                                  (UNSICHER — siehe Review-Workflow)
      --audit-log <JSON>      Audit-Log schreiben
      --action <ART>          blackout (Standard) | whiteout | replace
      --replace-with <TEXT>   Ersatztext für --action replace
      --padding <PUNKT>       Rand um jede Schwärzung (Standard: 1.0)
      --allow-undecodable-images  nicht dekodierbare Bilder durchgehen lassen
                                  (UNSICHER — siehe unten)
      --max-decompressed-mb <MB>  Budget für alle entpackten Streams (1024)
      --max-parsed-mb <MB>        davon für geparste Streams (16)
      --max-image-mb <MB>         gleichzeitig gehaltene dekodierte Bildbytes (256)
      --max-input-mb <MB>         Obergrenze für die Eingabedatei selbst (512)
      --max-candidates <N>        Obergrenze für Trefferkandidaten (100000)
      --gui                   grafische Oberfläche starten
      --list-patterns         eingebaute Muster auflisten
      --write-demo <PDF>      Beispieldatei erzeugen
      --json                  Zusammenfassung als JSON
  -q, --quiet                 weniger Ausgabe
```

Die fünf `--max-…`-Grenzen sind Schutzschalter gegen präparierte Eingabedateien;
was sie abwehren und warum sie so hoch bzw. so niedrig liegen, steht in
[`SECURITY.md`](SECURITY.md).

**Sie gelten in beiden Programmen.** Die Oberfläche lädt über
`redact_pipeline::load_document` mit **derselben** `Config` wie ein Lauf ohne
`--gui` (`crates/redact-gui/src/state.rs`), liest die Datei über dasselbe
`redact_pipeline::read_input` und geht bei der Trefferauswertung durch dasselbe
`collect_regions_for`, das `--max-candidates` prüft. Ein `redact-rs --gui
auszug.pdf --max-parsed-mb 4` wirkt also wirklich. Was **nicht** gilt: die
Fensterfassung `redact-rs-gui.exe` hat gar keine Kommandozeile und arbeitet
deshalb immer mit den Vorgaben aus der Tabelle in
[`SECURITY.md`](SECURITY.md) — nicht abgeschaltet, nur nicht verstellbar.

`--allow-unverified-review` ist neben `--allow-undecodable-images` der zweite
Schalter, der eine Prüfung aufhebt: siehe [Review-Workflow](#review-workflow).

`--allow-undecodable-images` ist der einzige Schalter, der die Sicherheit
*senkt*: siehe [Bilder werden wirklich geschwärzt](#bilder).

Rückgabewerte:

| Wert | Bedeutung |
|---|---|
| `0` | Erfolg — und nichts blieb ungeprüft |
| `1` | Verarbeitungsfehler. Im Stapelbetrieb: mindestens eine Datei ist gescheitert |
| `2` | Bedienfehler (Argumente, Einstellungsdatei, Prüfsummen, `--max-candidates`) |
| `3` | **Verarbeitet, aber nicht vollständig geprüft.** Es ist eine Ausgabedatei entstanden, aber mindestens eine Stelle des Dokuments **konnte** die Analyse nicht durchsuchen — ein XObject ohne bekanntes `/Subtype` etwa, oder ein Bild, das sich nicht dekodieren lässt. Was dort steht, kann nicht geschwärzt worden sein. Der Lauf sagt auf stderr, welche Stellen das waren: sie stehen dort mit `NICHT GEPRÜFT` statt `Warnung`, und am Ende steht ihre Zahl. |

`3` ist kein Fehler und kein „alles gut“ — es ist die Aufforderung, genau diese
Stellen anzusehen. In einem Skript gehört er behandelt wie ein Fehler, solange
niemand hingeschaut hat.

Ein gewöhnliches `Warnung:` setzt den Rückgabewert **nicht**. Ein Rasterbild
auf der Seite ist eine bekannte Grenze des Verfahrens und der Normalfall bei
gescannten Auszügen; `NICHT GEPRÜFT` meint etwas anderes, nämlich eine Stelle,
an der der Interpreter ausgestiegen ist.

<a id="verschluesselte-pdfs"></a>
### Verschlüsselte PDFs

Ohne Passwort werden sie abgelehnt — das bleibt so. Mit Passwort werden sie
entschlüsselt und wie jede andere Datei verarbeitet:

```bash
# Bequem, aber lesbar: die Kommandozeile steht in der Prozessliste (ps)
# und in der Shell-Historie
redact-rs auszug.pdf --password geheim

# Besser: über die Umgebung — nimmt keinen der beiden Wege
 REDACT_RS_PASSWORD=geheim redact-rs auszug.pdf

# Oder ohne Tippen: die Oberfläche fragt in einem Fenster mit verdeckter
# Eingabe, sobald ein verschlüsseltes Dokument geöffnet wird
redact-rs --gui auszug.pdf
```

Das Passwort landet **in keiner erzeugten Datei** — nicht in der Ausgabe-PDF,
nicht im Audit-Log, nicht in der Review-Datei — und in keiner Fehlermeldung.

**Ein Passwort schaltet keine Grenze ab.** Die Vorprüfung läuft über Rohbytes
und sieht an einer verschlüsselten Datei nur die Objektstruktur, nicht die
Streams — deshalb läuft **nach** der Entschlüsselung dieselbe Prüfung ein
zweites Mal auf dem entschlüsselten Dokument
(`redact_pipeline::check_limits_after_decryption`). Es misst derselbe Code mit
denselben Grenzen und denselben Meldungen wie bei einer unverschlüsselten
Datei. Vorher hing an einem Passwort, ob überhaupt gemessen wurde: eine
196-kB-Datei mit einem 64 MB entpackenden Content-Stream wurde ohne Passwort in
0,0 s abgelehnt und lief mit Passwort in den OOM-Killer.

Was auch danach ungeprüft bleibt (der Arbeitsspeicher wird nicht
überschrieben; Berechtigungsbits werden nicht durchgesetzt), steht in
[`SECURITY.md`](SECURITY.md#passwörter-verschlüsselter-pdfs).

Ohne Passwort ist die Meldung knapp und nennt den Ausweg **nicht** — den nennt
`redact-rs --help` unter `--password`:

```console
$ redact-rs auszug.pdf -o out.pdf
Fehler: PDF-Fehler: auszug.pdf: Dokument ist verschlüsselt. Verschlüsselte PDFs
werden nicht verarbeitet — bitte vorher entschlüsseln.
$ echo $?
1
```

<a id="stapel"></a>
### Mehrere Dateien auf einmal

Mehrere Eingabedateien oder ein Verzeichnis ergeben einen Stapel. Je Datei
entsteht ein Ergebnis **neben der Eingabe**, am Ende steht eine
Zusammenfassung:

```bash
# Alle PDFs eines Verzeichnisses (oberste Ebene, nicht rekursiv)
redact-rs auszuege/

# Oder einzeln benannt
redact-rs januar/auszug.pdf februar/auszug.pdf
```

Nachgemessen an zwei Kopien des Demo-Kontoauszugs (`--write-demo`). **stdout**:

```text
januar/auszug.pdf → januar/auszug_geschwaerzt.pdf (7 Schwärzung(en))
februar/auszug.pdf → februar/auszug_geschwaerzt.pdf (7 Schwärzung(en))

2 Datei(en): 2 vollständig geprüft, 0 verarbeitet (aber nicht vollständig geprüft), 0 fehlgeschlagen.
```

Dazu **auf stderr**, und zwar jeweils **bevor** die Datei geöffnet wird — sonst
hielte eine riesige Datei den Stapel auf, ohne dass ihr Name irgendwo stünde:

```text
[1/2] januar/auszug.pdf
[2/2] februar/auszug.pdf
```

**Die Zusammenfassung nennt drei Zahlen, nicht zwei.** „Verarbeitet“ hieß früher
auch für die Dateien, deren Text niemand gelesen hatte: wer zwanzig Auszüge
laufen ließ, bekam „20 verarbeitet, 0 fehlgeschlagen“ und Rückgabewert 0,
obwohl in einer davon eine Kontonummer unberührt stand. Die mittlere Zahl steht
deshalb für sich — es sind die Dateien mit [Rückgabewert 3](#kommandozeile), und
keine Datei zählt in zweien mit.

So sieht ein Stapel aus, in dem eine Datei nur zum Teil geprüft werden konnte
und eine gar nicht zu öffnen war (nachgemessen an drei Dateien in einem
Verzeichnis). **stdout**:

```text
./a.pdf → ./a_geschwaerzt.pdf (7 Schwärzung(en))
./b.pdf → ./b_geschwaerzt.pdf (0 Schwärzung(en))  ← nicht vollständig geprüft

3 Datei(en): 1 vollständig geprüft, 1 verarbeitet (aber nicht vollständig geprüft), 1 fehlgeschlagen.

Bei 1 Datei(en) blieb ein Teil des Dokuments ungelesen — was dort steht, kann nicht geschwärzt worden sein. Die Stellen stehen oben auf stderr; bitte diese Ergebnisse von Hand prüfen. (Rückgabewert 3.)
```

**stderr**:

```text
[1/3] ./a.pdf
[2/3] ./b.pdf
NICHT VOLLSTÄNDIG GEPRÜFT ./b.pdf: Das Form-XObject „Fm0“ (Objekt 6 0) steht in
den Ressourcen, wird aber nirgends gezeichnet; sein Text wurde nicht durchsucht
und kann deshalb nicht geschwärzt worden sein.
[3/3] ./c.pdf
FEHLGESCHLAGEN ./c.pdf: PDF-Fehler: ./c.pdf: keine PDF-Datei (Header %PDF- fehlt)
```

Mit `--quiet` bleiben von beiden Strömen genau die beiden Zeilen übrig, die
etwas zu bedeuten haben — `NICHT VOLLSTÄNDIG GEPRÜFT` und `FEHLGESCHLAGEN`;
stdout bleibt dann leer, und auch die Fortschrittszeilen entfallen
(nachgemessen).

Der Rückgabewert war hier **`1`**: eine gescheiterte Datei sticht die
unvollständig geprüfte. Ohne die kaputte Datei ist er `3` (ebenfalls
nachgemessen: `2 Datei(en): 1 vollständig geprüft, 1 verarbeitet (aber nicht
vollständig geprüft), 0 fehlgeschlagen.`).

Drei Eigenschaften, auf die es dabei ankommt:

* **Eine kaputte Datei bricht den Stapel nicht ab.** Sie wird auf stderr
  gemeldet (auch mit `--quiet`), der Rest läuft weiter, und der Rückgabewert
  ist am Ende `1`.
* **Kein Ergebnis überschreibt ein anderes.** Weil die Ausgabe neben ihrer
  Eingabe entsteht, kommen sich zwei gleichnamige Dateien aus verschiedenen
  Verzeichnissen nicht ins Gehege. Deshalb sind die Schalter mit *einem*
  festen Ziel im Stapelbetrieb verboten: `-o`, `--review-out`, `--audit-log`
  und `--apply-review`.
* **Ein zweiter Lauf über dasselbe Verzeichnis kaskadiert nicht.** Beim
  Auflösen eines Verzeichnisses werden Dateien mit dem Namenszusatz
  übergangen — `auszug_geschwaerzt.pdf` wird nicht zu
  `auszug_geschwaerzt_geschwaerzt.pdf`. Ausdrücklich genannte Dateien werden
  nie übergangen.

Mit `--json` kommt statt der Zusammenfassung eine Liste, in der auch die
gescheiterten Dateien mit ihrem Fehler stehen.

<a id="einstellungsdatei"></a>
### Einstellungsdatei

Was man nicht bei jedem Aufruf tippen will, steht in einer kleinen YAML-Datei:

| System | Pfad |
|---|---|
| Linux, BSD | `$XDG_CONFIG_HOME/redact-rs/settings.yaml`, sonst `~/.config/redact-rs/settings.yaml` |
| macOS | `~/.config/redact-rs/settings.yaml` |
| Windows | `%APPDATA%\redact-rs\settings.yaml` |

`REDACT_RS_CONFIG` zeigt auf eine andere Datei und schlägt alles davon.

```yaml
# Alle Schlüssel sind freiwillig; was fehlt, behält seine Vorgabe.
output_suffix: _anonym        # Namenszusatz          (Vorgabe: _geschwaerzt)
patterns: [iban_de, bic]      # Standard-Muster       (Vorgabe: die eingebaute Auswahl)
disabled_patterns: [date_de]  # dauerhaft abgeschaltet (Vorgabe: keines)
min_confidence: 0.4           # Mindestvertrauen      (Vorgabe: 0.5)
padding: 2.0                  # Polsterung in Punkt   (Vorgabe: 1.0)
theme: dunkel                 # Thema der Oberfläche  (hell | dunkel)
```

**`no_patterns` gibt es hier bewusst nicht.** „Alle automatischen Funde aus“ ist
die weitreichendste Einstellung dieses Werkzeugs, und sie wäre aus einer Datei
heraus **nicht mehr zu widerrufen**: `--no-patterns` ist ein Schalter ohne
Gegenstück, es gibt kein `--patterns-an`. Stünde er in der Datei, liefe jeder
Aufruf ohne Erkennung, und die Kommandozeile hätte kein Mittel dagegen.
`disabled_patterns` ist etwas anderes: eine Liste auf der Kommandozeile
**ersetzt** die aus der Datei, die Angabe bleibt also widerrufbar.

**Rangfolge: Kommandozeile schlägt Datei schlägt Vorgabe.** Ein Schalter, der
nicht angegeben wurde, überschreibt die Datei nicht — das ist der Grund, warum
`--output-suffix`, `--padding` und `--min-confidence` in der Hilfe keinen
Standardwert mehr anzeigen. Geprüft wird die Reihenfolge in
`crates/redact-cli/tests/settings.rs`.

Ein unbekannter Schlüssel (Tippfehler) und ein unbekanntes Thema beenden den
Lauf mit einer Meldung. Eine Einstellung, die stillschweigend nicht wirkt, wäre
das schlechtere Verhalten. Eine **fehlende** Datei ist dagegen kein Fehler.

**Gelesen wird die Datei nur von `redact-rs` (bzw. `redact-rs.exe`).** Dort
wird sie beim Bauen von `redact_pipeline::Config` angewendet — und weil
`redact-rs --gui` dieselbe `Config` an die Oberfläche weiterreicht, gelten
Namenszusatz, Muster, Schwelle, Polsterung und Thema auch dort.

Die Fensterfassung zum Doppelklicken (`redact-rs-gui.exe`) liest sie
**nicht**: sie baut ein `Config::default()` und ruft `Settings::load()` nie
auf (`crates/redact-cli/src/bin/redact-rs-gui.rs`; dasselbe gilt für das
Entwickler-Binary `redact-gui`). Wer per Doppelklick arbeitet und trotzdem
seine Einstellungen will, startet über die Konsolenfassung:

```bash
redact-rs --gui kontoauszug.pdf
```

### Eingebaute Muster

Die Spalten „mit Kt.“ und „ohne“ sind die Konfidenz **mit** bzw. **ohne**
passendes Schlüsselwort daneben. Alles unterhalb von `--min-confidence`
(Vorgabe **0.50**) wird verworfen — ein Muster, dessen Wert in der Spalte
„ohne“ darunter liegt, findet also nichts, solange kein Schlüsselwort
danebensteht. Genau dieselbe Tabelle liefert `redact-rs --list-patterns`.

| ID | Beschreibung | mit Kt. | ohne | Standard |
|----|--------------|---------|------|----------|
| `iban_de` | Deutsche IBAN (DE + 2 Prüfziffern + 18 Ziffern), mod-97-geprüft | 0.95 | – | **an** |
| `iban_intl` | Internationale IBAN (zu unspezifisch) | 0.90 | – | aus |
| `glaeubiger_id` | SEPA-Gläubiger-ID (Creditor Identifier), mod-97-geprüft | 0.95 | – | **an** |
| `konto_nr` | Kontonummer (6–10 Ziffern nach „Kto.“ o. ä.) | 0.85 | 0.30 | **an** |
| `blz` | Bankleitzahl (8 Ziffern nach „BLZ“/„Bankleitzahl“) | 0.80 | 0.25 | **an** |
| `bic` | BIC/SWIFT mit Länderkennung-Prüfung | 0.80 | – | **an** |
| `amount_eur` | Geldbetrag in Euro | 0.70 | – | aus |
| `date_de` | Datum TT.MM.JJJJ | 0.60 | – | aus |
| `credit_card` | Kreditkartennummer (13–19 Ziffern), Luhn-geprüft | 0.90 | – | **an** |
| `steuer_id` | Steuerliche Identifikationsnummer (11 Ziffern nach „Steuer-ID“) | 0.90 | 0.25 | **an** |
| `email` | E-Mail-Adresse | 0.90 | – | **an** |
| `phone_de` | Deutsche Telefonnummer | 0.85 | 0.35 | **an** |

Muster mit Prüfsumme (IBAN, Gläubiger-ID, BIC, Kreditkarte) verwerfen Treffer,
die die Prüfung nicht bestehen — das drückt die Fehlalarmquote deutlich.
Ausgeschaltete Muster lassen sich mit `--patterns amount_eur` gezielt aktivieren.

**Was sich an den Vorgaben geändert hat** — und warum:

* `date_de` und `amount_eur` sind **aus**. Datum und Betrag sind der *Inhalt*
  eines Kontoauszugs, nicht sein Schutzgut; auf einem Kontoauszug war praktisch
  jeder Treffer ein Fehltreffer. Wer sie braucht (etwa ein Gehaltsdatum),
  schaltet sie mit `--patterns date_de` gezielt ein.
* `steuer_id` ist **an**. Die Steuer-ID *ist* Schutzgut, und sie ist eindeutiger
  als vieles andere auf dem Papier.
* `konto_nr` und `blz` entscheiden nicht mehr über die **Ziffernlänge**, sondern
  über den **Kontext**. Eine nackte achtstellige Zahl ist kein Schutzgut — sie
  bekommt 0.30 bzw. 0.25 und fällt damit unter die Schwelle. Steht „Kto.“ oder
  „BLZ“ daneben, steigt sie auf 0.85 bzw. 0.80. Wer auch die Verdachtsfälle
  sehen will, senkt die Schwelle: `--min-confidence 0.25`.

**Kein Muster erkennt Namen.** Für Kontoinhaber, Empfänger, Arbeitgeber und
Ähnliches gibt es nur die [Buchungsliste](#buchungsliste), die
[manuellen Regionen](#manuelle-regionen) oder die GUI.

### Automatische Funde abschalten

Manchmal will man die Vorschläge des Werkzeugs nicht. Ein Muster, das in *dieser*
Aktenlage nur Fehltreffer erzeugt; ein Dokument, an dem allein von Hand
geschwärzt werden soll. Beides geht — ganz und einzeln, auf der Kommandozeile
und in der Oberfläche:

```bash
# Gar keine automatische Suche: geschwärzt wird nur, was von Hand
# gezogen oder über die Buchungsliste angegeben ist.
redact-rs auszug.pdf -o out.pdf --no-patterns --manual-regions regionen.json

# Ein einzelnes Muster heraus, die übrigen laufen weiter.
redact-rs auszug.pdf -o out.pdf --disable-pattern konto_nr

# Mehrere, kommagetrennt oder mehrfach angegeben.
redact-rs auszug.pdf -o out.pdf --disable-pattern konto_nr,phone_de
```

Gemeint sind die Muster, die der Lauf **wirklich anwendet** — also die Spalte
„Standard: **an**“ der [Tabelle oben](#eingebaute-muster) bzw. das, was
`--patterns` ausgewählt hat. Ein ohnehin ausgeschaltetes Muster (`date_de`,
`amount_eur`, `iban_intl`) braucht kein `--disable-pattern`; abgelehnt wird es
trotzdem nicht, denn ein bekannter Name ist ein bekannter Name.

Wie sich die Schalter zueinander verhalten, am Demo-Kontoauszug nachgemessen
(er ergibt mit den Vorgabemustern 7 Treffer):

| Aufruf | Treffer | Ansage |
|---|---|---|
| `--disable-pattern konto_nr` | 6 | `… 1 Muster abgeschaltet (konto_nr).` |
| `--disable-pattern konto_nr,phone_de` | 5 | `… 2 Muster abgeschaltet (konto_nr, phone_de).` |
| `--disable-pattern date_de` (war ohnehin aus) | 7 | `… 1 Muster abgeschaltet (date_de).` |
| `--patterns iban_de` | 2 | keine |
| `--patterns iban_de --disable-pattern bic` | 2 | `… 1 Muster abgeschaltet (bic).` |
| `--patterns iban_de --disable-pattern iban_de` | 0 | `… 1 Muster abgeschaltet (iban_de).` |
| `--no-patterns --disable-pattern bic` | 0 | `… abgeschaltet (--no-patterns).` |
| `--disable-pattern iban` | — | Abbruch, Rückgabewert 2 |

Zwei Zeilen der Tabelle überraschen und sind deshalb Absicht:

* **`--disable-pattern date_de`** ändert nichts an der Trefferzahl — das Muster
  war ohnehin aus — und wird trotzdem angesagt. Die Ansage berichtet, was
  *angeordnet* wurde, nicht was gewirkt hat; das ist die Aussage, die eine
  Prüferin braucht.
* **`--no-patterns` schlägt `--disable-pattern`.** Die Ansage nennt dann nur
  noch das Gröbere: bei „alles aus“ ist die Zahl der einzeln abgeschalteten
  Muster gegenstandslos.

In der Oberfläche steht der Schalter über der Trefferliste: das Häkchen
**„Automatisch suchen“** für alles, darunter aufklappbar **„Muster einzeln“**
mit einem Kästchen je Muster dieses Laufs. Die Liste ist zugeklappt, solange
nichts abgeschaltet ist, und offen, sobald etwas aus ist — eine Abschaltung
soll man sehen, ohne danach zu suchen.

Beim Umschalten wird die Trefferliste **neu gerechnet**. Das kostet: jede
Abwahl, jede je Treffer gewählte Schwärzungsart und ein geladenes Review sind
danach weg. Erhalten bleiben die selbst gezogenen Rechtecke und die
Schutzmarken der Buchungsliste (die steht in der Konfiguration und wird nicht
angefasst). Deshalb kommt vorher dieselbe Rückfrage wie bei „Analysieren“ — und
sagt man dort „nein“, springt auch das Kästchen zurück: die Seitenleiste ändert
nichts selbst, sie meldet nur den Wunsch.

**Ein unbekannter Name ist ein Bedienfehler**, kein Achselzucken:

```console
$ redact-rs auszug.pdf -o out.pdf --disable-pattern iban
Fehler: Konfigurationsfehler: --disable-pattern: „iban“ ist kein bekanntes
Muster. Gültig sind: iban_de, iban_intl, glaeubiger_id, konto_nr, blz, bic,
amount_eur, date_de, credit_card, steuer_id, email, phone_de.
(`redact-rs --list-patterns` zeigt sie mit Beschreibung.) Es wurde nichts
abgeschaltet und nichts geschwärzt: ein übergangener Name sähe aus wie eine
Abschaltung und wäre keine.
$ echo $?
2
```

Der Lauf endet also **vor** der ersten Schwärzung; es entsteht keine
Ausgabedatei. Ein stillschweigend übergangener Tippfehler ergäbe dagegen eine
Datei, die anders ist als erwartet, und niemand erführe warum.

#### Der Zustand ist sichtbar — das ist der Punkt

Eine Datei, die mit abgeschalteter Erkennung entstanden ist, sieht in jeder
Zahl aus wie eine vollständig geprüfte: „0 Treffer“ heißt dort nicht „nichts
gefunden“, sondern „nicht gesucht“. Deshalb steht die Abschaltung an **drei**
Stellen, und alle drei speisen sich aus derselben Angabe:

* in der Zusammenfassung auf stdout und als Warnung auf stderr —
  `Automatische Erkennung: abgeschaltet (--no-patterns). …`;
* in der Oberfläche in der Kopfzeile der Trefferliste und als Satz in
  Warnfarbe unter dem Schalter. Ganz aus:
  `Automatische Suche AUS — nicht gesucht, nur von Hand: 2 Treffer · 2 werden
  geschwärzt`. Einzelne Muster aus:
  `6 Treffer · 6 werden geschwärzt · 1 Muster abgeschaltet` — die Zahl steht
  dann hinter den Trefferzahlen, weil sie sie ergänzt statt sie umzudeuten.
  Ist noch kein Dokument geladen, sagt die Statuszeile den Zustand trotzdem
  (`Automatische Erkennung: alle Muster an` bzw. der jeweilige Satz);
* im **Audit-Log** als eigenes Feld, das auch dann dasteht, wenn nichts
  abgeschaltet war:

```json
"patterns": { "all_disabled": false, "disabled": ["konto_nr"] }
```

Zusätzlich ist „Analysieren“ in der Oberfläche ausgegraut, solange die Analyse
nachweislich nichts finden könnte (kein Muster, keine Buchungsliste, keine
Regionsdatei) — mit dem Grund in der Sprechblase. Ein Knopf, der eine leere
Trefferliste hinterlässt, läse sich sonst als „nichts gefunden“.

Was **nicht** geschieht: der Rückgabewert bleibt `0`. Eine abgeschaltete
Erkennung ist eine Anweisung des Aufrufenden und kein Befund an der Datei — die
Analyse hat das Dokument vollständig gelesen und auf Geheiß nach weniger
gesucht. Rückgabewert `3` ist der Frage „hat das Werkzeug alles *gesehen*?“
vorbehalten (siehe [Kommandozeile](#kommandozeile)); spränge er auch bei jedem
`--no-patterns` an, wäre er für die Fälle wertlos, für die es ihn gibt. Die
Begründung im Einzelnen steht in `crates/redact-pipeline/src/coverage.rs`.

## Buchungsliste

CSV mit Kopfzeile. `context_before`/`context_after` sind optional und
verifizieren einen Treffer anhand seines Umfelds.

```csv
id,list_type,pattern,context_before,context_after,is_regex
b001,positive,"Musterfirma GmbH","Überweisung an",,false
b002,positive,"DE89 3704 0044 0532 0130 00",,,false
b003,negative,"Max Mustermann",,,false
```

Regeln:

* Ein Treffer der **Negativliste blockiert** jede Schwärzung, die zu mindestens
  50 % **innerhalb** des Negativtreffers liegt — auch wenn ein Muster oder die
  Positivliste anschlägt. (Der Negativtreffer wird dafür vorher um 2 pt
  vergrößert.) Maßgeblich ist der Flächenanteil des *Kandidaten*, nicht der des
  Negativtreffers.
* Ein Treffer der **Positivliste erzwingt** eine Schwärzung, auch ohne Muster.
* **Manuelle Regionen** überstimmen die Negativliste: eine bewusste
  Nutzerentscheidung wird nicht automatisch verworfen.
* Der Vergleich ignoriert Groß-/Kleinschreibung und die *Menge* an Leerraum —
  eine IBAN wird also auch gefunden, wenn im PDF mehrere Leerzeichen zwischen
  den Blöcken stehen.
* **Leerraum wird zusammengefasst, nicht entfernt.** Der Eintrag
  `DE89 3704 0044 0532 0130 00` trifft *nicht* auf `DE89370400440532013000` im
  PDF (nachgemessen: `Textzeilen: 1, Treffer gesamt: 0`). Wer beide
  Schreibweisen abdecken will, nimmt zwei Einträge — oder das Muster `iban_de`,
  das die ungruppierte Form von sich aus erkennt.
* **Nicht über einen Zeilenumbruch hinweg.** Abgeglichen wird immer gegen eine
  einzelne extrahierte Textzeile. Steht eine IBAN im PDF über zwei Zeilen
  verteilt, sind das zwei getrennte Textzeilen, und der Eintrag trifft nicht.
  Für solche Fälle bleibt nur eine manuelle Region bzw. die GUI.
* `is_regex = true` wird nur für metazeichenfreie Muster als Literal-Suche
  akzeptiert; echte reguläre Ausdrücke gehören in `--patterns-config`.

Vollständiges Beispiel: [`examples/booking_list.csv`](examples/booking_list.csv).

## Manuelle Regionen

Koordinaten im PDF-User-Space (Punkt = 1/72 Zoll, Ursprung links unten),
Seitennummern 0-basiert:

```json
[
  {
    "page": 0,
    "rect": { "ll": { "x": 60.0, "y": 745.0 }, "ur": { "x": 300.0, "y": 760.0 } },
    "text": null,
    "source": { "manual": { "reason": "Kontoinhaber" } }
  }
]
```

```bash
redact-rs eingabe.pdf -o ausgabe.pdf --manual-regions examples/manual_regions.json
```

## Eigene Patterns

```yaml
extend_builtins: true
patterns:
  - id: date_de
    enabled: false            # eingebautes Muster abschalten
  - id: kundennummer          # eigenes Muster ergänzen
    regex: 'KdNr\.?\s*\d{5,}'
    confidence: 0.85
```

```bash
redact-rs eingabe.pdf -o ausgabe.pdf --patterns-config examples/patterns.yaml
```

Es kommt [`fancy-regex`](https://docs.rs/fancy-regex) zum Einsatz, Look-around
(`(?<!…)`, `(?!…)`) ist also erlaubt. Auch hier gilt: gesucht wird je
extrahierter Textzeile, ein Muster kann keinen Zeilenumbruch überspannen.

<a id="bilder"></a>

## Bilder werden wirklich geschwärzt

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
                          Datei dadurch größer)
```

Die Zeile „ohne Textfund“ ist hier der Normalfall und kein Mangel: in einem
Rasterbild steht für die Analyse kein Text, zu entfernen gibt es also nichts —
überschrieben werden die Bildpunkte. Der Befund heißt im Audit-Log `covered`,
siehe [`effect` je Region](#befund).

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

### Der Preis: die Datei ist nicht mehr bitgleich

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
was vorher dort stand. Die Zusammenfassung sagt trotzdem pauschal „Datei
dadurch größer“; gemeint ist der Regelfall.

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

### Nicht dekodierbare Bilder brechen den Lauf ab

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

## Review-Workflow

Der empfohlene Weg für alles, was das Haus verlässt: erst sehen, was das
Werkzeug schwärzen *würde*, dann anwenden.

```bash
# 1. Analysieren
redact-rs kontoauszug.pdf --review --review-out review.json \
    --patterns iban_de,konto_nr --booking-list buchungen.csv

# 2. review.json im Editor prüfen: "enabled": false setzt einen Treffer ab
#    (oder in der GUI per Checkbox)

# 3. Anwenden
redact-rs kontoauszug.pdf -o geschwaerzt.pdf \
    --apply-review review.json --audit-log audit.json
```

> ### ⚠ `review.json` ist selbst ein Geheimnisträger
>
> Die Review-Datei enthält **jeden gefundenen Treffer im Klartext** — im Feld
> `text` steht die IBAN, der Name, die Telefonnummer, so wie sie im Original
> stehen:
>
> ```json
> "region": { "page": 0, "rect": { … }, "text": "DE89 3704 0044 0532 0130 00", … }
> ```
>
> Ohne `--review-out` liegt sie als `<eingabe>_review.json` **direkt neben dem
> Original**, also mit hoher Wahrscheinlichkeit in genau dem Ordner, der gleich
> weitergegeben, synchronisiert oder gesichert wird. Unter Unix wird sie mit
> Modus `0600` angelegt; das schützt vor anderen Konten auf derselben Maschine,
> nicht vor einem Cloud-Ordner und nicht vor einem Mailanhang.
>
> **Sie darf das Haus nicht verlassen.** Nach dem Anwenden löschen. Wer sie
> aufbewahren muss (Nachvollziehbarkeit), legt sie dorthin, wo auch das
> ungeschwärzte Original liegen darf.

Die Review-Datei trägt den SHA-256 der Eingabe. Wird sie auf ein anderes
Dokument angewendet, bricht der Lauf mit einer Fehlermeldung ab — Koordinaten
aus einer fremden Datei würden sonst an falscher Stelle schwärzen.

Eine **leere** Prüfsumme wurde früher stillschweigend durchgewinkt — wer
`"sha256": ""` von Hand eintrug, umging die Sperre damit vollständig. Sie wird
jetzt abgelehnt; `--allow-unverified-review` ist der ausdrückliche Weg daran
vorbei. Eine *falsche* Prüfsumme bleibt auch mit diesem Schalter abgelehnt:
„ungeprüft“ ist etwas anderes als „nachweislich fremd“.

Dieselbe Prüfung gilt für eine Review-Datei hinter `--manual-regions`. Der
Schalter nimmt beide Formate an, und bis v0.2.0 war er damit der Weg an der
Sperre vorbei: dieselbe Datei, die `--apply-review` mit Exit 2 zurückwies, ging
hier wortlos durch. Ein nacktes Regions-Array (siehe unten) bleibt ungeprüft —
es nennt keine Herkunft und behauptet auch keine.

## Audit-Log

```json
{
  "timestamp": "2026-08-01T09:04:22Z",
  "tool": { "name": "redact-rs", "version": "0.3.0" },
  "input":  { "path": "kontoauszug.pdf", "sha256": "466c0af4…" },
  "output": { "path": "geschwaerzt.pdf", "sha256": "c2c54724…" },
  "redactions": [
    {
      "page": 0,
      "rect":           { "ll": { "x": 100.9, "y": 732.8 }, "ur": { "x": 239.9, "y": 742.5 } },
      "effective_rect": { "ll": { "x":  99.9, "y": 731.8 }, "ur": { "x": 240.9, "y": 743.5 } },
      "effect": "applied",
      "removed_glyphs": 22,
      "action": "blackout",
      "reason": "booking: b002 (positive)",
      "source": "booking_list"
    }
  ],
  "blocked_by_negative_list": [
    { "page": 1, "pattern": "Max Mustermann", "booking_id": "b003" }
  ],
  "metadata_stripped": true,
  "metadata": {
    "info": true, "xmp": false, "piece_info": 0, "struct_tree": false,
    "names": 0, "embedded_files": 0, "javascript": 0,
    "acroform": true, "xfa": true,
    "field_values": 2, "file_attachments": 1, "open_action": true,
    "additional_actions": 2, "optional_content": true,
    "summary": ["/Info-Dictionary", "Formulardefinition (/AcroForm)", "…"]
  },
  "effect": {
    "padding": 1.0, "pages": 2, "requested": 4,
    "applied": 4, "covered": 0, "degenerate": 0, "missing_page": 0,
    "removed_glyphs": 66, "drawn_rects": 4, "removed_annotations": 0,
    "redacted_images": 0, "copied_images": 0
  },
  "patterns": { "all_disabled": false, "disabled": [] }
}
```

`rect` ist das gefundene Rechteck, `effective_rect` dasselbe zuzüglich
`--padding`. `metadata` ist der **gemessene** Bericht darüber, was tatsächlich
entfernt wurde (nicht, was vorgesehen war); `effect` fasst den Lauf in Zahlen
zusammen — `redacted_images` und `copied_images` beziffern die
[Bildschwärzung](#bilder).

`patterns` sagt, wonach der Lauf **nicht** gesucht hat: `all_disabled` für
`--no-patterns`, `disabled` für jedes einzeln abgeschaltete Muster (siehe
[Automatische Funde abschalten](#automatische-funde-abschalten)). Das Feld steht
auch dann da, wenn nichts abgeschaltet war — „nichts abgeschaltet“ ist die
Aussage, auf die sich ein Prüfer verlassen können muss, und ein Feld, das nur
im Ausnahmefall erschiene, machte ein Log mit abgeschalteter Erkennung
ununterscheidbar von einem Log aus einer älteren Fassung. Denselben Sachverhalt
trägt zusätzlich ein Satz in `warnings`: einmal für Maschinen, einmal für
Menschen.

<a id="befund"></a>

### `effect` je Region: was wirklich passiert ist

`"effect"` an einem Eintrag ist **gemessen**, nicht aus dem Rechteck
geschlossen. `removed_glyphs` daneben nennt die Zeichen, die genau diese Region
aus dem Content-Stream entfernt hat. Vier Befunde sind möglich:

| Befund | Was geschah | Ist das in Ordnung? |
|---|---|---|
| `applied` | Mindestens ein Zeichen entfernt. | Ja — und nur das bezeugt eine Schwärzung. |
| `covered` | Deck-Rechteck gezeichnet, aber kein Zeichen getroffen. | **Kommt darauf an.** Über einer Grafik oder einem Rasterbild steht kein Text; die Bildpunkte werden trotzdem überschrieben. Liegen die Koordinaten dagegen daneben, bleibt der Text darunter lesbar. |
| `degenerate` | Rechteck ist nach `--padding` leer, die Region wurde übersprungen. | Nein. Ein negatives `--padding` verkleinert jeden Bereich. |
| `missing_page` | Die genannte Seite gibt es im Dokument nicht. Es geschah **gar nichts**. | Nein. Fast immer die verwechselte Zählweise — siehe unten. |

Die Summen dazu stehen unter `effect`: `requested` ist die Zahl der geplanten
Regionen, und `applied + covered + degenerate + missing_page` ergibt sie
wieder. `pages` nennt die Seitenzahl des Dokuments, damit sich `missing_page`
nachprüfen lässt; `missing_pages` listet die angesprochenen Seiten (0-basiert)
und steht nur dann im Log, wenn es welche gab.

Jeder Befund außer `applied` steht auch in der Zusammenfassung auf der Konsole
und als Warnung auf stderr — ein Lauf, der nichts entfernt hat, endet nicht
mehr wortlos mit „Schwärzungen: 3“.

Nachgemessen an einem dreiseitigen Dokument mit drei Regionen, von denen eine
`"page": 3` nennt:

```console
$ redact-rs drei.pdf -o out.pdf --no-patterns --manual-regions regionen.json
Seiten:             3
Textzeilen:         3
Treffer gesamt:     3
Automatische Erkennung: abgeschaltet (--no-patterns). …
Schwärzungen:       3
  davon wirksam:      2 (Zeichen entfernt)
  davon wirkungslos:  1 (Seite gibt es in diesem Dokument nicht)
Entfernte Zeichen:  82
Deck-Rechtecke:     2
…
Warnung: 1 von 3 Schwärzung(en) liegen auf einer Seite, die es in diesem
Dokument nicht gibt (Seite 4; das Dokument hat 3 Seite(n)). Dort wurde nichts
entfernt und nichts überdeckt — der Text steht unverändert in der Ausgabe.
Häufigste Ursache ist die Zählweise: in JSON ist die erste Seite „page“: 0, die
letzte also 2.
$ echo $?
0
```

Der Rückgabewert bleibt hier **`0`**: die Analyse hat das Dokument vollständig
gelesen: eine Region, die ins Leere zeigt, ist ein Fehler in der *Eingabe* des
Nutzers und keine Stelle, die das Werkzeug nicht durchsuchen konnte. Genau
deshalb steht der Befund in der Zusammenfassung, auf stderr **und** als
`missing_page` im Audit-Log.

### `page` zählt überall gleich

**In jeder JSON-Datei ist die erste Seite `0`** — in `review.json`, im
Audit-Log und in der `--manual-regions`-Eingabe. Nur im Fließtext, also in der
Konsolenausgabe und in der Oberfläche, heißt dieselbe Seite „Seite 1“.

Das war bis v0.2.0 nicht so: das Audit-Log zählte als einziges ab 1. Wer
`review.json` und `audit.json` nebeneinander legte — und genau dazu lädt der
Workflow ein —, sah denselben Treffer einmal als `"page": 0` und einmal als
`"page": 1`. Eine Seitenzahl aus dem Log in eine Regionsdatei zu übernehmen
ging damit still daneben. Diese Stolperfalle gibt es nicht mehr.

Wer trotzdem ab 1 zählt, erfährt es: eine Region auf einer Seite, die es nicht
gibt, wird nicht angefasst und im Log als `missing_page` geführt — mit einer
Warnung, die die gemeinte Seite im Klartext nennt. Vorher meldete derselbe Lauf
„Schwärzungen: 3“ und `"effect": "applied"` für alle drei, obwohl eine davon
nirgendwo lag und die erste Seite unangetastet blieb.

### Was im Audit-Log im Klartext steht

* **Nicht** der geschwärzte Text selbst. Ein Eintrag unter `redactions` nennt
  Seite, Rechteck, Aktion und die Herkunft (`pattern: iban_de (confidence 0.99)`
  bzw. `booking: b002 (positive)`), nicht den Fund.
* **Wohl** der Text der Negativlisten-Einträge: unter
  `blocked_by_negative_list` steht das Suchmuster wörtlich (`"pattern": "Max
  Mustermann"`). Genau das sind die Werte, die man *nicht* schwärzen wollte —
  aber es sind trotzdem Personendaten.
* Bei manuellen Regionen der eingegebene Grund (`"reason"`), so wie er
  formuliert wurde.

Das Audit-Log wird deshalb ebenfalls mit Modus `0600` angelegt. Zusammen mit
dem Original erlaubt es außerdem, jede Schwärzung punktgenau zu lokalisieren.

<a id="pruefen"></a>

## Prüfen, ob die Schwärzung gewirkt hat

### `pdftotext … | grep …` taugt dafür nicht

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

   | Geheimnis versteckt in … | `pdftotext` | `strings` | `redact_pdf::leaks` |
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
   [Der Textspiegel im Seiteninhalt](#der-textspiegel-im-seiteninhalt).

### Die Prüfung, die das Werkzeug mitbringt

`redact_pdf::leaks(bytes, needle)` (in
[`crates/redact-pdf/src/audit_bytes.rs`](crates/redact-pdf/src/audit_bytes.rs))
sucht auf allen Ebenen, auf denen ein Geheimnis überleben kann: rohe
Dateibytes, jeder roh gefundene `stream … endstream`-Block (auch
Flate-dekomprimiert, also inklusive Altrevisionen), jedes Stream-Objekt des
Objektgraphen dekodiert, die Objekte in `/ObjStm`-Containern und **jedes**
Zeichenketten-Objekt unter jedem Schlüssel — jeweils in UTF-8, Latin-1/PDFDoc,
UTF-16BE und als Hex-String. Im Zweifel meldet es zu viel.

Eine eigene Unterkommando-Schnittstelle dafür gibt es (noch) nicht; `leaks` ist
eine Bibliotheksfunktion. So wird sie benutzt:

```bash
cargo new --bin pdf-leck-pruefen && cd pdf-leck-pruefen
cargo add --path /pfad/zu/redactrs/crates/redact-pdf
```

```rust
// src/main.rs
fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let pdf = args.next().expect("Aufruf: pdf-leck-pruefen <ausgabe.pdf> <text>…");
    let bytes = std::fs::read(&pdf)?;
    let mut leck = false;
    for needle in args {
        let hits = redact_pdf::leaks(&bytes, &needle);
        if hits.is_empty() {
            println!("sauber: {needle}");
        } else {
            leck = true;
            println!("LECK ({}x): {needle}", hits.len());
            for h in &hits {
                println!("    {h}");
            }
        }
    }
    std::process::exit(if leck { 1 } else { 0 });
}
```

Nachgemessen an der Ausgabe aus dem [Schnellstart](#schnellstart), die mit
`--patterns iban_de,bic,email` entstanden ist:

```console
$ cargo run --release -- ../kontoauszug_geschwaerzt.pdf \
      "DE89 3704 0044 0532 0130 00" "Max Mustermann" "532013000"
sauber: DE89 3704 0044 0532 0130 00
LECK (6x): Max Mustermann
    Rohdaten-Stream @0x1e7 (inflate) [Inhalt, UTF-8/ASCII]: …(Kontoinhaber: Max Mustermann) Tj…
    Objekt 11 0 <Stream, lopdf-dekodiert> [Zeichenketten-Verkettung]: …Kontoinhaber: Max Mustermann…
    …
LECK (4x): 532013000
    …
$ echo $?
1
```

Der Rückgabewert ist `1`, sobald irgendetwas gefunden wurde, und `0` sonst
(beides nachgemessen) — das Programm eignet sich damit als Kontrollschritt in
einem Skript.

**Und die Gegenprobe**, die dem Ergebnis erst seinen Wert gibt: dieselbe Datei
ohne `--patterns`, also mit den Vorgabemustern, gegen alle fünf Werte geprüft:

```console
$ cargo run --release -- ../std.pdf \
      "DE89 3704 0044 0532 0130 00" "532013000" "+49 30 123456789" \
      "12345678901" "Max Mustermann"
sauber: DE89 3704 0044 0532 0130 00
sauber: 532013000
sauber: +49 30 123456789
sauber: 12345678901
LECK (6x): Max Mustermann
```

IBAN, Kontonummer, Telefonnummer und Steuer-ID sind restlos weg — auf allen
Ebenen, auf denen `leaks` sucht, nicht nur dort, wo `pdftotext` hinsieht. Übrig
ist der **Name**, und zwar genau so oft wie vorher: für Namen gibt es kein
Muster, und niemand hat ihn markiert. Das ist keine Schwäche der Prüfung,
sondern ihr Ergebnis.

### Und wogegen prüft man?

`leaks` beantwortet die Frage „steht *dieser* Text noch in der Datei?“. Die
Liste der Suchbegriffe muss also von Hand entstehen: die Werte aus dem Original,
die nicht nach draußen dürfen. Zwei brauchbare Quellen dafür:

* das Feld `text` jedes Eintrags in `review.json` (dort steht genau das, was das
  Werkzeug gefunden hat) — und dazu die Werte, die dort **fehlen**;
* eine Sichtprüfung der Originalseiten.

Das ersetzt die Sichtprüfung des Ergebnisses nicht. `leaks` beweist, dass eine
bekannte Zeichenkette weg ist. Dass nichts *Unbekanntes* stehen geblieben ist,
kann es nicht beweisen.

### „0 Schwärzungen“ ist kein Freibrief

Meldet ein Lauf `Schwärzungen: 0`, kann das heißen: in der Datei steht nichts
Schützenswertes. Es kann genauso heißen: der Text steckt in einem Rasterbild,
die Schrift benutzt eine Kodierung, die der Extraktor nicht auflösen konnte, das
Muster passt auf diese Schreibweise nicht, oder die Negativliste hat alles
blockiert. Siehe auch den gleichnamigen Abschnitt in [`SECURITY.md`](SECURITY.md).

Für einen Teil dieser Fälle gibt es inzwischen ein Signal: Stellen, die die
Analyse nicht durchsuchen **konnte**, stehen auf stderr mit `NICHT GEPRÜFT` und
setzen den Rückgabewert auf `3`. Das deckt längst nicht alles ab — ein Muster,
das schlicht nicht passt, merkt niemand, und ein Rasterbild zählt bewusst nicht
dazu —, aber es macht einen Teil der stillen Fälle laut.

<a id="grenzen"></a>

## Was dieses Werkzeug nicht leistet

Die Liste ist bewusst ausführlich und unfreundlich. Jeder Punkt ist am Code oder
an einem Lauf belegt; wo **[Testfall]** steht, hält ein Test im Repository genau
dieses Verhalten fest. Vollständigkeit lässt sich nicht zusichern — was hier
nicht steht, ist deshalb nicht automatisch abgedeckt.

### Erkennung

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

### Gescannte Dokumente

* **Keine OCR.** Steht die sensible Information als Pixel in einem Rasterbild,
  **findet** die Analyse sie nicht. Sie muss von Hand gezogen werden — in der
  GUI mit der Maus oder über `--manual-regions`. Was gezogen wurde, wird dann
  aber auch wirklich entfernt (siehe [Bilder](#bilder)); das ist der
  Unterschied zwischen „nicht gefunden“ und „nicht geschwärzt“.
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
  ([Details](#bilder)).

### Bekannte Lecks in der Schwärzung

Von den vier Lecks, die hier früher standen, sind **drei behoben**. Übrig ist
eines:

* **Erscheinungsströme außerhalb des Schwärzungsbereichs.** Annotationen, die
  in einen Schwärzungsbereich ragen, werden entfernt. Der Erscheinungsstrom
  (`/AP`) einer Annotation, die *nicht* hineinragt, wird nicht geprüft — ihr
  Text bleibt in der Datei. **[Testfall]**
  (`appearance_stream_outside_the_redaction_is_also_cleaned`, Aufgabe #26)

Dazu diese Grenzen, für die es keinen Testfall gibt:

* **Vektorgrafiken** werden nicht durchsucht — Text, der als Pfad gezeichnet
  ist, ist für die Analyse unsichtbar.
* **Eingebettete Dateien** werden nicht durchsucht. Sie werden allerdings
  **entfernt** (`/Names /EmbeddedFiles` und `/FileAttachment`-Annotationen),
  also nicht auf Inhalte geprüft, sondern samt Inhalt gelöscht.
* **Type3-Fonts** liefern kein Fontprogramm und werden nur genähert behandelt.

Der eine offene Testfall steht in
[`crates/redact-pdf/tests/known_leaks.rs`](crates/redact-pdf/tests/known_leaks.rs)
und trägt ein `#[ignore]` mit Aufgabennummer, damit die Suite grün bleibt und
der Defekt trotzdem dokumentiert ist. Nachstellen:

```bash
cargo test -p redact-pdf --no-default-features -- --ignored
# 0 passed; 1 failed  →  das Leck besteht weiterhin
```

Solange dieser Test fehlschlägt, besteht das Leck. Schlägt er plötzlich *nicht*
mehr fehl, ist es behoben und das `#[ignore]` kann weg. Dafür sorgt zusätzlich
ein **Kanarienvogel** (`canary_appearance_stream_outside_the_redaction_still_leaks`),
der ohne `#[ignore]` läuft und rot wird, sobald das Leck verschwindet.

**Behoben** (die Tests laufen jetzt ohne `#[ignore]` und sind grün — insgesamt
12 bestandene Tests in `known_leaks.rs`):

| früheres Leck | Test |
|---|---|
| Inline-Bilder zerreißen den Seiteninhalt | `text_after_an_inline_image_is_still_found_by_the_extractor`, `unrelated_text_after_an_inline_image_survives_the_rewrite`, `redacting_a_page_with_an_inline_image_removes_the_secret` |
| Formularfeld-Werte (`/V`) bleiben stehen | `form_field_value_is_redacted_too` |
| verwaiste Objekte werden mitgeschrieben | `objects_unpacked_from_an_object_stream_are_not_carried_over` |
| inkrementelle Vorversionen bleiben erhalten | `incremental_history_is_dropped_when_the_file_is_rewritten` |
| `/ActualText` eines Struktur-Elements spiegelt den geschwärzten Text | `struct_elem_actual_text_does_not_mirror_the_redacted_text` |
| **Textspiegel im Seiteninhalt** (`/ActualText`, `/Alt`, `/E` an einem Marked-Content-Abschnitt) überleben die Schwärzung | 9 Fälle in [`crates/redact-pdf/tests/marked_content.rs`](crates/redact-pdf/tests/marked_content.rs) — siehe unten |

#### Der Textspiegel im Seiteninhalt

Ein getaggtes PDF darf den Glyphen eines Abschnitts einen Ersatztext
beistellen: `/Span <</ActualText (DE89 …)>> BDC … EMC`. Der steht als Klartext
**im Content-Stream**, nicht in einem Objekt daneben. Die Glyphen wurden
korrekt entfernt, das Deck-Rechteck saß richtig — und `pdftotext` gab in der
Voreinstellung trotzdem die vollständige IBAN aus, weil es den Spiegel
bevorzugt.

Die drei Schlüssel `/ActualText`, `/Alt` und `/E`
(`MIRROR_KEYS` in `crates/redact-pdf/src/content.rs`) werden jetzt **geleert**,
sobald von den Glyphen darunter etwas entfernt wurde — nicht nur gefunden.
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

### Verarbeitung

* **Verschlüsselte PDFs** werden ohne Passwort abgelehnt. Mit `--password` bzw.
  `REDACT_RS_PASSWORD` werden sie entschlüsselt und normal verarbeitet — was
  dabei ungeprüft bleibt, steht in
  [`SECURITY.md`](SECURITY.md#passwörter-verschlüsselter-pdfs). Nicht jedes
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
  ([siehe oben](#stapel)), sie steigt aber **nicht** in Unterverzeichnisse ab.
* **Keine unbegrenzte Größe.** Sechs Grenzen greifen, jede mit einer eigenen
  Meldung und einem Rückgabewert statt eines Speicherfehlers:

  | Was | Vorgabe | Stellschraube |
  |---|---|---|
  | Größe der Eingabedatei | 512 MB | `--max-input-mb` |
  | entpackte Bytes über alle Streams | 1024 MB | `--max-decompressed-mb` |
  | davon: Streams, die geparst werden | 16 MB | `--max-parsed-mb` |
  | gleichzeitig gehaltene dekodierte Bildbytes | 256 MB | `--max-image-mb` |
  | Trefferkandidaten je Datei | 100 000 | `--max-candidates` (Exit 2) |
  | Zeichen, die **eine Seite** setzen darf | 1 000 000 | fest |

  Dazu ein **Aufwandskonto** gegen die Vervielfachung durch Form-XObjects: Eine
  Datei von 2 368 Byte hält jede Byte-Grenze ein und lässt trotzdem acht
  Form-XObjects einander so oft zeichnen, dass über zwei Millionen Durchläufe
  entstehen. Dagegen hilft keine Größengrenze. Das Konto zählt deshalb, was
  wirklich anfällt, und wächst mit dem Inhalt, den die Datei *mitbringt* —
  nicht mit dem, was sie daraus macht. Ist es leer, wird die Datei abgelehnt.

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
  [`SECURITY.md`](SECURITY.md).

### Was der Nutzer selbst tun muss

1. **Das Ergebnis ansehen.** Jede Seite, mit den Augen. Die Zahl der
   Schwärzungen ist keine Aussage über Vollständigkeit.
2. **Gegen die eigenen Werte prüfen** — siehe
   [Prüfen, ob die Schwärzung gewirkt hat](#pruefen).
3. **`review.json` und `audit.json` aufräumen.** Beide liegen standardmäßig
   neben dem Original und tragen Klartext (Details oben).
4. **Bei gescannten Seiten die Rechtecke selbst ziehen.** Ohne OCR *findet* das
   Werkzeug dort nichts; gezogene Bereiche werden aber wirklich aus den Pixeln
   entfernt ([Details](#bilder)). Die Verantwortung dafür, dass jede
   schützenswerte Stelle ein Rechteck bekommen hat, liegt beim Auge des
   Nutzers.
5. **Das Original getrennt aufbewahren.** Die Ausgabe entsteht neben der
   Eingabe; eine Verwechslung beim Verschicken ist der wahrscheinlichste Fehler
   überhaupt.

## Grafische Oberfläche

```bash
redact-rs                          # GUI ohne Dokument
redact-rs --gui kontoauszug.pdf    # GUI mit vorgeladenem PDF
```

Unter Windows genügt ein Doppelklick auf `redact-rs-gui.exe` — oder man zieht
ein PDF auf ihr Symbol. Die GUI (egui/eframe, ein einziges Binary ohne
zusätzliche Laufzeit) zeigt die Seiten so, wie `redact-render` sie rastert, mit
allen gefundenen Treffern als farbige Rahmen:
🔵 Muster · 🟢 Positivliste · 🔴 Negativliste (blockiert) · 🟠 manuell.

### Dokumente öffnen

Über „🗁 Öffnen“ (Strg+O) oder **per Drag & Drop** auf das Fenster. Bei mehreren
abgelegten Dateien gewinnt die erste PDF, Nicht-PDFs werden abgelehnt
(`classify_drop` in `crates/redact-gui/src/lib.rs` — eine reine Funktion,
damit das ohne Maus prüfbar ist). Gingen dabei von Hand gezogene Rechtecke
verloren, wird vorher gefragt.

Ist das geöffnete Dokument **verschlüsselt**, erscheint ein Fenster mit
verdeckter Eingabe. Das ist zugleich der bequemste Weg, ein Passwort *nicht*
über die Kommandozeile zu geben. Passt es nicht, bleibt die Frage stehen; das
falsche Passwort wird nicht behalten und steht in keiner Meldung.

### Rechtecke ziehen, verschieben und an den Ecken nachziehen

Ein neuer Bereich entsteht durch Aufziehen mit der Maus — sichtbar **ab dem
Bild des Drucks** und beginnend **am Druckpunkt**, nicht erst dort, wo egui den
Zug bemerkt. Ein Klick wählt ein vorhandenes Rechteck aus.

Ein ausgewähltes Rechteck trägt **vier Eckgriffe** (`Handle::TopLeft` …
`BottomRight` in `crates/redact-gui/src/selector.rs`). Daran lässt es sich
nachziehen: die gegenüberliegende Ecke bleibt stehen, der Mauszeiger zeigt die
Ziehrichtung an. Die Fangzone ist mit 16 Bildschirmpunkten bewusst größer als
das gezeichnete 5-pt-Quadrat — eine Zone in Zeichnungsgröße trifft man nur
zufällig.

Zieht man ein Rechteck über einen Treffer der **Negativliste**, überstimmt die
bewusste Handbewegung die Schutzliste — und die Oberfläche **sagt es** in der
Statuszeile, statt die Wirkung stillschweigend umzukehren. Strg+Z nimmt es
zurück.

### Rückgängig und Wiederholen

**Strg+Z** und **Strg+Y**, auch über die Knöpfe ↺ / ↻ in der Leiste. Der
Verlauf hält bis zu **50 Schnappschüsse** je Richtung
(`HISTORY_LIMIT` in `crates/redact-gui/src/history.rs`); ältere fallen unten
heraus. Eine neue Änderung nach einem Rückgängig macht den
Wiederholen-Stapel ungültig. Beim Öffnen eines anderen Dokuments wird der
Verlauf verworfen — er gehört zum Inhalt, nicht zum Fenster.

Erfasst sind alle Änderungen an der Trefferliste: Anlegen, Löschen,
Verschieben, Nachziehen, An- und Abwählen, das Übernehmen einer Review-Datei.

### Miniaturansichten und Zoom

Links steht eine Spalte mit **Miniaturansichten**, Seitenzahl neben jedem Bild.
Sie zeigt dieselben Kleinbilder, die der Hauptbereich ohnehin anfordert — es
wird nichts doppelt gerendert. Die Spalte lässt sich am Rand zwischen 96 und
260 Punkt breit ziehen.

Der **Zoom** reicht von **0,25× bis 4×** (`MIN_ZOOM`/`MAX_ZOOM` in
`crates/redact-gui/src/state.rs`), Schrittweite 1,25×. In der Leiste stehen
dafür vier Knöpfe: ➖ Kleiner, ➕ Größer, ⛶ Passend (ganze Seite ins Fenster)
und ⟲ 100 %; daneben ein Schieberegler für den stufenlosen Wert. Gerendert wird
auf einem eigenen Thread, damit Seitenwechsel und Zoomen die Oberfläche nicht
anhalten; lässt sich eine Seite nicht rasterisieren, springt eine schematische
Vorschau ein und zeigt wenigstens die Lage des Textes.

### Tastaturbedienung

| Taste | Wirkung |
|---|---|
| Strg+O | PDF öffnen |
| Strg+S | Geschwärztes PDF exportieren |
| Strg+Z / Strg+Y | Rückgängig / Wiederholen |
| Bild auf/ab, Pos1/Ende | blättern |
| Pfeiltasten | mit Auswahl: das Rechteck um 1 pt verschieben (mit Umschalt 10 pt) — ohne Auswahl: blättern |
| Entf | ausgewähltes Rechteck löschen |
| Esc | Auswahl aufheben |

Unter macOS tritt die Befehlstaste an die Stelle von Strg. **Liegt der Fokus in
einem Textfeld, gehören alle Tasten dorthin** und nirgendwo sonst hin — sonst
löschte die Rücktaste im Feld „Ersetzen“ die ausgewählte Region. Ein Knopf mit
Fokus (nach einem Druck auf Tabulator) ist dabei kein Textfeld; die Kürzel
wirken dort weiter (`a_tab_press_does_not_kill_every_shortcut` in
`crates/redact-gui/src/app.rs`).

### Trefferliste

Neben der Miniaturspalte steht die Trefferliste. Ihre Überschrift nennt
**beide** Zahlen — gefundene Treffer und die, die tatsächlich geschwärzt werden;
nur die zweite sagt etwas über das Ergebnis. Treffer, die die Konfliktauflösung
verwirft (blockiert, doppelt), stehen ausgegraut und durchgestrichen statt
angehakt und farbig. Geschützte Einträge der Negativliste werden **nicht**
durchgestrichen — durchgestrichen läse sich wie „entfernt“, gemeint ist das
Gegenteil; dort steht das Wort „geschützt“.

Je Eintrag lässt sich

* die Schwärzung **abwählen** (Häkchen) — ein Negativlisten-Treffer bleibt aus,
* die **Schwärzungsart ändern**: `blackout`, `whiteout` oder `replace` samt
  eigenem Ersatztext, je Treffer einzeln. Neue Treffer bekommen die Art aus
  `--action`/`--replace-with`.

Über der Liste steht der Schalter für die **automatische Erkennung**: das
Häkchen „Automatisch suchen“ (dasselbe wie `--no-patterns`) und darunter,
aufklappbar, „Muster einzeln“ mit einem Kästchen je Muster dieses Laufs. Ist
etwas abgeschaltet, sagt es die Kopfzeile selbst — `Automatische Suche AUS —
nicht gesucht, nur von Hand: 2 Treffer · 2 werden geschwärzt` —, denn
„0 Treffer“ hieße dort sonst dasselbe wie bei einem sauberen Dokument.
Solange nichts zu finden wäre (keine Muster, keine Buchungsliste, keine
Regionsdatei), ist „🔍 Analysieren“ ausgegraut und die Sprechblase sagt warum.
Einzelheiten unter [Automatische Funde abschalten](#automatische-funde-abschalten).

Über „🗄 Review speichern“ / „📋 Review laden“ geht der Stand als JSON hinaus
und wieder herein — mit der SHA-256-Prüfsumme des Dokuments, geprüft von
**derselben** Funktion, die `--apply-review` benutzt. Eine Review-Datei zu einem
anderen PDF wird abgelehnt.

Ob die Oberfläche hell oder dunkel startet, sagt `theme` in der
[Einstellungsdatei](#einstellungsdatei); umschalten lässt es sich jederzeit in
der Leiste oben. Die vier Trefferfarben erreichen in beiden Themen mindestens
3:1 Kontrast (WCAG 1.4.11), geprüft gegen die Flächen, die egui wirklich malt.

### GUI und CLI teilen sich die Verarbeitungskette — und das ist geprüft

Hier stand früher eine Liste von Unterschieden zwischen beiden Programmen —
die GUI hatte den Ablauf abgetippt statt geteilt und war davon abgewichen
(feste Polsterung, stilles Überschreiben, Audit-Log ohne Modus `0600`).
Beide gehen jetzt durch dasselbe Crate **`redact-pipeline`**: `redact-cli` und
`redact-gui` hängen beide daran, und die genannten Abweichungen sind geschlossen
(u. a. `export_uses_the_padding_from_the_configuration` und
`audit_path_follows_the_chosen_output` in `crates/redact-gui/src/state.rs`;
Review-Datei und Audit-Log gehen über `redact_pipeline::write_review_file`,
also über den einen Schreibpfad mit Modus `0600`).

**Den End-to-End-Vergleich gibt es inzwischen.**
[`crates/redact-cli/tests/cli_and_gui_agree.rs`](crates/redact-cli/tests/cli_and_gui_agree.rs)
startet das gebaute `redact-rs`-Binary als eigenen Prozess und daneben
`AppState` (laden → analysieren → exportieren) mit denselben Einstellungen und
vergleicht die Ausgabedatei **byteweise** sowie das Audit-Log Feld für Feld
(beide Prüfsummen eingeschlossen; ausgenommen sind nur Zeitstempel und Pfade,
die zwangsläufig verschieden sind). Sieben Fälle laufen dort:

| Test | prüft |
|---|---|
| `the_binary_and_the_window_produce_the_same_file_and_the_same_log` | Vorgabe-Aktion, `--padding 3`: gleiche Bytes, gleiches Log |
| `the_binary_and_the_window_agree_on_a_replacement_too` | dasselbe mit `--action replace` — der schärfere Fall, weil eine Font-Ressource dazukommt |
| `the_binary_and_the_window_agree_on_a_switched_off_pattern` | `--disable-pattern email`: gleiche Bytes, gleiches Log samt Feld `patterns` — mit Gegenprobe, dass ein Lauf **ohne** die Abschaltung andere Bytes ergibt |
| `the_binary_and_the_window_agree_with_no_patterns_at_all` | `--no-patterns` plus eine Region von Hand: beide schwärzen nur diese eine |
| `both_ways_write_the_same_review_file` | gleiche Review-Datei, beide mit Modus `0600` |
| `both_ways_write_the_same_review_file_with_a_replacement` | dasselbe mit Ersatztext |
| `a_deviating_window_would_be_caught` | **Gegenprobe**: die früher bestandene Abweichung (feste Polsterung) muss zu verschiedenen Bytes führen |

```bash
cargo test -p redact-cli --test cli_and_gui_agree
# test result: ok. 7 passed
```

Was trotzdem für die Kommandozeile spricht: nur dort ist der ganze Lauf ein
einzelner, wiederholbarer Befehl, den man in ein Skript schreiben und in einem
Protokoll nachlesen kann. Wer aus der GUI ein nachvollziehbares Ergebnis
braucht, exportiert eine Review-Datei und wendet sie mit
`redact-rs --apply-review` an.

## Architektur

```
redact-core       Domänenmodell, Konfliktauflösung, Review-Format, Namensregeln
redact-pdf        Content-Stream-Interpreter, Textextraktion, echte Schwärzung,
                  Leck-Detektor (`leaks`)
redact-patterns   Regex-Muster inkl. IBAN-/BIC-/Luhn-Prüfung
redact-booking    CSV-Buchungsliste, Positiv-/Negativ-Matching
redact-render     Rasterisierung der Seiten für die Vorschau (reines Rust)
redact-pipeline   die Verarbeitungskette — von CLI *und* GUI benutzt,
                  samt Audit-Log und Schreibpfad
redact-cli        Kommandozeile (Argumente, Ausgabe)
redact-gui        egui-Oberfläche (optionales Feature `gui` von redact-cli)
```

Ablauf:

```
PDF ──laden──► Textextraktion (zeichengenaue Koordinaten)
                     │
        ┌────────────┼────────────┐
        ▼            ▼            ▼
   manuell      Buchungsliste   Muster
        └────────────┼────────────┘
                     ▼
             Konfliktauflösung (Negativliste gewinnt)
                     │
             ┌───────┴────────┐
             ▼                ▼
        Review-JSON     Schwärzung anwenden
                              │
                     Metadaten strippen ──► PDF + Audit-Log
```

Der Kern ist ein eigener Content-Stream-Interpreter: er führt Grafik- und
Textzustand mit (CTM, `Tm`, `Tf`, `Tc`, `Tw`, `Tz`, `Ts`, `TL`), löst Fonts über
`/Widths`, CID-`/W`, `/ToUnicode` und `/Differences` auf und berechnet für jedes
einzelne Zeichen eine Bounding-Box. Beim Schwärzen werden die betroffenen
Zeichen aus der Text-Operation entfernt; der entfallende Vorschub wird als
`TJ`-Kerningwert eingesetzt, damit der übrige Text exakt an seiner Stelle bleibt.

## Sicherheit

Dieser Abschnitt nennt nur die Zusicherungen. Bedrohungsmodell, Grenzen für
Eingabedateien und die zugehörigen Messungen stehen in
[`SECURITY.md`](SECURITY.md); was das Werkzeug fachlich *nicht* kann, steht
unter [Was dieses Werkzeug nicht leistet](#grenzen).

**Was zugesichert wird**

* Text, den die Analyse gefunden hat, wird aus dem Content-Stream **entfernt**,
  nicht übermalt. Für die geprüften Fälle ist das mit
  [`redact_pdf::leaks`](#pruefen) an der geschriebenen Datei abgesichert — also
  mit einem Orakel, das nicht auf dem eigenen Extraktor beruht.
* **Pixel unter einer Schwärzung werden überschrieben**, nicht überdeckt. Das
  gilt für Bild-XObjects wie für Inline-Bilder, auch in Form-XObjects und auch
  bei gedrehten oder skalierten Platzierungen; wird dasselbe Bild von mehreren
  Seiten benutzt, bekommt die geschwärzte Seite eine eigene Kopie. Lässt sich
  ein betroffenes Bild nicht dekodieren, **bricht der Lauf ab**, statt eine
  Datei zu erzeugen, deren Schwärzung nur obenauf liegt ([Details](#bilder)).
* Text in Form-XObjects wird ebenfalls entfernt. Wird dasselbe XObject mehrfach
  platziert, wirkt die Entfernung auf alle Platzierungen — im Zweifel wird also
  eher zu viel als zu wenig geschwärzt.
* Annotationen, die in einen Schwärzungsbereich ragen, werden gelöscht.
* **Metadaten und Restdaten werden entfernt** — der Content-Stream ist nur
  *eine* der Stellen, an denen ein Geheimnis in einer PDF-Datei steht:

  | Woher | Was |
  |---|---|
  | Trailer | `/Info` (Titel, Autor …) |
  | Katalog | XMP (`/Metadata`), `/PieceInfo`, `/StructTreeRoot` samt `/MarkInfo`, `/Names` und `/Dests` |
  | Katalog | `/AcroForm` — **samt `/XFA`** (ein vollständiger zweiter Formulardatensatz als XML) |
  | jedes Formularfeld | die Werte `/V`, `/DV` und `/RV` — auch bei Widgets, die nur noch über `/Annots` erreichbar sind |
  | Katalog | `/OpenAction` und `/AA` — Aktionen, die beim Öffnen bzw. bei Ereignissen laufen und `/S /JavaScript` sein dürfen |
  | Katalog | `/OCProperties` — die Verwaltung optionaler Inhalte („Ebenen“) |
  | jede Seite | `/Metadata`, `/PieceInfo`, `/StructParents`, `/AA` |
  | jede Seite | Annotationen vom Typ `/FileAttachment` — ein Dateianhang klebt nicht nur im `/Names`-Baum |

  Preis: benannte Sprünge im Dokument funktionieren danach nicht mehr, und aus
  einem Formular wird ein totes Blatt Papier. Das ist die sichere Richtung.
  Objekte, die dadurch unerreichbar werden, werden zusätzlich aus der Datei
  geworfen (`prune_unreachable`) — `lopdf` schriebe sonst auch alles mit, was
  niemand mehr referenziert.

  So sieht das an einer Datei aus, die all das trägt:

  ```console
  $ redact-rs meta.pdf -o meta_out.pdf --booking-list buchungen.csv
  Metadaten entfernt: /Info-Dictionary, Formulardefinition (/AcroForm),
  XFA-Formulardaten (/XFA), Öffnen-Aktion (/OpenAction), Ebenen (/OCProperties),
  2 Feldwerte, 1 Dateianhang-Annotation, 2 Ereignisaktionen (/AA)
  ```

  Mit [`redact_pdf::leaks`](#pruefen) an der Ausgabedatei nachgeprüft: Feldwert,
  XFA-Inhalt, `/Info`-Titel, OpenAction-JavaScript, Dateianhang und der Name
  einer Ebene (`/OCG /Name`) sind restlos weg. Der Ebenenname war zuletzt die
  einzige verbliebene Fundstelle; er wird jetzt auch dann geleert, wenn eine
  Seite das `/OCG` über `/Resources /Properties` weiterhin referenziert und es
  deshalb das Entfernen von `/OCProperties` überlebt.
* Alles läuft lokal und im Speicher; es werden keine Netzverbindungen
  aufgebaut. Beim Schreiben entsteht genau **eine** temporäre Datei, und zwar
  im Zielverzeichnis (`.<name>.redact-<pid>-<n>.tmp`); sie wird per `rename`
  zur Ausgabedatei, damit nie eine halb geschriebene Datei sichtbar wird. In
  `/tmp` landet nichts.
* Gleiche Eingabe + gleiche Konfiguration ⇒ byteweise gleiche Ausgabe.
* Verschlüsselte PDFs werden ohne Passwort **abgelehnt**; strukturell defekte
  werden abgelehnt, nicht repariert. Das Passwort steht in keiner erzeugten
  Datei und in keiner Meldung
  ([`SECURITY.md`](SECURITY.md#passwörter-verschlüsselter-pdfs)).
* Review-Datei und Audit-Log entstehen unter Unix mit Modus `0600` — **in
  beiden Programmen**. Sie gehen durch denselben Schreibpfad
  (`redact_pipeline::write_review_file` bzw. `AuditLog::write`, beide über
  `redact_pdf::document::write_file` mit `secret_options`), und beide Programme
  rufen ihn. Gemessen an der geschriebenen Datei: die Oberfläche in
  `export_removes_the_text_from_the_pdf`
  (`crates/redact-gui/src/state.rs`, `assert_eq!(mode, 0o600)` auf das
  Audit-Log), beide Wege nebeneinander in `both_ways_write_the_same_review_file`
  (`crates/redact-cli/tests/cli_and_gui_agree.rs`). Mit `0644` und ohne
  Symlink-Prüfung entstanden sie in der Oberfläche früher — das ist geschlossen.

## Abweichungen vom Ursprungskonzept

Das Konzept wurde bei der Umsetzung an einigen Stellen fachlich nachgeschärft.
Alle Abweichungen sind bewusst:

| Konzept | Umsetzung | Begründung |
|---------|-----------|------------|
| fünf Traits (`Extractor`, `Analyzer`, …) in `redact-core/src/traits.rs` | **keine Traits**; `traits.rs` ist gelöscht, die Methoden sind inhärent (`PdfExtractor::extract_with_warnings`, `PatternMatcher::find_matches`, `BookingMatcher::find_matches`, `PdfRedactor::apply_with_report`) | Von jedem Trait gab es genau eine Implementierung, kein `dyn`-Gebrauch und keine generische Schranke. Der letzte, `Extractor`, blieb nur stehen, weil sieben Testdateien ihn importieren mussten, um `extract` überhaupt aufrufen zu dürfen — ein Trait, den nur Tests brauchen, ist die Umkehrung seines Zwecks. |
| `Extractor::extract → Vec<Region>` | `→ Vec<TextRun>` | Eine `Region` braucht zwingend eine `Source`; bei reiner Extraktion steht die noch gar nicht fest. `TextRun` liefert zusätzlich die Glyph-Boxen, ohne die für einen Regex-Treffer *innerhalb* einer Zeile keine exakte Box berechenbar wäre. |
| `Analyzer::analyze(&[Region])` | Analyse auf `&[TextRun]` | Dieselbe Begründung: Analyse braucht Text **mit** Zeichenkoordinaten. |
| Matching auf einzelnen Text-Runs | Matching auf zusammengesetzten **Zeilen** | Eine IBAN wird in der Praxis über mehrere `Tj`-Operationen ausgegeben. Ohne Zeilenbildung liegt der Recall weit unter dem Ziel. Über einen Zeilenumbruch hinweg trifft aber auch das nicht. |
| `BookingMatcher`: `text.contains(pattern)` | normalisierter Vergleich (Groß-/Kleinschreibung, Leerraum-Menge) | Sonst scheitert der Vergleich an jeder abweichenden Anzahl Leerzeichen. |
| Negativliste blockiert alles | Negativliste blockiert *nicht* manuelle Regionen | Eine bewusste Nutzerentscheidung darf nicht automatisch verworfen werden. |
| `--review` liefert eine Regionsliste | eigenes `ReviewFile` mit Version, SHA-256 und `enabled`-Schalter je Treffer | Ohne Prüfsumme lassen sich Koordinaten versehentlich auf ein fremdes Dokument anwenden. |
| GUI: `pdfium-render` für die Seitenanzeige | eigener Rust-Rasterizer `redact-render` (tiny-skia + skrifa) | `pdfium` verlangt eine native Bibliothek zur Laufzeit und hätte das „ein einziges Binary“-Versprechen gebrochen. `pdfium` ist im Projekt gar nicht enthalten — auch nicht als optionales Feature. |
| GUI: `eframe` mit `wgpu` | `eframe` mit `glow` (OpenGL) | Deutlich kleineres Binary und kürzere Bauzeit; für diese Oberfläche reicht OpenGL vollständig aus. |
| `is_regex` in der Buchungsliste | Metazeichen werden abgelehnt, Verweis auf `--patterns-config` | Hält `redact-booking` abhängigkeitsfrei; echte Regexe gehören ohnehin in die Musterkonfiguration. |
| — | zusätzlich: Annotationen entfernen, Bilder-Warnung, `--json`, `--write-demo`, Prüfsummen-Validatoren, Leck-Detektor `leaks` | Lücken, die beim Umsetzen sichtbar wurden. |

## Stand der Akzeptanzkriterien

| Kriterium | Stand | Nachweis |
|-----------|-------|----------|
| 10 Seiten in unter 2 s (ohne OCR) | erfüllt | Nachgemessen: `crates/redact-pdf/examples/gen10.rs` erzeugt 10 Seiten à 45 Zeilen (59 824 Byte); `redact-rs gross.pdf -o out.pdf --patterns iban_de,amount_eur,date_de` findet **1350 Treffer**, schwärzt alle 1350 und entfernt 24 300 Zeichen. **0,16 s** Wanduhr für den ganzen Prozess (bestes von fünf Läufen, Release, `/usr/bin/time`; Spitzenspeicher 12,6 MB) — also inklusive Start, Lesen und Schreiben. Hier stand früher „77 ms“; das ist nicht die Zeit, die ein Aufruf braucht. |
| Deutsche IBAN wird zuverlässig erkannt | erfüllt, **innerhalb einer Zeile** | Muster mit mod-97-Prüfung; gruppiert und ungruppiert getestet. Über einen Zeilenumbruch verteilt wird sie nicht gefunden. |
| Negativliste blockiert Schwärzung zuverlässig | erfüllt | `negative_list_prevents_redaction` prüft am fertigen PDF, dass die geschützte IBAN erhalten bleibt und die ungeschützte verschwindet |
| Copy-Paste liefert keinen sensitiven Text | erfüllt für gefundenen Text | geprüft mit `redact_pdf::leaks` an der geschriebenen Datei, nicht mit dem eigenen Extraktor. Was die Analyse nicht findet, wird nicht geschwärzt — siehe [Grenzen](#grenzen). |
| Audit-Log mit SHA-256 beider Dateien | erfüllt | `review_then_apply_roundtrip` |
| Aussagekräftige Fehler bei kaputten PDFs | erfüllt | `rejects_broken_pdf_with_clear_message` |
| Manuelle Regionen (JSON) werden angewendet | erfüllt | `manual_regions_are_applied` |
| Das Audit-Log bescheinigt nur Gemessenes | erfüllt | `a_region_on_a_page_that_does_not_exist_is_not_logged_as_applied`, `a_region_without_text_under_it_is_covered_not_missing`, `a_degenerate_padding_is_not_logged_as_a_redaction` — jeweils mit Gegenprobe |
| Review-Datei wirkt nur auf ihr eigenes Dokument | erfüllt, auch hinter `--manual-regions` | `review_file_from_another_document_is_rejected`, `a_foreign_review_file_behind_manual_regions_is_refused_too` |
| Metadaten im Ausgabe-PDF entfernt | erfüllt | `metadata_is_stripped`, `names_tree_is_removed_as_the_module_documentation_promises` |
| GUI: Rechtecke ziehen, Treffer abwählen, Export | umgesetzt, darüber hinaus | dazu Eckgriffe, Rückgängig/Wiederholen, Miniaturansichten, Zoom 0,25×–4×, Tastaturbedienung, Drag & Drop und die Schwärzungsart je Treffer ([Details](#grafische-oberfläche)). Alle Rechnungen und Zustandsübergänge liegen als reine Funktionen in `state.rs`, `selector.rs`, `viewer.rs`, `history.rs`, `focus.rs` und sind ohne Fenster getestet; das Fensterverhalten selbst ist nicht automatisiert prüfbar |
| GUI-Binary unter 30 MB | erfüllt, alle vier Artefakte | An den **ausgelieferten** Binaries von v0.3.0 nachgemessen (heruntergeladen und gegen `SHA256SUMS-BINARIES` geprüft): `redact-rs.exe` 10,6 MB (10 586 624 Byte), `redact-rs-gui.exe` 9,8 MB (9 827 840 Byte), Linux/glibc `redact-rs` 14,9 MB (14 900 000 Byte), Linux/musl `redact-rs` 4,9 MB (4 944 824 Byte). Der größte Wert liegt bei der Hälfte der Grenze. Hier stand früher „Windows 7,4 MB (7 395 328 Byte)“ und „Linux 13 MB“ — beides aus einer früheren Fassung und an keinem ausgelieferten Artefakt nachgemessen. |
| Export der GUI identisch zur CLI | erfüllt | `cli_and_gui_agree.rs` startet das gebaute Binary als eigenen Prozess und daneben `AppState` (laden → analysieren → exportieren) und vergleicht Ausgabedatei **byteweise** sowie das Audit-Log Feld für Feld — inzwischen **sieben** Fälle: Vorgabe-Aktion, `--action replace`, `--disable-pattern`, `--no-patterns`, die Review-Datei in beiden Varianten und die Gegenprobe `a_deviating_window_would_be_caught`, die die früher bestandene Abweichung nachstellt und anschlagen muss (Tabelle unter [Grafische Oberfläche](#grafische-oberfläche)). Nachgemessen: `test result: ok. 7 passed`. |

Von dem, was das Konzept in §11 außerhalb des MVP führt, sind das
[Entschlüsseln passwortgeschützter PDFs](#verschluesselte-pdfs) und die
[Stapelverarbeitung](#stapel) inzwischen umgesetzt. Nicht umgesetzt: OCR für
gescannte PDFs und ein Plugin-System.

## Entwicklung

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build -p redact-cli --no-default-features       # ohne GUI
cargo test -p redact-pdf -- --ignored                 # die bekannten Lecks
./scripts/build-windows.sh                            # Windows-Binary (mingw)
cargo run --release -p redact-pdf --example gen10 -- gross.pdf 10
```

## Release bauen

**Zwei Vorbedingungen, an denen kein Weg vorbeiführt** (`verify` in
[`.github/workflows/release.yml`](.github/workflows/release.yml)):

1. **Der Commit muss auf dem Default-Branch liegen.** Geprüft mit
   `git merge-base --is-ancestor` gegen den Branch, den die GitHub-API als
   Default meldet. Ein Arbeitsbranch veröffentlicht nichts — auch nicht über
   einen Tag, auch nicht über `workflow_dispatch` mit eigenem `ref`.
2. **Die vollständige CI muss grün sein.** `release.yml` ruft
   [`ci.yml`](.github/workflows/ci.yml) als wiederverwendbaren Workflow auf
   (Job `ci`) und macht ihn zur Vorbedingung von `build` und `release` — es ist
   dieselbe Datei wie bei jedem Push, keine Kopie. Vorher gab es in diesem
   Ablauf keinen einzigen Test-, Clippy-, fmt- oder `cargo deny`-Schritt: aus
   rotem Code konnte ein öffentliches Binary werden, und genau das ist bei
   v0.3.0 passiert.

Dazu prüft `verify`, dass Tag, `.release-version` und die Version des Crates
`redact-cli` (über `cargo metadata`) dasselbe sagen, und nagelt den Ref **einmal**
auf eine Commit-ID fest, die dann CI, Bau und Veröffentlichung gemeinsam
benutzen. Eine Versionsnummer mit Bindestrich (`v0.4.0-rc.1`) wird auf jedem
Weg als Vorabversion gekennzeichnet.

**Der übliche Weg** ist die Versionsdatei. Sie trägt einen Kommentarkopf, der
sagt, dass eine Änderung an ihr veröffentlicht — der gehört nicht weggeworfen,
deshalb wird nur die Versionszeile ersetzt und nicht die ganze Datei:

```bash
# Nur die Versionszeile ersetzen; Kommentarkopf bleibt stehen.
sed -i 's/^[0-9].*/0.4.0/' .release-version
# Version in Cargo.toml gleichziehen (verify vergleicht beide)
git commit -am "Release 0.4.0"
git push origin HEAD:main          # Default-Branch, sonst passiert nichts
```

`publish-release.yml` reagiert auf Änderungen an `.release-version`, aber nur
auf `main`/`master` **und** nur, wenn das auch der tatsächliche Default-Branch
ist. Der Tag wird vom Release selbst am gebauten Commit angelegt; man legt ihn
nicht vorher an.

**Der Tag-Weg** existiert weiter (`git push origin v0.4.0` löst `release.yml`
direkt aus), führt aber durch dieselben beiden Vorbedingungen. Ein Tag auf einem
Arbeitsstand scheitert an Nr. 1:

```text
::error::Commit <sha> liegt nicht auf dem Default-Branch (main). Ein Release
entsteht nur aus Code, der dort angekommen ist. Erst zusammenführen, dann
veröffentlichen.
```

Der Weg über die Versionsdatei existiert zusätzlich, weil in abgeschotteten
Umgebungen häufig nur auf einen bestimmten Branch gepusht werden darf und
`workflow_dispatch` über die API gesperrt ist.

Gebaut werden **drei** Artefakte (Matrix in `release.yml`): Windows
(CLI + GUI), Linux/glibc (CLI + GUI) und Linux/musl (nur CLI, statisch). Dazu
kommen `SHA256SUMS` und `SHA256SUMS-BINARIES` als eigene Release-Dateien.

## Lizenz

Wahlweise [MIT](LICENSE-MIT) oder [Apache-2.0](LICENSE-APACHE).
