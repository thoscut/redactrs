# Kommandozeile

## Aufruf und Optionen

Ohne `-o` wird **neben der Eingabedatei** gespeichert: aus `kontoauszug.pdf`
wird `kontoauszug_geschwaerzt.pdf`. Der Zusatz lässt sich mit
`--output-suffix` ändern — oder dauerhaft in der
[Einstellungsdatei](#einstellungsdatei). Eine vorhandene Datei wird nur mit `--force`
überschrieben — ein zweiter Lauf soll ein bereits geprüftes Ergebnis nicht
unbemerkt ersetzen. Für `--review` gilt dasselbe Schema
(`kontoauszug_review.json`).

Der Namenszusatz ist **ein Name**: Pfadtrenner, `..` und Steuerzeichen werden
abgelehnt, bevor irgendetwas entsteht — sonst wäre er ein Wegweiser aus dem
Eingabeverzeichnis heraus (die Begründung steht in
[`SECURITY.md`](../SECURITY.md)). Geprüft wird er **auch dann, wenn `-o` gesetzt
ist** und der Zusatz gar nicht gebraucht würde: ein untaugliches
`--output-suffix` soll auffallen, wo es steht, und nicht erst im nächsten Lauf
ohne `-o`.

```console
$ redact-rs kontoauszug.pdf -o ok.pdf --output-suffix "/../boese"
Fehler: Konfigurationsfehler: der Namenszusatz „/../boese“ ist keiner: er
enthält einen Pfadtrenner. …
$ echo $?
2
```

```text
redact-rs [EINGABE.pdf | VERZEICHNIS …] [OPTIONEN]

  -o, --output <PDF>          Ausgabedatei (Standard: neben der Eingabe)
      --output-suffix <TEXT>  Namenszusatz (Standard: _geschwaerzt)
      --password <PW>         Passwort eines verschlüsselten PDFs
                              (besser: REDACT_RS_PASSWORD — siehe unten)
  -f, --force                 vorhandene Ausgabedatei überschreiben
      --patterns <IDs>        Muster, kommagetrennt (z.B. iban_de,bic)
      --no-patterns           gar keine Muster anwenden
      --disable-pattern <ID>  einzelnes Muster abschalten (mehrfach möglich)
      --patterns-config <F>   eigene Musterkonfiguration (YAML oder JSON)
      --min-confidence <WERT> Mindestvertrauen eines Treffers (Standard: 0.5)
      --booking-list <CSV>    Buchungsliste (Positiv-/Negativliste)
      --manual-regions <JSON> manuell festgelegte Regionen
      --review                nur analysieren, nichts schwärzen
      --review-out <JSON>     Zieldatei des Review-Exports
      --apply-review <JSON>   geprüfte Review-Datei anwenden
      --allow-unverified-review   Review-Datei ohne Prüfsumme trotzdem anwenden
                                  (UNSICHER — siehe Review-Workflow)
      --audit-log <JSON>      Audit-Log schreiben
      --action <ART>          blackout (Standard) | whiteout | replace
      --replace-with <TEXT>   Ersatztext für --action replace
      --padding <PUNKT>       Rand um jede Schwärzung (Standard: 1.0)
      --allow-undecodable-images  nicht dekodierbare Bilder durchgehen lassen
                                  (UNSICHER — siehe unten)
      --max-decompressed-mb <MB>  Budget für alle entpackten Streams (1024);
                                  bei --check-leaks auch das Budget der Suche
                                  (darüber: NICHT GEPRÜFT, rc 3)
      --max-parsed-mb <MB>        davon für alles, woraus PDF-Syntax wird (16):
                                  geparste Streams UND der Rumpf der Datei
                                  (Objektköpfe, Dictionaries, xref)
      --max-image-mb <MB>         gleichzeitig gehaltene dekodierte Bildbytes (256)
      --max-input-mb <MB>         Obergrenze für die Eingabedatei selbst (512)
      --max-candidates <N>        Obergrenze für Trefferkandidaten (100000)
      --check-leaks <TEXT>    NACHPRÜFEN statt schwärzen: steht dieser Text
                              noch in der Datei? Mehrfach angebbar; „-“ liest
                              die Begriffe zeilenweise von stdin. Rückgabewert
                              3 bei Fund oder nicht geprüfter Stelle (siehe Prüfen)
      --gui                   grafische Oberfläche starten
      --list-patterns         eingebaute Muster auflisten
      --write-demo <PDF>      Beispieldatei erzeugen
      --json                  Zusammenfassung als JSON
  -q, --quiet                 weniger Ausgabe
```

Die fünf `--max-…`-Grenzen sind Schutzschalter gegen präparierte Eingabedateien;
was sie abwehren und warum sie so hoch bzw. so niedrig liegen, steht in
[`SECURITY.md`](../SECURITY.md).

**Sie gelten in beiden Programmen.** Die Oberfläche lädt über
`redact_pipeline::load_document` mit **derselben** `Config` wie ein Lauf ohne
`--gui` (`crates/redact-gui/src/state.rs`), liest die Datei über dasselbe
`redact_pipeline::read_input` und geht bei der Trefferauswertung durch dasselbe
`collect_regions_for`, das `--max-candidates` prüft. Ein `redact-rs --gui
auszug.pdf --max-parsed-mb 4` wirkt also wirklich. Was **nicht** gilt: die
Fensterfassung `redact-rs-gui.exe` hat gar keine Kommandozeile und arbeitet
deshalb immer mit den Vorgaben aus der Tabelle in
[`SECURITY.md`](../SECURITY.md) — nicht abgeschaltet, nur nicht verstellbar.

`--allow-unverified-review` ist neben `--allow-undecodable-images` der zweite
Schalter, der eine Prüfung aufhebt: siehe [Review-Workflow](review-und-audit.md#review-workflow).

`--allow-undecodable-images` ist der einzige Schalter, der die Sicherheit
*senkt*: siehe [Bilder werden wirklich geschwärzt](bilder.md#bilder).

Rückgabewerte:

| Wert | Bedeutung |
|---|---|
| `0` | Erfolg — und nichts blieb ungeprüft |
| `1` | Verarbeitungsfehler. Im Stapelbetrieb: mindestens eine Datei ist gescheitert |
| `2` | Bedienfehler (Argumente, Einstellungsdatei, Prüfsummen, `--max-candidates`) |
| `3` | **Der Lauf ist gelungen, das Ergebnis ist es nicht — sieh hin.** Beim Schwärzen: es ist eine Ausgabedatei entstanden, aber mindestens eine Stelle des Dokuments **konnte** die Analyse nicht durchsuchen — ein XObject ohne bekanntes `/Subtype` etwa, oder ein Bild, das sich nicht dekodieren lässt. Was dort steht, kann nicht geschwärzt worden sein. Der Lauf sagt auf stderr, welche Stellen das waren: sie stehen dort mit `NICHT GEPRÜFT` statt `Warnung`, und am Ende steht ihre Zahl. Bei [`--check-leaks`](pruefen.md#pruefen): mindestens einer der Suchbegriffe steht noch in der Datei — **oder** eine Stelle konnte nicht geprüft werden (fünf Gründe, siehe [`SECURITY.md`](../SECURITY.md)), auch ohne einen einzigen Fund. |

`3` ist kein Fehler und kein „alles gut“ — es ist die Aufforderung, genau diese
Stellen anzusehen. In einem Skript gehört er behandelt wie ein Fehler, solange
niemand hingeschaut hat.

Alle drei Fälle senden dieselbe Nachricht, und das ist der Grund für dieselbe
Zahl: *die Datei ist nicht abgenommen.* Ein Fund unter `--check-leaks` ist
ausdrücklich **kein** `1` — die Datei wurde gelesen, die Suche lief vollständig
durch, die Antwort steht fest. Sie lautet nur „ja, es steht noch drin“.

Der dritte Fall ist der stillste und deshalb der wichtigste: `--check-leaks`
endet **auch ohne Fund** mit `3`, wenn es eine Stelle nicht lesen konnte. Sieben
Gründe gibt es dafür — ein Strom über dem Restbudget von
`--max-decompressed-mb`, eine Verschachtelung unterhalb der Tiefe, bis zu der
die Objektsicht liest, ein Filtername, den das Programm nicht kennt, eine vom
Lader abgelehnte Vorprüfung, der Schriftdekoder, der deshalb gar nicht erst
lief, eine Seite, die der Interpreter ablehnt, und ein Strom, den der Lader
anders übernahm, als er in den Rohbytes steht; `SECURITY.md` zählt sie mit
ihren Meldungen auf. Über
eine solche Stelle sagt „nicht gefunden“ nichts, und genau deshalb darf sie
nicht als `0` durchgehen. Sie steht als `NICHT GEPRÜFT: …` in der Ausgabe.

Ein gewöhnliches `Warnung:` setzt den Rückgabewert **nicht**. Ein Rasterbild
auf der Seite ist eine bekannte Grenze des Verfahrens und der Normalfall bei
gescannten Auszügen; `NICHT GEPRÜFT` meint etwas anderes, nämlich eine Stelle,
an der der Interpreter ausgestiegen ist.

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

**Ein Passwort schaltet keine Grenze ab.** Die Vorprüfung läuft über Rohbytes
und sieht an einer verschlüsselten Datei nur die Objektstruktur, nicht die
Streams — deshalb läuft **nach** der Entschlüsselung dieselbe Prüfung ein
zweites Mal auf dem entschlüsselten Dokument
(`redact_pipeline::check_limits_after_decryption`). Es misst derselbe Code mit
denselben Grenzen und denselben Meldungen wie bei einer unverschlüsselten
Datei. Vorher hing an einem Passwort, ob überhaupt gemessen wurde: eine
196-kB-Datei mit einem 64 MB entpackenden Content-Stream wurde ohne Passwort in
0,0 s abgelehnt und lief mit Passwort in den OOM-Killer.

Was auch danach ungeprüft bleibt (der Arbeitsspeicher wird nicht
überschrieben; Berechtigungsbits werden nicht durchgesetzt), steht in
[`SECURITY.md`](../SECURITY.md#passwörter-verschlüsselter-pdfs).

Ohne Passwort ist die Meldung knapp und nennt den Ausweg **nicht** — den nennt
`redact-rs --help` unter `--password`:

```console
$ redact-rs auszug.pdf -o out.pdf
Fehler: PDF-Fehler: auszug.pdf: Dokument ist verschlüsselt. Verschlüsselte PDFs
werden nicht verarbeitet — bitte vorher entschlüsseln.
$ echo $?
1
```

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

Nachgemessen an zwei Kopien des Demo-Kontoauszugs (`--write-demo`). **stdout**:

```text
januar/auszug.pdf → januar/auszug_geschwaerzt.pdf (7 Schwärzung(en))
februar/auszug.pdf → februar/auszug_geschwaerzt.pdf (7 Schwärzung(en))

2 Datei(en): 2 vollständig geprüft, 0 verarbeitet (aber nicht vollständig geprüft), 0 fehlgeschlagen.
```

Dazu **auf stderr**, und zwar jeweils **bevor** die Datei geöffnet wird — sonst
hielte eine riesige Datei den Stapel auf, ohne dass ihr Name irgendwo stünde:

```text
[1/2] januar/auszug.pdf
[2/2] februar/auszug.pdf
```

**Die Zusammenfassung nennt drei Zahlen, nicht zwei.** „Verarbeitet“ hieß früher
auch für die Dateien, deren Text niemand gelesen hatte: wer zwanzig Auszüge
laufen ließ, bekam „20 verarbeitet, 0 fehlgeschlagen“ und Rückgabewert 0,
obwohl in einer davon eine Kontonummer unberührt stand. Die mittlere Zahl steht
deshalb für sich — es sind die Dateien mit [Rückgabewert 3](#kommandozeile), und
keine Datei zählt in zweien mit.

So sieht ein Stapel aus, in dem eine Datei nur zum Teil geprüft werden konnte
und eine gar nicht zu öffnen war (nachgemessen an drei Dateien in einem
Verzeichnis). **stdout**:

```text
./a.pdf → ./a_geschwaerzt.pdf (7 Schwärzung(en))
./b.pdf → ./b_geschwaerzt.pdf (0 Schwärzung(en))  ← nicht vollständig geprüft

3 Datei(en): 1 vollständig geprüft, 1 verarbeitet (aber nicht vollständig geprüft), 1 fehlgeschlagen.

Bei 1 Datei(en) blieb ein Teil des Dokuments ungelesen — was dort steht, kann nicht geschwärzt worden sein. Die Stellen stehen oben auf stderr; bitte diese Ergebnisse von Hand prüfen. (Rückgabewert 3.)
```

**stderr**:

```text
[1/3] ./a.pdf
[2/3] ./b.pdf
NICHT VOLLSTÄNDIG GEPRÜFT ./b.pdf: Das Form-XObject „Fm0“ (Objekt 6 0) steht in
den Ressourcen, wird aber nirgends gezeichnet; sein Text wurde nicht durchsucht
und kann deshalb nicht geschwärzt worden sein.
[3/3] ./c.pdf
FEHLGESCHLAGEN ./c.pdf: PDF-Fehler: ./c.pdf: keine PDF-Datei (Header %PDF- fehlt)
```

Mit `--quiet` bleiben von beiden Strömen genau die beiden Zeilen übrig, die
etwas zu bedeuten haben — `NICHT VOLLSTÄNDIG GEPRÜFT` und `FEHLGESCHLAGEN`;
stdout bleibt dann leer, und auch die Fortschrittszeilen entfallen
(nachgemessen).

Der Rückgabewert war hier **`1`**: eine gescheiterte Datei sticht die
unvollständig geprüfte. Ohne die kaputte Datei ist er `3` (ebenfalls
nachgemessen: `2 Datei(en): 1 vollständig geprüft, 1 verarbeitet (aber nicht
vollständig geprüft), 0 fehlgeschlagen.`).

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
disabled_patterns: [date_de]  # dauerhaft abgeschaltet (Vorgabe: keines)
min_confidence: 0.4           # Mindestvertrauen      (Vorgabe: 0.5)
padding: 2.0                  # Polsterung in Punkt   (Vorgabe: 1.0)
theme: dunkel                 # Thema der Oberfläche  (hell | dunkel)
layout: fokus                 # Arbeitsansicht        (werkbank | fokus)
```

**`no_patterns` gibt es hier bewusst nicht.** „Alle automatischen Funde aus“ ist
die weitreichendste Einstellung dieses Werkzeugs, und sie wäre aus einer Datei
heraus **nicht mehr zu widerrufen**: `--no-patterns` ist ein Schalter ohne
Gegenstück, es gibt kein `--patterns-an`. Stünde er in der Datei, liefe jeder
Aufruf ohne Erkennung, und die Kommandozeile hätte kein Mittel dagegen.
`disabled_patterns` ist etwas anderes: eine Liste auf der Kommandozeile
**ersetzt** die aus der Datei, die Angabe bleibt also widerrufbar.

**Rangfolge: Kommandozeile schlägt Datei schlägt Vorgabe.** Ein Schalter, der
nicht angegeben wurde, überschreibt die Datei nicht — das ist der Grund, warum
`--output-suffix`, `--padding` und `--min-confidence` in der Hilfe keinen
Standardwert mehr anzeigen. Geprüft wird die Reihenfolge in
`crates/redact-cli/tests/settings.rs`.

Ein unbekannter Schlüssel (Tippfehler) und ein unbekanntes Thema beenden den
Lauf mit einer Meldung. Eine Einstellung, die stillschweigend nicht wirkt, wäre
das schlechtere Verhalten. Eine **fehlende** Datei ist dagegen kein Fehler.

**Gelesen wird die Datei nur von `redact-rs` (bzw. `redact-rs.exe`).** Dort
wird sie beim Bauen von `redact_pipeline::Config` angewendet — und weil
`redact-rs --gui` dieselbe `Config` an die Oberfläche weiterreicht, gelten
Namenszusatz, Muster, Schwelle, Polsterung und Thema auch dort.

Die Fensterfassung zum Doppelklicken (`redact-rs-gui.exe`) liest sie
**nicht**: sie baut ein `Config::default()` und ruft `Settings::load()` nie
auf (`crates/redact-cli/src/bin/redact-rs-gui.rs`; dasselbe gilt für das
Entwickler-Binary `redact-gui`). Wer per Doppelklick arbeitet und trotzdem
seine Einstellungen will, startet über die Konsolenfassung:

```bash
redact-rs --gui kontoauszug.pdf
```

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
| `glaeubiger_id` | SEPA-Gläubiger-ID (Creditor Identifier), mod-97-geprüft | 0.95 | – | **an** |
| `konto_nr` | Kontonummer (6–10 Ziffern nach „Kto.“, „Kto.-Nr.“, „Konto Nr.“ o. ä.) | 0.85 | 0.30 | **an** |
| `blz` | Bankleitzahl (8 Ziffern, auch als „370 400 44“, nach „BLZ“/„Bankleitzahl“) | 0.80 | 0.25 | **an** |
| `bic` | BIC/SWIFT mit Länderkennung-Prüfung | 0.80 | – | **an** |
| `amount_eur` | Geldbetrag in Euro | 0.70 | – | aus |
| `date_de` | Datum TT.MM.JJJJ | 0.60 | – | aus |
| `credit_card` | Kreditkartennummer (13–19 Ziffern am Stück, 4-4-4-4[-3], 4-6-5 oder 4-6-4), Luhn-geprüft | 0.90 | – | **an** |
| `steuer_id` | Steuerliche Identifikationsnummer (11 Ziffern, auch als „12 345 678 903“, nach „Steuer-ID“/„Identifikationsnummer“) | 0.90 | 0.25 | **an** |
| `email` | E-Mail-Adresse | 0.90 | – | **an** |
| `phone_de` | Deutsche Telefonnummer | 0.85 | 0.35 | **an** |

Muster mit Prüfsumme (IBAN, Gläubiger-ID, BIC, Kreditkarte) verwerfen Treffer,
die die Prüfung nicht bestehen — das drückt die Fehlalarmquote deutlich.
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

### Automatische Funde abschalten

Manchmal will man die Vorschläge des Werkzeugs nicht. Ein Muster, das in *dieser*
Aktenlage nur Fehltreffer erzeugt; ein Dokument, an dem allein von Hand
geschwärzt werden soll. Beides geht — ganz und einzeln, auf der Kommandozeile
und in der Oberfläche:

```bash
# Gar keine automatische Suche: geschwärzt wird nur, was von Hand
# gezogen oder über die Buchungsliste angegeben ist.
redact-rs auszug.pdf -o out.pdf --no-patterns --manual-regions regionen.json

# Ein einzelnes Muster heraus, die übrigen laufen weiter.
redact-rs auszug.pdf -o out.pdf --disable-pattern konto_nr

# Mehrere, kommagetrennt oder mehrfach angegeben.
redact-rs auszug.pdf -o out.pdf --disable-pattern konto_nr,phone_de
```

Gemeint sind die Muster, die der Lauf **wirklich anwendet** — also die Spalte
„Standard: **an**“ der [Tabelle oben](#eingebaute-muster) bzw. das, was
`--patterns` ausgewählt hat. Ein ohnehin ausgeschaltetes Muster (`date_de`,
`amount_eur`, `iban_intl`) braucht kein `--disable-pattern`; abgelehnt wird es
trotzdem nicht, denn ein bekannter Name ist ein bekannter Name.

Wie sich die Schalter zueinander verhalten, am Demo-Kontoauszug nachgemessen
(er ergibt mit den Vorgabemustern 7 Treffer):

| Aufruf | Treffer | Ansage |
|---|---|---|
| `--disable-pattern konto_nr` | 6 | `… 1 Muster abgeschaltet (konto_nr).` |
| `--disable-pattern konto_nr,phone_de` | 5 | `… 2 Muster abgeschaltet (konto_nr, phone_de).` |
| `--disable-pattern date_de` (war ohnehin aus) | 7 | `… 1 Muster abgeschaltet (date_de).` |
| `--patterns iban_de` | 2 | keine |
| `--patterns iban_de --disable-pattern bic` | 2 | `… 1 Muster abgeschaltet (bic).` |
| `--patterns iban_de --disable-pattern iban_de` | 0 | `… 1 Muster abgeschaltet (iban_de).` |
| `--no-patterns --disable-pattern bic` | 0 | `… abgeschaltet (--no-patterns).` |
| `--disable-pattern iban` | — | Abbruch, Rückgabewert 2 |

Zwei Zeilen der Tabelle überraschen und sind deshalb Absicht:

* **`--disable-pattern date_de`** ändert nichts an der Trefferzahl — das Muster
  war ohnehin aus — und wird trotzdem angesagt. Die Ansage berichtet, was
  *angeordnet* wurde, nicht was gewirkt hat; das ist die Aussage, die eine
  Prüferin braucht.
* **`--no-patterns` schlägt `--disable-pattern`.** Die Ansage nennt dann nur
  noch das Gröbere: bei „alles aus“ ist die Zahl der einzeln abgeschalteten
  Muster gegenstandslos.

In der Oberfläche steht der Schalter über der Trefferliste: das Häkchen
**„Automatisch suchen“** für alles, darunter aufklappbar **„Muster einzeln“**
mit einem Kästchen je Muster dieses Laufs. Die Liste ist zugeklappt, solange
nichts abgeschaltet ist, und offen, sobald etwas aus ist — eine Abschaltung
soll man sehen, ohne danach zu suchen.

Beim Umschalten wird die Trefferliste **neu gerechnet**. Das kostet: jede
Abwahl, jede je Treffer gewählte Schwärzungsart und ein geladenes Review sind
danach weg. Erhalten bleiben die selbst gezogenen Rechtecke und die
Schutzmarken der Buchungsliste (die steht in der Konfiguration und wird nicht
angefasst). Deshalb kommt vorher dieselbe Rückfrage wie bei „Analysieren“ — und
sagt man dort „nein“, springt auch das Kästchen zurück: die Seitenleiste ändert
nichts selbst, sie meldet nur den Wunsch.

**Ein unbekannter Name ist ein Bedienfehler**, kein Achselzucken:

```console
$ redact-rs auszug.pdf -o out.pdf --disable-pattern iban
Fehler: Konfigurationsfehler: --disable-pattern: „iban“ ist kein bekanntes
Muster. Gültig sind: iban_de, iban_intl, glaeubiger_id, konto_nr, blz, bic,
amount_eur, date_de, credit_card, steuer_id, email, phone_de.
(`redact-rs --list-patterns` zeigt sie mit Beschreibung.) Es wurde nichts
abgeschaltet und nichts geschwärzt: ein übergangener Name sähe aus wie eine
Abschaltung und wäre keine.
$ echo $?
2
```

Der Lauf endet also **vor** der ersten Schwärzung; es entsteht keine
Ausgabedatei. Ein stillschweigend übergangener Tippfehler ergäbe dagegen eine
Datei, die anders ist als erwartet, und niemand erführe warum.

#### Der Zustand ist sichtbar — das ist der Punkt

Eine Datei, die mit abgeschalteter Erkennung entstanden ist, sieht in jeder
Zahl aus wie eine vollständig geprüfte: „0 Treffer“ heißt dort nicht „nichts
gefunden“, sondern „nicht gesucht“. Deshalb steht die Abschaltung an **drei**
Stellen, und alle drei speisen sich aus derselben Angabe:

* in der Zusammenfassung auf stdout und als Warnung auf stderr —
  `Automatische Erkennung: abgeschaltet (--no-patterns). …`;
* in der Oberfläche in der Kopfzeile der Trefferliste und als Satz in
  Warnfarbe unter dem Schalter. Ganz aus:
  `Automatische Suche AUS — nicht gesucht, nur von Hand: 2 Treffer · 2 werden
  geschwärzt`. Einzelne Muster aus:
  `6 Treffer · 6 werden geschwärzt · 1 Muster abgeschaltet` — die Zahl steht
  dann hinter den Trefferzahlen, weil sie sie ergänzt statt sie umzudeuten.
  Ist noch kein Dokument geladen, sagt die Statuszeile den Zustand trotzdem
  (`Automatische Erkennung: alle Muster an` bzw. der jeweilige Satz);
* im **Audit-Log** als eigenes Feld, das auch dann dasteht, wenn nichts
  abgeschaltet war:

```json
"patterns": { "all_disabled": false, "disabled": ["konto_nr"] }
```

Zusätzlich ist „Analysieren“ in der Oberfläche ausgegraut, solange die Analyse
nachweislich nichts finden könnte (kein Muster, keine Buchungsliste, keine
Regionsdatei) — mit dem Grund in der Sprechblase. Ein Knopf, der eine leere
Trefferliste hinterlässt, läse sich sonst als „nichts gefunden“.

Was **nicht** geschieht: der Rückgabewert bleibt `0`. Eine abgeschaltete
Erkennung ist eine Anweisung des Aufrufenden und kein Befund an der Datei — die
Analyse hat das Dokument vollständig gelesen und auf Geheiß nach weniger
gesucht. Rückgabewert `3` ist der Frage „hat das Werkzeug alles *gesehen*?“
vorbehalten (siehe [Kommandozeile](#kommandozeile)); spränge er auch bei jedem
`--no-patterns` an, wäre er für die Fälle wertlos, für die es ihn gibt. Die
Begründung im Einzelnen steht in `crates/redact-pipeline/src/coverage.rs`.

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
  den Blöcken stehen. Ein Umlaut, den das PDF als Grundbuchstabe plus
  Akzentzeichen liefert („u“ + U+0308), gilt als derselbe Buchstabe wie „ü“.
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

Vollständiges Beispiel: [`examples/booking_list.csv`](../examples/booking_list.csv).

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
