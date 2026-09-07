"""Stage deterministic Tauri resources and emit a bundle config overlay."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import stat
import sys
from pathlib import Path
from typing import TYPE_CHECKING, cast

if TYPE_CHECKING:
    from typing import Never

_REPOSITORY_ROOT = str(Path(__file__).resolve().parents[2])
if _REPOSITORY_ROOT not in sys.path:
    sys.path.insert(0, _REPOSITORY_ROOT)

# Keep direct script execution from a Nix source root on the package import path.
from scripts.release.build_runtime import (  # noqa: E402
    normalize_pe as _build_normalize_pe,
)

BYTECODE_SUFFIXES = {".pyc", ".pyo"}
IGNORED_NAMES = {".pytest_cache", ".ruff_cache"}


def invalid_value(message: str) -> Never:
    raise ValueError(message)


def _is_real_directory(path: Path) -> bool:
    try:
        return (
            not path.is_symlink()
            and not path.is_junction()
            and stat.S_ISDIR(path.stat(follow_symlinks=False).st_mode)
        )
    except OSError:
        return False


def reject_python_bytecode(root: Path) -> None:
    """Reject staged Python bytecode without traversing redirected paths."""
    if not _is_real_directory(root):
        invalid_value(
            f"Python distribution is redirected or not one real directory: {root}"
        )

    pending_directories: list[Path] = [root]
    while pending_directories:
        directory = pending_directories.pop()
        if not _is_real_directory(directory):
            invalid_value(f"Python distribution directory is redirected: {directory}")
        try:
            entries = tuple(directory.iterdir())
        except OSError as error:
            message = f"Python distribution directory cannot be inspected: {directory}"
            raise ValueError(message) from error
        for entry in entries:
            try:
                redirected = entry.is_symlink() or entry.is_junction()
                entry_mode = entry.stat(follow_symlinks=False).st_mode
            except OSError as error:
                message = f"Python distribution entry cannot be inspected: {entry}"
                raise ValueError(message) from error
            if (
                entry.name == "__pycache__"
                or entry.suffix.casefold() in BYTECODE_SUFFIXES
            ):
                invalid_value(
                    f"Python distribution contains bytecode artifact: {entry}"
                )
            if not redirected and stat.S_ISDIR(entry_mode):
                pending_directories.append(entry)


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
        or Path(_directory, name).is_symlink()
        or Path(_directory, name).is_junction()
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


def _normalize_staged_pe(path: Path) -> None:
    """Normalize a staged Windows PE in place; non-PE is a no-op."""
    # Fail-closed on malformed PE; non-PE/ELF must be no-op for Linux.
    _build_normalize_pe(path)
    # Preserve deterministic executable metadata after normalization.
    path.chmod(0o755)
    os.utime(path, (0, 0))


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
    emitted_files: list[tuple[str, Path]] = []
    for candidate in root.rglob("*"):
        if not candidate.is_file():
            continue
        relative = candidate.relative_to(root).as_posix()
        if relative == "resource-manifest.json":
            continue
        emitted_files.append((relative, candidate))
    emitted_files.sort(key=lambda item: item[0])
    return [
        {
            "path": relative,
            "sha256": sha256_file(path),
            "size": path.stat().st_size,
        }
        for relative, path in emitted_files
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
    reject_python_bytecode(python)
    windows = worker.suffix.casefold() == ".exe"
    selected_python = python_executable(python, windows)
    if output.exists():
        invalid_value(f"resource output already exists: {output}")

    output.mkdir(parents=True)
    copy_tree(web, output / "web/dist")
    worker_name = "pokecon-worker.exe" if windows else "pokecon-worker"
    uv_name = "uv.exe" if windows else "uv"
    copy_executable(worker, output / worker_name)
    _normalize_staged_pe(output / worker_name)
    copy_executable(uv, output / "uv" / uv_name)
    copy_tree(wheelhouse, output / "python-wheels")
    copy_tree(python, output / "python")
    canonical_python = (
        output / "python/python.exe" if windows else output / "python/bin/python3.14"
    )
    copy_executable(selected_python, canonical_python)
    if windows:
        for pattern in ("python*.dll", "vcruntime*.dll"):
            for library in sorted(python.glob(pattern)):
                if library.is_file():
                    normalized_copy(str(library), str(output / library.name))
    reject_python_bytecode(output / "python")

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
