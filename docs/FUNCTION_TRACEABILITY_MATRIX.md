# 機能トレーサビリティmatrix

## 1. 目的と完了境界

この文書は、製品定義書の機能を、役割、owner、実装、受入test、現在のgapへ対応付ける監査artifactです。

`AR-10.4-FUNCTIONS`と`AR-11-01`の予定証跡を一つのmatrixへ統合します。

このmatrixは機能を実装済みへ昇格させません。
判定は既存の`TRACEABILITY_BACKEND.md`、`TRACEABILITY_FRONTEND.md`、`TRACEABILITY_INTEGRATION.md`の直接証拠を正とし、本書はそのownerとtestの対応を追加します。

`実装済み`は直接証拠と受入testが存在する機能、`部分`は実装またはtestが不足する機能、`未実装`は実装がない機能、`将来`は仕様上の将来機能、`外部受入待ち`はlocal／CIだけでは完了しない機能を表します。

## 2. 定義書sectionのcoverage

| 定義書 | 対応section | 詳細判定の正本 |
|---|---|---|
| backend | §1.2、§4.4、§4.6、§6.1.6、§7.8、§7.9、§10、付録B、§11.1-§11.5、§12、§14 | [`TRACEABILITY_BACKEND.md`](TRACEABILITY_BACKEND.md) §1.2-§14。実装済み151行、部分／未実装／仕様のみを含む |
| frontend | §0、§0.1-§0.2、§1.1-§1.4、§3、§4、§5、§6、§7、§8、§9、§10、§11.5.7、§13 | [`TRACEABILITY_FRONTEND.md`](TRACEABILITY_FRONTEND.md) §0-§13。Web/Tauriを実装済み、GPUI／PWA／browser gateの未受入を区別 |
| integration | §0、§1.3、§3.4、§6.1.5、§6.2.2、§7.1-§7.7、§8、§15 | [`TRACEABILITY_INTEGRATION.md`](TRACEABILITY_INTEGRATION.md) §0-§15。production call graph、worker、IPC、camera、shutdown、外部受入を区別 |

このcoverage表は、詳細表の行を機能matrixへ再掲する代わりに、sectionの取りこぼしを監査するために使用します。

## 3. 機能、owner、実装、testの対応

| 機能 | 定義書section | 役割／owner | 実装の直接証拠 | 受入test／artifact | 判定と残り |
|---|---|---|---|---|---|
| 起動、readiness、shutdown、resource ownership | backend §1.2、integration §7.1-§7.7、§15 | Rust composition／`ProductionRuntime` owner | `rust/pokecon/src/production.rs:176-445`、`src/entrypoint.rs`、`src/runtime/` | `rust/pokecon/tests/lifecycle.rs`、`nix run .#cargo-test`、Normal CI Rust job | 実装済み。process-level camera timeoutと外部browserは別gap |
| 設定pipeline、四つのroot、`app_name`、path／secret policy | backend §4.4、§11.1-§11.4、§12、§14 | Settings pipeline／registry owner | `rust/pokecon/src/settings/pipeline.rs`、`src/settings/roots.rs`、`registry/settings.json` | `rust/pokecon/tests/contract_sync.rs`、settings unit tests、`nix run .#contract-check` | 実装済み。仕様と異なるsecret modeは詳細表でowner decision待ち |
| 動的設定のPython／Lua runtimeとtransaction | backend §11.5、integration §7.5 | Dynamic worker／dynamic host／watcher owner | `rust/pokecon/src/dynamic_runtime.rs`、`src/dynamic/`、`src/worker_binary/dynamic/`、`src/dynamic_watcher.rs`、`src/application_backend.rs` | `rust/pokecon/tests/lifecycle.rs`、`dynamic_watcher::tests`（17 passed）、dynamic unit tests、contract registry | 部分。startup／manual／OS-native auto-reload、debounce、single-flight、停止／reapは実装・focused test済み。current remote required CIとfull product acceptanceは別gap |
| Profile switchとscript worker lifecycle | backend §1.2、§10、integration §7.2 | `ProfileService`／script worker owner | `rust/pokecon/src/profile_service.rs`、`src/script_runtime.rs`、`src/worker/supervisor.rs` | `rust/pokecon/tests/lifecycle.rs`、`tests/script_runtime.rs` | 実装済み。live browser profile discoveryは外部受入待ち |
| 固定compatibility corpusとpromotion | backend §4.6 | Compatibility registry／CI owner | `registry/compatibility.json`、`compatibility/fixed-manifest.json`、`scripts/compatibility/` | `tests/compatibility/`、`nix run .#compatibility`（baseline 3、script 103、discovered 98） | 実装済み。rolling／same-SHA live fixtureは外部証跡待ち |
| Camera enumeration、selector、capture、reconfigure | backend §6.1.6、§7.9、integration §6.1.5 | Camera manager／shared ring owner | `rust/pokecon/src/camera/manager.rs`、`selector.rs`、`shared_ring.rs`、`native.rs` | camera manager／shared ring tests、`tests/concurrent_camera_serial_load.rs`、`nix run .#cargo-test` | 部分。production latency／throughput／real cameraは未受入 |
| Shared-memory frame publish、reader pin、timeout teardown | backend §7.9、integration §15.6 | Shared ring／camera shutdown owner | `rust/pokecon/src/camera/shared_ring.rs:151-671`、`manager.rs:120-453`、`production.rs` | focused camera tests、`production::tests::production_runtime_shutdown_faults_retain_mapping_and_fail_closed`、`ar1129-production-fault/production-runtime-shutdown-faults.json`、`production::tests::production_shutdown_reaps_dynamic_fault_worker_before_serial_stop`、`production::tests::production_shutdown_with_clean_camera_reaps_dynamic_and_allows_release`、`production::tests::production_python_reader_on_runtime_mapping_leaves_no_pin`、`ar1129-production-fault/production-runtime-dynamic-shutdown.json`、`ar1129-production-fault/production-reader-lifecycle.json`（schema `production-reader-lifecycle/1`）、`production::tests::production_python_reader_crash_abandoned_pin_recovers_after_reap`、`ar1129-production-fault/production-reader-crash-recovery.json`（schema `production-reader-crash-recovery/1`）、`production::tests::production_reader_drop_retains_unconfirmed_mapping`、`ar1129-production-fault/production-reader-drop-retention.json`（schema `production-reader-drop-retention/1`）、integration-test-support付き`contract_sync` 27 passed、Rust CI、`TRACEABILITY_INTEGRATION.md` §15.6 | 部分。production composition rootでcamera writer timeout／serial fault-close、実dynamic workerのStopReport／OS reap、hung／clean writerのstep-5 gate分離、実Python workerのproduction mapping open／`RingReader::read`／clean process reap／reader pin recovery `0`を確認した。crash packetは実reader path証明後の診断seam植え付け1 pinをforced SIGKILL＋実OS reapでrecovery `1`、fallbackなし、step-5 gate=trueまで確認した。reader packetはthin `ScriptSessionStop` wrapper、virtual I/O、dynamic workerなしのnarrow evidenceであり、`shutdown_order`はawait済みcall-graph order＋terminal observationで独立event-traceではない。process-level flush／exit branch、process-exit reclaim、step-9 teardown、live hardwareは要検証として残る。Drop packetは実`Drop`のreader-fallback `mem::forget` retentionを同process内のdescriptor再open＋pre-drop frame一致で確認したが、process-exit reclaimは未証明のまま残る |
| Serial enumeration、codec、disconnect、neutral | backend §10.5、integration §6.2.2 | Serial manager／controller owner | `rust/pokecon/src/device/serial/`、`src/device/controller.rs` | serial manager tests、lifecycle tests、virtual I/O contract | 実装済み。real MCU／electrical behaviorは外部受入待ち |
| Controller input、priority、neutralization、input release | backend §10.4-§10.5、frontend §5.3、integration §7.3 | Controller safety／input boundary owner | `rust/pokecon/src/device/controller.rs`、`src/server/realtime/`、`web/src/lib/input.ts` | `rust/pokecon/tests/lifecycle.rs`、`web/src/lib/input.test.ts`、mutation／contract gates、production `production-priority-contention/1`、AR-11-05 `production-contention-adoption-record/1`（`decision=DEFER`） | 部分。実`ProductionRuntime`共有`InputArbiter`で6 source・5 roundのpriority winner 5／5、starvation 0、serial wire到達、before／after各30 timing samplesを記録した。before／after timingはlatency／jitter改善を示さず、jitter／実WebSocket queue停滞はunmeasuredのため採用判定はDEFER（公平性証拠と性能採用を分離）とした。cross-function contention order、実WebSocket逆圧、queue停滞閾値、clean source／remote CIは未受入 |
| Worker IPC、framing、queue、reader／writer termination | backend §7.8、integration §7.4 | Worker supervisor／IPC owner | `rust/pokecon/src/worker/supervisor.rs`、`src/worker/ipc/` | lifecycle、script runtime、fault worker tests、Rust contract CI | 実装済み。provider／OS負荷によるflaky testはrerun証跡で扱い、test契約を弱めていない |
| Python／Lua public script surface | backend §10、付録B、integration §8 | Script worker／public compatibility owner | `rust/pokecon/src/worker_binary/script/`、`generated/`、`registry/protocol.json` | `tests/script_runtime.rs`、`tests/contract_sync.rs`、compatibility corpus | 部分。CommandMetaの仕様差分とLuaJIT厳密版固定は詳細表のgap |
| Command discovery、tag、filter、shortcut | backend §10.4、frontend §6.4 | Dynamic command registry／Web Commands UI owner | `rust/pokecon/src/dynamic/command.rs`、`web/src/lib/components/CommandsTab.svelte` | `tests/script_runtime.rs`、`web/src/lib/components/CommandsTab.test.ts`、contract gate | 実装済み。tag／filterのbackend cacheはRust owner、UIはread-only projection |
| Settings／profile／camera／serial／notification UI | frontend §0.2、§4-§6 | Svelte UI owner、backend settings owner | `web/src/lib/components/`、`web/src/lib/runtime.ts`、generated OpenAPI types | 26 Web test files／112 tests、Svelte diagnostics 0、`nix run .#web-check` | 実装済み。仕様hex色、high contrast、browser version gateなどはfrontend詳細表のgap |
| Web／Tauri mode、REST、OpenAPI、WebSocket、WebRTC／MJPEG | frontend §6、integration §3.4、§7.6 | Axum／OpenAPI backend、Svelte/Tauri adapter owner | `rust/pokecon/src/server/`、`api/openapi.json`、`web/src/lib/media.ts`、`tauri.conf.json` | `tests/contract_sync.rs`、UI boundary tests、web tests、Package clean-install | 部分。browser live WebRTC／fallback／410 backend acceptanceは外部待ち |
| Screenshot、download、native save path | frontend §6.1.4、backend §10.3、integration §6.1.5 | Web camera UI／Tauri capability owner | `web/src/lib/components/CameraViewport.svelte`、`CameraTab.svelte`、`desktop.ts` | CameraViewport／CameraTab tests、package smoke | 実装済み。GPUI capabilityは将来matrixへ分離 |
| Notifications、secret projection、failure isolation | backend §10.3、frontend §4.3、integration §7.7 | Notification service／Web notification UI owner | `rust/pokecon/src/device/notification/`、`web/src/lib/components/NotificationsTab.svelte` | notification tests、`NotificationsTab.test.ts`、secret-safe contract | 実装済み。外部Discord／Windows notification deliveryは実環境受入待ち |
| Packaging、managed runtime、wheelhouse、NSIS／Debian | backend §14、frontend §0、integration §8 | release scripts／Nix／Package workflow owner | `scripts/release/`、`flake.nix`、`.github/workflows/package.yml` | Package CI `36150170942`全8 job、primary／repro artifact、release tests | 実装済み。Release tag／publicationはowner decision待ち |
| CI regions、aggregate、Nix evidence、timing | plan §3、integration §8 | `.github/workflows`／`scripts/ci` owner | `normal-ci.yml`、`package.yml`、`scripts/ci/aggregate.py`、`nix_evidence.py`、`ci_timing.py` | Normal CI `36150170777`、p95 timing artifact、`tests/quality/`、mutation 395/395 | 部分。fixture run matrix、cancel-in-progress、10-run history、non-draft mergeabilityは外部証跡待ち |
| GPUI native frontend | frontend §0、integration §0、GPUI plan | Future UI owner。現行Web/Tauriを維持 | `rust/pokecon/src/gpui/`、`src/entrypoint.rs`、`flake.nix`の`apps.gpui`、`Cargo.toml`／`Cargo.lock`のexact pin、`docs/GPUI_PHASE0_INVENTORY.md` | fake-view／headless tests、GPUI feature build／Clippy／all-features testは実装済み。Phase 1 Gate 1のnative window、IME／clipboard／AccessKit、license／clean-source／remote証跡は未実施 | PoC実装済み・Gate 1未受入。採否、依存closure、native／WASM scopeはowner decision待ち |

## 4. 役割とownerの境界

- Rust composition、settings、camera、serial、worker、IPC、OpenAPIの正準stateとresource lifetimeはRust側がownerです。
- Web／Tauriはbackend stateを表示・操作するclientであり、hardware resource、canonical state、設定schemaを重複所有しません。
- script／dynamic workerは公開互換surfaceとworker内実行をownerしますが、camera、serial、notificationのresourceはRust hostへ委譲します。
- CI／release scriptsは検証と配布証跡をownerしますが、未受入の製品性能や実機結果を作りません。
- GPUIはPhase 0のinventoryだけが存在し、Phase 1以降のownerと採否は未決定です。

## 5. 未完了matrixと次の判定

このmatrixから、実装の不足と証拠の不足を次のように分けます。

### 5.1 PMが追加できる証拠

- CI fixtureのsingle-area、mixed-area、cancel-in-progress、same-SHAのrun matrix。
- production function別のvirtual I/O latency／throughput／stability report。ただし閾値とjitter／soak定義が決まった後に実行します。
- clean worktree、non-draft PRのpending／failure mergeability、10-run p95の外部記録。

### 5.2 owner decisionが先の項目

- production latency／throughput／jitterの閾値とsoak duration。
- resource contentionの優先順位、starvation、backpressureの規範。
- `auto_reload_config`の対応OS／notify backend差分と、remote／full product acceptanceの証跡。
- GPUI native／WASMのscope、依存release、license、Gate 1採否。
- Release tag作成とtag-triggered publication。

### 5.3 外部環境が必要な項目

- 実camera、実serial、実MCU、browser WebRTC／MJPEG、keyboard／accessibility。
- clean detached worktreeが必要なlive browser受入。
- 実署名を含むuser-owned Release publication。

この分類は、matrixの`実装済み`判定を未受入へ広げず、次のownerまたは外部証跡を具体化するために使用します。

## 6. 更新規則

新しい機能を追加した場合は、定義書section、実装owner、直接test、受入artifact、status、gapを同じrowへ反映します。

直接証拠が変わった場合は、本書だけでなく対応する詳細traceability文書を先に更新します。

未実装数が0でない限り、`AR-10.4-FUNCTIONS`の「未実装数0」を完了扱いにしません。

owner decisionや外部環境待ちのrowは、判断または実証の後にのみstatusを更新します。
