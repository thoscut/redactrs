# Installation

Fertige Binaries liegen unter
[Releases](https://github.com/thoscut/redactrs/releases) — **drei Archive**:

| Archiv | Inhalt | wofür |
|---|---|---|
| `redact-rs-<version>-x86_64-windows.zip` | `redact-rs.exe` + `redact-rs-gui.exe` | Windows, mit Oberfläche |
| `redact-rs-<version>-x86_64-linux.tar.gz` | `redact-rs` (glibc) | Linux, mit Oberfläche |
| `redact-rs-<version>-x86_64-linux-musl.tar.gz` | `redact-rs` (statisch, **ohne** Oberfläche) | Linux-Fassungen, auf denen der glibc-Bau nicht läuft — Debian 12, RHEL/Alma/Rocky 8 und 9, Ubuntu 22.04. Keine Systemvoraussetzungen. |

Das musl-Archiv ist die reine Kommandozeile (`--no-default-features`): kein
`--gui`, kein `redact-rs-gui`. Alles andere ist identisch.

Das Windows-Archiv enthält **zwei Programme**:

| Datei | wofür |
|---|---|
| `redact-rs-gui.exe` | Die zum **Doppelklicken**. Öffnet nur das Fenster, ohne die schwarze Konsole dahinter, und nimmt ein PDF entgegen, das man auf ihr Symbol zieht. |
| `redact-rs.exe` | Die **Konsolenfassung** mit allen Optionen — und der einzige Weg zu Ausgaben auf stdout (`--json`, `--list-patterns`). |

Zwei Dateien statt eines Schalters, weil `#![windows_subsystem = "windows"]`
das Konsolenfenster nur um den Preis *jeder* Ausgabe auf stdout/stderr abschaltet
(siehe `crates/redact-cli/src/bin/redact-rs-gui.rs`). Unter Linux gibt es diese
Trennung nicht; dort startet `redact-rs` ohne Argumente die Oberfläche.

Dazu liegt in **jedem** der drei Archive (nachgesehen in
`.github/workflows/release.yml`):

* die `README.md`, die Doku unter `docs/` (alle `*.md`) und
  [`SECURITY.md`](../SECURITY.md) — sonst liefen die Verweise in beiden
  Richtungen ins Leere,
* dieselbe Doku als **HTML-Buch** im Verzeichnis `doku/` — Einstieg
  `doku/index.html`, mit Suche und ohne Netzzugang lesbar (gebaut mit
  mdBook, siehe [Entwicklung](entwicklung.md#die-doku-als-buch-bauen)),
* [`CHANGELOG.md`](../CHANGELOG.md) — wer eine ältere Fassung ersetzt, muss ohne
  Netzzugang sehen können, welche Lecks dazwischen geschlossen wurden. Seit
  v0.4.0 packt der Release-Job ihn mit; die v0.3.0-Archive enthalten ihn noch
  nicht (nachgesehen in den ausgelieferten Archiven).
* die drei Lizenztexte `LICENSE-MIT`, `LICENSE-APACHE` und `LICENSE-OFL.txt`
  (die eingebetteten Schriften stehen unter der SIL Open Font License),
* das Verzeichnis `examples/`.

**Quelltext liegt in keinem der Archive** (nachgesehen: `tar -tzf … | grep -c
crates/` ⇒ 0). Deshalb sind Dateien aus dem Quellbaum in dieser Doku zwar beim
Namen genannt — `crates/redact-pdf/src/audit_bytes.rs`,
`.github/workflows/release.yml` —, aber **nicht verlinkt**: ein relativer
Verweis zeigte aus dem Archiv heraus ins Leere. Und alles, was diese Doku zum
Bedienen des Werkzeugs anleitet, kommt ohne Quelltext, ohne Rust-Toolchain und
ohne Netzzugang aus. Das gilt ausdrücklich für
[Prüfen, ob die Schwärzung gewirkt hat](pruefen.md#pruefen): dort stand bis 0.4.0 eine
Anleitung, die beides verlangte — heute ist es ein Schalter des Programms.

**Die SHA-256-Prüfsummen liegen *nicht* im Archiv.** Sie sind eigene
Release-Dateien und müssen getrennt heruntergeladen werden — was auch der
einzige Weg ist, auf dem sie etwas beweisen: eine Prüfsumme im geprüften
Archiv prüft nichts.

| Release-Datei | Inhalt |
|---|---|
| `SHA256SUMS` | Prüfsummen der Archive |
| `SHA256SUMS-BINARIES` | Prüfsummen der entpackten Binaries |

```bash
sha256sum -c SHA256SUMS                              # Linux
Get-FileHash <datei> -Algorithm SHA256               # Windows (PowerShell)
```

Selbst bauen (benötigt **Rust 1.88** oder neuer — `rust-version` in
`Cargo.toml`):

```bash
git clone https://github.com/thoscut/redactrs
cd redactrs
cargo build --release                                    # mit GUI
cargo build --release -p redact-cli --no-default-features # nur CLI
# Binary: target/release/redact-rs
```

Das `-p redact-cli` im zweiten Aufruf ist nicht optional: `--no-default-features`
allein wirkt auf **alle** Mitglieder des Workspace, und `redact-gui` ist selbst
ein Mitglied — es würde also samt `eframe`/`egui`/`rfd` trotzdem gebaut. Erst die
Paketauswahl nimmt die Oberfläche aus dem Abhängigkeitsgraphen (dasselbe tut
`scripts/build-windows.sh --no-gui`).

`cargo build --release` erzeugt **drei** ausführbare Dateien, von denen nur die
ersten beiden ausgeliefert werden:

| Binary | Herkunft | Status |
|---|---|---|
| `redact-rs` | `crates/redact-cli/src/main.rs` | die Konsolenfassung, im Release enthalten |
| `redact-rs-gui` | `crates/redact-cli/src/bin/redact-rs-gui.rs` | die Fensterfassung, im Windows-Archiv enthalten |
| `redact-gui` | `crates/redact-gui/src/main.rs` | Nebenprodukt des GUI-Crates, **nicht** im Release und nirgends dokumentiert außer hier. Es öffnet dasselbe Fenster wie `redact-rs-gui`, kennt aber `-h`/`-V` und weist unbekannte Optionen zurück (`redact-rs-gui` übergeht sie, weil es unter Windows ohne Konsole nichts melden könnte). Beim Entwickeln praktisch, weil es ohne `redact-cli` gebaut wird. Für den Einsatz ist `redact-rs` bzw. `redact-rs-gui` gemeint. |

Voraussetzungen zum Bauen der GUI unter Linux:

```bash
sudo apt-get install libgtk-3-dev libxkbcommon-dev libwayland-dev
```
