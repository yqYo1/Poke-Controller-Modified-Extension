#!/usr/bin/env python3
"""Central source-existence guards shared by Nix apps and GitHub Actions."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from enum import StrEnum
from pathlib import Path
from typing import Final


class Guard(StrEnum):
    """Source groups that activate conditional verification jobs."""

    RUST = "rust"
    PYTHON = "python"
    PYTHON_LINT = "python-lint"
    PYTEST = "pytest"
    WEB = "web"
    APP = "app"
    SPA = "spa"


@dataclass(frozen=True, slots=True)
class GuardResult:
    """Applicability result and its user-facing evidence."""

    applicable: bool
    reason: str


PYTHON_SUFFIXES: Final = frozenset({".py", ".pyi"})
WEB_SUFFIXES: Final = frozenset({".svelte", ".ts", ".tsx"})


def _has_file(root: Path, directory: str, suffixes: frozenset[str]) -> bool:
    path = root / directory
    return path.is_dir() and any(
        candidate.is_file() and candidate.suffix in suffixes
        for candidate in path.rglob("*")
    )


def _python_tests_exist(root: Path) -> bool:
    tests = root / "tests"
    return tests.is_dir() and any(
        candidate.is_file()
        and candidate.suffix == ".py"
        and (candidate.name.startswith("test_") or candidate.name.endswith("_test.py"))
        for candidate in tests.rglob("*.py")
    )


def _web_exists(root: Path) -> bool:
    return (
        (root / "web/package.json").is_file()
        and (root / "web/bun.lock").is_file()
        and _has_file(root, "web/src", WEB_SUFFIXES)
    )


def evaluate_guard(root: Path, guard: Guard) -> GuardResult:
    """Evaluate one source guard against a repository or fixture root."""
    checks: dict[Guard, tuple[bool, str, str]] = {
        Guard.RUST: (
            _has_file(root, "rust", frozenset({".rs"})),
            "Rust crate source exists",
            "Rust crate source (rust/**/src/**/*.rs) is absent",
        ),
        Guard.PYTHON: (
            _has_file(root, "python", PYTHON_SUFFIXES),
            "Python package source or typing source exists",
            "Python package source (python/**/*.{py,pyi}) is absent",
        ),
        Guard.PYTHON_LINT: (
            any(
                _has_file(root, directory, PYTHON_SUFFIXES)
                for directory in ("python", "scripts", "tests")
            ),
            "Python lint source exists",
            "Python lint source (python, scripts, or tests) is absent",
        ),
        Guard.PYTEST: (
            _python_tests_exist(root),
            "collectable pytest source exists",
            "pytest source (tests/**/test_*.py or tests/**/*_test.py) is absent",
        ),
        Guard.WEB: (
            _web_exists(root),
            "Web package, lockfile, and application source exist",
            "Web prerequisites (package.json, bun.lock, web/src source) are absent",
        ),
        Guard.APP: (
            (root / "rust/pokecon-app/src/main.rs").is_file(),
            "Rust application entry point exists",
            "Rust application entry point (rust/pokecon-app/src/main.rs) is absent",
        ),
        Guard.SPA: (
            (root / "rust/pokecon-app/src/main.rs").is_file() and _web_exists(root),
            "Rust application and complete Web package exist",
            "Rust application or complete Web package is absent",
        ),
    }
    applicable, present_reason, absent_reason = checks[guard]
    return GuardResult(
        applicable=applicable,
        reason=present_reason if applicable else absent_reason,
    )


def _write_github_output(path: Path, result: GuardResult) -> None:
    with path.open("a", encoding="utf-8") as output:
        output.write(f"applicable={str(result.applicable).lower()}\n")


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("guard", type=Guard, choices=list(Guard))
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--github-output", type=Path)
    parser.add_argument("--require-applicable", action="store_true")
    return parser.parse_args()


def main() -> int:
    """Run a guard and optionally expose its result to GitHub Actions."""
    args = _parse_args()
    result = evaluate_guard(args.root.resolve(), args.guard)
    status = "applicable" if result.applicable else "not applicable"
    print(f"{args.guard.value}: {status}: {result.reason}")
    if args.github_output is not None:
        _write_github_output(args.github_output, result)
    if args.require_applicable and not result.applicable:
        return 3
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
