from __future__ import annotations

import json
import sys
from typing import TYPE_CHECKING

import pytest

from scripts.quality.source_guard import (
    RETIRED_NATIVE_BINDING_EXIT_CODE,
    Guard,
    evaluate_guard,
    find_retired_native_binding_violations,
    main,
)

if TYPE_CHECKING:
    from pathlib import Path

RETIRED_NATIVE_BUILD_BACKEND = "matu" + chr(114) + "in"


def _touch(root: Path, relative_path: str) -> None:
    path = root / relative_path
    path.parent.mkdir(parents=True, exist_ok=True)
    path.touch()


def _write(root: Path, relative_path: str, content: str) -> None:
    path = root / relative_path
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


@pytest.mark.parametrize("guard", list(Guard))
def test_source_guard_reports_absent_fixtures(guard: Guard, tmp_path: Path) -> None:
    result = evaluate_guard(tmp_path, guard)
    assert not result.applicable
    assert "absent" in result.reason


@pytest.mark.parametrize(
    ("guard", "files"),
    [
        (Guard.RUST, ["rust/example/src/lib.rs"]),
        (Guard.PYTHON, ["python/pokecon/__init__.py"]),
        (Guard.PYTHON_LINT, ["scripts/quality/source_guard.py"]),
        (Guard.PYTEST, ["tests/component/test_runtime.py"]),
        (
            Guard.WEB,
            [
                "web/package.json",
                "web/bun.lock",
                "web/src/routes/+page.svelte",
            ],
        ),
        (Guard.APP, ["rust/pokecon/src/main.rs"]),
        (
            Guard.SPA,
            [
                "rust/pokecon/src/main.rs",
                "web/package.json",
                "web/bun.lock",
                "web/src/routes/+page.svelte",
            ],
        ),
    ],
)
def test_source_guard_reports_present_fixtures(
    guard: Guard,
    files: list[str],
    tmp_path: Path,
) -> None:
    for relative_path in files:
        _touch(tmp_path, relative_path)
    result = evaluate_guard(tmp_path, guard)
    assert result.applicable
    assert "absent" not in result.reason


@pytest.mark.parametrize("guard", [Guard.APP, Guard.SPA])
def test_legacy_application_entry_point_does_not_activate_app_or_spa(
    guard: Guard,
    tmp_path: Path,
) -> None:
    for relative_path in (
        "rust/pokecon-app/src/main.rs",
        "web/package.json",
        "web/bun.lock",
        "web/src/routes/+page.svelte",
    ):
        _touch(tmp_path, relative_path)

    result = evaluate_guard(tmp_path, guard)
    assert not result.applicable
    assert "absent" in result.reason


@pytest.mark.parametrize(
    ("relative_path", "content"),
    [
        ("Cargo.toml", 'members = ["rust/pokecon-' + 'pybindings"]\n'),
        (
            "python/pokecon/__init__.py",
            "from pokecon." + "_native import runtime_version\n",
        ),
        (
            "pyproject.toml",
            f'build-backend = "{RETIRED_NATIVE_BUILD_BACKEND}"\n',
        ),
        ("uv.lock", f'name = "{RETIRED_NATIVE_BUILD_BACKEND}"\n'),
        ("requirements.txt", f"{RETIRED_NATIVE_BUILD_BACKEND}==1.14.0\n"),
        ("flake.nix", f"nativeBuilder = pkgs.{RETIRED_NATIVE_BUILD_BACKEND};\n"),
        (
            ".github/workflows/package.yml",
            f"run: {RETIRED_NATIVE_BUILD_BACKEND} build --release\n",
        ),
        (
            "scripts/release/windows.ps1",
            f"{RETIRED_NATIVE_BUILD_BACKEND} build --release\n",
        ),
        (
            ".github/workflows/release.yml",
            "run: Copy-Item poke_controller_modified_extension-0.1.0-py3-none-any.whl dist/\n",
        ),
    ],
)
def test_retired_native_binding_markers_fail_closed(
    relative_path: str,
    content: str,
    tmp_path: Path,
) -> None:
    _write(tmp_path, relative_path, content)

    violations = find_retired_native_binding_violations(tmp_path)

    assert len(violations) == 1
    assert relative_path in violations[0]


def test_retired_native_binding_path_fails_closed(tmp_path: Path) -> None:
    relative_path = "rust/pokecon-" + "pybindings/Cargo.toml"
    _touch(tmp_path, relative_path)

    violations = find_retired_native_binding_violations(tmp_path)

    assert violations == ("rust/pokecon-pybindings: retired path exists",)


@pytest.mark.parametrize("suffix", [".py", ".so", ".pyd", ".dylib", ".dll"])
def test_retired_native_module_path_fails_closed(suffix: str, tmp_path: Path) -> None:
    relative_path = f"python/pokecon/_native{suffix}"
    _touch(tmp_path, relative_path)

    violations = find_retired_native_binding_violations(tmp_path)

    assert violations == (f"{relative_path}: retired native module path exists",)


def test_historical_native_binding_references_are_explicitly_exempt(
    tmp_path: Path,
) -> None:
    historical_reference = "\n".join(
        (
            "pokecon-" + "pybindings",
            "pokecon." + "_native",
            RETIRED_NATIVE_BUILD_BACKEND,
        )
    )
    for relative_path in ("PLAN.md", "ARCHITECTURE_REVIEW.md"):
        _write(tmp_path, relative_path, historical_reference)

    assert find_retired_native_binding_violations(tmp_path) == ()


def test_resolved_native_binding_registry_audit_remains_allowed(tmp_path: Path) -> None:
    _write(
        tmp_path,
        "rust/pokecon/registry/foundation.json",
        '{"path_audit":[{"id":"legacy_python_native_extension",'
        '"paths":["rust/pokecon-pybindings","pokecon._native","maturin"],'
        '"status":"resolved","phase":13}]}\n',
    )

    assert find_retired_native_binding_violations(tmp_path) == ()


@pytest.mark.parametrize("field", ["id", "paths", "status", "phase"])
def test_native_binding_registry_audit_must_remain_canonical(
    field: str, tmp_path: Path
) -> None:
    audit = {
        "id": "legacy_python_native_extension",
        "paths": ["rust/pokecon-pybindings", "pokecon._native", "maturin"],
        "status": "resolved",
        "phase": 13,
    }
    audit[field] = {
        "id": "legacy_native_extension",
        "paths": ["rust/pokecon-pybindings"],
        "status": "implemented",
        "phase": 2,
    }[field]
    _write(
        tmp_path,
        "rust/pokecon/registry/foundation.json",
        json.dumps({"path_audit": [audit]}),
    )

    violations = find_retired_native_binding_violations(tmp_path)

    assert violations
    assert all("foundation.json" in violation for violation in violations)


def test_native_binding_markers_in_active_registry_entries_fail_closed(
    tmp_path: Path,
) -> None:
    _write(
        tmp_path,
        "rust/pokecon/registry/foundation.json",
        json.dumps(
            {
                "path_audit": [
                    {
                        "id": "legacy_python_native_extension",
                        "paths": [
                            "rust/pokecon-pybindings",
                            "pokecon._native",
                            "maturin",
                        ],
                        "status": "resolved",
                        "phase": 13,
                    },
                    {
                        "id": "python_package",
                        "resolution": "build through maturin",
                    },
                ]
            }
        ),
    )

    violations = find_retired_native_binding_violations(tmp_path)

    assert len(violations) == 1
    assert "foundation.json" in violations[0]
    assert "maturin" in violations[0]


def test_generic_worker_native_and_dependency_wheel_sources_remain_allowed(
    tmp_path: Path,
) -> None:
    _write(
        tmp_path,
        "rust/pokecon/Cargo.toml",
        '[dependencies]\npyo3 = { version = "0.28", features = ["auto-initialize"] }\n',
    )
    _write(
        tmp_path,
        "scripts/release/build_runtime.py",
        'dependency_wheel = "pyaudio-0.2.14-cp314-cp314-linux_x86_64.whl"\n',
    )
    _write(
        tmp_path,
        ".github/workflows/package.yml",
        "run: audit-worker-native-wheelhouse --require-pyaudio\n",
    )

    assert find_retired_native_binding_violations(tmp_path) == ()


def test_source_guard_cli_rejects_retired_native_binding_marker(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    _write(
        tmp_path,
        "pyproject.toml",
        f'build-backend = "{RETIRED_NATIVE_BUILD_BACKEND}"\n',
    )
    monkeypatch.setattr(
        sys,
        "argv",
        ["source_guard", "rust", "--root", str(tmp_path)],
    )

    assert main() == RETIRED_NATIVE_BINDING_EXIT_CODE
    assert (
        "retired-native-binding: violation: pyproject.toml" in capsys.readouterr().err
    )
