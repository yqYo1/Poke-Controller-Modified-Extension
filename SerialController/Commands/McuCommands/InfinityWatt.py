# -*- coding: utf-8 -*-
from __future__ import annotations

from typing import TYPE_CHECKING

from Commands.McuCommandBase import McuCommand

if TYPE_CHECKING:
    from typing import Final


# Mash A button
class InfinityWatt(McuCommand):
    NAME: Final[str] = "無限ワット"

    def __init__(self, sync_name: str = "inf_watt") -> None:
        super().__init__(sync_name)
