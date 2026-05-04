from __future__ import annotations

import importlib
import os
import sys
from typing import TYPE_CHECKING
from unittest.mock import MagicMock, patch

import numpy as np
import pytest

if TYPE_CHECKING:
    from tests.conftest import MockCamera, MockSender

SAMPLES_DIR = os.path.join(
    os.path.dirname(os.path.dirname(__file__)),
    "SerialController",
    "Commands",
    "PythonCommands",
    "Samples",
)


def _load_sample_module(module_name: str, file_name: str | None = None):
    """Load a sample module by name, handling hyphenated filenames."""
    import importlib.util

    if file_name is None:
        file_name = module_name + ".py"
    spec = importlib.util.spec_from_file_location(
        module_name, os.path.join(SAMPLES_DIR, file_name)
    )
    assert spec is not None and spec.loader is not None
    mod = importlib.util.module_from_spec(spec)
    sys.modules[module_name] = mod
    spec.loader.exec_module(mod)
    return mod


sys.path.insert(
    0, os.path.join(os.path.dirname(os.path.dirname(__file__)), "SerialController")
)


class TestKeysImports:
    """Test that all Keys types used by sample scripts can be imported."""

    def test_import_button(self) -> None:
        from Commands.Keys import Button

        assert hasattr(Button, "A")
        assert hasattr(Button, "B")
        assert hasattr(Button, "X")
        assert hasattr(Button, "Y")
        assert hasattr(Button, "L")
        assert hasattr(Button, "R")
        assert hasattr(Button, "ZL")
        assert hasattr(Button, "ZR")
        assert hasattr(Button, "MINUS")
        assert hasattr(Button, "PLUS")
        assert hasattr(Button, "LCLICK")
        assert hasattr(Button, "RCLICK")
        assert hasattr(Button, "HOME")
        assert hasattr(Button, "CAPTURE")

    def test_import_direction(self) -> None:
        from Commands.Keys import Direction as Dir
        from Commands.Keys import Stick

        d = Dir(Stick.LEFT, 0, 0.5)
        assert d.stick is not None
        assert d.x is not None
        assert d.y is not None

    def test_import_stick(self) -> None:
        from Commands.Keys import Stick

        assert hasattr(Stick, "LEFT")
        assert hasattr(Stick, "RIGHT")

    def test_import_hat(self) -> None:
        from Commands.Keys import Hat

        assert hasattr(Hat, "TOP")
        assert hasattr(Hat, "RIGHT")
        assert hasattr(Hat, "BTM")
        assert hasattr(Hat, "LEFT")
        assert hasattr(Hat, "CENTER")

    def test_import_touchscreen(self) -> None:
        from Commands.Keys import Touchscreen

        ts = Touchscreen(160, 120)
        assert ts.x == 160
        assert ts.y == 120

    def test_direction_predefined(self) -> None:
        from Commands.Keys import Direction

        assert Direction.UP is not None
        assert Direction.DOWN is not None
        assert Direction.LEFT is not None
        assert Direction.RIGHT is not None

    def test_direction_right_stick(self) -> None:
        from Commands.Keys import Direction

        assert Direction.R_UP is not None
        assert Direction.R_DOWN is not None
        assert Direction.R_LEFT is not None
        assert Direction.R_RIGHT is not None


class TestPythonCommandImports:
    """Test that base command classes can be imported."""

    def test_import_python_command_base(self) -> None:
        from Commands.PythonCommandBase import ImageProcPythonCommand, PythonCommand

        assert PythonCommand is not None
        assert ImageProcPythonCommand is not None

    def test_python_command_has_required_methods(self) -> None:
        from Commands.PythonCommandBase import PythonCommand

        required_methods = [
            "press",
            "pressRep",
            "hold",
            "holdEnd",
            "wait",
            "short_wait",
            "finish",
            "checkIfAlive",
            "direct_serial",
            "reload_com_port",
            "LINE_text",
            "discord_text",
            "do",
            "start",
            "end",
            "sendStopRequest",
        ]
        for method_name in required_methods:
            assert hasattr(PythonCommand, method_name), f"Missing method: {method_name}"

    def test_image_proc_command_has_required_methods(self) -> None:
        from Commands.PythonCommandBase import ImageProcPythonCommand

        required_methods = [
            "isContainTemplate",
            "isContainTemplate_max",
            "isContainedImage",
            "saveCapture",
            "getCameraImage",
            "openImage",
            "get_filespec",
            "setTemplateDir",
            "displayRectangle",
            "displayText",
            "popupImage",
            "LINE_image",
            "discord_image",
        ]
        for method_name in required_methods:
            assert hasattr(ImageProcPythonCommand, method_name), (
                f"Missing method: {method_name}"
            )


class TestSenderMock:
    """Test the MockSender fixture."""

    def test_sender_open_close(self, mock_sender: MockSender) -> None:
        assert not mock_sender.isOpened()
        mock_sender.openSerial(3, "COM3")
        assert mock_sender.isOpened()
        mock_sender.closeSerial()
        assert not mock_sender.isOpened()

    def test_sender_write_row(self, mock_sender: MockSender) -> None:
        mock_sender.openSerial(3, "COM3")
        mock_sender.writeRow("test command")
        assert "test command" in mock_sender.written_rows

    def test_sender_write_list(self, mock_sender: MockSender) -> None:
        mock_sender.openSerial(3, "COM3")
        data = [1, 2, 3, 4]
        mock_sender.writeList(data)
        assert data in mock_sender.written_lists


class TestCameraMock:
    """Test the MockCamera fixture."""

    def test_camera_open_close(self, mock_camera: MockCamera) -> None:
        assert not mock_camera.isOpened()
        mock_camera.openCamera(0)
        assert mock_camera.isOpened()
        mock_camera.destroy()
        assert not mock_camera.isOpened()

    def test_camera_read_frame(self, mock_camera: MockCamera) -> None:
        mock_camera.openCamera(0)
        frame = mock_camera.readFrame()
        assert frame is not None
        assert isinstance(frame, np.ndarray)
        assert frame.shape == (720, 1280, 3)

    def test_camera_save_capture(self, mock_camera: MockCamera, tmp_path) -> None:
        mock_camera.openCamera(0)
        original_cwd = os.getcwd()
        try:
            os.chdir(str(tmp_path))
            mock_camera.saveCapture("test_capture")
            assert os.path.exists(os.path.join("./Captures", "test_capture.png"))
        finally:
            os.chdir(original_cwd)

    def test_camera_image_bgr_property(self, mock_camera: MockCamera) -> None:
        mock_camera.openCamera(0)
        img = mock_camera.image_bgr
        assert img is not None
        assert isinstance(img, np.ndarray)


class TestSampleScriptImports:
    """Test that all sample script files can be imported without errors."""

    @pytest.mark.parametrize(
        "filename",
        [
            "AutoLeague.py",
            "AutoRelease.py",
            "Discord_Test.py",
            "InputSerial.py",
            "isContainedImage_sample.py",
            "isContainTemplate_sample.py",
            "LineNotifySample.py",
            "LoggingSample.py",
            "MashA.py",
            "Print-sample.py",
            "Qingpi_Touchscreen_sample.py",
            "RaidPassword.py",
            "RecPlay.py",
            "Stick_Sample1.py",
            "Stick-Sample2.py",
            "Widget-sample.py",
        ],
    )
    def test_sample_import(self, filename: str) -> None:
        filepath = os.path.join(SAMPLES_DIR, filename)
        assert os.path.exists(filepath), f"Sample file not found: {filename}"

        spec = importlib.util.spec_from_file_location(
            f"Samples.{filename[:-3]}",
            filepath,
        )
        assert spec is not None
        assert spec.loader is not None
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)

    def test_listen_shiny_import_skips_missing_pyaudio(self) -> None:
        filepath = os.path.join(SAMPLES_DIR, "listen_shiny.py")
        assert os.path.exists(filepath)

        spec = importlib.util.spec_from_file_location("Samples.listen_shiny", filepath)
        assert spec is not None
        assert spec.loader is not None
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        assert hasattr(module, "ListenShiny")


class TestSampleInstantiation:
    """Test that sample command classes can be instantiated."""

    def test_auto_league_instantiation(self) -> None:
        from Samples.AutoLeague import AutoLeague

        cmd = AutoLeague()
        assert cmd.NAME == "自動リーグ周回"
        assert hasattr(cmd, "do")

    def test_mash_a_instantiation(self) -> None:
        from Samples.MashA import Mash_A

        cmd = Mash_A()
        assert cmd.NAME == "A連打"

    def test_input_serial_instantiation(self) -> None:
        from Samples.InputSerial import InputKeyboard

        cmd = InputKeyboard()
        assert cmd.NAME == "シリアル入力"

    def test_raid_password_instantiation(self) -> None:
        from Samples.RaidPassword import Move2

        cmd = Move2()
        assert cmd.NAME == "キーボード入力2"

    def test_stick_sample1_instantiation(self) -> None:
        from Samples.Stick_Sample1 import StickSample1

        cmd = StickSample1()
        assert cmd.NAME == "スティック1"

    def test_stick_sample2_instantiation(self) -> None:
        mod = _load_sample_module("Stick_Sample2", "Stick-Sample2.py")
        cmd = mod.StickSample2()
        assert cmd.NAME == "スティック2"

    def test_print_sample_instantiation(self) -> None:
        mod = _load_sample_module("Print_sample", "Print-sample.py")
        cmd = mod.Print_sample()
        assert cmd.NAME == "Print出力(sample)"

    def test_qingpi_touchscreen_instantiation(self) -> None:
        from Samples.Qingpi_Touchscreen_sample import Qingpi_Touchscreen_sample

        cmd = Qingpi_Touchscreen_sample()
        assert cmd.NAME == "Qingpi_Touchscreen_sample"

    def test_widget_sample_instantiation(self) -> None:
        mod = _load_sample_module("Widget_sample", "Widget-sample.py")
        cmd = mod.Widget_sample()
        assert cmd.NAME == "Widget-sample"

    def test_auto_release_instantiation(self, mock_camera: MockCamera) -> None:
        from Samples.AutoRelease import AutoRelease

        cmd = AutoRelease(mock_camera)
        assert cmd.NAME == "自動リリース"
        assert cmd.camera is mock_camera

    def test_discord_test_instantiation(self, mock_camera: MockCamera) -> None:
        from Samples.Discord_Test import DiscordTest

        cmd = DiscordTest(mock_camera)
        assert cmd.NAME == "Discord通知テスト"

    def test_logging_sample_instantiation(self, mock_camera: MockCamera) -> None:
        from Samples.LoggingSample import LoggingSample

        cmd = LoggingSample(mock_camera)
        assert cmd.NAME == "ログ出力のサンプル"

    def test_line_notify_sample_instantiation(self, mock_camera: MockCamera) -> None:
        from Samples.LineNotifySample import LineSample

        cmd = LineSample(mock_camera)
        assert cmd.NAME == "LINE通知サンプル"

    def test_is_contain_template_sample_instantiation(
        self, mock_camera: MockCamera
    ) -> None:
        from Samples.isContainTemplate_sample import isContainTemplate_sample

        cmd = isContainTemplate_sample(mock_camera)
        assert cmd.NAME == "画像認識関数紹介(1) ver. 0.0.1"

    def test_is_contained_image_sample_instantiation(
        self, mock_camera: MockCamera
    ) -> None:
        from Samples.isContainedImage_sample import isContainTemplate_sample

        cmd = isContainTemplate_sample(mock_camera)
        assert cmd.NAME == "画像認識関数紹介(2) ver. 0.0.1"


class TestPublicApiWithMocks:
    """Test public API methods using mock Sender and Camera."""

    def test_press_with_mock(self, mock_sender: MockSender) -> None:
        from Commands.Keys import Button

        cmd = AutoLeagueForTest()
        cmd.keys = MagicMock()
        cmd.keys.ser = mock_sender

        cmd.press(Button.A, duration=0.01, wait=0.01)
        cmd.keys.input.assert_called()
        cmd.keys.inputEnd.assert_called()

    def test_hold_with_mock(self, mock_sender: MockSender) -> None:
        from Commands.Keys import Direction, Stick

        cmd = AutoLeagueForTest()
        cmd.keys = MagicMock()
        cmd.keys.ser = mock_sender

        cmd.hold(Direction(Stick.LEFT, 70), wait=0.01)
        cmd.keys.hold.assert_called()

    def test_wait_with_mock(self, mock_sender: MockSender) -> None:
        cmd = AutoLeagueForTest()
        cmd.keys = MagicMock()
        cmd.keys.ser = mock_sender
        cmd.alive = True

        cmd.wait(0.01)

    def test_finish_with_mock(self, mock_sender: MockSender) -> None:
        from Commands.PythonCommandBase import StopThread

        cmd = AutoLeagueForTest()
        cmd.keys = MagicMock()
        cmd.keys.ser = mock_sender
        cmd.socket0 = MagicMock()
        cmd.mqtt0 = MagicMock()

        with pytest.raises(StopThread):
            cmd.finish()
        assert cmd.alive is False

    def test_discord_text_with_mock(self, mock_sender: MockSender) -> None:

        cmd = AutoLeagueForTest()
        cmd.keys = MagicMock()
        cmd.keys.ser = mock_sender
        cmd.Discord = MagicMock()

        cmd.discord_text("test message")
        cmd.Discord.send_message.assert_called()

    def test_image_proc_save_capture(self, mock_camera: MockCamera, tmp_path) -> None:

        cmd = ImageProcTest(mock_camera)
        original_cwd = os.getcwd()
        try:
            os.chdir(str(tmp_path))
            cmd.saveCapture("test", mode=True)
        finally:
            os.chdir(original_cwd)

    def test_image_proc_get_filespec(self, mock_camera: MockCamera) -> None:
        cmd = ImageProcTest(mock_camera)

        result = cmd.get_filespec("test.png", mode="t")
        assert "test.png" in result

        result = cmd.get_filespec("/abs/path/test.png", mode="t")
        assert result == "/abs/path/test.png"

    def test_image_proc_is_contain_template(
        self, mock_camera: MockCamera, tmp_path
    ) -> None:
        import numpy as np

        cmd = ImageProcTest(mock_camera)
        dummy_img = np.zeros((100, 100, 3), dtype=np.uint8)
        template_path = tmp_path / "dummy.png"
        # Write a minimal valid PNG so getFilespec works (no cv2 dependency)
        template_path.write_bytes(b"\x89PNG\r\n\x1a\n" + b"\x00" * 100)
        cmd.template_path_name = str(tmp_path)
        cmd.gui = MagicMock()
        cmd.canvas = MagicMock()

        with patch("ImageProcessing.ImageProcessing") as mock_img_proc:
            mock_instance = MagicMock()
            mock_instance.isContainTemplate.return_value = (
                True,
                (0, 0),
                100,
                100,
                0.9,
            )
            mock_img_proc.return_value = mock_instance
            # Preserve image_type for isinstance() checks in commands.py
            import numpy as np
            mock_img_proc.image_type = np.ndarray

            result = cmd.isContainTemplate(
                "dummy.png", threshold=0.7, use_gray=True, show_position=False
            )
            assert result is True

    def test_image_proc_is_contained_image(
        self, mock_camera: MockCamera, tmp_path
    ) -> None:
        import numpy as np

        cmd = ImageProcTest(mock_camera)
        dummy_img = np.zeros((100, 100, 3), dtype=np.uint8)
        image_path = tmp_path / "dummy.png"
        # Write a minimal valid PNG so getFilespec works (no cv2 dependency)
        image_path.write_bytes(b"\x89PNG\r\n\x1a\n" + b"\x00" * 100)
        cmd.template_path_name = str(tmp_path)
        cmd.capture_path_name = str(tmp_path)
        cmd.gui = MagicMock()
        cmd.canvas = MagicMock()

        with patch("ImageProcessing.ImageProcessing") as mock_img_proc:
            mock_instance = MagicMock()
            mock_instance.isContainTemplate.return_value = (
                True,
                (0, 0),
                100,
                100,
                0.9,
            )
            mock_img_proc.return_value = mock_instance
            # Preserve image_type for isinstance() checks in commands.py
            import numpy as np
            mock_img_proc.image_type = np.ndarray

            result = cmd.isContainedImage(
                "dummy.png", threshold=0.7, show_position=False
            )
            assert result is True


from Commands.PythonCommandBase import ImageProcPythonCommand, PythonCommand


class AutoLeagueForTest(PythonCommand):
    def __init__(self) -> None:
        super().__init__()
        self.socket0 = MagicMock()
        self.mqtt0 = MagicMock()

    def do(self) -> None:
        pass


class ImageProcTest(ImageProcPythonCommand):
    def __init__(self, cam) -> None:
        super().__init__(cam)

    def do(self) -> None:
        pass
