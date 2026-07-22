#[cfg(unix)]
use std::net::SocketAddrV4;
use std::net::{Ipv4Addr, TcpListener};
use std::process::Command;
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
use nix::sys::signal::{Signal, kill};
#[cfg(unix)]
use nix::unistd::Pid;
use tempfile::TempDir;
#[cfg(unix)]
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
#[cfg(unix)]
use tokio::net::TcpStream;
#[cfg(unix)]
use tokio::process::Command as TokioCommand;
#[cfg(unix)]
use tokio::time::{Instant, sleep, timeout};

#[test]
fn rust_main_starts_and_exits_cleanly_in_both_modes() {
    let roots = TempDir::new().expect("isolated roots must exist");
    for ui_mode in ["web", "desktop"] {
        let port = unused_local_port().to_string();
        let mut command = Command::new(env!("CARGO_BIN_EXE_pokecon"));
        isolate_std_command(&mut command, &roots);
        let status = command
            .args([
                "--ui",
                ui_mode,
                "--port",
                &port,
                "--dynamic-config-language",
                "none",
                "--exit-after-startup",
            ])
            .status()
            .expect("PokeCon process must start");
        assert!(status.success(), "{ui_mode} startup probe failed");
    }
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

    let port = unused_local_port().to_string();
    let mut command = Command::new(env!("CARGO_BIN_EXE_pokecon"));
    isolate_std_command(&mut command, &roots);
    let output = command
        .args([
            "--port",
            &port,
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
async fn operating_system_signals_use_the_clean_shutdown_path() {
    let roots = TempDir::new().expect("isolated roots must exist");
    for signal in [Signal::SIGINT, Signal::SIGTERM] {
        let port = unused_local_port();
        let port_argument = port.to_string();
        let mut command = TokioCommand::new(env!("CARGO_BIN_EXE_pokecon"));
        isolate_tokio_command(&mut command, &roots);
        let mut child = command
            .args([
                "--port",
                &port_argument,
                "--dynamic-config-language",
                "none",
            ])
            .spawn()
            .expect("PokeCon process must start");
        wait_until_listening(&mut child, port).await;
        assert_ui_is_served(port).await;

        kill(
            Pid::from_raw(
                child
                    .id()
                    .expect("running process must have an identifier")
                    .cast_signed(),
            ),
            signal,
        )
        .expect("the operating-system signal must be delivered");
        let status = timeout(Duration::from_secs(10), child.wait())
            .await
            .unwrap_or_else(|_| {
                child
                    .start_kill()
                    .expect("timed-out process must be terminated");
                panic!("PokeCon did not stop after {signal:?}");
            })
            .expect("process status must be readable");
        assert!(
            status.success(),
            "shutdown after {signal:?} failed: {status}"
        );
    }
}

fn unused_local_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("an ephemeral loopback port must be available")
        .local_addr()
        .expect("the loopback listener must have an address")
        .port()
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
async fn wait_until_listening(child: &mut tokio::process::Child, port: u16) {
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
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
