#!/usr/bin/env python3
"""Central source-existence guards shared by Nix apps and GitHub Actions."""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass
from enum import StrEnum
from pathlib import Path
from typing import Final, cast


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
ACTIVE_TEXT_SUFFIXES: Final = frozenset(
    {
        ".cjs",
        ".css",
        ".html",
        ".js",
        ".json",
        ".json5",
        ".jsonl",
        ".jsx",
        ".lock",
        ".lua",
        ".md",
        ".mjs",
        ".nix",
        ".ps1",
        ".py",
        ".pyi",
        ".rs",
        ".rules",
        ".sh",
        ".svelte",
        ".svg",
        ".toml",
        ".ts",
        ".tsx",
        ".txt",
        ".yaml",
        ".yml",
    }
)
IGNORED_SOURCE_DIRECTORIES: Final = frozenset(
    {
        ".direnv",
        ".git",
        ".pytest_cache",
        ".ruff_cache",
        ".venv",
        "dist",
        "node_modules",
        "result",
        "target",
    }
)
HISTORICAL_NATIVE_BINDING_REFERENCES: Final = frozenset(
    {Path("ARCHITECTURE_REVIEW.md"), Path("PLAN.md")}
)
NATIVE_BINDING_AUDIT_DECLARATION: Final = Path("rust/pokecon/registry/foundation.json")
RETIRED_NATIVE_BINDING_PATH: Final = Path("rust") / ("pokecon-" + "pybindings")
RETIRED_NATIVE_MODULE_DIRECTORY: Final = Path("python/pokecon")
RETIRED_NATIVE_MODULE_PREFIX: Final = "_native"
RETIRED_NATIVE_BINDING_MARKERS: Final = (
    "pokecon-" + "pybindings",
    "pokecon." + "_native",
    "matu" + chr(114) + "in",
)
NATIVE_BINDING_AUDIT_PATHS: Final = (
    RETIRED_NATIVE_BINDING_PATH.as_posix(),
    RETIRED_NATIVE_BINDING_MARKERS[1],
    RETIRED_NATIVE_BINDING_MARKERS[2],
)
FIRST_PARTY_RELEASE_WHEEL_MARKERS: Final = ("poke_controller_modified_extension-",)
RETIRED_NATIVE_BINDING_EXIT_CODE: Final = 4


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


def _is_active_text_source(root: Path, candidate: Path) -> bool:
    if not candidate.is_file() or candidate.is_symlink():
        return False
    relative_path = candidate.relative_to(root)
    if relative_path in HISTORICAL_NATIVE_BINDING_REFERENCES:
        return False
    if relative_path == NATIVE_BINDING_AUDIT_DECLARATION:
        return False
    if "tests" in relative_path.parts:
        return False
    if any(part in IGNORED_SOURCE_DIRECTORIES for part in relative_path.parts):
        return False
    return candidate.suffix in ACTIVE_TEXT_SUFFIXES


def _active_text_sources(root: Path) -> tuple[Path, ...]:
    sources: list[Path] = []
    for directory, directory_names, file_names in root.walk(top_down=True):
        directory_names[:] = sorted(
            name
            for name in directory_names
            if name != "tests" and name not in IGNORED_SOURCE_DIRECTORIES
        )
        sources.extend(
            candidate
            for name in sorted(file_names)
            if _is_active_text_source(root, candidate := directory / name)
        )
    return tuple(sources)


def _foundation_source_without_resolved_native_audit(path: Path) -> str:
    source = path.read_text(encoding="utf-8", errors="replace")
    if not any(
        marker.casefold() in source.casefold()
        for marker in RETIRED_NATIVE_BINDING_MARKERS
    ):
        return source
    try:
        raw_document: object = json.loads(source)
    except json.JSONDecodeError:
        return source
    if not isinstance(raw_document, dict):
        return source
    document = cast("dict[str, object]", raw_document)
    raw_audits = document.get("path_audit")
    if not isinstance(raw_audits, list):
        return source
    audits = cast("list[object]", raw_audits)
    matching_audits: list[dict[str, object]] = []
    for raw_audit in audits:
        if isinstance(raw_audit, dict):
            audit = cast("dict[str, object]", raw_audit)
            if audit.get("id") == "legacy_python_native_extension":
                matching_audits.append(audit)
    if len(matching_audits) != 1:
        return source
    resolved_audit = matching_audits[0]
    raw_paths = resolved_audit.get("paths")
    if not isinstance(raw_paths, list):
        return source
    paths = cast("list[object]", raw_paths)
    expected_paths = [*NATIVE_BINDING_AUDIT_PATHS]
    if (
        resolved_audit.get("status") != "resolved"
        or paths != expected_paths
        or resolved_audit.get("phase") != 13
    ):
        return source
    active_document = dict(document)
    active_document["path_audit"] = [
        audit for audit in audits if audit is not resolved_audit
    ]
    return json.dumps(active_document, ensure_ascii=False, sort_keys=True)


def find_retired_native_binding_violations(root: Path) -> tuple[str, ...]:
    """Return active first-party references to the retired native binding."""
    resolved_root = root.resolve()
    violations: list[str] = []
    retired_path = resolved_root / RETIRED_NATIVE_BINDING_PATH
    if retired_path.exists() or retired_path.is_symlink():
        violations.append(
            f"{RETIRED_NATIVE_BINDING_PATH.as_posix()}: retired path exists"
        )
    native_module_directory = resolved_root / RETIRED_NATIVE_MODULE_DIRECTORY
    for native_module in sorted(
        native_module_directory.glob(f"{RETIRED_NATIVE_MODULE_PREFIX}*")
    ):
        if native_module.exists() or native_module.is_symlink():
            relative_module = native_module.relative_to(resolved_root).as_posix()
            violations.append(f"{relative_module}: retired native module path exists")

    for candidate in _active_text_sources(resolved_root):
        relative_path = candidate.relative_to(resolved_root)
        if RETIRED_NATIVE_BINDING_PATH in relative_path.parents:
            continue
        source = candidate.read_text(encoding="utf-8", errors="replace").casefold()
        markers: list[str] = list(RETIRED_NATIVE_BINDING_MARKERS)
        if relative_path == Path(".github/workflows/release.yml"):
            markers.extend(FIRST_PARTY_RELEASE_WHEEL_MARKERS)
        violations.extend(
            f"{relative_path.as_posix()}: contains retired first-party "
            f"native binding marker {marker!r}"
            for marker in markers
            if marker.casefold() in source
        )
    audit_declaration = resolved_root / NATIVE_BINDING_AUDIT_DECLARATION
    if audit_declaration.is_file() and not audit_declaration.is_symlink():
        active_registry_source = _foundation_source_without_resolved_native_audit(
            audit_declaration
        ).casefold()
        violations.extend(
            f"{NATIVE_BINDING_AUDIT_DECLARATION.as_posix()}: contains retired "
            f"first-party native binding marker {marker!r} outside the canonical "
            "resolved audit"
            for marker in RETIRED_NATIVE_BINDING_MARKERS
            if marker.casefold() in active_registry_source
        )
    return tuple(violations)


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
            (root / "rust/pokecon/src/main.rs").is_file(),
            "Rust application entry point exists",
            "Rust application entry point (rust/pokecon/src/main.rs) is absent",
        ),
        Guard.SPA: (
            (root / "rust/pokecon/src/main.rs").is_file() and _web_exists(root),
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
    root = args.root.resolve()
    violations = find_retired_native_binding_violations(root)
    if violations:
        for violation in violations:
            print(f"retired-native-binding: violation: {violation}", file=sys.stderr)
        return RETIRED_NATIVE_BINDING_EXIT_CODE

    result = evaluate_guard(root, args.guard)
    status = "applicable" if result.applicable else "not applicable"
    print(f"{args.guard.value}: {status}: {result.reason}")
    if args.github_output is not None:
        _write_github_output(args.github_output, result)
    if args.require_applicable and not result.applicable:
        return 3
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
