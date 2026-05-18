#![allow(clippy::arc_with_non_send_sync)]
//!
//! Provides a thread-safe, async-aware wrapper around the `mlua::Lua` runtime
//! with integration into the PokeCon event system.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::info;

use crate::events::{Event, EventBus, EventPhase};
use mlua::Lua;

use crate::lua::api::{ApiHandle, PokeConApi};

/// Errors that can occur in the Lua runtime.
#[derive(Debug, thiserror::Error)]
pub enum LuaRuntimeError {
    /// Lua VM error (script load, function call, etc.)
    #[error("Lua error: {0}")]
    Lua(#[from] mlua::Error),
    /// I/O error (file reading, etc.)
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// No script has been loaded yet.
    #[error("Script not loaded")]
    NotLoaded,
    /// The requested function was not found in the Lua globals.
    #[error("Function not found: {0}")]
    FunctionNotFound(String),
    /// Script execution exceeded the allowed timeout.
    #[error("Script execution timeout after {0}s")]
    Timeout(u64),
}

/// Thread-safe Lua runtime wrapper.
///
/// Wraps a single `mlua::Lua` instance inside `Arc<Mutex<Lua>>` so that
/// multiple async tasks can share the same runtime.  An optional `EventBus`
/// can be wired so that the Lua `pokecon.on()` / `pokecon.emit()` family
/// of functions talk to the rest of the PokeCon event system.
///
/// # Lua API
///
/// The `pokecon` global table provides:
///
/// | Category   | Functions |
/// |------------|-----------|
/// | Input      | `wait`, `press_button`, `press`, `hold`, `hold_end`, `move_stick`, `direct_serial` |
/// | Logging    | `log`, `print_s`, `print_t`, `print_t1`, `print_t2` |
/// | Events     | `on`, `off`, `emit`, `define_event`, `autocmd` |
/// | Notify     | `discord_text`, `line_text` |
///
pub struct LuaRuntime {
    lua: Arc<Mutex<Lua>>,
    loaded_script: Option<String>,
    event_bus: Option<EventBus>,
    api_handle: Arc<ApiHandle>,
}

impl LuaRuntime {
    /// Create a new LuaRuntime with no event bus integration.
    ///
    /// Event-related Lua API calls (`on`, `off`, `emit`) will be no-ops
    /// (no global event bus), but `on` still stores callbacks for local
    /// dispatch via [`dispatch_event`](Self::dispatch_event).
    pub fn new() -> Result<Self, LuaRuntimeError> {
        let lua = Lua::new();
        let api_handle = Arc::new(ApiHandle::new());
        PokeConApi::register_with(&lua, api_handle.clone())?;
        Ok(Self {
            lua: Arc::new(Mutex::new(lua)),
            loaded_script: None,
            event_bus: None,
            api_handle,
        })
    }

    /// Create a new LuaRuntime wired to a shared EventBus.
    ///
    /// Lua event functions (`on`, `off`, `emit`) will be bridged to the
    /// given bus, allowing Lua scripts to participate in the global
    /// PokeCon event system.
    pub fn with_event_bus(event_bus: EventBus) -> Result<Self, LuaRuntimeError> {
        let lua = Lua::new();
        let api_handle = Arc::new(ApiHandle::with_event_bus(event_bus.clone()));
        PokeConApi::register_with(&lua, api_handle.clone())?;
        Ok(Self {
            lua: Arc::new(Mutex::new(lua)),
            loaded_script: None,
            event_bus: Some(event_bus),
            api_handle,
        })
    }

    /// Create a new LuaRuntime with a pre-built ApiHandle.
    ///
    /// Useful when the caller has already built an `ApiHandle` that
    /// shares an existing `EventBus` with other components.
    pub fn with_handle(api_handle: Arc<ApiHandle>) -> Result<Self, LuaRuntimeError> {
        let lua = Lua::new();
        let event_bus = api_handle.event_bus.clone();
        PokeConApi::register_with(&lua, api_handle.clone())?;
        Ok(Self {
            lua: Arc::new(Mutex::new(lua)),
            loaded_script: None,
            event_bus,
            api_handle,
        })
    }

    /// Load and execute a Lua script string.
    ///
    /// The script runs immediately; any global functions it defines become
    /// available for later `call_function()` calls.
    pub async fn load_script(&mut self, script: &str) -> Result<(), LuaRuntimeError> {
        let lua = self.lua.lock().await;
        lua.load(script).exec()?;
        self.loaded_script = Some(script.to_string());
        info!("Lua script loaded successfully ({} bytes)", script.len());
        Ok(())
    }

    /// Load and execute a Lua script from a file.
    ///
    /// Equivalent to reading the file as a string and calling `load_script`.
    pub async fn load_file<P: AsRef<Path>>(&mut self, path: P) -> Result<(), LuaRuntimeError> {
        let script = tokio::fs::read_to_string(path.as_ref()).await?;
        self.load_script(&script).await
    }

    /// Call a Lua function by name with the given arguments.
    ///
    /// Returns the result as an `mlua::Value`.
    pub async fn call_function<A: mlua::IntoLuaMulti>(
        &self,
        name: &str,
        args: A,
    ) -> Result<mlua::Value, LuaRuntimeError> {
        let lua = self.lua.lock().await;
        let globals = lua.globals();
        let func: mlua::Function = globals.get(name)?;
        let result = func.call_async(args).await?;
        Ok(result)
    }

    /// Call a Lua function with a timeout.
    ///
    /// If the call does not complete within `timeout_secs`, the error
    /// `LuaRuntimeError::Timeout` is returned.
    ///
    /// **Note:** Because `mlua` does not support true preemption, the
    /// timeout is implemented via `tokio::time::timeout` on the async
    /// side.  A long-running Lua loop will still block the async task
    /// until it yields or returns.
    pub async fn call_function_with_timeout<A: mlua::IntoLuaMulti>(
        &self,
        name: &str,
        args: A,
        timeout_secs: u64,
    ) -> Result<mlua::Value, LuaRuntimeError> {
        let lua = self.lua.clone();
        let name = name.to_owned();
        tokio::time::timeout(Duration::from_secs(timeout_secs), async move {
            let lua = lua.lock().await;
            let globals = lua.globals();
            let func: mlua::Function = globals.get(name.as_str())?;
            let result = func.call_async(args).await?;
            Ok(result)
        })
        .await
        .map_err(|_| LuaRuntimeError::Timeout(timeout_secs))?
    }

    /// Set a global variable in the Lua environment.
    pub async fn set_global(&self, name: &str, value: mlua::Value) -> Result<(), LuaRuntimeError> {
        let lua = self.lua.lock().await;
        lua.globals().set(name, value)?;
        Ok(())
    }

    /// Get a global variable from the Lua environment.
    pub async fn get_global(&self, name: &str) -> Result<mlua::Value, LuaRuntimeError> {
        let lua = self.lua.lock().await;
        let value = lua.globals().get(name)?;
        Ok(value)
    }

    /// Eval a Lua expression string and return the result.
    ///
    /// This is useful for quick inline evaluations (e.g., testing conditions).
    pub async fn eval<A: mlua::FromLuaMulti>(&self, code: &str) -> Result<A, LuaRuntimeError> {
        let lua = self.lua.lock().await;
        let result = lua.load(code).eval()?;
        Ok(result)
    }

    /// Check whether a global function exists.
    pub async fn has_function(&self, name: &str) -> bool {
        let lua = self.lua.lock().await;
        lua.globals().get::<mlua::Function>(name).is_ok()
    }

    /// Dispatch a Rust-side [`Event`] to Lua callbacks registered via
    /// `pokecon.on()`.
    ///
    /// Looks up the Lua callback registry by `event.event_type` and calls
    /// each registered function.  This must be called from a context that
    /// holds the Lua lock — typically from the main event loop.
    ///
    /// Returns the number of Lua callbacks that were invoked.
    pub async fn dispatch_event(&self, event: &Event) -> usize {
        let event_name = &event.event_type;

        // Look up callback keys WITHOUT holding the Lua lock.
        // This prevents lock ordering inversion between the parking_lot
        // `lua_callbacks` Mutex and the tokio `lua` Mutex.
        let keys = {
            let cbs = self.api_handle.lua_callbacks.lock();
            cbs.get(event_name).cloned().unwrap_or_default()
        };

        if keys.is_empty() {
            return 0;
        }

        // ── CAUTION: Re-entrancy risk ──────────────────────────────────
        // The Lua lock is held across `func.call()`.  If a Lua callback
        // calls `pokecon.emit()` which triggers a Rust handler that calls
        // `dispatch_event()` again, the second call will deadlock because
        // `tokio::sync::Mutex` is not re-entrant.
        //
        // A full fix would require deferred event processing or a
        // re-entrant locking mechanism.  For now, the lock is scoped to
        // the minimum needed: we already fetched callback keys above.
        let lua = self.lua.lock().await;

        // Retrieve the callback registry table from Lua
        let registry: mlua::Table = match lua.named_registry_value("pokecon_callbacks") {
            Ok(r) => r,
            Err(_) => return 0,
        };

        let mut count = 0usize;
        for key in &keys {
            if let Ok(func) = registry.get::<mlua::Function>(*key) {
                // Build a simple Lua table from the event data for the callback
                let event_table = event_to_lua_table(&lua, event);
                if func.call::<()>(event_table).is_ok() {
                    count += 1;
                }
            }
        }
        count
    }

    /// Return a reference to the shared EventBus, if one was wired.
    pub fn event_bus(&self) -> Option<&EventBus> {
        self.event_bus.as_ref()
    }

    /// Return a reference to the ApiHandle.
    pub fn api_handle(&self) -> &Arc<ApiHandle> {
        &self.api_handle
    }

    /// Attach a KeyPress instance for serial controller input.
    /// This enables `press()`, `hold()`, `release()`, and `send_serial()`
    /// Lua API functions to actually communicate with hardware.
    pub fn set_keypress(&mut self, keypress: crate::serial::keypress::KeyPress) {
        // Use make_mut since we need exclusive access to the ApiHandle
        // (the Arc has only one strong reference in typical usage)
        if let Some(inner) = Arc::get_mut(&mut self.api_handle) {
            inner.set_keypress(keypress);
        }
    }

    /// Attach a Camera instance for image capture and template matching.
    /// This enables `capture()` and `match_template()` Lua API functions.
    #[cfg(feature = "v4l")]
    pub fn set_camera(&mut self, camera: crate::cv::Camera) {
        if let Some(inner) = Arc::get_mut(&mut self.api_handle) {
            inner.set_camera(camera);
        }
    }

    /// Check whether a script has been loaded.
    pub fn is_loaded(&self) -> bool {
        self.loaded_script.is_some()
    }

    /// Return the loaded script text, if any.
    pub fn script(&self) -> Option<&str> {
        self.loaded_script.as_deref()
    }
}

impl Default for LuaRuntime {
    fn default() -> Self {
        Self::new().expect("Failed to create Lua runtime")
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Convert a Lua `Value` into a `serde_json::Value`.
///
/// Supports all basic Lua types:
/// - `nil` → JSON `null`
/// - `boolean` → JSON `bool`
/// - `integer` / `number` → JSON `number`
/// - `string` → JSON `string` (lossy on non-UTF-8)
/// - `table` → JSON array (if all integer keys 1..n) or JSON object
///
/// Unsupported types (`function`, `thread`, `userdata`, `lightuserdata`,
/// `error`, `luau`-specific types) are converted to JSON `null`.
///
/// Circular references in tables are detected and replaced with `null`.
pub fn lua_value_to_json(lua: &Lua, value: &mlua::Value) -> mlua::Result<serde_json::Value> {
    let mut visited: Vec<mlua::Table> = Vec::new();
    lua_value_to_json_inner(lua, value, &mut visited)
}

/// Internal recursive helper that tracks visited tables for cycle detection.
fn lua_value_to_json_inner(
    lua: &Lua,
    value: &mlua::Value,
    visited: &mut Vec<mlua::Table>,
) -> mlua::Result<serde_json::Value> {
    match value {
        mlua::Value::Nil => Ok(serde_json::Value::Null),

        mlua::Value::Boolean(b) => Ok(serde_json::Value::Bool(*b)),

        mlua::Value::Integer(i) => Ok(serde_json::Value::Number(serde_json::Number::from(*i))),

        mlua::Value::Number(n) => {
            // f64 may be NaN or Infinity — map those to null
            if let Some(num) = serde_json::Number::from_f64(*n) {
                Ok(serde_json::Value::Number(num))
            } else {
                Ok(serde_json::Value::Null)
            }
        }

        mlua::Value::String(s) => Ok(serde_json::Value::String(s.to_string_lossy())),

        mlua::Value::Table(t) => {
            // ── Cycle detection ────────────────────────────────
            if visited.contains(t) {
                return Ok(serde_json::Value::Null);
            }
            visited.push(t.clone());

            let result = table_to_json(lua, t, visited)?;

            visited.pop();
            Ok(result)
        }

        // ── Unsupported types → null ──────────────────────────
        mlua::Value::LightUserData(_)
        | mlua::Value::Function(_)
        | mlua::Value::Thread(_)
        | mlua::Value::UserData(_)
        | mlua::Value::Error(_)
        | mlua::Value::Other(_) => Ok(serde_json::Value::Null),
    }
}

/// Convert a Lua `Table` to a JSON value, using key inspection to decide
/// whether to produce an array or an object.
fn table_to_json(
    lua: &Lua,
    table: &mlua::Table,
    visited: &mut Vec<mlua::Table>,
) -> mlua::Result<serde_json::Value> {
    // Collect all keys first (avoids holding the Lua lock across recursion)
    let mut keys: Vec<mlua::Value> = Vec::new();
    table.for_each(|key: mlua::Value, _value: mlua::Value| {
        keys.push(key);
        Ok(())
    })?;

    if keys.is_empty() {
        // Empty table — default to empty array
        return Ok(serde_json::Value::Array(Vec::new()));
    }

    if is_array_keys(&keys) {
        // ── Array mode ────────────────────────────────────────
        let len = keys.len();
        let mut arr = Vec::with_capacity(len);
        for i in 1..=len {
            let val: mlua::Value = table.raw_get(i as i64)?;
            arr.push(lua_value_to_json_inner(lua, &val, visited)?);
        }
        Ok(serde_json::Value::Array(arr))
    } else {
        // ── Object mode ───────────────────────────────────────
        let mut map = serde_json::Map::new();
        for key in &keys {
            let val: mlua::Value = table.raw_get(key.clone())?;
            let k = lua_value_to_json_inner(lua, key, visited)?;
            let v = lua_value_to_json_inner(lua, &val, visited)?;

            // JSON object keys must be strings — convert non-string keys
            let key_str = match k {
                serde_json::Value::String(s) => s,
                other => format!("{}", other),
            };
            map.insert(key_str, v);
        }
        Ok(serde_json::Value::Object(map))
    }
}

/// Determine whether a set of Lua keys represents a JSON array.
///
/// Returns `true` if all keys are positive integers forming the exact
/// contiguous set {1, 2, …, n} (order-independent).
fn is_array_keys(keys: &[mlua::Value]) -> bool {
    let mut int_keys: Vec<i64> = Vec::with_capacity(keys.len());

    for key in keys {
        match key {
            mlua::Value::Integer(i) if *i >= 1 => int_keys.push(*i),
            _ => return false, // non-integer key → object
        }
    }

    if int_keys.len() != keys.len() {
        return false;
    }

    // Sort and deduplicate to detect sparse arrays
    int_keys.sort_unstable();
    int_keys.dedup();

    // Must be exactly {1, 2, …, n}
    for (idx, k) in int_keys.iter().enumerate() {
        if *k != (idx as i64 + 1) {
            return false;
        }
    }

    true
}

/// Convert a Rust [`Event`] into a Lua table suitable for passing to callbacks.
fn event_to_lua_table(lua: &Lua, event: &Event) -> mlua::Result<mlua::Table> {
    let t = lua.create_table()?;
    t.set("event_type", event.event_type.as_str())?;
    t.set("propagation_stopped", event.propagation_stopped)?;

    // Phase
    let phase_str = match event.phase {
        EventPhase::Capture => "Capture",
        EventPhase::Bubble => "Bubble",
        EventPhase::AtTarget => "AtTarget",
    };
    t.set("phase", phase_str)?;

    // Data — populate top-level keys from the JSON value
    match &event.data {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                let lua_val = json_value_to_lua(lua, v)?;
                t.set(k.as_str(), lua_val)?;
            }
        }
        other => {
            t.set("data", format!("{}", other))?;
        }
    }

    Ok(t)
}

/// Convert a `serde_json::Value` into a `mlua::Value`.
fn json_value_to_lua(lua: &Lua, val: &serde_json::Value) -> mlua::Result<mlua::Value> {
    match val {
        serde_json::Value::Null => Ok(mlua::Value::Nil),
        serde_json::Value::Bool(b) => Ok(mlua::Value::Boolean(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(mlua::Value::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(mlua::Value::Number(f))
            } else {
                Ok(mlua::Value::Nil)
            }
        }
        serde_json::Value::String(s) => Ok(mlua::Value::String(lua.create_string(s)?)),
        serde_json::Value::Array(arr) => {
            let tbl = lua.create_table()?;
            for (i, v) in arr.iter().enumerate() {
                tbl.set(i + 1, json_value_to_lua(lua, v)?)?;
            }
            Ok(mlua::Value::Table(tbl))
        }
        serde_json::Value::Object(map) => {
            let tbl = lua.create_table()?;
            for (k, v) in map {
                tbl.set(k.as_str(), json_value_to_lua(lua, v)?)?;
            }
            Ok(mlua::Value::Table(tbl))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_load_and_call() {
        let mut runtime = LuaRuntime::new().unwrap();
        runtime
            .load_script("function add(a, b) return a + b end")
            .await
            .unwrap();

        let result = runtime.call_function("add", (2i64, 3i64)).await.unwrap();
        assert_eq!(result, mlua::Value::Integer(5));
    }

    #[tokio::test]
    async fn test_globals() {
        let runtime = LuaRuntime::new().unwrap();

        // set_global via the runtime
        {
            let lua = runtime.lua.lock().await;
            lua.globals()
                .set("test_var", lua.create_string("hello").unwrap())
                .unwrap();
        }

        let value = runtime.get_global("test_var").await.unwrap();
        // Extract the string from the mlua::Value
        match &value {
            mlua::Value::String(s) => {
                assert_eq!(s.to_str().unwrap(), "hello");
            }
            _ => panic!("Expected String, got {:?}", value),
        }
    }

    #[tokio::test]
    async fn test_eval() {
        let runtime = LuaRuntime::new().unwrap();
        let result: i64 = runtime.eval("1 + 2 * 3").await.unwrap();
        assert_eq!(result, 7);
    }

    #[tokio::test]
    async fn test_load_file_not_found() {
        let mut runtime = LuaRuntime::new().unwrap();
        let result = runtime.load_file("/nonexistent/script.lua").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_has_function() {
        let mut runtime = LuaRuntime::new().unwrap();
        runtime
            .load_script("function exists() return true end")
            .await
            .unwrap();

        assert!(runtime.has_function("exists").await);
        assert!(!runtime.has_function("nonexistent").await);
    }

    #[tokio::test]
    async fn test_is_loaded() {
        let mut runtime = LuaRuntime::new().unwrap();
        assert!(!runtime.is_loaded());
        runtime.load_script("-- empty").await.unwrap();
        assert!(runtime.is_loaded());
    }

    #[tokio::test]
    async fn test_call_function_not_found() {
        let runtime = LuaRuntime::new().unwrap();
        let result = runtime.call_function("nonexistent", ()).await;
        assert!(result.is_err());
        // Should be a Lua error (attempt to call nil)
        match result {
            Err(LuaRuntimeError::Lua(ref e)) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("attempt to call") || msg.contains("nil"),
                    "Unexpected error: {}",
                    msg
                );
            }
            other => panic!("Expected Lua error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_pokecon_api_available() {
        let runtime = LuaRuntime::new().unwrap();
        let result: String = runtime
            .eval("return type(pokecon)")
            .await
            .unwrap_or_else(|_| "nil".into());
        assert_eq!(result, "table");
    }

    #[tokio::test]
    async fn test_pokecon_log_from_lua() {
        let mut runtime = LuaRuntime::new().unwrap();
        runtime
            .load_script("pokecon.log('hello from test')")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_event_bus_integration() {
        let bus = EventBus::new();
        let mut runtime = LuaRuntime::with_event_bus(bus.clone()).unwrap();

        // Register a handler via Lua
        runtime
            .load_script(
                r#"
                pokecon.on("lua.test", function()
                    -- callback from Lua
                end)
                "#,
            )
            .await
            .unwrap();

        // The Lua-side registry should have the handler
        let cbs = runtime.api_handle().lua_callbacks.lock();
        assert!(cbs.contains_key("lua.test"));
        assert_eq!(cbs["lua.test"].len(), 1);
    }

    #[tokio::test]
    async fn test_call_function_with_timeout() {
        let mut runtime = LuaRuntime::new().unwrap();
        runtime
            .load_script("function add(a, b) return a + b end")
            .await
            .unwrap();

        // Fast call should complete within timeout
        let result = runtime
            .call_function_with_timeout("add", (2i64, 3i64), 10)
            .await
            .unwrap();
        assert_eq!(result, mlua::Value::Integer(5));

        // Non-existent function should fail immediately (Lua error, not timeout)
        let result = runtime
            .call_function_with_timeout("nonexistent", (), 10)
            .await;
        assert!(result.is_err());
        assert!(!matches!(result.unwrap_err(), LuaRuntimeError::Timeout(..)));
    }

    #[tokio::test]
    async fn test_script() {
        let mut runtime = LuaRuntime::new().unwrap();
        assert!(runtime.script().is_none());

        runtime.load_script("-- test").await.unwrap();
        assert!(runtime.script().is_some());
        assert!(runtime.script().unwrap().contains("-- test"));
    }

    #[tokio::test]
    async fn test_dispatch_event_to_lua() {
        let mut runtime = LuaRuntime::new().unwrap();

        // Load a script that registers a callback and tracks calls
        runtime
            .load_script(
                r#"
                call_count = 0
                pokecon.on("dispatch.test", function(ev)
                    call_count = call_count + 1
                end)
                "#,
            )
            .await
            .unwrap();

        // Dispatch a Rust-side event to Lua
        let event = Event::new("dispatch.test", serde_json::json!({"key": "value"}));
        let count = runtime.dispatch_event(&event).await;
        assert_eq!(count, 1);

        // Verify the Lua variable was updated
        let call_count: i64 = runtime.eval("return call_count").await.unwrap();
        assert_eq!(call_count, 1);
    }

    #[tokio::test]
    async fn test_dispatch_event_no_handler() {
        let runtime = LuaRuntime::new().unwrap();
        let event = Event::new("no.handler", serde_json::json!({}));
        let count = runtime.dispatch_event(&event).await;
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_dispatch_event_multiple_handlers() {
        let mut runtime = LuaRuntime::new().unwrap();

        runtime
            .load_script(
                r#"
                counter = 0
                pokecon.on("multi", function() counter = counter + 1 end)
                pokecon.on("multi", function() counter = counter + 1 end)
                "#,
            )
            .await
            .unwrap();

        let event = Event::new("multi", serde_json::json!({}));
        let count = runtime.dispatch_event(&event).await;
        assert_eq!(count, 2);

        let counter: i64 = runtime.eval("return counter").await.unwrap();
        assert_eq!(counter, 2);
    }

    // ── lua_value_to_json tests ─────────────────────────────────────────

    #[test]
    fn test_lua_value_to_json_nil() {
        let lua = Lua::new();
        let result = lua_value_to_json(&lua, &mlua::Value::Nil).unwrap();
        assert_eq!(result, serde_json::Value::Null);
    }

    #[test]
    fn test_lua_value_to_json_boolean() {
        let lua = Lua::new();
        assert_eq!(
            lua_value_to_json(&lua, &mlua::Value::Boolean(true)).unwrap(),
            serde_json::Value::Bool(true),
        );
        assert_eq!(
            lua_value_to_json(&lua, &mlua::Value::Boolean(false)).unwrap(),
            serde_json::Value::Bool(false),
        );
    }

    #[test]
    fn test_lua_value_to_json_integer() {
        let lua = Lua::new();
        assert_eq!(
            lua_value_to_json(&lua, &mlua::Value::Integer(42)).unwrap(),
            serde_json::json!(42),
        );
        assert_eq!(
            lua_value_to_json(&lua, &mlua::Value::Integer(-1)).unwrap(),
            serde_json::json!(-1),
        );
        assert_eq!(
            lua_value_to_json(&lua, &mlua::Value::Integer(0)).unwrap(),
            serde_json::json!(0),
        );
    }

    #[test]
    fn test_lua_value_to_json_number() {
        let lua = Lua::new();
        let result = lua_value_to_json(&lua, &mlua::Value::Number(2.5)).unwrap();
        assert_eq!(result, serde_json::json!(2.5));
    }

    #[test]
    fn test_lua_value_to_json_nan_infinity() {
        let lua = Lua::new();
        // NaN → null
        assert_eq!(
            lua_value_to_json(&lua, &mlua::Value::Number(f64::NAN)).unwrap(),
            serde_json::Value::Null,
        );
        // Infinity → null
        assert_eq!(
            lua_value_to_json(&lua, &mlua::Value::Number(f64::INFINITY)).unwrap(),
            serde_json::Value::Null,
        );
        // -Infinity → null
        assert_eq!(
            lua_value_to_json(&lua, &mlua::Value::Number(f64::NEG_INFINITY)).unwrap(),
            serde_json::Value::Null,
        );
    }

    #[test]
    fn test_lua_value_to_json_string() {
        let lua = Lua::new();
        let s = lua.create_string("hello world").unwrap();
        let result = lua_value_to_json(&lua, &mlua::Value::String(s)).unwrap();
        assert_eq!(result, serde_json::json!("hello world"));
    }

    #[test]
    fn test_lua_value_to_json_string_non_utf8() {
        let lua = Lua::new();
        let s = lua.create_string(b"test\xff").unwrap();
        let result = lua_value_to_json(&lua, &mlua::Value::String(s)).unwrap();
        assert_eq!(result, serde_json::json!("test\u{fffd}"));
    }

    #[test]
    fn test_lua_value_to_json_unsupported_types() {
        let lua = Lua::new();
        // LightUserData
        let ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let lud = mlua::LightUserData(ptr);
        assert_eq!(
            lua_value_to_json(&lua, &mlua::Value::LightUserData(lud)).unwrap(),
            serde_json::Value::Null,
        );
    }

    #[test]
    fn test_lua_value_to_json_array() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set(1, 10).unwrap();
        table.set(2, 20).unwrap();
        table.set(3, 30).unwrap();
        let result = lua_value_to_json(&lua, &mlua::Value::Table(table)).unwrap();
        assert_eq!(result, serde_json::json!([10, 20, 30]));
    }

    #[test]
    fn test_lua_value_to_json_empty_table() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        let result = lua_value_to_json(&lua, &mlua::Value::Table(table)).unwrap();
        assert_eq!(result, serde_json::json!([]));
    }

    #[test]
    fn test_lua_value_to_json_object() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("name", "Alice").unwrap();
        table.set("age", 30).unwrap();
        let result = lua_value_to_json(&lua, &mlua::Value::Table(table)).unwrap();
        assert_eq!(result, serde_json::json!({"name": "Alice", "age": 30}));
    }

    #[test]
    fn test_lua_value_to_json_mixed_array() {
        let lua = Lua::new();
        // Table with both array-style and hash-style keys → object
        let table = lua.create_table().unwrap();
        table.set(1, "a").unwrap();
        table.set(2, "b").unwrap();
        table.set("extra", "c").unwrap();
        let result = lua_value_to_json(&lua, &mlua::Value::Table(table)).unwrap();
        // Since not all keys are integer, it should be an object
        assert!(
            result.is_object(),
            "mixed table should be an object, got: {:?}",
            result,
        );
    }

    #[test]
    fn test_lua_value_to_json_sparse_array() {
        let lua = Lua::new();
        // Table with gaps in integer keys → object (not array)
        let table = lua.create_table().unwrap();
        table.set(1, "first").unwrap();
        table.set(3, "third").unwrap(); // gap at 2
        let result = lua_value_to_json(&lua, &mlua::Value::Table(table)).unwrap();
        assert!(
            result.is_object(),
            "sparse table should be an object, got: {:?}",
            result,
        );
    }

    #[test]
    fn test_lua_value_to_json_nested_tables() {
        let lua = Lua::new();
        let inner = lua.create_table().unwrap();
        inner.set("x", 1).unwrap();
        inner.set("y", 2).unwrap();

        let outer = lua.create_table().unwrap();
        outer.set("point", inner).unwrap();
        outer.set("label", "origin").unwrap();

        let result = lua_value_to_json(&lua, &mlua::Value::Table(outer)).unwrap();
        assert_eq!(
            result,
            serde_json::json!({"point": {"x": 1, "y": 2}, "label": "origin"}),
        );
    }

    #[test]
    fn test_lua_value_to_json_nested_arrays() {
        let lua = Lua::new();
        let inner = lua.create_table().unwrap();
        inner.set(1, 1).unwrap();
        inner.set(2, 2).unwrap();
        inner.set(3, 3).unwrap();

        let outer = lua.create_table().unwrap();
        outer.set(1, inner).unwrap();

        let result = lua_value_to_json(&lua, &mlua::Value::Table(outer)).unwrap();
        assert_eq!(result, serde_json::json!([[1, 2, 3]]));
    }

    #[test]
    fn test_lua_value_to_json_circular_reference() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("name", "self-ref").unwrap();
        table.set("self", table.clone()).unwrap(); // circular reference

        let result = lua_value_to_json(&lua, &mlua::Value::Table(table)).unwrap();
        // The "self" field should be null to break the cycle
        assert_eq!(
            result.get("name").and_then(|v| v.as_str()),
            Some("self-ref"),
        );
        assert_eq!(result.get("self").unwrap(), &serde_json::Value::Null);
    }

    #[test]
    fn test_lua_value_to_json_circular_nested() {
        let lua = Lua::new();
        // a → b → a (cycle)
        let a = lua.create_table().unwrap();
        let b = lua.create_table().unwrap();
        a.set("name", "a").unwrap();
        b.set("name", "b").unwrap();
        a.set("child", b.clone()).unwrap();
        b.set("parent", a.clone()).unwrap();

        let result = lua_value_to_json(&lua, &mlua::Value::Table(a)).unwrap();
        assert_eq!(result.get("name").and_then(|v| v.as_str()), Some("a"),);
        let child = result.get("child").unwrap();
        assert_eq!(child.get("name").and_then(|v| v.as_str()), Some("b"),);
        // parent should be null (circular)
        assert_eq!(child.get("parent").unwrap(), &serde_json::Value::Null,);
    }

    #[test]
    fn test_lua_value_to_json_mixed_nested_types() {
        let lua = Lua::new();
        // Test a realistic scenario: table with various types
        let table = lua.create_table().unwrap();
        table.set("null_val", mlua::Value::Nil).unwrap();
        table.set("bool_val", true).unwrap();
        table.set("int_val", 42).unwrap();
        table.set("float_val", 2.5).unwrap();
        table.set("str_val", "text").unwrap();

        let result = lua_value_to_json(&lua, &mlua::Value::Table(table)).unwrap();
        assert_eq!(result["null_val"], serde_json::Value::Null);
        assert_eq!(result["bool_val"], serde_json::json!(true));
        assert_eq!(result["int_val"], serde_json::json!(42));
        assert_eq!(result["float_val"], serde_json::json!(2.5));
        assert_eq!(result["str_val"], serde_json::json!("text"));
    }

    #[test]
    fn test_lua_value_to_json_integer_keys_out_of_order() {
        let lua = Lua::new();
        // Keys set out of order but still consecutive 1..3
        let table = lua.create_table().unwrap();
        table.set(2, "middle").unwrap();
        table.set(1, "first").unwrap();
        table.set(3, "last").unwrap();
        let result = lua_value_to_json(&lua, &mlua::Value::Table(table)).unwrap();
        assert_eq!(result, serde_json::json!(["first", "middle", "last"]));
    }

    #[test]
    fn test_lua_value_to_json_start_at_zero() {
        let lua = Lua::new();
        // Table with key 0 — should be object, not array
        let table = lua.create_table().unwrap();
        table.set(0, "zero").unwrap();
        table.set(1, "one").unwrap();
        let result = lua_value_to_json(&lua, &mlua::Value::Table(table)).unwrap();
        assert!(
            result.is_object(),
            "table with key 0 should be an object, got: {:?}",
            result,
        );
    }

    #[test]
    fn test_lua_value_to_json_duplicate_integer_keys() {
        let lua = Lua::new();
        // Duplicate key (last write wins in Lua)
        let table = lua.create_table().unwrap();
        table.set(1, "first").unwrap();
        table.set(1, "replaced").unwrap();
        table.set(2, "second").unwrap();
        let result = lua_value_to_json(&lua, &mlua::Value::Table(table)).unwrap();
        assert_eq!(result, serde_json::json!(["replaced", "second"]));
    }
}
