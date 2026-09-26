#!/usr/bin/env bash
set -euo pipefail

readonly ubuntu_image='ubuntu@sha256:52df9b1ee71626e0088f7d400d5c6b5f7bb916f8f0c82b474289a4ece6cf3faf'
readonly docker=${POKECON_DOCKER:?POKECON_DOCKER must identify the Nix-provided Docker client}
readonly docker_runtime_probe_repetitions=2
readonly docker_runtime_web_hard_seconds=65
readonly docker_runtime_desktop_hard_seconds=120
readonly docker_runtime_margin_seconds=30
readonly docker_runtime_outer_term_seconds=420
readonly docker_runtime_outer_kill_grace_seconds=30
readonly docker_cleanup_operation_timeout_seconds=10
readonly docker_cleanup_operation_kill_grace_seconds=2
readonly docker_runtime_probe_budget_seconds=$((
  docker_runtime_probe_repetitions * docker_runtime_web_hard_seconds
  + docker_runtime_probe_repetitions * docker_runtime_desktop_hard_seconds
))
readonly docker_runtime_required_seconds=$((
  docker_runtime_probe_budget_seconds + docker_runtime_margin_seconds
))
if ((docker_runtime_required_seconds >= docker_runtime_outer_term_seconds)); then
  echo "Docker runtime budget must complete before outer TERM" >&2
  exit 2
fi

run_bounded_docker_cleanup() {
  timeout \
    --signal=TERM \
    --kill-after="${docker_cleanup_operation_kill_grace_seconds}s" \
    "${docker_cleanup_operation_timeout_seconds}s" \
    "$docker" "$@"
}

if [[ $# -ne 1 ]]; then
  echo "usage: $0 PACKAGE.deb" >&2
  exit 2
fi

package=$(realpath "$1")
if [[ ! -f $package || $package != *.deb ]]; then
  echo "Debian package is missing or has the wrong extension: $package" >&2
  exit 2
fi

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
context=$(mktemp -d -t pokecon-debian-install.XXXXXXXXXXXXXXXX)
container_id_file="$context/container.id"
context_basename=${context##*/}
random_suffix=${context_basename#pokecon-debian-install.}
random_suffix=${random_suffix,,}
image=

cleanup() {
  local container_id=
  local inspected_id=
  local -a container_id_lines=()
  if [[ -f $container_id_file && ! -L $container_id_file ]] \
    && mapfile -t container_id_lines <"$container_id_file" \
    && [[ ${#container_id_lines[@]} -eq 1 ]]; then
    container_id=${container_id_lines[0]}
    if [[ $container_id =~ ^[0-9a-f]{64}$ ]]; then
      inspected_id=$(
        run_bounded_docker_cleanup \
          container inspect --format '{{.Id}}' "$container_id" \
          2>/dev/null
      ) || inspected_id=
      if [[ $inspected_id == "$container_id" ]]; then
        run_bounded_docker_cleanup \
          container rm --force "$container_id" \
          >/dev/null 2>&1 || true
      fi
    fi
  fi
  if [[ $image =~ ^pokecon-package-smoke:[0-9a-f]{20}-[a-z0-9]{16}$ ]]; then
    run_bounded_docker_cleanup image rm "$image" >/dev/null 2>&1 || true
  fi
  rm -rf -- "$context"
}
trap cleanup EXIT

if [[ ! $random_suffix =~ ^[a-z0-9]{16}$ ]]; then
  echo "invalid random suffix derived from Docker build context" >&2
  exit 2
fi
package_sha=$(sha256sum "$package" | cut -d ' ' -f 1)
container_name="pokecon-package-smoke-${package_sha:0:20}-${random_suffix}"
image="pokecon-package-smoke:${package_sha:0:20}-${random_suffix}"
if [[ ! $package_sha =~ ^[0-9a-f]{64}$ \
  || ! $container_name =~ ^pokecon-package-smoke-[0-9a-f]{20}-[a-z0-9]{16}$ \
  || ! $image =~ ^pokecon-package-smoke:[0-9a-f]{20}-[a-z0-9]{16}$ ]]; then
  echo "invalid Docker runtime identity derived from package and context" >&2
  exit 2
fi

cp -- "$package" "$context/pokecon.deb"
cp -- "$script_dir/debian_container_smoke.sh" "$context/smoke.sh"

"$docker" build \
  --no-cache \
  --platform linux/amd64 \
  --progress plain \
  --tag "$image" \
  --file - \
  "$context" <<EOF
FROM $ubuntu_image
ARG DEBIAN_FRONTEND=noninteractive
COPY pokecon.deb /tmp/pokecon.deb
COPY smoke.sh /usr/local/bin/pokecon-package-smoke
RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
      'dbus-daemon' \
      'libgl1-mesa-dri' \
      'x11-utils' \
      'xdotool' \
      'xvfb' \
      /tmp/pokecon.deb \
    && useradd --create-home --uid 10001 pokecon-smoke \
    && chmod 0755 /usr/local/bin/pokecon-package-smoke \
    && rm -rf /var/lib/apt/lists/*
ENTRYPOINT ["/usr/local/bin/pokecon-package-smoke"]
EOF

timeout \
  --signal=TERM \
  --kill-after="${docker_runtime_outer_kill_grace_seconds}s" \
  "${docker_runtime_outer_term_seconds}s" \
  "$docker" run \
  --rm \
  --name "$container_name" \
  --cidfile "$container_id_file" \
  --network none \
  --platform linux/amd64 \
  "$image"
