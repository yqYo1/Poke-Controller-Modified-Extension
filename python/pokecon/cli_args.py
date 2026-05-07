"""CLI argument parser for Poke-Controller.

Supports:
- ``--scripts-dir`` — override script directory
- ``--profile`` — select configuration profile
- ``--ui`` — choose UI mode (web, tauri, legacy)
- ``--verbose`` / ``--quiet`` — logging control
"""

from __future__ import annotations

import argparse

from pokecon.scripts_dir import add_scripts_dir_argument, get_scripts_dir_from_args


def create_parser() -> argparse.ArgumentParser:
    """Create the main argument parser."""
    parser = argparse.ArgumentParser(
        prog="pokecon",
        description="Poke-Controller Modified Extension — Game console automation",
    )

    # Script directory
    add_scripts_dir_argument(parser)

    # Profile selection
    parser.add_argument(
        "--profile",
        type=str,
        default="default",
        help="Configuration profile to use (default: 'default')",
    )

    # UI mode - auto detects GUI environment
    parser.add_argument(
        "--ui",
        choices=["web", "tauri", "legacy", "headless", "auto"],
        default="auto",
        help="UI mode to use (default: 'auto' — detects GUI environment)",
    );

    # Logging
    parser.add_argument(
        "--verbose",
        "-v",
        action="count",
        default=0,
        help="Increase verbosity (use multiple times for more detail)",
    )
    parser.add_argument(
        "--quiet",
        "-q",
        action="store_true",
        help="Suppress non-error output",
    )

    # Device settings
    parser.add_argument(
        "--serial-port",
        type=str,
        default=None,
        help="Serial port path (e.g., /dev/ttyUSB0, COM3)",
    )
    parser.add_argument(
        "--camera-index",
        type=int,
        default=0,
        help="Camera device index (default: 0)",
    )

    # Web UI settings
    parser.add_argument(
        "--web-host",
        type=str,
        default="127.0.0.1",
        help="Web UI bind address (default: 127.0.0.1)",
    )
    parser.add_argument(
        "--web-port",
        type=int,
        default=8080,
        help="Web UI port (default: 8080)",
    )

    return parser


def parse_args(args: list[str] | None = None) -> argparse.Namespace:
    """Parse command-line arguments."""
    parser = create_parser()
    return parser.parse_args(args)


def get_config_from_args(parsed: argparse.Namespace) -> dict:
    """Convert parsed args to a configuration dictionary."""
    scripts_dir = get_scripts_dir_from_args(parsed)

    return {
        "scripts_dir": scripts_dir,
        "profile": parsed.profile,
        "ui_mode": parsed.ui,
        "verbose": parsed.verbose,
        "quiet": parsed.quiet,
        "serial_port": parsed.serial_port,
        "camera_index": parsed.camera_index,
        "web_host": parsed.web_host,
        "web_port": parsed.web_port,
    }


__all__ = [
    "create_parser",
    "parse_args",
    "get_config_from_args",
]
