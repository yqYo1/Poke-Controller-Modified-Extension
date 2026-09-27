"""Package CI aggregate/gating pin (AR-10.10-07).

Pins the ``package.yml`` required-gate contract against the real workflow
with the standard library only (no PyYAML): the exact job id set, the
``Package CI Required`` check identity and needs order, the six-entry
aggregate map (every expression gates on ``product`` only with reason
``product paths changed``), the bijection between aggregate keys and the
``NAME=${{ needs.NAME.result }}`` result arguments, the per-job ``if:``
gates (independently re-derived with local regexes), and the eight plan
region outputs.

Also drives ``scripts/ci/aggregate.py`` as a subprocess with
package-shaped ``PLAN_JSON`` fixtures mirroring the workflow map:
product-irrelevant (all six skipped, exit 0), product-relevant (all six
success, exit 0), and relevant-with-one-failure (exit 1 naming the job).
Mutated inputs prove the aggregate fails closed: a dropped result, an
unknown planned job, and an applicable-but-skipped job are contract
violations (exit 1), while a malformed result argument is a schema
error (exit 2).
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import cast

REPOSITORY = Path(__file__).resolve().parents[2]
WORKFLOW = REPOSITORY / ".github/workflows/package.yml"
AGGREGATE = REPOSITORY / "scripts/ci/aggregate.py"

EXPECTED_JOB_IDS = (
    "plan",
    "linux",
    "linux_repro",
    "repro_check",
    "windows",
    "windows_repro",
    "windows_repro_check",
    "required",
)

EXPECTED_NEEDS = (
    "plan",
    "linux",
    "linux_repro",
    "repro_check",
    "windows",
    "windows_repro",
    "windows_repro_check",
)

EXPECTED_AGGREGATE_JOBS = (
    "linux",
    "linux_repro",
    "repro_check",
    "windows",
    "windows_repro",
    "windows_repro_check",
)

EXPECTED_CHECK_NAME = "Package CI Required"
EXPECTED_GATE_CONDITION = "needs.plan.outputs.product == 'true'"
EXPECTED_GATE_EXPRESSION = "${{ needs.plan.outputs.product == 'true' }}"
EXPECTED_REASON = "product paths changed"

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

_JOB_HEADER_RE = re.compile(r"(?m)^  ([A-Za-z0-9_-]+):\s*$")
_JOB_IF_RE = re.compile(r"(?m)^    if: (.*)$")
_PLAN_OUTPUT_RE = re.compile(r"(?m)^      ([A-Za-z0-9_]+): \$\{\{ steps\.regions\.")
_OUTPUT_REF_RE = re.compile(r"needs\.plan\.outputs\.([A-Za-z0-9_]+)")
_AGGREGATE_ENTRY_RE = re.compile(
    r'"([A-Za-z0-9_-]+)":\{"applicable":(\$\{\{.*?\}\}),"reason":"([^"]*)"\}'
)
_RESULT_ARG_RE = re.compile(
    r"(?m)^\s*([A-Za-z0-9_-]+)=\$\{\{\s*needs\.([A-Za-z0-9_-]+)\.result\s*\}\}"
)
_NEEDS_ENTRY_RE = re.compile(r"(?m)^      - (\S+)\s*$")
_REQUIRED_NAME_RE = re.compile(r"(?m)^    name: (.+?)\s*$")


def _jobs_body(text: str) -> str:
    return text.split("\njo" + "bs:\n", 1)[1]


def _job_blocks(text: str) -> dict[str, str]:
    body = _jobs_body(text)
    headers = list(_JOB_HEADER_RE.finditer(body))
    assert headers, "package.yml has no parsable job headers"
    blocks: dict[str, str] = {}
    for index, header in enumerate(headers):
        end = headers[index + 1].start() if index + 1 < len(headers) else len(body)
        blocks[header.group(1)] = body[header.end() : end]
    return blocks


def _rederive_job_conditions(text: str) -> dict[str, str | None]:
    """Re-derive each job-level if: without trusting a shared helper."""
    conditions: dict[str, str | None] = {}
    for job_id, block in _job_blocks(text).items():
        match = _JOB_IF_RE.search(block)
        if match is None:
            conditions[job_id] = None
            continue
        first = match.group(1).strip()
        if first not in (">", ">-", "|", "|-"):
            conditions[job_id] = first
            continue
        rest = block[match.end() :].splitlines()[1:]
        continued = [line.strip() for line in rest if line.strip()][:1]
        conditions[job_id] = " ".join(continued)
    return conditions


def _workflow_text() -> str:
    return WORKFLOW.read_text(encoding="utf-8")


def test_job_id_set_is_exact() -> None:
    blocks = _job_blocks(_workflow_text())
    assert sorted(blocks) == sorted(EXPECTED_JOB_IDS)
    body = _jobs_body(_workflow_text())
    ordered = [match.group(1) for match in _JOB_HEADER_RE.finditer(body)]
    assert ordered == list(EXPECTED_JOB_IDS)


def test_required_check_identity_and_needs_order() -> None:
    required = _job_blocks(_workflow_text())["required"]
    name_match = _REQUIRED_NAME_RE.search(required)
    assert name_match is not None, "required job has no parsable name:"
    assert name_match.group(1) == EXPECTED_CHECK_NAME
    assert _NEEDS_ENTRY_RE.findall(required) == list(EXPECTED_NEEDS)


def test_aggregate_map_pins_product_gate() -> None:
    required = _job_blocks(_workflow_text())["required"]
    entries = _AGGREGATE_ENTRY_RE.findall(required)
    assert [name for name, _, _ in entries] == list(EXPECTED_AGGREGATE_JOBS)
    for name, expression, reason in entries:
        refs = _OUTPUT_REF_RE.findall(expression)
        assert refs == ["product"], name
        assert expression == EXPECTED_GATE_EXPRESSION, name
        assert reason == EXPECTED_REASON, name


def test_result_arguments_match_aggregate_keys() -> None:
    required = _job_blocks(_workflow_text())["required"]
    arguments = _RESULT_ARG_RE.findall(required)
    assert sorted(name for name, _ in arguments) == sorted(EXPECTED_AGGREGATE_JOBS)
    for name, needs_ref in arguments:
        assert needs_ref == name, name


def test_gated_jobs_reference_product_only() -> None:
    conditions = _rederive_job_conditions(_workflow_text())
    assert sorted(conditions) == sorted(EXPECTED_JOB_IDS)
    for job_id in EXPECTED_AGGREGATE_JOBS:
        condition = conditions[job_id]
        assert condition is not None, job_id
        assert _OUTPUT_REF_RE.findall(condition) == ["product"], job_id
        assert condition.strip() == EXPECTED_GATE_CONDITION, job_id


def test_plan_outputs_contain_all_regions() -> None:
    outputs = _PLAN_OUTPUT_RE.findall(_workflow_text())
    assert outputs, "plan outputs block is unparsable"
    assert set(REGION_NAMES) <= set(outputs)
    assert "regions_json" in outputs


@dataclass(frozen=True, slots=True)
class AggregateFixture:
    plan: dict[str, object]
    results: tuple[str, ...]


def _package_plan(*, product: bool) -> dict[str, object]:
    regions = {name: (name == "product" and product) for name in REGION_NAMES}
    return {
        "plan_status": "success",
        "regions": regions,
        "region_outputs": {
            name: str(selected).lower() for name, selected in regions.items()
        },
        "jobs": {
            name: {"applicable": product, "reason": EXPECTED_REASON}
            for name in EXPECTED_AGGREGATE_JOBS
        },
    }


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


def test_irrelevant_product_false_accepts_all_skips() -> None:
    fixture = AggregateFixture(
        plan=_package_plan(product=False),
        results=tuple(f"{name}=skipped" for name in EXPECTED_AGGREGATE_JOBS),
    )

    completed = _run(fixture)

    assert completed.returncode == 0
    assert completed.stderr == ""
    document = _document(completed)
    assert document["conclusion"] == "success"
    assert document["violations"] == []
    jobs = cast("list[dict[str, object]]", document["jobs"])
    assert sorted(str(job["name"]) for job in jobs) == sorted(EXPECTED_AGGREGATE_JOBS)
    for job in jobs:
        assert job["applicable"] is False
        assert job["expected_result"] == "skipped"
        assert job["result"] == "skipped"
        assert job["reason"] == EXPECTED_REASON


def test_relevant_product_true_accepts_all_success() -> None:
    fixture = AggregateFixture(
        plan=_package_plan(product=True),
        results=tuple(f"{name}=success" for name in EXPECTED_AGGREGATE_JOBS),
    )

    completed = _run(fixture)

    assert completed.returncode == 0
    assert completed.stderr == ""
    document = _document(completed)
    assert document["conclusion"] == "success"
    assert document["violations"] == []
    assert document["region_outputs"] == {
        **{name: "false" for name in REGION_NAMES if name != "product"},
        "product": "true",
    }
    jobs = cast("list[dict[str, object]]", document["jobs"])
    assert sorted(str(job["name"]) for job in jobs) == sorted(EXPECTED_AGGREGATE_JOBS)
    for job in jobs:
        assert job["applicable"] is True
        assert job["expected_result"] == "success"
        assert job["result"] == "success"


def test_relevant_single_failure_names_the_job() -> None:
    results = tuple(
        f"{name}={'failure' if name == 'windows_repro_check' else 'success'}"
        for name in EXPECTED_AGGREGATE_JOBS
    )
    completed = _run(
        AggregateFixture(plan=_package_plan(product=True), results=results)
    )

    assert completed.returncode == 1
    assert completed.stderr == (
        "ci-aggregate: contract violation: job 'windows_repro_check' is "
        "applicable and must conclude 'success', got 'failure'\n"
    )
    document = _document(completed)
    assert document["conclusion"] == "failure"
    assert document["violations"] == [
        "job 'windows_repro_check' is applicable and must conclude "
        "'success', got 'failure'"
    ]


def test_dropped_result_argument_fails_closed() -> None:
    results = tuple(
        f"{name}=success" for name in EXPECTED_AGGREGATE_JOBS if name != "linux"
    )
    completed = _run(
        AggregateFixture(plan=_package_plan(product=True), results=results)
    )

    assert completed.returncode == 1
    assert "missing result for planned job 'linux'" in completed.stderr
    assert _document(completed)["conclusion"] == "failure"


def test_unknown_job_in_plan_map_fails_closed() -> None:
    plan = _package_plan(product=False)
    jobs = cast("dict[str, object]", plan["jobs"])
    jobs["coffee"] = {"applicable": False, "reason": "unknown job"}
    results = tuple(f"{name}=skipped" for name in EXPECTED_AGGREGATE_JOBS)
    completed = _run(AggregateFixture(plan=plan, results=results))

    assert completed.returncode == 1
    assert "missing result for planned job 'coffee'" in completed.stderr
    assert _document(completed)["conclusion"] == "failure"


def test_applicable_job_passed_skipped_fails_closed() -> None:
    results = tuple(
        f"{name}={'skipped' if name == 'linux' else 'success'}"
        for name in EXPECTED_AGGREGATE_JOBS
    )
    completed = _run(
        AggregateFixture(plan=_package_plan(product=True), results=results)
    )

    assert completed.returncode == 1
    assert "job 'linux' was unexpectedly skipped" in completed.stderr
    assert _document(completed)["conclusion"] == "failure"


def test_malformed_result_argument_is_schema_error() -> None:
    completed = _run(
        AggregateFixture(plan=_package_plan(product=True), results=("linux",))
    )

    assert completed.returncode == 2
    assert completed.stdout == ""
    assert "must have the form NAME=CONCLUSION" in completed.stderr
