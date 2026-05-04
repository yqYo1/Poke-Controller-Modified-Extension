#![allow(clippy::arc_with_non_send_sync)]
//!
//! Provides a thread-safe, async-aware wrapper around the `mlua::Lua` runtime
//! with integration into the PokeCon event system.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::info;

use mlua::Lua;
use pokecon_events::{Event, EventBus, EventPhase};

use crate::api::{ApiHandle, PokeConApi};

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
        let lua = self.lua.lock().await;
        let event_name = &event.event_type;

        // Look up callback keys for this event type
        let keys = {
            let cbs = self.api_handle.lua_callbacks.lock();
            cbs.get(event_name).cloned().unwrap_or_default()
        };

        if keys.is_empty() {
            return 0;
        }

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
}
