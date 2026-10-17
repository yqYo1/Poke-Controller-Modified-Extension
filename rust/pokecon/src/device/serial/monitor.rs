//! Serial send monitor: accumulates the data sent to the device.
//!
//! Mirrors the upstream `serial_monitor` service: senders append entries from
//! any thread and return immediately, the display side drains the pending
//! entries in one batch, and the oldest entries are dropped while nobody
//! drains (bounded at [`DEFAULT_MAX_LINES`] lines). The display line is
//! `[HH:MM:SS.mmm][format name] value`: strings print as-is, bytes as
//! space-separated hexadecimal.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Default bound on pending lines (upstream `DEFAULT_MAX_LINES`).
pub const DEFAULT_MAX_LINES: usize = 10_000;

/// One pending send entry.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    stamp_millis: u128,
    format_name: String,
    value: String,
}

/// Accumulates sent data until the display side drains it.
#[derive(Debug)]
pub struct SerialMonitor {
    max_lines: usize,
    entries: Mutex<VecDeque<Entry>>,
}

impl SerialMonitor {
    /// Builds a monitor bounded to `max_lines` pending entries.
    #[must_use]
    pub fn new(max_lines: usize) -> Self {
        Self {
            max_lines,
            entries: Mutex::new(VecDeque::new()),
        }
    }

    /// Records one send from any thread; the timestamp is taken here.
    ///
    /// `value` is rendered at drain time so the sending thread stays fast;
    /// non-text values are stringified before storing.
    pub fn add_bytes(&self, format_name: &str, value: &[u8]) {
        self.add_entry(format_name, hex_bytes(value));
    }

    /// Records one text send from any thread.
    pub fn add_text(&self, format_name: &str, value: &str) {
        self.add_entry(format_name, value.to_owned());
    }

    fn add_entry(&self, format_name: &str, value: String) {
        let stamp_millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis());
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        while entries.len() >= self.max_lines {
            entries.pop_front();
        }
        entries.push_back(Entry {
            stamp_millis,
            format_name: format_name.to_owned(),
            value,
        });
    }

    /// Drains the pending entries into display lines (one per entry).
    ///
    /// Entries appended while draining stay for the next call.
    #[must_use]
    pub fn drain(&self) -> String {
        let Ok(mut entries) = self.entries.lock() else {
            return String::new();
        };
        let mut lines = String::new();
        while let Some(entry) = entries.pop_front() {
            lines.push_str(&format_line(
                entry.stamp_millis,
                &entry.format_name,
                &entry.value,
            ));
        }
        lines
    }

    /// Drops every pending entry.
    pub fn clear(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.clear();
        }
    }
}

impl Default for SerialMonitor {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_LINES)
    }
}

/// Renders bytes as space-separated lowercase hexadecimal.
fn hex_bytes(value: &[u8]) -> String {
    value
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Builds one display line: `[HH:MM:SS.mmm][format name] value\n`.
///
/// The clock uses local time; milliseconds are derived before splitting so a
/// rounding carry cannot disagree with the seconds.
#[must_use]
pub fn format_line(stamp_millis: u128, format_name: &str, value: &str) -> String {
    let seconds = i64::try_from(stamp_millis / 1000).unwrap_or(0);
    let milliseconds = stamp_millis % 1000;
    let clock = chrono::DateTime::from_timestamp(seconds, 0).map_or_else(
        || "??:??:??".to_owned(),
        |utc| {
            utc.with_timezone(&chrono::Local)
                .format("%H:%M:%S")
                .to_string()
        },
    );
    format!("[{clock}.{milliseconds:03}][{format_name}] {value}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_render_as_space_separated_hex() {
        assert_eq!(hex_bytes(&[0x82, 0xFF, 0x00]), "82 ff 00");
        assert_eq!(hex_bytes(&[]), "");
    }

    #[test]
    fn lines_follow_the_upstream_shape() {
        let line = format_line(0, "Default", "0x8013 1 0 ff 80 80");
        assert!(line.ends_with("[Default] 0x8013 1 0 ff 80 80\n"), "{line}");
        assert!(line.starts_with('['), "{line}");
        assert_eq!(line.matches('[').count(), 2);
        assert!(line.contains('.'), "{line}");
    }

    #[test]
    fn millisecond_carry_keeps_seconds_consistent() {
        // 999.6 ms rounds to 1000 ms: the second and millisecond parts must
        // not disagree (upstream rounds before splitting).
        let line = format_line(1_999, "NX", "ab");
        assert!(line.contains("01.999]"), "{line}");
    }

    #[test]
    fn drain_returns_pending_lines_in_order() {
        let monitor = SerialMonitor::new(DEFAULT_MAX_LINES);
        monitor.add_bytes("Default", &[0x82, 0xEF]);
        monitor.add_text("NX", "text");
        let drained = monitor.drain();
        let lines: Vec<&str> = drained.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("[Default] 82 ef"), "{drained}");
        assert!(lines[1].contains("[NX] text"), "{drained}");
        assert_eq!(monitor.drain(), "");
    }

    #[test]
    fn oldest_entries_drop_when_the_display_stalls() {
        let monitor = SerialMonitor::new(3);
        for index in 0..5 {
            monitor.add_text("Default", &index.to_string());
        }
        let drained = monitor.drain();
        let lines: Vec<&str> = drained.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("] 2"), "{drained}");
        assert!(lines[2].contains("] 4"), "{drained}");
    }

    #[test]
    fn clear_drops_pending_entries() {
        let monitor = SerialMonitor::new(DEFAULT_MAX_LINES);
        monitor.add_text("Default", "value");
        monitor.clear();
        assert_eq!(monitor.drain(), "");
    }
}
