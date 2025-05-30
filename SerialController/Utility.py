# -*- coding: utf-8 -*-
from __future__ import annotations

import importlib
import inspect
import os
from glob import glob
from logging import DEBUG, NullHandler, getLogger
from os.path import join, relpath
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from logging import Logger
    from types import ModuleType
    from typing import Final

logger: Final[Logger] = getLogger(__name__)
logger.addHandler(NullHandler())
logger.setLevel(DEBUG)
logger.propagate = True


def ospath(path: str) -> str:
    return path.replace("/", os.sep)


# Show all file names under the directory
def browseFileNames(
    path: str = ".",
    ext: str = "",
    recursive: bool = True,
    name_only: bool = True,
) -> list[str]:
    search_path = join(path, "**") if recursive else path
    search_path = join(search_path, "*" + ext)

    if name_only:
        return [relpath(f, path) for f in glob(search_path, recursive=recursive)]
    return glob(search_path, recursive=recursive)


def getClassesInModule(module: object) -> list[type]:
    return [members[1] for members in inspect.getmembers(module, inspect.isclass)]


def getModuleNames(base_path: str) -> list[str]:
    filenames = browseFileNames(path=base_path, ext=".py", name_only=False)
    return [name[:-3].replace(os.sep, ".") for name in filenames]


def importAllModules(
    base_path: str,
    mod_names: list[str] | None = None,
) -> list[ModuleType]:
    modules: list[ModuleType] = []
    for name in getModuleNames(base_path) if mod_names is None else mod_names:
        logger.debug(f"Import module: {name}")
        modules.append(importlib.import_module(name))

    return modules
