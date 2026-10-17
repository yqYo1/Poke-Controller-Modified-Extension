use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Runtime selected by a dynamic configuration source.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DynamicConfigLanguage {
    Python,
    Lua,
}

impl DynamicConfigLanguage {
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Python => "py",
            Self::Lua => "lua",
        }
    }

    #[must_use]
    pub fn from_path(path: &std::path::Path) -> Option<Self> {
        match path.extension().and_then(std::ffi::OsStr::to_str) {
            Some("py") => Some(Self::Python),
            Some("lua") => Some(Self::Lua),
            _ => None,
        }
    }
}

/// Closed REST/IPC operation union for dynamic configuration loading.
#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum DynamicConfigControl {
    LoadPath {
        path: String,
    },
    LoadContent {
        language: DynamicConfigLanguage,
        content: String,
    },
    Reload {},
}

impl std::fmt::Debug for DynamicConfigControl {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LoadPath { path } => formatter
                .debug_struct("LoadPath")
                .field("path", path)
                .finish(),
            Self::LoadContent { language, content } => formatter
                .debug_struct("LoadContent")
                .field("language", language)
                .field("content_bytes", &content.len())
                .finish(),
            Self::Reload {} => formatter.write_str("Reload"),
        }
    }
}

/// Last successfully selected source retained for reload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DynamicSource {
    pub path: PathBuf,
    pub language: DynamicConfigLanguage,
}

/// Secret-safe result returned by a load/reload operation. Evaluation errors
/// are represented by `loaded=false` while the prior generation stays active.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicLoadResult {
    pub display_path: String,
    pub language: DynamicConfigLanguage,
    pub loaded: bool,
    pub diagnostic: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{DynamicConfigControl, DynamicConfigLanguage};

    #[test]
    fn control_union_rejects_unknown_fields_and_debug_redacts_content() {
        let parsed: DynamicConfigControl = serde_json::from_str(
            r#"{"action":"load_content","language":"python","content":"token-value"}"#,
        )
        .unwrap();
        let debug = format!("{parsed:?}");
        assert!(!debug.contains("token-value"));
        assert!(debug.contains("11"));
        assert!(
            serde_json::from_str::<DynamicConfigControl>(
                r#"{"action":"reload","path":"unexpected"}"#
            )
            .is_err()
        );
        assert_eq!(DynamicConfigLanguage::Lua.extension(), "lua");
    }
}
