"""Rust-core adapter — bridge between Python wrappers and Rust-backed implementations.

This module provides ``_RustCoreAdapter``, a mixin class that **CommandMeta**
injects into the MRO of every user-script subclass of ``PythonCommand`` /
``ImageProcPythonCommand``.

The adapter sits between the user's class and the registered implementation
(e.g. ``_PythonCommandV1Interface`` or ``PythonCommand`` itself when registered
as ``v1``).  When Rust PyO3 bindings (``pokecon-pybindings``) become available,
the adapter can override methods to dispatch to Rust while falling back to the
Python implementation via ``super()``.

Architecture
~~~~~~~~~~~~
::

    UserClass.do()
        └── self.press()   →   _RustCoreAdapter.press()
                                    ├── [Rust available] → Rust serial
                                    └── [Rust missing]  → super().press()
                                                               └── ImplClass.press()
"""

from __future__ import annotations

import importlib.util as _importlib_util

# Check for the compiled Rust extension specifically.
# The Python package has ``commands`` (with 's'); the Rust extension has
# ``command`` (without 's') as a submodule.  We use this distinction to
# detect whether the Rust ``pokecon`` extension is actually installed.
_RUST_CORE_AVAILABLE: bool = _importlib_util.find_spec("pokecon.command") is not None


class _RustCoreAdapter:
    """Transparent mixin injected into the MRO by ``CommandMeta``.

    When a user script calls ``self.press(...)`` the MRO lookup reaches this
    class first (after the user's own class).  Currently the adapter passes
    all calls through to the registered implementation via ``super()``.
    Future phases will add Rust-dispatch logic.

    Usage
    -----
    This class is **never** referenced by user code directly.  It is injected
    automatically by ``CommandMeta.__new__``.
    """

    # NOTE: No __slots__ — this class participates in cooperative MRO and
    # may be combined with classes that have arbitrary instance dicts.

    def __init__(self, *args: object, **kwargs: object) -> None:
        self._use_rust: bool = _RUST_CORE_AVAILABLE
        super().__init__(*args, **kwargs)

    @property
    def available(self) -> bool:
        """``True`` when the Rust ``pokecon`` extension is importable."""
        return self._use_rust

    # ── Serial / KeyPress bridge stubs (for future Rust integration) ───────

    def press_button(self, button_name: str, duration_ms: int) -> None:
        """Send a button-press via Rust serial (when bindings exist)."""
        if self._use_rust:
            try:
                import pokecon.command as _cmd

                _cmd.press_button(button_name, duration_ms)
                return
            except (ImportError, AttributeError):
                msg = "Rust serial bindings not yet wired"
                raise NotImplementedError(msg) from None
        msg = "Rust serial bindings not available"
        raise NotImplementedError(msg)

    # ── Image processing bridge methods ──────────────────────────────────────

    def template_match(
        self,
        template_path: str,
        threshold: float = 0.7,
    ) -> tuple[bool, tuple[int, int], float]:
        """Run template matching via Rust CV (when bindings exist)."""
        if self._use_rust:
            try:
                import pokecon.image_proc as _ip

                return _ip.template_match(template_path, threshold)
            except (ImportError, AttributeError):
                msg = "Rust CV bindings not yet wired"
                raise NotImplementedError(msg) from None
        msg = "Rust CV bindings not available"
        raise NotImplementedError(msg)


__all__ = ["_RustCoreAdapter", "_RUST_CORE_AVAILABLE"]
