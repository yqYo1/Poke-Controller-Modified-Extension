"""Record and verify the exact distributables selected for package signing."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import stat
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, cast

if TYPE_CHECKING:
    from collections.abc import Iterable
    from typing import BinaryIO, Never


POLICY_RELATIVE_PATH = Path("rust/pokecon/signing-targets.json")
TAURI_ROOT = "rust/pokecon"
PATH_STAT_CTIME_IS_CREATION_TIME = os.name == "nt"


def invalid_value(message: str) -> Never:
    raise ValueError(message)


def strict_object(pairs: Iterable[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            invalid_value(f"strict JSON repeats key {key!r}")
        result[key] = value
    return result


def invalid_json_constant(value: str) -> Never:
    invalid_value(f"strict JSON contains invalid constant {value!r}")


def canonical_json_bytes(value: object) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")


def mapping(value: object, label: str) -> dict[str, object]:
    if not isinstance(value, dict):
        invalid_value(f"{label} must be an object")
    raw = cast("dict[object, object]", value)
    if not all(isinstance(key, str) for key in raw):
        invalid_value(f"{label} keys must be strings")
    return {cast("str", key): item for key, item in raw.items()}


def required_string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        invalid_value(f"{label} must be a non-empty string")
    return value


def exact_keys(value: dict[str, object], expected: set[str], label: str) -> None:
    if set(value) != expected:
        invalid_value(f"{label} has unexpected keys")


@dataclass(frozen=True)
class BundlePolicy:
    format: str
    suffix: str


def bundle_policy(value: object, label: str) -> BundlePolicy:
    policy = mapping(value, label)
    exact_keys(policy, {"format", "suffix"}, label)
    format_name = required_string(policy.get("format"), f"{label}.format")
    suffix = required_string(policy.get("suffix"), f"{label}.suffix")
    if not suffix.startswith(".") and not suffix.startswith("-"):
        invalid_value(f"{label}.suffix must start with '.' or '-'")
    return BundlePolicy(format=format_name, suffix=suffix)


def load_policy(project_root: Path, platform: str) -> tuple[BundlePolicy, str]:
    policy_path = project_root / POLICY_RELATIVE_PATH
    policy = load_strict_json(policy_path, "signing target policy")
    exact_keys(
        policy,
        {"platforms", "schema_version", "tauri_root"},
        "signing target policy",
    )
    schema_version = policy.get("schema_version")
    if type(schema_version) is not int or schema_version != 1:
        invalid_value("signing target policy schema_version must be integer 1")
    if policy.get("tauri_root") != TAURI_ROOT:
        invalid_value(f"signing target policy tauri_root must be {TAURI_ROOT!r}")
    platforms = mapping(policy.get("platforms"), "signing target policy platforms")
    exact_keys(platforms, {"linux", "windows"}, "signing target policy platforms")
    selected = mapping(
        platforms.get(platform), f"signing target policy platform {platform}"
    )
    exact_keys(selected, {"bundle"}, f"signing target policy platform {platform}")
    bundle = bundle_policy(
        selected.get("bundle"), f"signing target policy {platform}.bundle"
    )
    return bundle, hashlib.sha256(canonical_json_bytes(policy)).hexdigest()


def file_identity(metadata: os.stat_result) -> tuple[int, int, int]:
    return metadata.st_dev, metadata.st_ino, stat.S_IFMT(metadata.st_mode)


def file_path_snapshot(
    metadata: os.stat_result,
) -> tuple[int, int, int, int, int, int | None]:
    """Return fields that have matching semantics for path stat and fstat."""
    compatible_ctime = (
        None if PATH_STAT_CTIME_IS_CREATION_TIME else metadata.st_ctime_ns
    )
    return (
        *file_identity(metadata),
        metadata.st_size,
        metadata.st_mtime_ns,
        compatible_ctime,
    )


def file_snapshot(metadata: os.stat_result) -> tuple[int, int, int, int, int, int]:
    return (
        *file_identity(metadata),
        metadata.st_size,
        metadata.st_mtime_ns,
        metadata.st_ctime_ns,
    )


def opened_regular_file(
    path: Path,
    label: str,
    stream: BinaryIO,
) -> os.stat_result:
    try:
        opened = os.fstat(stream.fileno())
        linked = path.stat(follow_symlinks=False)
    except OSError as error:
        message = f"{label} cannot be inspected: {path}"
        raise ValueError(message) from error
    if (
        path.is_symlink()
        or path.is_junction()
        or not stat.S_ISREG(opened.st_mode)
        or not stat.S_ISREG(linked.st_mode)
        or file_path_snapshot(opened) != file_path_snapshot(linked)
    ):
        invalid_value(f"{label} must be one stable real regular file: {path}")
    if opened.st_size <= 0:
        invalid_value(f"{label} must not be empty: {path}")
    return opened


def finish_regular_file(
    path: Path,
    label: str,
    stream: BinaryIO,
    before: os.stat_result,
) -> os.stat_result:
    try:
        after = os.fstat(stream.fileno())
        linked = path.stat(follow_symlinks=False)
    except OSError as error:
        message = f"{label} cannot be revalidated: {path}"
        raise ValueError(message) from error
    if (
        path.is_symlink()
        or path.is_junction()
        or file_snapshot(before) != file_snapshot(after)
        or file_path_snapshot(after) != file_path_snapshot(linked)
    ):
        invalid_value(f"{label} changed while it was read: {path}")
    return after


def read_regular_bytes(path: Path, label: str) -> bytes:
    try:
        with path.open("rb") as stream:
            before = opened_regular_file(path, label, stream)
            contents = stream.read()
            finish_regular_file(path, label, stream, before)
    except OSError as error:
        message = f"{label} cannot be read: {path}"
        raise ValueError(message) from error
    return contents


def sha256_regular_file(path: Path, label: str) -> tuple[str, int]:
    digest = hashlib.sha256()
    try:
        with path.open("rb") as stream:
            before = opened_regular_file(path, label, stream)
            while block := stream.read(64 * 1024):
                digest.update(block)
            after = finish_regular_file(path, label, stream, before)
    except OSError as error:
        message = f"{label} cannot be read: {path}"
        raise ValueError(message) from error
    return digest.hexdigest(), after.st_size


def load_strict_json(path: Path, label: str) -> dict[str, object]:
    raw_bytes = read_regular_bytes(path, label)
    try:
        raw_text = raw_bytes.decode("utf-8")
        raw: object = json.loads(
            raw_text,
            object_pairs_hook=strict_object,
            parse_constant=invalid_json_constant,
        )
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        message = f"{label} is not strict UTF-8 JSON"
        raise ValueError(message) from error
    return mapping(raw, label)


def target_record(path: Path, role: str, format_name: str) -> dict[str, object]:
    digest, size = sha256_regular_file(path, f"{role} signing input")
    return {
        "file_name": path.name,
        "format": format_name,
        "role": role,
        "sha256": digest,
        "size": size,
    }


def signing_inputs(
    project_root: Path,
    platform: str,
    bundle: Path,
) -> dict[str, object]:
    root = project_root.resolve(strict=True)
    tauri_root = root / TAURI_ROOT
    try:
        tauri_metadata = tauri_root.stat(follow_symlinks=False)
    except OSError as error:
        message = f"Tauri root cannot be inspected: {tauri_root}"
        raise ValueError(message) from error
    if (
        tauri_root.is_symlink()
        or tauri_root.is_junction()
        or not stat.S_ISDIR(tauri_metadata.st_mode)
    ):
        invalid_value(f"Tauri root must be one real directory: {tauri_root}")
    policy, policy_sha256 = load_policy(root, platform)
    if not bundle.name.endswith(policy.suffix):
        invalid_value(
            f"{platform} bundle signing input must end with {policy.suffix!r}"
        )

    return {
        "platform": platform,
        "policy": {
            "canonical_sha256": policy_sha256,
            "path": POLICY_RELATIVE_PATH.as_posix(),
        },
        "schema_version": 1,
        "targets": [target_record(bundle, "bundle", policy.format)],
        "tauri_root": TAURI_ROOT,
    }


def verify_signing_inputs(
    manifest_path: Path,
    expected: dict[str, object],
) -> None:
    observed = load_strict_json(manifest_path, "signing input manifest")
    if canonical_json_bytes(observed) != canonical_json_bytes(expected):
        invalid_value(
            f"signing input manifest does not match its current targets: {manifest_path}"
        )


def write_signing_inputs(output: Path, manifest: dict[str, object]) -> None:
    if output.exists() or output.is_symlink() or output.is_junction():
        invalid_value(f"signing input manifest output already exists: {output}")
    parent = output.parent
    try:
        parent_metadata = parent.stat(follow_symlinks=False)
    except OSError as error:
        message = f"signing input manifest parent cannot be inspected: {parent}"
        raise ValueError(message) from error
    if (
        parent.is_symlink()
        or parent.is_junction()
        or not stat.S_ISDIR(parent_metadata.st_mode)
    ):
        invalid_value(
            f"signing input manifest parent must be one real directory: {parent}"
        )
    encoded = (
        json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    ).encode()
    try:
        with output.open("xb") as stream:
            stream.write(encoded)
        output.chmod(0o644)
        os.utime(output, (0, 0))
    except OSError:
        output.unlink(missing_ok=True)
        raise


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=("linux", "windows"), required=True)
    parser.add_argument("--bundle", type=Path, required=True)
    destination = parser.add_mutually_exclusive_group(required=True)
    destination.add_argument("--output", type=Path)
    destination.add_argument("--verify", type=Path, metavar="MANIFEST")
    arguments = parser.parse_args()
    project_root = Path(__file__).resolve().parents[2]
    manifest = signing_inputs(
        project_root,
        cast("str", arguments.platform),
        cast("Path", arguments.bundle),
    )
    if arguments.output is not None:
        write_signing_inputs(cast("Path", arguments.output), manifest)
    else:
        verify_signing_inputs(cast("Path", arguments.verify), manifest)
    print(json.dumps(manifest, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
