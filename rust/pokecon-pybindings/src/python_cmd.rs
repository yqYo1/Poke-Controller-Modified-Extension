use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;

use pokecon_core::notify::discord::DiscordNotifier;
use pokecon_core::notify::line::LineNotifier;
use pokecon_core::notify::{Notification, Notifier};
use pokecon_core::serial::keypress::KeyPress;
use pokecon_core::serial::keys::{Button, GamepadInput, Hat, parse_buttons};

// ---------------------------------------------------------------------------
// Global tokio runtime shared across all PythonCommand instances.
// ---------------------------------------------------------------------------
fn global_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| tokio::runtime::Runtime::new().expect("Failed to create tokio runtime"))
}

// ---------------------------------------------------------------------------
// PythonCommand
// ---------------------------------------------------------------------------

/// A Python command that holds named callbacks and provides
/// controller input / notification methods.
///
/// This is the Python-facing representation of a command.  User scripts register
/// callback functions (e.g. ``"do"``) that the Rust runtime can invoke
/// synchronously.
///
/// Python usage::
///
///     from pokecon.command import PythonCommand
///
///     def on_do(**kwargs):
///         print("do called with", kwargs)
///
///     cmd = PythonCommand("my_command")
///     cmd.register_callback("do", on_do)
///     result = cmd.trigger("do", {"arg": 42})
///
///     # Input methods
///     cmd.press("A", 0.1, 0.1)
///     cmd.hold("A|B", 0.5)
///     cmd.wait(1.0)
///
///     # Notifications
///     cmd.line_text("Hello from Rust!")
///     cmd.discord_text("Hello Discord!")
#[pyclass]
pub struct PythonCommand {
    name: String,
    callbacks: Mutex<HashMap<String, PyObject>>,
    alive: bool,
    keypress: Option<KeyPress>,
    discord: Option<DiscordNotifier>,
    line: Option<LineNotifier>,
}

#[pymethods]
impl PythonCommand {
    #[new]
    fn new(name: String) -> Self {
        Self {
            name,
            callbacks: Mutex::new(HashMap::new()),
            alive: true,
            keypress: None,
            discord: None,
            line: None,
        }
    }

    /// The name of this command.
    #[getter]
    fn name(&self) -> PyResult<String> {
        Ok(self.name.clone())
    }

    /// Register a Python callback for a named event (e.g. ``"do"``).
    fn register_callback(&self, event: String, callback: PyObject) -> PyResult<()> {
        let mut callbacks = self
            .callbacks
            .lock()
            .map_err(|e| PyRuntimeError::new_err(format!("Mutex poisoned: {}", e)))?;
        callbacks.insert(event, callback);
        Ok(())
    }

    /// Trigger the callback registered for *event*, passing *kwargs* as keyword
    /// arguments.  Returns the callback's return value, or ``None`` if no
    /// callback is registered.
    fn trigger<'py>(
        &self,
        py: Python<'py>,
        event: String,
        kwargs: Option<Bound<'py, PyDict>>,
    ) -> PyResult<Option<Bound<'py, PyAny>>> {
        // Clone the callback while holding the lock so we can release it
        // before calling into Python (avoids potential deadlocks).
        let cb = {
            let callbacks = self
                .callbacks
                .lock()
                .map_err(|e| PyRuntimeError::new_err(format!("Mutex poisoned: {}", e)))?;
            callbacks.get(&event).cloned()
        };

        match cb {
            Some(cb) => {
                let args = pyo3::types::PyTuple::empty(py);
                match kwargs {
                    Some(ref kwargs) => cb.bind(py).call(args, Some(kwargs)).map(Some),
                    None => cb.bind(py).call(args, None).map(Some),
                }
            }
            None => Ok(None),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Core input methods
    // ─────────────────────────────────────────────────────────────────────────

    /// Press (and release) a button or combination of buttons.
    ///
    /// * ``buttons`` — button name(s) separated by ``|``, ``+``, or ``,``
    ///   (e.g. ``"A"``, ``"A|B"``, ``"DPAD_UP"``).
    /// * ``duration`` — how long to hold the buttons (seconds, default 0.1).
    /// * ``wait`` — how long to wait after releasing (seconds, default 0.1).
    fn press(&mut self, buttons: String, duration: f64, wait: f64) -> PyResult<()> {
        self.check_alive()?;

        let inputs = parse_buttons(&buttons);
        if inputs.is_empty() {
            // If nothing to press, just wait
            Self::sleep_wait(duration);
            Self::sleep_wait(wait);
            return Ok(());
        }

        // Get or create keypress
        let rt = global_runtime();

        let kp = self.keypress.get_or_insert_with(|| {
            let sender = pokecon_core::serial::sender::Sender::new(false);
            KeyPress::new(sender)
        });

        // Press → wait → release → wait
        rt.block_on(kp.input(&inputs))
            .map_err(|e| PyRuntimeError::new_err(format!("Serial input failed: {}", e)))?;

        Self::sleep_wait(duration);

        rt.block_on(kp.input_end(&inputs))
            .map_err(|e| PyRuntimeError::new_err(format!("Serial input_end failed: {}", e)))?;

        Self::sleep_wait(wait);

        Ok(())
    }

    /// Hold down a button or combination of buttons.
    ///
    /// * ``buttons`` — button name(s) separated by ``|``, ``+``, or ``,``.
    /// * ``duration`` — how long to continue holding (seconds, default 0.1).
    fn hold(&mut self, buttons: String, duration: f64) -> PyResult<()> {
        self.check_alive()?;

        let inputs = parse_buttons(&buttons);
        if inputs.is_empty() {
            Self::sleep_wait(duration);
            return Ok(());
        }

        let rt = global_runtime();

        let kp = self.keypress.get_or_insert_with(|| {
            let sender = pokecon_core::serial::sender::Sender::new(false);
            KeyPress::new(sender)
        });

        rt.block_on(kp.hold(&inputs))
            .map_err(|e| PyRuntimeError::new_err(format!("Serial hold failed: {}", e)))?;

        Self::sleep_wait(duration);

        Ok(())
    }

    /// Release all currently held buttons.
    ///
    /// * ``duration`` — how long to wait after releasing (seconds, default 0.1).
    fn hold_end(&mut self, duration: f64) -> PyResult<()> {
        self.check_alive()?;

        if let Some(ref mut kp) = self.keypress {
            let rt = global_runtime();

            rt.block_on(kp.neutral())
                .map_err(|e| PyRuntimeError::new_err(format!("Serial hold_end failed: {}", e)))?;
        }

        Self::sleep_wait(duration);

        Ok(())
    }

    /// Wait (sleep) for the given duration in seconds.
    fn wait(&self, wait_time: f64) -> PyResult<()> {
        Self::sleep_wait(wait_time);
        Ok(())
    }

    /// Short wait of 0.1 seconds.
    fn short_wait(&self) -> PyResult<()> {
        Self::sleep_wait(0.1);
        Ok(())
    }

    /// Check whether the command is still alive.
    ///
    /// Returns ``True`` if alive, ``False`` if ``finish()`` has been called
    /// or the command has been stopped.
    fn check_if_alive(&self) -> bool {
        self.alive
    }

    /// Gracefully finish the command — sets the alive flag to ``False`` and
    /// releases any held buttons.
    fn finish(&mut self) -> PyResult<()> {
        self.alive = false;

        // Release held buttons
        if let Some(ref mut kp) = self.keypress {
            let rt = global_runtime();

            let _ = rt.block_on(kp.neutral());
            let _ = rt.block_on(kp.end());
        }

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Notification methods
    // ─────────────────────────────────────────────────────────────────────────

    /// Send a LINE notification.
    ///
    /// * ``text`` — the message text to send.
    /// * ``token`` — optional LINE channel access token.  If not provided, uses
    ///   the notifier created from ``set_line_token()``.
    fn line_text(&mut self, text: String, token: Option<String>) -> PyResult<()> {
        let rt = global_runtime();

        // If a token is provided, create a temporary notifier.
        // Otherwise use the stored one (if available).
        if let Some(token) = token {
            let notifier = LineNotifier::new(token);
            let notification = Notification::new(text);
            rt.block_on(notifier.send(&notification))
                .map_err(|e| PyRuntimeError::new_err(format!("LINE notification failed: {}", e)))?;
        } else if let Some(ref notifier) = self.line {
            let notification = Notification::new(text);
            rt.block_on(notifier.send(&notification))
                .map_err(|e| PyRuntimeError::new_err(format!("LINE notification failed: {}", e)))?;
        } else {
            // No token and no notifier configured — log and return Ok
            tracing::info!("LINE notification skipped (no notifier configured)");
        }

        Ok(())
    }

    /// Send a Discord notification via webhook.
    ///
    /// * ``text`` — the message content to send.
    /// * ``webhook_url`` — optional Discord webhook URL.  If not provided,
    ///   uses the notifier created from ``set_discord_webhook()``.
    fn discord_text(&mut self, text: String, webhook_url: Option<String>) -> PyResult<()> {
        let rt = global_runtime();

        if let Some(url) = webhook_url {
            let notifier = DiscordNotifier::new(url);
            let notification = Notification::new(text);
            rt.block_on(notifier.send(&notification)).map_err(|e| {
                PyRuntimeError::new_err(format!("Discord notification failed: {}", e))
            })?;
        } else if let Some(ref notifier) = self.discord {
            let notification = Notification::new(text);
            rt.block_on(notifier.send(&notification)).map_err(|e| {
                PyRuntimeError::new_err(format!("Discord notification failed: {}", e))
            })?;
        } else {
            tracing::info!("Discord notification skipped (no webhook configured)");
        }

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Configuration helpers
    // ─────────────────────────────────────────────────────────────────────────

    /// Set the LINE channel access token for future ``line_text()`` calls.
    fn set_line_token(&mut self, token: String) {
        self.line = Some(LineNotifier::new(token));
    }

    /// Set the Discord webhook URL for future ``discord_text()`` calls.
    fn set_discord_webhook(&mut self, webhook_url: String) {
        self.discord = Some(DiscordNotifier::new(webhook_url));
    }

    /// Open a serial connection.
    ///
    /// * ``port_num`` — COM port number (e.g. ``3`` for ``COM3``).
    /// * ``port_name`` — optional explicit port name (overrides port_num).
    /// * ``baudrate`` — baud rate (default 115200).
    fn open_serial(
        &mut self,
        port_num: u32,
        port_name: Option<String>,
        baudrate: Option<u32>,
    ) -> PyResult<bool> {
        let rt = global_runtime();

        let kp = self.keypress.get_or_insert_with(|| {
            let sender = pokecon_core::serial::sender::Sender::new(false);
            KeyPress::new(sender)
        });

        let baud = baudrate.unwrap_or(115200);
        let opened = rt
            .block_on(kp.sender_mut().open(port_num, port_name.as_deref(), baud))
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to open serial: {}", e)))?;

        Ok(opened)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal helpers
// ─────────────────────────────────────────────────────────────────────────────

impl PythonCommand {
    /// Check the alive flag; returns ``Err`` (with a clear message) if dead.
    fn check_alive(&self) -> PyResult<()> {
        if !self.alive {
            return Err(PyRuntimeError::new_err(
                "Command is no longer alive. Has finish() been called?",
            ));
        }
        Ok(())
    }

    /// Sleep for the given number of seconds (free function, no &self needed).
    fn sleep_wait(seconds: f64) {
        if seconds > 0.0 {
            std::thread::sleep(Duration::from_secs_f64(seconds));
        }
    }
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PythonCommand>()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pokecon_core::serial::keys::parse_buttons;

    #[test]
    fn test_new_python_command() {
        Python::with_gil(|py| {
            let cmd = Bound::new(py, PythonCommand::new("test_cmd".to_string())).unwrap();
            let name: String = cmd.getattr("name").unwrap().extract().unwrap();
            assert_eq!(name, "test_cmd");
            let alive: bool = cmd
                .call_method0("check_if_alive")
                .unwrap()
                .extract()
                .unwrap();
            assert!(alive);
        });
    }

    #[test]
    fn test_register_callback() {
        Python::with_gil(|py| {
            let cmd = Bound::new(py, PythonCommand::new("cmd".to_string())).unwrap();

            let callback = pyo3::types::PyCFunction::new_closure(
                py,
                None,
                None,
                |_args: &pyo3::Bound<'_, pyo3::types::PyTuple>,
                 _kwargs: Option<&pyo3::Bound<'_, pyo3::types::PyDict>>|
                 -> PyResult<i64> { Ok(42) },
            )
            .unwrap();

            cmd.call_method1("register_callback", ("do", callback))
                .unwrap();

            // Trigger with empty kwargs
            let result = cmd.call_method1("trigger", ("do", py.None())).unwrap();
            // Should return Some(42)
            assert!(!result.is_none());
        });
    }

    #[test]
    fn test_trigger_nonexistent() {
        Python::with_gil(|py| {
            let cmd = Bound::new(py, PythonCommand::new("cmd".to_string())).unwrap();
            let result = cmd.call_method1("trigger", ("nope", py.None())).unwrap();
            assert!(result.is_none());
        });
    }

    #[test]
    fn test_finish_marks_dead() {
        Python::with_gil(|py| {
            let cmd = Bound::new(py, PythonCommand::new("cmd".to_string())).unwrap();
            cmd.call_method0("finish").unwrap();
            let alive: bool = cmd
                .call_method0("check_if_alive")
                .unwrap()
                .extract()
                .unwrap();
            assert!(!alive);
        });
    }

    #[test]
    fn test_wait() {
        Python::with_gil(|py| {
            let cmd = Bound::new(py, PythonCommand::new("cmd".to_string())).unwrap();
            // Should not panic
            cmd.call_method1("wait", (0.01f64,)).unwrap();
        });
    }

    #[test]
    fn test_short_wait() {
        Python::with_gil(|py| {
            let cmd = Bound::new(py, PythonCommand::new("cmd".to_string())).unwrap();
            cmd.call_method0("short_wait").unwrap();
        });
    }

    #[test]
    fn test_parse_buttons_single() {
        let result = parse_buttons("A");
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0], GamepadInput::SingleButton(Button::A)));
    }

    #[test]
    fn test_parse_buttons_combo_pipe() {
        let result = parse_buttons("A|B|X");
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_parse_buttons_combo_plus() {
        let result = parse_buttons("A+B+X");
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_parse_buttons_combo_comma() {
        let result = parse_buttons("A, B, Y");
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_parse_buttons_dpad() {
        let result = parse_buttons("DPAD_UP");
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0], GamepadInput::SingleHat(Hat::TOP)));

        let result = parse_buttons("DPAD_DOWN");
        assert!(matches!(result[0], GamepadInput::SingleHat(Hat::BTM)));
    }

    #[test]
    fn test_parse_buttons_stick() {
        let result = parse_buttons("LSTICK_UP");
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0], GamepadInput::SingleDirection(_)));

        let result = parse_buttons("RSTICK_LEFT");
        assert!(matches!(result[0], GamepadInput::SingleDirection(_)));
    }

    #[test]
    fn test_parse_buttons_stick_shorthand() {
        let result = parse_buttons("L_UP");
        assert_eq!(result.len(), 1);

        let result = parse_buttons("R_RIGHT");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_parse_buttons_aliases() {
        let result = parse_buttons("TOP");
        assert!(matches!(result[0], GamepadInput::SingleHat(Hat::TOP)));

        let result = parse_buttons("SELECT");
        assert!(matches!(
            result[0],
            GamepadInput::SingleButton(Button::SELECT)
        ));

        let result = parse_buttons("START");
        assert!(matches!(
            result[0],
            GamepadInput::SingleButton(Button::START)
        ));
    }

    #[test]
    fn test_parse_buttons_empty() {
        let result = parse_buttons("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_buttons_unknown_ignored() {
        let result = parse_buttons("A+UNKNOWN+B");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_sleep_wait_zero() {
        PythonCommand::sleep_wait(0.0);
        // Should not panic or sleep
    }

    #[test]
    fn test_sleep_wait_positive() {
        let start = std::time::Instant::now();
        PythonCommand::sleep_wait(0.01);
        let elapsed = start.elapsed();
        assert!(elapsed >= std::time::Duration::from_millis(9));
    }

    #[test]
    fn test_config_setters() {
        Python::with_gil(|py| {
            let cmd = Bound::new(py, PythonCommand::new("cmd".to_string())).unwrap();
            cmd.call_method1("set_line_token", ("test-token",)).unwrap();
            cmd.call_method1("set_discord_webhook", ("https://example.com/webhook",))
                .unwrap();
            // No panic = success
        });
    }
}
