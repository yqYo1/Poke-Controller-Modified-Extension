# GPUIフロントエンド段階導入計画

> 対象読者: Poke-Con EXの開発者および実装支援エージェント。利用者向け操作説明ではない。
>
> 状態: 実装前の段階導入計画。GPUIベースのフロントエンドを実装する方向性を定めるが、PoCの検証結果を待たずに現行UIを廃止する計画ではない。
>
> Phase 0 inventory: [`GPUI_PHASE0_INVENTORY.md`](GPUI_PHASE0_INVENTORY.md)。これは現行baselineと未決事項を記録するartifactであり、GPUI採用・依存追加・selector実装の証拠ではない。
> 作業ブランチ: `refactor/rust-core`。計画作成時の基準commit: `66197f52397bbec606624c9f3e3c93064cfc957d`。

## 1. 目的と判断

GPUIを利用したフロントエンドを、現行Svelte/Tauri/Web UIと並行して導入する。Rustバックエンドとフロントエンドの責務を分け、UIがデバイスや正準状態を直接所有しない境界を維持する。一方で、将来の独立再利用や交換可能性のためだけに別crate、汎用plugin機構、過剰なtrait階層を追加しない。

Rustの構造移行は`PLAN.md`のPhase 2.1–2.7で完了と記録されている。一方、大規模リファクタリング後の製品全体はまだPoC評価段階であり、production主経路のlatency/throughput、実機安定性、外部browserからのWebRTC/fallback受入は未証明である。GPUIはその未検証backendを同時に作り直すのではなく、既存Rust coreを使う追加のUI PoCとして、小さい縦切りで評価する。既存UIとbackendをrollback可能な基準経路として残し、UI追加とbackend再設計・性能改善を同じcheckpointで混ぜない。

初期のUI部品候補は`longbridge/gpui-kit`とする。これはネイティブUIを作るための候補であり、同プロジェクトのWASMギャラリーだけを理由にPoke-Conブラウザ版での採用が実証済みとは扱わない。GPUI KitとGPUIの正確な対応版は実装開始時に公開manifest・lock情報で再確認し、浮動する`main`依存は使わない。

## 2. 現行構成と前提

以下は計画の出発点であり、実装前に該当コードとテストを再確認する。

- Rust本体は`rust/pokecon/`の単一Cargo package `pokecon`。同一Rust processがAxum server、状態、camera、serial、lifecycle等を所有する。workerは別processで、既存IPC契約を使う。
- 現行フロントエンドはSvelteKit/Svelte/TypeScriptのSPA。Web modeではブラウザからAxumへ接続し、desktop modeではTauri WebViewを加える。両方とも同じRust backendと公開APIを使う。
- 起動CLIは`rust/pokecon/src/entrypoint.rs`の`--ui web|desktop`で、現在の既定値は`web`。desktop固有のOrigin許可、スクリーンショット動作、window/tray lifecycle等はTauriに関係するcapabilityを持つため、GPUIへ機械的に引き継いではならない。
- `docs/ARCHITECTURE.md`はWeb UIをREST、WebSocket、WebRTCを介した表示・操作clientとし、Rust側をhardware resourceと正準stateの所有者としている。HTTP/OpenAPI契約の正本は`rust/pokecon/src/server/api.rs`および`api/openapi.json`。TypeScript型は生成物である。
- 現行Nix packageは`packages.default`/`packages.pokecon`で、`apps.default`はその`pokecon`実行ファイルをWeb modeの既定値で起動し、`apps.tauri`は同じ実行ファイルへ`--ui desktop`を渡す。既存のユーザー向け起動形は`nix run . -- --ui web`と`nix run .#tauri`。GPUIでもこの同一package/binaryの方針を守り、新しいbackend実行ファイル/packageへ分岐させない。
- 規範要件は`SPECIFICATION.md`を入口とするbackend／frontend／integrationの三定義書に分割した。GPUI追加と既存UI維持は明示された製品目標であり、実装技術を固定していた旧UI条項は機能要件とbackend契約へ整理済み。未実装の要件を実装済みとして扱わず、PoC gateで証明する。

## 3. 目標の起動構成

Nixで現行UIとGPUI UIを明示的に選んで起動できるようにする。選択用flake appは同じ`pokecon` package／同じbackend実行ファイルを呼び、UI選択以外の設定、worker、API、backend lifecycleを重複させない。

現行の利用形と、追加する候補の利用形は次のとおり。`nix run .#gpui`は未実装の計画上の入口であり、Phase 2で追加する。既存の`apps.default`と`apps.tauri`の名前・動作は変更しない。

```text
nix run . -- --ui web       # 既存: Svelte Web mode (apps.default)
nix run .#tauri             # 既存: Svelte + Tauri desktop (apps.tauri)
nix run .#gpui              # 計画: GPUI native + 同じRust backend
```

既存CLIは現在`--ui web|desktop`のみを受け付ける。追加する`--ui gpui`はCLIの既定値やWeb/Desktopの動作を変えず、新しい同一binary内の表示形態として追加する。`nix run .#gpui`は`apps.tauri`と同じ薄いwrapperとして、同じ`packages.pokecon/bin/pokecon`に`--ui gpui`を渡す。

GPUI nativeも同一Rust process内で起動し、既存の`run_configured_controlled`が持つ単一backend lifecycleと`ProductionRuntime`を共有する。backend接続方式は小さなtyped in-process boundaryと既存Axum API clientをPoCで比較し、latency、failure handling、Origin/path制約、型安全性から選択する。どちらもcamera/serial/worker/controller内部serviceやnative handleを直接操作せず、正準state/resourceを二重所有しない。不要なloopback HTTPや汎用trait階層も強制しない。Browser/WASM frontendは既存の公開REST/WS/WebRTC契約を使い、wire schemaはOpenAPI正本から生成する。

```text
nix run .               nix run .#tauri             nix run .#gpui
     |                         |                          |
apps.default               apps.tauri                 planned app
     |                         |                          |
pokecon --ui web           pokecon --ui desktop       pokecon --ui gpui
Svelte Web                 Svelte/Tauri              GPUI native
     \_________________________|__________________________/
                               v
             one run_configured_controlled/backend owner
                               |
                    REST/WS/WebRTC for Web clients
```

WebAssembly版は別のbackendでも独立した製品成果物でもない。ブラウザからの同一UI共有は以前の検討対象だが、現時点ではnative PoCと同時に完了しなければならない必須条件とは確定していない。Phase 7で既存Svelte browser baselineと分けて評価し、同じGPUI viewをWASMへビルドしFetch/WebSocket/WebRTC等を介して既存Web backendに接続できるかを判断する。現行`PLAN.md`に記録されたbrowser backend `410 Gone`による外部WebRTC受入未証明は、GPUIの失敗とも成功とも扱わず別途解消・分類する。

## 4. 責務と変更境界

1. **Backend**はhardware、worker、settings、profile、controller state、media producer、server、graceful shutdownの正準所有者であり続ける。既存の主要な制御・通信契約を維持する。
2. **Frontend**は表示、入力、画面内navigation、ユーザー操作を担当し、独自の正準device stateや二重のlifecycleを作らない。サーバーsnapshotとeventから表示状態を復元できる。
3. **UI integration**は既存Axum API clientと同じRust package内の小さなtyped private adapterをPoCで比較する。native GPUI shellとWeb/Tauri shellはいずれも既存のbackend supervisorへ接続し、唯一の`ProductionRuntime`、router、shutdown coordinator、resource ownerを共有する。必要が明らかになる前に別crate、汎用trait hierarchy、public reusable APIを作らない。UIコードが`application_backend`やhardware resource serviceを直接操作する設計は避ける。
4. **契約生成**はbrowser/WASM用の既存OpenAPIとgeneratorを維持する。wire型をUI用に手作業で複製しない。native GPUIのprivate command/query/event型は公開wire型と混同せず、両clientで共通化する対象と内部に留める対象をPoCで決める。
5. **UI固有capability**は明示的なmatrixで扱う。Web、Tauri、GPUIごとにOrigin/Host、screenshot save/download、file chooser、clipboard、single-instance、tray、window close、shutdown policyを検査する。Tauri固有動作をGPUI modeへ誤って許可・適用しない。
6. **互換性**は現行のSvelte Web/Desktopを回帰可能なまま維持する。backendや公開APIの変更を避けられない場合は、理由・contract diff・既存UIへの影響・rollbackをそのcheckpointで記録する。

## 5. 段階とcheckpoint

各項目は完了証跡を得るまで未完了とする。各実装checkpointではNix経由の対象gateと`nix run .#check`を通し、該当しないgateは理由と代替証跡を記録する。ハードウェア実機を前提とする受入は置かず、virtual backend、no-network fake、loopback、browser automationを優先する。

### Phase 0 — 基準線と対象範囲を固定する

- [ ] 現行のclean状態と基準commitを記録し、Svelte Web、Tauri Desktop、共通backend/APIのbuild・起動・停止gateを選ぶ。
- [ ] frontend／integration定義書の機能要件と必須要件を照合し、GPUIでも維持する操作、Web専用要件、Tauri専用capability、native受入が必要なkeyboard/IME/accessibility項目を対応づける。変更が必要な製品要件は本計画だけで変更せず、対応する定義書と受入条件を同一作業で更新する。
- [ ] 現行UIの各主要操作からREST/WS/WebRTCまでの経路をinventory化し、最初のvertical sliceを選ぶ。基本候補は状態snapshot表示、設定の一項目更新、controller操作、camera表示の順とする。
- [ ] 現行起動とUI capabilityのテストを、GPUI追加後にも回せるbaselineとして記録する。`PLAN.md`が報告するbrowser backend `410 Gone`、外部WebRTC/fallback受入未証明、production性能計測未完了を明記し、virtual I/OやREST read-backを実機性能・browser映像の代替証拠にしない。
- [ ] 完了条件は既存CLI/起動gateの実行結果、`--help`、API/OpenAPI差分なし、既知の未受入項目一覧である。既知のbackend受入不足はGPUI差分のregressionと混同せず、GPUI実装中にbackend性能改善を同時着手しない。

### Phase 1 — GPUI Kitの最小platform PoC

- [ ] `gpui-kit`の採用release、GPUI snapshotの完全一致、Apache-2.0および同梱・推移依存license、必要なNix native librariesを確定する。`0.x`のAPI変更は想定し、更新は対応する全snapshot一式で行う。
- [ ] Poke-Conの独立したnative viewを一つ作る。fake dataで設定入力、CJK文字、clipboard、日本語IME、keyboard focus/navigation、アクセシビリティtree、theme/font fallbackを確認する。
- [ ] GPUI native event loopと既存`#[tokio::main]`/backend supervisorの共存を、hardwareなしで実証する。GPUIのmain-thread占有を考慮し、backendをGPUI event loopへ移動・重複起動せず、Tauriの`block_in_place`構造をそのままコピーしない。
- [ ] 同じRust package内のviewを`wasm32-unknown-unknown`でもcompile/displayできるか任意の初期probeとして調べる。WASM表示不可やbrowser input制約はnative UI PoCを進めるblockerとはせず、production browser対応を主張しない。
- [ ] WASMギャラリーの成功をアプリ全体の成功証明にしない。single canvas/window制約、threading/COOP-COEP要件、WebGPU/WebGL2、browser fonts、入力、screen readerの限界を記録する。
- **Gate 1:** native window起動、event loop/backend監督の共存、IME/CJK/clipboard/accessibilityの基本確認が成立し、未解決点と回避案が文書化されること。WASMの成否は別記し、native PoCの合否へ混ぜない。

#### PoC依存候補の一次資料調査（未採用）

一次資料で確認した候補releaseは[`gpui-kit v0.6.6`](https://github.com/longbridge/gpui-kit/releases/tag/v0.6.6)（tag commit `9765ae2c9a5eccfa13891248a445991e6f6a09d8`）です。
同tagの[workspace manifest](https://raw.githubusercontent.com/longbridge/gpui-kit/refs/tags/v0.6.6/Cargo.toml)はGPUI関連crateを`=0.3.6`、`gpui-pre-reqwest`を`=0.12.15`へ完全固定します。
[kit manifest](https://raw.githubusercontent.com/longbridge/gpui-kit/refs/tags/v0.6.6/crates/kit/Cargo.toml)は`gpui-kit`をApache-2.0と宣言しています。
[同tagのNix定義](https://raw.githubusercontent.com/longbridge/gpui-kit/refs/tags/v0.6.6/flake.nix)にはWayland、Vulkan、XCB、GTK3等のLinux依存があります。

この候補調査はPhase 1の依存採用・Gate 1通過を意味しません。
Poke-ConにはGPUI依存がなく、候補tagのworkspace lockfile（1,248 package）とcrates.ioの1,186件のlicense metadata、icon license、上流課題を監査しました。既定featureの実際の依存閉包、MPL該当package、任意のGTK3／WebKitGTK配布条件、ライセンス原文の法的確認、Poke-Con上のnative buildとIME/CJK/clipboard/accessibility／Tokio共存は未確定です。
これらの確認とfake-data native viewの実証が済むまで、Cargo依存、lockfile、Nix native libraryを追加せず、selector実装をPhase 1の代用にしません。

### 候補依存のライセンスと上流課題

`gpui-kit v0.6.6`のtag commitは`9765ae2c9a5eccfa13891248a445991e6f6a09d8`です。[kit manifest](https://raw.githubusercontent.com/longbridge/gpui-kit/refs/tags/v0.6.6/crates/kit/Cargo.toml)はdefault featureを`component`＋`assets`とし、同tagの`gpui-base`、`gpui-component`、`gpui-kit-assets` manifestはApache-2.0を宣言します。[workspace manifest](https://raw.githubusercontent.com/longbridge/gpui-kit/refs/tags/v0.6.6/Cargo.toml)はGPUI crateを`=0.3.6`、`gpui-pre-reqwest`を`=0.12.15`へ固定します。`shell`、`webview`、`tree-sitter-*`はopt-inです。

このtagの[リポジトリトップレベル](https://github.com/longbridge/gpui-kit/tree/v0.6.6)に`NOTICE`はなく、[`LICENSE-APACHE`](https://raw.githubusercontent.com/longbridge/gpui-kit/refs/tags/v0.6.6/LICENSE-APACHE)が存在します。crates.io version metadataは、[`gpui-pre 0.3.6`](https://crates.io/crates/gpui-pre/0.3.6)、[`gpui-pre-platform 0.3.6`](https://crates.io/crates/gpui-pre-platform/0.3.6)、[`gpui-pre-linux 0.3.6`](https://crates.io/crates/gpui-pre-linux/0.3.6)、[`gpui-pre-wgpu 0.3.6`](https://crates.io/crates/gpui-pre-wgpu/0.3.6)、[`gpui-pre-macros 0.3.6`](https://crates.io/crates/gpui-pre-macros/0.3.6)、[`gpui-pre-web 0.3.6`](https://crates.io/crates/gpui-pre-web/0.3.6)、[`gpui-pre-reqwest-client 0.3.6`](https://crates.io/crates/gpui-pre-reqwest-client/0.3.6)、[`gpui-pre-sum-tree 0.3.6`](https://crates.io/crates/gpui-pre-sum-tree/0.3.6)をApache-2.0、[`gpui-pre-reqwest 0.12.15`](https://crates.io/crates/gpui-pre-reqwest/0.12.15)をMIT OR Apache-2.0と宣言しています。

上流[`Cargo.lock`](https://raw.githubusercontent.com/longbridge/gpui-kit/refs/tags/v0.6.6/Cargo.lock)は1,248 package（crates.io 1,186、Git 24、workspace path 38）を解決し、crates.io metadataのlicense fieldは全1,186件で解決、未分類は0件でした。ただしlockfileはworkspace／target／optional featureの和集合であり、既定Linux feature closureを確定しません。

- **MPL-2.0**: metadata上7 package。`option-ext 0.2.0`([crates.io metadata](https://crates.io/crates/option-ext/0.2.0))は`dirs-sys`→`dirs`→`zed-font-kit`→`gpui-pre`経由で既定Linuxの依存閉包に入る可能性があり、feature解決後の閉包を再検証します。`cbindgen 0.28.0`はmacOS向けbuild依存、`cssparser 0.29.6`／`cssparser-macros 0.6.1`／`dtoa-short 0.3.5`／`selectors 0.24.0`は`lb-wry`のAndroid target経由、`dwrote 0.11.5`はWindows targetです。MPLのソース提供条件を含め、target別の頒布可否は未承認です。
- **複数license／metadata要確認**: [`self_cell 1.3.0`](https://crates.io/crates/self_cell/1.3.0)はApache-2.0 OR GPL-2.0-only（Apache選択可能）、`r-efi 5.3.0`／`6.0.0`はMIT OR Apache-2.0 OR LGPL-2.1-or-later（permissive選択可能）です。[`tree-sitter-graphql 0.1.0`](https://crates.io/crates/tree-sitter-graphql/0.1.0)はcrates.io metadataが`non-standard`ですが、上流LICENSEはMITと報告されています。任意feature依存ですが、公開crate metadataとの不一致は法務確認対象です。
- **Git／workspace path依存とnative配布**: lockfile上の`llrt`はApache-2.0、`quickjs-jit`はMIT、公開対象path crateはApache-2.0宣言ですが、同梱C engineのlicense原文は全文照合していません。`gpui-pre-linux 0.3.6`の既定runtime依存にGTKはなく、[`lb-wry 0.53.3` manifest／feature metadata](https://docs.rs/crate/lb-wry/0.53.3/source/Cargo.toml)の`os-webview` featureを選ぶ場合にGTK／WebKitGTK／JavaScriptCore／Soupを含むnative依存が入ります。上流Nix開発環境のnative library一覧と実際の頒布runtime閉包は分けて確認します。

[`crates/assets/LICENSE-LUCIDE`](https://raw.githubusercontent.com/longbridge/gpui-kit/refs/tags/v0.6.6/crates/assets/LICENSE-LUCIDE)にはLucide由来iconのISC許諾文と、列挙されたFeather由来iconに適用されるMIT許諾文が併記されています。該当assetを再配布する場合は両方の著作権表示と許諾文を保持します。以上はregistry metadataと主要なlicense原文に基づく技術監査であり、法的助言・頒布承認ではありません。`cargo metadata`で選定feature／target closureを固定し、Git/C engine原文も含む`cargo-about`等のsource-based license reportと配布noticeを生成・reviewするまで、license／notice gateは未完了です。

上流の版固定には実例があります。[#3156](https://github.com/longbridge/gpui-kit/issues/3156)では、`gpui-pre`の`register_inspector_element`変更によりcaret要件が破損し、[#3163](https://github.com/longbridge/gpui-kit/pull/3163)で完全固定へ切り替えました。v0.6.6のCIにもexact pin検査があります。PoCの依存更新ではGPUI関連snapshotとlockfileをまとめて更新し、同じNix gateを実行します。

native viewの受入では、次の既知課題を個別に再現確認します。

- 日本語IME: [Zed PR #60589](https://github.com/zed-industries/zed/pull/60589)はLinux X11／Fcitxで長時間稼働後に入力不能となる問題を報告し、[issue #64389](https://github.com/zed-industries/zed/issues/64389)はIME composition selectionのoffset計算問題を報告しています。上流修正の存在だけでは日本語IMEの受入根拠にならないため、長時間入力とcompositionを試験します。
- accessibilityとfocus: [gpui-kit #3182](https://github.com/longbridge/gpui-kit/issues/3182)はv0.6.6のfocusable listにAccessKit roleがなくtree nodeが出ない事例を記録し、[#2968](https://github.com/longbridge/gpui-kit/issues/2968)はsidebar項目のrole欠落を報告しています。[#3183](https://github.com/longbridge/gpui-kit/issues/3183)はv0.6.0時点の入力focus報告で、投稿者自身がv0.6.6では変更済みの可能性を注記しています。いずれもPoke-Conでの不具合を証明するものではなく、fake-data viewでTab遷移とaccessibility treeを検査する再現候補です。
- clipboard: [Zed PR #61338](https://github.com/zed-industries/zed/pull/61338)はWayland selectionの寿命と`text/plain` MIMEの相互運用を扱います。[PR #54857](https://github.com/zed-industries/zed/pull/54857)はGPUI Webのclipboard event bridgeを実装する提案です。native Wayland clipboardとWASM/browser clipboardは別gateとして扱います。
- 起動と配布: 同tagの[Nix定義](https://raw.githubusercontent.com/longbridge/gpui-kit/refs/tags/v0.6.6/flake.nix)は開発環境にWayland、Vulkan、XCB、XKB、GTK3等を列挙しますが、これは既定Linux runtimeのリンク閉包を意味しません。既定Linux backendと任意`lb-wry`のnative依存を区別し、PoCではX11／Waylandの起動・focus、Nix開発環境、実際の配布依存を別々に検査します。
- event loop: upstreamの[`Application::run`](https://github.com/zed-industries/zed/blob/bcf6582/crates/gpui/src/app.rs)は通常platform上でevent loopを占有し、[`run_embedded`](https://github.com/zed-industries/zed/blob/bcf6582/crates/gpui/src/app.rs)は外部event loopから制御するための別経路を提供します。いずれも既存Tokio backend supervisorとの共存を証明しません。Gate 1で起動、制御、停止の共存をPoke-Con上で実証します。

以上の課題は候補を排除する結論ではなく、Phase 1／Gate 1とGate 6へ引き継ぐ試験対象です。native build、Tokio共存、日本語IME、clipboard、accessibility、推移依存監査の結果が揃うまではPoCを完了扱いしません。

### Phase 2 — 並行起動選択とfrontend/backend接続

- [ ] `apps.default`と`apps.tauri`を変更せず、同じ`packages.pokecon/bin/pokecon`へ`--ui gpui`を渡す薄い`apps.gpui`を追加する。別Cargo package、別backend binary、別workerは作らない。
- [ ] Rust CLIに`UiArgument::Gpui`/`UiMode::Gpui`を追加し、`UiMode::capabilities()`でTauri Origin許可・Tauri screenshot pathをGPUIへ誤適用しない。既存`web`/`desktop`の意味とdefault、`README`の起動形は維持し、CLI help fixtureとflake canonical hashを更新する。GPUI Kit依存を加える場合はCargo manifest/lock identity assertionとNix native runtime dependenciesも同時に更新する。
- [ ] GPUI shellは既存の`run_packaged_backend`/`run_configured_controlled`を一度だけ起動し、backend readiness、shutdown supervisor、同一`ProductionRuntime`を共有する。GPUIのevent loopとTokio runtimeのthread/lifecycle順序をfake backendおよびsoftware-only runtimeで検証する。
- [ ] typed in-process UI adapterと既存Axum API clientでstate snapshot、設定mutation一つ、revision付き通知、commit/read-backのvertical sliceを比較する。latency、failure handling、Origin/path制約、型安全性、運用の実測で採否を決める。`ApplicationBackend`やcamera/serial/worker serviceをviewから直接参照させない。
- [ ] server start/stop、shutdown coordinator、設定、profile、worker起動に二重所有がないことを検証する。UI起動失敗時にもbackend/workerを残すか閉じるかを既存lifecycle契約に沿って決定し、全終了経路をtestする。
- **Gate 2:** `nix run . -- --ui web`、`nix run .#tauri`、`nix run .#gpui`が同じ製品binaryを使い、一度だけbackend/workerを起動すること。GPUI固有capability、所有権、失敗/read-back契約が検証されること。browser向け既存API contractに変更が要る場合はOpenAPI同期、互換性検査、Svelte client testを追加する。

### Phase 3 — 制御と設定の縦切り

- [ ] status/state snapshot、一設定更新、profile切替、controller入力の順にGPUIへ移す。各操作はUI入力→選定したquery/command境界→backend commit→event/snapshotのread-back→UI表示まで確認する。
- [ ] stale revision、server拒否、切断、再接続、UI window close、操作中shutdownで誤った成功表示や二重実行がないことをno-network fake/integration testで確認する。
- [ ] 代表的な画面で機能・操作・状態・安全停止・回復要件を満たすか比較する。GPUIはnative widget/layoutを使用でき、Tkinterのpixel単位の視覚parityは要求しない。見た目の調整は操作の意味と状態遷移が一致した後に行う。
- **Gate 3:** 少なくとも一つの実ユーザーフローが選定した明示的なUI/backend境界で完結し、失敗・再接続・read-backの契約が検証されること。内部resource serviceへの直接callやbackend/UI間の重複した正準stateを必須にする構造なら境界案を見直す。

### Phase 4 — Camera/mediaと性能の最大リスクを先に検証

- [ ] 最初に現行WebSocketのbinary JPEG fallbackを受信してGPUI viewに継続描画する最小PoCを作る。WebRTC/H264、overlay、入力は混ぜず、既存backend/media contract変更なしでframe lifetimeとtexture uploadを確認する。
- [ ] 次に現行WebRTC primaryをnativeで受信し、H264 decode/renderとsignalingを検証する。GPUI canvasが`MediaStream`/`<video>`等のDOM mediaを自動的に扱えると仮定せず、native client/transport、frame format/conversion、texture upload、frame lifetimeの具体経路を選ぶ。
- [ ] WebRTC primary中のfallback frameを誤ってprimary stateから降格させないこと、3秒 inactivity時のfallback、retry後のWebRTC再昇格を既存`MediaView` semanticsと合わせて試験する。Backend camera ownerは維持し、camera/serial handleをGPUIへ渡さない。
- [ ] カラーピッカー、crop/screenshot、touchscreen area選択など映像上の入力を代表例として検証する。WebRTC不可時にも現在のfallback契約が失われないことを確認する。
- [ ] mock/virtual cameraとloopbackでlatency・frame drops・throughput・CPU/memory・shutdown cleanupを測定する。frontend／integration定義書にある該当経路の数値目標を採用判定に用い、実測値がないのに達成を主張しない。
- **Gate 4:** JPEG fallbackとWebRTC primaryの両方、切替/retry、主要camera操作、対象latency、継続描画、終了時解放をvirtual/loopback条件で証明する。WebRTC表示の方法がない、または性能/入力要件を満たさない場合は全画面移植を停止し、native video host等の代案を比較する。実機性能達成とは主張しない。

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

### Phase 7 — GPUI WebAssemblyのbrowser受入 (native PoCとは別判断)

- [ ] GPUIの同一UIをbrowserでも提供する要件が確定した場合に進める。Phase 1の同一view compileだけでなく、必要な主要画面とAPI clientをWASM targetでbuildし、production相当の静的hostへdeployする。
- [ ] Browser Fetch、WebSocket、WebRTC受信とvideo/frame presentationを実動作で確認する。Safari 16.4+を含む既存browser baseline、CJK/IME、clipboard、keyboard、accessibility、screen reader、focus/Tab動作をbrowser matrixで受け入れる。
- [ ] マルチthreadingを選ぶ場合だけ、SharedArrayBufferとCOOP/COEPを含む本番hosting headerを設計・testする。シングルthread設計で十分なら、不要なcross-origin isolationを追加しない。
- [ ] backend serverから配信するSPA、別静的host、GPUI canvasを埋め込む構成のいずれかを、Origin/security contract・運用・cache・asset/font配信と合わせて決める。未決のまま本番対応を宣言しない。
- **Gate 7:** GPUIをbrowser UIとして採用する場合だけ、SPECの対象browserとcamera/control要件を満たす本番受入証跡を要求する。満たせない場合はGPUI native採用と切り分け、Svelte Webを維持する。未達の共通UI要件を仕様変更なしに達成したとは扱わない。

### Phase 8 — 採否・段階的切替

- [ ] Gate 1–6の証跡からGPUI nativeの採否を決める。GPUI nativeのみ採用、native+WASM採用、PoC中断のいずれも結果として許容する。WASM/ browser対応は明確に要求された場合だけGate 7を必須化する。
- [ ] GPUI nativeを既定にする前に、現行UIに対するnative主要要件parity、performance、crash/restart、package、CI、accessibilityを完了する。browser全体の受入は既存Svelte Web baselineとGPUI browserを区別して扱う。
- [ ] 旧Svelte/Tauri起動selectorとrollback用Nix appは、利用者が移行完了を承認するまで削除しない。
- [ ] 製品契約変更が必要な場合は、対象となるbackend／frontend／integration定義書、補助文書、受入gateを同じ承認済み作業で同期する。本計画だけの更新を仕様変更の代替にしない。

## 6. PoC合否条件と停止条件

**継続条件**

- 同じRust backendと同じ実行ファイルで、現行UIとGPUIを選んで起動・停止できる。backend本体の構造移行は既存完了証跡を前提とし、未証明のhardware/browser acceptanceは別途明記する。
- UIはbackend内部のcamera/serial/worker/controller所有serviceを直接操作せず、typed UI boundaryのcommand/query結果とstate/event read-backを表示する。
- 主要control flowとmedia flowが現行の挙動・低遅延目標を満たし、worker/API/setting contractを不必要に変えない。
- 対象OS、IME、accessibility、native lifecycle、packageの必須要件を実証する。GPUI browserを採用範囲に含める場合だけbrowser acceptanceもGate 7で実証し、それ以外はSvelte Web維持を明記する。
- `nix run`の各選択肢が同じpackage/binaryを使い、回帰gateが全て再現可能である。

**停止または再設計条件**

- camera/WebRTCの描画経路がない、または目標latency/fallbackを達成できない。
- GPUI browserが共有UIの必須要件として選ばれたのに、対象browser・IME・accessibility要件を満たせない (native PoC自体を自動的に棄却する条件ではない)。
- 安全なbackend境界を維持するにはAPI契約の大規模変更やUI専用hardware ownershipが必要となる。
- Tauriの必須lifecycle/配布契約が代替できず、実装負担がPoC便益を上回る。

停止時は現行UIを残し、どのgateで不成立となったか、GPUI native-only等に縮退するかを意思決定記録へ残す。PoCを通らなかったことを理由に、現行仕様や既存APIを暗黙に縮小しない。

## 7. 参照先

### リポジトリ内

- `SPECIFICATION.md` — 定義書群の入口と分類・担当表。
- `docs/SPECIFICATION_BACKEND.md` — script/runtime/settings/deviceの必須契約。
- `docs/SPECIFICATION_FRONTEND.md` — UI機能、keyboard/IME/accessibility、browser要件と応答性。
- `docs/SPECIFICATION_INTEGRATION.md` — API/transport、camera/save、serial設定操作、frontend境界とlifecycle。
- `PLAN.md` — Rust構造移行Phase 2.1–2.7の完了記録、未証明のproduction latency/throughputとexternal browser acceptance、同一binary Web/Desktopの既存受入。
- `docs/ARCHITECTURE.md` — backend ownership、process topology、frontend責務、state/API境界。
- `docs/HTTP_API.md`、`api/openapi.json`、`rust/pokecon/src/server/api.rs` — 公開HTTP/API contract。
- `rust/pokecon/src/entrypoint.rs` — 現行`--ui web|desktop` selector、Tauri shellから`run_packaged_backend`を監督する起動lifecycle。
- `rust/pokecon/src/lib.rs` — `UiMode` capability分岐と単一の`run_configured_controlled` backend lifecycle。
- `rust/pokecon/src/desktop/mod.rs`、`rust/pokecon/src/runtime/` — Tauri shellおよび共通shutdown境界。
- `rust/pokecon/src/tests/ui_boundary_acceptance.rs`、`rust/pokecon/tests/startup.rs` — UI capability、同一SPA/API、startup modeの受入。
- `web/src/lib/`、`web/src/routes/` — 現行Svelte state/API/media/UI。
- `flake.nix` — `apps.default`、既存`apps.tauri`、`packages.pokecon`、`packages.web`、canonical flake hashとNix task/runtime/package構成。
- `tests/fixtures/cli-help/pokecon.txt` — CLIの`--ui`値追加時に同期するhelp snapshot。

### GPUI / GPUI Kit 一次情報

- GPUI Kit: <https://github.com/longbridge/gpui-kit>
- GPUI Kit Web gallery: <https://github.com/longbridge/gpui-kit/tree/main/crates/story-web>
- GPUI Kit Web build workflow: <https://github.com/longbridge/gpui-kit/blob/main/.github/workflows/release-website.yml>
- GPUI Kit regular CI: <https://github.com/longbridge/gpui-kit/blob/main/.github/workflows/ci.yml>
- Zed GPUI Web backend constraints: <https://github.com/zed-industries/zed/tree/main/crates/gpui_web>

## 8. 進捗

- [x] 作業対象を既存`refactor/rust-core` branch/worktreeに固定し、現行CLI、backend ownership、OpenAPI境界、Nix default launcher、仕様との既知のずれを計画の前提として記録する。
- [ ] Phase 0 — 現行要件と実装baselineを再照合する。
- [ ] Phase 1 — GPUI Kit native view、Tokio/event-loop共存PoC (WASM probeは別記)。
- [ ] Phase 2 — 同一binaryの`apps.gpui` selectorとbackend接続。
- [ ] Phase 3 — control/settings vertical slice。
- [ ] Phase 4 — media/performance gate。
- [ ] Phase 5 — 残り画面。
- [ ] Phase 6 — native lifecycle/package。
- [ ] Phase 7 — GPUI browser/WASM受入 (共有UI要件が確定した場合)。
- [ ] Phase 8 — 採否・段階的切替。
