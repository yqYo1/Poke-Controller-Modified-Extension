from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path
from typing import TYPE_CHECKING, cast

import pytest

from scripts.ci.timing import (
    ChangeKind,
    comparison_document,
    p95_document,
    parse_report,
)

if TYPE_CHECKING:
    from collections.abc import Sequence

REPOSITORY = Path(__file__).resolve().parents[2]
TIMING = REPOSITORY / "scripts/ci/timing.py"
STORE_HASH = "0" * 32


def _store_path(name: str) -> str:
    return f"/nix/store/{STORE_HASH}-{name}"


def _report(
    *,
    index: int = 1,
    sha: str | None = None,
    attempt: int = 1,
    change_kind: str = "fast",
    regions: Sequence[str] = ("contracts", "rust"),
    measured_wall_seconds: float = 120.0,
    workflow_wall_seconds: float = 125.0,
    built_derivations: Sequence[str] = (),
    substituted_store_paths: Sequence[str] = (),
    cache_read: bool = True,
    cache_write: bool = False,
    cache_actor: str = "octocat",
    cache_event: str = "pull_request",
) -> dict[str, object]:
    return {
        "schema_version": 1,
        "sha": sha if sha is not None else f"{index:040x}",
        "attempt": attempt,
        "change_kind": change_kind,
        "regions": list(regions),
        "measured_wall_seconds": measured_wall_seconds,
        "workflow": {
            "name": "Continuous Integration",
            "wall_seconds": workflow_wall_seconds,
        },
        "jobs": [
            {
                "name": "fast",
                "conclusion": "success",
                "wall_seconds": 100.0,
                "steps": [
                    {"name": "test", "wall_seconds": 70.0},
                    {"name": "checkout", "wall_seconds": 5.0},
                ],
                "built_derivations": list(built_derivations),
                "substituted_store_paths": list(substituted_store_paths),
            }
        ],
        "cache": {
            "read": cache_read,
            "write": cache_write,
            "actor": cache_actor,
            "event": cache_event,
        },
    }


def _source(report: dict[str, object]) -> str:
    return json.dumps(report, separators=(",", ":"))


def _run(command: str, *reports: dict[str, object]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(  # noqa: S603 - fixed repository script and fixture data
        [sys.executable, "-I", str(TIMING), command, *map(_source, reports)],
        check=False,
        capture_output=True,
        text=True,
        timeout=5,
    )


def _document(completed: subprocess.CompletedProcess[str]) -> dict[str, object]:
    raw_document: object = json.loads(completed.stdout)
    assert isinstance(raw_document, dict)
    return cast("dict[str, object]", raw_document)


def _assert_canonical_stdout(completed: subprocess.CompletedProcess[str]) -> None:
    document = _document(completed)
    expected = json.dumps(
        document,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    )
    assert completed.stdout == f"{expected}\n"


def test_validate_emits_complete_canonical_timing_evidence() -> None:
    report = _report(
        sha="A" * 40,
        built_derivations=(
            _store_path("pokecon.drv"),
            _store_path("pokecon-web.drv"),
        ),
        substituted_store_paths=(_store_path("pokecon-source"),),
    )

    completed = _run("validate", report)

    assert completed.returncode == 0
    assert completed.stderr == ""
    _assert_canonical_stdout(completed)
    document = _document(completed)
    assert document["command"] == "validate"
    assert document["conclusion"] == "success"
    validated = cast("dict[str, object]", document["report"])
    assert validated["sha"] == "a" * 40
    assert validated["schema_version"] == 1
    assert validated["measured_wall_seconds"] == 120.0
    assert validated["regions"] == ["contracts", "rust"]
    jobs = cast("list[dict[str, object]]", validated["jobs"])
    assert jobs[0]["steps"] == [
        {"name": "checkout", "wall_seconds": 5.0},
        {"name": "test", "wall_seconds": 70.0},
    ]


def test_validate_rejects_non_push_cache_write_as_contract_violation() -> None:
    completed = _run(
        "validate",
        _report(cache_write=True, cache_event="pull_request"),
    )

    assert completed.returncode == 1
    assert "cache writes are permitted only for push events" in completed.stderr
    assert _document(completed)["conclusion"] == "failure"


def test_validate_rejects_substitution_when_cache_read_is_false() -> None:
    completed = _run(
        "validate",
        _report(
            cache_read=False,
            substituted_store_paths=(_store_path("pokecon"),),
        ),
    )

    assert completed.returncode == 1
    assert "cache.read is false" in completed.stderr


def test_p95_uses_nearest_rank_and_only_top_level_measured_wall_time() -> None:
    reports = [
        _report(
            index=index,
            measured_wall_seconds=float(value),
            workflow_wall_seconds=9999.0,
        )
        for index, value in enumerate(
            (10, 20, 30, 40, 50, 60, 70, 80, 90, 180),
            start=1,
        )
    ]

    completed = _run("p95", *reversed(reports))

    assert completed.returncode == 0
    assert completed.stderr == ""
    _assert_canonical_stdout(completed)
    document = _document(completed)
    assert document == {
        "change_kind": "fast",
        "command": "p95",
        "conclusion": "success",
        "nearest_rank": 10,
        "p95_wall_seconds": 180.0,
        "sample_count": 10,
        "samples": [
            {
                "attempt": 1,
                "measured_wall_seconds": float(value),
                "sha": f"{index:040x}",
            }
            for index, value in enumerate(
                (10, 20, 30, 40, 50, 60, 70, 80, 90, 180),
                start=1,
            )
        ],
        "threshold_seconds": 180.0,
        "violations": [],
    }


def test_nearest_rank_for_twenty_samples_selects_rank_nineteen() -> None:
    reports = tuple(
        parse_report(
            _source(
                _report(
                    index=index,
                    change_kind="docs",
                    regions=("docs",),
                    measured_wall_seconds=float(index),
                )
            )
        )
        for index in range(1, 21)
    )

    document = p95_document(reports)

    assert document["nearest_rank"] == 19
    assert document["p95_wall_seconds"] == 19.0
    assert document["threshold_seconds"] == 300.0


@pytest.mark.parametrize(
    ("change_kind", "threshold"),
    [("fast", 180.0), ("docs", 300.0), ("product", 600.0)],
)
def test_p95_threshold_regression_is_a_contract_failure(
    change_kind: str, threshold: float
) -> None:
    reports = [
        _report(
            index=index,
            change_kind=change_kind,
            measured_wall_seconds=threshold + (1.0 if index == 10 else 0.0),
        )
        for index in range(1, 11)
    ]

    completed = _run("p95", *reports)

    assert completed.returncode == 1
    assert f"exceeds threshold {threshold} seconds" in completed.stderr
    assert _document(completed)["conclusion"] == "failure"


def test_p95_requires_at_least_ten_reports() -> None:
    completed = _run("p95", *(_report(index=index) for index in range(1, 10)))

    assert completed.returncode == 1
    assert "p95 requires at least 10 reports; got 9" in completed.stderr


def test_p95_requires_one_change_kind() -> None:
    reports = [
        _report(index=index, change_kind="docs" if index == 10 else "fast")
        for index in range(1, 11)
    ]

    completed = _run("p95", *reports)

    assert completed.returncode == 1
    assert "p95 requires reports of exactly one change_kind" in completed.stderr
    document = _document(completed)
    assert document["change_kind"] is None
    assert document["p95_wall_seconds"] == 120.0
    assert document["threshold_seconds"] is None


def test_compare_accepts_concrete_cache_reuse_improvement() -> None:
    sha = "a" * 40
    first = _report(
        sha=sha,
        attempt=1,
        change_kind="product",
        regions=("product", "rust", "web"),
        measured_wall_seconds=500.0,
        built_derivations=(
            _store_path("pokecon.drv"),
            _store_path("pokecon-web.drv"),
            _store_path("pokecon-test.drv"),
        ),
        cache_event="push",
        cache_write=True,
    )
    second = _report(
        sha=sha,
        attempt=2,
        change_kind="product",
        regions=("product", "rust", "web"),
        measured_wall_seconds=300.0,
        built_derivations=(_store_path("pokecon-test.drv"),),
        substituted_store_paths=(
            _store_path("unrelated-dependency"),
            _store_path("pokecon-0.1.0"),
        ),
        cache_event="push",
    )

    completed = _run("compare", first, second)

    assert completed.returncode == 0
    assert completed.stderr == ""
    _assert_canonical_stdout(completed)
    document = _document(completed)
    assert document["conclusion"] == "success"
    assert document["built_derivations_decreased"] is True
    assert document["wall_seconds_decreased"] is True
    assert document["substituted_pokecon_paths_present"] is True
    second_summary = cast("dict[str, object]", document["second"])
    assert second_summary["substituted_pokecon_paths"] == [_store_path("pokecon-0.1.0")]


@pytest.mark.parametrize(
    ("mutation", "diagnostic"),
    [
        ("sha", "compare requires the same SHA"),
        ("change_kind", "compare requires the same change_kind"),
        ("attempt", "second attempt must be later"),
        ("substitution", "no substituted PokeCon store paths"),
        ("builds", "must build fewer derivations"),
        ("wall", "must have lower measured wall time"),
    ],
)
def test_compare_rejects_missing_concrete_improvement(
    mutation: str, diagnostic: str
) -> None:
    sha = "b" * 40
    first = _report(
        sha=sha,
        attempt=1,
        measured_wall_seconds=100.0,
        built_derivations=(
            _store_path("pokecon.drv"),
            _store_path("pokecon-web.drv"),
        ),
    )
    second = _report(
        sha="c" * 40 if mutation == "sha" else sha,
        attempt=1 if mutation == "attempt" else 2,
        change_kind="docs" if mutation == "change_kind" else "fast",
        measured_wall_seconds=100.0 if mutation == "wall" else 50.0,
        built_derivations=(
            (_store_path("pokecon.drv"), _store_path("pokecon-web.drv"))
            if mutation == "builds"
            else (_store_path("pokecon.drv"),)
        ),
        substituted_store_paths=(
            (_store_path("dependency"),)
            if mutation == "substitution"
            else (_store_path("pokecon"),)
        ),
    )

    completed = _run("compare", first, second)

    assert completed.returncode == 1
    assert diagnostic in completed.stderr
    assert _document(completed)["conclusion"] == "failure"


def test_cache_hit_flag_cannot_replace_concrete_substitution_evidence() -> None:
    report = _report()
    cache = cast("dict[str, object]", report["cache"])
    cache["cache_hit"] = True

    completed = _run("validate", report)

    assert completed.returncode == 2
    assert completed.stdout == ""
    assert "unexpected keys: cache_hit" in completed.stderr


@pytest.mark.parametrize(
    ("mutation", "diagnostic"),
    [
        ("schema", "schema_version must equal 1"),
        ("sha", "exactly 40 hexadecimal"),
        ("attempt", "attempt must be an integer"),
        ("kind", "unsupported value"),
        ("regions_order", "regions must be sorted"),
        ("regions_duplicate", "regions must contain unique values"),
        ("duration", "must be a non-negative finite number"),
        ("derivation", "contains non-derivation path"),
        ("extra", "unexpected keys: extra"),
    ],
)
def test_report_schema_errors_exit_two(mutation: str, diagnostic: str) -> None:
    report = _report()
    if mutation == "schema":
        report["schema_version"] = 2
    elif mutation == "sha":
        report["sha"] = "short"
    elif mutation == "attempt":
        report["attempt"] = 0
    elif mutation == "kind":
        report["change_kind"] = "other"
    elif mutation == "regions_order":
        report["regions"] = ["rust", "contracts"]
    elif mutation == "regions_duplicate":
        report["regions"] = ["rust", "rust"]
    elif mutation == "duration":
        report["measured_wall_seconds"] = -1
    elif mutation == "derivation":
        jobs = cast("list[dict[str, object]]", report["jobs"])
        jobs[0]["built_derivations"] = [_store_path("pokecon")]
    elif mutation == "extra":
        report["extra"] = True
    else:
        raise AssertionError(mutation)

    completed = _run("validate", report)

    assert completed.returncode == 2
    assert completed.stdout == ""
    assert completed.stderr.startswith("ci-timing: input error: ")
    assert diagnostic in completed.stderr


def test_non_finite_json_duration_is_rejected() -> None:
    source = _source(_report()).replace("120.0", "NaN", 1)
    completed = subprocess.run(  # noqa: S603 - fixed repository script
        [sys.executable, "-I", str(TIMING), "validate", source],
        check=False,
        capture_output=True,
        text=True,
        timeout=5,
    )

    assert completed.returncode == 2
    assert completed.stdout == ""
    assert "non-finite number 'NaN'" in completed.stderr


def test_public_compare_uses_measured_duration_and_derivation_counts() -> None:
    sha = "d" * 40
    first = parse_report(
        _source(
            _report(
                sha=sha,
                attempt=1,
                measured_wall_seconds=90.0,
                workflow_wall_seconds=10.0,
                built_derivations=(
                    _store_path("pokecon.drv"),
                    _store_path("pokecon-web.drv"),
                ),
            )
        )
    )
    second = parse_report(
        _source(
            _report(
                sha=sha,
                attempt=2,
                measured_wall_seconds=80.0,
                workflow_wall_seconds=999.0,
                built_derivations=(_store_path("pokecon.drv"),),
                substituted_store_paths=(_store_path("pokecon"),),
            )
        )
    )

    document = comparison_document(first, second)

    assert first.change_kind is ChangeKind.FAST
    assert document["conclusion"] == "success"
    assert document["wall_seconds_decreased"] is True
