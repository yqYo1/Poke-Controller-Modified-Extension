"""XDG-compliant script directory discovery and configuration.

Provides utilities for discovering where user scripts should be loaded from,
following the XDG Base Directory Specification with sensible fallbacks.

Priority (highest to lowest):
1. Command-line argument: ``--scripts-dir``
2. Environment variable: ``POKECON_SCRIPTS_DIR``
3. XDG config home: ``$XDG_CONFIG_HOME/pokecon/scripts/``
4. Default fallback: ``~/.config/pokecon/scripts/``
5. Legacy fallback: ``./SerialController/Commands/PythonCommands/``

Usage
-----
>>> from pokecon.scripts_dir import get_scripts_dir, get_scripts_dir_with_fallbacks
>>> get_scripts_dir()
PosixPath('/home/user/.config/pokecon/scripts')
>>> get_scripts_dir_with_fallbacks()
[PosixPath('/home/user/.config/pokecon/scripts'), PosixPath('/path/to/project/SerialController/Commands/PythonCommands')]
"""

from __future__ import annotations

import os
from pathlib import Path
from typing import Final

# ── Constants ────────────────────────────────────────────────────────────────

DEFAULT_APP_NAME: Final[str] = "pokecon"
DEFAULT_SCRIPTS_SUBDIR: Final[str] = "scripts"
ENV_VAR_NAME: Final[str] = "POKECON_SCRIPTS_DIR"
LEGACY_RELATIVE_PATH: Final[str] = "SerialController/Commands/PythonCommands"


# ── Public API ───────────────────────────────────────────────────────────────


def get_scripts_dir() -> Path:
    """Return the primary script directory path.

    Resolution order:
    1. ``POKECON_SCRIPTS_DIR`` environment variable
    2. ``$XDG_CONFIG_HOME/pokecon/scripts/``
    3. ``~/.config/pokecon/scripts/``

    The directory is created automatically if it does not exist.
    """
    # 1. Environment variable
    if env_path := os.environ.get(ENV_VAR_NAME):
        path = Path(env_path).expanduser().resolve()
        path.mkdir(parents=True, exist_ok=True)
        return path

    # 2. XDG config home
    xdg_config = os.environ.get("XDG_CONFIG_HOME")
    if xdg_config:
        path = Path(xdg_config) / DEFAULT_APP_NAME / DEFAULT_SCRIPTS_SUBDIR
        path.mkdir(parents=True, exist_ok=True)
        return path

    # 3. Default fallback (~/.config/)
    path = Path.home() / ".config" / DEFAULT_APP_NAME / DEFAULT_SCRIPTS_SUBDIR
    path.mkdir(parents=True, exist_ok=True)
    return path


def get_legacy_scripts_dir(project_root: Path | None = None) -> Path | None:
    """Return the legacy script directory if it exists.

    Parameters
    ----------
    project_root
        The project root directory. If ``None``, uses the current working
        directory.

    Returns
    -------
    Path | None
        The legacy script directory path, or ``None`` if it does not exist.
    """
    if project_root is None:
        project_root = Path.cwd()
    legacy_path = project_root / LEGACY_RELATIVE_PATH
    return legacy_path if legacy_path.exists() else None


def get_scripts_dir_with_fallbacks(
    project_root: Path | None = None,
    include_legacy: bool = True,
) -> list[Path]:
    """Return all script directories to search, in priority order.

    Parameters
    ----------
    project_root
        The project root directory for legacy fallback.
    include_legacy
        Whether to include the legacy ``SerialController/Commands/PythonCommands/``
        directory as a fallback.

    Returns
    -------
    list[Path]
        List of script directory paths. The primary directory is always first.
        Legacy directories (if enabled and existing) are appended.
    """
    dirs: list[Path] = [get_scripts_dir()]

    if include_legacy:
        if legacy := get_legacy_scripts_dir(project_root):
            if legacy not in dirs:
                dirs.append(legacy)

    return dirs


def get_sample_scripts_dir() -> Path:
    """Return the directory for built-in sample scripts.

    These are shipped with the package and serve as templates or examples.
    """
    # When installed, samples are in the package directory
    import pokecon

    pkg_dir = Path(pokecon.__file__).parent
    samples = pkg_dir / "samples"
    if samples.exists():
        return samples

    # Development fallback: look in the project tree
    project_root = pkg_dir.parent.parent  # python/pokecon -> python -> project_root
    dev_samples = (
        project_root / "SerialController" / "Commands" / "PythonCommands" / "Samples"
    )
    if dev_samples.exists():
        return dev_samples

    # Create an empty samples dir as last resort
    samples.mkdir(parents=True, exist_ok=True)
    return samples


def ensure_scripts_structure(base_dir: Path | None = None) -> Path:
    """Ensure the script directory has a sensible initial structure.

    Creates subdirectories like ``Samples/``, ``Custom/``, etc.

    Returns
    -------
    Path
        The base script directory.
    """
    base = base_dir or get_scripts_dir()

    # Create standard subdirectories
    (base / "Samples").mkdir(exist_ok=True)
    (base / "Custom").mkdir(exist_ok=True)
    (base / "Templates").mkdir(exist_ok=True)

    # Create a README
    readme = base / "README.md"
    if not readme.exists():
        readme.write_text(
            "# Poke-Controller Scripts\\n\\n"
            "Place your Python scripts here.\\n\\n"
            "- ``Samples/`` — Example scripts (safe to modify or delete)\\n"
            "- ``Custom/`` — Your own scripts\\n"
            "- ``Templates/`` — Reusable script templates\\n\\n"
            "Scripts should inherit from ``PythonCommand`` or ``ImageProcPythonCommand``.\\n"
        )

    return base


# ── CLI helpers ──────────────────────────────────────────────────────────────


def add_scripts_dir_argument(parser) -> None:
    """Add ``--scripts-dir`` argument to an argparse parser."""
    parser.add_argument(
        "--scripts-dir",
        type=Path,
        default=None,
        help=(
            "Directory containing user scripts. "
            f"Overrides {ENV_VAR_NAME} env var and XDG defaults."
        ),
    )


def get_scripts_dir_from_args(args) -> Path:
    """Resolve script directory from parsed CLI args.

    Uses ``args.scripts_dir`` if provided, otherwise falls back to
    :func:`get_scripts_dir`.
    """
    if hasattr(args, "scripts_dir") and args.scripts_dir is not None:
        path = args.scripts_dir.expanduser().resolve()
        path.mkdir(parents=True, exist_ok=True)
        return path
    return get_scripts_dir()


__all__ = [
    "get_scripts_dir",
    "get_legacy_scripts_dir",
    "get_scripts_dir_with_fallbacks",
    "get_sample_scripts_dir",
    "ensure_scripts_structure",
    "add_scripts_dir_argument",
    "get_scripts_dir_from_args",
    "ENV_VAR_NAME",
    "LEGACY_RELATIVE_PATH",
]
