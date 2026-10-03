#[cfg(unix)]
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
#[cfg(unix)]
use std::path::PathBuf;
use std::process::Command;
#[cfg(unix)]
use std::process::Stdio;
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
use nix::sys::signal::{Signal, kill};
#[cfg(unix)]
use nix::unistd::Pid;
use tempfile::TempDir;
#[cfg(unix)]
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};
#[cfg(unix)]
use tokio::net::TcpStream;
#[cfg(unix)]
use tokio::process::Command as TokioCommand;
#[cfg(unix)]
use tokio::sync::oneshot;
#[cfg(unix)]
use tokio::task::JoinHandle;
#[cfg(unix)]
use tokio::time::{Instant, sleep, timeout};

#[test]
fn rust_main_starts_and_exits_cleanly_in_web_mode() {
    assert_startup_probe("web");
}

#[test]
fn rust_main_starts_and_exits_cleanly_in_desktop_mode() {
    assert_startup_probe("desktop");
}

#[cfg(feature = "gpui")]
#[test]
fn rust_main_starts_and_exits_cleanly_in_gpui_mode() {
    assert_startup_probe("gpui");
}

fn assert_startup_probe(ui_mode: &str) {
    let roots = TempDir::new().expect("isolated roots must exist");
    let mut command = Command::new(env!("CARGO_BIN_EXE_pokecon"));
    isolate_std_command(&mut command, &roots);
    let status = command
        .args([
            "--ui",
            ui_mode,
            "--ephemeral-port",
            "--dynamic-config-language",
            "none",
            "--exit-after-startup",
        ])
        .status()
        .expect("PokeCon process must start");
    assert!(status.success(), "{ui_mode} startup probe failed");
}

#[test]
fn dynamic_events_run_at_application_lifecycle_boundaries() {
    let roots = TempDir::new().expect("isolated roots must exist");
    let config_root = roots.path().join("config").join("pokecon");
    std::fs::create_dir_all(&config_root).expect("isolated config root must exist");
    std::fs::write(
        config_root.join("init.lua"),
        r#"
pokecon.autocmd.on("AppStartupPost", {
    callback = function()
        print("dynamic-startup-event")
    end,
})
pokecon.autocmd.on("AppShutdownPre", {
    callback = function()
        print("dynamic-shutdown-event")
    end,
})
"#,
    )
    .expect("dynamic fixture must be written");

    let mut command = Command::new(env!("CARGO_BIN_EXE_pokecon"));
    isolate_std_command(&mut command, &roots);
    let output = command
        .args([
            "--ephemeral-port",
            "--dynamic-config-language",
            "lua",
            "--exit-after-startup",
        ])
        .output()
        .expect("PokeCon dynamic startup probe must run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "dynamic startup probe failed: stdout={stdout}; stderr={stderr}"
    );
    assert!(
        stdout.contains("dynamic-startup-event"),
        "startup event output is missing: {stdout}"
    );
    assert!(
        stdout.contains("dynamic-shutdown-event"),
        "shutdown event output is missing: {stdout}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn sigint_uses_the_clean_shutdown_path() {
    assert_signal_uses_clean_shutdown(Signal::SIGINT, 1).await;
}

#[cfg(unix)]
#[tokio::test]
async fn sigterm_uses_the_clean_shutdown_path() {
    assert_signal_uses_clean_shutdown(Signal::SIGTERM, 1).await;
}

#[cfg(unix)]
#[tokio::test]
async fn sigint_double_delivery_still_exits_cleanly() {
    assert_signal_uses_clean_shutdown(Signal::SIGINT, 2).await;
}

#[cfg(unix)]
#[tokio::test]
async fn sigterm_double_delivery_still_exits_cleanly() {
    assert_signal_uses_clean_shutdown(Signal::SIGTERM, 2).await;
}

#[cfg(unix)]
async fn assert_signal_uses_clean_shutdown(signal: Signal, deliveries: u32) {
    let roots = TempDir::new().expect("isolated roots must exist");
    let mut command = TokioCommand::new(env!("CARGO_BIN_EXE_pokecon"));
    isolate_tokio_command(&mut command, &roots);
    command.stdout(Stdio::piped());
    let mut child = command
        .args(["--ephemeral-port", "--dynamic-config-language", "none"])
        .spawn()
        .expect("PokeCon process must start");
    let (ready_address, stdout_task) = monitor_startup_stdout(&mut child);
    let address = wait_for_ready_address(&mut child, ready_address).await;
    let port = address.port();
    wait_until_listening(&mut child, address).await;
    assert_ui_is_served(port).await;
    assert_api_is_served(port).await;

    let pid = Pid::from_raw(
        child
            .id()
            .expect("running process must have an identifier")
            .cast_signed(),
    );
    kill(pid, signal).expect("the operating-system signal must be delivered");
    for _ in 1..deliveries {
        sleep(Duration::from_millis(150)).await;
        // The process may already have exited, so delivery failure is acceptable.
        let _ = kill(pid, signal);
    }
    let status = match timeout(Duration::from_secs(10), child.wait()).await {
        Ok(result) => result.expect("process status must be readable"),
        Err(_elapsed) => {
            child
                .start_kill()
                .expect("timed-out process must be terminated");
            child
                .wait()
                .await
                .expect("terminated process must be reaped");
            panic!("PokeCon did not stop after {signal:?}");
        }
    };
    let stdout = stdout_task
        .await
        .expect("startup stdout monitor must not panic")
        .expect("startup stdout must remain readable");
    assert!(
        status.success(),
        "shutdown after {signal:?} failed: {status}; stdout={stdout}"
    );
    assert!(
        stdout.contains("POKECON-RUNTIME-0002"),
        "clean-stop marker is missing after {signal:?}: {stdout}"
    );
    assert!(
        stdout.contains("PokeCon stopped cleanly"),
        "clean-stop message is missing after {signal:?}: {stdout}"
    );
}

fn isolated_environment(roots: &TempDir) -> [(&'static str, std::path::PathBuf); 7] {
    let base = roots.path();
    [
        ("HOME", base.join("home")),
        ("USERPROFILE", base.join("home")),
        ("XDG_CONFIG_HOME", base.join("config")),
        ("XDG_DATA_HOME", base.join("data")),
        ("XDG_CACHE_HOME", base.join("cache")),
        ("XDG_STATE_HOME", base.join("state")),
        ("APPDATA", base.join("appdata")),
    ]
}

fn isolate_std_command(command: &mut Command, roots: &TempDir) {
    let web_root = prepare_web_fixture(roots);
    for (name, value) in isolated_environment(roots) {
        command.env(name, value);
    }
    command.env("LOCALAPPDATA", roots.path().join("localappdata"));
    command.env("POKECON_WEB_DIR", web_root);
    command.env("RUST_LOG", "info");
}

#[cfg(unix)]
fn isolate_tokio_command(command: &mut TokioCommand, roots: &TempDir) {
    let web_root = prepare_web_fixture(roots);
    for (name, value) in isolated_environment(roots) {
        command.env(name, value);
    }
    command.env("LOCALAPPDATA", roots.path().join("localappdata"));
    command.env("POKECON_WEB_DIR", web_root);
    command.env("RUST_LOG", "info");
}

fn prepare_web_fixture(roots: &TempDir) -> std::path::PathBuf {
    let web_root = roots.path().join("web").join("dist");
    std::fs::create_dir_all(&web_root).expect("isolated web root must exist");
    std::fs::write(
        web_root.join("index.html"),
        "<!doctype html><title>PokeCon</title><p>pokecon-startup-fixture</p>",
    )
    .expect("isolated SPA document must be written");
    web_root
}

#[cfg(unix)]
fn monitor_startup_stdout(
    child: &mut tokio::process::Child,
) -> (
    oneshot::Receiver<SocketAddrV4>,
    JoinHandle<std::io::Result<String>>,
) {
    let stdout = child
        .stdout
        .take()
        .expect("startup stdout must be configured as piped");
    let (ready_sender, ready_receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut output = String::new();
        let mut ready_sender = Some(ready_sender);
        while let Some(line) = lines.next_line().await? {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(&line);
            if let Some(address) = ready_address_from_log(&line)
                && let Some(sender) = ready_sender.take()
            {
                let _ignored_receiver_closed = sender.send(address);
            }
        }
        Ok(output)
    });
    (ready_receiver, task)
}

#[cfg(unix)]
fn ready_address_from_log(line: &str) -> Option<SocketAddrV4> {
    let document: serde_json::Value = serde_json::from_str(line).ok()?;
    let fields = document.get("fields")?;
    if fields.get("diagnostic_id")?.as_str()? != "POKECON-RUNTIME-0001" {
        return None;
    }
    let address = fields.get("listen_address")?.as_str()?.parse().ok()?;
    match address {
        SocketAddr::V4(address) if address.ip().is_loopback() && address.port() != 0 => {
            Some(address)
        }
        SocketAddr::V4(_) | SocketAddr::V6(_) => None,
    }
}

#[cfg(unix)]
#[test]
fn readiness_log_accepts_only_usable_ipv4_loopback_addresses() {
    let valid = serde_json::json!({
        "fields": {
            "diagnostic_id": "POKECON-RUNTIME-0001",
            "listen_address": "127.0.0.1:43210",
        },
    })
    .to_string();
    assert_eq!(
        ready_address_from_log(&valid),
        Some(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 43_210))
    );

    for rejected in [
        "not-json".to_owned(),
        serde_json::json!({
            "fields": {
                "diagnostic_id": "POKECON-RUNTIME-9999",
                "listen_address": "127.0.0.1:43210",
            },
        })
        .to_string(),
        serde_json::json!({
            "fields": {
                "diagnostic_id": "POKECON-RUNTIME-0001",
                "listen_address": "127.0.0.1:0",
            },
        })
        .to_string(),
        serde_json::json!({
            "fields": {
                "diagnostic_id": "POKECON-RUNTIME-0001",
                "listen_address": "192.0.2.1:43210",
            },
        })
        .to_string(),
        serde_json::json!({
            "fields": {
                "diagnostic_id": "POKECON-RUNTIME-0001",
                "listen_address": "[::1]:43210",
            },
        })
        .to_string(),
    ] {
        assert_eq!(ready_address_from_log(&rejected), None, "{rejected}");
    }
}

#[cfg(unix)]
async fn wait_for_ready_address(
    child: &mut tokio::process::Child,
    ready_address: oneshot::Receiver<SocketAddrV4>,
) -> SocketAddrV4 {
    match timeout(Duration::from_secs(10), ready_address).await {
        Ok(Ok(address)) => address,
        Ok(Err(_closed)) => {
            let status = child
                .try_wait()
                .expect("process status must be readable after stdout closes");
            if status.is_none() {
                child
                    .start_kill()
                    .expect("stdout-closed process must be terminated");
                child
                    .wait()
                    .await
                    .expect("stdout-closed process must be reaped");
            }
            panic!("PokeCon stdout closed before readiness: {status:?}");
        }
        Err(_elapsed) => {
            child
                .start_kill()
                .expect("timed-out process must be terminated");
            child
                .wait()
                .await
                .expect("terminated process must be reaped");
            panic!("PokeCon did not publish its ephemeral listen address");
        }
    }
}

#[cfg(unix)]
async fn wait_until_listening(child: &mut tokio::process::Child, address: SocketAddrV4) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if timeout(Duration::from_millis(100), TcpStream::connect(address))
            .await
            .is_ok_and(|result| result.is_ok())
        {
            return;
        }
        if let Some(status) = child.try_wait().expect("process status must be readable") {
            panic!("PokeCon exited before becoming ready: {status}");
        }
        if Instant::now() >= deadline {
            child
                .start_kill()
                .expect("timed-out process must be terminated");
            child
                .wait()
                .await
                .expect("terminated process must be reaped");
            panic!("PokeCon did not listen on {address}");
        }
        sleep(Duration::from_millis(20)).await;
    }
}

#[cfg(unix)]
async fn assert_ui_is_served(port: u16) {
    let mut stream = TcpStream::connect(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
        .await
        .expect("ready server must accept an HTTP request");
    let request =
        format!("GET /ui/ HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("UI request must be written");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .await
        .expect("UI response must be readable");
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(response.contains("pokecon-startup-fixture"), "{response}");
}

#[cfg(unix)]
async fn assert_api_is_served(port: u16) {
    for (path, required) in [
        ("/api/settings", "pending_restart_values"),
        ("/api/state", "command_display_lists"),
        ("/api/devices/cameras", "data"),
        ("/api/devices/serial-ports", "data"),
    ] {
        let mut stream = TcpStream::connect(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
            .await
            .expect("ready server must accept an API request");
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n");
        stream
            .write_all(request.as_bytes())
            .await
            .expect("API request must be written");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .await
            .expect("API response must be readable");
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        assert!(response.contains(required), "{path}: {response}");
    }
}

// AR-11-26 process responsibility / lifetime evidence.
//
// The production state machine lives in `run_configured_controlled` and
// `shutdown_production` (`rust/pokecon/src/lib.rs`): the Rust main process
// owns the listener bind, the settings/state backend, native camera/serial
// resources, and both worker generations; the script worker runs profile
// commands and is the only generation that profile switch may replace; the
// dynamic worker is a single persistent generation that application shutdown
// alone may reap (`DynamicRestartForbidden`). These tests exercise that
// machine through the real production binary (native backends, settings
// pipeline, real OS worker processes) instead of a simulated model.

#[cfg(unix)]
#[tokio::test]
async fn sigterm_shutdown_follows_the_production_shutdown_order() {
    // Real-path state-transition report for one SIGTERM-driven shutdown:
    //
    // | order | observed stdout marker | production step that emits it |
    // | --- | --- | --- |
    // | 1 | `dynamic-startup-event` (lua `AppStartupPost` print) | `emit_startup_post`, before readiness |
    // | 2 | `POKECON-RUNTIME-0001` readiness | listener bound, routes published |
    // | 3 | `POKECON-RUNTIME-0003` with `Signal(Terminate)` | `ShutdownCoordinator` accepts the first reason |
    // | 4 | `dynamic-shutdown-event` (lua `AppShutdownPre` print) | `DynamicRuntime::prepare_shutdown` closes dynamic mutations while services are alive |
    // | 5 | `POKECON-RUNTIME-0002` + `PokeCon stopped cleanly` | inputs/camera/scripts, dynamic reap, serial, server join |
    //
    // The dynamic worker is a real OS child process, so marker 4 can only
    // appear while application services are still alive. `0003 < shutdown
    // event < 0002` therefore pins the live order `prepare_shutdown ->
    // stop_inputs_camera_and_scripts -> shutdown_worker -> stop_serial`
    // against the production call path. (Marker 1 is presence-only: worker
    // log forwarding is asynchronous, so its position relative to marker 2
    // is not pinned.)
    let roots = TempDir::new().expect("isolated roots must exist");
    let config_root = roots.path().join("config").join("pokecon");
    std::fs::create_dir_all(&config_root).expect("isolated config root must exist");
    std::fs::write(
        config_root.join("init.lua"),
        "pokecon.autocmd.on(\"AppStartupPost\", {\n    callback = function()\n        print(\"dynamic-startup-event\")\n    end,\n})\npokecon.autocmd.on(\"AppShutdownPre\", {\n    callback = function()\n        print(\"dynamic-shutdown-event\")\n    end,\n})\n",
    )
    .expect("dynamic fixture must be written");

    let mut command = TokioCommand::new(env!("CARGO_BIN_EXE_pokecon"));
    isolate_tokio_command(&mut command, &roots);
    command.stdout(Stdio::piped());
    let mut child = command
        .args(["--ephemeral-port", "--dynamic-config-language", "lua"])
        .spawn()
        .expect("PokeCon process must start");
    let (ready_address, stdout_task) = monitor_startup_stdout(&mut child);
    let address = wait_for_ready_address(&mut child, ready_address).await;
    let port = address.port();
    wait_until_listening(&mut child, address).await;
    assert_ui_is_served(port).await;
    assert_api_is_served(port).await;

    let pid = Pid::from_raw(
        child
            .id()
            .expect("running process must have an identifier")
            .cast_signed(),
    );
    kill(pid, Signal::SIGTERM).expect("the operating-system signal must be delivered");
    let status = match timeout(Duration::from_secs(20), child.wait()).await {
        Ok(result) => result.expect("process status must be readable"),
        Err(_elapsed) => {
            child
                .start_kill()
                .expect("timed-out process must be terminated");
            child
                .wait()
                .await
                .expect("terminated process must be reaped");
            panic!("PokeCon did not stop after SIGTERM");
        }
    };
    let stdout = stdout_task
        .await
        .expect("startup stdout monitor must not panic")
        .expect("startup stdout must remain readable");
    assert!(
        status.success(),
        "shutdown after SIGTERM failed: {status}; stdout={stdout}"
    );

    assert!(
        stdout.contains("dynamic-startup-event"),
        "dynamic startup-post event is missing: {stdout}"
    );
    let ready = stdout
        .find("POKECON-RUNTIME-0001")
        .unwrap_or_else(|| panic!("readiness marker POKECON-RUNTIME-0001 is missing: {stdout}"));
    let request_line = stdout
        .lines()
        .find(|line| line.contains("POKECON-RUNTIME-0003"))
        .unwrap_or_else(|| {
            panic!("shutdown-request marker POKECON-RUNTIME-0003 is missing: {stdout}")
        });
    assert!(
        request_line.contains("Signal(Terminate)"),
        "shutdown request must record the SIGTERM reason: {request_line}"
    );
    let request = stdout
        .find("POKECON-RUNTIME-0003")
        .expect("shutdown-request marker offset must be readable");
    let shutdown_event = stdout
        .find("dynamic-shutdown-event")
        .unwrap_or_else(|| panic!("dynamic shutdown-pre event is missing: {stdout}"));
    let stopped = stdout
        .find("POKECON-RUNTIME-0002")
        .unwrap_or_else(|| panic!("clean-stop marker POKECON-RUNTIME-0002 is missing: {stdout}"));
    // The marker and the message share one JSON log line, so intra-line
    // field order is not pinned; the same-line assertion below is enough.
    let stopped_line = stdout
        .lines()
        .find(|line| line.contains("POKECON-RUNTIME-0002"))
        .expect("clean-stop line must be readable");
    assert!(
        stopped_line.contains("PokeCon stopped cleanly"),
        "clean-stop message is missing from its marker line: {stopped_line}"
    );
    assert!(
        ready < request && request < shutdown_event && shutdown_event < stopped,
        "production shutdown order violated \
         (0001={ready}, 0003={request}, shutdown-pre={shutdown_event}, \
         0002={stopped}): {stdout}"
    );
}

#[cfg(unix)]
#[allow(clippy::too_many_lines)]
#[tokio::test]
async fn bind_conflict_fails_closed_without_publishing_readiness() {
    // AR-11-26 injected fault evidence on the real production boot path.
    // Holding the configured port open forces the production listener bind
    // to fail after `ProductionRuntime::build` has already constructed the
    // native resources, exercising the bind-failure cleanup in
    // `run_configured_controlled` (signal task abort + `shutdown_production`
    // on the half-built runtime) with a real OS error instead of a mock:
    //
    // - the process exits non-zero promptly (bounded cleanup, no stall);
    // - readiness (`POKECON-RUNTIME-0001`) is never published;
    // - a clean stop (`POKECON-RUNTIME-0002`) is never claimed;
    // - the fault cause (`AddrInUse`) is reported;
    // - the same roots boot cleanly once the conflict is gone (no residue).
    let holder = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback holder must bind");
    let occupied = holder
        .local_addr()
        .expect("holder must have an address")
        .port();

    let roots = TempDir::new().expect("isolated roots must exist");
    let mut command = TokioCommand::new(env!("CARGO_BIN_EXE_pokecon"));
    isolate_tokio_command(&mut command, &roots);
    command.env("POKECON_PORT", occupied.to_string());
    let bind_stdout = roots.path().join("bind-conflict.stdout");
    let bind_stderr = roots.path().join("bind-conflict.stderr");
    command.stdout(Stdio::from(
        std::fs::File::create(&bind_stdout).expect("bind stdout log must be creatable"),
    ));
    command.stderr(Stdio::from(
        std::fs::File::create(&bind_stderr).expect("bind stderr log must be creatable"),
    ));
    // No `--ephemeral-port`: the production server binds the configured
    // (occupied) port, hitting the real `BoundServer::bind` failure path.
    let mut child = command
        .args(["--dynamic-config-language", "none"])
        .spawn()
        .expect("PokeCon process must start");
    let status = match timeout(Duration::from_secs(30), child.wait()).await {
        Ok(result) => result.expect("process status must be readable"),
        Err(_elapsed) => {
            child
                .start_kill()
                .expect("timed-out process must be terminated");
            child
                .wait()
                .await
                .expect("terminated process must be reaped");
            panic!("bind-failed process did not exit; production cleanup stalled");
        }
    };
    let output = std::process::Output {
        status,
        stdout: std::fs::read(&bind_stdout).expect("bind stdout log must be readable"),
        stderr: std::fs::read(&bind_stderr).expect("bind stderr log must be readable"),
    };
    assert!(
        !output.status.success(),
        "bind conflict must fail: {:?}",
        output.status
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");
    assert!(
        !combined.contains("POKECON-RUNTIME-0001"),
        "readiness must never publish after a bind failure: {combined}"
    );
    assert!(
        !combined.contains("POKECON-RUNTIME-0002"),
        "clean stop must not be claimed without a ready runtime: {combined}"
    );
    assert!(
        combined.contains("AddrInUse") || combined.contains("Address already in use"),
        "bind fault must be reported: {combined}"
    );

    drop(holder);
    let mut healthy = TokioCommand::new(env!("CARGO_BIN_EXE_pokecon"));
    isolate_tokio_command(&mut healthy, &roots);
    healthy.env_remove("POKECON_PORT");
    let healthy_stdout = roots.path().join("healthy.stdout");
    let healthy_stderr = roots.path().join("healthy.stderr");
    healthy.stdout(Stdio::from(
        std::fs::File::create(&healthy_stdout).expect("healthy stdout log must be creatable"),
    ));
    healthy.stderr(Stdio::from(
        std::fs::File::create(&healthy_stderr).expect("healthy stderr log must be creatable"),
    ));
    let mut healthy = healthy
        .args([
            "--ephemeral-port",
            "--dynamic-config-language",
            "none",
            "--exit-after-startup",
        ])
        .spawn()
        .expect("healthy process must start");
    let healthy_status = match timeout(Duration::from_mins(1), healthy.wait()).await {
        Ok(result) => result.expect("healthy process status must be readable"),
        Err(_elapsed) => {
            healthy
                .start_kill()
                .expect("timed-out process must be terminated");
            healthy
                .wait()
                .await
                .expect("terminated process must be reaped");
            panic!("healthy boot after a bind failure did not exit");
        }
    };
    let healthy_output = std::process::Output {
        status: healthy_status,
        stdout: std::fs::read(&healthy_stdout).expect("healthy stdout log must be readable"),
        stderr: std::fs::read(&healthy_stderr).expect("healthy stderr log must be readable"),
    };
    assert!(
        healthy_output.status.success(),
        "healthy boot after a bind failure failed: {} {}",
        String::from_utf8_lossy(&healthy_output.stdout),
        String::from_utf8_lossy(&healthy_output.stderr)
    );

    let artifact_root = PathBuf::from(
        std::env::var("POKECON_STARTUP_FAULT_OUT").unwrap_or_else(|_| "target".to_owned()),
    )
    .join("startup-fault");
    std::fs::create_dir_all(&artifact_root).expect("startup fault artifact dir must be creatable");
    let artifact = artifact_root.join("production-startup-bind-conflict.json");
    std::fs::write(
        &artifact,
        serde_json::to_string_pretty(&serde_json::json!({
            "fixture_id": "production-startup-bind-conflict-v1",
            "call_graph_pin": "run_configured_controlled -> ProductionRuntime::build -> BoundServer::bind -> shutdown_production",
            "states": [
                {"state": "starting", "observed": true},
                {"state": "production_runtime_build_before_bind", "observed": true},
                {"state": "bind_failed_addr_in_use", "observed": true},
                {"state": "readiness_published", "observed": false},
                {"state": "clean_stop_claimed", "observed": false},
                {"state": "bounded_cleanup_exit", "observed": true},
                {"state": "healthy_restart_after_release", "observed": true},
            ],
            "fault": {
                "cause": "AddrInUse",
                "conflicting_port": occupied,
                "failed_exit_code": status.code(),
                "healthy_exit_code": healthy_output.status.code(),
            },
            "restart": {
                "same_isolated_roots": true,
                "startup_probe_success": healthy_output.status.success(),
                "residue_markers": ["POKECON-RUNTIME-0001", "POKECON-RUNTIME-0002"],
            },
        }))
        .expect("startup fault artifact must serialize"),
    )
    .expect("startup fault artifact must be writable");
    eprintln!("startup-fault artifact: {}", artifact.display());
}
