#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

mode="${1:-generate}"
if [[ "$mode" != "generate" && "$mode" != "--check" ]]; then
  echo "usage: scripts/generate-api-types.sh [generate|--check]" >&2
  exit 2
fi

npm --prefix api ci --ignore-scripts --no-audit --no-fund

if [[ "$mode" == "--check" ]]; then
  cargo run --locked --package pokecon-server --bin generate_openapi -- --check
  generated_dir="$(mktemp -d)"
  trap 'rm -rf "$generated_dir"' EXIT
  api/node_modules/.bin/openapi-typescript api/openapi.json \
    --output "$generated_dir/generated.ts"
  if ! cmp --silent api/generated.ts "$generated_dir/generated.ts"; then
    echo "api/generated.ts differs from the OpenAPI-generated client types" >&2
    diff --unified api/generated.ts "$generated_dir/generated.ts" || true
    exit 1
  fi
else
  cargo run --locked --package pokecon-server --bin generate_openapi
  api/node_modules/.bin/openapi-typescript api/openapi.json \
    --output api/generated.ts
fi
