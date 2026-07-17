#!/usr/bin/env bash

set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

server_manifest="src-server/Cargo.toml"
server_lock="src-server/Cargo.lock"

if [[ ! -f "$server_manifest" ]]; then
  echo "not applicable: obsolete src-server crate is absent; Phase 2 replaces this guard"
  exit 0
fi

if [[ ! -f "$server_lock" ]]; then
  echo "error: $server_lock is required when $server_manifest exists" >&2
  exit 1
fi

if ! git ls-files --error-unmatch -- "$server_lock" >/dev/null 2>&1; then
  echo "error: $server_lock exists but is not tracked by git" >&2
  exit 1
fi

echo "$server_lock exists and is tracked"
