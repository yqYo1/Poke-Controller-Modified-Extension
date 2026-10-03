from __future__ import annotations

import os
import shutil
import signal
import stat
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

import pytest

from scripts.integration.pidfd_signal import (
    EXIT_ERROR,
    EXIT_IDENTITY_NOT_LIVE,
    EXIT_SENT,
    MAXIMUM_LINUX_PID_T,
    MAXIMUM_START_TICKS,
    ExpectedIdentity,
    IdentityNotLive,
    KernelHooks,
    ProcessSnapshot,
    SignalCommand,
    parse_process_stat,
    read_process_snapshot,
    signal_exact_process,
)

REPOSITORY = Path(__file__).resolve().parents[2]
HELPER = REPOSITORY / "scripts/integration/pidfd_signal.py"


def _python_executable() -> Path:
    return Path(sys.executable).resolve(strict=True)


def _sleep_command() -> Path:
    executable = shutil.which("sleep")
    assert executable is not None
    return Path(executable).absolute()


def _sleep_executable() -> Path:
    return _sleep_command().resolve(strict=True)


def _launch_child(executable: Path | None = None) -> subprocess.Popen[bytes]:
    command = executable if executable is not None else _sleep_command()
    return subprocess.Popen(  # noqa: S603 - exact test fixture executable
        [str(command), "60"],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )


def _cleanup_child(child: subprocess.Popen[bytes]) -> None:
    if child.poll() is not None:
        return
    child.terminate()
    try:
        child.wait(timeout=5)
    except subprocess.TimeoutExpired:
        child.kill()
        child.wait(timeout=5)


def _run_helper(
    signal_name: str,
    pid: int,
    pgid: int,
    start_ticks: int,
    executable: Path,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(  # noqa: S603 - exact Python and helper paths
        [
            sys.executable,
            "-I",
            "-S",
            str(HELPER),
            signal_name,
            str(pid),
            str(pgid),
            str(start_ticks),
            str(executable),
        ],
        check=False,
        capture_output=True,
        text=True,
        timeout=10,
    )


def _live_child_identity(child: subprocess.Popen[bytes]) -> ProcessSnapshot:
    snapshot = read_process_snapshot(child.pid)
    assert snapshot.pid == child.pid
    assert snapshot.pgid == child.pid
    return snapshot


def test_parser_is_exact_and_helper_contains_no_numeric_signal_fallback() -> None:
    executable = _python_executable()
    helper_source = HELPER.read_text(encoding="utf-8")
    assert "os.kill" not in helper_source
    assert "subprocess" not in helper_source
    assert "signal.pidfd_send_signal" in helper_source
    assert helper_source.index(
        "pidfd = _open_pidfd(expected, hooks)"
    ) < helper_source.index("_validate_expected_path(expected.executable)")

    invalid_commands = (
        ("INT", "2", "2", "1", str(executable)),
        ("X" * 5000, "2", "2", "1", str(executable)),
        ("TERM", "1", "2", "1", str(executable)),
        ("TERM", "2", "1", "1", str(executable)),
        ("KILL", "2", "2", "0", str(executable)),
        ("TERM", "02", "2", "1", str(executable)),
        ("TERM", "2", "2", "1", "relative/executable"),
    )
    for command in invalid_commands:
        completed = subprocess.run(  # noqa: S603 - exact Python and helper paths
            [sys.executable, "-I", "-S", str(HELPER), *command],
            check=False,
            capture_output=True,
            text=True,
            timeout=10,
        )
        assert completed.returncode == EXIT_ERROR
        assert len(completed.stderr) <= 1024


def test_process_stat_parser_handles_closing_parentheses_in_comm() -> None:
    pid = 321
    fields = ["S", "10", "321", *("0" for _index in range(16)), "98765"]
    contents = f"{pid} (fixture ) name) {' '.join(fields)}\n"

    assert parse_process_stat(pid, contents) == ("S", 321, 98765)


@pytest.mark.parametrize(
    ("pid", "pgid", "start_ticks"),
    [
        (999999999999999999999999999999999999999999999999, 2, 1),
        (2, MAXIMUM_LINUX_PID_T + 1, 1),
        (2, 2, MAXIMUM_START_TICKS + 1),
    ],
)
def test_cli_rejects_oversized_integer_fields_without_a_traceback(
    pid: int,
    pgid: int,
    start_ticks: int,
) -> None:
    completed = _run_helper(
        "TERM",
        pid,
        pgid,
        start_ticks,
        _python_executable(),
    )

    assert completed.returncode == EXIT_ERROR
    assert completed.stdout == ""
    assert "Traceback" not in completed.stderr
    assert len(completed.stderr) <= 1024


@pytest.mark.skipif(sys.platform != "linux", reason="pidfds require Linux")
def test_cli_sends_term_to_a_matching_real_child() -> None:
    child = _launch_child()
    try:
        identity = _live_child_identity(child)
        completed = _run_helper(
            "TERM",
            identity.pid,
            identity.pgid,
            identity.start_ticks,
            identity.executable,
        )

        assert completed.returncode == EXIT_SENT, completed.stderr
        assert child.wait(timeout=5) == -signal.SIGTERM
    finally:
        _cleanup_child(child)


@pytest.mark.skipif(sys.platform != "linux", reason="pidfds require Linux")
@pytest.mark.parametrize("mismatch", ["start_ticks", "pgid", "executable"])
def test_identity_mismatch_leaves_the_real_child_alive(
    tmp_path: Path,
    mismatch: str,
) -> None:
    child = _launch_child()
    try:
        identity = _live_child_identity(child)
        pgid = identity.pgid
        start_ticks = identity.start_ticks
        executable = identity.executable
        if mismatch == "start_ticks":
            start_ticks += 1
        elif mismatch == "pgid":
            pgid += 1
        else:
            executable = tmp_path / "different-python"
            shutil.copy2(_python_executable(), executable)

        completed = _run_helper(
            "TERM",
            identity.pid,
            pgid,
            start_ticks,
            executable,
        )

        assert completed.returncode == EXIT_IDENTITY_NOT_LIVE
        assert "identity-not-live" in completed.stderr
        assert child.poll() is None
    finally:
        _cleanup_child(child)


@pytest.mark.skipif(sys.platform != "linux", reason="pidfds require Linux")
@pytest.mark.parametrize("invalid_kind", ["symlink", "directory", "nonexecutable"])
def test_invalid_expected_executable_is_a_stable_error_and_leaves_child_alive(
    tmp_path: Path,
    invalid_kind: str,
) -> None:
    child = _launch_child()
    try:
        identity = _live_child_identity(child)
        invalid_executable = tmp_path / invalid_kind
        if invalid_kind == "symlink":
            invalid_executable.symlink_to(identity.executable)
        elif invalid_kind == "directory":
            invalid_executable.mkdir()
        else:
            shutil.copy2(_sleep_executable(), invalid_executable)
            invalid_executable.chmod(0o600)

        completed = _run_helper(
            "TERM",
            identity.pid,
            identity.pgid,
            identity.start_ticks,
            invalid_executable,
        )

        assert completed.returncode == EXIT_ERROR
        assert "stable-error" in completed.stderr
        assert child.poll() is None
    finally:
        _cleanup_child(child)


@pytest.mark.skipif(sys.platform != "linux", reason="pidfds require Linux")
def test_replacing_the_running_executable_path_is_refused(tmp_path: Path) -> None:
    copied_executable = tmp_path / "sleep"
    replacement = tmp_path / "replacement-sleep"
    shutil.copy2(_sleep_executable(), copied_executable)
    shutil.copy2(_sleep_executable(), replacement)
    child = _launch_child(copied_executable)
    try:
        identity = _live_child_identity(child)
        assert identity.executable == copied_executable
        os.replace(replacement, copied_executable)

        completed = _run_helper(
            "TERM",
            identity.pid,
            identity.pgid,
            identity.start_ticks,
            copied_executable,
        )

        assert completed.returncode == EXIT_IDENTITY_NOT_LIVE
        assert child.poll() is None
    finally:
        _cleanup_child(child)


def _regular_status(device: int = 7, inode: int = 11) -> os.stat_result:
    return os.stat_result((stat.S_IFREG | 0o755, inode, device, 1, 0, 0, 1, 0, 0, 0))


@dataclass(slots=True)
class FakeKernel:
    snapshots: list[ProcessSnapshot]
    pidfd_states: list[bool]
    closed: list[int] = field(default_factory=list)
    sent: list[tuple[int, int]] = field(default_factory=list)
    file_statuses: dict[int, os.stat_result] = field(default_factory=dict)
    next_expected_descriptor: int = 101
    next_live_descriptor: int = 201
    pidfd: int = 91

    def open_pidfd(self, _pid: int, _flags: int) -> int:
        return self.pidfd

    def pidfd_exited(self, _pidfd: int) -> bool:
        return self.pidfd_states.pop(0) if self.pidfd_states else False

    def read_snapshot(self, _pid: int) -> ProcessSnapshot:
        return self.snapshots.pop(0)

    def open_expected(self, _path: Path) -> int:
        descriptor = self.next_expected_descriptor
        self.next_expected_descriptor += 1
        return descriptor

    def open_live(self, _pid: int) -> int:
        descriptor = self.next_live_descriptor
        self.next_live_descriptor += 1
        return descriptor

    def fstat(self, descriptor: int) -> os.stat_result:
        return self.file_statuses.get(descriptor, _regular_status())

    def close(self, descriptor: int) -> None:
        self.closed.append(descriptor)

    def send(self, pidfd: int, signal_number: int) -> None:
        self.sent.append((pidfd, signal_number))

    def hooks(self) -> KernelHooks:
        return KernelHooks(
            pidfd_open=self.open_pidfd,
            pidfd_exited=self.pidfd_exited,
            read_snapshot=self.read_snapshot,
            open_expected_executable=self.open_expected,
            open_live_executable=self.open_live,
            fstat=self.fstat,
            close=self.close,
            pidfd_send_signal=self.send,
        )


def _fake_command(executable: Path) -> SignalCommand:
    return SignalCommand(
        signal_name="TERM",
        identity=ExpectedIdentity(
            pid=400,
            pgid=400,
            start_ticks=8000,
            executable=executable,
        ),
    )


def _fake_snapshot(
    executable: Path,
    *,
    pid: int = 400,
    start_ticks: int = 8000,
) -> ProcessSnapshot:
    return ProcessSnapshot(
        pid=pid,
        state="S",
        pgid=400,
        start_ticks=start_ticks,
        executable=executable,
    )


def test_injected_snapshot_pid_mismatch_never_sends_and_closes_open_fds(
    tmp_path: Path,
) -> None:
    executable = tmp_path / "fixture-executable"
    shutil.copy2(_python_executable(), executable)
    fake = FakeKernel(
        snapshots=[_fake_snapshot(executable, pid=401)],
        pidfd_states=[False],
    )

    with pytest.raises(IdentityNotLive):
        signal_exact_process(_fake_command(executable), hooks=fake.hooks())

    assert fake.sent == []
    assert set(fake.closed) == {fake.pidfd, 101}


def test_injected_identity_race_never_reaches_pidfd_send_signal(tmp_path: Path) -> None:
    executable = tmp_path / "fixture-executable"
    shutil.copy2(_python_executable(), executable)
    fake = FakeKernel(
        snapshots=[
            _fake_snapshot(executable),
            _fake_snapshot(executable, start_ticks=8001),
        ],
        pidfd_states=[False, False],
    )

    with pytest.raises(IdentityNotLive):
        signal_exact_process(_fake_command(executable), hooks=fake.hooks())

    assert fake.sent == []
    assert set(fake.closed) == {fake.pidfd, 101, 201}


def test_successful_injected_transaction_sends_only_through_pidfd_and_closes_fds(
    tmp_path: Path,
) -> None:
    executable = tmp_path / "fixture-executable"
    shutil.copy2(_python_executable(), executable)
    fake = FakeKernel(
        snapshots=[_fake_snapshot(executable), _fake_snapshot(executable)],
        pidfd_states=[False, False, False],
    )

    signal_exact_process(_fake_command(executable), hooks=fake.hooks())

    assert fake.sent == [(fake.pidfd, signal.SIGTERM)]
    assert set(fake.closed) == {fake.pidfd, 101, 102, 201, 202}


def test_final_pidfd_liveness_fence_refuses_exit_and_closes_all_fds(
    tmp_path: Path,
) -> None:
    executable = tmp_path / "fixture-executable"
    shutil.copy2(_python_executable(), executable)
    fake = FakeKernel(
        snapshots=[_fake_snapshot(executable), _fake_snapshot(executable)],
        pidfd_states=[False, False, True],
    )

    with pytest.raises(IdentityNotLive):
        signal_exact_process(_fake_command(executable), hooks=fake.hooks())

    assert fake.sent == []
    assert set(fake.closed) == {fake.pidfd, 101, 102, 201, 202}


def test_reopened_expected_executable_mismatch_refuses_send_and_closes_fds(
    tmp_path: Path,
) -> None:
    executable = tmp_path / "fixture-executable"
    shutil.copy2(_python_executable(), executable)
    fake = FakeKernel(
        snapshots=[_fake_snapshot(executable), _fake_snapshot(executable)],
        pidfd_states=[False, False],
        file_statuses={102: _regular_status(inode=12)},
    )

    with pytest.raises(IdentityNotLive):
        signal_exact_process(_fake_command(executable), hooks=fake.hooks())

    assert fake.sent == []
    assert set(fake.closed) == {fake.pidfd, 101, 102, 201}


def test_exited_pidfd_is_refused_and_closed_before_identity_reads(
    tmp_path: Path,
) -> None:
    executable = tmp_path / "fixture-executable"
    shutil.copy2(_python_executable(), executable)
    fake = FakeKernel(snapshots=[], pidfd_states=[True])

    with pytest.raises(IdentityNotLive):
        signal_exact_process(_fake_command(executable), hooks=fake.hooks())

    assert fake.sent == []
    assert fake.snapshots == []
    assert fake.closed == [fake.pidfd]
