from __future__ import annotations

import subprocess
from pathlib import Path
from typing import TYPE_CHECKING

from scripts.quality.source_filter import check_source_filter

if TYPE_CHECKING:
    import pytest


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


def test_source_filter_reports_javascript_build_sources(tmp_path: Path) -> None:
    (tmp_path / "flake.nix").write_text(
        'pkgs.lib.hasSuffix ".js" path',
        encoding="utf-8",
    )
    source = tmp_path / "web/src/app.js"
    source.parent.mkdir(parents=True)
    source.touch()

    report = check_source_filter(tmp_path, ["web"])
    assert not report.missing_extensions
    assert report.javascript_sources == ("web/src/app.js",)


def test_source_filter_uses_git_inventory_in_a_worktree(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    (tmp_path / ".git").touch()
    (tmp_path / "flake.nix").write_text(
        'pkgs.lib.hasSuffix ".ts" path',
        encoding="utf-8",
    )
    source = tmp_path / "web/src/app.ts"
    source.parent.mkdir(parents=True)
    source.touch()
    generated = tmp_path / "web/build/generated.js"
    generated.parent.mkdir(parents=True)
    generated.touch()

    def fake_run(
        arguments: list[str],
        **_kwargs: object,
    ) -> subprocess.CompletedProcess[str]:
        assert "--exclude-standard" in arguments
        return subprocess.CompletedProcess(
            arguments,
            returncode=0,
            stdout="web/src/app.ts\0",
        )

    monkeypatch.setattr("scripts.quality.source_filter.subprocess.run", fake_run)

    report = check_source_filter(tmp_path, ["web"])
    assert report.source_extensions == frozenset({"ts"})
    assert not report.javascript_sources


def test_repository_build_sources_are_typescript_only() -> None:
    root = Path(__file__).resolve().parents[2]
    for config_name in ("eslint.config.ts", "svelte.config.ts", "vite.config.ts"):
        assert (root / "web" / config_name).is_file()
    report = check_source_filter(root)
    assert not report.javascript_sources, ", ".join(report.javascript_sources)
