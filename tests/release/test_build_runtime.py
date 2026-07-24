from __future__ import annotations

import os
import shlex
import zipfile
from pathlib import Path
from typing import TYPE_CHECKING

import pytest

import scripts.release.build_runtime as release_runtime
from scripts.release.build_runtime import (
    PORTABLE_BUILD_PREFIX,
    REPRODUCIBLE_ZIP_EPOCH,
    install_python,
    normalize_python_sysconfig,
    normalize_wheel,
    python_executable,
    wheel_build_environment,
)

if TYPE_CHECKING:
    from collections.abc import Mapping, Sequence


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


def test_install_python_discovers_the_requested_uv_installation(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    workspace = tmp_path / "workspace"
    workspace.mkdir()
    output = tmp_path / "runtime"
    commands: list[list[str]] = []

    def fake_run(
        arguments: Sequence[str | Path],
        *,
        environment: Mapping[str, str] | None = None,
        capture: bool = False,
    ) -> str:
        command = [str(argument) for argument in arguments]
        commands.append(command)
        if "install" in command:
            install_root = Path(command[command.index("--install-dir") + 1])
            installed = install_root / "cpython-fixture"
            executable = (
                installed / "python.exe"
                if os.name == "nt"
                else installed / "bin/python3.14"
            )
            executable.parent.mkdir(parents=True)
            executable.write_bytes(b"fixture")
            (install_root / "unrelated-layout").mkdir()
            return ""
        assert capture
        assert environment is not None
        install_root = Path(environment["UV_PYTHON_INSTALL_DIR"])
        executable = (
            install_root / "cpython-fixture/python.exe"
            if os.name == "nt"
            else install_root / "cpython-fixture/bin/python3.14"
        )
        return str(executable)

    monkeypatch.setattr(release_runtime, "run", fake_run)

    def ignore_normalize(_root: Path, _prefix: Path) -> None:
        return

    def ignore_verify(_python: Path, _root: Path) -> None:
        return

    monkeypatch.setattr(release_runtime, "normalize_python_sysconfig", ignore_normalize)
    monkeypatch.setattr(release_runtime, "verify_python", ignore_verify)

    installed = install_python(Path("uv"), output, workspace)

    assert installed == python_executable(output)
    assert any("find" in command for command in commands)
    assert not (output / "unrelated-layout").exists()


def test_python_executable_accepts_the_windows_venv_layout(tmp_path: Path) -> None:
    executable = tmp_path / "Scripts/python.exe"
    executable.parent.mkdir()
    executable.write_bytes(b"fixture")

    assert python_executable(tmp_path, platform_name="nt") == executable


def test_wheel_build_environment_maps_the_portable_python_prefix(
    tmp_path: Path,
) -> None:
    runtime = tmp_path / "portable python"
    environment = wheel_build_environment(runtime, None, {"CFLAGS": "-O2"})

    assert environment["SOURCE_DATE_EPOCH"] == str(REPRODUCIBLE_ZIP_EPOCH)
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


def test_wheel_bootstrap_uses_a_zip_compatible_reproducible_epoch(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    workspace = tmp_path / "workspace"
    workspace.mkdir()
    output = tmp_path / "wheelhouse"
    output.mkdir()
    environments: list[tuple[list[str], dict[str, str]]] = []

    def fake_run(
        arguments: Sequence[str | Path],
        *,
        environment: Mapping[str, str] | None = None,
        capture: bool = False,
    ) -> str:
        _ = capture
        command = [str(argument) for argument in arguments]
        assert environment is not None
        environments.append((command, dict(environment)))
        if "venv" in command:
            venv = Path(command[command.index("venv") + 1])
            executable = (
                venv / "Scripts/python.exe"
                if os.name == "nt"
                else venv / "bin/python3.14"
            )
            executable.parent.mkdir(parents=True)
            executable.write_bytes(b"fixture")
        if "wheel" in command:
            (output / "fixture-1.0-py3-none-any.whl").write_bytes(b"fixture")
        return ""

    monkeypatch.setattr(release_runtime, "run", fake_run)

    def ignore_normalize_wheel(
        _wheel: Path, _patchelf: Path | None, _strip: Path | None
    ) -> None:
        return

    monkeypatch.setattr(release_runtime, "normalize_wheel", ignore_normalize_wheel)

    release_runtime.build_wheels(
        Path("uv"),
        Path("python"),
        tmp_path / "requirements.lock",
        output,
        tmp_path / "runtime",
        workspace,
        None,
        None,
        None,
    )

    ensurepip_environment = next(
        environment for command, environment in environments if "ensurepip" in command
    )
    assert ensurepip_environment["SOURCE_DATE_EPOCH"] == str(REPRODUCIBLE_ZIP_EPOCH)


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
    root = Path(__file__).resolve().parents[2]
    workflow = (root / ".github/workflows" / workflow_name).read_text(encoding="utf-8")

    assert "$env:RUNNER_TEMP" in workflow
    assert "$PSNativeCommandUseErrorActionPreference = $true" in workflow
    assert "--runtime-output target/" not in workflow
    assert "--wheelhouse-output target/" not in workflow
    assert "--config $env:POKECON_BUNDLE_CONFIG" in workflow


def test_nix_release_task_isolates_reproducible_target_native_abi() -> None:
    root = Path(__file__).resolve().parents[2]
    flake = (root / "flake.nix").read_text(encoding="utf-8")

    assert 'linux-release-nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.05";' in flake
    assert 'linuxReleaseMaximumGlibc = "2.39";' in flake
    assert "lib.versionAtLeast linuxReleaseMaximumGlibc releaseGlibcVersion" in flake
    assert '"${pkgs.coreutils}/bin/env" -i' in flake
    assert 'CC="${linuxReleaseCc}/bin/gcc"' in flake
    assert 'CFLAGS="-I${linuxReleasePortaudio}/include"' in flake
    assert 'LDFLAGS="-L${linuxReleasePortaudio}/lib"' in flake
    assert 'PKG_CONFIG_PATH="${linuxReleasePortaudio}/lib/pkgconfig"' in flake
    assert "[build_ecodes]" in flake
    assert "${linuxReleasePkgs.linuxHeaders}/include/linux/input.h" in flake
    assert '"$release_build_home/.pydistutils.cfg"' in flake
    assert flake.count('--runtime-library-path "${linuxReleasePortaudio}/lib"') == 2
    assert 'CFLAGS="-I${pkgs.portaudio}/include' not in flake
    assert 'LDFLAGS="-L${pkgs.portaudio}/lib' not in flake
    assert 'cp -a "${source}/." "$workdir/"' in flake
    assert 'release_workdir="$CARGO_TARGET_DIR/pokecon-release-workdir"' in flake
    assert 'export CFLAGS="-ffile-prefix-map=$workdir=/build/pokecon' in flake
    assert 'export CXXFLAGS="-ffile-prefix-map=$workdir=/build/pokecon' in flake
    assert 'export POKECON_RUST_REMAP_SOURCE="$workdir"' in flake
    assert 'export POKECON_RUST_REMAP_PYTHON="$release_python"' in flake
    assert 'export RUSTC_WRAPPER="${reproducibleRustcWrapper}"' in flake
    assert "--remap-path-prefix=$POKECON_RUST_REMAP_SOURCE=/build/pokecon" in flake
    assert "--remap-path-prefix=$POKECON_RUST_REMAP_PYTHON=/build/python" in flake
    assert "-Lnative=$POKECON_RUST_REMAP_PYTHON/lib" in flake
    assert '--application "$normalized_application"' in flake
    assert '--worker "$normalized_worker"' in flake
    assert 'cp -p "$application_backup" "$application"' in flake
