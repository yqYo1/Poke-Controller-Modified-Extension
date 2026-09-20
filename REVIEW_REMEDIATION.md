# レビューfinding対応台帳

> 326ファイルのread-only詳細レビューで確定したfindingを、実装修正・検証・commit・pushまで追跡する作業用レビュー記録。製品仕様・利用者向けドキュメントではない。

## 1. 基準と運用

- 基準SHA: `1d0e1b4e54df087ec82aa2cc18b21d0725823a65`
- 対象branch: `refactor/rust-core`
- 詳細finding: ignored `CURRENT_IMPLEMENTATION_REVIEW.md`（各行のF番号・severity・evidence・fix案が正本）
- 本台帳: tracked。作業を跨いでも修正wave、commit、検証状態を失わないために使用する。
- 各findingは、`修正済み`、`証拠付き非該当`、`外部blocked`のいずれかへ遷移するまで完了扱いにしない。
- 既存仕様を変更する場合は、実装だけでなく仕様・API schema・生成物・テスト・CI・レビュー書を同一waveで更新する。

## 2. 全体進捗

- 確定finding: **455件**、findingを含むファイル: **139件**
- LGTMファイルとcredential-bearing `.envrc`のsafe-static reviewにはfinding対応を作らない。
- 現在の検証済み修正: **110 / 455 finding、40 / 139 finding-file**（Wave 1/2を継続中、Wave 3はCI timing／normal-ciの5件を完了）。
- Wave 1: **97 / 222 finding、33ファイル**。Wave 2: **8 / 110 finding、5ファイル**。Wave 3: **5 / 123 finding、2ファイル（timing.py、normal-ci.yml完了）**。
- 追加の実行契約修正: `flake.nix`のCargo target uv launcherをsymlinkからregular fileへ変更し、`settings::uv`のsymlink拒否とNix `contract-check`のdynamic startup fixtureを一致させた。これはflake行#1の5 finding完了数には加算しない。

## 3. 修正wave

| Wave | 対象 | ファイル数 | finding数 | 状態 | 完了証跡 |
|---|---|---:|---:|---|---|
| Wave 1 | Rust runtime、settings、worker、server、contracts、Rust tests | 65 | 222 | 部分完了（97/222） | realtime_connection focused compile/test passed; signed commits continue |
| Wave 2 | API、Python typings、Web、frontend tests | 37 | 110 | 部分完了（8/110） | camera-selector/realtime/runtime/settings Web packets; Bun tests/lint passed |
| Wave 3 | flake、CI、scripts、release、Python/tests | 37 | 123 | 部分完了（5/123） | timing p95 bootstrap、normal-ci timeout/action SHA、contract-sync・mutation・Nix contract passed |

## 4. ファイル別対応状態

| # | ファイル | finding数 | severity | Wave | 状態 | commit / 検証 |
|---:|---|---:|---|---|---|
| 1 | `flake.nix` | 5 | medium,medium,medium,low,low | Wave 3 | 未着手 | — |
| 6 | `python/pokecon/typings/Commands/PythonCommandBase.pyi` | 4 | medium,medium,medium,low | Wave 2 | 未着手 | — |
| 9 | `python/pokecon/typings/Commands/__init__.pyi` | 1 | medium | Wave 2 | 未着手 | — |
| 11 | `python/pokecon/typings/Commands/dialogue.pyi` | 4 | high,medium,medium,low | Wave 2 | 未着手 | — |
| 16 | `rust/pokecon/build.rs` | 2 | medium,low | Wave 1 | 未着手 | — |
| 17 | `rust/pokecon/linux/reload-udev.sh` | 2 | medium,low | Wave 1 | 未着手 | — |
| 18 | `rust/pokecon/src/application_backend.rs` | 1 | medium | Wave 1 | 修正済み・検証済み | application_backend focused test 3 passed; update-check HTTP/JSON deadlinesとdynamic-config error分類を実装 |
| 20 | `rust/pokecon/src/bin/generate_contracts.rs` | 3 | medium,medium,low | Wave 1 | 未着手 | — |
| 26 | `rust/pokecon/src/camera/manager.rs` | 4 | high,high,high,medium | Wave 1 | 修正済み・検証済み | camera manager tests: 5 passed; startup deadline、bounded queue、NoWritableSlot、timeout後shutdown所有権を修正 |
| 29 | `rust/pokecon/src/camera/native.rs` | 1 | major | Wave 1 | 修正済み・検証済み | Windows MediaFoundation source validation added: Closest candidate is accepted only after effective format/fps/resolution validation; Linux cfg compile unaffected |
| 34 | `rust/pokecon/src/command_service.rs` | 1 | high | Wave 1 | 未着手 | — |
| 36 | `rust/pokecon/src/contract_generator.rs` | 2 | low,low | Wave 1 | 未着手 | — |
| 38 | `rust/pokecon/src/contracts/dynamic_typings.rs` | 1 | medium | Wave 1 | 未着手 | — |
| 41 | `rust/pokecon/src/contracts/settings_artifacts.rs` | 3 | medium,medium,low | Wave 1 | 未着手 | — |
| 45 | `rust/pokecon/src/device/hardware.rs` | 1 | high | Wave 1 | 未着手 | — |
| 48 | `rust/pokecon/src/device/notification.rs` | 3 | high,high,medium | Wave 1 | 修正済み・検証済み | device::notification focused test 21 passed; HTTP timeout、delivery cancellation、stop drain priorityを実装 |
| 50 | `rust/pokecon/src/device/serial/manager.rs` | 1 | high | Wave 1 | 修正済み・検証済み | device::serial::manager focused test 10 passed; same-config apply no longer emits false disconnect without reconnect |
| 51 | `rust/pokecon/src/device/serial/mod.rs` | 2 | major,major | Wave 1 | 未着手 | — |
| 54 | `rust/pokecon/src/device/serial/virtual_port.rs` | 1 | high | Wave 1 | 修正済み・検証済み | virtual serial module compile passed; close race rechecks cancellation after delayed write |
| 57 | `rust/pokecon/src/dynamic/command.rs` | 3 | medium,medium,medium | Wave 1 | 未着手 | — |
| 59 | `rust/pokecon/src/dynamic/event.rs` | 5 | high,medium,medium,medium,medium | Wave 1 | 一部修正・検証済み | dynamic::event focused test 5 passed; once registration atomically consumed at snapshot, F2-F5 pending |
| 60 | `rust/pokecon/src/dynamic/host.rs` | 2 | high,high | Wave 1 | 未着手 | — |
| 63 | `rust/pokecon/src/dynamic/source.rs` | 1 | High | Wave 1 | 未着手 | — |
| 64 | `rust/pokecon/src/dynamic/transaction.rs` | 3 | high,medium,medium | Wave 1 | 未着手 | — |
| 66 | `rust/pokecon/src/dynamic_runtime.rs` | 4 | high,high,medium,medium | Wave 1 | 修正済み・focused検証済み | dynamic runtime tests passed; startup-post/initialize deadlines, HostStopping ordering、receiver drain orderingを修正 |
| 71 | `rust/pokecon/src/openapi_generator.rs` | 3 | medium,low,low | Wave 1 | 修正済み・検証済み | server::openapi focused test 5 passed; try_parse、artifact path IO context、bare filename parent handlingを実装 |
| 75 | `rust/pokecon/src/production.rs` | 3 | high,high,medium | Wave 1 | 修正済み・検証済み | production focused test passed; BuildCleanupでbuild失敗cleanup、abort後join、error source保持を実装 |
| 79 | `rust/pokecon/src/script_host.rs` | 3 | high,medium,medium | Wave 1 | 未着手 | — |
| 80 | `rust/pokecon/src/script_runtime.rs` | 3 | high,high,medium | Wave 1 | 修正済み・検証済み | script_runtime focused compile/test passed; generation reuse拒否、initialize deadline、receiver cleanup、stderr safe warningを実装 |
| 81 | `rust/pokecon/src/server/api.rs` | 2 | medium,medium | Wave 1 | 修正済み・検証済み | server::api focused test 7 passed; NormalizedRegion deserialize boundsとScriptUiAction generation validationを実装 |
| 82 | `rust/pokecon/src/server/backend.rs` | 3 | high,medium,medium | Wave 1 | 未着手 | — |
| 84 | `rust/pokecon/src/server/openapi.rs` | 3 | medium,medium,low | Wave 1 | 修正済み・検証済み | server::openapi 5 tests passed; all 13 discriminators and every properties node are closed; REST route binding test covers parity |
| 87 | `rust/pokecon/src/server/realtime_connection.rs` | 3 | high,medium,medium | Wave 1 | 修正済み・検証済み | realtime_connection focused compile/test passed; offer retention/consume ordering and cancellation-raced backend fixed |
| 90 | `rust/pokecon/src/server/rest/dynamic_config.rs` | 1 | medium | Wave 1 | 未着手 | — |
| 94 | `rust/pokecon/src/server/rest/script_ui.rs` | 1 | medium | Wave 1 | 修正済み・検証済み | application_backend::script_ui_failure maps TkObjectNotFound to 409 conflict; full lib gate pending |
| 100 | `rust/pokecon/src/server/state.rs` | 1 | high | Wave 1 | 修正済み・検証済み | server::state focused test 9 passed; pending_restart_values retain後deltaと削除nullを実装 |
| 102 | `rust/pokecon/src/server/webrtc.rs` | 3 | high,high,medium | Wave 1 | 修正済み・検証済み | server::webrtc focused test 4 passed; peer failure cleanup、activity drop、broadcast lag resyncを実装 |
| 103 | `rust/pokecon/src/server/websocket.rs` | 3 | medium,medium,medium | Wave 1 | 修正済み・検証済み | server::websocket focused test 23 passed; latest-only Pong、native Ping forwarding、protocol closeを実装 |
| 105 | `rust/pokecon/src/settings/lock.rs` | 4 | high,high,medium,medium | Wave 1 | 修正済み・検証済み | settings lock compile passed; bounded try_lock timeout、0700/0600 permissionsを実装 |
| 106 | `rust/pokecon/src/settings/manifest.rs` | 2 | low,low | Wave 1 | 修正済み・検証済み | manifest/venv compile and full lib passed; canonical path fingerprint now uses OS-native bytes token and read doc distinguishes resolution failures |
| 108 | `rust/pokecon/src/settings/package.rs` | 5 | medium,medium,low,low,low | Wave 1 | 修正済み・検証済み | settings::package focused test 8 passed; bounded clause search, Windows direct path normalization, empty base rejection, marker/empty index handlingを実装 |
| 109 | `rust/pokecon/src/settings/path.rs` | 5 | high,high,medium,medium,medium | Wave 1 | 修正済み・検証済み | settings::path focused test 4 passed; relative base escape、foreign Windows/device syntax、non-Unicode home、canonicalize-before-metadataを実装 |
| 110 | `rust/pokecon/src/settings/persistence.rs` | 9 | high,medium,medium,medium,medium,medium,low,low,low | Wave 1 | 修正済み・focused検証済み | persistence tests passed; semantic prevalidation、owner-only mode、quoted path、inline table、array/u64変換を修正 |
| 111 | `rust/pokecon/src/settings/pipeline.rs` | 4 | high,high,medium,medium | Wave 1 | 修正済み・focused検証済み | pipeline tests passed; raw non-Unicode passthrough、package resolution、resource-root staging、warning mergeを修正 |
| 112 | `rust/pokecon/src/settings/python.rs` | 8 | medium,medium,medium,low,low,low,low,low | Wave 1 | 未着手 | — |
| 113 | `rust/pokecon/src/settings/roots.rs` | 2 | medium,low | Wave 1 | 未着手 | — |
| 114 | `rust/pokecon/src/settings/scaffold.rs` | 6 | medium,low,low,low,low,low | Wave 1 | 未着手 | — |
| 115 | `rust/pokecon/src/settings/service.rs` | 3 | high,high,medium | Wave 1 | 修正済み・focused検証済み | service tests passed; persisted document direct commit、error propagation、profile path staging/live adapter rollbackを修正 |
| 116 | `rust/pokecon/src/settings/uv.rs` | 8 | medium,medium,medium,low,low,medium,low,low | Wave 1 | 一部修正・検証済み | settings::uv focused test 5 passed; SafeComponent version/path, metadata sha256 and symlink source validation fixed, remaining 5 findings pending |
| 117 | `rust/pokecon/src/settings/venv.rs` | 11 | high,high,high,medium,medium,medium,medium,medium,low,low,low | Wave 1 | 未着手 | — |
| 118 | `rust/pokecon/src/settings_runtime.rs` | 8 | high,medium,medium,medium,medium,medium,low,low | Wave 1 | 修正済み・focused検証済み | settings_runtime tests passed; bounded runtime bridge、rollback Result、reconcile baseline、Profile adaptersを修正 |
| 121 | `rust/pokecon/src/tests/ui_boundary_acceptance.rs` | 7 | high,high,medium,medium,medium,low,low | Wave 1 | 未着手 | — |
| 129 | `rust/pokecon/src/worker/script/mod.rs` | 1 | medium | Wave 1 | 未着手 | — |
| 130 | `rust/pokecon/src/worker/script/protocol.rs` | 7 | high,high,high,medium,medium,medium,medium | Wave 1 | 一部修正・検証済み | ScriptExecutionOutcome now deny_unknown_fields; payload bounds F2/F3 pending |
| 131 | `rust/pokecon/src/worker/supervisor.rs` | 6 | high,medium,medium,medium,medium,low | Wave 1 | 修正済み・focused検証済み | supervisor tests compiled; role spawn gate、failed-launch rollback/reap、bounded stop/reaper、JoinSet role trackingを修正 |
| 133 | `rust/pokecon/src/worker_binary/dynamic/engine.rs` | 6 | medium,medium,medium,high,medium,low | Wave 1 | 一部修正・検証済み | coordinator barrier保持とcaller generation Superseded guard、callback内source/profile switchのbounded defer/drainを実装。dynamic lifecycle bidirectional IPC、engine回帰、`nix run .#cargo -- test -p pokecon --all-features --lib` 453 passed。残り4件は未着手 |
| 135 | `rust/pokecon/src/worker_binary/dynamic/runtime/lua.rs` | 7 | high,high,high,medium,medium,medium,medium | Wave 1 | 一部修正・検証済み | Lua restricted stdlib sandboxを実装、残り6件は未着手 |
| 137 | `rust/pokecon/src/worker_binary/dynamic/runtime/python.rs` | 7 | high,high,high,medium,medium,medium,medium | Wave 1 | 一部修正・検証済み | F1をtrusted-code契約として`docs/DYNAMIC_CONFIGURATION.md`へ明示、F3を`e649bfd`で修正。Python source evaluationを`spawn_blocking`へ移し、EvaluationScopeを伝搬、coordinatorを評価中に解放、generation再検証を追加。adversarial coordinator test、engine 8 tests、full lib 454 passed、contract-check成功。F2/F4/F5/F6/F7は未着手。 |
| 140 | `rust/pokecon/src/worker_binary/script/python.rs` | 6 | medium,high,medium,medium,low,low | Wave 1 | 未着手 | — |
| 142 | `rust/pokecon/tests/contract_sync.rs` | 2 | high,medium | Wave 1 | 修正済み・検証済み | contract_sync with integration-test-support: 12 passed; canonical inventory JSON and CI timing threshold cross-check aligned |
| 144 | `rust/pokecon/tests/cross_process.rs` | 2 | medium,low | Wave 1 | 未着手 | — |
| 146 | `rust/pokecon/tests/lifecycle.rs` | 2 | high,high | Wave 1 | 未着手 | — |
| 147 | `rust/pokecon/tests/native_serial_pty.rs` | 2 | medium,low | Wave 1 | 未着手 | — |
| 149 | `rust/pokecon/tests/script_runtime.rs` | 1 | medium | Wave 1 | 未着手 | — |
| 151 | `rust/pokecon/tests/support/mod.rs` | 1 | medium | Wave 1 | 未着手 | — |
| 155 | `scripts/acceptance/records.py` | 2 | high,medium | Wave 3 | 未着手 | — |
| 160 | `scripts/ci/timing.py` | 2 | medium,low | Wave 3 | 修正済み・検証済み | `fb62a63`; `nix develop -c ... pytest -q tests/quality/test_ci_timing.py`: 51 passed、empty p95 failure document、720s contract、bootstrap workflow assertionsを検証。 |
| 162 | `scripts/compatibility/inventory.py` | 4 | medium,low,low,low | Wave 3 | 未着手 | — |
| 163 | `scripts/compatibility/promote.py` | 5 | high,high,medium,medium,medium | Wave 3 | 未着手 | — |
| 164 | `scripts/compatibility/roll.py` | 2 | high,medium | Wave 3 | 未着手 | — |
| 165 | `scripts/compatibility/runner.py` | 6 | high,high,medium,medium,medium,low | Wave 3 | 未着手 | — |
| 166 | `scripts/integration/editor_lsp_smoke.py` | 2 | high,medium | Wave 3 | 未着手 | — |
| 168 | `scripts/integration/pidfd_signal.py` | 1 | Medium | Wave 3 | 未着手 | — |
| 169 | `scripts/integration/proc_socket_evidence.py` | 1 | medium | Wave 3 | 未着手 | — |
| 171 | `scripts/integration/virtual-io-smoke.sh` | 3 | medium,medium,low | Wave 3 | 未着手 | — |
| 172 | `scripts/performance/benchmark.py` | 6 | high,high,medium,medium,medium,medium | Wave 3 | 未着手 | — |
| 176 | `scripts/quality/run_parallel_checks.py` | 4 | medium,low,medium,low | Wave 3 | 未着手 | — |
| 177 | `scripts/quality/source_filter.py` | 4 | high,medium,medium,low | Wave 3 | 未着手 | — |
| 180 | `scripts/release/build_runtime.py` | 10 | high,high,high,high,medium,medium,medium,medium,medium,medium | Wave 3 | 未着手 | — |
| 182 | `scripts/release/debian_install_smoke.sh` | 2 | medium,medium | Wave 3 | 未着手 | — |
| 185 | `scripts/release/normalize_linux_elf.py` | 3 | medium,medium,medium | Wave 3 | 未着手 | — |
| 187 | `scripts/release/package_smoke.py` | 4 | high,medium,medium,medium | Wave 3 | 未着手 | — |
| 188 | `scripts/release/signing_manifest.py` | 2 | high,medium | Wave 3 | 未着手 | — |
| 189 | `scripts/release/stage.py` | 3 | high,medium,medium | Wave 3 | 未着手 | — |
| 190 | `scripts/release/windows_install_smoke.ps1` | 4 | medium,medium,medium,low | Wave 3 | 未着手 | — |
| 194 | `tests/quality/security/test_binary_cache_trust.py` | 5 | high,high,medium,medium,low | Wave 3 | 未着手 | — |
| 195 | `tests/quality/test_bun_toolchain.py` | 4 | high,medium,medium,low | Wave 3 | 未着手 | — |
| 202 | `tests/quality/test_parallel_checks.py` | 3 | high,medium,medium | Wave 3 | 未着手 | — |
| 203 | `tests/quality/test_pidfd_signal.py` | 2 | high,medium | Wave 3 | 未着手 | — |
| 206 | `tests/quality/test_source_filter.py` | 2 | medium,high | Wave 3 | 未着手 | — |
| 208 | `tests/quality/test_ui_package_check.py` | 7 | high,high,medium,medium,medium,medium,medium | Wave 3 | 未着手 | — |
| 209 | `tests/quality/test_version_contract.py` | 4 | high,medium,medium,medium | Wave 3 | 未着手 | — |
| 211 | `tests/release/test_debian_install_smoke.py` | 2 | medium,medium | Wave 3 | 未着手 | — |
| 212 | `tests/release/test_gate.py` | 3 | high,medium,medium | Wave 3 | 未着手 | — |
| 213 | `tests/release/test_normalize_debian_package.py` | 2 | medium,medium | Wave 3 | 未着手 | — |
| 215 | `tests/release/test_package_smoke.py` | 1 | high | Wave 3 | 未着手 | — |
| 216 | `tests/release/test_signing_manifest.py` | 3 | high,medium,medium | Wave 3 | 未着手 | — |
| 218 | `tests/release/test_windows_install_smoke.py` | 5 | medium,medium,high,medium,medium | Wave 3 | 未着手 | — |
| 223 | `web/src/app.html` | 1 | medium | Wave 2 | 未着手 | — |
| 225 | `web/src/lib/actions.ts` | 2 | high,low | Wave 2 | 未着手 | — |
| 228 | `web/src/lib/camera-selector.test.ts` | 1 | major | Wave 2 | 修正済み・検証済み | Nix devShell Bun camera-selector test 3 passed; tests assert index/path equivalent option keys |
| 229 | `web/src/lib/camera-selector.ts` | 1 | high | Wave 2 | 修正済み・検証済み | Nix devShell Bun camera-selector test 3 passed; option key now uses normalized camera identity |
| 230 | `web/src/lib/components/AnalogStick.svelte` | 5 | high,medium,medium,medium,low | Wave 2 | 未着手 | — |
| 231 | `web/src/lib/components/CameraTab.svelte` | 5 | high,high,medium,medium,low | Wave 2 | 未着手 | — |
| 232 | `web/src/lib/components/CameraTab.test.ts` | 3 | high,medium,medium | Wave 2 | 未着手 | — |
| 233 | `web/src/lib/components/CameraViewport.svelte` | 3 | high,high,medium | Wave 2 | 未着手 | — |
| 235 | `web/src/lib/components/CommandsTab.svelte` | 6 | high,high,medium,medium,medium,low | Wave 2 | 未着手 | — |
| 237 | `web/src/lib/components/ControllerPanel.svelte` | 5 | high,medium,medium,medium,medium | Wave 2 | 未着手 | — |
| 239 | `web/src/lib/components/InputSafety.svelte` | 3 | high,medium,medium | Wave 2 | 未着手 | — |
| 240 | `web/src/lib/components/InputSafety.test.ts` | 4 | medium,medium,low,high | Wave 2 | 未着手 | — |
| 241 | `web/src/lib/components/MainPanel.svelte` | 4 | medium,low,low,low | Wave 2 | 未着手 | — |
| 243 | `web/src/lib/components/ManualTab.test.ts` | 3 | high,medium,medium | Wave 2 | 未着手 | — |
| 244 | `web/src/lib/components/NotificationsTab.svelte` | 2 | medium,low | Wave 2 | 未着手 | — |
| 246 | `web/src/lib/components/OtherTab.svelte` | 6 | medium,medium,low,low,low,low | Wave 2 | 未着手 | — |
| 248 | `web/src/lib/components/OutputGroup.svelte` | 2 | medium,low | Wave 2 | 未着手 | — |
| 249 | `web/src/lib/components/OutputGroup.test.ts` | 2 | medium,low | Wave 2 | 未着手 | — |
| 250 | `web/src/lib/components/OutputPanel.svelte` | 2 | medium,low | Wave 2 | 未着手 | — |
| 253 | `web/src/lib/components/ScriptDialog.svelte` | 4 | high,high,medium,medium | Wave 2 | 未着手 | — |
| 254 | `web/src/lib/components/ScriptTkWindow.svelte` | 1 | medium | Wave 2 | 未着手 | — |
| 255 | `web/src/lib/components/ScriptUiLayer.svelte` | 3 | high,medium,medium | Wave 2 | 未着手 | — |
| 256 | `web/src/lib/components/ScriptUiLayer.test.ts` | 1 | medium | Wave 2 | 未着手 | — |
| 257 | `web/src/lib/components/SerialTab.svelte` | 3 | medium,low,medium | Wave 2 | 未着手 | — |
| 259 | `web/src/lib/components/TouchPad.svelte` | 3 | medium,medium,low | Wave 2 | 未着手 | — |
| 268 | `web/src/lib/input.ts` | 1 | medium | Wave 2 | 未着手 | — |
| 272 | `web/src/lib/realtime.ts` | 1 | high | Wave 2 | 修正済み・検証済み | Nix devShell Bun realtime test 7 passed; revision-gap trigger change preserved through resync |
| 275 | `web/src/lib/runtime.test.ts` | 5 | medium,medium,medium,low,low | Wave 2 | 未着手 | — |
| 276 | `web/src/lib/runtime.ts` | 5 | high,medium,medium,medium,medium | Wave 2 | 一部修正・検証済み | Nix devShell Bun runtime test 5 passed; subscriber isolation F1/F2 fixed, lifecycle F3-F5 remain |
| 278 | `web/src/lib/settings.ts` | 4 | high,medium,medium,medium | Wave 2 | 一部修正・検証済み | Nix devShell Bun settings test 6 passed; subscriber isolation/stale refresh/bounded retired IDs fixed, F4 pending |
| 280 | `web/src/lib/vite-config.test.ts` | 3 | medium,medium,low | Wave 2 | 未着手 | — |
| 286 | `web/src/routes/page.test.ts` | 4 | medium,medium,medium,low | Wave 2 | 未着手 | — |
| 292 | `.github/workflows/normal-ci.yml` | 3 | high,medium,medium | Wave 3 | 修正済み・検証済み | `5471e53`; 10件未満のtiming historyはwarning付きでgate未適用、9 jobにtimeout-minutes、mutable actionをcommit SHA固定。`nix run .#actionlint`、quality 39 passed、contract-sync 14 passed、contract-check成功。 |
| 293 | `.github/workflows/package.yml` | 1 | medium | Wave 3 | 未着手 | — |
| 299 | `api/openapi.json` | 1 | major | Wave 2 | 未着手 | — |
| 310 | `rust-toolchain.toml` | 1 | high | Wave 3 | 未着手 | — |
| 311 | `rust/pokecon/Cargo.toml` | 3 | high,medium,low | Wave 1 | 未着手 | — |
| 313 | `rust/pokecon/registry/acceptance-record.schema.json` | 4 | medium,medium,medium,medium | Wave 1 | 未着手 | — |
| 314 | `rust/pokecon/registry/ci.json` | 4 | high,high,medium,medium | Wave 1 | 未着手 | — |
| 317 | `rust/pokecon/registry/generation.json` | 1 | major | Wave 1 | 未着手 | — |
| 323 | `web/src/lib/api/openapi.json` | 2 | high,high | Wave 2 | 未着手 | — |

## 5. 完了判定

1. findingごとの実装または証拠付き非該当判定をCURRENT_IMPLEMENTATION_REVIEW.mdへ反映する。
2. 対応箇所のfocused testをNix経由で実行し、必要なadversarial/no-network testを追加する。
3. `nix fmt -- --ci`、該当lint、該当test、`git diff --check`を成功させる。
4. 自然な機能単位で署名commit・pushし、台帳へcommit SHAを記録する。
5. push後は最新SHAのCIを`nix run .#ci-watch -- refactor/rust-core <timeout>`で監視し、失敗を次waveへ持ち越さない。

## 6. 外部要因

- 実機hardware acceptanceは対象外。Linux、Nix、virtual serial、PTY、CIで検証する。
- 性能thresholdや実測値は捏造しない。不足サンプル、未完了run、既存の別経路artifactを代用しない。
- release tagは利用者担当であり、この台帳の完了条件に含めない。
- credential-bearing fileの値は出力・保存・要約せず、必要な場合はsafe-static inspectionのみ行う。
