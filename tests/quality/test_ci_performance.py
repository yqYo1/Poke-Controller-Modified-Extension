from __future__ import annotations

from pathlib import Path

REPOSITORY = Path(__file__).resolve().parents[2]


def test_normal_ci_performance_gate_is_blocking_and_artifact_backed() -> None:
    workflow = (REPOSITORY / ".github/workflows/normal-ci.yml").read_text(
        encoding="utf-8"
    )
    runner = (REPOSITORY / "scripts/performance/benchmark.py").read_text(
        encoding="utf-8"
    )
    job_start = workflow.index("  performance:\n")
    job_end = workflow.index("  windows:\n", job_start)
    job = workflow[job_start:job_end]

    assert "name: Browser primitive performance smoke (Linux)" in job
    assert "if: needs.plan.outputs.product == 'true'" in job
    assert "needs: plan" in job
    assert "needs: [plan, product_flake]" not in job
    assert "nix path-info --derivation .#packages.x86_64-linux.pokecon" in job
    assert "Build the release product used for the fixture identity" not in job
    assert "nix run .#performance-check --" in job
    assert "--samples 300" in job
    assert "--warmup-seconds 60" in job
    assert "--timeout-seconds 600" in job
    assert (
        'gh api \\\n              "/repos/${GITHUB_REPOSITORY}/actions/artifacts/${artifact_id}/zip" \\\n              > "$zip_path"'
        in job
    )
    assert "gh api --output" not in job
    assert "if: always()" in job
    assert "performance-record.json" in job
    assert "path: ${{ github.workspace }}/performance-evidence/" in job
    for artifact_name in (
        "performance-record.json",
        "performance-report.json",
        "performance-samples.json",
        "performance-browser.log",
    ):
        assert artifact_name in runner
    assert "captureStream(60)" in runner
    assert "stream_fps: 60" in runner
    assert "if-no-files-found: error" in job


def test_normal_ci_required_aggregates_performance_result() -> None:
    workflow = (REPOSITORY / ".github/workflows/normal-ci.yml").read_text(
        encoding="utf-8"
    )
    required = workflow[workflow.index("  required:\n") :]

    assert "      - performance\n" in required
    assert (
        '"performance":{"applicable":${{ needs.plan.outputs.product == \'true\' }}'
        in required
    )
    assert "performance=${{ needs.performance.result }}" in required
