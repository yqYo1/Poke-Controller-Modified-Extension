from __future__ import annotations

from pathlib import Path

import pytest

from scripts.build_release_runtime import (
    PORTABLE_BUILD_PREFIX,
    normalize_python_sysconfig,
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
