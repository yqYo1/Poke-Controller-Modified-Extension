# 総合トレーサビリティ表

この文書は、製品仕様の入口、三つの規範定義書、現在のアーキテクチャ説明、進行計画を一つの入口から対応付けるためのartifactです。

この文書自体は製品要件を追加しません。
要件の意味と優先順位は`SPECIFICATION.md`と三つの規範定義書が正本です。
個別要件の直接証拠、部分判定、未実装判定は、各traceability文書の行を参照してください。

## 1. 検証基準と判定規則

- 検証基準は、対象worktreeの現行`HEAD`と同時点の未コミット差分です。
- パスはリポジトリルート相対です。
- `src/`は`rust/pokecon/src/`を指します。
- `registry/`は`rust/pokecon/registry/`を指します。
- `実装済み`は、対応するsource symbol、生成物、または実行済みtestが直接確認できる状態です。
- `部分的`は、要件の一部だけが確認できる状態です。
- `未実装`は、実装または必要なgateが存在しない状態です。
- `将来`は、仕様が未実装の目標として定めている状態です。
- `外部証跡待ち`は、実機、実browser、clean machine、Release tag、またはGitHub上の追加操作が必要な状態です。
- local test、CI success、source-only auditは、別の種類の未確認項目を暗黙に完了扱いしません。

## 2. 文書とartifactの責務

| 文書 | 種別 | この表での役割 | 正本または証拠 |
|---|---|---|---|
| [`SPECIFICATION.md`](../SPECIFICATION.md) | 製品仕様の入口 | 必須／機能要件、担当領域、旧section番号から正本への対応を定める | §要件分類、規範定義書表、旧番号対応表 |
| [`docs/SPECIFICATION_BACKEND.md`](SPECIFICATION_BACKEND.md) | 規範定義書 | 設定、runtime、script API、resource、IPC、camera、環境のbackend契約を定める | [`TRACEABILITY_BACKEND.md`](TRACEABILITY_BACKEND.md) |
| [`docs/SPECIFICATION_FRONTEND.md`](SPECIFICATION_FRONTEND.md) | 規範定義書 | Web／Tauri／GPUIの操作、表示、accessibility、browser品質を定める | [`TRACEABILITY_FRONTEND.md`](TRACEABILITY_FRONTEND.md) |
| [`docs/SPECIFICATION_INTEGRATION.md`](SPECIFICATION_INTEGRATION.md) | 規範定義書 | HTTP、WebSocket、WebRTC、保存、入力、lifecycle、frontend境界を定める | [`TRACEABILITY_INTEGRATION.md`](TRACEABILITY_INTEGRATION.md) |
| [`docs/ARCHITECTURE.md`](ARCHITECTURE.md) | 実装説明 | 現在のprocess、ownership、state、停止順を説明する。要件の正本ではない | source call graph、module／queue／lockの説明 |
| [`docs/ARCHITECTURE_HANDOFF.md`](ARCHITECTURE_HANDOFF.md) | 受入準備artifact | architecture handoffのownership、lifecycle、公開境界、依存方向、配布matrixと未成立証拠を集約する。要件の正本ではない | source参照、PLAN対応表、次のtest packet |
| [`PLAN.md`](../PLAN.md) | 進行計画 | 受入条件、実行証跡、未完了項目、owner／外部作業を記録する。要件の正本ではない | checkbox、CI run、artifact、残タスク |
| [`FUNCTION_TRACEABILITY_MATRIX.md`](FUNCTION_TRACEABILITY_MATRIX.md) | 機能監査artifact | 定義書の機能sectionを役割、owner、実装、test、status、gapへ対応付ける。要件の正本ではない | 機能matrix、直接source、受入test、未完了分類 |

利用者と開発者が入口を見つけるための[`docs/README.md`](README.md)は補助ナビゲーションです。
同じAPI一覧や設定一覧をこの表へ複製せず、正準文書へリンクします。

## 3. 規範sectionから直接証拠表への対応

### 3.1 Backend

| 正本section | 詳細な直接証拠表 | 判定の読み方 |
|---|---|---|
| §0 要件分類、§1.2 設計方針、§4.4 設定file、§4.6 互換性、§6.1.6 camera | [`TRACEABILITY_BACKEND.md:10-34`](TRACEABILITY_BACKEND.md#12--44--46--616) | settings、compatibility、cameraのsource／registry／testを個別行で判定 |
| §7.8 worker IPC、§7.9 shared memory camera | [`TRACEABILITY_BACKEND.md:36-62`](TRACEABILITY_BACKEND.md#78--79) | IPC、reader／writer、mapping、pin、timeout fallback、benchmark gapを個別行で判定 |
| §10 command class、付録B CommandMeta | [`TRACEABILITY_BACKEND.md:64-95`](TRACEABILITY_BACKEND.md#10--付録b) | public script surface、typing、protocol、部分的なCommandMetaを個別行で判定 |
| §11 settings filesystem | [`TRACEABILITY_BACKEND.md:97-127`](TRACEABILITY_BACKEND.md#111-114) | registry、scope、pipeline、dynamic config、apply pathを個別行で判定 |
| §12 environment、§14 development environment | [`TRACEABILITY_BACKEND.md:128-154`](TRACEABILITY_BACKEND.md#12--14) | env surface、XDG、Nix／devShellを個別行で判定 |
| §11.5 runtime dynamic configuration | [`TRACEABILITY_BACKEND.md:155-170`](TRACEABILITY_BACKEND.md#115) | manual reload、dynamic worker、`auto_reload_config`の未実装部分を分離して判定 |
| 集計、action、将来候補 | [`TRACEABILITY_BACKEND.md:171-197`](TRACEABILITY_BACKEND.md#集計) | 集計は個別行の要約であり、未実装を成功へ変換しない |

### 3.2 Frontend

| 正本section | 詳細な直接証拠表 | 判定の読み方 |
|---|---|---|
| §0 frontend選択、必須要件、機能の所有分担 | [`TRACEABILITY_FRONTEND.md:9-37`](TRACEABILITY_FRONTEND.md#0-分類と-frontend-選択) | Web／Tauriは実装、GPUIは将来または対象外として分離 |
| §1.1目的、§1.4対象外、§3非機能 | [`TRACEABILITY_FRONTEND.md:39-69`](TRACEABILITY_FRONTEND.md#11-目的--14-対象外機能) | browser、accessibility、応答性の実装機構と実測gapを分離 |
| §4重点機能、§5 UI layout | [`TRACEABILITY_FRONTEND.md:71-171`](TRACEABILITY_FRONTEND.md#4-重点機能要件) | UI操作とbackend-owned契約を混同せず、未実装色／deadzone等を残す |
| §6 tab仕様 | [`TRACEABILITY_FRONTEND.md:172-334`](TRACEABILITY_FRONTEND.md#6-タブ仕様) | tabごとの直接source、test、未達を列挙 |
| §9 theme、§11.5.7 dynamic-config UI、§13 storage | [`TRACEABILITY_FRONTEND.md:335-356`](TRACEABILITY_FRONTEND.md#9-テーマサポート将来) | theme、manual reload UI、storageのfrontend面を判定 |
| 付録A Tkinter reference、gap一覧 | [`TRACEABILITY_FRONTEND.md:357-379`](TRACEABILITY_FRONTEND.md#a-tkinter-ui-リファレンス非規範) | 非規範referenceを製品受入証拠に昇格させない |

### 3.3 Integration

| 正本section | 詳細な直接証拠表 | 判定の読み方 |
|---|---|---|
| §0 scope、§1.3 process model、§3.4 reconnect | [`TRACEABILITY_INTEGRATION.md:9-50`](TRACEABILITY_INTEGRATION.md#0-担当範囲と要件分類) | mode、single owner、heartbeat、reconnectを個別判定 |
| §6.1.5 screenshot、§6.2.2 serial settings | [`TRACEABILITY_INTEGRATION.md:51-80`](TRACEABILITY_INTEGRATION.md#615-スクリーンショットキャプチャe2e) | format、保存、serial transaction、rollbackを個別判定 |
| §7.1 stack、§7.2 WebRTC、§7.3 fallback | [`TRACEABILITY_INTEGRATION.md:81-137`](TRACEABILITY_INTEGRATION.md#71-スタック概要) | primary、fallback、promotion、input／log queueを個別判定 |
| §7.4 REST、§7.5 keyboard、§7.6 mouse、§7.7 gamepad | [`TRACEABILITY_INTEGRATION.md:138-204`](TRACEABILITY_INTEGRATION.md#74-http-rest-api) | API／入力wireを型、handler、testへ対応付け |
| §8 type system | [`TRACEABILITY_INTEGRATION.md:205-212`](TRACEABILITY_INTEGRATION.md#8-型システム) | OpenAPI、protocol、closed union、生成物の正本を判定 |
| §15 desktop lifecycle | [`TRACEABILITY_INTEGRATION.md:213-239`](TRACEABILITY_INTEGRATION.md#15-デスクトップライフサイクル閉じる動作) | shutdown、camera fallback、入力解放、未確認process teardownを分離 |
| 残課題 | [`TRACEABILITY_INTEGRATION.md:240-252`](TRACEABILITY_INTEGRATION.md#残課題一覧実装修正は本artifactの範囲外) | 残課題は受入未成立の理由として保持 |

各詳細表の行が要件単位の直接対応表です。
本indexはsection単位の漏れを防ぎ、詳細表は行単位の判定を保持します。

## 4. Architecture handoffと共通gateの接続

| 対象 | 正本／artifact | 確認内容 | 完了扱いにしない条件 |
|---|---|---|---|
| ownership、process、queue、lock、shutdown | [`docs/ARCHITECTURE.md`](ARCHITECTURE.md)と[`docs/ARCHITECTURE_HANDOFF.md`](ARCHITECTURE_HANDOFF.md)、三つのtraceability表 | architectureの主張を実在source symbolとproduction call graphへ接続し、handoffの未成立testを分離 | architecture説明だけでruntime受入、実機、browser受入を主張しない |
| 開発入口、生成物、正準registry | [`docs/README.md`](README.md)、[`docs/DEVELOPMENT.md`](DEVELOPMENT.md)、`registry/` | Nix task、source boundary、generated artifactの責務を接続 | clean detached worktreeが必要なgateを既存worktreeの成功で代用しない |
| checkpoint共通gate | [`PLAN.md`](../PLAN.md)の各checkpointと`flake.nix` task | format、contract、source filter、test、build、文書lint、CI runを実行結果へ接続 | 以前のSHA、単一unit test、delegated reviewだけではcurrent SHAの受入にしない |
| package／release | [`docs/INSTALL.md`](INSTALL.md)、[`docs/PACKAGE_REPRODUCIBILITY_EXCEPTIONS.md`](PACKAGE_REPRODUCIBILITY_EXCEPTIONS.md)、Package CI artifact | bundle、clean-install、upgrade、uninstall、二重build、manifestを接続 | `v*` tag、署名runner、実機／clean machineの外部証跡をCI successだけで代用しない |
| 外部受入 | [`docs/ACCEPTANCE.md`](ACCEPTANCE.md) | browser、hardware、security、load、release-candidate recordのschemaを接続 | virtual I/O、REST read-back、source auditを実browser／実機の代用にしない |

## 5. Checkpoint共通gateの対応

`PLAN.md:93-115`は、フェーズ1、構造移行、フェーズ3、フェーズ4.1-4.5へ同じ完了gateを適用する正本です。
各checkpointの実行結果は`PLAN.md`の該当commit／run記録に残し、未実行または対象外の場合は理由と代替証跡を記録します。

| Gate群 | PLANの直接対応 | このindexでの扱い |
|---|---|---|
| VCS checkpoint境界 | `PLAN.md:97-98`（`AR-13.1-01`、`AR-11-41`） | commit SHA、順序、Nix log、CI URLをcheckpoint単位で照合する。現在の過去checkpoint全件の再実行を暗黙に主張しない |
| contract／cargo／clippy／Rust build | `PLAN.md:99-102`（`AR-13.1-03`〜`AR-13.1-06`） | `contract-check`、`cargo-test`、`clippy`、`build-rust`の終了コードと成果物を対応付ける。単一current runは過去checkpointの証拠ではない |
| compatibility corpus | `PLAN.md:103`（`AR-13.1-10`） | fixed baseline、script、discovered command、report SHAを`compatibility` artifactへ対応付ける |
| Web／format／flake／aggregate | `PLAN.md:104-110` | Web check、`nix fmt -- --ci`、flake evaluation、aggregate `#check`の結果を分けて記録する。dirty worktreeのlocal gateとclean／CI gateを混同しない |
| CLI／mode／worker integration | `PLAN.md:111-113`（`AR-13.1-07`〜`AR-13.1-09`） | CLI help、Web／Tauri health、script／dynamic worker IPC・協調停止・fault reportを別々に受入する |
| CI completion and required contexts | `PLAN.md:114` | `ci-watch`の終了code、Normal／Package required aggregate、同一SHAを読み戻す。PRがDRAFTである場合はmergeability試験と扱わない |

現HEADで取得済みの単発gate（`contract-check`、`cargo-test`、`clippy`、`build-rust`、`compatibility`、`release-check`、`acceptance-record-check`、`#check`）は、現在のsourceとdocument consistencyの証拠です。
これらは、未取得のclean worktree、10-run p95、production性能、実機、実browser、Release tag証拠を置き換えません。

## 6. 現時点で暗黙に成功扱いしない項目

| 項目 | 現在の判定 | 直接の残課題／次の証拠 |
|---|---|---|
| `auto_reload_config` OS-native watcher | 未実装 | `TRACEABILITY_BACKEND.md` §11.5と`TRACEABILITY_FRONTEND.md` §11.5.7。watch対象、debounce、single-flight、停止順、worker不在／parse errorの製品判断後に実装する |
| GPUI native frontend | 将来／未実装 | `TRACEABILITY_FRONTEND.md` §0、`TRACEABILITY_INTEGRATION.md` §0、[`GPUI_PHASE0_INVENTORY.md`](GPUI_PHASE0_INVENTORY.md)。GPUI Phase 0 inventoryはbaseline／未決事項の記録であり、Gate 1-6や採否の証拠ではない |
| production主経路のlatency／throughput／jitter／soak | 未完了 | `TRACEABILITY_BACKEND.md` §7.9.6、`docs/ACCEPTANCE.md`性能schema。CI workflow timing p95は製品性能の代用ではない |
| 外部browser／tailnet WebRTC | 外部証跡待ち | `PLAN.md`外部browser受入、`docs/ACCEPTANCE.md` browser matrix。isolated serviceのstop／cleanupを含む |
| 実機／driver／firmware／console | 外部証跡待ち | `docs/ACCEPTANCE.md`のhardware record。virtual I/O successは代用しない |
| clean detached worktree再受入 | 制約により未成立 | `PLAN.md`現行残タスク。既存worktreeのみを使う制約の変更が必要 |
| Release tagとtag起点Release | 利用者担当 | `PLAN.md`担当外の外部操作。明示指示なしにtag／Releaseを作成しない |
| CI critical-path p95 | d800277のNormal CI run `36131178144`／artifact `ci-timing-36131178144-1`ではp95 715.0秒、threshold 720.0秒、violations 0 | これはCI gateの証拠。`PLAN.md:727`に記録した別SHA（b719ad5d）のartifact `ci-timing-36096392708-1`の674.0秒とは別証跡であり、10分（600秒）の製品目標、10-run same-kind履歴、製品latencyとは別に判定する |

## 7. 更新手順

1. 正本仕様またはsource／testを変更する。
2. 該当する詳細traceability表の直接証拠と判定を更新する。
3. このindexのsectionリンク、gap、文書責務だけを同期する。
4. `PLAN.md`は実行結果、artifact、CI run、GitHub設定readbackがある場合だけcheckboxを更新する。
5. `nix run .#markdownlint`、`nix run .#textlint`、`nix run .#typos`、必要なcontract／test gateを実行する。

この表の追加や更新は、未実装・外部証跡待ちを実装済みへ変換する操作ではありません。
