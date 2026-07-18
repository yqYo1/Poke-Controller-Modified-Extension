use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use atomic_write_file::OpenOptions;
use serde_json::Value as JsonValue;
use thiserror::Error;
use toml::Value as TomlValue;
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value};

use crate::lock::{LockError, LockManager};

/// A format-preserving, cross-process-safe TOML settings store.
#[derive(Clone, Debug)]
pub struct TomlStore {
    locks: LockManager,
}

impl TomlStore {
    #[must_use]
    pub const fn new(locks: LockManager) -> Self {
        Self { locks }
    }

    /// Reads a settings document without creating it.
    ///
    /// # Errors
    ///
    /// Returns an error for I/O or invalid TOML. File content is never included
    /// in the error.
    pub fn read(&self, path: &Path) -> Result<SettingsDocument, PersistenceError> {
        read_document(path)
    }

    /// Applies a set of dotted-path leaf updates as one atomic transaction.
    ///
    /// Unknown keys, comments, and existing ordering remain intact. A JSON null
    /// removes the TOML key because TOML has no null scalar.
    ///
    /// # Errors
    ///
    /// Returns an error without replacing the prior file if locking, parsing,
    /// conversion, write, sync, or atomic commit fails.
    pub fn update(
        &self,
        path: &Path,
        updates: &[(String, JsonValue)],
    ) -> Result<SettingsDocument, PersistenceError> {
        let _guard = self.locks.settings(path)?;
        let mut document = read_document(path)?;
        for (dotted_path, value) in updates {
            update_leaf(&mut document.document, dotted_path, value)?;
        }
        write_document(path, &document.document)?;
        document.parsed = parse_toml_value(path, &document.document.to_string())?;
        Ok(document)
    }
}

/// Both the editable syntax tree and semantic TOML value for one snapshot.
#[derive(Clone)]
pub struct SettingsDocument {
    path: PathBuf,
    document: DocumentMut,
    parsed: TomlValue,
}

impl fmt::Debug for SettingsDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SettingsDocument")
            .field("path", &self.path)
            .field("document", &"<redacted>")
            .field("parsed", &"<redacted>")
            .finish()
    }
}

impl SettingsDocument {
    #[must_use]
    pub const fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Looks up one dotted semantic path.
    #[must_use]
    pub fn get(&self, dotted_path: &str) -> Option<&TomlValue> {
        let mut value = &self.parsed;
        for segment in dotted_path.split('.') {
            value = value.get(segment)?;
        }
        Some(value)
    }

    /// Returns the format-preserved serialization for contract tests. The
    /// content can contain secrets and must never enter diagnostics.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn source(&self) -> String {
        self.document.to_string()
    }
}

fn read_document(path: &Path) -> Result<SettingsDocument, PersistenceError> {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(PersistenceError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let document = source
        .parse::<DocumentMut>()
        .map_err(|_| PersistenceError::InvalidToml(path.to_path_buf()))?;
    let parsed = parse_toml_value(path, &source)?;
    Ok(SettingsDocument {
        path: path.to_path_buf(),
        document,
        parsed,
    })
}

fn parse_toml_value(path: &Path, source: &str) -> Result<TomlValue, PersistenceError> {
    if source.trim().is_empty() {
        return Ok(TomlValue::Table(toml::map::Map::new()));
    }
    source
        .parse::<TomlValue>()
        .map_err(|_| PersistenceError::InvalidToml(path.to_path_buf()))
}

fn update_leaf(
    document: &mut DocumentMut,
    dotted_path: &str,
    value: &JsonValue,
) -> Result<(), PersistenceError> {
    let segments = dotted_path.split('.').collect::<Vec<_>>();
    let (leaf, parents) = segments
        .split_last()
        .ok_or_else(|| PersistenceError::InvalidPath(dotted_path.to_owned()))?;
    if leaf.is_empty() || parents.iter().any(|segment| segment.is_empty()) {
        return Err(PersistenceError::InvalidPath(dotted_path.to_owned()));
    }
    let mut table = document.as_table_mut();
    for segment in parents {
        if !table.contains_key(segment) {
            let mut nested = Table::new();
            nested.set_implicit(true);
            table.insert(segment, Item::Table(nested));
        }
        table = table
            .get_mut(segment)
            .and_then(Item::as_table_mut)
            .ok_or_else(|| PersistenceError::PathCollision(dotted_path.to_owned()))?;
    }
    if value.is_null() {
        table.remove(leaf);
    } else if let Some(existing) = table.get_mut(leaf) {
        let mut replacement = json_to_item(value)?;
        if let (Some(current), Some(next)) = (existing.as_value(), replacement.as_value_mut()) {
            *next.decor_mut() = current.decor().clone();
        }
        *existing = replacement;
    } else {
        table.insert(leaf, json_to_item(value)?);
    }
    Ok(())
}

fn json_to_item(value: &JsonValue) -> Result<Item, PersistenceError> {
    match value {
        JsonValue::Null => Err(PersistenceError::UnsupportedValue),
        JsonValue::Bool(value) => Ok(Item::Value(Value::from(*value))),
        JsonValue::Number(value) => {
            if let Some(integer) = value.as_i64() {
                Ok(Item::Value(Value::from(integer)))
            } else if let Some(float) = value.as_f64() {
                Ok(Item::Value(Value::from(float)))
            } else {
                Err(PersistenceError::UnsupportedValue)
            }
        }
        JsonValue::String(value) => Ok(Item::Value(Value::from(value.as_str()))),
        JsonValue::Array(values) => {
            if values.iter().all(JsonValue::is_object) {
                let mut array = ArrayOfTables::new();
                for value in values {
                    let object = value.as_object().expect("array item was checked as object");
                    let mut table = Table::new();
                    for (name, child) in object {
                        table.insert(name, json_to_item(child)?);
                    }
                    array.push(table);
                }
                Ok(Item::ArrayOfTables(array))
            } else {
                let mut array = Array::new();
                for child in values {
                    let item = json_to_item(child)?;
                    let value = item
                        .into_value()
                        .map_err(|_| PersistenceError::UnsupportedValue)?;
                    array.push(value);
                }
                Ok(Item::Value(Value::Array(array)))
            }
        }
        JsonValue::Object(values) => {
            let mut table = InlineTable::new();
            for (name, child) in values {
                let item = json_to_item(child)?;
                let value = item
                    .into_value()
                    .map_err(|_| PersistenceError::UnsupportedValue)?;
                table.insert(name, value);
            }
            Ok(Item::Value(Value::InlineTable(table)))
        }
    }
}

fn write_document(path: &Path, document: &DocumentMut) -> Result<(), PersistenceError> {
    let parent = path
        .parent()
        .ok_or_else(|| PersistenceError::InvalidPath(path.display().to_string()))?;
    fs::create_dir_all(parent).map_err(|source| PersistenceError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    #[cfg(unix)]
    let options = {
        use atomic_write_file::unix::OpenOptionsExt as AtomicOpenOptionsExt;
        use std::os::unix::fs::OpenOptionsExt as StdOpenOptionsExt;

        let existed = path.exists();
        let mut options = OpenOptions::new();
        options.mode(0o600);
        options.preserve_mode(existed);
        options
    };
    #[cfg(not(unix))]
    let options = OpenOptions::new();
    let mut file = options.open(path).map_err(|source| PersistenceError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    file.write_all(document.to_string().as_bytes())
        .map_err(|source| PersistenceError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    file.commit().map_err(|source| PersistenceError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    sync_parent(parent)?;
    Ok(())
}

#[cfg(unix)]
fn sync_parent(parent: &Path) -> Result<(), PersistenceError> {
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| PersistenceError::Io {
            path: parent.to_path_buf(),
            source,
        })
}

#[cfg(not(unix))]
fn sync_parent(_parent: &Path) -> Result<(), PersistenceError> {
    Ok(())
}

/// Atomic settings persistence failures. Values are intentionally omitted.
#[derive(Debug, Error)]
pub enum PersistenceError {
    #[error(transparent)]
    Lock(#[from] LockError),
    #[error("settings file is not valid TOML: {0}")]
    InvalidToml(PathBuf),
    #[error("invalid dotted TOML path {0}")]
    InvalidPath(String),
    #[error("TOML path collides with a non-table value: {0}")]
    PathCollision(String),
    #[error("setting value cannot be represented in TOML")]
    UnsupportedValue,
    #[error("settings I/O failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::TempDir;

    use super::TomlStore;
    use crate::lock::LockManager;
    use crate::roots::{BaseDirectories, EffectiveRoots, SafeComponent};

    fn roots(temp: &TempDir) -> EffectiveRoots {
        let base = temp.path();
        let bases = BaseDirectories::linux(&crate::roots::RootEnvironment::from_values([
            ("HOME", base.as_os_str().to_os_string()),
            ("XDG_CONFIG_HOME", base.join("config").into_os_string()),
            ("XDG_DATA_HOME", base.join("data").into_os_string()),
            ("XDG_CACHE_HOME", base.join("cache").into_os_string()),
            ("XDG_STATE_HOME", base.join("state").into_os_string()),
        ]))
        .expect("bases must resolve");
        EffectiveRoots::from_bases(
            SafeComponent::new("pokecon").expect("name must be safe"),
            &bases,
        )
    }

    #[test]
    fn updates_preserve_comments_unknown_keys_and_order() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let roots = roots(&temp);
        roots.ensure().expect("roots must be created");
        let path = roots.config.join("settings.toml");
        fs::write(
            &path,
            "# retained\n[unknown]\nanswer = 42\n\n[server]\n# port comment\nport = 8020\n",
        )
        .expect("fixture must be writable");
        let store = TomlStore::new(LockManager::new(&roots));
        let snapshot = store
            .update(&path, &[("server.port".to_owned(), json!(9000))])
            .expect("update must succeed");
        let source = snapshot.source();
        assert!(source.contains("# retained"));
        assert!(source.contains("answer = 42"));
        assert!(source.contains("# port comment"));
        assert!(source.contains("port = 9000"));
        assert!(source.find("[unknown]") < source.find("[server]"));
        assert!(!format!("{snapshot:?}").contains("answer = 42"));
    }

    #[cfg(unix)]
    #[test]
    fn new_secret_capable_settings_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().expect("temporary directory must exist");
        let roots = roots(&temp);
        roots.ensure().expect("roots must be created");
        let path = roots.config.join("settings.toml");
        TomlStore::new(LockManager::new(&roots))
            .update(&path, &[("global.language".to_owned(), json!("ja"))])
            .expect("update must succeed");
        let mode = fs::metadata(path)
            .expect("metadata must be readable")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}
