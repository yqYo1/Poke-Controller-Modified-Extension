from __future__ import annotations

import tkinter as tk
from logging import DEBUG, NullHandler, getLogger
from typing import TYPE_CHECKING

from Commands import UnitCommand

if TYPE_CHECKING:
    from collections.abc import Callable
    from logging import Logger
    from tkinter import Event, Misc, Toplevel
    from typing import Final

    from Commands.Sender import Sender


class ControllerGUI:
    """
    GUI of switch controller simulator
    """

    def __init__(self, root: Misc, ser: Sender) -> None:
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
