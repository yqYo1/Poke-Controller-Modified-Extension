use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Component, Path, PathBuf};

use pokecon_contracts::model::{PathKind, PathMetadata};
use thiserror::Error;

use crate::roots::{EffectiveRoots, RootEnvironment};

/// Input surface controlling the base of a relative path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathSource {
    CommandLine,
    Environment,
    GlobalToml,
    ProfileToml,
    Dynamic,
    OpenApi,
}

/// OS-specific variable syntax used during the required one-pass expansion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExpansionStyle {
    Unix,
    Windows,
}

impl ExpansionStyle {
    #[must_use]
    pub const fn native() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Unix
        }
    }
}

/// Resolves and validates one registry-classified path value.
///
/// # Errors
///
/// Returns a secret-safe error for invalid expansion syntax, missing variables,
/// inaccessible paths, or metadata-policy violations.
pub fn resolve_path(
    raw: &str,
    source: PathSource,
    roots: &EffectiveRoots,
    startup_cwd: &Path,
    environment: &RootEnvironment,
    metadata: &PathMetadata,
) -> Result<PathBuf, PathError> {
    if raw.is_empty() {
        return Err(PathError::Empty);
    }
    let expanded = expand_environment(raw, environment, ExpansionStyle::native())?;
    let expanded = expand_tilde(&expanded, environment)?;
    let path = PathBuf::from(&expanded);
    if foreign_absolute_path(&expanded) && !path.is_absolute() {
        return Err(PathError::ForeignAbsolute);
    }
    let base = match source {
        PathSource::CommandLine => startup_cwd,
        PathSource::Environment
        | PathSource::GlobalToml
        | PathSource::ProfileToml
        | PathSource::Dynamic
        | PathSource::OpenApi => &roots.config,
    };
    let normalized = lexical_normalize(if path.is_absolute() {
        path
    } else {
        base.join(path)
    });
    apply_metadata(normalized, metadata)
}

/// Applies one OS-specific environment expansion pass.
///
/// # Errors
///
/// Returns an error for malformed syntax, undefined names, or non-Unicode
/// environment values.
pub fn expand_environment(
    raw: &str,
    environment: &RootEnvironment,
    style: ExpansionStyle,
) -> Result<String, PathError> {
    match style {
        ExpansionStyle::Unix => expand_unix(raw, environment),
        ExpansionStyle::Windows => expand_windows(raw, environment),
    }
}

fn expand_unix(raw: &str, environment: &RootEnvironment) -> Result<String, PathError> {
    let bytes = raw.as_bytes();
    let mut result = String::with_capacity(raw.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'$' {
            let character = raw[cursor..]
                .chars()
                .next()
                .expect("cursor must point to a character boundary");
            result.push(character);
            cursor += character.len_utf8();
            continue;
        }
        if bytes.get(cursor + 1) == Some(&b'{') {
            let name_start = cursor + 2;
            let Some(relative_end) = raw[name_start..].find('}') else {
                return Err(PathError::MalformedExpansion);
            };
            let name_end = name_start + relative_end;
            let name = &raw[name_start..name_end];
            if name.is_empty() || !valid_variable_name(name) {
                return Err(PathError::MalformedExpansion);
            }
            append_environment_value(&mut result, environment, name)?;
            cursor = name_end + 1;
            continue;
        }
        let name_start = cursor + 1;
        let mut name_end = name_start;
        while name_end < bytes.len()
            && (bytes[name_end].is_ascii_alphanumeric() || bytes[name_end] == b'_')
        {
            name_end += 1;
        }
        if name_end == name_start {
            result.push('$');
            cursor += 1;
            continue;
        }
        append_environment_value(&mut result, environment, &raw[name_start..name_end])?;
        cursor = name_end;
    }
    Ok(result)
}

fn expand_windows(raw: &str, environment: &RootEnvironment) -> Result<String, PathError> {
    let mut result = String::with_capacity(raw.len());
    let mut remainder = raw;
    while let Some(start) = remainder.find('%') {
        result.push_str(&remainder[..start]);
        let after_start = &remainder[start + 1..];
        let Some(end) = after_start.find('%') else {
            return Err(PathError::MalformedExpansion);
        };
        let name = &after_start[..end];
        if name.is_empty() || !valid_variable_name(name) {
            return Err(PathError::MalformedExpansion);
        }
        append_environment_value(&mut result, environment, name)?;
        remainder = &after_start[end + 1..];
    }
    result.push_str(remainder);
    Ok(result)
}

fn valid_variable_name(name: &str) -> bool {
    name.bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn append_environment_value(
    output: &mut String,
    environment: &RootEnvironment,
    name: &str,
) -> Result<(), PathError> {
    let value = environment
        .get(name)
        .ok_or_else(|| PathError::UndefinedVariable(name.to_owned()))?;
    output.push_str(
        value
            .to_str()
            .ok_or_else(|| PathError::NonUnicodeVariable(name.to_owned()))?,
    );
    Ok(())
}

fn expand_tilde(raw: &str, environment: &RootEnvironment) -> Result<String, PathError> {
    let suffix = if raw == "~" {
        Some("")
    } else {
        raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\"))
    };
    let Some(suffix) = suffix else {
        if raw.starts_with('~') {
            return Err(PathError::OtherUserTilde);
        }
        return Ok(raw.to_owned());
    };
    let home = if cfg!(target_os = "windows") {
        environment
            .get("USERPROFILE")
            .or_else(|| environment.get("HOME"))
    } else {
        environment.get("HOME")
    }
    .and_then(OsStr::to_str)
    .ok_or(PathError::MissingHome)?;
    if suffix.is_empty() {
        Ok(home.to_owned())
    } else {
        Ok(Path::new(home).join(suffix).to_string_lossy().into_owned())
    }
}

fn foreign_absolute_path(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\'))
        || raw.starts_with("\\\\")
}

/// Performs purely lexical dot/dot-dot normalization without resolving links.
#[must_use]
pub fn lexical_normalize(path: impl AsRef<Path>) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.as_ref().components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(
                    normalized.components().next_back(),
                    Some(Component::Normal(_))
                ) {
                    normalized.pop();
                } else if !normalized.has_root() {
                    normalized.push(component.as_os_str());
                }
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

/// Produces a stable absolute identity for lock and manifest hashing.
///
/// Existing paths resolve symlinks. Missing paths use an absolute lexical
/// representation, allowing a lock to be obtained before creation.
///
/// # Errors
///
/// Returns an I/O error if the current directory or canonical path cannot be
/// obtained.
pub fn canonical_identity(path: &Path) -> Result<PathBuf, PathError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| PathError::Io {
                path: PathBuf::from("."),
                source,
            })?
            .join(path)
    };
    let absolute = lexical_normalize(absolute);
    let mut ancestor = absolute.as_path();
    let mut missing = Vec::<OsString>::new();
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => {
                let mut identity = fs::canonicalize(ancestor).map_err(|source| PathError::Io {
                    path: ancestor.to_path_buf(),
                    source,
                })?;
                for component in missing.iter().rev() {
                    identity.push(component);
                }
                return Ok(lexical_normalize(identity));
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                let Some(component) = ancestor.file_name() else {
                    return Err(PathError::Io {
                        path: ancestor.to_path_buf(),
                        source,
                    });
                };
                missing.push(component.to_os_string());
                ancestor = ancestor.parent().ok_or_else(|| PathError::Io {
                    path: absolute.clone(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "path has no existing ancestor",
                    ),
                })?;
            }
            Err(source) => {
                return Err(PathError::Io {
                    path: ancestor.to_path_buf(),
                    source,
                });
            }
        }
    }
}

fn apply_metadata(mut path: PathBuf, metadata: &PathMetadata) -> Result<PathBuf, PathError> {
    if metadata.auto_create && !path.exists() {
        match metadata.expected_type {
            PathKind::Directory => fs::create_dir_all(&path),
            PathKind::File => path.parent().map_or(Ok(()), fs::create_dir_all),
        }
        .map_err(|source| PathError::Io {
            path: path.clone(),
            source,
        })?;
    }
    if metadata.must_exist && !path.exists() {
        return Err(PathError::Missing(path));
    }
    if path.exists() {
        let file_type = fs::metadata(&path)
            .map_err(|source| PathError::Io {
                path: path.clone(),
                source,
            })?
            .file_type();
        let correct_type = match metadata.expected_type {
            PathKind::File => file_type.is_file(),
            PathKind::Directory => file_type.is_dir(),
        };
        if !correct_type {
            return Err(PathError::WrongType(path));
        }
        match metadata.expected_type {
            PathKind::File => {
                fs::File::open(&path).map_err(|source| PathError::Io {
                    path: path.clone(),
                    source,
                })?;
            }
            PathKind::Directory => {
                fs::read_dir(&path).map_err(|source| PathError::Io {
                    path: path.clone(),
                    source,
                })?;
            }
        }
        if metadata.resolve_symlink {
            path = fs::canonicalize(&path).map_err(|source| PathError::Io {
                path: path.clone(),
                source,
            })?;
        }
    }
    Ok(path)
}

/// Path expansion and policy failures.
#[derive(Debug, Error)]
pub enum PathError {
    #[error("path value must not be empty")]
    Empty,
    #[error("path contains malformed environment expansion syntax")]
    MalformedExpansion,
    #[error("path references undefined environment variable {0}")]
    UndefinedVariable(String),
    #[error("path environment variable {0} is not valid Unicode")]
    NonUnicodeVariable(String),
    #[error("current-user home directory is unavailable")]
    MissingHome,
    #[error("~other-user path expansion is unsupported")]
    OtherUserTilde,
    #[error("path uses an absolute syntax from a different host platform")]
    ForeignAbsolute,
    #[error("required path does not exist: {0}")]
    Missing(PathBuf),
    #[error("path has the wrong filesystem type: {0}")]
    WrongType(PathBuf),
    #[error("filesystem operation failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use super::{ExpansionStyle, canonical_identity, expand_environment, lexical_normalize};
    use crate::roots::RootEnvironment;

    #[test]
    fn environment_expansion_is_exactly_one_pass() {
        let environment = RootEnvironment::from_values([
            ("FIRST", "$SECOND"),
            ("SECOND", "expanded"),
            ("WIN", "%OTHER%"),
            ("OTHER", "expanded"),
        ]);
        assert_eq!(
            expand_environment("$FIRST", &environment, ExpansionStyle::Unix)
                .expect("Unix expansion must succeed"),
            "$SECOND"
        );
        assert_eq!(
            expand_environment("%WIN%", &environment, ExpansionStyle::Windows)
                .expect("Windows expansion must succeed"),
            "%OTHER%"
        );
        assert!(expand_environment("$MISSING", &environment, ExpansionStyle::Unix).is_err());
    }

    #[test]
    fn lexical_normalization_does_not_touch_symlinks_or_case() {
        assert_eq!(
            lexical_normalize(Path::new("/Root/a/../B/./file")),
            Path::new("/Root/B/file")
        );
    }

    #[cfg(unix)]
    #[test]
    fn canonical_identity_resolves_the_longest_existing_symlinked_ancestor() {
        use std::os::unix::fs::symlink;

        let temporary = TempDir::new().expect("temporary directory must exist");
        let target = temporary.path().join("target");
        fs::create_dir(&target).expect("target directory must exist");
        let link = temporary.path().join("link");
        symlink(&target, &link).expect("directory symlink must be created");
        assert_eq!(
            canonical_identity(&link.join("missing/venv")).expect("identity must resolve"),
            target.join("missing/venv")
        );

        let broken = temporary.path().join("broken");
        symlink(temporary.path().join("absent"), &broken).expect("broken link must be created");
        assert!(canonical_identity(&broken).is_err());
    }
}
