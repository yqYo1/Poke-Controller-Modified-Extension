#!/usr/bin/env python3
"""Verify that the Nix source filter retains every build-source extension."""

from __future__ import annotations

import argparse
import os
import re
import subprocess
from dataclasses import dataclass
from pathlib import Path
from shutil import which
from typing import TYPE_CHECKING, Final, cast

if TYPE_CHECKING:
    from collections.abc import Iterable

FILTER_EXTENSION: Final = re.compile(r'hasSuffix "\.([A-Za-z0-9]+)"')
DEFAULT_SOURCE_ROOTS: Final = (
    "api",
    "rust",
    "python",
    "scripts",
    "tests",
    "web",
)
IGNORED_DIRECTORIES: Final = frozenset(
    {"target", "node_modules", "dist", ".svelte-kit", "__pycache__"}
)
JAVASCRIPT_SUFFIXES: Final = frozenset({".js", ".jsx", ".mjs", ".cjs"})
NIX_EXTENSIONLESS_FILES: Final = frozenset({".gitignore", "LICENSE"})


@dataclass(frozen=True, slots=True)
class SourceFilterReport:
    """Compared source and filter extension sets."""

    filter_extensions: frozenset[str]
    source_extensions: frozenset[str]
    missing_extensions: frozenset[str]
    unused_extensions: frozenset[str]
    javascript_sources: tuple[str, ...]
    extensionless_sources: tuple[str, ...]


def _git_source_files(
    root: Path,
    source_roots: tuple[str, ...],
) -> tuple[Path, ...] | None:
    """List repository sources while respecting Git ignore rules when available."""
    if not (root / ".git").exists():
        return None
    git = which("git")
    if git is None:
        msg = "git is required to inspect sources in a worktree"
        raise RuntimeError(msg)

    completed = subprocess.run(  # noqa: S603 - arguments are passed without a shell
        [
            git,
            "-C",
            str(root),
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            *source_roots,
        ],
        check=True,
        stdout=subprocess.PIPE,
        text=False,
    )
    raw_output = cast("bytes | bytearray | memoryview | str", completed.stdout)
    if isinstance(raw_output, str):
        relative_paths = [item for item in raw_output.split("\0") if item]
    else:
        relative_paths = [
            os.fsdecode(item) for item in bytes(raw_output).split(b"\0") if item
        ]
    return tuple(
        sorted(
            path
            for relative_path in relative_paths
            for path in (root / relative_path,)
            if path.is_file()
            and not any(
                component in IGNORED_DIRECTORIES
                for component in Path(relative_path).parts
            )
        )
    )


def _source_files(root: Path, source_roots: Iterable[str]) -> tuple[Path, ...]:
    source_roots = tuple(source_roots)
    git_source_files = _git_source_files(root, source_roots)
    if git_source_files is not None:
        return git_source_files

    if (root / ".gitignore").is_file():
        msg = "a non-worktree source root with .gitignore requires a Git checkout"
        raise RuntimeError(msg)

    files: set[Path] = set()
    for relative_root in source_roots:
        directory = root / relative_root
        if not directory.is_dir():
            continue
        for path in directory.rglob("*"):
            relative_path = path.relative_to(root)
            if not path.is_file() or any(
                component in IGNORED_DIRECTORIES for component in relative_path.parts
            ):
                continue
            files.add(path)
    return tuple(sorted(files))


def check_source_filter(
    root: Path,
    source_roots: Iterable[str] = DEFAULT_SOURCE_ROOTS,
) -> SourceFilterReport:
    """Compare extensions retained by ``flake.nix`` with build-source files."""
    flake_text = (root / "flake.nix").read_text(encoding="utf-8")
    filter_extensions = frozenset(FILTER_EXTENSION.findall(flake_text))
    source_files = _source_files(root, source_roots)
    source_extensions = frozenset(
        path.suffix.removeprefix(".") for path in source_files if path.suffix
    )
    extensionless_sources = tuple(
        path.relative_to(root).as_posix()
        for path in source_files
        if not path.suffix and path.name not in NIX_EXTENSIONLESS_FILES
    )
    return SourceFilterReport(
        filter_extensions=filter_extensions,
        source_extensions=source_extensions,
        missing_extensions=source_extensions - filter_extensions,
        unused_extensions=filter_extensions - source_extensions,
        javascript_sources=tuple(
            path.relative_to(root).as_posix()
            for path in source_files
            if path.suffix in JAVASCRIPT_SUFFIXES
        ),
        extensionless_sources=extensionless_sources,
    )


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    return parser.parse_args()


def main() -> int:
    """Run the source-filter comparison and return a CI-friendly status."""
    args = _parse_args()
    report = check_source_filter(args.root.resolve())
    print("source extensions:", ", ".join(sorted(report.source_extensions)))
    print("filter extensions:", ", ".join(sorted(report.filter_extensions)))
    if report.unused_extensions:
        print("unused filter extensions:", ", ".join(sorted(report.unused_extensions)))
    failed = False
    if report.javascript_sources:
        print(
            "JavaScript build sources must be migrated to TypeScript:",
            ", ".join(report.javascript_sources),
        )
        failed = True
    if report.missing_extensions:
        print(
            "missing filter extensions:", ", ".join(sorted(report.missing_extensions))
        )
        failed = True
    if report.extensionless_sources:
        print(
            "extensionless build sources are not retained by the Nix filter:",
            ", ".join(report.extensionless_sources),
        )
        failed = True
    if failed:
        return 1
    print("all build-source extensions are retained and no JavaScript sources remain")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
