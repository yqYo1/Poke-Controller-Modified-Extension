use std::process::Command;

#[test]
fn both_worker_roles_start_and_exit_cleanly() {
    for worker_kind in ["script", "dynamic"] {
        let status = Command::new(env!("CARGO_BIN_EXE_pokecon-worker"))
            .args(["--kind", worker_kind, "--exit-after-startup"])
            .status()
            .expect("worker process must start");
        assert!(
            status.success(),
            "{worker_kind} worker startup probe failed"
        );
    }
}
