//! AR-11-04 acceptance: serial-controller output and camera frame consumption
//! advance concurrently under shared load (hardware-free).
//!
//! This exercises the production [`CameraManager`]/frame-consumer path and the
//! production [`SerialManager`]/controller-output path at the same time using
//! the existing virtual camera and virtual serial backends. No `/dev/video*`
//! nodes, kernel modules, or physical ports are touched.
//!
//! Evidence is output/frame progress plus genuine overlap: bounded controller
//! sends must all land on the wire while the frame consumer observes fresh
//! frames across the whole serial phase, and the two activity intervals must
//! intersect.
//!
//! Measurement half (perf fold 1): after the concurrent window, a second phase
//! collects 300 [`Instant`]-derived samples per metric at 1920x1080 —
//! serial-send latency, latest-frame acquisition latency, and recognition
//! latency — and writes a JSON artifact (`$POKECON_PERF_OUT`, default
//! `target/concurrent-camera-serial-1080p-v1.json`). Statistics mirror
//! `nearest_rank` in `scripts/performance/benchmark.py` (p50/p95/maximum,
//! p99 detail, throughput = N/duration, jitter = p99-p50, windowed stability).
//! Numeric thresholds stay advisory (recorded, never asserted); only sample
//! counts, error counts, and the pre-existing assertions can fail the test.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
use tokio::sync::Barrier;

/// Bounded controller sends sharing the window with frame consumption.
const SERIAL_SENDS: usize = 64;
/// Minimum distinct frames observed while serial output is in flight.
const REQUIRED_DISTINCT_FRAMES: u64 = 8;
/// Generous per-side deadline; the virtual backends answer immediately.
const SIDE_DEADLINE: Duration = Duration::from_secs(10);
/// Outer bound so a stuck path fails instead of hanging CI.
///
/// Sized for a 1920x1080 session plus 300 full-frame recognition passes in a
/// debug test profile; the concurrent window itself still settles in seconds.
const OVERALL_TIMEOUT: Duration = Duration::from_mins(2);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Perf artifact identity for this extended harness.
const FIXTURE_ID: &str = "concurrent-camera-serial-1080p-v1";
/// Single session resolution for both the load window and measurement.
/// (Deliberately no 640x360 debug mode; debug via `-- --nocapture`.)
const PERF_RESOLUTION: CaptureResolution = CaptureResolution::R1920x1080;
/// Samples per metric, mirroring `SAMPLE_COUNT=300` in
/// `scripts/performance/benchmark.py`.
const PERF_SAMPLES: usize = 300;
/// Timed warm-up iterations discarded before measurement. The 60s wall-clock
/// warm-up belongs to the fold-2 CI job, not this in-process harness.
const PERF_WARMUP_SAMPLES: usize = 10;
/// Stability window size: 300 samples split into 10 windows of 30.
const PERF_WINDOW_SIZE: usize = 30;
/// Bounded polls for a published frame per measurement iteration.
const ACQUIRE_POLLS: usize = 100;
/// Known solid color the session frames are derived from.
const SOLID_BGR: [u8; 3] = [9, 18, 27];
/// Contrasting color for the non-trivial checker variant.
const CHECKER_ALT_BGR: [u8; 3] = [200, 150, 100];
/// Checker tile edge in pixels; 1920x1080 factors into 16x9 tiles of 120px.
const CHECKER_TILE: u32 = 120;
/// Artifact path when `POKECON_PERF_OUT` is unset, relative to the test
/// working directory (the crate directory under `cargo test`).
const DEFAULT_ARTIFACT_PATH: &str = "target/concurrent-camera-serial-1080p-v1.json";

/// Effort-target advisory thresholds (milliseconds). Numbers are advisory
/// until AR-11-10 closes: they are recorded in the artifact but never
/// asserted, because shared runners are too noisy to gate on.
const ADVISORY_P95_SERIAL_SEND_MS: f64 = 50.0;
const ADVISORY_P95_FRAME_ACQUIRE_MS: f64 = 5.0;
const ADVISORY_P95_RECOGNITION_MS: f64 = 250.0;

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

/// Nearest-rank quantile with integer rank math, mirroring `nearest_rank` in
/// `scripts/performance/benchmark.py` (`index = max(0, ceil(q*n) - 1)`).
/// Callers pass the quantile as `numerator/denominator` (e.g. p95 = 95/100);
/// the ceiling division `(numerator * n + denominator - 1) / denominator`
/// keeps the rank exact with no float/int casts.
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

fn summarize(samples_ms: &[f64], metric: &str) -> MetricSummary {
    assert_eq!(
        samples_ms.len(),
        PERF_SAMPLES,
        "{metric} must hold exactly {PERF_SAMPLES} samples"
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

struct PerfOutcome {
    serial_ms: Vec<f64>,
    acquire_ms: Vec<f64>,
    recognition_ms: Vec<f64>,
    errors: u64,
    window_serial_p95_ms: Vec<f64>,
}

fn stability_windows(serial_ms: &[f64]) -> Vec<f64> {
    serial_ms
        .chunks(PERF_WINDOW_SIZE)
        .map(|window| {
            let mut ordered = window.to_vec();
            ordered.sort_by(f64::total_cmp);
            nearest_rank(&ordered, 95, 100)
        })
        .collect()
}

async fn collect_perf_samples(serial: &SerialManager, camera: &CameraManager) -> PerfOutcome {
    let frames = camera.frame_source();
    let checker_sad = expected_checker_sad();
    // Sanity-pin the stand-in itself: a locally built solid frame scores zero.
    assert_eq!(
        sad_vs_solid(BgrFrame::solid(PERF_RESOLUTION, SOLID_BGR).pixels()),
        0,
        "SAD stand-in must score zero on the known solid color"
    );

    for index in 0..PERF_WARMUP_SAMPLES {
        let button = if index % 2 == 0 { Button::A } else { Button::B };
        serial
            .send_controller_state(controller_with(button))
            .await
            .expect("warm-up send must succeed");
        if let Some(media) = frames.latest() {
            let _ = sad_vs_solid(media.frame.pixels());
        }
    }

    let mut serial_ms = Vec::with_capacity(PERF_SAMPLES);
    let mut acquire_ms = Vec::with_capacity(PERF_SAMPLES);
    let mut recognition_ms = Vec::with_capacity(PERF_SAMPLES);
    let mut errors = 0u64;
    for index in 0..PERF_SAMPLES {
        let button = if index % 2 == 0 { Button::A } else { Button::B };
        let started = Instant::now();
        if serial
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
            sad == 0 || sad == checker_sad,
            "recognition SAD must match a known session frame (solid=0, checker={checker_sad}), saw {sad}"
        );
    }
    assert_eq!(errors, 0, "measurement phase must complete without errors");
    let window_serial_p95_ms = stability_windows(&serial_ms);
    assert_eq!(
        window_serial_p95_ms.len(),
        PERF_SAMPLES / PERF_WINDOW_SIZE,
        "every stability window must complete"
    );
    PerfOutcome {
        serial_ms,
        acquire_ms,
        recognition_ms,
        errors,
        window_serial_p95_ms,
    }
}

fn artifact_path() -> String {
    std::env::var("POKECON_PERF_OUT").unwrap_or_else(|_| DEFAULT_ARTIFACT_PATH.to_owned())
}

fn summary_json(name: &str, summary: &MetricSummary, advisory_p95_ms: f64) -> serde_json::Value {
    serde_json::json!({
        "metric": name,
        "unit": "ms",
        "sample_count": summary.count,
        "p50": summary.p50_ms,
        "p95": summary.p95_ms,
        "maximum": summary.maximum_ms,
        "p99": summary.p99_ms,
        "jitter_p99_minus_p50": summary.jitter_p99_minus_p50_ms,
        "throughput_per_second": summary.throughput_per_second,
        "duration_ms": summary.duration_ms,
        "advisory_p95_ms": advisory_p95_ms,
        "advisory_exceeded": summary.p95_ms > advisory_p95_ms,
    })
}

fn write_perf_artifact(
    path: &str,
    serial: &MetricSummary,
    acquire: &MetricSummary,
    recognition: &MetricSummary,
    outcome: &PerfOutcome,
) {
    let document = serde_json::json!({
        "fixture_id": FIXTURE_ID,
        "resolution": PERF_RESOLUTION.as_str(),
        "frame_width": 1920,
        "frame_height": 1080,
        "sample_count": PERF_SAMPLES,
        "warmup_samples_skipped": PERF_WARMUP_SAMPLES,
        "method": {
            "percentile": "nearest-rank (index = max(0, ceil(q*n)-1)), mirroring scripts/performance/benchmark.py",
            "throughput": "samples / (sum-of-spans / 1000), mirroring _metric_summary",
            "jitter": "p99 - p50 per metric",
            "stability": "all 30-sample windows completed with zero errors; per-window serial p95 reported",
            "recognition": "pixel-SAD stand-in for template matching over latest-frame bytes (NOT the real recognition path)",
            "acquire": "in-process LatestFrameSource::latest() mirror acquisition (Arc clone, no pixel copy)",
            "serial": "send_controller_state until the write completes on the virtual endpoint",
            "warmup": "10 timed iterations discarded; the 60s wall-clock warm-up belongs to the fold-2 CI job",
        },
        "header": {
            "build_sha": std::env::var("POKECON_PERF_BUILD_SHA").unwrap_or_else(|_| "unknown".to_owned()),
            "runner": std::env::var("RUNNER_NAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "unknown".to_owned()),
            "package_version": env!("CARGO_PKG_VERSION"),
            "profile": std::env::var("POKECON_PERF_PROFILE").unwrap_or_else(|_| "test".to_owned()),
        },
        "measurements": {
            "serial_send_latency_ms": summary_json("serial_send_latency_ms", serial, ADVISORY_P95_SERIAL_SEND_MS),
            "frame_acquire_latency_ms": summary_json("frame_acquire_latency_ms", acquire, ADVISORY_P95_FRAME_ACQUIRE_MS),
            "recognition_latency_ms": summary_json("recognition_latency_ms", recognition, ADVISORY_P95_RECOGNITION_MS),
        },
        "stability": {
            "window_size": PERF_WINDOW_SIZE,
            "windows": outcome.window_serial_p95_ms.len(),
            "windows_passed": outcome.window_serial_p95_ms.len(),
            "criterion": "zero errors and every window completed without stalls",
            "errors": outcome.errors,
            "window_serial_p95_ms": outcome.window_serial_p95_ms,
        },
        "evaluation": {
            "mode": "advisory",
            "result": "pass",
            "notes": "numeric thresholds are advisory until AR-11-10 closes; only sample/error counts gate",
        },
        "samples": {
            "serial_send_latency_ms": outcome.serial_ms,
            "frame_acquire_latency_ms": outcome.acquire_ms,
            "recognition_latency_ms": outcome.recognition_ms,
        },
    });
    let parent = std::path::Path::new(path)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = parent {
        std::fs::create_dir_all(parent).expect("perf artifact parent must be creatable");
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(&document).expect("perf artifact must serialize"),
    )
    .expect("perf artifact must be writable");
    eprintln!("perf artifact: {path}");
}

struct ConcurrentFixture {
    camera: CameraManager,
    serial: SerialManager,
    endpoint: VirtualSerialEndpoint,
    wire_baseline: usize,
}

fn start_camera() -> CameraManager {
    let backend = VirtualCameraBackend::default();
    backend.push_open(VirtualOpenPlan::Success(VirtualSessionPlan::recorded(
        30,
        [
            RecordedFrame::Solid(SOLID_BGR),
            RecordedFrame::Frame(build_checker_frame()),
        ],
    )));
    let camera = CameraManager::start(
        Arc::new(backend),
        CameraConfig::new(CameraSelector::Index(0), 30, PERF_RESOLUTION).unwrap(),
        FlipMode::None,
    )
    .unwrap();
    assert!(
        camera.status().camera_opened,
        "virtual capture must open before the load window"
    );
    camera
}

async fn start_serial() -> (SerialManager, VirtualSerialEndpoint, usize) {
    let backend = VirtualSerialBackend::default();
    let endpoint = VirtualSerialEndpoint::new();
    // Small deterministic latency per low-level write so the serial phase
    // spans a real window the frame consumer must overlap with.
    endpoint.set_write_delay(Duration::from_millis(1)).await;
    backend
        .push_plan(SerialVirtualOpenPlan::Success(endpoint.clone()))
        .await;
    let serial = SerialManager::new(Arc::new(backend));
    serial
        .apply_config(
            SerialConfig::new("concurrent-loopback", 9600, ControllerFormat::Default).unwrap(),
        )
        .await
        .unwrap();
    // The connect transaction itself emits one initial frame; only bytes
    // after this baseline belong to the bounded load window.
    let baseline = endpoint.written().await.len();
    (serial, endpoint, baseline)
}

async fn start_fixture() -> ConcurrentFixture {
    let camera = start_camera();
    let (serial, endpoint, wire_baseline) = start_serial().await;
    ConcurrentFixture {
        camera,
        serial,
        endpoint,
        wire_baseline,
    }
}

async fn drive_serial_load(
    serial: SerialManager,
    barrier: Arc<Barrier>,
    serial_done: Arc<AtomicBool>,
) -> (Instant, Instant) {
    barrier.wait().await;
    let mut first: Option<Instant> = None;
    let mut last: Option<Instant> = None;
    for index in 0..SERIAL_SENDS {
        let button = if index % 2 == 0 { Button::A } else { Button::B };
        serial
            .send_controller_state(controller_with(button))
            .await
            .expect("controller send must succeed under camera load");
        let now = Instant::now();
        if first.is_none() {
            first = Some(now);
        }
        last = Some(now);
    }
    serial_done.store(true, Ordering::Release);
    (
        first.expect("at least one send"),
        last.expect("at least one send"),
    )
}

async fn consume_frames(
    camera: CameraManager,
    barrier: Arc<Barrier>,
    serial_done: Arc<AtomicBool>,
) -> (Option<Instant>, Option<Instant>, u64) {
    let frames = camera.frame_source();
    barrier.wait().await;
    let deadline = Instant::now() + SIDE_DEADLINE;
    let mut highest: Option<u64> = None;
    let mut distinct: u64 = 0;
    let mut first: Option<Instant> = None;
    let mut last: Option<Instant> = None;
    // Keep consuming until fresh frames were observed across the
    // whole serial phase (or the generous deadline expires).
    while Instant::now() < deadline
        && (distinct < REQUIRED_DISTINCT_FRAMES || !serial_done.load(Ordering::Acquire))
    {
        if let Some(media) = frames.latest() {
            let sequence = media.frame_sequence;
            if highest.is_none_or(|top| sequence > top) {
                highest = Some(sequence);
                distinct += 1;
                let now = Instant::now();
                if first.is_none() {
                    first = Some(now);
                }
                last = Some(now);
            }
        }
        tokio::task::yield_now().await;
    }
    (first, last, distinct)
}

fn assert_overlap(
    serial_first: Instant,
    serial_last: Instant,
    frame_first: Instant,
    frame_last: Instant,
) {
    assert!(
        serial_first <= frame_last && frame_first <= serial_last,
        "serial output and frame consumption must overlap in time"
    );
}

async fn assert_wire_output(endpoint: &VirtualSerialEndpoint, wire_baseline: usize) {
    let written = endpoint.written().await;
    let wire = std::str::from_utf8(&written[wire_baseline..])
        .expect("controller wire output must be UTF-8");
    assert_eq!(
        wire.lines().count(),
        SERIAL_SENDS,
        "every bounded controller send must land on the wire"
    );
}

fn assert_perf_artifact(path: &str) {
    let raw = std::fs::read_to_string(path).expect("perf artifact must be readable");
    let parsed: serde_json::Value =
        serde_json::from_str(&raw).expect("perf artifact must be valid JSON");
    assert_eq!(
        parsed["fixture_id"], FIXTURE_ID,
        "perf artifact must carry this harness fixture id"
    );
    assert_eq!(
        parsed["resolution"], "1920x1080",
        "perf artifact must record the 1080p session"
    );
    assert_eq!(
        parsed["sample_count"], PERF_SAMPLES,
        "perf artifact must hold 300 samples per metric"
    );
    for metric in [
        "serial_send_latency_ms",
        "frame_acquire_latency_ms",
        "recognition_latency_ms",
    ] {
        assert_eq!(
            parsed["measurements"][metric]["sample_count"], PERF_SAMPLES,
            "perf artifact {metric} must hold 300 samples"
        );
    }
    assert_eq!(
        parsed["stability"]["errors"], 0,
        "perf artifact must record zero measurement errors"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn serial_output_and_frame_consumption_progress_concurrently() {
    tokio::time::timeout(OVERALL_TIMEOUT, async {
        let fixture = start_fixture().await;

        // A three-party barrier releases both load tasks from the same point
        // so their activity windows genuinely intersect.
        let barrier = Arc::new(Barrier::new(3));
        let serial_done = Arc::new(AtomicBool::new(false));

        let serial_task = tokio::spawn(drive_serial_load(
            fixture.serial.clone(),
            barrier.clone(),
            serial_done.clone(),
        ));
        let frame_task = tokio::spawn(consume_frames(
            fixture.camera.clone(),
            barrier.clone(),
            serial_done.clone(),
        ));

        barrier.wait().await;
        let (serial_window, frame_outcome) = tokio::join!(serial_task, frame_task);
        let (serial_first, serial_last) = serial_window.expect("serial task must not panic");
        let (frame_first, frame_last, distinct_frames) =
            frame_outcome.expect("frame task must not panic");

        assert!(
            distinct_frames >= REQUIRED_DISTINCT_FRAMES,
            "frame consumer must observe fresh frames while serial output is in flight \
             (saw {distinct_frames} distinct sequences)"
        );
        let (frame_first, frame_last) = frame_first
            .zip(frame_last)
            .expect("frame consumer must observe at least one frame");
        assert_overlap(serial_first, serial_last, frame_first, frame_last);

        assert_wire_output(&fixture.endpoint, fixture.wire_baseline).await;
        assert!(
            fixture.camera.status().camera_opened,
            "capture must still be open after the concurrent load window"
        );

        let perf = collect_perf_samples(&fixture.serial, &fixture.camera).await;
        let serial_summary = summarize(&perf.serial_ms, "serial_send_latency_ms");
        let acquire_summary = summarize(&perf.acquire_ms, "frame_acquire_latency_ms");
        let recognition_summary = summarize(&perf.recognition_ms, "recognition_latency_ms");
        let path = artifact_path();
        write_perf_artifact(
            &path,
            &serial_summary,
            &acquire_summary,
            &recognition_summary,
            &perf,
        );
        assert_perf_artifact(&path);

        fixture.serial.disconnect().await.unwrap();
        fixture.camera.shutdown(SHUTDOWN_TIMEOUT).unwrap();
    })
    .await
    .expect("concurrent load must settle within the overall bound");
}
