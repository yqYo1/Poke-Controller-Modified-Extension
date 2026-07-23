"""Validate and append compatibility candidate promotion decisions."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
from datetime import UTC, datetime
from pathlib import Path
from typing import TYPE_CHECKING, Never

from scripts.compatibility_inventory import (
    require_mapping,
    require_sequence,
    require_string,
)

if TYPE_CHECKING:
    from collections.abc import Mapping


def invalid_value(message: str) -> Never:
    raise ValueError(message)


def canonical_sha256(value: Mapping[str, object]) -> str:
    encoded = json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode()
    return hashlib.sha256(encoded).hexdigest()


def record_sha256(record: Mapping[str, object]) -> str:
    unsigned = {key: value for key, value in record.items() if key != "record_sha256"}
    return canonical_sha256(unsigned)


def load_candidates(path: Path) -> dict[str, dict[str, object]]:
    raw: object = json.loads(path.read_text(encoding="utf-8"))
    document = require_mapping(raw, "compatibility candidates")
    if document.get("schema_version") != 1:
        invalid_value("compatibility candidates schema_version must be 1")
    candidates: dict[str, dict[str, object]] = {}
    for index, raw_candidate in enumerate(
        require_sequence(document.get("candidates"), "candidates")
    ):
        candidate = require_mapping(raw_candidate, f"candidates[{index}]")
        identifier = require_string(candidate.get("id"), "candidate id")
        commit = require_string(candidate.get("commit"), "candidate commit")
        require_string(candidate.get("repository"), "candidate repository")
        require_string(candidate.get("baseline_id"), "candidate baseline_id")
        if re.fullmatch(r"[a-z0-9][a-z0-9._-]*", identifier) is None:
            invalid_value(f"candidate id is not a safe component: {identifier}")
        if len(commit) != 40 or not all(
            character in "0123456789abcdef" for character in commit
        ):
            invalid_value(
                f"candidate {identifier} commit must be a lowercase full Git SHA"
            )
        if identifier in candidates:
            invalid_value(f"duplicate candidate id: {identifier}")
        candidates[identifier] = candidate
    return candidates


def load_history(path: Path) -> list[dict[str, object]]:
    if not path.is_file():
        invalid_value(f"promotion history does not exist: {path}")
    records: list[dict[str, object]] = []
    for line_number, line in enumerate(
        path.read_text(encoding="utf-8").splitlines(), 1
    ):
        if not line:
            invalid_value(f"promotion history line {line_number} is empty")
        raw: object = json.loads(line)
        records.append(require_mapping(raw, f"promotion history line {line_number}"))
    if not records:
        invalid_value("promotion history must contain a genesis record")
    return records


def validate_history(
    records: list[dict[str, object]], candidates: Mapping[str, object]
) -> None:
    previous: str | None = None
    decided: set[str] = set()
    for sequence, record in enumerate(records):
        if record.get("schema_version") != 1:
            invalid_value(f"promotion record {sequence} schema_version must be 1")
        if record.get("sequence") != sequence:
            invalid_value(f"promotion record {sequence} has a non-contiguous sequence")
        if record.get("previous_sha256") != previous:
            invalid_value(f"promotion record {sequence} breaks the hash chain")
        digest = require_string(record.get("record_sha256"), "record_sha256")
        if digest != record_sha256(record):
            invalid_value(f"promotion record {sequence} digest differs")
        kind = require_string(record.get("kind"), "promotion kind")
        if sequence == 0:
            if kind != "genesis" or record.get("fixed_baselines_immutable") is not True:
                invalid_value("promotion history must start with immutable genesis")
        else:
            if kind not in {"promoted", "quarantined"}:
                invalid_value(f"promotion record {sequence} has an invalid decision")
            candidate_id = require_string(record.get("candidate_id"), "candidate_id")
            if candidate_id not in candidates:
                invalid_value(
                    f"promotion record {sequence} references an unknown candidate"
                )
            if candidate_id in decided:
                invalid_value(f"candidate {candidate_id} has multiple decisions")
            decided.add(candidate_id)
        previous = digest


def validate_decision_results(
    records: list[dict[str, object]], result_directory: Path
) -> None:
    for sequence, record in enumerate(records[1:], 1):
        candidate_id = require_string(record.get("candidate_id"), "candidate_id")
        result_path = result_directory / f"{candidate_id}.json"
        if not result_path.is_file():
            invalid_value(
                f"promotion record {sequence} has no durable result: {result_path}"
            )
        raw: object = json.loads(result_path.read_text(encoding="utf-8"))
        result = require_mapping(raw, f"promotion result {candidate_id}")
        if canonical_sha256(result) != record.get("result_sha256"):
            invalid_value(f"promotion result {candidate_id} digest differs")
        if record.get("kind") == "promoted":
            if result.get("result") != "passed":
                invalid_value(f"promoted candidate {candidate_id} did not pass")
            corpus = require_mapping(result.get("corpus_manifest"), "corpus_manifest")
            if corpus.get("id") != candidate_id:
                invalid_value(
                    f"promoted candidate {candidate_id} corpus identity differs"
                )


def persist_result(
    result_directory: Path, candidate_id: str, result: Mapping[str, object]
) -> Path:
    result_directory.mkdir(parents=True, exist_ok=True)
    result_path = result_directory / f"{candidate_id}.json"
    encoded = json.dumps(result, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    if result_path.exists():
        if result_path.read_text(encoding="utf-8") != encoded:
            invalid_value(f"candidate result is immutable once written: {result_path}")
        return result_path
    temporary = result_path.with_suffix(".json.tmp")
    temporary.write_text(encoded, encoding="utf-8", newline="\n")
    os.replace(temporary, result_path)
    return result_path


def promotion_failures(
    candidate: Mapping[str, object], result: Mapping[str, object]
) -> list[str]:
    failures: list[str] = []
    if result.get("schema_version") != 1:
        failures.append("invalid_result_schema")
    if result.get("candidate_id") != candidate.get("id"):
        failures.append("candidate_identity_mismatch")
    if result.get("commit") != candidate.get("commit"):
        failures.append("candidate_commit_mismatch")
    if result.get("result") != "passed":
        failures.append("candidate_verification_failed")
    fixed_chain = require_mapping(result.get("full_fixed_chain"), "full_fixed_chain")
    if fixed_chain.get("result") != "passed" or fixed_chain.get("baseline_count") != 3:
        failures.append("full_three_repository_chain_failed")
    runtime = require_mapping(result.get("runtime"), "runtime")
    if runtime.get("result") != "passed" or runtime.get("unverified") not in {0, None}:
        failures.append("runtime_unverified")
    api_surface = require_mapping(result.get("api_surface"), "api_surface")
    breaking = require_sequence(api_surface.get("breaking_changes"), "breaking_changes")
    if breaking:
        failures.append("breaking_api_change")
    if api_surface.get("result") != "non_breaking":
        failures.append("api_surface_unverified")
    return failures


def append_decision(
    history_path: Path,
    records: list[dict[str, object]],
    candidate: Mapping[str, object],
    result: Mapping[str, object],
    timestamp: str,
) -> dict[str, object]:
    failures = promotion_failures(candidate, result)
    previous = require_string(records[-1].get("record_sha256"), "record_sha256")
    record: dict[str, object] = {
        "schema_version": 1,
        "sequence": len(records),
        "previous_sha256": previous,
        "kind": "quarantined" if failures else "promoted",
        "candidate_id": require_string(candidate.get("id"), "candidate id"),
        "baseline_id": require_string(candidate.get("baseline_id"), "baseline_id"),
        "repository": require_string(candidate.get("repository"), "repository"),
        "commit": require_string(candidate.get("commit"), "commit"),
        "result_sha256": canonical_sha256(result),
        "timestamp": timestamp,
        "reasons": failures,
    }
    record["record_sha256"] = record_sha256(record)
    encoded = json.dumps(
        record, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    )
    with history_path.open("a", encoding="utf-8", newline="\n") as output:
        output.write(f"{encoded}\n")
        output.flush()
        os.fsync(output.fileno())
    return record


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--candidates", type=Path, default=root / "compatibility/candidates.json"
    )
    parser.add_argument(
        "--history", type=Path, default=root / "compatibility/promotions.jsonl"
    )
    parser.add_argument("--results", type=Path, default=root / "compatibility/results")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--candidate")
    parser.add_argument("--result", type=Path)
    parser.add_argument("--timestamp")
    arguments = parser.parse_args()

    candidates = load_candidates(arguments.candidates)
    records = load_history(arguments.history)
    validate_history(records, candidates)
    validate_decision_results(records, arguments.results)
    if arguments.check:
        if arguments.candidate is not None or arguments.result is not None:
            parser.error("--check cannot be combined with a candidate decision")
        return 0
    if arguments.candidate is None or arguments.result is None:
        parser.error("--candidate and --result are required unless --check is used")
    candidate = candidates.get(arguments.candidate)
    if candidate is None:
        parser.error(f"candidate does not exist: {arguments.candidate}")
    if any(record.get("candidate_id") == arguments.candidate for record in records):
        parser.error(f"candidate already has a decision: {arguments.candidate}")
    raw_result: object = json.loads(arguments.result.read_text(encoding="utf-8"))
    result = require_mapping(raw_result, "candidate result")
    persist_result(arguments.results, arguments.candidate, result)
    timestamp = arguments.timestamp or datetime.now(UTC).isoformat().replace(
        "+00:00", "Z"
    )
    decision = append_decision(arguments.history, records, candidate, result, timestamp)
    print(json.dumps(decision, ensure_ascii=False, sort_keys=True))
    return 0 if decision["kind"] == "promoted" else 2


if __name__ == "__main__":
    raise SystemExit(main())
