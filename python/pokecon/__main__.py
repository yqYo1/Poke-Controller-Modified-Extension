"""Poke-Controller entry point — ``python -m pokecon``.

Parses CLI arguments and launches the appropriate UI mode.
"""

from __future__ import annotations

import sys

from pokecon.cli_args import parse_args, get_config_from_args
from pokecon.scripts_dir import ensure_scripts_structure


def main() -> int:
    """Main entry point for Poke-Controller.

    Returns
    -------
    int
        Exit code (0 for success, non-zero for errors).
    """
    args = parse_args()

    # Ensure script directory exists
    scripts_dir = ensure_scripts_structure()

    if args.verbose > 0:
        print(f"Poke-Controller Modified Extension")
        print(f"Scripts directory: {scripts_dir}")
        print(f"UI mode: {args.ui}")
        print(f"Profile: {args.profile}")

    # Launch UI based on mode
    if args.ui == "legacy":
        print("Launching legacy tkinter UI...")
        # TODO: Import and launch legacy UI
        # from SerialController.Window import Window
        # Window().run()
        print("Legacy UI not yet integrated. Use --ui web for Web UI.")
        return 1

    elif args.ui == "web":
        print(f"Launching Web UI on {args.web_host}:{args.web_port}...")
        # TODO: Import and launch web UI
        # from pokecon_web import start_server
        # start_server(host=args.web_host, port=args.web_port)
        print("Web UI not yet integrated.")
        return 1

    elif args.ui == "tauri":
        print("Launching Tauri UI...")
        # TODO: Import and launch Tauri UI
        print("Tauri UI not yet integrated.")
        return 1

    elif args.ui == "headless":
        print("Running in headless mode...")
        # TODO: Load scripts and run without UI
        from pokecon.script_loader import ScriptLoader
        loader = ScriptLoader()
        commands = loader.load_all()
        print(f"Loaded {len(commands)} commands")
        return 0

    return 0


if __name__ == "__main__":
    sys.exit(main())
