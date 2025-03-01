#!/usr/bin/env python3
# -*- coding: utf-8 -*-

from time import sleep
import keyboard

from . import CommandBase
from .Keys import Button, Hat, KeyPress


# Sigle button command
class UnitCommand(CommandBase.Command):
    def __init__(self):
        super().__init__()

    def start(self, ser, postProcess=None):
        self.isRunning = True
        self.key = KeyPress(ser)

    def end(self, ser):
        pass

    # do nothing at wait time(s)
    def wait(self, wait):
        sleep(wait)

    def press(self, btn):
        self.key.input([btn])
        self.wait(0.1)
        self.key.inputEnd([btn])
        self.isRunning = False
        self.key = None

    def hold(self, btn):
        self.key.input([btn])
        self.isRunning = False
        self.key = None

    def holdEnd(self, btn):
        self.key.inputEnd([btn])
        self.isRunning = False
        self.key = None


class A(UnitCommand):
    def __init__(self):
        super().__init__()

    def start(self, ser, flag=False):
        super().start(ser)
        self.press(Button.A)

    def hold_unit(self, event, ser, flag=False):
        super().start(ser)
        self.hold(Button.A)

    def holdEnd_unit(self, event, ser, flag=False):
        if keyboard.is_pressed("shift"):
            pass
        else:
            super().start(ser)
            self.holdEnd(Button.A)


class B(UnitCommand):
    def __init__(self):
        super().__init__()

    def start(self, ser):
        super().start(ser)
        self.press(Button.B)

    def hold_unit(self, event, ser, flag=False):
        super().start(ser)
        self.hold(Button.B)

    def holdEnd_unit(self, event, ser, flag=False):
        if keyboard.is_pressed("shift"):
            pass
        else:
            super().start(ser)
            self.holdEnd(Button.B)


class X(UnitCommand):
    def __init__(self):
        super().__init__()

    def start(self, ser):
        super().start(ser)
        self.press(Button.X)

    def hold_unit(self, event, ser, flag=False):
        super().start(ser)
        self.hold(Button.X)

    def holdEnd_unit(self, event, ser, flag=False):
        if keyboard.is_pressed("shift"):
            pass
        else:
            super().start(ser)
            self.holdEnd(Button.X)


class Y(UnitCommand):
    def __init__(self):
        super().__init__()

    def start(self, ser):
        super().start(ser)
        self.press(Button.Y)

    def hold_unit(self, event, ser, flag=False):
        super().start(ser)
        self.hold(Button.Y)

    def holdEnd_unit(self, event, ser, flag=False):
        if keyboard.is_pressed("shift"):
            pass
        else:
            super().start(ser)
            self.holdEnd(Button.Y)


class L(UnitCommand):
    def __init__(self):
        super().__init__()

    def start(self, ser):
        super().start(ser)
        self.press(Button.L)

    def hold_unit(self, event, ser, flag=False):
        super().start(ser)
        self.hold(Button.L)

    def holdEnd_unit(self, event, ser, flag=False):
        if keyboard.is_pressed("shift"):
            pass
        else:
            super().start(ser)
            self.holdEnd(Button.L)


class R(UnitCommand):
    def __init__(self):
        super().__init__()

    def start(self, ser):
        super().start(ser)
        self.press(Button.R)

    def hold_unit(self, event, ser, flag=False):
        super().start(ser)
        self.hold(Button.R)

    def holdEnd_unit(self, event, ser, flag=False):
        if keyboard.is_pressed("shift"):
            pass
        else:
            super().start(ser)
            self.holdEnd(Button.R)


class ZL(UnitCommand):
    def __init__(self):
        super().__init__()

    def start(self, ser):
        super().start(ser)
        self.press(Button.ZL)

    def hold_unit(self, event, ser, flag=False):
        super().start(ser)
        self.hold(Button.ZL)

    def holdEnd_unit(self, event, ser, flag=False):
        if keyboard.is_pressed("shift"):
            pass
        else:
            super().start(ser)
            self.holdEnd(Button.ZL)


class ZR(UnitCommand):
    def __init__(self):
        super().__init__()

    def start(self, ser):
        super().start(ser)
        self.press(Button.ZR)

    def hold_unit(self, event, ser, flag=False):
        super().start(ser)
        self.hold(Button.ZR)

    def holdEnd_unit(self, event, ser, flag=False):
        if keyboard.is_pressed("shift"):
            pass
        else:
            super().start(ser)
            self.holdEnd(Button.ZR)


class MINUS(UnitCommand):
    def __init__(self):
        super().__init__()

    def start(self, ser):
        super().start(ser)
        self.press(Button.MINUS)

    def hold_unit(self, event, ser, flag=False):
        super().start(ser)
        self.hold(Button.MINUS)

    def holdEnd_unit(self, event, ser, flag=False):
        if keyboard.is_pressed("shift"):
            pass
        else:
            super().start(ser)
            self.holdEnd(Button.MINUS)


class PLUS(UnitCommand):
    def __init__(self):
        super().__init__()

    def start(self, ser):
        super().start(ser)
        self.press(Button.PLUS)

    def hold_unit(self, event, ser, flag=False):
        super().start(ser)
        self.hold(Button.PLUS)

    def holdEnd_unit(self, event, ser, flag=False):
        if keyboard.is_pressed("shift"):
            pass
        else:
            super().start(ser)
            self.holdEnd(Button.PLUS)


class LCLICK(UnitCommand):
    def __init__(self):
        super().__init__()

    def start(self, ser):
        super().start(ser)
        self.press(Button.LCLICK)

    def hold_unit(self, event, ser, flag=False):
        super().start(ser)
        self.hold(Button.LCLICK)

    def holdEnd_unit(self, event, ser, flag=False):
        if keyboard.is_pressed("shift"):
            pass
        else:
            super().start(ser)
            self.holdEnd(Button.LCLICK)


class RCLICK(UnitCommand):
    def __init__(self):
        super().__init__()

    def start(self, ser):
        super().start(ser)
        self.press(Button.RCLICK)

    def hold_unit(self, event, ser, flag=False):
        super().start(ser)
        self.hold(Button.RCLICK)

    def holdEnd_unit(self, event, ser, flag=False):
        if keyboard.is_pressed("shift"):
            pass
        else:
            super().start(ser)
            self.holdEnd(Button.RCLICK)


class HOME(UnitCommand):
    def __init__(self):
        super().__init__()

    def start(self, ser):
        super().start(ser)
        self.press(Button.HOME)

    def hold_unit(self, event, ser, flag=False):
        super().start(ser)
        self.hold(Button.HOME)

    def holdEnd_unit(self, event, ser, flag=False):
        if keyboard.is_pressed("shift"):
            pass
        else:
            super().start(ser)
            self.holdEnd(Button.HOME)


class CAPTURE(UnitCommand):
    def __init__(self):
        super().__init__()

    def start(self, ser):
        super().start(ser)
        self.press(Button.CAPTURE)

    def hold_unit(self, event, ser, flag=False):
        super().start(ser)
        self.hold(Button.CAPTURE)

    def holdEnd_unit(self, event, ser, flag=False):
        if keyboard.is_pressed("shift"):
            pass
        else:
            super().start(ser)
            self.holdEnd(Button.CAPTURE)


class UP(UnitCommand):
    def __init__(self):
        super().__init__()

    def start(self, ser):
        super().start(ser)
        self.key.input(Hat.TOP)
        self.wait(0.1)
        self.key.input(Hat.CENTER)
        self.key = None

    def hold_unit(self, event, ser, flag=False):
        super().start(ser)
        self.hold(Hat.TOP)

    def holdEnd_unit(self, event, ser, flag=False):
        if keyboard.is_pressed("shift"):
            pass
        else:
            super().start(ser)
            self.holdEnd(Hat.CENTER)


class UP_RIGHT(UnitCommand):
    def __init__(self):
        super().__init__()

    def start(self, ser):
        super().start(ser)
        self.key.input(Hat.TOP_RIGHT)
        self.wait(0.1)
        self.key.input(Hat.CENTER)
        self.key = None

    def hold_unit(self, event, ser, flag=False):
        super().start(ser)
        self.hold(Hat.TOP_RIGHT)

    def holdEnd_unit(self, event, ser, flag=False):
        if keyboard.is_pressed("shift"):
            pass
        else:
            super().start(ser)
            self.holdEnd(Hat.CENTER)


class RIGHT(UnitCommand):
    def __init__(self):
        super().__init__()

    def start(self, ser):
        super().start(ser)
        self.key.input(Hat.RIGHT)
        self.wait(0.1)
        self.key.input(Hat.CENTER)
        self.key = None

    def hold_unit(self, event, ser, flag=False):
        super().start(ser)
        self.hold(Hat.RIGHT)

    def holdEnd_unit(self, event, ser, flag=False):
        if keyboard.is_pressed("shift"):
            pass
        else:
            super().start(ser)
            self.holdEnd(Hat.CENTER)


class DOWN_RIGHT(UnitCommand):
    def __init__(self):
        super().__init__()

    def start(self, ser):
        super().start(ser)
        self.key.input(Hat.BTM_RIGHT)
        self.wait(0.1)
        self.key.input(Hat.CENTER)
        self.key = None

    def hold_unit(self, event, ser, flag=False):
        super().start(ser)
        self.hold(Hat.BTM_RIGHT)

    def holdEnd_unit(self, event, ser, flag=False):
        if keyboard.is_pressed("shift"):
            pass
        else:
            super().start(ser)
            self.holdEnd(Hat.CENTER)


class DOWN(UnitCommand):
    def __init__(self):
        super().__init__()

    def start(self, ser):
        super().start(ser)
        self.key.input(Hat.BTM)
        self.wait(0.1)
        self.key.input(Hat.CENTER)
        self.key = None

    def hold_unit(self, event, ser, flag=False):
        super().start(ser)
        self.hold(Hat.BTM)

    def holdEnd_unit(self, event, ser, flag=False):
        if keyboard.is_pressed("shift"):
            pass
        else:
            super().start(ser)
            self.holdEnd(Hat.CENTER)


class DOWN_LEFT(UnitCommand):
    def __init__(self):
        super().__init__()

    def start(self, ser):
        super().start(ser)
        self.key.input(Hat.BTM_LEFT)
        self.wait(0.1)
        self.key.input(Hat.CENTER)
        self.key = None

    def hold_unit(self, event, ser, flag=False):
        super().start(ser)
        self.hold(Hat.BTM_LEFT)

    def holdEnd_unit(self, event, ser, flag=False):
        if keyboard.is_pressed("shift"):
            pass
        else:
            super().start(ser)
            self.holdEnd(Hat.CENTER)


class LEFT(UnitCommand):
    def __init__(self):
        super().__init__()

    def start(self, ser):
        super().start(ser)
        self.key.input(Hat.LEFT)
        self.wait(0.1)
        self.key.input(Hat.CENTER)
        self.key = None

    def hold_unit(self, event, ser, flag=False):
        super().start(ser)
        self.hold(Hat.LEFT)

    def holdEnd_unit(self, event, ser, flag=False):
        if keyboard.is_pressed("shift"):
            pass
        else:
            super().start(ser)
            self.holdEnd(Hat.CENTER)


class UP_LEFT(UnitCommand):
    def __init__(self):
        super().__init__()

    def start(self, ser):
        super().start(ser)
        self.key.input(Hat.TOP_LEFT)
        self.wait(0.1)
        self.key.input(Hat.CENTER)
        self.key = None

    def hold_unit(self, event, ser, flag=False):
        super().start(ser)
        self.hold(Hat.TOP_LEFT)

    def holdEnd_unit(self, event, ser, flag=False):
        if keyboard.is_pressed("shift"):
            pass
        else:
            super().start(ser)
            self.holdEnd(Hat.CENTER)
