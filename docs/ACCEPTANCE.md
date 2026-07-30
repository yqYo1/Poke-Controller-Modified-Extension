# 外部環境で受入判定を記録する

この文書は、release candidateを実機、実browser、clean installationで判定するrelease担当者と本体開発者向けです。

一般利用者の動作確認手順ではありません。

CI fixtureだけでは証明できないUSB、MCU firmware、対象console、物理camera、browser、性能、desktop lifecycleを同じ形式で記録します。

未実施のgateを成功として記録しません。

## CI、仮想I/O、外部受入を分ける

**CI gate**は正準contract、unit test、integration fixture、build、lintを決定的に検査します。

**仮想I/O gate**はLinux kernelのPTYとV4L2 loopbackを使い、productionのnative OS APIまでを検査します。

**外部受入gate**はphysical device、対象console、実browser、clean machineを使用します。

三つのgateは検出するfailureが異なります。

CIや仮想I/Oの成功を、外部受入の成功へ読み替えません。

実機failureを回避するCI専用backendやproduction外のprotocolで合格させません。

## 対象commitとartifactを固定する

受入開始前に次を満たします。

1. 対象source commitが署名済みで、作業branchの必須CIがすべて成功していることを確認します。
2. 同じcommitからLinux package、Windows installer、またはNix closureを作ります。
3. 配布artifactのSHA-256またはNix store pathを記録します。
4. 試験machineへ旧versionのprocess、worker、Data rootが残っていないことを確認します。
5. 単調時計、既知frame、loopbackまたはMCU fixtureの時刻とversionを記録します。

LinuxとWindowsは同じsource commitから作ったartifactを使用します。

途中でcode、設定default、fixture、firmwareを変更した場合は新しい対象として最初から記録します。

## secretを証拠から除く

受入記録へtoken、webhook URL、broker credential、生のserial number、個人path、任意code本文を保存しません。

外部serviceの成功は、送信先が発行した秘匿化済みrequest IDで照合します。

device identityは同じdeviceを識別できるsalted hashまたは秘匿化したlabelへ変換します。

sourceの同一性は本文ではなくcommit、path、SHA-256で記録します。

screen captureやpacket captureにもsecretがないことを保存前に確認します。

## record schemaを使用する

記録は`artifacts/hardware/<platform>/<capability>/<UTC timestamp>.json`へ保存します。

形式の正本は[`acceptance-record.schema.json`](../rust/pokecon/registry/acceptance-record.schema.json)です。

[example](../rust/pokecon/registry/acceptance-record.example.json)は形式確認専用で、受入証拠ではありません。

一つのrecordは一つのplatformとcapabilityを表します。

`source_commit`には40桁の対象commitを記録します。

`build_identity`にはartifact hashまたはNix store pathを記録します。

各stepはこの文書に示すID、順序、結果を使用します。

schemaとsemantic ruleは次で検証します。

```bash
nix run .#acceptance-record-check -- /absolute/path/to/artifacts/hardware/linux/performance/2026-01-01T000000Z.json
```

同じsource commitに必要なrelease matrix全体は次で検証します。

```bash
nix run .#acceptance-record-check -- --release-candidate <40桁のsource commit> /absolute/path/to/artifacts/hardware
```

総合結果を`passed`にできるのは、必須stepを記載順にすべて含め、各stepが`passed`で、数値閾値を満たす場合だけです。

validatorはstepの欠落、追加、重複、順序違い、閾値違反、開始時刻以後でない完了時刻を拒否します。

`not_applicable`を含むrecordは、そのcapabilityの合格証拠に使用できません。

## 仮想I/Oを実機前に実行する

Linuxでは実機gateへ進む前に次を実行します。

```bash
nix run .#virtual-io-check
```

既存のV4L2 loopback indexを使う場合は引数で指定します。

```bash
nix run .#virtual-io-check -- 42
```

taskはPTY masterとslave間のpartial read、partial writeをproduction serial backendで検査します。

taskはffmpegの既知patternをV4L2へ入力し、列挙、format交渉、BGR decode、再設定をproduction camera backendで検査します。

実行kernelに対応する`v4l2loopback`と、必要時にmoduleをloadできるpasswordless `sudo`が必要です。

task自身がmoduleをloadした場合だけ終了時にunloadします。

PTYはUSB抜線、MCU firmware、対象consoleの認識、電気的noiseを再現しません。

V4L2 loopbackは物理camera固有format、driver、帯域、抜線を完全には再現しません。

## MCUとシリアル

LinuxとWindowsの両方で、対象consoleへ接続したMCUを使用します。

capabilityは`mcu_serial_device_and_target_console`です。

1. `protocol_frames`：native selectorの生値を選び、default、Qingpi、3DSの各test vectorを送信します。
2. `control_mapping`：button、左右stick、hat、touchを操作し、表現可能な入力がconsole側と一致することを確認します。
3. `reconnect_and_partial_write`：partial write、USB抜線、3秒間隔、20回上限、明示disconnectによる再試行取消しを確認します。
4. `neutral_shutdown`：WebSocket切断、script停止、profile切替、application終了後にneutral frameが到達することを確認します。
5. `selector_rollback`：設定変更失敗後に同じ旧selectorへ戻り、別deviceを暗黙選択しないことを確認します。

logic analyzerまたはMCU UART captureにはinitial neutral、操作frame、disconnect neutralを含めます。

default形式はASCIIとraw hexの両方を、binary形式はoffset付きhexとdecode結果を保存します。

formatが表現しない入力は成功と推測せず、制約どおりに欠落することを確認します。

wire byteと期待値は[周辺機器開発ガイド](PERIPHERAL_DEVELOPMENT.md)を正本とします。

## カメラ

LinuxではV4L2 device、Windowsではnative camera IDを使用します。

色、座標、timestampが既知のframe fixtureを撮影します。

capabilityは`capture_device_with_known_frame_fixture`です。

1. `known_frame_processing`：640×360、1280×720、1920×1080でBGR色順、crop、flip、template resultを確認します。
2. `resolution_pin_consistency`：resolution変更中に破損frameがなく、pin中frameの内容が変化しないことを確認します。
3. `screenshot_destinations`：PNG、JPEG、download、Captures、native path、衝突suffix、overwriteを確認します。
4. `media_fallback_recovery`：WebRTC、MJPEG fallback、主経路復旧が同じframe系列を表示することを確認します。
5. `device_loss_identity`：停止または抜線時に別deviceへ暗黙切替せず、再試行UIと診断IDが出ることを確認します。

fixture原本のhash、camera format、capture FPS、UI FPS、browserをrecordへ含めます。

画面の目視だけで色順と座標を判定せず、保存frameまたは数値sampleも残します。

## 音声

LinuxとWindowsで実audio input deviceを検証します。

capabilityは`audio_input_device`です。

1. `audio_device_selection`：native selectorの選択、再起動後の同一性、暗黙の代替deviceを選ばないことを確認します。
2. `audio_capture_processing`：既知音源を取得し、対象互換scriptが要求する処理結果を確認します。
3. `audio_disconnect_recovery`：device停止または抜線時の診断、再試行、同じselectorでの復旧を確認します。
4. `application_continuity`：音声失敗後もcommand、UI、shutdownが継続することを確認します。

既知音源のhash、sample rate、channel、device selectorを秘匿化して記録します。

## 外部通知

credential付きDiscord、MQTT、socketなどをLinuxとWindowsで検証します。

capabilityは`credentialed_network_notification_endpoints`です。

1. `notification_success`：受信側の秘匿化済みrequest IDで到達を確認します。
2. `notification_timeout`：到達不能または応答保留fixtureでbounded timeoutと診断を確認します。
3. `notification_rejection`：認証拒否またはprotocol拒否をsecret-safeなerrorとして確認します。
4. `application_continuity`：失敗後もcommand、UI、shutdownが継続することを確認します。

credentialと受信payloadはrecordや添付artifactへ保存しません。

外部serviceのrate limitや一時障害をPokeCon成功へ読み替えず、送信側と受信側の結果を分けます。

## browser matrix

Linux backendとWindows backendのそれぞれに対し、Chrome、Edge 94以上、Firefox 130以上、Safari 16.4以上の現行利用可能versionで確認します。

SafariはmacOS上のremote browser clientから対象backendへ接続できます。

browserごとに別の`browser_matrix` recordを作ります。

capabilityは`browser_matrix`です。

1. `keyboard_workflows`：6 main tab、Commandsの3 subtab、10 shortcut、start、pause、resume、stopをkeyboardだけで操作します。
2. `accessibility_layout`：focus順、ARIA名、200% zoom、OS high contrast、狭幅layoutを確認します。
3. `webrtc_channels`：WebRTC videoと2 DataChannelを確立し、入力generation切替を確認します。
4. `fallback_recovery`：主経路を故障させ、30秒後のMJPEGまたはWebSocket fallbackと、復旧後のWebRTC再昇格を確認します。
5. `reconnect_snapshot_recovery`：heartbeat timeout、3秒間隔、20回上限、手動再接続、revision gapからのsnapshot復旧を確認します。
6. `feature_workflows`：camera、serial、command、profile、設定競合、script dialog、Tk、overlayの主要操作を確認します。

browser version、backend platform、input device、zoom、accessibility modeをenvironmentへ記録します。

一つのChromium browser成功を別browserの成功へ流用しません。

## 性能gate

LinuxとWindowsのrelease buildを別々に測定します。

1920×1080既知frame、接続済みloopback controller、他の高負荷processを停止したclean machineを使用します。

60秒warm-up後に単調時計で各metricを300 sample以上取得します。

使用browserを`environment.browser`へ記録します。

capabilityは`performance`です。

1. `measurement_environment`：build、artifact hash、device、browser、clock、frame fixture、loopback経路を記録します。
2. `warmup`：60秒warm-upし、resolution、FPS、接続経路が安定したことを確認します。
3. `sample_collection`：次の5 metricを同じ条件で各300 sample以上取得します。
4. `threshold_evaluation`：raw sampleからp50、p95、maximumを再計算し、閾値と一致することを確認します。

| metric | 合格条件 |
|---|---|
| `webrtc_video_latency` | p95が100 ms未満 |
| `mjpeg_video_latency` | p95が150 ms以下 |
| `controller_input_latency` | p95が50 ms未満 |
| `ui_frame_rate` | p50が60 FPS以上 |
| `ui_input_latency` | p95が16 ms未満 |

映像遅延はfixtureの表示timestampからbrowser描画までを測ります。

controller入力遅延は入力受付からloopback frame観測までを測ります。

UI入力遅延はevent timestampから次のpaintまでを測ります。

`performance` recordには5 metricすべてを含めます。

debug build、異なるresolution、sample不足の結果を閾値判定へ使用しません。

## desktop lifecycle

LinuxとWindowsでdesktop bundleを使用します。

capabilityは`desktop_lifecycle`です。

1. `close_policies`：`ask`、`shutdown`、`keep_backend`、確認dialog、取消しを確認します。
2. `tray_and_single_instance`：trayからの復帰と既存instance検出を確認します。
3. `os_shutdown_and_repeated_close`：OS shutdownとclose連打が同じ停止系列へ収束することを確認します。
4. `hung_resource_shutdown`：worker hang、camera writer hang、接続中serverの停止順とdeadlineを確認します。
5. `neutralization_and_mapping_lifetime`：終了後のneutral入力と、writer停止失敗時にmappingを早期unmapしないことを確認します。

process ID、最初のshutdown reason、終了所要時間、残ったchild processを記録します。

windowが消えただけでprocess終了と判定しません。

## 統合負荷と長時間stress

LinuxとWindowsのrelease buildでproduction topologyを最低60分継続します。

1920×1080 camera、WebRTC browser、serial loopbackまたはMCU、Python command、動的Lua設定、操作中UIを同時に動かします。

capabilityは`integrated_load_stress`です。

1. `concurrent_topology`：Rust、Python、Lua、WebまたはTauri、camera、WebRTC、serialが同時稼働することを確認します。
2. `sustained_load`：camera処理、controller出力、script callback、UI操作を最低60分継続し、破損frame、入力滞留、無応答がないことを確認します。
3. `queue_pressure`：callback queueとIPC queueをworkload manifestの上限まで加圧し、優先度、bounded overflow、nonblocking契約を確認します。
4. `reconnect_and_profile_cycles`：WebSocket、WebRTC再接続とprofile切替を反復し、世代混在、旧worker再生成、snapshot欠落がないことを確認します。
5. `faulted_shutdown`：callback実行中、IPC滞留中、camera writerまたはworker故障中にshutdownし、neutral化とdeadline内停止を確認します。
6. `resource_leak_audit`：開始時と終了時のresource差分を比較し、task、mapping、socket、handle、descriptorが単調増加しないことを確認します。

workload manifest、event rate、cycle count、開始時と終了時のCPU、memory、handleまたはdescriptor数を保存します。

queue容量をproduction値から変更した場合は別条件として扱います。

一時的な無応答後に回復しても、stepの合格条件へ影響したfailureを隠しません。

## security acceptance

LinuxとWindowsでloopback clientと別hostのLAN clientを使用します。

capabilityは`security_acceptance`です。

1. `lan_trust_warning`：非loopback bindの保存時と再起動要求時に、認証なしの完全信頼とPythonまたはLua実行権限をUIが明示することを確認します。
2. `lan_dynamic_code_execution`：LAN clientからHost、Origin、Content-Type、固定header契約を満たして`load_content`、`load_path`、`reload`を実行でき、追加認証やpeer IP制限がないことを確認します。
3. `secret_non_disclosure`：設定snapshot、OpenAPI response、WebSocket、log、診断、manifest、debug表示に平文secretが現れないことを確認します。
4. `path_jail`：相対path traversalとsymlink escapeが拒否され、許可された明示的絶対pathだけが正準identityで処理されることを確認します。
5. `request_boundary_controls`：不正Host、Origin、Content-Type、固定header、WebSocket preflightが閉じて拒否され、SPA fallbackが未知APIを隠さないことを確認します。

任意code本文、secret、token、個人pathはevidenceへ保存せず、hash、redacted request ID、診断IDだけを記録します。

LAN code executionが成功することは現在の信頼modelの確認であり、認証があることを意味しません。

公開境界は[HTTP APIとリアルタイム通信](HTTP_API.md#共通request境界を満たす)と照合します。

## release candidateを判定する

同じsource commitについて、次のrecordが揃うまで外部platform gateは未完了です。

- LinuxとWindowsのMCUとserial。
- LinuxとWindowsのcamera。
- LinuxとWindowsのperformance。
- LinuxとWindowsのdesktop lifecycle。
- LinuxとWindowsのaudio。
- LinuxとWindowsのcredential付き外部service。
- Linux backendとWindows backendに対するChrome、Edge、Firefox、Safariのbrowser matrix。
- LinuxとWindowsのintegrated load stress。
- LinuxとWindowsのsecurity acceptance。
- schemaとsemantic matrix検証済みで総合`passed`のrecord。
- 各recordが参照する秘匿化済みevidence。

対象機能がreleaseに存在するのに`not_applicable`で省略しません。

record failureを既知問題として成功へ変更せず、修正後の新しいcommitと新しいrecordで再検証します。

すべてのrecordが揃った後に`acceptance-record-check --release-candidate`を実行し、validatorの成功をrelease evidenceへ保存します。
