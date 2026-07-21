use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::Value;

use crate::{COMPATIBILITY_FIXED_MANIFEST_JSON, ContractError, PROTOCOL_REGISTRY_JSON};

/// One generated file in the Python 3.14 `Commands` compatibility typing tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PythonTypingFile {
    relative_path: &'static str,
    source: String,
}

impl PythonTypingFile {
    #[must_use]
    pub const fn relative_path(&self) -> &'static str {
        self.relative_path
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
}

/// Generates the complete Python 3.14 user-script compatibility stub tree.
///
/// The protocol registry owns the public modules and member names. The
/// immutable compatibility manifest is checked as an additional projection so
/// a generated tree can never omit an import used by a fixed baseline script.
///
/// # Errors
///
/// Returns [`ContractError`] when either registry is malformed or its public
/// surface has drifted from the fully typed templates in this generator.
pub fn commands_python_typings() -> Result<Vec<PythonTypingFile>, ContractError> {
    validate_public_surface()?;
    validate_fixed_imports()?;
    Ok(COMMANDS_TYPING_FILES
        .iter()
        .map(|template| {
            let header = if matches!(
                template.relative_path,
                "commands.pyi" | "Commands/__init__.pyi" | "Commands/PythonCommandBase.pyi"
            ) {
                GENERATED_IMPORT_ORDER_EXEMPT_HEADER
            } else {
                GENERATED_FORMAT_EXEMPT_HEADER
            };
            PythonTypingFile {
                relative_path: template.relative_path,
                source: template.source.replacen(GENERATED_HEADER, header, 1),
            }
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct ProtocolRegistry {
    public_surfaces: Vec<PublicSurface>,
}

#[derive(Debug, Deserialize)]
struct PublicSurface {
    worker: String,
    languages: Vec<String>,
    namespaces: Vec<String>,
    #[serde(default)]
    required_imports: Vec<RequiredImport>,
    #[serde(default)]
    class_members: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    module_functions: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct RequiredImport {
    module: String,
    symbols: Vec<String>,
}

// This function is intentionally a literal projection of every registry-owned
// public name; splitting the table would make cross-namespace review harder.
#[allow(clippy::too_many_lines)]
fn validate_public_surface() -> Result<(), ContractError> {
    let registry = serde_json::from_str::<ProtocolRegistry>(PROTOCOL_REGISTRY_JSON)?;
    let surface = registry
        .public_surfaces
        .iter()
        .find(|surface| surface.worker == "user_script")
        .ok_or_else(|| invariant("protocol registry has no user_script public surface"))?;

    require_equal(
        "user-script languages",
        &surface.languages,
        &strings(&["python"]),
    )?;
    require_equal(
        "user-script namespaces",
        &surface.namespaces,
        &strings(&[
            "Commands.PythonCommandBase",
            "Commands.McuCommandBase",
            "Commands.Keys",
            "Commands.dialogue",
            "Commands.image_proc",
            "Commands.net",
            "Commands.PythonCommands.bridge_functions.bridge_functions",
        ]),
    )?;

    let required_imports = surface
        .required_imports
        .iter()
        .map(|required| (required.module.as_str(), required.symbols.clone()))
        .collect::<BTreeMap<_, _>>();
    let expected_imports = BTreeMap::from([
        (
            "Commands.Keys",
            strings(&[
                "Button",
                "Hat",
                "Direction",
                "Touchscreen",
                "KeyPress",
                "Stick",
            ]),
        ),
        ("Commands.McuCommandBase", strings(&["McuCommand"])),
        (
            "Commands.PythonCommandBase",
            strings(&["PythonCommand", "ImageProcPythonCommand"]),
        ),
        (
            "Commands.PythonCommands.bridge_functions.bridge_functions",
            strings(&["BridgeFunctions"]),
        ),
    ]);
    require_equal(
        "user-script required imports",
        &required_imports,
        &expected_imports,
    )?;

    let expected_classes = BTreeMap::from([
        (
            "Camera".to_owned(),
            strings(&[
                "image_bgr",
                "readFrame",
                "isOpened",
                "fps",
                "capture_size",
                "flip",
                "flip_mode",
                "set_flip",
                "saveCapture",
                "openCamera",
                "destroy",
                "camera_thread_start",
                "camera_thread_stop",
                "camera_update",
            ]),
        ),
        (
            "CaptureArea".to_owned(),
            strings(&[
                "ImgRect",
                "ImgText",
                "deleteImageRect",
                "deleteImageText",
                "setFps",
                "setShowsize",
                "changeRightMouseMode",
                "setTouchscreenArea",
                "saveCapture",
                "show_size",
                "is_show_var",
                "update",
                "BindLeftClick",
                "BindRightClick",
                "UnbindLeftClick",
                "UnbindRightClick",
                "mouseCtrlLeftPress",
                "mouseLeftPress",
                "mouseLeftPressing",
                "mouseRightPress",
                "mouseRightPressing",
                "StartRangeSS",
                "MotionRangeSS",
                "ReleaseRangeSS",
            ]),
        ),
        (
            "ImageProcPythonCommand".to_owned(),
            strings(&[
                "camera",
                "cam",
                "gui",
                "canvas",
                "discord_image",
                "LINE_image",
                "isContainTemplate",
                "isContainTemplate_max",
                "isContainTemplateGPU",
                "isContainedImage",
                "saveCapture",
                "popupImage",
                "getCameraImage",
                "openImage",
                "setTemplateDir",
                "get_filespec",
                "displayRectangle",
                "displayText",
            ]),
        ),
        (
            "KeyPress".to_owned(),
            strings(&[
                "input", "inputEnd", "hold", "holdEnd", "neutral", "end", "ser",
            ]),
        ),
        ("McuCommand".to_owned(), strings(&["start", "end"])),
        (
            "PythonCommand".to_owned(),
            strings(&[
                "_logger",
                "keys",
                "do",
                "finish",
                "checkIfAlive",
                "press",
                "pressRep",
                "hold",
                "holdEnd",
                "wait",
                "short_wait",
                "direct_serial",
                "reload_com_port",
                "print_t1",
                "print_t2",
                "print_t",
                "print_s",
                "print_ts",
                "print_t1b",
                "print_t2b",
                "print_tb",
                "print_tbs",
                "show_var",
                "show_dialog",
                "is_dialog_closed",
                "wait_dialog",
                "dialogue6widget",
                "dialogue6widget_save_settings",
                "dialogue6widget_select_settings",
                "dialogue",
                "socket_connect",
                "socket_disconnect",
                "socket_transmit_message",
                "socket_receive_message",
                "socket_receive_message2",
                "socket_change_ipaddr",
                "socket_change_port",
                "socket_change_alive",
                "mqtt_transmit_message",
                "mqtt_receive_message",
                "mqtt_receive_message2",
                "mqtt_change_broker_address",
                "mqtt_change_id",
                "mqtt_change_clientId",
                "mqtt_change_pub_token",
                "mqtt_change_sub_token",
                "LINE_text",
                "discord_text",
            ]),
        ),
        ("Sender".to_owned(), strings(&["writeRow", "write"])),
        ("Widget".to_owned(), strings(&["value", "has_result"])),
    ]);
    require_equal(
        "user-script class members",
        &surface.class_members,
        &expected_classes,
    )?;

    let expected_functions = BTreeMap::from([
        (
            "Commands.dialogue".to_owned(),
            strings(&[
                "Widget",
                "show_dialog",
                "is_dialog_closed",
                "wait_dialog",
                "dialogue6widget",
                "dialogue6widget_save_settings",
                "dialogue6widget_select_settings",
                "dialogue",
            ]),
        ),
        (
            "Commands.image_proc".to_owned(),
            strings(&[
                "isContainTemplate",
                "isContainTemplate_max",
                "isContainTemplateGPU",
                "isContainedImage",
                "saveCapture",
                "popupImage",
                "getCameraImage",
                "openImage",
                "setTemplateDir",
                "get_filespec",
                "displayRectangle",
                "displayText",
            ]),
        ),
        (
            "Commands.net".to_owned(),
            strings(&[
                "socket_connect",
                "socket_disconnect",
                "socket_transmit_message",
                "socket_receive_message",
                "socket_receive_message2",
                "socket_change_ipaddr",
                "socket_change_port",
                "socket_change_alive",
                "mqtt_transmit_message",
                "mqtt_receive_message",
                "mqtt_receive_message2",
                "mqtt_change_broker_address",
                "mqtt_change_id",
                "mqtt_change_clientId",
                "mqtt_change_pub_token",
                "mqtt_change_sub_token",
            ]),
        ),
    ]);
    require_equal(
        "user-script module functions",
        &surface.module_functions,
        &expected_functions,
    )
}

fn validate_fixed_imports() -> Result<(), ContractError> {
    let manifest = serde_json::from_str::<Value>(COMPATIBILITY_FIXED_MANIFEST_JSON)?;
    let baselines = manifest["baselines"]
        .as_array()
        .ok_or_else(|| invariant("fixed compatibility manifest has no baselines array"))?;
    let mut fixed_imports = BTreeMap::<String, BTreeSet<String>>::new();
    for baseline in baselines {
        let scripts = baseline["scripts"]
            .as_array()
            .ok_or_else(|| invariant("fixed compatibility baseline has no scripts array"))?;
        for script in scripts {
            let imports = script["imports"]
                .as_array()
                .ok_or_else(|| invariant("fixed compatibility script has no imports array"))?;
            for import in imports {
                let Some(module) = import["module"].as_str() else {
                    continue;
                };
                if !module.starts_with("Commands.") {
                    continue;
                }
                let names = import["names"].as_array().ok_or_else(|| {
                    invariant(format!("fixed import {module} has no names array"))
                })?;
                let entry = fixed_imports.entry(module.to_owned()).or_default();
                for name in names {
                    let name = name.as_str().ok_or_else(|| {
                        invariant(format!("fixed import {module} has a non-string name"))
                    })?;
                    entry.insert(name.to_owned());
                }
            }
        }
    }

    let registry = serde_json::from_str::<ProtocolRegistry>(PROTOCOL_REGISTRY_JSON)?;
    let surface = registry
        .public_surfaces
        .iter()
        .find(|surface| surface.worker == "user_script")
        .ok_or_else(|| invariant("protocol registry has no user_script public surface"))?;
    let declared = surface
        .required_imports
        .iter()
        .map(|required| {
            (
                required.module.as_str(),
                required
                    .symbols
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for (module, names) in fixed_imports {
        let available = declared.get(module.as_str()).ok_or_else(|| {
            invariant(format!(
                "fixed baseline imports undeclared Commands module {module}"
            ))
        })?;
        for name in names {
            if !available.contains(name.as_str()) {
                return Err(invariant(format!(
                    "fixed baseline imports undeclared symbol {module}.{name}"
                )));
            }
        }
    }
    Ok(())
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn require_equal<T>(label: &str, actual: &T, expected: &T) -> Result<(), ContractError>
where
    T: std::fmt::Debug + PartialEq,
{
    if actual == expected {
        Ok(())
    } else {
        Err(invariant(format!(
            "{label} drifted; expected {expected:?}, found {actual:?}"
        )))
    }
}

fn invariant(message: impl Into<String>) -> ContractError {
    ContractError::Invariant(message.into())
}

#[derive(Clone, Copy)]
struct PythonTypingTemplate {
    relative_path: &'static str,
    source: &'static str,
}

const COMMANDS_TYPING_FILES: &[PythonTypingTemplate] = &[
    PythonTypingTemplate {
        relative_path: "commands.pyi",
        source: COMMANDS_AGGREGATE,
    },
    PythonTypingTemplate {
        relative_path: "Commands/__init__.pyi",
        source: COMMANDS_INIT,
    },
    PythonTypingTemplate {
        relative_path: "Commands/Sender.pyi",
        source: SENDER,
    },
    PythonTypingTemplate {
        relative_path: "Commands/Keys.pyi",
        source: KEYS,
    },
    PythonTypingTemplate {
        relative_path: "Commands/dialogue.pyi",
        source: DIALOGUE,
    },
    PythonTypingTemplate {
        relative_path: "Commands/net.pyi",
        source: NET,
    },
    PythonTypingTemplate {
        relative_path: "Commands/PythonCommandBase.pyi",
        source: PYTHON_COMMAND_BASE,
    },
    PythonTypingTemplate {
        relative_path: "Commands/image_proc.pyi",
        source: IMAGE_PROC,
    },
    PythonTypingTemplate {
        relative_path: "Commands/McuCommandBase.pyi",
        source: MCU_COMMAND_BASE,
    },
    PythonTypingTemplate {
        relative_path: "Commands/PythonCommands/__init__.pyi",
        source: GENERATED_HEADER,
    },
    PythonTypingTemplate {
        relative_path: "Commands/PythonCommands/bridge_functions/__init__.pyi",
        source: BRIDGE_INIT,
    },
    PythonTypingTemplate {
        relative_path: "Commands/PythonCommands/bridge_functions/bridge_functions.pyi",
        source: BRIDGE_FUNCTIONS,
    },
];

const GENERATED_HEADER: &str = "# Generated by `nix run .#generate-contracts`; do not edit.\n";
const GENERATED_FORMAT_EXEMPT_HEADER: &str =
    "# Generated by `nix run .#generate-contracts`; do not edit.\n# fmt: off\n";
const GENERATED_IMPORT_ORDER_EXEMPT_HEADER: &str =
    "# Generated by `nix run .#generate-contracts`; do not edit.\n# ruff: noqa: I001\n# fmt: off\n";

const COMMANDS_AGGREGATE: &str = r"# Generated by `nix run .#generate-contracts`; do not edit.
from Commands.Keys import Button as Button, Direction as Direction, Hat as Hat, KeyPress as KeyPress, Stick as Stick, Touchscreen as Touchscreen
from Commands.McuCommandBase import McuCommand as McuCommand
from Commands.PythonCommandBase import Camera as Camera, CaptureArea as CaptureArea, ImageProcPythonCommand as ImageProcPythonCommand, PythonCommand as PythonCommand, StopThread as StopThread
from Commands.PythonCommands.bridge_functions.bridge_functions import BridgeFunctions as BridgeFunctions
from Commands.dialogue import Widget as Widget
";

const COMMANDS_INIT: &str = r"# Generated by `nix run .#generate-contracts`; do not edit.
from . import dialogue as dialogue, image_proc as image_proc, net as net
";

const SENDER: &str = r"# Generated by `nix run .#generate-contracts`; do not edit.
class Sender:
    def writeRow(self, row: str) -> None: ...
    def write(self, data: bytes | bytearray | memoryview | list[int]) -> None: ...
";

const KEYS: &str = r"# Generated by `nix run .#generate-contracts`; do not edit.
from enum import Enum, IntEnum, IntFlag
from typing import ClassVar

from .Sender import Sender

class Button(IntFlag):
    Y = 1
    B = 2
    A = 4
    X = 8
    L = 16
    R = 32
    ZL = 64
    ZR = 128
    MINUS = 256
    PLUS = 512
    LCLICK = 1024
    RCLICK = 2048
    HOME = 4096
    CAPTURE = 8192
    SELECT = MINUS
    START = PLUS
    POWER = LCLICK
    WIRELESS = RCLICK

class Hat(IntEnum):
    TOP = 0
    TOP_RIGHT = 1
    RIGHT = 2
    BTM_RIGHT = 3
    BTM = 4
    BTM_LEFT = 5
    LEFT = 6
    TOP_LEFT = 7
    CENTER = 8

class Stick(Enum):
    LEFT = 1
    RIGHT = 2

class Direction:
    UP: ClassVar[Direction]
    RIGHT: ClassVar[Direction]
    DOWN: ClassVar[Direction]
    LEFT: ClassVar[Direction]
    UP_RIGHT: ClassVar[Direction]
    DOWN_RIGHT: ClassVar[Direction]
    DOWN_LEFT: ClassVar[Direction]
    UP_LEFT: ClassVar[Direction]
    R_UP: ClassVar[Direction]
    R_RIGHT: ClassVar[Direction]
    R_DOWN: ClassVar[Direction]
    R_LEFT: ClassVar[Direction]
    R_UP_RIGHT: ClassVar[Direction]
    R_DOWN_RIGHT: ClassVar[Direction]
    R_DOWN_LEFT: ClassVar[Direction]
    R_UP_LEFT: ClassVar[Direction]

    stick: Stick
    x: int
    y: int
    showName: str | None

    def __init__(self, stick: Stick, angle: tuple[int, int] | float, magnification: float = 1.0, isDegree: bool = True, showName: str | None = None) -> None: ...
    @property
    def name(self) -> str: ...

class Touchscreen:
    x: int
    y: int

    def __init__(self, x: int, y: int) -> None: ...
    @property
    def name(self) -> str: ...

type Buttons = Button | Hat | Direction | Touchscreen
type ButtonsList = list[Buttons]
type GamepadInput = ButtonsList | Buttons

class KeyPress:
    ser: Sender

    def __init__(self, ser: Sender) -> None: ...
    def input(self, btns: GamepadInput, ifPrint: bool = True) -> None: ...
    def inputEnd(self, btns: GamepadInput, ifPrint: bool = True, unset_hat: bool = True, unset_Touchscreen: bool = True) -> None: ...
    def hold(self, btns: GamepadInput) -> None: ...
    def holdEnd(self, btns: GamepadInput) -> None: ...
    def neutral(self) -> None: ...
    def end(self) -> None: ...
";

const DIALOGUE: &str = r#"# Generated by `nix run .#generate-contracts`; do not edit.
from typing import Literal, overload

class Widget[T]:
    @overload
    def __init__(self: Widget[str], widget_type: Literal["Entry"], label: str, default: str) -> None: ...
    @overload
    def __init__(self: Widget[bool], widget_type: Literal["Check"], label: str, default: bool) -> None: ...
    @overload
    def __init__[U](self: Widget[U], widget_type: Literal["Combo"], label: str, options: list[U], default: U) -> None: ...
    @overload
    def __init__(self: Widget[int], widget_type: Literal["Spin"], label: str, options: list[int], default: int) -> None: ...
    @overload
    def __init__(self: Widget[int], widget_type: Literal["Spin"], label: str, min: int, max: int, default: int) -> None: ...
    @overload
    def __init__(self: Widget[float], widget_type: Literal["Scale"], label: str, min: float, max: float, default: float) -> None: ...
    @overload
    def __init__(self: Widget[None], widget_type: Literal["Next"]) -> None: ...

    value: T
    @property
    def has_result(self) -> bool: ...

type DialogueWidget = Widget[str] | Widget[int] | Widget[float] | Widget[bool] | Widget[None]
type DialogueWidgets = list[DialogueWidget] | DialogueWidget
type DialogueEntryItem = str | bool | int | float | list[str]
type DialogueEntry = list[DialogueEntryItem]
type DialogueList = list[DialogueEntry]

@overload
def show_dialog(title: str, widgets: DialogueWidgets, blocking: Literal[True] = True) -> Literal[0]: ...
@overload
def show_dialog(title: str, widgets: DialogueWidgets, blocking: Literal[False]) -> int: ...
def is_dialog_closed(dialog_id: int) -> bool: ...
def wait_dialog(dialog_id: int) -> Literal[0]: ...

@overload
def dialogue6widget(title: str, dialogue_list: DialogueList, desc: str | None = None, need: type[list[object]] = list) -> list[str]: ...
@overload
def dialogue6widget(title: str, dialogue_list: DialogueList, desc: str | None = None, *, need: type[dict[object, object]]) -> dict[int | str, str]: ...
@overload
def dialogue6widget(title: str, dialogue_list: DialogueList, desc: str | None, need: type[dict[object, object]]) -> dict[int | str, str]: ...
@overload
def dialogue6widget_save_settings(title: str, dialogue_list: DialogueList, filename: str, desc: str | None = None, need: type[list[object]] = list) -> list[str]: ...
@overload
def dialogue6widget_save_settings(title: str, dialogue_list: DialogueList, filename: str, desc: str | None = None, *, need: type[dict[object, object]]) -> dict[int | str, str]: ...
@overload
def dialogue6widget_save_settings(title: str, dialogue_list: DialogueList, filename: str, desc: str | None, need: type[dict[object, object]]) -> dict[int | str, str]: ...
@overload
def dialogue6widget_select_settings(title: str, dialogue_list: DialogueList, dirname: str, desc: str | None = None, need: type[list[object]] = list) -> list[str]: ...
@overload
def dialogue6widget_select_settings(title: str, dialogue_list: DialogueList, dirname: str, desc: str | None = None, *, need: type[dict[object, object]]) -> dict[int | str, str]: ...
@overload
def dialogue6widget_select_settings(title: str, dialogue_list: DialogueList, dirname: str, desc: str | None, need: type[dict[object, object]]) -> dict[int | str, str]: ...
@overload
def dialogue(title: str, message: int | str | list[int | str], desc: str | None = None, need: type[list[object]] = list) -> list[str]: ...
@overload
def dialogue(title: str, message: int | str | list[int | str], desc: str | None = None, *, need: type[dict[object, object]]) -> dict[int | str, str]: ...
@overload
def dialogue(title: str, message: int | str | list[int | str], desc: str | None, need: type[dict[object, object]]) -> dict[int | str, str]: ...
"#;

const NET: &str = r"# Generated by `nix run .#generate-contracts`; do not edit.
def socket_connect() -> None: ...
def socket_disconnect() -> None: ...
def socket_transmit_message(message: str) -> None: ...
def socket_receive_message(header: str, show_msg: bool = False) -> str | None: ...
def socket_receive_message2(headerlist: list[str], show_msg: bool = False) -> str | None: ...
def socket_change_ipaddr(addr: str) -> None: ...
def socket_change_port(port: int) -> None: ...
def socket_change_alive(flag: bool) -> None: ...
def mqtt_transmit_message(roomid: str, message: str) -> None: ...
def mqtt_receive_message(roomid: str, header: str, show_msg: bool = False) -> str | None: ...
def mqtt_receive_message2(roomid: str, headerlist: list[str], show_msg: bool = False) -> str | None: ...
def mqtt_change_broker_address(broker_address: str) -> None: ...
def mqtt_change_id(mqtt_id: str) -> None: ...
def mqtt_change_clientId(clientId: str) -> None: ...
def mqtt_change_pub_token(pub_token: str) -> None: ...
def mqtt_change_sub_token(sub_token: str) -> None: ...
";

const PYTHON_COMMAND_BASE: &str = r#"# Generated by `nix run .#generate-contracts`; do not edit.
from collections.abc import Sequence
from logging import Logger
from typing import Literal, overload

from cv2.typing import MatLike  # pyright: ignore[reportUnknownVariableType]

from .Keys import GamepadInput, KeyPress
from .dialogue import DialogueList, DialogueWidgets

type CropFmt = Literal["", "1", "2", "3", "4", "11", "12", "13", "14"]
type ScreenshotFormat = Literal["png", "jpeg"]
type BgrRange = dict[Literal["lower", "upper"], int | tuple[int, int, int]]

class StopThread(Exception): ...

class Camera:
    def __init__(self, fps: int = 45) -> None: ...
    @property
    def image_bgr(self) -> MatLike: ...
    def readFrame(self) -> MatLike: ...
    def isOpened(self) -> bool: ...
    @property
    def fps(self) -> int: ...
    @fps.setter
    def fps(self, value: int) -> None: ...
    @property
    def capture_size(self) -> tuple[int, int]: ...
    @property
    def flip(self) -> bool: ...
    @property
    def flip_mode(self) -> int: ...
    def set_flip(self, value: Literal["None", "Vertical", "Horizontal", "Both"]) -> None: ...
    def saveCapture(self, filename: str | None = None, crop: int | Literal["1"] | Literal["2"] | None = None, crop_ax: list[int] | None = None, img: MatLike | None = None, format: ScreenshotFormat | None = None) -> None: ...  # noqa: A002
    def openCamera(self, cameraId: int | str) -> None: ...
    def destroy(self) -> None: ...
    def camera_thread_start(self) -> None: ...
    def camera_thread_stop(self) -> None: ...
    def camera_update(self) -> None: ...

class CaptureArea:
    @property
    def show_size(self) -> tuple[int, int]: ...
    @property
    def is_show_var(self) -> bool: ...
    def ImgRect(self, x1: int, y1: int, x2: int, y2: int, outline: str, tag: str | int, ms: int, flag: bool = True) -> None: ...
    def ImgText(self, x1: int, y1: int, txt: str, tag: str | int, ms: int, ft: tuple[str, int] = ("UD デジタル 教科書体 NP-B", 20), color: str = "black", flag: bool = True) -> None: ...
    def deleteImageRect(self, tag: int | str) -> None: ...
    def deleteImageText(self, tag: int | str) -> None: ...
    def setFps(self, fps: str | int) -> None: ...
    def setShowsize(self, show_height: int, show_width: int) -> None: ...
    def changeRightMouseMode(self, mode: str) -> None: ...
    def setTouchscreenArea(self, x1: int, y1: int, x2: int, y2: int) -> None: ...
    def saveCapture(self) -> None: ...
    def update(self) -> None: ...
    def BindLeftClick(self) -> None: ...
    def BindRightClick(self) -> None: ...
    def UnbindLeftClick(self) -> None: ...
    def UnbindRightClick(self) -> None: ...
    def mouseCtrlLeftPress(self, event: object) -> None: ...
    def mouseLeftPress(self, event: object, keys_: KeyPress) -> None: ...
    def mouseLeftPressing(self, event: object, keys_: KeyPress) -> None: ...
    def mouseRightPress(self, event: object, keys_: KeyPress) -> None: ...
    def mouseRightPressing(self, event: object, keys_: KeyPress) -> None: ...
    def StartRangeSS(self, event: object) -> None: ...
    def MotionRangeSS(self, event: object) -> None: ...
    def ReleaseRangeSS(self, event: object) -> None: ...

class PythonCommand:
    _logger: Logger
    keys: KeyPress

    def __init__(self) -> None: ...
    def do(self) -> None: ...
    def finish(self) -> None: ...
    def checkIfAlive(self) -> Literal[True]: ...
    def press(self, buttons: GamepadInput, duration: float = 0.1, wait: float = 0.1) -> None: ...
    def pressRep(self, buttons: GamepadInput, repeat: int, duration: float = 0.1, interval: float = 0.1, wait: float = 0.1) -> None: ...
    def hold(self, buttons: GamepadInput, wait: float = 0.1) -> None: ...
    def holdEnd(self, buttons: GamepadInput) -> None: ...
    def wait(self, wait: float) -> None: ...
    def short_wait(self, wait: float) -> None: ...
    def direct_serial(self, commands: list[str], waittimes: list[float]) -> None: ...
    def reload_com_port(self) -> None: ...
    def print_t1(self, *objects: object, sep: str = " ", end: str = "\n") -> None: ...
    def print_t2(self, *objects: object, sep: str = " ", end: str = "\n") -> None: ...
    def print_t(self, *objects: object, sep: str = " ", end: str = "\n") -> None: ...
    def print_s(self, *objects: object, sep: str = " ", end: str = "\n") -> None: ...
    def print_ts(self, *objects: object, sep: str = " ", end: str = "\n") -> None: ...
    def print_t1b(self, mode: Literal["w", "a", "d"], *objects: object, sep: str = " ", end: str = "\n") -> None: ...
    def print_t2b(self, mode: Literal["w", "a", "d"], *objects: object, sep: str = " ", end: str = "\n") -> None: ...
    def print_tb(self, mode: Literal["w", "a", "d"], *objects: object, sep: str = " ", end: str = "\n") -> None: ...
    def print_tbs(self, mode: Literal["w", "a", "d"], *objects: object, sep: str = " ", end: str = "\n") -> None: ...
    def show_var(self) -> None: ...
    @overload
    def show_dialog(self, title: str, widgets: DialogueWidgets, blocking: Literal[True] = True) -> Literal[0]: ...
    @overload
    def show_dialog(self, title: str, widgets: DialogueWidgets, blocking: Literal[False]) -> int: ...
    def is_dialog_closed(self, dialog_id: int) -> bool: ...
    def wait_dialog(self, dialog_id: int) -> Literal[0]: ...
    @overload
    def dialogue6widget(self, title: str, dialogue_list: DialogueList, desc: str | None = None, need: type[list[object]] = list) -> list[str]: ...
    @overload
    def dialogue6widget(self, title: str, dialogue_list: DialogueList, desc: str | None = None, *, need: type[dict[object, object]]) -> dict[int | str, str]: ...
    @overload
    def dialogue6widget(self, title: str, dialogue_list: DialogueList, desc: str | None, need: type[dict[object, object]]) -> dict[int | str, str]: ...
    @overload
    def dialogue6widget_save_settings(self, title: str, dialogue_list: DialogueList, filename: str, desc: str | None = None, need: type[list[object]] = list) -> list[str]: ...
    @overload
    def dialogue6widget_save_settings(self, title: str, dialogue_list: DialogueList, filename: str, desc: str | None = None, *, need: type[dict[object, object]]) -> dict[int | str, str]: ...
    @overload
    def dialogue6widget_save_settings(self, title: str, dialogue_list: DialogueList, filename: str, desc: str | None, need: type[dict[object, object]]) -> dict[int | str, str]: ...
    @overload
    def dialogue6widget_select_settings(self, title: str, dialogue_list: DialogueList, dirname: str, desc: str | None = None, need: type[list[object]] = list) -> list[str]: ...
    @overload
    def dialogue6widget_select_settings(self, title: str, dialogue_list: DialogueList, dirname: str, desc: str | None = None, *, need: type[dict[object, object]]) -> dict[int | str, str]: ...
    @overload
    def dialogue6widget_select_settings(self, title: str, dialogue_list: DialogueList, dirname: str, desc: str | None, need: type[dict[object, object]]) -> dict[int | str, str]: ...
    @overload
    def dialogue(self, title: str, message: int | str | list[int | str], desc: str | None = None, need: type[list[object]] = list) -> list[str]: ...
    @overload
    def dialogue(self, title: str, message: int | str | list[int | str], desc: str | None = None, *, need: type[dict[object, object]]) -> dict[int | str, str]: ...
    @overload
    def dialogue(self, title: str, message: int | str | list[int | str], desc: str | None, need: type[dict[object, object]]) -> dict[int | str, str]: ...
    def socket_connect(self) -> None: ...
    def socket_disconnect(self) -> None: ...
    def socket_transmit_message(self, message: str) -> None: ...
    def socket_receive_message(self, header: str, show_msg: bool = False) -> str | None: ...
    def socket_receive_message2(self, headerlist: list[str], show_msg: bool = False) -> str | None: ...
    def socket_change_ipaddr(self, addr: str) -> None: ...
    def socket_change_port(self, port: int) -> None: ...
    def socket_change_alive(self, flag: bool) -> None: ...
    def mqtt_transmit_message(self, roomid: str, message: str) -> None: ...
    def mqtt_receive_message(self, roomid: str, header: str, show_msg: bool = False) -> str | None: ...
    def mqtt_receive_message2(self, roomid: str, headerlist: list[str], show_msg: bool = False) -> str | None: ...
    def mqtt_change_broker_address(self, broker_address: str) -> None: ...
    def mqtt_change_id(self, mqtt_id: str) -> None: ...
    def mqtt_change_clientId(self, clientId: str) -> None: ...
    def mqtt_change_pub_token(self, pub_token: str) -> None: ...
    def mqtt_change_sub_token(self, sub_token: str) -> None: ...
    def LINE_text(self, txt: str, token: str = "") -> None: ...
    def discord_text(self, content: str = "", index: int = 0, keys: str = "DISCORD_WEBHOOK") -> None: ...

class ImageProcPythonCommand(PythonCommand):
    camera: Camera
    cam: Camera
    gui: CaptureArea
    canvas: CaptureArea

    def __init__(self, cam: Camera, gui: CaptureArea | None = None) -> None: ...
    def discord_image(self, content: str = "", index: int = 0, crop_fmt: CropFmt = "", crop: list[int] | None = None, keys: str | list[str] = "DISCORD_WEBHOOK") -> None: ...
    def LINE_image(self, txt: str, crop_fmt: str = "", crop: list[int] | None = None, token: str = "") -> None: ...
    def isContainTemplate(self, template_path: str, threshold: float = 0.7, use_gray: bool = True, show_value: bool = False, show_position: bool = True, show_only_true_rect: bool = True, ms: float = 2000, crop_fmt: CropFmt = "", crop: list[int] | None = None, mask_path: str | None = None, use_gpu: bool = False, BGR_range: BgrRange | None = None, threshold_binary: int | None = None, crop_template: list[int] | None = None, show_image: bool = False, color: list[str] | None = None) -> bool: ...
    def isContainTemplate_max(self, template_path_list: list[str], threshold: float = 0.7, use_gray: bool = True, show_value: bool = False, show_position: bool = True, show_only_true_rect: bool = True, ms: float = 2000, crop_fmt: CropFmt = "", crop: list[int] | None = None, mask_path_list: list[str | None] | None = None, BGR_range: BgrRange | None = None, threshold_binary: int | None = None, crop_template: list[int] | None = None, show_image: bool = False, color: list[str] | None = None) -> tuple[int, list[float], list[bool]]: ...
    def isContainTemplateGPU(self, template_path: str, threshold: float = 0.7, use_gray: bool = True, show_value: bool = False, show_position: bool = True, show_only_true_rect: bool = True, ms: float = 2000, crop_fmt: CropFmt = "", crop: list[int] | None = None, mask_path: str | None = None, BGR_range: BgrRange | None = None, threshold_binary: int | None = None, crop_template: list[int] | None = None, show_image: bool = False, color: list[str] | None = None) -> bool: ...
    def isContainedImage(self, image_path: str, threshold: float = 0.7, use_gray: bool = True, show_value: bool = False, show_position: bool = True, show_only_true_rect: bool = True, ms: float = 2000, crop_fmt: CropFmt = "", crop: list[int] | None = None, mask_path: str | None = None, use_gpu: bool = False, BGR_range: BgrRange | None = None, threshold_binary: int | None = None, crop_template: list[int] | None = None, show_image: bool = False, color: list[str] | None = None) -> bool: ...
    def saveCapture(self, filename: str | None = None, crop_fmt: CropFmt = "", crop: list[int] | None = None, mode: bool = True, format: ScreenshotFormat | None = None) -> None: ...  # noqa: A002
    def popupImage(self, crop_fmt: CropFmt = "", crop: list[int] | None = None, title: str = "image") -> None: ...
    def getCameraImage(self, crop_fmt: CropFmt = "", crop: list[int] | None = None) -> MatLike: ...
    def openImage(self, filename: str, mode: str = "t") -> MatLike | None: ...
    def setTemplateDir(self, path: str) -> None: ...
    def get_filespec(self, filename: str, mode: str = "t") -> str: ...
    def displayRectangle(self, max_loc: list[int] | Sequence[int], width: int, height: int, tag: str | None = None, ms: float = 2000, color: list[str] | None = None, crop_fmt: CropFmt = "", crop: list[int] | None = None) -> None: ...
    def displayText(self, position: Sequence[int], txt: str, tag: str | None = None, ms: float = 2000, font: str = "UD デジタル 教科書体 NP-B", fontsize: int = 20, color: str = "black") -> None: ...
"#;

const IMAGE_PROC: &str = r#"# Generated by `nix run .#generate-contracts`; do not edit.
from collections.abc import Sequence

from cv2.typing import MatLike  # pyright: ignore[reportUnknownVariableType]

from .PythonCommandBase import BgrRange, CropFmt, ScreenshotFormat

def isContainTemplate(template_path: str, threshold: float = 0.7, use_gray: bool = True, show_value: bool = False, show_position: bool = True, show_only_true_rect: bool = True, ms: float = 2000, crop_fmt: CropFmt = "", crop: list[int] | None = None, mask_path: str | None = None, use_gpu: bool = False, BGR_range: BgrRange | None = None, threshold_binary: int | None = None, crop_template: list[int] | None = None, show_image: bool = False, color: list[str] | None = None) -> bool: ...
def isContainTemplate_max(template_path_list: list[str], threshold: float = 0.7, use_gray: bool = True, show_value: bool = False, show_position: bool = True, show_only_true_rect: bool = True, ms: float = 2000, crop_fmt: CropFmt = "", crop: list[int] | None = None, mask_path_list: list[str | None] | None = None, BGR_range: BgrRange | None = None, threshold_binary: int | None = None, crop_template: list[int] | None = None, show_image: bool = False, color: list[str] | None = None) -> tuple[int, list[float], list[bool]]: ...
def isContainTemplateGPU(template_path: str, threshold: float = 0.7, use_gray: bool = True, show_value: bool = False, show_position: bool = True, show_only_true_rect: bool = True, ms: float = 2000, crop_fmt: CropFmt = "", crop: list[int] | None = None, mask_path: str | None = None, BGR_range: BgrRange | None = None, threshold_binary: int | None = None, crop_template: list[int] | None = None, show_image: bool = False, color: list[str] | None = None) -> bool: ...
def isContainedImage(image_path: str, threshold: float = 0.7, use_gray: bool = True, show_value: bool = False, show_position: bool = True, show_only_true_rect: bool = True, ms: float = 2000, crop_fmt: CropFmt = "", crop: list[int] | None = None, mask_path: str | None = None, use_gpu: bool = False, BGR_range: BgrRange | None = None, threshold_binary: int | None = None, crop_template: list[int] | None = None, show_image: bool = False, color: list[str] | None = None) -> bool: ...
def saveCapture(filename: str | None = None, crop_fmt: CropFmt = "", crop: list[int] | None = None, mode: bool = True, format: ScreenshotFormat | None = None) -> None: ...  # noqa: A002
def popupImage(crop_fmt: CropFmt = "", crop: list[int] | None = None, title: str = "image") -> None: ...
def getCameraImage(crop_fmt: CropFmt = "", crop: list[int] | None = None) -> MatLike: ...
def openImage(filename: str, mode: str = "t") -> MatLike | None: ...
def setTemplateDir(path: str) -> None: ...
def get_filespec(filename: str, mode: str = "t") -> str: ...
def displayRectangle(max_loc: list[int] | Sequence[int], width: int, height: int, tag: str | None = None, ms: float = 2000, color: list[str] | None = None, crop_fmt: CropFmt = "", crop: list[int] | None = None) -> None: ...
def displayText(position: Sequence[int], txt: str, tag: str | None = None, ms: float = 2000, font: str = "UD デジタル 教科書体 NP-B", fontsize: int = 20, color: str = "black") -> None: ...
"#;

const MCU_COMMAND_BASE: &str = r"# Generated by `nix run .#generate-contracts`; do not edit.
from collections.abc import Callable

from .Sender import Sender

class McuCommand:
    def __init__(self, sync_name: str) -> None: ...
    def start(self, ser: Sender, postProcess: Callable[[], None] | None) -> None: ...
    def end(self, ser: Sender) -> None: ...
";

const BRIDGE_INIT: &str = r"# Generated by `nix run .#generate-contracts`; do not edit.
from .bridge_functions import BridgeFunctions as BridgeFunctions
";

const BRIDGE_FUNCTIONS: &str = r#"# Generated by `nix run .#generate-contracts`; do not edit.
from Commands.PythonCommandBase import ImageProcPythonCommand

class BridgeFunctions:
    def __init__(self, commands: ImageProcPythonCommand) -> None: ...
    def check_pokecon_extension(self) -> bool: ...
    def get_profile_name(self) -> str: ...
    def set_template_directory(self, name1: str, name2: str) -> None: ...
    def get_template_directory(self) -> str: ...
    def bf_print(self, *objects: object, sep: str = " ", end: str = "\n") -> None: ...
    def bf_print_w(self, *objects: object, sep: str = " ", end: str = "\n") -> None: ...
    def bf_print_a(self, *objects: object, sep: str = " ", end: str = "\n") -> None: ...
    def bf_isContainTemplate(self, template_path: str, threshold: float = 0.7, use_gray: bool = True, show_value: bool = False, show_position: bool = True, show_only_true_rect: bool = True, ms: float = 2000, crop_fmt: str | int = "", crop: list[int] | None = None, mask_path: str | None = None, use_gpu: bool = False, BGR_range: dict[str, int | tuple[int, int, int]] | None = None, threshold_binary: int | None = None, crop_template: list[int] | None = None, show_image: bool = False, color: list[str] | None = None) -> bool: ...
    def bf_isContainTemplate_max(self, template_path_list: list[str], threshold: float = 0.7, use_gray: bool = True, show_value: bool = False, show_position: bool = True, show_only_true_rect: bool = True, ms: float = 2000, crop_fmt: str | int = "", crop: list[int] | None = None, mask_path_list: list[str | None] | None = None, BGR_range: dict[str, int | tuple[int, int, int]] | None = None, threshold_binary: int | None = None, crop_template: list[int] | None = None, show_image: bool = False, color: list[str] | None = None) -> tuple[int, list[float], list[bool]]: ...
    def bf_dialogue(self, title: str, message: list[object] | str, desc: str | None = None, need: type[list[object]] | type[dict[object, object]] = list) -> list[str] | dict[int | str, str]: ...
    def bf_dialogue6widget(self, title: str, dialogue_list: list[list[object]], desc: str | None = None, need: type[list[object]] | type[dict[object, object]] = list) -> list[str] | dict[int | str, str]: ...
    def bf_dialogue6widget_save_settings(self, title: str, dialogue_list: list[list[object]], filename: str, desc: str | None = None, need: type[list[object]] | type[dict[object, object]] = list) -> list[str] | dict[int | str, str]: ...
    def bf_dialogue6widget_select_settings(self, title: str, dialogue_list: list[list[object]], dirname: str, desc: str | None = None, need: type[list[object]] | type[dict[object, object]] = list) -> list[str] | dict[int | str, str]: ...
    def bf_show_informations(self, name: str, developer: str | list[str], contributor: str | list[str] | None = None, description: str | None = None) -> None: ...
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_typings_cover_the_canonical_surface() {
        let files = commands_python_typings().unwrap();
        assert_eq!(files.len(), COMMANDS_TYPING_FILES.len());
        let combined = files
            .iter()
            .map(PythonTypingFile::source)
            .collect::<String>();
        for member in [
            "class PythonCommand:",
            "class ImageProcPythonCommand(PythonCommand):",
            "class Camera:",
            "class CaptureArea:",
            "class Widget[T]:",
            "class McuCommand:",
            "class BridgeFunctions:",
        ] {
            assert!(
                combined.contains(member),
                "missing generated member {member}"
            );
        }
        assert!(!combined.contains("Any"));
        assert!(!combined.contains("Callable[..., object]"));
    }

    #[test]
    fn generated_typing_paths_are_unique_and_safe() {
        let files = commands_python_typings().unwrap();
        let paths = files
            .iter()
            .map(PythonTypingFile::relative_path)
            .collect::<BTreeSet<_>>();
        assert_eq!(paths.len(), files.len());
        assert!(paths.iter().all(|path| {
            !path.starts_with('/')
                && !path.split('/').any(|component| component == "..")
                && std::path::Path::new(path)
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("pyi"))
        }));
    }
}
