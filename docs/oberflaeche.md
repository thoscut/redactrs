# Grafische Oberfläche

```bash
redact-rs                          # GUI ohne Dokument
redact-rs --gui kontoauszug.pdf    # GUI mit vorgeladenem PDF
```

Unter Windows genügt ein Doppelklick auf `redact-rs-gui.exe` — oder man zieht
ein PDF auf ihr Symbol. Die GUI (egui/eframe, ein einziges Binary ohne
zusätzliche Laufzeit) zeigt die Seiten so, wie `redact-render` sie rastert, mit
allen gefundenen Treffern als farbige Rahmen:
🔵 Muster · 🟢 Positivliste · 🔴 Negativliste (blockiert) · 🟠 manuell.

## Dokumente öffnen

Über „🗁 Öffnen“ (Strg+O) oder **per Drag & Drop** auf das Fenster. Bei mehreren
abgelegten Dateien gewinnt die erste PDF, Nicht-PDFs werden abgelehnt
(`classify_drop` in `crates/redact-gui/src/lib.rs` — eine reine Funktion,
damit das ohne Maus prüfbar ist). Gingen dabei von Hand gezogene Rechtecke
verloren, wird vorher gefragt.

Ist das geöffnete Dokument **verschlüsselt**, erscheint ein Fenster mit
verdeckter Eingabe. Das ist zugleich der bequemste Weg, ein Passwort *nicht*
über die Kommandozeile zu geben. Passt es nicht, bleibt die Frage stehen; das
falsche Passwort wird nicht behalten und steht in keiner Meldung.

## Rechtecke ziehen, verschieben und an den Ecken nachziehen

Ein neuer Bereich entsteht auf **zwei** Wegen. Mit der Maus durch Aufziehen —
sichtbar **ab dem Bild des Drucks** und beginnend **am Druckpunkt**, nicht erst
dort, wo egui den Zug bemerkt. Ein Klick wählt ein vorhandenes Rechteck aus.

**Ohne Maus** geht es seit v0.5.0 genauso: der Knopf „🔲 Rechteck“ in der
Leiste oder **Strg+R** legt eines in der Mitte der gezeigten Seite an
(200 × 40 pt, `NEW_REGION_SIZE` in `crates/redact-gui/src/state.rs`) und wählt
es aus. Danach schieben die **Pfeiltasten** (1 pt, mit Umschalt 10 pt), und
**Strg+Pfeil** ändert die Größe: die linke untere Ecke bleibt stehen, die
rechte obere wandert. Für den Nutzer, für den dieser Weg gebaut wurde, ist das
der ganze Weg — Aufziehen mit der Maus ist die *andere* Möglichkeit, nicht die
einzige.

Ein ausgewähltes Rechteck trägt **vier Eckgriffe** (`Handle::TopLeft` …
`BottomRight` in `crates/redact-gui/src/selector.rs`). Daran lässt es sich
nachziehen: die gegenüberliegende Ecke bleibt stehen, der Mauszeiger zeigt die
Ziehrichtung an. Die Fangzone ist mit 20 Bildschirmpunkten bewusst größer als
das gezeichnete 7-pt-Quadrat — eine Zone in Zeichnungsgröße trifft man nur
zufällig.

Zieht man ein Rechteck über einen Treffer der **Negativliste**, überstimmt die
bewusste Handbewegung die Schutzliste — und die Oberfläche **sagt es** in der
Statuszeile, statt die Wirkung stillschweigend umzukehren. Strg+Z nimmt es
zurück.

## Rückgängig und Wiederholen

**Strg+Z** und **Strg+Y**, auch über die Knöpfe ↺ / ↻ in der Leiste. Der
Verlauf hält bis zu **50 Schnappschüsse** je Richtung
(`HISTORY_LIMIT` in `crates/redact-gui/src/history.rs`); ältere fallen unten
heraus. Eine neue Änderung nach einem Rückgängig macht den
Wiederholen-Stapel ungültig. Beim Öffnen eines anderen Dokuments wird der
Verlauf verworfen — er gehört zum Inhalt, nicht zum Fenster.

Erfasst sind alle Änderungen an der Trefferliste. Es sind zehn, und die Liste
ist vollständig — sie steht hier, weil eine zu kurze Aufzählung schlimmer ist
als gar keine: wer „Analysieren“ darin nicht findet, drückt den Knopf ohne die
Zuversicht, ihn zurücknehmen zu können.

| Änderung | wo |
|---|---|
| Rechteck anlegen (Maus, Knopf „🔲 Rechteck“, Strg+R) | `add_manual_region` |
| Rechteck löschen (Entf) | `delete_selected` |
| Verschieben mit den Pfeiltasten | `move_selected` |
| **Größe ändern** mit Strg+Pfeil | `resize_selected` |
| Verschieben und Nachziehen mit der Maus | `begin_manual_edit` |
| An- und Abwählen | `set_enabled` |
| **Schwärzungsart** (Balken / weiß / Ersetzen) | `set_action` |
| **Ersatztext** ändern | `edit_replacement` |
| Review-Datei übernehmen | `apply_review_file` |
| **Analysieren** | `analyze` |

Der letzte ist der, den die frühere Aufzählung verschwieg, und der, bei dem es
am meisten kostet: **Analysieren tauscht die Trefferliste aus.** Selbst
gezogene Rechtecke trägt es ausdrücklich hinüber — die Entscheidungen an den
**Mustertreffern** dagegen nicht: abgewählt, andere Schwärzungsart, eigener
Ersatztext sind danach weg, weil deren Zeilen neu entstehen. Genau deshalb ist
es ein Verlaufsschritt, und ein Strg+Z holt den alten Stand vollständig zurück
(`r7_analysieren_ist_ein_verlaufsschritt` in
`crates/redact-gui/src/rev8_tests.rs`).

## Miniaturansichten und Zoom

Links steht eine Spalte mit **Miniaturansichten**, Seitenzahl unter jedem Bild, die Zahl der Schwärzungen als graue Plakette in seiner Ecke. Ohne Dokument fehlt die Spalte.
Sie zeigt dieselben Kleinbilder, die der Hauptbereich ohnehin anfordert — es
wird nichts doppelt gerendert. Die Spalte lässt sich am Rand zwischen 96 und
260 Punkt breit ziehen.

Der **Zoom** reicht von **0,25× bis 4×** (`MIN_ZOOM`/`MAX_ZOOM` in
`crates/redact-gui/src/state.rs`), Schrittweite 1,25×. In der Ansichtsleiste
direkt über der Seite stehen dafür vier Knöpfe: ➖ Kleiner, ➕ Größer, ⛶ Passend
(ganze Seite ins Fenster) und ⟲ 1:1 (zurück auf 100 %); daneben ein Schieberegler, der den Wert
in Prozent zeigt und sich auch eintippen lässt. Links davon blättern ⏴ Zurück
und Vor ⏵, dazwischen steht, auf welcher Seite man ist. Die obere Leiste trägt
nur noch, was mit dem Dokument geschieht; „Exportieren“ ist dort als einziger
Knopf farbig hervorgehoben. Gerendert wird
auf einem eigenen Thread, damit Seitenwechsel und Zoomen die Oberfläche nicht
anhalten; lässt sich eine Seite nicht rasterisieren, springt eine schematische
Vorschau ein und zeigt wenigstens die Lage des Textes.

## Tastaturbedienung

Die Tabelle nennt die Tasten so, wie sie gedrückt werden: steht kein Strg
davor, gilt der Eintrag für die Taste **allein**. Was dieselbe Taste mit
gehaltener Strg tut (Strg+Bild ab, Strg+Entf, Strg+Esc …), ist hier nicht
zugesagt.

| Taste | Wirkung |
|---|---|
| Strg+O | PDF öffnen |
| Strg+S | Geschwärztes PDF exportieren |
| Strg+R | Rechteck in der Mitte der gezeigten Seite anlegen und auswählen (wie der Knopf „🔲 Rechteck“) |
| Strg+Z / Strg+Y | Rückgängig / Wiederholen |
| Bild auf/ab, Pos1/Ende | blättern |
| Pfeiltasten | mit Auswahl: das Rechteck um 1 pt verschieben (mit Umschalt 10 pt) — ohne Auswahl: blättern |
| Strg+Pfeil | mit Auswahl: die Größe um 1 pt ändern (mit Umschalt 10 pt); die linke untere Ecke bleibt stehen. Ohne Auswahl: nichts — dafür genügt der Pfeil allein |
| Entf | ausgewähltes Rechteck löschen |
| Esc | Auswahl aufheben |
| Tabulator / Umschalt+Tab | von Knopf zu Knopf durch die Leiste, vorwärts bzw. rückwärts. Ausgegraute Knöpfe werden übersprungen — in beiden Richtungen |

Liegt die Auswahl auf einer Seite, die gerade nicht gezeigt wird, tun
Pfeiltasten und Strg+Pfeil **nichts** und sagen es in der Statuszeile — mit dem
Verb, um das es ging („Nicht verschoben …“ bzw. „Größe nicht geändert …“) und
mit dem Ausweg. Gibt es die genannte Seite im Dokument gar nicht (das kann nur
eine Review- oder Regionsdatei mitbringen), nennt die Absage einen anderen
Ausweg: dorthin lässt sich nicht blättern.

Unter macOS tritt die Befehlstaste an die Stelle von Strg. **Liegt der Fokus in
einem Textfeld, gehören alle Tasten dorthin** und nirgendwo sonst hin — sonst
löschte die Rücktaste im Feld „Ersetzen“ die ausgewählte Region. Ein Knopf mit
Fokus (nach einem Druck auf Tabulator) ist dabei kein Textfeld; die Kürzel
wirken dort weiter (`a_tab_press_does_not_kill_every_shortcut` in
`crates/redact-gui/src/app.rs`).

## Nachprüfung nach dem Export

Nach jedem Export liest die Oberfläche die geschriebenen Bytes zurück und
sucht darin mit `redact_pdf::leaks_many` die Texte, die sie gerade geschwärzt
hat — dieselbe Prüfung wie [`--check-leaks`](pruefen.md#pruefen), nur ohne Tippen, weil
die Oberfläche die Suchbegriffe schon kennt. Sie läuft im Hintergrund; das
Ergebnis steht in der Statuszeile, ein Fund zusätzlich ganz vorn in den
Warnungen.

Gesucht werden höchstens 1 000 Begriffe je Nachprüfung — dieselbe Decke wie
bei `--check-leaks` (`redact_core::MAX_CHECK_NEEDLES`), und gezählt wird
**jede Schreibweise**, die in einer geschwärzten Zeile steht. Was darüber
liegt, wird gesagt und nicht verschwiegen: die Zeile nennt die Zahl der nicht
gesuchten Texte. Geprüft ist
*diese Liste*, nicht die Datei — ein selbst gezogenes Rechteck hat keinen
bekannten Text, und die Zeile nennt die Zahl solcher Rechtecke.

Ein Text, den Sie bewusst stehen lassen, deckt nur sich selbst: steht dieselbe
Zeichenfolge aus einer geschwärzten Zeile noch **wörtlich** in der Ausgabe,
wird sie gemeldet. Nur wenn allein die Fassung ohne Leerraum trifft und eine
stehen gelassene Zeile dieselbe Zeichenfolge trägt, zählt der Fund nicht als
Leck — die Zeile sagt, wie viele Texte das betrifft. Sind beide **wörtlich**
gleich, wird der Text gar nicht gesucht, und die Statuszeile sagt, dass sie
über ihn nichts aussagt — die Warnung dazu bleibt stehen.

Bleiben Stellen der Datei ungeprüft, sagt die Statuszeile, wie viele es sind,
und nennt **bis zu drei davon** beim Namen samt Grund; der Rest wird gezählt
(„… und N weitere“). „Nicht gefunden“ ist dann keine Aussage.

## Trefferliste

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
Einzelheiten unter [Automatische Funde abschalten](kommandozeile.md#automatische-funde-abschalten).

Über „🗄 Review speichern“ / „📋 Review laden“ geht der Stand als JSON hinaus
und wieder herein — mit der SHA-256-Prüfsumme des Dokuments, geprüft von
**derselben** Funktion, die `--apply-review` benutzt. Eine Review-Datei zu einem
anderen PDF wird abgelehnt.

Ob die Oberfläche hell oder dunkel startet, sagt `theme` in der
[Einstellungsdatei](kommandozeile.md#einstellungsdatei); umschalten lässt es sich jederzeit
rechts in der Leiste oben (🌙/☀).

Es gibt zwei **Arbeitsansichten**, umschaltbar mit dem Knopf daneben (⛶/☰)
und voreingestellt über `layout` in der Einstellungsdatei:

* **Werkbank** (`werkbank`, Vorgabe) — feste Leisten: oben die
  Dokument-Aktionen, links die Miniaturen, rechts Treffer und Auswahl, über
  der Seite Blättern und Zoom.
* **Fokus** (`fokus`) — die Seite füllt das Fenster; Datei, Treffer,
  Miniaturen und Werkzeuge schweben als Karten darüber. Die Trefferkarte lässt
  sich einklappen, dann bekommt die Seite ihren Platz.

Beide Ansichten haben dieselben Knöpfe mit denselben Texten und Tasten; die
Statuszeile mit Warnungen bleibt in beiden eine feste Leiste. Die vier Trefferfarben erreichen in beiden Themen mindestens
3:1 Kontrast (WCAG 1.4.11), geprüft gegen die Flächen, die egui wirklich malt.

## GUI und CLI teilen sich die Verarbeitungskette — und das ist geprüft

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
`crates/redact-cli/tests/cli_and_gui_agree.rs`
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
