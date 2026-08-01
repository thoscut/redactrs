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

---

## Inhalt

1. [Schnellstart](#schnellstart)
2. [Installation](#installation)
3. [Kommandozeile](#kommandozeile)
   * [Verschlüsselte PDFs](#verschluesselte-pdfs)
   * [Mehrere Dateien auf einmal](#stapel)
   * [Einstellungsdatei](#einstellungsdatei)
4. [Buchungsliste](#buchungsliste)
5. [Manuelle Regionen](#manuelle-regionen)
6. [Eigene Patterns](#eigene-patterns)
7. [Bilder werden wirklich geschwärzt](#bilder)
8. [Review-Workflow](#review-workflow)
9. [Audit-Log](#audit-log)
10. [Prüfen, ob die Schwärzung gewirkt hat](#pruefen)
11. [Was dieses Werkzeug nicht leistet](#grenzen)
12. [Grafische Oberfläche](#grafische-oberfläche)
13. [Architektur](#architektur)
14. [Sicherheit — was zugesichert wird](#sicherheit)
15. [Abweichungen vom Ursprungskonzept](#abweichungen-vom-ursprungskonzept)
16. [Entwicklung](#entwicklung)

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

Fertige Binaries für Windows und Linux liegen unter
[Releases](https://github.com/thoscut/redactrs/releases).

Das Windows-Archiv enthält **zwei Programme**, dazu die Beispieldateien und die
SHA-256-Prüfsummen:

| Datei | wofür |
|---|---|
| `redact-rs-gui.exe` | Die zum **Doppelklicken**. Öffnet nur das Fenster, ohne die schwarze Konsole dahinter, und nimmt ein PDF entgegen, das man auf ihr Symbol zieht. |
| `redact-rs.exe` | Die **Konsolenfassung** mit allen Optionen — und der einzige Weg zu Ausgaben auf stdout (`--json`, `--list-patterns`). |

Zwei Dateien statt eines Schalters, weil `#![windows_subsystem = "windows"]`
das Konsolenfenster nur um den Preis *jeder* Ausgabe auf stdout/stderr abschaltet
(siehe `crates/redact-cli/src/bin/redact-rs-gui.rs`). Unter Linux gibt es diese
Trennung nicht; dort startet `redact-rs` ohne Argumente die Oberfläche.

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

```text
redact-rs [EINGABE.pdf | VERZEICHNIS …] [OPTIONEN]

  -o, --output <PDF>          Ausgabedatei (Standard: neben der Eingabe)
      --output-suffix <TEXT>  Namenszusatz (Standard: _geschwaerzt)
      --password <PW>         Passwort eines verschlüsselten PDFs
                              (besser: REDACT_RS_PASSWORD — siehe unten)
  -f, --force                 vorhandene Ausgabedatei überschreiben
      --patterns <IDs>        Muster, kommagetrennt (z.B. iban_de,bic)
      --no-patterns           gar keine Muster anwenden
      --patterns-config <F>   eigene Musterkonfiguration (YAML oder JSON)
      --min-confidence <WERT> Mindestvertrauen eines Treffers (Standard: 0.5)
      --booking-list <CSV>    Buchungsliste (Positiv-/Negativliste)
      --manual-regions <JSON> manuell festgelegte Regionen
      --review                nur analysieren, nichts schwärzen
      --review-out <JSON>     Zieldatei des Review-Exports
      --apply-review <JSON>   geprüfte Review-Datei anwenden
      --audit-log <JSON>      Audit-Log schreiben
      --action <ART>          blackout (Standard) | whiteout | replace
      --replace-with <TEXT>   Ersatztext für --action replace
      --padding <PUNKT>       Rand um jede Schwärzung (Standard: 1.0)
      --allow-undecodable-images  nicht dekodierbare Bilder durchgehen lassen
                                  (UNSICHER — siehe unten)
      --max-decompressed-mb <MB>  Budget für alle entpackten Streams (1024)
      --max-parsed-mb <MB>        davon für geparste Streams (16)
      --max-candidates <N>        Obergrenze für Trefferkandidaten (100000)
      --gui                   grafische Oberfläche starten
      --list-patterns         eingebaute Muster auflisten
      --write-demo <PDF>      Beispieldatei erzeugen
      --json                  Zusammenfassung als JSON
  -q, --quiet                 weniger Ausgabe
```

Die drei `--max-…`-Grenzen sind Schutzschalter gegen präparierte Eingabedateien;
was sie abwehren und warum sie so hoch bzw. so niedrig liegen, steht in
[`SECURITY.md`](SECURITY.md). Sie wirken **nur in der Kommandozeile** — die
grafische Oberfläche lädt mit den fest eingebauten Vorgaben.

`--allow-undecodable-images` ist der einzige Schalter, der die Sicherheit
*senkt*: siehe [Bilder werden wirklich geschwärzt](#bilder).

Rückgabewerte: `0` Erfolg, `1` Verarbeitungsfehler, `2` Bedienfehler. Im
Stapelbetrieb heißt `1`: mindestens eine Datei ist gescheitert.

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
Was dabei ungeprüft bleibt (die Vorprüfung kann verschlüsselte Streams nicht
auspacken; der Arbeitsspeicher wird nicht überschrieben), steht in
[`SECURITY.md`](SECURITY.md#passwörter-verschlüsselter-pdfs).

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

```text
januar/auszug.pdf → januar/auszug_geschwaerzt.pdf (4 Schwärzung(en))
februar/auszug.pdf → februar/auszug_geschwaerzt.pdf (3 Schwärzung(en))

2 Datei(en): 2 verarbeitet, 0 fehlgeschlagen.
```

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
min_confidence: 0.4           # Mindestvertrauen      (Vorgabe: 0.5)
padding: 2.0                  # Polsterung in Punkt   (Vorgabe: 1.0)
theme: dunkel                 # Thema der Oberfläche  (hell | dunkel)
```

**Rangfolge: Kommandozeile schlägt Datei schlägt Vorgabe.** Ein Schalter, der
nicht angegeben wurde, überschreibt die Datei nicht — das ist der Grund, warum
`--output-suffix`, `--padding` und `--min-confidence` in der Hilfe keinen
Standardwert mehr anzeigen. Geprüft wird die Reihenfolge in
`crates/redact-cli/tests/settings.rs`.

Ein unbekannter Schlüssel (Tippfehler) und ein unbekanntes Thema beenden den
Lauf mit einer Meldung. Eine Einstellung, die stillschweigend nicht wirkt, wäre
das schlechtere Verhalten. Eine **fehlende** Datei ist dagegen kein Fehler.

Die Datei gilt für beide Programme: sie wird beim Bauen von
`redact_pipeline::Config` angewendet, und dieselbe `Config` bekommt die
grafische Oberfläche.

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
| `konto_nr` | Kontonummer (6–10 Ziffern nach „Kto.“ o. ä.) | 0.85 | 0.30 | **an** |
| `blz` | Bankleitzahl (8 Ziffern nach „BLZ“/„Bankleitzahl“) | 0.80 | 0.25 | **an** |
| `bic` | BIC/SWIFT mit Länderkennung-Prüfung | 0.80 | – | **an** |
| `amount_eur` | Geldbetrag in Euro | 0.70 | – | aus |
| `date_de` | Datum TT.MM.JJJJ | 0.60 | – | aus |
| `credit_card` | Kreditkartennummer (13–19 Ziffern), Luhn-geprüft | 0.90 | – | **an** |
| `steuer_id` | Steuerliche Identifikationsnummer (11 Ziffern nach „Steuer-ID“) | 0.90 | 0.25 | **an** |
| `email` | E-Mail-Adresse | 0.90 | – | **an** |
| `phone_de` | Deutsche Telefonnummer | 0.85 | 0.35 | **an** |

Muster mit Prüfsumme (IBAN, BIC, Kreditkarte) verwerfen Treffer, die die
Prüfung nicht bestehen — das drückt die Fehlalarmquote deutlich.
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

Nachgemessen an einem 200×100-Bild, das eine Seite als Scan trägt, mit einer
manuellen Region darüber:

```console
$ redact-rs scan.pdf -o scan_geschwaerzt.pdf --manual-regions regionen.json
Seiten:             1
Schwärzungen:       1
Entfernte Zeichen:  0
Deck-Rechtecke:     1
Überschriebene Bilder: 1 (neu kodiert, verlustbehaftet)
```

Das Bild aus der Ausgabedatei erneut dekodiert und Pixel für Pixel mit dem
Original verglichen: **4784 von 20 000 Pixeln geändert, alle 4784 auf Schwarz;
kein einziges Pixel außerhalb des Rechtecks verändert.**

> Das Wort **„verlustbehaftet“ in dieser Ausgabezeile ist falsch** und wird
> noch korrigiert. Neu kodiert wird immer verlustfrei mit `/FlateDecode`
> (`crates/redact-pdf/src/image.rs`); die Messung oben zeigt außerhalb der
> Schwärzung null veränderte Pixel — auch dann, wenn das Original ein JPEG war.
> Nicht bitgleich ist die *Datei*, nicht das *Bild*.

### Der Preis: die Datei ist nicht mehr bitgleich

Ein überschriebenes Bild wird **verlustfrei neu kodiert — immer als
`/FlateDecode`**. Ein `/DCTDecode`-Bild (JPEG) verliert dabei seinen Filter:

```console
$ # vorher                        nachher
$ #   /Filter /DCTDecode            /Filter /FlateDecode
$ #   10 307 Stream-Bytes           39 296 Stream-Bytes
$ #   Datei 10 961 Byte             Datei 39 990 Byte
```

Das ist Absicht. JPEG neu zu kodieren wäre verlustbehaftet, und die DCT-Blöcke
am Rand der Schwärzung könnten Reste der ursprünglichen Pixel zurücktragen.
Die Datei wird dafür deutlich größer.

Ehrlich dazugesagt: **verlustfrei heißt pixelgleich, nicht bytegleich.** Die
Bildpunkte außerhalb der Schwärzung sind nachweislich unverändert (siehe die
Messung oben, auch beim JPEG-Fall), aber das Bild-Objekt in der Datei ist ein
anderes als im Original. Wer Bitgleichheit gegenüber dem Original braucht, darf
keine Schwärzung über ein Bild legen.

### Nicht dekodierbare Bilder brechen den Lauf ab

JPEG-2000 (`/JPXDecode`) und Fax-Kodierung (`/CCITTFaxDecode`) kann redact-rs
nicht öffnen. Eine Schwärzung darauf ließe sich nur *überdecken* — die Pixel
blieben in der Datei. Deshalb bricht der Lauf in diesem Fall mit einem Fehler
ab, statt eine Datei zu erzeugen, deren Schwärzung nur obenauf liegt:

```console
$ redact-rs scan_jpx.pdf -o out.pdf --manual-regions regionen.json
Fehler: PDF-Fehler: Bild /Im0 auf Seite 1 lässt sich nicht dekodieren
(Filter: JPXDecode). Die Schwärzung läge nur darüber; die Pixel blieben
in der Datei.
$ echo $?
1
```

`--allow-undecodable-images` hebt das auf. Dann entsteht eine Ausgabe (Exit
`0`), in der das Bild **ungeschwärzt** ist und die Schwärzung nur darüberliegt;
es bleibt bei einer Warnung in der Zusammenfassung und im Audit-Log:

```console
$ redact-rs scan_jpx.pdf -o out.pdf --manual-regions regionen.json \
      --allow-undecodable-images
…
Warnung: Bild /Im0 auf Seite 1 lässt sich nicht dekodieren (Filter: JPXDecode).
Die Schwärzung läge nur darüber; die Pixel blieben in der Datei.
```

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

Diese Sperre hängt aber daran, dass das Feld **gefüllt** ist: ist
`input.sha256` leer, wird die Prüfung übersprungen und die Datei auf jedes
beliebige Dokument angewendet. Wer eine Review-Datei von Hand baut oder
zusammenkopiert, muss die Prüfsumme also mitschreiben — sonst gibt es keine
Warnung, sondern nur schwarze Balken an den falschen Stellen.

## Audit-Log

```json
{
  "timestamp": "2026-08-01T09:04:22Z",
  "tool": { "name": "redact-rs", "version": "0.1.0" },
  "input":  { "path": "kontoauszug.pdf", "sha256": "466c0af4…" },
  "output": { "path": "geschwaerzt.pdf", "sha256": "c2c54724…" },
  "redactions": [
    {
      "page": 0,
      "rect":           { "ll": { "x": 100.9, "y": 732.8 }, "ur": { "x": 239.9, "y": 742.5 } },
      "effective_rect": { "ll": { "x":  99.9, "y": 731.8 }, "ur": { "x": 240.9, "y": 743.5 } },
      "effect": "applied",
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
    "info": true, "xmp": false, "acroform": true, "xfa": true,
    "field_values": 2, "file_attachments": 1, "open_action": true,
    "additional_actions": 2, "optional_content": true,
    "summary": ["/Info-Dictionary", "Formulardefinition (/AcroForm)", "…"]
  },
  "effect": {
    "padding": 1.0, "requested": 4, "applied": 4, "degenerate": 0,
    "removed_glyphs": 66, "drawn_rects": 4, "removed_annotations": 0,
    "redacted_images": 0, "copied_images": 0
  }
}
```

`rect` ist das gefundene Rechteck, `effective_rect` dasselbe zuzüglich
`--padding`. `metadata` ist der **gemessene** Bericht darüber, was tatsächlich
entfernt wurde (nicht, was vorgesehen war); `effect` fasst den Lauf in Zahlen
zusammen — `redacted_images` und `copied_images` beziffern die
[Bildschwärzung](#bilder).

### `page` zählt überall gleich

**In jeder JSON-Datei ist die erste Seite `0`** — in `review.json`, im
Audit-Log und in der `--manual-regions`-Eingabe. Nur im Fließtext, also in der
Konsolenausgabe und in der Oberfläche, heißt dieselbe Seite „Seite 1“.

Das war bis v0.2.0 nicht so: das Audit-Log zählte als einziges ab 1. Wer
`review.json` und `audit.json` nebeneinander legte — und genau dazu lädt der
Workflow ein —, sah denselben Treffer einmal als `"page": 0` und einmal als
`"page": 1`. Eine Seitenzahl aus dem Log in eine Regionsdatei zu übernehmen
ging damit still daneben. Diese Stolperfalle gibt es nicht mehr.

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

```console
$ cargo run --release -- ../kontoauszug_geschwaerzt.pdf \
      "DE89 3704 0044 0532 0130 00" "Max Mustermann" "532013000"
sauber: DE89 3704 0044 0532 0130 00
LECK (6x): Max Mustermann
    Rohdaten-Stream @0x1e1 (inflate) [Inhalt, UTF-8/ASCII]: …(Kontoinhaber: Max Mustermann) Tj…
    Objekt 11 0 <Stream, lopdf-dekodiert> [Zeichenketten-Verkettung]: …Kontoinhaber: Max Mustermann…
    …
LECK (4x): 532013000
    …
```

Der Rückgabewert ist `1`, sobald irgendetwas gefunden wurde — das Programm
eignet sich damit als Kontrollschritt in einem Skript.

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
  $ redact-rs wrap.pdf -o wrap_out2.pdf --patterns iban_de
  Treffer gesamt:     0
  ```

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
  Treffer gesamt:     0
  Warnung: XObject „Fx“ hat kein bekanntes /Subtype (weder /Form noch /Image);
  sein Inhalt wurde nicht durchsucht. Steht dort Text, blieb er ungeschwärzt.
  ```

  Eine solche Warnung ist der Hinweis, dass „0 Schwärzungen“ nichts bedeutet.
  Sie ersetzt die Sichtprüfung nicht.

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
  Ausgabe:            scan_geschwaerzt.pdf
  Warnung: 1 von 1 Seite(n) enthalten Rasterbilder. Geschwärzte Bereiche
  werden im Bild selbst überschrieben; gelesen wird der Bildinhalt aber
  nicht — Text *in* einem Bild (Scan, Foto) findet die Analyse ohne OCR nicht.
  ```

  Festgehalten von `a_pure_scan_is_reported_even_without_any_redaction`
  (`crates/redact-pdf/tests/images.rs`). Die Ausgabedatei enthält trotzdem
  alles: wer ein gescanntes Dokument bearbeitet, muss die Seiten selbst
  ansehen und die Bereiche von Hand ziehen.
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
| `/ActualText` spiegelt den geschwärzten Text | `struct_elem_actual_text_does_not_mirror_the_redacted_text` |

### Verarbeitung

* **Verschlüsselte PDFs** werden ohne Passwort abgelehnt. Mit `--password` bzw.
  `REDACT_RS_PASSWORD` werden sie entschlüsselt und normal verarbeitet — was
  dabei ungeprüft bleibt, steht in
  [`SECURITY.md`](SECURITY.md#passwörter-verschlüsselter-pdfs). Nicht jedes
  Verfahren ist lesbar; was `lopdf` nicht beherrscht, endet mit derselben
  Meldung wie ein falsches Passwort.
* **Strukturell defekte PDFs** werden abgelehnt, nicht repariert — eine
  „reparierte“ Datei könnte Inhalte enthalten, die der Analyse entgehen.
* **Kein Plugin-System.** Die Stapelverarbeitung gibt es inzwischen
  ([siehe oben](#stapel)), sie steigt aber **nicht** in Unterverzeichnisse ab.
* **Keine unbegrenzte Größe.** Sehr viele Treffer in einer Datei lassen die
  Konfliktauflösung quadratisch wachsen; der Lauf bricht ab `--max-candidates`
  (Vorgabe 100 000) mit Exit 2 ab. Details und Messwerte in
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

Ein PDF lässt sich auch **per Drag & Drop** auf das Fenster ziehen. Beim
Export ist der Dateiname bereits vorbelegt: dasselbe Verzeichnis wie das
Original, mit dem Namenszusatz aus dem Feld „Namenszusatz“.

Die GUI (egui/eframe, ein einziges Binary ohne zusätzliche Laufzeit) zeigt die
Seiten mit allen gefundenen Treffern als farbige Rahmen:
🔵 Muster · 🟢 Positivliste · 🔴 Negativliste (blockiert) · 🟠 manuell.
Neue Bereiche zieht man mit der Maus, Treffer schaltet man per Checkbox ab.

Ist das geöffnete Dokument **verschlüsselt**, erscheint ein Fenster mit
verdeckter Eingabe. Das ist zugleich der bequemste Weg, ein Passwort *nicht*
über die Kommandozeile zu geben. Passt es nicht, bleibt die Frage stehen; das
falsche Passwort wird nicht behalten und steht in keiner Meldung.

Ob die Oberfläche hell oder dunkel startet, sagt `theme` in der
[Einstellungsdatei](#einstellungsdatei); umschalten lässt es sich jederzeit in
der Leiste oben.

### GUI und CLI teilen sich inzwischen die Verarbeitungskette

Hier stand früher eine Liste von Unterschieden zwischen beiden Programmen —
die GUI hatte den Ablauf abgetippt statt geteilt und war davon abgewichen
(feste Polsterung, stilles Überschreiben, Audit-Log ohne Modus `0600`).
Beide gehen jetzt durch dasselbe Crate **`redact-pipeline`**: `redact-cli` und
`redact-gui` hängen beide daran, und die genannten Abweichungen sind geschlossen
(u. a. `export_uses_the_padding_from_the_configuration` und
`audit_path_follows_the_chosen_output` in `crates/redact-gui/src/state.rs`;
Review-Datei und Audit-Log gehen über `redact_pipeline::write_review_file`,
also über den einen Schreibpfad mit Modus `0600`).

Was **weiterhin gilt**: Es gibt keinen Test, der beide *Programme* startet und
ihre Ausgabedateien byteweise vergleicht. Dass die Kette dieselbe ist, ist am
gemeinsamen Crate ablesbar, nicht an einem End-to-End-Vergleich. Wer ein
nachvollziehbares, prüfbares Ergebnis braucht, nimmt weiterhin die
Kommandozeile — oder exportiert aus der GUI eine Review-Datei und wendet sie
mit `redact-rs --apply-review` an.

## Architektur

```
redact-core       Domänenmodell, Traits, Konfliktauflösung, Review-Format
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
* Review-Datei und Audit-Log entstehen unter Unix mit Modus `0600` — das gilt
  für die Kommandozeile; die Oberfläche schreibt beide anders
  ([siehe oben](#grafische-oberfläche)).

## Abweichungen vom Ursprungskonzept

Das Konzept wurde bei der Umsetzung an einigen Stellen fachlich nachgeschärft.
Alle Abweichungen sind bewusst:

| Konzept | Umsetzung | Begründung |
|---------|-----------|------------|
| `Extractor::extract → Vec<Region>` | `→ Vec<TextRun>` | Eine `Region` braucht zwingend eine `Source`; bei reiner Extraktion steht die noch gar nicht fest. `TextRun` liefert zusätzlich die Glyph-Boxen, ohne die für einen Regex-Treffer *innerhalb* einer Zeile keine exakte Box berechenbar wäre. |
| `Analyzer::analyze(&[Region])` | `analyze(&[TextRun])` | Dieselbe Begründung: Analyse braucht Text **mit** Zeichenkoordinaten. |
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
| 10 Seiten in unter 2 s (ohne OCR) | erfüllt | 10 Seiten, 450 Zeilen, 1350 Schwärzungen in 77 ms (`examples/gen10.rs`, Release) |
| Deutsche IBAN wird zuverlässig erkannt | erfüllt, **innerhalb einer Zeile** | Muster mit mod-97-Prüfung; gruppiert und ungruppiert getestet. Über einen Zeilenumbruch verteilt wird sie nicht gefunden. |
| Negativliste blockiert Schwärzung zuverlässig | erfüllt | `negative_list_prevents_redaction` prüft am fertigen PDF, dass die geschützte IBAN erhalten bleibt und die ungeschützte verschwindet |
| Copy-Paste liefert keinen sensitiven Text | erfüllt für gefundenen Text | geprüft mit `redact_pdf::leaks` an der geschriebenen Datei, nicht mit dem eigenen Extraktor. Was die Analyse nicht findet, wird nicht geschwärzt — siehe [Grenzen](#grenzen). |
| Audit-Log mit SHA-256 beider Dateien | erfüllt | `review_then_apply_roundtrip` |
| Aussagekräftige Fehler bei kaputten PDFs | erfüllt | `rejects_broken_pdf_with_clear_message` |
| Manuelle Regionen (JSON) werden angewendet | erfüllt | `manual_regions_are_applied` |
| Metadaten im Ausgabe-PDF entfernt | erfüllt | `metadata_is_stripped`, `names_tree_is_removed_as_the_module_documentation_promises` |
| GUI: Rechtecke ziehen, Treffer abwählen, Export | umgesetzt | Logik als reine Funktionen getestet; das Fensterverhalten selbst ist nicht automatisiert prüfbar |
| GUI-Binary unter 30 MB | erfüllt (für die gemessene Datei) | Windows 7,4 MB nachgemessen (`dist/redact-rs.exe`, 7 395 328 Byte) — das ist die **Konsolenfassung**. `redact-rs-gui.exe` ist seitdem als zweite Datei dazugekommen und hier **nicht** nachgemessen; der Linux-Wert (13 MB) stammt ebenfalls aus einer früheren Messung. |
| Export der GUI identisch zur CLI | **nicht end-to-end nachgewiesen** | Beide Programme gehen inzwischen durch dasselbe Crate `redact-pipeline`, und die früher dokumentierten Abweichungen sind geschlossen ([siehe oben](#grafische-oberfläche)). Es gibt aber weiterhin keinen Test, der beide *Programme* startet und die Ausgabedateien byteweise vergleicht. |

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

Die Version steht in `.release-version` (und muss zur Version in `Cargo.toml`
passen). Wird diese Datei geändert und gepusht, baut GitHub Actions die
Binaries für Windows und Linux, erzeugt `SHA256SUMS` und veröffentlicht den
Release samt Tag:

```bash
printf '0.2.0\n' > .release-version
# Version in Cargo.toml gleichziehen
git commit -am "Release 0.2.0" && git push
```

Alternativ genügt ein Tag:

```bash
git tag -a v0.2.0 -m "redact-rs 0.2.0" && git push origin v0.2.0
```

Der Weg über die Versionsdatei existiert zusätzlich, weil in abgeschotteten
Umgebungen häufig nur auf einen bestimmten Branch gepusht werden darf und
`workflow_dispatch` über die API gesperrt ist.

## Lizenz

Wahlweise [MIT](LICENSE-MIT) oder [Apache-2.0](LICENSE-APACHE).
