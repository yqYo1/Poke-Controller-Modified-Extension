"""Run two independent verification commands concurrently with bounded cleanup."""

from __future__ import annotations

import os
import re
import signal
import sys
import time
from contextlib import suppress
from dataclasses import dataclass
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Sequence
    from types import FrameType
    from typing import NoReturn


SAFE_LABEL = re.compile(r"[a-z0-9][a-z0-9-]*")
HANDLED_SIGNALS = frozenset((signal.SIGINT, signal.SIGTERM))
TERMINATION_GRACE_SECONDS = 1.0
POST_KILL_REAP_SECONDS = 1.0
TERMINATION_POLL_SECONDS = 0.02
EXITED_GROUP_GRACE_SECONDS = 0.05


@dataclass(frozen=True)
class CheckSpec:
    label: str
    command: tuple[str, ...]


@dataclass
class RunningCheck:
    spec: CheckSpec
    pid: int
    log_path: Path
    returncode: int | None = None


class TerminationRequested(Exception):
    """Carry the conventional shell status for an intercepted signal."""

    status: int

    def __init__(self, status: int) -> None:
        super().__init__(status)
        self.status = status


def usage() -> NoReturn:
    print(
        "usage: run_parallel_checks.py "
        "LABEL COMMAND [ARG ...] --next LABEL COMMAND [ARG ...]",
        file=sys.stderr,
    )
    raise SystemExit(2)


def parse_arguments(arguments: Sequence[str]) -> tuple[CheckSpec, CheckSpec]:
    if len(arguments) < 5:
        usage()
    try:
        delimiter = arguments.index("--next", 1)
    except ValueError:
        usage()
    if delimiter < 2 or delimiter + 2 >= len(arguments):
        usage()

    first = CheckSpec(arguments[0], tuple(arguments[1:delimiter]))
    second = CheckSpec(arguments[delimiter + 1], tuple(arguments[delimiter + 2 :]))
    for check in (first, second):
        if SAFE_LABEL.fullmatch(check.label) is None:
            print(
                f"parallel check label is not a safe log name: {check.label}",
                file=sys.stderr,
            )
            raise SystemExit(2)
    if first.label == second.label:
        print("parallel check labels must be distinct", file=sys.stderr)
        raise SystemExit(2)
    return first, second


def request_termination(signum: int, _frame: FrameType | None) -> NoReturn:
    raise TerminationRequested(128 + signum)


def ignore_termination_signals() -> None:
    for handled_signal in HANDLED_SIGNALS:
        signal.signal(handled_signal, signal.SIG_IGN)


def start_check(spec: CheckSpec, log_directory: Path) -> RunningCheck:
    log_path = log_directory / f"{spec.label}.log"
    log_descriptor = os.open(log_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    null_descriptor = os.open(os.devnull, os.O_RDONLY)
    try:
        pid = os.posix_spawnp(
            spec.command[0],
            spec.command,
            os.environ,
            file_actions=(
                (os.POSIX_SPAWN_DUP2, null_descriptor, 0),
                (os.POSIX_SPAWN_DUP2, log_descriptor, 1),
                (os.POSIX_SPAWN_DUP2, log_descriptor, 2),
                (os.POSIX_SPAWN_CLOSE, null_descriptor),
                (os.POSIX_SPAWN_CLOSE, log_descriptor),
            ),
            setpgroup=0,
            setsigdef=HANDLED_SIGNALS,
            setsigmask=(),
        )
    finally:
        os.close(null_descriptor)
        os.close(log_descriptor)
    return RunningCheck(spec=spec, pid=pid, log_path=log_path)


def process_group_exists(check: RunningCheck) -> bool:
    if check.returncode is not None:
        return False
    try:
        os.killpg(check.pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def signal_process_group(check: RunningCheck, sent_signal: signal.Signals) -> None:
    if check.returncode is not None:
        return
    with suppress(ProcessLookupError):
        os.killpg(check.pid, sent_signal)


def poll_check(check: RunningCheck) -> bool:
    if check.returncode is not None:
        return True
    previous_mask = signal.pthread_sigmask(signal.SIG_BLOCK, HANDLED_SIGNALS)
    try:
        exited = os.waitid(
            os.P_PID,
            check.pid,
            os.WEXITED | os.WNOHANG | os.WNOWAIT,
        )
        if exited is not None:
            signal_process_group(check, signal.SIGTERM)
            signal_process_group(check, signal.SIGCONT)
            time.sleep(EXITED_GROUP_GRACE_SECONDS)
            signal_process_group(check, signal.SIGKILL)
            waited_pid, wait_status = os.waitpid(check.pid, 0)
            if waited_pid != check.pid:
                message = f"waitpid returned an unrelated process: {waited_pid}"
                raise RuntimeError(message)
            check.returncode = os.waitstatus_to_exitcode(wait_status)
    finally:
        signal.pthread_sigmask(signal.SIG_SETMASK, previous_mask)
    return check.returncode is not None


def wait_for_check(check: RunningCheck) -> int:
    while not poll_check(check):
        time.sleep(TERMINATION_POLL_SECONDS)
    if check.returncode is None:
        message = f"check status was not recorded after wait: {check.spec.label}"
        raise RuntimeError(message)
    return check.returncode


def terminate_checks(checks: Sequence[RunningCheck]) -> None:
    ignore_termination_signals()
    for check in checks:
        signal_process_group(check, signal.SIGTERM)
        signal_process_group(check, signal.SIGCONT)

    grace_deadline = time.monotonic() + TERMINATION_GRACE_SECONDS
    while any(process_group_exists(check) for check in checks):
        remaining = grace_deadline - time.monotonic()
        if remaining <= 0:
            break
        time.sleep(min(TERMINATION_POLL_SECONDS, remaining))

    for check in checks:
        if process_group_exists(check):
            signal_process_group(check, signal.SIGKILL)

    reap_deadline = time.monotonic() + POST_KILL_REAP_SECONDS
    while not all(poll_check(check) for check in checks):
        remaining = reap_deadline - time.monotonic()
        if remaining <= 0:
            break
        time.sleep(min(TERMINATION_POLL_SECONDS, remaining))
    for check in checks:
        if check.returncode is None:
            print(
                f"aggregate {check.spec.label} did not exit after SIGKILL",
                file=sys.stderr,
            )


def normalized_status(returncode: int) -> int:
    return 128 - returncode if returncode < 0 else returncode


def replay_logs(checks: Sequence[RunningCheck], statuses: Sequence[int]) -> None:
    for check, status in zip(checks, statuses, strict=True):
        print(f"\n=== aggregate {check.spec.label} ===")
        sys.stdout.write(check.log_path.read_text(encoding="utf-8", errors="replace"))
        if status != 0:
            print(
                f"aggregate {check.spec.label} failed with status {status}",
                file=sys.stderr,
            )


def launch_checks(
    specs: tuple[CheckSpec, CheckSpec],
    log_directory: Path,
    running: list[RunningCheck],
) -> tuple[CheckSpec, OSError] | None:
    launch_error: tuple[CheckSpec, OSError] | None = None
    previous_mask = signal.pthread_sigmask(signal.SIG_BLOCK, HANDLED_SIGNALS)
    try:
        for spec in specs:
            try:
                running.append(start_check(spec, log_directory))
            except OSError as error:
                launch_error = spec, error
                break
    finally:
        signal.pthread_sigmask(signal.SIG_SETMASK, previous_mask)
    return launch_error


def run_checks(specs: tuple[CheckSpec, CheckSpec]) -> int:
    with TemporaryDirectory(prefix="pokecon-parallel-checks.") as temporary:
        log_directory = Path(temporary)
        running: list[RunningCheck] = []
        try:
            launch_error = launch_checks(specs, log_directory, running)
            if launch_error is not None:
                failed_spec, error = launch_error
                print(
                    f"aggregate {failed_spec.label} could not start: {error}",
                    file=sys.stderr,
                )
                terminate_checks(running)
                return 127

            statuses = [normalized_status(wait_for_check(check)) for check in running]
            ignore_termination_signals()
            replay_logs(running, statuses)
            return next((status for status in statuses if status != 0), 0)
        except TerminationRequested as termination:
            terminate_checks(running)
            return termination.status


def main(arguments: Sequence[str] | None = None) -> int:
    specs = parse_arguments(sys.argv[1:] if arguments is None else arguments)
    for handled_signal in HANDLED_SIGNALS:
        signal.signal(handled_signal, request_termination)
    return run_checks(specs)


if __name__ == "__main__":
    raise SystemExit(main())
