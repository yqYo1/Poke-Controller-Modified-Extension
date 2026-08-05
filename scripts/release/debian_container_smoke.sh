#!/usr/bin/env bash
set -euo pipefail

package=/tmp/pokecon.deb
app_user=pokecon-smoke
home=/home/pokecon-smoke
config="$home/.config"
data="$home/.local/share"
cache="$home/.cache"
state="$home/.local/state"
application=/usr/bin/pokecon
script_path=$(readlink -f -- "$0")
readonly desktop_readiness_budget_seconds=60
readonly desktop_bounded_subprocess_timeout_seconds=2
readonly desktop_bounded_subprocess_kill_grace_seconds=1
readonly desktop_cleanup_process_count=3
readonly desktop_cleanup_per_process_budget_seconds=7
readonly desktop_timeout_margin_seconds=5
readonly desktop_outer_term_seconds=90
readonly desktop_max_bounded_subprocess_overshoot_seconds=$((
  desktop_bounded_subprocess_timeout_seconds
  + desktop_bounded_subprocess_kill_grace_seconds
))
readonly desktop_cleanup_budget_seconds=$((
  desktop_cleanup_process_count * desktop_cleanup_per_process_budget_seconds
))
readonly desktop_completion_budget_seconds=$((
  desktop_readiness_budget_seconds
  + desktop_max_bounded_subprocess_overshoot_seconds
  + desktop_cleanup_budget_seconds
  + desktop_timeout_margin_seconds
))
if ((desktop_completion_budget_seconds >= desktop_outer_term_seconds)); then
  echo "desktop probe timeout budget must complete before outer TERM" >&2
  exit 2
fi

application_sha256() {
  sha256sum "$application" | cut -d ' ' -f 1
}

assert_application_sha256() {
  if [[ $# -ne 2 || ! $1 =~ ^[0-9a-f]{64}$ ]]; then
    echo "internal executable SHA-256 contract is invalid" >&2
    return 2
  fi
  local expected_sha=$1
  local checkpoint=$2
  local actual_sha
  actual_sha=$(application_sha256)
  if [[ $actual_sha != "$expected_sha" ]]; then
    echo "installed executable changed at $checkpoint: expected $expected_sha, got $actual_sha" >&2
    return 1
  fi
}

readiness_time_remains() {
  if [[ $# -ne 1 || ! $1 =~ ^[0-9]+$ ]]; then
    return 2
  fi
  ((SECONDS < $1))
}

run_bounded_readiness_command() {
  if [[ $# -lt 2 || ! $1 =~ ^[0-9]+$ ]]; then
    return 2
  fi
  local readiness_deadline=$1
  shift
  if ! readiness_time_remains "$readiness_deadline"; then
    return 124
  fi
  timeout \
    --signal=TERM \
    --kill-after="${desktop_bounded_subprocess_kill_grace_seconds}s" \
    "${desktop_bounded_subprocess_timeout_seconds}s" \
    "$@"
}

process_is_running() {
  if [[ $# -ne 1 || ! $1 =~ ^[0-9]+$ || $1 -le 1 ]]; then
    return 1
  fi
  local pid=$1
  local stat_line
  local stat_tail
  local state_code
  IFS= read -r stat_line 2>/dev/null <"/proc/$pid/stat" || return 1
  case "$stat_line" in
    "$pid ("*) ;;
    *) return 1 ;;
  esac
  stat_tail=${stat_line##*) }
  if [[ $stat_tail == "$stat_line" ]]; then
    return 1
  fi
  state_code=${stat_tail%% *}
  [[ $state_code != X && $state_code != x && $state_code != Z ]]
}

terminate_and_reap() {
  if [[ $# -ne 2 || ! $1 =~ ^[0-9]+$ || $1 -le 1 ]]; then
    return 2
  fi
  local pid=$1
  local label=$2
  local attempt
  local wait_status=0
  if process_is_running "$pid"; then
    kill -TERM "$pid" 2>/dev/null || true
    for ((attempt = 0; attempt < 50; attempt += 1)); do
      if ! process_is_running "$pid"; then
        break
      fi
      sleep 0.1
    done
  fi
  if process_is_running "$pid"; then
    kill -KILL "$pid" 2>/dev/null || true
    for ((attempt = 0; attempt < 20; attempt += 1)); do
      if ! process_is_running "$pid"; then
        break
      fi
      sleep 0.1
    done
  fi
  if process_is_running "$pid"; then
    echo "$label did not stop before its bounded cleanup deadline" >&2
    return 1
  fi
  wait "$pid" || wait_status=$?
  printf 'debian-install-smoke: reaped %s pid=%s status=%s\n' \
    "$label" "$pid" "$wait_status"
}

process_stat_snapshot() {
  if [[ $# -ne 1 || ! $1 =~ ^[0-9]+$ || $1 -le 1 ]]; then
    return 2
  fi
  local pid=$1
  local stat_line
  local stat_tail
  local state
  local process_group
  local session
  local start_time
  local -a stat_fields=()
  IFS= read -r stat_line 2>/dev/null <"/proc/$pid/stat" || return 1
  case "$stat_line" in
    "$pid ("*) ;;
    *) return 1 ;;
  esac
  stat_tail=${stat_line##*) }
  if [[ $stat_tail == "$stat_line" ]]; then
    return 1
  fi
  read -r -a stat_fields <<<"$stat_tail"
  if [[ ${#stat_fields[@]} -lt 20 ]]; then
    return 1
  fi
  state=${stat_fields[0]}
  process_group=${stat_fields[2]}
  session=${stat_fields[3]}
  start_time=${stat_fields[19]}
  if [[ ! $state =~ ^[A-Za-z]$ \
    || ! $process_group =~ ^[0-9]+$ \
    || ! $session =~ ^[0-9]+$ \
    || ! $start_time =~ ^[0-9]+$ ]]; then
    return 1
  fi
  printf '%s %s %s %s\n' "$state" "$process_group" "$session" "$start_time"
}

application_group_has_live_members() {
  if [[ $# -ne 2 \
    || ! $1 =~ ^[0-9]+$ \
    || ! $2 =~ ^[0-9]+$ \
    || $1 -le 1 \
    || $1 != "$2" ]]; then
    return 2
  fi
  local expected_process_group=$1
  local expected_session=$2
  local stat_path
  local member_pid
  local member_state
  local member_process_group
  local member_session
  local member_start_time
  for stat_path in /proc/[0-9]*/stat; do
    member_pid=${stat_path#/proc/}
    member_pid=${member_pid%/stat}
    if IFS=' ' read -r \
      member_state \
      member_process_group \
      member_session \
      member_start_time \
      < <(process_stat_snapshot "$member_pid"); then
      if [[ $member_process_group == "$expected_process_group" \
        && $member_session == "$expected_session" \
        && -n $member_start_time \
        && $member_state != Z \
        && $member_state != X \
        && $member_state != x ]]; then
        return 0
      fi
    fi
  done
  return 1
}

application_group_identity_is_verified() {
  if [[ $# -ne 4 \
    || ! $1 =~ ^[0-9]+$ \
    || ! $2 =~ ^[0-9]+$ \
    || ! $3 =~ ^[0-9]+$ \
    || ! $4 =~ ^[0-9]+$ \
    || $1 -le 1 \
    || $1 != "$2" \
    || $1 != "$3" ]]; then
    return 2
  fi
  local application_pid=$1
  local expected_process_group=$2
  local expected_session=$3
  local expected_start_time=$4
  local leader_state
  local leader_process_group
  local leader_session
  local leader_start_time
  if IFS=' ' read -r \
    leader_state \
    leader_process_group \
    leader_session \
    leader_start_time \
    < <(process_stat_snapshot "$application_pid"); then
    [[ $leader_process_group == "$expected_process_group" \
      && $leader_session == "$expected_session" \
      && $leader_start_time == "$expected_start_time" ]]
    return
  fi

  # Linux retains the private SID/PGID identity while any live member remains.
  # This permits cleanup after Bash has collected the leader status, without
  # treating an unrelated reused leader PID as the recorded application.
  application_group_has_live_members \
    "$expected_process_group" "$expected_session"
}

assert_application_group_leader_identity() {
  if [[ $# -ne 4 ]]; then
    return 2
  fi
  local application_pid=$1
  local expected_process_group=$2
  local expected_session=$3
  local expected_start_time=$4
  local leader_state
  local leader_process_group
  local leader_session
  local leader_start_time
  if ! IFS=' ' read -r \
    leader_state \
    leader_process_group \
    leader_session \
    leader_start_time \
    < <(process_stat_snapshot "$application_pid"); then
    return 1
  fi
  [[ $leader_state != Z \
    && $leader_state != X \
    && $leader_state != x \
    && $leader_process_group == "$expected_process_group" \
    && $leader_session == "$expected_session" \
    && $leader_start_time == "$expected_start_time" \
    && $application_pid == "$expected_process_group" \
    && $application_pid == "$expected_session" ]]
}

signal_verified_application_group() {
  if [[ $# -ne 5 || ! $5 =~ ^(TERM|KILL)$ ]]; then
    return 2
  fi
  local application_pid=$1
  local expected_process_group=$2
  local expected_session=$3
  local expected_start_time=$4
  local signal_name=$5
  if ! application_group_identity_is_verified \
    "$application_pid" \
    "$expected_process_group" \
    "$expected_session" \
    "$expected_start_time"; then
    echo "refusing broad -$signal_name for unverified application PGID $expected_process_group" >&2
    return 1
  fi
  if ! application_group_has_live_members \
    "$expected_process_group" "$expected_session"; then
    return 0
  fi
  if ! kill "-$signal_name" -- "-$expected_process_group" 2>/dev/null \
    && application_group_has_live_members \
      "$expected_process_group" "$expected_session"; then
    echo "failed to send $signal_name to application PGID $expected_process_group" >&2
    return 1
  fi
}

wait_for_application_group_quiescence() {
  if [[ $# -ne 3 \
    || ! $1 =~ ^[0-9]+$ \
    || ! $2 =~ ^[0-9]+$ \
    || ! $3 =~ ^[0-9]+$ \
    || $1 -le 1 \
    || $1 != "$2" \
    || $3 -le 0 ]]; then
    return 2
  fi
  local expected_process_group=$1
  local expected_session=$2
  local maximum_attempts=$3
  local attempt
  local empty_observations=0
  for ((attempt = 0; attempt < maximum_attempts; attempt += 1)); do
    if application_group_has_live_members \
      "$expected_process_group" "$expected_session"; then
      empty_observations=0
    else
      empty_observations=$((empty_observations + 1))
      if [[ $empty_observations -ge 2 ]]; then
        return 0
      fi
    fi
    sleep 0.1
  done
  ! application_group_has_live_members \
    "$expected_process_group" "$expected_session"
}

terminate_application_group_and_reap() {
  if [[ $# -ne 5 \
    || ! $1 =~ ^[0-9]+$ \
    || ! $2 =~ ^[0-9]+$ \
    || ! $3 =~ ^[0-9]+$ \
    || ! $4 =~ ^[0-9]+$ \
    || $1 -le 1 \
    || $1 != "$2" \
    || $1 != "$3" ]]; then
    return 2
  fi
  local application_pid=$1
  local expected_process_group=$2
  local expected_session=$3
  local expected_start_time=$4
  local label=$5
  local wait_status=0

  if application_group_has_live_members \
    "$expected_process_group" "$expected_session"; then
    signal_verified_application_group \
      "$application_pid" \
      "$expected_process_group" \
      "$expected_session" \
      "$expected_start_time" \
      TERM \
      || return 1
    if ! wait_for_application_group_quiescence \
      "$expected_process_group" "$expected_session" 50; then
      signal_verified_application_group \
        "$application_pid" \
        "$expected_process_group" \
        "$expected_session" \
        "$expected_start_time" \
        KILL \
        || return 1
      if ! wait_for_application_group_quiescence \
        "$expected_process_group" "$expected_session" 20; then
        echo "$label PGID $expected_process_group retained live members" >&2
        return 1
      fi
    fi
  fi
  if application_group_has_live_members \
    "$expected_process_group" "$expected_session"; then
    echo "$label PGID $expected_process_group did not become quiescent" >&2
    return 1
  fi
  wait "$application_pid" || wait_status=$?
  printf 'debian-install-smoke: reaped %s pid=%s pgid=%s sid=%s status=%s\n' \
    "$label" \
    "$application_pid" \
    "$expected_process_group" \
    "$expected_session" \
    "$wait_status"
}

desktop_session_probe() {
  if [[ $# -ne 2 || ! $1 =~ ^(installed|upgraded)$ || ! $2 =~ ^[0-9a-f]{64}$ ]]; then
    echo "internal desktop probe contract is invalid" >&2
    return 2
  fi
  local readiness_deadline
  readiness_deadline=$((SECONDS + desktop_readiness_budget_seconds))
  local probe_label=$1
  local expected_sha=$2
  local probe_root
  local runtime
  local dbus_address_file
  local display_number_file
  local dbus_address
  local display_number
  local display
  local dbus_pid=
  local xvfb_pid=
  local application_pid=
  local application_process_group=
  local application_session=
  local application_start_time=
  local cleanup_status
  local probe_status
  local attempt
  local executable
  local process_state=
  local process_group=
  local process_session=
  local process_start_time=
  local window
  local observed_title=
  local observed_pid=
  local -a windows=()

  probe_root=$(mktemp -d "$home/.pokecon-desktop-$probe_label.XXXXXXXX")
  runtime="$probe_root/runtime"
  dbus_address_file="$probe_root/dbus.address"
  display_number_file="$probe_root/display.number"
  mkdir -p "$runtime"
  chmod 0700 "$probe_root" "$runtime"

  # Invoked explicitly on success and indirectly by the EXIT trap on failure.
  desktop_cleanup() {
    probe_status=$?
    trap - EXIT HUP INT TERM
    set +e
    cleanup_status=0
    if [[ -n $application_pid ]]; then
      if [[ -n $application_process_group \
        && -n $application_session \
        && -n $application_start_time ]]; then
        terminate_application_group_and_reap \
          "$application_pid" \
          "$application_process_group" \
          "$application_session" \
          "$application_start_time" \
          "$probe_label application" \
          || cleanup_status=1
      else
        echo "$probe_label application did not establish a verified private process group" >&2
        terminate_and_reap "$application_pid" "$probe_label unverified application" \
          || cleanup_status=1
        cleanup_status=1
      fi
    fi
    if [[ -n $xvfb_pid ]]; then
      terminate_and_reap "$xvfb_pid" "$probe_label Xvfb" || cleanup_status=1
    fi
    if [[ -n $dbus_pid ]]; then
      terminate_and_reap "$dbus_pid" "$probe_label D-Bus" || cleanup_status=1
    fi
    if [[ $probe_status -ne 0 ]]; then
      for evidence in \
        "$probe_root/application.log" \
        "$probe_root/xvfb.log" \
        "$probe_root/dbus.log" \
        "$probe_root/window.xprop" \
        "$probe_root/window.xwininfo"; do
        if [[ -s $evidence ]]; then
          printf '%s\n' "--- $(basename -- "$evidence") (bounded) ---" >&2
          head -c 16384 -- "$evidence" >&2 || true
          printf '\n' >&2
        fi
      done
    fi
    rm -rf -- "$probe_root" || cleanup_status=1
    if [[ $probe_status -eq 0 && $cleanup_status -ne 0 ]]; then
      probe_status=$cleanup_status
    fi
    exit "$probe_status"
  }
  trap desktop_cleanup EXIT
  trap 'exit 129' HUP
  trap 'exit 130' INT
  trap 'exit 143' TERM

  dbus-daemon \
    --session \
    --nofork \
    --nopidfile \
    --print-address=3 \
    3>"$dbus_address_file" \
    >"$probe_root/dbus.log" 2>&1 &
  dbus_pid=$!
  for ((attempt = 0; attempt < 100; attempt += 1)); do
    if ! readiness_time_remains "$readiness_deadline"; then
      break
    fi
    if [[ -s $dbus_address_file ]] && process_is_running "$dbus_pid"; then
      break
    fi
    sleep 0.1
  done
  if [[ ! -s $dbus_address_file ]] || ! process_is_running "$dbus_pid"; then
    echo "$probe_label D-Bus session did not become ready" >&2
    return 1
  fi
  IFS= read -r dbus_address <"$dbus_address_file"
  if [[ $dbus_address != unix:* ]]; then
    echo "$probe_label D-Bus session published an invalid address" >&2
    return 1
  fi

  Xvfb \
    -displayfd 3 \
    -screen 0 1440x900x24 \
    -nolisten tcp \
    -ac \
    3>"$display_number_file" \
    >"$probe_root/xvfb.log" 2>&1 &
  xvfb_pid=$!
  for ((attempt = 0; attempt < 100; attempt += 1)); do
    if ! readiness_time_remains "$readiness_deadline"; then
      break
    fi
    if [[ -s $display_number_file ]] && process_is_running "$xvfb_pid"; then
      break
    fi
    sleep 0.1
  done
  if [[ ! -s $display_number_file ]] || ! process_is_running "$xvfb_pid"; then
    echo "$probe_label Xvfb did not publish a display" >&2
    return 1
  fi
  IFS= read -r display_number <"$display_number_file"
  if [[ ! $display_number =~ ^[0-9]+$ ]]; then
    echo "$probe_label Xvfb published an invalid display number" >&2
    return 1
  fi
  display=":$display_number"
  for ((attempt = 0; attempt < 100; attempt += 1)); do
    if ! readiness_time_remains "$readiness_deadline"; then
      break
    fi
    if run_bounded_readiness_command \
      "$readiness_deadline" \
      env DISPLAY="$display" xwininfo -root \
      >"$probe_root/root.xwininfo" 2>/dev/null; then
      break
    fi
    sleep 0.1
  done
  if ! run_bounded_readiness_command \
    "$readiness_deadline" \
    env DISPLAY="$display" xwininfo -root >/dev/null 2>&1; then
    echo "$probe_label Xvfb display did not become queryable" >&2
    return 1
  fi

  assert_application_sha256 "$expected_sha" "$probe_label desktop launch"
  env -i \
    HOME="$home" \
    USER="$app_user" \
    LOGNAME="$app_user" \
    PATH=/usr/bin:/bin \
    XDG_CONFIG_HOME="$config" \
    XDG_DATA_HOME="$data" \
    XDG_CACHE_HOME="$cache" \
    XDG_STATE_HOME="$state" \
    XDG_RUNTIME_DIR="$runtime" \
    DISPLAY="$display" \
    DBUS_SESSION_BUS_ADDRESS="$dbus_address" \
    GDK_BACKEND=x11 \
    GSETTINGS_BACKEND=memory \
    WEBKIT_DISABLE_DMABUF_RENDERER=1 \
    LIBGL_ALWAYS_SOFTWARE=1 \
    NO_AT_BRIDGE=1 \
    RUST_LOG=info \
    setsid -- \
    "$application" \
    --ui desktop \
    --bind-address 127.0.0.1 \
    --port 0 \
    --dynamic-config-language none \
    --disable-compositing true \
    --ui-desktop-close-behavior shutdown \
    >"$probe_root/application.log" 2>&1 &
  application_pid=$!

  executable=
  for ((attempt = 0; attempt < 100; attempt += 1)); do
    if ! readiness_time_remains "$readiness_deadline"; then
      break
    fi
    executable=$(readlink -f -- "/proc/$application_pid/exe" 2>/dev/null || true)
    process_state=
    process_group=
    process_session=
    process_start_time=
    if IFS=' ' read -r \
      process_state \
      process_group \
      process_session \
      process_start_time \
      < <(process_stat_snapshot "$application_pid"); then
      if [[ -z $application_start_time ]]; then
        application_start_time=$process_start_time
      elif [[ $process_start_time != "$application_start_time" ]]; then
        echo "$probe_label application PID identity changed during startup" >&2
        return 1
      fi
      if [[ $process_group == "$application_pid" \
        && $process_session == "$application_pid" ]]; then
        application_process_group=$process_group
        application_session=$process_session
        if [[ $executable == "$application" \
          && $process_state != Z \
          && $process_state != X \
          && $process_state != x ]]; then
          break
        fi
      fi
    fi
    sleep 0.1
  done
  if [[ $executable != "$application" \
    || $application_process_group != "$application_pid" \
    || $application_session != "$application_pid" \
    || -z $application_start_time ]] \
    || ! assert_application_group_leader_identity \
      "$application_pid" \
      "$application_process_group" \
      "$application_session" \
      "$application_start_time"; then
    echo "$probe_label desktop process did not become the private leader running $application" >&2
    return 1
  fi

  window=
  while readiness_time_remains "$readiness_deadline"; do
    windows=()
    mapfile -t windows < <(
      run_bounded_readiness_command \
        "$readiness_deadline" \
        env DISPLAY="$display" \
        xdotool search --onlyvisible --name '^PokeCon Controller$' \
        2>/dev/null || true
    )
    if [[ ${#windows[@]} -eq 1 ]]; then
      window=${windows[0]}
      observed_title=$(
        run_bounded_readiness_command \
          "$readiness_deadline" \
          env DISPLAY="$display" xdotool getwindowname "$window"
      ) || observed_title=
      observed_pid=$(
        run_bounded_readiness_command \
          "$readiness_deadline" \
          env DISPLAY="$display" xdotool getwindowpid "$window"
      ) || observed_pid=
      if [[ $observed_title == 'PokeCon Controller' \
        && $observed_pid == "$application_pid" ]] \
        && run_bounded_readiness_command \
          "$readiness_deadline" \
          env DISPLAY="$display" \
          xprop -id "$window" _NET_WM_PID _NET_WM_NAME WM_NAME \
          >"$probe_root/window.xprop" \
        && grep -E "_NET_WM_PID.*=[[:space:]]*${application_pid}[[:space:]]*$" \
          "$probe_root/window.xprop" >/dev/null \
        && run_bounded_readiness_command \
          "$readiness_deadline" \
          env DISPLAY="$display" xwininfo -id "$window" \
          >"$probe_root/window.xwininfo" \
        && grep -F 'Map State: IsViewable' \
          "$probe_root/window.xwininfo" >/dev/null \
        && grep -F '"PokeCon Controller"' \
          "$probe_root/window.xwininfo" >/dev/null; then
        break
      fi
    fi
    if ! process_is_running "$application_pid" \
      || ! process_is_running "$xvfb_pid" \
      || ! process_is_running "$dbus_pid"; then
      break
    fi
    sleep 0.1
  done
  if [[ ${#windows[@]} -ne 1 || $window != "${windows[0]:-}" \
    || $observed_title != 'PokeCon Controller' \
    || $observed_pid != "$application_pid" ]]; then
    echo "$probe_label desktop did not expose exactly one owned visible native window" >&2
    return 1
  fi
  if ! grep -E "_NET_WM_PID.*=[[:space:]]*${application_pid}[[:space:]]*$" \
    "$probe_root/window.xprop" >/dev/null \
    || ! grep -F 'Map State: IsViewable' \
      "$probe_root/window.xwininfo" >/dev/null \
    || ! grep -F '"PokeCon Controller"' \
      "$probe_root/window.xwininfo" >/dev/null \
    || ! process_is_running "$application_pid" \
    || ! process_is_running "$xvfb_pid" \
    || ! process_is_running "$dbus_pid"; then
    echo "$probe_label desktop window evidence was not stable" >&2
    return 1
  fi
  executable=$(readlink -f -- "/proc/$application_pid/exe")
  if [[ $executable != "$application" ]] \
    || ! assert_application_group_leader_identity \
      "$application_pid" \
      "$application_process_group" \
      "$application_session" \
      "$application_start_time"; then
    echo "$probe_label native window owner no longer leads its private application session" >&2
    return 1
  fi
  assert_application_sha256 "$expected_sha" "$probe_label desktop evidence"
  printf 'debian-install-smoke: %s desktop pid=%s pgid=%s sid=%s window=%s title=%q sha256=%s\n' \
    "$probe_label" \
    "$application_pid" \
    "$application_process_group" \
    "$application_session" \
    "$window" \
    "$observed_title" \
    "$expected_sha"
  grep -E '_NET_WM_PID|_NET_WM_NAME|WM_NAME' "$probe_root/window.xprop"
  grep -E 'Window id:|Map State:' "$probe_root/window.xwininfo"
  desktop_cleanup
}

if [[ ${1:-} == __desktop_session_probe ]]; then
  shift
  desktop_session_probe "$@"
  exit $?
fi

run_web_probe() {
  if [[ $# -ne 1 ]]; then
    return 2
  fi
  local probe_label=$1
  runuser -u "$app_user" -- env -i \
    HOME="$home" \
    USER="$app_user" \
    LOGNAME="$app_user" \
    PATH=/usr/bin:/bin \
    XDG_CONFIG_HOME="$config" \
    XDG_DATA_HOME="$data" \
    XDG_CACHE_HOME="$cache" \
    XDG_STATE_HOME="$state" \
    RUST_LOG=info \
    timeout --signal=TERM --kill-after=5s 60s \
    "$application" --ui web --exit-after-startup
  printf 'debian-install-smoke: %s Web startup passed\n' "$probe_label"
}

run_desktop_probe() {
  if [[ $# -ne 2 ]]; then
    return 2
  fi
  local probe_label=$1
  local expected_sha=$2
  runuser -u "$app_user" -- env -i \
    HOME="$home" \
    USER="$app_user" \
    LOGNAME="$app_user" \
    PATH=/usr/bin:/bin \
    XDG_CONFIG_HOME="$config" \
    XDG_DATA_HOME="$data" \
    XDG_CACHE_HOME="$cache" \
    XDG_STATE_HOME="$state" \
    RUST_LOG=info \
    timeout --signal=TERM --kill-after=30s \
    "${desktop_outer_term_seconds}s" \
    "$script_path" __desktop_session_probe "$probe_label" "$expected_sha"
}

run_probe() {
  if [[ $# -ne 2 ]]; then
    return 2
  fi
  local probe_label=$1
  local expected_sha=$2
  assert_application_sha256 "$expected_sha" "$probe_label before Web startup"
  run_web_probe "$probe_label"
  assert_application_sha256 "$expected_sha" "$probe_label after Web startup"
  run_desktop_probe "$probe_label" "$expected_sha"
  assert_application_sha256 "$expected_sha" "$probe_label after desktop startup"
  printf 'debian-install-smoke: %s Web+Desktop probe passed sha256=%s\n' \
    "$probe_label" "$expected_sha"
}

installed_sha=$(application_sha256)
run_probe installed "$installed_sha"
runuser -u "$app_user" -- mkdir -p "$config/pokecon/profiles/default"
runuser -u "$app_user" -- touch "$config/pokecon/profiles/default/package-smoke.preserved"

dpkg --install "$package"
test -f "$config/pokecon/profiles/default/package-smoke.preserved"
upgraded_sha=$(application_sha256)
if [[ $upgraded_sha != "$installed_sha" ]]; then
  echo "upgrade changed the installed executable SHA-256: expected $installed_sha, got $upgraded_sha" >&2
  exit 1
fi
run_probe upgraded "$installed_sha"

package_name=$(dpkg-deb --field "$package" Package)
dpkg --remove "$package_name"
test ! -e "$application"
test -f "$config/pokecon/profiles/default/package-smoke.preserved"
