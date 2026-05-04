from typing import ClassVar

from pokecon._meta import CommandMeta


class PythonCommand(metaclass=CommandMeta):
    __is_interface__ = True

    NAME: ClassVar[str] = ""
    TAGS: ClassVar = None
    stdout_destination: ClassVar[str] = "1"

    def do(self) -> None:
        raise NotImplementedError("do() must be overridden by user script")


class ImageProcPythonCommand(PythonCommand):
    __is_interface__ = True

    template_path_name: ClassVar[str] = "./Template/"
    capture_path_name: ClassVar[str] = "./Captures/"
