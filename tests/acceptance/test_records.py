from __future__ import annotations

import json
from pathlib import Path

import pytest

from scripts.acceptance.records import (
    AcceptanceContract,
    expected_release_gate_keys,
    load_contract,
    main,
    mapping,
    missing_release_gates,
    string,
    validate_passed_record,
    validate_schema_and_records,
)

ROOT = Path(__file__).resolve().parents[2]
SCHEMA = ROOT / "rust/pokecon/registry/acceptance-record.schema.json"
EXAMPLE = ROOT / "rust/pokecon/registry/acceptance-record.example.json"
SOURCE_COMMIT = "1" * 40
BROWSER_CAPABILITIES = {
    "browser_matrix",
    "performance",
    "integrated_load_stress",
    "security_acceptance",
}


def performance_measurements() -> list[dict[str, object]]:
    return [
        {
            "metric": "webrtc_video_latency",
            "unit": "ms",
            "sample_count": 300,
            "p50": 40,
            "p95": 80,
            "maximum": 90,
        },
        {
            "metric": "mjpeg_video_latency",
            "unit": "ms",
            "sample_count": 300,
            "p50": 80,
            "p95": 140,
            "maximum": 145,
        },
        {
            "metric": "controller_input_latency",
            "unit": "ms",
            "sample_count": 300,
            "p50": 20,
            "p95": 40,
            "maximum": 45,
        },
        {
            "metric": "ui_frame_rate",
            "unit": "fps",
            "sample_count": 300,
            "p50": 60,
            "p95": 62,
            "maximum": 63,
        },
        {
            "metric": "ui_input_latency",
            "unit": "ms",
            "sample_count": 300,
            "p50": 8,
            "p95": 15,
            "maximum": 15.5,
        },
    ]


def passed_record(
    contract: AcceptanceContract,
    capability_id: str,
    platform: str = "linux",
    browser: str = "chrome",
    source_commit: str = SOURCE_COMMIT,
) -> dict[str, object]:
    capability = contract.capabilities[capability_id]
    environment: dict[str, object] = {
        "os_name": "Acceptance fixture OS",
        "os_version": "1",
        "app_mode": "desktop",
        "build_identity": "acceptance-fixture-build",
        "artifact_sha256": "2" * 64,
        "device_inventory": ["fixture device; serial number redacted"],
    }
    if capability_id in BROWSER_CAPABILITIES:
        environment["browser"] = {
            "name": browser,
            "version": "999",
            "client_os": "Acceptance fixture client OS",
        }
    return {
        "schema_version": 1,
        "specification_version": contract.specification_version,
        "source_commit": source_commit,
        "capability": capability_id,
        "platform": platform,
        "started_at": "2026-01-01T00:00:00Z",
        "completed_at": "2026-01-01T01:00:00Z",
        "operator": "acceptance-fixture-operator",
        "environment": environment,
        "requirement_ids": ["§15"],
        "steps": [
            {
                "id": step_id,
                "result": "passed",
                "evidence": [f"fixture/{capability_id}/{step_id}.json"],
            }
            for step_id in capability.required_steps
        ],
        "measurements": performance_measurements()
        if capability_id == "performance"
        else [],
        "result": "passed",
        "notes": "Synthetic contract fixture; never release evidence.",
        "diagnostic_ids": [],
    }


def write_record(path: Path, record: dict[str, object]) -> None:
    path.write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")


def release_matrix_records(contract: AcceptanceContract) -> list[dict[str, object]]:
    records: list[dict[str, object]] = []
    for platform in contract.platforms:
        for capability in contract.capabilities.values():
            if not capability.release_required:
                continue
            browsers = (
                contract.browsers if capability.matrix_per_browser else ("chrome",)
            )
            records.extend(
                passed_record(contract, capability.identifier, platform, browser)
                for browser in browsers
            )
    return records


def test_every_capability_accepts_only_its_complete_canonical_steps(
    tmp_path: Path,
) -> None:
    contract = load_contract(SCHEMA)
    valid_paths: list[Path] = []
    for capability_id in contract.capabilities:
        record = passed_record(contract, capability_id)
        validate_passed_record(record, contract, capability_id)
        path = tmp_path / f"{capability_id}.json"
        write_record(path, record)
        valid_paths.append(path)
    validate_schema_and_records(SCHEMA, EXAMPLE, valid_paths)

    incomplete = passed_record(contract, "mcu_serial_device_and_target_console")
    steps = incomplete["steps"]
    assert isinstance(steps, list)
    steps.pop()
    with pytest.raises(ValueError, match=r"canonical.*steps"):
        validate_passed_record(incomplete, contract, "incomplete")
    invalid_path = tmp_path / "incomplete.json"
    write_record(invalid_path, incomplete)
    with pytest.raises(ValueError, match="schema validation"):
        validate_schema_and_records(SCHEMA, EXAMPLE, [invalid_path])


def test_passed_record_requires_forward_elapsed_time() -> None:
    contract = load_contract(SCHEMA)
    record = passed_record(contract, "desktop_lifecycle")
    record["completed_at"] = record["started_at"]
    with pytest.raises(ValueError, match="later than started_at"):
        validate_passed_record(record, contract, "reversed-time")


def test_release_candidate_requires_every_platform_and_browser_gate() -> None:
    contract = load_contract(SCHEMA)
    records = release_matrix_records(contract)
    assert len(records) == len(expected_release_gate_keys(contract)) == 24
    assert missing_release_gates(records, contract, SOURCE_COMMIT) == []

    removed = records.pop()
    removed_capability = contract.capabilities[str(removed["capability"])]
    browser_name = "chrome"
    if removed_capability.matrix_per_browser:
        environment = mapping(removed.get("environment"), "removed.environment")
        browser = mapping(environment.get("browser"), "removed.environment.browser")
        browser_name = string(browser.get("name"), "removed.environment.browser.name")
    suffix = f"/{browser_name}" if removed_capability.matrix_per_browser else ""
    assert missing_release_gates(records, contract, SOURCE_COMMIT) == [
        f"{removed['platform']}/{removed['capability']}{suffix}"
    ]

    foreign = passed_record(
        contract,
        str(removed["capability"]),
        str(removed["platform"]),
        browser_name,
        source_commit="3" * 40,
    )
    assert missing_release_gates([*records, foreign], contract, SOURCE_COMMIT)


def test_release_candidate_cli_validates_the_complete_matrix(tmp_path: Path) -> None:
    contract = load_contract(SCHEMA)
    for index, record in enumerate(release_matrix_records(contract)):
        write_record(tmp_path / f"record-{index:02}.json", record)
    assert main(["--release-candidate", SOURCE_COMMIT, str(tmp_path)]) == 0

    (tmp_path / "record-23.json").unlink()
    with pytest.raises(SystemExit) as failure:
        main(["--release-candidate", SOURCE_COMMIT, str(tmp_path)])
    assert failure.value.code == 1
