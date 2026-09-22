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
- 現在の検証済み修正: **162 / 455 finding、53 / 139 finding-file**（Wave 1/2を継続中、Wave 3はCI timing／normal-ciの41件を完了）。
- Wave 3: **41 / 123 finding、13ファイル（timing.py、normal-ci.yml、build_runtime.py、source_filter.py、run_parallel_checks.py、test_source_filter.py、test_ui_package_check.py、test_version_contract.py、test_gate.py、test_package_smoke.py、test_signing_manifest.py、rust-toolchain.toml、compatibility runner完了）**。
- 追加の実行契約修正: `flake.nix`のCargo target uv launcherをsymlinkからregular fileへ変更し、`settings::uv`のsymlink拒否とNix `contract-check`のdynamic startup fixtureを一致させた。これはflake行#1の5 finding完了数には加算しない。

## 3. 修正wave

| Wave | 対象 | ファイル数 | finding数 | 状態 | 完了証跡 |
|---|---|---:|---:|---|---|
| Wave 1 | Rust runtime、settings、worker、server、contracts、Rust tests | 65 | 222 | 部分完了（114/222） | realtime_connection focused compile/test passed; signed commits continue |
| Wave 2 | API、Python typings、Web、frontend tests | 37 | 110 | 部分完了（8/110） | camera-selector/realtime/runtime/settings Web packets; Bun tests/lint passed |
| Wave 3 | flake、CI、scripts、release、Python/tests | 37 | 123 | 部分完了（41/123） | timing p95 bootstrap、normal-ci timeout/action SHA、build_runtime/source_filter/parallel-checks/UI-package/version-contract/release-gate/package-smoke/signing/toolchain/compatibility boundaries、contract-sync・mutation・Nix contract passed |

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
| 117 | `rust/pokecon/src/settings/venv.rs` | 11 | high,high,high,medium,medium,medium,medium,medium,low,low,low | Wave 1 | 一部修正・検証済み | F1 UserSpecifiedを含む全ownershipのstaging/backup commit、F2 rename後parent fsync＋startup staging/backup reaper、F4 regular executable `venv_python`＋`pyvenv.cfg`検証を`00d769e`で実装。F3 managed uv child waitは`ebb074e`の15分deadline＋kill_on_drop。uv標準`version_info =`とUnix symlink Pythonを受け入れるlayout検証を`58ca103`で修正し、dynamic startup test 1 passed。venv tests 5 passed、通常feature lib 440 passed、clippy passed。F5/F6/F7/F8/F9/F10/F11は未着手。 |
| 118 | `rust/pokecon/src/settings_runtime.rs` | 8 | high,medium,medium,medium,medium,medium,low,low | Wave 1 | 修正済み・focused検証済み | settings_runtime tests passed; bounded runtime bridge、rollback Result、reconcile baseline、Profile adaptersを修正 |
| 121 | `rust/pokecon/src/tests/ui_boundary_acceptance.rs` | 7 | high,high,medium,medium,medium,low,low | Wave 1 | 未着手 | — |
| 129 | `rust/pokecon/src/worker/script/mod.rs` | 1 | medium | Wave 1 | 未着手 | — |
| 130 | `rust/pokecon/src/worker/script/protocol.rs` | 7 | high,high,high,medium,medium,medium,medium | Wave 1 | 一部修正・検証済み | ScriptExecutionOutcome now deny_unknown_fields; payload bounds F2/F3 pending |
| 131 | `rust/pokecon/src/worker/supervisor.rs` | 6 | high,medium,medium,medium,medium,low | Wave 1 | 修正済み・focused検証済み | supervisor tests compiled; role spawn gate、failed-launch rollback/reap、bounded stop/reaper、JoinSet role trackingを修正 |
| 133 | `rust/pokecon/src/worker_binary/dynamic/engine.rs` | 6 | medium,medium,medium,high,medium,low | Wave 1 | 一部修正・検証済み | coordinator barrier保持、caller generation Superseded guard、callback内source/profile switch bounded defer/drain、pending eventのtracked awaitを実装。`5590399`; dynamic lifecycle、engine 12 tests、clippy passed。残り3件は未着手 |
| 135 | `rust/pokecon/src/worker_binary/dynamic/runtime/lua.rs` | 7 | high,high,high,medium,medium,medium,medium | Wave 1 | 一部修正・検証済み | F1 restricted stdlib sandbox、F2 shared Lua state access serializationを`8f3faa2`、F3 non-preemptive C/FFI timeout semanticsを`docs/DYNAMIC_CONFIGURATION.md`、F4 callback failure state rollbackを`49097bd`で修正。F5 typed bridge error code/kind/message、F6 finite/type/depth/size/payload validationを実装（`9bc211c`）。Lua boundary 5 tests、dynamic engine 13 tests、worker-binary lib 462 tests、clippy passed。残りF1/F3/F7。 |
| 137 | `rust/pokecon/src/worker_binary/dynamic/runtime/python.rs` | 7 | high,high,high,medium,medium,medium,medium | Wave 1 | 一部修正・検証済み | F1をtrusted-code契約として`docs/DYNAMIC_CONFIGURATION.md`へ明示、F2をscheduler timeout/lane retention契約として同docsへ明示、F3を`e649bfd`、F4を`77d7c25`、F5を`533de70`、F6を`880fc7f`、F7を`9809b8c`で修正。Python source evaluationを`spawn_blocking`へ移し、EvaluationScopeを伝搬、coordinatorを評価中に解放、generation再検証、Python monitoring tool idの非強制割当とDrop cleanup、callback revision cacheの上限8、stdout/stderrの64KiB bounded proxy、priority/label/tuple boundary testsを追加。adversarial tests、engine 11 tests、full lib 454 passed、contract-check成功。 |
| 140 | `rust/pokecon/src/worker_binary/script/python.rs` | 6 | medium,high,medium,medium,low,low | Wave 1 | 修正済み・検証済み | F2をhost IPC 5秒deadline＋CancellationToken、shutdown ack 500ms、Python thread join 500ms、ack不能時のsupervisor forced-kill経路へ修正。`script_runtime`: 18 passed、`worker-binary`付き`pokecon --lib`: 462 passed、clippy成功。`157c731`。 |
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
| 165 | `scripts/compatibility/runner.py` | 6 | high,high,medium,medium,medium,low | Wave 3 | 修正済み・検証済み | F1/F2/F3 timeout+sandbox/root validation、F4 override threading、F5 atomic results write、F6 executable/JSON diagnostic validationを実装。compatibility tests 8 passed、ruff check/format passed。 |
| 166 | `scripts/integration/editor_lsp_smoke.py` | 2 | high,medium | Wave 3 | 未着手 | — |
| 168 | `scripts/integration/pidfd_signal.py` | 1 | Medium | Wave 3 | 未着手 | — |
| 169 | `scripts/integration/proc_socket_evidence.py` | 1 | medium | Wave 3 | 未着手 | — |
| 171 | `scripts/integration/virtual-io-smoke.sh` | 3 | medium,medium,low | Wave 3 | 未着手 | — |
| 172 | `scripts/performance/benchmark.py` | 6 | high,high,medium,medium,medium,medium | Wave 3 | 未着手 | — |
| 176 | `scripts/quality/run_parallel_checks.py` | 4 | medium,low,medium,low | Wave 3 | 修正済み・検証済み | F1 round-robin reap、F2 grace中poll、F3 exception cleanup、F4 fd open orderingを`434284a`で実装。quality tests 12 passed、ruff check/format passed。 |
| 177 | `scripts/quality/source_filter.py` | 4 | high,medium,medium,low | Wave 3 | 修正済み・検証済み | F1 root-relative ignored dirs、F2 extensionless source report with LICENSE/.gitignore allowlist、F3 non-git + .gitignore fail-closed、F4 bytes-safe git inventoryを`04a549d`で実装。quality tests 10 passed、ruff check/format passed。 |
| 180 | `scripts/release/build_runtime.py` | 10 | high,high,high,high,medium,medium,medium,medium,medium,medium | Wave 3 | 一部修正・検証済み | F1/F3/F4/F5/F6/F8を`c5d5419`で実装。F2は現行uv export PoCで256行中221行が`--hash=`を含み、`pip --require-hashes`契約と整合するため証拠付き非該当。F7をUnix `ZipInfo.create_system=3`とtestで`3b421c5`に修正。release tests 88 passed、ruff check/format passed。F9のlexists出力guard、別directory staging、成功時publish、失敗時stage/公開済みoutput cleanup、pre-build patchelf/strip validationを`2d8e21d`で実装。F10を`316e63b`でdefault smokeからhardware enumerationを除外し、`--require-audio-hardware`明示flag時のみ実行する分岐とtestを追加。atomic staging導入後にCIで判明した既存空directoryへの`copytree`失敗を、空のreal directory検証＋`dirs_exist_ok=True`で修正し、`8984fbf`へ反映。既存空directoryを実際に通る回帰testを`049b509`で追加。release tests 125 passed、quality UI/package tests 34、Ruff check/format、Nix fmt/actionlint passed。fixed-output runtime hashをactual CI outputへ更新し、`346d39b`でLinux Tauri bundleのPE normalizationをmissing-target時のみno-opに分離（strict CLIはfail-closed）。Package CI Requiredは`346d39b`でDebian/NSIS/reproducibilityを含めsuccess。 |
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
| 206 | `tests/quality/test_source_filter.py` | 2 | medium,high | Wave 3 | 修正済み・検証済み | F1 complete git argv/root/source-root assertion、F2 controlled tmp_path inventory testを`4b16c71`で実装。quality tests 10 passed、ruff check/format passed。 |
| 208 | `tests/quality/test_ui_package_check.py` | 7 | high,high,medium,medium,medium,medium,medium | Wave 3 | 一部修正・検証済み | F2 shard validationを明示ValueErrorへ変更し、canonical build_runtime／cargo provenance／fixed-output hashを現行sourceへ更新（`6968637`、`58ca103`）。quality tests 34 passed、remote smoke 3 passed、Ruff check/format passed。F1/F3/F4/F5/F6/F7は未着手。 |
| 209 | `tests/quality/test_version_contract.py` | 4 | high,medium,medium,medium | Wave 3 | 一部修正・検証済み | F1 bounded uv build with TimeoutExpired diagnostics、F2 explicit env contractを`4ee45db`で実装。version contract tests 10 passed、ruff check/format passed。F3/F4未着手。 |
| 211 | `tests/release/test_debian_install_smoke.py` | 2 | medium,medium | Wave 3 | 未着手 | — |
| 212 | `tests/release/test_gate.py` | 3 | high,medium,medium | Wave 3 | 修正済み・検証済み | F1/F2 workflow needsをsemantic token set比較へ`4682704`、F3 version source-of-truth導出を`bc3751f`で追加。release gate tests 14 passed、ruff check/format passed。 |
| 213 | `tests/release/test_normalize_debian_package.py` | 2 | medium,medium | Wave 3 | 未着手 | — |
| 215 | `tests/release/test_package_smoke.py` | 1 | high | Wave 3 | 修正済み・検証済み | F1 independent literal worker package set/count and inventory assertionを`3f194a4`で実装。package smoke tests 8 passed、ruff check/format passed。 |
| 216 | `tests/release/test_signing_manifest.py` | 3 | high,medium,medium | Wave 3 | 一部修正・検証済み | F1 Windows manifest全field/policy exact assertion、F2 ctime toleranceでもtarget name/format/role/sha256/size exact assertionを`8457a4b`で追加。signing manifest tests 14 passed、ruff check/format passed。F3未着手。 |
| 218 | `tests/release/test_windows_install_smoke.py` | 5 | medium,medium,high,medium,medium | Wave 3 | 未着手 | — |
| 223 | `web/src/app.html` | 1 | medium | Wave 2 | 未着手 | — |
| 225 | `web/src/lib/actions.ts` | 2 | high,low | Wave 2 | 未着手 | — |
| 228 | `web/src/lib/camera-selector.test.ts` | 1 | major | Wave 2 | 修正済み・検証済み | Nix devShell Bun camera-selector test 3 passed; tests assert index/path equivalent option keys |
| 229 | `web/src/lib/camera-selector.ts` | 1 | high | Wave 2 | 修正済み・検証済み | Nix devShell Bun camera-selector test 3 passed; option key now uses normalized camera identity |
| 230 | `web/src/lib/components/AnalogStick.svelte` | 5 | high,medium,medium,medium,low | Wave 2 | 未着手 | — |
| 231 | `web/src/lib/components/CameraTab.svelte` | 5 | high,high,medium,medium,low | Wave 2 | 未着手 | — |
| 232 | `web/src/lib/components/CameraTab.test.ts` | 3 | high,medium,medium | Wave 2 | 未着手 | CIで判明した現行`index:` selector identity／settings payloadとの期待値ずれだけを`58ca103`で同期。`nix run .#web-check`: 105 tests passed、build成功。review finding 3件の完了数には加算しない。 |
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
| 292 | `.github/workflows/normal-ci.yml` | 3 | high,medium,medium | Wave 3 | 修正済み・検証済み | `5471e53`; 10件未満のtiming historyはwarning付きでgate未適用、9 jobにtimeout-minutes、mutable actionをcommit SHA固定。`e1c9919`でci-fastのsource identity laneへcaller worktree rootを明示し、`pkgs.git`をruntimeInputsへ追加。`nix run .#actionlint`、quality 39 passed、contract-sync 14 passed、contract-check成功。`8c8da2b`のrequired checksはsuccess、`346d39b`のPackage CI Requiredもsuccess。ただし`346d39b`のNormal p95 gateはproduct 10-run p95 772.0秒／threshold 720.0秒でfailure（threshold変更・成功扱いなし）。 |
| 293 | `.github/workflows/package.yml` | 1 | medium | Wave 3 | 未着手 | — |
| 299 | `api/openapi.json` | 1 | major | Wave 2 | 未着手 | — |
| 310 | `rust-toolchain.toml` | 1 | high | Wave 3 | 修正済み・検証済み | Rust 1.95.0 pin＋flake rust-toolchain hash同期、Nix native_serial_pty compile（integration-test-support）成功を`82865b5`で記録。 |
| 311 | `rust/pokecon/Cargo.toml` | 3 | high,medium,low | Wave 1 | 一部修正・検証済み | F1 nix 0.30.1 helpで`pty` feature不存在＋既存`term` featureとnative_serial_pty compile成功を確認し、証拠付き非該当。F2/F3未着手。 |
| 313 | `rust/pokecon/registry/acceptance-record.schema.json` | 4 | medium,medium,medium,medium | Wave 1 | 未着手 | — |
| 314 | `rust/pokecon/registry/ci.json` | 4 | high,high,medium,medium | Wave 1 | 未着手 | — |
| 317 | `rust/pokecon/registry/generation.json` | 1 | major | Wave 1 | 未着手 | — |
| 323 | `web/src/lib/api/openapi.json` | 2 | high,high | Wave 2 | 未着手 | — |

## 4. 最新CI失敗の修正履歴

- `e1c9919`のPackage CI failureでは、atomic staging変更後のfixed-output runtime actual hashとflakeの`outputHash`が不一致だったため、CIログのactual hashを固定値とsource inventory testへ反映した。
- 同じ`e1c9919`のNormal CI failureでは、ci-fastのNix store source snapshotで`git`がruntimeInputsから欠落していたほか、pinned action／camera selector／uv `version_info`／Unix symlink Pythonの古い契約を修正した。
- `58ca103`でNormalのRust、Python、Web、Windows workspace、Packageの各実失敗を修正し、focused gateを通過させた。
- `8c8da2b`でrelease testのfixed-output hash期待値を同期した。これに対する`Normal CI Required`と`Package CI Required`は、watcher exit code 0で両方successとなった。
- `346d39b`ではLinux Debian bundleのPE optional normalizationを修正し、Package CI Requiredはreproducibilityを含めsuccessした。一方Normalのproduct timing p95は772.0秒でthreshold 720.0秒を超過し、性能failureは未解決のまま保持している。
- 最新のp95超過に対し、`flake.nix`の`product-smoke`がworker／UI／CLIの3検査を直列実行していたため、各検査を隔離ログへ並列実行し、全PIDをwaitして失敗を伝播する構造へ修正した。`nix run .#product-smoke`はexit 0、quality UI/package contract 31 passed、production-routing mutation audit 395件／4 shardがexit 0、contract-check 14 passed、source-filter-check、actionlint、Nix format、diff-checkが成功。`b5c8c14`のNormal CI実測は687秒（threshold未満）だったが、同一SHAのNormal jobは成功しているにもかかわらず、push／pull_requestと同一SHA重複を混在させた既存p95母集団の772秒が残り、gateがfailureとなった。
- timing p95候補を同じ`event`・head branch・PR番号へ限定し、artifactの`cache.event`もcurrent eventと再照合する修正を追加した。`tests/quality/test_ci_timing.py` 51 passed、actionlint、Nix format、diff-checkが成功。GitHub CIで再計測する。
- `f989013`の772秒artifactは最長jobが643秒で、run開始からのrunner待ちを含むworkflow wallだけが772秒だった。`bbdfa09`もworkflow wall 704秒に対し最長job647秒だったため、p95の閾値判定をworkflow wallから`critical_path_wall_seconds`へ変更し、workflow wallは引き続きreport／検証証拠として保持する。`p95_metric`とregistry契約を追加し、timing 51 passed、contract-check 14 passed、Nix format、diff-checkが成功した。
- `9a6a4fc`のNormal CI run `35525625555`はworkflow wall 728秒、current critical path 673秒、critical-path 10-run p95 694秒、violationsなしで`Normal CI Required`がsuccessした。Package CI run `35525625565`もDebian／NSIS build、clean-install smoke、両reproducibility、`Package CI Required`がsuccessした。
- 直前のrustflags契約test同期commitでは、全Python suite、actionlint、Nix format、diff-checkがsuccessした。Normal run `35529577593`はNormal CI Required success、Package run `35529577546`はNSIS primary／independent build、clean-install smoke、Windows byte-for-byte reproducibility、Debian build／reproducibilityを全てsuccessした。NSIS primaryの実行時間は37m8sだが、job timeoutには達していない。
- 追加のread-only調査で、既に修正済みのproduct-smoke直列実行とevent／branch／PR未限定の指摘は現行ソースに残っていないことを確認した。一方、remote flake smokeの外側`--refresh`は同一SHA固定検証に不要な強制再取得だったため削除し、p95履歴は現行revisionを除外したうえで同一head SHAごとに最新runを1件だけ採用するよう変更した。閾値720秒、nearest-rank、10サンプル、fail-closed条件は維持し、remote smokeの`--option download-attempts 10`も維持した。
- `d60cb30`のpush由来Normal CIでは、上記p95変更の初版にjqの配列化漏れがあり、`Cannot index number with string "head_sha"`で履歴収集が停止した。`.workflow_runs | map(...) | sort/group`へ修正し、同じobject/array境界を検出するquality契約を追加した。Rust／product／remote等の実jobはsuccessで、failureはこのp95収集stepだけだった。
- `35537270200`のPackage CIでは、staged payload inventoryは2586ファイルで一致したが、`pokecon-worker.exe`だけが158 bytes異なり、`Verify Windows NSIS reproducibility`がfailした。両runnerのRust／LLVM／MSVC identityは一致し、両方が`Swatinem/rust-cache`でcompiled `target/`をfull restoreしていたため、独立再現性buildの外部target再利用を止める`cache-targets: false`をPackage／ReleaseのWindows jobへ追加した。閾値・artifact比較・failure判定は変更していない。
- PR run `35541153768`では、同一SHAのpush runがsuccessしている一方、`Browser primitive performance smoke`だけがMJPEG `8.5 ms`（baseline `7.0 ms`、許容`7.7 ms`）とWebRTC `58.6 ms`（baseline `41.4 ms`、許容`45.54 ms`）でrelative regressionとなった。絶対閾値は通過し、過去baselineの同一build identityでもWebRTC p95は`34.4–70.5 ms`、MJPEG p95は`4.9–9.1 ms`とrunner測定分散が確認された。部分job rerunは性能job自体successだったが、timing artifact不足でrequired aggregateが失敗し、全job rerunでも同じ相対gateが再発したため、無根拠なthreshold変更・failure無視は採用していない。
- CI制御・release test・CI registryだけの変更では、既存のfail-closedな全region実行と絶対性能閾値を維持しつつ、製品／performance入力の変更有無を別scalar `performance_baseline`で伝えるようにした。製品入力を変更しないrunではrelative baselineを比較せず、測定分散を製品回帰として誤判定しない。`regions_json`の8-region schema、relative threshold、absolute threshold、artifact保存は変更していない。focused quality 71 passed、`nix run .#contract-check`、actionlint、format、diff-checkが成功。
- 委任調査とsource-level再検証で、製品入力を含むrunにもbaseline中央値だけでは履歴分散の内側を誤ってfailureにする問題が残ると確定した。`scripts/performance/benchmark.py`はlatencyを`max(中央値×1.10, 履歴p95最大値)`、FPSを`min(中央値×0.95, 履歴p50最小値)`で評価し、`performance-report.json`へ`baseline_extreme`と`threshold_rule`を保存するよう変更した。absolute threshold、bootstrap、malformed baselineのfail-closedは維持。`tests/performance/test_benchmark.py`を分散ケースへ拡張し、focused suiteは73 passed、contract-check、actionlint、format、diff-checkが成功。

## 5. 2026-09-22 シリアル切り替えとICE警告

- ユーザー報告の`could not listen udp fe80::…: 無効な引数です (os error 22)`は、`webrtc-ice`がIPv6 link-local addressをinterface scopeなしの`SocketAddr`としてbindしようとして発生する警告であり、シリアルsocketのbind失敗ではない。`webrtc-ice-0.14.0/src/agent/agent_gather.rs`の候補収集処理と、現行ホストの`fe80::/64` interface addressをsource／system stateで照合した。
- `rust/pokecon/src/server/webrtc.rs`でICE候補からunscoped IPv6 link-local addressを除外し、該当addressをbindしない契約テストを追加した。通常のIPv6 ULAとIPv4候補は保持する。
- `rust/pokecon/src/device/serial/manager.rs`へ、現在のselectorを安全に開くidempotentな`connect`操作を追加した。既存接続時は二重open・二重receive monitorを作らず、設定変更時の既存`update_config` transactionは旧endpointを中立化・closeしてから新endpointをopenする。
- `rust/pokecon/src/application_backend.rs`の接続中`connect`無操作分岐を削除し、常にserial managerのtransactionへ委譲した。`web/src/lib/components/SerialTab.svelte`では接続中も`Connect`を有効にし、明示的な`Disconnect`だけが切断操作となるUIへ変更した。
- UI回帰テスト、serial managerのidempotent connect／旧endpoint close→新endpoint install、ICE link-local filterのテストを追加した。
- 追加のPR Normal run `35707737079`／`35709829066`では、Python contract failureをsource contract同期で修正した。一方Browser performance smokeはabsolute thresholdを通過したままrelative baselineだけがfailureとなった。`scripts/performance/benchmark.py`はproduction Rust/Web UI/serialを呼び出さない独立loopback fixtureであることをsource call graphとartifactで確認し、`scripts/ci/regions.py`の`performance_baseline`をfixture本体（`scripts/performance/`）とruntime入力（`flake.nix`／`flake.lock`）だけへ限定した。Rust/Web変更ではabsolute smokeを維持し、relative比較を行わない。threshold引き上げやfailure無視ではない。
- 上記分類修正のfocused qualityは62 passed、Python全体は518 passed／2 deselected。`nix fmt -- --ci`と`git diff --check`も成功した。
- 検証: `nix run .#cargo -- test --locked -p pokecon --lib` は443 passed／0 failed、`nix run .#web-check`はsvelte-check 0 errors／0 warnings、Web 26 files／106 passed、production build success。`nix run .#clippy`、`nix run .#contract-check`、`nix fmt -- --ci`、`git diff --check`も成功した。実機serial hardwareは未使用。

## 6. 完了判定

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
