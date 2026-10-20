# Roadmap

Was als Nächstes kommt, was gerade läuft und was bewusst später kommt. Die
Liste ist ein Arbeitsstand, keine Zusage: was erledigt ist, wandert ins
[`CHANGELOG.md`](CHANGELOG.md) und fliegt hier raus. Was das Werkzeug heute
nicht leistet, steht ausführlich in der README unter
[Was dieses Werkzeug nicht leistet](README.md#grenzen); diese Datei sagt nur,
in welcher Reihenfolge davon etwas angegangen wird.

Stand: 20.10.2026, veröffentlicht ist
[0.7.1](https://github.com/thoscut/redactrs/releases/tag/v0.7.1).

## Seit 0.7.1 erledigt (in `main`, noch nicht veröffentlicht)

* **Zeitwächter `zm_b` ursächlich behoben**
  ([PR #9](https://github.com/thoscut/redactrs/pull/9)): die Export-Fahne
  setzt `redact_pipeline::apply` direkt hinter dem `rename`, nicht mehr die
  Oberfläche.
* **Leck #26 behoben** ([PR #10](https://github.com/thoscut/redactrs/pull/10)):
  der Erscheinungsstrom (`/AP`) einer Annotation außerhalb des
  Schwärzungsbereichs wird nach dem geschwärzten Text durchsucht und
  bereinigt. Damit sind alle vier früher dokumentierten Lecks behoben.
* **Experten-Review** ([PR #11](https://github.com/thoscut/redactrs/pull/11)):
  Lecks, Speichergrenzen, Erkennung und Rückgabewert.
* **Security-Review Runde 2**
  ([PR #12](https://github.com/thoscut/redactrs/pull/12)): Verschlüsselung,
  Seitenbaum, Träger, Lesarten.

## In Arbeit

### 0.7.2 ([PR #8](https://github.com/thoscut/redactrs/pull/8))

* Veröffentlicht die Korrekturen, die seit 0.7.1 in `main` liegen.
* `serde_yaml` wird durch `serde-saphyr` ersetzt. Damit entfällt
  `unsafe-libyaml`, das `cargo audit` als nicht mehr gepflegt meldet.
* Die übrigen Hinweise auf nicht mehr gepflegte Crates (`zvariant`,
  `zvariant_derive`, `zbus_names`) kommen nur indirekt über den Dateidialog
  (`rfd` → `ashpd` → `zbus`). Sie verschwinden erst mit einer `rfd`-Fassung,
  die auf `zbus` 6 aufsetzt; die gibt es noch nicht.

### Überarbeitete Oberfläche ([PR #6](https://github.com/thoscut/redactrs/pull/6))

* Eigener Farbsatz, Ansichtsleiste über der Seite, Seitenleiste mit
  Zwischenüberschriften, Leerzustand mit Ablagefläche, größere Griffe,
  sichtbarer Fokus.
* Offen ist die Entscheidung, ob er so gemergt wird oder ob zuerst eine der
  drei alternativen Designrichtungen (Werkbank, Prüfprotokoll, Fokus) in ihn
  einfließt.

## Als Nächstes

1. **Release 0.8** mit der neuen Oberfläche.

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
