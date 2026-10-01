"""Static logical-check execution inventory for the four CI workflows.

Parses ``.github/workflows/{normal-ci,package,release,compatibility-roll}.yml``
with the standard library only (no PyYAML), resolves every verification-grade
invocation to a LOGICAL CHECK category instead of a raw flake app name, and
asserts that no logical check runs twice on the same SHA plus target
environment. Intentional multi-site executions are documented as exceptions
with an exact reason string; anything else multi-site is a duplication
violation and fails the artifact closed.

Logical mapping (derived from flake.nix app text, not guessed):

* ``nix fmt`` (bare, and the ``formatting`` lane of ``ci-parallel``) is the
  format lane (flake ``check`` app runs treefmt ``--ci``; ``ci-parallel``
  formatting lane runs ``nix fmt -- --ci``).
* ``ci-fast`` (the ``static`` lane of ``ci-parallel``) is static analysis
  (flake ``ci-fast`` text runs basedpyright, shell-lint, source-identity,
  and prose lanes via ``scripts/quality/run_parallel_checks.py``).
* ``ci-regions`` is static changeset classification (``scripts/ci/regions.py``
  never builds, tests, or packages); it is a singleton planner per workflow.
* ``ci-rust-contracts`` / ``rust-ci-core`` realize the Rust core derivation
  (cargo test graph), the contract-sync derivation, and the compatibility corpus
  derivation (flake ``realizeRustCiCore``); the dedicated ``clippy`` app owns
  the workspace clippy gate; they descend to build plus generated-drift plus
  compatibility lanes.
* ``contract-check`` realizes only the contract-sync derivation plus the
  settings-schema, records, and API-types drift lanes (flake
  ``realizeContractSync``); it is pure generated-drift.
* ``.#test`` is the pytest suite; ``.#test-production-routing-mutations`` is
  the production-routing mutation audit runner; both are test executions.
* ``.#web-check`` (and the release Windows web bundle verification) is web;
  ``.#performance-check`` and ``.#production-perf-check`` are performance;
  ``.#compatibility`` and ``.#compatibility-roll`` are compatibility.
* ``.#ci-aggregate`` is the required-status merge gate, the same gate family
  as the release gate (``.#check``); both are release readiness gates, as are
  ``.#release-check``, the signing-input manifests, and
  ``scripts.release.gate``.
* ``.#ci-timing`` (collect/validate/p95) and ``.#nix-evidence`` are timing
  machinery, not product checks; they are inventoried as documented
  exceptions.
* ``.#product-smoke`` and ``.#remote-flake-smoke`` are live smoke executions.
* ``.#tauri-build`` / ``cargo tauri build``, ``.#package-smoke``,
  ``.#package-install-smoke`` / ``windows_install_smoke.ps1``,
  ``.#package-reproducibility-check``, and byte-compare reproducibility
  verdicts assemble or verify installer bundles; they are packaging.
* ``nix build .#pokecon``, ``flake check --no-build``, and workspace
  ``cargo check`` validate the build graph; they are build.

The three ``rust_contracts`` branches (combined / rust-only / contract-only)
are mutually exclusive by plan-output step conditions, so only one executes
per SHA; the step conditions are recorded on each site for auditability.
The Windows ``cargo check`` targets ``windows-latest`` while the Linux Rust
work targets ``ubuntu-latest``, so they are not same-environment duplicates.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Final, Never, TypedDict, cast

if TYPE_CHECKING:
    from collections.abc import Mapping, Sequence

type JsonObject = dict[str, object]

SCHEMA_ID: Final = "ci-check-inventory/1"
WORKFLOWS_DIR_RELATIVE: Final = Path(".github/workflows")
WORKFLOWS: Final = (
    "normal-ci.yml",
    "package.yml",
    "release.yml",
    "compatibility-roll.yml",
)

JOB_HEADER_RE: Final = re.compile(r"(?m)^  ([A-Za-z0-9_-]+):\s*$")
JOBS_SECTION_RE: Final = re.compile(r"(?m)^jobs:\s*$")
STEP_SPLIT_RE: Final = re.compile(r"(?m)^      - ")
RUNS_ON_RE: Final = re.compile(r"(?m)^    runs-on: (.+?)\s*$")
RUN_LINE_RE: Final = re.compile(r"^        run:(.*)$")
IF_LINE_RE: Final = re.compile(r"^        if:(.*)$")
BLOCK_SCALAR_RE: Final = re.compile(r"\A\s*[>|][+\-0-9]*\s*\Z")

NIX_RUNNER_RE: Final = (
    r"(?:nix|\"\$POKECON_NIX_WRAPPER\"|\"\$POKECON_NIX_REAL\""
    r"|\$POKECON_NIX_WRAPPER|\$POKECON_NIX_REAL)"
)
LOCAL_RUN_RE: Final = re.compile(
    NIX_RUNNER_RE + r"\s+run\s+(?:--refresh\s+)?\.#([\w-]+)"
)
REMOTE_RUN_RE: Final = re.compile(
    NIX_RUNNER_RE + r"\s+run\b[\s\S]*?github:[^'\"]*#([\w-]+)"
)
NIX_BUILD_RE: Final = re.compile(r"\bnix\s+build\s+\.#([\w-]+)")
NIX_FMT_RE: Final = re.compile(r"\bnix\s+fmt\b")
FLAKE_CHECK_RE: Final = re.compile(r"\bflake\s+check\b")
CARGO_CHECK_RE: Final = re.compile(r"\bcargo\s+check\b")
CARGO_TAURI_RE: Final = re.compile(r"\bcargo\s+tauri\s+build\b")
RELEASE_GATE_RE: Final = re.compile(r"python\s+-m\s+scripts\.release\.gate\b")
SIGNING_MANIFEST_RE: Final = re.compile(
    r"python\s+-m\s+scripts\.release\.signing_manifest\b"
)
INSTALL_SMOKE_RE: Final = re.compile(r"windows_install_smoke\.ps1")
BYTE_COMPARE_RE: Final = re.compile(r"(?:^|[\s;\"'`])cmp\s+(--|\")|fc\.exe\s+/b")
WEB_BUNDLE_RE: Final = re.compile(r"svelte-check")
CI_PARALLEL_RE: Final = re.compile(r"\.#ci-parallel(?![\w-])")

APP_LOGIC: Final = {
    "ci-regions": "static-analysis",
    "ci-fast": "static-analysis",
    "nix-fmt": "format",
    "contract-check": "generated-drift",
    "test": "test",
    "test-production-routing-mutations": "test",
    "web-check": "web",
    "web-bundle-verification": "web",
    "performance-check": "performance",
    "production-perf-check": "performance",
    "compatibility": "compatibility",
    "compatibility-roll": "compatibility",
    "ci-aggregate": "release",
    "check": "release",
    "release-check": "release",
    "signing-input-manifest": "release",
    "scripts.release.signing_manifest": "release",
    "scripts.release.gate": "release",
    "ci-timing": "timing-machinery",
    "nix-evidence": "timing-machinery",
    "product-smoke": "smoke",
    "remote-flake-smoke": "smoke",
    "tauri-build": "packaging",
    "cargo-tauri-build": "packaging",
    "clippy": "static-analysis",
    "package-smoke": "packaging",
    "package-install-smoke": "packaging",
    "windows-install-smoke": "packaging",
    "package-reproducibility-check": "packaging",
    "installer-byte-compare": "packaging",
    "pokecon": "build",
    "flake-check": "build",
    "cargo-check": "build",
}

COMPOSITE_LANES: Final = {
    "ci-parallel": (
        ("nix-fmt", "format"),
        ("ci-fast", "static-analysis"),
    ),
    "ci-rust-contracts": (
        ("ci-rust-contracts", "build"),
        ("ci-rust-contracts", "generated-drift"),
        ("ci-rust-contracts", "compatibility"),
    ),
    "rust-ci-core": (
        ("rust-ci-core", "build"),
        ("rust-ci-core", "generated-drift"),
        ("rust-ci-core", "compatibility"),
    ),
}


@dataclass(frozen=True)
class ExpectedException:
    exception_id: str
    workflow: str
    logical: str
    app: str
    env: str
    reason: str


EXPECTED_EXCEPTIONS: Final = (
    ExpectedException(
        exception_id="normal-ci:timing-machinery:ci-timing:ubuntu-latest",
        workflow="normal-ci.yml",
        logical="timing-machinery",
        app="ci-timing",
        env="ubuntu-latest",
        reason=(
            "ci-timing collect/validate/p95 are sequential phases of one "
            "timing report pipeline, not three executions of a product check"
        ),
    ),
    ExpectedException(
        exception_id="normal-ci:timing-machinery:nix-evidence:ubuntu-latest",
        workflow="normal-ci.yml",
        logical="timing-machinery",
        app="nix-evidence",
        env="ubuntu-latest",
        reason=(
            "per-job Nix evidence collection (rust, product, remote) "
            "feeding the timing report, not a repeated product check"
        ),
    ),
    ExpectedException(
        exception_id="package:packaging:tauri-build:ubuntu-latest",
        workflow="package.yml",
        logical="packaging",
        app="tauri-build",
        env="ubuntu-latest",
        reason=(
            "primary/reproduction Debian build pair proving "
            "byte-for-byte reproducibility"
        ),
    ),
    ExpectedException(
        exception_id="package:packaging:cargo-tauri-build:windows-latest",
        workflow="package.yml",
        logical="packaging",
        app="cargo-tauri-build",
        env="windows-latest",
        reason=(
            "primary/reproduction NSIS build pair proving byte-for-byte reproducibility"
        ),
    ),
    ExpectedException(
        exception_id="package:release:signing-input-manifest:ubuntu-latest",
        workflow="package.yml",
        logical="release",
        app="signing-input-manifest",
        env="ubuntu-latest",
        reason="record/verify signing-input pair at the Linux handoff",
    ),
    ExpectedException(
        exception_id=(
            "package:release:scripts.release.signing_manifest:windows-latest"
        ),
        workflow="package.yml",
        logical="release",
        app="scripts.release.signing_manifest",
        env="windows-latest",
        reason="record/verify signing-input pair at the Windows handoff",
    ),
    ExpectedException(
        exception_id="package:packaging:windows-install-smoke:windows-latest",
        workflow="package.yml",
        logical="packaging",
        app="windows-install-smoke",
        env="windows-latest",
        reason="primary/reproduction clean-install verification pair",
    ),
    ExpectedException(
        exception_id="release:release:release-check:ubuntu-latest",
        workflow="release.yml",
        logical="release",
        app="release-check",
        env="ubuntu-latest",
        reason=(
            "phased release gates: tag identity, Linux checksums, complete manifest"
        ),
    ),
    ExpectedException(
        exception_id="release:packaging:tauri-build:ubuntu-latest",
        workflow="release.yml",
        logical="packaging",
        app="tauri-build",
        env="ubuntu-latest",
        reason=("primary/rebuild Debian pair proving byte-for-byte reproducibility"),
    ),
    ExpectedException(
        exception_id="release:packaging:cargo-tauri-build:windows-latest",
        workflow="release.yml",
        logical="packaging",
        app="cargo-tauri-build",
        env="windows-latest",
        reason=("primary/rebuild NSIS pair proving byte-for-byte reproducibility"),
    ),
    ExpectedException(
        exception_id="release:release:signing-input-manifest:ubuntu-latest",
        workflow="release.yml",
        logical="release",
        app="signing-input-manifest",
        env="ubuntu-latest",
        reason="record/verify signing-input pair at the Linux handoff",
    ),
    ExpectedException(
        exception_id=(
            "release:release:scripts.release.signing_manifest:windows-latest"
        ),
        workflow="release.yml",
        logical="release",
        app="scripts.release.signing_manifest",
        env="windows-latest",
        reason="record/verify signing-input pair at the Windows handoff",
    ),
    ExpectedException(
        exception_id="release:release:scripts.release.gate:windows-latest",
        workflow="release.yml",
        logical="release",
        app="scripts.release.gate",
        env="windows-latest",
        reason=(
            "phased Windows release gates: tag identity, then artifacts and checksums"
        ),
    ),
    ExpectedException(
        exception_id="release:packaging:windows-install-smoke:windows-latest",
        workflow="release.yml",
        logical="packaging",
        app="windows-install-smoke",
        env="windows-latest",
        reason=("primary/reproduction/final clean-install verification triple"),
    ),
)


class SiteReport(TypedDict):
    workflow: str
    job: str
    step: str
    command: str
    env: str
    app: str
    logical: str
    remote: bool
    condition: str | None


class ExceptionReport(TypedDict):
    id: str
    workflow: str
    logical: str
    app: str
    env: str
    reason: str
    sites: list[SiteReport]


class ViolationReport(TypedDict):
    logical: str
    app: str
    env: str
    sites: list[SiteReport]


def invalid_value(message: str) -> Never:
    raise ValueError(message)


def split_job_blocks(text: str, workflow: str) -> dict[str, str]:
    section = JOBS_SECTION_RE.search(text)
    if section is None:
        invalid_value(f"{workflow} has no top-level jobs section")
    body = text[section.end() :]
    headers = list(JOB_HEADER_RE.finditer(body))
    if not headers:
        invalid_value(f"{workflow} jobs section names no jobs")
    blocks: dict[str, str] = {}
    for index, header in enumerate(headers):
        job_id = header.group(1)
        if job_id in blocks:
            invalid_value(f"{workflow} has a duplicate job id: {job_id!r}")
        end = headers[index + 1].start() if index + 1 < len(headers) else len(body)
        blocks[job_id] = body[header.end() : end]
    return blocks


def job_env(block: str, workflow: str, job_id: str) -> str:
    match = RUNS_ON_RE.search(block)
    if match is None:
        invalid_value(f"{workflow} job {job_id!r} has no runner declaration")
    return match.group(1).strip().strip("\"'")


def extract_run_body(chunk: str) -> str | None:
    lines = chunk.splitlines()
    start: int | None = None
    inline: str | None = None
    for index, line in enumerate(lines):
        match = RUN_LINE_RE.match(line)
        if match is None:
            continue
        if start is not None:
            invalid_value("workflow step has more than one run key")
        start = index
        remainder = match.group(1).strip()
        if remainder == "" or BLOCK_SCALAR_RE.match(match.group(1)) is not None:
            inline = None
        else:
            inline = remainder
    if start is None:
        return None
    if inline is not None:
        return inline
    body: list[str] = []
    for line in lines[start + 1 :]:
        if line.strip() == "" or line.startswith("          "):
            body.append(line)
        else:
            break
    text = "\n".join(body).strip("\n")
    if not text.strip():
        invalid_value("workflow step has an empty run block")
    return text


def extract_step_condition(chunk: str) -> str | None:
    lines = chunk.splitlines()
    for index, line in enumerate(lines):
        match = IF_LINE_RE.match(line)
        if match is None:
            continue
        remainder = match.group(1).strip()
        if remainder == "" or BLOCK_SCALAR_RE.match(match.group(1)) is not None:
            continued: list[str] = []
            for extra in lines[index + 1 :]:
                if extra.strip() == "" or extra.startswith("          "):
                    if extra.strip() != "":
                        continued.append(extra.strip())
                else:
                    break
            if not continued:
                invalid_value("workflow step uses a folded if without a body")
            return re.sub(r"\s+", " ", " ".join(continued))[:200]
        return re.sub(r"\s+", " ", remainder)[:200]
    return None


def snippet(text: str, limit: int = 160) -> str:
    return re.sub(r"\s+", " ", text).strip()[:limit]


def logical_for(app: str, workflow: str, context: str) -> str:
    logical = APP_LOGIC.get(app)
    if logical is None:
        message = f"{workflow} invokes an unknown check app: {app!r} ({context})"
        invalid_value(message)
    return logical


def emit_site(
    workflow: str,
    job_id: str,
    step_name: str,
    command: str,
    env: str,
    app: str,
    logical: str,
    remote: bool,
    condition: str | None,
) -> SiteReport:
    return {
        "workflow": workflow,
        "job": job_id,
        "step": step_name,
        "command": command,
        "env": env,
        "app": app,
        "logical": logical,
        "remote": remote,
        "condition": condition,
    }


def scan_step_body(
    body: str,
    workflow: str,
    job_id: str,
    step_name: str,
    env: str,
    condition: str | None,
) -> list[SiteReport]:
    sites: list[SiteReport] = []
    parallel = CI_PARALLEL_RE.search(body)
    if parallel is not None:
        lanes = COMPOSITE_LANES["ci-parallel"]
        if "formatting" not in body or "nix fmt" not in body:
            invalid_value(f"{workflow} ci-parallel step lost its formatting lane")
        if "--next" not in body or "static" not in body or "ci-fast" not in body:
            invalid_value(f"{workflow} ci-parallel step lost its static lane")
        for lane_app, lane_logical in lanes:
            sites.append(
                emit_site(
                    workflow,
                    job_id,
                    step_name,
                    snippet("nix run .#ci-parallel -- " + lane_app),
                    env,
                    lane_app,
                    lane_logical,
                    False,
                    condition,
                )
            )
        return sites
    for match in REMOTE_RUN_RE.finditer(body):
        app = match.group(1)
        logical_for(app, workflow, step_name)
        sites.append(
            emit_site(
                workflow,
                job_id,
                step_name,
                snippet(match.group(0)),
                env,
                app,
                APP_LOGIC[app],
                True,
                condition,
            )
        )
    for match in LOCAL_RUN_RE.finditer(body):
        app = match.group(1)
        if app in COMPOSITE_LANES:
            for lane_app, lane_logical in COMPOSITE_LANES[app]:
                sites.append(
                    emit_site(
                        workflow,
                        job_id,
                        step_name,
                        snippet(match.group(0)),
                        env,
                        lane_app,
                        lane_logical,
                        False,
                        condition,
                    )
                )
        else:
            logical = logical_for(app, workflow, step_name)
            sites.append(
                emit_site(
                    workflow,
                    job_id,
                    step_name,
                    snippet(match.group(0)),
                    env,
                    app,
                    logical,
                    False,
                    condition,
                )
            )
    for match in NIX_BUILD_RE.finditer(body):
        app = match.group(1)
        logical = logical_for(app, workflow, step_name)
        sites.append(
            emit_site(
                workflow,
                job_id,
                step_name,
                snippet(match.group(0)),
                env,
                app,
                logical,
                False,
                condition,
            )
        )
    if NIX_FMT_RE.search(body) is not None:
        sites.append(
            emit_site(
                workflow,
                job_id,
                step_name,
                "nix fmt",
                env,
                "nix-fmt",
                APP_LOGIC["nix-fmt"],
                False,
                condition,
            )
        )
    if FLAKE_CHECK_RE.search(body) is not None:
        sites.append(
            emit_site(
                workflow,
                job_id,
                step_name,
                "flake check --no-build",
                env,
                "flake-check",
                APP_LOGIC["flake-check"],
                False,
                condition,
            )
        )
    cargo_match = CARGO_CHECK_RE.search(body)
    if cargo_match is not None:
        sites.append(
            emit_site(
                workflow,
                job_id,
                step_name,
                snippet(cargo_match.group(0)),
                env,
                "cargo-check",
                APP_LOGIC["cargo-check"],
                False,
                condition,
            )
        )
    if CARGO_TAURI_RE.search(body) is not None:
        sites.append(
            emit_site(
                workflow,
                job_id,
                step_name,
                "cargo tauri build",
                env,
                "cargo-tauri-build",
                APP_LOGIC["cargo-tauri-build"],
                False,
                condition,
            )
        )
    if RELEASE_GATE_RE.search(body) is not None:
        sites.append(
            emit_site(
                workflow,
                job_id,
                step_name,
                "python -m scripts.release.gate",
                env,
                "scripts.release.gate",
                APP_LOGIC["scripts.release.gate"],
                False,
                condition,
            )
        )
    if SIGNING_MANIFEST_RE.search(body) is not None:
        sites.append(
            emit_site(
                workflow,
                job_id,
                step_name,
                "python -m scripts.release.signing_manifest",
                env,
                "scripts.release.signing_manifest",
                APP_LOGIC["scripts.release.signing_manifest"],
                False,
                condition,
            )
        )
    if INSTALL_SMOKE_RE.search(body) is not None:
        sites.append(
            emit_site(
                workflow,
                job_id,
                step_name,
                "windows_install_smoke.ps1",
                env,
                "windows-install-smoke",
                APP_LOGIC["windows-install-smoke"],
                False,
                condition,
            )
        )
    if BYTE_COMPARE_RE.search(body) is not None:
        sites.append(
            emit_site(
                workflow,
                job_id,
                step_name,
                "byte-compare primary/reproduction bundles",
                env,
                "installer-byte-compare",
                APP_LOGIC["installer-byte-compare"],
                False,
                condition,
            )
        )
    if WEB_BUNDLE_RE.search(body) is not None:
        sites.append(
            emit_site(
                workflow,
                job_id,
                step_name,
                "bun web bundle verification (lint/svelte-check/test)",
                env,
                "web-bundle-verification",
                APP_LOGIC["web-bundle-verification"],
                False,
                condition,
            )
        )
    return sites


def collect_sites(text: str, workflow: str) -> list[SiteReport]:
    sites: list[SiteReport] = []
    blocks = split_job_blocks(text, workflow)
    for job_id, block in blocks.items():
        env = job_env(block, workflow, job_id)
        chunks = STEP_SPLIT_RE.split(block)
        for chunk in chunks[1:]:
            first = chunk.splitlines()[0] if chunk.splitlines() else ""
            if first.startswith("name:"):
                step_name = first[len("name:") :].strip()
            else:
                step_name = ""
            body = extract_run_body(chunk)
            if body is None:
                continue
            if step_name == "":
                invalid_value(
                    f"{workflow} job {job_id!r} has a run step without a name"
                )
            condition = extract_step_condition(chunk)
            sites.extend(
                scan_step_body(body, workflow, job_id, step_name, env, condition)
            )
    if not sites:
        invalid_value(f"{workflow} declares no logical-check executions")
    return sites


def sort_sites(sites: list[SiteReport]) -> list[SiteReport]:
    return sorted(
        sites,
        key=lambda site: (
            site["workflow"],
            site["job"],
            site["step"],
            site["app"],
            site["logical"],
            site["command"],
        ),
    )


def build_inventory(texts: Mapping[str, str]) -> JsonObject:
    if set(texts) != set(WORKFLOWS):
        invalid_value(
            f"check inventory requires exactly the four workflows, got: {sorted(texts)}"
        )
    all_sites: list[SiteReport] = []
    for workflow in WORKFLOWS:
        all_sites.extend(collect_sites(texts[workflow], workflow))
    checks: dict[str, list[SiteReport]] = {}
    for site in all_sites:
        checks.setdefault(site["logical"], []).append(site)
    for logical, logical_sites in checks.items():
        checks[logical] = sort_sites(logical_sites)
    groups: dict[tuple[str, str, str, str], list[SiteReport]] = {}
    for site in all_sites:
        key = (site["workflow"], site["logical"], site["app"], site["env"])
        groups.setdefault(key, []).append(site)
    expected_keys = {
        (item.workflow, item.logical, item.app, item.env): item
        for item in EXPECTED_EXCEPTIONS
    }
    exception_ids = [item.exception_id for item in EXPECTED_EXCEPTIONS]
    if len(set(exception_ids)) != len(exception_ids):
        invalid_value("check inventory has duplicate exception ids")
    exceptions: list[ExceptionReport] = []
    for key in sorted(groups):
        if len(groups[key]) < 2:
            continue
        item = expected_keys.get(key)
        if item is None:
            continue
        exceptions.append(
            {
                "id": item.exception_id,
                "workflow": item.workflow,
                "logical": item.logical,
                "app": item.app,
                "env": item.env,
                "reason": item.reason,
                "sites": sort_sites(groups[key]),
            }
        )
    for item in EXPECTED_EXCEPTIONS:
        key = (item.workflow, item.logical, item.app, item.env)
        if len(groups.get(key, [])) < 2:
            invalid_value(
                f"check inventory exception is stale (no multi-site group): {item.exception_id}"
            )
    duplication: dict[str, list[ViolationReport]] = {name: [] for name in WORKFLOWS}
    for key in sorted(groups):
        if len(groups[key]) < 2:
            continue
        if key in expected_keys:
            continue
        workflow, logical, app, env = key
        violation: ViolationReport = {
            "logical": logical,
            "app": app,
            "env": env,
            "sites": sort_sites(groups[key]),
        }
        duplication[workflow].append(violation)
    report: JsonObject = {}
    report["schema"] = SCHEMA_ID
    report["workflows"] = list(WORKFLOWS)
    report["app_logic"] = dict(APP_LOGIC)
    report["composite_lanes"] = {
        app: [
            {"app": lane_app, "logical": lane_logical}
            for lane_app, lane_logical in lanes
        ]
        for app, lanes in COMPOSITE_LANES.items()
    }
    report["checks"] = checks
    report["exceptions"] = exceptions
    report["duplication"] = duplication
    return report


def read_texts(project_root: Path) -> dict[str, str]:
    texts: dict[str, str] = {}
    for workflow in WORKFLOWS:
        path = project_root / WORKFLOWS_DIR_RELATIVE / workflow
        try:
            texts[workflow] = path.read_text(encoding="utf-8")
        except OSError as error:
            message = f"cannot read workflow {path}: {error}"
            raise ValueError(message) from error
    return texts


def main(argv: Sequence[str] | None = None) -> int:
    default_root = Path.cwd()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=default_root)
    arguments = parser.parse_args(argv)
    try:
        project_root = cast("Path", arguments.root)
        report = build_inventory(read_texts(project_root))
        violations = cast("dict[str, list[object]]", report["duplication"])
        violated = sorted(name for name, items in violations.items() if items)
    except (OSError, ValueError) as error:
        parser.exit(1, f"ci check inventory failed: {error}\n")
    print(json.dumps(report, indent=2, sort_keys=True))
    if violated:
        print(
            f"ci check inventory failed: duplication in {violated}",
            file=sys.stderr,
            flush=True,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
