from __future__ import annotations

import json
import sys
from typing import TYPE_CHECKING

import pytest

from scripts.ci import nix_evidence
from scripts.ci.nix_evidence import (
    ACT_BUILD,
    ACT_COPY_PATH,
    ACT_SUBSTITUTE,
    EvidenceError,
    build_report,
    collect_evidence,
    serialized_report,
)

if TYPE_CHECKING:
    from pathlib import Path

DRV = "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-pokecon.drv"
SOURCE = "/nix/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-pokecon-source"
COPIED = "/nix/store/cccccccccccccccccccccccccccccccc-pokecon-runtime"


def event(action: str, **values: object) -> str:
    return json.dumps({"action": action, **values}, separators=(",", ":"))


def start(activity_id: int, activity_type: int, fields: list[dict[str, object]]) -> str:
    return event("start", id=activity_id, type=activity_type, fields=fields)


def stop(activity_id: int) -> str:
    return event("stop", id=activity_id)


def valid_log() -> str:
    return "\n".join(
        [
            start(1, ACT_BUILD, [{"s": DRV}, {"s": ""}]),
            start(2, ACT_SUBSTITUTE, [{"s": SOURCE}, {"s": "https://cache.example"}]),
            start(3, ACT_COPY_PATH, [{"s": COPIED}, {"s": "src"}, {"s": "dst"}]),
            event("result", id=1, type=101, fields=[]),
            event("msg", level="info", msg="text is deliberately ignored"),
            stop(3),
            stop(2),
            stop(1),
        ]
    )


def test_collects_relevant_paths_without_msg_matching() -> None:
    evidence = collect_evidence(valid_log())

    assert evidence == {
        "activity_count": 8,
        "built_derivations": [DRV],
        "copied_store_paths": [COPIED],
        "substituted_store_paths": [SOURCE],
    }


def test_ignores_child_task_noise_but_requires_structured_events() -> None:
    noisy_log = (
        f"running 475 tests\n{valid_log()}\ntest result: ok. 475 passed; 0 failed"
    )

    assert collect_evidence(noisy_log) == collect_evidence(valid_log())


def test_collection_is_deterministic_and_deduplicates_paths() -> None:
    log = "\n".join(
        [
            start(2, ACT_SUBSTITUTE, [{"s": SOURCE}, {"s": "cache"}]),
            start(1, ACT_BUILD, [{"s": DRV}]),
            stop(1),
            stop(2),
            start(4, ACT_SUBSTITUTE, [{"s": SOURCE}, {"s": "cache"}]),
            stop(4),
        ]
    )

    assert collect_evidence(log)["substituted_store_paths"] == [SOURCE]


@pytest.mark.parametrize(
    "log, message",
    [
        ("not-json", "no structured Nix events"),
        ('{"action":"start"', "not valid JSON"),
        (start(1, ACT_BUILD, [{"s": DRV}]), "unterminated"),
        (
            "\n".join(
                [start(1, ACT_BUILD, [{"s": DRV}]), start(1, ACT_BUILD, [{"s": DRV}])]
            ),
            "duplicate",
        ),
        (event("unknown", id=1), "unsupported action"),
    ],
)
def test_rejects_incomplete_or_unsupported_streams(log: str, message: str) -> None:
    with pytest.raises(EvidenceError, match=message):
        collect_evidence(log)


def test_accepts_nix_prefix_and_raw_internal_json_fields() -> None:
    log = (
        "\n".join(
            [
                "@nix " + start(7, ACT_SUBSTITUTE, [{"s": SOURCE}, {"s": "cache"}]),
                "@nix " + event("result", id=7, type=105, fields=[0, 0, 0, 0]),
                "@nix " + stop(7),
            ]
        )
        .replace('{"s": "' + SOURCE + '"}', '"' + SOURCE + '"')
        .replace('{"s": "cache"}', '"cache"')
    )

    assert collect_evidence(log)["substituted_store_paths"] == [SOURCE]


def test_copy_path_is_not_substitution() -> None:
    evidence = collect_evidence(
        "\n".join([start(1, ACT_COPY_PATH, [{"s": COPIED}]), stop(1)])
    )

    assert evidence["copied_store_paths"] == [COPIED]
    assert evidence["substituted_store_paths"] == []


def test_report_is_incomplete_without_command_success_or_fallback_disable() -> None:
    parsed = collect_evidence(valid_log())
    incomplete = build_report(
        job_name="job",
        parsed=parsed,
        observed_command_success=False,
        fallback_disabled=True,
    )
    fallback_enabled = build_report(
        job_name="job",
        parsed=parsed,
        observed_command_success=True,
        fallback_disabled=False,
    )

    assert incomplete["capture_complete"] is False
    assert fallback_enabled["capture_complete"] is False


def test_serialized_report_is_canonical() -> None:
    report = build_report(
        job_name="Rust and contracts (Linux)",
        parsed=collect_evidence(valid_log()),
        observed_command_success=True,
        fallback_disabled=True,
    )

    serialized = serialized_report(report)
    assert serialized.endswith("\n")
    assert "\n" not in serialized[:-1]
    assert json.loads(serialized) == report
    assert serialized == serialized_report(dict(reversed(report.items())))
    assert report["capture_complete"] is True
    assert report["schema"] == "nix-evidence/1"


def test_cli_writes_report_and_fails_closed_for_incomplete_log(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    log_path = tmp_path / "events.jsonl"
    output_path = tmp_path / "evidence.json"
    log_path.write_text(valid_log(), encoding="utf-8")
    monkeypatch.setattr(
        sys,
        "argv",
        [
            "nix_evidence",
            "--log",
            str(log_path),
            "--output",
            str(output_path),
            "--job-name",
            "job",
            "--observed-command-success",
            "true",
            "--fallback-disabled",
            "true",
        ],
    )

    assert nix_evidence.main() == 0
    assert (
        json.loads(output_path.read_text(encoding="utf-8"))["capture_complete"] is True
    )
