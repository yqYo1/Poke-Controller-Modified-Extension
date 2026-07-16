# Poke-Controller Modified Extension 実装作業計画

- **規範**: [`SPECIFICATION.md`](SPECIFICATION.md) 2.2.0
- **基準コミット**: `658519757909f91531cc2e8ab61e6f4a80af9612`
- **対象ブランチ**: `refactor/rust-core`

## 1. 目的

本計画は、実装前の定義書を唯一の設計根拠として、Poke-Controller Modified ExtensionをRustコア、Python／Luaワーカー、axum、SvelteKit、Tauriで段階的に再実装するための作業順序を定める。

各フェーズは、後続作業が依存できる実行可能な成果物、検証手段、完了条件までを含む。フェーズ名だけの進捗や、未実行のスタブを完了として扱わない。

## 2. 完了の定義

再実装全体は、次の条件をすべて満たした時点で完了とする。

1. 定義書の対象機能がWindowsとLinuxで利用できる。
2. Rustメイン、動的設定ワーカー、ユーザースクリプトワーカー、axum、Tauri、Web UIの責務境界が定義書どおりである。
3. 正準設定レジストリからTOML、CLI、環境変数、動的設定、UI、OpenAPIが生成または検証され、表面間の差異がない。
4. 固定3ベースラインの互換コーパスがすべて成功し、追補候補の昇格処理が自動化されている。
5. カメラ、共有メモリ、シリアル、IPC、profile切替、shutdownの正常系と障害系が自動試験で検証されている。
6. OpenAPI、TypeScript型、Python型、Lua型が同期し、型生成差分がCIで検出される。
7. Nix appを介した整形、静的解析、単体試験、統合試験、ビルド、パッケージ検査が成功する。
8. 実機を必要としない試験はCIで再現でき、実機試験には手順と記録形式がある。
9. 署名付きコミット、必須レビュー、全CI成功、バージョン更新を経てリリース可能である。
10. 実装完了後にREADMEと利用者向け文書を現行実装へ合わせて再作成する。

## 3. 開始時点

現在の追跡対象には、定義書、Nix／CI設定、Rust workspaceのルートmanifest、Python project設定、Tauriアイコンがある。Rustクレートの`src/`、Python互換層、Web UI、サーバー実装、テスト実装は存在しない。

したがって、次を前提とする。

- 旧実装の復元や継ぎ足しではなく、定義書からの再構築として進める。
- `Cargo.toml`、`pyproject.toml`、`flake.nix`、release workflowの既存記述は将来構成の参考であり、定義書より優先しない。
- 実装ソースが復活した時点で、現在skipされているCIを自動的に厳格実行へ戻す。
- `docs/`は実装途中の設計置き場として復元しない。実装計画は本ファイル、規範は`SPECIFICATION.md`へ集約する。
- PWA、macOS、キーコンフィグ再設計、LINE UI、Pokémon HOMEは本計画の対象外とする。

## 4. 実施規則

1. 外部挙動や公開APIを変更する必要が生じた場合は、実装より先に`SPECIFICATION.md`を更新する。
2. すべての開発・整形・検査・ビルド・実行はNix flake appまたは`nix develop --command`を介して行う。
3. 各機能はghq配下の専用worktreeとfeature branchで実装し、ルートcheckoutや既定ブランチへ直接コミットしない。
4. 一つのPRには一つの検証可能な契約または密接な変更群だけを含める。後続フェーズを動かすための巨大な未検証PRを作らない。
5. 各PRは定義書の該当節、追加した試験、完了条件を本文に記載する。
6. commitはSSH署名付きとし、push後は人間・botレビューと全CIを確認する。
7. `--admin`、raw merge API、ruleset変更、CI回避でmergeしない。
8. 有効なレビュー指摘を反映し、必要なバージョン更新を行ってからmergeする。
9. 実装フェーズの完了判定は、コードの存在ではなく、そのフェーズの受入試験の成功で行う。

## 5. 依存関係

```text
フェーズ1  契約と検証基盤
   └─フェーズ2  Rust／Nix／プロセス土台
       ├─フェーズ3  XDG・設定・永続化
       │   ├─フェーズ4  IPC・ワーカー監督
       │   │   ├─フェーズ5  コントローラー・シリアル・通知
       │   │   ├─フェーズ6  カメラ・共有メモリ
       │   │   └─フェーズ7  動的設定
       │   │       └─フェーズ8  ユーザースクリプト互換層
       │   │           └─フェーズ9  profile・コマンド・callback統合
       │   └──────────────────────────────┐
       └──────────────────────────────────┤
                                          └─フェーズ10 REST・WebSocket・WebRTC
                                               └─フェーズ11 Web UI
                                                    └─フェーズ12 Tauri・ライフサイクル
                                                         └─フェーズ13 配布環境・パッケージ
                                                              └─フェーズ14 互換コーパス確定
                                                                   └─フェーズ15 統合・リリース
```

次の作業は早期から並行できる。

- 固定ベースラインの互換コーパス収集はフェーズ1から開始し、フェーズ14で合否判定へ接続する。
- Windows／Linuxのデバイスadapterは、共通trait確定後に別workstreamで並行実装できる。
- Web UIはフェーズ10の生成OpenAPI型とmock serverが確定した画面から順に並行実装できる。
- fault injection、仮想カメラ、仮想シリアルfixtureは対象サブシステムと同じフェーズで追加する。
- 利用者向け文書は実装完了後に作成するが、API docstringと生成型の説明は各フェーズで同時に更新する。

## 6. フェーズ1 — 契約と検証基盤

**参照**: §0、§1、§2、§3、§4.6、§8、§10、§11、§14

### 6.1 作業

1. 定義書の公開契約を、設定CID、REST path、WebSocket variant、IPC kind、イベント名、公開Python／Lua名、対応プラットフォームへ分類する。
2. 定義書の正準設定レジストリ78件に対応する実装上の機械可読な単一ソースを作り、定義書との同期と各表面への投影差分をCIで検査する方針を確定する。
3. 固定3ベースラインをimmutableなcommit SHAで取得し、スクリプト、import、公開symbol、期待結果をmanifest化する。
4. 互換コーパスの固定領域と追補候補領域を分離し、破壊的候補を昇格させない判定形式を作る。
5. OpenAPI、Python stub、Lua annotation、TypeScript生成物の配置と再生成コマンドを決める。
6. 単体、契約、統合、障害注入、実機、互換の試験カテゴリとfixture命名規則を定める。
7. 現在のmanifest、flake app、workflowが期待する将来パスを棚卸しし、定義書と矛盾する古いcrate名やrelease手順を修正対象として記録する。

### 6.2 成果物

- 契約検査用のテスト骨格
- 正準設定レジストリの機械可読schema
- 固定ベースラインmanifestと互換コーパス配置
- 生成物の出力先とdrift検査
- 各CI jobの適用条件一覧

### 6.3 完了条件

- 同じ正準設定CID、イベント名、REST pathを複数の手書き正本から読み取る必要がない。
- 固定3 SHAを再取得して同じコーパスを構築できる。
- 未実装の検査は「成功」ではなく、理由付きのnot applicableとして区別される。
- `nix fmt`、typos、Nix metadata、schema検査が実装ソースなしでも成功する。

## 7. フェーズ2 — Rust／Nix／プロセス土台

**依存**: フェーズ1

**参照**: §1.2、§1.3、§3、§7.1、§14、§15.1

### 7.1 作業

1. 定義書上の責務に合わせてRust workspaceを再構成する。内部crate名は実装都合で決め、公開API名として扱わない。
2. Rustメイン、axum server、Tauri shell、PyO3境界、worker executableの最小起動経路を作る。
3. tokio runtime、構造化logging、診断ID、cancel token、graceful shutdown coordinatorを共通化する。
4. プラットフォーム差をtrait境界へ隔離し、LinuxとWindows adapterの雛形を用意する。
5. `nix run .`、`build-rust`、`cargo-test`、`clippy`、`test`、`basedpyright`、`web-check`が対象ソースの出現に応じて有効化されるようflakeを整える。
6. workspace、Python package、Web package、Tauri serverのversion源を同期できる形にする。
7. CIのsource-existence guardについて、対象あり／なしの両分岐をfixtureで検証する。

### 7.2 成果物

- build可能なRust workspace
- 起動後に正常終了できるRustメインとworker executable
- Nixから実行できる全task app
- Linux／Windows共通traitとplatform module
- tracingと診断の共通基盤

### 7.3 完了条件

- Rustメインを起動し、子workerなしの状態で正常終了できる。
- SIGINT、SIGTERM、Tauri終了要求が同じshutdown coordinatorへ入る。
- `nix run .#clippy`、`nix run .#cargo-test`、`nix run .#build-rust`が成功する。
- LinuxとWindowsのCIでworkspaceの最低限ビルドが成功する。

## 8. フェーズ3 — XDG・設定・永続化

**依存**: フェーズ2

**参照**: §6.7、§7.4、§11.3、§11.4、§12、§13、§14.1、§14.2、§14.5、§15.8〜§15.11

### 8.1 作業

1. `app_name`で分離されたConfig、Data、State、Cacheの4ルートをLinux／Windowsで解決する。
2. bootstrap preparseを実装し、worker構築前に`app_name`、`dynamic_config_language`、`python.dynamic.*`を解決する。
3. 正準設定レジストリからdefault、型、scope、mutability、secret、CLI、環境変数、TOML、動的path、UI、OpenAPI metadataを投影する。
4. TOMLの未知キー・コメント・並び順を保持する原子的編集、権限設定、親directory同期を実装する。
5. settings lock、venv lock、HMAC鍵の排他生成、manifest署名、secret maskingを実装する。
6. profile-capableとglobal専用の保存先を分離し、bootstrap／startup-only／runtime設定の適用時点を実装する。
7. 設定PATCHのクラスA〜D、`expected_revision`、rollback、`pending_restart_values`、`apply_failures`をservice層で実装する。
8. CLI、環境変数、TOMLの優先順位と、相対path基準、enum正規化、bool／JSON直列化を実装する。
9. `server.port`、`server.bind_address`、`server.web_dir`等のstartup-only値を保存値と現在値に分ける。
10. 相対pathはCLIだけをcurrent working directory基準、TOML／環境変数／動的設定を実効Config基準とする。閉じたenumはruntimeで大小文字不問に正規化し、生成型は正準値だけを列挙する。
11. profileは`--profile`／`-p`、`POKECON_PROFILE`、TOMLの全経路で解決し、名前の大小文字同一性をOS／filesystemの規則に従わせる。
12. managed uvをDataルートへ準備し、正準pyprojectのdependencies／dependency groups／extras、application constraint、package metadata constraint、`uv_config`を統合したexact syncとmutable source再検証を実装する。
13. dynamic worker用venvとprofile別user worker用venvを分離し、manifest、HMAC、single-flight、cross-process lock、破損時再構築を実装する。
14. generated Python／Lua typingsをDataへ、user-editable pyproject／init／profile設定をConfigへ置くpath契約を実装する。

### 8.2 成果物

- XDG path resolver
- canonical settings service
- TOML editorとlock manager
- CLI／環境変数parser
- secret maskingとHMAC鍵管理
- managed uv／venv manager
- settings contract tests

### 8.3 完了条件

- 正準CID78件と環境変数78件に重複・欠落がない。
- 各設定の全表面がregistry metadataと一致する。
- 複数processから同じTOMLとHMAC鍵へ競合しても破損しない。
- profile切替中のprofile-capable書込みが`409`となり、誤ったprofileへ保存されない。
- startup-only値は保存されるが現processへ適用されず、restart情報へ現れる。
- secret平文がログ、REST応答、WebSocket、manifestへ出ない。
- 同じvenv pathへの同時準備が1処理へ集約され、別processとの競合後も同じmanifestへ収束する。

## 9. フェーズ4 — IPC・ワーカー監督

**依存**: フェーズ2、フェーズ3

**参照**: §1.2、§7.8、§11.5.6.4、§14.5、§15.6

### 9.1 作業

1. stdin／stdout上のlength-prefix付きMsgPack framingと1 MiB上限を実装する。
2. `request`、`response`、`error`、`event`、`log` kindと閉じたpayload検証を実装する。
3. bounded writer queue、pending waiter table、reader／writer独立task、exactly-once切断処理を実装する。
4. per-worker generation、cancel token、stopping世代拒否、late response破棄を実装する。
5. worker起動、協調停止、期限、強制終了、OS process回収を共通supervisorへまとめる。
6. stdoutのprotocol化とstderrのout-of-band診断を実装する。
7. worker crash、partial frame、oversize、EOF、writer panic、reader panic、queue overflowのfault injectionを作る。
8. Rust側resource ownershipを定義し、Python／Luaのfinallyに安全解放を依存させない。

### 9.2 成果物

- IPC codecとschema
- worker supervisor
- generation／cancellation manager
- fault injection fixture
- worker lifecycle integration tests

### 9.3 完了条件

- readerとwriterが同時に終了しても全waiterが一度だけ完了する。
- 破損frameとoversize frameを副作用なしで拒否できる。
- worker crash後にボタン、stick、touchが必ず解放される。
- 通常運用中に動的設定workerをkill／再生成しない。
- shutdown時だけ定義済みの強制終了例外が機能する。

## 10. フェーズ5 — コントローラー・シリアル・通知

**依存**: フェーズ3、フェーズ4

**参照**: §5.3、§6.2、§6.3、§6.5、§7.5〜§7.7、§10.5、§11.4.1.6

### 10.1 作業

1. ボタン、hat、左右stick、touchの正準状態modelとneutral stateを実装する。
2. 入力sourceごとのgeneration、sequence、snapshot、重複排除、強制解放を実装する。
3. keyboard、mouse、browser gamepad、ユーザースクリプト入力を同じarbiterへ接続する。
4. Switch／3DS controller data formatとserial frame直列化を実装する。
5. Linuxのudev selectorとWindowsのCOM selectorを生値のまま保持する。
6. port、baud rate、data formatの即時切替transactionとrollbackを実装する。
7. reconnect、20回上限、3秒間隔、明示切断による再試行取消しを実装する。
8. controller送信、serial送受信、切断、partial write、再接続の仮想serial試験を作る。
9. Discord webhookとWindows native通知のadapter、開始／終了hook、通知test操作、失敗時の非致命診断を実装する。
10. Pro Controller／XInput等のhardware controller sourceと記録状態を正準入力modelへ接続する。キーコンフィグ再設計は含めない。

### 10.2 成果物

- controller state machine
- input arbiter
- serial adapterとcodec
- notification serviceとplatform adapter
- virtual serial fixture
- controller／serial contract tests

### 10.3 完了条件

- 異なる入力sourceの競合時も定義済みpriorityとrelease規則を守る。
- 経路切替後に旧generationの入力が再適用されない。
- serial設定変更が成功時だけ確定し、失敗時は旧接続へ戻る。
- worker／WebSocket切断時にneutral stateを実機またはloopback fixtureで確認できる。
- notification失敗がcommand実行やアプリケーションを停止せず、secretを診断へ出さない。

## 11. フェーズ6 — カメラ・共有メモリ・画像保存

**依存**: フェーズ3、フェーズ4

**参照**: §3.1、§6.1、§7.3、§7.9、§10.4.3

### 11.1 作業

1. Linux V4L2 selectorとWindows native selectorを保持するcamera adapterを実装する。
2. capture FPS、解像度、flip、screenshot formatのruntime適用を実装する。
3. 最大1920×1080 BGR uint8固定容量の3-slot共有メモリを実装する。
4. slot CAS、`reader_pin_count`、release／acquire、publication token、8回retry、zero frame規則を実装する。
5. 解像度変更時に未pin非current slotへ最初の完全frameを書き、出版後に旧slotを遅延更新する。
6. worker死亡時のpin回収とshutdown時のwriter未停止guardを実装する。
7. screenshotのcrop、png／jpeg、固定ベースライン名、衝突suffix、overwrite、download／captures／path variantを実装する。
8. Motion JPEG生成とWebRTCへ渡すframe sourceを分離する。
9. virtual V4L2、記録済みframe source、Windows mock adapterで試験する。
10. writer crash、reader crash、pin保持、解像度変更、rollback、shutdownのstress testを作る。

### 11.2 成果物

- camera adapter
- shared memory ring
- screenshot service
- media frame source
- virtual camera／recorded frame fixture

### 11.3 完了条件

- pin中slotのdataとmetadataを変更しないことを競合試験で証明する。
- 解像度変更中もreaderが破損frameを観測しない。
- camera設定失敗時に旧設定へrollbackし、暗黙に別deviceを選ばない。
- writer停止失敗時に共有メモリをunmapせず、process終了まで安全を維持する。
- screenshotのpath traversal、衝突、形式、上書き規則が全variantで一致する。

## 12. フェーズ7 — 動的設定エンジン

**依存**: フェーズ3、フェーズ4

**参照**: §1.2、§7.4、§11.5、§14.4、§14.5、§15.6

### 12.1 作業

1. グローバル動的設定worker内にCPython 3.14 main interpreterとLuaJIT runtimeを埋め込む。
2. `dynamic_config_language`による初期runtimeと、`pokecon.source()`による他言語runtimeの遅延初期化を実装する。
3. Python／Luaで同じ`pokecon.*`名前空間、設定path、state、event、autocmd、callbackを提供する。
4. top-level評価、`source()`、reloadを単一coordinatorで直列化する。
5. registration IDごとのlane、priority queue、bounded concurrency、queue eviction、`once()`を実装する。
6. soft／grace／hard timeoutと論理完了、実終了、lane占有を分離する。
7. `load_path`、`load_content`、`reload`、原子的保存、前世代fallbackを実装する。
8. 非loopback LANでも動的設定操作を制限せず、完全信頼境界としてHost／Origin等の既定検証だけを適用する。
9. `Commands.*`を動的設定workerのimport pathへ入れず、worker境界を保証する。
10. callbackが部分適用中の設定を観測しないdispatch barrierを実装する。
11. 組み込みイベントはPre／Post後置の正準名で発火し、event callbackへ引数やpayloadを渡さず`pokecon.state`から状態を読む。`on()`／`once()`はload順序のため未定義raw文字列を受理し、`BuiltinEvent`は補完用の正準値として提供する。

### 12.2 成果物

- dynamic worker executable
- Python／Lua `pokecon.*` bindings
- callback coordinator／executor
- source／reload transaction
- cross-language conformance tests

### 12.3 完了条件

- PythonとLuaで同じ操作が同じstate遷移・error・timeoutを生む。
- 同じ登録IDは直列、異なる登録IDは上限内で並行する。
- soft deadline後もcallback実終了まではlaneと実行枠を解放しない。
- hard timeoutでも通常運用中の動的設定worker processを終了・再生成せず、定義済みの論理失敗と診断だけを適用する。
- reload失敗時に前世代の有効設定を維持する。
- LANから`load_content`／`load_path`／`reload`を実行でき、UIが完全信頼の警告を表示する。
- Python／Luaの同一イベントが同じPre／Post順序で発火し、callback引数が常に空である。

## 13. フェーズ8 — ユーザースクリプト互換層

**依存**: フェーズ4、フェーズ5、フェーズ6

**参照**: §4.6、§10、§14.5

### 13.1 作業

1. profileごとのCPythonユーザースクリプトworkerとmanaged venvを実装する。
2. `Commands` package、`PythonCommand`、`ImageProcPythonCommand`、`Camera`、`CaptureArea`、`Keyboard`、dialog APIを公開する。
3. `Commands.dialogue`、`Commands.net`、`Commands.image_proc`を実行中command contextへ束縛する。
4. controller、serial、camera、image processing、notification、socket、MQTTをIPC proxyとして実装する。
5. 固定ベースラインのcamelCase名、引数、default、同期挙動、例外、`saveCapture`の層差を維持する。
6. blocking／non-blocking dialog、Widget.value、確認済みdefault、異常close時停止を実装する。
7. Python専用の`Commands.*`互換APIについてPython 3.14 stubを生成し、公開名とruntime実体の一致を検査する。動的設定用のPython／Lua型はフェーズ7の`pokecon.*`生成型として分離する。
8. MatLike copy ownership、crop、template matching、overlayのworker内処理を実装する。
9. worker timeout、停止、強制終了、profile単位の環境分離を実装する。

### 13.2 成果物

- Python package `Commands`
- user-script worker
- PyO3 proxy／binding
- dialog／image／network compatibility API
- `Commands.*`用`.pyi`
- API contract tests

### 13.3 完了条件

- 固定ベースラインから抽出したimportとsignatureがLSPとruntimeで一致する。
- 不正なAPI利用をbasedpyright／LuaLSまたはruntime境界で検出する。
- dialogのblocking／non-blocking resultと異常closeが定義書どおりである。
- workerを停止してもRust所有resourceと入力stateが残らない。

## 14. フェーズ9 — profile・コマンド・callback統合

**依存**: フェーズ7、フェーズ8

**参照**: §4.1、§4.2、§6.4、§10、§11.5.6

### 14.1 作業

1. profile発見、作成、選択、保存先、per-profile venv、user worker lifecycleを統合する。
2. 12-step profile切替transaction、非再入gate、generation stopping、rollbackを実装する。
3. Python command探索、module／class識別、candidate順序、tag統合を実装する。
4. 固定コマンド、MCU command、shortcut 10件を同じ表示modelへ統合する。
5. `ScriptLoadPost`後に有限tag一覧を確定し、全tagの表示一覧を事前計算して1世代として置換する。
6. custom sort callbackで重複、欠落、separator、空一覧を保持する。
7. command start／pause／resume／stop、reload、error、display cache loadingを実装する。
8. profile切替中のcallback再入、user worker停止失敗、late IPC、cache再構築失敗を試験する。
9. 単一のcommand callbackは登録／解除関数ではなくPythonのcallable変数またはLua関数への代入で設定し、`None`／`nil`で解除する。関連priorityとsoft／grace／hard timeoutを同じ名前空間に置き、専用値が未設定なら`dynamic.callback_*`を継承する。

### 14.2 成果物

- profile manager
- command discovery／execution service
- tag／display cache
- custom sort bridge
- profile／command integration tests

### 14.3 完了条件

- profile切替の全成功／失敗地点で整合した旧状態または新状態のどちらかだけが見える。
- command候補の発見順を保ち、暗黙の追加sortを行わない。
- 全tag cacheが一世代として切り替わり、途中結果をUIへ見せない。
- 空一覧、重複、欠落、separatorをcallback結果としてそのまま表現できる。
- callback解除、priority変更、timeout継承変更が次の事前計算世代へ原子的に反映される。

## 15. フェーズ10 — REST・WebSocket・WebRTC

**依存**: フェーズ3、フェーズ5〜フェーズ9

**参照**: §3.4、§7、§8、§15.9〜§15.11

### 15.1 作業

1. axumへ設定、状態、command、device、notification test、screenshot、動的設定、profile launcher、更新確認のREST endpointを実装する。
2. utoipaからOpenAPIを生成し、closed object、discriminated union、decimal string型を表現する。
3. OpenAPIからTypeScriptを生成し、手書きwire型を禁止するdrift検査を作る。
4. 単一`ui.state.changed`、ephemeral event、WebRTC signaling、input messageのWebSocket unionを実装する。
5. global revision、snapshot、buffer、gap検出、domain別replay、再取得を実装する。
6. heartbeat、nonce、再接続、最大試行、手動再接続を実装する。
7. WebRTC video／DataChannelを主経路として実装し、30秒間の復旧試行後にMJPEG／WebSocket fallbackへ移る。
8. static file jail、SPA fallback、Host／Origin／Content-Type／固定header検証を実装する。
9. `server.bind_address`の非loopback完全信頼契約を維持し、認証やpeer-IP制限を追加しない。
10. protocol fuzz、revision競合、out-of-order event、切断／再接続、path traversalを試験する。

### 15.2 成果物

- axum REST server
- OpenAPI documentと生成TypeScript
- WebSocket message router
- WebRTC／MJPEG transport
- static file server
- protocol conformance tests

### 15.3 完了条件

- OpenAPI生成差分がなく、SPAが手書きwire型を持たない。
- RESTとWebSocketの同一transactionが同じrevisionを使用する。
- gap、重複、再接続時にUIが推測で差分を補わず、正しいsnapshotへ回復する。
- WebRTC障害時にfallbackし、復旧後に主経路へ戻る。
- static root外のfileへsymlink、encoded path、Windows pathを介して到達できない。

## 16. フェーズ11 — Web UI

**依存**: フェーズ10。画面単位では生成型とmock endpoint確定後に並行可能

**参照**: §3.2、§3.3、§5、§6、§9、§13

### 16.1 作業

1. SvelteKit 2、Svelte 5、Tailwind CSS v4のshell、routing、state storeを構築する。
2. 6 main tab、Commands内3 subtab、右側panel、7 widget modeを実装する。
3. camera canvas、crop／touch area、screenshot、FPS／解像度／flip設定を実装する。
4. serial monitor、connect state、再試行、data format選択を実装する。
5. keyboard／mouse／gamepad入力とsoftware controllerをinput protocolへ接続する。
6. command一覧、tag filter、separator、10 shortcut、実行制御を実装する。
7. notification、その他、server settings、profile launcher、動的設定editorを実装する。
8. startup-onlyの現在値と保存値、restart-required、apply failure、expected revision競合を表示する。
9. Web modeではlocal file managerを開かず、path表示とcopyだけを提供する。
10. keyboard操作、focus、ARIA、contrast、responsive layoutを検証する。
11. themeは現行組み込み範囲だけを実装し、PWAとcustom themeを将来扱いのままにする。
12. clientだけに属する値は§13の規則に従ってbrowser storageへ保存し、正準設定レジストリの値を重複保存しない。

### 16.2 成果物

- SvelteKit SPA
- generated API client／state store
- 全tabとdialog component
- accessibility tests
- Vitest／component／browser integration tests

### 16.3 完了条件

- 定義書の全UI controlが対応する正準設定または明示的なruntime actionへ接続される。
- 表示だけ存在する未接続controlがない。
- WebSocket再接続とrevision競合から自動回復できる。
- keyboardだけで主要操作を完了できる。
- Chrome／Edge 94以上、Firefox 130以上、Safari 16.4以上でlayout、入力、WebRTCまたは定義済みfallbackが機能する。

## 17. フェーズ12 — Tauri・デスクトップライフサイクル

**依存**: フェーズ6、フェーズ10、フェーズ11

**参照**: §1.3、§6.1.5、§7.4、§15

### 17.1 作業

1. Tauri window、tray、native file dialog、Config directory操作をaxum serverと統合する。
2. GUI環境判定、Tauri mode、standalone Web mode、headless server modeを実装する。
3. `close_behavior`のexit／minimize／background／askを実装する。
4. `AppShutdownPre`、input neutral化、camera、user worker、dynamic worker、shared memory、serial、axum、Tauriの停止順を実装する。
5. 各停止stepへ定義済み期限を適用し、camera writer未停止時のmapping保持とprocess終了を実装する。
6. Linux compositor無効化とWindows相当のstartup-only適用を実装する。
7. native path、download、capturesのscreenshot保存経路をmode別に検証する。
8. close連打、OS shutdown、worker hang、camera hang、server接続中終了を障害注入する。

### 17.2 成果物

- Tauri desktop shell
- tray／native dialog integration
- lifecycle coordinator
- shutdown fault tests
- Linux／Windows desktop smoke tests

### 17.3 完了条件

- 全close behaviorが定義書どおりで、確認dialogの例外条件も一致する。
- shutdownが無期限に停止せず、終了時に入力stateを残さない。
- camera writer停止失敗時もuse-after-unmapを起こさない。
- Web modeからサーバーホストのfile managerを起動できない。

## 18. フェーズ13 — 配布環境・パッケージ

**依存**: フェーズ3、フェーズ4、フェーズ7、フェーズ8、フェーズ12

**参照**: §14、§15.9〜§15.11

### 18.1 作業

1. nix環境と非nix環境の起動経路を分け、同じ実効設定とworker構成へ収束させる。
2. フェーズ3のmanaged uv／venv managerをNix package、Linux package、Windows installerへ統合し、配布後も同じexact syncとlock契約を使う。
3. dynamic workerとprofile user workerのvenv／manifest／HMACがinstall／upgradeで混同・消去されないことを検証する。
4. generated Python／Lua typingsとuser-editable設定がData／Configの正しい配布先へ生成されることを検証する。
5. Rust binary、Tauri bundle、Python wheel、Web assetsをNixから再現可能にbuildする。
6. LinuxとWindowsの依存library、camera／serial権限、WebView runtimeをpackageへ含める。
7. release workflowのcrate一覧、artifact、version、公開順を実際のworkspace構成へ合わせる。
8. install、upgrade、profile維持、uninstall、offline起動を検証する。

### 18.2 成果物

- Nix package／app
- Linux packageとWindows installer
- Python wheel
- 配布物へ統合済みのmanaged uv／venv環境
- reproducible release artifact

### 18.3 完了条件

- clean machineでNix経路と非Nix経路の両方から起動できる。
- 同じlockと入力から同じdependency環境を再現できる。
- Config／Data／State／Cacheの責務が混ざらない。
- package後のSPAが404 fallback pageだけにならず、全assetを含む。

## 19. フェーズ14 — 互換コーパス確定と自動追補

**依存**: フェーズ8、フェーズ9、フェーズ13。コーパス収集自体はフェーズ1から継続

**参照**: §4.6、§10.7、§14.5

### 19.1 作業

1. 固定3ベースラインの全対象スクリプトをmanaged user worker環境で実行する。
2. import、class discovery、signature、controller、serial、camera、image、dialog、network、notificationを領域別に検証する。
3. 実機依存scriptへdeterministic fixtureまたは明示的な実機gateを割り当てる。
4. 将来repoのcandidateをimmutable SHAで収集し、API差分と実行結果を記録する。
5. 非破壊candidateだけをappend-only領域へ自動昇格し、固定領域を変更しない。
6. 破壊的candidate、環境依存失敗、未検証candidateを昇格させず、理由付きで隔離する。
7. corpus manifest、artifact、result、promotion履歴の改ざんとdriftをCIで検出する。

### 19.2 成果物

- 固定互換コーパス
- append-only追補コーパス
- compatibility runner
- promotion reportと隔離一覧

### 19.3 完了条件

- 固定3ベースラインが全件成功する。
- 新規破壊的candidateが固定保証を狭めない。
- 同じSHAとfixtureから同じ判定を再現できる。
- 互換失敗時にscript、API、fixture、worker logまで追跡できる。

## 20. フェーズ15 — 統合・性能・リリース

**依存**: フェーズ1〜フェーズ14

**参照**: §3、§14、§15および各機能節

### 20.1 作業

1. Rust、Python、Lua、Web、Tauriを同一process topologyで起動するend-to-end試験を作る。
2. camera＋WebRTC＋serial＋script＋UIを同時に動かす負荷試験を実行する。
3. WebRTC映像遅延100ms未満、Motion JPEG fallback遅延50〜150ms、controller入力遅延50ms未満、UI 60 FPS／入力応答16ms未満を再現可能な計測条件で検証する。
4. callback queue、IPC queue、WebSocket再接続、profile切替、shutdownを長時間stress testする。
5. LinuxとWindowsで実機camera、serial device、browser、desktop modeを検証する。
6. LAN完全信頼警告、secret非露出、path jail、dynamic code実行をsecurity acceptanceとして確認する。
7. full Nix gate、生成物drift、source filter、remote flake、package build、installer smoke testをCIへ統合する。
8. README、利用者向け設定説明、移行手順、troubleshooting、changelogを現行実装から作成する。
9. versionを全artifactで一致させ、署名、review、CI、release artifact hashを確認する。
10. release candidateで固定互換コーパスと全platform matrixを再実行する。

### 20.2 最終検証ゲート

```bash
nix fmt -- --ci
nix run .#clippy
nix run .#cargo-test
nix run .#ruff-check
nix run .#ruff-format-check
nix run .#basedpyright
nix run .#test
nix run .#web-check
nix run .#generate-api-types
nix run .#check
nix build .#pokecon-server
```

上記に加え、次を成功させる。

- Linux／Windows package build
- OpenAPI／TypeScript／Python／Lua生成物drift検査
- virtual camera／serial統合試験
- shared memory／IPC／shutdown fault injection
- 固定3ベースライン互換コーパス
- WebRTC主経路、MJPEG／WebSocket fallback、主経路復旧
- Tauri／Web mode smoke test
- clean install／upgrade／offline起動

### 20.3 完了条件

- 全必須CI、review、platform gateが成功する。
- 未解決の重大・高severity defectがない。
- 対象外機能を実装済みとして表示しない。
- release artifactとsource commitの対応を検証できる。
- 実装後文書が実際のCLI、設定、UI、API、既定値を反映する。

## 21. 横断的な受入基準

### 21.1 型とschema

- Rust内部型、OpenAPI、TypeScript、Python、Luaで同じ閉じたenumと判別unionを使う。
- `Any`、無制限`dict[str, object]`、未検証raw JSONで公開境界を作らない。
- JavaScript安全整数範囲に依存しない値は10進整数文字列として扱う。
- generated artifactを手編集せず、正本から再生成する。

### 21.2 並行処理

- 同じ登録IDだけを直列化し、異なるIDの並行性を不必要に失わない。
- priorityは待機中の開始順だけへ作用し、実行中処理をpreemptしない。
- lock順序、generation、cancel token、resource ownerを試験で確認する。
- timeoutの論理完了と実process／task終了を混同しない。

### 21.3 障害時動作

- 失敗時は部分適用状態を公開しない。
- fallback、rollback、利用不能化、process終了のどれを選ぶかを定義書どおりに固定する。
- secret、生path、任意コード内容を診断へ出さない。
- worker、device、networkの障害後も入力をneutralへ戻す。

### 21.4 プラットフォーム

- LinuxとWindowsのraw selectorを共通indexへ書き換えない。
- platform差はadapter内へ閉じ、上位serviceの状態遷移とerror codeを揃える。
- macOS固有対応を対象platformの完了条件へ混ぜない。

### 21.5 UI

- 設定controlは正準CIDと一対一に対応する。
- startup-only、pending restart、apply failure、revision conflictを推測せず表示する。
- backend処理を伴う離散操作はREST、通知はWebSocket、stream入力はDataChannel／WebSocketを使う。
- Web modeとTauri modeの権限差をUIで明示する。

## 22. リスクと対策

| リスク | 対策 | 阻止条件 |
|---|---|---|
| 定義書と実装の乖離 | spec-first変更、契約生成、drift CI | 未反映の外部挙動変更があるPRはmergeしない |
| IPC／共有メモリ競合 | generation、CAS、pin、fault injection | race再現試験が不安定または未実施なら後続統合へ進まない |
| 動的workerの停止不能 | 通常時再生成禁止、shutdown時だけ強制終了 | resource安全性をRust側で証明できない実装を採用しない |
| 設定表面の欠落 | 正準registryから全表面を生成・検査 | CID／環境変数／OpenAPI件数差があればmergeしない |
| 固定互換性の後退 | immutable corpus、append-only昇格 | 固定3ベースライン失敗を既知問題として許容しない |
| 実機依存によるCI空洞化 | virtual fixture＋別実機gate | 実機試験だけでしか検出できない契約を無検証で完了扱いしない |
| LAN任意コード実行 | 完全信頼の明示、既定localhost、UI警告 | 認証済みと誤認させる表示や説明を許可しない |
| Nix packageのsource漏れ | source filter検査、package後SPA試験 | packageがfallback pageだけを含む場合はreleaseしない |
| 既存workflowの古いcrate名 | workspaceからpublish graphを生成・検証 | 存在しないcrateを公開するrelease jobを残さない |

## 23. 進捗管理

各フェーズは次の状態だけを使用する。

- **未着手**: 前提フェーズまたは実装が開始されていない。
- **実装中**: branch上で実装・試験を進めている。
- **検証中**: 成果物が揃い、フェーズの受入試験とreviewを実施している。
- **完了**: 必須試験、review、CI、mergeが完了している。
- **阻止**: 外部依存または未解決の仕様判断により進行できず、根拠と解除条件が記録されている。

進捗表には、各フェーズのPR、基準commit、成功した検証gate、未解決riskだけを記録する。単なる作業量やファイル数を完了根拠にしない。
