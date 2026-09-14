from __future__ import annotations

from pathlib import Path

import pytest

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
DETERMINISTIC_MSVS_RUSTFLAGS = (
    'rustflags: "-C debuginfo=0 -C strip=debuginfo '
    '-C link-arg=/Brepro -C link-arg=/DEBUG:NONE"'
)


@pytest.mark.parametrize(
    ("workflow", "expected_count"),
    (
        (".github/workflows/package.yml", 2),
        (".github/workflows/release.yml", 1),
    ),
)
def test_windows_workflow_uses_deterministic_msvc_flags(
    workflow: str, expected_count: int
) -> None:
    workflow_text = (REPOSITORY_ROOT / workflow).read_text(encoding="utf-8")
    assert workflow_text.count(DETERMINISTIC_MSVS_RUSTFLAGS) == expected_count
