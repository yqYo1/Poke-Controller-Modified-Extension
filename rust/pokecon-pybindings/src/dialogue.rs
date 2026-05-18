use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

// ---------------------------------------------------------------------------
// Helper: ensure a tkinter root window exists
// ---------------------------------------------------------------------------

fn _ensure_tk(py: Python<'_>) -> PyResult<PyObject> {
    let tk = py.import("tkinter")?;
    // Try to get existing default root
    let root = tk.getattr("_default_root").ok();

    match root {
        Some(r) if !r.is_none() => Ok(r.into()),
        _ => {
            let new_root = tk.call_method0("Tk")?;
            new_root.call_method0("withdraw")?;
            Ok(new_root.into())
        }
    }
}

// ---------------------------------------------------------------------------
// Simple message box dialogs — call tkinter.messagebox directly
// ---------------------------------------------------------------------------

/// Show a message box dialog.
#[pyfunction]
#[pyo3(signature = (title, message, kind = "info"))]
fn show_message(title: String, message: String, kind: &str) -> PyResult<()> {
    Python::with_gil(|py| {
        _ensure_tk(py)?;
        let msgbox = py.import("tkinter.messagebox")?;
        let func_name = match kind {
            "warning" => "showwarning",
            "error" => "showerror",
            _ => "showinfo",
        };
        let func = msgbox.getattr(func_name)?;
        func.call1((title, message))?;
        Ok(())
    })
}

/// Show a Yes/No confirmation dialog.
#[pyfunction]
fn confirm(title: String, message: String) -> PyResult<bool> {
    Python::with_gil(|py| {
        _ensure_tk(py)?;
        let msgbox = py.import("tkinter.messagebox")?;
        let func = msgbox.getattr("askyesno")?;
        let result = func.call1((title, message))?;
        result
            .extract::<bool>()
            .map_err(|e| PyRuntimeError::new_err(format!("confirm dialog failed: {e}")))
    })
}

/// Show a text input dialog.
#[pyfunction]
#[pyo3(signature = (title, prompt, default = ""))]
fn input_dialog(title: String, prompt: String, default: &str) -> PyResult<Option<String>> {
    Python::with_gil(|py| {
        _ensure_tk(py)?;
        let sd = py.import("tkinter.simpledialog")?;
        let func = sd.getattr("askstring")?;
        let kwargs = PyDict::new(py);
        kwargs.set_item("initialvalue", default)?;
        let result = func.call((title, prompt), Some(&kwargs))?;
        if result.is_none() {
            return Ok(None);
        }
        result
            .extract::<String>()
            .map(Some)
            .map_err(|e| PyRuntimeError::new_err(format!("input_dialog failed: {e}")))
    })
}

/// Show an OK/Cancel dialog.
#[pyfunction]
fn ok_cancel(title: String, message: String) -> PyResult<bool> {
    Python::with_gil(|py| {
        _ensure_tk(py)?;
        let msgbox = py.import("tkinter.messagebox")?;
        let func = msgbox.getattr("askokcancel")?;
        let result = func.call1((title, message))?;
        result
            .extract::<bool>()
            .map_err(|e| PyRuntimeError::new_err(format!("ok_cancel dialog failed: {e}")))
    })
}

/// Show a Retry/Cancel dialog.
#[pyfunction]
fn retry_cancel(title: String, message: String) -> PyResult<bool> {
    Python::with_gil(|py| {
        _ensure_tk(py)?;
        let msgbox = py.import("tkinter.messagebox")?;
        let func = msgbox.getattr("askretrycancel")?;
        let result = func.call1((title, message))?;
        result
            .extract::<bool>()
            .map_err(|e| PyRuntimeError::new_err(format!("retry_cancel dialog failed: {e}")))
    })
}

// ---------------------------------------------------------------------------
// dialogue() — delegates to pure-Python implementation
// ---------------------------------------------------------------------------

/// Show a multi-entry input dialog (original Poke-Controller API).
///
/// The adapter's ``show_dialogue()`` bypasses this Rust binding and calls
/// the pure-Python implementation directly to avoid the module name
/// collision between the compiled ``pokecon.dialogue`` extension and the
/// ``dialogue.py`` source file.  This function exists so that
/// ``pokecon.dialogue.dialogue()`` is available on the Rust module for
/// completeness, but it returns an error directing callers to use the
/// adapter or the Python ``dialogue`` module directly.
#[pyfunction]
#[pyo3(signature = (title, message, desc = None, _need = "list"))]
fn dialogue(
    title: String,
    message: PyObject,
    desc: Option<String>,
    _need: &str,
) -> PyResult<PyObject> {
    Python::with_gil(|py| {
        // Load the Python dialogue module via importlib.util to avoid
        // the module name collision with this Rust extension.
        let importlib = py.import("importlib")?;
        let util = importlib.getattr("util")?;

        // Find the spec for a known pokecon Python submodule (not Rust)
        // to locate the package source directory.
        let spec = util.call_method1("find_spec", ("pokecon.commands",))?;
        let origin: String = spec.getattr("origin")?.extract()?;

        let py_path = {
            let p = std::path::Path::new(&origin);
            let parent = p.parent().ok_or_else(|| {
                PyRuntimeError::new_err(format!("cannot determine parent of {}", origin))
            })?;
            let dialogue_py = parent.join("dialogue.py");
            dialogue_py.to_string_lossy().to_string()
        };

        let file_spec = util.call_method1(
            "spec_from_file_location",
            ("pokecon._dialogue_py", &py_path),
        )?;
        let py_mod = util.call_method1("module_from_spec", (&file_spec,))?;
        let loader = file_spec.getattr("loader")?;
        loader.call_method1("exec_module", (&py_mod,))?;

        let desc_str: String = desc.unwrap_or_default();
        let result = if desc_str.is_empty() {
            py_mod.call_method1("dialogue", (&title, &message, py.None(), "list"))?
        } else {
            py_mod.call_method1("dialogue", (&title, &message, &desc_str, "list"))?
        };
        Ok(result.into())
    })
}

// ---------------------------------------------------------------------------
// dialogue6widget() — delegates to pure-Python implementation
// ---------------------------------------------------------------------------

/// Internal helper: load the pure-Python dialogue module via importlib.
fn _load_py_dialogue_module(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    let importlib = py.import("importlib")?;
    let util = importlib.getattr("util")?;

    let spec = util.call_method1("find_spec", ("pokecon.commands",))?;
    let origin: String = spec.getattr("origin")?.extract()?;

    let py_path = {
        let p = std::path::Path::new(&origin);
        let parent = p.parent().ok_or_else(|| {
            PyRuntimeError::new_err(format!("cannot determine parent of {}", origin))
        })?;
        let dialogue_py = parent.join("dialogue.py");
        dialogue_py.to_string_lossy().to_string()
    };

    let file_spec = util.call_method1(
        "spec_from_file_location",
        ("pokecon._dialogue_py", &py_path),
    )?;
    let py_mod = util.call_method1("module_from_spec", (&file_spec,))?;
    let loader = file_spec.getattr("loader")?;
    loader.call_method1("exec_module", (&py_mod,))?;

    Ok(py_mod)
}

/// Show a multi-widget input dialog (original Poke-Controller API).
#[pyfunction]
#[pyo3(signature = (title, dialogue_list, desc = None, need = "list"))]
fn dialogue6widget(
    title: String,
    dialogue_list: PyObject,
    desc: Option<String>,
    need: &str,
) -> PyResult<PyObject> {
    Python::with_gil(|py| {
        let py_mod = _load_py_dialogue_module(py)?;
        let desc_str: String = desc.unwrap_or_default();
        let result = if desc_str.is_empty() {
            py_mod.call_method1("dialogue6widget", (&title, &dialogue_list, py.None(), need))?
        } else {
            py_mod.call_method1("dialogue6widget", (&title, &dialogue_list, &desc_str, need))?
        };
        Ok(result.into())
    })
}

/// Show a multi-widget dialog with settings persistence.
#[pyfunction]
#[pyo3(signature = (title, dialogue_list, filename, desc = None, need = "list"))]
fn dialogue6widget_save_settings(
    title: String,
    dialogue_list: PyObject,
    filename: String,
    desc: Option<String>,
    need: &str,
) -> PyResult<PyObject> {
    Python::with_gil(|py| {
        let py_mod = _load_py_dialogue_module(py)?;
        let desc_str: String = desc.unwrap_or_default();
        let result = if desc_str.is_empty() {
            py_mod.call_method1(
                "dialogue6widget_save_settings",
                (&title, &dialogue_list, &filename, py.None(), need),
            )?
        } else {
            py_mod.call_method1(
                "dialogue6widget_save_settings",
                (&title, &dialogue_list, &filename, &desc_str, need),
            )?
        };
        Ok(result.into())
    })
}

/// Show a multi-widget dialog with settings selection.
#[pyfunction]
#[pyo3(signature = (title, dialogue_list, dirname, desc = None, need = "list"))]
fn dialogue6widget_select_settings(
    title: String,
    dialogue_list: PyObject,
    dirname: String,
    desc: Option<String>,
    need: &str,
) -> PyResult<PyObject> {
    Python::with_gil(|py| {
        let py_mod = _load_py_dialogue_module(py)?;
        let desc_str: String = desc.unwrap_or_default();
        let result = if desc_str.is_empty() {
            py_mod.call_method1(
                "dialogue6widget_select_settings",
                (&title, &dialogue_list, &dirname, py.None(), need),
            )?
        } else {
            py_mod.call_method1(
                "dialogue6widget_select_settings",
                (&title, &dialogue_list, &dirname, &desc_str, need),
            )?
        };
        Ok(result.into())
    })
}

// ===================================================================
// Module registration
// ===================================================================

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(show_message, m)?)?;
    m.add_function(wrap_pyfunction!(confirm, m)?)?;
    m.add_function(wrap_pyfunction!(input_dialog, m)?)?;
    m.add_function(wrap_pyfunction!(dialogue, m)?)?;
    m.add_function(wrap_pyfunction!(ok_cancel, m)?)?;
    m.add_function(wrap_pyfunction!(retry_cancel, m)?)?;
    m.add_function(wrap_pyfunction!(dialogue6widget, m)?)?;
    m.add_function(wrap_pyfunction!(dialogue6widget_save_settings, m)?)?;
    m.add_function(wrap_pyfunction!(dialogue6widget_select_settings, m)?)?;
    Ok(())
}

// ===================================================================
// Tests
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_functions_registered() {
        Python::with_gil(|py| {
            let m = PyModule::new(py, "test_dialogue").unwrap();
            register(&m).unwrap();
            for name in &[
                "show_message",
                "confirm",
                "input_dialog",
                "dialogue",
                "ok_cancel",
                "retry_cancel",
                "dialogue6widget",
                "dialogue6widget_save_settings",
                "dialogue6widget_select_settings",
            ] {
                assert!(
                    m.getattr(name).is_ok(),
                    "function {name} should be registered"
                );
            }
        });
    }
}
