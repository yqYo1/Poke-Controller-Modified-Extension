# バックエンド仕様トレーサビリティ（直接証拠）

- 対象: `ghq/github.com/yqYo1/Poke-Controller-Modified-Extension/.worktree/refactor-rust-core`、ブランチ `refactor/rust-core`、対象worktreeの現行HEADと監査時点の未コミット差分
- 正本: `docs/SPECIFICATION_BACKEND.md`
- 作成日: 2026-09-24。読み取り専用監査を基礎とし、現行ソース／テストの証拠に合わせて判定記録を更新。
- 判定基準: 実装済み=直接シンボルあり / 部分的=一部のみ証拠あり / 未実装=実装なし / 仕様のみ=コード不要の宣言
- パス表記は worktree ルート相対。`src/`=`rust/pokecon/src/`、`registry/`=`rust/pokecon/registry/`、`tests/`=`rust/pokecon/tests/`。`compatibility/` は worktree ルート相対（`rust/pokecon/registry/` 接頭辞とは別体系）。
- 前身 `/home/yayoi/backend-traceability.md`（127行）の全行を現行ソースで再検証し、古い引用を修正した後継。行数・集計は証拠が支持したため維持。

## 1.2 / 4.4 / 4.6 / 6.1.6

| 要件 | 判定 | 直接証拠 |
|---|---|---|
| §1.2 UI温存（ピクセル等価なし） | 部分的 | `src/tests/ui_boundary_acceptance.rs:1` Web-primary/Tauri-adapter境界; `src/desktop/mod.rs:87` ローカルTauriコマンド |
| §1.2 スクリプト互換2層ポインタ（§4.6） | 実装済み | `registry/compatibility.json:4` fixed_baselines; `compatibility/fixed-manifest.json:2` baselines |
| §1.2 フロント技術選択（SvelteKit実装のみ、ツールキット統一なし） | 仕様のみ | `src/` に `sveltekit`/`gpui` ヒットなし（grep exit 1）; 仕様§1.2 |
| §1.2 通信範囲（ブラウザHTTP/WS契約; ネイティブは同一正準バックエンド、ブラウザ経由なし） | 実装済み | `src/server/openapi.rs:1` 決定的OpenAPI生成; `src/worker/supervisor.rs:549` stdinパイプ（プロセス内/パイプ、ブラウザ迂回なし） |
| §1.2 型安全（正準OpenAPI由来の配線型、Rust型はプロセス内） | 実装済み | `src/openapi_generator.rs:32` check_openapi_artifact; `src/server/api.rs:5` ドリフト不可 |
| §1.2 認証なし（ローカル/LANのみ） | 実装済み | `src/server/security.rs:26-27` allowed_hosts/allowed_origins のみ、認証ミドルウェアなし（※旧:33は行ずれ） |
| §1.2 実装層+監督（Rustコアが資源所有; 順序決定はIPC経由のscript/dynamicワーカー、pyオブジェクト横断なし） | 実装済み | `src/worker/supervisor.rs:387` spawn; `src/worker/ipc/mod.rs:1` stdin/stdout上の型付きMessagePack IPC |
| §1.2 環境分離（ワーカー毎venv、プロファイル切替はscriptワーカーのみ再生、dynamicワーカー維持） | 実装済み | `src/script_runtime.rs:255` ManagedUserScriptFactory::spawn（遅延生成）+ `:122-166` prepare内venv準備; `src/command_service.rs:291` はtrait宣言のみ; `src/worker/supervisor.rs:382` dynamic never regenerated（補強: `:322-323` DynamicProfileSwitchForbidden） |
| §1.2 サブインタプリタなし（独立OSプロセス） | 実装済み | `src/worker/supervisor.rs:421,427` プロセスパイプ; `src/` 全体で `subinterpret` ゼロヒット |
| §1.2 言語仕様（Python 3.14厳密; LuaJIT 2.1; Commands.*プロキシ、_meta.pyフックのみ） | 実装済み | `src/settings/python.rs:17` PORTABLE_PYTHON_VERSION=3.14.3; `src/worker_binary/dynamic/runtime/lua.rs:5` はimportのみ; `Cargo.toml:53` mlua 0.10 + luajit/vendored features（LuaJIT 2.1の厳密版固定は未検証）; `src/worker_binary/script/python.rs:764` Commands._meta |
| §1.2 ランタイム供給（nix/PBS、ワーカー毎venv、shutdown_timeout_ms、fail-soft） | 実装済み | `src/settings/python.rs:32` Nix CPython選択; `src/settings/venv.rs:270` fail-soft; `src/dynamic_host.rs:1156` shutdown_timeout_ms |
| §4.4 settings.ini廃止 | 実装済み | `src/` に `settings.ini` ゼロヒット; 正準パスは `src/settings/scaffold.rs:42` settings.toml |
| §4.4 静的設定=settings.toml | 実装済み | `src/settings/scaffold.rs:42` roots.config.join(settings.toml)（※旧persistence.rs:475はテスト内引用のため差替）; `src/settings/scaffold.rs:42` global_settings |
| §4.4 動的設定=init.py/init.lua | 実装済み | `src/settings/scaffold.rs:44-45` init.py/init.lua; `src/worker_binary/dynamic/engine.rs:1501` fixture |
| §4.6.1 固定3-SHA不変ベースライン | 実装済み | `registry/compatibility.json:9,26,43` 3 SHA; `compatibility/fixed-manifest.json:4,1694,3319` 同 |
| §4.6.1 ブランチ参照のみ+load-and-run検証 | 部分的 | `registry/compatibility.json:8-9` reference_branch+規範commit; expectations all_resolve（ブランチ非規範フラグなし） |
| §4.6.2 ローリング監視+二重ゲート（実行時+表面、不存在証明なし） | 部分的 | `registry/ci.json:317` ローリング評価; `src/compatibility_tool.rs:24` 不変コーパス探索; `.github/workflows/compatibility-roll.yml:27-30` ワークフロー実在（※旧「src外」記述は更新） |
| §4.6.2 追加専用+失敗コミット隔離/診断 | 実装済み | `registry/compatibility.json:60` append_only + `:73` non_promotable_results; `compatibility/promotions.jsonl:1` genesis連鎖（履歴は1件） |
| §6.1.6 バックエンド自動選択ネイティブ取込 | 部分的 | `src/camera/native.rs:4` + `:21-22` Linux本番モジュール; `:29` v4l::buffer::Type; opencv取込クレートなし（※仕様はOpenCV必須のため意図的乖離） |
| §6.1.6 非UIブロッキング非同期取込 | 実装済み | `src/camera/manager.rs:112` 専用取込スレッド; `:153` thread-affine; `src/application_backend.rs:492` spawn_blocking(enumerate) |
| §6.1.6 camera.device int&#124;str + Linux/Windows/common選択規則 | 実装済み | `src/camera/selector.rs:14` CameraSelector{Index(u32),Native(String)}; `:26` native(); `:28-32` 空/NUL拒否; `:239-243` 列挙優先 ById>ByPath>Index; `:211,216` by-id/by-path; `:118` MF不透明対応; `:362` ContainsNul; `:387` 負index拒否 |

## 7.8 / 7.9

| 要件 | 判定 | 直接証拠 |
|---|---|---|
| 7.8.1 トランスポート: 二重匿名パイプ+両端非同期reader/writer | 実装済み | `src/worker/supervisor.rs:541-557` build_command（stdin/stdout/stderrパイプ、kill_on_drop）; `src/worker/ipc/connection.rs:359` IpcConnection::spawn（reader+writerタスク）; `registry/protocol.json` length_prefixed_msgpack_over_stdio |
| 7.8.1 stderr OOB + TCP/ソケット/トークンなし | 部分的 | `src/worker/supervisor.rs:128-133` OobDiagnostic（フレーム化なし）; `:279` take_diagnostics; `:433-436` stderrパイプ取得。TCP禁止の積極的シンボルなし |
| 7.8.2 フレーミング: 4B BE長+MsgPack1個+1MiB事前確保拒否 | 実装済み | `src/worker/ipc/codec.rs:10` MAX_PAYLOAD_BYTES; `:63` encode_frame; `:92,140` 超過の確保前拒否 |
| 7.8.2 直列化書込+有界キュー/背圧（QueueFull） | 実装済み | `src/worker/ipc/connection.rs:724-753` writer_loop単一書込; `:457-471` try_send→QueueFull |
| 7.8.2 readerループEOF=切断+ReaderTerminatedガード（Drop/finally、待機解除、キュー破棄） | 実装済み | `src/worker/ipc/connection.rs:673-706` reader_loop（:701 EOF→ReaderTerminated）; `:259-281` DisconnectGuard Drop |
| 7.8.2 writerガードWriterTerminated+exactly-once（生産者+待機者の起床） | 実装済み | `src/worker/ipc/connection.rs:724-753` WriterTerminated+完了エラー; `:615-643` exactly-once遷移+计数 |
| 7.8.3 エンベロープ種別/欄+閉値共用体（Anyなし） | 実装済み | `src/worker/ipc/schema.rs:341-342` Envelope（deny_unknown_fields）; `:45` IpcValue閉共用体 |
| 7.8.4 request/response/event意味論 | 実装済み | `src/worker/ipc/schema.rs:342-381` variants; `src/worker/ipc/connection.rs:512` request_with_cancellation; `:556,575,594` respond/respond_error/send_event; `:230` complete_pending |
| 7.8.4 エラー: 閉IpcErrorPayload{code,message}、ASCII符号、符号対応プロキシ | 実装済み | `src/worker/ipc/schema.rs:254-255` IpcErrorPayload deny_unknown_fields; ASCII検証; `src/worker/ipc/connection.rs:541-544` Remote{code,message} |
| 7.8.4 SerialDisconnected: 未開始シリアル送信の拒否; 実行中は直列化 | 実装済み | `src/application_backend.rs:2438-2497` queued `SendCancelled`→`SerialDisconnected` regression; `:2499-2507` disconnected send mapping; `src/script_host.rs:2135-2141` public code mapping; `src/worker/ipc/schema.rs:442-447` closed ASCII code validation |
| 7.8.4 log種別+重大ポリシー; 7.8.5 stdout横取り（生パイプバイトなし） | 部分的 | `src/worker/ipc/schema.rs:294-337` LogLevel/LogTarget/LogPayload; `src/worker/script/protocol.rs:27` HOST_OUTPUT=script.host.output要求（Envelope::Log未使用）; critical-no-kill方針シンボルなし |
| 7.8.6 制御面のみ+kill意味論（キュー取消、実行中シリアル全体、強制解放） | 実装済み | 1MiB上限が制御面を拘束（codec.rs:10）; `src/worker/supervisor.rs:686-687` begin_stopping+force_release; `:748-753` start_kill; `src/worker/ipc/connection.rs:21-28` ResourceSafety::force_release |
| 7.9.1 目標: フレームのみ、≤2コピー、安定フレーム、明示freeなし | 実装済み | `src/camera/shared_ring.rs` write_slot（1コピー）+ copy_payload（読者1コピー）; RingReader::readは独立可変コピー、履歴は部分更新なし |
| 7.9.2 構成: 名前付き永続SHM、両OS同一意味 | 部分的 | `src/camera/shared_ring.rs:339-350` timeout fallbackのPOSIX name unlink／Windows no separate unlink; `src/camera/shared_ring.rs:662-671` `shm_unlink`（既存mappingは保持）; `src/camera/manager.rs:309-357` writer timeoutからfallbackへ接続。shm_open/CreateFileMapping直接シンボルなし |
| 7.9.2 流れ: release-store出版、acquire-load、UINT64_MAX→ゼロフレーム、私用コピー+unpin | 実装済み | `src/camera/shared_ring.rs:151` release store_published_token; `:101` acquire load; `:215` None→zero; `:240-255` pin生成、`:456-471` PinGuard::dropでunpin |
| 7.9.3 SharedHeaderトークン（slot&#124;seq、巻戻し、予約UINT64_MAX、順序自由） | 実装済み | `src/camera/shared_ring.rs:11` INVALID_PUBLISHED_TOKEN; `:12` MAX_FRAME_SEQUENCE; `:504-522` encode/decode_token; `:566` SharedHeader |
| 7.9.3 SlotHeader欄+単一読者pin 0/1 | 実装済み | `src/camera/shared_ring.rs:571` SlotHeader（seq/state/pin/byte_length/dtype/shape/strides）; `:240-255` pin CAS 0→1; `:524` ReaderAlreadyPinned; `:456-471` PinGuard Drop |
| 7.9.3 MappingDescriptorはIPC経由で一度 | 実装済み | `src/camera/shared_ring.rs:23` MappingDescriptor deny_unknown_fields; `:88` open検証; `src/camera/manager.rs:193,366` mapping_descriptor; `src/script_host.rs:624-630` camera_initializeで記述子配送; `src/worker_binary/script/python.rs:510-513` 相当のopen+Reader生成 |
| 7.9.4 writer: 現行除外、state 0/2+pin0 CAS、ブロックせず破棄、release出版; state=0なし再利用 | 実装済み | `src/camera/shared_ring.rs:160-195` reserve_noncurrent_slot（現行スキップ、CAS、NoWritableSlot）; `src/camera/manager.rs:437-439` NoWritableSlot時破棄（publish経路）、`:541-545` 同（reconfigure経路）（※`:415` は起動失敗時のinvalidate_publicationであり混同しない） |
| 7.9.4 reader: pin+再トークン検査+seq検査、8回yield再試行、履歴/ゼロ退避、PinGuard | 実装済み | `src/camera/shared_ring.rs:13` READ_RETRY_LIMIT=8; `:209-232` read_published_with_hook; `:372` history同形 else zero |
| 7.9.4 最新フレーム+衝突回復（回収→pin初期化、対応維持、記述子再送） | 実装済み | `src/camera/shared_ring.rs:289` recover_reader_pins; `:335` トークン無効化し対応維持; `src/camera/manager.rs:415,489,514,610` invalidate_publication |
| 7.9.5 Camera API統合（image_bgr/readFrame/getCameraImage→私用コピー） | 実装済み | `src/worker_binary/script/python.rs:895` camera_frame; `:2327-2334` image_bgr/readFrame; getCameraImage定義 `:3517-3518`（`:3247` は利用側） |
| 7.9.6 ベンチマーク注記（28.23ms対3.28ms; CIモックのみゲート） | 未実装 | 値は仕様文のみ。benches/なし、criterionなし、カメラ/CIにモックI/O性能ゲートなし（再確認） |

## 10 + 付録B

※ `python.rs`= `src/worker_binary/script/python.rs`、`contract_sync.rs`= `tests/contract_sync.rs`、`protocol.json`= `registry/protocol.json` に読替（旧引用の系統的パス不足を修正。行番号は現行で一致確認）。

| 項目 | 判定 | 直接証拠 |
|---|---|---|
| 10.1 公開隔離（ユーザースクリプトワーカーのみ、init.py/init.luaにCommandsなし） | 実装済み | protocol.json:299-356 動的設定名前空間+forbidden_namespaces=["Commands"]; contract_sync.rs:208 禁止断定; python.rs:556 add_python_paths |
| 10.2 方針（Rustが資源所有、CommandMeta切替フック、互換、命名） | 実装済み | python.rs:814-1078 PyApi IPC転送; protocol.json:58-68 ipc種別; _meta.pyi:3 CommandMeta |
| 10.3 公開モジュール dialogue/image_proc/net 射影 | 実装済み | protocol.json:251-294 module_functions（dialogue 8、net 16、image_proc 12）; dialogue.pyi:32-61、net.pyi:3-18、image_proc.pyi:9-20 |
| 10.3 所有権（実体束縛、commandなしRuntimeError、全局なし; ImageProc専用守衛） | 実装済み | python.rs:844-868 is_executing/is_alive/checkpoint/finish/abort; python.rs:731 validate_runtime_surface |
| 10.4.1 階層 Command(Meta)->PythonCommand->ImageProc; McuCommand | 実装済み | PythonCommandBase.pyi:73 PythonCommand(Command,metaclass=CommandMeta); :153 ImageProcPythonCommand; McuCommandBase.pyi:9; protocol.json:107-119 required_imports |
| 10.4.2 ライフサイクル do/finish/checkIfAlive + _logger/alive/keys/postProcess | 実装済み | PythonCommandBase.pyi:74-83; protocol.json:132-139 |
| 10.4.2 GamepadInput型 Button/Hat/Direction/Touchscreen（+Stick） | 実装済み | Keys.pyi:8 Button(IntFlag)、:28 Hat、:39 Stick、:43 Direction、:70 Touchscreen; protocol.json:115-118 |
| 10.4.2 入力 press/pressRep/hold/holdEnd/wait/short_wait/direct_serial | 実装済み | PythonCommandBase.pyi:84-90; python.rs:958 controller_input、:983 controller_neutral; protocol.json:140-146 |
| 10.4.2 設定 reload_com_port | 実装済み | PythonCommandBase.pyi:91; python.rs:1001 serial_reload; protocol.json:147 |
| 10.4.2 出力 print_t1/t2/t/s/ts/t1b/t2b/tb/tbs + show_var | 実装済み | PythonCommandBase.pyi:92-101; python.rs:1005 output; protocol.json:148-157 |
| 10.4.2 dialog群 show_dialog/is_dialog_closed/wait_dialog/dialogue6widget*2/dialogue | 実装済み | PythonCommandBase.pyi:103-131; python.rs:1030 dialog_open、:1054 dialog_status、:1062 dialog_close_all; protocol.json:158-164 |
| 10.4.2 socket群（8関数） | 実装済み | PythonCommandBase.pyi:132-139; net.pyi:3-10; python.rs:1066 network; protocol.json:165-172+262-269 |
| 10.4.2 MQTT群（8関数） | 実装済み | PythonCommandBase.pyi:140-148; net.pyi:11-18; python.rs:1066 network; protocol.json:173-180+270-277 |
| 10.4.2 通知 LINE_text（no-op）/discord_text | 実装済み | PythonCommandBase.pyi:149-150; python.rs:1073 notification; protocol.json:181-182 |
| 10.4.3 通知 discord_image/LINE_image + CropFmt/ScreenshotFormat | 実装済み | PythonCommandBase.pyi:15-16、:156-157; python.rs:1073、:911 save_image、:935 popup_image |
| 10.4.3 ctor + camera/cam/gui/canvas注入 | 実装済み | PythonCommandBase.pyi:153-155、:158 __init__; protocol.json:184-188 |
| 10.4.3 Camera（image_bgr/readFrame/isOpened/fps/capture_size/flip/flip_mode/set_flip/saveCapture+内部5） | 実装済み | PythonCommandBase.pyi:21-43; python.rs:880 camera_state、:888 camera_control、:895 camera_frame、:911 save_image; protocol.json:204-219 |
| 10.4.3 CaptureArea（ImgRect/ImgText/delete*/setFps/setShowsize/rightMouse/touchscreen/saveCapture/show_size/is_show_var+mouse/range関数） | 実装済み | PythonCommandBase.pyi:45-71; python.rs:929 overlay; protocol.json:220-245 |
| 10.4.3 tkinter→Web橋渡し（Toplevel/Scale/Button/Label部分集合、コールバックスレッド、再接続） | 部分的 | python.rs:951 tk、:367 tk_event、:490 `_shutdown_tk`、:596 `_reset_tk`、:615 Cleanup（※旧:611は+4ずれ）; Scale/get/config/再接続部分集合の専用シンボルなし |
| 10.4.3 画像処理12関数（match/max/GPU/contained/save/popup/get/open/setDir/filespec/rect/text） | 実装済み | PythonCommandBase.pyi:159-170; image_proc.pyi:9-20; python.rs:895、:929、:935; protocol.json:191-203+280-293 |
| 10.4.4 McuCommand（sync_name）+start/end | 実装済み | McuCommandBase.pyi:9-12; python.rs:994 serial_write_row; protocol.json:114、246; contract_sync.rs:1664（※旧:1668は行ずれ） |
| 10.5.1 KeyPress input/inputEnd/hold/holdEnd/neutral/end/ser | 実装済み | Keys.pyi:82-91; python.rs:958、983、987、994; protocol.json:247 |
| 10.5.2 Sender writeRow/write + to_bytes変換（str/float TypeError、int実行時のみ） | 実装済み | Sender.pyi:3-5 writeRow/write; python.rs:987、994; protocol.json:248 |
| 10.6 dialog API Widget[T] overloads/value/has_result + 遮断/非遮断/status/wait + save/select-settings | 実装済み | dialogue.pyi:5-61; PythonCommandBase.pyi:103-131; python.rs:1030-1062; protocol.json:249、252-261 |
| 10.7 互換ゲート（imports、keys/ser、save_settings、_logger、tk標本、discord/LINE、削除なし） | 実装済み | protocol.json:107-119 required_imports、:295 compatibility_projection; contract_sync.rs:150、262、1158、1664-1696（※旧1668-1758は範囲ずれ） |
| 付録B CommandMeta（抽象do()検査→TypeError、__target_implementation__切替; 現行は全IPCプロキシ） | 部分的 | _meta.pyi:3 素朴 `class CommandMeta(type)`（do検査/切替シンボルなし）; protocol.json:130 空メンバ; python.rs:731-773 存在検証のみ |

## 11.1-11.4

※ `pipeline.rs`等=`src/settings/…`、`contract_sync.rs`=`tests/contract_sync.rs` に読替。

| 要件 | 判定 | 直接証拠 |
|---|---|---|
| 11.1 種別: 静的TOML対動的py/lua | 実装済み | registry/settings.json 79件; `src/contracts/model.rs:19` Setting |
| 11.1 TOMLは動的でない; 動的はpy/luaのみ | 実装済み | `src/settings/pipeline.rs:1370` apply_dynamic; `:318` bootstrap/dynamic-surface守衛 |
| 11.1 隔離動的ワーカー; `none`は生成なし | 部分的 | `src/dynamic_runtime.rs:225` bootstrap_dynamic、`:280` selected_language（none→runtime None）（※旧:95は誤引用）; pipeline.rs:588 load_before_dynamic |
| 11.1 `[python.script]`プロファイル毎対`[python.dynamic]`全体のみ; CPy3.14固定 | 実装済み | pipeline.rs:263 PythonWorker::Dynamic; registry python.dynamic.*全体範囲（scopeはregistryで検証） |
| 11.1 py/lua同一 `pokecon.opt.xxx`; 平坦/階層同一 | 実装済み | registry dynamic.name pokecon.opt.*; pipeline.rs:322 Dynamic normalize_value |
| 11.2 settings.ini廃止; 全体+`profiles/<name>/settings.toml` | 実装済み | pipeline.rs:905 config/settings.toml; roots.rs:237 profile_settings |
| 11.3 6段階パイプライン last-wins | 実装済み | pipeline.rs:59 SettingSource; :605 load_stage; :1308/:1339/:1370 env/cli/dynamic層 |
| 11.3 bootstrap事前解析; default<global<env<cli; 既定注入なし | 実装済み | pipeline.rs:882 resolve_bootstrap; :631 active_profile解決 |
| 11.3 `app_name` 表面/優先/既定 `pokecon` | 実装済み | registry app_name（cli --app-name/env POKECON_APPNAME）; pipeline.rs:898 SafeComponent::new |
| 11.3 4ルート（Config/Data/Cache/State）; ディレクトリ自動生成、ファイルなし | 実装済み | roots.rs:188 from_bases; :246 ensure create_dir_all |
| 11.3 `app_name` 文法 受理/拒否; trim/正規化/展開なし | 実装済み | roots.rs:11 SafeComponent; :216 native検証+tests:346 |
| 11.3 `dynamic_config_language` python/lua/none 既定lua、全体のみ | 実装済み | registry dynamic_config_language; pipeline.rs:1168 範囲守衛; :1942 不正値試験 |
| 11.3 `python.dynamic.venv` bootstrap全体のみ+pathメタ | 実装済み | registry/settings.json:3607-3613 pathメタ（directory/auto_create）; pipeline.rs:882-903 resolve_bootstrap全体解決; path.rs:45 resolve_path |
| 11.3 `pokecon.opt.*` 代入; キー結合、深結合なし | 実装済み | pipeline.rs:1151 apply_toml キー単位; :1383 動的適用 |
| 11.3 全体のみbootstrap範囲+診断付無視 | 実装済み | pipeline.rs:1168 ProfileToml範囲外無視; :403 ignored_profile_global_settings |
| 11.4.1 全静的=動的（bootstrap除く）; 79件registry | 実装済み | registry/settings.json 79件; pipeline.rs:1380 bootstrap/dynamic.name守衛 |
| 11.4.1 CLI/env既定命名; null表面明示 | 実装済み | `src/contracts/model.rs:137` Surfaces（型宣言）; registry app_name toml/dynamic unsupported_reason; `src/contracts/validate.rs:58-70` 一意性検査 + `:173-186` flag形状/POKECON_接頭辞検査（CLI/env名はregistryに明示、自動導出の生成器は未確認） |
| 11.4.1 厳密JSON複合; bool true/false; enum ASCII折畳 | 実装済み | pipeline.rs:1464 parse_wire_value厳密JSON; :1517 parse_bool; :1551 eq_ignore_ascii_case |
| 11.4.1.4 4段階path; ソース認識基底; メタ; app_name除外 | 実装済み | path.rs:45 resolve_path; :63 CLI=cwd else config; :88 expand_environment; :224 lexical_normalize |
| 11.4.1.5 UI/OpenAPI永続; startup_only restart_required | 実装済み | service.rs:124 SettingsService; :337 patch実装 + :373-379 class別適用（:300 validate_patchはゲート検証のみ） |
| 11.4.1.6 TouchscreenArea閉オブジェクト+制約 | 実装済み | registry input.touchscreen_area left_lt_right/top_lt_bottom、additional_properties false |
| 11.4.2 動的path定義 `pokecon.opt.*` | 実装済み | registry dynamic.name; pipeline.rs:322/:1383 Dynamic正規化（定義:1532） |
| 11.4.3 secret種別+供給優先+0600 | 部分的 | registry webhook_url secret:true; persistence.rs:380 mode(0o600)+preserve_mode(false)で既存0600以外も0600へリセット、:516試験が証明。SPEC§11.4.3.3（既存にchmodせずWARNING）と乖離（製品判断はここでは行わない） |
| 11.4.3 隠蔽getter/UI/REST; 編集ログ; 追加機構なし | 実装済み | pipeline.rs:26 SECRET_MASK; :1641 public_value; :1655 dynamic_secret_value; :1663 secret-safe不正文 |

## 12 / 14

| 項目 | 判定 | 直接証拠 |
|---|---|---|
| §12 core環境（LANGUAGE/AUTO_RELOAD/DYNAMIC_CONFIG_LANGUAGE/CALLBACK_*/REPORT_IGNORED/PROFILE/APPNAME） | 実装済み | registry/settings.json 79件（LANGUAGE×1/AUTO×1/DYNAMIC×6/REPORT×1/PROFILE×1/APPNAME×1）; pipeline.rs:1321 環境適用 |
| §12 camera環境（CAPTURE_FPS/CAPTURE_RESOLUTION/DEVICE/FLIP_MODE/SCREENSHOT_FORMAT） | 実装済み | registry CAMERA×5; pipeline.rs:2286 POKECON_CAMERA_FLIP_MODE試験; tests/contract_sync.rs:82 正準一致 |
| §12 serial環境（PORT/BAUD_RATE/DATA_FORMAT） | 実装済み | registry SERIAL×3; pipeline.rs:1321 汎用環境適用; tests/contract_sync.rs:1482 specification_environment_names |
| §12 input環境（KEYBOARD/ALLOW_MANUAL/STICKS×2/TOUCHSCREEN_AREA厳密JSON） | 実装済み | registry INPUT×5; pipeline.rs:1473 StrictJson環境復号; tests/contract_sync.rs:119-123 registry対仕様env集合等価 |
| §12 通知+websocket環境（LINE/DISCORD×6/WINDOWS×2 + WS_RECONNECT×2/PING/PONG） | 実装済み | registry NOTIFICATIONS×8/WEBSOCKET×4; pipeline.rs:26 SECRET_MASK; :1642 secret getter隠蔽 |
| §12 webrtc/server環境（WEBRTC×2/STUN/JPEG_QUALITY/WEB_DIR/PORT/BIND_ADDRESS） | 実装済み | registry WEBRTC×2/STUN×1/JPEG×1/WEB×1/PORT×1/BIND×1; pipeline.rs:1999 POKECON_PORT試験; :2208 POKECON_WEB_DIR試験 |
| §12 ui/shortcuts/commands環境（UI×11/SHORTCUTS×10/TAG_MATCH_MODE + DISABLE_COMPOSITING） | 実装済み | registry UI×11/SHORTCUTS×10/COMMANDS×1/DISABLE×1; pipeline.rs:2114 POKECON_DISABLE_COMPOSITING試験 |
| §12 python-script環境（VENV/SHUTDOWN_TIMEOUT/PACKAGES_LIST/OVERRIDE×2/UV_CONFIG/REVALIDATE） | 実装済み | registry script python 7行; pipeline.rs:2341 PACKAGES_LIST試験; venv.rs:75 revalidate_mutable_sources |
| §12 python-dynamic環境 + POKECON_UV_*橋渡し（6動的行; 空接尾辞誤; secret編集） | 実装済み | registry dynamic python 6行; uv.rs:514 UvChildEnvironment::build、:547 strip_prefix("POKECON_UV_")、:550/:814 空接尾辞誤、:596-601 `<redacted>` Debug; pipeline.rs:2454 POKECON_UV_INDEX_URL秘匿試験 |
| §12→registry射影 + CI同期主張（規範射影; 79/79同期検証） | 実装済み | tests/contract_sync.rs:82-123 registry 79=仕様§12 79行; pipeline.rs:2469 試験、:2474 79件、:2482 環境名79 |
| §14.1 ディレクトリ/XDG/app-name（Config/Data/Cache/State; XDG+退避; KnownFolder; SafeComponent） | 実装済み | roots.rs:132-135 xdg_or_fallback; :317 MissingKnownFolder（:276 使用側）; :188 from_bases; :216 native; :359 XDG退避試験 |
| §14.1 権限+配置（0700/0600、自動chmodなし; .hmac-key; typings/venv-*配置） | 実装済み | roots.rs:~294 mode(0o700); scaffold.rs:~122 0o700 + ~143 0o600; lock.rs:~115 0o700/0o600; hmac_key.rs:182 hard_link単一勝者（※旧:22は誤引用）; scaffold.rs:67-75 Data下typings/lua-typings |
| §14.2 生成時期（欠落→生成; 編集可は上書きなし; 生成物は更新） | 実装済み | scaffold.rs:29-45 ensure（編集可の欠落生成+生成物の原子更新）; flake.nix:3318-3319 包装品scaffold検証（profileディレクトリ存在のみ、設定ファイル内容の検証なし）（※旧3216/3310は誤引用） |
| §14.3 nix python（構築時path; python314; 全体pythonなし） | 実装済み | python.rs:56-59 build_python優先; flake.nix:184 python314.withPackages; python.rs:297 bin/python3.14候補 |
| §14.4 非nix単体（Data下固定pbs; ワーカー毎venv; 固定uv） | 実装済み | python.rs:17 3.14.3; ~105 Data/`python`/; ~229 downloads; script_runtime.rs:99 + dynamic_runtime.rs ワーカー毎VenvManager |
| §14.5 構築時3源集合（common/worker-script/worker-dynamic） | 実装済み | pyproject.toml:38-54 dependency-groups worker-script 13件/worker-dynamic空（※旧41-52は範囲ずれ、rust/pokecon配下のpyprojectではない）; rust/pokecon/build.rs:102-103 埋込抽出; package.rs:947-950 embedded len 13 |
| §14.5.1 package対応結合/MUS + override（union extras; 句単位衝突） | 実装済み | package.rs:253 結合/MUS解決; :280-287 override_application_constraints; :315 メタoverride; :436 merge_across_sources; :985 union結合試験（※旧pipeline.rs:2319引用は未検証のため削除） |
| §14.5.2 固定uv exact-sync + uv.toml扱い（明示path else --no-config） | 実装済み | uv.rs:189 ManagedUv::prepare; ~654 UvPlan::new; ~656-660 uv_config else --no-config; venv.rs:134 VenvStage::ExactSync |
| §14.5.3 fail-soft（動的: 未起動+静的継続; script: 命令毎誤+再試行; 古退避なし） | 実装済み | dynamic_runtime.rs bootstrap_dynamic（静的写像退避+startup_failure記録）; script_runtime.rs:122-166 prepare誤の命令毎伝播; venv.rs:376-380 不整合環境の利用不可 |
| §14.5.4-14.5.5 lock + manifest（単一飛行; State venv-lock; Data venv-manifest hash名; HMAC指紋; 可変再検証） | 実装済み | venv.rs:262-329 単一飛行+locks.venv; lock.rs:48 venv() + ~66 hash lock名; manifest.rs:222-224 Data venv-manifests（※旧:214はずれ）、:254 sha256 path_for（※旧:249はずれ）; :34-70 指紋入力; venv.rs:415 mutable_sources_reusable; package.rs:705 mutable_direct_source（呼出:657） |
| §14.6 python LSP（7 LSP: basedpyright/pyright/mypy/pylsp/pyrefly/ty/ruff; typings+venv-script） | 部分的 | scaffold.rs:191-194 initial_pyprojectはbasedpyright/pyright/mypy/ruffのみ。pylsp/pyrefly/tyはrepo全体ゼロヒット |
| §14.7 lua LSP（.luarc.json library + checkTableShape; 上書きなし） | 実装済み | scaffold.rs:198-202 initial_luarc（workspace.library=Data/lua-typings、type.checkTableShape=true）; :65 create_editable_file上書きなし |

## 11.5

| 要件 | 判定 | 直接証拠 |
|---|---|---|
| 11.5.1 時期（起動/手動/auto-reload） | 部分的 | worker_binary/dynamic/dispatch.rs:246 起動init.{primary.extension()}; dynamic/control.rs:35 DynamicConfigControl::{LoadPath,Reload} + worker_binary/dynamic/engine.rs:379 制御処理。自動reload監視なし（`notify`/FileWatcherは`src/`全体ゼロヒット、再grep確認） |
| 11.5.2 共存（Neovim式単一選択） | 実装済み | dynamic_runtime.rs:~280 selected_language（python/lua/none）; registry/settings.json dynamic_config_language既定"lua"; worker_binary/dynamic/engine.rs:296 primary eager + :472 ensure_runtime遅延 |
| 11.5.3 共通（即時/記憶/優先） | 実装済み | dynamic/transaction.rs:205 EvaluationTransaction::set_setting; dynamic/host.rs:435 InMemoryDynamicHost（※旧:446はずれ）; dynamic_runtime.rs:225 bootstrap_dynamic（動的>静的、CLI最上） |
| 11.5.4 Python API表面 | 実装済み | worker_binary/dynamic/runtime/python.rs:501 source、:507 get_state/set_state、~543 profile_switch、~549 controller_update; dynamic/protocol.rs PYTHON_SITE_PACKAGES_ENV組込CPython; dynamic_runtime.rs:245-267 起動失敗は前世代維持 |
| 11.5.5 Lua API表面 | 実装済み | worker_binary/dynamic/runtime/lua.rs:251 autocmd.on、:376 pokecon={opt,state,autocmd,errors,profile,controller,...}; :5 mlua組込; Pythonと同一primary+遅延ensure_runtime経路 |
| 11.5.6.1 autocmd/event（同時+queue+優先+timeout） | 実装済み | dynamic/event.rs:258,275,343,348,373,386 on/once/off/clear/define/list_defined; :130 RegistrationOptions{priority:i32}; dynamic/callback.rs:20 CallbackSettings{soft 2000,grace 1000,hard 5000,max_concurrency 8,queue 1024} + :85 resolve + :103 hard>=soft+grace検証; :466 HandlerId毎busy_lanes; EventResult.cancelled厳密False取消 |
| 11.5.6.2 source() | 実装済み | dynamic/source.rs:94 SourceStore::resolve（config基底、expand_tilde、字句+正準symlink逸脱拒否、.py/.lua検査）; :26-36 SourceError::CycleDetected; worker_binary/dynamic/engine.rs:489 同一スレッド同期評価 |
| 11.5.6.3 state | 実装済み | dynamic_host.rs:411 public_state_snapshot; :919 profile_switch_begin/get/set/merge_state_value; command_candidates/tagsは段階script-loadで書込可（~496/:514） |
| 11.5.6.4 profile（切替含む） | 実装済み | worker_binary/dynamic/engine.rs:463 DynamicEngine::switch_profile + dynamic/protocol.rs SWITCH_PROFILE/HOST_PROFILE_SWITCH_{BEGIN,COMMIT,ABORT,END}; dynamic_host.rs:642 prepare_profile_switch、~686取消、~702 commit、:744 try_begin_profile_switch_gate（再入→ProfileSwitchBusy拒否） |
| 11.5.6.5 controller | 実装済み | device/controller.rs:442 ControllerUpdate + :390 StickPolar + :337 apply()検証; dynamic/host.rs:128,133 controller_update/controller_reset（実装:682）; dynamic/protocol.rs HOST_CONTROLLER_{UPDATE,RESET}; 結合 runtime/python.rs:261 _Controller、runtime/lua.rs:272 controller={update,reset} |
| 11.5.6.6 commands（sort/tag_match/separator） | 実装済み | dynamic/command.rs:285 CommandRegistry（sort+tag_match安定slot）; :93 CommandOptionField{Priority,SoftTimeoutMs,...}; separator()+CommandDisplayItem/build_display_cache; worker_binary/dynamic/engine.rs:408 sort_commands/:422 tag_matches/:441 build_command_cache |

## 集計

- 実装済み 112 / 部分的 13 / 未実装 1 / 仕様のみ 1（計127。独立監査で2件の判定修正: 11.4.1.5→実装済み、11.4.3→部分的。7.8.4のSerialDisconnected mappingを現行source／testへ反映。総数は維持）
- 部分的13: §1.2 UI温存、§4.6.1参照専用、§4.6.2監視、§6.1.6自動選択、7.8.1 OOB、7.8.4 log、7.9.2 SHM、10.4.3 tk橋渡し、付録B、11.1 none、11.4.3、§14.6 LSP、11.5.1時期
- 未実装1: 7.9.6ベンチマーク
- 仕様のみ1: §1.2 frontend技術選択

## 旧引用の修正（前身からの差分）

系統的: `python.rs`→`src/worker_binary/script/python.rs`、`contract_sync.rs`→`tests/contract_sync.rs`、`protocol.json`→`registry/protocol.json`、裸`pipeline.rs/roots.rs`等→`src/settings/…`。
個別: security.rs :33→:26-27; scaffold正準はpersistence.rs:475（試験内）でなくscaffold.rs:42; selector ContainsNul :361→:362、空/NUL拒否 :25→:28-32、列挙優先 :239-243・負index拒否 :387を追加; tk Cleanup :611→:615; contract_sync McuCommand :1668→:1664、互換範囲:1668-1758→1664-1696; dynamic_runtime :95→:225/:280; host.rs :446→:435; uv build :512→:514; manifest :214→:222-224、:249→:254; hmac_key :22→:182; flake scaffold :3216/3310→:3318-3319（profileディレクトリ存在のみ）; package mutable_direct_sourceは定義:705（呼出:657）、override :271-280→:280-287; pipeline.rs:2319引用は未検証のため削除。§4.6.2は compatibility-roll.yml 実在を反映（判定时分的維持）。
独立監査修正: 7.9.4 manager NoWritableSlot :415→:437-439/:541-545（:415は起動失敗時無効化）; 7.9.5 getCameraImage定義 :3517-3518（:3247は利用側）; 7.9.2 pin生成 :240-255とPinGuard::drop :443-460を分離; 7.9.3 script_host.rs:624-630記述子配送を追加; 11.4.1.5 service.rs:337/:373-379で実装済みへ（:300は検証のみ）; 11.4.3 persistence.rs:380+:516でSPEC§11.4.3.3と乖離のため部分的へ; 11.3 venv証拠をregistry:3607-3613+pipeline:882-903+path:45へ; 11.4.2 :468→:1383; app_name :~898→:898; 射影試験 :2477→:2469/:2474/:2482; pyproject :41-52→:38-54+build.rs:102-103; roots MissingKnownFolder :144→:317; lua.rs:5はimportのみにつきCargo.toml:53を引用（LuaJIT 2.1厳密版は未検証）; 遅延venvはscript_runtime.rs:255/:122-166を引用; CLI/env自動導出の生成器は未確認; `compatibility/` はworktreeルート相対を明記。

## 対応が必要な未達（アクション）

1. 7.9.6 参照ベンチマーク harness の追加（未実装）。benches/criterion なし。CIはモックI/O性能ゲートとして配線。
2. 11.5.1 自動reload監視の実装（部分的の核）。`notify`/FileWatcher は `src/` ゼロヒット。明示 Reload のみ。
3. §14.6 LSP 7種の補完（部分的）。pylsp/pyrefly/ty の雛形・配線なし。
4. 10.4.3 tk橋渡しの Scale/get/config/再接続部分集合の証拠化または仕様整合（部分的）。
5. 付録B CommandMeta の do検査・切替シンボル欠落（部分的）。存在検証のみ。
6. §6.1.6 仕様OpenCV必須と実装nokhwa+v4lの乖離解消（部分的）。仕様改訂か実装変更のいずれか。
7. §10.7 Windows通知「実装済み」（仕様L992相当）。script表面に windows 通知シンボルなし。要確認（将来行は未実装/未検証見込）。
8. 7.8.1 TCP禁止の積極的証拠なし（部分的）。許可経路の列挙または試験追加。

## 行なし仕様規範（将来の行候補）

§1.2 機能優先/L44、単一dynamic根拠/L46、動的コールバック同時機械/L60-68、IPC内部/API等価/L71-72、全体init単発評価/L73、切替手順/L74、Tkinter固定/L76、venv exact-sync警告/L91、開発手順/L93-95。§4.6.2 全鎖再評価/L128、自動定義/L131。§10.3 bridge_functions非束縛、§10.4.3 saveCapture層差・MatLike、§10.6.1 Widget遷移、§10.7 Windows通知。§11.4.1.1 正準registry生成表面、§11.5.4.1 Python誤処理、§11.5.5.1 Lua runtime版、§11.5.5.2 Lua誤処理、§11.5.6.x 下位小節、§14.5.2.1・§14.5.5.1 下位規則、§12 変数毎既定/旗（POKECON_DYNAMIC_CALLBACK_*×5等）。
