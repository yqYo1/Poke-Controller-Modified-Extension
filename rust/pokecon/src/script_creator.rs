//! Script template creation (upstream `services/script_creator.py`).
//!
//! Builds a ready-to-run Python command skeleton from the entered fields
//! (script name, file name, folder, `BridgeFunctions` usage, image folder),
//! creates the missing folders, and writes README.md / License.txt templates
//! next to the script without overwriting existing files. All user-facing
//! messages and generated file bodies follow the upstream text (Japanese),
//! since they are shown to script authors and shipped inside generated
//! scripts.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Folder for scripts and template images, relative to the data root
/// (upstream `COMMANDS_PATH` / `TEMPLATE_PATH`).
pub const COMMANDS_PATH: [&str; 2] = ["Commands", "PythonCommands"];
pub const TEMPLATE_PATH: &str = "Template";
/// `BridgeFunctions` file (relative to the commands folder) and its import line.
pub const BRIDGE_FILE: [&str; 2] = ["bridge_functions", "bridge_functions.py"];
pub const BRIDGE_IMPORT: &str =
    "from Commands.PythonCommands.bridge_functions.bridge_functions import BridgeFunctions";
/// Image folder kinds (upstream `IN_TEMPLATE` / `IN_SCRIPT_FOLDER`).
pub const IN_TEMPLATE: &str = "template";
pub const IN_SCRIPT_FOLDER: &str = "script";
/// Files created next to the script.
pub const README_NAME: &str = "README.md";
pub const LICENSE_NAME: &str = "License.txt";
/// Placeholder written when no author is entered.
pub const AUTHOR_PLACEHOLDER: &str = "(名前)";
/// Bridge credit written into READMEs of bridge-using scripts.
pub const BRIDGE_CREDIT: &str =
    "Poke-Controller Modified/Extension 橋渡し関数(作成者：フウ(@dragonite303))";
/// Author memory file (relative to the config root, profile-independent).
pub const SETTING_PATH: [&str; 2] = ["profiles", "script_creator.json"];
pub const NAME_MAX_LENGTH: usize = 100;
pub const FILE_NAME_MAX_LENGTH: usize = 64;
/// Folders never offered as script folders.
pub const HIDDEN_FOLDERS: [&str; 2] = ["__pycache__", "bridge_functions"];
/// Windows-reserved file and folder names.
const RESERVED_NAMES: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];
/// Characters forbidden in folder names (Windows; `.` also breaks module paths).
const FOLDER_FORBIDDEN: [char; 10] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|', '.'];

/// Widget example markers written into every generated script.
const WIDGET_BEGIN: &str =
    "# ---- ウィジェット(ダイアログ)を使う場合は、ここから下の「# 」を外します ----";
const WIDGET_END: &str = "# ---- ウィジェットの例は、ここまで ----";

/// Related-program list for License.txt (upstream `_LICENSE_REFERENCES`).
const LICENSE_REFERENCES: [(&str, &str); 20] = [
    ("Python", "https://docs.python.org/ja/3/license.html"),
    (
        "Poke-Controller",
        "https://github.com/KawaSwitch/Poke-Controller",
    ),
    ("lufa", "https://github.com/abcminiuser/lufa"),
    (
        "Poke-Controller-Modified",
        "https://github.com/Moi-poke/Poke-Controller-Modified",
    ),
    (
        "Poke-Controller-Modified Extension",
        "https://github.com/futo030/Poke-Controller-Modified-Extension",
    ),
    ("Pillow", "https://github.com/python-pillow/Pillow"),
    ("numpy", "https://numpy.org/doc/stable/license.html"),
    (
        "opencv-python",
        "https://github.com/opencv/opencv/tree/master/",
    ),
    (
        "pandas",
        "https://pandas.pydata.org/pandas-docs/stable/getting_started/overview.html",
    ),
    ("plyer", "https://github.com/kivy/plyer"),
    ("pygubu", "https://github.com/alejandroautalan/pygubu"),
    ("pynput", "https://github.com/pravinnath/pynput"),
    (
        "pyocr",
        "https://gitlab.gnome.org/World/OpenPaperwork/pyocr/-/tree/master/",
    ),
    ("pyperclip", "https://github.com/asweigart/pyperclip"),
    ("pyserial", "https://github.com/pyserial/pyserial"),
    ("pythonnet", "https://github.com/pythonnet/pythonnet"),
    ("requests", "https://github.com/psf/requests"),
    ("scipy", "https://github.com/scipy/scipy"),
    ("pygame", "https://github.com/pygame/pygame"),
    ("paho-mqtt", "https://github.com/eclipse/paho.mqtt.python"),
];

/// Failure to create a script; the message is user-facing (upstream text).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScriptCreateError(pub String);

impl std::fmt::Display for ScriptCreateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ScriptCreateError {}

/// Entered script specification (upstream `ScriptSpec`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScriptSpec {
    pub name: String,
    pub file_name: String,
    pub folder: String,
    pub use_bridge: bool,
    pub template_folder: String,
    pub template_location: String,
    pub author: String,
}

impl ScriptSpec {
    /// Author written into generated files (placeholder when empty).
    #[must_use]
    pub fn author_name(&self) -> String {
        let trimmed = self.author.trim();
        if trimmed.is_empty() {
            AUTHOR_PLACEHOLDER.to_owned()
        } else {
            trimmed.to_owned()
        }
    }

    /// Trimmed copy with folders normalized to `/` separators.
    #[must_use]
    pub fn normalized(&self) -> Self {
        Self {
            name: self.name.trim().to_owned(),
            file_name: self.file_name.trim().to_owned(),
            folder: normalize_folder(&self.folder),
            use_bridge: self.use_bridge,
            template_folder: normalize_folder(&self.template_folder),
            template_location: if self.template_location.is_empty() {
                IN_TEMPLATE.to_owned()
            } else {
                self.template_location.clone()
            },
            author: self.author.trim().to_owned(),
        }
    }
}

/// Result of a successful creation (upstream `ScriptResult`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScriptResult {
    pub path: PathBuf,
    pub command_name: String,
    pub class_name: String,
    pub created_folders: Vec<PathBuf>,
    pub created_files: Vec<PathBuf>,
    pub skipped_files: Vec<PathBuf>,
}

/// Normalizes a folder entry to `/`-separated form without empty parts.
#[must_use]
pub fn normalize_folder(text: &str) -> String {
    text.replace('\\', "/")
        .split('/')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

fn has_control_characters(text: &str) -> bool {
    text.chars().any(|c| c < '\u{20}' || c == '\u{7f}')
}

/// Validates the script name; returns the user-facing reason or empty.
#[must_use]
pub fn check_name(name: &str) -> String {
    if name.is_empty() {
        return "スクリプト名を入力してください。".to_owned();
    }
    if has_control_characters(name) {
        return "スクリプト名に、改行やタブは使えません。".to_owned();
    }
    if name.chars().count() > NAME_MAX_LENGTH {
        return format!("スクリプト名は {NAME_MAX_LENGTH} 文字以内にしてください。");
    }
    String::new()
}

/// Validates the author (may be empty); returns the reason or empty.
#[must_use]
pub fn check_author(author: &str) -> String {
    if has_control_characters(author) {
        return "作成者名に、改行やタブは使えません。".to_owned();
    }
    if author.chars().count() > NAME_MAX_LENGTH {
        return format!("作成者名は {NAME_MAX_LENGTH} 文字以内にしてください。");
    }
    String::new()
}

fn valid_file_name(file_name: &str) -> bool {
    !file_name.is_empty()
        && file_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Validates the file name (without extension); returns the reason or empty.
#[must_use]
pub fn check_file_name(file_name: &str) -> String {
    if file_name.is_empty() {
        return "ファイル名を入力してください。".to_owned();
    }
    if file_name.to_lowercase().ends_with(".py") {
        return "ファイル名に「.py」は付けないでください(自動で付きます)。".to_owned();
    }
    if !valid_file_name(file_name) {
        return "ファイル名に使えるのは、半角の英数字と「-」「_」だけです。".to_owned();
    }
    if file_name.chars().count() > FILE_NAME_MAX_LENGTH {
        return format!("ファイル名は {FILE_NAME_MAX_LENGTH} 文字以内にしてください。");
    }
    if RESERVED_NAMES.contains(&file_name.to_lowercase().as_str()) {
        return format!(
            "「{file_name}」は、Windows でファイル名に使えません。別の名前にしてください。"
        );
    }
    if file_name == "__init__" {
        return "「__init__」は、ファイル名に使えません。".to_owned();
    }
    String::new()
}

/// Validates a folder entry (`A/B` form; empty means none).
#[must_use]
pub fn check_folder(folder: &str, label: &str) -> String {
    for part in folder.split('/').filter(|part| !part.is_empty()) {
        if part
            .chars()
            .any(|c| FOLDER_FORBIDDEN.contains(&c) || c < '\u{20}')
        {
            return format!("{label}のフォルダー名に、次の文字は使えません: \\ / : * ? \" < > | .");
        }
        if RESERVED_NAMES.contains(&part.to_lowercase().as_str()) {
            return format!(
                "「{part}」は、Windows でフォルダー名に使えません。別の名前にしてください。"
            );
        }
    }
    String::new()
}

/// Absolute path of the script to create.
#[must_use]
pub fn script_path(spec: &ScriptSpec, data_root: &Path) -> PathBuf {
    let mut path = data_root.join(COMMANDS_PATH[0]).join(COMMANDS_PATH[1]);
    for part in spec.folder.split('/').filter(|part| !part.is_empty()) {
        path = path.join(part);
    }
    path.join(format!("{}.py", spec.file_name))
}

/// Absolute image folder (empty when no template folder was entered).
#[must_use]
pub fn template_dir(spec: &ScriptSpec, data_root: &Path) -> PathBuf {
    if spec.template_folder.is_empty() {
        return PathBuf::new();
    }
    let mut path = if spec.template_location == IN_SCRIPT_FOLDER {
        script_path(spec, data_root)
            .parent()
            .map_or_else(|| data_root.to_path_buf(), Path::to_path_buf)
    } else {
        data_root.join(TEMPLATE_PATH)
    };
    for part in spec
        .template_folder
        .split('/')
        .filter(|part| !part.is_empty())
    {
        path = path.join(part);
    }
    path
}

fn relative_to(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Whether the `BridgeFunctions` file exists.
#[must_use]
pub fn bridge_available(data_root: &Path) -> bool {
    data_root
        .join(COMMANDS_PATH[0])
        .join(COMMANDS_PATH[1])
        .join(BRIDGE_FILE[0])
        .join(BRIDGE_FILE[1])
        .is_file()
}

/// Display name in the command list: `name (folder/file)`.
#[must_use]
pub fn command_name(spec: &ScriptSpec) -> String {
    let location = [spec.folder.as_str(), spec.file_name.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    format!("{} ({location})", spec.name)
}

/// Class name derived from the file name (upstream `class_name_for`).
#[must_use]
pub fn class_name_for(file_name: &str) -> String {
    let parts = file_name
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let mut name = parts
        .iter()
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<String>();
    let starts_with_ascii_letter = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
    if !starts_with_ascii_letter {
        name = format!("Command{name}");
    }
    name
}

/// Validates the whole normalized spec; returns the reason or empty.
#[must_use]
pub fn check_spec(spec: &ScriptSpec, data_root: &Path) -> String {
    let error = [
        check_name(&spec.name),
        check_author(&spec.author),
        check_file_name(&spec.file_name),
        check_folder(&spec.folder, "格納場所"),
        check_folder(&spec.template_folder, "画像保存先"),
    ]
    .into_iter()
    .find(|reason| !reason.is_empty());
    if let Some(reason) = error {
        return reason;
    }
    if spec.template_location != IN_TEMPLATE && spec.template_location != IN_SCRIPT_FOLDER {
        return "画像保存先の種類が正しくありません。".to_owned();
    }
    if spec.template_location == IN_SCRIPT_FOLDER
        && !spec.template_folder.is_empty()
        && !spec.use_bridge
    {
        return "画像保存先をスクリプト格納場所の中にできるのは、BridgeFunctions を使用する場合だけです。"
            .to_owned();
    }
    let first_part = spec.folder.split('/').next().unwrap_or_default();
    if HIDDEN_FOLDERS.contains(&first_part) {
        return format!("「{first_part}」は、スクリプトの格納場所に使えません。");
    }
    let path = script_path(spec, data_root);
    if path.exists() {
        return format!(
            "同じ名前のファイルが、すでにあります: {}",
            relative_to(&path, data_root)
        );
    }
    for folder in [path.parent(), Some(template_dir(spec, data_root).as_path())]
        .into_iter()
        .flatten()
    {
        let mut current = folder.to_path_buf();
        while current.starts_with(data_root) && current != data_root {
            if current.is_file() {
                return format!(
                    "フォルダーにしたい場所に、同じ名前のファイルがあります: {}",
                    relative_to(&current, data_root)
                );
            }
            if !current.pop() {
                break;
            }
        }
    }
    if spec.use_bridge && !bridge_available(data_root) {
        return "BridgeFunctions(Commands/PythonCommands/bridge_functions/bridge_functions.py)がありません。「使用しない」にするか、bridge_functions フォルダーを置いてください。".to_owned();
    }
    String::new()
}

/// Preview lines shown while typing (empty when file name/folder invalid).
#[must_use]
pub fn preview_lines(spec: &ScriptSpec, data_root: &Path) -> Vec<String> {
    if spec.file_name.is_empty()
        || !check_file_name(&spec.file_name).is_empty()
        || !check_folder(&spec.folder, "").is_empty()
    {
        return Vec::new();
    }
    let path = script_path(spec, data_root);
    let mut lines = vec![format!("作るファイル: {}", relative_to(&path, data_root))];
    let template = template_dir(spec, data_root);
    if !template.as_os_str().is_empty() && check_folder(&spec.template_folder, "").is_empty() {
        let state = if template.is_dir() {
            ""
        } else {
            "(新しく作ります)"
        };
        lines.push(format!(
            "画像のフォルダー: {}{state}",
            relative_to(&template, data_root)
        ));
    }
    let folder = path.parent().map_or_else(PathBuf::new, Path::to_path_buf);
    let extras = [README_NAME, LICENSE_NAME]
        .into_iter()
        .filter(|name| !folder.join(name).exists())
        .collect::<Vec<_>>();
    if extras.is_empty() {
        lines.push("README.md と License.txt は、すでにあるので、作りません".to_owned());
    } else {
        lines.push(format!(
            "一緒に作るファイル: {}(スクリプトと同じフォルダー)",
            extras.join("、")
        ));
    }
    lines
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| format!("\"{value}\""))
}

/// Widget example lines (before commenting); bridge form mirrors
/// `bridge_functions/Sample/bf_sample.py`.
fn widget_example(spec: &ScriptSpec) -> Vec<String> {
    if !spec.use_bridge {
        return [
            "# ダイアログ呼び出し",
            "ret = self.dialogue6widget(",
            "    \"config\",",
            "    [[\"Entry\", \"テスト1\", 1]],",
            "    desc=\"テスト用ウィジェット\",",
            ")",
            "",
            "# 表示する",
            "print(f\"ウィジェット1結果:{ret[0]}\")",
            "",
            "# ダイアログ呼び出し(選択肢から選ぶ)",
            "ret = self.dialogue6widget(",
            "    \"config\",",
            "    [[\"Combo\", \"テスト2\", [\"A\", \"B\", \"C\"], \"A\"]],",
            "    desc=\"テスト用ウィジェット2\",",
            ")",
            "",
            "# 表示する",
            "print(f\"ウィジェット2結果:{ret[0]}\")",
        ]
        .map(str::to_owned)
        .to_vec();
    }
    let save = "self.bf.bf_dialogue6widget_save_settings";
    let select = "self.bf.bf_dialogue6widget_select_settings";
    let show = "self.bf.bf_print({})";
    let show_other = "self.bf.bf_print_w({})";
    vec![
        "# 履歴保存用ファイル".to_owned(),
        format!(
            "settings_file = os.path.join(os.path.dirname(__file__), \"{}_setting.json\")",
            spec.file_name
        ),
        String::new(),
        "# ダイアログ呼び出し(前回の値を初期値とする)".to_owned(),
        format!("ret = {save}("),
        "    \"config\",".to_owned(),
        "    [[\"Entry\", \"テスト1\", 1]],".to_owned(),
        "    settings_file,".to_owned(),
        "    desc=\"テスト用ウィジェット\",".to_owned(),
        ")".to_owned(),
        String::new(),
        "# 表示する".to_owned(),
        show.replace("{}", &json_string("ウィジェット1結果:{ret[0]}")),
        String::new(),
        "# 履歴保存用ディレクトリ".to_owned(),
        format!(
            "settings_dir = os.path.join(os.path.dirname(__file__), \"{}_settings\")",
            spec.file_name
        ),
        String::new(),
        "# ダイアログ呼び出し(保存した値を選択して呼び出す)".to_owned(),
        format!("ret = {select}("),
        "    \"config\",".to_owned(),
        "    [[\"Combo\", \"テスト2\", [\"A\", \"B\", \"C\"], \"(n/a)\"]],".to_owned(),
        "    settings_dir,".to_owned(),
        "    desc=\"テスト用ウィジェット2\",".to_owned(),
        ")".to_owned(),
        String::new(),
        "# 表示する(標準出力に割り当てられていない方の出力画面に、上書きで表示)".to_owned(),
        show_other.replace("{}", &json_string("ウィジェット2結果:{ret[0]}")),
    ]
}

/// Body of the generated script (upstream `build_source`).
#[allow(
    clippy::too_many_lines,
    reason = "mirrors the upstream template builder"
)]
#[must_use]
pub fn build_source(spec: &ScriptSpec) -> String {
    let name = json_string(&spec.name);
    let use_image = !spec.template_folder.is_empty() || spec.use_bridge;
    let base_class = if use_image {
        "ImageProcPythonCommand"
    } else {
        "PythonCommand"
    };
    let print_call = if spec.use_bridge {
        "self.bf.bf_print"
    } else {
        "print"
    };
    let mut lines = vec![
        "#!/usr/bin/env python3".to_owned(),
        "# -*- coding: utf-8 -*-".to_owned(),
        String::new(),
        "# ボタン・スティック・十字キーの名前(使わないものは、消して構いません)".to_owned(),
        "from Commands.Keys import Button, Direction, Hat  # noqa: F401".to_owned(),
        format!("from Commands.PythonCommandBase import {base_class}"),
    ];
    if spec.use_bridge {
        lines.push(BRIDGE_IMPORT.to_owned());
        let uses_os =
            !spec.template_folder.is_empty() && spec.template_location == IN_SCRIPT_FOLDER;
        lines.push(
            if uses_os {
                "import os"
            } else {
                "import os  # noqa: F401 (ウィジェットの設定の保存先に使います)"
            }
            .to_owned(),
        );
    }
    lines.push(String::new());
    if !spec.template_folder.is_empty() {
        let place = if spec.template_location == IN_SCRIPT_FOLDER {
            "このファイルと同じフォルダーの中"
        } else {
            "SerialController/Template の中"
        };
        lines.push(format!("# テンプレート画像を置くフォルダー({place})"));
        lines.push(format!(
            "TEMPLATE_DIR = {}",
            json_string(&spec.template_folder)
        ));
        lines.push(String::new());
    }
    lines.push(String::new());
    lines.push(format!(
        "class {}({base_class}):",
        class_name_for(&spec.file_name)
    ));
    lines.push(format!("    NAME = {name}"));
    lines.push("    TAGS = []".to_owned());
    lines.push(String::new());
    if use_image {
        lines.push("    def __init__(self, cam):".to_owned());
        lines.push("        super().__init__(cam)".to_owned());
    } else {
        lines.push("    def __init__(self):".to_owned());
        lines.push("        super().__init__()".to_owned());
    }
    lines.push(String::new());
    lines.push("    def do(self):".to_owned());
    let mut body: Vec<String> = Vec::new();
    if spec.use_bridge {
        body.push("# クロスプラットフォーム用クラス".to_owned());
        body.push("self.bf = BridgeFunctions(self)".to_owned());
        body.push(String::new());
        body.push("# プログラム情報の表示".to_owned());
        body.push(format!("developer = {}", json_string(&spec.author_name())));
        body.push("self.bf.bf_show_informations(self.NAME, developer)".to_owned());
        body.push(String::new());
        let images_in_script_folder =
            !spec.template_folder.is_empty() && spec.template_location == IN_SCRIPT_FOLDER;
        if images_in_script_folder {
            body.push(
                "# テンプレートファイルディレクトリの設定(このファイルと同じフォルダーの中の TEMPLATE_DIR を使う)"
                    .to_owned(),
            );
            body.push(
                "# ※Poke-Controller Modified では、場所は変わらず、Template フォルダーの中の画像が使われます"
                    .to_owned(),
            );
            body.push(
                "self.bf.set_template_directory(os.path.join(os.path.dirname(__file__)), TEMPLATE_DIR)"
                    .to_owned(),
            );
            body.push(String::new());
        } else if !spec.template_folder.is_empty() {
            body.push("# テンプレートファイルディレクトリの設定".to_owned());
            body.push("self.bf.set_template_directory(\"./Template\", TEMPLATE_DIR)".to_owned());
            body.push(String::new());
        }
    } else {
        body.push(format!(
            "print({})",
            json_string(&format!("{} を開始します", spec.name))
        ));
        body.push(String::new());
    }
    body.push(WIDGET_BEGIN.to_owned());
    body.extend(widget_example(spec).into_iter().map(|line| {
        if line.is_empty() {
            "#".to_owned()
        } else {
            format!("# {line}")
        }
    }));
    body.push(WIDGET_END.to_owned());
    body.push(String::new());
    body.push("# ここに、実行する処理を書きます".to_owned());
    if spec.template_folder.is_empty() {
        body.push("# 例: A を 0.1 秒押して、1 秒待つ".to_owned());
        body.push("# self.press(Button.A, duration=0.1, wait=1.0)".to_owned());
    } else {
        let check = if spec.use_bridge {
            "self.bf.bf_isContainTemplate"
        } else {
            "self.isContainTemplate"
        };
        body.push("# 例: 画像が見つかるまで、A を押す".to_owned());
        body.push(format!(
            "# while not {check}(f\"{{TEMPLATE_DIR}}/sample.png\"):"
        ));
        body.push("#     self.press(Button.A, wait=1.0)".to_owned());
    }
    body.push("self.wait(1.0)".to_owned());
    body.push(String::new());
    body.push(format!(
        "{print_call}({})",
        json_string(&format!("{} を終了します", spec.name))
    ));
    for line in &body {
        if line.is_empty() {
            lines.push(String::new());
        } else {
            lines.push(format!("        {line}"));
        }
    }
    lines.push(String::new());
    lines.join("\n")
}

fn windows_path(spec: &ScriptSpec, names: &[&str]) -> String {
    let mut parts = vec![
        "SerialController".to_owned(),
        COMMANDS_PATH[0].to_owned(),
        COMMANDS_PATH[1].to_owned(),
    ];
    parts.extend(
        spec.folder
            .split('/')
            .filter(|part| !part.is_empty())
            .map(str::to_owned),
    );
    parts.extend(names.iter().map(|name| (*name).to_owned()));
    parts.join("\\")
}

/// README.md template (upstream `build_readme`; same sections as
/// `bridge_functions/README.md`).
#[allow(
    clippy::too_many_lines,
    reason = "mirrors the upstream template builder"
)]
#[must_use]
pub fn build_readme(spec: &ScriptSpec, app_version: &str) -> String {
    let author = spec.author_name();
    let app = if app_version.is_empty() {
        "Poke-Controller Modified Extension".to_owned()
    } else {
        format!("Poke-Controller Modified Extension ver.{app_version}")
    };
    let mut lines = vec![
        "fileencoding=utf-8".to_owned(),
        String::new(),
        format!("{} ver.0.0.1 作成者：{author}", spec.name),
        String::new(),
        "■要約".to_owned(),
        "\u{3000}(本プログラムの説明を書いてください。)".to_owned(),
        String::new(),
        "■開発環境".to_owned(),
        format!("\u{3000}python v{}", python_version()),
        format!("\u{3000}{app}"),
        String::new(),
        "■事前準備".to_owned(),
    ];
    if spec.folder.is_empty() {
        lines.push(format!(
            "* {}となるようにおいてください。",
            windows_path(spec, &[&format!("{}.py", spec.file_name)])
        ));
    } else {
        lines.push(format!(
            "* {}\\{}のフォルダにフォルダごとおいてください。",
            ["SerialController", COMMANDS_PATH[0], COMMANDS_PATH[1]].join("\\"),
            spec.folder.replace('/', "\\")
        ));
        lines.push(format!(
            "\u{3000}{}となる想定です。",
            windows_path(spec, &[&format!("{}.py", spec.file_name)])
        ));
    }
    if spec.use_bridge {
        lines.push("* 橋渡し関数(bridge_functions)が必要です。SerialController\\Commands\\PythonCommands\\bridge_functions\\bridge_functions.pyとなるようにおいてください。".to_owned());
    }
    let images_in_script_folder =
        !spec.template_folder.is_empty() && spec.template_location == IN_SCRIPT_FOLDER;
    if images_in_script_folder {
        lines.push(format!(
            "* テンプレート画像は、{}においてください。",
            windows_path(
                spec,
                &spec
                    .template_folder
                    .split('/')
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
            )
        ));
        lines.push(format!(
            "\u{3000}※Poke-Controller Modifiedでは、SerialController\\Template\\{}においてください(テンプレートファイルの場所を変更できないため)。",
            spec.template_folder.replace('/', "\\")
        ));
    } else if !spec.template_folder.is_empty() {
        lines.push(format!(
            "* テンプレート画像は、SerialController\\Template\\{}においてください。",
            spec.template_folder.replace('/', "\\")
        ));
    }
    lines.extend(
        [
            String::new(),
            "■使用方法".to_owned(),
            format!(
                "* Commandsタブの一覧から「{}」を選び、Startを押します。",
                command_name(spec)
            ),
            "* (使い方の手順を書いてください。)".to_owned(),
            String::new(),
            "■対象のバージョン".to_owned(),
            format!("* {app}"),
        ]
        .map(String::from),
    );
    if spec.use_bridge {
        lines.push("* Poke-Controller Modified(橋渡し関数が対応しているバージョン)".to_owned());
    }
    lines.extend(
        [
            String::new(),
            "■利用上の注意".to_owned(),
            "* 勝手に改変して使用いただいて構いません。".to_owned(),
            "* License.txtに記載の通り改変しての配布は禁止とさせていただきます。".to_owned(),
            "* 想定外の動作をした場合でも責任はとれません。自己責任で利用ください。".to_owned(),
            "* 商用利用/自作発言はお控えください。".to_owned(),
            String::new(),
            "■その他".to_owned(),
            "* 質問などありましたら作成者までお問い合わせください。なお、返信には数日ほどかかる場合もありますのでご承知おきください。常識やマナーを守って問い合わせいただければ幸いです。".to_owned(),
            "* 改変のない再配布は自由に可能です。".to_owned(),
            "* 本プログラムの挙動に関して、他の方に問い合わせをするのはご迷惑をおかけする可能性があるため基本的におやめください。".to_owned(),
            "* 修正および機能追加を希望される場合は作成者までご連絡ください。".to_owned(),
        ]
        .map(String::from),
    );
    if spec.use_bridge {
        lines.push(String::new());
        lines.push("■使用しているプログラム".to_owned());
        lines.push(format!(
            "* 本プログラムは、{BRIDGE_CREDIT}を使用しています。"
        ));
    }
    lines.push(String::new());
    lines.join("\n")
}

fn python_version() -> String {
    std::process::Command::new("python3")
        .arg("-c")
        .arg("import platform; print(platform.python_version())")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|version| version.trim().to_owned())
        .filter(|version| !version.is_empty())
        .unwrap_or_else(|| "3".to_owned())
}

/// License.txt template (upstream `build_license`; forbids commercial use,
/// not MIT).
#[allow(
    clippy::too_many_lines,
    reason = "mirrors the upstream template builder"
)]
#[must_use]
pub fn build_license(spec: &ScriptSpec, year: i32) -> String {
    let mut lines = vec![
        spec.name.clone(),
        format!("Copyright (C) {year}年 {}", spec.author_name()),
        String::new(),
        "1.本プログラムは、個人のみ使用可能であり、プログラム利用者は以下のことを実行可能です。".to_owned(),
        "\u{3000}・プログラムの実行、複製、改変、改変しない状態での二次配布".to_owned(),
        String::new(),
        "2.本プログラム利用者は、以下のことを遵守しなければなりません。".to_owned(),
        "\u{3000}・商用利用(※)をしない。".to_owned(),
        "\u{3000}\u{3000}※商用利用とは、本プログラムを用いて金銭的な対価を得ることを指します。".to_owned(),
        "\u{3000}\u{3000}(例：本プログラムの販売、本プログラムを用いて得られた成果物の販売、など。なお、配信サイトにおける配信での利用は対象外です。）".to_owned(),
        String::new(),
        "3.本プログラム利用者は、以下の行為を行う場合、プログラムのどこか(例:readme)に引用の旨を記載する必要があります。".to_owned(),
        "\u{3000}・本プログラムを同梱したプログラムの配布".to_owned(),
        String::new(),
        "4.本プログラムの少なくとも一部を抽出または改変して公開したい場合、プログラム作成者に連絡の上、プログラム作成者より配布するものとします。".to_owned(),
        String::new(),
        "5.本プログラムではプログラミング言語(python)およびpythonのライブラリを使用します。各プログラムに関する権利はそれぞれの権利者が有します。".to_owned(),
        "\u{3000}権利者から本プログラムの利用中止を求められた場合は直ちに本プログラムの利用を中止するものとします。".to_owned(),
        String::new(),
    ];
    let mut references: Vec<(String, &str)> = LICENSE_REFERENCES
        .iter()
        .map(|(name, url)| ((*name).to_owned(), *url))
        .collect();
    if spec.use_bridge {
        references.insert(
            5,
            (
                "Poke-Controller Modified/Extension 橋渡し関数".to_owned(),
                "(同梱の bridge_functions フォルダーの License.txt)",
            ),
        );
    }
    let width = references
        .iter()
        .map(|(name, _)| name.chars().count())
        .max()
        .unwrap_or(0)
        + 3;
    for (name, url) in references {
        lines.push(format!(
            "{name}{}{url}",
            " ".repeat(width.saturating_sub(name.chars().count()))
        ));
    }
    lines.push(String::new());
    lines.push("6.本プログラム利用者は、自らの責任において本プログラムを利用することに同意し、本プログラムの利用によって生じる一切の損害、損失について、著作権者は損害賠償義務及びその他一切の責任を負いません。".to_owned());
    lines.push(String::new());
    lines.join("\n")
}

/// Creates the script (and missing folders), then the README / License
/// templates when they do not exist yet. Errors carry user-facing text.
pub fn create_script(
    spec: &ScriptSpec,
    data_root: &Path,
    app_version: &str,
    year: i32,
) -> Result<ScriptResult, ScriptCreateError> {
    let spec = spec.normalized();
    let error = check_spec(&spec, data_root);
    if !error.is_empty() {
        return Err(ScriptCreateError(error));
    }
    let path = script_path(&spec, data_root);
    let mut result = ScriptResult {
        path: path.clone(),
        command_name: command_name(&spec),
        class_name: class_name_for(&spec.file_name),
        created_folders: Vec::new(),
        created_files: Vec::new(),
        skipped_files: Vec::new(),
    };
    let folders = [path.parent().map(Path::to_path_buf), {
        let template = template_dir(&spec, data_root);
        if template.as_os_str().is_empty() {
            None
        } else {
            Some(template)
        }
    }];
    for folder in folders.into_iter().flatten() {
        if !folder.is_dir() {
            let mut missing = Vec::new();
            let mut current = folder.clone();
            while !current.is_dir() {
                missing.push(current.clone());
                if !current.pop() {
                    break;
                }
            }
            std::fs::create_dir_all(&folder).map_err(|error| {
                ScriptCreateError(format!("ファイルを作れませんでした({})。", error.kind()))
            })?;
            missing.reverse();
            result.created_folders.extend(missing);
        }
    }
    if path.exists() {
        return Err(ScriptCreateError(format!(
            "同じ名前のファイルが、すでにあります: {}",
            relative_to(&path, data_root)
        )));
    }
    std::fs::write(&path, build_source(&spec)).map_err(|error| {
        ScriptCreateError(format!("ファイルを作れませんでした({})。", error.kind()))
    })?;
    let folder = path.parent().map_or_else(PathBuf::new, Path::to_path_buf);
    for (name, content) in [
        (README_NAME, build_readme(&spec, app_version)),
        (LICENSE_NAME, build_license(&spec, year)),
    ] {
        let extra = folder.join(name);
        if extra.exists() {
            result.skipped_files.push(extra);
        } else if std::fs::write(&extra, content).is_ok() {
            result.created_files.push(extra);
        } else {
            result.skipped_files.push(extra);
        }
    }
    Ok(result)
}

/// Folder names under `root` in `A/B` form, sorted, skipping dot/underscore
/// folders and `hidden` (upstream `list_folders`).
#[must_use]
pub fn list_folders(root: &Path, hidden: &BTreeSet<String>) -> Vec<String> {
    fn walk(folder: &Path, prefix: &str, hidden: &BTreeSet<String>, found: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(folder) else {
            return;
        };
        let mut names = entries
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        names.sort_by_key(|name| name.to_lowercase());
        for name in names {
            let path = folder.join(&name);
            if name.starts_with(['.', '_']) || hidden.contains(&name) || !path.is_dir() {
                continue;
            }
            found.push(format!("{prefix}{name}"));
            walk(&path, &format!("{prefix}{name}/"), hidden, found);
        }
    }
    let mut found = Vec::new();
    walk(root, "", hidden, &mut found);
    found
}

/// Existing script folders under `Commands/PythonCommands`.
#[must_use]
pub fn script_folders(data_root: &Path) -> Vec<String> {
    let hidden = HIDDEN_FOLDERS
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    list_folders(
        &data_root.join(COMMANDS_PATH[0]).join(COMMANDS_PATH[1]),
        &hidden,
    )
}

/// Existing image folders under `Template`.
#[must_use]
pub fn template_folders(data_root: &Path) -> Vec<String> {
    list_folders(&data_root.join(TEMPLATE_PATH), &BTreeSet::new())
}

/// Reads the remembered author name (empty when unknown or invalid).
#[must_use]
pub fn load_author(config_root: &Path) -> String {
    let path = config_root.join(SETTING_PATH[0]).join(SETTING_PATH[1]);
    let Ok(content) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return String::new();
    };
    let Some(author) = value.get("author").and_then(|value| value.as_str()) else {
        return String::new();
    };
    if check_author(author).is_empty() {
        author.to_owned()
    } else {
        String::new()
    }
}

/// Remembers the author name; failures are ignored (upstream behavior).
pub fn save_author(config_root: &Path, author: &str) {
    let path = config_root.join(SETTING_PATH[0]).join(SETTING_PATH[1]);
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }
    let value = serde_json::json!({ "author": author.trim() });
    let _ = std::fs::write(
        path,
        serde_json::to_string_pretty(&value).unwrap_or_default(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> ScriptSpec {
        ScriptSpec {
            name: "テスト".to_owned(),
            file_name: "test_script".to_owned(),
            ..ScriptSpec::default()
        }
    }

    #[test]
    fn name_and_file_name_checks_follow_the_upstream() {
        assert_eq!(check_name(""), "スクリプト名を入力してください。");
        assert!(check_name("改\n行").contains("改行やタブ"));
        assert_eq!(
            check_file_name("a.py"),
            "ファイル名に「.py」は付けないでください(自動で付きます)。"
        );
        assert!(check_file_name("日本語").contains("半角の英数字"));
        assert!(check_file_name("con").contains("Windows"));
        assert!(check_file_name("__init__").contains("使えません"));
        assert_eq!(check_file_name("ok_name-1"), "");
    }

    #[test]
    fn folder_checks_reject_windows_forbidden_characters() {
        assert!(check_folder("a.b", "格納場所").contains("使えません"));
        assert!(check_folder("A/B", "").is_empty());
        assert!(check_folder("com1", "").contains("Windows"));
        assert_eq!(normalize_folder(" a\\b//c "), "a/b/c");
    }

    #[test]
    fn class_names_follow_the_upstream() {
        assert_eq!(class_name_for("test_script"), "TestScript");
        assert_eq!(class_name_for("a-b_c"), "ABC");
        assert_eq!(class_name_for("1st"), "Command1st");
    }

    #[test]
    fn plain_script_uses_python_command_and_widget_comments() {
        let source = build_source(&spec());
        assert!(source.contains("from Commands.PythonCommandBase import PythonCommand"));
        assert!(source.contains("class TestScript(PythonCommand):"));
        assert!(source.contains("    NAME = \"テスト\""));
        assert!(source.contains("def __init__(self):"));
        assert!(source.contains("print(\"テスト を開始します\")"));
        assert!(source.contains(WIDGET_BEGIN));
        assert!(source.contains("# # ダイアログ呼び出し"));
        assert!(source.contains(WIDGET_END));
        assert!(source.contains("print(\"テスト を終了します\")"));
    }

    #[test]
    fn image_script_uses_image_proc_command_and_template_dir() {
        let mut spec = spec();
        spec.template_folder = "images".to_owned();
        let source = build_source(&spec);
        assert!(source.contains("from Commands.PythonCommandBase import ImageProcPythonCommand"));
        assert!(source.contains("TEMPLATE_DIR = \"images\""));
        assert!(source.contains("def __init__(self, cam):"));
        assert!(source.contains("# while not self.isContainTemplate"));
    }

    #[test]
    fn bridge_script_uses_bridge_import_and_prints() {
        let mut spec = spec();
        spec.use_bridge = true;
        let source = build_source(&spec);
        assert!(source.contains(BRIDGE_IMPORT));
        assert!(source.contains("self.bf = BridgeFunctions(self)"));
        assert!(source.contains("self.bf.bf_show_informations(self.NAME, developer)"));
        assert!(source.contains("self.bf.bf_print(\"テスト を終了します\")"));
        assert!(source.contains("# # 履歴保存用ファイル"));
    }

    #[test]
    fn readme_and_license_follow_the_upstream_templates() {
        let spec = spec();
        let readme = build_readme(&spec, "0.2.0");
        assert!(readme.contains("テスト ver.0.0.1 作成者：(名前)"));
        assert!(readme.contains("Poke-Controller Modified Extension ver.0.2.0"));
        assert!(readme.contains("Commandsタブの一覧から「テスト (test_script)」を選び"));
        let license = build_license(&spec, 2026);
        assert!(license.contains("Copyright (C) 2026年 (名前)"));
        assert!(license.contains("商用利用(※)をしない。"));
        assert!(license.contains("Poke-Controller-Modified Extension"));
    }

    #[test]
    fn create_writes_script_and_extras_without_overwriting() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path();
        let spec = spec();
        let result = create_script(&spec, root, "", 2026).expect("create");
        assert!(
            result
                .path
                .ends_with("Commands/PythonCommands/test_script.py")
        );
        assert_eq!(result.command_name, "テスト (test_script)");
        assert!(result.created_files.len() == 2);
        assert!(
            std::fs::read_to_string(&result.path)
                .expect("read")
                .contains("class TestScript")
        );
        let second = create_script(&spec, root, "", 2026);
        assert!(second.is_err());
        assert!(second.expect_err("err").0.contains("すでにあります"));
    }

    #[test]
    fn create_rejects_folder_scripts_location_without_bridge() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut spec = spec();
        spec.template_folder = "images".to_owned();
        spec.template_location = IN_SCRIPT_FOLDER.to_owned();
        let error = create_script(&spec, directory.path(), "", 2026).expect_err("err");
        assert!(error.0.contains("BridgeFunctions"));
    }

    #[test]
    fn author_round_trips_through_the_setting_file() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path();
        assert_eq!(load_author(root), "");
        save_author(root, "  作成者  ");
        assert_eq!(load_author(root), "作成者");
    }

    #[test]
    fn folder_lists_skip_hidden_and_bridge_folders() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path();
        let commands = root.join("Commands").join("PythonCommands");
        std::fs::create_dir_all(commands.join("Samples/Deep")).expect("mkdir");
        std::fs::create_dir_all(commands.join("bridge_functions")).expect("mkdir");
        std::fs::create_dir_all(commands.join("_hidden")).expect("mkdir");
        std::fs::create_dir_all(root.join("Template/icons")).expect("mkdir");
        assert_eq!(script_folders(root), vec!["Samples", "Samples/Deep"]);
        assert_eq!(template_folders(root), vec!["icons"]);
    }
}
