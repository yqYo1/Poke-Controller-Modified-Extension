#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

mode="${1:-generate}"
if [[ "$mode" != "generate" && "$mode" != "--check" ]]; then
  echo "usage: scripts/quality/generate-api-types.sh [generate|--check]" >&2
  exit 2
fi

api_node_modules=${POKECON_API_NODE_MODULES:-}
if [[ $api_node_modules != /* || ! -d $api_node_modules ]]; then
  echo "POKECON_API_NODE_MODULES must identify the absolute Nix-provided API dependency directory" >&2
  echo "run this generator through: nix run .#generate-api-types" >&2
  exit 2
fi
openapi_typescript="$api_node_modules/.bin/openapi-typescript"
if [[ ! -f $openapi_typescript ]]; then
  echo "openapi-typescript is missing from the Nix-provided API dependencies: $openapi_typescript" >&2
  exit 2
fi
web_generated="web/src/lib/api/openapi.ts"
web_schema="web/src/lib/api/openapi.json"

if [[ "$mode" == "--check" ]]; then
  cargo run --locked --package pokecon-server --bin generate_openapi -- --check
  generated_dir="$(mktemp -d)"
  trap 'rm -rf "$generated_dir"' EXIT
  bun --bun "$openapi_typescript" api/openapi.json \
    --output "$generated_dir/generated.ts"
  if ! cmp --silent "$web_generated" "$generated_dir/generated.ts"; then
    echo "$web_generated differs from the OpenAPI-generated client types" >&2
    diff --unified "$web_generated" "$generated_dir/generated.ts" || true
    exit 1
  fi
  if ! cmp --silent "$web_schema" api/openapi.json; then
    echo "$web_schema differs from the generated OpenAPI schema" >&2
    diff --unified "$web_schema" api/openapi.json || true
    exit 1
  fi
else
  cargo run --locked --package pokecon-server --bin generate_openapi
  mkdir -p "$(dirname "$web_generated")"
  bun --bun "$openapi_typescript" api/openapi.json \
    --output "$web_generated"
  install -D -m 0644 api/openapi.json "$web_schema"
fi
