use std::collections::HashSet;
use std::sync::{Arc, Weak};

use async_trait::async_trait;
use mlua::{
    Error as LuaError, ExternalError, Function, HookTriggers, Lua, LuaOptions, LuaSerdeExt,
    MultiValue, StdLib, Table, Value as LuaValue, VmState,
};
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::dynamic::callback::{
    Callback, CallbackError, CallbackErrorKind, CallbackLimits, CallbackReturn, InvocationContext,
};
use crate::dynamic::command::{CommandCallbackKind, CommandOptionField, CommandOptionValue};
use crate::dynamic::event::{HandlerId, RegistrationOptions};
use crate::worker_binary::dynamic::engine::{
    DeadlineCheckpoint, DynamicEngineError, EngineInner, InvocationScope, deadline_checkpoint,
};

const LUA_CALLBACK_INVOKER_REGISTRY_KEY: &str = "pokecon.callback_invoker";
const MAX_LUA_VALUE_DEPTH: usize = 64;
const MAX_LUA_VALUE_ENTRIES: usize = 65_536;
const MAX_LUA_SORT_ENTRIES: usize = 4_096;
const MAX_LUA_SERIALIZED_BYTES: usize = 1_048_576;

const LUA_BOOTSTRAP: &str = r##"
local api = _pokecon_api
local MAX_VALUE_DEPTH = api.max_value_depth
local array_metatable = api.array_metatable
local raw_pcall = pcall
local raw_require = require
local unpack_values = table.unpack or unpack

function require(name)
    if name == "Commands" or string.sub(name, 1, 9) == "Commands." then
        error("module '" .. name .. "' not found", 2)
    end
    return raw_require(name)
end

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

local BridgeError = {
    __name = "DynamicBridgeError",
}
local bridge_error_metatable = {
    __index = BridgeError,
    __tostring = function(error)
        return error.message
    end,
}

local function normalize_bridge_error(error)
    local fields = api.bridge_error_fields(error)
    if fields == nil then
        return error
    end
    return setmetatable(fields, bridge_error_metatable)
end

local function pack(...)
    return {n = select("#", ...), ...}
end

function pcall(...)
    local result = pack(raw_pcall(...))
    if not result[1] then
        result[2] = normalize_soft_timeout(result[2])
        result[2] = normalize_bridge_error(result[2])
    end
    return unpack_values(result, 1, result.n)
end

function print(...)
    local values = {}
    for index = 1, select("#", ...) do
        values[index] = tostring(select(index, ...))
    end
    api.record_output(table.concat(values, "\t"))
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

local state_context_stack = {}

local function deep_copy(value, copies, depth)
    if type(value) ~= "table" then
        return value
    end
    depth = (depth or 0) + 1
    if depth > MAX_VALUE_DEPTH then
        error("Lua state exceeds the maximum nesting depth", 2)
    end
    copies = copies or {}
    if copies[value] ~= nil then
        return copies[value]
    end
    local copied = {}
    copies[value] = copied
    for key, item in pairs(value) do
        copied[deep_copy(key, copies, depth)] = deep_copy(item, copies, depth)
    end
    local metatable = debug and debug.getmetatable(value) or getmetatable(value)
    if type(metatable) == "table" then
        return setmetatable(copied, metatable)
    end
    if metatable == false then
        return setmetatable(copied, array_metatable)
    end
    return copied
end

local function deep_equal(left, right, seen, depth)
    if rawequal(left, right) then
        return true
    end
    if type(left) ~= type(right) then
        return false
    end
    if type(left) ~= "table" then
        return false
    end
    depth = (depth or 0) + 1
    if depth > MAX_VALUE_DEPTH then
        error("Lua state exceeds the maximum nesting depth", 2)
    end
    seen = seen or {}
    if seen[left] ~= nil then
        return seen[left] == right
    end
    seen[left] = right
    for key, value in pairs(left) do
        if not deep_equal(value, right[key], seen, depth) then
            return false
        end
    end
    for key in pairs(right) do
        if left[key] == nil then
            return false
        end
    end
    return true
end

local function active_state_cache()
    return state_context_stack[#state_context_stack]
end

local function flush_state_cache(cache)
    for name, entry in pairs(cache) do
        if not deep_equal(entry.before, entry.value) then
            api.merge_state(name, entry.before, entry.value)
        end
    end
end

local function invoke_callback(callback, ...)
    local cache = {}
    state_context_stack[#state_context_stack + 1] = cache
    local result = pack(raw_pcall(callback, ...))
    state_context_stack[#state_context_stack] = nil
    if not result[1] then
        error(result[2], 0)
    end
    local flush_result = pack(raw_pcall(flush_state_cache, cache))
    if not flush_result[1] then
        error(flush_result[2], 0)
    end
    return unpack_values(result, 2, result.n)
end

_pokecon_invoke_callback = invoke_callback

local state = setmetatable({}, {
    __index = function(_, name)
        local cache = active_state_cache()
        if cache == nil then
            return api.get_state(name)
        end
        if cache[name] == nil then
            local value = api.get_state(name)
            cache[name] = {before = deep_copy(value), value = value}
        end
        return cache[name].value
    end,
    __newindex = function(_, name, value)
        local cache = active_state_cache()
        if cache == nil then
            api.set_state(name, value)
            return
        end
        if cache[name] == nil then
            cache[name] = {before = deep_copy(api.get_state(name)), value = value}
        else
            cache[name].value = value
        end
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
    DynamicBridgeError = BridgeError,
}
function errors.is_callback_soft_timeout(error)
    return (
        type(error) == "table"
        and error.__pokecon_callback_soft_timeout__ == true
    ) or api.is_callback_soft_timeout(error)
end
function errors.code(error)
    if type(error) == "table" and error.__pokecon_bridge_error__ == true then
        return error.code
    end
    return api.bridge_error_field(error, "code")
end
function errors.kind(error)
    if type(error) == "table" and error.__pokecon_bridge_error__ == true then
        return error.kind
    end
    return api.bridge_error_field(error, "kind")
end
function errors.message(error)
    if type(error) == "table" and error.__pokecon_bridge_error__ == true then
        return error.message
    end
    return api.bridge_error_field(error, "message") or tostring(error)
end

local function command_options(name)
    local callbacks = {}
    local timeout_fields = {
        soft_timeout_ms = true,
        soft_timeout_grace_ms = true,
        hard_timeout_ms = true,
    }
    return setmetatable({}, {
        __index = function(_, field)
            if field == "callback" then
                local revision = api.command_callback_revision(name)
                if revision == nil then
                    return nil
                end
                return callbacks[revision]
            end
            if field == "priority" then
                return api.command_priority(name)
            end
            if timeout_fields[field] then
                return api.command_timeout(name, field)
            end
            error("unknown command callback option: " .. tostring(field), 2)
        end,
        __newindex = function(_, field, value)
            if field == "callback" then
                if value ~= nil and type(value) ~= "function" then
                    error("command callback must be function or nil", 2)
                end
                local revision = api.set_command_callback(name, value)
                if revision ~= nil then
                    callbacks[revision] = value
                end
                return
            end
            if field == "priority" then
                if type(value) ~= "number" or value % 1 ~= 0 then
                    error("command callback priority must be an integer", 2)
                end
                api.set_command_priority(name, value)
                return
            end
            if timeout_fields[field] then
                if value ~= nil and (
                    type(value) ~= "number" or value % 1 ~= 0
                ) then
                    error(field .. " must be an integer or nil", 2)
                end
                if value ~= nil and value < 0 then
                    error(field .. " must be non-negative", 2)
                end
                api.set_command_timeout(name, field, value)
                return
            end
            error("unknown command callback option: " .. tostring(field), 2)
        end,
    })
end

local commands = {
    sort = command_options("sort"),
    tag_match = command_options("tag_match"),
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

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
struct LuaBindingError {
    code: String,
    kind: CallbackErrorKind,
    message: String,
}

impl LuaBindingError {
    fn from_engine(error: &DynamicEngineError) -> Self {
        let (code, kind) = match error {
            DynamicEngineError::Host(error) => (error.code.clone(), host_error_kind(&error.code)),
            DynamicEngineError::Source(_) => {
                ("SourceError".to_owned(), CallbackErrorKind::Internal)
            }
            DynamicEngineError::Transaction(_) => (
                "TransactionError".to_owned(),
                CallbackErrorKind::Configuration,
            ),
            DynamicEngineError::Event(_) => {
                ("EventError".to_owned(), CallbackErrorKind::Configuration)
            }
            DynamicEngineError::Command(_) => {
                ("CommandError".to_owned(), CallbackErrorKind::Configuration)
            }
            DynamicEngineError::Python(_) => ("PythonRuntime".to_owned(), CallbackErrorKind::User),
            DynamicEngineError::Lua(_) => ("LuaRuntime".to_owned(), CallbackErrorKind::User),
            DynamicEngineError::LuaBridge { code, kind, .. } => (code.clone(), *kind),
            DynamicEngineError::Evaluation(_) => {
                ("EvaluationError".to_owned(), CallbackErrorKind::User)
            }
            DynamicEngineError::NoActiveEvaluation => {
                ("NoActiveEvaluation".to_owned(), CallbackErrorKind::Internal)
            }
            DynamicEngineError::NoTokioRuntime => {
                ("NoTokioRuntime".to_owned(), CallbackErrorKind::Internal)
            }
        };
        Self {
            code,
            kind,
            message: error.to_string(),
        }
    }
}

fn host_error_kind(code: &str) -> CallbackErrorKind {
    if code.starts_with("Ipc")
        || code.ends_with("Disconnected")
        || code.ends_with("Timeout")
        || code == "StateEncodingFailed"
        || code == "UserWorkerStopFailed"
    {
        CallbackErrorKind::Internal
    } else {
        CallbackErrorKind::Configuration
    }
}

fn lua_error(error: &DynamicEngineError) -> LuaError {
    LuaBindingError::from_engine(error).into_lua_err()
}

fn map_lua_engine_error(error: &LuaError) -> DynamicEngineError {
    if let Some(error) = lua_error_downcast::<LuaBindingError>(error) {
        DynamicEngineError::LuaBridge {
            code: error.code.clone(),
            kind: error.kind,
            message: error.message.clone(),
        }
    } else {
        DynamicEngineError::Lua(error.to_string())
    }
}

#[derive(Default)]
struct LuaValueBudget {
    entries: usize,
    string_bytes: usize,
}

fn validate_lua_value(value: &LuaValue) -> mlua::Result<()> {
    let mut budget = LuaValueBudget::default();
    let mut active_tables = HashSet::new();
    validate_lua_value_inner(value, 0, &mut budget, &mut active_tables)
}

fn validate_lua_value_inner(
    value: &LuaValue,
    depth: usize,
    budget: &mut LuaValueBudget,
    active_tables: &mut HashSet<usize>,
) -> mlua::Result<()> {
    if depth > MAX_LUA_VALUE_DEPTH {
        return Err(LuaError::runtime(format!(
            "Lua value exceeds the maximum nesting depth of {MAX_LUA_VALUE_DEPTH}"
        )));
    }
    match value {
        LuaValue::Nil | LuaValue::Boolean(_) | LuaValue::Integer(_) => Ok(()),
        LuaValue::Number(number) if number.is_finite() => Ok(()),
        LuaValue::Number(_) => Err(LuaError::runtime("Lua numbers must be finite")),
        LuaValue::String(string) => {
            budget.string_bytes = budget
                .string_bytes
                .checked_add(string.as_bytes().len())
                .ok_or_else(|| LuaError::runtime("Lua value string size overflow"))?;
            if budget.string_bytes > MAX_LUA_SERIALIZED_BYTES {
                return Err(LuaError::runtime(
                    "Lua value exceeds the maximum serialized payload size",
                ));
            }
            Ok(())
        }
        LuaValue::Table(table) => {
            let pointer = table.to_pointer() as usize;
            if !active_tables.insert(pointer) {
                return Err(LuaError::runtime("cyclic Lua tables are not supported"));
            }
            let mut integer_keys = 0_usize;
            let mut string_keys = 0_usize;
            let mut largest_integer_key = 0_i64;
            for pair in table.clone().pairs::<LuaValue, LuaValue>() {
                let (key, item) = pair?;
                budget.entries = budget
                    .entries
                    .checked_add(1)
                    .ok_or_else(|| LuaError::runtime("Lua value entry count overflow"))?;
                if budget.entries > MAX_LUA_VALUE_ENTRIES {
                    return Err(LuaError::runtime(format!(
                        "Lua value exceeds the maximum entry count of {MAX_LUA_VALUE_ENTRIES}"
                    )));
                }
                match key {
                    LuaValue::String(string) => {
                        string_keys += 1;
                        budget.string_bytes = budget
                            .string_bytes
                            .checked_add(string.as_bytes().len())
                            .ok_or_else(|| LuaError::runtime("Lua table key size overflow"))?;
                    }
                    LuaValue::Integer(index) if index > 0 => {
                        integer_keys += 1;
                        largest_integer_key = largest_integer_key.max(index);
                    }
                    _ => {
                        return Err(LuaError::runtime(
                            "Lua table keys must be strings or positive contiguous integers",
                        ));
                    }
                }
                validate_lua_value_inner(&item, depth + 1, budget, active_tables)?;
            }
            active_tables.remove(&pointer);
            if budget.string_bytes > MAX_LUA_SERIALIZED_BYTES {
                return Err(LuaError::runtime(
                    "Lua value exceeds the maximum serialized payload size",
                ));
            }
            if integer_keys > 0 && string_keys > 0 {
                return Err(LuaError::runtime(
                    "Lua tables cannot mix object keys and array indices",
                ));
            }
            if integer_keys > 0 && usize::try_from(largest_integer_key).ok() != Some(integer_keys) {
                return Err(LuaError::runtime(
                    "Lua array tables must have contiguous integer indices",
                ));
            }
            Ok(())
        }
        LuaValue::Function(_)
        | LuaValue::Thread(_)
        | LuaValue::UserData(_)
        | LuaValue::LightUserData(_)
        | LuaValue::Error(_) => Err(LuaError::runtime(
            "Lua functions, threads, userdata, lightuserdata, and errors cannot cross the value boundary",
        )),
        LuaValue::Other(_) => Err(LuaError::runtime("unsupported Lua value type")),
    }
}

fn from_lua_value<T>(lua: &Lua, value: LuaValue) -> mlua::Result<T>
where
    T: DeserializeOwned,
{
    validate_lua_value(&value)?;
    let json = lua.from_value::<Value>(value)?;
    let encoded = serde_json::to_vec(&json).map_err(LuaError::external)?;
    if encoded.len() > MAX_LUA_SERIALIZED_BYTES {
        return Err(LuaError::runtime(
            "Lua value exceeds the maximum serialized payload size",
        ));
    }
    serde_json::from_slice(&encoded).map_err(LuaError::external)
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

fn command_kind(name: &str) -> mlua::Result<CommandCallbackKind> {
    CommandCallbackKind::try_from(name).map_err(|error| LuaError::runtime(error.to_string()))
}

fn command_timeout_field(name: &str) -> mlua::Result<CommandOptionField> {
    let field =
        CommandOptionField::try_from(name).map_err(|error| LuaError::runtime(error.to_string()))?;
    if field == CommandOptionField::Priority {
        Err(LuaError::runtime("priority is not a timeout field"))
    } else {
        Ok(field)
    }
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
            let value = from_lua_value::<Value>(lua, value)?;
            engine_from_weak(&weak)?
                .set_setting(&path, value)
                .map_err(|error| lua_error(&error))
        })?,
    )?;
    Ok(())
}

fn install_event_api(
    lua: &Lua,
    api: &Table,
    engine: &Weak<EngineInner>,
    access: &Arc<Mutex<()>>,
) -> mlua::Result<()> {
    let weak = engine.clone();
    let callback_lua = lua.clone();
    let callback_access = access.clone();
    api.set(
        "register",
        lua.create_function(move |lua, (event, options, once): (String, Table, bool)| {
            let (callback, options) = registration_options(lua, &options)?;
            let callback: Arc<dyn Callback> = Arc::new(LuaCallback {
                lua: callback_lua.clone(),
                access: callback_access.clone(),
                callback,
                return_mode: LuaReturnMode::Any,
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
            let value = from_lua_value::<Value>(lua, value)?;
            engine_from_weak(&weak)?
                .set_state(&name, value)
                .map_err(|error| lua_error(&error))
        })?,
    )?;

    let weak = engine.clone();
    api.set(
        "merge_state",
        lua.create_function(
            move |lua, (name, before, value): (String, LuaValue, LuaValue)| {
                let before = from_lua_value::<Value>(lua, before)?;
                let value = from_lua_value::<Value>(lua, value)?;
                engine_from_weak(&weak)?
                    .merge_state(&name, before, value)
                    .map_err(|error| lua_error(&error))
            },
        )?,
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
            let value = from_lua_value::<Value>(lua, value)?;
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

    let weak = engine.clone();
    api.set(
        "record_output",
        lua.create_function(move |_, message: String| {
            engine_from_weak(&weak)?.record_output(&message);
            Ok(())
        })?,
    )?;
    Ok(())
}

fn install_command_api(
    lua: &Lua,
    api: &Table,
    engine: &Weak<EngineInner>,
    access: &Arc<Mutex<()>>,
) -> mlua::Result<()> {
    install_command_read_api(lua, api, engine)?;
    install_command_write_api(lua, api, engine, access)
}

fn install_command_read_api(
    lua: &Lua,
    api: &Table,
    engine: &Weak<EngineInner>,
) -> mlua::Result<()> {
    let weak = engine.clone();
    api.set(
        "command_callback_revision",
        lua.create_function(move |_, name: String| {
            engine_from_weak(&weak)?
                .command_callback_revision(command_kind(&name)?)
                .map(|revision| revision.map(|revision| revision.to_string()))
                .map_err(|error| lua_error(&error))
        })?,
    )?;

    let weak = engine.clone();
    api.set(
        "command_priority",
        lua.create_function(move |_, name: String| {
            let value = engine_from_weak(&weak)?
                .command_option(command_kind(&name)?, CommandOptionField::Priority)
                .map_err(|error| lua_error(&error))?;
            let CommandOptionValue::Priority(value) = value else {
                unreachable!("priority getter returns a priority value");
            };
            Ok(value)
        })?,
    )?;

    let weak = engine.clone();
    api.set(
        "command_timeout",
        lua.create_function(move |_, (name, field): (String, String)| {
            let value = engine_from_weak(&weak)?
                .command_option(command_kind(&name)?, command_timeout_field(&field)?)
                .map_err(|error| lua_error(&error))?;
            let CommandOptionValue::Timeout(value) = value else {
                unreachable!("timeout getter returns a timeout value");
            };
            Ok(value)
        })?,
    )?;
    Ok(())
}

fn install_command_write_api(
    lua: &Lua,
    api: &Table,
    engine: &Weak<EngineInner>,
    access: &Arc<Mutex<()>>,
) -> mlua::Result<()> {
    let weak = engine.clone();
    api.set(
        "set_command_priority",
        lua.create_function(move |_, (name, value): (String, i64)| {
            let value = i32::try_from(value)
                .map_err(|_| LuaError::runtime("priority must fit signed 32-bit integer"))?;
            engine_from_weak(&weak)?
                .set_command_option(
                    command_kind(&name)?,
                    CommandOptionField::Priority,
                    CommandOptionValue::Priority(value),
                )
                .map_err(|error| lua_error(&error))
        })?,
    )?;

    let weak = engine.clone();
    api.set(
        "set_command_timeout",
        lua.create_function(
            move |_, (name, field, value): (String, String, Option<i64>)| {
                let value = non_negative_timeout(value, &field)?;
                engine_from_weak(&weak)?
                    .set_command_option(
                        command_kind(&name)?,
                        command_timeout_field(&field)?,
                        CommandOptionValue::Timeout(value),
                    )
                    .map_err(|error| lua_error(&error))
            },
        )?,
    )?;

    let weak = engine.clone();
    let callback_lua = lua.clone();
    let callback_access = access.clone();
    api.set(
        "set_command_callback",
        lua.create_function(move |lua, (name, value): (String, LuaValue)| {
            let kind = command_kind(&name)?;
            let callback: Option<Arc<dyn Callback>> = match value {
                LuaValue::Nil => None,
                LuaValue::Function(callback) => {
                    if let Ok(jit) = lua.globals().get::<Table>("jit") {
                        let off: Function = jit.get("off")?;
                        off.call::<()>((callback.clone(), true))?;
                    }
                    Some(Arc::new(LuaCallback {
                        lua: callback_lua.clone(),
                        access: callback_access.clone(),
                        callback,
                        return_mode: match kind {
                            CommandCallbackKind::Sort => LuaReturnMode::SortList,
                            CommandCallbackKind::TagMatch => LuaReturnMode::Any,
                        },
                    }))
                }
                _ => {
                    return Err(LuaError::runtime(
                        "command callback must be function or nil",
                    ));
                }
            };
            engine_from_weak(&weak)?
                .set_command_callback(kind, callback)
                .map(|revision| revision.map(|revision| revision.to_string()))
                .map_err(|error| lua_error(&error))
        })?,
    )?;
    Ok(())
}

fn callback_error_kind_name(kind: CallbackErrorKind) -> &'static str {
    match kind {
        CallbackErrorKind::User => "user",
        CallbackErrorKind::SoftTimeout => "soft_timeout",
        CallbackErrorKind::HardTimeout => "hard_timeout",
        CallbackErrorKind::Configuration => "configuration",
        CallbackErrorKind::Disconnected => "disconnected",
        CallbackErrorKind::Internal => "internal",
    }
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

    api.set(
        "bridge_error_fields",
        lua.create_function(|lua, value: LuaValue| {
            let LuaValue::Error(error) = value else {
                return Ok(None);
            };
            let Some(error) = lua_error_downcast::<LuaBindingError>(&error) else {
                return Ok(None);
            };
            let fields = lua.create_table()?;
            fields.set("__pokecon_bridge_error__", true)?;
            fields.set("code", error.code.clone())?;
            fields.set("kind", callback_error_kind_name(error.kind))?;
            fields.set("message", error.message.clone())?;
            Ok(Some(fields))
        })?,
    )?;

    api.set(
        "bridge_error_field",
        lua.create_function(|_, (value, field): (LuaValue, String)| {
            let LuaValue::Error(error) = value else {
                return Ok(None);
            };
            let Some(error) = lua_error_downcast::<LuaBindingError>(&error) else {
                return Ok(None);
            };
            let value = match field.as_str() {
                "code" => Some(error.code.clone()),
                "kind" => Some(callback_error_kind_name(error.kind).to_owned()),
                "message" => Some(error.message.clone()),
                _ => None,
            };
            Ok(value)
        })?,
    )?;
    Ok(())
}
fn install_api(lua: &Lua, engine: &Weak<EngineInner>, access: &Arc<Mutex<()>>) -> mlua::Result<()> {
    let api = lua.create_table()?;
    api.set("max_value_depth", MAX_LUA_VALUE_DEPTH)?;
    api.set("array_metatable", lua.array_metatable())?;
    install_setting_api(lua, &api, engine)?;
    install_event_api(lua, &api, engine, access)?;
    install_host_api(lua, &api, engine)?;
    install_command_api(lua, &api, engine, access)?;
    install_timeout_api(lua, &api)?;
    lua.globals().set("_pokecon_api", api)?;
    Ok(())
}

pub(crate) struct LuaRuntime {
    lua: Lua,
    access: Arc<Mutex<()>>,
}

impl LuaRuntime {
    pub(crate) fn new(engine: &Weak<EngineInner>) -> Result<Self, DynamicEngineError> {
        let access = Arc::new(Mutex::new(()));
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::BIT,
            LuaOptions::default(),
        )
        .map_err(|error| map_lua_engine_error(&error))?;
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
        install_api(&lua, engine, &access).map_err(|error| map_lua_engine_error(&error))?;
        lua.load(LUA_BOOTSTRAP)
            .set_name("@pokecon-bootstrap")
            .exec()
            .map_err(|error| map_lua_engine_error(&error))?;
        let callback_invoker = lua
            .globals()
            .get::<Function>("_pokecon_invoke_callback")
            .map_err(|error| map_lua_engine_error(&error))?;
        lua.set_named_registry_value(LUA_CALLBACK_INVOKER_REGISTRY_KEY, callback_invoker)
            .map_err(|error| map_lua_engine_error(&error))?;
        lua.globals()
            .set("_pokecon_invoke_callback", LuaValue::Nil)
            .map_err(|error| map_lua_engine_error(&error))?;
        Ok(Self { lua, access })
    }

    pub(crate) fn evaluate(
        &self,
        source: &str,
        display_path: &str,
    ) -> Result<(), DynamicEngineError> {
        let _access = self.access.lock();
        self.lua
            .load(source)
            .set_name(format!("@{display_path}"))
            .exec()
            .map_err(|error| map_lua_engine_error(&error))
    }
}

#[derive(Clone, Copy)]
enum LuaReturnMode {
    Any,
    SortList,
}

struct LuaCallback {
    lua: Lua,
    access: Arc<Mutex<()>>,
    callback: Function,
    return_mode: LuaReturnMode,
}

#[async_trait]
impl Callback for LuaCallback {
    async fn invoke(&self, context: InvocationContext) -> Result<CallbackReturn, CallbackError> {
        let lua = self.lua.clone();
        let access = self.access.clone();
        let callback = self.callback.clone();
        let return_mode = self.return_mode;
        tokio::task::spawn_blocking(move || {
            let _access = access.lock();
            let _scope = InvocationScope::enter(context.clone());
            let arguments = context
                .arguments
                .iter()
                .map(|argument| lua.to_value(argument))
                .collect::<mlua::Result<Vec<_>>>()
                .map_err(|error| {
                    CallbackError::internal(format!(
                        "Lua callback argument serialization failed: {error}"
                    ))
                })?;
            let mut invocation = Vec::with_capacity(arguments.len() + 1);
            invocation.push(LuaValue::Function(callback));
            invocation.extend(arguments);
            let callback_invoker: Function = lua
                .named_registry_value(LUA_CALLBACK_INVOKER_REGISTRY_KEY)
                .map_err(|error| {
                    CallbackError::internal(format!("Lua callback invoker lookup failed: {error}"))
                })?;
            let value = callback_invoker
                .call::<LuaValue>(MultiValue::from_vec(invocation))
                .map_err(|error| classify_lua_callback_error(&error))?;
            match return_mode {
                LuaReturnMode::Any => callback_return_from_lua(&lua, value)
                    .map_err(|error| classify_lua_return_error(&error)),
                LuaReturnMode::SortList => command_sort_return_from_lua(&lua, value)
                    .map_err(|error| classify_lua_return_error(&error)),
            }
        })
        .await
        .map_err(|error| CallbackError::internal(format!("Lua callback task failed: {error}")))?
    }
}

fn command_sort_return_from_lua(lua: &Lua, value: LuaValue) -> mlua::Result<CallbackReturn> {
    let LuaValue::Table(table) = value else {
        return Err(LuaError::runtime(
            "command sort callback must return an array table",
        ));
    };
    let length = table.raw_len();
    if length > MAX_LUA_SORT_ENTRIES {
        return Err(LuaError::runtime(format!(
            "command sort callback returned more than the maximum of {MAX_LUA_SORT_ENTRIES} entries"
        )));
    }
    validate_lua_value(&LuaValue::Table(table.clone()))?;
    for pair in table.clone().pairs::<LuaValue, LuaValue>() {
        let (key, _value) = pair?;
        let index = match key {
            LuaValue::Integer(index) => usize::try_from(index).ok(),
            _ => None,
        };
        if !index.is_some_and(|index| (1..=length).contains(&index)) {
            return Err(LuaError::runtime(
                "command sort callback must return a contiguous array table",
            ));
        }
    }
    let mut values = Vec::with_capacity(length);
    for index in 1..=length {
        let value = table.raw_get::<LuaValue>(index)?;
        if matches!(value, LuaValue::Nil) {
            return Err(LuaError::runtime(
                "command sort callback must return a contiguous array table",
            ));
        }
        values.push(from_lua_value(lua, value)?);
    }
    Ok(CallbackReturn::Value(Value::Array(values)))
}

fn callback_return_from_lua(lua: &Lua, value: LuaValue) -> mlua::Result<CallbackReturn> {
    match value {
        LuaValue::Nil => Ok(CallbackReturn::None),
        LuaValue::Boolean(value) => Ok(CallbackReturn::Boolean(value)),
        value => Ok(CallbackReturn::Value(from_lua_value(lua, value)?)),
    }
}

fn classify_lua_return_error(error: &LuaError) -> CallbackError {
    if lua_error_contains::<LuaSoftTimeoutError>(error)
        || lua_error_contains::<LuaHardTimeoutError>(error)
        || lua_error_contains::<LuaBindingError>(error)
    {
        classify_lua_callback_error(error)
    } else {
        CallbackError::user(format!("unsupported Lua callback return value: {error}"))
    }
}

fn classify_lua_callback_error(error: &LuaError) -> CallbackError {
    if lua_error_contains::<LuaSoftTimeoutError>(error) {
        CallbackError::soft_timeout(error.to_string())
    } else if lua_error_contains::<LuaHardTimeoutError>(error) {
        CallbackError::hard_timeout("callback exceeded its hard timeout")
    } else if let Some(error) = lua_error_downcast::<LuaBindingError>(error) {
        CallbackError {
            kind: error.kind,
            message: error.message.clone(),
        }
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

#[cfg(test)]
mod tests {
    use mlua::{UserData, UserDataMethods};

    use super::{
        Lua, LuaError, LuaValue, MAX_LUA_SERIALIZED_BYTES, MAX_LUA_SORT_ENTRIES,
        command_sort_return_from_lua, validate_lua_value,
    };

    struct TestUserData;

    impl UserData for TestUserData {
        fn add_methods<M: UserDataMethods<Self>>(_methods: &mut M) {}
    }

    #[test]
    fn lua_value_boundary_rejects_non_json_values_and_cycles() {
        let lua = Lua::new();
        let function = lua.create_function(|_, ()| Ok(())).unwrap();
        assert!(validate_lua_value(&LuaValue::Function(function)).is_err());
        let userdata = lua.create_userdata(TestUserData).unwrap();
        assert!(validate_lua_value(&LuaValue::UserData(userdata)).is_err());
        assert!(validate_lua_value(&LuaValue::Number(f64::NAN)).is_err());
        assert!(validate_lua_value(&LuaValue::Number(f64::INFINITY)).is_err());
        assert!(
            validate_lua_value(&LuaValue::Error(Box::new(LuaError::runtime("error")))).is_err()
        );

        let table = lua.create_table().unwrap();
        table.set("self", table.clone()).unwrap();
        assert!(validate_lua_value(&LuaValue::Table(table)).is_err());
    }

    #[test]
    fn lua_value_boundary_rejects_depth_and_payload_overflow() {
        let lua = Lua::new();
        let root = lua.create_table().unwrap();
        let mut cursor = root.clone();
        for _ in 0..65 {
            let child = lua.create_table().unwrap();
            cursor.set(1, child.clone()).unwrap();
            cursor = child;
        }
        assert!(validate_lua_value(&LuaValue::Table(root)).is_err());

        let string = lua
            .create_string(vec![b'x'; MAX_LUA_SERIALIZED_BYTES + 1])
            .unwrap();
        assert!(validate_lua_value(&LuaValue::String(string)).is_err());
    }

    #[test]
    fn lua_sort_boundary_rejects_oversized_arrays_before_allocation() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        for index in 1..=MAX_LUA_SORT_ENTRIES + 1 {
            table.set(index, index).unwrap();
        }
        assert!(command_sort_return_from_lua(&lua, LuaValue::Table(table)).is_err());
    }
}
