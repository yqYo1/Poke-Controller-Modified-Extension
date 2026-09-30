#!/usr/bin/env python3
"""Generate the deterministic GPUI selected-closure license report.

Source-based inventory of the exact ``x86_64-unknown-linux-gnu`` dependency
closure selected by the ``gpui`` feature. Uses live ``cargo metadata`` output
(``--locked``) and the checked-out registry sources, never the upstream union
lockfile. Fails closed when license or source metadata is missing.

Regenerate with::

    nix develop -c python3 -I scripts/licenses/generate_gpui_closure_report.py

Validate with::

    nix develop -c python3 -I scripts/licenses/check_gpui_closure_report.py
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any, Final

SCHEMA: Final = "gpui-selected-closure-license-report"
SCHEMA_VERSION: Final = 1
TARGET: Final = "x86_64-unknown-linux-gnu"
REPORT_REL_PATH: Final = Path("docs/licenses/gpui-selected-closure-license-report.json")

PERMISSIVE_CHOICES: Final = frozenset(
    {
        "0BSD",
        "Apache-2.0",
        "BSD-2-Clause",
        "BSD-3-Clause",
        "CC0-1.0",
        "ISC",
        "MIT",
        "MIT-0",
        "Unlicense",
        "Zlib",
        "BSL-1.0",
        "bzip2-1.0.6",
        "Unicode-3.0",
    }
)

COPYLEFT_TOKENS: Final = frozenset(
    {
        "GPL-2.0-only",
        "GPL-3.0-only",
        "LGPL-2.0-only",
        "LGPL-2.1-only",
        "LGPL-2.1-or-later",
        "LGPL-3.0-only",
        "AGPL-3.0-only",
        "MPL-2.0",
    }
)

LICENSE_TEXT_NAMES: Final = frozenset(
    {
        "license",
        "license.txt",
        "license.md",
        "license-apache",
        "license-mit",
        "license-gplv2",
        "license-gplv3",
        "license-lucide",
        "license-mpl",
        "licence",
        "licence.txt",
        "copying",
        "copyright",
        "notice",
        "unlicense",
    }
)


def repo_root() -> Path:
    """Return the repository root derived from this script location."""
    return Path(__file__).resolve().parents[2]


def sha256_file(path: Path) -> str:
    """Return the hex SHA-256 of a file."""
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run_argv(argv: list[str], root: Path) -> str:
    """Run a fixed argv list and return stdout, failing closed on error."""
    completed = subprocess.run(  # noqa: S603 - argv lists only; never shell-expanded
        argv,
        cwd=root,
        capture_output=True,
        check=False,
        text=True,
    )
    if completed.returncode != 0:
        message = (
            f"command failed ({completed.returncode}): {' '.join(argv)}\n"
            f"{completed.stderr[-2000:]}"
        )
        raise RuntimeError(message)
    return completed.stdout


def cargo_metadata(root: Path, with_gpui: bool) -> dict[str, Any]:
    """Return parsed ``cargo metadata`` for the selected target closure."""
    cargo = shutil.which("cargo")
    if cargo is None:
        message = "cargo not found on PATH (run inside nix develop)"
        raise RuntimeError(message)
    argv = [
        cargo,
        "metadata",
        "--locked",
        "--format-version",
        "1",
        "--filter-platform",
        TARGET,
        "--manifest-path",
        "Cargo.toml",
    ]
    if with_gpui:
        argv.extend(["--features", "gpui"])
    return json.loads(run_argv(argv, root))


def tool_version(binary: str, root: Path) -> str:
    """Return ``<binary> --version`` output for provenance."""
    exe = shutil.which(binary)
    if exe is None:
        return "unavailable"
    return run_argv([exe, "--version"], root).strip().splitlines()[0]


def lock_checksums(root: Path) -> dict[tuple[str, str], str]:
    """Map (name, version) to the Cargo.lock checksum for registry packages."""
    with (root / "Cargo.lock").open("rb") as handle:
        lock = tomllib.load(handle)
    checksums: dict[tuple[str, str], str] = {}
    for package in lock.get("package", []):
        checksum = package.get("checksum")
        if checksum:
            checksums[(package["name"], package["version"])] = checksum
    return checksums


def registry_src_dir() -> Path | None:
    """Return the crates.io registry source directory, if exactly one exists."""
    cargo_home = Path(
        os.environ.get("CARGO_HOME", str(Path.home() / ".cargo")),
    )
    candidates = sorted((cargo_home / "registry" / "src").glob("index.crates.io-*"))
    if len(candidates) == 1:
        return candidates[0]
    return None


def license_text_files(source_dir: Path | None) -> list[str]:
    """List license-text basenames present in an extracted crate source."""
    if source_dir is None or not source_dir.is_dir():
        return []
    return [
        child.name
        for child in sorted(source_dir.iterdir())
        if child.is_file() and child.name.lower() in LICENSE_TEXT_NAMES
    ]


def split_license_alternatives(declared: str) -> list[str]:
    """Split a license expression on top-level OR alternatives."""
    return [part.strip(" ()") for part in declared.split(" OR ")]


def classify(
    package: dict[str, Any],
    checksum: str | None,
    text_files: list[str],
) -> tuple[list[str], str | None]:
    """Return (review reasons, elected permissive license or None)."""
    reasons: list[str] = []
    elected: str | None = None
    declared: str | None = package.get("license")
    source: str | None = package.get("source")
    is_registry = (source or "").startswith("registry")
    if not declared and not package.get("license_file"):
        reasons.append("missing-license")
        return reasons, None
    if declared is None:
        return reasons, None
    if is_registry and checksum is None:
        reasons.append("no-lock-checksum")
    if source is not None and not is_registry and not source.startswith("path"):
        reasons.append("non-registry-source")
    if declared.strip() == "MPL-2.0":
        reasons.append("mpl-2.0-source-offer")
    elif any(
        token in COPYLEFT_TOKENS for token in split_license_alternatives(declared)
    ):
        alternatives = split_license_alternatives(declared)
        permissive = [a for a in alternatives if a in PERMISSIVE_CHOICES]
        if permissive:
            elected = sorted(permissive)[0]
        else:
            reasons.append("no-permissive-alternative")
    if is_registry and not text_files:
        reasons.append("license-text-absent-from-crate")
    return reasons, elected


def build_report(root: Path) -> dict[str, Any]:
    """Build the deterministic report document from current sources."""
    with (root / "Cargo.toml").open("rb") as handle:
        workspace_manifest = tomllib.load(handle)
    with (root / "rust" / "pokecon" / "Cargo.toml").open("rb") as handle:
        pokecon_manifest = tomllib.load(handle)
    gpui_req = workspace_manifest["workspace"]["dependencies"]["gpui-kit"]
    gpui_pin = gpui_req["version"] if isinstance(gpui_req, dict) else gpui_req
    gpui_default_features = (
        gpui_req.get("default-features", True) if isinstance(gpui_req, dict) else True
    )

    gpui_meta = cargo_metadata(root, with_gpui=True)
    base_meta = cargo_metadata(root, with_gpui=False)
    checksums = lock_checksums(root)
    registry_dir = registry_src_dir()

    gpui_nodes = {node["id"]: node for node in gpui_meta["resolve"]["nodes"]}
    base_ids = {node["id"] for node in base_meta["resolve"]["nodes"]}
    packages = {package["id"]: package for package in gpui_meta["packages"]}

    entries: list[dict[str, Any]] = []
    unresolved: list[dict[str, Any]] = []
    attribution_required: list[str] = []
    elections: list[dict[str, Any]] = []
    for package_id in sorted(
        gpui_nodes, key=lambda pid: (packages[pid]["name"], packages[pid]["version"])
    ):
        package = packages[package_id]
        name = package["name"]
        version = package["version"]
        source = package.get("source")
        if source is None:
            source_kind = "path"
        elif source.startswith("registry"):
            source_kind = "registry"
        elif source.startswith("git"):
            source_kind = "git"
        else:
            source_kind = source
        checksum = checksums.get((name, version))
        if source_kind == "registry" and registry_dir is not None:
            source_dir = registry_dir / f"{name}-{version}"
            relpath = f"{registry_dir.name}/{name}-{version}"
        elif source_kind == "path" and name == "pokecon":
            source_dir = root
            relpath = "."
        else:
            source_dir = None
            relpath = None
        text_files = license_text_files(source_dir)
        reasons, elected = classify(package, checksum, text_files)
        node = gpui_nodes[package_id]
        entries.append(
            {
                "name": name,
                "version": version,
                "license": package.get("license"),
                "license_file": package.get("license_file"),
                "repository": package.get("repository"),
                "source": source_kind,
                "checksum": checksum,
                "source_relpath": relpath,
                "license_text_files": text_files,
                "in_gpui_delta": package_id not in base_ids,
                "resolved_features": sorted(node.get("features", [])),
                "review_reasons": sorted(reasons),
            }
        )
        if elected is not None:
            elections.append(
                {
                    "name": name,
                    "version": version,
                    "declared": package.get("license"),
                    "elected": elected,
                }
            )
        if reasons == ["license-text-absent-from-crate"]:
            attribution_required.append(f"{name}@{version}")
        elif reasons:
            unresolved.append(
                {
                    "name": name,
                    "version": version,
                    "license": package.get("license"),
                    "source": source_kind,
                    "checksum": checksum,
                    "reasons": sorted(reasons),
                }
            )

    added = sorted(
        f"{packages[package_id]['name']}@{packages[package_id]['version']}"
        for package_id in gpui_nodes
        if package_id not in base_ids
    )
    removed = sorted(
        f"{packages[package_id]['name']}@{packages[package_id]['version']}"
        for package_id in base_ids
        if package_id not in gpui_nodes
    )

    def resolved_version(crate: str) -> str | None:
        for package in packages.values():
            if package["name"] == crate:
                return package["version"]
        return None

    assets_in_closure = any(
        package["name"] == "gpui-kit-assets" for package in packages.values()
    )
    assets_version = resolved_version("gpui-kit-assets")
    if assets_version is None:
        with (root / "Cargo.lock").open("rb") as handle:
            lock_doc = tomllib.load(handle)
        for entry in lock_doc.get("package", []):
            if entry.get("name") == "gpui-kit-assets":
                assets_version = entry.get("version")
                break
    assets_lock = checksums.get(("gpui-kit-assets", assets_version or ""))
    assets_text: list[str] = []
    if registry_dir is not None and assets_version:
        assets_text = license_text_files(
            registry_dir / f"gpui-kit-assets-{assets_version}"
        )

    input_files = ["Cargo.toml", "Cargo.lock", "rust/pokecon/Cargo.toml"]
    return {
        "schema": SCHEMA,
        "schema_version": SCHEMA_VERSION,
        "target": TARGET,
        "feature_set": {
            "package": "pokecon",
            "enabled_features": ["gpui"],
            "pokecon_gpui_feature": pokecon_manifest["features"]["gpui"],
            "pokecon_default_features": pokecon_manifest["features"]["default"],
            "gpui_kit_manifest_pin": gpui_pin,
            "gpui_kit_default_features": gpui_default_features,
        },
        "resolved_pins": {
            "gpui-kit": resolved_version("gpui-kit"),
            "gpui-base": resolved_version("gpui-base"),
            "gpui-pre": resolved_version("gpui-pre"),
        },
        "inputs": {
            "files": {rel: sha256_file(root / rel) for rel in input_files},
            "cargo_version": tool_version("cargo", root),
            "rustc_version": tool_version("rustc", root),
            "command": (
                "cargo metadata --locked --format-version 1 "
                f"--filter-platform {TARGET} --features gpui "
                "--manifest-path Cargo.toml"
            ),
        },
        "closure": {
            "package_count": len(entries),
            "packages": entries,
        },
        "gpui_delta": {
            "added_count": len(added),
            "removed_count": len(removed),
            "added": added,
            "removed": removed,
        },
        "icon_assets": {
            "crate": "gpui-kit-assets",
            "version": assets_version,
            "shipped_in_closure": assets_in_closure,
            "lock_checksum": assets_lock,
            "license_text_in_pinned_source": assets_text,
            "note": (
                "Lucide ISC and Feather MIT icon texts apply only when "
                "gpui-kit-assets ships; it is outside this closure."
            ),
        },
        "dual_license_elections": sorted(
            elections, key=lambda item: (item["name"], item["version"])
        ),
        "notice_attribution_required": sorted(attribution_required),
        "unresolved": sorted(
            unresolved, key=lambda item: (item["name"], item["version"])
        ),
        "scope_notes": [
            (
                "Selected closure only: x86_64-unknown-linux-gnu with the "
                "pokecon gpui feature. Other targets, features, and the upstream "
                "union lockfile are out of scope."
            ),
            (
                "No git sources in this closure; all non-path packages are "
                "crates.io registry packages with Cargo.lock checksums."
            ),
            (
                "This report covers Cargo sources only. Nix system libraries "
                "(fontconfig, freetype, libxkbcommon, XCB, Mesa, Vulkan loader, "
                "GTK/WebKitGTK via the Tauri path) need a separate runtime "
                "closure review."
            ),
            (
                "Technical inventory only; not legal advice and not a "
                "redistribution approval."
            ),
        ],
    }


def main(argv: list[str] | None = None) -> int:
    """Generate the report and write it deterministically."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output",
        type=Path,
        default=REPORT_REL_PATH,
        help="Report path (relative paths resolve against the repo root).",
    )
    args = parser.parse_args(argv)
    root = repo_root()
    output = args.output if args.output.is_absolute() else root / args.output
    try:
        report = build_report(root)
    except RuntimeError as exc:
        print(f"generate_gpui_closure_report: {exc}", file=sys.stderr)
        return 1
    text = json.dumps(report, indent=2, sort_keys=True) + "\n"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(text, encoding="utf-8")
    print(
        f"wrote {output} ({len(report['closure']['packages'])} packages, "
        f"{len(report['unresolved'])} unresolved, "
        f"{len(report['notice_attribution_required'])} notice-attributions)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
