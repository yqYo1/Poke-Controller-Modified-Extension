"""Source boundary report tests (AR-10.10-13 / AR-11-44).

Covers only pure logic with synthetic inputs: manifest parsing and
``product ++`` resolution, fail-closed mutations, prefix membership
semantics, derived expectations, matrix cell evaluation, and the
include/exclude fixture builder. Heavy ``nix`` evaluations never run
under pytest; the live matrix is evidence captured outside the suite.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Final

import pytest

from scripts.acceptance import source_boundary_report as report

if TYPE_CHECKING:
    from pathlib import Path

SYNTHETIC_FLAKE: Final = """{
sourceBoundaryPaths = rec {
  product = [
    "rust/pokecon/src"
    "compatibility/fixed-manifest.json"
  ];
  release = product ++ [
    "README.md"
  ];
  rustCoreTest = product ++ [
    "rust/pokecon/tests"
  ];
  compatibilityCheck = [
    "compatibility/candidates.json"
    "compatibility/fixed-manifest.json"
  ];
  rustTest = product ++ [
    "scripts/ci/aggregate.py"
  ];
  web = [ "web" ];
  api = [ "api" ];
  python = [
    "python"
  ];
  documentation = [
    "docs"
  ];
  quality = [
    "scripts"
    "web"
  ];
};
optionalSourceBoundaryPaths = {
  product = [
    "rust/pokecon/capabilities"
  ];
  rustTest = [
    "extra/static"
  ]
  ++ resolvedRustTestTombstonePaths;
};
}
"""


def write_flake(root: Path, text: str = SYNTHETIC_FLAKE) -> Path:
    (root / "flake.nix").write_text(text, encoding="utf-8")
    return root


def test_manifest_resolves_product_extension() -> None:
    resolved, _ = report.parse_boundary_sets(SYNTHETIC_FLAKE)
    assert resolved["product"] == [
        "rust/pokecon/src",
        "compatibility/fixed-manifest.json",
    ]
    assert resolved["release"] == [
        "rust/pokecon/src",
        "compatibility/fixed-manifest.json",
        "README.md",
    ]
    assert "rust/pokecon/tests" in resolved["rustCoreTest"]
    assert "scripts/ci/aggregate.py" in resolved["rustTest"]
    assert resolved["web"] == ["web"]


def test_manifest_records_optional_dynamic_tail() -> None:
    _, optional = report.parse_boundary_sets(SYNTHETIC_FLAKE)
    assert optional["product"] == (["rust/pokecon/capabilities"], [])
    assert optional["rustTest"] == (
        ["extra/static"],
        ["resolvedRustTestTombstonePaths"],
    )


def test_manifest_rejects_missing_block() -> None:
    with pytest.raises(ValueError, match="has no"):
        report.parse_boundary_sets("{ }")


def test_manifest_rejects_unbalanced_braces() -> None:
    broken = SYNTHETIC_FLAKE.replace("};\noptional", ";\noptional", 1)
    with pytest.raises(ValueError, match=r"boundary statement|unbalanced|trailing"):
        report.parse_boundary_sets(broken)


def test_manifest_rejects_missing_area() -> None:
    broken = SYNTHETIC_FLAKE.replace('  web = [ "web" ];\n', "")
    with pytest.raises(ValueError, match=r"misses areas.*web"):
        report.parse_boundary_sets(broken)


def test_manifest_rejects_unknown_area() -> None:
    broken = SYNTHETIC_FLAKE.replace(
        '  web = [ "web" ];', '  web = [ "web" ];\n  nope = [ "x" ];'
    )
    with pytest.raises(ValueError, match="unknown boundary area"):
        report.parse_boundary_sets(broken)


def test_manifest_rejects_absolute_path() -> None:
    broken = SYNTHETIC_FLAKE.replace('"web"', '"/web"', 1)
    with pytest.raises(ValueError, match="non-normalized"):
        report.parse_boundary_sets(broken)


def test_manifest_rejects_parent_escape() -> None:
    broken = SYNTHETIC_FLAKE.replace('"web"', '"../web"', 1)
    with pytest.raises(ValueError, match="non-normalized"):
        report.parse_boundary_sets(broken)


def test_manifest_rejects_glob_path() -> None:
    broken = SYNTHETIC_FLAKE.replace('"web"', '"web/*"', 1)
    with pytest.raises(ValueError, match="non-normalized"):
        report.parse_boundary_sets(broken)


def test_path_in_area_matches_exact_and_subpath() -> None:
    assert report.path_in_area("web", ["web"])
    assert report.path_in_area("web/src/app.css", ["web"])
    assert report.path_in_area("docs/README.md", ["docs", "web"])


def test_path_in_area_rejects_sibling_prefix() -> None:
    assert not report.path_in_area("webapp/x.ts", ["web"])
    assert not report.path_in_area("web", ["web/src"])
    assert not report.path_in_area("other.md", ["docs"])


def test_derive_expected_changed_for_product_probe() -> None:
    manifest = {
        area: [area] if area not in ("release", "rustCoreTest", "rustTest") else []
        for area in report.AREAS
    }
    manifest["release"] = ["product", "README.md"]
    manifest["rustCoreTest"] = ["product", "rust/pokecon/tests"]
    manifest["rustTest"] = ["product", "scripts/ci/aggregate.py"]
    expected = report.derive_expected_changed("product/src/main.rs", manifest)
    assert expected == {
        "packages.x86_64-linux.pokecon",
        "packages.x86_64-linux.pokecon-core",
        "checks.x86_64-linux.rust-core-artifacts",
        "checks.x86_64-linux.production-perf-target",
        "checks.x86_64-linux.contract-sync",
        "checks.x86_64-linux.compatibility-corpus",
    }


def test_derive_expected_changed_for_isolated_probe() -> None:
    manifest = {area: [area] for area in report.AREAS}
    manifest["release"] = ["release", "README.md"]
    manifest["rustCoreTest"] = ["rustCoreTest"]
    manifest["rustTest"] = ["rustTest"]
    assert report.derive_expected_changed("README.md", manifest) == set()
    assert report.derive_expected_changed("web/src/app.css", manifest) == {
        "packages.x86_64-linux.pokecon",
        "packages.x86_64-linux.web",
    }


def test_derive_expected_changed_for_shared_probe() -> None:
    manifest = {area: [area] for area in report.AREAS}
    manifest["compatibilityCheck"] = ["compatibility"]
    manifest["rustTest"] = ["compatibility", "scripts"]
    expected = report.derive_expected_changed("compatibility/candidates.json", manifest)
    assert expected == {
        "checks.x86_64-linux.contract-sync",
        "checks.x86_64-linux.compatibility-corpus",
    }


def test_derive_expected_changed_respects_product_exclusion() -> None:
    manifest = {area: [area] for area in report.AREAS}
    manifest["product"] = ["rust/pokecon/src"]
    manifest["release"] = ["rust/pokecon/src", "README.md"]
    manifest["rustCoreTest"] = ["rust/pokecon/src", "rust/pokecon/tests"]
    manifest["rustTest"] = ["rust/pokecon/src", "scripts"]
    expected = report.derive_expected_changed(
        "rust/pokecon/src/tests/example.rs", manifest
    )
    # Excluded from the product-family sources, but still selected by the
    # rustCoreTest/rustTest sources which carry no exclusions; the
    # ${rustCoreCheck} input edge additionally moves compatibility-corpus.
    assert expected == {
        "checks.x86_64-linux.rust-core-artifacts",
        "checks.x86_64-linux.production-perf-target",
        "checks.x86_64-linux.contract-sync",
        "checks.x86_64-linux.compatibility-corpus",
    }


def test_derive_expected_changed_covers_derivation_input_edges() -> None:
    # compatibility-corpus and contract-sync take ${rustCoreCheck} as a
    # build input (flake.nix:2940,2988-2989), so a product probe must be
    # expected to move them via the rustCoreTest area.
    manifest = {area: [area] for area in report.AREAS}
    manifest["product"] = ["rust/pokecon/src"]
    manifest["rustCoreTest"] = ["rust/pokecon/src", "rust/pokecon/tests"]
    manifest["rustTest"] = ["rust/pokecon/src", "scripts"]
    manifest["compatibilityCheck"] = ["compatibility"]
    assert report.derive_expected_changed("rust/pokecon/src/main.rs", manifest) == {
        "packages.x86_64-linux.pokecon",
        "packages.x86_64-linux.pokecon-core",
        "checks.x86_64-linux.rust-core-artifacts",
        "checks.x86_64-linux.production-perf-target",
        "checks.x86_64-linux.contract-sync",
        "checks.x86_64-linux.compatibility-corpus",
    }


def test_evaluate_cell_pass_and_failed() -> None:
    changed, status = report.evaluate_cell("drv-a", "drv-b", True)
    assert (changed, status) == (True, "PASS")
    changed, status = report.evaluate_cell("drv-a", "drv-a", False)
    assert (changed, status) == (False, "PASS")
    changed, status = report.evaluate_cell("drv-a", "drv-a", True)
    assert (changed, status) == (False, "FAILED")
    changed, status = report.evaluate_cell("drv-a", "drv-b", False)
    assert (changed, status) == (True, "FAILED")


def test_build_manifest_reports_sorted_paths_and_probes(tmp_path: Path) -> None:
    manifest = report.build_manifest(write_flake(tmp_path))
    assert manifest["schema"] == report.SCHEMA_ID
    assert len(manifest["flake_sha256"]) == 64
    assert set(manifest["areas"]) == set(report.AREAS)
    for area in report.AREAS:
        entry = manifest["areas"][area]
        assert entry["paths"] == sorted(entry["paths"])
        assert entry["count"] == len(entry["paths"])
        assert entry["probe"] == report.AREA_PROBES[area]


def test_build_include_exclude_marks_membership(tmp_path: Path) -> None:
    root = write_flake(tmp_path)
    manifest = report.build_manifest(root)
    effective = {name: entry["paths"] for name, entry in manifest["areas"].items()}
    fixture = report.build_include_exclude(root, effective)
    assert fixture["soundness"]
    by_file = {row["file"]: row for row in fixture["rows"]}
    assert "product" in by_file["rust/pokecon/src/main.rs"]["member_of"]
    assert by_file["README.md"]["member_of"] == ["release"]
    assert (
        "compatibilityCheck"
        in by_file["compatibility/fixed-manifest.json"]["member_of"]
    )
    assert "product" in by_file["compatibility/fixed-manifest.json"]["member_of"]


def test_split_attr_accepts_package_and_check_shapes() -> None:
    assert report.split_attr("packages.x86_64-linux.web") == (
        "packages",
        "x86_64-linux",
        "web",
    )
    assert report.split_attr("checks.x86_64-linux.contract-sync") == (
        "checks",
        "x86_64-linux",
        "contract-sync",
    )


def test_split_attr_rejects_other_shapes() -> None:
    for bad in ("apps.x86_64-linux.test", "packages.web", "web", "", "a.b.c.d"):
        with pytest.raises(ValueError, match="unsupported shape"):
            report.split_attr(bad)


def test_matrix_rejects_unknown_area(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="unknown matrix area"):
        report.run_matrix(
            write_flake(tmp_path),
            {area: [area] for area in report.AREAS},
            areas=["nope"],
        )


def test_matrix_rejects_missing_probe(tmp_path: Path) -> None:
    root = write_flake(tmp_path)
    with pytest.raises(ValueError, match="matrix probe is missing"):
        report.run_matrix(
            root,
            {area: [area] for area in report.AREAS},
            areas=["product"],
        )
