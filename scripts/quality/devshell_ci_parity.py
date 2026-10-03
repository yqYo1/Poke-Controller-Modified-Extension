#!/usr/bin/env python3
"""Standalone fail-closed devShell/CI parity report generator (AR-13.1-24).

Produces a deterministic JSON report distinguishing a dirty local snapshot
from a clean tracked-source result. Never claims parity when required
fields are unavailable.

Read-only by design: only ``git rev-parse``/``git ls-files``/``git status``
and ``nix eval``/``nix path-info``/``nix flake show`` are invoked. No commits, no
pushes, no network fetches. Standard library + subprocess only.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shlex
import subprocess
import sys
from collections.abc import Callable, Sequence
from datetime import UTC, datetime
from pathlib import Path
from typing import Final, cast

SCHEMA: Final = "devshell-ci-parity/v1"
REPORT_VERSION: Final = 1
DEFAULT_MAX_TASKS: Final = 64
SUBPROCESS_TIMEOUT: Final = 120

SNAPSHOT_CLEAN: Final = "clean-tracked-source"
SNAPSHOT_DIRTY: Final = "dirty-local-snapshot"

VERDICT_PARITY: Final = "parity"
VERDICT_NON_PARITY: Final = "non-parity"

_HEX40: Final = re.compile(r"\A[0-9a-f]{40}\Z")
_HEX64: Final = re.compile(r"\A[0-9a-f]{64}\Z")
_FLAKE_ATTR: Final = re.compile(r"\.#([A-Za-z0-9_][A-Za-z0-9._-]*)")
_SECRET_KEY: Final = re.compile(
    r"token|secret|password|api[_-]?key|bearer", re.IGNORECASE
)


class ReportError(Exception):
    """Fail-closed validation or collection error."""


JsonObject = dict[str, object]


def _run(
    argv: Sequence[str], cwd: Path, timeout: int = SUBPROCESS_TIMEOUT
) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(  # noqa: S603 - argv lists only; never shell-expanded
            list(argv),
            cwd=str(cwd),
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
    except (OSError, subprocess.SubprocessError) as exc:
        return subprocess.CompletedProcess(
            list(argv),
            127,
            "",
            f"failed to run {' '.join(list(argv)[:3])}: {exc}",
        )


def git_head(repository: Path) -> str:
    proc = _run(["git", "rev-parse", "HEAD"], repository)
    if proc.returncode != 0:
        msg = f"git rev-parse HEAD failed: {proc.stderr.strip()[:300]}"
        raise ReportError(msg)
    head = proc.stdout.strip()
    if not _HEX40.match(head):
        msg = "git HEAD is not a 40-hex object id"
        raise ReportError(msg)
    return head


def git_tracked_files(repository: Path) -> list[dict[str, str]]:
    proc = _run(["git", "ls-files", "-s"], repository)
    if proc.returncode != 0:
        msg = f"git ls-files failed: {proc.stderr.strip()[:300]}"
        raise ReportError(msg)
    entries: list[dict[str, str]] = []
    for line in proc.stdout.splitlines():
        if not line.strip():
            continue
        try:
            meta, path = line.split("\t", 1)
            mode, oid, _stage = meta.split()
        except ValueError as exc:
            msg = f"unparsable git ls-files line: {line[:120]!r}"
            raise ReportError(msg) from exc
        if not _HEX40.match(oid):
            msg = f"non-hex object id for {path!r}"
            raise ReportError(msg)
        if not path or path.startswith("/"):
            msg = f"unsafe tracked path: {path!r}"
            raise ReportError(msg)
        entries.append({"mode": mode, "oid": oid, "path": path})
    if not entries:
        msg = "no tracked source files found"
        raise ReportError(msg)
    entries.sort(key=lambda item: item["path"])
    return entries


def git_status_lines(repository: Path) -> list[str]:
    proc = _run(["git", "status", "--porcelain=v1", "-uall"], repository)
    if proc.returncode != 0:
        msg = f"git status failed: {proc.stderr.strip()[:300]}"
        raise ReportError(msg)
    return sorted(line for line in proc.stdout.splitlines() if line.strip())


def identity_of_argv(argv: list[str]) -> str:
    return hashlib.sha256("\0".join(argv).encode("utf-8")).hexdigest()


def parse_task_spec(spec: str) -> tuple[str, str]:
    if "::" in spec:
        name, command = spec.split("::", 1)
    elif "=" in spec:
        name, command = spec.split("=", 1)
    else:
        msg = f"task {spec!r} needs a NAME::COMMAND or NAME=COMMAND delimiter"
        raise ReportError(msg)
    name, command = name.strip(), command.strip()
    if not name or not command:
        msg = f"task {spec!r} has an empty name or command"
        raise ReportError(msg)
    return name, command


def split_command(command: str) -> list[str]:
    try:
        argv = shlex.split(command, posix=True)
    except ValueError as exc:
        msg = f"unparsable command {command!r}: {exc}"
        raise ReportError(msg) from exc
    if not argv:
        msg = f"empty command: {command!r}"
        raise ReportError(msg)
    return argv


def infer_flake_attr(command: str) -> str | None:
    match = _FLAKE_ATTR.search(command)
    return match.group(1) if match else None


def _probe_store_path(store_path: str, repository: Path) -> dict[str, str]:
    """Probe one already-realized store path without building or guessing."""
    if not store_path.startswith("/nix/store/") or store_path.count("/") < 3:
        return {"reason": f"unsafe store path: {store_path}", "status": "unavailable"}
    proc = _run(
        ["nix", "path-info", "--json", "--json-format", "2", store_path],
        repository,
    )
    if proc.returncode != 0:
        reason = proc.stderr.strip().splitlines()
        detail = reason[-1][:200] if reason else f"exit {proc.returncode}"
        return {"reason": f"nix path-info failed: {detail}", "status": "unavailable"}
    hash_proc = _run(
        ["nix", "hash", "path", "--type", "sha256", store_path], repository
    )
    nar_hash = hash_proc.stdout.strip()
    if hash_proc.returncode != 0 or not nar_hash:
        detail = hash_proc.stderr.strip().splitlines()
        return {
            "reason": (
                f"no narHash for {store_path}; nix hash path failed: "
                f"{detail[-1][:200] if detail else f'exit {hash_proc.returncode}'}"
            ),
            "status": "unavailable",
        }
    return {"hash": nar_hash, "status": "available", "store_path": store_path}


def _app_program_store_path(
    _flake_ref: str, app_name: str, repository: Path
) -> tuple[str | None, str | None]:
    """Resolve an app's launcher to its store root for NAR probing.

    Flake apps are attribute sets, not derivations, so ``nix path-info`` cannot
    query ``apps.<system>.<name>`` directly. The app's ``program`` field is the
    launcher path that can be hashed once the task has built it.
    """
    system_proc = _run(
        ["nix", "eval", "--raw", "--impure", "--expr", "builtins.currentSystem"],
        repository,
    )
    system = system_proc.stdout.strip()
    if system_proc.returncode != 0 or not re.fullmatch(r"[A-Za-z0-9_-]+", system):
        detail = system_proc.stderr.strip().splitlines()
        return None, (
            "could not determine Nix system: "
            f"{detail[-1][:200] if detail else f'exit {system_proc.returncode}'}"
        )
    # Nix system and app names may contain punctuation with meaning in an
    # attribute path (for example, ``x86_64-linux`` or ``app.with.dot``).
    # Quote both keys so Nix selects the literal attrset entries instead of
    # parsing them as operators or nested attributes.
    program_expr = (
        f"let flake = builtins.getFlake {json.dumps(str(repository.resolve()))}; "
        f"in flake.apps.{json.dumps(system)}.{json.dumps(app_name)}.program"
    )
    program_proc = _run(
        ["nix", "eval", "--raw", "--impure", "--expr", program_expr],
        repository,
    )
    program = program_proc.stdout.strip()
    if program_proc.returncode != 0 or not program.startswith("/nix/store/"):
        detail = program_proc.stderr.strip().splitlines()
        return None, (
            f"app program lookup failed for {app_name}: "
            f"{detail[-1][:200] if detail else f'exit {program_proc.returncode}'}"
        )
    store_path = program.split("/bin/", 1)[0]
    if not store_path.startswith("/nix/store/") or store_path == program:
        return None, f"app program is not a store launcher path: {program}"
    return store_path, None


def probe_nar_hash(
    flake_ref: str,
    attr: str,
    repository: Path,
    *,
    app_name: str | None = None,
) -> dict[str, str]:
    """Return an available/unavailable NAR record; never fabricate a hash.

    ``nix run`` names an app, whose NAR is the launcher program rather than an
    app attribute derivation. Probe that program when the direct path is not a
    derivation, preserving an explicit unavailable result when it is unrealized.
    """
    proc = _run(
        ["nix", "path-info", "--json", "--json-format", "2", f"{flake_ref}#{attr}"],
        repository,
    )
    if proc.returncode != 0 and app_name is not None:
        store_path, app_error = _app_program_store_path(flake_ref, app_name, repository)
        if store_path is not None:
            return _probe_store_path(store_path, repository)
        return {
            "reason": app_error or f"app {app_name} is unavailable",
            "status": "unavailable",
        }
    if proc.returncode != 0:
        reason = proc.stderr.strip().splitlines()
        detail = reason[-1][:200] if reason else f"exit {proc.returncode}"
        return {"reason": f"nix path-info failed: {detail}", "status": "unavailable"}
    try:
        raw = json.loads(proc.stdout)
    except json.JSONDecodeError:
        return {
            "reason": "nix path-info returned non-JSON output",
            "status": "unavailable",
        }
    if not isinstance(raw, dict) or not raw:
        return {
            "reason": "nix path-info returned an empty result",
            "status": "unavailable",
        }
    payload = cast("JsonObject", raw)
    # Nix returns ``null`` for locally built paths until their NAR hash is
    # requested explicitly. Compute the hash from the exact store path instead
    # of substituting a plausible value.
    if "info" in payload and isinstance(payload["info"], dict):
        info_payload = cast("JsonObject", payload["info"])
        store_dir_value = info_payload.get("storeDir", "/nix/store")
        store_dir = (
            store_dir_value if isinstance(store_dir_value, str) else "/nix/store"
        )
        paths_value = info_payload.get("paths")
        if isinstance(paths_value, list) and paths_value:
            path_values = cast("list[object]", paths_value)
            path_strings: list[str] = [
                path for path in path_values if isinstance(path, str)
            ]
            if len(path_strings) != len(path_values):
                return {
                    "reason": "nix path-info returned a non-string store path",
                    "status": "unavailable",
                }
            store_path = sorted(path_strings)[0]
        else:
            # json-format 2 represents the path map as {store_path: info}.
            candidates = [key for key in info_payload if key != "storeDir"]
            if not candidates:
                return {
                    "reason": "nix path-info returned no store path",
                    "status": "unavailable",
                }
            store_path = sorted(candidates)[0]
        if not store_path.startswith("/"):
            store_path = f"{store_dir}/{store_path}"
    else:
        store_dir = "/nix/store"
        store_path = sorted(payload)[0]
        info = payload[store_path]
        if isinstance(info, dict):
            nar = cast("JsonObject", info)
            nar_hash = nar.get("narHash")
            if isinstance(nar_hash, str) and nar_hash:
                return {
                    "hash": nar_hash,
                    "status": "available",
                    "store_path": store_path,
                }
    if not store_path.startswith(f"{store_dir}/"):
        return {
            "reason": f"unsafe store path from nix path-info: {store_path}",
            "status": "unavailable",
        }
    return _probe_store_path(store_path, repository)


NarProbe = Callable[[str], dict[str, str]]


def collect_task(
    name: str,
    command: str,
    flake_ref: str,
    repository: Path,
    package_overrides: dict[str, str],
    *,
    nar_probe: NarProbe | None = None,
) -> dict[str, object]:
    argv = split_command(command)
    attr = package_overrides.get(name, infer_flake_attr(command))
    app_name = (
        attr
        if attr is not None
        and len(argv) >= 2
        and argv[0] == "nix"
        and argv[1] == "run"
        and name not in package_overrides
        else None
    )
    if attr is None:
        nar = {
            "reason": "command has no Nix store artifact (for example, nix fmt)",
            "status": "not-applicable",
        }
    elif nar_probe is not None:
        nar = nar_probe(attr)
    else:
        nar = probe_nar_hash(flake_ref, attr, repository, app_name=app_name)
    if nar.get("status") not in ("available", "unavailable", "not-applicable"):
        msg = f"task {name!r}: invalid NAR probe result"
        raise ReportError(msg)
    record: dict[str, object] = {
        "argv": argv,
        "command": command,
        "identity_sha256": identity_of_argv(argv),
        "name": name,
        "nar": nar,
    }
    if attr is not None:
        record["flake_attr"] = attr
    return record


ShowRunner = Callable[[list[str], Path], str]


def _default_show_runner(argv: list[str], cwd: Path) -> str:
    proc = _run(argv, cwd)
    if proc.returncode != 0:
        msg = f"nix flake show failed: {proc.stderr.strip()[:300]}"
        raise ReportError(msg)
    return proc.stdout


def discover_tasks(
    flake_ref: str,
    repository: Path,
    max_tasks: int,
    kinds: Sequence[str] = ("apps", "checks"),
    runner: ShowRunner | None = None,
) -> list[tuple[str, str]]:
    runner = runner or _default_show_runner
    raw = runner(["nix", "flake", "show", "--json", flake_ref], repository)
    try:
        value = json.loads(raw)
    except json.JSONDecodeError as exc:
        msg = "nix flake show returned non-JSON output"
        raise ReportError(msg) from exc
    if not isinstance(value, dict):
        msg = "nix flake show returned a non-object"
        raise ReportError(msg)
    payload = cast("JsonObject", value)
    current_system_proc = _run(
        ["nix", "eval", "--raw", "--impure", "--expr", "builtins.currentSystem"],
        repository,
    )
    current_system = (
        current_system_proc.stdout.strip()
        if current_system_proc.returncode == 0
        else ""
    )
    found: dict[str, str] = {}
    for kind in kinds:
        section = payload.get(kind, {})
        if not isinstance(section, dict):
            continue
        systems = cast("JsonObject", section)
        ordered_systems = [
            system
            for system in [current_system, *sorted(systems)]
            if system and system in systems
        ]
        ordered_systems = list(dict.fromkeys(ordered_systems))
        for system in ordered_systems:
            entries = systems[system]
            if not isinstance(entries, dict):
                continue
            names = cast("JsonObject", entries)
            for entry_name in names:
                task_name = f"{kind}.{entry_name}"
                if task_name in found:
                    continue
                if kind == "apps":
                    found[task_name] = f"nix run {flake_ref}#{entry_name}"
                else:
                    found[task_name] = (
                        f"nix build {flake_ref}#checks.{system}.{entry_name}"
                    )
    if len(found) > max_tasks:
        msg = (
            f"discovered {len(found)} tasks exceeds --max-tasks {max_tasks}; "
            "pass an explicit bounded --task list instead"
        )
        raise ReportError(msg)
    if not found:
        msg = "flake discovery returned no tasks"
        raise ReportError(msg)
    return sorted(found.items())


def build_report(
    repository: Path,
    task_specs: Sequence[str],
    *,
    flake_ref: str = ".",
    package_overrides: dict[str, str] | None = None,
    nar_probe: NarProbe | None = None,
    generated_at: str | None = None,
) -> dict[str, object]:
    if not task_specs:
        msg = "at least one task is required"
        raise ReportError(msg)
    if len(task_specs) > DEFAULT_MAX_TASKS:
        msg = f"{len(task_specs)} tasks exceeds the bound of {DEFAULT_MAX_TASKS}"
        raise ReportError(msg)
    overrides = package_overrides or {}
    head = git_head(repository)
    tracked = git_tracked_files(repository)
    status = git_status_lines(repository)
    clean = not status
    snapshot = SNAPSHOT_CLEAN if clean else SNAPSHOT_DIRTY

    tasks: list[dict[str, object]] = []
    seen: set[str] = set()
    for spec in task_specs:
        name, command = parse_task_spec(spec)
        if name in seen:
            msg = f"duplicate task name: {name!r}"
            raise ReportError(msg)
        seen.add(name)
        tasks.append(
            collect_task(
                name,
                command,
                flake_ref,
                repository,
                overrides,
                nar_probe=nar_probe,
            )
        )
    tasks.sort(key=lambda item: str(item["name"]))

    reasons: list[str] = []
    if not clean:
        reasons.append("dirty-worktree: local snapshot differs from tracked source")
    for task in tasks:
        nar_value = task["nar"]
        if not isinstance(nar_value, dict):
            msg = "task records need nar objects"
            raise ReportError(msg)
        nar = cast("JsonObject", nar_value)
        if nar.get("status") == "unavailable":
            reasons.append(
                f"task {task['name']}: nar-unavailable ({nar.get('reason')})"
            )
    verdict = VERDICT_PARITY if not reasons else VERDICT_NON_PARITY

    stamp = generated_at or datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%SZ")
    report: dict[str, object] = {
        "generated_at": stamp,
        "git": {
            "clean": clean,
            "head": head,
            "status": status,
            "tracked_files": tracked,
        },
        "parity": {"reasons": reasons, "verdict": verdict},
        "repository": str(repository),
        "schema": SCHEMA,
        "snapshot": snapshot,
        "tasks": tasks,
        "version": REPORT_VERSION,
    }
    validate_report(report)
    return report


def _reject_secrets(value: object) -> None:
    if isinstance(value, dict):
        mapping = cast("JsonObject", value)
        for key, item in mapping.items():
            if isinstance(key, str) and _SECRET_KEY.search(key):
                msg = f"report must not carry secrets (key {key!r})"
                raise ReportError(msg)
            _reject_secrets(item)
    elif isinstance(value, list):
        items = cast("list[object]", value)
        for item in items:
            _reject_secrets(item)


def validate_report(report: object) -> None:
    if not isinstance(report, dict):
        msg = "report must be a JSON object"
        raise ReportError(msg)
    doc = cast("JsonObject", report)
    if doc.get("schema") != SCHEMA:
        msg = f"unsupported schema: {doc.get('schema')!r}"
        raise ReportError(msg)
    if doc.get("version") != REPORT_VERSION:
        msg = f"unsupported version: {doc.get('version')!r}"
        raise ReportError(msg)
    if not isinstance(doc.get("repository"), str) or not doc["repository"]:
        msg = "report needs a non-empty repository path"
        raise ReportError(msg)
    stamp = doc.get("generated_at")
    if not isinstance(stamp, str) or not stamp:
        msg = "report needs a generated_at timestamp"
        raise ReportError(msg)

    git_value = doc.get("git")
    if not isinstance(git_value, dict):
        msg = "report needs a git object"
        raise ReportError(msg)
    git = cast("JsonObject", git_value)
    head = git.get("head")
    if not isinstance(head, str) or not _HEX40.match(head):
        msg = "git.head must be a 40-hex object id"
        raise ReportError(msg)
    clean = git.get("clean")
    if not isinstance(clean, bool):
        msg = "git.clean must be a boolean"
        raise ReportError(msg)
    status_value = git.get("status")
    if not isinstance(status_value, list):
        msg = "git.status must be a list of strings"
        raise ReportError(msg)
    status_items = cast("list[object]", status_value)
    if any(not isinstance(line, str) for line in status_items):
        msg = "git.status must be a list of strings"
        raise ReportError(msg)
    status = cast("list[str]", status_items)
    if clean != (len(status) == 0):
        msg = "git.clean disagrees with git.status"
        raise ReportError(msg)
    tracked_value = git.get("tracked_files")
    if not isinstance(tracked_value, list) or not tracked_value:
        msg = "git.tracked_files must be a non-empty list"
        raise ReportError(msg)
    tracked = cast("list[object]", tracked_value)
    seen_paths: set[str] = set()
    for entry in tracked:
        if not isinstance(entry, dict):
            msg = "tracked file entries must be objects"
            raise ReportError(msg)
        fields = cast("JsonObject", entry)
        path, oid, mode = fields.get("path"), fields.get("oid"), fields.get("mode")
        if not isinstance(path, str) or not path or path.startswith("/"):
            msg = f"invalid tracked path: {path!r}"
            raise ReportError(msg)
        if path in seen_paths:
            msg = f"duplicate tracked path: {path!r}"
            raise ReportError(msg)
        seen_paths.add(path)
        if not isinstance(oid, str) or not _HEX40.match(oid):
            msg = f"tracked file {path!r} needs a 40-hex oid"
            raise ReportError(msg)
        if not isinstance(mode, str) or not mode:
            msg = f"tracked file {path!r} needs a mode"
            raise ReportError(msg)

    snapshot = doc.get("snapshot")
    expected_snapshot = SNAPSHOT_CLEAN if clean else SNAPSHOT_DIRTY
    if snapshot != expected_snapshot:
        msg = f"snapshot {snapshot!r} disagrees with git cleanliness ({expected_snapshot!r})"
        raise ReportError(msg)

    tasks_value = doc.get("tasks")
    if not isinstance(tasks_value, list) or not tasks_value:
        msg = "report needs a non-empty task list"
        raise ReportError(msg)
    tasks = cast("list[object]", tasks_value)
    seen_names: set[str] = set()
    for task in tasks:
        if not isinstance(task, dict):
            msg = "task entries must be objects"
            raise ReportError(msg)
        fields = cast("JsonObject", task)
        name, command, argv_value = (
            fields.get("name"),
            fields.get("command"),
            fields.get("argv"),
        )
        digest = fields.get("identity_sha256")
        if not isinstance(name, str) or not name:
            msg = "tasks need non-empty names"
            raise ReportError(msg)
        if name in seen_names:
            msg = f"duplicate task name: {name!r}"
            raise ReportError(msg)
        seen_names.add(name)
        if not isinstance(command, str) or not command:
            msg = f"task {name!r} needs a command"
            raise ReportError(msg)
        if not isinstance(argv_value, list) or not argv_value:
            msg = f"task {name!r} needs a non-empty argv list"
            raise ReportError(msg)
        argv_items = cast("list[object]", argv_value)
        if any(not isinstance(a, str) for a in argv_items):
            msg = f"task {name!r} needs a non-empty argv list"
            raise ReportError(msg)
        argv = cast("list[str]", argv_items)
        if not isinstance(digest, str) or not _HEX64.match(digest):
            msg = f"task {name!r} needs a sha256 command identity"
            raise ReportError(msg)
        if digest != identity_of_argv([str(a) for a in argv]):
            msg = f"task {name!r}: command identity does not match argv"
            raise ReportError(msg)
        nar_value = fields.get("nar")
        if not isinstance(nar_value, dict):
            msg = f"task {name!r} needs a nar record"
            raise ReportError(msg)
        nar = cast("JsonObject", nar_value)
        nar_status = nar.get("status")
        if nar_status == "available":
            nar_hash = nar.get("hash")
            if not isinstance(nar_hash, str) or not nar_hash:
                msg = f"task {name!r}: available nar needs a hash"
                raise ReportError(msg)
        elif nar_status == "unavailable":
            reason = nar.get("reason")
            if not isinstance(reason, str) or not reason:
                msg = f"task {name!r}: unavailable nar needs a reason"
                raise ReportError(msg)
            if "hash" in nar:
                msg = f"task {name!r}: unavailable nar must not carry a hash"
                raise ReportError(msg)
        elif nar_status == "not-applicable":
            reason = nar.get("reason")
            if not isinstance(reason, str) or not reason:
                msg = f"task {name!r}: not-applicable nar needs a reason"
                raise ReportError(msg)
            if "hash" in nar:
                msg = f"task {name!r}: not-applicable nar must not carry a hash"
                raise ReportError(msg)
        else:
            msg = f"task {name!r}: nar.status must be available|unavailable|not-applicable"
            raise ReportError(msg)

    parity_value = doc.get("parity")
    if not isinstance(parity_value, dict):
        msg = "report needs a parity object"
        raise ReportError(msg)
    parity = cast("JsonObject", parity_value)
    verdict = parity.get("verdict")
    reasons_value = parity.get("reasons")
    if verdict not in (VERDICT_PARITY, VERDICT_NON_PARITY):
        msg = f"invalid parity verdict: {verdict!r}"
        raise ReportError(msg)
    if not isinstance(reasons_value, list):
        msg = "parity.reasons must be a list of strings"
        raise ReportError(msg)
    reason_items = cast("list[object]", reasons_value)
    if any(not isinstance(r, str) for r in reason_items):
        msg = "parity.reasons must be a list of strings"
        raise ReportError(msg)
    reasons = cast("list[str]", reason_items)
    dirty = snapshot == SNAPSHOT_DIRTY
    any_unavailable = False
    for task in tasks:
        if not isinstance(task, dict):
            continue
        task_nar = cast("JsonObject", task).get("nar")
        if not isinstance(task_nar, dict):
            continue
        if cast("JsonObject", task_nar).get("status") == "unavailable":
            any_unavailable = True
            break
    if verdict == VERDICT_PARITY and (dirty or any_unavailable or reasons):
        msg = "parity verdict is forbidden for dirty or incomplete evidence"
        raise ReportError(msg)
    if verdict == VERDICT_NON_PARITY and not (dirty or any_unavailable or reasons):
        msg = "non-parity verdict needs at least one reason"
        raise ReportError(msg)

    _reject_secrets(doc)


def dumps_report(report: dict[str, object]) -> str:
    return json.dumps(report, indent=2, sort_keys=True) + "\n"


def build_argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Fail-closed devShell/CI parity evidence (AR-13.1-24)."
    )
    parser.add_argument(
        "--repository", default=".", help="repository root (default: .)"
    )
    parser.add_argument("--output", required=True, help="destination JSON report path")
    parser.add_argument(
        "--task",
        action="append",
        default=[],
        metavar="NAME::COMMAND",
        help="bounded task entry; repeatable (NAME::COMMAND or NAME=COMMAND)",
    )
    parser.add_argument(
        "--package",
        action="append",
        default=[],
        metavar="TASK=FLAKE_ATTR",
        help="explicit flake attr for NAR lookup of TASK; repeatable",
    )
    parser.add_argument("--flake-ref", default=".", help="flake reference (default: .)")
    parser.add_argument(
        "--discover",
        action="store_true",
        help="discover tasks from flake apps/checks instead of --task",
    )
    parser.add_argument(
        "--max-tasks",
        type=int,
        default=DEFAULT_MAX_TASKS,
        help=f"task bound (default: {DEFAULT_MAX_TASKS})",
    )
    return parser


def parse_package_overrides(values: Sequence[str]) -> dict[str, str]:
    overrides: dict[str, str] = {}
    for value in values:
        if "=" not in value:
            msg = f"package {value!r} needs TASK=FLAKE_ATTR form"
            raise ReportError(msg)
        task_name, attr = value.split("=", 1)
        task_name, attr = task_name.strip(), attr.strip()
        if not task_name or not attr:
            msg = f"package {value!r} has an empty task or attr"
            raise ReportError(msg)
        overrides[task_name] = attr
    return overrides


def main(argv: Sequence[str] | None = None) -> int:
    parser = build_argument_parser()
    args = parser.parse_args(argv)
    if args.max_tasks < 1 or args.max_tasks > DEFAULT_MAX_TASKS:
        print(
            f"--max-tasks must be within 1..{DEFAULT_MAX_TASKS}",
            file=sys.stderr,
        )
        return 2
    repository = Path(args.repository).resolve()
    if not (repository / ".git").exists():
        print(f"not a git repository: {repository}", file=sys.stderr)
        return 2
    try:
        overrides = parse_package_overrides(args.package)
        if args.discover:
            if args.task:
                msg = "--discover cannot be combined with --task"
                raise ReportError(msg)
            kinds: tuple[str, ...] = ("apps", "checks")
            specs = [
                f"{name}::{command}"
                for name, command in discover_tasks(
                    args.flake_ref, repository, args.max_tasks, kinds
                )
            ]
        else:
            specs = list(args.task)
        report = build_report(
            repository,
            specs,
            flake_ref=args.flake_ref,
            package_overrides=overrides,
        )
    except ReportError as exc:
        print(f"devshell-ci-parity: {exc}", file=sys.stderr)
        return 2
    output = Path(args.output)
    try:
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(dumps_report(report), encoding="utf-8")
    except OSError as exc:
        print(f"devshell-ci-parity: cannot write {output}: {exc}", file=sys.stderr)
        return 2
    verdict_value = report["parity"]
    if not isinstance(verdict_value, dict):
        print("devshell-ci-parity: invalid parity object", file=sys.stderr)
        return 2
    verdict = cast("JsonObject", verdict_value)
    outcome = verdict.get("verdict")
    print(f"snapshot={report['snapshot']} verdict={outcome}")
    return 0 if outcome == VERDICT_PARITY else 1


if __name__ == "__main__":
    raise SystemExit(main())
