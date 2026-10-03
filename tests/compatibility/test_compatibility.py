from __future__ import annotations

import copy
import hashlib
import json
import subprocess
import sys
from pathlib import Path

import pytest

from scripts.compatibility.inventory import Baseline
from scripts.compatibility.promote import (
    append_decision,
    canonical_sha256,
    load_candidates,
    load_history,
    persist_result,
    promotion_failures,
    record_sha256,
    validate_decision_results,
    validate_history,
)
from scripts.compatibility.roll import changed_script_paths, discover_candidates
from scripts.compatibility.runner import (
    command_root_for,
    main,
    run_managed_discovery,
    script_domains,
    serialized_report,
    serialized_results,
    success_report,
    verify_baseline,
)

REPOSITORY = Path(__file__).resolve().parents[2]


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


def test_compatibility_runner_rejects_empty_or_incompatible_script_roots() -> None:
    empty = Baseline("empty", "https://example.invalid/repository.git", "a" * 40, ())
    with pytest.raises(ValueError, match="must not be empty"):
        command_root_for(empty)
    incompatible = Baseline(
        "incompatible",
        "https://example.invalid/repository.git",
        "a" * 40,
        ("/Commands/PythonCommands", "relative/McuCommands"),
    )
    with pytest.raises(ValueError, match="incompatible paths"):
        command_root_for(incompatible)


def test_compatibility_runner_maps_discovery_timeout(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def timeout(*_args: object, **_kwargs: object) -> object:
        command = "compatibility"
        raise subprocess.TimeoutExpired(command, 120)

    monkeypatch.setattr("scripts.compatibility.runner.subprocess.run", timeout)
    with pytest.raises(RuntimeError, match="exceeded 120 seconds"):
        run_managed_discovery(
            Path("compatibility"),
            Path("worker"),
            Path("Commands"),
            Path("Data"),
            Path("site-packages"),
        )


def test_compatibility_runner_rejects_manifest_paths_outside_sandbox(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    baseline = Baseline(
        "fixture",
        "https://example.invalid/repository.git",
        "a" * 40,
        ("Commands/PythonCommands",),
    )

    def materialize(_repository: Path, _baseline: Baseline, destination: Path) -> Path:
        command_root = destination / "Commands"
        command_root.mkdir()
        return command_root

    monkeypatch.setattr("scripts.compatibility.runner.materialize_scripts", materialize)

    def fake_run_managed_discovery(*_args: object) -> dict[str, object]:
        return {"commands": []}

    monkeypatch.setattr(
        "scripts.compatibility.runner.run_managed_discovery",
        fake_run_managed_discovery,
    )
    manifest = {
        "id": "fixture",
        "scripts": [{"path": "../escape.py", "sha256": "0" * 64}],
    }
    with pytest.raises(ValueError, match="safe relative path"):
        verify_baseline(
            baseline,
            manifest,
            tmp_path,
            Path("compatibility"),
            Path("worker"),
            tmp_path,
        )


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


def passing_compatibility_results() -> dict[str, object]:
    return {
        "manifest_sha256": "c" * 64,
        "results_sha256": "d" * 64,
        "summary": {
            "baseline_count": 3,
            "script_count": 103,
            "discovered_command_count": 98,
        },
    }


def test_compatibility_success_report_is_deterministic_and_compact() -> None:
    report = success_report(passing_compatibility_results())
    assert report["schema"] == "compatibility-report/1"
    assert report["result"] == "passed"
    assert report["manifest_sha256"] == "c" * 64
    assert report["results_sha256"] == "d" * 64
    assert report["baseline_count"] == 3
    assert report["script_count"] == 103
    assert report["discovered_command_count"] == 98

    line = serialized_report(report)
    assert "\n" not in line
    assert json.loads(line) == report
    assert serialized_report(success_report(passing_compatibility_results())) == line
    for absent in ("baselines", "commands", "fixture_catalog", "Commands/"):
        assert absent not in line


def check_argv(output: Path, site_packages: Path) -> list[str]:
    return [
        "runner",
        "--check",
        "--output",
        str(output),
        "--compatibility-binary",
        "compatibility",
        "--worker",
        "worker",
        "--site-packages",
        str(site_packages),
    ]


def test_compatibility_check_success_emits_corpus_sha_report(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    results = passing_compatibility_results()

    def fake_build_results(*_args: object, **_kwargs: object) -> dict[str, object]:
        return results

    def fake_verify_promoted(*_args: object, **_kwargs: object) -> None:
        return None

    monkeypatch.setattr(
        "scripts.compatibility.runner.build_results", fake_build_results
    )
    monkeypatch.setattr(
        "scripts.compatibility.runner.verify_promoted_corpora",
        fake_verify_promoted,
    )
    output = tmp_path / "fixed-results.json"
    output.write_text(serialized_results(results), encoding="utf-8")
    monkeypatch.setattr(sys, "argv", check_argv(output, tmp_path))

    assert main() == 0

    line = capsys.readouterr().out.strip()
    assert line.count("\n") == 0
    payload = json.loads(line)
    assert payload["schema"] == "compatibility-report/1"
    assert payload["result"] == "passed"
    assert payload["manifest_sha256"] == "c" * 64
    assert payload["results_sha256"] == "d" * 64
    assert payload["baseline_count"] == 3


def test_compatibility_check_failure_emits_no_success_report(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    results = passing_compatibility_results()

    def fake_build_results(*_args: object, **_kwargs: object) -> dict[str, object]:
        return results

    def fake_verify_promoted(*_args: object, **_kwargs: object) -> None:
        return None

    monkeypatch.setattr(
        "scripts.compatibility.runner.build_results", fake_build_results
    )
    monkeypatch.setattr(
        "scripts.compatibility.runner.verify_promoted_corpora",
        fake_verify_promoted,
    )
    output = tmp_path / "fixed-results.json"
    output.write_text("{}\n", encoding="utf-8")
    monkeypatch.setattr(sys, "argv", check_argv(output, tmp_path))

    with pytest.raises(RuntimeError, match="drift"):
        main()
    assert capsys.readouterr().out == ""


def test_compatibility_promotion_failure_emits_no_success_report(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    results = passing_compatibility_results()

    def fake_build_results(*_args: object, **_kwargs: object) -> dict[str, object]:
        return results

    def fake_verify_promoted(*_args: object, **_kwargs: object) -> None:
        message = "promoted compatibility execution drift detected"
        raise RuntimeError(message)

    monkeypatch.setattr(
        "scripts.compatibility.runner.build_results", fake_build_results
    )
    monkeypatch.setattr(
        "scripts.compatibility.runner.verify_promoted_corpora",
        fake_verify_promoted,
    )
    output = tmp_path / "fixed-results.json"
    output.write_text(serialized_results(results), encoding="utf-8")
    monkeypatch.setattr(sys, "argv", check_argv(output, tmp_path))

    with pytest.raises(RuntimeError, match="drift"):
        main()
    assert capsys.readouterr().out == ""


def test_failed_evaluation_leaves_fixed_baseline_untouched(
    tmp_path: Path,
) -> None:
    candidates_path, history_path = write_ledger(tmp_path)
    fixed_manifest = tmp_path / "fixed-manifest.json"
    fixed_manifest.write_text(
        json.dumps({"baselines": [], "schema_version": 1}, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    fixed_results = tmp_path / "fixed-results.json"
    fixed_results.write_text(
        serialized_results(passing_compatibility_results()),
        encoding="utf-8",
    )
    result_directory = tmp_path / "results"
    durable_result = result_directory / "candidate-one.json"
    persist_result(result_directory, "candidate-one", passing_result())

    fixed_digests = {
        path: hashlib.sha256(path.read_bytes()).hexdigest()
        for path in (
            candidates_path,
            fixed_manifest,
            fixed_results,
            durable_result,
        )
    }
    history_before = history_path.read_bytes()

    failing = copy.deepcopy(passing_result())
    failing["result"] = "failed"
    candidate = load_candidates(candidates_path)["candidate-one"]
    assert "candidate_verification_failed" in promotion_failures(candidate, failing)
    with pytest.raises(ValueError, match="immutable once written"):
        persist_result(result_directory, "candidate-one", failing)

    assert history_path.read_bytes() == history_before
    assert len(load_history(history_path)) == 1
    for path, digest in fixed_digests.items():
        assert hashlib.sha256(path.read_bytes()).hexdigest() == digest


def test_tampered_fixed_results_fails_check_closed(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    results = passing_compatibility_results()

    def fake_build_results(*_args: object, **_kwargs: object) -> dict[str, object]:
        return results

    def fake_verify_promoted(*_args: object, **_kwargs: object) -> None:
        return None

    monkeypatch.setattr(
        "scripts.compatibility.runner.build_results", fake_build_results
    )
    monkeypatch.setattr(
        "scripts.compatibility.runner.verify_promoted_corpora",
        fake_verify_promoted,
    )
    output = tmp_path / "fixed-results.json"
    serialized = serialized_results(results)
    output.write_text(serialized, encoding="utf-8")
    output.write_text(serialized.replace("d" * 64, "0" * 64), encoding="utf-8")
    assert output.read_text(encoding="utf-8") != serialized
    monkeypatch.setattr(sys, "argv", check_argv(output, tmp_path))

    with pytest.raises(RuntimeError, match="drift"):
        main()
    assert capsys.readouterr().out == ""


def test_quarantined_decision_appends_record_without_promotion(
    tmp_path: Path,
) -> None:
    candidates_path, history_path = write_ledger(tmp_path)
    candidates = load_candidates(candidates_path)
    records = load_history(history_path)
    candidates_before = candidates_path.read_bytes()
    history_before = history_path.read_bytes()

    result = copy.deepcopy(passing_result())
    api_surface = result["api_surface"]
    assert isinstance(api_surface, dict)
    api_surface["breaking_changes"] = ["public_api_removed"]
    api_surface["result"] = "breaking"

    result_directory = tmp_path / "results"
    persist_result(result_directory, "candidate-one", result)
    decision = append_decision(
        history_path,
        records,
        candidates["candidate-one"],
        result,
        "2026-07-22T00:00:00Z",
    )
    assert decision["kind"] == "quarantined"

    updated = load_history(history_path)
    assert len(updated) == len(records) + 1
    assert updated[-1]["kind"] == "quarantined"
    assert updated[-1]["previous_sha256"] == records[-1]["record_sha256"]
    assert updated[-1]["record_sha256"] == record_sha256(updated[-1])
    validate_history(updated, candidates)
    validate_decision_results(updated, result_directory)
    assert all(record.get("kind") != "promoted" for record in updated)
    assert candidates_path.read_bytes() == candidates_before
    assert history_path.read_bytes() != history_before


def report_argv(output: Path, site_packages: Path, report: Path) -> list[str]:
    return [*check_argv(output, site_packages), "--report-output", str(report)]


def test_compatibility_check_report_output_writes_compact_file(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    results = passing_compatibility_results()

    def fake_build_results(*_args: object, **_kwargs: object) -> dict[str, object]:
        return results

    def fake_verify_promoted(*_args: object, **_kwargs: object) -> None:
        return None

    monkeypatch.setattr(
        "scripts.compatibility.runner.build_results", fake_build_results
    )
    monkeypatch.setattr(
        "scripts.compatibility.runner.verify_promoted_corpora",
        fake_verify_promoted,
    )
    output = tmp_path / "fixed-results.json"
    output.write_text(serialized_results(results), encoding="utf-8")
    report = tmp_path / "compatibility-report.json"
    monkeypatch.setattr(sys, "argv", report_argv(output, tmp_path, report))

    assert main() == 0

    line = capsys.readouterr().out.strip()
    assert line.count("\n") == 0
    payload = json.loads(line)
    assert payload["schema"] == "compatibility-report/1"
    assert payload["result"] == "passed"
    assert report.is_file() and not report.is_symlink()
    assert report.read_text(encoding="utf-8") == line + "\n"


def test_compatibility_check_report_output_failure_writes_no_file(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    results = passing_compatibility_results()

    def fake_build_results(*_args: object, **_kwargs: object) -> dict[str, object]:
        return results

    def fake_verify_promoted(*_args: object, **_kwargs: object) -> None:
        return None

    monkeypatch.setattr(
        "scripts.compatibility.runner.build_results", fake_build_results
    )
    monkeypatch.setattr(
        "scripts.compatibility.runner.verify_promoted_corpora",
        fake_verify_promoted,
    )
    output = tmp_path / "fixed-results.json"
    output.write_text("{}\n", encoding="utf-8")
    report = tmp_path / "compatibility-report.json"
    monkeypatch.setattr(sys, "argv", report_argv(output, tmp_path, report))

    with pytest.raises(RuntimeError, match="drift"):
        main()
    assert capsys.readouterr().out == ""
    assert not report.exists()


def test_compatibility_check_report_output_rejects_symlink(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    results = passing_compatibility_results()

    def fake_build_results(*_args: object, **_kwargs: object) -> dict[str, object]:
        return results

    def fake_verify_promoted(*_args: object, **_kwargs: object) -> None:
        return None

    monkeypatch.setattr(
        "scripts.compatibility.runner.build_results", fake_build_results
    )
    monkeypatch.setattr(
        "scripts.compatibility.runner.verify_promoted_corpora",
        fake_verify_promoted,
    )
    output = tmp_path / "fixed-results.json"
    output.write_text(serialized_results(results), encoding="utf-8")
    report = tmp_path / "compatibility-report.json"
    report.symlink_to(tmp_path / "nothing.json")
    monkeypatch.setattr(sys, "argv", report_argv(output, tmp_path, report))

    with pytest.raises(ValueError, match="already exists"):
        main()
    assert report.is_symlink()


def test_compatibility_task_emits_only_its_own_report() -> None:
    flake = (REPOSITORY / "flake.nix").read_text(encoding="utf-8")
    _prefix, separator, tail = flake.partition("            compatibility = mkTask {\n")
    assert separator, "missing compatibility task in flake.nix"
    body, separator, _suffix = tail.partition(
        "            compatibility-roll = mkTask {\n"
    )
    assert separator, "missing compatibility-roll task in flake.nix"
    assert "POKECON_ACCEPTANCE_REPORT_DIR" in body
    assert "--report-output" in body
    assert "compatibility-report.json" in body
    for foreign in (
        "schema-report.json",
        "boundary-report.json",
        "abstraction-report.json",
        "lifecycle-report.json",
    ):
        assert foreign not in body
