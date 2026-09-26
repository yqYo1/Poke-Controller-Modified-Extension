use std::io::{self, Read, Write};
use std::time::Duration;

const MAX_PAYLOAD_BYTES: u32 = 1_048_576;

fn main() -> io::Result<()> {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "eof".to_owned());
    match mode.as_str() {
        "crash" => {
            eprintln!("fault fixture: deliberate worker crash");
            std::process::exit(71);
        }
        "partial-frame" => {
            let mut stdout = io::stdout().lock();
            stdout.write_all(&10_u32.to_be_bytes())?;
            stdout.write_all(&[0x81, 0xa4])?;
            stdout.flush()?;
            std::process::exit(72);
        }
        "oversize" => {
            let mut stdout = io::stdout().lock();
            stdout.write_all(&(MAX_PAYLOAD_BYTES + 1).to_be_bytes())?;
            stdout.flush()?;
            std::process::exit(73);
        }
        "non-string-map" => {
            let body: &[u8] = &[
                0x83, 0xa4, b'k', b'i', b'n', b'd', 0xa5, b'e', b'v', b'e', b'n', b't', 0xa2, b'o',
                b'p', 0xac, b'w', b'o', b'r', b'k', b'e', b'r', b'.', b'r', b'e', b'a', b'd', b'y',
                0xa7, b'p', b'a', b'y', b'l', b'o', b'a', b'd', 0x81, 0x01, 0xc0,
            ];
            let mut stdout = io::stdout().lock();
            let frame_len = u32::try_from(body.len()).expect("fixture payload length fits u32");
            stdout.write_all(&frame_len.to_be_bytes())?;
            stdout.write_all(body)?;
            stdout.flush()?;
            std::process::exit(75);
        }
        "ignore-shutdown" => loop {
            std::thread::sleep(Duration::from_mins(1));
        },
        "ack-then-hang" => {
            if let Err(error) = acknowledge_shutdown_then_hang() {
                eprintln!("fault fixture (ack-then-hang): {error}");
                std::process::exit(76);
            }
            loop {
                std::thread::sleep(Duration::from_mins(1));
            }
        }
        "eof" => Ok(()),
        unknown => {
            eprintln!("unknown fault fixture mode: {unknown}");
            std::process::exit(74);
        }
    }
}

/// Reads one `worker.shutdown` request frame from stdin, acknowledges it with
/// a hand-built `Envelope::Response` frame on stdout, then never exits.
fn acknowledge_shutdown_then_hang() -> io::Result<()> {
    let mut stdin = io::stdin().lock();
    let mut prefix = [0_u8; 4];
    stdin.read_exact(&mut prefix)?;
    let body_length = u32::from_be_bytes(prefix);
    if body_length == 0 || body_length > MAX_PAYLOAD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unexpected shutdown frame length {body_length}"),
        ));
    }
    let mut body = vec![0_u8; body_length as usize];
    stdin.read_exact(&mut body)?;
    drop(stdin);
    let id = extract_request_id(&body).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "shutdown request carries no id")
    })?;

    // map3 {kind: "response", id: <copied uint>, payload: nil}; `op` is
    // omitted, matching `Envelope::Response { op: None }` under
    // `rmp_serde::to_vec_named`.
    let mut response = Vec::with_capacity(27 + id.len());
    response.extend_from_slice(&[0x83, 0xa4, b'k', b'i', b'n', b'd']);
    response.extend_from_slice(&[0xa8, b'r', b'e', b's', b'p', b'o', b'n', b's', b'e']);
    response.extend_from_slice(&[0xa2, b'i', b'd']);
    response.extend_from_slice(id);
    response.extend_from_slice(&[0xa7, b'p', b'a', b'y', b'l', b'o', b'a', b'd', 0xc0]);

    let frame_length = u32::try_from(response.len())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut stdout = io::stdout().lock();
    stdout.write_all(&frame_length.to_be_bytes())?;
    stdout.write_all(&response)?;
    stdout.flush()?;
    Ok(())
}

/// Copies the raw `MessagePack` bytes of the `id` uint out of a request map body.
fn extract_request_id(body: &[u8]) -> Option<&[u8]> {
    let key = [0xa2, b'i', b'd'];
    let start = body.windows(3).position(|window| window == key)? + 3;
    let end = match *body.get(start)? {
        byte if byte & 0x80 == 0 => start + 1,
        0xcc => start + 2,
        0xcd => start + 3,
        0xce => start + 5,
        0xcf => start + 9,
        _ => return None,
    };
    body.get(start..end)
}
