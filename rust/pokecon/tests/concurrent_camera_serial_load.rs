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

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use pokecon::integration_test_support::camera::{
    CameraConfig, CameraManager, CameraSelector, CaptureResolution, FlipMode, RecordedFrame,
    VirtualCameraBackend, VirtualOpenPlan, VirtualSessionPlan,
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
const OVERALL_TIMEOUT: Duration = Duration::from_secs(30);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

fn controller_with(button: Button) -> ControllerState {
    let mut state = ControllerState::NEUTRAL;
    state.buttons.set(button, true);
    state
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
        [RecordedFrame::Solid([9, 18, 27])],
    )));
    let camera = CameraManager::start(
        Arc::new(backend),
        CameraConfig::new(CameraSelector::Index(0), 30, CaptureResolution::R640x360).unwrap(),
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

        fixture.serial.disconnect().await.unwrap();
        fixture.camera.shutdown(SHUTDOWN_TIMEOUT).unwrap();
    })
    .await
    .expect("concurrent load must settle within the overall bound");
}
