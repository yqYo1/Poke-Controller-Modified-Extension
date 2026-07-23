"""Normalize Linux release executables for a non-Nix Debian installation."""

from __future__ import annotations

import argparse
import re
import subprocess
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Sequence


SYSTEM_INTERPRETER = "/lib64/ld-linux-x86-64.so.2"
MAXIMUM_GLIBC = (2, 39)
GLIBC_PATTERN = re.compile(r"GLIBC_(\d+)\.(\d+)")


def run(arguments: Sequence[str | Path], *, capture: bool = False) -> str:
    completed = subprocess.run(
        [str(argument) for argument in arguments],
        check=True,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None,
        text=True,
    )
    return completed.stdout.strip() if capture else ""


def normalize_binary(
    binary: Path,
    patchelf: Path,
    strip: Path,
    objdump: Path,
    *,
    rpath: str | None,
) -> dict[str, object]:
    if not binary.is_file():
        message = f"release executable is missing: {binary}"
        raise ValueError(message)
    run([patchelf, "--set-interpreter", SYSTEM_INTERPRETER, binary])
    if rpath is None:
        run([patchelf, "--remove-rpath", binary])
    else:
        run([patchelf, "--set-rpath", rpath, binary])
    run([strip, "--strip-unneeded", binary])

    interpreter = run([patchelf, "--print-interpreter", binary], capture=True)
    actual_rpath = run([patchelf, "--print-rpath", binary], capture=True)
    needed = sorted(
        line
        for line in run([patchelf, "--print-needed", binary], capture=True).splitlines()
        if line
    )
    symbols = run([objdump, "-T", binary], capture=True)
    versions = sorted(
        {
            (int(match.group(1)), int(match.group(2)))
            for match in GLIBC_PATTERN.finditer(symbols)
        }
    )
    maximum = versions[-1] if versions else (0, 0)
    if interpreter != SYSTEM_INTERPRETER:
        message = f"unexpected ELF interpreter for {binary}: {interpreter}"
        raise ValueError(message)
    expected_rpath = "" if rpath is None else rpath
    if actual_rpath != expected_rpath:
        message = (
            f"release executable has RPATH {actual_rpath!r}; "
            f"expected {expected_rpath!r}: {binary}"
        )
        raise ValueError(message)
    if any("/" in library or library.startswith(".") for library in needed):
        message = f"release executable has a path-qualified dependency: {binary}"
        raise ValueError(message)
    if maximum[0] > MAXIMUM_GLIBC[0] or (
        maximum[0] == MAXIMUM_GLIBC[0] and maximum[1] > MAXIMUM_GLIBC[1]
    ):
        message = (
            f"{binary} requires GLIBC {maximum[0]}.{maximum[1]}, newer than "
            f"{MAXIMUM_GLIBC[0]}.{MAXIMUM_GLIBC[1]}"
        )
        raise ValueError(message)
    return {
        "path": str(binary),
        "interpreter": interpreter,
        "rpath": actual_rpath,
        "needed": needed,
        "maximum_glibc": f"{maximum[0]}.{maximum[1]}",
    }


def normalize_release(
    application: Path,
    worker: Path,
    python_root: Path,
    patchelf: Path,
    strip: Path,
    objdump: Path,
) -> list[dict[str, object]]:
    python_library = python_root / "lib/libpython3.14.so.1.0"
    if not python_library.is_file():
        message = f"portable Python shared library is missing: {python_library}"
        raise ValueError(message)
    reports = [
        normalize_binary(application, patchelf, strip, objdump, rpath=None),
        normalize_binary(
            worker,
            patchelf,
            strip,
            objdump,
            rpath="$ORIGIN/python/lib",
        ),
    ]
    worker_needed = reports[1]["needed"]
    if (
        not isinstance(worker_needed, list)
        or "libpython3.14.so.1.0" not in worker_needed
    ):
        message = "managed worker is not linked to the pinned CPython 3.14 runtime"
        raise ValueError(message)
    return reports


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--application", type=Path, required=True)
    parser.add_argument("--worker", type=Path, required=True)
    parser.add_argument("--python-root", type=Path, required=True)
    parser.add_argument("--patchelf", type=Path, required=True)
    parser.add_argument("--strip", type=Path, required=True)
    parser.add_argument("--objdump", type=Path, required=True)
    arguments = parser.parse_args()
    reports = normalize_release(
        arguments.application,
        arguments.worker,
        arguments.python_root,
        arguments.patchelf,
        arguments.strip,
        arguments.objdump,
    )
    for report in reports:
        print(report)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
