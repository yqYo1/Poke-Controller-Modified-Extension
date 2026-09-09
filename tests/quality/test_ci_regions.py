from __future__ import annotations

import json
import shutil
import subprocess
import sys
from pathlib import Path
from typing import TYPE_CHECKING

import pytest

from scripts.ci.regions import (
    REGIONS,
    ROUTING_PATHS,
    ZERO_OBJECT_IDS,
    ClassificationError,
    Region,
    classify_git_diff,
    classify_paths,
    git_changed_paths,
    is_routing_path,
)

if TYPE_CHECKING:
    from collections.abc import Sequence

REPOSITORY = Path(__file__).resolve().parents[2]
CLASSIFIER = REPOSITORY / "scripts/ci/regions.py"
ZERO_SHA = "0" * 40


@pytest.mark.parametrize(
    ("path", "expected_regions"),
    [
        ("README.md", {Region.DOCS}),
        ("PLAN.md", set[Region]()),
        ("TASK.md", set[Region]()),
        ("generated/settings.schema.json", {Region.CONTRACTS}),
        (
            "rust/pokecon/src/lib.rs",
            {Region.RUST, Region.ROUTING, Region.PRODUCT},
        ),
        ("python/pokecon/__init__.py", {Region.PYTHON, Region.PRODUCT}),
        ("web/src/routes/+page.svelte", {Region.WEB, Region.PRODUCT}),
    ],
)
def test_single_area_fixtures_select_owned_regions(
    path: str,
    expected_regions: set[Region],
) -> None:
    classification = classify_paths([path])

    assert classification.changed_paths == (path,)
    assert classification.applicable == expected_regions
    assert classification.fail_closed is False
    payload = classification.as_json_object()
    assert payload["schema_version"] == 1
    assert payload["regions"] == {
        region.value: region in expected_regions for region in REGIONS
    }
    reasons = payload["reasons"]
    assert isinstance(reasons, dict)
    assert reasons == {
        region.value: [path] if region in expected_regions else [] for region in REGIONS
    }


def test_mixed_paths_are_normalized_sorted_and_explained_per_region() -> None:
    classification = classify_paths(
        [
            "rust/pokecon/src/lib.rs",
            "./docs/DEVELOPMENT.md",
            "python/pokecon/typings/commands.pyi",
            "rust/pokecon/src/lib.rs",
        ]
    )

    assert classification.changed_paths == (
        "docs/DEVELOPMENT.md",
        "python/pokecon/typings/commands.pyi",
        "rust/pokecon/src/lib.rs",
    )
    assert classification.applicable == {
        Region.DOCS,
        Region.CONTRACTS,
        Region.RUST,
        Region.PYTHON,
        Region.ROUTING,
        Region.PRODUCT,
    }
    assert classification.reasons == {
        "docs": ["docs/DEVELOPMENT.md"],
        "contracts": ["python/pokecon/typings/commands.pyi"],
        "rust": ["rust/pokecon/src/lib.rs"],
        "python": ["python/pokecon/typings/commands.pyi"],
        "routing": ["rust/pokecon/src/lib.rs"],
        "web": [],
        "product": [
            "python/pokecon/typings/commands.pyi",
            "rust/pokecon/src/lib.rs",
        ],
        "remote_flake": [],
    }


def test_routing_exact_inputs_match_the_production_audit_inventory() -> None:
    expected = frozenset(
        {
            ".gitignore",
            "Cargo.lock",
            "Cargo.toml",
            "LICENSE",
            "flake.lock",
            "flake.nix",
            "pyproject.toml",
            "rust-toolchain.toml",
            "rust/pokecon/Cargo.toml",
            "rust/pokecon/build.rs",
            "rust/pokecon/icons/32x32.png",
            "rust/pokecon/icons/128x128.png",
            "rust/pokecon/icons/128x128@2x.png",
            "rust/pokecon/icons/icon.icns",
            "rust/pokecon/icons/icon.ico",
            "rust/pokecon/linux/70-pokecon-controller.rules",
            "rust/pokecon/linux/reload-udev.sh",
            "scripts/integration/virtual-io-smoke.sh",
            "scripts/release/build_runtime.py",
            "scripts/release/installer.nsi",
            "scripts/release/nsis-reproducibility.nsh",
            "scripts/release/normalize_debian_package.py",
            "scripts/release/normalize_linux_elf.py",
            "scripts/release/stage.py",
            "tests/quality/test_ui_package_check.py",
            "uv.lock",
        }
    )

    assert expected == ROUTING_PATHS
    for path in expected:
        assert is_routing_path(path), path
        assert Region.ROUTING in classify_paths([path]).applicable


@pytest.mark.parametrize(
    "path",
    [
        "rust/pokecon/src/lib.rs",
        "rust/pokecon/src/server/rest/state.rs",
        "rust/pokecon/capabilities/default.json",
        "rust/pokecon/permissions/default.toml",
        "rust/pokecon/tauri.conf.json",
        "rust/pokecon/tauri.linux.conf.json5",
        "rust/pokecon/Tauri.toml",
        "rust/pokecon/Tauri.linux.toml",
        ".cargo/config",
        "tools/.cargo/config.toml",
    ],
)
def test_routing_discovered_inputs_fail_closed_to_the_mutation_gate(path: str) -> None:
    assert is_routing_path(path)
    assert Region.ROUTING in classify_paths([path]).applicable


@pytest.mark.parametrize(
    "path",
    [
        "python/pokecon/commands.py",
        "rust/pokecon/src/server/routes.json",
        "rust/pokecon/permissions/autogenerated/schema.json",
        "rust/pokecon/config/tauri.conf.json",
        "rust/pokecon/tauri.linux.desktop.conf.json",
        ".worktree/fixture/.cargo/config.toml",
        "target/fixture/.cargo/config",
    ],
)
def test_non_inputs_do_not_select_the_routing_mutation_gate(path: str) -> None:
    assert not is_routing_path(path)
    assert Region.ROUTING not in classify_paths([path]).applicable


def _git(repository: Path, arguments: Sequence[str]) -> str:
    executable = shutil.which("git")
    assert executable is not None
    completed = subprocess.run(  # noqa: S603 - Nix supplies the Git executable.
        [executable, "-C", str(repository), *arguments],
        check=True,
        capture_output=True,
        text=True,
    )
    return completed.stdout.strip()


def _write(repository: Path, relative_path: str, content: str) -> None:
    path = repository / relative_path
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def _commit(repository: Path, message: str) -> str:
    _git(repository, ("add", "--all"))
    _git(repository, ("commit", "--quiet", "--message", message))
    return _git(repository, ("rev-parse", "HEAD"))


def _repository(tmp_path: Path) -> Path:
    repository = tmp_path / "repository"
    repository.mkdir()
    _git(repository, ("init", "--quiet"))
    _git(repository, ("config", "user.email", "ci-regions@example.invalid"))
    _git(repository, ("config", "user.name", "CI Regions"))
    return repository


def test_git_rename_classifies_both_source_and_destination(tmp_path: Path) -> None:
    repository = _repository(tmp_path)
    _write(repository, "docs/guide.md", "same content\n")
    base = _commit(repository, "base")
    destination = repository / "python/pokecon/guide.py"
    destination.parent.mkdir(parents=True)
    _git(repository, ("mv", "docs/guide.md", "python/pokecon/guide.py"))
    head = _commit(repository, "rename")

    classification = classify_git_diff(
        repository=repository,
        base=base,
        head=head,
    )

    assert classification.changed_paths == (
        "docs/guide.md",
        "python/pokecon/guide.py",
    )
    assert classification.applicable == {
        Region.DOCS,
        Region.PYTHON,
        Region.PRODUCT,
    }


def test_git_deletion_retains_deleted_path_for_classification(tmp_path: Path) -> None:
    repository = _repository(tmp_path)
    _write(repository, "web/src/retired.ts", "export const retired = true;\n")
    base = _commit(repository, "base")
    (repository / "web/src/retired.ts").unlink()
    head = _commit(repository, "delete")

    assert git_changed_paths(
        repository=repository,
        base=base,
        head=head,
    ) == ("web/src/retired.ts",)
    assert classify_git_diff(
        repository=repository,
        base=base,
        head=head,
    ).applicable == {Region.WEB, Region.PRODUCT}


def test_zero_before_uses_the_local_head_tree(tmp_path: Path) -> None:
    repository = _repository(tmp_path)
    _write(repository, "README.md", "new branch\n")
    _write(repository, "generated/settings.schema.json", "{}\n")
    head = _commit(repository, "initial")

    assert ZERO_SHA in ZERO_OBJECT_IDS
    classification = classify_git_diff(
        repository=repository,
        base=ZERO_SHA,
        head=head,
    )

    assert classification.changed_paths == (
        "README.md",
        "generated/settings.schema.json",
    )
    assert classification.applicable == {Region.DOCS, Region.CONTRACTS}
    assert classification.fail_closed is False


@pytest.mark.parametrize(
    "compatibility_path",
    [
        "compatibility/corpus.json",
        "scripts/compatibility/runner.py",
        "tests/compatibility/test_runner.py",
    ],
)
def test_compatibility_inputs_preserve_rust_and_product_guarantees(
    compatibility_path: str,
) -> None:
    classification = classify_paths([compatibility_path])

    assert {
        Region.CONTRACTS,
        Region.RUST,
        Region.PRODUCT,
    }.issubset(classification.applicable)
    assert classification.fail_closed is False


@pytest.mark.parametrize(
    "contract_path",
    [
        "rust/pokecon/src/server/api.rs",
        "rust/pokecon/src/server/openapi.rs",
        "rust/pokecon/src/server/paths.rs",
        "web/src/lib/api/openapi.json",
        "web/src/lib/api/openapi.ts",
    ],
)
def test_openapi_inputs_and_outputs_select_contract_drift_checks(
    contract_path: str,
) -> None:
    classification = classify_paths([contract_path])

    assert Region.CONTRACTS in classification.applicable


@pytest.mark.parametrize(
    "control_path",
    [
        ".github/workflows/normal-ci.yml",
        "flake.lock",
        "flake.nix",
        "scripts/ci/regions.py",
        "scripts/quality/generate-api-types.sh",
        "rust/pokecon/registry/ci.json",
    ],
)
def test_ci_control_paths_fail_closed_to_every_region(control_path: str) -> None:
    classification = classify_paths([control_path])

    assert classification.applicable == frozenset(REGIONS)
    assert classification.fail_closed is True
    assert classification.reasons == {
        region.value: [control_path] for region in REGIONS
    }


def test_cli_emits_versioned_json_and_github_outputs(tmp_path: Path) -> None:
    github_output = tmp_path / "github-output"
    completed = subprocess.run(  # noqa: S603 - fixed interpreter and repository script.
        [
            sys.executable,
            "-I",
            str(CLASSIFIER),
            "--path",
            "README.md",
            "--path",
            "web/src/app.ts",
            "--github-output",
            str(github_output),
        ],
        check=False,
        capture_output=True,
        text=True,
    )

    assert completed.returncode == 0
    assert completed.stderr == ""
    payload = json.loads(completed.stdout)
    assert payload == {
        "schema_version": 1,
        "changed_paths": ["README.md", "web/src/app.ts"],
        "regions": {
            "docs": True,
            "contracts": False,
            "rust": False,
            "python": False,
            "routing": False,
            "web": True,
            "product": True,
            "remote_flake": False,
        },
        "reasons": {
            "docs": ["README.md"],
            "contracts": [],
            "rust": [],
            "python": [],
            "routing": [],
            "web": ["web/src/app.ts"],
            "product": ["web/src/app.ts"],
            "remote_flake": [],
        },
        "fail_closed": False,
    }
    output_lines = github_output.read_text(encoding="utf-8").splitlines()
    assert output_lines[:-1] == [
        "docs=true",
        "contracts=false",
        "rust=false",
        "python=false",
        "routing=false",
        "web=true",
        "product=true",
        "remote_flake=false",
    ]
    assert output_lines[-1].startswith("regions_json=")
    assert (
        json.loads(output_lines[-1].removeprefix("regions_json=")) == payload["regions"]
    )


@pytest.mark.parametrize(
    "arguments",
    [
        [],
        ["--base", "HEAD"],
        ["README.md", "--base", "HEAD~1", "--head", "HEAD"],
        ["../outside"],
        ["--base", "missing", "--head", "HEAD"],
    ],
)
def test_cli_usage_and_git_errors_exit_two(arguments: list[str]) -> None:
    completed = subprocess.run(  # noqa: S603 - fixed interpreter and repository script.
        [
            sys.executable,
            "-I",
            str(CLASSIFIER),
            "--repository",
            str(REPOSITORY),
            *arguments,
        ],
        check=False,
        capture_output=True,
        text=True,
    )

    assert completed.returncode == 2
    assert "usage:" in completed.stderr
    assert completed.stdout == ""


def test_missing_local_base_is_an_offline_classification_error(
    tmp_path: Path,
) -> None:
    repository = _repository(tmp_path)
    _write(repository, "README.md", "head\n")
    head = _commit(repository, "initial")

    with pytest.raises(ClassificationError, match=r"git rev-parse .*failed"):
        git_changed_paths(
            repository=repository,
            base="f" * 40,
            head=head,
        )
