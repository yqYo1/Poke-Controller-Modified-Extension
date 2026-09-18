from __future__ import annotations

from pathlib import Path

REPOSITORY = Path(__file__).resolve().parents[2]
WORKFLOW = REPOSITORY / ".github/workflows/normal-ci.yml"
SMOKE_SCRIPT = REPOSITORY / "scripts/ci/remote-flake-smoke.sh"


def _job_section(workflow: str, job_name: str) -> str:
    lines = workflow.splitlines(keepends=True)
    start = next(
        index for index, line in enumerate(lines) if line == f"  {job_name}:\n"
    )
    end = next(
        (
            index
            for index in range(start + 1, len(lines))
            if lines[index].startswith("  ") and not lines[index].startswith("    ")
        ),
        len(lines),
    )
    return "".join(lines[start:end])


def test_remote_flake_job_is_checkout_free_and_sha_pinned() -> None:
    workflow = WORKFLOW.read_text(encoding="utf-8")
    remote_job = _job_section(workflow, "remote_flake")
    product_job = _job_section(workflow, "product_flake")

    assert "actions/checkout" not in remote_job
    assert (
        "remote_revision=\"${{ github.event_name == 'pull_request' && github.event.pull_request.head.sha || github.sha }}\""
        in remote_job
    )
    assert (
        '"github:${{ github.repository }}/$remote_revision#remote-flake-smoke"'
        in remote_job
    )
    assert '--repository "${{ github.repository }}"' in remote_job
    assert '--revision "$remote_revision"' in remote_job
    assert "--option download-attempts 10" in remote_job
    assert "http-connections" not in remote_job
    assert "nix run ." not in remote_job
    assert "actions/checkout@v6" in product_job


def test_required_aggregate_tracks_remote_flake_job_separately() -> None:
    workflow = WORKFLOW.read_text(encoding="utf-8")
    required = workflow[workflow.index("  required:\n") :]

    assert "      - remote_flake\n" in required
    assert (
        '"remote_flake":{"applicable":${{ needs.plan.outputs.remote_flake == \'true\' }}'
        in required
    )
    assert "remote_flake=${{ needs.remote_flake.result }}" in required
    assert "github:${GITHUB_REPOSITORY}/${remote_revision}#pokecon" in required


def test_remote_smoke_script_isolation_and_runtime_checks_are_explicit() -> None:
    script = SMOKE_SCRIPT.read_text(encoding="utf-8")

    assert 'mktemp -d "$HOME/pokecon-remote-flake-smoke.XXXXXX"' in script
    for directory in (
        '"$test_root/home"',
        '"$test_root/config"',
        '"$test_root/data"',
        '"$test_root/cache"',
        '"$test_root/state"',
        '"$test_root/runtime"',
    ):
        assert directory in script
    assert "XDG_CONFIG_HOME=$test_root/config" in script
    assert "XDG_DATA_HOME=$test_root/data" in script
    assert "XDG_CACHE_HOME=$test_root/cache" in script
    assert "XDG_STATE_HOME=$test_root/state" in script
    assert 'app_environment+=("POKECON_PORT=$port")' in script
    assert "nix_network_options=(" in script
    assert "--option download-attempts 10" in script
    assert "http-connections" not in script
    assert "curl --silent --output /dev/null --connect-timeout 1" in script
    assert 'nix build \\\n  "${nix_network_options[@]}" "$flake_ref#pokecon"' in script
    assert "nix path-info ./result" in script
    assert 'nix run \\\n  "${nix_network_options[@]}" "$flake_ref" -- --help' in script
    assert '[ -s "$test_root/help.txt" ]' in script
    assert "grep -q '^Usage: pokecon' \"$test_root/help.txt\"" in script
    assert '"$base_url/"' in script
    assert '"$base_url/api/settings"' in script
    assert "grep -oE '/_app/" in script
    assert "asset_count=$((asset_count + 1))" in script
