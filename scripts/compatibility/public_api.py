"""Static public API surface extraction and comparison for rolling candidates.

Ported from the upstream ``SerialController/tests/public_api.py`` approach: extract the
public API of the ``Commands`` modules as AST signatures (functions, classes, methods,
attributes, variables) and compare a candidate commit against its baseline. Removed
names, changed argument lists, and newly required arguments are breaking; compatible
additions (new names, optional arguments) are reported as unrecorded additions.

The comparison is static and never imports the compared code; runtime behavior remains
covered by the corpus execution gate.
"""

from __future__ import annotations

import ast
from typing import TYPE_CHECKING, Never

from scripts.compatibility.inventory import (
    decode_python_source,
    require_mapping,
    require_sequence,
)

if TYPE_CHECKING:
    from pathlib import Path

SERIAL_CONTROLLER_DIRECTORY = "SerialController"

# Compared files, relative to SerialController/, with the restricted name set when the
# whole module is not part of the guarantee (SwitchKeyboard exposes only KEY upstream).
PUBLIC_API_FILES: tuple[tuple[str, tuple[str, ...] | None], ...] = (
    ("Commands/CommandBase.py", None),
    ("Commands/PythonCommandBase.py", None),
    ("Commands/McuCommandBase.py", None),
    ("Commands/Keys.py", None),
    ("Commands/SwitchKeyboard.py", ("KEY",)),
)

_FUNCTION_NODES = (ast.FunctionDef, ast.AsyncFunctionDef)
_PARAMETER_LISTS = ("functions", "methods")
_NAME_LISTS = ("bases", "attributes", "variables")
_KINDS = (*_PARAMETER_LISTS, *_NAME_LISTS)


def invalid_value(message: str) -> Never:
    raise ValueError(message)


def _public(name: str) -> bool:
    return not name.startswith("_")


def _parameter_token(name: str, default: ast.expr | None) -> str:
    return name if default is None else f"{name}={ast.unparse(default)}"


def _parameters(node: ast.FunctionDef | ast.AsyncFunctionDef) -> list[str]:
    """Argument tokens exactly as written (decorators must not hide the real signature)."""
    args = node.args
    positional = [*args.posonlyargs, *args.args]
    defaults: list[ast.expr | None] = [None] * (
        len(positional) - len(args.defaults)
    ) + list(args.defaults)
    tokens: list[str] = []
    for index, (argument, default) in enumerate(zip(positional, defaults, strict=True)):
        tokens.append(_parameter_token(argument.arg, default))
        if args.posonlyargs and index == len(args.posonlyargs) - 1:
            tokens.append("/")
    if args.vararg:
        tokens.append("*" + args.vararg.arg)
    elif args.kwonlyargs:
        tokens.append("*")
    for argument, default in zip(args.kwonlyargs, args.kw_defaults, strict=True):
        tokens.append(_parameter_token(argument.arg, default))
    if args.kwarg:
        tokens.append("**" + args.kwarg.arg)
    return tokens


def _targets(statement: ast.stmt) -> list[ast.expr]:
    if isinstance(statement, ast.Assign):
        return list(statement.targets)
    if isinstance(statement, ast.AnnAssign):
        return [statement.target]
    return []


def _assigned_names(statement: ast.stmt) -> list[str]:
    """Names created by an assignment (``A = 1``, ``A, B = 1, 2``)."""
    names: list[str] = []
    for target in _targets(statement):
        names.extend(
            node.id
            for node in ast.walk(target)
            if isinstance(node, ast.Name) and isinstance(node.ctx, ast.Store)
        )
    return names


def _assigned_attributes(
    statement: ast.stmt, owner: str | None = None
) -> list[tuple[str, str]]:
    """``name.attribute = value`` assignments; ``owner`` restricts them to one name."""
    found: list[tuple[str, str]] = []
    for node in ast.walk(statement):
        if not (isinstance(node, ast.Attribute) and isinstance(node.ctx, ast.Store)):
            continue
        if isinstance(node.value, ast.Name) and owner in (None, node.value.id):
            found.append((node.value.id, node.attr))
    return found


def _class_entry(node: ast.ClassDef) -> dict[str, object]:
    methods: dict[str, list[str]] = {}
    attributes: set[str] = set()
    for item in node.body:
        if isinstance(item, _FUNCTION_NODES):
            if item.name == "__init__":
                methods[item.name] = _parameters(item)
                # Instance variables created in the constructor (``self.keys = ...``).
                attributes.update(
                    attribute for _, attribute in _assigned_attributes(item, "self")
                )
            elif _public(item.name):
                methods[item.name] = _parameters(item)
        else:
            attributes.update(_assigned_names(item))
    return {
        "bases": [ast.unparse(base) for base in node.bases],
        "methods": dict(sorted(methods.items())),
        "attributes": sorted(name for name in attributes if _public(name)),
    }


def _with_added_attribute(entry: dict[str, object], name: str) -> dict[str, object]:
    existing = [
        item
        for item in require_sequence(entry.get("attributes"), "class attributes")
        if isinstance(item, str)
    ]
    return {**entry, "attributes": sorted({*existing, name})}


def _module(
    root: Path, path: str, only: tuple[str, ...] | None
) -> dict[str, object] | None:
    file_path = root / SERIAL_CONTROLLER_DIRECTORY / path
    if not file_path.is_file():
        return None
    source = decode_python_source(file_path.read_bytes(), str(file_path))
    try:
        tree = ast.parse(source, filename=str(file_path))
    except SyntaxError as error:
        message = f"{path}: Python parse failed: {error}"
        raise ValueError(message) from error
    functions: dict[str, list[str]] = {}
    classes: dict[str, dict[str, object]] = {}
    variables: set[str] = set()
    added: list[tuple[str, str]] = []
    for item in tree.body:
        if isinstance(item, _FUNCTION_NODES):
            functions[item.name] = _parameters(item)
        elif isinstance(item, ast.ClassDef):
            classes[item.name] = _class_entry(item)
        else:
            variables.update(_assigned_names(item))
            # Attributes added outside the class body (``Direction.UP = ...``).
            added.extend(_assigned_attributes(item))
    for owner, name in added:
        if owner in classes and _public(name):
            classes[owner] = _with_added_attribute(classes[owner], name)

    def wanted(name: str) -> bool:
        return _public(name) if only is None else name in only

    return {
        "functions": {
            name: value for name, value in sorted(functions.items()) if wanted(name)
        },
        "classes": {
            name: value for name, value in sorted(classes.items()) if wanted(name)
        },
        "variables": sorted(name for name in variables if wanted(name)),
    }


def collect_public_api(root: Path) -> dict[str, object]:
    """Extract the compared public API surface from a materialized repository root."""
    files: dict[str, object] = {}
    for path, only in PUBLIC_API_FILES:
        entry = _module(root, path, only)
        if entry is not None:
            files[path] = entry
    return {"files": files}


def public_api_file_count(api: dict[str, object]) -> int:
    """Number of comparable files found in an extracted surface (0 means none)."""
    return len(require_mapping(api.get("files"), "public API files"))


def _require_string_list(value: object, path: str) -> list[str]:
    items = require_sequence(value, f"public API surface at {path or '<root>'}")
    if not all(isinstance(item, str) for item in items):
        invalid_value(f"public API surface is malformed at {path or '<root>'}")
    return [item for item in items if isinstance(item, str)]


def _compare_parameters(
    recorded: list[str],
    current: list[str],
    path: str,
    broken: list[str],
    unrecorded: list[str],
) -> None:
    if recorded == current:
        return
    if current[: len(recorded)] != recorded:
        broken.append(
            f"changed arguments: {path}: ({', '.join(recorded)}) -> ({', '.join(current)})"
        )
        return
    appended = current[len(recorded) :]
    required = [
        token for token in appended if "=" not in token and not token.startswith("*")
    ]
    if required:
        broken.append(f"added required arguments: {path}: {', '.join(required)}")
    else:
        unrecorded.append(f"{path}: arguments {', '.join(appended)}")


def _is_mapping(value: object) -> bool:
    """Mapping check kept as a helper so callers do not narrow to a partial dict type."""
    return isinstance(value, dict)


def compare_public_api(
    recorded: object,
    current: object,
    path: str = "",
    kind: str = "",
) -> tuple[list[str], list[str]]:
    """Compare recorded and current public API surfaces.

    Returns ``(breaking, unrecorded)``: breaking changes end promotion; unrecorded
    additions are compatible additions that are not part of the recorded surface yet.
    """
    broken: list[str] = []
    unrecorded: list[str] = []
    if _is_mapping(recorded) and _is_mapping(current):
        recorded_mapping = require_mapping(recorded, "recorded public API surface")
        current_mapping = require_mapping(current, "current public API surface")
        for key, value in recorded_mapping.items():
            where = f"{path} > {key}" if path else key
            if key not in current_mapping:
                broken.append(f"removed: {where}")
                continue
            nested_broken, nested_unrecorded = compare_public_api(
                value, current_mapping[key], where, key if key in _KINDS else kind
            )
            broken += nested_broken
            unrecorded += nested_unrecorded
        unrecorded += [
            f"{path} > {key}" if path else key
            for key in current_mapping
            if key not in recorded_mapping
        ]
    elif kind in _PARAMETER_LISTS:
        _compare_parameters(
            _require_string_list(recorded, path),
            _require_string_list(current, path),
            path,
            broken,
            unrecorded,
        )
    elif kind in _NAME_LISTS:
        recorded_names = _require_string_list(recorded, path)
        current_names = _require_string_list(current, path)
        broken += [
            f"removed: {path} > {name}"
            for name in recorded_names
            if name not in current_names
        ]
        unrecorded += [
            f"{path} > {name}" for name in current_names if name not in recorded_names
        ]
    return broken, unrecorded
