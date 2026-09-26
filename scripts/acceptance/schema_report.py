"""Machine-readable sha256 report for handoff section 5.1 surfaces.

Parses the public-surface inventory table from docs/ARCHITECTURE_HANDOFF.md
(section "### 5.1" up to the next "###" heading), extracts the canonical
repo-relative paths named by each row, and records a sha256 digest per path
so contract drift is detectable from a single JSON artifact.
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

SCHEMA_ID = "schema-report/1"
SECTION_HEADING = "### 5.1"
BACKTICK_PATTERN = re.compile(r"`([^`]*)`")
LINE_REF_PATTERN = re.compile(r"\A(.+?):\d.*\Z")
NORMALIZE_PATTERN = re.compile(r"[^a-z0-9]+")
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


@dataclass(frozen=True)
class SurfaceEntry:
    label: str
    paths: tuple[str, ...]


def normalize_surface_key(label: str) -> str:
    key = NORMALIZE_PATTERN.sub("_", label.lower()).strip("_")
    if not key:
        invalid_value(f"surface label has no alphanumeric content: {label!r}")
    return key


def strip_line_ref(span: str) -> str:
    match = LINE_REF_PATTERN.match(span)
    if match is None:
        return span
    return match.group(1)


def expand_braces(span: str) -> list[str]:
    start = span.find("{")
    end = span.find("}", start + 1) if start >= 0 else -1
    if start < 0 or end < 0:
        return [span]
    alternatives = span[start + 1 : end].split(",")
    if len(alternatives) < 2 or any(not part for part in alternatives):
        return [span]
    prefix = span[:start]
    suffix = span[end + 1 :]
    expanded: list[str] = []
    for part in alternatives:
        expanded.extend(expand_braces(prefix + part + suffix))
    return expanded


def is_canonical_path(span: str) -> bool:
    if "/" not in span or span.startswith(("/", ".")):
        return False
    base = span.rsplit("/", 1)[-1]
    return "." in base and not base.startswith(".") and not base.endswith(".")


def extract_paths(cells: Sequence[str]) -> tuple[str, ...]:
    collected: list[str] = []
    for cell in cells:
        for span in BACKTICK_PATTERN.findall(cell):
            candidate = strip_line_ref(span.strip())
            for expanded in expand_braces(candidate):
                if is_canonical_path(expanded) and expanded not in collected:
                    collected.append(expanded)
    return tuple(collected)


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


def parse_section(text: str) -> dict[str, SurfaceEntry]:
    start = text.find(SECTION_HEADING)
    if start < 0:
        invalid_value("handoff section 5.1 heading not found")
    rest = text[start + len(SECTION_HEADING) :]
    end = rest.find("\n###")
    section = rest[:end] if end >= 0 else rest
    rows = [line for line in section.splitlines() if line.startswith("|")]
    if not rows:
        invalid_value("handoff section 5.1 contains no table rows")
    entries: dict[str, SurfaceEntry] = {}
    for index, line in enumerate(rows):
        if index == 0 or is_separator_row(split_row(line)):
            continue
        cells = split_row(line)
        if len(cells) != 4:
            invalid_value(f"section 5.1 row must have 4 cells: {line!r}")
        label = cells[0]
        if not label:
            invalid_value(f"section 5.1 row has an empty label: {line!r}")
        key = normalize_surface_key(label)
        if key in entries:
            invalid_value(f"duplicate section 5.1 surface: {label!r}")
        entries[key] = SurfaceEntry(label=label, paths=extract_paths(cells[1:]))
    return entries


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def build_report(root: Path, handoff: Path) -> JsonObject:
    entries = parse_section(handoff.read_text(encoding="utf-8"))
    surfaces: dict[str, object] = {}
    for key, entry in entries.items():
        if not entry.paths:
            invalid_value(f"surface has no canonical paths: {entry.label!r}")
        digests: dict[str, str] = {}
        for relative in entry.paths:
            candidate = root / relative
            if not candidate.is_file():
                invalid_value(
                    f"surface {entry.label!r} names a missing path: {relative}"
                )
            digests[relative] = sha256_file(candidate)
        surface: JsonObject = {}
        surface["label"] = entry.label
        surface["paths"] = digests
        surfaces[key] = surface
    report: JsonObject = {}
    report["schema"] = SCHEMA_ID
    report["surfaces"] = surfaces
    report["surface_count"] = len(surfaces)
    return report


def load_baseline(path: Path) -> JsonObject:
    raw: object = json.loads(path.read_text(encoding="utf-8"))
    return mapping(raw, str(path))


def report_digests(report: JsonObject, label: str) -> dict[str, dict[str, str]]:
    surfaces = mapping(report.get("surfaces"), f"{label} surfaces")
    digests: dict[str, dict[str, str]] = {}
    for key, entry in surfaces.items():
        entry_map = mapping(entry, f"{label} surface {key!r}")
        paths = mapping(entry_map.get("paths"), f"{label} surface {key!r} paths")
        table: dict[str, str] = {}
        for name, digest in paths.items():
            if not isinstance(digest, str) or not digest:
                invalid_value(
                    f"{label} surface {key!r} path {name!r} must map to a digest string"
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
            drift.append(f"missing surface: {key}")
            continue
        for name, digest in expected.items():
            if name not in actual:
                drift.append(f"missing path: {name}")
            elif actual[name] != digest:
                drift.append(f"drifted path: {name}")
    for key, actual in current.items():
        expected = baseline.get(key)
        if expected is None:
            drift.append(f"unexpected surface: {key}")
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
    arguments = parser.parse_args(argv)
    try:
        project_root = cast("Path", arguments.root)
        handoff_arg = cast("Path | None", arguments.handoff)
        baseline_arg = cast("Path | None", arguments.baseline)
        handoff_path = (
            handoff_arg
            if handoff_arg is not None
            else project_root / "docs/ARCHITECTURE_HANDOFF.md"
        )
        report = build_report(project_root, handoff_path)
        if baseline_arg is not None:
            baseline = load_baseline(baseline_arg)
            if baseline.get("schema") != SCHEMA_ID:
                invalid_value(f"baseline {baseline_arg} uses an unexpected schema")
            drift = find_drift(
                report_digests(report, "schema report"),
                report_digests(baseline, f"baseline {baseline_arg}"),
            )
            if drift:
                invalid_value("schema drift detected: " + "; ".join(drift))
    except (OSError, ValueError) as error:
        parser.exit(1, f"schema report failed: {error}\n")
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
