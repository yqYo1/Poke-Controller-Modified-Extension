"""Validate acceptance records and the complete external release matrix."""

from __future__ import annotations

import argparse
import json
import subprocess
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import TYPE_CHECKING, Never, cast

if TYPE_CHECKING:
    from collections.abc import Sequence

type JsonObject = dict[str, object]


@dataclass(frozen=True)
class CapabilityContract:
    identifier: str
    required_steps: tuple[str, ...]
    release_required: bool
    matrix_per_browser: bool


@dataclass(frozen=True)
class AcceptanceContract:
    specification_version: str
    platforms: tuple[str, ...]
    browsers: tuple[str, ...]
    capabilities: dict[str, CapabilityContract]


def invalid_value(message: str) -> Never:
    raise ValueError(message)


def mapping(value: object, label: str) -> JsonObject:
    if not isinstance(value, dict):
        invalid_value(f"{label} must be an object")
    untyped = cast("dict[object, object]", value)
    if not all(isinstance(key, str) for key in untyped):
        invalid_value(f"{label} must use string keys")
    return {cast("str", key): item for key, item in untyped.items()}


def objects(value: object, label: str) -> list[JsonObject]:
    if not isinstance(value, list):
        invalid_value(f"{label} must be an array")
    items = cast("list[object]", value)
    return [mapping(item, f"{label}[{index}]") for index, item in enumerate(items)]


def string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        invalid_value(f"{label} must be a non-empty string")
    return value


def strings(value: object, label: str) -> tuple[str, ...]:
    if not isinstance(value, list):
        invalid_value(f"{label} must be an array")
    items = cast("list[object]", value)
    result = tuple(
        string(item, f"{label}[{index}]") for index, item in enumerate(items)
    )
    if len(result) != len(set(result)):
        invalid_value(f"{label} must not contain duplicates")
    return result


def boolean(value: object, label: str) -> bool:
    if not isinstance(value, bool):
        invalid_value(f"{label} must be a boolean")
    return value


def commit_sha(value: object, label: str) -> str:
    commit = string(value, label)
    if len(commit) != 40 or any(
        character not in "0123456789abcdef" for character in commit
    ):
        invalid_value(f"{label} must be a 40-character lowercase hexadecimal commit")
    return commit


def load_json(path: Path) -> JsonObject:
    raw: object = json.loads(path.read_text(encoding="utf-8"))
    return mapping(raw, str(path))


def load_contract(schema_path: Path) -> AcceptanceContract:
    schema = load_json(schema_path)
    matrix = mapping(
        schema.get("x-pokecon-release-matrix"),
        "acceptance schema x-pokecon-release-matrix",
    )
    capability_rows = objects(matrix.get("capabilities"), "release matrix capabilities")
    capabilities: dict[str, CapabilityContract] = {}
    for index, row in enumerate(capability_rows):
        label = f"release matrix capabilities[{index}]"
        identifier = string(row.get("id"), f"{label}.id")
        if identifier in capabilities:
            invalid_value(f"duplicate acceptance capability: {identifier}")
        capabilities[identifier] = CapabilityContract(
            identifier=identifier,
            required_steps=strings(
                row.get("required_steps"), f"{label}.required_steps"
            ),
            release_required=boolean(
                row.get("release_required"), f"{label}.release_required"
            ),
            matrix_per_browser=boolean(
                row.get("matrix_per_browser"),
                f"{label}.matrix_per_browser",
            ),
        )
    if not capabilities:
        invalid_value("release matrix capabilities must not be empty")
    return AcceptanceContract(
        specification_version=string(
            matrix.get("specification_version"),
            "release matrix specification_version",
        ),
        platforms=strings(matrix.get("platforms"), "release matrix platforms"),
        browsers=strings(matrix.get("browsers"), "release matrix browsers"),
        capabilities=capabilities,
    )


def parse_timestamp(value: object, label: str) -> datetime:
    timestamp = string(value, label)
    try:
        return datetime.fromisoformat(timestamp.replace("Z", "+00:00"))
    except ValueError as error:
        message = f"{label} is not an ISO 8601 timestamp"
        raise ValueError(message) from error


def validate_passed_record(
    record: JsonObject,
    contract: AcceptanceContract,
    label: str,
) -> None:
    if record.get("result") != "passed":
        return
    if record.get("specification_version") != contract.specification_version:
        invalid_value(f"{label} uses an unexpected specification_version")
    capability_id = string(record.get("capability"), f"{label}.capability")
    capability = contract.capabilities.get(capability_id)
    if capability is None:
        invalid_value(f"{label} uses unknown capability {capability_id!r}")
    step_rows = objects(record.get("steps"), f"{label}.steps")
    actual_steps = tuple(
        string(step.get("id"), f"{label}.steps[{index}].id")
        for index, step in enumerate(step_rows)
    )
    if actual_steps != capability.required_steps:
        invalid_value(
            f"{label} must contain the canonical {capability_id} steps in order: "
            f"{', '.join(capability.required_steps)}"
        )
    for index, step in enumerate(step_rows):
        if step.get("result") != "passed":
            invalid_value(f"{label}.steps[{index}] must be passed")
        evidence = step.get("evidence")
        if not isinstance(evidence, list) or not evidence:
            invalid_value(f"{label}.steps[{index}].evidence must not be empty")
    started_at = parse_timestamp(record.get("started_at"), f"{label}.started_at")
    completed_at = parse_timestamp(record.get("completed_at"), f"{label}.completed_at")
    if completed_at <= started_at:
        invalid_value(f"{label}.completed_at must be later than started_at")


def release_gate_key(
    platform: str,
    capability: str,
    browser: str | None = None,
) -> str:
    suffix = f"/{browser}" if browser is not None else ""
    return f"{platform}/{capability}{suffix}"


def expected_release_gate_keys(contract: AcceptanceContract) -> set[str]:
    expected: set[str] = set()
    for platform in contract.platforms:
        for capability in contract.capabilities.values():
            if not capability.release_required:
                continue
            if capability.matrix_per_browser:
                expected.update(
                    release_gate_key(platform, capability.identifier, browser)
                    for browser in contract.browsers
                )
            else:
                expected.add(release_gate_key(platform, capability.identifier))
    return expected


def observed_release_gate_keys(
    records: Sequence[JsonObject],
    contract: AcceptanceContract,
    source_commit: str,
) -> set[str]:
    observed: set[str] = set()
    for index, record in enumerate(records):
        if (
            record.get("source_commit") != source_commit
            or record.get("result") != "passed"
        ):
            continue
        label = f"record[{index}]"
        platform = string(record.get("platform"), f"{label}.platform")
        capability_id = string(record.get("capability"), f"{label}.capability")
        capability = contract.capabilities.get(capability_id)
        if capability is None or not capability.release_required:
            continue
        browser: str | None = None
        if capability.matrix_per_browser:
            environment = mapping(record.get("environment"), f"{label}.environment")
            browser_row = mapping(
                environment.get("browser"), f"{label}.environment.browser"
            )
            browser = string(
                browser_row.get("name"), f"{label}.environment.browser.name"
            )
        observed.add(release_gate_key(platform, capability_id, browser))
    return observed


def missing_release_gates(
    records: Sequence[JsonObject],
    contract: AcceptanceContract,
    source_commit: str,
) -> list[str]:
    return sorted(
        expected_release_gate_keys(contract)
        - observed_release_gate_keys(records, contract, source_commit)
    )


def expand_record_paths(arguments: Sequence[Path]) -> tuple[Path, ...]:
    paths: set[Path] = set()
    for argument in arguments:
        if argument.is_file():
            paths.add(argument)
        elif argument.is_dir():
            paths.update(path for path in argument.rglob("*.json") if path.is_file())
        else:
            invalid_value(f"acceptance record path does not exist: {argument}")
    return tuple(sorted(paths))


def run_checked(command: Sequence[str], label: str) -> None:
    completed = subprocess.run(  # noqa: S603 - argv is passed directly without a shell.
        command,
        check=False,
        capture_output=True,
        text=True,
    )
    if completed.returncode == 0:
        return
    detail = "\n".join(
        part.strip() for part in (completed.stdout, completed.stderr) if part.strip()
    )
    invalid_value(f"{label} failed{f': {detail}' if detail else ''}")


def validate_schema_and_records(
    schema_path: Path,
    example_path: Path,
    record_paths: Sequence[Path],
) -> None:
    run_checked(
        ["check-jsonschema", "--check-metaschema", str(schema_path)],
        "acceptance record metaschema validation",
    )
    run_checked(
        ["check-jsonschema", "--schemafile", str(schema_path), str(example_path)],
        "acceptance record example validation",
    )
    if record_paths:
        run_checked(
            [
                "check-jsonschema",
                "--schemafile",
                str(schema_path),
                *(str(path) for path in record_paths),
            ],
            "acceptance record schema validation",
        )


def main(argv: Sequence[str] | None = None) -> int:
    root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--release-candidate",
        metavar="SOURCE_COMMIT",
        help="require the complete Linux/Windows release matrix for this commit",
    )
    parser.add_argument("records", nargs="*", type=Path)
    arguments = parser.parse_args(argv)
    schema_path = root / "rust/pokecon/registry/acceptance-record.schema.json"
    example_path = root / "rust/pokecon/registry/acceptance-record.example.json"
    try:
        record_paths = expand_record_paths(arguments.records)
        if arguments.release_candidate is not None and not record_paths:
            invalid_value("release-candidate validation requires acceptance records")
        validate_schema_and_records(schema_path, example_path, record_paths)
        contract = load_contract(schema_path)
        records = [load_json(path) for path in record_paths]
        for path, record in zip(record_paths, records, strict=True):
            validate_passed_record(record, contract, str(path))
        if arguments.release_candidate is not None:
            source_commit = commit_sha(
                arguments.release_candidate, "release candidate source commit"
            )
            missing = missing_release_gates(records, contract, source_commit)
            if missing:
                invalid_value(
                    "release candidate is missing gates: " + ", ".join(missing)
                )
    except (OSError, ValueError) as error:
        parser.exit(1, f"acceptance record validation failed: {error}\n")
    if record_paths:
        print(f"validated {len(record_paths)} acceptance record(s)")
    else:
        print("acceptance record schema and example are valid")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
