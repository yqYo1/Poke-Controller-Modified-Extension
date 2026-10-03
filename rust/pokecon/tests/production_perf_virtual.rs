//! Production-main-path virtual-I/O perf harness with `fixture_id`
//! `production-virtual-v1` (wave-E fold-2a).
//!
//! This exercises the production [`CameraManager`]/frame-consumer path and the
//! production [`SerialManager`]/controller-output path end to end using the
//! same software-virtual backends as `concurrent_camera_serial_load.rs`
//! (`VirtualCameraBackend`, `VirtualSerialBackend`). No `/dev/video*` nodes,
//! kernel modules, or physical ports are touched, so the fixture runs on a
//! clean `ubuntu-latest` runner.
//!
//! Fixed conditions: 1920x1080 known frames (`RecordedFrame::Solid` plus a
//! checker variant for a non-trivial recognition workload), release profile
//! for evidence (`--release`; debug builds are sanity only), monotonic
//! `Instant` spans, 60s wall-clock warm-up + 300 samples per streaming metric
//! in full mode.
//!
//! Mode defaults by build profile: release builds run full evidence mode
//! (60s wall-clock warm-up, 300 samples per streaming metric, 300
//! stop/recovery cycles); debug builds run fast sanity (30 samples, no
//! wall-clock warm-up, 3 cycles) because debug numbers are not evidence,
//! which keeps broader debug suites that execute this harness cheap.
//!
//! Metrics: `camera_frame_latency`, `serial_send_latency`,
//! `recognition_latency` (pixel-SAD stand-in over full-frame bytes, labeled
//! as such in the report), derived `frame_throughput`/`serial_throughput`,
//! plus `shutdown_latency` (serial disconnect + camera shutdown) and
//! `recovery_latency` (fresh start + serial reconnect to first fresh frame).
//! Statistics port the ~40-line nearest-rank core from
//! `scripts/performance/benchmark.py` (`nearest_rank`, `_metric_summary`):
//! p50/p95/maximum, p99 detail, throughput = N/(duration/1000),
//! jitter = p99-p50.
//!
//! Threshold policy: fixed p95 budgets are part of the required gate. The
//! budgets are intentionally generous absolute limits for this virtual-I/O
//! fixture; every sample count, error count, schema invariant, and threshold
//! comparison is blocking. No mutable baseline or runner-local substitution
//! can turn a threshold failure into a pass.
//!
//! Knobs (all optional): `POKECON_PRODUCTION_PERF_OUT` (artifact dir,
//! default `target/` under the crate directory, i.e. `rust/pokecon/target/`),
//! `POKECON_PRODUCTION_PERF_FAST=1|0` (force fast sanity or full mode),
//! `POKECON_PRODUCTION_PERF_SAMPLES` /
//! `POKECON_PRODUCTION_PERF_WARMUP_SECS` / `POKECON_PRODUCTION_PERF_CYCLES`
//! (explicit per-value overrides), `POKECON_PERF_BUILD_SHA` (report header),
//! `POKECON_PERF_TARGET_REUSE` (whether the release executable set was
//! reused from a verified same-SHA target handoff).

use std::sync::Arc;
use std::time::{Duration, Instant};

use pokecon::integration_test_support::camera::{
    BgrFrame, CameraConfig, CameraManager, CameraSelector, CaptureResolution, FlipMode,
    RecordedFrame, VirtualCameraBackend, VirtualOpenPlan, VirtualSessionPlan,
};
use pokecon::integration_test_support::device::controller::{Button, ControllerState};
use pokecon::integration_test_support::device::serial::{
    ControllerFormat, SerialConfig, SerialManager, VirtualOpenPlan as SerialVirtualOpenPlan,
    VirtualSerialBackend, VirtualSerialEndpoint,
};

/// Artifact identity for this harness.
const FIXTURE_ID: &str = "production-virtual-v1";
/// Single session resolution for warm-up, measurement, and recovery.
const PERF_RESOLUTION: CaptureResolution = CaptureResolution::R1920x1080;
/// Full-mode streaming samples per metric (mirrors `SAMPLE_COUNT=300`).
const FULL_SAMPLES: usize = 300;
/// Fast-mode streaming samples (debug-build default, or `FAST=1`).
const FAST_SAMPLES: usize = 30;
/// Full-mode wall-clock warm-up, discarded before measurement.
const FULL_WARMUP_SECS: u64 = 60;
/// Timed warm-up iterations used when the wall-clock warm-up is disabled.
const TIMED_WARMUP_ITERS: usize = 10;
/// Full-mode stop/recovery cycles, so shutdown/recovery also reach 300 samples.
const FULL_CYCLES: usize = 300;
/// Fast-mode stop/recovery cycles.
const FAST_CYCLES: usize = 3;
/// Bounded polls for a published frame per streaming iteration.
const ACQUIRE_POLLS: usize = 100;
/// Per-cycle bound waiting for the first fresh frame after a restart.
const RECOVERY_POLL_TIMEOUT: Duration = Duration::from_secs(10);
/// Bound for each measured shutdown (disconnect + writer termination).
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
/// Known solid color the session frames are derived from.
const SOLID_BGR: [u8; 3] = [9, 18, 27];
/// Contrasting color for the non-trivial checker variant.
const CHECKER_ALT_BGR: [u8; 3] = [200, 150, 100];
/// Checker tile edge in pixels; 1920x1080 factors into 16x9 tiles of 120px.
const CHECKER_TILE: u32 = 120;

/// Required p95 budgets (milliseconds) for the release virtual-I/O gate.
const BLOCKING_P95_CAMERA_FRAME_MS: f64 = 10.0;
const BLOCKING_P95_SERIAL_SEND_MS: f64 = 50.0;
const BLOCKING_P95_RECOGNITION_MS: f64 = 500.0;
const BLOCKING_P95_SHUTDOWN_MS: f64 = 5_000.0;
const BLOCKING_P95_RECOVERY_MS: f64 = 30_000.0;

fn controller_with(button: Button) -> ControllerState {
    let mut state = ControllerState::NEUTRAL;
    state.buttons.set(button, true);
    state
}

/// Non-trivial 1920x1080 recognition surface: 120px checker tiles alternating
/// the session solid color with a contrasting color.
fn build_checker_frame() -> BgrFrame {
    let (width, height) = (1920, 1080);
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 3);
    for y in 0..height {
        for x in 0..width {
            let alt = (x / CHECKER_TILE + y / CHECKER_TILE) % 2 == 1;
            pixels.extend_from_slice(if alt { &CHECKER_ALT_BGR } else { &SOLID_BGR });
        }
    }
    BgrFrame::new(width, height, pixels).expect("checker pixels must fill 1920x1080")
}

/// Expected SAD of the checker frame against [`SOLID_BGR`], recomputed at
/// tile level so it stays independent of the pixel loop above.
fn expected_checker_sad() -> u64 {
    let (cols, rows) = (1920 / CHECKER_TILE, 1080 / CHECKER_TILE);
    let mut alt_tiles = 0u64;
    for row in 0..rows {
        for col in 0..cols {
            if (col + row) % 2 == 1 {
                alt_tiles += 1;
            }
        }
    }
    let per_pixel: u64 = CHECKER_ALT_BGR
        .iter()
        .zip(SOLID_BGR.iter())
        .map(|(actual, baseline)| u64::from(actual.abs_diff(*baseline)))
        .sum();
    alt_tiles * u64::from(CHECKER_TILE) * u64::from(CHECKER_TILE) * per_pixel
}

/// Pixel-SAD stand-in for template matching over the latest frame bytes.
///
/// This is a measurement workload, NOT the real recognition path: a
/// content-independent full-frame pass (sum of per-byte absolute differences
/// against the known solid color) with no new dependencies.
fn sad_vs_solid(pixels: &[u8]) -> u64 {
    let mut total = 0u64;
    for (index, byte) in pixels.iter().enumerate() {
        total += u64::from(byte.abs_diff(SOLID_BGR[index % 3]));
    }
    total
}

/// Nearest-rank quantile, mirroring `nearest_rank` in
/// `scripts/performance/benchmark.py` (`index = max(0, ceil(q*n) - 1)`).
/// Callers pass the quantile as `numerator/denominator` (p95 = 95/100); the
/// ceiling division keeps the rank exact with no float/int casts.
fn nearest_rank(sorted_ms: &[f64], numerator: u64, denominator: u64) -> f64 {
    assert!(!sorted_ms.is_empty(), "cannot rank an empty sample set");
    assert!(denominator > 0, "quantile denominator must be positive");
    let count = u64::try_from(sorted_ms.len()).expect("sample count must fit u64");
    let rank = (numerator * count).div_ceil(denominator);
    sorted_ms[usize::try_from(rank.saturating_sub(1))
        .expect("rank must fit usize")
        .min(sorted_ms.len() - 1)]
}

struct MetricSummary {
    count: usize,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    maximum_ms: f64,
    jitter_p99_minus_p50_ms: f64,
    throughput_per_second: f64,
    duration_ms: f64,
}

/// Per-metric summary mirroring `_metric_summary` in
/// `scripts/performance/benchmark.py`: exact sample count, finite
/// non-negative samples, p50/p95/maximum, p99/throughput/duration detail.
fn summarize(samples_ms: &[f64], metric: &str, expected_count: usize) -> MetricSummary {
    assert_eq!(
        samples_ms.len(),
        expected_count,
        "{metric} must hold exactly {expected_count} samples"
    );
    let mut sorted = samples_ms.to_vec();
    sorted.sort_by(f64::total_cmp);
    for (index, sample) in sorted.iter().enumerate() {
        assert!(
            sample.is_finite() && *sample >= 0.0,
            "{metric}.samples[{index}] must be a finite non-negative number"
        );
    }
    let p50_ms = nearest_rank(&sorted, 50, 100);
    let p95_ms = nearest_rank(&sorted, 95, 100);
    let p99_ms = nearest_rank(&sorted, 99, 100);
    let maximum_ms = sorted[sorted.len() - 1];
    let duration_ms = sorted.iter().sum();
    assert!(
        duration_ms > 0.0,
        "{metric} total duration must be positive for throughput"
    );
    let sample_count = u32::try_from(sorted.len()).expect("sample count must fit u32");
    MetricSummary {
        count: sorted.len(),
        p50_ms,
        p95_ms,
        p99_ms,
        maximum_ms,
        jitter_p99_minus_p50_ms: p99_ms - p50_ms,
        throughput_per_second: f64::from(sample_count) / (duration_ms / 1000.0),
        duration_ms,
    }
}

struct RunConfig {
    streaming_samples: usize,
    warmup_secs: u64,
    cycles: usize,
    fast: bool,
}

fn env_usize(name: &str, default: usize) -> usize {
    match std::env::var(name) {
        Ok(raw) => raw
            .parse()
            .unwrap_or_else(|_| panic!("{name} must be a usize, saw {raw:?}")),
        Err(_) => default,
    }
}

fn env_u64(name: &str, default: u64) -> u64 {
    match std::env::var(name) {
        Ok(raw) => raw
            .parse()
            .unwrap_or_else(|_| panic!("{name} must be a u64, saw {raw:?}")),
        Err(_) => default,
    }
}

/// Resolves the run mode: `POKECON_PRODUCTION_PERF_FAST` (`1`/`0`) forces
/// fast sanity or full evidence mode; unset defaults by build profile
/// (debug builds: fast sanity, release builds: full mode).
fn run_config() -> RunConfig {
    let fast = match std::env::var("POKECON_PRODUCTION_PERF_FAST").as_deref() {
        Ok("1") => true,
        Ok("0") => false,
        Ok(other) => panic!("POKECON_PRODUCTION_PERF_FAST must be 0 or 1, saw {other:?}"),
        Err(_) => cfg!(debug_assertions),
    };
    let config = RunConfig {
        streaming_samples: env_usize(
            "POKECON_PRODUCTION_PERF_SAMPLES",
            if fast { FAST_SAMPLES } else { FULL_SAMPLES },
        ),
        warmup_secs: env_u64(
            "POKECON_PRODUCTION_PERF_WARMUP_SECS",
            if fast { 0 } else { FULL_WARMUP_SECS },
        ),
        cycles: env_usize(
            "POKECON_PRODUCTION_PERF_CYCLES",
            if fast { FAST_CYCLES } else { FULL_CYCLES },
        ),
        fast,
    };
    assert!(config.streaming_samples >= 1, "samples must be positive");
    assert!(config.cycles >= 1, "cycles must be positive");
    config
}

fn camera_config() -> CameraConfig {
    CameraConfig::new(CameraSelector::Index(0), 30, PERF_RESOLUTION)
        .expect("1080p virtual camera config must build")
}

fn serial_config() -> SerialConfig {
    SerialConfig::new("production-perf-loopback", 9600, ControllerFormat::Default)
        .expect("loopback serial config must build")
}

fn session_plan(checker: &BgrFrame) -> VirtualSessionPlan {
    VirtualSessionPlan::recorded(
        30,
        [
            RecordedFrame::Solid(SOLID_BGR),
            RecordedFrame::Frame(checker.clone()),
        ],
    )
}

struct Fixture {
    camera: CameraManager,
    serial: SerialManager,
    backend: Arc<VirtualCameraBackend>,
    checker_sad: u64,
}

async fn start_fixture(checker: &BgrFrame, cycles: usize) -> Fixture {
    let backend = Arc::new(VirtualCameraBackend::default());
    for _ in 0..=cycles {
        backend.push_open(VirtualOpenPlan::Success(session_plan(checker)));
    }
    let camera = CameraManager::start(backend.clone(), camera_config(), FlipMode::None)
        .expect("virtual capture must start");
    assert!(
        camera.status().camera_opened,
        "virtual capture must open before measurement"
    );

    let serial_backend = Arc::new(VirtualSerialBackend::default());
    // NOTE: each plan needs its own endpoint: `close()` cancels the
    // endpoint permanently, so a reconnect must open a pristine handle
    // (mirroring a reopened physical port).
    for _ in 0..=cycles {
        serial_backend
            .push_plan(SerialVirtualOpenPlan::Success(VirtualSerialEndpoint::new()))
            .await;
    }
    let serial = SerialManager::new(serial_backend);
    serial
        .apply_config(serial_config())
        .await
        .expect("loopback serial must open");

    let checker_frame = BgrFrame::solid(PERF_RESOLUTION, SOLID_BGR);
    assert_eq!(
        sad_vs_solid(checker_frame.pixels()),
        0,
        "SAD stand-in must score zero on the known solid color"
    );
    Fixture {
        camera,
        serial,
        backend,
        checker_sad: expected_checker_sad(),
    }
}

async fn warm_up(fixture: &Fixture, config: &RunConfig) {
    let frames = fixture.camera.frame_source();
    if config.warmup_secs > 0 {
        let deadline = Instant::now() + Duration::from_secs(config.warmup_secs);
        let mut index = 0usize;
        while Instant::now() < deadline {
            let button = if index.is_multiple_of(2) {
                Button::A
            } else {
                Button::B
            };
            fixture
                .serial
                .send_controller_state(controller_with(button))
                .await
                .expect("warm-up send must succeed");
            if let Some(media) = frames.latest() {
                let _ = sad_vs_solid(media.frame.pixels());
            }
            index += 1;
        }
        eprintln!(
            "production-perf warm-up: {index} iterations over {}s",
            config.warmup_secs
        );
    } else {
        for index in 0..TIMED_WARMUP_ITERS {
            let button = if index.is_multiple_of(2) {
                Button::A
            } else {
                Button::B
            };
            fixture
                .serial
                .send_controller_state(controller_with(button))
                .await
                .expect("warm-up send must succeed");
            if let Some(media) = frames.latest() {
                let _ = sad_vs_solid(media.frame.pixels());
            }
        }
        eprintln!("production-perf warm-up: {TIMED_WARMUP_ITERS} timed iterations (fast mode)");
    }
}

/// Streaming-phase samples, all in milliseconds.
struct StreamOutcome {
    serial: Vec<f64>,
    acquire: Vec<f64>,
    recognition: Vec<f64>,
}

async fn collect_stream_samples(fixture: &Fixture, config: &RunConfig) -> StreamOutcome {
    let frames = fixture.camera.frame_source();
    let mut serial_ms = Vec::with_capacity(config.streaming_samples);
    let mut acquire_ms = Vec::with_capacity(config.streaming_samples);
    let mut recognition_ms = Vec::with_capacity(config.streaming_samples);
    let mut errors = 0u64;
    for index in 0..config.streaming_samples {
        let button = if index.is_multiple_of(2) {
            Button::A
        } else {
            Button::B
        };
        let started = Instant::now();
        if fixture
            .serial
            .send_controller_state(controller_with(button))
            .await
            .is_ok()
        {
            serial_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        } else {
            errors += 1;
            continue;
        }

        let started = Instant::now();
        let mut observed = None;
        for _ in 0..ACQUIRE_POLLS {
            if let Some(media) = frames.latest() {
                observed = Some(media);
                break;
            }
            tokio::task::yield_now().await;
        }
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        let Some(media) = observed else {
            errors += 1;
            continue;
        };
        acquire_ms.push(elapsed_ms);

        let started = Instant::now();
        let sad = sad_vs_solid(media.frame.pixels());
        recognition_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        // Every published session frame is either the solid frame (SAD 0) or
        // the checker variant; anything else means the fixture drifted.
        assert!(
            sad == 0 || sad == fixture.checker_sad,
            "recognition SAD must match a known session frame"
        );
    }
    assert_eq!(errors, 0, "streaming phase must complete without errors");
    StreamOutcome {
        serial: serial_ms,
        acquire: acquire_ms,
        recognition: recognition_ms,
    }
}

async fn wait_first_frame(camera: &CameraManager) {
    let frames = camera.frame_source();
    let deadline = Instant::now() + RECOVERY_POLL_TIMEOUT;
    loop {
        if frames.latest().is_some() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "restarted capture must publish a fresh frame within {RECOVERY_POLL_TIMEOUT:?}"
        );
        tokio::task::yield_now().await;
    }
}

/// Stop/recovery-phase samples, all in milliseconds.
struct CycleOutcome {
    shutdown: Vec<f64>,
    recovery: Vec<f64>,
}

/// Repeated stop/recovery around the production paths: per cycle, time the
/// serial disconnect + camera shutdown, then a fresh start + serial reconnect
/// to the first fresh frame. Ends with a running fixture for teardown.
async fn collect_stop_recovery(fixture: &mut Fixture, config: &RunConfig) -> CycleOutcome {
    let mut shutdown_ms = Vec::with_capacity(config.cycles);
    let mut recovery_ms = Vec::with_capacity(config.cycles);
    for _ in 0..config.cycles {
        let started = Instant::now();
        fixture
            .serial
            .disconnect()
            .await
            .expect("serial disconnect must succeed");
        fixture
            .camera
            .shutdown(SHUTDOWN_TIMEOUT)
            .expect("camera shutdown must settle");
        shutdown_ms.push(started.elapsed().as_secs_f64() * 1000.0);

        let started = Instant::now();
        let camera = CameraManager::start(fixture.backend.clone(), camera_config(), FlipMode::None)
            .expect("virtual capture must restart");
        assert!(
            camera.status().camera_opened,
            "restarted capture must reopen (open plans exhausted?)"
        );
        fixture
            .serial
            .connect()
            .await
            .expect("serial reconnect must succeed");
        wait_first_frame(&camera).await;
        // Prove the recovered serial path carries traffic, not just opens.
        fixture
            .serial
            .send_controller_state(controller_with(Button::A))
            .await
            .expect("recovered serial path must carry a send");
        recovery_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        fixture.camera = camera;
    }
    CycleOutcome {
        shutdown: shutdown_ms,
        recovery: recovery_ms,
    }
}

struct Summaries {
    camera: MetricSummary,
    serial: MetricSummary,
    recognition: MetricSummary,
    shutdown: MetricSummary,
    recovery: MetricSummary,
}

fn summarize_all(streams: &StreamOutcome, cycles: &CycleOutcome, config: &RunConfig) -> Summaries {
    Summaries {
        camera: summarize(
            &streams.acquire,
            "camera_frame_latency",
            config.streaming_samples,
        ),
        serial: summarize(
            &streams.serial,
            "serial_send_latency",
            config.streaming_samples,
        ),
        recognition: summarize(
            &streams.recognition,
            "recognition_latency",
            config.streaming_samples,
        ),
        shutdown: summarize(&cycles.shutdown, "shutdown_latency", config.cycles),
        recovery: summarize(&cycles.recovery, "recovery_latency", config.cycles),
    }
}

fn metric_entry(
    name: &str,
    unit: &str,
    summary: &MetricSummary,
    threshold_p95: f64,
    method: &str,
) -> serde_json::Value {
    serde_json::json!({
        "metric": name,
        "unit": unit,
        "sample_count": summary.count,
        "p50": summary.p50_ms,
        "p95": summary.p95_ms,
        "maximum": summary.maximum_ms,
        "statistics": {
            "p99": summary.p99_ms,
            "jitter_p99_minus_p50": summary.jitter_p99_minus_p50_ms,
            "throughput_per_second": summary.throughput_per_second,
            "duration_ms": summary.duration_ms,
            "method": method,
        },
        "threshold": {
            "threshold_p95": threshold_p95,
            "blocking": true,
            "exceeded": summary.p95_ms > threshold_p95,
        },
    })
}

fn one_measurement(
    name: &str,
    summary: &MetricSummary,
    threshold_p95: f64,
    method: &str,
) -> serde_json::Value {
    metric_entry(name, "ms", summary, threshold_p95, method)
}

fn measurement_entries(summaries: &Summaries) -> serde_json::Value {
    serde_json::json!([
        one_measurement(
            "camera_frame_latency",
            &summaries.camera,
            BLOCKING_P95_CAMERA_FRAME_MS,
            "in-process LatestFrameSource::latest() acquisition (Arc clone, no pixel copy)"
        ),
        one_measurement(
            "serial_send_latency",
            &summaries.serial,
            BLOCKING_P95_SERIAL_SEND_MS,
            "send_controller_state until the write completes on the virtual endpoint"
        ),
        one_measurement(
            "recognition_latency",
            &summaries.recognition,
            BLOCKING_P95_RECOGNITION_MS,
            "pixel-SAD stand-in for template matching over latest-frame bytes (NOT the real recognition path)"
        ),
        one_measurement(
            "shutdown_latency",
            &summaries.shutdown,
            BLOCKING_P95_SHUTDOWN_MS,
            "serial disconnect (neutral frame + close) plus CameraManager::shutdown"
        ),
        one_measurement(
            "recovery_latency",
            &summaries.recovery,
            BLOCKING_P95_RECOVERY_MS,
            "fresh CameraManager::start plus serial reconnect to first fresh frame and one proving send"
        ),
    ])
}

fn threshold_entries(measurements: &serde_json::Value) -> Vec<serde_json::Value> {
    measurements
        .as_array()
        .expect("measurements must be an array")
        .iter()
        .map(|entry| {
            serde_json::json!({
                "metric": entry["metric"],
                "observed_p95": entry["p95"],
                "threshold_p95": entry["threshold"]["threshold_p95"],
                "blocking": true,
                "exceeded": entry["threshold"]["exceeded"],
            })
        })
        .collect()
}

fn mode_entry(config: &RunConfig) -> serde_json::Value {
    serde_json::json!({
        "profile": std::env::var("POKECON_PRODUCTION_PERF_PROFILE").unwrap_or_else(|_| "test".to_owned()),
        "streaming_samples_per_metric": config.streaming_samples,
        "warmup_seconds": config.warmup_secs,
        "shutdown_recovery_cycles": config.cycles,
        "fast_sanity": config.fast,
    })
}

fn target_reuse() -> bool {
    match std::env::var("POKECON_PERF_TARGET_REUSE") {
        Ok(value) if value == "1" || value.eq_ignore_ascii_case("true") => true,
        Ok(value) if value == "0" || value.eq_ignore_ascii_case("false") => false,
        Ok(value) => panic!("POKECON_PERF_TARGET_REUSE must be true or false, saw {value:?}"),
        Err(_) => false,
    }
}

fn header_entry() -> serde_json::Value {
    serde_json::json!({
        "build_sha": std::env::var("POKECON_PERF_BUILD_SHA").unwrap_or_else(|_| "unknown".to_owned()),
        "runner": std::env::var("RUNNER_NAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "unknown".to_owned()),
        "package_version": env!("CARGO_PKG_VERSION"),
        "profile": std::env::var("POKECON_PRODUCTION_PERF_PROFILE").unwrap_or_else(|_| "test".to_owned()),
        "target_reuse": target_reuse(),
    })
}

fn build_report(
    config: &RunConfig,
    summaries: &Summaries,
    streams: &StreamOutcome,
    cycles: &CycleOutcome,
) -> serde_json::Value {
    let measurements = measurement_entries(summaries);
    let threshold_evaluation = threshold_entries(&measurements);
    let result = if threshold_evaluation
        .iter()
        .any(|entry| entry["exceeded"].as_bool().unwrap_or(true))
    {
        "fail"
    } else {
        "pass"
    };
    serde_json::json!({
        "fixture_id": FIXTURE_ID,
        "resolution": PERF_RESOLUTION.as_str(),
        "frame_width": 1920,
        "frame_height": 1080,
        "mode": mode_entry(config),
        "method": {
            "percentile": "nearest-rank (index = max(0, ceil(q*n)-1)), mirroring scripts/performance/benchmark.py",
            "throughput": "samples / (sum-of-spans / 1000), mirroring _metric_summary",
            "jitter": "p99 - p50 per metric",
            "recognition": "pixel-SAD stand-in for template matching over latest-frame bytes (NOT the real recognition path)",
            "warmup": "wall-clock warm-up iterations discarded before measurement (fast mode: timed iterations only)",
        },
        "header": header_entry(),
        "measurements": measurements,
        "statistics": {
            "serial_throughput_per_second": summaries.serial.throughput_per_second,
            "frame_throughput_per_second": summaries.camera.throughput_per_second,
            "recognition_throughput_per_second": summaries.recognition.throughput_per_second,
            "method": "derived per metric as samples / (sum-of-spans / 1000)",
        },
        "threshold_evaluation": threshold_evaluation,
        "baseline": {
            "status": "fixed-threshold",
            "report_count": 0,
            "aggregation": serde_json::Value::Null,
            "note": "Fixed absolute p95 budgets are required for production-virtual-v1; no mutable baseline is used",
        },
        "result": result,
        "evaluation": {
            "mode": "blocking",
            "result": result,
            "notes": "fixed p95 budgets, sample counts, error counts, and schema invariants are required gates",
        },
        "streaming_sample_counts": {
            "serial_send_latency_ms": streams.serial.len(),
            "camera_frame_latency_ms": streams.acquire.len(),
            "recognition_latency_ms": streams.recognition.len(),
        },
        "cycle_counts": {
            "shutdown_latency_ms": cycles.shutdown.len(),
            "recovery_latency_ms": cycles.recovery.len(),
        },
    })
}

fn build_samples(
    config: &RunConfig,
    streams: &StreamOutcome,
    cycles: &CycleOutcome,
) -> serde_json::Value {
    serde_json::json!({
        "fixture_id": FIXTURE_ID,
        "streaming_samples_per_metric": config.streaming_samples,
        "shutdown_recovery_cycles": config.cycles,
        "samples": {
            "camera_frame_latency_ms": streams.acquire,
            "serial_send_latency_ms": streams.serial,
            "recognition_latency_ms": streams.recognition,
            "shutdown_latency_ms": cycles.shutdown,
            "recovery_latency_ms": cycles.recovery,
        },
    })
}

fn summary_lines(report: &serde_json::Value) -> Vec<String> {
    let mut lines = vec![
        format!("fixture_id: {}", report["fixture_id"]),
        format!(
            "mode: {} streaming samples, {}s warm-up, {} stop/recovery cycles{}",
            report["mode"]["streaming_samples_per_metric"],
            report["mode"]["warmup_seconds"],
            report["mode"]["shutdown_recovery_cycles"],
            if report["mode"]["fast_sanity"].as_bool().unwrap_or(false) {
                " (fast sanity)"
            } else {
                ""
            }
        ),
        format!(
            "derived throughput: serial {:.1}/s, frame {:.1}/s",
            report["statistics"]["serial_throughput_per_second"],
            report["statistics"]["frame_throughput_per_second"],
        ),
    ];
    if let Some(measurements) = report["measurements"].as_array() {
        for entry in measurements {
            lines.push(format!(
                "{}: p50={:.3}ms p95={:.3}ms max={:.3}ms n={} threshold_p95={} {}",
                entry["metric"],
                entry["p50"].as_f64().unwrap_or(f64::NAN),
                entry["p95"].as_f64().unwrap_or(f64::NAN),
                entry["maximum"].as_f64().unwrap_or(f64::NAN),
                entry["sample_count"],
                entry["threshold"]["threshold_p95"],
                if entry["threshold"]["exceeded"].as_bool().unwrap_or(false) {
                    "BLOCKING-EXCEEDED"
                } else {
                    "within-blocking-budget"
                },
            ));
        }
    }
    lines.push(format!(
        "baseline: {} | result: {} (numeric thresholds blocking)",
        report["baseline"]["status"], report["result"],
    ));
    lines
}

fn artifact_paths() -> (String, String, String, String) {
    let out_dir =
        std::env::var("POKECON_PRODUCTION_PERF_OUT").unwrap_or_else(|_| "target".to_owned());
    (
        out_dir.clone(),
        format!("{out_dir}/performance-report.json"),
        format!("{out_dir}/performance-samples.json"),
        format!("{out_dir}/production-perf.log"),
    )
}

fn write_artifacts(
    report: &serde_json::Value,
    samples: &serde_json::Value,
) -> (String, String, String) {
    let (out_dir, report_path, samples_path, log_path) = artifact_paths();
    std::fs::create_dir_all(&out_dir).expect("perf output dir must be creatable");
    std::fs::write(
        &report_path,
        serde_json::to_string_pretty(report).expect("report must serialize"),
    )
    .expect("report must be writable");
    std::fs::write(
        &samples_path,
        serde_json::to_string_pretty(samples).expect("samples must serialize"),
    )
    .expect("samples must be writable");
    let mut log = vec!["pokecon production-perf harness log".to_owned()];
    log.extend(summary_lines(report));
    log.push(format!(
        "artifacts: {report_path} {samples_path} {log_path}"
    ));
    std::fs::write(&log_path, log.join("\n") + "\n").expect("log must be writable");
    for line in summary_lines(report) {
        println!("{line}");
    }
    eprintln!("production-perf artifacts: {report_path} {samples_path} {log_path}");
    (report_path, samples_path, log_path)
}

fn assert_report_shape(path: &str, config: &RunConfig) {
    let raw = std::fs::read_to_string(path).expect("report must be readable");
    let parsed: serde_json::Value = serde_json::from_str(&raw).expect("report must be valid JSON");
    assert_eq!(parsed["fixture_id"], FIXTURE_ID, "report fixture id");
    assert_eq!(parsed["resolution"], "1920x1080", "report resolution");
    assert_eq!(
        parsed["baseline"]["status"], "fixed-threshold",
        "fixed threshold policy"
    );
    assert_eq!(parsed["result"], "pass", "report result");
    assert!(
        parsed["header"]["target_reuse"].is_boolean(),
        "report target_reuse provenance must be boolean"
    );
    let measurements = parsed["measurements"]
        .as_array()
        .expect("measurements must be an array");
    assert_eq!(measurements.len(), 5, "five latency measurements");
    for entry in measurements {
        let name = entry["metric"].as_str().unwrap_or("<missing>");
        let expected = if name == "shutdown_latency" || name == "recovery_latency" {
            config.cycles
        } else {
            config.streaming_samples
        };
        assert_eq!(entry["sample_count"], expected, "{name} sample count");
        assert!(
            entry["threshold"]["blocking"].as_bool().unwrap_or(false),
            "{name} threshold must be blocking"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn production_main_path_virtual_perf() {
    let config = run_config();
    let overall = Duration::from_secs(config.warmup_secs.saturating_add(600));
    tokio::time::timeout(overall, async {
        let checker = build_checker_frame();
        let mut fixture = start_fixture(&checker, config.cycles).await;
        warm_up(&fixture, &config).await;
        let streams = collect_stream_samples(&fixture, &config).await;
        let cycles = collect_stop_recovery(&mut fixture, &config).await;
        let summaries = summarize_all(&streams, &cycles, &config);
        let report = build_report(&config, &summaries, &streams, &cycles);
        let samples = build_samples(&config, &streams, &cycles);
        let (report_path, _, _) = write_artifacts(&report, &samples);
        assert_report_shape(&report_path, &config);

        fixture.serial.disconnect().await.unwrap();
        fixture
            .camera
            .shutdown(SHUTDOWN_TIMEOUT)
            .expect("final teardown shutdown must settle");
    })
    .await
    .expect("production perf harness must settle within the overall bound");
}
