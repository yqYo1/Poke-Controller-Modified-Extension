# GPUI Phase 0 inventory

## 1. 目的と判定境界

この文書は、`docs/GPUI_FRONTEND_PLAN.md` Phase 0のinventory artifactです。
現行Web/Tauri baseline、backend ownership、公開契約、UI capability差、最初のvertical slice候補、既知の未受入を一つの表へ固定します。

この文書のPhase 0表は、実装開始前のbaseline inventoryとして凍結した記録です。後続packetでCargo依存、`--ui gpui`、`apps.gpui`、native fake viewを追加しましたが、Phase 1 Gate 1のnative PoC合否や既存UIの廃止はまだ決定していません。

監査基準は対象worktreeの現行HEADと監査時点の未コミット差分です。
clean worktreeを要求する受入は、当該inventoryの存在だけでは完了扱いにしません。

## 2. 現行の製品入口とownership

| 表示形態 | 現在の入口 | UI実装 | backend／process | Phase 0判定 |
|---|---|---|---|---|
| Web | `nix run . -- --ui web` | `web/`のSvelteKit／Svelte／TypeScript SPA | `pokecon` Rust processがAxum、state、device、camera、serial、worker lifecycleを所有 | baseline |
| Tauri desktop | `nix run .#tauri` | 同じWeb UIをTauri WebViewで表示 | 同じ`pokecon` binaryへ`--ui desktop`を渡し、Tauri shellを追加 | baseline |
| GPUI native | `nix run .#gpui`、`--ui gpui` | `gpui-kit 0.6.6`のfake-data native view | 同じ`pokecon` binary、optional `gpui` feature、専用GPUI UI thread | PoC実装済み／Gate 1未受入 |
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
| `auto_reload_config` OS-native watcher | 実装済み・current remote acceptance未完了 | `rust/pokecon/src/dynamic_watcher.rs`（notify 7、single-global、debounce、single-flight、停止／reap）と17件のfocused test。GPUI作業とは分離し、current remote／full product acceptanceはbackend gateで扱う |
| production latency／throughput／jitter／soak | 未完了 | CI critical-path timingと製品性能を分離し、GPUI採否の根拠にしない |
| external browser／tailnet WebRTC | 外部証跡待ち | GPUI native PoCの成功／失敗に転用しない |
| real hardware／driver／firmware／console | 外部証跡待ち | virtual I/Oをnative実機の代用にしない |
| clean detached worktree | 未成立 | 既存worktree制約下で完了扱いしない |
| Release tag／tag起点Release | 利用者担当 | 明示指示なしに実施しない |
| GPUI dependency／license closure | `gpui-kit 0.6.6` exact dependencyとLinux native inputsを実装済み | Cargo.lock／license closure／配布noticeの最終監査は未完了。採否へ転用しない |

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
- Xvfbのnative GPU surface起動はGLX visual／adapter不足で失敗する一方、Weston headless Wayland＋Mesa llvmpipe GL software rendererではproduction readiness（`POKECON-RUNTIME-0001`）、`Gpui` UI mode、`DesktopExit` shutdown request／clean stop（`POKECON-RUNTIME-0003`／`0002`）を`gpui-native-wayland-runtime-report.json`へ保存した。Vulkan adapter初期化失敗はexpected GL fallbackとして記録し、panic／unexpected errorはない。なおIME／clipboard／focus／accessibility treeの実window操作証跡は別に不足しており、GPUI test-supportのfake-view testだけではGate 1を通過させない。
- `gpui-kit 0.6.6`のexact lockとApache-2.0宣言、fontconfig／freetype／libxkbcommon／XCBのNix inputsは確認済みだが、全target license／noticeと配布runtime closureは未完了であること。
- backend接続方式、`--ui gpui`、`apps.gpui`、native media実装はPoC入口まで決定済みだが、vertical slice／media受入は未完了であること。
- Gate 1を通過しておらず、既存Svelte／Tauriを置換する根拠がないこと。

- `gpui-kit 0.6.6` exact dependency、`gpui`／`gpui-test-support` feature、Linux native inputs（fontconfig、freetype、libxkbcommon、XCB、Mesa、Vulkan loader）と同一binary selectorを実装した。Cargo／clippy／test-support／CLI help／flake provenanceはlocalでpassしている。
- Xvfb下のreal native windowはGLX visual／GPU adapter不足でfailした。`No suitable GPU adapters`／`Unable to find a compatible adapter`は正直なruntime blockerとして保存し、fake-view testやheadless platformをnative Gate 1の代替にしない。
- `nix build .#pokecon`と`nix run .#ui-package-check`は、GPUI filesをintent-to-addした現行dirty worktreeでpassした（package store pathとapplication SHA-256はrun artifactに記録）。初回はGit source filterがuntracked GPUI filesを除外して`file not found for module gpui`となったため、clean tracked source／commit別証跡とは別扱いである。

## 7. Phase 1実装checkpoint

現行差分で実装した入口と、Gate 1未受入の証跡境界は次のとおりです。

## 8. 次のowner decision

Phase 1 Gate 1のruntime証拠取得後に必要な決定です。決定がないまま既存UIを置換しません。

1. GPUI native PoCをPhase 1へ進めるか。
2. GPUI browser／WASMを今回の製品対象に含めるか、それともSvelte Webを維持するか。
3. native PoCで比較するbackend接続方式（typed in-process adapter／既存API client）の評価条件。
4. candidate release、license／notice、Nix native dependencyの受入責任者と判定基準。

この決定は既存UIの採否・置換判断を拘束します。fake-data view packetは実装済みですが、Gate 1受入と採用決定がない限り本番UI移行へ進めません。
