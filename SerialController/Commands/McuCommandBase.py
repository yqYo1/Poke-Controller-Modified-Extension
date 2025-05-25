# -*- coding: utf-8 -*-
from __future__ import annotations

from typing import TYPE_CHECKING, Final

from Commands import CommandBase

if TYPE_CHECKING:
    from collections.abc import Callable

    from Commands.Sender import Sender


# MCU command
class McuCommand(CommandBase.Command):
    def __init__(self, sync_name: str) -> None:
        super().__init__()
        self.sync_name: Final = sync_name
        self.postProcess: Callable[[], None] | None = None

    def start(self, ser: Sender, postProcess: Callable[[], None]) -> None:
        ser.writeRow(self.sync_name)
        self.isRunning: bool = True
        self.postProcess = postProcess

    def end(self, ser: Sender) -> None:
        ser.writeRow("end")
        self.isRunning = False
        if self.postProcess is not None:
            self.postProcess()
