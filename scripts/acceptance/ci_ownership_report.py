"""Region/job/check ownership matrix for the Normal CI workflow.

Parses ``.github/workflows/normal-ci.yml`` with the standard library only,
maps the eight stable execution regions to plan outputs, job gates, and the
required-check aggregate expressions, and derives representative single-region
fixture sets (verified through ``scripts/ci/regions.py``) so trigger
ownership is auditable from a single JSON artifact.
"""

from __future__ import annotations

import argparse
import json
import re
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Final, Never, TypedDict, cast

from scripts.ci.regions import REGIONS, classify_paths

if TYPE_CHECKING:
    from collections.abc import Sequence

type JsonObject = dict[str, object]

SCHEMA_ID: Final = "ci-ownership-report/1"
WORKFLOW_RELATIVE: Final = Path(".github/workflows/normal-ci.yml")
AUXILIARY_OUTPUTS: Final = ("regions_json", "performance_baseline")
ALWAYS_JOB: Final = "fast"
PLANNER_JOB: Final = "plan"

JOB_HEADER_RE: Final = re.compile(r"(?m)^  ([A-Za-z0-9_-]+):\s*$")
JOBS_SECTION_RE: Final = re.compile(r"(?m)^jobs:\s*$")
JOB_NAME_RE: Final = re.compile(r"(?m)^    name: (.+?)\s*$")
JOB_IF_RE: Final = re.compile(r"(?m)^    if: (.*)$")
JOB_RUNS_ON_RE: Final = re.compile(r"(?m)^    runs-on: (.+?)\s*$")
OUTPUTS_SECTION_RE: Final = re.compile(r"(?m)^    outputs:\s*$")
OUTPUT_ENTRY_RE: Final = re.compile(r"(?m)^      ([A-Za-z0-9_]+): (.*?)\s*$")
NEEDS_SECTION_RE: Final = re.compile(r"(?m)^    needs:\s*$")
NEEDS_ENTRY_RE: Final = re.compile(r"(?m)^      - (\S+)\s*$")
REGION_REF_RE: Final = re.compile(r"needs\.plan\.outputs\.([A-Za-z0-9_]+)")
AGGREGATE_ENTRY_RE: Final = re.compile(
    r"\"([A-Za-z0-9_]+)\":\{\"applicable\":"
    r"(\$\{\{.*?\}\}|true|false),\"reason\":\"([^\"]*)\"\}"
)
REGION_OUTPUT_ENTRY_RE: Final = re.compile(
    r"\"([A-Za-z0-9_]+)\":\"\$\{\{ needs\.plan\.outputs\.([A-Za-z0-9_]+) \}\}\""
)
RESULT_ARG_RE: Final = re.compile(
    r"(?m)^\s*([A-Za-z0-9_]+)=\$\{\{ needs\.([A-Za-z0-9_]+)\.result \}\}"
)

FIXTURE_PATHS: Final = (
    ("docs", ("docs/guide.md",)),
    ("contracts", ("api/schema.json",)),
    ("rust", ("rust/tooling/src/lib.rs",)),
    ("python", ("tools/helper.py",)),
    ("routing", (".gitignore",)),
    ("web", ("web/src/app.css",)),
    ("product", ("scripts/performance/collect.sh",)),
    ("remote_flake", ("Cargo.toml",)),
    ("mixed", ("docs/guide.md", "rust/tooling/src/lib.rs")),
)


def invalid_value(message: str) -> Never:
    raise ValueError(message)


def mapping(value: object, label: str) -> JsonObject:
    if not isinstance(value, dict):
        invalid_value(f"{label} must be an object")
    untyped = cast("dict[object, object]", value)
    if not all(isinstance(key, str) for key in untyped):
        invalid_value(f"{label} must use string keys")
    return {cast("str", key): item for key, item in untyped.items()}


@dataclass(frozen=True)
class JobEntry:
    job_id: str
    name: str
    gates: tuple[str, ...]
    condition_raw: str | None


@dataclass(frozen=True)
class AggregateEntry:
    job_id: str
    expression: str
    regions: tuple[str, ...]
    reason: str


@dataclass(frozen=True)
class FixtureEntry:
    name: str
    changed_paths: tuple[str, ...]
    regions: tuple[str, ...]
    applicable_jobs: tuple[str, ...]


class JobReport(TypedDict):
    name: str
    gates: list[str]
    condition_raw: str | None


class AggregateReport(TypedDict):
    applicable_expression: str
    regions: list[str]
    reason: str


class RequiredReport(TypedDict):
    check_name: str
    needs: list[str]
    region_outputs: list[str]
    aggregates: dict[str, AggregateReport]
    result_arguments: list[str]


class FixtureReport(TypedDict):
    name: str
    changed_paths: list[str]
    regions: list[str]
    applicable_jobs: list[str]


def known_names() -> frozenset[str]:
    return frozenset([region.value for region in REGIONS] + list(AUXILIARY_OUTPUTS))


def extract_region_refs(expression: str, *, label: str) -> tuple[str, ...]:
    refs: list[str] = []
    for ref in REGION_REF_RE.findall(expression):
        if ref not in known_names():
            invalid_value(f"{label} references unknown region: {ref!r}")
        if ref not in refs:
            refs.append(ref)
    return tuple(refs)


def split_job_blocks(text: str) -> dict[str, str]:
    section = JOBS_SECTION_RE.search(text)
    if section is None:
        invalid_value("workflow has no top-level jobs section")
    body = text[section.end() :]
    headers = list(JOB_HEADER_RE.finditer(body))
    if not headers:
        invalid_value("workflow jobs section names no jobs")
    blocks: dict[str, str] = {}
    for index, header in enumerate(headers):
        job_id = header.group(1)
        if job_id in blocks:
            invalid_value(f"duplicate workflow job id: {job_id!r}")
        end = headers[index + 1].start() if index + 1 < len(headers) else len(body)
        blocks[job_id] = body[header.end() : end]
    return blocks


def extract_job_condition(block: str) -> str | None:
    match = JOB_IF_RE.search(block)
    if match is None:
        return None
    first = match.group(1).strip()
    if first not in (">", ">-", "|", "|-"):
        return first
    continued: list[str] = []
    for line in block[match.end() :].splitlines()[1:]:
        if not line.strip():
            continue
        if len(line) - len(line.lstrip(" ")) < 6:
            break
        continued.append(line.strip())
    if not continued:
        invalid_value("workflow job uses a folded if without a condition body")
    return " ".join(continued)


def parse_jobs(blocks: dict[str, str]) -> dict[str, JobEntry]:
    jobs: dict[str, JobEntry] = {}
    for job_id, block in blocks.items():
        if JOB_RUNS_ON_RE.search(block) is None:
            invalid_value(f"workflow job {job_id!r} has no runner declaration")
        name_match = JOB_NAME_RE.search(block)
        if name_match is None:
            invalid_value(f"workflow job {job_id!r} has no display name")
        condition = extract_job_condition(block)
        gates = (
            extract_region_refs(condition, label=f"job {job_id!r} if")
            if condition is not None
            else ()
        )
        gate_regions = tuple(
            ref for ref in gates if ref in [region.value for region in REGIONS]
        )
        jobs[job_id] = JobEntry(
            job_id=job_id,
            name=name_match.group(1),
            gates=gate_regions,
            condition_raw=condition,
        )
    return jobs


def parse_plan_outputs(block: str) -> dict[str, str]:
    section = OUTPUTS_SECTION_RE.search(block)
    if section is None:
        invalid_value("plan job has no outputs block")
    tail = block[section.end() :]
    outputs: dict[str, str] = {}
    for line in tail.splitlines()[1:]:
        if not line.strip():
            continue
        if len(line) - len(line.lstrip(" ")) < 6:
            break
        entry = OUTPUT_ENTRY_RE.match(line)
        if entry is None:
            invalid_value(f"plan outputs block has a malformed entry: {line!r}")
        key = entry.group(1)
        if key in outputs:
            invalid_value(f"duplicate plan output: {key!r}")
        if key not in known_names():
            invalid_value(f"plan output maps to an unknown region: {key!r}")
        outputs[key] = entry.group(2)
    for region in REGIONS:
        if region.value not in outputs:
            invalid_value(f"region {region.value!r} is missing from plan outputs")
    return outputs


def parse_required_needs(block: str) -> list[str]:
    section = NEEDS_SECTION_RE.search(block)
    if section is None:
        invalid_value("required job has no needs list")
    needs: list[str] = []
    for line in block[section.end() :].splitlines()[1:]:
        if not line.strip():
            continue
        if not line.startswith("      "):
            break
        entry = NEEDS_ENTRY_RE.match(line)
        if entry is None:
            break
        if entry.group(1) in needs:
            invalid_value(f"duplicate required needs entry: {entry.group(1)!r}")
        needs.append(entry.group(1))
    if not needs:
        invalid_value("required job needs list is empty")
    return needs


def parse_plan_json(block: str) -> tuple[dict[str, str], dict[str, AggregateEntry]]:
    anchor = block.find("PLAN_JSON:")
    if anchor < 0:
        invalid_value("required job has no PLAN_JSON block")
    segment = block[anchor:]
    region_anchor = segment.find('"region_outputs"')
    if region_anchor < 0:
        invalid_value("required PLAN_JSON block has no region_outputs map")
    region_segment = segment[region_anchor : region_anchor + 2000]
    region_outputs: dict[str, str] = {}
    for entry in REGION_OUTPUT_ENTRY_RE.finditer(region_segment):
        if entry.group(1) != entry.group(2):
            invalid_value(
                "required region_outputs key does not match "
                f"its plan output: {entry.group(1)!r}"
            )
        if entry.group(1) in region_outputs:
            invalid_value(f"duplicate required region output: {entry.group(1)!r}")
        region_outputs[entry.group(1)] = entry.group(2)
    if not region_outputs:
        invalid_value("required PLAN_JSON block has a malformed region_outputs map")
    aggregates: dict[str, AggregateEntry] = {}
    for entry in AGGREGATE_ENTRY_RE.finditer(segment):
        job_id = entry.group(1)
        if job_id in aggregates:
            invalid_value(f"duplicate aggregate entry: {job_id!r}")
        regions = extract_region_refs(
            entry.group(2), label=f"aggregate {job_id!r} expression"
        )
        gate_regions = tuple(
            ref for ref in regions if ref in [region.value for region in REGIONS]
        )
        aggregates[job_id] = AggregateEntry(
            job_id=job_id,
            expression=entry.group(2),
            regions=gate_regions,
            reason=entry.group(3),
        )
    if not aggregates:
        invalid_value("required PLAN_JSON block has no aggregate jobs map")
    return region_outputs, aggregates


def parse_result_arguments(block: str) -> list[str]:
    arguments: list[str] = []
    for entry in RESULT_ARG_RE.finditer(block):
        if entry.group(1) != entry.group(2):
            invalid_value(
                "required result argument does not match "
                f"its job result: {entry.group(1)!r}"
            )
        if entry.group(1) in arguments:
            invalid_value(f"duplicate required result argument: {entry.group(1)!r}")
        arguments.append(entry.group(1))
    if not arguments:
        invalid_value("required job passes no result arguments to ci-aggregate")
    return arguments


def build_fixtures(jobs: dict[str, JobEntry]) -> list[FixtureEntry]:
    ordered = list(jobs)
    fixtures: list[FixtureEntry] = []
    for name, paths in FIXTURE_PATHS:
        classification = classify_paths(paths)
        regions = tuple(sorted(region.value for region in classification.applicable))
        region_set = frozenset(regions)
        applicable = tuple(
            job_id
            for job_id in ordered
            if job_id == ALWAYS_JOB
            or (
                bool(jobs[job_id].gates) and region_set.intersection(jobs[job_id].gates)
            )
        )
        if ALWAYS_JOB not in applicable:
            invalid_value("workflow has no always-required fast job")
        fixtures.append(
            FixtureEntry(
                name=name,
                changed_paths=tuple(paths),
                regions=regions,
                applicable_jobs=applicable,
            )
        )
    return fixtures


def build_report(text: str, workflow_label: str) -> JsonObject:
    blocks = split_job_blocks(text)
    jobs = parse_jobs(blocks)
    if PLANNER_JOB not in jobs:
        invalid_value("workflow has no plan job")
    if ALWAYS_JOB not in jobs:
        invalid_value("workflow has no always-required fast job")
    plan_outputs = parse_plan_outputs(blocks[PLANNER_JOB])
    if "required" not in jobs:
        invalid_value("workflow has no required aggregate job")
    required_block = blocks["required"]
    name_match = JOB_NAME_RE.search(required_block)
    if name_match is None:
        invalid_value("required job has no display name")
    needs = parse_required_needs(required_block)
    region_outputs, aggregates = parse_plan_json(required_block)
    result_arguments = parse_result_arguments(required_block)
    for job_id in aggregates:
        if job_id not in jobs:
            invalid_value(f"aggregate {job_id!r} is absent from workflow jobs")
    for job_id in needs:
        if job_id != PLANNER_JOB and job_id not in aggregates:
            invalid_value(f"required.needs {job_id!r} has no aggregate entry")
    for job_id in aggregates:
        if job_id not in needs:
            invalid_value(f"aggregate {job_id!r} has no required.needs entry")
    for job_id in result_arguments:
        if job_id not in aggregates:
            invalid_value(f"required result argument {job_id!r} has no aggregate entry")
    for job_id in aggregates:
        if job_id not in result_arguments:
            invalid_value(f"aggregate {job_id!r} has no required result argument")
    for key in region_outputs:
        if key not in plan_outputs:
            invalid_value(f"region output {key!r} is missing from plan outputs")
    region_names = [region.value for region in REGIONS]
    for region in REGIONS:
        if region.value not in region_outputs:
            invalid_value(
                f"region {region.value!r} is missing from required region outputs"
            )
    job_reports: dict[str, object] = {}
    for job_id, entry in jobs.items():
        job_report: JobReport = {
            "name": entry.name,
            "gates": list(entry.gates),
            "condition_raw": entry.condition_raw,
        }
        job_reports[job_id] = job_report
    aggregate_reports: dict[str, object] = {}
    for job_id, entry in aggregates.items():
        aggregate_report: AggregateReport = {
            "applicable_expression": entry.expression,
            "regions": list(entry.regions),
            "reason": entry.reason,
        }
        aggregate_reports[job_id] = aggregate_report
    required_report: RequiredReport = {
        "check_name": name_match.group(1),
        "needs": needs,
        "region_outputs": sorted(region_outputs),
        "aggregates": cast("dict[str, AggregateReport]", aggregate_reports),
        "result_arguments": result_arguments,
    }
    fixture_reports: list[object] = []
    for fixture in build_fixtures(jobs):
        fixture_report: FixtureReport = {
            "name": fixture.name,
            "changed_paths": list(fixture.changed_paths),
            "regions": list(fixture.regions),
            "applicable_jobs": list(fixture.applicable_jobs),
        }
        fixture_reports.append(fixture_report)
    region_outputs_list = [key for key in sorted(plan_outputs) if key in region_names]
    report: JsonObject = {}
    report["schema"] = SCHEMA_ID
    report["workflow"] = workflow_label
    report["regions"] = region_names
    report["plan_outputs"] = {
        "regions": region_outputs_list,
        "payload": [key for key in sorted(plan_outputs) if key == "regions_json"],
        "auxiliary": [
            key for key in sorted(plan_outputs) if key == "performance_baseline"
        ],
    }
    report["job_order"] = list(jobs)
    report["jobs"] = job_reports
    report["required"] = required_report
    report["fixtures"] = fixture_reports
    return report


def main(argv: Sequence[str] | None = None) -> int:
    default_root = Path.cwd()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=default_root)
    parser.add_argument("--workflow", type=Path, default=None)
    arguments = parser.parse_args(argv)
    try:
        project_root = cast("Path", arguments.root)
        workflow_arg = cast("Path | None", arguments.workflow)
        workflow_path = (
            workflow_arg
            if workflow_arg is not None
            else project_root / WORKFLOW_RELATIVE
        )
        text = workflow_path.read_text(encoding="utf-8")
        try:
            workflow_label = str(workflow_path.relative_to(project_root))
        except ValueError:
            workflow_label = str(workflow_path)
        report = build_report(text, workflow_label)
    except (OSError, ValueError) as error:
        parser.exit(1, f"ci ownership report failed: {error}\n")
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
