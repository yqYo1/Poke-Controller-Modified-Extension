# -*- coding: utf-8 -*-
from __future__ import annotations

from logging import getLogger  # , Formatter, handlers, StreamHandler, DEBUG
from time import sleep
from typing import TYPE_CHECKING

import numpy as np

from . import CommandBase
from .Keys import Direction, KeyPress, Stick

if TYPE_CHECKING:
    from collections.abc import Callable
    from logging import Logger
    from typing import Final

    from Keys import GamepadInput
    from Sender import Sender

# Single button command


class StickCommand(CommandBase.Command):
    def __init__(self) -> None:
        super().__init__()
        self.key: KeyPress | None = None
        self.isRunning: bool
        self._logger: Logger = getLogger(__name__)

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        self.isRunning = True
        self.key = KeyPress(ser)

    def end(self, ser: Sender) -> None:
        self.isRunning = False
        self.key = KeyPress(ser)

    # do nothing at wait time(s)
    def wait(self, wait: float) -> None:
        sleep(wait)

    def press(self, btn: GamepadInput) -> None:
        if self.key is not None:
            self.key.input(btn)
            self.wait(0.1)
            self.key.inputEnd(btn)
            self.isRunning = False
            self.key = None

    # press button at duration times(s)
    def stick(self, stick: Direction, duration: float = 0.015, wait: float = 0) -> None:
        if self.key is not None:
            self.key.input(stick, ifPrint=False)
            # print(buttons)
            self.wait(duration)
            self.wait(wait)

    def stick_end(self, stick: Direction | None = None) -> None:
        if stick is None:
            stick = Direction(Stick.LEFT, 0)
        if self.key is not None:
            self.key.inputEnd(stick)


class StickLeft(StickCommand):
    def __init__(self, ser: Sender) -> None:
        super().__init__()
        self.ser: Final[Sender] = ser
        self.key: KeyPress | None = None
        self._logger: Logger = getLogger(__name__)

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        super().start(ser)
        self.key = KeyPress(ser)
        self._logger.debug("Start RightStick Serial Connection")

    def LStick(self, angle: float, r: float = 1.0, duration: float = 0.015) -> None:  # noqa: ARG002
        self.ser.writeRow(
            f"3 8 {hex(int(128 + r * 127.5 * np.cos(np.deg2rad(angle))))} {hex(int(128 - r * 127.5 * np.sin(np.deg2rad(angle))))} 80 80",
            is_show=False,
        )

    def end(self, ser: Sender) -> None:
        super().end(ser)
        self.stick_end(stick=Direction(Stick.LEFT, 0))


class StickRight(StickCommand):
    def __init__(self) -> None:
        super().__init__()
        self.key: KeyPress | None = None
        self._logger: Logger = getLogger(__name__)

    # def start(self, ser):
    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        super().start(ser)
        self.key = KeyPress(ser)
        self._logger.debug("Start RightStick Serial Connection")

    def RStick(self, angle: float, r: float = 1.0, duration: float = 0.015) -> None:  # noqa: ARG002
        if self.key is not None:
            self.key.ser.writeRow(
                f"3 8 80 80 {hex(int(128 + r * 127.5 * np.cos(np.deg2rad(angle))))} {hex(int(128 - r * 127.5 * np.sin(np.deg2rad(angle))))}",
                is_show=False,
            )

    def end(self, ser: Sender) -> None:
        super().end(ser)
        self.stick_end(stick=Direction(Stick.RIGHT, 0))
