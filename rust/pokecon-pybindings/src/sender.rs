use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use std::sync::OnceLock;

use pokecon_core::serial::sender::Sender as RustSender;

// ---------------------------------------------------------------------------
// Global tokio runtime shared across all Sender instances.
// ---------------------------------------------------------------------------
fn global_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| tokio::runtime::Runtime::new().expect("Failed to create tokio runtime"))
}

// ---------------------------------------------------------------------------
// PythonSender — wraps the Rust Sender for use from Python.
// ---------------------------------------------------------------------------

/// A serial port sender for communicating with a Switch / Arduino.
///
/// Provides open / close / send / write operations on a serial port.
///
/// Python usage::
///
///     from pokecon.sender import Sender
///
///     s = Sender()
///     s.open(port_num=0, baudrate=115200)
///     s.send(b"\\x00\\x01\\x02")
///     s.write_row("0x000004 8")
///     s.close()
#[pyclass(name = "Sender")]
pub struct PythonSender {
    inner: RustSender,
}

#[pymethods]
impl PythonSender {
    /// Create a new Sender instance.
    ///
    /// * ``is_show_serial`` — if True, print sent data to stdout (default False).
    #[new]
    #[pyo3(signature = (is_show_serial = None))]
    fn new(is_show_serial: Option<bool>) -> Self {
        Self {
            inner: RustSender::new(is_show_serial.unwrap_or(false)),
        }
    }

    /// Open a serial port connection.
    ///
    /// * ``port_num`` — COM port number (e.g. ``0`` for ``/dev/ttyUSB0`` or ``COM1``).
    /// * ``port_name`` — optional explicit port path (overrides auto-detection).
    /// * ``baudrate`` — baud rate (default 115200).
    ///
    /// Returns ``True`` if the port was opened successfully.
    fn open(
        &mut self,
        port_num: u32,
        port_name: Option<String>,
        baudrate: Option<u32>,
    ) -> PyResult<bool> {
        let rt = global_runtime();
        let baud = baudrate.unwrap_or(115200);
        rt.block_on(self.inner.open(port_num, port_name.as_deref(), baud))
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to open serial: {e}")))
    }

    /// Close the serial port connection.
    fn close(&mut self) -> PyResult<()> {
        let rt = global_runtime();
        rt.block_on(self.inner.close());
        Ok(())
    }

    /// Check whether the serial port is currently open.
    fn is_opened(&self) -> bool {
        self.inner.is_opened()
    }

    /// Send raw bytes to the serial port.
    ///
    /// * ``data`` — bytes to send.
    fn send(&mut self, data: Vec<u8>) -> PyResult<()> {
        let rt = global_runtime();
        rt.block_on(self.inner.write_list(&data, false))
            .map_err(|e| PyRuntimeError::new_err(format!("Serial send failed: {e}")))
    }

    /// Write a row (text command) to the serial port.
    ///
    /// * ``row`` — the text row to send (e.g. ``"0x000004 8"``).
    /// * ``is_show`` — if True, print the sent row to stdout (default False).
    #[pyo3(signature = (row, is_show = false))]
    fn write_row(&mut self, row: &str, is_show: bool) -> PyResult<()> {
        let rt = global_runtime();
        rt.block_on(self.inner.write_row(row, is_show))
            .map_err(|e| PyRuntimeError::new_err(format!("Serial write_row failed: {e}")))
    }

    /// Set the baudrate on the open serial port.
    ///
    /// * ``baudrate`` — new baud rate.
    fn set_baudrate(&mut self, baudrate: u32) -> PyResult<()> {
        let rt = global_runtime();
        rt.block_on(self.inner.set_baudrate(baudrate))
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to set baudrate: {e}")))
    }

    /// Set the data format name (e.g. ``"Default"``, ``"Qingpi"``).
    fn set_data_format(&mut self, format: &str) {
        self.inner.set_data_format(format);
    }

    /// Get the current data format name.
    fn get_data_format(&self) -> String {
        self.inner.get_data_format().to_string()
    }

    /// Send the ``"end"`` command to finalise a frame sequence.
    fn send_end(&mut self) -> PyResult<()> {
        let rt = global_runtime();
        rt.block_on(self.inner.send_end())
            .map_err(|e| PyRuntimeError::new_err(format!("Serial send_end failed: {e}")))
    }

    /// Build a port path from a port number (static helper).
    ///
    /// * ``port_num`` — port number
    /// * ``port_name`` — optional explicit name
    #[staticmethod]
    fn build_port_path(port_num: u32, port_name: Option<String>) -> PyResult<String> {
        RustSender::build_port_path(port_num, port_name.as_deref())
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to build port path: {e}")))
    }

    fn __repr__(&self) -> String {
        if self.inner.is_opened() {
            format!("<Sender opened format='{}'>", self.inner.get_data_format())
        } else {
            "<Sender closed>".to_string()
        }
    }
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PythonSender>()?;
    Ok(())
}
