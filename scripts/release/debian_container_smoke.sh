#!/usr/bin/env bash
set -euo pipefail

package=/tmp/pokecon.deb
app_user=pokecon-smoke
home=/home/pokecon-smoke
config="$home/.config"
data="$home/.local/share"
cache="$home/.cache"
state="$home/.local/state"

run_app() {
  runuser -u "$app_user" -- env \
    HOME="$home" \
    XDG_CONFIG_HOME="$config" \
    XDG_DATA_HOME="$data" \
    XDG_CACHE_HOME="$cache" \
    XDG_STATE_HOME="$state" \
    RUST_LOG=info \
    timeout 60 /usr/bin/pokecon --ui web --exit-after-startup
}

run_app
runuser -u "$app_user" -- mkdir -p "$config/pokecon/profiles/default"
runuser -u "$app_user" -- touch "$config/pokecon/profiles/default/package-smoke.preserved"

dpkg --install "$package"
test -f "$config/pokecon/profiles/default/package-smoke.preserved"
run_app

package_name=$(dpkg-deb --field "$package" Package)
dpkg --remove "$package_name"
test ! -e /usr/bin/pokecon
test -f "$config/pokecon/profiles/default/package-smoke.preserved"
