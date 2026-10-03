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
    build_start = workflow.index("  production_perf_build:\n")
    build_end = workflow.index("  production_perf_target_upload:\n", build_start)
    build_job = workflow[build_start:build_end]
    upload_start = workflow.index("  production_perf_target_upload:\n")
    upload_end = workflow.index("  production_perf:\n", upload_start)
    upload_job = workflow[upload_start:upload_end]
    job_start = workflow.index("  production_perf:\n")
    job_end = workflow.index("  windows:\n", job_start)
    job = workflow[job_start:job_end]

    assert "name: Production performance target build (Linux)" in build_job
    assert "needs: plan" in build_job
    assert (
        "if: needs.plan.outputs.product == 'true' && github.event_name == 'push'"
        in build_job
    )
    assert "timeout-minutes: 20" in build_job
    assert "Prepare production perf Cargo target" in build_job
    assert "nix run .#production-perf-prepare" in build_job
    assert "POKECON_PERF_BUILD_SHA: ${{ github.sha }}" in build_job
    assert (
        "POKECON_PERF_TARGET_REUSE: ${{ steps.production_perf_target_restore.outputs.cache-hit == 'true' }}"
        in build_job
    )
    assert "Save production perf Cargo target" in build_job
    assert "Upload production perf Cargo target for same-SHA consumers" not in (
        build_job
    )
    assert "contains(fromJSON('[\"yqYo1\"]'), github.actor)" in build_job
    assert (
        "uses: actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9"
        in build_job
    )
    assert (
        "uses: actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9" in build_job
    )
    assert (
        "key: pokecon-production-perf-target-v1-${{ runner.os }}-${{ github.sha }}"
        in build_job
    )
    assert "restore-keys:" not in build_job
    build_save_start = build_job.index(
        "      - name: Save production perf Cargo target"
    )
    build_save_step = build_job[build_save_start:]
    assert "success()" in build_save_step
    assert "github.event_name == 'push'" in build_save_step
    assert "github.actor" in build_save_step
    assert (
        "steps.production_perf_target_restore.outputs.cache-hit != 'true'"
        in build_save_step
    )

    assert "name: Production performance target upload (Linux)" in upload_job
    assert "needs: [plan, production_perf_build]" in upload_job
    assert "needs.plan.outputs.product == 'true'" in upload_job
    assert "github.event_name == 'push'" in upload_job
    assert "needs.production_perf_build.result == 'success'" in upload_job
    assert "runs-on: ubuntu-latest" in upload_job
    assert "timeout-minutes: 20" in upload_job
    assert "Restore production perf Cargo target (read-only)" in upload_job
    assert (
        "uses: actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9"
        in upload_job
    )
    assert (
        "key: pokecon-production-perf-target-v1-${{ runner.os }}-${{ github.sha }}"
        in upload_job
    )
    assert "restore-keys:" not in upload_job
    assert "nix run" not in upload_job
    assert "actions/cache/save@" not in upload_job
    upload_step_start = upload_job.index(
        "      - name: Upload production perf Cargo target for same-SHA consumers"
    )
    upload_step = upload_job[upload_step_start:]
    assert "success()" in upload_step
    assert "github.event_name == 'push'" in upload_step
    assert "github.actor" in upload_step
    assert "contains(fromJSON('[\"yqYo1\"]'), github.actor)" in upload_step
    assert (
        "uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a"
        in upload_step
    )
    assert "name: pokecon-production-perf-target-${{ github.sha }}" in upload_step
    assert "path: target/nix-tasks/release" in upload_step
    assert "if-no-files-found: error" in upload_step
    assert "retention-days: 3" in upload_step
    assert "compression-level: 0" in upload_step
    assert "include-hidden-files: true" in upload_step

    assert "name: Production main-path virtual performance (Linux)" in job
    assert "needs: [plan, production_perf_build, production_perf_target_upload]" in (
        job
    )
    assert "always()" in job
    assert "needs.plan.outputs.product == 'true'" in job
    assert "runs-on: ubuntu-latest" in job
    assert "timeout-minutes: 20" in job
    assert "actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803" in job
    assert "id: production_perf_source" in job
    assert "github.event.pull_request.head.repo.full_name == github.repository" in job
    assert 'actual_source_sha="$(git rev-parse HEAD)"' in job
    assert "production perf checkout revision differs from expected source" in job
    assert "cachix/install-nix-action@13d8dd58da0234aa297dedd986986ccb8e7f3e24" in job
    assert "Restore production perf Cargo target (read-only)" in job
    assert "id: production_perf_target_restore" in job
    assert "uses: actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9" in job
    assert "path: target/nix-tasks/release" in job
    assert (
        "key: pokecon-production-perf-target-v1-${{ runner.os }}-${{ steps.production_perf_source.outputs.source_sha }}"
        in job
    )
    assert "restore-keys:" not in job
    assert "Download production perf Cargo target from push build (read-only)" in job
    assert "id: production_perf_build_target_download" in job
    assert "needs.production_perf_build.result == 'success'" in job
    assert "needs.production_perf_target_upload.result == 'success'" in job
    push_download_start = job.index(
        "      - name: Download production perf Cargo target from push build (read-only)"
    )
    push_download_end = job.index(
        "      - name: Discover the same-SHA push production target artifact (read-only)",
        push_download_start,
    )
    push_download_step = job[push_download_start:push_download_end]
    assert "github.event_name == 'push'" in push_download_step
    assert (
        "needs.production_perf_target_upload.result == 'success'" in push_download_step
    )
    assert "needs.production_perf_build.result" not in push_download_step
    assert (
        "steps.production_perf_target_restore.outputs.cache-hit != 'true'"
        in push_download_step
    )
    assert "continue-on-error: true" in job
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
        "POKECON_PRODUCTION_PERF_SOURCE_SHA: ${{ steps.production_perf_source.outputs.source_sha }}"
        in job
    )
    assert "actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c" in job
    assert "id: production_perf_target_download" in job
    assert "path: target/nix-tasks/release" in job
    assert "Restore Cargo executable bits lost by artifact transport" in job
    assert "if [ -d target/nix-tasks/release/build ]; then" in job
    assert "if [ -d target/nix-tasks/release/deps ]; then" in job
    assert "for executable_name in pokecon pokecon-worker; do" in job
    assert "target/nix-tasks/release/$executable_name" in job
    assert "steps.production_perf_build_target_download.outcome == 'success'" in job
    assert "steps.production_perf_target_download.outcome == 'success'" in job
    assert "find target/nix-tasks/release/build -type f -name build-script-build" in job
    assert "production_perf_virtual-*" in job
    assert "main_path_trace_virtual-*" in job
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
    assert (
        "POKECON_PERF_BUILD_SHA: ${{ steps.production_perf_source.outputs.source_sha }}"
        in job
    )
    assert "POKECON_PERF_TARGET_REUSE: >-" in job
    assert (
        "steps.production_perf_target_restore.outputs.cache-hit == 'true' || steps.production_perf_derivation_materialize.outputs.realized == 'true' || steps.production_perf_build_target_download.outcome == 'success' || steps.production_perf_target_download.outcome == 'success'"
        in job
    )
    assert "nix run .#production-perf-check" in job
    flake = (REPOSITORY / "flake.nix").read_text(encoding="utf-8")
    production_task_start = flake.index("productionPerfCheck = mkTask")
    production_task = flake[production_task_start:]
    assert (
        'production_perf_manifest="$CARGO_TARGET_DIR/release/.pokecon-production-perf-executables.json"'
        in production_task
    )
    assert "production_perf_reuse_ready=false" in production_task
    assert (
        "prepared Cargo target is unavailable; using cold Cargo build"
        in production_task
    )
    assert "prepared Cargo executable is unavailable" in production_task
    assert (
        "os.path.commonpath((target_root, prepared_path)) != target_root"
        in production_task
    )
    assert (
        "os.path.islink(prepared_path) or not os.path.isfile(prepared_path) or not os.access(prepared_path, os.X_OK)"
        in production_task
    )
    assert "POKECON_PERF_TARGET_REUSE" in production_task
    assert 'cargo" test --locked --release' in production_task
    assert (
        "--test production_perf_virtual --test main_path_trace_virtual --no-run"
        in production_task
    )
    assert "POKECON_PERF_PREPARE_ONLY" in production_task
    assert (
        "release test executables prepared; measurement phase skipped"
        in production_task
    )
    prepare_task_start = flake.index("productionPerfPrepare = mkTask")
    prepare_task = flake[prepare_task_start:]
    assert 'name = "production-perf-prepare"' in prepare_task
    assert "export POKECON_PERF_PREPARE_ONLY=true" in prepare_task
    assert 'exec "${productionPerfCheck.program}" "$@"' in prepare_task
    assert "Save production perf Cargo target" not in job
    assert "Upload production perf Cargo target for same-SHA pull requests" not in job
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

    assert "id: production_perf_derivation_key" in build_job
    assert "id: production_perf_derivation_restore" in build_job
    assert "id: production_perf_derivation" in build_job
    assert "id: production_perf_derivation_export" in build_job
    assert (
        "nix path-info --derivation .#checks.x86_64-linux.production-perf-target"
        in build_job
    )
    assert "invalid production perf derivation path" in build_job
    assert "key=pokecon-nix-v1-linux-production-perf-${target_drv##*/}" in build_job
    assert "pokecon-production-perf-derivation-cache" in build_job
    assert "restore-keys:" not in build_job
    assert "Realize production perf derivation (substitute or cold-build once)" in (
        build_job
    )
    assert "nix-store --realise" in build_job
    assert "nix build" not in build_job
    assert "materialized production perf manifest keys are not exact" in build_job
    assert "raw Cargo fallback runs with fail-closed validation" in build_job
    assert "steps.production_perf_derivation.outputs.realized != 'true'" in (build_job)
    assert "prepared Cargo executable manifest keys are not exact" in build_job
    assert "prepared Cargo executable escapes target directory" in build_job
    assert "nix store sign --key-file" in build_job
    assert "--recursive" in build_job
    assert "nix copy --to" in build_job
    assert "chmod 600" in build_job
    assert "POKECON_NIX_CACHE_SECRET_KEY" in build_job
    assert 'echo "$POKECON_NIX_CACHE_SECRET_KEY"' not in build_job
    assert "steps.production_perf_derivation_restore.outputs.cache-hit" in (build_job)
    assert "steps.production_perf_derivation.outputs.realized == 'true'" in (build_job)
    assert (
        "steps.production_perf_derivation_export.outputs.exported == 'true'"
        in build_job
    )
    assert "require-sigs = true" in build_job
    assert "pokecon-nix-cache-1:" in build_job
    assert "actions/cache/save@" not in upload_job
    assert "id: production_perf_derivation_key" in job
    assert "id: production_perf_derivation_restore" in job
    assert "id: production_perf_derivation_cache" in job
    assert "id: production_perf_derivation_materialize" in job
    assert "Materialize production perf derivation executables (substitute-only)" in job
    assert "steps.production_perf_derivation_cache.outputs.enabled == 'true'" in job
    assert 'nix-store --query --outputs "$derivation_drv"' in job
    assert 'cache_uri="file://$POKECON_NIX_CACHE_DIRECTORY?priority=20"' in job
    assert 'nix path-info --store "$cache_uri" --recursive "$output_path"' in job
    assert (
        'nix store verify --store "$cache_uri" --recursive --sigs-needed 1 --no-contents "$output_path"'
        in job
    )
    assert "cache_available=false" in job
    assert 'nix copy --from "$cache_uri" "$output_path"' in job
    assert 'nix-store --option fallback false --realise "$derivation_drv"' not in job
    assert "actions/cache/save@" not in job
    assert "nix store sign" not in job
    assert "nix copy --to" not in job
    assert "production-perf-derivation-cache" in job

    assert "productionPerfTarget = pkgs.stdenv.mkDerivation" in flake
    assert "checks.production-perf-target = productionPerfTarget;" in flake
    assert "src = rustCoreTestSource;" in flake
    assert "integration-test-support,worker-binary" in flake
    assert 'cargo" test --locked --release' in production_task
    assert "release/deps/production_perf_virtual-nix" in flake
    assert "release/deps/pokecon-nix" in flake
    assert "release/deps/main_path_trace_virtual-nix" in flake


def test_production_perf_target_inventory_filters_non_executable_artifacts() -> None:
    flake = (REPOSITORY / "flake.nix").read_text(encoding="utf-8")
    target_start = flake.index("productionPerfTarget = pkgs.stdenv.mkDerivation")
    extractor_start = flake.index("extract_production_perf_executable", target_start)
    target_section = flake[target_start:extractor_start]
    inventory_start = target_section.index('select(.reason == "compiler-artifact")')
    inventory = target_section[inventory_start:]
    executable_filter = 'select((.executable | type) == "string")'
    non_empty_filter = 'select(.executable != "")'
    test_profile_filter = ".profile.test == true"
    worker_target_filter = '.target.name == "pokecon-worker"'
    actual_anchor = inventory.index("unique_by([.name, .kind, .executable]) as $actual")
    assert executable_filter in inventory
    assert non_empty_filter in inventory
    assert test_profile_filter in inventory
    assert worker_target_filter in inventory
    assert inventory.index(executable_filter) < actual_anchor
    assert inventory.index(non_empty_filter) < actual_anchor
    assert inventory.index(test_profile_filter) < actual_anchor
    assert inventory.index(worker_target_filter) < actual_anchor
    assert "Cargo returned duplicate production perf target names" in target_section
    assert (
        "Cargo returned an empty or non-string production perf executable"
        in target_section
    )
    assert "Cargo returned duplicate production perf executable paths" in target_section


def test_production_perf_materialization_failure_is_fail_closed() -> None:
    workflow = (REPOSITORY / ".github/workflows/normal-ci.yml").read_text(
        encoding="utf-8"
    )
    build_start = workflow.index("  production_perf_build:\n")
    build_end = workflow.index("  production_perf_target_upload:\n", build_start)
    build_job = workflow[build_start:build_end]
    realize_start = build_job.index(
        "Realize production perf derivation (substitute or cold-build once)"
    )
    realize_end = build_job.index(
        "      - name: Restore production perf Cargo target (read-only)",
        realize_start,
    )
    realize_step = build_job[realize_start:realize_end]
    assert (
        "production perf derivation realized but materialization failed; "
        "failing closed without raw Cargo fallback" in realize_step
    )
    failure_index = realize_step.index("failing closed without raw Cargo fallback")
    assert "exit 2" in realize_step[failure_index : failure_index + 200]
    assert (
        "production perf derivation materialization failed; "
        "raw Cargo fallback runs with fail-closed validation"
    ) not in build_job
    assert (
        "production perf derivation is unavailable; "
        "raw Cargo fallback runs with fail-closed validation" in realize_step
    )
    assert "steps.production_perf_derivation.outputs.realized != 'true'" in (build_job)


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
    assert "      - production_perf_build\n" in required
    assert "      - production_perf_target_upload\n" in required
    assert "      - production_perf\n" in required
    assert (
        '"performance":{"applicable":${{ needs.plan.outputs.product == \'true\' }}'
        in required
    )
    assert (
        "\"production_perf_build\":{\"applicable\":${{ github.event_name == 'push' && needs.plan.outputs.product == 'true' }}"
        in required
    )
    assert (
        "\"production_perf_target_upload\":{\"applicable\":${{ github.event_name == 'push' && needs.plan.outputs.product == 'true' }}"
        in required
    )
    assert (
        '"production_perf":{"applicable":${{ needs.plan.outputs.product == \'true\' }}'
        in required
    )
    assert "production_perf_build=${{ needs.production_perf_build.result }}" in required
    assert (
        "production_perf_target_upload=${{ needs.production_perf_target_upload.result }}"
        in required
    )
    assert "performance=${{ needs.performance.result }}" in required
    assert "production_perf=${{ needs.production_perf.result }}" in required
