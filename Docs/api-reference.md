# API リファレンス

Poke-Controller Modified Extension の全 API リファレンスです。

## 目次

- [PythonCommand API](#pythoncommand-api)
- [ImageProcPythonCommand API](#imageprocpythoncommand-api)
- [KeyPress / Keys API](#keypress--keys-api)
- [HTTP API](#http-api)
- [WebSocket API](#websocket-api)
- [PyO3 Bindings API](#pyo3-bindings-api)

---

## PythonCommand API

### 入力操作

#### `press(buttons, duration=0.1, wait=0.1)`

ボタンを押下 → duration秒待機 → 解放 → wait秒待機。

| パラメータ | 型 | デフォルト | 説明 |
|-----------|-----|----------|------|
| `buttons` | `GamepadInput` or `list` | 必須 | 押下する入力 |
| `duration` | `float` | 0.1 | 押下継続秒数 |
| `wait` | `float` | 0.1 | 解放後待機秒数 |

```python
self.press(Button.A)
self.press([Button.A, Button.B], duration=0.2)
self.press(Hat.UP)
self.press(Direction(Stick.LEFT, 90))
```

#### `pressRep(buttons, repeat, duration=0.1, interval=0.1, wait=0.1)`

press を repeat 回繰り返し。

| パラメータ | 型 | デフォルト | 説明 |
|-----------|-----|----------|------|
| `repeat` | `int` | 必須 | 繰り返し回数（0で待機のみ） |
| `interval` | `float` | 0.1 | 連打間隔 |

```python
self.pressRep(Button.A, 5, interval=0.05)
```

#### `hold(buttons, wait=0.1)`

ボタンを押しっぱなし状態にする。

#### `holdEnd(buttons)`

hold 状態のボタンを解放。

#### `wait(sec)`

待機。sec > 0.1 は sleep、sec <= 0.1 はビジーウェイト。

#### `short_wait(sec)`

常にビジーウェイトで待機（短時間精度重視）。

#### `finish()`

コマンドを終了。停止処理へ移行。

### 通知

#### `LINE_text(text, token=None)`

LINE通知を送信。

#### `discord_text(text, webhook_url=None)`

Discord通知を送信。

#### `notification(title, message)`

Windows通知を送信。

### 通信

#### `socket_connect(ip, port)`

Socket接続。

#### `socket_disconnect()`

Socket切断。

#### `mqtt_connect(broker, port, topic)`

MQTT接続。

#### `mqtt_disconnect()`

MQTT切断。

### ログ

#### `print(message)`

通常ログ出力。

#### `print_info(message)`

情報ログ出力。

#### `print_warning(message)`

警告ログ出力。

#### `print_error(message)`

エラーログ出力。

### ダイアログ

#### `dialogue(text, options=None)`

ダイアログを表示。options指定で選択式。

---

## ImageProcPythonCommand API

PythonCommand の全機能に加え、画像認識機能が利用可能。

### 画像認識

#### `isContainTemplate(template_path, show_position=False, search_range=None)`

テンプレート画像が画面内に存在するか判定。

| パラメータ | 型 | デフォルト | 説明 |
|-----------|-----|----------|------|
| `template_path` | `str` | 必須 | テンプレート画像パス |
| `show_position` | `bool` | False | 認識位置をGUIに表示 |
| `search_range` | `tuple` or `None` | None | 検索範囲 (x, y, w, h) |

```python
if self.isContainTemplate("target.png", search_range=(100, 100, 200, 150)):
    self.press(Button.A)
```

#### `isContainedImage(large_path, small_path)`

大きな画像が小さな画像を含むか判定。

#### `save_capture(filename, crop=None)`

現在のフレームを保存。

| パラメータ | 型 | デフォルト | 説明 |
|-----------|-----|----------|------|
| `filename` | `str` | 必須 | 保存ファイル名 |
| `crop` | `tuple` or `None` | None | 切り出し範囲 (x, y, w, h) |

#### `readFrame()`

現在のフレームを OpenCV 形式 (ndarray) で取得。

#### `convertCv2Format(frame)`

フレームのフォーマットを変換。

---

## KeyPress / Keys API

### Button

```python
from Commands.Keys import Button

Button.Y      # 0x0001
Button.B      # 0x0002
Button.A      # 0x0004
Button.X      # 0x0008
Button.L      # 0x0010
Button.R      # 0x0020
Button.ZL     # 0x0040
Button.ZR     # 0x0080
Button.MINUS  # 0x0100
Button.PLUS   # 0x0200
Button.LCLICK # 0x0400
Button.RCLICK # 0x0800
Button.HOME   # 0x1000
Button.CAPTURE # 0x2000
```

### Hat (D-Pad)

```python
from Commands.Keys import Hat

Hat.TOP       # 0x00
Hat.TOP_RIGHT # 0x01
Hat.RIGHT     # 0x02
Hat.BOTTOM_RIGHT # 0x03
Hat.BOTTOM    # 0x04
Hat.BOTTOM_LEFT # 0x05
Hat.LEFT      # 0x06
Hat.TOP_LEFT  # 0x07
Hat.CENTER    # 0x08
```

### Direction (アナログスティック)

```python
from Commands.Keys import Direction, Stick

# 角度指定（度）
Direction(Stick.LEFT, 0)    # 上
Direction(Stick.LEFT, 90)   # 右
Direction(Stick.LEFT, 180)  # 下
Direction(Stick.LEFT, 270)  # 左

# 座標指定（0-255）
Direction(Stick.LEFT, 128, 0)   # x=128, y=0
Direction(Stick.RIGHT, 255, 255) # x=255, y=255
```

### Touchscreen

```python
from Commands.Keys import Touchscreen

Touchscreen(100, 200)  # x=100, y=200
```

---

## HTTP API

### 共通仕様

- ベースURL: `http://127.0.0.1:8020`
- レスポンス形式: JSON
- エラーレスポンス: `{ "error": "message" }`

### Status

#### `GET /api/status`

ヘルスチェック。

**Response:**
```json
{
  "status": "ok",
  "version": "0.1.0",
  "mode": "shared"
}
```

### Serial

#### `GET /api/serial/ports`

利用可能なシリアルポート一覧。

**Response:**
```json
{
  "ports": ["/dev/ttyUSB0", "/dev/ttyACM0"]
}
```

#### `POST /api/serial/open`

シリアルポートを開く。

**Request:**
```json
{
  "port_num": 0,
  "port_name": "/dev/ttyUSB0",
  "baudrate": 9600
}
```

**Response:**
```json
{
  "success": true,
  "port": "/dev/ttyUSB0"
}
```

#### `POST /api/serial/close`

シリアルポートを閉じる。

#### `POST /api/serial/write`

データを送信。

**Request:**
```json
{
  "data": "btn_a\r\n"
}
```

#### `GET /api/serial/status`

接続状態を取得。

**Response:**
```json
{
  "is_open": true,
  "port": "/dev/ttyUSB0"
}
```

### Camera

#### `GET /api/camera/status`

カメラ状態を取得。

#### `POST /api/camera/open`

カメラを開く。

**Request:**
```json
{
  "device_index": 0,
  "width": 1280,
  "height": 720
}
```

#### `POST /api/camera/close`

カメラを閉じる。

#### `GET /api/camera/frame`

現在のフレームを取得（base64 JPEG）。

**Response:**
```json
{
  "image": "/9j/4AAQSkZJRgABAQ...",
  "width": 1280,
  "height": 720
}
```

#### `POST /api/camera/capture`

フレームをファイルに保存。

**Request:**
```json
{
  "filename": "screenshot.jpg"
}
```

### Input

#### `POST /api/input/press`

ボタンを押下。

**Request:**
```json
{
  "buttons": "A",
  "duration": 0.1,
  "wait": 0.1
}
```

#### `POST /api/input/hold`

ボタンをhold。

**Request:**
```json
{
  "buttons": "A",
  "duration": 1.0
}
```

#### `POST /api/input/release`

全ボタンを解放。

#### `POST /api/input/stick`

スティックを操作。

**Request:**
```json
{
  "stick": "LSTICK",
  "x": 128,
  "y": 0,
  "duration": 0.5
}
```

#### `POST /api/input/touch`

タッチ操作。

**Request:**
```json
{
  "x": 100,
  "y": 200,
  "duration": 0.1
}
```

### Commands

#### `GET /api/commands`

スクリプト一覧を取得。

**Response:**
```json
{
  "commands": [
    {
      "name": "MashA",
      "path": "SerialController/Commands/PythonCommands/MashA.py",
      "description": "A連打"
    }
  ]
}
```

#### `POST /api/commands/load`

スクリプトをロード。

**Request:**
```json
{
  "name": "MashA"
}
```

#### `POST /api/commands/start`

スクリプトを開始。

#### `POST /api/commands/stop`

実行中のスクリプトを停止。

#### `GET /api/commands/active`

実行中のスクリプト情報。

**Response:**
```json
{
  "active": true,
  "name": "MashA"
}
```

---

## WebSocket API

### 接続

```javascript
const ws = new WebSocket('ws://127.0.0.1:8020/ws');
```

### 受信イベント

#### `camera.frame`

カメラフレーム更新。

```json
{
  "type": "camera.frame",
  "payload": {
    "image": "/9j/4AAQSkZJRgABAQ...",
    "timestamp": "2026-01-01T00:00:00Z"
  }
}
```

#### `command.start`

スクリプト開始。

```json
{
  "type": "command.start",
  "payload": {
    "name": "MashA",
    "timestamp": "2026-01-01T00:00:00Z"
  }
}
```

#### `command.stop`

スクリプト停止。

```json
{
  "type": "command.stop",
  "payload": {
    "timestamp": "2026-01-01T00:00:00Z"
  }
}
```

#### `command.error`

エラー発生。

```json
{
  "type": "command.error",
  "payload": {
    "name": "MashA",
    "message": "Serial port not open",
    "timestamp": "2026-01-01T00:00:00Z"
  }
}
```

#### `serial.data`

シリアルデータ受信。

```json
{
  "type": "serial.data",
  "payload": {
    "data": "btn_a",
    "timestamp": "2026-01-01T00:00:00Z"
  }
}
```

---

## PyO3 Bindings API

### Python から Rust モジュールを利用

```python
from pokecon import keys, command, image_proc, events

# keys モジュール
from pokecon.keys import PyButton, PyDirection
btn = PyButton.A | PyButton.B

# command モジュール
from pokecon.command import PythonCommand
cmd = PythonCommand("test")
cmd.press("A", 0.1, 0.1)
cmd.wait(0.5)

# image_proc モジュール
from pokecon.image_proc import crop, grayscale

# events モジュール
from pokecon.events import EventBus
```

### pokecon.keys

| クラス/関数 | 説明 |
|------------|------|
| `PyButton` | ボタンビットマスク（A, B, X, Y, L, R, ZL, ZR...） |
| `PyDirection` | 方向入力（角度または座標指定） |
| `PyHat` | D-Pad入力 |
| `PyStick` | スティック種別（LEFT, RIGHT） |
| `PyTouchscreen` | タッチ座標 |
| `convert_button()` | ボタン変換 |
| `get_direction()` | 方向取得 |

### pokecon.command

| クラス/メソッド | 説明 |
|----------------|------|
| `PythonCommand` | コマンドクラス |
| `name` | コマンド名 |
| `press(buttons, duration, wait)` | ボタン押下 |
| `hold(buttons, duration)` | ボタンhold |
| `hold_end(duration)` | hold解除 |
| `wait(wait_time)` | 待機 |
| `short_wait()` | 短時間待機 |
| `check_if_alive()` | 生存確認 |
| `finish()` | 終了 |
| `line_text(text)` | LINE通知 |
| `discord_text(text)` | Discord通知 |
| `register_callback(event, callback)` | コールバック登録 |
| `trigger(event, kwargs)` | コールバック実行 |

### pokecon.image_proc

| 関数 | 説明 |
|------|------|
| `crop(image, x, y, w, h)` | 画像切り出し |
| `grayscale(image)` | グレースケール変換 |

### pokecon.events

| クラス/メソッド | 説明 |
|----------------|------|
| `EventBus` | イベントバス |
| `subscribe(topic)` | 購読 |
| `publish(topic, event)` | 発行 |
