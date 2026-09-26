"""Schema report tests for handoff section 5.1 public surfaces.

Pins the exact set of inventory surfaces, checks that every recorded sha256
matches an independently recomputed file digest, and proves the report fails
closed on drift, missing files, and malformed rows.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path

import pytest

from scripts.acceptance import schema_report

REPOSITORY = Path(__file__).resolve().parents[2]
HANDOFF = REPOSITORY / "docs/ARCHITECTURE_HANDOFF.md"

EXPECTED_KEYS = frozenset(
    {
        "rest",
        "websocket",
        "webrtc_fallback",
        "openapi_typescript",
        "settings_dynamic_schema",
        "cli_mode",
        "worker_ipc",
    }
)

FIXTURE_HANDOFF = """\
# fixture handoff

### 5.1 fixture surfaces

| surface | current | source | boundary |
| --- | --- | --- | --- |
| Alpha | `pkg/a.rs:1-10` | `pkg/b.json` | boundary |
| Beta lane | `pkg/{c,d}.rs` | text | `pkg/e.toml:3` |

### 5.2 next
"""

FIXTURE_FILES = ("a.rs", "b.json", "c.rs", "d.rs", "e.toml")


def _write_fixture(root: Path) -> Path:
    package = root / "pkg"
    package.mkdir()
    for name in FIXTURE_FILES:
        (package / name).write_text(f"content of {name}\n", encoding="utf-8")
    handoff = root / "HANDOFF.md"
    handoff.write_text(FIXTURE_HANDOFF, encoding="utf-8")
    return handoff


def test_schema_report_lists_every_section_5_1_surface() -> None:
    entries = schema_report.parse_section(HANDOFF.read_text(encoding="utf-8"))
    assert set(entries) == set(EXPECTED_KEYS)


def test_schema_report_records_sha256_per_surface() -> None:
    report = schema_report.build_report(REPOSITORY, HANDOFF)
    assert report.get("schema") == "schema-report/1"
    assert report.get("surface_count") == len(EXPECTED_KEYS)
    surfaces = schema_report.mapping(report.get("surfaces"), "schema report surfaces")
    assert set(surfaces) == set(EXPECTED_KEYS)
    for key in EXPECTED_KEYS:
        entry = schema_report.mapping(surfaces[key], f"surface {key}")
        paths = schema_report.mapping(entry.get("paths"), f"surface {key} paths")
        assert paths, f"surface {key} must record at least one path"
        for name, recorded in paths.items():
            digest = hashlib.sha256((REPOSITORY / name).read_bytes()).hexdigest()
            assert recorded == digest, name


def test_schema_report_fails_closed_on_drift(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    handoff = _write_fixture(tmp_path)
    args = ["--root", str(tmp_path), "--handoff", str(handoff)]
    assert schema_report.main(args) == 0
    captured = capsys.readouterr()
    payload: object = json.loads(captured.out)
    assert isinstance(payload, dict)
    baseline = tmp_path / "baseline.json"
    baseline.write_text(captured.out, encoding="utf-8")

    (tmp_path / "pkg" / "c.rs").write_text("tampered\n", encoding="utf-8")
    with pytest.raises(SystemExit) as drifted:
        schema_report.main([*args, "--baseline", str(baseline)])
    assert drifted.value.code != 0
    assert "pkg/c.rs" in capsys.readouterr().err

    (tmp_path / "pkg" / "d.rs").unlink()
    with pytest.raises(SystemExit) as missing:
        schema_report.main(args)
    assert missing.value.code != 0
    assert "pkg/d.rs" in capsys.readouterr().err


def test_schema_report_rejects_unknown_surface_shape(tmp_path: Path) -> None:
    malformed = tmp_path / "malformed.md"
    malformed.write_text(
        "### 5.1 shapes\n"
        "\n"
        "| a | b | c |\n"
        "| --- | --- | --- |\n"
        "| only | three | cells |\n"
        "\n"
        "### 5.2 next\n",
        encoding="utf-8",
    )
    with pytest.raises(ValueError, match="4 cells"):
        schema_report.parse_section(malformed.read_text(encoding="utf-8"))

    duplicated = tmp_path / "duplicated.md"
    duplicated.write_text(
        "### 5.1 shapes\n"
        "\n"
        "| surface | current | source | boundary |\n"
        "| --- | --- | --- | --- |\n"
        "| Alpha | `pkg/a.rs` | text | text |\n"
        "| ALPHA | `pkg/b.json` | text | text |\n"
        "\n"
        "### 5.2 next\n",
        encoding="utf-8",
    )
    with pytest.raises(ValueError, match="duplicate"):
        schema_report.parse_section(duplicated.read_text(encoding="utf-8"))
