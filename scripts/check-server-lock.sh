#!/usr/bin/env bash
# shellcheck disable=SC2317
#
# check-server-lock.sh — Verify src-server/Cargo.lock exists and is tracked by git.
#
# This script is used both by pre-commit hooks (via flake.nix) and CI
# (via remote-flake.yml or standalone execution). It detects if the
# Cargo.lock file for the server Rust project is missing or untracked,
# which would cause remote flake evaluations to fail.
#
# Design for git-hooks.nix pre-commit compatibility:
#
# git-hooks.nix pre-commit hooks run via "bash ${self}/scripts/check-server-lock.sh"
# where ${self} resolves to a Nix store path (e.g., /nix/store/xxxx-source/).
# In this context:
#   - The working directory is set to the actual git repository root
#   - git commands work from the CWD (it's inside a git repo)
#   - BUT .git is NOT present in ${self} (Nix store path)
#   - So we must NOT cd to REPO_ROOT before running git commands
#
# The script determines REPO_ROOT from BASH_SOURCE[0], which correctly
# resolves to ${self} in the Nix store context or the actual repo root
# in manual/CI execution. We use REPO_ROOT only for file existence checks,
# NOT for git commands.
#
# Usage:
#   ./scripts/check-server-lock.sh
#
# Exit codes:
#   0 — all checks pass
#   1 — one or more checks failed

set -euo pipefail

# Save the original working directory. In pre-commit context, pre-commit
# sets CWD to the actual git repository root. In manual execution, this
# could be the repo root or a subdirectory.
ORIGINAL_DIR="$(pwd)"

# Determine the source root from the script's own location.
# When run as "bash ${self}/scripts/check-server-lock.sh", this resolves
# to ${self} (Nix store path or actual repo root).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# NOTE: We do NOT cd to REPO_ROOT here! In pre-commit context,
# REPO_ROOT is a Nix store path with no .git directory, which would
# break all git commands. Instead, we stay in ORIGINAL_DIR (which
# pre-commit sets to the actual git repo root) for git operations,
# and use REPO_ROOT only for file existence verification.

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

echo "═══ src-server/Cargo.lock checks ═══"
echo ""

# ── Check 1: File exists ──────────────────────────────────────────────────
LOCKFILE_SRC="src-server/Cargo.lock"
ABSOLUTE_LOCKFILE="${REPO_ROOT}/${LOCKFILE_SRC}"
if [[ -f "${ABSOLUTE_LOCKFILE}" ]]; then
  pass "${LOCKFILE_SRC} exists on disk."
else
  fail "${LOCKFILE_SRC} does not exist!"
  echo "       Run 'cd src-server && cargo generate-lockfile' to create it." >&2
  errors=$((errors + 1))
fi

# ── Check 2: File is tracked by git ────────────────────────────────────────
# Strategy for git-hooks.nix pre-commit compatibility:
#
#   Pre-commit runs hooks with CWD set to the actual git repo root.
#   Git commands work correctly from this CWD.
#
#   However, REPO_ROOT (= ${self}) points to the Nix store path,
#   which is NOT a git repository. So we MUST run git commands
#   from the actual git working tree, not from REPO_ROOT.
#
#   1. Find the actual git repo root using `git rev-parse --show-toplevel`
#      from ORIGINAL_DIR. If it exists, run `git ls-files` there using a
#      path relative to the repo root.
#
#   2. If ORIGINAL_DIR is not inside a git repo, fall back to
#      verifying file existence in REPO_ROOT (= ${self} in Nix store).
#      ${self} is a snapshot of all git-tracked files at build time —
#      if the file exists there, it was git-tracked when built.

if GIT_REPO_ROOT="$(git -C "${ORIGINAL_DIR}" rev-parse --show-toplevel 2>/dev/null)"; then
  # We're inside a git working tree — use git ls-files from the repo root.
  # The file path must be relative to the repo root (not ORIGINAL_DIR).
  if git -C "${GIT_REPO_ROOT}" ls-files --error-unmatch "${LOCKFILE_SRC}" >/dev/null 2>&1; then
    pass "${LOCKFILE_SRC} is tracked by git."
  else
    fail "${LOCKFILE_SRC} is NOT tracked by git!"
    echo "       Run 'git add ${LOCKFILE_SRC}' to track it." >&2
    errors=$((errors + 1))
  fi
else
  # Not inside a git repository from the original directory.
  # This occurs when the script runs outside a git working tree.
  # Fall back to checking file existence in the Nix store source root
  # (${self}), which is a snapshot of git-tracked files at build time.
  if [ -f "${ABSOLUTE_LOCKFILE}" ]; then
    pass "${LOCKFILE_SRC} is tracked by git (verified via source root)."
  else
    fail "${LOCKFILE_SRC} is NOT tracked by git!"
    echo "       Run 'git add ${LOCKFILE_SRC}' to track it." >&2
    errors=$((errors + 1))
  fi
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
