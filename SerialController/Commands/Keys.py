# -*- coding: utf-8 -*-
from __future__ import annotations

import math
import queue
import time
from collections import OrderedDict
from enum import Enum, IntEnum, IntFlag, auto
from logging import DEBUG, NullHandler, getLogger
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from logging import Logger
    from typing import Final

    from Commands.Sender import Sender

    type Buttons = Button | Hat | Stick | Direction | Touchscreen
    type ButtonsList = list[Buttons]
    type GamepadInput = ButtonsList | Buttons


class Button(IntFlag):
    Y = auto()  # 1
    B = auto()  # 2
    A = auto()  # 3
    X = auto()  # 4
    L = auto()  # 5
    R = auto()  # 6
    ZL = auto()  # 7
    ZR = auto()  # 8
    MINUS = auto()  # 9
    PLUS = auto()  # 10
    LCLICK = auto()  # 11
    RCLICK = auto()  # 12
    HOME = auto()  # 13
    CAPTURE = auto()  # 14
    SELECT = MINUS  # for 3DS, 9
    START = PLUS  # for 3DS, 10
    POWER = LCLICK  # for 3DS, 11
    WIRELESS = RCLICK  # for 3DS, 12


# 3DS Controller用にビット位置を並び替えるためのdict
conversion_default_button: dict[Button, Button] = {
    Button.Y: Button.Y,
    Button.B: Button.B,
    Button.A: Button.A,
    Button.X: Button.X,
    Button.L: Button.L,
    Button.R: Button.R,
    Button.ZL: Button.ZL,
    Button.ZR: Button.ZR,
    Button.MINUS: Button.MINUS,
    Button.PLUS: Button.PLUS,
    Button.LCLICK: Button.LCLICK,
    Button.RCLICK: Button.RCLICK,
    Button.HOME: Button.HOME,
    Button.CAPTURE: Button.CAPTURE,
    Button.SELECT: Button.SELECT,
    Button.START: Button.START,
    Button.POWER: Button.POWER,
    Button.WIRELESS: Button.WIRELESS,
}

conversion_3ds_controller_button: dict[Button, int] = {
    Button.A: 1,
    Button.B: 2,
    Button.X: 4,
    Button.Y: 8,
    Button.L: 16,
    Button.R: 32,
    Button.HOME: 64,
    Button.START: 128,
    Button.SELECT: 256,
    Button.POWER: 512,
    Button.MINUS: 256,
    Button.PLUS: 128,
    Button.LCLICK: 512,
    Button.RCLICK: 0,
    Button.ZL: 0,
    Button.ZR: 0,
    Button.CAPTURE: 0,
    Button.WIRELESS: 0,
}


class Hat(IntEnum):
    TOP = 0  # 8
    TOP_RIGHT = 1
    RIGHT = 2  # 4
    BTM_RIGHT = 3
    BTM = 4  # 2
    BTM_LEFT = 5
    LEFT = 6  # 1
    TOP_LEFT = 7
    CENTER = 8  # 0


convert_hat_default = list(range(9))
convert_hat_3ds_controller = [8, 0, 4, 0, 2, 0, 1, 0, 0]


class Stick(Enum):
    LEFT = auto()
    RIGHT = auto()


class Tilt(Enum):
    UP = auto()
    RIGHT = auto()
    DOWN = auto()
    LEFT = auto()
    R_UP = auto()
    R_RIGHT = auto()
    R_DOWN = auto()
    R_LEFT = auto()


# direction value definitions
direction_min = 0
direction_center = 128
direction_max = 255


# serial format
class SendFormat:
    def __init__(self) -> None:
        self._logger: Final[Logger] = getLogger(__name__)
        self._logger.addHandler(NullHandler())
        self._logger.setLevel(DEBUG)
        self._logger.propagate = True

        # This format structure needs to be the same as the one written in Joystick.c
        self.format: OrderedDict[str, int] = OrderedDict(
            [
                ("btn", 0),  # send bit array for buttons
                ("hat", Hat.CENTER),
                ("lx", direction_center),
                ("ly", direction_center),
                ("rx", direction_center),
                ("ry", direction_center),
                ("sx", 0),
                ("sy", 0),
            ],
        )

        self.L_stick_changed: bool = False
        self.R_stick_changed: bool = False
        self.Hat_pos: Hat = Hat.CENTER

    def setButton(
        self,
        btns: list[Button],
        convert: dict[Button, Button] | dict[Button, int] = conversion_default_button,
    ) -> None:
        for btn in btns:
            self.format["btn"] |= convert[btn]

    def unsetButton(
        self,
        btns: list[Button],
        convert: dict[Button, Button] | dict[Button, int] = conversion_default_button,
    ) -> None:
        for btn in btns:
            self.format["btn"] &= ~convert[btn]

    def resetAllButtons(self) -> None:
        self.format["btn"] = 0

    def setHat(self, btns, convert: list[int] = convert_hat_default) -> None:
        # self._logger.debug(btns)
        if not btns:
            self.format["hat"] = self.Hat_pos
        else:
            self.Hat_pos = convert[btns[0]]
            self.format["hat"] = convert[btns[0]]  # takes only first element

    def unsetHat(self, convert=convert_hat_default) -> None:
        # if self.Hat_pos is not Hat.CENTER:
        self.Hat_pos = convert[Hat.CENTER]
        self.format["hat"] = self.Hat_pos

    def setAnyDirection(self, dirs, x_reverse=False, y_reverse=False) -> None:
        for dir in dirs:
            if dir.stick == Stick.LEFT:
                if self.format["lx"] != dir.x or self.format["ly"] != 255 - dir.y:
                    self.L_stick_changed = True

                self.format["lx"] = dir.x if not x_reverse else 255 - dir.x
                self.format["ly"] = (
                    255 - dir.y if not y_reverse else dir.y
                )  # NOTE: y axis directs under
            elif dir.stick == Stick.RIGHT:
                if self.format["rx"] != dir.x or self.format["ry"] != 255 - dir.y:
                    self.R_stick_changed = True

                self.format["rx"] = dir.x if not x_reverse else 255 - dir.x
                self.format["ry"] = 255 - dir.y if not y_reverse else dir.y

    def unsetDirection(self, dirs) -> None:
        if Tilt.UP in dirs or Tilt.DOWN in dirs:
            self.format["ly"] = direction_center
            self.format["lx"] = self.fixOtherAxis(self.format["lx"])
            self.L_stick_changed = True
        if Tilt.RIGHT in dirs or Tilt.LEFT in dirs:
            self.format["lx"] = direction_center
            self.format["ly"] = self.fixOtherAxis(self.format["ly"])
            self.L_stick_changed = True
        if Tilt.R_UP in dirs or Tilt.R_DOWN in dirs:
            self.format["ry"] = direction_center
            self.format["rx"] = self.fixOtherAxis(self.format["rx"])
            self.R_stick_changed = True
        if Tilt.R_RIGHT in dirs or Tilt.R_LEFT in dirs:
            self.format["rx"] = direction_center
            self.format["ry"] = self.fixOtherAxis(self.format["ry"])
            self.R_stick_changed = True

    # Use this to fix an either tilt to max when the other axis sets to 0
    def fixOtherAxis(self, fix_target):
        if fix_target == direction_center:
            return direction_center
        return 0 if fix_target < direction_center else 255

    def resetAllDirections(self) -> None:
        self.format["lx"] = direction_center
        self.format["ly"] = direction_center
        self.format["rx"] = direction_center
        self.format["ry"] = direction_center
        self.L_stick_changed = True
        self.R_stick_changed = True
        self.Hat_pos = Hat.CENTER

    def setTouchscreen(self, dirs) -> None:
        if not dirs:
            pass
        else:
            self.format["sx"] = dirs[0].x  # takes only first element
            self.format["sy"] = dirs[0].y  # takes only first element

    def unsetTouchscreen(self) -> None:
        self.format["sx"] = 0
        self.format["sy"] = 0

    def convert2str(self) -> str:
        str_format = ""
        str_L = ""
        str_R = ""
        str_Hat = ""
        space = " "

        # set bits array with stick flags
        send_btn = int(self.format["btn"]) << 2
        # send_btn |= 0x3
        if self.L_stick_changed:
            send_btn |= 0x2
            str_L = (
                format(self.format["lx"], "x") + space + format(self.format["ly"], "x")
            )
        if self.R_stick_changed:
            send_btn |= 0x1
            str_R = (
                format(self.format["rx"], "x") + space + format(self.format["ry"], "x")
            )
        # if self.Hat_changed:
        str_Hat = str(int(self.format["hat"]))
        # format(send_btn, 'x') + \
        # print(hex(send_btn))
        str_format = (
            format(send_btn, "#06x")
            + (space + str_Hat)
            + (space + str_L if self.L_stick_changed else "")
            + (space + str_R if self.R_stick_changed else "")
        )

        self.L_stick_changed = False
        self.R_stick_changed = False

        # print(str_format)
        return str_format  # the last space is not needed

    def convert2list(self) -> list[int]:
        """
        For Qingpi
        """
        header = 0xAB  # fixed value
        send_btn = int(self.format["btn"])
        send_hat = int(self.format["hat"])
        send_lstick_x = self.format["lx"]
        send_lstick_y = self.format["ly"]
        send_touch_x = int(self.format["sx"])
        send_touch_y = int(self.format["sy"])

        return [
            header,
            send_btn & 0xFF,
            (send_btn >> 8) & 0xFF,
            send_hat,
            send_lstick_x,
            send_lstick_y,
            direction_center,
            direction_center,
            send_touch_x & 0xFF,
            (send_touch_x >> 8) & 0xFF,
            send_touch_y,
        ]

    def convert2list2(self) -> list[int]:
        """
        For 3DS Controller
        """
        header = 0xA1  # fixed value
        send_btn = int(self.format["btn"])
        send_hat = convert_hat_3ds_controller[int(self.format["hat"])]

        header2 = 0xA2  # fixed value
        send_lx = (
            self.format["lx"] if self.format["lx"] >= 128 else 127 - self.format["lx"]
        )
        send_ly = (
            self.format["ly"] if self.format["ly"] >= 128 else 127 - self.format["ly"]
        )

        return [
            header,
            ((send_btn & 0xF) << 4) | send_hat,
            (send_btn >> 4) & 0x3F,
            header2,
            send_lx,
            send_ly,
        ]


# This class handle L stick and R stick at any angles
class Direction:
    UP: Direction | None = None
    RIGHT: Direction | None = None
    DOWN: Direction | None = None
    LEFT: Direction | None = None
    UP_RIGHT: Direction | None = None
    DOWN_RIGHT: Direction | None = None
    DOWN_LEFT: Direction | None = None
    UP_LEFT: Direction | None = None
    R_UP: Direction | None = None
    R_RIGHT: Direction | None = None
    R_DOWN: Direction | None = None
    R_LEFT: Direction | None = None
    R_UP_RIGHT: Direction | None = None
    R_DOWN_RIGHT: Direction | None = None
    R_DOWN_LEFT: Direction | None = None
    R_UP_LEFT: Direction | None = None

    def __init__(
        self,
        stick: Stick,
        angle: tuple[int, int] | float,
        magnification: float = 1.0,
        isDegree: bool = True,
        showName: str | None = None,
    ) -> None:
        self._logger: Logger = getLogger(__name__)
        self._logger.addHandler(NullHandler())
        self._logger.setLevel(DEBUG)
        self._logger.propagate = True

        self.stick: Final = stick
        self.angle_for_show: Final = angle
        self.showName: str | None = showName
        if magnification > 1.0:
            self.mag: float = 1.0
        elif magnification < 0:
            self.mag = 0.0
        else:
            self.mag = magnification

        if isinstance(angle, tuple):
            # assuming (X, Y)
            self.x: int = angle[0]
            self.y: int = angle[1]
            self.showName = "(" + str(self.x) + ", " + str(self.y) + ")"
            print("押し込み量", self.showName)
        else:
            angle = math.radians(angle) if isDegree else angle

            # We set stick X and Y from 0 to 255, so they are calculated as below.
            # X = 127.5*cos(theta) + 127.5
            # Y = 127.5*sin(theta) + 127.5
            self.x = math.ceil(127.5 * math.cos(angle) * self.mag + 127.5)
            self.y = math.floor(127.5 * math.sin(angle) * self.mag + 127.5)

    def __repr__(self) -> str:
        if self.showName:
            return f"<{self.stick}, {self.showName}>"
        return f"<{self.stick}, {self.angle_for_show}[deg]>"

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, Direction):
            return False

        return bool(
            self.stick == other.stick and self.angle_for_show == other.angle_for_show,
        )

    def getTilting(self) -> list[Tilt]:
        tilting: list[Tilt] = []
        if self.stick == Stick.LEFT:
            if self.x < direction_center:
                tilting.append(Tilt.LEFT)
            elif self.x > direction_center:
                tilting.append(Tilt.RIGHT)

            if self.y < direction_center - 1:
                tilting.append(Tilt.DOWN)
            elif self.y > direction_center - 1:
                tilting.append(Tilt.UP)
        elif self.stick == Stick.RIGHT:
            if self.x < direction_center:
                tilting.append(Tilt.R_LEFT)
            elif self.x > direction_center:
                tilting.append(Tilt.R_RIGHT)

            if self.y < direction_center - 1:
                tilting.append(Tilt.R_DOWN)
            elif self.y > direction_center - 1:
                tilting.append(Tilt.R_UP)
        return tilting


NEUTRAL = (128, 127)
"""
スティックが中心にあることを表します。
丸め誤差の関係で"80 80"になるのは`(128, 127)`です。
"""

# Left stick for ease of use
Direction.UP = Direction(Stick.LEFT, 90, showName="UP")
Direction.RIGHT = Direction(Stick.LEFT, 0, showName="RIGHT")
Direction.DOWN = Direction(Stick.LEFT, -90, showName="DOWN")
Direction.LEFT = Direction(Stick.LEFT, -180, showName="LEFT")
Direction.UP_RIGHT = Direction(Stick.LEFT, 45, showName="UP_RIGHT")
Direction.DOWN_RIGHT = Direction(Stick.LEFT, -45, showName="DOWN_RIGHT")
Direction.DOWN_LEFT = Direction(Stick.LEFT, -135, showName="DOWN_LEFT")
Direction.UP_LEFT = Direction(Stick.LEFT, 135, showName="UP_LEFT")
# Right stick for ease of use
Direction.R_UP = Direction(Stick.RIGHT, 90, showName="UP")
Direction.R_RIGHT = Direction(Stick.RIGHT, 0, showName="RIGHT")
Direction.R_DOWN = Direction(Stick.RIGHT, -90, showName="DOWN")
Direction.R_LEFT = Direction(Stick.RIGHT, -180, showName="LEFT")
Direction.R_UP_RIGHT = Direction(Stick.RIGHT, 45, showName="UP_RIGHT")
Direction.R_DOWN_RIGHT = Direction(Stick.RIGHT, -45, showName="DOWN_RIGHT")
Direction.R_DOWN_LEFT = Direction(Stick.RIGHT, -135, showName="DOWN_LEFT")
Direction.R_UP_LEFT = Direction(Stick.RIGHT, 135, showName="UP_LEFT")


class Touchscreen:
    def __init__(self, x, y) -> None:
        self._logger = getLogger(__name__)
        self._logger.addHandler(NullHandler())
        self._logger.setLevel(DEBUG)
        self._logger.propagate = True

        self.x = x
        self.y = y


# handles serial input to Joystick.c


class KeyPress:
    serial_data_format_name = "Default"

    def __init__(self, ser: Sender) -> None:
        self._logger = getLogger(__name__)
        self._logger.addHandler(NullHandler())
        self._logger.setLevel(DEBUG)
        self._logger.propagate = True

        self.q = queue.Queue()
        self.ser: Sender = ser
        self.format = SendFormat()
        self.holdButton = []
        self.btn_name2 = [
            "LEFT",
            "RIGHT",
            "UP",
            "DOWN",
            "UP_LEFT",
            "UP_RIGHT",
            "DOWN_LEFT",
            "DOWN_RIGHT",
        ]

        self.pushing_to_show = None
        self.pushing = None
        self.pushing2 = None
        self._pushing = None
        self._chk_neutral = None
        self.NEUTRAL = dict(self.format.format)

        self.input_time_0 = time.perf_counter()
        self.input_time_1 = time.perf_counter()
        self.inputEnd_time_0 = time.perf_counter()
        self.was_neutral = True

    def init_hat(self) -> None:
        pass

    def input(self, btns: GamepadInput, ifPrint: bool = True) -> None:
        self._pushing = dict(self.format.format)
        if not isinstance(btns, list):
            btns = [btns]

        for btn in self.holdButton:
            if btn not in btns:
                btns.append(btn)
        if self.serial_data_format_name == "3DS Controller":
            self.format.setButton(
                [btn for btn in btns if type(btn) is Button],
                convert=conversion_3ds_controller_button,
            )
            self.format.setHat([btn for btn in btns if type(btn) is Hat])
            self.format.setAnyDirection([btn for btn in btns if type(btn) is Direction])
            self.ser.writeList(self.format.convert2list2())
        else:
            self.format.setButton([btn for btn in btns if type(btn) is Button])
            self.format.setHat([btn for btn in btns if type(btn) is Hat])
            self.format.setAnyDirection([btn for btn in btns if type(btn) is Direction])
            if self.serial_data_format_name == "Qingpi":
                self.format.setTouchscreen(
                    [btn for btn in btns if type(btn) is Touchscreen],
                )
                self.ser.writeList(self.format.convert2list())
            else:
                self.ser.writeRow(self.format.convert2str())
        self.input_time_0 = time.perf_counter()

        # self._logger.debug(f": {list(map(str,self.format.format.values()))}")

    def inputEnd(
        self,
        btns: GamepadInput,
        ifPrint: bool = True,
        unset_hat: bool = True,
        unset_Touchscreen: bool = True,
    ) -> None:
        # self._logger.debug(f"input end: {btns}")
        self.pushing2 = dict(self.format.format)

        self.ed = time.perf_counter()
        if not isinstance(btns, list):
            btns = [btns]
        # self._logger.debug(btns)

        # get tilting direction from angles
        tilts = []
        for dir in [btn for btn in btns if type(btn) is Direction]:
            tiltings = dir.getTilting()
            for tilting in tiltings:
                tilts.append(tilting)
        # self._logger.debug(tilts)

        if self.serial_data_format_name == "3DS Controller":
            self.format.unsetButton(
                [btn for btn in btns if type(btn) is Button],
                convert=conversion_3ds_controller_button,
            )
            if unset_hat:
                self.format.unsetHat()
            self.format.unsetDirection(tilts)
            self.ser.writeList(self.format.convert2list2())
        else:
            self.format.unsetButton([btn for btn in btns if type(btn) is Button])
            if unset_hat:
                self.format.unsetHat()
            self.format.unsetDirection(tilts)
            if self.serial_data_format_name == "Qingpi":
                if unset_Touchscreen or (
                    True in [btn for btn in btns if type(btn) is Touchscreen]
                ):
                    self.format.unsetTouchscreen()
                self.ser.writeList(self.format.convert2list())
            else:
                self.ser.writeRow(self.format.convert2str())

    def hold(self, btns: GamepadInput) -> None:
        if not isinstance(btns, list):
            btns = [btns]

        flag_isTouchscreen = False
        for btn in btns:
            if type(btn) is Touchscreen:
                flag_isTouchscreen = True
        if flag_isTouchscreen:
            for btn in self.holdButton:
                if type(btn) is Touchscreen:
                    self.holdButton.remove(btn)
        for btn in btns:
            if btn in self.holdButton:
                print("Warning: " + btn.name + " is already in holding state")
                self._logger.warning(f"Warning: {btn.name} is already in holding state")
                return

            self.holdButton.append(btn)
        self.input(btns)

    def holdEnd(self, btns: GamepadInput) -> None:
        if not isinstance(btns, list):
            btns = [btns]

        flag_isTouchscreen = False
        for btn in btns:
            if type(btn) is not Touchscreen:
                self.holdButton.remove(btn)
            else:
                flag_isTouchscreen = True
        if flag_isTouchscreen:
            for btn in self.holdButton:
                if type(btn) is Touchscreen:
                    self.holdButton.remove(btn)

        self.inputEnd(btns)

    def neutral(self) -> None:
        btns = self.holdButton
        self.holdButton = []
        self.inputEnd(btns, unset_hat=True, unset_Touchscreen=True)

    def end(self) -> None:
        if self.serial_data_format_name in ["Qingpi", "3DS Controller"]:
            pass
        else:
            self.ser.writeRow("end")

    def serialcommand_direct_send(
        self,
        serialcommands: list[str],
        waittime: list[float],
    ) -> None:
        for wtime, row in zip(waittime, serialcommands, strict=False):
            time.sleep(wtime)
            self.ser.writeRow_wo_perf_counter(row, is_show=False)
