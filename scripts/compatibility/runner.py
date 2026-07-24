"""Execute the immutable compatibility corpus through the managed worker."""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import subprocess
import tarfile
import tempfile
from pathlib import Path, PurePosixPath
from typing import TYPE_CHECKING, Never, cast

from scripts.compatibility.inventory import (
    Baseline,
    load_baselines,
    materialize_repository,
    parse_repository_overrides,
    require_mapping,
    require_sequence,
    require_string,
    run_git,
)
from scripts.compatibility.promote import (
    canonical_sha256,
    load_candidates,
    load_history,
    validate_decision_results,
    validate_history,
)

if TYPE_CHECKING:
    from collections.abc import Mapping


def invalid_value(message: str) -> Never:
    raise ValueError(message)


def failed_runtime(message: str) -> Never:
    raise RuntimeError(message)


FIXTURE_CATALOG: dict[str, dict[str, str]] = {
    "managed_worker_discovery": {
        "mode": "deterministic_fixture",
        "selector": "pokecon-worker::script_runtime::discovery_preserves_declaration_order_and_separates_tag_layers",
    },
    "controller_serial": {
        "mode": "deterministic_fixture",
        "selector": "pokecon-worker::script_runtime::script_worker_executes_controller_serial_and_output_proxies",
    },
    "camera_image": {
        "mode": "deterministic_fixture",
        "selector": "pokecon-worker::script_runtime::script_camera_images_are_private_and_image_processing_stays_worker_local",
    },
    "dialog_tk": {
        "mode": "deterministic_fixture",
        "selector": "pokecon-worker::script_runtime::script_dialogs_preserve_widget_lifecycle_and_legacy_forms",
    },
    "network_notification": {
        "mode": "deterministic_fixture",
        "selector": "pokecon-worker::script_runtime::script_network_and_notifications_use_closed_fail_soft_proxies",
    },
    "overlay_pointer": {
        "mode": "deterministic_fixture",
        "selector": "pokecon-worker::script_runtime::overlay_pointer_events_run_fifo_callbacks_without_blocking_ipc",
    },
    "filesystem": {
        "mode": "deterministic_fixture",
        "selector": "compatibility_runner::immutable_archive_and_digest_validation",
    },
    "mcu_device": {
        "mode": "explicit_hardware_gate",
        "selector": "hardware:mcu_serial_device_and_target_console",
    },
    "camera_device": {
        "mode": "explicit_hardware_gate",
        "selector": "hardware:capture_device_with_known_frame_fixture",
    },
    "audio_device": {
        "mode": "explicit_hardware_gate",
        "selector": "hardware:audio_input_device",
    },
    "external_services": {
        "mode": "explicit_hardware_gate",
        "selector": "hardware:credentialed_network_notification_endpoints",
    },
}


def load_manifest(path: Path) -> dict[str, object]:
    raw: object = json.loads(path.read_text(encoding="utf-8"))
    return require_mapping(raw, "fixed compatibility manifest")


def command_root_for(baseline: Baseline) -> PurePosixPath:
    roots = [PurePosixPath(root) for root in baseline.script_roots]
    common = PurePosixPath(os.path.commonpath([str(root) for root in roots]))
    if common.name in {"McuCommands", "PythonCommands"}:
        common = common.parent
    if common.name != "Commands" or not all(
        root.is_relative_to(common) for root in roots
    ):
        invalid_value(
            f"{baseline.identifier}: script roots do not share a Commands directory"
        )
    return common


def materialize_scripts(
    repository: Path, baseline: Baseline, destination: Path
) -> Path:
    archive = run_git(
        repository,
        [
            "archive",
            "--format=tar",
            baseline.commit,
            "--",
            *baseline.script_roots,
        ],
    )
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:") as source:
        source.extractall(destination, filter="data")
    command_root = destination.joinpath(*command_root_for(baseline).parts)
    if not command_root.is_dir():
        failed_runtime(
            f"{baseline.identifier}: archive has no materialized command root"
        )
    return command_root


def run_managed_discovery(
    compatibility_binary: Path,
    worker: Path,
    command_root: Path,
    data_root: Path,
    site_packages: Path,
) -> dict[str, object]:
    process = subprocess.run(  # noqa: S603 - validated direct executable, never a shell
        [
            str(compatibility_binary),
            "--worker",
            str(worker),
            "--command-root",
            str(command_root),
            "--data-root",
            str(data_root),
            "--site-packages",
            str(site_packages),
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if process.returncode != 0:
        diagnostic = process.stderr.strip()
        failed_runtime(f"managed worker discovery failed: {diagnostic}")
    raw: object = json.loads(process.stdout)
    return require_mapping(raw, "managed worker discovery result")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while block := source.read(64 * 1024):
            digest.update(block)
    return digest.hexdigest()


def strings(value: object, label: str) -> list[str]:
    return [require_string(item, label) for item in require_sequence(value, label)]


def script_domains(script: Mapping[str, object], has_commands: bool) -> list[str]:
    imports = {
        require_string(
            require_mapping(row, "script import").get("module"), "import module"
        )
        for row in require_sequence(script.get("imports"), "script imports")
    }
    classes = require_sequence(script.get("classes"), "script classes")
    bases = {
        base
        for raw_class in classes
        for base in strings(
            require_mapping(raw_class, "script class").get("bases"), "class bases"
        )
    }
    references = set(strings(script.get("referenced_api"), "referenced_api"))
    joined = " ".join(sorted(imports | bases | references)).casefold()

    domains = ["managed_worker_discovery"]
    if has_commands or any("command" in base.casefold() for base in bases):
        domains.append("controller_serial")
    if any(
        token in joined for token in ("imageproc", "camera", "template", "cv2", "numpy")
    ):
        domains.extend(("camera_image", "camera_device"))
    if any(token in joined for token in ("dialog", "tkinter", "widget")):
        domains.append("dialog_tk")
    if any(token in joined for token in ("socket_", "mqtt_", "discord_", "line_")):
        domains.extend(("network_notification", "external_services"))
    if any(
        token in joined
        for token in ("displayrectangle", "displaytext", ".gui", "lstick")
    ):
        domains.append("overlay_pointer")
    if imports.intersection({"os", "shutil"}):
        domains.append("filesystem")
    if "pyaudio" in imports:
        domains.append("audio_device")
    if any("mcucommand" in base.casefold() for base in bases):
        domains.append("mcu_device")
    return list(dict.fromkeys(domains))


def commands_by_path(
    discovery: Mapping[str, object], command_prefix: PurePosixPath
) -> dict[str, list[dict[str, object]]]:
    grouped: dict[str, list[dict[str, object]]] = {}
    for index, raw_command in enumerate(
        require_sequence(discovery.get("commands"), "discovery commands")
    ):
        command = require_mapping(raw_command, f"discovery commands[{index}]")
        relative = PurePosixPath(
            require_string(command.get("relative_path"), "command relative_path")
        )
        path = str(command_prefix / relative)
        normalized: dict[str, object] = {
            "class_name": require_string(
                require_mapping(command.get("command"), "command metadata").get(
                    "class_name"
                ),
                "command class_name",
            ),
            "kind": require_string(command.get("kind"), "command kind"),
            "name": require_string(
                require_mapping(command.get("command"), "command metadata").get("name"),
                "command name",
            ),
            "manual_tags": strings(command.get("manual_tags"), "command manual_tags"),
        }
        grouped.setdefault(path, []).append(normalized)
    return grouped


def verify_baseline(
    baseline: Baseline,
    manifest: Mapping[str, object],
    repository: Path,
    compatibility_binary: Path,
    worker: Path,
    site_packages: Path,
) -> dict[str, object]:
    if (
        require_string(manifest.get("id"), "manifest baseline id")
        != baseline.identifier
    ):
        invalid_value(f"manifest baseline order differs for {baseline.identifier}")
    with tempfile.TemporaryDirectory(
        prefix=f"pokecon-compatibility-{baseline.identifier}-"
    ) as raw_directory:
        directory = Path(raw_directory)
        command_root = materialize_scripts(repository, baseline, directory)
        data_root = directory / "Data"
        data_root.mkdir()
        discovery = run_managed_discovery(
            compatibility_binary,
            worker,
            command_root,
            data_root,
            site_packages,
        )
        grouped = commands_by_path(discovery, command_root_for(baseline))
        scripts: list[dict[str, object]] = []
        known_paths: set[str] = set()
        for index, raw_script in enumerate(
            require_sequence(manifest.get("scripts"), "manifest scripts")
        ):
            script = require_mapping(raw_script, f"manifest scripts[{index}]")
            path = require_string(script.get("path"), "script path")
            known_paths.add(path)
            source = directory.joinpath(*PurePosixPath(path).parts)
            expected_sha = require_string(script.get("sha256"), "script sha256")
            if not source.is_file() or sha256_file(source) != expected_sha:
                failed_runtime(
                    f"{baseline.identifier}:{path}: immutable content digest differs"
                )
            commands = grouped.get(path, [])
            domains = script_domains(script, bool(commands))
            scripts.append(
                {
                    "path": path,
                    "sha256": expected_sha,
                    "managed_worker": {
                        "import_evaluation": "passed",
                        "commands": commands,
                    },
                    "domains": domains,
                    "runtime_evidence": [
                        {"id": domain, **FIXTURE_CATALOG[domain]} for domain in domains
                    ],
                    "result": "passed",
                }
            )
        unknown = sorted(set(grouped) - known_paths)
        if unknown:
            failed_runtime(
                f"{baseline.identifier}: worker discovered untracked scripts: {unknown}"
            )

    canonical_discovery = json.dumps(
        grouped, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode()
    return {
        "id": baseline.identifier,
        "repository": baseline.repository,
        "commit": baseline.commit,
        "script_count": len(scripts),
        "discovered_command_count": sum(len(commands) for commands in grouped.values()),
        "discovery_sha256": hashlib.sha256(canonical_discovery).hexdigest(),
        "scripts": scripts,
        "result": "passed",
    }


def build_results(
    registry_path: Path,
    manifest_path: Path,
    repository_overrides: Mapping[str, Path],
    compatibility_binary: Path,
    worker: Path,
    site_packages: Path,
) -> dict[str, object]:
    for label, path in (
        ("compatibility binary", compatibility_binary),
        ("managed worker", worker),
    ):
        if not path.is_file():
            invalid_value(f"{label} is not a regular file: {path}")
    if not site_packages.is_dir():
        invalid_value(f"site-packages is not a directory: {site_packages}")

    baselines = load_baselines(registry_path)
    manifest = load_manifest(manifest_path)
    manifest_rows = require_sequence(manifest.get("baselines"), "manifest baselines")
    if len(manifest_rows) != len(baselines):
        invalid_value("manifest and registry baseline counts differ")
    results: list[dict[str, object]] = []
    for baseline, raw_manifest in zip(baselines, manifest_rows, strict=True):
        with materialize_repository(baseline, repository_overrides) as repository:
            results.append(
                verify_baseline(
                    baseline,
                    require_mapping(raw_manifest, "manifest baseline"),
                    repository,
                    compatibility_binary,
                    worker,
                    site_packages,
                )
            )
    canonical = json.dumps(
        results, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode()
    return {
        "schema_version": 1,
        "execution_model": "isolated_managed_worker",
        "fixture_catalog": FIXTURE_CATALOG,
        "manifest_sha256": require_string(
            manifest.get("inventory_sha256"), "manifest inventory_sha256"
        ),
        "results_sha256": hashlib.sha256(canonical).hexdigest(),
        "summary": {
            "baseline_count": len(results),
            "script_count": sum(
                cast("int", result["script_count"]) for result in results
            ),
            "discovered_command_count": sum(
                cast("int", result["discovered_command_count"]) for result in results
            ),
            "failed": 0,
            "result": "passed",
        },
        "baselines": results,
    }


def serialized_results(results: Mapping[str, object]) -> str:
    return json.dumps(results, ensure_ascii=False, indent=2, sort_keys=True) + "\n"


def verify_promoted_corpora(
    registry_path: Path,
    candidates_path: Path,
    history_path: Path,
    result_directory: Path,
    compatibility_binary: Path,
    worker: Path,
    site_packages: Path,
) -> None:
    candidates = load_candidates(candidates_path)
    records = load_history(history_path)
    validate_history(records, candidates)
    validate_decision_results(records, result_directory)
    baselines = {
        baseline.identifier: baseline for baseline in load_baselines(registry_path)
    }
    for record in records:
        if record.get("kind") != "promoted":
            continue
        candidate_id = require_string(record.get("candidate_id"), "candidate_id")
        candidate = candidates[candidate_id]
        baseline_id = require_string(candidate.get("baseline_id"), "baseline_id")
        baseline = baselines.get(baseline_id)
        if baseline is None:
            invalid_value(
                f"promoted candidate {candidate_id} has an unknown baseline {baseline_id}"
            )
        result_path = result_directory / f"{candidate_id}.json"
        raw_result: object = json.loads(result_path.read_text(encoding="utf-8"))
        result = require_mapping(raw_result, f"candidate result {candidate_id}")
        if canonical_sha256(result) != record.get("result_sha256"):
            failed_runtime(f"promoted candidate result drift detected: {candidate_id}")
        manifest = require_mapping(result.get("corpus_manifest"), "corpus_manifest")
        expected_execution = require_mapping(
            result.get("candidate_execution"), "candidate_execution"
        )
        promoted = Baseline(
            identifier=candidate_id,
            repository=require_string(
                candidate.get("repository"), "candidate repository"
            ),
            commit=require_string(candidate.get("commit"), "candidate commit"),
            script_roots=baseline.script_roots,
        )
        with materialize_repository(promoted, {}) as repository:
            actual_execution = verify_baseline(
                promoted,
                manifest,
                repository,
                compatibility_binary,
                worker,
                site_packages,
            )
        if actual_execution != expected_execution:
            failed_runtime(
                f"promoted compatibility execution drift detected: {candidate_id}"
            )


def main() -> int:
    root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--registry",
        type=Path,
        default=root / "rust/pokecon-contracts/registry/compatibility.json",
    )
    parser.add_argument(
        "--manifest", type=Path, default=root / "compatibility/fixed-manifest.json"
    )
    parser.add_argument(
        "--output", type=Path, default=root / "compatibility/fixed-results.json"
    )
    parser.add_argument(
        "--candidates", type=Path, default=root / "compatibility/candidates.json"
    )
    parser.add_argument(
        "--history", type=Path, default=root / "compatibility/promotions.jsonl"
    )
    parser.add_argument("--results", type=Path, default=root / "compatibility/results")
    parser.add_argument("--compatibility-binary", type=Path, required=True)
    parser.add_argument("--worker", type=Path, required=True)
    parser.add_argument("--site-packages", type=Path, required=True)
    parser.add_argument(
        "--repository",
        action="append",
        default=[],
        metavar="BASELINE_ID=/ABSOLUTE/PATH",
    )
    parser.add_argument("--check", action="store_true")
    arguments = parser.parse_args()
    results = build_results(
        arguments.registry,
        arguments.manifest,
        parse_repository_overrides(arguments.repository),
        arguments.compatibility_binary,
        arguments.worker,
        arguments.site_packages,
    )
    serialized = serialized_results(results)
    if arguments.check:
        if (
            not arguments.output.is_file()
            or arguments.output.read_text(encoding="utf-8") != serialized
        ):
            failed_runtime(f"compatibility result drift detected: {arguments.output}")
    else:
        arguments.output.parent.mkdir(parents=True, exist_ok=True)
        arguments.output.write_text(serialized, encoding="utf-8")
    verify_promoted_corpora(
        arguments.registry,
        arguments.candidates,
        arguments.history,
        arguments.results,
        arguments.compatibility_binary,
        arguments.worker,
        arguments.site_packages,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
