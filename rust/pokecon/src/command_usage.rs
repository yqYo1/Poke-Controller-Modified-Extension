//! Command usage counts (frequency store).
//!
//! Mirrors the upstream `command_usage` service: the same command is keyed by
//! (command type, NAME, file path relative to the command root), counts live
//! in a profile-independent JSON file (`profiles/command_usage.json`), and
//! writes go through a temporary file plus a rename so an interrupted write
//! never corrupts the store. The upstream guards cross-window writes with a
//! lock file; this application is single-instance, so a process mutex covers
//! the same guarantee.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;

/// Storage format version (upstream `FORMAT_VERSION`).
const FORMAT_VERSION: u32 = 1;

/// One command identity: type ("Python" / "Mcu"), the NAME, and the file path
/// relative to the command root.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct UsageKey {
    pub command_type: String,
    pub name: String,
    pub file: String,
}

/// The on-disk document: `{"version": 1, "commands": [...]}`.
#[derive(Debug, Default, Serialize)]
struct UsageDocument {
    version: u32,
    commands: Vec<UsageRow>,
}

#[derive(Debug, Serialize)]
struct UsageRow {
    #[serde(rename = "type")]
    command_type: String,
    name: String,
    file: String,
    count: u64,
}

/// Records command usage counts in one JSON file.
#[derive(Debug)]
pub struct CommandUsageStore {
    path: PathBuf,
    counts: Mutex<BTreeMap<UsageKey, u64>>,
}

impl CommandUsageStore {
    /// Loads the store from `path`, returning an empty store when the file is
    /// missing or unreadable (the upstream returns an empty table as well).
    #[must_use]
    pub fn load(path: PathBuf) -> Self {
        let counts = read_counts(&path).unwrap_or_default();
        Self {
            path,
            counts: Mutex::new(counts),
        }
    }

    /// The store path.
    #[must_use]
    #[allow(
        dead_code,
        reason = "consumed by the usage-count API surface in the next step"
    )]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Increments the count for `key` and rewrites the store.
    ///
    /// Returns `false` when the store cannot be written; the caller must not
    /// treat that as a command failure (upstream logs and continues).
    #[must_use]
    pub fn record(&self, key: &UsageKey) -> bool {
        let Ok(mut counts) = self.counts.lock() else {
            return false;
        };
        *counts.entry(key.clone()).or_insert(0) += 1;
        write_counts(&self.path, &counts).is_ok()
    }

    /// The recorded count for one key.
    #[must_use]
    #[allow(
        dead_code,
        reason = "consumed by the usage-count API surface in the next step"
    )]
    pub fn count_of(&self, key: &UsageKey) -> u64 {
        self.counts
            .lock()
            .map_or(0, |counts| counts.get(key).copied().unwrap_or(0))
    }

    /// A snapshot of all counts.
    #[must_use]
    #[allow(
        dead_code,
        reason = "consumed by the usage-count API surface in the next step"
    )]
    pub fn snapshot(&self) -> BTreeMap<UsageKey, u64> {
        self.counts
            .lock()
            .map_or_else(|_| BTreeMap::new(), |counts| counts.clone())
    }
}

/// Reads counts from the store file.
///
/// Mirrors the upstream tolerance: the document must be an object with a
/// `commands` array, but individual malformed rows are skipped while the
/// valid ones are kept.
fn read_counts(path: &Path) -> Result<BTreeMap<UsageKey, u64>, std::io::Error> {
    let text = std::fs::read_to_string(path)?;
    let document: serde_json::Value = serde_json::from_str(&text).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("unexpected command usage format: {error}"),
        )
    })?;
    let Some(rows) = document
        .get("commands")
        .and_then(serde_json::Value::as_array)
    else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "unexpected command usage format: commands is not an array",
        ));
    };
    let mut counts: BTreeMap<UsageKey, u64> = BTreeMap::new();
    for row in rows {
        let Some(command_type) = row.get("type").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(name) = row.get("name").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(file) = row.get("file").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(count) = row.get("count").and_then(count_value) else {
            continue;
        };
        if count == 0 {
            continue;
        }
        let key = UsageKey {
            command_type: command_type.to_owned(),
            name: name.to_owned(),
            file: file.to_owned(),
        };
        *counts.entry(key).or_insert(0) += count;
    }
    Ok(counts)
}

/// Reads a count value, accepting integers and numeric strings like the
/// upstream `int()` conversion.
fn count_value(value: &serde_json::Value) -> Option<u64> {
    if let Some(number) = value.as_u64() {
        return Some(number);
    }
    value.as_str().and_then(|text| text.trim().parse().ok())
}

/// Writes counts atomically: a temporary file first, then a rename.
fn write_counts(path: &Path, counts: &BTreeMap<UsageKey, u64>) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent)?;
    }
    let document = UsageDocument {
        version: FORMAT_VERSION,
        commands: counts
            .iter()
            .map(|(key, count)| UsageRow {
                command_type: key.command_type.clone(),
                name: key.name.clone(),
                file: key.file.clone(),
                count: *count,
            })
            .collect(),
    };
    let text = serde_json::to_string_pretty(&document).map_err(std::io::Error::other)?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temporary, text)?;
    match std::fs::rename(&temporary, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(&temporary);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: &str) -> UsageKey {
        UsageKey {
            command_type: "Python".to_owned(),
            name: name.to_owned(),
            file: format!("samples/{name}.py"),
        }
    }

    #[test]
    fn record_persists_and_reloads_counts() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("profiles/command_usage.json");
        let store = CommandUsageStore::load(path.clone());
        assert!(store.record(&key("first")));
        assert!(store.record(&key("first")));
        assert!(store.record(&key("second")));
        assert_eq!(store.count_of(&key("first")), 2);
        assert_eq!(store.count_of(&key("second")), 1);

        let reloaded = CommandUsageStore::load(path);
        assert_eq!(reloaded.count_of(&key("first")), 2);
        assert_eq!(reloaded.count_of(&key("second")), 1);
    }

    #[test]
    fn missing_file_starts_empty() {
        let directory = tempfile::tempdir().expect("tempdir");
        let store = CommandUsageStore::load(directory.path().join("profiles/command_usage.json"));
        assert_eq!(store.count_of(&key("first")), 0);
        assert!(store.snapshot().is_empty());
    }

    #[test]
    fn malformed_file_starts_empty() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("command_usage.json");
        std::fs::write(&path, "not json").expect("write");
        let store = CommandUsageStore::load(path);
        assert!(store.snapshot().is_empty());
    }

    #[test]
    fn zero_and_unknown_rows_are_ignored() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("command_usage.json");
        std::fs::write(
            &path,
            r#"{"version":1,"commands":[
                {"type":"Python","name":"first","file":"samples/first.py","count":3},
                {"type":"Python","name":"zero","file":"samples/zero.py","count":0},
                {"type":"Python","name":"bad"}
            ]}"#,
        )
        .expect("write");
        let store = CommandUsageStore::load(path);
        assert_eq!(store.count_of(&key("first")), 3);
        assert_eq!(store.count_of(&key("zero")), 0);
        assert_eq!(store.snapshot().len(), 1);
    }
}
