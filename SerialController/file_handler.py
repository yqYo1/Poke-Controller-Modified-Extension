from __future__ import annotations

import os
import threading
from functools import cache
from logging import DEBUG, NullHandler, getLogger
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from typing import ClassVar, Final, Self

IS_COMPILED: Final[bool] = "__compiled__" in globals()


def _get_base_path() -> str:
    base_path = os.path.dirname(os.path.abspath(__file__))
    os.chdir(base_path)
    return base_path


class FileHandler:
    _instance: Self | None = None
    _lock: threading.Lock = threading.Lock()
    BASE_PATH: Final[str] = _get_base_path()
    PROFILE: ClassVar[str] = "default"
    _logger: Final = getLogger(__name__)
    _logger.addHandler(NullHandler())
    _logger.setLevel(DEBUG)
    _logger.propagate = True

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
            FileHandler._logger.debug(f"make directory: '{profile_path}'")
        elif os.path.isfile(profile_path):
            msg = f"{profile_path} is a file, not a directory.\n"
            raise FileExistsError(msg)
        return profile_path
