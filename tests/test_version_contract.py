import tomllib
from pathlib import Path
from typing import Any


def _read_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as source:
        return tomllib.load(source)


def test_workspace_is_the_single_version_source() -> None:
    root = Path(__file__).resolve().parents[1]
    cargo = _read_toml(root / "Cargo.toml")
    pyproject = _read_toml(root / "pyproject.toml")

    workspace_version = cargo["workspace"]["package"]["version"]
    assert workspace_version == "0.1.0"
    assert pyproject["project"]["dynamic"] == ["version"]
    assert "version" not in pyproject["project"]
