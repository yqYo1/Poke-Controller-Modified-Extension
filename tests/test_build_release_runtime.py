from __future__ import annotations

import os
import shlex
import zipfile
from pathlib import Path

import pytest

from scripts.build_release_runtime import (
    PORTABLE_BUILD_PREFIX,
    normalize_python_sysconfig,
    normalize_wheel,
    wheel_build_environment,
)


def test_normalize_python_sysconfig_replaces_temporary_prefix(tmp_path: Path) -> None:
    runtime = tmp_path / "runtime"
    sysconfig = runtime / "lib/python3.14/_sysconfigdata__linux_fixture.py"
    sysconfig.parent.mkdir(parents=True)
    installed_prefix = tmp_path / "temporary/python-install"
    sysconfig.write_text(
        f"build_time_vars = {{'prefix': {str(installed_prefix)!r}, "
        f"'BINDIR': {str(installed_prefix / 'bin')!r}}}\n",
        encoding="utf-8",
    )

    normalize_python_sysconfig(runtime, installed_prefix)

    content = sysconfig.read_text(encoding="utf-8")
    assert str(installed_prefix) not in content
    assert content.count(PORTABLE_BUILD_PREFIX) == 2


def test_normalize_python_sysconfig_rejects_unrelated_prefix(tmp_path: Path) -> None:
    runtime = tmp_path / "runtime"
    sysconfig = runtime / "lib/python3.14/_sysconfigdata__linux_fixture.py"
    sysconfig.parent.mkdir(parents=True)
    sysconfig.write_text(
        "build_time_vars = {'prefix': '/somewhere/else'}\n", encoding="utf-8"
    )

    with pytest.raises(ValueError, match="does not contain its install prefix"):
        normalize_python_sysconfig(runtime, tmp_path / "temporary/python-install")


def test_wheel_build_environment_maps_the_portable_python_prefix(
    tmp_path: Path,
) -> None:
    runtime = tmp_path / "portable python"
    environment = wheel_build_environment(runtime, None, {"CFLAGS": "-O2"})

    assert environment["SOURCE_DATE_EPOCH"] == "0"
    if os.name == "nt":
        assert (
            f"/pathmap:{runtime.resolve()}={PORTABLE_BUILD_PREFIX}" in environment["CL"]
        )
    else:
        assert shlex.split(environment["CFLAGS"]) == [
            "-O2",
            f"-ffile-prefix-map={runtime.resolve()}={PORTABLE_BUILD_PREFIX}",
        ]
        assert environment["LDFLAGS"] == "-Wl,--build-id=none"


def test_normalize_wheel_repacks_unchanged_members_deterministically(
    tmp_path: Path,
) -> None:
    first = tmp_path / "first.whl"
    second = tmp_path / "second.whl"
    members = {
        "example/__init__.py": b"VALUE = 1\n",
        "example-1.0.dist-info/RECORD": b"",
    }
    for wheel, date_time, names in [
        (first, (2025, 1, 2, 3, 4, 6), list(members)),
        (second, (2026, 7, 8, 9, 10, 12), list(reversed(members))),
    ]:
        with zipfile.ZipFile(wheel, "w") as archive:
            for name in names:
                info = zipfile.ZipInfo(name, date_time=date_time)
                archive.writestr(info, members[name])

    normalize_wheel(first, None, None)
    normalize_wheel(second, None, None)

    assert first.read_bytes() == second.read_bytes()


@pytest.mark.parametrize("workflow_name", ["package.yml", "release.yml"])
def test_windows_release_resources_are_isolated_from_cargo_cache(
    workflow_name: str,
) -> None:
    root = Path(__file__).resolve().parents[1]
    workflow = (root / ".github/workflows" / workflow_name).read_text(encoding="utf-8")

    assert "$env:RUNNER_TEMP" in workflow
    assert "--runtime-output target/" not in workflow
    assert "--wheelhouse-output target/" not in workflow
    assert "--config $env:POKECON_BUNDLE_CONFIG" in workflow


def test_nix_release_task_declares_native_build_paths() -> None:
    root = Path(__file__).resolve().parents[1]
    flake = (root / "flake.nix").read_text(encoding="utf-8")

    assert 'CFLAGS="-I${pkgs.portaudio}/include' in flake
    assert 'LDFLAGS="-L${pkgs.portaudio}/lib' in flake
    assert 'NIX_LDFLAGS="-L$release_python/lib' in flake
