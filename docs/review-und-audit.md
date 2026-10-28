# Review-Workflow und Audit-Log

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
hier wortlos durch. Ein nacktes Regions-Array (siehe
[Manuelle Regionen](kommandozeile.md#manuelle-regionen)) bleibt ungeprüft —
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
    "off_page": 0,
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
[Bildschwärzung](bilder.md#bilder).

`patterns` sagt, wonach der Lauf **nicht** gesucht hat: `all_disabled` für
`--no-patterns`, `disabled` für jedes einzeln abgeschaltete Muster (siehe
[Automatische Funde abschalten](kommandozeile.md#automatische-funde-abschalten)). Das Feld steht
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
aus dem Content-Stream entfernt hat. **Fünf** Befunde sind möglich:

| Befund | Was geschah | Ist das in Ordnung? |
|---|---|---|
| `applied` | Mindestens ein Zeichen entfernt. | Ja — und nur das bezeugt eine Schwärzung. |
| `covered` | Deck-Rechteck gezeichnet, aber kein Zeichen getroffen. | **Kommt darauf an.** Über einer Grafik oder einem Rasterbild steht kein Text; die Bildpunkte werden trotzdem überschrieben. Liegen die Koordinaten dagegen daneben, bleibt der Text darunter lesbar. |
| `degenerate` | Rechteck ist nach `--padding` leer, die Region wurde übersprungen. | Nein. Ein negatives `--padding` verkleinert jeden Bereich. |
| `missing_page` | Die genannte Seite gibt es im Dokument nicht. Es geschah **gar nichts**. | Nein. Fast immer die verwechselte Zählweise — siehe unten. |
| `off_page` | Die Seite gibt es, das Rechteck liegt aber **vollständig neben dem Blatt**. Kein Zeichen kann darunter liegen, und was außerhalb der MediaBox gezeichnet wird, sieht kein Betrachter. | Nein. Fast immer Koordinaten aus einer Review- oder Regionsdatei, die zu einem anders großen Blatt gehören. |

`off_page` gibt es seit **0.6.0**. Bis dahin kannte ihn nur die Oberfläche
(`HitOutcome::OffPage`); im Audit-Log lief derselbe Fall als `covered` mit —
also unter dem Befund, der auch „über einer Grafik steht eben kein Text“
bedeutet und deshalb häufig harmlos ist. Wer ein Log aus 0.5.0 oder älter
liest, findet das Feld nicht; wer eines aus 0.6.0 liest, muss es in seine
Rechnung aufnehmen.

Die Summen dazu stehen unter `effect`: `requested` ist die Zahl der geplanten
Regionen, und

```
applied + covered + degenerate + missing_page + off_page = requested
```

ergibt sie wieder. **An dieser Gleichung rechnet ein Prüfer nach, ob er alle
wirkungslosen Regionen gesehen hat** — mit der alten, vierteiligen Fassung
fehlte je `off_page` einer, und zwar unbemerkt: die Summe war dann kleiner als
`requested`, ohne dass irgendetwas gefehlt hätte.

`pages` nennt die Seitenzahl des Dokuments, damit sich `missing_page`
nachprüfen lässt; `missing_pages` listet die angesprochenen Seiten (0-basiert)
und steht nur dann im Log, wenn es welche gab. Dasselbe leistet
`off_page_pages` für `off_page`.

Jeder Befund außer `applied` steht auch in der Zusammenfassung auf der Konsole
und als Warnung auf stderr — ein Lauf, der nichts entfernt hat, endet nicht
mehr wortlos mit „Schwärzungen: 3“.

Nachgemessen an der Demo (`--write-demo`, zwei Seiten) mit drei manuellen
Regionen: eine über dem Kontoinhaber, eine bei `[3000 3000 3100 3100]` neben
dem Blatt, eine auf `"page": 9`.

```console
$ redact-rs demo.pdf -o out.pdf --no-patterns --manual-regions regionen.json \
    --audit-log audit.json
Seiten:             2
Textzeilen:         15
Treffer gesamt:     3
Automatische Erkennung: abgeschaltet (--no-patterns). …
Schwärzungen:       3
  davon wirksam:      1 (Zeichen entfernt)
  davon wirkungslos:  1 (Seite gibt es in diesem Dokument nicht)
  davon wirkungslos:  1 (Rechteck liegt neben der Seite)
Entfernte Zeichen:  28
Deck-Rechtecke:     2
…
Warnung: 1 von 3 Schwärzung(en) liegen auf einer Seite, die es in diesem
Dokument nicht gibt (Seite 10; das Dokument hat 2 Seite(n)). Dort wurde nichts
entfernt und nichts überdeckt — der Text steht unverändert in der Ausgabe.
Häufigste Ursache ist die Zählweise: in JSON ist die erste Seite „page“: 0, die
letzte also 1.
Warnung: 1 von 3 Schwärzung(en) liegen vollständig neben der Seite, auf der sie
stehen sollen (Seite 1). Dort kann kein Zeichen liegen und kein Deck-Rechteck
sichtbar werden — der Text der Seite steht unverändert in der Ausgabe.
Häufigste Ursache sind Koordinaten aus einer Review- oder Regionsdatei, die zu
einem anders großen Blatt gehören.
$ echo $?
0
```

Und dasselbe im Log — hier steht die Gleichung ausgerechnet da:

```json
"effect": {
  "padding": 1.0, "pages": 2, "requested": 3,
  "applied": 1, "covered": 0, "degenerate": 0,
  "missing_page": 1, "missing_pages": [9],
  "off_page": 1, "off_page_pages": [0],
  "removed_glyphs": 28, "drawn_rects": 2, "removed_annotations": 0,
  "redacted_images": 0, "copied_images": 0
}
```

`1 + 0 + 0 + 1 + 1 = 3`. Ohne `off_page` in der Rechnung käme 2 heraus, und
genau eine wirkungslose Region wäre unbemerkt geblieben.

Der Rückgabewert bleibt hier **`0`**: die Analyse hat das Dokument vollständig
gelesen: eine Region, die ins Leere zeigt, ist ein Fehler in der *Eingabe* des
Nutzers und keine Stelle, die das Werkzeug nicht durchsuchen konnte. Genau
deshalb stehen beide Befunde in der Zusammenfassung, auf stderr **und** als
`missing_page` bzw. `off_page` im Audit-Log.

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
