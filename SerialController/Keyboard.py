# -*- coding: utf-8 -*-
from __future__ import annotations

import configparser
import os
from logging import DEBUG, NullHandler, getLogger
from typing import TYPE_CHECKING

from Commands.Keys import Button, Direction, Hat
from pynput.keyboard import Key, Listener

if TYPE_CHECKING:
    from pynput.keyboard import KeyCode
    from logging import Logger


# This handles keyboard interactions
class Keyboard:
    def __init__(self) -> None:
        self._logger: Logger = getLogger(__name__)
        self._logger.addHandler(NullHandler())
        self._logger.setLevel(DEBUG)
        self._logger.propagate = True

        self.listener = Listener(on_press=self.on_press, on_release=self.on_release)

    def listen(self) -> None:
        self.listener.start()
        self._logger.debug("Keyboard control start")

    def stop(self) -> None:
        self.listener.stop()
        self._logger.debug("Keyboard control stop")

    def on_press(self, key: Key | KeyCode | None) -> None:
        try:
            if key is not None:
                print(f"alphanumeric key {key.char} pressed")
        except AttributeError:
            print(f"special key {key} pressed")

    def on_release(self, key: Key | KeyCode | None) -> None:
        print(f"{key} released")


# This regards a keyboard inputs as Switch controller
class SwitchKeyboardController(Keyboard):
    SETTING_PATH: str = os.path.join(
        os.path.dirname(__file__),
        "profiles",
        "default",
        "settings.ini",
    )

    def __init__(self, keyPress) -> None:
        super().__init__()

        self._logger: Logger = getLogger(__name__)
        self._logger.addHandler(NullHandler())
        self._logger.setLevel(DEBUG)
        self._logger.propagate = True

        self.to_use = Button.A
        self.setting = configparser.ConfigParser()
        self.setting.optionxform = str

        self._logger.debug("Loading Keyboard control key-map setting")
        if os.path.isfile(self.SETTING_PATH):
            self.setting.read(self.SETTING_PATH, encoding="utf-8")
        self.key = keyPress
        self.holding = []
        self.holdingDir = []
        self.holdingHatDir = []
        self.key_map_B = {
            (i[1] if len(i[1]) == 1 else eval(str(i[1]))): eval(i[0])
            for i in self.setting.items("KeyMap-Button")
        }
        self.key_map_D = {
            (i[1] if len(i[1]) == 1 else eval(str(i[1]))): eval(i[0])
            for i in self.setting.items("KeyMap-Direction")
        }
        self.key_map_H = {
            (i[1] if len(i[1]) == 1 else eval(str(i[1]))): eval(i[0])
            for i in self.setting.items("KeyMap-Hat")
        }
        self.key_map = {**self.key_map_B, **self.key_map_D, **self.key_map_H}
        # self._logger.debug(self.key_map)

        self._logger.debug("Initialization finished")

    def on_press(self, key) -> None:
        # for debug (show row key data)
        # super().on_press(key)

        if key is None:
            print("unknown key has input")
            self._logger.warning("Unknown key has input")

        # self._logger.debug(f"key type is {type(key)}")
        # self._logger.debug(f"key  is '{type(key)}'")
        # self._logger.debug(f"holding is {self.holding}")

        try:
            _k = key.char
            key_type = type(self.key_map[_k])
        except AttributeError:
            try:
                _k = key
                key_type = type(self.key_map[_k])
            except KeyError:
                return
        except Exception as e:
            self._logger.error("Key has not recognized")
            self._logger.error(type(e))
            self._logger.error(e)
            _k = None
            key_type = None

        try:
            # self._logger.debug(self.holding)
            if key_type is type(Button.A):
                # self._logger.debug("Button pushed")
                if _k in self.holding:
                    return
                for k in self.key_map:
                    if _k == k:
                        self.key.input(self.key_map[_k])
                        self.holding.append(_k)
            elif key_type is type(Direction.LEFT):
                if _k in self.holdingDir:
                    return
                for k in self.key_map:
                    if _k == k:
                        self.holdingDir.append(_k)
                        self.inputDir(self.holdingDir)
            elif key_type is type(Hat.TOP):
                # self._logger.debug("Hat pushed")
                if _k in self.holding:
                    return
                for k in self.key_map:
                    if _k == k:
                        self.key.input(self.key_map[_k])
                        self.holding.append(_k)
                        # self._logger.debug(f"stick: {key}")
            # elif key_type is type(Hat.TOP):
            #     self._logger.debug("Hat")
            #     if _k in self.holdingHatDir:
            #         return
            #     for k in self.key_map.keys():
            #         if _k == k:
            #             self.holdingHatDir.append(_k)
            #             self.inputDir(self.holdingHatDir)
            #             # self._logger.debug(f"stick: {key}")
            # self._logger.debug(f"k is {_k}")

        # for special keys
        except AttributeError:
            if key in self.holdingDir:
                return

            for k in self.key_map:
                if key == k:
                    self.holdingDir.append(key)
                    self.inputDir(self.holdingDir)
                    # self._logger.debug(f"stick: {key}")

    def on_release(self, key) -> None:
        # self._logger.debug(f"key  is '{key}'")

        if key is None:
            print("unknown key has released")
            self._logger.warning("Unknown key has input")
        try:
            _k = key.char
            key_type = type(self.key_map[_k])
        except AttributeError:
            try:
                _k = key
                key_type = type(self.key_map[_k])
            except KeyError:
                return
        except Exception as e:
            self._logger.error("Key has not recognized")
            self._logger.error(type(e))
            self._logger.error(e)
            _k = None
            key_type = None

        try:
            if key_type is type(Button.A):
                # self._logger.debug("Button released")
                if _k in self.holding:
                    self.holding.remove(_k)
                    # if not self.holdingDir:
                    #     self.key.inputEnd(self.key_map[_k])
                    self.key.inputEnd(self.key_map[_k])
            elif key_type is type(Direction.LEFT):
                # self._logger.debug("Direction")
                if _k in self.holdingDir:
                    self.holdingDir.remove(_k)
                    if not self.holdingDir:
                        self.key.inputEnd(self.key_map[_k])
                        # self._logger.debug(f"holding {self.holdingDir}")
                    self.inputDir(self.holdingDir)
            elif key_type is type(Hat.TOP):
                # self._logger.debug(f"Hat released, {self.key_map[_k]}")
                if _k in self.holding:
                    self.holding.remove(_k)
                    # self._logger.debug("try")
                    # if not self.holdingDir:
                    #     self.key.inputEnd(self.key_map[_k], unset_hat=True)
                    self.key.inputEnd(self.key_map[_k], unset_hat=True)
            # elif key_type is type(Hat.TOP):
            #     if _k in self.holdingHatDir:
            #         self.holdingHatDir.remove(_k)
            #         if not self.holdingHatDir:
            #             self.key.inputEnd(self.key_map[_k])
            #             # self._logger.debug(f"holding {self.holdingDir}")
            #         self.inputDir(self.holdingHatDir)
            # Todo: Hat のリリース処理追加

            # self._logger.debug("done")
            # self._logger.debug(f"push out {self.key_map[key.char]}")

        except AttributeError as e:
            self._logger.debug(e)
            # if key in self.holdingDir:
            #     self.holdingDir.remove(key)
            #     if self.holdingDir == []:
            #         self.key.inputEnd(self.key_map[key])
            #         # self._logger.debug(f"holding {self.holdingDir}")
            #     self.inputDir(self.holdingDir)

    def inputDir(self, dirs) -> None:
        self._logger.debug(dirs)
        if len(dirs) == 0:
            return
        if len(dirs) == 1:
            self.key.input(self.key_map[dirs[0]])
        elif len(dirs) > 1:
            valid_dirs = dirs[-2:]  # set only last 2 directions
            to_input = []
            if Key.up in valid_dirs:
                if Key.right in valid_dirs:
                    to_input.append(Direction.UP_RIGHT)
                elif Key.left in valid_dirs:
                    to_input.append(Direction.UP_LEFT)
            elif Key.down in valid_dirs:
                if Key.left in valid_dirs:
                    to_input.append(Direction.DOWN_LEFT)
                elif Key.right in valid_dirs:
                    to_input.append(Direction.DOWN_RIGHT)

            # if Key.up in valid_dirs:
            #     if Key.right in valid_dirs:
            #         to_input.append(Direction.UP_RIGHT)
            #     elif Key.left in valid_dirs:
            #         to_input.append(Direction.UP_LEFT)
            # elif Key.down in valid_dirs:
            #     if Key.left in valid_dirs:
            #         to_input.append(Direction.DOWN_LEFT)
            #     elif Key.right in valid_dirs:
            #         to_input.append(Direction.DOWN_RIGHT)
            self.key.input(to_input)
