#!/usr/bin/env bash
#
# devcx installer — downloads the latest release binary from GitHub Releases.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/shutx-net/devcx/main/install.sh | bash
#
# Environment overrides:
#   DEVCX_INSTALL_DIR  install into this directory
#                      (default: /usr/local/bin as root, ~/.local/bin otherwise)
#   DEVCX_VERSION      install a specific release tag, e.g. v0.1.0
#                      (default: latest release)

set -euo pipefail

REPO="shutx-net/devcx"
BIN="devcx"

err() {
  echo "error: $*" >&2
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || err "required command not found: $1"
}

need curl
need tar

# --- Detect platform --------------------------------------------------------
os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
  Linux)
    case "$arch" in
      x86_64 | amd64) target="x86_64-unknown-linux-musl" ;;
      aarch64 | arm64) target="aarch64-unknown-linux-musl" ;;
      *) err "unsupported architecture: $arch. See the README for manual install or 'cargo install'." ;;
    esac
    ;;
  *)
    err "unsupported platform: $os. See the README for manual install or 'cargo install'."
    ;;
esac

# --- Resolve download URL ---------------------------------------------------
# taiki-e/upload-rust-binary-action names the checksum file after the binary
# and target only — it does NOT include the archive's .tar.gz suffix.
archive="${BIN}-${target}.tar.gz"
checksum="${BIN}-${target}.sha256"
if [ -n "${DEVCX_VERSION:-}" ]; then
  base_url="https://github.com/${REPO}/releases/download/${DEVCX_VERSION}"
else
  base_url="https://github.com/${REPO}/releases/latest/download"
fi

# --- Download and verify ----------------------------------------------------
tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

echo "Downloading ${base_url}/${archive} ..." >&2
curl -fsSL -o "${tmpdir}/${archive}" "${base_url}/${archive}"
curl -fsSL -o "${tmpdir}/${checksum}" "${base_url}/${checksum}"

(
  cd "$tmpdir"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c "${checksum}" >&2
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c "${checksum}" >&2
  else
    err "neither sha256sum nor shasum found; cannot verify download"
  fi
)

tar -xzf "${tmpdir}/${archive}" -C "$tmpdir"
[ -f "${tmpdir}/${BIN}" ] || err "archive did not contain the ${BIN} binary"

# --- Install ----------------------------------------------------------------
if [ -n "${DEVCX_INSTALL_DIR:-}" ]; then
  install_dir="$DEVCX_INSTALL_DIR"
elif [ "${EUID:-$(id -u)}" -eq 0 ]; then
  install_dir="/usr/local/bin"
else
  install_dir="${HOME}/.local/bin"
fi

mkdir -p "$install_dir"
chmod +x "${tmpdir}/${BIN}"
rm -f "${install_dir}/${BIN}"
cp "${tmpdir}/${BIN}" "${install_dir}/${BIN}"

echo "Installed ${BIN} to ${install_dir}/${BIN}" >&2

case ":${PATH}:" in
  *":${install_dir}:"*) ;;
  *)
    echo "note: ${install_dir} is not in your PATH. Add it with:" >&2
    echo "  export PATH=\"${install_dir}:\$PATH\"" >&2
    ;;
esac

"${install_dir}/${BIN}" --version
