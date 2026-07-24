# 外部受入ゲート

この文書は、CIの決定的fixtureだけでは証明できない実機、実ブラウザ、性能、デスクトップライフサイクルの検証手順と記録形式を定めます。対象commitごとに実行し、未実施のgateを成功として記録してはいけません。

## 共通前提

1. 対象commitがSSH署名済みで、作業ブランチ上の必須CIがすべて成功していることを確認します。
2. 同じcommitから作ったLinux `.deb` またはWindows NSIS installerをclean machineへ導入します。Nix経路を検証する場合は同じcommitのclosureを使います。
3. 配布物を使う場合はSHA-256を記録します。source実行の場合はNix store pathを`build_identity`へ記録します。
4. 単調時計を使う測定器、既知frame、loopback／MCU fixtureを試験前に同期します。
5. token、webhook URL、broker credential、生のserial number、ユーザーpath、任意コード本文を記録しません。

記録は`artifacts/hardware/<platform>/<capability>/<UTC timestamp>.json`へ保存します。形式は[`acceptance-record.schema.json`](../rust/pokecon-contracts/registry/acceptance-record.schema.json)が正本です。[example](../rust/pokecon-contracts/registry/acceptance-record.example.json)は形式確認専用で、受入証拠ではありません。

```bash
nix run .#acceptance-record-check -- \
  /absolute/path/to/artifacts/hardware/linux/performance/2026-01-01T000000Z.json

# 同じsource commitに必要なrelease matrix全体を検査
nix run .#acceptance-record-check -- \
  --release-candidate <40桁のsource commit> \
  /absolute/path/to/artifacts/hardware
```

総合結果を`passed`にできるのは、以下に示す必須step IDを記載順にすべて含め、各stepが`passed`で、該当する閾値をすべて満たした場合だけです。validatorはstepの欠落、追加、重複、順序違い、計測閾値違反、開始時刻以後でない完了時刻を拒否します。`not_applicable`を含む記録は、そのcapabilityの合格証拠として使用できません。

## 仮想I/O事前試験

Linuxでは実機gateへ進む前に、kernel PTYとV4L2 loopbackを使ってnative I/O経路を検査できます。

```bash
nix run .#virtual-io-check

# 既存のloopback device indexを使う場合
nix run .#virtual-io-check -- 42
```

このタスクはPTY master／slave間のpartial read／writeと、`ffmpeg`の既知test patternを入力した`/dev/videoN`の列挙、format交渉、BGR復号、再設定を検証します。実行中kernelに対応する`v4l2loopback` moduleと、moduleをloadできるpasswordless `sudo`が必要です。タスク自身がmoduleをloadした場合だけ終了時にunloadします。

仮想I/Oはnative OS APIまでの決定的な回帰試験です。ただし、USB切断、MCU firmware、対象console上の入力、物理camera固有format、driver差分は再現しないため、以下の実機記録を省略できません。

## 実機gate

### MCU／シリアル

LinuxとWindowsの両方で、対象consoleへ接続したMCUを使用します。

1. `protocol_frames`: native selectorの生値を選択し、既定、Qingpi、3DSの各frameを送信します。
2. `control_mapping`: ボタン、左右stick、hat、touchを操作し、console側で一致を確認します。
3. `reconnect_and_partial_write`: partial write、抜線、20回再接続上限、明示切断による再試行取消しを確認します。
4. `neutral_shutdown`: WebSocket切断、script停止、profile切替、アプリ終了後にneutral frameが到達することを確認します。
5. `selector_rollback`: 設定変更失敗時に同じ旧selectorへ戻り、別deviceを暗黙選択しないことを確認します。

capabilityは`mcu_serial_device_and_target_console`です。

### カメラ

LinuxではV4L2 device、Windowsではnative camera IDを使い、色と座標が既知のframe fixtureを撮影します。

1. `known_frame_processing`: 640×360、1280×720、1920×1080でBGR色順、crop、flip、template resultを確認します。
2. `resolution_pin_consistency`: 解像度変更中に破損frameがなく、pin中frameが変化しないことを確認します。
3. `screenshot_destinations`: PNG／JPEG、download／captures／native path、衝突suffix、overwriteを確認します。
4. `media_fallback_recovery`: WebRTC映像、MJPEG fallback、主経路復旧が同じframe系列を表示することを確認します。
5. `device_loss_identity`: device停止／抜線時に暗黙の別device選択がなく、再試行UIと診断IDを確認します。

capabilityは`capture_device_with_known_frame_fixture`です。

### 音声と外部通知

正準固定互換コーパスが要求する音声入力deviceを、LinuxとWindowsで`audio_input_device`として次の順に検証します。

1. `audio_device_selection`: native selectorの選択、再起動後の同一性、暗黙の代替deviceを選ばないことを確認します。
2. `audio_capture_processing`: 既知音源を取得し、互換scriptが要求する処理結果を確認します。
3. `audio_disconnect_recovery`: device停止／抜線時の診断、再試行、同じselectorでの復旧を確認します。
4. `application_continuity`: 音声失敗後もcommand、UI、shutdownが継続することを確認します。

credential付きDiscord、MQTT、socket等は`credentialed_network_notification_endpoints`として次の順に検証します。credentialと受信payloadはevidenceへ保存せず、送信先側の秘匿化済みrequest IDだけを使用します。

1. `notification_success`: 受信側request IDで成功を確認します。
2. `notification_timeout`: 到達不能または応答保留fixtureでtimeoutと診断を確認します。
3. `notification_rejection`: 認証拒否またはprotocol拒否を安全なエラーとして確認します。
4. `application_continuity`: 失敗後もcommand、UI、アプリケーションが継続することを確認します。

## ブラウザmatrix

LinuxとWindowsのbackendそれぞれについて、Chrome／Edge 94以上、Firefox 130以上、Safari 16.4以上の現行利用可能versionで確認します。SafariはmacOS上のremote browser clientから対象backendへ接続して構いません。各browserを別の`browser_matrix`記録にします。

1. `keyboard_workflows`: 6 main tab、Commandsの3 subtab、10 shortcut、開始／一時停止／再開／停止をkeyboardだけで操作します。
2. `accessibility_layout`: focus順、ARIA名、200% zoom、OS high-contrast、狭幅layoutを確認します。
3. `webrtc_channels`: WebRTC videoと2 DataChannelを確立し、入力generation切替を確認します。
4. `fallback_recovery`: 主経路を故障させ、30秒後のMJPEG／WebSocket fallbackと、復旧後のWebRTC再昇格を確認します。
5. `reconnect_snapshot_recovery`: heartbeat timeout、3秒間隔／20回上限、手動再接続、revision gapからのsnapshot回復を確認します。
6. `feature_workflows`: camera、serial、command、profile、設定競合、script dialog／Tk／overlayの主要操作を確認します。

## 性能gate

release build、1920×1080既知frame、接続済みloopback controller、他の高負荷processを停止したclean machineを使います。60秒warm-up後、単調時計で300 sample以上を取得し、p50、p95、maximumを記録します。LinuxとWindowsを別記録にし、使用browserを`environment.browser`へ記録します。

1. `measurement_environment`: build、artifact hash、device、browser、時計、frame fixture、loopback経路を記録します。
2. `warmup`: 60秒warm-upし、解像度と接続経路が安定したことを確認します。
3. `sample_collection`: 5 metricを同じ条件で各300 sample以上取得します。
4. `threshold_evaluation`: 生データからp50、p95、maximumを再計算し、次の閾値と一致することを確認します。

| metric | 合格条件 |
|---|---|
| `webrtc_video_latency` | p95が100 ms未満 |
| `mjpeg_video_latency` | p95が150 ms以下 |
| `controller_input_latency` | p95が50 ms未満 |
| `ui_frame_rate` | p50が60 FPS以上 |
| `ui_input_latency` | p95が16 ms未満 |

映像遅延はfixtureの表示timestampからbrowser描画まで、controller入力は入力受付からloopback frame観測まで、UI入力はevent timestampから次のpaintまでを測ります。`performance`記録には5 metricすべてを含めます。

## デスクトップライフサイクル

LinuxとWindowsで次を検証します。capabilityは`desktop_lifecycle`です。

1. `close_policies`: `ask`、`shutdown`、`keep_backend`、確認dialog、取消しを確認します。
2. `tray_and_single_instance`: tray復帰と既存instance検出を確認します。
3. `os_shutdown_and_repeated_close`: OS shutdownとclose連打が同じ停止系列へ収束することを確認します。
4. `hung_resource_shutdown`: worker hang、camera writer hang、接続中serverの停止順と期限を確認します。
5. `neutralization_and_mapping_lifetime`: 終了後のneutral入力と、writer停止失敗時にmappingを早期unmapしないことを確認します。

## 統合負荷・長時間stress

LinuxとWindowsのrelease buildで、1920×1080既知frame camera、WebRTC browser、serial loopbackまたはMCU、Python script、動的Lua設定、操作中UIを同時に動かします。使用したworkload manifest、event rate、cycle count、開始／終了時のCPU・memory・handle／descriptor数をevidenceへ保存し、最低60分継続します。capabilityは`integrated_load_stress`です。

1. `concurrent_topology`: Rust、Python、Lua、Web／Tauri、camera、WebRTC、serialが同じproduction topologyで同時稼働することを確認します。
2. `sustained_load`: camera処理、controller出力、script callback、UI操作を最低60分継続し、破損frame、入力滞留、無応答がないことを確認します。
3. `queue_pressure`: callback queueとIPC queueをworkload manifest記載の上限まで加圧し、優先度、bounded overflow、非blocking契約を確認します。
4. `reconnect_and_profile_cycles`: WebSocket／WebRTC再接続とprofile切替を反復し、世代混在、旧worker再生成、snapshot欠落がないことを確認します。
5. `faulted_shutdown`: callback実行中、IPC滞留中、camera writer／worker故障中にshutdownし、neutral化と期限内停止を確認します。
6. `resource_leak_audit`: 開始／終了時のresource差分と診断logを比較し、単調増加するtask、mapping、socket、handle／descriptorがないことを確認します。

## Security acceptance

LinuxとWindowsで、loopback clientと別hostのLAN clientを使用します。任意コード本文、secret、token、個人pathはevidenceへ保存せず、hash、redacted request ID、診断IDだけを記録します。capabilityは`security_acceptance`です。

1. `lan_trust_warning`: 非loopback `server.bind_address`の保存時と再起動要求時に、認証なしの完全信頼とPython／Lua実行権限をUIが明示することを確認します。
2. `lan_dynamic_code_execution`: LAN clientから既定Host／Origin／Content-Type／固定header契約を満たして`load_content`／`load_path`／`reload`を実行でき、追加認証やpeer-IP制限がないことを確認します。
3. `secret_non_disclosure`: 設定snapshot、OpenAPI応答、WebSocket、log、診断、manifest、debug表示に平文secretが現れないことを確認します。
4. `path_jail`: 相対path traversalとsymlink escapeが拒否され、仕様で許可された明示的絶対pathだけが正準identityで処理されることを確認します。
5. `request_boundary_controls`: 不正Host、Origin、Content-Type、固定header、WebSocket preflightが閉じて拒否され、SPA fallbackが未知APIを隠さないことを確認します。

## Release candidateの判定

同じsource commitに対し、次が揃うまで外部platform gateは未完了です。

- LinuxとWindowsのMCU／serial、camera、performance、desktop lifecycle
- 固定互換scriptが要求するaudio／外部service
- Linux／Windows backendに対するChrome、Edge、Firefox、Safariのbrowser matrix
- LinuxとWindowsの統合負荷／長時間stress、security acceptance
- schemaとsemantic matrix検証済みで総合`passed`の記録と、参照した秘匿化済みevidence

記録の失敗は既知問題として成功へ読み替えず、修正後の新しいcommitと新しい記録で再検証します。
