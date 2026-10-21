# Roadmap

Was als Nächstes kommt, was gerade läuft und was bewusst später kommt. Die
Liste ist ein Arbeitsstand, keine Zusage: was erledigt ist, wandert ins
[`CHANGELOG.md`](CHANGELOG.md) und fliegt hier raus. Was das Werkzeug heute
nicht leistet, steht ausführlich in der README unter
[Was dieses Werkzeug nicht leistet](README.md#grenzen); diese Datei sagt nur,
in welcher Reihenfolge davon etwas angegangen wird.

Stand: 21.10.2026, veröffentlicht ist
[0.7.2](https://github.com/thoscut/redactrs/releases/tag/v0.7.2).

## In Arbeit

### 0.8.0

* Veröffentlicht die überarbeitete Oberfläche
  ([PR #6](https://github.com/thoscut/redactrs/pull/6)): eigener Farbsatz,
  Ansichten „Werkbank“ und „Fokus“ (Schlüssel `layout`), Barrierefreiheit.
  Einzelheiten im [`CHANGELOG.md`](CHANGELOG.md).

## Bekannt, wartet auf andere

* Die Hinweise von `cargo audit` auf nicht mehr gepflegte Crates (`zvariant`,
  `zvariant_derive`, `zbus_names`) kommen nur indirekt über den Dateidialog
  (`rfd` → `ashpd` → `zbus`). Sie verschwinden erst mit einer `rfd`-Fassung,
  die auf `zbus` 6 aufsetzt; die gibt es noch nicht.

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
