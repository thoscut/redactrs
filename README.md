# redact-rs

Ein schlankes, **lokales** CLI- und GUI-Werkzeug in Rust zum Schwärzen sensibler
Daten in PDF-Dokumenten (Bankunterlagen, Kontoauszüge, Rechnungen).

* **Manuelle Schwärzung** über Regionen (JSON oder per Maus in der GUI)
* **Automatische Schwärzung** über Regex-Muster (IBAN, BIC, Beträge, …)
* **Buchungsliste** mit Positiv- und Negativliste (CSV)
* **Echte Schwärzung**: der Text wird aus dem Content-Stream *entfernt*,
  nicht nur übermalt
* **Keine Cloud**, keine temporären Dateien, deterministische Ausgabe

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
9. [Grafische Oberfläche](#grafische-oberfläche)
10. [Architektur](#architektur)
11. [Sicherheit — was garantiert wird und was nicht](#sicherheit)
12. [Abweichungen vom Ursprungskonzept](#abweichungen-vom-ursprungskonzept)
13. [Entwicklung](#entwicklung)

---

## Schnellstart

```bash
# Beispiel-Kontoauszug erzeugen
redact-rs --write-demo kontoauszug.pdf

# Automatisch schwärzen — Ergebnis landet als kontoauszug_geschwaerzt.pdf
# direkt neben der Eingabedatei
redact-rs kontoauszug.pdf --patterns iban_de,bic,email

# Prüfen, dass wirklich nichts mehr drinsteht
pdftotext kontoauszug_geschwaerzt.pdf - | grep DE89   # → kein Treffer
```

## Installation

Fertige Binaries für Windows und Linux liegen unter
[Releases](https://github.com/thoscut/redactrs/releases).
Das Windows-Archiv enthält `redact-rs.exe`, die Beispieldateien und die
SHA-256-Prüfsummen.

Selbst bauen:

```bash
git clone https://github.com/thoscut/redactrs
cd redactrs
cargo build --release                      # mit GUI
cargo build --release --no-default-features # nur CLI, kein X11/Wayland nötig
# Binary: target/release/redact-rs
```

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
      --gui                   grafische Oberfläche starten
      --list-patterns         eingebaute Muster auflisten
      --write-demo <PDF>      Beispieldatei erzeugen
      --json                  Zusammenfassung als JSON
  -q, --quiet                 weniger Ausgabe
```

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

## Buchungsliste

CSV mit Kopfzeile. `context_before`/`context_after` sind optional und
verifizieren einen Treffer anhand seines Umfelds.

```csv
id,list_type,pattern,context_before,context_after,is_regex
b001,positive,"Musterfirma GmbH","Überweisung an",,
b002,positive,"DE89 3704 0044 0532 0130 00",,,
b003,negative,"Max Mustermann",,,
```

Regeln:

* Ein Treffer der **Negativliste blockiert** jede Schwärzung, die ihn zu
  mindestens 50 % überdeckt — auch wenn ein Muster oder die Positivliste
  anschlägt.
* Ein Treffer der **Positivliste erzwingt** eine Schwärzung, auch ohne Muster.
* **Manuelle Regionen** überstimmen die Negativliste: eine bewusste
  Nutzerentscheidung wird nicht automatisch verworfen.
* Der Vergleich ignoriert Groß-/Kleinschreibung und Leerraum — eine IBAN wird
  auch dann gefunden, wenn sie im PDF anders umbrochen ist.

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
(`(?<!…)`, `(?!…)`) ist also erlaubt.

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

Die Review-Datei enthält den SHA-256 der Eingabe. Wird sie auf ein anderes
Dokument angewendet, bricht der Lauf mit einer Fehlermeldung ab — Koordinaten
aus einer fremden Datei würden sonst an falscher Stelle schwärzen.

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

Seitennummern sind im Log 1-basiert (menschenlesbar), intern und in den
JSON-Ein-/Ausgabeformaten 0-basiert.

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
Der Export benutzt exakt dieselbe Pipeline wie die CLI.

## Architektur

```
redact-core       Domänenmodell, Traits, Konfliktauflösung, Review-Format
redact-pdf        Content-Stream-Interpreter, Textextraktion, echte Schwärzung
redact-patterns   Regex-Muster inkl. IBAN-/BIC-/Luhn-Prüfung
redact-booking    CSV-Buchungsliste, Positiv-/Negativ-Matching
redact-cli        Kommandozeile und Pipeline-Orchestrierung
redact-gui        egui-Oberfläche (optionales Feature `gui`)
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

**Was das Werkzeug garantiert**

* Geschwärzter Text ist aus dem Content-Stream entfernt — Copy-&-Paste,
  `pdftotext` und Textsuche finden ihn nicht mehr. Das ist mit
  End-to-End-Tests abgesichert.
* Text in Form-XObjects wird ebenfalls entfernt. Wird dasselbe XObject mehrfach
  platziert, wirkt die Entfernung auf alle Platzierungen — im Zweifel wird also
  eher zu viel als zu wenig geschwärzt.
* Annotationen, die in einen Schwärzungsbereich ragen, werden gelöscht.
* Metadaten werden entfernt: `/Info`, XMP (`/Metadata`), `/PieceInfo`,
  `/StructTreeRoot`.
* Alles läuft lokal und im Speicher; es werden keine temporären Dateien
  angelegt und keine Netzverbindungen aufgebaut.
* Gleiche Eingabe + gleiche Konfiguration ⇒ byteweise gleiche Ausgabe.
* Verschlüsselte oder strukturell defekte PDFs werden **abgelehnt**, nicht
  repariert.

**Was das Werkzeug NICHT garantiert**

* **Gescannte Dokumente.** Steht die sensible Information in einem Rasterbild,
  wird sie überdeckt, aber das Bild wird nicht neu kodiert — die Pixel bleiben
  in der Datei. In diesem Fall gibt redact-rs eine Warnung aus. OCR ist nicht
  Teil dieser Fassung.
* **Vektorgrafiken und eingebettete Dateien** werden nicht durchsucht.
* Die Erkennung ist eine Heuristik. Für alles, was das Haus verlässt, gilt der
  [Review-Workflow](#review-workflow) — automatische Erkennung ersetzt keine
  Sichtprüfung.

## Abweichungen vom Ursprungskonzept

Das Konzept wurde bei der Umsetzung an einigen Stellen fachlich nachgeschärft.
Alle Abweichungen sind bewusst:

| Konzept | Umsetzung | Begründung |
|---------|-----------|------------|
| `Extractor::extract → Vec<Region>` | `→ Vec<TextRun>` | Eine `Region` braucht zwingend eine `Source`; bei reiner Extraktion steht die noch gar nicht fest. `TextRun` liefert zusätzlich die Glyph-Boxen, ohne die für einen Regex-Treffer *innerhalb* einer Zeile keine exakte Box berechenbar wäre. |
| `Analyzer::analyze(&[Region])` | `analyze(&[TextRun])` | Dieselbe Begründung: Analyse braucht Text **mit** Zeichenkoordinaten. |
| Matching auf einzelnen Text-Runs | Matching auf zusammengesetzten **Zeilen** | Eine IBAN wird in der Praxis über mehrere `Tj`-Operationen ausgegeben. Ohne Zeilenbildung liegt der Recall weit unter dem Ziel. |
| `BookingMatcher`: `text.contains(pattern)` | normalisierter Vergleich (Groß-/Kleinschreibung, Leerraum) | Sonst scheitert der Vergleich an jedem abweichenden Umbruch. |
| Negativliste blockiert alles | Negativliste blockiert *nicht* manuelle Regionen | Eine bewusste Nutzerentscheidung darf nicht automatisch verworfen werden. |
| `--review` liefert eine Regionsliste | eigenes `ReviewFile` mit Version, SHA-256 und `enabled`-Schalter je Treffer | Ohne Prüfsumme lassen sich Koordinaten versehentlich auf ein fremdes Dokument anwenden. |
| GUI: `pdfium-render` für die Seitenanzeige | reiner Rust-Vorschau-Renderer, `pdfium` nur als optionales Feature | `pdfium` verlangt eine native Bibliothek zur Laufzeit und hätte das „ein einziges Binary“-Versprechen gebrochen. |
| GUI: `eframe` mit `wgpu` | `eframe` mit `glow` (OpenGL) | Deutlich kleineres Binary und kürzere Bauzeit; für diese Oberfläche reicht OpenGL vollständig aus. |
| `is_regex` in der Buchungsliste | wird abgelehnt, Verweis auf `--patterns-config` | Hält `redact-booking` abhängigkeitsfrei; echte Regexe gehören ohnehin in die Musterkonfiguration. |
| — | zusätzlich: Annotationen entfernen, Bilder-Warnung, `--json`, `--write-demo`, Prüfsummen-Validatoren | Lücken, die beim Umsetzen sichtbar wurden. |

## Stand der Akzeptanzkriterien

| Kriterium | Stand | Nachweis |
|-----------|-------|----------|
| 10 Seiten in unter 2 s (ohne OCR) | erfüllt | 10 Seiten, 450 Zeilen, 1350 Schwärzungen in ca. 70 ms (`examples/gen10.rs`) |
| Deutsche IBAN wird zuverlässig erkannt | erfüllt | Muster mit mod-97-Prüfung; gruppiert und ungruppiert getestet |
| Negativliste blockiert Schwärzung zuverlässig | erfüllt | `negative_list_prevents_redaction` prüft am fertigen PDF, dass die geschützte IBAN erhalten bleibt und die ungeschützte verschwindet |
| Copy-Paste liefert keinen sensitiven Text | erfüllt | `redaction_removes_text_from_content_stream`: nach dem Lauf ist der Text nicht mehr extrahierbar |
| Audit-Log mit SHA-256 beider Dateien | erfüllt | `review_then_apply_roundtrip` |
| Aussagekräftige Fehler bei kaputten PDFs | erfüllt | `rejects_broken_pdf_with_clear_message` |
| Manuelle Regionen (JSON) werden angewendet | erfüllt | `manual_regions_are_applied` |
| Metadaten im Ausgabe-PDF entfernt | erfüllt | `metadata_is_stripped` |
| GUI: Rechtecke ziehen, Treffer abwählen, Export | umgesetzt | Logik als reine Funktionen getestet; das Fensterverhalten selbst ist nicht automatisiert prüfbar |
| GUI-Binary unter 30 MB | erfüllt | Linux 13 MB, Windows 7,4 MB (Release, gestrippt) |
| Export der GUI identisch zur CLI | erfüllt | Test vergleicht beide Ausgaben byteweise |

Nicht umgesetzt (laut Konzept §11 außerhalb des MVP): OCR für gescannte PDFs,
Entschlüsseln passwortgeschützter PDFs, Batch-Verarbeitung, Plugin-System.

## Entwicklung

```bash
cargo test --workspace                              # alle Tests (191)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build --no-default-features                   # ohne GUI
./scripts/build-windows.sh                          # Windows-Binary (mingw)
cargo run --release -p redact-pdf --example gen10 -- gross.pdf 10
```

## Lizenz

Wahlweise [MIT](LICENSE-MIT) oder [Apache-2.0](LICENSE-APACHE).
