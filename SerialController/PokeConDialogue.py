# -*- coding: utf-8 -*-
from __future__ import annotations

import contextlib
import glob
import json
import os
import tkinter as tk
from tkinter import ttk
from typing import TYPE_CHECKING, cast, overload

if TYPE_CHECKING:
    from typing import Any, Literal

    type _CheckWidget = tuple[Literal["check"], str, bool]
    type _EntryWidget = tuple[Literal["entry"], str, str]
    type _ComboWidget = tuple[Literal["combo"], str, list[str], str]
    type _RadioWidget = tuple[Literal["radio"], str, list[str], str]
    type _SpinWidget = tuple[Literal["spin"], str, list[str], str]
    type _ScaleWidget = tuple[Literal["scale"], str, int | float, int | float, int | float, int]
    type _NextWidget = tuple[Literal["next"]]
    type WidgetSetting = (
        _CheckWidget
        | _EntryWidget
        | _ComboWidget
        | _RadioWidget
        | _SpinWidget
        | _ScaleWidget
        | _NextWidget
    )
    type DialogueList = list[WidgetSetting]

# from logging import getLogger, DEBUG, NullHandlerxx


class PokeConDialogue:
    @overload
    def __init__(
        self,
        parent: tk.Toplevel,
        title: str,
        message: int | str | list[int | str],
        desc: str | None = None,
        mode: Literal[0] = 0,
        pos: Literal[1, 2, 3] = 2,
    ) -> None: ...

    @overload
    def __init__(
        self,
        parent: tk.Toplevel,
        title: str,
        message: DialogueList,
        desc: str | None = None,
        mode: Literal[1] = 1,
        pos: Literal[1, 2, 3] = 2,
    ) -> None: ...

    def __init__(
        self,
        parent: tk.Toplevel,
        title: str,
        message: int | str | list[int | str] | DialogueList,
        desc: str | None = None,
        mode: Literal[0, 1] = 0,
        pos: Literal[1, 2, 3] = 2,
    ) -> None:
        """
        pokecon用ダイアログ生成関数(注意:mode=0と1でmessageの取り扱いが大きく異なる。)
        mode | int: 0のときEntryのみ、1のとき6種類のwidgetに対応
        pos | int: OK/Cancelの位置(1のときTOP、2のときBOTTOM、3のときBOTH)
        title | str: タイトル
        message | mode=0の場合 : int/str/list: Entryのラベル、mode=1の場合 : list[widget, widget, ...]: widgetごとの設定をリスト化したもの
        widget | list : widgetごとの設定(ウィジェットの種類によってリストの中身は異なる。以下を参照。)
        checkbox/entryの場合 : [type, subtitle, init] (例) ["check", "Check(例)", True]、["ENTRY", "Entry(例)", "初期値"]
        combobox/radiobutton/spinboxの場合 : [type, subtitle, selectlist, init] (例) ["Combo", "Combo(例)", ["hello", "world"], "hello"]、["RADIO", "Radio(例)", ["dog", "cat"],"dog"]、["Spin", "Spin(例)", list(map(str, range(10))), "3"]
        scaleの場合 : [type, subtitle, min, max, init, digit] (例) ["Scale", "scale(例)", 0, 100, 50.1, 2]
        type | str: widgetの種類(check/combo/entry/radio/spin/scaleのいずれか。大文字小文字は問わない)
        subtitle | str : widgetのタイトル
        init | checkboxの場合bool,scaleの場合int/float,その他str : 初期値
        selectlist | list : 項目のリスト
        min/max | int/float : scaleの最小値と最大値
        digit | int : 有効桁数
        return : なし
        """
        self._ls: list[str] = list()
        self.isOK: bool = False

        self.message_dialogue: tk.Toplevel = parent
        self.message_dialogue.title(title)
        self.message_dialogue.attributes("-topmost", True)  # pyright:ignore[reportUnknownMemberType]
        self.message_dialogue.protocol("WM_DELETE_WINDOW", self.close_window)

        self.main_frame: tk.Frame = tk.Frame(self.message_dialogue)

        description = desc if desc is not None else title
        self.description_label: ttk.Label = ttk.Label(
            self.main_frame,
            text=description,
            anchor="center",
        )
        self.description_label.grid(
            column=0,
            columnspan=2,
            ipadx="10",
            ipady="10",
            row=0,
            sticky="nsew",
        )

        cnt = 1
        if pos in [1, 3]:
            self.result: ttk.Frame = ttk.Frame(self.main_frame)
            self.OK: ttk.Button = ttk.Button(self.result, command=self.ok_command)
            self.OK.configure(text="OK")
            self.OK.grid(column=0, row=1, padx=5, pady=5)
            self.Cancel: ttk.Button = ttk.Button(
                self.result,
                command=self.cancel_command,
            )
            self.Cancel.configure(text="Cancel")
            self.Cancel.grid(column=1, row=1, sticky="ew", padx=5, pady=5)
            self.result.grid(column=0, columnspan=2, pady=5, row=cnt, sticky="ew")
            self.result.grid_anchor("center")
            cnt += 1

        self.inputs: ttk.Frame = ttk.Frame(self.main_frame)

        self.dialogue_ls: dict[str | int, tk.Variable] = {}
        x = self.message_dialogue.master.winfo_x()
        w = self.message_dialogue.master.winfo_width()
        y = self.message_dialogue.master.winfo_y()
        h = self.message_dialogue.master.winfo_height()
        w_ = self.message_dialogue.winfo_width()
        h_ = self.message_dialogue.winfo_height()
        self.message_dialogue.geometry(
            f"+{int(x + w / 2 - w_ / 2)}+{int(y + h / 2 - h_ / 2)}",
        )

        if mode == 0:
            self.mode0(message)  # pyright: ignore[reportArgumentType]
        else:
            self.mode1(message)  # pyright: ignore[reportArgumentType]

        self.inputs.grid(
            column=0,
            columnspan=2,
            ipadx="10",
            ipady="10",
            row=cnt,
            sticky="nsew",
        )
        self.inputs.grid_anchor("center")
        cnt += 1

        if pos in [2, 3]:
            self.result2: ttk.Frame = ttk.Frame(self.main_frame)
            self.OK2: ttk.Button = ttk.Button(self.result2, command=self.ok_command)
            self.OK2.configure(text="OK")
            self.OK2.grid(column=0, row=1, padx=5, pady=5)
            self.Cancel2: ttk.Button = ttk.Button(
                self.result2,
                command=self.cancel_command,
            )
            self.Cancel2.configure(text="Cancel")
            self.Cancel2.grid(column=1, row=1, sticky="ew", padx=5, pady=5)
            self.result2.grid(column=0, columnspan=2, pady=5, row=cnt, sticky="ew")
            self.result2.grid_anchor("center")

        self.main_frame.pack()
        self.message_dialogue.master.wait_window(self.message_dialogue)

    def mode0(self, message: str | int | list[str | int]) -> None:
        if not isinstance(message, list):
            message = [message]

        for i, m in enumerate(message):
            self.dialogue_ls[m] = tk.StringVar()
            label = ttk.Label(self.inputs, text=m)
            entry = ttk.Entry(self.inputs, textvariable=self.dialogue_ls[m])
            label.grid(column=0, row=i, sticky="nsew", padx=3, pady=3)
            entry.grid(column=1, row=i, sticky="nsew", padx=3, pady=3)

    def mode1(self, dialogue_list: DialogueList) -> None:
        frame: list[ttk.LabelFrame | None] = []

        scale_label_list: list[tk.Label] = []
        scale_index_list: list[int] = []
        scale_digit_list: list[int] = []

        def change_scale_value(
            _: tk.Event | None = None,
        ) -> None:
            for i, (index, fmt) in enumerate(
                zip(scale_index_list, scale_digit_list, strict=False),
            ):
                setting = dialogue_list[index]
                if setting[0].casefold() != "scale":
                    continue
                setting = cast("_ScaleWidget", setting)
                if fmt != 0:
                    val = round(float(self.dialogue_ls[setting[1]].get()), fmt)
                    scale_label_list[i]["text"] = f"{val}"
                    self.dialogue_ls[setting[1]].set(val)
                else:
                    scale_label_list[i]["text"] = f"{self.dialogue_ls[setting[1]].get()}"

        column0 = 0
        row0 = 0
        for i, setting in enumerate(dialogue_list):
            widget_type = setting[0].casefold()
            if widget_type == "next":
                column0 += 1
                row0 = 0
                frame.append(None)
            else:
                frame.append(ttk.LabelFrame(self.inputs, text=setting[1]))

                if widget_type == "check":
                    setting = cast("_CheckWidget", setting)
                    self.dialogue_ls[setting[1]] = tk.BooleanVar(value=setting[2])
                    widget = ttk.Checkbutton(frame[i], variable=self.dialogue_ls[setting[1]])
                    widget.grid(column=0, row=0, sticky="nsew", padx=3, pady=3)
                elif widget_type == "combo":
                    setting = cast("_ComboWidget", setting)
                    text_length = 10
                    for name in setting[2]:
                        text_length = max(text_length, len(str(name)) + 5)
                    self.dialogue_ls[setting[1]] = tk.StringVar(value=setting[3])
                    widget = ttk.Combobox(
                        frame[i],
                        values=setting[2],
                        textvariable=self.dialogue_ls[setting[1]],
                        width=text_length,
                        state="readonly",
                    )
                    widget.grid(column=0, row=0, sticky="nsew", padx=3, pady=3)
                elif widget_type == "entry":
                    setting = cast("_EntryWidget", setting)
                    self.dialogue_ls[setting[1]] = tk.StringVar(value=setting[2])
                    widget = ttk.Entry(frame[i], textvariable=self.dialogue_ls[setting[1]])
                    widget.grid(column=0, row=0, sticky="nsew", padx=3, pady=3)
                elif widget_type == "radio":
                    setting = cast("_RadioWidget", setting)
                    self.dialogue_ls[setting[1]] = tk.StringVar(value=setting[3])
                    for j, text0 in enumerate(setting[2]):
                        widget = ttk.Radiobutton(
                            frame[i],
                            text=text0,
                            variable=self.dialogue_ls[setting[1]],
                            value=text0,
                        )
                        widget.grid(column=j, row=0, sticky="nsew", padx=3, pady=3)
                elif widget_type == "scale":
                    setting = cast("_ScaleWidget", setting)
                    scale_index_list.append(i)
                    scale_digit_list.append(setting[5])
                    if setting[5] != 0:
                        self.dialogue_ls[setting[1]] = tk.DoubleVar(value=setting[4])
                        scale_label_list.append(
                            tk.Label(
                                frame[i],
                                width=10,
                                text=f"{round(self.dialogue_ls[setting[1]].get(), setting[5])}",
                            ),
                        )
                    else:
                        self.dialogue_ls[setting[1]] = tk.IntVar(value=setting[4])
                        scale_label_list.append(
                            tk.Label(
                                frame[i],
                                width=10,
                                text=f"{self.dialogue_ls[setting[1]].get()}",
                            ),
                        )
                    widget = ttk.Scale(
                        frame[i],
                        from_=setting[2],
                        to=setting[3],
                        variable=cast("tk.DoubleVar | tk.IntVar", self.dialogue_ls[setting[1]]),
                        command=change_scale_value,
                    )
                    scale_label_list[-1].grid(column=0, row=0, sticky="nsew", padx=3, pady=3)
                    widget.grid(column=1, row=0, sticky="nsew", padx=3, pady=3)
                elif widget_type == "spin":
                    setting = cast("_SpinWidget", setting)
                    self.dialogue_ls[setting[1]] = tk.StringVar(value=setting[3])
                    widget = ttk.Spinbox(
                        frame[i],
                        values=setting[2],
                        textvariable=self.dialogue_ls[setting[1]],
                    )
                    widget.grid(column=0, row=0, sticky="nsew", padx=3, pady=3)

                if f := frame[i]:
                    f.grid(column=column0, row=row0, sticky="nsew", padx=3, pady=3)
                row0 += 1

        for i, setting in enumerate(dialogue_list):
            widget_type = setting[0].casefold()
            if widget_type == "next":
                pass
            elif f := frame[i]:
                if widget_type == "scale":
                    f.grid_columnconfigure(0, weight=1)
                    f.grid_columnconfigure(1, weight=3)
                elif widget_type != "radio":
                    f.grid_columnconfigure(0, weight=1)

    def ret_value(
        self,
        need: type,
    ) -> list[Any] | dict[str | int, Any] | Literal[False]:
        if self.isOK:
            if need is dict:
                return {k: v.get() for k, v in self.dialogue_ls.items()}
            if need is list:
                return self._ls
            print("Wrong arg. Try Return list.")
            return self._ls
        return False

    def close_window(self) -> None:
        self.message_dialogue.destroy()
        self.isOK = False

    def ok_command(self) -> None:
        self._ls: list[Any] = [v.get() for k, v in self.dialogue_ls.items()]
        self.message_dialogue.destroy()
        self.isOK = True

    def cancel_command(self) -> None:
        self.message_dialogue.destroy()
        self.isOK = False


def check_widget_name(dialogue_list: list[Any], except_name: list[Any] | None = None) -> bool:
    """
    ウィジェットに同一名称がないかを確認
    """
    if except_name is None:
        except_name = []
    input_name = [
        setting[1] for setting in dialogue_list if len(setting) > 1
    ] + except_name
    checked_name = []
    output_name = [
        name
        for name in input_name
        if name not in checked_name and not checked_name.append(name)
    ]

    return len(input_name) == len(output_name)


def get_setting(filename: str) -> dict[str, Any] | None:
    """
    保存した設定値を読み込む
    """
    try:
        with open(filename, encoding="utf-8") as f:
            return json.load(f)
    except Exception:
        return None


def save_setting(filename: str, settings: dict[str, Any]) -> None:
    """
    設定値を保存する
    """
    with open(filename, "w", encoding="utf-8") as f:
        json.dump(settings, f, indent=4, ensure_ascii=False)


def generate_new_dialogue_list(dialogue_list: list[Any], filename: str | None) -> list[Any]:
    if filename is None:
        return dialogue_list
    settings = get_setting(filename)
    if not settings:
        return dialogue_list
    new_dialogue_list = []
    for setting in dialogue_list:
        if len(setting) < 2:
            pass
        else:
            with contextlib.suppress(Exception):
                setting[-1] = settings[setting[1]]
        new_dialogue_list.append(setting)
    return new_dialogue_list


def save_dialogue_settings(
    new_dialogue_list: list[Any],
    ret: list[Any] | dict[Any, Any],
    filename: str,
) -> None:
    try:
        settings = {}
        if isinstance(ret, list):
            cnt = 0
            for setting in new_dialogue_list:
                if len(setting) < 2:
                    pass
                else:
                    settings[setting[1]] = ret[cnt]
                    cnt += 1
            save_setting(filename, settings)
        else:
            save_setting(filename, ret)
    except Exception:
        print("Error: Configuration dump failed.")


def get_settings_list(dirname: str) -> list[str]:
    if os.path.isdir(dirname):
        pass
    else:
        os.makedirs(dirname)
    filename = os.path.join(dirname, "**", "*.json")
    settings_list = glob.glob(filename, recursive=True)

    len_pass = len(dirname) + 1
    return [file[len_pass:-5] for file in settings_list if file[len_pass] != "_"]
