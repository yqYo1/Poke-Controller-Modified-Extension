#!/bin/env bash

SERIALDIR=$(cd $(dirname $0)/SerialController;pwd)
cd $SERIALDIR
. ../.venv/bin/activate
./Window.py
