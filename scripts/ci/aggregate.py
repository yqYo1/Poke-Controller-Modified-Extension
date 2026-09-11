"""Evaluate planned CI job applicability against GitHub job results."""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from enum import StrEnum
from typing import Final, cast


class Result(StrEnum):
    """GitHub-style conclusions accepted at the aggregate boundary."""

    SUCCESS = "success"
    SKIPPED = "skipped"
    FAILURE = "failure"
    CANCELLED = "cancelled"
    TIMED_OUT = "timed_out"


class InputError(ValueError):
    """The aggregate input does not conform to its closed schema."""


@dataclass(frozen=True, slots=True)
class PlannedJob:
    """Applicability decision made by the planning job."""

    applicable: bool
    reason: str

    @property
    def expected_result(self) -> Result:
        """Return the only result permitted for this applicability decision."""
        return Result.SUCCESS if self.applicable else Result.SKIPPED


@dataclass(frozen=True, slots=True)
class Plan:
    """Validated planning-job output."""

    plan_status: Result
    regions: dict[str, bool]
    region_outputs: dict[str, str]
    jobs: dict[str, PlannedJob]


@dataclass(frozen=True, slots=True)
class JobReport:
    """Expected and observed state for one planned job."""

    name: str
    applicable: bool
    reason: str
    expected_result: Result
    result: Result | None

    def to_document(self) -> dict[str, object]:
        """Return this row in the stable report schema."""
        return {
            "applicable": self.applicable,
            "expected_result": self.expected_result.value,
            "name": self.name,
            "reason": self.reason,
            "result": None if self.result is None else self.result.value,
        }


@dataclass(frozen=True, slots=True)
class AggregateReport:
    """Deterministic aggregate conclusion and its complete evidence."""

    plan_status: Result
    regions: dict[str, bool]
    region_outputs: dict[str, str]
    jobs: tuple[JobReport, ...]
    extra_results: tuple[tuple[str, Result], ...]
    violations: tuple[str, ...]

    @property
    def conclusion(self) -> Result:
        """Translate contract validity into the required gate conclusion."""
        return Result.FAILURE if self.violations else Result.SUCCESS

    def to_document(self) -> dict[str, object]:
        """Return the stable JSON report schema."""
        return {
            "conclusion": self.conclusion.value,
            "extra_results": [
                {"name": name, "result": result.value}
                for name, result in self.extra_results
            ],
            "jobs": [job.to_document() for job in self.jobs],
            "plan_status": self.plan_status.value,
            "region_outputs": self.region_outputs,
            "regions": self.regions,
            "violations": list(self.violations),
        }

    def to_json(self) -> str:
        """Serialize the report canonically for fixtures and audit evidence."""
        return json.dumps(
            self.to_document(),
            ensure_ascii=False,
            separators=(",", ":"),
            sort_keys=True,
        )


type ResultMap = Mapping[str, Result]

REGION_NAMES: Final = (
    "docs",
    "contracts",
    "rust",
    "python",
    "routing",
    "web",
    "product",
    "remote_flake",
)
REGION_KEYS: Final = frozenset(REGION_NAMES)
PLAN_KEYS: Final = frozenset({"jobs", "plan_status", "region_outputs", "regions"})
JOB_KEYS: Final = frozenset({"applicable", "reason"})
JOB_NAME_PATTERN: Final = re.compile(r"[A-Za-z_][A-Za-z0-9_-]*\Z")
VALID_RESULTS: Final = ", ".join(result.value for result in Result)


def _object_without_duplicate_keys(
    pairs: list[tuple[str, object]],
) -> dict[str, object]:
    document: dict[str, object] = {}
    for key, value in pairs:
        if key in document:
            message = f"JSON contains duplicate key {key!r}"
            raise InputError(message)
        document[key] = value
    return document


def _require_object(value: object, context: str) -> dict[str, object]:
    if not isinstance(value, dict):
        message = f"{context} must be a JSON object"
        raise InputError(message)
    return cast("dict[str, object]", value)


def _require_exact_keys(
    document: Mapping[str, object], expected: frozenset[str], context: str
) -> None:
    actual = frozenset(document)
    missing = sorted(expected - actual)
    extra = sorted(actual - expected)
    diagnostics: list[str] = []
    if missing:
        diagnostics.append(f"missing keys: {', '.join(missing)}")
    if extra:
        diagnostics.append(f"unexpected keys: {', '.join(extra)}")
    if diagnostics:
        message = f"{context} has {'; '.join(diagnostics)}"
        raise InputError(message)


def _require_job_name(name: str, context: str) -> None:
    if JOB_NAME_PATTERN.fullmatch(name) is None:
        message = (
            f"{context} {name!r} is not a valid job name "
            "(expected [A-Za-z_][A-Za-z0-9_-]*)"
        )
        raise InputError(message)


def _parse_result(value: object, context: str) -> Result:
    if not isinstance(value, str):
        message = f"{context} must be a string"
        raise InputError(message)
    try:
        return Result(value)
    except ValueError as error:
        message = (
            f"{context} has unsupported result {value!r}; expected one of: "
            f"{VALID_RESULTS}"
        )
        raise InputError(message) from error


def _parse_regions(value: object) -> dict[str, bool]:
    context = "plan.regions"
    document = _require_object(value, context)
    _require_exact_keys(document, REGION_KEYS, context)
    regions: dict[str, bool] = {}
    for name in REGION_NAMES:
        selected = document[name]
        if not isinstance(selected, bool):
            message = f"{context}[{name!r}] must be a boolean"
            raise InputError(message)
        regions[name] = selected
    return regions


def _parse_region_outputs(value: object) -> dict[str, str]:
    context = "plan.region_outputs"
    document = _require_object(value, context)
    _require_exact_keys(document, REGION_KEYS, context)
    outputs: dict[str, str] = {}
    for name in REGION_NAMES:
        output = document[name]
        if not isinstance(output, str) or output not in {"true", "false"}:
            message = f"{context}[{name!r}] must be exactly 'true' or 'false'"
            raise InputError(message)
        outputs[name] = output
    return outputs


def _require_matching_region_outputs(
    regions: Mapping[str, bool], region_outputs: Mapping[str, str]
) -> None:
    for name in REGION_NAMES:
        expected = str(regions[name]).lower()
        actual = region_outputs[name]
        if actual != expected:
            message = (
                f"plan.region_outputs[{name!r}] must equal the lowercase "
                f"plan.regions[{name!r}] boolean {expected!r}, got {actual!r}"
            )
            raise InputError(message)


def parse_plan(source: str) -> Plan:
    """Parse and validate the closed JSON planning schema."""
    try:
        raw_document = json.loads(
            source,
            object_pairs_hook=_object_without_duplicate_keys,
        )
    except json.JSONDecodeError as error:
        message = (
            f"plan is not valid JSON at line {error.lineno}, column {error.colno}: "
            f"{error.msg}"
        )
        raise InputError(message) from error

    document = _require_object(cast("object", raw_document), "plan")
    _require_exact_keys(document, PLAN_KEYS, "plan")
    plan_status = _parse_result(document["plan_status"], "plan.plan_status")
    regions = _parse_regions(document["regions"])
    region_outputs = _parse_region_outputs(document["region_outputs"])
    _require_matching_region_outputs(regions, region_outputs)
    raw_jobs = _require_object(document["jobs"], "plan.jobs")

    jobs: dict[str, PlannedJob] = {}
    for name in sorted(raw_jobs):
        _require_job_name(name, "plan job")
        raw_job = _require_object(raw_jobs[name], f"plan.jobs[{name!r}]")
        _require_exact_keys(raw_job, JOB_KEYS, f"plan.jobs[{name!r}]")

        applicable = raw_job["applicable"]
        if not isinstance(applicable, bool):
            message = f"plan.jobs[{name!r}].applicable must be a boolean"
            raise InputError(message)
        reason = raw_job["reason"]
        if not isinstance(reason, str) or not reason.strip():
            message = f"plan.jobs[{name!r}].reason must be a non-empty string"
            raise InputError(message)
        jobs[name] = PlannedJob(applicable=applicable, reason=reason)

    return Plan(
        plan_status=plan_status,
        regions=regions,
        region_outputs=region_outputs,
        jobs=jobs,
    )


def parse_results(arguments: Sequence[str]) -> dict[str, Result]:
    """Parse repeated ``NAME=CONCLUSION`` arguments without ambiguity."""
    results: dict[str, Result] = {}
    for position, argument in enumerate(arguments, start=1):
        name, separator, raw_result = argument.partition("=")
        if not separator:
            message = f"result argument {position} must have the form NAME=CONCLUSION"
            raise InputError(message)
        _require_job_name(name, f"result argument {position} job name")
        if name in results:
            message = f"result for job {name!r} was provided more than once"
            raise InputError(message)
        results[name] = _parse_result(
            raw_result,
            f"result for job {name!r}",
        )
    return results


def evaluate(plan: Plan, results: ResultMap) -> AggregateReport:
    """Require every planned job to have exactly its applicability result."""
    violations: list[str] = []
    if plan.plan_status is not Result.SUCCESS:
        violations.append(
            "plan failed: expected plan_status 'success', "
            f"got {plan.plan_status.value!r}"
        )

    jobs: list[JobReport] = []
    for name in sorted(plan.jobs):
        planned = plan.jobs[name]
        actual = results.get(name)
        expected = planned.expected_result
        jobs.append(
            JobReport(
                name=name,
                applicable=planned.applicable,
                reason=planned.reason,
                expected_result=expected,
                result=actual,
            )
        )
        if actual is None:
            violations.append(
                f"missing result for planned job {name!r}; expected {expected.value!r}"
            )
        elif actual is not expected:
            if planned.applicable and actual is Result.SKIPPED:
                violations.append(
                    f"job {name!r} was unexpectedly skipped; applicable jobs "
                    "must conclude 'success'"
                )
            else:
                applicability = "applicable" if planned.applicable else "inapplicable"
                violations.append(
                    f"job {name!r} is {applicability} and must conclude "
                    f"{expected.value!r}, got {actual.value!r}"
                )

    extra_results = tuple(
        (name, results[name]) for name in sorted(results.keys() - plan.jobs.keys())
    )
    violations.extend(
        f"extra result for unplanned job {name!r}: {result.value!r}"
        for name, result in extra_results
    )
    return AggregateReport(
        plan_status=plan.plan_status,
        regions=plan.regions,
        region_outputs=plan.region_outputs,
        jobs=tuple(jobs),
        extra_results=extra_results,
        violations=tuple(violations),
    )


def _parse_args(argv: Sequence[str] | None) -> tuple[str, tuple[str, ...]]:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "plan_json",
        metavar="PLAN_JSON",
        help=(
            "JSON object with plan status, eight region decisions, scalar region "
            "outputs, and job applicability"
        ),
    )
    parser.add_argument(
        "results",
        metavar="NAME=CONCLUSION",
        nargs="*",
        help="actual GitHub result for a planned job",
    )
    arguments = parser.parse_args(argv)
    return cast("str", arguments.plan_json), tuple(cast("list[str]", arguments.results))


def main(argv: Sequence[str] | None = None) -> int:
    """Evaluate CLI input, emit canonical evidence, and return a stable status."""
    plan_json, result_arguments = _parse_args(argv)
    try:
        plan = parse_plan(plan_json)
        results = parse_results(result_arguments)
    except InputError as error:
        print(f"ci-aggregate: input error: {error}", file=sys.stderr)
        return 2

    report = evaluate(plan, results)
    print(report.to_json())
    for violation in report.violations:
        print(f"ci-aggregate: contract violation: {violation}", file=sys.stderr)
    return 1 if report.violations else 0


if __name__ == "__main__":
    raise SystemExit(main())
