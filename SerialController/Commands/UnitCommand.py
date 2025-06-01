# -*- coding: utf-8 -*-
from __future__ import annotations

from time import sleep
from typing import TYPE_CHECKING

from Commands import CommandBase
from Commands.Keys import Button, Hat, KeyPress

if TYPE_CHECKING:
    from collections.abc import Callable

    from Commands.Keys import Buttons
    from Commands.Sender import Sender


# Sigle button command
class UnitCommand(CommandBase.Command):
    def __init__(self) -> None:
        super().__init__()

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        self.isRunning: bool = True
        self.key: KeyPress = KeyPress(ser)

    def end(self, ser: Sender) -> None:
        pass

    # do nothing at wait time(s)
    def wait(self, wait: float) -> None:
        sleep(wait)

    def press(self, btn: Buttons) -> None:
        self.key.input([btn])
        self.wait(0.1)
        self.key.inputEnd([btn])
        self.isRunning = False
        # self.key = None


class A(UnitCommand):
    def __init__(self) -> None:
        super().__init__()

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        super().start(ser)
        self.press(Button.A)


class B(UnitCommand):
    def __init__(self) -> None:
        super().__init__()

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        super().start(ser)
        self.press(Button.B)


class X(UnitCommand):
    def __init__(self) -> None:
        super().__init__()

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        super().start(ser)
        self.press(Button.X)


class Y(UnitCommand):
    def __init__(self) -> None:
        super().__init__()

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        super().start(ser)
        self.press(Button.Y)


class L(UnitCommand):
    def __init__(self) -> None:
        super().__init__()

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        super().start(ser)
        self.press(Button.L)


class R(UnitCommand):
    def __init__(self) -> None:
        super().__init__()

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        super().start(ser)
        self.press(Button.R)


class ZL(UnitCommand):
    def __init__(self) -> None:
        super().__init__()

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        super().start(ser)
        self.press(Button.ZL)


class ZR(UnitCommand):
    def __init__(self) -> None:
        super().__init__()

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        super().start(ser)
        self.press(Button.ZR)


class MINUS(UnitCommand):
    def __init__(self) -> None:
        super().__init__()

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        super().start(ser)
        self.press(Button.MINUS)


class PLUS(UnitCommand):
    def __init__(self) -> None:
        super().__init__()

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        super().start(ser)
        self.press(Button.PLUS)


class LCLICK(UnitCommand):
    def __init__(self) -> None:
        super().__init__()

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        super().start(ser)
        self.press(Button.LCLICK)


class RCLICK(UnitCommand):
    def __init__(self) -> None:
        super().__init__()

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        super().start(ser)
        self.press(Button.RCLICK)


class HOME(UnitCommand):
    def __init__(self) -> None:
        super().__init__()

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        super().start(ser)
        self.press(Button.HOME)


class CAPTURE(UnitCommand):
    def __init__(self) -> None:
        super().__init__()

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        super().start(ser)
        self.press(Button.CAPTURE)


class UP(UnitCommand):
    def __init__(self) -> None:
        super().__init__()

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        super().start(ser)
        self.key.input(Hat.TOP)
        self.wait(0.1)
        self.key.input(Hat.CENTER)


class UP_RIGHT(UnitCommand):
    def __init__(self) -> None:
        super().__init__()

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        super().start(ser)
        self.key.input(Hat.TOP_RIGHT)
        self.wait(0.1)
        self.key.input(Hat.CENTER)


class RIGHT(UnitCommand):
    def __init__(self) -> None:
        super().__init__()

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        super().start(ser)
        self.key.input(Hat.RIGHT)
        self.wait(0.1)
        self.key.input(Hat.CENTER)


class DOWN_RIGHT(UnitCommand):
    def __init__(self) -> None:
        super().__init__()

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        super().start(ser)
        self.key.input(Hat.BTM_RIGHT)
        self.wait(0.1)
        self.key.input(Hat.CENTER)


class DOWN(UnitCommand):
    def __init__(self) -> None:
        super().__init__()

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        super().start(ser)
        self.key.input(Hat.BTM)
        self.wait(0.1)
        self.key.input(Hat.CENTER)


class DOWN_LEFT(UnitCommand):
    def __init__(self) -> None:
        super().__init__()

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        super().start(ser)
        self.key.input(Hat.BTM_LEFT)
        self.wait(0.1)
        self.key.input(Hat.CENTER)


class LEFT(UnitCommand):
    def __init__(self) -> None:
        super().__init__()

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        super().start(ser)
        self.key.input(Hat.LEFT)
        self.wait(0.1)
        self.key.input(Hat.CENTER)


class UP_LEFT(UnitCommand):
    def __init__(self) -> None:
        super().__init__()

    def start(self, ser: Sender, postProcess: Callable[[], None] | None = None) -> None:  # noqa: ARG002
        super().start(ser)
        self.key.input(Hat.TOP_LEFT)
        self.wait(0.1)
        self.key.input(Hat.CENTER)
