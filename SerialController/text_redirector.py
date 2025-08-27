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
        self.q: queue.Queue[str] = queue.Queue()
        self.interval: Final = interval_ms
        self.text_widget: Final = text_widget
        self.text_widget.after(self.interval, self._drain)
        self.always_atutoscroll: bool = always_atutoscroll

    def write(self, text: str) -> None:
        self.q.put(text)

    def flush(self) -> None:
        pass

    def _should_scroll(self) -> bool:
        if self.always_atutoscroll:
            return True
        return self._is_at_bottom()

    def _is_at_bottom(self) -> bool:
        _, last = self.text_widget.yview()  # pyright:ignore[reportUnknownMemberType]
        return abs(last - 1.0) < 1e-3

    def _drain(self) -> None:
        buf: list[str] = []
        try:
            while True:
                buf.append(self.q.get_nowait())
        except queue.Empty:
            pass
        if buf:
            do_scroll = self._should_scroll()
            self.text_widget.configure(state="normal")
            self.text_widget.insert("end", "".join(buf))
            self.text_widget.configure(state="disabled")
            if do_scroll:
                self.text_widget.yview("end")  # pyright:ignore[reportUnknownMemberType]
        self.text_widget.after(self.interval, self._drain)
