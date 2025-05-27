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
        self._L_holding = None
        self.R_holding: bool = False
        self._R_holding = None
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
                    print(
                        f"connecting to COM{portNum}({baudrate})",
                    )
                    self._logger.info(
                        f"connecting to COM{portNum}({baudrate})",
                    )
                    self.ser = serial.Serial("COM" + str(portNum), baudrate)
                    return True
                if os.name == "posix":
                    if platform.system() == "Darwin":
                        print(
                            f"connecting to /dev/tty.usbserial-{portNum}({baudrate})",
                        )
                        self._logger.info(
                            f"connecting to /dev/tty.usbserial-{portNum}({baudrate})",
                        )
                        self.ser = serial.Serial(
                            "/dev/tty.usbserial-" + str(portNum),
                            baudrate,
                        )
                        return True
                    print(
                        f"connecting to /dev/ttyUSB{portNum}({baudrate})",
                    )
                    self._logger.info(
                        f"connecting to /dev/ttyUSB{portNum}({baudrate})",
                    )
                    self.ser = serial.Serial("/dev/ttyUSB" + str(portNum), baudrate)
                    return True
                print("Not supported OS")
                self._logger.warning("Not supported OS")
                return False
            print(f"connecting to {portName}")
            self._logger.info(f"connecting to {portName}")
            self.ser = serial.Serial(portName, 9600)
            return True
        except OSError as e:
            print("COM Port: can't be established")
            self._logger.error(f"COM Port: can't be established {e}")
            # print(e)
            return False

    def closeSerial(self) -> None:
        self._logger.debug("Closing the serial communication")
        self.ser.close()

    def isOpened(self) -> bool:
        self._logger.debug("Checking if serial communication is open")
        # return bool(self.ser is not None and self.ser.is_open)
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

    def writeList(self, values: list, is_show: bool = False) -> None:
        try:
            self.time_bef = time.perf_counter()
            if self.before is not None and self.before != "end" and is_show:
                pass

            self.ser.write(values)
            self.time_aft = time.perf_counter()
            self.before = values
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

    def writeRow_wo_perf_counter(self, row: str, is_show: bool = False) -> None:
        try:
            self.ser.write((row + "\r\n").encode("utf-8"))
        except serial.serialutil.SerialException as e:
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
                        print(
                            "self.press(Direction({}, {:.0f}), duration={:.2f})".format(
                                "Stick.LEFT",
                                self._L_holding,
                                self.time_bef - self.time_aft,
                            ),
                        )
                        self._logger.debug(
                            "self.press(Direction({}, {:.0f}), duration={:.2f})".format(
                                "Stick.LEFT",
                                self._L_holding,
                                self.time_bef - self.time_aft,
                            ),
                        )
                    elif self.R_holding:
                        print(
                            "self.press(Direction({}, {:.0f}), duration={:.2f})".format(
                                "Stick.RIGHT",
                                self._R_holding,
                                self.time_bef - self.time_aft,
                            ),
                        )
                        self._logger.debug(
                            "self.press(Direction({}, {:.0f}), duration={:.2f})".format(
                                "Stick.RIGHT",
                                self._R_holding,
                                self.time_bef - self.time_aft,
                            ),
                        )
                    if LStick == [128, 128]:
                        self.L_holding = False
                    if RStick == [128, 128]:
                        self.R_holding = False
                    else:
                        pass
                elif useLStick or useRStick:
                    if LStick == [128, 128] and RStick == [128, 128]:
                        if useRStick and useRStick:
                            if len(btns) == 3:
                                print(
                                    "self.press({}, duration={:.2f})".format(
                                        ", ".join(btns[1:]),
                                        self.time_bef - self.time_aft,
                                    ),
                                )
                                self._logger.debug(
                                    "self.press([{}], duration={:.2f})".format(
                                        ", ".join(btns[1:]),
                                        self.time_bef - self.time_aft,
                                    ),
                                )
                            elif len(btns) > 3:
                                print(
                                    "self.press([{}], duration={:.2f})".format(
                                        ", ".join(btns[1:]),
                                        self.time_bef - self.time_aft,
                                    ),
                                )
                                self._logger.debug(
                                    "self.press([{}], duration={:.2f})".format(
                                        ", ".join(btns[1:]),
                                        self.time_bef - self.time_aft,
                                    ),
                                )
                            self.L_holding = False
                            self.R_holding = False
                        else:
                            if len(btns) > 2:
                                print(
                                    "self.press([{}], duration={:.2f})".format(
                                        ", ".join(btns[1:]),
                                        self.time_bef - self.time_aft,
                                    ),
                                )
                                self._logger.debug(
                                    "self.press([{}], duration={:.2f})".format(
                                        ", ".join(btns[1:]),
                                        self.time_bef - self.time_aft,
                                    ),
                                )
                                self.L_holding = False
                                self.R_holding = False
                            if len(btns) == 2:
                                print(
                                    "self.press({}, duration={:.2f})".format(
                                        ", ".join(btns[1:]),
                                        self.time_bef - self.time_aft,
                                    ),
                                )
                                self._logger.debug(
                                    "self.press({}, duration={:.2f})".format(
                                        ", ".join(btns[1:]),
                                        self.time_bef - self.time_aft,
                                    ),
                                )
                                self.L_holding = False
                                self.R_holding = False
                            elif len(btns) == 1:
                                self.L_holding = False
                                self.R_holding = False
                    elif LStick != [128, 128] and RStick == [128, 128]:  # USING L Stick
                        self.L_holding = True
                        self._L_holding = LStick_deg
                        self.R_holding = False
                        if len(btns) > 1:
                            print(
                                "self.press([{}, Direction({}, {:.0f})], duration={:.2f})".format(
                                    ", ".join(btns[1:]),
                                    btns[0],
                                    self._L_holding,
                                    self.time_bef - self.time_aft,
                                ),
                            )
                            self._logger.debug(
                                "self.press([{}, Direction({}, {:.0f})], duration={:.2f})".format(
                                    ", ".join(btns[1:]),
                                    btns[0],
                                    self._L_holding,
                                    self.time_bef - self.time_aft,
                                ),
                            )
                        elif len(btns) == 1:
                            print(
                                f"self.press(Direction({btns[0]}, {self._L_holding:.0f}), duration={self.time_bef - self.time_aft:.2f})",
                            )
                            self._logger.debug(
                                f"self.press(Direction({btns[0]}, {self._L_holding:.0f}), duration={self.time_bef - self.time_aft:.2f})",
                            )
                    elif LStick == [128, 128] and RStick != [128, 128]:  # USING R stick
                        self.L_holding = False
                        self.R_holding = True
                        self._R_holding = RStick_deg
                        if len(btns) > 1:
                            print(
                                "self.press([{}, Direction({}, {:.0f})], duration={:.2f})".format(
                                    ", ".join(btns[1:]),
                                    btns[0],
                                    self._R_holding,
                                    self.time_bef - self.time_aft,
                                ),
                            )
                            self._logger.debug(
                                "self.press([{}, Direction({}, {:.0f})], duration={:.2f})".format(
                                    ", ".join(btns[1:]),
                                    btns[0],
                                    self._R_holding,
                                    self.time_bef - self.time_aft,
                                ),
                            )
                        elif len(btns) == 1:
                            print(
                                f"self.press(Direction({btns[0]}, {self._R_holding:.0f}), duration={self.time_bef - self.time_aft:.2f})",
                            )
                            self._logger.debug(
                                f"self.press(Direction({btns[0]}, {self._R_holding:.0f}), duration={self.time_bef - self.time_aft:.2f})",
                            )
                    elif LStick != [128, 128] and RStick != [128, 128]:
                        self.L_holding = True
                        self.R_holding = True
                        print(
                            f"self.press([Direction({btns[0]}, {RStick_deg:.0f}), Direction({btns[1]}, {LStick_deg:.0f})], duration={self.time_bef - self.time_aft:.2f})",
                        )
                        self._logger.debug(
                            f"self.press([Direction({btns[0]}, {RStick_deg:.0f}), Direction({btns[1]}, {LStick_deg:.0f})], duration={self.time_bef - self.time_aft:.2f})",
                        )
                elif len(btns) == 1:
                    if self.L_holding:
                        print(
                            f"self.press([{btns[0]}, Direction(Stick.LEFT, {self._L_holding:.0f})], duration={self.time_bef - self.time_aft:.2f})",
                        )
                        self._logger.debug(
                            f"self.press({btns[0]}, Direction(Stick.LEFT, {self._L_holding:.0f}), duration={self.time_bef - self.time_aft:.2f})",
                        )
                    elif self.R_holding:
                        print(
                            f"self.press([{btns[0]}, Direction(Stick.RIGHT, {self._R_holding:.0f})], duration={self.time_bef - self.time_aft:.2f})",
                        )
                        self._logger.debug(
                            f"self.press({btns[0]}, Direction(Stick.RIGHT, {self._R_holding:.0f}), duration={self.time_bef - self.time_aft:.2f})",
                        )
                    else:
                        print(
                            f"self.press({btns[0]}, duration={self.time_bef - self.time_aft:.2f})",
                        )
                        self._logger.debug(
                            f"self.press({btns[0]}, duration={self.time_bef - self.time_aft:.2f})",
                        )
                elif len(btns) > 1:
                    if self.L_holding:
                        print(
                            "self.press([{}, Direction(Stick.LEFT, {:.0f})], duration={:.2f})".format(
                                ", ".join(btns),
                                self._L_holding,
                                self.time_bef - self.time_aft,
                            ),
                        )
                        self._logger.debug(
                            "self.press([{}, Direction(Stick.LEFT, {:.0f})], duration={:.2f})".format(
                                ", ".join(btns),
                                self._L_holding,
                                self.time_bef - self.time_aft,
                            ),
                        )
                    elif self.R_holding:
                        print(
                            "self.press([{}, Direction(Stick.RIGHT, {:.0f})], duration={:.2f})".format(
                                ", ".join(btns),
                                self._R_holding,
                                self.time_bef - self.time_aft,
                            ),
                        )
                        self._logger.debug(
                            "self.press([{}, Direction(Stick.RIGHT, {:.0f})], duration={:.2f})".format(
                                ", ".join(btns),
                                self._R_holding,
                                self.time_bef - self.time_aft,
                            ),
                        )
                    else:
                        print(
                            "self.press([{}], duration={:.2f})".format(
                                ", ".join(btns),
                                self.time_bef - self.time_aft,
                            ),
                        )
                        self._logger.debug(
                            "self.press([{}], duration={:.2f})".format(
                                ", ".join(btns),
                                self.time_bef - self.time_aft,
                            ),
                        )
        except Exception as e:
            self._logger.error("Error:", e)
