"""Signal one exact Linux process identity through a pinned pidfd."""

from __future__ import annotations

import argparse
import errno
import os
import select
import signal
import stat
import sys
from contextlib import ExitStack, suppress
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Final, Literal, Never, cast

if TYPE_CHECKING:
    from collections.abc import Callable, Sequence

EXIT_SENT: Final = 0
EXIT_ERROR: Final = 2
EXIT_IDENTITY_NOT_LIVE: Final = 75
MAXIMUM_PROC_STAT_BYTES: Final = 4096
MAXIMUM_DIAGNOSTIC_CHARACTERS: Final = 512
MAXIMUM_LINUX_PID_T: Final = (1 << 31) - 1
MAXIMUM_START_TICKS: Final = (1 << 64) - 1
DEAD_PROCESS_STATES: Final = frozenset({"X", "Z", "x"})

type SignalName = Literal["TERM", "KILL"]
type PidfdOpen = Callable[[int, int], int]
type PidfdExited = Callable[[int], bool]
type SnapshotReader = Callable[[int], ProcessSnapshot]
type ExpectedExecutableOpener = Callable[[Path], int]
type LiveExecutableOpener = Callable[[int], int]
type FileStatusReader = Callable[[int], os.stat_result]
type FileDescriptorCloser = Callable[[int], None]
type PidfdSignalSender = Callable[[int, int], None]


@dataclass(frozen=True, slots=True)
class ExpectedIdentity:
    """Caller-provided process identity that must match exactly."""

    pid: int
    pgid: int
    start_ticks: int
    executable: Path


@dataclass(frozen=True, slots=True)
class SignalCommand:
    """Validated command-line request."""

    signal_name: SignalName
    identity: ExpectedIdentity


@dataclass(frozen=True, slots=True)
class ProcessSnapshot:
    """One paired ``stat`` and ``exe`` observation from procfs."""

    pid: int
    state: str
    pgid: int
    start_ticks: int
    executable: Path


@dataclass(frozen=True, slots=True)
class ExecutableIdentity:
    """Filesystem identity pinned by an open executable descriptor."""

    device: int
    inode: int


@dataclass(frozen=True, slots=True)
class KernelHooks:
    """Injectable Linux operations used by the identity transaction."""

    pidfd_open: PidfdOpen
    pidfd_exited: PidfdExited
    read_snapshot: SnapshotReader
    open_expected_executable: ExpectedExecutableOpener
    open_live_executable: LiveExecutableOpener
    fstat: FileStatusReader
    close: FileDescriptorCloser
    pidfd_send_signal: PidfdSignalSender


class IdentityNotLive(RuntimeError):
    """The pidfd or one of the expected identity fields no longer matches."""


class StableHelperError(RuntimeError):
    """The helper cannot safely complete its bounded signaling transaction."""


class BoundedArgumentParser(argparse.ArgumentParser):
    """Argument parser whose diagnostics cannot echo unbounded input."""

    def error(self, message: str) -> Never:
        self.print_usage(sys.stderr)
        self.exit(
            EXIT_ERROR,
            f"{self.prog}: error: {_bounded(message)}\n",
        )


def _bounded(message: str) -> str:
    normalized = " ".join(message.splitlines())
    if len(normalized) <= MAXIMUM_DIAGNOSTIC_CHARACTERS:
        return normalized
    return f"{normalized[: MAXIMUM_DIAGNOSTIC_CHARACTERS - 3]}..."


def _positive_integer(
    value: str,
    *,
    minimum: int,
    maximum: int,
    label: str,
) -> int:
    try:
        parsed = int(value, 10)
    except ValueError as error:
        message = f"{label} must be a base-10 integer"
        raise argparse.ArgumentTypeError(message) from error
    if parsed < minimum or parsed > maximum or str(parsed) != value:
        message = f"{label} must be a canonical integer between {minimum} and {maximum}"
        raise argparse.ArgumentTypeError(message)
    return parsed


def _process_id(value: str) -> int:
    return _positive_integer(
        value,
        minimum=2,
        maximum=MAXIMUM_LINUX_PID_T,
        label="PID",
    )


def _process_group_id(value: str) -> int:
    return _positive_integer(
        value,
        minimum=2,
        maximum=MAXIMUM_LINUX_PID_T,
        label="process-group ID",
    )


def _start_ticks(value: str) -> int:
    return _positive_integer(
        value,
        minimum=1,
        maximum=MAXIMUM_START_TICKS,
        label="start ticks",
    )


def _absolute_path(value: str) -> Path:
    path = Path(value)
    if not path.is_absolute():
        message = "expected executable must be an absolute path"
        raise argparse.ArgumentTypeError(message)
    return path


def parse_command(arguments: Sequence[str] | None = None) -> SignalCommand:
    """Parse the exact, intentionally small pidfd signaling CLI."""
    parser = BoundedArgumentParser(description=__doc__)
    parser.add_argument("signal", choices=("TERM", "KILL"))
    parser.add_argument("pid", type=_process_id)
    parser.add_argument("pgid", type=_process_group_id)
    parser.add_argument("start_ticks", type=_start_ticks)
    parser.add_argument("executable", type=_absolute_path)
    namespace = parser.parse_args(arguments)
    return SignalCommand(
        signal_name=cast("SignalName", namespace.signal),
        identity=ExpectedIdentity(
            pid=cast("int", namespace.pid),
            pgid=cast("int", namespace.pgid),
            start_ticks=cast("int", namespace.start_ticks),
            executable=cast("Path", namespace.executable),
        ),
    )


def _pidfd_open(pid: int, flags: int) -> int:
    opener = cast("PidfdOpen | None", getattr(os, "pidfd_open", None))
    if sys.platform != "linux" or opener is None:
        message = "Linux os.pidfd_open is unavailable"
        raise StableHelperError(message)
    return opener(pid, flags)


def _pidfd_exited(pidfd: int) -> bool:
    poller = select.poll()
    poller.register(pidfd, select.POLLIN | select.POLLERR | select.POLLHUP)
    return bool(poller.poll(0))


def _read_bounded_proc_stat(pid: int) -> str:
    path = f"/proc/{pid}/stat"
    flags = os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW
    descriptor = os.open(path, flags)
    try:
        contents = os.read(descriptor, MAXIMUM_PROC_STAT_BYTES + 1)
    finally:
        os.close(descriptor)
    if len(contents) > MAXIMUM_PROC_STAT_BYTES:
        message = "proc stat record exceeds the safety bound"
        raise IdentityNotLive(message)
    try:
        return contents.decode("utf-8")
    except UnicodeDecodeError as error:
        message = "proc stat record is not valid UTF-8"
        raise IdentityNotLive(message) from error


def parse_process_stat(pid: int, contents: str) -> tuple[str, int, int]:
    """Parse state, process group, and start time around a parenthesized comm."""
    prefix = f"{pid} ("
    closing = contents.rfind(") ")
    if not contents.startswith(prefix) or closing < len(prefix):
        message = "proc stat record does not identify the expected PID"
        raise IdentityNotLive(message)
    fields = contents[closing + 2 :].split()
    if len(fields) < 20:
        message = "proc stat record is incomplete"
        raise IdentityNotLive(message)
    state = fields[0]
    try:
        pgid = int(fields[2], 10)
        start_ticks = int(fields[19], 10)
    except ValueError as error:
        message = "proc stat identity fields are malformed"
        raise IdentityNotLive(message) from error
    if len(state) != 1 or not state.isalpha() or state in DEAD_PROCESS_STATES:
        message = "process state is dead or malformed"
        raise IdentityNotLive(message)
    if (
        pgid <= 1
        or pgid > MAXIMUM_LINUX_PID_T
        or start_ticks <= 0
        or start_ticks > MAXIMUM_START_TICKS
    ):
        message = "proc stat identity fields are outside their safe ranges"
        raise IdentityNotLive(message)
    return state, pgid, start_ticks


def read_process_snapshot(pid: int) -> ProcessSnapshot:
    """Read one paired process-stat and executable-link snapshot."""
    try:
        contents = _read_bounded_proc_stat(pid)
        executable = Path(os.readlink(f"/proc/{pid}/exe"))
    except OSError as error:
        if error.errno in {errno.ENOENT, errno.ESRCH}:
            message = "process disappeared while its proc identity was read"
            raise IdentityNotLive(message) from error
        message = f"cannot read process identity: {error.strerror or error.__class__.__name__}"
        raise StableHelperError(message) from error
    state, pgid, start_ticks = parse_process_stat(pid, contents)
    return ProcessSnapshot(
        pid=pid,
        state=state,
        pgid=pgid,
        start_ticks=start_ticks,
        executable=executable,
    )


def _open_expected_executable(path: Path) -> int:
    return os.open(path, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW)


def _open_live_executable(pid: int) -> int:
    return os.open(f"/proc/{pid}/exe", os.O_RDONLY | os.O_CLOEXEC)


def _close_file_descriptor(descriptor: int) -> None:
    with suppress(OSError):
        os.close(descriptor)


def _pidfd_send_signal(pidfd: int, signal_number: int) -> None:
    sender = cast(
        "Callable[[int, int, object | None, int], None] | None",
        getattr(signal, "pidfd_send_signal", None),
    )
    if sys.platform != "linux" or sender is None:
        message = "Linux signal.pidfd_send_signal is unavailable"
        raise StableHelperError(message)
    sender(pidfd, signal_number, None, 0)


SYSTEM_HOOKS: Final = KernelHooks(
    pidfd_open=_pidfd_open,
    pidfd_exited=_pidfd_exited,
    read_snapshot=read_process_snapshot,
    open_expected_executable=_open_expected_executable,
    open_live_executable=_open_live_executable,
    fstat=os.fstat,
    close=_close_file_descriptor,
    pidfd_send_signal=_pidfd_send_signal,
)


def _validate_expected_path(path: Path) -> None:
    try:
        canonical = Path(os.path.realpath(path, strict=True))
    except OSError as error:
        message = f"expected executable cannot be resolved: {error.strerror or error.__class__.__name__}"
        raise StableHelperError(message) from error
    if canonical != path:
        message = "expected executable is not an exact canonical path"
        raise StableHelperError(message)


def _executable_identity(
    status: os.stat_result,
    *,
    require_execute_permission: bool,
) -> ExecutableIdentity:
    if not stat.S_ISREG(status.st_mode):
        message = "executable descriptor does not refer to a regular file"
        if require_execute_permission:
            raise StableHelperError(message)
        raise IdentityNotLive(message)
    if require_execute_permission and status.st_mode & 0o111 == 0:
        message = "expected executable has no execute permission"
        raise StableHelperError(message)
    return ExecutableIdentity(device=status.st_dev, inode=status.st_ino)


def _validate_snapshot(
    snapshot: ProcessSnapshot,
    expected: ExpectedIdentity,
) -> None:
    if (
        len(snapshot.state) != 1
        or not snapshot.state.isalpha()
        or snapshot.state in DEAD_PROCESS_STATES
    ):
        message = "live process state is dead or malformed"
        raise IdentityNotLive(message)
    if snapshot.pid != expected.pid:
        message = "live PID does not match the expected PID"
        raise IdentityNotLive(message)
    if snapshot.pgid != expected.pgid:
        message = "live process group does not match the expected process group"
        raise IdentityNotLive(message)
    if snapshot.start_ticks != expected.start_ticks:
        message = "live start ticks do not match the expected start ticks"
        raise IdentityNotLive(message)
    if snapshot.executable != expected.executable:
        message = "live executable path does not match the expected executable"
        raise IdentityNotLive(message)


def _ensure_pidfd_live(pidfd: int, hooks: KernelHooks) -> None:
    try:
        exited = hooks.pidfd_exited(pidfd)
    except OSError as error:
        message = (
            f"cannot inspect pidfd state: {error.strerror or error.__class__.__name__}"
        )
        raise StableHelperError(message) from error
    if exited:
        message = "pidfd reports that the expected process has exited"
        raise IdentityNotLive(message)


def _open_pidfd(expected: ExpectedIdentity, hooks: KernelHooks) -> int:
    try:
        return hooks.pidfd_open(expected.pid, 0)
    except OverflowError as error:
        message = "expected PID is outside the Linux pid_t range"
        raise StableHelperError(message) from error
    except OSError as error:
        if error.errno == errno.ESRCH:
            message = "expected process is no longer live"
            raise IdentityNotLive(message) from error
        message = f"cannot open pidfd: {error.strerror or error.__class__.__name__}"
        raise StableHelperError(message) from error


def _open_expected(
    path: Path,
    stack: ExitStack,
    hooks: KernelHooks,
) -> tuple[int, ExecutableIdentity]:
    try:
        descriptor = hooks.open_expected_executable(path)
    except OSError as error:
        message = f"cannot open expected executable safely: {error.strerror or error.__class__.__name__}"
        raise StableHelperError(message) from error
    stack.callback(hooks.close, descriptor)
    try:
        identity = _executable_identity(
            hooks.fstat(descriptor),
            require_execute_permission=True,
        )
    except OSError as error:
        message = f"cannot inspect expected executable: {error.strerror or error.__class__.__name__}"
        raise StableHelperError(message) from error
    return descriptor, identity


def _open_live(
    pid: int,
    stack: ExitStack,
    hooks: KernelHooks,
) -> tuple[int, ExecutableIdentity]:
    try:
        descriptor = hooks.open_live_executable(pid)
    except OSError as error:
        if error.errno in {errno.ENOENT, errno.ESRCH}:
            message = "process executable disappeared while it was pinned"
            raise IdentityNotLive(message) from error
        message = f"cannot open process executable: {error.strerror or error.__class__.__name__}"
        raise StableHelperError(message) from error
    stack.callback(hooks.close, descriptor)
    try:
        identity = _executable_identity(
            hooks.fstat(descriptor),
            require_execute_permission=False,
        )
    except OSError as error:
        message = f"cannot inspect process executable: {error.strerror or error.__class__.__name__}"
        raise StableHelperError(message) from error
    return descriptor, identity


def _require_same_executable(
    observed: ExecutableIdentity,
    expected: ExecutableIdentity,
) -> None:
    if observed != expected:
        message = (
            "open executable identity does not match the expected device and inode"
        )
        raise IdentityNotLive(message)


def signal_exact_process(
    command: SignalCommand,
    *,
    hooks: KernelHooks = SYSTEM_HOOKS,
) -> None:
    """Validate twice and signal only the process pinned by a pidfd."""
    expected = command.identity
    pidfd = _open_pidfd(expected, hooks)
    with ExitStack() as stack:
        stack.callback(hooks.close, pidfd)
        _ensure_pidfd_live(pidfd, hooks)
        _validate_expected_path(expected.executable)

        _expected_fd, expected_executable = _open_expected(
            expected.executable,
            stack,
            hooks,
        )
        first_snapshot = hooks.read_snapshot(expected.pid)
        _validate_snapshot(first_snapshot, expected)
        _first_live_fd, first_live_executable = _open_live(
            expected.pid,
            stack,
            hooks,
        )
        _require_same_executable(first_live_executable, expected_executable)
        _ensure_pidfd_live(pidfd, hooks)

        second_snapshot = hooks.read_snapshot(expected.pid)
        _validate_snapshot(second_snapshot, expected)
        if (
            second_snapshot.pid,
            second_snapshot.pgid,
            second_snapshot.start_ticks,
            second_snapshot.executable,
        ) != (
            first_snapshot.pid,
            first_snapshot.pgid,
            first_snapshot.start_ticks,
            first_snapshot.executable,
        ):
            message = "process identity changed between proc snapshots"
            raise IdentityNotLive(message)

        _reopened_expected_fd, reopened_expected = _open_expected(
            expected.executable,
            stack,
            hooks,
        )
        _require_same_executable(reopened_expected, expected_executable)
        _second_live_fd, second_live_executable = _open_live(
            expected.pid,
            stack,
            hooks,
        )
        _require_same_executable(second_live_executable, expected_executable)
        _require_same_executable(second_live_executable, first_live_executable)
        _ensure_pidfd_live(pidfd, hooks)

        signal_number = {
            "TERM": signal.SIGTERM,
            "KILL": signal.SIGKILL,
        }[command.signal_name]
        try:
            hooks.pidfd_send_signal(pidfd, signal_number)
        except OSError as error:
            if error.errno == errno.ESRCH:
                message = "expected process exited before pidfd signaling"
                raise IdentityNotLive(message) from error
            message = (
                f"pidfd signaling failed: {error.strerror or error.__class__.__name__}"
            )
            raise StableHelperError(message) from error


def main(arguments: Sequence[str] | None = None) -> int:
    """Run the pidfd-only command and return a deterministic status."""
    command = parse_command(arguments)
    try:
        signal_exact_process(command)
    except IdentityNotLive as error:
        print(
            f"pidfd-signal: identity-not-live: {_bounded(str(error))}",
            file=sys.stderr,
        )
        return EXIT_IDENTITY_NOT_LIVE
    except StableHelperError as error:
        print(f"pidfd-signal: stable-error: {_bounded(str(error))}", file=sys.stderr)
        return EXIT_ERROR
    return EXIT_SENT


if __name__ == "__main__":
    raise SystemExit(main())
