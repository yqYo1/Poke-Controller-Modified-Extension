from __future__ import annotations

from pathlib import Path

import pytest

from scripts.release.normalize_linux_elf import (
    CANONICAL_BUILD_ROOT,
    normalize_ephemeral_build_root,
)


def test_normalize_ephemeral_build_root_rewrites_every_occurrence(
    tmp_path: Path,
) -> None:
    temporary_root = Path("/") / "tmp"
    build_root = temporary_root / "pokecon-rust-gate-home.Ab12Cd34"
    binary = tmp_path / "pokecon"
    content = (
        b"prefix\0"
        + bytes(build_root)
        + b"/cargo-target/release-python/lib\0middle\0"
        + bytes(build_root)
        + b"/cargo-target/pokecon-release-workdir/rust/pokecon-app\0suffix"
    )
    binary.write_bytes(content)

    assert normalize_ephemeral_build_root(binary, build_root) == 2
    assert binary.read_bytes() == content.replace(
        bytes(build_root), CANONICAL_BUILD_ROOT.encode("ascii")
    )


@pytest.mark.parametrize(
    "build_root",
    [
        Path("/") / "tmp" / "pokecon-rust-gate-home.too-short",
        Path("/") / "tmp" / "unrelated.Ab12Cd34",
        Path("relative/pokecon-rust-gate-home.Ab12Cd34"),
    ],
)
def test_normalize_ephemeral_build_root_rejects_unexpected_roots(
    tmp_path: Path, build_root: Path
) -> None:
    binary = tmp_path / "pokecon"
    binary.write_bytes(b"fixture")

    with pytest.raises(ValueError, match="unexpected isolated release build root"):
        normalize_ephemeral_build_root(binary, build_root)
