from __future__ import annotations

import os
from typing import TYPE_CHECKING

import pytest

from scripts.release.normalize_debian_package import (
    REPRODUCIBLE_EPOCH,
    installed_size_kib,
    normalize_installed_size,
    normalize_md5sums,
    normalize_timestamps,
)

if TYPE_CHECKING:
    from pathlib import Path


def test_normalize_timestamps_covers_files_and_directories(tmp_path: Path) -> None:
    directory = tmp_path / "directory"
    directory.mkdir()
    file = directory / "payload"
    file.write_text("payload", encoding="utf-8")

    normalize_timestamps(tmp_path)

    expected = REPRODUCIBLE_EPOCH
    assert int(os.stat(tmp_path).st_mtime_ns) == expected
    assert int(os.stat(directory).st_mtime_ns) == expected
    assert int(os.stat(file).st_mtime_ns) == expected


def test_normalize_md5sums_orders_records_by_package_path(tmp_path: Path) -> None:
    control = tmp_path / "DEBIAN"
    control.mkdir()
    md5sums = control / "md5sums"
    md5sums.write_text(
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  usr/share/z-last\n"
        "cccccccccccccccccccccccccccccccc  usr/lib/PokeCon Controller/file\n"
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  usr/bin/pokecon\n",
        encoding="utf-8",
    )

    normalize_md5sums(tmp_path)

    assert md5sums.read_text(encoding="utf-8") == (
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  usr/bin/pokecon\n"
        "cccccccccccccccccccccccccccccccc  usr/lib/PokeCon Controller/file\n"
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  usr/share/z-last\n"
    )


def test_normalize_installed_size_uses_policy_units(tmp_path: Path) -> None:
    control_directory = tmp_path / "DEBIAN"
    control_directory.mkdir()
    control = control_directory / "control"
    control.write_text(
        "Package: poke-con-controller\nInstalled-Size: 999999\n",
        encoding="utf-8",
    )
    binary_directory = tmp_path / "usr/bin"
    binary_directory.mkdir(parents=True)
    (binary_directory / "pokecon").write_bytes(b"x" * 1025)
    os.link(binary_directory / "pokecon", binary_directory / "pokecon-hardlink")
    (binary_directory / "empty").write_bytes(b"")
    (binary_directory / "pokecon-link").symlink_to("pokecon")

    assert installed_size_kib(tmp_path) == 6

    normalize_installed_size(tmp_path)

    assert control.read_text(encoding="utf-8") == (
        "Package: poke-con-controller\nInstalled-Size: 6\n"
    )


@pytest.mark.parametrize(
    "installed_size",
    [
        "",
        "Installed-Size: unknown\n",
        "Installed-Size: 1\nInstalled-Size: 2\n",
    ],
)
def test_normalize_installed_size_rejects_invalid_control(
    tmp_path: Path, installed_size: str
) -> None:
    control_directory = tmp_path / "DEBIAN"
    control_directory.mkdir()
    (control_directory / "control").write_text(installed_size, encoding="utf-8")

    with pytest.raises(ValueError, match="exactly one numeric Installed-Size"):
        normalize_installed_size(tmp_path)
