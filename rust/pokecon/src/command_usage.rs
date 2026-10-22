//! Command usage counts and run history.
//!
//! Mirrors the upstream `command_usage` service plus the settings-backed run
//! history: the same command is keyed by (command type, NAME, file path
//! relative to the command root), counts and the last run timestamp live in a
//! profile-independent JSON file (`profiles/command_usage.json`), and
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

/// Maximum retained run-history entries (upstream `GuiSettings.HISTORY_MAX`).
pub const HISTORY_MAX: usize = 10;

/// Timestamp format for run history (upstream `HISTORY_TIME_FORMAT`).
pub const HISTORY_TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

/// One command identity: type ("Python" / "Mcu"), the NAME, and the file path
/// relative to the command root.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct UsageKey {
    pub command_type: String,
    pub name: String,
    pub file: String,
}

/// The on-disk document: `{"version": 1, "commands": [...], "history": [...]}`.
#[derive(Debug, Default, Serialize)]
struct UsageDocument {
    version: u32,
    commands: Vec<UsageRow>,
    history: Vec<HistoryRow>,
}

#[derive(Debug, Serialize)]
struct HistoryRow {
    #[serde(rename = "type")]
    command_type: String,
    name: String,
    last_run: String,
}

#[derive(Debug, Serialize)]
struct UsageRow {
    #[serde(rename = "type")]
    command_type: String,
    name: String,
    file: String,
    count: u64,
}

/// One run-history entry (upstream `command_history` row).
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct HistoryEntry {
    pub command_type: String,
    pub name: String,
    /// `%Y-%m-%d %H:%M:%S` local time of the last run.
    pub last_run: String,
}

/// Records command usage counts and the run history in one JSON file.
#[derive(Debug)]
pub struct CommandUsageStore {
    path: PathBuf,
    state: Mutex<UsageState>,
}

#[derive(Debug, Default)]
struct UsageState {
    counts: BTreeMap<UsageKey, u64>,
    /// Newest first (upstream `command_history` order).
    history: Vec<HistoryEntry>,
}

impl CommandUsageStore {
    /// Loads the store from `path`, returning an empty store when the file is
    /// missing or unreadable (the upstream returns an empty table as well).
    #[must_use]
    pub fn load(path: PathBuf) -> Self {
        let state = read_state(&path).unwrap_or_default();
        Self {
            path,
            state: Mutex::new(state),
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
        self.record_at(key, &history_timestamp())
    }

    /// Increments the count for `key` and records the run time, rewriting the
    /// store once. `last_run` uses [`HISTORY_TIME_FORMAT`].
    #[must_use]
    pub fn record_at(&self, key: &UsageKey, last_run: &str) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        *state.counts.entry(key.clone()).or_insert(0) += 1;
        add_history(
            &mut state.history,
            &key.command_type,
            &key.name,
            last_run,
            HISTORY_MAX,
        );
        write_state(&self.path, &state).is_ok()
    }

    /// The run history, newest first.
    #[must_use]
    pub fn history(&self) -> Vec<HistoryEntry> {
        self.state
            .lock()
            .map_or_else(|_| Vec::new(), |state| state.history.clone())
    }

    /// Removes one history entry by (type, name).
    #[must_use]
    pub fn remove_history(&self, command_type: &str, name: &str) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let before = state.history.len();
        state
            .history
            .retain(|entry| !(entry.command_type == command_type && entry.name == name));
        if state.history.len() == before {
            return true;
        }
        write_state(&self.path, &state).is_ok()
    }

    /// Clears the whole run history.
    #[must_use]
    pub fn clear_history(&self) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state.history.is_empty() {
            return true;
        }
        state.history.clear();
        write_state(&self.path, &state).is_ok()
    }

    /// The recorded count for one key.
    #[must_use]
    #[allow(
        dead_code,
        reason = "consumed by the usage-count API surface in the next step"
    )]
    pub fn count_of(&self, key: &UsageKey) -> u64 {
        self.state
            .lock()
            .map_or(0, |state| state.counts.get(key).copied().unwrap_or(0))
    }

    /// A snapshot of all counts.
    #[must_use]
    #[allow(
        dead_code,
        reason = "consumed by the usage-count API surface in the next step"
    )]
    pub fn snapshot(&self) -> BTreeMap<UsageKey, u64> {
        self.state
            .lock()
            .map_or_else(|_| BTreeMap::new(), |state| state.counts.clone())
    }
}

/// Reads counts and run history from the store file.
///
/// Mirrors the upstream tolerance: the document must be an object with a
/// `commands` array, but individual malformed rows are skipped while the
/// valid ones are kept. A missing `history` array is an older document.
fn read_state(path: &Path) -> Result<UsageState, std::io::Error> {
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
    let mut history = Vec::new();
    if let Some(rows) = document
        .get("history")
        .and_then(serde_json::Value::as_array)
    {
        for row in rows {
            let Some(command_type) = row.get("type").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let Some(name) = row.get("name").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let Some(last_run) = row.get("last_run").and_then(serde_json::Value::as_str) else {
                continue;
            };
            if command_type.is_empty() || name.is_empty() || last_run.is_empty() {
                continue;
            }
            history.push(HistoryEntry {
                command_type: command_type.to_owned(),
                name: name.to_owned(),
                last_run: last_run.to_owned(),
            });
            if history.len() == HISTORY_MAX {
                break;
            }
        }
    }
    Ok(UsageState { counts, history })
}

/// Adds one run to the newest-first history: the same command moves to the
/// front and the oldest entries beyond [`HISTORY_MAX`] are dropped (upstream
/// `command_catalog.add_history`).
fn add_history(
    history: &mut Vec<HistoryEntry>,
    command_type: &str,
    name: &str,
    last_run: &str,
    max_entries: usize,
) {
    history.retain(|entry| !(entry.command_type == command_type && entry.name == name));
    history.insert(
        0,
        HistoryEntry {
            command_type: command_type.to_owned(),
            name: name.to_owned(),
            last_run: last_run.to_owned(),
        },
    );
    history.truncate(max_entries);
}

/// The current local time in [`HISTORY_TIME_FORMAT`].
#[must_use]
pub fn history_timestamp() -> String {
    chrono::Local::now().format(HISTORY_TIME_FORMAT).to_string()
}

/// Reads a count value, accepting integers and numeric strings like the
/// upstream `int()` conversion.
fn count_value(value: &serde_json::Value) -> Option<u64> {
    if let Some(number) = value.as_u64() {
        return Some(number);
    }
    value.as_str().and_then(|text| text.trim().parse().ok())
}

/// Writes counts and history atomically: a temporary file first, then a
/// rename.
fn write_state(path: &Path, state: &UsageState) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent)?;
    }
    let document = UsageDocument {
        version: FORMAT_VERSION,
        commands: state
            .counts
            .iter()
            .map(|(key, count)| UsageRow {
                command_type: key.command_type.clone(),
                name: key.name.clone(),
                file: key.file.clone(),
                count: *count,
            })
            .collect(),
        history: state
            .history
            .iter()
            .map(|entry| HistoryRow {
                command_type: entry.command_type.clone(),
                name: entry.name.clone(),
                last_run: entry.last_run.clone(),
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
    #[test]
    fn history_keeps_newest_first_and_moves_repeats_to_the_front() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("profiles/command_usage.json");
        let store = CommandUsageStore::load(path.clone());
        assert!(store.record_at(&key("a"), "2026-10-19 10:00:00"));
        assert!(store.record_at(&key("b"), "2026-10-19 10:05:00"));
        assert!(store.record_at(&key("a"), "2026-10-19 10:10:00"));

        let history = store.history();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].name, "a");
        assert_eq!(history[0].last_run, "2026-10-19 10:10:00");
        assert_eq!(history[1].name, "b");
        assert_eq!(store.count_of(&key("a")), 2);

        let reloaded = CommandUsageStore::load(path);
        assert_eq!(reloaded.history(), history);
    }

    #[test]
    fn history_is_limited_to_the_newest_entries() {
        let directory = tempfile::tempdir().expect("tempdir");
        let store = CommandUsageStore::load(directory.path().join("command_usage.json"));
        for index in 0..(HISTORY_MAX + 5) {
            assert!(store.record_at(
                &key(&format!("command{index}")),
                &format!("2026-10-19 10:00:{index:02}")
            ));
        }
        let history = store.history();
        assert_eq!(history.len(), HISTORY_MAX);
        assert_eq!(history[0].name, format!("command{}", HISTORY_MAX + 4));
    }

    #[test]
    fn history_is_removable_and_clearable() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("command_usage.json");
        let store = CommandUsageStore::load(path.clone());
        assert!(store.record_at(&key("old"), "2026-10-19 09:00:00"));
        assert!(store.record_at(&key("new"), "2026-10-19 11:00:00"));

        let history = store.history();
        assert_eq!(history[0].name, "new");
        assert_eq!(history[1].name, "old");

        assert!(store.remove_history("Python", "old"));
        let reloaded = CommandUsageStore::load(path.clone());
        assert_eq!(reloaded.history().len(), 1);
        assert!(reloaded.clear_history());
        assert!(CommandUsageStore::load(path).history().is_empty());
    }

    #[test]
    fn documents_without_history_still_load_counts() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("command_usage.json");
        std::fs::write(
            &path,
            r#"{"version":1,"commands":[{"type":"Python","name":"first","file":"samples/first.py","count":2}]}"#,
        )
        .expect("write");
        let store = CommandUsageStore::load(path);
        assert_eq!(store.count_of(&key("first")), 2);
        assert!(store.history().is_empty());
    }
}
