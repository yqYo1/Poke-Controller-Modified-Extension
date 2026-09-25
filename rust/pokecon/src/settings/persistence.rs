use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use atomic_write_file::OpenOptions;
use serde_json::Value as JsonValue;
use thiserror::Error;
use toml::Value as TomlValue;
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Key, Table, Value};

use crate::settings::lock::{LockError, LockManager};

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
        let serialized = document.document.to_string();
        // Validate the complete post-update document before replacing the
        // previous file. A format-preserving parse alone accepts documents
        // that the semantic TOML deserializer rejects.
        document.parsed = parse_document(path, &serialized, document.document.clone())?;
        write_document(path, &document.document)?;
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
        for segment in Key::parse(dotted_path).ok()? {
            value = value.get(segment.get())?;
        }
        Some(value)
    }

    /// Returns dotted paths for every scalar/array leaf in the document.
    #[must_use]
    pub fn leaf_paths(&self, known: &BTreeSet<String>) -> Vec<String> {
        fn visit(
            value: &TomlValue,
            prefix: &str,
            known: &BTreeSet<String>,
            paths: &mut Vec<String>,
        ) {
            if !prefix.is_empty() && known.contains(prefix) {
                paths.push(prefix.to_owned());
                return;
            }
            match value {
                TomlValue::Table(table) => {
                    if table.is_empty() && !prefix.is_empty() {
                        paths.push(prefix.to_owned());
                    }
                    for (key, child) in table {
                        let path = if prefix.is_empty() {
                            format_key_segment(key)
                        } else {
                            format!("{prefix}.{}", format_key_segment(key))
                        };
                        visit(child, &path, known, paths);
                    }
                }
                TomlValue::Array(values) => {
                    let mut visited_table = false;
                    for (index, child) in values.iter().enumerate() {
                        if matches!(child, TomlValue::Table(_)) {
                            visited_table = true;
                            visit(child, &format!("{prefix}[{index}]"), known, paths);
                        } else {
                            paths.push(prefix.to_owned());
                            return;
                        }
                    }
                    if !visited_table && !prefix.is_empty() {
                        paths.push(prefix.to_owned());
                    }
                }
                TomlValue::String(_)
                | TomlValue::Integer(_)
                | TomlValue::Float(_)
                | TomlValue::Boolean(_)
                | TomlValue::Datetime(_) => paths.push(prefix.to_owned()),
            }
        }

        let mut paths = Vec::new();
        visit(&self.parsed, "", known, &mut paths);
        paths
    }

    /// Returns the format-preserving serialization for contract tests.
    /// The content can contain secrets and must never enter diagnostics.
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
        .map_err(|error| invalid_toml(path, &source, error.span()))?;
    let parsed = parse_document(path, &source, document.clone())?;
    Ok(SettingsDocument {
        path: path.to_path_buf(),
        document,
        parsed,
    })
}

fn parse_document(
    path: &Path,
    source: &str,
    document: DocumentMut,
) -> Result<TomlValue, PersistenceError> {
    if source.trim().is_empty() {
        return Ok(TomlValue::Table(toml::map::Map::new()));
    }
    toml_edit::de::from_document(document).map_err(|error| invalid_toml(path, source, error.span()))
}

fn invalid_toml(
    path: &Path,
    source: &str,
    span: Option<std::ops::Range<usize>>,
) -> PersistenceError {
    let location = span
        .map(|span| {
            let offset = span.start.min(source.len());
            let line = source[..offset]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1;
            let column = source[..offset]
                .rsplit_once('\n')
                .map_or(offset + 1, |(_, remainder)| remainder.len() + 1);
            format!(" (line {line}, column {column})")
        })
        .unwrap_or_default();
    PersistenceError::InvalidToml {
        path: path.to_path_buf(),
        location,
    }
}

fn update_leaf(
    document: &mut DocumentMut,
    dotted_path: &str,
    value: &JsonValue,
) -> Result<(), PersistenceError> {
    let segments = Key::parse(dotted_path)
        .map_err(|_| PersistenceError::InvalidPath(dotted_path.to_owned()))?
        .into_iter()
        .map(|segment| segment.get().to_owned())
        .collect::<Vec<_>>();
    if segments.is_empty() {
        return Err(PersistenceError::InvalidPath(dotted_path.to_owned()));
    }
    update_table(document.as_table_mut(), &segments, dotted_path, value)
}

fn update_table(
    table: &mut Table,
    segments: &[String],
    dotted_path: &str,
    value: &JsonValue,
) -> Result<(), PersistenceError> {
    let (head, rest) = segments
        .split_first()
        .ok_or_else(|| PersistenceError::InvalidPath(dotted_path.to_owned()))?;
    if rest.is_empty() {
        if value.is_null() {
            table.remove(head);
        } else if let Some(existing) = table.get_mut(head) {
            replace_item(existing, value)?;
        } else {
            table.insert(head, json_to_item(value)?);
        }
        return Ok(());
    }
    if !table.contains_key(head) {
        let mut nested = Table::new();
        nested.set_implicit(true);
        table.insert(head, Item::Table(nested));
    }
    let item = table
        .get_mut(head)
        .ok_or_else(|| PersistenceError::PathCollision(dotted_path.to_owned()))?;
    if let Some(child) = item.as_table_mut() {
        return update_table(child, rest, dotted_path, value);
    }
    if let Some(inline) = item.as_value().and_then(Value::as_inline_table).cloned() {
        let mut promoted = Table::new();
        for (key, child) in &inline {
            promoted.insert(key, Item::Value(child.clone()));
        }
        *item = Item::Table(promoted);
        return update_table(
            item.as_table_mut()
                .ok_or_else(|| PersistenceError::PathCollision(dotted_path.to_owned()))?,
            rest,
            dotted_path,
            value,
        );
    }
    Err(PersistenceError::PathCollision(dotted_path.to_owned()))
}

fn replace_item(existing: &mut Item, value: &JsonValue) -> Result<(), PersistenceError> {
    {
        let mut replacement = json_to_item(value)?;
        if let (Some(current), Some(next)) = (existing.as_value(), replacement.as_value_mut()) {
            *next.decor_mut() = current.decor().clone();
        }
        *existing = replacement;
    }
    Ok(())
}

fn format_key_segment(key: &str) -> String {
    if !key.is_empty()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        key.to_owned()
    } else {
        format!("\"{}\"", key.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn json_to_item(value: &JsonValue) -> Result<Item, PersistenceError> {
    match value {
        JsonValue::Null => Err(PersistenceError::UnsupportedValue),
        JsonValue::Bool(value) => Ok(Item::Value(Value::from(*value))),
        JsonValue::Number(value) => {
            if let Some(integer) = value.as_i64() {
                Ok(Item::Value(Value::from(integer)))
            } else if let Some(unsigned) = value.as_u64() {
                i64::try_from(unsigned)
                    .map(|integer| Item::Value(Value::from(integer)))
                    .map_err(|_| PersistenceError::UnsupportedValue)
            } else if let Some(float) = value.as_f64() {
                Ok(Item::Value(Value::from(float)))
            } else {
                Err(PersistenceError::UnsupportedValue)
            }
        }
        JsonValue::String(value) => Ok(Item::Value(Value::from(value.as_str()))),
        JsonValue::Array(values) => {
            if !values.is_empty() && values.iter().all(JsonValue::is_object) {
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
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| PersistenceError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    #[cfg(unix)]
    let options = {
        use atomic_write_file::unix::OpenOptionsExt as AtomicOpenOptionsExt;
        use std::os::unix::fs::OpenOptionsExt as StdOpenOptionsExt;

        let mut options = OpenOptions::new();
        options.mode(0o600);
        // SPECIFICATION_BACKEND §11.4.3.3: new files are created `0600`,
        // while pre-existing files keep their current permission bits
        // through atomic replacement. Never auto-chmod existing files.
        options.preserve_mode(true);
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
    #[error(
        "設定ファイル {path} のTOML構文が正しくありません。table、key、quote、arrayを確認して修正してください。{location}"
    )]
    InvalidToml { path: PathBuf, location: String },
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
    use std::collections::BTreeSet;
    use std::fs;

    use serde_json::json;
    use tempfile::TempDir;

    use super::{PersistenceError, TomlStore, TomlValue};
    use crate::settings::lock::LockManager;
    use crate::settings::roots::{BaseDirectories, EffectiveRoots, SafeComponent};

    fn roots(temp: &TempDir) -> EffectiveRoots {
        let base = temp.path();
        let bases =
            BaseDirectories::linux(&crate::settings::roots::RootEnvironment::from_values([
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

    #[cfg(unix)]
    #[test]
    fn updating_an_existing_settings_file_preserves_insecure_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().expect("temporary directory must exist");
        let roots = roots(&temp);
        roots.ensure().expect("roots must be created");
        let path = roots.config.join("settings.toml");
        fs::write(&path, "[global]\nlanguage = \"ja\"\n").expect("fixture must be writable");
        let mut permissions = fs::metadata(&path)
            .expect("metadata must be readable")
            .permissions();
        permissions.set_mode(0o644);
        fs::set_permissions(&path, permissions).expect("fixture mode must be writable");

        TomlStore::new(LockManager::new(&roots))
            .update(&path, &[("global.language".to_owned(), json!("en"))])
            .expect("update must succeed");
        let content = fs::read_to_string(&path).expect("content must be readable");
        assert!(content.contains("language = \"en\""));
        let mode = fs::metadata(path)
            .expect("metadata must be readable")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o644);
    }

    #[test]
    fn arrays_inline_tables_and_quoted_keys_round_trip() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let roots = roots(&temp);
        roots.ensure().expect("roots must be created");
        let path = roots.config.join("settings.toml");
        let store = TomlStore::new(LockManager::new(&roots));

        let document = store
            .update(&path, &[("items".to_owned(), json!([]))])
            .expect("empty arrays must be representable");
        assert!(matches!(
            document.get("items"),
            Some(TomlValue::Array(values)) if values.is_empty()
        ));

        let _document = store
            .update(&path, &[("profile".to_owned(), json!({"language": "ja"}))])
            .expect("inline tables must be representable");
        let document = store
            .update(&path, &[("profile.region".to_owned(), json!("JP"))])
            .expect("nested updates must promote inline tables safely");
        assert_eq!(
            document.get("profile.language").and_then(TomlValue::as_str),
            Some("ja")
        );
        assert_eq!(
            document.get("profile.region").and_then(TomlValue::as_str),
            Some("JP")
        );

        let document = store
            .update(&path, &[("\"a.b\".\"c.d\"".to_owned(), json!(1))])
            .expect("quoted TOML keys must be addressable");
        assert_eq!(
            document
                .get("\"a.b\".\"c.d\"")
                .and_then(TomlValue::as_integer),
            Some(1)
        );
    }

    #[test]
    fn unsupported_unsigned_integer_is_rejected_without_precision_loss() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let roots = roots(&temp);
        roots.ensure().expect("roots must be created");
        let path = roots.config.join("settings.toml");
        let result = TomlStore::new(LockManager::new(&roots))
            .update(&path, &[("large".to_owned(), json!(u64::MAX))]);
        assert!(matches!(result, Err(PersistenceError::UnsupportedValue)));
        assert!(!path.exists(), "rejected values must not create a file");
    }

    #[test]
    fn leaf_paths_include_unknown_keys_inside_array_tables() {
        let temp = TempDir::new().expect("temporary directory must exist");
        let roots = roots(&temp);
        roots.ensure().expect("roots must be created");
        let path = roots.config.join("settings.toml");
        let document = TomlStore::new(LockManager::new(&roots))
            .update(&path, &[("items".to_owned(), json!([{"unknown": 1}]))])
            .expect("array tables must be representable");
        assert!(
            document
                .leaf_paths(&BTreeSet::new())
                .iter()
                .any(|path| path == "items[0].unknown")
        );
    }
}
