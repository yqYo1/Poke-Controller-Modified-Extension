use std::process::Command;

mod support;

use support::worker_binary;

#[test]
fn both_worker_roles_start_and_exit_cleanly() {
    for worker_kind in ["script", "dynamic"] {
        let output = Command::new(worker_binary())
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
