from __future__ import annotations

import os
from typing import TYPE_CHECKING

from scripts.normalize_debian_package import REPRODUCIBLE_EPOCH, normalize_timestamps

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
