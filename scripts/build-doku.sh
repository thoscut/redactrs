#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# build-doku.sh
#
# Baut die Doku als HTML-Buch mit mdBook.
#
# Verwendung:
#   ./scripts/build-doku.sh [ziel-verzeichnis]     (Vorgabe: target/doku)
#
# Die Markdown-Dateien liegen verteilt im Repository: README.md, SECURITY.md
# und CHANGELOG.md an der Wurzel, die Einzelheiten unter docs/. Sie verweisen
# relativ aufeinander (`../SECURITY.md`, `docs/pruefen.md`), und so sollen sie
# auf GitHub lesbar bleiben. mdBook verlangt dagegen alles unter *einem*
# Quellverzeichnis und kopiert jede Nicht-Markdown-Datei daraus mit - die
# Repository-Wurzel selbst kommt also nicht in Frage (target/, crates/).
#
# Deshalb stellt dieses Skript die Quelle in derselben Lage zusammen, wie sie
# im Repository liegt, nur ohne den Rest. Damit zeigt jeder relative Verweis
# im gebauten Buch auf die Seite, auf die er im Repository zeigt.
# ---------------------------------------------------------------------------
set -euo pipefail

WURZEL="$(cd "$(dirname "$0")/.." && pwd)"
ZIEL="${1:-$WURZEL/target/doku}"
ARBEIT="$WURZEL/target/doku-quelle"

if ! command -v mdbook > /dev/null 2>&1; then
    echo "Fehler: mdbook fehlt - cargo install --locked mdbook@0.5.4" >&2
    exit 1
fi

rm -rf "$ARBEIT"
mkdir -p "$ARBEIT/src"
cp "$WURZEL/book/book.toml" "$ARBEIT/"
cp "$WURZEL/book/SUMMARY.md" "$ARBEIT/src/"

# Die Seiten des Buchs und alles, worauf sie verweisen: Bilder und Belege
# unter docs/, die Beispieldateien und die Lizenztexte.
for f in README.md SECURITY.md CHANGELOG.md CONTRIBUTING.md ROADMAP.md \
         PRUEFLISTE.md LICENSE-MIT LICENSE-APACHE; do
    cp "$WURZEL/$f" "$ARBEIT/src/"
done
cp -r "$WURZEL/docs" "$WURZEL/examples" "$ARBEIT/src/"

# mdBook endet auch nach einem ERROR mit Rückgabewert 0 (nachgemessen an
# 0.5.4 mit einem unbekannten Schlüssel in book.toml: ERROR im Log, Exit 0,
# und ein Buch ohne die Einstellung). Deshalb zählt hier das Log.
LOG="$ARBEIT/mdbook.log"
mdbook build "$ARBEIT" 2>&1 | tee "$LOG"
if grep -qE '(^|[[:space:]])(WARN|ERROR)[[:space:]]' "$LOG"; then
    echo "Fehler: mdbook hat gewarnt oder einen Fehler gemeldet (siehe oben)" >&2
    exit 1
fi

rm -rf "$ZIEL"
mkdir -p "$(dirname "$ZIEL")"
cp -r "$ARBEIT/html" "$ZIEL"
echo "Doku gebaut: $ZIEL/index.html"
