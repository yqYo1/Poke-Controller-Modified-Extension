"""Discover and evaluate rolling compatibility candidates."""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import subprocess
from datetime import UTC, datetime
from pathlib import Path
from typing import TYPE_CHECKING, Never

from scripts.compatibility_inventory import (
    Baseline,
    inventory_baseline,
    load_baselines,
    materialize_repository,
    parse_repository_overrides,
    require_mapping,
    require_sequence,
    require_string,
)
from scripts.compatibility_promote import (
    append_decision,
    load_candidates,
    load_history,
    persist_result,
    validate_decision_results,
    validate_history,
)
from scripts.compatibility_runner import (
    FIXTURE_CATALOG,
    build_results,
    load_manifest,
    serialized_results,
    verify_baseline,
)

if TYPE_CHECKING:
    from collections.abc import Callable, Mapping


def invalid_value(message: str) -> Never:
    raise ValueError(message)


def remote_head(repository: str) -> str:
    git = shutil.which("git")
    if git is None:
        message = "git is unavailable for default-branch lookup"
        raise RuntimeError(message)
    process = subprocess.run(  # noqa: S603 - resolved direct executable, never a shell
        [git, "ls-remote", repository, "HEAD"],
        check=False,
        capture_output=True,
        text=True,
    )
    if process.returncode != 0:
        message = f"default-branch lookup failed for {repository}"
        raise RuntimeError(message)
    fields = process.stdout.split()
    if len(fields) != 2 or fields[1] != "HEAD":
        message = f"default-branch lookup was malformed for {repository}"
        raise RuntimeError(message)
    commit = fields[0]
    if len(commit) != 40 or not all(
        character in "0123456789abcdef" for character in commit
    ):
        message = f"default-branch lookup returned an invalid commit for {repository}"
        raise RuntimeError(message)
    return commit


def discover_candidates(
    registry_path: Path,
    candidates_path: Path,
    resolve_head: Callable[[str], str] = remote_head,
) -> list[str]:
    raw_registry: object = json.loads(registry_path.read_text(encoding="utf-8"))
    registry = require_mapping(raw_registry, "compatibility registry")
    rows = require_sequence(registry.get("fixed_baselines"), "fixed_baselines")
    raw_candidates: object = json.loads(candidates_path.read_text(encoding="utf-8"))
    document = require_mapping(raw_candidates, "compatibility candidates")
    if document.get("schema_version") != 1:
        invalid_value("compatibility candidates schema_version must be 1")
    candidates = require_sequence(document.get("candidates"), "candidates")
    existing_commits = {
        require_string(
            require_mapping(row, "candidate").get("commit"), "candidate commit"
        )
        for row in candidates
    }
    discovered: list[str] = []
    for index, raw_row in enumerate(rows):
        row = require_mapping(raw_row, f"fixed_baselines[{index}]")
        baseline_id = require_string(row.get("id"), "baseline id")
        repository = require_string(row.get("repository"), "baseline repository")
        fixed_commit = require_string(row.get("commit"), "baseline commit")
        commit = resolve_head(repository)
        if commit == fixed_commit or commit in existing_commits:
            continue
        identifier = f"{baseline_id}-{commit[:12]}"
        candidates.append(
            {
                "id": identifier,
                "baseline_id": baseline_id,
                "repository": repository,
                "commit": commit,
            }
        )
        existing_commits.add(commit)
        discovered.append(identifier)
    if discovered:
        candidates_path.write_text(
            json.dumps(document, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
            newline="\n",
        )
        load_candidates(candidates_path)
    return discovered


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while block := source.read(64 * 1024):
            digest.update(block)
    return digest.hexdigest()


def changed_script_paths(
    fixed_manifest: Mapping[str, object], candidate_manifest: Mapping[str, object]
) -> set[str]:
    fixed = {
        require_string(script.get("path"), "fixed script path"): require_string(
            script.get("sha256"), "fixed script sha256"
        )
        for raw_script in require_sequence(
            fixed_manifest.get("scripts"), "fixed scripts"
        )
        for script in [require_mapping(raw_script, "fixed script")]
    }
    return {
        path
        for raw_script in require_sequence(
            candidate_manifest.get("scripts"), "candidate scripts"
        )
        for script in [require_mapping(raw_script, "candidate script")]
        for path in [require_string(script.get("path"), "candidate script path")]
        if fixed.get(path)
        != require_string(script.get("sha256"), "candidate script sha256")
    }


def fixed_chain_summary(results: Mapping[str, object]) -> dict[str, object]:
    summary = require_mapping(results.get("summary"), "fixed result summary")
    if summary.get("result") != "passed" or summary.get("baseline_count") != 3:
        invalid_value("the complete fixed compatibility chain did not pass")
    return {
        "result": "passed",
        "baseline_count": 3,
        "script_count": summary.get("script_count"),
        "results_sha256": results.get("results_sha256"),
    }


def evaluate_candidate(
    candidate: Mapping[str, object],
    baseline: Baseline,
    fixed_manifest: Mapping[str, object],
    fixed_results: Mapping[str, object],
    compatibility_binary: Path,
    worker: Path,
    site_packages: Path,
    protocol_path: Path,
    repository_overrides: Mapping[str, Path],
) -> dict[str, object]:
    candidate_id = require_string(candidate.get("id"), "candidate id")
    commit = require_string(candidate.get("commit"), "candidate commit")
    repository = require_string(candidate.get("repository"), "candidate repository")
    promoted = Baseline(
        identifier=candidate_id,
        repository=repository,
        commit=commit,
        script_roots=baseline.script_roots,
    )
    fixed_chain = fixed_chain_summary(fixed_results)
    try:
        override = repository_overrides.get(candidate_id) or repository_overrides.get(
            baseline.identifier
        )
        overrides = {candidate_id: override} if override is not None else {}
        with materialize_repository(promoted, overrides) as materialized:
            candidate_manifest = inventory_baseline(
                promoted, {candidate_id: materialized}
            )
            execution = verify_baseline(
                promoted,
                candidate_manifest,
                materialized,
                compatibility_binary,
                worker,
                site_packages,
            )
        changed = changed_script_paths(fixed_manifest, candidate_manifest)
        unverified: list[dict[str, object]] = []
        deterministic: list[str] = []
        for raw_script in require_sequence(
            execution.get("scripts"), "executed scripts"
        ):
            script = require_mapping(raw_script, "executed script")
            path = require_string(script.get("path"), "executed script path")
            if path not in changed:
                continue
            hardware_domains = [
                require_string(evidence.get("id"), "runtime evidence id")
                for raw_evidence in require_sequence(
                    script.get("runtime_evidence"), "runtime evidence"
                )
                for evidence in [require_mapping(raw_evidence, "runtime evidence")]
                if evidence.get("mode") == "explicit_hardware_gate"
            ]
            if hardware_domains:
                unverified.append({"path": path, "domains": hardware_domains})
            else:
                deterministic.append(path)
        runtime_result = "passed" if not unverified else "unverified"
        return {
            "schema_version": 1,
            "candidate_id": candidate_id,
            "baseline_id": baseline.identifier,
            "repository": repository,
            "commit": commit,
            "result": "passed" if not unverified else "unverified",
            "full_fixed_chain": fixed_chain,
            "runtime": {
                "result": runtime_result,
                "changed_or_added": len(changed),
                "deterministic_scripts": sorted(deterministic),
                "unverified": len(unverified),
                "hardware_gates": unverified,
            },
            "api_surface": {
                "result": "non_breaking",
                "breaking_changes": [],
                "protocol_sha256": file_sha256(protocol_path),
                "verification": "generated_contract_and_managed_import_discovery",
            },
            "fixture_catalog": FIXTURE_CATALOG,
            "corpus_manifest": candidate_manifest,
            "candidate_execution": execution,
        }
    except (OSError, RuntimeError, ValueError, json.JSONDecodeError) as error:
        return {
            "schema_version": 1,
            "candidate_id": candidate_id,
            "baseline_id": baseline.identifier,
            "repository": repository,
            "commit": commit,
            "result": "failed",
            "full_fixed_chain": fixed_chain,
            "runtime": {"result": "failed", "unverified": 1},
            "api_surface": {
                "result": "unverified",
                "breaking_changes": [],
                "protocol_sha256": file_sha256(protocol_path),
            },
            "diagnostic_code": type(error).__name__,
        }


def evaluate_pending(
    registry_path: Path,
    manifest_path: Path,
    fixed_output: Path,
    candidates_path: Path,
    history_path: Path,
    result_directory: Path,
    compatibility_binary: Path,
    worker: Path,
    site_packages: Path,
    protocol_path: Path,
    repository_overrides: Mapping[str, Path],
    timestamp: str | None,
) -> list[dict[str, object]]:
    candidates = load_candidates(candidates_path)
    records = load_history(history_path)
    validate_history(records, candidates)
    validate_decision_results(records, result_directory)
    decided = {
        require_string(record.get("candidate_id"), "candidate_id")
        for record in records[1:]
    }
    pending = [
        candidate
        for identifier, candidate in candidates.items()
        if identifier not in decided
    ]
    if not pending:
        return []

    fixed_results = build_results(
        registry_path,
        manifest_path,
        repository_overrides,
        compatibility_binary,
        worker,
        site_packages,
    )
    expected = serialized_results(fixed_results)
    if (
        not fixed_output.is_file()
        or fixed_output.read_text(encoding="utf-8") != expected
    ):
        invalid_value("fixed compatibility results drifted before rolling evaluation")
    fixed_manifest = load_manifest(manifest_path)
    fixed_rows = {
        require_string(row.get("id"), "fixed manifest id"): row
        for raw_row in require_sequence(
            fixed_manifest.get("baselines"), "fixed baselines"
        )
        for row in [require_mapping(raw_row, "fixed manifest baseline")]
    }
    baselines = {
        baseline.identifier: baseline for baseline in load_baselines(registry_path)
    }
    decision_timestamp = timestamp or datetime.now(UTC).isoformat().replace(
        "+00:00", "Z"
    )
    decisions: list[dict[str, object]] = []
    for candidate in pending:
        candidate_id = require_string(candidate.get("id"), "candidate id")
        baseline_id = require_string(candidate.get("baseline_id"), "baseline_id")
        baseline = baselines.get(baseline_id)
        fixed_row = fixed_rows.get(baseline_id)
        if baseline is None or fixed_row is None:
            invalid_value(
                f"candidate {candidate_id} has an unknown baseline {baseline_id}"
            )
        report = evaluate_candidate(
            candidate,
            baseline,
            fixed_row,
            fixed_results,
            compatibility_binary,
            worker,
            site_packages,
            protocol_path,
            repository_overrides,
        )
        persist_result(result_directory, candidate_id, report)
        decision = append_decision(
            history_path,
            records,
            candidate,
            report,
            decision_timestamp,
        )
        records.append(decision)
        decisions.append(decision)
    validate_history(records, candidates)
    validate_decision_results(records, result_directory)
    return decisions


def main() -> int:
    root = Path(__file__).resolve().parents[1]
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
        "--fixed-output", type=Path, default=root / "compatibility/fixed-results.json"
    )
    parser.add_argument(
        "--candidates", type=Path, default=root / "compatibility/candidates.json"
    )
    parser.add_argument(
        "--history", type=Path, default=root / "compatibility/promotions.jsonl"
    )
    parser.add_argument("--results", type=Path, default=root / "compatibility/results")
    parser.add_argument(
        "--protocol",
        type=Path,
        default=root / "rust/pokecon-contracts/registry/protocol.json",
    )
    parser.add_argument("--compatibility-binary", type=Path)
    parser.add_argument("--worker", type=Path)
    parser.add_argument("--site-packages", type=Path)
    parser.add_argument("--repository", action="append", default=[])
    parser.add_argument("--timestamp")
    parser.add_argument("--discover-only", action="store_true")
    parser.add_argument("--evaluate-only", action="store_true")
    arguments = parser.parse_args()
    if arguments.discover_only and arguments.evaluate_only:
        parser.error("--discover-only and --evaluate-only are mutually exclusive")
    discovered: list[str] = []
    if not arguments.evaluate_only:
        discovered = discover_candidates(arguments.registry, arguments.candidates)
    decisions: list[dict[str, object]] = []
    if not arguments.discover_only:
        if (
            arguments.compatibility_binary is None
            or arguments.worker is None
            or arguments.site_packages is None
        ):
            parser.error(
                "--compatibility-binary, --worker, and --site-packages are required for evaluation"
            )
        decisions = evaluate_pending(
            arguments.registry,
            arguments.manifest,
            arguments.fixed_output,
            arguments.candidates,
            arguments.history,
            arguments.results,
            arguments.compatibility_binary,
            arguments.worker,
            arguments.site_packages,
            arguments.protocol,
            parse_repository_overrides(arguments.repository),
            arguments.timestamp,
        )
    print(
        json.dumps(
            {"discovered": discovered, "decisions": decisions},
            ensure_ascii=False,
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
