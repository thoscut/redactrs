# Architektur und Stand der Umsetzung

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
| Copy-Paste liefert keinen sensitiven Text | erfüllt für gefundenen Text | geprüft mit `redact_pdf::leaks` an der geschriebenen Datei, nicht mit dem eigenen Extraktor. Was die Analyse nicht findet, wird nicht geschwärzt — siehe [Grenzen](grenzen.md#grenzen). |
| Audit-Log mit SHA-256 beider Dateien | erfüllt | `review_then_apply_roundtrip` |
| Aussagekräftige Fehler bei kaputten PDFs | erfüllt | `rejects_broken_pdf_with_clear_message` |
| Manuelle Regionen (JSON) werden angewendet | erfüllt | `manual_regions_are_applied` |
| Das Audit-Log bescheinigt nur Gemessenes | erfüllt | `a_region_on_a_page_that_does_not_exist_is_not_logged_as_applied`, `a_region_without_text_under_it_is_covered_not_missing`, `a_degenerate_padding_is_not_logged_as_a_redaction` — jeweils mit Gegenprobe |
| Review-Datei wirkt nur auf ihr eigenes Dokument | erfüllt, auch hinter `--manual-regions` | `review_file_from_another_document_is_rejected`, `a_foreign_review_file_behind_manual_regions_is_refused_too` |
| Metadaten im Ausgabe-PDF entfernt | erfüllt | `metadata_is_stripped`, `names_tree_is_removed_as_the_module_documentation_promises` |
| GUI: Rechtecke ziehen, Treffer abwählen, Export | umgesetzt, darüber hinaus | dazu Eckgriffe, Rückgängig/Wiederholen, Miniaturansichten, Zoom 0,25×–4×, Tastaturbedienung, Drag & Drop und die Schwärzungsart je Treffer ([Details](oberflaeche.md#grafische-oberfläche)). Alle Rechnungen und Zustandsübergänge liegen als reine Funktionen in `state.rs`, `selector.rs`, `viewer.rs`, `history.rs`, `focus.rs` und sind ohne Fenster getestet; das Fensterverhalten selbst ist nicht automatisiert prüfbar |
| GUI-Binary unter 30 MB | erfüllt, alle vier Artefakte — mit reichlich Abstand | Das größte Artefakt ist Linux/glibc mit **14,3 MB**, also unter der Hälfte der Grenze. Eine genaue Zahl je Artefakt steht hier bewusst nicht mehr: sie veraltet mit jeder Fassung, und die Zusage ist die Grenze, nicht der Messwert. Wer die Zahlen der Fassung braucht, die er heruntergeladen hat, misst sie in einer Zeile — siehe darunter. |
| Export der GUI identisch zur CLI | erfüllt | `cli_and_gui_agree.rs` startet das gebaute Binary als eigenen Prozess und daneben `AppState` (laden → analysieren → exportieren) und vergleicht Ausgabedatei **byteweise** sowie das Audit-Log Feld für Feld — inzwischen **sieben** Fälle: Vorgabe-Aktion, `--action replace`, `--disable-pattern`, `--no-patterns`, die Review-Datei in beiden Varianten und die Gegenprobe `a_deviating_window_would_be_caught`, die die früher bestandene Abweichung nachstellt und anschlagen muss (Tabelle unter [Grafische Oberfläche](oberflaeche.md#grafische-oberfläche)). Nachgemessen: `test result: ok. 7 passed`. |

**Die Binärgrößen selbst nachmessen** — zwei Zeilen, nichts wird gebaut:

```bash
tar -xzf redact-rs-<version>-x86_64-linux.tar.gz  && stat -c '%s %n' redact-rs
unzip -oq redact-rs-<version>-x86_64-windows.zip  && stat -c '%s %n' redact-rs*.exe
```

An v0.4.0 ergibt das (heruntergeladen und gegen `SHA256SUMS-BINARIES` geprüft,
`sha256sum -c` meldet 4× OK):

| Artefakt | Byte | ≈ |
|---|---:|---:|
| `redact-rs.exe` | 10 766 336 | 10,3 MB |
| `redact-rs-gui.exe` | 9 999 360 | 9,5 MB |
| Linux/glibc `redact-rs` | 15 041 568 | 14,3 MB |
| Linux/musl `redact-rs` | 5 055 416 | 4,8 MB |

Gegenüber den für v0.3.0 festgehaltenen Werten sind alle vier gewachsen, am
stärksten Linux/glibc (14 900 000 → 15 041 568 Byte); die v0.3.0-Zahlen sind
hier nicht neu nachgemessen, sie stammen aus dem Eintrag im Änderungsverlauf.
Diese Zahlen gelten für **diese** Fassung und für keine andere — das ist der
Grund, warum in der Tabelle darüber nur noch die Grenze steht.

Von dem, was das Konzept in §11 außerhalb des MVP führt, sind das
[Entschlüsseln passwortgeschützter PDFs](kommandozeile.md#verschluesselte-pdfs) und die
[Stapelverarbeitung](kommandozeile.md#stapel) inzwischen umgesetzt. Nicht umgesetzt: OCR für
gescannte PDFs und ein Plugin-System.
