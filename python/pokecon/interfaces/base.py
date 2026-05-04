from typing import ClassVar


class Command:
    __is_interface__ = True

    NAME: ClassVar[str] = ""
