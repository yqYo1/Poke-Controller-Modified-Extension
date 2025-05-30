# -*- coding: utf-8 -*-
from __future__ import annotations

import configparser
import io
import json
import os
from logging import DEBUG, NullHandler, getLogger
from typing import TYPE_CHECKING

import cv2
import numpy as np
import requests
from PIL import Image

if TYPE_CHECKING:
    from logging import Logger
    from typing import Final

    from cv2.typing import MatLike


def convert_bgr_to_bytes(image_bgr: MatLike) -> bytes:
    """
    BGRの画像をbyte形式に変換する
    """
    image_rgb = cv2.cvtColor(image_bgr, cv2.COLOR_BGR2RGB)
    image = Image.fromarray(image_rgb)
    png = io.BytesIO()  # 空のio.BytesIOオブジェクトを用意
    image.save(
        png,
        format="png",
    )  # 空のio.BytesIOオブジェクトにpngファイルとして書き込み
    return png.getvalue()  # io.BytesIOオブジェクトをbytes形式で読みとり


class Discord_Notify:
    DISCORD_SETTING_PATH: str = os.path.join(
        os.path.dirname(__file__),
        "profiles",
        "default",
        "discord_token.ini",
    )

    def __init__(
        self,
        webhook_url: str = "webhook_url",  # noqa: ARG002
        username: str = "username",  # noqa: ARG002
        avatar_url: str = "",  # noqa: ARG002
        token_name: str = "token",  # noqa: ARG002
    ) -> None:
        self._logger: Final[Logger] = getLogger(__name__)
        self._logger.addHandler(NullHandler())
        self._logger.setLevel(DEBUG)
        self._logger.propagate = True

        self.default_username: str = f"Poke-Controller Modified Extension (profile: {os.path.basename(os.path.dirname(self.DISCORD_SETTING_PATH))})"

        self.res = None
        self.setting_file = configparser.ConfigParser(
            comment_prefixes="#",
            allow_no_value=True,
        )
        self.open_file_with_utf8()

        self.section_list = list(self.setting_file.keys())
        self.webhook_url_dict = {
            key: self.setting_file[key]["webhook_url"]
            for key in self.section_list
            if "DISCORD_WEBHOOK" in key
        }
        self.username_dict = {
            key: self.setting_file[key]["username"]
            for key in self.section_list
            if "DISCORD_WEBHOOK" in key
        }
        self.avatar_url_dict = {
            key: self.setting_file[key]["avatar_url"]
            for key in self.section_list
            if "DISCORD_WEBHOOK" in key
        }
        self.webhook_keys = [
            key for key in self.section_list if "DISCORD_WEBHOOK" in key
        ]

        try:
            self.res = [
                requests.get(self.webhook_url_dict[key], timeout=15)
                for key in self.webhook_keys
            ]
            self.status = [responses.status_code for responses in self.res]
            self.chk_webhook_json = [responses.json() for responses in self.res]
        except Exception as e:
            self._logger.error(e)

    def open_file_with_utf8(self) -> None:
        """
        utf-8 のファイルを BOM ありかどうかを自動判定して読み込む
        (ファイルがない場合は空のファイルを作成する)
        """
        if os.path.isfile(self.DISCORD_SETTING_PATH):
            is_with_bom = self.is_utf8_file_with_bom(self.DISCORD_SETTING_PATH)

            encoding = "utf-8-sig" if is_with_bom else "utf-8"

            self._logger.debug("Load token file")
            self.setting_file.read(self.DISCORD_SETTING_PATH, encoding)
        else:
            dirname = os.path.dirname(self.DISCORD_SETTING_PATH)
            if not os.path.isdir(dirname):
                os.makedirs(dirname)
                self._logger.debug(f"mkdir: '{dirname}'")
            self.setting_file["DISCORD"] = {
                "channel_id": "",
                "token": "",
            }
            self.setting_file["DISCORD_WEBHOOK"] = {
                "webhook_url": "",
                "username": "",
                "avatar_url": "",
            }
            with open(self.DISCORD_SETTING_PATH, "w", encoding="utf-8") as file:
                self.setting_file.write(file)
            os.chmod(path=self.DISCORD_SETTING_PATH, mode=0o644)
            self._logger.debug("Generate token file")

    def is_utf8_file_with_bom(self, filename: str) -> bool:
        """
        utf-8 ファイルが BOM ありかどうかを判定する
        """
        with open(filename, encoding="utf-8") as f:
            return f.readline()[0] == "\ufeff"

    def __str__(self) -> str:
        for stat in self.status:
            if stat == 401:
                self._logger.error("Invalid url")
                return "DISCORD API Check FAILED."
            if stat == 200:
                self._logger.info("Valid url")
                return "DISCORD API Check OK!"
        return "DISCORD API Check FAILED."

    def send_message(
        self,
        notification_message: str | None = None,
        image: MatLike | None = None,
        keys: str | list[str] = "DISCORD_WEBHOOK",
    ) -> None:
        """
        DISCORDにテキスト/画像を通知する
        imageが設定されていなければテキストのみ、設定されていればテキストと画像を通知する
        imageはBGRを想定する
        """
        if keys == "ALL":
            keys = self.webhook_keys
        elif isinstance(keys, str):
            keys = [keys]
        else:
            print("keysのフォーマットが間違っています")
            self._logger.error('Using the wrong "keys" format')

        # 画像の有無判定
        flag_image = False
        if isinstance(image, np.ndarray):
            flag_image = True

        for key in keys:
            try:
                content = {}
                if self.username_dict[key] != "":
                    content["username"] = self.username_dict[key]
                else:
                    content["username"] = self.default_username
                if self.avatar_url_dict[key] != "":
                    content["avatar_url"] = self.avatar_url_dict[key]
                if notification_message:
                    content["content"] = notification_message
                else:
                    content["content"] = ""

                files = {"payload_json": (None, json.dumps(content))}

                if flag_image:
                    files["media"] = ("pokecon_image.png", convert_bgr_to_bytes(image))
                else:
                    pass

                if notification_message:
                    if flag_image:
                        send_data_type = "テキスト・画像"
                        send_data_type_eng = "Text & Image"
                    else:
                        send_data_type = "テキスト"
                        send_data_type_eng = "Text"
                elif flag_image:
                    send_data_type = "画像"
                    send_data_type_eng = "Image"
                else:
                    send_data_type = "(empty)"
                    send_data_type_eng = "empty"

                self.res = requests.post(
                    self.webhook_url_dict[key],
                    files=files,
                    timeout=15,
                )

                if self.res.status_code in [200, 204]:
                    print(f"[{key}]{send_data_type}を送信しました。")
                    self._logger.info(f"Send {send_data_type_eng}")
                else:
                    print(
                        f"[{key}]{send_data_type}の送信に失敗しました。({self.res.status_code})",
                    )
                    self._logger.error(f"Failed to send {send_data_type_eng}")
            except Exception:
                print(f"[{key}]webhook urlを確認してください。")
                self._logger.error("Using the wrong token")

    def getRateLimit(self) -> None:
        try:
            for i, name in enumerate(self.webhook_keys):
                print(f"For: {name}")
                print("X-RateLimit-Limit: " + self.res[i].headers["X-RateLimit-Limit"])
                print(
                    "X-RateLimit-Remaining: "
                    + self.res[i].headers["X-RateLimit-Remaining"],
                )
                import datetime

                dt = datetime.datetime.fromtimestamp(
                    int(self.res[i].headers["X-RateLimit-Reset"]),
                    datetime.timezone(datetime.timedelta(hours=9)),
                )
                print("Reset time:", dt, "\n")

                self._logger.info(
                    f"DISCORD API - Limit: {self.res[i].headers['X-RateLimit-Limit']}",
                )
                self._logger.info(
                    f"DISCORD API - Remaining: {self.res[i].headers['X-RateLimit-Remaining']}",
                )
                self._logger.info(f"Reset time: {dt}")
        except AttributeError as e:
            self._logger.error(e)
        except KeyError as e:
            self._logger.error(e)


if __name__ == "__main__":
    """
    status  HTTPステータスコードに準拠した値
       200  成功時
       401  アクセストークンが無効
    """
    # import cv2
    # cap = cv2.VideoCapture(0)
    # cap.set(cv2.CAP_PROP_FRAME_WIDTH, 1280)
    # cap.set(cv2.CAP_PROP_FRAME_HEIGHT, 720)
    # ret, frame = cap.read()

    DISCORD = Discord_Notify()
    DISCORD.getRateLimit()
    # DISCORD.send_message(notification_message="test")
    # DISCORD.send_message(notification_message="test", image=frame)
