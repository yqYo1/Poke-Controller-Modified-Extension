from __future__ import annotations

import importlib
import sys
from logging import DEBUG, NullHandler, getLogger
from typing import TYPE_CHECKING

import Utility as util

if TYPE_CHECKING:
    from logging import Logger
    from types import ModuleType
    from typing import Final, TypeVar

    from Commands.CommandBase import Command

    CommandLike = TypeVar("CommandLike", bound=Command)

logger: Final[Logger] = getLogger(__name__)
logger.addHandler(NullHandler())
logger.setLevel(DEBUG)
logger.propagate = True


class CommandLoader[CommandLike]:
    def __init__(self, base_path: str, base_class: type[CommandLike]) -> None:
        self.path: str = base_path
        self.base_type: Final[type[CommandLike]] = base_class
        self.modules: list[ModuleType] = []

    def load(self) -> list[type[CommandLike]]:
        if not self.modules:  # load if empty
            self.modules = util.importAllModules(self.path)

        # return command class types
        return self.getCommandClasses()

    def reload(self) -> list[type[CommandLike]]:
        loaded_module_dic = {mod.__name__: mod for mod in self.modules}
        cur_module_names = util.getModuleNames(self.path)

        # Load only not loaded modules
        not_loaded_module_names = list(
            set(cur_module_names) - set(loaded_module_dic.keys()),
        )
        if len(not_loaded_module_names) > 0:
            self.modules.extend(
                util.importAllModules(self.path, not_loaded_module_names),
            )

        # Reload commands except deleted ones
        for mod_name in list(set(cur_module_names) & set(loaded_module_dic.keys())):
            importlib.reload(loaded_module_dic[mod_name])

        # Unload deleted commands
        for mod_name in list(set(loaded_module_dic.keys()) - set(cur_module_names)):
            self.modules.remove(loaded_module_dic[mod_name])
            sys.modules.pop(
                loaded_module_dic[mod_name].__name__,
            )  # Un-import module forcefully

        # return command class types
        return self.getCommandClasses()

    def getCommandClasses(self) -> list[type[CommandLike]]:
        classes: list[type[CommandLike]] = []
        for mod in self.modules:
            # extract module of having "NAME"
            class_list: list[type[CommandLike]] = [
                c
                for c in util.getClassesInModule(mod)
                if issubclass(c, self.base_type)
                and isinstance(getattr(c, "NAME", False), str)
            ]

            # make tags of directory name
            for c in class_list:
                dir_name = "/".join(mod.__name__.split(".")[2:])
                dir_tags = ["@" + t for t in mod.__name__.split(".")[2:-1]]

                # add tags of directory name
                tags = getattr(c, "TAGS", False)
                processed_tags: list[str]
                if tags:
                    if isinstance(tags, list):
                        logger.debug(f"TAGS name add: {dir_tags}")
                        processed_tags = [tag for tag in tags if isinstance(tag, str)]  # pyright: ignore[reportUnknownVariableType]
                    elif isinstance(tags, str):
                        processed_tags = [tags]
                    else:
                        processed_tags = []
                else:
                    processed_tags = []
                processed_tags.extend(dir_tags)
                logger.debug(f"TAGS name add: {dir_tags}")
                setattr(c, "TAGS", processed_tags)  # noqa: B010

                # rename NAME
                c.NAME = f"{c.NAME} ({dir_name})"  # pyright: ignore[reportAttributeAccessIssue, reportUnknownMemberType]
                classes.append(c)

        return classes
