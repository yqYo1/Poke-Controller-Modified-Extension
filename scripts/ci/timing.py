"""Validate and evaluate deterministic CI timing and cache evidence."""

from __future__ import annotations

import argparse
import json
import math
import re
import sys
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from enum import StrEnum
from typing import Final, Never, cast


class InputError(ValueError):
    """Input JSON or CLI arguments do not conform to the closed schema."""


class ChangeKind(StrEnum):
    """CI timing classes with explicit completion targets."""

    FAST = "fast"
    DOCS = "docs"
    PRODUCT = "product"


class Conclusion(StrEnum):
    """GitHub-style job conclusions recorded by timing evidence."""

    SUCCESS = "success"
    SKIPPED = "skipped"
    FAILURE = "failure"
    CANCELLED = "cancelled"
    TIMED_OUT = "timed_out"


@dataclass(frozen=True, slots=True)
class StepTiming:
    """Wall-clock evidence for one workflow step."""

    name: str
    wall_seconds: float

    def to_document(self) -> dict[str, object]:
        return {"name": self.name, "wall_seconds": self.wall_seconds}


@dataclass(frozen=True, slots=True)
class JobTiming:
    """Wall-clock and Nix evidence for one workflow job."""

    name: str
    conclusion: Conclusion
    wall_seconds: float
    steps: tuple[StepTiming, ...]
    built_derivations: tuple[str, ...]
    substituted_store_paths: tuple[str, ...]

    def to_document(self) -> dict[str, object]:
        return {
            "built_derivations": list(self.built_derivations),
            "conclusion": self.conclusion.value,
            "name": self.name,
            "steps": [step.to_document() for step in self.steps],
            "substituted_store_paths": list(self.substituted_store_paths),
            "wall_seconds": self.wall_seconds,
        }


@dataclass(frozen=True, slots=True)
class WorkflowTiming:
    """Wall-clock evidence for the whole GitHub workflow."""

    name: str
    wall_seconds: float

    def to_document(self) -> dict[str, object]:
        return {"name": self.name, "wall_seconds": self.wall_seconds}


@dataclass(frozen=True, slots=True)
class CacheAccess:
    """Cache permissions and the GitHub principal that exercised them."""

    read: bool
    write: bool
    actor: str
    event: str

    def to_document(self) -> dict[str, object]:
        return {
            "actor": self.actor,
            "event": self.event,
            "read": self.read,
            "write": self.write,
        }


@dataclass(frozen=True, slots=True)
class TimingReport:
    """Validated, canonical evidence from one workflow attempt."""

    sha: str
    attempt: int
    change_kind: ChangeKind
    regions: tuple[str, ...]
    measured_wall_seconds: float
    workflow: WorkflowTiming
    jobs: tuple[JobTiming, ...]
    cache: CacheAccess

    @property
    def built_derivation_count(self) -> int:
        return sum(len(job.built_derivations) for job in self.jobs)

    @property
    def substituted_store_paths(self) -> tuple[str, ...]:
        return tuple(
            sorted({path for job in self.jobs for path in job.substituted_store_paths})
        )

    @property
    def substituted_pokecon_paths(self) -> tuple[str, ...]:
        return tuple(
            path for path in self.substituted_store_paths if _is_pokecon_path(path)
        )

    def to_document(self) -> dict[str, object]:
        return {
            "attempt": self.attempt,
            "cache": self.cache.to_document(),
            "change_kind": self.change_kind.value,
            "jobs": [job.to_document() for job in self.jobs],
            "measured_wall_seconds": self.measured_wall_seconds,
            "regions": list(self.regions),
            "schema_version": SCHEMA_VERSION,
            "sha": self.sha,
            "workflow": self.workflow.to_document(),
        }


type JsonDocument = dict[str, object]
type ResultDocument = dict[str, object]
type ReportSequence = Sequence[TimingReport]

SCHEMA_VERSION: Final = 1
REPORT_KEYS: Final = frozenset(
    {
        "attempt",
        "cache",
        "change_kind",
        "jobs",
        "measured_wall_seconds",
        "regions",
        "schema_version",
        "sha",
        "workflow",
    }
)
WORKFLOW_KEYS: Final = frozenset({"name", "wall_seconds"})
JOB_KEYS: Final = frozenset(
    {
        "built_derivations",
        "conclusion",
        "name",
        "steps",
        "substituted_store_paths",
        "wall_seconds",
    }
)
STEP_KEYS: Final = frozenset({"name", "wall_seconds"})
CACHE_KEYS: Final = frozenset({"actor", "event", "read", "write"})
THRESHOLDS: Final = {
    ChangeKind.FAST: 180.0,
    ChangeKind.DOCS: 300.0,
    ChangeKind.PRODUCT: 600.0,
}
MINIMUM_P95_SAMPLES: Final = 10
SHA_PATTERN: Final = re.compile(r"[0-9a-fA-F]{40}\Z")
REGION_PATTERN: Final = re.compile(r"[a-z][a-z0-9_-]*\Z")
EVENT_PATTERN: Final = re.compile(r"[a-z][a-z0-9_]*\Z")
STORE_PATH_PATTERN: Final = re.compile(r"/nix/store/[0-9a-z]{32}-(?P<name>[^/\s]+)\Z")
POKECON_STORE_NAME_PATTERN: Final = re.compile(r"pokecon(?:[.-]|\Z)")


def _is_pokecon_path(path: str) -> bool:
    match = STORE_PATH_PATTERN.fullmatch(path)
    return (
        match is not None
        and POKECON_STORE_NAME_PATTERN.match(match.group("name")) is not None
    )


def _object_without_duplicate_keys(
    pairs: list[tuple[str, object]],
) -> JsonDocument:
    document: JsonDocument = {}
    for key, value in pairs:
        if key in document:
            message = f"JSON contains duplicate key {key!r}"
            raise InputError(message)
        document[key] = value
    return document


def _reject_json_constant(value: str) -> Never:
    message = f"JSON contains non-finite number {value!r}"
    raise InputError(message)


def _require_object(value: object, context: str) -> JsonDocument:
    if not isinstance(value, dict):
        message = f"{context} must be a JSON object"
        raise InputError(message)
    return cast("JsonDocument", value)


def _require_array(value: object, context: str) -> list[object]:
    if not isinstance(value, list):
        message = f"{context} must be a JSON array"
        raise InputError(message)
    return cast("list[object]", value)


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


def _require_string(value: object, context: str) -> str:
    if not isinstance(value, str) or not value.strip():
        message = f"{context} must be a non-empty string"
        raise InputError(message)
    return value


def _require_boolean(value: object, context: str) -> bool:
    if not isinstance(value, bool):
        message = f"{context} must be a boolean"
        raise InputError(message)
    return value


def _require_wall_seconds(value: object, context: str) -> float:
    if isinstance(value, bool) or not isinstance(value, int | float):
        message = f"{context} must be a non-negative finite number"
        raise InputError(message)
    wall_seconds = float(value)
    if not math.isfinite(wall_seconds) or wall_seconds < 0:
        message = f"{context} must be a non-negative finite number"
        raise InputError(message)
    return wall_seconds


def _require_string_array(
    value: object,
    context: str,
    *,
    pattern: re.Pattern[str] | None = None,
    require_sorted: bool = False,
) -> tuple[str, ...]:
    raw_values = _require_array(value, context)
    values: list[str] = []
    for index, raw_value in enumerate(raw_values):
        item = _require_string(raw_value, f"{context}[{index}]")
        if pattern is not None and pattern.fullmatch(item) is None:
            message = f"{context}[{index}] has invalid value {item!r}"
            raise InputError(message)
        values.append(item)
    if len(values) != len(set(values)):
        message = f"{context} must contain unique values"
        raise InputError(message)
    if require_sorted and values != sorted(values):
        message = f"{context} must be sorted"
        raise InputError(message)
    return tuple(values if require_sorted else sorted(values))


def _require_store_paths(
    value: object, context: str, *, derivations: bool
) -> tuple[str, ...]:
    paths = _require_string_array(value, context)
    for path in paths:
        if STORE_PATH_PATTERN.fullmatch(path) is None:
            message = f"{context} contains invalid Nix store path {path!r}"
            raise InputError(message)
        if derivations and not path.endswith(".drv"):
            message = f"{context} contains non-derivation path {path!r}"
            raise InputError(message)
    return paths


def _parse_step(value: object, context: str) -> StepTiming:
    document = _require_object(value, context)
    _require_exact_keys(document, STEP_KEYS, context)
    return StepTiming(
        name=_require_string(document["name"], f"{context}.name"),
        wall_seconds=_require_wall_seconds(
            document["wall_seconds"], f"{context}.wall_seconds"
        ),
    )


def _parse_conclusion(value: object, context: str) -> Conclusion:
    raw_conclusion = _require_string(value, context)
    try:
        return Conclusion(raw_conclusion)
    except ValueError as error:
        allowed = ", ".join(conclusion.value for conclusion in Conclusion)
        message = (
            f"{context} has unsupported conclusion {raw_conclusion!r}; "
            f"expected one of: {allowed}"
        )
        raise InputError(message) from error


def _parse_job(value: object, context: str) -> JobTiming:
    document = _require_object(value, context)
    _require_exact_keys(document, JOB_KEYS, context)
    raw_steps = _require_array(document["steps"], f"{context}.steps")
    steps = tuple(
        sorted(
            (
                _parse_step(raw_step, f"{context}.steps[{index}]")
                for index, raw_step in enumerate(raw_steps)
            ),
            key=lambda step: step.name,
        )
    )
    step_names = [step.name for step in steps]
    if len(step_names) != len(set(step_names)):
        message = f"{context}.steps must have unique names"
        raise InputError(message)
    return JobTiming(
        name=_require_string(document["name"], f"{context}.name"),
        conclusion=_parse_conclusion(document["conclusion"], f"{context}.conclusion"),
        wall_seconds=_require_wall_seconds(
            document["wall_seconds"], f"{context}.wall_seconds"
        ),
        steps=steps,
        built_derivations=_require_store_paths(
            document["built_derivations"],
            f"{context}.built_derivations",
            derivations=True,
        ),
        substituted_store_paths=_require_store_paths(
            document["substituted_store_paths"],
            f"{context}.substituted_store_paths",
            derivations=False,
        ),
    )


def _parse_workflow(value: object) -> WorkflowTiming:
    context = "report.workflow"
    document = _require_object(value, context)
    _require_exact_keys(document, WORKFLOW_KEYS, context)
    return WorkflowTiming(
        name=_require_string(document["name"], f"{context}.name"),
        wall_seconds=_require_wall_seconds(
            document["wall_seconds"], f"{context}.wall_seconds"
        ),
    )


def _parse_cache(value: object) -> CacheAccess:
    context = "report.cache"
    document = _require_object(value, context)
    _require_exact_keys(document, CACHE_KEYS, context)
    event = _require_string(document["event"], f"{context}.event")
    if EVENT_PATTERN.fullmatch(event) is None:
        message = f"{context}.event has invalid value {event!r}"
        raise InputError(message)
    return CacheAccess(
        read=_require_boolean(document["read"], f"{context}.read"),
        write=_require_boolean(document["write"], f"{context}.write"),
        actor=_require_string(document["actor"], f"{context}.actor"),
        event=event,
    )


def parse_report(source: str) -> TimingReport:
    """Parse one strict timing report and normalize unordered evidence arrays."""
    try:
        raw_document = json.loads(
            source,
            object_pairs_hook=_object_without_duplicate_keys,
            parse_constant=_reject_json_constant,
        )
    except json.JSONDecodeError as error:
        message = (
            f"report is not valid JSON at line {error.lineno}, column {error.colno}: "
            f"{error.msg}"
        )
        raise InputError(message) from error

    document = _require_object(cast("object", raw_document), "report")
    _require_exact_keys(document, REPORT_KEYS, "report")

    schema_version = document["schema_version"]
    if isinstance(schema_version, bool) or schema_version != SCHEMA_VERSION:
        message = f"report.schema_version must equal {SCHEMA_VERSION}"
        raise InputError(message)

    sha = _require_string(document["sha"], "report.sha")
    if SHA_PATTERN.fullmatch(sha) is None:
        message = "report.sha must contain exactly 40 hexadecimal characters"
        raise InputError(message)

    attempt = document["attempt"]
    if isinstance(attempt, bool) or not isinstance(attempt, int) or attempt < 1:
        message = "report.attempt must be an integer greater than or equal to 1"
        raise InputError(message)

    raw_change_kind = _require_string(document["change_kind"], "report.change_kind")
    try:
        change_kind = ChangeKind(raw_change_kind)
    except ValueError as error:
        allowed = ", ".join(kind.value for kind in ChangeKind)
        message = (
            f"report.change_kind has unsupported value {raw_change_kind!r}; "
            f"expected one of: {allowed}"
        )
        raise InputError(message) from error

    regions = _require_string_array(
        document["regions"],
        "report.regions",
        pattern=REGION_PATTERN,
        require_sorted=True,
    )
    raw_jobs = _require_array(document["jobs"], "report.jobs")
    jobs = tuple(
        sorted(
            (
                _parse_job(raw_job, f"report.jobs[{index}]")
                for index, raw_job in enumerate(raw_jobs)
            ),
            key=lambda job: job.name,
        )
    )
    job_names = [job.name for job in jobs]
    if len(job_names) != len(set(job_names)):
        message = "report.jobs must have unique names"
        raise InputError(message)

    return TimingReport(
        sha=sha.lower(),
        attempt=attempt,
        change_kind=change_kind,
        regions=regions,
        measured_wall_seconds=_require_wall_seconds(
            document["measured_wall_seconds"], "report.measured_wall_seconds"
        ),
        workflow=_parse_workflow(document["workflow"]),
        jobs=jobs,
        cache=_parse_cache(document["cache"]),
    )


def report_violations(report: TimingReport) -> tuple[str, ...]:
    """Return trust and evidence contradictions within one valid report."""
    violations: list[str] = []
    if report.cache.write and report.cache.event != "push":
        violations.append(
            "cache writes are permitted only for push events, "
            f"got event {report.cache.event!r}"
        )
    if report.substituted_store_paths and not report.cache.read:
        violations.append(
            "substituted store paths were recorded while cache.read is false"
        )
    return tuple(violations)


def _canonical_json(document: Mapping[str, object]) -> str:
    return json.dumps(
        document,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    )


def validation_document(report: TimingReport) -> ResultDocument:
    """Create the stable validation report for one timing attempt."""
    violations = report_violations(report)
    return {
        "command": "validate",
        "conclusion": "failure" if violations else "success",
        "report": report.to_document(),
        "violations": list(violations),
    }


def _report_identity(report: TimingReport) -> str:
    return f"{report.sha} attempt {report.attempt}"


def p95_document(reports: ReportSequence) -> ResultDocument:
    """Calculate nearest-rank p95 for at least ten reports of one kind."""
    ordered_reports = tuple(
        sorted(
            reports,
            key=lambda report: (
                report.sha,
                report.attempt,
                report.measured_wall_seconds,
            ),
        )
    )
    violations: list[str] = []
    for report in ordered_reports:
        violations.extend(
            f"{_report_identity(report)}: {violation}"
            for violation in report_violations(report)
        )

    kinds = sorted({report.change_kind.value for report in ordered_reports})
    same_kind = len(kinds) == 1
    if not same_kind:
        violations.append(
            f"p95 requires reports of exactly one change_kind; got {', '.join(kinds)}"
        )
    if len(ordered_reports) < MINIMUM_P95_SAMPLES:
        violations.append(
            f"p95 requires at least {MINIMUM_P95_SAMPLES} reports; "
            f"got {len(ordered_reports)}"
        )

    change_kind = ordered_reports[0].change_kind if same_kind else None
    measured_values = sorted(report.measured_wall_seconds for report in ordered_reports)
    nearest_rank = math.ceil(0.95 * len(measured_values))
    p95_wall_seconds = measured_values[nearest_rank - 1]
    threshold_seconds = None if change_kind is None else THRESHOLDS[change_kind]
    if (
        change_kind is not None
        and threshold_seconds is not None
        and p95_wall_seconds > threshold_seconds
    ):
        violations.append(
            f"{change_kind.value} p95 {p95_wall_seconds} seconds exceeds "
            f"threshold {threshold_seconds} seconds"
        )

    return {
        "change_kind": None if change_kind is None else change_kind.value,
        "command": "p95",
        "conclusion": "failure" if violations else "success",
        "nearest_rank": nearest_rank,
        "p95_wall_seconds": p95_wall_seconds,
        "sample_count": len(ordered_reports),
        "samples": [
            {
                "attempt": report.attempt,
                "measured_wall_seconds": report.measured_wall_seconds,
                "sha": report.sha,
            }
            for report in ordered_reports
        ],
        "threshold_seconds": threshold_seconds,
        "violations": violations,
    }


def _comparison_attempt_document(report: TimingReport) -> ResultDocument:
    return {
        "attempt": report.attempt,
        "built_derivation_count": report.built_derivation_count,
        "cache": report.cache.to_document(),
        "measured_wall_seconds": report.measured_wall_seconds,
        "substituted_pokecon_paths": list(report.substituted_pokecon_paths),
        "substituted_store_path_count": len(report.substituted_store_paths),
    }


def comparison_document(first: TimingReport, second: TimingReport) -> ResultDocument:
    """Compare two attempts using concrete Nix and wall-clock evidence."""
    violations: list[str] = []
    for label, report in (("first", first), ("second", second)):
        violations.extend(
            f"{label} report: {violation}" for violation in report_violations(report)
        )

    same_sha = first.sha == second.sha
    same_change_kind = first.change_kind is second.change_kind
    second_attempt_is_later = second.attempt > first.attempt
    built_derivations_decreased = (
        second.built_derivation_count < first.built_derivation_count
    )
    wall_seconds_decreased = second.measured_wall_seconds < first.measured_wall_seconds
    if not same_sha:
        violations.append(
            f"compare requires the same SHA; got {first.sha!r} and {second.sha!r}"
        )
    if not same_change_kind:
        violations.append(
            "compare requires the same change_kind; "
            f"got {first.change_kind.value!r} and {second.change_kind.value!r}"
        )
    if not second_attempt_is_later:
        violations.append(
            "second attempt must be later than first attempt; "
            f"got {first.attempt} then {second.attempt}"
        )
    if not second.substituted_pokecon_paths:
        violations.append("second attempt has no substituted PokeCon store paths")
    if not built_derivations_decreased:
        violations.append(
            "second attempt must build fewer derivations; "
            f"got {first.built_derivation_count} then "
            f"{second.built_derivation_count}"
        )
    if not wall_seconds_decreased:
        violations.append(
            "second attempt must have lower measured wall time; "
            f"got {first.measured_wall_seconds} then "
            f"{second.measured_wall_seconds} seconds"
        )

    return {
        "built_derivations_decreased": built_derivations_decreased,
        "command": "compare",
        "conclusion": "failure" if violations else "success",
        "first": _comparison_attempt_document(first),
        "same_change_kind": same_change_kind,
        "same_sha": same_sha,
        "second": _comparison_attempt_document(second),
        "second_attempt_is_later": second_attempt_is_later,
        "sha": first.sha if same_sha else None,
        "substituted_pokecon_paths_present": bool(second.substituted_pokecon_paths),
        "violations": violations,
        "wall_seconds_decreased": wall_seconds_decreased,
    }


def _parse_args(argv: Sequence[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    validate_parser = subparsers.add_parser(
        "validate", help="validate one timing report"
    )
    validate_parser.add_argument("report", metavar="REPORT_JSON")

    p95_parser = subparsers.add_parser(
        "p95", help="calculate nearest-rank p95 for same-kind reports"
    )
    p95_parser.add_argument("reports", metavar="REPORT_JSON", nargs="+")

    compare_parser = subparsers.add_parser(
        "compare", help="compare two attempts of one SHA"
    )
    compare_parser.add_argument("first", metavar="FIRST_REPORT_JSON")
    compare_parser.add_argument("second", metavar="SECOND_REPORT_JSON")
    return parser.parse_args(argv)


def _emit(document: ResultDocument) -> int:
    print(_canonical_json(document))
    raw_violations = document["violations"]
    violations = cast("list[str]", raw_violations)
    for violation in violations:
        print(f"ci-timing: contract violation: {violation}", file=sys.stderr)
    return 1 if violations else 0


def main(argv: Sequence[str] | None = None) -> int:
    """Execute one evidence command with stable output and exit semantics."""
    arguments = _parse_args(argv)
    command = cast("str", arguments.command)
    try:
        if command == "validate":
            report = parse_report(cast("str", arguments.report))
            document = validation_document(report)
        elif command == "p95":
            report_sources = cast("list[str]", arguments.reports)
            reports = tuple(parse_report(source) for source in report_sources)
            document = p95_document(reports)
        elif command == "compare":
            first = parse_report(cast("str", arguments.first))
            second = parse_report(cast("str", arguments.second))
            document = comparison_document(first, second)
        else:
            raise AssertionError(command)
    except InputError as error:
        print(f"ci-timing: input error: {error}", file=sys.stderr)
        return 2
    return _emit(document)


if __name__ == "__main__":
    raise SystemExit(main())
