"""Logical-check execution inventory tests (AR-10.10-08 / AR-11-43 / AR-13.1-12).

Pins the exact logical-check site sets for each of the four workflows,
asserts the duplication claim (zero same-check plus same-environment
repeats) with every intentional multi-site group covered by an exception
carrying an exact reason string, cross-checks the normal-ci command set
with an independently re-derived raw scan, and proves both fail-closed
mutations break red:

* Mutation A (synthetic duplicate): appending a second ``nix run .#test``
  step to an in-memory copy of ``normal-ci.yml`` must surface exactly one
  duplication violation for the ``test`` app.
* Mutation B (removed site): deleting the ``nix run .#test`` line from an
  in-memory copy must fail the exact-set assertion for ``normal-ci.yml``.
* Mutation C (CLI duplicate): the command line must exit nonzero on a
  duplicated site while keeping its stdout JSON valid.
"""

from __future__ import annotations

import json
import re
from collections import Counter
from pathlib import Path
from typing import Final, cast

import pytest

from scripts.acceptance import ci_check_inventory

REPOSITORY = Path(__file__).resolve().parents[2]
WORKFLOWS_DIR = REPOSITORY / ".github/workflows"

type JsonObject = dict[str, object]
type SiteTuple = tuple[str, str, str, str, str]

EXPECTED_NORMAL_CI: tuple[SiteTuple, ...] = (
    (
        "fast",
        "Run formatting and fast static checks in parallel",
        "ci-fast",
        "static-analysis",
        "ubuntu-latest",
    ),
    (
        "fast",
        "Run formatting and fast static checks in parallel",
        "nix-fmt",
        "format",
        "ubuntu-latest",
    ),
    (
        "performance",
        "Run the browser loopback performance gate",
        "performance-check",
        "performance",
        "ubuntu-latest",
    ),
    (
        "production_perf",
        "Run the production main-path virtual performance gate",
        "production-perf-check",
        "performance",
        "ubuntu-latest",
    ),
    (
        "plan",
        "Classify changed regions",
        "ci-regions",
        "static-analysis",
        "ubuntu-latest",
    ),
    (
        "product_flake",
        "Collect product Nix evidence",
        "nix-evidence",
        "timing-machinery",
        "ubuntu-latest",
    ),
    (
        "product_flake",
        "Evaluate all local flake outputs",
        "flake-check",
        "build",
        "ubuntu-latest",
    ),
    (
        "product_flake",
        "Run product smoke checks",
        "product-smoke",
        "smoke",
        "ubuntu-latest",
    ),
    ("python_tests", "Run Python tests", "test", "test", "ubuntu-latest"),
    (
        "remote_flake",
        "Collect remote Nix evidence",
        "nix-evidence",
        "timing-machinery",
        "ubuntu-latest",
    ),
    (
        "remote_flake",
        "Run default app help from remote, then build and web smoke without checkout",
        "remote-flake-smoke",
        "smoke",
        "ubuntu-latest",
    ),
    (
        "required",
        "Aggregate required job results",
        "ci-aggregate",
        "release",
        "ubuntu-latest",
    ),
    (
        "required",
        "Collect CI timing evidence",
        "ci-timing",
        "timing-machinery",
        "ubuntu-latest",
    ),
    (
        "required",
        "Collect CI timing evidence",
        "ci-timing",
        "timing-machinery",
        "ubuntu-latest",
    ),
    (
        "required",
        "Enforce timing p95 history gate (blocking, fail-closed)",
        "ci-timing",
        "timing-machinery",
        "ubuntu-latest",
    ),
    (
        "routing_mutations",
        "Run production-routing mutation audit",
        "test-production-routing-mutations",
        "test",
        "ubuntu-latest",
    ),
    (
        "rust_contracts",
        "Collect Rust Nix evidence",
        "nix-evidence",
        "timing-machinery",
        "ubuntu-latest",
    ),
    ("rust_contracts", "Run Rust checks", "rust-ci-core", "build", "ubuntu-latest"),
    (
        "rust_contracts",
        "Run Rust checks",
        "rust-ci-core",
        "compatibility",
        "ubuntu-latest",
    ),
    (
        "rust_contracts",
        "Run Rust checks",
        "rust-ci-core",
        "generated-drift",
        "ubuntu-latest",
    ),
    (
        "rust_contracts",
        "Run combined Rust and contract checks",
        "ci-rust-contracts",
        "build",
        "ubuntu-latest",
    ),
    (
        "rust_contracts",
        "Run combined Rust and contract checks",
        "ci-rust-contracts",
        "compatibility",
        "ubuntu-latest",
    ),
    (
        "rust_contracts",
        "Run combined Rust and contract checks",
        "ci-rust-contracts",
        "generated-drift",
        "ubuntu-latest",
    ),
    (
        "rust_contracts",
        "Run contract checks",
        "contract-check",
        "generated-drift",
        "ubuntu-latest",
    ),
    (
        "rust_clippy",
        "Run Rust clippy",
        "clippy",
        "static-analysis",
        "ubuntu-latest",
    ),
    ("web", "Run Web checks", "web-check", "web", "ubuntu-latest"),
    (
        "windows",
        "Check all Windows targets and features",
        "cargo-check",
        "build",
        "windows-latest",
    ),
)

EXPECTED_PACKAGE: tuple[SiteTuple, ...] = (
    (
        "linux",
        "Audit packaged runtime and offline wheelhouse",
        "package-smoke",
        "packaging",
        "ubuntu-latest",
    ),
    ("linux", "Build Debian package", "tauri-build", "packaging", "ubuntu-latest"),
    (
        "linux",
        "Record Linux signing inputs",
        "signing-input-manifest",
        "release",
        "ubuntu-latest",
    ),
    (
        "linux",
        "Verify Linux signing inputs at handoff",
        "signing-input-manifest",
        "release",
        "ubuntu-latest",
    ),
    (
        "linux",
        "Verify clean install, offline startup, upgrade, and uninstall",
        "package-install-smoke",
        "packaging",
        "ubuntu-latest",
    ),
    (
        "linux_repro",
        "Build Debian package from identical inputs",
        "tauri-build",
        "packaging",
        "ubuntu-latest",
    ),
    (
        "plan",
        "Classify changed regions",
        "ci-regions",
        "static-analysis",
        "ubuntu-latest",
    ),
    (
        "repro_check",
        "Verify byte-for-byte package reproducibility",
        "package-reproducibility-check",
        "packaging",
        "ubuntu-latest",
    ),
    (
        "required",
        "Aggregate required package results",
        "ci-aggregate",
        "release",
        "ubuntu-latest",
    ),
    (
        "windows",
        "Build offline NSIS installer",
        "cargo-tauri-build",
        "packaging",
        "windows-latest",
    ),
    (
        "windows",
        "Record Windows signing inputs",
        "scripts.release.signing_manifest",
        "release",
        "windows-latest",
    ),
    (
        "windows",
        "Verify Windows signing inputs at handoff",
        "scripts.release.signing_manifest",
        "release",
        "windows-latest",
    ),
    (
        "windows",
        "Verify clean install, startup, upgrade, and uninstall",
        "windows-install-smoke",
        "packaging",
        "windows-latest",
    ),
    (
        "windows_repro",
        "Build offline NSIS installer",
        "cargo-tauri-build",
        "packaging",
        "windows-latest",
    ),
    (
        "windows_repro",
        "Verify clean install tree for reproduction build",
        "windows-install-smoke",
        "packaging",
        "windows-latest",
    ),
    (
        "windows_repro_check",
        "Verify byte-for-byte NSIS reproducibility",
        "installer-byte-compare",
        "packaging",
        "ubuntu-latest",
    ),
)

EXPECTED_RELEASE: tuple[SiteTuple, ...] = (
    (
        "github-release",
        "Generate complete checksum manifest",
        "release-check",
        "release",
        "ubuntu-latest",
    ),
    (
        "linux",
        "Audit packaged runtime and offline wheelhouse",
        "package-smoke",
        "packaging",
        "ubuntu-latest",
    ),
    ("linux", "Build Linux Nix package", "pokecon", "build", "ubuntu-latest"),
    ("linux", "Build Linux Tauri package", "tauri-build", "packaging", "ubuntu-latest"),
    ("linux", "Generate Linux checksums", "release-check", "release", "ubuntu-latest"),
    (
        "linux",
        "Rebuild Linux package from identical inputs",
        "tauri-build",
        "packaging",
        "ubuntu-latest",
    ),
    (
        "linux",
        "Record Linux signing inputs",
        "signing-input-manifest",
        "release",
        "ubuntu-latest",
    ),
    ("linux", "Run complete release gate", "check", "release", "ubuntu-latest"),
    ("linux", "Validate release identity", "release-check", "release", "ubuntu-latest"),
    (
        "linux",
        "Verify Linux signing inputs at handoff",
        "signing-input-manifest",
        "release",
        "ubuntu-latest",
    ),
    (
        "linux",
        "Verify byte-for-byte Linux package reproducibility",
        "installer-byte-compare",
        "packaging",
        "ubuntu-latest",
    ),
    (
        "linux",
        "Verify clean install, offline startup, upgrade, and uninstall",
        "package-install-smoke",
        "packaging",
        "ubuntu-latest",
    ),
    (
        "windows",
        "Build Web distribution",
        "web-bundle-verification",
        "web",
        "windows-latest",
    ),
    (
        "windows",
        "Build offline NSIS installer",
        "cargo-tauri-build",
        "packaging",
        "windows-latest",
    ),
    (
        "windows",
        "Collect installer and checksums",
        "scripts.release.gate",
        "release",
        "windows-latest",
    ),
    (
        "windows",
        "Collect installer and checksums",
        "scripts.release.signing_manifest",
        "release",
        "windows-latest",
    ),
    (
        "windows",
        "Rebuild Windows package from identical inputs",
        "cargo-tauri-build",
        "packaging",
        "windows-latest",
    ),
    (
        "windows",
        "Record Windows signing inputs",
        "scripts.release.signing_manifest",
        "release",
        "windows-latest",
    ),
    (
        "windows",
        "Validate release identity",
        "scripts.release.gate",
        "release",
        "windows-latest",
    ),
    (
        "windows",
        "Verify byte-for-byte Windows NSIS reproducibility",
        "installer-byte-compare",
        "packaging",
        "windows-latest",
    ),
    (
        "windows",
        "Verify clean install, startup, upgrade, and uninstall",
        "windows-install-smoke",
        "packaging",
        "windows-latest",
    ),
    (
        "windows",
        "Verify first Windows clean install tree",
        "windows-install-smoke",
        "packaging",
        "windows-latest",
    ),
    (
        "windows",
        "Verify second Windows clean install tree",
        "windows-install-smoke",
        "packaging",
        "windows-latest",
    ),
)

EXPECTED_COMPATIBILITY_ROLL: tuple[SiteTuple, ...] = (
    (
        "evaluate",
        "Discover, execute, and classify immutable candidates",
        "compatibility-roll",
        "compatibility",
        "ubuntu-latest",
    ),
    (
        "evaluate",
        "Format generated ledgers and reports",
        "nix-fmt",
        "format",
        "ubuntu-latest",
    ),
    (
        "evaluate",
        "Verify the complete guarantee chain",
        "compatibility",
        "compatibility",
        "ubuntu-latest",
    ),
)

EXPECTED_BY_WORKFLOW: dict[str, tuple[SiteTuple, ...]] = {
    "normal-ci.yml": EXPECTED_NORMAL_CI,
    "package.yml": EXPECTED_PACKAGE,
    "release.yml": EXPECTED_RELEASE,
    "compatibility-roll.yml": EXPECTED_COMPATIBILITY_ROLL,
}

EXPECTED_EXCEPTION_REASONS: dict[str, str] = {
    "normal-ci:timing-machinery:ci-timing:ubuntu-latest": (
        "ci-timing collect/validate/p95 are sequential phases of one "
        "timing report pipeline, not three executions of a product check"
    ),
    "normal-ci:timing-machinery:nix-evidence:ubuntu-latest": (
        "per-job Nix evidence collection (rust, product, remote) "
        "feeding the timing report, not a repeated product check"
    ),
    "package:packaging:tauri-build:ubuntu-latest": (
        "primary/reproduction Debian build pair proving byte-for-byte reproducibility"
    ),
    "package:packaging:cargo-tauri-build:windows-latest": (
        "primary/reproduction NSIS build pair proving byte-for-byte reproducibility"
    ),
    "package:release:signing-input-manifest:ubuntu-latest": (
        "record/verify signing-input pair at the Linux handoff"
    ),
    "package:release:scripts.release.signing_manifest:windows-latest": (
        "record/verify signing-input pair at the Windows handoff"
    ),
    "package:packaging:windows-install-smoke:windows-latest": (
        "primary/reproduction clean-install verification pair"
    ),
    "release:release:release-check:ubuntu-latest": (
        "phased release gates: tag identity, Linux checksums, complete manifest"
    ),
    "release:packaging:tauri-build:ubuntu-latest": (
        "primary/rebuild Debian pair proving byte-for-byte reproducibility"
    ),
    "release:packaging:cargo-tauri-build:windows-latest": (
        "primary/rebuild NSIS pair proving byte-for-byte reproducibility"
    ),
    "release:release:signing-input-manifest:ubuntu-latest": (
        "record/verify signing-input pair at the Linux handoff"
    ),
    "release:release:scripts.release.signing_manifest:windows-latest": (
        "record/verify signing-input pair at the Windows handoff"
    ),
    "release:release:scripts.release.gate:windows-latest": (
        "phased Windows release gates: tag identity, then artifacts and checksums"
    ),
    "release:packaging:windows-install-smoke:windows-latest": (
        "primary/reproduction/final clean-install verification triple"
    ),
}

EXPECTED_EXCEPTION_SITE_COUNTS: dict[str, int] = {
    "normal-ci:timing-machinery:ci-timing:ubuntu-latest": 3,
    "normal-ci:timing-machinery:nix-evidence:ubuntu-latest": 3,
    "package:packaging:tauri-build:ubuntu-latest": 2,
    "package:packaging:cargo-tauri-build:windows-latest": 2,
    "package:release:signing-input-manifest:ubuntu-latest": 2,
    "package:release:scripts.release.signing_manifest:windows-latest": 2,
    "package:packaging:windows-install-smoke:windows-latest": 2,
    "release:release:release-check:ubuntu-latest": 3,
    "release:packaging:tauri-build:ubuntu-latest": 2,
    "release:packaging:cargo-tauri-build:windows-latest": 2,
    "release:release:signing-input-manifest:ubuntu-latest": 2,
    "release:release:scripts.release.signing_manifest:windows-latest": 2,
    "release:release:scripts.release.gate:windows-latest": 2,
    "release:packaging:windows-install-smoke:windows-latest": 3,
}

EXPECTED_REPORT_APP_COUNTS: dict[str, dict[str, int]] = {
    "normal-ci.yml": {
        "ci-regions": 1,
        "clippy": 1,
        "ci-rust-contracts": 3,
        "rust-ci-core": 3,
        "contract-check": 1,
        "nix-evidence": 3,
        "test": 1,
        "test-production-routing-mutations": 1,
        "web-check": 1,
        "product-smoke": 1,
        "flake-check": 1,
        "remote-flake-smoke": 1,
        "performance-check": 1,
        "production-perf-check": 1,
        "cargo-check": 1,
        "ci-aggregate": 1,
        "ci-timing": 3,
        "ci-fast": 1,
        "nix-fmt": 1,
    },
    "package.yml": {
        "ci-regions": 1,
        "ci-aggregate": 1,
        "package-install-smoke": 1,
        "package-reproducibility-check": 1,
        "package-smoke": 1,
        "signing-input-manifest": 2,
        "tauri-build": 2,
        "cargo-tauri-build": 2,
        "scripts.release.signing_manifest": 2,
        "windows-install-smoke": 2,
        "installer-byte-compare": 1,
    },
    "release.yml": {
        "check": 1,
        "package-install-smoke": 1,
        "package-smoke": 1,
        "release-check": 3,
        "signing-input-manifest": 2,
        "tauri-build": 2,
        "pokecon": 1,
        "installer-byte-compare": 2,
        "scripts.release.gate": 2,
        "web-bundle-verification": 1,
        "cargo-tauri-build": 2,
        "scripts.release.signing_manifest": 2,
        "windows-install-smoke": 3,
    },
    "compatibility-roll.yml": {
        "compatibility": 1,
        "compatibility-roll": 1,
        "nix-fmt": 1,
    },
}

RAW_APP_RE: Final = re.compile(r"\.#([\w-]+)|github:[^'\"]*#([\w-]+)")
NIX_FMT_LINE_RE: Final = re.compile(r"nix\s+fmt\b")
FLAKE_CHECK_LINE_RE: Final = re.compile(r"flake\s+check\b")
CARGO_CHECK_LINE_RE: Final = re.compile(r"cargo\s+check\b")


def _read_texts() -> dict[str, str]:
    return {
        name: (WORKFLOWS_DIR / name).read_text(encoding="utf-8")
        for name in ci_check_inventory.WORKFLOWS
    }


def _report() -> JsonObject:
    return ci_check_inventory.build_inventory(_read_texts())


def _checks(report: JsonObject) -> dict[str, list[JsonObject]]:
    return cast("dict[str, list[JsonObject]]", report["checks"])


def _site_tuples(report: JsonObject, workflow: str) -> list[SiteTuple]:
    return sorted(
        (
            cast("str", site["job"]),
            cast("str", site["step"]),
            cast("str", site["app"]),
            cast("str", site["logical"]),
            cast("str", site["env"]),
        )
        for sites in _checks(report).values()
        for site in sites
        if site["workflow"] == workflow
    )


def test_schema_identity_and_workflow_set() -> None:
    report = _report()
    assert report["schema"] == "ci-check-inventory/1"
    assert report["workflows"] == [
        "normal-ci.yml",
        "package.yml",
        "release.yml",
        "compatibility-roll.yml",
    ]


def test_logical_site_sets_are_exact() -> None:
    report = _report()
    for workflow, expected in EXPECTED_BY_WORKFLOW.items():
        assert _site_tuples(report, workflow) == sorted(expected), (
            f"logical site set changed for {workflow}"
        )


def test_report_app_counts_match_verified_reality() -> None:
    report = _report()
    for workflow, expected_counts in EXPECTED_REPORT_APP_COUNTS.items():
        observed = Counter(
            cast("str", site["app"])
            for sites in _checks(report).values()
            for site in sites
            if site["workflow"] == workflow
        )
        assert dict(observed) == expected_counts, (
            f"raw app counts changed for {workflow}: {dict(observed)}"
        )


def test_normal_ci_headline_counts() -> None:
    report = _report()
    observed = Counter(
        cast("str", site["app"])
        for sites in _checks(report).values()
        for site in sites
        if site["workflow"] == "normal-ci.yml"
    )
    assert observed["ci-timing"] == 3
    assert observed["nix-evidence"] == 3
    assert observed.get("ci-parallel", 0) == 0
    assert observed["nix-fmt"] == 1
    assert observed["ci-fast"] == 1
    for single in (
        "ci-regions",
        "test",
        "test-production-routing-mutations",
        "web-check",
        "performance-check",
        "production-perf-check",
        "ci-aggregate",
    ):
        assert observed[single] == 1, f"normal-ci {single} count changed"


def test_duplication_is_empty_for_every_workflow() -> None:
    report = _report()
    duplication = cast("dict[str, list[object]]", report["duplication"])
    assert sorted(duplication) == sorted(EXPECTED_BY_WORKFLOW)
    for workflow, violations in duplication.items():
        assert violations == [], f"duplication violation in {workflow}: {violations}"


def test_exceptions_match_exactly_with_reasons() -> None:
    report = _report()
    exceptions = cast("list[JsonObject]", report["exceptions"])
    observed_reasons = {
        cast("str", entry["id"]): cast("str", entry["reason"]) for entry in exceptions
    }
    assert observed_reasons == EXPECTED_EXCEPTION_REASONS
    observed_counts = {
        cast("str", entry["id"]): len(cast("list[object]", entry["sites"]))
        for entry in exceptions
    }
    assert observed_counts == EXPECTED_EXCEPTION_SITE_COUNTS


def test_rust_contract_branches_record_exclusive_conditions() -> None:
    report = _report()
    conditions: dict[str, str | None] = {}
    for sites in _checks(report).values():
        for site in sites:
            if site["workflow"] != "normal-ci.yml":
                continue
            if site["job"] != "rust_contracts":
                continue
            app = cast("str", site["app"])
            if app in ("ci-rust-contracts", "rust-ci-core", "contract-check"):
                conditions.setdefault(app, cast("str | None", site["condition"]))
    assert set(conditions) == {"ci-rust-contracts", "rust-ci-core", "contract-check"}
    assert all(condition is not None for condition in conditions.values())
    assert len(set(conditions.values())) == 3
    assert "contracts" in cast("str", conditions["ci-rust-contracts"])
    assert "rust" in cast("str", conditions["rust-ci-core"])


def test_windows_cargo_check_uses_its_own_target_env() -> None:
    report = _report()
    envs = {
        cast("str", site["env"])
        for sites in _checks(report).values()
        for site in sites
        if site["workflow"] == "normal-ci.yml"
        and cast("str", site["app"]) == "cargo-check"
    }
    assert envs == {"windows-latest"}


def test_normal_ci_commands_rederived_independently() -> None:
    """Re-scan raw normal-ci text with a separate minimal pattern set.

    The report must account for every raw invocation: composite apps expand
    (ci-parallel yields the nix-fmt plus ci-fast lane sites, the rust apps
    yield one site per lane), everything else maps one to one.
    """
    text = (WORKFLOWS_DIR / "normal-ci.yml").read_text(encoding="utf-8")
    joined = re.sub(r"\\\n", " ", text)
    lane_continuation = re.compile(r"\s*(formatting|--next)\b")
    raw: list[str] = []
    for line in joined.splitlines():
        if lane_continuation.match(line) is not None:
            continue
        if "nix run" in line or 'NIX_WRAPPER" run' in line or 'NIX_REAL" run' in line:
            raw.extend(
                match.group(1) or match.group(2) for match in RAW_APP_RE.finditer(line)
            )
        elif NIX_FMT_LINE_RE.search(line) is not None:
            raw.append("nix-fmt")
        elif FLAKE_CHECK_LINE_RE.search(line) is not None:
            raw.append("flake-check")
        elif CARGO_CHECK_LINE_RE.search(line) is not None:
            raw.append("cargo-check")
    report = _report()
    observed = Counter(
        cast("str", site["app"])
        for sites in _checks(report).values()
        for site in sites
        if site["workflow"] == "normal-ci.yml"
    )
    expansion = {"ci-parallel": 2, "ci-rust-contracts": 3, "rust-ci-core": 3}
    expected: Counter[str] = Counter()
    for app in raw:
        if app == "ci-parallel":
            expected["nix-fmt"] += 1
            expected["ci-fast"] += 1
        elif app in expansion:
            expected[app] += expansion[app]
        else:
            expected[app] += 1
    assert raw.count("ci-parallel") == 1
    assert dict(observed) == dict(expected), (
        f"report dropped or added normal-ci commands: {dict(observed)} vs {dict(expected)}"
    )


def test_main_reports_valid_json_with_empty_duplication(
    capsys: pytest.CaptureFixture[str],
) -> None:
    code = ci_check_inventory.main(["--root", str(REPOSITORY)])
    assert code == 0
    report = json.loads(capsys.readouterr().out)
    assert report["schema"] == "ci-check-inventory/1"
    for violations in cast("dict[str, list[object]]", report["duplication"]).values():
        assert violations == []


def test_main_returns_nonzero_on_duplication(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """Mutation C: the CLI must fail closed with exit 1 on duplication.

    A full workflow copy with one synthetic duplicate keeps stdout's JSON
    valid while the process exits nonzero and reports the violation on
    stderr.
    """
    workflows_dir = tmp_path / ".github" / "workflows"
    workflows_dir.mkdir(parents=True)
    texts = _read_texts()
    old = "      - name: Run Python tests\n        run: nix run .#test\n"
    assert old in texts["normal-ci.yml"]
    texts["normal-ci.yml"] = texts["normal-ci.yml"].replace(
        old,
        old + "      - name: Run Python tests again\n        run: nix run .#test\n",
        1,
    )
    for name, text in texts.items():
        (workflows_dir / name).write_text(text, encoding="utf-8")
    code = ci_check_inventory.main(["--root", str(tmp_path)])
    captured = capsys.readouterr()
    assert code == 1
    report = json.loads(captured.out)
    violations = cast("dict[str, list[JsonObject]]", report["duplication"])
    assert len(violations["normal-ci.yml"]) == 1
    assert violations["normal-ci.yml"][0]["logical"] == "test"
    assert "duplication" in captured.err


def test_unknown_check_app_fails_closed() -> None:
    texts = _read_texts()
    old = "        run: nix run .#test\n"
    assert old in texts["normal-ci.yml"]
    texts["normal-ci.yml"] = texts["normal-ci.yml"].replace(
        old, "        run: nix run .#coffee-check\n", 1
    )
    with pytest.raises(ValueError, match="unknown check app"):
        ci_check_inventory.build_inventory(texts)


def test_synthetic_duplicated_command_is_detected_as_duplication() -> None:
    """Mutation A: a second ``nix run .#test`` step in normal-ci text.

    The duplicated ``test`` app on ``ubuntu-latest`` has no documented
    exception, so exactly one duplication violation must surface.
    """
    texts = _read_texts()
    old = "      - name: Run Python tests\n        run: nix run .#test\n"
    assert old in texts["normal-ci.yml"]
    texts["normal-ci.yml"] = texts["normal-ci.yml"].replace(
        old,
        old + "      - name: Run Python tests again\n        run: nix run .#test\n",
        1,
    )
    report = ci_check_inventory.build_inventory(texts)
    duplication = cast("dict[str, list[JsonObject]]", report["duplication"])
    violations = duplication["normal-ci.yml"]
    assert len(violations) == 1
    assert violations[0]["logical"] == "test"
    assert violations[0]["app"] == "test"
    assert violations[0]["env"] == "ubuntu-latest"
    assert len(cast("list[object]", violations[0]["sites"])) == 2
    for workflow in ("package.yml", "release.yml", "compatibility-roll.yml"):
        assert duplication[workflow] == []


def test_removed_site_fails_exact_set_assertion() -> None:
    """Mutation B: deleting the ``nix run .#test`` line from normal-ci text.

    The exact-set pin for ``normal-ci.yml`` must no longer hold, proving the
    pin is sensitive to a dropped logical-check site.
    """
    texts = _read_texts()
    lines = [
        line
        for line in texts["normal-ci.yml"].splitlines(keepends=True)
        if line.strip() != "run: nix run .#test"
    ]
    assert len(lines) < len(texts["normal-ci.yml"].splitlines(keepends=True))
    texts["normal-ci.yml"] = "".join(lines)
    report = ci_check_inventory.build_inventory(texts)
    with pytest.raises(AssertionError):
        assert _site_tuples(report, "normal-ci.yml") == sorted(EXPECTED_NORMAL_CI)
