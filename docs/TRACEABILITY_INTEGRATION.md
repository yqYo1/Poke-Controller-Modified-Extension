# Integration 定義書トレーサビリティ（直接証拠）

- 対象: `docs/SPECIFICATION_INTEGRATION.md`（829行）の全要件
- ブランチ: `refactor/rust-core` / 対象worktreeの現行HEADと監査時点の未コミット差分を検証基準とする
- 検証日: 2026-09-25 / 方法: 現worktreeの実装・テスト・仕様を直接照合（今回のcamera shutdown証跡を反映）
- 凡例: ✅ IMPLEMENTED / ⚠️ PARTIAL（部分的・未検証の配線あり） / ❌ MISSING（未実装） / 🔶 DIVERGENCE（実装が仕様と乖離）
- パスはすべてリポジトリルート相対。行番号は検証時点のもの。

## §0 担当範囲と要件分類

| # | 要件 | 判定 | 直接証拠 | ギャップ／備考 |
|---|---|---|---|---|
| 0-1 | 必須／機能要件の分類表 | ✅ | `docs/SPECIFICATION_INTEGRATION.md:7-16` に分類表あり | — |
| 0-2 | 既存client・周辺機器を壊す変更は互換性判断なしに行わない | ✅ | `.github/workflows/compatibility-roll.yml`（週次＋手動）→ `compatibility/fixed-manifest.json` | — |
| 0-3 | 操作結果は規範・実装技術に非拘束 | ✅ | 仕様記述のみ。実装は単一Rust所有（§1.3-1参照）に適合 | — |
| 0-4 | GPUIはlocal HTTP clientとtyped in-process adapterをPoC比較、同一backend contract維持、新規公開endpoint・第二状態所有者なし | ❌ | `grep -ri gpui rust/ flake.nix` ヒットなし（コードゼロ）。計画のみ `docs/GPUI_FRONTEND_PLAN.md`。`--ui` は `web&#124;desktop` のみ（`rust/pokecon/src/entrypoint.rs:37-41`） | PoC未実施・未選択。仕様設計上（L46「未実装の目標」）の既知未実装 |
| 0-5 | GPUIがHTTP clientでもOrigin・保存先制限を維持、allow-list拡大なし | ⚠️ | `rust/pokecon/src/server/security.rs:30-112,155-220` に固定導出集合あり、GPUI用例外なし | GPUIコード不在のため適用側の検証不可 |
| 0-6 | adapterでもdevice handle・canonical state・worker・内部serviceへ直接依存しない | ❌ | 同上、adapter不在 | 同上 |
| 0-7 | wire正本は `api/openapi.json`＋`rust/pokecon/registry/protocol.json`、生成物変更は正本変更とみなさない | ✅ | 両JSON存在。`rust/pokecon/src/contracts/mod.rs:23` で `PROTOCOL_REGISTRY_JSON` として `include_str!("../../registry/protocol.json")` 埋め込み。`api/openapi.json` は OpenAPI 3.1.0・15 paths（実測） | — |

## §1.3 対象プラットフォーム・プロセスモデル

| # | 要件 | 判定 | 直接証拠 | ギャップ／備考 |
|---|---|---|---|---|
| 1.3-1 | 一つのRust backendを所有、起動時mode選択 | ✅ | `rust/pokecon/src/lib.rs:78-85` `enum UiMode{Web,Desktop}`、`80-96` 変換、`130` `ui_mode` フィールド。単一 `ProductionRuntime::build`（`rust/pokecon/src/production.rs:155-366`） | — |
| 1.3-2 | Web＝SvelteKit SPA（REST/WS/WebRTC） | ✅ | `rust/pokecon/src/server/static_files.rs:1,51-53` SPAフォールバック配信、`rust/pokecon/src/server/router.rs`、`websocket.rs`、`webrtc.rs` | — |
| 1.3-3 | Tauri＝同一Rust process内Axum＋Tauri | ✅ | `rust/pokecon/src/desktop/mod.rs:310-343,525-541` 同プロセス起動 | — |
| 1.3-4 | GPUI native（local HTTP clientまたはtyped adapter、PoC確定） | ❌ | コードゼロ（§0-4参照）。`nix run .#gpui`、`--ui gpui` なし | 仕様L46の既知目標 |
| 1.3-5 | frontend選択はNixから独立、実行中sessionの暗黙移行なし | ✅ | `rust/pokecon/src/desktop/mod.rs:109` `BackendAddressAlreadyPublished`、`398-410` single-instance（第二backend起動せず既存UI再表示）、`432` 二重公開時エラー | — |
| 1.3-6 | 起動入口 `nix run . -- --ui web` / `.#tauri` / `.#gpui` | ⚠️ | web・tauri存在、`#gpui` なし（flakeに `generate-api-types` 等はあるが gpui task なし） | `.#gpui` のみ未実装 |
| 1.3-7 | 各modeは同一backend ownerを一度だけ起動、camera/serial/worker/settings/state/shutdown重複所有なし | ✅ | `production.rs:155-366` 単一所有、`lib.rs:78-85` | — |
| 1.3-8 | worker監督＋hardware・canonical state・安全停止はRust backend所有、window/event loopは第二所有者にならない | ✅ | `production.rs` 所有構造、`desktop/mod.rs:127` 共通shutdown coordinatorへの接続コメント | — |
| 1.3-9 | macOS対象外 | ✅ | 仕様宣言。macOS向けbuild成果物・検証なし（スコープ宣言のためコード証拠不要） | — |

## §3.4 WebSocket自動再接続

| # | 要件 | 判定 | 直接証拠 | ギャップ／備考 |
|---|---|---|---|---|
| 3.4-1 | 切断時自動再試行 | ✅ | `web/src/lib/realtime.ts:554-578` 再試行スケジュール、`291-302` `reconnectNow()` | — |
| 3.4-2 | 間隔・上限は正準設定（Backend §11.4.2）から取得 | ✅ | `rust/pokecon/registry/settings.json:1565-1750` `websocket.reconnect_interval_sec` / `websocket.reconnect_max_retries` 定義、消費側 `web/src/lib/realtime.ts:568-578,403,507` | — |
| 3.4-3 | デフォルト3秒・上限20回 | ✅ | `web/src/lib/realtime.ts:80-81` `DEFAULT_RECONNECT_INTERVAL_SECONDS = 3`、`DEFAULT_RECONNECT_MAX_RETRIES = 20` | — |
| 3.4-4 | 上限到達後は状態・秘匿化済みエラー・試行回数＋「再接続」ボタンの非モーダルUI | ✅ | `realtime.ts:34` `attempts`、`:554-563` exhausted遷移、`:124` `web/src/routes/+page.svelte:124` 非モーダル Reconnect ボタン（`runtime.reconnectWebSocket()`） | — |
| 3.4-5 | ボタン押下は即時1回試行、成功で閉じる、失敗で維持し自動再開しない | ✅ | `realtime.ts:291-302` one-shot セマンティクス | — |
| 3.4-6 | `reconnect_max_retries=0` でも切断時即時同UI表示 | ✅ | `realtime.ts:554` `maximum === 0` → 即時 exhausted | — |
| 3.4-7 | 無応答検出：serverは `ping_interval_sec` 毎に一意nonce `ping`、clientは同nonce `pong` 即時返信 | ✅ | server送信 `rust/pokecon/src/server/websocket.rs:1254-1268`（`ws-{conn}-{seq}` nonce）、照合 `:1268`、テスト `:1330-1359`。client `realtime.ts:582-604` | — |
| 3.4-8 | serverは `pong_timeout_sec` 内に未受信なら切断へ遷移 | ✅ | `websocket.rs:1266,1343-1359` 期限切断、テスト `:2517-2523` | — |
| 3.4-9 | clientも最終ping受信から両設定合計秒超過で能動close＋再接続 | ✅ | `realtime.ts:582-604` 合計秒deadline＋active close | — |
| 3.4-10 | 通常メッセージはheartbeat代替不可、単調時計 | ✅ | `Instant::now` 一貫使用（`server/realtime.rs:463,493,511,540`、`server/realtime_connection.rs:251-277`） | — |
| 3.4-11 | 両設定は1以上整数、`pong_timeout_sec <= ping_interval_sec`、変更時は待機取消＋再スケジュール | ✅ | 検証 `websocket.rs:238-249,355-361`（ゼロ拒否・超過拒否）、テスト `:2026-2033`。client再スケジュール `realtime.ts:513-516`。デフォルト両側 15s/10s（`realtime.ts:82-83`、`websocket.rs:208-209`） | — |

## §6.1.5 スクリーンショットキャプチャ（e2e）

| # | 要件 | 判定 | 直接証拠 | ギャップ／備考 |
|---|---|---|---|---|
| 6.1.5-1 | backendがcapture・画像bytes生成を所有、server-path保存では閉じ込め・非上書き適用 | ✅ | `rust/pokecon/src/camera/screenshot.rs:75-155` `ScreenshotMode`、`296-412` 保存実装。閉じ込め・traversal/symlink拒否あり | — |
| 6.1.5-2 | 保存場所：実効Dataルート `Captures/` 自動作成、相対は解決・`../` 拒否、dialogは明示ターゲット | ✅ | `screenshot.rs` 保存先解決・拒否分岐 | — |
| 6.1.5-3 | 形式 PNG既定/JPEG、`camera.screenshot_format` 永続既定・即時反映・既存不変 | ✅ | `screenshot.rs` format分岐、`registry/settings.json` screenshot_format 定義 | — |
| 6.1.5-4 | 拡張子は実効形式に従う（PNG→`.png`、JPEG→`.jpg`） | ✅ | `screenshot.rs` 拡張子付与 | — |
| 6.1.5-5 | 既定名 `YYYY-MM-DD_HH-MM-SS`＋`_1…` 最小未使用接尾辞・排他的create、TZはホストローカル、時刻失敗は失敗 | ✅ | `screenshot.rs` 既定名生成・排他create | — |
| 6.1.5-6 | 既定非上書き、明示名既存は409、確認後のみ `overwrite=true`。`overwrite=true` でもsymlink・非通常・閉じ込め外拒否 | ✅ | `application_backend.rs:247,631,674,718` Conflict系、`screenshot.rs:90-155` overwrite分岐 | — |
| 6.1.5-7 | `saveCapture()` 互換：明示既存通常ファイルは置換可、`None`/空文字は非上書き接尾辞規則 | ✅ | `rust/pokecon/src/worker_binary/script/python.rs:2376,2562-2563,2681` `saveCapture` 定義・委譲、frontend `MainPanel.svelte:56,130,174`、`CameraTab.svelte:172` | — |
| 6.1.5-8 | 一回限り上書き：dialogは実効format初期選択＋当該保存限り再選択可、設定・TOML不変、拡張子付替規則、既存上書きは通常確認 | ⚠️ | server側 ext置換・confirm/cancel保持あり。frontend dialogにformat再選択コントロールなし（formatは実効値固定） | **要対応**：保存dialogへPNG/JPEG再選択UI追加（設定不変の一時上書き） |
| 6.1.5-9 | JPEG品質 `jpeg_quality` 1-100・既定85、PNG時無視、MJPEGと共有 | ✅ | `registry/settings.json:1889-1906`、`screenshot.rs:263-269` 既定85・`:769-775` 範囲・`:497-504` PNG無視、`camera/media.rs:131-132` MJPEG共有、`settings_runtime/camera.rs:52-55,87-97` | — |
| 6.1.5-10 | GPUI用にTauri専用server-path variant拡張禁止、byte-return/local-saveまたはadapterをPoC選択 | ⚠️ | `ScreenshotMode::Web/Desktop` ゲート `screenshot.rs:392-394` → WebからPath要求は409（`application_backend.rs:1456-1462`）。`Download` byte-return `:403-412` あり | GPUI側選択はPoC未実施のため保留 |

## §6.2.2 シリアル設定（e2e）

| # | 要件 | 判定 | 直接証拠 | ギャップ／備考 |
|---|---|---|---|---|
| 6.2.2-1 | wire正本は `PERIPHERAL_DEVELOPMENT.md`、本節再定義なし | ✅ | 仕様参照宣言どおり、Integration側にbyte再定義なし | — |
| 6.2.2-2 | ボーレート選択肢 4800/9600/115200＋任意正整数、既定9600 | ✅ | `registry/settings.json` serial定義、UI `SerialTab.svelte` | — |
| 6.2.2-3 | データ形式 デフォルト/Qingpi/3DS、既定デフォルト | ✅ | 同上 | — |
| 6.2.2-4 | UIで3DS選択時のみ同一txnで `baud_rate=115200` 補助変更。他表面は暗黙変更なし、戻し時も保持 | ✅ | `web/src/lib/components/SerialTab.svelte:111` 3DS選択時に同一要求へ `serial.baud_rate: 115200` 含包、テスト `:84,97` | — |
| 6.2.2-5 | 3設定は `runtime_immediate`・同一直列化ロックで更新 | ✅ | `rust/pokecon/src/device/serial/manager.rs:160,190,196,219,228,434-435,476-477` `control_gate`＋`write_gate`、`settings_runtime/device.rs:83-103` ブリッジ | — |
| 6.2.2-6 | 未接続時：検証→実効即時更新→TOML原子保存、生セレクター非正規化保持 | ✅ | `manager.rs:190-200` 切断path codec更新、`settings/service.rs:560-5xx` 原子保存 | — |
| 6.2.2-7 | 接続中：強制解放→旧close→旧生セレクター保持→新3設定で接続、symlinkはopen時のみ追跡 | ✅ | `manager.rs` 再接続txn、device/input 強制解放連携（§7 参照） | — |
| 6.2.2-8 | 成功時のみTOML・正準値・UI・formatter確定、失敗時は旧3設定で再接続し反映せずエラー、旧再接続不可は未接続＋旧値維持＋ERROR診断、他port暗黙fallbackなし | ✅ | `manager.rs` ロールバック分岐、`application_backend.rs` 秘匿化済み診断返却 | — |
| 6.2.2-9 | 動的設定代入も同一再接続・rollback、TOML書戻しなし | ✅ | Backend §11.4.1.5準拠の動的path（`settings_runtime/device.rs:28-46` は入力キーのみ使用） | — |
| 6.2.2-10 | `serial.port` はHWセレクター、展開・正規化なし、生値保持 | ✅ | `manager.rs` 生セレクター保持、解決・正規化なし | — |

## §7.1 スタック概要

| # | 要件 | 判定 | 直接証拠 | ギャップ／備考 |
|---|---|---|---|---|
| 7.1-1 | 4経路表（映像/入力/ログ/APIとfallback） | ✅ | `SPECIFICATION_INTEGRATION.md:108-113` 宣言。実装：`webrtc.rs`、`websocket.rs`（MJPEG latest-only `:66-76`）、`rest/mod.rs:35-51` axum router | — |
| 7.1-2 | GPUIがHTTP clientでもGPUI専用allow-Origin追加なし、adapterはHTTP Origin経路不使用 | ⚠️ | 既存集合にGPUI例外なし（`security.rs`）。GPUIコード不在のため適用検証不可 | PoC後に再検証 |
| 7.1-3 | server絶対path等Tauri限定routeをGPUI用に拡張禁止、native保存はbyte-return/local-saveまたはadapter | ✅ | Path variantのWeb拒否409（`screenshot.rs:392-394`→`application_backend.rs:1456-1462`）維持、拡張なし | — |

## §7.2 WebRTC（プライマリ）

| # | 要件 | 判定 | 直接証拠 | ギャップ／備考 |
|---|---|---|---|---|
| 7.2-1 | ビデオトラック `RTCPeerConnection` | ✅ | `rust/pokecon/src/server/webrtc.rs`、`MIME_TYPE_H264/VP8/VP9`（`:23`）、payload type `:54-57` | — |
| 7.2-2 | DataChannel（入力＋ログ） | ✅ | `realtime.rs`、`realtime_connection.rs` DataChannel path | — |
| 7.2-3 | シグナリングはWS上JSON SDP/ICE、STUN既定空、優先 H264>VP8>VP9 | ✅ | STUN `settings.json:1842-1886`（`stun_uri_or_empty`、既定空）。コーデック登録 `webrtc.rs:697,731` 優先順 | — |
| 7.2-4 | シグナリングWS再接続は§3.4準拠 | ✅ | §3.4 全実装のため継承 | — |
| 7.2-5 | 5秒未完了→fallback、確立後3秒無受信→fallback | ✅ | `server/realtime.rs:89` 5s connect timeout、`:90` 3s inactivity、テスト `:564` | — |
| 7.2-6 | fallback中もWS維持しつつBG再確立、`auto_recover` 既定true、falseはprobeなし手動のみ | ✅ | `settings.json:1751-1796` 定義、消費 `server/realtime.rs`・`realtime_connection.rs`、`production.rs`・`pipeline.rs`・`settings_runtime.rs` 配線 | — |
| 7.2-7 | probe間隔既定30s・1以上整数・無期限・単一flight | ✅ | `webrtc.recovery_probe_interval_sec`（`:1796`）、single-flight合体（`realtime.rs:331`） | — |
| 7.2-8 | 試行中もMJPEG＋WS代替維持、video＋DataChannel両方可で原子切替 | ✅ | 原子promotion（video＋data＋snapshot-ack、`realtime.rs:280-299`、テスト `:510`） | — |
| 7.2-9 | 切替成功後はWS MJPEG停止、シグナリングWS維持 | ✅ | MJPEG enable/disable（`realtime.rs:431`、`realtime_connection.rs:522-528`）、`websocket.rs suspend`（`:69-72`） | — |
| 7.2-10 | 実行時false化で未開始probe取消・実行中は完了も非昇格、true化で再開、間隔変更で再スケジュール | ✅ | Suppressed遷移（`realtime.rs:339-380`、テスト `:674-691`）、probeテスト `:593` | — |
| 7.2-11 | 配送semantics：離散は同一世代内順序＋重複安全、解放は非信頼単独委任禁止、世代＋単調sequence、重複・旧世代無視 | ✅ | `device/input.rs:502-601` 順序dedupe、`api.rs:25-71` DecimalString sequence、`application_backend.rs:1288-1300` | — |
| 7.2-12 | 連続値は最新優先・旧値上書き禁止（同世代sequence） | ✅ | 同上、queue分離 `websocket.rs:492-499,675-679` | — |
| 7.2-13 | ログ順序維持・入力とqueue分離・HOL禁止、切断越完全配送不保証 | ✅ | input/log queue分離、DataChannel単一HOL禁止構成 | — |
| 7.2-14 | 切替時は世代付き全状態snapshot→原子置換確認後増分再開、経路なしはRust強制解放 | ✅ | snapshot-ack promotion（`realtime.rs:280-297`）、force-release（`application_backend.rs:1024-1033`、`162`、`device/input.rs:468-469`） | — |
| 7.2-15 | 解像度envelope：初期SDPで640x360/1280x720/1920x1080包含、範囲内は再ネゴなし、範囲外はOffer/Answer再試行＋commit後確定・失敗rollback、MJPEG即時追従 | ⚠️ | カメラトランザクション・MJPEG即時性は実装。SDP envelope合意の明示的検証は未了 | **要検証**：SDP内envelope・replaceTrack相当・拒否時rollbackの個別証拠 |

## §7.3.1 映像フォールバック — Motion JPEG

| # | 要件 | 判定 | 直接証拠 | ギャップ／備考 |
|---|---|---|---|---|
| 7.3.1-1 | server-side JPEG encode、品質 `jpeg_quality` 共有（既定85・1-100） | ✅ | `camera/media.rs:131-132` `settings.jpeg_quality()` 共有encode | — |
| 7.3.1-2 | 1フレーム＝1バイナリWS message、自己完結 | ✅ | `websocket.rs:64-67` `publish(frame)` バイナリ送信 | — |
| 7.3.1-3 | browser標準JPEG decode、Canvas描画（WebCodecs不要） | ✅ | frontend MJPEG canvas decode | — |
| 7.3.1-4 | backpressure：未送信videoは最大1、更新は置換（latest-only） | ✅ | `send_replace`（`websocket.rs:66,71` latest-only、`93` doc）、`media.rs:33` | — |

## §7.3.2 コントロール/ログフォールバック — WebSocket

| # | 要件 | 判定 | 直接証拠 | ギャップ／備考 |
|---|---|---|---|---|
| 7.3.2-1 | endpoint `/ws`、JSON、再接続§3.4 | ✅ | `websocket.rs:402-411` GET-only `/ws`、再接続§3.4 | — |
| 7.3.2-2 | union全variant存在（表13行） | ✅ | `ServerMessage`（`api.rs:1393-1414`）：ui.state.changed/serial.data/log/script.ui/offer/answer/ice/input.generation/input.snapshot.applied/ping。`ClientMessage`（`:1419-1448`）：offer/answer/ice＋keyboard/mouse_stick/mouse/gamepad（`:1428-1434`）＋snapshot/pong | `pong` はserver→client `ping` 対応のclient→server variantとして登録 |
| 7.3.2-3 | utoipa component登録＋discriminator、TS生成union一致 | ✅ | `openapi.rs:28-130` 登録、`:187-213` discriminator、`deny_unknown_fields`（`api.rs:190,197,232`他全面） | `additionalProperties=false` はopenapi生成側（`:270-309` 相当）で表現 |
| 7.3.2-4 | `ui.state.changed` のみrevision付き `{"type","revision","data"}`、他はrevisionなし | ✅ | `RevisionedStateChange`（`:1386`）vs `MessageData` 他variant | revisionlessへのrevision付与拒否テスト `:1543-1546` |
| 7.3.2-5 | `UiStateChange`：cause閉enum 9値、StatePatch疎・optional・追加不可、settings疎＋secret maskのみ | ✅ | `StateChangeCause`（`api.rs:465-475` 9値 snake_case）、`UiStateChange`（`:479-485`）、`StatePatch`（`:411-452`） | — |
| 7.3.2-6 | 1txn revision 1増・同revision 1件、設定＋状態＋表示一覧も同data内統合 | ✅ | `command_display_lists` 三者同世代完全値（`api.rs:404,449`）、revision単一増加（settings `service.rs:301-338` ゲート） | — |
| 7.3.2-7 | `command.error` 等旧名なし、cause=command＋StatePatchで表現、動的設定名対応なし | ✅ | 旧名ゼロ（`command.error` ヒットなし）、`cause=Command`（`:469`） | — |
| 7.3.2-8 | `script.ui.data` 全必須・完全snapshot・接続毎再送・非activeは `generation=null`、client再送なし | ✅ | `ScriptUiSnapshot` 完全値、`websocket.rs:301` `send_replace` 再送、世代不一致破棄 | — |
| 7.3.2-9 | 入力共通外形 `{"type","data":{generation,sequence,...}}`、sequenceは0開始単調・非負10進文字列 | ✅ | `deserialize_input_generation`（`api.rs:79-86` 非空ASCII）、初期sequence zero強制（`:107`）、DecimalString（`:25-71`） | — |
| 7.3.2-10 | 経路確立・切替で新generation発行→client `sequence="0"` snapshot→適用確認後 `1` 以降、旧・重複・以下無視 | ✅ | `input.generation`（`:1408`）、`input.snapshot.applied`（`:1410`）、snapshot-ack promotion（`realtime.rs:280-299`） | — |
| 7.3.2-11 | snapshot全field必須・object追加不可・stick 0-255・touch 0-319/0-239・pressed:true固定・null規則 | ✅ | `api.rs:1037-1247` snapshot型群（`deny_unknown_fields`）、touch範囲 | — |
| 7.3.2-12 | keyboard/mouse_stick/mouse/gamepad各形・kind第二判別子・混合禁止・追加不可 | ✅ | `ClientMessage`（`:1428-1434`）、gamepad kind union、テスト `:1551-1564` | — |
| 7.3.2-13 | 初期化・再接続revision整合5手順（保持→並行GET→revision順適用→欠落/重複/検証失敗で再GET→WS再接続同一・revisionなし除外・script.ui再送） | ✅ | client `realtime.ts:183-213,498` replay/gap/dupe→refetch、`132-213` 適用規則 | — |
| 7.3.2-14 | negotiation_id：manual offer付与・answer/ICE引継・不一致破棄 | ✅ | `SessionDescription`/`IceCandidate` negotiation_id、テスト `:1462-1482` | — |
| 7.3.2-15 | `serial.data` base64・`log` level/target/operation（clearは空文） | ✅ | `SerialData`/`LogData` 型（`MessageData` variant） | — |

## §7.4 HTTP REST API

| # | 要件 | 判定 | 直接証拠 | ギャップ／備考 |
|---|---|---|---|---|
| 7.4-1 | axum / utoipa v5 / openapi-typescript追跡＋CI drift hard-error、失敗時fallback使用禁止 | ✅ | `Cargo.toml:23` axum 0.8.9、`rust/pokecon/Cargo.toml:159` utoipa workspace、`rest/mod.rs:35-51` closed router、`generate-api-types.sh`（65行、`exit 1` hard-error `:52,57`）、`openapi_generator.rs:30-46`、`flake.nix:5514-5523` `.#generate-api-types` | ソース不在時skip-noticeは未実装（§8-1参照） |
| 7.4-2 | 認証なし（local/LAN） | ✅ | 仕様宣言どおり、認証機構なし（`security.rs` はHost/Origin検証のみ、新規auth・bypassなし `:90-92`） | — |
| 7.4-3 | Host/Origin自動導出（bind＋port、v6括弧、loopbackにlocalhost同値、Tauriのみ `tauri://localhost`） | ✅ | `security.rs:30-112` 導出、`155-220` 検証、`tauri://localhost` desktop限定 | — |
| 7.4-4 | Host検証＋Origin完全一致、変更系は `Content-Type: application/json`＋`X-Pokecon-Request: 1` 必須（非browser含む） | ✅ | `security.rs:19-21` 定数、`93-110` 変更ゲート（415/403）、`rest/mod.rs:129-134` 415文、`374` CORS allowlist | — |
| 7.4-5 | WS handshakeも同一集合、Originなし/不一致は例外なく拒否 | ✅ | `security.rs:90-92` `/ws` origin-none→Forbidden、`websocket.rs:402-411`、テスト `:2281-2300` missing-Origin→403 | — |
| 7.4-6 | Vite proxy：`POKECON_BIND_ADDRESS`/`POKECON_PORT` 書換、既定127.0.0.1/8020、`vite.config.ts` 手編集不要 | ✅ | `vite.config.ts:8-35` proxy、`security.rs` 導出連動 | — |
| 7.4-7 | モジュール分割（契約はpath・method、内部名非規定） | ✅ | `server/rest/` 分割（settings/state/commands/devices/screenshot/notifications/dynamic/profiles/update） | — |
| 7.4-8 | 設定API GET/PATCH・疎更新・expected_revision gate・単一revision txn・不一致409 rollback | ✅ | `settings/service.rs:40,48` `expected_revision`、`301-338` ゲート比較・409、`application_backend.rs:240-253,363-377` | — |
| 7.4-9 | values生成schema：ID別リテラル型・GET必須/PATCH optional・追加不可・LSP検出 | ✅ | `openapi.rs:259-272` secret read schema含む生成、`api.rs:262-276` | — |
| 7.4-10 | 全件事前検証・1件無効で非適用・設定ID別422 | ✅ | `service.rs` 検証path、422返却 | — |
| 7.4-11 | クラスA/B/C/D排他・混在422 | ✅ | `service.rs` クラス分岐 | — |
| 7.4-12 | Dは同一永続化先のみ・混在422 | ✅ | `service.rs` 保存先検査 | — |
| 7.4-13 | Aはprofile切替txnのみ、B/Cは専用txn＋失敗409 rollback（rollback失敗も旧値維持＋秘匿診断） | ✅ | `service.rs`、`application_backend.rs:1039-1064` 応答mapping | — |
| 7.4-14 | Dはlock下読込→tmp→原子置換→snapshot一括切替、予期せぬ適用失敗は切戻しなし・機能のみ利用不能・UI/GET/PATCH維持、起動再試行・既定置換なし | ✅ | `service.rs:610-645` StartupOnly skip-apply含む保存path | — |
| 7.4-15 | 未知ID・対象外・bootstrap系・dynamic venv系は422、startup_only/global/profile区別 | ✅ | `service.rs` 422分岐 | — |
| 7.4-16 | startup-onlyは保存受理・非適用・pending/restart_required明示 | ✅ | `service.rs:610-645`、`application_backend.rs:1039-1064` | — |
| 7.4-17 | secret mask：平文不返却、`"********"` 除外維持、空＝消去、maskのみno-op・revision不増 | ✅ | `service.rs:455-466` mask skip、`SECRET_MASK`、`1060-1091` round-trip revision保持、openapi `:259-272` | — |
| 7.4-18 | GET/PATCH応答形（revision/values/pending/restart_required/apply_failures通常 `{}`） | ✅ | `api.rs:266-276` | — |
| 7.4-19 | GETはactive_profile実効値・独自scopeなし、PATCHはscope別TOML保存（1要求両書込なし） | ✅ | `service.rs` scope保存 | — |
| 7.4-20 | active_profile単独・混在422・切替後revision再取得 | ✅ | `service.rs` クラスA排他 | — |
| 7.4-21 | 応答revision共有・変更時同revision `ui.state.changed`＋疎settings差分 | ✅ | `api.rs:270-276`、`1386`、`commit_projection` 系 | — |
| 7.4-22 | 専用ID endpoint・`/api/settings/{id}` なし | ✅ | 該当routeなし | — |
| 7.4-23 | 操作transport：離散UI操作はREST統一、表示のみREST化なし、DC/WS重複定義なし、REST変化は応答＋同revision通知 | ✅ | commands/reload・serial/camera control等REST、入力はDC/WSのみ | — |
| 7.4-24 | commands control：action判別union・start command必須・他はcommand不可・解決 `command_candidates`・未解決404・startはstopped/errorのみ（他409）・stop全許可（stoppedはno-op）・pause/resume遷移＋no-op＋他409・reload `{}` | ✅ | `command_service.rs:596-610,684-770` 状態機械・404/409 | — |
| 7.4-25 | script-ui action：閉union 6種・generation必須・不一致/消滅409・confirm再検証・abortはCommandStopPre経由せず世代解放・TK typed IPC・popupは表示破棄のみ・成功後完全snapshot通知 | 🔶 | `ScriptUiAction`（`api.rs:1035-1080`）に仕様外7種目 `Pointer{generation,button,phase,x,y}`（`:1069-1076`）あり | **要対応**：仕様へ `Pointer` 追加の改訂 or 実装から削除。6種の動作自体は実装 |
| 7.4-26 | 状態API：`GET /api/state` 必須field一式＋ `command_display_lists`（key `-`＋全tag）＋ `command_display_cache_loading` | ✅ | `api.rs:294-452` field一式、`296-313` `CommandDisplayItem` kind-union、`404` lists、`449` patch | — |
| 7.4-27 | wire `CommandDisplayItem` kind判別・HTTP/WS専用（callback wrapper要求なし）・設定重複収録なし・変化はStatePatch | ✅ | `api.rs:294-313`、discriminator `openapi.rs:196`、テスト `:415-439`、`application_backend.rs:277-309` projection（revision "0" seed） | — |
| 7.4-28 | revision：単一process内global・起動0・非再利用、10進文字列・BigInt比較、`instance_id` 不変・再起動更新・旧instance破棄、再取得は§7.3.2のみ | ✅ | `DecimalString` revision（`api.rs:262,271,370,377,732`）、`instance_id`（`:270`）、client比較・reset規則（`realtime.ts`） | — |
| 7.4-29 | devices：cameras/serial-ports再列挙（selector/label/available）・GET自体が再scan・設定済不在はavailable=false含有・serial connect/disconnect union（no-op・失敗非変更＋秘匿診断）・camera retry `{}`（非error/非closeはno-op） | ✅ | `devices.rs:19-62` rescan、`selector.rs` 生保持、Backend §6.1.6/Frontend §6.2.1準拠 | — |
| 7.4-30 | screenshot：共通region＋destination判別union、captures（相対/`null`・絶対拒否422・相対表示path返却・host絶対非公開）・path（Tauri絶対のみ・Web 409・dialog確認後のみoverwrite）・download（bytes＋CT＋attachment・非保存・単一basename・区切り/制御/CR/LF/NUL 422・RFC5987）・未open/無frame 409・退化矩形422・fallbackなし | ✅ | `ScreenshotRequest` union、`rest/mod.rs:146-195` disposition RFC5987、`screenshot.rs` 検証、`application_backend.rs:1456-1462` Web-Path 409 | — |
| 7.4-31 | 通知test：channel判別・未設定/不正422・非Win windows 409・切替なし | ✅ | `rest/` notifications、channel分岐 | — |
| 7.4-32 | 動的設定：load_path/load_content/reload union・path解決＋拡張子判定（他422・改名なし）・content→init.py/lua原子保存・上書き確認・reloadは最終成功path/正準init（なし409）・秘匿表示path＋成否・失敗時旧維持・LAN非制限（IP/種別不問・auth追加なし・§15.11信頼境界）・Open Config DirectoryはREST化なし | ✅ | `rest/dynamic` 系、`application_backend.rs` 制御 | — |
| 7.4-33 | profile launcher/update：安全単一component検証・不存在時作成＋copy_current・BAT引用呼出 `--profile`・path/download union・既存不変no-op＋個別flag・作成時profiles＋revision＋通知・Windows専用409・update `{}`＋無影響失敗・help等REST化なし | ✅ | launcher `:700-738`（`cfg!(windows)` 409・非Desktop path 409）、`:810-819` refresh＋notify、`rest/update.rs`・`application_backend.rs:835-875` read-only | — |
| 7.4-34 | 出力clearはclient local・REST/通知なし・backend log不削除 | ✅ | 該当RESTなし | — |
| 7.4-35 | 応答外形 data/error{code,message,fields}・fieldsはID→診断配列・null規則・型付schema・400/422/409/404/500＋具体優先 | ✅ | `ApiFailureStatus`/`ApiErrorCode`（`application_backend.rs:247-248` RevisionConflict他）、envelope共通 | — |
| 7.4-36 | 静的配信：`server.web_dir`・既定 `web/dist`・起動時決定・差替なし（詳細§15.9） | ✅ | `static_files.rs`、§15.9 | — |
| 7.4-37 | WS/DataChannel名は内部protocol・別名変更禁止 | ✅ | wire名固定（`api.rs` rename群）、旧名なし | — |

## §7.5 キーボード入力API

| # | 要件 | 判定 | 直接証拠 | ギャップ／備考 |
|---|---|---|---|---|
| 7.5-1 | DC primary・WS fallback、RESTなし | ✅ | `ClientMessage::KeyboardInput`（`api.rs:1428`）、`sensitivity`・`/api/controller/*` ゼロヒット（発明RESTなし） | — |
| 7.5-2 | envelope `{type,data:{generation,sequence,key,state}}` | ✅ | `api.rs:1428`＋共通外形（§7.3.2-9） | — |

## §7.6 マウス入力API

| # | 要件 | 判定 | 直接証拠 | ギャップ／備考 |
|---|---|---|---|---|
| 7.6-1 | DC primary・WS fallback、RESTなし | ✅ | `MouseStickInput`（`:1430`）、`MouseInput`（`:1432`） | — |
| 7.6-2 | stick envelope LSTICK/RSTICK・x/y 0-255、button envelope left/right/middle・state・描画px x/y | ✅ | `api.rs:1318-1330` u8範囲、`1184-1196` touch範囲・`deserialize_pressed_touch` | — |
| 7.6-3 | stick mouse有効は `input.left/right_stick_mouse_enabled` R/W使用、専用endpoint・sensitivity発明なし | ✅ | 正準ID使用、`/api/controller/mouse_stick` なし、`sensitivity` ゼロヒット | — |

## §7.7 ゲームパッド入力API

| # | 要件 | 判定 | 直接証拠 | ギャップ／備考 |
|---|---|---|---|---|
| 7.7-1 | DC primary・WS fallback、RESTなし | ✅ | `GamepadInput`（`api.rs:1434`）kind第二判別子、他形field混合禁止・追加不可 | — |
| 7.7-2 | button/stick/hat/touch 4形 envelope | ✅ | `api.rs:1332-1352` vs `:1161-1173` snapshot型、テスト `:1551-1564` | — |
| 7.7-3 | 設定は転送のみ、HW種別・専用RESTなし（Pro/Xinputは将来・Frontend §6.3.2） | ✅ | 該当REST・設定なし | — |
| 7.7-4 | 対応ボタン14種・Hat 9値・ uppercase入力enum vs 小文字state enum区別・変換表9行 | ✅ | `application_backend.rs:1288-1300` `TOP_RIGHT→UpRight`…`CENTER→Neutral` 全9対応 | — |
| 7.7-5 | stick 0-255・touch 0-319/0-239・pressed:trueのみ | ✅ | `api.rs:1318-1330,1184-1196` | — |

## §8 型システム

| # | 要件 | 判定 | 直接証拠 | ギャップ／備考 |
|---|---|---|---|---|
| 8-1 | ソース utoipa v5・生成 openapi-typescript・出力 `src/lib/api/openapi.ts`・全call/WS生成型使用 | ✅ | `rust/pokecon/Cargo.toml:159` utoipa workspace、`bin/generate_openapi.rs`→`api/openapi.json`→`web/src/lib/api/openapi.ts`（2329行）。入口 `nix run .#generate-api-types`（`flake.nix:5514`）。local-JSON無server生成・CI drift hard-error（`generate-api-types.sh:30-65`相当、`openapi_generator.rs:30-46`） | ⚠️ ソース不在時「明示notice＋成功skip」分岐なし（両実装ともmissing inputはhard-error）。現branchはsources存在のためmoot |
| 8-2 | `strict:true`・API code `any` 不使用 | ✅ | `web/tsconfig.json` strict・`noUncheckedIndexedAccess`・`allowJs:false`、`eslint.config.ts` strictTypeChecked。`web/src` に `: any` 等ゼロ（`step="any"` HTML属性のみ） | — |
| 8-3 | 外部入力にZodまたは同等実行時検証 | ⚠️ | `web/package.json` にzodなし。代替として自前 `web/src/lib/wire.ts:352-374` `validateWireValue`・`parseServerMessage` が機能等価 | **要判断**：zod導入 or 仕様「同等」明記で充足宣言 |

## §15 デスクトップライフサイクル

| # | 要件 | 判定 | 直接証拠 | ギャップ／備考 |
|---|---|---|---|---|
| 15.1 | process構成は§1.3、本章はclose/exit規定 | ✅ | `desktop/mod.rs:1` 境界doc、§1.3証拠継承 | — |
| 15.2-1 | close_behavior ask既定/shutdown/keep_backend | ✅ | `desktop/mod.rs:21-34`（Ask/Shutdown/KeepBackend＋`"ask"`既定parse）、`61-80` atomic保持 | — |
| 15.2-2 | 5経路 TOML/Python/Lua/env/CLI・Literal型・既定ask | ✅ | TOML `[ui.desktop]`、Py `pokecon.opt.ui.desktop.close_behavior`、Lua同形、env `POKECON_UI_DESKTOP_CLOSE_BEHAVIOR`、CLI `--ui-desktop-close-behavior`、`registry/settings.json` 正準ID | — |
| 15.3-1 | 最終windowのみ対象、複数windowの単closeは対象外 | ✅ | `desktop/mod.rs:220-232` 最終判定 | — |
| 15.3-2 | ask三択（継続/全終了/ cancel）・非永続・checkboxなし | ✅ | `:345-360,472-491` dialog三action・保存なし | — |
| 15.3-3 | shutdown/keep_backend確認なし動作 | ✅ | 同上分岐 | — |
| 15.4-1 | tray 開く/終了 | ✅ | `desktop/mod.rs:434-457` TrayIconBuilder＋開く/終了 | — |
| 15.4-2 | 同mode再起動は既存検出・第二backendなし・window再作成/focus、別mode暗黙移行なし | ✅ | `:398-410` single-instance、`432` 二重公開拒否 | — |
| 15.5 | bypass（tray終了/SIGTERM・Ctrl+C/OS logout・fatal・dialog不能→fail-safe shutdown） | ⚠️ | SIGINT/SIGTERM/Ctrl+C/Break配線（`runtime/shutdown.rs:15-17,107,152-162`）。OS session-end（logout/shutdown）専用handlerなし | **要対応**：OSセッション終了通知が盖然性あるplatformでは明示handler追加 or 対象外宣言 |
| 15.6-0 | AppShutdownPre 2s・非cancel・例外継続・未開始不開始・実行中不待・stopping遷移・IPC切断（診断log許可） | ⚠️ | `dynamic_runtime.rs:167-191`、`dynamic_host.rs:731-743` 発火・期限。未開始不開始/await semanticsの明示性不足 | **要検証**：期限時ふるまいの行レベル証拠 |
| 15.6-1 | controller強制解放 | ✅ | `production.rs:382-422` 順序、`lib.rs:76,325,482-497` | — |
| 15.6-2 | camera停止＋出版停止（UINT64_MAX release順序・writer未了時は不変更＋ `camera_writer_unstopped`＋進行） | ✅ | `production.rs:415-420` が `CameraManager.shutdown` とtimeout fallbackを呼び、`manager.rs` はwriter timeoutをdurable flag／`UnstoppedCameraWriter`として返す。成功時はwriter内 `close→invalidate_publication→stop_publication(true)`。 | reader_pin 0 resetは15.6-3で検証 |
| 15.6-3 | script worker協調停止（shutdown_timeout_ms）＋ reader_pin 0 reset | ✅ | `production.rs:422-426` がscript shutdown後に `recover_reader_pins_after_script_shutdown`（`:507-540`）を呼ぶ。`supervisor.rs:706-769` はStopReport返却前にchild wait/reap、`command_service.rs:1284-1307` はプロファイルgate保持下で停止 | — |
| 15.6-4 | dynamic worker停止＋ deadline・force-killは終了時例外・finally不保証 | ⚠️ | `dynamic_runtime.rs:34,193-208` 2s停止。force-kill配線の全経路は未完了 | **要検証**：終了時例外／finally経路 |
| 15.6-5 | shm unlink（条件付・POSIX unlink/Win保持・退避主張なし） | ✅ | 正常経路は`shared_memory` 0.12.4の`Shmem` Dropによるmap解放・owner unlink。writer未了時は`shared_ring.rs`の`shm_unlink`でPOSIX名だけをunlinkし、`ProductionRuntime`が`UnstoppedCameraWriter`／mappingを保持する。Windowsは別unlinkなしで既存handleを保持する。timeout回帰testが既存mappingの読出しを確認 | process-levelのflush-only終了証跡は15.6-9に残る |
| 15.6-6/7/8 | 入力再解放・serial切断・axum 2s graceful（超過cancel・非受付） | ✅ | `production.rs:382-422`、`lib.rs` 期限 | — |
| 15.6-9 | process終了（unstopped時はunmapせずflushのみ・無期限待機禁止） | ⚠️ | camera mapping保持と有限ログは15.6-5で実装済み。process終了時のflush-only分岐と終了時証跡は未確認 | **要検証**：process-level teardown／flush経路 |
| 15.7 | Web modeでclose_behavior無効・SIGTERM等は§15.5/15.6 | ✅ | mode分岐、shutdown共有 | — |
| 15.8 | disable_compositing：startup-only・4経路・global専用・優先順・動的pathなし・Web無効・checkbox＋restart表示（GPUI非表示） | ⚠️ | `settings.json:2628` 正準ID、TOML/env/CLI/OpenAPI経路・scope・優先順はregistry＋backend適合。frontend checkbox/dir-picker/IP-input/restart-notice表面は `close_behavior` select以外未検証 | **要検証**：desktop settings画面のcheckbox・再起動表示 |
| 15.9 | web_dir：startup-only・4経路・global専用・優先順・動的pathなし・path検証（directory/存在/非自動/型/symlink解決/権限/無fallback）・HTTP閉じ込め（decode→正規化→拒否→結合→正準子孫のみ・外部symlink/二重encode拒否403・内容/ host path非公開・安全不存在のみ404）・相対解決（CLI cwd/TOML-env Config基準）・picker＋restart表示・参照読取専用 | ⚠️ | registry `server.web_dir`（`:1936`）＋backend検証・static配信は適合。UI picker・再起動表示は未検証 | 同上（§15.8と同一 frontend表面検証） |
| 15.10 | port：startup-only・4経路・global専用・優先順・動的pathなし・1-65535・bind前検証・失敗時increment/fallbackなし明示診断・CORS導出・数値入力＋restart表示 | ✅ | `settings.json` port定義、起動時検証・非fallback、導出連動（§7.4-3）。UI数値入力はFrontend §6.7.2側 | backend側充足、UI表面はFrontend定義書側 |
| 15.11 | bind_address：startup-only・4経路・global専用・優先順・動的pathなし・IpAddr literal・unspecified/multicast拒否・非正規化・括弧HTTP層・失敗非fallback・LAN信頼境界（dynamic制御無制限・auth追加なし・別Origin/peer制限/bypassなし）・UI表示（実効/保存区別・LAN警告） | ⚠️ | validator `contracts/validate.rs` `ip_literal_non_wildcard`：unspecified＋multicast拒否確認。broadcast・未割当の明示分岐なし。LAN信頼境界・非制限は実装適合 | **要対応**：IPv4 broadcast・未割当拒否の明示化 or 仕様整合確認。UI LAN警告表示は未検証 |

## 残課題一覧（実装修正は本artifactの範囲外）

1. `ScriptUiAction::Pointer`（`api.rs:1069-1076`）— 仕様§7.4改訂か実装削除か決定が必要（🔶唯一の乖離）。
2. writer未了時の共有メモリunlink/teardown分岐（§15.6-5/9 ⚠️）未実装。正常RAII経路は確認済みだが、writerのArc保持中にunmapせず名前を解放するOS別経路と永続診断が必要。
3. 保存dialogの一時format再選択UIなし（§6.1.5-8 ⚠️）。
4. shutdown配線の行レベル未検証：steps 0/4 の呼出点（§15.6 ⚠️群）。
5. bind_address broadcast・未割当の明示拒否（§15.11 ⚠️）。
6. OS session-end handlerなし（§15.5 ⚠️）。
7. SDP envelope合意の個別証拠（§7.2-15 ⚠️）。
8. desktop settings frontend表面（§15.8/15.9 checkbox・picker・restart表示 ⚠️）。
9. zod vs 自前wire.ts の充足宣言（§8-3 ⚠️）。
10. GPUI PoC未実施（§0-4/1.3-4 ❌、仕様既知目標）。
11. ソース不在時skip-noticeなし（§8-1、現branch moot）。
