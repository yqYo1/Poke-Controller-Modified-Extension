"""Boundary report tests for handoff section 5.3 strong boundaries.

Pins the exact set of justified boundaries, checks that every recorded sha256
matches an independently recomputed file digest, pins the abstraction baseline
counts, and proves the report fails closed on drift, missing files, malformed
rows, and break-red mutations (missing necessity, synthetic extra boundary).
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import cast

import pytest

from scripts.acceptance import boundary_report

REPOSITORY = Path(__file__).resolve().parents[2]
HANDOFF = REPOSITORY / "docs/ARCHITECTURE_HANDOFF.md"

EXPECTED_IDS = frozenset(
    {
        "worker-process",
        "ipc-wire-value",
        "lan-http-trust",
        "os-platform-conditional",
        "os-bundle-distribution",
    }
)

EXPECTED_ABSTRACTION_COUNTS = {
    "member_count": 1,
    "dependency_count": 53,
    "trait_count": 21,
    "service_count": 10,
    "bin_count": 6,
    "conversion_file_count": 14,
}

FIXTURE_HANDOFF = """\
# fixture handoff

### 5.3 fixture boundaries

| boundary | necessity | evidence |
| --- | --- | --- |
| `alpha-boundary` | Alpha must stay isolated. | `pkg/a.rs:1` |
| `beta-boundary` | Beta must stay typed. | `pkg/b.json` |

### 5.4 next
"""

FIXTURE_REGISTRY = {
    "schema_version": 1,
    "boundaries": [
        {
            "id": "alpha-boundary",
            "kind": "process",
            "necessity": "Alpha must stay isolated.",
            "trust": "n/a",
            "distribution": "n/a",
            "evidence": {
                "source_refs": ["pkg/a.rs:1"],
                "registry_refs": [],
                "check_refs": ["alpha_boundary_is_pinned"],
                "symbol_anchors": ["ALPHA_MARKER"],
            },
        },
        {
            "id": "beta-boundary",
            "kind": "trust",
            "necessity": "Beta must stay typed.",
            "trust": "typed only",
            "distribution": "n/a",
            "evidence": {
                "source_refs": ["pkg/b.json"],
                "registry_refs": [],
                "check_refs": ["flake-fixture-check"],
                "symbol_anchors": ["BETA_MARKER"],
            },
        },
    ],
    "non_boundaries": ["fixture-non-boundary"],
}


def _write_fixture(root: Path) -> Path:
    package = root / "pkg"
    package.mkdir()
    (package / "a.rs").write_text("ALPHA_MARKER line\n", encoding="utf-8")
    (package / "b.json").write_text('{"BETA_MARKER": true}\n', encoding="utf-8")
    registry_dir = root / "rust/pokecon/registry"
    registry_dir.mkdir(parents=True)
    (registry_dir / "strong_boundaries.json").write_text(
        json.dumps(FIXTURE_REGISTRY), encoding="utf-8"
    )
    baseline = root / "rust/pokecon/registry/abstraction_baseline.json"
    baseline.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "workspace_members": ["rust/pokecon"],
                "workspace_dependency_names": ["alpha-dep"],
                "bins": [
                    {
                        "name": "alpha-bin",
                        "path": "pkg/a.rs",
                        "required_features": [],
                        "boundary_ref": None,
                    }
                ],
                "features": ["alpha-feature"],
                "traits": [
                    {
                        "path": "pkg/a.rs",
                        "name": "AlphaTrait",
                        "justification": "fixture seam",
                    }
                ],
                "services": [{"path": "pkg/a.rs", "justification": "fixture service"}],
                "conversion_files": ["pkg/a.rs"],
            }
        ),
        encoding="utf-8",
    )
    test_dir = root / "rust/pokecon/tests"
    test_dir.mkdir(parents=True, exist_ok=True)
    (test_dir / "contract_sync.rs").write_text(
        "fn alpha_boundary_is_pinned() {}\n", encoding="utf-8"
    )
    (root / "flake.nix").write_text("flake-fixture-check", encoding="utf-8")
    handoff = root / "HANDOFF.md"
    handoff.write_text(FIXTURE_HANDOFF, encoding="utf-8")
    return handoff


def test_boundary_report_lists_every_section_5_3_boundary() -> None:
    entries = boundary_report.parse_section(HANDOFF.read_text(encoding="utf-8"))
    assert set(entries) == set(EXPECTED_IDS)


def test_boundary_report_records_sha256_per_boundary() -> None:
    report = boundary_report.build_report(REPOSITORY, HANDOFF)
    assert report.get("schema") == "boundary-report/1"
    assert report.get("boundary_count") == len(EXPECTED_IDS)
    boundaries = boundary_report.mapping(report.get("boundaries"), "report boundaries")
    assert set(boundaries) == set(EXPECTED_IDS)
    for key in EXPECTED_IDS:
        entry = boundary_report.mapping(boundaries[key], f"boundary {key}")
        for field in ("kind", "necessity", "trust", "distribution"):
            assert isinstance(entry.get(field), str) and entry[field], (key, field)
        paths = boundary_report.mapping(entry.get("paths"), f"boundary {key} paths")
        assert paths, f"boundary {key} must record at least one path"
        for name, recorded in paths.items():
            digest = hashlib.sha256((REPOSITORY / name).read_bytes()).hexdigest()
            assert recorded == digest, name


def test_boundary_report_pins_abstraction_counts() -> None:
    report = boundary_report.build_abstraction_report(REPOSITORY)
    assert report.get("schema") == "abstraction-report/1"
    for field, expected in EXPECTED_ABSTRACTION_COUNTS.items():
        assert report.get(field) == expected, field
    assert (
        len(cast("list[object]", report["dependencies"]))
        == (EXPECTED_ABSTRACTION_COUNTS["dependency_count"])
    )


def test_boundary_report_fails_closed_on_drift(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    handoff = _write_fixture(tmp_path)
    args = ["--root", str(tmp_path), "--handoff", str(handoff), "--check"]
    assert boundary_report.main(args) == 0
    captured = capsys.readouterr()
    payload: object = json.loads(captured.out)
    assert isinstance(payload, dict)
    assert payload["schema"] == "boundary-report/1"
    baseline = tmp_path / "baseline.json"
    baseline.write_text(captured.out, encoding="utf-8")

    (tmp_path / "pkg" / "a.rs").write_text(
        "ALPHA_MARKER line\nappended\n", encoding="utf-8"
    )
    with pytest.raises(SystemExit) as drifted:
        boundary_report.main([*args, "--baseline", str(baseline)])
    assert drifted.value.code != 0
    assert "pkg/a.rs" in capsys.readouterr().err

    (tmp_path / "pkg" / "b.json").unlink()
    with pytest.raises(SystemExit) as missing:
        boundary_report.main(args)
    assert missing.value.code != 0
    assert "pkg/b.json" in capsys.readouterr().err


def test_boundary_report_rejects_unknown_boundary_shape(tmp_path: Path) -> None:
    malformed = tmp_path / "malformed.md"
    malformed.write_text(
        "### 5.3 shapes\n\n| a | b |\n| --- | --- |\n| only | two |\n\n### 5.4 next\n",
        encoding="utf-8",
    )
    with pytest.raises(ValueError, match="3 cells"):
        boundary_report.parse_section(malformed.read_text(encoding="utf-8"))

    duplicated = tmp_path / "duplicated.md"
    duplicated.write_text(
        "### 5.3 shapes\n"
        "\n"
        "| boundary | necessity | evidence |\n"
        "| --- | --- | --- |\n"
        "| `dup` | first. | `pkg/a.rs` |\n"
        "| `dup` | second. | `pkg/b.json` |\n"
        "\n"
        "### 5.4 next\n",
        encoding="utf-8",
    )
    with pytest.raises(ValueError, match="duplicate"):
        boundary_report.parse_section(duplicated.read_text(encoding="utf-8"))


def test_boundary_report_breaks_red_on_missing_necessity(tmp_path: Path) -> None:
    handoff = _write_fixture(tmp_path)
    registry_path = tmp_path / "rust/pokecon/registry/strong_boundaries.json"
    raw, boundaries = _load_fixture_registry(registry_path)
    first = cast("dict[str, object]", boundaries[0])
    first["necessity"] = ""
    registry_path.write_text(json.dumps(raw), encoding="utf-8")
    with pytest.raises(SystemExit) as missing:
        boundary_report.main(["--root", str(tmp_path), "--handoff", str(handoff)])
    assert missing.value.code != 0


def _load_fixture_registry(
    registry_path: Path,
) -> tuple[dict[str, object], list[object]]:
    raw: object = json.loads(registry_path.read_text(encoding="utf-8"))
    assert isinstance(raw, dict)
    registry = cast("dict[str, object]", raw)
    boundaries = registry.get("boundaries")
    assert isinstance(boundaries, list)
    return registry, cast("list[object]", boundaries)


def test_boundary_report_breaks_red_on_synthetic_extra_boundary(
    tmp_path: Path,
) -> None:
    handoff = _write_fixture(tmp_path)
    registry_path = tmp_path / "rust/pokecon/registry/strong_boundaries.json"
    raw, boundaries = _load_fixture_registry(registry_path)
    boundaries.append(
        {
            "id": "sentinel-extra-boundary",
            "kind": "process",
            "necessity": "Synthetic boundary with no handoff row.",
            "trust": "n/a",
            "distribution": "n/a",
            "evidence": {
                "source_refs": ["pkg/a.rs:1"],
                "registry_refs": [],
                "check_refs": ["alpha_boundary_is_pinned"],
                "symbol_anchors": ["ALPHA_MARKER"],
            },
        }
    )
    registry_path.write_text(json.dumps(raw), encoding="utf-8")
    with pytest.raises(ValueError, match=r"missing from handoff 5\.3"):
        boundary_report.build_report(tmp_path, handoff)


def test_boundary_report_output_writes_file_matching_stdout(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    handoff = _write_fixture(tmp_path)
    args = ["--root", str(tmp_path), "--handoff", str(handoff), "--check"]
    assert boundary_report.main(args) == 0
    baseline_stdout = capsys.readouterr().out
    payload: object = json.loads(baseline_stdout)
    assert isinstance(payload, dict)
    assert payload["schema"] == "boundary-report/1"

    output = tmp_path / "boundary-report.json"
    assert boundary_report.main([*args, "--output", str(output)]) == 0
    assert capsys.readouterr().out == baseline_stdout
    assert output.is_file() and not output.is_symlink()
    assert output.read_text(encoding="utf-8") == baseline_stdout


def test_abstraction_report_output_writes_file_matching_stdout(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    _write_fixture(tmp_path)
    args = ["--root", str(tmp_path), "--abstraction"]
    assert boundary_report.main(args) == 0
    baseline_stdout = capsys.readouterr().out
    payload: object = json.loads(baseline_stdout)
    assert isinstance(payload, dict)
    assert payload["schema"] == "abstraction-report/1"

    output = tmp_path / "abstraction-report.json"
    assert boundary_report.main([*args, "--output", str(output)]) == 0
    assert capsys.readouterr().out == baseline_stdout
    assert output.read_text(encoding="utf-8") == baseline_stdout


def test_boundary_report_output_is_fail_closed(tmp_path: Path) -> None:
    handoff = _write_fixture(tmp_path)
    args = ["--root", str(tmp_path), "--handoff", str(handoff)]

    output = tmp_path / "boundary-report.json"
    output.write_text("{}\n", encoding="utf-8")
    with pytest.raises(SystemExit) as existing:
        boundary_report.main([*args, "--output", str(output)])
    assert existing.value.code != 0

    dangling = tmp_path / "dangling.json"
    dangling.symlink_to(tmp_path / "nothing.json")
    with pytest.raises(SystemExit) as redirected:
        boundary_report.main([*args, "--output", str(dangling)])
    assert redirected.value.code != 0
    assert dangling.is_symlink()

    with pytest.raises(SystemExit) as missing_parent:
        boundary_report.main([*args, "--output", str(tmp_path / "absent" / "out.json")])
    assert missing_parent.value.code != 0
