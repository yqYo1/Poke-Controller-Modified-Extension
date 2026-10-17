"""Normalize Linux release executables for a non-Nix Debian installation."""

from __future__ import annotations

import argparse
import hashlib
import re
import stat
import subprocess
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Sequence


SYSTEM_INTERPRETER = "/lib64/ld-linux-x86-64.so.2"
MAXIMUM_GLIBC = (2, 39)
GLIBC_PATTERN = re.compile(r"GLIBC_(\d+)\.(\d+)")
EPHEMERAL_BUILD_ROOT_PREFIX = Path("/") / "tmp" / "pokecon-rust-gate-home."
EPHEMERAL_BUILD_ROOT_PATTERN = re.compile(
    rf"{re.escape(str(EPHEMERAL_BUILD_ROOT_PREFIX))}[A-Za-z0-9]{{8}}"
)
CANONICAL_BUILD_ROOT = "/build/pokecon-release-root.00000000"


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while block := source.read(64 * 1024):
            digest.update(block)
    return digest.hexdigest()


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


def _is_real_regular_file(path: Path) -> bool:
    try:
        return (
            not path.is_symlink()
            and not path.is_junction()
            and stat.S_ISREG(path.stat(follow_symlinks=False).st_mode)
        )
    except OSError:
        return False


def normalize_ephemeral_build_root(binary: Path, build_root: Path) -> int:
    source = str(build_root)
    if EPHEMERAL_BUILD_ROOT_PATTERN.fullmatch(source) is None:
        message = f"unexpected isolated release build root: {source}"
        raise ValueError(message)
    source_bytes = source.encode("ascii")
    replacement_bytes = CANONICAL_BUILD_ROOT.encode("ascii")
    if len(source_bytes) != len(replacement_bytes):
        message = (
            "canonical release build root must preserve the ELF byte layout: "
            f"{source!r} -> {CANONICAL_BUILD_ROOT!r}"
        )
        raise ValueError(message)
    content = binary.read_bytes()
    occurrences = content.count(source_bytes)
    if occurrences:
        binary.write_bytes(content.replace(source_bytes, replacement_bytes))
    return occurrences


def normalize_binary(
    binary: Path,
    patchelf: Path,
    strip: Path,
    objdump: Path,
    build_root: Path,
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
    normalized_build_root_occurrences = normalize_ephemeral_build_root(
        binary, build_root
    )

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
        "sha256": sha256_file(binary),
        "size": binary.stat().st_size,
        "interpreter": interpreter,
        "rpath": actual_rpath,
        "needed": needed,
        "maximum_glibc": f"{maximum[0]}.{maximum[1]}",
        "normalized_build_root_occurrences": normalized_build_root_occurrences,
    }


def normalize_release(
    application: Path | None,
    worker: Path | None,
    python_root: Path | None,
    patchelf: Path,
    strip: Path,
    objdump: Path,
    build_root: Path,
) -> list[dict[str, object]]:
    if application is None and worker is None:
        message = "release normalization requires an application or worker"
        raise ValueError(message)
    if worker is not None:
        if python_root is None:
            message = "portable Python root is required for worker normalization"
            raise ValueError(message)
        python_library = python_root / "lib/libpython3.14.so.1.0"
        if not _is_real_regular_file(python_library):
            message = f"portable Python shared library is missing: {python_library}"
            raise ValueError(message)

    reports: list[dict[str, object]] = []
    if application is not None:
        reports.append(
            normalize_binary(
                application,
                patchelf,
                strip,
                objdump,
                build_root,
                rpath=None,
            )
        )
    if worker is not None:
        worker_report = normalize_binary(
            worker,
            patchelf,
            strip,
            objdump,
            build_root,
            rpath="$ORIGIN/python/lib",
        )
        worker_needed = worker_report["needed"]
        if (
            not isinstance(worker_needed, list)
            or "libpython3.14.so.1.0" not in worker_needed
        ):
            message = "managed worker is not linked to the pinned CPython 3.14 runtime"
            raise ValueError(message)
        reports.append(worker_report)
    return reports


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--application", type=Path)
    parser.add_argument("--worker", type=Path)
    parser.add_argument("--python-root", type=Path)
    parser.add_argument("--patchelf", type=Path, required=True)
    parser.add_argument("--strip", type=Path, required=True)
    parser.add_argument("--objdump", type=Path, required=True)
    parser.add_argument("--ephemeral-build-root", type=Path, required=True)
    arguments = parser.parse_args()
    reports = normalize_release(
        arguments.application,
        arguments.worker,
        arguments.python_root,
        arguments.patchelf,
        arguments.strip,
        arguments.objdump,
        arguments.ephemeral_build_root,
    )
    for report in reports:
        print(report)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
