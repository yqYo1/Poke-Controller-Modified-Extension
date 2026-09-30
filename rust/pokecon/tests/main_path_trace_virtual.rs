//! Main-path trace harness with `fixture_id` `main-path-trace-v1` (wave-E
//! fold-3a).
//!
//! This traces the production main path end to end on virtual I/O, one
//! parent-side span per stage per cycle: script command execution
//! (`ScriptWorkerClient::execute` over the production Python worker path)
//! → camera frame acquisition (`CameraManager::frame_source().latest()`)
//! → recognition workload (pixel-SAD stand-in over full-frame bytes,
//! labeled as such) → serial output (`SerialManager::send_controller_state`)
//! → wire bytes observed (`VirtualSerialEndpoint::written` growth), plus the
//! wall-clock `total_end_to_end_ms` per cycle. No worker-internal
//! instrumentation; no `rust/pokecon/src` changes.
//!
//! The command payload is a real `ImageProcPythonCommand` script whose
//! `do()` calls the production camera surface (`readFrame`,
//! `getCameraImage`) against a deterministic worker-side frame ring, so the
//! trace covers the production Python camera path on every cycle. The
//! parent-side virtual camera session (1920x1080 recorded frames) feeds the
//! `frame_acquire` stage; the virtual serial endpoint feeds the
//! `wire_observe` stage. No `/dev/video*` nodes or physical ports are
//! touched.
//!
//! Mode defaults by build profile: release builds run full evidence mode
//! (300 end-to-end cycles, 60s wall-clock warm-up); debug builds run fast
//! sanity (3 cycles, no wall-clock warm-up) because debug numbers are not
//! evidence, which keeps broader debug suites that execute this harness
//! cheap.
//!
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
//! Knobs (all optional): `POKECON_MAIN_PATH_TRACE_OUT` (artifact dir,
//! default `target/` under the crate directory, i.e. `rust/pokecon/target/`),
//! `POKECON_MAIN_PATH_TRACE_FAST=1|0` (force fast sanity or full mode),
//! `POKECON_MAIN_PATH_TRACE_SAMPLES` / `POKECON_MAIN_PATH_TRACE_CYCLES`
//! (end-to-end cycle count; `CYCLES` wins when both are set) /
//! `POKECON_MAIN_PATH_TRACE_WARMUP_SECS` (explicit per-value overrides),
//! `POKECON_PERF_BUILD_SHA` (report header).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pokecon::integration_test_support::camera::{
    BgrFrame, CameraConfig, CameraManager, CameraSelector, CaptureResolution, FlipMode,
    RecordedFrame, ScreenshotFormat, SharedFrameRing, VirtualCameraBackend, VirtualOpenPlan,
    VirtualSessionPlan,
};
use pokecon::integration_test_support::device::controller::{Button, ControllerState};
use pokecon::integration_test_support::device::serial::{
    ControllerFormat, SerialConfig, SerialManager, VirtualOpenPlan as SerialVirtualOpenPlan,
    VirtualSerialBackend, VirtualSerialEndpoint,
};
use pokecon::integration_test_support::worker as pokecon_worker;
use pokecon_worker::WorkerKind;
use pokecon_worker::ipc::ResourceSafety;
use pokecon_worker::script::protocol::{
    self, HostCameraControlRequest, HostCameraInitializeResult, HostCameraState,
    HostControllerInputRequest, HostDialogOpenRequest, HostDialogOpenResult,
    HostDialogStatusRequest, HostDialogStatusResult, HostNetworkRequest, HostNetworkResult,
    HostNotificationRequest, HostOutputRequest, HostOverlayRequest, HostPopupImageRequest,
    HostTkRequest, HostTkResult, ScriptDialogState, ScriptExecuteRequest, ScriptExecutionOutcome,
    ScriptInitializeRequest,
};
use pokecon_worker::script::{ScriptHost, ScriptHostError, ScriptWorkerClient};
use pokecon_worker::supervisor::{ManagedWorker, StopPurpose, WorkerLaunch, WorkerSupervisor};
use tempfile::TempDir;

mod support;

use support::worker_binary;

/// Artifact identity for this harness.
const FIXTURE_ID: &str = "main-path-trace-v1";
/// Single session resolution for warm-up and measurement.
const TRACE_RESOLUTION: CaptureResolution = CaptureResolution::R1920x1080;
/// Worker-side frame-ring resolution the trace script observes.
const SCRIPT_RING_RESOLUTION: CaptureResolution = CaptureResolution::R640x360;
/// Full-mode end-to-end trace cycles.
const FULL_CYCLES: usize = 300;
/// Fast-mode end-to-end trace cycles (debug-build default, or `FAST=1`).
const FAST_CYCLES: usize = 3;
/// Full-mode wall-clock warm-up, discarded before measurement.
const FULL_WARMUP_SECS: u64 = 60;
/// Timed warm-up iterations used when the wall-clock warm-up is disabled.
const TIMED_WARMUP_ITERS: usize = 10;
/// Bounded polls for a published frame per cycle.
const ACQUIRE_POLLS: usize = 100;
/// Bound waiting for wire bytes to grow after each serial send.
const WIRE_OBSERVE_TIMEOUT: Duration = Duration::from_secs(5);
/// Known solid color the session frames are derived from.
const SOLID_BGR: [u8; 3] = [9, 18, 27];
/// Contrasting color for the non-trivial checker variant.
const CHECKER_ALT_BGR: [u8; 3] = [200, 150, 100];
/// Checker tile edge in pixels; 1920x1080 factors into 16x9 tiles of 120px.
const CHECKER_TILE: u32 = 120;

/// Required p95 budgets (milliseconds) for the release virtual-I/O gate.
const BLOCKING_P95_COMMAND_DISPATCH_MS: f64 = 5_000.0;
const BLOCKING_P95_CAMERA_FRAME_MS: f64 = 10.0;
const BLOCKING_P95_RECOGNITION_MS: f64 = 500.0;
const BLOCKING_P95_SERIAL_SEND_MS: f64 = 50.0;
const BLOCKING_P95_WIRE_OBSERVE_MS: f64 = 50.0;
const BLOCKING_P95_TOTAL_END_TO_END_MS: f64 = 30_000.0;

/// Minimal production script exercising the real worker camera surface on
/// every traced cycle: `readFrame` plus a `getCameraImage` crop.
const TRACE_SOURCE: &str = r#"
from Commands.PythonCommandBase import ImageProcPythonCommand


class Trace(ImageProcPythonCommand):
    def do(self):
        assert self.camera.isOpened()
        frame = self.camera.readFrame()
        assert frame.shape == (360, 640, 3)
        cropped = self.getCameraImage("1", [60, 50, 90, 70])
        assert cropped.shape == (20, 30, 3)
        self.print_t1("trace-frame-ok")
"#;

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
    cycles: usize,
    warmup_secs: u64,
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

/// Resolves the run mode: `POKECON_MAIN_PATH_TRACE_FAST` (`1`/`0`) forces
/// fast sanity or full evidence mode; unset defaults by build profile
/// (debug builds: fast sanity, release builds: full mode).
fn run_config() -> RunConfig {
    let fast = match std::env::var("POKECON_MAIN_PATH_TRACE_FAST").as_deref() {
        Ok("1") => true,
        Ok("0") => false,
        Ok(other) => panic!("POKECON_MAIN_PATH_TRACE_FAST must be 0 or 1, saw {other:?}"),
        Err(_) => cfg!(debug_assertions),
    };
    let default_cycles = if fast { FAST_CYCLES } else { FULL_CYCLES };
    // `SAMPLES` and `CYCLES` both set the end-to-end cycle count (one sample
    // per stage per cycle); `CYCLES` wins when both are set.
    let sampled = env_usize("POKECON_MAIN_PATH_TRACE_SAMPLES", default_cycles);
    let config = RunConfig {
        cycles: env_usize("POKECON_MAIN_PATH_TRACE_CYCLES", sampled),
        warmup_secs: env_u64(
            "POKECON_MAIN_PATH_TRACE_WARMUP_SECS",
            if fast { 0 } else { FULL_WARMUP_SECS },
        ),
        fast,
    };
    assert!(config.cycles >= 1, "cycles must be positive");
    config
}

fn camera_config() -> CameraConfig {
    CameraConfig::new(CameraSelector::Index(0), 30, TRACE_RESOLUTION)
        .expect("1080p virtual camera config must build")
}

fn serial_config() -> SerialConfig {
    SerialConfig::new("main-path-trace-loopback", 9600, ControllerFormat::Default)
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

/// Minimal [`ScriptHost`]: serves the deterministic worker-side camera ring
/// and state; every other host surface is an inert stub because the trace
/// script only reads the camera and prints.
#[derive(Debug, Default)]
struct TraceScriptHost {
    camera_ring: Mutex<Option<SharedFrameRing>>,
    camera_state: Mutex<Option<HostCameraState>>,
}

impl ScriptHost for TraceScriptHost {
    fn controller_input(
        &self,
        _request: HostControllerInputRequest,
    ) -> Result<(), ScriptHostError> {
        Ok(())
    }

    fn controller_neutral(&self) -> Result<(), ScriptHostError> {
        Ok(())
    }

    fn serial_write(&self, _data: Vec<u8>) -> Result<(), ScriptHostError> {
        Ok(())
    }

    fn serial_write_row(&self, _row: String) -> Result<(), ScriptHostError> {
        Ok(())
    }

    fn serial_reload(&self) -> Result<(), ScriptHostError> {
        Ok(())
    }

    fn output(&self, _request: HostOutputRequest) -> Result<(), ScriptHostError> {
        Ok(())
    }

    fn dialog_open(
        &self,
        _request: HostDialogOpenRequest,
    ) -> Result<HostDialogOpenResult, ScriptHostError> {
        Ok(HostDialogOpenResult { dialog_id: 1 })
    }

    fn dialog_status(
        &self,
        _request: HostDialogStatusRequest,
    ) -> Result<HostDialogStatusResult, ScriptHostError> {
        Ok(HostDialogStatusResult {
            state: ScriptDialogState::Aborted,
        })
    }

    fn dialog_close_all(&self) -> Result<(), ScriptHostError> {
        Ok(())
    }

    fn network(&self, _request: HostNetworkRequest) -> Result<HostNetworkResult, ScriptHostError> {
        Ok(HostNetworkResult { message: None })
    }

    fn notification(&self, _request: HostNotificationRequest) -> Result<(), ScriptHostError> {
        Ok(())
    }

    fn camera_initialize(&self) -> Result<HostCameraInitializeResult, ScriptHostError> {
        Ok(HostCameraInitializeResult {
            mapping: self
                .camera_ring
                .lock()
                .unwrap()
                .as_ref()
                .map(SharedFrameRing::descriptor),
            state: self.current_camera_state(),
        })
    }

    fn camera_control(
        &self,
        _request: HostCameraControlRequest,
    ) -> Result<HostCameraState, ScriptHostError> {
        Ok(self.current_camera_state())
    }

    fn overlay(&self, _request: HostOverlayRequest) -> Result<(), ScriptHostError> {
        Ok(())
    }

    fn popup_image(&self, _request: HostPopupImageRequest) -> Result<(), ScriptHostError> {
        Ok(())
    }

    fn tk(&self, _request: HostTkRequest) -> Result<HostTkResult, ScriptHostError> {
        Ok(HostTkResult::default())
    }
}

impl TraceScriptHost {
    fn current_camera_state(&self) -> HostCameraState {
        self.camera_state
            .lock()
            .unwrap()
            .unwrap_or(HostCameraState {
                opened: false,
                fps: 30,
                capture_resolution: SCRIPT_RING_RESOLUTION,
                flip_mode: FlipMode::None,
                screenshot_format: ScreenshotFormat::Png,
            })
    }
}

impl ResourceSafety for TraceScriptHost {
    fn force_release(&self) {}
}

fn create_profile() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temporary = TempDir::new().expect("temporary profile is created");
    let command_root = temporary.path().join("Commands");
    let data_root = temporary.path().join("Data");
    std::fs::create_dir(&command_root).expect("command root is created");
    std::fs::create_dir(&data_root).expect("data root is created");
    (temporary, command_root, data_root)
}

async fn spawn_client(host: Arc<TraceScriptHost>) -> (Arc<ManagedWorker>, Arc<ScriptWorkerClient>) {
    let supervisor = WorkerSupervisor::new();
    let mut launch = WorkerLaunch::managed(worker_binary(), WorkerKind::Script).clear_environment();
    if let Some(site_packages) = std::env::var_os(protocol::PYTHON_SITE_PACKAGES_ENV) {
        launch = launch.environment(protocol::PYTHON_SITE_PACKAGES_ENV, site_packages);
    }
    let worker = supervisor
        .spawn(launch, host.clone())
        .await
        .expect("script worker starts");
    let client =
        Arc::new(ScriptWorkerClient::attach(worker.clone(), host).expect("script client attaches"));
    (worker, client)
}

async fn initialize(
    client: &ScriptWorkerClient,
    command_root: &std::path::Path,
    data_root: &std::path::Path,
) {
    use pokecon_worker::script::protocol::ScriptWorkerStatus;
    assert_eq!(
        client.status().await.expect("initial status succeeds"),
        ScriptWorkerStatus::uninitialized()
    );
    let initialized = client
        .initialize(&ScriptInitializeRequest {
            profile: "default".to_owned(),
            command_root: command_root.to_path_buf(),
            data_root: data_root.to_path_buf(),
        })
        .await
        .expect("script runtime initializes");
    assert!(initialized.status.initialized);
    assert_eq!(initialized.status.profile.as_deref(), Some("default"));
    assert!(!initialized.status.running);
}

async fn stop_worker(worker: &ManagedWorker) -> (bool, bool, bool) {
    let report = worker
        .stop(StopPurpose::ApplicationShutdown, Duration::from_secs(10))
        .await
        .expect("script worker stops cooperatively");
    assert!(report.cooperative_acknowledged, "{report:?}");
    assert!(!report.forced);
    assert!(report.exit.success);
    (
        report.cooperative_acknowledged,
        report.forced,
        report.exit.success,
    )
}

fn execute_request() -> ScriptExecuteRequest {
    ScriptExecuteRequest {
        path: "trace.py".into(),
        class_name: "Trace".to_owned(),
        tags: Vec::new(),
    }
}

struct Fixture {
    camera: CameraManager,
    serial: SerialManager,
    endpoint: VirtualSerialEndpoint,
    checker_sad: u64,
    _profile: TempDir,
    worker: Arc<ManagedWorker>,
    client: Arc<ScriptWorkerClient>,
    /// Keeps the production-like host-owned mapping available for the reader
    /// lifecycle assertion after the worker process has been reaped.
    host: Arc<TraceScriptHost>,
}

async fn start_fixture(checker: &BgrFrame) -> Fixture {
    let backend = Arc::new(VirtualCameraBackend::default());
    backend.push_open(VirtualOpenPlan::Success(session_plan(checker)));
    let camera = CameraManager::start(backend, camera_config(), FlipMode::None)
        .expect("virtual capture must start");
    assert!(
        camera.status().camera_opened,
        "virtual capture must open before measurement"
    );

    let serial_backend = Arc::new(VirtualSerialBackend::default());
    let endpoint = VirtualSerialEndpoint::new();
    serial_backend
        .push_plan(SerialVirtualOpenPlan::Success(endpoint.clone()))
        .await;
    let serial = SerialManager::new(serial_backend);
    serial
        .apply_config(serial_config())
        .await
        .expect("loopback serial must open");

    let solid = BgrFrame::solid(TRACE_RESOLUTION, SOLID_BGR);
    assert_eq!(
        sad_vs_solid(solid.pixels()),
        0,
        "SAD stand-in must score zero on the known solid color"
    );

    let host = Arc::new(TraceScriptHost::default());
    let ring =
        SharedFrameRing::create(SCRIPT_RING_RESOLUTION).expect("worker frame ring is created");
    ring.publish(&BgrFrame::solid(SCRIPT_RING_RESOLUTION, SOLID_BGR))
        .expect("worker frame is published");
    *host.camera_ring.lock().unwrap() = Some(ring);
    *host.camera_state.lock().unwrap() = Some(HostCameraState {
        opened: true,
        fps: 30,
        capture_resolution: SCRIPT_RING_RESOLUTION,
        flip_mode: FlipMode::None,
        screenshot_format: ScreenshotFormat::Png,
    });

    let (profile, command_root, data_root) = create_profile();
    std::fs::write(command_root.join("trace.py"), TRACE_SOURCE).expect("trace script is written");
    let (worker, client) = spawn_client(host.clone()).await;
    initialize(&client, &command_root, &data_root).await;

    // Prime the Python command path once so measurement cycles never include
    // first-import cost; doubles as an early wiring check.
    let primed = client
        .execute(&execute_request())
        .await
        .expect("priming trace execution succeeds");
    assert_eq!(
        primed.outcome,
        ScriptExecutionOutcome::Completed,
        "priming trace execution must complete"
    );

    Fixture {
        camera,
        serial,
        endpoint,
        checker_sad: expected_checker_sad(),
        _profile: profile,
        worker,
        client,
        host,
    }
}

async fn warm_up(fixture: &Fixture, config: &RunConfig) {
    let frames = fixture.camera.frame_source();
    if config.warmup_secs > 0 {
        let deadline = Instant::now() + Duration::from_secs(config.warmup_secs);
        let mut index = 0usize;
        let mut last_frame_sequence: Option<u64> = None;
        // Advance one send+recognition step per published frame. A free-running
        // loop emits thousands of sends per frame, and every wire observation
        // clones the virtual serial's whole accepted-byte buffer (`written()`),
        // so later cycles must not inherit a multi-gigabyte backlog: frame
        // cadence keeps the buffer bounded while still exercising the send and
        // recognition surfaces for the full window.
        while Instant::now() < deadline {
            match frames.latest() {
                Some(media) if last_frame_sequence != Some(media.frame_sequence) => {
                    last_frame_sequence = Some(media.frame_sequence);
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
                    let _ = sad_vs_solid(media.frame.pixels());
                    index += 1;
                }
                _ => tokio::task::yield_now().await,
            }
        }
        eprintln!(
            "main-path-trace warm-up: {index} iterations over {}s",
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
        eprintln!("main-path-trace warm-up: {TIMED_WARMUP_ITERS} timed iterations (fast mode)");
    }
}

/// Per-cycle end-to-end samples, all in milliseconds.
#[derive(Default)]
struct CycleSamples {
    dispatch: Vec<f64>,
    acquire: Vec<f64>,
    recognition: Vec<f64>,
    serial_send: Vec<f64>,
    wire_observe: Vec<f64>,
    total: Vec<f64>,
}

async fn wait_wire_growth(endpoint: &VirtualSerialEndpoint, baseline: usize) -> Result<(), String> {
    let deadline = Instant::now() + WIRE_OBSERVE_TIMEOUT;
    loop {
        if endpoint.written().await.len() > baseline {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "wire bytes must grow past {baseline} within {WIRE_OBSERVE_TIMEOUT:?}"
            ));
        }
        tokio::task::yield_now().await;
    }
}

async fn collect_trace_cycles(fixture: &Fixture, config: &RunConfig) -> CycleSamples {
    let frames = fixture.camera.frame_source();
    let mut samples = CycleSamples {
        dispatch: Vec::with_capacity(config.cycles),
        acquire: Vec::with_capacity(config.cycles),
        recognition: Vec::with_capacity(config.cycles),
        serial_send: Vec::with_capacity(config.cycles),
        wire_observe: Vec::with_capacity(config.cycles),
        total: Vec::with_capacity(config.cycles),
    };
    for index in 0..config.cycles {
        let button = if index.is_multiple_of(2) {
            Button::A
        } else {
            Button::B
        };
        let wire_baseline = fixture.endpoint.written().await.len();
        let cycle_started = Instant::now();

        let started = Instant::now();
        let result = fixture
            .client
            .execute(&execute_request())
            .await
            .expect("trace command execution succeeds");
        assert_eq!(
            result.outcome,
            ScriptExecutionOutcome::Completed,
            "trace command must complete every cycle"
        );
        samples
            .dispatch
            .push(started.elapsed().as_secs_f64() * 1000.0);

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
            panic!("frame acquisition must observe a published frame every cycle");
        };
        assert_eq!(media.frame.size().width(), 1920, "acquired frame width");
        assert_eq!(media.frame.size().height(), 1080, "acquired frame height");
        samples.acquire.push(elapsed_ms);

        let started = Instant::now();
        let sad = sad_vs_solid(media.frame.pixels());
        samples
            .recognition
            .push(started.elapsed().as_secs_f64() * 1000.0);
        // Every published session frame is either the solid frame (SAD 0) or
        // the checker variant; anything else means the fixture drifted.
        assert!(
            sad == 0 || sad == fixture.checker_sad,
            "recognition SAD must match a known session frame"
        );

        let started = Instant::now();
        fixture
            .serial
            .send_controller_state(controller_with(button))
            .await
            .expect("trace serial send must succeed");
        samples
            .serial_send
            .push(started.elapsed().as_secs_f64() * 1000.0);

        let started = Instant::now();
        wait_wire_growth(&fixture.endpoint, wire_baseline)
            .await
            .expect("wire bytes must grow after every serial send");
        samples
            .wire_observe
            .push(started.elapsed().as_secs_f64() * 1000.0);

        samples
            .total
            .push(cycle_started.elapsed().as_secs_f64() * 1000.0);
    }
    samples
}

struct Summaries {
    dispatch: MetricSummary,
    camera: MetricSummary,
    recognition: MetricSummary,
    serial: MetricSummary,
    wire: MetricSummary,
    total: MetricSummary,
}

fn summarize_all(samples: &CycleSamples, config: &RunConfig) -> Summaries {
    Summaries {
        dispatch: summarize(&samples.dispatch, "command_dispatch_latency", config.cycles),
        camera: summarize(&samples.acquire, "camera_frame_latency", config.cycles),
        recognition: summarize(&samples.recognition, "recognition_latency", config.cycles),
        serial: summarize(&samples.serial_send, "serial_send_latency", config.cycles),
        wire: summarize(&samples.wire_observe, "wire_observe_latency", config.cycles),
        total: summarize(&samples.total, "total_end_to_end_latency", config.cycles),
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
            "command_dispatch_latency",
            &summaries.dispatch,
            BLOCKING_P95_COMMAND_DISPATCH_MS,
            "ScriptWorkerClient::execute round-trip of the trace script (production Python readFrame/getCameraImage path)"
        ),
        one_measurement(
            "camera_frame_latency",
            &summaries.camera,
            BLOCKING_P95_CAMERA_FRAME_MS,
            "in-process LatestFrameSource::latest() acquisition (Arc clone, no pixel copy)"
        ),
        one_measurement(
            "recognition_latency",
            &summaries.recognition,
            BLOCKING_P95_RECOGNITION_MS,
            "pixel-SAD stand-in for template matching over latest-frame bytes (NOT the real recognition path)"
        ),
        one_measurement(
            "serial_send_latency",
            &summaries.serial,
            BLOCKING_P95_SERIAL_SEND_MS,
            "send_controller_state until the write completes on the virtual endpoint"
        ),
        one_measurement(
            "wire_observe_latency",
            &summaries.wire,
            BLOCKING_P95_WIRE_OBSERVE_MS,
            "parent-side poll until VirtualSerialEndpoint::written grows past the pre-send baseline"
        ),
        one_measurement(
            "total_end_to_end_latency",
            &summaries.total,
            BLOCKING_P95_TOTAL_END_TO_END_MS,
            "wall-clock per-cycle span from command dispatch start to wire-byte observation"
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
        "profile": std::env::var("POKECON_MAIN_PATH_TRACE_PROFILE").unwrap_or_else(|_| "test".to_owned()),
        "end_to_end_cycles": config.cycles,
        "warmup_seconds": config.warmup_secs,
        "fast_sanity": config.fast,
    })
}

fn header_entry() -> serde_json::Value {
    serde_json::json!({
        "build_sha": std::env::var("POKECON_PERF_BUILD_SHA").unwrap_or_else(|_| "unknown".to_owned()),
        "runner": std::env::var("RUNNER_NAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "unknown".to_owned()),
        "package_version": env!("CARGO_PKG_VERSION"),
        "profile": std::env::var("POKECON_MAIN_PATH_TRACE_PROFILE").unwrap_or_else(|_| "test".to_owned()),
    })
}

fn build_report(
    config: &RunConfig,
    summaries: &Summaries,
    samples: &CycleSamples,
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
        "resolution": TRACE_RESOLUTION.as_str(),
        "frame_width": 1920,
        "frame_height": 1080,
        "mode": mode_entry(config),
        "method": {
            "percentile": "nearest-rank (index = max(0, ceil(q*n)-1)), mirroring scripts/performance/benchmark.py",
            "throughput": "samples / (sum-of-spans / 1000), mirroring _metric_summary",
            "jitter": "p99 - p50 per metric",
            "recognition": "pixel-SAD stand-in for template matching over latest-frame bytes (NOT the real recognition path)",
            "spans": "parent-side only; no worker-internal instrumentation",
            "warmup": "wall-clock warm-up iterations discarded before measurement (fast mode: timed iterations only)",
        },
        "header": header_entry(),
        "measurements": measurements,
        "statistics": {
            "command_dispatch_throughput_per_second": summaries.dispatch.throughput_per_second,
            "frame_throughput_per_second": summaries.camera.throughput_per_second,
            "recognition_throughput_per_second": summaries.recognition.throughput_per_second,
            "serial_throughput_per_second": summaries.serial.throughput_per_second,
            "wire_observe_throughput_per_second": summaries.wire.throughput_per_second,
            "end_to_end_throughput_per_second": summaries.total.throughput_per_second,
            "method": "derived per metric as samples / (sum-of-spans / 1000)",
        },
        "threshold_evaluation": threshold_evaluation,
        "baseline": {
            "status": "fixed-threshold",
            "report_count": 0,
            "aggregation": serde_json::Value::Null,
            "note": "Fixed absolute p95 budgets are required for main-path-trace-v1; no mutable baseline is used",
        },
        "result": result,
        "evaluation": {
            "mode": "blocking",
            "result": result,
            "notes": "fixed p95 budgets, sample counts, error counts, and schema invariants are required gates",
        },
        "cycle_counts": {
            "command_dispatch_latency_ms": samples.dispatch.len(),
            "camera_frame_latency_ms": samples.acquire.len(),
            "recognition_latency_ms": samples.recognition.len(),
            "serial_send_latency_ms": samples.serial_send.len(),
            "wire_observe_latency_ms": samples.wire_observe.len(),
            "total_end_to_end_latency_ms": samples.total.len(),
        },
    })
}

fn build_samples(config: &RunConfig, samples: &CycleSamples) -> serde_json::Value {
    serde_json::json!({
        "fixture_id": FIXTURE_ID,
        "end_to_end_cycles": config.cycles,
        "samples": {
            "command_dispatch_latency_ms": samples.dispatch,
            "camera_frame_latency_ms": samples.acquire,
            "recognition_latency_ms": samples.recognition,
            "serial_send_latency_ms": samples.serial_send,
            "wire_observe_latency_ms": samples.wire_observe,
            "total_end_to_end_latency_ms": samples.total,
        },
    })
}

fn summary_lines(report: &serde_json::Value) -> Vec<String> {
    let mut lines = vec![
        format!("fixture_id: {}", report["fixture_id"]),
        format!(
            "mode: {} end-to-end cycles, {}s warm-up{}",
            report["mode"]["end_to_end_cycles"],
            report["mode"]["warmup_seconds"],
            if report["mode"]["fast_sanity"].as_bool().unwrap_or(false) {
                " (fast sanity)"
            } else {
                ""
            }
        ),
        format!(
            "derived throughput: end-to-end {:.1}/s, dispatch {:.1}/s",
            report["statistics"]["end_to_end_throughput_per_second"],
            report["statistics"]["command_dispatch_throughput_per_second"],
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
        std::env::var("POKECON_MAIN_PATH_TRACE_OUT").unwrap_or_else(|_| "target".to_owned());
    (
        out_dir.clone(),
        format!("{out_dir}/main-path-trace-report.json"),
        format!("{out_dir}/main-path-trace-samples.json"),
        format!("{out_dir}/main-path-trace.log"),
    )
}

fn write_artifacts(
    report: &serde_json::Value,
    samples: &serde_json::Value,
) -> (String, String, String) {
    let (out_dir, report_path, samples_path, log_path) = artifact_paths();
    std::fs::create_dir_all(&out_dir).expect("trace output dir must be creatable");
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
    let mut log = vec!["pokecon main-path-trace harness log".to_owned()];
    log.extend(summary_lines(report));
    log.push(format!(
        "artifacts: {report_path} {samples_path} {log_path}"
    ));
    std::fs::write(&log_path, log.join("\n") + "\n").expect("log must be writable");
    for line in summary_lines(report) {
        println!("{line}");
    }
    eprintln!("main-path-trace artifacts: {report_path} {samples_path} {log_path}");
    (report_path, samples_path, log_path)
}

fn write_reader_lifecycle_artifact(
    ring: &SharedFrameRing,
    (cooperative_acknowledged, forced, exit_success): (bool, bool, bool),
    recovered_pins: usize,
) -> String {
    let (out_dir, _, _, _) = artifact_paths();
    let path = format!("{out_dir}/reader-lifecycle-report.json");
    let report = serde_json::json!({
        "schema": "reader-lifecycle/1",
        "fixture_id": FIXTURE_ID,
        "worker_kind": "script",
        "reader_surface": "rust/pokecon/src/worker_binary/script/python.rs::initialize_python",
        "mapping": ring.descriptor(),
        "shutdown": {
            "cooperative_acknowledged": cooperative_acknowledged,
            "forced": forced,
            "process_reaped": true,
            "exit_success": exit_success,
            "replacement_reader_absent": true,
        },
        "recovered_pin_count": recovered_pins,
        "result": if recovered_pins == 0 { "passed" } else { "failed" },
    });
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&report).expect("reader lifecycle report serializes"),
    )
    .expect("reader lifecycle report is writable");
    eprintln!("main-path-trace reader lifecycle artifact: {path}");
    path
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
    let measurements = parsed["measurements"]
        .as_array()
        .expect("measurements must be an array");
    assert_eq!(measurements.len(), 6, "six latency measurements");
    for entry in measurements {
        let name = entry["metric"].as_str().unwrap_or("<missing>");
        assert_eq!(entry["sample_count"], config.cycles, "{name} sample count");
        assert!(
            entry["threshold"]["blocking"].as_bool().unwrap_or(false),
            "{name} threshold must be blocking"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn main_path_trace_virtual() {
    let config = run_config();
    let overall = Duration::from_secs(config.warmup_secs.saturating_add(900));
    tokio::time::timeout(overall, async {
        let checker = build_checker_frame();
        let fixture = start_fixture(&checker).await;
        warm_up(&fixture, &config).await;
        let samples = collect_trace_cycles(&fixture, &config).await;
        let summaries = summarize_all(&samples, &config);
        let report = build_report(&config, &summaries, &samples);
        let sample_doc = build_samples(&config, &samples);
        let (report_path, _, _) = write_artifacts(&report, &sample_doc);
        assert_report_shape(&report_path, &config);

        let stop_report = stop_worker(&fixture.worker).await;
        let reader_ring = fixture
            .host
            .camera_ring
            .lock()
            .expect("camera ring mutex is not poisoned")
            .clone()
            .expect("worker reader used the host-published mapping");
        let recovered = reader_ring
            .recover_reader_pins(true, true)
            .expect("reaped worker permits reader-pin recovery");
        eprintln!(
            "main-path-trace reader lifecycle: script-worker-reaped=true, replacement-reader-absent=true, recovered-pins={recovered}"
        );
        assert_eq!(
            recovered, 0,
            "a cooperatively reaped Python reader must not leave an abandoned pin"
        );
        let _reader_lifecycle_path =
            write_reader_lifecycle_artifact(&reader_ring, stop_report, recovered);
        fixture.serial.disconnect().await.unwrap();
        fixture
            .camera
            .shutdown(Duration::from_secs(5))
            .expect("final teardown shutdown must settle");
    })
    .await
    .expect("main-path trace harness must settle within the overall bound");
}
