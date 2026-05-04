use mlua::{Lua, Result as LuaResult, Table, Value};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

#[derive(Debug, thiserror::Error)]
pub enum LuaRuntimeError {
    #[error("Lua error: {0}")]
    Lua(#[from] mlua::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Script not loaded")]
    NotLoaded,
    #[error("Script execution timeout")]
    Timeout,
}

pub struct LuaRuntime {
    lua: Arc<Mutex<Lua>>,
    loaded_script: Option<String>,
}

impl LuaRuntime {
    pub fn new() -> Result<Self, LuaRuntimeError> {
        let lua = Lua::new();
        Ok(Self {
            lua: Arc::new(Mutex::new(lua)),
            loaded_script: None,
        })
    }

    pub async fn load_script(&mut self, script: &str) -> Result<(), LuaRuntimeError> {
        let lua = self.lua.lock().await;
        lua.load(script).exec()?;
        self.loaded_script = Some(script.to_string());
        info!("Lua script loaded successfully");
        Ok(())
    }

    pub async fn load_file<P: AsRef<std::path::Path>>(
        &mut self,
        path: P,
    ) -> Result<(), LuaRuntimeError> {
        let script = tokio::fs::read_to_string(path).await?;
        self.load_script(&script).await
    }

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

    pub async fn set_global(
        &self,
        name: &str,
        value: mlua::Value,
    ) -> Result<(), LuaRuntimeError> {
        let lua = self.lua.lock().await;
        lua.globals().set(name, value)?;
        Ok(())
    }

    pub async fn get_global(&self, name: &str) -> Result<mlua::Value, LuaRuntimeError> {
        let lua = self.lua.lock().await;
        let value = lua.globals().get(name)?;
        Ok(value)
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded_script.is_some()
    }

    pub fn script(&self) -> Option<&str> {
        self.loaded_script.as_deref()
    }
}

impl Default for LuaRuntime {
    fn default() -> Self {
        Self::new().expect("Failed to create Lua runtime")
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

        let result = runtime
            .call_function("add", (2i64, 3i64))
            .await
            .unwrap();

        assert_eq!(result, mlua::Value::Integer(5));
    }

    #[tokio::test]
    async fn test_globals() {
        let runtime = LuaRuntime::new().unwrap();
        let lua = runtime.lua.lock().await;
        lua.globals().set("test_var", "hello").unwrap();
        drop(lua);

        let value = runtime.get_global("test_var").await.unwrap();
        let lua = runtime.lua.lock().await;
        let s: String = lua.unpack(value).unwrap();
        assert_eq!(s, "hello");
    }
}
