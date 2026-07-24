from __future__ import annotations

from typing import TYPE_CHECKING

from scripts.quality.source_filter import check_source_filter

if TYPE_CHECKING:
    from pathlib import Path


def test_source_filter_accepts_a_covered_extension(tmp_path: Path) -> None:
    (tmp_path / "flake.nix").write_text(
        'pkgs.lib.hasSuffix ".rs" path',
        encoding="utf-8",
    )
    source = tmp_path / "rust/example/src/lib.rs"
    source.parent.mkdir(parents=True)
    source.touch()

    report = check_source_filter(tmp_path, ["rust"])
    assert not report.missing_extensions


def test_source_filter_rejects_an_uncovered_extension(tmp_path: Path) -> None:
    (tmp_path / "flake.nix").write_text(
        'pkgs.lib.hasSuffix ".rs" path',
        encoding="utf-8",
    )
    source = tmp_path / "rust/example/src/contract.new"
    source.parent.mkdir(parents=True)
    source.touch()

    report = check_source_filter(tmp_path, ["rust"])
    assert report.missing_extensions == frozenset({"new"})
