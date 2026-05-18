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
- エラーレスポンス: `{ "status": "error", "message": "..." }`
- ステータス取得成功時のレスポンスは原則 `{ "status": "ok", ... }`

### Core

#### `GET /`

ルートアクセス。`/ui/` へ一時リダイレクト。

**Response:** `302 Found` → `Location: /ui/`

---

#### `GET /mobile`

Mobile UI プレースホルダ（未実装）。

**Response:** `501 Not Implemented`

---

#### `GET /api/status`

ヘルスチェック。サーバーの稼働状態とバージョンを返す。

**Response:**
```json
{
  "status": "ok",
  "version": "0.1.0",
  "mode": "shared"
}
```

---

#### `GET /api/greet`

挨拶メッセージを返す。クエリパラメータ `?name=` で名前を指定可能（デフォルト: `Trainer`）。

**Query:**
| パラメータ | 型 | デフォルト | 説明 |
|-----------|-----|----------|------|
| `name` | `string` | `"Trainer"` | 挨拶対象の名前 |

**Response:**
```json
{
  "message": "Hello, Trainer! Welcome to Poke-Controller."
}
```

---

#### `GET /api/openapi.json`

OpenAPI 3.0 スキーマを JSON 形式で返す。

**Response:** OpenAPI specification (JSON)

---

### Controller

#### `GET /api/controller/type`

現在のコントローラー種別を取得。

**Response:**
```json
{
  "status": "ok",
  "gamepad_type": "ProController"
}
```

#### `POST /api/controller/type`

コントローラー種別を設定。

**Request:**
```json
{
  "gamepad_type": "ProController"
}
```

| フィールド | 型 | 説明 |
|-----------|-----|------|
| `gamepad_type` | `string` | `"ProController"` または `"Xinput"` |

**Response:**
```json
{
  "status": "ok",
  "gamepad_type": "ProController"
}
```

---

#### `GET /api/controller/keyboard`

キーボード入力の有効/無効状態を取得。

**Response:**
```json
{
  "status": "ok",
  "keyboard_enabled": false
}
```

#### `POST /api/controller/keyboard`

キーボード入力を有効化/無効化。

**Request:**
```json
{
  "enabled": true
}
```

**Response:**
```json
{
  "status": "ok",
  "keyboard_enabled": true
}
```

---

#### `GET /api/controller/mouse_stick`

マウススティック制御の設定を取得。

**Response:**
```json
{
  "status": "ok",
  "left_enabled": false,
  "right_enabled": false,
  "sensitivity": 1.0
}
```

#### `POST /api/controller/mouse_stick`

マウススティック制御を有効化/無効化。

**Request:**
```json
{
  "stick": "left",
  "enabled": true,
  "sensitivity": 1.5
}
```

| フィールド | 型 | 説明 |
|-----------|-----|------|
| `stick` | `string` | `"left"` または `"right"` |
| `enabled` | `bool` | 有効/無効 |
| `sensitivity` | `float` | 感度倍率（デフォルト: 1.0） |

**Response:**
```json
{
  "status": "ok",
  "stick": "left",
  "enabled": true,
  "sensitivity": 1.5
}
```

---

### Camera

#### `GET /api/cameras`

利用可能なカメラデバイス一覧を取得。

**Response:**
```json
{
  "status": "ok",
  "devices": [
    { "index": 0, "name": "Integrated Camera" },
    { "index": 1, "name": "USB Camera" }
  ]
}
```

---

#### `GET /api/camera/status`

カメラの接続状態と現在の設定を取得。

**Response（接続中）:**
```json
{
  "status": "ok",
  "is_open": true,
  "device_index": 0,
  "width": 1280,
  "height": 720,
  "fps": 30,
  "flip": "none"
}
```

**Response（未接続）:**
```json
{
  "status": "ok",
  "is_open": false
}
```

---

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

| フィールド | 型 | 説明 |
|-----------|-----|------|
| `device_index` | `int` | カメラデバイスインデックス |
| `width` | `int` | 解像度幅（オプション） |
| `height` | `int` | 解像度高さ（オプション） |

---

#### `POST /api/camera/close`

カメラを閉じる。

---

#### `GET /api/camera/frame`

現在のフレームを base64 JPEG で取得。

**Response:**
```json
{
  "image": "/9j/4AAQSkZJRgABAQ...",
  "width": 1280,
  "height": 720,
  "format": "jpeg"
}
```

---

#### `POST /api/camera/capture`

現在のフレームをファイルに保存。

**Request:**
```json
{
  "filename": "screenshot.jpg"
}
```

**Response:**
```json
{
  "status": "ok",
  "path": "/path/to/screenshot.jpg"
}
```

---

#### `POST /api/camera/config`

カメラの設定（解像度、FPS、フリップ）を更新。

**Request:**
```json
{
  "width": 1920,
  "height": 1080,
  "fps": 60,
  "flip": "horizontal"
}
```

| フィールド | 型 | 説明 |
|-----------|-----|------|
| `width` | `int` | 幅（1〜4096、オプション） |
| `height` | `int` | 高さ（1〜4096、オプション） |
| `fps` | `int` | FPS（1〜120、オプション） |
| `flip` | `string` | `"none"`, `"horizontal"`, `"vertical"`, `"both"`（オプション） |

**Response:**
```json
{
  "status": "ok",
  "device_index": 0,
  "width": 1920,
  "height": 1080,
  "fps": 60,
  "flip": "horizontal"
}
```

---

#### `GET /camera/stream`

MJPEG ストリームを配信（マルチパートHTTPレスポンス）。

**Response:** `Content-Type: multipart/x-mixed-replace; boundary=frame`

---

### Input

#### `POST /api/input/press`

ボタンを押下 → duration ミリ秒待機 → 解放 → wait ミリ秒待機。

**Request:**
```json
{
  "buttons": ["A"],
  "duration": 100,
  "wait": 100
}
```

| フィールド | 型 | デフォルト | 説明 |
|-----------|-----|----------|------|
| `buttons` | `string[]` | 必須 | ボタン名の配列（`"A"`, `"B"`, `"X"`, `"Y"`, `"L"`, `"R"`, `"ZL"`, `"ZR"`, `"MINUS"`, `"PLUS"`, `"LCLICK"`, `"RCLICK"`, `"HOME"`, `"CAPTURE"`） |
| `duration` | `int` | `50` | 押下継続時間（ミリ秒） |
| `wait` | `int` | `0` | 解放後の待機時間（ミリ秒） |

**Response:**
```json
{
  "status": "ok",
  "message": "Buttons pressed: A"
}
```

---

#### `POST /api/input/hold`

ボタンを押しっぱなしにする。`duration` を指定しない場合は `POST /api/input/release` で解放するまで継続。

**Request:**
```json
{
  "buttons": ["A"],
  "duration": 1000
}
```

| フィールド | 型 | デフォルト | 説明 |
|-----------|-----|----------|------|
| `buttons` | `string[]` | 必須 | ボタン名の配列 |
| `duration` | `int` | `0` | ホールド時間（ミリ秒、0=解放まで継続） |

---

#### `POST /api/input/release`

全ボタンを解放する（hold 状態の解除）。

**Response:**
```json
{
  "status": "ok",
  "message": "All buttons released"
}
```

---

#### `POST /api/input/stick`

アナログスティックを操作。

**Request:**
```json
{
  "stick": "left",
  "x": 128,
  "y": 0,
  "duration": 500
}
```

| フィールド | 型 | 説明 |
|-----------|-----|------|
| `stick` | `string` | `"left"` または `"right"` |
| `x` | `int` | X座標（0〜255、128=中央） |
| `y` | `int` | Y座標（0〜255、128=中央） |
| `duration` | `int` | 操作継続時間（ミリ秒、デフォルト: 0） |

---

#### `POST /api/input/touch`

タッチスクリーン操作。

**Request:**
```json
{
  "x": 100,
  "y": 200,
  "duration": 100
}
```

| フィールド | 型 | 説明 |
|-----------|-----|------|
| `x` | `int` | X座標 |
| `y` | `int` | Y座標 |
| `duration` | `int` | タップ継続時間（ミリ秒、デフォルト: 0） |

---

### Serial

#### `GET /api/serial/ports`

利用可能なシリアルポート一覧を取得。

**Response:**
```json
{
  "ports": ["/dev/ttyUSB0", "/dev/ttyACM0"]
}
```

---

#### `POST /api/serial/open`

シリアルポートを開く。

**Request:**
```json
{
  "port_name": "/dev/ttyUSB0",
  "baudrate": 9600
}
```

| フィールド | 型 | 説明 |
|-----------|-----|------|
| `port_name` | `string` | ポートのパス（例: `"/dev/ttyUSB0"`） |
| `baudrate` | `int` | ボーレート（デフォルト: 9600） |

**Response:**
```json
{
  "status": "ok",
  "port": "/dev/ttyUSB0"
}
```

---

#### `POST /api/serial/close`

シリアルポートを閉じる。

**Response:**
```json
{
  "status": "ok",
  "message": "Serial port closed"
}
```

---

#### `POST /api/serial/write`

シリアルポートにデータを送信。

**Request:**
```json
{
  "data": "btn_a\r\n"
}
```

**Response:**
```json
{
  "status": "ok",
  "message": "Data written"
}
```

---

#### `POST /api/serial/config`

シリアルポートの設定（ボーレート、データ形式）を更新。

**Request:**
```json
{
  "baudrate": 115200,
  "data_format": "default"
}
```

| フィールド | 型 | 説明 |
|-----------|-----|------|
| `baudrate` | `int` | ボーレート（オプション） |
| `data_format` | `string` | データ形式（オプション） |

**Response:**
```json
{
  "status": "ok",
  "message": "Serial config updated"
}
```

---

#### `GET /api/serial/status`

シリアルポートの接続状態を取得。

**Response:**
```json
{
  "status": "ok",
  "is_open": true,
  "port": "/dev/ttyUSB0"
}
```

---

### Commands

#### `GET /api/commands`

利用可能なスクリプト一覧を取得。

**Response:**
```json
{
  "commands": [
    {
      "name": "MashA",
      "path": "scripts/PythonCommands/MashA.py",
      "description": "A連打"
    }
  ]
}
```

---

#### `POST /api/commands/load`

スクリプトをロード（名前指定）。

**Request:**
```json
{
  "name": "MashA"
}
```

**Response:**
```json
{
  "status": "ok",
  "name": "MashA"
}
```

---

#### `POST /api/commands/start`

ロード済みのスクリプトを開始。

**Response:**
```json
{
  "status": "ok",
  "message": "Command started"
}
```

---

#### `POST /api/commands/stop`

実行中のスクリプトを停止。

**Response:**
```json
{
  "status": "ok",
  "message": "Command stopped"
}
```

---

#### `GET /api/commands/active`

現在実行中のスクリプト情報を取得。

**Response（実行中）:**
```json
{
  "active": true,
  "name": "MashA",
  "path": "scripts/PythonCommands/MashA.py",
  "description": "A連打"
}
```

**Response（未実行）:**
```json
{
  "active": false
}
```

---

#### `POST /api/commands/filter`

スクリプト一覧にフィルターを適用して結果を返す。

**Request:**
```json
{
  "filter": "mash"
}
```

**Response:**
```json
{
  "status": "ok",
  "filter": "mash",
  "commands": [
    {
      "name": "MashA",
      "path": "scripts/PythonCommands/MashA.py",
      "description": "A連打"
    }
  ]
}
```

---

#### `POST /api/commands/reload`

スクリプトディレクトリを再スキャンしてスクリプト一覧をリロード。

**Response:**
```json
{
  "status": "ok",
  "message": "Scanned 5 commands",
  "commands": [
    { "name": "MashA", "path": "scripts/PythonCommands/MashA.py", "description": "A連打" }
  ]
}
```

---

### Profile

#### `GET /api/profile`

利用可能なプロファイル一覧を取得。

**Response:**
```json
{
  "status": "ok",
  "profiles": [
    {
      "name": "default",
      "description": "Default profile",
      "active": true
    },
    {
      "name": "pogo",
      "description": "Pokémon GO profile",
      "active": false
    }
  ],
  "active": "default"
}
```

#### `POST /api/profile`

プロファイルをアクティブ化。

**Request:**
```json
{
  "name": "pogo"
}
```

**Response:**
```json
{
  "status": "ok",
  "message": "Profile pogo activated",
  "active": "pogo"
}
```

---

### Notifications

#### `GET /api/notifications/config`

現在の通知設定を取得。

**Response:**
```json
{
  "status": "ok",
  "windows_enabled": true,
  "discord_enabled": false,
  "discord_webhook_url": ""
}
```

#### `POST /api/notifications/config`

通知設定を更新。

**Request:**
```json
{
  "windows_enabled": true,
  "discord_enabled": true,
  "discord_webhook_url": "https://discord.com/api/webhooks/..."
}
```

| フィールド | 型 | 説明 |
|-----------|-----|------|
| `windows_enabled` | `bool` | Windows通知の有効/無効（オプション） |
| `discord_enabled` | `bool` | Discord通知の有効/無効（オプション） |
| `discord_webhook_url` | `string` | Discord Webhook URL（オプション、SSRF対策済み） |

**Response:**
```json
{
  "status": "ok",
  "message": "Notification config updated"
}
```

---

#### `POST /api/notifications/send`

現在の設定を使用してテスト通知を送信。

**Request:**
```json
{
  "message": "Hello from Poke-Controller!",
  "title": "Test Notification"
}
```

| フィールド | 型 | 説明 |
|-----------|-----|------|
| `message` | `string` | 通知メッセージ（必須） |
| `title` | `string` | 通知タイトル（オプション、デフォルト: "Poke-Controller"） |

**Response:**
```json
{
  "status": "ok",
  "results": [
    { "channel": "windows", "status": "sent" },
    { "channel": "discord", "status": "error", "error": "..." }
  ]
}
```

### Static Files

#### `GET /ui/*`

SvelteKit でビルドされた静的ファイルを配信。`/ui/` 以下の任意のパスにアクセスすると対応するファイルが返される。

**例:**
- `GET /ui/` → `index.html`
- `GET /ui/_app/version.json` → バージョンファイル

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
