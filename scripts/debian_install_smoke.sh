#!/usr/bin/env bash
set -euo pipefail

readonly ubuntu_image='ubuntu@sha256:52df9b1ee71626e0088f7d400d5c6b5f7bb916f8f0c82b474289a4ece6cf3faf'
readonly docker=${POKECON_DOCKER:?POKECON_DOCKER must identify the Nix-provided Docker client}

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
context=$(mktemp -d -t pokecon-debian-install.XXXXXXXX)
package_sha=$(sha256sum "$package" | cut -d ' ' -f 1)
image="pokecon-package-smoke:${package_sha:0:20}-$$"

cleanup() {
  "$docker" image rm "$image" >/dev/null 2>&1 || true
  rm -rf -- "$context"
}
trap cleanup EXIT

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
    && apt-get install --yes --no-install-recommends /tmp/pokecon.deb \
    && useradd --create-home --uid 10001 pokecon-smoke \
    && chmod 0755 /usr/local/bin/pokecon-package-smoke \
    && rm -rf /var/lib/apt/lists/*
ENTRYPOINT ["/usr/local/bin/pokecon-package-smoke"]
EOF

"$docker" run \
  --rm \
  --network none \
  --platform linux/amd64 \
  "$image"
