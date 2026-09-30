#!/usr/bin/env python3
"""Validate the tracked GPUI selected-closure license report and NOTICE.

Regenerates the report from current sources, byte-compares it with the
tracked file, checks structural invariants, and verifies that every
unresolved item and dual-license election is reflected in the NOTICE.

Run with::

    nix develop -c python3 -I scripts/licenses/check_gpui_closure_report.py
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any, Final

REPORT_REL_PATH: Final = Path("docs/licenses/gpui-selected-closure-license-report.json")
NOTICE_REL_PATH: Final = Path("docs/licenses/NOTICE-GPUI")
GENERATOR_REL_PATH: Final = Path("scripts/licenses/generate_gpui_closure_report.py")
EXPECTED_SCHEMA: Final = "gpui-selected-closure-license-report"
EXPECTED_SCHEMA_VERSION: Final = 1


def repo_root() -> Path:
    """Return the repository root derived from this script location."""
    return Path(__file__).resolve().parents[2]


def fail(message: str, errors: list[str]) -> None:
    """Record a validation failure."""
    errors.append(message)


def check_sorted(packages: list[dict[str, Any]], errors: list[str]) -> None:
    """Verify deterministic package ordering."""
    keys = [(item["name"], item["version"]) for item in packages]
    if keys != sorted(keys):
        fail("closure packages are not sorted by (name, version)", errors)


def check_report(report: dict[str, Any], errors: list[str]) -> None:
    """Verify structural invariants of the regenerated report."""
    if report.get("schema") != EXPECTED_SCHEMA:
        fail(f"unexpected schema: {report.get('schema')!r}", errors)
    if report.get("schema_version") != EXPECTED_SCHEMA_VERSION:
        fail(f"unexpected schema_version: {report.get('schema_version')!r}", errors)
    for field in (
        "target",
        "feature_set",
        "resolved_pins",
        "inputs",
        "closure",
        "gpui_delta",
        "icon_assets",
        "dual_license_elections",
        "notice_attribution_required",
        "unresolved",
        "scope_notes",
    ):
        if field not in report:
            fail(f"missing top-level field: {field}", errors)
    packages = report.get("closure", {}).get("packages", [])
    if report.get("closure", {}).get("package_count") != len(packages):
        fail("closure package_count does not match packages length", errors)
    check_sorted(packages, errors)
    by_name = {(item["name"], item["version"]): item for item in packages}
    delta = report.get("gpui_delta", {})
    if delta.get("added_count") != len(delta.get("added", [])):
        fail("gpui_delta added_count does not match added length", errors)
    if delta.get("removed_count") != len(delta.get("removed", [])):
        fail("gpui_delta removed_count does not match removed length", errors)
    for item in report.get("unresolved", []):
        key = (item["name"], item["version"])
        if key not in by_name:
            fail(f"unresolved item not in closure: {key[0]}@{key[1]}", errors)
        elif not by_name[key].get("review_reasons"):
            fail(
                f"unresolved item has no review reasons: {key[0]}@{key[1]}",
                errors,
            )
    for item in packages:
        if not item.get("license") and not item.get("license_file"):
            key = (item["name"], item["version"])
            unresolved_keys = {
                (entry["name"], entry["version"])
                for entry in report.get("unresolved", [])
            }
            if key not in unresolved_keys:
                fail(
                    f"package without license metadata is not unresolved: "
                    f"{key[0]}@{key[1]}",
                    errors,
                )
    if report.get("icon_assets", {}).get("shipped_in_closure"):
        fail("gpui-kit-assets unexpectedly entered the closure", errors)


def check_notice(report: dict[str, Any], notice: str, errors: list[str]) -> None:
    """Verify the NOTICE reflects every unresolved item and election."""
    for item in report.get("unresolved", []):
        token = f"{item['name']}@{item['version']}"
        if token not in notice:
            fail(f"NOTICE is missing unresolved item: {token}", errors)
    for token in report.get("notice_attribution_required", []):
        if token not in notice:
            fail(f"NOTICE is missing attribution item: {token}", errors)
    for item in report.get("dual_license_elections", []):
        token = f"{item['name']}@{item['version']}"
        if token not in notice or item["elected"] not in notice:
            fail(f"NOTICE is missing license election: {token}", errors)
    for token in (
        EXPECTED_SCHEMA,
        "MPL-2.0",
        "Lucide",
        "Feather",
        "gpui-kit-assets",
    ):
        if token not in notice:
            fail(f"NOTICE is missing required token: {token}", errors)


def main(argv: list[str] | None = None) -> int:
    """Regenerate, compare, and validate the tracked license artifacts."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.parse_args(argv)
    root = repo_root()
    errors: list[str] = []
    tracked = root / REPORT_REL_PATH
    if not tracked.is_file():
        print(f"missing tracked report: {tracked}", file=sys.stderr)
        return 1
    with tempfile.TemporaryDirectory(prefix="gpui-license-check-") as tmpdir:
        regenerated = Path(tmpdir) / "report.json"
        completed = subprocess.run(  # noqa: S603 - argv lists only
            [
                sys.executable,
                "-I",
                str(root / GENERATOR_REL_PATH),
                "--output",
                str(regenerated),
            ],
            cwd=root,
            capture_output=True,
            check=False,
            text=True,
        )
        if completed.returncode != 0:
            print(f"generator failed:\n{completed.stderr[-2000:]}", file=sys.stderr)
            return 1
        if regenerated.read_bytes() != tracked.read_bytes():
            fail("tracked report differs from regeneration (re-run generator)", errors)
    report = json.loads(tracked.read_text(encoding="utf-8"))
    check_report(report, errors)
    notice_path = root / NOTICE_REL_PATH
    if not notice_path.is_file():
        fail(f"missing NOTICE: {notice_path}", errors)
    else:
        check_notice(report, notice_path.read_text(encoding="utf-8"), errors)
    if errors:
        for message in errors:
            print(f"check_gpui_closure_report: {message}", file=sys.stderr)
        return 1
    print(
        f"PASS: {len(report['closure']['packages'])} packages, "
        f"{report['gpui_delta']['added_count']} gpui-delta, "
        f"{len(report['unresolved'])} unresolved, NOTICE linked"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
