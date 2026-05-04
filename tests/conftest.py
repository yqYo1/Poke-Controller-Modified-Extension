from __future__ import annotations

import os
import sys
import types
from unittest.mock import MagicMock

# Create mock tkinter before anything else imports it
_mock_tk = types.ModuleType("tkinter")
_mock_tk.BooleanVar = MagicMock
_mock_tk.Toplevel = MagicMock
_mock_tk.Text = MagicMock
for name in [
    "Label",
    "Button",
    "Entry",
    "Frame",
    "StringVar",
    "IntVar",
    "Checkbutton",
    "OptionMenu",
    "Radiobutton",
    "Spinbox",
    "Scale",
    "Combobox",
    "Scrollbar",
    "Canvas",
    "PhotoImage",
    "Tk",
    "Menu",
    "Menubutton",
    "Message",
    "PanedWindow",
    "Toplevel",
    "simpledialog",
    "colorchooser",
    "filedialog",
    "font",
    "N",
    "S",
    "E",
    "W",
    "NW",
    "NE",
    "SW",
    "SE",
]:
    setattr(_mock_tk, name, MagicMock)

_mock_tkf = types.ModuleType("tkinter.filedialog")
_mock_tkf.askopenfile = MagicMock(return_value=MagicMock(name="mock_file"))
_mock_tkf.askopenfilename = MagicMock(return_value="")
_mock_tkf.asksaveasfilename = MagicMock(return_value="")
_mock_tkf.askdirectory = MagicMock(return_value="")
_mock_tk.filedialog = _mock_tkf

_mock_tkm = types.ModuleType("tkinter.messagebox")
_mock_tkm.showinfo = MagicMock()
_mock_tkm.showwarning = MagicMock()
_mock_tkm.showerror = MagicMock()
_mock_tkm.askokcancel = MagicMock(return_value=True)
_mock_tkm.askyesno = MagicMock(return_value=True)
_mock_tk.messagebox = _mock_tkm

_mock_tks = types.ModuleType("tkinter.simpledialog")
_mock_tks.askstring = MagicMock()
_mock_tk.simpledialog = _mock_tks

sys.modules["tkinter"] = _mock_tk
sys.modules["tkinter.filedialog"] = _mock_tkf
sys.modules["tkinter.messagebox"] = _mock_tkm
sys.modules["tkinter.simpledialog"] = _mock_tks
sys.modules["tkinter.ttk"] = _mock_tk

PROJECT_ROOT = os.path.dirname(os.path.dirname(__file__))
SERIALCONTROLLER_DIR = os.path.join(PROJECT_ROOT, "SerialController")

if SERIALCONTROLLER_DIR not in sys.path:
    sys.path.insert(0, SERIALCONTROLLER_DIR)

PYTHONCOMMANDS_DIR = os.path.join(SERIALCONTROLLER_DIR, "Commands", "PythonCommands")
if PYTHONCOMMANDS_DIR not in sys.path:
    sys.path.insert(0, PYTHONCOMMANDS_DIR)

# Mock plyer notification
_mock_plyer = types.ModuleType("plyer")
_mock_plyer.notification = MagicMock()
sys.modules["plyer"] = _mock_plyer


# Apply remaining patches to prevent import errors
def _apply_import_patches() -> None:
    _mock_discord = types.ModuleType("DiscordNotify")
    _mock_discord.Discord_Notify = MagicMock
    sys.modules["DiscordNotify"] = _mock_discord

    _mock_line = types.ModuleType("LineNotify")
    _mock_line.Line_Notify = MagicMock
    sys.modules["LineNotify"] = _mock_line

    _mock_dialogue = types.ModuleType("PokeConDialogue")
    _mock_dialogue.PokeConDialogue = MagicMock
    _mock_dialogue.check_widget_name = MagicMock(return_value=True)
    _mock_dialogue.generate_new_dialogue_list = MagicMock(return_value=[])
    _mock_dialogue.get_settings_list = MagicMock(return_value=[])
    _mock_dialogue.save_dialogue_settings = MagicMock()
    sys.modules["PokeConDialogue"] = _mock_dialogue

    _mock_settings = types.ModuleType("Settings")
    _mock_settings.GuiSettings = MagicMock
    sys.modules["Settings"] = _mock_settings

    _mock_ext_tools = types.ModuleType("ExternalTools")
    _mock_ext_tools.SocketCommunications = MagicMock
    _mock_ext_tools.MQTTCommunications = MagicMock
    sys.modules["ExternalTools"] = _mock_ext_tools

    _mock_assets = types.ModuleType("gui.assets")
    _mock_assets.CaptureArea = MagicMock
    sys.modules["gui"] = types.ModuleType("gui")
    sys.modules["gui.assets"] = _mock_assets

    _mock_redirector = types.ModuleType("text_redirector")
    _mock_redirector.TextRedirector = MagicMock
    sys.modules["text_redirector"] = _mock_redirector


_apply_import_patches()

from typing import TYPE_CHECKING

import pytest

if TYPE_CHECKING:
    from cv2.typing import MatLike


class MockSender:
    """Mock for SerialController/Commands/Sender.py Sender class."""

    def __init__(self, *args, **kwargs) -> None:
        self.is_open = False
        self.written_rows: list[str] = []
        self.written_lists: list[list[int]] = []

    def openSerial(
        self, portNum: int, portName: str | None = "", baudrate: int = 9600
    ) -> bool:
        self.is_open = True
        return True

    def closeSerial(self) -> None:
        self.is_open = False

    def isOpened(self) -> bool:
        return self.is_open

    def writeRow(self, row: str, is_show: bool = False) -> None:
        self.written_rows.append(row)

    def writeList(self, values: list[int], is_show: bool = False) -> None:
        self.written_lists.append(values)

    def writeRow_wo_perf_counter(self, row: str, is_show: bool = False) -> None:
        self.written_rows.append(row)

    def show_input(self, output: list[str]) -> None:
        pass


class MockCamera:
    """Mock for SerialController/Camera.py Camera class."""

    def __init__(self, fps: int = 45) -> None:
        self.fps = fps
        self._is_opened = False
        self._flip = False
        self._flip_mode = 0

    @property
    def image_bgr(self) -> MatLike:
        import numpy as np

        return np.zeros((720, 1280, 3), dtype=np.uint8)

    @property
    def flip(self) -> bool:
        return self._flip

    @flip.setter
    def flip(self, value: bool) -> None:
        self._flip = value

    @property
    def flip_mode(self) -> int:
        return self._flip_mode

    @flip_mode.setter
    def flip_mode(self, value: int) -> None:
        self._flip_mode = value

    def set_flip(self, value: str) -> None:
        pass

    def openCamera(self, cameraId: int) -> None:
        self._is_opened = True

    def isOpened(self) -> bool:
        return self._is_opened

    def readFrame(self) -> MatLike:
        import numpy as np

        return np.zeros((720, 1280, 3), dtype=np.uint8)

    def saveCapture(
        self,
        filename: str | None = None,
        crop: int | str | None = None,
        crop_ax: list[int] | None = None,
        img: MatLike | None = None,
    ) -> None:
        os.makedirs("./Captures", exist_ok=True)
        if filename is None:
            filename = "capture"
        if not filename.endswith(".png"):
            filename = filename + ".png"
        path = os.path.join("./Captures", filename)
        parent = os.path.dirname(path)
        if parent:
            os.makedirs(parent, exist_ok=True)
        import cv2
        import numpy as np

        cv2.imwrite(path, np.zeros((720, 1280, 3), dtype=np.uint8))

    def destroy(self) -> None:
        self._is_opened = False

    def camera_thread_start(self) -> None:
        pass

    def camera_thread_stop(self) -> None:
        pass


@pytest.fixture()
def mock_sender() -> MockSender:
    return MockSender()


@pytest.fixture()
def mock_camera() -> MockCamera:
    return MockCamera()
