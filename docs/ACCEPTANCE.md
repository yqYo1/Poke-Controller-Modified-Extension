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
```

総合結果を`passed`にできるのは、必須stepがすべて`passed`で、該当する閾値をすべて満たした場合だけです。`not_applicable`を含む記録は、そのcapabilityの合格証拠として使用できません。

## 実機gate

### MCU／シリアル

LinuxとWindowsの両方で、対象consoleへ接続したMCUを使用します。

1. native selectorの生値を選択し、既定、Qingpi、3DSの各frameを送信します。
2. ボタン、左右stick、hat、touchを操作し、console側で一致を確認します。
3. partial write、抜線、20回再接続上限、明示切断による再試行取消しを確認します。
4. WebSocket切断、script停止、profile切替、アプリ終了後にneutral frameが到達することを確認します。
5. 設定変更失敗時に同じ旧selectorへ戻り、別deviceを暗黙選択しないことを確認します。

capabilityは`mcu_serial_device_and_target_console`です。

### カメラ

LinuxではV4L2 device、Windowsではnative camera IDを使い、色と座標が既知のframe fixtureを撮影します。

1. 640×360、1280×720、1920×1080でBGR色順、crop、flip、template resultを確認します。
2. 解像度変更中に破損frameがなく、pin中frameが変化しないことを確認します。
3. PNG／JPEG、download／captures／native path、衝突suffix、overwriteを確認します。
4. WebRTC映像、MJPEG fallback、主経路復旧が同じframe系列を表示することを確認します。
5. device停止／抜線時に暗黙の別device選択がなく、再試行UIと診断IDを確認します。

capabilityは`capture_device_with_known_frame_fixture`です。

### 音声と外部通知

固定互換コーパスが要求する場合だけ、音声入力deviceを`audio_input_device`として検証します。credential付きDiscord、MQTT、socket等は`credentialed_network_notification_endpoints`として検証し、成功／timeout／拒否を記録します。credentialと受信payloadはevidenceへ保存せず、送信先側の秘匿化済みrequest IDだけを使用します。通知失敗後もcommandとアプリケーションが継続することを必須stepに含めます。

## ブラウザmatrix

LinuxとWindowsのbackendそれぞれについて、Chrome／Edge 94以上、Firefox 130以上、Safari 16.4以上の現行利用可能versionで確認します。SafariはmacOS上のremote browser clientから対象backendへ接続して構いません。各browserを別の`browser_matrix`記録にします。

1. 6 main tab、Commandsの3 subtab、10 shortcut、開始／一時停止／再開／停止をkeyboardだけで操作します。
2. focus順、ARIA名、200% zoom、OS high-contrast、狭幅layoutを確認します。
3. WebRTC videoと2 DataChannelを確立し、入力generation切替を確認します。
4. 主経路を故障させ、30秒後のMJPEG／WebSocket fallbackと、復旧後のWebRTC再昇格を確認します。
5. heartbeat timeout、3秒間隔／20回上限、手動再接続、revision gapからのsnapshot回復を確認します。
6. camera、serial、command、profile、設定競合、script dialog／Tk／overlayの主要操作を確認します。

## 性能gate

release build、1920×1080既知frame、接続済みloopback controller、他の高負荷processを停止したclean machineを使います。60秒warm-up後、単調時計で300 sample以上を取得し、p50、p95、maximumを記録します。LinuxとWindowsを別記録にし、使用browserを`environment.browser`へ記録します。

| metric | 合格条件 |
|---|---|
| `webrtc_video_latency` | p95が100 ms未満 |
| `mjpeg_video_latency` | p95が150 ms以下 |
| `controller_input_latency` | p95が50 ms未満 |
| `ui_frame_rate` | p50が60 FPS以上 |
| `ui_input_latency` | p95が16 ms未満 |

映像遅延はfixtureの表示timestampからbrowser描画まで、controller入力は入力受付からloopback frame観測まで、UI入力はevent timestampから次のpaintまでを測ります。`performance`記録には5 metricすべてを含めます。

## デスクトップライフサイクル

LinuxとWindowsで`ask`、`shutdown`、`keep_backend`を確認します。確認dialog、tray復帰、既存instance検出、OS shutdown、close連打、worker hang、camera writer hang、接続中serverの停止順を検証します。終了後に入力がneutralであり、期限を超えて停止せず、writer停止失敗時にmappingを早期unmapしないことを確認します。capabilityは`desktop_lifecycle`です。

## Release candidateの判定

同じsource commitに対し、次が揃うまで外部platform gateは未完了です。

- LinuxとWindowsのMCU／serial、camera、performance、desktop lifecycle
- 該当する固定互換scriptが要求するaudio／外部service
- Linux／Windows backendに対するChrome、Edge、Firefox、Safariのbrowser matrix
- schema検証済みで総合`passed`の記録と、参照した秘匿化済みevidence

記録の失敗は既知問題として成功へ読み替えず、修正後の新しいcommitと新しい記録で再検証します。
