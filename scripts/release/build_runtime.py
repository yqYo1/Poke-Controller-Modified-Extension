"""Build the pinned portable CPython runtime and its offline wheelhouse."""

from __future__ import annotations

import argparse
import base64
import csv
import hashlib
import json
import os
import shlex
import shutil
import stat
import subprocess
import sys
import tempfile
import tomllib
import zipfile
from pathlib import Path
from typing import TYPE_CHECKING, cast

if TYPE_CHECKING:
    from collections.abc import Mapping, Sequence


PYTHON_VERSION = "3.14.3"
LINUX_PYTHON_INSTALL_REQUEST = "cpython-3.14.3-linux-x86_64-gnu"
WINDOWS_PYTHON_INSTALL_REQUEST = "cpython-3.14.3-windows-x86_64-none"
LINUX_PYTHON_MINOR_REDIRECT = "cpython-3.14-linux-x86_64-gnu"
WINDOWS_PYTHON_MINOR_REDIRECT = "cpython-3.14-windows-x86_64-none"
SETUPTOOLS_VERSION = "82.0.1"
WHEEL_VERSION = "0.46.3"
PORTABLE_BUILD_PREFIX = "/install"
PORTABLE_SYSTEM_INTERPRETER = "/lib64/ld-linux-x86-64.so.2"
PORTABLE_PYTHON_RPATH = "$ORIGIN/../lib"
REPRODUCIBLE_ZIP_EPOCH = 315_532_800
PE_REPRODUCIBLE_TIMESTAMP = REPRODUCIBLE_ZIP_EPOCH
WORKER_RUNTIME_SMOKE = """\
import cv2, numpy, pandas, PIL, pyaudio, scipy

audio = pyaudio.PyAudio()
stream = None
try:
    inputs = [
        (index, audio.get_device_info_by_index(index))
        for index in range(audio.get_device_count())
        if audio.get_device_info_by_index(index).get("maxInputChannels", 0) > 0
    ]
    if inputs:
        index, _device = inputs[0]
        stream = audio.open(
            format=pyaudio.paInt16,
            channels=1,
            rate=44_100,
            input=True,
            input_device_index=index,
            frames_per_buffer=64,
        )
finally:
    if stream is not None:
        stream.close()
    audio.terminate()
"""


def run(
    arguments: Sequence[str | Path],
    *,
    environment: Mapping[str, str] | None = None,
    capture: bool = False,
) -> str:
    command = [str(argument) for argument in arguments]
    child_environment = dict(os.environ if environment is None else environment)
    child_environment["PYTHONDONTWRITEBYTECODE"] = "1"
    completed = subprocess.run(
        command,
        check=True,
        env=child_environment,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None,
        text=True,
    )
    return completed.stdout.strip() if capture else ""


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while block := source.read(64 * 1024):
            digest.update(block)
    return digest.hexdigest()


def object_mapping(value: object, label: str) -> dict[str, object]:
    if not isinstance(value, dict):
        message = f"{label} must be an object"
        raise ValueError(message)
    untyped = cast("dict[object, object]", value)
    if not all(isinstance(key, str) for key in untyped):
        message = f"{label} keys must be strings"
        raise ValueError(message)
    return {cast("str", key): item for key, item in untyped.items()}


def python_install_request(platform_name: str) -> str:
    if platform_name == "linux":
        return LINUX_PYTHON_INSTALL_REQUEST
    if platform_name == "win32":
        return WINDOWS_PYTHON_INSTALL_REQUEST
    message = f"portable Python install request does not support {platform_name!r}"
    raise ValueError(message)


def python_minor_redirect_name(platform_name: str) -> str:
    if platform_name == "linux":
        return LINUX_PYTHON_MINOR_REDIRECT
    if platform_name == "win32":
        return WINDOWS_PYTHON_MINOR_REDIRECT
    message = f"portable Python minor redirect does not support {platform_name!r}"
    raise ValueError(message)


def python_executable(root: Path, *, platform_name: str | None = None) -> Path:
    platform_name = os.name if platform_name is None else platform_name
    candidates = (
        [root / "python.exe", root / "Scripts/python.exe", root / "bin/python.exe"]
        if platform_name == "nt"
        else [root / "bin/python3.14", root / "bin/python3", root / "bin/python"]
    )
    executable = next((path for path in candidates if path.is_file()), None)
    if executable is None:
        message = f"portable Python has no supported executable below {root}"
        raise ValueError(message)
    return executable


def normalize_python_bytecode(root: Path) -> None:
    """Remove upstream bytecode caches without traversing redirected paths."""
    if not _is_real_directory(root):
        message = f"portable Python runtime root is redirected: {root}"
        raise ValueError(message)

    bytecode_files: list[Path] = []
    cache_directories: list[Path] = []
    pending_directories: list[Path] = [root]
    while pending_directories:
        directory = pending_directories.pop()
        if not _is_real_directory(directory):
            message = (
                f"portable Python runtime directory became redirected: {directory}"
            )
            raise ValueError(message)
        try:
            entries = tuple(directory.iterdir())
        except OSError as error:
            message = (
                f"portable Python runtime directory cannot be inspected: {directory}"
            )
            raise ValueError(message) from error
        for entry in entries:
            try:
                redirected = entry.is_symlink() or entry.is_junction()
                entry_mode = entry.stat(follow_symlinks=False).st_mode
            except OSError as error:
                message = f"portable Python runtime entry cannot be inspected: {entry}"
                raise ValueError(message) from error
            if entry.suffix == ".pyc":
                if redirected or not stat.S_ISREG(entry_mode):
                    message = (
                        "portable Python bytecode artifact is not one real file: "
                        f"{entry}"
                    )
                    raise ValueError(message)
                if directory.name != "__pycache__":
                    message = (
                        "portable Python bytecode artifact is outside a real "
                        f"__pycache__ directory: {entry}"
                    )
                    raise ValueError(message)
                bytecode_files.append(entry)
                continue
            if entry.name == "__pycache__":
                if redirected or not stat.S_ISDIR(entry_mode):
                    message = (
                        "portable Python bytecode cache is redirected or not one "
                        f"real directory: {entry}"
                    )
                    raise ValueError(message)
                cache_directories.append(entry)
                pending_directories.append(entry)
                continue
            if directory.name == "__pycache__":
                message = (
                    "portable Python bytecode cache contains non-bytecode residue: "
                    f"{entry}"
                )
                raise ValueError(message)
            if not redirected and stat.S_ISDIR(entry_mode):
                pending_directories.append(entry)

    for bytecode_file in bytecode_files:
        cache_directory = bytecode_file.parent
        if not _is_real_directory(cache_directory):
            message = (
                "portable Python bytecode cache became redirected before "
                f"normalization: {cache_directory}"
            )
            raise ValueError(message)
        if not _is_real_regular_file(bytecode_file):
            message = (
                "portable Python bytecode artifact became redirected or non-regular: "
                f"{bytecode_file}"
            )
            raise ValueError(message)
        try:
            bytecode_file.unlink()
        except OSError as error:
            message = (
                f"portable Python bytecode artifact cannot be removed: {bytecode_file}"
            )
            raise ValueError(message) from error

    for cache_directory in sorted(
        cache_directories,
        key=lambda path: len(path.parts),
        reverse=True,
    ):
        if not _is_real_directory(cache_directory):
            message = (
                f"portable Python bytecode cache became redirected: {cache_directory}"
            )
            raise ValueError(message)
        try:
            residue = tuple(cache_directory.iterdir())
        except OSError as error:
            message = (
                f"portable Python bytecode cache cannot be inspected: {cache_directory}"
            )
            raise ValueError(message) from error
        if residue:
            message = (
                "portable Python bytecode cache became nonempty during normalization: "
                f"{cache_directory}"
            )
            raise ValueError(message)
        try:
            cache_directory.rmdir()
        except OSError as error:
            message = (
                f"portable Python bytecode cache cannot be removed: {cache_directory}"
            )
            raise ValueError(message) from error


def audit_python_bytecode(root: Path) -> None:
    """Fail if a runtime contains bytecode without following redirects."""
    if not _is_real_directory(root):
        message = f"portable Python runtime root is redirected: {root}"
        raise ValueError(message)

    pending_directories: list[Path] = [root]
    while pending_directories:
        directory = pending_directories.pop()
        if not _is_real_directory(directory):
            message = (
                f"portable Python runtime directory became redirected: {directory}"
            )
            raise ValueError(message)
        try:
            entries = tuple(directory.iterdir())
        except OSError as error:
            message = (
                f"portable Python runtime directory cannot be inspected: {directory}"
            )
            raise ValueError(message) from error
        for entry in entries:
            try:
                redirected = entry.is_symlink() or entry.is_junction()
                entry_mode = entry.stat(follow_symlinks=False).st_mode
            except OSError as error:
                message = f"portable Python runtime entry cannot be inspected: {entry}"
                raise ValueError(message) from error
            if entry.suffix in {".pyc", ".pyo"} or entry.name == "__pycache__":
                message = (
                    "portable Python runtime contains a bytecode artifact after "
                    f"normalization: {entry}"
                )
                raise ValueError(message)
            if not redirected and stat.S_ISDIR(entry_mode):
                pending_directories.append(entry)


def normalize_python_sysconfig(root: Path, installed_prefix: Path) -> None:
    """Remove uv's temporary install prefix from POSIX sysconfig data."""
    sysconfig_files = sorted(root.rglob("_sysconfigdata__*.py"))
    if not sysconfig_files:
        if os.name == "nt":
            return
        message = f"portable Python has no sysconfig data below {root}"
        raise ValueError(message)

    source_prefix = str(installed_prefix)
    replacements = 0
    for path in sysconfig_files:
        content = path.read_text(encoding="utf-8")
        occurrences = content.count(source_prefix)
        if occurrences == 0:
            continue
        path.write_text(
            content.replace(source_prefix, PORTABLE_BUILD_PREFIX),
            encoding="utf-8",
        )
        replacements += occurrences
    if replacements == 0:
        message = "portable Python sysconfig data does not contain its install prefix"
        raise ValueError(message)


def verify_python(executable: Path, root: Path) -> None:
    probe = run(
        [
            executable,
            "-c",
            (
                "import json, platform, sys, sysconfig; "
                "print(json.dumps({"
                "'version': platform.python_version(), "
                "'prefix': sys.prefix, "
                "'config_prefix': sysconfig.get_config_var('prefix'), "
                "'include': sysconfig.get_path('include')"
                "}))"
            ),
        ],
        capture=True,
    )
    parsed = object_mapping(json.loads(probe), "portable Python probe")
    version = parsed.get("version")
    if version != PYTHON_VERSION:
        message = f"portable Python version {version!r} differs from {PYTHON_VERSION}"
        raise ValueError(message)
    expected_prefix = root.resolve()
    prefix = Path(str(parsed.get("prefix", ""))).resolve()
    config_prefix = Path(str(parsed.get("config_prefix", ""))).resolve()
    include = Path(str(parsed.get("include", ""))).resolve()
    if prefix != expected_prefix or config_prefix != expected_prefix:
        message = "portable Python did not relocate its runtime prefix"
        raise ValueError(message)
    if expected_prefix not in include.parents:
        message = "portable Python include path is outside its runtime prefix"
        raise ValueError(message)


def _is_real_regular_file(path: Path) -> bool:
    try:
        return (
            not path.is_symlink()
            and not path.is_junction()
            and stat.S_ISREG(path.stat(follow_symlinks=False).st_mode)
        )
    except OSError:
        return False


def _is_real_directory(path: Path) -> bool:
    try:
        return (
            not path.is_symlink()
            and not path.is_junction()
            and stat.S_ISDIR(path.stat(follow_symlinks=False).st_mode)
        )
    except OSError:
        return False


def managed_python_install_prefix(
    install_root: Path,
    install_request: str,
    *,
    platform_name: str,
) -> Path:
    expected_install_request = python_install_request(platform_name)
    minor_redirect_name = python_minor_redirect_name(platform_name)
    if install_request != expected_install_request:
        message = (
            f"managed Python install request {install_request!r} differs from "
            f"{expected_install_request!r}"
        )
        raise ValueError(message)
    if not _is_real_directory(install_root):
        message = f"uv managed Python installation root is redirected: {install_root}"
        raise ValueError(message)
    resolved_install_root = install_root.resolve(strict=True)
    entries = {entry.name: entry for entry in install_root.iterdir()}
    expected_names = {
        ".gitignore",
        ".lock",
        ".temp",
        install_request,
        minor_redirect_name,
    }
    actual_names = set(entries)
    if actual_names != expected_names:
        missing = sorted(expected_names - actual_names)
        extras = sorted(actual_names - expected_names)
        message = (
            "uv managed Python installation root inventory differs from the "
            f"pinned layout; missing={missing!r}; extras={extras!r}"
        )
        raise ValueError(message)

    gitignore = entries[".gitignore"]
    if not _is_real_regular_file(gitignore) or gitignore.read_bytes() != b"*":
        message = "uv managed Python .gitignore must be one real file containing b'*'"
        raise ValueError(message)
    lock = entries[".lock"]
    if not _is_real_regular_file(lock) or lock.read_bytes() != b"":
        message = "uv managed Python .lock must be one real empty file"
        raise ValueError(message)
    temporary = entries[".temp"]
    if not _is_real_directory(temporary) or tuple(temporary.iterdir()):
        message = "uv managed Python .temp must be one real empty directory"
        raise ValueError(message)

    full_install = entries[install_request]
    if not _is_real_directory(full_install):
        message = f"uv managed Python full installation is redirected: {full_install}"
        raise ValueError(message)
    installed_prefix = full_install.resolve(strict=True)
    if installed_prefix.parent != resolved_install_root:
        message = "uv installed managed CPython outside the isolated installation root"
        raise ValueError(message)

    minor_redirect = entries[minor_redirect_name]
    if platform_name == "linux":
        redirect_kind_is_valid = (
            minor_redirect.is_symlink() and not minor_redirect.is_junction()
        )
    elif platform_name == "win32":
        redirect_kind_is_valid = (
            minor_redirect.is_junction() and not minor_redirect.is_symlink()
        )
    else:
        message = f"portable Python layout does not support {platform_name!r}"
        raise ValueError(message)
    if not redirect_kind_is_valid:
        message = (
            "uv managed Python minor redirect has the wrong platform-specific kind: "
            f"{minor_redirect}"
        )
        raise ValueError(message)
    if minor_redirect.resolve(strict=True) != installed_prefix:
        message = (
            "uv managed Python minor redirect does not target the full installation"
        )
        raise ValueError(message)
    return installed_prefix


def install_python(uv: Path, output: Path, workspace: Path) -> Path:
    install_root = workspace / "python-installs"
    install_request = python_install_request(sys.platform)
    run(
        [
            uv,
            "--no-config",
            "python",
            "install",
            install_request,
            "--managed-python",
            "--no-bin",
            "--install-dir",
            install_root,
        ]
    )
    installed_prefix = managed_python_install_prefix(
        install_root,
        install_request,
        platform_name=sys.platform,
    )
    installed_python = python_executable(installed_prefix)
    resolved_installed_python = installed_python.resolve(strict=True)
    if installed_prefix not in resolved_installed_python.parents:
        message = "uv installed a managed CPython executable outside its runtime root"
        raise ValueError(message)
    shutil.copytree(installed_prefix, output, symlinks=True)
    normalize_python_bytecode(output)
    normalize_python_sysconfig(output, installed_prefix)
    return python_executable(output)


def portable_python_elf_metadata(executable: Path, patchelf: Path) -> tuple[str, str]:
    return (
        run([patchelf, "--print-interpreter", executable], capture=True),
        run([patchelf, "--print-rpath", executable], capture=True),
    )


def prepare_python_execution_copy(
    runtime_root: Path,
    workspace: Path,
    patchelf: Path,
    execution_loader: Path,
    execution_library_path: str,
) -> tuple[Path, Path, str]:
    """Create a temporary Nix-loadable copy without changing portable output bytes."""
    raw_python = python_executable(runtime_root)
    if raw_python.is_symlink() or not raw_python.is_file():
        message = f"portable Python executable must be one real file: {raw_python}"
        raise ValueError(message)
    raw_metadata = portable_python_elf_metadata(raw_python, patchelf)
    expected_raw_metadata = (PORTABLE_SYSTEM_INTERPRETER, PORTABLE_PYTHON_RPATH)
    if raw_metadata != expected_raw_metadata:
        message = (
            "portable Python ELF boundary changed: "
            f"interpreter={raw_metadata[0]!r}, rpath={raw_metadata[1]!r}"
        )
        raise ValueError(message)
    raw_digest = sha256_file(raw_python)
    if not execution_loader.is_absolute() or not execution_loader.is_file():
        message = f"portable Python execution loader is invalid: {execution_loader}"
        raise ValueError(message)
    library_entries = execution_library_path.split(os.pathsep)
    if not library_entries or any(
        not entry or not Path(entry).is_absolute() or not Path(entry).is_dir()
        for entry in library_entries
    ):
        message = (
            "portable Python execution library path is not an exact absolute inventory"
        )
        raise ValueError(message)

    execution_root = workspace / "python-execution"
    shutil.copytree(runtime_root, execution_root, symlinks=True)
    execution_python = python_executable(execution_root)
    if execution_python.is_symlink() or not execution_python.is_file():
        message = (
            "portable Python execution copy must expose one real executable: "
            f"{execution_python}"
        )
        raise ValueError(message)
    execution_rpath = os.pathsep.join((PORTABLE_PYTHON_RPATH, execution_library_path))
    run([patchelf, "--set-interpreter", execution_loader, execution_python])
    run([patchelf, "--set-rpath", execution_rpath, execution_python])
    execution_metadata = portable_python_elf_metadata(execution_python, patchelf)
    if execution_metadata != (str(execution_loader), execution_rpath):
        message = (
            "portable Python execution copy did not receive its pinned ELF boundary"
        )
        raise ValueError(message)
    if portable_python_elf_metadata(raw_python, patchelf) != expected_raw_metadata:
        message = "preparing the execution copy changed portable Python ELF metadata"
        raise ValueError(message)
    if sha256_file(raw_python) != raw_digest:
        message = "preparing the execution copy changed portable Python bytes"
        raise ValueError(message)
    verify_python(execution_python, execution_root)
    return execution_python, execution_root, raw_digest


def export_requirements(uv: Path, project: Path, output: Path) -> None:
    run(
        [
            uv,
            "--no-config",
            "--quiet",
            "export",
            "--project",
            project,
            "--frozen",
            "--no-dev",
            "--group",
            "worker-script",
            "--no-emit-project",
            "--no-header",
            "--no-annotate",
            "--format",
            "requirements.txt",
            "--output-file",
            output,
        ]
    )


def direct_requirements(project: Path, output: Path) -> None:
    with (project / "pyproject.toml").open("rb") as source:
        pyproject = object_mapping(tomllib.load(source), "pyproject.toml")
    groups = object_mapping(
        pyproject.get("dependency-groups"), "pyproject.toml dependency-groups"
    )
    requirements_value = groups.get("worker-script")
    if not isinstance(requirements_value, list):
        message = "worker-script must be an array of requirement strings"
        raise ValueError(message)
    requirements_objects = cast("list[object]", requirements_value)
    if not all(isinstance(item, str) for item in requirements_objects):
        message = "worker-script must be an array of requirement strings"
        raise ValueError(message)
    requirements = cast("list[str]", requirements_objects)
    output.write_text("\n".join(requirements) + "\n", encoding="utf-8")


def is_elf(path: Path) -> bool:
    with path.open("rb") as source:
        return source.read(4) == b"\x7fELF"


def _pe_rva_to_file_offset(
    rva: int,
    sections: list[tuple[int, int, int, int]],
) -> int | None:
    for (
        virtual_address,
        virtual_size,
        pointer_to_raw_data,
        size_of_raw_data,
    ) in sections:
        if pointer_to_raw_data == 0:
            continue
        if virtual_size == 0 and size_of_raw_data == 0:
            continue
        extent = virtual_size if virtual_size != 0 else size_of_raw_data
        if virtual_size != 0 and size_of_raw_data != 0:
            extent = max(virtual_size, size_of_raw_data)
        if virtual_address <= rva < virtual_address + extent:
            return (rva - virtual_address) + pointer_to_raw_data
    return None


def _normalize_pe(path: Path) -> bool:
    data = path.read_bytes()
    if len(data) < 2 or data[0:2] != b"MZ":
        return False
    if len(data) < 0x40:
        message = f"PE file is truncated: missing DOS header: {path}"
        raise ValueError(message)
    e_lfanew = int.from_bytes(data[0x3C:0x40], "little")
    if e_lfanew < 0 or e_lfanew + 6 > len(data):
        message = f"PE file is truncated: invalid e_lfanew {e_lfanew}: {path}"
        raise ValueError(message)
    if data[e_lfanew : e_lfanew + 4] != b"PE\x00\x00":
        message = f"PE file has invalid PE signature: {path}"
        raise ValueError(message)
    if e_lfanew + 4 + 20 > len(data):
        message = f"PE file is truncated: missing COFF header: {path}"
        raise ValueError(message)
    coff_timestamp_offset = e_lfanew + 4 + 4
    number_of_sections = int.from_bytes(
        data[e_lfanew + 4 + 2 : e_lfanew + 4 + 4], "little"
    )
    size_of_optional_header = int.from_bytes(
        data[e_lfanew + 4 + 16 : e_lfanew + 4 + 18], "little"
    )
    optional_header_offset = e_lfanew + 4 + 20
    if optional_header_offset + size_of_optional_header > len(data):
        message = f"PE file is truncated: missing optional header: {path}"
        raise ValueError(message)
    mutable = bytearray(data)
    changed = False
    fixed = PE_REPRODUCIBLE_TIMESTAMP.to_bytes(4, "little")
    if mutable[coff_timestamp_offset : coff_timestamp_offset + 4] != fixed:
        mutable[coff_timestamp_offset : coff_timestamp_offset + 4] = fixed
        changed = True
    if size_of_optional_header == 0:
        message = f"PE file is truncated: missing optional header: {path}"
        raise ValueError(message)
    if size_of_optional_header < 2:
        message = f"PE file is truncated: optional header too small: {path}"
        raise ValueError(message)
    magic = int.from_bytes(
        mutable[optional_header_offset : optional_header_offset + 2], "little"
    )
    if magic not in (0x10B, 0x20B):
        message = f"PE file has invalid optional header magic {magic:#x}: {path}"
        raise ValueError(message)
    # Validate section table bounds for every PE with nonzero sections
    section_headers_offset = optional_header_offset + size_of_optional_header
    if (
        number_of_sections != 0
        and section_headers_offset + number_of_sections * 40 > len(data)
    ):
        message = f"PE file is truncated: missing section headers: {path}"
        raise ValueError(message)
    debug_rva_offset: int | None = None
    if magic == 0x10B:
        data_directory_offset = optional_header_offset + 96
        number_of_rva_offset = optional_header_offset + 92
        if size_of_optional_header < 96:
            message = f"PE file is truncated: missing NumberOfRvaAndSizes: {path}"
            raise ValueError(message)
        if number_of_rva_offset + 4 > optional_header_offset + size_of_optional_header:
            message = f"PE file is truncated: missing NumberOfRvaAndSizes: {path}"
            raise ValueError(message)
        number_of_rva = int.from_bytes(
            mutable[number_of_rva_offset : number_of_rva_offset + 4], "little"
        )
        if number_of_rva > 6:
            if size_of_optional_header < 96 + 8 * 7:
                message = f"PE file is truncated: missing debug data directory: {path}"
                raise ValueError(message)
            debug_rva_offset = data_directory_offset + 6 * 8
    elif magic == 0x20B:
        data_directory_offset = optional_header_offset + 112
        number_of_rva_offset = optional_header_offset + 108
        if size_of_optional_header < 112:
            message = f"PE file is truncated: missing NumberOfRvaAndSizes: {path}"
            raise ValueError(message)
        if number_of_rva_offset + 4 > optional_header_offset + size_of_optional_header:
            message = f"PE file is truncated: missing NumberOfRvaAndSizes: {path}"
            raise ValueError(message)
        number_of_rva = int.from_bytes(
            mutable[number_of_rva_offset : number_of_rva_offset + 4], "little"
        )
        if number_of_rva > 6:
            if size_of_optional_header < 112 + 8 * 7:
                message = f"PE file is truncated: missing debug data directory: {path}"
                raise ValueError(message)
            debug_rva_offset = data_directory_offset + 6 * 8
    if debug_rva_offset is not None:
        if debug_rva_offset + 8 > optional_header_offset + size_of_optional_header:
            message = f"PE file is truncated: missing debug data directory: {path}"
            raise ValueError(message)
        debug_rva = int.from_bytes(
            mutable[debug_rva_offset : debug_rva_offset + 4], "little"
        )
        debug_size = int.from_bytes(
            mutable[debug_rva_offset + 4 : debug_rva_offset + 8], "little"
        )
        if (debug_rva == 0) != (debug_size == 0):
            message = f"PE file has inconsistent debug directory: {path}"
            raise ValueError(message)
        if debug_rva != 0 and debug_size != 0:
            if debug_size % 28 != 0:
                message = (
                    f"PE file has invalid debug directory size {debug_size}: {path}"
                )
                raise ValueError(message)
            sections: list[tuple[int, int, int, int]] = []
            for index in range(number_of_sections):
                offset = section_headers_offset + index * 40
                virtual_size = int.from_bytes(
                    mutable[offset + 8 : offset + 12], "little"
                )
                virtual_address = int.from_bytes(
                    mutable[offset + 12 : offset + 16], "little"
                )
                size_of_raw_data = int.from_bytes(
                    mutable[offset + 16 : offset + 20], "little"
                )
                pointer_to_raw_data = int.from_bytes(
                    mutable[offset + 20 : offset + 24], "little"
                )
                sections.append(
                    (
                        virtual_address,
                        virtual_size,
                        pointer_to_raw_data,
                        size_of_raw_data,
                    )
                )
            file_offset = _pe_rva_to_file_offset(debug_rva, sections)
            if file_offset is None:
                message = f"PE debug directory RVA does not map to file offset: {path}"
                raise ValueError(message)
            if file_offset + debug_size > len(mutable):
                message = (
                    f"PE file is truncated: debug directory extends beyond file: {path}"
                )
                raise ValueError(message)
            count = debug_size // 28
            for idx in range(count):
                entry_offset = file_offset + idx * 28
                if entry_offset + 28 > len(mutable):
                    message = (
                        f"PE file is truncated: debug entry extends beyond file: {path}"
                    )
                    raise ValueError(message)
                if mutable[entry_offset + 4 : entry_offset + 8] != fixed:
                    mutable[entry_offset + 4 : entry_offset + 8] = fixed
                    changed = True
    if changed:
        path.write_bytes(mutable)
    return changed


def normalize_pe(path: Path) -> bool:
    """Production-safe PE normalization for Windows release binaries.

    For a valid PE input, fixes COFF TimeDateStamp and removes/zeros the
    IMAGE_DEBUG_DIRECTORY data directory, debug directory records, and their
    raw payload (CodeView/VC_FEATURE/POGO). Fail-closed on malformed PE;
    non-PE/ELF is a no-op for Linux.
    """
    data = path.read_bytes()
    if len(data) < 2 or data[0:2] != b"MZ":
        return False
    if len(data) < 0x40:
        message = f"PE file is truncated: missing DOS header: {path}"
        raise ValueError(message)
    e_lfanew = int.from_bytes(data[0x3C:0x40], "little")
    if e_lfanew < 0 or e_lfanew + 6 > len(data):
        message = f"PE file is truncated: invalid e_lfanew {e_lfanew}: {path}"
        raise ValueError(message)
    if data[e_lfanew : e_lfanew + 4] != b"PE\x00\x00":
        message = f"PE file has invalid PE signature: {path}"
        raise ValueError(message)
    if e_lfanew + 4 + 20 > len(data):
        message = f"PE file is truncated: missing COFF header: {path}"
        raise ValueError(message)
    coff_timestamp_offset = e_lfanew + 4 + 4
    number_of_sections = int.from_bytes(
        data[e_lfanew + 4 + 2 : e_lfanew + 4 + 4], "little"
    )
    size_of_optional_header = int.from_bytes(
        data[e_lfanew + 4 + 16 : e_lfanew + 4 + 18], "little"
    )
    optional_header_offset = e_lfanew + 4 + 20
    if optional_header_offset + size_of_optional_header > len(data):
        message = f"PE file is truncated: missing optional header: {path}"
        raise ValueError(message)
    mutable = bytearray(data)
    changed = False
    fixed = PE_REPRODUCIBLE_TIMESTAMP.to_bytes(4, "little")
    if mutable[coff_timestamp_offset : coff_timestamp_offset + 4] != fixed:
        mutable[coff_timestamp_offset : coff_timestamp_offset + 4] = fixed
        changed = True
    if size_of_optional_header == 0:
        message = f"PE file is truncated: missing optional header: {path}"
        raise ValueError(message)
    if size_of_optional_header < 2:
        message = f"PE file is truncated: optional header too small: {path}"
        raise ValueError(message)
    magic = int.from_bytes(
        mutable[optional_header_offset : optional_header_offset + 2], "little"
    )
    if magic not in (0x10B, 0x20B):
        message = f"PE file has invalid optional header magic {magic:#x}: {path}"
        raise ValueError(message)
    section_headers_offset = optional_header_offset + size_of_optional_header
    if (
        number_of_sections != 0
        and section_headers_offset + number_of_sections * 40 > len(data)
    ):
        message = f"PE file is truncated: missing section headers: {path}"
        raise ValueError(message)
    debug_rva_offset: int | None = None
    if magic == 0x10B:
        data_directory_offset = optional_header_offset + 96
        number_of_rva_offset = optional_header_offset + 92
        if size_of_optional_header < 96:
            message = f"PE file is truncated: missing NumberOfRvaAndSizes: {path}"
            raise ValueError(message)
        if number_of_rva_offset + 4 > optional_header_offset + size_of_optional_header:
            message = f"PE file is truncated: missing NumberOfRvaAndSizes: {path}"
            raise ValueError(message)
        number_of_rva = int.from_bytes(
            mutable[number_of_rva_offset : number_of_rva_offset + 4], "little"
        )
        if number_of_rva > 6:
            if size_of_optional_header < 96 + 8 * 7:
                message = f"PE file is truncated: missing debug data directory: {path}"
                raise ValueError(message)
            debug_rva_offset = data_directory_offset + 6 * 8
    elif magic == 0x20B:
        data_directory_offset = optional_header_offset + 112
        number_of_rva_offset = optional_header_offset + 108
        if size_of_optional_header < 112:
            message = f"PE file is truncated: missing NumberOfRvaAndSizes: {path}"
            raise ValueError(message)
        if number_of_rva_offset + 4 > optional_header_offset + size_of_optional_header:
            message = f"PE file is truncated: missing NumberOfRvaAndSizes: {path}"
            raise ValueError(message)
        number_of_rva = int.from_bytes(
            mutable[number_of_rva_offset : number_of_rva_offset + 4], "little"
        )
        if number_of_rva > 6:
            if size_of_optional_header < 112 + 8 * 7:
                message = f"PE file is truncated: missing debug data directory: {path}"
                raise ValueError(message)
            debug_rva_offset = data_directory_offset + 6 * 8
    if debug_rva_offset is not None:
        if debug_rva_offset + 8 > optional_header_offset + size_of_optional_header:
            message = f"PE file is truncated: missing debug data directory: {path}"
            raise ValueError(message)
        debug_rva = int.from_bytes(
            mutable[debug_rva_offset : debug_rva_offset + 4], "little"
        )
        debug_size = int.from_bytes(
            mutable[debug_rva_offset + 4 : debug_rva_offset + 8], "little"
        )
        if (debug_rva == 0) != (debug_size == 0):
            message = f"PE file has inconsistent debug directory: {path}"
            raise ValueError(message)
        if debug_rva != 0 and debug_size != 0:
            if debug_size % 28 != 0:
                message = (
                    f"PE file has invalid debug directory size {debug_size}: {path}"
                )
                raise ValueError(message)
            sections: list[tuple[int, int, int, int]] = []
            for index in range(number_of_sections):
                offset = section_headers_offset + index * 40
                virtual_size = int.from_bytes(
                    mutable[offset + 8 : offset + 12], "little"
                )
                virtual_address = int.from_bytes(
                    mutable[offset + 12 : offset + 16], "little"
                )
                size_of_raw_data = int.from_bytes(
                    mutable[offset + 16 : offset + 20], "little"
                )
                pointer_to_raw_data = int.from_bytes(
                    mutable[offset + 20 : offset + 24], "little"
                )
                sections.append(
                    (
                        virtual_address,
                        virtual_size,
                        pointer_to_raw_data,
                        size_of_raw_data,
                    )
                )
            file_offset = _pe_rva_to_file_offset(debug_rva, sections)
            if file_offset is None:
                message = f"PE debug directory RVA does not map to file offset: {path}"
                raise ValueError(message)
            if file_offset + debug_size > len(mutable):
                message = (
                    f"PE file is truncated: debug directory extends beyond file: {path}"
                )
                raise ValueError(message)
            count = debug_size // 28
            payload_ranges: list[tuple[int, int]] = []
            for idx in range(count):
                entry_offset = file_offset + idx * 28
                if entry_offset + 28 > len(mutable):
                    message = (
                        f"PE file is truncated: debug entry extends beyond file: {path}"
                    )
                    raise ValueError(message)
                size_of_data = int.from_bytes(
                    mutable[entry_offset + 16 : entry_offset + 20], "little"
                )
                pointer_to_raw_data = int.from_bytes(
                    mutable[entry_offset + 24 : entry_offset + 28], "little"
                )
                if size_of_data != 0:
                    if pointer_to_raw_data == 0:
                        message = f"PE debug entry has invalid payload pointer: {path}"
                        raise ValueError(message)
                    if pointer_to_raw_data + size_of_data > len(mutable):
                        message = f"PE debug payload extends beyond file: {path}"
                        raise ValueError(message)
                    payload_ranges.append((pointer_to_raw_data, size_of_data))
            for ptr, sz in payload_ranges:
                if mutable[ptr : ptr + sz] != b"\x00" * sz:
                    mutable[ptr : ptr + sz] = b"\x00" * sz
                    changed = True
            for idx in range(count):
                entry_offset = file_offset + idx * 28
                if mutable[entry_offset : entry_offset + 28] != b"\x00" * 28:
                    mutable[entry_offset : entry_offset + 28] = b"\x00" * 28
                    changed = True
            if mutable[debug_rva_offset : debug_rva_offset + 8] != b"\x00" * 8:
                mutable[debug_rva_offset : debug_rva_offset + 8] = b"\x00" * 8
                changed = True
    if changed:
        path.write_bytes(mutable)
    return changed


# Backwards-compatible aliases for the production PE helper.
normalize_pe_executable = normalize_pe
normalize_pe_file = normalize_pe


def safe_rpath(path: Path, patchelf: Path) -> None:
    rpath = run([patchelf, "--print-rpath", path], capture=True)
    entries = [
        entry
        for entry in rpath.split(":")
        if entry and entry.startswith(("$ORIGIN", "${ORIGIN}"))
    ]
    if entries == [entry for entry in rpath.split(":") if entry]:
        return
    if entries:
        run([patchelf, "--set-rpath", ":".join(entries), path])
    else:
        run([patchelf, "--remove-rpath", path])


def wheel_record(root: Path) -> None:
    records = list(root.glob("*.dist-info/RECORD"))
    if len(records) != 1:
        message = f"wheel must contain exactly one RECORD file: {root}"
        raise ValueError(message)
    record = records[0]
    rows: list[tuple[str, str, str]] = []
    for path in sorted(
        candidate for candidate in root.rglob("*") if candidate.is_file()
    ):
        relative = path.relative_to(root).as_posix()
        if path == record:
            rows.append((relative, "", ""))
            continue
        digest = hashlib.sha256(path.read_bytes()).digest()
        encoded = base64.urlsafe_b64encode(digest).rstrip(b"=").decode("ascii")
        rows.append((relative, f"sha256={encoded}", str(path.stat().st_size)))
    with record.open("w", encoding="utf-8", newline="") as output:
        csv.writer(output, lineterminator="\n").writerows(rows)


def repack_wheel(root: Path, output: Path) -> None:
    temporary = output.with_suffix(".normalized.whl")
    with zipfile.ZipFile(
        temporary,
        "w",
        compression=zipfile.ZIP_DEFLATED,
        compresslevel=9,
    ) as archive:
        for path in sorted(
            candidate for candidate in root.rglob("*") if candidate.is_file()
        ):
            relative = path.relative_to(root).as_posix()
            info = zipfile.ZipInfo(relative, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            mode = stat.S_IMODE(path.stat().st_mode)
            info.external_attr = mode << 16
            archive.writestr(info, path.read_bytes(), compresslevel=9)
    temporary.replace(output)


def normalize_wheel(
    wheel: Path,
    patchelf: Path | None,
    strip: Path | None,
) -> None:
    with tempfile.TemporaryDirectory(prefix="pokecon-wheel-") as directory:
        root = Path(directory)
        with zipfile.ZipFile(wheel) as archive:
            archive.extractall(root)
        changed = False
        for binary in sorted(path for path in root.rglob("*") if path.is_file()):
            if is_elf(binary):
                if patchelf is None or strip is None:
                    message = (
                        f"native wheel normalization requires ELF tools: {wheel.name}"
                    )
                    raise ValueError(message)
                rpath = run([patchelf, "--print-rpath", binary], capture=True)
                unsafe = any(
                    entry and not entry.startswith(("$ORIGIN", "${ORIGIN}"))
                    for entry in rpath.split(":")
                )
                if not unsafe:
                    continue
                safe_rpath(binary, patchelf)
                run([strip, "--strip-unneeded", binary])
                normalized_rpath = run(
                    [patchelf, "--print-rpath", binary], capture=True
                )
                if any(
                    entry and not entry.startswith(("$ORIGIN", "${ORIGIN}"))
                    for entry in normalized_rpath.split(":")
                ):
                    message = (
                        f"native wheel member retains an unsafe RPATH: {wheel.name}"
                    )
                    raise ValueError(message)
                changed = True
                continue
            if _normalize_pe(binary):
                changed = True
        if changed:
            wheel_record(root)
        repack_wheel(root, wheel)


def wheel_build_environment(
    runtime_root: Path,
    vcpkg_path: Path | None,
    base: Mapping[str, str] | None = None,
) -> dict[str, str]:
    environment = dict(os.environ if base is None else base)
    environment["SOURCE_DATE_EPOCH"] = str(REPRODUCIBLE_ZIP_EPOCH)
    environment["PIP_DISABLE_PIP_VERSION_CHECK"] = "1"
    environment["PIP_NO_CACHE_DIR"] = "1"
    resolved_runtime = runtime_root.resolve()
    if os.name == "nt":
        prefix_map = subprocess.list2cmdline(
            [f"/pathmap:{resolved_runtime}={PORTABLE_BUILD_PREFIX}"]
        )
        compiler_flags = environment.get("CL", "").strip()
        environment["CL"] = " ".join(
            flag for flag in (compiler_flags, prefix_map) if flag
        )
    else:
        prefix_map = shlex.quote(
            f"-ffile-prefix-map={resolved_runtime}={PORTABLE_BUILD_PREFIX}"
        )
        compiler_flags = environment.get("CFLAGS", "").strip()
        environment["CFLAGS"] = " ".join(
            flag for flag in (compiler_flags, prefix_map) if flag
        )
        linker_flags = environment.get("LDFLAGS", "").strip()
        environment["LDFLAGS"] = " ".join(
            flag for flag in (linker_flags, "-Wl,--build-id=none") if flag
        )
    if vcpkg_path is not None:
        environment["VCPKG_PATH"] = str(vcpkg_path)
    return environment


def build_wheels(
    uv: Path,
    python: Path,
    requirements: Path,
    output: Path,
    runtime_root: Path,
    workspace: Path,
    patchelf: Path | None,
    strip: Path | None,
    vcpkg_path: Path | None,
) -> None:
    build_venv = workspace / "build-venv"
    environment = wheel_build_environment(runtime_root, vcpkg_path)
    run(
        [uv, "--no-config", "venv", build_venv, "--python", python],
        environment=environment,
    )
    build_python = python_executable(build_venv)
    run(
        [build_python, "-m", "ensurepip", "--upgrade"],
        environment=environment,
    )
    run(
        [
            uv,
            "--no-config",
            "pip",
            "install",
            "--python",
            build_python,
            f"setuptools=={SETUPTOOLS_VERSION}",
            f"wheel=={WHEEL_VERSION}",
        ],
        environment=environment,
    )
    run(
        [
            build_python,
            "-m",
            "pip",
            "wheel",
            "--no-build-isolation",
            "--no-deps",
            "--require-hashes",
            "--wheel-dir",
            output,
            "--requirement",
            requirements,
        ],
        environment=environment,
    )
    wheels = sorted(output.glob("*.whl"))
    if not wheels:
        message = "worker-script wheelhouse is empty"
        raise ValueError(message)
    if (patchelf is None) != (strip is None):
        message = "--patchelf and --strip must be provided together"
        raise ValueError(message)
    for wheel in wheels:
        normalize_wheel(wheel, patchelf, strip)


def verify_wheelhouse(
    uv: Path,
    python: Path,
    project: Path,
    wheelhouse: Path,
    workspace: Path,
    runtime_library_path: Path | None,
) -> list[dict[str, str]]:
    direct = workspace / "requirements-direct.txt"
    compiled = workspace / "requirements-compiled.txt"
    direct_requirements(project, direct)
    common = ["--offline", "--no-config", "--quiet", "pip"]
    package_source = ["--no-index", "--find-links", wheelhouse]
    run(
        [
            uv,
            *common,
            "compile",
            direct,
            "--python",
            python,
            "--output-file",
            compiled,
            *package_source,
        ]
    )
    verify_venv = workspace / "verify-venv"
    run([uv, "--no-config", "venv", verify_venv, "--python", python])
    verify_python = python_executable(verify_venv)
    run(
        [
            uv,
            *common,
            "sync",
            compiled,
            "--python",
            verify_python,
            *package_source,
        ]
    )
    smoke_environment = dict(os.environ)
    if runtime_library_path is not None:
        variable = "PATH" if os.name == "nt" else "LD_LIBRARY_PATH"
        current = smoke_environment.get(variable)
        smoke_environment[variable] = (
            str(runtime_library_path)
            if not current
            else os.pathsep.join((str(runtime_library_path), current))
        )
    run([verify_python, "-c", WORKER_RUNTIME_SMOKE], environment=smoke_environment)
    inventory = run(
        [
            uv,
            "--no-config",
            "pip",
            "list",
            "--python",
            verify_python,
            "--format",
            "json",
        ],
        capture=True,
    )
    parsed: object = json.loads(inventory)
    if not isinstance(parsed, list):
        message = "uv pip list returned an invalid inventory"
        raise ValueError(message)
    items = [
        object_mapping(item, "uv pip list entry")
        for item in cast("list[object]", parsed)
    ]
    return [
        {"name": str(item.get("name", "")), "version": str(item.get("version", ""))}
        for item in items
    ]


def build_release_runtime(
    project: Path,
    uv: Path,
    runtime_output: Path,
    wheelhouse_output: Path,
    patchelf: Path | None = None,
    strip: Path | None = None,
    vcpkg_path: Path | None = None,
    runtime_library_path: Path | None = None,
    execution_loader: Path | None = None,
    execution_library_path: str | None = None,
) -> dict[str, object]:
    if runtime_output.exists() or wheelhouse_output.exists():
        message = "release runtime outputs must not already exist"
        raise ValueError(message)
    runtime_output.parent.mkdir(parents=True, exist_ok=True)
    wheelhouse_output.mkdir(parents=True)
    with tempfile.TemporaryDirectory(
        prefix="pokecon-release-runtime-", dir=runtime_output.parent
    ) as directory:
        workspace = Path(directory)
        raw_python = install_python(uv, runtime_output, workspace)
        execution_python = raw_python
        execution_root = runtime_output
        raw_python_digest = sha256_file(raw_python)
        if os.name == "nt":
            if execution_loader is not None or execution_library_path is not None:
                message = (
                    "Windows portable Python must not receive an ELF execution boundary"
                )
                raise ValueError(message)
            verify_python(raw_python, runtime_output)
        elif sys.platform.startswith("linux"):
            if (
                patchelf is None
                or execution_loader is None
                or execution_library_path is None
            ):
                message = (
                    "Linux portable Python requires patchelf, an execution loader, "
                    "and an execution library path"
                )
                raise ValueError(message)
            execution_python, execution_root, raw_python_digest = (
                prepare_python_execution_copy(
                    runtime_output,
                    workspace,
                    patchelf,
                    execution_loader,
                    execution_library_path,
                )
            )
        else:
            verify_python(raw_python, runtime_output)
        requirements = wheelhouse_output / "requirements.lock"
        export_requirements(uv, project, requirements)
        build_wheels(
            uv,
            execution_python,
            requirements,
            wheelhouse_output,
            execution_root,
            workspace,
            patchelf,
            strip,
            vcpkg_path,
        )
        inventory = verify_wheelhouse(
            uv,
            execution_python,
            project,
            wheelhouse_output,
            workspace,
            runtime_library_path,
        )
        if sha256_file(raw_python) != raw_python_digest:
            message = "release build changed the raw portable Python executable"
            raise ValueError(message)
        if sys.platform.startswith("linux"):
            if patchelf is None:
                message = "Linux portable Python lost its patchelf audit boundary"
                raise ValueError(message)
            if portable_python_elf_metadata(raw_python, patchelf) != (
                PORTABLE_SYSTEM_INTERPRETER,
                PORTABLE_PYTHON_RPATH,
            ):
                message = "release build changed portable Python ELF metadata"
                raise ValueError(message)
        audit_python_bytecode(runtime_output)
    wheels = [
        {
            "path": wheel.name,
            "sha256": sha256_file(wheel),
            "size": wheel.stat().st_size,
        }
        for wheel in sorted(wheelhouse_output.glob("*.whl"))
    ]
    manifest: dict[str, object] = {
        "schema_version": 1,
        "python_version": PYTHON_VERSION,
        "uv_version": run([uv, "--version"], capture=True),
        "requirements_sha256": sha256_file(wheelhouse_output / "requirements.lock"),
        "wheels": wheels,
        "inventory": inventory,
    }
    manifest_path = wheelhouse_output / "wheelhouse-manifest.json"
    manifest_path.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(
        json.dumps(
            {
                "python_version": PYTHON_VERSION,
                "wheel_count": len(wheels),
                "inventory_count": len(inventory),
            },
            sort_keys=True,
        )
    )
    return manifest


def _resolve_pe_cli_path(raw: Path) -> Path:
    text = str(raw)
    if "://" in text:
        message = f"PE normalization does not accept remote input: {raw!r}"
        raise ValueError(message)
    candidate = Path(raw)
    if candidate.is_absolute():
        resolved = candidate
    else:
        resolved = (Path.cwd() / candidate).resolve(strict=False)
    if resolved.exists():
        return resolved
    # Fallback: try repository root relative (handles Tauri cwd ambiguity).
    repo_root = Path.cwd()
    for parent in [Path.cwd(), *list(Path.cwd().parents)]:
        if (parent / "Cargo.toml").exists():
            repo_root = parent
            break
    # Also consider script location as repo hint (deterministic, no broad except)
    script_parents = Path(__file__).resolve().parents
    if len(script_parents) > 3:
        script_root = script_parents[3]
        if (script_root / "Cargo.toml").exists():
            repo_root = script_root
    alternative = (repo_root / candidate).resolve(strict=False)
    if alternative.exists():
        return alternative
    # Also try ../../target style for rust/pokecon cwd
    if not candidate.is_absolute():
        for prefix in [Path("../../") / candidate, Path("../") / candidate]:
            prefixed = (Path.cwd() / prefix).resolve(strict=False)
            if prefixed.exists():
                return prefixed
    return resolved


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--normalize-pe", type=str, dest="normalize_pe")
    parser.add_argument("--project", type=Path, required=False)
    parser.add_argument("--uv", type=Path, required=False)
    parser.add_argument("--runtime-output", type=Path, required=False)
    parser.add_argument("--wheelhouse-output", type=Path, required=False)
    parser.add_argument("--patchelf", type=Path)
    parser.add_argument("--strip", type=Path)
    parser.add_argument("--vcpkg-path", type=Path)
    parser.add_argument("--runtime-library-path", type=Path)
    parser.add_argument("--execution-loader", type=Path)
    parser.add_argument("--execution-library-path")
    arguments = parser.parse_args()
    if arguments.normalize_pe is not None:
        if any(
            value is not None
            for value in (
                arguments.project,
                arguments.uv,
                arguments.runtime_output,
                arguments.wheelhouse_output,
            )
        ):
            parser.error(
                "--normalize-pe cannot be combined with runtime build arguments"
            )
        raw_normalize_pe = arguments.normalize_pe
        if "://" in raw_normalize_pe:
            parser.error("--normalize-pe accepts only local paths")
        target = _resolve_pe_cli_path(Path(raw_normalize_pe))
        if not target.exists() and os.name != "nt":
            return 0
        if target.is_symlink() or target.is_junction():
            message = f"PE target must be a regular file: {target}"
            raise ValueError(message)
        if not target.is_file():
            message = f"PE target is not a regular file: {target}"
            raise ValueError(message)
        normalize_pe(target)
        return 0
    if (
        arguments.project is None
        or arguments.uv is None
        or arguments.runtime_output is None
        or arguments.wheelhouse_output is None
    ):
        parser.error(
            "--project, --uv, --runtime-output, and --wheelhouse-output are required"
        )
    build_release_runtime(
        arguments.project.resolve(),
        arguments.uv.resolve(),
        arguments.runtime_output.resolve(),
        arguments.wheelhouse_output.resolve(),
        None if arguments.patchelf is None else arguments.patchelf.resolve(),
        None if arguments.strip is None else arguments.strip.resolve(),
        None if arguments.vcpkg_path is None else arguments.vcpkg_path.resolve(),
        None
        if arguments.runtime_library_path is None
        else arguments.runtime_library_path.resolve(),
        None
        if arguments.execution_loader is None
        else arguments.execution_loader.resolve(),
        arguments.execution_library_path,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
