from __future__ import annotations

from logging import DEBUG, NullHandler, getLogger
from typing import TYPE_CHECKING

from Controller.ControllerBase import ControllerBase

if TYPE_CHECKING:
    from logging import Logger
    from typing import Final


class ProController(ControllerBase):
    NAME: Final = "Pro Controller"

    def __init__(self) -> None:
        super().__init__()

        self._logger: Final[Logger] = getLogger(__name__)
        self._logger.addHandler(NullHandler())
        self._logger.setLevel(DEBUG)
        self._logger.propagate = True
