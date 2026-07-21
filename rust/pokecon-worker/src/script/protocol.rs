//! Closed user-script worker and reverse-host payloads.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const INITIALIZE: &str = "script.initialize";
pub const STATUS: &str = "script.status";
pub const EXECUTE: &str = "script.execute";
pub const STOP: &str = "script.stop";

/// Private worker environment bridge for the exact synchronized script venv.
pub const PYTHON_SITE_PACKAGES_ENV: &str = "POKECON_INTERNAL_SCRIPT_SITE_PACKAGES";

pub const HOST_CONTROLLER_INPUT: &str = "script.host.controller_input";
pub const HOST_CONTROLLER_NEUTRAL: &str = "script.host.controller_neutral";
pub const HOST_SERIAL_WRITE: &str = "script.host.serial_write";
pub const HOST_SERIAL_WRITE_ROW: &str = "script.host.serial_write_row";
pub const HOST_SERIAL_RELOAD: &str = "script.host.serial_reload";
pub const HOST_OUTPUT: &str = "script.host.output";
pub const HOST_DIALOG_OPEN: &str = "script.host.dialog_open";
pub const HOST_DIALOG_STATUS: &str = "script.host.dialog_status";
pub const HOST_DIALOG_CLOSE_ALL: &str = "script.host.dialog_close_all";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptInitializeRequest {
    pub profile: String,
    pub command_root: PathBuf,
    pub data_root: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptInitializeResult {
    pub status: ScriptWorkerStatus,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptWorkerStatus {
    pub initialized: bool,
    pub profile: Option<String>,
    pub running: bool,
    pub execution_id: Option<u64>,
}

impl ScriptWorkerStatus {
    #[must_use]
    pub const fn uninitialized() -> Self {
        Self {
            initialized: false,
            profile: None,
            running: false,
            execution_id: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptExecuteRequest {
    pub path: PathBuf,
    pub class_name: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum ScriptExecutionOutcome {
    Completed,
    Finished,
    Stopped,
    Failed { message: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptExecutionResult {
    pub execution_id: u64,
    pub outcome: ScriptExecutionOutcome,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptStopResult {
    pub stop_requested: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptInputAction {
    Press,
    Release,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ScriptButton {
    Y,
    B,
    A,
    X,
    L,
    R,
    Zl,
    Zr,
    Minus,
    Plus,
    Lclick,
    Rclick,
    Home,
    Capture,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptHat {
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
    TopLeft,
    Center,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptStick {
    Left,
    Right,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "kind")]
pub enum ScriptControl {
    Button { button: ScriptButton },
    Hat { direction: ScriptHat },
    Stick { stick: ScriptStick, x: u8, y: u8 },
    Touchscreen { x: u16, y: u16 },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostControllerInputRequest {
    pub action: ScriptInputAction,
    pub controls: Vec<ScriptControl>,
    pub unset_hat: bool,
    pub unset_touchscreen: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostSerialWriteRequest {
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostSerialWriteRowRequest {
    pub row: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ScriptOutputTarget {
    #[serde(rename = "stdout")]
    Stdout,
    #[serde(rename = "panel1")]
    Panel1,
    #[serde(rename = "panel2")]
    Panel2,
    #[serde(rename = "alternate")]
    Alternate,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScriptOutputMode {
    Write,
    Append,
    Delete,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostOutputRequest {
    pub target: ScriptOutputTarget,
    pub mode: Option<ScriptOutputMode>,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "value")]
pub enum ScriptDialogValue {
    None,
    String(String),
    Bool(bool),
    Integer(i64),
    Float(f64),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptDialogWidgetKind {
    Entry,
    Check,
    Combo,
    Radio,
    Spin,
    Scale,
    Next,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptDialogWidget {
    pub kind: ScriptDialogWidgetKind,
    pub label: Option<String>,
    pub value: ScriptDialogValue,
    pub options: Vec<ScriptDialogValue>,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub precision: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostDialogOpenRequest {
    pub title: String,
    pub description: Option<String>,
    pub widgets: Vec<ScriptDialogWidget>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostDialogOpenResult {
    pub dialog_id: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostDialogStatusRequest {
    pub dialog_id: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum ScriptDialogState {
    Open,
    Confirmed { values: Vec<ScriptDialogValue> },
    Aborted,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostDialogStatusResult {
    pub state: ScriptDialogState,
}
