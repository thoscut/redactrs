# Mitarbeit an redact-rs

Dieses Werkzeug schwärzt Bankunterlagen. Ein Fehler darin ist kein
Schönheitsfehler, sondern eine Kontonummer, die weitergereicht wird. Das
prägt die Regeln unten — besonders den Abschnitt
[Der Prüfmaßstab](#der-prüfmaßstab), der der wichtigste dieser Datei ist.

**Eine Sicherheitslücke gehört nicht hierher.** Wie sie zu melden ist, steht
in [`SECURITY.md`](SECURITY.md) unter „Einen Fund melden“ — privat, nicht als
öffentliches Issue.

---

## Bauen

Rust **1.88** oder neuer (`rust-version` in `Cargo.toml`; der Job `msrv` in
`.github/workflows/ci.yml` misst das nach, statt es zu behaupten). Gebaut wird
mit der in [`rust-toolchain.toml`](rust-toolchain.toml) festgenagelten
Toolchain — `rustup` zieht sie beim ersten `cargo`-Aufruf im Verzeichnis
selbst, es ist nichts einzustellen.

```bash
git clone https://github.com/thoscut/redactrs
cd redactrs
cargo build --release                                     # mit Oberfläche
cargo build --release -p redact-cli --no-default-features # nur Kommandozeile
```

Das `-p redact-cli` im zweiten Aufruf ist nicht optional: `--no-default-features`
allein wirkt auf **alle** Mitglieder des Workspace, und `redact-gui` ist selbst
eines — es würde samt `eframe`/`egui`/`rfd` trotzdem gebaut.

Die Oberfläche braucht unter Linux Entwicklungspakete. Was die CI installiert,
steht im Job `test` in `.github/workflows/ci.yml`; das ist die maßgebliche
Liste:

```bash
sudo apt-get install -y --no-install-recommends \
    pkg-config libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
    libx11-dev libxcursor-dev libxrandr-dev libxi-dev \
    libgl1-mesa-dev libegl1-mesa-dev libfontconfig1-dev
```

Wer nur an der Kommandozeile oder an `redact-pdf` arbeitet, braucht davon
nichts: ohne die Oberfläche hat der Abhängigkeitsgraph keine C-Anteile. Beleg
ist der musl-Build des Release-Workflows — er baut
`-p redact-cli --no-default-features` statisch und installiert dafür kein
einziges Systempaket (Begründung in [`.cargo/config.toml`](.cargo/config.toml)).

## Das Gate

Diese drei Befehle müssen durchlaufen, bevor ein Beitrag eingereicht wird:

```bash
cargo test --workspace && \
cargo clippy --workspace --all-targets -- -D warnings && \
cargo fmt --all -- --check
```

Die CI führt dieselben aus, nur zusätzlich mit `--locked` (Job `test` in
`.github/workflows/ci.yml`) — lokal ohne, damit ein `cargo update` beim
Ausprobieren nicht sofort gegen die Wand läuft. Wer `Cargo.lock` anfasst,
nimmt `--locked` dazu.

Dazu kommt in der CI:

```bash
cargo check --workspace --locked                  # Job "msrv", Toolchain 1.88
cargo deny check advisories licenses bans sources # Job "supply-chain"
cargo audit                                       # ebenda
cargo clippy --workspace --all-targets --locked -- -D warnings  # Job "build-windows", nativ
```

Der Windows-Clippy ist kein Doppel des Linux-Laufs: `cfg(not(unix))`-Zweige
sieht nur er. Wer ihn lokal nachstellen will, braucht das Ziel, aber keinen
Linker (`rustup target add x86_64-pc-windows-gnu`):

```bash
cargo clippy --workspace --all-targets --locked \
  --target x86_64-pc-windows-gnu -- -D warnings
```

Clippy **prüft** den Windows-Zweig, es **führt** ihn nicht aus. Der einzige
Läufer, der `cfg(not(unix))` ausführt, ist der Job „Build (windows-2025)“ der
CI — lokal ist ein `cargo test` für Windows nicht zu haben (kein mingw-Linker;
`zf_q5_plattformzusagen` hält die Messung fest). Er hat nach der Fix-Runde 9
einen echten Fehler am Programm gefunden, den kein Linux-Lauf sehen konnte
(CHANGELOG, „Nach der Runde 9“). Deshalb gilt: **ein Befund, ein Commit — und
nach jedem Push wird der CI-Lauf angesehen, insbesondere der Windows-Job,
bevor die nächste Runde beginnt.** Ein Commit, der sieben Befundklassen
zugleich schließt, hinterlässt einen roten Job, dessen Ursache erst nach dem
Push sichtbar wird.

**Debug-Information.** Die Workspace-`Cargo.toml` setzt `[profile.dev]
debug = "line-tables-only"`. Ohne das trägt jedes Testbinary seine eigene volle
Kopie der Debug-Information aller Abhängigkeiten — an einem GUI-Testbinary
vorher 219,4 MB von 240,7 MB, und `cargo test --workspace` passte mit seinen
Testzielen nicht mehr in die Plattenzuteilung des Prüfrechners. Mit
Zeilentabellen sind es an demselben gebauten Binary 78,0 MB, Backtraces nennen
weiter Datei:Zeile, und am erzeugten Maschinencode ändert die Stufe nichts
(`zn_a_debug_info_stufe` bindet die Zeile und die Wirkung). Für Gate-Läufe
dazu `CARGO_INCREMENTAL=0`, wie die CI es setzt: inkrementelle Artefakte tragen
zum Gate nichts bei und kosten Platz.

Und im Release-Workflow, nicht in der CI, der Bau ohne Oberfläche:

```bash
cargo build --release --locked --target x86_64-unknown-linux-musl \
    -p redact-cli --no-default-features
```

`cargo deny` liest [`deny.toml`](deny.toml); Ziele und Features stehen dort und
nicht im Workflow, damit ein lokaler Lauf dasselbe prüft. Die Version ist
festgelegt (`cargo install --locked cargo-deny@0.20.2`), weil sich das
Konfigurationsschema zwischen den Reihen ändert.

**Kein `--allow-dirty`, kein `#[allow(...)]` als Abkürzung.** Eine Clippy-Regel
abzuschalten ist eine Entscheidung mit Begründung im Code, kein Weg am Gate
vorbei.

### Drei Spuren, eine Probenliste, ein Ende

Die Fix-Runden seit 0.6.0 hatten drei Aufgaben in einer Runde, und die
konvergieren verschieden. Seit der Runde 9 sind sie getrennt:

* **Spur A — Schwärzung.** `image.rs`, `meta.rs`, Spiegel und Formular,
  Orakel, Filterkette, Tiefe. **Nur sie zählt gegen das Ende.** Ende, wenn
  zwei aufeinanderfolgende Runden keinen Befund der Klassen *stilles Leck*,
  *abgelehnte gewöhnliche Datei* oder *Dienstverweigerung* liefern **und**
  jede Zeile der Probenliste in [`PRUEFLISTE.md`](PRUEFLISTE.md) in zwei
  Runden hintereinander grün und mutationsfest ist. Die Runde 9 hätte das nicht
  erfüllt (ein Leck, eine Ablehnung) — so streng soll es sein.
* **Spur B — Prüfapparat.** Die Bindungsmaschine (`zi_e`, `zj_e`, `zl_e`,
  `belege.rs`), Kopien von Helfern, Wächter, die an Zahlen hängen. Wird
  gesammelt und in einer eigenen, seltenen Runde geschlossen; zählt nicht
  gegen das Ende. **Die Bindungsmaschine ist eingefroren:** eine Verschärfung
  braucht einen Fall, in dem eine Zahl im geprüften Block tatsächlich falsch
  war — nicht ein Merkmal, das fehlen könnte.
* **Spur C — Doku.** Sätze hinter dem Code, Messzahlen ohne Weg. Wird beim
  Release-Schnitt bereinigt, nicht je Runde.

Das Mandat der letzten zwei Runden ist nicht mehr *widerlege* — eine Runde
mit diesem Auftrag kann nicht mit „nichts“ enden, das ist ihr Sinn —, sondern:
*prüfe, ob eine Zeile der Probenliste noch auftritt, und suche nach einer
Klasse, die nicht auf ihr steht.* Die erste Frage kann „nein“ beantworten, die
zweite hält die Liste ehrlich. Eine neue Klasse kommt als neue Zeile dazu, und
dann steht sichtbar, dass der Umfang gewachsen ist.

**Mutationsnachweis je Korrektur, mit Skript.** Jede Korrektur wird einmal
zurückgenommen, und der Test, der sie bindet, muss dabei rot werden — als
Skript, das den Baum aus einer Sicherungskopie wiederherstellt (nicht per
`git checkout`, das stellt den *committeten* Stand her, in dem die Korrektur
fehlen kann), mit `md5sum` vor und nach.

### Vor dem Commit zusätzlich: die Belege in `docs/`

Wer den **Rasterizer** (`redact-render`) oder die **Schwärzung** angefasst hat,
erzeugt die Bilder der README neu und prüft sie nach — sonst zeigt die
Startseite einen Stand, den der Code nicht mehr hat:

```bash
./scripts/make-preview.sh          # erzeugt docs/*.png, *.gif, *.txt neu
python3 scripts/check-preview.py docs   # prüft sie (36 Prüfungen, ~0,8 s)
git status --short docs/           # hat sich etwas bewegt?
```

**Bitgleichheit gilt nur auf derselben Maschine.** Dort liefert ein zweiter Lauf
dieselben SHA-256-Summen für alle sechs Dateien (nachgemessen); über
Maschinengrenzen ist das **nicht** zugesagt — `tiny-skia` rastert mit SIMD
(SSE bzw. NEON),
Fließkomma-Codegen darf sich zwischen `rustc`-Fassungen ändern, und die
PNG-Kompression hängt an flate2/miniz_oxide aus `Cargo.lock`. Ein `git status`,
der auf einem anderen Rechner Bytes meldet, ist deshalb **kein Befund** und
gehört nicht in einen Beitrag; maßgeblich ist, was `check-preview.py` sagt. Die
Begründung im Langen steht in
[`docs/vorher-nachher.md`](docs/vorher-nachher.md#wie-zuverlässig-ist-nachbauen).

---

## Der Prüfmaßstab

Dieses Projekt hat eine ungewöhnliche Prüfkultur, und sie hat einen Grund:
mehrfach stand hier Dokumentation, die mehr versprach als der Code hielt, und
mehrfach gab eine naheliegende Gegenprobe eine falsche Entwarnung. Wer hier
beiträgt, hält sich an drei Sätze.

### 1. Jeder Befund braucht einen echten Lauf

Keine Behauptung ohne Messung. „Sollte jetzt gehen“, „vermutlich behoben“,
„der Test deckt das ab“ sind keine Belege. In den Beitrag gehört, was
tatsächlich gelaufen ist: der Befehl, die Ausgabe, die Zahl. Ein Kommentar im
Code, der eine Grenze begründet, nennt die gemessene Zahl und nicht die
geschätzte.

Das gilt auch für die Gegenrichtung: ein Test, der grün ist, weil er die
Schutzmaßnahme gar nicht auslöst, ist schlimmer als kein Test. In `0.6.0`
wurden drei Schutzdecken auf „unbegrenzt“ gesetzt — 1 034 Tests blieben grün.
Ein neuer Test für eine Decke zeigt deshalb, dass er **rot** wird, wenn die
Decke fällt.

### 2. Der Maßstab ist `redact_pdf::leaks` — und nur der

Ob eine Zeichenkette wirklich aus einer Datei verschwunden ist, beantwortet
in diesem Projekt **eine** Funktion:
[`redact_pdf::leaks`](crates/redact-pdf/src/audit_bytes.rs). Hinter dem
Schalter `redact-rs <datei> --check-leaks "<text>"` steckt dieselbe Funktion,
und hinter der Nachprüfung der Oberfläche ebenfalls.

**Nicht der eigene Extraktor.** Der Extraktor dieses Projekts
(`redact_pdf::PdfExtractor`) beantwortet die Frage „was würde ein Leser
sehen?“. Er ist der falsche Zeuge dafür, ob etwas noch *in der Datei steht* —
er sieht genau das nicht, was er selbst nicht interpretiert. Ein Test, der
seine eigene Schwärzung mit dem eigenen Extraktor prüft, prüft, ob zwei Teile
desselben Codes dieselbe blinde Stelle haben.

### 3. `pdftotext | grep` gibt nachweislich falsche Entwarnung

Das ist keine Vorsichtsregel, sondern ein gemessener Fall. An einer 767 Byte
großen Datei mit einer IBAN in einem Flate-komprimierten Objektstrom
(`/ObjStm`) geben `pdftotext`, `strings` **und** `grep` auf der Rohdatei
übereinstimmend Entwarnung — `redact-rs --check-leaks` findet dieselbe IBAN an
zehn Stellen. Die vollständige Messung steht im README unter „Prüfen, ob die
Schwärzung gewirkt hat“, der Fall als Test im Baum
(`a_secret_in_a_compressed_object_stream_is_found` in
`crates/redact-cli/tests/check_leaks.rs`).

Gleiches gilt für eine Zeichenkette in UTF-16BE oder als Hex-String: für eine
reine Rohbyte-Suche unsichtbar.

**Und: „nicht gefunden“ ist kein Freibrief.** `--check-leaks` beantwortet
genau die Frage „steht *dieser* Text noch in der Datei?“ für genau die
übergebene Liste. Ein zweiter Name, eine andere Schreibweise, Text in einem
Rasterbild — nicht geprüft. Das Werkzeug sagt das nach jedem sauberen Lauf
selbst; wer es in einem Beitrag zitiert, zitiert es mit diesem Satz.

---

## Was das Projekt von einem Beitrag erwartet

**KISS.** Die einfachste Lösung, die das Problem wirklich löst. Eine
Abstraktion, die erst bei einem zweiten Anwendungsfall nützt, wartet auf den
zweiten Anwendungsfall. Eine neue Abhängigkeit braucht eine Begründung — der
Abhängigkeitsgraph ist Angriffsfläche eines Werkzeugs, das fremde PDFs liest.

**Tests, die den Befund festhalten.** Ein Fehler wird zusammen mit dem Test
behoben, der ihn zeigt. Die Testnamen hier sind ganze Sätze auf Englisch
(`a_secret_in_a_compressed_object_stream_is_found`) und beschreiben das
Verhalten, nicht die Funktion.

**Sprache.** Kommentare, Doku-Kommentare und alle Texte, die ein Benutzer zu
sehen bekommt, sind auf Deutsch. Bezeichner im Code und Testnamen sind auf
Englisch. Das ist gewachsen und bleibt so — Einheitlichkeit schlägt Geschmack.

**Kommentare erklären das Warum.** Der Code sagt, was passiert. Ein Kommentar,
der das wiederholt, ist Ballast; einer, der die Messung, den Fehlversuch oder
die verworfene Alternative festhält, ist der Grund, warum dieselbe Sackgasse
nicht zweimal betreten wird. Die vorhandenen Kommentare sind das Vorbild.

**Änderungsverlauf.** Verhaltensänderungen gehören nach
[`CHANGELOG.md`](CHANGELOG.md) unter „Unveröffentlicht“. Diese Datei erzeugt
die Release-Notizen — was dort fehlt, fehlt im Release. Ändert sich eine
Sicherheitszusage, wird der Eintrag mit **⚠ Sicherheit** gekennzeichnet.

**Was nicht in einen Beitrag gehört.** Echte Kontoauszüge, echte IBANs, echte
Namen, echte Adressen — auch nicht „nur zum Nachstellen“ und auch nicht in
einem Anhang. Die Testdaten dieses Repositories sind durchweg erfunden bzw.
die offiziellen Beispielwerte (`Max Mustermann`, `DE89 3704 0044 0532 0130 00`,
`@example.org`). Wer einen Fehler an einer echten Datei gefunden hat, baut die
kleinste künstliche Datei nach, die ihn auslöst. Ein Repository ist öffentlich
und bleibt es — in Klonen, in Archiven, in Suchmaschinen.

---

## Umgangston

Sachlich, knapp, in der Sache hart und gegenüber Personen freundlich. Kritik
gilt dem Code und der Messung, nicht dem, der sie eingereicht hat. Wer eine
Behauptung bestreitet, bestreitet sie mit einem Gegenlauf.

Dies ist ein kleines Projekt mit einem Verantwortlichen; über Grenzfälle
entscheidet der Eigentümer des Repositories, und im Zweifel schließt er einen
Vorgang. **Bewusst kein eigener Verhaltenskodex:** ein Contributor Covenant
verspricht ein Meldeverfahren mit einer Kontaktadresse und einer
Eskalationsstufe. Beides gibt es hier nicht, und einen Text hinzulegen, der
etwas zusichert, was niemand einlöst, wäre genau der Fehler, den dieses
Projekt an anderer Stelle mühsam abgestellt hat — Dokumentation, die mehr
verspricht als die Wirklichkeit hält. Kommt eine Gemeinschaft zustande, die
ein Verfahren braucht, wird ein Kodex nachgereicht, der eines beschreibt.

---

## Lizenz

Beiträge stehen unter denselben Bedingungen wie das Projekt: **MIT ODER
Apache-2.0**, nach Wahl des Nutzers ([`LICENSE-MIT`](LICENSE-MIT),
[`LICENSE-APACHE`](LICENSE-APACHE)). Wer etwas einreicht, erklärt sich damit
einverstanden.

Die mitgelieferten Schriften in `crates/redact-render/assets/fonts/` fallen
**nicht** darunter: sie sind aus den Liberation Fonts abgeleitet und stehen
unter der **SIL Open Font License 1.1**
([`crates/redact-render/assets/fonts/LICENSE-OFL.txt`](crates/redact-render/assets/fonts/LICENSE-OFL.txt)).
Klausel 3 der OFL verlangt, dass eine geänderte Fassung keinen Reserved Font
Name trägt — deshalb heißen sie hier „Redact Sans“, „Redact Serif“ und
„Redact Mono“. Wer sie austauscht oder ergänzt, hält diese Klausel ein und
trägt die Lizenz in `deny.toml` unter `[licenses] allow` nach, falls es eine
andere ist.
