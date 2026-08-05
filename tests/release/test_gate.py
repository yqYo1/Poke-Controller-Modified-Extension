from __future__ import annotations

import hashlib
import json
from pathlib import Path

import pytest

from scripts.release.gate import validate_release, write_checksums


def test_repository_release_versions_and_contracts_match() -> None:
    root = Path(__file__).resolve().parents[2]
    assert validate_release(root, "v0.1.0") == "0.1.0"
    with pytest.raises(ValueError, match="must equal"):
        validate_release(root, "v0.1.1")


@pytest.mark.parametrize("features", [[], ["contract-generator"]])
def test_release_rejects_tauri_cargo_feature_selection(
    tmp_path: Path, features: list[str]
) -> None:
    root = Path(__file__).resolve().parents[2]
    fixture = tmp_path / "release"
    for relative in (
        "Cargo.toml",
        "CHANGELOG.md",
        "web/package.json",
        "web/bun.lock",
        "rust/pokecon/tauri.conf.json",
        "compatibility/fixed-manifest.json",
        "compatibility/fixed-results.json",
    ):
        source = root / relative
        destination = fixture / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(source.read_bytes())

    config_path = fixture / "rust/pokecon/tauri.conf.json"
    config = json.loads(config_path.read_text(encoding="utf-8"))
    config["build"] = {"features": features}
    config_path.write_text(json.dumps(config), encoding="utf-8")

    with pytest.raises(ValueError, match="must not select Cargo features"):
        validate_release(fixture, "v0.1.0")


def test_checksums_cover_sorted_relative_artifacts(tmp_path: Path) -> None:
    artifacts = tmp_path / "artifacts"
    artifacts.mkdir()
    (artifacts / "z.bin").write_bytes(b"z")
    nested = artifacts / "nested"
    nested.mkdir()
    (nested / "a.bin").write_bytes(b"a")
    output = artifacts / "SHA256SUMS"
    lines = write_checksums(artifacts, output)
    assert lines == [
        f"{hashlib.sha256(b'a').hexdigest()}  nested/a.bin",
        f"{hashlib.sha256(b'z').hexdigest()}  z.bin",
    ]
    assert output.read_text(encoding="utf-8") == "\n".join(lines) + "\n"


@pytest.mark.parametrize(
    "relative_path",
    [
        ".tauri-build.lock",
        ".tauri-publish/package.deb",
        "nested/.tauri-previous/package.deb",
        ".tauri-unexpected/residue",
    ],
)
def test_checksums_reject_transient_tauri_publication_state(
    tmp_path: Path, relative_path: str
) -> None:
    artifacts = tmp_path / "artifacts"
    artifact = artifacts / relative_path
    artifact.parent.mkdir(parents=True)
    artifact.write_bytes(b"residue")

    with pytest.raises(ValueError, match="transient Tauri publication state"):
        write_checksums(artifacts, artifacts / "SHA256SUMS")
