from __future__ import annotations

import re
from pathlib import Path

REPOSITORY = Path(__file__).resolve().parents[2]


def section(source: str, start: str, end: str) -> str:
    start_index = source.index(start)
    end_index = source.index(end, start_index)
    return source[start_index:end_index]


def readonly_integer(source: str, name: str) -> int:
    match = re.search(rf"^readonly {re.escape(name)}=([0-9]+)$", source, re.MULTILINE)
    assert match is not None
    return int(match.group(1))


def test_debian_image_allows_only_the_desktop_probe_packages() -> None:
    launcher = (REPOSITORY / "scripts/release/debian_install_smoke.sh").read_text(
        encoding="utf-8"
    )

    assert (
        "ubuntu@sha256:"
        "52df9b1ee71626e0088f7d400d5c6b5f7bb916f8f0c82b474289a4ece6cf3faf" in launcher
    )
    dockerfile = section(launcher, '"$context" <<EOF\n', "\nEOF\n")
    assert dockerfile.count("apt-get update") == 1
    assert dockerfile.count("apt-get install --yes --no-install-recommends") == 1
    assert dockerfile.count("rm -rf /var/lib/apt/lists/*") == 1
    helper_packages = re.findall(
        r"^      '([^']+)' \\$",
        dockerfile,
        flags=re.MULTILINE,
    )
    assert helper_packages == [
        "dbus-daemon",
        "libgl1-mesa-dri",
        "x11-utils",
        "xdotool",
        "xvfb",
    ]
    assert all(
        re.fullmatch(r"[a-z0-9][a-z0-9+.-]*", package) for package in helper_packages
    )
    assert not any(
        operator in package for package in helper_packages for operator in "=<>"
    )
    container_package = "/" + "tmp" + "/pokecon.deb"
    assert dockerfile.count(container_package) == 2
    assert "ENTRYPOINT" in dockerfile
    assert launcher.count("--network none") == 1
    assert launcher.index("apt-get install") < launcher.index("--network none")


def test_debian_runtime_owns_cid_cleanup_and_fits_outer_timeout() -> None:
    launcher = (REPOSITORY / "scripts/release/debian_install_smoke.sh").read_text(
        encoding="utf-8"
    )
    smoke = (REPOSITORY / "scripts/release/debian_container_smoke.sh").read_text(
        encoding="utf-8"
    )
    bounded_cleanup = section(
        launcher,
        "run_bounded_docker_cleanup() {\n",
        "\n}\n\nif [[ $# -ne 1 ]]",
    )
    cleanup = section(launcher, "\ncleanup() {\n", "\n}\ntrap cleanup EXIT")
    runtime = launcher[launcher.index("\ntimeout \\\n", launcher.index("\nEOF\n")) :]

    repetitions = readonly_integer(launcher, "docker_runtime_probe_repetitions")
    web_hard_seconds = readonly_integer(launcher, "docker_runtime_web_hard_seconds")
    desktop_hard_seconds = readonly_integer(
        launcher, "docker_runtime_desktop_hard_seconds"
    )
    margin_seconds = readonly_integer(launcher, "docker_runtime_margin_seconds")
    outer_term_seconds = readonly_integer(launcher, "docker_runtime_outer_term_seconds")
    outer_kill_grace_seconds = readonly_integer(
        launcher, "docker_runtime_outer_kill_grace_seconds"
    )

    assert repetitions == 2
    assert web_hard_seconds == 65
    assert desktop_hard_seconds == 120
    assert margin_seconds == 30
    assert outer_term_seconds == 420
    assert outer_kill_grace_seconds == 30
    hard_probe_seconds = (
        repetitions * web_hard_seconds + repetitions * desktop_hard_seconds
    )
    required_seconds = hard_probe_seconds + margin_seconds
    assert hard_probe_seconds == 370
    assert required_seconds == 400
    assert required_seconds < outer_term_seconds
    assert (
        "readonly docker_runtime_probe_budget_seconds=$((\n"
        "  docker_runtime_probe_repetitions * docker_runtime_web_hard_seconds\n"
        "  + docker_runtime_probe_repetitions * docker_runtime_desktop_hard_seconds\n"
        "))" in launcher
    )
    assert (
        "readonly docker_runtime_required_seconds=$((\n"
        "  docker_runtime_probe_budget_seconds + docker_runtime_margin_seconds\n"
        "))" in launcher
    )
    assert (
        "docker_runtime_required_seconds >= docker_runtime_outer_term_seconds"
        in launcher
    )

    web_probe = section(smoke, "run_web_probe() {\n", "\n}\n\nrun_desktop_probe()")
    web_timeout = re.search(
        r"timeout --signal=TERM --kill-after=([0-9]+)s ([0-9]+)s",
        web_probe,
    )
    assert web_timeout is not None
    assert int(web_timeout.group(1)) + int(web_timeout.group(2)) == web_hard_seconds
    desktop_probe = section(smoke, "run_desktop_probe() {\n", "\n}\n\nrun_probe()")
    desktop_outer_term_seconds = readonly_integer(smoke, "desktop_outer_term_seconds")
    desktop_timeout = re.search(
        r"timeout --signal=TERM --kill-after=([0-9]+)s \\\n"
        r'    "\$\{desktop_outer_term_seconds\}s"',
        desktop_probe,
    )
    assert desktop_timeout is not None
    assert (
        int(desktop_timeout.group(1)) + desktop_outer_term_seconds
        == desktop_hard_seconds
    )

    cleanup_timeout_seconds = readonly_integer(
        launcher, "docker_cleanup_operation_timeout_seconds"
    )
    cleanup_kill_grace_seconds = readonly_integer(
        launcher, "docker_cleanup_operation_kill_grace_seconds"
    )
    assert cleanup_timeout_seconds == 10
    assert cleanup_kill_grace_seconds == 2
    assert '--kill-after="${docker_cleanup_operation_kill_grace_seconds}s"' in (
        bounded_cleanup
    )
    assert '"${docker_cleanup_operation_timeout_seconds}s"' in bounded_cleanup

    context_creation = "context=$(mktemp -d -t pokecon-debian-install.XXXXXXXXXXXXXXXX)"
    container_name = (
        'container_name="pokecon-package-smoke-${package_sha:0:20}-${random_suffix}"'
    )
    image_name = 'image="pokecon-package-smoke:${package_sha:0:20}-${random_suffix}"'
    assert context_creation in launcher
    assert 'container_id_file="$context/container.id"' in launcher
    assert "random_suffix=${context_basename#pokecon-debian-install.}" in launcher
    assert "random_suffix=${random_suffix,,}" in launcher
    assert "! $random_suffix =~ ^[a-z0-9]{16}$" in launcher
    assert container_name in launcher
    assert image_name in launcher
    assert launcher.index(context_creation) < launcher.index(container_name)
    assert "! $package_sha =~ ^[0-9a-f]{64}$" in launcher
    assert (
        "! $container_name =~ "
        "^pokecon-package-smoke-[0-9a-f]{20}-[a-z0-9]{16}$" in launcher
    )
    assert "! $image =~ ^pokecon-package-smoke:[0-9a-f]{20}-[a-z0-9]{16}$" in launcher
    assert 'container_name="pokecon-package-smoke-${package_sha:0:20}-$$"' not in (
        launcher
    )

    assert "[[ -f $container_id_file && ! -L $container_id_file ]]" in cleanup
    assert 'mapfile -t container_id_lines <"$container_id_file"' in cleanup
    assert "[[ ${#container_id_lines[@]} -eq 1 ]]" in cleanup
    assert "[[ $container_id =~ ^[0-9a-f]{64}$ ]]" in cleanup
    assert "container inspect --format '{{.Id}}' \"$container_id\"" in cleanup
    assert '[[ $inspected_id == "$container_id" ]]' in cleanup
    assert 'container rm --force "$container_id"' in cleanup
    assert 'container rm --force "$container_name"' not in launcher
    assert '"$container_name"' not in cleanup
    assert '"$docker"' not in cleanup
    assert cleanup.count("run_bounded_docker_cleanup") == 3
    assert (
        cleanup.index("container inspect")
        < cleanup.index('container rm --force "$container_id"')
        < cleanup.index('image rm "$image"')
        < cleanup.index('rm -rf -- "$context"')
    )
    assert '--kill-after="${docker_runtime_outer_kill_grace_seconds}s"' in runtime
    assert '"${docker_runtime_outer_term_seconds}s"' in runtime
    assert runtime.index("timeout \\\n") < runtime.index('"$docker" run \\\n')
    assert runtime.count("--rm") == 1
    assert runtime.count('--name "$container_name"') == 1
    assert runtime.count('--cidfile "$container_id_file"') == 1
    assert runtime.count("--network none") == 1


def test_installed_and_upgraded_debian_binary_prove_web_and_desktop_modes() -> None:
    smoke = (REPOSITORY / "scripts/release/debian_container_smoke.sh").read_text(
        encoding="utf-8"
    )
    web_probe = section(smoke, "run_web_probe() {\n", "\n}\n\nrun_desktop_probe()")
    desktop_probe = section(
        smoke,
        "desktop_session_probe() (\n",
        "\n)\n\nif [[ ${1:-} == __desktop_session_probe ]]",
    )
    group_member_scan = section(
        smoke,
        "application_group_has_live_members() {\n",
        "\n}\n\napplication_group_identity_is_verified()",
    )
    group_signal = section(
        smoke,
        "signal_verified_application_group() {\n",
        "\n}\n\nwait_for_application_group_quiescence()",
    )
    group_cleanup = section(
        smoke,
        "terminate_application_group_and_reap() {\n",
        "\n}\n\ndesktop_session_probe() (",
    )
    combined_probe = section(smoke, "run_probe() {\n", "\n}\n\ninstalled_sha=")

    assert "application=/usr/bin/pokecon" in smoke
    assert smoke.count('run_probe installed "$installed_sha"') == 1
    assert smoke.count('run_probe upgraded "$installed_sha"') == 1
    assert (
        smoke.index('run_probe installed "$installed_sha"')
        < smoke.index('dpkg --install "$package"')
        < smoke.index('run_probe upgraded "$installed_sha"')
    )
    assert 'if [[ $upgraded_sha != "$installed_sha" ]]' in smoke
    assert combined_probe.count("run_web_probe") == 1
    assert combined_probe.count("run_desktop_probe") == 1
    assert combined_probe.count("assert_application_sha256") == 3
    assert (
        combined_probe.index("before Web startup")
        < combined_probe.index("run_web_probe")
        < combined_probe.index("after Web startup")
        < combined_probe.index("run_desktop_probe")
        < combined_probe.index("after desktop startup")
    )

    assert '"$application" --ui web --exit-after-startup' in web_probe
    assert "--ui desktop" in desktop_probe
    assert "--exit-after-startup" not in desktop_probe
    assert "dbus-daemon \\\n    --session \\\n    --nofork" in desktop_probe
    assert "Xvfb \\\n    -displayfd 3" in desktop_probe
    assert "-nolisten tcp" in desktop_probe
    assert 'DBUS_SESSION_BUS_ADDRESS="$dbus_address"' in desktop_probe
    assert 'DISPLAY="$display"' in desktop_probe
    assert 'setsid -- \\\n    "$application"' in desktop_probe
    assert "--port 8020" in desktop_probe
    assert "--port 0" not in desktop_probe
    assert "application_pid=$!" in desktop_probe
    assert '"/proc/$application_pid/exe"' in desktop_probe
    assert "process_stat_snapshot()" in smoke
    assert "process_group=${stat_fields[2]}" in smoke
    assert "session=${stat_fields[3]}" in smoke
    assert "start_time=${stat_fields[19]}" in smoke
    assert 'process_group == "$application_pid"' in desktop_probe
    assert 'process_session == "$application_pid"' in desktop_probe
    assert desktop_probe.count("assert_application_group_leader_identity") == 2

    exact_window_search = "xdotool search --onlyvisible --name '^PokeCon Controller$'"
    assert desktop_probe.count(exact_window_search) == 1
    assert "[[ ${#windows[@]} -eq 1 ]]" in desktop_probe
    assert 'xdotool getwindowname "$window"' in desktop_probe
    assert 'xdotool getwindowpid "$window"' in desktop_probe
    assert "observed_title == 'PokeCon Controller'" in desktop_probe
    assert 'observed_pid == "$application_pid"' in desktop_probe
    assert 'xprop -id "$window" _NET_WM_PID _NET_WM_NAME WM_NAME' in desktop_probe
    assert "_NET_WM_PID.*=" in desktop_probe
    assert 'xwininfo -id "$window"' in desktop_probe
    assert "Map State: IsViewable" in desktop_probe
    assert '"PokeCon Controller"' in desktop_probe

    assert desktop_probe.count("assert_application_sha256") == 2
    assert desktop_probe.count("process_is_running") >= 8
    assert "terminate_application_group_and_reap \\\n" in desktop_probe
    assert '"$application_process_group"' in desktop_probe
    assert '"$application_session"' in desktop_probe
    assert '"$application_start_time"' in desktop_probe
    assert 'terminate_and_reap "$xvfb_pid"' in desktop_probe
    assert 'terminate_and_reap "$dbus_pid"' in desktop_probe
    assert "member_state != Z" in group_member_scan
    assert "member_state != X" in group_member_scan
    assert "member_state != x" in group_member_scan
    assert group_signal.count("application_group_identity_is_verified") == 1
    assert group_signal.count('kill "-$signal_name" -- "-$expected_process_group"') == 1
    assert group_signal.index("application_group_identity_is_verified") < (
        group_signal.index('kill "-$signal_name"')
    )
    assert group_cleanup.count("signal_verified_application_group") == 2
    assert group_cleanup.count("wait_for_application_group_quiescence") == 2
    assert (
        group_cleanup.index("TERM")
        < group_cleanup.index("wait_for_application_group_quiescence")
        < group_cleanup.index("KILL")
        < group_cleanup.rindex("wait_for_application_group_quiescence")
        < group_cleanup.rindex("application_group_has_live_members")
        < group_cleanup.index('wait "$application_pid"')
    )
    assert desktop_probe.rstrip().endswith("desktop_cleanup")


def test_debian_desktop_readiness_budget_finishes_before_outer_term() -> None:
    smoke = (REPOSITORY / "scripts/release/debian_container_smoke.sh").read_text(
        encoding="utf-8"
    )
    desktop_probe = section(
        smoke,
        "desktop_session_probe() (\n",
        "\n)\n\nif [[ ${1:-} == __desktop_session_probe ]]",
    )
    bounded_command = section(
        smoke,
        "run_bounded_readiness_command() {\n",
        "\n}\n\nprocess_is_running()",
    )
    direct_cleanup = section(
        smoke,
        "terminate_and_reap() {\n",
        "\n}\n\nprocess_stat_snapshot()",
    )
    group_cleanup = section(
        smoke,
        "terminate_application_group_and_reap() {\n",
        "\n}\n\ndesktop_session_probe() (",
    )
    group_wait = section(
        smoke,
        "wait_for_application_group_quiescence() {\n",
        "\n}\n\nterminate_application_group_and_reap()",
    )
    outer_probe = section(smoke, "run_desktop_probe() {\n", "\n}\n\nrun_probe()")

    readiness_seconds = readonly_integer(smoke, "desktop_readiness_budget_seconds")
    subprocess_seconds = readonly_integer(
        smoke, "desktop_bounded_subprocess_timeout_seconds"
    )
    subprocess_kill_grace_seconds = readonly_integer(
        smoke, "desktop_bounded_subprocess_kill_grace_seconds"
    )
    cleanup_processes = readonly_integer(smoke, "desktop_cleanup_process_count")
    cleanup_per_process_seconds = readonly_integer(
        smoke, "desktop_cleanup_per_process_budget_seconds"
    )
    margin_seconds = readonly_integer(smoke, "desktop_timeout_margin_seconds")
    outer_term_seconds = readonly_integer(smoke, "desktop_outer_term_seconds")

    assert readiness_seconds == 60
    assert subprocess_seconds == 2
    assert subprocess_kill_grace_seconds == 1
    assert cleanup_processes == 3
    assert cleanup_per_process_seconds == 7
    assert margin_seconds == 5
    assert outer_term_seconds == 90
    maximum_subprocess_overshoot = subprocess_seconds + subprocess_kill_grace_seconds
    cleanup_seconds = cleanup_processes * cleanup_per_process_seconds
    completion_seconds = (
        readiness_seconds
        + maximum_subprocess_overshoot
        + cleanup_seconds
        + margin_seconds
    )
    assert maximum_subprocess_overshoot == 3
    assert cleanup_seconds == 21
    assert completion_seconds == 89
    assert completion_seconds < outer_term_seconds
    assert (
        "readonly desktop_max_bounded_subprocess_overshoot_seconds=$((\n"
        "  desktop_bounded_subprocess_timeout_seconds\n"
        "  + desktop_bounded_subprocess_kill_grace_seconds\n"
        "))" in smoke
    )
    assert (
        "readonly desktop_cleanup_budget_seconds=$((\n"
        "  desktop_cleanup_process_count * desktop_cleanup_per_process_budget_seconds\n"
        "))" in smoke
    )
    assert (
        "readonly desktop_completion_budget_seconds=$((\n"
        "  desktop_readiness_budget_seconds\n"
        "  + desktop_max_bounded_subprocess_overshoot_seconds\n"
        "  + desktop_cleanup_budget_seconds\n"
        "  + desktop_timeout_margin_seconds\n"
        "))" in smoke
    )
    assert "desktop_completion_budget_seconds >= desktop_outer_term_seconds" in smoke

    assert 'readiness_time_remains "$readiness_deadline"' in bounded_command
    assert (
        '--kill-after="${desktop_bounded_subprocess_kill_grace_seconds}s"'
        in bounded_command
    )
    assert '"${desktop_bounded_subprocess_timeout_seconds}s"' in bounded_command
    deadline = "readiness_deadline=$((SECONDS + desktop_readiness_budget_seconds))"
    assert desktop_probe.count(deadline) == 1
    assert desktop_probe.index(deadline) < desktop_probe.index("dbus-daemon \\\n")
    assert desktop_probe.count("for ((attempt = 0; attempt < 100;") == 4
    assert desktop_probe.count("sleep 0.1") == 5
    assert desktop_probe.count('readiness_time_remains "$readiness_deadline"') == 5
    assert desktop_probe.count("run_bounded_readiness_command") == 7
    assert "timeout --signal=TERM --kill-after=1s 2s" not in desktop_probe

    dbus_ready = section(desktop_probe, "  dbus-daemon \\\n", "\n\n  Xvfb \\\n")
    xvfb_ready = section(
        desktop_probe,
        "  Xvfb \\\n",
        '\n\n  assert_application_sha256 "$expected_sha"',
    )
    app_ready = section(
        desktop_probe,
        "  application_pid=$!\n",
        "\n  window=\n",
    )
    window_ready = section(
        desktop_probe,
        "  window=\n",
        "\n  if [[ ${#windows[@]} -ne 1",
    )
    assert 'readiness_time_remains "$readiness_deadline"' in dbus_ready
    assert xvfb_ready.count('readiness_time_remains "$readiness_deadline"') == 2
    assert xvfb_ready.count("run_bounded_readiness_command") == 2
    assert 'readiness_time_remains "$readiness_deadline"' in app_ready
    assert 'while readiness_time_remains "$readiness_deadline"' in window_ready
    assert window_ready.count("run_bounded_readiness_command") == 5

    assert direct_cleanup.count("attempt < 50") == 1
    assert direct_cleanup.count("attempt < 20") == 1
    assert direct_cleanup.count("sleep 0.1") == 2
    assert group_cleanup.count('"$expected_process_group" "$expected_session" 50') == 1
    assert group_cleanup.count('"$expected_process_group" "$expected_session" 20') == 1
    assert group_wait.count("sleep 0.1") == 1
    assert "--kill-after=30s" in outer_probe
    assert (
        '"${desktop_outer_term_seconds}s" \\\n'
        '    "$script_path" __desktop_session_probe' in smoke
    )
