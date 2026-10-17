//! Run lists: saved sequences of commands executed in order (upstream
//! `services/run_list.py`).
//!
//! A run list is stored per profile as `run_lists.json` next to the settings
//! file. Saved lists appear in the command list as `name (実行リスト)` with
//! the `@実行リスト` tag, and behave like ordinary commands when started.
//! This module owns the schema, validation, and atomic storage; the command
//! list integration and the executor consume it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// File name and format version (upstream `FILE_NAME` / `FORMAT_VERSION`).
pub const FILE_NAME: &str = "run_lists.json";
pub const FORMAT_VERSION: u32 = 1;
/// Command kinds and their user-facing names.
pub const PYTHON: &str = "Python";
pub const MCU: &str = "Mcu";
/// Display suffix and list tag (upstream `NAME_SUFFIX` / `LIST_TAG`).
pub const NAME_SUFFIX: &str = " (実行リスト)";
pub const LIST_TAG: &str = "@実行リスト";
pub const NAME_MAX_LENGTH: usize = 50;
pub const REPEAT_MAX: u32 = 9_999;
pub const LIMIT_MAX_MINUTES: f64 = 60.0 * 24.0 * 7.0;
pub const INTERVAL_MAX_SECONDS: f64 = 3_600.0;
/// Error handling modes (upstream `ON_ERROR_*`).
pub const ON_ERROR_STOP: &str = "stop";
pub const ON_ERROR_NEXT: &str = "next";
/// `loops = 0` repeats until Stop is pressed (upstream `LOOP_FOREVER`).
pub const LOOP_FOREVER: u32 = 0;

/// One row of a run list (upstream `RunItem`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunItem {
    #[serde(rename = "type")]
    pub command_type: String,
    pub name: String,
    #[serde(default = "default_repeat")]
    pub repeat: u32,
    #[serde(default)]
    pub limit_minutes: Option<f64>,
}

fn default_repeat() -> u32 {
    1
}

/// A run list (upstream `RunList`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunList {
    pub name: String,
    #[serde(default)]
    pub items: Vec<RunItem>,
    #[serde(default = "default_interval")]
    pub interval_seconds: f64,
    #[serde(default = "default_on_error")]
    pub on_error: String,
    #[serde(default = "default_loops")]
    pub loops: u32,
}

fn default_interval() -> f64 {
    1.0
}

fn default_on_error() -> String {
    ON_ERROR_STOP.to_owned()
}

fn default_loops() -> u32 {
    1
}

impl RunList {
    /// Display name in the command list (`name (実行リスト)`).
    #[must_use]
    pub fn display_name(&self) -> String {
        format!("{}{NAME_SUFFIX}", self.name)
    }
}

/// Whether a command-list name belongs to a run list (nested lists are
/// rejected; upstream `is_list_name`).
#[must_use]
pub fn is_list_name(command_type: &str, name: &str) -> bool {
    command_type == PYTHON && name.ends_with(NAME_SUFFIX)
}

/// Storage path for the current profile: `run_lists.json` next to the
/// settings file (the profile settings directory).
#[must_use]
pub fn lists_path(profile_settings_dir: &Path) -> PathBuf {
    profile_settings_dir.join(FILE_NAME)
}

/// Validates a list name; returns the user-facing reason or empty.
#[must_use]
pub fn check_name(name: &str, others: &[String]) -> String {
    let text = name.trim();
    if text.is_empty() {
        return "名前を入力してください。".to_owned();
    }
    if text.chars().count() > NAME_MAX_LENGTH {
        return format!("名前は {NAME_MAX_LENGTH} 文字までにしてください。");
    }
    if text.chars().any(|c| c < '\u{20}' || c == '\u{7f}') {
        return "名前に、改行やタブは使えません。".to_owned();
    }
    if others.iter().any(|other| other == text) {
        return format!("「{text}」という名前の実行リストが、すでにあります。");
    }
    String::new()
}

/// Validates one row; returns the reason or empty (upstream `check_item`).
#[must_use]
pub fn check_item(item: &RunItem) -> String {
    if item.command_type != PYTHON && item.command_type != MCU {
        return format!("コマンドの種類が正しくありません({})。", item.command_type);
    }
    if item.name.is_empty() {
        return "コマンドを選んでください。".to_owned();
    }
    if is_list_name(&item.command_type, &item.name) {
        return "実行リストの中に、実行リストは入れられません。".to_owned();
    }
    if !(1..=REPEAT_MAX).contains(&item.repeat) {
        return format!("回数は 1 〜 {REPEAT_MAX} の整数にしてください。");
    }
    match item.limit_minutes {
        None => {
            if item.command_type == MCU {
                return "Mcu コマンドは、終わったことが分からないので、制限時間を入れてください。"
                    .to_owned();
            }
        }
        Some(minutes) => {
            if !(minutes > 0.0 && minutes <= LIMIT_MAX_MINUTES) {
                return format!(
                    "制限時間は、0 より大きく {LIMIT_MAX_MINUTES} 分までにしてください。"
                );
            }
        }
    }
    String::new()
}

/// Validates a whole list; returns the first reason or empty.
#[must_use]
pub fn check_list(run_list: &RunList, others: &[String]) -> String {
    let error = check_name(&run_list.name, others);
    if !error.is_empty() {
        return error;
    }
    if run_list.items.is_empty() {
        return "コマンドを 1 つ以上、足してください。".to_owned();
    }
    for (index, item) in run_list.items.iter().enumerate() {
        let error = check_item(item);
        if !error.is_empty() {
            return format!("{} 行目: {error}", index + 1);
        }
    }
    if !(0.0..=INTERVAL_MAX_SECONDS).contains(&run_list.interval_seconds) {
        return format!("コマンドの間の待ち時間は、0 〜 {INTERVAL_MAX_SECONDS} 秒にしてください。");
    }
    if run_list.on_error != ON_ERROR_STOP && run_list.on_error != ON_ERROR_NEXT {
        return "エラーのときの動きが正しくありません。".to_owned();
    }
    if run_list.loops > REPEAT_MAX {
        return format!(
            "リスト全体の繰り返しは、0(Stop を押すまで)〜 {REPEAT_MAX} の整数にしてください。"
        );
    }
    String::new()
}

/// Display form of a limit (`None` renders empty; upstream `describe_limit`).
#[must_use]
pub fn describe_limit(minutes: Option<f64>) -> String {
    match minutes {
        None => String::new(),
        Some(minutes) => format!("{} 分", format_number(minutes)),
    }
}

/// Formats a number like Python's `%g` for the values used here.
#[allow(
    clippy::cast_possible_truncation,
    reason = "limit values are small integral minutes"
)]
fn format_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// The stored document shape (`{"version": 1, "lists": [...]}`).
#[derive(Serialize, Deserialize)]
struct StoredDocument {
    version: u32,
    lists: Vec<RunList>,
}

/// Reads the saved run lists; missing or unreadable files yield an empty
/// list, and invalid lists are skipped with the reason returned alongside.
#[must_use]
pub fn load_lists(path: &Path) -> (Vec<RunList>, Vec<String>) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return (Vec::new(), Vec::new());
    };
    let Ok(document) = serde_json::from_str::<StoredDocument>(&content) else {
        return (Vec::new(), vec!["Failed to read the run lists".to_owned()]);
    };
    let mut result: Vec<RunList> = Vec::new();
    let mut skipped = Vec::new();
    for row in document.lists {
        let mut others: Vec<String> = result.iter().map(|list| list.name.clone()).collect();
        others.shrink_to_fit();
        let error = check_list(&row, &others);
        if !error.is_empty() {
            skipped.push(error);
            continue;
        }
        result.push(row);
    }
    (result, skipped)
}

/// Writes the run lists atomically (temporary file + rename).
pub fn save_lists(run_lists: &[RunList], path: &Path) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let document = StoredDocument {
        version: FORMAT_VERSION,
        lists: run_lists.to_vec(),
    };
    let content = serde_json::to_string_pretty(&document)?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let result =
        std::fs::write(&temporary, content).and_then(|()| std::fs::rename(&temporary, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// Parses a text input as a finite number (upstream `parse_number`).
///
/// Empty input yields `Ok(None)` when `allow_empty`, `Err` otherwise.
pub fn parse_number(text: &str, allow_empty: bool) -> Result<Option<f64>, &'static str> {
    let text = text.trim();
    if text.is_empty() {
        return if allow_empty { Ok(None) } else { Err("empty") };
    }
    let value: f64 = text.parse().map_err(|_error| "not a number")?;
    if !value.is_finite() {
        return Err("not a finite number");
    }
    Ok(Some(value))
}

/// Names of the run lists for duplicate checking.
#[must_use]
pub fn list_names(run_lists: &[RunList]) -> Vec<String> {
    run_lists.iter().map(|list| list.name.clone()).collect()
}

/// Set of display names, used to filter nested lists out of pickers.
#[must_use]
pub fn display_names(run_lists: &[RunList]) -> BTreeSet<String> {
    run_lists
        .iter()
        .map(RunList::display_name)
        .collect::<BTreeSet<_>>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str) -> RunItem {
        RunItem {
            command_type: PYTHON.to_owned(),
            name: name.to_owned(),
            repeat: 1,
            limit_minutes: None,
        }
    }

    fn sample_list() -> RunList {
        RunList {
            name: "テスト".to_owned(),
            items: vec![item("first"), item("second")],
            interval_seconds: 1.0,
            on_error: ON_ERROR_STOP.to_owned(),
            loops: 1,
        }
    }

    #[test]
    fn checks_follow_the_upstream_rules() {
        assert_eq!(check_name("", &[]), "名前を入力してください。");
        assert!(check_name("a\nb", &[]).contains("改行やタブ"));
        assert!(check_name("x", &["x".to_owned()]).contains("すでにあります"));
        assert_eq!(check_name("ok", &[]), "");
        assert!(check_item(&item("")).contains("選んでください"));
        assert!(check_item(&item("a (実行リスト)")).contains("入れられません"));
        let mcu = RunItem {
            command_type: MCU.to_owned(),
            name: "mcu".to_owned(),
            repeat: 1,
            limit_minutes: None,
        };
        assert!(check_item(&mcu).contains("制限時間"));
        let bad_repeat = RunItem {
            repeat: 0,
            ..item("x")
        };
        assert!(check_item(&bad_repeat).contains("1 〜 9999"));
    }

    #[test]
    fn list_checks_cover_the_whole_shape() {
        let mut list = sample_list();
        assert_eq!(check_list(&list, &[]), "");
        list.items.clear();
        assert!(check_list(&list, &[]).contains("1 つ以上"));
        list.items.push(RunItem {
            repeat: 0,
            ..item("x")
        });
        assert!(check_list(&list, &[]).contains("1 行目"));
        list.items[0].repeat = 1;
        list.interval_seconds = INTERVAL_MAX_SECONDS + 1.0;
        assert!(check_list(&list, &[]).contains("待ち時間"));
        list.interval_seconds = 1.0;
        list.on_error = "bogus".to_owned();
        assert!(check_list(&list, &[]).contains("エラー"));
        list.on_error = ON_ERROR_NEXT.to_owned();
        list.loops = REPEAT_MAX + 1;
        assert!(check_list(&list, &[]).contains("繰り返し"));
    }

    #[test]
    fn storage_round_trips_atomically() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = lists_path(directory.path());
        assert_eq!(load_lists(&path).0, Vec::new());
        let lists = vec![sample_list()];
        save_lists(&lists, &path).expect("save");
        let (loaded, skipped) = load_lists(&path);
        assert!(skipped.is_empty());
        assert_eq!(loaded, lists);
        assert!(
            std::fs::read_to_string(&path)
                .expect("read")
                .contains("\"version\": 1")
        );
    }

    #[test]
    fn invalid_lists_are_skipped_with_reasons() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = lists_path(directory.path());
        let bad = RunList {
            name: "bad".to_owned(),
            items: Vec::new(),
            interval_seconds: 1.0,
            on_error: ON_ERROR_STOP.to_owned(),
            loops: 1,
        };
        save_lists(&[sample_list(), bad], &path).expect("save");
        let (loaded, skipped) = load_lists(&path);
        assert_eq!(loaded.len(), 1);
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].contains("1 つ以上"));
    }

    #[test]
    fn numbers_parse_like_the_upstream() {
        assert_eq!(parse_number(" 2.5 ", false), Ok(Some(2.5)));
        assert_eq!(parse_number("", true), Ok(None));
        assert!(parse_number("", false).is_err());
        assert!(parse_number("nan", false).is_err());
        assert_eq!(describe_limit(None), "");
        assert_eq!(describe_limit(Some(1.5)), "1.5 分");
        assert_eq!(describe_limit(Some(2.0)), "2 分");
    }

    #[test]
    fn display_names_carry_the_suffix() {
        let lists = vec![sample_list()];
        assert!(display_names(&lists).contains("テスト (実行リスト)"));
        assert!(is_list_name(PYTHON, "テスト (実行リスト)"));
        assert!(!is_list_name(MCU, "テスト (実行リスト)"));
    }
}
