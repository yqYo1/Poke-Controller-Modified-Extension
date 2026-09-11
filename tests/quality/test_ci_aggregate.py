from __future__ import annotations

import json
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import cast

import pytest

from scripts.ci.aggregate import Result, evaluate, parse_plan, parse_results

REPOSITORY = Path(__file__).resolve().parents[2]
AGGREGATE = REPOSITORY / "scripts/ci/aggregate.py"
REGION_NAMES = (
    "docs",
    "contracts",
    "rust",
    "python",
    "routing",
    "web",
    "product",
    "remote_flake",
)


@dataclass(frozen=True, slots=True)
class AggregateFixture:
    plan: dict[str, object]
    results: tuple[str, ...]


def _plan(
    jobs: dict[str, object],
    *,
    plan_status: str = "success",
    selected_regions: tuple[str, ...] = (),
) -> dict[str, object]:
    selected = frozenset(selected_regions)
    assert selected <= frozenset(REGION_NAMES)
    regions = {name: name in selected for name in REGION_NAMES}
    return {
        "plan_status": plan_status,
        "regions": regions,
        "region_outputs": {name: str(value).lower() for name, value in regions.items()},
        "jobs": jobs,
    }


@pytest.fixture
def success_fixture() -> AggregateFixture:
    return AggregateFixture(
        plan=_plan(
            {
                "rust": {"applicable": True, "reason": "Rust source changed"},
                "fast": {"applicable": True, "reason": "always required"},
            },
            selected_regions=("rust",),
        ),
        results=("rust=success", "fast=success"),
    )


@pytest.fixture
def failure_fixture() -> AggregateFixture:
    return AggregateFixture(
        plan=_plan(
            {
                "rust": {"applicable": True, "reason": "Rust source changed"},
            },
            selected_regions=("rust",),
        ),
        results=("rust=failure",),
    )


@pytest.fixture
def explicit_skip_fixture() -> AggregateFixture:
    return AggregateFixture(
        plan=_plan(
            {
                "web": {
                    "applicable": False,
                    "reason": "no Web paths changed",
                },
                "fast": {"applicable": True, "reason": "always required"},
            },
            selected_regions=("docs",),
        ),
        results=("web=skipped", "fast=success"),
    )


def _run(fixture: AggregateFixture) -> subprocess.CompletedProcess[str]:
    return subprocess.run(  # noqa: S603 - fixed repository script and fixture data
        [
            sys.executable,
            "-I",
            str(AGGREGATE),
            json.dumps(fixture.plan),
            *fixture.results,
        ],
        check=False,
        capture_output=True,
        text=True,
        timeout=5,
    )


def _document(completed: subprocess.CompletedProcess[str]) -> dict[str, object]:
    raw_document: object = json.loads(completed.stdout)
    assert isinstance(raw_document, dict)
    return cast("dict[str, object]", raw_document)


def test_success_fixture_emits_canonical_report(
    success_fixture: AggregateFixture,
) -> None:
    completed = _run(success_fixture)

    assert completed.returncode == 0
    assert completed.stderr == ""
    document = _document(completed)
    assert completed.stdout == (
        json.dumps(
            document,
            ensure_ascii=False,
            separators=(",", ":"),
            sort_keys=True,
        )
        + "\n"
    )
    assert document == {
        "conclusion": "success",
        "extra_results": [],
        "jobs": [
            {
                "applicable": True,
                "expected_result": "success",
                "name": "fast",
                "reason": "always required",
                "result": "success",
            },
            {
                "applicable": True,
                "expected_result": "success",
                "name": "rust",
                "reason": "Rust source changed",
                "result": "success",
            },
        ],
        "plan_status": "success",
        "region_outputs": {
            "docs": "false",
            "contracts": "false",
            "rust": "true",
            "python": "false",
            "routing": "false",
            "web": "false",
            "product": "false",
            "remote_flake": "false",
        },
        "regions": {
            "docs": False,
            "contracts": False,
            "rust": True,
            "python": False,
            "routing": False,
            "web": False,
            "product": False,
            "remote_flake": False,
        },
        "violations": [],
    }


def test_explicit_skip_fixture_is_a_successful_aggregate(
    explicit_skip_fixture: AggregateFixture,
) -> None:
    completed = _run(explicit_skip_fixture)

    assert completed.returncode == 0
    assert completed.stderr == ""
    document = _document(completed)
    assert document["conclusion"] == "success"
    assert document["jobs"] == [
        {
            "applicable": True,
            "expected_result": "success",
            "name": "fast",
            "reason": "always required",
            "result": "success",
        },
        {
            "applicable": False,
            "expected_result": "skipped",
            "name": "web",
            "reason": "no Web paths changed",
            "result": "skipped",
        },
    ]


def test_failure_fixture_is_a_contract_failure(
    failure_fixture: AggregateFixture,
) -> None:
    completed = _run(failure_fixture)

    assert completed.returncode == 1
    assert completed.stderr == (
        "ci-aggregate: contract violation: job 'rust' is applicable and must "
        "conclude 'success', got 'failure'\n"
    )
    document = _document(completed)
    assert document["conclusion"] == "failure"
    assert document["violations"] == [
        "job 'rust' is applicable and must conclude 'success', got 'failure'"
    ]


@pytest.mark.parametrize("result", ["failure", "cancelled", "timed_out"])
def test_non_success_conclusions_fail_applicable_jobs(result: str) -> None:
    fixture = AggregateFixture(
        plan=_plan(
            {
                "checks": {"applicable": True, "reason": "source changed"},
            },
            selected_regions=("rust",),
        ),
        results=(f"checks={result}",),
    )

    completed = _run(fixture)

    assert completed.returncode == 1
    assert f"got '{result}'" in completed.stderr
    assert _document(completed)["conclusion"] == "failure"


def test_unexpected_skip_fails_an_applicable_job() -> None:
    fixture = AggregateFixture(
        plan=_plan(
            {
                "checks": {"applicable": True, "reason": "source changed"},
            },
            selected_regions=("rust",),
        ),
        results=("checks=skipped",),
    )

    completed = _run(fixture)

    assert completed.returncode == 1
    assert "job 'checks' was unexpectedly skipped" in completed.stderr


def test_success_fails_an_inapplicable_job() -> None:
    fixture = AggregateFixture(
        plan=_plan(
            {
                "checks": {"applicable": False, "reason": "paths unchanged"},
            },
        ),
        results=("checks=success",),
    )

    completed = _run(fixture)

    assert completed.returncode == 1
    assert "inapplicable and must conclude 'skipped', got 'success'" in completed.stderr


def test_missing_and_extra_results_are_reported_deterministically() -> None:
    fixture = AggregateFixture(
        plan=_plan(
            {
                "rust": {"applicable": True, "reason": "Rust source changed"},
                "web": {"applicable": False, "reason": "Web paths unchanged"},
            },
            selected_regions=("rust",),
        ),
        results=("unplanned=success", "web=skipped"),
    )

    completed = _run(fixture)

    assert completed.returncode == 1
    assert completed.stderr == (
        "ci-aggregate: contract violation: missing result for planned job 'rust'; "
        "expected 'success'\n"
        "ci-aggregate: contract violation: extra result for unplanned job "
        "'unplanned': 'success'\n"
    )
    document = _document(completed)
    assert document["extra_results"] == [{"name": "unplanned", "result": "success"}]


@pytest.mark.parametrize("plan_status", ["failure", "cancelled", "timed_out"])
def test_plan_failure_always_fails_the_aggregate(plan_status: str) -> None:
    fixture = AggregateFixture(
        plan=_plan({}, plan_status=plan_status),
        results=(),
    )

    completed = _run(fixture)

    assert completed.returncode == 1
    assert (
        f"plan failed: expected plan_status 'success', got '{plan_status}'"
        in completed.stderr
    )


@pytest.mark.parametrize(
    ("mutation", "diagnostic"),
    [
        ("not_json", "plan is not valid JSON"),
        ("array", "plan must be a JSON object"),
        ("missing_top_level", "missing keys: jobs"),
        ("extra_top_level", "unexpected keys: other"),
        ("job_applicable", "applicable must be a boolean"),
        ("job_reason", "reason must be a non-empty string"),
        ("duplicate_top_level", "duplicate key 'plan_status'"),
    ],
)
def test_plan_schema_errors_exit_two(mutation: str, diagnostic: str) -> None:
    plan = _plan({"fast": {"applicable": True, "reason": "always required"}})
    if mutation == "not_json":
        source = "not-json"
    elif mutation == "array":
        source = "[]"
    elif mutation == "missing_top_level":
        source = '{"plan_status":"success"}'
    elif mutation == "extra_top_level":
        plan["other"] = True
        source = json.dumps(plan)
    elif mutation == "job_applicable":
        jobs = cast("dict[str, dict[str, object]]", plan["jobs"])
        jobs["fast"]["applicable"] = "true"
        source = json.dumps(plan)
    elif mutation == "job_reason":
        jobs = cast("dict[str, dict[str, object]]", plan["jobs"])
        jobs["fast"]["reason"] = ""
        source = json.dumps(plan)
    elif mutation == "duplicate_top_level":
        source = json.dumps(plan).replace(
            '"plan_status": "success"',
            '"plan_status": "success", "plan_status": "failure"',
            1,
        )
    else:
        raise AssertionError(mutation)

    completed = subprocess.run(  # noqa: S603 - fixed repository script
        [sys.executable, "-I", str(AGGREGATE), source],
        check=False,
        capture_output=True,
        text=True,
        timeout=5,
    )

    assert completed.returncode == 2
    assert completed.stdout == ""
    assert completed.stderr.startswith("ci-aggregate: input error: ")
    assert diagnostic in completed.stderr


@pytest.mark.parametrize(
    ("mutation", "diagnostic"),
    [
        ("regions_not_object", "plan.regions must be a JSON object"),
        ("regions_missing", "plan.regions has missing keys: docs"),
        ("regions_extra", "plan.regions has unexpected keys: other"),
        ("regions_string", "plan.regions['docs'] must be a boolean"),
        ("region_outputs_not_object", "plan.region_outputs must be a JSON object"),
        (
            "region_outputs_missing",
            "plan.region_outputs has missing keys: docs",
        ),
        (
            "region_outputs_extra",
            "plan.region_outputs has unexpected keys: other",
        ),
        (
            "region_outputs_boolean",
            "plan.region_outputs['docs'] must be exactly 'true' or 'false'",
        ),
        (
            "region_outputs_uppercase",
            "plan.region_outputs['docs'] must be exactly 'true' or 'false'",
        ),
        (
            "region_outputs_mismatch",
            "must equal the lowercase plan.regions['docs'] boolean 'false', got 'true'",
        ),
    ],
)
def test_region_plan_schema_errors_exit_two(mutation: str, diagnostic: str) -> None:
    plan = _plan({})
    if mutation == "regions_not_object":
        plan["regions"] = []
    elif mutation == "region_outputs_not_object":
        plan["region_outputs"] = []
    else:
        field = "regions" if mutation.startswith("regions_") else "region_outputs"
        values = cast("dict[str, object]", plan[field])
        if mutation.endswith("_missing"):
            del values["docs"]
        elif mutation.endswith("_extra"):
            values["other"] = False if field == "regions" else "false"
        elif mutation == "regions_string":
            values["docs"] = "false"
        elif mutation == "region_outputs_boolean":
            values["docs"] = False
        elif mutation == "region_outputs_uppercase":
            values["docs"] = "False"
        elif mutation == "region_outputs_mismatch":
            values["docs"] = "true"
        else:
            raise AssertionError(mutation)

    completed = _run(AggregateFixture(plan=plan, results=()))

    assert completed.returncode == 2
    assert completed.stdout == ""
    assert completed.stderr.startswith("ci-aggregate: input error: ")
    assert diagnostic in completed.stderr


@pytest.mark.parametrize("field", ["regions", "region_outputs"])
def test_duplicate_region_keys_exit_two(field: str) -> None:
    source = json.dumps(_plan({}), separators=(",", ":"))
    target = '"docs":false' if field == "regions" else '"docs":"false"'
    source = source.replace(target, f"{target},{target}", 1)

    completed = subprocess.run(  # noqa: S603 - fixed repository script
        [sys.executable, "-I", str(AGGREGATE), source],
        check=False,
        capture_output=True,
        text=True,
        timeout=5,
    )

    assert completed.returncode == 2
    assert completed.stdout == ""
    assert (
        completed.stderr
        == "ci-aggregate: input error: JSON contains duplicate key 'docs'\n"
    )


@pytest.mark.parametrize(
    ("results", "diagnostic"),
    [
        (("fast",), "must have the form NAME=CONCLUSION"),
        (("fast=unknown",), "unsupported result 'unknown'"),
        (("fast=success", "fast=success"), "provided more than once"),
        (("invalid name=success",), "is not a valid job name"),
    ],
)
def test_result_schema_errors_exit_two(
    results: tuple[str, ...], diagnostic: str
) -> None:
    fixture = AggregateFixture(
        plan=_plan(
            {
                "fast": {"applicable": True, "reason": "always required"},
            },
        ),
        results=results,
    )

    completed = _run(fixture)

    assert completed.returncode == 2
    assert completed.stdout == ""
    assert diagnostic in completed.stderr


def test_public_evaluator_preserves_typed_contract() -> None:
    plan_source = json.dumps(
        _plan(
            {"docs": {"applicable": False, "reason": "unchanged"}},
            selected_regions=("docs",),
        ),
        separators=(",", ":"),
    )
    plan = parse_plan(plan_source)
    results = parse_results(("docs=skipped",))

    report = evaluate(plan, results)

    assert plan.plan_status is Result.SUCCESS
    assert plan.regions == {
        "docs": True,
        "contracts": False,
        "rust": False,
        "python": False,
        "routing": False,
        "web": False,
        "product": False,
        "remote_flake": False,
    }
    assert plan.region_outputs == {
        "docs": "true",
        "contracts": "false",
        "rust": "false",
        "python": "false",
        "routing": "false",
        "web": "false",
        "product": "false",
        "remote_flake": "false",
    }
    assert results == {"docs": Result.SKIPPED}
    assert report.conclusion is Result.SUCCESS
    assert report.regions == plan.regions
    assert report.region_outputs == plan.region_outputs
    assert report.violations == ()
