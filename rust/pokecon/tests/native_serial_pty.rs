#![cfg(unix)]

use std::fs::File;
use std::io::{Read as _, Write as _};
use std::time::Duration;

use nix::pty::{OpenptyResult, openpty};
use nix::unistd::ttyname;
use pokecon::integration_test_support::device::serial::{
    NativeSerialBackend, PortSelector, SerialBackend as _,
};
use tokio::time::timeout;

const TEST_TIMEOUT: Duration = Duration::from_secs(2);

/// Counts open descriptors whose target is the PTY slave path.
fn count_slave_fds(slave_path: &str) -> usize {
    std::fs::read_dir("/proc/self/fd")
        .expect("fd inventory must be readable")
        .flatten()
        .filter(|entry| {
            std::fs::read_link(entry.path())
                .is_ok_and(|target| target.to_string_lossy() == slave_path)
        })
        .count()
}

#[tokio::test]
async fn native_serial_backend_exchanges_bytes_with_a_real_pty() {
    let OpenptyResult { master, slave } = openpty(None, None).expect("PTY creation must succeed");
    let slave_path = ttyname(&slave).expect("PTY slave path must be discoverable");
    let selector = PortSelector::new(slave_path.to_string_lossy().into_owned())
        .expect("PTY slave path must be a valid serial selector");
    let serial = NativeSerialBackend
        .open(&selector, 115_200)
        .await
        .expect("native serial backend must open the PTY slave");
    let mut peer = File::from(master);

    let outbound = b"pokecon-native-serial-outbound";
    let mut written = 0;
    while written < outbound.len() {
        let count = timeout(TEST_TIMEOUT, serial.write(&outbound[written..]))
            .await
            .expect("native serial write must not time out")
            .expect("native serial write must succeed");
        assert!(count > 0, "native serial write must make progress");
        written += count;
    }
    let mut observed = vec![0; outbound.len()];
    peer.read_exact(&mut observed)
        .expect("PTY master must receive native serial output");
    assert_eq!(observed, outbound);

    let inbound = b"pokecon-native-serial-inbound";
    peer.write_all(inbound)
        .expect("PTY master must write the inbound fixture");
    let mut received = vec![0; inbound.len()];
    let mut read = 0;
    while read < inbound.len() {
        let count = timeout(TEST_TIMEOUT, serial.read(&mut received[read..]))
            .await
            .expect("native serial read must not time out")
            .expect("native serial read must succeed");
        assert!(count > 0, "native serial read must make progress");
        read += count;
    }
    assert_eq!(received.as_slice(), inbound);

    serial
        .close()
        .await
        .expect("native serial close must succeed");
    drop(slave);
}

#[tokio::test]
async fn native_serial_close_releases_port_while_clones_survive() {
    let OpenptyResult { master, slave } = openpty(None, None).expect("PTY creation must succeed");
    let slave_path = ttyname(&slave).expect("PTY slave path must be discoverable");
    let slave_path = slave_path.to_string_lossy().into_owned();
    let selector = PortSelector::new(slave_path.clone())
        .expect("PTY slave path must be a valid serial selector");
    let baseline = count_slave_fds(&slave_path);

    let serial = NativeSerialBackend
        .open(&selector, 115_200)
        .await
        .expect("native serial backend must open the PTY slave");
    assert_eq!(
        count_slave_fds(&slave_path),
        baseline + 1,
        "opening the native port must hold exactly one new slave descriptor"
    );
    // Retain a second owner like the manager's replaced connection and its
    // receive monitor so the test proves the descriptor is released by
    // `close` itself rather than by dropping every `Arc` clone.
    let retained = serial.clone();
    let _peer = File::from(master);

    let pending_read = tokio::spawn({
        let io = serial.clone();
        async move {
            let mut byte = [0u8; 1];
            io.read(&mut byte).await
        }
    });

    serial
        .close()
        .await
        .expect("native serial close must succeed");
    serial
        .close()
        .await
        .expect("native serial close must stay idempotent");

    let settled = timeout(TEST_TIMEOUT, pending_read)
        .await
        .expect("pending native serial read must settle after close")
        .expect("pending read task must not panic");
    assert!(
        settled.is_err(),
        "pending native serial read must fail after close"
    );

    let mut byte = [0u8; 1];
    let read_after_close = timeout(TEST_TIMEOUT, serial.read(&mut byte)).await;
    assert!(
        matches!(read_after_close, Ok(Err(_))),
        "reads after close must fail fast instead of hanging"
    );
    let write_after_close = timeout(TEST_TIMEOUT, serial.write(&[0])).await;
    assert!(
        matches!(write_after_close, Ok(Err(_))),
        "writes after close must fail fast instead of hanging"
    );

    assert_eq!(
        count_slave_fds(&slave_path),
        baseline,
        "close must release the slave descriptor while clones are still alive"
    );

    let replacement = NativeSerialBackend
        .open(&selector, 115_200)
        .await
        .expect("replacement open must succeed after close releases the exclusive port");
    replacement
        .close()
        .await
        .expect("replacement close must succeed");

    drop(retained);
    drop(serial);
    drop(slave);
}
