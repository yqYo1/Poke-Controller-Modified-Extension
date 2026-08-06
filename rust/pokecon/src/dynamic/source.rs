use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::settings::path::lexical_normalize;
use atomic_write_file::OpenOptions;
use parking_lot::Mutex;

use crate::dynamic::control::{DynamicConfigLanguage, DynamicSource};

/// Fully resolved, cycle-comparable source file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedSource {
    pub path: PathBuf,
    pub language: DynamicConfigLanguage,
    pub display_path: String,
}

/// Dynamic source path, persistence, or cycle failure.
#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("dynamic source path must not be empty")]
    Empty,
    #[error("dynamic source must have a .py or .lua extension")]
    UnsupportedExtension,
    #[error("relative dynamic source escapes the Config root")]
    RelativeEscape,
    #[error("dynamic source cycle detected at {0}")]
    Cycle(String),
    #[error("there is no successfully loaded dynamic source to reload")]
    NoCurrentSource,
    #[error("dynamic source is not a regular file: {0}")]
    NotFile(String),
    #[error("dynamic source I/O failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Config-root-aware path resolver, atomic content store, reload pointer, and
/// normalized source stack.
#[derive(Debug)]
pub struct SourceStore {
    config_root: PathBuf,
    canonical_config_root: PathBuf,
    home: Option<PathBuf>,
    current: Mutex<Option<DynamicSource>>,
    stack: Mutex<Vec<PathBuf>>,
}

impl SourceStore {
    /// Creates a store and ensures the Config root exists.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the Config root cannot be created or resolved.
    pub fn new(
        config_root: impl Into<PathBuf>,
        home: Option<PathBuf>,
    ) -> Result<Self, SourceError> {
        let config_root = absolute_lexical(config_root.into())?;
        fs::create_dir_all(&config_root).map_err(|source| SourceError::Io {
            path: config_root.clone(),
            source,
        })?;
        let canonical_config_root =
            fs::canonicalize(&config_root).map_err(|source| SourceError::Io {
                path: config_root.clone(),
                source,
            })?;
        Ok(Self {
            config_root,
            canonical_config_root,
            home,
            current: Mutex::new(None),
            stack: Mutex::new(Vec::new()),
        })
    }

    #[must_use]
    pub fn config_root(&self) -> &Path {
        &self.config_root
    }

    /// Resolves one user-supplied path with Config-root containment for relative
    /// values and unrestricted explicit absolute paths.
    ///
    /// # Errors
    ///
    /// Rejects missing/non-file paths, unsupported extensions, and relative
    /// lexical or symlink escapes.
    pub fn resolve(&self, raw: &str) -> Result<ResolvedSource, SourceError> {
        if raw.is_empty() {
            return Err(SourceError::Empty);
        }
        let expanded = self.expand_tilde(raw)?;
        let input = expanded;
        let relative = !input.is_absolute();
        let lexical = lexical_normalize(if relative {
            self.config_root.join(input)
        } else {
            input
        });
        if relative && !lexical.starts_with(&self.config_root) {
            return Err(SourceError::RelativeEscape);
        }
        if !lexical.is_file() {
            return Err(SourceError::NotFile(redacted_path(
                &lexical,
                &self.config_root,
            )));
        }
        let canonical = fs::canonicalize(&lexical).map_err(|source| SourceError::Io {
            path: lexical.clone(),
            source,
        })?;
        if relative && !canonical.starts_with(&self.canonical_config_root) {
            return Err(SourceError::RelativeEscape);
        }
        let language = DynamicConfigLanguage::from_path(&canonical)
            .ok_or(SourceError::UnsupportedExtension)?;
        Ok(ResolvedSource {
            display_path: redacted_path(&canonical, &self.canonical_config_root),
            path: canonical,
            language,
        })
    }

    /// Atomically writes browser-provided content to the canonical init file and
    /// returns its resolved identity.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the complete file cannot be atomically replaced.
    pub fn save_content(
        &self,
        language: DynamicConfigLanguage,
        content: &str,
    ) -> Result<ResolvedSource, SourceError> {
        let path = self
            .config_root
            .join(format!("init.{}", language.extension()));
        atomic_write(&path, content.as_bytes())?;
        self.resolve(path.to_string_lossy().as_ref())
    }

    /// Reads a previously resolved regular source file as UTF-8.
    ///
    /// # Errors
    ///
    /// Returns an I/O error for unreadable or non-UTF-8 content.
    pub fn read(&self, source_file: &ResolvedSource) -> Result<String, SourceError> {
        fs::read_to_string(&source_file.path).map_err(|source| SourceError::Io {
            path: source_file.path.clone(),
            source,
        })
    }

    /// Records a source only after its complete evaluation commits.
    pub fn mark_success(&self, source: &ResolvedSource) {
        *self.current.lock() = Some(DynamicSource {
            path: source.path.clone(),
            language: source.language,
        });
    }

    /// Resolves the last successfully selected source again, so deletion and
    /// symlink changes are revalidated at reload time.
    ///
    /// # Errors
    ///
    /// Returns `NoCurrentSource` before a successful load or the normal path
    /// validation error if the file changed.
    pub fn reload_source(&self) -> Result<ResolvedSource, SourceError> {
        let current = self
            .current
            .lock()
            .clone()
            .ok_or(SourceError::NoCurrentSource)?;
        self.resolve(current.path.to_string_lossy().as_ref())
    }

    /// Pushes one canonical identity for nested `source()` cycle detection.
    ///
    /// # Errors
    ///
    /// Returns a cycle error if the same normalized file is already active.
    pub fn push(&self, source: &ResolvedSource) -> Result<SourceGuard<'_>, SourceError> {
        let mut stack = self.stack.lock();
        if stack.contains(&source.path) {
            return Err(SourceError::Cycle(source.display_path.clone()));
        }
        stack.push(source.path.clone());
        drop(stack);
        Ok(SourceGuard { store: self })
    }

    fn expand_tilde(&self, raw: &str) -> Result<PathBuf, SourceError> {
        if !cfg!(target_os = "linux") || !raw.starts_with('~') {
            return Ok(PathBuf::from(raw));
        }
        let suffix = if raw == "~" {
            ""
        } else if let Some(suffix) = raw.strip_prefix("~/") {
            suffix
        } else {
            return Err(SourceError::RelativeEscape);
        };
        let home = self.home.as_ref().ok_or(SourceError::RelativeEscape)?;
        Ok(if suffix.is_empty() {
            home.clone()
        } else {
            home.join(suffix)
        })
    }
}

/// Pops exactly one source identity on success and every error unwind path.
#[derive(Debug)]
pub struct SourceGuard<'a> {
    store: &'a SourceStore,
}

impl Drop for SourceGuard<'_> {
    fn drop(&mut self) {
        self.store.stack.lock().pop();
    }
}

fn absolute_lexical(path: PathBuf) -> Result<PathBuf, SourceError> {
    if path.is_absolute() {
        Ok(lexical_normalize(path))
    } else {
        std::env::current_dir()
            .map(|current| lexical_normalize(current.join(path)))
            .map_err(|source| SourceError::Io {
                path: PathBuf::from("."),
                source,
            })
    }
}

fn redacted_path(path: &Path, config_root: &Path) -> String {
    path.strip_prefix(config_root).map_or_else(
        |_| {
            path.file_name().map_or_else(
                || "<external>".to_owned(),
                |name| name.to_string_lossy().into_owned(),
            )
        },
        |relative| relative.to_string_lossy().into_owned(),
    )
}

fn atomic_write(path: &Path, content: &[u8]) -> Result<(), SourceError> {
    let parent = path.parent().ok_or_else(|| SourceError::Io {
        path: path.to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent"),
    })?;
    fs::create_dir_all(parent).map_err(|source| SourceError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    #[cfg(unix)]
    let options = {
        use atomic_write_file::unix::OpenOptionsExt as AtomicOpenOptionsExt;
        use std::os::unix::fs::OpenOptionsExt as StdOpenOptionsExt;

        let mut options = OpenOptions::new();
        options.mode(0o600);
        options.preserve_mode(path.exists());
        options
    };
    #[cfg(not(unix))]
    let options = OpenOptions::new();
    let mut file = options.open(path).map_err(|source| SourceError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    file.write_all(content).map_err(|source| SourceError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    file.commit().map_err(|source| SourceError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    sync_parent(parent)?;
    Ok(())
}

#[cfg(unix)]
fn sync_parent(parent: &Path) -> Result<(), SourceError> {
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| SourceError::Io {
            path: parent.to_path_buf(),
            source,
        })
}

#[cfg(not(unix))]
fn sync_parent(_parent: &Path) -> Result<(), SourceError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn relative_paths_are_config_based_and_cycle_identity_is_canonical() {
        let temporary = TempDir::new().unwrap();
        let config = temporary.path().join("config");
        let store = SourceStore::new(&config, Some(temporary.path().to_path_buf())).unwrap();
        fs::write(config.join("plugin.py"), "value = 1\n").unwrap();
        let source = store.resolve("./nested/../plugin.py").unwrap();
        assert_eq!(source.display_path, "plugin.py");
        let _guard = store.push(&source).unwrap();
        assert!(matches!(store.push(&source), Err(SourceError::Cycle(_))));
    }

    #[test]
    fn content_save_is_atomic_and_failed_load_does_not_change_reload_pointer() {
        let temporary = TempDir::new().unwrap();
        let store = SourceStore::new(temporary.path().join("config"), None).unwrap();
        assert!(matches!(
            store.reload_source(),
            Err(SourceError::NoCurrentSource)
        ));
        let source = store
            .save_content(DynamicConfigLanguage::Lua, "return true\n")
            .unwrap();
        assert_eq!(store.read(&source).unwrap(), "return true\n");
        assert!(matches!(
            store.reload_source(),
            Err(SourceError::NoCurrentSource)
        ));
        store.mark_success(&source);
        assert_eq!(store.reload_source().unwrap().path, source.path);
    }

    #[cfg(unix)]
    #[test]
    fn relative_symlink_escape_is_rejected_but_absolute_external_path_is_allowed() {
        use std::os::unix::fs::symlink;

        let temporary = TempDir::new().unwrap();
        let config = temporary.path().join("config");
        let outside = temporary.path().join("outside.py");
        fs::write(&outside, "pass\n").unwrap();
        let store = SourceStore::new(&config, None).unwrap();
        symlink(&outside, config.join("escape.py")).unwrap();
        assert!(matches!(
            store.resolve("escape.py"),
            Err(SourceError::RelativeEscape)
        ));
        assert_eq!(
            store
                .resolve(outside.to_string_lossy().as_ref())
                .unwrap()
                .path,
            fs::canonicalize(outside).unwrap()
        );
    }
}
