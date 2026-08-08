from __future__ import annotations

import hashlib
import json
import os
import stat
from pathlib import Path
from types import SimpleNamespace
from typing import cast

import pytest

from scripts.release import signing_manifest
from scripts.release.signing_manifest import (
    load_policy,
    signing_inputs,
    verify_signing_inputs,
    write_signing_inputs,
)

REPOSITORY = Path(__file__).resolve().parents[2]


def artifact(path: Path, contents: bytes) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(contents)
    return path


def metadata_with_ctime(
    metadata: os.stat_result,
    ctime_ns: int,
) -> os.stat_result:
    return cast(
        "os.stat_result",
        cast(
            "object",
            SimpleNamespace(
                st_dev=metadata.st_dev,
                st_ino=metadata.st_ino,
                st_mode=metadata.st_mode,
                st_size=metadata.st_size,
                st_mtime_ns=metadata.st_mtime_ns,
                st_ctime_ns=ctime_ns,
            ),
        ),
    )


def test_linux_manifest_records_exact_debian_signing_input(tmp_path: Path) -> None:
    package = artifact(tmp_path / "PokeCon Controller_0.1.0_amd64.deb", b"deb")
    manifest = signing_inputs(REPOSITORY, "linux", package)
    output = tmp_path / "signing-inputs.json"
    write_signing_inputs(output, manifest)

    assert manifest == {
        "platform": "linux",
        "policy": {
            "canonical_sha256": load_policy(REPOSITORY, "linux")[1],
            "path": "rust/pokecon/signing-targets.json",
        },
        "schema_version": 1,
        "targets": [
            {
                "file_name": package.name,
                "format": "deb",
                "role": "bundle",
                "sha256": hashlib.sha256(b"deb").hexdigest(),
                "size": 3,
            }
        ],
        "tauri_root": "rust/pokecon",
    }
    assert json.loads(output.read_text(encoding="utf-8")) == manifest
    assert output.stat().st_mtime == 0
    assert stat.S_IMODE(output.stat().st_mode) == 0o644
    verify_signing_inputs(output, manifest)


def test_windows_manifest_records_exact_nsis_signing_input(tmp_path: Path) -> None:
    installer = artifact(
        tmp_path / "PokeCon Controller_0.1.0_x64-setup.exe", b"installer"
    )

    manifest = signing_inputs(REPOSITORY, "windows", installer)

    assert manifest["targets"] == [
        {
            "file_name": installer.name,
            "format": "nsis",
            "role": "bundle",
            "sha256": hashlib.sha256(b"installer").hexdigest(),
            "size": 9,
        },
    ]


def test_windows_path_fstat_ctime_difference_is_not_a_file_change(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    installer = artifact(tmp_path / "package-setup.exe", b"installer")
    original_fstat = os.fstat

    def windows_fstat(file_descriptor: int) -> os.stat_result:
        metadata = original_fstat(file_descriptor)
        return metadata_with_ctime(metadata, metadata.st_ctime_ns + 1)

    monkeypatch.setattr(
        signing_manifest,
        "PATH_STAT_CTIME_IS_CREATION_TIME",
        True,
    )
    monkeypatch.setattr(
        "scripts.release.signing_manifest.os.fstat",
        windows_fstat,
    )

    manifest = signing_inputs(REPOSITORY, "windows", installer)

    assert manifest["targets"]


def test_windows_handle_ctime_change_during_read_is_rejected(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    installer = artifact(tmp_path / "package-setup.exe", b"installer")
    original_fstat = os.fstat
    call_count = 0

    def changing_fstat(file_descriptor: int) -> os.stat_result:
        nonlocal call_count
        metadata = original_fstat(file_descriptor)
        call_count += 1
        return metadata_with_ctime(metadata, metadata.st_ctime_ns + call_count)

    monkeypatch.setattr(
        signing_manifest,
        "PATH_STAT_CTIME_IS_CREATION_TIME",
        True,
    )
    monkeypatch.setattr(
        "scripts.release.signing_manifest.os.fstat",
        changing_fstat,
    )

    with pytest.raises(ValueError, match="changed while it was read"):
        signing_manifest.read_regular_bytes(installer, "installer")


@pytest.mark.parametrize(
    ("platform", "bundle_name"),
    [
        ("linux", "package.tar.gz"),
        ("windows", "package.exe"),
    ],
)
def test_manifest_rejects_inputs_outside_the_root_policy(
    tmp_path: Path,
    platform: str,
    bundle_name: str,
) -> None:
    bundle = artifact(tmp_path / bundle_name, b"bundle")

    with pytest.raises(ValueError, match="must end with"):
        signing_inputs(REPOSITORY, platform, bundle)


def test_manifest_rejects_redirected_and_existing_outputs(tmp_path: Path) -> None:
    package_source = artifact(tmp_path / "source.deb", b"deb")
    package = tmp_path / "package.deb"
    package.symlink_to(package_source)
    with pytest.raises(ValueError, match="stable real regular file"):
        signing_inputs(REPOSITORY, "linux", package)

    output = artifact(tmp_path / "signing-inputs.json", b"existing")
    with pytest.raises(ValueError, match="already exists"):
        write_signing_inputs(output, {"schema_version": 1})
    assert output.read_bytes() == b"existing"


@pytest.mark.parametrize(
    ("policy_bytes", "message"),
    [
        (b'{"schema_version":1,"schema_version":1}', "repeats key"),
        ('{"schema_version":1}'.encode("utf-16"), "not strict UTF-8 JSON"),
        (
            (REPOSITORY / "rust/pokecon/signing-targets.json")
            .read_bytes()
            .replace(b'"schema_version": 1', b'"schema_version": true'),
            "must be integer 1",
        ),
    ],
)
def test_manifest_rejects_noncanonical_policy_encoding_and_keys(
    tmp_path: Path,
    policy_bytes: bytes,
    message: str,
) -> None:
    policy = tmp_path / "rust/pokecon/signing-targets.json"
    policy.parent.mkdir(parents=True)
    policy.write_bytes(policy_bytes)

    with pytest.raises(ValueError, match=message):
        load_policy(tmp_path, "linux")


def test_policy_hash_is_independent_of_checkout_line_endings(tmp_path: Path) -> None:
    policy_bytes = (REPOSITORY / "rust/pokecon/signing-targets.json").read_bytes()
    hashes: list[str] = []
    for name, contents in (
        ("lf", policy_bytes),
        ("crlf", policy_bytes.replace(b"\n", b"\r\n")),
    ):
        policy = tmp_path / name / "rust/pokecon/signing-targets.json"
        policy.parent.mkdir(parents=True)
        policy.write_bytes(contents)
        hashes.append(load_policy(tmp_path / name, "linux")[1])

    assert len(set(hashes)) == 1


def test_manifest_verification_rejects_a_stale_target(tmp_path: Path) -> None:
    package = artifact(tmp_path / "package.deb", b"original")
    manifest = signing_inputs(REPOSITORY, "linux", package)
    output = tmp_path / "signing-inputs.json"
    write_signing_inputs(output, manifest)

    package.write_bytes(b"replacement")
    current = signing_inputs(REPOSITORY, "linux", package)
    with pytest.raises(ValueError, match="does not match its current targets"):
        verify_signing_inputs(output, current)


def test_package_and_release_workflows_preserve_signing_manifests() -> None:
    package = (REPOSITORY / ".github/workflows/package.yml").read_text()
    release = (REPOSITORY / ".github/workflows/release.yml").read_text()
    flake = (REPOSITORY / "flake.nix").read_text()

    linux_command = (
        "nix run .#signing-input-manifest --\n"
        "          --platform linux\n"
        "          --bundle dist/tauri/*.deb\n"
        "          --output dist/tauri/signing-inputs.json"
    )
    windows_command = (
        "python -m scripts.release.signing_manifest `\n"
        "            --platform windows `\n"
        "            --bundle $installer `\n"
        "            --output target/release/bundle/nsis/signing-inputs.json"
    )
    for workflow in (package, release):
        assert "--application" not in workflow
        assert workflow.count(linux_command) == 1
        assert workflow.count(windows_command) == 1
        assert (
            workflow.index("cargo tauri build --ci --bundles nsis")
            < workflow.index(windows_command)
            < workflow.index("windows_install_smoke.ps1")
        )
    package_windows_verify = "--verify target/release/bundle/nsis/signing-inputs.json"
    assert (
        package.index("windows_install_smoke.ps1")
        < package.index(package_windows_verify)
        < package.index(
            "actions/upload-artifact@v7", package.index(package_windows_verify)
        )
    )
    release_windows_verify = "--verify dist/signing-inputs-windows-x86_64.json"
    assert "Copy-Item target/release/bundle/nsis/*.exe dist/" in release
    assert (
        release.index("Copy-Item target/release/bundle/nsis/*.exe dist/")
        < (release.index(release_windows_verify))
        < release.index(
            "python -m scripts.release.gate", release.index(windows_command)
        )
    )
    assert package.count("dist/tauri/signing-inputs.json") == 3
    assert package.count("target/release/bundle/nsis/signing-inputs.json") == 3
    assert release.count("dist/tauri/signing-inputs.json") == 2
    assert release.count("target/release/bundle/nsis/signing-inputs.json") == 2
    assert "mv dist/tauri/*.deb dist/" in release
    assert "cp dist/tauri/signing-inputs.json" not in release
    assert "--verify dist/signing-inputs-linux-x86_64.json" in release
    assert "dist/signing-inputs-linux-x86_64.json" in release
    assert "dist/signing-inputs-windows-x86_64.json" in release
    assert "signing-input-manifest = mkTask {" in flake
    assert (
        'python -I "${repositorySource}/scripts/release/signing_manifest.py" "$@"'
        in flake
    )
