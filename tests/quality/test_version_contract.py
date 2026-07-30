import tomllib
from pathlib import Path
from typing import Any


def _read_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as source:
        return tomllib.load(source)


def test_workspace_is_the_single_version_source() -> None:
    root = Path(__file__).resolve().parents[2]
    cargo = _read_toml(root / "Cargo.toml")
    pyproject = _read_toml(root / "pyproject.toml")

    workspace_version = cargo["workspace"]["package"]["version"]
    assert workspace_version == "0.1.0"
    assert pyproject["project"]["dynamic"] == ["version"]
    assert "version" not in pyproject["project"]


def test_maturin_development_layout_fix_is_isolated() -> None:
    root = Path(__file__).resolve().parents[2]
    pyproject = _read_toml(root / "pyproject.toml")
    maturin = pyproject["tool"]["maturin"]

    assert maturin["python-packages"] == ["python/pokecon"]
    assert "python-source" not in maturin

    flake = (root / "flake.nix").read_text(encoding="utf-8")
    for contract in (
        'maturin_source="$gate_home/maturin-source"',
        "maturin-develop expected exactly one legacy python-packages setting",
        "Maturin wheel payload must live under pokecon/",
        "Maturin wheel pure-Python inventory differs from caller source",
        "maturin venv contains unexpected distributions",
        "maturin venv project RECORD has an unsafe path",
        "site-packages tree contains a symlink",
        'maturin_lock="$maturin_target_root/.maturin-develop.lock"',
        "using isolated per-run Cargo target:",
        'or "\\\\" in member',
        "-v | -vv* | --verbose)",
        "PokeCon task lock changed while it was held",
        "refusing a hook path outside the worktree and Git common directory",
    ):
        assert contract in flake
