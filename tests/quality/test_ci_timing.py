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
    run_status: str = "completed",
    run_started_at: str = "2026-01-01T00:00:00Z",
    run_conclusion: str | None = "success",
    collection_kind: str = "upstream_completed_max",
) -> dict[str, object]:
    return {
        "schema_version": 2,
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
                "wall_seconds": 5.0,
                "steps": [
                    {"name": "test", "wall_seconds": 4.0},
                    {"name": "checkout", "wall_seconds": 1.0},
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
        "run": {
            "started_at": run_started_at,
            "status": run_status,
            "conclusion": run_conclusion,
        },
        "collection": {"kind": collection_kind},
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
    assert validated["schema_version"] == 2
    assert validated["measured_wall_seconds"] == 120.0
    assert validated["regions"] == ["contracts", "rust"]
    jobs = cast("list[dict[str, object]]", validated["jobs"])
    assert jobs[0]["steps"] == [
        {"name": "checkout", "wall_seconds": 1.0},
        {"name": "test", "wall_seconds": 4.0},
    ]


def test_validate_rejects_non_push_cache_write_as_contract_violation() -> None:
    completed = _run(
        "validate",
        _report(cache_write=True, cache_event="pull_request"),
    )

    assert completed.returncode == 1
    assert "cache writes are permitted only for push events" in completed.stderr
    assert _document(completed)["conclusion"] == "failure"


def test_validate_rejects_untrusted_push_cache_writer() -> None:
    completed = _run(
        "validate",
        _report(
            cache_write=True,
            cache_event="push",
            cache_actor="untrusted-contributor",
        ),
    )

    assert completed.returncode == 1
    assert "cache writes require a trusted actor" in completed.stderr
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
    [("fast", 180.0), ("docs", 300.0), ("product", 720.0)],
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
        cache_actor="yqYo1",
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
        ("schema", "schema_version must equal 2"),
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
        report["schema_version"] = 999
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


def test_collect_builds_a_report_from_deterministic_github_json(tmp_path: Path) -> None:
    jobs = {
        "jobs": [
            {
                "name": "Fast checks",
                "conclusion": "success",
                "started_at": "2026-01-01T00:00:00Z",
                "completed_at": "2026-01-01T00:00:05Z",
                "steps": [
                    {
                        "name": "Run checks",
                        "started_at": "2026-01-01T00:00:01Z",
                        "completed_at": "2026-01-01T00:00:04Z",
                    }
                ],
            }
        ]
    }
    metadata = {
        "sha": "a" * 40,
        "attempt": 1,
        "change_kind": "fast",
        "regions": ["contracts", "rust"],
        "measured_wall_seconds": 5.0,
        "workflow": {"name": "Normal CI", "wall_seconds": 5.0},
        "cache": {
            "read": True,
            "write": False,
            "actor": "octocat",
            "event": "pull_request",
        },
        "jobs_evidence": {},
        "run": {
            "started_at": "2026-01-01T00:00:00Z",
            "status": "in_progress",
            "conclusion": None,
        },
        "collection": {"kind": "upstream_completed_max"},
    }
    output = tmp_path / "timing-report.json"
    completed = subprocess.run(  # noqa: S603 - fixed repository script and fixture data
        [
            sys.executable,
            "-I",
            str(TIMING),
            "collect",
            "--jobs",
            json.dumps(jobs),
            "--run-metadata",
            json.dumps(metadata),
            "--output",
            str(output),
        ],
        check=False,
        capture_output=True,
        text=True,
        timeout=5,
    )
    assert completed.returncode == 0, completed.stderr
    report = json.loads(output.read_text(encoding="utf-8"))
    assert report["sha"] == "a" * 40
    assert report["jobs"][0]["wall_seconds"] == 5.0


def test_collect_rejects_in_progress_job_status(tmp_path: Path) -> None:
    jobs: dict[str, object] = {
        "jobs": [
            {
                "name": "Fast checks",
                "conclusion": None,
                "status": "in_progress",
                "started_at": "2026-01-01T00:00:00Z",
                "completed_at": None,
                "steps": [],
            }
        ]
    }
    metadata = {
        "sha": "a" * 40,
        "attempt": 1,
        "change_kind": "fast",
        "regions": ["contracts", "rust"],
        "measured_wall_seconds": 5.0,
        "workflow": {"name": "Normal CI", "wall_seconds": 5.0},
        "cache": {
            "read": True,
            "write": False,
            "actor": "octocat",
            "event": "pull_request",
        },
        "jobs_evidence": {},
        "run": {
            "started_at": "2026-01-01T00:00:00Z",
            "status": "in_progress",
            "conclusion": None,
        },
        "collection": {"kind": "upstream_completed_max"},
    }
    output = tmp_path / "timing-report.json"
    completed = subprocess.run(  # noqa: S603
        [
            sys.executable,
            "-I",
            str(TIMING),
            "collect",
            "--jobs",
            json.dumps(jobs),
            "--run-metadata",
            json.dumps(metadata),
            "--output",
            str(output),
        ],
        check=False,
        capture_output=True,
        text=True,
        timeout=5,
    )
    assert completed.returncode == 2
    assert "is not completed" in completed.stderr
    assert not output.exists() or output.read_text(encoding="utf-8") == ""


def test_collect_rejects_under_measured_critical_path(tmp_path: Path) -> None:
    jobs = {
        "jobs": [
            {
                "name": "Fast checks",
                "conclusion": "success",
                "started_at": "2026-01-01T00:00:00Z",
                "completed_at": "2026-01-01T00:00:05Z",
                "steps": [
                    {
                        "name": "check",
                        "started_at": "2026-01-01T00:00:00Z",
                        "completed_at": "2026-01-01T00:00:05Z",
                    }
                ],
            },
            {
                "name": "Rust and contracts (Linux)",
                "conclusion": "success",
                "started_at": "2026-01-01T00:00:00Z",
                "completed_at": "2026-01-01T00:00:10Z",
                "steps": [
                    {
                        "name": "check",
                        "started_at": "2026-01-01T00:00:00Z",
                        "completed_at": "2026-01-01T00:00:10Z",
                    }
                ],
            },
        ]
    }
    metadata = {
        "sha": "a" * 40,
        "attempt": 1,
        "change_kind": "fast",
        "regions": ["contracts", "rust"],
        "measured_wall_seconds": 5.0,
        "workflow": {"name": "Normal CI", "wall_seconds": 5.0},
        "cache": {
            "read": True,
            "write": False,
            "actor": "octocat",
            "event": "pull_request",
        },
        "jobs_evidence": {},
        "run": {
            "started_at": "2026-01-01T00:00:00Z",
            "status": "in_progress",
            "conclusion": None,
        },
        "collection": {"kind": "upstream_completed_max"},
    }
    output = tmp_path / "timing-report.json"
    completed = subprocess.run(  # noqa: S603
        [
            sys.executable,
            "-I",
            str(TIMING),
            "collect",
            "--jobs",
            json.dumps(jobs),
            "--run-metadata",
            json.dumps(metadata),
            "--output",
            str(output),
        ],
        check=False,
        capture_output=True,
        text=True,
        timeout=5,
    )
    assert completed.returncode == 2
    assert "under-measures" in completed.stderr or "less than" in completed.stderr


def test_collect_succeeds_with_completed_critical_path(tmp_path: Path) -> None:
    jobs = {
        "jobs": [
            {
                "name": "Fast checks",
                "conclusion": "success",
                "started_at": "2026-01-01T00:00:00Z",
                "completed_at": "2026-01-01T00:00:07Z",
                "steps": [
                    {
                        "name": "check",
                        "started_at": "2026-01-01T00:00:00Z",
                        "completed_at": "2026-01-01T00:00:07Z",
                    }
                ],
            },
            {
                "name": "Rust and contracts (Linux)",
                "conclusion": "success",
                "started_at": "2026-01-01T00:00:00Z",
                "completed_at": "2026-01-01T00:00:12Z",
                "steps": [
                    {
                        "name": "check",
                        "started_at": "2026-01-01T00:00:00Z",
                        "completed_at": "2026-01-01T00:00:12Z",
                    }
                ],
            },
        ]
    }
    metadata = {
        "sha": "b" * 40,
        "attempt": 2,
        "change_kind": "fast",
        "regions": ["contracts", "rust"],
        "measured_wall_seconds": 12.0,
        "workflow": {"name": "Normal CI", "wall_seconds": 12.0},
        "cache": {
            "read": True,
            "write": False,
            "actor": "octocat",
            "event": "pull_request",
        },
        "jobs_evidence": {},
        "run": {
            "started_at": "2026-01-01T00:00:00Z",
            "status": "completed",
            "conclusion": "success",
        },
        "collection": {"kind": "upstream_completed_max"},
    }
    output = tmp_path / "timing-report.json"
    completed = subprocess.run(  # noqa: S603
        [
            sys.executable,
            "-I",
            str(TIMING),
            "collect",
            "--jobs",
            json.dumps(jobs),
            "--run-metadata",
            json.dumps(metadata),
            "--output",
            str(output),
        ],
        check=False,
        capture_output=True,
        text=True,
        timeout=5,
    )
    assert completed.returncode == 0, completed.stderr
    report = json.loads(output.read_text(encoding="utf-8"))
    assert report["measured_wall_seconds"] == 12.0
    assert report["jobs"][1]["wall_seconds"] == 12.0
    assert report["run"]["status"] == "completed"
    assert report["collection"]["kind"] == "upstream_completed_max"


def test_validate_rejects_measured_less_than_critical_path() -> None:
    report = _report(measured_wall_seconds=4.0, workflow_wall_seconds=4.0)
    completed = _run("validate", report)
    assert completed.returncode == 1
    assert "less than critical-path" in completed.stderr
    assert _document(completed)["conclusion"] == "failure"


def test_p95_accepts_json_report_paths(tmp_path: Path) -> None:
    reports = [_report(index=index) for index in range(1, 11)]
    paths: list[str] = []
    for index, report in enumerate(reports, start=1):
        path = tmp_path / f"timing-{index}.json"
        path.write_text(_source(report), encoding="utf-8")
        paths.append(str(path))

    completed = subprocess.run(  # noqa: S603 - fixed repository script and fixture data
        [sys.executable, "-I", str(TIMING), "p95", *paths],
        check=False,
        capture_output=True,
        text=True,
        timeout=5,
    )

    assert completed.returncode == 0
    assert _document(completed)["conclusion"] == "success"


def test_p95_rejects_unsuccessful_job_evidence() -> None:
    reports: list[dict[str, object]] = []
    for index in range(1, 11):
        report = _report(index=index)
        if index == 10:
            jobs = cast("list[dict[str, object]]", report["jobs"])
            jobs[0]["conclusion"] = "failure"
        reports.append(report)

    completed = _run("p95", *reports)

    assert completed.returncode == 1
    assert "unsuccessful jobs: fast" in completed.stderr


def test_p95_rejects_stale_history_older_than_30_days() -> None:
    newest = "2026-02-01T00:00:00Z"
    stale = "2025-12-20T00:00:00Z"
    reports = [
        _report(
            index=index,
            run_started_at=newest if index > 1 else stale,
            measured_wall_seconds=10.0,
        )
        for index in range(1, 11)
    ]
    completed = _run("p95", *reports)
    assert completed.returncode == 1
    assert "stale" in completed.stderr.lower()
    document = _document(completed)
    assert document["conclusion"] == "failure"
    raw_violations = document["violations"]
    assert isinstance(raw_violations, list)
    violations = cast("list[object]", raw_violations)
    assert any(isinstance(v, str) and "stale" in v.lower() for v in violations)


def test_p95_rejects_future_dated_history() -> None:
    # Future-dated sample makes older samples stale (>30d) relative to newest.
    newest = "2026-02-01T00:00:00Z"
    future = "2026-04-15T00:00:00Z"
    reports = [
        _report(
            index=index,
            run_started_at=future if index == 10 else newest,
            measured_wall_seconds=10.0,
        )
        for index in range(1, 11)
    ]
    completed = _run("p95", *reports)
    assert completed.returncode == 1
    assert "stale" in completed.stderr.lower()


def test_p95_history_gate_is_blocking_in_workflow() -> None:
    workflow = (REPOSITORY / ".github/workflows/normal-ci.yml").read_text(
        encoding="utf-8"
    )
    # Must contain blocking history gate step
    assert "Enforce timing p95 history gate (blocking, fail-closed)" in workflow
    assert "nix run .#ci-timing -- p95" in workflow
    assert 'select(.conclusion == "success" or .conclusion == "failure")' in workflow
    assert "timing-report.json" in workflow
    assert "ci-timing-${candidate_id}-${candidate_attempt}" in workflow
    assert 'gh api "/repos/${GITHUB_REPOSITORY}/actions/artifacts/' in workflow
    assert "if: always()" in workflow  # ensure artifact upload is not lost on failure
    assert "timing-p95.json" in workflow
    assert 'artifact_name="ci-timing-${candidate_id}-${candidate_attempt}"' in workflow
    assert "name == $name" in workflow
    assert 'name "timing-report.json" -print -quit' in workflow
    assert "if-no-files-found: warn" in workflow
    # Must not contain old caveat non-blocking wording
    assert "caveat: insufficient history for blocking p95" not in workflow
    # Must preserve completion-safe upstream_completed_max and cache trust
    assert "upstream_completed_max" in workflow
    assert "cache_read=true" in workflow or "cache.read" in workflow
    # Must handle malformed/stale/incomplete fail-closed and not claim invented p95
    assert (
        "incomplete history" in workflow.lower()
        or "insufficient history" in workflow.lower()
    )
    assert "malformed" in workflow.lower()
    assert "missing or invalid change_kind" in workflow


def test_p95_stale_threshold_matches_timing_contract() -> None:
    # Verify timing.py constants match contract scrutiny in contract_sync.rs
    source = TIMING.read_text(encoding="utf-8")
    assert "MAX_HISTORY_AGE_DAYS: Final = 30" in source
    assert "MAX_HISTORY_STALE_SECONDS" in source
    assert "report is stale" in source.lower()


def test_validate_accepts_none_with_empty_regions() -> None:
    completed = _run("validate", _report(change_kind="none", regions=()))

    assert completed.returncode == 0
    assert completed.stderr == ""
    _assert_canonical_stdout(completed)
    document = _document(completed)
    assert document["conclusion"] == "success"
    validated = cast("dict[str, object]", document["report"])
    assert validated["change_kind"] == "none"
    assert validated["regions"] == []


def test_validate_rejects_none_with_nonempty_regions() -> None:
    completed = _run("validate", _report(change_kind="none"))

    assert completed.returncode == 1
    assert "change_kind 'none' requires empty regions" in completed.stderr
    assert _document(completed)["conclusion"] == "failure"


@pytest.mark.parametrize("change_kind", ["fast", "docs", "product"])
def test_validate_rejects_timed_kind_with_empty_regions(change_kind: str) -> None:
    completed = _run("validate", _report(change_kind=change_kind, regions=()))

    assert completed.returncode == 1
    assert "must use change_kind 'none'" in completed.stderr
    assert _document(completed)["conclusion"] == "failure"


def test_p95_rejects_none_without_silent_threshold() -> None:
    reports = [
        _report(index=index, change_kind="none", regions=()) for index in range(1, 11)
    ]

    completed = _run("p95", *reports)

    assert completed.returncode == 1
    assert "p95 is not defined for change_kind 'none'" in completed.stderr
    document = _document(completed)
    assert document["conclusion"] == "failure"
    assert document["change_kind"] == "none"
    assert document["threshold_seconds"] is None


def test_p95_rejects_mixed_none_and_fast_history() -> None:
    reports = [
        _report(
            index=index,
            change_kind="none" if index == 10 else "fast",
            regions=() if index == 10 else ("contracts", "rust"),
        )
        for index in range(1, 11)
    ]

    completed = _run("p95", *reports)

    assert completed.returncode == 1
    assert "p95 is not defined for change_kind 'none'" in completed.stderr
    assert _document(completed)["conclusion"] == "failure"


def test_none_change_kind_has_no_p95_threshold() -> None:
    source = TIMING.read_text(encoding="utf-8")
    assert 'NONE = "none"' in source
    assert "ChangeKind.NONE" in source
    # none must stay outside the threshold table so p95 can never silently
    # calculate a threshold for zero-region runs.
    assert "NONE: " not in source
    assert "NONE:" not in source


def test_planning_only_runs_use_none_and_skip_p95_gate() -> None:
    workflow = (REPOSITORY / ".github/workflows/normal-ci.yml").read_text(
        encoding="utf-8"
    )
    # none is derived only when no region output is true; product/docs keep
    # priority and every other region keeps the fast default.
    assert "change_kind=none" in workflow
    assert "fast|docs|product|none" in workflow
    assert "skipping p95 history/threshold gate" in workflow
    # validate/upload still runs for none (only the p95 step exits early).
    assert "nix run .#ci-timing -- validate" in workflow
    assert "timing-report.json" in workflow
    # fail-closed gates for real timing kinds are unchanged.
    assert "fast) threshold=180" in workflow
    assert "docs) threshold=300" in workflow
    assert "product) threshold=720" in workflow
    assert "unknown change_kind" in workflow
