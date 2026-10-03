# Poke-Con EX製品仕様

> 対象読者: 本プロジェクトの開発者および実装支援agent。エンドユーザー向け説明書ではありません。
>
> 状態: 本書は仕様文書群の入口です。製品の規範要件は以下3文書に分割して定義します。実装現状や開発計画は規範要件を変更しません。

## 要件の分類

仕様は担当領域と要件分類の二つの軸で整理します。

**必須要件**は、スクリプトや周辺機器との互換性、設定解決規則、シリアル通信protocol、resource ownership、安全停止、共有wire schemaなど、利用者や連携先が依存する安定契約です。変更する場合は利用者の明示判断、影響調査、versioningまたは移行、受入更新を同時に行います。

**機能要件**は、利用者が実行できる操作、表示、失敗からの復旧、UI構成、応答性、latency等の品質目標です。現在の製品目標としてすべて満たす必要がありますが、製品判断で改訂できます。実装technologyを固定する分類ではありません。

二つの分類は優先順位ではありません。機能要件も任意ではなく、必須要件も実装手段そのものを固定するとは限りません。要件が混在する章では節ごとに分類します。

## 規範定義書

| 定義書 | 主な責務 | 必須要件 | 機能要件 |
|---|---|---|---|
| [バックエンド定義書](docs/SPECIFICATION_BACKEND.md) | 設定・動的設定、Python／Lua runtimeと互換性、user script API、device/resource ownership | settings/script stable contract、serial wire、runtime compatibility、安全停止 | backend capabilitiesとinternal implementation |
| [フロントエンド定義書](docs/SPECIFICATION_FRONTEND.md) | 各UIの操作、画面、可視状態、client-local behavior | 既存Web／Tauri維持、GPUI選択起動、backend state ownershipの尊重 | UI操作・情報配置・accessibility・browser support・latency |
| [連携定義書](docs/SPECIFICATION_INTEGRATION.md) | HTTP／WebSocket／WebRTC、end-to-end保存・設定操作、lifecycle、GPUI境界 | 既存wire contract、state同期、安全停止、GPUI接続境界 | 操作反映、通信回復、接続方式のPoC選択、体感性能 |

定義書中の数値・wire formatは対象範囲内で規範です。実装・試験の都合で緩和しません。

## FrontendとGPUIの適用範囲

現行のWeb browser UIとTauri desktop UIを維持し、native GPUI frontendを追加する方向で定義します。

frontendは起動時に選択し、同じRust backendが状態、camera、serial、worker、settings、shutdownを一度だけ所有します。

目標起動形は`nix run . -- --ui web`、`nix run .#tauri`、`nix run .#gpui`です。GPUI入口とfrontendは未実装のため、実装完了とは扱いません。

Svelte／Tailwind／Tauri固有の機能は現行implementationまたはそのmodeに限定します。GPUI採用は既存script API、serial protocol、browser wire contract、backend ownershipを変更する理由になりません。

GPUIのWASM/browser提供は、本仕様から自動的に要求されません。native GPUIと既存Svelte browser UIは別frontendとして扱います。

## 旧section番号からの移行対応

既存の`SPECIFICATION.md`で使っていた`§`番号をトレーサビリティ用に維持します。複数文書に同じ親番号がある場合は、次の対応表で参照先を決めます。

| 旧番号 | 正本 |
|---|---|
| §1.1、§1.4、§3.1–§3.3、§4.1–§4.3、§4.5、§4.7、§5、§6（§6.1.5・§6.1.6・§6.2.2を除く）、§9、§11.5.7、§13、付録A | フロントエンド定義書 |
| §1.2、§4.4、§4.6、§6.1.6、§7.8–§7.9、§10–§12（ただし§11.5.7を除く）、§14、付録B | バックエンド定義書 |
| §1.3、§3.4、§6.1.5、§6.2.2、§7.1–§7.7、§8、§15 | 連携定義書 |
| §2 用語集 | 本書 |

定義書間の参照には参照先の文書名を添えます。旧番号を用いたまま未分類の要求を追加せず、追加時に担当と分類を記録します。

旧§4は明示的なユーザー指示を記録する節でした。その各子節は上表の担当文書へ移し、内容を削除していません。旧§1・§4・§7の親見出し自体は独立した要件を持たず、それぞれの子節で担当・分類・正本を定めます。

## 正準情報と補助文書

設定fieldの機械可読な正本は`rust/pokecon/registry/settings.json`です。

HTTP／WebSocket schemaの正本は`api/openapi.json`と`rust/pokecon/registry/protocol.json`です。

シリアルwire protocolの公開正本は[周辺機器開発ガイド](docs/PERIPHERAL_DEVELOPMENT.md)です。

script APIの公開互換仕様はバックエンド定義書とし、生成typingsはその契約を開発時に補助します。

[アーキテクチャ文書](docs/ARCHITECTURE.md)、[総合トレーサビリティ表](docs/TRACEABILITY_INDEX.md)、[GPUI段階導入計画](docs/GPUI_FRONTEND_PLAN.md)、`PLAN.md`は実装説明または進行計画です。要件と矛盾する場合は本仕様群を正とし、補助文書を同期します。

## 2. 用語集

| 用語 | 定義 |
|------|------|
| **フラット構造** | ドット区切りの階層を持たない、単一レベルの属性アクセス方式。`pokecon.opt` 名前空間では、意味的に独立した単体の設定のみフラットパスを使用する。例: `pokecon.opt.language`（フラット）vs `pokecon.opt.camera.capture_fps`（階層） |
| **階層構造（名前空間）** | 意味的に関連する複数の設定値をグループ化するドット区切りの名前空間。`pokecon.opt.camera.capture_fps` のように、サブシステム単位で階層化する。TOMLのセクション構成とは独立した規範的な設計であり、自動変換ルールに依存しない |
| **Neovim/Vim風** | Neovim/Vimエディタの設定方式を模した設計。イベント名の`Pre`/`Post`後置（`BufReadPre`/`BufReadPost`に類似）、キー記法の`<C-a>`形式等 |
| **後勝ち** | 複数の設定ソース間で同じキーが存在する場合、優先順位の高いソースの値を採用する方式。起動時設定はこの優先順位に基づく評価パイプラインとして、各ソースを順次適用する |
| **動的設定** | 実行時に評価される設定ファイル（`init.py`/`init.lua`）。イベントハンドラ登録やカスタムロジックを含む |
| **静的設定** | 起動時に読み込まれる設定ファイル（`settings.toml`）。TOML形式で、グローバル設定やプロファイル管理を含む |
| **Pre/Postフェーズ** | イベントの実行前（Pre）と実行後（Post）の2つのフェーズ。イベント名に`Pre`/`Post`を後置して区別 |
| **XDG Base Directory** | Linux/Unix系の設定・データ・キャッシュ・状態ディレクトリの標準規格。`${XDG_CONFIG_HOME:-$HOME/.config}/pokecon`（設定）、`${XDG_DATA_HOME:-$HOME/.local/share}/pokecon`（データ）、`${XDG_CACHE_HOME:-$HOME/.cache}/pokecon`（キャッシュ）、`${XDG_STATE_HOME:-$HOME/.local/state}/pokecon`（状態）。Windowsでは各々 `%APPDATA%\\pokecon` / `%LOCALAPPDATA%\\pokecon\\data` / `%LOCALAPPDATA%\\pokecon\\cache` / `%LOCALAPPDATA%\\pokecon\\state` にマッピングされる（[バックエンド定義書](docs/SPECIFICATION_BACKEND.md) §14.1.1参照） |
| **HandlerId** | イベントハンドラの登録時に返される識別子。ハンドラの解除（`off()`）に使用。型: `int` |
| **フォールバック** | プライマリ方式が利用できない場合に使用される代替方式。例: WebRTC不可時のWebSocketフォールバック |
| **デッドゾーン** | アナログスティック等の入力デバイスにおいて、中央付近の微小な入力を無視する領域 |
| **チャタリング** | 機械的な接点のバウンスにより、意図しない短時間の連続入力が発生する現象 |
| **シグナリング** | WebRTCにおいて、通信相手との接続確立に必要な情報（SDP、ICE candidate等）を交換するプロセス |
| **holdEndSkip** | ボタンホールド中にShift+クリック（またはShift+タッチ終了）を行うことで、バックエンドに解放シグナルを送信せずに視覚的なボタン状態のみをリセットする操作。アプリケーション終了時やプロファイル切替時には、holdEndSkip中のボタンも含めて通常の解放フレームをアクティブなトランスポート経由で送信してから切断し、Rustのオーソリタティブ状態を常にクリアする（トランスポート利用不可の場合は送信をスキップしても状態はクリアする）。shift-release単体では解放シグナルを送信せず、これが意図された設計である。 |
| **StopThread** | コマンドスレッドを安全に終了させるための例外型。`checkIfAlive()` で `self.alive` が `False` の場合に送出される |
| **augroup** | Neovimのイベントハンドラグループ機能。`pokecon.autocmd` の `group` パラメータに相当 |
| **CRF** | Constant Rate Factor（固定品質係数）。H.264/VP9等の動画エンコーダーで品質を固定し可変ビットレートでエンコードする方式 |
| **WebRTC** | Web Real-Time Communication。ブラウザ間でP2Pのリアルタイム通信（映像・音声・データ）を行うW3C標準API |
| **SDP** | Session Description Protocol。WebRTC接続確立時に通信パラメータ（コーデック、解像度等）を交渉するためのテキストベースプロトコル |
| **ICE** | Interactive Connectivity Establishment。NAT越えの通信経路探索技術。ICE candidateは接続候補アドレス |
| **STUN** | Session Traversal Utilities for NAT。NAT環境下でのグローバルIPアドレス発見に使用するプロトコル |
| **SPA** | Single Page Application。単一HTMLページ内で画面遷移を行うWebアプリケーション形式 |
| **PWA** | Progressive Web App。サービスワーカー等を用いてネイティブアプリライクな動作を実現するWebアプリ形式 |
| **GIL** | Global Interpreter Lock。CPythonで同時に実行できるスレッドを1つに制限する機構。`Python::with_gil()` で取得する |
| **OpenAPI** | OpenAPI Specification。REST APIの仕様を記述する標準フォーマット。`utoipa`（Rust）+ `openapi-typescript` でTypeScript型を生成する |
| **MJPEG** | Motion JPEG。各フレームをJPEGでエンコードする動画フォーマット |
| **Camera** | カメラデバイスを管理するクラス。OpenCVベースで、フレーム取得・FPS制御・反転設定・キャプチャ保存等を担う。`self.camera` として `ImageProcPythonCommand` に注入される。UIフレームワークに依存しない |
| **CaptureArea** | カメラ映像の表示領域を管理するクラス。元実装は `tk.Canvas` のサブクラス。オーバーレイ描画（矩形・テキスト）、UI表示サイズ管理、マウス/タッチイベント処理等を担う。`self.gui` / `self.canvas` として注入される（エイリアス）。リファクタリング後はUIフレームワークに依存しないバックエンド実装となる |

---
