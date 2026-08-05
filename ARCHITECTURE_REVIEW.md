# PokeCon実装アーキテクチャレビュー

## 1. 文書の目的

この文書は、現在の実装全体を、利用者が意図するPokeConの構造へ修正するためのレビュー記録です。

対象には、プロセス境界、状態とハードウェアリソースの所有権、データ経路、公開API、設定とプロファイル、障害隔離、起動停止、配布形態、内部モジュール、Rustクレート構成を含めます。

クレート構成は実装アーキテクチャから導く下位判断であり、認識調整の出発点にはしません。

現在の実装を正解として定義を合わせる文書ではありません。

現在の実装は調査対象であり、認識調整で確定した利用者の意図が変更後の基準になります。

Codexは、確定した判断、修正要求、受入条件に従って実装を変更します。

この文書は認識調整の進行に合わせて更新し、以前の暫定判断と確定判断が矛盾する場合は、古い記載を残さず最新の判断へ置き換えます。

現在の構造に至った経緯や、過去の指示漏れの原因は調査対象にしません。

## 2. レビューの進め方

認識調整は、製品の機能要件と優先順位から個別の実装境界へ向かう順序で行います。

先に大きな責務境界を確定し、その判断から導ける下位の配置は自律的に更新します。

利用者の判断が必要な設計上の分岐だけを、一件ずつ確認します。

確認順は次のとおりです。

1. PokeConが利用者へ提供する最も重要な価値
2. 定義書に記載する全機能の役割と、機能が競合した場合の処理優先順位
3. 互換性、安全性、性能など、機能を成立させる品質要件
4. 機能間の情報と制御の流れ
5. 状態とハードウェアリソースの所有者
6. Rustメイン、ユーザースクリプトワーカー、動的設定ワーカーのプロセス境界
7. Web、Tauri、HTTP、ユーザースクリプト、動的設定の公開境界
8. 設定、プロファイル、コマンドのライフサイクル
9. 障害、タイムアウト、再生成、終了処理の責任分担
10. Windows、Linux、Nix、非Nix、配布成果物の境界
11. 内部モジュール、Rustクレート、依存方向
12. ビルド、テスト、受入条件

確定済みの上位判断から一意に決まる下位項目は、追加確認せずこの文書へ反映します。

## 3. 機能要件の優先順位

固定ベースラインのREADMEは、PokeConをゲーム機自動化支援ソフトウェアと説明しています。

現在の利用ガイドでは、Python／MCUコマンドの実行、カメラ映像の取得、シリアル経由のコントローラー出力、手動操作、通知、設定を一つのアプリケーションから利用できます。

定義書は、固定ベースラインと自動追補保証コーパスに含まれるユーザースクリプトを変更なしで動作させることを必須要件としています。

認識調整により、PokeConの最優先機能はPython／MCUコマンドの自動実行環境であると確定しました。

この実行環境は、スクリプトを評価する言語ランタイムだけを指しません。

ゲームを操作するシリアル通信、ゲーム状態を画像認識するためのカメラ、およびスクリプト実行部分と両サブシステムを結ぶ低遅延で安定した通信までを一体の主機能として扱います。

自動実行時は、ユーザースクリプトワーカーを機能上の実行主体とします。

RustメインがOS上でワーカーを起動・監視・停止することや、ハードウェア資源を所有することは、製品機能上もRustメインが主であることを意味しません。

調査で確認できた機能群と、定義書上の扱いは次のとおりです。

| 機能群 | 定義書で確認できる扱い | 相対的な優先順位 |
|---|---|---|
| Python／MCU自動化コマンド | 実行制御と既存スクリプト互換性を必須とする | 最優先実行環境の実行主体 |
| カメラと画像認識 | 映像取得、画像処理、スクリーンショットを提供する | 最優先実行環境の不可欠な構成要素 |
| シリアルとコントローラー出力 | ゲーム機への入力と複数形式を提供する | 最優先実行環境の不可欠な構成要素 |
| シリアル出力と画像認識の相互関係 | 両方を低遅延かつ安定して提供する | 同じ高い優先度とし、明示的な優先処理の要否は実測して判断する |
| 主機能内部の通信 | スクリプト実行部分、カメラ、シリアルを接続する | 低遅延かつ安定していることを最優先要件に含める |
| 停止と入力解放 | コマンド停止、全入力解放、ニュートラル状態の送信を行う | 他の通常処理より先に処理する最上位の競合時優先度 |
| 手動操作 | ソフトウェアコントローラー、キーボード、マウス操作を提供する | 接続確認、デバッグ、復旧の必須支援機能。単独利用時も低遅延かつ安定して動作する |
| Web UI | 旧Tkinter UIとの機能・視覚的パリティを必須とする | 操作・監視・停止・復旧の必須支援機能。競合時は制御要求を表示配信より優先する |
| Tauri UI | Web UIと同じフロントエンドをデスクトップ表示する | Web UIより優先度の低いデスクトップ提供形態 |
| 静的設定とプロファイル | 実行環境の構成、複数用途、切替を提供する | 実行環境を再現可能に構成し、安全に切り替える必須支援機能 |
| 動的設定 | Python／Luaによる実行時カスタマイズを提供する | 自動実行を停止せず、成功時だけ候補世代へ一括切替する必須支援機能 |
| 通知 | Windows通知とDiscord通知を提供する | 有界キューと期限付き再試行へ隔離し、主経路を待たせない必須支援機能 |
| PWA、テーマ、ハードウェア制御 | 今後のバージョンで実装する | 現行機能より低いことが明示されている |

残る機能の仕様上の「必須」は各機能の受入条件を示しますが、最優先実行環境に対する相対的な重要度までは示しません。

定義書に記載する機能は、すべて製品に必要な機能です。

優先順位は、機能を削除、省略、未完成のまま許容するための区分ではありません。

複数機能が同時に低遅延処理、計算資源、I/O、ロック、キューを要求した場合に、どの処理を先に進め、どの処理を後へ回すかを決めるために使用します。

低い優先順位の機能も、単独で動作する場合は、その機能に定義された性能と安定性を満たす必要があります。

この序列を認識調整で先に確定し、機能間の関係から実装境界、状態所有、プロセス、モジュール、クレートを導きます。

この確定済みの序列を、後続の実装アーキテクチャ判断の基準とします。

## 4. 定義書が規定するシステム境界

`SPECIFICATION.md`は、OS上の監督・資源所有と製品機能上の主従を区別しています。

ユーザースクリプトワーカーは、自動実行の順序と次の操作を決定する機能上の実行主体です。

Rustメインプロセスは、ワーカーの起動・監視・停止と、シリアルポート、カメラハンドル、サーバーソケット、正準コントローラー状態、共有状態、安全停止を所有する監督兼資源サービスです。

自動実行中は、ユーザースクリプトワーカーからRustメインへカメラ、シリアル、コントローラー、UI、ネットワーク、通知等の資源操作要求が流れ、Rustメインが安全制約と入力調停を適用して応答します。

Rustメインからユーザースクリプトワーカーへ流れる起動、コマンド開始、停止、世代切替は監督制御であり、通常の自動実行の主制御ではありません。

Tauriとaxumは同じRustメインプロセスで動作し、デスクトップモードとWebモードは同じ実行ファイルの起動方法として分かれます。

機能上の優先順位はWeb UIを上位とし、TauriはWeb UIをデスクトップアプリケーションとして提供するための下位の表示形態とします。

ユーザースクリプトと動的設定は、それぞれ専用の別OSプロセスで実行しますが、両者の機能上の役割は同格ではありません。

ユーザースクリプトワーカーは最優先実行環境の実行主体として、プロファイルに対応するCPython環境を持ち、プロファイル切替時に終了します。

動的設定ワーカーは自動実行環境を構成する支援機能であり、CPythonとLuaJITを同じ管理単位に持ち、プロファイル切替を越えて永続します。

ワーカーとUIは、Rust管理のシリアライズ可能なAPIまたは双方向IPCを介してメインプロセスへ接続し、ハードウェアハンドルや言語ランタイムのネイティブオブジェクトをプロセス境界越しに共有しません。

安全停止、全入力解放、ニュートラル送信は、機能上の主であるユーザースクリプトワーカーの通常要求よりも常に優先します。

この節は定義書に明記済みの目標であり、現在の実装から導いた暫定判断ではありません。

以後のレビューでは、このシステム境界を満たしているだけでなく、利用者が意図する単純さ、変更容易性、互換性、障害時の挙動まで実装へ反映できているかを確認します。

特に、プロセス分離とIPCは、それ自体を目的とせず、最優先実行環境の低遅延性と安定性を満たすかによって評価します。

## 5. レビュー開始時のクレート構成

Cargo workspaceはレビュー開始時点で11クレートから構成されていました。

**合成起点**は、サービスを組み立て、プロセス全体の起動と停止を制御するOS上の監督実装です。これは、製品機能上の実行主体を意味しません。

| クレート | 現在の主な責務 | 概算の非テストRust行数 | 成果物または実行境界 |
|---|---|---:|---|
| `pokecon-app` | 合成起点、サービス接続、プロファイル、コマンド、起動停止 | 10,400 | メイン実行ファイル |
| `pokecon-contracts` | 正準レジストリー、スキーマ、型生成 | 3,100 | ライブラリー、生成コマンド |
| `pokecon-core` | 停止調整、OSシグナル、診断、プラットフォーム抽象 | 400 | 共通ライブラリー |
| `pokecon-camera` | カメラ、フレーム、共有メモリー、画像保存 | 5,100 | ハードウェア所有ライブラリー |
| `pokecon-device` | コントローラー状態、入力、シリアル、通知 | 4,600 | ハードウェア所有ライブラリー |
| `pokecon-dynamic` | 動的設定、イベント、コールバック、Python／Lua実行 | 8,600 | 実行エンジンライブラリー |
| `pokecon-desktop` | Tauri、ウィンドウ、トレイ、終了方針 | 480 | デスクトップアダプター |
| `pokecon-pybindings` | Pythonネイティブ拡張 | 16 | Python用`cdylib` |
| `pokecon-server` | REST、WebSocket、WebRTC、OpenAPI、SPA配信 | 10,100 | 通信ライブラリー |
| `pokecon-settings` | 設定解決、永続化、XDGルート、uv、venv | 8,500 | 設定ライブラリー |
| `pokecon-worker` | ワーカー実行、IPC、世代管理、プロセス監督 | 9,600 | ワーカー実行ファイル |

行数は構造を把握するための概算であり、クレート境界を行数だけで決めるための基準ではありません。

## 6. レビュー開始時の依存構造

各クレートの`Cargo.toml`に記載されたworkspace内の`path`依存を全件調査した結果は次のとおりです。

表の右列は、左列のクレートが直接依存するクレートを示します。

| クレート | 直接依存するworkspaceクレート |
|---|---|
| `pokecon-contracts` | なし |
| `pokecon-core` | なし |
| `pokecon-pybindings` | なし |
| `pokecon-settings` | `pokecon-contracts` |
| `pokecon-desktop` | `pokecon-core` |
| `pokecon-camera` | `pokecon-settings` |
| `pokecon-device` | `pokecon-settings` |
| `pokecon-server` | `pokecon-camera`、`pokecon-contracts` |
| `pokecon-dynamic` | `pokecon-contracts`、`pokecon-device`、`pokecon-settings` |
| `pokecon-worker` | `pokecon-contracts`、`pokecon-camera`、`pokecon-core`、`pokecon-dynamic` |
| `pokecon-app` | 他の全クレート。ただし`pokecon-pybindings`を除く |

この直接依存表から内部依存に循環がないことを確認しています。

循環がないことは、依存方向が利用者の意図に合っていることまでは意味しません。

特に`pokecon-camera`と`pokecon-device`が`pokecon-settings`へ依存する構造は、下位のハードウェア処理が上位の設定適用手順を知る形になっています。

## 7. クレート分割が現在提供している効果

### 7.1 依存方向のコンパイル時検査

別クレートにすると、非公開モジュールの慣習だけに頼らず、Cargoが依存方向を検査できます。

レビュー開始時は`pokecon-app`を参照する下位クレートがなかったため、合成起点への逆依存を防げていました。

ただし、クレート境界は実行時の障害隔離や信頼境界を作りません。

実行時隔離を作るのは、`pokecon-worker`を別OSプロセスとして起動する構造です。

### 7.2 重い依存の隔離

Phase 2.7の最初の不可分atomで`pokecon-desktop` compatibility packageを削除し、Tauri関連依存とdesktop unit testを`pokecon`へ統合しました。Tauri関連依存は製品機能で分岐せず、Tauri設定、ビルドスクリプト、アプリケーション実行ファイル、署名対象、インストーラー生成も`pokecon`が所有します。

標準成果物にWeb UIとTauriの両方を含めるため、`tauri-shell`によるWeb専用ビルドを廃止しました。

`pokecon-camera`はカメラと画像処理のプラットフォーム依存を所有しています。

`pokecon-device`はシリアル、ゲームパッド、Windows通知の依存を所有しています。

`pokecon-server`はWebRTC、OpenH264、axumの依存を所有しています。

分離したクレートが下位の共通クレートへ不要な依存を持つ場合、この効果は弱くなります。

### 7.3 独立した成果物

`pokecon-app`と`pokecon-worker`は別の実行ファイルを生成しますが、別プロセスに必要なのは別の実行ファイルであり、別パッケージではありません。

同じパッケージの複数の`[[bin]]`で、Rust監督・資源サービス、共通ワーカー、開発用生成器を提供します。

ユーザースクリプトワーカーと動的設定ワーカーは同じ共通ワーカー実行ファイルを使い、起動時引数で役割を切り替えます。

Web UIとTauriは別の実行ファイルにせず、同じPokeCon本体実行ファイルの起動時引数で切り替えます。

`pokecon-pybindings`が生成するPythonネイティブ拡張には、現在必要な公開機能がありません。

### 7.4 対象を絞った検査

`pokecon-contracts`は、他のworkspaceクレートを対象にせず単独でビルドとテストを実行できます。

一方、正式な完了ゲートはworkspace全体のClippy、テスト、ビルドを実行します。

現在の分割に対象変更時の反復を短くする効果はありますが、完了ゲート全体を短縮する効果は確認できていません。

### 7.5 Cargo機能フラグとの比較

現在、Webとdesktopを切り替えるアプリケーション固有のCargo機能フラグはありません。`contract-generator`は開発用generator targetだけを有効にします。

`pokecon-app`は、`pokecon-camera`、`pokecon-device`、`pokecon-dynamic`、`pokecon-server`、`pokecon-settings`、`pokecon-worker`へ無条件に依存しています。

したがって、これらを別クレートにしたこと自体は、メインアプリケーションのビルドから各依存を除外するビルドオプションとして機能していません。

同一製品内の任意機能、OS差分、複数の実行ファイルは、まずモジュール、Cargo機能フラグ、target固有依存、複数の`[[bin]]`で表現できるかを検討します。

## 8. クレート構成の確定評価

この節は、確定した「大きくまとめ、必要性が生じた場合だけ分ける」という上位方針をレビュー開始時の11クレートへ適用した評価です。

| クレート | 確定評価 | 評価理由または維持条件 |
|---|---|---|
| `pokecon-app` | `pokecon`本体パッケージへ改称 | メインプロセスと製品全体の合成起点を所有する |
| `pokecon-contracts` | 本体へ統合 | 正準契約と生成は、本体ライブラリーの内部モジュールと開発用実行ファイルで提供できる |
| `pokecon-core` | 本体へ統合 | 共通利用だけでは、単一製品内で別パッケージにする根拠にならない |
| `pokecon-camera` | 本体へ統合 | カメラ所有は内部モジュール、専用スレッド、外部I/O境界で表現できる |
| `pokecon-device` | 本体へ統合 | デバイス所有とOS差分は内部モジュール、トレイト、target条件で表現できる |
| `pokecon-dynamic` | 本体へ統合 | 動的設定ワーカーは共通ワーカー実行ファイルの起動時役割と内部モジュールで表現できる |
| `pokecon-desktop` | 本体へ統合 | TauriはPokeCon本体と同じ実行ファイルの起動モードであり、別パッケージまたは別実行ファイルにする要件がない |
| `pokecon-pybindings` | 廃止 | 現在は未使用の二関数だけを公開し、必要な公開機能または独立成果物がない |
| `pokecon-server` | 本体へ統合 | 通信責務とwire型は本体パッケージ内のモジュールとして表現できる |
| `pokecon-settings` | 本体へ統合 | 設定と永続化を独立再利用または独立配布する要件がない |
| `pokecon-worker` | 本体へ統合 | 別OSプロセスは共通ワーカー用の`[[bin]]`で表現し、スクリプトと動的設定の役割を起動時引数で切り替えられる |

統合後は、Tauriデスクトップを含むPokeCon本体の1パッケージとします。

### 8.1 統合後のCargo構成

統合後のworkspace memberは`rust/pokecon/`だけとし、Cargoパッケージ名を`pokecon`とします。

| 種別 | 名前 | 役割 |
|---|---|---|
| 監督・資源サービス | `pokecon` | Web／Tauri切替、OS上の監督、正準状態、安全停止、ハードウェア所有 |
| 共通ワーカー | `pokecon-worker` | `--kind script`では自動実行の機能上の実行主体、`--kind dynamic`では動的設定支援を担う別OSプロセス |
| 開発用実行ファイル | 契約生成、互換性検査、障害試験 | 配布対象ではない生成・検査専用入口 |

本体ライブラリーは、少なくとも次の内部モジュールに分けます。

| モジュール | 主な責務 |
|---|---|
| `runtime` | 合成、ライフサイクル、停止調整、プロファイル、コマンド実行 |
| `settings` | 設定解決、永続化、XDGルート、uv、venv |
| `camera` | カメラ、フレーム、共有メモリー、画像保存 |
| `device` | コントローラー状態、入力調停、シリアル、通知 |
| `server` | REST、WebSocket、WebRTC、OpenAPI、SPA配信 |
| `desktop` | Tauri、ウィンドウ、トレイ、終了方針 |
| `contracts` | 正準レジストリー、スキーマ、型生成 |
| `worker` | 双方向IPC、世代管理、親側のプロセス監督、ワーカー側の実行入口 |
| `dynamic` | 動的設定、イベント、コールバック、Python／Lua実行 |
| `diagnostics` | ログ、診断、状態観測 |
| `platform` | OSシグナルとtarget固有処理 |

実行ファイルの入口は薄く保ち、本体ライブラリーの内部モジュールを呼び出すだけにします。

内部モジュールは既定で非公開とし、同じ製品内の将来再利用を想定した公開APIを追加しません。

### 8.2 言語ランタイムと内部依存方向

PokeCon本体実行ファイルはワーカーの起動、IPC、世代、停止を管理しますが、CPythonまたはLuaJITの状態を生成しません。

CPythonとLuaJITを初期化してPython／Luaコードを実行する実装は、`pokecon-worker`からだけ参照する実行ファイル固有モジュールに置きます。

本体ライブラリーの`dynamic`と`worker`には、ワーカーとの契約、IPCクライアント、プロセス監督、実行状態だけを置きます。

内部依存は次の方向を許可します。

- 製品実行ファイルの合成起点は全内部モジュールを接続できる
- `runtime`は`settings`、`camera`、`device`、`worker`、`dynamic`、`contracts`、`diagnostics`、`platform`を利用できる
- `server`と`desktop`は`runtime`が公開する製品内の制御面と`contracts`を利用できる
- `worker`と`dynamic`の本体側は`contracts`、`diagnostics`、`platform`を利用できる
- `settings`、`camera`、`device`は`contracts`、`diagnostics`、`platform`を利用できる

次の依存は作りません。

- `contracts`から他の実行時モジュールへの依存
- `camera`または`device`から設定永続化、`runtime`、`server`、`desktop`への依存
- `runtime`から表示・通信アダプターである`server`または`desktop`への依存
- `server`または`desktop`からシリアルポート、カメラハンドル、インタープリター状態への直接アクセス
- ワーカーからメインプロセスが所有するハードウェアハンドルまたは正準状態への直接アクセス
- `settings`、`camera`、`device`、`worker`、`dynamic`、`contracts`、`diagnostics`、`platform`から合成・状態所有モジュールである`runtime`への逆依存

カメラとデバイスへ動的設定を適用するアダプターは`runtime`に置き、各モジュール自身は適用に必要な値型と操作だけを公開します。

## 9. クレート構成に必要な変更

### 9.1 設定アダプターの配置

`rust/pokecon-camera/src/settings_applier.rs`の`CameraSettingsApplier`は、`rust/pokecon-settings/src/service.rs`の`RuntimeSettingsApplier`を実装しています。

`rust/pokecon-device/src/serial/manager.rs`の`SerialSettingsApplier`も同じ設定サービストレイトを実装しています。

このため、カメラとデバイスが、TOML永続化、HMAC、uv、venvまで所有する設定クレートへ依存しています。

統合時には、具体的な設定アダプターを合成起点へ移し、カメラとデバイスの内部モジュールはドメイン固有の設定適用APIだけを公開します。

### 9.2 Pythonネイティブ拡張の必要性

`pokecon-pybindings`が現在公開するのは、実行時バージョンと対象OSを返す二関数だけです。

`pyproject.toml`と正準レジストリーは`pokecon._native`を配布対象として参照しています。

一方、Python実行コードに、この二関数を呼び出す処理または`pokecon._native`を読み込む処理はありません。

Rust境界でなければ提供できない具体的な公開APIと受入テストがないため、`pokecon-pybindings`を削除し、純Pythonパッケージへ変更してネイティブwheelのビルドを削除します。

### 9.3 動的設定エンジンの境界

`pokecon-dynamic`は、イベント、コールバック、トランザクションだけでなく、PyO3とLuaJITの実行実装も含みます。

したがって、現在のクレート境界は、動的実行意味論をワーカーIPCとプロセス監督から分けていますが、インタープリター依存を分けてはいません。

動的設定エンジンは本体パッケージへ統合し、別OSプロセスとして必要な境界は共通ワーカー実行ファイル、起動時の役割引数、IPCで維持します。

### 9.4 正準契約と検査専用データ

`pokecon-contracts`は、設定や公開プロトコルに加えて、CI適用表やテスト分類もライブラリー定数として埋め込んでいます。

検査だけが読むデータを共通ライブラリーへ埋め込むと、その変更が下流クレートの再コンパイル要因になります。

実行時または生成時に必要な契約だけをライブラリーへ残し、検査専用データをテスト側で読む案を採用できます。

### 9.5 serverのwire型

`pokecon-server`は、カメラやコントローラーのドメイン型に対応するwire型を独自に定義しています。

この重複には変換と同期の費用がありますが、ドメイン型へOpenAPI依存を持ち込まない効果もあります。

第二の利用者が存在しない現状では、wire型だけの追加クレートを作る案は採用しません。

## 10. 認識調整で確定した方針

### 10.1 最優先の製品機能

PokeConの最優先機能は、ユーザーがPython／MCUコマンドを自動実行するための環境です。

この環境には、コマンドを実行する言語ランタイム、ゲーム状態を画像認識するためのカメラ、ゲームを操作するシリアル通信、およびこれらを結ぶ通信を含めます。

カメラとシリアル通信は、個々のコマンドが任意に利用する周辺的な追加機能ではなく、最優先実行環境を構成する主要部分です。

コマンド実行部分、カメラ、シリアル通信の間は、低遅延で安定して通信できなければなりません。

既存互換スクリプトが変更なしで動作することも、この実行環境の必須要件です。

自動実行時の機能上の実行主体は、ユーザースクリプトワーカーです。

ユーザースクリプトワーカーはコマンド内の実行順序と次の操作を決定し、カメラ、シリアル、コントローラー、UI、ネットワーク、通知等の操作を双方向IPCでRustメインへ要求します。

RustメインはOS上の監督兼資源サービスとして、ワーカーの起動・停止、ハードウェアハンドル、正準状態、安全制約、入力調停を所有します。RustメインがOS上の親プロセスであることは、製品機能上も主であることを意味しません。

カメラとシリアルはユーザースクリプトワーカーより低い優先度の支援機能ではなく、ワーカーと一体の最優先実行環境です。制御関係ではワーカーから要求される資源サービスであり、通常運転時の処理優先度では主経路を構成します。

UI、手動操作、動的設定、プロファイル管理、通知、表示、ログ配信は最優先実行環境を支える機能として従属させ、ワーカーからの主経路へ待機、逆圧、障害を伝播させません。

ただし、安全停止、全入力解放、ニュートラル送信は、ユーザースクリプトワーカーの通常要求を含む全通常処理より優先します。

### 10.2 UIの役割と優先順位

UIは、最優先実行環境を操作、監視、停止、復旧するための必須支援機能です。

Web UIを主要なUIとします。

TauriはWeb UIより優先度が低く、同じWeb UIをデスクトップアプリケーションとして提供するための表示形態として扱います。

Web UIとTauriは同じPokeCon実行ファイルで提供し、起動時のCLI引数で切り替えます。

Web専用の別成果物は作りません。

対応OS向けの標準成果物はWebモードとTauriモードを常に含み、`tauri-shell`の有無による機能差を作りません。

Tauri固有の都合によって、Web UIまたは最優先実行環境の機能、低遅延性、安定性を制限しません。

最優先実行環境とWeb UIの表示配信が資源を競合した場合は、実行環境とUIからの制御要求を先に処理します。

停止と全入力解放は、UIから要求された場合も最上位の安全経路を使用します。

映像表示は古いフレームを蓄積せず、最新の完全なフレームへ追従します。

同じ項目の状態更新は古い更新を集約し、最新状態を配信します。

ログ配信には有界キューを使用し、実行経路へ逆圧を掛けません。

低速または切断されたUIクライアントによって、最優先実行環境を待たせません。

### 10.3 手動操作の役割と品質

手動操作は、自動実行と同格の主機能ではなく、接続確認、コマンド開発時のデバッグ、障害時の復旧を行う必須支援機能です。

手動操作のために自動スクリプトの機能、性能、安定性を低下させません。

一方、手動操作だけを利用する場合も、低遅延かつ安定して動作する必要があります。

自動スクリプト機能を使わないことは、手動操作経路の品質要件を緩和する理由になりません。

自動スクリプト実行中の手動介入は、動作の微調整などの有効な用途として維持します。

スクリプト実行中のコントローラー入力をスクリプトへ排他的に所有させるか、手動介入を許可して入力を調停するかは設定可能にします。

既定では手動介入を許可し、無人運転などで厳密な排他が必要な利用者だけが排他モードを有効にします。

手動介入を許可した場合にスクリプトの動作が変化することは、介入したユーザーの責任として扱います。

排他モードでは停止と全入力解放を除く手動入力を拒否し、介入許可モードでは定義された入力調停規則に従ってスクリプト入力と手動入力を合成します。

介入許可モードでは、ユーザーが現在操作している入力要素だけを手動入力で一時的に上書きします。

手動で操作していない入力要素はスクリプト入力を維持し、手動操作を終えた入力要素はスクリプト入力へ戻します。

ボタン入力はスクリプト入力と手動入力を合成します。

この設定はCLI引数、設定ファイル、環境変数、Web UIから変更でき、適用後の入力から即時に反映します。

### 10.4 仕様機能と競合時の優先順位

定義書に記載する機能は、すべて必要な機能です。

機能の優先順位は、ある機能を不要または省略可能と判断するために使用しません。

優先順位は、複数機能が同時に低遅延処理、計算資源、I/O、ロック、キューを要求した場合の処理順序を決めます。

後へ回す機能も、競合がない状態では、その機能に定義された低遅延性、性能、安定性を満たさなければなりません。

停止要求、全入力解放、ニュートラル状態のシリアル送信は、他の通常処理より先に処理します。

この最上位の優先順位は、安全な状態への遷移を先に完了させるために使用します。

安全な状態への遷移後は、停止契約に従って残りの処理を継続または終了します。

通常運転時のシリアル出力とカメラ画像認識は、同じ高い優先度を持ちます。

両経路のどちらかを常に先に処理する優先機構は、既定では設けません。

両経路に避けられない資源競合があり、優先機構によって製品全体の遅延と安定性が実測上改善する場合に限り、具体的な優先処理を採用します。

優先機構自体が遅延、ジッター、飢餓、キュー停滞、実装複雑性を増やす場合は採用しません。

### 10.5 静的設定とプロファイル

静的設定とプロファイルは、自動実行環境を再現可能に構成し、用途ごとに安全に切り替えるための必須支援機能です。

主機能の再現性、互換性、複数用途への適用を成立させる基盤として扱います。

実行中にプロファイル切替を要求した場合は、新しいコマンド実行の受付を停止し、実行中のスクリプトを停止して全入力を解放します。

旧プロファイルのスクリプトワーカーを終了した後、プロファイルと関連設定を一括で切り替え、新プロファイルの実行環境を初期化します。

プロファイル切替後に、切替前のコマンドを自動的に再開または再実行しません。

スクリプト停止によって失われた実行状態は復元できるとは限らないため、切替後はアイドル状態とし、ユーザーによる新しい明示的なコマンド実行を待ちます。

切替に失敗した場合は、可能なら旧プロファイルと関連設定を一括で復元し、旧プロファイルの実行環境をアイドル状態まで再初期化します。

この場合も、切替前のコマンドは自動的に再開または再実行しません。

旧プロファイルへの復元にも失敗した場合は、全入力を解放した安全な停止状態を維持します。

失敗した段階をUIとログへ明示し、ユーザーが復旧または別のプロファイル切替を明示的に実行するまで、新しいコマンドを受け付けません。

### 10.6 動的設定の再読み込み

動的設定の再読み込み中も、現在の動的設定を有効に保ち、実行中の自動スクリプトを停止しません。

新しいPython／Lua設定、コールバック登録、コマンド一覧を候補世代として構築します。

読み込みと検証がすべて成功した場合だけ、候補世代へ一括で切り替えます。

切替時点で実行中の旧コールバックは、旧世代のまま完了させます。

読み込みまたは検証に失敗した場合は、現在の設定を変更せず、エラーをUIとログへ通知します。

動的設定の再読み込みによって、カメラ、シリアル、自動スクリプトを待たせません。

### 10.7 通知配信の隔離

通知配信と最優先実行環境が資源を競合した場合は、最優先実行環境を先に処理します。

通知要求は有界キューへ入れ、通知処理では主経路のロックを保持しません。

キュー上限時は主経路を待たせず、通知の呼び出し元へ明示的な失敗を返します。

通知の再試行回数と期限を制限し、外部サービス障害を自動スクリプトワーカー全体へ伝播させません。

通知APIが完了待ちを要求する場合でも、待つのはその呼び出しだけとし、カメラ、画像認識、シリアル通信、他の実行要求を継続します。

停止と全入力解放は、通知処理より優先します。

### 10.8 単一製品としての単純さ

PokeConは、独立した再利用可能部品の集合ではなく、一つの製品として設計します。

将来の独立再利用と交換可能性は設計要件に含めません。

内部設計では、単純さ、変更の追跡しやすさ、状態所有の一元化を優先します。

別プロセス、信頼境界、任意のプラットフォーム依存、異なる配布成果物など、実際の必要性がある場合にだけ強い実装境界を設けます。

再利用可能性だけを理由に、トレイト、サービス層、変換型、クレートを追加しません。

### 10.9 統合を既定とする構成

Rust実装は、最初に大きなパッケージと内部モジュールとして構成します。

任意機能はCargo機能フラグ、OS差分はtarget条件、別OSプロセスは複数の実行ファイルで表現することを先に検討します。

別クレートへの分離は既定とせず、単一パッケージでは満たせない具体的な要件または実測された問題が生じた場合にだけ採用します。

責務の違い、ファイル数、行数、別OSプロセスであることだけでは、別クレートにする根拠としません。

現在の11クレートのうち、`pokecon-pybindings`を除く実装はPokeCon本体の1パッケージへ統合し、Rust監督・資源サービス、共通ワーカー、開発用生成器を複数の`[[bin]]`として配置します。

共通ワーカーは、起動時引数で機能上の実行主体であるユーザースクリプトまたは支援機能である動的設定の役割を選択します。

`pokecon-pybindings`は削除します。

PokeCon本体実行ファイルはWeb UIとTauriの両方を含み、起動時引数で表示形態を選択します。

`tauri-shell`を利用者が選択する製品機能フラグとしては廃止し、対応しないOSが存在する場合だけ内部のtarget条件を使用します。

### 10.10 CI構成とフィードバック時間

現行CIが検証している機能領域は必要ですが、実行構成は変更範囲、Nixの成果物境界、必須ゲートの責務に一致していません。

通常のpushでは、`Basedpyright`、`Lint`、`Nix Source Filter Check`、`Pytest`、`Ruff Check`、`Rust CI`、`SPA 404 Check`、`Remote Flake Test`の最大8ワークフロー、18ジョブが個別runnerで起動します。

各ジョブはcheckoutとNix導入を繰り返します。

`cachix/install-nix-action`はNixを導入しますが、現在のworkflowにはPokeCon固有のNix成果物を保存、配信するバイナリキャッシュがありません。

現行の`source`はRust、Python、Web、文書、workflowを一つの`builtins.path`へ含め、`pokeconPackage.src`にもそのまま渡しています。

このため、製品ビルドに不要な文書変更でもsource hashが変わり、PokeCon本体とWebのNix成果物を再利用できません。

`source-guard`は対象ソースが存在するかを判定する適用可能性検査であり、今回の差分がその領域を変更したかは判定しません。

したがって、ソースが存在する現在のリポジトリーでは、無関係な変更に対する重いジョブの実行を防ぎません。

現行構成には、次の重複があります。

- Rustのformat検査を`Rust CI`と`Lint`の両方で実行する
- Clippyを`Rust CI`と`Lint`の両方で実行する
- Ruff checkとRuff format checkを`Lint`と`Ruff Check`の両方で実行する
- Basedpyrightを`contract-check`と`Basedpyright`の両方で実行する
- source filter検査を`contract-check`と`Nix Source Filter Check`の両方で実行する
- `SPA 404 Check`が`doCheck = true`のPokeConパッケージをビルドし、Rust CIとは別runnerでRustテストを再実行する
- `Remote Flake Test`がローカルとリモートの既定appおよび`check` appをそれぞれ評価、ビルドする一方、`check --help`は検査本体を実行しない

これらは異なる環境を検証するために必要な重複ではなく、同じSHAに対する同じ論理検査の再実行です。

`Ruff Check`だけは`on: [push, pull_request]`で全branchを対象とし、他の通常workflowとtrigger範囲が一致していません。

同じcommitがpushとpull requestの両方で検査される場合にも、重複実行を防ぐ構成がありません。

Windows jobは`Build workspace (Windows)`という名前ですが、実際には`cargo check`だけを実行しており、リンク可能な成果物を生成しません。

Windowsでのリンクとインストーラー生成は`Package CI`が所有するため、通常CIのjob名と契約は`Check workspace (Windows)`へ揃えます。

また、release時の`nix run .#check`にはrelease identity検査が含まれますが、通常CIの個別workflowには同等の検査がありません。

PRでversion、manifest、lock、生成物の整合性を完了条件にするため、重い`check`全体を重複実行するのではなく、release identityの原子的Nix taskを通常CIで一度実行します。

GitHubの既定branchにはactiveな`protect` rulesetがありますが、現在のruleは削除禁止、non-fast-forward禁止、更新制限だけです。

required status checkは設定されていないため、現在のCIは成功しなくてもruleset上はmergeを拒否しません。

通常CIは、次の構成へ変更します。

1. 常に起動する一つの通常CI workflowで変更領域を判定し、最後に安定した名前の集約ゲートを必ず完了させる
2. 文書、契約、Rust、Python、Web、製品smoke、リモートflakeを独立した適用領域として判定する
3. workflowは常に起動し、workflow-level path filterで全体を省略しない
4. 必要のない領域jobは成功扱いで明示的に省略し、branch protectionの必須ゲートを不定にしない
5. feature commitはpull requestイベント、既定branchと明示的な統合branchへの直接反映はpushイベントで検査し、同じSHAを両イベントで重複検査しない
6. 集約CIゲートを既定branchのrulesetでrequired status checkに指定し、検査失敗中または未完了のmergeを拒否する
7. Package CIも常に軽量な集約ゲートを返し、配布物に関係しない変更は明示的成功、関係する変更はOS別package jobの成功を必須とする
8. format、静的解析、生成物drift、テスト、ビルド、互換性検査を削除せず、同じSHAと同じ対象環境では各論理検査を一度だけ実行する
9. 短い文書検査と静的検査は少数のfast jobへまとめ、同じrunner上のNix storeを再利用する
10. RustのClippy、build、test、互換性検査は一つのLinux jobで`CARGO_TARGET_DIR`を共有し、Windows固有のworkspace checkは別jobとして維持する
11. `contract-check`からBasedpyright、source filter、一般shell lintを分離し、契約生成、契約同期、schema、受入記録、API生成物driftだけを所有させる
12. Basedpyright、source filter、shell lint、release identityは対応する原子的Nix taskとして通常CIで一度だけ実行する
13. PokeCon本体、Web、Python、文書、検査スクリプトごとにNix sourceを分け、無関係なファイル変更で製品成果物のderivation hashを変えない
14. 製品パッケージのビルドとRustテストderivationを分け、SPA smokeではテスト済みの製品成果物を再利用してサーバー起動と組込みWeb資源だけを検査する
15. リモートflake検査はリモートSHAのmetadataと既定appの起動可能性を検証し、ローカル側は`nix flake check --no-build`で全出力を評価する
16. 検査を実行しない`check --help`のローカル・リモート重複は削除する
17. PokeCon固有derivationをNixの方法で再利用できるバイナリキャッシュを導入し、信頼済みpushだけが書き込み、pull requestは読み取りだけを行う
18. 同一commitの再実行でsubstituteされたstore path、build対象derivation数、wall-clock時間を比較し、cache hit表示だけで有効性を判断しない
19. 通常CIへbranch単位の`cancel-in-progress`を設定し、新しいpush後も古いSHAの重い検査を継続しない

通常CIの完了目標は、fast jobを3分以内、文書だけの変更を5分以内、製品コード変更の必須ゲートを10分以内とします。

直近10回の同種変更に対する95パーセンタイルが目標を超えた場合は、job名だけでなくstepとderivation単位の時間を記録して回帰として扱います。

`scripts/ci-watch.sh`の既定600秒は、現行の正常なcritical pathより短いため不適切です。

CI短縮後の95パーセンタイルに30パーセント以上の余裕を加えた監視期限へ変更し、監視期限切れとGitHub Actionsの`completed failure`を異なる終了理由として表示します。

`Package CI`と`Release`は、OS別成果物、クリーンインストール、アップグレード、アンインストール、再現可能性、署名対象を検証する独立した配布ゲートとして維持します。

同一入力からパッケージを二回生成する再現可能性検査は意図した重複であり、通常CIの重複削減対象には含めません。

### 10.11 direnvと既定devShellを廃止してNix appへ統一する

現行の`.envrc`は`use flake`だけを実行し、repositoryへ移動したshellへ既定devShellを自動適用します。

既定devShellはRust、Python、Bun、Tauri、品質検査用tool、ビルド用環境変数、pre-commit hook導入を一つの常駐環境として提供します。

一方、CI、format、lint、test、build、生成、互換性検査、packageの再現可能な操作は、すでに個別のflake appまたは`nix fmt`として定義されています。

これらのtaskは必要なtoolと環境変数を自身で宣言しているため、実行前に`nix develop`またはdirenvでdevShellへ入る必要がありません。

direnvとdevShellはCI runner間のNix成果物共有、derivationの再利用、検査時間短縮には寄与せず、常駐するPATHと環境変数によって未定義の直接commandを偶然成功させる経路を作ります。

現行devShellだけに残る用途は、対象を絞った任意Cargo操作、frontend dev server、pre-commit hook導入、エディター向け環境変数です。

ただし、現行devShellは`rust-analyzer`を明示的に含まず、対話的なdesktop buildに必要な`PKG_CONFIG_PATH`と`BINDGEN_EXTRA_CLANG_ARGS`も設定しないため、完全なIDE環境または対話ビルド環境ではありません。

既定devShell、`.envrc`、direnvを必須とする開発経路を削除し、次のflake appへ置き換えます。

1. 任意のCargo subcommandと引数を、固定Rust toolchain、Python、uv、desktop native dependency、共有`CARGO_TARGET_DIR`を設定した上でcallerのworktreeに対して実行する`cargo` app
2. 固定したBunとlock fileからfrontend依存を準備し、callerの`web/`を監視してhot reloadする`web-dev` app
3. `git-hooks.nix`が生成したpre-commit hookを現在のworktreeへ明示的に導入する`hooks-install` app
4. エディター連携が必要な場合に、Nixが固定したlanguage server実行ファイルを出力またはエディターを起動する`editor` app

書き込み、watch、hot reload、対象を絞ったtestを行うappは、`setupWorkdir`の一時copyではなくcallerのworktreeを明示的な作業対象にします。

読み取り専用の完了gateは引き続きNix store上の正準sourceまたは隔離した一時copyを使用し、callerの未追跡fileやambient環境へ依存しません。

`nix develop`、`nix develop --command`、direnv、自動devShellを正規の開発手順として残しません。

`AGENTS.md`、`SPECIFICATION.md`、`PLAN.md`、`README.md`、`docs/DEVELOPMENT.md`、`docs/TROUBLESHOOTING.md`のdevShell前提を、flake appを唯一の開発入口とする記述へ同時に更新します。

移行作業を行う各worktreeでは、direnvが生成した非追跡の`.direnv/`cacheも削除し、version管理または配布の対象にはしません。

Nix appで表現できない対話用途が後から生じた場合も、既定devShellを復活させず、用途と環境を限定したappとして追加します。

## 11. 実装変更へ渡す情報

Codexの実装計画と各変更の受入条件には、次の情報を使用します。

- 定義書に記載する全機能の役割
- 機能同士が資源を競合した場合の処理優先順位
- 停止、全入力解放、ニュートラル状態送信の最上位優先経路
- シリアル出力と画像認識を同じ高い優先度で独立して進める通常経路
- 明示的な優先機構を導入する前後の遅延、ジッター、飢餓、キュー停滞の比較結果
- コマンド実行、画像認識、シリアル出力を結ぶ主経路
- ユーザースクリプトワーカーを自動実行時の機能上の実行主体とする主従関係
- RustメインをOS上の監督兼資源サービスとし、プロセス親子関係と製品機能上の主従を区別する構成
- ワーカーからRustメインへの資源操作要求を自動実行の主制御とし、Rustメインからの起動・停止・世代切替を監督制御とする双方向IPC
- 主経路の遅延、スループット、停止、復旧に関する受入条件
- Web UIを主要UI、Tauriを下位の表示形態とする機能境界
- UI制御要求を表示配信より優先し、映像、状態、ログの滞留を主経路へ伝播させない受入条件
- 自動スクリプトへ影響を与えず、単独利用時も低遅延で安定する手動操作の受入条件
- スクリプト実行中の手動介入を許可または拒否でき、停止と入力解放は常に受け付ける入力調停設定
- 介入許可時に操作中の入力要素だけを一時上書きし、操作終了後にスクリプト入力へ戻す要素単位の調停規則
- 実行停止と入力解放後にプロファイルを一括で切り替え、旧コマンドを自動再開しないライフサイクル
- プロファイル切替失敗時に旧設定だけを復元してアイドル状態へ戻し、復元失敗時は新しい実行を拒否する受入条件
- 動的設定を候補世代で構築し、成功時だけ自動実行を止めずに一括切替するライフサイクル
- 通知を有界キューと期限付き再試行へ隔離し、主経路へ待機と障害を伝播させない受入条件
- PokeCon本体を1パッケージへ統合し、共通ワーカー実行ファイルを役割引数付きの別OSプロセスとして起動する構成
- `pokecon-pybindings`とネイティブwheelを削除する移行手順
- Web UIとTauriを同じPokeCon本体実行ファイルに含め、起動時引数で切り替える構成
- `tauri-shell`によるWeb専用ビルドを廃止し、対応OS向け標準成果物へ両UIモードを常に含める構成
- 製品全体で優先する設計原則
- 正準状態とハードウェアリソースの所有者
- プロセスごとの責務、寿命、再生成規則
- UI、HTTP、IPC、ユーザースクリプト、動的設定の公開境界
- 設定、プロファイル、コマンド、互換性のライフサイクル
- 障害、タイムアウト、ロールバック、終了処理の責任分担
- Windows、Linux、Nix、非Nixの配布形態
- workspace memberを`rust/pokecon/`だけとする移行
- Cargoパッケージ名を`pokecon`へ揃える移行
- 各内部モジュールが所有する責務
- 各内部モジュールが所有しない責務
- 許可する内部依存方向
- 禁止する内部依存方向
- 別プロセスと同一プロセスの境界
- 個別成果物と配布方法
- 移動または削除する型、トレイト、モジュール
- 移行段階、監督・資源サービス実行ファイル、共通ワーカーごとの検証コマンド
- workspace全体の完了ゲート
- 通常CIの変更領域判定、各jobの所有検査、集約必須ゲート
- 同一SHAと同一対象環境で論理検査を重複実行しないworkflow構成
- 製品、Web、Python、文書、検査スクリプトを分けるNix source境界
- PokeCon固有derivationを再利用するバイナリキャッシュの信頼境界
- fast job、文書変更、製品コード変更のCI完了時間目標
- CI時間をstep、derivation、cache substituteの単位で検証する受入条件
- direnv、`.envrc`、既定devShellを削除し、flake appを唯一の開発入口とする構成
- callerのworktreeを対象とするCargo、frontend dev server、hook導入、エディター連携app
- 完了gateの隔離実行と、書き込みまたはwatchを行うappの明示的なcaller-worktree実行を区別する契約

未確定の方針をCodexが現在の実装から推測して補完しないようにします。

## 12. 検証結果

現在のブランチで、次のNix taskが成功しています。

| task | 結果 | 実測時間 |
|---|---|---:|
| `nix run .#contract-check` | 成功 | 1分14秒 |
| `nix run .#build-rust` | 成功 | 1分55秒 |
| `nix run .#cargo-test` | 305件成功 | 1分46秒 |
| `nix run .#clippy` | 成功 | 1分10秒 |

この結果は現在の実装がビルドとテストに成功することを示します。

この結果だけでは、現在の責務配置が利用者の意図に合っているとは判定しません。

### 12.1 CI実行時間

GitHub Actionsの成功済みrunについて、workflowの`startedAt`から`updatedAt`までを計測しました。

| workflow | 成功済みrun | 実測時間 |
|---|---|---:|
| `Nix Source Filter Check`、`Pytest`、`Ruff Check`、`Basedpyright` | `30484270677`、`30484270689`、`30484270715`、`30484270634` | 35秒～56秒 |
| `Lint` | `30381699661`、`30484270721`、`30484595228` | 3分40秒～3分53秒 |
| `Rust CI` | `30484270635`、`30484595476` | 11分21秒～13分24秒 |
| `SPA 404 Check` | `30380122821`、`30484270621`、`30484595626` | 10分1秒～13分6秒 |
| `Remote Flake Test` | `30380123291`、`30381699859`、`30484270651`、`30484595870` | 12分49秒～13分20秒 |

これらのrunでは`createdAt`と`startedAt`が一致しており、10分を超える時間はrunner待ちではなくworkflow実行中に発生しています。

run `30484270635`のRust Linux jobでは、Clippyに2分56秒、全クレートbuildに3分1秒、testに1分33秒、互換性corpusに2分59秒を直列に要しています。

同じSHAの`Lint`でも別runner上のClippyに3分46秒を要しており、Rust静的解析を二重にコンパイルしています。

run `30484270621`の`SPA 404 Check`では、12分40秒のjobのうち`nix build .#pokecon`とサーバー起動が12分6秒を占めます。

run `30484270651`の`Remote Flake Test`では、リモート既定appのhelp実行が12分17秒、別runnerのローカル既定appのhelp実行が11分40秒を占めます。

その後の`check` app helpと`nix flake check --no-build`は各15秒以下であり、critical pathは既定appを二つのrunnerで別々にビルドする構成にあります。

正常終了したcritical pathが600秒を超えるため、現在の`ci-watch.sh`はCI失敗がなくても監視期限切れになります。

特に、文書または定義書だけの変更でもpath filterのない`SPA 404 Check`と`Remote Flake Test`が製品成果物をビルドし、`SPECIFICATION.md`の変更では`Rust CI`も全Rust検査を実行します。

したがって、現在の遅延は単発のrunner混雑ではなく、workflowの適用範囲、重複検査、Nix source境界、成果物再利用の構成上の問題です。

### 12.2 Phase 2.4移行時点のワーカー境界evidence

この節はPhase 2.4完了時点のsource ownershipとOS process境界を固定する移行中の記録です。Cargo package、target、旧クレートdirectoryの統合はまだ完了しておらず、Phase 2.7で行います。

#### 移行中のsource ownership

| source | Phase 2.4時点の所有内容 | 移行状態 |
|---|---|---|
| `rust/pokecon/src/dynamic/` | メインプロセス側の動的設定契約、host、状態、transaction | 正準source。変更の起点はこのdirectoryとする |
| `rust/pokecon/src/worker/` | typed IPC契約、client、generation、OS process supervision | 正準source。変更の起点はこのdirectoryとする |
| `rust/pokecon/src/worker_binary/` | script／dynamic workerのentrypoint、CPython／LuaJITを含む子プロセス専用runtime | 子プロセスだけが参照するruntime source |
| `rust/pokecon-dynamic/src/lib.rs` | `rust/pokecon/src/dynamic/`を再公開するcompatibility facade | 旧package利用者を保つ暫定入口 |
| `rust/pokecon-worker/` | compatibility facade、`pokecon-worker` bin、compatibility／fault-fixture bin、integration test | Phase 2.7までbin／test ownerを維持する暫定package |

この配置はsourceの正準所有を移したことを示しますが、workspace member、Cargo package名、target ownershipの統合完了を意味しません。

#### processごとの所有、寿命、置換規則

| OS process | 機能上の所有 | 寿命と置換 | 所有しないもの |
|---|---|---|---|
| Rust main | 正準controller状態、camera、serial、server、profileと、それらのresource serviceおよびOS process supervision | application lifetime。子processを起動、停止、reapし、generation規則を適用する | CPython／LuaJITのinterpreter state |
| script worker | ユーザーcommandの実行判断と実行、子process内CPython | profile lifetime。profile switchでは旧processを停止、reapしてから置換する | hardware handle、正準controller／camera／serial／profile状態 |
| dynamic worker | 動的設定評価を支援するPython／Lua実行、子process内CPython／LuaJIT | application lifetime。profile switchではdynamic worker process generationを置換せず、停止後に新しいgenerationを作らない | hardware handle、正準controller／camera／serial／profile状態、主たる自動実行判断 |

script workerとdynamic workerのいずれもhardware handleまたは正準状態を所有しません。OS上の親であるRust mainと、製品機能上の実行判断を持つscript workerは同じ「主」を意味せず、dynamic workerは動的設定評価の支援役です。

#### 機能要求と監督制御の方向

```text
ユーザースクリプト／script workerの実行判断
  │ framed MessagePack request
  │ script.host.controller_input、script.host.serial_*、script.host.outputなど
  ▼
Rust mainのresource service／正準状態／hardware所有
  │ framed MessagePack response（完了またはdata）
  ▼
script workerが次の実行判断を継続

Rust main ── spawn／force-kill／reap ───────────────▶ worker
           OS supervision
Rust main ── framed initialize／pause／cooperative shutdown ──▶ worker
           lifecycle control-plane
Rust main ── worker process generation gate
           parent-local supervision
```

Rust mainから返すresponseは要求の完了または取得dataであり、次のscript execution decisionをRust mainへ移しません。spawn、force-kill、reapはOS supervision、initialize、pause、cooperative shutdown requestはframed lifecycle control-plane、worker process generation gateは親process内のsupervisionです。いずれもscript execution decisionの所有移転ではなく、機能上の主制御への再分類を禁止します。

#### OS process boundary

typed request、response、event、logとしてOS process境界を横断する唯一のIPCは、stdin／stdout上のframed MessagePackです。spawn、force-kill、reapはOS supervisor operationであり、domain objectを転送しません。stderrは診断専用のout-of-band byte streamであり、domain objectの転送には使用しません。

CPythonのobject、LuaJITのvalue、interpreter-native object／pointerは子process内に留まります。camera／deviceのnative hardware handleと正準状態はRust mainに留まり、どちらもframed IPCを横断しません。Rust mainは`HostControllerInputRequest`、`HostSerialWriteRequest`、`HostOutputRequest`などのtyped domain／protocol valueだけを送受信します。

cameraのbulk frameだけは、別途定義したshared-memory `SharedFrameRing`を使用します。Rust mainがhardwareとpublicationを所有し、typed `MappingDescriptor`だけをframed MessagePackで一度渡してchild readerがmappingを開くため、camera handleまたは正準状態の所有は移転しません。ringには固定layoutのframe byteとpublication metadataだけを置き、interpreter object／pointerまたはnative hardware handleを渡しません。

#### 反証可能なevidence mapping

次表のchild binary provenanceはintegration testが起動する子実行ファイルを指します。integration test harnessと親側supervisorはsource-builtです。`worker-package-check`のoverrideはstartup testの直接起動とmanaged childを含む通常worker childをすべてexact packaged workerへ置き換え、明示的な`pokecon-worker-fault-fixture`は対象外としてsource-builtのまま維持します。

| test／task | 検証対象 | child binary provenance |
|---|---|---|
| `script_worker_executes_controller_serial_and_output_proxies` | script workerがcontroller、serial、output要求を決定してRust main側hostへ送り、完了後も実行を継続する方向 | 通常testではsource-built worker、`worker-package-check`内ではexact packaged worker |
| `dynamic_worker_runs_both_languages_over_bidirectional_ipc` | dynamic childのCPython／LuaJIT初期化と、host request／eventを含む双方向IPC | 通常testではsource-built worker、`worker-package-check`内ではexact packaged worker |
| `managed_worker_uses_protocol_stdout_and_cooperative_stop` | stdoutがframed protocolだけを運び、typed ping responseとcooperative stop acknowledgementを返すこと | 通常testではsource-built worker、`worker-package-check`内ではexact packaged worker |
| `dynamic_worker_is_forced_only_at_app_shutdown_and_never_regenerated`、`profile_switch_force_stops_and_replaces_only_the_script_worker` | dynamicのforce条件とgeneration retention、scriptだけのprofile replacement | source-built supervisor test harnessとsource-built `pokecon-worker-fault-fixture`。packaged app／workerのforce証拠とは扱わない |
| `both_worker_roles_start_and_exit_cleanly` | `--kind script`と`--kind dynamic`が起動し、protocol stdoutを汚さず終了すること | 通常testではsource-built worker、`worker-package-check`内ではexact packaged worker |
| `nix run .#worker-package-check` | `${self'.packages.pokecon}`のimmutable store outputからappと兄弟workerを導出し、productによるexact sibling `execve`、隔離profile、Lua marker、cooperative stopを確認する。dynamic startup rejectionとstatic fail-soft fallbackのlogがあれば失敗する。process tracingを伴うtask実行はLinux限定で、非Linuxではappを評価できるがunsupported errorで終了し、CI evidenceはUbuntu／Linux jobに限る | product probeはexact packaged app／worker。integration testは通常childだけをexact packaged workerへ置換し、force／generationはsource-built harness／fault fixture |

このevidenceが証明する範囲は、sourceとprocessの所有、機能要求と監督制御の方向、interpreter object／hardware ownership／shared-memory descriptor境界の保存です。Phase 4で行うpriority scheduling、latency、input arbitrationその他のbehavior変更を実装または証明したものではありません。

### 12.3 Phase 2.5a移行時点のdesktop ownership evidence

この節はPhase 2.5a完了時点のdesktop sourceとbundle inputの正準所有だけを固定する移行中の記録です。

| path | Phase 2.5a時点の所有内容 | 移行状態 |
|---|---|---|
| `rust/pokecon/src/desktop/mod.rs` | Tauri window、tray、single instance、close policy | 正準source。旧crateだけがpath指定でcompileし、PokeCon本体はprivate `desktop/facade.rs`から同じ型をaliasする |
| `rust/pokecon/icons/` | tracked desktop icon一式 | 正準icon tree |
| `rust/pokecon/tauri.conf.json`、`rust/pokecon/linux/` | Tauri設定とLinux bundle input | `rust/pokecon/`起点の正準package input |
| `rust/pokecon-desktop/src/lib.rs` | 正準desktop sourceを再公開するcompatibility facade | 旧packageとtest targetはPhase 2.7まで維持する暫定入口 |

`tauri-shell` featureはPhase 2.5a時点では維持していましたが、Phase 2.6で削除しました。Cargo package、workspace member、依存と旧crate directoryの統合はPhase 2.7で行います。

現行の`--ui desktop --exit-after-startup`はTauri windowの生成とevent loopを通らないため、実packageのTauri window起動を証明しません。real Tauri-window packaged proofは次の不可分atomで追加し、このPhase 2.5aでは完了を主張しません。

## 13. 実装の移行順序

クレート統合と機能挙動の変更を一度に混在させず、次の順序で進めます。

1. `rust/pokecon/`と`pokecon`パッケージを作り、現在の`pokecon`実行ファイルのCLIと動作を維持したまま合成起点を移す
2. `pokecon-core`と`pokecon-contracts`を`runtime`、`diagnostics`、`platform`、`contracts`へ移し、停止経路と公開契約生成物が変化していないことを検証する
3. `pokecon-settings`、`pokecon-camera`、`pokecon-device`、`pokecon-server`を内部モジュールへ順番に移し、各段階でworkspace全体を検証する
4. `pokecon-dynamic`と`pokecon-worker`を移し、`pokecon-worker --kind script`と`--kind dynamic`の別プロセス境界、双方向IPC、強制停止、世代管理を維持する。`--kind script`からRustメインへの資源操作要求を自動実行の主制御として扱い、親プロセス側の監督実装へ実行判断を移さない
5. `pokecon-desktop`を`desktop`へ移し、同じ`pokecon`実行ファイルの起動時引数でWebとTauriを切り替える
6. `tauri-shell`によるWeb専用ビルドを廃止して標準成果物を更新し、続いて`pokecon-pybindings`を削除してPython配布物を更新する
7. 旧クレートのディレクトリーとworkspace memberを削除し、Cargo.lock、Nix、CI、リリース、インストーラー、文書内のパスとパッケージ名を更新する
8. 通常CIを変更領域判定、重複のない領域別job、集約必須ゲート、分割したNix source、バイナリキャッシュへ移行し、実測時間と検査完全性を検証する
9. Cargo、frontend dev server、hook導入、エディター連携のflake appを追加して対話用途を移行し、`.envrc`と既定devShellを削除した後、repository全体のdevShell前提を更新する
10. この文書で確定した入力調停、通知隔離、動的設定切替、プロファイル切替の挙動変更を、構造統合とは別の変更として実装する

各段階で、既存の互換性検査、Rustテスト、Clippy、ビルド、契約検査をNix taskから実行します。

旧クレートを削除する前に、それを参照するCargo manifest、Nix式、CI、リリーススクリプト、Pythonビルド、Tauri設定が残っていないことを機械的に検査します。

### 13.1 各段階の受入条件

各構造移行は独立したコミットにし、その段階の検証が失敗した場合は後続のクレートを移動しません。

構造移行中は公開CLI、設定、IPC、生成契約、配布物の名前と配置、実行時挙動を変更しません。

各段階で、少なくとも次を検証します。

- `nix run .#contract-check`
- `nix run .#cargo-test`
- `nix run .#clippy`
- `nix run .#build-rust`
- `pokecon --help`と`pokecon-worker --help`の公開CLI差分
- WebモードとTauriモードの起動検査
- `pokecon-worker --kind script`と`--kind dynamic`の起動、IPC、停止、強制終了検査
- 固定互換性基準に対するPythonコマンド検査

CI移行では、文書だけ、定義書だけ、Rustだけ、Pythonだけ、Webだけ、flakeだけを変更したfixtureまたは実commitを用意し、適用対象jobと省略対象jobが設計どおりであることを検証します。

各論理検査が同一SHAと同一対象環境で一度だけ実行され、集約必須ゲートが成功、失敗、明示的省略を正しく集約することを確認します。

既定branchのrulesetを読み戻し、通常CIとPackage CIの集約ゲートがrequired status checkであり、未完了または失敗時にmerge可能と判定されないことを確認します。

rulesetの検証で管理者権限、APIによる直接merge、rulesetの一時無効化を使用しません。

同一commitを二回実行し、二回目のPokeCon固有derivationがバイナリキャッシュからsubstituteされ、build対象derivation数とwall-clock時間が減少することを確認します。

直近10回相当の実行でfast job、文書変更、製品コード変更の時間目標を満たし、`ci-watch.sh`が正常なcritical pathを監視期限切れにしないことを確認します。

devShell移行では、新しいworktreeでdirenvまたは`nix develop`を使用せず、flake appだけから開発を開始できることを確認します。

`cargo` appで対象package、個別test、lock file更新、metadata確認をcallerのworktreeに対して実行でき、固定toolchainとビルド環境が既存のRust完了gateと一致することを確認します。

`web-dev` appが固定Bunとlock fileを使用し、callerの`web/`に対する変更をhot reloadへ反映し、終了後に依存差分や生成物を意図せずcommit対象へ残さないことを確認します。

`hooks-install` appを新しいworktreeで一度実行した後、git hookがNixで固定したpre-commit検査を実行することを確認します。

`editor` appまたはNixが出力するlanguage server実行ファイルだけで、Rust、Python、TypeScriptの解析がhost toolchainとdirenvへ依存せず動作することを確認します。

Git管理対象fileから`.envrc`、`direnv`、`nix develop`、`devShell`参照を検索し、履歴説明を除いて正規の開発手順に残っていないことを確認します。

移行を検証するworktreeに非追跡の`.direnv/`cacheが残っていないことを確認します。

既存のflake taskをdevShell外から実行し、CI、format、lint、test、build、生成、互換性検査、packageの結果が移行前と一致することを確認します。

通常CIの再構成後も、Package CIとReleaseがOS別成果物、クリーンインストール、アップグレード、アンインストール、再現可能性を従来どおり検証することを確認します。

ワーカー統合後は、配布された`pokecon`が同じ配布物内の`pokecon-worker`を解決でき、プロファイル別環境で両方の役割を起動できることを検証します。

デスクトップ統合後は、Tauri設定、アイコン、バンドル資源、署名対象、Linuxパッケージ、Windowsインストーラーが`rust/pokecon/`を起点に生成されることを検証します。

`pokecon-pybindings`を削除する段階では、Pythonパッケージから`_native`のimportとwheel生成を削除し、Nix、maturin、リリースゲート、成果物一覧にネイティブwheelへの参照が残っていないことを検証します。

最終段階では、リポジトリー全体から旧クレート名と`rust/pokecon-*`パスを検索し、明示的に維持する履歴説明以外の参照がないことを確認します。
