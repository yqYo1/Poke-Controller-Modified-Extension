from __future__ import annotations

import os
import threading
from functools import cache
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from typing import Final, Self

IS_COMPILED: Final[bool] = "__compiled__" in globals()


def _get_base_path() -> str:
    if not IS_COMPILED:
        return os.path.dirname(os.path.abspath(__file__))
    return os.path.dirname(os.path.abspath(__file__))


class FileHandler:
    _instance: Self | None = None
    _lock: threading.Lock = threading.Lock()
    # _initialized: bool = False
    BASE_PATH: Final[str] = _get_base_path()

    def __new__(cls) -> Self:
        if cls._instance is None:
            with cls._lock:
                if cls._instance is None:
                    cls._instance = super().__new__(cls)

        return cls._instance

    # def __init__(self) -> None:
    #     if not self._initialized:
    #         with self._lock:
    #             if not self._initialized:
    #                 self._initialized = True

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
    @cache
    def get_settings_path(profile: str = "default") -> str:
        """
        Returns the absolute path to the settings file.
        """
        return os.path.join(FileHandler.get_profile_path(profile), "settings.ini")

    # フォルダ作成は初回のみでいいので副作用があるがメモ化してる
    @staticmethod
    @cache
    def get_profile_path(profile: str = "default") -> str:
        """
        Returns the absolute path to the profile directory.
        Create the folder if it is the first time and does not exist
        """
        profile_path = os.path.join(FileHandler.BASE_PATH, "profiles", profile)
        if not os.path.exists(profile_path):
            os.makedirs(profile_path, mode=0o755, exist_ok=False)
        elif os.path.isfile(profile_path):
            msg = f"{profile_path} is a file, not a directory.\n"
            raise FileExistsError(msg)
        return profile_path
