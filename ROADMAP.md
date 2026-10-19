# Roadmap

Was als Nächstes kommt, was gerade läuft und was bewusst später kommt. Die
Liste ist ein Arbeitsstand, keine Zusage: was erledigt ist, wandert ins
[`CHANGELOG.md`](CHANGELOG.md) und fliegt hier raus. Was das Werkzeug heute
nicht leistet, steht ausführlich in der README unter
[Was dieses Werkzeug nicht leistet](README.md#grenzen); diese Datei sagt nur,
in welcher Reihenfolge davon etwas angegangen wird.

Stand: 19.10.2026, veröffentlicht ist
[0.7.1](https://github.com/thoscut/redactrs/releases/tag/v0.7.1).

## In Arbeit

### 0.7.2 — Abhängigkeiten aufräumen

* `serde_yaml` wird durch `serde-saphyr` ersetzt. Damit entfällt
  `unsafe-libyaml`, das `cargo audit` als nicht mehr gepflegt meldet.
* Die übrigen Hinweise auf nicht mehr gepflegte Crates (`zvariant`,
  `zvariant_derive`, `zbus_names`) kommen nur indirekt über den Dateidialog
  (`rfd` → `ashpd` → `zbus`). Sie verschwinden erst mit einer `rfd`-Fassung,
  die auf `zbus` 6 aufsetzt; die gibt es noch nicht. Bis dahin stehen sie als
  bekannte Hinweise in der CI.

### Überarbeitete Oberfläche ([PR #6](https://github.com/thoscut/redactrs/pull/6))

* Eigener Farbsatz, Ansichtsleiste über der Seite, Seitenleiste mit
  Zwischenüberschriften, Leerzustand mit Ablagefläche, größere Griffe,
  sichtbarer Fokus. Die CI ist grün, der PR lässt sich konfliktfrei mergen.
* Offen ist die Entscheidung, ob er so gemergt wird oder ob zuerst eine der
  drei alternativen Designrichtungen (Werkbank, Prüfprotokoll, Fokus) in ihn
  einfließt.

## Als Nächstes

1. **Zeitwächter `zm_b` unter Windows ursächlich beheben.** Der Test
   `zm_b_zwischen_rename_und_fahne_passt_ein_bild` misst das Fenster zwischen
   dem `rename` der Ausgabedatei und dem Setzen der Fahne in der Oberfläche
   und ist auf geteilten Windows-Läufern einmal über 16,7 ms gekommen. Die
   Korrektur, die der Test selbst nennt: die Fahne **in** `redact-pipeline`
   unmittelbar hinter dem `rename` setzen. Das ändert die API von `apply`, und
   der Test wird umgebaut. Eigener PR, weil es die Sicherheitslogik berührt.
2. **Bekanntes Leck: Erscheinungsströme außerhalb des Schwärzungsbereichs.**
   Der `/AP` einer Annotation, die nicht in eine Schwärzung ragt, wird nicht
   geprüft; ihr Text bleibt in der Datei. Festgehalten von
   `appearance_stream_outside_the_redaction_is_also_cleaned` (Aufgabe #26,
   `#[ignore]`) und dem Kanarienvogel in
   `crates/redact-pdf/tests/known_leaks.rs`. Es ist das letzte der vier
   früher dokumentierten Lecks.
3. **Release 0.8** mit der neuen Oberfläche und, falls bis dahin fertig, den
   beiden Punkten darüber.

## Später, nach Bedarf

Aus der Liste der Grenzen in der README. Jeder Punkt ist ein eigenes
Vorhaben; welcher zuerst kommt, hängt davon ab, welche Dokumente in der
Praxis daran scheitern.

* **Treffer über einen Zeilenumbruch hinweg** (eine IBAN, die auf zwei Zeilen
  umbricht). Muster und Buchungsliste sehen heute nur eine Textzeile.
* **Namen, Adressen, Freitexte.** Es gibt kein Muster und keine
  Named-Entity-Erkennung; heute geht das nur über Buchungsliste, manuelle
  Region oder die GUI.
* **OCR für gescannte Seiten.** Text in Rasterbildern wird nicht gefunden,
  nur von Hand gezogene Bereiche werden geschwärzt. Der `/Alt` eines Bildes
  gehört zum selben blinden Fleck.
* **JPEG 2000 und CCITT-Fax dekodieren**, damit eine Schwärzung darauf nicht
  abbricht.
* **Kein Kernabzug unter Windows.** Unter Linux und den übrigen
  Unix-Systemen ist er abgeschaltet, unter Windows ist nichts umgesetzt.
* **Vektorgrafiken und Type3-Fonts.** Als Pfad gezeichneter Text ist
  unsichtbar, Type3-Fonts werden nur genähert behandelt.

## Wie ein Punkt hier landet

Eine neue Fehlerklasse aus einer Prüfrunde kommt in die
[`PRUEFLISTE.md`](PRUEFLISTE.md), nicht hierher. Hierher gehört, was eine
Entscheidung braucht oder über mehr als einen PR geht.
