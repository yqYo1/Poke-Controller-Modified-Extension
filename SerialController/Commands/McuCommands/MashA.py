# -*- coding: utf-8 -*-
from __future__ import annotations

from typing import TYPE_CHECKING

from Commands.McuCommandBase import McuCommand

if TYPE_CHECKING:
    from typing import Final


# Mash A button
class Mash_A(McuCommand):
    NAME: Final[str] = "A連打"

    def __init__(self, sync_name: str = "mash_a") -> None:
        super().__init__(sync_name)
