# PokeCon動的設定ガイド

この文書は、`init.py`または`init.lua`でruntimeの設定、event、profile、controller、command表示を制御する開発者を対象にします。

動的設定は任意codeを実行する上級機能です。

一般利用者はこの文書を読む必要がありません。

通常のPython commandを作る場合は[ユーザースクリプト開発ガイド](SCRIPT_DEVELOPMENT.md)を使用します。

## user-script workerとの違いを理解する

**動的設定**は、application lifecycle全体にわたって保持されるpersistent worker内のPythonまたはLua codeです。

「動的設定」は設定overlay、event callback、command並べ替え、profile切替、controller updateを扱います。

**ユーザースクリプト**は、Commands画面から選択して開始と停止を行うPython commandです。

動的設定の公開namespaceは`pokecon`です。

ユーザースクリプトの公開namespaceは`Commands`です。

動的設定から`Commands`互換moduleを使用せず、ユーザースクリプトから`pokecon`のpersistent stateを前提にしません。

両workerはvenv、lifecycle、停止条件、公開APIが異なります。

## Config rootのinit fileから始める

初回起動時にConfig rootへ`init.py`と`init.lua`を作成します。

既存fileは上書きしません。

`dynamic_config_language`が`python`なら`init.py`を、`lua`なら`init.lua`をprimary sourceとして使用します。

`none`ならprimary動的設定を起動しません。

Pythonの最小例は次のとおりです。

```python
import pokecon

pokecon.opt.language = "ja"


def on_camera_open() -> None:
    pokecon.state.tags = ["camera-ready"]


pokecon.autocmd.on("CameraOpenPost", callback=on_camera_open)
```

Luaの最小例は次のとおりです。

```lua
pokecon.opt.language = "ja"

local function on_camera_open()
    pokecon.state.tags = { "camera-ready" }
end

pokecon.autocmd.on("CameraOpenPost", {
    callback = on_camera_open,
})
```

値の大文字小文字を許容するenumでも、生成typingsが示す正準表記を使用します。

## 生成typingsをeditorへ読み込む

起動時にPython用の型情報をData rootの`typings/pokecon/__init__.pyi`へ生成します。

Lua用の型情報をData rootの`lua-typings/pokecon.d.lua`へ生成します。

Config rootの`pyproject.toml`は生成されたPython typingsとscript venvをeditorへ知らせる初期設定です。

Config rootの`.luarc.json`は生成されたLua typingsをLua language serverへ知らせます。

`pyproject.toml`と`.luarc.json`は初回だけ作成するユーザー編集fileです。

生成typingsは起動時に更新できるmanaged fileなので、直接編集しません。

正確なmember、型、Literalは生成typingsを参照します。

## 読み込みを一つのtransactionとして扱う

source読込は新しい**generation**をstageし、すべての評価と検証に成功した場合だけ公開します。

「generation」は、設定overlay、handler、custom event、command callbackを同時に入れ替える単位です。

新しいsourceでexception、型error、path error、commit errorが起きた場合は`loaded=false`となり、直前の成功generationを保持します。

失敗した評価の途中で代入した設定や登録したhandlerは公開しません。

handler IDはrollback後に再利用しないため、失敗を挟むと番号に欠番が生じます。

欠番は異常ではありません。

`Reload`は最後に成功したsourceをもう一度path検証して読み込みます。

成功したsourceがない状態で`Reload`すると拒否します。

`auto_reload_config`を有効にした場合も、失敗したgenerationへ切り替えません。

## UIからsourceを読み込む

画面上部のMenuを開き、「動的設定」から`.py`または`.lua`を選択します。

Web modeでは選択したcontentをConfig rootの対応する`init.py`または`init.lua`へ原子的に保存してから評価します。

Desktop modeでは許可されたpathからsourceを読み込めます。

読込結果の`display_path`は個人pathを必要以上に公開しない表記です。

「再読み込み」は最後に成功したsourceを対象にします。

sourceを読み込んだ直後は設定snapshot、active profile、command一覧、診断を確認します。

## source pathの境界を守る

相対pathはConfig rootを基準に解決します。

`..`によるlexical escapeとsymlinkによるConfig root外へのescapeを拒否します。

明示的な絶対pathはConfig root外でも使用できます。

絶対pathを許可するのは運用者が対象fileを明示的に信頼した場合だけです。

Linuxでは`~`と`~/...`をhome directoryとして展開します。

`.py`と`.lua`以外の拡張子は拒否します。

`pokecon.source(path)`は別sourceを現在の評価へ読み込みます。

同じcanonical fileをsource stack内で再び読み込むcycleは拒否します。

Python sourceからLua sourceを、Lua sourceからPython sourceを読み込めます。

```python
import pokecon

pokecon.source("./common.lua")
```

nested sourceの評価も外側と同じtransactionに含まれます。

## `pokecon.opt`で許可された設定だけを変更する

`pokecon.opt`は動的変更を許可した正準設定を型付きpropertyとして公開します。

代入時に型、range、scope、mutabilityを検証します。

無効な値を代入した場合は現在のtransactionを失敗させます。

bootstrap設定とserverのstartup-only設定は動的namespaceへ公開しません。

```python
import pokecon

pokecon.opt.camera.capture_fps = 30
pokecon.opt.camera.flip_mode = "horizontal"
pokecon.opt.ui.fps = 30
pokecon.opt.serial.baud_rate = 115200
```

```lua
pokecon.opt.camera.capture_fps = 30
pokecon.opt.camera.flip_mode = "horizontal"
pokecon.opt.ui.fps = 30
pokecon.opt.serial.baud_rate = 115200
```

動的設定はTOMLを直接書き換える機能ではなく、起動中のmemory overlayです。

次回起動でも同じ値を使うにはsourceを保持して読み込むか、適切なTOMLへ永続化します。

全propertyはData rootの生成typingsと[設定リファレンス](SETTINGS.md)で確認します。

## `pokecon.state`の読取値を使う

`pokecon.state`はRustが所有する現在の公開状態をsnapshotとして提供します。

| property | 意味 | 書込 |
|---|---|---|
| `serial_port` | 現在のserial selector | 不可 |
| `serial_baud_rate` | 現在のbaud rate | 不可 |
| `serial_connected` | serial接続状態 | 不可 |
| `camera_opened` | camera open状態 | 不可 |
| `camera_fps` | camera capture FPS | 不可 |
| `camera_resolution` | capture解像度 | 不可 |
| `camera_device` | camera selector | 不可 |
| `is_running` | application稼働状態 | 不可 |
| `command_state` | `running`、`paused`、`stopped`、`error` | 不可 |
| `current_command` | 実行中command名 | 不可 |
| `command_candidates` | 検出中のcommand metadata | ScriptLoad stage内で可 |
| `tags` | command tag一覧 | ScriptLoad stage内で可 |
| `active_profile` | 公開済みprofile | 不可 |
| `pending_profile` | 切替中のprofileまたは`None`／`nil` | 不可 |
| `available_profiles` | 利用可能なprofile | 不可 |
| `last_input` | 最後の入力説明 | 不可 |
| `holding_buttons` | 現在保持中のbutton名 | 不可 |
| `pid` | backend process ID | 不可 |

`command_candidates`と`tags`は`ScriptLoadPre`中のstageを編集するための例外的なmutable viewです。

mutable listやtableを取得して要素を変更した場合も、stageへ反映します。

他のstate propertyを設定の代用として書き換えません。

設定変更は`pokecon.opt`を使用し、profile切替は`pokecon.profile`を使用します。

## built-in eventへcallbackを登録する

次の22個のbuilt-in eventを定義済みです。

| lifecycle | event |
|---|---|
| application | `AppStartupPost`、`AppShutdownPre` |
| serial | `SerialConnectPost`、`SerialDisconnectPre`、`SerialDisconnectPost` |
| camera | `CameraOpenPost`、`CameraClosePre`、`CameraClosePost` |
| command | `CommandStartPre`、`CommandStartPost`、`CommandStopPre`、`CommandStopPost`、`CommandErrorPre`、`CommandErrorPost` |
| script discovery | `ScriptLoadPre`、`ScriptLoadPost` |
| dynamic config | `ConfigReloadPre`、`ConfigReloadPost` |
| input | `InputPressedPre`、`InputReleasedPost` |
| profile | `ProfileSwitchPre`、`ProfileSwitchPost` |

`pokecon.autocmd.on`は繰り返し実行するhandlerを登録します。

`pokecon.autocmd.once`は実際の最初の実行開始時にhandlerを登録解除します。

両方ともhandler IDを返します。

`pokecon.autocmd.off(handler_id)`は対象handlerを解除し、未知IDでもerrorにしません。

`pokecon.autocmd.clear(target)`は`all`、既知event名、group名のいずれかを指定します。

Pythonの登録例は次のとおりです。

```python
import pokecon


def release_controller() -> None:
    pokecon.controller.reset()


handler_id = pokecon.autocmd.on(
    "AppShutdownPre",
    callback=release_controller,
    group="safety",
    priority=100,
)
```

Luaの登録例は次のとおりです。

```lua
local handler_id = pokecon.autocmd.on("AppShutdownPre", {
    callback = function()
        pokecon.controller.reset()
    end,
    group = "safety",
    priority = 100,
})
```

callback引数は常にありません。

priorityはsigned 32-bit integerの範囲で指定します。

同じeventのcallbackはscheduler上でpriorityを使いますが、共有stateへ依存する順序制御として濫用しません。

## Pre eventの取消しを限定して使う

次のPre eventだけがcancellableです。

- `SerialDisconnectPre`
- `CameraClosePre`
- `CommandStartPre`
- `CommandStopPre`
- `CommandErrorPre`
- `ScriptLoadPre`
- `ConfigReloadPre`
- `InputPressedPre`
- `ProfileSwitchPre`

cancellableなPre eventでcallbackが正確なboolean `False`またはLuaの`false`を返すと、event全体の結果を取消しとして扱います。

文字列、0、空listなどのfalse相当値は取消しになりません。

複数handlerを同時にsnapshotして実行するため、一つのhandlerが`false`を返しても他のhandler実行を途中で打ち切りません。

Post eventとcancellableでないeventの戻り値は取消しに使いません。

安全な停止やneutral化を恒久的に拒否するcallbackは作りません。

## custom eventを定義して発行する

`pokecon.event.define(name)`は空でなくNULを含まないcustom eventを冪等に定義します。

`pokecon.event.list_defined()`はbuilt-inと明示定義したcustom eventを辞書順で返します。

未定義event名にもhandlerを先に登録できますが、`define`するまで発行できません。

`pokecon.event.emit(name)`は定義済みeventを発行します。

```python
import pokecon

pokecon.event.define("CalibrationFinished")


def on_calibration() -> None:
    pokecon.state.tags = ["calibrated"]


pokecon.autocmd.on("CalibrationFinished", callback=on_calibration)
pokecon.event.emit("CalibrationFinished")
```

同じeventをそのeventのcallbackから直接再発行する再帰は無視し、診断を記録します。

別eventを連鎖させる場合もcycleとqueue圧力を設計します。

## callback timeoutを個別に上書きする

handler登録時に`soft_timeout_ms`、`soft_timeout_grace_ms`、`hard_timeout_ms`を指定できます。

省略した値はglobalの`dynamic.callback_*`から継承します。

soft timeoutではPythonへ`pokecon.errors.CallbackSoftTimeoutError`を、Luaへ同等のerror objectを通知します。

Pythonでは通常のexceptionとしてcleanupできます。

```python
import pokecon


def bounded_work() -> None:
    try:
        while True:
            pass
    except pokecon.errors.CallbackSoftTimeoutError as error:
        print(error.handler_id, error.elapsed_ms, error.soft_timeout_ms)


pokecon.autocmd.on(
    "AppShutdownPre",
    callback=bounded_work,
    soft_timeout_ms=10,
    soft_timeout_grace_ms=100,
    hard_timeout_ms=250,
)
```

Luaでは`pcall`と`pokecon.errors.is_callback_soft_timeout(error)`で判定できます。

hard timeoutへ達したcallbackはgenerationの健全性を損なうため、長時間処理をcallback内へ置きません。

## `pokecon.profile`で切替を要求する

`pokecon.profile.current()`は現在のactive profile名を返します。

`pokecon.profile.list()`は利用可能なprofile名を返します。

`pokecon.profile.switch(name)`は切替を受理した場合に`True`または`true`を返します。

同じprofile、切替中、Pre callbackから許可されない再入操作では`False`または`false`を返します。

`ProfileSwitchPre`中は`state.active_profile`が旧profileを、`state.pending_profile`が切替先を示します。

`ProfileSwitchPost`では`state.active_profile`が新profileを示し、`pending_profile`は空になります。

```python
import pokecon

if "capture" in pokecon.profile.list():
    accepted = pokecon.profile.switch("capture")
    print(f"switch accepted: {accepted}")
```

profile切替はworker停止と設定commitを伴うため、event callbackから反復要求しません。

## `pokecon.controller`でsparse updateを送る

`pokecon.controller.update(value)`は指定fieldだけを変更するsparse updateです。

`pokecon.controller.reset()`は動的設定sourceが所有する入力をneutralへ戻します。

button fieldは`a`、`b`、`x`、`y`、`l`、`r`、`zl`、`zr`、`lclick`、`rclick`、`plus`、`minus`、`home`、`capture`です。

stickは0から255の`x`と`y`、または0から360度の`angle`と0から1の`strength`を指定します。

polar angleは0度が右、90度が上です。

hatは`up`、`down`、`left`、`right`、4方向のdiagonal、`neutral`です。

touchは`x`が0から319、`y`が0から239であり、`pressed=false`はreleaseを表します。

```python
import pokecon

pokecon.controller.update(
    {
        "a": True,
        "left_stick": {"angle": 90.0, "strength": 1.0},
        "hat": "up_right",
        "touch": {"x": 160, "y": 120, "pressed": True},
    }
)
pokecon.controller.reset()
```

update全体を先に検証するため、一つのfieldが不正なら他のfieldも適用しません。

終了、reload、profile切替ではcontroller ownershipをneutralへ戻す設計にします。

## command一覧を並べ替える

`pokecon.commands.sort.callback`へcallbackを設定すると、command一覧の表示順とseparatorを変更できます。

callbackは`CommandInfo`のlistを受け取り、`CommandInfo`と`CommandSeparator`からなるlistを返します。

`pokecon.commands.separator(label)`はseparatorを作ります。

```python
import pokecon


def sort_commands(commands):
    ordered = sorted(commands, key=lambda command: command["name"])
    return [pokecon.commands.separator("Alphabetical"), *ordered]


pokecon.commands.sort.priority = 10
pokecon.commands.sort.callback = sort_commands
```

生成typingsで`CommandInfo`と`CommandSortItem`の正確な型を確認します。

callbackが無効なshapeを返した場合は新しいdisplay cacheを公開しません。

同じcommandを複数回返すことは可能ですが、shortcut identityは元のmodule pathとclass名を使用します。

`pokecon.commands.sort.callback = None`またはLuaの`nil`でcustom sortを解除します。

sort callbackにもpriorityと3種類のtimeoutを設定できます。

## tag照合を上書きする

通常のtag照合は`commands.tag_match_mode`の`exact`、`partial`、`prefix`、`suffix`を使用します。

`pokecon.commands.tag_match.callback`を設定すると、選択tagと`CommandInfo`から表示可否を返せます。

```python
import pokecon


def match_fast(selected_tag: str, command) -> bool:
    return selected_tag == "fast" and "fast" in command["tags"]


pokecon.commands.tag_match.callback = match_fast
```

callbackはfiniteなtag一覧ごとのdisplay cache構築中に呼ばれるため、network I/Oや重い処理を入れません。

`None`または`nil`へ戻すとbuilt-in matcherを使用します。

## 読み込みとruntime errorを切り分ける

読込直後の`loaded=false`はsource評価またはcommitの失敗です。

以前のgenerationが動作している場合は、reload成功と誤認せず診断を確認します。

event時だけ発生するerrorはcallback failureとしてhandler IDとevent名を記録します。

soft timeout、hard timeout、queue overflowは別の原因なので区別します。

profile切替errorでは`active_profile`と`pending_profile`を確認します。

controllerが残る場合は`pokecon.controller.reset()`を呼び、applicationの「Release all input」とserial切断も実行します。

LAN clientからcodeを読み込んだ可能性がある場合は、source file、設定revision、接続元の信頼境界を確認します。

任意code本文、個人path、secretをissueへ貼らず、最小化した再現sourceを別途作成します。

## 公開契約の正本を参照する

動的設定の人向け挙動はこの文書を公開ガイドとします。

正確なPython signatureは`python/pokecon/typings/__init__.pyi`からData rootへ生成したtypingを参照します。

正確なLua annotationは`generated/lua/pokecon.d.lua`からData rootへ生成したtypingを参照します。

設定propertyは`rust/pokecon/registry/settings.json`を正本とします。

event名と公開namespaceは`rust/pokecon/registry/protocol.json`を正本とします。

本体開発者が契約を変更する場合は、[生成物を正準入力から更新する](DEVELOPMENT.md#生成物を正準入力から更新する)の検査を実行します。
