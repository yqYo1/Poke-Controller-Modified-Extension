use std::sync::{Arc, Weak};

use async_trait::async_trait;
use mlua::{
    Error as LuaError, ExternalError, Function, HookTriggers, Lua, LuaSerdeExt, MultiValue, Table,
    Value as LuaValue, VmState,
};
use serde_json::Value;

use crate::callback::{Callback, CallbackError, CallbackLimits, CallbackReturn, InvocationContext};
use crate::engine::{
    DeadlineCheckpoint, DynamicEngineError, EngineInner, InvocationScope, deadline_checkpoint,
};
use crate::event::{HandlerId, RegistrationOptions};

const LUA_BOOTSTRAP: &str = r##"
local api = _pokecon_api
local raw_pcall = pcall
local unpack_values = table.unpack or unpack

local CallbackSoftTimeoutError = {
    __name = "CallbackSoftTimeoutError",
}
local soft_timeout_metatable = {
    __index = CallbackSoftTimeoutError,
    __tostring = function(error)
        return string.format(
            "CallbackSoftTimeoutError(handler_id=%d, elapsed_ms=%d, soft_timeout_ms=%d)",
            error.handler_id,
            error.elapsed_ms,
            error.soft_timeout_ms
        )
    end,
}

local function normalize_soft_timeout(error)
    local fields = api.callback_soft_timeout_fields(error)
    if fields == nil then
        return error
    end
    return setmetatable(fields, soft_timeout_metatable)
end

local function pack(...)
    return {n = select("#", ...), ...}
end

function pcall(...)
    local result = pack(raw_pcall(...))
    if not result[1] then
        result[2] = normalize_soft_timeout(result[2])
    end
    return unpack_values(result, 1, result.n)
end

local function setting_namespace(prefix)
    return setmetatable({}, {
        __index = function(_, name)
            local path = prefix .. "." .. name
            local kind = api.setting_kind(path)
            if kind == "namespace" then
                return setting_namespace(path)
            end
            if kind == "value" then
                return api.get_setting(path)
            end
            error("unknown dynamic setting: " .. path, 2)
        end,
        __newindex = function(_, name, value)
            api.set_setting(prefix .. "." .. name, value)
        end,
    })
end

local state = setmetatable({}, {
    __index = function(_, name)
        return api.get_state(name)
    end,
    __newindex = function(_, name, value)
        api.set_state(name, value)
    end,
})

local function options(value)
    if type(value) == "function" then
        return {callback = value}
    end
    if type(value) ~= "table" then
        error("autocmd options must be a table or callback function", 3)
    end
    return value
end

local autocmd = {}
function autocmd.on(event, value)
    return api.register(event, options(value), false)
end
function autocmd.once(event, value)
    return api.register(event, options(value), true)
end
autocmd.off = api.off
autocmd.clear = api.clear

local event = {
    define = api.define,
    emit = api.emit,
    list_defined = api.list_defined,
}

local profile = {
    current = api.profile_current,
    list = api.profile_list,
    switch = api.profile_switch,
}

local controller = {
    update = api.controller_update,
    reset = api.controller_reset,
}

local errors = {
    CallbackSoftTimeoutError = CallbackSoftTimeoutError,
}
function errors.is_callback_soft_timeout(error)
    return (
        type(error) == "table"
        and error.__pokecon_callback_soft_timeout__ == true
    ) or api.is_callback_soft_timeout(error)
end

local function command_options()
    return {
        callback = nil,
        priority = 0,
        soft_timeout_ms = nil,
        soft_timeout_grace_ms = nil,
        hard_timeout_ms = nil,
    }
end

local commands = {
    sort = command_options(),
    tag_match = command_options(),
}
function commands.separator(label)
    if label ~= nil and type(label) ~= "string" then
        error("separator label must be string or nil", 2)
    end
    return {__pokecon_separator__ = true, label = label}
end

pokecon = {
    opt = setting_namespace("pokecon.opt"),
    state = state,
    autocmd = autocmd,
    event = event,
    errors = errors,
    profile = profile,
    controller = controller,
    commands = commands,
    source = api.source,
}

_pokecon_api = nil
"##;

#[derive(Debug, thiserror::Error)]
#[error(
    "CallbackSoftTimeoutError(handler_id={handler_id}, elapsed_ms={elapsed_ms}, soft_timeout_ms={soft_timeout_ms})"
)]
struct LuaSoftTimeoutError {
    handler_id: u64,
    elapsed_ms: u64,
    soft_timeout_ms: u64,
}

#[derive(Debug, thiserror::Error)]
#[error("callback exceeded its hard timeout")]
struct LuaHardTimeoutError;

fn lua_error(error: &DynamicEngineError) -> LuaError {
    LuaError::runtime(error.to_string())
}

fn engine_from_weak(engine: &Weak<EngineInner>) -> mlua::Result<Arc<EngineInner>> {
    engine
        .upgrade()
        .ok_or_else(|| LuaError::runtime("dynamic engine is shutting down"))
}

fn non_negative_timeout(value: Option<i64>, name: &str) -> mlua::Result<Option<u64>> {
    value
        .map(|value| {
            u64::try_from(value)
                .map_err(|_| LuaError::runtime(format!("{name} must be non-negative")))
        })
        .transpose()
}

fn registration_options(
    lua: &Lua,
    options: &Table,
) -> mlua::Result<(Function, RegistrationOptions)> {
    let callback: Function = options.get("callback")?;
    if let Ok(jit) = lua.globals().get::<Table>("jit") {
        let off: Function = jit.get("off")?;
        off.call::<()>((callback.clone(), true))?;
    }
    let priority = options.get::<Option<i64>>("priority")?.unwrap_or(0);
    let priority = i32::try_from(priority)
        .map_err(|_| LuaError::runtime("priority must fit signed 32-bit integer"))?;
    Ok((
        callback,
        RegistrationOptions {
            group: options.get("group")?,
            priority,
            limits: CallbackLimits {
                soft_timeout_ms: non_negative_timeout(
                    options.get("soft_timeout_ms")?,
                    "soft_timeout_ms",
                )?,
                soft_timeout_grace_ms: non_negative_timeout(
                    options.get("soft_timeout_grace_ms")?,
                    "soft_timeout_grace_ms",
                )?,
                hard_timeout_ms: non_negative_timeout(
                    options.get("hard_timeout_ms")?,
                    "hard_timeout_ms",
                )?,
            },
        },
    ))
}

fn install_setting_api(lua: &Lua, api: &Table, engine: &Weak<EngineInner>) -> mlua::Result<()> {
    let weak = engine.clone();
    api.set(
        "setting_kind",
        lua.create_function(move |_, path: String| {
            Ok(engine_from_weak(&weak)?.setting_kind(&path))
        })?,
    )?;

    let weak = engine.clone();
    api.set(
        "get_setting",
        lua.create_function(move |lua, path: String| {
            let value = engine_from_weak(&weak)?
                .setting(&path)
                .map_err(|error| lua_error(&error))?;
            lua.to_value(&value)
        })?,
    )?;

    let weak = engine.clone();
    api.set(
        "set_setting",
        lua.create_function(move |lua, (path, value): (String, LuaValue)| {
            let value = lua.from_value::<Value>(value)?;
            engine_from_weak(&weak)?
                .set_setting(&path, value)
                .map_err(|error| lua_error(&error))
        })?,
    )?;
    Ok(())
}

fn install_event_api(lua: &Lua, api: &Table, engine: &Weak<EngineInner>) -> mlua::Result<()> {
    let weak = engine.clone();
    let callback_lua = lua.clone();
    api.set(
        "register",
        lua.create_function(move |lua, (event, options, once): (String, Table, bool)| {
            let (callback, options) = registration_options(lua, &options)?;
            let callback: Arc<dyn Callback> = Arc::new(LuaCallback {
                lua: callback_lua.clone(),
                callback,
            });
            engine_from_weak(&weak)?
                .register(&event, callback, options, once)
                .map(HandlerId::get)
                .map_err(|error| lua_error(&error))
        })?,
    )?;

    let weak = engine.clone();
    api.set(
        "off",
        lua.create_function(move |_, handler_id: u64| {
            engine_from_weak(&weak)?
                .off(HandlerId::new(handler_id))
                .map_err(|error| lua_error(&error))
        })?,
    )?;

    let weak = engine.clone();
    api.set(
        "clear",
        lua.create_function(move |_, target: String| {
            engine_from_weak(&weak)?
                .clear(&target)
                .map_err(|error| lua_error(&error))
        })?,
    )?;

    let weak = engine.clone();
    api.set(
        "define",
        lua.create_function(move |_, event: String| {
            engine_from_weak(&weak)?
                .define(&event)
                .map_err(|error| lua_error(&error))
        })?,
    )?;

    let weak = engine.clone();
    api.set(
        "emit",
        lua.create_function(move |_, event: String| {
            engine_from_weak(&weak)?
                .emit_from_binding(&event)
                .map_err(|error| lua_error(&error))
        })?,
    )?;

    let weak = engine.clone();
    api.set(
        "list_defined",
        lua.create_function(move |_, ()| {
            engine_from_weak(&weak)?
                .list_defined()
                .map_err(|error| lua_error(&error))
        })?,
    )?;

    let weak = engine.clone();
    api.set(
        "source",
        lua.create_function(move |_, path: String| {
            engine_from_weak(&weak)?
                .source_from_binding(&path)
                .map_err(|error| lua_error(&error))
        })?,
    )?;
    Ok(())
}

fn install_host_api(lua: &Lua, api: &Table, engine: &Weak<EngineInner>) -> mlua::Result<()> {
    let weak = engine.clone();
    api.set(
        "get_state",
        lua.create_function(move |lua, name: String| {
            let value = engine_from_weak(&weak)?
                .state(&name)
                .map_err(|error| lua_error(&error))?;
            lua.to_value(&value)
        })?,
    )?;

    let weak = engine.clone();
    api.set(
        "set_state",
        lua.create_function(move |lua, (name, value): (String, LuaValue)| {
            let value = lua.from_value::<Value>(value)?;
            engine_from_weak(&weak)?
                .set_state(&name, value)
                .map_err(|error| lua_error(&error))
        })?,
    )?;

    let weak = engine.clone();
    api.set(
        "profile_current",
        lua.create_function(move |_, ()| {
            engine_from_weak(&weak)?
                .profile_current()
                .map_err(|error| lua_error(&error))
        })?,
    )?;

    let weak = engine.clone();
    api.set(
        "profile_list",
        lua.create_function(move |_, ()| {
            engine_from_weak(&weak)?
                .profile_list()
                .map_err(|error| lua_error(&error))
        })?,
    )?;

    let weak = engine.clone();
    api.set(
        "profile_switch",
        lua.create_function(move |_, name: String| {
            engine_from_weak(&weak)?
                .profile_switch(&name)
                .map_err(|error| lua_error(&error))
        })?,
    )?;

    let weak = engine.clone();
    api.set(
        "controller_update",
        lua.create_function(move |lua, value: LuaValue| {
            let value = lua.from_value::<Value>(value)?;
            engine_from_weak(&weak)?
                .controller_update(value)
                .map_err(|error| lua_error(&error))
        })?,
    )?;

    let weak = engine.clone();
    api.set(
        "controller_reset",
        lua.create_function(move |_, ()| {
            engine_from_weak(&weak)?
                .controller_reset()
                .map_err(|error| lua_error(&error))
        })?,
    )?;
    Ok(())
}

fn install_timeout_api(lua: &Lua, api: &Table) -> mlua::Result<()> {
    api.set(
        "callback_soft_timeout_fields",
        lua.create_function(|lua, value: LuaValue| {
            let LuaValue::Error(error) = value else {
                return Ok(None);
            };
            let Some(error) = lua_error_downcast::<LuaSoftTimeoutError>(&error) else {
                return Ok(None);
            };
            let fields = lua.create_table()?;
            fields.set("__pokecon_callback_soft_timeout__", true)?;
            fields.set("handler_id", error.handler_id)?;
            fields.set("elapsed_ms", error.elapsed_ms)?;
            fields.set("soft_timeout_ms", error.soft_timeout_ms)?;
            Ok(Some(fields))
        })?,
    )?;

    api.set(
        "is_callback_soft_timeout",
        lua.create_function(|_, value: LuaValue| {
            Ok(matches!(
                value,
                LuaValue::Error(ref error)
                    if error.downcast_ref::<LuaSoftTimeoutError>().is_some()
            ))
        })?,
    )?;
    Ok(())
}

fn install_api(lua: &Lua, engine: &Weak<EngineInner>) -> mlua::Result<()> {
    let api = lua.create_table()?;
    install_setting_api(lua, &api, engine)?;
    install_event_api(lua, &api, engine)?;
    install_host_api(lua, &api, engine)?;
    install_timeout_api(lua, &api)?;
    lua.globals().set("_pokecon_api", api)?;
    Ok(())
}

pub(crate) struct LuaRuntime {
    lua: Lua,
}

impl LuaRuntime {
    pub(crate) fn new(engine: &Weak<EngineInner>) -> Result<Self, DynamicEngineError> {
        let lua = Lua::new();
        lua.set_hook(HookTriggers::new().every_nth_instruction(100), |_, _| {
            match deadline_checkpoint() {
                None => Ok(VmState::Continue),
                Some(DeadlineCheckpoint::Soft {
                    handler_id,
                    elapsed_ms,
                    soft_timeout_ms,
                }) => Err(LuaSoftTimeoutError {
                    handler_id: handler_id.get(),
                    elapsed_ms,
                    soft_timeout_ms,
                }
                .into_lua_err()),
                Some(DeadlineCheckpoint::Hard) => Err(LuaHardTimeoutError.into_lua_err()),
            }
        });
        install_api(&lua, engine).map_err(|error| DynamicEngineError::Lua(error.to_string()))?;
        lua.load(LUA_BOOTSTRAP)
            .set_name("@pokecon-bootstrap")
            .exec()
            .map_err(|error| DynamicEngineError::Lua(error.to_string()))?;
        Ok(Self { lua })
    }

    pub(crate) fn evaluate(
        &self,
        source: &str,
        display_path: &str,
    ) -> Result<(), DynamicEngineError> {
        self.lua
            .load(source)
            .set_name(format!("@{display_path}"))
            .exec()
            .map_err(|error| DynamicEngineError::Lua(error.to_string()))
    }
}

struct LuaCallback {
    lua: Lua,
    callback: Function,
}

#[async_trait]
impl Callback for LuaCallback {
    async fn invoke(&self, context: InvocationContext) -> Result<CallbackReturn, CallbackError> {
        let lua = self.lua.clone();
        let callback = self.callback.clone();
        tokio::task::spawn_blocking(move || {
            let _scope = InvocationScope::enter(context.clone());
            let arguments = context
                .arguments
                .iter()
                .map(|argument| lua.to_value(argument))
                .collect::<mlua::Result<Vec<_>>>()?;
            let value = callback.call::<LuaValue>(MultiValue::from_vec(arguments))?;
            callback_return_from_lua(&lua, value)
        })
        .await
        .map_err(|error| CallbackError::internal(format!("Lua callback task failed: {error}")))?
        .map_err(|error| classify_lua_callback_error(&error))
    }
}

fn callback_return_from_lua(lua: &Lua, value: LuaValue) -> mlua::Result<CallbackReturn> {
    match value {
        LuaValue::Nil => Ok(CallbackReturn::None),
        LuaValue::Boolean(value) => Ok(CallbackReturn::Boolean(value)),
        value => Ok(CallbackReturn::Value(lua.from_value(value)?)),
    }
}

fn classify_lua_callback_error(error: &LuaError) -> CallbackError {
    if lua_error_contains::<LuaSoftTimeoutError>(error) {
        CallbackError::soft_timeout(error.to_string())
    } else if lua_error_contains::<LuaHardTimeoutError>(error) {
        CallbackError::hard_timeout("callback exceeded its hard timeout")
    } else {
        CallbackError::user(error.to_string())
    }
}

fn lua_error_contains<T: std::error::Error + Send + Sync + 'static>(error: &LuaError) -> bool {
    lua_error_downcast::<T>(error).is_some()
}

fn lua_error_downcast<T: std::error::Error + Send + Sync + 'static>(
    error: &LuaError,
) -> Option<&T> {
    error.downcast_ref::<T>().or_else(|| {
        error
            .parent()
            .and_then(|parent| lua_error_downcast::<T>(parent))
    })
}
