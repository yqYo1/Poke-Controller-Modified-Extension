from __future__ import annotations

import hashlib
import json
from pathlib import Path

import pytest

from scripts.release.gate import validate_release, write_checksums


def workflow_section(document: str, start: str, end: str) -> str:
    _prefix, separator, tail = document.partition(start)
    assert separator, f"missing workflow section start: {start}"
    body, separator, _suffix = tail.partition(end)
    assert separator, f"missing workflow section end after: {start}"
    return body


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


def test_package_ci_builds_debian_reproducibility_proof_in_parallel() -> None:
    root = Path(__file__).resolve().parents[2]
    workflow = (root / ".github/workflows/package.yml").read_text(encoding="utf-8")
    flake = (root / "flake.nix").read_text(encoding="utf-8")
    primary = workflow_section(
        workflow,
        "  linux:\n",
        "  linux-reproducibility-build:\n",
    )
    reproduction = workflow_section(
        workflow,
        "  linux-reproducibility-build:\n",
        "  linux-reproducibility:\n",
    )
    comparison = workflow_section(
        workflow,
        "  linux-reproducibility:\n",
        "  windows:\n",
    )

    build_command = "nix run .#tauri-build -- --bundles deb"
    assert primary.count(build_command) == 1
    assert reproduction.count(build_command) == 1
    assert comparison.count(build_command) == 0
    assert "needs:" not in primary
    assert "needs:" not in reproduction
    assert comparison.count("needs: [linux, linux-reproducibility-build]") == 1
    assert "Preserve first package build" not in workflow
    assert "Rebuild Debian package from identical inputs" not in workflow

    assert primary.count("name: package-linux-x86_64\n") == 1
    assert reproduction.count("name: package-linux-x86_64-reproducibility\n") == 1
    assert primary.count("retention-days: 3") == 1
    assert reproduction.count("retention-days: 3") == 1
    for artifact_name in (
        "package-linux-x86_64",
        "package-linux-x86_64-reproducibility",
    ):
        assert comparison.count(f"name: {artifact_name}\n") == 1
    for comparison_setup in (
        "actions/checkout@v6",
        "cachix/install-nix-action@v31",
        "actions/download-artifact@v8",
        "nix run .#package-reproducibility-check -- primary reproduction",
    ):
        assert comparison_setup in comparison

    reproducibility_app = workflow_section(
        flake,
        "            package-reproducibility-check = mkTask {\n",
        "            package-install-smoke = mkTask {\n",
    )
    for comparison_proof in (
        "pkgs.diffutils",
        "pkgs.findutils",
        'if [ "$#" -ne 2 ]',
        'mapfile -d "" -t primary_bundles',
        'mapfile -d "" -t reproduction_bundles',
        "find -P \"$primary_root\" -type f -name '*.deb' -print0",
        "find -P \"$reproduction_root\" -type f -name '*.deb' -print0",
        "if [ \"''${#primary_bundles[@]}\" -ne 1 ]",
        "sha256sum -- \"''${primary_bundles[0]}\" \"''${reproduction_bundles[0]}\"",
        "cmp -- \"''${primary_bundles[0]}\" \"''${reproduction_bundles[0]}\"",
    ):
        assert comparison_proof in reproducibility_app
    assert reproducibility_app.index("sha256sum --") < reproducibility_app.index(
        "cmp --"
    )
