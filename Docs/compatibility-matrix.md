# Compatibility Matrix: Old Python API → New Rust-backed API

## Overview

This document maps the original Python-only `Poke-Controller-Modified-Extension` API (SerialController/)
to the new Rust-backed architecture on the `refactor/rust-core` branch.

## 1. Module: `Commands.Keys` → `python/pokecon/keys.py` + Rust `pokecon-core::serial`

| Old API | Old Type | New API Location | Rust Equivalent | Status |
|---------|----------|-----------------|-----------------|--------|
| `Button` | `IntFlag` enum | `pokecon.keys.Button` | `pokecon_core::serial::keys::Button` (bitflags) | ⚠️ String stub, should be IntFlag |
| `Hat` | `IntEnum` enum | `pokecon.keys.Hat` | `pokecon_core::serial::keys::Hat` (enum) | ⚠️ String stub, should be IntEnum |
| `Stick` | `Enum` | **MISSING** | `pokecon_core::serial::keys::Stick` (enum) | ❌ Not exposed |
| `Tilt` | `Enum` | **MISSING** | `pokecon_core::serial::keys::Tilt` (enum) | ❌ Not exposed |
| `Direction` | Class with x,y,stick | **MISSING** | `pokecon_core::serial::keys::Direction` (struct) | ❌ Not exposed |
| `Touchscreen` | Class with x,y | **MISSING** | `pokecon_core::serial::keys::Touchscreen` (struct) | ❌ Not exposed |
| `SendFormat` | Builds serial frames | **MISSING** | `pokecon_core::serial::format::SendFormat` | ❌ Not exposed |
| `KeyPress` | Serial input handler | **MISSING** | `pokecon_core::serial::keypress::KeyPress` | ❌ Not exposed |
| `GamepadInput` | Type alias | **MISSING** | `pokecon_core::serial::keys::GamepadInput` (enum) | ❌ Not exposed |
| `Direction.{UP,DOWN,...}` | Predefined class vars | **MISSING** | `Direction::up(d)`, etc. (methods) | ❌ Not exposed |

## 2. Module: `Commands.PythonCommandBase` → `python/pokecon/commands.py` + Rust `pokecon-pybindings/src/python_cmd.rs`

| Old API | Description | New API Location | Status |
|---------|-------------|------------------|--------|
| `PythonCommand` | Base class for all user scripts | `pokecon.commands.PythonCommand` (metaclass) | ⚠️ Stub only |
| `PythonCommand.do()` | Abstract entrypoint | `PythonCommand.do()` | ✅ Defined as abstract |
| `PythonCommand.press()` | Press and release button(s) | **MISSING** | ❌ |
| `PythonCommand.pressRep()` | Repeated press | **MISSING** | ❌ |
| `PythonCommand.hold()` | Hold button down | **MISSING** | ❌ |
| `PythonCommand.holdEnd()` | Release held button | **MISSING** | ❌ |
| `PythonCommand.wait()` | Wait for duration | **MISSING** | ❌ |
| `PythonCommand.short_wait()` | Busy-wait for short duration | **MISSING** | ❌ |
| `PythonCommand.finish()` | Graceful stop | **MISSING** | ❌ |
| `PythonCommand.checkIfAlive()` | Check stop flag | **MISSING** | ❌ |
| `PythonCommand.start()` | Thread launch | **MISSING** | ❌ |
| `PythonCommand.end()` | Stop signal | **MISSING** | ❌ |
| `PythonCommand.sendStopRequest()` | Send stop request | **MISSING** | ❌ |
| `PythonCommand.do_safe()` | Orchestration wrapper | **MISSING** | ❌ |
| `PythonCommand.direct_serial()` | Raw serial send | **MISSING** | ❌ |
| `PythonCommand.reload_com_port()` | Reload COM port | **MISSING** | ❌ |
| `PythonCommand.LINE_text()` | LINE notify text | **MISSING** | ❌ |
| `PythonCommand.discord_text()` | Discord webhook text | **MISSING** | ❌ |
| `PythonCommand.show_var()` | Debug variable display | **MISSING** | ❌ |
| `ImageProcPythonCommand` | Base with camera access | `pokecon.commands.ImageProcPythonCommand` | ⚠️ Stub only |
| `ImageProcPythonCommand.isContainTemplate()` | Template matching | **MISSING** | ❌ |
| `ImageProcPythonCommand.isContainTemplate_max()` | Multi-template | **MISSING** | ❌ |
| `ImageProcPythonCommand.isContainTemplateGPU()` | GPU template matching | **MISSING** | ❌ |
| `ImageProcPythonCommand.isContainedImage()` | Image-to-image matching | **MISSING** | ❌ |
| `ImageProcPythonCommand.saveCapture()` | Save screenshot | **MISSING** | ❌ |
| `ImageProcPythonCommand.getCameraImage()` | Get camera frame | **MISSING** | ❌ |
| `ImageProcPythonCommand.openImage()` | Load image from file | **MISSING** | ❌ |
| `ImageProcPythonCommand.get_filespec()` | Resolve file path | **MISSING** | ❌ |
| `ImageProcPythonCommand.setTemplateDir()` | Set template directory | **MISSING** | ❌ |
| `ImageProcPythonCommand.displayRectangle()` | Draw rect on canvas | **MISSING** | ❌ |
| `ImageProcPythonCommand.displayText()` | Draw text on canvas | **MISSING** | ❌ |
| `ImageProcPythonCommand.popupImage()` | Show image popup | **MISSING** | ❌ |
| `ImageProcPythonCommand.LINE_image()` | LINE notify with image | **MISSING** | ❌ |
| `ImageProcPythonCommand.discord_image()` | Discord notify with image | **MISSING** | ❌ |
| `StopThread` | Exception for thread exit | **MISSING** | ❌ |
| `convertCv2Format()` | Crop format conversion | **MISSING** | ❌ |
| `generateRandomCharacter()` | Random string util | **MISSING** | ❌ |

## 3. Module: `Commands.CommandBase` → `python/pokecon/interfaces/base.py`

| Old API | New API Location | Status |
|---------|-----------------|--------|
| `Command.NAME` | `Command.NAME` | ✅ |
| `Command.TAGS` | **MISSING** | ❌ |
| `Command.print_t1/t2/t/ts/t1b/t2b/tb/tbs()` | **MISSING** | ❌ |
| `Command.dialogue()` | **MISSING** | ❌ |
| `Command.dialogue6widget()` | **MISSING** | ❌ |
| `Command.dialogue6widget_save_settings()` | **MISSING** | ❌ |
| `Command.dialogue6widget_select_settings()` | **MISSING** | ❌ |
| `Command.socket_*()` | **MISSING** | ❌ |
| `Command.mqtt_*()` | **MISSING** | ❌ |
| Various class vars (isPause, canvas, isGuide, etc.) | **MISSING** | ❌ |

## 4. Module: `Commands.Sender` → Rust `pokecon-core/src/serial/sender.rs`

| Old API | Rust Equivalent | Py binding | Status |
|---------|----------------|------------|--------|
| `Sender.__init__()` | `Sender::new()` | **MISSING** | ❌ |
| `Sender.openSerial()` | `Sender::open()` | **MISSING** | ❌ |
| `Sender.closeSerial()` | `Sender::close()` | **MISSING** | ❌ |
| `Sender.isOpened()` | `Sender::is_opened()` | **MISSING** | ❌ |
| `Sender.writeRow()` | `Sender::write_row()` | **MISSING** | ❌ |
| `Sender.writeList()` | `Sender::write_list()` | **MISSING** | ❌ |
| `Sender.writeRow_wo_perf_counter()` | `Sender::write_row_wo_counter()` | **MISSING** | ❌ |

## 5. Module: `ImageProcessing` → Rust `pokecon-core::cv`

| Old API | Rust Equivalent | Py binding | Status |
|---------|----------------|------------|--------|
| `ImageProcessing.isContainTemplate()` | **MISSING** in Rust | ❌ | ❌ |
| `ImageProcessing.isContainTemplate_max()` | **MISSING** in Rust | ❌ | ❌ |
| `ImageProcessing.saveImage()` | **MISSING** in Rust | ❌ | ❌ |
| `crop_image()` | Manual `crop()` in pybindings | `pokecon.image_proc.crop()` | ✅ |
| `getImage()` | **MISSING** in Rust | ❌ | ❌ |

## Summary Dashboard

| Category | Total | ✅ Complete | ⚠️ Partial/Stub | ❌ Missing |
|----------|-------|------------|------------------|------------|
| Keys (Button, Hat, Stick, etc.) | 10 | 0 | 2 | 8 |
| KeyPress (input, hold, etc.) | 8 | 0 | 0 | 8 |
| PythonCommand methods | 22 | 1 | 1 | 20 |
| ImageProcPythonCommand methods | 14 | 0 | 0 | 14 |
| CommandBase (print, dialog, socket, mqtt) | 25+ | 0 | 1 | 24+ |
| Sender (serial port) | 8 | 0 | 0 | 8 |
| ImageProcessing | 4 | 1 | 0 | 3 |
| **TOTAL** | **~91** | **2** | **4** | **~85** |

## Test Compatibility Status

The test file `tests/test_script_compatibility.py` currently imports from the OLD paths:
- `from Commands.Keys import Button, Direction, Hat, Stick, Touchscreen` 
- `from Commands.PythonCommandBase import PythonCommand, ImageProcPythonCommand, StopThread`

The import hack in `python/pokecon/__init__.py` only patches `Commands.PythonCommandBase`. It does NOT patch `Commands.Keys`.

**For tests to pass, both `Commands.Keys` and `Commands.PythonCommandBase` must be patched** with compatible wrapper classes.
