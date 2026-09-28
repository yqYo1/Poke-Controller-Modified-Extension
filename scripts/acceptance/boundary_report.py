"""Machine-readable boundary/abstraction report for handoff section 5.3.

Parses the strong-boundary justification table from docs/ARCHITECTURE_HANDOFF.md
(section "### 5.3" up to the next "###" heading), pins it against
rust/pokecon/registry/strong_boundaries.json and (with --abstraction)
rust/pokecon/registry/abstraction_baseline.json, and emits one JSON document
(schema "boundary-report/1", or "abstraction-report/1" with --abstraction)
to stdout.

Fail-closed: any parse or validation failure exits non-zero with a message.
No network access; standard library only.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Never, cast

if TYPE_CHECKING:
    from collections.abc import Sequence

type JsonObject = dict[str, object]

SCHEMA_ID = "boundary-report/1"
ABSTRACTION_SCHEMA = "abstraction-report/1"
SECTION_HEADING = "### 5.3"
BACKTICK_PATTERN = re.compile(r"`([^`]*)`")
LINE_REF_PATTERN = re.compile(r"\A(.+?):\d.*\Z")
SEPARATOR_CELL_PATTERN = re.compile(r"\A:?-{3,}:?\Z")


def invalid_value(message: str) -> Never:
    raise ValueError(message)


def mapping(value: object, label: str) -> JsonObject:
    if not isinstance(value, dict):
        invalid_value(f"{label} must be an object")
    untyped = cast("dict[object, object]", value)
    if not all(isinstance(key, str) for key in untyped):
        invalid_value(f"{label} must use string keys")
    return {cast("str", key): item for key, item in untyped.items()}


def as_list(value: object, label: str) -> list[object]:
    if not isinstance(value, list):
        invalid_value(f"{label} must be an array")
    return cast("list[object]", value)


def as_str_list(value: object, label: str) -> list[str]:
    items = as_list(value, label)
    for item in items:
        if not isinstance(item, str):
            invalid_value(f"{label} must list strings")
    return cast("list[str]", items)


@dataclass(frozen=True)
class BoundaryRow:
    boundary_id: str
    necessity: str
    evidence_paths: tuple[str, ...]


def strip_line_ref(span: str) -> str:
    match = LINE_REF_PATTERN.match(span)
    if match is None:
        return span
    return match.group(1)


def split_row(line: str) -> list[str]:
    cells = [cell.strip() for cell in line.split("|")]
    if cells and not cells[0]:
        cells = cells[1:]
    if cells and not cells[-1]:
        cells = cells[:-1]
    return cells


def is_separator_row(cells: Sequence[str]) -> bool:
    return bool(cells) and all(
        SEPARATOR_CELL_PATTERN.match(cell) is not None for cell in cells
    )


def parse_section(text: str) -> dict[str, BoundaryRow]:
    start = text.find(SECTION_HEADING)
    if start < 0:
        invalid_value("handoff section 5.3 heading not found")
    rest = text[start + len(SECTION_HEADING) :]
    cuts = [
        rest.find(marker) for marker in ("\n### ", "\n## ") if rest.find(marker) >= 0
    ]
    end = min(cuts) if cuts else -1
    section = rest[:end] if end >= 0 else rest
    rows = [line for line in section.splitlines() if line.startswith("|")]
    if not rows:
        invalid_value("handoff section 5.3 contains no table rows")
    entries: dict[str, BoundaryRow] = {}
    for index, line in enumerate(rows):
        cells = split_row(line)
        if index == 0 or is_separator_row(cells):
            continue
        if len(cells) != 3:
            invalid_value(f"section 5.3 row must have 3 cells: {line!r}")
        spans = BACKTICK_PATTERN.findall(cells[0])
        if len(spans) != 1:
            invalid_value(f"section 5.3 row must name one boundary id: {line!r}")
        boundary_id = spans[0].strip()
        if not boundary_id:
            invalid_value(f"section 5.3 row has an empty boundary id: {line!r}")
        if boundary_id in entries:
            invalid_value(f"duplicate section 5.3 boundary: {boundary_id!r}")
        necessity = cells[1].strip()
        if not necessity or "\n" in necessity:
            invalid_value(f"section 5.3 row must carry a one-line necessity: {line!r}")
        evidence = tuple(
            strip_line_ref(span.strip()) for span in BACKTICK_PATTERN.findall(cells[2])
        )
        if not evidence:
            invalid_value(f"section 5.3 row must cite evidence paths: {line!r}")
        entries[boundary_id] = BoundaryRow(
            boundary_id=boundary_id, necessity=necessity, evidence_paths=evidence
        )
    return entries


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def split_anchor(anchor: str) -> tuple[str, int | None]:
    head, separator, tail = anchor.rpartition(":")
    if not separator:
        return anchor, None
    first = tail.split("-", 1)[0]
    if first.isdigit():
        return head, int(first)
    return anchor, None


def load_registry(root: Path, relative: str) -> JsonObject:
    path = root / relative
    try:
        raw: object = json.loads(path.read_text(encoding="utf-8"))
    except OSError as error:
        invalid_value(f"registry {relative} must be readable: {error}")
    return mapping(raw, relative)


def check_ref_is_live(name: str, contract_sync: str, flake: str) -> bool:
    return f"fn {name}" in contract_sync or name in flake


def validate_boundaries(
    root: Path, registry: JsonObject, rows: dict[str, BoundaryRow]
) -> JsonObject:
    if registry.get("schema_version") != 1:
        invalid_value("strong boundary registry schema_version must be 1")
    raw_boundaries = as_list(registry.get("boundaries"), "strong boundary registry")
    if not raw_boundaries:
        invalid_value(
            "strong boundary registry must define a non-empty boundaries array"
        )
    contract_sync = (root / "rust/pokecon/tests/contract_sync.rs").read_text(
        encoding="utf-8"
    )
    flake = (root / "flake.nix").read_text(encoding="utf-8")
    boundaries: dict[str, object] = {}
    seen: set[str] = set()
    for entry in raw_boundaries:
        item = mapping(entry, "strong boundary entry")
        boundary_id = item.get("id")
        if not isinstance(boundary_id, str) or not boundary_id:
            invalid_value("strong boundary entry must have a non-empty string id")
        if boundary_id in seen:
            invalid_value(f"duplicate strong boundary id: {boundary_id!r}")
        seen.add(boundary_id)
        for field in ("kind", "necessity", "trust", "distribution"):
            text = item.get(field)
            if not isinstance(text, str) or not text.strip() or "\n" in text:
                invalid_value(
                    f"boundary {boundary_id} field {field} must be one non-empty line"
                )
        row = rows.get(boundary_id)
        if row is None:
            invalid_value(f"boundary {boundary_id} is missing from handoff 5.3")
        evidence = mapping(item.get("evidence"), f"boundary {boundary_id} evidence")
        raw_refs = as_list(
            evidence.get("source_refs"), f"boundary {boundary_id} evidence"
        )
        if not raw_refs:
            invalid_value(f"boundary {boundary_id} must list evidence.source_refs")
        digests: dict[str, str] = {}
        texts: list[str] = []
        for ref in raw_refs:
            if not isinstance(ref, str) or not ref:
                invalid_value(
                    f"boundary {boundary_id} source_ref must be a non-empty string"
                )
            path, line = split_anchor(ref)
            candidate = root / path
            if not candidate.is_file():
                invalid_value(f"boundary {boundary_id} names a missing path: {path}")
            text = candidate.read_text(encoding="utf-8")
            texts.append(text)
            if line is not None:
                total = len(text.splitlines())
                if line < 1 or line > total:
                    invalid_value(
                        f"boundary {boundary_id} anchor {ref} is out of range "
                        f"(1-{total})"
                    )
            digests[path] = sha256_file(candidate)
        joined = "\n".join(texts)
        raw_symbols = as_list(
            evidence.get("symbol_anchors"), f"boundary {boundary_id} evidence"
        )
        if not raw_symbols:
            invalid_value(f"boundary {boundary_id} must list evidence.symbol_anchors")
        for symbol in raw_symbols:
            if not isinstance(symbol, str) or not symbol.strip():
                invalid_value(f"boundary {boundary_id} symbol_anchor must be non-empty")
            if symbol not in joined:
                invalid_value(
                    f"symbol anchor {symbol!r} of boundary {boundary_id} "
                    "must occur in its evidence sources"
                )
        raw_registries = as_list(
            evidence.get("registry_refs"), f"boundary {boundary_id} evidence"
        )
        for ref in raw_registries:
            if not isinstance(ref, str) or not (root / ref).is_file():
                invalid_value(
                    f"boundary {boundary_id} names a missing registry: {ref!r}"
                )
        raw_checks = as_list(
            evidence.get("check_refs"), f"boundary {boundary_id} evidence"
        )
        for name in raw_checks:
            if not isinstance(name, str) or not check_ref_is_live(
                name, contract_sync, flake
            ):
                invalid_value(
                    f"check ref {name!r} of boundary {boundary_id} must name "
                    "a contract_sync test or a flake task"
                )
        boundary: JsonObject = {}
        boundary["kind"] = item["kind"]
        boundary["necessity"] = item["necessity"]
        boundary["trust"] = item["trust"]
        boundary["distribution"] = item["distribution"]
        boundary["handoff_necessity"] = row.necessity
        boundary["paths"] = digests
        boundaries[boundary_id] = boundary
    missing = [key for key in rows if key not in seen]
    if missing:
        invalid_value(f"handoff 5.3 boundaries missing from registry: {missing}")
    raw_non = as_list(registry.get("non_boundaries"), "strong boundary registry")
    if not raw_non:
        invalid_value("strong boundary registry must define a non_boundaries array")
    non_boundaries = sorted(item for item in raw_non if isinstance(item, str) and item)
    if len(non_boundaries) != len(raw_non):
        invalid_value("non_boundaries must list non-empty strings")
    return {"boundaries": boundaries, "non_boundaries": non_boundaries}


def build_report(root: Path, handoff: Path) -> JsonObject:
    rows = parse_section(handoff.read_text(encoding="utf-8"))
    registry = load_registry(root, "rust/pokecon/registry/strong_boundaries.json")
    validated = validate_boundaries(root, registry, rows)
    report: JsonObject = {}
    report["schema"] = SCHEMA_ID
    report["boundaries"] = validated["boundaries"]
    report["non_boundaries"] = validated["non_boundaries"]
    report["boundary_count"] = len(
        cast("dict[object, object]", validated["boundaries"])
    )
    return report


def build_abstraction_report(root: Path) -> JsonObject:
    registry = load_registry(root, "rust/pokecon/registry/abstraction_baseline.json")
    if registry.get("schema_version") != 1:
        invalid_value("abstraction baseline schema_version must be 1")
    members = registry.get("workspace_members")
    if members != ["rust/pokecon"]:
        invalid_value('abstraction baseline workspace_members must be ["rust/pokecon"]')
    dep_names = as_str_list(
        registry.get("workspace_dependency_names"), "abstraction baseline"
    )
    if not dep_names or sorted(dep_names) != dep_names:
        invalid_value("workspace_dependency_names must be a sorted non-empty name list")
    raw_traits = as_list(registry.get("traits"), "abstraction baseline")
    if not raw_traits:
        invalid_value("abstraction baseline must define a non-empty traits array")
    for entry in raw_traits:
        item = mapping(entry, "abstraction trait entry")
        for field in ("path", "name", "justification"):
            text = item.get(field)
            if not isinstance(text, str) or not text.strip() or "\n" in (text):
                invalid_value(f"trait entry field {field} must be one non-empty line")
        trait_path = cast("str", item["path"])
        if not (root / trait_path).is_file():
            invalid_value(f"trait entry names a missing path: {trait_path}")
    raw_services = as_list(registry.get("services"), "abstraction baseline")
    if not raw_services:
        invalid_value("abstraction baseline must define a non-empty services array")
    for entry in raw_services:
        item = mapping(entry, "abstraction service entry")
        service_path = item.get("path")
        if not isinstance(service_path, str) or not (root / service_path).is_file():
            invalid_value(f"service entry names a missing path: {service_path!r}")
    raw_bins = as_list(registry.get("bins"), "abstraction baseline")
    if not raw_bins:
        invalid_value("abstraction baseline must define a non-empty bins array")
    strong = load_registry(root, "rust/pokecon/registry/strong_boundaries.json")
    strong_ids: set[str] = set()
    for entry in as_list(strong.get("boundaries"), "strong boundary registry"):
        strong_id = mapping(entry, "strong boundary entry").get("id")
        if not isinstance(strong_id, str) or not strong_id:
            invalid_value("strong boundary entry must have a non-empty string id")
        strong_ids.add(strong_id)
    for entry in raw_bins:
        item = mapping(entry, "abstraction bin entry")
        bin_path = item.get("path")
        if not isinstance(bin_path, str) or not (root / bin_path).is_file():
            invalid_value(f"bin entry names a missing path: {bin_path!r}")
        ref = item.get("boundary_ref")
        if ref is not None and ref not in strong_ids:
            invalid_value(f"bin boundary_ref {ref!r} must name a strong boundary id")
    raw_conversions = as_list(registry.get("conversion_files"), "abstraction baseline")
    if not raw_conversions:
        invalid_value(
            "abstraction baseline must define a non-empty conversion_files array"
        )
    report: JsonObject = {}
    report["schema"] = ABSTRACTION_SCHEMA
    report["member_count"] = 1
    report["dependency_count"] = len(dep_names)
    report["trait_count"] = len(raw_traits)
    report["service_count"] = len(raw_services)
    report["bin_count"] = len(raw_bins)
    report["conversion_file_count"] = len(raw_conversions)
    report["dependencies"] = sorted(dep_names)
    return report


def load_baseline(path: Path) -> JsonObject:
    raw: object = json.loads(path.read_text(encoding="utf-8"))
    return mapping(raw, str(path))


def report_digests(report: JsonObject, label: str) -> dict[str, dict[str, str]]:
    boundaries = mapping(report.get("boundaries"), f"{label} boundaries")
    digests: dict[str, dict[str, str]] = {}
    for key, entry in boundaries.items():
        entry_map = mapping(entry, f"{label} boundary {key!r}")
        paths = mapping(entry_map.get("paths"), f"{label} boundary {key!r} paths")
        table: dict[str, str] = {}
        for name, digest in paths.items():
            if not isinstance(digest, str) or not digest:
                invalid_value(
                    f"{label} boundary {key!r} path {name!r} must map to a digest string"
                )
            table[name] = digest
        digests[key] = table
    return digests


def find_drift(
    current: dict[str, dict[str, str]], baseline: dict[str, dict[str, str]]
) -> list[str]:
    drift: list[str] = []
    for key, expected in baseline.items():
        actual = current.get(key)
        if actual is None:
            drift.append(f"missing boundary: {key}")
            continue
        for name, digest in expected.items():
            if name not in actual:
                drift.append(f"missing path: {name}")
            elif actual[name] != digest:
                drift.append(f"drifted path: {name}")
    for key, actual in current.items():
        expected = baseline.get(key)
        if expected is None:
            drift.append(f"unexpected boundary: {key}")
            continue
        drift.extend(
            f"unexpected path: {name}" for name in actual if name not in expected
        )
    return drift


def main(argv: Sequence[str] | None = None) -> int:
    root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=root)
    parser.add_argument("--handoff", type=Path, default=None)
    parser.add_argument("--baseline", type=Path, default=None)
    parser.add_argument("--abstraction", action="store_true")
    parser.add_argument("--check", action="store_true")
    arguments = parser.parse_args(argv)
    try:
        project_root = cast("Path", arguments.root)
        handoff_arg = cast("Path | None", arguments.handoff)
        baseline_arg = cast("Path | None", arguments.baseline)
        want_abstraction = cast("bool", arguments.abstraction)
        handoff_path = (
            handoff_arg
            if handoff_arg is not None
            else project_root / "docs/ARCHITECTURE_HANDOFF.md"
        )
        if want_abstraction:
            report = build_abstraction_report(project_root)
            expected_schema = ABSTRACTION_SCHEMA
        else:
            report = build_report(project_root, handoff_path)
            expected_schema = SCHEMA_ID
            if cast("bool", arguments.check):
                # --check additionally validates the abstraction side so one
                # invocation covers both registries like the contract_sync
                # tests do.
                build_abstraction_report(project_root)
        if baseline_arg is not None:
            if want_abstraction:
                invalid_value("--baseline drift comparison needs the boundary report")
            baseline = load_baseline(baseline_arg)
            if baseline.get("schema") != SCHEMA_ID:
                invalid_value(f"baseline {baseline_arg} uses an unexpected schema")
            drift = find_drift(
                report_digests(report, "boundary report"),
                report_digests(baseline, f"baseline {baseline_arg}"),
            )
            if drift:
                invalid_value("boundary drift detected: " + "; ".join(drift))
        if report.get("schema") != expected_schema:
            invalid_value("boundary report carries an unexpected schema")
    except (OSError, ValueError) as error:
        parser.exit(1, f"boundary report failed: {error}\n")
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
