from __future__ import annotations

import json
import os
from typing import TYPE_CHECKING

from scripts.stage_release import bundle_resource_map, stage_resources

if TYPE_CHECKING:
    from pathlib import Path


def executable(path: Path, content: bytes = b"fixture") -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content)
    path.chmod(0o755)


def stage_fixture(root: Path, name: str) -> tuple[dict[str, object], Path, Path]:
    web = root / "inputs/web"
    web.mkdir(parents=True, exist_ok=True)
    (web / "index.html").write_text("<main>PokeCon</main>", encoding="utf-8")
    executable(root / "inputs/pokecon-worker")
    executable(root / "inputs/uv")
    wheelhouse = root / "inputs/python-wheels"
    wheelhouse.mkdir(exist_ok=True)
    (wheelhouse / "fixture-1.0-py3-none-any.whl").write_bytes(b"wheel")
    (wheelhouse / "requirements.lock").write_text("fixture==1.0\n", encoding="utf-8")
    (wheelhouse / "wheelhouse-manifest.json").write_text(
        '{"schema_version":1}\n', encoding="utf-8"
    )
    python = root / "inputs/python"
    executable(python / "bin/python3.14")
    (python / "module.py").write_text("VALUE = 1\n", encoding="utf-8")
    python_alias = python / "python"
    if not python_alias.is_symlink():
        python_alias.symlink_to("bin/python3.14")
    cache = python / "__pycache__"
    cache.mkdir(exist_ok=True)
    (cache / "module.pyc").write_bytes(b"unstable")
    output = root / name
    config = root / f"{name}.json"
    manifest = stage_resources(
        web,
        root / "inputs/pokecon-worker",
        root / "inputs/uv",
        wheelhouse,
        python,
        output,
        config,
    )
    return manifest, output, config


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
    assert not (first_output / "python/python").exists()
    assert not (first_output / "python/__pycache__").exists()
    assert json.loads(first_config.read_text(encoding="utf-8"))["bundle"][
        "resources"
    ] == {f"{first_output.resolve()}/": ""}
    assert (first_output / "resource-manifest.json").stat().st_mtime == 0
    assert (second_output / "resource-manifest.json").stat().st_mtime == 0


def test_windows_bundle_map_makes_root_destinations_explicit(tmp_path: Path) -> None:
    output = tmp_path / "bundle-resources"
    (output / "python").mkdir(parents=True)
    (output / "python/python.exe").write_bytes(b"python")
    (output / "pokecon-worker.exe").write_bytes(b"worker")
    (output / "resource-manifest.json").write_text("{}\n", encoding="utf-8")

    assert bundle_resource_map(output, windows=True) == {
        f"{(output / 'python').resolve()}{os.sep}": "python",
        str((output / "pokecon-worker.exe").resolve()): "pokecon-worker.exe",
        str((output / "resource-manifest.json").resolve()): ("resource-manifest.json"),
    }
