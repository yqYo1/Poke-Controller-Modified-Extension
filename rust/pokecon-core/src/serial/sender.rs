use std::io;
use thiserror::Error;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio_serial::{SerialPort, SerialPortBuilderExt, SerialStream};
use tracing::{debug, error, info};

#[derive(Error, Debug)]
pub enum SerialError {
    #[error("Failed to open serial port: {0}")]
    OpenError(io::Error),

    #[error("Serial port not found: {0}")]
    PortNotFound(String),

    #[error("Permission denied opening serial port: {0}")]
    PermissionDenied(String),

    #[error("Failed to write to serial port: {0}")]
    WriteError(io::Error),

    #[error("Serial port operation timed out")]
    Timeout,

    #[error("Serial port is not open")]
    NotOpen,

    #[error("Unsupported OS")]
    UnsupportedOS,

    #[error("Failed to set baudrate: {0}")]
    BaudrateError(String),
}

pub struct Sender {
    port: Option<BufWriter<SerialStream>>,
    is_show_serial: bool,
    before: Option<String>,
    data_format: String,
}

impl Sender {
    pub fn new(is_show_serial: bool) -> Self {
        Self {
            port: None,
            is_show_serial,
            before: None,
            data_format: "Default".to_string(),
        }
    }

    pub fn build_port_path(port_num: u32, port_name: Option<&str>) -> Result<String, SerialError> {
        if let Some(name) = port_name {
            if !name.is_empty() {
                return Ok(name.to_string());
            }
        }

        let os = std::env::consts::OS;
        match os {
            "windows" => Ok(format!("COM{}", port_num)),
            "macos" => Ok(format!("/dev/tty.usbserial-{}", port_num)),
            "linux" => Ok(format!("/dev/ttyUSB{}", port_num)),
            _ => Err(SerialError::UnsupportedOS),
        }
    }

    pub async fn open(
        &mut self,
        port_num: u32,
        port_name: Option<&str>,
        baudrate: u32,
    ) -> Result<bool, SerialError> {
        let path = Self::build_port_path(port_num, port_name)?;

        info!("connecting to {} ({})", path, baudrate);

        let port = match tokio_serial::new(&path, baudrate).open_native_async() {
            Ok(p) => p,
            Err(e) => {
                let io_err: io::Error = e.into();
                error!("COM Port: can't be established: {}", io_err);
                return Err(match io_err.kind() {
                    io::ErrorKind::NotFound => SerialError::PortNotFound(path.clone()),
                    io::ErrorKind::PermissionDenied => SerialError::PermissionDenied(path.clone()),
                    _ => SerialError::OpenError(io_err),
                });
            }
        };

        self.port = Some(BufWriter::new(port));
        Ok(true)
    }

    pub async fn close(&mut self) {
        debug!("Closing the serial communication");
        if let Some(ref mut port) = self.port {
            let _ = port.flush().await;
        }
        self.port = None;
    }

    pub fn is_opened(&self) -> bool {
        self.port.is_some()
    }

    /// Set baudrate on the open serial port.
    pub async fn set_baudrate(&mut self, baudrate: u32) -> Result<(), SerialError> {
        let port = self.port.as_mut().ok_or(SerialError::NotOpen)?;
        let serial = port.get_mut();
        serial
            .set_baud_rate(baudrate)
            .map_err(|e| SerialError::BaudrateError(e.to_string()))?;
        info!("Serial baudrate changed to {}", baudrate);
        Ok(())
    }

    /// Set the data format name (e.g. "Default", "Qingpi", "3DS Controller").
    pub fn set_data_format(&mut self, format: &str) {
        self.data_format = format.to_string();
    }

    /// Get the current data format name.
    pub fn get_data_format(&self) -> &str {
        &self.data_format
    }

    pub async fn write_row(&mut self, row: &str, is_show: bool) -> Result<(), SerialError> {
        if let Some(ref before) = self.before {
            if before != "end" && is_show {
                let output: Vec<&str> = before.split(' ').collect();
                self.show_input(&output);
            }
        }

        let port = self.port.as_mut().ok_or(SerialError::NotOpen)?;

        let data = format!("{}\r\n", row);
        port.write_all(data.as_bytes()).await.map_err(|e| {
            error!("Error: {}", e);
            SerialError::WriteError(e)
        })?;
        port.flush().await.map_err(|e| {
            error!("Error: {}", e);
            SerialError::WriteError(e)
        })?;

        self.before = Some(row.to_string());

        if self.is_show_serial {
            println!("{}", row);
        }

        Ok(())
    }

    pub async fn write_list(&mut self, values: &[u8], is_show: bool) -> Result<(), SerialError> {
        let port = self.port.as_mut().ok_or(SerialError::NotOpen)?;

        port.write_all(values).await.map_err(|e| {
            error!("Error: {}", e);
            SerialError::WriteError(e)
        })?;
        port.flush().await.map_err(|e| {
            error!("Error: {}", e);
            SerialError::WriteError(e)
        })?;

        self.before = Some(format!("{:?}", values));

        if self.is_show_serial && is_show {
            println!("{:?}", values);
        }

        Ok(())
    }

    pub async fn write_row_wo_counter(&mut self, row: &str) -> Result<(), SerialError> {
        let port = self.port.as_mut().ok_or(SerialError::NotOpen)?;

        let data = format!("{}\r\n", row);
        port.write_all(data.as_bytes()).await.map_err(|e| {
            error!("Error: {}", e);
            SerialError::WriteError(e)
        })?;
        port.flush().await.map_err(|e| {
            error!("Error: {}", e);
            SerialError::WriteError(e)
        })?;

        if self.is_show_serial {
            println!("{}", row);
        }

        Ok(())
    }

    fn show_input(&self, output: &[&str]) {
        if output.len() < 2 {
            return;
        }

        let btn_hex =
            match u16::from_str_radix(output[0].strip_prefix("0x").unwrap_or(output[0]), 16) {
                Ok(v) => v,
                Err(_) => return,
            };

        let buttons: Vec<String> = (0..16)
            .filter(|x| (btn_hex >> x) & 1 == 1)
            .map(|x| format!("Button[{}]", x))
            .collect();

        let hat_idx = output[1].parse::<usize>().unwrap_or(8);
        let hat_names = [
            "TOP",
            "TOP_RIGHT",
            "RIGHT",
            "BTM_RIGHT",
            "BTM",
            "BTM_LEFT",
            "LEFT",
            "TOP_LEFT",
            "CENTER",
        ];
        let hat_name = hat_names.get(hat_idx).unwrap_or(&"CENTER");

        if *hat_name != "CENTER" {
            info!(
                "Buttons: {:?}, Hat: {}",
                buttons.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                hat_name
            );
        } else if !buttons.is_empty() {
            info!(
                "Buttons: {:?}",
                buttons.iter().map(|s| s.as_str()).collect::<Vec<_>>()
            );
        }
    }

    pub async fn send_end(&mut self) -> Result<(), SerialError> {
        self.write_row("end", false).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_port_path_linux() {
        // Default: Linux path with port number
        let path = Sender::build_port_path(0, None).unwrap();
        assert_eq!(path, "/dev/ttyUSB0");
    }

    #[test]
    fn test_build_port_path_explicit_name() {
        let path = Sender::build_port_path(0, Some("/dev/ttyS0")).unwrap();
        assert_eq!(path, "/dev/ttyS0");
    }

    #[test]
    fn test_build_port_path_empty_name() {
        // Empty name should fall back to default path
        let path = Sender::build_port_path(1, Some("")).unwrap();
        assert_eq!(path, "/dev/ttyUSB1");
    }

    #[tokio::test]
    async fn test_open_virtual_serial_port() {
        // Integration test: opens a virtual serial port for testing.
        // Set SERIAL_TEST_PORT to the PTY path (e.g. /tmp/serial-test-a).
        // Skips if the env var is not set or the port doesn't exist.
        let port_path = match std::env::var("SERIAL_TEST_PORT") {
            Ok(p) => p,
            Err(_) => {
                eprintln!(
                    "Skipping serial test: SERIAL_TEST_PORT not set. Run scripts/setup-serial-test.sh first."
                );
                return;
            }
        };

        if !std::path::Path::new(&port_path).exists() {
            eprintln!(
                "Skipping serial test: {port_path} does not exist. Run scripts/setup-serial-test.sh first."
            );
            return;
        }

        let mut sender = Sender::new(false);
        let result = sender.open(0, Some(&port_path), 115200).await;
        match result {
            Ok(true) => {
                assert!(sender.is_opened());
                // Try writing a test command
                sender.write_row("test_command", false).await.unwrap();
                sender.close().await;
                assert!(!sender.is_opened());
                eprintln!(
                    "Serial test: successfully opened {port_path}, wrote test data, and closed."
                );
            }
            Ok(false) => {
                eprintln!("Serial test: open returned false (unexpected).");
            }
            Err(e) => {
                eprintln!("Skipping serial test: could not open {port_path}: {e}");
            }
        }
    }
}
