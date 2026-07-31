#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# build-windows.sh
#
# Baut die Windows-Binary von `redact-rs` lokal per Cross-Compiling
# (Linux -> x86_64-pc-windows-gnu) und legt sie unter dist/ ab.
#
# Voraussetzungen:
#   * Rust-Target:  rustup target add x86_64-pc-windows-gnu
#   * mingw-w64:    sudo apt-get install -y mingw-w64
#                   (liefert x86_64-w64-mingw32-gcc / -ar)
#
# Der Linker wird ueber .cargo/config.toml gesetzt; dieses Skript prueft
# lediglich, ob die Toolchain vorhanden ist, und gibt sonst eine klare
# Fehlermeldung aus.
#
# Verwendung:
#   ./scripts/build-windows.sh              # Standard-Features (inkl. GUI)
#   ./scripts/build-windows.sh --no-gui     # ohne egui-Oberflaeche
#
# Hinweis: Die offiziellen Release-Binaries werden mit dem MSVC-Target auf
# einem echten Windows-Runner gebaut (.github/workflows/release.yml). Dieses
# Skript ist fuer schnelle lokale Tests gedacht.
# ---------------------------------------------------------------------------
set -euo pipefail

TARGET="x86_64-pc-windows-gnu"
PACKAGE="redact-cli"
BIN="redact-rs.exe"

# Projektwurzel bestimmen (das Skript liegt in scripts/).
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
cd "${ROOT_DIR}"

# Eigenes Target-Verzeichnis, damit parallele native Builds nicht blockiert
# werden (Cargo haelt pro Target-Dir eine Datei-Sperre).
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${ROOT_DIR}/target-xbuild}"

# --- Argumente ------------------------------------------------------------
CARGO_FEATURE_ARGS=()
case "${1:-}" in
    --no-gui)
        CARGO_FEATURE_ARGS+=(--no-default-features)
        ;;
    "")
        ;;
    -h | --help)
        sed -n '2,25p' "${BASH_SOURCE[0]}"
        exit 0
        ;;
    *)
        echo "Fehler: unbekannte Option '${1}' (erlaubt: --no-gui, --help)" >&2
        exit 2
        ;;
esac

# --- Vorbedingungen pruefen ----------------------------------------------
if ! command -v cargo > /dev/null 2>&1; then
    echo "Fehler: 'cargo' wurde nicht gefunden. Bitte Rust installieren:" >&2
    echo "        https://rustup.rs" >&2
    exit 1
fi

if ! command -v x86_64-w64-mingw32-gcc > /dev/null 2>&1; then
    cat >&2 << 'EOF'
Fehler: Die mingw-w64 Cross-Toolchain fehlt.
        Es wurde kein 'x86_64-w64-mingw32-gcc' im PATH gefunden.

Installation:
  Debian/Ubuntu : sudo apt-get update && sudo apt-get install -y mingw-w64
  Fedora        : sudo dnf install -y mingw64-gcc
  Arch Linux    : sudo pacman -S mingw-w64-gcc
  macOS (brew)  : brew install mingw-w64
EOF
    exit 1
fi

if ! rustup target list --installed 2> /dev/null | grep -qx "${TARGET}"; then
    echo "Fehler: Das Rust-Target '${TARGET}' ist nicht installiert." >&2
    echo "        Nachinstallieren mit:" >&2
    echo "            rustup target add ${TARGET}" >&2
    exit 1
fi

# --- Bauen ----------------------------------------------------------------
echo "==> Cross-Build fuer ${TARGET}"
echo "    Paket           : ${PACKAGE}"
echo "    Features        : ${CARGO_FEATURE_ARGS[*]:-<default, inkl. gui>}"
echo "    CARGO_TARGET_DIR: ${CARGO_TARGET_DIR}"
echo

cargo build \
    --release \
    --target "${TARGET}" \
    --package "${PACKAGE}" \
    ${CARGO_FEATURE_ARGS[@]+"${CARGO_FEATURE_ARGS[@]}"}

BUILT="${CARGO_TARGET_DIR}/${TARGET}/release/${BIN}"
if [[ ! -f "${BUILT}" ]]; then
    echo "Fehler: Erwartete Datei wurde nicht erzeugt: ${BUILT}" >&2
    exit 1
fi

# --- Ergebnis nach dist/ kopieren ----------------------------------------
DIST_DIR="${ROOT_DIR}/dist"
mkdir -p "${DIST_DIR}"
cp -f "${BUILT}" "${DIST_DIR}/${BIN}"

echo
echo "==> Fertig: ${DIST_DIR}/${BIN}"
if command -v file > /dev/null 2>&1; then
    file "${DIST_DIR}/${BIN}"
fi
if command -v sha256sum > /dev/null 2>&1; then
    (cd "${DIST_DIR}" && sha256sum "${BIN}")
fi
