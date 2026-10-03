#!/usr/bin/env bash
set -euo pipefail

usage() {
  printf '%s\n' \
    'usage: remote-flake-smoke --repository OWNER/REPOSITORY --revision FULL_COMMIT_SHA' >&2
}

repository=
revision=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --repository)
      [ "$#" -ge 2 ] || { usage; exit 2; }
      repository=$2
      shift 2
      ;;
    --revision)
      [ "$#" -ge 2 ] || { usage; exit 2; }
      revision=$2
      shift 2
      ;;
    --help)
      usage
      exit 0
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

owner=${repository%%/*}
name=${repository#*/}
if [ -z "$owner" ] || [ -z "$name" ] || [ "$owner/$name" != "$repository" ]; then
  printf 'repository must be exactly OWNER/REPOSITORY: %s\n' "$repository" >&2
  exit 2
fi
case "$owner" in
  *[!A-Za-z0-9_.-]*)
    printf 'repository owner contains an invalid character: %s\n' "$owner" >&2
    exit 2
    ;;
esac
case "$name" in
  *[!A-Za-z0-9_.-]*)
    printf 'repository name contains an invalid character: %s\n' "$name" >&2
    exit 2
    ;;
esac
if [ "${#revision}" -ne 40 ]; then
  printf 'revision must be one full 40-character commit SHA: %s\n' "$revision" >&2
  exit 2
fi
case "$revision" in
  *[!0-9a-f]*)
    printf 'revision must be a lowercase hexadecimal commit SHA: %s\n' "$revision" >&2
    exit 2
    ;;
esac

flake_ref="github:$repository/$revision"
test_root="$(mktemp -d "$HOME/pokecon-remote-flake-smoke.XXXXXX")"
app_log="$test_root/pokecon.log"
index_html="$test_root/index.html"
settings_json="$test_root/settings.json"
asset_urls="$test_root/asset-urls.txt"
unique_asset_urls="$test_root/asset-urls.unique.txt"

mkdir -p \
  "$test_root/home" \
  "$test_root/config" \
  "$test_root/data" \
  "$test_root/cache" \
  "$test_root/state" \
  "$test_root/runtime"
chmod 700 "$test_root/home" "$test_root/config" "$test_root/data" "$test_root/cache" "$test_root/state" "$test_root/runtime"

# Do not let an ambient PokeCon setting override the isolated smoke environment.
while IFS= read -r -d '' environment_entry; do
  environment_name=${environment_entry%%=*}
  case "$environment_name" in
    POKE_CON_* | POKECON_*) unset "$environment_name" ;;
  esac
done < <(env -0)

app_environment=(
  "HOME=$test_root/home"
  "XDG_CONFIG_HOME=$test_root/config"
  "XDG_DATA_HOME=$test_root/data"
  "XDG_CACHE_HOME=$test_root/cache"
  "XDG_STATE_HOME=$test_root/state"
  "XDG_RUNTIME_DIR=$test_root/runtime"
  'POKECON_BIND_ADDRESS=127.0.0.1'
)

port=
port_base=$((20000 + (BASHPID % 30000)))
for port_offset in $(seq 0 100); do
  candidate_port=$((port_base + port_offset))
  if [ "$candidate_port" -gt 60000 ]; then
    candidate_port=$((20000 + port_offset))
  fi
  if curl --silent --output /dev/null --connect-timeout 1 --max-time 2 \
    "http://127.0.0.1:$candidate_port/"; then
    continue
  fi
  port=$candidate_port
  break
done
[ -n "$port" ] || {
  printf 'could not find an unused local TCP port for the smoke server\n' >&2
  exit 1
}
app_environment+=("POKECON_PORT=$port")
nix_network_options=(
  --option download-attempts 10
)

printf 'remote_flake=%s\n' "$flake_ref"
printf 'test_root=%s\n' "$test_root"

cd "$test_root"
env "${app_environment[@]}" nix build \
  "${nix_network_options[@]}" "$flake_ref#pokecon"
[ -x result/bin/pokecon ] || {
  printf 'remote pokecon build has no executable result/bin/pokecon\n' >&2
  exit 1
}

env "${app_environment[@]}" nix run \
  "${nix_network_options[@]}" "$flake_ref" -- --help > "$test_root/help.txt"
[ -s "$test_root/help.txt" ] || {
  printf 'remote pokecon --help returned an empty response\n' >&2
  exit 1
}
if ! grep -q '^Usage: pokecon' "$test_root/help.txt"; then
  printf 'remote pokecon --help response has no usage line\n' >&2
  exit 1
fi

env "${app_environment[@]}" timeout --signal=TERM --kill-after=5s 90s \
  nix run "${nix_network_options[@]}" "$flake_ref" -- \
  --ui web \
  --port "$port" \
  --bind-address 127.0.0.1 > "$app_log" 2>&1 &
app_pid=$!

stop_app() {
  if kill -0 "$app_pid" 2>/dev/null; then
    kill -TERM "$app_pid"
  fi
  wait "$app_pid" 2>/dev/null || true
}
trap stop_app EXIT

base_url="http://127.0.0.1:$port"
ready=false
for attempt in $(seq 1 60); do
  if http_status="$(curl --silent --show-error --output "$index_html" --write-out '%{http_code}' --connect-timeout 2 --max-time 5 "$base_url/")"; then
    if [ "$http_status" = 200 ] && [ -s "$index_html" ]; then
      ready=true
      printf 'web_ready_attempt=%s\n' "$attempt"
      break
    fi
  fi
  if ! kill -0 "$app_pid" 2>/dev/null; then
    break
  fi
  sleep 1
done
if [ "$ready" != true ]; then
  printf 'web frontend did not become ready; log=%s\n' "$app_log" >&2
  exit 1
fi

if settings_status="$(curl --silent --show-error --output "$settings_json" --write-out '%{http_code}' --connect-timeout 2 --max-time 5 "$base_url/api/settings")"; then
  [ "$settings_status" = 200 ] || {
    printf 'settings API returned HTTP %s\n' "$settings_status" >&2
    exit 1
  }
else
  printf 'settings API request failed\n' >&2
  exit 1
fi
[ -s "$settings_json" ] || {
  printf 'settings API returned an empty response\n' >&2
  exit 1
}

if ! grep -oE '/_app/[^"[:space:]]+\.(js|css)' "$index_html" > "$asset_urls"; then
  printf 'index.html contains no SvelteKit JS/CSS asset references\n' >&2
  exit 1
fi
sort -u "$asset_urls" > "$unique_asset_urls"
asset_count=0
while IFS= read -r asset_url; do
  [ -n "$asset_url" ] || continue
  asset_count=$((asset_count + 1))
  if asset_status="$(curl --silent --show-error --output /dev/null --write-out '%{http_code}' --connect-timeout 2 --max-time 5 "$base_url$asset_url")"; then
    [ "$asset_status" = 200 ] || {
      printf 'frontend asset returned HTTP %s: %s\n' "$asset_status" "$asset_url" >&2
      exit 1
    }
  else
    printf 'frontend asset request failed: %s\n' "$asset_url" >&2
    exit 1
  fi
done < "$unique_asset_urls"
[ "$asset_count" -gt 0 ] || {
  printf 'index.html contains no unique SvelteKit assets\n' >&2
  exit 1
}

printf 'build_result=%s\n' "$(nix path-info ./result)"
printf 'web_http_status=200\nsettings_http_status=200\nfrontend_asset_count=%s\n' "$asset_count"
