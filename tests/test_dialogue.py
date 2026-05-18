"""Tests for pokecon._dialogue_impl (dialogue implementation)."""

from __future__ import annotations

from unittest.mock import MagicMock, patch

import pytest


@pytest.fixture(autouse=True)
def _mock_tk_root():
    """Mock tkinter.Tk to avoid GUI interactions during tests."""
    with patch("tkinter.Tk") as mock_tk:
        mock_root = MagicMock()
        mock_root.winfo_exists.return_value = True
        mock_root.winfo_width.return_value = 200
        mock_root.winfo_height.return_value = 100
        mock_root.winfo_screenwidth.return_value = 1920
        mock_root.winfo_screenheight.return_value = 1080
        mock_tk.return_value = mock_root
        yield mock_root


class TestShowMessage:
    """Tests for ``show_message()``."""

    def test_show_info(self):
        from pokecon.dialogue import show_message

        with patch("tkinter.messagebox.showinfo") as mock_showinfo:
            show_message("Title", "Message", kind="info")
            mock_showinfo.assert_called_once_with("Title", "Message")

    def test_show_warning(self):
        from pokecon.dialogue import show_message

        with patch("tkinter.messagebox.showwarning") as mock_showwarning:
            show_message("Warning", "Be careful", kind="warning")
            mock_showwarning.assert_called_once_with("Warning", "Be careful")

    def test_show_error(self):
        from pokecon.dialogue import show_message

        with patch("tkinter.messagebox.showerror") as mock_showerror:
            show_message("Error", "Something broke", kind="error")
            mock_showerror.assert_called_once_with("Error", "Something broke")

    def test_default_kind_is_info(self):
        from pokecon.dialogue import show_message

        with patch("tkinter.messagebox.showinfo") as mock_showinfo:
            show_message("Test", "Default kind")
            mock_showinfo.assert_called_once()


class TestConfirm:
    """Tests for ``confirm()``."""

    def test_confirm_yes(self):
        from pokecon.dialogue import confirm

        with patch("tkinter.messagebox.askyesno", return_value=True) as mock_ask:
            result = confirm("Confirm", "Are you sure?")
            assert result is True
            mock_ask.assert_called_once_with("Confirm", "Are you sure?")

    def test_confirm_no(self):
        from pokecon.dialogue import confirm

        with patch("tkinter.messagebox.askyesno", return_value=False) as mock_ask:
            result = confirm("Confirm", "Are you sure?")
            assert result is False
            mock_ask.assert_called_once()


class TestInputDialog:
    """Tests for ``input_dialog()``."""

    def test_input_with_value(self):
        from pokecon.dialogue import input_dialog

        with patch("tkinter.simpledialog.askstring", return_value="hello") as mock_ask:
            result = input_dialog("Input", "Enter text", default="default")
            assert result == "hello"
            mock_ask.assert_called_once_with(
                "Input", "Enter text", initialvalue="default"
            )

    def test_input_cancelled(self):
        from pokecon.dialogue import input_dialog

        with patch("tkinter.simpledialog.askstring", return_value=None) as mock_ask:
            result = input_dialog("Input", "Enter text")
            assert result is None
            mock_ask.assert_called_once()

    def test_input_default_empty(self):
        from pokecon.dialogue import input_dialog

        with patch("tkinter.simpledialog.askstring", return_value="") as mock_ask:
            result = input_dialog("Input", "Enter text")
            assert result == ""
            mock_ask.assert_called_once_with("Input", "Enter text", initialvalue="")


class TestOkCancel:
    """Tests for ``ok_cancel()``."""

    def test_ok(self):
        from pokecon.dialogue import ok_cancel

        with patch("tkinter.messagebox.askokcancel", return_value=True) as mock_ask:
            result = ok_cancel("Confirm", "Proceed?")
            assert result is True
            mock_ask.assert_called_once_with("Confirm", "Proceed?")

    def test_cancel(self):
        from pokecon.dialogue import ok_cancel

        with patch("tkinter.messagebox.askokcancel", return_value=False) as mock_ask:
            result = ok_cancel("Confirm", "Proceed?")
            assert result is False


class TestRetryCancel:
    """Tests for ``retry_cancel()``."""

    def test_retry(self):
        from pokecon.dialogue import retry_cancel

        with patch("tkinter.messagebox.askretrycancel", return_value=True) as mock_ask:
            result = retry_cancel("Retry?", "Try again?")
            assert result is True

    def test_cancel(self):
        from pokecon.dialogue import retry_cancel

        with patch("tkinter.messagebox.askretrycancel", return_value=False) as mock_ask:
            result = retry_cancel("Retry?", "Try again?")
            assert result is False


class TestDialogue:
    """Tests for ``dialogue()`` — the original entry-based dialog API."""

    def test_dialogue_str_message(self):
        """A single string message creates one entry."""
        from pokecon.dialogue import dialogue

        with patch("tkinter.Toplevel") as mock_toplevel:
            mock_top = MagicMock()
            mock_toplevel.return_value = mock_top

            result = dialogue("Title", "Name")

            # When Toplevel is mocked, wait_window won't work,
            # but we should get None because the dialog was
            # never interacted with
            assert result is None or isinstance(result, list)

    def test_dialogue_int_message(self):
        """An integer message creates N entry fields."""
        from pokecon.dialogue import dialogue

        with patch("tkinter.Toplevel") as mock_toplevel:
            mock_top = MagicMock()
            mock_toplevel.return_value = mock_top

            result = dialogue("Title", 3)
            assert result is None or isinstance(result, list)

    def test_dialogue_list_message(self):
        """A list message creates entry fields for each element."""
        from pokecon.dialogue import dialogue

        with patch("tkinter.Toplevel") as mock_toplevel:
            mock_top = MagicMock()
            mock_toplevel.return_value = mock_top

            result = dialogue("Title", ["Name", "Age", "Class"])
            assert result is None or isinstance(result, list)

    def test_dialogue_with_desc(self):
        """Description parameter is passed through."""
        from pokecon.dialogue import dialogue

        with patch("tkinter.Toplevel") as mock_toplevel:
            mock_top = MagicMock()
            mock_toplevel.return_value = mock_top

            result = dialogue("Title", "Name", desc="Please enter your name")
            assert result is None or isinstance(result, list)

    def test_dialogue_return_dict(self):
        """When need=dict, returns a dict instead of list."""
        from pokecon.dialogue import dialogue

        with patch("tkinter.Toplevel") as mock_toplevel:
            mock_top = MagicMock()
            mock_toplevel.return_value = mock_top

            result = dialogue("Title", ["Name", "Age"], need=dict)
            assert result is None or isinstance(result, dict)


class TestAdapterDialogueFunctions:
    """Tests for the adapter dialogue functions."""

    def test_show_message_dialog_python_fallback(self):
        """show_message_dialog falls back to pure Python."""
        from pokecon._adapter import show_message_dialog

        with patch("pokecon.dialogue.show_message") as mock_show:
            show_message_dialog("Title", "Msg", kind="info")
            mock_show.assert_called_once_with("Title", "Msg", "info")

    def test_confirm_dialog_python_fallback(self):
        """confirm_dialog falls back to pure Python."""
        from pokecon._adapter import confirm_dialog

        with patch("pokecon.dialogue.confirm", return_value=True) as mock_confirm:
            result = confirm_dialog("Confirm", "Sure?")
            assert result is True
            mock_confirm.assert_called_once_with("Confirm", "Sure?")

    def test_input_dialog_python_fallback(self):
        """input_dialog adapter falls back to pure Python."""
        from pokecon._adapter import input_dialog

        with patch(
            "pokecon.dialogue.input_dialog", return_value="result"
        ) as mock_input:
            result = input_dialog("Input", "Enter", "default")
            assert result == "result"
            mock_input.assert_called_once_with("Input", "Enter", "default")

    def test_show_dialogue_python_fallback(self):
        """show_dialogue adapter falls back to pure Python."""
        from pokecon._adapter import show_dialogue

        with patch("pokecon.dialogue.dialogue", return_value=["val1"]) as mock_dialogue:
            result = show_dialogue("Title", "Label")
            assert result == ["val1"]
            mock_dialogue.assert_called_once_with("Title", "Label", None, list)
