//! Manual command-tag editing (PLAN `タグ編集`).
//!
//! Command tags come from two sources: directory names (automatic, prefixed
//! with `@`, upstream `CommandLoader.py`) and the class-level `TAGS`
//! attribute of each Python/Mcu command script. This module edits the latter.
//!
//! Source rewriting is structural: the file is parsed with a Python AST and
//! only the value expression of the class `TAGS` assignment is replaced (or a
//! new assignment is inserted after the class docstring). Every write is
//! guarded by the SHA-256 digest of the bytes that were read, so a file that
//! changed after the editor snapshot is reported as a conflict instead of
//! being silently overwritten.

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use atomic_write_file::OpenOptions;
use rustpython_parser::ast::{Constant, Expr, Mod, Ranged, Stmt};
use rustpython_parser::{Mode, parse};
use sha2::{Digest as _, Sha256};

/// Longest accepted manual tag, in characters (`TAGS` entries).
pub const TAG_MAX_LENGTH: usize = 50;
/// Prefix reserved for directory-derived automatic tags.
pub const AUTOMATIC_TAG_PREFIX: char = '@';

/// Failure while reading, parsing, or rewriting one command file.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum TagEditError {
    #[error("タグを入力してください")]
    EmptyTag,
    #[error("タグは{TAG_MAX_LENGTH}文字以内で入力してください")]
    TagTooLong,
    #[error("タグの先頭に「@」は使えません（ディレクトリ由来の自動タグ用に予約されています）")]
    ReservedTag,
    #[error("タグに制御文字は使えません")]
    ControlTag,
    #[error("同じタグが重複しています: {0}")]
    DuplicateTag(String),
    #[error("コマンドの解析に失敗しました: {0}")]
    Parse(String),
    #[error("コマンドクラスが見つかりません: {0}")]
    MissingClass(String),
    #[error("TAGS は文字列のリストで指定してください")]
    InvalidTags,
    #[error("コマンドファイルがコマンドルートの外を指しています")]
    PathEscape,
    #[error("コマンドファイルを読み書きできません: {0}")]
    Io(String),
}

/// One command file known to the running command service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TagCommandSource {
    pub name: String,
    pub module_path: String,
    pub class_name: String,
    pub relative_path: PathBuf,
    /// Complete tag list currently shown by the UI (automatic + manual).
    pub tags: Vec<String>,
}

/// One command with its editable manual tags and file digest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TagCommandView {
    pub name: String,
    pub module_path: String,
    pub class_name: String,
    pub relative_path: String,
    /// SHA-256 of the bytes the tag list was read from; empty when the file
    /// could not be read or parsed.
    pub digest: String,
    pub manual_tags: Vec<String>,
    pub automatic_tags: Vec<String>,
    /// Read/parse failure; the rest of the snapshot stays usable.
    pub error: Option<String>,
}

/// Result of reading every command file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TagSnapshot {
    pub commands: Vec<TagCommandView>,
}

/// One tag in the merged view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TagSummary {
    pub name: String,
    pub manual: bool,
    pub automatic: bool,
    pub count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TagSetEdit {
    pub module_path: String,
    pub class_name: String,
    pub digest: String,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TagApplyRequest {
    /// Replace the manual tags of specific commands (digest-guarded).
    Set(Vec<TagSetEdit>),
    /// Rename one manual tag across every command that declares it.
    Rename { from: String, to: String },
    /// Remove one manual tag from every command that declares it.
    Delete { tag: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TagApplyStatus {
    Updated,
    Unchanged,
    Conflict,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TagApplyOutcome {
    pub module_path: String,
    pub class_name: String,
    pub relative_path: String,
    pub status: TagApplyStatus,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TagApplyReport {
    pub outcomes: Vec<TagApplyOutcome>,
    pub changed: usize,
    pub conflicts: usize,
    pub snapshot: TagSnapshot,
}

/// Normalizes and validates one manual tag list.
///
/// Tags are trimmed, must be non-empty, at most [`TAG_MAX_LENGTH`] characters,
/// free of control characters, must not start with [`AUTOMATIC_TAG_PREFIX`],
/// and must not repeat.
pub fn normalize_tags(tags: &[String]) -> Result<Vec<String>, TagEditError> {
    let mut normalized = Vec::with_capacity(tags.len());
    let mut seen = BTreeSet::new();
    for raw in tags {
        let tag = raw.trim();
        if tag.is_empty() {
            return Err(TagEditError::EmptyTag);
        }
        if tag.chars().count() > TAG_MAX_LENGTH {
            return Err(TagEditError::TagTooLong);
        }
        if tag.starts_with(AUTOMATIC_TAG_PREFIX) {
            return Err(TagEditError::ReservedTag);
        }
        if tag.chars().any(char::is_control) {
            return Err(TagEditError::ControlTag);
        }
        if !seen.insert(tag.to_owned()) {
            return Err(TagEditError::DuplicateTag(tag.to_owned()));
        }
        normalized.push(tag.to_owned());
    }
    Ok(normalized)
}

fn normalize_one(tag: &str) -> Result<String, TagEditError> {
    normalize_tags(&[tag.to_owned()]).map(|mut tags| tags.remove(0))
}

/// Reads the manual `TAGS` declared by `class_name` in `source`.
pub fn extract_manual_tags(source: &str, class_name: &str) -> Result<Vec<String>, TagEditError> {
    let class = find_class(source, class_name)?;
    match last_tags_assignment(&class.body) {
        Some(
            TagsAssignment::Assign { value, .. }
            | TagsAssignment::Annotated {
                value: Some(value), ..
            },
        ) => tag_literal(value),
        Some(TagsAssignment::Annotated { value: None, .. }) | None => Ok(Vec::new()),
    }
}

/// Rewrites the manual `TAGS` of `class_name`, preserving the rest of the
/// file byte-for-byte.
pub fn rewrite_manual_tags(
    source: &str,
    class_name: &str,
    tags: &[String],
) -> Result<String, TagEditError> {
    let tags = normalize_tags(tags)?;
    let literal = render_literal(&tags);
    let class = find_class(source, class_name)?;
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    match last_tags_assignment(&class.body) {
        Some(
            TagsAssignment::Assign { value, .. }
            | TagsAssignment::Annotated {
                value: Some(value), ..
            },
        ) => {
            let start = value.start().to_usize();
            let end = value.end().to_usize();
            Ok(splice(source, start, end, &literal))
        }
        Some(TagsAssignment::Annotated { value: None, stmt }) => {
            let end = stmt.end().to_usize();
            Ok(splice(source, end, end, &format!(" = {literal}")))
        }
        None => {
            let anchor = class
                .body
                .iter()
                .enumerate()
                .find(|(index, statement)| !(*index == 0 && is_docstring(statement)))
                .map(|(_, statement)| statement.start().to_usize());
            if let Some(position) = anchor {
                let line_start = line_start(source, position);
                let indent = &source[line_start..position];
                let indent = if indent.chars().all(|c| c == ' ' || c == '\t') {
                    indent
                } else {
                    "    "
                };
                Ok(splice(
                    source,
                    line_start,
                    line_start,
                    &format!("{indent}TAGS = {literal}{newline}"),
                ))
            } else {
                let position = class
                    .body
                    .last()
                    .map_or_else(|| class.end().to_usize(), |s| s.end().to_usize());
                let next_line = source[position..]
                    .find('\n')
                    .map_or(source.len(), |offset| position + offset + 1);
                let at_end = next_line == source.len()
                    && !source[next_line.saturating_sub(1)..].starts_with('\n');
                let prefix = if at_end { newline } else { "" };
                Ok(splice(
                    source,
                    next_line,
                    next_line,
                    &format!("{prefix}    TAGS = {literal}{newline}"),
                ))
            }
        }
    }
}

/// Per-file tag editor rooted at the user command directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandTagEditor {
    root: PathBuf,
}

impl CommandTagEditor {
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Reads every command file and reports manual tags plus digests.
    pub fn snapshot(&self, commands: &[TagCommandSource]) -> TagSnapshot {
        TagSnapshot {
            commands: commands
                .iter()
                .map(|command| match self.read_tags(command) {
                    Ok((digest, manual_tags)) => TagCommandView {
                        name: command.name.clone(),
                        module_path: command.module_path.clone(),
                        class_name: command.class_name.clone(),
                        relative_path: relative_string(&command.relative_path),
                        digest,
                        automatic_tags: without_manual(&command.tags, &manual_tags),
                        manual_tags,
                        error: None,
                    },
                    Err(error) => TagCommandView {
                        name: command.name.clone(),
                        module_path: command.module_path.clone(),
                        class_name: command.class_name.clone(),
                        relative_path: relative_string(&command.relative_path),
                        digest: String::new(),
                        manual_tags: Vec::new(),
                        automatic_tags: command.tags.clone(),
                        error: Some(error.to_string()),
                    },
                })
                .collect(),
        }
    }

    /// Applies one tag edit request and returns a fresh snapshot.
    pub fn apply(
        &self,
        commands: &[TagCommandSource],
        request: &TagApplyRequest,
    ) -> Result<TagApplyReport, TagEditError> {
        let (outcomes, changed, conflicts) = match request {
            TagApplyRequest::Set(edits) => self.apply_set(commands, edits),
            TagApplyRequest::Rename { from, to } => self.apply_rename(commands, from, to)?,
            TagApplyRequest::Delete { tag } => self.apply_delete(commands, tag)?,
        };
        Ok(TagApplyReport {
            outcomes,
            changed,
            conflicts,
            snapshot: self.snapshot(commands),
        })
    }

    fn apply_set(
        &self,
        commands: &[TagCommandSource],
        edits: &[TagSetEdit],
    ) -> (Vec<TagApplyOutcome>, usize, usize) {
        let mut outcomes = Vec::new();
        let mut changed = 0usize;
        let mut conflicts = 0usize;
        for edit in edits {
            let source = commands.iter().find(|command| {
                command.module_path == edit.module_path && command.class_name == edit.class_name
            });
            let Some(source) = source else {
                outcomes.push(TagApplyOutcome {
                    module_path: edit.module_path.clone(),
                    class_name: edit.class_name.clone(),
                    relative_path: String::new(),
                    status: TagApplyStatus::Error,
                    message: Some("コマンドが見つかりません".to_owned()),
                });
                continue;
            };
            match self.set_one(source, edit) {
                Ok(outcome) => {
                    match outcome.status {
                        TagApplyStatus::Updated => changed += 1,
                        TagApplyStatus::Conflict => conflicts += 1,
                        _ => {}
                    }
                    outcomes.push(outcome);
                }
                Err(error) => outcomes.push(Self::error_outcome(source, &error)),
            }
        }
        (outcomes, changed, conflicts)
    }

    fn apply_rename(
        &self,
        commands: &[TagCommandSource],
        from: &str,
        to: &str,
    ) -> Result<(Vec<TagApplyOutcome>, usize, usize), TagEditError> {
        let from = normalize_one(from)?;
        let to = normalize_one(to)?;
        let mut outcomes = Vec::new();
        let mut changed = 0usize;
        if from == to {
            return Ok((outcomes, changed, 0));
        }
        for source in commands {
            let Ok((_digest, manual_tags)) = self.read_tags(source) else {
                continue;
            };
            if !manual_tags.contains(&from) {
                continue;
            }
            let mut tags = Vec::with_capacity(manual_tags.len());
            for tag in manual_tags {
                if tag == from {
                    if !tags.contains(&to) {
                        tags.push(to.clone());
                    }
                } else if !tags.contains(&tag) {
                    tags.push(tag);
                }
            }
            match self.write(source, &tags) {
                Ok(outcome) => {
                    if outcome.status == TagApplyStatus::Updated {
                        changed += 1;
                    }
                    outcomes.push(outcome);
                }
                Err(error) => outcomes.push(Self::error_outcome(source, &error)),
            }
        }
        Ok((outcomes, changed, 0))
    }

    fn apply_delete(
        &self,
        commands: &[TagCommandSource],
        tag: &str,
    ) -> Result<(Vec<TagApplyOutcome>, usize, usize), TagEditError> {
        let tag = normalize_one(tag)?;
        let mut outcomes = Vec::new();
        let mut changed = 0usize;
        for source in commands {
            let Ok((_digest, manual_tags)) = self.read_tags(source) else {
                continue;
            };
            if !manual_tags.contains(&tag) {
                continue;
            }
            let tags = manual_tags
                .into_iter()
                .filter(|existing| existing != &tag)
                .collect::<Vec<_>>();
            match self.write(source, &tags) {
                Ok(outcome) => {
                    if outcome.status == TagApplyStatus::Updated {
                        changed += 1;
                    }
                    outcomes.push(outcome);
                }
                Err(error) => outcomes.push(Self::error_outcome(source, &error)),
            }
        }
        Ok((outcomes, changed, 0))
    }

    /// Merged manual/automatic tag list for the editor.
    #[must_use]
    pub fn summarize(snapshot: &TagSnapshot) -> Vec<TagSummary> {
        let mut summary: std::collections::BTreeMap<String, TagSummary> =
            std::collections::BTreeMap::new();
        for command in &snapshot.commands {
            for tag in &command.manual_tags {
                let entry = summary.entry(tag.clone()).or_insert_with(|| TagSummary {
                    name: tag.clone(),
                    manual: false,
                    automatic: false,
                    count: 0,
                });
                if !entry.manual {
                    entry.manual = true;
                    entry.count += 1;
                }
            }
            for tag in &command.automatic_tags {
                let entry = summary.entry(tag.clone()).or_insert_with(|| TagSummary {
                    name: tag.clone(),
                    manual: false,
                    automatic: false,
                    count: 0,
                });
                if !entry.automatic {
                    entry.automatic = true;
                    entry.count += 1;
                }
            }
        }
        summary.into_values().collect()
    }

    fn set_one(
        &self,
        source: &TagCommandSource,
        edit: &TagSetEdit,
    ) -> Result<TagApplyOutcome, TagEditError> {
        let tags = normalize_tags(&edit.tags)?;
        let (digest, current) = self.read_tags(source)?;
        if digest != edit.digest {
            return Ok(Self::outcome(
                source,
                TagApplyStatus::Conflict,
                Some("ファイルが編集後に変更されています。再読み込みしてください".to_owned()),
            ));
        }
        if current == tags {
            return Ok(Self::outcome(source, TagApplyStatus::Unchanged, None));
        }
        self.write(source, &tags)
    }

    fn write(
        &self,
        source: &TagCommandSource,
        tags: &[String],
    ) -> Result<TagApplyOutcome, TagEditError> {
        let path = self.resolve(&source.relative_path)?;
        let bytes = fs::read(&path).map_err(|error| TagEditError::Io(error.to_string()))?;
        let source_text = String::from_utf8(bytes).map_err(|_error| {
            TagEditError::Io("コマンドファイルがUTF-8ではありません".to_owned())
        })?;
        let updated = rewrite_manual_tags(&source_text, &source.class_name, tags)?;
        if updated == source_text {
            return Ok(Self::outcome(source, TagApplyStatus::Unchanged, None));
        }
        atomic_write(&path, updated.as_bytes())
            .map_err(|error| TagEditError::Io(error.to_string()))?;
        Ok(Self::outcome(source, TagApplyStatus::Updated, None))
    }

    fn read_tags(&self, source: &TagCommandSource) -> Result<(String, Vec<String>), TagEditError> {
        let path = self.resolve(&source.relative_path)?;
        let bytes = fs::read(&path).map_err(|error| TagEditError::Io(error.to_string()))?;
        let digest = hex::encode(Sha256::digest(&bytes));
        let text = String::from_utf8(bytes).map_err(|_error| {
            TagEditError::Io("コマンドファイルがUTF-8ではありません".to_owned())
        })?;
        let tags = extract_manual_tags(&text, &source.class_name)?;
        Ok((digest, tags))
    }

    fn resolve(&self, relative: &Path) -> Result<PathBuf, TagEditError> {
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(TagEditError::PathEscape);
        }
        let root = self
            .root
            .canonicalize()
            .map_err(|error| TagEditError::Io(error.to_string()))?;
        let path = root
            .join(relative)
            .canonicalize()
            .map_err(|error| TagEditError::Io(error.to_string()))?;
        if !path.starts_with(&root) {
            return Err(TagEditError::PathEscape);
        }
        Ok(path)
    }

    fn outcome(
        source: &TagCommandSource,
        status: TagApplyStatus,
        message: Option<String>,
    ) -> TagApplyOutcome {
        TagApplyOutcome {
            module_path: source.module_path.clone(),
            class_name: source.class_name.clone(),
            relative_path: relative_string(&source.relative_path),
            status,
            message,
        }
    }

    fn error_outcome(source: &TagCommandSource, error: &TagEditError) -> TagApplyOutcome {
        TagApplyOutcome {
            module_path: source.module_path.clone(),
            class_name: source.class_name.clone(),
            relative_path: relative_string(&source.relative_path),
            status: TagApplyStatus::Error,
            message: Some(error.to_string()),
        }
    }
}

enum TagsAssignment<'a> {
    Assign {
        value: &'a Expr,
    },
    Annotated {
        stmt: &'a Stmt,
        value: Option<&'a Expr>,
    },
}

fn find_class(
    source: &str,
    class_name: &str,
) -> Result<rustpython_parser::ast::StmtClassDef, TagEditError> {
    let module = parse(source, Mode::Module, "<command>")
        .map_err(|error| TagEditError::Parse(error.to_string()))?;
    let Mod::Module(module) = module else {
        return Err(TagEditError::Parse("module mode expected".to_owned()));
    };
    module
        .body
        .into_iter()
        .find_map(|statement| match statement {
            Stmt::ClassDef(class) if class.name.as_str() == class_name => Some(class),
            _ => None,
        })
        .ok_or_else(|| TagEditError::MissingClass(class_name.to_owned()))
}

fn last_tags_assignment(body: &[Stmt]) -> Option<TagsAssignment<'_>> {
    let mut found = None;
    for statement in body {
        match statement {
            Stmt::Assign(assign) if assign.targets.iter().any(is_tags_name) => {
                found = Some(TagsAssignment::Assign {
                    value: assign.value.as_ref(),
                });
            }
            Stmt::AnnAssign(assign) if is_tags_name(&assign.target) => {
                found = Some(TagsAssignment::Annotated {
                    stmt: statement,
                    value: assign.value.as_deref(),
                });
            }
            _ => {}
        }
    }
    found
}

fn is_tags_name(expr: &Expr) -> bool {
    matches!(expr, Expr::Name(name) if name.id.as_str() == "TAGS")
}

fn is_docstring(statement: &Stmt) -> bool {
    matches!(
        statement,
        Stmt::Expr(expr)
            if matches!(
                expr.value.as_ref(),
                Expr::Constant(constant) if matches!(constant.value, Constant::Str(_))
            )
    )
}

fn tag_literal(expr: &Expr) -> Result<Vec<String>, TagEditError> {
    let elements: Vec<&Expr> = match expr {
        Expr::List(list) => list.elts.iter().collect(),
        Expr::Tuple(tuple) => tuple.elts.iter().collect(),
        Expr::Constant(constant) => {
            return match &constant.value {
                Constant::Str(value) => Ok(vec![value.clone()]),
                _ => Err(TagEditError::InvalidTags),
            };
        }
        _ => return Err(TagEditError::InvalidTags),
    };
    elements
        .into_iter()
        .map(|element| match element {
            Expr::Constant(constant) => match &constant.value {
                Constant::Str(value) => Ok(value.clone()),
                _ => Err(TagEditError::InvalidTags),
            },
            _ => Err(TagEditError::InvalidTags),
        })
        .collect()
}

fn render_literal(tags: &[String]) -> String {
    let entries = tags
        .iter()
        .map(|tag| format!("{tag:?}"))
        .collect::<Vec<_>>();
    format!("[{}]", entries.join(", "))
}

fn without_manual(tags: &[String], manual: &[String]) -> Vec<String> {
    tags.iter()
        .filter(|tag| !manual.contains(tag))
        .cloned()
        .collect()
}

fn splice(source: &str, start: usize, end: usize, replacement: &str) -> String {
    let mut updated = String::with_capacity(source.len() + replacement.len());
    updated.push_str(&source[..start]);
    updated.push_str(replacement);
    updated.push_str(&source[end..]);
    updated
}

fn line_start(source: &str, offset: usize) -> usize {
    source[..offset].rfind('\n').map_or(0, |index| index + 1)
}

fn relative_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn atomic_write(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
    })?;
    fs::create_dir_all(parent)?;
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
    let mut file = options.open(path)?;
    file.write_all(content)?;
    file.commit()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "from Commands.PythonCommandBase import PythonCommand\n\n\nclass Sample(PythonCommand):\n    NAME = \"サンプル\"\n    TAGS = [\"Rank\", \"Device\"]\n\n    def do(self):\n        pass\n";

    #[test]
    fn reads_manual_tags_from_the_class_attribute() {
        assert_eq!(
            extract_manual_tags(SAMPLE, "Sample").unwrap(),
            vec!["Rank".to_owned(), "Device".to_owned()]
        );
    }

    #[test]
    fn accepts_a_single_string_like_upstream() {
        let source = "class Sample:\n    TAGS = \"Rank\"\n";
        assert_eq!(extract_manual_tags(source, "Sample").unwrap(), vec!["Rank"]);
    }

    #[test]
    fn rewrites_only_the_value_expression() {
        let updated = rewrite_manual_tags(SAMPLE, "Sample", &["Alpha".to_owned()]).unwrap();
        assert!(updated.contains("    TAGS = [\"Alpha\"]\n"));
        assert!(updated.contains("NAME = \"サンプル\""));
        assert_eq!(
            extract_manual_tags(&updated, "Sample").unwrap(),
            vec!["Alpha".to_owned()]
        );
    }

    #[test]
    fn preserves_trailing_comments_and_crlf() {
        let source = "class Sample:\r\n    TAGS = []  # keep me\r\n";
        let updated = rewrite_manual_tags(source, "Sample", &["A".to_owned()]).unwrap();
        assert_eq!(
            updated,
            "class Sample:\r\n    TAGS = [\"A\"]  # keep me\r\n"
        );
    }

    #[test]
    fn inserts_after_the_docstring_when_missing() {
        let source = "class Sample:\n    \"\"\"doc\"\"\"\n\n    def do(self):\n        pass\n";
        let updated = rewrite_manual_tags(source, "Sample", &["A".to_owned()]).unwrap();
        let docstring = updated.find("\"\"\"doc\"\"\"").unwrap();
        let tags = updated.find("    TAGS = [\"A\"]\n").unwrap();
        let method = updated.find("    def do(self):").unwrap();
        assert!(docstring < tags && tags < method, "{updated}");
        assert_eq!(extract_manual_tags(&updated, "Sample").unwrap(), vec!["A"]);
    }

    #[test]
    fn inserts_at_the_first_body_statement() {
        let source = "class Sample:\n    def do(self):\n        pass\n";
        let updated = rewrite_manual_tags(source, "Sample", &["A".to_owned()]).unwrap();
        assert!(
            updated.contains("class Sample:\n    TAGS = [\"A\"]\n    def do"),
            "{updated}"
        );
    }

    #[test]
    fn reports_a_missing_class() {
        assert_eq!(
            extract_manual_tags(SAMPLE, "Other"),
            Err(TagEditError::MissingClass("Other".to_owned()))
        );
    }

    #[test]
    fn rejects_invalid_tag_shapes() {
        let source = "class Sample:\n    TAGS = (1, 2)\n";
        assert_eq!(
            extract_manual_tags(source, "Sample"),
            Err(TagEditError::InvalidTags)
        );
    }

    #[test]
    fn normalizes_tags() {
        assert_eq!(
            normalize_tags(&[" Rank ".to_owned(), "Device".to_owned()]).unwrap(),
            vec!["Rank".to_owned(), "Device".to_owned()]
        );
        assert_eq!(
            normalize_tags(&[String::new()]),
            Err(TagEditError::EmptyTag)
        );
        assert_eq!(
            normalize_tags(&["@Dir".to_owned()]),
            Err(TagEditError::ReservedTag)
        );
        assert_eq!(
            normalize_tags(&["A".to_owned(), "A".to_owned()]),
            Err(TagEditError::DuplicateTag("A".to_owned()))
        );
    }

    fn editor_with(files: &[(&str, &str)]) -> (tempfile::TempDir, CommandTagEditor) {
        let dir = tempfile::tempdir().unwrap();
        for (name, body) in files {
            let path = dir.path().join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, body).unwrap();
        }
        let editor = CommandTagEditor::new(dir.path().to_path_buf());
        (dir, editor)
    }

    fn source(relative: &str, tags: &[&str]) -> TagCommandSource {
        TagCommandSource {
            name: "Sample".to_owned(),
            module_path: "Commands.PythonCommands.sample".to_owned(),
            class_name: "Sample".to_owned(),
            relative_path: PathBuf::from(relative),
            tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
        }
    }

    #[test]
    fn snapshot_reports_digest_and_separates_automatic_tags() {
        let (_dir, editor) = editor_with(&[("sample.py", SAMPLE)]);
        let snapshot = editor.snapshot(&[source("sample.py", &["@Samples", "Rank", "Device"])]);
        let command = &snapshot.commands[0];
        assert_eq!(command.manual_tags, vec!["Rank", "Device"]);
        assert_eq!(command.automatic_tags, vec!["@Samples"]);
        assert_eq!(command.digest.len(), 64);
        assert!(command.error.is_none());
    }

    #[test]
    fn set_refuses_a_stale_digest() {
        let (_dir, editor) = editor_with(&[("sample.py", SAMPLE)]);
        let commands = vec![source("sample.py", &["Rank"])];
        let report = editor
            .apply(
                &commands,
                &TagApplyRequest::Set(vec![TagSetEdit {
                    module_path: "Commands.PythonCommands.sample".to_owned(),
                    class_name: "Sample".to_owned(),
                    digest: "stale".to_owned(),
                    tags: vec!["New".to_owned()],
                }]),
            )
            .unwrap();
        assert_eq!(report.conflicts, 1);
        assert_eq!(report.outcomes[0].status, TagApplyStatus::Conflict);
        assert_eq!(
            extract_manual_tags(SAMPLE, "Sample").unwrap(),
            vec!["Rank", "Device"]
        );
    }

    #[test]
    fn set_updates_the_file_and_snapshot() {
        let (_dir, editor) = editor_with(&[("sample.py", SAMPLE)]);
        let commands = vec![source("sample.py", &["Rank", "Device"])];
        let snapshot = editor.snapshot(&commands);
        let report = editor
            .apply(
                &commands,
                &TagApplyRequest::Set(vec![TagSetEdit {
                    module_path: "Commands.PythonCommands.sample".to_owned(),
                    class_name: "Sample".to_owned(),
                    digest: snapshot.commands[0].digest.clone(),
                    tags: vec!["New".to_owned()],
                }]),
            )
            .unwrap();
        assert_eq!(report.changed, 1);
        assert_eq!(report.snapshot.commands[0].manual_tags, vec!["New"]);
    }

    #[test]
    fn rename_updates_every_command_that_declares_the_tag() {
        let other = "class Other:\n    TAGS = [\"Rank\", \"Keep\"]\n";
        let (dir, editor) = editor_with(&[("sample.py", SAMPLE), ("other.py", other)]);
        let commands = vec![
            source("sample.py", &["Rank", "Device"]),
            TagCommandSource {
                name: "Other".to_owned(),
                module_path: "Commands.PythonCommands.other".to_owned(),
                class_name: "Other".to_owned(),
                relative_path: PathBuf::from("other.py"),
                tags: vec!["Rank".to_owned(), "Keep".to_owned()],
            },
        ];
        let report = editor
            .apply(
                &commands,
                &TagApplyRequest::Rename {
                    from: "Rank".to_owned(),
                    to: "Level".to_owned(),
                },
            )
            .unwrap();
        assert_eq!(report.changed, 2);
        assert_eq!(
            extract_manual_tags(other, "Other").unwrap(),
            vec!["Rank", "Keep"]
        );
        let updated = fs::read_to_string(dir.path().join("sample.py")).unwrap();
        assert_eq!(
            extract_manual_tags(&updated, "Sample").unwrap(),
            vec!["Level", "Device"]
        );
    }

    #[test]
    fn delete_removes_the_tag_everywhere() {
        let (dir, editor) = editor_with(&[("sample.py", SAMPLE)]);
        let commands = vec![source("sample.py", &["Rank", "Device"])];
        let report = editor
            .apply(
                &commands,
                &TagApplyRequest::Delete {
                    tag: "Rank".to_owned(),
                },
            )
            .unwrap();
        assert_eq!(report.changed, 1);
        assert_eq!(
            extract_manual_tags(SAMPLE, "Sample").unwrap(),
            vec!["Rank", "Device"]
        );
        let updated = fs::read_to_string(dir.path().join("sample.py")).unwrap();
        assert_eq!(
            extract_manual_tags(&updated, "Sample").unwrap(),
            vec!["Device"]
        );
    }

    #[test]
    fn rejects_paths_outside_the_command_root() {
        let (_dir, editor) = editor_with(&[("sample.py", SAMPLE)]);
        let escaped = TagCommandSource {
            relative_path: PathBuf::from("../escape.py"),
            ..source("sample.py", &[])
        };
        assert_eq!(
            editor.read_tags(&escaped).unwrap_err(),
            TagEditError::PathEscape
        );
    }

    #[test]
    fn summarize_counts_manual_and_automatic_usage() {
        let (_dir, editor) = editor_with(&[("sample.py", SAMPLE)]);
        let snapshot = editor.snapshot(&[source("sample.py", &["@Samples", "Rank", "Device"])]);
        let summary = CommandTagEditor::summarize(&snapshot);
        let rank = summary.iter().find(|tag| tag.name == "Rank").unwrap();
        assert!(rank.manual);
        assert_eq!(rank.count, 1);
        let automatic = summary.iter().find(|tag| tag.name == "@Samples").unwrap();
        assert!(automatic.automatic);
        assert!(!automatic.manual);
    }
}
