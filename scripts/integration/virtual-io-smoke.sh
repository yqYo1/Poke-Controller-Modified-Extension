#!/usr/bin/env bash
set -euo pipefail

if [[ $(uname -s) != Linux ]]; then
  echo "virtual-io-smoke is supported only on Linux" >&2
  exit 2
fi

if [[ $# -gt 1 ]]; then
  echo "usage: $0 [V4L2_INDEX]" >&2
  exit 2
fi

readonly v4l2_index=${1:-${POKECON_V4L2_INDEX:-42}}
if [[ ! $v4l2_index =~ ^[0-9]+$ ]] || ((v4l2_index > 255)); then
  echo "V4L2_INDEX must be an integer from 0 through 255" >&2
  exit 2
fi

privilege_wrapper=

resolve_privilege_wrapper() {
  local candidate
  if [[ -n ${POKECON_SUDO:-} ]]; then
    candidate=$POKECON_SUDO
    if [[ $candidate != /* ]] || [[ ! -x $candidate ]]; then
      echo "POKECON_SUDO must be an absolute executable path: $candidate" >&2
      return 2
    fi
    privilege_wrapper=$candidate
    return 0
  fi

  for candidate in /run/wrappers/bin/sudo /usr/bin/sudo /bin/sudo; do
    if [[ -x $candidate ]]; then
      privilege_wrapper=$candidate
      return 0
    fi
  done

  echo "no host privilege wrapper found; set POKECON_SUDO to an absolute executable path" >&2
  return 2
}

run_privileged() {
  if ((EUID == 0)); then
    "$@"
    return
  fi
  if [[ -z $privilege_wrapper ]]; then
    resolve_privilege_wrapper || return
  fi
  "$privilege_wrapper" -n "$@"
}

readonly device="/dev/video${v4l2_index}"
repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
readonly repo_root
if ! modprobe_bin=$(command -v modprobe); then
  echo "modprobe is required to prepare a virtual camera" >&2
  exit 1
fi
readonly modprobe_bin
if ! chmod_bin=$(type -P chmod); then
  echo "chmod is required to prepare a virtual camera" >&2
  exit 1
fi
readonly chmod_bin
if ! true_bin=$(type -P true); then
  echo "true is required to verify non-interactive privilege elevation" >&2
  exit 1
fi
readonly true_bin
temp_dir=$(mktemp -d -t pokecon-virtual-io.XXXXXXXX)
readonly temp_dir
readonly writer_log="$temp_dir/ffmpeg.log"
# rust/pokecon/build.rs requires POKECON_RESOURCE_PROVENANCE. The Nix gate
# sanitizes the ambient environment before invoking this script, so establish
# the development value here instead of relying on the caller.
export POKECON_RESOURCE_PROVENANCE=development
v4l2loopback_loaded_by_script=false
videodev_loaded_by_script=false
writer_pid=

cleanup() {
  local status=$? cleanup_failed=false
  trap - EXIT
  if [[ -n $writer_pid ]]; then
    kill "$writer_pid" >/dev/null 2>&1 || true
    wait "$writer_pid" >/dev/null 2>&1 || true
  fi
  # Unload only modules this script loaded, in reverse order of loading.
  # Preexisting modules or devices are never touched.
  if [[ $v4l2loopback_loaded_by_script == true ]]; then
    if ! run_privileged "$modprobe_bin" -r v4l2loopback >/dev/null 2>&1; then
      echo "error: could not unload v4l2loopback loaded by this script" >&2
      cleanup_failed=true
    fi
  fi
  if [[ $videodev_loaded_by_script == true ]]; then
    if ! run_privileged "$modprobe_bin" -r videodev >/dev/null 2>&1; then
      echo "error: could not unload videodev loaded by this script" >&2
      cleanup_failed=true
    fi
  fi
  if ((status != 0)) && [[ -s $writer_log ]]; then
    echo "virtual camera writer log:" >&2
    sed 's/^/  /' "$writer_log" >&2
  fi
  if ! rm -rf -- "$temp_dir"; then
    echo "error: could not remove temporary directory $temp_dir" >&2
    cleanup_failed=true
  fi
  if [[ $cleanup_failed == true ]]; then
    exit 1
  fi
  exit "$status"
}
trap cleanup EXIT

cd "$repo_root"

if [[ ! -e $device ]]; then
  if [[ -d /sys/module/v4l2loopback ]]; then
    echo "v4l2loopback is already loaded but $device does not exist; choose an existing loopback index" >&2
    exit 1
  fi
  if ((EUID != 0)); then
    if ! resolve_privilege_wrapper; then
      exit 2
    fi
    if ! run_privileged "$true_bin" >/dev/null 2>&1; then
      echo "non-interactive host privilege elevation is required to load the v4l2loopback kernel module" >&2
      exit 1
    fi
  fi
  # Only claim modules this script actually loaded so cleanup never unloads
  # a preexisting videodev or v4l2loopback.
  if [[ ! -d /sys/module/videodev ]]; then
    run_privileged "$modprobe_bin" videodev
    videodev_loaded_by_script=true
  fi
  run_privileged "$modprobe_bin" v4l2loopback \
    video_nr="$v4l2_index" \
    card_label=PokeCon-Virtual-Camera \
    exclusive_caps=1 \
    max_width=1920 \
    max_height=1080
  v4l2loopback_loaded_by_script=true
  # udev creates the device node asynchronously, so wait for it before
  # adjusting permissions instead of assuming modprobe was synchronous.
  for _ in {1..100}; do
    if [[ -e $device ]]; then
      break
    fi
    sleep 0.1
  done
  if [[ ! -e $device ]]; then
    echo "$device did not appear after loading v4l2loopback" >&2
    exit 1
  fi
  run_privileged "$chmod_bin" 0666 "$device"
fi

if [[ ! -r $device || ! -w $device ]]; then
  echo "$device must be readable and writable by the current user" >&2
  exit 1
fi

ffmpeg \
  -hide_banner \
  -loglevel error \
  -re \
  -f lavfi \
  -i testsrc2=size=640x360:rate=30 \
  -pix_fmt yuyv422 \
  -f v4l2 \
  "$device" \
  </dev/null >"$writer_log" 2>&1 &
writer_pid=$!

camera_ready=false
writer_failed=false
for _ in {1..100}; do
  if ! kill -0 "$writer_pid" >/dev/null 2>&1; then
    writer_failed=true
    break
  fi
  if v4l2-ctl --device="$device" --all 2>/dev/null | grep -q 'Video Capture'; then
    # v4l2-ctl reports capture caps even when the writer already exited, so
    # re-check writer liveness before claiming readiness.
    if kill -0 "$writer_pid" >/dev/null 2>&1; then
      camera_ready=true
    else
      writer_failed=true
    fi
    break
  fi
  sleep 0.1
done
if [[ $writer_failed == true ]]; then
  echo "the virtual camera writer exited before $device became capture-ready (see virtual camera writer log)" >&2
  exit 1
fi
if [[ $camera_ready != true ]]; then
  echo "the virtual camera did not become capture-ready" >&2
  exit 1
fi

# The writer must still be alive when the integration tests start; otherwise
# the V4L2 test would fail against a device with no frame source.
if ! kill -0 "$writer_pid" >/dev/null 2>&1; then
  echo "the virtual camera writer exited before the integration tests started (see virtual camera writer log)" >&2
  exit 1
fi

cargo test --locked --package pokecon --test native_serial_pty \
  --features integration-test-support
POKECON_V4L2_INDEX="$v4l2_index" \
  cargo test --locked --package pokecon --test native_v4l2 \
  --features integration-test-support -- --ignored --nocapture

echo "virtual serial PTY and V4L2 camera checks passed"
