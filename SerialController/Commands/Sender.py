# -*- coding: utf-8 -*-
from __future__ import annotations

import math
import os
import platform
import time
from logging import DEBUG, NullHandler, getLogger
from typing import TYPE_CHECKING

import serial

if TYPE_CHECKING:
    import tkinter as tk
    from logging import Logger
    from typing import Final


class Sender:
    def __init__(self, is_show_serial: tk.BooleanVar, if_print: bool = True) -> None:
        self.ser: serial.Serial = serial.Serial()
        self.is_show_serial: tk.BooleanVar = is_show_serial

        self._logger: Logger = getLogger(__name__)
        self._logger.addHandler(NullHandler())
        self._logger.setLevel(DEBUG)
        self._logger.propagate = True

        self.before: str | None = None
        self.L_holding: bool = False
        self._L_holding: float | None = None
        self.R_holding: bool = False
        self._R_holding: float | None = None
        self.is_print: bool = if_print
        self.time_bef: float = time.perf_counter()
        self.time_aft: float = time.perf_counter()
        self.Buttons: Final[list[str]] = [
            "Stick.RIGHT",
            "Stick.LEFT",
            "Button.Y",
            "Button.B",
            "Button.A",
            "Button.X",
            "Button.L",
            "Button.R",
            "Button.ZL",
            "Button.ZR",
            "Button.MINUS",
            "Button.PLUS",
            "Button.LCLICK",
            "Button.RCLICK",
            "Button.HOME",
            "Button.CAPTURE",
        ]
        self.Hat: Final[list[str]] = [
            "TOP",
            "TOP_RIGHT",
            "RIGHT",
            "BTM_RIGHT",
            "BTM",
            "BTM_LEFT",
            "LEFT",
            "TOP_LEFT",
            "CENTER",
        ]

    def openSerial(
        self,
        portNum: int,
        portName: str | None = "",
        baudrate: int = 9600,
    ) -> bool | None:
        try:
            if portName is None or portName == "":
                if os.name == "nt":
                    msg = f"connecting to COM{portName}({baudrate})"
                    print(msg)
                    self._logger.info(msg)
                    self.ser = serial.Serial("COM" + str(portNum), baudrate)
                    return True
                if os.name == "posix":
                    if platform.system() == "Darwin":
                        msg = f"connecting to /dev/tty.usbserial-{portNum}({baudrate})"
                        print(msg)
                        self._logger.info(msg)
                        self.ser = serial.Serial(
                            "/dev/tty.usbserial-" + str(portNum),
                            baudrate,
                        )
                        return True
                    msg = f"connecting to /dev/ttyUSB{portNum}({baudrate})"
                    print(msg)
                    self._logger.info(msg)
                    self.ser = serial.Serial("/dev/ttyUSB" + str(portNum), baudrate)
                    return True
                print("Not supported OS")
                self._logger.warning("Not supported OS")
                return False
            msg = f"connecting to {portNum}({baudrate})"
            print(msg)
            self._logger.info(msg)
            self.ser = serial.Serial(portName, baudrate)
            return True
        except OSError as e:
            print("COM Port: can't be established")
            self._logger.error(f"COM Port: can't be established {e}")
            return False

    def closeSerial(self) -> None:
        self._logger.debug("Closing the serial communication")
        self.ser.close()

    def isOpened(self) -> bool:
        self._logger.debug("Checking if serial communication is open")
        return self.ser.is_open

    def writeRow(self, row: str, is_show: bool = False) -> None:
        try:
            self.time_bef = time.perf_counter()
            if self.before is not None and self.before != "end" and is_show:
                output = self.before.split(" ")
                self.show_input(output)

            self.ser.write((row + "\r\n").encode("utf-8"))
            self.time_aft = time.perf_counter()
            self.before = row
        except serial.SerialException as e:
            # print(e)
            self._logger.error(f"Error : {e}")
        except AttributeError as e:
            print("Using a port that is not open.")
            self._logger.error("Maybe Using a port that is not open.")
            self._logger.error(e)
        # self._logger.debug(f"{row}")
        # Show sending serial datas
        if self.is_show_serial.get():
            print(row)

    def writeList(self, values: list[int], is_show: bool = False) -> None:
        try:
            self.time_bef = time.perf_counter()
            if self.before is not None and self.before != "end" and is_show:
                pass

            # 3DS周りで使用されているみたいだが検証出来ないので無視
            self.ser.write(values)  # pyright: ignore[reportArgumentType]
            self.time_aft = time.perf_counter()
            self.before = str(values)
        except serial.SerialException as e:
            # print(e)
            self._logger.error(f"Error : {e}")
        except AttributeError as e:
            print("Using a port that is not open.")
            self._logger.error("Maybe Using a port that is not open.")
            self._logger.error(e)
        # self._logger.debug(f"{values}")
        # Show sending serial datas
        if self.is_show_serial.get():
            print(values)

    def writeRow_wo_perf_counter(self, row: str, is_show: bool = False) -> None:  # noqa: ARG002
        try:
            self.ser.write((row + "\r\n").encode("utf-8"))
        except serial.SerialException as e:
            # エラーはあえてprintでも出す。
            print(e)
            self._logger.error(f"Error : {e}")
        except AttributeError as e:
            print("Using a port that is not open.")
            self._logger.error("Maybe Using a port that is not open.")
            self._logger.error(e)
        # self._logger.debug(f"{row}")
        # Show sending serial datas
        if self.is_show_serial.get():
            print(row)

    def show_input(self, output: list[str]) -> None:
        try:
            # print(output)
            btns = [self.Buttons[x] for x in range(16) if int(output[0], 16) >> x & 1]
            useRStick = int(output[0], 16) >> 0 & 1
            useLStick = int(output[0], 16) >> 1 & 1
            Hat = self.Hat[int(output[1])]
            if Hat != "CENTER":
                btns = [*btns, "Hat." + str(Hat)]
            LStick = list(map(lambda x: int(x, 16), output[2:4]))
            RStick = list(map(lambda x: int(x, 16), output[4:]))
            LStick_deg = math.degrees(math.atan2(128 - LStick[1], LStick[0] - 128))
            RStick_deg = math.degrees(math.atan2(128 - RStick[1], RStick[0] - 128))
            # self._logger.info(output)
            if self.is_print:
                if len(btns) == 0:
                    if self.L_holding:
                        msg = (
                            "self.press(Direction({}, {:.0f}), duration={:.2f})".format(
                                "Stick.LEFT",
                                self._L_holding,
                                self.time_bef - self.time_aft,
                            )
                        )
                        print(msg)
                        self._logger.debug(msg)
                    elif self.R_holding:
                        msg = (
                            "self.press(Direction({}, {:.0f}), duration={:.2f})".format(
                                "Stick.RIGHT",
                                self._R_holding,
                                self.time_bef - self.time_aft,
                            )
                        )
                        print(msg)
                        self._logger.debug(msg)
                    if LStick == [128, 128]:
                        self.L_holding = False
                    if RStick == [128, 128]:
                        self.R_holding = False
                elif useLStick or useRStick:
                    if LStick == [128, 128] and RStick == [128, 128]:
                        self.L_holding = False
                        self.R_holding = False
                        if useRStick and useRStick:
                            if len(btns) == 3:
                                msg = "self.press({}, duration={:.2f})".format(
                                    ", ".join(btns[1:]),
                                    self.time_bef - self.time_aft,
                                )
                                print(msg)
                                self._logger.debug(msg)
                            elif len(btns) > 3:
                                msg = "self.press([{}], duration={:.2f})".format(
                                    ", ".join(btns[1:]),
                                    self.time_bef - self.time_aft,
                                )
                                print(msg)
                                self._logger.debug(msg)
                        elif len(btns) > 2:
                            msg = "self.press([{}], duration={:.2f})".format(
                                ", ".join(btns[1:]),
                                self.time_bef - self.time_aft,
                            )
                            print(msg)
                            self._logger.debug(msg)
                        elif len(btns) == 2:
                            msg = "self.press({}, duration={:.2f})".format(
                                ", ".join(btns[1:]),
                                self.time_bef - self.time_aft,
                            )
                            print(msg)
                            self._logger.debug(msg)
                        elif len(btns) == 1:
                            pass
                    elif LStick != [128, 128] and RStick == [128, 128]:  # USING L Stick
                        self.L_holding = True
                        self._L_holding = LStick_deg
                        self.R_holding = False
                        if len(btns) > 1:
                            msg = "self.press([{}, Direction({}, {:.0f})], duration={:.2f})".format(
                                ", ".join(btns[1:]),
                                btns[0],
                                self._L_holding,
                                self.time_bef - self.time_aft,
                            )
                            print(msg)
                            self._logger.debug(msg)
                        elif len(btns) == 1:
                            msg = f"self.press(Direction({btns[0]}, {self._L_holding:.0f}), duration={self.time_bef - self.time_aft:.2f})"
                            print(msg)
                            self._logger.debug(msg)
                    elif LStick == [128, 128] and RStick != [128, 128]:  # USING R stick
                        self.L_holding = False
                        self.R_holding = True
                        self._R_holding = RStick_deg
                        if len(btns) > 1:
                            msg = "self.press([{}, Direction({}, {:.0f})], duration={:.2f})".format(
                                ", ".join(btns[1:]),
                                btns[0],
                                self._R_holding,
                                self.time_bef - self.time_aft,
                            )
                            print(msg)
                            self._logger.debug(msg)
                        elif len(btns) == 1:
                            msg = f"self.press(Direction({btns[0]}, {self._R_holding:.0f}), duration={self.time_bef - self.time_aft:.2f})"
                            print(msg)
                            self._logger.debug(msg)
                    elif LStick != [128, 128] and RStick != [128, 128]:
                        self.L_holding = True
                        self.R_holding = True
                        msg = f"self.press([Direction({btns[0]}, {RStick_deg:.0f}), Direction({btns[1]}, {LStick_deg:.0f})], duration={self.time_bef - self.time_aft:.2f})"
                        print(msg)
                        self._logger.debug(msg)
                elif len(btns) == 1:
                    if self.L_holding:
                        msg = f"self.press([{btns[0]}, Direction(Stick.LEFT, {self._L_holding:.0f})], duration={self.time_bef - self.time_aft:.2f})"
                        print(msg)
                        self._logger.debug(msg)
                    elif self.R_holding:
                        msg = f"self.press([{btns[0]}, Direction(Stick.RIGHT, {self._R_holding:.0f})], duration={self.time_bef - self.time_aft:.2f})"
                        print(msg)
                        self._logger.debug(msg)
                    else:
                        msg = f"self.press({btns[0]}, duration={self.time_bef - self.time_aft:.2f})"
                        print(msg)
                        self._logger.debug(msg)
                elif len(btns) > 1:
                    if self.L_holding:
                        msg = "self.press([{}, Direction(Stick.LEFT, {:.0f})], duration={:.2f})".format(
                            ", ".join(btns),
                            self._L_holding,
                            self.time_bef - self.time_aft,
                        )
                        print(msg)
                        self._logger.debug(msg)
                    elif self.R_holding:
                        msg = "self.press([{}, Direction(Stick.RIGHT, {:.0f})], duration={:.2f})".format(
                            ", ".join(btns),
                            self._R_holding,
                            self.time_bef - self.time_aft,
                        )
                        print(msg)
                        self._logger.debug(msg)
                    else:
                        msg = "self.press([{}], duration={:.2f})".format(
                            ", ".join(btns),
                            self.time_bef - self.time_aft,
                        )
                        print(msg)
                        self._logger.debug(msg)
        except Exception as e:
            self._logger.error(f"Error:{e}")
