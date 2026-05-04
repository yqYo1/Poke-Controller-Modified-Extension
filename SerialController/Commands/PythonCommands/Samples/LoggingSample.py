#!/usr/bin/env python3
# -*- coding: utf-8 -*-

from logging import DEBUG, NullHandler, getLogger

from Commands.PythonCommandBase import ImageProcPythonCommand


# ログ出力のサンプル
class LoggingSample(ImageProcPythonCommand):
    NAME = "ログ出力のサンプル"

    def __init__(self, cam):
        super().__init__(cam)
        self._logger = getLogger(__name__)
        self._logger.addHandler(NullHandler())
        self._logger.setLevel(DEBUG)
        self._logger.propagate = True

    def do(self):
        self._logger.debug("DEBUG")
        self._logger.info("INFO")
        self._logger.warning("WARNING")
        self._logger.error("ERROR")
        self._logger.critical("CRITICAL")
