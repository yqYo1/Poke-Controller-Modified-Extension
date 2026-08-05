import hashlib
import tomllib
from pathlib import Path
from typing import Any

CARGO_CACHE_DIRECTORY_TAG = (
    "Signature: 8a477f597d28d172789f06886806bc55\n"
    "# This file is a cache directory tag created by cargo.\n"
    "# For information about cache directory tags see https://bford.info/cachedir/\n"
)


def _read_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as source:
        return tomllib.load(source)


def test_workspace_is_the_single_version_source() -> None:
    root = Path(__file__).resolve().parents[2]
    cargo = _read_toml(root / "Cargo.toml")
    pyproject = _read_toml(root / "pyproject.toml")

    workspace_version = cargo["workspace"]["package"]["version"]
    assert workspace_version == "0.1.0"
    assert pyproject["project"]["dynamic"] == ["version"]
    assert "version" not in pyproject["project"]


def test_maturin_development_layout_fix_is_isolated() -> None:
    root = Path(__file__).resolve().parents[2]
    pyproject = _read_toml(root / "pyproject.toml")
    maturin = pyproject["tool"]["maturin"]

    assert maturin["python-packages"] == ["python/pokecon"]
    assert "python-source" not in maturin

    flake = (root / "flake.nix").read_text(encoding="utf-8")
    for contract in (
        'maturin_source="$gate_home/maturin-source"',
        "maturin-develop expected exactly one legacy python-packages setting",
        "Maturin wheel payload must live under pokecon/",
        "Maturin wheel pure-Python inventory differs from caller source",
        "maturin venv contains unexpected distributions",
        "maturin venv project RECORD has an unsafe path",
        "site-packages tree contains a symlink",
        'maturin_lock="$maturin_target_root/.maturin-develop.lock"',
        "using isolated per-run Cargo target:",
        'or "\\\\" in member',
        "-v | -vv* | --verbose)",
        "PokeCon task lock changed while it was held",
        "refusing a hook path outside the worktree and Git common directory",
    ):
        assert contract in flake


def test_per_run_cargo_target_disables_incremental_output() -> None:
    root = Path(__file__).resolve().parents[2]
    flake = (root / "flake.nix").read_text(encoding="utf-8")
    helper = flake.split("setupPerRunCargoTarget = ''", maxsplit=1)[1].split(
        "\n          '';", maxsplit=1
    )[0]

    incremental_export = "export CARGO_INCREMENTAL=0"
    target_export = 'export CARGO_TARGET_DIR="$cargo_target_dir"'
    assert helper.index(incremental_export) < helper.index(target_export)


def test_per_run_cargo_target_installs_canonical_cache_directory_tag() -> None:
    root = Path(__file__).resolve().parents[2]
    flake = (root / "flake.nix").read_text(encoding="utf-8")
    helper = flake.split("setupPerRunCargoTarget = ''", maxsplit=1)[1].split(
        "\n          '';", maxsplit=1
    )[0]

    assert len(CARGO_CACHE_DIRECTORY_TAG.encode()) == 177
    indented_tag = "".join(
        f"            {line}"
        for line in CARGO_CACHE_DIRECTORY_TAG.splitlines(keepends=True)
    )
    tag_definition = (
        "          cargoCacheDirectoryTag = pkgs.writeText "
        "\"pokecon-cargo-cache-directory-tag\" ''\n"
        f"{indented_tag}"
        "          '';\n"
    )
    assert flake.count(tag_definition) == 1
    assert flake.count("cargoCacheDirectoryTag =") == 1
    assert (
        hashlib.sha256(CARGO_CACHE_DIRECTORY_TAG.encode()).hexdigest()
        == "6d9d1d216e0f83abc5e5662ca62c92b4f23009466b54fa27321a69acdb778bb2"
    )
    tag_placement = (
        "            ${setupIsolatedCargoHome}\n"
        "            ${setupPerRunCargoTarget}\n"
        "            ${setupUvLinks}\n"
        "          '';\n\n"
        f"{tag_definition}"
        "\n"
        "          setupWorkdir = ''"
    )
    assert flake.count(tag_placement) == 1

    target_mkdir = 'mkdir -- "$cargo_target_dir"'
    target_containment = '"$canonical_gate_home"/*)'
    tag_path = 'cargo_cache_tag="$cargo_target_dir/CACHEDIR.TAG"'
    tag_creation = (
        'if ! (set -o noclobber; umask 022; "${pkgs.coreutils}/bin/cat" '
        '"${cargoCacheDirectoryTag}" >"$cargo_cache_tag"); then'
    )
    tag_identity_guard = (
        'if [ -L "$cargo_cache_tag" ] || [ ! -f "$cargo_cache_tag" ] \\'
    )
    tag_canonical_guard = (
        '|| [ "$(readlink -f "$cargo_cache_tag")" != "$cargo_cache_tag" ]; then'
    )
    tag_comparison = (
        'if ! "${pkgs.diffutils}/bin/cmp" -s -- "${cargoCacheDirectoryTag}" '
        '"$cargo_cache_tag"; then'
    )
    incremental_export = "export CARGO_INCREMENTAL=0"
    target_export = 'export CARGO_TARGET_DIR="$cargo_target_dir"'
    variable_cleanup = "unset canonical_gate_home cargo_cache_tag cargo_target_dir"
    tag_section = helper[helper.index(tag_path) : helper.index(incremental_export)]
    assert tuple(line.strip() for line in tag_section.splitlines() if line.strip()) == (
        tag_path,
        tag_creation,
        'echo "failed to create the canonical Cargo cache directory tag: $cargo_cache_tag" >&2',
        "exit 2",
        "fi",
        tag_identity_guard,
        tag_canonical_guard,
        'echo "per-run Cargo cache directory tag is redirected or not a regular file: $cargo_cache_tag" >&2',
        "exit 2",
        "fi",
        tag_comparison,
        'echo "per-run Cargo cache directory tag differs from Cargo\'s canonical tag: $cargo_cache_tag" >&2',
        "exit 2",
        "fi",
    )
    assert (
        helper.index(target_mkdir)
        < helper.index(target_containment)
        < helper.index(tag_path)
        < helper.index(tag_creation)
        < helper.index(tag_identity_guard)
        < helper.index(tag_canonical_guard)
        < helper.index(tag_comparison)
        < helper.index(incremental_export)
        < helper.index(target_export)
        < helper.index(variable_cleanup)
    )
    per_run_target_with_uv = (
        "            ${setupPerRunCargoTarget}\n            ${setupUvLinks}"
    )
    assert flake.count(per_run_target_with_uv) == 2


def test_aggregate_check_reuses_rust_artifacts_without_mid_run_clean() -> None:
    root = Path(__file__).resolve().parents[2]
    flake = (root / "flake.nix").read_text(encoding="utf-8")
    aggregate_check = flake.split("            check = mkTask {", maxsplit=1)[1].split(
        "\n            };", maxsplit=1
    )[0]
    assert "${cliHelpCheck.program}" not in aggregate_check
    assert "packaged CLI and UI use dedicated apps" in aggregate_check

    regular_build = "cargo build --locked --workspace --all-features"
    workspace_clippy = (
        "cargo clippy --locked --workspace --all-targets --all-features -- -D warnings"
    )
    workspace_test = "cargo test --locked --workspace --all-features"
    assert "reclaimPerRunCargoTarget" not in flake
    assert "cargo clean" not in flake
    linux_only_lld_export = (
        "${lib.optionalString pkgs.stdenv.isLinux "
        "\"export RUSTFLAGS='-C link-arg=-Wl,--threads=1'\"}"
    )
    dev_debug_export = "export CARGO_PROFILE_DEV_DEBUG=line-tables-only"
    test_debug_export = "export CARGO_PROFILE_TEST_DEBUG=line-tables-only"
    contract_generator = (
        "cargo run --locked --package pokecon --bin generate_contracts "
        "--features contract-generator -- --check"
    )
    targeted_contract_test = (
        "cargo test --locked --package pokecon --test contract_sync"
    )
    serial_build = f"{regular_build} --jobs 1"
    aggregate_workspace_builds = tuple(
        line.strip()
        for line in aggregate_check.splitlines()
        if "cargo build " in line and "--workspace" in line
    )
    assert aggregate_workspace_builds == (serial_build,)
    assert aggregate_check.count(workspace_clippy) == 1
    assert aggregate_check.count(workspace_test) == 1
    assert aggregate_check.count("--jobs 1") == 1
    assert aggregate_check.count("--threads=1") == 1
    assert aggregate_check.count(dev_debug_export) == 1
    assert aggregate_check.count(test_debug_export) == 1
    assert aggregate_check.count(targeted_contract_test) == 0
    assert flake.count(targeted_contract_test) == 1
    assert (
        aggregate_check.index(dev_debug_export)
        < aggregate_check.index(test_debug_export)
        < aggregate_check.index(linux_only_lld_export)
        < aggregate_check.index(contract_generator)
        < aggregate_check.index(workspace_test)
        < aggregate_check.index(serial_build)
        < aggregate_check.index(workspace_clippy)
    )

    for compatibility_binary in (
        '--compatibility-binary "$CARGO_TARGET_DIR/debug/pokecon-compatibility"',
        '--worker "$CARGO_TARGET_DIR/debug/pokecon-worker"',
    ):
        assert aggregate_check.count(compatibility_binary) == 1
        assert aggregate_check.index(workspace_clippy) < aggregate_check.index(
            compatibility_binary
        )

    all_workspace_builds = tuple(
        line.strip()
        for line in flake.splitlines()
        if "cargo build " in line and "--workspace" in line
    )
    assert all_workspace_builds == (
        regular_build,
        regular_build,
        regular_build,
        serial_build,
    )
    assert flake.count("--threads=1") == 1


def test_aggregate_check_runtime_inputs_include_exactly_one_jq() -> None:
    root = Path(__file__).resolve().parents[2]
    flake = (root / "flake.nix").read_text(encoding="utf-8")
    aggregate_check = flake.split("            check = mkTask {", maxsplit=1)[1].split(
        "\n            };", maxsplit=1
    )[0]
    runtime_inputs = aggregate_check.split(
        "              runtimeInputs = rustTaskInputs ++ [", maxsplit=1
    )[1].split("\n              ];", maxsplit=1)[0]
    runtime_input_names = tuple(
        line.strip() for line in runtime_inputs.splitlines() if line.strip()
    )

    assert runtime_input_names == (
        "pkgs.basedpyright",
        "pkgs.actionlint",
        "bun",
        "pkgs.check-jsonschema",
        "pkgs.diffutils",
        "pkgs.jq",
        "pkgs.markdownlint-cli",
        "pkgs.ripgrep",
        "pkgs.shellcheck",
        "pkgs.textlint",
        "pkgs.textlint-rule-no-start-duplicated-conjunction",
        "pkgs.typos",
    )
    assert runtime_input_names.count("pkgs.jq") == 1
