"""Parse Nix internal-json activity logs into deterministic CI evidence."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import TYPE_CHECKING, Final, Never, cast

if TYPE_CHECKING:
    from collections.abc import Mapping, Sequence

SCHEMA: Final = "nix-evidence/1"
ACT_COPY_PATH: Final = 100
ACT_BUILD: Final = 105
ACT_SUBSTITUTE: Final = 108
RELEVANT_ACTIONS: Final = frozenset({"start", "stop", "result", "msg"})
STORE_PREFIX: Final = "/nix/store/"


class EvidenceError(ValueError):
    """The input is not a supported Nix internal-json stream."""


def _fail(message: str) -> Never:
    raise EvidenceError(message)


def _fail_from(message: str, cause: BaseException) -> Never:
    raise EvidenceError(message) from cause


JsonObject = dict[str, object]


def _object(value: object, context: str) -> JsonObject:
    if not isinstance(value, dict):
        _fail(f"{context} must be a JSON object")
    raw_mapping = cast("dict[object, object]", value)
    if any(not isinstance(key, str) for key in raw_mapping):
        _fail(f"{context} must be a JSON object")
    return {cast("str", key): item for key, item in raw_mapping.items()}


def _string(value: object, context: str) -> str:
    if not isinstance(value, str) or not value:
        _fail(f"{context} must be a non-empty string")
    return value


def _integer(value: object, context: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        _fail(f"{context} must be an integer")
    return value


def _store_path(value: object, context: str) -> str:
    path = _string(value, context)
    if not path.startswith(STORE_PREFIX) or "\n" in path or "\r" in path:
        _fail(f"{context} must be an absolute Nix store path")
    return path


def _fields(value: object, context: str) -> tuple[object, ...]:
    if not isinstance(value, list):
        _fail(f"{context} must be an array")
    parsed: list[object] = []
    raw_fields = cast("list[object]", value)
    for index, raw_field in enumerate(raw_fields):
        # Nix 2.34 emits fields as raw JSON strings/integers.  Some older
        # structured-log producers encode the same values as {"s": ...} or
        # {"i": ...}; accept both without consulting the human text field.
        if isinstance(raw_field, str):
            parsed.append(raw_field)
            continue
        if isinstance(raw_field, int) and not isinstance(raw_field, bool):
            parsed.append(raw_field)
            continue
        field = _object(raw_field, f"{context}[{index}]")
        if len(field) != 1:
            _fail(f"{context}[{index}] must contain one typed value")
        key, typed_value = next(iter(field.items()))
        if key == "s":
            if not isinstance(typed_value, str):
                _fail(f"{context}[{index}].s must be a string")
            parsed.append(typed_value)
        elif key == "i":
            parsed.append(_integer(typed_value, f"{context}[{index}].i"))
        else:
            _fail(f"{context}[{index}] has unsupported field type {key!r}")
    return tuple(parsed)


def _first_string(fields: Sequence[object], context: str) -> str:
    if not fields or not isinstance(fields[0], str):
        _fail(f"{context} requires a string as its first field")
    return _store_path(fields[0], context)


def _parse_line(raw_line: str, line_number: int) -> JsonObject:
    payload = raw_line.strip()
    if payload.startswith("@nix "):
        payload = payload[5:]
    try:
        value = json.loads(
            payload,
            parse_constant=lambda value: (_ for _ in ()).throw(ValueError(value)),
        )
    except (json.JSONDecodeError, ValueError) as error:
        _fail_from(f"line {line_number} is not valid JSON", error)
    return _object(value, f"line {line_number}")


def parse_internal_json(text: str) -> tuple[dict[str, object], ...]:
    """Parse JSONL, reporting noise/malformed lines as a caller-visible error."""
    events: list[dict[str, object]] = []
    for line_number, raw_line in enumerate(text.splitlines(), start=1):
        if not raw_line.strip():
            continue
        stripped = raw_line.strip()
        if not stripped.startswith(("{", "@nix {")):
            _fail(f"line {line_number} is non-JSON noise")
        event = _parse_line(raw_line, line_number)
        action = event.get("action")
        if not isinstance(action, str) or action not in RELEVANT_ACTIONS:
            _fail(f"line {line_number} has unsupported action")
        events.append(event)
    return tuple(events)


def _activity(
    event: Mapping[str, object], context: str
) -> tuple[int, int, tuple[object, ...]]:
    activity_id = _integer(event.get("id"), f"{context}.id")
    activity_type = _integer(event.get("type"), f"{context}.type")
    fields = _fields(event.get("fields", []), f"{context}.fields")
    return activity_id, activity_type, fields


def collect_evidence(text: str) -> dict[str, object]:
    """Collect path observations and parser completeness from one Nix stream."""
    events = parse_internal_json(text)
    active: dict[int, tuple[int, tuple[object, ...]]] = {}
    stopped: set[int] = set()
    built: set[str] = set()
    substituted: set[str] = set()
    copied: set[str] = set()
    for index, event in enumerate(events, start=1):
        action = _string(event.get("action"), f"event {index}.action")
        if action == "start":
            activity_id, activity_type, fields = _activity(event, f"event {index}")
            if activity_id in active or activity_id in stopped:
                _fail(f"duplicate activity id {activity_id}")
            active[activity_id] = (activity_type, fields)
            if activity_type == ACT_BUILD:
                built.add(_first_string(fields, f"event {index}.fields"))
            elif activity_type == ACT_SUBSTITUTE:
                path = _first_string(fields, f"event {index}.fields")
                if len(fields) < 2 or not isinstance(fields[1], str):
                    _fail(f"event {index}.fields requires a substituter URI")
                substituted.add(path)
            elif activity_type == ACT_COPY_PATH:
                copied.add(_first_string(fields, f"event {index}.fields"))
        elif action == "stop":
            activity_id = _integer(event.get("id"), f"event {index}.id")
            if activity_id not in active or activity_id in stopped:
                _fail(f"stop without a unique start for activity {activity_id}")
            stopped.add(activity_id)
        elif action == "result":
            _integer(event.get("id"), f"event {index}.id")
            _integer(event.get("type"), f"event {index}.type")
            _fields(event.get("fields", []), f"event {index}.fields")
        else:  # msg
            level = event.get("level")
            if not (
                isinstance(level, str)
                or (isinstance(level, int) and not isinstance(level, bool))
            ):
                _fail(f"event {index}.level must be a string or integer")
            _string(event.get("msg"), f"event {index}.msg")
    if active.keys() != stopped:
        missing = sorted(active.keys() - stopped)
        _fail(f"unterminated activities: {missing}")
    return {
        "built_derivations": sorted(built),
        "copied_store_paths": sorted(copied),
        "substituted_store_paths": sorted(substituted),
        "activity_count": len(events),
    }


def _bool_argument(value: str) -> bool:
    normalized = value.strip().lower()
    if normalized == "true":
        return True
    if normalized == "false":
        return False
    message = "expected true or false"
    raise argparse.ArgumentTypeError(message)


def _string_list(value: object, context: str) -> list[str]:
    if not isinstance(value, list):
        _fail(f"{context} must be a list of strings")
    raw_values = cast("list[object]", value)
    if any(not isinstance(item, str) for item in raw_values):
        _fail(f"{context} must be a list of strings")
    return sorted(cast("list[str]", raw_values))


def build_report(
    *,
    job_name: str,
    parsed: Mapping[str, object],
    observed_command_success: bool,
    fallback_disabled: bool,
    parser_error: str | None = None,
) -> dict[str, object]:
    complete = parser_error is None and observed_command_success and fallback_disabled
    built_derivations = _string_list(
        parsed.get("built_derivations", []), "built_derivations"
    )
    copied_store_paths = _string_list(
        parsed.get("copied_store_paths", []), "copied_store_paths"
    )
    substituted_store_paths = _string_list(
        parsed.get("substituted_store_paths", []), "substituted_store_paths"
    )
    report: dict[str, object] = {
        "activity_count": parsed.get("activity_count", 0),
        "built_derivations": built_derivations,
        "capture_complete": complete,
        "copied_store_paths": copied_store_paths,
        "fallback_disabled": fallback_disabled,
        "job_name": _string(job_name, "job_name"),
        "observed_command_success": observed_command_success,
        "schema": SCHEMA,
        "substituted_store_paths": substituted_store_paths,
    }
    if parser_error is not None:
        report["error"] = parser_error
    return report


def serialized_report(report: Mapping[str, object]) -> str:
    """Serialize a report canonically as one JSON object followed by LF."""
    return (
        json.dumps(report, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
        + "\n"
    )


def _arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--log", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--job-name", required=True)
    parser.add_argument(
        "--observed-command-success", type=_bool_argument, required=True
    )
    parser.add_argument("--fallback-disabled", type=_bool_argument, required=True)
    return parser.parse_args()


def main() -> int:
    arguments = _arguments()
    parser_error: str | None = None
    parsed: dict[str, object] = {
        "activity_count": 0,
        "built_derivations": [],
        "copied_store_paths": [],
        "substituted_store_paths": [],
    }
    try:
        parsed = collect_evidence(arguments.log.read_text(encoding="utf-8"))
    except (OSError, EvidenceError) as error:
        parser_error = str(error)
    report = build_report(
        job_name=arguments.job_name,
        parsed=parsed,
        observed_command_success=arguments.observed_command_success,
        fallback_disabled=arguments.fallback_disabled,
        parser_error=parser_error,
    )
    arguments.output.parent.mkdir(parents=True, exist_ok=True)
    arguments.output.write_text(serialized_report(report), encoding="utf-8")
    if parser_error is not None or not report["capture_complete"]:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
