"""Stage deterministic Tauri resources and emit a bundle config overlay."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import stat
from pathlib import Path
from typing import TYPE_CHECKING, cast

if TYPE_CHECKING:
    from typing import Never


IGNORED_NAMES = {"__pycache__", ".pytest_cache", ".ruff_cache"}
IGNORED_SUFFIXES = {".pyc", ".pyo"}


def invalid_value(message: str) -> Never:
    raise ValueError(message)


def normalized_copy(source: str, destination: str) -> str:
    source_path = Path(source)
    destination_path = Path(destination)
    shutil.copyfile(source_path, destination_path, follow_symlinks=True)
    executable = bool(source_path.stat().st_mode & stat.S_IXUSR)
    destination_path.chmod(0o755 if executable else 0o644)
    os.utime(destination_path, (0, 0))
    return str(destination_path)


def ignore_entries(_directory: str, names: list[str]) -> set[str]:
    return {
        name
        for name in names
        if name in IGNORED_NAMES
        or Path(name).suffix.casefold() in IGNORED_SUFFIXES
        or Path(_directory, name).is_symlink()
    }


def copy_tree(source: Path, destination: Path) -> None:
    shutil.copytree(
        source,
        destination,
        symlinks=False,
        copy_function=normalized_copy,
        ignore=ignore_entries,
    )
    for directory, directories, _files in os.walk(destination, topdown=False):
        for name in directories:
            path = Path(directory, name)
            path.chmod(0o755)
            os.utime(path, (0, 0))
    destination.chmod(0o755)
    os.utime(destination, (0, 0))


def copy_executable(source: Path, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination, follow_symlinks=True)
    destination.chmod(0o755)
    os.utime(destination, (0, 0))


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while block := source.read(64 * 1024):
            digest.update(block)
    return digest.hexdigest()


def resource_inventory(root: Path) -> list[dict[str, object]]:
    return [
        {
            "path": path.relative_to(root).as_posix(),
            "sha256": sha256_file(path),
            "size": path.stat().st_size,
        }
        for path in sorted(
            candidate for candidate in root.rglob("*") if candidate.is_file()
        )
        if path.name != "resource-manifest.json"
    ]


def require_file(path: Path, label: str) -> None:
    if not path.is_file():
        invalid_value(f"{label} is not a regular file: {path}")


def require_directory(path: Path, label: str) -> None:
    if not path.is_dir():
        invalid_value(f"{label} is not a directory: {path}")


def python_executable(root: Path, windows: bool) -> Path:
    candidates = (
        [root / "python.exe", root / "bin/python.exe"]
        if windows
        else [root / "bin/python3.14", root / "bin/python3", root / "bin/python"]
    )
    executable = next((path for path in candidates if path.is_file()), None)
    if executable is None:
        invalid_value(f"Python resource has no supported executable below {root}")
    return executable


def bundle_resource_map(output: Path, *, windows: bool) -> dict[str, str]:
    """Map staged resources to the runtime resource root."""
    resolved = output.resolve()
    if not windows:
        return {f"{resolved}{os.sep}": ""}

    resources: dict[str, str] = {}
    for path in sorted(resolved.rglob("*")):
        if path.is_dir():
            continue
        if not path.is_file():
            invalid_value(f"unsupported staged resource entry: {path}")
        resources[str(path)] = path.relative_to(resolved).as_posix()
    return resources


def stage_resources(
    web: Path,
    worker: Path,
    uv: Path,
    wheelhouse: Path,
    python: Path,
    output: Path,
    config_output: Path,
) -> dict[str, object]:
    require_directory(web, "Web distribution")
    require_file(web / "index.html", "Web entrypoint")
    require_file(worker, "managed worker")
    require_file(uv, "managed uv")
    require_directory(wheelhouse, "offline Python wheelhouse")
    require_file(wheelhouse / "wheelhouse-manifest.json", "offline wheelhouse manifest")
    require_file(wheelhouse / "requirements.lock", "offline requirements lock")
    require_directory(python, "Python distribution")
    windows = worker.suffix.casefold() == ".exe"
    python_executable(python, windows)
    if output.exists():
        invalid_value(f"resource output already exists: {output}")

    output.mkdir(parents=True)
    copy_tree(web, output / "web/dist")
    worker_name = "pokecon-worker.exe" if windows else "pokecon-worker"
    uv_name = "uv.exe" if windows else "uv"
    copy_executable(worker, output / worker_name)
    copy_executable(uv, output / "uv" / uv_name)
    copy_tree(wheelhouse, output / "python-wheels")
    copy_tree(python, output / "python")
    if windows:
        for pattern in ("python*.dll", "vcruntime*.dll"):
            for library in sorted(python.glob(pattern)):
                if library.is_file():
                    normalized_copy(str(library), str(output / library.name))

    files = resource_inventory(output)
    manifest: dict[str, object] = {
        "schema_version": 1,
        "layout_version": 1,
        "platform": "windows" if windows else "unix",
        "files": files,
    }
    canonical = json.dumps(
        files, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode()
    manifest["content_sha256"] = hashlib.sha256(canonical).hexdigest()
    manifest_path = output / "resource-manifest.json"
    manifest_path.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    manifest_path.chmod(0o644)
    os.utime(manifest_path, (0, 0))

    config_output.parent.mkdir(parents=True, exist_ok=True)
    config = {"bundle": {"resources": bundle_resource_map(output, windows=windows)}}
    config_output.write_text(
        json.dumps(config, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--web", type=Path, required=True)
    parser.add_argument("--worker", type=Path, required=True)
    parser.add_argument("--uv", type=Path, required=True)
    parser.add_argument("--wheelhouse", type=Path, required=True)
    parser.add_argument("--python", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--config-output", type=Path, required=True)
    arguments = parser.parse_args()
    manifest = stage_resources(
        arguments.web,
        arguments.worker,
        arguments.uv,
        arguments.wheelhouse,
        arguments.python,
        arguments.output,
        arguments.config_output,
    )
    files = manifest["files"]
    file_count = len(cast("list[object]", files)) if isinstance(files, list) else 0
    print(
        json.dumps(
            {
                "content_sha256": manifest["content_sha256"],
                "file_count": file_count,
                "platform": manifest["platform"],
            },
            ensure_ascii=False,
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
