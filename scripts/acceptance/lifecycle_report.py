"""Generate a lifecycle report from the §6 handoff table and compatibility state.

Parses the lifecycle state table in ``docs/ARCHITECTURE_HANDOFF.md`` §6
(5 subjects, four lifecycle columns, and 根拠 evidence),
embeds the compatibility row's actual runtime state from
``compatibility/fixed-results.json`` and ``compatibility/promotions.jsonl``,
and emits one JSON document (schema ``lifecycle-report/1``) to stdout.

Fail-closed: any parse or validation failure exits non-zero with a message.
No network access; standard library only.
"""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from typing import TYPE_CHECKING, Never, TypedDict, cast

if TYPE_CHECKING:
    from collections.abc import Sequence

type JsonObject = dict[str, object]


class LifecycleSubject(TypedDict):
    generate: str
    switch: str
    fail: str
    discard: str
    evidence: list[str]


class CompatibilityState(TypedDict):
    schema: str
    baseline_count: int
    manifest_sha256: str
    results_sha256: str
    promotion_records: int
    promotion_kinds: dict[str, int]


class LifecycleReport(TypedDict):
    schema: str
    subjects: dict[str, LifecycleSubject]
    compatibility_state: CompatibilityState


SCHEMA = "lifecycle-report/1"

SECTION_HEADING = "## 6."
NEXT_HEADING = "## 7."

SUBJECT_KEYS = ("settings", "profile", "command", "dynamic", "compatibility")
COLUMN_KEYS = ("generate", "switch", "fail", "discard")

_LINK_TARGET = re.compile(r"\]\(([^)\s]+)\)")
_MARKDOWN_LINK = re.compile(r"\[[^\]]*\]\([^)]+\)")
_BACKTICK_PATH = re.compile(r"`([^`]+)`")
_LINE_REF = re.compile(r":\d[\d,.-]*$")


def invalid_value(message: str) -> Never:
    raise ValueError(message)


def mapping(value: object, label: str) -> JsonObject:
    if not isinstance(value, dict):
        invalid_value(f"{label} must be an object")
    untyped = cast("dict[object, object]", value)
    return {cast("str", key): item for key, item in untyped.items()}


def _section(text: str, heading: str, next_heading: str) -> str:
    try:
        start = text.index(heading)
    except ValueError:
        invalid_value(f"handoff is missing section heading: {heading}")
    try:
        end = text.index(next_heading, start + len(heading))
    except ValueError:
        invalid_value(f"handoff is missing section heading: {next_heading}")
    return text[start:end]


def _table_rows(section: str) -> list[str]:
    return [
        line
        for line in section.splitlines()
        if line.startswith("|") and not line.startswith("|---")
    ]


def _is_separator(row: str) -> bool:
    return all(
        re.fullmatch(r":?-{3,}:?", cell) is not None for cell in _split_cells(row)
    )


def _split_cells(row: str) -> list[str]:
    return [cell.strip() for cell in row.strip().strip("|").split("|")]


def subject_key(label: str) -> str:
    name = label.strip()
    if name == "settings":
        return "settings"
    if name == "profile":
        return "profile"
    if name == "command":
        return "command"
    if name == "dynamic config":
        return "dynamic"
    if name == "compatibility corpus":
        return "compatibility"
    invalid_value(f"unknown lifecycle subject: {label!r}")


def normalize_evidence_path(raw: str, *, docs_relative: bool = False) -> str:
    """Strip anchors and line refs; resolve docs-relative markdown links."""
    path = raw.strip()
    if "#" in path and not path.startswith("#"):
        path = path.split("#", 1)[0]
    path = _LINE_REF.sub("", path)
    if docs_relative and "/" not in path and not path.startswith("."):
        # Bare filename from a docs-relative markdown link (e.g. ARCHITECTURE.md).
        path = f"docs/{path}"
    return path


def extract_evidence(cell: str) -> list[str]:
    found = [
        normalize_evidence_path(match.group(1), docs_relative=True)
        for match in _LINK_TARGET.finditer(cell)
    ]
    # Strip markdown links first so backticked link display text
    # (e.g. [`ARCHITECTURE.md:114-132`](...)) is not double-counted.
    remainder = _MARKDOWN_LINK.sub("", cell)
    for match in _BACKTICK_PATH.finditer(remainder):
        candidate = normalize_evidence_path(match.group(1).strip())
        basename = candidate.rsplit("/", 1)[-1]
        if "/" in candidate and "." not in basename:
            continue  # e.g. `Data/Commands`: a domain term, not a repo path.
        if "/" in candidate or candidate.endswith(
            (".nix", ".md", ".json", ".rs", ".py")
        ):
            found.append(candidate)
    ordered = list(dict.fromkeys(found))
    if not ordered:
        invalid_value(f"lifecycle row has no extractable evidence: {cell!r}")
    return ordered


def parse_lifecycle_table(handoff_text: str) -> dict[str, LifecycleSubject]:
    section = _section(handoff_text, SECTION_HEADING, NEXT_HEADING)
    rows = [row for row in _table_rows(section) if not _is_separator(row)]
    if len(rows) < 2:
        invalid_value("lifecycle section has no table")
    subjects: dict[str, LifecycleSubject] = {}
    for row in rows[1:]:  # Skip the header row.
        cells = _split_cells(row)
        if len(cells) != 6:
            invalid_value(f"lifecycle row must have 6 cells: {row!r}")
        label, generate, switch, fail, discard, evidence_cell = cells
        key = subject_key(label)
        if key in subjects:
            invalid_value(f"duplicate lifecycle subject: {label!r}")
        columns: dict[str, str] = {
            "generate": generate,
            "switch": switch,
            "fail": fail,
            "discard": discard,
        }
        for column, value in columns.items():
            if not value:
                invalid_value(
                    f"lifecycle subject {label!r} has an empty column: {column}"
                )
        subjects[key] = LifecycleSubject(
            generate=generate,
            switch=switch,
            fail=fail,
            discard=discard,
            evidence=extract_evidence(evidence_cell),
        )
    missing = [key for key in SUBJECT_KEYS if key not in subjects]
    if missing:
        invalid_value(f"lifecycle table is missing subjects: {', '.join(missing)}")
    extra = [key for key in subjects if key not in SUBJECT_KEYS]
    if extra:
        invalid_value(f"lifecycle table has unexpected subjects: {', '.join(extra)}")
    return subjects


def load_compatibility_state_from_paths(
    results_path: Path, promotions_path: Path
) -> CompatibilityState:
    try:
        raw_results: object = json.loads(results_path.read_text(encoding="utf-8"))
    except OSError as error:
        invalid_value(f"cannot read compatibility results: {error}")
    results = mapping(raw_results, "compatibility/fixed-results.json")
    summary = mapping(
        results.get("summary"), "compatibility/fixed-results.json summary"
    )
    baseline_count = summary.get("baseline_count")
    if not isinstance(baseline_count, int):
        invalid_value("compatibility summary baseline_count must be an integer")
    manifest_sha256 = results.get("manifest_sha256")
    results_sha256 = results.get("results_sha256")
    if not isinstance(manifest_sha256, str) or not manifest_sha256:
        invalid_value("compatibility/fixed-results.json manifest_sha256 must be set")
    if not isinstance(results_sha256, str) or not results_sha256:
        invalid_value("compatibility/fixed-results.json results_sha256 must be set")
    schema_version = results.get("schema_version")
    if not isinstance(schema_version, int):
        invalid_value(
            "compatibility/fixed-results.json schema_version must be an integer"
        )

    counts_by_kind: dict[str, int] = {}
    record_count = 0
    try:
        promotions_text = promotions_path.read_text(encoding="utf-8")
    except OSError as error:
        invalid_value(f"cannot read compatibility promotions: {error}")
    for lineno, line in enumerate(promotions_text.splitlines(), start=1):
        if not line.strip():
            continue
        try:
            raw_record: object = json.loads(line)
        except json.JSONDecodeError as error:
            invalid_value(f"promotions.jsonl line {lineno} is not valid JSON: {error}")
        record = mapping(raw_record, f"promotions.jsonl line {lineno}")
        kind = record.get("kind")
        if not isinstance(kind, str) or not kind:
            invalid_value(
                f"promotions.jsonl line {lineno} kind must be a non-empty string"
            )
        counts_by_kind[kind] = counts_by_kind.get(kind, 0) + 1
        record_count += 1
    return CompatibilityState(
        schema=f"compatibility-report/{schema_version}",
        baseline_count=baseline_count,
        manifest_sha256=manifest_sha256,
        results_sha256=results_sha256,
        promotion_records=record_count,
        promotion_kinds=counts_by_kind,
    )


def load_compatibility_state(root: Path) -> CompatibilityState:
    return load_compatibility_state_from_paths(
        root / "compatibility/fixed-results.json",
        root / "compatibility/promotions.jsonl",
    )


def build_report(root: Path, handoff_text: str) -> LifecycleReport:
    subjects = parse_lifecycle_table(handoff_text)
    compatibility_state = load_compatibility_state(root)
    return LifecycleReport(
        schema=SCHEMA,
        subjects=subjects,
        compatibility_state=compatibility_state,
    )


def main(argv: Sequence[str] | None = None) -> int:
    root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--handoff", type=Path, default=root / "docs/ARCHITECTURE_HANDOFF.md"
    )
    parser.add_argument(
        "--fixed-results",
        type=Path,
        default=root / "compatibility/fixed-results.json",
    )
    parser.add_argument(
        "--promotions",
        type=Path,
        default=root / "compatibility/promotions.jsonl",
    )
    arguments = parser.parse_args(argv)
    try:
        handoff_path = arguments.handoff
        try:
            handoff_text = handoff_path.read_text(encoding="utf-8")
        except OSError as error:
            invalid_value(f"cannot read handoff: {error}")
        subjects = parse_lifecycle_table(handoff_text)
        compatibility_state = load_compatibility_state_from_paths(
            Path(arguments.fixed_results), Path(arguments.promotions)
        )
        report: LifecycleReport = {
            "schema": SCHEMA,
            "subjects": subjects,
            "compatibility_state": compatibility_state,
        }
    except (OSError, ValueError) as error:
        parser.exit(1, f"lifecycle report failed: {error}\n")
    print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
