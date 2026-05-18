//! Lua API bindings for PokeCon functions.
//!
//! This module defines the `pokecon` global table exposed to Lua scripts,
//! providing input operations, logging, event handling, and notifications.
//!
//! # Event bridging
//!
//! Lua callbacks registered via `pokecon.on()` cannot be passed directly to
//! `EventBus` because `mlua::Function` is `!Send + !Sync` (it holds a weak
//! reference to the Lua state).  Instead, callbacks are stored in a Lua-side
//! registry table.  The caller must bridge events from the Rust side into Lua
//! explicitly (e.g. via [`LuaRuntime::dispatch_event`] or a polling loop).

use mlua::{Function as LuaFunction, Lua, Result as LuaResult, Table, Value};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::events::{Event, EventBus};
use crate::serial::keypress::KeyPress;
use crate::serial::keys::{Button, GamepadInput, Hat};

/// Global tokio runtime used for async serial/camera operations inside
/// synchronous Lua callbacks.
fn global_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RUNTIME.get_or_init(|| tokio::runtime::Runtime::new().expect("Failed to create tokio runtime"))
}

/// Parse a button name string (e.g. "A", "A|B", "DPAD_UP", "LSTICK_UP")
/// into a `Vec<GamepadInput>`, matching the PythonCommand semantics.
fn parse_buttons(buttons: &str) -> Vec<GamepadInput> {
    let parts: Vec<&str> = buttons
        .split(['|', '+', ','])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    let mut result = Vec::new();
    for part in parts {
        match part.to_uppercase().as_str() {
            "A" => result.push(GamepadInput::SingleButton(Button::A)),
            "B" => result.push(GamepadInput::SingleButton(Button::B)),
            "X" => result.push(GamepadInput::SingleButton(Button::X)),
            "Y" => result.push(GamepadInput::SingleButton(Button::Y)),
            "L" => result.push(GamepadInput::SingleButton(Button::L)),
            "R" => result.push(GamepadInput::SingleButton(Button::R)),
            "ZL" => result.push(GamepadInput::SingleButton(Button::ZL)),
            "ZR" => result.push(GamepadInput::SingleButton(Button::ZR)),
            "MINUS" | "-" => result.push(GamepadInput::SingleButton(Button::MINUS)),
            "PLUS" | "+" => result.push(GamepadInput::SingleButton(Button::PLUS)),
            "LCLICK" | "L3" => result.push(GamepadInput::SingleButton(Button::LCLICK)),
            "RCLICK" | "R3" => result.push(GamepadInput::SingleButton(Button::RCLICK)),
            "HOME" => result.push(GamepadInput::SingleButton(Button::HOME)),
            "CAPTURE" => result.push(GamepadInput::SingleButton(Button::CAPTURE)),
            "SELECT" => result.push(GamepadInput::SingleButton(Button::SELECT)),
            "START" => result.push(GamepadInput::SingleButton(Button::START)),
            "DPAD_UP" | "TOP" => result.push(GamepadInput::SingleHat(Hat::TOP)),
            "DPAD_DOWN" | "BTM" => result.push(GamepadInput::SingleHat(Hat::BTM)),
            "DPAD_LEFT" | "LEFT" => result.push(GamepadInput::SingleHat(Hat::LEFT)),
            "DPAD_RIGHT" | "RIGHT" => result.push(GamepadInput::SingleHat(Hat::RIGHT)),
            "DPAD_TOP_RIGHT" | "TOP_RIGHT" => result.push(GamepadInput::SingleHat(Hat::TOP_RIGHT)),
            "DPAD_BTM_RIGHT" | "BTM_RIGHT" => result.push(GamepadInput::SingleHat(Hat::BTM_RIGHT)),
            "DPAD_BTM_LEFT" | "BTM_LEFT" => result.push(GamepadInput::SingleHat(Hat::BTM_LEFT)),
            "DPAD_TOP_LEFT" | "TOP_LEFT" => result.push(GamepadInput::SingleHat(Hat::TOP_LEFT)),
            _ => {
                // Unknown button name — warn but don't fail
                tracing::warn!("Unknown button name in Lua API: {}", part);
            }
        }
    }
    result
}

/// Handle for bridging Lua callbacks into the Rust event system.
///
/// The handle stores the shared `EventBus` reference (if any) and a
/// **Lua callback registry** so that `pokecon.on()` from Lua can be
/// tracked without violating `Send + Sync` constraints.
///
/// It also holds optional references to [`KeyPress`] (for serial input)
/// and [`Camera`] (for image capture) so that Lua API functions can
/// perform real hardware operations when configured.
#[derive(Clone)]
pub struct ApiHandle {
    /// Shared event bus for dispatching events from Lua.
    pub event_bus: Option<EventBus>,
    /// Registry of Lua callbacks, keyed by event name.
    /// Stored behind `Arc<Mutex<…>>` so it can be shared across threads;
    /// the actual `mlua::Function` values are only accessed from the
    /// Lua runtime's own thread.
    pub lua_callbacks: Arc<Mutex<HashMap<String, Vec<usize>>>>,
    /// Optional KeyPress instance for serial controller input.
    pub keypress: Option<Arc<tokio::sync::Mutex<KeyPress>>>,
    /// Optional Camera instance for image capture and template matching.
    #[cfg(feature = "v4l")]
    pub camera: Option<Arc<crate::cv::Camera>>,
    // NOTE: we cannot store `mlua::Function` here because it is !Send.
    // Instead, Lua callbacks live inside the Lua registry (by reference)
    // and are identified by an integer key.
}

impl ApiHandle {
    /// Create a new ApiHandle with no event bus.
    pub fn new() -> Self {
        Self {
            event_bus: None,
            lua_callbacks: Arc::new(Mutex::new(HashMap::new())),
            keypress: None,
            #[cfg(feature = "v4l")]
            camera: None,
        }
    }

    /// Create a new ApiHandle with an event bus.
    pub fn with_event_bus(event_bus: EventBus) -> Self {
        Self {
            event_bus: Some(event_bus),
            lua_callbacks: Arc::new(Mutex::new(HashMap::new())),
            keypress: None,
            #[cfg(feature = "v4l")]
            camera: None,
        }
    }

    /// Attach a KeyPress instance for serial controller input.
    pub fn with_keypress(mut self, keypress: KeyPress) -> Self {
        self.keypress = Some(Arc::new(tokio::sync::Mutex::new(keypress)));
        self
    }

    /// Attach a Camera instance for image capture.
    #[cfg(feature = "v4l")]
    pub fn with_camera(mut self, camera: crate::cv::Camera) -> Self {
        self.camera = Some(Arc::new(camera));
        self
    }

    /// Set the KeyPress instance after construction.
    pub fn set_keypress(&mut self, keypress: KeyPress) {
        self.keypress = Some(Arc::new(tokio::sync::Mutex::new(keypress)));
    }

    /// Set the Camera instance after construction.
    #[cfg(feature = "v4l")]
    pub fn set_camera(&mut self, camera: crate::cv::Camera) {
        self.camera = Some(Arc::new(camera));
    }

    /// Store a Lua function reference in the Lua registry and return its
    /// integer key.  The caller must own the Lua lock.
    pub fn store_callback(lua: &Lua, func: &LuaFunction) -> usize {
        if let Ok(reg) = lua.named_registry_value::<Table>("pokecon_callbacks") {
            let next_key: usize = reg.get("__next_key").unwrap_or(1usize);
            reg.set("__next_key", next_key + 1).ok();
            reg.set(next_key, func.clone()).ok();
            next_key
        } else {
            0
        }
    }
}

impl Default for ApiHandle {
    fn default() -> Self {
        Self::new()
    }
}

/// PokeCon Lua API registration.
///
/// Registers the `pokecon` global table with all bindings into the given Lua state.
pub struct PokeConApi;

impl PokeConApi {
    /// Register all PokeCon API functions into the Lua state.
    ///
    /// If `handle` is provided, event-related functions (`on`, `emit`, etc.)
    /// will be wired to the shared `EventBus`, and hardware functions
    /// (`press`, `hold`, `capture`, etc.) will use the optional KeyPress
    /// and Camera references from the handle.
    #[allow(clippy::too_many_lines)]
    pub fn register_with_handle(lua: &Lua, handle: Option<Arc<ApiHandle>>) -> LuaResult<()> {
        let api = lua.create_table()?;

        // ── Initialise callback registry in Lua ──────────────────
        let cb_reg = lua.create_table()?;
        cb_reg.set("__next_key", 1usize)?;
        lua.set_named_registry_value("pokecon_callbacks", cb_reg)?;

        // ── Input operations ──────────────────────────────────────

        // wait(ms): Sleep for the given number of milliseconds.
        api.set(
            "wait",
            lua.create_function(|_, ms: u64| {
                std::thread::sleep(Duration::from_millis(ms));
                Ok(())
            })?,
        )?;

        // press_button(button, duration_ms): Legacy single-button press.
        api.set(
            "press_button",
            lua.create_function(|_, (button, duration): (String, u64)| {
                println!("[PokeCon] Press {} for {}ms", button, duration);
                Ok(())
            })?,
        )?;

        // press(button_str): Press and release a button or combination.
        // button_str can be "A", "A|B", "DPAD_UP", etc.
        // Returns immediately; the press/release sequence is synchronous.
        let handle_press = handle.clone();
        api.set(
            "press",
            lua.create_function(move |_, button_str: String| {
                let inputs = parse_buttons(&button_str);
                if inputs.is_empty() {
                    return Err(mlua::Error::runtime(format!(
                        "No valid buttons in '{}'",
                        button_str
                    )));
                }
                if let Some(ref h) = handle_press {
                    if let Some(ref kp_arc) = h.keypress {
                        let rt = global_runtime();
                        let mut kp = rt.block_on(kp_arc.lock());
                        match rt.block_on(kp.input(&inputs)) {
                            Ok(()) => {
                                std::thread::sleep(Duration::from_millis(50));
                                let _ = rt.block_on(kp.input_end(&inputs));
                            }
                            Err(e) => {
                                return Err(mlua::Error::runtime(format!(
                                    "Serial input failed: {}",
                                    e
                                )));
                            }
                        }
                        return Ok(());
                    }
                }
                // Fallback: log the action
                println!(
                    "[PokeCon Lua] Press '{}' (serial not connected, logged only)",
                    button_str
                );
                Ok(())
            })?,
        )?;

        // release(button_str): Release a specific button or combination.
        // This is the counterpart to hold().
        let handle_release = handle.clone();
        api.set(
            "release",
            lua.create_function(move |_, button_str: String| {
                let inputs = parse_buttons(&button_str);
                if inputs.is_empty() {
                    return Err(mlua::Error::runtime(format!(
                        "No valid buttons in '{}'",
                        button_str
                    )));
                }
                if let Some(ref h) = handle_release {
                    if let Some(ref kp_arc) = h.keypress {
                        let rt = global_runtime();
                        let mut kp = rt.block_on(kp_arc.lock());
                        match rt.block_on(kp.input_end(&inputs)) {
                            Ok(()) => return Ok(()),
                            Err(e) => {
                                return Err(mlua::Error::runtime(format!(
                                    "Serial release failed: {}",
                                    e
                                )));
                            }
                        }
                    }
                }
                println!(
                    "[PokeCon Lua] Release '{}' (serial not connected, logged only)",
                    button_str
                );
                Ok(())
            })?,
        )?;

        // hold(button_str): Hold down a button or combination.
        let handle_hold = handle.clone();
        api.set(
            "hold",
            lua.create_function(move |_, button_str: String| {
                let inputs = parse_buttons(&button_str);
                if inputs.is_empty() {
                    return Err(mlua::Error::runtime(format!(
                        "No valid buttons in '{}'",
                        button_str
                    )));
                }
                if let Some(ref h) = handle_hold {
                    if let Some(ref kp_arc) = h.keypress {
                        let rt = global_runtime();
                        let mut kp = rt.block_on(kp_arc.lock());
                        match rt.block_on(kp.hold(&inputs)) {
                            Ok(()) => return Ok(()),
                            Err(e) => {
                                return Err(mlua::Error::runtime(format!(
                                    "Serial hold failed: {}",
                                    e
                                )));
                            }
                        }
                    }
                }
                println!(
                    "[PokeCon Lua] Hold '{}' (serial not connected, logged only)",
                    button_str
                );
                Ok(())
            })?,
        )?;

        // hold_end(button_str): Legacy alias for release (kept for backward compat).
        let handle_hold_end = handle.clone();
        api.set(
            "hold_end",
            lua.create_function(move |_, buttons: Vec<String>| {
                let joined = buttons.join("|");
                let inputs = parse_buttons(&joined);
                if inputs.is_empty() {
                    return Ok(());
                }
                if let Some(ref h) = handle_hold_end {
                    if let Some(ref kp_arc) = h.keypress {
                        let rt = global_runtime();
                        let mut kp = rt.block_on(kp_arc.lock());
                        let _ = rt.block_on(kp.input_end(&inputs));
                        return Ok(());
                    }
                }
                println!("[PokeCon] Release {:?}", buttons);
                Ok(())
            })?,
        )?;

        api.set(
            "move_stick",
            lua.create_function(|_, (stick, x, y): (String, f64, f64)| {
                println!("[PokeCon] Move {} stick to ({}, {})", stick, x, y);
                Ok(())
            })?,
        )?;

        // ── Image capture ─────────────────────────────────────────

        // capture(): Capture a frame from the camera if available.
        let handle_capture = handle.clone();
        api.set(
            "capture",
            lua.create_function(move |lua, _: ()| {
                #[cfg(feature = "v4l")]
                if let Some(ref h) = handle_capture {
                    if let Some(ref cam_arc) = h.camera {
                        let rt = global_runtime();
                        match rt.block_on(cam_arc.capture()) {
                            Ok(frame) => {
                                let t = lua.create_table()?;
                                t.set("width", frame.width)?;
                                t.set("height", frame.height)?;
                                t.set("format", format!("{:?}", frame.format))?;
                                t.set("size_bytes", frame.data.len())?;
                                return Ok(Value::Table(t));
                            }
                            Err(e) => {
                                return Err(mlua::Error::runtime(format!("Capture failed: {}", e)));
                            }
                        }
                    }
                }
                println!("[PokeCon Lua] capture() called but no camera is configured");
                let t = lua.create_table()?;
                t.set("width", 0u32)?;
                t.set("height", 0u32)?;
                t.set("error", "No camera configured")?;
                Ok(Value::Table(t))
            })?,
        )?;

        // match_template(template_path, threshold): Load a template image
        // and perform template matching against the current camera frame.
        let handle_match = handle.clone();
        api.set(
            "match_template",
            lua.create_function(move |lua, (template_path, threshold): (String, f64)| {
                #[cfg(feature = "v4l")]
                if let Some(ref h) = handle_match {
                    if let Some(ref cam_arc) = h.camera {
                        let rt = global_runtime();
                        let frame = match rt.block_on(cam_arc.capture()) {
                            Ok(f) => f,
                            Err(e) => {
                                return Err(mlua::Error::runtime(format!(
                                    "Camera capture failed: {}",
                                    e
                                )));
                            }
                        };
                        // Convert captured frame to grayscale for matching
                        let gray_frame =
                            match crate::cv::image_processing::ImageProcessor::grayscale(&frame) {
                                Ok(f) => f,
                                Err(e) => {
                                    return Err(mlua::Error::runtime(format!(
                                        "Grayscale conversion failed: {}",
                                        e
                                    )));
                                }
                            };
                        // Load template from disk (PPM/PGM format)
                        let template = match load_image_from_file(&template_path) {
                            Ok(img) => img,
                            Err(e) => {
                                return Err(mlua::Error::runtime(format!(
                                    "Failed to load template '{}': {}",
                                    template_path, e
                                )));
                            }
                        };
                        // Convert template to grayscale
                        let gray_template =
                            match crate::cv::image_processing::ImageProcessor::grayscale(&template)
                            {
                                Ok(t) => t,
                                Err(e) => {
                                    return Err(mlua::Error::runtime(format!(
                                        "Template grayscale failed: {}",
                                        e
                                    )));
                                }
                            };
                        // Perform matching
                        let results =
                            match crate::cv::image_processing::ImageProcessor::template_match(
                                &gray_frame,
                                &gray_template,
                                threshold,
                            ) {
                                Ok(r) => r,
                                Err(e) => {
                                    return Err(mlua::Error::runtime(format!(
                                        "Template matching failed: {}",
                                        e
                                    )));
                                }
                            };
                        // Return results as a Lua table
                        let result_table = lua.create_table()?;
                        for (i, m) in results.iter().enumerate() {
                            let entry = lua.create_table()?;
                            entry.set("x", m.point.x)?;
                            entry.set("y", m.point.y)?;
                            entry.set("confidence", m.confidence)?;
                            result_table.set(i + 1, entry)?;
                        }
                        return Ok(Value::Table(result_table));
                    }
                }
                println!(
                    "[PokeCon Lua] match_template('{}', {}) called but no camera configured",
                    template_path, threshold
                );
                Ok(Value::Table(lua.create_table()?))
            })?,
        )?;

        // ── Legacy press/hold with opts (backward compat) ─────────

        let handle_press_legacy = handle.clone();
        api.set(
            "press_legacy",
            lua.create_function(move |_, (buttons, opts): (Vec<String>, Option<Table>)| {
                let duration: u64 = opts
                    .as_ref()
                    .and_then(|t| t.get("duration").ok())
                    .unwrap_or(100);
                let wait: u64 = opts
                    .as_ref()
                    .and_then(|t| t.get("wait").ok())
                    .unwrap_or(100);
                let joined = buttons.join("|");
                let inputs = parse_buttons(&joined);
                if inputs.is_empty() {
                    return Ok(());
                }
                if let Some(ref h) = handle_press_legacy {
                    if let Some(ref kp_arc) = h.keypress {
                        let rt = global_runtime();
                        let mut kp = rt.block_on(kp_arc.lock());
                        if let Err(e) = rt.block_on(kp.input(&inputs)) {
                            return Err(mlua::Error::runtime(format!(
                                "Serial input failed: {}",
                                e
                            )));
                        }
                        std::thread::sleep(Duration::from_millis(duration));
                        if let Err(e) = rt.block_on(kp.input_end(&inputs)) {
                            return Err(mlua::Error::runtime(format!(
                                "Serial input_end failed: {}",
                                e
                            )));
                        }
                        std::thread::sleep(Duration::from_millis(wait));
                        return Ok(());
                    }
                }
                for btn in &buttons {
                    println!(
                        "[PokeCon] Press {} for {}ms (wait {}ms)",
                        btn, duration, wait
                    );
                }
                Ok(())
            })?,
        )?;

        let handle_hold_legacy = handle.clone();
        api.set(
            "hold_legacy",
            lua.create_function(move |_, (buttons, opts): (Vec<String>, Option<Table>)| {
                let wait: u64 = opts
                    .as_ref()
                    .and_then(|t| t.get("wait").ok())
                    .unwrap_or(100);
                let joined = buttons.join("|");
                let inputs = parse_buttons(&joined);
                if inputs.is_empty() {
                    return Ok(());
                }
                if let Some(ref h) = handle_hold_legacy {
                    if let Some(ref kp_arc) = h.keypress {
                        let rt = global_runtime();
                        let mut kp = rt.block_on(kp_arc.lock());
                        if let Err(e) = rt.block_on(kp.hold(&inputs)) {
                            return Err(mlua::Error::runtime(format!("Serial hold failed: {}", e)));
                        }
                        std::thread::sleep(Duration::from_millis(wait));
                        return Ok(());
                    }
                }
                for btn in &buttons {
                    println!("[PokeCon] Hold {} (wait {}ms)", btn, wait);
                }
                Ok(())
            })?,
        )?;

        // ── Logging ───────────────────────────────────────────────

        api.set(
            "log",
            lua.create_function(|_, msg: String| {
                println!("[PokeCon Lua] {}", msg);
                Ok(())
            })?,
        )?;

        api.set(
            "print_s",
            lua.create_function(|_, msg: String| {
                println!("[PokeCon Lua/print_s] {}", msg);
                Ok(())
            })?,
        )?;

        api.set(
            "print_t",
            lua.create_function(|_, msg: String| {
                println!("[PokeCon Lua/print_t] {}", msg);
                Ok(())
            })?,
        )?;

        api.set(
            "print_t1",
            lua.create_function(|_, msg: String| {
                println!("[PokeCon Lua/print_t1] {}", msg);
                Ok(())
            })?,
        )?;

        api.set(
            "print_t2",
            lua.create_function(|_, msg: String| {
                println!("[PokeCon Lua/print_t2] {}", msg);
                Ok(())
            })?,
        )?;

        // ── Event system ──────────────────────────────────────────
        //
        // Event functions need access to the Lua state to actually call
        // callbacks.  We store them in the Lua registry and the caller
        // is responsible for bridging events (see `LuaRuntime::dispatch_event`).

        // Pre-clone handle for closures created after the event section,
        // since `emit` moves `handle`.
        let handle_serial = handle.clone();
        let handle_direct_serial = handle.clone();

        let handle_clone = handle.clone();
        api.set(
            "on",
            lua.create_function(move |lua, (event_name, callback): (String, LuaFunction)| {
                let key = ApiHandle::store_callback(lua, &callback);
                if let Some(ref h) = handle_clone {
                    let mut cbs = h.lua_callbacks.lock();
                    cbs.entry(event_name.clone()).or_default().push(key);
                    println!(
                        "[PokeCon Lua] Registered handler #{} for '{}'",
                        key, event_name
                    );
                }
                Ok(())
            })?,
        )?;

        let handle_clone = handle.clone();
        api.set(
            "off",
            lua.create_function(move |_, event_name: String| {
                if let Some(ref h) = handle_clone {
                    let mut cbs = h.lua_callbacks.lock();
                    cbs.remove(&event_name);
                    if let Some(ref bus) = h.event_bus {
                        let _ = bus.off(&event_name);
                    }
                    println!("[PokeCon Lua] Removed handlers for '{}'", event_name);
                }
                Ok(())
            })?,
        )?;

        api.set(
            "emit",
            lua.create_function(move |_, (event_name, data): (String, Option<Value>)| {
                let json_data = lua_value_to_json(&data);
                let mut event = Event::new(&event_name, json_data);
                if let Some(ref h) = handle {
                    if let Some(ref bus) = h.event_bus {
                        bus.emit(&mut event);
                    }
                }
                println!(
                    "[PokeCon Lua] Emitted event '{}' (propagation: {})",
                    event_name,
                    if event.propagation_stopped {
                        "stopped"
                    } else {
                        "continued"
                    }
                );
                Ok(())
            })?,
        )?;

        api.set(
            "define_event",
            lua.create_function(|_, (name, schema): (String, Option<Table>)| {
                println!(
                    "[PokeCon Lua] Defined user event '{}' with schema {:?}",
                    name,
                    schema.is_some()
                );
                Ok(())
            })?,
        )?;

        api.set(
            "autocmd",
            lua.create_function(|_, (event_name, opts): (String, Option<Table>)| {
                let callback_str = opts
                    .as_ref()
                    .and_then(|t| t.get::<String>("callback").ok())
                    .unwrap_or_default();
                let group = opts
                    .as_ref()
                    .and_then(|t| t.get::<String>("group").ok())
                    .unwrap_or_default();
                println!(
                    "[PokeCon Lua] Registered autocmd '{}' (group={}, callback={})",
                    event_name, group, callback_str
                );
                Ok(())
            })?,
        )?;

        // ── Serial communication ──────────────────────────────────

        // send_serial(data): Send a raw string command over the serial port.
        api.set(
            "send_serial",
            lua.create_function(move |_, data: String| {
                if let Some(ref h) = handle_serial {
                    if let Some(ref kp_arc) = h.keypress {
                        let rt = global_runtime();
                        let mut kp = rt.block_on(kp_arc.lock());
                        match rt.block_on(kp.sender_mut().write_row_wo_counter(&data)) {
                            Ok(()) => {
                                tracing::info!("[PokeCon Lua] Sent serial: {}", data);
                                return Ok(());
                            }
                            Err(e) => {
                                return Err(mlua::Error::runtime(format!(
                                    "Serial send failed: {}",
                                    e
                                )));
                            }
                        }
                    }
                }
                println!("[PokeCon Lua] Serial send: '{}'", data);
                Ok(())
            })?,
        )?;

        // direct_serial(cmd, wait_ms): Legacy direct serial command.
        api.set(
            "direct_serial",
            lua.create_function(move |_, (cmd, wait_ms): (String, Option<u64>)| {
                let w = wait_ms.unwrap_or(0);
                if let Some(ref h) = handle_direct_serial {
                    if let Some(ref kp_arc) = h.keypress {
                        let rt = global_runtime();
                        let mut kp = rt.block_on(kp_arc.lock());
                        let _ = rt.block_on(kp.sender_mut().write_row_wo_counter(&cmd));
                        if w > 0 {
                            std::thread::sleep(Duration::from_millis(w));
                        }
                        return Ok(());
                    }
                }
                println!("[PokeCon Lua] Serial: '{}' (wait={}ms)", cmd, w);
                Ok(())
            })?,
        )?;

        // ── Notifications ─────────────────────────────────────────

        // notify(message): Send a desktop notification.
        api.set(
            "notify",
            lua.create_function(|_, message: String| {
                #[cfg(feature = "notify")]
                {
                    use crate::notify::{Notification, Notifier, windows::WindowsNotifier};
                    let notifier = WindowsNotifier::new("Poke-Controller");
                    let notification = Notification::new(message.clone());
                    let rt = global_runtime();
                    match rt.block_on(notifier.send(&notification)) {
                        Ok(()) => {
                            tracing::info!("[PokeCon Lua] Desktop notification sent: {}", message);
                            return Ok(());
                        }
                        Err(e) => {
                            // Notification failures are non-fatal (headless env, etc.)
                            tracing::warn!("[PokeCon Lua] Desktop notification failed: {}", e);
                        }
                    }
                }
                println!("[PokeCon Lua] Notify: {}", message);
                Ok(())
            })?,
        )?;

        // discord_text(content, index): Send a Discord webhook notification.
        api.set(
            "discord_text",
            lua.create_function(|_, (content, index): (String, Option<u64>)| {
                let idx = index.unwrap_or(0);
                println!("[PokeCon Lua] Discord [{}]: {}", idx, content);
                Ok(())
            })?,
        )?;

        // line_text(txt, token): Send a LINE notification.
        api.set(
            "line_text",
            lua.create_function(|_, (txt, token): (String, Option<String>)| {
                println!(
                    "[PokeCon Lua] LINE: {} (token={})",
                    txt,
                    token.unwrap_or_default()
                );
                Ok(())
            })?,
        )?;

        lua.globals().set("pokecon", api)?;
        Ok(())
    }

    /// Convenience: register with no handle (event functions become no-ops).
    pub fn register(lua: &Lua) -> LuaResult<()> {
        Self::register_with_handle(lua, None)
    }

    /// Register with an ApiHandle for event-bus integration.
    pub fn register_with(lua: &Lua, handle: Arc<ApiHandle>) -> LuaResult<()> {
        Self::register_with_handle(lua, Some(handle))
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Convert an optional Lua `Value` into a `serde_json::Value` for event data.
fn lua_value_to_json(val: &Option<Value>) -> serde_json::Value {
    match val {
        Some(Value::Table(t)) => {
            let mut map = serde_json::Map::new();
            for (key, val) in t.pairs::<Value, Value>().flatten() {
                let k = format!("{:?}", key);
                let v = format!("{:?}", val);
                map.insert(k, serde_json::Value::String(v));
            }
            serde_json::Value::Object(map)
        }
        Some(Value::String(s)) => serde_json::Value::String(s.to_string_lossy().to_string()),
        Some(Value::Integer(i)) => serde_json::Value::Number((*i).into()),
        Some(Value::Number(n)) => {
            if let Some(i) = serde_json::Number::from_f64(*n) {
                serde_json::Value::Number(i)
            } else {
                serde_json::Value::Null
            }
        }
        Some(Value::Boolean(b)) => serde_json::Value::Bool(*b),
        Some(Value::Nil) | None => serde_json::Value::Null,
        _ => serde_json::Value::Null,
    }
}

/// Load an image from a file path into a `Frame`.
///
/// Supports PPM (P6) and PGM (P5) binary formats.
#[cfg(feature = "v4l")]
fn load_image_from_file(path: &str) -> Result<crate::cv::Frame, String> {
    let data = std::fs::read(path).map_err(|e| format!("Cannot read file: {}", e))?;
    let s = std::str::from_utf8(&data)
        .map_err(|_| "File is not valid UTF-8 (image headers)".to_string())?;

    if s.starts_with("P6\n") || s.starts_with("P6\r\n") || s.starts_with("P6 ") {
        load_ppm(&data)
    } else if s.starts_with("P5\n") || s.starts_with("P5\r\n") || s.starts_with("P5 ") {
        load_pgm(&data)
    } else {
        Err("Unsupported image format. Supported: PPM (P6), PGM (P5)".to_string())
    }
}

/// Parse a PPM (P6) binary image.
#[cfg(feature = "v4l")]
fn load_ppm(data: &[u8]) -> Result<crate::cv::Frame, String> {
    let header_end = find_header_end(data)?;
    // Header is ASCII text
    let header =
        std::str::from_utf8(&data[..header_end]).map_err(|_| "Invalid PPM header".to_string())?;
    let parts: Vec<&str> = header
        .split_whitespace()
        .filter(|s| !s.starts_with('#'))
        .collect();
    if parts.len() < 4 || parts[0] != "P6" {
        return Err("Invalid PPM header".to_string());
    }
    let width: u32 = parts[1]
        .parse()
        .map_err(|_| "Invalid PPM width".to_string())?;
    let height: u32 = parts[2]
        .parse()
        .map_err(|_| "Invalid PPM height".to_string())?;
    let _maxval: u32 = parts[3]
        .parse()
        .map_err(|_| "Invalid PPM maxval".to_string())?;
    let pixel_data = &data[header_end..];
    let expected = (width * height * 3) as usize;
    if pixel_data.len() < expected {
        return Err(format!(
            "Truncated PPM data: expected {} bytes, got {}",
            expected,
            pixel_data.len()
        ));
    }
    Ok(crate::cv::Frame {
        width,
        height,
        data: pixel_data[..expected].to_vec(),
        format: crate::cv::PixelFormat::Rgb,
    })
}

/// Parse a PGM (P5) binary image.
#[cfg(feature = "v4l")]
fn load_pgm(data: &[u8]) -> Result<crate::cv::Frame, String> {
    let header_end = find_header_end(data)?;
    let header =
        std::str::from_utf8(&data[..header_end]).map_err(|_| "Invalid PGM header".to_string())?;
    let parts: Vec<&str> = header
        .split_whitespace()
        .filter(|s| !s.starts_with('#'))
        .collect();
    if parts.len() < 4 || parts[0] != "P5" {
        return Err("Invalid PGM header".to_string());
    }
    let width: u32 = parts[1]
        .parse()
        .map_err(|_| "Invalid PGM width".to_string())?;
    let height: u32 = parts[2]
        .parse()
        .map_err(|_| "Invalid PGM height".to_string())?;
    let _maxval: u32 = parts[3]
        .parse()
        .map_err(|_| "Invalid PGM maxval".to_string())?;
    let pixel_data = &data[header_end..];
    let expected = (width * height) as usize;
    if pixel_data.len() < expected {
        return Err(format!(
            "Truncated PGM data: expected {} bytes, got {}",
            expected,
            pixel_data.len()
        ));
    }
    Ok(crate::cv::Frame {
        width,
        height,
        data: pixel_data[..expected].to_vec(),
        format: crate::cv::PixelFormat::Gray,
    })
}

/// Find the end of the PPM/PGM header (after the last whitespace before pixel data).
#[cfg(feature = "v4l")]
fn find_header_end(data: &[u8]) -> Result<usize, String> {
    let text = std::str::from_utf8(data).map_err(|_| "Not valid UTF-8".to_string())?;
    // Find first newline (end of magic number line)
    let pos = text
        .find('\n')
        .ok_or_else(|| "No newline in PPM/PGM header".to_string())?;
    let after_magic = &text[pos + 1..];
    let mut lines = after_magic.lines();
    // Skip comment lines to find dimensions
    let dim_line = loop {
        match lines.next() {
            Some(line) if line.starts_with('#') => continue,
            Some(line) => break line,
            None => return Err("Unexpected end of PPM/PGM header".to_string()),
        }
    };
    let _dims: Vec<&str> = dim_line.split_whitespace().collect();
    if _dims.len() < 2 {
        return Err("Invalid dimensions in PPM/PGM header".to_string());
    }
    // Next non-comment line is maxval
    let maxval_line = loop {
        match lines.next() {
            Some(line) if line.starts_with('#') => continue,
            Some(line) => break line,
            None => return Err("Unexpected end of PPM/PGM header (maxval)".to_string()),
        }
    };
    // Calculate byte offset for pixel data
    let header_str = format!("{}\n{}\n{}\n", &text[..pos + 1], dim_line, maxval_line);
    Ok(header_str.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::{Lua, Table};

    #[test]
    fn test_api_registration() {
        let lua = Lua::new();
        PokeConApi::register(&lua).unwrap();

        let api: Table = lua.globals().get("pokecon").unwrap();
        assert!(api.contains_key("wait").unwrap());
        assert!(api.contains_key("press_button").unwrap());
        assert!(api.contains_key("press").unwrap());
        assert!(api.contains_key("hold").unwrap());
        assert!(api.contains_key("release").unwrap());
        assert!(api.contains_key("hold_end").unwrap());
        assert!(api.contains_key("move_stick").unwrap());
        assert!(api.contains_key("log").unwrap());
        assert!(api.contains_key("print_s").unwrap());
        assert!(api.contains_key("print_t").unwrap());
        assert!(api.contains_key("print_t1").unwrap());
        assert!(api.contains_key("print_t2").unwrap());
        assert!(api.contains_key("capture").unwrap());
        assert!(api.contains_key("match_template").unwrap());
        assert!(api.contains_key("send_serial").unwrap());
        assert!(api.contains_key("direct_serial").unwrap());
        assert!(api.contains_key("notify").unwrap());
        assert!(api.contains_key("discord_text").unwrap());
        assert!(api.contains_key("line_text").unwrap());
        assert!(api.contains_key("on").unwrap());
        assert!(api.contains_key("off").unwrap());
        assert!(api.contains_key("emit").unwrap());
        assert!(api.contains_key("define_event").unwrap());
        assert!(api.contains_key("autocmd").unwrap());
    }

    #[test]
    fn test_log_function() {
        let lua = Lua::new();
        PokeConApi::register(&lua).unwrap();

        lua.load("pokecon.log('test message')").exec().unwrap();
    }

    #[test]
    fn test_press_function() {
        let lua = Lua::new();
        PokeConApi::register(&lua).unwrap();

        // New API: single string argument
        lua.load("pokecon.press('A')").exec().unwrap();
        lua.load("pokecon.press('A|B')").exec().unwrap();
        lua.load("pokecon.press('DPAD_UP')").exec().unwrap();
    }

    #[test]
    fn test_hold_and_release() {
        let lua = Lua::new();
        PokeConApi::register(&lua).unwrap();

        // New API: hold and release with single string
        lua.load("pokecon.hold('ZL')").exec().unwrap();
        lua.load("pokecon.release('ZL')").exec().unwrap();

        // hold_end with Vec<String> (legacy API)
        lua.load("pokecon.hold_end({'ZR'})").exec().unwrap();
    }

    #[test]
    fn test_capture_function() {
        let lua = Lua::new();
        PokeConApi::register(&lua).unwrap();

        // Capture (no camera configured — should return empty table)
        lua.load(
            r#"
            local result = pokecon.capture()
            assert(type(result) == "table")
            "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn test_match_template_function() {
        let lua = Lua::new();
        PokeConApi::register(&lua).unwrap();

        // match_template (no camera configured — should return empty table)
        lua.load(
            r#"
            local result = pokecon.match_template("nonexistent.ppm", 0.8)
            assert(type(result) == "table")
            "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn test_send_serial_function() {
        let lua = Lua::new();
        PokeConApi::register(&lua).unwrap();

        // send_serial (no serial connected — should log and return)
        lua.load("pokecon.send_serial('REQ')").exec().unwrap();
    }

    #[test]
    fn test_notify_function() {
        let lua = Lua::new();
        PokeConApi::register(&lua).unwrap();

        // notify (may fail silently in headless env)
        lua.load("pokecon.notify('test notification')")
            .exec()
            .unwrap();
    }

    #[test]
    fn test_press_invalid_button() {
        let lua = Lua::new();
        PokeConApi::register(&lua).unwrap();

        // Invalid button should return an error
        let result = lua.load("pokecon.press('')").exec();
        assert!(result.is_err());
    }

    #[test]
    fn test_emit_event() {
        let lua = Lua::new();
        PokeConApi::register(&lua).unwrap();

        lua.load(r#"pokecon.emit("test_event", { key = "value" })"#)
            .exec()
            .unwrap();
    }

    #[test]
    fn test_emit_string_event() {
        let lua = Lua::new();
        PokeConApi::register(&lua).unwrap();

        lua.load(r#"pokecon.emit("simple_event", "hello")"#)
            .exec()
            .unwrap();
    }

    #[test]
    fn test_define_event() {
        let lua = Lua::new();
        PokeConApi::register(&lua).unwrap();

        lua.load(
            r#"
            pokecon.define_event("EncounterShiny", {
                pokemon_name = { type = "string", required = true }
            })
            "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn test_autocmd() {
        let lua = Lua::new();
        PokeConApi::register(&lua).unwrap();

        lua.load(
            r#"
            pokecon.autocmd("CommandStartPre", {
                pattern = "*",
                group = "my_group",
                callback = "my_callback"
            })
            "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn test_print_functions() {
        let lua = Lua::new();
        PokeConApi::register(&lua).unwrap();

        lua.load(
            r#"
            pokecon.print_s("stdout message")
            pokecon.print_t("terminal message")
            pokecon.print_t1("top log")
            pokecon.print_t2("bottom log")
            "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn test_direct_serial() {
        let lua = Lua::new();
        PokeConApi::register(&lua).unwrap();

        lua.load(r#"pokecon.direct_serial("REQ", 100)"#)
            .exec()
            .unwrap();
    }

    #[test]
    fn test_wait_function() {
        let lua = Lua::new();
        PokeConApi::register(&lua).unwrap();

        lua.load("pokecon.wait(50)").exec().unwrap();
    }

    #[test]
    fn test_emit_with_event_bus() {
        let lua = Lua::new();
        let bus = EventBus::new();
        let bus_clone = bus.clone();
        let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let called_clone = called.clone();

        // Register a Rust-side handler on the bus
        bus_clone.on("test_from_lua", move |_| {
            called_clone.store(true, std::sync::atomic::Ordering::SeqCst);
        });

        let handle = Arc::new(ApiHandle::with_event_bus(bus_clone));
        PokeConApi::register_with(&lua, handle).unwrap();

        lua.load(r#"pokecon.emit("test_from_lua", { key = 42 })"#)
            .exec()
            .unwrap();

        assert!(called.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn test_on_registers_callback() {
        let lua = Lua::new();
        let handle = Arc::new(ApiHandle::new());
        PokeConApi::register_with(&lua, handle.clone()).unwrap();

        lua.load(
            r#"
            pokecon.on("test.event", function()
                -- no-op callback
            end)
            "#,
        )
        .exec()
        .unwrap();

        // Verify callback was stored in the Lua registry
        let cbs = handle.lua_callbacks.lock();
        assert!(cbs.contains_key("test.event"));
        assert_eq!(cbs["test.event"].len(), 1);
    }

    #[test]
    fn test_off_removes_callback() {
        let lua = Lua::new();
        let handle = Arc::new(ApiHandle::new());
        PokeConApi::register_with(&lua, handle.clone()).unwrap();

        lua.load(
            r#"
            pokecon.on("test.event", function() end)
            "#,
        )
        .exec()
        .unwrap();
        assert!(handle.lua_callbacks.lock().contains_key("test.event"));

        lua.load(r#"pokecon.off("test.event")"#).exec().unwrap();
        assert!(!handle.lua_callbacks.lock().contains_key("test.event"));
    }

    #[test]
    fn test_on_multiple_callbacks() {
        let lua = Lua::new();
        let handle = Arc::new(ApiHandle::new());
        PokeConApi::register_with(&lua, handle.clone()).unwrap();

        lua.load(
            r#"
            pokecon.on("multi", function() end)
            pokecon.on("multi", function() end)
            "#,
        )
        .exec()
        .unwrap();

        let cbs = handle.lua_callbacks.lock();
        assert_eq!(cbs["multi"].len(), 2);
    }

    #[test]
    fn test_emit_via_event_bus_triggers_handler() {
        let lua = Lua::new();
        let bus = EventBus::new();
        let bus_clone = bus.clone();
        let hit_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let hit = hit_count.clone();

        bus_clone.on("emit.test", move |_| {
            hit.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });

        let handle = Arc::new(ApiHandle::with_event_bus(bus_clone));
        PokeConApi::register_with(&lua, handle).unwrap();

        lua.load(r#"pokecon.emit("emit.test", {})"#).exec().unwrap();

        assert_eq!(hit_count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn test_api_handle_with_keypress() {
        let sender = crate::serial::sender::Sender::new(false);
        let kp = KeyPress::new(sender);
        let handle = ApiHandle::new().with_keypress(kp);
        assert!(handle.keypress.is_some());
    }

    #[test]
    fn test_api_handle_default_keypress_none() {
        let handle = ApiHandle::new();
        assert!(handle.keypress.is_none());
    }

    #[test]
    fn test_parse_buttons_single() {
        let inputs = parse_buttons("A");
        assert_eq!(inputs.len(), 1);
        assert!(matches!(inputs[0], GamepadInput::SingleButton(Button::A)));
    }

    #[test]
    fn test_parse_buttons_multiple() {
        let inputs = parse_buttons("A|B");
        assert_eq!(inputs.len(), 2);
    }

    #[test]
    fn test_parse_buttons_dpad() {
        let inputs = parse_buttons("DPAD_UP");
        assert_eq!(inputs.len(), 1);
        assert!(matches!(inputs[0], GamepadInput::SingleHat(Hat::TOP)));
    }

    #[test]
    fn test_parse_buttons_empty() {
        let inputs = parse_buttons("");
        assert!(inputs.is_empty());
    }
}
