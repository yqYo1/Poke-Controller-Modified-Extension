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

use libftdi1_sys as ftdi;

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
}
