# -*- coding: utf-8 -*-
from __future__ import annotations

import datetime
import logging
import os
import time
import tkinter as tk
from collections import deque
from logging import DEBUG, NullHandler, StreamHandler, getLogger
from tkinter.scrolledtext import ScrolledText
from typing import TYPE_CHECKING

import cv2
import numpy as np
from Commands.Keys import NEUTRAL, Direction, Stick, Touchscreen
from file_handler import FileHandler
from PIL import Image, ImageTk

if TYPE_CHECKING:
    from collections.abc import Callable
    from logging import Logger
    from tkinter import BooleanVar, Canvas, Event, Misc, Toplevel
    from typing import Final

    from Camera import Camera
    from Commands.Keys import KeyPress
    from Commands.Sender import Sender

try:
    os.makedirs("log")
except FileExistsError as e:
    print(e)

isTakeLog = False
# logger_stick = getLogger(__name__)
nowtime = datetime.datetime.fromtimestamp(time.time()).strftime("%Y%m%d_%H%M%S")


class CaptureArea(tk.Canvas):
    def __init__(
        self,
        camera: Camera,
        fps: str | int,
        right_mouse_mode: str,
        is_show: BooleanVar,
        ser: KeyPress,
        master: Misc | None = None,
        show_width: int = 640,
        show_height: int = 360,
    ) -> None:
        super().__init__(
            master,
            borderwidth=0,
            cursor="tcross",
            width=show_width,
            height=show_height,
        )

        self._logger: Final[Logger] = getLogger(__name__)
        self._logger.addHandler(NullHandler())
        self._logger.setLevel(DEBUG)
        self._logger.propagate = True

        if master is not None:
            self.master: tk.Misc = master
        self.radius: int = 60  # 描画する円の半径
        self.camera: Camera = camera
        # self.show_size = (640, 360)
        self.show_width: int = int(show_width)
        self.show_height: int = int(show_height)
        self.show_size: tuple[int, int] = (self.show_width, self.show_height)
        self.is_show_var: tk.BooleanVar = is_show
        self.lx_init: int = 0
        self.ly_init: int = 0
        self.rx_init: int = 0
        self.ry_init: int = 0
        self.min_x: int = 0
        self.min_y: int = 0
        self.max_x: int = 0
        self.max_y: int = 0
        # self.keys = None
        self.keys_: KeyPress = ser
        self.lcircle = None
        self.lcircle2 = None
        self.rcircle = None
        self.rcircle2 = None
        self.LStick = None
        self.RStick = None
        self.calc_time = None
        self.ss = None
        self.dq = None
        self._langle = None
        self._lmag = None
        self._rangle = None
        self._rmag = None
        self.RightMouseMode: str = "Default"
        self.touchscreen_start_x: int = 1
        self.touchscreen_start_y: int = 1
        self.touchscreen_end_x: int = 320
        self.touchscreen_end_y: int = 240

        self.stick_handler = StreamHandler()
        self.stick_logging_level = DEBUG
        self.stick_handler.setLevel(self.stick_logging_level)
        # self._logger.setLevel(self.stick_logging_level)
        # self._logger.addHandler(self.stick_handler)
        # self._logger.propagate = False
        if isTakeLog:
            filename_base = os.path.join("log", f"{nowtime}")
            self.LS = logging.FileHandler(
                filename=f"{filename_base}_LStick.log",
                encoding="utf-8",
            )
            self.LS.setLevel(logging.DEBUG)
            self.LSTICK_logger: Final[Logger] = logging.getLogger("L_STICK")
            self.LSTICK_logger.setLevel(logging.DEBUG)
            self.LSTICK_logger.addHandler(self.LS)

            self.RS = logging.FileHandler(
                filename=f"{filename_base}_RStick.log",
                encoding="utf-8",
            )
            self.RS.setLevel(logging.DEBUG)
            self.RSTICK_logger: Final[Logger] = logging.getLogger("R_STICK")
            self.RSTICK_logger.setLevel(logging.DEBUG)
            self.RSTICK_logger.addHandler(self.RS)
        # self.circle =

        self.setFps(fps)
        self.changeRightMouseMode(right_mouse_mode)

        self.bind("<Control-ButtonPress-1>", self.mouseCtrlLeftPress)
        self.bind("<Control-ButtonRelease-1>", self.mouseCtrlLeftRelease)
        self.bind("<Control-Shift-ButtonPress-1>", self.StartRangeSS)
        self.bind("<Control-Shift-Button1-Motion>", self.MotionRangeSS)
        self.bind("<Control-Shift-ButtonRelease-1>", self.ReleaseRangeSS)

        self.bind("<Control-Alt-ButtonPress-1>", self.StartRangeSS)
        self.bind("<Control-Alt-Button1-Motion>", self.MotionRangeSS)
        self.bind(
            "<Control-Alt-ButtonRelease-1>",
            self.ReleaseRangeSS_asksaveasfilename,
        )

        self.bind("<Control-ButtonPress-3>", self.StartRangeTouchscreen)
        self.bind("<Control-Button3-Motion>", self.MotionRangeTouchscreen)
        self.bind("<Control-ButtonRelease-3>", self.ReleaseRangeTouchscreen)

        # Set disabled image first
        disabled_img = cv2.imread(
            FileHandler.get_asset_path("disabled.png"),
            cv2.IMREAD_GRAYSCALE,
        )
        disabled_pil = Image.fromarray(disabled_img)
        self.disabled_tk = ImageTk.PhotoImage(disabled_pil)
        self.im = self.disabled_tk
        # self.configure(image=self.disabled_tk)  # labelからキャンバスに変更したので微修正
        self.im_ = self.create_image(0, 0, image=self.disabled_tk, anchor=tk.NW)

    def ApplyLStickMouse(self) -> None:
        if self.master.is_use_left_stick_mouse.get():
            self.BindLeftClick()
        else:
            self.UnbindLeftClick()

    def ApplyRStickMouse(self) -> None:
        if self.master.is_use_right_stick_mouse.get():
            self.BindRightClick()
        else:
            self.UnbindRightClick()

    def StartRangeSS(self, event: Event[Canvas]) -> None:
        self.ss = self.camera.image_bgr
        if self.master.is_use_left_stick_mouse.get():
            self.UnbindLeftClick()
        if self.master.is_use_right_stick_mouse.get():
            self.UnbindRightClick()

        self.min_x, self.min_y = event.x, event.y
        self.delete("SelectArea")
        self.create_rectangle(
            self.min_x,
            self.min_y,
            self.min_x + 1,
            self.min_y + 1,
            width=3.0,
            outline="red",
            tag="SelectArea",
        )

        ratio_x = float(self.camera.capture_size[0] / self.show_size[0])
        ratio_y = float(self.camera.capture_size[1] / self.show_size[1])
        msg = f"Mouse down: Show ({self.min_x}, {self.min_y}) / Capture ({int(self.min_x * ratio_x)}, {int(self.min_y * ratio_y)})"
        print(msg)
        self._logger.info(msg)

        if self.master.is_use_left_stick_mouse.get():
            self.BindLeftClick()
        if self.master.is_use_right_stick_mouse.get():
            self.BindRightClick()

    def MotionRangeSS(self, event: Event[Canvas]) -> None:
        if event.x < 0:
            self.max_x = 0
        else:
            self.max_x = min(self.show_width, event.x)
        if event.y < 0:
            self.max_y = 0
        else:
            self.max_y = min(self.show_height, event.y)
        self.coords(
            "SelectArea",
            self.min_x,
            self.min_y,
            self.max_x + 1,
            self.max_y + 1,
        )
        self.coords(
            "SelectAreaFilled",
            self.min_x,
            self.min_y,
            self.max_x + 1,
            self.max_y + 1,
        )

    def ReleaseRangeSS(self, event: Event[Canvas]) -> None:  # noqa: ARG002
        # self.max_x, self.max_y = event.x, event.y
        ratio_x = float(self.camera.capture_size[0] / self.show_size[0])
        ratio_y = float(self.camera.capture_size[1] / self.show_size[1])
        print(
            f"Mouse up: Show ({self.max_x}, {self.max_y}) / Capture ({int(self.max_x * ratio_x)}, {int(self.max_y * ratio_y)})",
        )
        self._logger.info(
            f"Mouse up: Show ({self.max_x}, {self.max_y}) / Capture ({int(self.max_x * ratio_x)}, {int(self.max_y * ratio_y)})",
        )
        if self.min_x > self.max_x:
            self.min_x, self.max_x = self.max_x, self.min_x
        if self.min_y > self.max_y:
            self.min_y, self.max_y = self.max_y, self.min_y

        self.camera.saveCapture(
            crop=1,
            crop_ax=[
                int(self.min_x * ratio_x),
                int(self.min_y * ratio_x),
                int(self.max_x * ratio_x),
                int(self.max_y * ratio_x),
            ],
        )

        # t = 0
        self.after(250, self.delete("SelectArea"))

        if self.master.is_use_left_stick_mouse.get():
            self.BindLeftClick()
        if self.master.is_use_right_stick_mouse.get():
            self.BindRightClick()

    def ReleaseRangeSS_asksaveasfilename(self, event: Event[Canvas]) -> None:  # noqa: ARG002
        # self.max_x, self.max_y = event.x, event.y
        ratio_x = float(self.camera.capture_size[0] / self.show_size[0])
        ratio_y = float(self.camera.capture_size[1] / self.show_size[1])
        print(
            f"Mouse up: Show ({self.max_x}, {self.max_y}) / Capture ({int(self.max_x * ratio_x)}, {int(self.max_y * ratio_y)})",
        )
        self._logger.info(
            f"Mouse up: Show ({self.max_x}, {self.max_y}) / Capture ({int(self.max_x * ratio_x)}, {int(self.max_y * ratio_y)})",
        )
        if self.min_x > self.max_x:
            self.min_x, self.max_x = self.max_x, self.min_x
        if self.min_y > self.max_y:
            self.min_y, self.max_y = self.max_y, self.min_y

        filename = filedialog.asksaveasfilename(
            title="名前を付けて保存",
            filetypes=[("PNG", ".png")],
            initialdir="./TEMPLATE/",
            defaultextension="png",
        )
        if filename != "":
            self.camera.saveCapture(
                filename=filename[:-4],
                crop=1,
                crop_ax=[
                    int(self.min_x * ratio_x),
                    int(self.min_y * ratio_x),
                    int(self.max_x * ratio_x),
                    int(self.max_y * ratio_x),
                ],
            )

        # t = 0
        self.after(250, self.delete("SelectArea"))

        if self.master.is_use_left_stick_mouse.get():
            self.BindLeftClick()
        if self.master.is_use_right_stick_mouse.get():
            self.BindRightClick()

    def StartRangeTouchscreen(self, event: Event[Canvas]) -> None:
        self.ss = self.camera.image_bgr
        if self.master.is_use_left_stick_mouse.get():
            self.UnbindLeftClick()
        if self.master.is_use_right_stick_mouse.get():
            self.UnbindRightClick()

        self.touchscreen_start_x, self.touchscreen_start_y = event.x, event.y
        self.delete("SelectArea")
        self.create_rectangle(
            self.touchscreen_start_x,
            self.touchscreen_start_y,
            self.touchscreen_start_x + 1,
            self.touchscreen_start_y + 1,
            width=3.0,
            outline="red",
            tag="SelectArea",
        )

        if self.master.is_use_left_stick_mouse.get():
            self.BindLeftClick()
        if self.master.is_use_right_stick_mouse.get():
            self.BindRightClick()

    def MotionRangeTouchscreen(self, event: Event[Canvas]) -> None:
        if event.x < 0:
            self.touchscreen_end_x = 0
        else:
            self.touchscreen_end_x = min(self.show_width, event.x)
        if event.y < 0:
            self.touchscreen_end_y = 0
        else:
            self.touchscreen_end_y = min(self.show_height, event.y)
        self.coords(
            "SelectArea",
            self.touchscreen_start_x,
            self.touchscreen_start_y,
            self.touchscreen_end_x + 1,
            self.touchscreen_end_y + 1,
        )
        self.coords(
            "SelectAreaFilled",
            self.touchscreen_start_x,
            self.touchscreen_start_y,
            self.touchscreen_end_x + 1,
            self.touchscreen_end_y + 1,
        )

    def ReleaseRangeTouchscreen(self, event: Event[Canvas]) -> None:  # noqa: ARG002
        if self.touchscreen_start_x > self.touchscreen_end_x:
            self.touchscreen_start_x, self.touchscreen_end_x = (
                self.touchscreen_end_x,
                self.touchscreen_start_x,
            )
        if self.touchscreen_start_y > self.touchscreen_end_y:
            self.touchscreen_start_y, self.touchscreen_end_y = (
                self.touchscreen_end_y,
                self.touchscreen_start_y,
            )

        print(
            f"Touchscreen Area: ({self.touchscreen_start_x}, {self.touchscreen_start_y}), ({self.touchscreen_end_x}, {self.touchscreen_end_y})",
        )

        # t = 0
        self.after(250, self.delete("SelectArea"))

        if self.master.is_use_left_stick_mouse.get():
            self.BindLeftClick()
        if self.master.is_use_right_stick_mouse.get():
            self.BindRightClick()

    def setFps(self, fps: str) -> None:
        # self.next_frames = int(16 * (60 / int(fps)))
        self.next_frames = int(1000 / int(fps))
        self._logger.info(f"FPS set to {fps}")

    def setShowsize(self, show_height: int, show_width: int) -> None:
        self.show_width = int(show_width)
        self.show_height = int(show_height)
        self.show_size = (self.show_width, self.show_height)
        self.config(width=self.show_width, height=self.show_height)
        print(f"Show size set to {self.show_width} x {self.show_height}")
        self._logger.info(
            f"Show size set to {self.show_width} x {self.show_height}",
        )

    def changeRightMouseMode(self, mode: str) -> None:
        self.RightMouseMode = mode

    def setTouchscreenArea(
        self,
        touchscreen_start_x: int,
        touchscreen_start_y: int,
        touchscreen_end_x: int,
        touchscreen_end_y: int,
    ) -> None:
        self.touchscreen_start_x = touchscreen_start_x
        self.touchscreen_start_y = touchscreen_start_y
        self.touchscreen_end_x = touchscreen_end_x
        self.touchscreen_end_y = touchscreen_end_y

    def mouseCtrlLeftPress(self, event: Event[Canvas]) -> None:
        _img = cv2.cvtColor(self.camera.image_bgr, cv2.COLOR_BGR2RGB)
        if self.master.is_use_left_stick_mouse.get():
            self.UnbindLeftClick()
        x, y = event.x, event.y
        ratio_x = float(self.camera.capture_size[0] / self.show_size[0])
        ratio_y = float(self.camera.capture_size[1] / self.show_size[1])
        print(
            f"Mouse down: Show ({x}, {y}) / Capture ({int(x * ratio_x)}, {int(y * ratio_y)})",
        )
        print(
            f"Color [R: {_img[int(y * ratio_y), int(x * ratio_x)][0]}, "
            f"G: {_img[int(y * ratio_y), int(x * ratio_x)][1]}, "
            f"B: {_img[int(y * ratio_y), int(x * ratio_x)][2]}]",
        )
        self._logger.info(
            f"Mouse down: Show ({x}, {y}) / Capture ({int(x * ratio_x)}, {int(y * ratio_y)})",
        )

    def mouseCtrlLeftRelease(self, event: Event[Canvas]) -> None:  # noqa: ARG002
        if self.master.is_use_left_stick_mouse.get():
            self.BindLeftClick()

    def mouseLeftPress(self, event: Event[Canvas], keys_: KeyPress) -> None:  # noqa: ARG002
        if self.master.is_use_right_stick_mouse.get():
            self.UnbindRightClick()
        self.config(cursor="dot")
        self.lx_init, self.ly_init = event.x, event.y
        self.lcircle = self.create_oval(
            self.lx_init - self.radius,
            self.ly_init - self.radius,
            self.lx_init + self.radius,
            self.ly_init + self.radius,
            outline="cyan",
            tag="lcircle",
        )
        self.lcircle2 = self.create_oval(
            self.lx_init - self.radius // 10,
            self.ly_init - self.radius // 10,
            self.lx_init + self.radius // 10,
            self.ly_init + self.radius // 10,
            fill="cyan",
            tag="lcircle2",
        )
        # self.LStick = StickCommand.StickLeft()
        # self.LStick.start(ser)
        if isTakeLog:
            if self.dq is None:
                self.dq = deque()
            else:
                self.dq.clear()

            if self.calc_time is None:
                self.calc_time = time.perf_counter()
            else:
                # LSTICK_logger.debug(f"{0},{0},{time.perf_counter() - self.calc_time}")
                self.dq.append([0, 0, time.perf_counter() - self.calc_time])
            self._langle = None
            self._lmag = None

    def mouseLeftPressing(
        self,
        event: Event[Canvas],
        keys_: KeyPress,  # noqa: ARG002
        angle: int = 0,  # noqa: ARG002
    ) -> None:
        # _time = self.calc_time
        langle = np.rad2deg(np.arctan2(self.ly_init - event.y, event.x - self.lx_init))
        mag = (
            np.sqrt((self.ly_init - event.y) ** 2 + (event.x - self.lx_init) ** 2)
            / self.radius
        )
        if mag <= 0:
            mag = 0
        elif mag >= 1:
            mag = 1

        if (self._langle and self._lmag) is not None and isTakeLog:
            _time = time.perf_counter()
            if _time - self.calc_time > 0.05:
                self.keys_.input(
                    Direction(
                        Stick.LEFT,
                        (
                            int(128 + mag * 127.5 * np.cos(np.deg2rad(langle))),
                            255 - int(128 - mag * 127.5 * np.sin(np.deg2rad(langle))),
                        ),
                    ),
                )
                self.dq.append([langle, mag, _time - self.calc_time])
                self.calc_time = _time
        elif not isTakeLog:
            self.keys_.input(
                Direction(
                    Stick.LEFT,
                    (
                        int(128 + mag * 127.5 * np.cos(np.deg2rad(langle))),
                        255 - int(128 - mag * 127.5 * np.sin(np.deg2rad(langle))),
                    ),
                ),
            )

        if mag >= 1:
            center_x = (self.radius + self.radius // 11) * np.cos(np.deg2rad(langle))
            center_y = (self.radius + self.radius // 11) * np.sin(np.deg2rad(langle))
            circ_x_1 = self.lx_init + center_x - self.radius // 10
            circ_x_2 = self.lx_init + center_x + self.radius // 10
            circ_y_1 = self.ly_init - center_y - self.radius // 10
            circ_y_2 = self.ly_init - center_y + self.radius // 10
        else:
            circ_x_1 = event.x - self.radius // 10
            circ_x_2 = event.x + self.radius // 10
            circ_y_1 = event.y - self.radius // 10
            circ_y_2 = event.y + self.radius // 10

        self.coords(
            "lcircle2",
            circ_x_1,
            circ_y_1,
            circ_x_2,
            circ_y_2,
        )
        self._langle = langle
        self._lmag = mag

    def mouseLeftRelease(self, keys_: KeyPress) -> None:  # noqa: ARG002
        self.config(cursor="tcross")
        self.keys_.input(Direction(Stick.LEFT, NEUTRAL))
        self.delete("lcircle")
        self.delete("lcircle2")
        if self.master.is_use_right_stick_mouse.get():
            self.BindRightClick()
        # self.event_generate('<Motion>', warp=True, x=self.lx_init, y=self.ly_init)
        if isTakeLog:
            self.dq.append(
                [self._langle, self._lmag, time.perf_counter() - self.calc_time],
            )
            for _ in self.dq:
                self.LSTICK_logger.debug(",".join(list(map(str, _))))

    def mouseRightPress(self, event: Event[Canvas], keys_: KeyPress) -> None:
        if self.master.is_use_left_stick_mouse.get():
            self.UnbindLeftClick()

        if self.RightMouseMode == "Qingpi":
            if (
                self.touchscreen_start_x < event.x
                and event.x < self.touchscreen_end_x
                and self.touchscreen_start_y < event.y
                and event.y < self.touchscreen_end_y
            ):
                width = self.touchscreen_end_x - self.touchscreen_start_x
                height = self.touchscreen_end_y - self.touchscreen_start_y
                pos_x = int(320.0 * (event.x - self.touchscreen_start_x) / width)
                pos_y = int(240.0 * (event.y - self.touchscreen_start_y) / height)
                keys_.input(Touchscreen(pos_x, pos_y))
        else:
            self.config(cursor="dot")
            self.rx_init, self.ry_init = event.x, event.y
            self.rcircle = self.create_oval(
                self.rx_init - self.radius,
                self.ry_init - self.radius,
                self.rx_init + self.radius,
                self.ry_init + self.radius,
                outline="red",
                tag="rcircle",
            )
            self.rcircle2 = self.create_oval(
                self.rx_init - self.radius // 10,
                self.ry_init - self.radius // 10,
                self.rx_init + self.radius // 10,
                self.ry_init + self.radius // 10,
                fill="red",
                tag="rcircle2",
            )

            # self.RStick = StickCommand.StickRight()
            # self.RStick.start(ser)
            if isTakeLog:
                if self.dq is None:
                    self.dq = deque()
                else:
                    self.dq.clear()

                if self.calc_time is None:
                    self.calc_time = time.perf_counter()
                else:
                    # LSTICK_logger.debug(f"{0},{0},{time.perf_counter() - self.calc_time}")
                    self.dq.append([0, 0, time.perf_counter() - self.calc_time])
            self._rangle = None
            self._rmag = None

    def mouseRightPressing(
        self,
        event: Event[Canvas],
        keys_: KeyPress,
        angle: int = 0,  # noqa: ARG002
    ) -> None:
        if self.RightMouseMode == "Qingpi":
            if (
                self.touchscreen_start_x < event.x
                and event.x < self.touchscreen_end_x
                and self.touchscreen_start_y < event.y
                and event.y < self.touchscreen_end_y
            ):
                width = self.touchscreen_end_x - self.touchscreen_start_x
                height = self.touchscreen_end_y - self.touchscreen_start_y
                pos_x = int(320.0 * (event.x - self.touchscreen_start_x) / width)
                pos_y = int(240.0 * (event.y - self.touchscreen_start_y) / height)
                keys_.input(Touchscreen(pos_x, pos_y))
        else:
            rangle = np.rad2deg(
                np.arctan2(self.ry_init - event.y, event.x - self.rx_init),
            )
            mag = (
                np.sqrt((self.ry_init - event.y) ** 2 + (event.x - self.rx_init) ** 2)
                / self.radius
            )
            if mag <= 0:
                mag = 0
            elif mag >= 1:
                mag = 1
            if (self._langle and self._lmag) is not None and isTakeLog:
                _time = time.perf_counter()
                if _time - self.calc_time > 0.05:
                    self.keys_.input(
                        Direction(
                            Stick.RIGHT,
                            (
                                int(128 + mag * 127.5 * np.cos(np.deg2rad(rangle))),
                                255
                                - int(128 - mag * 127.5 * np.sin(np.deg2rad(rangle))),
                            ),
                        ),
                    )
                    self.dq.append([rangle, mag, _time - self.calc_time])
                    self.calc_time = _time
            elif not isTakeLog:
                self.keys_.input(
                    Direction(
                        Stick.RIGHT,
                        (
                            int(128 + mag * 127.5 * np.cos(np.deg2rad(rangle))),
                            255 - int(128 - mag * 127.5 * np.sin(np.deg2rad(rangle))),
                        ),
                    ),
                )
            if mag >= 1:
                center_x = (self.radius + self.radius // 11) * np.cos(
                    np.deg2rad(rangle),
                )
                center_y = (self.radius + self.radius // 11) * np.sin(
                    np.deg2rad(rangle),
                )
                circ_x_1 = self.rx_init + center_x - self.radius // 10
                circ_x_2 = self.rx_init + center_x + self.radius // 10
                circ_y_1 = self.ry_init - center_y - self.radius // 10
                circ_y_2 = self.ry_init - center_y + self.radius // 10
            else:
                circ_x_1 = event.x - self.radius // 10
                circ_x_2 = event.x + self.radius // 10
                circ_y_1 = event.y - self.radius // 10
                circ_y_2 = event.y + self.radius // 10

            self.coords(
                "rcircle2",
                circ_x_1,
                circ_y_1,
                circ_x_2,
                circ_y_2,
            )
            self._rangle = rangle
            self._rmag = mag

    def mouseRightRelease(self, keys_: KeyPress) -> None:
        if self.RightMouseMode == "Qingpi":
            keys_.inputEnd(Touchscreen(0, 0))
        else:
            self.config(cursor="tcross")
            self.keys_.input(Direction(Stick.RIGHT, NEUTRAL))
            self.delete("rcircle")
            self.delete("rcircle2")
            if self.master.is_use_left_stick_mouse.get():
                self.BindLeftClick()

            # self.event_generate('<Motion>', warp=True, x=self.rx_init, y=self.ry_init)
            if isTakeLog:
                self.dq.append(
                    [self._rangle, self._rmag, time.perf_counter() - self.calc_time],
                )
                for _ in self.dq:
                    self.RSTICK_logger.debug(",".join(list(map(str, _))))

    def startCapture(self) -> None:
        self.capture()

    def capture(self) -> None:
        if self.is_show_var.get():
            image_bgr = self.camera.readFrame()
        else:
            self.after(self.next_frames, self.capture)
            return

        if image_bgr is not None:  # pyright:ignore[reportUnnecessaryComparison]
            image_rgb = cv2.cvtColor(image_bgr, cv2.COLOR_BGR2RGB)
            image_pil = Image.fromarray(image_rgb).resize(self.show_size)
            image_tk = ImageTk.PhotoImage(image_pil)

            self.im = image_tk
            # self.configure( image=image_tk)
            self.itemconfig(self.im_, image=image_tk)
        else:
            self.im = self.disabled_tk
            # self.configure(image=self.disabled_tk)
            self.itemconfig(self.im_, image=self.disabled_tk)

        self.after(self.next_frames, self.capture)

    def saveCapture(self) -> None:
        self.camera.saveCapture()

    def ImgRect(
        self,
        x1: int,
        y1: int,
        x2: int,
        y2: int,
        outline: str,
        tag: str | int,
        ms: int,
        flag: bool = True,
    ) -> None:
        ratio_x = float(self.show_size[0] / self.camera.capture_size[0])
        ratio_y = float(self.show_size[1] / self.camera.capture_size[1])
        self.create_rectangle(
            (x1 - 1.0) * ratio_x,
            (y1 - 1.0) * ratio_y,
            (x2 + 1.0) * ratio_x,
            (y2 + 1.0) * ratio_y,
            width=4.5,
            outline="white",
            tag=tag,
        )
        self.create_rectangle(
            x1 * ratio_x,
            y1 * ratio_y,
            x2 * ratio_x,
            y2 * ratio_y,
            width=2.5,
            outline=outline,
            tag=tag,
        )
        if flag:
            self.after(ms, self.deleteImageRect, tag)

    def deleteImageRect(self, tag: int | str) -> None:
        self.delete(tag)

    def ImgText(
        self,
        x1: int,
        y1: int,
        txt: str,
        tag: int | str,
        ms: int,
        ft: tuple[str, int] = ("UD デジタル 教科書体 NP-B", 20),
        color: str = "black",
        flag: bool = True,
    ) -> None:
        ratio_x = float(self.show_size[0] / self.camera.capture_size[0])
        ratio_y = float(self.show_size[1] / self.camera.capture_size[1])
        str_len = 0.3528 * ft[1] * len(txt)  # 1[pt] = 0.3528[mm]
        self.create_text(
            ((x1 - 1.0) * ratio_x) + str_len,
            (y1 - 1.0) * ratio_y,
            text=txt,
            font=ft,
            tag=tag,
            fill=color,
        )
        if flag:
            self.after(ms, self.deleteImageText, tag)

    def deleteImageText(self, tag: int | str) -> None:
        self.delete(tag)

    def BindLeftClick(self) -> None:
        self.bind("<ButtonPress-1>", lambda ev: self.mouseLeftPress(ev, self.keys_))
        self.bind("<Button1-Motion>", lambda ev: self.mouseLeftPressing(ev, self.keys_))
        self.bind("<ButtonRelease-1>", lambda ev: self.mouseLeftRelease(self.keys_))  # noqa: ARG005
        self._logger.debug("Bind <ButtonPress-1>")
        self._logger.debug("Bind <Button1-Motion>")
        self._logger.debug("Bind <ButtonRelease-1>")

    def BindRightClick(self) -> None:
        self.bind("<ButtonPress-3>", lambda ev: self.mouseRightPress(ev, self.keys_))
        self.bind(
            "<Button3-Motion>",
            lambda ev: self.mouseRightPressing(ev, self.keys_),
        )
        self.bind("<ButtonRelease-3>", lambda ev: self.mouseRightRelease(self.keys_))  # noqa: ARG005
        self._logger.debug("Bind <ButtonPress-3>")
        self._logger.debug("Bind <Button3-Motion>")
        self._logger.debug("Bind <ButtonRelease-3>")

    def UnbindLeftClick(self) -> None:
        self.unbind("<ButtonPress-1>")
        self.unbind("<Button1-Motion>")
        self.unbind("<ButtonRelease-1>")
        self._logger.debug("Unbind <ButtonPress-1>")
        self._logger.debug("Unbind <Button1-Motion>")
        self._logger.debug("Unbind <ButtonRelease-1>")

    def UnbindRightClick(self) -> None:
        self.unbind("<ButtonPress-3>")
        self.unbind("<Button3-Motion>")
        self.unbind("<ButtonRelease-3>")
        self._logger.debug("Unbind <ButtonPress-3>")
        self._logger.debug("Unbind <Button3-Motion>")
        self._logger.debug("Unbind <ButtonRelease-3>")


# GUI of switch controller simulator
class ControllerGUI:
    def __init__(self, root: Misc, ser: Sender) -> None:
        from Commands import UnitCommand  # noqa: PLC0415

        self.UnitCommand: Final = UnitCommand
        self._logger: Final[Logger] = getLogger(__name__)
        self._logger.addHandler(NullHandler())
        self._logger.setLevel(DEBUG)
        self._logger.propagate = True

        self.window: Final[Toplevel] = tk.Toplevel(root)
        self.window.title("Switch Controller Simulator")
        root_geometry = root.geometry().split("+")
        root_x = int(root_geometry[1])
        root_y = int(root_geometry[2])
        self.window.geometry(f"{600}x{300}{250 + root_x:+d}{125 + root_y:+d}")
        self.window.resizable(False, False)

        joycon_L_color = "#95f1ff"
        joycon_R_color = "#ff6b6b"

        joycon_L_frame = tk.Frame(
            self.window,
            width=300,
            height=300,
            relief="flat",
            bg=joycon_L_color,
        )
        joycon_R_frame = tk.Frame(
            self.window,
            width=300,
            height=300,
            relief="flat",
            bg=joycon_R_color,
        )
        hat_frame = tk.Frame(joycon_L_frame, relief="flat", bg=joycon_L_color)
        abxy_frame = tk.Frame(joycon_R_frame, relief="flat", bg=joycon_R_color)

        # ABXY
        tk.Button(
            abxy_frame,
            text="A",
            command=lambda: self.UnitCommand.A().start(ser),
        ).grid(row=1, column=2)
        tk.Button(
            abxy_frame,
            text="B",
            command=lambda: self.UnitCommand.B().start(ser),
        ).grid(row=2, column=1)
        tk.Button(
            abxy_frame,
            text="X",
            command=lambda: self.UnitCommand.X().start(ser),
        ).grid(row=0, column=1)
        tk.Button(
            abxy_frame,
            text="Y",
            command=lambda: self.UnitCommand.Y().start(ser),
        ).grid(row=1, column=0)
        abxy_frame.place(relx=0.2, rely=0.3)

        # HAT
        tk.Button(
            hat_frame,
            text="UP",
            command=lambda: self.UnitCommand.UP().start(ser),
        ).grid(row=0, column=1)
        tk.Button(
            hat_frame,
            text="",
            command=lambda: self.UnitCommand.UP_RIGHT().start(ser),
        ).grid(row=0, column=2)
        tk.Button(
            hat_frame,
            text="RIGHT",
            command=lambda: self.UnitCommand.RIGHT().start(ser),
        ).grid(row=1, column=2)
        tk.Button(
            hat_frame,
            text="",
            command=lambda: self.UnitCommand.DOWN_RIGHT().start(ser),
        ).grid(row=2, column=2)
        tk.Button(
            hat_frame,
            text="DOWN",
            command=lambda: self.UnitCommand.DOWN().start(ser),
        ).grid(row=2, column=1)
        tk.Button(
            hat_frame,
            text="",
            command=lambda: self.UnitCommand.DOWN_LEFT().start(ser),
        ).grid(row=2, column=0)
        tk.Button(
            hat_frame,
            text="LEFT",
            command=lambda: self.UnitCommand.LEFT().start(ser),
        ).grid(row=1, column=0)
        tk.Button(
            hat_frame,
            text="",
            command=lambda: self.UnitCommand.UP_LEFT().start(ser),
        ).grid(row=0, column=0)
        hat_frame.place(relx=0.2, rely=0.6)

        # L side
        tk.Button(
            joycon_L_frame,
            text="L",
            width=20,
            command=lambda: self.UnitCommand.L().start(ser),
        ).place(x=30, y=30)
        tk.Button(
            joycon_L_frame,
            text="ZL",
            width=20,
            command=lambda: self.UnitCommand.ZL().start(ser),
        ).place(x=30, y=0)
        tk.Button(
            joycon_L_frame,
            text="LCLICK",
            width=7,
            command=lambda: self.UnitCommand.LCLICK().start(ser),
        ).place(x=120, y=120)
        tk.Button(
            joycon_L_frame,
            text="MINUS",
            width=5,
            command=lambda: self.UnitCommand.MINUS().start(ser),
        ).place(x=220, y=70)
        tk.Button(
            joycon_L_frame,
            text="CAP",
            width=5,
            command=lambda: self.UnitCommand.CAPTURE().start(ser),
        ).place(x=200, y=270)

        # R side
        tk.Button(
            joycon_R_frame,
            text="R",
            width=20,
            command=lambda: self.UnitCommand.R().start(ser),
        ).place(x=120, y=30)
        tk.Button(
            joycon_R_frame,
            text="ZR",
            width=20,
            command=lambda: self.UnitCommand.ZR().start(ser),
        ).place(x=120, y=0)
        tk.Button(
            joycon_R_frame,
            text="RCLICK",
            width=7,
            command=lambda: self.UnitCommand.RCLICK().start(ser),
        ).place(x=120, y=205)
        tk.Button(
            joycon_R_frame,
            text="PLUS",
            width=5,
            command=lambda: self.UnitCommand.PLUS().start(ser),
        ).place(x=35, y=70)
        tk.Button(
            joycon_R_frame,
            text="HOME",
            width=5,
            command=lambda: self.UnitCommand.HOME().start(ser),
        ).place(x=50, y=270)

        joycon_L_frame.grid(row=0, column=0)
        joycon_R_frame.grid(row=0, column=1)

        # button style settings
        for button in abxy_frame.winfo_children():
            self.applyButtonSetting(button)
        for button in hat_frame.winfo_children():
            self.applyButtonSetting(button)
        for button in [
            b for b in joycon_L_frame.winfo_children() if type(b) is tk.Button
        ]:
            self.applyButtonColor(button)
        for button in [
            b for b in joycon_R_frame.winfo_children() if type(b) is tk.Button
        ]:
            self.applyButtonColor(button)

        self._logger.debug("Create GUI controller")

    def applyButtonSetting(self, button: Misc) -> None:
        button["width"] = 7
        self.applyButtonColor(button)

    def applyButtonColor(self, button: Misc) -> None:
        button["bg"] = "#343434"
        button["fg"] = "#fff"

    def bind(self, event: str, func: Callable[[Event[Misc]], None]) -> None:
        self.window.bind(event, func)

    def protocol(self, event: str, func: Callable[..., None]) -> None:
        self.window.protocol(event, func)

    def focus_force(self) -> None:
        self.window.focus_force()

    def destroy(self) -> None:
        self.window.destroy()
        self._logger.debug("GUI controller destroyed")


# To avoid the error says 'ScrolledText' object has no attribute 'flush'
class MyScrolledText(ScrolledText):
    def flush(self) -> None:
        pass
