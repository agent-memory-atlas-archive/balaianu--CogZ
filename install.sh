#!/usr/bin/env bash
# CogZ install script — downloads the latest release binary from GitHub.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/balaianu/CogZ/master/install.sh | bash
#
# Or:
#   ./install.sh
#
# Pin a specific release:
#   COGZ_VERSION=v0.5.8 ./install.sh
#
# Works on Linux, macOS, and Windows (Git Bash / MSYS2).
# Windows PowerShell users can alternatively use install.ps1:
#   irm https://raw.githubusercontent.com/balaianu/CogZ/master/install.ps1 | iex
#
# Installs to ~/.local/bin/cogz (or /usr/local/bin/cogz if run as root).
# On Windows, installs to ~/.local/bin/cogz.exe.
# Creates ~/.local/share/cogz/models/ for the model cache.

set -euo pipefail

REPO="balaianu/CogZ"

# COGZ_VERSION=vX.Y.Z pins a specific release; unset installs latest.
if [ -n "${COGZ_VERSION:-}" ]; then
    RELEASE_API="https://api.github.com/repos/${REPO}/releases/tags/${COGZ_VERSION}"
else
    RELEASE_API="https://api.github.com/repos/${REPO}/releases/latest"
fi

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$@"
    else
        shasum -a 256 "$@"
    fi
}

# Detect architecture and platform.
ARCH=$(uname -m)
OS=$(uname -s | tr '[:upper:]' '[:lower:]')

# Detect Windows under Git Bash / MSYS2 / MinGW.
# uname -s returns MINGW*_NT-* or MSYS_NT-* in these environments.
# WSL returns Linux-* and gets the Linux binary (correct — it's a Linux env).
IS_WINDOWS=false
case "${OS}" in
    mingw*|msys*)
        IS_WINDOWS=true
        ;;
esac

case "${OS}-${ARCH}" in
    linux-x86_64)  ASSET="cogz-x86_64-unknown-linux-gnu" ;;
    linux-aarch64) ASSET="cogz-aarch64-unknown-linux-gnu" ;;
    darwin-arm64)  ASSET="cogz-aarch64-apple-darwin" ;;
    mingw*-x86_64|msys*-x86_64)
        ASSET="cogz-x86_64-pc-windows-msvc.exe"
        ;;
    *)
        echo "Unsupported platform: ${OS}-${ARCH}"
        echo "Supported: linux-x86_64, linux-aarch64, darwin-arm64 (Apple Silicon)"
        if [ "${IS_WINDOWS}" = "false" ]; then
            echo "Windows: use install.ps1 in PowerShell, or this script in Git Bash"
        fi
        echo "macOS Intel: not supported (use Rosetta 2 or FTS-only mode)"
        exit 1
        ;;
esac

# Determine install directory and binary name.
if [ "${IS_WINDOWS}" = "true" ]; then
    INSTALL_DIR="${HOME}/.local/bin"
    BINARY_NAME="cogz.exe"
    mkdir -p "${INSTALL_DIR}"
elif [ "$(id -u)" -eq 0 ]; then
    INSTALL_DIR="/usr/local/bin"
    BINARY_NAME="cogz"
else
    INSTALL_DIR="${HOME}/.local/bin"
    BINARY_NAME="cogz"
    mkdir -p "${INSTALL_DIR}"
fi

echo "Installing CogZ for ${OS}-${ARCH}..."

# Fetch the release JSON once — both asset URLs come from it.
echo "Fetching release metadata..."
RELEASE_JSON=$(curl -fsSL "${RELEASE_API}")
DOWNLOAD_URL=$(printf '%s' "${RELEASE_JSON}" | grep -o '"browser_download_url": "[^"]*'"${ASSET}"'"' | head -1 | sed 's/"browser_download_url": "//;s/"$//')

if [ -z "${DOWNLOAD_URL}" ]; then
    echo "Error: could not find ${ASSET} in the release."
    echo "Check https://github.com/${REPO}/releases for available assets."
    exit 1
fi

# Download to a temporary file. Trap covers every exit path below.
TMP_FILE=$(mktemp /tmp/cogz-download-XXXXXX)
TMP_SUMS=""
trap 'rm -f "${TMP_FILE}" ${TMP_SUMS:+"${TMP_SUMS}"}' EXIT
echo "Downloading ${ASSET}..."
curl -fsSL -o "${TMP_FILE}" "${DOWNLOAD_URL}"

# Verify checksum from SHA256SUMS — missing checksums fail closed.
CHECKSUM_URL=$(printf '%s' "${RELEASE_JSON}" | grep -o '"browser_download_url": "[^"]*SHA256SUMS"' | head -1 | sed 's/"browser_download_url": "//;s/"$//')
if [ -z "${CHECKSUM_URL}" ]; then
    echo "Error: SHA256SUMS not found in the release."
    echo "Refusing to install an unverified binary."
    exit 1
fi
TMP_SUMS=$(mktemp /tmp/cogz-sums-XXXXXX)
curl -fsSL -o "${TMP_SUMS}" "${CHECKSUM_URL}"
EXPECTED_HASH=$(grep "  ${ASSET}$" "${TMP_SUMS}" | awk '{print $1}')
if [ -z "${EXPECTED_HASH}" ]; then
    echo "Error: SHA256SUMS downloaded but no entry for ${ASSET}."
    echo "Refusing to install unverified binary."
    exit 1
fi
ACTUAL_HASH=$(sha256 "${TMP_FILE}" | awk '{print $1}')
if [ "${EXPECTED_HASH}" != "${ACTUAL_HASH}" ]; then
    echo "Error: checksum mismatch."
    echo "  Expected: ${EXPECTED_HASH}"
    echo "  Actual:   ${ACTUAL_HASH}"
    exit 1
fi
echo "Checksum verified."

# Make executable (no-op on Windows, harmless under Git Bash).
chmod +x "${TMP_FILE}" 2>/dev/null || true

# Install.
DEST_PATH="${INSTALL_DIR}/${BINARY_NAME}"
mv "${TMP_FILE}" "${DEST_PATH}"
echo "Installed to ${DEST_PATH}"

# Create model cache directory.
MODEL_DIR="${HOME}/.local/share/cogz/models"
mkdir -p "${MODEL_DIR}"
echo "Model cache: ${MODEL_DIR}"

# Check PATH.
case ":${PATH}:" in
    *":${INSTALL_DIR}:"*)
        ;;
    *)
        echo ""
        echo "WARNING: ${INSTALL_DIR} is not on your PATH."
        if [ "${IS_WINDOWS}" = "true" ]; then
            echo "Add ${INSTALL_DIR} to your PATH in Windows Settings,"
            echo "or add this to your shell profile:"
            echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
        else
            echo "Add this line to your shell config (~/.bashrc, ~/.zshrc, etc.):"
            echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
        fi
        ;;
esac

# Verify.
echo ""
echo "Verification:"
"${DEST_PATH}" --version

echo ""
echo "Next steps:"
echo "  cd ~/your-project"
echo "  cogz init"
echo "  cogz index"
echo ""
echo "To enable agent hooks (Devin/Claude Code):"
echo "  See docs/integration/hooks.md"
echo "  Add cogz capture-event entries to your agent's hook config"
echo ""
echo "Done."
