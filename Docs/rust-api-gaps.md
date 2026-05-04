# Rust API Gaps — What Needs Filling in `rust/pokecon-pybindings/`

## Current State

The `rust/pokecon-pybindings/` crate exposes 4 submodules with minimal surface area:

| Submodule | Exported Items | Status |
|-----------|---------------|--------|
| `pokecon.keys` | `convert_button(u16)` → `list[str]`, `get_direction(u8)` → `str` | ❌ Barely started |
| `pokecon.events` | `EventBus` class with `emit()` | ✅ Basic |
| `pokecon.image_proc` | `crop()`, `grayscale()` | ❌ Skeletal |
| `pokecon.command` | `PythonCommand(name)` with `register_callback()` / `trigger()` | ❌ Event-only, no real command methods |

## Priority 1: Full `pokecon.keys` Module

### Type classes (must mirror Python API exactly)

- **`Button`** (IntFlag PyO3 class with all 14 members + aliases)
- **`Hat`** (IntEnum with 9 positions)
- **`Stick`** (Enum with Left/Right)
- **`Tilt`** (Enum with 8 directions)
- **`Direction`** (PyO3 class with x, y, stick, predefined class variables)
- **`Touchscreen`** (PyO3 class with x, y)
- **`SendFormat`** (PyO3 class wrapping `pokecon_serial::format::SendFormat`)
- **`KeyPress`** (PyO3 class wrapping `pokecon_serial::keypress::KeyPress`)

### Functions

- `Button.convert(…)` — match Python `conversion_default_button` / `conversion_3ds_controller_button`

## Priority 2: Full `pokecon.sender` Module

Expose the Rust `Sender` struct so Python users don't need `pyserial`.

- `Sender.open(port_num, port_name, baudrate)` → `bool`
- `Sender.close()` → `None`
- `Sender.is_opened()` → `bool`
- `Sender.write_row(row)` → `None`
- `Sender.write_list(values)` → `None`
- `Sender.write_row_wo_counter(row)` → `None`

## Priority 3: Full `pokecon.command` Module

Expose `PythonCommand` with the same lifecycle methods as the original:

- `PythonCommand.__init__()`
- `PythonCommand.do()` (abstract)
- `PythonCommand.press(buttons, duration, wait)` → async wrapper
- `PythonCommand.hold(buttons, wait)` → async wrapper
- `PythonCommand.holdEnd(buttons)` → async wrapper
- `PythonCommand.wait(seconds)` → async sleep
- `PythonCommand.finish()` → call keys.end()
- `PythonCommand.checkIfAlive()` → check stop flag

The current `PythonCommand` in Rust is event-driven (register_callback/trigger). The
Python wrapper at `python/pokecon/commands.py` provides the imperative do()-based
interface that user scripts expect. The Rust class should support BOTH patterns.

## Priority 4: Template Matching in `pokecon.image_proc`

- `ImageProcessing` class with `isContainTemplate(src, template, threshold)` → `(bool, x, y, w, h, score)`
- `isContainTemplate_max(src, templates, threshold)` → `(idx, scores, judges)`
- `saveImage(img, path)` → `None`
- `getImage(path, mode)` → `ndarray`
- `crop_image(img, crop)` → `ndarray`

## Priority 5: Notification in `pokecon.notify`

- `Discord.send_message(content, image, webhook)` → `None`
- `Line.send_message(text, token)` → `None`

## Priority 6: Network in `pokecon.net`

- `SocketCommunications.connect()`, `send()`, `receive()`
- `MQTTCommunications.connect()`, `publish()`, `subscribe()`

These are all blocked until the PyO3 bindings expose them.
