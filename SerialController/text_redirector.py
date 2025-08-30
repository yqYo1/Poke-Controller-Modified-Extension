from __future__ import annotations

import queue
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    import tkinter as tk
    from typing import Final


class TextRedirector:
    def __init__(
        self,
        text_widget: tk.Text,
        interval_ms: int = 50,
        always_atutoscroll: bool = False,
    ) -> None:
        self.q: queue.Queue[tuple[bool, str]] = queue.Queue()
        self.interval: Final = interval_ms
        self.text_widget: Final = text_widget
        self.always_atutoscroll: bool = always_atutoscroll
        # self.text_widget.after(self.interval, self.update)

    def write(self, string: str, clear: bool = False) -> None:
        if clear:
            self.q = queue.Queue()

        self.q.put((clear, string))

    def flush(self) -> None:
        pass

    def _is_at_bottom(self) -> bool:
        _, last = self.text_widget.yview()  # pyright:ignore[reportUnknownMemberType]
        return abs(last - 1.0) < 1e-3

    def _should_scroll(self) -> bool:
        if self.always_atutoscroll:
            return True
        return self._is_at_bottom()

    def update(self) -> None:
        buf: list[tuple[bool, str]] = []
        try:
            while True:
                buf.append(self.q.get_nowait())
        except queue.Empty:
            pass
        buf_s = [i[1] for i in buf]
        if buf_s:
            do_scroll = self._should_scroll()
            self.text_widget.configure(state="normal")

            if buf[0][0]:
                self.text_widget.delete("1.0", "end")

            s = "".join(buf_s).replace("\r\n", "\n")
            parts = s.split("\r")
            if parts[0]:
                self.text_widget.insert("end", parts[0])
            for seg in parts[1:]:
                self.text_widget.delete("end-1c linestart", "end-1c")
                self.text_widget.insert("end", seg)

            self.text_widget.configure(state="disabled")
            if do_scroll:
                self.text_widget.yview("end")  # pyright:ignore[reportUnknownMemberType]
        #     self.text_widget.update_idletasks()
        # self.text_widget.after(self.interval, self.update)
