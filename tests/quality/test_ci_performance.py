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
    assert "Resolve recent passing performance baselines" in job
    assert "CURRENT_EVENT: ${{ github.event_name }}" in job
    assert "CURRENT_BRANCH: ${{ github.head_ref || github.ref_name }}" in job
    assert "CURRENT_PR_NUMBER: ${{ github.event.pull_request.number || '' }}" in job
    assert "CURRENT_FIXTURE_ID: browser-loopback-v2" in job
    assert "PERFORMANCE_BASELINE: ${{ needs.plan.outputs.performance_baseline }}" in job
    assert (
        "No browser performance fixture or runner input changed; skipping relative performance baseline comparison"
        in job
    )
    assert 'runs_json="$baseline_root/runs.json"' in job
    assert '--slurpfile runs "$runs_json"' in job
    assert "($runs[0].workflow_runs" in job
    assert 'select(($events[$run_id].event // "") == $current_event)' in job
    assert 'select(($events[$run_id].conclusion // "") == "success")' in job
    assert 'select(($events[$run_id].head_branch // "") == $current_branch)' in job
    assert "pull_request_numbers" in job
    assert "$current_pr_number | tonumber" in job
    assert "and .fixture_id == $current_fixture_id" in job
    assert "group_by(.workflow_run.head_sha)" in job
    assert "baseline_limit=10" in job
    assert "selected_dir/$(printf '%02d' \"$selected_count\").json" in job
    assert 'if [ "$selected_count" -ge "$baseline_limit" ]' in job
    assert "until $baseline_limit are available" in job
    assert 'select((.workflow_run.head_sha // "") != $current_revision)' in job
    assert "POKECON_PERFORMANCE_FIXTURE_ID: browser-loopback-v2" in job
    assert "POKECON_PERFORMANCE_SOURCE_COMMIT: ${{ github.sha }}" in job
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
    webrtc_index = runner.index(
        "metrics.webrtc_video_latency = await collectWebRtc(loopback);"
    )
    close_index = runner.index("closeWebRtcLoopback(loopback);")
    mjpeg_index = runner.index("metrics.mjpeg_video_latency = await collectMjpeg();")
    assert webrtc_index < close_index < mjpeg_index
    assert "loopback.captureTrack.stop();" in runner
    assert "loopback.track.stop();" in runner
    assert runner.count("loopback.sender.close();") == 1
    assert runner.count("loopback.receiver.close();") == 1
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
