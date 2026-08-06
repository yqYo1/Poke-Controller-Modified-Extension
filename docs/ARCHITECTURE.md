# 本体アーキテクチャ

この文書は、PokeCon本体の責務境界、process構成、状態遷移、安全性を変更する開発者向けです。

build手順とcheckの選び方は[本体開発ガイド](DEVELOPMENT.md)を参照してください。

利用者向けの操作説明、ユーザースクリプトAPI、周辺機器のwire protocolは、それぞれ専用文書を正本とします。

## 設計上の中心を理解する

PokeConの中心原則は、hardware resourceと共有状態をRust processが所有し、Python、Lua、Web UIを制限された境界から接続することです。

Python workerはserial port、camera handle、server socketを直接所有しません。

Luaを含む動的設定workerもdeviceを直接所有しません。

Web clientはREST、WebSocket、WebRTCを介してRustの公開状態だけを操作します。

この所有関係により、script停止、profile切替、client切断、application終了時のcontroller neutral化をRust側で強制できます。

## process topologyを把握する

通常実行では一つのRust application processがHTTP server、device、設定、lifecycleを所有します。

desktop modeは同じbackendへTauri windowとtrayを加えます。

web modeはdesktop windowを作らず、browserから同じbackendへ接続します。

別processの`pokecon-worker`はPython commandと動的PythonまたはLuaの実行を担当します。

user-script workerはactive profileでcommandが必要になった時点で遅延起動します。

動的設定workerは動的設定を有効にした起動で維持され、event callbackと設定generationを管理します。

process間通信は閉じたmessage契約を使用し、Python objectやnative handleを共有しません。

概略の所有関係は次のとおりです。

```text
Web browser / Tauri WebView
        |
        | REST, WebSocket, WebRTC
        v
pokecon Rust process
  app orchestration
  settings and state hub
  camera and serial ownership
  notifications and desktop lifecycle
        |
        | bounded worker IPC
        +--------------------------+
        v                          v
user-script worker          dynamic-config worker
Python command generation   Python or Lua generation
```

## Rust crateの責務を分ける

Cargo workspaceは`pokecon`の1 crateで構成されます。

| crate | 所有する責務 | 所有しない責務 |
|---|---|---|
| `pokecon` | composition root、service接続、profile、command、起動と停止順、runtime・diagnostics・platform・settings・camera・deviceの正準実装、serverの正準実装・HTTP wire型・OpenAPI generator、正準contract registry・schema・生成器、dynamic契約・main側状態、親側worker IPC・generation・supervision、child専用Python／Lua runtime、worker／compatibility／fault-fixture binとintegration test、desktop shell／test、icon、Tauri設定、Linux bundle input、署名対象policy | 内部責務を別Rust packageとして公開すること |

crate間の新しい依存は、この表の責務を逆流させないように追加します。

この表のPhase 2.5配置はdesktopのsource、icon、Tauri設定、Linux bundle input、OS別の署名対象policyまでを`rust/pokecon/`へ移した状態です。Package／Releaseはそのpolicyから実際に配布するLinux packageとWindows installerの名前、形式、size、SHA-256を記録したmanifestを生成し、引き渡し直前に実物と再照合します。Phase 2.6ではdesktopを製品featureで分岐する構成を廃止し、Webとdesktopを常に同じ本体へ含めます。Phase 2.7ではすべての旧compatibility packageを削除し、workerの3 bin、device／camera／settingsのintegration tests、settingsのbuild-time resource生成もPokeCon本体へ統合しました。

下位crateが`pokecon`を参照する構造はcomposition rootを壊すため避けます。

公開wire型をapplication handlerへ埋め込まず、private `server::api`へ集約します。

設定fieldを個別crateへ重複定義せず、registryとtyped accessを経由します。

## frontendの責務を限定する

`web/`はSvelteKit、Svelte、TypeScriptで構成されるstatic SPAです。

frontend runtimeとpackage managerはBunです。

Node.js専用runtimeをproduction経路へ追加しません。

frontendはOpenAPIから生成したTypeScript型を使用し、Rust wire型を手で転記しません。

frontend storeはserver snapshotと差分の表示用projectionであり、最終的なdevice stateの所有者ではありません。

page reloadやWebSocket再接続ではserver snapshotから復元できる必要があります。

未知API pathをSPA fallbackへ変換しないため、`/api`と`/ws`はstatic routingより前で閉じます。

## 正準契約と生成物を区別する

人が変更する正準入力と、生成される出力を混同しません。

| 契約 | 正準入力 | 主な生成物 |
|---|---|---|
| 設定 | `rust/pokecon/registry/settings.json` | Rust metadata、OpenAPI setting schema、frontend metadata |
| 動的event | `rust/pokecon/registry/protocol.json`の`builtin_events` | Python stub、Lua annotation、runtime registry |
| Python command API | Rust bindingとworker実装の公開surface | `python/pokecon/typings/`以下の`.pyi` |
| HTTPとWebSocket | `rust/pokecon/src/server/api.rs`とpath declaration | `api/openapi.json`、TypeScript client型 |
| 受入記録 | `acceptance-record.schema.json`とsemantic validator | exampleと検証結果 |
| 互換性 | 固定source manifestと期待値 | inventory、corpus report、追補chain |

生成物を直接編集すると、次回生成時に失われるだけでなく、runtimeと型が不一致になります。

正準入力を変更し、generatorを実行し、差分をreviewします。

対応commandは[本体開発ガイド](DEVELOPMENT.md#生成物を正準入力から更新する)にあります。

## 設定pipelineを追う

設定はdefault、global TOML、profile TOML、環境変数、動的設定、明示CLIの順で解決されます。

各fieldはscope、surface、mutability、secret、validationをregistryで宣言します。

起動時pipelineはplatformごとのConfig、Data、Cache、State rootを確定してから保存fileを読みます。

bootstrap-only fieldはworkerやrootを作る前に確定し、runtime中に変更しません。

startup-only fieldは保存できても、running serviceへ即時適用せず再起動待ちになります。

runtime-immediate fieldは一つの設定transactionでvalidation、永続化、service適用、visible state commitへ進みます。

適用に失敗したfieldは`apply_failures`へ残し、UIが保存成功とruntime成功を混同しないようにします。

複数fieldを別revisionとして露出させず、一つのtransactionを一つのvisible revisionとして公開します。

詳細なfield表は[設定リファレンス](SETTINGS.md)を正本とします。

## visible stateを一貫させる

`StateHub`は設定snapshotとruntime state snapshotを保持します。

これはHTTP／WebSocketへ公開するrevision付きUI projectionであり、controller、camera、serialの正準状態そのものではありません。`ApplicationBackend`が`StateHub`と各resource serviceを所有してprojectionを更新し、`server`と`desktop`はhardware handleまたはinterpreter stateを直接所有しません。

RESTは完全snapshotを返し、WebSocketはtransactionごとのsparse patchをrevision付きで配信します。

revisionはJavaScriptの精度に依存しない10進文字列です。

state変更とsettings変更を同じ操作でcommitする場合は、一つの`UiStateChange`へまとめます。

subscriberの遅延やqueue gapで差分を再現できない場合は、完全snapshotから再同期します。

camera thread、serial event、worker callbackが直接WebSocketへ書かず、application backendとstate hubを通します。

ephemeralなserial data、log、WebRTC signalingはrevision付きstateとは別のbounded channelを使用します。

stateとephemeral eventを同じ信頼性で扱わないことが、slow clientによる全体停止を防ぎます。

## controller入力の所有権を仲裁する

keyboard、mouse、gamepad、user script、dynamic callbackは同じcontroller stateへ影響できます。

入力sourceごとの所有権はcontroller arbiterが管理します。

sourceが切断された場合は、そのsourceが保持していた入力をreleaseします。

WebSocket接続は固有generationを受け取り、初期neutral snapshotが適用されるまで差分入力を開始しません。

古いgenerationや順序の壊れたsequenceを現在の入力へ混ぜません。

user script停止とprofile切替でもarbiterからsourceを切断します。

最終controller stateだけがserial managerへ渡り、選択したcodecでframeへ変換されます。

wire formatは[周辺機器開発ガイド](PERIPHERAL_DEVELOPMENT.md)を正本とします。

## serial managerの安全性を保つ

serial managerはnative port、baud rate、format、接続generation、再試行を所有します。

接続直後は操作frameより先にneutral frameを送ります。

frame全体のwriteが完了した場合だけdelta codecの前回stateをcommitします。

partial writeで失敗した場合は、送信済みと仮定して次のdeltaを省略しません。

disconnect、source loss、shutdownではneutral送信を試みてからportを閉じます。

自動再接続は同じnative selectorだけを使用し、別deviceを暗黙選択しません。

設定transactionによる再接続に失敗した場合は、旧selectorと旧formatへのrollbackを試みます。

受信byteは文字列へ推測変換せずraw chunkとしてserverへ渡します。

## camera frameのlifetimeを守る

camera managerはnative capture、resolution、flip、最新frame publicationを所有します。

consumerは最新frame sourceからpinしたframeを読み、capture threadのbufferへ直接参照を保持しません。

WebRTC、MJPEG、screenshot、Python image processingは同じ正準frame系列を入力にします。

resolution変更中もpin済みframeの内容が変化してはいけません。

shutdownではproducerを停止してから共有mappingを解放します。

writerが期限内に停止しない場合はmappingを早期解放せず、診断可能な失敗として残します。

色順、crop、flip、保存形式を変更した場合は仮想fixtureと実機camera gateの両方を更新します。

## user-script workerを世代として扱う

command sourceはData rootの`Commands`以下にあり、workerが再帰探索します。

discoveryではmoduleを実行してcommand metadataを収集し、executionでは対象generationのmoduleを再度実行します。

import時副作用があり得るため、discoveryを単なるstatic parseとして扱いません。

active profileごとにcommand generationを持ち、reloadとprofile切替は新generationを作ります。

running commandがある状態の切替では、停止期限とcleanupを経て旧workerをreapします。

worker IPCはbounded timeoutを持ち、hangしたworkerがRust shutdownを無期限に止めないようにします。

Python互換surfaceの詳細は[ユーザースクリプト開発ガイド](SCRIPT_DEVELOPMENT.md)を正本とします。

## 動的設定をtransaction generationとして扱う

動的PythonまたはLua sourceはstage環境で評価されます。

読み込み成功時だけcallback、option、command表示を新generationへ切り替えます。

syntax error、source cycle、callback登録失敗があれば、現在generationを維持します。

`Pre` eventが厳密なboolean `false`を返すと対象操作をcancelできますが、同じevent batchのcallbackは登録順に評価されます。

callbackにはtimeoutを設け、worker failureをdevice threadへ伝播させません。

動的設定が変更できるfieldはregistryのdynamic surfaceに限定されます。

公開objectとeventの詳細は[動的設定ガイド](DYNAMIC_CONFIGURATION.md)を正本とします。

## server境界を守る

RESTとWebSocketはbind addressから許可HostとOriginを導出します。

mutating HTTP requestはJSON media typeと`X-Pokecon-Request: 1`を必要とします。

WebSocketはOriginを必須とし、browser以外のclientにも同じ境界を適用します。

これらはcross-origin誤操作を減らしますが、利用者認証の代替ではありません。

loopback以外へのbindは同一networkのclientへ完全操作権限を渡します。

公開wire契約は[HTTP APIとリアルタイム通信](HTTP_API.md)を参照してください。

## shutdownを一つの順序へ収束させる

window close、tray quit、OS signal、fatal task failureは`ShutdownCoordinator`へ理由を送ります。

最初に受理した理由がprocess全体の停止系列を開始し、重複closeは別系列を作りません。

production shutdownは次の順序です。

1. 動的workerへ`AppShutdownPre`を通知し、新しい動的mutationを閉じます。
2. background reconcilerを停止し、controller arbiterの全入力をreleaseして現在のneutral stateを送ります。
3. camera producerを停止し、user-script commandとworkerを期限付きで停止します。
4. 動的workerを停止してreapします。
5. controllerを再度neutral化し、serialへneutral frameを送ってportを閉じます。
6. HTTP serverをcancelし、期限内に終了しなければtaskをabortします。

停止途中の一つのservice failureで後続のneutral化とserver停止を省略しません。

新しいresource ownerを追加する場合は、生成順だけでなく停止順、deadline、強制停止後のlifetimeを同時に設計します。

## 診断とsecretの境界を維持する

公開errorは閉じたmachine-readable codeとsecret-safe messageを返します。

内部logはstable diagnostic IDを付け、操作とresource identityを追跡可能にします。

Discord webhook、token、password、任意code本文、受信payloadをerrorや受入記録へ含めません。

native pathとserial identityは必要な範囲で表示し、公開issue用の記録では秘匿化します。

fail-softな外部通知やlegacy helperも、失敗を完全に消さず診断へ残します。

## 変更時に守る不変条件

本体変更のreviewでは、少なくとも次の不変条件を確認します。

- hardware handleはRust ownerからworkerやfrontendへ漏れない。
- controller sourceの終了は保持入力のreleaseへ収束する。
- serial接続、切断、shutdownはneutral frameを優先する。
- native selector failureで別deviceを暗黙選択しない。
- 一つの設定transactionは一つのvisible revisionとして観測される。
- startup-only設定はruntime適用済みと表示されない。
- worker sourceの失敗で成功中のgenerationを破壊しない。
- queue、IPC、network、shutdownには有限の上限またはdeadlineがある。
- secretと任意code本文はerror、log、schema exampleへ入らない。
- 公開契約変更は正準入力、生成物、文書、fixtureを同じ変更で更新する。

これらを一時的なCI分岐で回避せず、productionと同じ経路のfixtureまたは実機gateで検証します。
