from __future__ import annotations

import os
import threading
from functools import cache
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from typing import Final, Self

IS_COMPILED: Final[bool] = "__compiled__" in globals()


def _get_base_path() -> str:
    base_path = os.path.dirname(os.path.abspath(__file__))
    os.chdir(base_path)
    return base_path
    # if not IS_COMPILED:
    #     return os.path.dirname(os.path.abspath(__file__))
    # return os.path.dirname(os.path.abspath(__file__))


class FileHandler:
    _instance: Self | None = None
    _lock: threading.Lock = threading.Lock()
    # _initialized: bool = False
    BASE_PATH: Final[str] = _get_base_path()
    PROFILE: str = "default"

    def __new__(cls) -> Self:
        if cls._instance is None:
            with cls._lock:
                if cls._instance is None:
                    cls._instance = super().__new__(cls)

        return cls._instance

    @staticmethod
    @cache
    def get_asset_path(file_name: str | None = None) -> str:
        """
        Returns the absolute path to the asset file.
        """
        if file_name is None:
            return os.path.join(FileHandler.BASE_PATH, "assets")
        return os.path.join(FileHandler.BASE_PATH, "assets", file_name)

    @staticmethod
    def get_configs_path(filename: str = "settings.ini") -> str:
        """
        Returns the absolute path to the settings file.
        """
        return os.path.join(FileHandler.get_profile_path(), filename)

    @staticmethod
    def get_profile_path() -> str:
        """
        Returns the absolute path to the profile directory.
        """
        profile_path = os.path.join(
            FileHandler.BASE_PATH,
            "profiles",
            FileHandler.PROFILE,
        )
        if not os.path.exists(profile_path):
            os.makedirs(profile_path, mode=0o755, exist_ok=False)
            print(f"mkdir: '{profile_path}")
        elif os.path.isfile(profile_path):
            msg = f"{profile_path} is a file, not a directory.\n"
            raise FileExistsError(msg)
        return profile_path

    # @staticmethod
    # @cache
    # def get_commands_path() -> str:
    #     """
    #     Returns the absolute path to the commands directory.
    #     """
    #     return os.path.realpath(
    #         os.path.join(FileHandler.BASE_PATH, "Commands/PythonCommands"),
    #     )
