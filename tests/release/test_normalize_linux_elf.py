from __future__ import annotations

from pathlib import Path

import pytest

from scripts.release import normalize_linux_elf
from scripts.release.normalize_linux_elf import (
    CANONICAL_BUILD_ROOT,
    normalize_ephemeral_build_root,
    normalize_release,
)

PINNED_PYTHON_LIBRARY = "libpython3.14.so.1.0"
type NormalizeCall = tuple[Path, Path, Path, Path, Path, str | None]


@pytest.fixture
def normalize_calls(monkeypatch: pytest.MonkeyPatch) -> list[NormalizeCall]:
    calls: list[NormalizeCall] = []

    def fake_normalize_binary(
        binary: Path,
        patchelf: Path,
        strip: Path,
        objdump: Path,
        build_root: Path,
        *,
        rpath: str | None,
    ) -> dict[str, object]:
        calls.append((binary, patchelf, strip, objdump, build_root, rpath))
        needed: list[str] = [] if rpath is None else [PINNED_PYTHON_LIBRARY]
        return {"path": str(binary), "needed": needed}

    monkeypatch.setattr(
        normalize_linux_elf,
        "normalize_binary",
        fake_normalize_binary,
    )
    return calls


def normalization_tool_paths(tmp_path: Path) -> tuple[Path, Path, Path, Path]:
    return (
        tmp_path / "patchelf",
        tmp_path / "strip",
        tmp_path / "objdump",
        tmp_path / "pokecon-rust-gate-home.Ab12Cd34",
    )


def create_python_root(tmp_path: Path) -> Path:
    python_root = tmp_path / "python"
    python_library = python_root / "lib" / PINNED_PYTHON_LIBRARY
    python_library.parent.mkdir(parents=True)
    python_library.write_bytes(b"portable-python-library")
    return python_root


def test_normalize_release_accepts_application_only(
    tmp_path: Path,
    normalize_calls: list[NormalizeCall],
) -> None:
    application = tmp_path / "pokecon"
    patchelf, strip, objdump, build_root = normalization_tool_paths(tmp_path)

    reports = normalize_release(
        application,
        None,
        None,
        patchelf,
        strip,
        objdump,
        build_root,
    )

    assert reports == [{"path": str(application), "needed": []}]
    assert normalize_calls == [
        (application, patchelf, strip, objdump, build_root, None)
    ]


def test_normalize_release_accepts_worker_only(
    tmp_path: Path,
    normalize_calls: list[NormalizeCall],
) -> None:
    worker = tmp_path / "pokecon-worker"
    python_root = create_python_root(tmp_path)
    patchelf, strip, objdump, build_root = normalization_tool_paths(tmp_path)

    reports = normalize_release(
        None,
        worker,
        python_root,
        patchelf,
        strip,
        objdump,
        build_root,
    )

    assert reports == [{"path": str(worker), "needed": [PINNED_PYTHON_LIBRARY]}]
    assert normalize_calls == [
        (worker, patchelf, strip, objdump, build_root, "$ORIGIN/python/lib")
    ]


def test_normalize_release_preserves_combined_order_and_preflights_worker(
    tmp_path: Path,
    normalize_calls: list[NormalizeCall],
) -> None:
    application = tmp_path / "pokecon"
    worker = tmp_path / "pokecon-worker"
    python_root = create_python_root(tmp_path)
    patchelf, strip, objdump, build_root = normalization_tool_paths(tmp_path)

    reports = normalize_release(
        application,
        worker,
        python_root,
        patchelf,
        strip,
        objdump,
        build_root,
    )

    assert reports == [
        {"path": str(application), "needed": []},
        {"path": str(worker), "needed": [PINNED_PYTHON_LIBRARY]},
    ]
    assert normalize_calls == [
        (application, patchelf, strip, objdump, build_root, None),
        (worker, patchelf, strip, objdump, build_root, "$ORIGIN/python/lib"),
    ]

    normalize_calls.clear()
    invalid_python_root = tmp_path / "invalid-python"
    with pytest.raises(ValueError, match="portable Python shared library is missing"):
        normalize_release(
            application,
            worker,
            invalid_python_root,
            patchelf,
            strip,
            objdump,
            build_root,
        )
    assert normalize_calls == []


def test_normalize_release_rejects_empty_selection_before_normalizing(
    tmp_path: Path,
    normalize_calls: list[NormalizeCall],
) -> None:
    patchelf, strip, objdump, build_root = normalization_tool_paths(tmp_path)

    with pytest.raises(ValueError, match="requires an application or worker"):
        normalize_release(
            None,
            None,
            None,
            patchelf,
            strip,
            objdump,
            build_root,
        )

    assert normalize_calls == []


@pytest.mark.parametrize(
    ("python_root_state", "diagnostic"),
    [
        ("omitted", "portable Python root is required"),
        ("missing-library", "portable Python shared library is missing"),
        ("symlink-library", "portable Python shared library is missing"),
    ],
)
def test_normalize_release_rejects_worker_without_pinned_python_before_normalizing(
    tmp_path: Path,
    normalize_calls: list[NormalizeCall],
    python_root_state: str,
    diagnostic: str,
) -> None:
    worker = tmp_path / "pokecon-worker"
    python_root: Path | None
    if python_root_state == "omitted":
        python_root = None
    else:
        python_root = tmp_path / "python-without-library"
        if python_root_state == "missing-library":
            python_root.mkdir()
        else:
            redirected_library = tmp_path / "redirected-libpython"
            redirected_library.write_bytes(b"redirected-python-library")
            python_library = python_root / "lib" / PINNED_PYTHON_LIBRARY
            python_library.parent.mkdir(parents=True)
            python_library.symlink_to(redirected_library)
    patchelf, strip, objdump, build_root = normalization_tool_paths(tmp_path)

    with pytest.raises(ValueError, match=diagnostic):
        normalize_release(
            None,
            worker,
            python_root,
            patchelf,
            strip,
            objdump,
            build_root,
        )

    assert normalize_calls == []


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
        + b"/cargo-target/pokecon-release-workdir/rust/pokecon\0suffix"
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
