"""Build the immutable compatibility inventory from exact Git commits."""

from __future__ import annotations

import argparse
import ast
import hashlib
import io
import json
import subprocess
import tempfile
import tokenize
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, cast

if TYPE_CHECKING:
    from collections.abc import Generator, Mapping, Sequence


@dataclass(frozen=True)
class Baseline:
    identifier: str
    repository: str
    commit: str
    script_roots: tuple[str, ...]


def require_mapping(value: object, label: str) -> dict[str, object]:
    if not isinstance(value, dict):
        raise ValueError(f"{label} must be an object with string keys")
    untyped = cast("dict[object, object]", value)
    if not all(isinstance(key, str) for key in untyped):
        raise ValueError(f"{label} must be an object with string keys")
    return {cast("str", key): item for key, item in untyped.items()}


def require_sequence(value: object, label: str) -> list[object]:
    if not isinstance(value, list):
        raise ValueError(f"{label} must be an array")
    return cast("list[object]", value)


def require_string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise ValueError(f"{label} must be a non-empty string")
    return value


def load_baselines(path: Path) -> list[Baseline]:
    raw: object = json.loads(path.read_text(encoding="utf-8"))
    registry = require_mapping(raw, "compatibility registry")
    rows = require_sequence(registry.get("fixed_baselines"), "fixed_baselines")
    baselines: list[Baseline] = []
    for index, raw_row in enumerate(rows):
        row = require_mapping(raw_row, f"fixed_baselines[{index}]")
        roots = tuple(
            require_string(root, f"fixed_baselines[{index}].script_roots")
            for root in require_sequence(
                row.get("script_roots"), f"fixed_baselines[{index}].script_roots"
            )
        )
        baselines.append(
            Baseline(
                identifier=require_string(
                    row.get("id"), f"fixed_baselines[{index}].id"
                ),
                repository=require_string(
                    row.get("repository"), f"fixed_baselines[{index}].repository"
                ),
                commit=require_string(
                    row.get("commit"), f"fixed_baselines[{index}].commit"
                ),
                script_roots=roots,
            )
        )
    if len(baselines) != 3:
        raise ValueError(f"expected three fixed baselines, found {len(baselines)}")
    return baselines


def parse_repository_overrides(values: Sequence[str]) -> dict[str, Path]:
    overrides: dict[str, Path] = {}
    for value in values:
        identifier, separator, raw_path = value.partition("=")
        if not separator or not identifier or not raw_path:
            raise ValueError("--repository must use BASELINE_ID=/absolute/path")
        path = Path(raw_path)
        if not path.is_absolute() or not path.is_dir():
            raise ValueError(
                f"repository override is not an existing absolute directory: {path}"
            )
        if identifier in overrides:
            raise ValueError(f"duplicate repository override: {identifier}")
        overrides[identifier] = path
    return overrides


def run_git(repository: Path, arguments: Sequence[str]) -> bytes:
    process = subprocess.run(
        ["git", "-C", str(repository), *arguments],
        check=False,
        capture_output=True,
    )
    if process.returncode != 0:
        diagnostic = process.stderr.decode("utf-8", errors="replace").strip()
        raise RuntimeError(
            f"git {' '.join(arguments)} failed in {repository}: {diagnostic}"
        )
    return process.stdout


@contextmanager
def materialize_repository(
    baseline: Baseline, overrides: Mapping[str, Path]
) -> Generator[Path]:
    override = overrides.get(baseline.identifier)
    if override is not None:
        run_git(override, ["cat-file", "-e", f"{baseline.commit}^{{commit}}"])
        yield override
        return

    with tempfile.TemporaryDirectory(
        prefix=f"pokecon-{baseline.identifier}-"
    ) as directory:
        repository = Path(directory)
        run_git(repository, ["init", "--quiet"])
        run_git(
            repository,
            ["fetch", "--quiet", "--depth=1", baseline.repository, baseline.commit],
        )
        fetched = run_git(repository, ["rev-parse", "FETCH_HEAD"]).decode().strip()
        if fetched != baseline.commit:
            raise RuntimeError(
                f"{baseline.identifier}: fetched {fetched}, expected {baseline.commit}"
            )
        yield repository


def decode_python_source(content: bytes, path: str) -> str:
    try:
        encoding, _ = tokenize.detect_encoding(io.BytesIO(content).readline)
        return content.decode(encoding)
    except (LookupError, SyntaxError, UnicodeDecodeError) as error:
        raise ValueError(f"{path}: cannot decode Python source: {error}") from error


def attribute_name(node: ast.AST) -> str | None:
    if isinstance(node, ast.Name):
        return node.id
    if isinstance(node, ast.Attribute):
        parent = attribute_name(node.value)
        return f"{parent}.{node.attr}" if parent is not None else None
    return None


def analyze_script(path: str, content: bytes) -> dict[str, object]:
    source = decode_python_source(content, path)
    try:
        tree = ast.parse(source, filename=path, type_comments=True)
    except SyntaxError as error:
        raise ValueError(f"{path}: Python 3.14 parse failed: {error}") from error

    imports: set[tuple[str, tuple[str, ...]]] = set()
    classes: list[dict[str, object]] = []
    references: set[str] = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                imports.add((alias.name, (alias.asname or alias.name,)))
        elif isinstance(node, ast.ImportFrom):
            module = "." * node.level + (node.module or "")
            names = tuple(sorted(alias.name for alias in node.names))
            imports.add((module, names))
        elif isinstance(node, ast.Call):
            name = attribute_name(node.func)
            if name is not None and name.startswith(("self.", "Commands.")):
                references.add(name)

    classes.extend(
        {
            "name": node.name,
            "bases": [ast.unparse(base) for base in node.bases],
            "line": node.lineno,
        }
        for node in tree.body
        if isinstance(node, ast.ClassDef)
    )

    import_rows = [
        {"module": module, "names": list(names)} for module, names in sorted(imports)
    ]
    return {
        "path": path,
        "sha256": hashlib.sha256(content).hexdigest(),
        "imports": import_rows,
        "classes": classes,
        "referenced_api": sorted(references),
        "expected": {
            "python_3_14_parse": "success",
            "imports": "resolve_without_source_changes",
            "discovered_classes": [class_row["name"] for class_row in classes],
            "runtime": "deterministic_fixture_or_explicit_hardware_gate",
        },
    }


def inventory_baseline(
    baseline: Baseline, overrides: Mapping[str, Path]
) -> dict[str, object]:
    with materialize_repository(baseline, overrides) as repository:
        tree_output = run_git(
            repository,
            [
                "ls-tree",
                "-r",
                "--name-only",
                baseline.commit,
                "--",
                *baseline.script_roots,
            ],
        ).decode("utf-8")
        paths = sorted(
            path for path in tree_output.splitlines() if path.endswith(".py")
        )
        if not paths:
            raise RuntimeError(f"{baseline.identifier}: no Python scripts found")
        scripts = [
            analyze_script(
                path,
                run_git(repository, ["show", f"{baseline.commit}:{path}"]),
            )
            for path in paths
        ]
    return {
        "id": baseline.identifier,
        "repository": baseline.repository,
        "commit": baseline.commit,
        "script_count": len(scripts),
        "scripts": scripts,
    }


def build_inventory(
    registry: Path, repository_overrides: Mapping[str, Path]
) -> dict[str, object]:
    baselines = load_baselines(registry)
    inventories = [
        inventory_baseline(baseline, repository_overrides) for baseline in baselines
    ]
    canonical = json.dumps(
        inventories, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")
    return {
        "schema_version": 1,
        "hash_algorithm": "sha256",
        "inventory_sha256": hashlib.sha256(canonical).hexdigest(),
        "baselines": inventories,
    }


def serialized_inventory(inventory: Mapping[str, object]) -> str:
    return json.dumps(inventory, ensure_ascii=False, indent=2, sort_keys=True) + "\n"


def main() -> int:
    root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--registry",
        type=Path,
        default=root / "rust/pokecon-contracts/registry/compatibility.json",
    )
    parser.add_argument(
        "--output", type=Path, default=root / "compatibility/fixed-manifest.json"
    )
    parser.add_argument(
        "--repository",
        action="append",
        default=[],
        metavar="BASELINE_ID=/ABSOLUTE/PATH",
        help="reuse a ghq clone instead of fetching the immutable commit",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="fail if the tracked output differs instead of updating it",
    )
    arguments = parser.parse_args()
    overrides = parse_repository_overrides(arguments.repository)
    inventory = build_inventory(arguments.registry, overrides)
    serialized = serialized_inventory(inventory)
    if arguments.check:
        if (
            not arguments.output.is_file()
            or arguments.output.read_text(encoding="utf-8") != serialized
        ):
            raise RuntimeError(
                f"compatibility inventory drift detected: {arguments.output}"
            )
    else:
        arguments.output.parent.mkdir(parents=True, exist_ok=True)
        arguments.output.write_text(serialized, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
