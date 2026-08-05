from __future__ import annotations

import hashlib
import json
from pathlib import Path

import pytest

from scripts.release.package_smoke import (
    EXPECTED_UDEV_RULES,
    REQUIRED_DEPENDENCIES,
    REQUIRED_WORKER_PACKAGES,
    UDEV_RULE_PATH,
    sha256_file,
    validate_resource_manifest,
    validate_spa,
    validate_udev_support,
    validate_wheelhouse,
)


def write_resource_manifest(root: Path) -> None:
    files = [
        {
            "path": path.relative_to(root).as_posix(),
            "sha256": sha256_file(path),
            "size": path.stat().st_size,
        }
        for path in sorted(
            candidate for candidate in root.rglob("*") if candidate.is_file()
        )
        if path.name != "resource-manifest.json"
    ]
    canonical = json.dumps(
        files, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode()
    (root / "resource-manifest.json").write_text(
        json.dumps(
            {
                "schema_version": 1,
                "layout_version": 1,
                "platform": "unix",
                "files": files,
                "content_sha256": hashlib.sha256(canonical).hexdigest(),
            }
        ),
        encoding="utf-8",
    )


def spa_fixture(root: Path) -> None:
    web = root / "web/dist"
    paths = [
        "_app/immutable/entry/app.fixture.js",
        "_app/immutable/entry/start.fixture.js",
        "_app/immutable/chunks/one.js",
        "_app/immutable/chunks/two.js",
        "_app/immutable/nodes/zero.js",
        "_app/immutable/assets/app.css",
    ]
    for relative in paths:
        path = web / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("fixture\n", encoding="utf-8")
    (web / "index.html").write_text(
        """<!doctype html>
<link href="/_app/immutable/entry/app.fixture.js" rel="modulepreload">
<link href="/_app/immutable/entry/start.fixture.js" rel="modulepreload">
<link href="/_app/immutable/chunks/one.js" rel="modulepreload">
<link href="/_app/immutable/chunks/two.js" rel="modulepreload">
<link href="/_app/immutable/nodes/zero.js" rel="modulepreload">
<link href="/_app/immutable/assets/app.css" rel="stylesheet">
""",
        encoding="utf-8",
    )


def test_resource_manifest_and_spa_are_complete(tmp_path: Path) -> None:
    spa_fixture(tmp_path)
    write_resource_manifest(tmp_path)
    _manifest, file_count = validate_resource_manifest(tmp_path)
    assert file_count == 7
    assert validate_spa(tmp_path) == (5, 1)


def test_debian_dependencies_match_package_auditor() -> None:
    repository = Path(__file__).resolve().parents[2]
    config = json.loads((repository / "rust/pokecon/tauri.conf.json").read_text())
    configured_dependencies = config["bundle"]["linux"]["deb"]["depends"]
    expected_dependencies = {
        "libayatana-appindicator3-1",
        "libgl1",
        "libglib2.0-0",
        "libgtk-3-0",
        "libportaudio2",
        "libsm6",
        "libudev1",
        "libwebkit2gtk-4.1-0",
        "libxcb1",
        "libxext6",
        "libxrender1",
    }

    assert "libxcb1" in configured_dependencies
    assert "libxcb1" in REQUIRED_DEPENDENCIES
    assert "libayatana-appindicator3-1" in configured_dependencies
    assert len(configured_dependencies) == len(expected_dependencies)
    assert set(configured_dependencies) == expected_dependencies
    assert expected_dependencies == REQUIRED_DEPENDENCIES


def test_resource_manifest_rejects_changed_file(tmp_path: Path) -> None:
    spa_fixture(tmp_path)
    write_resource_manifest(tmp_path)
    (tmp_path / "web/dist/index.html").write_text("changed", encoding="utf-8")
    with pytest.raises(ValueError, match="resource manifest mismatch"):
        validate_resource_manifest(tmp_path)


def test_spa_rejects_fallback_error_page(tmp_path: Path) -> None:
    spa_fixture(tmp_path)
    (tmp_path / "web/dist/index.html").write_text(
        "<h1>404</h1><p>Not Found</p>", encoding="utf-8"
    )
    with pytest.raises(ValueError, match="fallback error page"):
        validate_spa(tmp_path)


def test_udev_support_requires_rules_and_reload_scripts(tmp_path: Path) -> None:
    rules = tmp_path / UDEV_RULE_PATH
    rules.parent.mkdir(parents=True)
    rules.write_text("\n".join(EXPECTED_UDEV_RULES), encoding="utf-8")
    control = tmp_path / "DEBIAN"
    control.mkdir()
    for script_name in ("postinst", "postrm"):
        (control / script_name).write_text(
            "#!/bin/sh\nudevadm control --reload-rules\n", encoding="utf-8"
        )

    assert validate_udev_support(tmp_path) == 3


def test_wheelhouse_manifest_covers_fixed_worker_packages(tmp_path: Path) -> None:
    wheelhouse = tmp_path / "python-wheels"
    wheelhouse.mkdir()
    requirements = wheelhouse / "requirements.lock"
    requirements.write_text("pyaudio==0.2.14\n", encoding="utf-8")
    wheels: list[dict[str, object]] = []
    for package in sorted(REQUIRED_WORKER_PACKAGES):
        normalized = package.replace("-", "_")
        filename = (
            "pyaudio-0.2.14-cp314-cp314-linux_x86_64.whl"
            if package == "pyaudio"
            else f"{normalized}-1.0-py3-none-any.whl"
        )
        wheel = wheelhouse / filename
        wheel.write_bytes(package.encode())
        wheels.append(
            {
                "path": filename,
                "sha256": sha256_file(wheel),
                "size": wheel.stat().st_size,
            }
        )
    (wheelhouse / "wheelhouse-manifest.json").write_text(
        json.dumps(
            {
                "schema_version": 1,
                "python_version": "3.14.3",
                "requirements_sha256": sha256_file(requirements),
                "wheels": wheels,
                "inventory": [
                    {"name": package, "version": "1.0"}
                    for package in sorted(REQUIRED_WORKER_PACKAGES)
                ],
            }
        ),
        encoding="utf-8",
    )
    inventory, wheel_paths = validate_wheelhouse(tmp_path)
    assert len(inventory) == len(REQUIRED_WORKER_PACKAGES)
    assert len(wheel_paths) == len(REQUIRED_WORKER_PACKAGES)
