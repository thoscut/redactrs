#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# make-archive.sh
#
# Packt ein Staging-Verzeichnis *reproduzierbar* in ein Archiv.
#
# Verwendung:
#   ./scripts/make-archive.sh <staging-verzeichnis> <ziel-archiv>
#
# Unterstuetzte Endungen des Ziels: .tar.gz und .zip
#
# WARUM NICHT EINFACH `tar -czf` / `Compress-Archive`?
# Beide schreiben Zeitstempel in das Archiv, `tar` zusaetzlich uid/gid und
# die Reihenfolge, in der das Dateisystem die Eintraege liefert. Ergebnis:
# zwei Laeufe desselben Commits erzeugen Archive mit unterschiedlicher
# SHA-256, obwohl das enthaltene Binary bitgleich ist. Eine
# `SHA256SUMS`-Datei bezeugt dann nur noch "der Hash des Tarballs, den
# dieser eine Lauf gebaut hat" - und niemand kann das Ergebnis nachbauen.
#
# Deshalb wird hier alles normalisiert, was nicht vom Inhalt abhaengt:
#   * Reihenfolge der Eintraege: alphabetisch (--sort=name / sorted())
#   * mtime aller Eintraege:     Epoch 0 bzw. 1980-01-01 (Minimum von ZIP)
#   * Besitzer:                  uid/gid 0, numerisch
#   * Rechte:                    755 fuer ausfuehrbare Dateien, sonst 644
#   * gzip-Header:               ohne Dateiname/Zeitstempel (gzip -n)
#
# Der ZIP-Zweig laeuft ueber Python (stdlib `zipfile`) statt ueber
# PowerShells `Compress-Archive`, weil letzteres keine Moeglichkeit bietet,
# die Zeitstempel zu setzen. Python ist auf allen GitHub-Runnern vorhanden.
# ---------------------------------------------------------------------------
set -euo pipefail

if [ "$#" -ne 2 ]; then
    echo "Verwendung: $0 <staging-verzeichnis> <ziel-archiv>" >&2
    exit 2
fi

STAGING="$1"
ARCHIVE="$2"

if [ ! -d "$STAGING" ]; then
    echo "Fehler: '$STAGING' ist kein Verzeichnis" >&2
    exit 1
fi

# Python heisst je nach Runner `python3` oder `python`.
PYTHON=""
for candidate in python3 python; do
    if command -v "$candidate" > /dev/null 2>&1; then
        PYTHON="$candidate"
        break
    fi
done

# --- Rechte normalisieren -------------------------------------------------
# Unter Git-Bash/Windows ist das wirkungslos, dort setzt der ZIP-Zweig die
# Attribute ohnehin selbst.
find "$STAGING" -type d -exec chmod 755 {} +
find "$STAGING" -type f -perm -u+x -exec chmod 755 {} +
find "$STAGING" -type f ! -perm -u+x -exec chmod 644 {} +

rm -f "$ARCHIVE"

case "$ARCHIVE" in
    *.tar.gz)
        # GNU tar: --sort=name gibt es seit 1.28, auf ubuntu-* vorhanden.
        if ! tar --version 2> /dev/null | head -n1 | grep -q 'GNU tar'; then
            echo "Fehler: fuer .tar.gz wird GNU tar benoetigt" >&2
            exit 1
        fi
        tar --sort=name \
            --mtime='@0' \
            --owner=0 --group=0 --numeric-owner \
            --format=gnu \
            -cf - -C "$STAGING" . \
            | gzip -n -9 > "$ARCHIVE"
        ;;
    *.zip)
        if [ -z "$PYTHON" ]; then
            echo "Fehler: fuer .zip wird python3 benoetigt (nicht gefunden)" >&2
            exit 1
        fi
        "$PYTHON" - "$STAGING" "$ARCHIVE" << 'PYEOF'
import os
import sys
import zipfile

src, dst = sys.argv[1], sys.argv[2]

entries = []
for root, dirs, files in os.walk(src):
    dirs.sort()
    files.sort()
    for name in files:
        full = os.path.join(root, name)
        rel = os.path.relpath(full, src).replace(os.sep, "/")
        entries.append((rel, full))
entries.sort(key=lambda e: e[0])

if not entries:
    sys.exit("Fehler: Staging-Verzeichnis ist leer: %s" % src)

# 1980-01-01 00:00:00 ist der kleinste Zeitstempel, den das ZIP-Format
# darstellen kann; alles Aeltere wuerde zipfile ablehnen.
FIXED_DATE = (1980, 1, 1, 0, 0, 0)

with zipfile.ZipFile(dst, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as zf:
    for rel, full in entries:
        info = zipfile.ZipInfo(rel, date_time=FIXED_DATE)
        info.compress_type = zipfile.ZIP_DEFLATED
        info.create_system = 3  # Unix, damit die Rechte-Bits ausgewertet werden
        executable = rel.endswith(".exe") or (
            os.name != "nt" and os.access(full, os.X_OK)
        )
        info.external_attr = (0o755 if executable else 0o644) << 16
        with open(full, "rb") as fh:
            zf.writestr(info, fh.read())

print("geschrieben: %s (%d Eintraege)" % (dst, len(entries)))
PYEOF
        ;;
    *)
        echo "Fehler: unbekannte Archivendung: $ARCHIVE (erlaubt: .tar.gz, .zip)" >&2
        exit 2
        ;;
esac

echo "==> $ARCHIVE"
if command -v sha256sum > /dev/null 2>&1; then
    sha256sum "$ARCHIVE"
fi
