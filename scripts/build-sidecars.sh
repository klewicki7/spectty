#!/usr/bin/env bash
# build-sidecars.sh — build spectty sidecar binaries and place them in
# src-tauri/binaries/ with the Tauri-expected target-triple suffix.
#
# Usage (run from the repository root):
#   scripts/build-sidecars.sh
#
# This script is invoked by `pnpm tauri build` (and `pnpm tauri build --debug`)
# via the `beforeBuildCommand` in tauri.conf.json — it runs BEFORE the Tauri
# CLI invokes `cargo build`, so the binaries are present when tauri-build
# validates the `externalBin` entries.
#
# Why this script exists:
#   Tauri v2 bundles sidecars by reading `bundle.externalBin` from tauri.conf.json
#   at compile time (inside src-tauri/build.rs via tauri-build). The binary files
#   MUST exist with the target-triple suffix BEFORE cargo runs. Plain
#   `cargo build --workspace` must stay green with no pre-steps, so externalBin
#   lives only in the --config overlay (src-tauri/tauri.bundle.conf.json), which is
#   merged via TAURI_CONFIG only when the tauri CLI is involved. This script handles
#   the build-time population of src-tauri/binaries/.
#
# shellcheck shell=bash
set -euo pipefail

# ---------------------------------------------------------------------------
# 1. Detect host triple
# ---------------------------------------------------------------------------
HOST_TRIPLE="$(rustc -vV | grep '^host:' | awk '{print $2}')"
if [ -z "${HOST_TRIPLE}" ]; then
  echo "error: could not determine host triple from 'rustc -vV'" >&2
  exit 1
fi
echo "Host triple: ${HOST_TRIPLE}"

# ---------------------------------------------------------------------------
# 2. Resolve repository root (the script's parent's parent)
# ---------------------------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
BINARIES_DIR="${REPO_ROOT}/src-tauri/binaries"

# ---------------------------------------------------------------------------
# 3. Build both sidecar crates in release mode
# ---------------------------------------------------------------------------
echo "Building spectty-hook and spectty-mcp (release)..."
cargo build --release -p spectty-hook -p spectty-mcp --manifest-path "${REPO_ROOT}/Cargo.toml"

# ---------------------------------------------------------------------------
# 4. Copy binaries to src-tauri/binaries/ with target-triple suffix
# ---------------------------------------------------------------------------
mkdir -p "${BINARIES_DIR}"

HOOK_SRC="${REPO_ROOT}/target/release/spectty-hook"
MCP_SRC="${REPO_ROOT}/target/release/spectty-mcp"

if [ ! -f "${HOOK_SRC}" ]; then
  echo "error: built binary not found: ${HOOK_SRC}" >&2
  exit 1
fi
if [ ! -f "${MCP_SRC}" ]; then
  echo "error: built binary not found: ${MCP_SRC}" >&2
  exit 1
fi

cp "${HOOK_SRC}" "${BINARIES_DIR}/spectty-hook-${HOST_TRIPLE}"
cp "${MCP_SRC}"  "${BINARIES_DIR}/spectty-mcp-${HOST_TRIPLE}"

echo "Sidecars placed in ${BINARIES_DIR}:"
ls -lh "${BINARIES_DIR}/spectty-hook-${HOST_TRIPLE}" "${BINARIES_DIR}/spectty-mcp-${HOST_TRIPLE}"
