# Architecture handoff

この文書は、`PLAN.md` のレビュー引渡し情報を現行source、contract、配布証拠へ結び付けるための追補artifactです。製品要件の正本ではありません。規範要件は [`SPECIFICATION.md`](../SPECIFICATION.md) と三つの定義書、現在の実装構成は [`ARCHITECTURE.md`](ARCHITECTURE.md) が正本です。

## 1. 適用範囲と状態

- 対象HEAD: `900685a4d719bccc312ff1f037db1ce1bd1a5085`
- 対象PLAN: `AR-11-25`〜`AR-11-30`、`AR-11-33`〜`AR-11-40`、`AR-10.8-03`〜`AR-10.8-05`
- このartifactの目的は、所有者、非所有者、境界、寿命、停止順、依存方向、配布形態、検証入口を一つのhandoffへ集約することです。
- `source-verified` は現行sourceまたは既存の実行証拠で対応を確認した状態です。
- `documented` は責務または検証手順を記録しただけの状態です。実行証拠を意味しません。
- `pending-test` は、この文書に対応するarchitecture／fault／dependency testが未取得の状態です。
- `external-pending` は、clean worktree、実機、実browser、Release tag、owner decisionなど、このworktreeだけでは成立しない状態です。
- 下表の `documented`、`pending-test`、`external-pending` は、対応するPLAN checkboxを完了扱いにしません。

## 2. 境界の要約

```text
browser / Tauri WebView
        |
        | REST / WebSocket / WebRTC
        v
Rust application process
  composition root / HTTP server
  ApplicationBackend / StateHub
  controller arbiter
  CameraManager / SerialManager
  settings / profile / command services
        |
        | bounded worker IPC
        +-----------------------------+
        v                             v
script worker                 dynamic worker
Python command execution      Python or Lua dynamic execution
```

Rust processがhardware resourceと共有状態を所有し、frontend、Python、Luaは閉じた境界から接続するという構成は、[`ARCHITECTURE.md:11-39`](ARCHITECTURE.md#設計上の中心を理解する) と [`ARCHITECTURE.md:43-59`](ARCHITECTURE.md#process-topologyを把握する) に記載されています。

## 3. Ownership table

「owner」は正準状態、native handle、停止責任を保持するmoduleまたはprocessです。「非owner」は、そのresourceを直接保持せず、公開契約を通じて要求する側です。

| resource／責務 | owner | ownerではないもの | 現行source／根拠 | 状態 |
| --- | --- | --- | --- | --- |
| controllerの正準入力とrelease | `device::input::InputArbiter`、`ApplicationBackend` | browser、Tauri、script worker、dynamic worker | [`ARCHITECTURE.md:154-170`](ARCHITECTURE.md#controller入力の所有権を仲裁する)、`rust/pokecon/src/application_backend.rs:160-177` | source-verified |
| camera native handle、capture、最新frame | `camera::manager::CameraManager` | media consumer、WebRTC、MJPEG、script | [`ARCHITECTURE.md:190-204`](ARCHITECTURE.md#camera-frameのlifetimeを守る)、`rust/pokecon/src/production.rs:250-271` | source-verified |
| camera shared mappingの停止後lifetime | `CameraManager` とproduction shutdown fallback | writerをjoinできなかった呼出側による早期unmap | [`ARCHITECTURE.md:254-277`](ARCHITECTURE.md#shutdownを一つの順序へ収束させる)、`rust/pokecon/src/production.rs:394-426` | source-verified、fault test pending |
| serial port、selector、baud、codec状態 | `device::serial::manager::SerialManager` | controller arbiter、server、worker、frontend | [`ARCHITECTURE.md:172-188`](ARCHITECTURE.md#serial-managerの安全性を保つ)、`rust/pokecon/src/production.rs:273-282` | source-verified |
| visible settings／runtime stateのrevision | `server::state::StateHub` と `ApplicationBackend` | server handler、frontend store、device thread、worker callback | [`ARCHITECTURE.md:134-152`](ARCHITECTURE.md#visible-stateを一貫させる)、`rust/pokecon/src/production.rs:290-310` | source-verified |
| HTTP socket、REST／WebSocket router | composition rootの`entrypoint`／`production`とserver transport | `ApplicationBackend`以外のconsumer、worker、frontend | `rust/pokecon/src/lib.rs:45-75`、`rust/pokecon/src/production.rs:340-354`、[`ARCHITECTURE.md:238-252`](ARCHITECTURE.md#server境界を守る) | source-verified |
| user-script worker processとgeneration | `worker::supervisor::WorkerSupervisor`、`worker::generation::GenerationManager` | script code、profile service、dynamic worker | `rust/pokecon/src/worker/supervisor.rs:25-33`、`rust/pokecon/src/worker/generation.rs:21-84`、[`ARCHITECTURE.md:206-220`](ARCHITECTURE.md#user-script-workerを世代として扱う) | source-verified |
| dynamic Python／Lua generation | `dynamic::transaction`、dynamic worker、Rust側dynamic host | device manager、frontend、script worker | [`ARCHITECTURE.md:222-236`](ARCHITECTURE.md#動的設定をtransaction-generationとして扱う)、`rust/pokecon/src/production.rs:318-350` | source-verified |
| process-wide shutdown reasonとcancellation | `runtime::shutdown::ShutdownCoordinator` | 個別serviceが独自のprocess shutdown系列を作ること | `rust/pokecon/src/runtime/shutdown.rs:21-105`、[`ARCHITECTURE.md:254-277`](ARCHITECTURE.md#shutdownを一つの順序へ収束させる) | source-verified |
| 正準設定registryと生成contract | `settings`／`contracts`／`registry/` | frontend生成物、手書きwire型、workerが持つ複製registry | [`ARCHITECTURE.md:95-112`](ARCHITECTURE.md#正準契約と生成物を区別する)、`rust/pokecon/src/lib.rs:6-12, 36-40` | source-verified |
| compatibility baselineと候補promotion | compatibility toolと固定manifest／append-only promotion chain | 実行時runtime、候補側からbaselineを直接変更する処理 | `PLAN.md:204`、`flake.nix:428-472` | source-verified、promotion外部運用 pending |
| build／package／release artifact | Nix flake task、package／release workflow | runtimeが配布manifestを再定義すること | `Cargo.toml:1-8`、`flake.nix:1-9, 637-650`、`docs/TRACEABILITY_INDEX.md:77-102` | source-verified、Release tag pending |

### 3.1 Module ownership manifest

| module／process | 所有する責務 | 所有しない責務 | 現行source |
| --- | --- | --- | --- |
| `entrypoint`／`production` | composition、startup／shutdown順、service接続、router合成 | device wireの重複実装、frontendのcanonical state | `rust/pokecon/src/lib.rs:45-75`、`rust/pokecon/src/production.rs:142-143,297-384` |
| `application_backend` | StateHubとresource serviceのprojection、mutation gate、command／profile service接続 | TCP listener、worker native object、frontend store | `rust/pokecon/src/application_backend.rs:120-154,180-273`、`rust/pokecon/src/production.rs:297-350` |
| `server` | REST／WebSocket／WebRTCのwire、security、公開schema | camera／serial native handle、Rust private object | `rust/pokecon/src/lib.rs:36-38`、`rust/pokecon/src/server/rest/mod.rs:35-48`、[`ARCHITECTURE.md:238-252`](ARCHITECTURE.md#server境界を守る) |
| `device::input` | `InputArbiter`、source generation、release／neutral投影 | UI接続、worker process、serial port | [`ARCHITECTURE.md:154-170`](ARCHITECTURE.md#controller入力の所有権を仲裁する) |
| `device::serial` | native port、selector、codec、接続／切断／retry | WebSocket state、camera frame、script object | [`ARCHITECTURE.md:172-188`](ARCHITECTURE.md#serial-managerの安全性を保つ) |
| `camera` | capture session、latest frame、shared ring、writer shutdown | media transport、frontend、worker handle | [`ARCHITECTURE.md:190-204`](ARCHITECTURE.md#camera-frameのlifetimeを守る) |
| `worker::supervisor`／`worker::generation` | child process、IPC、generation phase、stop／reap | camera／serial/socket native handle | `rust/pokecon/src/worker/supervisor.rs:15-22`、`rust/pokecon/src/worker/generation.rs:21-84` |
| `worker_binary` | child側Python／Lua runtime、IPC protocol、child task cleanup | parent側のhardware resource、server listener | [`ARCHITECTURE.md:33-39`](ARCHITECTURE.md#process-topologyを把握する) |
| `dynamic::transaction`／dynamic host | stage→commit、callback／option／command generation | device handle、process-wide shutdown ownership | [`ARCHITECTURE.md:222-236`](ARCHITECTURE.md#動的設定をtransaction-generationとして扱う) |
| `settings`／`contracts`／`registry` | canonical settings／protocol／compatibility inputと生成 | generated frontend outputの直接編集、runtime device state | [`ARCHITECTURE.md:95-112`](ARCHITECTURE.md#正準契約と生成物を区別する)、`rust/pokecon/src/lib.rs:6-12,36-40` |
| `runtime::shutdown` | first-writer-wins reason、process cancellation | 各resourceの個別解放順を独自に作ること | `rust/pokecon/src/runtime/shutdown.rs:43-105` |
| frontend／generated client | server projectionの表示と公開contract利用 | canonical state、hardware handle、Rust private module | [`ARCHITECTURE.md:79-93`](ARCHITECTURE.md#frontendの責務を限定する) |

このmanifestはcrate内moduleの責務を記録するもので、公開Rust library APIを追加しません。`rust/pokecon/src/lib.rs:3-43`のprivate module declaration、`Cargo.toml:1-8`の単一package境界、既存`contract_sync`のdrift検査を同時に参照します。

### 3.2 重複所有検査の境界

この表で同じ正準resourceに複数ownerを割り当てていません。実装上の重複所有を完了扱いにするには、次の検査を追加して実行します。


1. `ApplicationBackend`以外がvisible revisionをcommitしていないことをsource checkで確認する。
2. `CameraManager`以外がcamera native handleまたはshared mappingの解放を行わないことをsource checkで確認する。
3. `SerialManager`以外がnative serial portを保持しないことをsource checkで確認する。
4. frontend／workerがRust private module、native handle、Python objectを境界越しに受け取らないことをcontract／IPC testで確認する。
5. それぞれを`nix run .#contract-check`または専用architecture testの出力へ保存する。

**判定:** 表は`documented`／source-verifiedですが、上記の重複所有検査reportは`pending-test`です。`AR-11-25`と`AR-10.8-03`は未完了のままです。

## 4. Processとstate transition

### 4.1 Rust application process

| state | entry condition | 許可される操作 | exit／最終状態 | 根拠 |
| --- | --- | --- | --- | --- |
| `starting` | CLIがcomposition rootを開始 | bootstrap、設定読込、resource初期化 | readyまたはfatal shutdown | `rust/pokecon/src/lib.rs:113-150`、`rust/pokecon/src/production.rs:245-310` |
| `running` | router、backend、resource、background taskが構成済み | 公開API、device投影、worker IPC | shutdown request | `rust/pokecon/src/production.rs:311-384` |
| `shutdown-pre` | `ShutdownCoordinator`が最初のreasonを受理 | `AppShutdownPre`、新規dynamic mutationの拒否 | `stopping` | [`ARCHITECTURE.md:254-267`](ARCHITECTURE.md#shutdownを一つの順序へ収束させる) |
| `stopping` | task、input、camera、script、dynamic、serialを順に停止 | bounded cleanup、diagnostic | `stopped`またはprocess exit | `rust/pokecon/src/production.rs:394-445` |
| `stopped` | serverとresourceの停止系列が完了 | 残存診断のみ | process exit | `rust/pokecon/src/runtime/shutdown.rs:61-99` |

### 4.2 worker generation

| state | script worker | dynamic worker | ルール |
| --- | --- | --- | --- |
| `absent` | commandが必要になるまで未起動 | dynamic設定無効時 | Rust processがresource ownerのまま |
| `running` | active profileのcommand generationを実行 | callback、option、command表示を提供 | `GenerationPhase::Running`、bounded IPC |
| `stopping` | profile switch／reload／shutdownでmutationを拒否 | `AppShutdownPre`後に新規mutationを拒否 | `GenerationPhase::Stopping`、cancellation token |
| `stopped` | processをreapしてから次generationを作成 | 通常動作中の再生成を許可しない | `GenerationPhase::Stopped`、`DynamicRestartForbidden` |

根拠は `rust/pokecon/src/worker/generation.rs:21-84` と [`ARCHITECTURE.md:206-236`](ARCHITECTURE.md#user-script-workerを世代として扱う) です。`rust/pokecon/tests/lifecycle.rs:567-602`のdynamic worker非再生成、`:605-635`のscript worker replacement、`:636-716`のscript worker停止中request gate、`:720-800`のdynamic worker停止中request gateは、`ManagedWorker::request`の停止中mutation拒否、許可classがIPCへ到達すること、reap後の全class拒否を実processで検証します。これはRust mainを含む全state table、production sequence、fault transition matrix、production shutdown fault matrixの受入証拠ではありません。

### 4.3 camera writerとserver

- camera writerは通常稼働からstop requestへ進み、deadline内joinならmappingを解放可能です。
- deadline超過時は`camera_writer_unstopped`をdurable stateとして保持し、`published_token`、slot state、mappingをprocess exitまで保持します。
- POSIXでは既存mappingを保持したまま共有memory nameだけをunlinkし、Windowsでは別unlinkを行いません。
- HTTP serverはboundからgraceful stoppingへ進み、deadline超過時は残存taskをcancelしてprocess exitへ進みます。

根拠は [`ARCHITECTURE.md:254-277`](ARCHITECTURE.md#shutdownを一つの順序へ収束させる) と `rust/pokecon/src/production.rs:394-465` です。camera／serverのdeadline fault reportは`pending-test`です。

## 5. Public boundary table

| caller | 許可する公開境界 | 返す型／契約 | 越えてはならないもの | 根拠 |
| --- | --- | --- | --- | --- |
| Web／Tauri frontend | REST、WebSocket、WebRTC media／signaling | OpenAPI／HTTP envelope、revision付きsnapshot／patch、media contract | Rust module、native handle、`StateHub`内部型 | `rust/pokecon/src/lib.rs:78-121`、[`ARCHITECTURE.md:79-93`](ARCHITECTURE.md#frontendの責務を限定する) |
| user script | bounded script host、command／profile service、controller／camera／serialの公開操作 | script API、typed event／result、safe diagnostic | serial port、camera handle、server socket | [`ARCHITECTURE.md:11-21`](ARCHITECTURE.md#設計上の中心を理解する)、`rust/pokecon/src/production.rs:318-350` |
| dynamic Python／Lua | staged evaluationとdynamic protocol | event、option、command display、callback result | device handle、Rust object、任意のinternal service | [`ARCHITECTURE.md:222-236`](ARCHITECTURE.md#動的設定をtransaction-generationとして扱う) |
| worker supervisor | bounded IPC、generation gate、fault／stop protocol | `IpcValue`、`DisconnectReason`、worker lifecycle messages | Python／Lua object、Rust `Arc`、native handle | `rust/pokecon/src/worker/supervisor.rs:15-22`、`rust/pokecon/src/worker/generation.rs:50-84` |
| compatibility tool | fixed baseline、candidate、result、append-only promotion record | compatibility report schema | runtime settings、current baselineの直接書換え | `PLAN.md:204`、`flake.nix:452-472` |

### 5.1 公開面inventory

| 公開面 | 現行path／型 | 構成source | 受入上の境界 |
| --- | --- | --- | --- |
| REST | `/api/commands/control`、`/api/commands/reload`、device／settings／state／notifications／dynamic-config／script-ui／profiles／updateの各route family | `rust/pokecon/src/server/rest/mod.rs:35-48,197`、`rust/pokecon/src/server/rest/{commands,devices,settings,state,notifications,dynamic_config,script_ui,profiles,update}.rs` | `/api`はstatic fallbackより前で閉じ、wire型はserver APIへ集約 |
| WebSocket | `/ws` upgrade、state／control／ephemeral queue | `rust/pokecon/src/server/websocket.rs:402-411`、`rust/pokecon/src/server/backend.rs:149-193` | Origin、request marker、heartbeat、bounded queueをtransportが所有 |
| WebRTC／fallback | `POKECON-CONTROL`、`POKECON-LOG`、`RealtimeRoute::{Connecting,WebRtc,WebSocketFallback}` | `rust/pokecon/src/server/webrtc.rs:47-49`、`rust/pokecon/src/server/realtime.rs:477-697` | media transportはcamera native handleを所有しない |
| OpenAPI／TypeScript | `api/openapi.json`、`web/src/lib/api/openapi.json`、generated TypeScript | `rust/pokecon/src/server/openapi.rs:32-33,168,225`、`rust/pokecon/src/bin/generate_openapi.rs` | Rust wire型、OpenAPI、frontend生成物のdriftを`contract_sync`で検査 |
| settings／dynamic schema | `generated/settings.schema.json`、`settings-ui.json`、protocol registry、Python／Lua typing | `rust/pokecon/registry/settings.json`、`rust/pokecon/registry/protocol.json`、`rust/pokecon/src/contracts/dynamic_typings.rs:9-38` | 正準registryを直接編集し、生成物を直接編集しない |
| CLI／mode | `pokecon --ui web&#124;desktop`、`pokecon-worker --kind script&#124;dynamic`、compatibility／generator bin | `rust/pokecon/Cargo.toml:13-52`、`rust/pokecon/src/entrypoint.rs:37-65`、`rust/pokecon/src/lib.rs:78-121` | worker／generator binは公開library APIにしない |
| worker IPC | typed MessagePack `IpcValue`、script／dynamic protocol | `rust/pokecon/src/worker/ipc/mod.rs:1-13`、`rust/pokecon/src/worker/ipc/schema.rs:16-64`、`rust/pokecon/src/worker/script/protocol.rs`、`rust/pokecon/src/dynamic/protocol.rs` | native object、non-string key、任意Rust objectをpayloadへ入れない |

このinventoryはsupported user-visible surfaceと、内部で接続するtyped boundaryを同じ表へ置きます。`server`内の`pub mod`はcrate外公開APIを意味せず、`rust/pokecon/src/lib.rs:38`の`mod server;`がcrate-private boundaryです。

### 5.2 公開schemaとnative object境界

現行の公開契約は、設定registry、protocol registry、server API、worker IPCの4系統です。生成物は正準入力から再生成し、生成物を直接編集しません（[`ARCHITECTURE.md:95-112`](ARCHITECTURE.md#正準契約と生成物を区別する)）。

`contract_sync`に次のnegative assertionを追加するまで、`AR-11-27`は完了扱いにしません。

- public APIの型にcamera／serial native handleが出ない。
- IPC payloadにPython／Lua objectまたはRust private objectが出ない。
- frontend generated typeのsourceがRust private module pathへ依存しない。
- dynamic callbackの失敗がdevice threadの所有権またはprocess shutdownを直接奪わない。

## 6. Settings、profile、command、compatibility corpusのlifecycle

| 対象 | 生成／読込 | 切替／適用 | 失敗時 | 破棄／保持 | 根拠 |
| --- | --- | --- | --- | --- | --- |
| settings | default、global TOML、profile TOML、環境変数、dynamic、明示CLIをregistryで解決 | runtime-immediate fieldは一つのtransaction、一つのvisible revisionへcommit | validation／apply failureを`apply_failures`へ残し、visible successと混同しない | startup-only／bootstrap-onlyはrunningへ遡及適用しない | [`ARCHITECTURE.md:114-132`](ARCHITECTURE.md#設定pipelineを追う) |
| profile | active profileの設定とcommand rootを解決 | 旧script generationを停止・cleanup・reapしてから新generation | 旧generationの状態を破壊せず、切替失敗を診断へ残す | active generationはprofile switchで破棄、Rust resourceは保持 | [`ARCHITECTURE.md:206-220`](ARCHITECTURE.md#user-script-workerを世代として扱う)、`rust/pokecon/src/production.rs:327-350` |
| command | `Data/Commands`をdiscoveryし、metadata取得後にexecutionへ進む | active generationのcommand serviceへ反映 | import／execution／IPC failureはgenerationを停止または拒否 | 旧generationのprocessとqueueをreap／drop | [`ARCHITECTURE.md:206-220`](ARCHITECTURE.md#user-script-workerを世代として扱う) |
| dynamic config | stage環境でPython／Lua sourceを評価 | 成功時だけcallback、option、command表示を新generationへcommit | syntax、cycle、callback登録 failureでは現行generationを維持 | 旧generationを置換後に停止、device resourceは保持 | [`ARCHITECTURE.md:222-236`](ARCHITECTURE.md#動的設定をtransaction-generationとして扱う) |
| compatibility corpus | 固定baseline、候補、full-chain結果をmanifestで読む | 候補監視→full-chain再評価→成功時のみappend-only promotion | failure時は従前baselineと保証を変更しない | baselineは上書きせず、結果とpromotion recordを追記 | `PLAN.md:204`、`flake.nix:428-472`、`docs/TRACEABILITY_INDEX.md:96-102` |

この表の生成、切替、失敗、破棄の実行reportは、`nix run .#contract-check`と`nix run .#compatibility`から同じartifactへ保存する必要があります。現在のlocal successだけでは、`AR-11-28`のcheckpoint別受入を完了扱いにしません。

## 7. Fault、timeout、rollback、shutdownの責任

| fault／操作 | 一次責任 | 最終状態／安全動作 | 実装根拠 | 受入状態 |
| --- | --- | --- | --- | --- |
| controller source切断 | input arbiter／ApplicationBackend | source保持入力をreleaseし、neutralへ収束 | [`ARCHITECTURE.md:154-170`](ARCHITECTURE.md#controller入力の所有権を仲裁する) | source-verified、fault matrix pending |
| serial write／disconnect failure | SerialManager | delta stateを成功write前にcommitせず、neutralを優先 | [`ARCHITECTURE.md:172-188`](ARCHITECTURE.md#serial-managerの安全性を保つ) | source-verified |
| camera writer timeout | CameraManager／production shutdown | `camera_writer_unstopped`を記録し、mappingを保持。POSIX name-only unlink、Windows unlinkなし | `rust/pokecon/src/production.rs:394-465`、[`ARCHITECTURE.md:190-204`](ARCHITECTURE.md#camera-frameのlifetimeを守る) | source-verified、外部／fault evidence pending |
| script callback／worker timeout | worker supervisor／command service | bounded timeout後にgenerationをstoppingへ進め、Rust shutdownを無期限停止させない | `rust/pokecon/src/worker/supervisor.rs:15-22`、[`ARCHITECTURE.md:206-220`](ARCHITECTURE.md#user-script-workerを世代として扱う) | source-verified、停止中request gateとshutdown_allのmulti-worker forced stop／reap／neutralization narrow testあり、worker timeout／shutdownのfull fault matrix pending |
| dynamic source／callback failure | dynamic transaction／dynamic worker | 現行generationを維持し、新generationへ切り替えない | [`ARCHITECTURE.md:222-236`](ARCHITECTURE.md#動的設定をtransaction-generationとして扱う) | source-verified、fault test pending |
| settings apply failure | SettingsService／applier | persistence、service適用、visible commitの成否を分離して報告し、失敗fieldを保持 | [`ARCHITECTURE.md:114-132`](ARCHITECTURE.md#設定pipelineを追う) | source-verified、contract report pending |
| first shutdown request | ShutdownCoordinator | first-writer-wins reason、全依存taskへcancellationを配布 | `rust/pokecon/src/runtime/shutdown.rs:43-105` | source-verified |
| shutdown step failure | production shutdown coordinator | 一つのservice failureで後続neutral化、serial停止、server停止を省略しない | `rust/pokecon/src/production.rs:394-445`、[`ARCHITECTURE.md:254-277`](ARCHITECTURE.md#shutdownを一つの順序へ収束させる) | source-verified、full fault test pending |
| compatibility candidate failure | compatibility runner／promotion gate | fixed baselineと従前保証を保持し、promotion recordを追加しない | `PLAN.md:204`、`flake.nix:452-472` | source-verified、external promotion evidence pending |

## 8. Distribution and artifact matrix

| OS／実行形態 | build／runtime入口 | 成果物／境界 | 現在の証拠 | 残る証拠 |
| --- | --- | --- | --- | --- |
| Linux／Nix開発・gate | `flake.nix`のNix app、`nix run .#...` | Rust package、worker、生成contract、check report | `Cargo.toml:1-8`、`docs/TRACEABILITY_INDEX.md:77-102`、local gate success | clean detached worktreeを含むfresh checkpoint |
| Linux／Debian package | Package workflow／Nix release input | Debian bundle、runtime／wheelhouse、clean-install、reproducibility manifest | Package CI `36164052702` attempt 2のsuccessを`PLAN.md`とtraceabilityへ記録済み | Release tag起点のpublication |
| Windows／NSIS | Tauri／NSIS package workflow、PE normalization | NSIS installer、payload／expanded-tree／outer installer manifest、clean install | Package CI `36164052702` attempt 2のWindows bundle／reproducibility success | Release tag起点のpublication、desktop probeの恒常的flakiness非存在 |
| Web mode | Rust applicationのWeb UI router | static SPA、REST、WebSocket、WebRTC | `rust/pokecon/src/lib.rs:78-121`、[`ARCHITECTURE.md:79-93`](ARCHITECTURE.md#frontendの責務を限定する) | 外部browser WebRTC／MJPEG fallback受入 |
| Tauri desktop | 同じRust backend + Tauri lifecycle | desktop shell、NSIS／platform bundle | `rust/pokecon/src/lib.rs:78-121`、`rust/pokecon/tauri.conf.json`、Package CI | 外部desktop probeの追加安定性証拠 |
| non-Nix host toolchain | 製品の正規build入口ではない | ambient toolchainによる結果を配布証拠にしない | `PLAN.md:161-181`のNix／direnv契約 | clean worktreeでのnegative report／owner判断 |
| Release／tag | `release.yml`とtag起点の外部操作 | signed release、publication、利用者向けartifact | `docs/TRACEABILITY_INDEX.md:84-102,114` | ownerによるtag／Release操作。明示指示なしに実施しない |

Package CIのsuccessはPackage受入の証拠ですが、Release tag、署名runnerの運用証跡、外部browser、実機受入を代用しません。

## 9. Dependency rule manifest

### 9.1 許可する方向

| source | allowed dependency | 理由／根拠 |
| --- | --- | --- |
| `entrypoint`／`production` | backend、device、camera、settings、server、worker supervisor | composition rootが生成順、停止順、resource ownerを統合する。`rust/pokecon/src/lib.rs:1-75`、`rust/pokecon/src/production.rs:245-384` |
| `ApplicationBackend` | StateHub、settings、host、camera、serial、service | visible projectionとresource serviceを一元化する。`rust/pokecon/src/production.rs:297-350` |
| server transport | public backend trait／wire型 | HTTP／WebSocketを公開境界へ限定する。[`ARCHITECTURE.md:238-252`](ARCHITECTURE.md#server境界を守る) |
| worker supervisor | IPC、generation、worker binary | process境界を所有し、bounded stopを適用する。`rust/pokecon/src/worker/supervisor.rs:15-22` |
| frontend／script／dynamic | generated／typed public contract | private Rust objectとnative handleを直接参照しない。`rust/pokecon/src/lib.rs:3-45`、[`ARCHITECTURE.md:79-112`](ARCHITECTURE.md#frontendの責務を限定する) |
| compatibility／release tool | fixed manifest、candidate、report、package source boundary | runtimeの正準状態を逆流させない。`flake.nix:428-472,637-650` |

### 9.2 禁止する方向

| forbidden edge | 禁止理由 | 検査入口 | 状態 |
| --- | --- | --- | --- |
| frontend → `rust/pokecon/src/*` private module／native handle | 公開wire契約を迂回する | generated type／source filter／contract test | pending-test |
| worker → camera／serial/server native handle | process境界と停止安全性を壊す | IPC schema／source check | pending-test |
| lower module → composition root | ownerと依存方向が逆流する | `lib.rs` module visibility／Cargo/source check | documented |
| generated artifact → canonical input | 再生成で失われ、runtimeと型がずれる | generator／drift test | source-verified |
| candidate promotion → fixed baseline overwrite | append-only compatibility保証を壊す | `compatibility` report／negative test | pending-test |
| UI／transport → hardware serviceの重複状態 | StateHubとresource ownerが分裂する | architecture test／review | pending-test |

workspaceは`pokecon`一packageで、Rust moduleは`rust/pokecon/src/lib.rs:3-43`のprivate declarationが基本です。これは依存方向の設計根拠ですが、上表の全forbidden edgeを自動検査するmanifestとfixtureはまだありません。

## 10. Migration inventoryと検証command

### 10.1 現行inventory

| 移行対象 | 現在の配置／状態 | 検証 |
| --- | --- | --- |
| Cargo workspace／package | workspace memberは`rust/pokecon`一件、package nameは`pokecon` | `Cargo.toml:1-8`、`nix run .#cargo -- metadata --locked --no-deps --format-version 1` |
| worker executable | `pokecon-worker`のscript／dynamic、fault fixture bin | `rust/pokecon/Cargo.toml:19-38`、`nix run .#cargo-test` |
| contract generator | `generate_contracts`、`generate_openapi` | `rust/pokecon/Cargo.toml:40-52`、`nix run .#contract-check` |
| frontend／desktop input | `web/`、Tauri config、Nix product/release source boundary | `rust/pokecon/src/lib.rs:78-121`、`flake.nix:637-650` |
| compatibility tool | fixed manifest、candidate、results、promotion log | `flake.nix:428-472`、`PLAN.md:204` |
| 旧package／旧compatibility入口 | 現行の正規workspace memberではない | `nix run .#cargo -- metadata --locked --no-deps --format-version 1` と許可済み`git grep` |

### 10.2 Phase別verification command表

| phase／目的 | command | 期待結果 | 現行の扱い |
| --- | --- | --- | --- |
| structure／contract | `nix run .#contract-check` | generated contract、public schema、source boundaryの差分なし | local successは取得済み、checkpoint別logは要整理 |
| Rust test／lifecycle | `nix run .#cargo -- test --locked -p pokecon --features 'integration-test-support worker-binary worker-test-fixture' --test lifecycle` | Rust lifecycle／fault test success | 7 passed。停止中request gateの狭い証拠は取得済み、全handoff fault testは未追加 |
| compatibility | `nix run .#compatibility` | fixed baseline不変、report SHA、promotion条件のfail-closed判定 | local reportは`PLAN.md:204`に記録、Release promotionは未成立 |
| aggregate check | `nix run .#check` | production、Web、Python、Rust、mutation、quality gate success | current local successは取得済み |
| release/package | `nix run .#release-check`、Package CI | artifact manifest、clean install、再現性 | local release checkとPackage CI success、Release tagは未成立 |
| workflow syntax | `nix run .#actionlint` | workflow syntax／input contract success | current local successは取得済み |
| clean migration | clean detached worktreeで上記全command | ambient toolchain、未追跡生成物、cache poisonの影響なし | owner許可が必要な項目が残る |

commandの成功だけで異なるcommit、clean worktree、外部Release、実機／実browserを同一視しません。checkpoint別の実行logとartifact pathを保存することが受入条件です。

## 11. PLAN対応表と最小の次作業

| PLAN item | このartifactで記録したもの | 未成立の受入証拠 | 最小の次作業 |
| --- | --- | --- | --- |
| `AR-11-25` | ownership table、非owner、重複検査の規則 | 重複所有source／negative testのreport | `contract_sync`にresource owner一意性とnative handle漏洩の検査を追加しNix実行 |
| `AR-11-26` | Rust／script／dynamic／camera／serverのstate table、`rust/pokecon/tests/lifecycle.rs:567-602`のdynamic worker非再生成、`:605-635`のscript replacement、`:636-716`／`:720-800`のscript／dynamic停止中request gate | Rust mainを含むstate table全体、production sequence、fault transition matrix、production shutdown fault matrix | `ShutdownCoordinator`／`shutdown_all`／production sequenceを含む残りのstate／fault caseを追加 |
| `AR-11-27` | UI／HTTP／IPC／script／dynamicのpublic boundary | native object境界越え禁止test、schema report | public schemaを列挙するcontract testとnegative fixtureを追加 |
| `AR-11-28` | settings／profile／command／dynamic／compatibility lifecycle | `contract-check`／`compatibility`の同一artifact report | lifecycle状態表を生成reportへ接続し、failure時baseline保持を実行検証 |
| `AR-11-29` | `rust/pokecon/tests/lifecycle.rs:802-855`の`shutdown_all` narrow test（Script／Dynamicのforced stop／reap／neutral化） | full shutdown fault matrix、camera fallback、signal path、production resource release test | `lifecycle`へ各timeout後の最終状態と後続停止を追加 |
| `AR-11-30` | OS／build／package／runtime matrix | Release tag／publication、外部browser／clean machine | Releaseはowner指示後、外部受入は別packetで取得 |
| `AR-11-33` | module ownership manifest相当の表 | machine-readable manifestとarchitecture test | 本表をmanifestへ固定し、source symbol存在を検査 |
| `AR-11-34` | negative responsibilityとforbidden edge表 | forbidden ownership test | native handle、socket、visible revisionの禁止参照をfixture付きで検査 |
| `AR-11-35` | allowed dependency direction表 | Cargo／source dependency check | module import inventoryを生成し許可edgeとの差分をfail-closed化 |
| `AR-11-36` | forbidden dependency direction表 | forbidden-edge fixture付きcheck | 意図的逆流fixtureを一件追加し、checkがrejectすることを確認 |
| `AR-11-37` | process／module deployment boundary | IPC boundary test | `cross_process`へpublic payload以外を拒否するfixtureを追加 |
| `AR-11-38` | artifact／OS distribution matrix | OS別clean-install report | existing Package artifactをmanifestへread-backし、Release待ちを分離 |
| `AR-11-39` | current package／binary／generator／compatibility inventory | phase別inventory、許可済みgit grep log | phase ID別にinventory snapshotを保存 |
| `AR-11-40` | command／expected result matrix | 各phaseの実行log | owner許可済みclean worktreeでphase commandを実行 |
| `AR-10.8-03` | ownership／single-state／change-pathの整理 | duplicate state inspection | `AR-11-25`のarchitecture testと同じreportへ接続 |
| `AR-10.8-04` | process／trust／platform／distribution boundaryの根拠 | 必要性／信頼／配布根拠表のreview | 各強い境界に一行の必要性根拠と実装証拠を追加 |
| `AR-10.8-05` | 追加抽象化を無条件に認めないdependency rule | abstraction inventoryとcrate数検査 | current `Cargo.toml` manifestをbaseline化し、追加時の根拠検査を追加 |

## 12. 完了判定

このartifactの追加だけでは、上表のPLAN itemを完了扱いにしません。現在完了扱いにできるのは、source／既存実行証拠で明示した行だけです。特に次の項目は未完了として保持します。

- clean detached worktree、phase checkpoint別の全gate、owner decision。
- production camera／serial性能、fault matrixの未取得test。
- 外部browser、実機camera／serial／MCU、Release tag／publication。
- machine-readable ownership／negative responsibility／dependency manifestと対応test。
- Sol review、PR merge、tag作成。これらは明示指示なしに実施しません。

この文書には認証情報、token、password、任意code本文、受信payloadを記録していません。外部証跡を追加する場合も値は保存せず、必要なら`[REDACTED]`で扱います。
