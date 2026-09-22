# GPUIフロントエンド段階導入計画

> 対象読者: Poke-Con EXの開発者および実装支援エージェント。利用者向け操作説明ではない。
>
> 状態: 実装前の段階導入計画。GPUIベースのフロントエンドを実装する方向性を定めるが、PoCの検証結果を待たずに現行UIを廃止する計画ではない。
>
> 作業ブランチ: `refactor/rust-core`。計画作成時の基準commit: `66197f52397bbec606624c9f3e3c93064cfc957d`。

## 1. 目的と判断

GPUIを利用したフロントエンドを、現行Svelte/Tauri/Web UIと並行して導入する。Rustバックエンドとフロントエンドの責務を分け、UIがデバイスや正準状態を直接所有しない境界を維持する。一方で、将来の独立再利用や交換可能性のためだけに別crate、汎用plugin機構、過剰なtrait階層を追加しない。

Rustリファクタリング全体が大規模なPoC段階であることを前提とする。GPUIはまず、実際のPoke-Conの主要経路が成立するかを小さい縦切りで確認する。PoC中は現行UIとバックエンドを動作可能な基準経路として残し、UI置換とバックエンド再設計を同じcheckpointで行わない。

初期のUI部品候補は`longbridge/gpui-kit`とする。これはネイティブUIを作るための候補であり、同プロジェクトのWASMギャラリーだけを理由にPoke-Conブラウザ版での採用が実証済みとは扱わない。GPUI KitとGPUIの正確な対応版は実装開始時に公開manifest・lock情報で再確認し、浮動する`main`依存は使わない。

## 2. 現行構成と前提

以下は計画の出発点であり、実装前に該当コードとテストを再確認する。

- Rust本体は`rust/pokecon/`の単一Cargo package `pokecon`。同一Rust processがAxum server、状態、camera、serial、lifecycle等を所有する。workerは別processで、既存IPC契約を使う。
- 現行フロントエンドはSvelteKit/Svelte/TypeScriptのSPA。Web modeではブラウザからAxumへ接続し、desktop modeではTauri WebViewを加える。両方とも同じRust backendと公開APIを使う。
- 起動CLIは`rust/pokecon/src/entrypoint.rs`の`--ui web|desktop`で、現在の既定値は`web`。desktop固有のOrigin許可、スクリーンショット動作、window/tray lifecycle等はTauriに関係するcapabilityを持つため、GPUIへ機械的に引き継いではならない。
- `docs/ARCHITECTURE.md`はWeb UIをREST、WebSocket、WebRTCを介した表示・操作clientとし、Rust側をhardware resourceと正準stateの所有者としている。HTTP/OpenAPI契約の正本は`rust/pokecon/src/server/api.rs`および`api/openapi.json`。TypeScript型は生成物である。
- 現行のNix `apps.default`は同じ`pokecon`実行ファイルを起動する。既存計画にも同一binaryのWeb/Desktop mode受入が記録されている。新旧UIを別backend binaryへ分岐させない。
- 現行`SPECIFICATION.md`にはSvelteKit/TailwindおよびTauri/Webの契約が記載されている。本計画はそのファイルを変更しない。PoC中に既存経路を残し、製品契約との最終整合はPoC結果をもとに別途決定する。GPUI経路を追加しただけで既存のUI要件が満たされたとは判定しない。

## 3. 目標の起動構成

Nixで現行UIとGPUI UIを明示的に選んで起動できるようにする。選択用flake appは同じ`pokecon` package／同じbackend実行ファイルを呼び、UI選択以外の設定、worker、API、backend lifecycleを重複させない。

計画上の利用形は次のとおり。flake app名とCLI引数の最終形は、既存CLI互換性と実装時のmode/capability分割を確認して確定する。

```text
nix run .#pokecon-web       # 既存のSvelte Web mode
nix run .#pokecon-desktop   # 既存のSvelte + Tauri desktop mode
nix run .#pokecon-gpui      # GPUI native frontend + 同じRust backend
```

既存の`nix run .`と`--ui web|desktop`の動作、既定値、`--help`互換性はPoC中に不用意に変更しない。必要なら既存CLI optionは互換入口として保持し、Nix app側で明示的なmodeを渡す。

GPUI native版でloopbackのAxum APIを必須にするか、最初から内部serviceへ直結するかは決め打ちしない。PoCでは、(a) command/query/eventを表す小さなtyped UI adapterを通じた同一process内連携と、(b) 既存Axum APIを使うclient方式を比較し、密結合を避けつつ追加の抽象化・transport負担を最小化する方式を選ぶ。どちらの方式でもGPUI viewからcamera/serial/worker/controllerの内部serviceやnative handleを直接操作せず、backendが唯一の正準状態所有者であることを保つ。Browser/WASM frontendは既存の公開REST/WS/WebRTC契約を使い、backendのUI都合でその契約を重複定義しない。

```text
nix run selector
        |
        v
one pokecon executable / one backend process
        |                                      |
        v                                      v
Svelte web or Tauri WebView                GPUI native view
        |                                      |
REST / WS / WebRTC                  typed in-process adapter OR API client
        |                                      |
        +---------------- one backend owner ---+
```

WebAssembly版は別のbackendでも独立した製品成果物でもない。後続PoCで同じGPUI view/clientをWASMへビルドし、ブラウザのFetch/WebSocket/WebRTC等を介して既存Web backendに接続できる場合に限り、GPUI Web modeを追加する。

## 4. 責務と変更境界

1. **Backend**はhardware、worker、settings、profile、controller state、media producer、server、graceful shutdownの正準所有者であり続ける。既存の主要な制御・通信契約を維持する。
2. **Frontend**は表示、入力、画面内navigation、ユーザー操作を担当し、独自の正準device stateや二重のlifecycleを作らない。サーバーsnapshotとeventから表示状態を復元できる。
3. **UI integration**は同じRust package内の小さなprivate module/adapterから開始する。必要が明らかになる前に別crate、汎用trait hierarchy、public reusable APIを作らない。UIコードが`application_backend`等を直接知る設計は避ける。native GPUIではtyped in-process adapterと既存API clientをPoCで比較し、妥当な境界を選ぶ。
4. **契約生成**はbrowser/WASM用の既存OpenAPIとgeneratorを維持する。wire型をUI用に手作業で複製しない。native GPUIのprivate command/query/event型は公開wire型と混同せず、両clientで共通化する対象と内部に留める対象をPoCで決める。
5. **UI固有capability**は明示的なmatrixで扱う。Web、Tauri、GPUIごとにOrigin/Host、screenshot save/download、file chooser、clipboard、single-instance、tray、window close、shutdown policyを検査する。Tauri固有動作をGPUI modeへ誤って許可・適用しない。
6. **互換性**は現行のSvelte Web/Desktopを回帰可能なまま維持する。backendや公開APIの変更を避けられない場合は、理由・contract diff・既存UIへの影響・rollbackをそのcheckpointで記録する。

## 5. 段階とcheckpoint

各項目は完了証跡を得るまで未完了とする。各実装checkpointではNix経由の対象gateと`nix run .#check`を通し、該当しないgateは理由と代替証跡を記録する。ハードウェア実機を前提とする受入は置かず、virtual backend、no-network fake、loopback、browser automationを優先する。

### Phase 0 — 基準線と対象範囲を固定する

- [ ] 現行のclean状態と基準commitを記録し、Svelte Web、Tauri Desktop、共通backend/APIのbuild・起動・停止gateを選ぶ。
- [ ] `SPECIFICATION.md`の画面機能、低遅延要件、ブラウザ対応、Tauri lifecycle、キーボード/IME/アクセシビリティ要件を、移行対象・非対象・要確認へ対応づける。仕様本文自体は変更しない。
- [ ] 現行UIの各主要操作からREST/WS/WebRTCまでの経路をinventory化し、最初のvertical sliceを選ぶ。基本候補は状態snapshot表示、設定の一項目更新、controller操作、camera表示の順とする。
- [ ] 現行起動とUI capabilityのテストを、GPUI追加後にも回せるbaselineとして記録する。完了条件は既存経路の実行結果、CLI `--help`、API/OpenAPI差分なし。

### Phase 1 — GPUI Kitの最小platform PoC

- [ ] `gpui-kit`の採用release、GPUI snapshotの完全一致、Apache-2.0および同梱・推移依存license、必要なNix native librariesを確定する。`0.x`のAPI変更は想定し、更新は対応する全snapshot一式で行う。
- [ ] Poke-Conの独立したPoC viewを一つ作る。設定入力、CJK文字、clipboard、日本語IME、keyboard focus/navigation、アクセシビリティtree、theme/font fallbackを確認する。
- [ ] 同一view crate/moduleをnative targetでbuild・起動し、別に`wasm32-unknown-unknown`でcompileしbrowser canvas上へ表示する。最初はfake dataを使いbackend統合を混ぜない。
- [ ] WASMギャラリーの成功をアプリ全体の成功証明にしない。single canvas/window制約、threading/COOP-COEP要件、WebGPU/WebGL2、browser fonts、入力、screen readerの限界を確認する。
- **Gate 1:** native起動とWASM表示の両方が成立し、IME/CJK/clipboard/accessibilityの未解決点と回避案が文書化されること。成立しない場合は全面実装へ進まず、blocking issueと継続判断を記録する。

### Phase 2 — 並行起動選択とfrontend/backend接続

- [ ] 同一`pokecon` binaryからSvelte Web、Svelte/Tauri desktop、GPUI nativeを明示起動できるNix appsを追加する。各appは同一package binaryへ異なる明示selectorを渡し、backendやworkerを重複起動しない。
- [ ] UI selectorとbackend execution/capability policyを分離する。現在の`--ui web|desktop`がTauri固有Originやscreenshot modeにも使われている箇所を調査し、GPUIを単に`Desktop`へaliasしてTauri権限を流用しない。旧CLI呼び出しは互換動作を保つ。
- [ ] native GPUIのtyped in-process adapter案と既存Axum API client案をfake backendおよびsoftware-only runtimeで比較する。少なくともstate snapshot、revision付き変更通知、設定mutation一つのrequest/commit/read-backを検証し、追加coupling、transport/serialization、UI起動・停止への影響を記録して採用案を選ぶ。
- [ ] server start/stop、shutdown coordinator、設定、profile、worker起動に二重所有がないことを検証する。UI起動失敗時にもbackend/workerを残すか閉じるかを既存lifecycle契約に沿って決定し、全終了経路をtestする。
- **Gate 2:** 1 process/1 backendを維持し、三つの起動入口を独立選択できること。GPUIが内部resource serviceを直接呼ばず、UI/backend間の所有権と失敗/read-back契約が明示されること。browser向け既存API contractのdiffがなければそのまま次へ進む。拡張が必要ならOpenAPI同期、互換性検査、両frontend testを追加する。

### Phase 3 — 制御と設定の縦切り

- [ ] status/state snapshot、一設定更新、profile切替、controller入力の順にGPUIへ移す。各操作はUI入力→選定したquery/command境界→backend commit→event/snapshotのread-back→UI表示まで確認する。
- [ ] stale revision、server拒否、切断、再接続、UI window close、操作中shutdownで誤った成功表示や二重実行がないことをno-network fake/integration testで確認する。
- [ ] 代表的な画面でTkinter機能・視覚parityの要件を満たすか比較する。見た目の最終調整は操作の意味と状態遷移が一致した後に行う。
- **Gate 3:** 少なくとも一つの実ユーザーフローが選定した明示的なUI/backend境界で完結し、失敗・再接続・read-backの契約が検証されること。内部resource serviceへの直接callやbackend/UI間の重複した正準stateを必須にする構造なら境界案を見直す。

### Phase 4 — Camera/mediaと性能の最大リスクを先に検証

- [ ] 現行のWebRTC primary、Motion JPEG/WebSocket fallback、再接続・fallback復帰をGPUI nativeで受信・描画できる最小media viewを作る。GPUI canvasが`MediaStream`/`<video>`等のDOM mediaを自動的に扱えると仮定しない。
- [ ] カメラ映像を選択するnative client/transport、frame format/conversion、texture upload、frame lifetimeの具体経路をPoCで選ぶ。Backend camera ownerは維持し、camera/serial handleをGPUIへ渡さない。
- [ ] カラーピッカー、crop/screenshot、touchscreen area選択など映像上の入力を代表例として検証する。WebRTC不可時にも現在のfallback契約が失われないことを確認する。
- [ ] mock/virtual cameraとloopbackでlatency・frame drops・throughput・CPU/memory・shutdown cleanupを測定する。`SPECIFICATION.md`の数値目標を採用判定に用い、実測値がないのに達成を主張しない。
- **Gate 4:** 主要camera操作、WebRTC/fallback、対象latency、継続描画、終了時解放をvirtual/loopback条件で証明する。WebRTC表示の方法がない、または性能/入力要件を満たさない場合は全画面移植を停止し、native video host等の代案を比較する。

### Phase 5 — 残りの画面を縦切りで移行

- [ ] Gate 3・4後に画面を機能単位で移す。各画面はUI操作、API contract、失敗状態、再接続、アクセシビリティ、既存Svelteとの差分を同じcheckpointで完結させる。
- [ ] 優先順はcamera/control、settings/devices、profiles/commands、dynamic configuration/script UI、logs/notifications等とし、依存と利用頻度をPhase 0 inventoryで再確認して調整する。
- [ ] 画面間navigation、複数pane相当の情報配置、dialog/menu、window resizeをGPUI single-window内で実現できるか確認する。Tauriのtray/single-instance/close behaviorはPhase 6のnative shell gateまで代替済みと扱わない。
- [ ] Svelte経路も継続してbuild・test・launchできることを各主要checkpointで確認する。

### Phase 6 — Native lifecycleとpackage integration

- [ ] GPUI windowのclose、app quit、OS signal、fatal backend failureを一つの既存`ShutdownCoordinator`へ接続する。最後のwindowの`ask`/`shutdown`/`keep_backend`意味を明示してテストする。
- [ ] single-instance、tray、window reopen、file chooser、screenshot save、clipboard、通知、WebView compositing専用設定の適用可否を個別に整理する。GPUI Kitの存在だけでTauri機能同等とは扱わない。
- [ ] Windows/Linux Nix build、Debian/NSIS package、署名/reproducibility、clean install、update/uninstall、起動selectorを検査する。既存配布物をPoC途中で置換しない。
- **Gate 6:** 必須lifecycleと対応OS配布、package smoke、既存UI rollbackが検証されること。置換できない製品要件は未実装のまま隠さず、差分と選択肢を記録する。

### Phase 7 — GPUI WebAssemblyのアプリ受入

- [ ] Phase 1の同一view compileだけでなく、必要な主要画面とAPI clientをWASM targetでbuildし、production相当の静的hostへdeployする。
- [ ] Browser Fetch、WebSocket、WebRTC受信とvideo/frame presentationを実動作で確認する。Safari 16.4+を含む既存browser baseline、CJK/IME、clipboard、keyboard、accessibility、screen reader、focus/Tab動作をbrowser matrixで受け入れる。
- [ ] マルチthreadingを選ぶ場合だけ、SharedArrayBufferとCOOP/COEPを含む本番hosting headerを設計・testする。シングルthread設計で十分なら、不要なcross-origin isolationを追加しない。
- [ ] backend serverから配信するSPA、別静的host、GPUI canvasを埋め込む構成のいずれかを、Origin/security contract・運用・cache・asset/font配信と合わせて決める。未決のまま本番対応を宣言しない。
- **Gate 7:** 既存browser機能を壊さず、SPECの対象browserとcamera/control要件を満たす本番受入証跡が揃うこと。GPUI browserが満たせなければ、GPUI nativeとSvelte browserを別UIとする案はユーザー判断へ戻し、仕様変更なしに共通UI要件を達成したとはしない。

### Phase 8 — 採否・段階的切替

- [ ] Gate 1–7の証跡から採用範囲を決定する。GPUI nativeのみ採用、native+WASM採用、PoC中断のいずれも結果として許容する。
- [ ] GPUI経路を既定にする前に、現行UIに対する全主要要件parity、performance、crash/restart、package、CI、accessibility、browser受入を完了する。
- [ ] 旧Svelte/Tauri起動selectorとrollback用Nix appは、利用者が移行完了を承認するまで削除しない。
- [ ] 製品契約変更が必要な場合だけ、別途承認された作業として`SPECIFICATION.md`、開発文書、受入gateを同期する。本計画の更新を仕様変更の代替にしない。

## 6. PoC合否条件と停止条件

**継続条件**

- 同じRust backendと同じ実行ファイルで、現行UIとGPUIを選んで起動・停止できる。
- UIはbackend内部のcamera/serial/worker/controller所有serviceを直接操作せず、API上の結果をread-backして表示する。
- 主要control flowとmedia flowが現行の挙動・低遅延目標を満たし、worker/API/setting contractを不必要に変えない。
- 対象OS、ブラウザ、IME、accessibility、native lifecycle、packageで必須要件を実証するか、未達要件を明確な判断事項へ残す。
- `nix run`の各選択肢が同じpackage/binaryを使い、回帰gateが全て再現可能である。

**停止または再設計条件**

- camera/WebRTCの描画経路がない、または目標latency/fallbackを達成できない。
- GPUI browserで対象browser・IME・accessibility要件を満たせない。
- 安全なbackend境界を維持するにはAPI契約の大規模変更やUI専用hardware ownershipが必要となる。
- Tauriの必須lifecycle/配布契約が代替できず、実装負担がPoC便益を上回る。

停止時は現行UIを残し、どのgateで不成立となったか、GPUI native-only等に縮退するかを意思決定記録へ残す。PoCを通らなかったことを理由に、現行仕様や既存APIを暗黙に縮小しない。

## 7. 参照先

### リポジトリ内

- `SPECIFICATION.md` — UI機能、ブラウザ/OS、latency、camera、keyboard、screenshot、lifecycleの目標契約。変更禁止。
- `PLAN.md` — 現在進行中のRust refactor、既存gate、CI状態、既存同一binary Web/Desktop受入記録。
- `docs/ARCHITECTURE.md` — backend ownership、process topology、frontend責務、state/API境界。
- `docs/HTTP_API.md`、`api/openapi.json`、`rust/pokecon/src/server/api.rs` — 公開HTTP/API contract。
- `rust/pokecon/src/entrypoint.rs` — 現行`--ui web|desktop` selectorと起動lifecycle。
- `rust/pokecon/src/desktop/mod.rs`、`rust/pokecon/src/runtime/` — Tauri shellおよび共通shutdown境界。
- `rust/pokecon/tests/ui_boundary_acceptance.rs`、`rust/pokecon/tests/startup.rs` — UI capability、同一SPA/API、startup modeの受入。
- `web/src/lib/`、`web/src/routes/` — 現行Svelte state/API/media/UI。
- `flake.nix` — `apps.default`、`packages.pokecon`、`packages.web`、Nix task/runtime/package構成。

### GPUI / GPUI Kit 一次情報

- GPUI Kit: <https://github.com/longbridge/gpui-kit>
- GPUI Kit Web gallery: <https://github.com/longbridge/gpui-kit/tree/main/crates/story-web>
- GPUI Kit Web build workflow: <https://github.com/longbridge/gpui-kit/blob/main/.github/workflows/release-website.yml>
- GPUI Kit regular CI: <https://github.com/longbridge/gpui-kit/blob/main/.github/workflows/ci.yml>
- Zed GPUI Web backend constraints: <https://github.com/zed-industries/zed/tree/main/crates/gpui_web>

## 8. 進捗

- [x] 作業対象を既存`refactor/rust-core` branch/worktreeに固定し、現行CLI、backend ownership、OpenAPI境界、Nix default launcher、仕様との既知のずれを計画の前提として記録する。
- [ ] Phase 0 — 現行要件と実装baselineを再照合する。
- [ ] Phase 1 — GPUI Kit native/WASM minimum PoC。
- [ ] Phase 2 — 同一binaryのNix UI selectorsとAPI接続。
- [ ] Phase 3 — control/settings vertical slice。
- [ ] Phase 4 — media/performance gate。
- [ ] Phase 5 — 残り画面。
- [ ] Phase 6 — native lifecycle/package。
- [ ] Phase 7 — browser/WASM app acceptance。
- [ ] Phase 8 — 採否・段階的切替。
