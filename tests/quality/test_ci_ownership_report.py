"""Ownership matrix tests for the Normal CI region/job/check contract.

Pins the exact region set, job set, required-check identity, aggregate
expressions, and representative fixture outcomes against the real
``normal-ci.yml``; cross-checks the job gates against independently
re-derived ``if:`` references; and proves the report fails closed on
mutated workflow copies.
"""

from __future__ import annotations

import json
import re
from pathlib import Path
from typing import cast

import pytest

from scripts.acceptance import ci_ownership_report
from scripts.ci.regions import REGIONS

REPOSITORY = Path(__file__).resolve().parents[2]
WORKFLOW = REPOSITORY / ".github/workflows/normal-ci.yml"

EXPECTED_JOB_IDS = (
    "plan",
    "fast",
    "rust_contracts",
    "rust_clippy",
    "python_tests",
    "routing_mutations",
    "web",
    "product_flake",
    "remote_flake",
    "performance",
    "production_perf",
    "windows",
    "required",
)

EXPECTED_NEEDS = (
    "plan",
    "fast",
    "rust_contracts",
    "rust_clippy",
    "python_tests",
    "routing_mutations",
    "web",
    "product_flake",
    "remote_flake",
    "performance",
    "production_perf",
    "windows",
)

EXPECTED_AGGREGATE_KEYS = (
    "fast",
    "rust_contracts",
    "rust_clippy",
    "python_tests",
    "routing_mutations",
    "web",
    "product_flake",
    "remote_flake",
    "performance",
    "production_perf",
    "windows",
)

EXPECTED_AGGREGATE_REGIONS = {
    "fast": [],
    "rust_contracts": ["contracts", "rust"],
    "rust_clippy": ["rust"],
    "python_tests": ["python"],
    "routing_mutations": ["routing"],
    "web": ["web"],
    "product_flake": ["product"],
    "remote_flake": ["remote_flake"],
    "performance": ["product"],
    "production_perf": ["product"],
    "windows": ["rust"],
}

EXPECTED_FIXTURES = {
    "docs": {
        "regions": ["docs"],
        "applicable_jobs": ["fast"],
    },
    "contracts": {
        "regions": ["contracts"],
        "applicable_jobs": ["fast", "rust_contracts"],
    },
    "rust": {
        "regions": ["rust"],
        "applicable_jobs": ["fast", "rust_contracts", "rust_clippy", "windows"],
    },
    "python": {
        "regions": ["python"],
        "applicable_jobs": ["fast", "python_tests"],
    },
    "routing": {
        "regions": ["routing"],
        "applicable_jobs": ["fast", "routing_mutations"],
    },
    # Every web/ path also selects the product region by construction.
    "web": {
        "regions": ["product", "web"],
        "applicable_jobs": [
            "fast",
            "web",
            "product_flake",
            "performance",
            "production_perf",
        ],
    },
    "product": {
        "regions": ["product"],
        "applicable_jobs": ["fast", "product_flake", "performance", "production_perf"],
    },
    # Every remote-flake path overlaps the product, routing, and rust
    # regions; flake.lock/flake.nix additionally fail closed to all regions.
    "remote_flake": {
        "regions": ["product", "remote_flake", "routing", "rust"],
        "applicable_jobs": [
            "fast",
            "rust_contracts",
            "rust_clippy",
            "routing_mutations",
            "product_flake",
            "remote_flake",
            "performance",
            "production_perf",
            "windows",
        ],
    },
    "mixed": {
        "regions": ["docs", "rust"],
        "applicable_jobs": ["fast", "rust_contracts", "rust_clippy", "windows"],
    },
}

_JOB_IF_RE = re.compile(r"(?m)^    if: (.*)$")
_PLAN_OUTPUT_RE = re.compile(r"(?m)^      ([A-Za-z0-9_]+): \$\{\{ steps\.regions\.")
_JOB_HEADER_RE = re.compile(r"(?m)^  ([A-Za-z0-9_-]+):\s*$")
_OUTPUT_REF_RE = re.compile(r"needs\.plan\.outputs\.([A-Za-z0-9_]+)")


def _str_list(value: object, label: str) -> list[str]:
    assert isinstance(value, list), f"{label} must be a list"
    items = cast("list[object]", value)
    assert all(isinstance(item, str) for item in items), f"{label} must list strings"
    return cast("list[str]", items)


def _run_report(arguments: list[str], capsys: pytest.CaptureFixture[str]) -> object:
    assert ci_ownership_report.main(arguments) == 0
    return json.loads(capsys.readouterr().out)


def _real_report(capsys: pytest.CaptureFixture[str]) -> dict[str, object]:
    payload = _run_report(["--workflow", str(WORKFLOW)], capsys)
    assert isinstance(payload, dict)
    return cast("dict[str, object]", payload)


def _rederive_job_conditions(text: str) -> dict[str, str | None]:
    """Re-derive each job-level if: without touching the report module."""
    body = text.split("\njo" + "bs:\n", 1)[1]
    headers = list(_JOB_HEADER_RE.finditer(body))
    conditions: dict[str, str | None] = {}
    for index, header in enumerate(headers):
        end = headers[index + 1].start() if index + 1 < len(headers) else len(body)
        block = body[header.end() : end]
        match = _JOB_IF_RE.search(block)
        if match is None:
            conditions[header.group(1)] = None
            continue
        first = match.group(1).strip()
        if first not in (">", ">-", "|", "|-"):
            conditions[header.group(1)] = first
            continue
        continued: list[str] = []
        for line in block[match.end() :].splitlines()[1:]:
            if not line.strip():
                continue
            if len(line) - len(line.lstrip(" ")) < 6:
                break
            continued.append(line.strip())
        conditions[header.group(1)] = " ".join(continued)
    return conditions


def test_region_set_matches_stable_regions(
    capsys: pytest.CaptureFixture[str],
) -> None:
    report = _real_report(capsys)
    assert report["schema"] == "ci-ownership-report/1"
    assert report["regions"] == [region.value for region in REGIONS]


def test_job_id_set_is_exact(capsys: pytest.CaptureFixture[str]) -> None:
    report = _real_report(capsys)
    jobs = ci_ownership_report.mapping(report.get("jobs"), "ownership jobs")
    assert sorted(jobs) == sorted(EXPECTED_JOB_IDS)
    assert _str_list(report.get("job_order"), "ownership job order") == list(
        EXPECTED_JOB_IDS
    )


def test_required_check_identity_is_exact(
    capsys: pytest.CaptureFixture[str],
) -> None:
    report = _real_report(capsys)
    required = ci_ownership_report.mapping(report.get("required"), "ownership required")
    assert required["check_name"] == "Normal CI Required"
    assert _str_list(required.get("needs"), "ownership needs") == list(EXPECTED_NEEDS)
    aggregates = ci_ownership_report.mapping(
        required.get("aggregates"), "ownership aggregates"
    )
    assert sorted(aggregates) == sorted(EXPECTED_AGGREGATE_KEYS)
    for job_id, expected in EXPECTED_AGGREGATE_REGIONS.items():
        entry = ci_ownership_report.mapping(
            aggregates[job_id], f"ownership aggregate {job_id}"
        )
        assert sorted(_str_list(entry.get("regions"), f"aggregate {job_id}")) == (
            expected
        ), job_id


def test_job_gates_match_aggregate_expressions(
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Cross-layer check: job if: refs must equal aggregate expression refs."""
    report = _real_report(capsys)
    text = WORKFLOW.read_text(encoding="utf-8")
    known = {region.value for region in REGIONS} | {
        "regions_json",
        "performance_baseline",
    }
    conditions = _rederive_job_conditions(text)
    assert sorted(conditions) == sorted(EXPECTED_JOB_IDS)
    jobs = ci_ownership_report.mapping(report.get("jobs"), "ownership jobs")
    required = ci_ownership_report.mapping(report.get("required"), "ownership required")
    aggregates = ci_ownership_report.mapping(
        required.get("aggregates"), "ownership aggregates"
    )
    for job_id, condition in conditions.items():
        refs = _OUTPUT_REF_RE.findall(condition or "")
        assert set(refs) <= known, job_id
        entry = ci_ownership_report.mapping(jobs[job_id], f"ownership job {job_id}")
        gates = _str_list(entry.get("gates"), f"job {job_id} gates")
        region_refs = sorted(
            ref
            for ref in set(refs)
            if ref not in ("performance_baseline", "regions_json")
        )
        assert sorted(gates) == region_refs, job_id
        if job_id in aggregates:
            aggregate = ci_ownership_report.mapping(
                aggregates[job_id], f"ownership aggregate {job_id}"
            )
            aggregate_regions = _str_list(
                aggregate.get("regions"), f"aggregate {job_id} regions"
            )
            assert sorted(aggregate_regions) == region_refs, job_id


def test_plan_outputs_map_to_known_names(
    capsys: pytest.CaptureFixture[str],
) -> None:
    report = _real_report(capsys)
    text = WORKFLOW.read_text(encoding="utf-8")
    known = {region.value for region in REGIONS} | {
        "regions_json",
        "performance_baseline",
    }
    parsed = _PLAN_OUTPUT_RE.findall(text)
    assert parsed, "plan outputs block is unparsable"
    assert set(parsed) <= known
    assert {region.value for region in REGIONS} <= set(parsed)
    plan_outputs = ci_ownership_report.mapping(
        report.get("plan_outputs"), "ownership plan outputs"
    )
    assert sorted(_str_list(plan_outputs.get("regions"), "plan regions")) == sorted(
        region.value for region in REGIONS
    )
    assert plan_outputs.get("payload") == ["regions_json"]
    assert plan_outputs.get("auxiliary") == ["performance_baseline"]


def test_fixtures_pin_regions_and_applicable_jobs(
    capsys: pytest.CaptureFixture[str],
) -> None:
    report = _real_report(capsys)
    fixtures_raw = report.get("fixtures")
    assert isinstance(fixtures_raw, list)
    fixtures = cast("list[object]", fixtures_raw)
    assert [cast("dict[str, object]", f).get("name") for f in fixtures] == [
        *(region.value for region in REGIONS),
        "mixed",
    ]
    by_name = {
        cast("str", cast("dict[str, object]", f).get("name")): cast(
            "dict[str, object]", f
        )
        for f in fixtures
    }
    for name, expected in EXPECTED_FIXTURES.items():
        entry = by_name[name]
        assert entry.get("regions") == expected["regions"], name
        assert entry.get("applicable_jobs") == expected["applicable_jobs"], name
        paths = cast("list[object]", entry.get("changed_paths"))
        assert paths, name
    mixed_regions = set(_str_list(by_name["mixed"].get("regions"), "mixed regions"))
    assert mixed_regions == set(
        _str_list(by_name["docs"].get("regions"), "docs regions")
    ) | set(_str_list(by_name["rust"].get("regions"), "rust regions"))
    mixed_jobs = set(
        _str_list(by_name["mixed"].get("applicable_jobs"), "mixed applicable jobs")
    )
    assert mixed_jobs == set(
        _str_list(by_name["docs"].get("applicable_jobs"), "docs applicable jobs")
    ) | set(_str_list(by_name["rust"].get("applicable_jobs"), "rust applicable jobs"))


def _mutated_workflow(tmp_path: Path, old: str, new: str) -> Path:
    text = WORKFLOW.read_text(encoding="utf-8")
    assert old in text
    mutated = tmp_path / "normal-ci.yml"
    mutated.write_text(text.replace(old, new, 1), encoding="utf-8")
    return mutated


def _remove_line_containing(tmp_path: Path, needle: str) -> Path:
    text = WORKFLOW.read_text(encoding="utf-8")
    lines = [line for line in text.splitlines(keepends=True) if needle not in line]
    assert len(lines) < len(text.splitlines(keepends=True))
    mutated = tmp_path / "normal-ci.yml"
    mutated.write_text("".join(lines), encoding="utf-8")
    return mutated


def test_dropped_aggregate_entry_fails_closed(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    mutated = _remove_line_containing(
        tmp_path, '"web":{"applicable":${{ needs.plan.outputs.web'
    )
    with pytest.raises(SystemExit) as failure:
        ci_ownership_report.main(["--workflow", str(mutated)])
    assert failure.value.code != 0
    assert "web" in capsys.readouterr().err


def test_unknown_region_in_job_gate_fails_closed(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    mutated = _mutated_workflow(
        tmp_path,
        "if: needs.plan.outputs.python == 'true'",
        "if: needs.plan.outputs.coffee == 'true'",
    )
    with pytest.raises(SystemExit) as failure:
        ci_ownership_report.main(["--workflow", str(mutated)])
    assert failure.value.code != 0
    assert "coffee" in capsys.readouterr().err


def test_removed_region_output_fails_closed(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    mutated = _remove_line_containing(
        tmp_path, "      routing: ${{ steps.regions.outputs.routing }}"
    )
    with pytest.raises(SystemExit) as failure:
        ci_ownership_report.main(["--workflow", str(mutated)])
    assert failure.value.code != 0
    assert "routing" in capsys.readouterr().err


def test_removed_needs_entry_fails_closed(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    mutated = _remove_line_containing(tmp_path, "      - windows\n")
    with pytest.raises(SystemExit) as failure:
        ci_ownership_report.main(["--workflow", str(mutated)])
    assert failure.value.code != 0
    assert "windows" in capsys.readouterr().err


def test_removed_result_argument_fails_closed(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    mutated = _remove_line_containing(tmp_path, "windows=${{ needs.windows.result }}")
    with pytest.raises(SystemExit) as failure:
        ci_ownership_report.main(["--workflow", str(mutated)])
    assert failure.value.code != 0
    assert "windows" in capsys.readouterr().err
