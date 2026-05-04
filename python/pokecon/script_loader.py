"""Modern script loader supporting multiple directories and XDG paths.

Replaces the legacy CommandLoader with a more flexible system that:
- Searches multiple script directories (primary + fallbacks)
- Supports XDG Base Directory Specification
- Allows runtime reload without restart
- Provides better error messages

Usage
-----
>>> from pokecon.script_loader import ScriptLoader
>>> loader = ScriptLoader()
>>> commands = loader.load_all()
>>> loader.reload()
"""

from __future__ import annotations

import importlib
import sys
from logging import DEBUG, NullHandler, getLogger
from pathlib import Path
from types import ModuleType
from typing import TYPE_CHECKING, Final

from pokecon.scripts_dir import get_scripts_dir_with_fallbacks

if TYPE_CHECKING:
    from logging import Logger

    from Commands.CommandBase import Command

logger: Final[Logger] = getLogger(__name__)
logger.addHandler(NullHandler())
logger.setLevel(DEBUG)
logger.propagate = True


class ScriptLoader:
    """Load user scripts from multiple directories.

    Parameters
    ----------
    script_dirs
        List of directories to search for scripts. If ``None``, uses
        :func:`get_scripts_dir_with_fallbacks`.
    base_class
        The base class that loaded scripts must inherit from.
    """

    def __init__(
        self,
        script_dirs: list[Path] | None = None,
        base_class: type | None = None,
    ) -> None:
        self.script_dirs: list[Path] = script_dirs or get_scripts_dir_with_fallbacks()
        self.base_class: type | None = base_class
        self._modules: dict[str, ModuleType] = {}
        self._loaded_paths: set[Path] = set()

    def load_all(self) -> list[type]:
        """Load all scripts from all configured directories.

        Returns
        -------
        list[type]
            List of command classes found in the script directories.
        """
        classes: list[type] = []

        for script_dir in self.script_dirs:
            if not script_dir.exists():
                logger.warning(f"Script directory does not exist: {script_dir}")
                continue

            dir_classes = self._load_from_directory(script_dir)
            classes.extend(dir_classes)

        return classes

    def _load_from_directory(self, script_dir: Path) -> list[type]:
        """Load scripts from a single directory."""
        classes: list[type] = []

        # Find all .py files (non-recursive for safety, or recursive if needed)
        py_files = sorted(script_dir.glob("*.py"))

        for py_file in py_files:
            if py_file.name.startswith("_"):
                continue  # Skip private modules

            try:
                module = self._load_module(py_file)
                if module is None:
                    continue

                file_classes = self._extract_classes(module)
                classes.extend(file_classes)
            except Exception as e:
                logger.warning(f"Failed to load {py_file}: {e}")

        return classes

    def _load_module(self, py_file: Path) -> ModuleType | None:
        """Load a single Python file as a module."""
        module_name = f"_pokecon_user_script.{py_file.stem}"

        # Avoid reloading if already loaded
        if module_name in sys.modules:
            return sys.modules[module_name]

        spec = importlib.util.spec_from_file_location(module_name, py_file)
        if spec is None or spec.loader is None:
            return None

        module = importlib.util.module_from_spec(spec)
        sys.modules[module_name] = module
        spec.loader.exec_module(module)

        self._modules[module_name] = module
        self._loaded_paths.add(py_file)

        return module

    def _extract_classes(self, module: ModuleType) -> list[type]:
        """Extract command classes from a loaded module."""
        classes: list[type] = []

        for name in dir(module):
            obj = getattr(module, name)
            if not isinstance(obj, type):
                continue

            # Check if it's a command class
            if self.base_class is not None:
                try:
                    if not issubclass(obj, self.base_class):
                        continue
                except TypeError:
                    continue

            # Must have NAME attribute
            if not isinstance(getattr(obj, "NAME", None), str):
                continue

            classes.append(obj)

        return classes

    def reload(self) -> list[type]:
        """Reload all scripts.

        Clears the module cache and re-imports everything.
        """
        # Clear cached modules
        for name in list(self._modules.keys()):
            sys.modules.pop(name, None)
        self._modules.clear()
        self._loaded_paths.clear()

        return self.load_all()

    def get_loaded_modules(self) -> dict[str, ModuleType]:
        """Return all currently loaded modules."""
        return dict(self._modules)

    def get_loaded_paths(self) -> set[Path]:
        """Return all loaded file paths."""
        return set(self._loaded_paths)


__all__ = ["ScriptLoader"]
