# PokeConユーザースクリプト開発ガイド

この文書は、Commands画面から実行するPython 3.14のcommandを作成、移植、保守する開発者を対象にします。

Rust本体の変更方法は[本体開発ガイド](DEVELOPMENT.md)で説明します。

起動中ずっと動くPythonまたはLuaの設定codeは[動的設定ガイド](DYNAMIC_CONFIGURATION.md)で説明します。

## 公開APIと内部実装を区別する

ユーザースクリプトが依存できる公開moduleは次のとおりです。

- `Commands._meta`
- `Commands.CommandBase`
- `Commands.PythonCommandBase`
- `Commands.McuCommandBase`
- `Commands.Keys`
- `Commands.dialogue`
- `Commands.image_proc`
- `Commands.net`

`pokecon.commands`は同じ型を短いimportで利用するためのalias moduleです。

Rustのcrate、worker IPC message、native camera handle、native serial handleは公開APIではありません。

Python codeはhardware resourceを直接所有せず、公開APIからRust hostへ操作を依頼します。

公開signatureの正本はData rootへ生成される`typings/Commands/*.pyi`です。

この文書はAPIを作業目的ごとに説明し、すべての既定引数を重複して掲載しません。

## command rootへsourceとassetを配置する

**command root**はData rootの`Commands` directoryです。

既存scriptとの互換性を保つ標準的な配置例は次のとおりです。

```text
<Data>/Commands/
├── PythonCommands/
│   ├── examples/
│   │   ├── capture_once.py
│   │   └── templates/
│   └── bridge_functions/
├── McuCommands/
├── Template/
└── shared_assets/
```

PokeConはcommand root以下を再帰的に読み、拡張子が正確に`.py`のregular fileを候補にします。

file名のstemが`_`で始まるsourceはcommand discoveryから除外します。

symlink entryとcommand root外へ解決されるpathはcommand sourceとして扱いません。

sourceとassetの相対位置を移植元と同じに保つと、既存のrelative pathを維持しやすくなります。

新しいsourceを配置した後はCommands画面の「Reload」を実行します。

`bridge_functions` directoryは作者の配布物を利用者が別途導入する場合だけ存在し、本体の雛形や配布物には含まれません。

## 最小のPython commandを作る

`PythonCommand`を継承し、引数なしの`do()`を実装します。

```python
from Commands.Keys import Button
from Commands.PythonCommandBase import PythonCommand


class PressA(PythonCommand):
    NAME = "Aを1回押す"
    TAGS = ["example", "input"]

    def do(self) -> None:
        self.press(Button.A, duration=0.1, wait=0.2)
```

`NAME`は画面へ表示する`str`です。

`NAME`を省略した場合はclassのsymbol名を使用します。

`TAGS`は手動tagの`list[str]`または`None`です。

同じfileに複数の`PythonCommand` subclassを定義できます。

`do()`を実装していないabstractなsubclassは一覧へ出しません。

別moduleからimportしたclassは、そのsource自身が定義したclassではないため一覧へ出しません。

## discovery時のtop-level実行を安全にする

command discoveryは各sourceをPython moduleとして評価します。

command実行時には選択したsourceを新しい実行namespaceでもう一度評価します。

そのため、top-level codeは一覧更新時と実行時の両方で動く可能性があります。

device操作、network送信、長時間処理、dialog表示をtop-levelへ置きません。

実際の操作は`do()`へ置きます。

定数、class定義、純粋なhelper関数、importだけをtop-levelへ置く構成を推奨します。

一つのsourceのimportまたは評価に失敗すると、そのreload generationを公開できません。

optional dependencyをimportする場合は、package設定を先に用意します。

## module pathとtagの決まりを理解する

command root下のsourceは`Commands.PythonCommands`を先頭とするmodule pathへ変換します。

例えば次のfileは`Commands.PythonCommands.Samples.Rank.commands`になります。

```text
<Data>/Commands/PythonCommands/Samples/Rank/commands.py
```

directory名は`@Samples`と`@Rank`のautomatic tagにもなります。

path中の`Commands`と`PythonCommands`自体はautomatic tagへ含めません。

最終tagはautomatic tag、classの`TAGS`、動的設定が追加したtagを重複なしで統合します。

classの宣言順は同じsource内で保持します。

directory走査の順序には依存せず、全体の表示順が必要なら動的設定のsort callbackを使用します。

shortcutは`NAME`ではなくmodule pathを保存するため、file移動やrenameで割当が切れる場合があります。

## command lifecycleへ停止点を入れる

`do()`の開始後はcommand stateが`running`になります。

Pauseではworkerの実行を停止可能なcheckpointで保留し、Resumeで同じcommandを継続します。

Stop、profile切替、application終了はcommandへ停止を通知します。

Python 3.14のinstruction monitoringも停止を検査するため、純粋なPython loopでも停止要求を受け取れます。

blocking native callや外部library内の長時間処理は、Python instruction checkpointへ戻るまで停止できない場合があります。

長い処理を小さい単位へ分け、`checkIfAlive()`または`wait()`を定期的に通します。

```python
from Commands.PythonCommandBase import PythonCommand


class Poll(PythonCommand):
    def do(self) -> None:
        while self.checkIfAlive():
            self.print_t1("poll")
            self.wait(0.1)
```

`alive`はworkerが実行を継続できるかを返します。

`checkIfAlive()`は停止済みならcleanupして`StopThread`を送出し、継続中なら`True`を返します。

`finish()`は正常終了をhostへ通知し、cleanup後に`StopThread`を送出します。

`StopThread`は`Exception`のsubclassなので、広い`except Exception`で捕捉した場合は必ず再送出します。

停止を握りつぶすretry loopを作りません。

```python
from Commands.PythonCommandBase import PythonCommand, StopThread


class SafeCatch(PythonCommand):
    def do(self) -> None:
        try:
            self.wait(10)
        except StopThread:
            raise
        except Exception as error:
            self.print_t1(f"operation failed: {error}")
```

## cleanupを冪等にする

`postProcess`へ引数なしcallbackを設定すると、command cleanup時に一度呼びます。

停止処理はbutton releaseと`postProcess`の失敗をlogへ記録し、残りのcleanupを継続します。

script側でも`try`と`finally`を使い、自分が開いたfileや外部connectionを閉じます。

```python
from Commands.Keys import Button
from Commands.PythonCommandBase import PythonCommand


class HoldSafely(PythonCommand):
    def do(self) -> None:
        self.hold(Button.A, wait=0)
        try:
            self.wait(10)
        finally:
            self.holdEnd(Button.A)
```

hostはcommand generationの終了時に入力ownershipを解放しますが、script自身も対応するreleaseを明示します。

cleanupは複数回呼ばれても安全な処理にします。

## button、hat、stick、touchを操作する

`Commands.Keys`はcontroller入力の互換型を提供します。

| 型 | 主な値 |
|---|---|
| `Button` | `Y`、`B`、`A`、`X`、`L`、`R`、`ZL`、`ZR`、`MINUS`、`PLUS`、`LCLICK`、`RCLICK`、`HOME`、`CAPTURE` |
| `Hat` | `TOP`、4方向のdiagonal、`RIGHT`、`BTM`、`LEFT`、`CENTER` |
| `Stick` | `LEFT`、`RIGHT` |
| `Direction` | 左右stickの既定8方向または任意の角度と倍率 |
| `Touchscreen` | 320×240内の`x`と`y` |

`Button.SELECT`は`MINUS`、`Button.START`は`PLUS`のaliasです。

`Button.POWER`は`LCLICK`、`Button.WIRELESS`は`RCLICK`の互換aliasです。

`Button`は`IntFlag`なので複数buttonを`|`で結合できます。

```python
from Commands.Keys import Button, Direction, Touchscreen
from Commands.PythonCommandBase import PythonCommand


class MixedInput(PythonCommand):
    def do(self) -> None:
        self.press(Button.L | Button.R)
        self.hold(Direction.UP)
        self.wait(0.2)
        self.holdEnd(Direction.UP)
        self.press(Touchscreen(160, 120))
```

`press(buttons, duration, wait)`は入力、duration待機、release、後続waitを順に行います。

`pressRep(buttons, repeat, duration, interval, wait)`は指定回数を繰り返します。

`hold(buttons, wait)`はreleaseせずに入力し、指定時間だけ待ちます。

`holdEnd(buttons)`は対象入力をreleaseします。

`KeyPress.input()`と`inputEnd()`はより低水準の互換methodです。

`KeyPress.neutral()`はcommand sourceのcontroller入力をneutralへ戻します。

正確な値とconstructorは`Commands/Keys.pyi`を参照します。

## 待機方法を選ぶ

`wait(seconds)`は0.1秒を超える待機を小分けにし、停止状態を繰り返し確認します。

0.1秒以下では`short_wait(seconds)`を使用します。

`short_wait()`は精度を優先するbusy loopなので、長い待機へ使用するとCPUを占有します。

負の値や0は実質的に待機せず、停止状態だけを確認します。

通常は`wait()`を使用し、入力timingの短い区間だけ`short_wait()`を直接使用します。

OS schedulerとserial transportがあるため、指定秒数をhard realtime保証として扱いません。

## output panelと標準出力を使い分ける

| method | 出力先 |
|---|---|
| `print_t1` | Output #1 |
| `print_t2` | Output #2 |
| `print_t` | 呼出しごとに交互のoutput |
| `print_s`、`print_ts` | stdout設定で選んだ先 |
| `print_t1b`、`print_t2b`、`print_tb`、`print_tbs` | `w`、`a`、`d` mode付きの対応先 |

各methodはPythonの`print`と同様に`sep`と`end`を受け取ります。

mode付きmethodの正確な置換、追記、削除挙動は型情報と実行結果で確認します。

`show_var()`はscript instanceの公開可能なfieldをoutputへ表示し、内部fieldは除外します。

大量のframe単位logはUI queueと診断を圧迫するため、頻度を制限します。

secretや受信payload全体をoutputへ表示しません。

## raw serial APIを限定して使う

`Sender.writeRow(row)`は文字列からCRとLFを除去し、末尾へ`\r\n`を一度付けて送信します。

`Sender.write(data)`は`bytes`、`bytearray`、`memoryview`、または0から255のinteger listをそのまま送信します。

raw writeもcontroller codecと同じ非interleave write gateを通ります。

Unicode文字列を`Sender.write()`へ渡さず、必要なら明示的にencodeします。

```python
from Commands.PythonCommandBase import PythonCommand


class RawProtocol(PythonCommand):
    def do(self) -> None:
        self.keys.ser.writeRow("status")
        self.keys.ser.write(b"\x01\x02\x03")
```

`direct_serial(commands, waittimes)`は対応するwaitの後に各commandを改行なしのrowとして送ります。

2個のlistの長さが異なる場合は短い側までしか処理しないため、呼出し前に同じ長さを検証します。

`reload_com_port()`は接続中ならneutral化して閉じ、保存済みの同じ設定へ再接続します。

scriptが別portを自動選択する機能ではありません。

raw protocolを周辺機器と設計する場合は[周辺機器開発ガイド](PERIPHERAL_DEVELOPMENT.md)も参照します。

## McuCommandの同期rowを使う

`McuCommand(sync_name)`は開始時に`sync_name`を一行送信し、停止時に`end`を一行送信する互換commandです。

```python
from Commands.McuCommandBase import McuCommand

firmware = McuCommand("capture-sequence")
firmware.NAME = "MCU capture sequence"
firmware.TAGS = ["mcu"]
```

discoveryでは`McuCommand` instanceをcommand候補として検出します。

実行中はStopまでworkerが生存し、停止時に`end`を送ります。

`start(Sender, postProcess)`と`end(Sender)`をoverrideする場合は、同期row、`isRunning`、callback cleanupの互換性を保ちます。

McuCommandのrow protocolはcontroller data formatとは別のraw serial契約です。

firmware側で両者のframe境界を混同しません。

## dialogを型付きWidgetで作る

`Commands.dialogue.Widget`は`Entry`、`Check`、`Combo`、`Spin`、`Scale`、`Next`を型付きで作成します。

```python
from Commands.dialogue import Widget
from Commands.PythonCommandBase import PythonCommand


class AskCount(PythonCommand):
    def do(self) -> None:
        name = Widget("Entry", "Name", "sample")
        count = Widget("Spin", "Count", min=1, max=10, default=1)
        enabled = Widget("Check", "Enabled", True)
        self.show_dialog("Run settings", [name, count, enabled])
        self.print_t1(name.value, count.value, enabled.value)
```

blockingな`show_dialog()`は回答まで待ち、0を返します。

`blocking=False`ではdialog IDを返します。

`is_dialog_closed(id)`で状態を確認し、`wait_dialog(id)`で完了まで待てます。

回答後は各Widgetの`value`と`has_result`を確認します。

`Next`は複数pageの区切りとして使用します。

`dialogue6widget`、`dialogue6widget_save_settings`、`dialogue6widget_select_settings`、`dialogue`は既存script用のlegacy helperです。

新規scriptでは`Widget`と`show_dialog()`を優先します。

dialogのclose buttonはcommand停止を要求するため、未保存の入力を前提にしません。

## image processing commandを作る

camera frameを扱うclassは`ImageProcPythonCommand`を継承します。

runtimeが`Camera`と`CaptureArea`を渡してinstanceを作るため、通常は引数なしのconstructorを自作しません。

```python
from Commands.PythonCommandBase import ImageProcPythonCommand


class FindTemplate(ImageProcPythonCommand):
    NAME = "Templateを探す"

    def do(self) -> None:
        if self.isContainTemplate("target.png", threshold=0.8):
            self.print_t1("found")
            self.saveCapture("found.png")
```

`camera`と`cam`は同じ`Camera` objectを指します。

`gui`と`canvas`は同じ`CaptureArea` objectを指します。

camera imageはBGR順の`numpy.uint8` arrayとして扱います。

hostから受け取るframeと切り出したframeは、script側が安全に処理できる連続したcopyです。

## crop形式を明示する

画像APIの`crop_fmt`は4個のintegerの解釈を指定します。

空文字または空のcropは全体を表します。

| `crop_fmt` | 4要素の順序 |
|---|---|
| `1` | `x1, y1, x2, y2` |
| `2` | `x1, y1, width, height` |
| `3` | `x1, x2, y1, y2` |
| `4` | `x1, width, y1, height` |
| `11` | `y1, x1, y2, x2` |
| `12` | `y1, x1, height, width` |
| `13` | `y1, y2, x1, x2` |
| `14` | `y1, height, x1, width` |

cropは必ず4個のintegerを指定します。

画像範囲外のsliceが空になる場合は後続処理でerrorになるため、現在のcapture sizeから検証します。

新規scriptでは読みやすい`crop_fmt="1"`を優先し、既存scriptの形式を無理に一括変換しません。

## image APIを目的ごとに選ぶ

| 目的 | method |
|---|---|
| 最新frameを得る | `getCameraImage`、`Camera.readFrame`、`Camera.image_bgr` |
| captureを保存する | `saveCapture`、`Camera.saveCapture` |
| popupへ表示する | `popupImage` |
| image fileを開く | `openImage` |
| template基準directoryを変える | `setTemplateDir` |
| mode別の絶対pathを組み立てる | `get_filespec` |
| 一つのtemplateを検索する | `isContainTemplate` |
| 複数templateから最大scoreを選ぶ | `isContainTemplate_max` |
| GPU互換入口を使う | `isContainTemplateGPU` |
| file image内にcamera frameがあるか調べる | `isContainedImage` |
| overlayへ矩形を出す | `displayRectangle` |
| overlayへ文字を出す | `displayText` |
| Discordへframeを送る | `discord_image` |
| LINE互換入口を呼ぶ | `LINE_image` |

template matchingは必要に応じてgrayscale、mask、BGR range、binary threshold、source crop、template cropを適用します。

判定はscoreが`threshold`を超えた場合にtrueです。

templateが探索画像より大きい場合はerrorにします。

maskの縦横はtemplateと一致させます。

`LINE_image`はLINE Notify終了により送信せず、warningを記録します。

`discord_image`の送信失敗は互換性のためwarningとして扱う場合があるため、成功が必須なら送信先側も確認します。

## templateとcaptureのpath基準を理解する

`ImageProcPythonCommand`の既定template directoryは`<Data>/Commands/Template`です。

`setTemplateDir()`へrelative pathを渡すとcommand rootを基準にします。

`get_filespec(filename, "t")`はtemplate directoryを基準にします。

`get_filespec(filename, "c")`はData rootの`Captures`を基準にします。

その他のmodeはcommand rootを基準にします。

明示的な絶対pathはそのまま使用します。

scriptを配布する場合は絶対pathを埋め込まず、sourceとassetを一つのdirectory構造にまとめます。

file名を外部入力から作る場合はpath traversalと上書きをscript側でも検証します。

## CameraとCaptureAreaの互換methodを使う

`Camera`は`readFrame`、`isOpened`、`fps`、`capture_size`、`flip`、`flip_mode`、`set_flip`、`saveCapture`、`openCamera`、`destroy`、camera thread互換methodを提供します。

これらはnative handleをscriptへ渡すAPIではなく、Rust所有のcamera serviceへのproxyまたは互換stateです。

`CaptureArea`はrectangle、text、FPS、表示size、touchscreen area、mouse binding、range screenshot操作の互換methodを提供します。

新規scriptでは`displayRectangle`、`displayText`、`saveCapture`などの高水準methodを優先します。

`CaptureArea`のlow-level mouse methodは既存script互換のために残しており、browser event objectを自由に受け取る一般的GUI APIではありません。

正確なmember一覧は`Commands/PythonCommandBase.pyi`を参照します。

## Tkinter互換surfaceの範囲を守る

workerはscript UIへ投影できる固定範囲の`tkinter`互換surfaceを提供します。

対応する主要classは`Toplevel`、`Scale`、`Button`、`Label`です。

対応するmessageboxは`showinfo`、`showwarning`、`showerror`です。

root `Tk`と`filedialog`は実装していません。

`Toplevel`のparentには生のdesktop widgetではなく互換`CaptureArea`を使用します。

未対応class、property、optionは黙って無視せず`NotImplementedError`を返します。

新規scriptでは可能なら`Widget` dialogとoverlay APIを使用し、Tkinter互換surfaceへの依存を小さくします。

## socketとMQTTをRust proxy経由で使う

socketとMQTT helperはworker processからnative socket objectを公開せず、boundedなRust proxyへrequestします。

| 操作群 | method |
|---|---|
| socket接続 | `socket_connect`、`socket_disconnect` |
| socket送受信 | `socket_transmit_message`、`socket_receive_message`、`socket_receive_message2` |
| socket設定 | `socket_change_ipaddr`、`socket_change_port`、`socket_change_alive` |
| MQTT送受信 | `mqtt_transmit_message`、`mqtt_receive_message`、`mqtt_receive_message2` |
| MQTT設定 | `mqtt_change_broker_address`、`mqtt_change_id`、`mqtt_change_clientId`、`mqtt_change_pub_token`、`mqtt_change_sub_token` |

socket portは0から65535のintegerです。

header listは`list[str]`で指定します。

一部のlegacy MQTT helperは外部failureをwarningと空文字へ変換します。

例外がなかったことだけで外部配信成功と判断せず、broker側の受信も確認します。

tokenと受信payloadをlogへ出しません。

module-levelの`Commands.net`関数は実行中command contextへ転送します。

command context外で呼ぶと`RuntimeError`になります。

## Discordと終了済みLINE APIを区別する

`discord_text`と`discord_image`はRust notification serviceへrequestします。

既定の設定keyは`DISCORD_WEBHOOK`互換名です。

Webhook本体はglobal secret設定としてRust側に保持し、script出力へ戻しません。

`LINE_text`と`LINE_image`はLINE Notify終了後の互換入口として残り、送信せずwarningを記録します。

新規scriptにLINE Notify依存を追加しません。

外部通知が処理の成否を決める場合は、fail-softなlegacy helperだけに依存せず、application側と送信先側の結果を設計します。

## 追加packageをprofileへ宣言する

script内から`pip install`を実行しません。

`python.script.packages.list`へpackage名、version、extrasを宣言します。

```toml
[python.script.packages]
list = [
  { name = "example-package", version = ">=1,<2", extras = ["image"] },
]
```

設定は次のpackage resolutionまたはuser worker generationで反映します。

PokeConは専用venvをexact syncするため、解決済み閉包にない手動導入packageを削除できます。

application constraintのoverrideは互換APIが必要とするdependencyを壊す可能性があります。

overrideを使う前に[package要求を宣言する](ADVANCED_USAGE.md#package要求を宣言する)を読みます。

offline端末では追加packageのwheelまたはuv cacheも用意します。

## editorでPython 3.14として検査する

Config rootの`pyproject.toml`は生成typingsを`extraPaths`へ設定します。

editorのPython interpreterにはprofileで使う専用venvを選択します。

target versionはPython 3.14です。

新規scriptはすべてのpublic関数とmethodへ型を付けます。

PEP 695のtype parameter syntaxを使用できます。

`Any`で互換errorを隠すより、生成stubと実際のreturn shapeを確認します。

repository内で互換性全体を検査する本体開発者は次を実行します。

```bash
nix run .#compatibility
```

このtaskは固定corpusのsource hash、Python 3.14 parse、import、class discovery、runtime evidenceを検査します。

個人scriptの動作保証を自動的に追加するtaskではありません。

## 仮想I/Oから実機へ段階的に試験する

最初にserialを接続しない状態でdiscovery、dialog、output、停止を確認します。

次にPTY loopbackでraw rowとcontroller frameを確認します。

camera処理は既知patternを入れたV4L2 loopbackでcrop、BGR、template scoreを確認します。

repositoryのLinux仮想I/O試験は次を使用します。

```bash
nix run .#virtual-io-check
```

このtaskはnative serialとcamera backendまでを検査しますが、個別scriptの全lifecycleを自動実行するものではありません。

最後に実機MCU、対象console、物理cameraで停止時neutralと座標を確認します。

hardware依存の合格記録は[外部受入ゲート](ACCEPTANCE.md)に従います。

## `bridge_functions`を作者配布物として扱う

`bridge_functions`は本project本体と異なるlicenseで作者が単体配布しているため、PokeConへ同梱しません。

必要な利用者は作者の正規配布元から、licenseと対象versionを確認して取得します。

配置先は次のdirectoryです。

```text
<Data>/Commands/PythonCommands/bridge_functions/
```

既存import pathは次の形を維持できます。

```python
from Commands.PythonCommands.bridge_functions.bridge_functions import BridgeFunctions
```

PokeConの生成typingsはこの外部moduleの実装や再配布権を提供しません。

配布packageへ含める場合は、script配布者が作者のlicenseと再配布条件を別途確認します。

import errorを回避するために本体repositoryへcopyを戻しません。

## 公開methodを目的別に確認する

`PythonCommand`の公開memberは次のgroupに分かれます。

| group | member |
|---|---|
| lifecycle | `alive`、`do`、`finish`、`checkIfAlive`、`postProcess` |
| controller | `keys`、`press`、`pressRep`、`hold`、`holdEnd`、`wait`、`short_wait` |
| serial | `direct_serial`、`reload_com_port` |
| output | `print_t1`、`print_t2`、`print_t`、`print_s`、`print_ts`、`print_t1b`、`print_t2b`、`print_tb`、`print_tbs`、`show_var` |
| dialog | `show_dialog`、`is_dialog_closed`、`wait_dialog`、`dialogue6widget`、`dialogue6widget_save_settings`、`dialogue6widget_select_settings`、`dialogue` |
| socket | `socket_connect`、`socket_disconnect`、`socket_transmit_message`、`socket_receive_message`、`socket_receive_message2`、`socket_change_ipaddr`、`socket_change_port`、`socket_change_alive` |
| MQTT | `mqtt_transmit_message`、`mqtt_receive_message`、`mqtt_receive_message2`、`mqtt_change_broker_address`、`mqtt_change_id`、`mqtt_change_clientId`、`mqtt_change_pub_token`、`mqtt_change_sub_token` |
| notification | `LINE_text`、`discord_text` |

`ImageProcPythonCommand`はこれらにcamera、capture、template、overlay、image notificationのmemberを追加します。

正確な引数順、default、overload、return typeは`Commands/PythonCommandBase.pyi`、`Commands/dialogue.pyi`、`Commands/image_proc.pyi`、`Commands/net.pyi`を参照します。

## 移植時の失敗を分類する

discoveryに出ない場合はfile拡張子、先頭`_`、classの`do()`、source import errorを確認します。

一覧には出るが開始できない場合はworker venv、class constructor、top-levelの二回目評価を確認します。

停止できない場合はblocking native call、`StopThread`の握りつぶし、長いC extension処理を確認します。

画像結果が違う場合はBGR順、capture resolution、crop形式、template size、thresholdを一つずつ確認します。

serial結果が違う場合はcontroller codecとraw `Sender` rowを分けてcaptureします。

通知だけ失敗する場合はfail-soft warning、credential、送信先logを確認します。

互換性不足を一時的なsource書換えだけで隠さず、元source、対象commit、期待挙動、hardware条件を記録します。

本体側で直す場合は公開contractまたは明示的なhardware gateとして扱います。
