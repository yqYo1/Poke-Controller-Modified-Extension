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

use pokecon_events::{Event, EventBus};

/// Handle for bridging Lua callbacks into the Rust event system.
///
/// The handle stores the shared `EventBus` reference (if any) and a
/// **Lua callback registry** so that `pokecon.on()` from Lua can be
/// tracked without violating `Send + Sync` constraints.
#[derive(Clone)]
pub struct ApiHandle {
    /// Shared event bus for dispatching events from Lua.
    pub event_bus: Option<EventBus>,
    /// Registry of Lua callbacks, keyed by event name.
    /// Stored behind `Arc<Mutex<…>>` so it can be shared across threads;
    /// the actual `mlua::Function` values are only accessed from the
    /// Lua runtime's own thread.
    pub lua_callbacks: Arc<Mutex<HashMap<String, Vec<usize>>>>,
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
        }
    }

    /// Create a new ApiHandle with an event bus.
    pub fn with_event_bus(event_bus: EventBus) -> Self {
        Self {
            event_bus: Some(event_bus),
            lua_callbacks: Arc::new(Mutex::new(HashMap::new())),
        }
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
    /// will be wired to the shared `EventBus`.
    pub fn register_with_handle(lua: &Lua, handle: Option<Arc<ApiHandle>>) -> LuaResult<()> {
        let api = lua.create_table()?;

        // ── Initialise callback registry in Lua ──────────────────
        let cb_reg = lua.create_table()?;
        cb_reg.set("__next_key", 1usize)?;
        lua.set_named_registry_value("pokecon_callbacks", cb_reg)?;

        // ── Input operations ──────────────────────────────────────

        api.set(
            "wait",
            lua.create_function(|_, ms: u64| {
                std::thread::sleep(std::time::Duration::from_millis(ms));
                Ok(())
            })?,
        )?;

        api.set(
            "press_button",
            lua.create_function(|_, (button, duration): (String, u64)| {
                println!("[PokeCon] Press {} for {}ms", button, duration);
                Ok(())
            })?,
        )?;

        api.set(
            "press",
            lua.create_function(|_, (buttons, opts): (Vec<String>, Option<Table>)| {
                let duration: u64 = opts
                    .as_ref()
                    .and_then(|t| t.get("duration").ok())
                    .unwrap_or(100);
                let wait: u64 = opts
                    .as_ref()
                    .and_then(|t| t.get("wait").ok())
                    .unwrap_or(100);
                for btn in &buttons {
                    println!(
                        "[PokeCon] Press {} for {}ms (wait {}ms)",
                        btn, duration, wait
                    );
                }
                Ok(())
            })?,
        )?;

        api.set(
            "hold",
            lua.create_function(|_, (buttons, opts): (Vec<String>, Option<Table>)| {
                let wait: u64 = opts
                    .as_ref()
                    .and_then(|t| t.get("wait").ok())
                    .unwrap_or(100);
                for btn in &buttons {
                    println!("[PokeCon] Hold {} (wait {}ms)", btn, wait);
                }
                Ok(())
            })?,
        )?;

        api.set(
            "hold_end",
            lua.create_function(|_, buttons: Vec<String>| {
                for btn in &buttons {
                    println!("[PokeCon] Release {}", btn);
                }
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

        let handle_clone = handle.clone();
        api.set(
            "on",
            lua.create_function(move |lua, (event_name, callback): (String, LuaFunction)| {
                // Store the callback reference in the Lua registry
                let key = ApiHandle::store_callback(lua, &callback);

                // Log the registration
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
                // Build JSON data from the Lua value
                let json_data = lua_value_to_json(&data);
                let event = Event::new(&event_name, json_data);

                // Emit to global EventBus if available
                if let Some(ref h) = handle {
                    if let Some(ref bus) = h.event_bus {
                        bus.emit(&event);
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

        // ── Notification stubs ────────────────────────────────────

        api.set(
            "discord_text",
            lua.create_function(|_, (content, index): (String, Option<u64>)| {
                let idx = index.unwrap_or(0);
                println!("[PokeCon Lua] Discord [{}]: {}", idx, content);
                Ok(())
            })?,
        )?;

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

        // ── Direct serial ─────────────────────────────────────────

        api.set(
            "direct_serial",
            lua.create_function(|_, (cmd, wait_ms): (String, Option<u64>)| {
                let w = wait_ms.unwrap_or(0);
                println!("[PokeCon Lua] Serial: '{}' (wait={}ms)", cmd, w);
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
            // mlua::Number is f64
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
        assert!(api.contains_key("hold_end").unwrap());
        assert!(api.contains_key("move_stick").unwrap());
        assert!(api.contains_key("log").unwrap());
        assert!(api.contains_key("print_s").unwrap());
        assert!(api.contains_key("print_t").unwrap());
        assert!(api.contains_key("print_t1").unwrap());
        assert!(api.contains_key("print_t2").unwrap());
        assert!(api.contains_key("on").unwrap());
        assert!(api.contains_key("off").unwrap());
        assert!(api.contains_key("emit").unwrap());
        assert!(api.contains_key("define_event").unwrap());
        assert!(api.contains_key("autocmd").unwrap());
        assert!(api.contains_key("discord_text").unwrap());
        assert!(api.contains_key("line_text").unwrap());
        assert!(api.contains_key("direct_serial").unwrap());
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

        lua.load(
            r#"
            pokecon.press({"A", "B"}, { duration = 200, wait = 150 })
            pokecon.press({"X"})
            "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn test_hold_and_release() {
        let lua = Lua::new();
        PokeConApi::register(&lua).unwrap();

        lua.load(
            r#"
            pokecon.hold({"ZL", "ZR"}, { wait = 500 })
            pokecon.hold_end({"ZL", "ZR"})
            "#,
        )
        .exec()
        .unwrap();
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
}
