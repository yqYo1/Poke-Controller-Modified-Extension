"""Repack a Tauri Debian bundle with deterministic archive metadata."""

from __future__ import annotations

import argparse
import os
import re
import stat
import subprocess
import tempfile
from pathlib import Path

REPRODUCIBLE_EPOCH = 0
KIBIBYTE = 1024
MD5SUM_PATTERN = re.compile(r"(?P<digest>[0-9a-f]{32})  (?P<path>.+)")
INSTALLED_SIZE_PATTERN = re.compile(
    r"^Installed-Size:[ \t]*[0-9]+[ \t]*$", re.MULTILINE
)


def normalize_md5sums(root: Path) -> None:
    """Order Debian file checksums by path instead of filesystem traversal order."""
    md5sums = root / "DEBIAN/md5sums"
    if not md5sums.is_file():
        return

    records: list[tuple[str, str]] = []
    for line in md5sums.read_text(encoding="utf-8").splitlines():
        match = MD5SUM_PATTERN.fullmatch(line)
        if match is None:
            message = f"Debian md5sums has an invalid record: {line!r}"
            raise ValueError(message)
        records.append((match.group("path"), line))
    paths = [path for path, _line in records]
    if len(paths) != len(set(paths)):
        message = "Debian md5sums repeats a package path"
        raise ValueError(message)

    ordered = [line for _path, line in sorted(records)]
    md5sums.write_text("".join(f"{line}\n" for line in ordered), encoding="utf-8")


def installed_size_kib(root: Path) -> int:
    """Calculate Installed-Size without filesystem-dependent directory sizes."""
    control_directory = root / "DEBIAN"
    total = 0
    hardlinks: set[tuple[int, int]] = set()
    for path in [root, *sorted(root.rglob("*"))]:
        if path == control_directory or control_directory in path.parents:
            continue
        metadata = path.lstat()
        if stat.S_ISREG(metadata.st_mode) or stat.S_ISLNK(metadata.st_mode):
            identity = (metadata.st_dev, metadata.st_ino)
            if identity not in hardlinks:
                total += (metadata.st_size + KIBIBYTE - 1) // KIBIBYTE
            if metadata.st_nlink > 1:
                hardlinks.add(identity)
        else:
            total += 1
    return total


def normalize_installed_size(root: Path) -> None:
    """Replace Tauri's filesystem-dependent Installed-Size calculation."""
    control = root / "DEBIAN/control"
    if not control.is_file():
        message = "Debian package control file is missing"
        raise ValueError(message)

    contents = control.read_text(encoding="utf-8")
    normalized, replacements = INSTALLED_SIZE_PATTERN.subn(
        f"Installed-Size: {installed_size_kib(root)}", contents
    )
    if replacements != 1:
        message = (
            "Debian package control file must contain exactly one numeric "
            "Installed-Size field"
        )
        raise ValueError(message)
    control.write_text(normalized, encoding="utf-8")


def normalize_timestamps(root: Path) -> None:
    """Set every archive member timestamp to the reproducible epoch."""
    paths = [root, *root.rglob("*")]
    for path in reversed(paths):
        os.utime(
            path,
            ns=(REPRODUCIBLE_EPOCH, REPRODUCIBLE_EPOCH),
            follow_symlinks=False,
        )


def normalize_package(package: Path, dpkg_deb: Path) -> None:
    """Rebuild ``package`` deterministically with the selected dpkg-deb."""
    package = package.resolve(strict=True)
    dpkg_deb = dpkg_deb.resolve(strict=True)
    if package.suffix != ".deb":
        message = f"Debian package has the wrong extension: {package}"
        raise ValueError(message)

    with tempfile.TemporaryDirectory(
        prefix="pokecon-debian-normalize-", dir=package.parent
    ) as directory:
        workspace = Path(directory)
        extracted = workspace / "root"
        rebuilt = workspace / package.name
        subprocess.run(
            [dpkg_deb, "--raw-extract", package, extracted],
            check=True,
        )
        normalize_md5sums(extracted)
        normalize_installed_size(extracted)
        normalize_timestamps(extracted)
        environment = {
            **os.environ,
            "LC_ALL": "C.UTF-8",
            "SOURCE_DATE_EPOCH": str(REPRODUCIBLE_EPOCH),
            "TZ": "UTC",
        }
        subprocess.run(
            [
                dpkg_deb,
                "--root-owner-group",
                "--uniform-compression",
                "--threads-max=1",
                "-Zxz",
                "-z6",
                "--build",
                extracted,
                rebuilt,
            ],
            check=True,
            env=environment,
        )
        os.replace(rebuilt, package)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", type=Path)
    parser.add_argument("--dpkg-deb", required=True, type=Path)
    return parser.parse_args()


def main() -> int:
    arguments = parse_args()
    normalize_package(arguments.package, arguments.dpkg_deb)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
