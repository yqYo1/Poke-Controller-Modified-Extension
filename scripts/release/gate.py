"""Validate release versions/contracts and generate artifact SHA-256 sums."""

from __future__ import annotations

import argparse
import hashlib
import json
import tomllib
from pathlib import Path
from typing import Never, cast


def invalid_value(message: str) -> Never:
    raise ValueError(message)


def mapping(value: object, label: str) -> dict[str, object]:
    if not isinstance(value, dict):
        invalid_value(f"{label} must be an object")
    untyped = cast("dict[object, object]", value)
    if not all(isinstance(key, str) for key in untyped):
        invalid_value(f"{label} must be an object")
    return {cast("str", key): item for key, item in untyped.items()}


def string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        invalid_value(f"{label} must be a non-empty string")
    return value


def load_json(path: Path) -> dict[str, object]:
    raw: object = json.loads(path.read_text(encoding="utf-8"))
    return mapping(raw, str(path))


def workspace_version(root: Path) -> str:
    cargo: object = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    workspace = mapping(mapping(cargo, "Cargo.toml").get("workspace"), "workspace")
    package = mapping(workspace.get("package"), "workspace.package")
    return string(package.get("version"), "workspace.package.version")


def validate_release(root: Path, tag: str | None = None) -> str:
    version = workspace_version(root)
    web = load_json(root / "web/package.json")
    if not (root / "web/bun.lock").is_file():
        invalid_value("web/bun.lock is required for a reproducible release")
    versions = {
        "web/package.json": web.get("version"),
    }
    mismatches = [name for name, value in versions.items() if value != version]
    if mismatches:
        invalid_value(f"release version differs in: {', '.join(mismatches)}")
    if tag is not None and tag != f"v{version}":
        invalid_value(f"release tag {tag!r} must equal v{version}")

    changelog = (root / "CHANGELOG.md").read_text(encoding="utf-8")
    if f"## {version} " not in changelog:
        invalid_value(f"CHANGELOG.md has no release heading for {version}")
    tauri = load_json(root / "rust/pokecon/tauri.conf.json")
    bundle = mapping(tauri.get("bundle"), "Tauri bundle")
    build = mapping(tauri.get("build"), "Tauri build")
    if bundle.get("active") is not True:
        invalid_value("Tauri bundle must be active for release")
    if "tauri-shell" not in cast("list[object]", build.get("features")):
        invalid_value("Tauri release build must enable tauri-shell")

    manifest = load_json(root / "compatibility/fixed-manifest.json")
    results = load_json(root / "compatibility/fixed-results.json")
    summary = mapping(results.get("summary"), "compatibility result summary")
    if (
        results.get("manifest_sha256") != manifest.get("inventory_sha256")
        or summary.get("result") != "passed"
        or summary.get("baseline_count") != 3
        or summary.get("script_count") != 103
        or summary.get("failed") != 0
    ):
        invalid_value("fixed compatibility release gate has not passed")
    return version


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while block := source.read(64 * 1024):
            digest.update(block)
    return digest.hexdigest()


def write_checksums(artifacts: Path, output: Path) -> list[str]:
    if not artifacts.is_dir():
        invalid_value(f"artifact directory does not exist: {artifacts}")
    transient_paths = sorted(
        path.relative_to(artifacts).as_posix()
        for path in artifacts.rglob("*")
        if any(
            part == ".tauri-build.lock" or part.startswith(".tauri-")
            for part in path.relative_to(artifacts).parts
        )
    )
    if transient_paths:
        invalid_value(
            "artifact directory contains transient Tauri publication state: "
            + ", ".join(transient_paths)
        )
    output_resolved = output.resolve()
    files = sorted(
        path
        for path in artifacts.rglob("*")
        if path.is_file() and path.resolve() != output_resolved
    )
    if not files:
        invalid_value(f"artifact directory is empty: {artifacts}")
    lines = [
        f"{sha256_file(path)}  {path.relative_to(artifacts).as_posix()}"
        for path in files
    ]
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return lines


def main() -> int:
    root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=root)
    parser.add_argument("--tag")
    parser.add_argument("--artifacts", type=Path)
    parser.add_argument("--checksums", type=Path)
    arguments = parser.parse_args()
    version = validate_release(arguments.root, arguments.tag)
    if (arguments.artifacts is None) != (arguments.checksums is None):
        parser.error("--artifacts and --checksums must be provided together")
    if arguments.artifacts is not None and arguments.checksums is not None:
        write_checksums(arguments.artifacts, arguments.checksums)
    print(version)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
