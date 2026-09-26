from __future__ import annotations

import subprocess
from pathlib import Path
from shutil import which

import pytest

from scripts.quality.source_filter import check_source_filter


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


def test_source_filter_ignored_directories_are_root_relative(tmp_path: Path) -> None:
    root = tmp_path / "dist" / "checkout"
    (root / "flake.nix").parent.mkdir(parents=True)
    (root / "flake.nix").write_text(
        'pkgs.lib.hasSuffix ".rs" path',
        encoding="utf-8",
    )
    source = root / "rust/example/src/lib.rs"
    source.parent.mkdir(parents=True)
    source.touch()

    report = check_source_filter(root, ["rust"])
    assert report.source_extensions == frozenset({"rs"})


def test_source_filter_reports_extensionless_non_allowlisted_sources(
    tmp_path: Path,
) -> None:
    (tmp_path / "flake.nix").write_text(
        'pkgs.lib.hasSuffix ".rs" path',
        encoding="utf-8",
    )
    source = tmp_path / "rust/example/NOTICE"
    source.parent.mkdir(parents=True)
    source.touch()

    report = check_source_filter(tmp_path, ["rust"])
    assert report.extensionless_sources == ("rust/example/NOTICE",)


def test_source_filter_fallback_respects_gitignore(tmp_path: Path) -> None:
    (tmp_path / ".gitignore").write_text("rust/generated/\n", encoding="utf-8")
    (tmp_path / "flake.nix").write_text(
        'pkgs.lib.hasSuffix ".rs" path',
        encoding="utf-8",
    )
    generated = tmp_path / "rust/generated/NOTICE"
    generated.parent.mkdir(parents=True)
    generated.touch()
    kept = tmp_path / "rust/NOTICE"
    kept.touch()

    with pytest.raises(RuntimeError, match="requires a Git checkout"):
        check_source_filter(tmp_path, ["rust"])


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
    git = which("git")
    assert git is not None

    def fake_run(
        arguments: list[str],
        **_kwargs: object,
    ) -> subprocess.CompletedProcess[bytes]:
        assert arguments == [
            git,
            "-C",
            str(tmp_path),
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            "web",
        ]
        return subprocess.CompletedProcess(
            arguments,
            returncode=0,
            stdout=b"web/src/app.ts\0",
        )

    monkeypatch.setattr("scripts.quality.source_filter.subprocess.run", fake_run)

    report = check_source_filter(tmp_path, ["web"])
    assert report.source_extensions == frozenset({"ts"})
    assert not report.javascript_sources


def test_repository_gitignore_preserves_canonical_rust_boundary(
    tmp_path: Path,
) -> None:
    root = Path(__file__).resolve().parents[2]
    (tmp_path / ".gitignore").write_text(
        (root / ".gitignore").read_text(encoding="utf-8"),
        encoding="utf-8",
    )
    git = which("git")
    assert git is not None
    subprocess.run(  # noqa: S603 - Git is resolved from the fixed Nix test PATH.
        [git, "-c", "core.excludesFile=/dev/null", "init", "--quiet"],
        cwd=tmp_path,
        check=True,
    )

    expected_ignored = {
        "rust/pokecon/Cargo.toml": False,
        "rust/pokecon/build.rs": False,
        "rust/pokecon/src/lib.rs": False,
        "rust/pokecon/src/future/module.rs": False,
        "rust/pokecon/registry/schema.json": False,
        "rust/pokecon/tests/startup.rs": False,
        "rust/pokecon/tests/future/startup.rs": False,
        "rust/pokecon/benches/runtime.rs": False,
        "rust/pokecon/tauri.conf.json": False,
        "rust/pokecon/icons/icon.png": False,
        "rust/pokecon/linux/70-pokecon-controller.rules": False,
        "rust/pokecon/linux/reload-udev.sh": False,
        "rust/pokecon/permissions/custom/nested.toml": False,
        "rust/pokecon/capabilities/desktop.json": False,
        "rust/pokecon/capabilities/desktop.json5": False,
        ".direnv/generated/config.json": True,
        "rust/pokecon/gen/schemas/capabilities.json": True,
        "rust/pokecon/permissions/autogenerated/example.toml": True,
        "rust/pokecon-app/Cargo.toml": True,
        "rust/pokecon-app/src/main.rs": True,
        "rust/pokecon-desktop/icons/stale.png": True,
    }
    for relative_path in expected_ignored:
        path = tmp_path / relative_path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.touch()

    completed = subprocess.run(  # noqa: S603 - Git is resolved from the fixed Nix test PATH.
        [
            git,
            "-c",
            "core.excludesFile=/dev/null",
            "check-ignore",
            "--no-index",
            "--stdin",
        ],
        cwd=tmp_path,
        input="\n".join(expected_ignored) + "\n",
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    assert completed.returncode == 0, completed.stderr
    ignored_paths = frozenset(completed.stdout.splitlines())
    assert {
        relative_path: relative_path in ignored_paths
        for relative_path in expected_ignored
    } == expected_ignored


def test_repository_generated_direnv_is_excluded_from_source_and_formatting() -> None:
    root = Path(__file__).resolve().parents[2]
    gitignore = (root / ".gitignore").read_text(encoding="utf-8")
    flake = (root / "flake.nix").read_text(encoding="utf-8")
    assert "/.direnv/" in gitignore
    assert "/.direnv/**" in gitignore
    assert 'direnvPath = "${inputs.self.outPath}/.direnv";' in flake
    assert "!isDirenvPath" in flake
    assert '".direnv/**"' in flake


def test_repository_build_sources_are_typescript_only(tmp_path: Path) -> None:
    repository = Path(__file__).resolve().parents[2]
    for config_name in ("eslint.config.ts", "svelte.config.ts", "vite.config.ts"):
        assert (repository / "web" / config_name).is_file()

    (tmp_path / "flake.nix").write_text(
        'pkgs.lib.hasSuffix ".ts" path',
        encoding="utf-8",
    )
    source = tmp_path / "web/src/app.ts"
    source.parent.mkdir(parents=True)
    source.touch()

    report = check_source_filter(tmp_path, ["web"])
    assert not report.javascript_sources
