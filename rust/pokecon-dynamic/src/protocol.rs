//! Closed payloads and operation names shared by the dynamic worker and its
//! Rust-main client. The transport envelope remains owned by `pokecon-worker`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use pokecon_device::controller::ControllerUpdate;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    CommandCacheBuildResult, CommandInfo, Diagnostic, DynamicConfigLanguage, DynamicLoadResult,
};

pub const INITIALIZE: &str = "dynamic.initialize";
pub const STATUS: &str = "dynamic.status";
pub const CONTROL: &str = "dynamic.control";
pub const EMIT: &str = "dynamic.emit";
pub const SORT_COMMANDS: &str = "dynamic.sort_commands";
pub const TAG_MATCHES: &str = "dynamic.tag_matches";
pub const BUILD_COMMAND_CACHE: &str = "dynamic.build_command_cache";

/// Private worker environment bridge used to add the exact synchronized venv
/// to embedded `CPython` without accepting ambient `PYTHONPATH` entries.
pub const PYTHON_SITE_PACKAGES_ENV: &str = "POKECON_INTERNAL_DYNAMIC_SITE_PACKAGES";

pub const HOST_SETTINGS_SNAPSHOT: &str = "dynamic.host.settings_snapshot";
pub const HOST_APPLY_SETTINGS: &str = "dynamic.host.apply_settings";
pub const HOST_STATE_SNAPSHOT: &str = "dynamic.host.state_snapshot";
pub const HOST_SET_STATE_VALUE: &str = "dynamic.host.set_state_value";
pub const HOST_PROFILE_CURRENT: &str = "dynamic.host.profile_current";
pub const HOST_PROFILE_LIST: &str = "dynamic.host.profile_list";
pub const HOST_PROFILE_SWITCH: &str = "dynamic.host.profile_switch";
pub const HOST_CONTROLLER_UPDATE: &str = "dynamic.host.controller_update";
pub const HOST_CONTROLLER_RESET: &str = "dynamic.host.controller_reset";

pub const DIAGNOSTIC_EVENT: &str = "dynamic.diagnostic";
pub const COMMAND_RECOMPUTE_EVENT: &str = "dynamic.command_recompute_requested";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicInitializeRequest {
    pub config_root: PathBuf,
    pub home: Option<PathBuf>,
    pub primary: DynamicConfigLanguage,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicInitializeResult {
    pub status: DynamicWorkerStatus,
    pub startup_load: Option<DynamicLoadResult>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicWorkerStatus {
    pub initialized: bool,
    pub generation: u64,
    pub initialized_languages: Vec<DynamicConfigLanguage>,
}

impl DynamicWorkerStatus {
    #[must_use]
    pub const fn uninitialized() -> Self {
        Self {
            initialized: false,
            generation: 0,
            initialized_languages: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicEmitRequest {
    pub event: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicEmitResult {
    pub cancelled: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicTagMatchRequest {
    pub selected_tag: String,
    pub command: CommandInfo,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicCommandCacheRequest {
    pub generation: u64,
    pub candidates: Vec<CommandInfo>,
}

pub type DynamicCommandCacheResult = CommandCacheBuildResult;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostSetStateValueRequest {
    pub name: String,
    pub value: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostProfileSwitchRequest {
    pub name: String,
}

pub type HostSettings = BTreeMap<String, Value>;
pub type HostState = BTreeMap<String, Value>;
pub type HostSettingsChanges = BTreeMap<String, Value>;
pub type HostControllerUpdate = ControllerUpdate;
pub type DynamicDiagnostic = Diagnostic;
