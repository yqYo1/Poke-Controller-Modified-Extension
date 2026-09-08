"""Validate and evaluate deterministic CI timing and cache evidence."""

from __future__ import annotations

import argparse
import datetime
import json
import math
import os
import re
import sys
import urllib.error
import urllib.request
from collections.abc import Callable, Mapping, Sequence
from dataclasses import dataclass
from enum import StrEnum
from pathlib import Path
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
class RunInfo:
    """Workflow run identity and completion state at collection time."""

    started_at: datetime.datetime
    status: str
    conclusion: str | None

    def to_document(self) -> dict[str, object]:
        return {
            "conclusion": self.conclusion,
            "started_at": self.started_at.isoformat().replace("+00:00", "Z"),
            "status": self.status,
        }


@dataclass(frozen=True, slots=True)
class CollectionInfo:
    """Explicit collection semantics for timing evidence."""

    kind: str

    def to_document(self) -> dict[str, object]:
        return {"kind": self.kind}


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
    run: RunInfo
    collection: CollectionInfo

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
            "collection": self.collection.to_document(),
            "jobs": [job.to_document() for job in self.jobs],
            "measured_wall_seconds": self.measured_wall_seconds,
            "regions": list(self.regions),
            "run": self.run.to_document(),
            "schema_version": SCHEMA_VERSION,
            "sha": self.sha,
            "workflow": self.workflow.to_document(),
        }

    @property
    def run_status(self) -> str:
        return self.run.status

    @property
    def collection_kind(self) -> str:
        return self.collection.kind

    @property
    def critical_path_wall_seconds(self) -> float:
        candidates = [
            job.wall_seconds
            for job in self.jobs
            if job.conclusion is not Conclusion.SKIPPED
        ]
        return max(candidates) if candidates else 0.0


type JsonDocument = dict[str, object]
type ResultDocument = dict[str, object]
type ReportSequence = Sequence[TimingReport]

SCHEMA_VERSION: Final = 2
REPORT_KEYS: Final = frozenset(
    {
        "attempt",
        "cache",
        "change_kind",
        "collection",
        "jobs",
        "measured_wall_seconds",
        "regions",
        "run",
        "schema_version",
        "sha",
        "workflow",
    }
)
WORKFLOW_KEYS: Final = frozenset({"name", "wall_seconds"})
RUN_KEYS: Final = frozenset({"conclusion", "started_at", "status"})
COLLECTION_KEYS: Final = frozenset({"kind"})
COLLECTION_KIND_UPSTREAM_COMPLETED_MAX: Final = "upstream_completed_max"
ALLOWED_COLLECTION_KINDS: Final = frozenset({COLLECTION_KIND_UPSTREAM_COMPLETED_MAX})
ALLOWED_RUN_STATUSES: Final = frozenset(
    {"queued", "in_progress", "waiting", "requested", "pending", "completed"}
)
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
    ChangeKind.PRODUCT: 720.0,
}
MINIMUM_P95_SAMPLES: Final = 10
MAX_HISTORY_AGE_DAYS: Final = 30
MAX_HISTORY_STALE_SECONDS: Final = MAX_HISTORY_AGE_DAYS * 24 * 60 * 60
TRUSTED_CACHE_WRITERS: Final = frozenset({"yqYo1"})
SHA_PATTERN: Final = re.compile(r"[0-9a-fA-F]{40}\Z")
REGION_PATTERN: Final = re.compile(r"[a-z][a-z0-9_-]*\Z")
EVENT_PATTERN: Final = re.compile(r"[a-z][a-z0-9_]*\Z")
STORE_PATH_PATTERN: Final = re.compile(r"/nix/store/[0-9a-z]{32}-(?P<name>[^/\s]+)\Z")
POKECON_STORE_NAME_PATTERN: Final = re.compile(r"pokecon(?:[.-]|\Z)")
GITHUB_API_BASE: Final = "https://api.github.com"


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


def _parse_run(value: object) -> RunInfo:
    context = "report.run"
    document = _require_object(value, context)
    _require_exact_keys(document, RUN_KEYS, context)
    started_at = _parse_github_timestamp(
        document["started_at"], f"{context}.started_at"
    )
    status_raw = _require_string(document["status"], f"{context}.status")
    status = status_raw.strip()
    if status not in ALLOWED_RUN_STATUSES:
        allowed = ", ".join(sorted(ALLOWED_RUN_STATUSES))
        message = f"{context}.status has unsupported value {status_raw!r}; expected one of: {allowed}"
        raise InputError(message)
    conclusion_raw = document["conclusion"]
    if conclusion_raw is None:
        conclusion = None
    else:
        conclusion = _require_string(conclusion_raw, f"{context}.conclusion")
        if not conclusion.strip():
            message = f"{context}.conclusion must be a non-empty string or null"
            raise InputError(message)
    return RunInfo(started_at=started_at, status=status, conclusion=conclusion)


def _parse_collection(value: object) -> CollectionInfo:
    context = "report.collection"
    document = _require_object(value, context)
    _require_exact_keys(document, COLLECTION_KEYS, context)
    kind_raw = _require_string(document["kind"], f"{context}.kind")
    kind = kind_raw.strip()
    if kind not in ALLOWED_COLLECTION_KINDS:
        allowed = ", ".join(sorted(ALLOWED_COLLECTION_KINDS))
        message = f"{context}.kind has unsupported value {kind_raw!r}; expected one of: {allowed}"
        raise InputError(message)
    return CollectionInfo(kind=kind)


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

    run = _parse_run(document["run"])
    collection = _parse_collection(document["collection"])

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
        run=run,
        collection=collection,
    )


def report_violations(report: TimingReport) -> tuple[str, ...]:
    """Return trust and evidence contradictions within one valid report."""
    violations: list[str] = []
    if report.cache.write and report.cache.event != "push":
        violations.append(
            "cache writes are permitted only for push events, "
            f"got event {report.cache.event!r}"
        )
    if report.cache.write and report.cache.actor not in TRUSTED_CACHE_WRITERS:
        violations.append(
            f"cache writes require a trusted actor, got actor {report.cache.actor!r}"
        )
    if report.substituted_store_paths and not report.cache.read:
        violations.append(
            "substituted store paths were recorded while cache.read is false"
        )
    if report.collection.kind != COLLECTION_KIND_UPSTREAM_COMPLETED_MAX:
        violations.append(
            f"collection.kind must be {COLLECTION_KIND_UPSTREAM_COMPLETED_MAX!r}, got {report.collection.kind!r}"
        )
    if report.run.status not in ALLOWED_RUN_STATUSES:
        violations.append(f"run.status has unsupported value {report.run.status!r}")
    critical = report.critical_path_wall_seconds
    if report.measured_wall_seconds + 1e-9 < critical:
        violations.append(
            f"measured_wall_seconds {report.measured_wall_seconds} is less than critical-path job wall {critical}"
        )
    if report.workflow.wall_seconds + 1e-9 < critical:
        violations.append(
            f"workflow wall_seconds {report.workflow.wall_seconds} is less than critical-path job wall {critical}"
        )
    if not report.cache.read:
        violations.append(
            "cache.read must be true for explicit timing evidence (was false)"
        )
    if not report.jobs or all(j.conclusion is Conclusion.SKIPPED for j in report.jobs):
        violations.append("jobs evidence is empty or all skipped")
    unsuccessful_jobs = tuple(
        sorted(
            job.name
            for job in report.jobs
            if job.conclusion not in {Conclusion.SUCCESS, Conclusion.SKIPPED}
        )
    )
    if unsuccessful_jobs:
        violations.append(
            "p95 history requires successful or skipped jobs; "
            f"unsuccessful jobs: {', '.join(unsuccessful_jobs)}"
        )
    return tuple(violations)


def _parse_github_timestamp(value: object, context: str) -> datetime.datetime:
    raw = _require_string(value, context)
    text = raw.strip()
    try:
        if text.endswith("Z"):
            text = text[:-1] + "+00:00"
        dt = datetime.datetime.fromisoformat(text)
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=datetime.UTC)
        return dt.astimezone(datetime.UTC)
    except Exception as error:
        message = f"{context} has invalid timestamp {raw!r}"
        raise InputError(message) from error


def _wall_seconds_between(
    started_raw: object, completed_raw: object, context: str
) -> float:
    started = _parse_github_timestamp(started_raw, f"{context}.started_at")
    completed = _parse_github_timestamp(completed_raw, f"{context}.completed_at")
    delta = (completed - started).total_seconds()
    if not math.isfinite(delta) or delta < 0:
        message = f"{context} has non-monotonic wall time"
        raise InputError(message)
    return delta


def _map_github_conclusion(raw: object, context: str) -> Conclusion:
    value = _require_string(raw, context)
    normalized = value.strip().lower()
    mapping: Mapping[str, Conclusion] = {
        "success": Conclusion.SUCCESS,
        "skipped": Conclusion.SKIPPED,
        "failure": Conclusion.FAILURE,
        "cancelled": Conclusion.CANCELLED,
        "timed_out": Conclusion.TIMED_OUT,
    }
    if normalized not in mapping:
        allowed = ", ".join(conclusion.value for conclusion in Conclusion)
        message = (
            f"{context} has unsupported conclusion {value!r}; "
            f"expected one of: {allowed}"
        )
        raise InputError(message)
    return mapping[normalized]


def _github_api_get(url: str, token: str) -> JsonDocument:
    if not token or not token.strip():
        message = "GITHUB_TOKEN is required for GitHub API collection"
        raise InputError(message)
    request = urllib.request.Request(url)  # noqa: S310 - URL is GitHub API validated above
    request.add_header("Authorization", f"Bearer {token.strip()}")
    request.add_header("Accept", "application/vnd.github+json")
    request.add_header("X-GitHub-Api-Version", "2022-11-28")
    try:
        with urllib.request.urlopen(request, timeout=15) as response:  # noqa: S310 - fixed GitHub API host
            status = response.status
            body = response.read()
            if status < 200 or status >= 300:
                message = f"GitHub API request to {url!r} failed with status {status}"
                raise InputError(message)
            try:
                document = json.loads(
                    body.decode("utf-8"),
                    object_pairs_hook=_object_without_duplicate_keys,
                    parse_constant=_reject_json_constant,
                )
            except json.JSONDecodeError as error:
                message = (
                    f"GitHub API response is not valid JSON at line {error.lineno}, "
                    f"column {error.colno}: {error.msg}"
                )
                raise InputError(message) from error
            return _require_object(cast("object", document), "github response")
    except urllib.error.HTTPError as error:
        body_text = ""
        try:
            body_text = error.read().decode("utf-8", errors="replace")[:500]
        except Exception:
            body_text = ""
        message = (
            f"GitHub API request to {url!r} failed with HTTP {error.code}: "
            f"{error.reason}; {body_text}"
        )
        raise InputError(message) from error
    except urllib.error.URLError as error:
        message = f"GitHub API request to {url!r} failed: {error.reason}"
        raise InputError(message) from error


def _fetch_workflow_run(
    repo: str,
    run_id: str,
    token: str,
    *,
    fetcher: Callable[[str, str], JsonDocument] | None = None,
) -> JsonDocument:
    if not re.fullmatch(r"[^/]+/[^/]+", repo):
        message = f"repo must be owner/name, got {repo!r}"
        raise InputError(message)
    if not re.fullmatch(r"[0-9]+", run_id):
        message = f"run_id must be numeric, got {run_id!r}"
        raise InputError(message)
    url = f"{GITHUB_API_BASE}/repos/{repo}/actions/runs/{run_id}"
    get = fetcher if fetcher is not None else _github_api_get
    return get(url, token)


def _fetch_jobs(
    repo: str,
    run_id: str,
    token: str,
    *,
    fetcher: Callable[[str, str], JsonDocument] | None = None,
) -> list[JsonDocument]:
    if not re.fullmatch(r"[^/]+/[^/]+", repo):
        message = f"repo must be owner/name, got {repo!r}"
        raise InputError(message)
    if not re.fullmatch(r"[0-9]+", run_id):
        message = f"run_id must be numeric, got {run_id!r}"
        raise InputError(message)
    get = fetcher if fetcher is not None else _github_api_get
    jobs: list[JsonDocument] = []
    page = 1
    total: int | None = None
    while True:
        url = f"{GITHUB_API_BASE}/repos/{repo}/actions/runs/{run_id}/jobs?per_page=100&page={page}"
        document = get(url, token)
        batch = _require_array(document.get("jobs"), "github jobs response.jobs")
        jobs.extend(
            _require_object(item, "github jobs response.jobs[]") for item in batch
        )
        if total is None:
            raw_total = document.get("total_count")
            if isinstance(raw_total, int) and raw_total >= 0:
                total = raw_total
            else:
                total = len(jobs)
        if len(jobs) >= total or not batch:
            break
        page += 1
        if page > 10:
            message = "GitHub jobs pagination exceeded 10 pages"
            raise InputError(message)
    return jobs


def _parse_jobs_evidence(
    raw: object, context: str
) -> dict[str, tuple[tuple[str, ...], tuple[str, ...]]]:
    document = _require_object(raw, context)
    evidence: dict[str, tuple[tuple[str, ...], tuple[str, ...]]] = {}
    for job_name, job_value in document.items():
        if not isinstance(job_name, str) or not job_name.strip():
            message = f"{context} has invalid job name {job_name!r}"
            raise InputError(message)
        job_doc = _require_object(job_value, f"{context}[{job_name!r}]")
        _require_exact_keys(
            job_doc,
            frozenset({"built_derivations", "substituted_store_paths"}),
            f"{context}[{job_name!r}]",
        )
        built = _require_store_paths(
            job_doc["built_derivations"],
            f"{context}[{job_name!r}].built_derivations",
            derivations=True,
        )
        substituted = _require_store_paths(
            job_doc["substituted_store_paths"],
            f"{context}[{job_name!r}].substituted_store_paths",
            derivations=False,
        )
        if job_name in evidence:
            message = f"{context} has duplicate job {job_name!r}"
            raise InputError(message)
        evidence[job_name] = (built, substituted)
    return evidence


def _build_report_from_github(
    *,
    sha: str,
    attempt: int,
    change_kind: ChangeKind,
    regions: tuple[str, ...],
    cache_actor: str,
    cache_event: str,
    cache_read: bool,
    cache_write: bool,
    jobs_evidence: Mapping[str, tuple[tuple[str, ...], tuple[str, ...]]],
    run_data: JsonDocument,
    jobs_data: Sequence[JsonDocument],
) -> TimingReport:
    # Validate sha/attempt/change_kind/regions/cache already validated by caller,
    # but re-validate for direct invocation.
    if SHA_PATTERN.fullmatch(sha) is None:
        message = "report.sha must contain exactly 40 hexadecimal characters"
        raise InputError(message)
    if attempt < 1:
        message = "report.attempt must be an integer greater than or equal to 1"
        raise InputError(message)
    # Validate run_data matches sha/attempt when present
    head_sha = run_data.get("head_sha")
    if isinstance(head_sha, str) and head_sha.lower() != sha.lower():
        message = (
            f"GitHub run head_sha {head_sha!r} does not match requested sha {sha!r}"
        )
        raise InputError(message)
    run_attempt = run_data.get("run_attempt")
    if isinstance(run_attempt, int) and run_attempt != attempt:
        message = f"GitHub run attempt {run_attempt!r} does not match requested attempt {attempt!r}"
        raise InputError(message)
    # Workflow timing - completion-safe upstream completed max
    workflow_name_raw = (
        run_data.get("name") or run_data.get("displayTitle") or "Normal CI"
    )
    workflow_name = _require_string(workflow_name_raw, "github run name")
    started_raw = run_data.get("run_started_at") or run_data.get("created_at")
    if started_raw is None:
        message = "GitHub run is missing run_started_at/created_at timestamp"
        raise InputError(message)
    status_raw = run_data.get("status")
    if status_raw is None:
        status_raw = "completed"
    status = _require_string(status_raw, "github run status").strip()
    if status not in ALLOWED_RUN_STATUSES:
        allowed = ", ".join(sorted(ALLOWED_RUN_STATUSES))
        message = f"github run status has unsupported value {status_raw!r}; expected one of: {allowed}"
        raise InputError(message)
    conclusion_raw = run_data.get("conclusion")
    if conclusion_raw is None:
        run_conclusion = None
    else:
        run_conclusion = (
            _require_string(conclusion_raw, "github run conclusion").strip() or None
        )
    run_started_at = _parse_github_timestamp(started_raw, "github run started_at")
    workflow_name_captured = workflow_name
    run_status_captured = status
    run_conclusion_captured = run_conclusion
    run_started_captured = run_started_at
    # Cache
    cache = CacheAccess(
        read=cache_read, write=cache_write, actor=cache_actor, event=cache_event
    )
    # Jobs
    job_timings: list[JobTiming] = []
    seen_names: set[str] = set()
    for raw_job in jobs_data:
        job_name = _require_string(raw_job.get("name"), "github job name")
        if job_name in seen_names:
            message = f"github jobs must have unique names; duplicate {job_name!r}"
            raise InputError(message)
        seen_names.add(job_name)
        # conclusion: GitHub may have null for in-progress, but we require it for completed runs
        raw_conclusion = raw_job.get("conclusion")
        if raw_conclusion is None:
            # Check status: if status != completed, fail closed
            status = raw_job.get("status")
            if status != "completed":
                message = f"github job {job_name!r} is not completed; status {status!r}"
                raise InputError(message)
            message = f"github job {job_name!r} is missing conclusion"
            raise InputError(message)
        conclusion = _map_github_conclusion(
            raw_conclusion, f"github job {job_name!r}.conclusion"
        )
        # For skipped jobs, wall time is 0 and steps may be missing
        if conclusion is Conclusion.SKIPPED:
            wall_seconds = 0.0
            steps: tuple[StepTiming, ...] = ()
        else:
            started = raw_job.get("started_at")
            completed = raw_job.get("completed_at")
            if not isinstance(started, str) or not isinstance(completed, str):
                message = (
                    f"github job {job_name!r} is missing started_at or completed_at"
                )
                raise InputError(message)
            wall_seconds = _wall_seconds_between(
                started, completed, f"github job {job_name!r}"
            )
            raw_steps_value = raw_job.get("steps")
            if not isinstance(raw_steps_value, list):
                message = f"github job {job_name!r} is missing steps array"
                raise InputError(message)
            raw_steps = cast("list[object]", raw_steps_value)
            step_timings: list[StepTiming] = []
            step_names: set[str] = set()
            for index, raw_step in enumerate(raw_steps):
                step_doc = _require_object(
                    raw_step, f"github job {job_name!r}.steps[{index}]"
                )
                step_name = _require_string(
                    step_doc.get("name"), f"github job {job_name!r}.steps[{index}].name"
                )
                if step_name in step_names:
                    message = f"github job {job_name!r}.steps must have unique names; duplicate {step_name!r}"
                    raise InputError(message)
                step_names.add(step_name)
                s_started = step_doc.get("started_at")
                s_completed = step_doc.get("completed_at")
                if not isinstance(s_started, str) or not isinstance(s_completed, str):
                    message = f"github job {job_name!r} step {step_name!r} is missing timestamps"
                    raise InputError(message)
                s_wall = _wall_seconds_between(
                    s_started,
                    s_completed,
                    f"github job {job_name!r} step {step_name!r}",
                )
                step_timings.append(StepTiming(name=step_name, wall_seconds=s_wall))
            steps = tuple(sorted(step_timings, key=lambda s: s.name))
        # Nix evidence from explicit workflow outputs
        if job_name in jobs_evidence:
            built, substituted = jobs_evidence[job_name]
        else:
            built = ()
            substituted = ()
        job_timings.append(
            JobTiming(
                name=job_name,
                conclusion=conclusion,
                wall_seconds=wall_seconds,
                steps=steps,
                built_derivations=built,
                substituted_store_paths=substituted,
            )
        )
    # Ensure at least one job? Fail closed if no jobs
    if not job_timings:
        message = "GitHub jobs response contains no jobs"
        raise InputError(message)
    # Check that supplied jobs_evidence does not contain unknown jobs (typo)
    # But allow extra evidence for jobs not in API? That would be inventing, so fail closed if evidence contains job not in API
    api_names = {j.name for j in job_timings}
    for ev_name in jobs_evidence:
        if ev_name not in api_names:
            message = (
                f"jobs_evidence contains unknown job {ev_name!r} not in GitHub jobs"
            )
            raise InputError(message)
    # Sort jobs by name for canonicalization
    jobs_sorted = tuple(sorted(job_timings, key=lambda j: j.name))
    completed_ends: list[datetime.datetime] = []
    for raw_job in jobs_data:
        c = raw_job.get("completed_at")
        if isinstance(c, str) and c.strip():
            try:
                completed_ends.append(
                    _parse_github_timestamp(c, "github job completed_at")
                )
            except InputError:
                continue
    if not completed_ends:
        measured_wall_seconds = max(
            (
                j.wall_seconds
                for j in jobs_sorted
                if j.conclusion is not Conclusion.SKIPPED
            ),
            default=0.0,
        )
        workflow_wall = measured_wall_seconds
    else:
        max_completed = max(completed_ends)
        delta = (max_completed - run_started_captured).total_seconds()
        if not math.isfinite(delta) or delta < 0:
            message = "github run has non-monotonic timing for upstream max"
            raise InputError(message)
        measured_wall_seconds = delta
        workflow_wall = delta
    workflow_timing = WorkflowTiming(
        name=workflow_name_captured, wall_seconds=workflow_wall
    )
    run_info = RunInfo(
        started_at=run_started_captured,
        status=run_status_captured,
        conclusion=run_conclusion_captured,
    )
    collection_info = CollectionInfo(kind=COLLECTION_KIND_UPSTREAM_COMPLETED_MAX)
    return TimingReport(
        sha=sha.lower(),
        attempt=attempt,
        change_kind=change_kind,
        regions=regions,
        measured_wall_seconds=measured_wall_seconds,
        workflow=workflow_timing,
        jobs=jobs_sorted,
        cache=cache,
        run=run_info,
        collection=collection_info,
    )


def collect_report(
    *,
    repo: str,
    run_id: str,
    sha: str,
    attempt: int,
    change_kind: str | ChangeKind,
    regions: Sequence[str] | str,
    cache_actor: str,
    cache_event: str,
    cache_read: bool | str,
    cache_write: bool | str,
    jobs_evidence_json: str | Mapping[str, object],
    token: str | None = None,
    workflow_name: str | None = None,
    fetcher: Callable[[str, str], JsonDocument] | None = None,
) -> TimingReport:
    """Collect timing evidence via GitHub API and explicit workflow outputs."""
    token_value = token if token is not None else os.environ.get("GITHUB_TOKEN", "")
    if not token_value or not token_value.strip():
        message = "GITHUB_TOKEN is required for GitHub API collection"
        raise InputError(message)
    # Normalize repo/run_id
    repo_norm = repo.strip()
    run_id_norm = run_id.strip()
    sha_norm = sha.strip().lower()
    if SHA_PATTERN.fullmatch(sha_norm) is None:
        # Allow uppercase input, then lower
        if SHA_PATTERN.fullmatch(sha.strip()) is None:
            message = "report.sha must contain exactly 40 hexadecimal characters"
            raise InputError(message)
        sha_norm = sha.strip().lower()
    if not isinstance(attempt, int) or attempt < 1:
        # attempt may be passed as string from CLI
        try:
            attempt_int = int(attempt)
        except Exception as error:
            message = "report.attempt must be an integer greater than or equal to 1"
            raise InputError(message) from error
        if attempt_int < 1:
            message = "report.attempt must be an integer greater than or equal to 1"
            raise InputError(message)
        attempt = attempt_int
    # change_kind
    raw_kind = (
        change_kind.value if isinstance(change_kind, ChangeKind) else str(change_kind)
    )
    raw_kind = raw_kind.strip()
    try:
        kind = ChangeKind(raw_kind)
    except ValueError as error:
        allowed = ", ".join(k.value for k in ChangeKind)
        message = f"report.change_kind has unsupported value {raw_kind!r}; expected one of: {allowed}"
        raise InputError(message) from error
    # regions: may be JSON string or list
    if isinstance(regions, str):
        try:
            parsed_regions = json.loads(
                regions,
                object_pairs_hook=_object_without_duplicate_keys,
                parse_constant=_reject_json_constant,
            )
        except json.JSONDecodeError as error:
            message = f"regions is not valid JSON: {error.msg}"
            raise InputError(message) from error
        regions_array = _require_array(cast("object", parsed_regions), "regions")
        region_list = [
            _require_string(v, f"regions[{i}]") for i, v in enumerate(regions_array)
        ]
    else:
        region_list = list(regions)  # type: ignore[arg-type]
    # Validate regions sorted, unique, pattern
    regions_tuple = _require_string_array(
        region_list,
        "report.regions",
        pattern=REGION_PATTERN,
        require_sorted=True,
    )
    # cache
    actor = cache_actor.strip()
    if not actor:
        message = "cache.actor must be a non-empty string"
        raise InputError(message)
    event = cache_event.strip()
    if EVENT_PATTERN.fullmatch(event) is None:
        message = f"report.cache.event has invalid value {event!r}"
        raise InputError(message)

    # cache_read/write may be bool or string
    def _parse_bool(value: object, context: str) -> bool:
        if isinstance(value, bool):
            return value
        if isinstance(value, str):
            low = value.strip().lower()
            if low == "true":
                return True
            if low == "false":
                return False
        message = f"{context} must be a boolean"
        raise InputError(message)

    read = _parse_bool(cache_read, "cache.read")
    write = _parse_bool(cache_write, "cache.write")
    # jobs_evidence_json
    if isinstance(jobs_evidence_json, str):
        raw_ev = jobs_evidence_json.strip()
        if not raw_ev:
            evidence_obj: Mapping[str, object] = {}
        else:
            try:
                parsed = json.loads(
                    raw_ev,
                    object_pairs_hook=_object_without_duplicate_keys,
                    parse_constant=_reject_json_constant,
                )
            except json.JSONDecodeError as error:
                message = f"jobs_evidence is not valid JSON: {error.msg}"
                raise InputError(message) from error
            evidence_obj = _require_object(cast("object", parsed), "jobs_evidence")
    else:
        evidence_obj = jobs_evidence_json
    jobs_evidence = _parse_jobs_evidence(evidence_obj, "jobs_evidence")
    # Fetch GitHub data
    run_data = _fetch_workflow_run(repo_norm, run_id_norm, token_value, fetcher=fetcher)
    jobs_data = _fetch_jobs(repo_norm, run_id_norm, token_value, fetcher=fetcher)
    # If workflow_name override provided, patch run_data
    if workflow_name is not None and workflow_name.strip():
        # Create copy to avoid mutating original
        run_data = dict(run_data)
        run_data["name"] = workflow_name.strip()
    return _build_report_from_github(
        sha=sha_norm,
        attempt=attempt,
        change_kind=kind,
        regions=regions_tuple,
        cache_actor=actor,
        cache_event=event,
        cache_read=read,
        cache_write=write,
        jobs_evidence=jobs_evidence,
        run_data=run_data,
        jobs_data=jobs_data,
    )


def _collect_from_deterministic_jsons(
    *, jobs_json: str, run_metadata_json: str
) -> TimingReport:
    """Collect timing report from deterministic JSON inputs without network."""
    try:
        raw_jobs = json.loads(
            jobs_json,
            object_pairs_hook=_object_without_duplicate_keys,
            parse_constant=_reject_json_constant,
        )
    except json.JSONDecodeError as error:
        message = f"jobs JSON is not valid JSON at line {error.lineno}, column {error.colno}: {error.msg}"
        raise InputError(message) from error
    jobs_array: list[object]
    if isinstance(raw_jobs, dict):
        jobs_doc = _require_object(cast("object", raw_jobs), "jobs JSON")
        if "jobs" in jobs_doc:
            jobs_array = _require_array(jobs_doc["jobs"], "jobs JSON.jobs")
        else:
            message = "jobs JSON object must contain key 'jobs'"
            raise InputError(message)
    elif isinstance(raw_jobs, list):
        jobs_array = cast("list[object]", raw_jobs)
    else:
        message = "jobs JSON must be an object with 'jobs' key or an array"
        raise InputError(message)
    jobs_data: list[JsonDocument] = []
    for idx, raw_job in enumerate(jobs_array):
        doc = _require_object(raw_job, f"jobs JSON.jobs[{idx}]")
        jobs_data.append(doc)
    try:
        raw_meta = json.loads(
            run_metadata_json,
            object_pairs_hook=_object_without_duplicate_keys,
            parse_constant=_reject_json_constant,
        )
    except json.JSONDecodeError as error:
        message = f"run metadata JSON is not valid JSON at line {error.lineno}, column {error.colno}: {error.msg}"
        raise InputError(message) from error
    meta = _require_object(cast("object", raw_meta), "run metadata")
    expected_meta_keys = frozenset(
        {
            "sha",
            "attempt",
            "change_kind",
            "regions",
            "workflow",
            "measured_wall_seconds",
            "cache",
            "jobs_evidence",
            "run",
            "collection",
        }
    )
    _require_exact_keys(meta, expected_meta_keys, "run metadata")
    sha = _require_string(meta["sha"], "run metadata.sha")
    if SHA_PATTERN.fullmatch(sha) is None:
        message = "run metadata.sha must contain exactly 40 hexadecimal characters"
        raise InputError(message)
    sha = sha.lower()
    attempt_val = meta["attempt"]
    if (
        isinstance(attempt_val, bool)
        or not isinstance(attempt_val, int)
        or attempt_val < 1
    ):
        message = "run metadata.attempt must be an integer greater than or equal to 1"
        raise InputError(message)
    attempt = attempt_val
    raw_kind = _require_string(meta["change_kind"], "run metadata.change_kind")
    try:
        change_kind = ChangeKind(raw_kind)
    except ValueError as error:
        allowed = ", ".join(k.value for k in ChangeKind)
        message = f"run metadata.change_kind has unsupported value {raw_kind!r}; expected one of: {allowed}"
        raise InputError(message) from error
    regions = _require_string_array(
        meta["regions"],
        "run metadata.regions",
        pattern=REGION_PATTERN,
        require_sorted=True,
    )
    workflow = _parse_workflow(meta["workflow"])
    measured = _require_wall_seconds(
        meta["measured_wall_seconds"], "run metadata.measured_wall_seconds"
    )
    cache = _parse_cache(meta["cache"])
    jobs_evidence = _parse_jobs_evidence(
        meta["jobs_evidence"], "run metadata.jobs_evidence"
    )
    run = _parse_run(meta["run"])
    collection = _parse_collection(meta["collection"])
    if collection.kind != COLLECTION_KIND_UPSTREAM_COMPLETED_MAX:
        message = f"run metadata.collection.kind must be {COLLECTION_KIND_UPSTREAM_COMPLETED_MAX!r}"
        raise InputError(message)
    job_timings: list[JobTiming] = []
    seen_names: set[str] = set()
    for raw_job in jobs_data:
        job_name = _require_string(raw_job.get("name"), "jobs JSON job name")
        if job_name in seen_names:
            message = f"jobs JSON must have unique names; duplicate {job_name!r}"
            raise InputError(message)
        seen_names.add(job_name)
        raw_conclusion = raw_job.get("conclusion")
        if raw_conclusion is None:
            status = raw_job.get("status")
            if status is not None and status != "completed":
                message = (
                    f"jobs JSON job {job_name!r} is not completed; status {status!r}"
                )
                raise InputError(message)
            message = f"jobs JSON job {job_name!r} is missing conclusion"
            raise InputError(message)
        conclusion = _map_github_conclusion(
            raw_conclusion, f"jobs JSON job {job_name!r}.conclusion"
        )
        if conclusion is Conclusion.SKIPPED:
            wall_seconds = 0.0
            steps: tuple[StepTiming, ...] = ()
        else:
            started = raw_job.get("started_at")
            completed = raw_job.get("completed_at")
            if not isinstance(started, str) or not isinstance(completed, str):
                message = (
                    f"jobs JSON job {job_name!r} is missing started_at or completed_at"
                )
                raise InputError(message)
            wall_seconds = _wall_seconds_between(
                started, completed, f"jobs JSON job {job_name!r}"
            )
            raw_steps_value = raw_job.get("steps")
            if not isinstance(raw_steps_value, list):
                message = f"jobs JSON job {job_name!r} is missing steps array"
                raise InputError(message)
            raw_steps = cast("list[object]", raw_steps_value)
            step_timings: list[StepTiming] = []
            step_names: set[str] = set()
            for s_idx, raw_step in enumerate(raw_steps):
                step_doc = _require_object(
                    raw_step, f"jobs JSON job {job_name!r}.steps[{s_idx}]"
                )
                step_name = _require_string(
                    step_doc.get("name"),
                    f"jobs JSON job {job_name!r}.steps[{s_idx}].name",
                )
                if step_name in step_names:
                    message = f"jobs JSON job {job_name!r}.steps must have unique names; duplicate {step_name!r}"
                    raise InputError(message)
                step_names.add(step_name)
                s_started = step_doc.get("started_at")
                s_completed = step_doc.get("completed_at")
                if not isinstance(s_started, str) or not isinstance(s_completed, str):
                    message = f"jobs JSON job {job_name!r} step {step_name!r} is missing timestamps"
                    raise InputError(message)
                s_wall = _wall_seconds_between(
                    s_started,
                    s_completed,
                    f"jobs JSON job {job_name!r} step {step_name!r}",
                )
                step_timings.append(StepTiming(name=step_name, wall_seconds=s_wall))
            steps = tuple(sorted(step_timings, key=lambda s: s.name))
        if job_name in jobs_evidence:
            built, substituted = jobs_evidence[job_name]
        else:
            built = ()
            substituted = ()
        job_timings.append(
            JobTiming(
                name=job_name,
                conclusion=conclusion,
                wall_seconds=wall_seconds,
                steps=steps,
                built_derivations=built,
                substituted_store_paths=substituted,
            )
        )
    if not job_timings:
        message = "jobs JSON contains no jobs"
        raise InputError(message)
    api_names = {j.name for j in job_timings}
    for ev_name in jobs_evidence:
        if ev_name not in api_names:
            message = f"run metadata.jobs_evidence contains unknown job {ev_name!r} not in jobs JSON"
            raise InputError(message)
    jobs_sorted = tuple(sorted(job_timings, key=lambda j: j.name))
    completed_times: list[datetime.datetime] = []
    for raw_job in jobs_data:
        name = raw_job.get("name")
        c = raw_job.get("completed_at")
        s = raw_job.get("started_at")
        if isinstance(c, str) and isinstance(s, str):
            try:
                c_dt = _parse_github_timestamp(
                    c, f"jobs JSON job {name!r}.completed_at"
                )
                completed_times.append(c_dt)
            except InputError:
                continue
    if completed_times:
        max_completed = max(completed_times)
        expected_upstream = (max_completed - run.started_at).total_seconds()
        if not math.isfinite(expected_upstream) or expected_upstream < 0:
            message = "run metadata has non-monotonic upstream timing"
            raise InputError(message)
        if measured + 1e-9 < expected_upstream:
            message = f"measured_wall_seconds {measured} is less than upstream completed max {expected_upstream} (run started at {run.started_at.isoformat()}) - under-measures critical path"
            raise InputError(message)
    else:
        critical = max(
            (
                j.wall_seconds
                for j in jobs_sorted
                if j.conclusion is not Conclusion.SKIPPED
            ),
            default=0.0,
        )
        if measured + 1e-9 < critical:
            message = f"measured_wall_seconds {measured} is less than critical-path job wall {critical}"
            raise InputError(message)
    critical = max(
        (j.wall_seconds for j in jobs_sorted if j.conclusion is not Conclusion.SKIPPED),
        default=0.0,
    )
    if measured + 1e-9 < critical:
        message = f"measured_wall_seconds {measured} is less than critical-path job wall {critical}"
        raise InputError(message)
    if workflow.wall_seconds + 1e-9 < critical:
        message = f"workflow wall_seconds {workflow.wall_seconds} is less than critical-path job wall {critical}"
        raise InputError(message)
    return TimingReport(
        sha=sha,
        attempt=attempt,
        change_kind=change_kind,
        regions=regions,
        measured_wall_seconds=measured,
        workflow=workflow,
        jobs=jobs_sorted,
        cache=cache,
        run=run,
        collection=collection,
    )


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

    # Fail-closed staleness: history must be contiguous and recent.
    # Stale is defined as any sample older than MAX_HISTORY_AGE_DAYS relative to newest sample,
    # or future-dated. This prevents reusing ancient baselines that hide regressions and ensures
    # p95 is computed from relevant, completed runs.
    if ordered_reports:
        newest_started = max(report.run.started_at for report in ordered_reports)
        for report in ordered_reports:
            age_seconds = (newest_started - report.run.started_at).total_seconds()
            if age_seconds < -1.0:
                violations.append(
                    f"{_report_identity(report)}: report is stale - future started_at "
                    f"{report.run.started_at.isoformat()} is after newest {newest_started.isoformat()}"
                )
            elif age_seconds > MAX_HISTORY_STALE_SECONDS:
                violations.append(
                    f"{_report_identity(report)}: report is stale - age {age_seconds / 86400:.1f} days exceeds "
                    f"{MAX_HISTORY_AGE_DAYS} days"
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


def _read_json_argument(value: str) -> str:
    """Accept JSON text or a path to a JSON file for CI shell interoperability."""
    candidate = Path(value)
    try:
        if candidate.is_file():
            return candidate.read_text(encoding="utf-8")
    except OSError:
        pass
    return value


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

    collect_parser = subparsers.add_parser(
        "collect", help="collect deterministic timing evidence from GitHub jobs JSON"
    )
    collect_parser.add_argument(
        "--jobs",
        required=True,
        help="GitHub jobs API JSON (jobs array or full response with jobs key)",
    )
    collect_parser.add_argument(
        "--run-metadata",
        required=True,
        help="Explicit run metadata JSON with sha, attempt, change_kind, regions, workflow, measured_wall_seconds, cache, jobs_evidence",
    )
    collect_parser.add_argument(
        "--output",
        required=False,
        default=None,
        help="path to write collected report JSON",
    )
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
            report = parse_report(_read_json_argument(cast("str", arguments.report)))
            document = validation_document(report)
        elif command == "p95":
            report_sources = cast("list[str]", arguments.reports)
            reports = tuple(
                parse_report(_read_json_argument(source)) for source in report_sources
            )
            document = p95_document(reports)
        elif command == "compare":
            first = parse_report(_read_json_argument(cast("str", arguments.first)))
            second = parse_report(_read_json_argument(cast("str", arguments.second)))
            document = comparison_document(first, second)
        elif command == "collect":
            report = _collect_from_deterministic_jsons(
                jobs_json=_read_json_argument(cast("str", arguments.jobs)),
                run_metadata_json=_read_json_argument(
                    cast("str", arguments.run_metadata)
                ),
            )
            output_path = cast("str | None", arguments.output)
            if output_path is not None:
                with open(output_path, "w", encoding="utf-8", newline="\n") as handle:
                    handle.write(_canonical_json(report.to_document()))
                    handle.write("\n")
            document = validation_document(report)
        else:
            raise AssertionError(command)
    except InputError as error:
        print(f"ci-timing: input error: {error}", file=sys.stderr)
        return 2
    return _emit(document)


if __name__ == "__main__":
    raise SystemExit(main())
