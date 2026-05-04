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

# Import pokecon to trigger patching of Commands.Keys and Commands.PythonCommandBase
# This must happen BEFORE any test file's module-level imports of these names,
# because the real PythonCommandBase.py uses Python 3.12+ syntax (PEP 695 generics).
PYTHON_DIR = os.path.join(PROJECT_ROOT, "python")
if PYTHON_DIR not in sys.path:
    sys.path.insert(0, PYTHON_DIR)

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

    # Mock ImageProcessing — the real file uses Python 3.12+ syntax (type CropFmt = ...)
    _mock_imgproc = types.ModuleType("ImageProcessing")
    import numpy as _np

    # Create a callable class mock that works with isinstance() AND attribute access
    # image_type must be accessible both as: ImageProcessing.image_type AND instance.image_type
    class _MockImgProcMeta(type):
        def __getattr__(cls, name):
            return MagicMock()

        def __call__(cls, *args, **kwargs):
            return MagicMock()

    class _MockImgProc(metaclass=_MockImgProcMeta):
        image_type = _np.ndarray

    _mock_imgproc.ImageProcessing = _MockImgProc
    _mock_imgproc.getImage = MagicMock(return_value=None)
    _mock_imgproc.crop_image = MagicMock(return_value=None)
    _mock_imgproc.opneImage = MagicMock()
    sys.modules["ImageProcessing"] = _mock_imgproc

    # Mock Camera — the real file may have version-dependent imports
    _mock_camera = types.ModuleType("Camera")
    _mock_camera.Camera = MagicMock
    sys.modules["Camera"] = _mock_camera

    # Mock Commands module package (imported by original PythonCommandBase)
    if "Commands" not in sys.modules:
        _mock_cmds = types.ModuleType("Commands")
        _mock_cmds.__path__ = []  # Make it a package
        sys.modules["Commands"] = _mock_cmds

    # Mock Commands.Keys for direct imports - use actual pokecon classes
    import pokecon as _pokecon

    _mock_keys = types.ModuleType("Commands.Keys")
    _mock_keys.Button = _pokecon.Button
    _mock_keys.Direction = _pokecon.Direction
    _mock_keys.Stick = _pokecon.Stick
    _mock_keys.Hat = _pokecon.Hat
    _mock_keys.Touchscreen = _pokecon.Touchscreen
    _mock_keys.KeyPress = _pokecon.KeyPress
    _mock_keys.SendFormat = _pokecon.SendFormat
    _mock_keys.Tilt = _pokecon.Tilt
    sys.modules["Commands.Keys"] = _mock_keys

    # Mock Commands.PythonCommandBase for direct imports - use actual pokecon classes
    _mock_pybase = types.ModuleType("Commands.PythonCommandBase")
    _mock_pybase.PythonCommand = _pokecon.PythonCommand
    _mock_pybase.ImageProcPythonCommand = _pokecon.ImageProcPythonCommand
    _mock_pybase.StopThread = Exception
    sys.modules["Commands.PythonCommandBase"] = _mock_pybase


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
        # Write a minimal placeholder PNG (no OpenCV dependency needed)
        import struct
        import zlib

        raw = b""
        for _ in range(32):
            raw += b"\x00" + b"\x00\x00\x00" * 32

        def _chunk(ctype, data):
            c = ctype + data
            return (
                struct.pack(">I", len(data))
                + c
                + struct.pack(">I", zlib.crc32(c) & 0xFFFFFFFF)
            )

        ihdr = struct.pack(">IIBBBBB", 32, 32, 8, 2, 0, 0, 0)
        with open(path, "wb") as f:
            f.write(b"\x89PNG\r\n\x1a\n")
            f.write(_chunk(b"IHDR", ihdr))
            f.write(_chunk(b"IDAT", zlib.compress(raw)))
            f.write(_chunk(b"IEND", b""))

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
