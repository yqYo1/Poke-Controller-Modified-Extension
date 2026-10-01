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
    job_end = workflow.index("  production_perf:\n", job_start)
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


def test_normal_ci_production_perf_gate_is_blocking_and_artifact_backed() -> None:
    workflow = (REPOSITORY / ".github/workflows/normal-ci.yml").read_text(
        encoding="utf-8"
    )
    job_start = workflow.index("  production_perf:\n")
    job_end = workflow.index("  windows:\n", job_start)
    job = workflow[job_start:job_end]

    assert "name: Production main-path virtual performance (Linux)" in job
    assert "if: needs.plan.outputs.product == 'true'" in job
    assert "needs: plan" in job
    assert "runs-on: ubuntu-latest" in job
    assert "timeout-minutes: 20" in job
    assert "actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803" in job
    assert "cachix/install-nix-action@13d8dd58da0234aa297dedd986986ccb8e7f3e24" in job
    assert "Restore production perf Cargo target (read-only)" in job
    assert "id: production_perf_target_restore" in job
    assert "uses: actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9" in job
    assert "path: target/nix-tasks/release" in job
    assert (
        "key: pokecon-production-perf-target-v1-${{ runner.os }}-${{ github.sha }}"
        in job
    )
    assert "restore-keys:" not in job
    assert "Discover the same-SHA push production target artifact (read-only)" in job
    discover_start = job.index(
        "      - name: Discover the same-SHA push production target artifact (read-only)"
    )
    discover_end = job.index(
        "      - name: Download the same-SHA push production target artifact (read-only)",
        discover_start,
    )
    discover_step = job[discover_start:discover_end]
    assert "github.event_name == 'pull_request'" in discover_step
    assert (
        "github.event.pull_request.head.repo.full_name == github.repository"
        in discover_step
    )
    assert (
        "steps.production_perf_target_restore.outputs.cache-hit != 'true'"
        in discover_step
    )
    assert ".head_sha == $sha" in discover_step
    assert ".head_repository.full_name == $repo" in discover_step
    assert '.actor.login == "yqYo1"' in discover_step
    assert ".expired == false" in discover_step
    assert (
        "POKECON_PRODUCTION_PERF_HEAD_SHA: ${{ github.event.pull_request.head.sha }}"
        in job
    )
    assert "actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c" in job
    assert "path: target/nix-tasks/release" in job
    assert "Run the production main-path virtual performance gate" in job
    assert (
        "POKECON_PRODUCTION_PERF_OUT: ${{ github.workspace }}/production-perf-evidence"
        in job
    )
    assert (
        "POKECON_MAIN_PATH_TRACE_OUT: ${{ github.workspace }}/production-perf-evidence/main-path-trace"
        in job
    )
    assert "POKECON_PRODUCTION_PERF_SAMPLES: '300'" in job
    assert "POKECON_PRODUCTION_PERF_WARMUP_SECS: '60'" in job
    assert "POKECON_PRODUCTION_PERF_CYCLES: '300'" in job
    assert "POKECON_MAIN_PATH_TRACE_CYCLES: '300'" in job
    assert "POKECON_MAIN_PATH_TRACE_WARMUP_SECS: '60'" in job
    assert "POKECON_PERF_BUILD_SHA: ${{ github.sha }}" in job
    assert "nix run .#production-perf-check" in job
    assert "Save production perf Cargo target" in job
    save_start = job.index("      - name: Save production perf Cargo target")
    save_end = job.index(
        "      - name: Inspect production perf evidence directory", save_start
    )
    save_step = job[save_start:save_end]
    assert "github.event_name == 'push'" in save_step
    assert "contains(fromJSON('[\"yqYo1\"]'), github.actor)" in save_step
    assert (
        "steps.production_perf_target_restore.outputs.cache-hit != 'true'" in save_step
    )
    assert (
        "uses: actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9" in save_step
    )
    assert "Upload production perf Cargo target for same-SHA pull requests" in job
    target_upload_start = job.index(
        "      - name: Upload production perf Cargo target for same-SHA pull requests"
    )
    target_upload_end = job.index(
        "      - name: Inspect production perf evidence directory", target_upload_start
    )
    target_upload = job[target_upload_start:target_upload_end]
    assert "github.event_name == 'push'" in target_upload
    assert "contains(fromJSON('[\"yqYo1\"]'), github.actor)" in target_upload
    assert "name: pokecon-production-perf-target-${{ github.sha }}" in target_upload
    assert "path: target/nix-tasks/release" in target_upload
    assert "if-no-files-found: error" in target_upload
    assert "retention-days: 3" in target_upload
    assert "compression-level: 0" in target_upload
    assert "Inspect production perf evidence directory" in job
    assert "performance-report.json" in job
    assert "performance-samples.json" in job
    assert "production-perf.log" in job
    assert "main-path-trace/main-path-trace-report.json" in job
    assert "main-path-trace/main-path-trace-samples.json" in job
    assert "main-path-trace/main-path-trace.log" in job
    assert "production perf evidence file is missing" in job
    assert "production main-path trace evidence file is missing" in job
    assert "if: always()" in job
    assert "name: pokecon-production-perf-linux-${{ github.run_attempt }}" in job
    assert "path: ${{ github.workspace }}/production-perf-evidence/" in job
    assert "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a" in job
    assert "if-no-files-found: error" in job
    evidence_upload_start = job.index("      - name: Upload production perf evidence")
    evidence_upload = job[evidence_upload_start:]
    assert "retention-days" not in evidence_upload
    trace = (REPOSITORY / "rust/pokecon/tests/main_path_trace_virtual.rs").read_text(
        encoding="utf-8"
    )
    production_trace = (
        REPOSITORY / "rust/pokecon/tests/production_perf_virtual.rs"
    ).read_text(encoding="utf-8")
    assert "BLOCKING_P95_" in trace
    assert "BLOCKING_P95_" in production_trace
    assert '"mode": "blocking"' in trace
    assert '"mode": "blocking"' in production_trace
    assert '"status": "fixed-threshold"' in trace
    assert '"status": "fixed-threshold"' in production_trace
    assert '"blocking": true' in trace
    assert '"blocking": true' in production_trace
    gate_start = job.index(
        "      - name: Run the production main-path virtual performance gate"
    )
    assert "continue-on-error:" not in job[gate_start:]
    assert "|| true" not in job


def test_normal_ci_contract_gates_publish_acceptance_reports() -> None:
    workflow = (REPOSITORY / ".github/workflows/normal-ci.yml").read_text(
        encoding="utf-8"
    )
    job_start = workflow.index("  rust_contracts:\n")
    job_end = workflow.index("  python_tests:\n", job_start)
    job = workflow[job_start:job_end]

    assert (
        "POKECON_ACCEPTANCE_REPORT_DIR: ${{ github.workspace }}/acceptance-contract-evidence"
        in job
    )
    for report_name in (
        "schema-report.json",
        "boundary-report.json",
        "abstraction-report.json",
        "lifecycle-report.json",
    ):
        assert report_name in job
    assert (
        "ci-acceptance-reports-${{ github.run_id }}-${{ github.run_attempt }}-rust"
        in job
    )
    assert "path: ${{ github.workspace }}/acceptance-contract-evidence" in job
    assert "if-no-files-found: warn" in job


def test_normal_ci_required_aggregates_performance_result() -> None:
    workflow = (REPOSITORY / ".github/workflows/normal-ci.yml").read_text(
        encoding="utf-8"
    )
    required = workflow[workflow.index("  required:\n") :]

    assert "      - performance\n" in required
    assert "      - production_perf\n" in required
    assert (
        '"performance":{"applicable":${{ needs.plan.outputs.product == \'true\' }}'
        in required
    )
    assert (
        '"production_perf":{"applicable":${{ needs.plan.outputs.product == \'true\' }}'
        in required
    )
    assert "performance=${{ needs.performance.result }}" in required
    assert "production_perf=${{ needs.production_perf.result }}" in required
