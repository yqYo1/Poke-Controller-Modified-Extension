use std::ffi::CString;
use std::sync::{Arc, Weak};

use async_trait::async_trait;
use pyo3::exceptions::{PyOverflowError, PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{
    PyAny, PyBool, PyCode, PyCodeInput, PyCodeMethods, PyDict, PyDictMethods, PyFloat, PyInt,
    PyList, PyListMethods, PyModule, PyModuleMethods, PyString, PyTuple,
};
use serde_json::{Map, Number, Value};

use crate::callback::CallbackLimits;
use crate::callback::{Callback, CallbackError, CallbackReturn, InvocationContext};
use crate::command::{CommandCallbackKind, CommandOptionField, CommandOptionValue};
use crate::engine::{
    DeadlineCheckpoint, DynamicEngineError, EngineInner, InvocationScope, deadline_checkpoint,
};
use crate::event::{HandlerId, RegistrationOptions};

const PYTHON_BOOTSTRAP: &str = r#"
import sys as _sys


class _BlockCommandsImport:
    @staticmethod
    def find_spec(fullname, _path, _target=None):
        if fullname == "Commands" or fullname.startswith("Commands."):
            raise ModuleNotFoundError(f"No module named '{fullname}'", name=fullname)
        return None


_sys.meta_path.insert(0, _BlockCommandsImport())


class CallbackSoftTimeoutError(TimeoutError):
    def __init__(self, handler_id, elapsed_ms, soft_timeout_ms):
        self.handler_id = handler_id
        self.elapsed_ms = elapsed_ms
        self.soft_timeout_ms = soft_timeout_ms
        super().__init__(
            f"callback {handler_id} exceeded soft timeout {soft_timeout_ms}ms "
            f"after {elapsed_ms}ms"
        )


class _CallbackHardTimeoutError(BaseException):
    pass


class _PokeconStdout:
    def __init__(self):
        self._buffer = ""

    def write(self, text):
        text = str(text)
        self._buffer += text
        while "\n" in self._buffer:
            line, self._buffer = self._buffer.split("\n", 1)
            _api.record_output(line)
        return len(text)

    def flush(self):
        if self._buffer:
            _api.record_output(self._buffer)
            self._buffer = ""

    def isatty(self):
        return False


def _deadline_monitor(_code, _offset):
    checkpoint = _api.deadline_checkpoint()
    if checkpoint is None:
        return
    kind, handler_id, elapsed_ms, soft_timeout_ms = checkpoint
    if kind == "soft":
        raise CallbackSoftTimeoutError(handler_id, elapsed_ms, soft_timeout_ms)
    raise _CallbackHardTimeoutError()


_tool_id = _sys.monitoring.OPTIMIZER_ID
try:
    _sys.monitoring.free_tool_id(_tool_id)
except ValueError:
    pass
_sys.monitoring.use_tool_id(_tool_id, "pokecon.callback.deadline")
_sys.monitoring.register_callback(
    _tool_id, _sys.monitoring.events.INSTRUCTION, _deadline_monitor
)
_sys.monitoring.set_events(_tool_id, _sys.monitoring.events.INSTRUCTION)


class _Errors:
    CallbackSoftTimeoutError = CallbackSoftTimeoutError

    @staticmethod
    def is_callback_soft_timeout(error):
        return isinstance(error, CallbackSoftTimeoutError)


class _SettingNamespace:
    __slots__ = ("_prefix",)

    def __init__(self, prefix):
        object.__setattr__(self, "_prefix", prefix)

    def __getattr__(self, name):
        path = f"{self._prefix}.{name}"
        kind = _api.setting_kind(path)
        if kind == "namespace":
            return _SettingNamespace(path)
        if kind == "value":
            return _api.get_setting(path)
        raise AttributeError(path)

    def __setattr__(self, name, value):
        if name == "_prefix":
            object.__setattr__(self, name, value)
            return
        _api.set_setting(f"{self._prefix}.{name}", value)


class _State:
    def __getattr__(self, name):
        return _api.get_state(name)

    def __setattr__(self, name, value):
        _api.set_state(name, value)


class _Autocmd:
    @staticmethod
    def on(
        event,
        *,
        callback,
        group=None,
        priority=0,
        soft_timeout_ms=None,
        soft_timeout_grace_ms=None,
        hard_timeout_ms=None,
    ):
        return _api.register(
            event,
            callback,
            group,
            priority,
            soft_timeout_ms,
            soft_timeout_grace_ms,
            hard_timeout_ms,
            False,
        )

    @staticmethod
    def once(
        event,
        *,
        callback,
        group=None,
        priority=0,
        soft_timeout_ms=None,
        soft_timeout_grace_ms=None,
        hard_timeout_ms=None,
    ):
        return _api.register(
            event,
            callback,
            group,
            priority,
            soft_timeout_ms,
            soft_timeout_grace_ms,
            hard_timeout_ms,
            True,
        )

    off = staticmethod(_api.off)
    clear = staticmethod(_api.clear)


class _Event:
    define = staticmethod(_api.define)
    emit = staticmethod(_api.emit)
    list_defined = staticmethod(_api.list_defined)


class _Profile:
    current = staticmethod(_api.profile_current)
    list = staticmethod(_api.profile_list)
    switch = staticmethod(_api.profile_switch)


class _Controller:
    update = staticmethod(_api.controller_update)
    reset = staticmethod(_api.controller_reset)


class CommandSeparator:
    __slots__ = ("label", "__pokecon_separator__")

    def __init__(self, label=None):
        if label is not None and not isinstance(label, str):
            raise TypeError("separator label must be str or None")
        self.label = label
        self.__pokecon_separator__ = True


class _CommandOptions:
    __slots__ = ("_name", "_callbacks")

    def __init__(self, name):
        object.__setattr__(self, "_name", name)
        object.__setattr__(self, "_callbacks", {})

    @property
    def callback(self):
        revision = _api.command_callback_revision(self._name)
        if revision is None:
            return None
        return self._callbacks.get(revision)

    @callback.setter
    def callback(self, value):
        if value is not None and not callable(value):
            raise TypeError("command callback must be callable or None")
        revision = _api.set_command_callback(self._name, value)
        if revision is not None:
            self._callbacks[revision] = value

    @property
    def priority(self):
        return _api.command_priority(self._name)

    @priority.setter
    def priority(self, value):
        if not isinstance(value, int) or isinstance(value, bool):
            raise TypeError("command callback priority must be int")
        _api.set_command_priority(self._name, value)

    def _get_timeout(self, field):
        return _api.command_timeout(self._name, field)

    def _set_timeout(self, field, value):
        if value is not None and (
            not isinstance(value, int) or isinstance(value, bool)
        ):
            raise TypeError(f"{field} must be int or None")
        if value is not None and value < 0:
            raise ValueError(f"{field} must be non-negative")
        _api.set_command_timeout(self._name, field, value)

    soft_timeout_ms = property(
        lambda self: self._get_timeout("soft_timeout_ms"),
        lambda self, value: self._set_timeout("soft_timeout_ms", value),
    )
    soft_timeout_grace_ms = property(
        lambda self: self._get_timeout("soft_timeout_grace_ms"),
        lambda self, value: self._set_timeout("soft_timeout_grace_ms", value),
    )
    hard_timeout_ms = property(
        lambda self: self._get_timeout("hard_timeout_ms"),
        lambda self, value: self._set_timeout("hard_timeout_ms", value),
    )


class _Commands:
    sort = _CommandOptions("sort")
    tag_match = _CommandOptions("tag_match")

    @staticmethod
    def separator(label=None):
        return CommandSeparator(label)


opt = _SettingNamespace("pokecon.opt")
state = _State()
autocmd = _Autocmd()
event = _Event()
errors = _Errors()
profile = _Profile()
controller = _Controller()
commands = _Commands()
source = _api.source
_sys.stdout = _PokeconStdout()
"#;

fn python_error(error: &DynamicEngineError) -> PyErr {
    PyRuntimeError::new_err(error.to_string())
}

fn engine_from_weak(engine: &Weak<EngineInner>) -> PyResult<Arc<EngineInner>> {
    engine
        .upgrade()
        .ok_or_else(|| PyRuntimeError::new_err("dynamic engine is shutting down"))
}

fn non_negative_timeout(value: Option<i64>, name: &str) -> PyResult<Option<u64>> {
    value
        .map(|value| {
            u64::try_from(value)
                .map_err(|_| PyValueError::new_err(format!("{name} must be non-negative")))
        })
        .transpose()
}

fn command_kind(name: &str) -> PyResult<CommandCallbackKind> {
    CommandCallbackKind::try_from(name).map_err(|error| PyValueError::new_err(error.to_string()))
}

fn command_timeout_field(name: &str) -> PyResult<CommandOptionField> {
    let field = CommandOptionField::try_from(name)
        .map_err(|error| PyValueError::new_err(error.to_string()))?;
    if field == CommandOptionField::Priority {
        Err(PyValueError::new_err("priority is not a timeout field"))
    } else {
        Ok(field)
    }
}

#[pyclass(frozen)]
struct PyApi {
    engine: Weak<EngineInner>,
}

#[pymethods]
impl PyApi {
    fn setting_kind(&self, path: &str) -> PyResult<Option<&'static str>> {
        Ok(engine_from_weak(&self.engine)?.setting_kind(path))
    }

    fn get_setting(&self, py: Python<'_>, path: &str) -> PyResult<Py<PyAny>> {
        let value = engine_from_weak(&self.engine)?
            .setting(path)
            .map_err(|error| python_error(&error))?;
        json_to_python(py, &value)
    }

    fn set_setting(&self, path: &str, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let engine = engine_from_weak(&self.engine)?;
        engine
            .set_setting(path, python_to_json(value)?)
            .map_err(|error| python_error(&error))
    }

    #[pyo3(signature = (
        event,
        callback,
        group,
        priority,
        soft_timeout_ms,
        soft_timeout_grace_ms,
        hard_timeout_ms,
        once
    ))]
    #[allow(clippy::too_many_arguments)]
    fn register(
        &self,
        py: Python<'_>,
        event: &str,
        callback: Py<PyAny>,
        group: Option<String>,
        priority: i64,
        soft_timeout_ms: Option<i64>,
        soft_timeout_grace_ms: Option<i64>,
        hard_timeout_ms: Option<i64>,
        once: bool,
    ) -> PyResult<u64> {
        if !callback.bind(py).is_callable() {
            return Err(PyTypeError::new_err("callback must be callable"));
        }
        let priority = i32::try_from(priority)
            .map_err(|_| PyOverflowError::new_err("priority must fit signed 32-bit integer"))?;
        let options = RegistrationOptions {
            group,
            priority,
            limits: CallbackLimits {
                soft_timeout_ms: non_negative_timeout(soft_timeout_ms, "soft_timeout_ms")?,
                soft_timeout_grace_ms: non_negative_timeout(
                    soft_timeout_grace_ms,
                    "soft_timeout_grace_ms",
                )?,
                hard_timeout_ms: non_negative_timeout(hard_timeout_ms, "hard_timeout_ms")?,
            },
        };
        let engine = engine_from_weak(&self.engine)?;
        let callback: Arc<dyn Callback> = Arc::new(PythonCallback {
            callback,
            return_mode: PythonReturnMode::Any,
        });
        engine
            .register(event, callback, options, once)
            .map(HandlerId::get)
            .map_err(|error| python_error(&error))
    }

    fn off(&self, handler_id: u64) -> PyResult<()> {
        engine_from_weak(&self.engine)?
            .off(HandlerId::new(handler_id))
            .map_err(|error| python_error(&error))
    }

    fn clear(&self, target: &str) -> PyResult<()> {
        engine_from_weak(&self.engine)?
            .clear(target)
            .map_err(|error| python_error(&error))
    }

    fn define(&self, event: &str) -> PyResult<()> {
        engine_from_weak(&self.engine)?
            .define(event)
            .map_err(|error| python_error(&error))
    }

    fn emit(&self, event: &str) -> PyResult<()> {
        engine_from_weak(&self.engine)?
            .emit_from_binding(event)
            .map_err(|error| python_error(&error))
    }

    fn list_defined(&self) -> PyResult<Vec<String>> {
        engine_from_weak(&self.engine)?
            .list_defined()
            .map_err(|error| python_error(&error))
    }

    fn source(&self, path: &str) -> PyResult<()> {
        engine_from_weak(&self.engine)?
            .source_from_binding(path)
            .map_err(|error| python_error(&error))
    }

    fn get_state(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
        let value = engine_from_weak(&self.engine)?
            .state(name)
            .map_err(|error| python_error(&error))?;
        json_to_python(py, &value)
    }

    fn set_state(&self, name: &str, value: &Bound<'_, PyAny>) -> PyResult<()> {
        engine_from_weak(&self.engine)?
            .set_state(name, python_to_json(value)?)
            .map_err(|error| python_error(&error))
    }

    fn profile_current(&self) -> PyResult<String> {
        engine_from_weak(&self.engine)?
            .profile_current()
            .map_err(|error| python_error(&error))
    }

    fn profile_list(&self) -> PyResult<Vec<String>> {
        engine_from_weak(&self.engine)?
            .profile_list()
            .map_err(|error| python_error(&error))
    }

    fn profile_switch(&self, name: &str) -> PyResult<bool> {
        engine_from_weak(&self.engine)?
            .profile_switch(name)
            .map_err(|error| python_error(&error))
    }

    fn controller_update(&self, value: &Bound<'_, PyAny>) -> PyResult<()> {
        engine_from_weak(&self.engine)?
            .controller_update(python_to_json(value)?)
            .map_err(|error| python_error(&error))
    }

    fn controller_reset(&self) -> PyResult<()> {
        engine_from_weak(&self.engine)?
            .controller_reset()
            .map_err(|error| python_error(&error))
    }

    fn command_callback_revision(&self, name: &str) -> PyResult<Option<u64>> {
        engine_from_weak(&self.engine)?
            .command_callback_revision(command_kind(name)?)
            .map_err(|error| python_error(&error))
    }

    fn command_priority(&self, name: &str) -> PyResult<i32> {
        let value = engine_from_weak(&self.engine)?
            .command_option(command_kind(name)?, CommandOptionField::Priority)
            .map_err(|error| python_error(&error))?;
        let CommandOptionValue::Priority(value) = value else {
            unreachable!("priority getter returns a priority value");
        };
        Ok(value)
    }

    fn command_timeout(&self, name: &str, field: &str) -> PyResult<Option<u64>> {
        let value = engine_from_weak(&self.engine)?
            .command_option(command_kind(name)?, command_timeout_field(field)?)
            .map_err(|error| python_error(&error))?;
        let CommandOptionValue::Timeout(value) = value else {
            unreachable!("timeout getter returns a timeout value");
        };
        Ok(value)
    }

    fn set_command_priority(&self, name: &str, value: i64) -> PyResult<()> {
        let value = i32::try_from(value)
            .map_err(|_| PyOverflowError::new_err("priority must fit signed 32-bit integer"))?;
        engine_from_weak(&self.engine)?
            .set_command_option(
                command_kind(name)?,
                CommandOptionField::Priority,
                CommandOptionValue::Priority(value),
            )
            .map_err(|error| python_error(&error))
    }

    fn set_command_timeout(&self, name: &str, field: &str, value: Option<i64>) -> PyResult<()> {
        let value = non_negative_timeout(value, field)?;
        engine_from_weak(&self.engine)?
            .set_command_option(
                command_kind(name)?,
                command_timeout_field(field)?,
                CommandOptionValue::Timeout(value),
            )
            .map_err(|error| python_error(&error))
    }

    fn set_command_callback(
        &self,
        py: Python<'_>,
        name: &str,
        callback: &Bound<'_, PyAny>,
    ) -> PyResult<Option<u64>> {
        let kind = command_kind(name)?;
        let callback: Option<Arc<dyn Callback>> = if callback.is_none() {
            None
        } else {
            if !callback.is_callable() {
                return Err(PyTypeError::new_err(
                    "command callback must be callable or None",
                ));
            }
            Some(Arc::new(PythonCallback {
                callback: callback.clone().unbind().clone_ref(py),
                return_mode: match kind {
                    CommandCallbackKind::Sort => PythonReturnMode::SortList,
                    CommandCallbackKind::TagMatch => PythonReturnMode::Any,
                },
            }))
        };
        engine_from_weak(&self.engine)?
            .set_command_callback(kind, callback)
            .map_err(|error| python_error(&error))
    }

    fn deadline_checkpoint(&self) -> Option<(&'static str, u64, u64, u64)> {
        let _engine = engine_from_weak(&self.engine).ok()?;
        match deadline_checkpoint()? {
            DeadlineCheckpoint::Soft {
                handler_id,
                elapsed_ms,
                soft_timeout_ms,
            } => Some(("soft", handler_id.get(), elapsed_ms, soft_timeout_ms)),
            DeadlineCheckpoint::Hard => Some(("hard", 0, 0, 0)),
        }
    }

    fn record_output(&self, message: &str) {
        if let Some(engine) = self.engine.upgrade() {
            engine.record_output(message);
        }
    }
}

pub(crate) struct PythonRuntime {
    module_name: &'static str,
}

impl PythonRuntime {
    pub(crate) fn new(engine: Weak<EngineInner>) -> Result<Self, DynamicEngineError> {
        let module_name = "pokecon";
        Python::initialize();
        Python::attach(|py| -> PyResult<()> {
            let module = PyModule::new(py, module_name)?;
            module.add("_api", Py::new(py, PyApi { engine })?)?;
            let bootstrap = CString::new(PYTHON_BOOTSTRAP)
                .map_err(|_| PyValueError::new_err("Python bootstrap contains NUL"))?;
            py.run(
                bootstrap.as_c_str(),
                Some(&module.dict()),
                Some(&module.dict()),
            )?;
            let modules = py.import("sys")?.getattr("modules")?;
            modules.set_item(module_name, module)?;
            Ok(())
        })
        .map_err(|error| DynamicEngineError::Python(error.to_string()))?;
        Ok(Self { module_name })
    }

    pub(crate) fn evaluate(
        &self,
        source: &str,
        display_path: &str,
    ) -> Result<(), DynamicEngineError> {
        let source = CString::new(source)
            .map_err(|_| DynamicEngineError::Python("source contains NUL".to_owned()))?;
        let display_path = CString::new(display_path)
            .map_err(|_| DynamicEngineError::Python("source path contains NUL".to_owned()))?;
        Python::attach(|py| -> PyResult<()> {
            let globals = PyDict::new(py);
            globals.set_item("__name__", "__pokecon_dynamic__")?;
            globals.set_item("__file__", display_path.to_string_lossy().as_ref())?;
            globals.set_item(self.module_name, py.import(self.module_name)?)?;
            PyCode::compile(py, &source, &display_path, PyCodeInput::File)?
                .run(Some(&globals), Some(&globals))?;
            Ok(())
        })
        .map_err(|error| DynamicEngineError::Python(error.to_string()))
    }
}

#[derive(Clone, Copy)]
enum PythonReturnMode {
    Any,
    SortList,
}

struct PythonCallback {
    callback: Py<PyAny>,
    return_mode: PythonReturnMode,
}

#[async_trait]
impl Callback for PythonCallback {
    async fn invoke(&self, context: InvocationContext) -> Result<CallbackReturn, CallbackError> {
        let callback = Python::attach(|py| self.callback.clone_ref(py));
        let return_mode = self.return_mode;
        tokio::task::spawn_blocking(move || {
            Python::attach(|py| -> PyResult<CallbackReturn> {
                let _scope = InvocationScope::enter(context.clone());
                let arguments = context
                    .arguments
                    .iter()
                    .map(|argument| json_to_python(py, argument))
                    .collect::<PyResult<Vec<_>>>()?;
                let arguments = PyTuple::new(py, arguments)?;
                let value = callback.bind(py).call(arguments, None)?;
                if matches!(return_mode, PythonReturnMode::SortList)
                    && !value.is_instance_of::<PyList>()
                {
                    return Err(PyTypeError::new_err(
                        "command sort callback must return list",
                    ));
                }
                callback_return_from_python(&value)
            })
            .map_err(|error| classify_python_callback_error(&error))
        })
        .await
        .map_err(|error| CallbackError::internal(format!("Python callback task failed: {error}")))?
    }
}

fn classify_python_callback_error(error: &PyErr) -> CallbackError {
    Python::attach(|py| {
        if let Ok(module) = py.import("pokecon") {
            if let Ok(soft) = module.getattr("CallbackSoftTimeoutError")
                && error.is_instance(py, &soft)
            {
                return CallbackError::soft_timeout(error.to_string());
            }
            if let Ok(hard) = module.getattr("_CallbackHardTimeoutError")
                && error.is_instance(py, &hard)
            {
                return CallbackError::hard_timeout("callback exceeded its hard timeout");
            }
        }
        CallbackError::user(error.to_string())
    })
}

fn callback_return_from_python(value: &Bound<'_, PyAny>) -> PyResult<CallbackReturn> {
    if value.is_none() {
        Ok(CallbackReturn::None)
    } else if value.is_instance_of::<PyBool>() {
        Ok(CallbackReturn::Boolean(value.extract()?))
    } else {
        Ok(CallbackReturn::Value(python_to_json(value)?))
    }
}

fn json_to_python(py: Python<'_>, value: &Value) -> PyResult<Py<PyAny>> {
    match value {
        Value::Null => Ok(py.None()),
        Value::Bool(value) => Ok(value.into_pyobject(py)?.to_owned().into_any().unbind()),
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Ok(value.into_pyobject(py)?.into_any().unbind())
            } else if let Some(value) = value.as_u64() {
                Ok(value.into_pyobject(py)?.into_any().unbind())
            } else if let Some(value) = value.as_f64() {
                Ok(value.into_pyobject(py)?.into_any().unbind())
            } else {
                Err(PyValueError::new_err("unsupported JSON number"))
            }
        }
        Value::String(value) => Ok(value.into_pyobject(py)?.into_any().unbind()),
        Value::Array(values) => {
            let values = values
                .iter()
                .map(|value| json_to_python(py, value))
                .collect::<PyResult<Vec<_>>>()?;
            Ok(PyList::new(py, values)?.into_any().unbind())
        }
        Value::Object(values) => {
            let dictionary = PyDict::new(py);
            for (key, value) in values {
                dictionary.set_item(key, json_to_python(py, value)?)?;
            }
            Ok(dictionary.into_any().unbind())
        }
    }
}

fn python_to_json(value: &Bound<'_, PyAny>) -> PyResult<Value> {
    if value.is_none() {
        return Ok(Value::Null);
    }
    if value.is_instance_of::<PyBool>() {
        return Ok(Value::Bool(value.extract()?));
    }
    if value.is_instance_of::<PyInt>() {
        if let Ok(integer) = value.extract::<i64>() {
            return Ok(Value::Number(integer.into()));
        }
        if let Ok(integer) = value.extract::<u64>() {
            return Ok(Value::Number(integer.into()));
        }
        return Err(PyOverflowError::new_err(
            "integer is outside the JSON range",
        ));
    }
    if value.is_instance_of::<PyFloat>() {
        let number = value.extract::<f64>()?;
        return Number::from_f64(number)
            .map(Value::Number)
            .ok_or_else(|| PyValueError::new_err("float must be finite"));
    }
    if value.is_instance_of::<PyString>() {
        return Ok(Value::String(value.extract()?));
    }
    if let Ok(values) = value.cast::<PyList>() {
        return values
            .iter()
            .map(|value| python_to_json(&value))
            .collect::<PyResult<Vec<_>>>()
            .map(Value::Array);
    }
    if let Ok(values) = value.cast::<PyTuple>() {
        return values
            .iter()
            .map(|value| python_to_json(&value))
            .collect::<PyResult<Vec<_>>>()
            .map(Value::Array);
    }
    if let Ok(values) = value.cast::<PyDict>() {
        let mut object = Map::new();
        for (key, value) in values.iter() {
            let key = key
                .extract::<String>()
                .map_err(|_| PyTypeError::new_err("dictionary keys must be strings"))?;
            object.insert(key, python_to_json(&value)?);
        }
        return Ok(Value::Object(object));
    }
    if value.hasattr("__pokecon_separator__")?
        && value.getattr("__pokecon_separator__")?.is_truthy()?
    {
        let label = python_to_json(&value.getattr("label")?)?;
        return Ok(Value::Object(Map::from_iter([
            ("__pokecon_separator__".to_owned(), Value::Bool(true)),
            ("label".to_owned(), label),
        ])));
    }
    Err(PyTypeError::new_err(format!(
        "value of type {} is not JSON-compatible",
        value.get_type().name()?
    )))
}
