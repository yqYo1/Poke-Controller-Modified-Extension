#![cfg(unix)]

use std::fs::File;
use std::io::{Read as _, Write as _};
use std::time::Duration;

use nix::pty::{OpenptyResult, openpty};
use nix::unistd::ttyname;
use pokecon_device::serial::{NativeSerialBackend, PortSelector, SerialBackend as _};
use tokio::time::timeout;

const TEST_TIMEOUT: Duration = Duration::from_secs(2);

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
