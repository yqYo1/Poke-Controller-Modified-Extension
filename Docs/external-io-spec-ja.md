# 外部 I/O 仕様（シリアル通信・カメラ・外部入力）

本書は、外部連携ソフト開発者向けの I/O 仕様書です。  
ユーザー向け API は `Docs/user-api-guide-ja.md` を参照してください。

---

## 1. 概要

本ソフトはシリアル送信形式を3種類持ちます。

- `Default`（ASCII 文字列 + CRLF）
- `Qingpi`（バイナリ11バイト）
- `3DS Controller`（バイナリ6バイト）

UI 設定:

- `Serial > Data Format` で切替
- 3DS 選択時ボーレートは 115200、それ以外は 9600 に強制変更

---

## 2. 実測（socat による送信確認）

`socat -d -d pty,raw,echo=0 pty,raw,echo=0` で仮想シリアル対を作り、  
`Sender.openSerial(..., portName=<PTY>, baudrate=9600)` で送信側を接続、対向PTYで受信を確認。

### 2.1 Sender API 生データ

- `writeRow("0x0010 8")` → `30 78 30 30 31 30 20 38 0d 0a`
  - 文字列 `"0x0010 8\r\n"` がそのまま送信
- `writeRow_wo_perf_counter("end")` → `65 6e 64 0d 0a`
  - 文字列 `"end\r\n"` がそのまま送信
- `writeList([0xAB, 0x01, 0x02])` → `ab 01 02`
  - バイト列がそのまま送信

=> pyserial 側でプロトコル内容の暗黙変換は確認されませんでした（`writeRow` は CRLF 付与のみ）。

### 2.2 KeyPress 実測

- Default:
  - `input(Button.A)` → `0x0010 8\r\n`
  - `inputEnd(Button.A)` → `0x0000 8\r\n`
  - `input(Direction(Stick.LEFT, 90))` → `0x0002 8 80 0\r\n`
- Qingpi:
  - `input(Touchscreen(160,120))` → `ab00000880808080a00078`
- 3DS:
  - `input([Button.A, Hat.LEFT])` → `a11100a28080`

---

## 3. Default フォーマット仕様

送信文字列:

`0xBBBB H [LX LY] [RX RY]\r\n`

- `BBBB`: 16bit 相当のビット群（実装上は stick 更新フラグを含む）
- `H`: Hat 値（0..8）
- `LX LY`: 左スティック（変更時のみ）
- `RX RY`: 右スティック（変更時のみ）

### 3.1 ビット割り当て（実測）

| 入力 | 送信例 | 立つビット |
|---|---|---|
| Y | `0x0004 8` | 2 |
| B | `0x0008 8` | 3 |
| A | `0x0010 8` | 4 |
| X | `0x0020 8` | 5 |
| L | `0x0040 8` | 6 |
| R | `0x0080 8` | 7 |
| ZL | `0x0100 8` | 8 |
| ZR | `0x0200 8` | 9 |
| MINUS | `0x0400 8` | 10 |
| PLUS | `0x0800 8` | 11 |
| LCLICK | `0x1000 8` | 12 |
| RCLICK | `0x2000 8` | 13 |
| HOME | `0x4000 8` | 14 |
| CAPTURE | `0x8000 8` | 15 |

補足:

- bit0: 右スティック変更フラグ
- bit1: 左スティック変更フラグ

### 3.2 Hat 値対応（実測）

| Hat | 値 |
|---|---|
| TOP | 0 |
| TOP_RIGHT | 1 |
| RIGHT | 2 |
| BTM_RIGHT | 3 |
| BTM | 4 |
| BTM_LEFT | 5 |
| LEFT | 6 |
| TOP_LEFT | 7 |
| CENTER | 8 |

---

## 4. Qingpi フォーマット

11バイト固定:

`[0xAB, btn_lo, btn_hi, hat, lx, ly, 0x80, 0x80, sx_lo, sx_hi, sy]`

- タッチ有効時に `sx/sy` が更新
- `Touchscreen(160,120)` 実測:
  - `ab 00 00 08 80 80 80 80 a0 00 78`

---

## 5. 3DS Controller フォーマット

6バイト固定:

`[0xA1, byte1, byte2, 0xA2, lx, ly]`

- `byte1`: 下位4bitに Hat, 上位4bitに一部ボタン
- `byte2`: 残りボタン
- 実装で 3DS 用ボタン変換テーブルを使用

---

## 6. MCUコマンド通信（McuCommands）

MCUコマンドは `SerialController/Commands/McuCommands/` 配下のクラスで、  
すべて `McuCommand`（`Commands/McuCommandBase.py`）を継承します。

### 6.1 通信シーケンス（実装準拠）

- 開始（Start）時:
  - `McuCommand.start()` が `Sender.writeRow(sync_name)` を実行
  - 実送信は `"<sync_name>\\r\\n"`（UTF-8文字列）
- 停止（Stop）時:
  - `McuCommand.end()` が `Sender.writeRow("end")` を実行
  - 実送信は `"end\\r\\n"`

補足:

- ACK/レスポンス待ちや再送制御はこの層にはありません。
- 1回の開始コマンド送信後は、停止まで追加のMCU制御文字列を送らない設計です。
- `McuCommand` は `Sender.writeRow(...)` を直接使用するため、Data Format 設定のバイナリ変換経路（`KeyPress.convert2list*`）は通りません。

### 6.2 既定 sync_name 一覧

| UI表示名 (`NAME`) | クラス | `sync_name` | 開始時送信文字列 |
|---|---|---|---|
| A連打 | `Mash_A` | `mash_a` | `mash_a\r\n` |
| 自動リーグ周回 | `AutoLeague` | `auto_league` | `auto_league\r\n` |
| 無限ワット | `InfinityWatt` | `inf_watt` | `inf_watt\r\n` |
| きのみ回収 | `PickUpBerry` | `pickupberry` | `pickupberry\r\n` |

### 6.3 ローダーとの関係

- `Window.loadCommands()` で `Commands/McuCommands` を `CommandLoader` が読み込みます。
- `NAME` は UI 表示時に `"{元NAME} ({ディレクトリ})"` 形式へ変換されます（実装上の表示名）。

---

## 7. カメラ・マウス・キーボード外部入力

### 7.1 カメラ

- OpenCV キャプチャ
  - Windows: `cv2.CAP_DSHOW`
  - 非Windows: `cv2.CAP_V4L2`
- 基本サイズ: `1280x720`
- Flip: `None/Vertical/Horizontal/Both`

### 7.2 マウス（Preview Canvas）

- `Ctrl+Shift+左ドラッグ`: 範囲キャプチャ保存
- `Ctrl+Alt+左ドラッグ`: 名前付き保存
- `Ctrl+右ドラッグ`: Qingpi タッチ領域設定
- `Use LStick Mouse`: 左ドラッグ→左スティック
- `Use RStick Mouse`:
  - Default/3DS: 右ドラッグ→右スティック
  - Qingpi: 右クリック/ドラッグ→タッチ

### 7.3 キーボード

- `SwitchKeyboardController` が `settings.ini` `KeyMap-*` を参照して入力変換

### 7.4 ゲームパッド

- Pro Controller / Xinput に対応
- 内部でシリアル形式へ変換して送信

---

## 8. 外部通信（Socket/MQTT）

`external_token.ini` から接続先を読み込み、未存在時は自動生成。

- SOCKET:
  - `addr`（初期 `127.0.0.1`）
  - `port`（初期 `49152`）
- MQTT:
  - `broker_address`, `id`, `fullaccess_token`, `readonly_token`

---

## 9. バグらしき挙動（記録のみ）

| 現在の挙動 | バグだと推察される内容 | 正しいと思われる挙動 |
|---|---|---|
| `change_buttons_position()` が 1/3 を保持できない | 条件分岐の `else` で常に2に戻る | 1/2/3 を保持 |
| 範囲キャプチャのY変換で `ratio_x` 使用箇所あり | `ratio_y` 未使用箇所があり座標ズレの恐れ | Yは `ratio_y` で変換 |
| LINE トークンが default プロファイル固定 | non-default profile でも切替されない | profile ごとの token 参照 |
| 右スティック変化判定比較式が重複 | インデックス比較の意図不一致の可能性 | X/Y を正しく比較 |
| Linux シリアル一覧で COM 前提 regex | Linux デバイス名で失敗可能性 | OS別に列挙/ソート実装 |
