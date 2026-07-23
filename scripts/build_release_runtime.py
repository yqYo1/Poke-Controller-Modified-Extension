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
import tempfile
import tomllib
import zipfile
from pathlib import Path
from typing import TYPE_CHECKING, cast

if TYPE_CHECKING:
    from collections.abc import Mapping, Sequence


PYTHON_VERSION = "3.14.3"
SETUPTOOLS_VERSION = "82.0.1"
WHEEL_VERSION = "0.46.3"
PORTABLE_BUILD_PREFIX = "/install"
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
    completed = subprocess.run(
        command,
        check=True,
        env=None if environment is None else dict(environment),
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


def python_executable(root: Path) -> Path:
    candidates = (
        [root / "python.exe", root / "bin/python.exe"]
        if os.name == "nt"
        else [root / "bin/python3.14", root / "bin/python3", root / "bin/python"]
    )
    executable = next((path for path in candidates if path.is_file()), None)
    if executable is None:
        message = f"portable Python has no supported executable below {root}"
        raise ValueError(message)
    return executable


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


def install_python(uv: Path, output: Path, workspace: Path) -> Path:
    install_root = workspace / "python-installs"
    run(
        [
            uv,
            "--no-config",
            "python",
            "install",
            PYTHON_VERSION,
            "--managed-python",
            "--no-bin",
            "--install-dir",
            install_root,
        ]
    )
    discovery_environment = os.environ.copy()
    discovery_environment["UV_PYTHON_INSTALL_DIR"] = str(install_root)
    discovered = run(
        [
            uv,
            "--no-config",
            "python",
            "find",
            PYTHON_VERSION,
            "--managed-python",
            "--no-python-downloads",
            "--no-project",
            "--resolve-links",
        ],
        environment=discovery_environment,
        capture=True,
    )
    if not discovered:
        message = "uv did not find the managed CPython installation it just installed"
        raise ValueError(message)
    discovered_python = Path(discovered).resolve()
    installed_prefix = (
        discovered_python.parent if os.name == "nt" else discovered_python.parent.parent
    )
    if installed_prefix.parent.resolve() != install_root.resolve():
        message = "uv found managed CPython outside the isolated installation root"
        raise ValueError(message)
    if python_executable(installed_prefix).resolve() != discovered_python:
        message = "uv found an unsupported managed CPython executable layout"
        raise ValueError(message)
    shutil.copytree(installed_prefix, output, symlinks=True)
    normalize_python_sysconfig(output, installed_prefix)
    executable = python_executable(output)
    verify_python(executable, output)
    return executable


def export_requirements(uv: Path, project: Path, output: Path) -> None:
    run(
        [
            uv,
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
            if not is_elf(binary):
                continue
            if patchelf is None or strip is None:
                message = f"native wheel normalization requires ELF tools: {wheel.name}"
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
            normalized_rpath = run([patchelf, "--print-rpath", binary], capture=True)
            if any(
                entry and not entry.startswith(("$ORIGIN", "${ORIGIN}"))
                for entry in normalized_rpath.split(":")
            ):
                message = f"native wheel member retains an unsafe RPATH: {wheel.name}"
                raise ValueError(message)
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
    environment["SOURCE_DATE_EPOCH"] = "0"
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
    run([uv, "--no-config", "venv", build_venv, "--python", python])
    build_python = python_executable(build_venv)
    run([build_python, "-m", "ensurepip", "--upgrade"])
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
        ]
    )
    environment = wheel_build_environment(runtime_root, vcpkg_path)
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
        python = install_python(uv, runtime_output, workspace)
        requirements = wheelhouse_output / "requirements.lock"
        export_requirements(uv, project, requirements)
        build_wheels(
            uv,
            python,
            requirements,
            wheelhouse_output,
            runtime_output,
            workspace,
            patchelf,
            strip,
            vcpkg_path,
        )
        inventory = verify_wheelhouse(
            uv,
            python,
            project,
            wheelhouse_output,
            workspace,
            runtime_library_path,
        )
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


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project", type=Path, required=True)
    parser.add_argument("--uv", type=Path, required=True)
    parser.add_argument("--runtime-output", type=Path, required=True)
    parser.add_argument("--wheelhouse-output", type=Path, required=True)
    parser.add_argument("--patchelf", type=Path)
    parser.add_argument("--strip", type=Path)
    parser.add_argument("--vcpkg-path", type=Path)
    parser.add_argument("--runtime-library-path", type=Path)
    arguments = parser.parse_args()
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
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
