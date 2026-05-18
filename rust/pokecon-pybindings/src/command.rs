use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::path::PathBuf;

use pokecon_core::command_manager::CommandManager as RustCommandManager;

// ---------------------------------------------------------------------------
// Command - wraps pokecon_core::command_manager::CommandManager
// ---------------------------------------------------------------------------

/// A command manager that discovers, loads, runs, and stops automation scripts.
///
/// This is the top-level controller for managing command lifecycle in
/// Poke-Controller.  Use it to:
///
/// - Scan a directory for available scripts (``.py`` / ``.lua``)
/// - Load / unload individual commands by path
/// - Set a command as the active (running) command
/// - Stop the currently running command
/// - Query the status of loaded / active commands
///
/// Python usage::
///
///     from pokecon.command import Command
///
///     cmd = Command("path/to/scripts")
///     cmd.scan()
///     cmd.load("path/to/scripts/my_script.py")
///     cmd.run("my_script")
///     print(cmd.status())
///     cmd.stop()
#[pyclass(name = "Command")]
pub struct PyCommand {
    inner: RustCommandManager,
}

#[pymethods]
impl PyCommand {
    /// Create a new Command manager that looks for scripts in *script_dir*.
    ///
    /// The directory is created if it does not exist.
    #[new]
    fn new(script_dir: String) -> PyResult<Self> {
        let path = PathBuf::from(&script_dir);
        let inner = RustCommandManager::new(path).map_err(|e| {
            PyRuntimeError::new_err(format!("Failed to create CommandManager: {e}"))
        })?;
        Ok(Self { inner })
    }

    /// Scan the script directory and discover available commands.
    ///
    /// Returns a list of command names found.
    fn scan(&mut self) -> PyResult<Vec<String>> {
        self.inner
            .scan()
            .map_err(|e| PyRuntimeError::new_err(format!("Scan failed: {e}")))
    }

    /// Load a command from the given file *path*.
    ///
    /// The command name is derived from the file stem (filename without extension).
    /// Returns the command name on success.
    fn load(&mut self, path: String) -> PyResult<String> {
        let path = PathBuf::from(&path);
        self.inner
            .load(path)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to load command: {e}")))
    }

    /// Unload a previously loaded command by *name*.
    fn unload(&mut self, name: String) -> PyResult<()> {
        self.inner
            .unload(&name)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to unload command: {e}")))
    }

    /// Reload metadata (description, etc.) for the command *name* from disk.
    fn reload(&mut self, name: String) -> PyResult<()> {
        self.inner
            .reload(&name)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to reload command: {e}")))
    }

    /// Set the named command as the active (running) command.
    ///
    /// The command must have been previously loaded via ``load()`` or ``scan()``.
    fn run(&mut self, name: String) -> PyResult<()> {
        self.inner
            .set_active(&name)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to run command: {e}")))
    }

    /// Stop the currently active command without unloading it.
    fn stop(&mut self) {
        self.inner.stop();
    }

    /// Return a dictionary describing the current state of the command manager.
    ///
    /// Keys:
    ///
    /// * ``active`` — name of the currently active command, or ``None``
    /// * ``commands`` — list of all loaded command names
    /// * ``count`` — number of loaded commands
    fn status<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);

        // Active command name
        match self.inner.active_name() {
            Some(name) => dict.set_item("active", name)?,
            None => dict.set_item("active", py.None())?,
        }

        // List of loaded command names
        let names = self.inner.names();
        dict.set_item("commands", &names)?;

        // Count
        dict.set_item("count", names.len())?;

        Ok(dict)
    }

    /// Return the name of the currently active command, or ``None``.
    fn active_name(&self) -> Option<String> {
        self.inner.active_name().map(|s| s.to_string())
    }

    /// Return a list of all loaded command names.
    fn names(&self) -> Vec<String> {
        self.inner.names()
    }

    /// Return the number of loaded commands.
    fn count(&self) -> usize {
        self.inner.names().len()
    }

    /// Check whether a particular command is loaded.
    fn is_loaded(&self, name: String) -> bool {
        self.inner.get(&name).is_some()
    }

    fn __repr__(&self) -> String {
        let active = self.inner.active_name().unwrap_or("none");
        let count = self.inner.names().len();
        format!("<Command count={} active='{}'>", count, active)
    }
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyCommand>()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_new_command() {
        let dir = tempfile::TempDir::new().unwrap();
        let script_dir = dir.path().join("scripts");
        std::fs::create_dir(&script_dir).unwrap();

        Python::with_gil(|py| {
            let cmd = Bound::new(
                py,
                PyCommand::new(script_dir.to_str().unwrap().to_string()).unwrap(),
            )
            .unwrap();
            let count: usize = cmd.call_method0("count").unwrap().extract().unwrap();
            assert_eq!(count, 0);
            let active: Option<String> =
                cmd.call_method0("active_name").unwrap().extract().unwrap();
            assert!(active.is_none());
        });
    }

    #[test]
    fn test_scan_and_load() {
        let dir = tempfile::TempDir::new().unwrap();
        let script_dir = dir.path().join("scripts");
        std::fs::create_dir(&script_dir).unwrap();

        let mut file = std::fs::File::create(script_dir.join("test_script.py")).unwrap();
        writeln!(file, "# Description: A test command").unwrap();
        writeln!(file, "print('hello')").unwrap();

        Python::with_gil(|py| {
            let cmd = Bound::new(
                py,
                PyCommand::new(script_dir.to_str().unwrap().to_string()).unwrap(),
            )
            .unwrap();

            // scan
            let names: Vec<String> = cmd.call_method0("scan").unwrap().extract().unwrap();
            assert!(names.contains(&"test_script".to_string()));
            assert_eq!(
                cmd.call_method0("count")
                    .unwrap()
                    .extract::<usize>()
                    .unwrap(),
                1
            );

            // load (by full path)
            let path = script_dir.join("another.py");
            std::fs::write(&path, "# Description: Another\n").unwrap();
            let name: String = cmd
                .call_method1("load", (path.to_str().unwrap(),))
                .unwrap()
                .extract()
                .unwrap();
            assert_eq!(name, "another");
            assert_eq!(
                cmd.call_method0("count")
                    .unwrap()
                    .extract::<usize>()
                    .unwrap(),
                2
            );

            // run
            cmd.call_method1("run", ("test_script",)).unwrap();
            let active: Option<String> =
                cmd.call_method0("active_name").unwrap().extract().unwrap();
            assert_eq!(active.as_deref(), Some("test_script"));

            // stop
            cmd.call_method0("stop").unwrap();
            let active: Option<String> =
                cmd.call_method0("active_name").unwrap().extract().unwrap();
            assert!(active.is_none());
        });
    }

    #[test]
    fn test_status() {
        let dir = tempfile::TempDir::new().unwrap();
        let script_dir = dir.path().join("scripts");
        std::fs::create_dir(&script_dir).unwrap();

        let mut file = std::fs::File::create(script_dir.join("my_cmd.py")).unwrap();
        writeln!(file, "# Description: My command").unwrap();
        writeln!(file, "print('hi')").unwrap();

        Python::with_gil(|py| {
            let cmd = Bound::new(
                py,
                PyCommand::new(script_dir.to_str().unwrap().to_string()).unwrap(),
            )
            .unwrap();
            cmd.call_method0("scan").unwrap();
            cmd.call_method1("run", ("my_cmd",)).unwrap();

            let status = cmd.call_method0("status").unwrap();
            let active: Option<String> = status.get_item("active").unwrap().extract().unwrap();
            assert_eq!(active.as_deref(), Some("my_cmd"));
            let commands: Vec<String> = status.get_item("commands").unwrap().extract().unwrap();
            assert!(commands.contains(&"my_cmd".to_string()));
            let count: usize = status.get_item("count").unwrap().extract().unwrap();
            assert_eq!(count, 1);
        });
    }
}
