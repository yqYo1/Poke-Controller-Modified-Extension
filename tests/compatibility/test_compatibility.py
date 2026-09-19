from __future__ import annotations

import copy
import json
from typing import TYPE_CHECKING

import pytest

from scripts.compatibility.inventory import Baseline
from scripts.compatibility.promote import (
    append_decision,
    canonical_sha256,
    load_candidates,
    load_history,
    persist_result,
    record_sha256,
    validate_decision_results,
    validate_history,
)
from scripts.compatibility.roll import changed_script_paths, discover_candidates
from scripts.compatibility.runner import command_root_for, script_domains

if TYPE_CHECKING:
    from pathlib import Path


def test_command_root_and_domain_classification_are_closed() -> None:
    baseline = Baseline(
        identifier="fixture",
        repository="https://example.invalid/repository.git",
        commit="a" * 40,
        script_roots=(
            "SerialController/Commands/PythonCommands",
            "SerialController/Commands/McuCommands",
        ),
    )
    assert str(command_root_for(baseline)) == "SerialController/Commands"

    script: dict[str, object] = {
        "imports": [
            {"module": "Commands.McuCommandBase", "names": ["McuCommand"]},
            {"module": "cv2", "names": ["cv2"]},
            {"module": "tkinter.messagebox", "names": ["messagebox"]},
            {"module": "pyaudio", "names": ["pyaudio"]},
        ],
        "classes": [{"name": "Fixture", "bases": ["McuCommand"], "line": 1}],
        "referenced_api": ["self.discord_image", "self.displayRectangle"],
    }
    assert script_domains(script, True) == [
        "managed_worker_discovery",
        "controller_serial",
        "camera_image",
        "camera_device",
        "dialog_tk",
        "network_notification",
        "external_services",
        "overlay_pointer",
        "audio_device",
        "mcu_device",
    ]


def candidate_document() -> dict[str, object]:
    return {
        "schema_version": 1,
        "candidates": [
            {
                "id": "candidate-one",
                "baseline_id": "yqyo1-extension",
                "repository": "https://example.invalid/repository.git",
                "commit": "a" * 40,
            }
        ],
    }


def passing_result() -> dict[str, object]:
    return {
        "schema_version": 1,
        "candidate_id": "candidate-one",
        "commit": "a" * 40,
        "result": "passed",
        "full_fixed_chain": {"result": "passed", "baseline_count": 3},
        "runtime": {"result": "passed", "unverified": 0},
        "api_surface": {"result": "non_breaking", "breaking_changes": []},
        "corpus_manifest": {"id": "candidate-one"},
    }


def write_ledger(directory: Path) -> tuple[Path, Path]:
    candidates = directory / "candidates.json"
    candidates.write_text(
        json.dumps(candidate_document(), sort_keys=True), encoding="utf-8"
    )
    genesis: dict[str, object] = {
        "schema_version": 1,
        "sequence": 0,
        "previous_sha256": None,
        "kind": "genesis",
        "fixed_baselines_immutable": True,
    }
    genesis["record_sha256"] = record_sha256(genesis)
    history = directory / "promotions.jsonl"
    history.write_text(
        json.dumps(genesis, sort_keys=True, separators=(",", ":")) + "\n",
        encoding="utf-8",
    )
    return candidates, history


def test_promotion_history_is_append_only_and_hash_chained(tmp_path: Path) -> None:
    candidates_path, history_path = write_ledger(tmp_path)
    candidates = load_candidates(candidates_path)
    records = load_history(history_path)
    validate_history(records, candidates)

    result = passing_result()
    decision = append_decision(
        history_path,
        records,
        candidates["candidate-one"],
        result,
        "2026-07-22T00:00:00Z",
    )
    assert decision["kind"] == "promoted"
    assert decision["result_sha256"] == canonical_sha256(result)
    validate_history(load_history(history_path), candidates)

    tampered = load_history(history_path)
    tampered[1]["commit"] = "b" * 40
    with pytest.raises(ValueError, match="digest differs"):
        validate_history(tampered, candidates)

    result_directory = tmp_path / "results"
    persist_result(result_directory, "candidate-one", result)
    validate_decision_results(load_history(history_path), result_directory)
    persisted = result_directory / "candidate-one.json"
    persisted.write_text("{}\n", encoding="utf-8")
    with pytest.raises(ValueError, match="digest differs"):
        validate_decision_results(load_history(history_path), result_directory)


def test_breaking_candidate_is_quarantined(tmp_path: Path) -> None:
    candidates_path, history_path = write_ledger(tmp_path)
    candidates = load_candidates(candidates_path)
    records = load_history(history_path)
    result = copy.deepcopy(passing_result())
    api_surface = result["api_surface"]
    assert isinstance(api_surface, dict)
    api_surface["breaking_changes"] = ["public_api_removed"]
    api_surface["result"] = "breaking"

    decision = append_decision(
        history_path,
        records,
        candidates["candidate-one"],
        result,
        "2026-07-22T00:00:00Z",
    )
    assert decision["kind"] == "quarantined"
    assert decision["reasons"] == ["breaking_api_change", "api_surface_unverified"]


def test_default_branch_discovery_is_append_only(tmp_path: Path) -> None:
    registry = tmp_path / "registry.json"
    registry.write_text(
        json.dumps(
            {
                "fixed_baselines": [
                    {
                        "id": "fixture",
                        "repository": "https://example.invalid/fixture.git",
                        "commit": "a" * 40,
                    }
                ]
            }
        ),
        encoding="utf-8",
    )
    candidates = tmp_path / "candidates.json"
    candidates.write_text(
        json.dumps({"schema_version": 1, "candidates": []}), encoding="utf-8"
    )
    head = "b" * 40

    def resolve(_repository: str) -> str:
        return head

    assert discover_candidates(registry, candidates, resolve) == [
        "fixture-bbbbbbbbbbbb"
    ]
    assert discover_candidates(registry, candidates, resolve) == []
    assert load_candidates(candidates)["fixture-bbbbbbbbbbbb"]["commit"] == head


def test_changed_script_detection_preserves_existing_guarantees() -> None:
    fixed: dict[str, object] = {
        "scripts": [
            {"path": "Commands/unchanged.py", "sha256": "a" * 64},
            {"path": "Commands/changed.py", "sha256": "b" * 64},
        ]
    }
    candidate: dict[str, object] = {
        "scripts": [
            {"path": "Commands/unchanged.py", "sha256": "a" * 64},
            {"path": "Commands/changed.py", "sha256": "c" * 64},
            {"path": "Commands/new.py", "sha256": "d" * 64},
        ]
    }
    assert changed_script_paths(fixed, candidate) == {
        "Commands/changed.py",
        "Commands/new.py",
    }
