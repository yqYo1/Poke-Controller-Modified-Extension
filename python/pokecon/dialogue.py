"""Message box, confirmation, and input dialogs (pure-Python fallback).

When the Rust extension is **not** compiled, this module provides all
dialog functionality using tkinter.  When the Rust extension IS compiled,
the compiled ``pokecon.dialogue`` binary module is loaded instead, and
this file is not used as the primary ``pokecon.dialogue`` module.
"""

from __future__ import annotations

import logging
import tkinter as tk
import tkinter.messagebox as _messagebox
import tkinter.simpledialog as _simpledialog
from typing import Any, Literal

_logger: logging.Logger = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------

_ROOT: tk.Tk | None = None


def _ensure_root() -> None:
    """Create a hidden tkinter root window if none exists."""
    global _ROOT
    try:
        root_exists = _ROOT is not None and bool(_ROOT.winfo_exists())
    except (tk.TclError, AttributeError):
        root_exists = False

    if _ROOT is None or not root_exists:
        _ROOT = tk.Tk()
        _ROOT.withdraw()


def _destroy_root() -> None:
    """Destroy the hidden root window if it exists."""
    global _ROOT
    if _ROOT is not None:
        try:
            _ROOT.destroy()
        except tk.TclError:
            pass
        _ROOT = None


_MESSAGE_BOX_FUNCS: dict[str, Any] = {
    "info": _messagebox.showinfo,
    "warning": _messagebox.showwarning,
    "error": _messagebox.showerror,
}


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------


def show_message(
    title: str,
    message: str,
    kind: Literal["info", "warning", "error"] = "info",
) -> None:
    """Show a message box dialog.

    Parameters
    ----------
    title : str
        Dialog window title.
    message : str
        Message body text.
    kind : Literal["info", "warning", "error"]
        The icon style (default ``"info"``).
    """
    _ensure_root()
    funcs: dict[str, Any] = {
        "info": _messagebox.showinfo,
        "warning": _messagebox.showwarning,
        "error": _messagebox.showerror,
    }
    func = funcs.get(kind, _messagebox.showinfo)
    func(title, message)


def confirm(title: str, message: str) -> bool:
    """Show a Yes/No confirmation dialog.

    Returns ``True`` if the user clicked **Yes**.
    """
    _ensure_root()
    return bool(_messagebox.askyesno(title, message))


def input_dialog(
    title: str,
    prompt: str,
    default: str = "",
) -> str | None:
    """Show a text input dialog.

    Returns the entered text, or ``None`` if cancelled.
    """
    _ensure_root()
    return _simpledialog.askstring(title, prompt, initialvalue=default)


def ok_cancel(title: str, message: str) -> bool:
    """Show an OK/Cancel dialog.

    Returns ``True`` if the user clicked **OK**.
    """
    _ensure_root()
    return bool(_messagebox.askokcancel(title, message))


def retry_cancel(title: str, message: str) -> bool:
    """Show a Retry/Cancel dialog.

    Returns ``True`` if the user clicked **Retry**.
    """
    _ensure_root()
    return bool(_messagebox.askretrycancel(title, message))


# ---------------------------------------------------------------------------
# dialogue() — original Poke-Controller API for entry-based input
# ---------------------------------------------------------------------------


def dialogue(
    title: str,
    message: str | int | list[str | int],
    desc: str | None = None,
    need: type[list[Any]] | type[dict[str, Any]] = list,
) -> list[str] | dict[str, str] | None:
    """Show an entry-based input dialog (original Poke-Controller API).

    Parameters
    ----------
    title : str
        Dialog window title.
    message : int | str | list[int | str]
        Entry field definitions:

        - **int**: creates *N* entry fields labelled ``"1"``, ``"2"``, …
        - **str**: a single entry field with this label.
        - **list**: each element is one label.
    desc : str, optional
        Description text (defaults to *title*).
    need : type, optional
        Return type: ``list`` (default) or ``dict``.

    Returns
    -------
    list[str] or dict[str, str] or None
        Values entered by the user, or ``None`` if cancelled.
    """
    _ensure_root()

    # Normalise *message* into a list of labels
    if isinstance(message, int):
        labels = [str(i + 1) for i in range(message)]
    elif isinstance(message, str):
        labels = [message]
    else:
        labels = [str(item) for item in message]

    # Build the custom dialog
    dialog = _EntryDialog(title=title, labels=labels, desc=desc or title)
    _ROOT.wait_window(dialog.top)  # type: ignore[union-attr]
    result = dialog.result

    if result is None:
        return None
    if need is dict:
        return dict(zip(labels, result, strict=False))
    return result


def dialogue6widget(
    title: str,
    dialogue_list: list[Any],
    desc: str | None = None,
    need: type[list[Any]] | type[dict[str, Any]] = list,
) -> list[Any] | dict[str, Any] | None:
    """Multi-widget input dialog (original API).

    Delegates to ``PokeConDialogue.generate_new_dialogue_list`` when
    the original module is available.
    """
    _logger.debug("dialogue6widget(%s): delegating to PokeConDialogue", title)
    try:
        from PokeConDialogue import (  # type: ignore[import-untyped]
            generate_new_dialogue_list,
        )

        return generate_new_dialogue_list(title, dialogue_list, desc, need, _ROOT)
    except ImportError:
        _logger.warning("PokeConDialogue not available, dialogue6widget is a no-op")
        return None


def dialogue6widget_save_settings(
    title: str,
    dialogue_list: list[Any],
    filename: str,
    desc: str | None = None,
    need: type[list[Any]] | type[dict[str, Any]] = list,
) -> list[Any] | dict[str, Any] | None:
    """Multi-widget dialog with settings persistence (original API)."""
    _logger.debug("dialogue6widget_save_settings(%s): delegating", title)
    try:
        from PokeConDialogue import (  # type: ignore[import-untyped]
            save_dialogue_settings,
        )

        return save_dialogue_settings(title, dialogue_list, filename, desc, need, _ROOT)
    except ImportError:
        _logger.warning(
            "PokeConDialogue not available, dialogue6widget_save_settings is a no-op"
        )
        return None


def dialogue6widget_select_settings(
    title: str,
    dialogue_list: list[Any],
    dirname: str,
    desc: str | None = None,
    need: type[list[Any]] | type[dict[str, Any]] = list,
) -> list[Any] | dict[str, Any] | None:
    """Multi-widget dialog with settings selection (original API)."""
    _logger.debug("dialogue6widget_select_settings(%s): delegating", title)
    try:
        from PokeConDialogue import (  # type: ignore[import-untyped]
            get_settings_list,
        )

        return get_settings_list(title, dialogue_list, dirname, desc, need, _ROOT)
    except ImportError:
        _logger.warning(
            "PokeConDialogue not available, dialogue6widget_select_settings is a no-op"
        )
        return None


# ---------------------------------------------------------------------------
# Internal _EntryDialog
# ---------------------------------------------------------------------------


class _EntryDialog:
    """A simple dialog with multiple text-entry fields."""

    def __init__(
        self,
        title: str,
        labels: list[str],
        desc: str = "",
    ) -> None:
        self.result: list[str] | None = None
        self.top = tk.Toplevel(_ROOT)
        self.top.title(title)
        self.top.resizable(False, False)
        self.top.transient(_ROOT)
        self.top.grab_set()

        if desc:
            desc_label = tk.Label(self.top)
            desc_label.config(text=desc, wraplength=400, justify="left")
            desc_label.pack(padx=10, pady=(10, 5), fill="x")

        self.entries: list[tk.Entry] = []
        frame = tk.Frame(self.top)
        frame.pack(padx=10, pady=5, fill="x")

        for label_text in labels:
            row_frame = tk.Frame(frame)
            row_frame.pack(fill="x", pady=2)
            lbl = tk.Label(row_frame)
            lbl.config(text=label_text, width=20, anchor="w")
            lbl.pack(side="left", padx=(0, 5))
            entry = tk.Entry(row_frame)
            entry.config(width=40)
            entry.pack(side="left", fill="x", expand=True)
            self.entries.append(entry)

        btn_frame = tk.Frame(self.top)
        btn_frame.pack(padx=10, pady=(5, 10))

        ok_btn = tk.Button(btn_frame, text="OK", width=10, command=self._on_ok)
        ok_btn.pack(side="left", padx=5)

        cancel_btn = tk.Button(
            btn_frame, text="Cancel", width=10, command=self._on_cancel
        )
        cancel_btn.pack(side="left", padx=5)

        self.top.bind("<Return>", lambda _: self._on_ok())
        self.top.bind("<Escape>", lambda _: self._on_cancel())

        self.top.update_idletasks()
        width = self.top.winfo_width()
        height = self.top.winfo_height()
        screen_w = self.top.winfo_screenwidth()
        screen_h = self.top.winfo_screenheight()
        x = (screen_w - width) // 2
        y = (screen_h - height) // 2
        self.top.geometry(f"+{x}+{y}")

        if self.entries:
            self.entries[0].focus_set()

    def _on_ok(self) -> None:
        self.result = [e.get() for e in self.entries]
        self.top.destroy()

    def _on_cancel(self) -> None:
        self.result = None
        self.top.destroy()


__all__ = [
    "show_message",
    "confirm",
    "input_dialog",
    "ok_cancel",
    "retry_cancel",
    "dialogue",
    "dialogue6widget",
    "dialogue6widget_save_settings",
    "dialogue6widget_select_settings",
]
