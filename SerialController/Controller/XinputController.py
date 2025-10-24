from __future__ import annotations

from logging import DEBUG, NullHandler, getLogger
from typing import TYPE_CHECKING

from Controller.ControllerBase import ControllerBase

if TYPE_CHECKING:
    from logging import Logger
    from typing import Final

    import pygame


class XinputController(ControllerBase):
    NAME: Final = "Xinput"

    def __init__(self) -> None:
        super().__init__()
        self.axis_dict: dict[int, str] = {
            5: "ZL",
            4: "ZR",
        }

        self.button_dict: dict[int, str] = {
            0: "A",
            1: "B",
            3: "X",
            4: "Y",
            6: "L",
            7: "R",
            10: "MINUS",
            11: "PLUS",
            12: "HOME",
            13: "LSTICK",
            14: "RSTICK",
        }

        self.axis_dict_shift: dict[int, int] = {
            5: 8,
            4: 9,
        }

        self.button_dict_shift: dict[int, int] = {
            0: 4,
            1: 3,
            3: 5,
            4: 2,
            6: 6,
            7: 7,
            10: 10,
            12: 14,
            11: 11,
            13: 12,
            14: 13,
        }

        self.hat_dict: dict[int, int] = {
            0: 5,  # down-left
            1: 4,  # down
            2: 3,  # down-right
            3: 6,  # left
            4: 8,  # center
            5: 2,  # right
            6: 7,  # up-left
            7: 0,  # up
            8: 1,  # up-right
        }

        self.bits_16: int = 0
        self._logger: Logger = getLogger(__name__)
        self._logger.addHandler(NullHandler())
        self._logger.setLevel(DEBUG)
        self._logger.propagate = True
        self.flag_print: bool = False
        self.hat_status: int = 4

    def event_check(
        self,
        events: list[pygame.event.EventType],
    ) -> None:
        cnt = 0
        for _, event in enumerate(events):
            if event.type == 1536:
                if (axis_code := event.dict["axis"]) in self.axis_dict_shift:
                    if cnt & 0x1 == 0:
                        self.flag_print = True
                        if self.map_axis(event.dict["value"]) >= 64:
                            self.bits_16 = self.bits_16 | (
                                1 << self.axis_dict_shift[axis_code]
                            )
                        else:
                            self.bits_16 = self.bits_16 & ~(
                                1 << self.axis_dict_shift[axis_code]
                            )
                    cnt += 1
            elif event.type == 1538:
                self.flag_print = True
                self.hat_status = (
                    event.dict["value"][0] + 1 + ((event.dict["value"][1] + 1) * 3)
                )
            elif event.type == 1539:
                self.flag_print = True
                if (btn_code := event.dict["button"]) in self.button_dict_shift:
                    self.bits_16 = self.bits_16 | (
                        1 << self.button_dict_shift[btn_code]
                    )
            elif event.type == 1540:
                self.flag_print = True
                if (btn_code := event.dict["button"]) in self.button_dict_shift:
                    self.bits_16 = self.bits_16 & ~(
                        1 << self.button_dict_shift[btn_code]
                    )
