# -*- coding: utf-8 -*-
from __future__ import annotations

from typing import TYPE_CHECKING

from Commands.McuCommandBase import McuCommand

if TYPE_CHECKING:
    from typing import Final


# Mash A button
class AutoLeague(McuCommand):
    NAME: Final[str] = "自動リーグ周回"

    def __init__(self, sync_name: str = "auto_league") -> None:
        super().__init__(sync_name)
