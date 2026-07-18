use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use pokecon_worker::WorkerKind;
use pokecon_worker::generation::GenerationError;
use pokecon_worker::ipc::{DisconnectReason, IpcValue, ResourceSafety};
use pokecon_worker::supervisor::{StopPurpose, SupervisorError, WorkerLaunch, WorkerSupervisor};

#[derive(Debug)]
struct ControllerSafetyProbe {
    buttons_pressed: AtomicBool,
    sticks_deflected: AtomicBool,
    touch_active: AtomicBool,
    releases: AtomicUsize,
}

impl ControllerSafetyProbe {
    fn active() -> Self {
        Self {
            buttons_pressed: AtomicBool::new(true),
            sticks_deflected: AtomicBool::new(true),
            touch_active: AtomicBool::new(true),
            releases: AtomicUsize::new(0),
        }
    }

    fn assert_neutral_once(&self) {
        assert!(!self.buttons_pressed.load(Ordering::Acquire));
        assert!(!self.sticks_deflected.load(Ordering::Acquire));
        assert!(!self.touch_active.load(Ordering::Acquire));
        assert_eq!(self.releases.load(Ordering::Acquire), 1);
    }
}

impl ResourceSafety for ControllerSafetyProbe {
    fn force_release(&self) {
        self.buttons_pressed.store(false, Ordering::Release);
        self.sticks_deflected.store(false, Ordering::Release);
        self.touch_active.store(false, Ordering::Release);
        self.releases.fetch_add(1, Ordering::AcqRel);
    }
}

#[tokio::test]
async fn managed_worker_uses_protocol_stdout_and_cooperative_stop() {
    let supervisor = WorkerSupervisor::new();
    let safety = Arc::new(ControllerSafetyProbe::active());
    let worker = supervisor
        .spawn(
            WorkerLaunch::managed(env!("CARGO_BIN_EXE_pokecon-worker"), WorkerKind::Script),
            safety.clone(),
        )
        .await
        .expect("worker starts");
    let mut diagnostics = worker
        .take_diagnostics()
        .expect("stderr diagnostic receiver is available once");
    let response = worker
        .connection()
        .request("worker.ping", IpcValue::Nil)
        .await
        .expect("a typed response proves stdout contained protocol frames only");
    let IpcValue::Map(response) = response else {
        panic!("ping response must be a map");
    };
    assert_eq!(
        response.get("kind"),
        Some(&IpcValue::String("script".to_owned()))
    );
    let diagnostic = tokio::time::timeout(Duration::from_secs(2), diagnostics.recv())
        .await
        .expect("worker emitted OOB startup diagnostic")
        .expect("diagnostic pipe remains open");
    assert!(!diagnostic.bytes.is_empty());

    let report = worker
        .stop(StopPurpose::ApplicationShutdown, Duration::from_secs(2))
        .await
        .expect("cooperative stop succeeds");
    assert!(report.cooperative_acknowledged);
    assert!(!report.forced);
    assert!(report.exit.success);
    safety.assert_neutral_once();
}

#[tokio::test]
async fn crash_and_malformed_frames_release_rust_owned_resources() {
    for (mode, expect_protocol_violation) in [
        ("crash", false),
        ("eof", false),
        ("partial-frame", true),
        ("oversize", true),
    ] {
        let supervisor = WorkerSupervisor::new();
        let safety = Arc::new(ControllerSafetyProbe::active());
        let worker = supervisor
            .spawn(
                WorkerLaunch::custom(
                    env!("CARGO_BIN_EXE_pokecon-worker-fault-fixture"),
                    WorkerKind::Script,
                )
                .argument(mode),
                safety.clone(),
            )
            .await
            .expect("fault fixture starts");
        let _exit = worker.wait().await.expect("fault fixture is reaped");
        let reason = worker
            .connection()
            .disconnect_reason()
            .expect("process exit disconnects transport");
        if expect_protocol_violation {
            assert!(
                matches!(reason, DisconnectReason::ProtocolViolation(_)),
                "{mode} must be rejected as a protocol violation, got {reason:?}"
            );
        }
        safety.assert_neutral_once();
        assert_eq!(worker.connection().disconnect_transition_count(), 1);
    }
}

#[tokio::test]
async fn dynamic_worker_is_forced_only_at_app_shutdown_and_never_regenerated() {
    let supervisor = WorkerSupervisor::new();
    let safety = Arc::new(ControllerSafetyProbe::active());
    let launch = WorkerLaunch::custom(
        env!("CARGO_BIN_EXE_pokecon-worker-fault-fixture"),
        WorkerKind::Dynamic,
    )
    .argument("ignore-shutdown");
    let worker = supervisor
        .spawn(launch.clone(), safety.clone())
        .await
        .expect("dynamic fixture starts");
    assert!(matches!(
        worker
            .stop(StopPurpose::ProfileSwitch, Duration::from_millis(20))
            .await,
        Err(SupervisorError::DynamicProfileSwitchForbidden)
    ));
    assert!(worker.generation().phase() == pokecon_worker::generation::GenerationPhase::Running);

    let report = worker
        .stop(StopPurpose::ApplicationShutdown, Duration::from_millis(100))
        .await
        .expect("shutdown force-stops an unresponsive dynamic worker");
    assert!(report.forced);
    safety.assert_neutral_once();

    assert!(matches!(
        supervisor
            .spawn(launch, Arc::new(ControllerSafetyProbe::active()))
            .await,
        Err(SupervisorError::Generation(
            GenerationError::DynamicRestartForbidden
        ))
    ));
}

#[tokio::test]
async fn profile_switch_force_stops_and_replaces_only_the_script_worker() {
    let supervisor = WorkerSupervisor::new();
    let old_safety = Arc::new(ControllerSafetyProbe::active());
    let old_worker = supervisor
        .spawn(
            WorkerLaunch::custom(
                env!("CARGO_BIN_EXE_pokecon-worker-fault-fixture"),
                WorkerKind::Script,
            )
            .argument("ignore-shutdown"),
            old_safety.clone(),
        )
        .await
        .expect("old script fixture starts");
    let old_generation = old_worker.generation().id();
    let report = old_worker
        .stop(StopPurpose::ProfileSwitch, Duration::from_millis(100))
        .await
        .expect("profile switch reaps an unresponsive script worker");
    assert!(report.forced);
    old_safety.assert_neutral_once();

    let replacement_safety = Arc::new(ControllerSafetyProbe::active());
    let replacement = supervisor
        .spawn(
            WorkerLaunch::custom(
                env!("CARGO_BIN_EXE_pokecon-worker-fault-fixture"),
                WorkerKind::Script,
            )
            .argument("eof"),
            replacement_safety.clone(),
        )
        .await
        .expect("script replacement starts only after old OS reap");
    assert_ne!(replacement.generation().id(), old_generation);
    replacement.wait().await.expect("replacement is reaped");
    replacement_safety.assert_neutral_once();
}
