use std::io::{self, Write};
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
        "eof" => Ok(()),
        unknown => {
            eprintln!("unknown fault fixture mode: {unknown}");
            std::process::exit(74);
        }
    }
}
