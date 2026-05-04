"""Rust-core adapter — bridge between Python wrappers and Rust-backed implementations.

This module is the central dispatch point for routing calls from backward-compatible
Python wrappers to the Rust ``pokecon-*`` crates (via PyO3 bindings).

Architecture
────────────
                       ┌──────────────────┐
                       │  User script      │  (e.g. AutoLeague.py)
                       │  from Commands…   │
                       └──────┬───────────┘
                              │ imports
                              ▼
                ┌─────────────────────────────┐
                │ python/pokecon/ (wrappers)   │
                │  keys.py  commands.py        │  ← Backward-compatible API
                └──────┬──────────────────────┘
                       │ delegates to
                       ▼
              ┌────────────────────┐
              │ _RustCoreAdapter   │  ← This module
              │ (dispatch logic)   │
              └──────┬─────────────┘
                     │ calls PyO3 bindings
                     ▼
          ┌──────────────────────────┐
          │ rust/pokecon-pybindings  │  ← PyO3 extension module
          │  ├─ keys.rs             │
          │  ├─ python_cmd.rs       │
          │  ├─ image_proc.rs       │
          │  └─ events.rs           │
          └──────┬──────────────────┘
                 │ FFI → Rust
                 ▼
          ┌──────────────────────────┐
          │ Rust workspace crates    │
          │  ├─ pokecon-serial       │
          │  ├─ pokecon-cv           │
          │  ├─ pokecon-core         │
          │  ├─ pokecon-notify       │
          │  ├─ pokecon-net          │
          │  └─ pokecon-events       │
          └──────────────────────────┘

Migration Plan
──────────────
Phase 1  (NOW)     – Pure-Python wrappers in python/pokecon/ mirror the old API.
                      Tests and user scripts import from old paths; wrappers
                      delegate to existing Python modules (ImageProcessing, etc.).

Phase 2  (NEXT)    – Extend Rust PyO3 bindings to expose:
                       - ``pokecon.keys.KeyPress`` (async → sync wrapper)
                       - ``pokecon.keys.Button / Hat / Stick / Direction / Touchscreen``
                       - ``pokecon.image_proc.TemplateMatcher``
                       - ``pokecon.serial.Sender``
                      Wrappers in python/pokecon/ get a ``_use_rust`` flag.

Phase 3  (FUTURE)  – Complete migration: all heavy lifting via Rust.
                      Python wrappers become thin delegates.
                      ``ImageProcessing`` (OpenCV) replaced by Rust ``pokecon-cv``.
"""

from __future__ import annotations

import sys
from typing import TYPE_CHECKING

try:
    import pokecon  # type: ignore[import-untyped] # noqa: F811

    _RUST_CORE_AVAILABLE = True
except ImportError:
    _RUST_CORE_AVAILABLE = False


class _RustCoreAdapter:
    """Conditional dispatch to Rust-backed implementations.

    Usage
    -----
    Subclasses of PythonCommand/ImageProcPythonCommand can call
    ``self._adapter.press(…)`` instead of the pure-Python path when
    the Rust extension is available.
    """

    __slots__ = ("_use_rust",)

    def __init__(self, use_rust: bool = False) -> None:
        self._use_rust = use_rust and _RUST_CORE_AVAILABLE

    @property
    def available(self) -> bool:
        return self._use_rust

    # ── Serial / KeyPress bridge ────────────────────────────────────────

    def press_button(self, button_name: str, duration_ms: int) -> None:
        """Send a button-press via Rust serial (when bindings exist)."""
        if self._use_rust:
            # TODO: call pokecon.serial.press(button_name, duration_ms)
            pass
        raise NotImplementedError("Rust serial bindings not yet wired")

    # ── Image processing bridge ─────────────────────────────────────────

    def template_match(
        self,
        template_path: str,
        threshold: float = 0.7,
    ) -> tuple[bool, tuple[int, int], float]:
        """Run template matching via Rust CV (when bindings exist)."""
        if self._use_rust:
            # TODO: call pokecon.image_proc.match_template(template_path, threshold)
            pass
        raise NotImplementedError("Rust CV bindings not yet wired")


__all__ = ["_RustCoreAdapter", "_RUST_CORE_AVAILABLE"]
