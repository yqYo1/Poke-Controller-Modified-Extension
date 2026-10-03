from __future__ import annotations

import hashlib
import json
import stat
from pathlib import Path
from typing import cast

import pytest

import scripts.release.stage as release_stage
from scripts.release.stage import (
    bundle_resource_map,
    resource_inventory,
    stage_resources,
)


def executable(path: Path, content: bytes = b"fixture") -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content)
    path.chmod(0o755)


def stage_fixture(
    root: Path,
    name: str,
    *,
    python_executable_path: str = "bin/python3.14",
    python_executable_symlink: bool = False,
    python_bytecode_path: str | None = None,
    windows: bool = False,
) -> tuple[dict[str, object], Path, Path]:
    web = root / "inputs/web"
    web.mkdir(parents=True, exist_ok=True)
    (web / "index.html").write_text("<main>PokeCon</main>", encoding="utf-8")
    worker_name = "pokecon-worker.exe" if windows else "pokecon-worker"
    uv_name = "uv.exe" if windows else "uv"
    executable(root / "inputs" / worker_name)
    executable(root / "inputs" / uv_name)
    wheelhouse = root / "inputs/python-wheels"
    wheelhouse.mkdir(exist_ok=True)
    (wheelhouse / "fixture-1.0-py3-none-any.whl").write_bytes(b"wheel")
    (wheelhouse / "requirements.lock").write_text("fixture==1.0\n", encoding="utf-8")
    (wheelhouse / "wheelhouse-manifest.json").write_text(
        '{"schema_version":1}\n', encoding="utf-8"
    )
    python = root / "inputs/python"
    selected_python = python / python_executable_path
    if python_executable_symlink:
        python_source = root / "inputs/python-executable-source"
        executable(python_source, b"python-runtime")
        selected_python.parent.mkdir(parents=True, exist_ok=True)
        selected_python.symlink_to(python_source)
    else:
        executable(selected_python, b"python-runtime")
    (python / "module.py").write_text("VALUE = 1\n", encoding="utf-8")
    if not windows:
        python_alias = python / "python"
        if not python_alias.is_symlink():
            python_alias.symlink_to(python_executable_path)
    if python_bytecode_path is not None:
        bytecode = python / python_bytecode_path
        bytecode.parent.mkdir(parents=True, exist_ok=True)
        bytecode.write_bytes(b"unstable")
    output = root / name
    config = root / f"{name}.json"
    manifest = stage_resources(
        web,
        root / "inputs" / worker_name,
        root / "inputs" / uv_name,
        wheelhouse,
        python,
        output,
        config,
    )
    return manifest, output, config


def assert_manifest_file(
    manifest: dict[str, object], output: Path, relative: str, contents: bytes
) -> None:
    raw_entries = manifest["files"]
    assert isinstance(raw_entries, list)
    entries = cast("list[object]", raw_entries)
    matching: list[dict[str, object]] = []
    for entry in entries:
        if not isinstance(entry, dict):
            continue
        typed_entry = cast("dict[str, object]", entry)
        if typed_entry.get("path") == relative:
            matching.append(typed_entry)
    assert matching == [
        {
            "path": relative,
            "sha256": hashlib.sha256(contents).hexdigest(),
            "size": len(contents),
        }
    ]
    canonical = output / relative
    assert canonical.read_bytes() == contents
    assert canonical.stat().st_mode & stat.S_IXUSR
    assert canonical.stat().st_mtime == 0


def test_release_staging_is_deterministic_and_complete(tmp_path: Path) -> None:
    first, first_output, first_config = stage_fixture(tmp_path, "first")
    second, second_output, _second_config = stage_fixture(tmp_path, "second")
    assert first["content_sha256"] == second["content_sha256"]
    assert first["files"] == second["files"]
    assert (first_output / "web/dist/index.html").is_file()
    assert (first_output / "pokecon-worker").is_file()
    assert (first_output / "uv/uv").is_file()
    assert (first_output / "python-wheels/fixture-1.0-py3-none-any.whl").is_file()
    assert (first_output / "python/bin/python3.14").is_file()
    assert_manifest_file(
        first, first_output, "python/bin/python3.14", b"python-runtime"
    )
    assert not (first_output / "python/python").exists()
    assert not (first_output / "python/__pycache__").exists()
    assert json.loads(first_config.read_text(encoding="utf-8"))["bundle"][
        "resources"
    ] == {f"{first_output.resolve()}/": ""}
    assert (first_output / "resource-manifest.json").stat().st_mtime == 0
    assert (second_output / "resource-manifest.json").stat().st_mtime == 0


@pytest.mark.parametrize(
    "python_bytecode_path",
    ["__pycache__/module.cpython-314.pyc", "loose.pyc", "loose.pyo"],
)
@pytest.mark.parametrize("windows", [False, True])
def test_release_staging_rejects_python_bytecode_without_copying(
    tmp_path: Path,
    python_bytecode_path: str,
    windows: bool,
) -> None:
    output = tmp_path / "rejected"
    config = tmp_path / "rejected.json"

    with pytest.raises(ValueError, match="contains bytecode artifact"):
        stage_fixture(
            tmp_path,
            "rejected",
            python_bytecode_path=python_bytecode_path,
            windows=windows,
        )

    bytecode = tmp_path / "inputs/python" / python_bytecode_path
    assert bytecode.read_bytes() == b"unstable"
    assert (tmp_path / "inputs/python/module.py").read_text(encoding="utf-8") == (
        "VALUE = 1\n"
    )
    assert not output.exists()
    assert not config.exists()


def test_release_staging_post_copy_audit_rejects_late_bytecode(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    output = tmp_path / "late-rejected"
    config = tmp_path / "late-rejected.json"
    late_bytecode = output / "python/__pycache__/late.cpython-314.pyc"
    original_copy_tree = release_stage.copy_tree

    def copy_tree_with_late_bytecode(source: Path, destination: Path) -> None:
        original_copy_tree(source, destination)
        if destination == output / "python":
            late_bytecode.parent.mkdir(parents=True)
            late_bytecode.write_bytes(b"late-bytecode")

    monkeypatch.setattr(
        release_stage,
        "copy_tree",
        copy_tree_with_late_bytecode,
    )

    with pytest.raises(ValueError, match="contains bytecode artifact"):
        stage_fixture(tmp_path, "late-rejected")

    assert not (tmp_path / "inputs/python/__pycache__").exists()
    assert late_bytecode.read_bytes() == b"late-bytecode"
    assert not (output / "resource-manifest.json").exists()
    assert not config.exists()


def test_release_staging_does_not_follow_python_junctions(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    redirected_directory = tmp_path / "inputs/python/redirected"
    redirected_bytecode = redirected_directory / "__pycache__/outside.pyc"
    original_is_junction = Path.is_junction

    def fake_is_junction(path: Path) -> bool:
        if path == redirected_directory:
            return True
        return original_is_junction(path)

    monkeypatch.setattr(Path, "is_junction", fake_is_junction)

    _manifest, output, _config = stage_fixture(
        tmp_path,
        "junction-safe",
        python_bytecode_path="redirected/__pycache__/outside.pyc",
    )

    assert redirected_bytecode.read_bytes() == b"unstable"
    assert not (output / "python/redirected").exists()


def test_unix_fallback_python_is_staged_at_canonical_path(tmp_path: Path) -> None:
    manifest, output, _config = stage_fixture(
        tmp_path,
        "unix-fallback",
        python_executable_path="bin/python3",
        python_executable_symlink=True,
    )

    assert not (output / "python/bin/python3").exists()
    assert_manifest_file(manifest, output, "python/bin/python3.14", b"python-runtime")


def test_windows_fallback_python_is_staged_at_canonical_path(tmp_path: Path) -> None:
    manifest, output, _config = stage_fixture(
        tmp_path,
        "windows-fallback",
        python_executable_path="bin/python.exe",
        windows=True,
    )

    assert (output / "python/bin/python.exe").is_file()
    assert_manifest_file(manifest, output, "python/python.exe", b"python-runtime")


def test_windows_canonical_python_input_remains_manifested(tmp_path: Path) -> None:
    manifest, output, _config = stage_fixture(
        tmp_path,
        "windows-canonical",
        python_executable_path="python.exe",
        windows=True,
    )

    assert_manifest_file(manifest, output, "python/python.exe", b"python-runtime")


def test_windows_bundle_map_maps_every_file_explicitly(tmp_path: Path) -> None:
    output = tmp_path / "bundle-resources"
    (output / "python").mkdir(parents=True)
    (output / "python/python.exe").write_bytes(b"python")
    (output / "pokecon-worker.exe").write_bytes(b"worker")
    (output / "resource-manifest.json").write_text("{}\n", encoding="utf-8")

    assert bundle_resource_map(output, windows=True) == {
        str((output / "python/python.exe").resolve()): "python/python.exe",
        str((output / "pokecon-worker.exe").resolve()): "pokecon-worker.exe",
        str((output / "resource-manifest.json").resolve()): ("resource-manifest.json"),
    }


def test_resource_inventory_orders_raw_emitted_posix_paths(tmp_path: Path) -> None:
    root = tmp_path / "resources"
    paths = [
        "python/include/Python.h",
        "python/Lib/site.py",
        "python/DLLs/_ssl.pyd",
        "python/resource-manifest.json",
        "web/dist/resource-manifest.json",
    ]
    for relative in paths:
        destination = root / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(relative.encode())
    (root / "resource-manifest.json").write_bytes(b"top-level manifest")

    inventory = resource_inventory(root)
    emitted = [entry["path"] for entry in inventory]

    assert emitted == sorted(paths)
    assert emitted == [
        "python/DLLs/_ssl.pyd",
        "python/Lib/site.py",
        "python/include/Python.h",
        "python/resource-manifest.json",
        "web/dist/resource-manifest.json",
    ]
    assert "resource-manifest.json" not in emitted
    for relative in (
        "python/resource-manifest.json",
        "web/dist/resource-manifest.json",
    ):
        contents = relative.encode()
        assert [entry for entry in inventory if entry["path"] == relative] == [
            {
                "path": relative,
                "sha256": hashlib.sha256(contents).hexdigest(),
                "size": len(contents),
            }
        ]
