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

> **Diese Datei erzeugt die Release-Notizen.** `.github/workflows/release.yml`
> schneidet beim Veröffentlichen den Abschnitt der gebauten Version heraus
> (`## <version>`, bis zur nächsten `## `-Überschrift). Beim Release ist
> „Unveröffentlicht“ deshalb in `## <version> — <datum>` umzubenennen; sonst
> findet der Job nichts und meldet eine Warnung statt der
> Verhaltensänderungen. Nachgeprüft: für 0.3.0, 0.2.0 und 0.1.0 findet er
> heute 98, 84 bzw. 33 Zeilen.

---

## Unveröffentlicht

Vierter Durchgang, und diesmal fast nur an der **Doku** — mit demselben
Maßstab wie am Code: jede Angabe hier stammt aus einem Lauf des gebauten
Binaries, nicht aus dem Quelltext.

### ⚠ Die README beschrieb ein Audit-Log, das es so nicht mehr gibt

Seit 0.6.0 hat `effect` einen **fünften** Befund, `off_page`. Die README nannte
weiter „vier Befunde“ und die Gleichung
`applied + covered + degenerate + missing_page = requested`. **An dieser
Gleichung rechnet ein Prüfer nach, ob er alle wirkungslosen Regionen gesehen
hat** — und sie ging nicht mehr auf. Gemessen an der Demo mit drei manuellen
Regionen (eine wirksam, eine neben dem Blatt, eine auf einer Seite, die es
nicht gibt): `requested 3, applied 1, covered 0, degenerate 0, missing_page 1,
off_page 1`, also `1 + 0 + 0 + 1 = 2 ≠ 3`. Genau eine wirkungslose Region wäre
unbemerkt geblieben.

`off_page` kam in README **und** `SECURITY.md` null mal vor. Beide nennen ihn
jetzt: die Befundtabelle hat eine fünfte Zeile, die Gleichung fünf Summanden,
der Beispiel-Auszug das Feld, und `SECURITY.md` warnt ausdrücklich davor, die
vierteilige Fassung aus 0.5.0 weiterzubenutzen.

### ⚠ `SECURITY.md` kannte `--check-leaks` nicht

Der Schalter kam dort null mal vor. Der Abschnitt zum Rückgabewert 3 nannte nur
die Stapelbedeutung („verarbeitet, aber nicht vollständig geprüft“) — wer ihn
in ein Skript übernahm, deutete einen **Leckfund** als bloß unvollständige
Prüfung. Der `--help`-Text des Binaries nennt seit 0.4.0 beide Fälle; jetzt tut
es `SECURITY.md` auch, mit einem eigenen Abschnitt zum Schalter.

Dort stand außerdem „Verbindlich ist `redact_pdf::leaks`“ — das ist die
Bibliotheksfunktion und damit gerade nicht das, was jemand ohne Quelltext
bedienen kann. Der Satz nennt jetzt beide Wege und sagt, dass es dieselbe
Funktion ist.

### ⚠ Kein Kernabzug mehr — und die Zusage zu `unsafe` ist enger geworden

`redact-rs` schaltet Kernabzüge als **allererste** Anweisung in `main` ab
(`prctl(PR_SET_DUMPABLE, 0)`). Ohne das schrieb der Kernel bei einem Absturz
den gesamten Arbeitsspeicher weg — samt Klartext des Dokuments und, bei einer
verschlüsselten Datei, dem Passwort.

Am **gebauten Binary** nachgemessen (`core_pattern = core`,
`ulimit -c unlimited`, Prozess mit `SIGABRT` beendet): ein `sleep` als
Kontrolle hinterlässt im selben Aufruf eine 454 656 Byte große `core`-Datei,
`redact-rs` **keine**; die Shell meldet nur noch `Aborted` statt
`Aborted (core dumped)`, bei unverändertem Rückgabewert 134.

**Die Zusage `#![forbid(unsafe_code)]` gilt jetzt für sieben statt acht
Crates.** `redact-cli` steht unter `#![deny(unsafe_code)]` mit **einer**
benannten Ausnahme an der `prctl`-Funktion. `SECURITY.md` sagte weiter „in
allen acht“ und gab dazu ein `grep`, das seither eine Datei weniger findet —
beides ist berichtigt, und die Ausnahme wird dort jetzt **beziffert** (zwei
`unsafe`-Blöcke: einer im ausgelieferten Programm, einer im Test, der
zurückliest, ob der Kernel den Zustand übernommen hat) statt behauptet.

Ausgeschrieben steht dort auch, **was der Schutz nicht leistet**: unter Windows
gibt es kein Gegenstück (`MiniDumpWriteDump` liegt beim Aufrufer, nicht beim
Ziel) — ausgerechnet die Plattform der Zielgruppe —, und unter macOS und jedem
anderen Nicht-Linux-System tut die Funktion nichts und liefert trotzdem `true`.
Dazu der Nebeneffekt: der Prozess ist danach für `ptrace` durch denselben
Benutzer unerreichbar, `gdb` und `strace` brauchen `root`.

### Die Oberfläche prüft nach dem Export selbst nach

Neu: nach jedem Export liest die Oberfläche die geschriebenen Bytes zurück und
sucht darin mit `redact_pdf::leaks` die Texte, die sie gerade geschwärzt hat.
Das ist die **stärkere Fassung** von `--check-leaks`, weil die Oberfläche
etwas hat, was der Kommandozeilennutzer nicht hat: sie kennt die Suchbegriffe
schon (in jeder geschwärzten Zeile steht der gefundene Text) und muss sie
weder tippen lassen noch in Prozessliste und Shell-Historie schreiben. Für die
Zielgruppe, die per Doppelklick arbeitet, war die Nachprüfung bis hierher gar
nicht erreichbar.

Drei Dinge stehen in der Zeile, die dabei entsteht, und zwar immer:

* das Ergebnis — bei einem Fund zusätzlich ganz vorn in den Warnungen;
* **derselbe Vorbehalt wie in der Kommandozeile**: geprüft ist *diese Liste*,
  nicht die Datei;
* **die Zahl der Rechtecke ohne bekannten Text.** Ein selbst gezogenes
  Rechteck hat keinen; darüber kann die Prüfung nichts sagen, und dort bleibt
  es bei der Sichtprüfung. Verschwiegen wäre die neue Anzeige an einem
  Dokument mit lauter Handregionen selbst eine falsche Entwarnung.

Gesucht werden höchstens 200 verschiedene Texte (`MAX_EXPORT_CHECK_NEEDLES`) —
`leaks` liest die Datei je Begriff einmal ganz durch, und die Trefferliste darf
100 000 Zeilen lang werden. Was darüber liegt, wird gesagt und nicht
verschwiegen.

### Drei Ungenauigkeiten der Oberfläche

* **Eine deckungsgleiche Handregion konnte den Platz des wirklich blockierten
  Mustertreffers verbrauchen.** `BlockedRegion` trägt nur Seite und Rechteck,
  nicht die Herkunft; zweimal Strg+R legt zwei buchstäblich gleiche Regionen
  an. Lag dort ein von der Negativliste gedeckter Mustertreffer derselben
  Fläche, stand an einem **selbst gezogenen** Rechteck „geschützt durch Ihre
  Liste“ (falsch — Handregionen überstimmen die Liste) und der wirklich
  blockierte Treffer hieß „doppelt“. Der Zeiger fragt jetzt zusätzlich, ob die
  Zeile überhaupt blockierbar ist. **Der Export war nie betroffen.**
* **Die Absage bei Strg+Pfeil sprach vom Verschieben.** Wer die Größe ändern
  wollte und dabei auf der falschen Seite stand, las „Nicht verschoben …“ und
  suchte den Fehler an der falschen Stelle. Jetzt nennt sie das Verb, um das es
  ging.
* **Dieselbe Absage riet, zu Seite 8 zu blättern** — bei einem zweiseitigen
  Dokument. Eine Zeile auf einer Seite, die es nicht gibt, kann nur eine
  Review- oder Regionsdatei mitbringen; die Absage nennt jetzt die Seitenzahl
  des Dokuments und einen gangbaren Ausweg.

### Doku: drei weitere Stellen, an denen sie hinter dem Code stand

* **Die Tastentabelle** kannte weder `Strg+R` noch `Strg+Pfeil`, und
  `Umschalt+Tab` stand nirgends in der Datei. Der Abschnitt „Rechtecke
  ziehen …“ sagte weiter „Ein neuer Bereich entsteht durch Aufziehen mit der
  Maus“ — für den Nutzer, für den v0.5.0 den mauslosen Weg gebaut hat, war das
  die falsche Auskunft. Beide Wege stehen jetzt nebeneinander, samt dem Knopf
  „🔲 Rechteck“, den die README überhaupt nicht kannte.
* **Die neuen Decken standen nirgends.** `MAX_TO_UNICODE_BYTES` (32 MB),
  `MAX_CACHED_FONT_ENTRIES` (400 000 Tabelleneinträge) und die
  MediaBox-Heilung („A4 angenommen“) kamen in README, `SECURITY.md` und dieser
  Datei je null mal vor — obwohl der Kopf zu 0.6.0 die Decken zur Hauptsache
  der Fassung erklärt. Die Grenzentabelle der README hat jetzt zwei weitere
  feste Grenzen, jede mit dem Lauf, aus dem die Angabe stammt: eine 1 064 Byte
  kleine Datei mit drei `bfrange`-Blöcken über je 65 536 Codes (rund 50 MB)
  reißt die erste Decke und endet mit „NICHT GEPRÜFT“ und Rückgabewert 3;
  dieselbe Datei mit einem Block (rund 16,8 MB) läuft mit 0 durch.
* **„Erfasst sind alle Änderungen“** nannte sechs; `history.record` steht an
  zehn Stellen. Es fehlten Größe ändern, Schwärzungsart, Ersatztext — und
  **Analysieren**. Die Liste ist jetzt vollständig und sagt gemessen, was
  Analysieren wirklich wegwirft: selbst gezogene Rechtecke trägt es hinüber,
  die Entscheidungen an den Mustertreffern nicht.

### Der Pfad des Build-Rechners ist jetzt vollständig draußen

Der Eintrag zu 0.5.0 hielt fest, dass im glibc-Artefakt weiterhin **drei**
Pfade des Build-Rechners stehen — Build-Skript-Ausgaben unter `OUT_DIR`, die
der eine `--remap-path-prefix` nicht erfasst — und dass die Behebung im Bauweg
aussteht. Sie ist erledigt: `RUSTFLAGS` trägt jetzt einen **zweiten** Prefix
für `${CARGO_TARGET_DIR:-$PWD/target}`.

**Gemessen, bevor die Zeile geschrieben wurde**, und mit einem auffälligen
Ersatzpfad, damit das Ergebnis nicht zu verwechseln ist:

```console
$ RUSTFLAGS="--remap-path-prefix=$CARGO_HOME/registry/src=/cargo-registry \
             --remap-path-prefix=$PWD/target=/ZWEITER-PREFIX" \
    cargo build --release --locked -p redact-cli
$ strings -a target/release/redact-rs | grep -c /home/user/redactrs
0
$ strings -a target/release/redact-rs | grep ZWEITER-PREFIX | sort -u
/ZWEITER-PREFIX/release/build/glutin_egl_sys-…/out/egl_bindings.rs
/ZWEITER-PREFIX/release/build/glutin_glx_sys-…/out/glx_bindings.rs
/ZWEITER-PREFIX/release/build/glutin_glx_sys-…/out/glx_extra_bindings.rs
```

Vorher standen an genau diesen drei Stellen dieselben Zeilen mit dem echten
Pfad des Arbeitsbaums — belegt am selben Baum mit nur einem Prefix
(`grep -c /home/user/redactrs` ⇒ **3**). Auch `redact-rs-gui` ist danach bei 0.

**Was hier nicht steht:** eine Zusage über das *veröffentlichte* Artefakt.
Gemessen ist ein lokaler Bau mit derselben Toolchain (1.94.1) und denselben
Schaltern; ob der Release-Job dasselbe liefert, zeigt erst der nächste Lauf.
Genau diese Zusage war im Eintrag zu 0.4.0 schon einmal zu weit gefasst —
deshalb diesmal die Trennung zwischen „gemessen“ und „zugesagt“.

---

## Unveröffentlicht

Bereich: `git log v0.6.0..HEAD`.

Vierter und letzter Durchgang der vereinbarten Schleife. Die Fehlerklasse
dieser Runde steht schon im Eintrag zu 0.6.0 zwischen den Zeilen, hier wird
sie ausgesprochen: **eine Zusicherung, die an ihrer Ursprungsstelle stimmt,
wird an der nächsten stillschweigend mitgenommen.** Dreimal dasselbe Muster —
und dreimal war die Abhilfe, die Zahl dort zu prüfen, wo sie *neu entsteht*,
nicht dort, wo sie hereinkommt.

### ⚠ Kein Kernabzug mehr

Stürzte der Lauf ab, schrieb der Kernel den ganzen Arbeitsspeicher auf die
Platte — mit dem Klartext des Dokuments und, bei einer verschlüsselten Datei,
dem Passwort. Der Absturz ließ sich über eine präparierte Eingabedatei
auslösen.

Gemessen am laufenden Programm (`core_pattern = core`, `ulimit -c unlimited`,
eine IBAN im Speicher, dann Absturz):

| | Abzugsdatei | die IBAN darin |
|---|---|---|
| 0.6.0 | 487 424 Byte | **3 Fundstellen** |
| 0.7.0 | keine | — |

`prctl(PR_SET_DUMPABLE, 0)` als erste Anweisung beider Binärziele. Zwei
Dinge, die dazugehören und nicht im Kleingedruckten stehen sollen:

* **Unter Windows gibt es kein Gegenstück** — ein Prozess kann sich dort dem
  Abbild nicht entziehen. Diese Absicherung schützt Linux und
  macOS-artige Systeme, also nicht die Plattform, auf der die meisten Nutzer
  dieses Werkzeugs sitzen.
* Der Prozess ist danach auch für `ptrace` durch denselben Benutzer
  unerreichbar. Für ein Werkzeug, das Kontoauszüge im Speicher hält, ist das
  die richtige Richtung; wer mit `gdb` an einem Fehler arbeitet, braucht
  dafür `root`.

Damit steht `redact-cli` unter `#![deny(unsafe_code)]` statt `forbid`, mit
**einer** benannten Ausnahme an genau der Funktion, die den `prctl`-Aufruf
enthält. Die übrigen sieben Crates bleiben unter `forbid`. `libc` lag über
`lopdf → getrandom` ohnehin in jedem Bau; es kommt keine Abhängigkeit hinzu.

### ⚠ NaN und Überlauf: ein Rechteck ohne brauchbare Koordinaten

`--padding nan` genügte, um **jede** Schwärzung wirkungslos zu machen — und
der Lauf meldete trotzdem „Deck-Rechteck gezeichnet". In der Ausgabe stand
dann `NaN NaN NaN NaN re f`. Ein NaN-Rechteck überdeckte rechnerisch *alles*
(`intersection_area` lieferte die volle Fläche), und im `dedup` verschluckte
es damit den echten IBAN-Treffer.

Schwerer: aus **endlichen** Koordinaten konnte `∞ − ∞` entstehen. Der
Wächter des Rechteckgitters prüfte `spanne > 64`, und das ist für `NaN`
falsch — die Schleife lief danach bis `i64::MAX`. Gemessen an einem
präparierten PDF: **vorher Abbruch nach 90 s, jetzt 0,01 s bei 9,1 MB.**

Die Regel steht jetzt einmal (`Rect::is_usable`), und dort, wo eine Zahl neu
entsteht, wird sie neu geprüft — am **Ergebnis**, nicht an der Eingabe.

### ⚠ Der Speicher, den eine Datei vor jedem Scan kostet

`--max-parsed-mb` zählte nur die entpackten Ströme. Der Rest der Datei —
Objekte, Verzeichnisse, Namen — kostet gemessen **197 bis 274 Byte je
Dictionary-Eintrag**, unabhängig von seiner Schreibweise. Eine dicht
geschriebene 44-MB-Datei ergab damit ein 1 804 MB großes Dokument, und der
Lauf endete mit Rückgabewert 0.

| Datei | 0.6.0 | 0.7.0 |
|---|---|---|
| 44,3 MB, dichte Verzeichnisse | Exit 0, **3 756 MB**, 12,6 s | Exit 1, 50 MB, 0,35 s |
| 49,9 MB, Verzeichnisse | Exit 0, 3 197 MB, 11,3 s | Exit 1, 56 MB, 0,44 s |
| 81,7 MB, nur Zahlen | Exit 0, 670 MB, 2,6 s | Exit 1, 89 MB, 0,61 s |

Dieselbe Bombe in einem *komprimierten* Objekt-Strom wurde seit jeher
abgelehnt; der Unterschied war allein, ob sie komprimiert war — und das
sucht sich ein Angreifer als Erstes aus. Gegenprobe an gewöhnlichen Dateien:
ein 2 000-Seiter mit 100 000 Treffern braucht 11 MB des 16-MB-Budgets, ein
300-seitiger Scan mit 600 MB Bilddaten 1 MB.

### Die Nachprüfung kostet nicht mehr Begriffe × Dateigröße

`--check-leaks` packte je Suchbegriff **die ganze Datei neu aus** und parste
den Objektgraphen neu. Diese Arbeit hängt an der Datei, nicht am Begriff.

| 792-kB-Datei | 0.6.0 | 0.7.0 |
|---|---|---|
| 1 Begriff | 0,13 s | 0,11 s |
| 10 Begriffe | 0,88 s | 0,25 s |
| 40 Begriffe | — | 0,69 s |

`redact_pdf::leaks(bytes, begriff)` bleibt unverändert und benutzt intern
denselben Durchgang. Neu ist eine Obergrenze von 1 000 Begriffen: die
Byte-Grenze allein ließ rund eine Million Zeilen zu, was über vier Stunden
Laufzeit ergäbe — von außen nicht von einem Hänger zu unterscheiden.

### Der Beleg, den man ansehen kann

`docs/vorher-nachher.md` zeigt dieselbe Seite vor und nach dem Lauf,
gerendert vom Rasterizer dieses Programms, daneben die vollständigen
`--check-leaks`-Läufe. Erzeugt von `./scripts/make-preview.sh`, bitgleich
reproduzierbar.

Bewusst nicht geschönt: „Kontoinhaber: Max Mustermann" steht im rechten Bild
**unverändert da**, weil es für Namen kein Muster gibt. Und `pdftotext`
steht dort als *Warnung* — mit Rückgabewert 1 („kein Treffer") direkt neben
dem Lauf, der in derselben Datei noch etwas findet.

### An der Fassungsgeschichte selbst

Der Commit-Verlauf wurde einmalig umgeschrieben: 67 Nachrichten trugen eine
Sitzungs-URL des Werkzeugs, mit dem sie entstanden sind. Sie sind entfernt;
die `Co-Authored-By`-Zeilen bleiben, weil sie eine wahre Angabe sind.

**Folge, die genannt gehört:** jeder Commit-SHA hat sich geändert, auch die
der Tags `v0.1.0` bis `v0.6.0`. Der *Inhalt* ist an jedem Tag byteidentisch
geblieben (nachgeprüft am Baum-Hash), die veröffentlichten Artefakte und
Release-Notizen sind unberührt. Die Build-Provenienz der Releases 0.4.0 bis
0.6.0 nennt aber weiterhin die **alten** SHAs; wer sie gegen den heutigen
Verlauf prüft, findet dort eine Abweichung, die keine inhaltliche ist.

---

## 0.6.0 — 2026-08-05

Bereich: `git log v0.5.0..v0.6.0`.

Dritter Durchgang. Er hat abgearbeitet, was die zweite Pruefung belegt
hinterlassen hatte — und dabei ist dreimal dieselbe Fehlerklasse
aufgetaucht: **eine Decke, die die falsche Einheit zaehlt.**

### ⚠ Drei Schutzdecken, die kein Test hielt

Ein Pruefer hatte sie alle drei auf „unbegrenzt" gesetzt: 1 034 Tests
blieben gruen. Gemessen schalten die Mutationen mehrere hundert Megabyte
Schutz und eine Quadratik ab. Sie haengen jetzt an deterministischen
Zaehlern statt an einer Uhr.

* Die Schriftendecke zaehlte **Verzeichnisse** — vierzig Namen in *einem*
  Verzeichnis kosteten 279 MB bei Zaehlerstand 1. Sie zaehlt jetzt
  Tabelleneintraege.
* Die Operationsdecke sah den vollstaendigen `/Resources`-Klon nicht, den
  der Zwischenspeicher daneben hielt: Stroeme mit null Operationen wogen
  null und wurden deshalb ausnahmslos behalten.
* Der Test fuer die Schichtdecke prueste gegen die Konstante, die er
  absichern sollte — wer sie anhob, hob die Schranke mit an.

### Leistung, jeweils an einer Messreihe belegt

* Die Schwaerzung legte je Bereich einen Bitvektor ueber **alle** Zeichen
  der Textoperation an und behielt ihn: 227 kB Eingabe kosteten 38,2 s und
  1 860 MB, jetzt 0,213 s und 92 MB.
* Die Vorauswahl der Bereiche war ein Streifen ueber **eine** Achse — bei
  einer Spalte, wie sie ein Kontoauszug erzeugt, lieferte sie exakt das
  volle Produkt und siebte damit nichts aus.
* Die Schrift lag als **Wert** im Grafikzustand und wurde bei jeder
  Fontwahl vollstaendig geklont. Der Zwischenspeicher aus 0.4.0
  verhinderte das erneute Parsen, nicht das Klonen; der teure Fall hatte
  sich nur verschoben.
* Dieselbe Schrift wird jetzt je Seitendurchlauf einmal geparst, gleich
  unter wie vielen Namen sie steht: 300 Namen auf dasselbe Objekt kosteten
  rund 18 s und etwa 2 GB, jetzt 0,077 s und 54 MB.
* In der Oberflaeche fielen zwei Rechnungen von 1,6 s und 26,7 s auf 0,109
  und 0,144 s. Ein Schluessel war dafuer nicht noetig — aber die
  Reihenfolge, auf die sich das stuetzt, war bisher nur eine
  Implementierungseigenschaft und ist jetzt als Test zugesagt.

### Geaendert

* **Kommandozeile und Oberflaeche sagen bei einem Rechteck neben der Seite
  dasselbe.** Die CLI meldete es als Schwaerzung und warnte erst hinterher;
  jetzt faellt der Befund auch dort **vor** der Messung, mit denselben
  Worten und einer eigenen Zahl in der Zusammenfassung.

### An dieser Datei selbst

* **Die Release-Notizen von 0.5.0 sind unvollstaendig, und zwar durch einen
  Ablauffehler.** Waehrend 0.5.0 vorbereitet wurde, trug ein Beitrag seine
  Punkte — darunter `--check-leaks` — unter „Unveroeffentlicht" ein, weil der
  Abschnitt `## 0.5.0` zu dem Zeitpunkt schon geschrieben war. Der Job
  schneidet beim Veroeffentlichen nur den Abschnitt der gebauten Version
  heraus; die Punkte sind also **ausgeliefert**, standen aber nicht in den
  Notizen. Sie sind hier unter 0.5.0 nachgetragen.

  Die Lehre daraus steht im Kopf dieser Datei: wer waehrend einer laufenden
  Freigabe etwas eintraegt, traegt es in den Abschnitt der Version ein, die
  gerade gebaut wird — nicht darueber. Ein Rechenweg, der das erzwingt, waere
  besser als eine Regel; solange es ihn nicht gibt, gehoert der Abgleich
  zwischen `## <version>` und `git log` in den Freigabeweg.

---

## 0.5.0 — 2026-08-05

Bereich: `git log v0.4.0..v0.5.0`.

Zweiter vollstaendiger Pruefdurchgang. Er hat zuerst gegen die eigenen
Aenderungen aus 0.4.0 gearbeitet — mit einem zweiten Abzug von 0.3.0 und
einer randomisierten Differenzsuche ueber 1 300 Anordnungen, denn nur so
laesst sich „Regression" von „war schon immer so" trennen.

### ⚠ Zwei Regressionen aus 0.4.0

* **Die Schichtwahl zerriss eine IBAN.** Der Code waehlte die Schicht mit
  dem groessten erreichten Wert, obwohl der Kommentar „am dichtesten davor"
  sagte — an einer gewoehnlichen Tabelle mit einem zu langen
  Empfaengernamen wanderte das erste IBAN-Stueck dadurch in eine Schicht
  und das zweite zurueck in die andere. 0.3.0 fand die IBAN, 0.4.0 meldete
  null Treffer bei Rueckgabewert 0. Ueber 3 900 zufaellige Anordnungen
  belegt: kein einziger Fall, den 0.3.0 fand und 0.5.0 verliert.
* **Eine Bildmaske deckte wieder auf, was verborgen war.** Wo `/SMask` und
  `/Mask` nebeneinander stehen, befolgte die eine Stelle das `/SMask` und
  die andere schrieb das `/Mask` mit. Es gibt jetzt eine einzige
  Entscheidungsstelle, aus der sich beide ableiten.

### ⚠ Wege, den Rechner lahmzulegen

* **Eine ToUnicode-Tabelle aus 6 kB ergab einen Abbruch**, aus 3 kB fast
  7 GB. Die Decke zaehlt Bytes und nicht Eintraege — eine zweite Bombe in
  derselben Funktion (ein sehr langer Zielstring) waere sonst durchgekommen.
* Die Hilfsdatei-Leser, die Bildmasken-Rekursion und vier superlineare
  Stellen sind geschlossen; die Konfliktaufloesung faellt von 74,6 s auf
  0,21 s bei 100 000 Kandidaten, die Trefferkoordinaten von 42,3 s auf
  0,078 s bei 32 000 Treffern.

### Neu

* **`--check-leaks <TEXT>` prueft eine fertige Datei auf Restdaten** — im
  ausgelieferten Binary, ohne Quelltext und ohne Netz. Die Anleitung dazu
  verlangte bisher eine Rust-Toolchain und einen Klon des Repositories. Die
  Gegenprobe: bei einer IBAN in einem komprimierten Objektstrom geben
  `pdftotext`, `strings` und `grep` uebereinstimmend Entwarnung.
* **Die Oberflaeche ist ohne Maus benutzbar.** Ein Rechteck liess sich nur
  mit der Maus erzeugen — und auf einem Kontoauszug sind Anschrift,
  Kontonummer und Kontoinhabername genau die Stellen, die von Hand gezogen
  werden muessen. `Strg+R` legt eines an, `Strg+Pfeil` aendert die Groesse.
  Ausserdem zerschnitt ein einziges ausgegrautes Bedienelement die
  Tabulator-Kette.
* **Eine entartete Seitengroesse brachte Oberflaeche und Kommandozeile
  auseinander**: die IBAN blieb im Fenster-Export stehen, waehrend die CLI
  mit derselben Einstellung beide Seiten schwaerzte.


### Nachgetragen

> Die folgenden Punkte sind **mit 0.5.0 ausgeliefert**, standen beim
> Veroeffentlichen aber noch unter „Unveroeffentlicht" und fehlen deshalb in
> den Release-Notizen von 0.5.0 auf GitHub. Hier stehen sie an der richtigen
> Stelle. Der Ablauf, der das verhindert, ist in 0.6.0 vermerkt.

### Neu

* **Die Nachprüfung steckt jetzt im ausgelieferten Binary:
  `--check-leaks <TEXT>`.** Der Schalter beantwortet die Frage, um die es bei
  einer Schwärzung am Ende geht — *steht dieser Text noch in der Datei?* — und
  sucht dafür auf allen Ebenen, auf denen ein Geheimnis überleben kann: rohe
  Dateibytes, jeder `stream … endstream`-Block (auch Flate-dekomprimiert),
  jedes Stream-Objekt dekodiert, die Objekte in `/ObjStm`-Containern, jedes
  Zeichenketten-Objekt unter jedem Schlüssel — in UTF-8, Latin-1/PDFDoc,
  UTF-16BE und als Hex-String.

  **Neu ist nicht die Suche, sondern ihre Erreichbarkeit.** `redact_pdf::leaks`
  gab es schon; der Schalter reicht sie durch und baut nichts nach. Bis
  einschließlich 0.4.0 war sie aber nur als Bibliotheksfunktion zu haben, und
  die README verwies dafür auf `cargo add --path …/crates/redact-pdf`. **Im
  Release-Archiv liegt kein Quelltext** (nachgesehen:
  `tar -tzf … | grep -c crates/` ⇒ 0), das Binary hatte kein entsprechendes
  Unterkommando, und der Verweis in der ausgelieferten README zeigte ins
  Leere. Wer nur das Release hatte — also die Zielgruppe —, konnte die
  wichtigste Kontrolle dieses Werkzeugs nicht ausführen, es sei denn mit
  Rust-Toolchain, Netzzugang zu crates.io und einem Klon eines privaten
  Repositories. Für ein Werkzeug, dessen erstes Versprechen „keine Cloud, keine
  Netzverbindung“ lautet, war das ein Bruch.

  Drei Festlegungen dazu:

  * **Rückgabewert `3` für einen Fund**, `0` für „keiner der Begriffe steht
    noch darin“. Ein Fund ist weder ein Verarbeitungsfehler (`1`) noch ein
    Bedienfehler (`2`): der Lauf ist gelungen, das *Ergebnis* ist es nicht.
    `0` wäre gefährlich — `redact-rs out.pdf --check-leaks "$IBAN" && versenden`
    verschickte die Datei mit der IBAN darin.
  * **Die Suchbegriffe sind Geheimnisse.** Auf der Kommandozeile stehen sie in
    der Prozessliste und in der Shell-Historie; `--check-leaks -` liest sie
    zeilenweise von der Standardeingabe
    (`redact-rs out.pdf --check-leaks - < begriffe.txt`) und nimmt keinen der
    beiden Wege. Nachgemessen mit `ps -o args=` während des Laufs.
  * **„Nichts gefunden“ wird nicht zur neuen falschen Entwarnung.** Jeder
    saubere Lauf sagt auf stdout dazu, dass damit genau diese Liste geprüft ist
    und sonst nichts.

  Ein Schalter des Schwärzens neben `--check-leaks` (`-o`, `--review`,
  `--patterns`, `--audit-log`, `--action` …) wird abgelehnt (`2`), statt
  wirkungslos mitzulaufen. Eine **verschlüsselte** Datei wird abgelehnt statt
  durchsucht: darin stehen die Zeichenketten verschlüsselt, eine Bytesuche
  fände auch dann nichts, wenn das Geheimnis noch darin steht. Die Eingabedatei
  geht durch dieselbe Typ- und Größenprüfung wie jede andere — eine benannte
  Pipe hält den Lauf nicht an.

### Behoben

* **Sieben relative Verweise in der ausgelieferten README zeigten ins Leere**,
  darunter der auf `crates/redact-pdf/src/audit_bytes.rs` — also genau der auf
  die Funktion, die den Abschnitt „Prüfen, ob die Schwärzung gewirkt hat“
  tragen sollte. Betroffen waren außerdem `crates/redact-pdf/tests/known_leaks.rs`,
  `crates/redact-pdf/tests/marked_content.rs`,
  `crates/redact-cli/tests/cli_and_gui_agree.rs`,
  `.github/workflows/release.yml`, `.github/workflows/ci.yml` und die Anleitung
  `cargo add --path /pfad/zu/redactrs/crates/redact-pdf`.

  Die Dateien sind weiterhin beim Namen genannt, aber nicht mehr verlinkt; der
  Abschnitt „Prüfen, ob die Schwärzung gewirkt hat“ ist neu geschrieben und
  **allein mit dem Release durchführbar**. Dass im Archiv kein Quelltext liegt,
  steht jetzt bei den Archivinhalten.

* **Die Aussage zu `--remap-path-prefix` im Eintrag zu 0.4.0 war zu weit
  gefasst.** Im ausgelieferten glibc-Binary stehen weiterhin drei Pfade des
  Build-Rechners — Build-Skript-Ausgaben unter `OUT_DIR`, die der Schalter
  nicht erfasst. musl und beide Windows-Artefakte sind sauber. Die Zahlen und
  die Erklärung stehen dort; die Behebung im Bauweg steht aus
  (`.github/workflows/`, z. B. `RUSTFLAGS` um ein zweites
  `--remap-path-prefix` für `$CARGO_TARGET_DIR` ergänzen oder die
  Debug-Informationen des Release-Baus abschalten).

### Geändert

* **Die Binärgrößen stehen nicht mehr als Zahlenreihe in der README.** Sie
  waren als v0.3.0-Messung ausgewiesen — ehrlich, aber mit v0.4.0 überholt.
  Die Akzeptanztabelle nennt jetzt nur noch das größte Artefakt und die
  Grenze; die Zahlen der Fassung, die jemand tatsächlich heruntergeladen hat,
  misst eine Zeile (`stat -c '%s %n' redact-rs`). Für v0.4.0 nachgemessen und
  gegen `SHA256SUMS-BINARIES` geprüft: `redact-rs.exe` 10 766 336,
  `redact-rs-gui.exe` 9 999 360, Linux/glibc 15 041 568, Linux/musl
  5 055 416 Byte — alle vier gewachsen, alle vier weit unter 30 MB.

---

## 0.4.0 — 2026-08-05

Bereich: `git log 9aa4808..v0.4.0`.

Diese Fassung ist das Ergebnis einer vollständigen Expertenprüfung aus fünf
Blickwinkeln — Bedrohung für den ausführenden Rechner, Kernversprechen der
Schwärzung, Bedienung, Doku und Freigabeweg, Leistung und Einfachheit. Jeder
Befund darin ist an einem echten Lauf belegt; die Zahlen unten sind gemessen,
nicht geschätzt.

### Neu

* **Die automatische Erkennung lässt sich abschalten — ganz und je Muster.**
  `--no-patterns` gab es schon; dazu kommen `--disable-pattern <ID>`
  (mehrfach oder kommagetrennt), der Schlüssel `disabled_patterns` in der
  Einstellungsdatei, ein Häkchen „Automatisch suchen“ samt aufklappbarer
  Musterliste in der Oberfläche und das Feld `patterns` im Audit-Log.

  **Der wichtigste Teil ist nicht der neue Schalter, sondern die Ansage.** In
  0.3.0 war `--no-patterns` **vollständig stumm**: kein Satz in der
  Zusammenfassung, keine Warnung auf stderr, kein Feld im Audit-Log
  (`detection_notice` und `PatternRecord` gibt es dort noch nicht — nachgesehen
  an `9aa4808`). Eine mit `--no-patterns` erzeugte Datei war von einer
  vollständig geprüften nicht zu unterscheiden — nicht am Ergebnis, nicht am
  Rückgabewert, nicht am Nachweis. „0 Treffer“ heißt dort nicht *nichts
  gefunden*, sondern *nicht gesucht*. **Wer 0.3.0 mit `--no-patterns`
  eingesetzt hat, sieht den Ergebnissen das nicht an und muss es aus dem
  Aufruf rekonstruieren.**

  Ab dieser Fassung sagt es jeder der drei Wege ausdrücklich, und alle drei
  speisen sich aus derselben Angabe:

  * die Zusammenfassung auf stdout, **direkt unter der Trefferzahl** — nicht
    nur auf stderr, denn `redact-rs … > bericht.txt` behielte sonst genau die
    harmlose Hälfte: `Automatische Erkennung: abgeschaltet (--no-patterns). …`
    bzw. `Automatische Erkennung: 1 Muster abgeschaltet (konto_nr). …`;
  * die Kopfzeile der Trefferliste in der Oberfläche
    (`Automatische Suche AUS — nicht gesucht, nur von Hand: …`), dazu ein Satz
    in Warnfarbe unter dem Schalter;
  * das Audit-Log als Feld `patterns`, das **auch dann** dasteht, wenn nichts
    abgeschaltet war (`{"all_disabled": false, "disabled": []}`) — ein Feld,
    das nur im Ausnahmefall erschiene, machte ein Log mit abgeschalteter
    Erkennung ununterscheidbar von einem Log einer älteren Fassung.

  Der **Rückgabewert bleibt 0**. Eine abgeschaltete Erkennung ist eine
  Anweisung des Aufrufenden und keine Deckungslücke: die Analyse hat das
  Dokument vollständig gelesen und auf Geheiß nach weniger gesucht. Spränge
  die 3 auch hier an, wäre sie für die Fälle wertlos, für die es sie gibt.
  Wer im Skript darauf prüfen will, liest `patterns` aus dem Audit-Log.

  Ein **unbekannter Musternamen beendet den Lauf mit Rückgabewert 2** und einer
  Meldung, die alle gültigen nennt; abgeschaltet und geschwärzt wird dann
  nichts. Ein stillschweigend übergangenes `--disable-pattern iban` sähe aus
  wie eine Abschaltung und wäre keine. `no_patterns` gibt es in der
  Einstellungsdatei bewusst **nicht** — der Schalter hat kein Gegenstück, aus
  einer Datei heraus wäre er nicht mehr zu widerrufen; `disabled_patterns` ist
  widerrufbar, weil eine Liste auf der Kommandozeile die aus der Datei ersetzt.

### ⚠ Sicherheit

* **Zwei weitere Stellen, an denen der Interpreter nichts sah, sind jetzt
  laut.** Beide endeten vorher mit „Treffer 0, Rückgabewert 0“ — dem
  schlimmsten Ergebnis, weil es sich wie „geprüft und sauber“ liest. Beide
  setzen jetzt den Rückgabewert **3** und stehen mit `NICHT GEPRÜFT` auf
  stderr:

  * ein **Form-XObject, das in `/Resources` steht, aber nirgends gezeichnet
    wird** — sein Text wurde nicht durchsucht. Ein Formular ohne Text bleibt
    unerwähnt, und eines, das nur eine andere Seite zeichnet, gilt nicht als
    Lücke; sonst spränge der Wert bei geteilten Ressourcen ständig an;
  * eine **weiche Maske (`/ExtGState /SMask /G`), die sich nicht lesen lässt** —
    keine eigene Objekt-Id, kein lesbarer Strom, nicht dekodierbar, zu tief
    verschachtelt oder nur teilweise zerlegbar.

  Eine weiche Maske, die sich **lesen** lässt, wird seit dieser Runde betreten
  und ihr Text mitgeschwärzt: sie ist ein vollwertiges Form-XObject mit eigenem
  Text, das kein `Do` je erreicht — der Interpreter kam bisher nie hinein,
  während `pdftotext` die IBAN im Klartext las.
* **Übereinander gedruckter Text wird wieder lesbar zusammengesetzt.**
  Fett-Imitat, Schlagschatten, Rückkern in einer `TJ`-Operation und mehrere
  Erscheinungsströme derselben Annotation verschränkten die Zeilenbildung
  zeichenweise zu `IIBBAANN::  DDEE8899 …` — kein Muster traf mehr, und der
  Lauf endete mit „Treffer 0“.
* **Was die Eingabe versteckt, bleibt in der Ausgabe versteckt.** Beim
  Schwärzen wird ein betroffenes Bild neu kodiert und sein Dictionary neu
  aufgebaut; eine dabei verlorene Maske (`/SMask`, `/Mask` als Stencil-Strom,
  `/Mask` als Farbschlüssel) dreht das Kernversprechen um — gerade eine Stelle,
  die die Eingabe unsichtbar macht, ist das Muster einer bereits mit einem
  anderen Werkzeug geschwärzten Stelle. Ein Stencil-Strom wird jetzt unverändert
  mitgeschrieben, ein Farbschlüssel in den Alphakanal gerechnet; was sich nicht
  sicher übertragen lässt, beendet den Lauf, statt still zu vereinfachen.
  Zwei Bilder, die sich gegenseitig als `/SMask` nennen, führten in eine
  endlose Rekursion.
* **Die Hilfsdateien haben eine feste Größengrenze.** Buchungsliste
  (`--booking-list`), Review-Datei (`--apply-review`) und Regionsliste
  (`--manual-regions`) **16 MB**, Musterkonfiguration (`--patterns-config`)
  **1 MB**; beide fest, ohne Schalter. Vorher gingen sie durch
  `std::fs::read`/`read_to_string`, und das legt einen Puffer in **Dateigröße**
  an, bevor irgendetwas geprüft ist: wie viel Arbeitsspeicher ein Lauf belegt,
  stand damit in der Datei. Nachgemessen an einer dünn belegten 6-GB-Datei
  hinter `--manual-regions`: vorher Exit 1 nach **24,0 s** bei **6 150 MB**,
  jetzt Exit 1 nach **0,00 s** bei **6,3 MB**.

  Das ist **keine** Vertrauensfrage — diese Dateien bringt der Bedienende
  selbst mit. Der wahrscheinlichste Weg dorthin ist kein Angriff, sondern ein
  vertippter Pfad: `--manual-regions` auf den 700-MB-Scan statt auf die
  JSON-Datei. Eine benannte Pipe oder ein Gerät wird abgelehnt, bevor die Größe
  überhaupt zur Sprache kommt (Länge 0, liefert endlos). Die Meldungen nennen
  Nennlänge, Grenze und die wahrscheinlichere Ursache.

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
* Der Pfad des Build-Rechners steht nicht mehr in dem Teil der ausgelieferten
  Binaries, den `--remap-path-prefix` erfasst — das ist der übersetzte
  Quelltext aller Crates. **Nicht erfasst sind die Ausgaben der Build-Skripte
  (`OUT_DIR`)**, und dort steht er weiterhin.

  Nachgemessen an den ausgelieferten v0.4.0-Artefakten
  (`strings -a -n 8 <binary> | grep -c /home/runner`):

  | Artefakt | Fundstellen |
  |---|---|
  | Linux/glibc `redact-rs` | **3** |
  | Linux/musl `redact-rs` | 0 |
  | `redact-rs.exe` | 0 |
  | `redact-rs-gui.exe` | 0 |

  Alle drei liegen unter
  `/home/runner/work/redactrs/redactrs/target/x86_64-unknown-linux-gnu/release/build/`
  und heißen `glutin_glx_sys-…/out/glx_extra_bindings.rs`,
  `glutin_glx_sys-…/out/glx_bindings.rs` und
  `glutin_egl_sys-…/out/egl_bindings.rs` — Dateien, die die Build-Skripte von
  `glutin_glx_sys` bzw. `glutin_egl_sys` (über `gl_generator`) zur Bauzeit in
  `OUT_DIR` erzeugen. `--remap-path-prefix` wirkt auf die Pfade, mit denen
  *Cargo* `rustc` aufruft, nicht auf einen absoluten Pfad, den ein Build-Skript
  selbst in `OUT_DIR` hineinschreibt. musl und beide Windows-Artefakte sind
  sauber, weil sie ohne Oberfläche (`--no-default-features`) bzw. ohne die
  X11-/EGL-Anbindung gebaut werden.

  **Was das heißt und was nicht:** preisgegeben ist der Pfad eines
  GitHub-Runners (`/home/runner/work/redactrs/redactrs`) — kein Geheimnis,
  aber auch nicht das, was der Eintrag zugesagt hat. Wer aus einem privaten
  Arbeitsbaum selbst baut, trägt seinen eigenen Pfad in dieses Artefakt.

  **Hier stand: „steht nicht mehr in den ausgelieferten Binaries“** — das war
  für das glibc-Artefakt falsch. Korrigiert am 2026-08-05, nachdem es an den
  Artefakten nachgemessen wurde; die Behebung selbst gehört in den Bauweg
  (siehe „Unveröffentlicht“).

  Was die Prüfsummen in `SHA256SUMS-BINARIES` belegen und was nicht, sagen die
  Release-Notizen jetzt ausdrücklich; die frühere Formulierung („Wer nachbauen
  will, vergleicht diesen Hash") war eine Zusage ohne Deckung.
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
* `README.md`, fünf Angaben, die als **Messung** dastanden und keine mehr
  waren — jede ist ersetzt durch eine, die für diesen Baum nachgefahren wurde:
  * dieselbe Stapel-Zusammenfassung mit zwei Zahlen wie in `SECURITY.md`; dazu
    fehlten in den Beispielen die Fortschrittszeilen auf stderr;
  * die Binärgrößen („Windows 7,4 MB / 7 395 328 Byte“, „Linux 13 MB“). An den
    **ausgelieferten** v0.3.0-Artefakten nachgemessen — gegen
    `SHA256SUMS-BINARIES` geprüft — sind es 10,6 MB (`redact-rs.exe`), 9,8 MB
    (`redact-rs-gui.exe`), 14,9 MB (Linux/glibc) und 4,9 MB (Linux/musl). Das
    Akzeptanzkriterium (< 30 MB) hält weiter, mit Abstand;
  * „1350 Schwärzungen in 77 ms“ für das 10-Seiten-Kriterium. 1350 Treffer
    stimmen; die Zeit für den ganzen Prozess ist **0,16 s** (bestes von fünf
    Läufen, Release);
  * die Zahl der Fälle in `cli_and_gui_agree.rs` stand an einer Stelle auf
    fünf, an der anderen auf sieben. Es sind sieben (`ok. 7 passed`);
  * „82×42 Bildpunkte“ für die gepolsterte Bildregion — die daneben genannten
    3696 geänderten Bildpunkte sind 84×44. Der Rasterrand wird nach **außen**
    gerundet, und das ist die sichere Richtung.
* `README.md`: „die Bildpunkte außerhalb der Schwärzung sind nachweislich
  unverändert, auch beim JPEG-Fall“ war zu stark. Für ein Flate-Bild stimmt es
  exakt (nachgemessen: null geänderte Bildpunkte außerhalb). Bei einem JPEG gilt
  es nur gegenüber dem **eigenen** Dekodat: gegen Pillow gemessen weichen 8231
  der 16 304 äußeren Bildpunkte ab, fast alle um höchstens 2 je Kanal. Das ist
  Dekoder-Rauschen und kein zurückgetragener Inhalt — aber wer es wörtlich
  nachprüfen will, braucht denselben Dekoder.
* `README.md`, Release-Rezept: es empfahl `printf '0.2.0\n' > .release-version`
  und nannte einen Tag als gleichwertige Alternative. Das erste warf den
  Kommentarkopf der Datei weg, der sagt, dass eine Änderung an ihr
  veröffentlicht; das zweite trifft nicht mehr zu, seit beide Wege durch
  dieselben zwei Vorbedingungen gehen (Default-Branch, grüne CI).
* `README.md`: der Änderungsverlauf war nirgends verlinkt, das musl-Archiv
  nirgends erwähnt, und die Liste des Archivinhalts kannte `CHANGELOG.md`
  noch nicht.

### ⚠ Bedienung — Wege, auf denen eine IBAN stehenbleiben konnte

* **Mit den Pfeiltasten ließ sich ein Treffer neben das Blatt schieben, und
  die Kopfzeile versprach ihn weiter.** `move_selected` war die einzige
  Rechteckänderung, die weder die Klemmung noch den gemeinsamen Schreibweg
  durchlief. Nach etwa sechzig Anschlägen lag das Rechteck komplett außerhalb
  der Seite — unsichtbar, weil der Maler dort abschneidet —, während die
  Kopfzeile unverändert „2 werden geschwärzt“ sagte und der Export ohne Fehler
  durchlief. Die Warnung kam erst danach. Über den Eckgriff war derselbe Weg
  längst geklemmt.

  Geklemmt wird jetzt **versetzend**, nicht schneidend: reines Beschneiden
  hätte den Balken am Blattrand bei jedem weiteren Anschlag schrumpfen lassen —
  genau die Richtung, die eine IBAN wieder hervorkommen lässt.

* **Ein mit den Pfeiltasten korrigierter Treffer galt nicht als Handarbeit.**
  „Analysieren“ oder eine Buchungsliste warf ihn deshalb **ohne Rückfrage**
  weg, während die vier anderen Wege zum selben Verlust fragen.

* **Regionen aus einer Review-Datei, die neben der Seite liegen, zählten als
  „wird geschwärzt“.** Sie tragen jetzt ein eigenes Wort in Liste und
  Kopfzeile und gehen weder in die Konfliktauflösung noch in den Export. Die
  Aussage steht damit *vor* der Arbeit statt als Warnung danach. Geklemmt wird
  hier bewusst **nicht**: eine Review-Datei ist die Angabe der Nutzerin.

* **Die Zahl an der Miniaturansicht zählte Zeilen statt Schwärzungen.** Eine
  Seite, auf der ein Schutzeintrag alles blockiert und drei Handrechtecke
  abgewählt sind, zeigte 5, obwohl dort nichts geschwärzt wird — und die
  Miniaturspalte ist der Ort, an dem man am Ende prüft, ob nichts stehen
  geblieben ist.

* Ein Seitenwechsel mitten im Zug am Eckgriff veränderte blind die Region auf
  der alten Seite, gerechnet mit der Geometrie der neuen. Bei zwei
  deckungsgleichen Zeilen gewann außerdem die Schwärzungsart der *abgewählten*
  — *ob* geschwärzt wurde stimmte, *wie* nicht. Und fünfzig Tastenanschläge
  leerten den gesamten Rückgängig-Stapel.

### Leistung

Vier Stellen wuchsen schneller als die Eingabe. Alle vier sind an
Messreihen belegt, und für jede ist nachgewiesen, dass sich am **Ergebnis**
nichts ändert — bei einem Werkzeug, das entscheidet, welche Schwärzung
wegfällt, wiegt das schwerer als die Geschwindigkeit.

* **Die Konfliktauflösung war weiterhin quadratisch — bei genau der Anordnung,
  die ein Kontoauszug erzeugt.** Der Streifenzug lief über die x-Achse;
  Treffer mit derselben x-Spanne, die sich nur in y unterscheiden — eine IBAN
  auf jeder Zeile, also eine Spalte — fielen nie aus dem Streifen. 100 000
  Kandidaten kosteten **74,6 s**, jetzt 0,21 s. Der vorhandene Test wählte
  ausgerechnet die Anordnung, die nicht wehtut; er prüft jetzt beide.

  Die Suche stützt sich auf ein Gitter und darf sich nur deshalb auf die Zelle
  des Mittelpunkts beschränken, weil beide Schwellen bei mindestens 50 %
  liegen. Diese Kopplung ist als **Compile-Fehler** verankert, nicht als
  Kommentar: wer eine Schwelle senkt, macht die Suche lautlos unvollständig.

* **Ein oft platziertes Form-XObject kostete 4,4 ms Rechenzeit je zusätzlichem
  Dateibyte.** Strom und Schriftverzeichnis wurden bei *jeder* Platzierung neu
  ausgepackt; eine Platzierung sind sechs Byte in der Datei. 1 600
  Platzierungen in einer 231-kB-Datei brauchten **126,5 s** — ohne Abbruch,
  ohne Warnung, Rückgabewert 0, obwohl die README ausdrücklich Schutz gegen
  diese Vervielfachung verspricht. Jetzt 0,65 s. Dieselbe Vervielfachung lag
  bei weichen Masken, Kachelmustern und Erscheinungsströmen.

* **Die Schwärzung wuchs quadratisch im Seiteninhalt.** Für jedes Rechteck
  wurde eine Markierung über *alle* Zeichen der Textoperation angelegt, und
  beide Größen wachsen mit dem Inhalt, weil Treffer aus Text entstehen. 25 600
  Kandidaten auf einer Seite: **23,3 s**, jetzt 2,9 s und genau linear.

* **Die Miniaturansichten kosteten je Einzelbild O(Seiten × Regionen).** Bei
  1 600 Seiten und 96 000 Regionen 743 ms je Bild, jetzt 0,8 ms.

### Prüfgrundlage

* **Zwei Schwellen, die entscheiden, ob eine Schwärzung verworfen wird, hielt
  kein Test.** Beide ließen sich auf einen falschen Wert setzen, ohne dass
  einer von 640 Tests anschlug.
* **Ein Test schlug auf grünem Code in einem von fünf Läufen fehl** — und
  verdeckte dabei die halbe Suite, weil `cargo test` nach dem ersten roten
  Testbinary abbricht.
* Die Prüfung der Aufwandsschranken zählt jetzt ausgepackte Ströme und
  geladene Schriftverzeichnisse statt Sekunden — deterministisch statt
  lastabhängig.

### Aufgeräumt

Eine Zusicherung, die nichts zusicherte; eine Hülle um ein einzelnes Feld;
ein `.max()`, das zwei Wahrheiten versöhnte, die nachweislich nie
auseinandergehen; zwei Funktionen ohne Aufrufer; und zwei öffentlich
einstellbare Felder, die keiner der 35 Aufrufer je gesetzt hat. Fünf Kopien
der Frage „ist das ein Bild?“ sind zu einer geworden — die abweichende von
ihnen löste eine indirekte Referenz auf und war unerreichbar; jetzt löst die
eine Fassung sie überall auf, was einen echten Fall von „Deckungslücke
gemeldet“ zu „Bild geschwärzt“ bewegt.

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
