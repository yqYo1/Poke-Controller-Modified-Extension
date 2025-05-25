# -*- coding: utf-8 -*-
from __future__ import annotations

import os
import tkinter as tk
from abc import ABC, abstractmethod
from typing import TYPE_CHECKING

from ExternalTools import MQTTCommunications, SocketCommunications
from PokeConDialogue import (
    PokeConDialogue,
    check_widget_name,
    generate_new_dialogue_list,
    get_settings_list,
    save_dialogue_settings,
)

if TYPE_CHECKING:
    from collections.abc import Callable
    from tkinter import Text
    from typing import Literal

    from Commands.Sender import Sender
    from GuiAssets import CaptureArea

# CommandBaseにGUIに関連する関数を集約する。
# print/widget/socket/mqtt関連


class Command(ABC):
    # __metaclass__ = ABCMeta
    text_area_1: Text | None = None
    text_area_2: Text | None = None
    stdout_destination: str = "1"
    pos_dialogue_buttons: str = "2"
    isPause: bool = False
    canvas: CaptureArea | None = None
    isGuide: bool = False
    isSimilarity: bool = False
    isImage: bool = False
    isWinNotStart: bool = False
    isWinNotEnd: bool = False
    isLineNotStart: bool = False
    isLineNotEnd: bool = False
    isDiscordNotStart: bool = False
    isDiscordNotEnd: bool = False
    app_name: str = ""
    cur_command_name: str = ""
    profilename: str = ""

    def __init__(self) -> None:
        self.isRunning: bool = False

        self.message_dialogue: tk.Toplevel | None = None
        self.socket0: SocketCommunications = SocketCommunications()
        self.mqtt0: MQTTCommunications = MQTTCommunications("")

    @abstractmethod
    def start(self, ser: Sender, postProcess: Callable[[], None]) -> None:
        pass

    @abstractmethod
    def end(self, ser: Sender) -> None:
        pass

    # MCUコマンドでは使用しない為ここではなくPythonCommandBaseに定義するのが適当?
    # @abstractmethod
    # def checkIfAlive(self) -> Literal[True]:
    #     pass

    ############### print functions ###############
    def print_s(self, *objects: object, sep: str = " ", end: str = "\n") -> None:
        print(*objects, sep=sep, end=end)

    def print_t1(self, *objects: object, sep: str = " ", end: str = "\n") -> None:
        """
        上側のログ画面に文字列を出力する
        """
        if self.text_area_1 is None:
            return
        try:
            txt = sep.join([str(obj) for obj in objects]) + end
            self.text_area_1.config(state="normal")
            self.text_area_1.insert("end", txt)
            self.text_area_1.config(state="disabled")
            self.text_area_1.see("end")
        except Exception:
            print(*objects, sep=sep, end=end)

    def print_t2(self, *objects: object, sep: str = " ", end: str = "\n") -> None:
        """
        下側のログ画面に文字列を出力する
        """
        if self.text_area_2 is None:
            return
        try:
            txt = sep.join([str(obj) for obj in objects]) + end
            self.text_area_2.config(state="normal")
            self.text_area_2.insert("end", txt)
            self.text_area_2.config(state="disabled")
            self.text_area_2.see("end")
        except Exception:
            print(*objects, sep=sep, end=end)

    def print_t(self, *objects: object, sep: str = " ", end: str = "\n") -> None:
        """
        標準出力先として割り当てられていない方のログ画面に文字列を出力する
        """
        if self.stdout_destination == "1":
            self.print_t2(*objects, sep=sep, end=end)
        elif self.stdout_destination == "2":
            self.print_t1(*objects, sep=sep, end=end)

    def print_ts(self, *objects: object, sep: str = " ", end: str = "\n") -> None:
        """
        標準出力先として割り当てられている方のログ画面に文字列を出力する
        """
        if self.stdout_destination == "1":
            self.print_t1(*objects, sep=sep, end=end)
        elif self.stdout_destination == "2":
            self.print_t2(*objects, sep=sep, end=end)

    def print_t1b(
        self,
        mode: Literal["w", "a", "d"],
        *objects: object,
        sep: str = " ",
        end: str = "\n",
    ) -> None:
        """
        上側のログ画面に文字列を出力する
        mode: ['w'/'a'/'d'] 'w'上書き, 'a'追記, 'd'削除
        """
        if self.text_area_1 is None:
            return
        try:
            txt = sep.join([str(obj) for obj in objects]) + end
            self.text_area_1.config(state="normal")
            if mode in ["w", "d"]:
                self.text_area_1.delete("1.0", "end")
            if mode == "w":
                self.text_area_1.insert("1.0", txt)
            elif mode == "a":
                self.text_area_1.insert("end", txt)
            self.text_area_1.config(state="disabled")
            self.text_area_1.see("end")
        except Exception:
            print(*objects, sep=sep, end=end)

    def print_t2b(
        self,
        mode: Literal["w", "a", "d"],
        *objects: object,
        sep: str = " ",
        end: str = "\n",
    ) -> None:
        """
        下側のログ画面に文字列を出力する
        mode: ['w'/'a'/'d'] 'w'上書き, 'a'追記, 'd'削除
        """
        if self.text_area_2 is None:
            return
        try:
            txt = sep.join([str(obj) for obj in objects]) + end
            self.text_area_2.config(state="normal")
            if mode in ["w", "d"]:
                self.text_area_2.delete("1.0", "end")
            if mode == "w":
                self.text_area_2.insert("1.0", txt)
            elif mode == "a":
                self.text_area_2.insert("end", txt)
            self.text_area_2.config(state="disabled")
            self.text_area_2.see("end")
        except Exception:
            print(*objects, sep=sep, end=end)

    def print_tb(
        self,
        mode: Literal["w", "a", "d"],
        *objects: object,
        sep: str = " ",
        end: str = "\n",
    ) -> None:
        """
        標準出力先として割り当てられていない方のログ画面に文字列を出力する
        mode: ['w'/'a'/'d'] 'w'上書き, 'a'追記, 'd'削除
        """
        if self.stdout_destination == "1":
            self.print_t2b(mode, *objects, sep=sep, end=end)
        elif self.stdout_destination == "2":
            self.print_t1b(mode, *objects, sep=sep, end=end)

    def print_tbs(
        self,
        mode: Literal["w", "a", "d"],
        *objects: object,
        sep: str = " ",
        end: str = "\n",
    ) -> None:
        """
        標準出力先として割り当てられている方のログ画面に文字列を出力する
        mode: ['w'/'a'/'d'] 'w'上書き, 'a'追記, 'd'削除
        """
        if self.stdout_destination == "1":
            self.print_t1b(mode, *objects, sep=sep, end=end)
        elif self.stdout_destination == "2":
            self.print_t2b(mode, *objects, sep=sep, end=end)

    def dialogue(
        self,
        title: str,
        message: int | str | list,
        desc: str | None = None,
        need: type = list,
    ) -> list | dict:
        """
        保存機能なしのダイアログ(Entryのみ)
        title: ダイアログのウインドウ名
        message: Refer to PokeConDialogue.py
        desc: description
        need: 出力する形式
        """
        # ダイアログ呼び出し
        self.message_dialogue = tk.Toplevel()
        ret = PokeConDialogue(
            self.message_dialogue,
            title,
            message,
            desc=desc,
            pos=int(self.pos_dialogue_buttons),
        ).ret_value(need)
        self.message_dialogue = None
        if not ret:
            self.finish()
            return None
        return ret

    def dialogue6widget(
        self,
        title: str,
        dialogue_list: list,
        desc: str | None = None,
        need: type = list,
    ) -> list | dict:
        """
        保存機能なしのダイアログ
        title: ダイアログのウインドウ名
        dialogue_list: Refer to PokeConDialogue.py
        desc: description
        need: 出力する形式
        """
        # ウィジェット名重複チェック
        if check_widget_name(dialogue_list):
            pass
        else:
            print("ウィジェット名に重複があります。重複しない名称を設定してください。")
            self.finish()  # pyright: ignore[reportAttributeAccessIssue,reportUnknownMemberType]

        # ダイアログ呼び出し
        self.message_dialogue = tk.Toplevel()
        ret = PokeConDialogue(
            self.message_dialogue,
            title,
            dialogue_list,
            desc=desc,
            mode=1,
            pos=int(self.pos_dialogue_buttons),
        ).ret_value(need)
        self.message_dialogue = None

        if not ret:
            self.finish()
            return None
        return ret

    def dialogue6widget_save_settings(
        self,
        title: str,
        dialogue_list: list,
        filename: str,
        desc: str | None = None,
        need: type = list,
    ) -> list | dict:
        """
        前の設定を呼び出すタイプのダイアログ
        title: ダイアログのウインドウ名
        dialogue_list: Refer to PokeConDialogue.py
        filename: 前の設定を保存するファイルのパス(絶対パス)
        desc: description
        need: 出力する形式
        """

        reserved_name = ["[PokeCon]設定ファイル名", "[PokeCon]設定を保存"]
        # ウィジェット名重複チェック
        if check_widget_name(dialogue_list, except_name=reserved_name):
            pass
        else:
            print(
                "ウィジェット名に重複があります。重複しない名称を設定してください。"
                f"また、「{reserved_name[0]}」および「{reserved_name[1]}」のウィジェット名は使用できません。",
            )
            self.finish()

        if check_widget_name(dialogue_list):
            pass
        else:
            print("ウィジェット名に重複があります。重複しない名称を設定してください。")
            self.finish()

        # ディレクトリがない場合は作成
        if not os.path.exists(os.path.dirname(filename)):
            os.makedirs(os.path.dirname(filename))
            print("設定ファイル保存用ディレクトリを作成しました。")

        # 過去の履歴を初期値に反映
        new_dialogue_list = generate_new_dialogue_list(dialogue_list, filename)

        # ダイアログ呼び出し
        self.message_dialogue = tk.Toplevel()
        ret = PokeConDialogue(
            self.message_dialogue,
            title,
            new_dialogue_list,
            desc=desc,
            mode=1,
            pos=int(self.pos_dialogue_buttons),
        ).ret_value(need)
        self.message_dialogue = None

        if not ret:
            self.finish()
        # [ok]選択時に入力履歴を保存
        save_dialogue_settings(new_dialogue_list, ret, filename)
        return ret

    def dialogue6widget_select_settings(
        self,
        title: str,
        dialogue_list: list,
        dirname: str,
        desc: str | None = None,
        need: type = list,
    ) -> list | dict:
        """
        保存した設定を選択して呼び出すタイプのダイアログ
        title: ダイアログのウインドウ名
        dialogue_list: Refer to PokeConDialogue.py
        filename: 設定を保存するディレクトリのパス(絶対パス)
        desc: description
        need: 出力する形式
        """
        reserved_name = ["[PokeCon]設定ファイル名", "[PokeCon]設定を保存"]
        # ウィジェット名重複チェック
        if check_widget_name(dialogue_list, except_name=reserved_name):
            pass
        else:
            print(
                "ウィジェット名に重複があります。重複しない名称を設定してください。"
                f"また、「{reserved_name[0]}」および「{reserved_name[1]}」のウィジェット名は使用できません。",
            )
            self.finish()

        # 設定ファイル名リスト生成
        settings_list = get_settings_list(dirname)

        # GUI画面表示
        ret = self.dialogue6widget(
            "Select Preset",
            [["Combo", "---設定ファイル選択---", settings_list, "(選択して下さい)"]],
        )

        # ディレクトリがない場合は作成
        if not os.path.exists(dirname):
            os.makedirs(dirname)
            print("設定ファイル保存用ディレクトリを作成しました。")

        filename = os.path.join(dirname, f"{ret[0]}.json")
        if not os.path.exists(filename):
            print("設定ファイルを選択しなかったのでデフォルト値で起動します。")
            filename = None
        else:
            pass

        # 過去の履歴を初期値に反映
        new_dialogue_list = generate_new_dialogue_list(dialogue_list, filename)

        # 設定保存用のウィジェットを追加
        new_dialogue_list.append(["Entry", "[PokeCon]設定ファイル名", ""])
        new_dialogue_list.append(["Check", "[PokeCon]設定を保存", False])

        # ダイアログ呼び出し
        self.message_dialogue = tk.Toplevel()
        ret = PokeConDialogue(
            self.message_dialogue,
            title,
            new_dialogue_list,
            desc=desc,
            mode=1,
            pos=int(self.pos_dialogue_buttons),
        ).ret_value(need)
        self.message_dialogue = None

        if not ret:
            self.finish()
            return None
        # 設定保存用のウィジェット関連の要素を削除
        if need is list:
            preset_name = ret[-2]
            save_preset = ret[-1]
            ret = ret[:-2]
        else:
            preset_name = ret["[PokeCon]設定ファイル名"]
            save_preset = ret["[PokeCon]設定を保存"]
            ret.pop("[PokeCon]設定ファイル名")
            ret.pop("[PokeCon]設定を保存")

        # [ok]選択時に入力履歴を保存
        if save_preset and preset_name != "":
            filename = os.path.join(dirname, f"{preset_name}.json")
            save_dialogue_settings(new_dialogue_list[:-2], ret, filename)
        filename = os.path.join(dirname, "前回の設定.json")
        save_dialogue_settings(new_dialogue_list[:-2], ret, filename)
        return ret

    ############### Socket functions ###############
    def socket_change_alive(self, flag: bool) -> None:
        self.socket0.alive = flag

    def socket_change_ipaddr(self, addr: str) -> None:
        """
        IPアドレスを変更する
        return:なし
        addr|str:IPアドレス
        """
        self.socket0.change_ipaddr(addr)

    def socket_change_port(self, port: int) -> None:
        """
        ポート番号を変更する
        return:なし
        port|int:ポート番号
        """
        self.socket0.change_port(port)

    def socket_connect(self) -> None:
        """
        socket通信用のserverと接続する
        return:なし
        """
        self.socket0.sock_connect()

    def socket_disconnect(self) -> None:
        """
        socket通信用のserverから切断する
        return:なし
        """
        self.socket0.sock_disconnect()

    def socket_receive_message(self, header: str, show_msg: bool = False) -> str | None:
        """
        socketを用いて先頭が特定の文字列であるメッセージを受信する
        return output|str:受信した文字列
        header|str:受信したい文字列(先頭)
        show_msg|bool:受信した文字列を出力する
        """
        output = self.socket0.receive_message(header, show_msg=show_msg)
        self.checkIfAlive()
        return output

    def socket_receive_message2(
        self,
        headerlist: list[str],
        show_msg: bool = False,
    ) -> str | None:
        """
        socketを用いて先頭が特定の文字列(複数設定可能)であるメッセージを受信する
        return output|str:受信した文字列
        headerlist|list[str]:受信したい文字列(先頭)のリスト
        show_msg|bool:受信した文字列を出力する
        """
        output = self.socket0.receive_message2(headerlist, show_msg=show_msg)
        self.checkIfAlive()
        return output

    def socket_transmit_message(self, message: str) -> None:
        """
        socketを用いてメッセージを送信する
        return:なし
        message|str:送信するメッセージ
        """
        self.socket0.transmit_message(message)
        self.checkIfAlive()

    ############### MQTT functions ###############
    def mqtt_change_broker_address(self, broker_address: str) -> None:
        """
        brokerアドレスを変更する
        return:なし
        broker_address|str:brokerアドレス
        """
        self.mqtt0.broker_address = broker_address

    def mqtt_change_id(self, mqtt_id: str) -> None:  # idは組み込み関数なので変更
        """
        IDを変更する
        return:なし
        id|str:ID
        """
        self.mqtt0.id_ = mqtt_id

    def mqtt_change_pub_token(self, pub_token: str) -> None:
        """
        pub用tokenを変更する
        return:なし
        pub_token|str:pub用token
        """
        self.mqtt0.pub_token = pub_token

    def mqtt_change_sub_token(self, sub_token: str) -> None:
        """
        sub用tokenを変更する
        return:なし
        sub_token|str:sub用token
        """
        self.mqtt0.sub_token = sub_token

    def mqtt_change_clientId(self, clientId: str) -> None:
        """
        接続者名を変更する
        return:なし
        clientId|str:接続者名
        """
        self.mqtt0.clientId = clientId

    def mqtt_receive_message(
        self,
        roomid: str,
        header: str,
        show_msg: bool = False,
    ) -> str | None:
        """
        MQTTを用いて先頭が特定の文字列であるメッセージを受信する
        return output|str:受信した文字列
        roomid|str:ROOM ID(topic)
        header|str:受信したい文字列(先頭)
        show_msg|bool:受信した文字列を出力する
        """
        output = self.mqtt0.receive_message(roomid, header, show_msg=show_msg)
        self.checkIfAlive()
        return output

    def mqtt_receive_message2(
        self,
        roomid: str,
        headerlist: str,
        show_msg: bool = False,
    ) -> str | None:
        """
        MQTTを用いて先頭が特定の文字列(複数設定可能)であるメッセージを受信する
        return output|str:受信した文字列
        roomid|str:ROOM ID(topic)
        headerlist|list[str]:受信したい文字列(先頭)のリスト
        show_msg|bool:受信した文字列を出力する
        """
        output = self.mqtt0.receive_message2(roomid, headerlist, show_msg=show_msg)
        self.checkIfAlive()
        return output

    def mqtt_transmit_message(self, roomid: str, message: str) -> None:
        """
        MQTTを用いてメッセージを送信する
        return:なし
        roomid|str:ROOM ID(topic)
        message|str:送信するメッセージ
        """
        self.mqtt0.transmit_message(roomid, message)
        self.checkIfAlive()
