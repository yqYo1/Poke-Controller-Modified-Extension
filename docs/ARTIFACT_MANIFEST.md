# 成果物 manifest と OS 別 clean-install 記録

本書は `AR-11-38` の予定証跡「artifact manifest と OS 別 clean-install report」に該当する。

正は `.github/workflows/package.yml`、`.github/workflows/release.yml` と `scripts/release/` の各 script であり、
本書は現行成果物、provenance、OS 別受入の読み戻し記録だけを担う。workflow 自体の変更は行わない。

## 配布成果物の一覧

| OS | 成果物 | 生成 command | provenance | 配布先 |
| --- | --- | --- | --- | --- |
| Linux | `dist/tauri/*.deb`（artifact `package-linux-x86_64`） | `nix run .#tauri-build -- --bundles deb` | `signing-inputs.json`（Linux signing inputs）、clean-install smoke log | Package CI artifact、Release workflow（tag 起点、利用者 deferred） |
| Windows | NSIS `*setup.exe`（artifact `package-windows-x86_64`） | `nix run .#tauri-build -- --bundles nsis`（offline、PE metadata 正規化） | `signing-inputs.json`（policy `schema_version=1`、`canonical_sha256`、file `sha256`／size）、`windows-payload-manifest.json`、`windows-install-tree-manifest.json` | 同上 |
| 共通 stage | web 配布物、managed runtime、offline wheelhouse、`pokecon-worker` | `package-smoke`／`package-install-smoke` で監査 | `resource-manifest.json`（`content_sha256`）、SPA 検証、wheelhouse 検証 | 各 installer に同梱 |
| Nix | `.#pokecon`、`.#web` ほか flake apps | `nix build` | store path（build log） | 開発・CI 内 |

現行dirty worktreeでの追加実体化:

- `nix build .#pokecon --no-link --print-out-paths` は `/nix/store/ph05fczcpc976ksxg0wcd2mjnizzpgvk-pokecon-0.1.0` を生成し、`bin/pokecon --help`（exit 0）と`--version`（`pokecon 0.1.0`）を確認した。NAR hashは `sha256-br1bIhWCFb4KzPJQsBfrGmTfWxnuEjiggkuhi2Abc2E=`。
- `nix build .#checks.x86_64-linux.rust-core-artifacts --no-link --print-out-paths` は `/nix/store/3s5rxg90k77a2jn0vczav2z6l3bgjb4l-pokecon-rust-core-check-0.1.0` を生成し、`passed` markerと4 output filesを確認した。NAR hashは `sha256-k8tTc4xnuWY0BC+xeDm7va+2dJRvzFVhFCZzbUzzS84=`。
- 後続の同じdirty source checkpointの成功realizationも独立検証した。package `/nix/store/4bj3spq0v05la85ppgpmv9p3g34svb3w-pokecon-0.1.0` は `bin/pokecon --help`／`--version`（`pokecon 0.1.0`）がexit 0、16 files、NAR size `166395888`、NAR hash `sha256-8dosM4jOW3QHLzdnWj9vl6eV0JjTw9MOgS5ZqIiXRJk=`。rust-core `/nix/store/nyjjqxvr0csvqk3qnyca120d91s64n7k-pokecon-rust-core-check-0.1.0` は `passed` marker、4 output files、NAR size `87395344`、NAR hash `sha256-oO8WVKvum2kjg8rl/i9t8+L5DTB48wNpKRmqu4hM4TA=`。これらはdirty local artifactであり、clean source／remote required CIの代替ではない。
- `nix build .#checks.x86_64-linux.rust-core-artifacts --no-link --print-out-paths` の最新realizationは `/nix/store/03sxhk2if9my3j5p5vzpk6kcz5ipb7nx-pokecon-rust-core-check-0.1.0`。`passed` marker、4 files、87,403,200 bytes、NAR hash `sha256-93dG94bChWUCpWbHiteFm7Ju3FTZQIW0pdZG2H8kzCs=`を読み戻した。これはproduction contention Clippy fix後の現行dirty sourceを対象にしたlocal artifactであり、clean tracked-source／remote required CIの代替ではない。
- `nix build .#checks.x86_64-linux.rust-core-artifacts --no-link --print-out-paths` のbefore／after contention baseline追加後realizationは `/nix/store/2f9bmzzxi4a7a0yc7yxsgw5ddzd3x5yw-pokecon-rust-core-check-0.1.0`。`passed` marker、4 files、87,403,200 bytes、NAR hash `sha256-93dG94bChWUCpWbHiteFm7Ju3FTZQIW0pdZG2H8kzCs=`を読み戻した。現行dirty sourceの再検証であり、clean tracked-source／remote required CIの代替ではない。
- `nix build .#checks.x86_64-linux.rust-core-artifacts --no-link --print-out-paths` の6 source contention fixture拡張後realizationは `/nix/store/n20mwzdqwlqn2k5l80s7gd0xlzvwgkp7-pokecon-rust-core-check-0.1.0`。`passed` marker、4 files、87,403,200 bytes、NAR hash `sha256-93dG94bChWUCpWbHiteFm7Ju3FTZQIW0pdZG2H8kzCs=`を読み戻した。現行dirty sourceの再検証であり、clean tracked-source／remote required CIの代替ではない。
- `nix build .#checks.x86_64-linux.rust-core-artifacts --out-link /home/yayoi/.hermes/cache/scratch/rust-core-artifacts-current` は現行dirty sourceでexit 0となり、再読出し可能な `/nix/store/ac55i3r548yw1ikshycgm4lizg2gsm53-pokecon-rust-core-check-0.1.0` を得た。`passed` marker、4 files、NAR size `87,404,328` bytes、NAR hash `sha256-93dG94bChWUCpWbHiteFm7Ju3FTZQIW0pdZG2H8kzCs=`を読み戻した。これはAR-11-29 fault fixtureおよび現行dirty sourceのlocal artifactであり、clean tracked-source／remote required CIの代替ではない。
- 最初のpackage build session `proc_9ad3f045e8c1`はRust release metadata書き込み中に`No space left on device`で失敗したため、heartbeatだけで成功扱いしなかった。既に許可済みの`nh clean all --keep 1 --keep-since 0h --optimise`を実行して8.9 GiBを解放し、serialized retry `proc_757b63b983fc`をexit 0まで完了した。生成物`/nix/store/qzn7i9ip3a9g121s74vvi59za0qrfyiw-pokecon-0.1.0`を読み戻し、launcher／worker `pokecon 0.1.0`、16 files、166,327,655 bytes、NAR hash `sha256-YXqlHbI17rRmMVWBjc92I9cEFW5/k7KO1gfDPUqlVB8=`を確認した。これはdirty-local evidenceであり、clean-source／remote Release evidenceではない。
- production-routingのapp境界も現行dirty treeで検証した。`nix run .#test-production-routing`は既存check derivationをrealizeし、single output path、非symlinkの`passed` marker、baseline pytest 1 passedでexit 0。`nix run .#test-production-routing-mutations -- --workers 1`は395 mutationを1 shardで実行しexit 0。wrapper／direct derivationの名前不一致を解消したが、clean source／remote required CIの代替ではない。
- 現行dirty treeのLinux配布物を再生成し、`nix run .#tauri-build -- --bundles deb`は authoritative worktreeでexit 0、`dist/tauri/PokeCon Controller_0.1.0_amd64.deb`（198,018,076 bytes、SHA-256 `2d31670ddd57ceccd72a2eed2bf9eb04bef29c7b22b708eaf4f550a1cff30647`）を得た。`dpkg-deb --info`で`Package: poke-con-controller`／`Version: 0.1.0`／`Architecture: amd64`を読み戻し、`nix run .#package-smoke -- 'dist/tauri/PokeCon Controller_0.1.0_amd64.deb'`はexit 0（`python_version=3.14.3`、`uv_version=0.11.8`、`wheel_count=28`、`resource_files=3801`）。これはdirty-local Linux bundleであり、remote Package／Release CIやclean-source parityの代替ではない。
- 同じ現行dirty sourceで`nix build .#pokecon --no-link --print-out-paths`は既存の`/nix/store/h68b1f2z6an2xyrq8l85hd1zzsic2dl7-pokecon-0.1.0`を再現し、launcher／worker `--version`、16 files、NAR `sha256-OHEgRWx+hcG/wkaOq5md7mMl0Q5gVS6VJQeFGWoT1bU=`を再確認した。
- `nix build .#checks.x86_64-linux.rust-core-artifacts --out-link /home/yayoi/.hermes/cache/scratch/rust-core-artifacts-ar1129-drop` は、Drop／reader-fallback retention packet追加後の現行dirty sourceでexit 0となり、`/nix/store/6nckq9582wh6w17bshdlvbma17s63qd1-pokecon-rust-core-check-0.1.0`を得た。`passed` marker、4 files、NAR size `87,406,544` bytes、NAR hash `sha256-80BmlVaBPW5Kpy5h02+tIkpGMI9QKpVOsX5UtrvjHFU=`を読み戻した。Nix sandboxでは`POKECON_PRODUCTION_ARTIFACT_DIR`未設定時にtest artifactを一時directoryへ退避し、直接dirty-worktree testではtraceability記載のHermes scratchへ保存する。clean tracked-source／remote required CIの代替ではない。
## provenance manifest 群

- `scripts/release/signing_manifest.py` は signing-inputs の policy、canonical SHA-256、file 単位 digest／size を記録する。
- `scripts/release/gate.py` は tool versions と SHA-256 を記録する。
- `scripts/release/package_smoke.py` は resource-manifest、SPA、wheelhouse を検証する。
- `scripts/acceptance/ci_check_inventory.py` は workflow 内の flake check app 呼出 site を (app, workflow, context) で inventory 化し、重複 0 と例外理由を検査する。
- `windows-install-tree-manifest.json` は clean-install 後の展開 tree を記録し、NSIS 再現性の三層比較に使う。

## production主経路 virtual-I/O 性能成果物

`nix run .#production-perf-check`は、browser primitive smokeとは別に、次の2 fixtureをrelease profileで実行する。

| fixture | report | raw sample | 固定条件 | 判定 |
| --- | --- | --- | --- | --- |
| `production-virtual-v1` | `performance-report.json` | `performance-samples.json` | 60秒warm-up、300 sample／metric、停止／復旧300 cycle | `evaluation.mode=blocking`、全`threshold_evaluation[].blocking=true` |
| `main-path-trace-v1` | `main-path-trace-report.json` | `main-path-trace-samples.json` | 60秒warm-up、command→frame→recognition→serial→wire 300 cycle | 同上 |

threshold超過時もreportとraw sampleを先に保存し、testおよびrequired `normal-ci/production_perf` jobをfailにする。固定p95 budgetはcamera frame 10 ms、recognition 500 ms、serial send 50 ms、wire observe 50 ms、command dispatch 5000 ms、shutdown 5000 ms、recovery／total end-to-end 30000 msであり、baseline／bootstrapの比較結果でblocking判定を置換しない。

localでのrelease実行とthresholdのbreak-red（total end-to-end budgetを0 msへ変更した場合のfail）を確認済みである。現行treeでの`main-path-trace-check`はexit 0、300 cycle、total p95 13.730 ms、report／raw／logを`/home/yayoi/.hermes/cache/scratch/main-path-trace-current/`へ保存した。ただし、現行worktreeは未commitであり、clean source snapshotとremote required CIのartifactは別の未完了gateとして扱う。

## 契約／lifecycle acceptance report

`contract-check`は現行dirty worktreeでexit 0となり、`POKECON_ACCEPTANCE_REPORT_DIR`を明示した同一gateから`schema-report/1`、`boundary-report/1`、`abstraction-report/1`、`lifecycle-report/1`を同じdirectoryへ生成する。`compatibility`は同じdirectoryへ`compatibility-report/1`を生成し、固定baseline不変と候補評価結果を保存する。これらはlocal acceptance artifactであり、commit別remote CIの代替ではない。

`devshell-ci-parity/v1`のreportは、現行worktreeがdirtyであることをfail-closedに記録し、Nix taskのtracked-file object IDと、実在するstore outputだけをNAR hashとして記録する。`nix run` appはapp attribute自体ではなく、現在のrepository rootを`builtins.getFlake`で評価したrealized launcher programをNAR probeし、`nix fmt`のようなstore artifactを持たないtaskは`not-applicable`として欠測と混同しない。app/package outputが未実体化の場合は推測せず`unavailable`として記録する。現行73 taskはbounded slice（64＋9）で`/home/yayoi/.hermes/cache/scratch/phase1-current/devshell-ci-parity-slice-1.json`および`devshell-ci-parity-slice-2.json`へ保存し、両方`verdict=non-parity`を確認した。clean parityを主張するためのcommit／pushは行わない。

## lifecycle／fault成果物

| fixture | artifact | 実測内容 | 境界 |
| --- | --- | --- | --- |
| `production-startup-bind-conflict-v1` | `startup-fault/production-startup-bind-conflict.json` | `ProductionRuntime::build`後のAddrInUse、readiness未公開、bounded cleanup、同一隔離rootsでのhealthy restart | production constructor全state-transition表とremote CIは未完了 |
| `production-build-rollback-v1` | `/home/yayoi/.hermes/cache/scratch/ar1129-build-rollback/production-build-rollback.json` | 実`ProductionRuntime::build_with_backends`でcamera session開始後にcommand root初期化を強制失敗し、`BuildCleanup`のcamera close `1`、serial open attempt `0`、エラー返却を確認。`start_media`がspawnしたfallback media taskは後続のfallibleなsettings／state／command-root／router初期化より前に同じcleanup集合へ登録される | dynamic／script worker fault、reader pin／OS reap、live hardware、clean source、remote CIは未完了 |
| `production-runtime-shutdown-faults-v1` | `/home/yayoi/.hermes/cache/scratch/ar1129-production-fault/production-runtime-shutdown-faults.json` | 同じ`SettingsPipeline`＋`ProductionRuntime::build_with_backends` composition rootで`Solid→Hang` camera writerと`BrokenPipe` serial writeを実行。writer timeout後のmapping fallback保持、shared-memory release gate拒否、serial endpointのfail-closed close、post-shutdown send error、反復shutdown後のwire bytes不変を確認（focused 1 passed、production lib suite 13 passed） | private `shutdown_production`のdynamic workerなし経路は実行済みだが、dynamic workerのprepare／stop／reap branch、実Python reader pin／OS reap、native camera／MCU、clean source、remote CIは未完了 |
| `production-runtime-dynamic-shutdown-v2` | `/home/yayoi/.hermes/cache/scratch/ar1129-production-fault/production-runtime-dynamic-shutdown.json` | 実`ProductionRuntime::build_with_backends`＋`SettingsPipeline`から実`pokecon-worker-fault-fixture`の`ack-then-hang` childを`WorkerKind::Dynamic`としてspawnし、`DynamicRuntime::prepare_shutdown`、cooperative ack後のforced stop、実OS wait/reap、`GenerationPhase::Stopped`、実`StopReport`（cooperative acknowledged／forced／非success exit）、`dynamic_reaped=true`、serial fail-closed停止を確認。clean-camera variantではwriter停止、fallbackなし、step-5 release gate=true、hung-camera variantではwriter fallback保持とgate=falseを確認（focused dynamic tests 2 passed、production lib suite 15 passed、integration-test-support付き`contract_sync` 27 passed） | `shutdown_order`は`lib.rs::shutdown_production`のawait済みcall-graph orderとterminal observationsであり独立event-traceではない。実Python reader pin／OS-level shared-memory lifecycle、native camera／MCU、live hardware、clean source、remote CIは未完了 |
| `production-reader-lifecycle-v1` | `/home/yayoi/.hermes/cache/scratch/ar1129-production-fault/production-reader-lifecycle.json` | 同じ`ProductionRuntime::build_with_backends`＋`SettingsPipeline`で生成した実camera mappingをminimal `ScriptHost::camera_initialize`から実`pokecon-worker`へ渡し、`initialize_python`の`SharedFrameRing::open`、`readFrame`／`getCameraImage`の`RingReader::read`、script outcome `Completed`、cooperative stop、process reap、recovered pin count `0`、replacement reader不在、fallbackなし、step-5 gate=trueを確認（schema `production-reader-lifecycle/1`、1280x720、exit code `0`） | clean Python-reader reapのnarrow evidence。thin `ScriptSessionStop` wrapper、virtual camera／serial、dynamic workerなし、crash-abandoned-pin、process-exit reclaim、Drop／step-9 teardown、native hardware、clean source、remote CIは未完了 |
| `production-reader-crash-recovery-v1` | `/home/yayoi/.hermes/cache/scratch/ar1129-production-fault/production-reader-crash-recovery.json` | 同じcomposition root＋実`pokecon-worker`で`reader.py`／`Reader`を`Completed`まで実行して実Python reader pathを先に証明し、`hang.py`／`Hanger`の`time.sleep(600)`をin-flightに保ったままproduction mappingへ診断seamで1 pinをplantし、`ManagedWorker::stop(ApplicationShutdown, 3s)`のforced SIGKILL＋実OS reapでrecovered pin count `1`、replacement reader不在、fallbackなし、step-5 gate=true、writer停止、serial tail cleanを確認（schema `production-reader-crash-recovery/1`、1280x720、cooperative_acknowledged=false、forced=true、非success exitでexit codeなし、hang outcomeはforced-stop後のclient error） | crash-abandoned-pin recoveryのnarrow evidence。pinは診断seam植え付け（hung scriptのtransient pinは残留しないため実reader path証明と併用）、thin `ScriptSessionStop` wrapper、virtual camera／serial、dynamic workerなし、injected forced stop（自発OS crashではない）、process-exit reclaim／Drop／step-9 teardown、native hardware、clean source、remote CIは未完了 |
| `production-reader-drop-retention-v1` | `/home/yayoi/.hermes/cache/scratch/ar1129-production-fault/production-reader-drop-retention.json` | 同じcomposition root＋virtual camera／serialでshutdown tail後のclean gateを確認し、既知1 frame publish後に`retain_dynamic_mapping_after_worker_shutdown(false)`でrelease gate=false・fallback descriptor一致を固定し、実`drop(runtime)`後のOS mappingを既存`SharedFrameRing::open`／`read_published`で再観測（schema `production-reader-drop-retention/1`、1280x720、先頭byte `13`、mapping_observable_after_drop=true、process_exit_reclaim_proven=false）。focused 1 passed、期待byte改竄のbreak-redはFAILED後に復元・再pass | 実`Drop`の`mem::forget` retention pathのnarrow evidence。同process内再open観測のためprocess-exit reclaim、step-9 teardown、crash pin、live hardware、clean source、remote CIは未完了 |
| `production-shutdown-fault-v1` | `shutdown-fault/production-shutdown-fault-sequence.json`、`reader-lifecycle/reader-lifecycle-report.json` | production-owned camera／serial／worker、neutral、forced stop、camera timeout fallback、serial fault-close、repeat idempotence。別fixtureの`main_path_trace_virtual`は実`pokecon-worker` script process、Python camera surface、shared-ring open、cooperative stop／reap、recovered pin count `0`を`reader-lifecycle/1`へ保存 | native `ProductionRuntime::build` hardwareなし初期化／shutdown smokeとvirtual shared composition rootは確認済み。script shutdown失敗／reader pin recovery失敗時のmapping retention gateを`ProductionRuntime::Drop`へ接続し、reader mappingとunstopped writer guardをprocess exitまで保持するproduction-used path、およびfocused helper testを追加したが、full native fault matrix、`ProductionRuntime`全経路へのreader fault注入、live hardware、remote evidenceは未成立 |

`production-priority-contention/1`は、現行dirty worktreeの実`ProductionRuntime` composition rootから取得したproduction-owned contention artifactである。共有`InputArbiter`の実mutexを6 source・5 roundで同時taskから競合させ、before baseline（同一priorityでarrival orderを反転するとwinner `[192, 64]`へ反転）とafter（explicit priority winner 5／5、starvation 0、lock-wait／apply各30 samples、production-wired `SerialManager`へのwire bytes 30）を記録した。timingはbefore／after各30 samplesで取得し、beforeはlock-wait p95=121 ns／apply p95=17,624 ns、afterはlock-wait p95=37,681 ns／apply p95=17,323 nsだった。このnarrow fixtureはwinner公平性とstarvation 0を示すがlatency／jitter改善を証明しないため、test-only queue modelのbefore／after、実WebSocket逆圧、clean source／remote CIは未完了のまま保持する。artifactは`/home/yayoi/.hermes/cache/scratch/production-contention-current/production-priority-contention.json`。remote CIの代替ではない。

`production-contention-adoption-record/1`は、同fixtureのcomputed値から生成するAR-11-05採用判定recordである。before／afterのlock-wait／apply p50／p95／max、arrival-order反転、starvation、serial wire bytesを記録し、jitter比較と実WebSocket queue停滞比較は`unmeasured`、採用判定は`decision=DEFER`（latency改善なしのためlatency／jitter改善としては不採用、公平性証拠と性能採用を分離）と明記する。artifactは`/home/yayoi/.hermes/cache/scratch/production-contention-current/production-contention-adoption.json`。dirty-local virtual-backend証跡であり、clean source／remote CI／実WebSocket逆圧／jitter測定の代替ではない。

## GPUI native runtime 成果物

`gpui-native-wayland-headless-v1`は、Weston `headless-backend.so`のWayland socketとMesa llvmpipe GL software rendererを使い、`--ui gpui --exit-after-startup`を実行したlocal runtime reportです。`POKECON-RUNTIME-0001`（Gpui readiness）、`POKECON-RUNTIME-0003`（DesktopExit）、`POKECON-RUNTIME-0002`（clean stop）、exit 0、selected adapter、panic absenceを記録しました。Vulkan adapter初期化失敗はexpected GL fallbackとしてreportへ保存し、unexpected errorとは区別しています。XvfbのGLX／adapter不足は別の環境限界として記録し、IME／clipboard／AccessKit treeの実window操作report、clean source、remote CIは未完了です。

現行treeでworktreeを明示して再実行した結果もexit 0／`status: "pass"`で、reportは`/home/yayoi/.hermes/cache/scratch/gpui-runtime-current/gpui-native-wayland-runtime-report.json`、logは同ディレクトリの`gpui-native-wayland-runtime.log`に保存した。これはdirty worktree上のLinux／Wayland／llvmpipe runtime証跡であり、clean source／remote CI／GPUI Phase 2以降を完了扱いにしない。

`gpui-headless-ime-geometry-v1`（`/home/yayoi/.hermes/cache/scratch/gpui-headless-ime-geometry/gpui-headless-ime-geometry.json`）は、実`GpuiFakeView`＋GPUI test windowのheadless shaperでmarked UTF-16 state、`bounds_for_range`のcandidate/caret boundsとelement origin追従、`character_index_for_point`の左端／単調性／行末境界を検証したartifactである。platform IME composition、rendered pixels、live candidate window、AccessKit／AT-SPI enumerationはこのheadless artifactの範囲外であり、Gate 1完了やclean source／remote CIの代替ではない。

`gpui-headless-focus-a11y-v1`（`/home/yayoi/.hermes/cache/scratch/gpui-headless-focus-a11y/gpui-headless-focus-a11y.json`）は、実`GpuiFakeView`＋GPUI test windowのheadless観測でTabのinput→copy→quit遷移とquit→inputのwrap、Shift-Tabの逆遷移、root／input／copy／quitの`Group`／`TextInput`／`Button` roleとlabel、visibleかつnon-zero boundsを検証したartifactである。status行はproductionに`.test_support()`登録がないため`try_find`不在の確認に狭めている。`nix run .#cargo -- test --locked -p pokecon --features gpui-test-support --lib gpui::fake_view`は3 passed、`nix fmt -- --ci`と`git diff --check`はexit 0だった。platform IME、rendered pixels、live candidate window、OS clipboard、AccessKit／AT-SPI enumerationはこのheadless artifactの範囲外であり、Gate 1完了やclean source／remote CIの代替ではない。

`gpui-g1-ops-x11-v1`では、dirty-local package `/nix/store/5h3asil0vkqvq03zwkzyzz0lpb5p2xd8-pokecon-0.1.0`をXwayland on Sway headless＋Mesa llvmpipeで起動し、real window、X11 CJK paste、application-owned `text/plain` read-back、window close（exit 0）を確認した。reportは`/home/yayoi/.hermes/cache/scratch/gpui-g1-ops-x11/run-report.json`、AT-SPI診断は同directoryの`accessibility-bfs.json`である。ただしAT-SPIはrootを含む3 nodeでlabel付きinput／button treeを返さず、IME composition／caret・hit-test、clean source／remote／hardwareは未証明なので、Gate 1完了の代替にはしない。

## 外部 browser local acceptance 成果物

`browser-acceptance-local-v1`は、専用HOME／XDG rootsで起動した現行dirty worktreeのWeb runtimeへ、browser backendから実接続したlocal reportである。`/api/state`のHTTP 200 read-back、camera／serial unavailable時のfallback、WebRTC timeout後のfallback維持、tab／tabpanel／region／label付きcontrolのAccessibility tree、keyboard focus ring、manual-control key probe、browser console error 0、server停止後の残留pokecon process 0を記録した。reportは`/home/yayoi/.hermes/cache/scratch/pokecon-browser-e2e/browser-acceptance-report.json`、focus screenshotは`/home/yayoi/.hermes/browser_screenshots/browser_screenshot_69dbb86a.png`である。

物理camera／serialなしのためWebRTCの成功昇格（fallback→primary）および実device映像は観測できず、これは外部browser gateの全完了やremote／commit別証跡を意味しない。

## OS 別 clean-install／再現性 report（読み戻し）

参照 run: Package CI `36353061684`（`pull_request`、head `fec66d9`、8/8 jobs SUCCESS）。

| job | ID | 内容 |
| --- | --- | --- |
| Debian bundle and clean-install smoke | `108715822568` | `.deb` 生成、package smoke、clean install／offline startup／upgrade／uninstall 検証 |
| NSIS bundle and clean-install smoke | `108715822604` | NSIS installer 生成、Windows での同検証 |
| Verify Debian package reproducibility | `108718837112` | primary／reproduction の byte-for-byte 比較 |
| Verify Windows NSIS reproducibility | `108720611250` | 外側 `.exe`、payload manifest、install tree の三層比較 |
| Required | `108720714749` | 集約 gate |

再現性検査の意図的な second build は [`PACKAGE_REPRODUCIBILITY_EXCEPTIONS.md`](PACKAGE_REPRODUCIBILITY_EXCEPTIONS.md) を正とする。

### 既知の flake 注記

`d644eeb` の `pull_request` run `36359352644` では、Windows NSIS 再現性検証だけが失敗した。

失敗は `pokecon-worker` の稀な codegen 非決定性（154 bytes 差、正準値は他 run で再現）であり、
同 run の clean-install smoke は成功している。

製品成果物の内容差を示す証拠はなく、継続観測の対象とする。

その後、`2d4baa7` の full build（Package CI run `36364102715`）で Windows NSIS 再現性検証は success を再確認しており、一過性の flake として扱う。

## Release との分離

`release.yml` は tag push を起点に `release-check --tag`、package smoke、install smoke、signing-input manifest を実行する。

`package.yml`（branch push／pull request 起点の検証）とは役割を分離している。

Release の実行実績はまだない。tag 作成は利用者 deferred であり、未実施は正当である。

## AR-11-30 配布マトリクスと未確定証跡

- **Linux / Nix**: `package.yml` の `linux` job が Debian bundle、package smoke、clean-install／offline startup／upgrade／uninstall、signing-input manifest を実行する。`release.yml` の `linux` job は tag 起点で同じ検査に加えて再現性、checksum、release asset を検査する。
- **Windows / Nix 由来の固定入力**: `package.yml` の `windows` job が NSIS bundle、install smoke、PE metadata 正規化、三層再現性を実行する。`release.yml` の `windows` job は tag 起点の installer、clean-install／upgrade／uninstall、checksum を検査する。
- **macOS**: 現行の `package.yml` と `release.yml` に macOS job はない。したがって macOS の build／package／runtime 証跡は存在せず、対応済みとは扱わない。
- **non-Nix toolchain**: 必須 workflow に non-Nix toolchain で製品を生成する job はない。Nix 外の実行環境互換性は未証明であり、Linux／Windows の Package CI 成功から推論しない。
- **物理環境**: MCU、カメラ、コンソール、音声、資格情報付き通知の実機証跡はこの matrix に含まれない。外部 browser／hardware acceptance は別 gate として扱う。

### 証跡の境界

Package CI の既存成功 run は workflow の配布設計と過去 commit の artifact 証跡であるが、現在の未コミット worktree の変更を検証した証跡ではない。現在の変更を remote required Package／Release CI で再検証するには commit／push と CI 完了が必要であり、本作業の `NO git commits/pushes` 制約下では AR-11-30 の remote evidence gate は未完了である。tag 操作と GitHub Release 公開も利用者担当として未実施である。
