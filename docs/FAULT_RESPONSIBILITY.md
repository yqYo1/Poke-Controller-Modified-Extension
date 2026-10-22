# 障害・timeout・rollback・終了処理の責任分担

本書は `AR-11-29` の「責任module／最終状態表と強制停止／resource解放／shutdown fault test」を、現行sourceの所有者とfault testへ対応付ける。

## 原則

- 故障を最初に観測した層が、その故障をtyped errorまたは停止理由へ変換する。
- 最終状態を決めるのは資源所有者であり、上位の呼出し層は再試行するか停止するかを選ぶだけにする。
- 強制停止はworker supervisorだけが行い、production orchestrationは期限と最終状態を要求する。
- resource解放は所有者のshutdown経路に集約し、失敗時はfail-closedで保持側へ倒す。暗黙の再生成・暗黙の代替資源への切替は行わない。
- rollbackは設定transactionの適用前状態へ戻す。戻せない場合は旧値を維持し、機能だけを利用不能にする。

## 責任moduleと最終状態

| 領域 | 最終責任module | timeout／障害時の最終状態 | 強制停止・解放 | 主な証跡 |
| --- | --- | --- | --- | --- |
| script／dynamic workerの監督 | `worker/supervisor.rs` | 協調停止の期限超過はforced stopとし、実OSでreapする。workerは再生成しない | supervisorがSIGKILL相当の強制停止を行い、process waitで回収する | `rust/pokecon/tests/lifecycle.rs` のack-then-hang、`production_shutdown_reaps_dynamic_fault_worker_before_serial_stop` |
| production全体の終了順序 | `production.rs` の`ProductionRuntime` | 入力停止→camera→serial→notification→dynamic worker→mapping release gateの順で進め、途中失敗はfail-closedで保持する | `stop_inputs_camera_and_scripts`と`BuildCleanup`が所有resourceを停止する | `production_runtime_shutdown_faults_retain_mapping_and_fail_closed`、`production_runtime_build_failure_rolls_back_started_resources` |
| カメラwriter停止 | `camera/manager.rs`／`camera/shared_ring.rs` | writerの停止timeoutはfallback mappingを保持し、release gateをfalseにする。reader pinの回収後にだけ解放する | camera managerがwriter／readerを停止し、mappingの単一所有者として解放を判定する | `camera_shutdown_timeout_retains_fallback_at_manager_level`、`production_python_reader_crash_abandoned_pin_recovers_after_reap` |
| シリアル送信・切断 | `device/serial/manager.rs` | 連続送信失敗は上限到達でコマンド停止を要求する。write失敗はendpointをfail-closeし、切断は冪等にする | serial managerがportをcloseし、neutral送信を所有する | `send_failure_streak`関連テスト、`production_runtime_shutdown_faults_retain_mapping_and_fail_closed` |
| 通知配送 | `device/notification.rs` | retry deadline超過・stop要求は配送を打ち切り、queueをdrainする。URL・本文はログへ出さない | notification serviceがqueueとretry sleepをcancelする | notification retry／deadline／stopテスト |
| 設定・profile切替 | `settings/service.rs`／`application_backend.rs` | 事前検証失敗は非適用。適用失敗は旧値へrollbackし、profile切替失敗は旧profileを維持する | mutation gateとrevisionで直列化し、失敗時に部分適用を残さない | settings rollback／profile switch tests、`patch_settings` |
| HTTP listener／signal task | `server.rs`／`entrypoint.rs` | listenerのI/O失敗・panicはfatal shutdownを要求する。readiness前の完了はtyped errorとして扱う | server taskを停止し、shutdown coordinatorへ通知する | `listener_io_failure_requests_fatal_shutdown`、`panicked_listener_task_requests_fatal_shutdown` |
| 入力合成・neutral | `production.rs`／input arbiter | stop、profile切替、transport切断、application終了でneutralを送る。manual入力の所有はarbiterが決める | arbiterが全入力源の最終状態を合成する | neutral回数のproduction shutdown tests |

## 終了処理の固定順序

1. 実行中commandへ停止を要求し、script入力とmanual入力をneutralへ戻す。
2. script workerを協調停止し、期限超過時だけforced stop／reapする。
3. camera writer／readerを停止し、timeout時はfallback mappingを保持する。
4. serialの送信を停止し、portをfail-closeする。
5. notificationのqueueをdrainし、retry sleepをcancelする。
6. dynamic workerを停止・reapする。
7. release gateを確認してcamera mappingを解放する。gateがfalseなら保持する。
8. shutdownを再実行しても、既にStoppedの資源は再timeoutせず最終状態を維持する。

## 非目標

実機カメラ／シリアル、外部browser、OS crashによるprocess消滅は、それぞれの外部受入gateで扱う。本書はproduction composition rootとfault fixtureで観測できる範囲の責任分担を確定するものであり、実機受入の代替ではない。
