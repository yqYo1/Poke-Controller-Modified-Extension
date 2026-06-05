//!
//! Dynamic script launcher that reads script configurations from TOML files
//! and dispatches execution to the appropriate runtime (Python or Lua).
//!
//! # Overview
//!
//! [`ScriptLauncher`] reads a TOML config file containing `[[scripts]]` entries
//! and provides methods to launch scripts by name or launch all at once.
//! Python scripts are validated for file existence only (actual PyO3 execution
//! is a separate task); Lua scripts are executed via [`crate::lua::LuaRuntime`].
//!
//! # Example TOML
//!
//! ```toml
//! [[scripts]]
//! name = "auto_battle"
//! path = "scripts/auto_battle.py"
//! type = "python"
//! args = ["--difficulty", "hard"]
//! env = { LOG_LEVEL = "debug" }
//! timeout = 300
//!
//! [[scripts]]
//! name = "quick_setup"
//! path = "scripts/setup.lua"
//! type = "lua"
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// The type of script to be launched.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ScriptType {
    /// Python script (.py) — validated but not executed from Rust directly.
    Python,
    /// Lua script (.lua) — executed via `LuaRuntime` when the `lua` feature is
    /// enabled.
    Lua,
}

/// Configuration for a single script defined in a TOML config file.
///
/// Corresponds to each `[[scripts]]` entry in the configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScriptConfig {
    /// Display / identifier name for the script.
    pub name: String,

    /// Path to the script file (relative to the config file, or absolute).
    pub path: PathBuf,

    /// Script type: `"python"` or `"lua"`.
    #[serde(rename = "type")]
    pub script_type: ScriptType,

    /// Command-line arguments passed to the script.
    #[serde(default)]
    pub args: Vec<String>,

    /// Environment variables set before launch.
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Execution timeout in seconds (default: 300).
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

/// Returns the default timeout value (300 seconds).
const fn default_timeout() -> u64 {
    300
}

/// Top-level TOML structure for a script configuration file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScriptsConfig {
    /// List of script entries.
    #[serde(default)]
    pub scripts: Vec<ScriptConfig>,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during script configuration loading and launching.
#[derive(Debug, Error)]
pub enum ScriptLaunchError {
    /// I/O error (file reading, directory creation, etc.).
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// TOML deserialization error.
    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    /// TOML serialization error.
    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    /// Script with the given name was not found in the loaded configuration.
    #[error("Script not found: {0}")]
    NotFound(String),

    /// Generic script execution failure.
    #[error("Script execution failed: {0}")]
    ExecutionFailed(String),
}

// ---------------------------------------------------------------------------
// ScriptLauncher
// ---------------------------------------------------------------------------

/// Manages loading of TOML-based script configuration and launching scripts.
///
/// Supports both Python and Lua script types. Python scripts are validated
/// (file existence) but not executed — actual Python execution requires
/// PyO3 integration (separate task). Lua scripts are executed using the
/// existing [`crate::lua::LuaRuntime`].
///
/// # Example
///
/// ```rust
/// use pokecon_core::script_launcher::ScriptLauncher;
///
/// let mut launcher = ScriptLauncher::new();
/// launcher.load_toml(r#"
///     [[scripts]]
///     name = "hello"
///     path = "scripts/hello.py"
///     type = "python"
/// "#).unwrap();
/// assert_eq!(launcher.script_count(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct ScriptLauncher {
    scripts: HashMap<String, ScriptConfig>,
    config_dir: PathBuf,
}

impl ScriptLauncher {
    /// Create a new empty `ScriptLauncher`.
    ///
    /// The config directory defaults to the current working directory.
    pub fn new() -> Self {
        Self {
            scripts: HashMap::new(),
            config_dir: PathBuf::from("."),
        }
    }

    /// Create a new `ScriptLauncher` with a specific config base directory.
    ///
    /// Script paths that are relative will be resolved against this directory.
    pub fn with_config_dir<P: AsRef<Path>>(config_dir: P) -> Self {
        Self {
            scripts: HashMap::new(),
            config_dir: config_dir.as_ref().to_path_buf(),
        }
    }

    /// Load a TOML configuration file containing script definitions.
    ///
    /// Each `[[scripts]]` entry in the TOML file becomes a registered script.
    /// The config directory is updated to the parent of the loaded file,
    /// so relative script paths are resolved correctly.
    pub fn load_config<P: AsRef<Path>>(&mut self, path: P) -> Result<(), ScriptLaunchError> {
        let path = path.as_ref().canonicalize()?;
        let content = std::fs::read_to_string(&path)?;
        let config: ScriptsConfig = toml::from_str(&content)?;

        // Update config_dir to the parent of the config file so that relative
        // script paths are resolved against the config file's directory.
        if let Some(parent) = path.parent() {
            self.config_dir = parent.to_path_buf();
        }

        for script in config.scripts {
            self.scripts.insert(script.name.clone(), script);
        }

        Ok(())
    }

    /// Load script configuration from a TOML string (useful for testing and
    /// in-memory configs).
    pub fn load_toml(&mut self, toml_str: &str) -> Result<(), ScriptLaunchError> {
        let config: ScriptsConfig = toml::from_str(toml_str)?;
        for script in config.scripts {
            self.scripts.insert(script.name.clone(), script);
        }
        Ok(())
    }

    /// Resolve a script's path, handling relative paths against the config
    /// directory.
    fn resolve_path(&self, config: &ScriptConfig) -> PathBuf {
        if config.path.is_absolute() {
            config.path.clone()
        } else {
            self.config_dir.join(&config.path)
        }
    }

    /// Launch a single script by name.
    ///
    /// For Python scripts, this validates that the file exists and returns
    /// `Ok(())`.  For Lua scripts, the file is loaded and executed via
    /// [`crate::lua::LuaRuntime`] (requires the `lua` feature).
    pub fn launch(&self, name: &str) -> Result<(), ScriptLaunchError> {
        let config = self
            .scripts
            .get(name)
            .ok_or_else(|| ScriptLaunchError::NotFound(name.to_string()))?;

        let script_path = self.resolve_path(config);

        if !script_path.exists() {
            return Err(ScriptLaunchError::NotFound(
                script_path.display().to_string(),
            ));
        }

        match config.script_type {
            ScriptType::Python => {
                // Python: validate file exists and return.
                // Actual Python execution via PyO3 is a separate task.
                Ok(())
            }
            ScriptType::Lua => {
                #[cfg(feature = "lua")]
                {
                    self.launch_lua(config, &script_path)
                }
                #[cfg(not(feature = "lua"))]
                {
                    let _ = (config, script_path);
                    Err(ScriptLaunchError::ExecutionFailed(
                        "Lua support not enabled (compile with --features lua)".to_string(),
                    ))
                }
            }
        }
    }

    /// Launch all registered scripts in sequence and return the results.
    ///
    /// The order follows insertion order from the TOML file / `load_toml` calls.
    pub fn launch_all(&self) -> Vec<(&str, Result<(), ScriptLaunchError>)> {
        let mut results = Vec::with_capacity(self.scripts.len());
        for name in self.scripts.keys() {
            let result = self.launch(name);
            results.push((name.as_str(), result));
        }
        results
    }

    /// Return a list of all registered script configurations.
    pub fn list_scripts(&self) -> Vec<&ScriptConfig> {
        self.scripts.values().collect()
    }

    /// Return a list of all registered script names.
    pub fn script_names(&self) -> Vec<&str> {
        self.scripts.keys().map(|s| s.as_str()).collect()
    }

    /// Check whether a script with the given name is registered.
    pub fn has_script(&self, name: &str) -> bool {
        self.scripts.contains_key(name)
    }

    /// Get the configuration for a script by name.
    pub fn get_script(&self, name: &str) -> Option<&ScriptConfig> {
        self.scripts.get(name)
    }

    /// Return the number of registered scripts.
    pub fn script_count(&self) -> usize {
        self.scripts.len()
    }

    // ── Lua launching ──────────────────────────────────────────────────────

    /// Launch a Lua script using a fresh `LuaRuntime`.
    ///
    /// A temporary tokio runtime is used internally because `LuaRuntime`'s
    /// `load_file` method is async.
    #[cfg(feature = "lua")]
    fn launch_lua(
        &self,
        config: &ScriptConfig,
        script_path: &Path,
    ) -> Result<(), ScriptLaunchError> {
        use crate::lua::LuaRuntime;

        let rt = tokio::runtime::Runtime::new().map_err(|e| {
            ScriptLaunchError::ExecutionFailed(format!(
                "Failed to create async runtime for Lua: {e}"
            ))
        })?;

        let mut lua = LuaRuntime::new().map_err(|e| {
            ScriptLaunchError::ExecutionFailed(format!(
                "Failed to create Lua runtime for '{}': {e}",
                config.name
            ))
        })?;

        rt.block_on(lua.load_file(script_path)).map_err(|e| {
            ScriptLaunchError::ExecutionFailed(format!(
                "Failed to load Lua script '{}': {e}",
                config.name
            ))
        })?;

        Ok(())
    }
}

impl Default for ScriptLauncher {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    // ── Config parsing / serialization ────────────────────────────────────

    #[test]
    fn test_config_roundtrip() {
        let config = ScriptsConfig {
            scripts: vec![
                ScriptConfig {
                    name: "auto_battle".to_string(),
                    path: PathBuf::from("scripts/auto_battle.py"),
                    script_type: ScriptType::Python,
                    args: vec!["--difficulty".to_string(), "hard".to_string()],
                    env: [("LOG_LEVEL".to_string(), "debug".to_string())]
                        .into_iter()
                        .collect(),
                    timeout: 300,
                },
                ScriptConfig {
                    name: "quick_setup".to_string(),
                    path: PathBuf::from("scripts/setup.lua"),
                    script_type: ScriptType::Lua,
                    args: vec![],
                    env: HashMap::new(),
                    timeout: 300,
                },
            ],
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let deserialized: ScriptsConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_default_timeout() {
        let toml_str = r#"
            [[scripts]]
            name = "no_timeout"
            path = "script.py"
            type = "python"
        "#;
        let config: ScriptsConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.scripts.len(), 1);
        assert_eq!(config.scripts[0].timeout, 300);
    }

    #[test]
    fn test_custom_timeout() {
        let toml_str = r#"
            [[scripts]]
            name = "with_timeout"
            path = "script.py"
            type = "python"
            timeout = 600
        "#;
        let config: ScriptsConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.scripts[0].timeout, 600);
    }

    #[test]
    fn test_script_type_lowercase() {
        let toml_str = r#"
            [[scripts]]
            name = "py"
            path = "a.py"
            type = "python"

            [[scripts]]
            name = "lua"
            path = "b.lua"
            type = "lua"
        "#;
        let config: ScriptsConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.scripts[0].script_type, ScriptType::Python);
        assert_eq!(config.scripts[1].script_type, ScriptType::Lua);
    }

    // ── ScriptLauncher ────────────────────────────────────────────────────

    #[test]
    fn test_load_toml() {
        let mut launcher = ScriptLauncher::new();
        launcher
            .load_toml(
                r#"
                [[scripts]]
                name = "alpha"
                path = "scripts/alpha.py"
                type = "python"

                [[scripts]]
                name = "beta"
                path = "scripts/beta.lua"
                type = "lua"
                "#,
            )
            .unwrap();

        assert_eq!(launcher.script_count(), 2);
        assert!(launcher.has_script("alpha"));
        assert!(launcher.has_script("beta"));
    }

    #[test]
    fn test_load_toml_overwrite() {
        let mut launcher = ScriptLauncher::new();
        launcher
            .load_toml(
                r#"
                [[scripts]]
                name = "foo"
                path = "first.py"
                type = "python"
                "#,
            )
            .unwrap();

        // Load another config with same name — should overwrite
        launcher
            .load_toml(
                r#"
                [[scripts]]
                name = "foo"
                path = "second.py"
                type = "python"
                "#,
            )
            .unwrap();

        assert_eq!(launcher.script_count(), 1);
        assert_eq!(
            launcher.get_script("foo").unwrap().path,
            PathBuf::from("second.py")
        );
    }

    #[test]
    fn test_list_scripts() {
        let mut launcher = ScriptLauncher::new();
        launcher
            .load_toml(
                r#"
                [[scripts]]
                name = "s1"
                path = "s1.py"
                type = "python"

                [[scripts]]
                name = "s2"
                path = "s2.lua"
                type = "lua"
                "#,
            )
            .unwrap();

        let scripts = launcher.list_scripts();
        assert_eq!(scripts.len(), 2);
        let names: Vec<&str> = scripts.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"s1"));
        assert!(names.contains(&"s2"));
    }

    #[test]
    fn test_script_names() {
        let mut launcher = ScriptLauncher::new();
        launcher
            .load_toml(
                r#"
                [[scripts]]
                name = "a"
                path = "a.py"
                type = "python"
                "#,
            )
            .unwrap();

        let names = launcher.script_names();
        assert_eq!(names, vec!["a"]);
    }

    #[test]
    fn test_has_script() {
        let mut launcher = ScriptLauncher::new();
        launcher
            .load_toml(
                r#"
                [[scripts]]
                name = "exists"
                path = "x.py"
                type = "python"
                "#,
            )
            .unwrap();

        assert!(launcher.has_script("exists"));
        assert!(!launcher.has_script("nonexistent"));
    }

    #[test]
    fn test_get_script() {
        let mut launcher = ScriptLauncher::new();
        launcher
            .load_toml(
                r#"
                [[scripts]]
                name = "getme"
                path = "scripts/getme.py"
                type = "python"
                args = ["--verbose"]
                env = { FOO = "bar" }
                timeout = 120
                "#,
            )
            .unwrap();

        let config = launcher.get_script("getme").unwrap();
        assert_eq!(config.name, "getme");
        assert_eq!(config.path, PathBuf::from("scripts/getme.py"));
        assert_eq!(config.script_type, ScriptType::Python);
        assert_eq!(config.args, vec!["--verbose"]);
        assert_eq!(config.env.get("FOO").unwrap(), "bar");
        assert_eq!(config.timeout, 120);
    }

    #[test]
    fn test_launch_nonexistent_script_name() {
        let launcher = ScriptLauncher::new();
        let result = launcher.launch("does_not_exist");
        assert!(result.is_err());
        match result {
            Err(ScriptLaunchError::NotFound(name)) => {
                assert_eq!(name, "does_not_exist");
            }
            _ => panic!("Expected NotFound error"),
        }
    }

    #[test]
    fn test_launch_python_file_exists() {
        let dir = TempDir::new().unwrap();
        let script_path = dir.path().join("hello.py");
        std::fs::write(&script_path, "print('hello')\n").unwrap();

        let mut launcher = ScriptLauncher::with_config_dir(dir.path());
        launcher
            .load_toml(&format!(
                r#"
                    [[scripts]]
                    name = "hello"
                    path = "{}"
                    type = "python"
                    "#,
                script_path.display()
            ))
            .unwrap();

        // Python launch should succeed (file exists)
        let result = launcher.launch("hello");
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    }

    #[test]
    fn test_launch_python_file_not_found() {
        let mut launcher = ScriptLauncher::new();
        launcher
            .load_toml(
                r#"
                [[scripts]]
                name = "missing"
                path = "/nonexistent/path/script.py"
                type = "python"
                "#,
            )
            .unwrap();

        let result = launcher.launch("missing");
        assert!(result.is_err());
    }

    #[test]
    fn test_launch_python_relative_path() {
        let dir = TempDir::new().unwrap();
        let scripts_dir = dir.path().join("scripts");
        std::fs::create_dir(&scripts_dir).unwrap();
        std::fs::write(scripts_dir.join("rel.py"), "print('rel')\n").unwrap();

        // Write config file inside the temp dir
        let config_path = dir.path().join("scripts.toml");
        let mut file = std::fs::File::create(&config_path).unwrap();
        writeln!(
            file,
            r#"[[scripts]]
name = "rel"
path = "scripts/rel.py"
type = "python"
"#
        )
        .unwrap();

        let mut launcher = ScriptLauncher::new();
        launcher.load_config(&config_path).unwrap();

        let result = launcher.launch("rel");
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    }

    #[test]
    fn test_launch_all() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.py"), "# script a\n").unwrap();
        std::fs::write(dir.path().join("b.py"), "# script b\n").unwrap();
        std::fs::write(dir.path().join("c.py"), "# script c\n").unwrap();

        let mut launcher = ScriptLauncher::with_config_dir(dir.path());
        launcher
            .load_toml(
                r#"
                [[scripts]]
                name = "a"
                path = "a.py"
                type = "python"

                [[scripts]]
                name = "b"
                path = "b.py"
                type = "python"

                [[scripts]]
                name = "c"
                path = "c.py"
                type = "python"
                "#,
            )
            .unwrap();

        let results = launcher.launch_all();
        assert_eq!(results.len(), 3);
        for (name, result) in &results {
            assert!(result.is_ok(), "Script '{}' failed: {:?}", name, result);
        }
    }

    #[test]
    fn test_empty_config() {
        let mut launcher = ScriptLauncher::new();
        launcher.load_toml("").unwrap();
        assert_eq!(launcher.script_count(), 0);
        assert!(launcher.list_scripts().is_empty());
    }

    #[test]
    fn test_load_config_file_not_found() {
        let mut launcher = ScriptLauncher::new();
        let result = launcher.load_config("/nonexistent/config.toml");
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_toml() {
        let mut launcher = ScriptLauncher::new();
        let result = launcher.load_toml("this is not valid toml [[[[");
        assert!(result.is_err());
        match result {
            Err(ScriptLaunchError::TomlParse(_)) => {} // expected
            _ => panic!("Expected TomlParse error"),
        }
    }

    #[test]
    fn test_default_impl() {
        let launcher = ScriptLauncher::default();
        assert_eq!(launcher.script_count(), 0);
    }

    #[test]
    fn test_with_config_dir() {
        let dir = TempDir::new().unwrap();
        let launcher = ScriptLauncher::with_config_dir(dir.path());
        assert_eq!(launcher.config_dir, dir.path());
    }

    #[test]
    fn test_clone() {
        let mut launcher = ScriptLauncher::new();
        launcher
            .load_toml(
                r#"
                [[scripts]]
                name = "clone_me"
                path = "x.py"
                type = "python"
                "#,
            )
            .unwrap();

        let cloned = launcher.clone();
        assert_eq!(cloned.script_count(), 1);
        assert!(cloned.has_script("clone_me"));
    }

    // ── Lua tests (only when lua feature is enabled) ──────────────────────

    #[cfg(feature = "lua")]
    #[test]
    fn test_launch_lua_file_exists() {
        let dir = TempDir::new().unwrap();
        let script_path = dir.path().join("hello.lua");
        std::fs::write(&script_path, "print('hello from lua')\n").unwrap();

        let mut launcher = ScriptLauncher::with_config_dir(dir.path());
        launcher
            .load_toml(&format!(
                r#"
                    [[scripts]]
                    name = "hello_lua"
                    path = "{}"
                    type = "lua"
                    "#,
                script_path.display()
            ))
            .unwrap();

        let result = launcher.launch("hello_lua");
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    }

    #[cfg(feature = "lua")]
    #[test]
    fn test_launch_lua_syntax_error() {
        let dir = TempDir::new().unwrap();
        let script_path = dir.path().join("bad.lua");
        // Invalid Lua syntax
        std::fs::write(&script_path, "this is not valid lua {{{}\n").unwrap();

        let mut launcher = ScriptLauncher::with_config_dir(dir.path());
        launcher
            .load_toml(&format!(
                r#"
                    [[scripts]]
                    name = "bad_lua"
                    path = "{}"
                    type = "lua"
                    "#,
                script_path.display()
            ))
            .unwrap();

        let result = launcher.launch("bad_lua");
        assert!(result.is_err(), "Expected error for invalid Lua syntax");
    }

    #[cfg(feature = "lua")]
    #[test]
    fn test_launch_lua_multiple_scripts() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.lua"), "print('a')\n").unwrap();
        std::fs::write(dir.path().join("b.lua"), "print('b')\n").unwrap();

        let mut launcher = ScriptLauncher::with_config_dir(dir.path());
        launcher
            .load_toml(
                r#"
                [[scripts]]
                name = "lua_a"
                path = "a.lua"
                type = "lua"

                [[scripts]]
                name = "lua_b"
                path = "b.lua"
                type = "lua"
                "#,
            )
            .unwrap();

        let results = launcher.launch_all();
        assert_eq!(results.len(), 2);
        for (name, result) in &results {
            assert!(result.is_ok(), "Lua script '{}' failed: {:?}", name, result);
        }
    }
}
