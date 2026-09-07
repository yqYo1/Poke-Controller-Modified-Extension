from __future__ import annotations

import json
import os
import shlex
import subprocess
import sys
import zipfile
from pathlib import Path
from typing import TYPE_CHECKING

import pytest

import scripts.release.build_runtime as release_runtime
from scripts.release.build_runtime import (
    LINUX_PYTHON_INSTALL_REQUEST,
    LINUX_PYTHON_MINOR_REDIRECT,
    PORTABLE_BUILD_PREFIX,
    PORTABLE_PYTHON_RPATH,
    PORTABLE_SYSTEM_INTERPRETER,
    REPRODUCIBLE_ZIP_EPOCH,
    WINDOWS_PYTHON_INSTALL_REQUEST,
    WINDOWS_PYTHON_MINOR_REDIRECT,
    audit_python_bytecode,
    build_release_runtime,
    install_python,
    managed_python_install_prefix,
    normalize_python_bytecode,
    normalize_python_sysconfig,
    normalize_wheel,
    prepare_python_execution_copy,
    python_executable,
    python_install_request,
    python_minor_redirect_name,
    run,
    verify_python,
    wheel_build_environment,
)

if TYPE_CHECKING:
    from collections.abc import Mapping, Sequence


def create_managed_python_layout(
    install_root: Path, platform_name: str
) -> tuple[Path, Path, str]:
    if platform_name == "linux":
        full_name = LINUX_PYTHON_INSTALL_REQUEST
        minor_name = LINUX_PYTHON_MINOR_REDIRECT
    elif platform_name == "win32":
        full_name = WINDOWS_PYTHON_INSTALL_REQUEST
        minor_name = WINDOWS_PYTHON_MINOR_REDIRECT
    else:
        raise AssertionError(platform_name)
    install_root.mkdir()
    (install_root / ".gitignore").write_bytes(b"*")
    (install_root / ".lock").write_bytes(b"")
    (install_root / ".temp").mkdir()
    full_install = install_root / full_name
    full_install.mkdir()
    minor_redirect = install_root / minor_name
    minor_redirect.mkdir()
    return full_install, minor_redirect, full_name


def emulate_redirects(
    monkeypatch: pytest.MonkeyPatch,
    redirects: Mapping[Path, tuple[str, Path]],
) -> None:
    original_is_symlink = Path.is_symlink
    original_is_junction = Path.is_junction
    original_resolve = Path.resolve

    def fake_is_symlink(path: Path) -> bool:
        redirect = redirects.get(path)
        if redirect is not None:
            return redirect[0] == "symlink"
        return original_is_symlink(path)

    def fake_is_junction(path: Path) -> bool:
        redirect = redirects.get(path)
        if redirect is not None:
            return redirect[0] == "junction"
        return original_is_junction(path)

    def fake_resolve(path: Path, strict: bool = False) -> Path:
        redirect = redirects.get(path)
        if redirect is not None:
            return original_resolve(redirect[1], strict=strict)
        return original_resolve(path, strict=strict)

    monkeypatch.setattr(Path, "is_symlink", fake_is_symlink)
    monkeypatch.setattr(Path, "is_junction", fake_is_junction)
    monkeypatch.setattr(Path, "resolve", fake_resolve)


@pytest.mark.parametrize(
    ("host_bytecode", "supplied_environment"),
    [
        (None, None),
        ("host-disabled", None),
        (
            "host-disabled",
            {"PYTHONDONTWRITEBYTECODE": "supplied-disabled", "SUPPLIED": "yes"},
        ),
    ],
)
def test_run_forces_no_bytecode_for_every_child_process(
    monkeypatch: pytest.MonkeyPatch,
    host_bytecode: str | None,
    supplied_environment: Mapping[str, str] | None,
) -> None:
    monkeypatch.setenv("HOST_SENTINEL", "inherited")
    if host_bytecode is None:
        monkeypatch.delenv("PYTHONDONTWRITEBYTECODE", raising=False)
    else:
        monkeypatch.setenv("PYTHONDONTWRITEBYTECODE", host_bytecode)
    captured_environments: list[dict[str, str]] = []

    def fake_subprocess_run(
        command: Sequence[str],
        *,
        check: bool,
        env: Mapping[str, str],
        stdin: int,
        stdout: int | None,
        stderr: int | None,
        text: bool,
    ) -> subprocess.CompletedProcess[str]:
        assert list(command) == ["fixture", "argument"]
        assert check
        assert stdin == subprocess.DEVNULL
        assert stdout == subprocess.PIPE
        assert stderr == subprocess.PIPE
        assert text
        captured_environments.append(dict(env))
        return subprocess.CompletedProcess(command, 0, stdout=" captured\n", stderr="")

    monkeypatch.setattr(subprocess, "run", fake_subprocess_run)

    assert (
        run(
            ["fixture", Path("argument")],
            environment=supplied_environment,
            capture=True,
        )
        == "captured"
    )
    assert len(captured_environments) == 1
    child_environment = captured_environments[0]
    assert child_environment["PYTHONDONTWRITEBYTECODE"] == "1"
    if supplied_environment is None:
        assert child_environment["HOST_SENTINEL"] == "inherited"
    else:
        assert child_environment == {
            "PYTHONDONTWRITEBYTECODE": "1",
            "SUPPLIED": "yes",
        }


@pytest.mark.parametrize(
    ("platform_name", "expected"),
    [
        ("linux", "cpython-3.14.3-linux-x86_64-gnu"),
        ("win32", "cpython-3.14.3-windows-x86_64-none"),
    ],
)
def test_python_install_request_is_exact_for_supported_platforms(
    platform_name: str, expected: str
) -> None:
    assert python_install_request(platform_name) == expected


def test_python_install_request_reads_platform_constants(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        release_runtime,
        "LINUX_PYTHON_INSTALL_REQUEST",
        "sentinel-linux-request",
    )
    monkeypatch.setattr(
        release_runtime,
        "WINDOWS_PYTHON_INSTALL_REQUEST",
        "sentinel-windows-request",
    )

    assert python_install_request("linux") == "sentinel-linux-request"
    assert python_install_request("win32") == "sentinel-windows-request"


@pytest.mark.parametrize("platform_name", ["darwin", "freebsd", "linux2", "cygwin"])
def test_python_install_request_rejects_unknown_platform(platform_name: str) -> None:
    with pytest.raises(ValueError, match="does not support"):
        python_install_request(platform_name)


@pytest.mark.parametrize(
    ("platform_name", "expected"),
    [
        ("linux", "cpython-3.14-linux-x86_64-gnu"),
        ("win32", "cpython-3.14-windows-x86_64-none"),
    ],
)
def test_python_minor_redirect_name_is_exact_for_supported_platforms(
    platform_name: str, expected: str
) -> None:
    assert python_minor_redirect_name(platform_name) == expected


@pytest.mark.parametrize("platform_name", ["darwin", "freebsd", "linux2", "cygwin"])
def test_python_minor_redirect_name_rejects_unknown_platform(
    platform_name: str,
) -> None:
    with pytest.raises(ValueError, match="does not support"):
        python_minor_redirect_name(platform_name)


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


def test_normalize_python_bytecode_removes_nested_caches_and_preserves_other_files(
    tmp_path: Path,
) -> None:
    runtime = tmp_path / "runtime"
    package = runtime / "lib/python3.14/package"
    first_cache = runtime / "lib/python3.14/__pycache__"
    second_cache = package / "__pycache__"
    nested_cache = second_cache / "__pycache__"
    first_cache.mkdir(parents=True)
    nested_cache.mkdir(parents=True)
    bytecode_files = (
        first_cache / "aliases.cpython-314.pyc",
        second_cache / "module.cpython-314.pyc",
        nested_cache / "nested.cpython-314.pyc",
    )
    for bytecode_file in bytecode_files:
        bytecode_file.write_bytes(b"upstream-bytecode")
    source = package / "module.py"
    source.write_text("VALUE = 1\n", encoding="utf-8")
    unrelated = runtime / "share/data.bin"
    unrelated.parent.mkdir(parents=True)
    unrelated.write_bytes(b"preserve")

    normalize_python_bytecode(runtime)

    assert all(not bytecode_file.exists() for bytecode_file in bytecode_files)
    assert all(
        not cache_directory.exists()
        for cache_directory in (first_cache, second_cache, nested_cache)
    )
    assert source.read_text(encoding="utf-8") == "VALUE = 1\n"
    assert unrelated.read_bytes() == b"preserve"


def test_normalize_python_bytecode_rejects_loose_pyc_without_changes(
    tmp_path: Path,
) -> None:
    runtime = tmp_path / "runtime"
    cache = runtime / "lib/python3.14/__pycache__"
    cache.mkdir(parents=True)
    cached_bytecode = cache / "module.cpython-314.pyc"
    cached_bytecode.write_bytes(b"cached")
    loose_bytecode = runtime / "loose.pyc"
    loose_bytecode.write_bytes(b"loose")

    with pytest.raises(ValueError, match="outside a real __pycache__ directory"):
        normalize_python_bytecode(runtime)

    assert cached_bytecode.read_bytes() == b"cached"
    assert loose_bytecode.read_bytes() == b"loose"
    assert cache.is_dir()


@pytest.mark.parametrize(
    ("entry_kind", "redirect_kind"),
    [
        ("cache", "symlink"),
        ("cache", "junction"),
        ("bytecode", "symlink"),
        ("bytecode", "junction"),
    ],
)
def test_normalize_python_bytecode_rejects_redirected_entries(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    entry_kind: str,
    redirect_kind: str,
) -> None:
    runtime = tmp_path / "runtime"
    runtime.mkdir()
    entry = runtime / ("__pycache__" if entry_kind == "cache" else "module.pyc")
    outside = tmp_path / f"outside-{entry_kind}"
    if entry_kind == "cache":
        entry.mkdir()
        outside.mkdir()
        outside_sentinel = outside / "sentinel"
        outside_sentinel.write_bytes(b"outside")
    else:
        entry.write_bytes(b"upstream-bytecode")
        outside_sentinel = outside
        outside_sentinel.write_bytes(b"outside")
    emulate_redirects(monkeypatch, {entry: (redirect_kind, outside)})

    with pytest.raises(ValueError, match="portable Python bytecode"):
        normalize_python_bytecode(runtime)

    assert entry.exists()
    assert outside_sentinel.read_bytes() == b"outside"


def test_normalize_python_bytecode_rejects_nonregular_pyc(tmp_path: Path) -> None:
    runtime = tmp_path / "runtime"
    nonregular_bytecode = runtime / "module.pyc"
    nonregular_bytecode.mkdir(parents=True)

    with pytest.raises(ValueError, match="is not one real file"):
        normalize_python_bytecode(runtime)

    assert nonregular_bytecode.is_dir()


def test_normalize_python_bytecode_rejects_non_pyc_cache_residue(
    tmp_path: Path,
) -> None:
    cache = tmp_path / "runtime/lib/python3.14/__pycache__"
    cache.mkdir(parents=True)
    bytecode = cache / "module.cpython-314.pyc"
    bytecode.write_bytes(b"upstream-bytecode")
    residue = cache / "README"
    residue.write_text("preserve\n", encoding="utf-8")

    with pytest.raises(ValueError, match="contains non-bytecode residue"):
        normalize_python_bytecode(tmp_path / "runtime")

    assert bytecode.read_bytes() == b"upstream-bytecode"
    assert cache.is_dir()
    assert residue.read_text(encoding="utf-8") == "preserve\n"


def test_audit_python_bytecode_accepts_a_clean_runtime(tmp_path: Path) -> None:
    runtime = tmp_path / "runtime"
    source = runtime / "lib/python3.14/module.py"
    source.parent.mkdir(parents=True)
    source.write_text("VALUE = 1\n", encoding="utf-8")

    audit_python_bytecode(runtime)

    assert source.read_text(encoding="utf-8") == "VALUE = 1\n"


@pytest.mark.parametrize("artifact_kind", ["cache", "bytecode", "optimized-bytecode"])
def test_audit_python_bytecode_rejects_and_preserves_artifacts(
    tmp_path: Path,
    artifact_kind: str,
) -> None:
    runtime = tmp_path / "runtime"
    runtime.mkdir()
    artifact_name = {
        "cache": "__pycache__",
        "bytecode": "loose.pyc",
        "optimized-bytecode": "loose.pyo",
    }[artifact_kind]
    artifact = runtime / artifact_name
    if artifact_kind == "cache":
        artifact.mkdir()
    else:
        artifact.write_bytes(b"late-bytecode")

    with pytest.raises(ValueError, match="contains a bytecode artifact"):
        audit_python_bytecode(runtime)

    assert artifact.exists()


def test_build_release_runtime_final_audit_rejects_late_bytecode(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    runtime = tmp_path / "runtime"
    wheelhouse = tmp_path / "wheelhouse"
    late_bytecode = runtime / "Lib/__pycache__/late.cpython-314.pyc"
    event_order: list[str] = []

    def fake_install_python(_uv: Path, output: Path, _workspace: Path) -> Path:
        event_order.append("install")
        python = output / "bin/python3.14"
        python.parent.mkdir(parents=True)
        python.write_bytes(b"portable-python")
        return python

    def fake_verify_python(_python: Path, _root: Path) -> None:
        event_order.append("verify-python")

    def fake_export_requirements(_uv: Path, _project: Path, output: Path) -> None:
        event_order.append("export")
        output.write_text("fixture==1 --hash=sha256:00\n", encoding="utf-8")

    def fake_build_wheels(
        _uv: Path,
        _python: Path,
        _requirements: Path,
        output: Path,
        _runtime_root: Path,
        _workspace: Path,
        _patchelf: Path | None,
        _strip: Path | None,
        _vcpkg_path: Path | None,
    ) -> None:
        event_order.append("build-wheels")
        (output / "fixture.whl").write_bytes(b"wheel")

    def fake_verify_wheelhouse(
        _uv: Path,
        _python: Path,
        _project: Path,
        _wheelhouse: Path,
        _workspace: Path,
        _runtime_library_path: Path | None,
    ) -> list[dict[str, str]]:
        event_order.append("verify-wheelhouse")
        late_bytecode.parent.mkdir(parents=True)
        late_bytecode.write_bytes(b"late-bytecode")
        return []

    monkeypatch.setattr(sys, "platform", "darwin")
    monkeypatch.setattr(release_runtime, "install_python", fake_install_python)
    monkeypatch.setattr(release_runtime, "verify_python", fake_verify_python)
    monkeypatch.setattr(
        release_runtime,
        "export_requirements",
        fake_export_requirements,
    )
    monkeypatch.setattr(release_runtime, "build_wheels", fake_build_wheels)
    monkeypatch.setattr(
        release_runtime,
        "verify_wheelhouse",
        fake_verify_wheelhouse,
    )

    with pytest.raises(ValueError, match="contains a bytecode artifact"):
        build_release_runtime(
            tmp_path / "project",
            Path("uv"),
            runtime,
            wheelhouse,
        )

    assert event_order == [
        "install",
        "verify-python",
        "export",
        "build-wheels",
        "verify-wheelhouse",
    ]
    assert late_bytecode.read_bytes() == b"late-bytecode"


def test_install_python_inventories_the_requested_uv_installation(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    workspace = tmp_path / "workspace"
    workspace.mkdir()
    output = tmp_path / "runtime"
    commands: list[list[str]] = []
    selected_platforms: list[str] = []
    inventory_calls: list[tuple[Path, str, str]] = []
    normalization_calls: list[tuple[str, Path, Path | None]] = []

    def fake_run(
        arguments: Sequence[str | Path],
        *,
        environment: Mapping[str, str] | None = None,
        capture: bool = False,
    ) -> str:
        command = [str(argument) for argument in arguments]
        commands.append(command)
        assert "install" in command
        assert environment is None
        assert not capture
        install_root = Path(command[command.index("--install-dir") + 1])
        installed = install_root / "sentinel-qualified-install-request"
        executable = (
            installed / "python.exe"
            if os.name == "nt"
            else installed / "bin/python3.14"
        )
        executable.parent.mkdir(parents=True)
        executable.write_bytes(b"fixture")
        return ""

    monkeypatch.setattr(release_runtime, "run", fake_run)

    def fake_python_install_request(platform_name: str) -> str:
        selected_platforms.append(platform_name)
        return "sentinel-qualified-install-request"

    monkeypatch.setattr(
        release_runtime,
        "python_install_request",
        fake_python_install_request,
    )

    def fake_managed_python_install_prefix(
        install_root: Path,
        install_request: str,
        *,
        platform_name: str,
    ) -> Path:
        inventory_calls.append((install_root, install_request, platform_name))
        return install_root / install_request

    monkeypatch.setattr(
        release_runtime,
        "managed_python_install_prefix",
        fake_managed_python_install_prefix,
    )

    def record_bytecode_normalization(root: Path) -> None:
        assert root.is_dir()
        normalization_calls.append(("bytecode", root, None))

    monkeypatch.setattr(
        release_runtime,
        "normalize_python_bytecode",
        record_bytecode_normalization,
    )

    def record_sysconfig_normalization(root: Path, prefix: Path) -> None:
        assert root.is_dir()
        normalization_calls.append(("sysconfig", root, prefix))

    monkeypatch.setattr(
        release_runtime,
        "normalize_python_sysconfig",
        record_sysconfig_normalization,
    )

    installed = install_python(Path("uv"), output, workspace)

    assert installed == python_executable(output)
    assert len(commands) == 1
    command = commands[0]
    assert "find" not in command
    assert LINUX_PYTHON_INSTALL_REQUEST == "cpython-3.14.3-linux-x86_64-gnu"
    assert WINDOWS_PYTHON_INSTALL_REQUEST == "cpython-3.14.3-windows-x86_64-none"
    assert release_runtime.PYTHON_VERSION == "3.14.3"
    assert selected_platforms == [sys.platform]
    assert inventory_calls == [
        (
            workspace / "python-installs",
            "sentinel-qualified-install-request",
            sys.platform,
        )
    ]
    assert normalization_calls == [
        ("bytecode", output, None),
        (
            "sysconfig",
            output,
            workspace / "python-installs/sentinel-qualified-install-request",
        ),
    ]
    assert command[:5] == [
        "uv",
        "--no-config",
        "python",
        "install",
        "sentinel-qualified-install-request",
    ]
    assert command.count("sentinel-qualified-install-request") == 1
    assert release_runtime.PYTHON_VERSION not in command


@pytest.mark.parametrize(
    ("platform_name", "redirect_kind"),
    [("linux", "symlink"), ("win32", "junction")],
)
def test_managed_python_install_prefix_accepts_exact_platform_layout(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    platform_name: str,
    redirect_kind: str,
) -> None:
    install_root = tmp_path / "python-installs"
    full_install, minor_redirect, full_name = create_managed_python_layout(
        install_root, platform_name
    )
    emulate_redirects(
        monkeypatch,
        {minor_redirect: (redirect_kind, full_install)},
    )

    assert managed_python_install_prefix(
        install_root,
        full_name,
        platform_name=platform_name,
    ) == full_install.resolve(strict=True)


@pytest.mark.parametrize(
    "inventory_change", ["missing", "minor-missing", "extra", "first-fallback"]
)
def test_managed_python_install_prefix_rejects_nonexact_inventory(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    inventory_change: str,
) -> None:
    install_root = tmp_path / "python-installs"
    full_install, minor_redirect, full_name = create_managed_python_layout(
        install_root, "linux"
    )
    redirect_target = full_install
    if inventory_change == "missing":
        (install_root / ".lock").unlink()
    elif inventory_change == "minor-missing":
        minor_redirect.rmdir()
    elif inventory_change == "extra":
        (install_root / "extra").mkdir()
    elif inventory_change == "first-fallback":
        full_install.rmdir()
        redirect_target = install_root / "aaa-first-directory"
        redirect_target.mkdir()
    else:
        raise AssertionError(inventory_change)
    emulate_redirects(
        monkeypatch,
        {minor_redirect: ("symlink", redirect_target)},
    )

    with pytest.raises(ValueError, match="inventory differs"):
        managed_python_install_prefix(
            install_root,
            full_name,
            platform_name="linux",
        )


@pytest.mark.parametrize(
    ("support_name", "redirect_kind"),
    [(".gitignore", "symlink"), (".lock", "junction")],
)
def test_managed_python_install_prefix_rejects_redirected_support_file(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    support_name: str,
    redirect_kind: str,
) -> None:
    install_root = tmp_path / "python-installs"
    full_install, minor_redirect, full_name = create_managed_python_layout(
        install_root, "linux"
    )
    support = install_root / support_name
    outside_support = tmp_path / f"outside-{support_name.removeprefix('.')}"
    outside_support.write_bytes(support.read_bytes())
    emulate_redirects(
        monkeypatch,
        {
            support: (redirect_kind, outside_support),
            minor_redirect: ("symlink", full_install),
        },
    )

    with pytest.raises(ValueError, match="must be one real"):
        managed_python_install_prefix(
            install_root,
            full_name,
            platform_name="linux",
        )


@pytest.mark.parametrize("support_name", [".gitignore", ".lock"])
def test_managed_python_install_prefix_rejects_nonregular_support_file(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    support_name: str,
) -> None:
    install_root = tmp_path / "python-installs"
    full_install, minor_redirect, full_name = create_managed_python_layout(
        install_root, "linux"
    )
    support = install_root / support_name
    support.unlink()
    support.mkdir()
    emulate_redirects(
        monkeypatch,
        {minor_redirect: ("symlink", full_install)},
    )

    with pytest.raises(ValueError, match="must be one real"):
        managed_python_install_prefix(
            install_root,
            full_name,
            platform_name="linux",
        )


@pytest.mark.parametrize(
    ("support_name", "content"),
    [(".gitignore", b"*\n"), (".lock", b"locked")],
)
def test_managed_python_install_prefix_rejects_support_file_content(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    support_name: str,
    content: bytes,
) -> None:
    install_root = tmp_path / "python-installs"
    full_install, minor_redirect, full_name = create_managed_python_layout(
        install_root, "linux"
    )
    (install_root / support_name).write_bytes(content)
    emulate_redirects(
        monkeypatch,
        {minor_redirect: ("symlink", full_install)},
    )

    with pytest.raises(ValueError, match="must be one real"):
        managed_python_install_prefix(
            install_root,
            full_name,
            platform_name="linux",
        )


@pytest.mark.parametrize("temporary_change", ["file", "residue", "redirect"])
def test_managed_python_install_prefix_rejects_nonempty_or_redirected_temp(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    temporary_change: str,
) -> None:
    install_root = tmp_path / "python-installs"
    full_install, minor_redirect, full_name = create_managed_python_layout(
        install_root, "linux"
    )
    temporary = install_root / ".temp"
    redirects: dict[Path, tuple[str, Path]] = {
        minor_redirect: ("symlink", full_install)
    }
    if temporary_change == "file":
        temporary.rmdir()
        temporary.write_bytes(b"")
    elif temporary_change == "residue":
        (temporary / "partial-download").mkdir()
    elif temporary_change == "redirect":
        outside_temp = tmp_path / "outside-temp"
        outside_temp.mkdir()
        redirects[temporary] = ("junction", outside_temp)
    else:
        raise AssertionError(temporary_change)
    emulate_redirects(monkeypatch, redirects)

    with pytest.raises(ValueError, match=r"\.temp must be one real empty directory"):
        managed_python_install_prefix(
            install_root,
            full_name,
            platform_name="linux",
        )


@pytest.mark.parametrize("redirect_kind", ["symlink", "junction"])
def test_managed_python_install_prefix_rejects_redirected_full_install(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    redirect_kind: str,
) -> None:
    install_root = tmp_path / "python-installs"
    full_install, minor_redirect, full_name = create_managed_python_layout(
        install_root, "linux"
    )
    outside_install = tmp_path / "outside-install"
    outside_install.mkdir()
    emulate_redirects(
        monkeypatch,
        {
            full_install: (redirect_kind, outside_install),
            minor_redirect: ("symlink", outside_install),
        },
    )

    with pytest.raises(ValueError, match="full installation is redirected"):
        managed_python_install_prefix(
            install_root,
            full_name,
            platform_name="linux",
        )


@pytest.mark.parametrize(
    ("platform_name", "wrong_kind"),
    [("linux", "junction"), ("win32", "symlink")],
)
def test_managed_python_install_prefix_rejects_wrong_minor_redirect_kind(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    platform_name: str,
    wrong_kind: str,
) -> None:
    install_root = tmp_path / "python-installs"
    full_install, minor_redirect, full_name = create_managed_python_layout(
        install_root, platform_name
    )
    emulate_redirects(
        monkeypatch,
        {minor_redirect: (wrong_kind, full_install)},
    )

    with pytest.raises(ValueError, match="wrong platform-specific kind"):
        managed_python_install_prefix(
            install_root,
            full_name,
            platform_name=platform_name,
        )


def test_managed_python_install_prefix_rejects_wrong_minor_redirect_target(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    install_root = tmp_path / "python-installs"
    _full_install, minor_redirect, full_name = create_managed_python_layout(
        install_root, "linux"
    )
    wrong_target = tmp_path / "wrong-target"
    wrong_target.mkdir()
    emulate_redirects(
        monkeypatch,
        {minor_redirect: ("symlink", wrong_target)},
    )

    with pytest.raises(ValueError, match="does not target the full installation"):
        managed_python_install_prefix(
            install_root,
            full_name,
            platform_name="linux",
        )


@pytest.mark.parametrize("redirect_kind", ["symlink", "junction"])
def test_managed_python_install_prefix_rejects_redirected_root(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    redirect_kind: str,
) -> None:
    install_root = tmp_path / "python-installs"
    full_install, minor_redirect, full_name = create_managed_python_layout(
        install_root, "linux"
    )
    emulate_redirects(
        monkeypatch,
        {
            install_root: (redirect_kind, install_root),
            minor_redirect: ("symlink", full_install),
        },
    )

    with pytest.raises(ValueError, match="installation root is redirected"):
        managed_python_install_prefix(
            install_root,
            full_name,
            platform_name="linux",
        )


def test_managed_python_install_prefix_rejects_missing_root(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="installation root is redirected"):
        managed_python_install_prefix(
            tmp_path / "missing",
            LINUX_PYTHON_INSTALL_REQUEST,
            platform_name="linux",
        )


def test_managed_python_install_prefix_normalizes_root_stat_error(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    install_root = tmp_path / "python-installs"
    install_root.mkdir()
    original_stat = Path.stat

    def denied_stat(path: Path, *, follow_symlinks: bool = True) -> os.stat_result:
        if path == install_root:
            raise PermissionError(install_root)
        return original_stat(path, follow_symlinks=follow_symlinks)

    monkeypatch.setattr(Path, "stat", denied_stat)

    with pytest.raises(ValueError, match="installation root is redirected"):
        managed_python_install_prefix(
            install_root,
            LINUX_PYTHON_INSTALL_REQUEST,
            platform_name="linux",
        )


def test_managed_python_install_prefix_rejects_unknown_platform(
    tmp_path: Path,
) -> None:
    with pytest.raises(ValueError, match="does not support"):
        managed_python_install_prefix(
            tmp_path / "python-installs",
            "cpython-3.14.3-unknown-x86_64-none",
            platform_name="unknown",
        )


def test_managed_python_install_prefix_rejects_mismatched_full_request(
    tmp_path: Path,
) -> None:
    install_root = tmp_path / "python-installs"
    create_managed_python_layout(install_root, "linux")

    with pytest.raises(ValueError, match=r"install request .* differs"):
        managed_python_install_prefix(
            install_root,
            WINDOWS_PYTHON_INSTALL_REQUEST,
            platform_name="linux",
        )


def test_verify_python_uses_runtime_version_not_install_request(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    runtime = tmp_path / "runtime"
    qualified_install_request = "sentinel-qualified-install-request"

    def fake_run(
        arguments: Sequence[str | Path],
        *,
        environment: Mapping[str, str] | None = None,
        capture: bool = False,
    ) -> str:
        _ = arguments
        assert environment is None
        assert capture
        return json.dumps(
            {
                "version": qualified_install_request,
                "prefix": str(runtime),
                "config_prefix": str(runtime),
                "include": str(runtime / "include"),
            }
        )

    def fake_python_install_request(_platform_name: str) -> str:
        return qualified_install_request

    monkeypatch.setattr(release_runtime, "run", fake_run)
    monkeypatch.setattr(
        release_runtime,
        "python_install_request",
        fake_python_install_request,
    )

    with pytest.raises(ValueError, match=r"differs from 3\.14\.3"):
        verify_python(runtime / "bin/python3.14", runtime)


def test_python_execution_copy_preserves_the_raw_portable_executable(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    runtime = tmp_path / "runtime"
    raw_python = runtime / "bin/python3.14"
    raw_python.parent.mkdir(parents=True)
    raw_python.write_bytes(b"portable-python")
    workspace = tmp_path / "workspace"
    workspace.mkdir()
    patchelf = tmp_path / "patchelf"
    patchelf.write_bytes(b"fixture")
    loader = tmp_path / "release/lib/ld-linux-x86-64.so.2"
    loader.parent.mkdir(parents=True)
    loader.write_bytes(b"fixture")
    glibc = tmp_path / "release/lib"
    libgcc = tmp_path / "gcc/lib"
    libgcc.mkdir(parents=True)
    metadata: dict[Path, tuple[str, str]] = {}

    def fake_run(
        arguments: Sequence[str | Path],
        *,
        environment: Mapping[str, str] | None = None,
        capture: bool = False,
    ) -> str:
        _ = environment
        command = [str(argument) for argument in arguments]
        executable = Path(command[-1])
        current = metadata.get(
            executable,
            (PORTABLE_SYSTEM_INTERPRETER, PORTABLE_PYTHON_RPATH),
        )
        if "--print-interpreter" in command:
            assert capture
            return current[0]
        if "--print-rpath" in command:
            assert capture
            return current[1]
        if "--set-interpreter" in command:
            metadata[executable] = (
                command[command.index("--set-interpreter") + 1],
                current[1],
            )
            return ""
        if "--set-rpath" in command:
            metadata[executable] = (
                current[0],
                command[command.index("--set-rpath") + 1],
            )
            return ""
        raise AssertionError(command)

    def fake_verify_python(_executable: Path, _root: Path) -> None:
        pass

    monkeypatch.setattr(release_runtime, "run", fake_run)
    monkeypatch.setattr(
        release_runtime,
        "verify_python",
        fake_verify_python,
    )

    execution_python, execution_root, raw_digest = prepare_python_execution_copy(
        runtime,
        workspace,
        patchelf,
        loader,
        os.pathsep.join((str(glibc), str(libgcc))),
    )

    assert execution_root == workspace / "python-execution"
    assert execution_python == execution_root / "bin/python3.14"
    assert metadata[execution_python] == (
        str(loader),
        os.pathsep.join((PORTABLE_PYTHON_RPATH, str(glibc), str(libgcc))),
    )
    assert raw_python.read_bytes() == b"portable-python"
    assert raw_digest == release_runtime.sha256_file(raw_python)


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
    build_stage_start = workflow.index("      - name: Build and stage managed runtimes")
    build_stage_end = workflow.index(
        "      - name: Build offline NSIS installer", build_stage_start
    )
    build_stage = workflow[build_stage_start:build_stage_end]
    installer_stage_end = workflow.index(
        "      - name: Record Windows signing inputs", build_stage_end
    )
    installer_stage = workflow[build_stage_end:installer_stage_end]
    error_preference = "$PSNativeCommandUseErrorActionPreference = $true"
    provenance_clear = "$env:POKECON_RESOURCE_PROVENANCE = $null"
    provenance_development = '$env:POKECON_RESOURCE_PROVENANCE = "development"'
    no_bytecode_current_process = '$env:PYTHONDONTWRITEBYTECODE = "1"'
    no_bytecode_later_steps = (
        '"PYTHONDONTWRITEBYTECODE=1" | Out-File -FilePath $env:GITHUB_ENV -Append'
    )
    runtime_build = "python -m scripts.release.build_runtime `"
    worker_build = (
        "cargo build --locked --release --package pokecon --bin pokecon-worker "
        "--features worker-binary"
    )
    stage_capture = "$stageJson = python -m scripts.release.stage `"
    stage_command = (
        "          $stageJson = python -m scripts.release.stage `\n"
        "            --web web/dist `\n"
        "            --worker target/release/pokecon-worker.exe `\n"
        "            --uv $uv `\n"
        "            --wheelhouse $releaseWheelhouse `\n"
        "            --python $releasePython `\n"
        "            --output $bundleResources `\n"
        "            --config-output $bundleConfig\n"
    )
    stage_parse = "$stageReport = $stageJson | ConvertFrom-Json -ErrorAction Stop"
    content_assignment = "$contentSha256 = $stageReport.content_sha256"
    content_validation = (
        r"if ($contentSha256 -isnot [string] -or "
        r"$contentSha256 -cnotmatch '\A[0-9a-f]{64}\z') {"
    )
    invalid_content_diagnostic = (
        'throw "release stage content_sha256 must be an exact lowercase SHA-256 digest"'
    )
    provenance_export = (
        '"POKECON_RESOURCE_PROVENANCE=packaged:$contentSha256" | '
        "Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append"
    )
    bundle_config_export = (
        '"POKECON_BUNDLE_CONFIG=$bundleConfig" | '
        "Out-File -FilePath $env:GITHUB_ENV -Append"
    )
    bundle_config_consumer = "--config $env:POKECON_BUNDLE_CONFIG"
    canonical_stage_identity_block = (
        stage_command + f"          {stage_parse}\n"
        f"          {content_assignment}\n"
        f"          {content_validation}\n"
        f"            {invalid_content_diagnostic}\n"
        "          }\n"
        f"          {provenance_export}\n"
        f"          {bundle_config_export}\n"
    )
    next_installer_step = "      - name: Build offline NSIS installer"
    canonical_installer_stage = (
        "      - name: Build offline NSIS installer\n"
        "        shell: pwsh\n"
        "        working-directory: rust/pokecon\n"
        "        run: >-\n"
        "          cargo tauri build --ci --bundles nsis\n"
        "          --config $env:POKECON_BUNDLE_CONFIG\n"
        "          -- --locked\n"
    )

    assert build_stage.count(error_preference) == 1
    assert build_stage.count(runtime_build) == 1
    assert build_stage.count(worker_build) == 1
    assert "scripts/release/stage.py" not in workflow
    assert "stage.py" not in workflow.casefold()
    assert build_stage.count(stage_capture) == 1
    assert build_stage.count(stage_command) == 1
    assert build_stage.count(canonical_stage_identity_block) == 1
    if workflow_name == "package.yml":
        assert workflow.count(provenance_clear) == 4
        assert workflow.count(provenance_development) == 2
        assert workflow.count(no_bytecode_current_process) == 2
        assert workflow.count(no_bytecode_later_steps) == 2
        assert workflow.count("scripts.release.stage") == 2
        for identity_statement in (
            stage_parse,
            content_assignment,
            content_validation,
            invalid_content_diagnostic,
            provenance_export,
        ):
            assert workflow.count(identity_statement) == 2
        assert workflow.count(bundle_config_export) == 2
        assert workflow.count("POKECON_RESOURCE_PROVENANCE") == 8
        assert workflow.count("POKECON_BUNDLE_CONFIG") == 4
        bundle_config_lines = tuple(
            line.strip()
            for line in workflow.splitlines()
            if "POKECON_BUNDLE_CONFIG" in line
        )
        assert bundle_config_lines == (
            bundle_config_export,
            bundle_config_consumer,
            bundle_config_export,
            bundle_config_consumer,
        )
        assert workflow.count(bundle_config_consumer) == 2
    else:
        assert workflow.count(provenance_clear) == 2
        assert workflow.count(provenance_development) == 1
        assert workflow.count(no_bytecode_current_process) == 1
        assert workflow.count(no_bytecode_later_steps) == 1
        assert workflow.count("scripts.release.stage") == 1
        for identity_statement in (
            stage_parse,
            content_assignment,
            content_validation,
            invalid_content_diagnostic,
            provenance_export,
        ):
            assert workflow.count(identity_statement) == 1
        assert workflow.count(bundle_config_export) == 1
        assert workflow.count("POKECON_RESOURCE_PROVENANCE") == 4
        assert workflow.count("POKECON_BUNDLE_CONFIG") == 3
        bundle_config_lines = tuple(
            line.strip()
            for line in workflow.splitlines()
            if "POKECON_BUNDLE_CONFIG" in line
        )
        assert bundle_config_lines == (
            bundle_config_export,
            bundle_config_consumer,
            bundle_config_consumer,
        )
        assert workflow.count(bundle_config_consumer) == 2
    provenance_lines = tuple(
        line.strip()
        for line in build_stage.splitlines()
        if "POKECON_RESOURCE_PROVENANCE" in line
    )
    assert provenance_lines == (
        provenance_clear,
        provenance_development,
        provenance_clear,
        provenance_export,
    )
    assert tuple(
        line.strip()
        for line in build_stage.splitlines()
        if line.strip().startswith("$stageJson =")
    ) == (stage_capture,)
    assert tuple(
        line.strip()
        for line in build_stage.splitlines()
        if line.strip().startswith("$stageReport =")
    ) == (stage_parse,)
    assert tuple(
        line.strip()
        for line in build_stage.splitlines()
        if line.strip().startswith("$contentSha256 =")
    ) == (content_assignment,)
    assert "$env:PATH" not in build_stage
    assert ".Substring(" not in build_stage
    assert (
        f"{error_preference}\n"
        f"          {provenance_clear}\n"
        f"          {no_bytecode_current_process}\n"
        f"          {no_bytecode_later_steps}\n" in build_stage
    )
    if workflow_name == "package.yml":
        assert installer_stage == canonical_installer_stage
    else:
        assert "Preserve first Windows package build" in installer_stage
        assert "Rebuild Windows package from identical inputs" in installer_stage
        assert "Verify byte-for-byte Windows NSIS reproducibility" in installer_stage
        assert installer_stage.count("cargo tauri build --ci --bundles nsis") == 2
        assert installer_stage.count(bundle_config_consumer) == 2
        assert "Get-FileHash" in installer_stage
        assert ".Length" in installer_stage
        assert "ReadAllBytes" in installer_stage
        assert "fc.exe" in installer_stage
        assert "fc.exe /b" in installer_stage
        assert (
            installer_stage.index(canonical_installer_stage.strip())
            < installer_stage.index("Preserve first Windows package build")
            < installer_stage.index("Rebuild Windows package from identical inputs")
            < installer_stage.index("Verify byte-for-byte Windows NSIS reproducibility")
        )
        assert installer_stage.count(canonical_installer_stage) == 1
    assert "--features" not in installer_stage
    post_bundle_config_export = build_stage.split(bundle_config_export, maxsplit=1)[1]
    assert not post_bundle_config_export.strip()
    ordered_statements = (
        error_preference,
        provenance_clear,
        runtime_build,
        provenance_development,
        worker_build,
        stage_capture,
        stage_parse,
        content_assignment,
        content_validation,
        invalid_content_diagnostic,
        provenance_export,
        bundle_config_export,
        next_installer_step,
    )
    statement_positions = tuple(
        workflow.index(statement, build_stage_start) for statement in ordered_statements
    )
    assert statement_positions == tuple(sorted(statement_positions))
    assert statement_positions[-1] == build_stage_end
    assert (
        build_stage.index(no_bytecode_current_process)
        < build_stage.index(no_bytecode_later_steps)
        < build_stage.index(runtime_build)
        < build_stage.index("$env:PYO3_PYTHON = $runtimePython")
        < build_stage.index(provenance_development)
        < build_stage.index(worker_build)
        < build_stage.index(provenance_clear, build_stage.index(worker_build))
        < build_stage.index(stage_capture)
    )


def test_package_ci_proves_windows_nsis_reproducibility() -> None:
    root = Path(__file__).resolve().parents[2]
    workflow = (root / ".github/workflows/package.yml").read_text(encoding="utf-8")
    repro_start = workflow.index("  windows_repro:")
    check_start = workflow.index("  windows_repro_check:", repro_start)
    required_start = workflow.index("  required:", check_start)
    repro = workflow[repro_start:check_start]
    check = workflow[check_start:required_start]

    assert "toolchain: '1.95.0'" in repro
    assert '"uv==0.11.8"' in repro
    assert "tauri-cli --version 2.11.4" in repro
    assert '"SOURCE_DATE_EPOCH=0"' in repro
    assert repro.count("cargo tauri build --ci --bundles nsis") == 1
    assert check.count("find -P") == 2
    assert "expected exactly one primary and one reproduction NSIS bundle" in check
    assert "sha256sum --" in check
    assert "cmp --" in check
    assert repro_start < check_start < required_start


def test_nix_release_task_isolates_reproducible_target_native_abi() -> None:
    root = Path(__file__).resolve().parents[2]
    flake = (root / "flake.nix").read_text(encoding="utf-8")
    runtime_output_hash = (
        'outputHash = "sha256-/oX5m7mZkIwXl383NXN4qc2qGnfsJSlPMgVKMrurSmY=";'
    )

    assert flake.count(runtime_output_hash) == 1
    assert "lib.fakeHash" not in flake
    assert 'linux-release-nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.05";' in flake
    assert 'linuxReleaseMaximumGlibc = "2.39";' in flake
    assert "lib.versionAtLeast linuxReleaseMaximumGlibc releaseGlibcVersion" in flake
    assert (
        "portableUvExecutionLoader = linuxReleasePkgs.stdenv.cc.bintools.dynamicLinker;"
    ) in flake
    assert (
        "portableUvExecutionLibraryPath = linuxReleasePkgs.lib.makeLibraryPath" in flake
    )
    assert "linuxReleasePkgs.glibc" in flake
    assert "linuxReleaseCc.cc.lib" in flake
    assert "linuxReleasePkgs.zlib" in flake
    assert "linuxReleasePkgs.xorg.libxcb" in flake
    assert "linuxReleasePkgs.libglvnd" in flake
    assert "linuxReleasePkgs.glib.out" in flake
    assert "linuxReleasePkgs.xorg.libSM" in flake
    assert "linuxReleasePkgs.xorg.libXext" in flake
    assert "linuxReleasePkgs.xorg.libXrender" in flake
    assert 'portableUvFileSha256 = "646adf5c' in flake
    assert 'portableUvSystemInterpreter = "/lib64/ld-linux-x86-64.so.2";' in flake
    assert 'portableUvVersionOutput = "uv 0.11.8 (x86_64-unknown-linux-gnu)";' in flake
    assert '"libgcc_s.so.1"' in flake
    assert '"${linuxReleasePkgs.patchelf}/bin/patchelf" --print-needed' in flake
    assert '--set-interpreter "${portableUvExecutionLoader}"' in flake
    assert '--set-rpath "${portableUvExecutionLibraryPath}"' in flake
    assert (
        'if ! actual_portable_uv_version_output="$("$out/bin/uv" --version)"; then'
        in flake
    )
    assert (
        'if [ "$actual_portable_uv_version_output" != "${portableUvVersionOutput}" ]; then'
        in flake
    )
    assert (
        "portable uv execution copy version probe failed; actual output: "
        "$actual_portable_uv_version_output"
    ) in flake
    assert (
        "portable uv execution copy reports an unexpected version; expected: "
        "${portableUvVersionOutput}; actual: $actual_portable_uv_version_output"
    ) in flake
    assert '"${pkgs.coreutils}/bin/env" -i' in flake
    assert 'CC="${linuxReleaseCc}/bin/gcc"' in flake
    assert 'CFLAGS="-I${linuxReleasePortaudio}/include"' in flake
    assert 'LDFLAGS="-L${linuxReleasePortaudio}/lib"' in flake
    assert 'PKG_CONFIG_PATH="${linuxReleasePortaudio}/lib/pkgconfig"' in flake
    assert 'PKG_CONFIG_LIBDIR="${linuxReleasePortaudio}/lib/pkgconfig"' in flake
    assert (
        "          linuxReleaseRuntimeLibraries = linuxReleasePkgs.symlinkJoin {\n"
        '            name = "pokecon-linux-release-runtime-libraries";\n'
        "            paths = [\n"
        "              linuxReleasePortaudio\n"
        "              linuxReleaseCc.cc.lib\n"
        "              linuxReleasePkgs.zlib\n"
        "              linuxReleasePkgs.xorg.libxcb\n"
        "              linuxReleasePkgs.libglvnd\n"
        "              linuxReleasePkgs.glib.out\n"
        "              linuxReleasePkgs.xorg.libSM\n"
        "              linuxReleasePkgs.xorg.libXext\n"
        "              linuxReleasePkgs.xorg.libXrender\n"
        "            ];\n"
        "          };\n" in flake
    )
    assert flake.count("linuxReleaseRuntimeLibraries") == 4
    assert "[build_ecodes]" in flake
    assert "${linuxReleasePkgs.linuxHeaders}/include/linux/input.h" in flake
    assert "linuxReleaseRuntime =" in flake
    assert 'if system == "x86_64-linux" then' in flake
    assert (
        'cp "${linuxReleaseEvdevConfig}" "$TMPDIR/release-home/.pydistutils.cfg"'
        in flake
    )
    assert 'HOME="$TMPDIR/release-home"' in flake
    assert "PIP_CONFIG_FILE=/dev/null" in flake
    linux_python_libc = LINUX_PYTHON_INSTALL_REQUEST.rsplit("-", 1)[1]
    assert linux_python_libc == "gnu"
    assert flake.count(f"                    UV_LIBC={linux_python_libc} \\\n") == 1
    assert flake.count("UV_LIBC") == 1
    assert "UV_NO_CONFIG=1" in flake
    assert "PYTHONDONTWRITEBYTECODE=1" in flake
    assert "PYTHONNOUSERSITE=1" in flake
    assert "PYTHONSAFEPATH=1" in flake
    assert (
        '"${pythonEnv}/bin/python" -I "${repositorySource}/scripts/release/build_runtime.py"'
        in flake
    )
    assert '--project "${controlledCargoSource}"' in flake
    assert flake.count('--uv "${portableUvExecutionBinary}"') == 1
    assert flake.count('--uv "${portableUvBinary}"') == 1
    assert flake.count('POKECON_BUILD_UV_PATH="${portableUvBinary}"') == 1
    assert '--execution-loader "${portableUvExecutionLoader}"' in flake
    assert '--execution-library-path "${portableUvExecutionLibraryPath}"' in flake
    assert '--runtime-output "$out/python"' in flake
    assert '--wheelhouse-output "$out/wheelhouse"' in flake
    assert "fixed release runtime contains Python bytecode cache artifacts" in flake
    assert 'release_python="${linuxReleaseRuntime}/python"' in flake
    assert 'release_wheelhouse="${linuxReleaseRuntime}/wheelhouse"' in flake
    assert 'release_build_home="$gate_home/release-home"' not in flake
    assert '[ "$2" = deb ]' in flake
    assert "bundle_args=(--bundles deb)" in flake
    assert "appimage" not in flake.casefold()
    assert "rpm" not in flake.casefold()
    assert (
        flake.count('--runtime-library-path "${linuxReleaseRuntimeLibraries}/lib"') == 2
    )
    assert '--runtime-library-path "${linuxReleasePortaudio}/lib"' not in flake
    assert flake.count("unset LD_LIBRARY_PATH") == 1
    assert "LD_LIBRARY_PATH=" not in flake
    assert 'CFLAGS="-I${linuxReleaseRuntimeLibraries}' not in flake
    assert 'LDFLAGS="-L${linuxReleaseRuntimeLibraries}' not in flake
    assert 'CFLAGS="-I${pkgs.portaudio}/include' not in flake
    assert 'LDFLAGS="-L${pkgs.portaudio}/lib' not in flake
    assert 'cp -a "${repositorySource}/." "$workdir/"' in flake
    assert 'release_workdir="$gate_home/pokecon-release-workdir"' in flake
    assert 'export CFLAGS="-ffile-prefix-map=$workdir=/build/pokecon' in flake
    assert 'export CXXFLAGS="-ffile-prefix-map=$workdir=/build/pokecon' in flake
    assert 'export POKECON_RUST_REMAP_SOURCE="${controlledCargoSource}"' in flake
    assert 'export POKECON_RUST_REMAP_PYTHON="$release_python"' in flake
    assert 'export RUSTC_WRAPPER="${reproducibleRustcWrapper}"' in flake
    assert "--remap-path-prefix=${pokeconProductSource}=/build/pokecon" in flake
    assert "--remap-path-prefix=${controlledCargoSource}=/build/pokecon" in flake
    assert "--remap-path-prefix=$POKECON_RUST_REMAP_SOURCE=/build/pokecon" in flake
    assert "--remap-path-prefix=$POKECON_RUST_REMAP_PYTHON=/build/python" in flake
    assert "-Lnative=$POKECON_RUST_REMAP_PYTHON/lib" in flake
    assert '--application "$normalized_application"' in flake
    assert '--worker "$normalized_worker"' in flake
    assert 'cp -p "$application_backup" "$application"' in flake
