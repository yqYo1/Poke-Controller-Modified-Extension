"""pokecon - Poke-Controller Python package (Rust-backed compatibility layer).

This package provides backward-compatible wrappers so that existing user scripts
(which import from ``Commands.Keys`` and ``Commands.PythonCommandBase``) continue
to work while the architecture migrates to a Rust core.

Import hacks are applied at package load time via ``_patch_modules()``.
"""

from __future__ import annotations

import sys
import types

# ---------------------------------------------------------------------------
# Import our wrapper implementations
# ---------------------------------------------------------------------------
from pokecon.commands import ImageProcPythonCommand, PythonCommand
from pokecon.keys import (
    Button,
    Direction,
    Hat,
    KeyPress,
    SendFormat,
    Stick,
    Tilt,
    Touchscreen,
    conversion_3ds_controller_button,
    conversion_default_button,
    convert_hat_3ds_controller,
    convert_hat_default,
    direction_center,
    direction_max,
    direction_min,
    NEUTRAL,
)


def _patch_modules() -> None:
    """Patch ``Commands.PythonCommandBase`` and ``Commands.Keys`` so that
    existing user scripts (e.g. AutoLeague, MashA) can import from their
    traditional paths without modification.
    """

    # -----------------------------------------------------------------------
    # 1. Patch Commands.PythonCommandBase
    # -----------------------------------------------------------------------
    mod_python_cmd = types.ModuleType("Commands.PythonCommandBase")
    mod_python_cmd.PythonCommand = PythonCommand
    mod_python_cmd.ImageProcPythonCommand = ImageProcPythonCommand
    mod_python_cmd.StopThread = __import__(
        "pokecon.commands", fromlist=["StopThread"]
    ).StopThread
    mod_python_cmd.pausedecorator = __import__(
        "pokecon.commands", fromlist=["pausedecorator"]
    ).pausedecorator
    mod_python_cmd.convertCv2Format = __import__(
        "pokecon.commands", fromlist=["convertCv2Format"]
    ).convertCv2Format
    mod_python_cmd.generateRandomCharacter = __import__(
        "pokecon.commands", fromlist=["generateRandomCharacter"]
    ).generateRandomCharacter
    sys.modules["Commands.PythonCommandBase"] = mod_python_cmd

    # -----------------------------------------------------------------------
    # 2. Patch Commands.Keys
    # -----------------------------------------------------------------------
    mod_keys = types.ModuleType("Commands.Keys")
    mod_keys.Button = Button
    mod_keys.Hat = Hat
    mod_keys.Stick = Stick
    mod_keys.Tilt = Tilt
    mod_keys.Direction = Direction
    mod_keys.Touchscreen = Touchscreen
    mod_keys.KeyPress = KeyPress
    mod_keys.SendFormat = SendFormat
    mod_keys.conversion_default_button = conversion_default_button
    mod_keys.conversion_3ds_controller_button = conversion_3ds_controller_button
    mod_keys.convert_hat_default = convert_hat_default
    mod_keys.convert_hat_3ds_controller = convert_hat_3ds_controller
    mod_keys.direction_min = direction_min
    mod_keys.direction_center = direction_center
    mod_keys.direction_max = direction_max
    mod_keys.NEUTRAL = NEUTRAL
    sys.modules["Commands.Keys"] = mod_keys

    # -----------------------------------------------------------------------
    # 3. Ensure Commands package exists
    # -----------------------------------------------------------------------
    if "Commands" not in sys.modules:
        sys.modules["Commands"] = types.ModuleType("Commands")


_patch_modules()

__all__ = [
    "PythonCommand",
    "ImageProcPythonCommand",
    "Button",
    "Hat",
    "Stick",
    "Tilt",
    "Direction",
    "Touchscreen",
    "KeyPress",
    "SendFormat",
]
