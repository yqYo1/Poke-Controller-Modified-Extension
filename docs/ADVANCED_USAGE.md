# PokeCon上級利用ガイド

この文書は、一般操作を理解した上で、profile、TOML、worker環境、LAN公開、外部APIを管理する利用者を対象にします。

ここにある設定は値として変更できても、意味を理解しない変更を推奨しません。

通常のcamera、serial、command操作は[利用ガイド](USER_GUIDE.md)を参照してください。

各設定IDの型、default、scope、反映時期は[設定リファレンス](SETTINGS.md)を参照してください。

## 変更前に復旧点を作る

上級設定を変更する前にPokeConを正常終了します。

Config rootとData rootを別の保存先へcopyします。

少なくともglobal `settings.toml`、対象profileの`settings.toml`、`init.py`、`init.lua`、`Commands`を含めます。

秘密値を含むbackupは暗号化するか、accessを限定します。

一度に複数の独立した設定群を変更せず、変更後に起動、camera、serial、command停止を確認します。

runtimeで変更する場合は設定snapshotのrevisionを記録します。

## profileの境界を設計する

profileには、用途と一緒に切り替えたいcamera screenshot形式、入力source、notification trigger、UI配置、shortcut、script worker環境を置きます。

camera device、serial port、server、Discord Webhook本体はglobal設定として全profileで共有します。

profile名には対象ゲーム、対象console、試験用途など、運用上区別できる名前を付けます。

同じ名前の大文字小文字を変えて別profileを作る運用は、OS間で混乱しやすいため避けます。

profile切替は次の処理を一つのlifecycle gateで行います。

1. 切替先の名前と設定を検証します。
2. `ProfileSwitchPre`を発行します。
3. 旧profileの実行中commandとworkerを停止します。
4. 切替先の設定とcommand cacheを準備します。
5. active profileを原子的に公開します。
6. `ProfileSwitchPost`を発行します。

切替先のuser-script workerは必要になるまで起動しません。

旧workerの停止に失敗した場合は設定rollbackまたは強制停止の診断を確認します。

切替中に別の切替やcommand開始を重ねません。

## 設定surfaceを目的ごとに選ぶ

| surface | 適する用途 | 注意点 |
|---|---|---|
| UI | 日常的な変更と状態確認 | UIにない設定は変更できない |
| global／profile TOML | 永続的な上級設定 | scopeとTOML名を確認する |
| 環境変数 | secret注入、container、process単位の差分 | shell履歴とprocess環境への露出に注意する |
| CLI | 一回の起動だけに適用する最優先値 | 明示したflagだけが上書きする |
| 動的設定 | eventに応じたruntime overlayとcommand表示変更 | 任意code実行であり信頼境界が広い |
| HTTP API | UI以外のlocal clientによるtransaction | revision、Host、Origin、header契約が必要 |

同じ設定を複数surfaceから指定すると、値の由来を追いにくくなります。

通常は永続値をTOMLまたはUIへ集約し、一時的な差分だけを環境変数かCLIへ置きます。

動的設定で固定値を代入する場合も、起動するたびにTOMLより後で上書きされることを記録します。

## startup-only値を変更して再起動する

`server.web_dir`、`server.port`、`server.bind_address`、`ui.desktop.disable_compositing`は保存しても現在processへ反映しません。

bootstrapの`dynamic_config_language`、`app_name`、`python.dynamic.*`も次回起動で反映します。

UIでは現在値と保存値を別に表示し、`Restart required`を示します。

再起動前に実行中commandを停止し、serialを明示的に切断します。

再起動後は待受address、profile、worker生成結果を確認します。

新しい設定で起動できない場合は、backupしたglobal TOMLへ戻すか、CLIで既知の安全な値を明示します。

```bash
pokecon --ui web --bind-address 127.0.0.1 --port 8020
```

## LAN公開の信頼境界を確認する

`server.bind_address`のdefaultは`127.0.0.1`です。

loopback以外の数値IPを指定すると、そのaddressとportへ到達できるclientにRESTとWebSocketを公開します。

公開APIにはcommand開始、controller入力、設定変更、profile切替、camera取得、serial操作、PythonまたはLuaの動的code読込が含まれます。

PokeConは利用者認証、role、API token、peer IPごとの権限制限を提供しません。

Host、Origin、Content-Type、固定request headerの検査はありますが、許可されたclientを個人として認証する機能ではありません。

この境界では接続を許可したLAN clientをPokeCon運用者と同等に信頼します。

家庭内LANであっても、guest network、共有Wi-Fi、VPN peer、port forwardが到達できる構成では公開しません。

internetへ直接公開しません。

reverse proxyを置くだけではWebSocket、Origin、controller入力、動的codeの権限設計を自動的に解決しません。

LAN公開が必要な場合は、専用VLANまたは明示的なfirewall ruleで到達元を限定します。

保存時にUIが示す警告を確認し、再起動後に意図したlocal IPだけで待ち受けていることをOS側でも確認します。

release候補では[Security acceptance](ACCEPTANCE.md#security-acceptance)を実行します。

## WebRTCとfallbackを調整する

映像はWebRTCを主経路にし、利用できない場合はWebSocket上のMotion JPEGへfallbackします。

`stun_server`が空文字ならSTUNを使用しません。

STUN URIを変更した値は、次のWebRTC接続または再接続から使用します。

`webrtc.auto_recover`が`true`ならfallback中に主経路への復旧を試みます。

`webrtc.recovery_probe_interval_sec`は復旧probeの間隔です。

値を短くしすぎるとfailure中のsignal処理を増やすため、実測なしに変更しません。

`jpeg_quality`はfallback frameとJPEG保存の品質と処理負荷へ影響します。

camera capture FPS、UI FPS、映像codec、network遅延は別の要因なので、一つの設定だけで映像性能を説明しません。

## WebSocketの監視値を変更する

`websocket.ping_interval_sec`はserverが接続確認を送る間隔です。

`websocket.pong_timeout_sec`は応答待ちの上限であり、ping間隔を超える構成は受け付けません。

`websocket.reconnect_interval_sec`と`websocket.reconnect_max_retries`はclient側の次回再接続系列から反映します。

既定の再接続は3秒間隔で最大20回です。

長いnetwork断を許容するために回数だけを増やすと、終了済みbackendへ古いUIが接続を試み続ける時間も増えます。

network障害とprocess障害を区別できる監視と組み合わせます。

## user-script専用venvを管理する

`python.script.venv`はprofileごとのuser-script workerに使用する専用venvです。

defaultはData rootの`venv-script`です。

指定directoryが存在しない場合は作成できます。

PokeConはmanaged Python、managed uv、package解決結果を使ってvenvをexact syncします。

「exact sync」は解決済み閉包に含まれないdistributionを削除できる同期です。

他のproject、system Python、手作業で保守するvenvを指定しません。

同じcanonical venv pathに対する同時準備はprocess内で一つにまとめ、process間ではOS lockで直列化します。

lock待ち後もmanifestを再検証し、別processが準備を完了していれば重複同期を避けます。

準備失敗はworker単位のerrorとして扱い、application全体を直ちに終了させません。

設定変更は次のuser worker generationで反映します。

確実に反映させるには実行中commandを停止し、CommandsのReloadまたはprofile切替を行います。

## package要求を宣言する

`python.script.packages.list`はprofile TOMLへpackage objectのarrayとして書きます。

各objectの`name`は必須で、`version`と`extras`は任意です。

```toml
[python.script.packages]
list = [
  { name = "rich", version = ">=13,<14" },
  { name = "example", extras = ["image"] },
]
```

`version`にはPEP 440のspecifierまたはdirect sourceを指定できます。

relativeなlocal pathを含むdirect sourceは、その値を定義した設定layerの基準directoryから解決します。

applicationが必要とするpackageと利用者指定はpackage名ごとに決定的にmergeします。

同じpriority内で矛盾するspecifierは、どのclauseが矛盾したかを保持して拒否します。

defaultではapplication constraintを優先し、互換runtimeを保護します。

`override_application_constraints = true`は利用者指定をapplication constraintより優先するため、worker互換性を壊す可能性があります。

`override_package_metadata_constraints = true`は利用者指定をuv overrideへ渡し、dependency metadataの制約を置き換える可能性があります。

2個のoverrideは問題の意味と復旧手順を説明できる場合だけ有効にします。

mutableなVCS branchやlocal sourceを使う場合は、identityが固定されないことを理解した上で`revalidate_mutable_sources`を検討します。

release運用ではimmutable commitまたはdigestで固定したsourceを優先します。

## uvのnetworkとcredentialを限定する

application既定packageだけを同期する配布環境では、同梱wheelhouseを使い、uvをofflineにします。

追加packageを指定すると、明示したuv config、index、cache、network条件が必要になる場合があります。

ambientな`UV_*`環境変数はworker準備processへそのまま渡しません。

許可するuv環境変数は`POKECON_UV_`を付けて指定し、child processでは対応する`UV_`名へ変換します。

例えば`POKECON_UV_INDEX_URL`はchildの`UV_INDEX_URL`になります。

元の`POKECON_UV_*`名はchildへ残しません。

`POKECON_UV_OFFLINE=0`のような明示指定は、同梱wheelhouseによるoffline defaultより優先します。

index credentialを設定する場合はshell履歴、process環境、CI logへ表示されないsecret管理を使用します。

`python.script.packages.uv_config`へ指定するfileは存在するregular fileでなければなりません。

uv config内のcredentialをissueや診断bundleへ含めません。

## dynamic worker環境を起動前に固定する

`python.dynamic.venv`と`python.dynamic.packages.*`はbootstrap設定です。

動的Python code自身が実行環境を変更する循環を避けるため、runtimeの動的overlayでは変更できません。

変更後はapplication全体を再起動します。

dynamic workerは一つのpersistent generationとしてevent callbackとruntime overlayを保持します。

user-script workerとはvenv、lifecycle、公開namespaceが異なります。

`Commands`互換moduleを動的設定から使用しません。

公開APIとcallbackの書き方は[動的設定ガイド](DYNAMIC_CONFIGURATION.md)を参照してください。

## callbackの負荷上限を調整する

動的callbackは同時実行数とqueue容量を持ちます。

`dynamic.callback_max_concurrency`は同時実行できるcallback数です。

`dynamic.callback_queue_capacity`は待機できるcallback数です。

値を大きくするとmemoryと停止時の未処理作業が増えます。

`dynamic.callback_soft_timeout_ms`はcallbackへ回復可能なtimeout errorを通知する時点です。

`dynamic.callback_soft_timeout_grace_ms`はsoft timeout後にcleanupを許す猶予です。

`dynamic.callback_hard_timeout_ms`はgenerationを強制終了する最終上限です。

個別handlerは登録時にtimeoutを上書きできます。

soft timeoutを0にする意味やhard timeoutとの関係を検証せず、一律に0へ変更しません。

callbackの待ち時間が長い場合は、timeoutを伸ばす前に同期I/O、無限loop、重い画像処理をevent callbackから分離します。

## secretの更新と削除を区別する

UIが返す固定maskは保存済みsecretの存在だけを示します。

固定maskを再送してもsecretを置換しません。

新しいWebhook URLへ変更する場合は新しい値を送信します。

secretを削除する場合は空文字を明示的に保存します。

HTTP clientは設定snapshotに平文がないことを前提にし、読取値をbackupとして扱いません。

secretのbackupとrestoreはPokeConのmasked snapshotではなく、利用者が管理するsecret storeで行います。

## 設定失敗から復旧する

UIの「設定ステータス」で`restart_required`と`apply_failures`を確認します。

runtime resourceの適用失敗では、設定が保存されていないか、旧設定へrollbackされている可能性があります。

cameraやserialのselector変更失敗時は、別deviceへの自動切替を期待しません。

旧deviceが同時に利用不能ならrollbackも失敗し、保存selectorは残っても接続は切れた状態になります。

最新の設定snapshot、runtime state、診断IDを別々に記録します。

TOMLのparse errorで起動できない場合は、直前に変更したtableとvalueだけをbackupと比較します。

未知keyを削除する前に、古いversionまたはplugin用の値でないかを確認します。

worker環境の同期失敗では、venvを汎用環境として修復せず、package要求、uv config、cache、networkを確認します。

復旧手順が不明な場合は[トラブルシュート](TROUBLESHOOTING.md)で症状別に切り分けます。

## 外部clientから操作する

設定と状態を別processから管理する場合は[HTTP APIガイド](HTTP_API.md)を参照します。

mutation requestには`Content-Type: application/json`と`X-Pokecon-Request: 1`が必要です。

複数clientが設定を更新する場合は`expected_revision`を省略しません。

RESTだけでなくWebSocketのstate revisionも監視し、gapを検出した場合は`GET /api/state`と`GET /api/settings`でsnapshotを取り直します。

LAN clientを実装することは、PokeConを認証付きserviceへ変えることではありません。

到達可能なclientを信頼する運用境界は変わりません。
