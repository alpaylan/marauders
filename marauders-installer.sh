#!/bin/sh
set -eu

REPO="${REPO:-alpaylan/marauders}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${VERSION:-latest}"

usage() {
  cat <<'EOF'
Install Marauders release binaries.

Usage:
  sh marauders-installer.sh [--install-dir DIR] [--repo OWNER/REPO] [--version TAG|latest]

Environment variables:
  REPO         GitHub repository (default: alpaylan/marauders)
  INSTALL_DIR  Destination directory (default: $HOME/.local/bin)
  VERSION      Release tag (e.g., v0.0.12) or latest (default: latest)
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --install-dir)
      INSTALL_DIR="$2"
      shift 2
      ;;
    --repo)
      REPO="$2"
      shift 2
      ;;
    --version)
      VERSION="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)
    case "$ARCH" in
      x86_64|amd64)
        TARGET="x86_64-unknown-linux-gnu"
        ;;
      *)
        echo "unsupported architecture on Linux: $ARCH" >&2
        exit 1
        ;;
    esac
    ;;
  Darwin)
    case "$ARCH" in
      arm64|aarch64)
        TARGET="aarch64-apple-darwin"
        ;;
      x86_64|amd64)
        TARGET="x86_64-apple-darwin"
        ;;
      *)
        echo "unsupported architecture on macOS: $ARCH" >&2
        exit 1
        ;;
    esac
    ;;
  *)
    echo "unsupported OS: $OS" >&2
    exit 1
    ;;
esac

ARCHIVE="marauders-${TARGET}.tar.gz"
if [ "$VERSION" = "latest" ]; then
  URL="https://github.com/${REPO}/releases/latest/download/${ARCHIVE}"
else
  URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARCHIVE}"
fi

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

echo "Downloading ${ARCHIVE} from ${URL}"
curl -fsSL "$URL" -o "${TMP_DIR}/${ARCHIVE}"
tar -xzf "${TMP_DIR}/${ARCHIVE}" -C "$TMP_DIR"

PKG_DIR="${TMP_DIR}/marauders-${TARGET}"
MAIN_BIN="${PKG_DIR}/marauders"
IMPORT_BIN="${PKG_DIR}/marauders-import-rust-mutants"

if [ ! -f "$MAIN_BIN" ] || [ ! -f "$IMPORT_BIN" ]; then
  echo "unexpected archive layout in ${ARCHIVE}" >&2
  exit 1
fi

if [ ! -d "$INSTALL_DIR" ]; then
  PARENT_DIR="$(dirname "$INSTALL_DIR")"
  if [ -w "$PARENT_DIR" ]; then
    mkdir -p "$INSTALL_DIR"
  elif command -v sudo >/dev/null 2>&1; then
    sudo mkdir -p "$INSTALL_DIR"
  else
    echo "cannot create install dir and sudo is unavailable: $INSTALL_DIR" >&2
    exit 1
  fi
fi

install_copy() {
  src="$1"
  dst="$2"
  if [ -w "$INSTALL_DIR" ]; then
    install -m 0755 "$src" "$dst"
  elif command -v sudo >/dev/null 2>&1; then
    sudo install -m 0755 "$src" "$dst"
  else
    echo "install dir is not writable and sudo is unavailable: $INSTALL_DIR" >&2
    exit 1
  fi
}

install_copy "$MAIN_BIN" "${INSTALL_DIR}/marauders"
install_copy "$IMPORT_BIN" "${INSTALL_DIR}/marauders-import-rust-mutants"

echo "Installed:"
echo "  ${INSTALL_DIR}/marauders"
echo "  ${INSTALL_DIR}/marauders-import-rust-mutants"
echo "Run 'marauders --help' to verify."
