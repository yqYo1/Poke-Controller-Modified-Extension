use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use atomic_write_file::AtomicWriteFile;
use pokecon_contracts::{
    ContractError, commands_python_typings, dynamic_lua_typings, dynamic_python_typings,
};
use thiserror::Error;

use crate::settings::roots::{EffectiveRoots, RootError, SafeComponent};

/// Creates missing editable configuration and regenerates managed typings in
/// their contractually distinct roots.
#[derive(Clone, Debug)]
pub struct ScaffoldManager {
    roots: EffectiveRoots,
}

impl ScaffoldManager {
    #[must_use]
    pub const fn new(roots: EffectiveRoots) -> Self {
        Self { roots }
    }

    /// Ensures all Phase-3 path contracts for one active profile.
    ///
    /// Existing user-editable files are never overwritten. Generated Python
    /// and Lua typings are atomically refreshed under Data.
    ///
    /// # Errors
    ///
    /// Returns a registry, safe-component, or filesystem error.
    pub fn ensure(&self, active_profile: &str) -> Result<ScaffoldPaths, ScaffoldError> {
        self.roots.ensure()?;
        let active_profile = SafeComponent::new(active_profile)?;
        let profiles = self.roots.config.join("profiles");
        let profile = profiles.join(active_profile.as_str());
        create_protected_directory(&profiles)?;
        create_protected_directory(&profile)?;

        let global_settings = self.roots.config.join("settings.toml");
        let profile_settings = profile.join("settings.toml");
        let init_python = self.roots.config.join("init.py");
        let init_lua = self.roots.config.join("init.lua");
        let pyproject = self.roots.config.join("pyproject.toml");
        let luarc = self.roots.config.join(".luarc.json");
        create_editable_file(
            &global_settings,
            "# PokeCon global settings. Unknown keys and comments are preserved.\n",
        )?;
        create_editable_file(
            &profile_settings,
            "# PokeCon profile settings. This file is user-editable.\n",
        )?;
        create_editable_file(
            &init_python,
            "# PokeCon Python dynamic configuration.\n# This file is never overwritten after creation.\n",
        )?;
        create_editable_file(
            &init_lua,
            "-- PokeCon Lua dynamic configuration.\n-- This file is never overwritten after creation.\n",
        )?;
        create_editable_file(&pyproject, &initial_pyproject(&self.roots))?;
        create_editable_file(&luarc, &initial_luarc(&self.roots)?)?;

        let python_typings = self.roots.data.join("typings/pokecon/__init__.pyi");
        let lua_typings = self.roots.data.join("lua-typings/pokecon.d.lua");
        write_generated(&python_typings, &dynamic_python_typings()?)?;
        write_generated(&lua_typings, &dynamic_lua_typings()?)?;
        let command_typings = commands_python_typings()?
            .into_iter()
            .map(|typing| {
                let path = self.roots.data.join("typings").join(typing.relative_path());
                write_generated(&path, typing.source())?;
                Ok(path)
            })
            .collect::<Result<Vec<_>, ScaffoldError>>()?;
        Ok(ScaffoldPaths {
            global_settings,
            profile_settings,
            init_python,
            init_lua,
            pyproject,
            luarc,
            python_typings,
            lua_typings,
            command_typings,
        })
    }
}

/// Concrete path projection returned after scaffolding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScaffoldPaths {
    pub global_settings: PathBuf,
    pub profile_settings: PathBuf,
    pub init_python: PathBuf,
    pub init_lua: PathBuf,
    pub pyproject: PathBuf,
    pub luarc: PathBuf,
    pub python_typings: PathBuf,
    pub lua_typings: PathBuf,
    pub command_typings: Vec<PathBuf>,
}

fn create_protected_directory(path: &Path) -> Result<(), ScaffoldError> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ScaffoldError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;

        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder
    };
    #[cfg(not(unix))]
    let builder = fs::DirBuilder::new();
    match builder.create(path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(source) => Err(ScaffoldError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn create_editable_file(path: &Path, source: &str) -> Result<(), ScaffoldError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(mut file) => file
            .write_all(source.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|source| ScaffoldError::Io {
                path: path.to_path_buf(),
                source,
            }),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(source) => Err(ScaffoldError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn write_generated(path: &Path, source: &str) -> Result<(), ScaffoldError> {
    let parent = path
        .parent()
        .ok_or_else(|| ScaffoldError::InvalidPath(path.to_path_buf()))?;
    fs::create_dir_all(parent).map_err(|source| ScaffoldError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let mut file = AtomicWriteFile::open(path).map_err(|source| ScaffoldError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    file.write_all(source.as_bytes())
        .map_err(|source| ScaffoldError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    file.commit().map_err(|source| ScaffoldError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn initial_pyproject(roots: &EffectiveRoots) -> String {
    let typings = serde_json::to_string(&roots.data.join("typings").to_string_lossy())
        .expect("path string must serialize");
    let venv = serde_json::to_string(&roots.data.join("venv-script").to_string_lossy())
        .expect("path string must serialize");
    format!(
        "# User-editable PokeCon LSP configuration. This file is created once.\n\
         [tool.basedpyright]\nextraPaths = [{typings}]\nvenvPath = {venv}\n\n\
         [tool.pyright]\nextraPaths = [{typings}]\nvenvPath = {venv}\n\n\
         [tool.mypy]\nmypy_path = {typings}\n\n\
         [tool.ruff]\nsrc = [{typings}]\n"
    )
}

fn initial_luarc(roots: &EffectiveRoots) -> Result<String, ScaffoldError> {
    serde_json::to_string_pretty(&serde_json::json!({
        "workspace.library": [roots.data.join("lua-typings").to_string_lossy()],
        "type.checkTableShape": true
    }))
    .map(|mut source| {
        source.push('\n');
        source
    })
    .map_err(ScaffoldError::Json)
}

/// Scaffolding failures.
#[derive(Debug, Error)]
pub enum ScaffoldError {
    #[error(transparent)]
    Contract(#[from] ContractError),
    #[error(transparent)]
    Root(#[from] RootError),
    #[error("invalid scaffold path {0}")]
    InvalidPath(PathBuf),
    #[error("scaffold JSON generation failed")]
    Json(#[source] serde_json::Error),
    #[error("scaffold I/O failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::ScaffoldManager;
    use crate::settings::roots::{BaseDirectories, EffectiveRoots, RootEnvironment, SafeComponent};

    #[test]
    fn editable_and_generated_assets_are_separated_and_edits_survive() {
        let temporary = TempDir::new().expect("temporary directory must exist");
        let base = temporary.path();
        let bases = BaseDirectories::linux(&RootEnvironment::from_values([
            ("HOME", base.as_os_str().to_os_string()),
            ("XDG_CONFIG_HOME", base.join("config").into_os_string()),
            ("XDG_DATA_HOME", base.join("data").into_os_string()),
            ("XDG_CACHE_HOME", base.join("cache").into_os_string()),
            ("XDG_STATE_HOME", base.join("state").into_os_string()),
        ]))
        .expect("bases must resolve");
        let roots = EffectiveRoots::from_bases(
            SafeComponent::new("pokecon").expect("name must be safe"),
            &bases,
        );
        let manager = ScaffoldManager::new(roots.clone());
        let paths = manager.ensure("Profile").expect("scaffold must succeed");
        assert!(paths.pyproject.starts_with(&roots.config));
        assert!(paths.init_python.starts_with(&roots.config));
        assert!(paths.profile_settings.starts_with(&roots.config));
        assert!(paths.python_typings.starts_with(&roots.data));
        assert!(paths.lua_typings.starts_with(&roots.data));
        assert!(
            paths
                .command_typings
                .iter()
                .all(|path| path.starts_with(roots.data.join("typings")))
        );
        assert!(paths.command_typings.iter().any(|path| {
            path.ends_with("Commands/PythonCommandBase.pyi")
                && fs::read_to_string(path)
                    .is_ok_and(|source| source.contains("class ImageProcPythonCommand"))
        }));
        fs::write(&paths.init_python, "# user edit\n").expect("edit must succeed");
        manager
            .ensure("Profile")
            .expect("second scaffold must succeed");
        assert_eq!(
            fs::read_to_string(paths.init_python).expect("init must be readable"),
            "# user edit\n"
        );
        let python_types =
            fs::read_to_string(paths.python_typings).expect("typings must be readable");
        assert!(python_types.contains("type Language = Literal["));
        assert!(python_types.contains("class _OptPythonScriptPackages:"));
        assert!(!python_types.contains("\"JA\""));
    }
}
