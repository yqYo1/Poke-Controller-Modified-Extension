"""
Full compatibility wrappers for PythonCommand and ImageProcPythonCommand.

Provides the exact same public API as original SerialController/Commands/PythonCommandBase.py
without importing from old modules (self-contained for cross-version compatibility).

Delegates serial I/O and image processing to the Rust-backed stack when available.
"""

from __future__ import annotations

import contextlib
import datetime
import os
import os.path
import random
import string
import threading
import time
import traceback
from abc import ABC, abstractmethod
from functools import wraps
from logging import DEBUG, Logger, NullHandler, getLogger
from time import sleep
from typing import (
    TYPE_CHECKING,
    Callable,
    ClassVar,
    Concatenate,
    Literal,
    ParamSpec,
    Sequence,
    TypeVar,
)

from pokecon._meta import CommandMeta

if TYPE_CHECKING:
    from typing import Any


# ---------------------------------------------------------------------------
# TypeVars for the pause decorator
# ---------------------------------------------------------------------------
P = ParamSpec("P")
R = TypeVar("R")
PythonCommandLike = TypeVar("PythonCommandLike", bound="PythonCommand")


# ---------------------------------------------------------------------------
# StopThread exception
# ---------------------------------------------------------------------------
class StopThread(Exception):
    """For notifying stop signal from Main window."""

    pass


# ---------------------------------------------------------------------------
# Pause decorator — matches original exactly
# ---------------------------------------------------------------------------
def pausedecorator(
    func: Callable[Concatenate[PythonCommandLike, P], R],
) -> Callable[Concatenate[PythonCommandLike, P], R]:
    """
    一時停止を実現するためのデコレータです。
    """

    @wraps(func)
    def inner(self: PythonCommandLike, *args: P.args, **kwargs: P.kwargs) -> R:
        result = func(self, *args, **kwargs)
        if self.isPause:
            self.show_var()
        while self.isPause:
            sleep(0.5)
            self.checkIfAlive()
        return result

    return inner


# ---------------------------------------------------------------------------
# Stub imports — resolved at runtime via lazy import
# These are the external dependencies the original code needs.
# They are imported here lazily so the module can load without them.
# ---------------------------------------------------------------------------


def _import_sender():
    """Lazy import Sender to avoid circular deps."""
    from Commands.Sender import Sender  # noqa: F401

    return Sender


def _import_keypress():
    """Lazy import KeyPress to avoid circular deps."""
    from Commands.Keys import KeyPress  # noqa: F401

    return KeyPress


def _import_notify():
    """Lazy import notification dependencies."""
    try:
        from plyer import notification  # noqa: F401

        return notification
    except Exception:
        return None


# ---------------------------------------------------------------------------
# CommandBase simulation — provides the interface that user scripts expect
# without importing the real CommandBase.
# ---------------------------------------------------------------------------
class _CommandBaseStub(ABC):
    """Compatibility stub for CommandBase.Command functionality.

    Provides the essential class variables and methods that user scripts
    and PythonCommand expect, without depending on the real CommandBase module.
    """

    NAME: ClassVar[str] = ""
    TAGS: ClassVar[Any] = None

    # Class variables set externally by the GUI framework
    text_area_1: ClassVar[Any] = None
    text_area_2: ClassVar[Any] = None
    text_redirector1: ClassVar[Any] = None
    text_redirector2: ClassVar[Any] = None
    stdout_destination: str = "1"
    pos_dialogue_buttons: Literal[1, 2, 3] = 2
    isPause: bool = False
    canvas: Any = None
    isGuide: bool = False
    isSimilarity: bool = False
    isImage: bool = False
    isWinNotStart: bool = False
    isWinNotEnd: bool = False
    isLineNotStart: bool = False
    isLineNotEnd: bool = False
    isDiscordNotStart: bool = False
    isDiscordNotEnd: bool = False
    app_name: str = ""
    cur_command_name: str = ""
    profilename: str = ""

    def __init__(self) -> None:
        self.isRunning: bool = False
        self.message_dialogue: Any = None
        self.socket0: Any = None  # SocketCommunications stub
        self.mqtt0: Any = None  # MQTTCommunications stub
        self._init_comms()

    def _init_comms(self) -> None:
        """Initialize socket and MQTT communication stubs."""
        try:
            from ExternalTools import MQTTCommunications, SocketCommunications

            self.socket0 = SocketCommunications()
            self.mqtt0 = MQTTCommunications("")
        except ImportError:
            self.socket0 = _SocketStub()
            self.mqtt0 = _MQTTStub("")

    @abstractmethod
    def start(self, ser, postProcess: Callable[[], None]) -> None: ...

    @abstractmethod
    def end(self, ser) -> None: ...

    # ── Print functions ─────────────────────────────────────────────
    def print_t1(self, *objects: object, sep: str = " ", end: str = "\n") -> None:
        print(*objects, sep=sep, end=end)

    def print_t2(self, *objects: object, sep: str = " ", end: str = "\n") -> None:
        print(*objects, sep=sep, end=end)

    def print_t(self, *objects: object, sep: str = " ", end: str = "\n") -> None:
        print(*objects, sep=sep, end=end)

    def print_s(self, *objects: object, sep: str = " ", end: str = "\n") -> None:
        print(*objects, sep=sep, end=end)

    def print_ts(self, *objects: object, sep: str = " ", end: str = "\n") -> None:
        print(*objects, sep=sep, end=end)

    def print_t1b(
        self,
        mode: Literal["w", "a", "d"],
        *objects: object,
        sep: str = " ",
        end: str = "\n",
    ) -> None:
        print(*objects, sep=sep, end=end)

    def print_t2b(
        self,
        mode: Literal["w", "a", "d"],
        *objects: object,
        sep: str = " ",
        end: str = "\n",
    ) -> None:
        print(*objects, sep=sep, end=end)

    def print_tb(
        self,
        mode: Literal["w", "a", "d"],
        *objects: object,
        sep: str = " ",
        end: str = "\n",
    ) -> None:
        print(*objects, sep=sep, end=end)

    def print_tbs(
        self,
        mode: Literal["w", "a", "d"],
        *objects: object,
        sep: str = " ",
        end: str = "\n",
    ) -> None:
        print(*objects, sep=sep, end=end)

    # ── Dialog functions ────────────────────────────────────────────
    def dialogue(self, title: str, message, desc=None, need=list):
        print(f"dialogue({title}): stubbed")
        return []

    def dialogue6widget(self, title: str, dialogue_list, desc=None, need=list):
        print(f"dialogue6widget({title}): stubbed")
        return {}

    def dialogue6widget_save_settings(
        self, title: str, dialogue_list, filename, desc=None, need=list
    ):
        print(f"dialogue6widget_save_settings({title}): stubbed")
        return []

    def dialogue6widget_select_settings(
        self, title: str, dialogue_list, dirname, desc=None, need=list
    ):
        print(f"dialogue6widget_select_settings({title}): stubbed")
        return []

    # ── Socket functions ────────────────────────────────────────────
    def socket_change_alive(self, flag: bool) -> None:
        if self.socket0 is not None:
            self.socket0.alive = flag

    def socket_change_ipaddr(self, addr: str) -> None:
        if self.socket0 is not None:
            self.socket0.change_ipaddr(addr)

    def socket_change_port(self, port: int) -> None:
        if self.socket0 is not None:
            self.socket0.change_port(port)

    def socket_connect(self) -> None:
        if self.socket0 is not None:
            self.socket0.sock_connect()

    def socket_disconnect(self) -> None:
        if self.socket0 is not None:
            self.socket0.sock_disconnect()

    def socket_receive_message(self, header: str, show_msg: bool = False) -> str | None:
        if self.socket0 is not None:
            return self.socket0.receive_message(header, show_msg=show_msg)
        return None

    def socket_receive_message2(
        self, headerlist: list[str], show_msg: bool = False
    ) -> str | None:
        if self.socket0 is not None:
            return self.socket0.receive_message2(headerlist, show_msg=show_msg)
        return None

    def socket_transmit_message(self, message: str) -> None:
        if self.socket0 is not None:
            self.socket0.transmit_message(message)
        self.checkIfAlive()

    # ── MQTT functions ──────────────────────────────────────────────
    def mqtt_change_broker_address(self, broker_address: str) -> None:
        if self.mqtt0 is not None:
            self.mqtt0.change_broker_address(broker_address)

    def mqtt_change_id(self, mqtt_id: str) -> None:
        if self.mqtt0 is not None:
            self.mqtt0.change_id(mqtt_id)

    def mqtt_change_pub_token(self, pub_token: str) -> None:
        if self.mqtt0 is not None:
            self.mqtt0.change_pub_token(pub_token)

    def mqtt_change_sub_token(self, sub_token: str) -> None:
        if self.mqtt0 is not None:
            self.mqtt0.change_sub_token(sub_token)

    def mqtt_connect(self) -> None:
        if self.mqtt0 is not None:
            self.mqtt0.mqtt_connect()

    def mqtt_disconnect(self) -> None:
        if self.mqtt0 is not None:
            self.mqtt0.mqtt_disconnect()

    def mqtt_publish(self, pub_topic: str, pub_message: str) -> None:
        if self.mqtt0 is not None:
            self.mqtt0.mqtt_publish(pub_topic, pub_message)


class _SocketStub:
    alive: bool = True
    flag_socket: bool = False

    def change_ipaddr(self, addr: str) -> None:
        pass

    def change_port(self, port: int) -> None:
        pass

    def sock_connect(self) -> None:
        pass

    def sock_disconnect(self) -> None:
        pass

    def receive_message(self, header: str, show_msg: bool = False) -> str | None:
        return None

    def receive_message2(
        self, headerlist: list[str], show_msg: bool = False
    ) -> str | None:
        return None

    def transmit_message(self, message: str) -> None:
        pass


class _MQTTStub:
    alive: bool = True

    def __init__(self, broker_address: str):
        pass

    def change_broker_address(self, addr: str):
        pass

    def change_id(self, id_: str):
        pass

    def change_pub_token(self, token: str):
        pass

    def change_sub_token(self, token: str):
        pass

    def mqtt_connect(self):
        pass

    def mqtt_disconnect(self):
        pass

    def mqtt_publish(self, topic: str, message: str):
        pass


# ---------------------------------------------------------------------------
# PythonCommand — matches original's public interface exactly
# ---------------------------------------------------------------------------
class PythonCommand(_CommandBaseStub, ABC, metaclass=CommandMeta):
    __is_interface__ = True
    """Base class for user automation scripts.

    Supports the original API: press(), hold(), holdEnd(), wait(),
    short_wait(), finish(), checkIfAlive(), direct_serial(),
    reload_com_port(), LINE_text(), discord_text(), etc.
    """

    def __init__(self) -> None:
        super().__init__()
        self._logger: Logger = getLogger(__name__)
        self._logger.addHandler(NullHandler())
        self._logger.setLevel(DEBUG)
        self._logger.propagate = True

        self.alive: bool = True
        self.keys: Any = None  # KeyPress — set during do_safe
        self.postProcess: Callable[[], None] | None = None
        self.thread: threading.Thread | None = None
        self.Line: Any = None
        self.Discord: Any = None
        self._init_notifications()

    def _init_notifications(self) -> None:
        """Initialize notification services (LINE, Discord)."""
        try:
            from DiscordNotify import Discord_Notify

            self.Discord = Discord_Notify()
        except ImportError:
            pass
        try:
            from LineNotify import Line_Notify

            self.Line = Line_Notify()
        except ImportError:
            self.Line = None

    def show_var(self) -> None:
        """Display internal variable list during pause."""
        var_dict = vars(self)
        del_dict = [
            "isRunning",
            "message_dialogue",
            "socket0",
            "mqtt0",
            "keys",
            "thread",
            "alive",
            "postProcess",
            "Line",
            "Discord",
            "_logger",
            "camera",
            "gui",
            "ImgProc",
        ]
        print("--------内部変数一覧--------")
        for k, v in var_dict.items():
            if k not in del_dict:
                print(k, "=", v)
        print("----------------------------")

    @abstractmethod
    def do(self) -> None:
        """Override with automation logic."""
        ...

    def do_safe(self, ser) -> None:
        """Orchestrate execution: setup → do() → finish()."""
        try:
            from Commands.Keys import KeyPress
        except ImportError:
            raise ImportError("Commands.Keys.KeyPress is required but not available")

        if self.keys is None:
            self.keys = KeyPress(ser)
            self.keys.init_hat()

        try:
            if self.alive:
                if self.isWinNotStart:
                    notification = _import_notify()
                    if notification:
                        notification.notify(
                            title=f"{self.app_name} (profile:{self.profilename})",
                            message=f"{self.cur_command_name} started.",
                            timeout=5,
                        )
                    else:
                        print('"plyer" is not installed.')
                if self.isLineNotStart:
                    self.LINE_text(
                        f"{self.app_name} (profile:{self.profilename})\n{self.cur_command_name} started.",
                    )
                if self.isDiscordNotStart:
                    self.discord_text(
                        f"{self.app_name} (profile:{self.profilename})\n{self.cur_command_name} started.",
                    )
                self.do()
                self.finish()
        except StopThread:
            print("-- finished successfully. --")
            self._logger.info("Command finished successfully")
            if self.isWinNotEnd:
                notification = _import_notify()
                if notification:
                    notification.notify(
                        title=f"{self.app_name} (profile:{self.profilename})",
                        message=f"{self.cur_command_name} finished.",
                        timeout=5,
                    )
                else:
                    print('"plyer" is not installed.')
            if self.isLineNotEnd:
                self.LINE_text(
                    f"{self.app_name} (profile:{self.profilename})\n{self.cur_command_name} finished.",
                )
            if self.isDiscordNotEnd:
                self.discord_text(
                    f"{self.app_name} (profile:{self.profilename})\n{self.cur_command_name} finished.",
                )
        except Exception as e:
            from Commands.Keys import KeyPress

            self.keys = KeyPress(ser)
            self.keys.init_hat()
            print("Interrupt:cmd(黒い画面)を確認してください。")
            print(e)
            self._logger.warning("Command stopped unexpectedly")
            traceback.print_exc()
            self.keys.end()
            self.alive = False

    def start(self, ser, postProcess: Callable[[], None]) -> None:
        """Start automation script in a thread."""
        self.alive = True
        if hasattr(self, "socket0") and self.socket0 is not None:
            self.socket0.alive = True
        if hasattr(self, "mqtt0") and self.mqtt0 is not None:
            self.mqtt0.alive = True
        self.postProcess = postProcess
        # Reset template path for ImageProcPythonCommand
        try:
            from pokecon.commands import ImageProcPythonCommand

            ImageProcPythonCommand.template_path_name = "./Template/"
        except ImportError:
            pass
        if not self.thread:
            self.thread = threading.Thread(target=self.do_safe, args=(ser,))
            self.thread.start()

    def end(self, ser) -> None:
        """Signal stop."""
        if hasattr(self, "socket0") and self.socket0 is not None:
            self.socket0.alive = False
        if hasattr(self, "mqtt0") and self.mqtt0 is not None:
            self.mqtt0.alive = False
        self.sendStopRequest()
        _ = ser

    def sendStopRequest(self) -> None:
        """Send stop request to running command."""
        if self.checkIfAlive():
            self.alive = False
            print("-- sent a stop request. --")
            self._logger.info("Sending stop request")
        if (
            hasattr(self, "socket0")
            and self.socket0 is not None
            and hasattr(self.socket0, "flag_socket")
        ):
            if self.socket0.flag_socket:
                self.socket_disconnect()

    def finish(self) -> None:
        """Gracefully finish the automation script."""
        self.alive = False
        if hasattr(self, "socket0") and self.socket0 is not None:
            self.socket0.alive = False
        if hasattr(self, "mqtt0") and self.mqtt0 is not None:
            self.mqtt0.alive = False
        if self.keys is not None:
            # end with ser attribute
            ser = getattr(self.keys, "ser", None)
            if ser is not None:
                self.end(ser)

    @pausedecorator
    def press(
        self,
        buttons,
        duration: float = 0.1,
        wait: float = 0.1,
    ) -> None:
        """Press button(s) for a duration."""
        if self.keys is not None:
            self.keys.input(buttons)
            self.wait(duration)
            self.keys.inputEnd(buttons)
            self.wait(wait)
        self.checkIfAlive()

    def pressRep(
        self,
        buttons,
        repeat: int,
        duration: float = 0.1,
        interval: float = 0.1,
        wait: float = 0.1,
    ) -> None:
        """Press button(s) repeatedly."""
        for i in range(repeat):
            self.press(buttons, duration, 0 if i == repeat - 1 else interval)
        self.wait(wait)

    @pausedecorator
    def hold(
        self,
        buttons,
        wait: float = 0.1,
    ) -> None:
        """Hold button(s) down."""
        if self.keys is not None:
            self.keys.hold(buttons)
        self.wait(wait)

    @pausedecorator
    def holdEnd(
        self,
        buttons,
    ) -> None:
        """Release held button(s)."""
        if self.keys is not None:
            self.keys.holdEnd(buttons)
        self.checkIfAlive()

    @pausedecorator
    def short_wait(self, wait: float) -> None:
        """Busy-wait for a short duration."""
        current_time = time.perf_counter()
        while time.perf_counter() < current_time + wait:
            pass
        self.checkIfAlive()

    @pausedecorator
    def wait(self, wait: float) -> None:
        """Wait for specified duration."""
        if float(wait) > 0.1:
            sleep(wait)
        else:
            current_time = time.perf_counter()
            while time.perf_counter() < current_time + wait:
                pass
        self.checkIfAlive()

    def checkIfAlive(self) -> bool:
        """Check alive flag; raise StopThread if False."""
        if not self.alive:
            if self.keys is not None:
                self.keys.end()
            self.keys = None
            self.thread = None
            if self.postProcess is not None:
                self.postProcess()
                self.postProcess = None
            self._logger.info("Exit from command successfully")
            raise StopThread("exit successfully")
        return True

    def direct_serial(self, serialcommands: list[str], waittime: list[float]) -> None:
        """Send raw serial commands."""
        if self.keys is not None:
            checkedcommands = [
                row.replace("\r", "").replace("\n", "") for row in serialcommands
            ]
            self.keys.serialcommand_direct_send(checkedcommands, waittime)

    def reload_com_port(self) -> None:
        """Reload COM port."""
        if self.keys is not None and hasattr(self.keys, "ser"):
            try:
                from Settings import GuiSettings
            except ImportError:
                print("Settings.GuiSettings not available")
                return
            ser = self.keys.ser
            if ser.isOpened():
                print("Port is already opened and being closed.")
                ser.closeSerial()
                self.reload_com_port()
            elif ser.openSerial(
                GuiSettings().com_port.get(),
                GuiSettings().com_port_name.get(),
                GuiSettings().baud_rate.get(),
            ):
                msg = (
                    "COM Port "
                    + str(GuiSettings().com_port.get())
                    + " connected successfully",
                )
                print(msg)
                self._logger.debug(msg)

    def LINE_text(self, txt: str, token: str = "") -> None:
        """Send LINE notification (currently disabled)."""
        print("LINE通知は使用出来ません。")
        print("Discord通知への切り替えを検討してください。")

    def discord_text(
        self,
        content: str = "",
        index: int = 0,
        keys: str = "DISCORD_WEBHOOK",
    ) -> None:
        """Send Discord notification."""
        if index != 0 and keys == "DISCORD_WEBHOOK":
            keys = f"DISCORD_WEBHOOK{index}"
        elif index == 0 and keys != "DISCORD_WEBHOOK":
            pass
        elif index != 0 and keys != "DISCORD_WEBHOOK":
            keys = f"DISCORD_WEBHOOK{index}"
        else:
            pass
        if self.Discord is not None and hasattr(self.Discord, "send_message"):
            with contextlib.suppress(Exception):
                self.Discord.send_message(notification_message=content, keys=keys)


# ---------------------------------------------------------------------------
# Utility functions
# ---------------------------------------------------------------------------
def generateRandomCharacter(n: int) -> str:
    """Generate random alphanumeric string of length n."""
    chars = string.ascii_lowercase + string.ascii_uppercase + string.digits
    return "".join([random.choice(chars) for _ in range(n)])


def convertCv2Format(
    crop_fmt: Any = "",
    crop: list[int] | None = None,
) -> tuple[list[int], list[int]]:
    """Convert crop coordinates between Pillow and OpenCV formats.

    See original PythonCommandBase.py docstring for format details.
    """
    if crop is None:
        crop = []

    try:
        if crop_fmt in {1, "1"}:
            res_cv2 = [crop[1], crop[3], crop[0], crop[2]]
        elif crop_fmt in {2, "2"}:
            res_cv2 = [crop[1], crop[1] + crop[3], crop[0], crop[0] + crop[2]]
        elif crop_fmt in {3, "3"}:
            res_cv2 = [crop[2], crop[3], crop[0], crop[1]]
        elif crop_fmt in {4, "4"}:
            res_cv2 = [crop[2], crop[2] + crop[3], crop[0], crop[0] + crop[1]]
        elif crop_fmt in {11, "11"}:
            res_cv2 = [crop[0], crop[2], crop[1], crop[3]]
        elif crop_fmt in {12, "12"}:
            res_cv2 = [crop[0], crop[0] + crop[2], crop[1], crop[1] + crop[3]]
        elif crop_fmt in {13, "13"}:
            res_cv2 = [crop[0], crop[1], crop[2], crop[3]]
        elif crop_fmt in {14, "14"}:
            res_cv2 = [crop[0], crop[0] + crop[1], crop[2], crop[2] + crop[3]]
        else:
            res_cv2 = [crop[1], crop[3], crop[0], crop[2]]
        res_pillow = [res_cv2[2], res_cv2[0], res_cv2[3], res_cv2[1]]
    except Exception:
        res_cv2 = []
        res_pillow = []

    return res_cv2, res_pillow


# ---------------------------------------------------------------------------
# ImageProcPythonCommand — matches original public interface exactly
# ---------------------------------------------------------------------------
class ImageProcPythonCommand(PythonCommand, ABC):
    __is_interface__ = True
    """PythonCommand with camera access and image processing capabilities."""

    template_path_name: ClassVar[str] = "./Template/"
    capture_path_name: ClassVar[str] = "./Captures/"

    def __init__(self, cam=None, gui=None) -> None:
        super().__init__()
        self._logger: Logger = getLogger(__name__)
        self._logger.addHandler(NullHandler())
        self._logger.setLevel(DEBUG)
        self._logger.propagate = True

        self.camera = cam
        self.gui = gui

    def get_filespec(self, filename: str, mode: str = "t") -> str:
        """Resolve file path for template/capture files."""
        if os.path.isabs(filename):
            return filename
        if mode == "c":
            return os.path.join(self.capture_path_name, filename)
        if mode == "t":
            return os.path.join(self.template_path_name, filename)
        return filename

    def setTemplateDir(self, path: str) -> None:
        """Set template search directory."""
        ImageProcPythonCommand.template_path_name = path

    def getCameraImage(
        self,
        crop_fmt: Any = "",
        crop: list[int] | None = None,
    ) -> Any:
        """Get camera frame, optionally cropped."""
        crop_cv2, _ = convertCv2Format(crop_fmt=crop_fmt, crop=crop)
        if self.camera is None:
            raise RuntimeError("Camera not initialized")
        src = self.camera.readFrame()
        try:
            from ImageProcessing import crop_image

            return crop_image(src, crop=crop_cv2)
        except ImportError:
            return src

    def openImage(self, filename: str, mode: str = "t") -> Any:
        """Load an image from disk."""
        try:
            from ImageProcessing import getImage

            return getImage(self.get_filespec(filename, mode=mode), mode="color")
        except ImportError:
            return None

    @pausedecorator
    def isContainTemplate(
        self,
        template_path: str,
        threshold: float = 0.7,
        use_gray: bool = True,
        show_value: bool = False,
        show_position: bool = True,
        show_only_true_rect: bool = True,
        ms: float = 2000,
        crop_fmt: Any = "",
        crop: list[int] | None = None,
        mask_path: str | None = None,
        use_gpu: bool = False,
        BGR_range: Any = None,
        threshold_binary: int | None = None,
        crop_template: list[int] | None = None,
        show_image: bool = False,
        color: list[str] | None = None,
    ) -> bool:
        """Template matching: search for template in camera frame."""
        from ImageProcessing import ImageProcessing, getImage

        crop_cv2, crop_pillow = convertCv2Format(crop_fmt=crop_fmt, crop=crop)
        crop_template_cv2, _ = convertCv2Format(crop_fmt=crop_fmt, crop=crop_template)

        if self.camera is None:
            raise RuntimeError("Camera not initialized")
        src = self.camera.readFrame()

        if isinstance(template_path, ImageProcessing.image_type):
            template_image = template_path
        else:
            template_image = getImage(
                self.get_filespec(template_path, mode="t"),
                mode="color",
            )

        if isinstance(mask_path, ImageProcessing.image_type):
            mask_image = mask_path
        else:
            mask_image = (
                getImage(self.get_filespec(mask_path, mode="t"), mode="binary")
                if mask_path is not None
                else None
            )

        if template_image is None:
            raise ValueError(
                f"template_path:{template_path}から画像を取得できませんでした。",
            )

        res, max_loc, width, height, max_val = ImageProcessing(
            use_gpu=use_gpu,
        ).isContainTemplate(
            src,
            template_image,
            mask_image=mask_image,
            threshold=threshold,
            use_gray=use_gray,
            crop=crop_cv2,
            BGR_range=BGR_range,
            threshold_binary=threshold_binary,
            crop_template=crop_template_cv2,
            show_image=show_image,
        )

        if show_value or self.isSimilarity:
            tm_mode = "NCC" if mask_path is not None else "ZNCC"
            print(f"{template_path} {tm_mode} value: {max_val}")

        if show_position:
            if color is None:
                color = ["blue", "red", "orange"]
            if crop_pillow != []:
                max_loc = list(max_loc)
                max_loc[0] += crop_pillow[0]
                max_loc[1] += crop_pillow[1]
            tag = str(time.perf_counter()) + str(random.random())
            if res:
                self.displayRectangle(
                    max_loc,
                    width,
                    height,
                    tag,
                    ms,
                    color=[color[0], color[2]],
                    crop=crop_pillow,
                )
            elif not show_only_true_rect:
                self.displayRectangle(
                    max_loc,
                    width,
                    height,
                    tag,
                    ms,
                    color=[color[1], color[2]],
                    crop=crop_pillow,
                )

        return res

    @pausedecorator
    def isContainTemplate_max(
        self,
        template_path_list: list[str],
        threshold: float = 0.7,
        use_gray: bool = True,
        show_value: bool = False,
        show_position: bool = True,
        show_only_true_rect: bool = True,
        ms: float = 2000,
        crop_fmt: Any = "",
        crop: list[int] | None = None,
        mask_path_list: list[str | None] | None = None,
        BGR_range: Any = None,
        threshold_binary: int | None = None,
        crop_template: list[int] | None = None,
        show_image: bool = False,
        color: list[str] | None = None,
    ) -> tuple[int, list[float], list[bool]]:
        """Multi-template matching: find best match among templates."""
        from ImageProcessing import ImageProcessing, getImage

        crop_cv2, crop_pillow = convertCv2Format(crop_fmt=crop_fmt, crop=crop)
        crop_template_cv2, _ = convertCv2Format(crop_fmt=crop_fmt, crop=crop_template)

        if self.camera is None:
            raise RuntimeError("Camera not initialized")
        src = self.camera.readFrame()

        template_image_list: list[Any] = []
        for i in template_path_list:
            if isinstance(i, ImageProcessing.image_type):
                template_image_list.append(i)
            else:
                image = getImage(self.get_filespec(i, mode="t"), mode="color")
                if image is not None:
                    template_image_list.append(image)
                else:
                    raise ValueError(
                        f"template_path:{i}から画像を取得できませんでした。"
                    )

        mask_image_list: list[Any] = []
        if mask_path_list is not None:
            for i in mask_path_list:
                if isinstance(i, ImageProcessing.image_type):
                    mask_image_list.append(i)
                elif i is None:
                    mask_image_list.append(None)
                else:
                    mask_image_list.append(
                        getImage(self.get_filespec(i, mode="t"), mode="binary"),
                    )

        max_idx, max_val_list, max_loc_list, width_list, height_list, judge_list = (
            ImageProcessing(use_gpu=False).isContainTemplate_max(
                src,
                template_image_list,
                mask_image_list=mask_image_list,
                threshold=threshold,
                use_gray=use_gray,
                crop=crop_cv2,
                BGR_range=BGR_range,
                threshold_binary=threshold_binary,
                crop_template=crop_template_cv2,
                show_image=show_image,
            )
        )

        if show_value or self.isSimilarity:
            tm_mode = (
                "ZNCC" if (mask_path_list == [] or mask_path_list is None) else "NCC"
            )
            for tp, mv in zip(template_path_list, max_val_list, strict=False):
                print(f"{tp} {tm_mode} value: {mv}")

        if show_position:
            if color is None:
                color = ["blue", "red", "orange"]
            if crop_pillow != []:
                max_loc = (
                    max_loc_list[max_idx][0] + crop_pillow[0],
                    max_loc_list[max_idx][1] + crop_pillow[1],
                )
            else:
                max_loc = max_loc_list[max_idx]
            tag = str(time.perf_counter()) + str(random.random())
            if True in judge_list:
                self.displayRectangle(
                    max_loc,
                    width_list[max_idx],
                    height_list[max_idx],
                    tag,
                    ms,
                    color=[color[0], color[2]],
                    crop=crop_pillow,
                )
            elif not show_only_true_rect:
                self.displayRectangle(
                    max_loc_list[max_idx],
                    width_list[max_idx],
                    height_list[max_idx],
                    tag,
                    ms,
                    color=[color[1], color[2]],
                    crop=crop_pillow,
                )

        return max_idx, max_val_list, judge_list

    @pausedecorator
    def isContainTemplateGPU(
        self,
        template_path: str,
        threshold: float = 0.7,
        use_gray: bool = True,
        show_value: bool = False,
        show_position: bool = True,
        show_only_true_rect: bool = True,
        ms: float = 2000,
        crop_fmt: Any = "",
        crop: list[int] | None = None,
        mask_path: str | None = None,
        BGR_range: Any = None,
        threshold_binary: int | None = None,
        crop_template: list[int] | None = None,
        show_image: bool = False,
        color: list[str] | None = None,
    ) -> bool:
        return self.isContainTemplate(
            template_path,
            threshold=threshold,
            use_gray=use_gray,
            show_value=show_value,
            show_position=show_position,
            show_only_true_rect=show_only_true_rect,
            ms=ms,
            crop_fmt=crop_fmt,
            crop=crop,
            mask_path=mask_path,
            use_gpu=True,
            BGR_range=BGR_range,
            threshold_binary=threshold_binary,
            crop_template=crop_template,
            show_image=show_image,
            color=color,
        )

    @pausedecorator
    def isContainedImage(
        self,
        image_path: str,
        threshold: float = 0.7,
        use_gray: bool = True,
        show_value: bool = False,
        show_position: bool = True,
        show_only_true_rect: bool = True,
        ms: float = 2000,
        crop_fmt: Any = "",
        crop: list[int] | None = None,
        mask_path: str | None = None,
        use_gpu: bool = False,
        BGR_range: Any = None,
        threshold_binary: int | None = None,
        crop_template: list[int] | None = None,
        show_image: bool = False,
        color: list[str] | None = None,
    ) -> bool:
        """Check if camera frame is contained in a reference image."""
        from ImageProcessing import ImageProcessing, getImage

        crop_cv2, crop_pillow = convertCv2Format(crop_fmt=crop_fmt, crop=crop)
        crop_template_cv2, crop_template_pillow = convertCv2Format(
            crop_fmt=crop_fmt,
            crop=crop_template,
        )

        if self.camera is None:
            raise RuntimeError("Camera not initialized")
        template_image = self.camera.readFrame()

        if isinstance(image_path, ImageProcessing.image_type):
            image = image_path
        else:
            image = getImage(self.get_filespec(image_path, mode="t"), mode="color")

        if isinstance(mask_path, ImageProcessing.image_type):
            mask_image = mask_path
        else:
            mask_image = (
                getImage(self.get_filespec(mask_path, mode="t"), mode="binary")
                if mask_path is not None
                else None
            )

        if image is None:
            raise ValueError(f"image_path:{image_path}から画像を取得できませんでした。")

        res, _, width, height, max_val = ImageProcessing(
            use_gpu=use_gpu,
        ).isContainTemplate(
            image,
            template_image,
            mask_image=mask_image,
            threshold=threshold,
            use_gray=use_gray,
            crop=crop_cv2,
            BGR_range=BGR_range,
            threshold_binary=threshold_binary,
            crop_template=crop_template_cv2,
            show_image=show_image,
        )

        if show_value or self.isSimilarity:
            tm_mode = "NCC" if mask_path is not None else "ZNCC"
            print(f"capture_image {tm_mode} value: {max_val}")

        if show_position:
            if color is None:
                color = ["blue", "red", "orange"]
            tag = str(time.perf_counter()) + str(random.random())
            if res:
                self.displayRectangle(
                    crop_template_pillow[0:2],
                    width,
                    height,
                    tag,
                    ms,
                    color=[color[0], color[2]],
                    crop=[],
                )
            elif not show_only_true_rect:
                self.displayRectangle(
                    crop_template_pillow[0:2],
                    width,
                    height,
                    tag,
                    ms,
                    color=[color[1], color[2]],
                    crop=[],
                )

        return res

    def displayRectangle(
        self,
        max_loc: list[int] | Sequence[int],
        width: int,
        height: int,
        tag: str | None = None,
        ms: float = 2000,
        color: list[str] | None = None,
        crop_fmt: Any = "",
        crop: list[int] | None = None,
    ) -> None:
        """Display rectangle on the GUI canvas."""
        if color is None:
            color = ["blue", "orange"]

        _, crop_pillow = convertCv2Format(crop_fmt=crop_fmt, crop=crop)

        top_left = max_loc
        if len(top_left) != 2:
            raise ValueError(f"max_loc:{max_loc}の要素数が不正です。")
        bottom_right = (top_left[0] + width + 1, top_left[1] + height + 1)

        if self.gui is not None:
            canvas = self.gui
        elif self.canvas is not None:
            canvas = self.canvas
        else:
            raise ValueError("self.guiとself.canvasがどちらもNoneです。")

        if tag is None:
            tag = generateRandomCharacter(10)

        if self.gui is not None or self.isGuide:
            if crop_pillow != [] and hasattr(canvas, "ImgRect"):
                canvas.ImgRect(
                    *crop_pillow[0:2],
                    *crop_pillow[2:4],
                    outline=color[1],
                    tag=tag,
                    ms=int(ms),
                    flag=False,
                )
            if hasattr(canvas, "ImgRect"):
                canvas.ImgRect(
                    top_left[0],
                    top_left[1],
                    bottom_right[0],
                    bottom_right[1],
                    outline=color[0],
                    tag=tag,
                    ms=int(ms),
                )

    def displayText(
        self,
        position: Sequence[int],
        txt: str,
        tag: str | None = None,
        ms: int = 2000,
        font: str = "UD デジタル 教科書体 NP-B",
        fontsize: int = 20,
        color: str = "black",
    ) -> None:
        """Display text on the GUI canvas."""
        if self.gui is not None:
            canvas = self.gui
        elif self.canvas is not None:
            canvas = self.canvas
        else:
            raise ValueError("self.guiとself.canvasがどちらもNoneです。")

        ft = (font, fontsize)
        if tag is None:
            tag = generateRandomCharacter(10)

        if self.gui is not None or self.isGuide:
            if hasattr(canvas, "ImgText"):
                canvas.ImgText(
                    position[0],
                    position[1],
                    txt=txt,
                    tag=tag,
                    ms=int(ms),
                    ft=ft,
                    color=color,
                )

    def saveCapture(
        self,
        filename: str | None = None,
        crop_fmt: Any = "",
        crop: list[int] | None = None,
        mode: bool = True,
    ) -> None:
        """Capture and save the current camera frame."""
        from ImageProcessing import ImageProcessing

        crop_cv2, _ = convertCv2Format(crop_fmt=crop_fmt, crop=crop)
        if self.camera is None:
            raise RuntimeError("Camera not initialized")
        src = self.camera.readFrame()

        if filename is None or filename == "":
            dt_now = datetime.datetime.now()
            filename = dt_now.strftime("%Y-%m-%d_%H-%M-%S") + ".png"
        else:
            filename = filename + ".png"

        if mode:
            save_path = self.get_filespec(filename, mode="c")
        else:
            save_path = self.get_filespec(filename, mode="n")

        ImageProcessing().saveImage(src, filename=save_path, crop=crop_cv2)

    def popupImage(
        self,
        crop_fmt: Any = "",
        crop: list[int] | None = None,
        title: str = "image",
    ) -> None:
        """Display camera frame in a popup window."""
        from ImageProcessing import opneImage

        crop_cv2, _ = convertCv2Format(crop_fmt=crop_fmt, crop=crop)
        if self.camera is None:
            raise RuntimeError("Camera not initialized")
        src = self.camera.readFrame()
        opneImage(src, crop=crop_cv2, title=title)

    def LINE_image(
        self,
        txt: str = "",
        crop_fmt: Any = "",
        crop: list[int] | None = None,
        token: str = "",
    ) -> None:
        """Send LINE notification with image (currently disabled)."""
        print("LINE通知は使用出来ません。")
        print("Discord通知への切り替えを検討してください。")

    def discord_image(
        self,
        content: str = "",
        index: int = 0,
        crop_fmt: Any = "",
        crop: list[int] | None = None,
        keys: Any = "DISCORD_WEBHOOK",
    ) -> None:
        """Send Discord notification with image."""
        from ImageProcessing import crop_image

        crop_cv2, _ = convertCv2Format(crop_fmt=crop_fmt, crop=crop)
        if self.camera is None:
            raise RuntimeError("Camera not initialized")
        src = self.camera.readFrame()
        cropped_image = crop_image(src, crop=crop_cv2)

        if index != 0 and keys == "DISCORD_WEBHOOK":
            keys = f"DISCORD_WEBHOOK{index}"
        elif index == 0 and keys != "DISCORD_WEBHOOK":
            pass
        elif index != 0 and keys != "DISCORD_WEBHOOK":
            keys = f"DISCORD_WEBHOOK{index}"

        if self.Discord is not None and hasattr(self.Discord, "send_message"):
            with contextlib.suppress(Exception):
                self.Discord.send_message(
                    notification_message=content,
                    image=cropped_image,
                    keys=keys,
                )


# ---------------------------------------------------------------------------
# Register default v1 interface implementations
# ---------------------------------------------------------------------------
# The PythonCommand and ImageProcPythonCommand classes themselves act as
# the v1 implementation.  When a user writes::
#
#     class MyCommand(PythonCommand):
#         def do(self): ...
#
# CommandMeta intercepts and sets the MRO to:
#     [MyCommand, _RustCoreAdapter, PythonCommand, …]
#
# Future API versions (v2, custom) will use different impl classes, but
# the existing PythonCommand class will remain available as the v1 impl.
CommandMeta.register_interface("PythonCommand", "v1", PythonCommand)
CommandMeta.register_interface("ImageProcPythonCommand", "v1", ImageProcPythonCommand)
