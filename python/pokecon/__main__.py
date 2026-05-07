"""Poke-Controller entry point — ``python -m pokecon``.

Parses CLI arguments and launches the appropriate UI mode.
"""

from __future__ import annotations

import sys

from pokecon.cli_args import parse_args
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
        print("Poke-Controller Modified Extension")
        print(f"Scripts directory: {scripts_dir}")
        print(f"UI mode: {args.ui}")
        print(f"Profile: {args.profile}")

    # Launch UI based on mode
    ui_mode = args.ui
    if ui_mode == "auto":
        # Auto-detect GUI environment
        import os
        if os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY"):
            ui_mode = "tauri"
        else:
            ui_mode = "web"
        if args.verbose > 0:
            print(f"Auto-detected UI mode: {ui_mode}")

    if ui_mode == "legacy":
        print("Launching legacy tkinter UI...")
        # TODO: Import and launch legacy UI
        # from SerialController.Window import Window
        # Window().run()
        print("Legacy UI not yet integrated. Use --ui web for Web UI.")
        return 1

    elif ui_mode == "web":
        print(f"Launching Web UI on {args.web_host}:{args.web_port}...")
        # The web UI is served by the Tauri binary's Axum server
        # Launch via subprocess: pokecon-tauri --ui web
        import subprocess
        import shutil
        import os

        # Find the Tauri binary
        tauri_binary = shutil.which("pokecon-tauri")
        if tauri_binary is None:
            # Try to find it in common locations
            for path in [
                "src-tauri/target/release/pokecon-tauri",
                "src-tauri/target/debug/pokecon-tauri",
            ]:
                if os.path.isfile(path) and os.access(path, os.X_OK):
                    tauri_binary = os.path.abspath(path)
                    break

        if tauri_binary is None:
            print("Error: pokecon-tauri binary not found.")
            print("Please build it first with: cargo build --release --manifest-path src-tauri/Cargo.toml")
            return 1

        cmd = [
            tauri_binary,
            "--ui", "web",
            "--port", str(args.web_port),
        ]
        if args.verbose > 0:
            print(f"Running: {' '.join(cmd)}")
        return subprocess.call(cmd)

    elif ui_mode == "tauri":
        print("Launching Tauri UI...")
        # Launch the Tauri binary
        import subprocess
        import shutil
        import os

        tauri_binary = shutil.which("pokecon-tauri")
        if tauri_binary is None:
            for path in [
                "src-tauri/target/release/pokecon-tauri",
                "src-tauri/target/debug/pokecon-tauri",
            ]:
                if os.path.isfile(path) and os.access(path, os.X_OK):
                    tauri_binary = os.path.abspath(path)
                    break

        if tauri_binary is None:
            print("Error: pokecon-tauri binary not found.")
            print("Please build it first with: cargo build --release --manifest-path src-tauri/Cargo.toml")
            return 1

        cmd = [
            tauri_binary,
            "--ui", "tauri",
            "--port", str(args.web_port),
        ]
        if args.verbose > 0:
            print(f"Running: {' '.join(cmd)}")
        return subprocess.call(cmd)

    elif ui_mode == "headless":
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
