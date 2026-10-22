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
_VALUE_LISTS = ("values", "constants")
_KINDS = (*_PARAMETER_LISTS, *_NAME_LISTS, *_VALUE_LISTS)


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


def _literal_value(
    statement: ast.stmt, scope: dict[str, str]
) -> tuple[str, str] | None:
    """Single-name assignment with a statically known value (literal or alias).

    Covers ``A = <literal>``, ``A: T = <literal>`` and ``A = <earlier name>`` where the
    earlier name was captured in the same scope. Values computed by calls (``auto()``),
    multi-target assignments and module attributes are outside the literal-evaluable
    scope and are skipped; enum values built with ``auto()`` therefore remain covered
    only by the runtime corpus gate, not by this static comparison.
    """
    value: ast.expr
    if isinstance(statement, ast.Assign) and len(statement.targets) == 1:
        target = statement.targets[0]
        value = statement.value
    elif isinstance(statement, ast.AnnAssign) and statement.value is not None:
        target = statement.target
        value = statement.value
    else:
        return None
    if not isinstance(target, ast.Name):
        return None
    if isinstance(value, ast.Name):
        alias = scope.get(value.id)
        return (target.id, alias) if alias is not None else None
    try:
        return (target.id, repr(ast.literal_eval(value)))
    except ValueError, SyntaxError:
        return None


def _class_entry(node: ast.ClassDef) -> dict[str, object]:
    methods: dict[str, list[str]] = {}
    attributes: set[str] = set()
    values: dict[str, str] = {}
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
            literal = _literal_value(item, values)
            if literal is not None:
                values[literal[0]] = literal[1]
    return {
        "bases": [ast.unparse(base) for base in node.bases],
        "methods": dict(sorted(methods.items())),
        "attributes": sorted(name for name in attributes if _public(name)),
        "values": dict(sorted(values.items())),
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
    constants: dict[str, str] = {}
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
            literal = _literal_value(item, constants)
            if literal is not None:
                constants[literal[0]] = literal[1]
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
        "constants": {
            name: value for name, value in sorted(constants.items()) if wanted(name)
        },
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
    elif kind in _VALUE_LISTS:
        if recorded != current:
            broken.append(f"changed value: {path}: {recorded} -> {current}")
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


def _class_members(node: ast.ClassDef) -> set[str]:
    members: set[str] = set()
    for item in node.body:
        if isinstance(item, _FUNCTION_NODES):
            members.add(item.name)
            members.update(
                attribute for _, attribute in _assigned_attributes(item, "self")
            )
        else:
            members.update(_assigned_names(item))
    return members


def collect_script_api_surface(typings_directory: Path) -> dict[str, object]:
    """Collect the script-visible API surface from the generated typings (``.pyi``).

    Returns ``{"classes": {name: [members]}}`` with each class's own members merged
    with those inherited from typings-known bases.
    """
    own: dict[str, set[str]] = {}
    bases: dict[str, list[str]] = {}
    for file_path in sorted(typings_directory.glob("*.pyi")):
        source = decode_python_source(file_path.read_bytes(), str(file_path))
        try:
            tree = ast.parse(source, filename=str(file_path))
        except SyntaxError as error:
            message = f"{file_path}: typings parse failed: {error}"
            raise ValueError(message) from error
        for node in tree.body:
            if isinstance(node, ast.ClassDef):
                own[node.name] = _class_members(node)
                bases[node.name] = [ast.unparse(base) for base in node.bases]
    if not own:
        invalid_value(f"no script API classes found in {typings_directory}")

    resolved: dict[str, set[str]] = {}

    def resolve(name: str, seen: set[str]) -> set[str]:
        if name in resolved:
            return resolved[name]
        if name in seen or name not in own:
            return set()
        members = set(own[name])
        for base in bases.get(name, []):
            members |= resolve(base, {*seen, name})
        resolved[name] = members
        return members

    return {"classes": {name: sorted(resolve(name, set())) for name in own}}


def collect_script_local_members(source_path: Path) -> set[str]:
    """Members a script defines for itself (class methods, attributes, self assignments)."""
    source = decode_python_source(source_path.read_bytes(), str(source_path))
    try:
        tree = ast.parse(source, filename=str(source_path))
    except SyntaxError as error:
        message = f"{source_path}: script parse failed: {error}"
        raise ValueError(message) from error
    members: set[str] = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.ClassDef):
            members.update(_class_members(node))
    return members


def unresolved_script_references(
    references: list[str],
    bases: list[str],
    local_members: set[str],
    surface: dict[str, object],
) -> list[str]:
    """References whose first ``self.`` segment resolves to neither the script nor the layer.

    The comparison is static and first-segment only: ``self.keys.ser.writeRow`` is
    checked as ``keys``. Non-``self.`` references are not judged here.
    """
    classes = require_mapping(surface.get("classes"), "script API surface classes")
    layer_members: set[str] = set()
    known_base = False
    for base in bases:
        if base not in classes:
            continue
        known_base = True
        layer_members.update(
            item
            for item in require_sequence(classes.get(base), "class members")
            if isinstance(item, str)
        )
    if not known_base:
        # No typings-known base: fall back to the union of every known class.
        for name in classes:
            layer_members.update(
                item
                for item in require_sequence(classes.get(name), "class members")
                if isinstance(item, str)
            )
    missing: list[str] = []
    for reference in references:
        parts = reference.split(".")
        if not reference.startswith("self.") or len(parts) < 2 or not parts[1]:
            continue
        root = parts[1]
        if root not in local_members and root not in layer_members:
            missing.append(reference)
    return sorted(set(missing))
