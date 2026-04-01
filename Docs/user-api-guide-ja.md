# ユーザー向け API ガイド（PythonCommands 用）

本書は、`SerialController/Commands/PythonCommands` に自作スクリプトを置くユーザー向けです。  
通信フォーマットや外部入力機器の詳細は `Docs/external-io-spec-ja.md` を参照してください。

---

## 1. スクリプト作成の基本

- 配置先: `SerialController/Commands/PythonCommands/`
- クラス要件:
  - `PythonCommand` または `ImageProcPythonCommand` を継承
  - `NAME`（文字列）を定義
- 実行本体:
  - `do(self)` を実装

例:

```python
from Commands.PythonCommandBase import PythonCommand
from Commands.Keys import Button

class MyCommand(PythonCommand):
    NAME = "My Command"
    def do(self):
        self.press(Button.A, duration=0.1, wait=0.2)
```

---

## 2. よく使う API

### 2.1 入力操作

- `press(buttons, duration=0.1, wait=0.1)`
- `pressRep(buttons, repeat, duration=0.1, interval=0.1, wait=0.1)`
- `hold(buttons, wait=0.1)`
- `holdEnd(buttons)`
- `wait(sec)` / `short_wait(sec)`
- `finish()`
- `direct_serial(serialcommands, waittime)`（上級者向け）
  - doの最後に必須ではなく、条件分岐で中断したいときにも使用可能

`buttons` は単体またはリストで指定可能:

- `Button.*`
- `Hat.*`
- `Direction(Stick.LEFT/RIGHT, angle_or_xy)`
- `Touchscreen(x, y)`（Qingpi 時）

#### 関数ごとの挙動

- `press`
  - 指定入力を押下 → `duration` 待機 → 解放 → `wait` 待機の順で実行
  - 実行のたびに停止要求（Stop）を確認
- `pressRep`
  - `press` を `repeat` 回繰り返し、最後に `wait` で待機
- `hold`
  - 指定入力を押しっぱなし状態にする
  - 同じ入力を再度 hold すると警告を出して無視
- `holdEnd`
  - hold 状態の入力を解除
  - hold されていない入力を解除しようとすると警告を出して無視
- `wait`
  - 0.1秒超は `sleep`、0.1秒以下はビジーウェイトで待機
  - 待機後に停止要求を確認
- `short_wait`
  - 常にビジーウェイトで待機（短時間精度重視）
- `finish`
  - コマンドを意図的に終了し、停止処理へ移行
  - ループ脱出や入力条件不一致時の早期終了に使用
- `direct_serial`
  - シリアル文字列をそのまま順次送信する
  - 各送信前に `waittime` 分だけ待機し、`writeRow_wo_perf_counter` で送信

#### 引数・戻り値詳細

- `press(buttons, duration=0.1, wait=0.1) -> None`
  - `buttons`: `GamepadInput`（`Button` / `Hat` / `Direction` / `Touchscreen` / それらのリスト）
  - `duration`: 押下継続秒（float）
  - `wait`: 解放後待機秒（float）
- `pressRep(buttons, repeat, duration=0.1, interval=0.1, wait=0.1) -> None`
  - `repeat`: 繰り返し回数（int）。`0` の場合は押下せず最後の `wait` のみ実行
  - `interval`: 連打中の間隔秒（最後の回には適用されない）
- `hold(buttons, wait=0.1) -> None`
  - `buttons`: 押しっぱなし対象
  - `wait`: hold 設定後の待機秒
- `holdEnd(buttons) -> None`
  - `buttons`: hold 解除対象
- `wait(sec) -> None`
  - `sec`: 待機秒。`>0.1` は `sleep`、`<=0.1` はビジーウェイト
- `short_wait(sec) -> None`
  - `sec`: 待機秒。常にビジーウェイト
- `finish() -> None`
  - 引数なし。コマンド停止要求を内部フラグに反映
- `direct_serial(serialcommands, waittime) -> None`
  - `serialcommands`: 送信文字列のリスト（`\r`/`\n` は内部で除去）
  - `waittime`: 各コマンド送信前の待機秒リスト
  - 実装は `zip(waittime, serialcommands, strict=False)` のため、長さ不一致時は短い側まで実行

### 2.2 ログ出力

- `print_s`: 標準出力先
- `print_t1` / `print_t2`: 上段/下段ログ
- `print_t`: 標準出力の反対側
- `print_t1b` / `print_t2b` / `print_tb`:
  - `mode="w"` 上書き, `mode="a"` 追記, `mode="d"` 消去

#### 関数ごとの挙動

- `print_s`
  - 現在の標準出力先に出力（Output#1 または Output#2）
- `print_t1` / `print_t2`
  - 出力先を固定して上段/下段ログへ表示
- `print_t`
  - 標準出力先「ではない側」に出力
- `print_t1b` / `print_t2b` / `print_tb`
  - `w`: 既存表示を消して書き換え
  - `a`: 末尾に追記
  - `d`: クリア（空文字で更新）

#### 引数・戻り値詳細

- `print_s(*objects, sep=" ", end="\n") -> None`
- `print_t1(*objects, sep=" ", end="\n") -> None`
- `print_t2(*objects, sep=" ", end="\n") -> None`
- `print_t(*objects, sep=" ", end="\n") -> None`
  - `*objects`: 表示対象（`str()` 変換される）
  - `sep`: オブジェクト連結文字
  - `end`: 行末文字列
- `print_t1b(mode, *objects, sep=" ", end="\n") -> None`
- `print_t2b(mode, *objects, sep=" ", end="\n") -> None`
- `print_tb(mode, *objects, sep=" ", end="\n") -> None`
  - `mode`: `"w"`（上書き）/ `"a"`（追記）/ `"d"`（消去）

### 2.3 ダイアログ

- `dialogue(...)`（Entry中心）
- `dialogue6widget(...)`（複数ウィジェット）
- `dialogue6widget_save_settings(...)`
- `dialogue6widget_select_settings(...)`

`dialogue6widget` の種別:

- `Entry`, `Check`, `Combo`, `Radio`, `Spin`, `Scale`, `Next`（列送り）

#### 関数ごとの挙動

- `dialogue`
  - Entry中心の簡易ダイアログ
  - `need=list` なら入力順配列、`need=dict` ならラベル名キーで返却
  - Cancel/クローズ時は `finish()` が呼ばれてコマンド終了
- `dialogue6widget`
  - 複数ウィジェット定義を表示して値を返す
  - `Next` を入れると次の列へ改段
  - ウィジェット名重複時はエラー表示後 `finish()`
- `dialogue6widget_save_settings`
  - 指定 JSON に前回値を保存/復元しながら表示
- `dialogue6widget_select_settings`
  - 保存済み設定ファイルを選んで読み込み後に表示
  - 実行後に「前回の設定.json」を更新

#### 引数・戻り値詳細

- `dialogue(title, message, desc=None, need=list) -> list[str] | dict | None`
  - `title`: ダイアログタイトル
  - `message`: `int | str | list[int | str]`（Entry ラベル）
  - `desc`: 説明文（省略時は `title`）
  - `need`: `list` または `dict` 推奨（戻り値形式）
- `dialogue6widget(title, dialogue_list, desc=None, need=list) -> list | dict | None`
  - `dialogue_list`: ウィジェット定義配列
    - `["Entry", name, init]`
    - `["Check", name, init_bool]`
    - `["Combo", name, choices, init]`
    - `["Radio", name, choices, init]`
    - `["Spin", name, choices, init]`
    - `["Scale", name, min, max, init, digit]`
    - `["Next"]`（次列へ）
- `dialogue6widget_save_settings(title, dialogue_list, filename, desc=None, need=list) -> list | dict`
  - `filename`: 設定保存先 JSON パス（親ディレクトリがなければ作成）
- `dialogue6widget_select_settings(title, dialogue_list, dirname, desc=None, need=list) -> list | dict | None`
  - `dirname`: プリセット JSON を列挙するディレクトリ
  - 選択結果に応じて `dirname/前回の設定.json` へ保存

---

## 3. 画像認識 API（ImageProcPythonCommand）

- `isContainTemplate(...)`
- `isContainTemplate_max(...)`
- `isContainTemplateGPU(...)`
- `isContainedImage(...)`
- `getCameraImage(...)`, `openImage(...)`
- `displayRectangle(...)`, `displayText(...)`
- `saveCapture(...)`, `popupImage(...)`

`crop_fmt` は複数形式を受け付けます（詳細は外部IO仕様書）。

#### 関数ごとの挙動

- `getCameraImage`
  - 現在フレームを取得し、必要なら crop して返却
- `openImage`
  - テンプレート/キャプチャ/絶対パスから画像を読み込み
- `isContainTemplate`
  - 現在フレーム内でテンプレート一致判定（True/False）
  - `show_value` で類似度表示、`show_position` で矩形描画
- `isContainTemplate_max`
  - 複数テンプレート中で最も一致した index と各類似度を返却
- `isContainTemplateGPU`
  - `isContainTemplate(..., use_gpu=True)` のラッパ
- `isContainedImage`
  - 「指定画像の中に現在フレーム（またはcrop）を含むか」を判定
- `displayRectangle` / `displayText`
  - Preview canvas へ矩形や文字を一定時間描画
- `saveCapture`
  - 現在フレームを保存（crop/パス指定可）
- `popupImage`
  - 現在フレーム（crop可）を別ウィンドウ表示

#### 引数・戻り値詳細

- `getCameraImage(crop_fmt="", crop=None) -> MatLike`
  - `crop_fmt`: 座標形式指定（詳細は外部I/O仕様）
  - `crop`: 切り抜き座標
- `openImage(filename, mode="t") -> MatLike | None`
  - `filename`: 画像名または絶対パス
  - `mode`: `"t"`=Template配下、`"c"`=Captures配下、その他=そのまま解決
- `isContainTemplate(... ) -> bool`
  - `template_path`: テンプレート画像名/パス
  - `threshold`: 一致判定閾値（既定 0.7）
  - `use_gray`: グレースケール照合有無
  - `show_value`: 類似度ログ出力
  - `show_position`: 検出矩形表示
  - `show_only_true_rect`: 不一致時の矩形表示抑制
  - `ms`: 矩形表示時間（ms）
  - `crop_fmt`, `crop`: 画面側の切り抜き指定
  - `mask_path`: マスク画像（指定時 NCC）
  - `use_gpu`: GPU照合有無
  - `BGR_range`: 色抽出下限/上限（`{"lower": ..., "upper": ...}`）
  - `threshold_binary`: 二値化閾値
  - `crop_template`: テンプレート側の切り抜き
  - `show_image`: デバッグ画像表示
  - `color`: `[一致枠色, 不一致枠色, crop範囲色]`
- `isContainTemplate_max(... ) -> tuple[int, list[float], list[bool]]`
  - `template_path_list`: テンプレート複数候補
  - `mask_path_list`: テンプレートごとのマスク一覧（`None` 可）
  - 戻り値: `(最大一致index, 各類似度, 各テンプレートの閾値判定)`
- `isContainTemplateGPU(... ) -> bool`
  - `isContainTemplate` と同等引数（`use_gpu=True` 固定）
- `isContainedImage(... ) -> bool`
  - `image_path`: 探索対象画像（この画像内に現在フレームを探す）
  - それ以外の主要引数は `isContainTemplate` と同様
- `displayRectangle(max_loc, width, height, tag=None, ms=2000, color=None, crop_fmt="", crop=None) -> None`
  - `max_loc`: 左上座標 `[x, y]`
  - `width`, `height`: 枠サイズ
  - `tag`: 描画タグ（省略時自動生成）
  - `color`: `[枠色, crop枠色]`
- `displayText(position, txt, tag=None, ms=2000, font="UD デジタル 教科書体 NP-B", fontsize=20, color="black") -> None`
  - `position`: `[x, y]`
  - `txt`: 表示文字列
- `saveCapture(filename=None, crop_fmt="", crop=None, mode=True) -> None`
  - `filename`: 拡張子なしファイル名（省略時は日時名）
  - `mode`: `True` で Captures 配下、`False` で指定パス扱い
- `popupImage(crop_fmt="", crop=None, title="image") -> None`
  - `title`: ポップアップウィンドウタイトル

---

## 4. 外部連携 API（スクリプトから呼ぶ関数）

- Socket: `socket_connect`, `socket_receive_message`, `socket_transmit_message` など
- MQTT: `mqtt_receive_message`, `mqtt_transmit_message` など
- Discord: `discord_text`, `discord_image`
- LINE:
  - 送信に使用していたサービスが終了した為使用不可。互換性の為に維持

#### 関数ごとの挙動

- Socket受信系
  - 指定ヘッダで始まるメッセージを待機し、一致時に返却
  - Stop時は `alive` 連動で待機ループを抜ける
- MQTT受信系
  - topic購読し、ヘッダ一致メッセージを返却
  - `receive_message2` は複数ヘッダ対応
- `discord_text` / `discord_image`
  - `keys` または `index` で webhook キーを選んで送信
  - 送信失敗は内部で握りつぶされる場合があるため、必要ならログ確認
- `LINE_text` / `LINE_image`
  - 現行実装では送信せず、代替としてDiscord利用を促すメッセージを表示

#### 引数・戻り値詳細

- Socket
  - `socket_change_alive(flag: bool) -> None`
  - `socket_change_ipaddr(addr: str) -> None`
  - `socket_change_port(port: int) -> None`
  - `socket_connect() -> None`
  - `socket_disconnect() -> None`
  - `socket_receive_message(header: str, show_msg: bool = False) -> str | None`
  - `socket_receive_message2(headerlist: list[str], show_msg: bool = False) -> str | None`
  - `socket_transmit_message(message: str) -> None`
- MQTT
  - `mqtt_change_broker_address(broker_address: str) -> None`
  - `mqtt_change_id(mqtt_id: str) -> None`
  - `mqtt_change_pub_token(pub_token: str) -> None`
  - `mqtt_change_sub_token(sub_token: str) -> None`
  - `mqtt_change_clientId(clientId: str) -> None`
  - `mqtt_receive_message(roomid: str, header: str, show_msg: bool = False) -> str | None`
  - `mqtt_receive_message2(roomid: str, headerlist: list[str], show_msg: bool = False) -> str | None`
  - `mqtt_transmit_message(roomid: str, message: str) -> None`
- Discord
  - `discord_text(content: str = "", index: int = 0, keys: str = "DISCORD_WEBHOOK") -> None`
  - `discord_image(content: str = "", index: int = 0, crop_fmt: CropFmt = "", crop: list[int] | None = None, keys: str | list[str] = "DISCORD_WEBHOOK") -> None`
  - `index != 0` の場合、Discord 送信先キーは `DISCORD_WEBHOOK{index}` に補正される

---

## 5. 実装準拠の注意点

- コマンド名は読み込み時に `NAME (ディレクトリ)` 表記になります。
- `TAGS` は UI フィルタに使用され、ディレクトリ由来の `@タグ` が自動追加されます。
- `ImageProcPythonCommand` のコンストラクタは `cam`（必要なら `gui`）を受ける形にすると互換性が高いです。
- `direct_serial` は生シリアル文字列をそのまま送るため、改行やフォーマット整合を自前で管理してください。
- Stop ボタンを押したときの終了は `checkIfAlive()` が呼ばれる地点で反映されます（重いループでは適宜 `wait` 等を挟む）。
