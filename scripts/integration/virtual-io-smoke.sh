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
module_loaded_by_script=false
writer_pid=

cleanup() {
  local status=$?
  trap - EXIT
  if [[ -n $writer_pid ]]; then
    kill "$writer_pid" >/dev/null 2>&1 || true
    wait "$writer_pid" >/dev/null 2>&1 || true
  fi
  if [[ $module_loaded_by_script == true ]]; then
    run_privileged "$modprobe_bin" -r v4l2loopback >/dev/null 2>&1 || \
      echo "warning: could not unload v4l2loopback" >&2
  fi
  if ((status != 0)) && [[ -s $writer_log ]]; then
    echo "virtual camera writer log:" >&2
    sed 's/^/  /' "$writer_log" >&2
  fi
  rm -rf -- "$temp_dir"
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
  run_privileged "$modprobe_bin" videodev
  run_privileged "$modprobe_bin" v4l2loopback \
    video_nr="$v4l2_index" \
    card_label=PokeCon-Virtual-Camera \
    exclusive_caps=1 \
    max_width=1920 \
    max_height=1080
  module_loaded_by_script=true
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
for _ in {1..100}; do
  if ! kill -0 "$writer_pid" >/dev/null 2>&1; then
    break
  fi
  if v4l2-ctl --device="$device" --all 2>/dev/null | grep -q 'Video Capture'; then
    camera_ready=true
    break
  fi
  sleep 0.1
done
if [[ $camera_ready != true ]]; then
  echo "the virtual camera did not become capture-ready" >&2
  exit 1
fi

cargo test --locked --package pokecon-device --test native_serial_pty
POKECON_V4L2_INDEX="$v4l2_index" \
  cargo test --locked --package pokecon-camera --test native_v4l2 -- --ignored --nocapture

echo "virtual serial PTY and V4L2 camera checks passed"
