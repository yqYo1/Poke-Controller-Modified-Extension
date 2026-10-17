use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// A validated application or profile directory name.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SafeComponent(String);

impl SafeComponent {
    /// Validates a platform-independent, single normal path component.
    ///
    /// The spelling is preserved exactly: no trimming, case folding, or
    /// Unicode normalization is performed.
    ///
    /// # Errors
    ///
    /// Returns [`RootError::UnsafeComponent`] for an empty value, dot
    /// component, separator, NUL, Windows drive/UNC form, or absolute path.
    pub fn new(value: impl Into<String>) -> Result<Self, RootError> {
        let value = value.into();
        if value.is_empty()
            || value == "."
            || value == ".."
            || value.contains(['/', '\\', '\0', ':'])
        {
            return Err(RootError::UnsafeComponent);
        }
        Ok(Self(value))
    }

    /// Returns the original, unnormalized spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<OsStr> for SafeComponent {
    fn as_ref(&self) -> &OsStr {
        OsStr::new(&self.0)
    }
}

/// Environment snapshot used to make root resolution deterministic and testable.
#[derive(Clone, Default)]
pub struct RootEnvironment {
    values: BTreeMap<String, OsString>,
}

impl fmt::Debug for RootEnvironment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RootEnvironment")
            .field("variable_count", &self.values.len())
            .field("values", &"<redacted>")
            .finish()
    }
}

impl RootEnvironment {
    /// Captures the current process environment.
    #[must_use]
    pub fn current() -> Self {
        Self {
            values: std::env::vars_os()
                .filter_map(|(name, value)| name.into_string().ok().map(|name| (name, value)))
                .collect(),
        }
    }

    /// Creates an environment snapshot from explicit values.
    #[must_use]
    pub fn from_values(
        values: impl IntoIterator<Item = (impl Into<String>, impl Into<OsString>)>,
    ) -> Self {
        Self {
            values: values
                .into_iter()
                .map(|(name, value)| (name.into(), value.into()))
                .collect(),
        }
    }

    /// Returns an environment value.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&OsStr> {
        self.values.get(name).map(OsString::as_os_str)
    }

    /// Iterates over captured names and values.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &OsStr)> {
        self.values
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_os_str()))
    }
}

/// Platform base directories before the application-name suffix is added.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BaseDirectories {
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
    pub state: PathBuf,
    layout: BaseLayout,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BaseLayout {
    AppSuffix,
    WindowsPurposeSuffix,
}

impl BaseDirectories {
    /// Resolves Linux XDG bases, applying the required absolute-path fallback.
    ///
    /// # Errors
    ///
    /// Returns [`RootError::MissingHome`] when `HOME` is unavailable or not
    /// absolute.
    pub fn linux(environment: &RootEnvironment) -> Result<Self, RootError> {
        let home = environment
            .get("HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or(RootError::MissingHome)?;
        Ok(Self {
            config: xdg_or_fallback(environment, "XDG_CONFIG_HOME", &home.join(".config")),
            data: xdg_or_fallback(environment, "XDG_DATA_HOME", &home.join(".local/share")),
            cache: xdg_or_fallback(environment, "XDG_CACHE_HOME", &home.join(".cache")),
            state: xdg_or_fallback(environment, "XDG_STATE_HOME", &home.join(".local/state")),
            layout: BaseLayout::AppSuffix,
        })
    }

    /// Resolves Windows Roaming/Local `AppData` bases.
    ///
    /// # Errors
    ///
    /// Returns [`RootError::MissingKnownFolder`] if either base is absent or
    /// not absolute.
    pub fn windows(environment: &RootEnvironment) -> Result<Self, RootError> {
        let roaming = absolute_environment_path(environment, "APPDATA")?;
        let local = absolute_environment_path(environment, "LOCALAPPDATA")?;
        Ok(Self {
            config: roaming,
            data: local.clone(),
            cache: local.clone(),
            state: local,
            layout: BaseLayout::WindowsPurposeSuffix,
        })
    }

    /// Resolves bases for the current supported target.
    ///
    /// # Errors
    ///
    /// Returns a platform-specific missing-base error.
    pub fn native(environment: &RootEnvironment) -> Result<Self, RootError> {
        #[cfg(target_os = "windows")]
        {
            Self::windows(environment)
        }
        #[cfg(not(target_os = "windows"))]
        {
            Self::linux(environment)
        }
    }
}

/// Effective Config/Data/Cache/State roots for one `app_name`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectiveRoots {
    pub app_name: SafeComponent,
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
    pub state: PathBuf,
}

impl EffectiveRoots {
    /// Appends one validated application component to all four bases.
    #[must_use]
    pub fn from_bases(app_name: SafeComponent, bases: &BaseDirectories) -> Self {
        let app_data = bases.data.join(app_name.as_str());
        let app_cache = bases.cache.join(app_name.as_str());
        let app_state = bases.state.join(app_name.as_str());
        Self {
            config: bases.config.join(app_name.as_str()),
            data: match bases.layout {
                BaseLayout::AppSuffix => app_data,
                BaseLayout::WindowsPurposeSuffix => app_data.join("data"),
            },
            cache: match bases.layout {
                BaseLayout::AppSuffix => app_cache,
                BaseLayout::WindowsPurposeSuffix => app_cache.join("cache"),
            },
            state: match bases.layout {
                BaseLayout::AppSuffix => app_state,
                BaseLayout::WindowsPurposeSuffix => app_state.join("state"),
            },
            app_name,
        }
    }

    /// Resolves roots using the current target's platform mapping.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsafe application name or missing platform
    /// base directory.
    pub fn native(app_name: &str, environment: &RootEnvironment) -> Result<Self, RootError> {
        let app_name = SafeComponent::new(app_name)?;
        let bases = BaseDirectories::native(environment)?;
        Ok(Self::from_bases(app_name, &bases))
    }

    /// Returns the active profile directory without permitting traversal.
    ///
    /// # Errors
    ///
    /// Returns an error when the profile is not one safe component.
    pub fn profile_dir(&self, profile: &str) -> Result<PathBuf, RootError> {
        let profile = SafeComponent::new(profile)?;
        Ok(self.config.join("profiles").join(profile.as_str()))
    }

    /// Returns the active profile settings file.
    ///
    /// # Errors
    ///
    /// Returns an error when the profile is unsafe.
    pub fn profile_settings(&self, profile: &str) -> Result<PathBuf, RootError> {
        Ok(self.profile_dir(profile)?.join("settings.toml"))
    }

    /// Creates the four roots, protecting only a newly-created Config root.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if any directory cannot be created.
    pub fn ensure(&self) -> Result<(), RootError> {
        create_config_root(&self.config)?;
        for root in [&self.data, &self.cache, &self.state] {
            fs::create_dir_all(root).map_err(|source| RootError::Io {
                path: root.clone(),
                source,
            })?;
        }
        Ok(())
    }
}

fn xdg_or_fallback(environment: &RootEnvironment, name: &str, fallback: &Path) -> PathBuf {
    environment
        .get(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| fallback.to_path_buf())
}

fn absolute_environment_path(
    environment: &RootEnvironment,
    name: &'static str,
) -> Result<PathBuf, RootError> {
    environment
        .get(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or(RootError::MissingKnownFolder(name))
}

fn create_config_root(path: &Path) -> Result<(), RootError> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| RootError::Io {
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
        Err(source) => Err(RootError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Root and safe-component failures.
#[derive(Debug, Error)]
pub enum RootError {
    #[error("application/profile name is not one safe path component")]
    UnsafeComponent,
    #[error("HOME is missing or is not an absolute path")]
    MissingHome,
    #[error("Windows known-folder environment {0} is missing or not absolute")]
    MissingKnownFolder(&'static str),
    #[error("filesystem operation failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{BaseDirectories, EffectiveRoots, RootEnvironment, SafeComponent};

    #[test]
    fn safe_components_reject_cross_platform_escape_forms_without_normalizing() {
        for invalid in [
            "",
            ".",
            "..",
            "/absolute",
            "\\absolute",
            "a/b",
            "a\\b",
            "C:drive",
            "\\\\server",
            "nul\0byte",
        ] {
            assert!(SafeComponent::new(invalid).is_err(), "accepted {invalid:?}");
        }
        for valid in ["pokecon", "Foo", " leading", "trailing ", "日本語"] {
            assert_eq!(
                SafeComponent::new(valid)
                    .expect("component must be valid")
                    .as_str(),
                valid
            );
        }
    }

    #[test]
    fn linux_xdg_resolution_falls_back_for_empty_or_relative_values() {
        let environment = RootEnvironment::from_values([
            ("HOME", "/home/tester"),
            ("XDG_CONFIG_HOME", "/xdg/config"),
            ("XDG_DATA_HOME", "relative"),
            ("XDG_CACHE_HOME", ""),
        ]);
        let bases = BaseDirectories::linux(&environment).expect("Linux bases must resolve");
        assert_eq!(bases.config, Path::new("/xdg/config"));
        assert_eq!(bases.data, Path::new("/home/tester/.local/share"));
        assert_eq!(bases.cache, Path::new("/home/tester/.cache"));
        assert_eq!(bases.state, Path::new("/home/tester/.local/state"));
    }

    #[test]
    fn windows_roots_use_roaming_config_and_local_purpose_subdirectories() {
        let environment = RootEnvironment::from_values([
            ("APPDATA", "/windows/roaming"),
            ("LOCALAPPDATA", "/windows/local"),
        ]);
        let bases = BaseDirectories::windows(&environment).expect("Windows bases must resolve");
        let roots = EffectiveRoots::from_bases(
            SafeComponent::new("App").expect("name must be valid"),
            &bases,
        );
        assert_eq!(roots.config, Path::new("/windows/roaming/App"));
        assert_eq!(roots.data, Path::new("/windows/local/App/data"));
        assert_eq!(roots.cache, Path::new("/windows/local/App/cache"));
        assert_eq!(roots.state, Path::new("/windows/local/App/state"));
    }
}
