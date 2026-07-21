use std::process::Command;

#[test]
fn both_worker_roles_start_and_exit_cleanly() {
    for worker_kind in ["script", "dynamic"] {
        let output = Command::new(env!("CARGO_BIN_EXE_pokecon-worker"))
            .args(["--kind", worker_kind, "--exit-after-startup"])
            .env("RUST_LOG", "info")
            .output()
            .expect("worker process must start");
        assert!(
            output.status.success(),
            "{worker_kind} worker startup probe failed"
        );
        assert!(
            output.stdout.is_empty(),
            "worker stdout is reserved exclusively for IPC frames"
        );
        assert!(
            !output.stderr.is_empty(),
            "worker diagnostics must use the out-of-band stderr pipe"
        );
    }
}
