use mlua::{Lua, Result as LuaResult, Table, Value};
use std::collections::HashMap;

pub struct PokeConApi;

impl PokeConApi {
    pub fn register(lua: &Lua) -> LuaResult<()> {
        let api = lua.create_table()?;

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
            "move_stick",
            lua.create_function(|_, (stick, x, y): (String, f64, f64)| {
                println!("[PokeCon] Move {} stick to ({}, {})", stick, x, y);
                Ok(())
            })?,
        )?;

        api.set(
            "log",
            lua.create_function(|_, msg: String| {
                println!("[PokeCon Lua] {}", msg);
                Ok(())
            })?,
        )?;

        lua.globals().set("pokecon", api)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::Lua;

    #[test]
    fn test_api_registration() {
        let lua = Lua::new();
        PokeConApi::register(&lua).unwrap();

        let api: Table = lua.globals().get("pokecon").unwrap();
        assert!(api.contains_key("wait").unwrap());
        assert!(api.contains_key("press_button").unwrap());
        assert!(api.contains_key("move_stick").unwrap());
        assert!(api.contains_key("log").unwrap());
    }

    #[test]
    fn test_log_function() {
        let lua = Lua::new();
        PokeConApi::register(&lua).unwrap();

        lua.load("pokecon.log('test message')")
            .exec()
            .unwrap();
    }
}
