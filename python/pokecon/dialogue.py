"""Message box, confirmation, and input dialogs (pure-Python fallback).

When the Rust extension is **not** compiled, this module provides all
dialog functionality using tkinter.  When the Rust extension IS compiled,
the compiled ``pokecon.dialogue`` binary module is loaded instead, and
this file is not used as the primary ``pokecon.dialogue`` module.
"""

from __future__ import annotations

import json
import logging
import os
import tkinter as tk
import tkinter.messagebox as _messagebox
import tkinter.simpledialog as _simpledialog
from tkinter import ttk
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


# ---------------------------------------------------------------------------
# dialogue6widget() — multi-widget dialog
# ---------------------------------------------------------------------------

_SUPPORTED_WIDGET_TYPES: frozenset[str] = frozenset(
    {"Entry", "Check", "Combo", "Radio", "Spin", "Scale"}
)


def _check_widget_names(dialogue_list: list[Any]) -> None:
    """Check for duplicate widget names in the dialogue list.

    Raises ``ValueError`` if duplicate names are found.
    """
    seen: set[str] = set()
    for item in dialogue_list:
        if not isinstance(item, list) or len(item) < 2:
            continue
        widget_type = str(item[0])
        if widget_type == "Next":
            continue
        if widget_type not in _SUPPORTED_WIDGET_TYPES:
            continue
        # Name is the second element for all supported widgets
        name = str(item[1])
        if name in seen:
            raise ValueError(f"Duplicate widget name: '{name}'")
        seen.add(name)


def _load_settings(filename: str) -> dict[str, Any]:
    """Load saved settings from a JSON file.

    Returns an empty dict if the file doesn't exist or can't be parsed.
    """
    if not os.path.isfile(filename):
        return {}
    try:
        with open(filename, encoding="utf-8") as f:
            data = json.load(f)
        if isinstance(data, dict):
            return data
        return {}
    except (json.JSONDecodeError, OSError) as exc:
        _logger.warning("Failed to load settings from %s: %s", filename, exc)
        return {}


def _save_settings(filename: str, settings: dict[str, Any]) -> None:
    """Save settings to a JSON file, creating parent directories as needed."""
    parent = os.path.dirname(filename)
    if parent:
        os.makedirs(parent, exist_ok=True)
    try:
        with open(filename, "w", encoding="utf-8") as f:
            json.dump(settings, f, indent=2, ensure_ascii=False)
    except OSError as exc:
        _logger.warning("Failed to save settings to %s: %s", filename, exc)


def _apply_saved_settings(
    dialogue_list: list[Any], settings: dict[str, Any]
) -> list[Any]:
    """Apply saved settings values to a dialogue list.

    Returns a new dialogue list with initial values replaced by saved values
    where names match.
    """
    result: list[Any] = []
    for item in dialogue_list:
        if not isinstance(item, list) or len(item) < 2:
            result.append(item)
            continue
        widget_type = str(item[0])
        if widget_type == "Next":
            result.append(item)
            continue
        name = str(item[1])
        if name in settings:
            saved_val = settings[name]
            new_item = list(item)
            if widget_type == "Entry" and len(new_item) >= 3:
                new_item[2] = str(saved_val)
            elif widget_type == "Check" and len(new_item) >= 3:
                new_item[2] = bool(saved_val)
            elif widget_type in ("Combo", "Radio", "Spin") and len(new_item) >= 4:
                new_item[3] = str(saved_val)
            elif widget_type == "Scale" and len(new_item) >= 5:
                try:
                    new_item[4] = float(saved_val)
                except (ValueError, TypeError):
                    pass
            result.append(new_item)
        else:
            result.append(item)
    return result


def dialogue6widget(
    title: str,
    dialogue_list: list[Any],
    desc: str | None = None,
    need: type[list[Any]] | type[dict[str, Any]] = list,
) -> list[Any] | dict[str, Any] | None:
    """Show a multi-widget input dialog (original Poke-Controller API).

    Parameters
    ----------
    title : str
        Dialog window title.
    dialogue_list : list
        Widget definitions. Each element is one of:

        - ``["Entry", name, init]`` — text entry field
        - ``["Check", name, init_bool]`` — checkbox
        - ``["Combo", name, choices, init]`` — dropdown combo box
        - ``["Radio", name, choices, init]`` — radio buttons
        - ``["Spin", name, choices, init]`` — spinbox
        - ``["Scale", name, min, max, init, digit]`` — scale slider
        - ``["Next"]`` — advance to next column
    desc : str, optional
        Description text shown at the top of the dialog.
    need : type, optional
        Return type: ``list`` (default) or ``dict``.

    Returns
    -------
    list or dict or None
        Widget values in order (list) or by name (dict), or ``None`` if cancelled.
    """
    _ensure_root()

    # Validate widget names
    _check_widget_names(dialogue_list)

    # Build and show the dialog
    dialog = _SixWidgetDialog(
        title=title, dialogue_list=dialogue_list, desc=desc or title
    )
    _ROOT.wait_window(dialog.top)  # type: ignore[union-attr]
    result = dialog.result

    if result is None:
        return None
    if need is dict:
        return dict(zip(dialog.names, result, strict=False))
    return result


def dialogue6widget_save_settings(
    title: str,
    dialogue_list: list[Any],
    filename: str,
    desc: str | None = None,
    need: type[list[Any]] | type[dict[str, Any]] = list,
) -> list[Any] | dict[str, Any] | None:
    """Multi-widget dialog with settings persistence.

    Loads previously saved values from the given JSON *filename* before
    showing the dialog, and saves the result back to the same file after
    the user clicks OK.

    Parameters
    ----------
    title : str
        Dialog window title.
    dialogue_list : list
        Widget definitions (same as ``dialogue6widget``).
    filename : str
        Path to the JSON settings file. Parent directories are created
        automatically.
    desc : str, optional
        Description text.
    need : type, optional
        Return type: ``list`` (default) or ``dict``.

    Returns
    -------
    list or dict or None
        Widget values, or ``None`` if cancelled.
    """
    _ensure_root()

    # Load saved settings
    settings = _load_settings(filename)
    if settings:
        _logger.debug("Loaded saved settings from %s", filename)
        dialogue_list = _apply_saved_settings(dialogue_list, settings)

    # Validate widget names
    _check_widget_names(dialogue_list)

    # Build and show the dialog
    dialog = _SixWidgetDialog(
        title=title, dialogue_list=dialogue_list, desc=desc or title
    )
    _ROOT.wait_window(dialog.top)  # type: ignore[union-attr]
    result = dialog.result

    if result is None:
        return None

    # Save settings
    if dialog.names and result:
        save_data = dict(zip(dialog.names, result, strict=False))
        _save_settings(filename, save_data)

    if need is dict:
        return dict(zip(dialog.names, result, strict=False))
    return result


def dialogue6widget_select_settings(
    title: str,
    dialogue_list: list[Any],
    dirname: str,
    desc: str | None = None,
    need: type[list[Any]] | type[dict[str, Any]] = list,
) -> list[Any] | dict[str, Any] | None:
    """Multi-widget dialog with settings selection.

    Lists available JSON settings files in the given *dirname* directory,
    lets the user choose one, applies it, and shows the dialog.  The result
    is saved back to ``dirname/前回の設定.json`` (previous settings).

    Parameters
    ----------
    title : str
        Dialog window title.
    dialogue_list : list
        Widget definitions (same as ``dialogue6widget``).
    dirname : str
        Directory containing preset JSON settings files.
    desc : str, optional
        Description text.
    need : type, optional
        Return type: ``list`` (default) or ``dict``.

    Returns
    -------
    list or dict or None
        Widget values, or ``None`` if cancelled.
    """
    _ensure_root()

    # List available settings files
    available: list[str] = []
    if os.path.isdir(dirname):
        for fname in os.listdir(dirname):
            if fname.endswith(".json"):
                available.append(os.path.join(dirname, fname))

    # Show selection dialog if there are choices
    chosen_file: str | None = None
    if available:
        # Let user pick via a simple selection dialog
        choice = _simpledialog.askstring(
            title,
            f"Select settings file (in {dirname}):\n"
            + "\n".join(os.path.basename(f) for f in available),
            initialvalue=os.path.basename(available[0]),
        )
        if choice is not None:
            candidate = os.path.join(dirname, choice)
            if candidate in available:
                chosen_file = candidate

        # Load chosen settings
        if chosen_file:
            settings = _load_settings(chosen_file)
            if settings:
                dialogue_list = _apply_saved_settings(dialogue_list, settings)
    else:
        _logger.debug("No settings files found in %s", dirname)

    # Also try loading 前回の設定.json (previous settings) if no file was chosen
    previous_file = os.path.join(dirname, "前回の設定.json")
    if chosen_file is None and os.path.isfile(previous_file):
        settings = _load_settings(previous_file)
        if settings:
            dialogue_list = _apply_saved_settings(dialogue_list, settings)

    # Validate widget names
    _check_widget_names(dialogue_list)

    # Build and show the dialog
    dialog = _SixWidgetDialog(
        title=title, dialogue_list=dialogue_list, desc=desc or title
    )
    _ROOT.wait_window(dialog.top)  # type: ignore[union-attr]
    result = dialog.result

    if result is None:
        return None

    # Save to 前回の設定.json
    if dialog.names and result:
        save_data = dict(zip(dialog.names, result, strict=False))
        os.makedirs(dirname, exist_ok=True)
        _save_settings(previous_file, save_data)

    if need is dict:
        return dict(zip(dialog.names, result, strict=False))
    return result


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


# ---------------------------------------------------------------------------
# Internal _SixWidgetDialog — multi-widget dialog
# ---------------------------------------------------------------------------


class _SixWidgetDialog:
    """A dialog with up to six widget types and multi-column layout.

    Supported widget types (matching the original ``PokeConDialogue`` API):

    - ``Entry`` — text entry field
    - ``Check`` — checkbox
    - ``Combo`` — dropdown combo box
    - ``Radio`` — radio buttons
    - ``Spin`` — spinbox
    - ``Scale`` — scale slider
    - ``Next`` — column break
    """

    # Maximum widgets per column before a forced column break
    _MAX_PER_COLUMN: int = 15

    def __init__(
        self,
        title: str,
        dialogue_list: list[Any],
        desc: str = "",
    ) -> None:
        self.result: list[Any] | None = None
        self.names: list[str] = []
        # Internal collection — holds widgets OR tkinter variable objects
        self._widget_refs: list[Any] = []
        self._widget_types: list[str] = []

        self.top = tk.Toplevel(_ROOT)
        self.top.title(title)
        self.top.resizable(False, False)
        self.top.transient(_ROOT)
        self.top.grab_set()

        if desc:
            desc_label = tk.Label(self.top)
            desc_label.config(text=desc, wraplength=600, justify="left")
            desc_label.pack(padx=10, pady=(10, 5), fill="x")

        # Build the widget grid
        self._build_widgets(dialogue_list)

        # OK / Cancel buttons
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

        # Center on screen
        self.top.update_idletasks()
        width = self.top.winfo_width()
        height = self.top.winfo_height()
        screen_w = self.top.winfo_screenwidth()
        screen_h = self.top.winfo_screenheight()
        x = (screen_w - width) // 2
        y = (screen_h - height) // 2
        self.top.geometry(f"+{x}+{y}")

        # Focus first widget
        if self._widget_refs:
            try:
                self._widget_refs[0].focus_set()
            except (tk.TclError, AttributeError):
                pass

    # ── widget building ──────────────────────────────────────────────

    def _build_widgets(self, dialogue_list: list[Any]) -> None:
        """Build the widget grid from the dialogue list definition."""
        # Split into columns at ``["Next"]`` markers
        columns: list[list[Any]] = [[]]
        for item in dialogue_list:
            if isinstance(item, list) and len(item) >= 1 and item[0] == "Next":
                columns.append([])
            else:
                columns[-1].append(item)

        # Create a paned container for columns
        outer = tk.Frame(self.top)
        outer.pack(padx=10, pady=5, fill="both", expand=True)

        for col_idx, col_items in enumerate(columns):
            if not col_items:
                continue
            col_frame = tk.Frame(outer, relief="solid", borderwidth=1)
            col_frame.pack(side="left", fill="both", expand=True, padx=2, pady=2)

            for item in col_items:
                self._build_widget_row(col_frame, item)

    def _build_widget_row(self, parent: tk.Frame, item: Any) -> None:
        """Build a single widget row within a column."""
        if not isinstance(item, list) or len(item) < 2:
            _logger.warning("Invalid widget definition: %s", item)
            return

        widget_type = str(item[0])

        if widget_type not in _SUPPORTED_WIDGET_TYPES:
            _logger.warning("Unsupported widget type '%s', skipping", widget_type)
            return

        name = str(item[1])
        self.names.append(name)

        row = tk.Frame(parent)
        row.pack(fill="x", pady=2, padx=5)

        # Label for the widget
        lbl = tk.Label(row, text=name, width=16, anchor="w")
        lbl.pack(side="left", padx=(0, 5))

        # Build the appropriate widget
        if widget_type == "Entry":
            self._build_entry(row, item, name)
        elif widget_type == "Check":
            self._build_check(row, item, name)
        elif widget_type == "Combo":
            self._build_combo(row, item, name)
        elif widget_type == "Radio":
            self._build_radio(row, item, name)
        elif widget_type == "Spin":
            self._build_spin(row, item, name)
        elif widget_type == "Scale":
            self._build_scale(row, item, name)

    def _build_entry(self, parent: tk.Frame, item: list[Any], name: str) -> None:
        init_val = str(item[2]) if len(item) >= 3 else ""
        entry = tk.Entry(parent, width=30)
        entry.insert(0, init_val)
        entry.pack(side="left", fill="x", expand=True)
        self._widget_refs.append(entry)
        self._widget_types.append("Entry")

    def _build_check(self, parent: tk.Frame, item: list[Any], name: str) -> None:
        init_val = bool(item[2]) if len(item) >= 3 else False
        var = tk.BooleanVar(value=init_val)
        cb = tk.Checkbutton(parent, variable=var)
        cb.pack(side="left")
        self._widget_refs.append(var)
        self._widget_types.append("Check")

    def _build_combo(self, parent: tk.Frame, item: list[Any], name: str) -> None:
        choices = list(item[2]) if len(item) >= 3 else []
        init_val = (
            str(item[3])
            if len(item) >= 4 and item[3] in choices
            else (str(choices[0]) if choices else "")
        )
        combo = ttk.Combobox(parent, values=choices, width=27, state="readonly")
        combo.set(init_val)
        combo.pack(side="left", fill="x", expand=True)
        self._widget_refs.append(combo)
        self._widget_types.append("Combo")

    def _build_radio(self, parent: tk.Frame, item: list[Any], name: str) -> None:
        choices = list(item[2]) if len(item) >= 3 else []
        init_val = (
            str(item[3]) if len(item) >= 4 else (str(choices[0]) if choices else "")
        )
        var = tk.StringVar(value=init_val)
        radio_frame = tk.Frame(parent)
        radio_frame.pack(side="left", fill="x", expand=True)
        for choice in choices:
            rb = tk.Radiobutton(
                radio_frame, text=str(choice), variable=var, value=str(choice)
            )
            rb.pack(side="left", padx=2)
        self._widget_refs.append(var)
        self._widget_types.append("Radio")

    def _build_spin(self, parent: tk.Frame, item: list[Any], name: str) -> None:
        choices = item[2] if len(item) >= 3 else []
        init_val = (
            str(item[3]) if len(item) >= 4 else (str(choices[0]) if choices else "")
        )
        # If choices is a list of strings, use values; otherwise use a range
        if isinstance(choices, list) and choices:
            spin = tk.Spinbox(parent, values=tuple(str(c) for c in choices), width=27)
        else:
            spin = tk.Spinbox(parent, from_=0, to=100, width=27)
        spin.delete(0, "end")
        spin.insert(0, init_val)
        spin.pack(side="left", fill="x", expand=True)
        self._widget_refs.append(spin)
        self._widget_types.append("Spin")

    def _build_scale(self, parent: tk.Frame, item: list[Any], name: str) -> None:
        min_val = int(item[2]) if len(item) >= 3 else 0
        max_val = int(item[3]) if len(item) >= 4 else 100
        init_val = int(item[4]) if len(item) >= 5 else min_val
        digit = int(item[5]) if len(item) >= 6 else 0
        var = tk.IntVar(value=init_val)
        scale = tk.Scale(
            parent,
            from_=min_val,
            to=max_val,
            orient="horizontal",
            variable=var,
            resolution=digit if digit > 0 else 1,
            length=200,
        )
        scale.pack(side="left", fill="x", expand=True)
        self._widget_refs.append(var)
        self._widget_types.append("Scale")

    # ── result collection ───────────────────────────────────────────

    def _get_values(self) -> list[Any]:
        """Collect current widget values into a list."""
        values: list[Any] = []
        for ref, wtype in zip(self._widget_refs, self._widget_types, strict=True):
            if wtype == "Entry":
                values.append(ref.get())
            elif wtype == "Check":
                values.append(ref.get())
            elif wtype == "Combo":
                values.append(ref.get())
            elif wtype == "Radio":
                values.append(ref.get())
            elif wtype == "Spin":
                values.append(ref.get())
            elif wtype == "Scale":
                values.append(ref.get())
        return values

    def _on_ok(self) -> None:
        self.result = self._get_values()
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
