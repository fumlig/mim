#!/usr/bin/env bash
# Download the Silero VAD ONNX model into ./models/.
#
# Idempotent: if the file already exists with the expected SHA-256, this
# script does nothing. Otherwise it (re-)downloads and verifies.
#
# Usage:
#   scripts/download-silero-vad.sh
#
# The resulting path (printed on success) can be passed to mim via
#   --vad-model <path>
# or the MIM_VAD_MODEL environment variable.

set -euo pipefail

VERSION="v6.2.1"
SHA256="1a153a22f4509e292a94e67d6f9b85e8deb25b4988682b7e174c65279d8788e3"
URL="https://github.com/snakers4/silero-vad/raw/${VERSION}/src/silero_vad/data/silero_vad.onnx"

# Resolve repo root (parent of this script's directory).
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
DEST_DIR="${REPO_ROOT}/models"
DEST="${DEST_DIR}/silero_vad.onnx"

verify() {
    # Echoes "ok" if $1 exists and matches $SHA256, empty otherwise.
    [ -f "$1" ] || return 0
    if command -v sha256sum >/dev/null 2>&1; then
        echo "$SHA256  $1" | sha256sum --check --status && echo ok
    elif command -v shasum >/dev/null 2>&1; then
        echo "$SHA256  $1" | shasum -a 256 --check --status && echo ok
    else
        echo "error: neither sha256sum nor shasum found" >&2
        exit 1
    fi
}

mkdir -p "${DEST_DIR}"

if [ "$(verify "${DEST}")" = "ok" ]; then
    echo "silero_vad.onnx already present and verified: ${DEST}"
    exit 0
fi

if [ -f "${DEST}" ]; then
    echo "existing ${DEST} failed checksum, re-downloading"
    rm -f "${DEST}"
fi

echo "downloading silero_vad.onnx ${VERSION}"
echo "  from ${URL}"
echo "  to   ${DEST}"

TMP="${DEST}.tmp"
trap 'rm -f "${TMP}"' EXIT

if command -v curl >/dev/null 2>&1; then
    curl --fail --location --progress-bar --output "${TMP}" "${URL}"
elif command -v wget >/dev/null 2>&1; then
    wget --quiet --show-progress --output-document="${TMP}" "${URL}"
else
    echo "error: need curl or wget to download the model" >&2
    exit 1
fi

if [ "$(verify "${TMP}")" != "ok" ]; then
    echo "error: downloaded file failed SHA-256 check" >&2
    echo "       expected ${SHA256}" >&2
    exit 1
fi

mv "${TMP}" "${DEST}"
trap - EXIT

echo "ok: ${DEST}"
