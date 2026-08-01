# redact-rs

Ein schlankes, **lokales** CLI- und GUI-Werkzeug in Rust zum Schwärzen sensibler
Daten in PDF-Dokumenten (Bankunterlagen, Kontoauszüge, Rechnungen).

* **Manuelle Schwärzung** über Regionen (JSON oder per Maus in der GUI)
* **Automatische Schwärzung** über Regex-Muster (IBAN, BIC, Beträge, …)
* **Buchungsliste** mit Positiv- und Negativliste (CSV)
* **Echte Schwärzung**: der gefundene Text wird aus dem Content-Stream
  *entfernt*, nicht nur übermalt
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
4. [Buchungsliste](#buchungsliste)
5. [Manuelle Regionen](#manuelle-regionen)
6. [Eigene Patterns](#eigene-patterns)
7. [Review-Workflow](#review-workflow)
8. [Audit-Log](#audit-log)
9. [Prüfen, ob die Schwärzung gewirkt hat](#pruefen)
10. [Was dieses Werkzeug nicht leistet](#grenzen)
11. [Grafische Oberfläche](#grafische-oberfläche)
12. [Architektur](#architektur)
13. [Sicherheit — was zugesichert wird](#sicherheit)
14. [Abweichungen vom Ursprungskonzept](#abweichungen-vom-ursprungskonzept)
15. [Entwicklung](#entwicklung)

---

## Schnellstart

```bash
# Beispiel-Kontoauszug erzeugen
redact-rs --write-demo kontoauszug.pdf

# Automatisch schwärzen — Ergebnis landet als kontoauszug_geschwaerzt.pdf
# direkt neben der Eingabedatei
redact-rs kontoauszug.pdf --patterns iban_de,bic,email
```

Und dann **nicht** aufhören. Der Lauf oben entfernt IBAN, BIC und E-Mail-Adresse
— den Kontoinhaber, die Kontonummer, die Telefonnummer und die Steuer-ID lässt
er stehen, weil kein aktives Muster darauf passt. Wie man das feststellt, steht
unter [Prüfen, ob die Schwärzung gewirkt hat](#pruefen).

## Installation

Fertige Binaries für Windows und Linux liegen unter
[Releases](https://github.com/thoscut/redactrs/releases).
Das Windows-Archiv enthält `redact-rs.exe`, die Beispieldateien und die
SHA-256-Prüfsummen.

Selbst bauen:

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
`--output-suffix` ändern. Eine vorhandene Datei wird nur mit `--force`
überschrieben — ein zweiter Lauf soll ein bereits geprüftes Ergebnis nicht
unbemerkt ersetzen. Für `--review` gilt dasselbe Schema
(`kontoauszug_review.json`).

```text
redact-rs [EINGABE.pdf] [OPTIONEN]

  -o, --output <PDF>          Ausgabedatei (Standard: neben der Eingabe)
      --output-suffix <TEXT>  Namenszusatz (Standard: _geschwaerzt)
  -f, --force                 vorhandene Ausgabedatei überschreiben
      --patterns <IDs>        Muster, kommagetrennt (z.B. iban_de,bic)
      --no-patterns           gar keine Muster anwenden
      --patterns-config <F>   eigene Musterkonfiguration (YAML oder JSON)
      --booking-list <CSV>    Buchungsliste (Positiv-/Negativliste)
      --manual-regions <JSON> manuell festgelegte Regionen
      --review                nur analysieren, nichts schwärzen
      --review-out <JSON>     Zieldatei des Review-Exports
      --apply-review <JSON>   geprüfte Review-Datei anwenden
      --audit-log <JSON>      Audit-Log schreiben
      --action <ART>          blackout (Standard) | whiteout | replace
      --replace-with <TEXT>   Ersatztext für --action replace
      --padding <PUNKT>       Rand um jede Schwärzung (Standard: 1.0)
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

Rückgabewerte: `0` Erfolg, `1` Verarbeitungsfehler, `2` Bedienfehler.

### Eingebaute Muster

| ID | Beschreibung | Standard |
|----|--------------|----------|
| `iban_de` | Deutsche IBAN, mit Prüfsummenkontrolle (mod 97) | an |
| `iban_intl` | Internationale IBAN | aus |
| `konto_nr` | Kontonummer (6–10 Ziffern, Heuristik) | an |
| `blz` | Bankleitzahl (8 Ziffern) | an |
| `bic` | BIC/SWIFT mit Länderkennung-Prüfung | an |
| `amount_eur` | Geldbetrag in deutscher Schreibweise | an |
| `date_de` | Datum TT.MM.JJJJ | an |
| `credit_card` | Kreditkartennummer, Luhn-geprüft | an |
| `steuer_id` | Steuerliche Identifikationsnummer | aus |
| `email` | E-Mail-Adresse | an |
| `phone_de` | Deutsche Telefonnummer | an |

Muster mit Prüfsumme (IBAN, BIC, Kreditkarte) verwerfen Treffer, die die
Prüfung nicht bestehen — das drückt die Fehlalarmquote deutlich.
Ausgeschaltete Muster lassen sich mit `--patterns steuer_id` gezielt aktivieren.

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
  den Blöcken stehen oder gar keine.
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
  "timestamp": "2026-07-31T18:12:08Z",
  "tool": { "name": "redact-rs", "version": "0.1.0" },
  "input":  { "path": "kontoauszug.pdf", "sha256": "466c0af4…" },
  "output": { "path": "geschwaerzt.pdf", "sha256": "4768dae5…" },
  "redactions": [
    {
      "page": 1,
      "rect": { "ll": { "x": 100.9, "y": 732.8 }, "ur": { "x": 239.9, "y": 742.5 } },
      "action": "blackout",
      "reason": "pattern: iban_de (confidence 0.99)",
      "source": "auto"
    }
  ],
  "blocked_by_negative_list": [
    { "page": 2, "pattern": "Max Mustermann", "booking_id": "b003" }
  ],
  "metadata_stripped": true
}
```

### Achtung: `page` bedeutet in den beiden Dateien nicht dasselbe

Wer `review.json` und `audit.json` nebeneinander legt — und genau dazu lädt der
Workflow ein —, liest zwei verschiedene Zählweisen desselben Feldes:

| Datei | Feld | erste Seite ist |
|---|---|---|
| `review.json` → `items[].region.page` | `page` | `0` |
| `review.json` → `blocked_by_negative_list[].page` | `page` | `0` |
| `audit.json` → `redactions[].page` | `page` | `1` |
| `audit.json` → `blocked_by_negative_list[].page` | `page` | `1` |
| Konsolenausgabe („Seite 1: …“) | — | `1` |
| `--manual-regions`-JSON (Eingabe) | `page` | `0` |

Derselbe blockierte Treffer erscheint also als `"page": 0` in der Review-Datei
und als `"page": 1` im Audit-Log. Das ist Absicht (Log menschenlesbar,
Austauschformate maschinennah), aber es ist eine Stolperfalle: eine Seitenzahl
aus dem Audit-Log darf man **nicht** in eine Regionsdatei übernehmen.

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
   Namen grundsätzlich, die Steuer-ID (Muster standardmäßig aus), hier zusätzlich
   Kontonummer und Telefonnummer, weil `--patterns iban_de,bic,email` sie
   ausgeschlossen hat — steht unangetastet in der Datei und wird von einem
   `grep DE89` nie berührt.
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

* **Muster sind Heuristiken.** `konto_nr` (Konfidenz 0.40) und `blz` (0.50)
  treffen auf beliebige Ziffernfolgen passender Länge; `steuer_id` und
  `iban_intl` sind standardmäßig **aus** und werden ohne `--patterns` gar nicht
  angewandt.
* **Fonts ohne `/ToUnicode`.** Steht der Text in einer Schrift, deren Kodierung
  sich nicht auflösen lässt, ist er für die Analyse unsichtbar — und wird
  deshalb nicht geschwärzt. Der Interpreter erzeugt dafür zwar eine Warnung,
  aber **sie erreicht den Nutzer nicht**: `Extractor::extract` wirft die
  Warnungen weg (`crates/redact-pdf/src/extract.rs:246` gibt nur `.0` des
  Tupels zurück), und weder Kommandozeile noch Oberfläche rufen
  `extract_with_warnings` auf. Was der Lauf meldet, sind ausschließlich die
  Warnungen der Schwärzung selbst.

### Gescannte Dokumente

* **Keine OCR.** Steht die sensible Information als Pixel in einem Rasterbild,
  findet die Analyse sie nicht.
* **Bilder werden nicht neu kodiert.** Eine Schwärzung über einem Bild zeichnet
  ein deckendes Rechteck *darüber*; die Bilddaten bleiben vollständig in der
  Datei und lassen sich mit jedem Extraktionswerkzeug wieder herausholen. Für
  gescannte Seiten ist die Ausgabe dieses Werkzeugs damit **nicht** sicher.
* **Die Warnung erscheint ausgerechnet dann nicht, wenn sie am wichtigsten
  wäre.** Der Hinweis „Seite enthält Rasterbilder …“ wird nur für Seiten
  erzeugt, auf denen mindestens eine Schwärzung stattfindet. Ein reiner Scan
  hat keinen extrahierbaren Text, also keine Treffer, also keine Schwärzung —
  und damit auch keine Warnung. Der Lauf sieht so aus:

  ```console
  $ redact-rs scan.pdf -o scan_geschwaerzt.pdf
  Eingabe:            scan.pdf
  Seiten:             1
  Schwärzungen:       0
  Entfernte Zeichen:  0
  Ausgabe:            scan_geschwaerzt.pdf
  ```

  Kein Fehler, keine Warnung, eine Ausgabedatei, die alles enthält. Wer ein
  gescanntes Dokument bearbeitet, muss die Seiten selbst ansehen und die
  Bereiche von Hand ziehen — und danach in Kauf nehmen, dass die Pixel
  darunter erhalten bleiben.

### Bekannte Lecks in der Schwärzung

* **Inline-Bilder zerreißen den Seiteninhalt.** Steht im Content-Stream ein
  Inline-Bild (`BI … ID … EI`), verliert der Parser alles dahinter. Der Text
  danach wird weder gefunden noch geschwärzt — **und er geht beim Neuschreiben
  der Seite verloren**. Aus einer Schwärzung wird also zusätzlich ein
  Datenverlust. **[Testfall]**
  (`text_after_an_inline_image_is_still_found_by_the_extractor`,
  `unrelated_text_after_an_inline_image_survives_the_rewrite`)
* **Formularfeld-Werte werden nicht geschwärzt.** Der Wert eines AcroForm-Feldes
  (`/V`) bleibt unangetastet, auch wenn das Feld mitten im Schwärzungsbereich
  liegt. **[Testfall]** (`form_field_value_is_redacted_too`)
* **Erscheinungsströme außerhalb des Schwärzungsbereichs.** Annotationen, die
  in einen Schwärzungsbereich ragen, werden entfernt. Der Erscheinungsstrom
  einer Annotation, die *nicht* hineinragt, wird nicht geprüft — ihr Text bleibt
  in der Datei. **[Testfall]**
  (`appearance_stream_outside_the_redaction_is_also_cleaned`)
* **Vektorgrafiken und eingebettete Dateien** werden nicht durchsucht.
* **Type3-Fonts** liefern kein Fontprogramm und werden nur genähert behandelt.

Die genannten vier Tests stehen in
[`crates/redact-pdf/tests/known_leaks.rs`](crates/redact-pdf/tests/known_leaks.rs)
und tragen ein `#[ignore]` mit Aufgabennummer, damit die Suite grün bleibt und
der Defekt trotzdem dokumentiert ist. Nachstellen:

```bash
cargo test -p redact-pdf --no-default-features -- --ignored
# 0 passed; 4 failed  →  alle vier Lecks bestehen weiterhin
```

Solange diese vier fehlschlagen, bestehen die Lecks. Schlägt einer davon
plötzlich *nicht* mehr fehl, ist das Leck behoben und das `#[ignore]` kann weg.

### Verarbeitung

* **Verschlüsselte PDFs** werden abgelehnt, nicht entschlüsselt.
* **Strukturell defekte PDFs** werden abgelehnt, nicht repariert — eine
  „reparierte“ Datei könnte Inhalte enthalten, die der Analyse entgehen.
* **Keine Stapelverarbeitung**, kein Plugin-System.
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
4. **Bei gescannten Seiten nicht auf dieses Werkzeug bauen.** Dort ist eine
   Neuausgabe der Seite als Bild ohne die betroffenen Pixel der einzige sichere
   Weg — und den kann redact-rs nicht.
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

### Die GUI ist **nicht** dieselbe Pipeline wie die CLI

`redact-gui` hat keine Abhängigkeit auf `redact-cli`; der Ablauf ist dort
eigenständig implementiert. Er führt dieselben drei Schritte in derselben
Reihenfolge aus (schwärzen → Metadaten strippen → schreiben), aber es gibt
keinen Test, der beide Programme ausführt und die Ergebnisse vergleicht.
Bekannte Unterschiede:

| | CLI | GUI |
|---|---|---|
| Polsterung um jede Schwärzung | `--padding` (Vorgabe 1.0) | fest 1.0 |
| Eingabegrenzen | über `--max-…` einstellbar | fest auf den Vorgaben |
| vorhandene Ausgabedatei | nur mit `--force` | wird überschrieben (der Dateidialog fragt vorher) |
| Audit-Log und Review-Datei | über den zentralen Schreibpfad, Modus 0600 | mit `std::fs::write`, also mit den Vorgaberechten des Kontos |

Wer ein nachvollziehbares, prüfbares Ergebnis braucht, nimmt die
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
redact-cli        Kommandozeile und Pipeline-Orchestrierung
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
* Text in Form-XObjects wird ebenfalls entfernt. Wird dasselbe XObject mehrfach
  platziert, wirkt die Entfernung auf alle Platzierungen — im Zweifel wird also
  eher zu viel als zu wenig geschwärzt.
* Annotationen, die in einen Schwärzungsbereich ragen, werden gelöscht.
* Metadaten werden entfernt: `/Info` aus dem Trailer, XMP (`/Metadata`) aus
  Katalog und Seiten, `/PieceInfo`, `/StructTreeRoot` samt `/MarkInfo` und
  `/StructParents` sowie `/Names` und `/Dests` des Katalogs (benannte Ziele,
  JavaScript, eingebettete Dateien — Preis: benannte Sprünge im Dokument
  funktionieren danach nicht mehr).
* Alles läuft lokal und im Speicher; es werden keine Netzverbindungen
  aufgebaut. Beim Schreiben entsteht genau **eine** temporäre Datei, und zwar
  im Zielverzeichnis (`.<name>.redact-<pid>-<n>.tmp`); sie wird per `rename`
  zur Ausgabedatei, damit nie eine halb geschriebene Datei sichtbar wird. In
  `/tmp` landet nichts.
* Gleiche Eingabe + gleiche Konfiguration ⇒ byteweise gleiche Ausgabe.
* Verschlüsselte oder strukturell defekte PDFs werden **abgelehnt**, nicht
  repariert.
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
| GUI-Binary unter 30 MB | erfüllt | Windows 7,4 MB nachgemessen (`dist/redact-rs.exe`, 7 395 328 Byte). Der Linux-Wert (13 MB) stammt aus einer früheren Messung und wurde bei dieser Prüfung **nicht** nachgemessen. |
| Export der GUI identisch zur CLI | **nicht nachgewiesen** | Der vorhandene Test (`export_matches_a_hand_built_pipeline_of_the_same_steps`) vergleicht den GUI-Export mit einem *im Test nachgebauten* Ablauf, nicht mit der CLI. `redact-gui` hängt nicht von `redact-cli` ab. Bekannte Unterschiede: [siehe oben](#grafische-oberfläche). |

Nicht umgesetzt (laut Konzept §11 außerhalb des MVP): OCR für gescannte PDFs,
Entschlüsseln passwortgeschützter PDFs, Batch-Verarbeitung, Plugin-System.

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
