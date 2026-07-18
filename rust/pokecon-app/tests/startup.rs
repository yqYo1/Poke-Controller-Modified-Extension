#[cfg(unix)]
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
use std::process::Command;
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
use nix::sys::signal::{Signal, kill};
#[cfg(unix)]
use nix::unistd::Pid;
#[cfg(unix)]
use tokio::net::TcpStream;
#[cfg(unix)]
use tokio::process::Command as TokioCommand;
#[cfg(unix)]
use tokio::time::{Instant, sleep, timeout};

#[test]
fn rust_main_starts_and_exits_cleanly_in_both_modes() {
    for ui_mode in ["web", "desktop"] {
        let status = Command::new(env!("CARGO_BIN_EXE_pokecon"))
            .args(["--ui", ui_mode, "--port", "0", "--exit-after-startup"])
            .status()
            .expect("PokeCon process must start");
        assert!(status.success(), "{ui_mode} startup probe failed");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn operating_system_signals_use_the_clean_shutdown_path() {
    for signal in [Signal::SIGINT, Signal::SIGTERM] {
        let port = unused_local_port();
        let port_argument = port.to_string();
        let mut child = TokioCommand::new(env!("CARGO_BIN_EXE_pokecon"))
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

#[cfg(unix)]
fn unused_local_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("an ephemeral loopback port must be available")
        .local_addr()
        .expect("the loopback listener must have an address")
        .port()
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
