#!/usr/bin/env bash

set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

workspace_manifest="Cargo.toml"
workspace_lock="Cargo.lock"

if [[ ! -f "$workspace_manifest" ]]; then
  echo "error: $workspace_manifest is missing" >&2
  exit 1
fi

if [[ ! -f "$workspace_lock" ]]; then
  echo "error: $workspace_lock is required for the root Rust workspace" >&2
  exit 1
fi

if ! git ls-files --error-unmatch -- "$workspace_lock" >/dev/null 2>&1; then
  echo "error: $workspace_lock exists but is not tracked by git" >&2
  exit 1
fi

cargo metadata --locked --no-deps --format-version 1 >/dev/null
echo "$workspace_lock is tracked and synchronized with the root workspace"
