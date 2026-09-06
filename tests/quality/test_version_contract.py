import hashlib
import importlib
import os
import re
import subprocess
import sys
import tomllib
import zipfile
from pathlib import Path, PurePosixPath
from typing import Any

import pytest

CARGO_CACHE_DIRECTORY_TAG = (
    "Signature: 8a477f597d28d172789f06886806bc55\n"
    "# This file is a cache directory tag created by cargo.\n"
    "# For information about cache directory tags see https://bford.info/cachedir/\n"
)


def _read_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as source:
        return tomllib.load(source)


def test_workspace_and_pure_python_package_versions_match() -> None:
    root = Path(__file__).resolve().parents[2]
    cargo = _read_toml(root / "Cargo.toml")
    pyproject = _read_toml(root / "pyproject.toml")

    workspace_version = cargo["workspace"]["package"]["version"]
    assert workspace_version == "0.1.0"
    assert pyproject["project"]["version"] == workspace_version
    assert "dynamic" not in pyproject["project"]
    assert pyproject["build-system"] == {
        "requires": ["uv_build==0.11.28"],
        "build-backend": "uv_build",
    }
    assert pyproject["tool"]["uv"]["build-backend"] == {
        "module-name": "pokecon",
        "module-root": "python",
    }


def test_pure_python_package_builds_offline_without_native_payload(
    tmp_path: Path,
) -> None:
    root = Path(__file__).resolve().parents[2]
    uv = Path(os.environ["POKECON_TEST_UV"])
    assert uv.is_file()
    output = tmp_path / "wheel"
    isolated_home = tmp_path / "home"
    isolated_tmp = tmp_path / "tmp"
    isolated_cache = tmp_path / "cache"
    for directory in (output, isolated_home, isolated_tmp, isolated_cache):
        directory.mkdir()
    environment = {
        "HOME": str(isolated_home),
        "PATH": os.environ["PATH"],
        "TMPDIR": str(isolated_tmp),
        "UV_CACHE_DIR": str(isolated_cache),
        "UV_NO_CONFIG": "1",
        "UV_OFFLINE": "1",
    }
    subprocess.run(  # noqa: S603 - executable path is injected by the pinned Nix task
        (
            str(uv),
            "--no-config",
            "build",
            "--offline",
            "--no-cache",
            "--wheel",
            "--out-dir",
            str(output),
        ),
        cwd=root,
        env=environment,
        check=True,
        capture_output=True,
        text=True,
    )

    wheels = tuple(output.glob("*.whl"))
    assert tuple(wheel.name for wheel in wheels) == (
        "poke_controller_modified_extension-0.1.0-py3-none-any.whl",
    )
    with zipfile.ZipFile(wheels[0]) as archive:
        members = tuple(archive.namelist())
        wheel_metadata_members = tuple(
            member for member in members if member.endswith(".dist-info/WHEEL")
        )
        assert len(wheel_metadata_members) == 1
        wheel_metadata = archive.read(wheel_metadata_members[0]).decode()
    assert "Root-Is-Purelib: true\n" in wheel_metadata
    assert "Tag: py3-none-any\n" in wheel_metadata
    assert not any(
        PurePosixPath(member).name.startswith("_native")
        or PurePosixPath(member).suffix in {".dll", ".dylib", ".pyd", ".so"}
        for member in members
    )


def test_first_party_native_extension_is_retired(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    root = Path(__file__).resolve().parents[2]
    assert not (root / "rust/pokecon-pybindings").exists()
    assert not tuple((root / "python/pokecon").glob("_native*"))

    active_contract = "\n".join(
        (root / relative).read_text(encoding="utf-8")
        for relative in (
            "Cargo.toml",
            "pyproject.toml",
            "uv.lock",
            "flake.nix",
            ".github/workflows/release.yml",
        )
    )
    for retired_marker in (
        "pokecon-pybindings",
        "pokecon._native",
        "maturin",
    ):
        assert retired_marker not in active_contract.lower()

    monkeypatch.setattr(sys, "path", [str(root / "python"), *sys.path])
    monkeypatch.delitem(sys.modules, "pokecon._native", raising=False)
    monkeypatch.delitem(sys.modules, "pokecon", raising=False)
    package = importlib.import_module("pokecon")
    assert package.__file__ is not None
    assert Path(package.__file__).resolve().is_relative_to(root / "python")
    with pytest.raises(ModuleNotFoundError):
        importlib.import_module("pokecon._native")


def test_interactive_cargo_task_isolation_contracts_remain() -> None:
    root = Path(__file__).resolve().parents[2]
    flake = (root / "flake.nix").read_text(encoding="utf-8")
    for contract in (
        "using isolated per-run Cargo target:",
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
    rust_check = flake.split(
        "          rustCoreCheck = pkgs.stdenv.mkDerivation {", maxsplit=1
    )[1].split("          contractSyncCheck =", maxsplit=1)[0]
    compatibility_check = flake.split(
        "          compatibilityCorpusCheck =", maxsplit=1
    )[1].split("          rustCoreDrvPath =", maxsplit=1)[0]
    assert "${cliHelpCheck.program}" not in aggregate_check
    assert "packaged CLI and UI use dedicated apps" in aggregate_check

    workspace_clippy = "cargo clippy --locked --profile test --workspace --all-targets --all-features --no-deps -- -D warnings"
    workspace_test = (
        "cargo test --locked --workspace --all-features --no-run \\\n"
        "                --message-format=json-render-diagnostics"
    )
    assert "reclaimPerRunCargoTarget" not in flake
    assert "cargo clean" not in flake
    linux_only_lld_export = (
        "${lib.optionalString pkgs.stdenv.isLinux "
        "\"export RUSTFLAGS='-C link-arg=-Wl,--threads=1'\"}"
    )
    dev_debug_export = "export CARGO_PROFILE_DEV_DEBUG="
    test_debug_export = "export CARGO_PROFILE_TEST_DEBUG=0"
    test_uv_export = 'export POKECON_TEST_UV="${pythonPackageBuildUv}/bin/uv"'
    api_type_check = "scripts/quality/generate-api-types.sh --check-types-only"
    assert "cargo build " not in aggregate_check
    assert workspace_clippy not in aggregate_check
    assert workspace_test not in aggregate_check
    assert "cargo build " not in rust_check
    assert rust_check.count(workspace_clippy) == 1
    assert rust_check.count(workspace_test) == 1
    assert rust_check.count("${setupUvLinks}") == 1
    assert aggregate_check.count("${realizeRustCiCore}") == 1
    assert "rustTaskInputs" not in aggregate_check
    assert "${setupQualityWorkdir}" in aggregate_check
    assert "${setupWorkdir}" not in aggregate_check
    assert aggregate_check.count("--jobs 1") == 0
    assert rust_check.count("--threads=1") == 1
    assert dev_debug_export not in rust_check
    assert rust_check.count(test_debug_export) == 1
    assert aggregate_check.count(test_uv_export) == 1
    assert "cargo test" not in aggregate_check
    assert "cargo test" not in compatibility_check
    assert "cargo run --locked --package pokecon --bin generate_contracts" not in (
        aggregate_check
    )
    assert aggregate_check.count(api_type_check) == 1
    assert (
        aggregate_check.index(test_uv_export)
        < aggregate_check.index("${realizeRustCiCore}")
        < aggregate_check.index(api_type_check)
    )
    assert (
        rust_check.index("${setupUvLinks}")
        < rust_check.index(test_debug_export)
        < rust_check.index(linux_only_lld_export)
        < rust_check.index(workspace_test)
        < rust_check.index(workspace_clippy)
    )

    for compatibility_binary in (
        '--compatibility-binary "${rustCoreCheck}/libexec/pokecon-compatibility"',
        '--worker "${rustCoreCheck}/libexec/pokecon-worker"',
    ):
        assert compatibility_check.count(compatibility_binary) == 1

    all_workspace_builds = tuple(
        line.strip()
        for line in flake.splitlines()
        if "cargo build " in line and "--workspace" in line
    )
    development_build = "cargo build --locked --workspace --all-features"
    assert all_workspace_builds == (development_build,)
    assert flake.count("--threads=1") == 1

    assert "ruff check --config ruff.toml --no-cache python scripts tests" not in (
        aggregate_check
    )
    assert (
        "ruff format --config ruff.toml --no-cache --check python scripts tests"
        not in aggregate_check
    )
    assert aggregate_check.count("treefmt --ci --working-dir") == 1
    assert flake.count("ruff-check.enable = true;") == 1
    assert flake.count("ruff-format.enable = true;") == 1


def test_native_rtc_tests_use_test_only_loopback_candidates() -> None:
    root = Path(__file__).resolve().parents[2]
    flake = (root / "flake.nix").read_text(encoding="utf-8")
    webrtc = (root / "rust/pokecon/src/server/webrtc.rs").read_text(encoding="utf-8")
    websocket = (root / "rust/pokecon/src/server/websocket.rs").read_text(
        encoding="utf-8"
    )
    rust_check = flake.split(
        "          rustCoreCheck = pkgs.stdenv.mkDerivation {", maxsplit=1
    )[1].split("          contractSyncCheck =", maxsplit=1)[0]

    for loopback_setting in (
        "#[cfg(test)]\n    let api_builder = {",
        "let mut setting_engine = webrtc::api::setting_engine::SettingEngine::default();",
        "setting_engine.set_include_loopback_candidate(true);",
        "setting_engine.set_ip_filter(Box::new(|ip| ip.is_loopback()));",
        "api_builder.with_setting_engine(setting_engine)",
    ):
        assert webrtc.count(loopback_setting) == 1
    assert webrtc.count("create_peer_connection(") == 3
    assert websocket.count("create_peer_connection(") == 1
    assert "acquire_loopback_test_lock" not in webrtc
    assert "acquire_loopback_test_lock" not in websocket
    assert rust_check.count("__darwinAllowLocalNetworking = pkgs.stdenv.isDarwin;") == 1
    assert rust_check.count("cargo test --locked --workspace --all-features") == 1
    for escaped_test in (
        "--skip server::webrtc::tests::loopback_transports_h264_and_isolated_data_channels",
        "--skip server::websocket::tests::realtime_connection_promotes_atomically_and_falls_back_without_closing_signaling",
        "pokecon-lib-tests",
    ):
        assert escaped_test not in flake


def test_rust_ci_split_executes_every_declared_non_contract_target_once() -> None:
    root = Path(__file__).resolve().parents[2]
    flake = (root / "flake.nix").read_text(encoding="utf-8")
    manifest = _read_toml(root / "rust/pokecon/Cargo.toml")
    workspace_manifest = _read_toml(root / "Cargo.toml")
    declared_tests = {test["name"] for test in manifest["test"]}
    rust_check = flake.split(
        "          rustCoreCheck = pkgs.stdenv.mkDerivation {", maxsplit=1
    )[1].split("          contractSyncCheck =", maxsplit=1)[0]
    inventory_contract = rust_check.split(
        "                def expected_targets: [", maxsplit=1
    )[1].split("                ];", maxsplit=1)[0]
    inventory_target_list = re.findall(
        r'^\s+name: "([a-z0-9_]+)",$', inventory_contract, flags=re.MULTILINE
    )
    inventory_targets = set(inventory_target_list)
    execution = rust_check.split(
        "# Execute the lib harness and every non-contract integration harness",
        maxsplit=1,
    )[1].split("cargo clippy --locked", maxsplit=1)[0]

    assert inventory_targets == {"pokecon", *declared_tests}
    assert len(inventory_target_list) == len(inventory_targets)
    assert workspace_manifest["workspace"]["members"] == ["rust/pokecon"]
    assert workspace_manifest["workspace"]["default-members"] == ["rust/pokecon"]
    assert manifest["lib"]["doctest"] is False
    assert all(binary.get("test") is False for binary in manifest["bin"])
    assert "example" not in manifest
    assert "bench" not in manifest
    assert rust_check.count("--no-run") == 1
    assert rust_check.count("--message-format=json-render-diagnostics") == 1
    assert rust_check.count("--all-targets") == 1
    assert rust_check.count("cargo test --locked") == 1
    assert "--doc" not in rust_check
    assert 'select(.name != "contract_sync")' in execution
    assert '"$test_executable"' in execution
    assert 'if [ "$executed_test_count" -ne 9 ]; then' in execution
    assert rust_check.count('install -m 0555 "$contract_test_executable"') == 1
    assert "src = rustCoreTestSource;" in rust_check

    rustdoc_code_fences = [
        f"{source.relative_to(root)}:{line_number}"
        for source in sorted((root / "rust/pokecon").rglob("*.rs"))
        for line_number, line in enumerate(
            source.read_text(encoding="utf-8").splitlines(), start=1
        )
        if "```" in line or "~~~" in line
    ]
    assert rustdoc_code_fences == []

    contract_check = flake.split("          contractSyncCheck =", maxsplit=1)[1].split(
        "          compatibilityCorpusCheck =", maxsplit=1
    )[0]
    assert "src = rustTestSource;" in contract_check
    assert 'export POKECON_CONTRACT_TEST_ROOT="$PWD"' in contract_check
    assert '"${rustCoreCheck}/libexec/contract-sync"' in contract_check

    contract_source = (root / "rust/pokecon/tests/contract_sync.rs").read_text(
        encoding="utf-8"
    )
    assert "include_str!" not in contract_source
    assert 'env::var_os("POKECON_CONTRACT_TEST_ROOT")' in contract_source


def test_aggregate_check_runtime_inputs_include_exactly_one_jq() -> None:
    root = Path(__file__).resolve().parents[2]
    flake = (root / "flake.nix").read_text(encoding="utf-8")
    aggregate_check = flake.split("            check = mkTask {", maxsplit=1)[1].split(
        "\n            };", maxsplit=1
    )[0]
    runtime_inputs = aggregate_check.split(
        "              runtimeInputs = [", maxsplit=1
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
        "pkgs.git",
        "pkgs.gnugrep",
        "pkgs.jq",
        "pkgs.markdownlint-cli",
        "pythonEnv",
        "pythonPackageBuildUv",
        "pkgs.ripgrep",
        "pkgs.shellcheck",
        "pkgs.textlint",
        "pkgs.textlint-rule-no-start-duplicated-conjunction",
        "pkgs.typos",
    )
    assert runtime_input_names.count("pkgs.jq") == 1
