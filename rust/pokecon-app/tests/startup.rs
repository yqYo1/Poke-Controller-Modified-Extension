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
            .args(["--ui", ui_mode, "--port", &port, "--exit-after-startup"])
            .status()
            .expect("PokeCon process must start");
        assert!(status.success(), "{ui_mode} startup probe failed");
    }
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
            .args(["--port", &port_argument])
            .spawn()
            .expect("PokeCon process must start");
        wait_until_listening(&mut child, port).await;

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
    for (name, value) in isolated_environment(roots) {
        command.env(name, value);
    }
    command.env("LOCALAPPDATA", roots.path().join("localappdata"));
}

#[cfg(unix)]
fn isolate_tokio_command(command: &mut TokioCommand, roots: &TempDir) {
    for (name, value) in isolated_environment(roots) {
        command.env(name, value);
    }
    command.env("LOCALAPPDATA", roots.path().join("localappdata"));
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
