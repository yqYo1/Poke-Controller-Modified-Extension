#!/usr/bin/env python3
"""Verify that the Nix source filter retains every build-source extension."""

from __future__ import annotations

import argparse
import re
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from collections.abc import Iterable

FILTER_EXTENSION: Final = re.compile(r'hasSuffix "\.([A-Za-z0-9]+)"')
DEFAULT_SOURCE_ROOTS: Final = (
    "rust",
    "python",
    "scripts",
    "tests",
    "web/src",
    "web/static",
)
IGNORED_DIRECTORIES: Final = frozenset(
    {"target", "node_modules", "dist", ".svelte-kit", "__pycache__"}
)


@dataclass(frozen=True, slots=True)
class SourceFilterReport:
    """Compared source and filter extension sets."""

    filter_extensions: frozenset[str]
    source_extensions: frozenset[str]
    missing_extensions: frozenset[str]
    unused_extensions: frozenset[str]


def _source_extensions(root: Path, source_roots: Iterable[str]) -> frozenset[str]:
    extensions: set[str] = set()
    for relative_root in source_roots:
        directory = root / relative_root
        if not directory.is_dir():
            continue
        for path in directory.rglob("*"):
            if not path.is_file() or any(
                component in IGNORED_DIRECTORIES for component in path.parts
            ):
                continue
            suffix = path.suffix.removeprefix(".")
            if suffix:
                extensions.add(suffix)
    return frozenset(extensions)


def check_source_filter(
    root: Path,
    source_roots: Iterable[str] = DEFAULT_SOURCE_ROOTS,
) -> SourceFilterReport:
    """Compare extensions retained by ``flake.nix`` with build-source files."""
    flake_text = (root / "flake.nix").read_text(encoding="utf-8")
    filter_extensions = frozenset(FILTER_EXTENSION.findall(flake_text))
    source_extensions = _source_extensions(root, source_roots)
    return SourceFilterReport(
        filter_extensions=filter_extensions,
        source_extensions=source_extensions,
        missing_extensions=source_extensions - filter_extensions,
        unused_extensions=filter_extensions - source_extensions,
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
    if report.missing_extensions:
        print(
            "missing filter extensions:", ", ".join(sorted(report.missing_extensions))
        )
        return 1
    print("all build-source extensions are retained by the Nix source filter")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
