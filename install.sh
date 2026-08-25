#!/usr/bin/env bash
# Install the Kaptein CLI and TUI from a signed GitHub release.
#
# This is the "Distribution & release sync" artifact (ROADMAP.md cross-cutting):
# it downloads the release binary for this platform, verifies its SHA-256
# checksum against the release's SHA256SUMS, and installs it to a user-writable
# location — no `cargo` required.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/egkristi/Kaptein/main/install.sh | bash
#   # or, to pick a version and install dir:
#   KAPTEIN_VERSION=v0.27.0 KAPTEIN_INSTALL_DIR="$HOME/.local/bin" ./install.sh
#
# Environment variables:
#   KAPTEIN_VERSION     release tag to install (default: latest)
#   KAPTEIN_INSTALL_DIR destination directory (default: ~/.local/bin)
#   KAPTEIN_BINARIES    which binaries to install (default: "kaptein kaptein-tui")

set -euo pipefail

REPO="egkristi/Kaptein"
BINARIES="${KAPTEIN_BINARIES:-kaptein kaptein-tui}"
INSTALL_DIR="${KAPTEIN_INSTALL_DIR:-$HOME/.local/bin}"

# Resolve the target triple for this platform (mirrors release.yml's matrix).
detect_target() {
  local os arch
  case "$(uname -s)" in
    Linux)  os="unknown-linux-gnu" ;;
    Darwin) os="apple-darwin" ;;
    *)
      echo "error: unsupported OS '$(uname -s)' (supported: Linux, macOS)" >&2
      exit 1
      ;;
  esac
  case "$(uname -m)" in
    x86_64|amd64) arch="x86_64" ;;
    aarch64|arm64) arch="aarch64" ;;
    *)
      echo "error: unsupported architecture '$(uname -m)'" >&2
      exit 1
      ;;
  esac
  echo "${arch}-${os}"
}

TARGET="$(detect_target)"
# The release ships one archive per target: tarball on unix, zip on Windows.
# (This script runs on Linux/macOS, so the artifact is always a .tar.gz.)
ARCHIVE="kaptein-${TARGET}.tar.gz"

# Determine the version: KAPTEIN_VERSION, else the latest release tag.
if [[ -z "${KAPTEIN_VERSION:-}" ]]; then
  if ! command -v gh >/dev/null 2>&1; then
    # No gh: query the GitHub API for the latest release tag.
    KAPTEIN_VERSION="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
      | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)"
  else
    KAPTEIN_VERSION="$(gh release view --repo "${REPO}" --json tagName --jq .tagName)"
  fi
fi

# Strip a leading "v" only when building the download URL (GitHub tags carry it).
VERSION_TAG="${KAPTEIN_VERSION#v}"

BASE_URL="https://github.com/${REPO}/releases/download/${KAPTEIN_VERSION}"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "==> Kaptein ${KAPTEIN_VERSION} (${TARGET})"
echo "==> Downloading ${ARCHIVE} ..."
curl -fsSL --retry 3 "${BASE_URL}/${ARCHIVE}" -o "${WORK}/${ARCHIVE}"
curl -fsSL --retry 3 "${BASE_URL}/SHA256SUMS" -o "${WORK}/SHA256SUMS"

echo "==> Verifying SHA-256 checksum ..."
(
  cd "${WORK}"
  # sha256sum -c reads "hash  filename" lines; the release ships that exact format.
  grep "  ${ARCHIVE}$" SHA256SUMS | sha256sum -c - >/dev/null
)

echo "==> Extracting ..."
tar -xzf "${WORK}/${ARCHIVE}" -C "${WORK}" kaptein kaptein-tui

mkdir -p "${INSTALL_DIR}"
for bin in ${BINARIES}; do
  install -m 0755 "${WORK}/${bin}" "${INSTALL_DIR}/${bin}"
  echo "==> Installed ${INSTALL_DIR}/${bin}"
done

# Ensure the install dir is on PATH (best effort).
case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    echo "==> Note: add ${INSTALL_DIR} to your PATH (it is not currently on it)."
    ;;
esac

echo "==> Done. Verify with: kaptein --version"
