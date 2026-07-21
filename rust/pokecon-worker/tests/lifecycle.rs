use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use pokecon_dynamic::protocol::DynamicInitializeRequest;
use pokecon_dynamic::protocol::PYTHON_SITE_PACKAGES_ENV;
use pokecon_dynamic::{
    CommandDisplayItem, CommandInfo, DynamicConfigControl, DynamicConfigLanguage, DynamicHost,
    InMemoryDynamicHost,
};
use pokecon_worker::WorkerKind;
use pokecon_worker::dynamic::DynamicWorkerClient;
use pokecon_worker::generation::GenerationError;
use pokecon_worker::ipc::{
    DisconnectReason, IpcValue, LogLevel, LogPayload, LogTarget, ResourceSafety,
};
use pokecon_worker::supervisor::{StopPurpose, SupervisorError, WorkerLaunch, WorkerSupervisor};
use serde_json::json;
use tempfile::TempDir;

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

#[derive(Debug)]
struct DynamicHostSafety {
    host: Arc<InMemoryDynamicHost>,
    releases: AtomicUsize,
}

impl DynamicHostSafety {
    fn new(host: Arc<InMemoryDynamicHost>) -> Self {
        Self {
            host,
            releases: AtomicUsize::new(0),
        }
    }
}

impl ResourceSafety for DynamicHostSafety {
    fn force_release(&self) {
        self.host
            .controller_reset()
            .expect("test host controller reset succeeds");
        self.releases.fetch_add(1, Ordering::AcqRel);
    }
}

fn dynamic_settings() -> BTreeMap<String, serde_json::Value> {
    BTreeMap::from([
        ("language".to_owned(), json!("ja")),
        ("commands.tag_match_mode".to_owned(), json!("exact")),
        ("dynamic.callback_soft_timeout_ms".to_owned(), json!(2000)),
        (
            "dynamic.callback_soft_timeout_grace_ms".to_owned(),
            json!(1000),
        ),
        ("dynamic.callback_hard_timeout_ms".to_owned(), json!(5000)),
        ("dynamic.callback_max_concurrency".to_owned(), json!(8)),
        ("dynamic.callback_queue_capacity".to_owned(), json!(1024)),
    ])
}

const LUA_DYNAMIC_SOURCE: &str = r#"
local commands_available = pcall(require, "Commands")
assert(not commands_available)
print("lua-frame-safe")
pokecon.opt.language = "EN"
pokecon.controller.update({a = true})
pokecon.autocmd.on("CommandStartPre", {
    callback = function()
        assert(pokecon.profile.current() == "default")
        assert(#pokecon.profile.list() == 2)
        assert(pokecon.state.active_profile == "default")
        assert(pokecon.profile.switch("Other"))
        pokecon.state.tags = {"worker-ipc"}
        return false
    end,
})
pokecon.commands.sort.callback = function(commands)
    return {pokecon.commands.separator("Pinned"), commands[1]}
end
"#;

const PYTHON_DYNAMIC_SOURCE: &str = r#"
import pokecon
import os
import worker_site_fixture
assert worker_site_fixture.VALUE == "venv-only"
assert "PATH" not in os.environ
assert os.environ["POKECON_WORKER_TEST_SENTINEL"] == "retained"
try:
    import Commands
except ModuleNotFoundError:
    pass
else:
    raise RuntimeError("Commands namespace leaked into dynamic config")
print("python-frame-safe")
pokecon.opt.language = "JA"
pokecon.controller.reset()
pokecon.controller.update({"a": True})
"#;

async fn assert_stdout_log(logs: &mut tokio::sync::mpsc::Receiver<LogPayload>, expected: &str) {
    let log = tokio::time::timeout(Duration::from_secs(2), logs.recv())
        .await
        .expect("language output arrives without corrupting stdout framing")
        .expect("dynamic log channel stays open");
    assert_eq!(log.level, LogLevel::Info);
    assert_eq!(log.target, LogTarget::Stdout);
    assert_eq!(log.message, expected);
}

async fn verify_lua_runtime(
    client: &DynamicWorkerClient,
    host: &InMemoryDynamicHost,
    logs: &mut tokio::sync::mpsc::Receiver<LogPayload>,
) {
    let loaded = client
        .control(&DynamicConfigControl::LoadContent {
            language: DynamicConfigLanguage::Lua,
            content: LUA_DYNAMIC_SOURCE.to_owned(),
        })
        .await
        .expect("Lua control request completes");
    assert!(loaded.loaded, "{:?}", loaded.diagnostic);
    assert_eq!(
        host.settings_snapshot()
            .expect("host settings are readable")["language"],
        json!("en")
    );
    assert!(host.controller_state().buttons.a);
    assert_stdout_log(logs, "lua-frame-safe").await;
    assert!(
        client
            .emit("CommandStartPre")
            .await
            .expect("event crosses worker IPC")
            .cancelled
    );
    assert_eq!(host.profile_current().unwrap(), "Other");
    assert_eq!(
        host.state_snapshot().unwrap()["tags"],
        json!(["worker-ipc"])
    );

    let command = CommandInfo {
        name: "Example".to_owned(),
        module_path: "Commands.Example".to_owned(),
        class_name: "Example".to_owned(),
        tags: vec!["sample".to_owned()],
    };
    assert_eq!(
        client
            .sort_commands(std::slice::from_ref(&command))
            .await
            .expect("sort callback crosses worker IPC"),
        vec![
            CommandDisplayItem::Separator {
                label: Some("Pinned".to_owned()),
            },
            CommandDisplayItem::Command { command },
        ]
    );
    assert!(
        client
            .tag_matches(
                "sample",
                &CommandInfo {
                    name: "Tagged".to_owned(),
                    module_path: "Commands.Tagged".to_owned(),
                    class_name: "Tagged".to_owned(),
                    tags: vec!["sample".to_owned()],
                },
            )
            .await
            .expect("tag matcher crosses worker IPC")
    );
}

async fn verify_python_runtime(
    client: &DynamicWorkerClient,
    host: &InMemoryDynamicHost,
    logs: &mut tokio::sync::mpsc::Receiver<LogPayload>,
) {
    let loaded = client
        .control(&DynamicConfigControl::LoadContent {
            language: DynamicConfigLanguage::Python,
            content: PYTHON_DYNAMIC_SOURCE.to_owned(),
        })
        .await
        .expect("Python control request completes");
    assert!(loaded.loaded, "{:?}", loaded.diagnostic);
    assert_eq!(
        host.settings_snapshot()
            .expect("host settings are readable")["language"],
        json!("ja")
    );
    assert_stdout_log(logs, "python-frame-safe").await;

    let failed_generation = client
        .status()
        .await
        .expect("status after both runtimes succeeds")
        .generation;
    let rejected = client
        .control(&DynamicConfigControl::LoadContent {
            language: DynamicConfigLanguage::Python,
            content: "import Commands\n".to_owned(),
        })
        .await
        .expect("evaluation failures use a typed load result");
    assert!(!rejected.loaded);
    let status = client.status().await.expect("worker remains responsive");
    assert_eq!(status.generation, failed_generation);
    assert_eq!(
        status.initialized_languages,
        vec![DynamicConfigLanguage::Python, DynamicConfigLanguage::Lua]
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        while host.diagnostics().is_empty() || host.command_recompute_requests() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("diagnostic and recompute events reach the parent host");
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
    assert!(worker.take_diagnostics().is_none());
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
    let report = worker
        .stop(StopPurpose::ApplicationShutdown, Duration::from_secs(2))
        .await
        .expect("cooperative stop succeeds");
    assert!(report.cooperative_acknowledged);
    assert!(!report.forced);
    if !report.exit.success {
        let mut failure_diagnostics = Vec::new();
        while let Ok(Some(diagnostic)) =
            tokio::time::timeout(Duration::from_millis(100), diagnostics.recv()).await
        {
            failure_diagnostics.extend_from_slice(&diagnostic.bytes);
        }
        panic!(
            "unexpected stop report: {report:?}; stderr: {}",
            String::from_utf8_lossy(&failure_diagnostics)
        );
    }
    safety.assert_neutral_once();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dynamic_worker_runs_both_languages_over_bidirectional_ipc() {
    let temporary = TempDir::new().expect("temporary config root is created");
    let site_packages = temporary.path().join("site-packages");
    std::fs::create_dir(&site_packages).expect("site-packages fixture is created");
    std::fs::write(
        site_packages.join("worker_site_fixture.py"),
        "VALUE = \"venv-only\"\n",
    )
    .expect("site-packages fixture is written");
    let host = Arc::new(
        InMemoryDynamicHost::new(
            dynamic_settings(),
            BTreeMap::from([
                ("active_profile".to_owned(), json!("default")),
                ("available_profiles".to_owned(), json!(["default", "Other"])),
                ("command_candidates".to_owned(), json!([])),
                ("tags".to_owned(), json!([])),
            ]),
        )
        .expect("dynamic host registry is valid"),
    );
    let neutral_controller = host.controller_state();
    let safety = Arc::new(DynamicHostSafety::new(host.clone()));
    let supervisor = WorkerSupervisor::new();
    let worker = supervisor
        .spawn(
            WorkerLaunch::managed(env!("CARGO_BIN_EXE_pokecon-worker"), WorkerKind::Dynamic)
                .clear_environment()
                .environment(PYTHON_SITE_PACKAGES_ENV, &site_packages)
                .environment("POKECON_WORKER_TEST_SENTINEL", "retained"),
            safety.clone(),
        )
        .await
        .expect("dynamic worker starts");
    let client = DynamicWorkerClient::attach(worker.clone(), host.clone())
        .expect("dynamic client attaches before initialization");
    let mut logs = client.take_logs().expect("log receiver is available once");

    assert_eq!(
        client.status().await.expect("status request succeeds"),
        pokecon_dynamic::protocol::DynamicWorkerStatus::uninitialized()
    );
    let initialized = client
        .initialize(&DynamicInitializeRequest {
            config_root: temporary.path().to_path_buf(),
            home: Some(temporary.path().to_path_buf()),
            primary: DynamicConfigLanguage::Lua,
        })
        .await
        .expect("dynamic engine initializes in the child process");
    assert!(initialized.status.initialized);
    assert_eq!(initialized.status.generation, 0);
    assert_eq!(
        initialized.status.initialized_languages,
        vec![DynamicConfigLanguage::Lua]
    );
    assert!(initialized.startup_load.is_none());

    verify_lua_runtime(&client, host.as_ref(), &mut logs).await;
    verify_python_runtime(&client, host.as_ref(), &mut logs).await;
    worker
        .connection()
        .request("worker.ping", IpcValue::Nil)
        .await
        .expect("protocol remains framed after language stdout and errors");

    let report = worker
        .stop(StopPurpose::ApplicationShutdown, Duration::from_secs(2))
        .await
        .expect("dynamic worker stops cooperatively at application shutdown");
    assert!(report.cooperative_acknowledged);
    assert!(!report.forced);
    assert!(report.exit.success);
    assert_eq!(host.controller_state(), neutral_controller);
    assert_eq!(safety.releases.load(Ordering::Acquire), 1);
    assert_eq!(client.dropped_log_count(), 0);
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
        let mut diagnostics =
            (mode == "crash").then(|| worker.take_diagnostics().expect("stderr is available"));
        let _exit = worker.wait().await.expect("fault fixture is reaped");
        if let Some(diagnostics) = diagnostics.as_mut() {
            let diagnostic = tokio::time::timeout(Duration::from_secs(2), diagnostics.recv())
                .await
                .expect("explicit fixture stderr arrives")
                .expect("explicit fixture diagnostic is retained");
            assert!(String::from_utf8_lossy(&diagnostic.bytes).contains("deliberate worker crash"));
        }
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
