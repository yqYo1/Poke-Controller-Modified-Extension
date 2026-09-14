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
