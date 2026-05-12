#!/usr/bin/env bash
# shellcheck disable=SC2317
#
# check-tauri-lock.sh — Verify src-tauri/Cargo.lock exists and is tracked by git.
#
# This script is used both by pre-commit hooks (via flake.nix) and CI
# (via remote-flake.yml or standalone execution). It detects if the
# Cargo.lock file for the Tauri Rust project is missing or untracked,
# which would cause remote flake evaluations to fail.
#
# Usage:
#   ./scripts/check-tauri-lock.sh
#
# Exit codes:
#   0 — all checks pass
#   1 — one or more checks failed

set -euo pipefail

# Determine repository root (works regardless of where script is called from)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

errors=0

# ── Colors for output ──────────────────────────────────────────────────────
if [[ -t 1 ]]; then
  RED='\033[0;31m'
  GREEN='\033[0;32m'
  BOLD='\033[1m'
  NC='\033[0m' # No Color
else
  RED=''
  GREEN=''
  BOLD=''
  NC=''
fi

pass() { echo -e "${GREEN}PASS${NC}  $1"; }
fail() { echo -e "${RED}FAIL${NC}  $1"; }

echo "═══ src-tauri/Cargo.lock checks ═══"
echo ""

# ── Check 1: File exists ──────────────────────────────────────────────────
LOCKFILE="src-tauri/Cargo.lock"
if [[ -f "${LOCKFILE}" ]]; then
  pass "${LOCKFILE} exists on disk."
else
  fail "${LOCKFILE} does not exist!"
  echo "       Run 'cd src-tauri && cargo generate-lockfile' to create it." >&2
  errors=$((errors + 1))
fi

# ── Check 2: File is tracked by git ────────────────────────────────────────
if git ls-files --error-unmatch "${LOCKFILE}" >/dev/null 2>&1; then
  pass "${LOCKFILE} is tracked by git."
else
  fail "${LOCKFILE} is NOT tracked by git!"
  echo "       Run 'git add ${LOCKFILE}' to track it." >&2
  errors=$((errors + 1))
fi

# ── Summary ────────────────────────────────────────────────────────────────
echo ""
if [[ ${errors} -gt 0 ]]; then
  echo -e "${RED}${BOLD}FAILED${NC} — ${errors} check(s) failed."
  exit 1
else
  echo -e "${GREEN}${BOLD}PASSED${NC} — all checks passed."
  exit 0
fi
