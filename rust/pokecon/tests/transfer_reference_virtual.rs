//! §7.9.6 CI-only reference benchmark: anonymous-pipe `MessagePack` transfer
//! versus shared-memory plus worker-side copy.
//!
//! This makes the specification's comparison executable instead of leaving
//! the prose reference observations (28.23 ms vs 3.28 ms) as prose-only.
//! Fixed conditions per `SPECIFICATION_BACKEND.md` §7.9.6: one known
//! 1920x1080 frame, 50 warm-up iterations (discarded), 200 measured
//! iterations per arm, current Linux host, CI-only mock/virtual I/O (no
//! physical devices).
//!
//! Arm A ("anonymous-pipe-msgpack") encodes the known frame as `MessagePack`
//! `bin` (framed directly per the `bin` family: `0xC6` + length + bytes),
//! pushes the bytes through a real OS anonymous pipe (`std::io::pipe`) from
//! a writer thread, and reads + decodes + byte-verifies them on the main
//! thread. The timed span covers encode, pipe transfer, decode, and
//! verification.
//! Framing is an 8-byte little-endian length prefix followed by the `MessagePack`
//! body. The body is raw `MessagePack` `bin`, not the §7.8 control-plane
//! envelope: a 1920x1080 BGR frame cannot fit that envelope's 1 MiB
//! (`MAX_PAYLOAD_BYTES`) limit, so the envelope is not a candidate carrier.
//!
//! Arm B ("shm-worker-copy") publishes the same known frame through the
//! production [`SharedFrameRing`] (§7.9.1 path: writer `publish`) and copies
//! it out through a second handle opened from the published
//! [`MappingDescriptor`] (§7.9.3 worker mapping flow) via `read_published`,
//! which performs the worker-side private copy. The timed span covers
//! publish plus the worker-side copy, including byte verification.
//!
//! The prose numbers in §7.9.6 are recorded in the report as
//! informational-only reference observations and are never asserted: this
//! harness gates only on fail-closed invariants (exact sample counts, finite
//! non-negative durations, per-iteration byte equality with the known frame,
//! explicit source/build identity, units, and schema shape). There is no
//! performance-threshold gate; this is a reference measurement, not a
//! production-performance acceptance.

use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::Instant;

use pokecon::integration_test_support::camera::{BgrFrame, CaptureResolution, SharedFrameRing};

/// Artifact identity for this harness.
const FIXTURE_ID: &str = "transfer-reference-v1";
/// Spec section this harness executes.
const SPEC_SECTION: &str = "7.9.6";
/// Fixed frame geometry per §7.9.6.
const FRAME_WIDTH: u32 = 1920;
const FRAME_HEIGHT: u32 = 1080;
/// Discarded warm-up iterations per arm.
const WARMUP_ITERS: usize = 50;
/// Measured iterations per arm (exact sample count gate).
const MEASURED_ITERS: usize = 200;
/// Total pipe iterations per arm (warm-up followed by measured).
const TOTAL_ITERS: usize = WARMUP_ITERS + MEASURED_ITERS;
/// `MessagePack` `bin 32` header marker (`0xC6`) followed by a big-endian
/// byte length and the raw bytes. A trial run through the workspace
/// `rmp-serde` round-trip measured ~1.1 s/frame in debug (per-element
/// overhead dominates at 6.2 MiB), so the single-frame body is framed here
/// directly per the `MessagePack` `bin` family instead. The pipe bytes are
/// genuine `MessagePack`, verifiable by any compliant decoder, and no new
/// dependency is introduced (the lockfile is pinned).
const MSG_BIN32: u8 = 0xC6;

/// Encodes one frame as `MessagePack` `bin 32`.
fn encode_bin_frame(frame_bytes: &[u8]) -> Vec<u8> {
    let length = u32::try_from(frame_bytes.len()).expect("frame must fit `MessagePack` bin32");
    let mut body = Vec::with_capacity(frame_bytes.len() + 5);
    body.push(MSG_BIN32);
    body.extend_from_slice(&length.to_be_bytes());
    body.extend_from_slice(frame_bytes);
    body
}

/// Decodes one `MessagePack` `bin 32` body fail-closed, returning the raw frame
/// bytes.
fn decode_bin_frame(body: &[u8], iteration: usize) -> &[u8] {
    assert!(
        body.len() >= 5,
        "iteration {iteration}: `MessagePack` body must carry a bin32 header"
    );
    assert_eq!(
        body[0], MSG_BIN32,
        "iteration {iteration}: `MessagePack` body must open with bin32"
    );
    let declared = usize::try_from(u32::from_be_bytes(
        body[1..5].try_into().expect("bin32 length is four bytes"),
    ))
    .expect("bin32 length must fit usize");
    assert_eq!(
        declared,
        body.len() - 5,
        "iteration {iteration}: MessagePack bin32 length must match the body"
    );
    &body[5..]
}
/// Reference observations quoted from `SPECIFICATION_BACKEND.md` §7.9.6 prose.
/// Informational only; never asserted or gated on.
const PROSE_REFERENCE_PIPE_MS: f64 = 28.23;
const PROSE_REFERENCE_SHM_MS: f64 = 3.28;

/// Deterministic known frame: all arms transfer these exact bytes.
/// Pixel `(x, y, c)` carries `(x * 31 + y * 17 + c * 7) % 256`, so the buffer
/// is incompressible-friendly, non-trivial, and fully reproducible.
fn known_frame_bytes() -> Vec<u8> {
    let mut pixels = Vec::with_capacity(FRAME_WIDTH as usize * FRAME_HEIGHT as usize * 3);
    for y in 0..FRAME_HEIGHT {
        for x in 0..FRAME_WIDTH {
            for channel in 0..3u32 {
                let value = (x * 31 + y * 17 + channel * 7) % 256;
                pixels.push(u8::try_from(value).expect("gradient residue must fit u8"));
            }
        }
    }
    pixels
}

/// Wrapping checksum used as a cheap pre-flight identity check for the known
/// frame (per-iteration correctness is proven by full byte equality).
fn checksum(bytes: &[u8]) -> u64 {
    let mut total = 0u64;
    for byte in bytes {
        total = total.wrapping_add(u64::from(*byte));
    }
    total
}

/// Nearest-rank quantile, mirroring `nearest_rank` in
/// `scripts/performance/benchmark.py` (`index = max(0, ceil(q*n) - 1)`).
/// Callers pass the quantile as `numerator/denominator` (p95 = 95/100).
fn nearest_rank(sorted_ms: &[f64], numerator: u64, denominator: u64) -> f64 {
    assert!(!sorted_ms.is_empty(), "cannot rank an empty sample set");
    assert!(denominator > 0, "quantile denominator must be positive");
    let count = u64::try_from(sorted_ms.len()).expect("sample count must fit u64");
    let rank = (numerator * count).div_ceil(denominator);
    sorted_ms[usize::try_from(rank.saturating_sub(1))
        .expect("rank must fit usize")
        .min(sorted_ms.len() - 1)]
}

struct ArmSummary {
    count: usize,
    mean_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    maximum_ms: f64,
    duration_ms: f64,
    throughput_per_second: f64,
}

/// Validates exact sample count plus finite non-negative samples, then
/// summarizes. Fail-closed: any violation panics.
fn summarize(samples_ms: &[f64], arm: &str) -> ArmSummary {
    assert_eq!(
        samples_ms.len(),
        MEASURED_ITERS,
        "{arm} must hold exactly {MEASURED_ITERS} samples"
    );
    let mut sorted = samples_ms.to_vec();
    sorted.sort_by(f64::total_cmp);
    for (index, sample) in sorted.iter().enumerate() {
        assert!(
            sample.is_finite() && *sample >= 0.0,
            "{arm}.samples[{index}] must be a finite non-negative number"
        );
    }
    let duration_ms: f64 = sorted.iter().sum();
    assert!(
        duration_ms > 0.0,
        "{arm} total duration must be positive for throughput"
    );
    let count = u32::try_from(sorted.len()).expect("sample count must fit u32");
    ArmSummary {
        count: sorted.len(),
        mean_ms: duration_ms / f64::from(count),
        p50_ms: nearest_rank(&sorted, 50, 100),
        p95_ms: nearest_rank(&sorted, 95, 100),
        p99_ms: nearest_rank(&sorted, 99, 100),
        maximum_ms: sorted[sorted.len() - 1],
        duration_ms,
        throughput_per_second: f64::from(count) / (duration_ms / 1000.0),
    }
}

/// Runs arm A: `MessagePack` encode, anonymous-pipe transfer, decode, verify.
/// Returns the measured (post-warm-up) per-iteration latencies in ms.
fn run_pipe_arm(frame_bytes: &[u8]) -> Vec<f64> {
    let expected_checksum = checksum(frame_bytes);
    let (reader, mut writer) = std::io::pipe().expect("anonymous pipe must open");
    // One writer thread for all iterations; the pipe itself plus the channel
    // preserve per-iteration ordering, so no extra synchronization is needed.
    let (sender, receiver) = mpsc::channel::<Vec<u8>>();
    let writer_thread = std::thread::spawn(move || {
        for body in receiver {
            let length = u64::try_from(body.len()).expect("MessagePack body must fit u64");
            writer
                .write_all(&length.to_le_bytes())
                .expect("pipe length-prefix write must succeed");
            writer
                .write_all(&body)
                .expect("pipe body write must succeed");
        }
    });

    let mut measured_ms = Vec::with_capacity(MEASURED_ITERS);
    let mut expected_body_len: Option<usize> = None;
    for iteration in 0..TOTAL_ITERS {
        let started = Instant::now();
        let body = encode_bin_frame(frame_bytes);
        sender.send(body).expect("writer thread must be alive");
        let mut prefix = [0u8; 8];
        let mut pipe_reader = &reader;
        pipe_reader
            .read_exact(&mut prefix)
            .expect("pipe length-prefix read must succeed");
        let body_len =
            usize::try_from(u64::from_le_bytes(prefix)).expect("pipe length prefix must fit usize");
        match expected_body_len {
            None => expected_body_len = Some(body_len),
            Some(expected) => assert_eq!(
                body_len, expected,
                "iteration {iteration}: pipe body length must be deterministic"
            ),
        }
        let mut body = vec![0u8; body_len];
        pipe_reader
            .read_exact(&mut body)
            .expect("pipe body read must succeed");
        let decoded = decode_bin_frame(&body, iteration);
        assert_eq!(
            checksum(decoded),
            expected_checksum,
            "iteration {iteration}: pipe transfer must preserve frame bytes"
        );
        assert_eq!(
            decoded, frame_bytes,
            "iteration {iteration}: pipe transfer must be byte-exact"
        );
        if iteration >= WARMUP_ITERS {
            measured_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        }
    }
    drop(sender);
    writer_thread.join().expect("writer thread must settle");
    measured_ms
}

/// Runs arm B: production ring publish plus worker-side copy via a
/// descriptor-opened handle. Returns measured latencies in ms.
fn run_shm_arm(frame: &BgrFrame) -> Vec<f64> {
    let expected: &[u8] = frame.pixels();
    let expected_checksum = checksum(expected);
    let owner = SharedFrameRing::create(CaptureResolution::R1920x1080).expect("ring must create");
    // Worker side: map the same region once through the published descriptor,
    // exactly the §7.9.3 "descriptor sent once, worker re-maps" flow.
    let worker = SharedFrameRing::open(owner.descriptor()).expect("ring must open");
    let mut measured_ms = Vec::with_capacity(MEASURED_ITERS);
    for iteration in 0..TOTAL_ITERS {
        let started = Instant::now();
        owner.publish(frame).expect("publish must succeed");
        let copied = worker
            .read_published()
            .expect("worker-side read must succeed")
            .expect("publication must be present");
        assert_eq!(
            checksum(copied.pixels()),
            expected_checksum,
            "iteration {iteration}: shared-memory copy must preserve frame bytes"
        );
        assert_eq!(
            copied.pixels(),
            expected,
            "iteration {iteration}: shared-memory copy must be byte-exact"
        );
        if iteration >= WARMUP_ITERS {
            measured_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        }
    }
    measured_ms
}

/// Resolves a non-empty source/build identity for every report. The Nix
/// rust-core check injects a content-addressed source-store identity; a local
/// worktree falls back to its current Git revision. The final manifest-path
/// fallback still identifies the source location when Git is unavailable, but
/// is never represented as `unknown`.
fn source_build_identity() -> String {
    if let Ok(identity) = std::env::var("POKECON_PERF_BUILD_SHA")
        && !identity.trim().is_empty()
        && identity != "unknown"
    {
        return identity;
    }
    if let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        && output.status.success()
    {
        let revision = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !revision.is_empty() {
            return revision;
        }
    }
    format!(
        "manifest:{}@{}",
        env!("CARGO_MANIFEST_DIR"),
        env!("CARGO_PKG_VERSION")
    )
}

fn runner_identity() -> String {
    std::env::var("RUNNER_NAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|value| !value.trim().is_empty() && value != "unknown")
        .unwrap_or_else(|| "local".to_owned())
}

fn framing_entry() -> serde_json::Value {
    serde_json::json!({
        "messagepack_body": "bin32",
        "outer_pipe_length_prefix": "u64 little-endian byte length",
        "production_worker_codec_used": false,
        "production_worker_codec_reason": "§7.8 control framing is a 4-byte big-endian length prefix with a 1 MiB MAX_PAYLOAD_BYTES limit; the 6220800-byte frame is intentionally outside that control-plane envelope",
        "spec_sections": ["7.8.2", "7.8.6", "7.9.6"],
        "interpretation": "synthetic anonymous-pipe reference arm, not a production §7.8 worker-codec measurement",
    })
}

fn arm_entry(name: &str, method: &str, summary: &ArmSummary) -> serde_json::Value {
    serde_json::json!({
        "arm": name,
        "method": method,
        "unit": "ms",
        "sample_count": summary.count,
        "warmup_iterations_discarded": WARMUP_ITERS,
        "mean": summary.mean_ms,
        "p50": summary.p50_ms,
        "p95": summary.p95_ms,
        "p99": summary.p99_ms,
        "maximum": summary.maximum_ms,
        "statistics": {
            "duration_ms": summary.duration_ms,
            "throughput_per_second": summary.throughput_per_second,
            "quantile_method": "nearest-rank (index = max(0, ceil(q*n)-1)), mirroring scripts/performance/benchmark.py",
        },
    })
}

fn build_report(
    pipe: &ArmSummary,
    shm: &ArmSummary,
    pipe_samples: &[f64],
    shm_samples: &[f64],
) -> serde_json::Value {
    let build_identity = source_build_identity();
    let runner = runner_identity();
    serde_json::json!({
        "fixture_id": FIXTURE_ID,
        "spec_section": SPEC_SECTION,
        "comparison": ["anonymous-pipe-msgpack", "shm-worker-copy"],
        "framing": framing_entry(),
        "frame": {
            "width": FRAME_WIDTH,
            "height": FRAME_HEIGHT,
            "channels": 3,
            "byte_len": u64::from(FRAME_WIDTH) * u64::from(FRAME_HEIGHT) * 3,
            "encoding": "deterministic gradient ((x*31 + y*17 + c*7) % 256)",
        },
        "iterations": {
            "warmup_discarded_per_arm": WARMUP_ITERS,
            "measured_per_arm": MEASURED_ITERS,
        },
        "unit": "ms",
        "arms": [
            arm_entry(
                "anonymous-pipe-msgpack",
                "MessagePack bin32 encode, OS anonymous-pipe transfer (writer thread), decode, byte-verify",
                pipe
            ),
            arm_entry(
                "shm-worker-copy",
                "SharedFrameRing::publish plus descriptor-opened read_published worker-side copy, byte-verify",
                shm
            ),
        ],
        "reference_observations": {
            "anonymous_pipe_msgpack_ms": PROSE_REFERENCE_PIPE_MS,
            "shm_worker_copy_ms": PROSE_REFERENCE_SHM_MS,
            "prose_reference": true,
            "asserted": false,
            "note": "SPECIFICATION_BACKEND.md §7.9.6 prose values; informational context only, never gated on",
        },
        "observed": {
            "pipe_mean_ms": pipe.mean_ms,
            "shm_mean_ms": shm.mean_ms,
            "pipe_over_shm_ratio": pipe.mean_ms / shm.mean_ms,
        },
        "header": {
            "build_sha": build_identity.clone(),
            "source_build_identity": build_identity,
            "runner": runner,
            "package_version": env!("CARGO_PKG_VERSION"),
        },
        "evaluation": {
            "mode": "reference-only",
            "result": "pass",
            "notes": "fail-closed invariants (exact sample counts, finite non-negative durations, per-iteration byte equality) are required; no performance-threshold gate",
        },
        "sample_counts": {
            "anonymous_pipe_msgpack_ms": pipe_samples.len(),
            "shm_worker_copy_ms": shm_samples.len(),
        },
    })
}

fn artifact_paths() -> (String, String, String) {
    let out_dir =
        std::env::var("POKECON_TRANSFER_REFERENCE_OUT").unwrap_or_else(|_| "target".to_owned());
    (
        out_dir.clone(),
        format!("{out_dir}/transfer-reference-report.json"),
        format!("{out_dir}/transfer-reference-samples.json"),
    )
}

fn write_artifacts(
    report: &serde_json::Value,
    pipe_samples: &[f64],
    shm_samples: &[f64],
) -> (String, String) {
    let (out_dir, report_path, samples_path) = artifact_paths();
    std::fs::create_dir_all(&out_dir).expect("reference output dir must be creatable");
    std::fs::write(
        &report_path,
        serde_json::to_string_pretty(report).expect("report must serialize"),
    )
    .expect("report must be writable");
    let samples = serde_json::json!({
        "fixture_id": FIXTURE_ID,
        "measured_per_arm": MEASURED_ITERS,
        "unit": "ms",
        "samples": {
            "anonymous_pipe_msgpack_ms": pipe_samples,
            "shm_worker_copy_ms": shm_samples,
        },
    });
    std::fs::write(
        &samples_path,
        serde_json::to_string_pretty(&samples).expect("samples must serialize"),
    )
    .expect("samples must be writable");
    println!(
        "transfer-reference: pipe mean={:.3}ms p95={:.3}ms | shm mean={:.3}ms p95={:.3}ms (n={MEASURED_ITERS} each, warm-up {WARMUP_ITERS} discarded)",
        report["observed"]["pipe_mean_ms"],
        report["arms"][0]["p95"],
        report["observed"]["shm_mean_ms"],
        report["arms"][1]["p95"],
    );
    eprintln!("transfer-reference artifacts: {report_path} {samples_path}");
    (report_path, samples_path)
}

/// Re-reads the report artifact and validates its schema fail-closed.
fn assert_report_shape(path: &str) -> serde_json::Value {
    let raw = std::fs::read_to_string(path).expect("report must be readable");
    let parsed: serde_json::Value = serde_json::from_str(&raw).expect("report must be valid JSON");
    assert_eq!(parsed["fixture_id"], FIXTURE_ID, "report fixture id");
    assert_eq!(parsed["spec_section"], SPEC_SECTION, "report spec section");
    assert_eq!(parsed["unit"], "ms", "report unit");
    assert_eq!(parsed["framing"]["messagepack_body"], "bin32");
    assert_eq!(
        parsed["framing"]["outer_pipe_length_prefix"],
        "u64 little-endian byte length"
    );
    assert_eq!(
        parsed["framing"]["production_worker_codec_used"], false,
        "the synthetic pipe arm must not be mistaken for §7.8 worker-codec evidence"
    );
    assert_eq!(
        parsed["framing"]["spec_sections"],
        serde_json::json!(["7.8.2", "7.8.6", "7.9.6"])
    );
    for key in ["build_sha", "source_build_identity", "runner"] {
        let value = parsed["header"][key].as_str().unwrap_or_default().trim();
        assert!(
            !value.is_empty() && value != "unknown",
            "header.{key} must identify the run"
        );
    }
    assert_eq!(parsed["frame"]["width"], FRAME_WIDTH, "report frame width");
    assert_eq!(
        parsed["frame"]["height"], FRAME_HEIGHT,
        "report frame height"
    );
    assert_eq!(
        parsed["iterations"]["measured_per_arm"], MEASURED_ITERS,
        "report measured iterations"
    );
    assert_eq!(
        parsed["iterations"]["warmup_discarded_per_arm"], WARMUP_ITERS,
        "report warm-up iterations"
    );
    let arms = parsed["arms"].as_array().expect("arms must be an array");
    assert_eq!(arms.len(), 2, "two comparison arms");
    for entry in arms {
        assert_eq!(entry["unit"], "ms", "arm unit");
        assert_eq!(entry["sample_count"], MEASURED_ITERS, "arm sample count");
        assert_eq!(
            entry["warmup_iterations_discarded"], WARMUP_ITERS,
            "arm warm-up count"
        );
        for key in ["mean", "p50", "p95", "p99", "maximum"] {
            let value = entry[key].as_f64().unwrap_or(f64::NAN);
            assert!(
                value.is_finite() && value >= 0.0,
                "arm statistic {key} must be finite non-negative"
            );
        }
    }
    // The prose reference values must stay informational: any future edit
    // that starts gating on them trips this invariant.
    assert_eq!(
        parsed["reference_observations"]["asserted"], false,
        "prose reference values must never become asserted"
    );
    assert_eq!(
        parsed["reference_observations"]["prose_reference"], true,
        "prose reference values must stay labeled"
    );
    assert_eq!(
        parsed["evaluation"]["mode"], "reference-only",
        "reference harness must not threshold-gate"
    );
    parsed
}

/// Re-reads and validates the raw-sample artifact, then checks that its
/// recomputed summaries agree with the separately stored report. A passing
/// report must therefore be backed by both readable JSON files, not only by
/// in-memory vectors that were present before serialization.
fn assert_samples_shape(path: &str, report: &serde_json::Value) {
    let raw = std::fs::read_to_string(path).expect("samples must be readable");
    let parsed: serde_json::Value = serde_json::from_str(&raw).expect("samples must be valid JSON");
    assert_eq!(parsed["fixture_id"], FIXTURE_ID, "samples fixture id");
    assert_eq!(parsed["measured_per_arm"], MEASURED_ITERS, "samples count");
    assert_eq!(parsed["unit"], "ms", "samples unit");
    let sample_sets = parsed["samples"].as_object().expect("samples map");
    let keys = ["anonymous_pipe_msgpack_ms", "shm_worker_copy_ms"];
    let arms = report["arms"].as_array().expect("report arms");
    assert_eq!(arms.len(), keys.len(), "report/sample arm count");
    for (index, key) in keys.iter().enumerate() {
        let values = sample_sets[*key].as_array().expect("sample arm array");
        assert_eq!(values.len(), MEASURED_ITERS, "sample arm length for {key}");
        let samples: Vec<f64> = values
            .iter()
            .enumerate()
            .map(|(sample_index, value)| {
                let value = value
                    .as_f64()
                    .unwrap_or_else(|| panic!("{key}[{sample_index}] must be a number"));
                assert!(
                    value.is_finite() && value >= 0.0,
                    "{key}[{sample_index}] must be finite non-negative"
                );
                value
            })
            .collect();
        let summary = summarize(&samples, key);
        let report_arm = &arms[index];
        for (field, actual) in [
            ("mean", summary.mean_ms),
            ("p50", summary.p50_ms),
            ("p95", summary.p95_ms),
            ("p99", summary.p99_ms),
            ("maximum", summary.maximum_ms),
        ] {
            let expected = report_arm[field]
                .as_f64()
                .unwrap_or_else(|| panic!("report arms[{index}].{field} must be a number"));
            let tolerance = f64::EPSILON * actual.abs().max(expected.abs()).max(1.0) * 8.0;
            assert!(
                (actual - expected).abs() <= tolerance,
                "report arms[{index}].{field} must match serialized samples: actual={actual:?}, expected={expected:?}, tolerance={tolerance:?}"
            );
        }
    }
}

#[test]
fn transfer_reference_7_9_6() {
    let frame_bytes = known_frame_bytes();
    assert_eq!(
        frame_bytes.len(),
        FRAME_WIDTH as usize * FRAME_HEIGHT as usize * 3,
        "known frame must fill 1920x1080x3"
    );
    let frame = BgrFrame::new(FRAME_WIDTH, FRAME_HEIGHT, frame_bytes.clone())
        .expect("known frame must validate");

    let pipe_samples = run_pipe_arm(&frame_bytes);
    let shm_samples = run_shm_arm(&frame);
    let pipe = summarize(&pipe_samples, "anonymous-pipe-msgpack");
    let shm = summarize(&shm_samples, "shm-worker-copy");

    let report = build_report(&pipe, &shm, &pipe_samples, &shm_samples);
    let (report_path, samples_path) = write_artifacts(&report, &pipe_samples, &shm_samples);
    let report = assert_report_shape(&report_path);
    assert_samples_shape(&samples_path, &report);
}

/// Exact float comparison for statistics unit tests: `nearest_rank` returns
/// an input sample unchanged, so bitwise equality is the correct assertion
/// (and satisfies the pedantic float-comparison lint).
fn assert_exact_float(actual: f64, expected: f64) {
    assert_eq!(
        actual.to_bits(),
        expected.to_bits(),
        "statistic must be bit-exact"
    );
}

#[test]
fn stats_nearest_rank_matches_benchmark_script() {
    // Mirrors the worked nearest-rank examples: n=5, p50 -> index 2,
    // p95/p99 -> index 4, p100 -> last element.
    let sorted = [1.0, 2.0, 3.0, 4.0, 5.0];
    assert_exact_float(nearest_rank(&sorted, 50, 100), 3.0);
    assert_exact_float(nearest_rank(&sorted, 95, 100), 5.0);
    assert_exact_float(nearest_rank(&sorted, 99, 100), 5.0);
    assert_exact_float(nearest_rank(&sorted, 100, 100), 5.0);
    // n=200 (the harness sample count): p50 -> index 99.
    let sorted200: Vec<f64> = (0..200).map(f64::from).collect();
    assert_exact_float(nearest_rank(&sorted200, 50, 100), 99.0);
    assert_exact_float(nearest_rank(&sorted200, 95, 100), 189.0);
}

#[test]
fn stats_summarize_enforces_exact_finite_samples() {
    let samples = vec![1.0; MEASURED_ITERS];
    let summary = summarize(&samples, "test-arm");
    assert_eq!(summary.count, MEASURED_ITERS);
    assert_exact_float(summary.mean_ms, 1.0);
    assert_exact_float(summary.p50_ms, 1.0);
}

#[test]
#[should_panic(expected = "must hold exactly 200 samples")]
fn stats_summarize_rejects_short_sample_set() {
    let samples = vec![1.0; MEASURED_ITERS - 1];
    let _ = summarize(&samples, "test-arm");
}

#[test]
#[should_panic(expected = "must be a finite non-negative number")]
fn stats_summarize_rejects_non_finite_sample() {
    let mut samples = vec![1.0; MEASURED_ITERS];
    samples[7] = f64::NAN;
    let _ = summarize(&samples, "test-arm");
}

#[test]
fn msgpack_bin_framing_round_trips_and_rejects_tampering() {
    let payload = vec![7u8; 1024];
    let body = encode_bin_frame(&payload);
    assert_eq!(body.len(), 1024 + 5, "bin32 overhead must be five bytes");
    assert_eq!(body[0], MSG_BIN32);
    assert_eq!(decode_bin_frame(&body, 0), payload.as_slice());
    let mut truncated = body.clone();
    truncated.pop();
    let result = std::panic::catch_unwind(|| decode_bin_frame(&truncated, 0));
    assert!(result.is_err(), "truncated body must be rejected");
    let mut wrong_marker = body.clone();
    wrong_marker[0] = 0xC4;
    let result = std::panic::catch_unwind(|| decode_bin_frame(&wrong_marker, 0));
    assert!(result.is_err(), "wrong MessagePack marker must be rejected");
}

#[test]
fn known_frame_is_deterministic_and_full_size() {
    let first = known_frame_bytes();
    let second = known_frame_bytes();
    assert_eq!(
        first.len(),
        1920usize * 1080 * 3,
        "known frame must be a full 1920x1080 BGR buffer"
    );
    assert_eq!(first, second, "known frame must be deterministic");
    assert_eq!(checksum(&first), checksum(&second));
    // Spot-check the gradient definition at (x=1, y=0): 31, 38, 45.
    assert_eq!(&first[3..6], &[31, 38, 45]);
    assert_ne!(checksum(&first), 0, "known frame must be non-trivial");
}
