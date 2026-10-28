# Entwicklung und Release

## Entwicklung

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build -p redact-cli --no-default-features       # ohne GUI
cargo test -p redact-pdf -- --ignored                 # Messungen und benannte Lücken
./scripts/build-windows.sh                            # Windows-Binary (mingw)
cargo run --release -p redact-pdf --example gen10 -- gross.pdf 10
```

### Die Doku als Buch bauen

Die Markdown-Dateien — `README.md`, `SECURITY.md`, `CHANGELOG.md` und alles
unter `docs/` — werden mit [mdBook](https://rust-lang.github.io/mdBook/) zu
einem HTML-Buch gebaut. Inhaltsverzeichnis und Einstellungen stehen in
`book/SUMMARY.md` und `book/book.toml`; eine neue Seite gehört in beide Welten,
also auch in `SUMMARY.md`.

```bash
cargo install --locked mdbook@0.5.4      # dieselbe Fassung wie in der CI
./scripts/build-doku.sh                  # erzeugt target/doku/index.html
python3 scripts/check-doku.py            # prüft jeden Verweis im Buch
```

`build-doku.sh` stellt die Quelle in derselben Verzeichnislage zusammen, in
der sie im Repository liegt, damit jeder relative Verweis auch im Buch
trifft. `check-doku.py` liest das gebaute HTML und meldet jede fehlende Datei
und jeden fehlenden Anker. Die CI tut beides im Job `doku`; der Release legt
das Buch als `doku/` in jedes Archiv.

### Die Belege in `docs/` neu erzeugen

Die Bilder ganz oben in der README sind keine Bildschirmfotos, sondern Ausgaben dieses
Programms. Wer am **Rasterizer** (`redact-render`) oder an der **Schwärzung**
etwas ändert, lässt danach diese beiden Befehle laufen — sonst zeigt die README
den Stand von vorgestern:

```bash
./scripts/make-preview.sh          # erzeugt docs/*.png, *.gif, *.txt neu
python3 scripts/check-preview.py docs   # prüft sie (54 Prüfungen, ~0,3 s)
```

`check-preview.py` ist ein zweiter, unabhängiger Leser in Python
(eigener PNG- und GIF-Dekoder, nur Standardbibliothek), damit ein Fehler im
eigenen Schreiber sich nicht selbst durchwinkt. Danach sagt `git status docs/`,
ob sich etwas bewegt hat.

Die Zahl der Prüfungen ist keine Zierde: das Skript druckt sie als letzte
Zeile (`Alle N Pruefungen bestanden.`), und
`crates/redact-cli/tests/belege.rs::die_zahl_der_pruefungen_steht_in_der_doku`
vergleicht sie mit der Zahl in diesem Satz. Hier stand „36 Prüfungen“, während
das Skript längst 54 meldete — abgeschrieben veraltet so eine Zahl still.

> **Bitgleichheit gilt nur auf derselben Maschine.** Dort liefert ein zweiter
> Lauf dieselben SHA-256-Summen für alle sechs Dateien (nachgemessen). Über
> Maschinengrenzen ist das **nicht** zugesagt: `tiny-skia` rastert mit SIMD
> (SSE bzw. NEON), Fließkomma-Codegen darf sich zwischen `rustc`-Fassungen
> ändern, und die PNG-Kompression hängt an flate2/miniz_oxide aus `Cargo.lock`.
> Ein `git status`, der auf einem anderen Rechner Bytes meldet, ist deshalb kein
> Befund. Darum prüft auch die CI **nicht** auf Bytegleichheit, sondern lässt
> `check-preview.py` über den eingecheckten **und** einen frisch erzeugten Satz
> laufen (Job `belege` in `.github/workflows/ci.yml`). Die Begründung im Langen
> steht in
> [`docs/vorher-nachher.md`](vorher-nachher.md#wie-zuverlässig-ist-nachbauen).

## Release bauen

**Zwei Vorbedingungen, an denen kein Weg vorbeiführt** (`verify` in
`.github/workflows/release.yml`):

1. **Der Commit muss auf dem Default-Branch liegen.** Geprüft mit
   `git merge-base --is-ancestor` gegen den Branch, den die GitHub-API als
   Default meldet. Ein Arbeitsbranch veröffentlicht nichts — auch nicht über
   einen Tag, auch nicht über `workflow_dispatch` mit eigenem `ref`.
2. **Die vollständige CI muss grün sein.** `release.yml` ruft
   `.github/workflows/ci.yml` als wiederverwendbaren Workflow auf
   (Job `ci`) und macht ihn zur Vorbedingung von `build` und `release` — es ist
   dieselbe Datei wie bei jedem Push, keine Kopie. Vorher gab es in diesem
   Ablauf keinen einzigen Test-, Clippy-, fmt- oder `cargo deny`-Schritt: aus
   rotem Code konnte ein öffentliches Binary werden, und genau das ist bei
   v0.3.0 passiert.

Dazu prüft `verify`, dass Tag, `.release-version` und die Version des Crates
`redact-cli` (über `cargo metadata`) dasselbe sagen, und nagelt den Ref **einmal**
auf eine Commit-ID fest, die dann CI, Bau und Veröffentlichung gemeinsam
benutzen. Eine Versionsnummer mit Bindestrich (`v0.4.0-rc.1`) wird auf jedem
Weg als Vorabversion gekennzeichnet.

**Der übliche Weg** ist die Versionsdatei. Sie trägt einen Kommentarkopf, der
sagt, dass eine Änderung an ihr veröffentlicht — der gehört nicht weggeworfen,
deshalb wird nur die Versionszeile ersetzt und nicht die ganze Datei:

```bash
# Nur die Versionszeile ersetzen; Kommentarkopf bleibt stehen.
sed -i 's/^[0-9].*/0.4.0/' .release-version
# Version in Cargo.toml gleichziehen (verify vergleicht beide)
git commit -am "Release 0.4.0"
git push origin HEAD:main          # Default-Branch, sonst passiert nichts
```

`publish-release.yml` reagiert auf Änderungen an `.release-version`, aber nur
auf `main`/`master` **und** nur, wenn das auch der tatsächliche Default-Branch
ist. Der Tag wird vom Release selbst am gebauten Commit angelegt; man legt ihn
nicht vorher an.

**Der Tag-Weg** existiert weiter (`git push origin v0.4.0` löst `release.yml`
direkt aus), führt aber durch dieselben beiden Vorbedingungen. Ein Tag auf einem
Arbeitsstand scheitert an Nr. 1:

```text
::error::Commit <sha> liegt nicht auf dem Default-Branch (main). Ein Release
entsteht nur aus Code, der dort angekommen ist. Erst zusammenführen, dann
veröffentlichen.
```

Der Weg über die Versionsdatei existiert zusätzlich, weil in abgeschotteten
Umgebungen häufig nur auf einen bestimmten Branch gepusht werden darf und
`workflow_dispatch` über die API gesperrt ist.

Gebaut werden **drei** Artefakte (Matrix in `release.yml`): Windows
(CLI + GUI), Linux/glibc (CLI + GUI) und Linux/musl (nur CLI, statisch). Dazu
kommen `SHA256SUMS` und `SHA256SUMS-BINARIES` als eigene Release-Dateien.
