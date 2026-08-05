#!/usr/bin/env bash
set -euo pipefail

parse_process_stat() {
  if [ "$#" -ne 5 ]; then
    return 2
  fi
  local stat_pid="$1"
  local stat_line="$2"
  local -n state_output="$3"
  local -n pgid_output="$4"
  local -n start_ticks_output="$5"
  local stat_tail
  local candidate_state
  local candidate_pgid
  local candidate_start_ticks
  local -a stat_fields=()
  state_output=
  pgid_output=
  start_ticks_output=

  if [[ ! "$stat_pid" =~ ^[0-9]+$ ]] || [ "$stat_pid" -le 1 ]; then
    return 1
  fi
  case "$stat_line" in
    "$stat_pid ("*) ;;
    *) return 1 ;;
  esac
  stat_tail="${stat_line##*) }"
  if [ "$stat_tail" = "$stat_line" ]; then
    return 1
  fi
  read -r -a stat_fields <<<"$stat_tail"
  if [ "${#stat_fields[@]}" -lt 20 ]; then
    return 1
  fi
  candidate_state="${stat_fields[0]}"
  candidate_pgid="${stat_fields[2]}"
  candidate_start_ticks="${stat_fields[19]}"
  if [[ ! "$candidate_state" =~ ^[A-Za-z]$ ]]; then
    return 1
  fi
  case "$candidate_state" in
    X | x | Z) return 1 ;;
  esac
  if [[ ! "$candidate_pgid" =~ ^[0-9]+$ ]] \
    || [ "$candidate_pgid" -le 1 ] \
    || [[ ! "$candidate_start_ticks" =~ ^[0-9]+$ ]] \
    || [ "$candidate_start_ticks" -le 0 ]; then
    return 1
  fi
  state_output="$candidate_state"
  pgid_output="$candidate_pgid"
  start_ticks_output="$candidate_start_ticks"
  [ -n "$state_output" ]
}

read_process_identity() {
  if [ "$#" -ne 6 ]; then
    return 2
  fi
  local requested_pid="$1"
  local -n pid_output="$2"
  local -n state_output="$3"
  local -n pgid_output="$4"
  local -n start_ticks_output="$5"
  local -n executable_output="$6"
  local first_stat_line
  local first_state
  local first_pgid
  local first_start_ticks
  local first_executable
  local first_canonical_executable
  local second_stat_line
  local second_state
  local second_pgid
  local second_start_ticks
  local second_executable
  local second_canonical_executable
  pid_output=
  state_output=
  pgid_output=
  start_ticks_output=
  executable_output=

  if [[ ! "$requested_pid" =~ ^[0-9]+$ ]] || [ "$requested_pid" -le 1 ]; then
    return 1
  fi
  IFS= read -r first_stat_line 2>/dev/null <"/proc/$requested_pid/stat" \
    || return 1
  parse_process_stat \
    "$requested_pid" "$first_stat_line" \
    first_state first_pgid first_start_ticks \
    || return 1
  first_executable="$(readlink -- "/proc/$requested_pid/exe" 2>/dev/null)" \
    || return 1
  first_canonical_executable="$(
    readlink -f -- "$first_executable" 2>/dev/null
  )" || return 1
  if [ -z "$first_state" ] \
    || [ -z "$first_executable" ] \
    || [ "$first_executable" != "$first_canonical_executable" ] \
    || [ ! -f "$first_canonical_executable" ]; then
    return 1
  fi

  IFS= read -r second_stat_line 2>/dev/null <"/proc/$requested_pid/stat" \
    || return 1
  parse_process_stat \
    "$requested_pid" "$second_stat_line" \
    second_state second_pgid second_start_ticks \
    || return 1
  second_executable="$(readlink -- "/proc/$requested_pid/exe" 2>/dev/null)" \
    || return 1
  second_canonical_executable="$(
    readlink -f -- "$second_executable" 2>/dev/null
  )" || return 1
  if [ -z "$second_executable" ] \
    || [ "$second_executable" != "$second_canonical_executable" ] \
    || [ ! -f "$second_canonical_executable" ] \
    || [ "$second_pgid" != "$first_pgid" ] \
    || [ "$second_start_ticks" != "$first_start_ticks" ] \
    || [ "$second_executable" != "$first_executable" ]; then
    return 1
  fi

  pid_output="$requested_pid"
  state_output="$second_state"
  pgid_output="$second_pgid"
  start_ticks_output="$second_start_ticks"
  executable_output="$second_executable"
  [ -n "$state_output" ]
}

read_process_parent_identity() {
  if [ "$#" -ne 6 ]; then
    return 2
  fi
  local requested_pid="$1"
  local -n pid_output="$2"
  local -n ppid_output="$3"
  local -n pgid_output="$4"
  local -n start_ticks_output="$5"
  local -n executable_output="$6"
  local parent_identity_snapshot_one_pid
  local parent_identity_snapshot_one_state
  local parent_identity_snapshot_one_pgid
  local parent_identity_snapshot_one_start_ticks
  local parent_identity_snapshot_one_executable
  local parent_identity_snapshot_one_ppid=
  local parent_identity_snapshot_one_ppid_count=0
  local parent_identity_snapshot_two_pid
  local parent_identity_snapshot_two_state
  local parent_identity_snapshot_two_pgid
  local parent_identity_snapshot_two_start_ticks
  local parent_identity_snapshot_two_executable
  local parent_identity_snapshot_two_ppid=
  local parent_identity_snapshot_two_ppid_count=0
  local status_line
  pid_output=
  ppid_output=
  pgid_output=
  start_ticks_output=
  executable_output=

  read_process_identity \
    "$requested_pid" \
    parent_identity_snapshot_one_pid parent_identity_snapshot_one_state \
    parent_identity_snapshot_one_pgid parent_identity_snapshot_one_start_ticks \
    parent_identity_snapshot_one_executable \
    || return 1
  while IFS= read -r status_line; do
    case "$status_line" in
      PPid:*)
        ((parent_identity_snapshot_one_ppid_count += 1))
        if [[ "$status_line" =~ ^PPid:[[:space:]]+([0-9]+)[[:space:]]*$ ]]; then
          parent_identity_snapshot_one_ppid="${BASH_REMATCH[1]}"
        else
          return 1
        fi
        ;;
    esac
  done 2>/dev/null <"/proc/$requested_pid/status" || return 1
  if [ "$parent_identity_snapshot_one_ppid_count" -ne 1 ] \
    || [ -z "$parent_identity_snapshot_one_state" ] \
    || [[ ! "$parent_identity_snapshot_one_ppid" =~ ^[0-9]+$ ]] \
    || [ "$parent_identity_snapshot_one_ppid" -le 1 ]; then
    return 1
  fi

  read_process_identity \
    "$requested_pid" \
    parent_identity_snapshot_two_pid parent_identity_snapshot_two_state \
    parent_identity_snapshot_two_pgid parent_identity_snapshot_two_start_ticks \
    parent_identity_snapshot_two_executable \
    || return 1
  while IFS= read -r status_line; do
    case "$status_line" in
      PPid:*)
        ((parent_identity_snapshot_two_ppid_count += 1))
        if [[ "$status_line" =~ ^PPid:[[:space:]]+([0-9]+)[[:space:]]*$ ]]; then
          parent_identity_snapshot_two_ppid="${BASH_REMATCH[1]}"
        else
          return 1
        fi
        ;;
    esac
  done 2>/dev/null <"/proc/$requested_pid/status" || return 1
  if [ "$parent_identity_snapshot_two_ppid_count" -ne 1 ] \
    || [ -z "$parent_identity_snapshot_two_state" ] \
    || [[ ! "$parent_identity_snapshot_two_ppid" =~ ^[0-9]+$ ]] \
    || [ "$parent_identity_snapshot_two_ppid" -le 1 ] \
    || [ "$parent_identity_snapshot_two_pid" != "$parent_identity_snapshot_one_pid" ] \
    || [ "$parent_identity_snapshot_two_ppid" != "$parent_identity_snapshot_one_ppid" ] \
    || [ "$parent_identity_snapshot_two_pgid" != "$parent_identity_snapshot_one_pgid" ] \
    || [ "$parent_identity_snapshot_two_start_ticks" != "$parent_identity_snapshot_one_start_ticks" ] \
    || [ "$parent_identity_snapshot_two_executable" != "$parent_identity_snapshot_one_executable" ]; then
    return 1
  fi

  pid_output="$parent_identity_snapshot_two_pid"
  ppid_output="$parent_identity_snapshot_two_ppid"
  pgid_output="$parent_identity_snapshot_two_pgid"
  start_ticks_output="$parent_identity_snapshot_two_start_ticks"
  executable_output="$parent_identity_snapshot_two_executable"
  [ -n "$ppid_output" ]
}

launch_product() {
  if [ "$#" -ne 12 ]; then
    echo "internal launch contract is invalid" >&2
    return 2
  fi
  local mode="$1"
  local mode_root="$2"
  local application="$3"
  local port="$4"
  local child_path="$5"
  local pid_file="$6"
  local status_file="$7"
  local display_file="$8"
  local xauthority_file="$9"
  local runtime_dir="${10}"
  local mesa_renderer="${11}"
  local identity_file="${12}"
  local -a desktop_environment=()

  if [ "$(dirname -- "$identity_file")" != "$(dirname -- "$pid_file")" ] \
    || [ "$(dirname -- "$identity_file")" != "$(dirname -- "$status_file")" ] \
    || [ "$identity_file" = "$pid_file" ] \
    || [ "$identity_file" = "$status_file" ]; then
    echo "internal launch identity path is invalid" >&2
    return 2
  fi

  if [ "$mode" = desktop ]; then
    if [ -z "${DISPLAY:-}" ] \
      || [ -z "${XAUTHORITY:-}" ] \
      || [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
      echo "desktop isolation did not provide display, Xauthority, and D-Bus state" >&2
      return 1
    fi
    printf '%s\n' "$DISPLAY" >"$display_file"
    printf '%s\n' "$XAUTHORITY" >"$xauthority_file"
    desktop_environment=(
      DISPLAY="$DISPLAY"
      XAUTHORITY="$XAUTHORITY"
      DBUS_SESSION_BUS_ADDRESS="$DBUS_SESSION_BUS_ADDRESS"
      GDK_BACKEND=x11
      GSETTINGS_BACKEND=memory
      WEBKIT_DISABLE_DMABUF_RENDERER=1
      LIBGL_ALWAYS_SOFTWARE=1
      LIBGL_DRIVERS_PATH="$mesa_renderer/lib/dri"
      __EGL_VENDOR_LIBRARY_FILENAMES="$mesa_renderer/share/glvnd/egl_vendor.d/50_mesa.json"
      NO_AT_BRIDGE=1
    )
  fi

  cd "$mode_root"
  set +e
  env -i \
    HOME="$mode_root/home" \
    USERPROFILE="$mode_root/home" \
    USER=pokecon-package-check \
    LOGNAME=pokecon-package-check \
    XDG_CONFIG_HOME="$mode_root/config" \
    XDG_DATA_HOME="$mode_root/data" \
    XDG_CACHE_HOME="$mode_root/cache" \
    XDG_STATE_HOME="$mode_root/state" \
    XDG_RUNTIME_DIR="$runtime_dir" \
    TMPDIR="$mode_root/tmp" \
    APPDATA="$mode_root/appdata" \
    LOCALAPPDATA="$mode_root/localappdata" \
    LANG=C \
    LC_ALL=C \
    TZ=UTC \
    NO_PROXY=127.0.0.1,localhost \
    no_proxy=127.0.0.1,localhost \
    RUST_LOG=info \
    PATH="$child_path" \
    "${desktop_environment[@]}" \
    "$application" \
    --ui "$mode" \
    --bind-address 127.0.0.1 \
    --port "$port" \
    --dynamic-config-language none \
    --disable-compositing true \
    --ui-desktop-close-behavior keep_backend &
  local application_pid=$!
  local observed_start_ticks=
  local identity_pid
  local identity_state
  local identity_pgid
  local identity_start_ticks
  local identity_executable
  local identity_ready=false
  for _attempt in {1..100}; do
    if read_process_identity \
      "$application_pid" \
      identity_pid identity_state identity_pgid identity_start_ticks \
      identity_executable; then
      if [ -z "$observed_start_ticks" ]; then
        observed_start_ticks="$identity_start_ticks"
      elif [ "$identity_start_ticks" != "$observed_start_ticks" ]; then
        break
      fi
      if [ "$identity_pid" = "$application_pid" ] \
        && [ "$identity_executable" = "$application" ]; then
        identity_ready=true
        break
      fi
    fi
    if ! kill -0 "$application_pid" 2>/dev/null; then
      break
    fi
    sleep 0.1
  done
  if [ "$identity_ready" != true ]; then
    echo "application PID $application_pid died or did not exec the exact packaged application before the identity deadline" >&2
    return 1
  fi
  local identity_temporary="$identity_file.tmp.$application_pid"
  if ! (
    umask 077
    printf '%s\n' \
      "pid=$identity_pid" \
      "state=$identity_state" \
      "pgid=$identity_pgid" \
      "start_ticks=$identity_start_ticks" \
      "executable=$identity_executable" \
      >"$identity_temporary"
  ) || ! chmod 0600 "$identity_temporary" \
    || ! mv -f -- "$identity_temporary" "$identity_file"; then
    echo "application PID $application_pid identity record could not be published" >&2
    return 1
  fi
  printf '%s\n' "$application_pid" >"$pid_file"
  wait "$application_pid"
  local application_status=$?
  printf '%s\n' "$application_status" >"$status_file"
  return "$application_status"
}

if [ "${1:-}" = __launch_product ]; then
  shift
  launch_product "$@"
  exit $?
fi

if [ "$#" -ne 12 ]; then
  echo "usage: ui_package_check.sh PACKAGE PYTHON STORE GATE_ROOT BASH CHILD_PATH DBUS_SESSION_CONFIG MESA_RENDERER PROC_SOCKET_EVIDENCE PIDFD_SIGNAL EWMH_CLOSE_RELAY OPENAPI" >&2
  exit 2
fi

readonly package_output="$1"
readonly project_python="$2"
readonly store_directory="$3"
readonly gate_root="$4"
readonly bash_binary="$5"
readonly child_path="$6"
readonly dbus_session_config="$7"
readonly mesa_renderer_input="$8"
readonly proc_socket_evidence_input="$9"
readonly pidfd_signal_input="${10}"
readonly ewmh_close_relay_input="${11}"
readonly openapi_input="${12}"
script_path="$(readlink -f -- "$0")"
readonly script_path

active_pid=
active_pgid=
active_start_ticks=
active_executable=
active_supervisor=
active_supervisor_pgid=
active_supervisor_start_ticks=
active_supervisor_executable=
active_group_leader=
active_group_leader_pgid=
active_group_leader_start_ticks=
active_group_leader_executable=
secondary_pid=
secondary_pgid=
secondary_start_ticks=
secondary_executable=
close_relay_supervisor=
close_relay_supervisor_pgid=
close_relay_supervisor_start_ticks=
close_relay_supervisor_executable=
close_relay_status=
current_mode=
current_mode_root=
desktop_process_group=
desktop_process_group_members='[]'
desktop_dbus_address=
desktop_dbus_socket=
desktop_egl_dispatcher=
desktop_mesa_software_driver=
desktop_x11_library=
desktop_close_relay_evidence='null'
desktop_primary_start_ticks=
desktop_primary_executable=
desktop_listener_inode=
desktop_initial_window=
desktop_initial_width=
desktop_initial_height=
desktop_reopened_window=
desktop_reopened_width=
desktop_reopened_height=
desktop_initial_webkit_pid=
desktop_reopened_webkit_pid=
desktop_initial_fingerprints='[]'
desktop_post_close_fingerprints='[]'
desktop_reopened_fingerprints='[]'
desktop_disappeared_fingerprints='[]'
desktop_initial_connection='null'
desktop_reopened_connection='null'
desktop_post_close_empty=false
desktop_secondary_pid=
desktop_secondary_status=
desktop_secondary_helper_status=

process_identity_matches() {
  if [ "$#" -ne 4 ]; then
    return 2
  fi
  local expected_pid="$1"
  local expected_pgid="$2"
  local expected_start_ticks="$3"
  local expected_executable="$4"
  local live_pid
  local live_state
  local live_pgid
  local live_start_ticks
  local live_executable
  if [[ ! "$expected_pid" =~ ^[0-9]+$ ]] \
    || [ "$expected_pid" -le 1 ] \
    || [[ ! "$expected_pgid" =~ ^[0-9]+$ ]] \
    || [ "$expected_pgid" -le 1 ] \
    || [[ ! "$expected_start_ticks" =~ ^[0-9]+$ ]] \
    || [ "$expected_start_ticks" -le 0 ] \
    || [ -z "$expected_executable" ]; then
    return 1
  fi
  read_process_identity \
    "$expected_pid" \
    live_pid live_state live_pgid live_start_ticks live_executable \
    || return 1
  [ "$live_pid" = "$expected_pid" ] \
    && [ "$live_pgid" = "$expected_pgid" ] \
    && [ "$live_start_ticks" = "$expected_start_ticks" ] \
    && [ "$live_executable" = "$expected_executable" ]
}

signal_process_if_identity_matches() {
  if [ "$#" -ne 6 ]; then
    return 2
  fi
  local signal="$1"
  local label="$2"
  local expected_pid="$3"
  local expected_pgid="$4"
  local expected_start_ticks="$5"
  local expected_executable="$6"
  case "$signal" in
    TERM | KILL) ;;
    *) return 2 ;;
  esac
  local helper_status=0
  "$project_python" -I -S "$canonical_pidfd_signal" \
    "$signal" "$expected_pid" "$expected_pgid" "$expected_start_ticks" \
    "$expected_executable" \
    || helper_status=$?
  case "$helper_status" in
    0) return 0 ;;
    75)
      echo "ui-package-check: skipped SIG$signal for $label because its exact pidfd identity is no longer live" >&2
      return 1
      ;;
    *)
      echo "ui-package-check: pidfd helper failed stably for $label with status $helper_status" >&2
      return 2
      ;;
  esac
}

capture_expected_process_identity() {
  if [ "$#" -ne 8 ]; then
    return 2
  fi
  local requested_pid="$1"
  local expected_executable="$2"
  local label="$3"
  local -n pid_output="$4"
  local -n pgid_output="$5"
  local -n start_ticks_output="$6"
  local -n executable_output="$7"
  local attempts="$8"
  local observed_start_ticks=
  local live_pid
  local live_state
  local live_pgid
  local live_start_ticks
  local live_executable
  pid_output=
  pgid_output=
  start_ticks_output=
  executable_output=
  if [[ ! "$attempts" =~ ^[0-9]+$ ]] || [ "$attempts" -le 0 ]; then
    return 2
  fi
  for ((_attempt = 0; _attempt < attempts; _attempt++)); do
    if read_process_identity \
      "$requested_pid" \
      live_pid live_state live_pgid live_start_ticks live_executable; then
      if [ -z "$observed_start_ticks" ]; then
        observed_start_ticks="$live_start_ticks"
      elif [ "$live_start_ticks" != "$observed_start_ticks" ]; then
        break
      fi
      if [ "$live_pid" = "$requested_pid" ] \
        && [ "$live_executable" = "$expected_executable" ]; then
        pid_output="$live_pid"
        pgid_output="$live_pgid"
        start_ticks_output="$live_start_ticks"
        executable_output="$live_executable"
        return 0
      fi
    fi
    sleep 0.05
  done
  echo "ui-package-check: $label did not expose its exact immutable process identity before the capture deadline" >&2
  return 1
}

reset_primary_identity_cache() {
  active_pid=
  active_pgid=
  active_start_ticks=
  active_executable=
  active_group_leader=
  active_group_leader_pgid=
  active_group_leader_start_ticks=
  active_group_leader_executable=
}

reset_active_supervisor_identity_cache() {
  active_supervisor=
  active_supervisor_pgid=
  active_supervisor_start_ticks=
  active_supervisor_executable=
}

reset_close_relay_identity_cache() {
  close_relay_supervisor=
  close_relay_supervisor_pgid=
  close_relay_supervisor_start_ticks=
  close_relay_supervisor_executable=
}

reset_secondary_identity_cache() {
  secondary_pid=
  secondary_pgid=
  secondary_start_ticks=
  secondary_executable=
}

consume_published_secondary_identity() {
  if [ "$#" -ne 2 ]; then
    return 2
  fi
  local identity_file="$1"
  local pid_file="$2"
  if [ ! -f "$identity_file" ] \
    || [ -L "$identity_file" ] \
    || [ "$(stat -c '%a' -- "$identity_file")" != 600 ]; then
    fail "same-session secondary identity record is missing, indirect, or non-private"
  fi
  if [ ! -f "$pid_file" ] || [ -L "$pid_file" ]; then
    fail "same-session secondary PID record is missing or indirect"
  fi
  local -a published_identity=()
  local -a published_pid_record=()
  mapfile -t published_identity <"$identity_file"
  mapfile -t published_pid_record <"$pid_file"
  if [ "${#published_identity[@]}" -ne 5 ] \
    || [[ "${published_identity[0]}" != pid=* ]] \
    || [[ "${published_identity[1]}" != state=* ]] \
    || [[ "${published_identity[2]}" != pgid=* ]] \
    || [[ "${published_identity[3]}" != start_ticks=* ]] \
    || [[ "${published_identity[4]}" != executable=* ]]; then
    fail "same-session secondary identity record has an invalid schema"
  fi
  local published_pid="${published_identity[0]#pid=}"
  local published_state="${published_identity[1]#state=}"
  local published_pgid="${published_identity[2]#pgid=}"
  local published_start_ticks="${published_identity[3]#start_ticks=}"
  local published_executable="${published_identity[4]#executable=}"
  if [ "${#published_pid_record[@]}" -ne 1 ] \
    || [ "${published_pid_record[0]}" != "$published_pid" ] \
    || [[ ! "$published_pid" =~ ^[0-9]+$ ]] \
    || [ "$published_pid" -le 1 ] \
    || [ "$published_pid" = "$active_pid" ]; then
    fail "same-session secondary published an invalid or non-distinct application PID"
  fi
  if [[ ! "$published_state" =~ ^[A-Za-z]$ ]]; then
    fail "same-session secondary published an invalid application state"
  fi
  case "$published_state" in
    X | x | Z) fail "same-session secondary published a dead application state" ;;
  esac
  if [[ ! "$published_pgid" =~ ^[0-9]+$ ]] \
    || [ "$published_pgid" -le 1 ] \
    || [[ ! "$published_start_ticks" =~ ^[0-9]+$ ]] \
    || [ "$published_start_ticks" -le 0 ] \
    || [ "$published_executable" != "$application" ]; then
    fail "same-session secondary published an invalid application identity"
  fi
  secondary_pid="$published_pid"
  secondary_pgid="$published_pgid"
  secondary_start_ticks="$published_start_ticks"
  secondary_executable="$published_executable"
  desktop_secondary_pid="$published_pid"
}

collect_descendants() {
  local root_pid="$1"
  local -a queue=("$root_pid")
  local -a descendants=()
  local parent
  local child
  local -a children=()
  while [ "${#queue[@]}" -gt 0 ]; do
    parent="${queue[0]}"
    queue=("${queue[@]:1}")
    mapfile -t children < <(pgrep -P "$parent" 2>/dev/null || true)
    for child in "${children[@]}"; do
      descendants+=("$child")
      queue+=("$child")
    done
  done
  if [ "${#descendants[@]}" -gt 0 ]; then
    printf '%s\n' "${descendants[@]}"
  fi
}

primary_group_anchor_matches() {
  if [[ "$active_pgid" =~ ^[0-9]+$ ]] \
    && [ "$active_pgid" -gt 1 ] \
    && process_identity_matches \
      "$active_pid" "$active_pgid" "$active_start_ticks" \
      "$active_executable"; then
    return 0
  fi
  [[ "$active_group_leader" =~ ^[0-9]+$ ]] \
    && [ "$active_group_leader" -gt 1 ] \
    && [ "$active_group_leader" = "$active_pgid" ] \
    && [ "$active_group_leader_pgid" = "$active_pgid" ] \
    && process_identity_matches \
      "$active_group_leader" "$active_group_leader_pgid" \
      "$active_group_leader_start_ticks" "$active_group_leader_executable"
}

snapshot_primary_group_identities() {
  if [ "$#" -ne 4 ]; then
    return 2
  fi
  local -n pid_output="$1"
  local -n pgid_output="$2"
  local -n start_ticks_output="$3"
  local -n executable_output="$4"
  local candidate_pid
  local candidate_pgid
  local live_pid
  local live_state
  local live_pgid
  local live_start_ticks
  local live_executable
  pid_output=()
  pgid_output=()
  start_ticks_output=()
  executable_output=()
  if ! primary_group_anchor_matches; then
    echo "ui-package-check: skipped primary process-group cleanup because no exact group anchor remains live" >&2
    return 1
  fi
  while read -r candidate_pid candidate_pgid; do
    if [ "$candidate_pgid" != "$active_pgid" ]; then
      continue
    fi
    if read_process_identity \
      "$candidate_pid" \
      live_pid live_state live_pgid live_start_ticks live_executable \
      && [ "$live_pid" = "$candidate_pid" ] \
      && [ "$live_pgid" = "$active_pgid" ]; then
      pid_output+=("$live_pid")
      pgid_output+=("$live_pgid")
      start_ticks_output+=("$live_start_ticks")
      executable_output+=("$live_executable")
    fi
  done < <(ps -e -o pid= -o pgid=)
  if [ "${#pid_output[@]}" -eq 0 ] || ! primary_group_anchor_matches; then
    pid_output=()
    pgid_output=()
    start_ticks_output=()
    executable_output=()
    echo "ui-package-check: skipped primary process-group cleanup because its exact snapshot could not be anchored" >&2
    return 1
  fi
}

show_failure_evidence() {
  if [ -z "$current_mode_root" ] || [ ! -d "$current_mode_root" ]; then
    return
  fi
  echo "ui-package-check failure evidence for mode=$current_mode" >&2
  local evidence
  for evidence in \
    "$current_mode_root/application.log" \
    "$current_mode_root/xvfb.stderr" \
    "$current_mode_root/close-relay.ready.json" \
    "$current_mode_root/close-relay.json" \
    "$current_mode_root/close-relay.stderr" \
    "$current_mode_root/secondary.log" \
    "$current_mode_root/secondary.identity" \
    "$current_mode_root/secondary.pid" \
    "$current_mode_root/secondary.status" \
    "$current_mode_root/secondary.helper.status" \
    "$current_mode_root/secondary.display" \
    "$current_mode_root/secondary.xauthority" \
    "$current_mode_root"/resource-snapshots.* \
    "$current_mode_root/openapi-method-results.json" \
    "$current_mode_root/cors-preflight-policy-results.json" \
    "$current_mode_root/unknown-api-boundary-results.json" \
    "$current_mode_root/advertised-bare-options-results.json" \
    "$current_mode_root"/socket.*.json \
    "$current_mode_root"/socket.*.stderr \
    "$current_mode_root"/*.headers \
    "$current_mode_root"/*.body; do
    if [ ! -s "$evidence" ]; then
      continue
    fi
    echo "--- $(basename -- "$evidence") (bounded) ---" >&2
    head -c 16384 -- "$evidence" >&2 || true
    echo >&2
  done
  if [ -s "$current_mode_root/display" ] \
    && [ -s "$current_mode_root/xauthority" ]; then
    local display
    local xauthority
    display="$(head -n 1 -- "$current_mode_root/display")"
    xauthority="$(head -n 1 -- "$current_mode_root/xauthority")"
    timeout --signal=TERM --kill-after=1s 2s \
      env DISPLAY="$display" XAUTHORITY="$xauthority" \
      xwininfo -root -tree 2>&1 | head -n 160 >&2 || true
  fi
  if [[ "$active_pid" =~ ^[0-9]+$ ]] && [ -d "/proc/$active_pid" ]; then
    ps -o pid=,ppid=,stat=,comm= -p "$active_pid" --ppid "$active_pid" 2>&1 \
      | head -n 80 >&2 || true
  fi
}

snapshot_secondary_tree_identities() {
  if [ "$#" -ne 4 ]; then
    return 2
  fi
  # These caller-owned namerefs are indexed arrays initialized immediately below.
  # shellcheck disable=SC2178
  local -n pid_output="$1"
  # shellcheck disable=SC2178
  local -n pgid_output="$2"
  # shellcheck disable=SC2178
  local -n start_ticks_output="$3"
  # shellcheck disable=SC2178
  local -n executable_output="$4"
  local -A seen_pids=()
  local queue_index=0
  local parent_pid
  local parent_pgid
  local parent_start_ticks
  local parent_executable
  local proc_path
  local candidate_pid
  local candidate_ppid
  local candidate_pgid
  local candidate_start_ticks
  local candidate_executable
  local unsafe_reason=
  pid_output=()
  pgid_output=()
  start_ticks_output=()
  executable_output=()

  if ! process_identity_matches \
    "$secondary_pid" "$secondary_pgid" "$secondary_start_ticks" \
    "$secondary_executable"; then
    unsafe_reason="the cached secondary root identity is not live"
  else
    pid_output+=("$secondary_pid")
    pgid_output+=("$secondary_pgid")
    start_ticks_output+=("$secondary_start_ticks")
    executable_output+=("$secondary_executable")
    seen_pids["$secondary_pid"]=1
  fi

  while [ -z "$unsafe_reason" ] \
    && [ "$queue_index" -lt "${#pid_output[@]}" ]; do
    parent_pid="${pid_output[$queue_index]}"
    parent_pgid="${pgid_output[$queue_index]}"
    parent_start_ticks="${start_ticks_output[$queue_index]}"
    parent_executable="${executable_output[$queue_index]}"
    if ! process_identity_matches \
      "$parent_pid" "$parent_pgid" "$parent_start_ticks" \
      "$parent_executable"; then
      unsafe_reason="a queued secondary parent identity changed before child discovery"
      break
    fi
    for proc_path in /proc/[0-9]*; do
      candidate_pid="${proc_path#/proc/}"
      if [[ ! "$candidate_pid" =~ ^[0-9]+$ ]] \
        || [ "$candidate_pid" -le 1 ] \
        || [ -n "${seen_pids[$candidate_pid]+present}" ]; then
        continue
      fi
      if ! read_process_parent_identity \
        "$candidate_pid" \
        candidate_pid candidate_ppid candidate_pgid candidate_start_ticks \
        candidate_executable; then
        continue
      fi
      if [ "$candidate_ppid" != "$parent_pid" ]; then
        continue
      fi
      if [ "${#pid_output[@]}" -ge 256 ]; then
        unsafe_reason="the secondary process tree exceeded the 256-member limit"
        break
      fi
      seen_pids["$candidate_pid"]=1
      pid_output+=("$candidate_pid")
      pgid_output+=("$candidate_pgid")
      start_ticks_output+=("$candidate_start_ticks")
      executable_output+=("$candidate_executable")
    done
    if [ -z "$unsafe_reason" ] \
      && ! process_identity_matches \
        "$parent_pid" "$parent_pgid" "$parent_start_ticks" \
        "$parent_executable"; then
      unsafe_reason="a queued secondary parent identity changed during child discovery"
      break
    fi
    ((queue_index += 1))
  done

  if [ -z "$unsafe_reason" ] \
    && ! process_identity_matches \
      "$secondary_pid" "$secondary_pgid" "$secondary_start_ticks" \
      "$secondary_executable"; then
    unsafe_reason="the secondary root identity changed during tree discovery"
  fi
  if [ -n "$unsafe_reason" ]; then
    pid_output=()
    pgid_output=()
    start_ticks_output=()
    executable_output=()
    echo "ui-package-check: skipped secondary tree snapshot because $unsafe_reason" >&2
    return 1
  fi
}

cleanup_secondary_process_tree() {
  local -a secondary_tree_pids=()
  local -a secondary_tree_pgids=()
  local -a secondary_tree_start_ticks=()
  local -a secondary_tree_executables=()
  if ! snapshot_secondary_tree_identities \
    secondary_tree_pids secondary_tree_pgids secondary_tree_start_ticks \
    secondary_tree_executables; then
    reset_secondary_identity_cache
    return
  fi

  local member_index
  for member_index in "${!secondary_tree_pids[@]}"; do
    signal_process_if_identity_matches \
      TERM "secondary process-tree member" \
      "${secondary_tree_pids[$member_index]}" \
      "${secondary_tree_pgids[$member_index]}" \
      "${secondary_tree_start_ticks[$member_index]}" \
      "${secondary_tree_executables[$member_index]}" \
      || true
  done
  for _attempt in {1..30}; do
    local member_alive=false
    for member_index in "${!secondary_tree_pids[@]}"; do
      if process_identity_matches \
        "${secondary_tree_pids[$member_index]}" \
        "${secondary_tree_pgids[$member_index]}" \
        "${secondary_tree_start_ticks[$member_index]}" \
        "${secondary_tree_executables[$member_index]}"; then
        member_alive=true
        break
      fi
    done
    if [ "$member_alive" = false ]; then
      break
    fi
    sleep 0.1
  done
  for member_index in "${!secondary_tree_pids[@]}"; do
    signal_process_if_identity_matches \
      KILL "secondary process-tree member" \
      "${secondary_tree_pids[$member_index]}" \
      "${secondary_tree_pgids[$member_index]}" \
      "${secondary_tree_start_ticks[$member_index]}" \
      "${secondary_tree_executables[$member_index]}" \
      || true
  done
  reset_secondary_identity_cache
}

failure_cleanup() {
  if [[ "$close_relay_supervisor" =~ ^[0-9]+$ ]] \
    && [ "$close_relay_supervisor" -gt 1 ]; then
    signal_process_if_identity_matches \
      TERM "X11 close-request relay supervisor" \
      "$close_relay_supervisor" "$close_relay_supervisor_pgid" \
      "$close_relay_supervisor_start_ticks" \
      "$close_relay_supervisor_executable" \
      || true
    wait "$close_relay_supervisor" 2>/dev/null || true
    reset_close_relay_identity_cache
  fi
  if [ -n "$secondary_pid" ]; then
    cleanup_secondary_process_tree
  fi
  local -a group_member_pids=()
  local -a group_member_pgids=()
  local -a group_member_start_ticks=()
  local -a group_member_executables=()
  if snapshot_primary_group_identities \
    group_member_pids group_member_pgids group_member_start_ticks \
    group_member_executables; then
    local member_index
    for member_index in "${!group_member_pids[@]}"; do
      signal_process_if_identity_matches \
        TERM "primary process-group member" \
        "${group_member_pids[$member_index]}" \
        "${group_member_pgids[$member_index]}" \
        "${group_member_start_ticks[$member_index]}" \
        "${group_member_executables[$member_index]}" \
        || true
    done
    for _attempt in {1..30}; do
      local group_member_alive=false
      for member_index in "${!group_member_pids[@]}"; do
        if process_identity_matches \
          "${group_member_pids[$member_index]}" \
          "${group_member_pgids[$member_index]}" \
          "${group_member_start_ticks[$member_index]}" \
          "${group_member_executables[$member_index]}"; then
          group_member_alive=true
          break
        fi
      done
      if [ "$group_member_alive" = false ]; then
        break
      fi
      sleep 0.1
    done
    for member_index in "${!group_member_pids[@]}"; do
      signal_process_if_identity_matches \
        KILL "primary process-group member" \
        "${group_member_pids[$member_index]}" \
        "${group_member_pgids[$member_index]}" \
        "${group_member_start_ticks[$member_index]}" \
        "${group_member_executables[$member_index]}" \
        || true
    done
  fi
  reset_primary_identity_cache
  if [[ "$active_supervisor" =~ ^[0-9]+$ ]] \
    && [ "$active_supervisor" -gt 1 ]; then
    signal_process_if_identity_matches \
      TERM "main setsid supervisor" \
      "$active_supervisor" "$active_supervisor_pgid" \
      "$active_supervisor_start_ticks" "$active_supervisor_executable" \
      || true
    for _attempt in {1..20}; do
      if ! process_identity_matches \
        "$active_supervisor" "$active_supervisor_pgid" \
        "$active_supervisor_start_ticks" "$active_supervisor_executable"; then
        break
      fi
      sleep 0.1
    done
    if process_identity_matches \
      "$active_supervisor" "$active_supervisor_pgid" \
      "$active_supervisor_start_ticks" "$active_supervisor_executable"; then
      signal_process_if_identity_matches \
        KILL "main setsid supervisor" \
        "$active_supervisor" "$active_supervisor_pgid" \
        "$active_supervisor_start_ticks" "$active_supervisor_executable" \
        || true
      for _attempt in {1..20}; do
        if ! process_identity_matches \
          "$active_supervisor" "$active_supervisor_pgid" \
          "$active_supervisor_start_ticks" "$active_supervisor_executable"; then
          break
        fi
        sleep 0.1
      done
    fi
    if process_identity_matches \
      "$active_supervisor" "$active_supervisor_pgid" \
      "$active_supervisor_start_ticks" "$active_supervisor_executable"; then
      echo "ui-package-check: failure supervisor remains uninterruptible after cleanup deadline" >&2
      return
    fi
    wait "$active_supervisor" 2>/dev/null || true
    reset_active_supervisor_identity_cache
  fi
}

on_exit() {
  local status=$?
  if [ "$status" -ne 0 ]; then
    show_failure_evidence
    failure_cleanup
  fi
  exit "$status"
}
trap on_exit EXIT

fail() {
  echo "ui-package-check: $*" >&2
  exit 1
}

require_contained_path() {
  local candidate="$1"
  local parent="$2"
  local label="$3"
  case "$candidate" in
    "$parent"/*) ;;
    *) fail "$label escapes the selected package: $candidate" ;;
  esac
}

validate_unique_json_object_keys() {
  if [ "$#" -ne 1 ]; then
    return 2
  fi
  "$project_python" -I -S -c '
import json
import sys


def unique_object(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON object key: {key}")
        result[key] = value
    return result


with open(sys.argv[1], encoding="utf-8") as document:
    json.load(document, object_pairs_hook=unique_object)
' "$1"
}
readonly -f validate_unique_json_object_keys

build_openapi_operation_inventory() {
  if [ "$#" -ne 2 ]; then
    return 2
  fi
  local document="$1"
  local inventory="$2"
  if ! jq -eS '
    def safe_api_path:
      type == "string"
      and test("^/api/[a-z0-9][a-z0-9-]*(/[a-z0-9][a-z0-9-]*)*$");
    def supported_method:
      . == "get" or . == "post" or . == "patch";
    if type != "object" or (.paths | type) != "object" then
      error("OpenAPI paths must be one JSON object")
    else
      [
        .paths
        | to_entries[]
        | .key as $path
        | if (($path | safe_api_path) or $path == "/ws") then .
          else error("OpenAPI contains an unsafe public path")
          end
        | if ((.value | type) == "object" and (.value | length) > 0) then .
          else error("OpenAPI path item must be a nonempty object")
          end
        | .value
        | to_entries[]
        | .key as $method
        | .value as $operation
        | if (
            ($method | supported_method)
            and ($operation | type) == "object"
            and ($operation.operationId | type) == "string"
            and ($operation.operationId | test("^[a-z][a-z0-9_]*$"))
            and ($operation.responses | type) == "object"
            and ($operation.responses | length) > 0
          ) then {
            method: $method,
            operation_id: $operation.operationId,
            path: $path
          }
          else error("OpenAPI path item contains an unsupported or malformed operation")
          end
      ]
      | sort_by([.path, .method])
      | if (
          length == 16
          and length == (unique_by([.path, .method]) | length)
          and length == (unique_by(.operation_id) | length)
          and (unique_by(.path) | length) == 15
          and ([.[] | select(.path | startswith("/api/"))] | length) == 15
          and ([.[] | select(.path == "/ws")] | length) == 1
          and ([.[] | select(.path == "/ws" and .method == "get")] | length) == 1
          and ([.[] | select(.method == "get")] | length) == 5
          and ([.[] | select(.method == "post")] | length) == 10
          and ([.[] | select(.method == "patch")] | length) == 1
        ) then .
        else error("OpenAPI public operation inventory is not the canonical 16-operation shape")
        end
    end
  ' "$document" >"$inventory"; then
    fail "OpenAPI public operation inventory is malformed, unsafe, duplicated, or unsupported"
  fi
}

if [ ! -x "$project_python" ]; then
  fail "project Python is unavailable: $project_python"
fi
if [ ! -x "$bash_binary" ]; then
  fail "fixed Bash is unavailable: $bash_binary"
fi
case "$proc_socket_evidence_input" in
  /*) ;;
  *) fail "proc socket evidence helper is not absolute: $proc_socket_evidence_input" ;;
esac
if [ ! -f "$proc_socket_evidence_input" ] || [ -L "$proc_socket_evidence_input" ]; then
  fail "proc socket evidence helper is not a regular immutable file: $proc_socket_evidence_input"
fi
canonical_proc_socket_evidence="$(readlink -f -- "$proc_socket_evidence_input")"
expected_proc_socket_evidence="$(dirname -- "$script_path")/proc_socket_evidence.py"
if [ "$canonical_proc_socket_evidence" != "$proc_socket_evidence_input" ] \
  || [ "$canonical_proc_socket_evidence" != "$expected_proc_socket_evidence" ]; then
  fail "proc socket evidence helper does not match the gate source: $proc_socket_evidence_input"
fi
case "$canonical_proc_socket_evidence" in
  "$store_directory"/*/*) ;;
  *) fail "proc socket evidence helper is outside an immutable store output" ;;
esac
case "$pidfd_signal_input" in
  /*) ;;
  *) fail "pidfd signal helper is not absolute: $pidfd_signal_input" ;;
esac
if [ ! -f "$pidfd_signal_input" ] || [ -L "$pidfd_signal_input" ]; then
  fail "pidfd signal helper is not a regular immutable file: $pidfd_signal_input"
fi
canonical_pidfd_signal="$(readlink -f -- "$pidfd_signal_input")"
expected_pidfd_signal="$(dirname -- "$script_path")/pidfd_signal.py"
if [ "$canonical_pidfd_signal" != "$pidfd_signal_input" ] \
  || [ "$canonical_pidfd_signal" != "$expected_pidfd_signal" ]; then
  fail "pidfd signal helper does not match the gate source: $pidfd_signal_input"
fi
case "$canonical_pidfd_signal" in
  "$store_directory"/*/*) ;;
  *) fail "pidfd signal helper is outside an immutable store output" ;;
esac
readonly canonical_pidfd_signal expected_pidfd_signal
case "$ewmh_close_relay_input" in
  /*) ;;
  *) fail "EWMH close relay is not absolute: $ewmh_close_relay_input" ;;
esac
if [ ! -f "$ewmh_close_relay_input" ] || [ -L "$ewmh_close_relay_input" ]; then
  fail "EWMH close relay is not a regular immutable file: $ewmh_close_relay_input"
fi
canonical_ewmh_close_relay="$(readlink -f -- "$ewmh_close_relay_input")"
expected_ewmh_close_relay="$(dirname -- "$script_path")/ewmh_close_relay.py"
if [ "$canonical_ewmh_close_relay" != "$ewmh_close_relay_input" ] \
  || [ "$canonical_ewmh_close_relay" != "$expected_ewmh_close_relay" ]; then
  fail "EWMH close relay does not match the gate source: $ewmh_close_relay_input"
fi
case "$canonical_ewmh_close_relay" in
  "$store_directory"/*/*) ;;
  *) fail "EWMH close relay is outside an immutable store output" ;;
esac
case "$openapi_input" in
  /*) ;;
  *) fail "OpenAPI document is not absolute: $openapi_input" ;;
esac
if [ ! -f "$openapi_input" ] || [ -L "$openapi_input" ]; then
  fail "OpenAPI document is not a regular immutable file: $openapi_input"
fi
gate_source_output="$(
  dirname -- "$(dirname -- "$(dirname -- "$script_path")")"
)"
if [ ! -d "$gate_source_output" ] \
  || [ -L "$gate_source_output" ] \
  || [ "$(readlink -f -- "$gate_source_output")" != "$gate_source_output" ] \
  || [ "$(dirname -- "$gate_source_output")" != "$store_directory" ] \
  || [ "$script_path" != "$gate_source_output/scripts/integration/ui_package_check.sh" ]; then
  fail "gate source is not a canonical direct immutable store output: $gate_source_output"
fi
canonical_openapi="$(readlink -f -- "$openapi_input")"
expected_openapi="$gate_source_output/api/openapi.json"
if [ "$canonical_openapi" != "$openapi_input" ] \
  || [ "$canonical_openapi" != "$expected_openapi" ]; then
  fail "OpenAPI document does not match the gate source: $openapi_input"
fi
if ! validate_unique_json_object_keys "$canonical_openapi" 2>/dev/null; then
  fail "OpenAPI document is invalid JSON or contains duplicate object keys"
fi
openapi_operation_inventory="$gate_root/openapi-operation-inventory.json"
build_openapi_operation_inventory \
  "$canonical_openapi" "$openapi_operation_inventory"
openapi_sha256="$(sha256sum "$canonical_openapi" | cut -d ' ' -f 1)"
openapi_operation_count="$(jq -er 'length' "$openapi_operation_inventory")"
openapi_rest_operation_count="$(
  jq -er '[.[] | select(.path | startswith("/api/"))] | length' \
    "$openapi_operation_inventory"
)"
openapi_websocket_operation_count="$(
  jq -er '[.[] | select(.path == "/ws")] | length' \
    "$openapi_operation_inventory"
)"
openapi_path_count="$(
  jq -er '[.[].path] | unique | length' "$openapi_operation_inventory"
)"
openapi_rest_path_count="$(
  jq -er '[.[].path | select(startswith("/api/"))] | unique | length' \
    "$openapi_operation_inventory"
)"
openapi_websocket_path_count="$(
  jq -er '[.[].path | select(. == "/ws")] | unique | length' \
    "$openapi_operation_inventory"
)"
readonly -a cors_success_forbidden_response_headers=(
  Allow
  Content-Length
  Transfer-Encoding
  Access-Control-Allow-Credentials
  Access-Control-Expose-Headers
  Access-Control-Max-Age
)
readonly -a advertised_bare_options_forbidden_response_headers=(
  Access-Control-Allow-Headers
  Access-Control-Allow-Methods
  Access-Control-Allow-Origin
  Allow
  Vary
)
advertised_bare_options_forbidden_response_headers_json="$(
  printf '%s\n' "${advertised_bare_options_forbidden_response_headers[@]}" \
    | jq -Rsc 'split("\n")[:-1]'
)"
readonly advertised_bare_options_forbidden_response_headers_json
readonly -a cors_rejection_forbidden_response_headers=(
  Access-Control-Allow-Credentials
  Access-Control-Allow-Headers
  Access-Control-Allow-Methods
  Access-Control-Allow-Origin
  Access-Control-Expose-Headers
  Access-Control-Max-Age
  Allow
  Vary
)
cors_rejection_forbidden_response_headers_json="$(
  printf '%s\n' "${cors_rejection_forbidden_response_headers[@]}" \
    | jq -Rsc 'split("\n")[:-1]'
)"
readonly cors_rejection_forbidden_response_headers_json
readonly -a openapi_method_matrix=(
  get head post put patch delete options trace connect
)
openapi_method_count="${#openapi_method_matrix[@]}"
openapi_method_probe_count="$((openapi_path_count * openapi_method_count))"
openapi_cors_preflight_count="$openapi_path_count"
openapi_rejected_method_count="$((
  openapi_method_probe_count
  - openapi_operation_count
  - openapi_cors_preflight_count
))"
readonly gate_source_output canonical_openapi openapi_operation_inventory
readonly openapi_sha256 openapi_operation_count openapi_rest_operation_count
readonly openapi_websocket_operation_count openapi_path_count openapi_method_count
readonly openapi_rest_path_count openapi_websocket_path_count
readonly openapi_method_probe_count openapi_cors_preflight_count
readonly openapi_rejected_method_count
case "$dbus_session_config" in
  /*) ;;
  *) fail "D-Bus session configuration is not absolute: $dbus_session_config" ;;
esac
if [ ! -f "$dbus_session_config" ] || [ -L "$dbus_session_config" ]; then
  fail "D-Bus session configuration is not a regular immutable file: $dbus_session_config"
fi
canonical_dbus_session_config="$(readlink -f -- "$dbus_session_config")"
dbus_store_relative="${canonical_dbus_session_config#"$store_directory"/}"
dbus_store_output_name="${dbus_store_relative%%/*}"
if [ "$dbus_store_relative" = "$canonical_dbus_session_config" ] \
  || [ -z "$dbus_store_output_name" ] \
  || [ "$dbus_store_relative" = "$dbus_store_output_name" ]; then
  fail "D-Bus session configuration is not inside a direct store output: $canonical_dbus_session_config"
fi
dbus_store_output="$store_directory/$dbus_store_output_name"
if [ ! -d "$dbus_store_output" ] \
  || [ -L "$dbus_store_output" ] \
  || [ "$canonical_dbus_session_config" != "$dbus_store_output/share/dbus-1/session.conf" ]; then
  fail "D-Bus session configuration has an unexpected store layout: $canonical_dbus_session_config"
fi
case "$mesa_renderer_input" in
  /*) ;;
  *) fail "Mesa renderer output is not absolute: $mesa_renderer_input" ;;
esac
if [ ! -d "$mesa_renderer_input" ] || [ -L "$mesa_renderer_input" ]; then
  fail "Mesa renderer is not a real immutable directory: $mesa_renderer_input"
fi
canonical_mesa_renderer="$(readlink -f -- "$mesa_renderer_input")"
if [ "$canonical_mesa_renderer" != "$mesa_renderer_input" ] \
  || [ "$(dirname -- "$canonical_mesa_renderer")" != "$store_directory" ]; then
  fail "Mesa renderer is not a direct output under evaluator store $store_directory: $mesa_renderer_input"
fi
mesa_egl_vendor_manifest="$canonical_mesa_renderer/share/glvnd/egl_vendor.d/50_mesa.json"
if [ ! -f "$mesa_egl_vendor_manifest" ] || [ -L "$mesa_egl_vendor_manifest" ]; then
  fail "Mesa EGL vendor manifest is not a regular immutable file: $mesa_egl_vendor_manifest"
fi
if [ "$(readlink -f -- "$mesa_egl_vendor_manifest")" != "$mesa_egl_vendor_manifest" ]; then
  fail "Mesa EGL vendor manifest traverses an unexpected symlink: $mesa_egl_vendor_manifest"
fi
mesa_vendor_library_declared="$(
  jq -er '
    .ICD.library_path
    | if type == "string" and length > 0 then . else empty end
  ' "$mesa_egl_vendor_manifest"
)" || fail "Mesa EGL vendor manifest does not declare one library path"
case "$mesa_vendor_library_declared" in
  "$canonical_mesa_renderer"/*) ;;
  *) fail "Mesa EGL vendor manifest declares a library outside its output: $mesa_vendor_library_declared" ;;
esac
mesa_egl_vendor_library="$(
  readlink -f -- "$mesa_vendor_library_declared" 2>/dev/null || true
)"
case "$mesa_egl_vendor_library" in
  "$canonical_mesa_renderer"/*) ;;
  *) fail "Mesa EGL vendor library escapes its renderer output: $mesa_egl_vendor_library" ;;
esac
if [ ! -f "$mesa_egl_vendor_library" ] || [ -L "$mesa_egl_vendor_library" ]; then
  fail "Mesa EGL vendor library is not a canonical regular file: $mesa_egl_vendor_library"
fi
mesa_dri_directory="$canonical_mesa_renderer/lib/dri"
if [ ! -d "$mesa_dri_directory" ] \
  || [ -L "$mesa_dri_directory" ] \
  || [ "$(readlink -f -- "$mesa_dri_directory")" != "$mesa_dri_directory" ]; then
  fail "Mesa DRI directory is not a real immutable directory: $mesa_dri_directory"
fi
mesa_swrast_entry="$mesa_dri_directory/swrast_dri.so"
if [ ! -f "$mesa_swrast_entry" ]; then
  fail "Mesa software rasterizer entry is unavailable: $mesa_swrast_entry"
fi
canonical_mesa_swrast_driver="$(readlink -f -- "$mesa_swrast_entry")"
case "$canonical_mesa_swrast_driver" in
  "$canonical_mesa_renderer"/*) ;;
  *) fail "Mesa software rasterizer escapes its renderer output: $canonical_mesa_swrast_driver" ;;
esac
if [ ! -f "$canonical_mesa_swrast_driver" ] \
  || [ -L "$canonical_mesa_swrast_driver" ]; then
  fail "Mesa software rasterizer target is not a canonical regular file: $canonical_mesa_swrast_driver"
fi
if [ -n "${LD_LIBRARY_PATH:-}" ]; then
  fail "gate process inherited a library-path override"
fi

canonical_package="$(readlink -f -- "$package_output")"
if [ "$(dirname -- "$canonical_package")" != "$store_directory" ]; then
  fail "package is not a direct output under evaluator store $store_directory: $canonical_package"
fi
application="$(readlink -f -- "$canonical_package/bin/pokecon")"
if [ ! -f "$application" ] || [ ! -x "$application" ]; then
  fail "packaged application is missing or not executable: $application"
fi
require_contained_path "$application" "$canonical_package" application
setsid_executable="$(command -v setsid 2>/dev/null || true)"
setsid_executable="$(readlink -f -- "$setsid_executable" 2>/dev/null || true)"
case "$setsid_executable" in
  "$store_directory"/*/*) ;;
  *) fail "setsid does not resolve to an immutable store executable: $setsid_executable" ;;
esac
if [ ! -f "$setsid_executable" ] \
  || [ -L "$setsid_executable" ] \
  || [ ! -x "$setsid_executable" ]; then
  fail "setsid is not an exact canonical executable: $setsid_executable"
fi
readonly setsid_executable
timeout_command="$(command -v timeout 2>/dev/null || true)"
timeout_store_relative="${timeout_command#"$store_directory"/}"
timeout_store_output_name="${timeout_store_relative%%/*}"
if [ "$timeout_store_relative" = "$timeout_command" ] \
  || [ -z "$timeout_store_output_name" ] \
  || [ "$timeout_command" != "$store_directory/$timeout_store_output_name/bin/timeout" ] \
  || [ ! -f "$timeout_command" ] \
  || [ ! -x "$timeout_command" ]; then
  fail "timeout command is not an executable in a direct immutable store output: $timeout_command"
fi
timeout_executable="$(readlink -f -- "$timeout_command" 2>/dev/null || true)"
case "$timeout_executable" in
  "$store_directory"/*/*) ;;
  *) fail "timeout does not resolve to an immutable store executable: $timeout_executable" ;;
esac
if [ ! -f "$timeout_executable" ] \
  || [ -L "$timeout_executable" ] \
  || [ ! -x "$timeout_executable" ]; then
  fail "timeout is not an exact canonical executable: $timeout_executable"
fi
readonly timeout_command timeout_executable

canonical_web="$(readlink -f -- "$canonical_package/web/dist")"
canonical_bin_web="$(readlink -f -- "$canonical_package/bin/web/dist")"
if [ ! -d "$canonical_web" ] || [ "$canonical_bin_web" != "$canonical_web" ]; then
  fail "executable-relative Web root does not resolve to the immutable distribution"
fi
require_contained_path "$canonical_web" "$canonical_package" "Web distribution"
packaged_index="$canonical_web/index.html"
if [ ! -f "$packaged_index" ] || [ -L "$packaged_index" ]; then
  fail "packaged Web entrypoint is not a regular immutable file"
fi
if grep -qiE '404|Not Found' "$packaged_index"; then
  fail "packaged Web entrypoint contains SvelteKit error page indicators"
fi
asset_path="$(grep -oE '/_app/immutable/entry/app\.[A-Za-z0-9_-]+\.js' \
  "$packaged_index" | head -n 1 || true)"
if [ -z "$asset_path" ]; then
  fail "packaged Web entrypoint does not identify the main Svelte asset"
fi
case "$asset_path" in
  /_app/immutable/entry/app.*.js) ;;
  *) fail "main Svelte asset path is unsafe: $asset_path" ;;
esac
packaged_asset="$(readlink -f -- "$canonical_web/${asset_path#/}")"
require_contained_path "$packaged_asset" "$canonical_web" "main Svelte asset"
if [ ! -f "$packaged_asset" ] || [ -L "$packaged_asset" ]; then
  fail "main Svelte asset is not a regular immutable file"
fi

application_sha256_before="$(sha256sum "$application" | cut -d ' ' -f 1)"
index_sha256="$(sha256sum "$packaged_index" | cut -d ' ' -f 1)"
asset_sha256="$(sha256sum "$packaged_asset" | cut -d ' ' -f 1)"

allocate_port() {
  local port
  port="$("$project_python" -I -S -c \
    'import socket; sock = socket.socket(); sock.bind(("127.0.0.1", 0)); print(sock.getsockname()[1]); sock.close()')"
  if [[ ! "$port" =~ ^[0-9]+$ ]] \
    || [ "$port" -lt 1 ] \
    || [ "$port" -gt 65535 ]; then
    fail "project Python returned an invalid ephemeral port"
  fi
  printf '%s\n' "$port"
}

prepare_mode_root() {
  local root="$1"
  mkdir -p \
    "$root/home" \
    "$root/config" \
    "$root/data" \
    "$root/cache" \
    "$root/state" \
    "$root/runtime" \
    "$root/tmp" \
    "$root/appdata" \
    "$root/localappdata"
  chmod 0700 "$root" "$root/runtime"
}

capture_resource_snapshot_inventory() {
  local root="$1"
  local output="$2"
  local temporary_root="$root/tmp"
  if [ ! -d "$temporary_root" ] || [ -L "$temporary_root" ]; then
    fail "private resource snapshot temporary root is missing or indirect"
  fi
  local canonical_temporary_root
  canonical_temporary_root="$(readlink -f -- "$temporary_root")"
  : >"$output"
  local snapshot
  local canonical_snapshot
  while IFS= read -r snapshot; do
    if [ ! -d "$snapshot" ] || [ -L "$snapshot" ]; then
      fail "private resource snapshot inventory contains a non-directory or symlink"
    fi
    canonical_snapshot="$(readlink -f -- "$snapshot")"
    if [ "$(dirname -- "$canonical_snapshot")" != "$canonical_temporary_root" ] \
      || [ "$canonical_snapshot" != "$snapshot" ]; then
      fail "private resource snapshot escaped its canonical temporary root"
    fi
    printf '%s\n' "${snapshot##*/}" >>"$output"
  done < <(
    find "$canonical_temporary_root" \
      -mindepth 1 -maxdepth 1 -name 'pokecon-verified-resources-*' -print \
      | LC_ALL=C sort
  )
}

launch_mode() {
  local mode="$1"
  local root="$2"
  local port="$3"
  local pid_file="$root/application.pid"
  local status_file="$root/application.status"
  local identity_file="$root/application.identity"
  local display_file="$root/display"
  local xauthority_record="$root/xauthority"
  local combined_file="$root/application.log"
  prepare_mode_root "$root"
  reset_primary_identity_cache
  reset_active_supervisor_identity_cache

  if [ "$mode" = web ]; then
    "$setsid_executable" --fork --wait \
      "$bash_binary" "$script_path" __launch_product \
      "$mode" "$root" "$application" "$port" "$child_path" \
      "$pid_file" "$status_file" "$display_file" "$xauthority_record" \
      "$root/runtime" "$canonical_mesa_renderer" "$identity_file" \
      >"$combined_file" 2>&1 &
  else
    local desktop_auth="$root/Xauthority"
    "$setsid_executable" --fork --wait \
      dbus-run-session --config-file="$canonical_dbus_session_config" -- \
      xvfb-run \
      --auto-servernum \
      --auth-file="$desktop_auth" \
      --error-file="$root/xvfb.stderr" \
      --server-args="-screen 0 1920x1080x24 -nolisten tcp" \
      "$bash_binary" "$script_path" __launch_product \
      "$mode" "$root" "$application" "$port" "$child_path" \
      "$pid_file" "$status_file" "$display_file" "$xauthority_record" \
      "$root/runtime" "$canonical_mesa_renderer" "$identity_file" \
      >"$combined_file" 2>&1 &
  fi
  local spawned_supervisor=$!
  local captured_supervisor
  local captured_supervisor_pgid
  local captured_supervisor_start_ticks
  local captured_supervisor_executable
  if ! capture_expected_process_identity \
    "$spawned_supervisor" "$setsid_executable" "$mode supervisor" \
    captured_supervisor captured_supervisor_pgid \
    captured_supervisor_start_ticks captured_supervisor_executable 100; then
    fail "$mode supervisor identity could not be captured safely"
  fi
  active_supervisor="$captured_supervisor"
  active_supervisor_pgid="$captured_supervisor_pgid"
  active_supervisor_start_ticks="$captured_supervisor_start_ticks"
  active_supervisor_executable="$captured_supervisor_executable"

  for _attempt in {1..300}; do
    if [ -s "$pid_file" ]; then
      break
    fi
    if ! kill -0 "$active_supervisor" 2>/dev/null; then
      fail "$mode supervisor stopped before publishing the application PID"
    fi
    sleep 0.1
  done
  if [ ! -s "$pid_file" ]; then
    fail "$mode application PID was not published before the startup deadline"
  fi
  if [ ! -f "$identity_file" ] \
    || [ -L "$identity_file" ] \
    || [ "$(stat -c '%a' -- "$identity_file")" != 600 ]; then
    fail "$mode application identity record is missing, indirect, or non-private"
  fi
  local -a published_identity=()
  mapfile -t published_identity <"$identity_file"
  if [ "${#published_identity[@]}" -ne 5 ] \
    || [[ "${published_identity[0]}" != pid=* ]] \
    || [[ "${published_identity[1]}" != state=* ]] \
    || [[ "${published_identity[2]}" != pgid=* ]] \
    || [[ "${published_identity[3]}" != start_ticks=* ]] \
    || [[ "${published_identity[4]}" != executable=* ]]; then
    fail "$mode application identity record has an invalid schema"
  fi
  local published_pid="${published_identity[0]#pid=}"
  local published_state="${published_identity[1]#state=}"
  local published_pgid="${published_identity[2]#pgid=}"
  local published_start_ticks="${published_identity[3]#start_ticks=}"
  local published_executable="${published_identity[4]#executable=}"
  local pid_record
  pid_record="$(head -n 1 -- "$pid_file")"
  if [[ ! "$published_pid" =~ ^[0-9]+$ ]] \
    || [ "$published_pid" -le 1 ] \
    || [ "$pid_record" != "$published_pid" ]; then
    fail "$mode published an invalid application PID"
  fi
  if [[ ! "$published_state" =~ ^[A-Za-z]$ ]]; then
    fail "$mode published an invalid application state"
  fi
  case "$published_state" in
    X | x | Z) fail "$mode published a dead application state" ;;
  esac
  if [[ ! "$published_pgid" =~ ^[0-9]+$ ]] \
    || [ "$published_pgid" -le 1 ] \
    || [[ ! "$published_start_ticks" =~ ^[0-9]+$ ]] \
    || [ "$published_start_ticks" -le 0 ] \
    || [ "$published_executable" != "$application" ]; then
    fail "$mode published an invalid application identity"
  fi
  local live_pid
  local live_state
  local live_pgid
  local live_start_ticks
  local live_executable
  if ! read_process_identity \
    "$published_pid" \
    live_pid live_state live_pgid live_start_ticks live_executable; then
    fail "$mode published application identity is not live"
  fi
  if [ "$live_pid" != "$published_pid" ] \
    || [ "$live_pgid" != "$published_pgid" ] \
    || [ "$live_start_ticks" != "$published_start_ticks" ] \
    || [ "$live_executable" != "$published_executable" ] \
    || [ "$live_executable" != "$application" ]; then
    fail "$mode published application identity differs from the live process"
  fi
  local gate_pgid
  gate_pgid="$(ps -o pgid= -p "$$" | tr -d '[:space:]')"
  if [[ ! "$gate_pgid" =~ ^[0-9]+$ ]] \
    || [ "$gate_pgid" -le 1 ] \
    || [ "$live_pgid" = "$gate_pgid" ]; then
    fail "$mode application is not contained in a private process group"
  fi
  active_pid="$live_pid"
  active_pgid="$live_pgid"
  active_start_ticks="$live_start_ticks"
  active_executable="$live_executable"
  local leader_pid
  local leader_state
  local leader_pgid
  local leader_start_ticks
  local leader_executable
  local leader_captured=false
  for _attempt in {1..100}; do
    if active_identity_matches \
      && read_process_identity \
        "$active_pgid" \
        leader_pid leader_state leader_pgid leader_start_ticks \
        leader_executable \
      && [ -n "$leader_state" ] \
      && [ "$leader_pid" = "$active_pgid" ] \
      && [ "$leader_pgid" = "$active_pgid" ]; then
      leader_captured=true
      break
    fi
    sleep 0.05
  done
  if [ "$leader_captured" != true ]; then
    fail "$mode private process-group leader identity could not be captured safely"
  fi
  active_group_leader="$leader_pid"
  active_group_leader_pgid="$leader_pgid"
  active_group_leader_start_ticks="$leader_start_ticks"
  active_group_leader_executable="$leader_executable"
  printf '%s\n' "$active_pgid" >"$root/application.pgid"
}

fetch() {
  local port="$1"
  local path="$2"
  local prefix="$3"
  local root="$4"
  set +e
  fetch_code="$(curl \
    --silent \
    --show-error \
    --noproxy '*' \
    --connect-timeout 1 \
    --max-time 3 \
    --dump-header "$root/$prefix.headers" \
    --output "$root/$prefix.body" \
    --write-out '%{http_code}' \
    "http://127.0.0.1:$port$path" 2>"$root/$prefix.curl.stderr")"
  fetch_status=$?
  set -e
  fetch_body="$root/$prefix.body"
  fetch_headers="$root/$prefix.headers"
}

require_json_response() {
  local expected_status="$1"
  local label="$2"
  if [ "$fetch_status" -ne 0 ] || [ "$fetch_code" != "$expected_status" ]; then
    fail "$label returned curl status $fetch_status and HTTP $fetch_code"
  fi
  if ! grep -iE '^content-type:[[:space:]]*application/json([;[:space:]]|$)' \
    "$fetch_headers" >/dev/null; then
    fail "$label did not return an application/json content type"
  fi
  if ! jq -e . "$fetch_body" >/dev/null; then
    fail "$label did not return valid JSON"
  fi
}

fetch_openapi_method_probe() {
  if [ "$#" -ne 6 ]; then
    return 2
  fi
  local port="$1"
  local method="$2"
  local path="$3"
  local preflight_for="$4"
  local prefix="$5"
  local root="$6"
  local -a request_arguments=()
  case "$method" in
    get)
      request_arguments=(
        --request GET
        --header "Origin: http://127.0.0.1:$port"
      )
      ;;
    head)
      request_arguments=(
        --request HEAD
        --ignore-content-length
        --header 'Connection: close'
        --header "Origin: http://127.0.0.1:$port"
      )
      ;;
    post | put | patch | delete)
      request_arguments=(
        --request "${method^^}"
        --header "Origin: http://127.0.0.1:$port"
        --header 'Content-Type: application/json'
        --header 'X-Pokecon-Request: 1'
        --data-binary '{'
      )
      ;;
    options)
      request_arguments=(
        --request OPTIONS
        --header "Origin: http://localhost:$port"
        --header "Access-Control-Request-Method: ${preflight_for^^}"
      )
      case "$preflight_for" in
        post | patch)
          request_arguments+=(
            --header 'Access-Control-Request-Headers: content-type,x-pokecon-request'
          )
          ;;
      esac
      if [ "$path:$preflight_for" = /api/state:connect ]; then
        request_arguments+=(
          --header 'X-Pokecon-Preflight-Method: GET'
        )
      fi
      ;;
    trace | connect)
      request_arguments=(
        --request "${method^^}"
        --header "Origin: http://127.0.0.1:$port"
      )
      ;;
    *)
      fail "method probe escaped the fixed standard matrix: $method $path"
      ;;
  esac
  set +e
  fetch_code="$(curl \
    --silent \
    --show-error \
    --globoff \
    --noproxy '*' \
    --connect-timeout 1 \
    --max-time 3 \
    --dump-header "$root/$prefix.headers" \
    --output "$root/$prefix.body" \
    --write-out '%{http_code}' \
    "${request_arguments[@]}" \
    "http://127.0.0.1:$port$path" 2>"$root/$prefix.curl.stderr")"
  fetch_status=$?
  set -e
  fetch_body="$root/$prefix.body"
  fetch_headers="$root/$prefix.headers"
}

require_head_method_rejection() {
  if [ "$#" -ne 1 ]; then
    return 2
  fi
  local label="$1"
  if [ "$fetch_status" -ne 0 ] || [ "$fetch_code" != 405 ]; then
    fail "$label returned curl status $fetch_status and HTTP $fetch_code"
  fi
  require_single_exact_header Content-Type application/json "$label"
  require_single_positive_decimal_header Content-Length "$label"
  if [ -s "$fetch_body" ]; then
    fail "$label returned response bytes after the HEAD headers"
  fi
}

require_single_exact_header() {
  if [ "$#" -ne 3 ]; then
    return 2
  fi
  local expected_name="$1"
  local expected_value="$2"
  local label="$3"
  local -a observed_values=()
  local header_line
  while IFS= read -r header_line || [ -n "$header_line" ]; do
    header_line="${header_line%$'\r'}"
    if [[ "$header_line" == *:* ]]; then
      local observed_name="${header_line%%:*}"
      if [ "${observed_name,,}" = "${expected_name,,}" ]; then
        local observed_value="${header_line#*:}"
        while [[ "$observed_value" == [[:space:]]* ]]; do
          observed_value="${observed_value:1}"
        done
        observed_values+=("$observed_value")
      fi
    fi
  done <"$fetch_headers"
  if [ "${#observed_values[@]}" -ne 1 ] \
    || [ "${observed_values[0]}" != "$expected_value" ]; then
    fail "$label did not return exactly one $expected_name header with value $expected_value"
  fi
}

require_single_positive_decimal_header() {
  if [ "$#" -ne 2 ]; then
    return 2
  fi
  local expected_name="$1"
  local label="$2"
  local -a observed_values=()
  local header_line
  while IFS= read -r header_line || [ -n "$header_line" ]; do
    header_line="${header_line%$'\r'}"
    if [[ "$header_line" == *:* ]]; then
      local observed_name="${header_line%%:*}"
      if [ "${observed_name,,}" = "${expected_name,,}" ]; then
        local observed_value="${header_line#*:}"
        while [[ "$observed_value" == [[:space:]]* ]]; do
          observed_value="${observed_value:1}"
        done
        observed_values+=("$observed_value")
      fi
    fi
  done <"$fetch_headers"
  if [ "${#observed_values[@]}" -ne 1 ] \
    || [[ ! "${observed_values[0]}" =~ ^[1-9][0-9]*$ ]]; then
    fail "$label did not return exactly one $expected_name header with a strict positive base-10 integer value"
  fi
}

require_cors_preflight() {
  if [ "$#" -ne 2 ]; then
    return 2
  fi
  local port="$1"
  local label="$2"
  if [ "$fetch_status" -ne 0 ] || [ "$fetch_code" != 204 ]; then
    fail "$label returned curl status $fetch_status and HTTP $fetch_code"
  fi
  if [ -s "$fetch_body" ]; then
    fail "$label returned a body"
  fi
  require_single_exact_header \
    Access-Control-Allow-Origin "http://localhost:$port" "$label"
  require_single_exact_header \
    Access-Control-Allow-Methods "GET, PATCH, POST, OPTIONS" "$label"
  require_single_exact_header \
    Access-Control-Allow-Headers "Content-Type, X-Pokecon-Request" "$label"
  require_single_exact_header Vary Origin "$label"
  local forbidden_name
  for forbidden_name in "${cors_success_forbidden_response_headers[@]}"; do
    require_absent_response_header "$forbidden_name" "$label"
  done
}
readonly -f require_cors_preflight

require_forbidden_preflight() {
  if [ "$#" -ne 1 ]; then
    return 2
  fi
  local label="$1"
  if [ "$fetch_status" -ne 0 ] || [ "$fetch_code" != 403 ]; then
    fail "$label returned curl status $fetch_status and HTTP $fetch_code"
  fi
  require_single_exact_header Content-Type application/json "$label"
  if ! jq -e . "$fetch_body" >/dev/null 2>&1; then
    fail "$label did not return valid JSON"
  fi
  if ! validate_unique_json_object_keys "$fetch_body" 2>/dev/null; then
    fail "$label did not return exactly one JSON document with unique object keys"
  fi
  if ! jq -e '
    keys == ["error"]
    and (.error | keys) == ["code", "fields", "message"]
    and .error.code == "request_forbidden"
    and .error.fields == null
    and .error.message == "request validation failed"
  ' "$fetch_body" >/dev/null; then
    fail "$label did not return the canonical request_forbidden rejection"
  fi
  local forbidden_name
  for forbidden_name in "${cors_rejection_forbidden_response_headers[@]}"; do
    require_absent_response_header "$forbidden_name" "$label"
  done
}
readonly -f require_forbidden_preflight

require_exact_allow_header() {
  if [ "$#" -ne 2 ]; then
    return 2
  fi
  require_single_exact_header Allow "$1" "$2"
}

probe_cors_preflight_policy() {
  if [ "$#" -ne 3 ]; then
    return 2
  fi
  local mode="$1"
  local port="$2"
  local root="$3"
  local pairs_tsv="$root/cors-preflight-advertised-pairs.tsv"
  local results_jsonl="$root/cors-preflight-policy-results.jsonl"
  local results="$root/cors-preflight-policy-results.json"
  if ! jq -r '
    sort_by([.path, .method])[]
    | [.path, .method, .operation_id]
    | @tsv
  ' "$openapi_operation_inventory" >"$pairs_tsv"; then
    fail "$mode could not materialize the advertised CORS preflight pairs"
  fi

  : >"$results_jsonl"
  local advertised_pair_count=0
  local path
  local requested_method
  local operation_id
  while IFS=$'\t' read -r path requested_method operation_id; do
    ((advertised_pair_count += 1))
    local prefix
    printf -v prefix 'cors-preflight-advertised-%03d' "$advertised_pair_count"
    fetch_openapi_method_probe \
      "$port" options "$path" "$requested_method" "$prefix" "$root"
    require_cors_preflight \
      "$port" "$mode advertised CORS preflight OPTIONS $path for ${requested_method^^}"
    jq -cnS \
      --arg classification advertised_pair \
      --arg operation_id "$operation_id" \
      --arg path "$path" \
      --arg requested_method "$requested_method" \
      --arg response_class cors_preflight \
      --argjson status "$fetch_code" \
      '{
        classification: $classification,
        operation_id: $operation_id,
        path: $path,
        requested_method: $requested_method,
        response_class: $response_class,
        status: $status
      }' >>"$results_jsonl"
  done <"$pairs_tsv"
  if [ "$advertised_pair_count" -ne "$openapi_operation_count" ]; then
    fail "$mode did not probe every advertised CORS preflight pair"
  fi

  local -a known_wrong_preflights=(
    '/api/settings:post'
    '/api/camera/retry:get'
    '/api/state:head'
    '/api/settings:put'
    '/api/state:delete'
    '/api/state:connect'
  )
  local known_wrong_count=0
  local body_sha256
  local probe
  for probe in "${known_wrong_preflights[@]}"; do
    ((known_wrong_count += 1))
    path="${probe%:*}"
    requested_method="${probe##*:}"
    local prefix
    printf -v prefix 'cors-preflight-known-wrong-%03d' "$known_wrong_count"
    fetch_openapi_method_probe \
      "$port" options "$path" "$requested_method" "$prefix" "$root"
    require_forbidden_preflight \
      "$mode unadvertised CORS preflight OPTIONS $path for ${requested_method^^}"
    body_sha256="$(sha256sum "$fetch_body" | cut -d ' ' -f 1)"
    jq -cnS \
      --slurpfile response "$fetch_body" \
      --arg body_sha256 "$body_sha256" \
      --arg classification known_path_wrong_method \
      --arg content_type application/json \
      --arg path "$path" \
      --arg requested_method "$requested_method" \
      --arg response_class request_forbidden \
      --argjson forbidden_response_headers_absent \
      "$cors_rejection_forbidden_response_headers_json" \
      --argjson status "$fetch_code" \
      '{
        body_sha256: $body_sha256,
        classification: $classification,
        content_type: $content_type,
        error: $response[0].error,
        forbidden_response_headers_absent: $forbidden_response_headers_absent,
        operation_id: null,
        path: $path,
        requested_method: $requested_method,
        response_class: $response_class,
        status: $status
      }' >>"$results_jsonl"
  done

  path="/api/not-in-openapi"
  requested_method="get"
  fetch_openapi_method_probe \
    "$port" options "$path" "$requested_method" \
    cors-preflight-unknown-path "$root"
  require_forbidden_preflight \
    "$mode unknown-path CORS preflight OPTIONS $path for ${requested_method^^}"
  body_sha256="$(sha256sum "$fetch_body" | cut -d ' ' -f 1)"
  jq -cnS \
    --slurpfile response "$fetch_body" \
    --arg body_sha256 "$body_sha256" \
    --arg classification unknown_path \
    --arg content_type application/json \
    --arg path "$path" \
    --arg requested_method "$requested_method" \
    --arg response_class request_forbidden \
    --argjson forbidden_response_headers_absent \
    "$cors_rejection_forbidden_response_headers_json" \
    --argjson status "$fetch_code" \
    '{
      body_sha256: $body_sha256,
      classification: $classification,
      content_type: $content_type,
      error: $response[0].error,
      forbidden_response_headers_absent: $forbidden_response_headers_absent,
      operation_id: null,
      path: $path,
      requested_method: $requested_method,
      response_class: $response_class,
      status: $status
    }' >>"$results_jsonl"

  if ! jq -e -S -s \
    --slurpfile advertised "$openapi_operation_inventory" \
    --argjson advertised_count "$openapi_operation_count" \
    --argjson forbidden_response_headers_absent \
    "$cors_rejection_forbidden_response_headers_json" \
    --argjson known_wrong_count "$known_wrong_count" '
      sort_by([.path, .requested_method, .classification])
      | . as $results
      | if (
          length == ($advertised_count + $known_wrong_count + 1)
          and length
            == (unique_by([.path, .requested_method, .classification]) | length)
          and ([$results[] | select(.classification == "advertised_pair")
            | {method: .requested_method, operation_id, path}]
            | sort_by([.path, .method]))
            == ($advertised[0] | sort_by([.path, .method]))
          and ([$results[]
            | select(.classification == "known_path_wrong_method")
            | {path, requested_method}] | sort_by([.path, .requested_method]))
            == ([
              {path: "/api/settings", requested_method: "post"},
              {path: "/api/camera/retry", requested_method: "get"},
              {path: "/api/state", requested_method: "head"},
              {path: "/api/settings", requested_method: "put"},
              {path: "/api/state", requested_method: "delete"},
              {path: "/api/state", requested_method: "connect"}
            ] | sort_by([.path, .requested_method]))
          and ([$results[] | select(.classification == "unknown_path")]
            | length) == 1
          and ([$results[] | select(.classification != "advertised_pair")
            | .body_sha256] | unique | length) == 1
          and all($results[];
            if .classification == "advertised_pair" then
              keys == [
                "classification", "operation_id", "path", "requested_method",
                "response_class", "status"
              ]
              and (.operation_id | type) == "string"
              and (.operation_id | length) > 0
              and (.requested_method == "get"
                or .requested_method == "patch"
                or .requested_method == "post")
              and .response_class == "cors_preflight"
              and .status == 204
            elif (.classification == "known_path_wrong_method"
              or .classification == "unknown_path") then
              keys == [
                "body_sha256", "classification", "content_type", "error",
                "forbidden_response_headers_absent", "operation_id", "path",
                "requested_method", "response_class", "status"
              ]
              and (.body_sha256 | test("^[0-9a-f]{64}$"))
              and .content_type == "application/json"
              and .error == {
                code: "request_forbidden",
                fields: null,
                message: "request validation failed"
              }
              and .forbidden_response_headers_absent
                == $forbidden_response_headers_absent
              and .operation_id == null
              and .response_class == "request_forbidden"
              and .status == 403
              and if .classification == "known_path_wrong_method" then
                (.path | startswith("/api/"))
              else
                .path == "/api/not-in-openapi"
                and .requested_method == "get"
              end
            else false
            end
          )
        ) then .
        else error("closed-world CORS preflight results are incomplete or malformed")
        end
    ' "$results_jsonl" >"$results"; then
    fail "$mode closed-world CORS preflight evidence is incomplete or malformed"
  fi
}

probe_openapi_method_matrix() {
  if [ "$#" -ne 3 ]; then
    return 2
  fi
  local mode="$1"
  local port="$2"
  local root="$3"
  local paths_tsv="$root/openapi-method-paths.tsv"
  local results_jsonl="$root/openapi-method-results.jsonl"
  local results="$root/openapi-method-results.json"
  if ! jq -r '
    group_by(.path)[]
    | (map(.method) | sort) as $methods
    | [
        .[0].path,
        ($methods | join(",")),
        (
          if ($methods | index("patch")) != null then "patch"
          elif ($methods | index("post")) != null then "post"
          else "get"
          end
        )
      ]
    | @tsv
  ' \
    "$openapi_operation_inventory" >"$paths_tsv"; then
    fail "$mode could not materialize the canonical OpenAPI path inventory"
  fi
  : >"$results_jsonl"
  local path_count=0
  local probe_count=0
  local advertised_probe_count=0
  local rejected_probe_count=0
  local preflight_probe_count=0
  local path
  local advertised_methods
  local preflight_for
  while IFS=$'\t' read -r path advertised_methods preflight_for; do
    ((path_count += 1))
    local method
    for method in "${openapi_method_matrix[@]}"; do
      ((probe_count += 1))
      local prefix
      printf -v prefix 'openapi-method-%03d' "$probe_count"
      fetch_openapi_method_probe \
        "$port" "$method" "$path" "$preflight_for" "$prefix" "$root"

      local classification
      local result_allow=
      local operation_id=
      local result_preflight_for=
      local response_class
      if [ "$method" = options ]; then
        classification=cors_preflight
        result_preflight_for="$preflight_for"
        response_class=cors_preflight
        ((preflight_probe_count += 1))
        require_cors_preflight \
          "$port" "$mode CORS preflight OPTIONS $path for ${preflight_for^^}"
      elif [[ ",$advertised_methods," = *",$method,"* ]]; then
        classification=advertised
        ((advertised_probe_count += 1))
        operation_id="$(
          jq -er --arg path "$path" --arg method "$method" '
            first(.[] | select(.path == $path and .method == $method))
            | .operation_id
          ' "$openapi_operation_inventory"
        )"
        local expected_status
        local expected_response_class
        case "$method:$path" in
          get:/ws)
            expected_status=400
            expected_response_class=invalid_request
            ;;
          get:/api/*)
            expected_status=200
            expected_response_class=success
            ;;
          post:/api/* | patch:/api/*)
            expected_status=400
            expected_response_class=malformed_json
            ;;
          *)
            fail "$mode encountered an unsupported advertised operation: $method $path"
            ;;
        esac
        require_json_response \
          "$expected_status" "$mode advertised operation $method $path"
        if [ "$expected_response_class" = success ]; then
          if ! jq -e 'keys == ["data"]' "$fetch_body" >/dev/null; then
            fail "$mode advertised GET operation returned the wrong JSON class: $path"
          fi
          response_class=success
        else
          if ! jq -e --arg expected_code "$expected_response_class" '
            keys == ["error"]
            and (.error | keys) == ["code", "fields", "message"]
            and .error.code == $expected_code
            and .error.fields == null
            and (.error.message | type) == "string"
            and (.error.message | length) > 0
          ' "$fetch_body" >/dev/null; then
            fail "$mode advertised operation returned the wrong rejection class: $method $path"
          fi
          response_class="$(jq -er '.error.code' "$fetch_body")"
        fi
      else
        classification=rejected
        ((rejected_probe_count += 1))
        result_allow="${advertised_methods^^}"
        result_allow="${result_allow//,/, }"
        if [ "$method" = head ]; then
          require_head_method_rejection \
            "$mode unadvertised method HEAD $path"
          response_class=method_not_allowed_head
        else
          require_json_response 405 \
            "$mode unadvertised method ${method^^} $path"
          if ! jq -e '
            keys == ["error"]
            and (.error | keys) == ["code", "fields", "message"]
            and .error.code == "method_not_allowed"
            and .error.fields == null
            and (.error.message | type) == "string"
            and (.error.message | length) > 0
          ' "$fetch_body" >/dev/null; then
            fail "$mode unadvertised method did not return the canonical rejection: $method $path"
          fi
          response_class=method_not_allowed
        fi
        require_exact_allow_header \
          "$result_allow" "$mode unadvertised method ${method^^} $path"
      fi

      jq -cnS \
        --arg allow "$result_allow" \
        --arg classification "$classification" \
        --arg method "$method" \
        --arg operation_id "$operation_id" \
        --arg path "$path" \
        --arg preflight_for "$result_preflight_for" \
        --arg response_class "$response_class" \
        --argjson status "$fetch_code" \
        '{
          allow: (if $allow == "" then null else $allow end),
          classification: $classification,
          method: $method,
          operation_id: (
            if $operation_id == "" then null else $operation_id end
          ),
          path: $path,
          preflight_for: (
            if $preflight_for == "" then null else $preflight_for end
          ),
          response_class: $response_class,
          status: $status
        }' >>"$results_jsonl"
    done
  done <"$paths_tsv"
  if [ "$path_count" -ne "$openapi_path_count" ] \
    || [ "$probe_count" -ne "$openapi_method_probe_count" ] \
    || [ "$advertised_probe_count" -ne "$openapi_operation_count" ] \
    || [ "$rejected_probe_count" -ne "$openapi_rejected_method_count" ] \
    || [ "$preflight_probe_count" -ne "$openapi_cors_preflight_count" ]; then
    fail "$mode did not execute the complete exact OpenAPI method matrix"
  fi
  if ! jq -e -S -s \
    --slurpfile advertised "$openapi_operation_inventory" \
    --argjson advertised_count "$openapi_operation_count" \
    --argjson expected_count "$openapi_method_probe_count" \
    --argjson path_count "$openapi_path_count" \
    --argjson preflight_count "$openapi_cors_preflight_count" \
    --argjson rejected_count "$openapi_rejected_method_count" '
      sort_by([.path, .method])
      | if (
          length == $expected_count
          and length == (unique_by([.path, .method]) | length)
          and ([.[].path] | unique | length) == $path_count
          and ([.[].method] | unique) == [
            "connect", "delete", "get", "head", "options", "patch", "post", "put",
            "trace"
          ]
          and ([.[] | select(.classification == "advertised")
            | {method, operation_id, path}] == $advertised[0])
          and ([.[] | select(.classification == "advertised")] | length)
            == $advertised_count
          and ([.[] | select(.classification == "rejected")] | length)
            == $rejected_count
          and ([.[] | select(.classification == "cors_preflight")] | length)
            == $preflight_count
          and all(.[];
            keys == [
              "allow", "classification", "method", "operation_id", "path",
              "preflight_for", "response_class", "status"
            ]
            and if .classification == "advertised" then
              .allow == null
              and .operation_id != null
              and .preflight_for == null
              and if .path == "/ws" then
                .method == "get"
                and .status == 400
                and .response_class == "invalid_request"
              elif .method == "get" then
                (.path | startswith("/api/"))
                and .status == 200
                and .response_class == "success"
              else
                (.method == "post" or .method == "patch")
                and (.path | startswith("/api/"))
                and .status == 400
                and .response_class == "malformed_json"
              end
            elif .classification == "cors_preflight" then
              . as $result
              | $result.allow == null
                and $result.method == "options"
                and $result.operation_id == null
                and ($result.preflight_for == "get"
                  or $result.preflight_for == "post"
                  or $result.preflight_for == "patch")
                and ([ $advertised[0][]
                  | select(.path == $result.path and .method == $result.preflight_for)
                ] | length) == 1
                and $result.status == 204
                and $result.response_class == "cors_preflight"
            elif .classification == "rejected" then
              . as $result
              | $result.operation_id == null
                and $result.preflight_for == null
                and $result.method != "options"
                and ([ $advertised[0][]
                  | select(.path == $result.path and .method == $result.method)
                ] | length) == 0
                and $result.status == 405
                and $result.allow == ([
                  $advertised[0][]
                  | select(.path == $result.path)
                  | .method
                  | ascii_upcase
                ] | sort | join(", "))
                and if $result.method == "head" then
                  $result.response_class == "method_not_allowed_head"
                else
                  $result.response_class == "method_not_allowed"
                end
            else false
            end
          )
        ) then .
        else error("OpenAPI exact method results are incomplete or malformed")
        end
    ' "$results_jsonl" >"$results"; then
    fail "$mode OpenAPI exact method-result inventory is incomplete or malformed"
  fi
}

wait_for_readiness() {
  local mode="$1"
  local port="$2"
  local root="$3"
  local ready=false
  for _attempt in {1..600}; do
    fetch "$port" /api/state readiness "$root"
    if [ "$fetch_status" -eq 0 ] \
      && [ "$fetch_code" = 200 ] \
      && jq -e --argjson pid "$active_pid" \
        'keys == ["data"] and .data.pid == $pid' \
        "$fetch_body" >/dev/null 2>&1; then
      ready=true
      break
    fi
    if ! kill -0 "$active_pid" 2>/dev/null; then
      fail "$mode application stopped before API readiness"
    fi
    sleep 0.1
  done
  if [ "$ready" != true ]; then
    fail "$mode API did not become ready before the startup deadline"
  fi
  require_json_response 200 "$mode readiness endpoint"
}

fetch_unknown_api_options_probe() {
  if [ "$#" -ne 5 ]; then
    return 2
  fi
  local port="$1"
  local path="$2"
  local request_variant="$3"
  local prefix="$4"
  local root="$5"
  local -a request_arguments=(--request OPTIONS)
  case "$request_variant" in
    bare) ;;
    origin_only)
      request_arguments+=(--header "Origin: http://127.0.0.1:$port")
      ;;
    requested_method_only)
      request_arguments+=(--header 'Access-Control-Request-Method: GET')
      ;;
    *)
      fail "unknown API OPTIONS probe escaped its fixed boundary variants: $request_variant"
      ;;
  esac
  set +e
  fetch_code="$(curl \
    --silent \
    --show-error \
    --globoff \
    --noproxy '*' \
    --connect-timeout 1 \
    --max-time 3 \
    --dump-header "$root/$prefix.headers" \
    --output "$root/$prefix.body" \
    --write-out '%{http_code}' \
    "${request_arguments[@]}" \
    "http://127.0.0.1:$port$path" 2>"$root/$prefix.curl.stderr")"
  fetch_status=$?
  set -e
  fetch_body="$root/$prefix.body"
  fetch_headers="$root/$prefix.headers"
}

require_resource_not_found_response() {
  if [ "$#" -ne 1 ]; then
    return 2
  fi
  local label="$1"
  require_json_response 404 "$label"
  if ! jq -e '
    keys == ["error"]
    and (.error | keys) == ["code", "fields", "message"]
    and .error.code == "resource_not_found"
    and .error.fields == null
    and .error.message == "API resource was not found"
  ' "$fetch_body" >/dev/null; then
    fail "$label did not return the canonical resource_not_found rejection"
  fi
  if grep -iE '<(!doctype[[:space:]]+html|html([[:space:]>]))' \
    "$fetch_body" >/dev/null; then
    fail "$label leaked the packaged SPA document"
  fi
}

require_head_resource_not_found_response() {
  if [ "$#" -ne 2 ]; then
    return 2
  fi
  local expected_content_length="$1"
  local label="$2"
  if [[ ! "$expected_content_length" =~ ^[1-9][0-9]*$ ]]; then
    fail "$label did not receive a positive canonical content length"
  fi
  if [ "$fetch_status" -ne 0 ] || [ "$fetch_code" != 404 ]; then
    fail "$label returned curl status $fetch_status and HTTP $fetch_code"
  fi
  require_single_exact_header Content-Type application/json "$label"
  require_single_exact_header Content-Length "$expected_content_length" "$label"
  if [ -s "$fetch_body" ]; then
    fail "$label returned response bytes after the HEAD headers"
  fi
}

probe_unknown_api_boundary_matrix() {
  if [ "$#" -ne 3 ]; then
    return 2
  fi
  local mode="$1"
  local port="$2"
  local root="$3"
  local path="/api/not-in-openapi"
  local results_jsonl="$root/unknown-api-boundary-results.jsonl"
  local results="$root/unknown-api-boundary-results.json"
  local expected_routed_count="$((openapi_method_count - 1))"
  local expected_options_count=4
  local expected_probe_count="$((expected_routed_count + expected_options_count))"
  local routed_count=0
  local options_count=0
  local canonical_content_length=
  : >"$results_jsonl"

  local method
  for method in "${openapi_method_matrix[@]}"; do
    if [ "$method" = options ]; then
      continue
    fi
    ((routed_count += 1))
    local prefix
    printf -v prefix 'unknown-api-routed-%02d' "$routed_count"
    fetch_openapi_method_probe \
      "$port" "$method" "$path" get "$prefix" "$root"
    local response_class=resource_not_found
    local result_content_length
    if [ "$method" = head ]; then
      require_head_resource_not_found_response \
        "$canonical_content_length" "$mode unknown API method HEAD"
      response_class=resource_not_found_head
      result_content_length="$canonical_content_length"
    else
      require_resource_not_found_response \
        "$mode unknown API method ${method^^}"
      result_content_length="$(stat -c '%s' "$fetch_body")"
      if [ -z "$canonical_content_length" ]; then
        if [ "$method" != get ]; then
          fail "$mode did not establish the unknown API envelope with GET first"
        fi
        canonical_content_length="$result_content_length"
      elif [ "$result_content_length" != "$canonical_content_length" ]; then
        fail "$mode unknown API methods returned non-canonical envelope lengths"
      fi
    fi
    jq -cnS \
      --arg classification routed_not_found \
      --arg method "$method" \
      --arg path "$path" \
      --arg request_variant valid_boundary_headers \
      --arg response_class "$response_class" \
      --argjson content_length "$result_content_length" \
      --argjson status "$fetch_code" \
      '{
        classification: $classification,
        content_length: $content_length,
        method: $method,
        path: $path,
        request_variant: $request_variant,
        response_class: $response_class,
        status: $status
      }' >>"$results_jsonl"
  done
  if [ "$routed_count" -ne "$expected_routed_count" ]; then
    fail "$mode did not probe every routed unknown API method"
  fi

  local request_variant
  for request_variant in bare origin_only requested_method_only; do
    ((options_count += 1))
    local prefix
    printf -v prefix 'unknown-api-options-%02d' "$options_count"
    fetch_unknown_api_options_probe \
      "$port" "$path" "$request_variant" "$prefix" "$root"
    require_forbidden_preflight \
      "$mode unknown API OPTIONS boundary variant $request_variant"
    jq -cnS \
      --arg classification security_rejected_options \
      --arg method options \
      --arg path "$path" \
      --arg request_variant "$request_variant" \
      --arg response_class request_forbidden \
      --argjson status "$fetch_code" \
      '{
        classification: $classification,
        content_length: null,
        method: $method,
        path: $path,
        request_variant: $request_variant,
        response_class: $response_class,
        status: $status
      }' >>"$results_jsonl"
  done

  if ! jq -ceS '
    first(.[] | select(
      .classification == "unknown_path"
      and .path == "/api/not-in-openapi"
      and .requested_method == "get"
      and .response_class == "request_forbidden"
      and .status == 403
    ))
    | select(. != null)
    | {
        classification: "security_rejected_options",
        content_length: null,
        method: "options",
        path,
        request_variant: "complete_preflight",
        response_class,
        status
      }
  ' "$root/cors-preflight-policy-results.json" >>"$results_jsonl"; then
    fail "$mode could not reuse the complete unknown-path CORS preflight evidence"
  fi
  ((options_count += 1))
  if [ "$options_count" -ne "$expected_options_count" ]; then
    fail "$mode did not cover every unknown API OPTIONS boundary variant"
  fi

  if ! jq -e -S -s \
    --arg path "$path" \
    --argjson canonical_content_length "$canonical_content_length" \
    --argjson expected_options_count "$expected_options_count" \
    --argjson expected_probe_count "$expected_probe_count" \
    --argjson expected_routed_count "$expected_routed_count" '
      sort_by([.method, .request_variant])
      | if (
          length == $expected_probe_count
          and length == (unique_by([.method, .request_variant]) | length)
          and ([.[].method] | unique) == [
            "connect", "delete", "get", "head", "options", "patch", "post",
            "put", "trace"
          ]
          and ([.[] | select(.classification == "routed_not_found")] | length)
            == $expected_routed_count
          and ([.[] | select(.classification == "security_rejected_options")]
            | length) == $expected_options_count
          and ([.[]
            | select(.classification == "security_rejected_options")
            | .request_variant] | sort) == [
              "bare", "complete_preflight", "origin_only", "requested_method_only"
            ]
          and all(.[];
            keys == [
              "classification", "content_length", "method", "path",
              "request_variant", "response_class", "status"
            ]
            and .path == $path
            and if .classification == "routed_not_found" then
              .method != "options"
              and .request_variant == "valid_boundary_headers"
              and .status == 404
              and .content_length == $canonical_content_length
              and if .method == "head" then
                .response_class == "resource_not_found_head"
              else
                .response_class == "resource_not_found"
              end
            elif .classification == "security_rejected_options" then
              .method == "options"
              and .content_length == null
              and .response_class == "request_forbidden"
              and .status == 403
            else false
            end
          )
        ) then .
        else error("unknown API boundary results are incomplete or malformed")
        end
    ' "$results_jsonl" >"$results"; then
    fail "$mode unknown API boundary evidence is incomplete or malformed"
  fi
}

fetch_advertised_bare_options_probe() {
  if [ "$#" -ne 4 ]; then
    return 2
  fi
  local port="$1"
  local path="$2"
  local prefix="$3"
  local root="$4"
  set +e
  fetch_code="$(curl \
    --silent \
    --show-error \
    --globoff \
    --noproxy '*' \
    --connect-timeout 1 \
    --max-time 3 \
    --request OPTIONS \
    --dump-header "$root/$prefix.headers" \
    --output "$root/$prefix.body" \
    --write-out '%{http_code}' \
    "http://127.0.0.1:$port$path" 2>"$root/$prefix.curl.stderr")"
  fetch_status=$?
  set -e
  fetch_body="$root/$prefix.body"
  fetch_headers="$root/$prefix.headers"
}

require_absent_response_header() {
  if [ "$#" -ne 2 ]; then
    return 2
  fi
  local forbidden_name="$1"
  local label="$2"
  local header_line
  while IFS= read -r header_line || [ -n "$header_line" ]; do
    header_line="${header_line%$'\r'}"
    if [[ "$header_line" == *:* ]]; then
      local observed_name="${header_line%%:*}"
      if [ "${observed_name,,}" = "${forbidden_name,,}" ]; then
        fail "$label returned forbidden $forbidden_name response header"
      fi
    fi
  done <"$fetch_headers"
}

require_advertised_bare_options_rejection() {
  if [ "$#" -ne 1 ]; then
    return 2
  fi
  local label="$1"
  if [ "$fetch_status" -ne 0 ] || [ "$fetch_code" != 403 ]; then
    fail "$label returned curl status $fetch_status and HTTP $fetch_code"
  fi
  require_single_exact_header Content-Type application/json "$label"
  if [ ! -s "$fetch_body" ]; then
    fail "$label returned an empty response body"
  fi
  if ! jq -e . "$fetch_body" >/dev/null 2>&1; then
    fail "$label did not return valid JSON"
  fi
  if ! validate_unique_json_object_keys "$fetch_body" 2>/dev/null; then
    fail "$label did not return exactly one JSON document with unique object keys"
  fi
  if ! jq -e '
    keys == ["error"]
    and (.error | keys) == ["code", "fields", "message"]
    and .error.code == "request_forbidden"
    and .error.fields == null
    and .error.message == "request validation failed"
  ' "$fetch_body" >/dev/null; then
    fail "$label did not return the canonical request_forbidden rejection"
  fi
  local forbidden_name
  for forbidden_name in \
    "${advertised_bare_options_forbidden_response_headers[@]}"; do
    require_absent_response_header "$forbidden_name" "$label"
  done
}

probe_advertised_bare_options_security_boundary() {
  if [ "$#" -ne 3 ]; then
    return 2
  fi
  local mode="$1"
  local port="$2"
  local root="$3"
  local paths="$root/advertised-bare-options-paths.txt"
  local results_jsonl="$root/advertised-bare-options-results.jsonl"
  local results="$root/advertised-bare-options-results.json"
  if ! jq -er '[.[].path] | unique[]' \
    "$openapi_operation_inventory" >"$paths"; then
    fail "$mode could not derive the unique advertised OpenAPI paths"
  fi

  : >"$results_jsonl"
  local path_count=0
  local path
  while IFS= read -r path || [ -n "$path" ]; do
    ((path_count += 1))
    local prefix
    printf -v prefix 'advertised-bare-options-%03d' "$path_count"
    fetch_advertised_bare_options_probe "$port" "$path" "$prefix" "$root"
    require_advertised_bare_options_rejection \
      "$mode advertised bare OPTIONS $path"
    local path_kind
    case "$path" in
      /api/*) path_kind=rest ;;
      /ws) path_kind=websocket ;;
      *) fail "$mode advertised bare OPTIONS escaped the canonical public paths: $path" ;;
    esac
    local body_sha256
    body_sha256="$(sha256sum "$fetch_body" | cut -d ' ' -f 1)"
    jq -cnS \
      --slurpfile response "$fetch_body" \
      --arg body_sha256 "$body_sha256" \
      --argjson forbidden_response_headers_absent \
        "$advertised_bare_options_forbidden_response_headers_json" \
      --arg path "$path" \
      --arg path_kind "$path_kind" \
      --argjson status "$fetch_code" \
      '{
        access_control_request_method_header_sent: false,
        body_sha256: $body_sha256,
        classification: "advertised_path_security_rejected",
        content_type: "application/json",
        error: $response[0].error,
        forbidden_response_headers_absent: $forbidden_response_headers_absent,
        method: "options",
        origin_header_sent: false,
        path: $path,
        path_kind: $path_kind,
        request_body_bytes: 0,
        request_variant: "bare",
        response_class: "request_forbidden",
        status: $status
      }' >>"$results_jsonl"
  done <"$paths"
  if [ "$path_count" -ne "$openapi_path_count" ]; then
    fail "$mode did not probe every unique advertised path with bare OPTIONS"
  fi

  if ! jq -e -S -s \
    --slurpfile operations "$openapi_operation_inventory" \
    --argjson expected_count "$openapi_path_count" \
    --argjson expected_rest_count "$openapi_rest_path_count" \
    --argjson expected_websocket_count "$openapi_websocket_path_count" '
      sort_by(.path)
      | . as $results
      | ($operations[0] | map(.path) | unique) as $canonical_paths
      | if (
          length == $expected_count
          and length == (unique_by(.path) | length)
          and map(.path) == $canonical_paths
          and ([.[] | select(.path_kind == "rest")] | length)
            == $expected_rest_count
          and ([.[] | select(.path_kind == "websocket")] | length)
            == $expected_websocket_count
          and ([.[] | select(.path == "/ws")] | length) == 1
          and ([.[] | select(.path == "/api/settings")] | length) == 1
          and ([.[].body_sha256] | unique | length) == 1
          and all(.[];
            keys == [
              "access_control_request_method_header_sent", "body_sha256",
              "classification", "content_type", "error",
              "forbidden_response_headers_absent", "method",
              "origin_header_sent", "path", "path_kind",
              "request_body_bytes", "request_variant", "response_class", "status"
            ]
            and .access_control_request_method_header_sent == false
            and (.body_sha256 | test("^[0-9a-f]{64}$"))
            and .classification == "advertised_path_security_rejected"
            and .content_type == "application/json"
            and .error == {
              code: "request_forbidden",
              fields: null,
              message: "request validation failed"
            }
            and .forbidden_response_headers_absent == [
              "Access-Control-Allow-Headers", "Access-Control-Allow-Methods",
              "Access-Control-Allow-Origin", "Allow", "Vary"
            ]
            and .method == "options"
            and .origin_header_sent == false
            and .request_body_bytes == 0
            and .request_variant == "bare"
            and .response_class == "request_forbidden"
            and .status == 403
            and if .path_kind == "rest" then
              (.path | startswith("/api/"))
            elif .path_kind == "websocket" then
              .path == "/ws"
            else false
            end
          )
        ) then $results
        else error("advertised bare OPTIONS results are incomplete or malformed")
        end
    ' "$results_jsonl" >"$results"; then
    fail "$mode advertised bare OPTIONS security evidence is incomplete or malformed"
  fi
}

active_identity_matches() {
  if [[ ! "$active_pid" =~ ^[0-9]+$ ]] \
    || [ "$active_pid" -le 1 ] \
    || [[ ! "$active_pgid" =~ ^[0-9]+$ ]] \
    || [ "$active_pgid" -le 1 ] \
    || [[ ! "$active_start_ticks" =~ ^[0-9]+$ ]] \
    || [ "$active_start_ticks" -le 0 ] \
    || [ -z "$active_executable" ]; then
    return 1
  fi
  local live_pid
  local live_state
  local live_pgid
  local live_start_ticks
  local live_executable
  read_process_identity \
    "$active_pid" \
    live_pid live_state live_pgid live_start_ticks live_executable \
    || return 1
  case "$live_state" in
    X | x | Z) return 1 ;;
  esac
  [ "$live_pid" = "$active_pid" ] \
    && [ "$live_pgid" = "$active_pgid" ] \
    && [ "$live_start_ticks" = "$active_start_ticks" ] \
    && [ "$live_executable" = "$active_executable" ] \
    && [ "$live_executable" = "$application" ]
}

validate_process_identity() {
  local mode="$1"
  if ! active_identity_matches; then
    fail "$mode live process differs from its published application identity"
  fi
  require_contained_path "$active_executable" "$canonical_package" \
    "$mode live executable"
  if tr '\0' '\n' <"/proc/$active_pid/environ" \
    | grep -E '^(LD_LIBRARY_PATH|http_proxy|https_proxy|HTTP_PROXY|HTTPS_PROXY|ALL_PROXY)=' \
      >/dev/null; then
    fail "$mode application inherited a forbidden library or proxy override"
  fi
  local compositing_reexec_marker_count=0
  local environment_entry
  while IFS= read -r -d '' environment_entry; do
    case "$environment_entry" in
      PCME_DESKTOP_COMPOSITING_CONFIGURED=1)
        ((compositing_reexec_marker_count += 1))
        ;;
      PCME_DESKTOP_COMPOSITING_CONFIGURED=*)
        fail "$mode application inherited a noncanonical compositing reexec marker"
        ;;
    esac
  done <"/proc/$active_pid/environ"
  if [ "$mode" = desktop ]; then
    if [ "$compositing_reexec_marker_count" -ne 1 ]; then
      fail "desktop application did not preserve exactly one compositing reexec marker"
    fi
  elif [ "$compositing_reexec_marker_count" -ne 0 ]; then
    fail "web application unexpectedly inherited a compositing reexec marker"
  fi
}

validate_http_contract() {
  local mode="$1"
  local port="$2"
  local root="$3"

  if ! jq -e --argjson pid "$active_pid" '
    keys == ["data"]
    and (.data | keys) == [
      "active_profile", "available_profiles", "camera_device", "camera_fps",
      "camera_opened", "camera_resolution", "command_candidates",
      "command_display_cache_loading", "command_display_lists", "command_state",
      "current_command", "holding_buttons", "is_running", "last_input",
      "pending_profile", "pid", "revision", "serial_baud_rate",
      "serial_connected", "serial_port", "tags"
    ]
    and .data.pid == $pid
    and (.data.revision | type) == "string"
    and (.data.active_profile | type) == "string"
    and (.data.available_profiles | type) == "array"
    and (.data.command_candidates | type) == "array"
    and (.data.command_display_lists | type) == "object"
  ' "$root/state.json" >/dev/null; then
    fail "$mode state endpoint violates its canonical schema or key set"
  fi

  fetch "$port" /api/settings settings "$root"
  require_json_response 200 "$mode settings endpoint"
  cp "$fetch_body" "$root/settings.json"
  if ! jq -e '
    keys == ["data"]
    and (.data | keys) == [
      "apply_failures", "pending_restart_values", "restart_required", "revision", "values"
    ]
    and (.data.revision | type) == "string"
    and (.data.values | type) == "object"
    and (.data.values | length) == 78
    and (.data.pending_restart_values | type) == "object"
    and (.data.restart_required | type) == "array"
    and (.data.apply_failures | type) == "object"
  ' "$root/settings.json" >/dev/null; then
    fail "$mode settings endpoint violates its canonical schema or key set"
  fi

  local configured_port
  local configured_address
  local configured_web
  local configured_language
  local configured_compositing
  local configured_close_behavior
  configured_port="$(jq -er '.data.values["server.port"]' "$root/settings.json")"
  configured_address="$(
    jq -er '.data.values["server.bind_address"]' "$root/settings.json"
  )"
  configured_web="$(jq -er '.data.values["server.web_dir"]' "$root/settings.json")"
  configured_language="$(
    jq -er '.data.values.dynamic_config_language' "$root/settings.json"
  )"
  configured_compositing="$(
    jq -r '.data.values["ui.desktop.disable_compositing"]' "$root/settings.json"
  )"
  configured_close_behavior="$(
    jq -er '.data.values["ui.desktop.close_behavior"]' "$root/settings.json"
  )"
  if [ "$configured_port" != "$port" ] \
    || [ "$configured_address" != 127.0.0.1 ] \
    || [ "$configured_language" != none ] \
    || [ "$configured_compositing" != true ] \
    || [ "$configured_close_behavior" != keep_backend ]; then
    fail "$mode settings do not reflect the isolated canonical launch values"
  fi
  if [ "$configured_web" != "$canonical_package/bin/web/dist" ] \
    || [ "$(readlink -f -- "$configured_web")" != "$canonical_web" ]; then
    fail "$mode resolved a Web root outside the selected immutable package"
  fi

  probe_openapi_method_matrix "$mode" "$port" "$root"
  probe_cors_preflight_policy "$mode" "$port" "$root"
  probe_unknown_api_boundary_matrix "$mode" "$port" "$root"
  probe_advertised_bare_options_security_boundary "$mode" "$port" "$root"

  fetch "$port" /ui/ ui "$root"
  if [ "$fetch_status" -ne 0 ] || [ "$fetch_code" != 200 ]; then
    fail "$mode /ui/ returned curl status $fetch_status and HTTP $fetch_code"
  fi
  if ! grep -iE '^content-type:[[:space:]]*text/html([;[:space:]]|$)' \
    "$fetch_headers" >/dev/null; then
    fail "$mode /ui/ did not return text/html"
  fi
  if ! cmp -s "$fetch_body" "$packaged_index"; then
    diff -u --label packaged-index --label "$mode-http-index" \
      "$packaged_index" "$fetch_body" >&2 || true
    fail "$mode /ui/ bytes differ from the packaged entrypoint"
  fi

  fetch "$port" "$asset_path" svelte-asset "$root"
  if [ "$fetch_status" -ne 0 ] || [ "$fetch_code" != 200 ]; then
    fail "$mode main Svelte asset returned curl status $fetch_status and HTTP $fetch_code"
  fi
  if ! cmp -s "$fetch_body" "$packaged_asset"; then
    fail "$mode main Svelte asset bytes differ from the packaged file"
  fi

  jq --sort-keys --arg root "$root" '
    del(.data.pid, .data.revision)
    | walk(if type == "string" then (split($root) | join("<PRIVATE_ROOT>")) else . end)
  ' "$root/state.json" >"$root/state.normalized.json"
  jq --sort-keys --arg root "$root" '
    del(
      .data.revision,
      .data.values["server.port"],
      .data.values["server.bind_address"],
      .data.values["server.web_dir"]
    )
    | walk(if type == "string" then (split($root) | join("<PRIVATE_ROOT>")) else . end)
  ' "$root/settings.json" >"$root/settings.normalized.json"
}

capture_socket_snapshot() {
  local root="$1"
  local label="$2"
  local port="$3"
  socket_snapshot_file="$root/socket.$label.json"
  local stderr_file="$root/socket.$label.stderr"
  set +e
  "$project_python" -I -S "$canonical_proc_socket_evidence" snapshot \
    --backend-pid "$active_pid" \
    --backend-pgid "$active_pgid" \
    --address 127.0.0.1 \
    --port "$port" \
    --webkit-store "$desktop_webkit_store" \
    >"$socket_snapshot_file" 2>"$stderr_file"
  socket_snapshot_status=$?
  set -e
  if ! jq -e '
    keys == ["diagnostics", "evidence", "schema_version", "status"]
    and .schema_version == 1
    and (.diagnostics | type) == "array"
  ' "$socket_snapshot_file" >/dev/null 2>&1; then
    fail "socket evidence helper did not emit its canonical JSON document for $label"
  fi
  case "$socket_snapshot_status" in
    0)
      if ! jq -e '
        .status == "ok"
        and .diagnostics == []
        and (.evidence | type) == "object"
        and (.evidence.backend | type) == "object"
        and (.evidence.listener | type) == "object"
        and (.evidence.connections | type) == "array"
        and (.evidence.fingerprints | type) == "array"
        and (.evidence.fingerprints | length) > 0
      ' "$socket_snapshot_file" >/dev/null; then
        fail "successful socket evidence has an invalid payload for $label"
      fi
      ;;
    75)
      if ! jq -e '
        .status == "retryable"
        and (.diagnostics | length) == 1
        and .diagnostics[0].retryable == true
        and (.diagnostics[0].code | type) == "string"
        and (.evidence == null or (.evidence | type) == "object")
      ' "$socket_snapshot_file" >/dev/null; then
        fail "retryable socket evidence has an invalid payload for $label"
      fi
      ;;
    2)
      if ! jq -e '
        .status == "error"
        and (.diagnostics | length) == 1
        and .diagnostics[0].retryable == false
      ' "$socket_snapshot_file" >/dev/null; then
        fail "fatal socket evidence has an invalid payload for $label"
      fi
      fail "socket evidence helper reported a stable error for $label: $(jq -r '.diagnostics[0].code' "$socket_snapshot_file")"
      ;;
    *)
      fail "socket evidence helper exited unexpectedly for $label: $socket_snapshot_status"
      ;;
  esac
}

compare_socket_snapshots() {
  local root="$1"
  local label="$2"
  local first="$3"
  local second="$4"
  local excluded_json="$5"
  socket_compare_file="$root/socket.$label.compare.json"
  local stderr_file="$root/socket.$label.compare.stderr"
  if ! jq -e '
    type == "array" and all(.[]; type == "string")
  ' <<<"$excluded_json" >/dev/null; then
    fail "socket comparison exclusions are not a string array for $label"
  fi
  local exclusion
  local -a command=(
    "$project_python" -I -S "$canonical_proc_socket_evidence" compare
    --first "$first"
    --second "$second"
  )
  while IFS= read -r exclusion; do
    command+=(--exclude "$exclusion")
  done < <(jq -r '.[]' <<<"$excluded_json")
  set +e
  "${command[@]}" >"$socket_compare_file" 2>"$stderr_file"
  socket_compare_status=$?
  set -e
  if ! jq -e '
    keys == ["diagnostics", "evidence", "schema_version", "status"]
    and .schema_version == 1
    and (.diagnostics | type) == "array"
  ' "$socket_compare_file" >/dev/null 2>&1; then
    fail "socket evidence comparison did not emit canonical JSON for $label"
  fi
  case "$socket_compare_status" in
    0)
      if ! jq -e '
        .status == "ok"
        and .diagnostics == []
        and (.evidence.fingerprints | type) == "array"
        and (.evidence.fingerprints | length) > 0
        and all(.evidence.fingerprints[]; type == "string")
      ' "$socket_compare_file" >/dev/null; then
        fail "successful socket evidence comparison has an invalid payload for $label"
      fi
      ;;
    75)
      if ! jq -e '
        .status == "retryable"
        and (.diagnostics | length) == 1
        and .diagnostics[0].retryable == true
        and .diagnostics[0].code == "stable-connection-unavailable"
      ' "$socket_compare_file" >/dev/null; then
        fail "retryable socket comparison has an invalid payload for $label"
      fi
      ;;
    2)
      if ! jq -e '
        .status == "error"
        and (.diagnostics | length) == 1
        and .diagnostics[0].retryable == false
      ' "$socket_compare_file" >/dev/null; then
        fail "fatal socket comparison has an invalid payload for $label"
      fi
      fail "socket evidence comparison reported a stable error for $label: $(jq -r '.diagnostics[0].code' "$socket_compare_file")"
      ;;
    *)
      fail "socket evidence comparison exited unexpectedly for $label: $socket_compare_status"
      ;;
  esac
}

record_primary_socket_identity() {
  local root="$1"
  local snapshot="$2"
  if ! jq -e \
    --argjson pid "$active_pid" \
    --argjson pgid "$active_pgid" \
    --arg executable "$application" \
    --arg address 127.0.0.1 \
    --argjson port "$desktop_port" '
      .status == "ok"
      and .evidence.backend.pid == $pid
      and .evidence.backend.pgid == $pgid
      and .evidence.backend.executable == $executable
      and .evidence.backend.state != "X"
      and .evidence.backend.state != "Z"
      and .evidence.backend.state != "x"
      and .evidence.listener.state == "0A"
      and .evidence.listener.local.address == $address
      and .evidence.listener.local.port == $port
      and (.evidence.listener.inode | type) == "number"
    ' "$snapshot" >/dev/null; then
    fail "initial socket evidence does not identify the primary listener"
  fi
  desktop_primary_start_ticks="$(
    jq -er '.evidence.backend.start_ticks' "$snapshot"
  )"
  desktop_primary_executable="$(
    jq -er '.evidence.backend.executable' "$snapshot"
  )"
  desktop_listener_inode="$(
    jq -er '.evidence.listener.inode' "$snapshot"
  )"
  jq --sort-keys '
    {
      backend: .evidence.backend,
      listener: .evidence.listener
    }
  ' "$snapshot" >"$root/primary.identity.json"
}

verify_socket_continuity() {
  local snapshot="$1"
  if [ "$(jq -r '.status' "$snapshot")" = ok ]; then
    if ! jq -e \
      --argjson pid "$active_pid" \
      --argjson pgid "$active_pgid" \
      --argjson start_ticks "$desktop_primary_start_ticks" \
      --arg executable "$desktop_primary_executable" \
      --arg address 127.0.0.1 \
      --argjson port "$desktop_port" \
      --argjson inode "$desktop_listener_inode" '
        .evidence.backend.pid == $pid
        and .evidence.backend.pgid == $pgid
        and .evidence.backend.start_ticks == $start_ticks
        and .evidence.backend.executable == $executable
        and .evidence.backend.state != "X"
        and .evidence.backend.state != "Z"
        and .evidence.backend.state != "x"
        and .evidence.listener.inode == $inode
        and .evidence.listener.state == "0A"
        and .evidence.listener.local.address == $address
        and .evidence.listener.local.port == $port
      ' "$snapshot" >/dev/null; then
      fail "socket evidence observed a changed primary identity or listener"
    fi
    return
  fi
  if ! jq -e \
    --argjson pid "$active_pid" \
    --argjson pgid "$active_pgid" \
    --argjson start_ticks "$desktop_primary_start_ticks" \
    --arg executable "$desktop_primary_executable" \
    --arg address 127.0.0.1 \
    --argjson port "$desktop_port" \
    --argjson inode "$desktop_listener_inode" '
      .status == "retryable"
      and .evidence.backend.pid == $pid
      and .evidence.backend.pgid == $pgid
      and .evidence.backend.start_ticks == $start_ticks
      and .evidence.backend.executable == $executable
      and .evidence.backend.state != "X"
      and .evidence.backend.state != "Z"
      and .evidence.backend.state != "x"
      and (.evidence.backend_owned_inodes | index($inode)) != null
      and ([
        .evidence.matching_port_rows[]
        | select(
            .state == "0A"
            and .local.address == $address
            and .local.port == $port
          )
        | .inode
      ] == [$inode])
    ' "$snapshot" >/dev/null; then
    fail "retryable socket evidence observed a changed primary identity or listener"
  fi
}

select_stable_connection() {
  local snapshot="$1"
  local comparison="$2"
  local fingerprint
  fingerprint="$(jq -er '.evidence.fingerprints[0]' "$comparison")"
  jq -ce --arg fingerprint "$fingerprint" '
    [.evidence.connections[] | select(.fingerprint == $fingerprint)][0]
  ' "$snapshot"
}

read_process_start_ticks() {
  local pid="$1"
  "$project_python" -I -S -c '
from pathlib import Path
import sys

raw = Path(sys.argv[1]).read_text()
closing = raw.rfind(")")
fields = raw[closing + 2:].split()
if closing < 0 or len(fields) < 20:
    raise SystemExit(2)
print(fields[19])
' "/proc/$pid/stat"
}

validate_primary_continuity() {
  local root="$1"
  local port="$2"
  local prefix="$3"
  local live_executable
  local live_pgid
  local live_start_ticks
  local live_state
  live_state="$(ps -o stat= -p "$active_pid" 2>/dev/null \
    | tr -d '[:space:]')" || true
  if [ -z "$live_state" ] || [[ "$live_state" = X* ]] \
    || [[ "$live_state" = Z* ]] || [[ "$live_state" = x* ]]; then
    fail "desktop primary process stopped or became dead during keep-backend validation"
  fi
  live_executable="$(readlink -f -- "/proc/$active_pid/exe" 2>/dev/null || true)"
  live_pgid="$(ps -o pgid= -p "$active_pid" 2>/dev/null \
    | tr -d '[:space:]')" || true
  live_start_ticks="$(read_process_start_ticks "$active_pid" 2>/dev/null || true)"
  if [ "$live_executable" != "$desktop_primary_executable" ] \
    || [ "$live_pgid" != "$active_pgid" ] \
    || [ "$live_start_ticks" != "$desktop_primary_start_ticks" ]; then
    fail "desktop primary PID was replaced or changed identity"
  fi
  if grep -F 'POKECON-RUNTIME-0003' "$root/application.log" >/dev/null; then
    fail "desktop close or reopen requested backend shutdown before the final SIGTERM"
  fi
  fetch "$port" /api/state "$prefix-state" "$root"
  require_json_response 200 "desktop $prefix state continuity endpoint"
  if ! jq -e --argjson pid "$active_pid" '
    keys == ["data"] and .data.pid == $pid
  ' "$fetch_body" >/dev/null; then
    fail "desktop API no longer reports the original primary PID after $prefix"
  fi
  fetch "$port" /api/settings "$prefix-settings" "$root"
  require_json_response 200 "desktop $prefix settings continuity endpoint"
  if ! jq -e \
    --argjson port "$port" '
      .data.values["server.port"] == $port
      and .data.values["server.bind_address"] == "127.0.0.1"
      and .data.values["ui.desktop.close_behavior"] == "keep_backend"
    ' "$fetch_body" >/dev/null; then
    fail "desktop API launch settings changed after $prefix"
  fi
}

capture_initial_socket_baseline() {
  local root="$1"
  local deadline=$((SECONDS + 60))
  while [ "$SECONDS" -lt "$deadline" ]; do
    capture_socket_snapshot "$root" initial.first "$desktop_port"
    if [ "$socket_snapshot_status" -eq 75 ]; then
      sleep 0.1
      continue
    fi
    local first="$socket_snapshot_file"
    sleep 2
    capture_socket_snapshot "$root" initial.second "$desktop_port"
    if [ "$socket_snapshot_status" -eq 75 ]; then
      sleep 0.1
      continue
    fi
    local second="$socket_snapshot_file"
    compare_socket_snapshots "$root" initial "$first" "$second" '[]'
    if [ "$socket_compare_status" -eq 75 ]; then
      sleep 0.1
      continue
    fi
    if ! jq -e -s '
      .[0].evidence.backend == .[1].evidence.backend
      and .[0].evidence.listener == .[1].evidence.listener
    ' "$first" "$second" >/dev/null; then
      fail "initial socket snapshots changed primary identity or listener"
    fi
    record_primary_socket_identity "$root" "$second"
    desktop_initial_fingerprints="$(
      jq -c '.evidence.fingerprints' "$socket_compare_file"
    )"
    desktop_initial_connection="$(
      select_stable_connection "$second" "$socket_compare_file"
    )"
    return
  done
  fail "desktop did not expose a stable WebKit connection triple before the deadline"
}

observe_window() {
  local root="$1"
  local expected_window="${2:-}"
  local display
  local xauthority
  local -a windows=()
  display="$(head -n 1 -- "$root/display")"
  xauthority="$(head -n 1 -- "$root/xauthority")"
  mapfile -t windows < <(
    timeout --signal=TERM --kill-after=1s 2s \
      env DISPLAY="$display" XAUTHORITY="$xauthority" \
      xdotool search --onlyvisible --name '^PokeCon Controller$' \
      2>/dev/null || true
  )
  if [ "${#windows[@]}" -ne 1 ]; then
    return 1
  fi
  observed_window="${windows[0]}"
  if [ -n "$expected_window" ] \
    && [ "$observed_window" != "$expected_window" ]; then
    return 1
  fi
  observed_title="$(
    timeout --signal=TERM --kill-after=1s 2s \
      env DISPLAY="$display" XAUTHORITY="$xauthority" \
      xdotool getwindowname "$observed_window"
  )" || return 1
  observed_pid="$(
    timeout --signal=TERM --kill-after=1s 2s \
      env DISPLAY="$display" XAUTHORITY="$xauthority" \
      xdotool getwindowpid "$observed_window"
  )" || return 1
  if [ "$observed_title" != "PokeCon Controller" ] \
    || [ "$observed_pid" != "$active_pid" ]; then
    return 1
  fi
  timeout --signal=TERM --kill-after=1s 2s \
    env DISPLAY="$display" XAUTHORITY="$xauthority" \
    xprop -id "$observed_window" _NET_WM_PID \
    >"$root/window.pid-property" || return 1
  if ! grep -E "=[[:space:]]*$active_pid$" \
    "$root/window.pid-property" >/dev/null; then
    return 1
  fi
  timeout --signal=TERM --kill-after=1s 2s \
    env DISPLAY="$display" XAUTHORITY="$xauthority" \
    xwininfo -id "$observed_window" >"$root/window.info" || return 1
  if ! grep -F 'Map State: IsViewable' "$root/window.info" >/dev/null; then
    return 1
  fi
  observed_width="$(
    grep -F 'Width:' "$root/window.info" | head -n 1 | tr -cd '0-9'
  )"
  observed_height="$(
    grep -F 'Height:' "$root/window.info" | head -n 1 | tr -cd '0-9'
  )"
  if [[ ! "$observed_width" =~ ^[0-9]+$ ]] \
    || [[ ! "$observed_height" =~ ^[0-9]+$ ]] \
    || [ "$observed_width" -ne 1440 ] \
    || [ "$observed_height" -ne 900 ]; then
    return 1
  fi
}

observe_webkit() {
  local expected_store="${1:-}"
  local expected_pid="${2:-}"
  local descendant
  local compositing_environment_count=0
  local dmabuf_environment_count=0
  local egl_vendor_environment_count=0
  local environment_entry
  local executable
  local found_mesa_vendor_library=false
  local libgl_drivers_environment_count=0
  local libgl_software_environment_count=0
  local _map_address
  local _map_device
  local _map_inode
  local _map_offset
  local _map_permissions
  local _map_suffix
  local mapped_basename
  local mapped_path
  local canonical_mapped_path
  local observed_egl_dispatcher=
  local observed_mesa_software_driver=
  local process_command
  local selected=
  local -a descendants=()
  mapfile -t descendants < <(collect_descendants "$active_pid")
  for descendant in "${descendants[@]}"; do
    if [ ! -e "/proc/$descendant/exe" ]; then
      continue
    fi
    executable="$(readlink -f -- "/proc/$descendant/exe" 2>/dev/null || true)"
    if [ "$(basename -- "$executable")" = WebKitWebProcess ]; then
      IFS= read -r process_command <"/proc/$descendant/comm" || continue
      case "$process_command" in
        WebKitWebProc*) ;;
        *) continue ;;
      esac
      selected="$descendant"
      webkit_executable="$executable"
      break
    fi
  done
  if [ -z "$selected" ]; then
    return 1
  fi
  case "$webkit_executable" in
    "$store_directory"/*/*) ;;
    *) return 1 ;;
  esac
  webkit_store="$(printf '%s\n' "$webkit_executable" | cut -d / -f 1-4)"
  if [ -n "$expected_store" ] && [ "$webkit_store" != "$expected_store" ]; then
    return 1
  fi
  if [ -n "$expected_pid" ] && [ "$selected" != "$expected_pid" ]; then
    return 1
  fi
  if ! grep -F "$webkit_store/" "/proc/$active_pid/maps" >/dev/null; then
    return 1
  fi
  if [ ! -r "/proc/$selected/environ" ]; then
    return 1
  fi
  while IFS= read -r -d '' environment_entry; do
    case "$environment_entry" in
      WEBKIT_DISABLE_COMPOSITING_MODE=*)
        compositing_environment_count=$((compositing_environment_count + 1))
        if [ "$environment_entry" != WEBKIT_DISABLE_COMPOSITING_MODE=1 ]; then
          return 1
        fi
        ;;
      WEBKIT_DISABLE_DMABUF_RENDERER=*)
        dmabuf_environment_count=$((dmabuf_environment_count + 1))
        if [ "$environment_entry" != WEBKIT_DISABLE_DMABUF_RENDERER=1 ]; then
          return 1
        fi
        ;;
      LIBGL_DRIVERS_PATH=*)
        libgl_drivers_environment_count=$((libgl_drivers_environment_count + 1))
        if [ "$environment_entry" != "LIBGL_DRIVERS_PATH=$mesa_dri_directory" ]; then
          return 1
        fi
        ;;
      LIBGL_ALWAYS_SOFTWARE=*)
        libgl_software_environment_count=$((libgl_software_environment_count + 1))
        if [ "$environment_entry" != LIBGL_ALWAYS_SOFTWARE=1 ]; then
          return 1
        fi
        ;;
      __EGL_VENDOR_LIBRARY_FILENAMES=*)
        egl_vendor_environment_count=$((egl_vendor_environment_count + 1))
        if [ "$environment_entry" != "__EGL_VENDOR_LIBRARY_FILENAMES=$mesa_egl_vendor_manifest" ]; then
          return 1
        fi
        ;;
    esac
  done <"/proc/$selected/environ"
  if [ "$compositing_environment_count" -ne 1 ] \
    || [ "$dmabuf_environment_count" -ne 1 ]; then
    return 1
  fi
  if [ "$libgl_drivers_environment_count" -ne 1 ] \
    || [ "$libgl_software_environment_count" -ne 1 ] \
    || [ "$egl_vendor_environment_count" -ne 1 ]; then
    return 1
  fi
  if tr '\0' '\n' <"/proc/$selected/environ" \
    | grep -E '^LD_LIBRARY_PATH=' >/dev/null; then
    return 1
  fi
  if [ ! -r "/proc/$selected/maps" ]; then
    return 1
  fi
  while read -r \
    _map_address _map_permissions _map_offset _map_device _map_inode \
    mapped_path _map_suffix; do
    case "$mapped_path" in
      /*) ;;
      *) continue ;;
    esac
    case "$mapped_path" in
      /usr/* | /lib/* | /lib64/* | /run/opengl-driver/*)
        mapped_basename="$(basename -- "$mapped_path")"
        case "$mapped_basename" in
          libEGL*.so* | libGL*.so* | libOpenGL*.so* | libgbm*.so* | *_dri.so* | *gallium*.so*)
            return 1
            ;;
        esac
        ;;
    esac
    case "$mapped_path" in
      "$canonical_mesa_renderer"/* | "$store_directory"/*-libglvnd-*/lib/libEGL.so.1*) ;;
      *) continue ;;
    esac
    canonical_mapped_path="$(
      readlink -f -- "$mapped_path" 2>/dev/null || true
    )"
    if [ "$canonical_mapped_path" = "$mesa_egl_vendor_library" ]; then
      found_mesa_vendor_library=true
    fi
    case "$canonical_mapped_path" in
      "$canonical_mesa_swrast_driver" | "$canonical_mesa_renderer"/lib/libgallium*.so*)
        if [ ! -f "$canonical_mapped_path" ] \
          || [ -L "$canonical_mapped_path" ]; then
          return 1
        fi
        if [ -n "$observed_mesa_software_driver" ] \
          && [ "$observed_mesa_software_driver" != "$canonical_mapped_path" ]; then
          return 1
        fi
        observed_mesa_software_driver="$canonical_mapped_path"
        ;;
    esac
    case "$canonical_mapped_path" in
      "$store_directory"/*-libglvnd-*/lib/libEGL.so.1*)
        if [ -n "$observed_egl_dispatcher" ] \
          && [ "$observed_egl_dispatcher" != "$canonical_mapped_path" ]; then
          return 1
        fi
        observed_egl_dispatcher="$canonical_mapped_path"
        ;;
    esac
  done <"/proc/$selected/maps"
  if [ "$found_mesa_vendor_library" != true ] \
    || [ -z "$observed_mesa_software_driver" ] \
    || [ -z "$observed_egl_dispatcher" ] \
    || [ ! -f "$observed_egl_dispatcher" ] \
    || [ -L "$observed_egl_dispatcher" ]; then
    return 1
  fi
  desktop_egl_dispatcher="$observed_egl_dispatcher"
  desktop_mesa_software_driver="$observed_mesa_software_driver"
  webkit_pid="$selected"
}

validate_private_process_group_closure() {
  local root="$1"
  local snapshot="$root/process-group.executables"
  local pending="$root/process-group.executables.pending"
  local candidate_pid
  local candidate_pgid
  local candidate_state
  local canonical_executable
  local found_application=false
  : >"$pending"
  while read -r candidate_pid candidate_pgid; do
    if [ "$candidate_pgid" != "$active_pgid" ]; then
      continue
    fi
    candidate_state="$(ps -o stat= -p "$candidate_pid" 2>/dev/null \
      | tr -d '[:space:]')" || true
    if [ -z "$candidate_state" ] || [[ "$candidate_state" = Z* ]]; then
      continue
    fi
    canonical_executable="$(
      readlink -f -- "/proc/$candidate_pid/exe" 2>/dev/null || true
    )"
    if [ -z "$canonical_executable" ]; then
      if [ ! -d "/proc/$candidate_pid" ]; then
        continue
      fi
      fail "desktop process group contains a live process without a canonical executable: $candidate_pid"
    fi
    case "$canonical_executable" in
      "$store_directory"/*/*) ;;
      *)
        fail "desktop process group escaped immutable store executables: pid=$candidate_pid executable=$canonical_executable"
        ;;
    esac
    if [ ! -f "$canonical_executable" ] || [ ! -x "$canonical_executable" ]; then
      fail "desktop process group executable is not a live immutable file: $canonical_executable"
    fi
    printf '%s\t%s\n' "$candidate_pid" "$canonical_executable" >>"$pending"
    if [ "$candidate_pid" = "$active_pid" ]; then
      found_application=true
    fi
  done < <(ps -e -o pid= -o pgid=)
  if [ "$found_application" != true ]; then
    fail "desktop application was absent from its private process-group closure"
  fi
  sort -n -k1,1 -- "$pending" >"$snapshot"
  rm -f -- "$pending"
  desktop_process_group="$active_pgid"
  desktop_process_group_members="$(
    jq -Rsc '
      split("\n")
      | map(
          select(length > 0)
          | split("\t")
          | {pid: (.[0] | tonumber), executable: .[1]}
        )
    ' "$snapshot"
  )"
}

validate_desktop_dbus() {
  local expected_address="unix:path=$gate_root/runtime/bus"
  local address_suffix
  local guid
  local runtime_mode
  local -a address_entries=()
  mapfile -t address_entries < <(
    tr '\0' '\n' <"/proc/$active_pid/environ" \
      | grep -E '^DBUS_SESSION_BUS_ADDRESS=' || true
  )
  if [ "${#address_entries[@]}" -ne 1 ]; then
    fail "desktop application did not receive exactly one private D-Bus address"
  fi
  desktop_dbus_address="${address_entries[0]#DBUS_SESSION_BUS_ADDRESS=}"
  case "$desktop_dbus_address" in
    "$expected_address") ;;
    "$expected_address",guid=*)
      address_suffix="${desktop_dbus_address#"$expected_address"}"
      guid="${address_suffix#,guid=}"
      if [[ ! "$guid" =~ ^[0-9a-f]{32}$ ]]; then
        fail "desktop application received an invalid private D-Bus GUID"
      fi
      ;;
    *) fail "desktop application escaped the gate-private D-Bus socket" ;;
  esac
  desktop_dbus_socket="$gate_root/runtime/bus"
  if [ ! -d "$gate_root/runtime" ] || [ -L "$gate_root/runtime" ]; then
    fail "desktop D-Bus runtime parent is not a real private directory"
  fi
  runtime_mode="$(stat -c '%a' -- "$gate_root/runtime")"
  if [ "$runtime_mode" != 700 ]; then
    fail "desktop D-Bus runtime parent mode is not 0700: $runtime_mode"
  fi
  if [ ! -S "$desktop_dbus_socket" ] || [ -L "$desktop_dbus_socket" ]; then
    fail "desktop D-Bus address does not name a live real socket"
  fi
}

start_close_request_relay() {
  local root="$1"
  local ready_file="$root/close-relay.ready.json"
  local evidence_file="$root/close-relay.json"
  local stderr_file="$root/close-relay.stderr"
  local -a libx11_mappings=()
  mapfile -t libx11_mappings < <(
    grep -aoE \
      "$store_directory/[a-z0-9]{32}-lib[xX]11-[^/[:space:]]+/lib/libX11\.so\.[0-9.]+" \
      "/proc/$active_pid/maps" | sort -u
  )
  if [ "${#libx11_mappings[@]}" -ne 1 ]; then
    fail "desktop primary did not map exactly one immutable X11 library"
  fi
  local libx11
  libx11="$(readlink -f -- "${libx11_mappings[0]}" 2>/dev/null || true)"
  case "$libx11" in
    "$store_directory"/*/lib/libX11.so.*) ;;
    *) fail "desktop primary mapped an invalid X11 library: $libx11" ;;
  esac
  if [ "$libx11" != "${libx11_mappings[0]}" ] \
    || [ ! -f "$libx11" ] \
    || [ -L "$libx11" ]; then
    fail "desktop primary X11 mapping is not a canonical immutable file: $libx11"
  fi
  desktop_x11_library="$libx11"
  reset_close_relay_identity_cache
  "$timeout_command" --signal=TERM --kill-after=1s 10s \
    env DISPLAY="$desktop_display" XAUTHORITY="$desktop_xauthority" \
    "$project_python" -I -S "$canonical_ewmh_close_relay" \
    --display "$desktop_display" \
    --lib-x11 "$libx11" \
    --ready-file "$ready_file" \
    --target-window "$desktop_initial_window" \
    --timeout-seconds 8 \
    >"$evidence_file" 2>"$stderr_file" &
  local spawned_relay_supervisor=$!
  local captured_relay_supervisor
  local captured_relay_pgid
  local captured_relay_start_ticks
  local captured_relay_executable
  if ! capture_expected_process_identity \
    "$spawned_relay_supervisor" "$timeout_executable" \
    "X11 close-request relay supervisor" \
    captured_relay_supervisor captured_relay_pgid \
    captured_relay_start_ticks captured_relay_executable 100; then
    fail "X11 close-request relay supervisor identity could not be captured safely"
  fi
  close_relay_supervisor="$captured_relay_supervisor"
  close_relay_supervisor_pgid="$captured_relay_pgid"
  close_relay_supervisor_start_ticks="$captured_relay_start_ticks"
  close_relay_supervisor_executable="$captured_relay_executable"
  close_relay_status=
  local relay_state
  for _attempt in {1..100}; do
    if [ -s "$ready_file" ]; then
      if ! jq -e \
        --arg display "$desktop_display" \
        --arg xauthority "$desktop_xauthority" \
        --argjson target "$desktop_initial_window" '
          keys == [
            "display", "event_mask", "protocols", "root_window",
            "schema_version", "status", "target_window", "xauthority"
          ]
          and .schema_version == 1
          and .status == "ready"
          and .display == $display
          and .target_window == $target
          and .event_mask == 524288
          and .protocols.wm_delete_window_advertised == true
          and (.root_window | type) == "number"
          and .root_window > 0
          and .xauthority == $xauthority
        ' "$ready_file" >/dev/null; then
        fail "X11 close-request relay published invalid readiness evidence"
      fi
      return
    fi
    relay_state="$(ps -o stat= -p "$close_relay_supervisor" 2>/dev/null \
      | tr -d '[:space:]')" || true
    if [ -z "$relay_state" ] || [[ "$relay_state" = Z* ]]; then
      set +e
      wait "$close_relay_supervisor"
      local relay_status=$?
      set -e
      reset_close_relay_identity_cache
      fail "X11 close-request relay stopped before readiness: $relay_status"
    fi
    sleep 0.05
  done
  fail "X11 close-request relay did not become ready before its deadline"
}

reap_close_relay_if_stopped() {
  if [[ ! "$close_relay_supervisor" =~ ^[0-9]+$ ]]; then
    return
  fi
  local relay_state
  relay_state="$(ps -o stat= -p "$close_relay_supervisor" 2>/dev/null \
    | tr -d '[:space:]')" || true
  if [ -n "$relay_state" ] && [[ "$relay_state" != Z* ]]; then
    return
  fi
  set +e
  wait "$close_relay_supervisor"
  close_relay_status=$?
  set -e
  reset_close_relay_identity_cache
  if [ "$close_relay_status" -ne 0 ]; then
    fail "X11 close-request relay stopped before destroying the target window: $close_relay_status"
  fi
}

close_initial_window_and_wait() {
  local root="$1"
  local display="$desktop_display"
  local xauthority="$desktop_xauthority"
  start_close_request_relay "$root"
  if ! timeout --signal=TERM --kill-after=1s 2s \
    env DISPLAY="$display" XAUTHORITY="$xauthority" \
    xdotool windowquit "$desktop_initial_window" \
    >"$root/close.stdout" 2>"$root/close.stderr"; then
    fail "desktop native close request failed"
  fi
  local deadline=$((SECONDS + 60))
  local -a visible_windows=()
  while [ "$SECONDS" -lt "$deadline" ]; do
    reap_close_relay_if_stopped
    mapfile -t visible_windows < <(
      timeout --signal=TERM --kill-after=1s 2s \
        env DISPLAY="$display" XAUTHORITY="$xauthority" \
        xdotool search --onlyvisible --name '^PokeCon Controller$' \
        2>/dev/null || true
    )
    if [ "${#visible_windows[@]}" -eq 0 ] \
      && ! timeout --signal=TERM --kill-after=1s 2s \
        env DISPLAY="$display" XAUTHORITY="$xauthority" \
        xwininfo -id "$desktop_initial_window" \
        >"$root/closed-window.info" 2>"$root/closed-window.stderr"; then
      if [[ "$close_relay_supervisor" =~ ^[0-9]+$ ]]; then
        set +e
        wait "$close_relay_supervisor"
        close_relay_status=$?
        set -e
        reset_close_relay_identity_cache
      fi
      if [ "$close_relay_status" != 0 ]; then
        fail "X11 close-request relay did not translate windowquit: $close_relay_status"
      fi
      if ! jq -s -e \
        --arg display "$desktop_display" \
        --arg xauthority "$desktop_xauthority" \
        --arg lib_x11 "$desktop_x11_library" \
        --argjson target "$desktop_initial_window" '
          .[0] as $ready
          | .[1] as $complete
          | ($complete | keys) == [
              "diagnostics", "evidence", "schema_version", "status"
            ]
          and $complete.schema_version == 1
          and $complete.status == "ok"
          and $complete.diagnostics == []
          and $complete.evidence.display == $display
          and $complete.evidence.xauthority == $xauthority
          and $complete.evidence.lib_x11 == $lib_x11
          and $complete.evidence.target_window == $target
          and $complete.evidence.event_mask == 524288
          and $complete.evidence.protocols_before_ready.wm_delete_window_advertised == true
          and $complete.evidence.protocols_before_forward.wm_delete_window_advertised == true
          and $complete.evidence.received.type == "ClientMessage"
          and $complete.evidence.received.message_type == "_NET_CLOSE_WINDOW"
          and $complete.evidence.received.send_event == true
          and $complete.evidence.received.target_window == $target
          and $complete.evidence.received.format == 32
          and $complete.evidence.received.data == [0, 0, 0, 0, 0]
          and $complete.evidence.received.received_on_root == $ready.root_window
          and $complete.evidence.forwarded.message_type == "WM_PROTOCOLS"
          and $complete.evidence.forwarded.protocol == "WM_DELETE_WINDOW"
          and $complete.evidence.forwarded.target_window == $target
          and $complete.evidence.forwarded.format == 32
          and $complete.evidence.forwarded.send_count == 1
          and ($complete.evidence.forwarded.data | length) == 5
          and $complete.evidence.forwarded.data[1:] == [0, 0, 0, 0]
          and ($complete.evidence.forwarded.data[0] | type) == "number"
          and (
            $complete.evidence.protocols_before_ready.advertised_atom_ids
            | index($complete.evidence.forwarded.data[0])
          ) != null
          and (
            $complete.evidence.protocols_before_forward.advertised_atom_ids
            | index($complete.evidence.forwarded.data[0])
          ) != null
          and $complete.evidence.forwarded.xsync_succeeded == true
        ' "$root/close-relay.ready.json" "$root/close-relay.json" >/dev/null; then
        fail "X11 close-request relay emitted invalid completion evidence"
      fi
      desktop_close_relay_evidence="$(jq -c '.evidence' "$root/close-relay.json")"
      validate_primary_continuity "$root" "$desktop_port" post-close
      return
    fi
    validate_primary_continuity "$root" "$desktop_port" closing
    sleep 0.1
  done
  fail "desktop window remained visible or its original XID survived the close deadline"
}

capture_post_close_socket_baseline() {
  local root="$1"
  local deadline=$((SECONDS + 60))
  while [ "$SECONDS" -lt "$deadline" ]; do
    capture_socket_snapshot "$root" post-close.first "$desktop_port"
    local first="$socket_snapshot_file"
    local first_status="$socket_snapshot_status"
    local first_code=
    if [ "$first_status" -eq 75 ]; then
      first_code="$(jq -er '.diagnostics[0].code' "$first")"
      if [ "$first_code" != connection-unavailable ]; then
        validate_primary_continuity "$root" "$desktop_port" post-close-churn
        sleep 0.1
        continue
      fi
    fi
    verify_socket_continuity "$first"
    sleep 2
    capture_socket_snapshot "$root" post-close.second "$desktop_port"
    local second="$socket_snapshot_file"
    local second_status="$socket_snapshot_status"
    local second_code=
    if [ "$second_status" -eq 75 ]; then
      second_code="$(jq -er '.diagnostics[0].code' "$second")"
      if [ "$second_code" != connection-unavailable ]; then
        validate_primary_continuity "$root" "$desktop_port" post-close-churn
        sleep 0.1
        continue
      fi
    fi
    verify_socket_continuity "$second"
    if [ "$first_status" -eq 75 ] && [ "$second_status" -eq 75 ]; then
      if [ "$first_code" = connection-unavailable ] \
        && [ "$second_code" = connection-unavailable ]; then
        desktop_post_close_fingerprints='[]'
        desktop_post_close_empty=true
        break
      fi
      sleep 0.1
      continue
    fi
    if [ "$first_status" -ne 0 ] || [ "$second_status" -ne 0 ]; then
      sleep 0.1
      continue
    fi
    compare_socket_snapshots "$root" post-close "$first" "$second" '[]'
    if [ "$socket_compare_status" -eq 75 ]; then
      sleep 0.1
      continue
    fi
    desktop_post_close_fingerprints="$(
      jq -c '.evidence.fingerprints' "$socket_compare_file"
    )"
    desktop_post_close_empty=false
    break
  done
  if [ "$desktop_post_close_fingerprints" = '[]' ] \
    && [ "$desktop_post_close_empty" != true ]; then
    fail "desktop socket state did not settle after closing its window"
  fi
  desktop_disappeared_fingerprints="$(
    jq -cn \
      --argjson initial "$desktop_initial_fingerprints" \
      --argjson post_close "$desktop_post_close_fingerprints" \
      '$initial - $post_close | unique'
  )"
  if ! jq -e 'length > 0' <<<"$desktop_disappeared_fingerprints" >/dev/null; then
    fail "closing the desktop window did not remove any initially stable WebKit connection"
  fi
  validate_primary_continuity "$root" "$desktop_port" post-close-baseline
}

terminate_secondary_if_live() {
  cleanup_secondary_process_tree
}

launch_same_session_secondary() {
  local root="$1"
  local snapshots_before="$root/resource-snapshots.before-secondary"
  local snapshots_after="$root/resource-snapshots.after-secondary"
  local pid_file="$root/secondary.pid"
  local status_file="$root/secondary.status"
  local identity_file="$root/secondary.identity"
  local helper_status_file="$root/secondary.helper.status"
  local display_file="$root/secondary.display"
  local xauthority_file="$root/secondary.xauthority"
  local log_file="$root/secondary.log"
  capture_resource_snapshot_inventory "$root" "$snapshots_before"
  local -a primary_snapshots=()
  mapfile -t primary_snapshots <"$snapshots_before"
  # This gate runs the exact-Nix package, whose verified immutable resource
  # root is borrowed. Packaged-provenance snapshot placement and lifetime are
  # covered by the Rust and production-routing mutation audits.
  if [ "${#primary_snapshots[@]}" -ne 0 ]; then
    fail "exact-Nix desktop primary unexpectedly materialized a private resource snapshot"
  fi
  reset_secondary_identity_cache
  set +e
  timeout --signal=TERM --kill-after=2s 20s \
    env \
      DISPLAY="$desktop_display" \
      XAUTHORITY="$desktop_xauthority" \
      DBUS_SESSION_BUS_ADDRESS="$desktop_dbus_address" \
      "$bash_binary" "$script_path" __launch_product \
      desktop "$root" "$application" "$desktop_port" "$child_path" \
      "$pid_file" "$status_file" "$display_file" "$xauthority_file" \
      "$root/runtime" "$canonical_mesa_renderer" "$identity_file" \
      >"$log_file" 2>&1
  desktop_secondary_helper_status=$?
  set -e
  printf '%s\n' "$desktop_secondary_helper_status" >"$helper_status_file"
  consume_published_secondary_identity "$identity_file" "$pid_file"
  if [ "$desktop_secondary_helper_status" -ne 0 ]; then
    terminate_secondary_if_live
    fail "same-session secondary helper did not exit cleanly: $desktop_secondary_helper_status"
  fi
  if [ ! -s "$status_file" ]; then
    terminate_secondary_if_live
    fail "same-session secondary did not publish its application status"
  fi
  desktop_secondary_status="$(head -n 1 -- "$status_file")"
  if [ "$desktop_secondary_status" != 0 ]; then
    terminate_secondary_if_live
    fail "same-session secondary application did not exit cleanly: $desktop_secondary_status"
  fi
  if [ ! -s "$display_file" ] || [ ! -s "$xauthority_file" ]; then
    fail "same-session secondary did not record display and Xauthority state"
  fi
  local secondary_display
  local secondary_xauthority
  secondary_display="$(head -n 1 -- "$display_file")"
  secondary_xauthority="$(
    readlink -f -- "$(head -n 1 -- "$xauthority_file")"
  )"
  if [ "$secondary_display" != "$desktop_display" ] \
    || [ "$secondary_xauthority" != "$desktop_xauthority" ]; then
    fail "same-session secondary escaped the primary display or Xauthority"
  fi
  if grep -iE \
    'POKECON-RUNTIME-000[1-4]|FatalError|fatal|panick|BackendStartup|bind|address.*in use|dynamic configuration|static Web root' \
    "$log_file" >/dev/null; then
    fail "same-session secondary emitted a runtime, fatal, panic, bind, or resource diagnostic"
  fi
  if process_identity_matches \
    "$secondary_pid" "$secondary_pgid" "$secondary_start_ticks" \
    "$secondary_executable"; then
    terminate_secondary_if_live
    fail "same-session secondary remained live after its synchronous launch"
  fi
  capture_resource_snapshot_inventory "$root" "$snapshots_after"
  if ! cmp -s -- "$snapshots_before" "$snapshots_after"; then
    diff -u \
      --label resource-snapshots-before-secondary \
      --label resource-snapshots-after-secondary \
      "$snapshots_before" "$snapshots_after" >&2 || true
    fail "same-session secondary changed the private resource snapshot inventory"
  fi
  reset_secondary_identity_cache
  validate_primary_continuity "$root" "$desktop_port" secondary-launch
}

wait_for_reopened_surface_and_socket() {
  local root="$1"
  local deadline=$((SECONDS + 60))
  while [ "$SECONDS" -lt "$deadline" ]; do
    if ! observe_window "$root" \
      || [ "$observed_window" = "$desktop_initial_window" ] \
      || ! observe_webkit "$desktop_webkit_store"; then
      validate_primary_continuity "$root" "$desktop_port" reopening
      sleep 0.1
      continue
    fi
    local candidate_window="$observed_window"
    local candidate_width="$observed_width"
    local candidate_height="$observed_height"
    capture_socket_snapshot "$root" reopened.first "$desktop_port"
    if [ "$socket_snapshot_status" -eq 75 ]; then
      sleep 0.1
      continue
    fi
    local first="$socket_snapshot_file"
    verify_socket_continuity "$first"
    sleep 2
    if ! observe_window "$root" "$candidate_window" \
      || ! observe_webkit "$desktop_webkit_store"; then
      sleep 0.1
      continue
    fi
    local candidate_webkit_pid="$webkit_pid"
    capture_socket_snapshot "$root" reopened.second "$desktop_port"
    if [ "$socket_snapshot_status" -eq 75 ]; then
      sleep 0.1
      continue
    fi
    local second="$socket_snapshot_file"
    verify_socket_continuity "$second"
    compare_socket_snapshots \
      "$root" reopened "$first" "$second" \
      "$desktop_post_close_fingerprints"
    if [ "$socket_compare_status" -eq 75 ]; then
      sleep 0.1
      continue
    fi
    desktop_reopened_window="$candidate_window"
    desktop_reopened_width="$candidate_width"
    desktop_reopened_height="$candidate_height"
    desktop_reopened_webkit_pid="$candidate_webkit_pid"
    desktop_reopened_fingerprints="$(
      jq -c '.evidence.fingerprints' "$socket_compare_file"
    )"
    desktop_reopened_connection="$(
      select_stable_connection "$second" "$socket_compare_file"
    )"
    validate_primary_continuity "$root" "$desktop_port" reopened
    return
  done
  fail "same-session secondary did not reopen a stable primary-owned desktop surface"
}

validate_desktop_surface() {
  local root="$1"
  local ready=false
  local readiness_deadline
  if [ ! -s "$root/display" ] || [ ! -s "$root/xauthority" ]; then
    fail "desktop launch did not publish display and Xauthority state"
  fi
  desktop_display="$(head -n 1 -- "$root/display")"
  desktop_xauthority="$(
    readlink -f -- "$(head -n 1 -- "$root/xauthority")"
  )"
  case "$desktop_xauthority" in
    "$root"/*) ;;
    *) fail "desktop Xauthority file is outside its private mode root" ;;
  esac
  if [ ! -f "$desktop_xauthority" ]; then
    fail "desktop Xauthority file is not live"
  fi

  readiness_deadline=$((SECONDS + 60))
  while [ "$SECONDS" -lt "$readiness_deadline" ]; do
    if observe_window "$root" && observe_webkit; then
      ready=true
      break
    fi
    if ! kill -0 "$active_pid" 2>/dev/null; then
      fail "desktop application stopped before its native surface became ready"
    fi
    sleep 0.1
  done
  if [ "$ready" != true ]; then
    fail "desktop did not expose one mapped native window and WebKit process before the deadline"
  fi
  desktop_initial_window="$observed_window"
  desktop_initial_width="$observed_width"
  desktop_initial_height="$observed_height"
  desktop_webkit_store="$webkit_store"
  desktop_initial_webkit_pid="$webkit_pid"

  indicator_mapping="$(
    grep -F -m 1 'libayatana-appindicator3.so.1' \
      "/proc/$active_pid/maps" || true
  )"
  if [ -z "$indicator_mapping" ]; then
    fail "desktop process did not map the Ayatana AppIndicator runtime"
  fi
  indicator_library="$(
    printf '%s\n' "$indicator_mapping" | tr -s ' ' | cut -d ' ' -f 6
  )"
  indicator_library="$(readlink -f -- "$indicator_library")"
  case "$indicator_library" in
    "$store_directory"/*/lib/libayatana-appindicator3.so.1*) ;;
    *)
      fail "desktop AppIndicator library is not from an immutable store output: $indicator_library"
      ;;
  esac
  indicator_directory="$(dirname -- "$indicator_library")"
  if ! grep -aF "$indicator_directory" "$application" >/dev/null; then
    fail "packaged application does not encode the mapped AppIndicator RPATH"
  fi

  capture_initial_socket_baseline "$root"
  if ! observe_window "$root" "$desktop_initial_window"; then
    fail "desktop native window did not remain mapped and stable for two seconds"
  fi
  if ! observe_webkit "$desktop_webkit_store" "$desktop_initial_webkit_pid"; then
    fail "desktop WebKit process did not remain in the application closure for two seconds"
  fi
  validate_private_process_group_closure "$root"
  validate_desktop_dbus
  local initial_dbus_address="$desktop_dbus_address"
  close_initial_window_and_wait "$root"
  capture_post_close_socket_baseline "$root"
  launch_same_session_secondary "$root"
  wait_for_reopened_surface_and_socket "$root"
  validate_private_process_group_closure "$root"
  validate_desktop_dbus
  if [ "$desktop_dbus_address" != "$initial_dbus_address" ]; then
    fail "desktop primary changed its private D-Bus session while reopening"
  fi
}

diagnostic_sequence() {
  local combined_file="$1"
  grep -oE 'POKECON-RUNTIME-000[1-4]' "$combined_file" || true
}

validate_diagnostics() {
  local mode="$1"
  local root="$2"
  local combined_file="$root/application.log"
  diagnostic_sequence "$combined_file" >"$root/diagnostics.sequence"
  printf '%s\n' \
    POKECON-RUNTIME-0001 \
    POKECON-RUNTIME-0003 \
    POKECON-RUNTIME-0002 \
    >"$root/diagnostics.expected"
  if ! cmp -s "$root/diagnostics.expected" "$root/diagnostics.sequence"; then
    diff -u --label expected-diagnostics --label "$mode-diagnostics" \
      "$root/diagnostics.expected" "$root/diagnostics.sequence" >&2 || true
    fail "$mode diagnostic IDs are missing, repeated, or out of order"
  fi
  if ! grep -E 'POKECON-RUNTIME-0003.*Signal\(Terminate\)' \
    "$combined_file" >/dev/null; then
    fail "$mode shutdown diagnostic does not report Signal(Terminate) on its 0003 line"
  fi
  if grep -F 'POKECON-RUNTIME-0004' "$combined_file" >/dev/null; then
    fail "$mode logs contain the signal-handler failure diagnostic"
  fi
  if grep -iE \
    'FatalError|panicked at|thread .* panicked|Failed to load ayatana-appindicator3|dynamic configuration is unavailable|continuing with static settings|BackendStartup|static Web root.*(missing|unavailable)' \
    "$combined_file" >/dev/null; then
    fail "$mode logs contain a fatal, panic, resource, dynamic, or AppIndicator failure"
  fi
  if grep -E \
    '/usr/|/usr/libexec|xdg-desktop-portal|Activating service name=.*org\.freedesktop\.portal|Successfully activated service .*org\.freedesktop\.portal' \
    "$combined_file" >/dev/null; then
    fail "$mode logs contain a host /usr path or desktop portal auto-activation"
  fi
}

stop_mode_normally() {
  local mode="$1"
  local port="$2"
  local root="$3"
  if ! signal_process_if_identity_matches \
    TERM "$mode application" \
    "$active_pid" "$active_pgid" "$active_start_ticks" \
    "$active_executable"; then
    fail "$mode application identity changed before normal shutdown; refusing SIGTERM"
  fi
  for _attempt in {1..400}; do
    if ! process_identity_matches \
      "$active_pid" "$active_pgid" "$active_start_ticks" \
      "$active_executable"; then
      break
    fi
    sleep 0.1
  done
  if process_identity_matches \
    "$active_pid" "$active_pgid" "$active_start_ticks" \
    "$active_executable"; then
    fail "$mode application did not stop before the shutdown deadline"
  fi
  for _attempt in {1..300}; do
    if ! process_identity_matches \
      "$active_supervisor" "$active_supervisor_pgid" \
      "$active_supervisor_start_ticks" "$active_supervisor_executable"; then
      break
    fi
    sleep 0.1
  done
  if process_identity_matches \
    "$active_supervisor" "$active_supervisor_pgid" \
    "$active_supervisor_start_ticks" "$active_supervisor_executable"; then
    fail "$mode supervisor did not stop before the shutdown deadline"
  fi
  set +e
  wait "$active_supervisor"
  supervisor_status=$?
  set -e
  if [ ! -s "$root/application.status" ]; then
    fail "$mode application did not publish its exit status"
  fi
  application_status="$(head -n 1 -- "$root/application.status")"
  if [ "$application_status" != 0 ] || [ "$supervisor_status" -ne 0 ]; then
    fail "$mode shutdown was not clean: application=$application_status supervisor=$supervisor_status"
  fi
  fetch "$port" /api/state closed-port "$root"
  if [ "$fetch_status" -eq 0 ]; then
    fail "$mode listener remained reachable after clean shutdown"
  fi
  local snapshots_after_shutdown="$root/resource-snapshots.after-shutdown"
  capture_resource_snapshot_inventory "$root" "$snapshots_after_shutdown"
  if [ -s "$snapshots_after_shutdown" ]; then
    fail "$mode retained private resource snapshots after clean shutdown"
  fi
  validate_diagnostics "$mode" "$root"
  reset_primary_identity_cache
  reset_active_supervisor_identity_cache
}

run_mode() {
  local mode="$1"
  local root="$2"
  local port="$3"
  current_mode="$mode"
  current_mode_root="$root"
  launch_mode "$mode" "$root" "$port"
  local launched_pid="$active_pid"
  wait_for_readiness "$mode" "$port" "$root"
  validate_process_identity "$mode"
  cp "$root/readiness.body" "$root/state.json"
  validate_http_contract "$mode" "$port" "$root"
  if [ "$mode" = desktop ]; then
    validate_desktop_surface "$root"
  fi
  stop_mode_normally "$mode" "$port" "$root"
  if [ "$mode" = web ]; then
    web_pid="$launched_pid"
  else
    desktop_pid="$launched_pid"
  fi
}

web_root="$gate_root/web-mode"
desktop_root="$gate_root/desktop-mode"
web_port="$(allocate_port)"
desktop_port="$(allocate_port)"
if [ "$web_port" = "$desktop_port" ]; then
  desktop_port="$(allocate_port)"
fi
if [ "$web_port" = "$desktop_port" ]; then
  fail "Web and desktop modes did not receive distinct ephemeral ports"
fi

run_mode web "$web_root" "$web_port"
run_mode desktop "$desktop_root" "$desktop_port"

if ! cmp -s \
  "$web_root/openapi-method-results.json" \
  "$desktop_root/openapi-method-results.json"; then
  diff -u --label web-openapi-methods --label desktop-openapi-methods \
    "$web_root/openapi-method-results.json" \
    "$desktop_root/openapi-method-results.json" >&2 || true
  fail "Web and desktop OpenAPI exact method-result inventories differ"
fi
if ! cmp -s \
  "$web_root/cors-preflight-policy-results.json" \
  "$desktop_root/cors-preflight-policy-results.json"; then
  diff -u --label web-cors-preflight-policy --label desktop-cors-preflight-policy \
    "$web_root/cors-preflight-policy-results.json" \
    "$desktop_root/cors-preflight-policy-results.json" >&2 || true
  fail "Web and desktop closed-world CORS preflight results differ"
fi
if ! cmp -s \
  "$web_root/unknown-api-boundary-results.json" \
  "$desktop_root/unknown-api-boundary-results.json"; then
  diff -u --label web-unknown-api-boundary --label desktop-unknown-api-boundary \
    "$web_root/unknown-api-boundary-results.json" \
    "$desktop_root/unknown-api-boundary-results.json" >&2 || true
  fail "Web and desktop unknown API boundary results differ"
fi
if ! cmp -s \
  "$web_root/advertised-bare-options-results.json" \
  "$desktop_root/advertised-bare-options-results.json"; then
  diff -u \
    --label web-advertised-bare-options \
    --label desktop-advertised-bare-options \
    "$web_root/advertised-bare-options-results.json" \
    "$desktop_root/advertised-bare-options-results.json" >&2 || true
  fail "Web and desktop advertised bare OPTIONS security results differ"
fi
web_openapi_method_probe_count="$(
  jq -er 'length' "$web_root/openapi-method-results.json"
)"
desktop_openapi_method_probe_count="$(
  jq -er 'length' "$desktop_root/openapi-method-results.json"
)"
openapi_method_results_sha256="$(
  sha256sum "$web_root/openapi-method-results.json" | cut -d ' ' -f 1
)"
cors_preflight_policy_results="$(
  jq -ceS . "$web_root/cors-preflight-policy-results.json"
)"
cors_preflight_policy_results_sha256="$(
  sha256sum "$web_root/cors-preflight-policy-results.json" | cut -d ' ' -f 1
)"
cors_preflight_policy_probe_count="$(
  jq -er 'length' "$web_root/cors-preflight-policy-results.json"
)"
cors_preflight_advertised_pair_count="$(
  jq -er '[.[] | select(.classification == "advertised_pair")] | length' \
    "$web_root/cors-preflight-policy-results.json"
)"
cors_preflight_known_wrong_count="$(
  jq -er '[.[] | select(.classification == "known_path_wrong_method")] | length' \
    "$web_root/cors-preflight-policy-results.json"
)"
cors_preflight_unknown_path_count="$(
  jq -er '[.[] | select(.classification == "unknown_path")] | length' \
    "$web_root/cors-preflight-policy-results.json"
)"
unknown_api_boundary_results="$(
  jq -ceS . "$web_root/unknown-api-boundary-results.json"
)"
web_unknown_api_boundary_probe_count="$(
  jq -er 'length' "$web_root/unknown-api-boundary-results.json"
)"
desktop_unknown_api_boundary_probe_count="$(
  jq -er 'length' "$desktop_root/unknown-api-boundary-results.json"
)"
unknown_api_routed_method_count="$(
  jq -er '[.[] | select(.classification == "routed_not_found")] | length' \
    "$web_root/unknown-api-boundary-results.json"
)"
unknown_api_options_rejection_count="$(
  jq -er \
    '[.[] | select(.classification == "security_rejected_options")] | length' \
    "$web_root/unknown-api-boundary-results.json"
)"
advertised_bare_options_results="$(
  jq -ceS . "$web_root/advertised-bare-options-results.json"
)"
advertised_bare_options_results_sha256="$(
  sha256sum "$web_root/advertised-bare-options-results.json" | cut -d ' ' -f 1
)"
web_advertised_bare_options_probe_count="$(
  jq -er 'length' "$web_root/advertised-bare-options-results.json"
)"
desktop_advertised_bare_options_probe_count="$(
  jq -er 'length' "$desktop_root/advertised-bare-options-results.json"
)"
advertised_bare_options_rest_path_count="$(
  jq -er '[.[] | select(.path_kind == "rest")] | length' \
    "$web_root/advertised-bare-options-results.json"
)"
advertised_bare_options_websocket_path_count="$(
  jq -er '[.[] | select(.path_kind == "websocket")] | length' \
    "$web_root/advertised-bare-options-results.json"
)"
if ! cmp -s \
  "$web_root/state.normalized.json" \
  "$desktop_root/state.normalized.json"; then
  diff -u --label web-state --label desktop-state \
    "$web_root/state.normalized.json" \
    "$desktop_root/state.normalized.json" >&2 || true
  fail "normalized Web and desktop state APIs differ"
fi
if ! cmp -s \
  "$web_root/settings.normalized.json" \
  "$desktop_root/settings.normalized.json"; then
  diff -u --label web-settings --label desktop-settings \
    "$web_root/settings.normalized.json" \
    "$desktop_root/settings.normalized.json" >&2 || true
  fail "normalized Web and desktop settings APIs differ"
fi

application_sha256_after="$(sha256sum "$application" | cut -d ' ' -f 1)"
if [ "$application_sha256_after" != "$application_sha256_before" ]; then
  fail "packaged application bytes changed while exercising UI modes"
fi

jq -n \
  --arg package "$canonical_package" \
  --arg application "$application" \
  --arg application_sha256 "$application_sha256_after" \
  --arg canonical_openapi_sha256 "$openapi_sha256" \
  --argjson advertised_operation_count "$openapi_operation_count" \
  --argjson advertised_rest_operation_count "$openapi_rest_operation_count" \
  --argjson advertised_websocket_operation_count "$openapi_websocket_operation_count" \
  --argjson canonical_path_count "$openapi_path_count" \
  --argjson standard_method_count "$openapi_method_count" \
  --argjson method_matrix_probe_count "$openapi_method_probe_count" \
  --argjson rejected_method_count "$openapi_rejected_method_count" \
  --argjson cors_preflight_count "$openapi_cors_preflight_count" \
  --argjson web_method_probe_count "$web_openapi_method_probe_count" \
  --argjson desktop_method_probe_count "$desktop_openapi_method_probe_count" \
  --arg method_results_sha256 "$openapi_method_results_sha256" \
  --argjson cors_rejection_forbidden_response_headers \
  "$cors_rejection_forbidden_response_headers_json" \
  --argjson cors_preflight_policy_results "$cors_preflight_policy_results" \
  --arg cors_preflight_policy_results_sha256 "$cors_preflight_policy_results_sha256" \
  --argjson cors_preflight_policy_probe_count "$cors_preflight_policy_probe_count" \
  --argjson cors_preflight_advertised_pair_count "$cors_preflight_advertised_pair_count" \
  --argjson cors_preflight_known_wrong_count "$cors_preflight_known_wrong_count" \
  --argjson cors_preflight_unknown_path_count "$cors_preflight_unknown_path_count" \
  --argjson unknown_api_boundary_results "$unknown_api_boundary_results" \
  --argjson web_unknown_api_boundary_probe_count "$web_unknown_api_boundary_probe_count" \
  --argjson desktop_unknown_api_boundary_probe_count "$desktop_unknown_api_boundary_probe_count" \
  --argjson unknown_api_routed_method_count "$unknown_api_routed_method_count" \
  --argjson unknown_api_options_rejection_count "$unknown_api_options_rejection_count" \
  --argjson advertised_bare_options_results "$advertised_bare_options_results" \
  --arg advertised_bare_options_results_sha256 "$advertised_bare_options_results_sha256" \
  --argjson web_advertised_bare_options_probe_count "$web_advertised_bare_options_probe_count" \
  --argjson desktop_advertised_bare_options_probe_count "$desktop_advertised_bare_options_probe_count" \
  --argjson advertised_bare_options_rest_path_count "$advertised_bare_options_rest_path_count" \
  --argjson advertised_bare_options_websocket_path_count "$advertised_bare_options_websocket_path_count" \
  --arg index_sha256 "$index_sha256" \
  --arg asset "$asset_path" \
  --arg asset_sha256 "$asset_sha256" \
  --argjson web_pid "$web_pid" \
  --argjson web_port "$web_port" \
  --argjson desktop_pid "$desktop_pid" \
  --argjson desktop_port "$desktop_port" \
  --argjson desktop_process_group "$desktop_process_group" \
  --argjson desktop_process_group_members "$desktop_process_group_members" \
  --arg desktop_dbus_address "$desktop_dbus_address" \
  --arg desktop_dbus_socket "$desktop_dbus_socket" \
  --arg desktop_display "$desktop_display" \
  --argjson desktop_primary_start_ticks "$desktop_primary_start_ticks" \
  --arg desktop_primary_executable "$desktop_primary_executable" \
  --argjson desktop_listener_inode "$desktop_listener_inode" \
  --argjson desktop_initial_window "$desktop_initial_window" \
  --argjson desktop_initial_width "$desktop_initial_width" \
  --argjson desktop_initial_height "$desktop_initial_height" \
  --argjson desktop_initial_webkit_pid "$desktop_initial_webkit_pid" \
  --argjson desktop_initial_fingerprints "$desktop_initial_fingerprints" \
  --argjson desktop_initial_connection "$desktop_initial_connection" \
  --argjson desktop_post_close_fingerprints "$desktop_post_close_fingerprints" \
  --argjson desktop_post_close_empty "$desktop_post_close_empty" \
  --argjson desktop_disappeared_fingerprints "$desktop_disappeared_fingerprints" \
  --arg desktop_x11_library "$desktop_x11_library" \
  --argjson desktop_close_relay_evidence "$desktop_close_relay_evidence" \
  --argjson desktop_secondary_pid "$desktop_secondary_pid" \
  --argjson desktop_secondary_status "$desktop_secondary_status" \
  --argjson desktop_secondary_helper_status "$desktop_secondary_helper_status" \
  --argjson desktop_reopened_window "$desktop_reopened_window" \
  --argjson desktop_reopened_width "$desktop_reopened_width" \
  --argjson desktop_reopened_height "$desktop_reopened_height" \
  --argjson desktop_reopened_webkit_pid "$desktop_reopened_webkit_pid" \
  --argjson desktop_reopened_fingerprints "$desktop_reopened_fingerprints" \
  --argjson desktop_reopened_connection "$desktop_reopened_connection" \
  --arg webkit_store "$desktop_webkit_store" \
  --arg indicator_library "$indicator_library" \
  --arg mesa_renderer_output "$canonical_mesa_renderer" \
  --arg mesa_egl_vendor_manifest "$mesa_egl_vendor_manifest" \
  --arg mesa_egl_vendor_library "$mesa_egl_vendor_library" \
  --arg mesa_swrast_entry "$mesa_swrast_entry" \
  --arg mesa_swrast_target "$canonical_mesa_swrast_driver" \
  --arg mesa_mapped_software_driver "$desktop_mesa_software_driver" \
  --arg egl_dispatcher "$desktop_egl_dispatcher" \
  '{
    gate: "ui-package-check",
    status: "PASS",
    package: $package,
    application: $application,
    application_sha256: $application_sha256,
    api_inventory: {
      canonical_openapi_sha256: $canonical_openapi_sha256,
      advertised_operation_count: $advertised_operation_count,
      advertised_rest_operation_count: $advertised_rest_operation_count,
      advertised_websocket_operation_count: $advertised_websocket_operation_count,
      canonical_path_count: $canonical_path_count,
      standard_method_count: $standard_method_count,
      method_matrix_probe_count: $method_matrix_probe_count,
      rejected_method_count: $rejected_method_count,
      cors_preflight_count: $cors_preflight_count,
      cors_preflight_policy: {
        allowed_headers: ["Content-Type", "X-Pokecon-Request"],
        allowed_methods: ["GET", "PATCH", "POST", "OPTIONS"],
        advertised_pair_count: $cors_preflight_advertised_pair_count,
        known_path_wrong_method_count: $cors_preflight_known_wrong_count,
        mode_results_byte_identical: true,
        probe_count_per_mode: $cors_preflight_policy_probe_count,
        results: $cors_preflight_policy_results,
        results_sha256: $cors_preflight_policy_results_sha256,
        unknown_path_count: $cors_preflight_unknown_path_count
      },
      bare_options_security_boundary: {
        advertised_path_count: ($advertised_bare_options_results | length),
        desktop_probe_count: $desktop_advertised_bare_options_probe_count,
        mode_results_byte_identical: true,
        rest_path_count: $advertised_bare_options_rest_path_count,
        results: $advertised_bare_options_results,
        results_sha256: $advertised_bare_options_results_sha256,
        web_probe_count: $web_advertised_bare_options_probe_count,
        websocket_path_count: $advertised_bare_options_websocket_path_count
      },
      unknown_api_boundary: {
        desktop_probe_count: $desktop_unknown_api_boundary_probe_count,
        mode_results_byte_identical: true,
        options_security_rejection_count: $unknown_api_options_rejection_count,
        path: "/api/not-in-openapi",
        probe_count_per_mode: ($unknown_api_boundary_results | length),
        results: $unknown_api_boundary_results,
        routed_method_count: $unknown_api_routed_method_count,
        web_probe_count: $web_unknown_api_boundary_probe_count
      },
      web_method_probe_count: $web_method_probe_count,
      desktop_method_probe_count: $desktop_method_probe_count,
      method_results_sha256: $method_results_sha256,
      mode_results_byte_identical: true,
      allow_policy: "openapi-advertised-methods-only-cors-options-excluded",
      bare_options_policy: "outer-security-403-before-inner-rest-websocket-dispatch",
      implicit_head_policy: "explicit-route-405-empty-body",
      options_policy: "advertised-pair-preflight-204-other-preflight-403",
      probe_scope: "bidirectional-exact-method-complement-non-mutating"
    },
    immutable_ui: {
      index_sha256: $index_sha256,
      main_asset: $asset,
      main_asset_sha256: $asset_sha256
    },
    web: {
      pid: $web_pid,
      port: $web_port,
      shutdown_status: 0,
      resource_snapshots_after_shutdown: 0
    },
    desktop: {
      pid: $desktop_pid,
      port: $desktop_port,
      shutdown_status: 0,
      resource_snapshots_after_shutdown: 0,
      primary_identity: {
        pid: $desktop_pid,
        start_ticks: $desktop_primary_start_ticks,
        pgid: $desktop_process_group,
        executable: $desktop_primary_executable,
        listener_inode: $desktop_listener_inode
      },
      process_group: {
        id: $desktop_process_group,
        members: $desktop_process_group_members
      },
      dbus: {
        address: $desktop_dbus_address,
        socket: $desktop_dbus_socket
      },
      display: $desktop_display,
      initial: {
        window: {
          id: $desktop_initial_window,
          title: "PokeCon Controller",
          mapped: true,
          width: $desktop_initial_width,
          height: $desktop_initial_height
        },
        webkit: {
          pid: $desktop_initial_webkit_pid,
          store_output: $webkit_store
        },
        socket: {
          stable_fingerprints: $desktop_initial_fingerprints,
          selected_connection: $desktop_initial_connection
        }
      },
      keep_backend_close: {
        request: "xdotool windowquit",
        relay: $desktop_close_relay_evidence,
        x11_library: $desktop_x11_library,
        visible_windows_after: 0,
        original_xid_destroyed: true,
        primary_identity_unchanged: true,
        api_continuity: {
          state_endpoint: {
            json_http_200: true,
            original_primary_pid_reported: true
          },
          settings_endpoint: {
            json_http_200: true,
            canonical_launch_values_preserved: true
          }
        },
        listener_unchanged: true,
        shutdown_requested_before_final_signal: false,
        settled_empty: $desktop_post_close_empty,
        stable_fingerprints: $desktop_post_close_fingerprints,
        disappeared_initial_fingerprints: $desktop_disappeared_fingerprints
      },
      secondary: {
        pid: $desktop_secondary_pid,
        helper_status: $desktop_secondary_helper_status,
        application_status: $desktop_secondary_status,
        same_mode_root: true,
        same_display: true,
        same_xauthority: true,
        same_dbus_session: true,
        same_runtime: true,
        same_renderer: true,
        same_port_and_arguments: true,
        resource_snapshot_inventory_unchanged: true
      },
      reopened: {
        window: {
          id: $desktop_reopened_window,
          differs_from_initial_xid: true,
          title: "PokeCon Controller",
          mapped: true,
          width: $desktop_reopened_width,
          height: $desktop_reopened_height
        },
        webkit: {
          pid: $desktop_reopened_webkit_pid,
          store_output: $webkit_store,
          pid_reuse_allowed: true
        },
        socket: {
          excluded_post_close_fingerprints: $desktop_post_close_fingerprints,
          stable_fingerprints: $desktop_reopened_fingerprints,
          selected_connection: $desktop_reopened_connection
        },
        primary_identity_unchanged: true,
        api_continuity: {
          state_endpoint: {
            json_http_200: true,
            original_primary_pid_reported: true
          },
          settings_endpoint: {
            json_http_200: true,
            canonical_launch_values_preserved: true
          }
        },
        listener_unchanged: true,
        process_group_revalidated: true,
        dbus_revalidated: true
      },
      software_renderer: {
        output: $mesa_renderer_output,
        egl_vendor_manifest: $mesa_egl_vendor_manifest,
        egl_vendor_library: $mesa_egl_vendor_library,
        swrast_entry: $mesa_swrast_entry,
        swrast_target: $mesa_swrast_target,
        mapped_software_driver: $mesa_mapped_software_driver,
        egl_dispatcher: $egl_dispatcher
      },
      appindicator_library: $indicator_library
    },
    api_parity: {
      normalized_state_payload: true,
      normalized_settings_payload: true
    },
    diagnostics: {
      startup: "POKECON-RUNTIME-0001",
      shutdown_request: "POKECON-RUNTIME-0003 Signal(Terminate)",
      clean_stop: "POKECON-RUNTIME-0002"
    }
  }
  | if (
      (keys == [
        "api_inventory", "api_parity", "application", "application_sha256",
        "desktop", "diagnostics", "gate", "immutable_ui", "package", "status", "web"
      ])
      and ((.api_inventory | keys) == [
        "advertised_operation_count", "advertised_rest_operation_count",
        "advertised_websocket_operation_count", "allow_policy",
        "bare_options_policy", "bare_options_security_boundary",
        "canonical_openapi_sha256", "canonical_path_count", "cors_preflight_count",
        "cors_preflight_policy",
        "desktop_method_probe_count", "implicit_head_policy", "method_matrix_probe_count",
        "method_results_sha256", "mode_results_byte_identical", "options_policy",
        "probe_scope", "rejected_method_count", "standard_method_count",
        "unknown_api_boundary", "web_method_probe_count"
      ])
      and (.api_inventory.canonical_openapi_sha256 | test("^[0-9a-f]{64}$"))
      and .api_inventory.advertised_operation_count == 16
      and .api_inventory.advertised_rest_operation_count == 15
      and .api_inventory.advertised_websocket_operation_count == 1
      and .api_inventory.canonical_path_count == 15
      and .api_inventory.standard_method_count == 9
      and .api_inventory.method_matrix_probe_count == 135
      and .api_inventory.rejected_method_count == 104
      and .api_inventory.cors_preflight_count == 15
      and ((.api_inventory.bare_options_security_boundary | keys) == [
        "advertised_path_count", "desktop_probe_count",
        "mode_results_byte_identical", "rest_path_count", "results",
        "results_sha256", "web_probe_count", "websocket_path_count"
      ])
      and .api_inventory.bare_options_security_boundary.advertised_path_count == 15
      and .api_inventory.bare_options_security_boundary.desktop_probe_count == 15
      and .api_inventory.bare_options_security_boundary.mode_results_byte_identical
        == true
      and .api_inventory.bare_options_security_boundary.rest_path_count == 14
      and .api_inventory.bare_options_security_boundary.web_probe_count == 15
      and .api_inventory.bare_options_security_boundary.websocket_path_count == 1
      and (.api_inventory.bare_options_security_boundary.results_sha256
        | test("^[0-9a-f]{64}$"))
      and (.api_inventory.bare_options_security_boundary.results | length) == 15
      and (.api_inventory.bare_options_security_boundary.results | length)
        == (.api_inventory.bare_options_security_boundary.results
          | unique_by(.path) | length)
      and ([.api_inventory.bare_options_security_boundary.results[]
        | select(.path_kind == "rest")] | length) == 14
      and ([.api_inventory.bare_options_security_boundary.results[]
        | select(.path_kind == "websocket" and .path == "/ws")] | length) == 1
      and ([.api_inventory.bare_options_security_boundary.results[]
        | select(.path == "/api/settings")] | length) == 1
      and ([.api_inventory.bare_options_security_boundary.results[].body_sha256]
        | unique | length) == 1
      and all(.api_inventory.bare_options_security_boundary.results[];
        keys == [
          "access_control_request_method_header_sent", "body_sha256",
          "classification", "content_type", "error",
          "forbidden_response_headers_absent", "method",
          "origin_header_sent", "path", "path_kind", "request_body_bytes",
          "request_variant", "response_class", "status"
        ]
        and .access_control_request_method_header_sent == false
        and (.body_sha256 | test("^[0-9a-f]{64}$"))
        and .classification == "advertised_path_security_rejected"
        and .content_type == "application/json"
        and .error == {
          code: "request_forbidden",
          fields: null,
          message: "request validation failed"
        }
        and .forbidden_response_headers_absent == [
          "Access-Control-Allow-Headers", "Access-Control-Allow-Methods",
          "Access-Control-Allow-Origin", "Allow", "Vary"
        ]
        and .method == "options"
        and .origin_header_sent == false
        and .path != "/api/not-in-openapi"
        and .request_body_bytes == 0
        and .request_variant == "bare"
        and .response_class == "request_forbidden"
        and .status == 403
        and if .path_kind == "rest" then
          (.path | startswith("/api/"))
        elif .path_kind == "websocket" then
          .path == "/ws"
        else false
        end
      )
      and ((.api_inventory.cors_preflight_policy | keys) == [
        "advertised_pair_count", "allowed_headers", "allowed_methods",
        "known_path_wrong_method_count", "mode_results_byte_identical",
        "probe_count_per_mode", "results", "results_sha256", "unknown_path_count"
      ])
      and .api_inventory.cors_preflight_policy.allowed_headers
        == ["Content-Type", "X-Pokecon-Request"]
      and .api_inventory.cors_preflight_policy.allowed_methods
        == ["GET", "PATCH", "POST", "OPTIONS"]
      and .api_inventory.cors_preflight_policy.advertised_pair_count == 16
      and .api_inventory.cors_preflight_policy.known_path_wrong_method_count == 6
      and .api_inventory.cors_preflight_policy.unknown_path_count == 1
      and .api_inventory.cors_preflight_policy.probe_count_per_mode == 23
      and .api_inventory.cors_preflight_policy.mode_results_byte_identical == true
      and (.api_inventory.cors_preflight_policy.results_sha256
        | test("^[0-9a-f]{64}$"))
      and (.api_inventory.cors_preflight_policy.results | length) == 23
      and ([.api_inventory.cors_preflight_policy.results[]
        | select(.classification == "advertised_pair")] | length) == 16
      and ([.api_inventory.cors_preflight_policy.results[]
        | select(.classification == "known_path_wrong_method")] | length) == 6
      and ([.api_inventory.cors_preflight_policy.results[]
        | select(.classification == "unknown_path")] | length) == 1
      and ([.api_inventory.cors_preflight_policy.results[]
        | select(.classification == "known_path_wrong_method")
        | {path, requested_method}] | sort_by([.path, .requested_method])) == ([
          {path: "/api/settings", requested_method: "post"},
          {path: "/api/camera/retry", requested_method: "get"},
          {path: "/api/state", requested_method: "head"},
          {path: "/api/settings", requested_method: "put"},
          {path: "/api/state", requested_method: "delete"},
          {path: "/api/state", requested_method: "connect"}
        ] | sort_by([.path, .requested_method]))
      and ([.api_inventory.cors_preflight_policy.results[]
        | select(.classification != "advertised_pair")
        | .body_sha256] | unique | length) == 1
      and all(.api_inventory.cors_preflight_policy.results[];
        if .classification == "advertised_pair" then
          keys == [
            "classification", "operation_id", "path", "requested_method",
            "response_class", "status"
          ]
          and (.operation_id | type) == "string"
          and (.operation_id | length) > 0
          and (.requested_method == "get"
            or .requested_method == "patch"
            or .requested_method == "post")
          and .status == 204
          and .response_class == "cors_preflight"
        elif (.classification == "known_path_wrong_method"
          or .classification == "unknown_path") then
          keys == [
            "body_sha256", "classification", "content_type", "error",
            "forbidden_response_headers_absent", "operation_id", "path",
            "requested_method", "response_class", "status"
          ]
          and (.body_sha256 | test("^[0-9a-f]{64}$"))
          and .content_type == "application/json"
          and .error == {
            code: "request_forbidden",
            fields: null,
            message: "request validation failed"
          }
          and .forbidden_response_headers_absent
            == $cors_rejection_forbidden_response_headers
          and .operation_id == null
          and .status == 403
          and .response_class == "request_forbidden"
          and if .classification == "known_path_wrong_method" then
            (.path | startswith("/api/"))
          else
            .path == "/api/not-in-openapi"
            and .requested_method == "get"
          end
        else false
        end
      )
      and ((.api_inventory.unknown_api_boundary | keys) == [
        "desktop_probe_count", "mode_results_byte_identical",
        "options_security_rejection_count", "path", "probe_count_per_mode",
        "results", "routed_method_count", "web_probe_count"
      ])
      and .api_inventory.unknown_api_boundary.desktop_probe_count == 12
      and .api_inventory.unknown_api_boundary.mode_results_byte_identical == true
      and .api_inventory.unknown_api_boundary.options_security_rejection_count == 4
      and .api_inventory.unknown_api_boundary.path == "/api/not-in-openapi"
      and .api_inventory.unknown_api_boundary.probe_count_per_mode == 12
      and .api_inventory.unknown_api_boundary.routed_method_count == 8
      and .api_inventory.unknown_api_boundary.web_probe_count == 12
      and (.api_inventory.unknown_api_boundary.results | length) == 12
      and (.api_inventory.unknown_api_boundary.results | length)
        == (.api_inventory.unknown_api_boundary.results
          | unique_by([.method, .request_variant]) | length)
      and ([.api_inventory.unknown_api_boundary.results[].method] | unique) == [
        "connect", "delete", "get", "head", "options", "patch", "post",
        "put", "trace"
      ]
      and ([.api_inventory.unknown_api_boundary.results[]
        | select(.classification == "routed_not_found")] | length) == 8
      and ([.api_inventory.unknown_api_boundary.results[]
        | select(.classification == "security_rejected_options")] | length) == 4
      and ([.api_inventory.unknown_api_boundary.results[]
        | select(.classification == "security_rejected_options")
        | .request_variant] | sort) == [
          "bare", "complete_preflight", "origin_only", "requested_method_only"
        ]
      and ([.api_inventory.unknown_api_boundary.results[]
        | select(.classification == "routed_not_found")
        | .content_length] | unique | length) == 1
      and all(.api_inventory.unknown_api_boundary.results[];
        keys == [
          "classification", "content_length", "method", "path",
          "request_variant", "response_class", "status"
        ]
        and .path == "/api/not-in-openapi"
        and if .classification == "routed_not_found" then
          .method != "options"
          and .request_variant == "valid_boundary_headers"
          and (.content_length | type) == "number"
          and .content_length > 0
          and .status == 404
          and if .method == "head" then
            .response_class == "resource_not_found_head"
          else
            .response_class == "resource_not_found"
          end
        elif .classification == "security_rejected_options" then
          .method == "options"
          and .content_length == null
          and .response_class == "request_forbidden"
          and .status == 403
        else false
        end
      )
      and any(.api_inventory.unknown_api_boundary.results[];
        .method == "options"
        and .request_variant == "complete_preflight"
      )
      and .api_inventory.web_method_probe_count == 135
      and .api_inventory.desktop_method_probe_count == 135
      and (.api_inventory.method_results_sha256 | test("^[0-9a-f]{64}$"))
      and .api_inventory.mode_results_byte_identical == true
      and .api_inventory.allow_policy
        == "openapi-advertised-methods-only-cors-options-excluded"
      and .api_inventory.bare_options_policy
        == "outer-security-403-before-inner-rest-websocket-dispatch"
      and .api_inventory.implicit_head_policy == "explicit-route-405-empty-body"
      and .api_inventory.options_policy
        == "advertised-pair-preflight-204-other-preflight-403"
      and .api_inventory.probe_scope
        == "bidirectional-exact-method-complement-non-mutating"
      and ((.web | keys) == [
        "pid", "port", "resource_snapshots_after_shutdown", "shutdown_status"
      ])
      and .web.resource_snapshots_after_shutdown == 0
      and .desktop.resource_snapshots_after_shutdown == 0
      and ((.desktop.secondary | keys) == [
        "application_status", "helper_status", "pid",
        "resource_snapshot_inventory_unchanged", "same_dbus_session",
        "same_display", "same_mode_root", "same_port_and_arguments",
        "same_renderer", "same_runtime", "same_xauthority"
      ])
      and .desktop.secondary.resource_snapshot_inventory_unchanged == true
      and (.desktop.initial.window.id | type) == "number"
      and (.desktop.reopened.window.id | type) == "number"
      and .desktop.initial.window.id != .desktop.reopened.window.id
      and .desktop.keep_backend_close.relay.target_window
        == .desktop.initial.window.id
      and ((.desktop.keep_backend_close.api_continuity | keys)
        == ["settings_endpoint", "state_endpoint"])
      and ((.desktop.keep_backend_close.api_continuity.state_endpoint | keys)
        == ["json_http_200", "original_primary_pid_reported"])
      and .desktop.keep_backend_close.api_continuity.state_endpoint.json_http_200 == true
      and .desktop.keep_backend_close.api_continuity.state_endpoint.original_primary_pid_reported == true
      and ((.desktop.keep_backend_close.api_continuity.settings_endpoint | keys)
        == ["canonical_launch_values_preserved", "json_http_200"])
      and .desktop.keep_backend_close.api_continuity.settings_endpoint.json_http_200 == true
      and .desktop.keep_backend_close.api_continuity.settings_endpoint.canonical_launch_values_preserved == true
      and ((.desktop.reopened.api_continuity | keys)
        == ["settings_endpoint", "state_endpoint"])
      and ((.desktop.reopened.api_continuity.state_endpoint | keys)
        == ["json_http_200", "original_primary_pid_reported"])
      and .desktop.reopened.api_continuity.state_endpoint.json_http_200 == true
      and .desktop.reopened.api_continuity.state_endpoint.original_primary_pid_reported == true
      and ((.desktop.reopened.api_continuity.settings_endpoint | keys)
        == ["canonical_launch_values_preserved", "json_http_200"])
      and .desktop.reopened.api_continuity.settings_endpoint.json_http_200 == true
      and .desktop.reopened.api_continuity.settings_endpoint.canonical_launch_values_preserved == true
      and ((.api_parity | keys)
        == ["normalized_settings_payload", "normalized_state_payload"])
      and .api_parity.normalized_state_payload == true
      and .api_parity.normalized_settings_payload == true
    ) then .
    else error("ui-package-check window identity output schema is invalid")
    end'
