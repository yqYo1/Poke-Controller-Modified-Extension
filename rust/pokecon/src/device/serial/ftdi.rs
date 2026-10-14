//! FTDI MPSSE device availability for the DS FT formats.
//!
//! The DS FT formats drive FTDI chips in MPSSE bit-bang mode; the payload
//! encoding itself lives in [`super::codec`]. This module detects whether a
//! matching device is attached so the integration layer can offer the
//! format only while the hardware is present, mirroring the upstream gate
//! (upstream `open_ftdi_port` fails without a device and the UI hides the
//! format). The I/O backend builds on the same libftdi1 bindings.
//!
//! Detection never opens the device: it asks libftdi to enumerate FTDI
//! vendor devices per MPSSE-capable product ID (upstream device types 6/7/8:
//! FT2232H, FT4232H, FT232H). Filtering happens inside libftdi, so the
//! crate's default feature set (no raw libusb access) is sufficient.
//!
//! The availability surface is consumed by the integration layer together
//! with the I/O backend in the next step of this workstream; until then the
//! items are compiled and tested but unreferenced by production code, so the
//! module opts out of the dead-code lint as a whole.
#![allow(dead_code)]

use std::ffi::CString;
use std::time::{Duration, Instant};

use libftdi1_sys as ftdi;
use thiserror::Error;

/// FTDI vendor ID shared by all FTDI chips.
pub const FTDI_VENDOR_ID: u16 = 0x0403;

/// Product IDs of the MPSSE-capable FTDI chips the upstream accepts
/// (device types 6/7/8: FT2232H, FT4232H, FT232H).
pub const MPSSE_PRODUCT_IDS: [u16; 3] = [0x6010, 0x6011, 0x6014];

/// A detected MPSSE-capable FTDI device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtdiDevice {
    /// Product ID of the chip (one of [`MPSSE_PRODUCT_IDS`]).
    pub product_id: u16,
    /// Device serial string when the chip exposes one.
    pub serial: Option<String>,
    /// Product description string when the chip exposes one.
    pub description: Option<String>,
}

/// Whether a product ID belongs to an MPSSE-capable chip.
pub fn is_mpsse_product_id(product_id: u16) -> bool {
    MPSSE_PRODUCT_IDS.contains(&product_id)
}

/// Enumerate attached MPSSE-capable FTDI devices without opening them.
///
/// libftdi filters by vendor/product during enumeration, so no device is
/// claimed or opened. String reads are best-effort: a chip without strings
/// or with unreadable strings still yields a device entry.
#[allow(unsafe_code)]
pub fn enumerate_mpsse_devices() -> Vec<FtdiDevice> {
    let mut devices = Vec::new();
    // SAFETY: the context is allocated and freed by libftdi1 itself; every
    // device list handed back by the library is released before the next
    // call, and all buffers passed to the string helper are stack arrays
    // with the exact lengths given to the callee.
    unsafe {
        let ctx = ftdi::ftdi_new();
        if ctx.is_null() {
            return devices;
        }
        for product_id in MPSSE_PRODUCT_IDS {
            let mut list: *mut ftdi::ftdi_device_list = std::ptr::null_mut();
            let found = ftdi::ftdi_usb_find_all(
                ctx,
                std::ptr::addr_of_mut!(list),
                i32::from(FTDI_VENDOR_ID),
                i32::from(product_id),
            );
            if found > 0 {
                let mut node = list;
                while !node.is_null() {
                    let mut serial_buf = [0i8; 64];
                    let mut desc_buf = [0i8; 64];
                    let rc = ftdi::ftdi_usb_get_strings(
                        ctx,
                        (*node).dev,
                        std::ptr::null_mut(),
                        0,
                        desc_buf.as_mut_ptr(),
                        64,
                        serial_buf.as_mut_ptr(),
                        64,
                    );
                    let serial = string_from_ffi(rc, &serial_buf);
                    let description = string_from_ffi(rc, &desc_buf);
                    devices.push(FtdiDevice {
                        product_id,
                        serial,
                        description,
                    });
                    node = (*node).next;
                }
            }
            if !list.is_null() {
                ftdi::ftdi_list_free(std::ptr::addr_of_mut!(list));
            }
        }
        ftdi::ftdi_free(ctx);
    }
    devices
}

/// Interpret a libftdi return code plus buffer as an optional string.
///
/// # Safety
///
/// `buf` must be the same buffer whose pointer and length were passed to the
/// libftdi call that produced `rc`.
#[allow(unsafe_code)]
unsafe fn string_from_ffi(rc: i32, buf: &[i8]) -> Option<String> {
    if rc < 0 {
        return None;
    }
    c_buffer_to_string(buf)
}

/// Convert a NUL-terminated C buffer to an owned string when non-empty.
fn c_buffer_to_string(buf: &[i8]) -> Option<String> {
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|byte| byte.cast_unsigned())
        .collect();
    if bytes.is_empty() {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// Bit mode value used before switching to MPSSE (upstream `BITMODE_RESET`).
const BITMODE_RESET: u8 = 0x00;
/// MPSSE bit-bang bit mode (upstream `BITMODE_MPSSE`).
const BITMODE_MPSSE: u8 = 0x02;
/// Time to let the mode switch settle before purging (upstream 0.05 s).
const MODE_SETTLE: Duration = Duration::from_millis(50);
/// The invalid command and error reply used to confirm MPSSE is answering.
const BAD_COMMAND: u8 = 0xAA;
const BAD_COMMAND_RESPONSE: [u8; 2] = [0xFA, 0xAA];
/// The sync reply deadline (upstream 0.5 s).
const SYNC_TIMEOUT: Duration = Duration::from_millis(500);
/// Transfer chunk size, the libftdi equivalent of the upstream USB parameters.
const TRANSFER_CHUNK: u32 = 65536;
/// Read/write timeout in milliseconds (upstream `setTimeouts(1000, 1000)`).
const USB_TIMEOUT_MS: i32 = 1000;
/// Input latency in milliseconds (upstream `setLatencyTimer(1)`).
const LATENCY_TIMER_MS: u8 = 1;
/// Poll interval while waiting for the MPSSE sync reply.
const SYNC_POLL_INTERVAL: Duration = Duration::from_millis(5);

/// A selector for one enumerated device: its serial number or its index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FtdiSelector {
    /// Match the device whose serial string equals this value.
    Serial(String),
    /// Address the n-th MPSSE-capable device in enumeration order.
    Index(u32),
}

impl FtdiSelector {
    /// All-digit selectors address the enumeration index; anything else is a
    /// serial number (boards may lack a programmed serial, so both forms
    /// exist; the upstream matched on the Windows COM port number instead).
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        if !raw.is_empty()
            && raw.bytes().all(|byte| byte.is_ascii_digit())
            && let Ok(index) = raw.parse::<u32>()
        {
            return Self::Index(index);
        }
        Self::Serial(raw.to_owned())
    }
}

/// Errors from the FTDI transport, mirroring the upstream `FtdiError`
/// messages shown to the user.
#[derive(Debug, Error)]
pub enum FtdiError {
    /// No MPSSE-capable device matched the selector.
    #[error("no MPSSE-capable FTDI device matched the selector: {0}")]
    DeviceNotFound(String),
    /// A libftdi call failed with the given negative return code.
    #[error("failed to {operation} the FTDI device (libftdi error {code})")]
    Driver {
        /// Short description of the failed operation.
        operation: &'static str,
        /// The negative return code libftdi produced.
        code: i32,
    },
    /// The MPSSE mode did not answer the sync probe in time.
    #[error("no MPSSE mode response (response: {0})")]
    MpsseNoResponse(String),
    /// The device was used after it was closed.
    #[error("the FTDI device is not open")]
    NotOpen,
}

/// One opened FTDI device driven in MPSSE mode (upstream `FtdiPort`).
///
/// The libftdi context pointer is not `Send`; the owning I/O adapter keeps
/// the port on one dedicated thread and communicates through channels.
pub struct FtdiPort {
    context: *mut ftdi::ftdi_context,
    closed: bool,
}

impl FtdiPort {
    /// Open the device addressed by the selector and configure MPSSE mode.
    ///
    /// The sequence mirrors the upstream `FtdiPort.setup`: reset, transfer
    /// parameters, timeouts, latency, reset bit mode, MPSSE bit mode, settle,
    /// purge, then the caller-provided released frame and the sync check.
    ///
    /// # Errors
    ///
    /// Fails when no device matches or any libftdi call reports an error.
    #[allow(unsafe_code)]
    pub fn open(selector: &FtdiSelector, released_frame: &[u8]) -> Result<Self, FtdiError> {
        // SAFETY: the context is allocated by libftdi and freed in `close`
        // (or `Drop`); every call receives the live pointer and validated
        // arguments.
        unsafe {
            let context = ftdi::ftdi_new();
            if context.is_null() {
                return Err(FtdiError::Driver {
                    operation: "allocate a context for",
                    code: -1,
                });
            }
            let (serial, index) = match selector {
                FtdiSelector::Serial(serial) => (Some(serial.as_str()), 0),
                FtdiSelector::Index(index) => (None, *index),
            };
            let serial_c = serial.and_then(|value| CString::new(value).ok());
            // Try each MPSSE-capable product ID in order, mirroring the
            // upstream scan over MPSSE device types.
            let mut opened = -1;
            for product_id in MPSSE_PRODUCT_IDS {
                opened = ftdi::ftdi_usb_open_desc_index(
                    context,
                    i32::from(FTDI_VENDOR_ID),
                    i32::from(product_id),
                    std::ptr::null(),
                    serial_c
                        .as_ref()
                        .map_or(std::ptr::null(), |value| value.as_ptr()),
                    index,
                );
                if opened >= 0 {
                    break;
                }
            }
            if opened < 0 {
                ftdi::ftdi_free(context);
                return Err(FtdiError::DeviceNotFound(selector.describe()));
            }
            let mut port = Self {
                context,
                closed: false,
            };
            port.configure(released_frame)?;
            Ok(port)
        }
    }

    /// Run the mode-switch and sync sequence on an opened context.
    #[allow(unsafe_code)]
    fn configure(&mut self, released_frame: &[u8]) -> Result<(), FtdiError> {
        // SAFETY: `self.context` is live until `close`/`Drop`.
        unsafe {
            check_ftdi(ftdi::ftdi_usb_reset(self.context), "reset")?;
            check_ftdi(
                ftdi::ftdi_read_data_set_chunksize(self.context, TRANSFER_CHUNK),
                "configure",
            )?;
            check_ftdi(
                ftdi::ftdi_write_data_set_chunksize(self.context, TRANSFER_CHUNK),
                "configure",
            )?;
            (*self.context).usb_read_timeout = USB_TIMEOUT_MS;
            (*self.context).usb_write_timeout = USB_TIMEOUT_MS;
            check_ftdi(
                ftdi::ftdi_set_latency_timer(self.context, LATENCY_TIMER_MS),
                "configure",
            )?;
            check_ftdi(
                ftdi::ftdi_set_bitmode(self.context, 0x00, BITMODE_RESET),
                "configure",
            )?;
            check_ftdi(
                ftdi::ftdi_set_bitmode(self.context, 0x00, BITMODE_MPSSE),
                "configure",
            )?;
        }
        std::thread::sleep(MODE_SETTLE);
        // SAFETY: same live context.
        unsafe {
            check_ftdi(ftdi::ftdi_usb_purge_buffers(self.context), "configure")?;
        }
        self.write(released_frame)?;
        self.check_mpsse()
    }

    /// Confirm that MPSSE answers by sending an invalid command and waiting
    /// for the error reply (upstream `_check_mpsse`, 0.5 s deadline).
    ///
    /// # Errors
    ///
    /// Fails when the reply does not arrive before the deadline.
    #[allow(unsafe_code)]
    pub fn check_mpsse(&mut self) -> Result<(), FtdiError> {
        self.write(&[BAD_COMMAND])?;
        let deadline = Instant::now() + SYNC_TIMEOUT;
        let mut response: Vec<u8> = Vec::new();
        while Instant::now() < deadline {
            let mut buffer = [0u8; 64];
            // SAFETY: live context; the buffer is a stack array with its
            // exact length passed to the callee.
            let read = unsafe { ftdi::ftdi_read_data(self.context, buffer.as_mut_ptr(), 64) };
            if read > 0 {
                let length = usize::try_from(read).unwrap_or(0);
                response.extend_from_slice(&buffer[..length]);
                if response
                    .windows(BAD_COMMAND_RESPONSE.len())
                    .any(|window| window == BAD_COMMAND_RESPONSE)
                {
                    return Ok(());
                }
            } else {
                std::thread::sleep(SYNC_POLL_INTERVAL);
            }
        }
        Err(FtdiError::MpsseNoResponse(hex_list(&response)))
    }

    /// Send raw bytes to the device (upstream `FtdiPort.write`).
    ///
    /// # Errors
    ///
    /// Fails when the port is closed or libftdi reports a write error.
    #[allow(unsafe_code)]
    pub fn write(&mut self, data: &[u8]) -> Result<usize, FtdiError> {
        if self.closed {
            return Err(FtdiError::NotOpen);
        }
        // SAFETY: live context; the slice pointer and length are consistent.
        let Ok(length) = i32::try_from(data.len()) else {
            return Err(FtdiError::Driver {
                operation: "send data to",
                code: -1,
            });
        };
        let written = unsafe { ftdi::ftdi_write_data(self.context, data.as_ptr(), length) };
        if written < 0 {
            return Err(FtdiError::Driver {
                operation: "send data to",
                code: written,
            });
        }
        Ok(usize::try_from(written).unwrap_or(0))
    }

    /// Release every pin and close the device (upstream `FtdiPort.close`).
    ///
    /// The bit mode intentionally stays MPSSE so the pins remain inputs; a
    /// failed release or close is logged by the caller, not raised.
    #[allow(unsafe_code)]
    pub fn close(&mut self, released_frame: &[u8]) -> Vec<FtdiError> {
        let mut failures = Vec::new();
        if !self.closed {
            if let Err(error) = self.write(released_frame) {
                failures.push(error);
            }
            // SAFETY: live context; freed exactly once here.
            unsafe {
                check_ftdi(ftdi::ftdi_usb_close(self.context), "close")
                    .err()
                    .into_iter()
                    .for_each(|error| failures.push(error));
                ftdi::ftdi_free(self.context);
            }
            self.context = std::ptr::null_mut();
            self.closed = true;
        }
        failures
    }

    /// Whether the port is still open.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        !self.closed
    }
}

impl Drop for FtdiPort {
    fn drop(&mut self) {
        if !self.closed {
            self.close(&[]);
        }
    }
}

impl FtdiSelector {
    /// A human-readable form used in error messages.
    #[must_use]
    fn describe(&self) -> String {
        match self {
            Self::Serial(serial) => format!("serial {serial}"),
            Self::Index(index) => format!("index {index}"),
        }
    }
}

/// Turn a negative libftdi return code into a typed error.
fn check_ftdi(code: i32, operation: &'static str) -> Result<(), FtdiError> {
    if code < 0 {
        return Err(FtdiError::Driver { operation, code });
    }
    Ok(())
}

/// Format bytes as space-separated lowercase hex for diagnostics.
fn hex_list(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "none".to_owned();
    }
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mpsse_product_ids_are_recognised() {
        assert!(is_mpsse_product_id(0x6010)); // FT2232H
        assert!(is_mpsse_product_id(0x6011)); // FT4232H
        assert!(is_mpsse_product_id(0x6014)); // FT232H
    }

    #[test]
    fn non_mpsse_product_ids_are_rejected() {
        assert!(!is_mpsse_product_id(0x6001)); // FT232R (UART mode)
        assert!(!is_mpsse_product_id(0x6015)); // FT230X (UART mode)
        assert!(!is_mpsse_product_id(0x0000));
    }

    #[test]
    fn empty_c_buffers_yield_no_string() {
        let buf = [0i8; 8];
        assert_eq!(c_buffer_to_string(&buf), None);
    }

    #[test]
    fn nul_terminated_c_buffers_yield_strings() {
        let mut buf = [0i8; 8];
        for (index, byte) in b"FT2232H".iter().enumerate() {
            buf[index] = byte.cast_signed();
        }
        assert_eq!(c_buffer_to_string(&buf).as_deref(), Some("FT2232H"));
    }

    #[test]
    #[allow(unsafe_code)]
    fn failed_string_reads_yield_none() {
        let buf = [0i8; 8];
        // SAFETY: the buffer is the same array that would be passed to the
        // libftdi call; here the call is simulated by a negative return code.
        let value = unsafe { string_from_ffi(-1, &buf) };
        assert_eq!(value, None);
    }

    #[test]
    fn digit_selectors_address_the_enumeration_index() {
        assert_eq!(FtdiSelector::parse("0"), FtdiSelector::Index(0));
        assert_eq!(FtdiSelector::parse("12"), FtdiSelector::Index(12));
    }

    #[test]
    fn non_digit_selectors_are_serial_numbers() {
        assert_eq!(
            FtdiSelector::parse("A50285BI"),
            FtdiSelector::Serial("A50285BI".to_owned())
        );
        assert_eq!(FtdiSelector::parse(""), FtdiSelector::Serial(String::new()));
        assert_eq!(
            FtdiSelector::parse("12A"),
            FtdiSelector::Serial("12A".to_owned())
        );
    }

    #[test]
    fn oversized_numeric_selectors_fall_back_to_serial_matching() {
        assert_eq!(
            FtdiSelector::parse("99999999999999999999"),
            FtdiSelector::Serial("99999999999999999999".to_owned())
        );
    }

    #[test]
    fn hex_lists_format_empty_and_non_empty_payloads() {
        assert_eq!(hex_list(&[]), "none");
        assert_eq!(hex_list(&[0xFA, 0xAA]), "fa aa");
        assert_eq!(hex_list(&[0x00, 0x0F]), "00 0f");
    }

    #[test]
    fn driver_errors_render_the_operation_and_code() {
        let error = FtdiError::Driver {
            operation: "reset",
            code: -5,
        };
        assert_eq!(
            error.to_string(),
            "failed to reset the FTDI device (libftdi error -5)"
        );
    }

    #[test]
    fn sync_failure_reports_the_observed_response() {
        let error = FtdiError::MpsseNoResponse("none".to_owned());
        assert_eq!(error.to_string(), "no MPSSE mode response (response: none)");
    }
}
