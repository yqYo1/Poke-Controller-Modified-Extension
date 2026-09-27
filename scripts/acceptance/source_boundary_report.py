"""Nix source-boundary evidence report (AR-10.10-13 / AR-11-44).

Reads ``flake.nix`` ``sourceBoundaryPaths`` directly (fail-closed static
parse, no Nix evaluation), emits a per-area manifest, runs a single-area
derivation-hash isolation matrix in temporary detached git worktrees, and
reports per-area file membership (include/exclude fixture).

Derivation -> scoped-source mapping (read from flake.nix, not guessed):

* ``packages.pokecon-core`` uses ``pokeconProductSource``
  (``sourceBoundaryPaths.product``, minus ``rust/pokecon/src/tests``).
* ``packages.web`` uses ``webSource`` (``sourceBoundaryPaths.web``).
* ``packages.pokecon`` assembles ``pokeconCorePackage`` + ``webPackage``,
  so it reacts to the union of the product and web areas.
* ``checks.rust-core-artifacts`` uses ``rustCoreTestSource``
  (``sourceBoundaryPaths.rustCoreTest`` = product + generated/lua,
  python typings, rust tests, toolchain; no exclusions).
* ``checks.contract-sync`` uses ``rustTestSource``
  (``sourceBoundaryPaths.rustTest`` = product + CI/registry/docs/api
  selection; no exclusions) and additionally takes the rustCoreTest-built
  ``${rustCoreCheck}`` binary as a build input (flake.nix:2940), so it
  tracks the union of the rustTest and rustCoreTest areas.
* ``checks.compatibility-corpus`` uses ``compatibilityCheckSource``
  (``sourceBoundaryPaths.compatibilityCheck``) and additionally takes
  the rustCoreTest-built ``${rustCoreCheck}`` binaries as build inputs
  (flake.nix:2988-2989), so it tracks the union of the
  compatibilityCheck and rustCoreTest areas. This edge was found through
  the matrix itself: the first run expected compatibility-corpus to be
  unaffected by product/rustCoreTest probes and marked those two cells
  FAILED; flake inspection confirmed the ``${rustCoreCheck}``
  interpolation, and the mapping was corrected here instead of the
  observations being adjusted.

Probe choice rationale (each probe belongs to its row area; the
``expected_changed`` set is *derived* from manifest membership, see
``derive_expected_changed``):

* product -> ``rust/pokecon/src/main.rs``: inside product, hence also in
  rustCoreTest and rustTest (both extend product). Expect pokecon,
  pokecon-core, rust-core-artifacts, contract-sync, and
  compatibility-corpus changed (the last via the rustCoreTestSource ->
  rustCoreCheck -> compatibilityCorpusCheck input edge); web unchanged.
  (Counter-example documented:
  ``compatibility/fixed-manifest.json`` is shared between product and
  compatibilityCheck and moves the same five through its own src too.)
* release -> ``README.md``: in release/documentation/quality only; no
  matrix attr consumes the release source, so expect no change.
* rustCoreTest -> ``generated/lua/pokecon.d.lua``: in rustCoreTest,
  rustTest, quality. Expect rust-core-artifacts + contract-sync +
  compatibility-corpus (input edge) changed.
* compatibilityCheck -> ``compatibility/candidates.json``: in
  compatibilityCheck, rustTest, quality. Expect contract-sync +
  compatibility-corpus changed.
* rustTest -> ``scripts/ci/aggregate.py``: in rustTest + quality only.
  Expect contract-sync changed alone.
* web -> ``web/src/app.css``: in web + quality only. ``rustTest`` covers
  just ``web/src/lib/api*`` + ``camera-selector.ts``, so app.css must NOT
  move contract-sync. Expect web + pokecon changed.
  (Counter-example: ``web/src/lib/api.ts`` would also move contract-sync.)
* api -> ``api/package.json``: in api + quality only. Expect no change.
  (Counter-example: ``api/openapi.json`` is in rustTest and would move
  contract-sync.)
* python -> ``python/pokecon/__init__.py``: in python + quality only
  (rustCoreTest/rustTest cover only ``python/pokecon/typings``).
  Expect no change.
* documentation -> ``docs/README.md``: in documentation + quality only
  (rustTest covers only a fixed docs subset: ACCEPTANCE, ARCHITECTURE*,
  SPECIFICATION_*). Expect no change.
* quality -> ``scripts/quality/run_parallel_checks.py``: in quality only
  (rustTest covers scripts/ci + scripts/compatibility, not
  scripts/quality). Expect no change.

Include/exclude mechanism: pure ``pathIsSelected`` mirror — a regular file
``rel`` is selected by an area iff ``rel == selected`` or ``rel``
starts with ``selected + "/"`` for some effective path of that area.
This is exactly the ``type == "regular"`` branch of ``mkScopedSource``.
Soundness limits (reported verbatim in the JSON output): the probe does
not evaluate Nix, so (1) ``optionalSourceBoundaryPaths`` dynamic tails
(``resolvedRustTestTombstonePaths``, registry tombstones) are reported as
unresolved markers rather than membership facts; (2) derivation-level
``excludedPaths`` (``rust/pokecon/src/tests`` for the product/release
sources) apply at derivation assembly, not at area membership, and are
reported separately; (3) directory-ancestor traversal (parent dirs kept
only to reach selected children) is not modeled because every probe is a
regular file; (4) the model assumes no glob/path with ``*`` appears in
the static lists (asserted at parse time).
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Final, Never, TypedDict, cast

if TYPE_CHECKING:
    from collections.abc import Mapping, Sequence

type JsonObject = dict[str, object]

SCHEMA_ID: Final = "source-boundary-report/1"
SOURCE_BOUNDARY_ANCHOR: Final = "sourceBoundaryPaths = rec {"
OPTIONAL_BOUNDARY_ANCHOR: Final = "optionalSourceBoundaryPaths = {"
FLAKE_RELATIVE: Final = Path("flake.nix")

AREAS: Final = (
    "product",
    "release",
    "rustCoreTest",
    "compatibilityCheck",
    "rustTest",
    "web",
    "api",
    "python",
    "documentation",
    "quality",
)

MATRIX_ATTRS: Final = (
    "packages.x86_64-linux.pokecon",
    "packages.x86_64-linux.pokecon-core",
    "packages.x86_64-linux.web",
    "checks.x86_64-linux.rust-core-artifacts",
    "checks.x86_64-linux.contract-sync",
    "checks.x86_64-linux.compatibility-corpus",
)

# Derivation attr -> areas whose paths feed its scoped source (flake.nix
# mapping quoted in the module docstring). pokecon assembles
# pokecon-core (product) + web (web). compatibility-corpus additionally
# tracks rustCoreTest: its buildPhase interpolates
# "${rustCoreCheck}/libexec/..." (flake.nix:2988-2989), making the
# rustCoreTest-built rustCoreCheck an input of its derivation, so every
# rustCoreTest change (including all of product) moves its drvPath.
# contract-sync's own src is rustTestSource and it additionally takes the
# rustCoreTest-built ${rustCoreCheck} as an input (flake.nix:2940); both
# areas are listed so the union stays right even if the manifest drifts
# (today rustCoreTest effective paths are a subset of rustTest's).
DERIVATION_SOURCE_AREAS: Final = {
    "packages.x86_64-linux.pokecon": ("product", "web"),
    "packages.x86_64-linux.pokecon-core": ("product",),
    "packages.x86_64-linux.web": ("web",),
    "checks.x86_64-linux.rust-core-artifacts": ("rustCoreTest",),
    "checks.x86_64-linux.contract-sync": ("rustTest", "rustCoreTest"),
    "checks.x86_64-linux.compatibility-corpus": (
        "compatibilityCheck",
        "rustCoreTest",
    ),
}

# Product-family sources (pokeconProductSource, pokeconReleaseSource)
# exclude this subtree even though the area path selects it.
PRODUCT_EXCLUDED_PREFIX: Final = "rust/pokecon/src/tests"

# One existing tracked probe file per area (rationale in module docstring).
AREA_PROBES: Final = {
    "product": "rust/pokecon/src/main.rs",
    "release": "README.md",
    "rustCoreTest": "generated/lua/pokecon.d.lua",
    "compatibilityCheck": "compatibility/candidates.json",
    "rustTest": "scripts/ci/aggregate.py",
    "web": "web/src/app.css",
    "api": "api/package.json",
    "python": "python/pokecon/__init__.py",
    "documentation": "docs/README.md",
    "quality": "scripts/quality/run_parallel_checks.py",
}

# Extra edge-case probes for include-exclude: shared files plus the
# product-family exclusion subtree.
EXTRA_PROBES: Final = (
    "compatibility/fixed-manifest.json",
    "api/openapi.json",
    "docs/ACCEPTANCE.md",
    "python/pokecon/typings/commands.pyi",
    "web/src/lib/api.ts",
    "rust/pokecon/src/tests/ui_boundary_acceptance.rs",
)

PROBE_SUFFIX: Final = b"\n"

INCLUDE_EXCLUDE_SOUNDNESS: Final = (
    "Membership mirrors the mkScopedSource regular-file branch "
    "(rel == selected or rel under selected + '/') without evaluating Nix. "
    "Limits: (1) optionalSourceBoundaryPaths dynamic tails "
    "(resolvedRustTestTombstonePaths) are unresolved markers, not "
    "membership facts; (2) derivation-level excludedPaths "
    "(rust/pokecon/src/tests for product/release sources) apply at "
    "derivation assembly, not area membership, and are reported "
    "separately; (3) directory-ancestor traversal is not modeled (all "
    "probes are regular files); (4) static lists must contain no '*' "
    "glob (asserted at parse time)."
)


class AreaReport(TypedDict):
    paths: list[str]
    count: int
    probe: str


class ManifestReport(TypedDict):
    schema: str
    flake: str
    flake_sha256: str
    areas: dict[str, AreaReport]
    optional_paths: dict[str, list[str]]
    optional_dynamic_tails: dict[str, list[str]]
    product_family_excluded: list[str]


class MatrixCell(TypedDict):
    attr: str
    baseline: str
    observed: str
    changed: bool
    expected_changed: bool
    status: str


class MatrixRow(TypedDict):
    area: str
    probe: str
    expected_changed: list[str]
    cells: list[MatrixCell]
    passed: bool


class MatrixReport(TypedDict):
    schema: str
    head_sha: str
    attrs: list[str]
    rows: list[MatrixRow]
    passed: bool
    failed_cells: list[str]


class MembershipRow(TypedDict):
    file: str
    exists: bool
    member_of: list[str]
    excluded_from_product_family: bool


class IncludeExcludeReport(TypedDict):
    schema: str
    mechanism: str
    soundness: str
    rows: list[MembershipRow]


def invalid_value(message: str) -> Never:
    raise ValueError(message)


def strip_line_comments(text: str) -> str:
    """Remove ``#`` comments outside double-quoted Nix strings."""
    kept: list[str] = []
    in_string = False
    escaped = False
    for line in text.splitlines():
        out: list[str] = []
        index = 0
        while index < len(line):
            char = line[index]
            if in_string:
                out.append(char)
                if escaped:
                    escaped = False
                elif char == "\\":
                    escaped = True
                elif char == '"':
                    in_string = False
                index += 1
                continue
            if char == '"':
                in_string = True
                out.append(char)
                index += 1
                continue
            if char == "#":
                break
            out.append(char)
            index += 1
        kept.append("".join(out))
    return "\n".join(kept)


def extract_braced_block(text: str, anchor: str) -> str:
    """Return the ``{...}`` body following *anchor* (fail-closed)."""
    start = text.find(anchor)
    if start < 0:
        invalid_value(f"flake.nix has no {anchor!r} block")
    brace = text.find("{", start)
    if brace < 0:
        invalid_value(f"{anchor!r} block has no opening brace")
    depth = 0
    in_string = False
    escaped = False
    index = brace
    while index < len(text):
        char = text[index]
        if in_string:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                in_string = False
        elif char == '"':
            in_string = True
        elif char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return text[brace + 1 : index]
        index += 1
    invalid_value(f"{anchor!r} block has unbalanced braces")


def split_top_level_statements(body: str) -> list[tuple[str, str]]:
    """Split ``name = value;`` statements at depth zero (fail-closed)."""
    statements: list[str] = []
    depth_square = 0
    depth_brace = 0
    depth_paren = 0
    in_string = False
    escaped = False
    current: list[str] = []
    for char in body:
        if in_string:
            current.append(char)
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                in_string = False
            continue
        if char == '"':
            in_string = True
            current.append(char)
        elif char in "[{(":
            if char == "[":
                depth_square += 1
            elif char == "{":
                depth_brace += 1
            else:
                depth_paren += 1
            current.append(char)
        elif char in "]})":
            if char == "]":
                depth_square -= 1
            elif char == "}":
                depth_brace -= 1
            else:
                depth_paren -= 1
            if min(depth_square, depth_brace, depth_paren) < 0:
                invalid_value("boundary block has unbalanced brackets")
            current.append(char)
        elif char == ";" and depth_square == depth_brace == depth_paren == 0:
            statements.append("".join(current))
            current = []
        else:
            current.append(char)
    if in_string:
        invalid_value("boundary block has an unterminated string")
    if depth_square != 0 or depth_brace != 0 or depth_paren != 0:
        invalid_value("boundary block has unbalanced brackets")
    remainder = "".join(current).strip()
    if remainder:
        invalid_value(f"boundary block has trailing text: {remainder[:80]!r}")
    parsed: list[tuple[str, str]] = []
    for statement in statements:
        if "=" not in statement:
            invalid_value(f"boundary statement has no '=': {statement[:80]!r}")
        name, _, value = statement.partition("=")
        name = name.strip()
        if not name.replace("_", "").isalnum() or not name[0].isalpha():
            invalid_value(f"boundary statement has a bad name: {name!r}")
        parsed.append((name, value.strip()))
    return parsed


def parse_string_list(value: str) -> list[str]:
    """Parse a Nix ``[ "a" "b" ]`` literal (fail-closed)."""
    text = value.strip()
    if not (text.startswith("[") and text.endswith("]")):
        invalid_value(f"boundary value is not a list: {value[:80]!r}")
    items: list[str] = []
    index = 1
    end = len(text) - 1
    while True:
        while index < end and text[index].isspace():
            index += 1
        if index >= end:
            break
        if text[index] != '"':
            invalid_value(f"boundary list has a non-string item: {text[:80]!r}")
        index += 1
        item: list[str] = []
        closed = False
        while index < end:
            char = text[index]
            if char == "\\":
                if index + 1 >= end:
                    invalid_value("boundary string has a trailing backslash")
                item.append(text[index + 1])
                index += 2
                continue
            if char == '"':
                closed = True
                index += 1
                break
            item.append(char)
            index += 1
        if not closed:
            invalid_value("boundary string is unterminated")
        items.append("".join(item))
    return items


def check_relative_path(item: str, area: str) -> None:
    if (
        item == ""
        or item.startswith("/")
        or "*" in item
        or any(component in ("", ".", "..") for component in item.split("/"))
    ):
        invalid_value(f"area {area!r} has a non-normalized path: {item!r}")


def parse_area_value(name: str, value: str) -> tuple[list[str], str | None, str | None]:
    """Parse ``name = <value>`` into (paths, base_ref, dynamic_tail)."""
    if "++" in value:
        parts = [part.strip() for part in value.split("++")]
        if len(parts) != 2:
            invalid_value(f"area {name!r} has more than one '++'")
        head, tail = parts
        if head == name or (head.startswith("[") and tail == name):
            invalid_value(f"area {name!r} is self-referential")
        if head in AREAS:
            return parse_string_list(tail), head, None
        if tail in ("resolvedRustTestTombstonePaths",) or not tail.startswith("["):
            return parse_string_list(head), None, tail
        invalid_value(f"area {name!r} has an unknown '++' shape")
    return parse_string_list(value), None, None


def parse_boundary_sets(
    flake_text: str,
) -> tuple[dict[str, list[str]], dict[str, tuple[list[str], list[str]]]]:
    """Parse required + optional boundary sets; resolve ``product ++`` refs."""
    required_body = strip_line_comments(
        extract_braced_block(flake_text, SOURCE_BOUNDARY_ANCHOR)
    )
    required_raw: dict[str, tuple[list[str], str | None]] = {}
    for name, value in split_top_level_statements(required_body):
        if name not in AREAS:
            invalid_value(f"unknown boundary area: {name!r}")
        if name in required_raw:
            invalid_value(f"duplicate boundary area: {name!r}")
        paths, base_ref, dynamic_tail = parse_area_value(name, value)
        if dynamic_tail is not None:
            invalid_value(
                f"required area {name!r} has a dynamic tail: {dynamic_tail!r}"
            )
        for item in paths:
            check_relative_path(item, name)
        required_raw[name] = (paths, base_ref)
    if set(required_raw) != set(AREAS):
        missing = sorted(set(AREAS) - set(required_raw))
        invalid_value(f"boundary block misses areas: {missing}")

    resolved: dict[str, list[str]] = {}
    for name in AREAS:
        paths, base_ref = required_raw[name]
        if base_ref is None:
            resolved[name] = list(paths)
            continue
        if base_ref != "product" or base_ref not in required_raw:
            invalid_value(f"area {name!r} extends unknown base {base_ref!r}")
        base_paths, base_base = required_raw[base_ref]
        if base_base is not None:
            invalid_value(f"area {name!r} extends non-root base {base_ref!r}")
        resolved[name] = list(base_paths) + [
            item for item in paths if item not in base_paths
        ]

    optional_body = strip_line_comments(
        extract_braced_block(flake_text, OPTIONAL_BOUNDARY_ANCHOR)
    )
    optional: dict[str, tuple[list[str], list[str]]] = {}
    for name, value in split_top_level_statements(optional_body):
        paths, base_ref, dynamic_tail = parse_area_value(name, value)
        if base_ref is not None:
            invalid_value(f"optional area {name!r} extends {base_ref!r}")
        for item in paths:
            check_relative_path(item, name)
        tails = [dynamic_tail] if dynamic_tail is not None else []
        optional[name] = (paths, tails)
    return resolved, optional


def path_in_area(relative_path: str, area_paths: Sequence[str]) -> bool:
    """Mirror the ``mkScopedSource`` regular-file branch (pathIsSelected)."""
    return any(
        relative_path == selected or relative_path.startswith(selected + "/")
        for selected in area_paths
    )


def is_product_family_excluded(relative_path: str) -> bool:
    return relative_path == PRODUCT_EXCLUDED_PREFIX or relative_path.startswith(
        PRODUCT_EXCLUDED_PREFIX + "/"
    )


def derive_expected_changed(
    probe: str, manifest: Mapping[str, Sequence[str]]
) -> set[str]:
    """Derive the expected-changed attr set from manifest membership."""
    member_of = {area for area in AREAS if path_in_area(probe, manifest[area])}
    excluded = is_product_family_excluded(probe)
    expected: set[str] = set()
    for attr, areas in DERIVATION_SOURCE_AREAS.items():
        hit = member_of.intersection(areas)
        if not hit:
            continue
        if excluded and hit == {"product"}:
            # Excluded from the product-family sources, and no other
            # feeding area selects the probe.
            continue
        expected.add(attr)
    return expected


def evaluate_cell(
    baseline: str, observed: str, expected_changed: bool
) -> tuple[bool, str]:
    changed = baseline != observed
    return changed, "PASS" if changed == expected_changed else "FAILED"


def build_manifest(repo_root: Path) -> ManifestReport:
    flake_path = repo_root / FLAKE_RELATIVE
    try:
        flake_text = flake_path.read_text(encoding="utf-8")
    except OSError as error:
        invalid_value(f"cannot read {flake_path}: {error}")
    resolved, optional = parse_boundary_sets(flake_text)
    areas: dict[str, AreaReport] = {}
    for name in AREAS:
        areas[name] = {
            "paths": sorted(resolved[name]),
            "count": len(resolved[name]),
            "probe": AREA_PROBES[name],
        }
    return {
        "schema": SCHEMA_ID,
        "flake": str(FLAKE_RELATIVE),
        "flake_sha256": hashlib.sha256(flake_text.encode("utf-8")).hexdigest(),
        "areas": areas,
        "optional_paths": {
            name: sorted(paths) for name, (paths, _) in optional.items()
        },
        "optional_dynamic_tails": {
            name: sorted(tails) for name, (_, tails) in optional.items()
        },
        "product_family_excluded": [PRODUCT_EXCLUDED_PREFIX],
    }


def build_include_exclude(
    repo_root: Path, manifest: Mapping[str, Sequence[str]]
) -> IncludeExcludeReport:
    probes = [AREA_PROBES[name] for name in AREAS] + list(EXTRA_PROBES)
    rows: list[MembershipRow] = [
        {
            "file": probe,
            "exists": (repo_root / probe).is_file(),
            "member_of": sorted(
                area for area in AREAS if path_in_area(probe, manifest[area])
            ),
            "excluded_from_product_family": is_product_family_excluded(probe),
        }
        for probe in probes
    ]
    return {
        "schema": SCHEMA_ID,
        "mechanism": (
            "pathIsSelected mirror: regular file rel is selected iff "
            "rel == selected or rel starts with selected + '/'."
        ),
        "soundness": INCLUDE_EXCLUDE_SOUNDNESS,
        "rows": rows,
    }


@dataclass(frozen=True)
class NixConfig:
    timeout_seconds: int = 900


def run_command(arguments: Sequence[str], workdir: Path, timeout_seconds: int) -> str:
    try:
        completed = subprocess.run(  # noqa: S603 - fixed argv without a shell.
            list(arguments),
            cwd=workdir,
            capture_output=True,
            text=True,
            timeout=timeout_seconds,
            check=False,
        )
    except subprocess.TimeoutExpired as error:
        invalid_value(
            f"command timed out after {timeout_seconds}s: {' '.join(arguments)} "
            f"({error})"
        )
    if completed.returncode != 0:
        invalid_value(
            f"command failed ({completed.returncode}): "
            f"{' '.join(arguments)}\n{completed.stderr.strip()[-2000:]}"
        )
    return completed.stdout


def split_attr(attr: str) -> tuple[str, str, str]:
    """Split ``<collection>.<system>.<leaf>`` (fail-closed).

    Only the ``packages`` and ``checks`` collections expose derivations
    with a ``drvPath``; anything else cannot take part in the matrix.
    """
    parts = attr.split(".")
    if len(parts) != 3 or parts[0] not in ("packages", "checks") or not all(parts):
        invalid_value(f"matrix attr has an unsupported shape: {attr!r}")
    return parts[0], parts[1], parts[2]


def derivation_hashes(
    tree_dir: Path, attrs: Sequence[str], config: NixConfig
) -> dict[str, str]:
    """Evaluate every attr's derivation path with identity preserved.

    One ``nix eval`` invocation per flake-output collection (``packages``,
    ``checks``) per tree state — still O(1) in the attr count, not one
    invocation per attr. ``nix path-info --derivation --json`` was tried
    first but is unsuitable: it keys results by store path in sorted
    order, losing which installable produced which derivation (verified
    empirically: reversed argument order still prints sorted). The
    ``--apply (mapAttrs drvPath)`` form instead returns a leaf-name keyed
    object, so every hash is looked up by attr name, never by position.
    Fail-closed guards: payload must be a string-valued object, every
    requested leaf must be present, and every value must be a ``*.drv``.
    """
    groups: dict[tuple[str, str], list[str]] = {}
    for attr in attrs:
        collection, system, _ = split_attr(attr)
        groups.setdefault((collection, system), []).append(attr)
    resolved: dict[str, str] = {}
    for (collection, system), group in sorted(groups.items()):
        output = run_command(
            [
                "nix",
                "eval",
                "--json",
                "--apply",
                "builtins.mapAttrs (n: v: v.drvPath)",
                f".#{collection}.{system}",
            ],
            tree_dir,
            config.timeout_seconds,
        )
        try:
            decoded = json.loads(output)
        except json.JSONDecodeError as error:
            invalid_value(f"nix eval returned invalid JSON: {error}")
        if not isinstance(decoded, dict):
            invalid_value("nix eval returned a non-object payload")
        payload = cast("JsonObject", decoded)
        for attr in group:
            _, _, leaf = split_attr(attr)
            if leaf not in payload:
                invalid_value(
                    f"nix eval has no leaf {leaf!r} under {collection}.{system}"
                )
            drv_path = payload[leaf]
            if not isinstance(drv_path, str):
                invalid_value("nix eval returned a non-string-valued object")
            if not drv_path.endswith(".drv"):
                invalid_value(f"nix eval returned a non-derivation path: {drv_path!r}")
            resolved[attr] = drv_path
    return resolved


def git_head_sha(repo_root: Path) -> str:
    return run_command(["git", "rev-parse", "HEAD"], repo_root, 60).strip()


def run_matrix(
    repo_root: Path,
    manifest: Mapping[str, Sequence[str]],
    areas: Sequence[str] = AREAS,
    attrs: Sequence[str] = MATRIX_ATTRS,
    config: NixConfig | None = None,
) -> MatrixReport:
    for area in areas:
        if area not in AREAS:
            invalid_value(f"unknown matrix area: {area!r}")
        probe = AREA_PROBES[area]
        if not (repo_root / probe).is_file():
            invalid_value(f"matrix probe is missing: {probe!r}")
    for attr in attrs:
        if attr not in DERIVATION_SOURCE_AREAS:
            invalid_value(f"unknown matrix attr: {attr!r}")
    resolved_config = NixConfig() if config is None else config
    head_sha = git_head_sha(repo_root)
    scratch_parent = Path(tempfile.mkdtemp(prefix="source-boundary-"))
    tree_dir = scratch_parent / "tree"
    try:
        run_command(
            ["git", "worktree", "add", "--detach", str(tree_dir), head_sha],
            repo_root,
            120,
        )
        try:
            baseline = derivation_hashes(tree_dir, attrs, resolved_config)
            rows: list[MatrixRow] = []
            for area in areas:
                probe = AREA_PROBES[area]
                expected = derive_expected_changed(probe, manifest)
                target = tree_dir / probe
                original = target.read_bytes()
                target.write_bytes(original + PROBE_SUFFIX)
                try:
                    observed = derivation_hashes(tree_dir, attrs, resolved_config)
                finally:
                    target.write_bytes(original)
                cells: list[MatrixCell] = []
                for attr in attrs:
                    base, seen = baseline[attr], observed[attr]
                    want = attr in expected
                    changed, status = evaluate_cell(base, seen, want)
                    cells.append(
                        {
                            "attr": attr,
                            "baseline": base,
                            "observed": seen,
                            "changed": changed,
                            "expected_changed": want,
                            "status": status,
                        }
                    )
                passed = all(cell["status"] == "PASS" for cell in cells)
                rows.append(
                    {
                        "area": area,
                        "probe": probe,
                        "expected_changed": sorted(expected),
                        "cells": cells,
                        "passed": passed,
                    }
                )
        finally:
            run_command(
                ["git", "worktree", "remove", "--force", str(tree_dir)],
                repo_root,
                120,
            )
    finally:
        shutil.rmtree(scratch_parent, ignore_errors=True)
    failed = sorted(
        f"{row['area']}:{cell['attr']}"
        for row in rows
        for cell in row["cells"]
        if cell["status"] != "PASS"
    )
    return {
        "schema": SCHEMA_ID,
        "head_sha": head_sha,
        "attrs": list(attrs),
        "rows": rows,
        "passed": not failed,
        "failed_cells": failed,
    }


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("manifest", help="emit the source boundary manifest")
    matrix_parser = subparsers.add_parser(
        "matrix", help="run the single-area derivation-hash isolation matrix"
    )
    matrix_parser.add_argument("--area", action="append", default=None)
    matrix_parser.add_argument("--attr", action="append", default=None)
    matrix_parser.add_argument("--timeout-seconds", type=int, default=900)
    subparsers.add_parser("include-exclude", help="report per-area file membership")
    arguments = parser.parse_args(argv)
    try:
        repo_root = cast("Path", arguments.root)
        manifest = build_manifest(repo_root)
        effective: Mapping[str, Sequence[str]] = {
            name: report["paths"] for name, report in manifest["areas"].items()
        }
        if arguments.command == "manifest":
            print(json.dumps(manifest, indent=2, sort_keys=True))
            return 0
        if arguments.command == "include-exclude":
            report = build_include_exclude(repo_root, effective)
            print(json.dumps(report, indent=2, sort_keys=True))
            return 0
        config = NixConfig(timeout_seconds=arguments.timeout_seconds)
        report = run_matrix(
            repo_root,
            effective,
            areas=arguments.area or AREAS,
            attrs=arguments.attr or MATRIX_ATTRS,
            config=config,
        )
        print(json.dumps(report, indent=2, sort_keys=True))
        if not report["passed"]:
            print(
                f"source boundary matrix failed: {report['failed_cells']}",
                file=sys.stderr,
                flush=True,
            )
            return 1
        return 0
    except (OSError, ValueError) as error:
        parser.exit(1, f"source boundary report failed: {error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
