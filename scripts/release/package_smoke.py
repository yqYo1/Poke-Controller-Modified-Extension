"""Audit a Debian bundle and exercise its packaged offline Python runtime."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import tempfile
import zipfile
from pathlib import Path, PurePosixPath
from typing import TYPE_CHECKING, cast

from scripts.release.build_runtime import PYTHON_VERSION, WORKER_RUNTIME_SMOKE

if TYPE_CHECKING:
    from collections.abc import Mapping, Sequence


SYSTEM_INTERPRETER = "/lib64/ld-linux-x86-64.so.2"
MAXIMUM_GLIBC = (2, 39)
MAXIMUM_INSTALLED_BYTES = 512 * 1024 * 1024
GLIBC_PATTERN = re.compile(r"GLIBC_(\d+)\.(\d+)")
ASSET_PATTERN = re.compile(r"[\"'(](/_app/[^\"')]+)")
REQUIRED_DEPENDENCIES = {
    "libgl1",
    "libglib2.0-0",
    "libgtk-3-0",
    "libportaudio2",
    "libsm6",
    "libudev1",
    "libwebkit2gtk-4.1-0",
    "libxext6",
    "libxrender1",
}
REQUIRED_WORKER_PACKAGES = {
    "icecream",
    "loguru",
    "numpy",
    "opencv-python",
    "pandas",
    "pillow",
    "pyaudio",
    "pygubu",
    "pynput",
    "pyserial",
    "pythonnet",
    "requests",
    "scipy",
}
UDEV_RULE_PATH = Path("usr/lib/udev/rules.d/70-pokecon-controller.rules")
EXPECTED_UDEV_RULES = (
    'SUBSYSTEM=="video4linux", TAG+="uaccess"',
    'SUBSYSTEM=="tty", KERNEL=="ttyACM[0-9]*", TAG+="uaccess"',
    'SUBSYSTEM=="tty", KERNEL=="ttyUSB[0-9]*", TAG+="uaccess"',
)


def run(
    arguments: Sequence[str | Path],
    *,
    environment: Mapping[str, str] | None = None,
) -> str:
    completed = subprocess.run(
        [str(argument) for argument in arguments],
        check=True,
        env=None if environment is None else dict(environment),
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
    )
    return completed.stdout.strip()


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


def object_list(value: object, label: str) -> list[object]:
    if not isinstance(value, list):
        message = f"{label} must be an array"
        raise ValueError(message)
    return cast("list[object]", value)


def load_object(path: Path, label: str) -> dict[str, object]:
    with path.open(encoding="utf-8") as source:
        return object_mapping(json.load(source), label)


def validate_resource_manifest(root: Path) -> tuple[dict[str, object], int]:
    manifest_path = root / "resource-manifest.json"
    manifest = load_object(manifest_path, "resource manifest")
    entries = object_list(manifest.get("files"), "resource manifest files")
    expected: dict[str, tuple[str, int]] = {}
    for raw_entry in entries:
        entry = object_mapping(raw_entry, "resource manifest entry")
        relative = entry.get("path")
        digest = entry.get("sha256")
        size = entry.get("size")
        if (
            not isinstance(relative, str)
            or not isinstance(digest, str)
            or not isinstance(size, int)
        ):
            message = "resource manifest entry has invalid fields"
            raise ValueError(message)
        parsed = PurePosixPath(relative)
        if (
            parsed.is_absolute()
            or not parsed.parts
            or any(part in {"", ".", ".."} for part in parsed.parts)
        ):
            message = f"resource manifest has an unsafe path: {relative!r}"
            raise ValueError(message)
        if relative in expected:
            message = f"resource manifest repeats a path: {relative}"
            raise ValueError(message)
        expected[relative] = (digest, size)

    actual: dict[str, tuple[str, int]] = {}
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            message = f"packaged resource must not be a symlink: {path}"
            raise ValueError(message)
        if path.is_file() and path != manifest_path:
            relative = path.relative_to(root).as_posix()
            actual[relative] = (sha256_file(path), path.stat().st_size)
    if actual != expected:
        missing = sorted(expected.keys() - actual.keys())
        extra = sorted(actual.keys() - expected.keys())
        changed = sorted(
            path
            for path in expected.keys() & actual.keys()
            if expected[path] != actual[path]
        )
        message = f"resource manifest mismatch: missing={missing}, extra={extra}, changed={changed}"
        raise ValueError(message)
    canonical = json.dumps(
        entries, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode()
    content_sha256 = manifest.get("content_sha256")
    if content_sha256 != hashlib.sha256(canonical).hexdigest():
        message = "resource manifest content identity is invalid"
        raise ValueError(message)
    return manifest, len(actual)


def validate_spa(root: Path) -> tuple[int, int]:
    web = root / "web/dist"
    index = web / "index.html"
    html = index.read_text(encoding="utf-8")
    folded = html.casefold()
    if "404" in folded or "not found" in folded:
        message = "packaged SPA is the SvelteKit fallback error page"
        raise ValueError(message)
    assets = {match.lstrip("/") for match in ASSET_PATTERN.findall(html)}
    if not assets or not any("/entry/app." in asset for asset in assets):
        message = "packaged SPA has no Svelte application entry"
        raise ValueError(message)
    missing = sorted(asset for asset in assets if not (web / asset).is_file())
    if missing:
        message = f"packaged SPA references missing assets: {missing}"
        raise ValueError(message)
    javascript = sum(path.suffix == ".js" for path in web.rglob("*"))
    stylesheets = sum(path.suffix == ".css" for path in web.rglob("*"))
    if javascript < 5 or stylesheets < 1:
        message = (
            f"packaged SPA asset set is incomplete: js={javascript}, css={stylesheets}"
        )
        raise ValueError(message)
    return javascript, stylesheets


def validate_wheelhouse(root: Path) -> tuple[list[dict[str, str]], list[Path]]:
    wheelhouse = root / "python-wheels"
    manifest = load_object(
        wheelhouse / "wheelhouse-manifest.json", "wheelhouse manifest"
    )
    requirements = wheelhouse / "requirements.lock"
    if manifest.get("python_version") != PYTHON_VERSION:
        message = "wheelhouse targets a different Python version"
        raise ValueError(message)
    if manifest.get("requirements_sha256") != sha256_file(requirements):
        message = "wheelhouse requirements identity is invalid"
        raise ValueError(message)

    wheels = sorted(wheelhouse.glob("*.whl"))
    entries = object_list(manifest.get("wheels"), "wheelhouse wheels")
    expected: dict[str, tuple[str, int]] = {}
    for raw_entry in entries:
        entry = object_mapping(raw_entry, "wheelhouse wheel")
        name = entry.get("path")
        digest = entry.get("sha256")
        size = entry.get("size")
        if (
            not isinstance(name, str)
            or not isinstance(digest, str)
            or not isinstance(size, int)
        ):
            message = "wheelhouse wheel has invalid fields"
            raise ValueError(message)
        if PurePosixPath(name).name != name or not name.endswith(".whl"):
            message = f"wheelhouse has an unsafe wheel name: {name!r}"
            raise ValueError(message)
        expected[name] = (digest, size)
    actual = {
        wheel.name: (sha256_file(wheel), wheel.stat().st_size) for wheel in wheels
    }
    if actual != expected:
        message = "wheelhouse files do not match wheelhouse-manifest.json"
        raise ValueError(message)

    inventory: list[dict[str, str]] = []
    for raw_item in object_list(manifest.get("inventory"), "wheelhouse inventory"):
        item = object_mapping(raw_item, "wheelhouse inventory entry")
        name = item.get("name")
        version = item.get("version")
        if (
            not isinstance(name, str)
            or not isinstance(version, str)
            or not name
            or not version
        ):
            message = "wheelhouse inventory entry has invalid fields"
            raise ValueError(message)
        inventory.append({"name": name, "version": version})
    names = {item["name"].casefold() for item in inventory}
    if not names >= REQUIRED_WORKER_PACKAGES:
        message = f"wheelhouse misses worker packages: {sorted(REQUIRED_WORKER_PACKAGES - names)}"
        raise ValueError(message)
    if not any(
        re.match(r"pyaudio-0\.2\.14-cp314-cp314-", wheel.name, re.IGNORECASE)
        for wheel in wheels
    ):
        message = "wheelhouse has no CPython 3.14 PyAudio 0.2.14 wheel"
        raise ValueError(message)
    allowed = {
        "requirements.lock",
        "wheelhouse-manifest.json",
        *(wheel.name for wheel in wheels),
    }
    unexpected = sorted(
        path.name for path in wheelhouse.iterdir() if path.name not in allowed
    )
    if unexpected:
        message = f"wheelhouse contains unexpected entries: {unexpected}"
        raise ValueError(message)
    return inventory, wheels


def is_elf(path: Path) -> bool:
    with path.open("rb") as source:
        return source.read(4) == b"\x7fELF"


def audit_elf(
    path: Path,
    patchelf: Path,
    objdump: Path,
    *,
    expected_interpreter: str | None = None,
    expected_rpath: str | None = None,
    required_libraries: frozenset[str] | None = None,
) -> dict[str, object]:
    if not is_elf(path):
        message = f"expected an ELF binary: {path}"
        raise ValueError(message)
    rpath = run([patchelf, "--print-rpath", path])
    needed = {
        line for line in run([patchelf, "--print-needed", path]).splitlines() if line
    }
    if expected_interpreter is not None:
        interpreter = run([patchelf, "--print-interpreter", path])
        if interpreter != expected_interpreter:
            message = f"unexpected ELF interpreter for {path}: {interpreter!r}"
            raise ValueError(message)
    if expected_rpath is not None:
        if rpath != expected_rpath:
            message = f"unexpected RPATH for {path}: {rpath!r}"
            raise ValueError(message)
    elif any(
        entry and not entry.startswith(("$ORIGIN", "${ORIGIN}"))
        for entry in rpath.split(":")
    ):
        message = f"ELF binary has a non-relative RPATH: {path}: {rpath!r}"
        raise ValueError(message)
    if any("/" in library or library.startswith(".") for library in needed):
        message = f"ELF binary has a path-qualified dependency: {path}"
        raise ValueError(message)
    required = frozenset[str]() if required_libraries is None else required_libraries
    if not required <= needed:
        message = f"ELF binary misses dependencies {sorted(required - needed)}: {path}"
        raise ValueError(message)
    symbols = run([objdump, "-T", path])
    versions = {
        (int(match.group(1)), int(match.group(2)))
        for match in GLIBC_PATTERN.finditer(symbols)
    }
    maximum = max(versions, default=(0, 0))
    if maximum[0] > MAXIMUM_GLIBC[0] or (
        maximum[0] == MAXIMUM_GLIBC[0] and maximum[1] > MAXIMUM_GLIBC[1]
    ):
        message = f"{path} requires GLIBC {maximum[0]}.{maximum[1]}"
        raise ValueError(message)
    return {
        "path": str(path),
        "rpath": rpath,
        "maximum_glibc": f"{maximum[0]}.{maximum[1]}",
    }


def audit_native_wheels(wheels: Sequence[Path], patchelf: Path, objdump: Path) -> int:
    native_members = 0
    pyaudio_linked = False
    with tempfile.TemporaryDirectory(prefix="pokecon-package-wheels-") as directory:
        temporary = Path(directory)
        for wheel in wheels:
            destination = temporary / wheel.stem
            destination.mkdir()
            with zipfile.ZipFile(wheel) as archive:
                for member in archive.infolist():
                    path = PurePosixPath(member.filename)
                    if path.is_absolute() or any(
                        part in {"", ".", ".."} for part in path.parts
                    ):
                        message = f"wheel has an unsafe member: {wheel.name}: {member.filename!r}"
                        raise ValueError(message)
                archive.extractall(destination)
            for member in sorted(
                path for path in destination.rglob("*") if path.is_file()
            ):
                if not is_elf(member):
                    continue
                required: frozenset[str] = (
                    frozenset({"libportaudio.so.2"})
                    if wheel.name.casefold().startswith("pyaudio-")
                    else frozenset[str]()
                )
                audit_elf(
                    member,
                    patchelf,
                    objdump,
                    required_libraries=required,
                )
                native_members += 1
                pyaudio_linked = pyaudio_linked or bool(required)
    if not pyaudio_linked:
        message = "packaged PyAudio extension was not linked to PortAudio v19"
        raise ValueError(message)
    return native_members


def runtime_environment(runtime_library_path: Path | None) -> dict[str, str]:
    environment = dict(os.environ)
    environment["UV_PYTHON_DOWNLOADS"] = "never"
    if runtime_library_path is not None:
        current = environment.get("LD_LIBRARY_PATH")
        environment["LD_LIBRARY_PATH"] = (
            str(runtime_library_path)
            if not current
            else os.pathsep.join((str(runtime_library_path), current))
        )
    return environment


def exercise_offline_runtime(
    root: Path,
    inventory: Sequence[dict[str, str]],
    runtime_library_path: Path | None,
) -> tuple[str, str]:
    uv = root / "uv/uv"
    python = root / "python/bin/python3.14"
    environment = runtime_environment(runtime_library_path)
    uv_version = run([uv, "--version"], environment=environment)
    python_version = run(
        [
            python,
            "-c",
            "import platform, sqlite3, ssl, tkinter; print(platform.python_version())",
        ],
        environment=environment,
    )
    if python_version != PYTHON_VERSION:
        message = f"packaged Python version is {python_version!r}"
        raise ValueError(message)
    with tempfile.TemporaryDirectory(prefix="pokecon-package-runtime-") as directory:
        workspace = Path(directory)
        environment["UV_CACHE_DIR"] = str(workspace / "uv-cache")
        requirements = workspace / "requirements.txt"
        requirements.write_text(
            "".join(
                f"{item['name']}=={item['version']}\n"
                for item in sorted(inventory, key=lambda item: item["name"].casefold())
            ),
            encoding="utf-8",
        )
        venv = workspace / "venv"
        run(
            [
                uv,
                "--offline",
                "--no-config",
                "--quiet",
                "venv",
                venv,
                "--python",
                python,
            ],
            environment=environment,
        )
        venv_python = venv / "bin/python3.14"
        run(
            [
                uv,
                "--offline",
                "--no-config",
                "--quiet",
                "pip",
                "sync",
                requirements,
                "--python",
                venv_python,
                "--no-index",
                "--find-links",
                root / "python-wheels",
            ],
            environment=environment,
        )
        run([venv_python, "-c", WORKER_RUNTIME_SMOKE], environment=environment)
    return uv_version, python_version


def dependency_names(value: str) -> set[str]:
    return {
        alternative.split(maxsplit=1)[0]
        for group in value.split(",")
        for alternative in group.split("|")
        if alternative.strip()
    }


def validate_udev_support(extracted: Path) -> int:
    rules_path = extracted / UDEV_RULE_PATH
    if not rules_path.is_file():
        message = f"Debian package is missing its udev rules: {UDEV_RULE_PATH}"
        raise ValueError(message)
    effective_rules = tuple(
        line.strip()
        for line in rules_path.read_text(encoding="utf-8").splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    )
    if effective_rules != EXPECTED_UDEV_RULES:
        message = f"Debian package has unexpected udev rules: {effective_rules}"
        raise ValueError(message)
    for script_name in ("postinst", "postrm"):
        script = extracted / "DEBIAN" / script_name
        if not script.is_file():
            message = f"Debian package is missing its {script_name} script"
            raise ValueError(message)
        content = script.read_text(encoding="utf-8")
        if "udevadm control --reload-rules" not in content:
            message = f"Debian package {script_name} does not reload udev rules"
            raise ValueError(message)
    return len(effective_rules)


def validate_debian_package(
    package: Path,
    dpkg_deb: Path,
    patchelf: Path,
    objdump: Path,
    runtime_library_path: Path | None,
) -> dict[str, object]:
    if not package.is_file():
        message = f"Debian package is missing: {package}"
        raise ValueError(message)
    dependencies = dependency_names(run([dpkg_deb, "--field", package, "Depends"]))
    if not dependencies >= REQUIRED_DEPENDENCIES:
        message = f"Debian package misses dependencies: {sorted(REQUIRED_DEPENDENCIES - dependencies)}"
        raise ValueError(message)
    with tempfile.TemporaryDirectory(prefix="pokecon-debian-") as directory:
        extracted = Path(directory)
        run([dpkg_deb, "--raw-extract", package, extracted])
        udev_rules = validate_udev_support(extracted)
        manifests = list((extracted / "usr/lib").glob("*/resource-manifest.json"))
        if len(manifests) != 1:
            message = "Debian package must contain exactly one resource manifest"
            raise ValueError(message)
        resources = manifests[0].parent
        _manifest, file_count = validate_resource_manifest(resources)
        javascript, stylesheets = validate_spa(resources)
        inventory, wheels = validate_wheelhouse(resources)
        application = extracted / "usr/bin/pokecon"
        worker = resources / "pokecon-worker"
        uv = resources / "uv/uv"
        python = resources / "python/bin/python3.14"
        audit_elf(
            application,
            patchelf,
            objdump,
            expected_interpreter=SYSTEM_INTERPRETER,
            expected_rpath="",
        )
        audit_elf(
            worker,
            patchelf,
            objdump,
            expected_interpreter=SYSTEM_INTERPRETER,
            expected_rpath="$ORIGIN/python/lib",
            required_libraries=frozenset({"libpython3.14.so.1.0"}),
        )
        audit_elf(
            uv,
            patchelf,
            objdump,
            expected_interpreter=SYSTEM_INTERPRETER,
            expected_rpath="",
        )
        audit_elf(
            python,
            patchelf,
            objdump,
            expected_interpreter=SYSTEM_INTERPRETER,
            expected_rpath="$ORIGIN/../lib",
        )
        native_members = audit_native_wheels(wheels, patchelf, objdump)
        uv_version, python_version = exercise_offline_runtime(
            resources, inventory, runtime_library_path
        )
        installed_bytes = sum(
            path.stat().st_size
            for path in extracted.rglob("*")
            if path.is_file() and not path.is_symlink()
        )
        if installed_bytes > MAXIMUM_INSTALLED_BYTES:
            message = f"Debian package expands to {installed_bytes} bytes"
            raise ValueError(message)
        return {
            "package": str(package),
            "version": run([dpkg_deb, "--field", package, "Version"]),
            "architecture": run([dpkg_deb, "--field", package, "Architecture"]),
            "resource_files": file_count,
            "wheel_count": len(wheels),
            "native_wheel_members": native_members,
            "javascript_assets": javascript,
            "stylesheet_assets": stylesheets,
            "installed_bytes": installed_bytes,
            "uv_version": uv_version,
            "python_version": python_version,
            "udev_rules": udev_rules,
        }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", type=Path)
    parser.add_argument("--dpkg-deb", type=Path, required=True)
    parser.add_argument("--patchelf", type=Path, required=True)
    parser.add_argument("--objdump", type=Path, required=True)
    parser.add_argument("--runtime-library-path", type=Path)
    arguments = parser.parse_args()
    report = validate_debian_package(
        arguments.package.resolve(),
        arguments.dpkg_deb.resolve(),
        arguments.patchelf.resolve(),
        arguments.objdump.resolve(),
        None
        if arguments.runtime_library_path is None
        else arguments.runtime_library_path.resolve(),
    )
    print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
