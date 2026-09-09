#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# make-preview.sh
#
# Erzeugt saemtliche Belege unter docs/ mit einem Aufruf.
#
# Verwendung:
#   ./scripts/make-preview.sh [zielverzeichnis]     (Vorgabe: docs)
#
# Geschrieben werden genau sechs Dateien:
#   vorher.png       Seite 1 der Beispieldatei aus `redact-rs --write-demo`
#   nachher.png      dieselbe Seite nach `redact-rs kontoauszug.pdf`
#   schwaerzung.gif  Animation A: dieselbe Seite ueber vier echte Ausgabedateien
#   konsole.gif      Animation B: der Lauf in der Konsole
#   konsole.txt      der Mitschnitt, aus dem konsole.gif gesetzt wurde
#   pruefung.txt     die Ausgaben von `--check-leaks` vorher und nachher
#
# WARUM ES DIESES SKRIPT GIBT
# Ein Bild in einer README ist eine Behauptung. Von Hand erzeugt ist es eine
# unpruefbare Behauptung: niemand sieht, ob der schwarze Balken aus dem
# Programm kommt oder aus einem Bildbearbeitungsprogramm. Deshalb entsteht
# hier jedes Byte aus einem Lauf des Programms selbst - die PDF-Dateien aus
# `--write-demo` und aus gewoehnlichen Aufrufen, die Bilder aus dem eigenen
# Rasterizer (`redact-render`), die Konsolenschrift aus dem eigenen
# Font-Rasterizer, der Nachweis aus `--check-leaks`.
#
# Was hier NICHT passiert: nachtraegliches Bearbeiten, Zusammensetzen,
# Beschriften, Aufhellen. Kein Ueberblenden, keine gezeichneten Zeiger, keine
# Pfeile. Der einzige Eingriff ist der Zuschnitt, und der ist fuer alle Bilder
# einer Gruppe derselbe (--crop in den Beispielprogrammen).
#
# DIE VIER SCHRITTE DER ANIMATION A - AUSDRUECKLICH
# Die vier Einzelbilder sind vier *wirklich geschriebene* PDF-Dateien. Damit
# eine Schwaerzung nach der anderen dazukommt, laeuft je Schritt ein eigener
# Aufruf mit einer anderen Musterauswahl (siehe SCHRITTE unten). Der
# gewoehnliche Aufruf `redact-rs kontoauszug.pdf` macht alle Schwaerzungen auf
# einmal - die Reihenfolge ist zum Zeigen gemacht und wird hier und in
# docs/vorher-nachher.md auch so benannt.
# ---------------------------------------------------------------------------
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target_dir="${1:-$repo_root/docs}"
# Wohin cargo baut. CARGO_TARGET_DIR gilt fuer `cargo build` von selbst; der
# Pfad zu den gebauten Programmen unten muss dieselbe Variable lesen, sonst
# baut das Skript an einen Ort und sucht an einem anderen (Abbruch mit 127).
#
# Ein relativer Wert (`CARGO_TARGET_DIR=target`) meint bei cargo das
# Verzeichnis, in dem cargo laeuft - das ist unten `$repo_root`. Das Skript
# wechselt danach in ein Wegwerfverzeichnis; ein relativer Pfad zeigte von
# dort ins Leere. Deshalb wird er hier gegen `$repo_root` aufgeloest, bevor
# irgendwo hin gewechselt wird.
build_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
case "$build_dir" in /*) ;; *) build_dir="$repo_root/$build_dir" ;; esac

# Aufloesung: die Seite wird mit BREITE Pixeln gerendert und danach auf den
# beschriebenen Teil zugeschnitten; uebrig bleiben rund 770 Pixel Breite.
BREITE=1500
# Die Animation braucht weniger, weil sie in der README neben Text steht und
# vier Einzelbilder traegt.
BREITE_GIF=1100
SEITE=1

# Die Suchbegriffe der Nachpruefung. Bewusst auch "Max Mustermann", obwohl -
# nein: WEIL - das Werkzeug ihn nicht schwaerzt. Es gibt kein Namensmuster,
# und ein Beleg, der das verschweigt, waere ein schoenerer Beleg als die
# Wahrheit hergibt.
BEGRIFFE=(
  "DE89 3704 0044 0532 0130 00"
  "COBADEFFXXX"
  "532013000"
  "Max Mustermann"
)

# Die vier Schritte der Animation A: je ein Aufruf, je eine Ausgabedatei.
# Schritt 0 laeuft mit --no-patterns - er zeigt die Seite, bevor irgendetwas
# geschwaerzt wurde, und ist trotzdem eine Ausgabedatei des Programms.
SCHRITTE=(
  "--no-patterns"
  "--patterns iban_de"
  "--patterns iban_de,bic"
  "--patterns iban_de,bic,konto_nr"
)

# Dieselben Begriffe als Kommandozeile, wie man sie abtippen wuerde.
befehl=""
for begriff in "${BEGRIFFE[@]}"; do
  befehl+=" \\\\\n      --check-leaks \"$begriff\""
done

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

cd "$repo_root"

echo "== 1/6  Programm und Belegwerkzeuge bauen ================================="
cargo build --quiet -p redact-cli --bin redact-rs
cargo build --quiet -p redact-render --example page_to_png
cargo build --quiet -p redact-render --example redaction_gif
cargo build --quiet -p redact-render --example console_gif
cli="$build_dir/debug/redact-rs"
png="$build_dir/debug/examples/page_to_png"
agif="$build_dir/debug/examples/redaction_gif"
cgif="$build_dir/debug/examples/console_gif"

echo
echo "== 2/6  Demo erzeugen und schwaerzen ======================================"
# Ab hier im Arbeitsverzeichnis, damit in der mitgeschriebenen Ausgabe
# `kontoauszug.pdf` steht und nicht der Pfad eines Wegwerfordners.
cd "$work"
# --write-demo schreibt die Beispieldatei, die auch die README benutzt.
"$cli" --write-demo kontoauszug.pdf
# Der gewoehnliche Aufruf: keine Sonderoptionen, keine Handarbeit an Regionen.
"$cli" kontoauszug.pdf -o kontoauszug_geschwaerzt.pdf

echo
echo "== 3/6  Die vier Schritte fuer Animation A ================================"
schritt_dateien=()
for index in "${!SCHRITTE[@]}"; do
  opts="${SCHRITTE[$index]}"
  datei="schritt$index.pdf"
  echo "--- $datei   (redact-rs kontoauszug.pdf $opts)"
  # Wortaufspaltung ist hier gewollt: SCHRITTE enthaelt fertige Schalterfolgen.
  # shellcheck disable=SC2086
  "$cli" kontoauszug.pdf -o "$datei" $opts 2>&1 | grep -E 'Schwärzungen|Deck-Rechtecke' || true
  schritt_dateien+=("$datei")
done

echo
echo "== 4/6  Bilder rendern ==================================================="
mkdir -p "$target_dir"
"$png" --page "$SEITE" --width "$BREITE" --crop \
  kontoauszug.pdf             "$target_dir/vorher.png" \
  kontoauszug_geschwaerzt.pdf "$target_dir/nachher.png"
echo
"$agif" --page "$SEITE" --width "$BREITE_GIF" --crop \
  "${schritt_dateien[@]}" "$target_dir/schwaerzung.gif"

echo
echo "== 5/6  Konsolenlauf mitschneiden und setzen =============================="
# Der Mitschnitt: `$ ` fuer den Befehl, `| ` fuer jede Zeile Ausgabe.
#
# Der angezeigte Befehl ist derselbe String, der ausgefuehrt wird - er wird
# einmal in eine Variable geschrieben, gedruckt und dann mit `bash -c`
# gestartet. Damit kann im Bild kein anderer Befehl stehen als der gelaufene.
#
# `sed 's/^/| /'` haengt das Praefix an, sonst wird an der Ausgabe nichts
# gemacht: nicht gekuerzt, nicht sortiert, nicht umgebrochen.
#
# Gearbeitet wird in einem frischen Unterordner, damit der Lauf genau die
# Form hat, die auch im Schnellstart steht - ohne -o und ohne --force.
mitschnitt="$target_dir/konsole.txt"
mkdir -p konsole
cp kontoauszug.pdf konsole/
pushd konsole >/dev/null

lauf_konsole() { # lauf_konsole <befehlszeile>
  echo "\$ ${1//$cli/redact-rs}"
  set +e
  bash -c "$1" 2>&1 | sed 's/^/| /'
  set -e
}
{
  echo "# Mitschnitt fuer docs/konsole.gif - erzeugt von scripts/make-preview.sh."
  echo "# '\$ ' = getippter Befehl, '| ' = eine Zeile Ausgabe eines echten Laufs,"
  echo "# '~ N' = N Hundertstelsekunden laenger stehen lassen."
  echo "#"
  echo "# Die Ausgabezeilen sind unveraendert; nur das Praefix '| ' kam dazu."
  echo "# Angezeigt und ausgefuehrt wird derselbe String - nur der Pfad des"
  echo "# gebauten Binaries ist durch seinen Namen ersetzt."
  lauf_konsole "$cli kontoauszug.pdf"
  echo "~ 60"
  echo "|"
  lauf_konsole "$cli kontoauszug_geschwaerzt.pdf --check-leaks \"${BEGRIFFE[0]}\""
  echo "~ 40"
  echo "|"
  echo '$ echo $?'
  # Der Rueckgabewert wird nicht behauptet, sondern noch einmal geholt.
  set +e
  "$cli" kontoauszug_geschwaerzt.pdf --check-leaks "${BEGRIFFE[0]}" >/dev/null 2>&1
  echo "| $?"
  set -e
} >"$mitschnitt"

popd >/dev/null
"$cgif" "$mitschnitt" "$target_dir/konsole.gif"

echo
echo "== 6/6  Nachpruefen, ob der Text weg ist =================================="
# Die Ausgabe wird mitgeschrieben, damit in docs/ nichts steht, was nicht aus
# einem Lauf stammt - vollstaendig, ohne Kuerzung, mit Rueckgabewert.
#
# `set +e`, weil --check-leaks bei einem Fund mit 3 zurueckkehrt. Hier ist
# genau das der erwartete Ausgang: vorher stehen alle vier Begriffe in der
# Datei, nachher noch der Name.
pruefung="$target_dir/pruefung.txt"
args=()
for begriff in "${BEGRIFFE[@]}"; do args+=(--check-leaks "$begriff"); done

lauf() { # lauf <ueberschrift> <datei>
  echo "--------------------------------------------------------------------"
  echo "$1"
  echo "--------------------------------------------------------------------"
  printf '$ redact-rs %s%b\n\n' "$2" "$befehl"
  set +e
  "$cli" "$2" "${args[@]}"
  local status=$?
  set -e
  echo
  echo "Rueckgabewert: $status"
  echo
}

{
  echo "Erzeugt von scripts/make-preview.sh mit $("$cli" --version)."
  echo "Ungekuerzte Ausgabe echter Laeufe - hier ist keine Zeile von Hand"
  echo "geschrieben, geloescht oder umgestellt."
  echo
  lauf "VORHER   die Datei aus \`redact-rs --write-demo kontoauszug.pdf\`" \
    kontoauszug.pdf
  lauf "NACHHER  dieselbe Datei nach \`redact-rs kontoauszug.pdf\`" \
    kontoauszug_geschwaerzt.pdf

  echo "--------------------------------------------------------------------"
  echo "DIE VIER SCHRITTE VON docs/schwaerzung.gif"
  echo "--------------------------------------------------------------------"
  echo "Jedes Einzelbild der Animation ist eine dieser Dateien. Die"
  echo "Musterauswahl steuert nur die Reihenfolge; der gewoehnliche Aufruf"
  echo "macht alle Schwaerzungen auf einmal."
  echo
  for index in "${!SCHRITTE[@]}"; do
    printf '  schritt%s.pdf   redact-rs kontoauszug.pdf %s\n' "$index" "${SCHRITTE[$index]}"
  done
  echo

  echo "--------------------------------------------------------------------"
  echo "GEGENPROBE  was die verbreitete Kontrolle dazu sagt"
  echo "--------------------------------------------------------------------"
  if command -v pdftotext >/dev/null 2>&1; then
    echo "\$ pdftotext kontoauszug_geschwaerzt.pdf - | grep -F 'DE89 3704'"
    set +e
    pdftotext kontoauszug_geschwaerzt.pdf - | grep -F "DE89 3704"
    status=$?
    set -e
    echo "Rueckgabewert: $status   (1 = kein Treffer)"
    echo
    echo "Das ist KEINE Entwarnung. grep prueft die eine Zeichenkette, nach"
    echo "der gefragt wurde, und pdftotext sieht nur den Seitentext. Was der"
    echo "Lauf darueber findet - \"Max Mustermann\" - taucht hier nicht auf,"
    echo "weil niemand danach gefragt hat. Der massgebliche Test ist"
    echo "--check-leaks; siehe README, Abschnitt \"Pruefen, ob die"
    echo "Schwaerzung gewirkt hat\"."
    echo
    echo "Version: $(pdftotext -v 2>&1 | head -1)"
  else
    echo "pdftotext ist auf dieser Maschine nicht vorhanden - Abschnitt"
    echo "entfaellt. Die Aussage der Gegenprobe steht in der README."
  fi
} >"$pruefung"

echo
echo "== Ergebnis =============================================================="
for datei in vorher.png nachher.png schwaerzung.gif konsole.gif konsole.txt pruefung.txt; do
  printf '%8d Byte  %s\n' "$(wc -c <"$target_dir/$datei")" "${target_dir#"$repo_root"/}/$datei"
done
