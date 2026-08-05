# Änderungsverlauf

Alle nennenswerten Änderungen an redact-rs, neueste zuerst.

**Wer eine ältere Fassung einsetzt, liest die Abschnitte dazwischen.** Dieses
Werkzeug schwärzt Bankunterlagen; seine Sicherheitszusagen haben sich zwischen
den Fassungen messbar verschoben. Einträge, bei denen eine ältere Fassung
Geheimnisse in der Ausgabe stehen ließ oder ohne Warnung Erfolg meldete, sind
mit **⚠ Sicherheit** gekennzeichnet — sie sind der Grund zu aktualisieren.

Die Fassungsnummern folgen [Semantic Versioning](https://semver.org/lang/de/).
Solange die erste Stelle `0` ist, kann sich Verhalten zwischen zwei mittleren
Stellen ändern; die dafür wichtigen Punkte stehen jeweils unter *Geändert*.

Grundlage jedes Eintrags ist ein Commit in diesem Repository — nachlesbar mit
`git log <von>..<bis>`. Die Bereiche stehen unter der jeweiligen Überschrift.

---

## Unveröffentlicht

Was seit `v0.3.0` im Baum liegt und in die nächste Fassung geht.
Bereich: `git log 9aa4808..HEAD`.

### Geändert

* **Ein Release entsteht nur noch aus einem Stand, dessen CI grün ist.** Der
  Freigabeweg hat den Code vorher nie geprüft: `verify` sah Versionsnummer und
  Tag an, gebaut wurde mit einem einzigen `cargo build --release` — kein Test,
  kein Clippy, kein fmt, kein `cargo deny`. So ist v0.3.0 entstanden: der Lauf
  „Publish Release" (`30740996889`, Commit `9aa4808`) endete erfolgreich,
  während die CI-Läufe desselben Commits (`30740996865`, `30741004317`) beide
  fehlschlugen. `release.yml` ruft jetzt `ci.yml` als wiederverwendbaren
  Workflow auf und macht ihn zur Vorbedingung für Bauen und Veröffentlichen.
* **Ein Push auf einen Arbeitsbranch veröffentlicht nichts mehr** (`04da92c`).
  `publish-release.yml` hörte auf `main`, `master` **und** `claude/**`; v0.3.0
  wurde deshalb für denselben Commit zweimal ausgelöst. Der Lauf des
  Arbeitsbranches war 14 Sekunden schneller, legte den Release als
  *Vorabversion* an — so steht v0.3.0 bis heute auf GitHub — und der Lauf von
  `main` scheiterte danach am bereits vorhandenen Tag.
* **Veröffentlicht wird nur, was auf dem Default-Branch angekommen ist.** Für
  den Datei-Weg (`.release-version`) galt das schon; ein gepushter Tag
  (`git tag v0.3.1 <arbeitsstand>`) und `workflow_dispatch` mit beliebigem
  `ref` kannten dagegen gar keinen Branch. `verify` prüft die Abstammung jetzt
  für alle Wege.
* **Der geprüfte Commit ist der gebaute Commit.** `verify` löst den Ref genau
  einmal zu einer Commit-ID auf; CI-Tor, Bauauftrag und Veröffentlichung
  bekommen diese ID. Vorher hat jeder Job den Ref selbst aufgelöst — bei einem
  Branch-Ref konnten das zwei verschiedene Commits sein.
* **Vorabversionen erkennt der Release an der Versionsnummer.** `v0.4.0-rc.1`
  wird als Vorabversion gekennzeichnet, auch auf dem Tag-Weg, auf dem der
  bisherige Schalter leer blieb.
* Der Pfad des Build-Rechners steht nicht mehr in den ausgelieferten Binaries
  (`--remap-path-prefix`). Was die Prüfsummen in `SHA256SUMS-BINARIES` belegen
  und was nicht, sagen die Release-Notizen jetzt ausdrücklich; die frühere
  Formulierung („Wer nachbauen will, vergleicht diesen Hash") war eine Zusage
  ohne Deckung.
* Es gibt diesen Änderungsverlauf. Bis einschließlich v0.3.0 stand
  ausschließlich in `git log`, was sich zwischen zwei Fassungen geändert hat.

### Behoben

* Zwei Stellen im Freigabeweg, die nie erreicht wurden: `grep -v` auf
  `.release-version` endet mit 1, wenn nur Kommentare übrig bleiben — unter
  `set -euo pipefail` starb der Schritt an dieser Stelle wortlos, statt die
  vorgesehene Meldung auszugeben. Und der Zweig, der einen Release außerhalb
  des Default-Branches als Vorabversion gekennzeichnet hätte, konnte seit
  `04da92c` nicht mehr greifen; er ist entfallen.
* `SECURITY.md`: die Tabelle zu `unsafe` in Abhängigkeiten nennt jetzt auch
  die Crates, die im reinen CLI-Bau Angreiferdaten sehen (`encoding_rs`,
  `aes`, `aho-corasick`, `memchr`, `simd-adler32`), und sagt, dass ein Bau
  ohne Oberfläche die Fläche verkleinert, aber nicht beseitigt. Alle Zahlen
  neu gemessen.
* `SECURITY.md`: die Stapel-Zusammenfassung in einem Beispiel stammte aus
  einer Fassung mit zwei Zahlen; das Programm gibt seit v0.3.0 drei aus.

---

## 0.3.0 — 2026-08-02

Vier unabhängige Prüfrunden nach 0.2.0. Der überwiegende Teil der Befunde war
nicht „fehlt noch", sondern „meldet Erfolg und tut es nicht".
Bereich: `git log v0.2.0..v0.3.0`.

### ⚠ Sicherheit

* **`/ActualText` überlebte die Schwärzung.** Ein Kontoauszug aus Word oder
  InDesign trug die geschwärzte IBAN weiterhin im Klartext: die Glyphen waren
  korrekt aus dem `TJ` entfernt, das Deck-Rechteck saß richtig — und
  `pdftotext` gab die IBAN in der Voreinstellung vollständig zurück. Das
  Programm meldete „2 Schwärzungen, 40 Zeichen entfernt", Exit 0, keine
  Warnung. `/Span <</ActualText …>> BDC`, `/Alt` und `/E` schreibt jeder
  PDF/UA-Erzeuger routinemäßig. Betroffen waren alle sechs gemessenen
  Varianten. **Wer v0.2.0 mit getaggten PDFs eingesetzt hat, sollte die
  Ergebnisse nachprüfen.**
* **Verschlüsselte Eingaben umgingen die Ressourcengrenzen vollständig.** Nach
  dem Entschlüsseln lief nur noch `validate`, keine Prüfung der jetzt
  entschlüsselten Streams. Am Release-Binary gemessen: eine verschlüsselte
  Dekompressionsbombe von 33 kB kostete 342 s und 15 446 MB und endete im
  globalen OOM-Killer; eine verschlüsselte Verschachtelungsbombe von 1,3 kB
  endete mit Exit 0, geschriebener Ausgabedatei und vier nachweisbaren Lecks.
  Die Grenzen greifen jetzt hinter der Entschlüsselung.
* **Zwei weitere Wege in den OOM-Killer sind zu.** Ein `/Image`-Teilstring im
  Stream-Dictionary hebelte beide Vorprüfungen aus (130 999 Byte → SIGABRT bei
  3 961 MB; 1 122 Byte → Exit 0 mit Geheimnis in der Ausgabe), und eine
  Fächerung über Form-XObjects ebenso (2 274 Byte → SIGABRT bei 4 044 MB).
  Über Budget und Tiefenprüfung entscheidet jetzt der ausgepackte Inhalt statt
  des vom Angreifer beschreibbaren Dictionaries, und ein Aufwandskonto zählt
  Vervielfachung statt Bytes.
* **`lopdf` 0.34 → 0.42: RUSTSEC-2026-0187 ist geschlossen** (Stack Overflow
  beim Parsen tief verschachtelter Objektstrukturen — genau das, was dieses
  Werkzeug tut). Die befristete Ausnahme in `deny.toml` ist ersatzlos
  entfallen. `tests/nesting_bomb.rs` belegt, dass die Bibliothek selbst
  repariert ist und nicht nur die eigene Vorprüfung greift.
* **`--manual-regions` umging die Identitätsprüfung von `--apply-review`.**
* **Das Audit-Log bescheinigte Schwärzungen, die nicht stattgefunden hatten.**
  Eine Regionsdatei mit den Seitenzahlen 1, 2, 3 für ein dreiseitiges Dokument
  — der naheliegende Fehler eines Menschen, der ab 1 zählt — ergab Exit 0,
  keine Warnung, drei Einträge „applied" und die IBAN der ersten Seite
  unverändert in der Ausgabe.
* **Eine Seite, deren Content-Stream sich in keine Operation zerlegen ließ,**
  endete mit Exit 0 und „0 Schwärzungen", während das Geheimnis unverändert in
  der Ausgabe stand. Jetzt ist das ein Fehler.
* **In der Oberfläche verschob `Entf` während eines Zuges eine unbeteiligte
  Region auf das Ziehrechteck** — deren Fläche wurde danach nicht mehr
  geschwärzt. Der Zug merkte sich einen Index in die Regionsliste statt einer
  Kennung.

### Geändert

* **Neuer Rückgabewert 3 für weiche Warnungen.** Vier Fälle heißen wörtlich
  „wurde nicht durchsucht und kann deshalb nicht geschwärzt worden sein" und
  endeten trotzdem mit 0; im Stapel zählten solche Dateien als verarbeitet.
  Die 2 bleibt der Bedienfehler („mach es anders"), die 3 richtet sich an die
  Ergebnisprüfung („schau selbst nach"). **Für Skripte ist das die wichtigste
  Änderung dieser Fassung.**
* Die Stapel-Zusammenfassung nennt drei Zahlen statt zwei: vollständig
  geprüft, verarbeitet (aber nicht vollständig geprüft), fehlgeschlagen. Keine
  Datei zählt in zweien mit.
* Das Audit-Log unterscheidet je Region vier Befunde statt alles „applied" zu
  nennen.
* Audit-Log und Review-Datei melden Seitenzahlen einheitlich; die frühere
  Warnung, man dürfe eine Seitenzahl nicht von der einen in die andere Datei
  übernehmen, ist gegenstandslos.
* Zwei stille Verhaltensänderungen in `lopdf` 0.42 abgefangen: `/Prev`
  verschwindet beim Laden aus dem Trailer (die Warnung zu inkrementellen
  Revisionen wäre spurlos entfallen), und die Bibliothek begrenzt sich selbst
  auf Verschachtelungstiefe 100 und lässt zu tiefe Objekte kommentarlos weg —
  für die Tiefen 101 bis 128 wäre ein neues Fenster für stillen Objektverlust
  entstanden.

### Behoben

* Ein Inline-Bild mit ` EI ` in den Nutzdaten löschte den Rest der Seite.
  `lopdf::Content::decode` liefert dabei kein `Err`, sondern bricht am ersten
  unverständlichen Byte ab und gibt den Rest lautlos auf. Die Bildgrenze wird
  jetzt geprüft statt geglaubt.
* `konto_nr` backtrackte quadratisch: 1 600 Buchstaben genügten, um das
  Regex-Budget zu reißen (8 000 Zeichen: 1 473,8 ms → 10,3 ms).
* Die Oberfläche hielt die Ressourcengrenzen nicht: `load_document` las mit
  `std::fs::read`, also ohne Größengrenze — eine Sparse-Datei mit scheinbar
  6 GB kostete 6 149 MB, bevor überhaupt feststand, ob es ein PDF ist.
* Die Oberfläche prüfte eine Review-Datei pauschal statt gegen das geladene
  Dokument und lehnte damit auch passende Dateien ab.
* Das Rechteck beginnt am Druckpunkt und ist ab dem Druck sichtbar, nicht erst
  nach 6 pt Mausbewegung; die Eckgriffe sind keine Attrappen mehr.
* `--action blackout --replace-with X` verlor X, sobald in der Oberfläche auf
  „Ersetzen" umgestellt wurde. Der Ersatztext hat jetzt eine Quelle statt
  zweier.
* „Analysieren" fragt, bevor es Auswahlentscheidungen verwirft; ein Druck auf
  Tabulator legt nicht mehr sämtliche Tastenkürzel lahm.
* `--action`, `--apply-review`, `--audit-log`, `--review-out` und die
  Ressourcengrenzen gelten auch im Fenster.
* Ein Release war zeitweise unmöglich: `verify` konnte `.release-version`
  nicht mehr lesen, seit die Datei einen Kommentarkopf trägt.

---

## 0.2.0 — 2026-08-01

Der Kern hat sich an mehreren Stellen von „meldet Erfolg" zu „tut es wirklich"
bewegt. Bereich: `git log v0.1.0..v0.2.0`.

### ⚠ Sicherheit

* **Gescannte Dokumente wurden nicht wirklich geschwärzt.** Das schwarze
  Rechteck lag nur obenauf; die Bildpunkte mit der IBAN blieben in der Datei.
  Pixel unter einer Schwärzung werden jetzt überschrieben — auch in
  Form-XObjects und bei gedrehten oder skalierten Platzierungen.
* **Gesperrt gesetzte IBANs wurden nicht erkannt.** Ab `Tc` > 1,4 pt steht
  zwischen jedem Zeichen ein Leerzeichen — genau so setzen Banken Kontofelder.
* **Ein Subset-Font ohne `/ToUnicode` lieferte lesbar aussehenden Unsinn ohne
  jede Warnung**; die Kommandozeile endete mit 0, und der Nutzer hielt die
  Datei für sauber. Die Warnschwelle für unlesbare Glyphen zählte außerdem den
  Anteil am Font statt das Unlesbare selbst: 29 % eines 1000-Zeichen-Fonts
  sind 290 unlesbare Zeichen — und es blieb still.
* **Jede Review-Datei aus der Oberfläche war auf jedes beliebige PDF
  anwendbar** (leeres `sha256`-Feld wurde stillschweigend durchgewunken) —
  Rechtecke an den falschen Stellen, ohne Hinweis.
* **Das Audit-Log bescheinigte Schwärzungen, die nicht stattgefunden hatten.**
* **Der Standardlauf schwärzte zu 75 % Fehltreffer** (jetzt 0 %, gemessen an
  echten Auszugszeilen).
* **Eine 92-kB-Datei brachte den Speicher auf 5,5 GB, eine 183-kB-Datei brach
  mit SIGABRT ab** — obwohl `SECURITY.md` ausdrücklich „kontrollierter Abbruch
  statt Speicherfehler" zusagte. Die Bildschwärzung dekodierte jedes Bild der
  Seite statt nur der betroffenen, klonte den Puffer und hielt die Arbeitsliste
  über alle Seiten. 5 508 MB → 187 MB. Betroffen war nicht nur ein Angreifer:
  ein 40-seitiger A4-Scan hielt 34 MB je Seite bis zum Schluss fest.
* **Ein `/OCG`, das eine Seite über `/Resources /Properties` referenziert,
  überlebte das Entfernen von `/OCProperties` samt seinem `/Name`** — heißt
  eine Ebene „Ebene Mustermann", stand der Name hinterher noch in der Datei.
* **Eine Ligatur konnte lesbar bleiben:** deckte der Bereich genau das zweite
  Teilzeichen ab, schrieb `rebuild_show` den Originalcode wieder heraus.

### Geändert

* Die zugesagte MSRV steht auf dem gemessenen Wert **1.88** statt auf der nie
  geprüften 1.75. Der MSRV-Job ist keine Notiz mehr, sondern eine Zusicherung.
* CLI und Oberfläche teilen sich die Verarbeitungskette wirklich
  (`crates/redact-pipeline`), statt es nur im Kommentar zu behaupten. Damit
  wirkt `--padding` auch in der Oberfläche, der Export überschreibt die
  Eingabedatei nicht mehr stillschweigend, und Audit-Log wie Review-Datei
  entstehen mit 0600 statt 0644, mit Symlink- und dev/ino-Prüfung und atomarem
  Umbenennen.
* Neue Obergrenze `--max-image-mb` (Vorgabe 256 MB), geprüft **vor** dem
  Auspacken. `--max-decompressed-mb` half hier nie — es verbucht die Rohbytes
  des Streams, nicht die dekodierten Bildpunkte.
* Die Meldung „(neu kodiert, verlustbehaftet)" ist weg: außerhalb der
  Schwärzung ändert sich kein Bildpunkt, auch bei JPEG-Quelle.
* Die Buchungsliste verspricht nicht mehr, über Zeilenumbrüche zu treffen.
* Das Windows-Archiv enthält zusätzlich `redact-rs-gui.exe` ohne
  Konsolenfenster.
* Release-Notizen versprechen eine Herkunftsaussage nur dort, wo GitHub sie
  auch erzeugt: für private Repositories unter einem Benutzerkonto gibt es
  keine, und das steht jetzt so darin.

### Neu

* Muster für die SEPA-Gläubiger-Identifikationsnummer (`DE98ZZZ09999999999`),
  standardmäßig aktiv. Sie steht auf jeder Lastschriftzeile und ist bei
  Einzelunternehmern unmittelbar personenbezogen.
* Echte Seitendarstellung in der Oberfläche.

### Behoben

* `replace_page_content` war quadratisch in der Seitenzahl
  (8 000 Seiten: 35,01 s → 0,83 s).
* `dict_int` dereferenzierte nicht: ein Bild mit indirektem `/Width` ließ die
  Schwärzung abbrechen. Sieben weitere Stellen ohne Dereferenzierung
  mitbehoben.
* Die Fehlermeldung bei zu großen Bildern nannte den Filter als Ursache, statt
  die gerissene Pixelgrenze.

### Geprüft und widerlegt

Zwei gemeldete Befunde waren keine — die Gegenproben bleiben als Tests stehen:
manuelle Regionen entfernen **nicht** die falschen Zeichen (geprüft mit engem
Zeilenabstand, 90° gedreht, im Form-XObject, über mehrere `Tj`, mit
`TJ`-Kerning, `Tw`, `Tz 50` und Lücken in `/Widths`), und gedrehter Text wird
sehr wohl extrahiert (bei 0/90/180/270° entstehen aus zwei gesetzten Zeilen
genau zwei).

---

## 0.1.0 — 2026-07-31

Erste Fassung. Bereich: `git log v0.1.0`.

### Neu

* `redact-core`: Domänenmodell (Region, Redaction, BookingEntry),
  zeichengenaue Text-Runs, Konfliktauflösung mit Vorrang der Negativliste,
  Review-JSON-Format.
* `redact-pdf`: eigener Content-Stream-Interpreter, der für jedes Zeichen eine
  Bounding-Box im User-Space berechnet (inklusive Form-XObjects);
  Font-Metriken aus `/Widths`, CID-`/W`, Standard-14-Fallback, `/ToUnicode`
  und `/Differences`. Geschwärzt wird durch **Entfernen** aus dem Content-
  Stream, der entfallende Vorschub wird als `TJ`-Kerning ersetzt, darüber das
  Deck-Rechteck. Metadaten werden gestrippt, Annotationen im
  Schwärzungsbereich gelöscht, verschlüsselte oder kaputte PDFs abgelehnt.
* `redact-patterns`: elf eingebaute Muster mit IBAN-, BIC- und Luhn-Prüfung,
  konfigurierbar über YAML/JSON.
* `redact-booking`: CSV-Buchungslisten mit Vorrang der Negativliste.
* `redact-cli`: Kommandozeile mit Review-Modus, `--apply-review`, Audit-Log
  und `--json`. Review-Dateien tragen den SHA-256 der Eingabe; ein Anwenden
  auf ein anderes Dokument wird abgelehnt. Das Audit-Log führt die SHA-256 von
  Ein- und Ausgabe sowie die blockierten Treffer.
* `redact-gui`: egui-Oberfläche mit Seitenvorschau, Rechtecken per Maus,
  abwählbaren Treffern, gesperrten Negativlisten-Treffern und Drag & Drop.
  Der Export benutzt dieselbe Pipeline wie die Kommandozeile.
* Ohne `-o` entsteht das Ergebnis neben der Eingabe, mit Namenszusatz
  (`kontoauszug.pdf` → `kontoauszug_geschwaerzt.pdf`); `--force` überschreibt,
  ohne `--force` bricht der Lauf ab.
* Build- und Release-Infrastruktur: CI mit fmt, Clippy (`-D warnings`) und
  Tests unter Linux und Windows; Release-Workflow für
  `x86_64-pc-windows-msvc` und `x86_64-unknown-linux-gnu` mit `SHA256SUMS`.
  Ein Push von `.release-version` löst die Veröffentlichung aus — in
  abgeschotteten Umgebungen ist das der einzige Weg, der immer funktioniert.
