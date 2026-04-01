# 開発者向け 内部実装メモ（実装準拠）

本書は、このリポジトリの開発者向けです。  
ユーザー向け情報は `Docs/user-api-guide-ja.md`、外部I/O仕様は `Docs/external-io-spec-ja.md` を参照してください。

---

## 1. 全体構造

- エントリ: `SerialController/Window.py`
- 中心クラス: `PokeControllerApp`
- 主要サブシステム:
  - コマンドロード/実行（Python/MCU）
  - シリアル送信 (`Commands/Sender.py`)
  - 入力抽象 (`Commands/Keys.py`)
  - 画像入力 (`Camera.py`, `gui/assets.py`)
  - 外部連携 (`ExternalTools.py`, `DiscordNotify.py`, `LineNotify.py`)
  - 設定・プロファイル (`Settings.py`, `file_handler.py`)

---

## 2. 起動シーケンス

1. `Window.py` で profile 引数を受け取り
2. `FileHandler.PROFILE` と `Command` の静的属性を設定
3. `GuiSettings` 読み込み
4. Camera/Serial/Keyboard/Preview を初期化
5. `loadCommands()` で Python/Mcu コマンドをロード
6. `mainloop`

`root.after(...)` で Canvas 更新ループを回しています。

---

## 3. コマンドロード内部

`CommandLoader` の挙動:

- `load()`: 初回 import
- `reload()`: 追加モジュール import、既存 reload、削除モジュール un-import
- `NAME` を持つクラスだけ抽出
- `TAGS` にディレクトリタグ（`@...`）を自動付与
- `NAME` は `"{元NAME} ({ディレクトリパス})"` に変換

UI の filter 値は `TAGS` 集計結果から生成されています。

---

## 4. 実行モデル

### PythonCommand

- `start()` でスレッド起動し `do_safe()` 実行
- `do_safe()`:
  - 通知（開始/終了）
  - `do()` 呼び出し
  - 例外処理 / `StopThread` で正常停止
- 停止は `alive` フラグ＋`checkIfAlive()` で伝播
- `pause` は `pausedecorator` で制御

### McuCommand

- `start()` で `sync_name` をシリアル送信
- `end()` で `"end"` 送信

---

## 5. I/O と状態共有

- `CommandBase.Command` は GUI 関連の共通機能を保持
  - ログ出力 (`print_t*`)
  - ダイアログ (`dialogue*`)
  - socket/mqtt wrapper
- 静的フラグ:
  - `isPause`, `isGuide`, `isSimilarity`, 通知フラグ群, `stdout_destination`
- `Command.canvas` は `CaptureArea` を指し、画像認識系から描画可能

---

## 6. シリアル形式実装ポイント

- 実際の送信は `KeyPress.input()/inputEnd()` が担当
- フォーマット分岐:
  - Default → `Sender.writeRow(convert2str())`
  - Qingpi → `Sender.writeList(convert2list())`
  - 3DS Controller → `Sender.writeList(convert2list2())`
- `set_serial_data_format()` でフォーマット変更時に
  - `KeyPress.serial_data_format_name` 更新
  - 右マウスモード更新
  - ボーレート強制変更（3DS=115200, それ以外=9600）

---

## 7. カメラ・Canvas 内部

- `Camera` は別スレッドでフレーム更新し、`readFrame()` は最新コピーを返す
- `CaptureArea`:
  - 表示サイズと元解像度変換を管理
  - マウスで L/R スティックや Qingpi タッチ入力を生成
  - `RightMouseMode` が `Qingpi` のとき右入力はタッチ座標へ変換

---

## 8. プロファイル切替の内部

- `FileHandler.PROFILE` を基準に `profiles/<name>/...` を参照
- `Window.py` 起動時、non-default profile では:
  - Discord/Socket/MQTT/Keyboard/KeyConfig の参照先を profile 用に差し替え
- `GuiSettings` は `FileHandler.get_configs_path()` 経由

---

## 9. テスト/検証メモ

UI 完全起動は行わず、以下を実行して挙動確認:

- `KeyPress` をダミー送信先で駆動し、Default/Qingpi/3DS の送信データを確認
- `convertCv2Format` を AST 抽出実行して変換結果を確認

---

## 10. バグらしき挙動（開発者観点）

1. `change_buttons_position()` が 1/3 を保持できない（常に2化）
2. 範囲キャプチャのY座標変換で `ratio_y` 未使用
3. LINE トークンの profile 切替が未対応（default固定）
4. `ControllerBase` の右スティック比較式が不自然（同式重複）
5. Linux のシリアル列挙で COM 前提 regex を使用
