from __future__ import annotations

import os
import signal
import subprocess
import sys
import time
from contextlib import suppress
from pathlib import Path

import pytest

REPOSITORY = Path(__file__).resolve().parents[2]
RUNNER = REPOSITORY / "scripts/quality/run_parallel_checks.py"


def test_parallel_check_cleanup_preserves_process_identity_and_is_bounded() -> None:
    source = RUNNER.read_text(encoding="utf-8")

    assert "pid = os.posix_spawnp(" in source
    assert "setpgroup=0" in source
    assert "setsigdef=HANDLED_SIGNALS" in source
    assert "setsigmask=()" in source
    assert "preexec_fn" not in source
    assert "signal.pthread_sigmask(signal.SIG_BLOCK, HANDLED_SIGNALS)" in source
    assert "signal.pthread_sigmask(signal.SIG_SETMASK, previous_mask)" in source
    assert "os.WEXITED | os.WNOHANG | os.WNOWAIT" in source
    assert "os.killpg(check.pid, sent_signal)" in source
    assert "signal_process_group(check, signal.SIGTERM)" in source
    assert "signal_process_group(check, signal.SIGCONT)" in source
    assert "signal_process_group(check, signal.SIGKILL)" in source
    assert source.index("signal_process_group(check, signal.SIGKILL)") < source.index(
        "waited_pid, wait_status = os.waitpid(check.pid, 0)"
    )
    termination = source[
        source.index("def terminate_checks(") : source.index("def normalized_status(")
    ]
    assert "reap_deadline = time.monotonic() + POST_KILL_REAP_SECONDS" in termination


def fake_command(label: str, status: int) -> list[str]:
    source = (
        "import sys, time; "
        "label, status = sys.argv[1], int(sys.argv[2]); "
        "print(f'{label}-output', flush=True); "
        "time.sleep(0.05); "
        "raise SystemExit(status)"
    )
    return [sys.executable, "-c", source, label, str(status)]


@pytest.mark.parametrize(
    ("first_status", "second_status", "expected_status"),
    [(0, 0, 0), (7, 0, 7), (0, 9, 9), (7, 9, 7)],
)
def test_parallel_checks_replay_both_logs_and_propagate_status(
    tmp_path: Path,
    first_status: int,
    second_status: int,
    expected_status: int,
) -> None:
    temporary_root = tmp_path / "tmp"
    temporary_root.mkdir()
    environment = os.environ.copy()
    environment["TMPDIR"] = str(temporary_root)

    completed = subprocess.run(  # noqa: S603 - fixed test runner and fake commands
        [
            sys.executable,
            "-I",
            str(RUNNER),
            "first",
            *fake_command("first", first_status),
            "--next",
            "second",
            *fake_command("second", second_status),
        ],
        check=False,
        capture_output=True,
        env=environment,
        text=True,
        timeout=5,
    )

    assert completed.returncode == expected_status
    assert completed.stdout == (
        "\n=== aggregate first ===\n"
        "first-output\n"
        "\n=== aggregate second ===\n"
        "second-output\n"
    )
    assert completed.stderr == "".join(
        f"aggregate {label} failed with status {status}\n"
        for label, status in (("first", first_status), ("second", second_status))
        if status != 0
    )
    assert list(temporary_root.iterdir()) == []


@pytest.mark.parametrize(
    ("arguments", "message"),
    [
        ([], "usage:"),
        (["first", "true", "second", "true", "extra"], "usage:"),
        (["first", "--next", "second", "true", "extra"], "usage:"),
        (
            ["../first", "true", "--next", "second", "true"],
            "label is not a safe log name",
        ),
        (
            ["duplicate", "true", "--next", "duplicate", "true"],
            "labels must be distinct",
        ),
    ],
)
def test_parallel_checks_reject_malformed_commands(
    arguments: list[str], message: str
) -> None:
    completed = subprocess.run(  # noqa: S603 - fixed test runner
        [sys.executable, "-I", str(RUNNER), *arguments],
        check=False,
        capture_output=True,
        text=True,
        timeout=5,
    )

    assert completed.returncode == 2
    assert message in completed.stderr


def process_tree_command(
    started: Path, grandchild_terminated: Path, *, parent_exits: bool = False
) -> list[str]:
    grandchild_source = """
import signal
import sys
import time
from pathlib import Path

ready = Path(sys.argv[1])
terminated = Path(sys.argv[2])


def terminate(_signal: int, _frame: object) -> None:
    terminated.write_text("terminated", encoding="utf-8")
    raise SystemExit(0)


signal.signal(signal.SIGTERM, terminate)
ready.write_text("ready", encoding="utf-8")
while True:
    time.sleep(0.05)
"""
    source = r"""
import os
import signal
import subprocess
import sys
import time
from pathlib import Path

started = Path(sys.argv[1])
grandchild_terminated = Path(sys.argv[2])
grandchild_source = sys.argv[3]
parent_mode = sys.argv[4]
grandchild_ready = started.with_name(f"{started.name}-grandchild-ready")
parent_release = started.with_name(f"{started.name}-release")

signal.signal(signal.SIGTERM, signal.SIG_IGN)
grandchild = subprocess.Popen(
    [sys.executable, "-c", grandchild_source, str(grandchild_ready), str(grandchild_terminated)]
)
deadline = time.monotonic() + 5
while not grandchild_ready.exists():
    if grandchild.poll() is not None:
        raise RuntimeError("grandchild exited before becoming ready")
    if time.monotonic() >= deadline:
        raise RuntimeError("grandchild did not become ready")
    time.sleep(0.01)
started.write_text(f"{os.getpid()}\n{grandchild.pid}\n", encoding="utf-8")
if parent_mode == "exit":
    while not parent_release.exists():
        time.sleep(0.01)
    raise SystemExit(0)
while True:
    time.sleep(0.05)
"""
    return [
        sys.executable,
        "-c",
        source,
        str(started),
        str(grandchild_terminated),
        grandchild_source,
        "exit" if parent_exits else "wait",
    ]


def test_parallel_checks_clean_exited_and_cancelled_process_groups_with_bounds(
    tmp_path: Path,
) -> None:
    temporary_root = tmp_path / "tmp"
    temporary_root.mkdir()
    first_started = tmp_path / "first-started"
    first_grandchild_terminated = tmp_path / "first-grandchild-terminated"
    second_started = tmp_path / "second-started"
    second_grandchild_terminated = tmp_path / "second-grandchild-terminated"
    environment = os.environ.copy()
    environment["TMPDIR"] = str(temporary_root)
    process = subprocess.Popen(  # noqa: S603 - fixed test runner and fake commands
        [
            sys.executable,
            "-I",
            str(RUNNER),
            "first",
            *process_tree_command(
                first_started,
                first_grandchild_terminated,
                parent_exits=True,
            ),
            "--next",
            "second",
            *process_tree_command(second_started, second_grandchild_terminated),
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=environment,
        text=True,
    )
    try:
        deadline = time.monotonic() + 5
        while not (first_started.exists() and second_started.exists()):
            if process.poll() is not None:
                pytest.fail(
                    f"parallel runner exited before both jobs started: {process.poll()}"
                )
            if time.monotonic() >= deadline:
                pytest.fail("parallel runner did not start both jobs")
            time.sleep(0.01)

        for started in (first_started, second_started):
            parent_pid, grandchild_pid = (
                int(value) for value in started.read_text(encoding="utf-8").splitlines()
            )
            assert os.getpgid(parent_pid) == parent_pid
            assert os.getpgid(grandchild_pid) == parent_pid

        first_started.with_name(f"{first_started.name}-release").write_text(
            "release", encoding="utf-8"
        )
        exited_group_deadline = time.monotonic() + 2
        while not first_grandchild_terminated.exists():
            if process.poll() is not None:
                pytest.fail(
                    "parallel runner exited before the second check was cancelled"
                )
            if time.monotonic() >= exited_group_deadline:
                pytest.fail("descendant of the exited first check was not terminated")
            time.sleep(0.01)

        termination_started = time.monotonic()
        process.send_signal(signal.SIGTERM)
        descendant_deadline = time.monotonic() + 2
        while not second_grandchild_terminated.exists():
            if process.poll() is not None:
                early_stdout, early_stderr = process.communicate(timeout=5)
                pytest.fail(
                    "parallel runner exited before terminating descendants: "
                    f"{process.returncode}, {early_stdout!r}, {early_stderr!r}"
                )
            if time.monotonic() >= descendant_deadline:
                pytest.fail("parallel runner did not signal both descendant groups")
            time.sleep(0.01)
        process.send_signal(signal.SIGINT)
        stdout, stderr = process.communicate(timeout=5)
        termination_elapsed = time.monotonic() - termination_started
    finally:
        if process.poll() is None:
            for started in (first_started, second_started):
                if not started.exists():
                    continue
                process_group = int(started.read_text(encoding="utf-8").splitlines()[0])
                with suppress(ProcessLookupError):
                    os.killpg(process_group, signal.SIGKILL)
            process.kill()
            process.communicate(timeout=5)

    assert process.returncode == 143, (stdout, stderr)
    assert termination_elapsed < 3
    assert stdout == ""
    assert stderr == ""
    assert first_grandchild_terminated.read_text(encoding="utf-8") == "terminated"
    assert second_grandchild_terminated.read_text(encoding="utf-8") == "terminated"
    assert list(temporary_root.iterdir()) == []
