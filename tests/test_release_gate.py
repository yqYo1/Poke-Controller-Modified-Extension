from __future__ import annotations

import hashlib
from pathlib import Path

import pytest

from scripts.release_gate import validate_release, write_checksums


def test_repository_release_versions_and_contracts_match() -> None:
    root = Path(__file__).resolve().parents[1]
    assert validate_release(root, "v0.1.0") == "0.1.0"
    with pytest.raises(ValueError, match="must equal"):
        validate_release(root, "v0.1.1")


def test_checksums_cover_sorted_relative_artifacts(tmp_path: Path) -> None:
    artifacts = tmp_path / "artifacts"
    artifacts.mkdir()
    (artifacts / "z.bin").write_bytes(b"z")
    nested = artifacts / "nested"
    nested.mkdir()
    (nested / "a.bin").write_bytes(b"a")
    output = artifacts / "SHA256SUMS"
    lines = write_checksums(artifacts, output)
    assert lines == [
        f"{hashlib.sha256(b'a').hexdigest()}  nested/a.bin",
        f"{hashlib.sha256(b'z').hexdigest()}  z.bin",
    ]
    assert output.read_text(encoding="utf-8") == "\n".join(lines) + "\n"
