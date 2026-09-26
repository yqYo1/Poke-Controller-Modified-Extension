# GPUI Phase 0 inventory

## 1. 目的と判定境界

この文書は、`docs/GPUI_FRONTEND_PLAN.md` Phase 0のinventory artifactです。
現行Web/Tauri baseline、backend ownership、公開契約、UI capability差、最初のvertical slice候補、既知の未受入を一つの表へ固定します。

この文書はGPUIの採用、Cargo依存の追加、`--ui gpui`の追加、`apps.gpui`の追加、既存UIの廃止を決定しません。
採否と実装開始は、Phase 0の証跡およびPhase 1 Gate 1のnative PoC結果を確認してから別途判断します。

監査基準は対象worktreeの現行HEADと監査時点の未コミット差分です。
clean worktreeを要求する受入は、当該inventoryの存在だけでは完了扱いにしません。

## 2. 現行の製品入口とownership

| 表示形態 | 現在の入口 | UI実装 | backend／process | Phase 0判定 |
|---|---|---|---|---|
| Web | `nix run . -- --ui web` | `web/`のSvelteKit／Svelte／TypeScript SPA | `pokecon` Rust processがAxum、state、device、camera、serial、worker lifecycleを所有 | baseline |
| Tauri desktop | `nix run .#tauri` | 同じWeb UIをTauri WebViewで表示 | 同じ`pokecon` binaryへ`--ui desktop`を渡し、Tauri shellを追加 | baseline |
| GPUI native | 入口なし | 未実装 | `--ui gpui`、`apps.gpui`、GPUI依存は存在しない | 将来目標／未受入 |
| GPUI browser／WASM | 製品入口なし | 対象範囲未確定 | 既存Svelte Webのbrowser受入とは分離して扱う | owner decision待ち |

直接根拠:

- `rust/pokecon/src/entrypoint.rs:37-57`は`UiArgument::{Web, Desktop}`と`--ui`の既定値`web`だけを定義する。
- `flake.nix:3253-3261`はpackage binaryを`--ui web`で起動するWeb gateを持つ。
- `docs/ARCHITECTURE.md:9-39`はRust processがhardware／canonical stateを所有し、Web/Tauri clientがREST／WebSocket／WebRTCで接続する境界を定義する。
- `docs/TRACEABILITY_FRONTEND.md:13-25`および`docs/TRACEABILITY_INTEGRATION.md:16-30`はGPUI selectorと入口が未実装であることを直接判定している。

## 3. 操作経路inventory

| 操作／表示 | 現行client側 | Rust／wire境界 | 最初のPoC候補 | 現状の受入判定 |
|---|---|---|---|---|
| state snapshot表示 | `web/src/lib/runtime.ts`、生成OpenAPI型 | REST snapshotとWebSocket差分 | fake snapshotをnative viewへ投影 | Web baselineあり、GPUI未実装 |
| 設定1項目の更新 | settings UI component | OpenAPI request→Rust settings pipeline→read-back | booleanまたはenum 1項目 | Web testあり、GPUI未実装 |
| profile一覧／切替 | `WorkspaceMenu.svelte`等 | profile serviceとrevision付きsnapshot | stale revision／拒否を含む切替 | browser／live refreshは未受入 |
| controller操作 | controller components／input mapping | typed command→Rust controller state→event | 1ボタンのpress/releaseとneutral化 | Web unit/integrationあり、GPUI未実装 |
| camera表示 | `CameraViewport.svelte`、`media.ts` | WebRTC primary／WebSocket JPEG fallback | 最初はbinary JPEG fallbackのみ | backend契約あり、GPUI frame upload未実装 |
| logs／notifications | `OutputGroup.svelte`、realtime runtime | WebSocket／snapshot projection | 1 output groupのsubscribe／clear | Web表示側あり、GPUI未実装 |
| command／dynamic UI | Commands／Other tabs | OpenAPI／worker IPC | workerなしのfake command result | backend contractあり、GPUI未実装 |

操作経路の方針:

1. GPUI viewはhardware handle、worker process、`ProductionRuntime`、canonical stateを直接所有しない。
2. 最初のnative vertical sliceはsnapshot表示→設定1項目の更新→commit/read-backを候補とする。
3. camera／WebRTC、controllerの高頻度入力、Tauri専用path／window capabilityは、最初のsliceへ混ぜない。
4. typed in-process adapterと既存Axum API clientの比較はPoCで行い、先に公開endpointや汎用traitを追加しない。

## 4. UI capability matrix

| Capability | Web | Tauri desktop | GPUI nativeの確認項目 |
|---|---|---|---|
| REST／WebSocket | 既存公開契約 | 同じbackendへWebView接続 | local HTTP clientまたはtyped adapter、Origin制約を比較 |
| WebRTC primary／JPEG fallback | 現行Web経路 | Tauri WebView経路 | native signaling、decode、frame lifetime、texture uploadを別証明 |
| screenshot／file save | browser download等 | Tauri固有path／dialog | byte-returnまたはnative local-saveを選択し、Tauri pathを流用しない |
| clipboard | browser契約 | desktop capability | CJK／IME／Wayland selectionをnative fake viewで検証 |
| keyboard／focus／IME | browser baseline | WebView baseline | 日本語IME、focus navigation、CJK compositionをnativeで実測 |
| accessibility tree | browser／WebView | Tauri accessibility | AccessKit tree、role、Tab遷移をfake dataで検証 |
| window／tray／close | browser tab | Tauri window/tray/single-instance | close、quit、signal、fatal backend failureを既存shutdownへ接続 |
| profile／state | snapshot projection | 同一backend | UIは表示projection、Rustがcanonical state owner |

現段階ではGPUI列を「未検証」とし、Web/Tauriの成功をGPUIの成功へ転用しません。
`disable_compositing`、`web_dir`、Tauri screenshot pathなどdesktop／WebView専用設定も、GPUIへ表示・適用しない前提で個別判定します。

## 5. baseline gateと既知の未受入

### 5.1 選定したbaseline gate

以下をGPUI追加前後で比較するbaseline gateとします。いずれもNix経由で実行し、GPUI未実装の現状成功とGPUI受入を混同しません。

- `nix run .#cli-help-check`
- `nix run .#contract-check`
- `nix run .#cargo-test`
- `nix run .#clippy`
- `nix run .#build-rust`
- `nix run .#web-check`
- `nix run .#check`
- `nix run .#compatibility`
- `nix run .#release-check`
- `nix run .#acceptance-record-check`
- Normal／Package required aggregateと同一SHAのCI read-back

現行sourceに対するこれらのgate成功は、GPUI native window、IME、accessibility、native media、実browser、実機性能の証拠ではありません。

### 5.2 GPUIと混同しない未受入

| 項目 | 現在の判定 | GPUI Phase 0での扱い |
|---|---|---|
| `auto_reload_config` OS-native watcher | 未実装 | GPUI作業と分離し、owner decision後にbackend packetとして扱う |
| production latency／throughput／jitter／soak | 未完了 | CI critical-path timingと製品性能を分離し、GPUI採否の根拠にしない |
| external browser／tailnet WebRTC | 外部証跡待ち | GPUI native PoCの成功／失敗に転用しない |
| real hardware／driver／firmware／console | 外部証跡待ち | virtual I/Oをnative実機の代用にしない |
| clean detached worktree | 未成立 | 既存worktree制約下で完了扱いしない |
| Release tag／tag起点Release | 利用者担当 | 明示指示なしに実施しない |
| GPUI dependency／license closure | 未採用 | candidate release、lock、target closure、license、native libsをPhase 1で再調査する |

## 6. Phase 0の完了条件と未決事項

### 6.1 Inventoryとして今回固定したもの

- 現行Svelte Web／Tauri desktop／共通Rust backendの責務境界。
- `--ui web|desktop`、Nix package／appの現行入口。
- state、settings、profile、controller、camera、logsの最初の操作経路。
- Web／Tauri／GPUIのcapability差と、GPUIへ流用してはいけないTauri専用要素。
- baseline gate一覧と、GPUIとは別に残る外部／性能／owner decision項目。
- 最初の候補vertical sliceと、camera／WebRTCを後段へ分離する理由。

### 6.2 まだ完了扱いにしない条件

- clean worktreeと基準commitを含む外部受入recordがないこと。
- native window、Tokio backend共存、CJK／IME、clipboard、focus、accessibility treeを実測していないこと。
- `gpui-kit`の採用release、exact lock、target依存、license／notice、Nix native closureをPoke-Con上で確定していないこと。
- backend接続方式、`--ui gpui`、`apps.gpui`、native media実装を決定していないこと。
- Gate 1を通過しておらず、既存Svelte／Tauriを置換する根拠がないこと。

## 7. 次のowner decision

Phase 0 inventoryの次に必要なのは、次の決定です。決定がないままCargo依存やselectorを追加しません。

1. GPUI native PoCをPhase 1へ進めるか。
2. GPUI browser／WASMを今回の製品対象に含めるか、それともSvelte Webを維持するか。
3. native PoCで比較するbackend接続方式（typed in-process adapter／既存API client）の評価条件。
4. candidate release、license／notice、Nix native dependencyの受入責任者と判定基準。

この決定後にだけ、`docs/GPUI_FRONTEND_PLAN.md` Phase 1の最小fake-data view packetを作成します。
