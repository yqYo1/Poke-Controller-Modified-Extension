# PokeCon設定リファレンス

この文書は、PokeConの設定をUI以外から管理する上級利用者と、設定機構を変更する本体開発者を対象にします。

通常操作で変更できる項目は[利用ガイド](USER_GUIDE.md)を先に参照してください。

動的設定のprogramming APIは[動的設定ガイド](DYNAMIC_CONFIGURATION.md)で説明します。

## UIからの変更を標準経路にする

通常はWebまたはTauri UIから設定を変更します。

UIとHTTP APIは同じ**設定transaction**を使用します。

「設定transaction」は値の検証、runtime resourceへの適用、永続化、revision更新を一つの処理として扱います。

即時適用に失敗した値は保存せず、変更前のresourceと設定へのrollbackを試みます。

TOMLの直接編集は、UIにsurfaceがない設定、起動前に決める設定、復旧時の編集に限定します。

## Config rootへ設定を保存する

global設定はConfig rootの`settings.toml`へ保存します。

profile設定はConfig rootの`profiles/<profile>/settings.toml`へ保存します。

OSごとのConfig rootは[初回起動で作る保存場所](INSTALL.md#初回起動で作る保存場所)を参照してください。

初回起動は不足する雛形だけを作成し、既存のTOMLを上書きしません。

TOML writerは未知のkeyとcommentを保持します。

同一fileを複数processから更新する操作はOS lockと原子的な置換で直列化します。

## bootstrap、global、profileのscopeを区別する

**scope**は値を解決する時期と保存先を表します。

| scope | 役割 | 主な保存先 |
|---|---|---|
| `bootstrap` | app名、動的言語、動的worker環境など、他の設定より先に必要な値 | global TOML、環境変数、CLI |
| `global` | camera、serial、server、通知など、全profileで共有する値 | global TOML |
| `profile` | command、shortcut、UI表示、script workerなど、active profileと一緒に切り替える値 | profile TOML |

`bootstrap`は永続化scopeとは別に、起動時の先行解決を示します。

`app_name`にはTOML surfaceがなく、環境変数またはCLIで指定します。

global専用keyをprofile TOMLへ書いてもglobal値の上書きには使用しません。

`report_ignored_profile_global_settings`が`true`なら、無視したkeyを診断へ記録します。

profile名とapp名は空文字、`.`、`..`、path separator、NUL、drive表記を許可しない単一の安全なpath componentです。

大文字小文字やUnicodeの正規化は行わないため、表記が異なる名前は別のprofileとして扱われます。

## 後のlayerほど優先する

同じ正準設定IDは次の順に解決し、後のlayerが前のlayerを上書きします。

1. 組み込みdefault
2. global TOML
3. profile TOML
4. 環境変数
5. `init.py`または`init.lua`の動的overlay
6. 明示的なCLI引数

CLI引数を省略した場合、CLI parserのdefaultを最後に再注入して他のlayerを上書きしません。

bootstrap設定はworker生成前に先行解決しますが、global TOML、環境変数、明示CLIの優先関係は同じです。

動的overlayは対応する設定だけに適用でき、bootstrap専用設定には適用できません。

現在値の由来を調べる場合は設定snapshotのprovenanceを確認します。

## scalarと複合値のencodingを使い分ける

TOMLでは各settingの型に対応するTOML値を使用します。

CLIと環境変数のscalarは、正準レジストリが定めるboolean、integer、number、string、enumとして解析します。

listやobjectなどの複合値をCLIまたは環境変数へ渡す場合は、厳格なJSON文字列を使用します。

空listは`[]`であり、空objectは`{}`です。

CSVや空文字を複合値の代用にしません。

profile名をCLIで指定する例は次のとおりです。

```bash
pokecon --profile capture --serial-baud-rate 115200
```

同じ値を環境変数で指定する例は次のとおりです。

```bash
POKECON_PROFILE=capture POKECON_SERIAL_BAUD_RATE=115200 pokecon
```

package listを環境変数で指定する場合はJSON arrayを一つの値として渡します。

```bash
POKECON_PYTHON_SCRIPT_PACKAGES_LIST='[{"name":"numpy","version":">=2.2,<3"}]' pokecon
```

利用可能なCLI flagは`pokecon --help`で確認します。

## 即時、遅延、再起動後の反映を区別する

**mutability**は保存した値をruntimeへ反映する時期を表します。

| mutability | 反映規則 |
|---|---|
| `runtime_immediate` | resourceへ適用できた場合だけ、その場でcommitして保存する |
| `runtime_deferred` | 保存後、指定された次回接続、再接続、worker生成、package解決で反映する |
| `startup_only` | 保存後に再起動待ちとして保持し、次回起動で反映する |

`runtime_immediate`のhardware変更は、旧resourceを閉じて新resourceを開くtransactionを含む場合があります。

新resourceを開けない場合は同じ旧selectorと旧設定への復帰を試み、別deviceを暗黙選択しません。

`runtime_deferred`の現在値と保存値は、対応するtriggerが発生するまで異なる場合があります。

`startup_only`の保存値は`pending_restart_values`へ入り、`restart_required`に設定IDが表示されます。

server port、bind address、Web root、動的worker環境を現在processへ暗黙適用しません。

## device selectorの表記を保持する

cameraとserialにはUIで列挙したnative selectorの生値を保存します。

deviceが見つからない場合も、同種の別deviceを自動選択しません。

Linuxのserial一覧は`/dev/serial/by-id`、`/dev/serial/by-path`、直接のdevice nodeの順に安定selectorを優先します。

保存時にcase foldingやpath表記の正規化は行いません。

Windowsではnative COM名とcamera IDをそのまま扱います。

OSを移行した後はselectorをUIから選び直します。

## path設定の検証規則を守る

path設定は正準レジストリの`path` policyに従ってdirectoryまたはfileとして検証します。

Data相対defaultは対象appのData rootから解決します。

`python.script.venv`と`python.dynamic.venv`は存在しない場合に作成できるdirectoryです。

`python.*.packages.uv_config`は指定時点で存在するfileでなければなりません。

symlinkを許可するpathも、最終的なcanonical identityを用いてlockとmanifestを管理します。

`server.web_dir`は起動時にSPA assetを読むresource pathです。

未知のfrontend拡張子を本体へ追加する作業は設定変更ではなく、Nix source filterを含む本体変更です。

## secretを平文snapshotへ戻さない

`notifications.discord.webhook_url`は**secret設定**です。

「secret設定」はUI、設定snapshot、WebSocket、log、診断、debug表示、signed manifestへ平文を返しません。

現在の固定maskをそのまま保存した場合はrevisionを変更せず、secretを置換しません。

secretを更新する場合は新しい値全体を一度だけ送信します。

token、Webhook URL、passwordを移行記録、受入記録、issue、terminal履歴へ残しません。

環境変数を使う場合も、process一覧、shell履歴、CI logへの露出を考慮します。

## revision付き更新で競合を検出する

設定snapshotの`revision`はcanonicalな非負10進文字列です。

HTTP APIで更新するときは、読取時のrevisionを`expected_revision`へ指定できます。

```json
{
  "expected_revision": "12",
  "values": {
    "ui.fps": 60
  }
}
```

別clientが先に更新してrevisionが変わった場合、古いrevisionからの更新は競合として拒否されます。

競合時は最新snapshotを読み直し、差分を確認してから再送します。

一つの通常PATCHにglobal設定とprofile設定を混在させません。

profile切替は旧script workerの停止、新profileの検証、設定適用、公開を一つの切替処理として扱います。

## bootstrap設定の一覧を確認する

表の「反映」は`S`が`startup_only`を表します。

| 設定ID | default | 制約または用途 | 反映 |
|---|---|---|---|
| `dynamic_config_language` | `lua` | `python`、`lua`、`none` | S |
| `app_name` | `pokecon` | 安全な単一componentで、TOML surfaceなし | S |
| `python.dynamic.venv` | `<Data>/venv-dynamic` | 自動作成可能な専用directory | S |
| `python.dynamic.packages.list` | `[]` | package objectのarray | S |
| `python.dynamic.packages.override_application_constraints` | `false` | application制約より利用者指定を優先する危険なoverride | S |
| `python.dynamic.packages.override_package_metadata_constraints` | `false` | dependency metadataをuv overrideへ写す危険なoverride | S |
| `python.dynamic.packages.uv_config` | `null` | 存在するuv config fileまたは`null` | S |
| `python.dynamic.packages.revalidate_mutable_sources` | `false` | mutableなdirect sourceを起動時に再検証する | S |

動的workerのpackage設定はworker自身の生成条件なので、動的overlayやruntimeのHTTP PATCHでは変更できません。

## global設定の一覧を確認する

表の「反映」は`I`が`runtime_immediate`、`D`が`runtime_deferred`、`S`が`startup_only`を表します。

| 設定ID | default | 制約または用途 | 反映 | UI |
|---|---|---|---|---|
| `language` | `ja` | `ja`、`en` | I | あり |
| `auto_reload_config` | `false` | `init.py`または`init.lua`の変更監視 | I | あり |
| `dynamic.callback_soft_timeout_ms` | `2000` | 0以上 | I | なし |
| `dynamic.callback_soft_timeout_grace_ms` | `1000` | 0以上 | I | なし |
| `dynamic.callback_hard_timeout_ms` | `5000` | 0以上 | I | なし |
| `dynamic.callback_max_concurrency` | `8` | 1以上 | I | なし |
| `dynamic.callback_queue_capacity` | `1024` | 1以上 | I | なし |
| `report_ignored_profile_global_settings` | `true` | profile TOML内のglobal keyを診断する | I | なし |
| `active_profile` | `default` | 安全なprofile名 | I | あり |
| `camera.capture_fps` | `60` | 1以上 | I | あり |
| `camera.capture_resolution` | `1280x720` | `640x360`、`1280x720`、`1920x1080` | I | あり |
| `camera.device` | `0` | 0以上のintegerまたは空でないnative selector | I | あり |
| `camera.flip_mode` | `none` | `none`、`vertical`、`horizontal`、`both` | I | あり |
| `serial.port` | 空文字 | 空文字は未選択、その他はnative selector | I | あり |
| `serial.baud_rate` | `9600` | 1以上 | I | あり |
| `serial.data_format` | `default` | `default`、`qingpi`、`3ds` | I | あり |
| `notifications.line_menu_behavior` | `message` | `message`、`noop` | I | なし |
| `notifications.discord.webhook_url` | 空文字 | Discord Webhook URLまたは空文字、secret | I | あり |
| `notifications.discord.username` | 空文字 | 送信時username | I | あり |
| `notifications.discord.avatar_url` | 空文字 | HTTP URLまたは空文字 | I | あり |
| `websocket.reconnect_interval_sec` | `3` | 1以上、次回再接続から反映 | D | なし |
| `websocket.reconnect_max_retries` | `20` | 0以上、次回再接続から反映 | D | なし |
| `websocket.ping_interval_sec` | `15` | 1以上 | I | なし |
| `websocket.pong_timeout_sec` | `10` | 1以上 | I | なし |
| `webrtc.auto_recover` | `true` | fallback後の自動復旧 | I | なし |
| `webrtc.recovery_probe_interval_sec` | `30` | 1以上 | I | なし |
| `stun_server` | 空文字 | STUN URIまたは空文字、次の接続から反映 | D | あり |
| `jpeg_quality` | `85` | 1から100 | I | なし |
| `server.web_dir` | bundled `web/dist` | 空でないresource path | S | あり |
| `server.port` | `8020` | 1から65535 | S | あり |
| `server.bind_address` | `127.0.0.1` | wildcardではない数値IP | S | あり |
| `ui.desktop.close_behavior` | `ask` | `ask`、`shutdown`、`keep_backend` | I | あり |
| `ui.desktop.disable_compositing` | `false` | Desktopだけに効果がある | S | あり |

`server.bind_address`はhostnameを受け付けず、`0.0.0.0`や`::`のwildcardも受け付けません。

非loopback IPを指定する前に[LAN公開の信頼境界](ADVANCED_USAGE.md#lan公開の信頼境界を確認する)を確認します。

## profile設定の一覧を確認する

表の「反映」は`I`が`runtime_immediate`、`D`が`runtime_deferred`を表します。

| 設定ID | default | 制約または用途 | 反映 | UI |
|---|---|---|---|---|
| `camera.screenshot_format` | `png` | `png`、`jpeg` | I | あり |
| `input.keyboard_enabled` | `true` | browser key eventの受付 | I | あり |
| `input.left_stick_mouse_enabled` | `false` | camera canvasの左drag | I | あり |
| `input.right_stick_mouse_enabled` | `false` | camera canvasの右drag | I | あり |
| `input.touchscreen_area` | 全画面 | 0から1の`left`、`top`、`right`、`bottom` | I | あり |
| `notifications.discord.on_script_start` | `false` | script開始通知 | I | あり |
| `notifications.discord.on_script_end` | `false` | script終了通知 | I | あり |
| `notifications.windows.on_script_start` | `false` | Windowsのscript開始通知 | I | あり |
| `notifications.windows.on_script_end` | `false` | Windowsのscript終了通知 | I | あり |
| `ui.fps_options` | `[5, 15, 30, 60]` | 1以上の重複しない選択肢 | I | なし |
| `ui.fps` | `30` | 1以上 | I | あり |
| `ui.widget_mode` | `all` | outputとcontrollerの表示組合せ | I | あり |
| `ui.output_split_ratio` | `20` | 0から100 | I | あり |
| `ui.stdout_destination` | `output_1` | `output_1`、`output_2` | I | あり |
| `ui.controller_position` | `bottom` | `top`、`bottom` | I | あり |
| `ui.dialog_button_position` | `bottom` | `bottom`、`top`、`both` | I | あり |
| `ui.camera.live_view_enabled` | `true` | camera表示の有効化 | I | あり |
| `ui.camera.pixel_values_visible` | `false` | pixel inspectorの表示 | I | あり |
| `ui.camera.guide_visible` | `false` | 3分割guideの表示 | I | あり |
| `shortcuts.button_1` | 空文字 | command module path | I | あり |
| `shortcuts.button_2` | 空文字 | command module path | I | あり |
| `shortcuts.button_3` | 空文字 | command module path | I | あり |
| `shortcuts.button_4` | 空文字 | command module path | I | あり |
| `shortcuts.button_5` | 空文字 | command module path | I | あり |
| `shortcuts.button_6` | 空文字 | command module path | I | あり |
| `shortcuts.button_7` | 空文字 | command module path | I | あり |
| `shortcuts.button_8` | 空文字 | command module path | I | あり |
| `shortcuts.button_9` | 空文字 | command module path | I | あり |
| `shortcuts.button_10` | 空文字 | command module path | I | あり |
| `commands.tag_match_mode` | `exact` | `exact`、`partial`、`prefix`、`suffix` | I | あり |
| `python.script.venv` | `<Data>/venv-script` | 自動作成可能な専用directory | D | なし |
| `python.script.shutdown_timeout_ms` | `2000` | 0以上 | I | なし |
| `python.script.packages.list` | `[]` | package objectのarray | D | なし |
| `python.script.packages.override_application_constraints` | `false` | application制約より利用者指定を優先する危険なoverride | D | なし |
| `python.script.packages.override_package_metadata_constraints` | `false` | dependency metadataをuv overrideへ写す危険なoverride | D | なし |
| `python.script.packages.uv_config` | `null` | 存在するuv config fileまたは`null` | D | なし |
| `python.script.packages.revalidate_mutable_sources` | `false` | mutableなdirect sourceを次回解決時に再検証する | D | なし |

`input.touchscreen_area`は`left < right`かつ`top < bottom`でなければなりません。

shortcutにはdisplay nameやclass名ではなく、検出されたcommandのmodule pathを保存します。

## TOML名が設定IDと異なる項目を確認する

多くのTOML名は設定IDと同じですが、互換性のため異なる名前を持つ項目があります。

| 設定ID | TOML名 |
|---|---|
| `language` | `global.language` |
| `auto_reload_config` | `global.auto_reload_config` |
| `dynamic_config_language` | `global.dynamic_config_language` |
| `report_ignored_profile_global_settings` | `config.report_ignored_profile_global_settings` |
| `active_profile` | `profiles.active_profile` |
| `serial.port` | `serial.serial_port` |
| `serial.baud_rate` | `serial.serial_baudrate` |
| `serial.data_format` | `serial.serial_data_format` |
| `notifications.discord.webhook_url` | `notifications.discord_webhook_url` |
| `notifications.discord.username` | `notifications.discord_username` |
| `notifications.discord.avatar_url` | `notifications.discord_avatar_url` |
| `notifications.discord.on_script_start` | `notifications.discord_on_script_start` |
| `notifications.discord.on_script_end` | `notifications.discord_on_script_end` |
| `stun_server` | `webrtc.stun_server` |
| `jpeg_quality` | `video.fallback.jpeg_quality` |
| `ui.fps_options` | `ui.ui_fps_options` |
| `ui.fps` | `ui.ui_fps` |

`app_name`にはTOML名がありません。

正確な全surface mappingは正準レジストリを参照します。

## TOMLの例を最小限から作る

global TOMLの例は次のとおりです。

```toml
[global]
language = "ja"
auto_reload_config = false

[profiles]
active_profile = "default"

[camera]
capture_fps = 60
capture_resolution = "1280x720"
device = 0
flip_mode = "none"

[serial]
serial_port = "/dev/serial/by-id/example"
serial_baudrate = 9600
serial_data_format = "default"

[server]
port = 8020
bind_address = "127.0.0.1"
```

profile TOMLでscript packageを追加する例は次のとおりです。

```toml
[camera]
screenshot_format = "png"

[commands]
tag_match_mode = "exact"

[python.script]
venv = "/absolute/path/to/dedicated-pokecon-venv"
shutdown_timeout_ms = 2000

[python.script.packages]
list = [
  { name = "example-package", version = ">=1,<2", extras = ["image"] },
]
override_application_constraints = false
override_package_metadata_constraints = false
revalidate_mutable_sources = false
```

既存の汎用venvを`python.script.venv`へ指定しません。

workerのvenv同期は解決済み閉包にないpackageを削除できるため、PokeCon専用directoryを使用します。

## 正準レジストリと生成物を参照する

人向けの目的と運用上の注意はこの文書を正本とします。

機械的な型、range、scope、mutability、TOML、CLI、環境変数、動的設定、UI、OpenAPIのmappingは次の正準レジストリと生成物で確認します。

| path | 役割 |
|---|---|
| `rust/pokecon/registry/settings.json` | 78設定の正準レジストリ |
| `generated/settings.schema.json` | 全設定IDを必須keyとして持つ閉じたJSON Schema |
| `generated/settings-ui.json` | UI control、access、secret、scope、mutabilityのmetadata |
| `python/pokecon/typings/__init__.pyi` | Python動的設定の型情報 |
| `generated/lua/pokecon.d.lua` | Lua動的設定の型情報 |
| `api/openapi.json` | RESTで読書きする設定snapshotとPATCH schema |

生成物は手編集しません。

正準レジストリを変更した本体開発者は`nix run .#generate-contracts`で生成物を更新し、`nix run .#contract-check`でdriftを検査します。
