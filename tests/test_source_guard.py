from __future__ import annotations

from typing import TYPE_CHECKING

import pytest

from scripts.source_guard import Guard, evaluate_guard

if TYPE_CHECKING:
    from pathlib import Path


def _touch(root: Path, relative_path: str) -> None:
    path = root / relative_path
    path.parent.mkdir(parents=True, exist_ok=True)
    path.touch()


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
        (Guard.PYTHON_LINT, ["scripts/source_guard.py"]),
        (Guard.PYTEST, ["tests/component/test_runtime.py"]),
        (
            Guard.WEB,
            [
                "web/package.json",
                "web/package-lock.json",
                "web/src/routes/+page.svelte",
            ],
        ),
        (Guard.APP, ["rust/pokecon-app/src/main.rs"]),
        (
            Guard.SPA,
            [
                "rust/pokecon-app/src/main.rs",
                "web/package.json",
                "web/package-lock.json",
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
