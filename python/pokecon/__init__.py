import sys
import types

from pokecon.commands import ImageProcPythonCommand, PythonCommand

# --- Import Hack ---

_mod_python_cmd = types.ModuleType("Commands.PythonCommandBase")
_mod_python_cmd.PythonCommand = PythonCommand
_mod_python_cmd.ImageProcPythonCommand = ImageProcPythonCommand
sys.modules["Commands.PythonCommandBase"] = _mod_python_cmd
