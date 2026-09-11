from __future__ import annotations

import json
from pathlib import Path
from typing import cast


def load_manifest(path: Path) -> dict[str, object]:
    raw: object = json.loads(path.read_text(encoding="utf-8"))
    assert isinstance(raw, dict)
    return cast("dict[str, object]", raw)


def test_bun_is_the_reproducible_javascript_toolchain() -> None:
    root = Path(__file__).resolve().parents[2]
    web = load_manifest(root / "web/package.json")
    api = load_manifest(root / "api/package.json")
    package_manager = web.get("packageManager")

    assert isinstance(package_manager, str)
    assert package_manager.startswith("bun@")
    assert api.get("packageManager") == package_manager
    for package in ("api", "web"):
        assert (root / package / "bun.lock").is_file()
        assert not (root / package / "package-lock.json").exists()

    raw_scripts = web.get("scripts")
    assert isinstance(raw_scripts, dict)
    scripts = cast("dict[str, object]", raw_scripts)
    assert scripts
    assert all(
        isinstance(command, str) and "bun --bun" in command
        for command in scripts.values()
    )

    flake = (root / "flake.nix").read_text(encoding="utf-8")
    assert "webBunDependencies = pkgs.stdenvNoCC.mkDerivation" in flake
    assert "apiBunDependencies = pkgs.stdenvNoCC.mkDerivation" in flake
    assert "pkgs.bun.version == bunVersion" in flake
    assert "--frozen-lockfile" in flake
    assert "buildNpmPackage" not in flake
    assert "nodejs_" not in flake
    assert "lib.fakeHash" not in flake

    for workflow_name in ("package.yml", "release.yml"):
        workflow = (root / ".github/workflows" / workflow_name).read_text(
            encoding="utf-8"
        )
        assert "oven-sh/setup-bun@v2" in workflow
        assert "bun-version-file: web/package.json" in workflow
        assert "actions/setup-node" not in workflow
        assert "npm " not in workflow

    generator = (root / "scripts/quality/generate-api-types.sh").read_text(
        encoding="utf-8"
    )
    assert "POKECON_API_NODE_MODULES" in generator
    assert "bun install" not in generator
    assert "bun --bun" in generator
    assert "npm " not in generator
