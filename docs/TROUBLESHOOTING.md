# 症状から原因を切り分ける

この文書は、PokeConの起動、画面、camera、serial、command、通知、APIに問題が起きた利用者と開発者向けです。

最初に安全な利用者向け確認を示し、checkoutやNix commandが必要な項目は本体開発者向けとして明記します。

## 調査前に安全な状態へ戻す

対象consoleで意図しない入力が続く場合は、最初にserialを明示disconnectします。

disconnectできない場合はPokeConを終了し、MCUを対象consoleから外します。

同じportへ複数のPokeCon instanceやserial monitorを同時接続しません。

設定fileやData rootを削除する前に、applicationを終了してbackupします。

token、password、webhook URL、broker credentialをlogやscreen captureへ含めません。

原因が不明なままdevice selector、profile、Data rootを一度に変更しません。

一つの変更ごとに症状が変わったかを記録します。

## versionと実行形態を確認する

次のcommandが動くか確認します。

```bash
pokecon --version
pokecon --help
```

desktop installer、Nix package、source checkoutのどれを起動したかを記録します。

画面に表示されたversionと実行fileのversionが一致しない場合は、古いshortcutや別instanceを疑います。

desktop windowを閉じてもbackendが残る`keep_backend`設定では、trayのQuitから完全終了します。

processが残った状態で別versionを起動しません。

## 起動しない問題を診断する

installer利用者は同じ配布物を再導入し、application directoryの一部だけを手で移動しないでください。

`packaged application is missing`は、`pokecon`、`pokecon-worker`、`web/dist`、uv、Python runtimeのbundle構成が壊れていることを示します。

releaseの`SHA256SUMS`と取得fileのSHA-256を照合します。

Linux packageではWebKitGTK、PortAudio、udev、camera関連libraryが解決されている必要があります。

Windowsではinstallerを再実行し、WebView2 runtimeとbundle resourceを修復します。

portがすでに使用中なら、別PokeCon instanceまたは同じ`server.port`を使うprocessを終了します。

本体開発者は用途別のNix flake appで次を確認します。

```bash
nix run .#build-rust
nix run .#web-check
nix run . -- --exit-after-startup
```

host toolchainで成功してNixで失敗する場合は、host側を正として扱わずflakeの依存、source filter、environmentを調べます。

## 初回起動と保存先を診断する

初回起動後にConfig、Data、Cache、State rootが作成されたか確認します。

OS別の既定pathは[インストールガイド](INSTALL.md#初回起動で作る保存場所)を参照してください。

`--config-root`などでrootを変更した場合は、別の場所を見ていないか確認します。

保存先に書込み権限がない場合は、applicationを管理者として常用せず、利用者が所有するdirectoryへrootを設定します。

`settings.toml`のsyntax errorが疑われる場合は元fileをbackupし、直前に変更したtable、quote、arrayだけを確認します。

unknown fieldを勝手に削除する前に、対象versionの[設定リファレンス](SETTINGS.md)と照合します。

profile名、Config root、Data rootをissueへ載せる場合は個人名を秘匿化します。

## UIに接続できない問題を診断する

browser modeではaddress barのhostと`server.bind_address`、`server.port`を照合します。

loopback bindへ別machineから接続することはできません。

LAN bindには認証がないため、接続試験のためだけにInternet向けinterfaceへ公開しません。

backendが起動しているか`GET /api/state`で確認できます。

```bash
curl http://127.0.0.1:8020/api/state
```

JSONではなくconnection errorならlistener、port、firewallを調べます。

JSON snapshotが返るのにUIだけ失敗する場合はbrowser console、static asset response、WebSocketを分けて調べます。

## HTTP 200で404画面になる問題を診断する

HTTP statusが200でも画面に`404 Not Found`と表示される場合は、SPA buildがfallback pageだけになっている可能性があります。

これは未知APIの404とは別の症状です。

本体開発者はNix buildを実行します。

```bash
nix build .#web
nix build .#pokecon-server
```

`result/web/dist`に`index.html`だけでなく複数のJavaScript、CSS chunkがあることを確認します。

callerのsourceにはcomponentがあるのにNix buildのchunkが欠ける場合は、追加した`.svelte`、`.ts`、asset拡張子が`flake.nix`のsource filterに含まれるか確認します。

CI専用にfileをcopyせず、Nix packageのsource集合を修正します。

`nix run .#source-filter-check`と`nix run .#web-check`を再実行します。

## 設定が保存されない問題を診断する

UIのerror codeと`fields`を確認します。

`revision_conflict`は別clientまたは別操作が先に同じ設定snapshotを更新したことを示します。

現在設定を再取得し、変更意図をmergeしてから再送します。

`persistence_failed`はConfigまたはprofile TOMLの書込み、atomic replace、permissionを確認します。

`restart_required`にfieldがある場合は保存済みでもrunning serviceには未反映です。

`apply_failures`にfieldがある場合は永続化とruntime適用を分けて診断します。

一つのfieldが失敗したtransactionをfileの直接編集で部分適用しません。

scope、mutability、優先順位は[設定リファレンス](SETTINGS.md)を参照してください。

## Python workerを開始できない問題を診断する

managed uvとCPythonは起動時にresource manifestのSHA-256を検証します。

integrity errorではpackageを再取得し、releaseのchecksumとbundle全体を確認します。

実行fileだけを新旧package間で差し替えません。

user指定venvはexact sync対象であり、解決済み閉包にないpackageは削除されます。

既存projectやsystemの共有venvを指定せず、PokeCon専用venvを使用します。

既定worker依存の同期は同梱wheelhouseだけを使い、networkへ接続しません。

追加packageの同期失敗ではpackage名、Python 3.14対応、platform wheel、proxy、certificate、明示したuv config、offline cacheを確認します。

`POKECON_UV_`からbridgeされたsecret値そのものをdiagnosticへ貼り付けません。

本体開発者は`nix run .#compatibility`でworker起動と固定sourceをまとめて確認します。

## commandが一覧へ出ない問題を診断する

sourceはData rootの`Commands`以下へ置きます。

`PythonCommands`と`McuCommands`の相対構造を保ちます。

拡張子は小文字の`.py`である必要があります。

stemが`_`で始まるfile、symlink、root外へ解決されるpathは探索対象になりません。

command classは対象module自身で定義され、`PythonCommand`を継承し、具体的な`do`を持つ必要があります。

importしただけのclassは対象moduleのcommandとして列挙されません。

Python 3.14でparseできないsyntax、存在しないpackage、import時exceptionは明示的errorになります。

sourceはdiscovery時に実行されるため、module top-levelのdevice accessや無期限処理を除きます。

表示順をfilesystem走査順へ依存させません。

正確な探索規則は[ユーザースクリプト開発ガイド](SCRIPT_DEVELOPMENT.md#command-rootへsourceとassetを配置する)を参照してください。

## commandを開始または停止できない問題を診断する

Commands tabの状態が`running`、`paused`、`stopped`、`error`のどれかを確認します。

同じcommandを二重に開始したり、停止済みcommandを再開したりするとstate conflictになる場合があります。

停止要求後も動くsourceは、公開wait APIを使わずC extensionやblocking I/Oへ長時間留まっていないか確認します。

cleanupは`StopThread`を握りつぶさず、`finally`で外部resourceを閉じます。

script側がserial connectionを直接所有せず、公開proxyを使用しているか確認します。

profile切替とcommand reloadを同時に実行せず、現在操作の完了を待ちます。

Tkやdialogの未対応propertyは無視されずerrorになるため、公開surfaceと照合します。

method一覧は[ユーザースクリプト開発ガイド](SCRIPT_DEVELOPMENT.md)を参照してください。

## bridge_functionsが見つからない問題を診断する

`bridge_functions`は本体と異なるlicenseで作者が単体配布するため、repositoryと配布packageに含まれません。

必要なcommandだけがこのmoduleを要求することを確認します。

作者の配布元、対応version、licenseを確認し、利用者がData rootへ別途配置します。

配置先は`<Data>/Commands/PythonCommands/bridge_functions/`です。

Internet上の別copyや旧repositoryから由来不明のfileを再配布しません。

配置後にCommands tabからreloadし、import errorのmodule pathを確認します。

## serial portを列挙できない問題を診断する

Linux packageは一般的な`ttyACM*`と`ttyUSB*`へactive local session用udev ruleを導入します。

deviceを挿し直し、`udevadm info`、device nodeのownerとgroup、session種別を確認します。

headless環境や独自symlinkでは`dialout` groupまたは管理者ruleが必要な場合があります。

WindowsではDevice ManagerのCOM port、vendor driver、接続音を確認します。

別application、serial monitor、旧PokeConがportを占有していないことを確認します。

保存済みselectorが`available: false`なら、同名labelだけで別deviceを選ばず再選択します。

USB接続順でCOM番号や`/dev/ttyUSB*`が変わる環境ではnative identityの安定性も確認します。

## serialへ接続できない問題を診断する

port、baud rate、data formatを周辺機器firmwareと照合します。

`3ds`をUIから選択した場合のbaud変更と、TOMLやAPIからformatだけを変えた場合を混同しません。

接続直後にfirmwareがinitial neutral frameを受信できるか確認します。

default形式はCRLF終端ASCII、Qingpiと3DSは固定長binaryです。

8N1などregistryにないline settingを公開契約として仮定せず、firmware側captureで実際の条件を確認します。

loopbackでprotocolを確認してから対象consoleへ接続します。

本体checkoutではPTY経路を実行できます。

```bash
nix run .#virtual-io-check
```

wire byteとtest vectorは[周辺機器開発ガイド](PERIPHERAL_DEVELOPMENT.md)を参照してください。

## 切断後に入力が残る問題を扱う

切断後の保持入力は安全性に関わる不具合です。

再接続を繰り返さず、MCUを対象consoleから外します。

発生前の入力sourceがbrowser、gamepad、Python command、動的設定のどれかを記録します。

WebSocket generation、serial format、明示disconnectの有無、shutdown方法を記録します。

initial neutral、最後の操作frame、disconnect neutralをlogic analyzerまたはMCU側logで取得します。

raw captureに個体serial numberやpayloadが含まれる場合は公開前に秘匿化します。

virtual I/Oだけ成功してもUSB、firmware、consoleの問題を除外できません。

## cameraが列挙されない問題を診断する

OSのprivacy設定とdevice permissionを確認し、別applicationがcameraを占有していない状態にします。

Linux packageのudev ruleはactive local sessionへ`video4linux` accessを付与します。

headless環境では`video` groupまたは管理者ruleが必要な場合があります。

Windowsではprivacy設定でdesktop applicationのcamera accessも有効にします。

保存済みselectorが利用不能でも別cameraへ暗黙切替しないため、Camera tabで再選択します。

resolutionをcameraが提供しない場合は、対応formatとFPSをOS toolで確認します。

## camera映像が壊れる問題を診断する

既知の色と座標を持つframeを入力し、resolution、BGR色順、flip、cropを一つずつ確認します。

UI表示、screenshot、Python image processingで同じ位置がずれる場合はcapture側のresolutionを先に確認します。

一経路だけ異なる場合はWebRTC、MJPEG、保存encodeを分けて調べます。

resolution変更中だけ破損する場合はframe pinと共有mappingのlifetimeを疑います。

本体開発者はV4L2 loopback試験を実行します。

```bash
nix run .#virtual-io-check
```

`module not found`またはsymbol errorでは、実行中の`uname -r`と同じkernel用moduleと`v4l2loopback`を確認します。

既存indexを使う場合は`nix run .#virtual-io-check -- INDEX`を使用します。

## WebRTCとfallbackを診断する

WebRTCが確立しない場合も、JSON WebSocketとMJPEG fallbackが動くかを確認します。

browser consoleでoffer、answer、ICE candidateとWebSocket close reasonを分けて記録します。

NATを越える公開serviceとして設計されていないため、まず同一hostまたは同一信頼LANで確認します。

`webrtc.failure_timeout_seconds`後にfallbackへ移り、`webrtc.recovery_interval_seconds`ごとに主経路を再試行します。

fallbackの映像もない場合はcamera sourceまたはWebSocket自体を先に診断します。

主経路へ戻らない場合はconnectionを何度も増やさず、一つのclientでrecovery logを採取します。

## desktop windowが閉じない問題を診断する

`ui.desktop.close_behavior`は`ask`、`shutdown`、`keep_backend`のいずれかです。

`keep_backend`ではwindowを閉じてもtrayとbackendが残ります。

完全終了はtrayのQuitを使用します。

close連打、OS shutdown、tray quitが別processを残す場合はprocess IDと最初のshutdown reasonを記録します。

Linuxで描画が不安定な場合は`ui.desktop.disable_compositing = true`を設定して再起動します。

Windowsでも同じ設定がWebViewのGPU compositingを無効化します。

設定変更後の`restart_required`を確認します。

## Windows通知やDiscordが失敗する問題を診断する

Notifications tabのtestを一channelずつ実行します。

Windows通知はOS support、通知permission、desktop modeを確認します。

Discordはwebhook URL、timeout、送信先側のrate limitとresponseを確認します。

secret-safe errorだけでは原因が足りない場合は、送信先側の秘匿化済みrequest IDを照合します。

webhook URLをscreenshot、log、issueへ貼り付けません。

失敗後もcommand、UI、shutdownが継続するか確認します。

## MQTTやsocket helperが失敗する問題を診断する

broker address、port、room ID、client ID、timeoutを確認します。

DNS、certificate、firewall、送信先protocolをPokeCon外の安全なfixtureでも確認します。

legacy helperは一部の外部失敗をfail-softで扱うため、UI diagnosticと送信先側logを両方確認します。

受信payload、password、tokenを公開記録へ含めません。

blocking送信でcommand停止が遅れる場合は、公開helperが提供するbounded timeout内かを確認します。

## 動的設定を読み込めない問題を診断する

PythonまたはLuaのsyntax、選択language、source pathを確認します。

相対pathはConfig root内に留まり、`..` traversalとsymlink escapeは拒否されます。

明示した絶対pathは許可されますが、server processが読めることと信頼できるsourceであることを確認します。

`source()`の循環はerrorになり、現在の成功generationを維持します。

callback timeoutやevent名の誤りを確認し、起動全体が更新されたと仮定しません。

`bootstrap-only`設定は動的surfaceにないためTOML、環境変数、CLIへ移します。

詳細は[動的設定ガイド](DYNAMIC_CONFIGURATION.md)を参照してください。

## APIの403または415を診断する

`403 request_forbidden`では`Host`、`Origin`、`X-Pokecon-Request`を確認します。

WebSocketはbrowser以外のclientでも`Origin`が必須です。

mutating requestは`X-Pokecon-Request: 1`を必要とします。

`415 unsupported_media_type`では`Content-Type: application/json`を確認します。

LAN bindのaddressと`localhost`を混ぜず、実際のlistener authorityを使用します。

curl例と共通envelopeは[HTTP APIとリアルタイム通信](HTTP_API.md)を参照してください。

security headerを無効化する開発用routeを作らず、clientを公開境界へ合わせます。

## 動作が遅い問題を診断する

camera resolution、capture FPS、UI FPS、WebRTCかMJPEGか、同時client数を記録します。

Python command、動的callback、notification、camera encodeのどれが同時実行中かを減らして切り分けます。

CPU、memory、handleまたはdescriptor、queue overflow、reconnect回数を時間軸で採取します。

debug buildの値をrelease performance thresholdと比較しません。

短い一sampleだけでp95を推測せず、[外部受入ゲート](ACCEPTANCE.md#性能gate)の条件で測定します。

performanceを上げるためにneutral化、timeout、revision同期を無効化しません。

## 不具合報告に必要な情報を集める

報告には次を含めます。

- PokeCon versionとsource commit。
- installer、Nix package、source checkoutの区別。
- OS、architecture、desktopまたはweb mode。
- active profileを秘匿化した識別子。
- 発生時刻と再現操作。
- 期待結果と実結果。
- HTTP status、error code、diagnostic ID。
- cameraまたはserialの設定を秘匿化した値。
- 仮想I/O、互換test、実機gateの実施結果。
- 最小再現sourceのhashと、共有可能な場合だけ本文。

次は含めません。

- token、password、webhook URL、broker credential。
- 生のserial numberや個人名を含むpath。
- 任意の受信payload。
- license上再配布できない外部module。
- 無関係なData root全体。

実機問題では[外部受入ゲート](ACCEPTANCE.md)のrecord schemaを使用すると、環境とstepを一貫して記録できます。
