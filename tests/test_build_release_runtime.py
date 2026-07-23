from __future__ import annotations

from typing import TYPE_CHECKING

import pytest

from scripts.build_release_runtime import (
    PORTABLE_BUILD_PREFIX,
    normalize_python_sysconfig,
)

if TYPE_CHECKING:
    from pathlib import Path


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
