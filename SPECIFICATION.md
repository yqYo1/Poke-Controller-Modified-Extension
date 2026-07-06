# Poke-Controller Modified Extension — 仕様書

> **バージョン**: 2.1.0
> **ブランチ**: `refactor/rust-core`
> **日付**: 2026-06-11
> **ソース**: セッション議事録から抽出した過去のユーザー要件（現在のコードベースではない）

---

## 0. 本ドキュメントの位置づけ

**本ドキュメントは要求仕様書として運用されます。**

**対象読者**: 本プロジェクトの開発者およびLLM（実装支援エージェント）。エンドユーザー向けの説明書ではありません。本ドキュメントに記載されているAPIは、開発者が実装時に参照するための仕様であり、エンドユーザーが直接読むことを想定していません。

- **仕様が先行し、実装が後に追従する**関係です。実装の都合で仕様を変更することはありません
- 実装と仕様に齟齬がある場合、**実装を修正してください**。齟齬が一時的なもの（実装予定だが未着手など）であれば、コード内にTODOコメントを記すか、他の適切なドキュメントに記載してください
- **定義書確定後の設計変更についても、実装の都合で仕様を変更することはありません**。余程の事態（言語仕様的に実装が困難など）がない限り、実装を仕様に合わせて修正してください
- 各セクションの「確定事項」は、要件の綿密な確認と合意形成プロセスを経た設計決定です
**今後のバージョンで実装予定の機能について**:
本仕様に記載されている「今後のバージョンで実装予定」の機能は、現在の設計に拡張性を持たせることを要件とします。これらの将来機能は、各機能の該当する章に分散して記載されています。将来機能の実装時には、該当する章の記載に従って実装してください。

**コードブロックについて**:
- 本ドキュメント内のコードブロックは、**設計意図・API構造・使用例を伝えるための例示**です。網羅的な実装を示すものではありません
- コードブロックに記載されていない部分については、実装時に適切に実装してください
- **コードブロック以外の本文（型定義、動作仕様、制約条件など）は遵守してください**
- コードブロック内の省略は、**関数の引数の一部だけを省略するなど分かりにくい形では行いません**。省略する場合は関数全体を省略するか、明確にコメントで示してください

---

## 1. 概要・設計方針

### 1.1 目的

本ドキュメントは、従来のPython/Tkinter UIを置き換えるPoke-Controller Modified Extensionの新しいWeb/デスクトップUIの要件を規定します。本仕様は、リファクタリングセッション中に伝達された**過去のユーザー要件のみ**から導出されており、現在のコードベースからは導出されていません。

### 1.2 設計方針

- **Tkinterとの機能・視覚的パリティ**: 新しいUIは、元のTkinterの機能・レイアウト・外観に厳密に一致する必要があります。ただし、本ドキュメントで明示的に変更された項目（§4等）は除きます。レイアウト、色、ボタンの間隔、ウィジェットの種類はオリジナルに準拠する必要があります。
- **スクリプト互換性**: §4.6を参照。リファクタリング前のバージョンで動作していたすべてのスクリプトは、引き続き正常に動作する必要があります。
- **モダンスタック**: SvelteKit + Svelte 5（Runesモード）+ **Tailwind CSS v4**（確定、変更不可）。
- **低遅延通信**: プライマリとしてWebRTC、フォールバックとしてビデオ: WebCodecs + WebSocket、DataChannel: WebSocketを使用。WebSocketは切断時に3秒ごとに自動再接続。
- **型安全性**: Rustコアから `utoipa` v5 + `openapi-typescript` を介してOpenAPI生成のTypeScript型を使用。
- **認証なし**: アプリケーションはローカル/LAN専用に設計。API認証は不要。

**実装レイヤー**:

| レイヤー | 言語 | 役割 | 例 |
|---------|------|------|-----|
| Rustコア | Rust | メインプロセス、すべてのコア処理 | イベントバス、シリアル通信、画像処理、Python実行環境管理 |
| PyO3バインディング | Rust（Pythonに公開） | Python API提供 | `pokecon.event`, `dialogue` |
| Python互換レイヤー | Python 3.12～3.14互換（最小限） | 将来の実装切り替え用フック | `CommandMeta`（`_meta.py`のみ） |
| Luaランタイム | LuaJIT 2.1 | 動的設定（`init.lua`）の実行 | `pokecon.autocmd` |

**Python実行方式**:
RustコアはPython実行環境を管理し、Python側にはPyO3の`#[pymodule]`/`#[pyclass]`/`#[pyfunction]`でRust APIを公開する。Pythonスクリプト内では `import pokecon` によりRust APIへアクセスする。

**Python実行環境の分離方針**:

1. ユーザースクリプト用Pythonと動的設定用Pythonは、実行環境を分離する。
2. 単一のCPythonインタープリター内で名前空間だけを分ける論理分離は採用しない。
3. 分離手段は、CPythonサブインタープリター（`Py_NewInterpreterFromConfig`）または同等の独立性を確保できるPython実行単位を候補とし、実装方式は設計フェーズで確定する。
4. 片方の `sys.path`、モジュール読み込み、グローバル状態の変更が、もう片方に影響しないことを保証する。
5. Pythonオブジェクトを実行環境間で直接共有しない。必要な状態共有はRustコアが管理するAPI境界を経由する。
6. 別プロセス方式は必須前提ではない。採用する場合は内部実装詳細とし、ユーザーに見える `pokecon` APIの挙動を変えない。
7. 動的設定でPython（`init.py`）を使用しない場合、動的設定用Python実行環境は生成しない。

**言語仕様**:
- **Python**: ランタイムは3.14を使用。コードは3.12～3.14で動作するよう記述し、現在公開されている非推奨・廃止予定の機能は避ける。例外を除き厳格な型注釈を必須とする。PEP 695型パラメータ、basedpyrightによる厳格な型チェックを使用
- **Lua**: LuaJIT 2.1をターゲット。動的設定用のスクリプト言語として使用

**注**: ユーザースクリプトや動的設定（`init.py`/`init.lua`）から呼び出されるAPIは、原則としてPyO3（Rust製）で実装される。Pythonファイル（`commands.py`, `events.py`等）は型注釈・ドキュメント・互換レイヤーのみを提供し、実際の処理はRust側で行う。PyO3で同一プロセス内に埋め込む場合、Python→RustのAPI呼び出しは直接的な関数呼び出しとして行われる。実装上の分離方式が異なる場合でも、その内部境界はユーザー向けAPIに露出しない。

**Pythonランタイム設定**: ユーザーが`settings.toml`で指定したPython実行環境（システムPythonまたは仮想環境）を使用できる。指定がない場合はデフォルトの3.14ランタイムを使用。

**Pythonインタープリターの提供方法**:
- **動的リンク方式**: PyO3は実行時に`libpython3.x.so`を動的にリンクする。これがPyO3の標準的・推奨される方法である
- **nix環境**: nix storeのPythonパスをビルド時に決定し、アプリケーションに組み込む。再現性が保証される
- **非nix環境**: python-build-standaloneが配布するPythonを自動ダウンロードし、使用する。これにより、実行環境にシステムPythonがインストールされていなくても動作する
- **venv**: いずれの環境でも、venvを作成して使用する。ユーザースクリプト用と動的設定用で別々のvenvを使用可能
- **ユーザー指定**: `settings.toml`でユーザーが任意のPython実行環境を指定可能。指定がある場合はそれを優先する

**開発ワークフロー**:
- **nix-first**: 本プロジェクトはnix flakeを使用して開発する。すべての開発タスク（ビルド、テスト、型チェック、フォーマット）は`nix run .#<task>`または`nix develop`内で実行する
- **nix環境優先、非nix環境もサポート**: まずnix環境で動作するよう実装し、その後非nix環境（Windows含む）でも動作するよう調節する。非nix環境では`PythonManager`がPythonインタープリターのセットアップを管理する（§14.4参照）

### 1.3 対象プラットフォーム

| プラットフォーム | UIモード | 備考 |
|----------|---------|-------|
| デスクトップ（Windows/Linux） | Tauri（WebViewラッパー） | Webモードとaxum HTTPサーバーを共有 |
| Webブラウザ | スタンドアロンSvelteKit SPA | axum HTTPサーバーによって提供 |
| モバイル（将来） | レスポンシブSPA | 同一コードベース、アダプティブレイアウト |

**注**: macOSは現時点では対象外。TauriのWebKit/GTK依存によるCI問題（AGENTS.md参照）により、macOS対応は現在のスコープ外とし、将来のバージョンでの判断とする。

### 1.4 対象外機能

以下の機能はメインブランチに実装されているが、本リファクタリングでは**実装しない**:

- **キーボードショートカット機能**: メインブランチのキーボードショートカット（キーマップ）機能は実装しない。ショートカットボタン（10ボタン）は実装するが、キーボード入力によるショートカット割り当ては行わない

---

## 2. 用語集

| 用語 | 定義 |
|------|------|
| **フラット構造** | ドット区切りの階層を持たない、単一レベルの属性アクセス方式。例: `pokecon.opt.camera_fps`（フラット）vs `pokecon.opt.camera.fps`（階層） |
| **Neovim/Vim風** | Neovim/Vimエディタの設定方式を模した設計。イベント名の`Pre`/`Post`後置（`BufReadPre`/`BufReadPost`に類似）、キー記法の`<C-a>`形式等 |
| **後勝ち** | 同じキー・設定に対して後から適用された値が優先される方式。設定の優先順位で使用 |
| **動的設定** | 実行時に評価される設定ファイル（`init.py`/`init.lua`）。イベントハンドラ登録やカスタムロジックを含む |
| **静的設定** | 起動時に読み込まれる設定ファイル（`settings.toml`）。TOML形式で、グローバル設定やプロファイル管理を含む |
| **Pre/Postフェーズ** | イベントの実行前（Pre）と実行後（Post）の2つのフェーズ。イベント名に`Pre`/`Post`を後置して区別 |
| **XDG Base Directory** | Linux/Unix系の設定・データ・キャッシュディレクトリの標準規格。`~/.config/`（XDG_CONFIG_HOME）、`~/.local/share/`（XDG_DATA_HOME）等 |
| **HandlerId** | イベントハンドラの登録時に返される識別子。ハンドラの解除（`off()`）に使用。型: `int` |
| **フォールバック** | プライマリ方式が利用できない場合に使用される代替方式。例: WebRTC不可時のWebSocketフォールバック |
| **デッドゾーン** | アナログスティック等の入力デバイスにおいて、中央付近の微小な入力を無視する領域 |
| **チャタリング** | 機械的な接点のバウンスにより、意図しない短時間の連続入力が発生する現象 |
| **シグナリング** | WebRTCにおいて、通信相手との接続確立に必要な情報（SDP、ICE candidate等）を交換するプロセス |
| **holdEndSkip** | ボタンホールド中にShift+クリック（またはShift+タッチ終了）を行うことで、バックエンドに解放シグナルを送信せずに視覚的なボタン状態のみをリセットする操作 |
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
## 3. 非機能要件

### 3.1 パフォーマンス

| 指標 | 目標 |
|--------|--------|
| ビデオ遅延（WebRTC） | < 100ms |
| ビデオ遅延（Motion JPEG + WebSocketフォールバック） | 50-150ms |
| コントローラー入力遅延 | < 50ms |
| UI応答性 | 60 FPSアニメーション、< 16ms入力応答 |

### 3.2 アクセシビリティ

- すべてのコントロールのキーボードナビゲーション。
- スクリーンリーダー用のARIAラベル。
- ハイコントラストモードのサポート。

### 3.3 ブラウザサポート

| ブラウザ | 最小バージョン |
|---------|----------------|
| Chrome/Edge | 94以上（WebCodecs対応のため） |
| Firefox | 130以上（WebCodecs対応のため） |
| Safari | 16.4以上（WebRTC対応。WebCodecs VideoはSafari 16.4+で対応） |

### 3.4 WebSocket自動再接続

- 接続断時に、自動的に再接続を試行します。
- 再接続間隔、リトライ回数上限は設定で変更可能（§11.4参照）。
- デフォルト値: 3秒ごとに試行、リトライ回数上限20回。上限到達後は手動再接続を促すUI表示。

---

## 4. 主要ユーザー要件と却下事項

このセクションでは、仕様を上書きまたは明確化する明示的なユーザー指示を記録します。

### 4.1 ショートカットボタン — 10個（4個ではない）

> **要件**: Commandsタブには正確に**10個**のショートカットボタンが必要です（4個ではありません）。これは元の数から明示的に変更されました。

### 4.2 実行制御 — 開始/一時停止/再開/停止（開始/停止だけではない）

> **要件**: 実行制御ボタンには**開始、一時停止、再開、および停止**を含める必要があります（開始と停止だけではありません）。一時停止は再開可能でなければなりません。

### 4.3 LINE通知 — UI削除

> **要件**: LINE通知UIは完全に削除されました。Discord Webhookのみがサポートされます。
>
> **メニュー項目について**: LINE Token Assignment / LINE Token Check のメニュー項目は、旧UI互換のためメニュー構造に残存します。これらのメニュー項目を選択した際の挙動は `settings.toml` で切り替え可能です:
> - **既定**: 削除済みである旨のメッセージを表示（`line_menu_behavior = "message"`）
> - **No-op**: 何も起こらない（`line_menu_behavior = "noop"`）

### 4.4 設定ファイル — `settings.ini` 廃止

> **要件**: 従来の `settings.ini` は廃止されました。すべての設定は `settings.toml` に移行されました。
- 静的設定: `settings.toml`
- 動的設定: `init.py`/`init.lua`

### 4.5 PWA — 今後のバージョンで実装予定

> **要件**: PWAは今後のバージョンで実装予定です。

### 4.6 スクリプト互換性 — リファクタリング前の全スクリプトが動作必須

> **要件**: リファクタリング前のバージョンで動作していたすべてのスクリプトは、引き続き正常に動作する必要があります。スクリプトAPIに破壊的変更は加えません。

互換性確認の対象は、少なくとも以下の3系統で読み込み可能なユーザースクリプトです。

- 現在のフォークのメインブランチ
- フォーク元である Extension のデフォルトブランチ
- Extension の fork 元である modified のデフォルトブランチ

具体的な互換性テスト対象や参照コミットを固定する場合は、
実装計画または検証計画で別途明記します。

### 4.7 テーマサポート — 今後のバージョンで実装予定

> **要件**: テーマサポート（ライト/ダーク/カスタム）は今後のバージョンで実装予定です。詳細は§9を参照。Tailwind CSS v4はスタイリングフレームワークとして確定しています。

---

## 5. UIレイアウト

### 5.1 全体構造

```
+-------------------------------------------------------------+
|  メニューバー（構成は§11.5.7.1参照）                         |
|-------------------------------------------------------------|
|  +----------------------+  +-----------------------------+  |
|  |                      |  |                             |  |
|  |   タブコンテンツ領域   |  |   右側パネル               |  |
|  |   (Notebook/TabView)  |  |                             |  |
|  |                      |  |  +-----------------------+  |  |
|  |  [カメラ]             |  |  | ソフトウェアコントローラー|  |  |
|  |  [シリアル]           |  |  | (Joy-Conレイアウト)   |  |  |
|  |  [手動制御]           |  |  +-----------------------+  |  |
|  |  [コマンド]           |  |                             |  |
|  |  [通知]               |  |  +-----------------------+  |  |
|  |  [その他]             |  |  | 出力#1                |  |  |
|  |                       |  |  +-----------------------+  |  |
|  |                       |  |                             |  |
|  |                       |  |  +-----------------------+  |  |
|  |                       |  |  | 出力#2                |  |  |
|  |                       |  |  +-----------------------+  |  |
|  +----------------------+  +-----------------------------+  |
+-------------------------------------------------------------+
```

メニューバーはタブ領域・右側パネルと同じトップレベルUI要素として扱う。
具体的なメニュー構成は§11.5.7.1に定義する。

### 5.2 タブ構造（6メインタブ + 3サブタブ）

> **タブ数に関する注記**: トップレベルに**6つのメインタブ**、Commandsタブ内に**3つのサブタブ**（Python Command、Mcu Command、Shortcut）を持つ。合計9つのタブ付きインターフェース。

| # | タブ名 | 説明 |
|---|--------|-------------|
| 1 | **カメラ** | 映像表示（Canvas/CaptureArea）、デバイス選択、FPS、フリップ、表示モード切替、マウスベースのスティック制御、スクリーンショット |
| 2 | **シリアル** | COMポート選択、ボーレート、データ形式（3種類）、接続/切断、シリアルモニター |
| 3 | **手動制御** | ソフトウェア制御（キーボード、マウススティックエミュレーション）、ハードウェア制御（ProController/Xinput、録画 ※現バージョンでは非表示）、完全なJoy-ConレイアウトのSwitch Controller Simulator |
| 4 | **コマンド** | 3サブタブ（Python Command、Mcu Command、Shortcut）、タグフィルター付きコマンドリスト、10ショートカットボタン、実行制御（開始/一時停止/再開/停止/再読み込み） |
| 5 | **通知** | Windows通知設定、Discord Webhook（URL、ユーザー名、アバター）、LINE UIは完全に削除 |
| 6 | **その他** | 出力サイズ調整、stdout出力先、ウィジェットモード選択、ソフトウェアコントローラー位置、ダイアログボタン位置、出力クリア |

### 5.3 右側パネル

#### 5.3.1 ソフトウェアコントローラー（Joy-Conレイアウト）

- **位置**: 右パネル内でTOP/BOTTOMを設定可能（その他タブのラジオボタンで選択）。
- **外観**: Joy-Con L（シアン `#56CCF2`）+ R（赤 `#E9514E`）レイアウト。
- **アクティブ色**: ボタンが押されている/保持されている間は黄色 `#FFD800`。
- **入力方法**:
  - **ホールド**: ポインター押下（`mousedown` / タッチ開始）でボタンホールドをトリガー（押下シグナル送信）。
  - **解放**: ポインター解放（`mouseup` / タッチ終了）でボタン解放をトリガー（解放シグナル送信）。
  - **Shift+解放**: `holdEndSkip` をトリガー — バックエンドに解放シグナルを送信せずに視覚的状態のみを切り替え。用途: ボタンを押したままの状態で別の操作を行いたい場合（例: Aボタン長押し中に別のボタンを短押し）。アプリケーション終了時やプロファイル切替時には、holdEndSkip中のボタンも含めて全てのボタンを強制解放する
  - **ブラウザ対応**: Shift+クリックによるブラウザのデフォルト動作（テキスト選択等）を防ぐため、`event.preventDefault()` を使用する
- **ボタン**: すべての標準Switchコントローラーボタン:
  - A、B、X、Y
  - L、R、ZL、ZR
  - MINUS（−）、PLUS（+）
  - HOME、CAPTURE
  - LCLICK（左スティック押し込み）、RCLICK（右スティック押し込み）
  - D-pad（上、下、左、右）
  - Lスティック（アナログ、0～255座標）
  - Rスティック（アナログ、0～255座標）
  - タッチスクリーンシミュレーション（320×240座標入力、0-based: 0～319, 0～239）
- **アナログスティック**: X軸およびY軸ともに0～255の範囲。中央位置にはデッドゾーンあり（値103～153はニュートラルとして扱われる）。マウスドラッグ中は`requestAnimationFrame`に同期して状態を確認し、前回送信から16ms以上経過している場合のみ送信する。無操作時は送信停止。

#### 5.3.2 出力パネル

- **出力#1（上部ログパネル）**: プライマリログ/出力表示。
- **出力#2（下部ログパネル）**: セカンダリログ/出力表示。
- **サイズ調整**: その他タブの「出力サイズ調整」スライダー（0～100）で制御。出力#1と出力#2の比率を決定。
- **ログソース**: バックエンドからWebRTC DataChannel（プライマリ）またはWebSocket（フォールバック）経由で受信したログ。WebRTC DataChannelが利用可能な場合はそちらを優先し、接続断時はWebSocketにフォールバック。
- **機能**: 自動スクロール、クリアボタン、クリップボードにコピー、ログレベルフィルタリング。
- **ログレベル**: DEBUG、INFO、WARNING、ERROR、CRITICAL（フィルタリング用）。各レベルの基準:
  - **DEBUG**: 開発時の詳細情報（関数呼び出し、内部状態変化）
  - **INFO**: ユーザーに知らせるべき通常の動作（コマンド開始/終了、接続確立）
  - **WARNING**: 問題の可能性があるが動作は継続（設定の非推奨項目使用、フォールバック発動）
  - **ERROR**: 動作に影響する問題（接続失敗、コマンド実行エラー）
  - **CRITICAL**: 致命的な問題（ハードウェア異常、データ破損）
- **独立ボタン**: その他タブの「出力をクリア」ボタン。

### 5.4 サブタブ構造（Commandsタブ）

Commandsタブには3つのサブタブがあります:

| # | サブタブ | 説明 |
|---|---------|-------------|
| 1 | **Python Command** | 利用可能なPythonコマンドスクリプトのリスト/ツリー |
| 2 | **Mcu Command** | 利用可能なMCUコマンドスクリプトのリスト/ツリー |
| 3 | **Shortcut** | 10ショートカットボタン割り当てグリッド |

### 5.5 ウィジェットモード（7種類）

UIは、その他タブのコンボボックスで選択可能な、右側パネルの7つの表示組み合わせをサポートする必要があります。
設定値（`settings.toml`の `widget_mode`）は以下の文字列をそのまま使用します:

| モード | 設定値（内部識別子） | ソフトウェアコントローラー | 出力#1 | 出力#2 | 説明 |
|------|--------------------|---------------------|-----------|-----------|-------------|
| 1 | `ALL (default)` | 表示 | 表示 | 表示 | フルパネル（デフォルト） |
| 2 | `Output#1 + Output#2` | 非表示 | 表示 | 表示 | 出力のみ |
| 3 | `Output#1 + Software-Controller` | 表示 | 表示 | 非表示 | 出力#1＋コントローラー |
| 4 | `Output#2 + Software-Controller` | 表示 | 非表示 | 表示 | 出力#2＋コントローラー |
| 5 | `Output#1 Only` | 非表示 | 表示 | 非表示 | 出力#1のみ |
| 6 | `Output#2 Only` | 非表示 | 非表示 | 表示 | 出力#2のみ |
| 7 | `Software-Controller Only` | 表示 | 非表示 | 非表示 | コントローラーのみ |

---

## 6. タブ仕様

### 6.1 カメラタブ

#### 6.1.1 映像表示

- **表示方法**: 映像レンダリング用のCanvas要素（旧API互換性のため `CaptureArea` としても参照可能）。
- **プライマリストリーム**: WebRTCビデオトラック（低遅延）。
- **フォールバック**: WebRTCが利用できない場合、WebCodecs + WebSocket（ブラウザネイティブHWデコード）。
- **フレームレート**: FPS設定（コンボボックス）で設定可能。

#### 6.1.2 カメラ設定

| コントロール | 種類 | 説明 |
|---------|------|-------------|
| **カメラデバイス選択** | コンボボックス | 利用可能なカメラデバイスのドロップダウン |
| **FPS** | コンボボックス | UI表示用フレームレート（`ui_fps`、選択肢: 5, 15, 30, 60）。バックエンド処理FPS（`camera_fps`）とは独立。選択肢は静的設定でカスタマイズ可 |
| **フリップ** | Checkbox | 水平/垂直フリップ切替 |

#### 6.1.3 表示モード切替（チェックボックス）

| モード | 説明 |
|------|-------------|
| **リアルタイム** | ライブ映像表示 |
| **ピクセル値** | カーソル位置のピクセル値（RGB/HSV/座標）をオーバーレイ表示 |
| **ガイド** | 構図用グリッド線やテンプレートマッチングの基準線をオーバーレイ表示 |

これらは表示オーバーレイ切替用のチェックボックスです。

#### 6.1.4 キャンバス上のマウス操作

カメラキャンバス（CaptureArea）は以下のマウス操作をサポートします:

| アクション | トリガー | 動作 |
|--------|---------|----------|
| **Lスティック/Rスティック制御** | キャンバス上でマウスドラッグ | ドラッグ方向/距離に基づいて左/右アナログスティック移動をエミュレート |
| **カラーピッカー** | Ctrl+クリック | クリック位置の色の値を取得 ※ブラウザのコンテキストメニューと競合する可能性あり。対応例: `event.preventDefault()` の使用 |
| **範囲スクリーンショット** | Ctrl+Shift+ドラッグ | キャンバス上の選択した矩形領域のスクリーンショットをキャプチャ ※ブラウザのテキスト選択と競合する可能性あり。対応例: `event.preventDefault()` の使用 |
| **名前付き保存** | Ctrl+Alt+ドラッグ | 選択した領域をファイル保存ダイアログで保存。初期ファイル名は `capture_YYYYMMDD_HHMMSS.png` ※ブラウザのショートカットと競合する可能性あり（特にLinux/ChromeでOSレベルのウィンドウ移動に使用される場合）。対応例: `event.preventDefault()` の使用、またはユーザー設定で別のキーコンボに変更可能 |

#### 6.1.5 スクリーンショットキャプチャ

- **保存場所**: `./Captures/` ディレクトリ。
- **形式**: PNG/JPEG（選択可能）。

#### 6.1.6 カメラバックエンド

- **バックエンド**: OpenCV。プラットフォームに応じた適切なバックエンドを自動選択。
- **非同期キャプチャ**: フレームキャプチャはUIスレッドをブロックしない方式で実行。

### 6.2 シリアルタブ

#### 6.2.1 接続制御

| コントロール | 種類 | 説明 |
|---------|------|-------------|
| **COMポート選択** | コンボボックス | 利用可能なCOM/シリアルポートのドロップダウン |
| **更新ボタン** | Button | 利用可能なポートを再スキャン |
| **接続/切断** | Toggle button | 選択したポートに接続または切断 |

#### 6.2.2 設定

| 設定 | オプション | デフォルト |
|---------|---------|---------|
| **ボーレート** | 9600 / 115200 | 9600 |
| **データ形式** | デフォルト / Qingpi / 3DS Controller | デフォルト |

**デフォルト形式**:
- ボーレート: 9600
- ワイヤーフォーマット: テキストベース `0x###### H [lx ly] [rx ry]\r\n`
  - `0x######` = ボタンマスク（6桁16進数）
  - `H` = ハット（方向キー）
  - `[lx ly]` = 左スティックX,Y（条件付き送信）
  - `[rx ry]` = 右スティックX,Y（条件付き送信）
  - スティックデータは `l_stick_changed`/`r_stick_changed` フラグがセットされた場合のみ送信

**Qingpi形式**:
- ボーレート: 9600
- ワイヤーフォーマット: バイナリ、11バイト固定長

**3DS Controller形式**:
- ボーレート: 115200
- ワイヤーフォーマット: バイナリ、6バイト固定長

#### 6.2.3 シリアルモニター

- **コンポーネント**: スクロールバー付きテキストウィジェット。
- **機能**: リアルタイムで入出力シリアルデータを表示。最新エントリへの自動スクロール、クリアボタン。

### 6.3 手動制御タブ

#### 6.3.1 ソフトウェア制御セクション

| コントロール | 種類 | 説明 |
|---------|------|-------------|
| **キーボード** | Checkbox | キーボードベースのコントローラー入力を有効化（グローバルホットキー） |
| **Lスティックマウス** | Checkbox | キャンバス上の左アナログスティックのマウスエミュレーションを有効化 |
| **Rスティックマウス** | Checkbox | キャンバス上の右アナログスティックのマウスエミュレーションを有効化 |

#### 6.3.2 ハードウェア制御セクション

> **ステータス: 今後のバージョンで実装予定**
>
> 現バージョンではハードウェア制御セクションを**非表示**とします（グレーアウトではなく非表示）。

#### 6.3.3 Switch Controller Simulator

タブコンテンツ領域に表示される完全なJoy-Conスタイルのボタンレイアウト。ボタン構成は§5.3.1と同じ。

### 6.4 Commandsタブ

#### 6.4.1 サブタブ構造とコマンドリスト

Commandsタブのサブタブ構造については §5.4 を参照。

##### 6.4.1.1 コマンドリスト

- **表示**: 利用可能なコマンドを表示するTreeview（階層構造を持つコマンドリスト）。
- **タグフィルター**: ラベル/タグでコマンドをフィルタリングするドロップダウンまたはコンボボックス。
- **列**: コマンド名、タグ、説明（Treeviewの場合）。

##### 6.4.1.2 タグ体系

タグはコマンドの分類・フィルタリングに使用されるメタデータです。

**CommandInfo構造体**（Python側の型定義。実際の実装はRustのPyO3バインディングで行われる）:
```python
class CommandInfo:
    name: str           # コマンド名（NAME属性）
    module_path: str    # モジュールファイルパス
    class_name: str     # クラス名
    tags: list[str]     # 統合後のタグ一覧（自動+手動+動的）
```

**自動タグ（ディレクトリ由来）**:
- Pythonモジュールのパスから自動生成（中間ディレクトリ名を `@` プレフィックス付きで抽出）
- 例: `Commands.PythonCommands.Samples.RankGlitch.MashA` → `["@Samples", "@RankGlitch"]`
- ネストしたディレクトリ構造に対応
- 元のディレクトリ名をそのまま使用（PascalCase変換なし）

**手動タグ（クラス属性）**:
- コマンドクラスの `TAGS` クラス属性で定義
- `list[str] | None` で指定可能
- `None` の場合は「タグなし」（空リストとして扱う）
- `@` プレフィックスは付かない（慣例）

**動的タグ（イベントによる追加）**:
- `ScriptLoadPre` イベントのコールバックで動的タグ追加が可能
  （イベント定義は§11.5.6.1.5、具体例は§11.5.4参照）
- 詳細なAPI仕様は§11（設定ファイルシステム）を参照

**タグ統合の例**:

```text
ディレクトリ構造:
Commands/
  PythonCommands/
    Samples/
      RankGlitch/
        MashA.py          # クラス TAGS = ["Rank"]
        MashB.py          # クラス TAGS = None
    Tournaments/
      Battle.py           # クラス TAGS = ["Battle", "Online"]
```

統合結果:
| コマンド | 自動タグ | 手動タグ | 動的タグ | 最終タグ |
|---------|---------|---------|---------|---------|
| MashA | @Samples, @RankGlitch | Rank | — | @Samples, @RankGlitch, Rank |
| MashB | @Samples, @RankGlitch | — | @Auto（動的追加） | @Samples, @RankGlitch, @Auto |
| Battle | @Tournaments | Battle, Online | — | @Tournaments, Battle, Online |

1. 自動タグ（ディレクトリ由来、`@` プレフィックス付き）
2. 手動タグ（クラス属性、`@` なし）
3. 動的タグ（イベントコールバックによる追加）

**タグの書き戻し**:
- 統合後のタグは、ユーザースクリプトからの参照とバックエンド処理の両方で使用可能な状態で保持される
- Pythonクラス属性と内部データ構造の両方に反映される

**タグの重複**:
- 同じタグが複数回追加された場合、自動的に重複を除去
- 統合後のタグ一覧はユニークなリストとなる

- **フィルター動作**:
  - デフォルトは完全一致（`selected_tag == tag`）
  - マッチング方式は設定で切り替え可能:
    - **静的設定** (`settings.toml`): `exact`（完全一致） / `partial`（部分一致） / `prefix`（前方一致） / `suffix`（後方一致）
    - **動的設定** (`init.py`/`init.lua`): カスタムマッチ関数を指定可能（§11.5.4参照）
  - **バックエンド側の責務**: タグフィルターのマッチング（完全一致/部分一致/前方一致/後方一致/カスタム関数）。マッチング結果はコマンドリストの表示/非表示を制御
  - **フロントエンド側の責務**: ファジーファインダーによる絞り込み（`fuse.js` を使用した部分一致スコアリング）。これはUI上の利便性向上のための補助機能であり、バックエンドのマッチング方式とは独立して動作する。フロントエンドの絞り込みはバックエンドのマッチング結果に対してさらにフィルタをかける2段階方式
  - UI上では `@` なしタグが先、`@` 付きタグが後に表示
  - ソート関数は動的設定ファイルで指定可能（§11.5.4参照）
  - 先頭に `"-"`（フィルター無効）を配置

**タグ専用のstate**:
- `pokecon.state.tags`: タグ一覧のみ（UI表示、フィルター選択肢生成用）
  - 型: `list[str]`
- `pokecon.state.command_candidates`: コマンド候補 + タグリスト（動的タグ追加、フィルタリング、実行用）
  - 型: `list[CommandInfo]`
- タグとコマンドの紐づけはコマンド側で管理

#### 6.4.2 ショートカットボタン（10ボタン）

- **数**: 10ショートカットボタン（要件が元の4ボタンから10ボタンに変更）。
- **割り当て**: 読み込まれた任意のコマンドにユーザー割り当て可能（クリックで割り当て）。
- **割り当て解除**: Shift+クリックで割り当てを解除。
- **クリア**: 右クリックで割り当てをクリア。
- **表示**: ボタンラベルに割り当てられたコマンド名を表示。
- **ショートカット割り当て方法**: ショートカットボタンをクリック → コマンドリストからコマンドを選択 → 割り当て完了。Shift+クリックで割り当て解除。右クリックでクリア
- **保存**: 設定はバックエンド側のローカルストレージ（`settings.toml` の `[shortcuts]` セクション）に永続化される。アクティブプロファイルの `profiles/<name>/settings.toml` に `[shortcuts]` がある場合はそれを使用し、未設定項目は§11.3の優先順位に従ってグローバル設定から継承する。これによりブラウザを変えても同一の設定が利用可能で、プロファイル切替時にショートカット設定も連動する。

#### 6.4.3 実行制御ボタン

| ボタン | アクション |
|--------|------------|
| **開始** | コマンド実行を開始 |
| **停止** | 実行を停止 |
| **一時停止** | 実行を一時停止（再開可能） |
| **再開** | 一時停止から再開 |
| **コマンドリスト再読み込み** | ファイルシステムからコマンドリストを再読み込み（コマンドリロード） |

- **状態表示**: 実行中 / 一時停止中 / 停止 / エラー。

### 6.5 通知タブ

#### 6.5.1 Windows通知

| コントロール | 種類 | 説明 |
|---------|------|-------------|
| **スクリプト開始時に通知** | Checkbox | スクリプト実行開始時にWindows通知を送信 |
| **スクリプト終了時に通知** | Checkbox | スクリプト実行終了時にWindows通知を送信 |
| **テスト** | Button | 設定を確認するためのテスト通知を送信 |

#### 6.5.2 Discord通知

| コントロール | 種類 | 説明 |
|---------|------|-------------|
| **Webhook URL** | Text input | Discord Webhook URL。URL形式（`https://discord.com/api/webhooks/...`）の構文検証を行う |
| **ユーザー名** | Text input | Discordメッセージのカスタムユーザー名（オプション） |
| **アバターURL** | Text input | Discordメッセージのカスタムアバター画像URL（オプション） |
| **テスト** | Button | 設定を確認するためのテスト通知を送信 |

#### 6.5.3 LINE通知

- **ステータス**: サービス終了（EOL）— **通知タブからUIは完全に削除**。
- **後方互換性**: 既存のユーザースクリプト用にスクリプトAPI（`LINE_text()`、`LINE_image()`）は維持。設定用のUIはなし。

### 6.6 その他タブ

#### 6.6.1 設定グループ

| セクション | コントロール | 種類 |
|---------|----------|------|
| **出力サイズ調整** | 出力#1と出力#2の幅比率を制御するスライダー（0～100）。スライダー値は10%～90%の範囲にマッピングされ、最小でも各出力は10%の幅を確保 | Scale/Slider |
| **stdout出力先** | stdout出力の出力先を選択する出力#1/出力#2ラジオボタン | Radio button |
| **出力をクリア** | 両方の出力パネルをクリアするボタン | Button |
| **ウィジェットモード** | 7モードのコンボボックス（§5.5参照） | Combobox |
| **ソフトウェアコントローラーの位置** | 右パネル内の位置を指定するtop/bottomラジオボタン | Radio button |
| **ダイアログボタンの位置** | ダイアログボタン配置用のtop/bottom/bothラジオボタン | Radio button |

---

## 7. 通信プロトコル（Web / ネットワーク）

### 7.1 スタック概要

```
カメラ映像:     WebRTCビデオトラック ──→ Motion JPEG over WebSocket フォールバック
コントローラー入力: WebRTC DataChannel ──→ WebSocket フォールバック
ログ/イベント:  WebRTC DataChannel ──→ WebSocket フォールバック
API呼び出し:    HTTP REST（axum）     ──→ （フォールバック不要）
```

### 7.2 WebRTC（プライマリ）

- **ビデオ**: ビデオトラックを使用したWebRTC `RTCPeerConnection`。
- **DataChannel**: コントローラー入力イベントとログストリーミング用。
- **シグナリング**: WebSocket上のJSONメッセージでSDP Offer/Answer/ICE candidateを交換。STUNサーバーは`settings.toml`で設定可能（デフォルト値あり）。コーデック優先順位: H.264 > VP8 > VP9。
- **自動再接続**: §3.4参照。
- **フォールバック条件**:
  - WebRTC接続が5秒以内に完了しない → WebSocketフォールバック起動
  - 接続確立後、3秒間連続でフレーム/データが受信できない → WebSocketにフォールバック
  - フォールバック中のWebRTC復旧検出は行わない（手動再接続を促す）

**通信内容**:

| 種類 | 内容 | フォールバック |
|------|------|--------------|
| 映像 | WebRTCビデオトラック | Motion JPEG over WebSocket |
| コントローラー入力 | WebRTC DataChannel | WebSocket |
| ログ | WebRTC DataChannel | WebSocket |
| API呼び出し | HTTP REST | なし（HTTP必須） |

### 7.3 Motion JPEG + WebSocket（フォールバック）

#### 7.3.1 映像フォールバック — Motion JPEG

映像フォールバックとして、**Motion JPEG over WebSocket** を採用します。各カメラフレームをJPEGとしてエンコードし、WebSocket経由で個別のバイナリメッセージとして送信します。

- **エンコーダー**: サーバーサイドで各フレームをJPEGにエンコード。品質パラメータは `settings.toml` で設定可能（デフォルト: 85、範囲: 1-100）
- **転送**: WebSocket経由で1フレーム=1バイナリメッセージとして送信。各フレームは自己完結したJPEGであり、フレーム間依存性がない
- **デコード**: ブラウザ標準のJPEGデコーダを使用（WebCodecs等の特殊API不要）
- **描画**: デコード結果を Canvas に描画

**特性**:

| 項目 | 値 |
|------|------|
| 遅延 | 50-150ms |
| エンコード | JPEG（品質パラメータ設定可能） |
| フレーム独立性 | 各フレームは完全なJPEG。TCP輻輳でフレーム遅延が発生しても次フレームで即座に回復し、デコーダ状態が壊れない |
| ブラウザサポート | 全ブラウザ対応（JPEGは標準機能） |

**Motion JPEG採用の理由**:
- **フォールバック層の最優先事項は確実動作**: WebRTCが失敗する環境（UDPブロック、古いブラウザ等）でも確実に動作する必要がある
- **各フレーム独立**: TCP上のWebSocketでは、H.264等のフレーム間圧縮方式は貧弱なネットワークでTCPバッファにフレームが蓄積し遅延が累積する問題がある。Motion JPEGは各フレームが独立しているため、遅延が蓄積しない
- **全ブラウザ対応**: WebCodecs API（H.264 HWデコード用）はSafari 16.4+、Chrome 94+等の制限があるが、JPEGデコードは全ブラウザ標準機能
- **画像認識用途への適合**: 各フレームが完全なJPEGであり、ブロックノイズが発生しない。テンプレートマッチング等の画像認識デバッグ表示に適する

#### 7.3.2 コントロール/ログフォールバック — WebSocket

- **エンドポイント**: `/ws`。
- **メッセージ**: JSON形式。
- **自動再接続**: §3.4参照。
- **イベント**:

| イベント | 方向 | ペイロード |
|-------|-----------|---------|
| `camera.open` | サーバー → クライアント | カメラオープン通知（`{"device_id": str, "resolution": [int, int]}`） |
| `command.start` | サーバー → クライアント | コマンド実行開始通知 |
| `command.stop` | サーバー → クライアント | コマンド実行停止通知 |
| `command.error` | サーバー → クライアント | コマンド実行エラー詳細 |
| `serial.data` | サーバー → クライアント | シリアルポート受信データ |
| `ping` | 双方向 | キープアライブping |
| `pong` | 双方向 | キープアライブpong応答 |

> **注（ペイロード詳細の意図的な除外）**: 上記イベントのうち、`camera.open` 以外のペイロード詳細（JSONフィールド構造、エラーコード体系、シリアルデータのエンコード方式等）は**意図的に仕様書から除外**しています。これらはフロントエンド（TypeScript）とバックエンド（Rust）間の内部通信プロトコルであり、ユーザーが直接参照・操作するものではありません。実装時に両レイヤー間で合意形成し、安定した仕様として維持します。`camera.open` のみ、UI側でカメラ解像度等を知るためにユーザーが間接的に依存する情報を含むため、ペイロード構造を公開しています。

**注**: WebSocketイベントは**バックエンドとフロントエンドSPA間の内部通信プロトコル**です。ユーザー（スクリプト作者や動的設定ファイルの作者）に露出しません。動的設定イベント（§11.5.6.1）のみがユーザーが使用できるイベントシステムです。

### 7.4 HTTP REST API

- **フレームワーク**: axum（Rustコア）。
- **ドキュメント**: OpenAPI仕様を使用したutoipa v5。
- **コード生成**: TypeScriptクライアント型用の `openapi-typescript`。生成失敗時は前回成功時の生成結果をフォールバックとして使用（git追跡）。CIでは型生成ジョブが独立して失敗することを許容し、アラートのみ行う
- **認証**: なし（ローカル/LAN専用）。
- **セキュリティ**: Originヘッダーの検証または同一発行元ポリシー（Same-Origin）による保護。CORS設定: `Access-Control-Allow-Origin` は `http://localhost:8020` のみ許可（デフォルト）。Tauriデスクトップモードでは `tauri://localhost` も許可。SvelteKit開発サーバー（`http://localhost:5173`）は `web/vite.config.ts` の `/api`・`/ws` proxy 経由で `127.0.0.1:8020` に接続し、バックエンド側のCORS許可originを増やさない
- **モジュール**: 複数のモジュールに分かれたREST API。
- **応答形式**: 一貫した構造のJSON。

**注**: WebSocketイベント名はUIとバックエンド間の内部通信プロトコルとして使用され、ユーザーが直接使用することはありません。命名規則は実装時に統一されます。

### 7.5 キーボード入力API

キーボード入力は低遅延が要求されるため、**WebRTC DataChannel**または**WebSocket**を使用。**HTTP RESTは使用しない**。

| 通信方式 | 用途 | フォールバック |
|---------|------|--------------|
| WebRTC DataChannel | プライマリ — キー入力イベント送信 | WebSocket |
| WebSocket | フォールバック — キー入力イベント送信 | なし |

**入力イベント形式**（WebSocket / DataChannel共通）:

```json
{
  "type": "keyboard_input",
  "key": "F5",
  "state": "pressed"
}
```

### 7.6 マウス入力API

マウス入力（スティック操作）は低遅延が要求されるため、**WebRTC DataChannel**または**WebSocket**を使用。**HTTP RESTは使用しない**。

| 通信方式 | 用途 | フォールバック |
|---------|------|--------------|
| WebRTC DataChannel | プライマリ — マウス/スティック入力イベント送信 | WebSocket |
| WebSocket | フォールバック — マウス/スティック入力イベント送信 | なし |

**入力イベント形式**（WebSocket / DataChannel共通）:

```json
{
  "type": "mouse_stick_input",
  "stick": "LSTICK",
  "x": 128,
  "y": 128
}
```

```json
{
  "type": "mouse_input",
  "button": "left",
  "state": "pressed",
  "x": 100,
  "y": 200
}
```

**設定取得/変更**（HTTP REST — 設定変更のみ）:

| メソッド | エンドポイント | 説明 |
|--------|----------|-------------|
| GET | `/api/controller/mouse_stick?stick=LSTICK|RSTICK` | マウススティック設定を取得 |
| POST | `/api/controller/mouse_stick` | マウススティック設定を設定（stick、enabled、sensitivity） |

### 7.7 ゲームパッド入力API

ゲームパッド入力は低遅延が要求されるため、**WebRTC DataChannel**または**WebSocket**を使用。**HTTP RESTは使用しない**。

| 通信方式 | 用途 | フォールバック |
|---------|------|--------------|
| WebRTC DataChannel | プライマリ — ゲームパッド入力イベント送信 | WebSocket |
| WebSocket | フォールバック — ゲームパッド入力イベント送信 | なし |

**入力イベント形式**（WebSocket / DataChannel共通）:

```json
{
  "type": "gamepad_input",
  "button": "A",
  "state": "pressed"
}
```

```json
{
  "type": "gamepad_input",
  "stick": "LSTICK",
  "x": 128,
  "y": 128
}
```

```json
{
  "type": "gamepad_input",
  "touch": {
    "x": 160,
    "y": 120,
    "pressed": true
  }
}
```

**設定取得/変更**（HTTP REST — 設定変更のみ）:

| メソッド | エンドポイント | 説明 |
|--------|----------|-------------|
| GET | `/api/controller/type` | 現在のゲームパッドタイプ設定を取得 |
| POST | `/api/controller/type` | ゲームパッドタイプを設定（`gamepad_type: "ProController" | "Xinput"`） |

- **対応ボタン**: A、B、X、Y、L、R、ZL、ZR、MINUS、PLUS、HOME、CAPTURE。
- **十字キー（Hat）**: UP、DOWN、LEFT、RIGHT、TOP_RIGHT、BTM_RIGHT、BTM_LEFT、TOP_LEFT、CENTER。
- **アナログスティック**: 両軸とも0～255の範囲。
- **タッチスクリーン**: `{x: 0–319, y: 0–239}` 座標（0-based）。

---

## 8. 型システム

### 8.1 OpenAPI → TypeScript

- **ソース**: `utoipa` v5マクロを使用したRustコア。
- **生成**: `openapi-typescript` CLI。
- **出力**: `src/lib/api/openapi.ts`。
- **使用法**: すべてのAPI呼び出しとWebSocketメッセージは生成された型を使用する必要があります。

**ワークフロー**:

```bash
# 1. RustコアでOpenAPI JSONを生成（ビルド時に自動実行）
cargo build

# 2. openapi-typescriptでTypeScript型を生成（デフォルトポート8020）
npx openapi-typescript http://localhost:8020/api-docs/openapi.json -o src/lib/api/openapi.ts

# 3. フロントエンドで型を使用
import { paths, components } from '$lib/api/openapi.ts'
```

**自動化**: `package.json`の`generate:api`スクリプトとして登録。CIでは生成済みの型ファイルをコミット。

### 8.2 型安全性要件

- 厳格なTypeScript（`strict: true`）。
- API関連コードに `any` 型は不使用。
- 外部入力に対するZodまたは同等の実行時検証。

---

## 9. テーマサポート（今後のバージョンで実装予定）

> **ステータス: 未実装 — 今後のバージョンで実装予定。**
> Tailwind CSS v4がスタイリングフレームワークとして確定しています。テーマシステムの要件は以下に記録されています。

### 9.1 組み込みテーマ

- ライトテーマ。
- ダークテーマ。
- システム設定の自動検出。

### 9.2 カスタムテーマ（将来）

- ユーザー定義の配色。
- CSS変数ベースのテーマ。

---

## 10. コマンドクラス（ユーザースクリプト向け）

### 10.1 API公開対象者

本ドキュメントに記載するAPIは、**ユーザースクリプト**向けに公開されます。

| 対象者 | 説明 | 使用場所 | 公開範囲 |
|--------|------|----------|----------|
| **ユーザースクリプト** | 自動化スクリプトを作成・実行する一般ユーザー | `Commands/PythonCommands/` 以下のスクリプト | `PythonCommand` クラスのメソッド、モジュールレベルの関数 |

**重要**:
- ユーザースクリプト向けAPIは動的設定（`init.py`/`init.lua`）からも使用可能
- 内部実装の名前空間は本仕様で規定するものではない。ユーザーがアクセスできるAPI名のみを規定する

### 10.2 設計方針

- **コアはRust**: すべてのコア処理はRustで実装。Pythonは必要な部分のみ（ユーザースクリプトAPI、互換レイヤー）。
- **メタクラスによる切り替え**: `CommandMeta`が将来の実装切り替え用フックを提供。現状はすべてPyO3（Rustバインディング）に流れる。
- **後方互換性**: リファクタリング前のスクリプトは変更なしで動作する必要がある。
- **型注釈**: 新APIは動作する型注釈を持つ。旧APIは互換性のために保持され、新APIと同等の完成度・品質でメンテナンスされる（§10.6参照）。
- **内部実装の命名**: ユーザースクリプトに公開するAPI（新ダイアログAPI `show_dialog` 以外）は、互換性のため全く同じ名前でアクセスできる必要がある。アクセスできれば内部の命名は自由（妥当なものであれば）。

### 10.3 公開モジュール

**注**: 以下のモジュールはRust/PyO3で実装され、Pythonファイルは型注釈・ドキュメント・互換レイヤーのみを提供する。実際の処理はRust側で行われる。

| モジュール | 内容 | ユーザースクリプトでのインポート例 |
|-----------|------|------------------------------|
| `dialogue` | ダイアログ関数 | `from Commands import dialogue` |
| `image_proc` | 画像処理（Rust実装）。詳細は §10.4.3 ImageProcPythonCommand 参照 | `from Commands import image_proc` |
| `net` | Socket、MQTT、HTTPクライアント。`from Commands import net` でインポート可能な関数群。`PythonCommand` のメソッド（`self.socket_connect()` 等）と同じ機能を提供するが、クラス外から使用可能。詳細は §10.4.2 PythonCommand（Socketメソッド）参照 | `from Commands import net` |

**注**: イベントシステム（`pokecon.autocmd`, `pokecon.event`）は§11で定義される。

### 10.4 コマンドクラス

#### 10.4.1 クラス階層

```
Command (metaclass=CommandMeta)
├── PythonCommand
│   └── ImageProcPythonCommand
└── McuCommand
```

**注**: すべてのクラスは `CommandMeta` メタクラスを使用（付録B参照）。`PythonCommand` と `ImageProcPythonCommand` は抽象メソッド `do()` を持つが、Pythonの `ABC` クラスを継承しない（`CommandMeta` で抽象クラスとして扱われる）。ユーザースクリプトでは `PythonCommand` または `ImageProcPythonCommand` を継承して `do()` を実装する。

#### 10.4.2 PythonCommand

**インポート**: `from Commands.PythonCommandBase import PythonCommand`

**ライフサイクルメソッド**:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `do()` | `do() -> None` | 抽象 — 自動化ロジックをオーバーライド |
| `finish()` | `finish() -> None` | スクリプトを正常停止 |
| `checkIfAlive()` | `checkIfAlive() -> Literal[True]` | 停止フラグ確認。`self.alive` が `True` の場合は `True` を返す。`False` の場合は後処理（`keys` 解放、`postProcess` 実行）を行った上で `StopThread` 例外を送出し、コマンドスレッドを安全に終了させる |
**入力メソッド**:

**GamepadInput型**:
```python
type Buttons = Button | Hat | Direction | Touchscreen
type ButtonsList = list[Buttons]
type GamepadInput = ButtonsList | Buttons
```

| 型 | 基底クラス | 説明 |
|-----|----------|------|
| `Button` | `IntFlag` | 物理ボタン。`Y`, `B`, `A`, `X`, `L`, `R`, `ZL`, `ZR`, `MINUS`, `PLUS`, `LCLICK`, `RCLICK`, `HOME`, `CAPTURE`。3DS互換エイリアス: `SELECT=MINUS`, `START=PLUS`, `POWER=LCLICK`, `WIRELESS=RCLICK` |
| `Hat` | `IntEnum` | D-Pad方向。`TOP`, `TOP_RIGHT`, `RIGHT`, `BTM_RIGHT`, `BTM`, `BTM_LEFT`, `LEFT`, `TOP_LEFT`, `CENTER` |
| `Direction` | クラス（enumではない） | アナログスティックの角度/位置。`Direction(stick, angle, magnification=1.0, isDegree=True, showName=None)` で任意角度を構築可能。定義済みインスタンス: `UP`, `RIGHT`, `DOWN`, `LEFT`, `UP_RIGHT`, `DOWN_RIGHT`, `DOWN_LEFT`, `UP_LEFT`（左スティック）、`R_UP`, `R_RIGHT`, `R_DOWN`, `R_LEFT`, `R_UP_RIGHT`, `R_DOWN_RIGHT`, `R_DOWN_LEFT`, `R_UP_LEFT`（右スティック） |
| `Touchscreen` | クラス | タッチスクリーン座標。`Touchscreen(x, y)`。Qingpiプロトコルのみで使用 |

`GamepadInput` は単一値またはリストを受け付ける。リストの場合、同じカテゴリの複数項目を同時に送信可能（例: `[Button.A, Button.B]` でA+B同時押し）。

| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `press()` | `press(buttons: GamepadInput, duration: float = 0.1, wait: float = 0.1) -> None` | ボタン押下（指定秒数保持後解放） |
| `pressRep()` | `pressRep(buttons: GamepadInput, repeat: int, duration: float = 0.1, interval: float = 0.1, wait: float = 0.1) -> None` | 繰り返し押下 |
| `hold()` | `hold(buttons: GamepadInput, wait: float = 0.1) -> None` | ボタンを押下状態で保持 |
| `holdEnd()` | `holdEnd(buttons: GamepadInput) -> None` | 保持中のボタンを解放 |
| `wait()` | `wait(wait: float) -> None` | 指定時間待機。>0.1sは `time.sleep()`（CPU効率、低精度）、≤0.1sはビジーループ（`time.perf_counter()`、高精度、高CPU使用率）。いずれの場合も `checkIfAlive()` を呼び出す |
| `short_wait()` | `short_wait(wait: float) -> None` | 常にビジーループ待機（`time.perf_counter()`、高精度、高CPU使用率）。`checkIfAlive()` を呼び出す |
| `direct_serial()` | `direct_serial(commands: list[str], waittimes: list[float]) -> None` | 生シリアルコマンド送信。`commands` から `\r`/`\n` を除去し、`zip(waittimes, commands, strict=False)` で並列処理。各コマンド送信前に `waittimes[i]` 秒待機。長さ不一致時は短い側まで実行。`writeRow_wo_perf_counter()` で `\r\n` を付加して送信。`strict=False` は互換性のため（長さ不一致時もエラーにしない）。空リスト時は何も送信しない |

**設定メソッド**:

| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `reload_com_port()` | `reload_com_port() -> None` | COMポート接続を再読み込み |

**出力メソッド**:
> **注**: `t1`=出力#1, `t2`=出力#2, `t`=stdout以外の出力, `s`=stdout割り当てパネル, `b`=モード付き（w=上書き, a=追記, d=削除）。例: `print_t1b()` = 出力#1へのモード付き出力。ただし、これは便宜的な覚え方であり、厳密な命名規則ではありません。


| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `print_t1()` | `print_t1(*objects: object, sep: str = ' ', end: str = '\n') -> None` | 上部ログパネルへ出力 |
| `print_t2()` | `print_t2(*objects: object, sep: str = ' ', end: str = '\n') -> None` | 下部ログパネルへ出力 |
| `print_t()` | `print_t(*objects: object, sep: str = ' ', end: str = '\n') -> None` | stdout出力先設定（`stdout_destination`）に応じて、stdout先として割り当てられていない方のログパネルへ出力。`stdout_destination=="1"` の場合は出力#2へ、`stdout_destination=="2"` の場合は出力#1へ |
| `print_s()` | `print_s(*objects: object, sep: str = ' ', end: str = '\n') -> None` | stdout割り当てパネルへ出力 |
| `print_ts()` | `print_ts(*objects: object, sep: str = ' ', end: str = '\n') -> None` | `print_s`と同じ（旧API互換のための別名） |
| `print_t1b()` | `print_t1b(mode: Literal["w", "a", "d"], *objects: object, sep: str = ' ', end: str = '\n') -> None` | 上部ログ（モード付き: w=上書き, a=追記, d=削除） |
| `print_t2b()` | `print_t2b(mode: Literal["w", "a", "d"], *objects: object, sep: str = ' ', end: str = '\n') -> None` | 下部ログ（モード付き） |
| `print_tb()` | `print_tb(mode: Literal["w", "a", "d"], *objects: object, sep: str = ' ', end: str = '\n') -> None` | stdout以外ログ（モード付き） |
| `print_tbs()` | `print_tbs(mode: Literal["w", "a", "d"], *objects: object, sep: str = ' ', end: str = '\n') -> None` | stdout割り当てパネルへ出力（モード付き: w=上書き, a=追記, d=削除） |
| `show_var()` | `show_var() -> None` | 内部変数の一覧をログパネルに表示。一時停止時に自動で呼び出されるほか、ユーザースクリプト内から手動で呼び出し可能。表示対象は `self` に定義した変数のみ。フレームワークが注入した `isRunning`, `message_dialogue`, `socket0`, `mqtt0`, `keys`, `thread`, `alive`, `postProcess`, `Line`, `Discord`, `_logger`, `camera`, `gui`, `ImgProc` は除外する |

**ダイアログメソッド**（ブロッキングWebポップアップ）:

| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `show_dialog()` | `show_dialog(title: str, widgets: list[Widget[str] | Widget[int] | Widget[float] | Widget[bool] | Widget[None]] | Widget[str] | Widget[int] | Widget[float] | Widget[bool] | Widget[None], blocking: bool = True) -> int` | 新API（推奨）。ブロッキングWebポップアップダイアログ。`blocking=True` で実行をブロックし、ダイアログIDを返す（結果は各Widgetの`value`属性から取得）。`blocking=False` で非ブロッキング実行。単一WidgetまたはWidgetリストを受け付ける |
| `is_dialog_closed()` | `is_dialog_closed(dialog_id: int) -> bool` | 非ブロッキングダイアログの終了確認 |
| `wait_dialog()` | `wait_dialog(dialog_id: int) -> Literal[0]` | 非ブロッキングダイアログの結果待機。ブロッキング待機後、戻り値は固定で`0`。結果は各Widgetの`value`属性から取得 |
| `dialogue6widget()` | `dialogue6widget(title: str, dialogue_list: list[list[Any]], desc: str | None = None, need: type[list] | type[dict] = list) -> list[str] | dict[int | str, str]` | 旧API（互換性維持）。マルチウィジェットダイアログ。`dialogue_list` は各ウィジェット定義のリスト。各要素は `[widget_type, label, ...]` の形式 |
| `dialogue6widget_select_settings()` | `dialogue6widget_select_settings(title: str, dialogue_list: list[list[Any]], dirname: str, desc: str | None = None, need: type[list] | type[dict] = list) -> list[str] | dict[int | str, str]` | 旧API（互換性維持）。設定選択付きダイアログ。`dialogue_list` の形式は `dialogue6widget()` と同じ |
| `dialogue()` | `dialogue(title: str, message: int | str | list[int | str], desc: str | None = None, need: type = list) -> list[str] | dict[int | str, str]` | 旧API（他実装との互換性必須）。単純ダイアログ |

**注**: 旧APIは互換性のために保持される。後方互換性を維持するため、旧APIも新APIと同等の完成度・品質でメンテナンスされる。一般ユーザーには新API（`show_dialog`）の使用を推奨するが、開発時の扱いは新APIと変わらない。

**Socketメソッド**:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `socket_connect()` | `socket_connect() -> None` | Socketサーバへ接続 |
| `socket_disconnect()` | `socket_disconnect() -> None` | Socketサーバから切断 |
| `socket_transmit_message()` | `socket_transmit_message(message: str) -> None` | Socket経由でメッセージ送信 |
| `socket_receive_message()` | `socket_receive_message(header: str, show_msg: bool = False) -> str | None` | ヘッダーフィルタ付き受信 |
| `socket_receive_message2()` | `socket_receive_message2(headerlist: list[str], show_msg: bool = False) -> str | None` | 複数ヘッダーフィルタ付き受信 |
| `socket_change_ipaddr()` | `socket_change_ipaddr(addr: str) -> None` | Socket IPアドレス変更 |
| `socket_change_port()` | `socket_change_port(port: int) -> None` | Socketポート変更 |
| `socket_change_alive()` | `socket_change_alive(flag: bool) -> None` | Socket aliveフラグ設定 |

**MQTTメソッド**:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `mqtt_transmit_message()` | `mqtt_transmit_message(roomid: str, message: str) -> None` | MQTTトピックへメッセージ公開 |
| `mqtt_receive_message()` | `mqtt_receive_message(roomid: str, header: str, show_msg: bool = False) -> str | None` | ヘッダーフィルタ付き購読/受信 |
| `mqtt_receive_message2()` | `mqtt_receive_message2(roomid: str, headerlist: list[str], show_msg: bool = False) -> str | None` | 複数ヘッダーフィルタ付き購読 |
| `mqtt_change_broker_address()` | `mqtt_change_broker_address(broker_address: str) -> None` | MQTTブローカーアドレス変更 |
| `mqtt_change_id()` | `mqtt_change_id(mqtt_id: str) -> None` | MQTTクライアントID変更 |
| `mqtt_change_clientId()` | `mqtt_change_clientId(clientId: str) -> None` | MQTT接続名変更 |
| `mqtt_change_pub_token()` | `mqtt_change_pub_token(pub_token: str) -> None` | 公開トークン変更 |
| `mqtt_change_sub_token()` | `mqtt_change_sub_token(sub_token: str) -> None` | 購読トークン変更 |

**通知メソッド**:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `LINE_text()` | `LINE_text(txt: str, token: str = "") -> None` | No-opスタブ（LINEサービスEOL）。WARNINGログを出力 |

#### 10.4.3 ImageProcPythonCommand

**インポート**: `from Commands.PythonCommandBase import ImageProcPythonCommand`

`PythonCommand`を拡張し、カメラと画像処理機能を追加。

**通知メソッド**（画像処理クラスのみ）:

| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `discord_image()` | `discord_image(content: str = "", index: int = 0, crop_fmt: CropFmt = "", crop: list[int] | None = None, keys: str | list[str] = "DISCORD_WEBHOOK") -> None` | Discord webhook経由でテキスト+スクリーンショット送信。`index`: 複数webhook設定時のインデックス（0始まり）。`keys`: 環境変数/設定キー名 |
| `LINE_image()` | `LINE_image(txt: str, crop_fmt: str = '', crop: list[int] | None = None, token: str = '') -> None` | No-opスタブ（LINEサービスEOL）。WARNINGログを出力 |

**トリミングパラメータ**:
- `crop_fmt: CropFmt` — トリミング形式。以下の値を指定:
  - `""` (デフォルト): トリミングなし
  - `"1"`: Pillow形式 [x_start, y_start, x_end, y_end]
  - `"2"`: Pillow形式 [x_start, y_start, width, height]
  - `"3"`: Pillow形式 [x_start, x_end, y_start, y_end]
  - `"4"`: Pillow形式 [x_start, width, y_start, height]
  - `"11"`: OpenCV形式 [y_start, x_start, y_end, x_end]
  - `"12"`: OpenCV形式 [y_start, x_start, height, width]
  - `"13"`: OpenCV形式 [y_start, y_end, x_start, x_end]
  - `"14"`: OpenCV形式 [y_start, height, x_start, width]

```python
from typing import Literal
type CropFmt = Literal["", "1", "2", "3", "4", "11", "12", "13", "14"]
```

- `crop: list[int] | None` — トリミング座標のリスト。`crop_fmt` に応じた4要素の整数リスト。`None`または空リストの場合はトリミングなし

**コンストラクタ**: `ImageProcPythonCommand(cam: Camera, gui: CaptureArea | None = None)`

> **注**: `cam` と `gui` は互換性維持のためのパラメータ。リファクタリング前のスクリプトでは `cam` を必須、`gui` を省略可能として使用。リファクタリング後もフレームワークが自動的に渡す。**ユーザースクリプト内で他のユーザースクリプトを実行する機能は提供されないため、ユーザースクリプトから `ImageProcPythonCommand` を直接インスタンス化することはない**。`cam` はフレームワークから呼び出される際に必ず渡される

**`self.camera` プロパティ（Camera型）**:

`self.camera` として `ImageProcPythonCommand` に注入される `Camera` クラスの公開API。一部ユーザーは提供されるラッパーメソッド（`getCameraImage()`, `displayRectangle()` 等）に頼らず、直接 `self.camera.*` にアクセスするヘルパー関数を自作するため、**全ての公開メソッド・プロパティを実装する必要がある**。`Camera` クラスはOpenCVベースのロジッククラスであり、Tkinter等のUIフレームワークに依存しない。

| メソッド/プロパティ | シグネチャ | 説明 |
|---------------------|-----------|------|
| `image_bgr` (property) | `image_bgr -> MatLike` | 現在のカメラフレーム（BGR形式）のコピーを取得 |
| `readFrame()` | `readFrame() -> MatLike` | `image_bgr` プロパティのエイリアス。現在のフレームのコピーを返す |
| `isOpened()` | `isOpened() -> bool` | カメラがオープンされているか |
| `fps` (property) | `fps -> int` | カメラFPS（取得・設定可能） |
| `capture_size` (property) | `capture_size -> tuple[int, int]` | キャプチャ解像度 `(width, height)`。UI表示サイズとの比率計算に使用される |
| `flip` (property) | `flip -> bool` | 画像反転の有無 |
| `flip_mode` (property) | `flip_mode -> int` | 反転モード（`0`: 上下反転, `1`: 左右反転, `-1`: 上下左右反転） |
| `set_flip()` | `set_flip(value: Literal["None", "Vertical", "Horizontal", "Both"] | str) -> None` | 反転設定。正規値は `"None"` / `"Vertical"` / `"Horizontal"` / `"Both"`。互換性のため、実行時は大文字小文字を区別せず受け入れる |
| `saveCapture()` | `saveCapture(filename: str | None = None, crop: int | Literal["1"] | Literal["2"] | None = None, crop_ax: list[int] | None = None, img: MatLike | None = None) -> None` | カメラフレームを `./Captures/` に保存。`crop` でトリミング指定（`1`: `[x1,y1,x2,y2]`, `2`: `[x,y,w,h]`） |

> **注**: `openCamera()`, `destroy()`, `camera_thread_start()`, `camera_thread_stop()`, `camera_update()` はフレームワークが管理する内部メソッド。ユーザースクリプトから直接呼び出すことを想定しないが、互換性のため `self.camera.*` 経由でアクセス可能とする

> **注（反転状態の対応）**: `set_flip("Vertical")` は `flip=True`, `flip_mode=0`、`set_flip("Horizontal")` は `flip=True`, `flip_mode=1`、`set_flip("Both")` は `flip=True`, `flip_mode=-1` と対応する。`set_flip("None")` は `flip=False` とし、`flip_mode` の値は参照しない。

**`self.gui` / `self.canvas` プロパティ（CaptureArea型）**:

`self.gui` として注入される `CaptureArea` クラスの公開API。`self.canvas` は `self.gui` のエイリアスであり、同一の `CaptureArea` インスタンスを指す。一部ユーザーはラッパーメソッドに頼らず直接 `self.gui.*` / `self.canvas.*` にアクセスするヘルパーを自作するため、**独自実装メソッドは全て実装する**。`CaptureArea` はTkinterの `tk.Canvas` に由来するが、リファクタリング後はUIフレームワークに依存しないバックエンド実装となる。

| メソッド | シグネチャ | 説明 |
|---------|-----------|------|
| `ImgRect()` | `ImgRect(x1: int, y1: int, x2: int, y2: int, outline: str, tag: str | int, ms: int, flag: bool = True) -> None` | カメラ映像に矩形をオーバーレイ描画。`show_size` と `camera.capture_size` の比率で座標をスケーリング。`flag=True` の場合 `ms` ミリ秒後に自動削除 |
| `ImgText()` | `ImgText(x1: int, y1: int, txt: str, tag: str | int, ms: int, ft: tuple[str, int] = ("UD デジタル 教科書体 NP-B", 20), color: str = "black", flag: bool = True) -> None` | カメラ映像にテキストをオーバーレイ描画。`flag=True` の場合 `ms` ミリ秒後に自動削除 |
| `deleteImageRect()` | `deleteImageRect(tag: int | str) -> None` | 指定タグの矩形オーバーレイを削除 |
| `deleteImageText()` | `deleteImageText(tag: int | str) -> None` | 指定タグのテキストオーバーレイを削除 |
| `setFps()` | `setFps(fps: str | int) -> None` | UI表示FPSを設定 |
| `setShowsize()` | `setShowsize(show_height: int, show_width: int) -> None` | UI表示サイズを設定 |
| `changeRightMouseMode()` | `changeRightMouseMode(mode: str) -> None` | 右クリックモードを変更 |
| `setTouchscreenArea()` | `setTouchscreenArea(...) -> None` | タッチスクリーン操作領域を設定 |
| `saveCapture()` | `saveCapture() -> None` | カメラフレームを保存（`camera.saveCapture()` に委譲） |
| `show_size` (property) | `show_size -> tuple[int, int]` | UI表示サイズ `(height, width)` |
| `is_show_var` (property) | `is_show_var -> bool` | 映像表示有効フラグ |

> **注（Tkinter由来のUI操作メソッド）**: `update()`（フレーム更新）、`BindLeftClick()`/`BindRightClick()`/`UnbindLeftClick()`/`UnbindRightClick()`（マウスイベントバインド）、`mouseCtrlLeftPress()`/`mouseLeftPress()`/`mouseLeftPressing()`/`mouseRightPress()`/`mouseRightPressing()` 等（マウス/タッチイベント処理）、`StartRangeSS()`/`MotionRangeSS()`/`ReleaseRangeSS()`（範囲スクリーンショット）は、元実装では `tk.Canvas` のAPIに依存していた。リファクタリング後はWebUIのDOMイベント/Canvas APIで再実装する。ユーザースクリプトから直接呼び出されることは想定されないが、フレームワーク内部で使用されるため互換性を維持する

**MatLike型**:
```python
from cv2.typing import MatLike
```

OpenCV画像配列型。`numpy.ndarray` のサブクラス互換。画像処理メソッドの戻り値・引数として使用される。

**画像処理メソッド**（Rust実装）:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `isContainTemplate()` | `isContainTemplate(template_path: str, threshold: float = 0.7, use_gray: bool = True, show_value: bool = False, show_position: bool = True, show_only_true_rect: bool = True, ms: float = 2000, crop_fmt: CropFmt = "", crop: list[int] | None = None, mask_path: str | None = None, use_gpu: bool = False, BGR_range: dict[Literal["lower", "upper"], int | tuple[int, int, int]] | None = None, threshold_binary: int | None = None, crop_template: list[int] | None = None, show_image: bool = False, color: list[str] | None = None) -> bool` | カメラフレームに対するテンプレートマッチング |
| `isContainTemplate_max()` | `isContainTemplate_max(template_path_list: list[str], threshold: float = 0.7, use_gray: bool = True, show_value: bool = False, show_position: bool = True, show_only_true_rect: bool = True, ms: float = 2000, crop_fmt: CropFmt = "", crop: list[int] | None = None, mask_path_list: list[str | None] | None = None, BGR_range: dict[Literal["lower", "upper"], int | tuple[int, int, int]] | None = None, threshold_binary: int | None = None, crop_template: list[int] | None = None, show_image: bool = False, color: list[str] | None = None) -> tuple[int, list[float], list[bool]]` | マルチテンプレートマッチング |
| `isContainTemplateGPU()` | `isContainTemplateGPU(template_path: str, threshold: float = 0.7, use_gray: bool = True, show_value: bool = False, show_position: bool = True, show_only_true_rect: bool = True, ms: float = 2000, crop_fmt: CropFmt = "", crop: list[int] | None = None, mask_path: str | None = None, BGR_range: dict[Literal["lower", "upper"], int | tuple[int, int, int]] | None = None, threshold_binary: int | None = None, crop_template: list[int] | None = None, show_image: bool = False, color: list[str] | None = None) -> bool` | `isContainTemplate()` の互換性維持スタブ。`use_gpu=True` を固定して呼び出すが、本リファクタリングではGPU処理は行わずCPUで実行する |
| `isContainedImage()` | `isContainedImage(image_path: str, threshold: float = 0.7, use_gray: bool = True, show_value: bool = False, show_position: bool = True, show_only_true_rect: bool = True, ms: float = 2000, crop_fmt: CropFmt = "", crop: list[int] | None = None, mask_path: str | None = None, use_gpu: bool = False, BGR_range: dict[Literal["lower", "upper"], int | tuple[int, int, int]] | None = None, threshold_binary: int | None = None, crop_template: list[int] | None = None, show_image: bool = False, color: list[str] | None = None) -> bool` | 逆テンプレートマッチング |
| `saveCapture()` | `saveCapture(filename: str | None = None, crop_fmt: CropFmt = "", crop: list[int] | None = None, mode: bool = True) -> None` | カメラフレームを./Captures/へ保存 |
| `popupImage()` | `popupImage(crop_fmt: CropFmt = "", crop: list[int] | None = None, title: str = "image") -> None` | カメラフレームをポップアップ表示 |
| `getCameraImage()` | `getCameraImage(crop_fmt: CropFmt = "", crop: list[int] | None = None) -> MatLike` | カメラフレームをOpenCV画像配列で取得 |
| `openImage()` | `openImage(filename: str, mode: str = "t") -> MatLike | None` | 画像ファイルを読み込み |
| `setTemplateDir()` | `setTemplateDir(path: str) -> None` | テンプレート画像ディレクトリを変更 |
| `get_filespec()` | `get_filespec(filename: str, mode: str = "t") -> str` | 相対ファイル名をフルパスに解決 |
| `displayRectangle()` | `displayRectangle(max_loc: list[int] | Sequence[int], width: int, height: int, tag: str | None = None, ms: float = 2000, color: list[str] | None = None, crop_fmt: CropFmt = "", crop: list[int] | None = None) -> None` | カメラ映像に矩形をオーバーレイ描画（バックエンド側で画像加工） |
| `displayText()` | `displayText(position: Sequence[int], txt: str, tag: str | None = None, ms: float = 2000, font: str = "UD デジタル 教科書体 NP-B", fontsize: int = 20, color: str = "black") -> None` | カメラ映像にテキストをオーバーレイ描画（バックエンド側で画像加工） |

> **注（`saveCapture()` の層差）**: `self.saveCapture()`（`ImageProcPythonCommand` 継承時）は画像処理APIであり、`crop_fmt` と `crop` を使用する。`self.camera.saveCapture()` は注入された `Camera` APIであり、`crop` と `crop_ax` を使用する。両者は互換性維持のため同名だが、属するレイヤーと引数構造が異なる。

**例**（典型的な使用パターン）:

```python
# 基本的なテンプレートマッチング
found = self.isContainTemplate("battle_start.png", threshold=0.8)

# グレースケール無効、特定領域で検索
found = self.isContainTemplate(
    "menu_icon.png",
    use_gray=False,
    crop=[100, 100, 200, 200]  # x, y, width, height
)

# マルチテンプレート（最も一致度の高いものを返す）
best_idx, scores, results = self.isContainTemplate_max(
    ["template_a.png", "template_b.png", "template_c.png"],
    threshold=0.75
)

# 検出位置に矩形を描画（2秒間表示）
self.displayRectangle([max_x, max_y], width=50, height=50, ms=2000)

# テキストオーバーレイ
self.displayText([10, 10], "HP: 100/100", ms=3000, color="green")
```

#### 10.4.4 McuCommand（McuCommandBase）

**インポート**: `from Commands.McuCommandBase import McuCommand`

> **注**: ファイル名は `McuCommandBase.py`、クラス名は `McuCommand`。メインブランチの実装に準拠

ファームウェアベースコマンド用。PythonCommandと同じメタクラス切り替え。

**コンストラクタ**: `McuCommand(sync_name: str)`

- `sync_name`: ファームウェアとの同期に使用するコマンド名。`ser.writeRow(sync_name)` で送信される

**メソッド**:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `start()` | `start(ser: Sender, postProcess: Callable[[], None] | None) -> None` | コマンド開始。`sync_name` をシリアル送信し、`isRunning = True` を設定 |
| `end()` | `end(ser: Sender) -> None` | コマンド終了。`"end"` をシリアル送信し、`isRunning = False` を設定。`postProcess` が設定されていれば実行 |

**ライフサイクル**:
1. `McuCommand("command_name")` でインスタンス作成
2. `start(ser, postProcess)` でコマンド開始（ファームウェアに `sync_name` を送信）
3. ファームウェア側で処理実行
4. `end(ser)` でコマンド終了（ファームウェアに `"end"` を送信）

**注**: McuCommandはPythonCommandとは異なり、Pythonコード内で処理を実行するのではなく、ファームウェア（マイコン）側で処理を実行する。Python側はコマンドの開始・終了のシグナル送信のみを担当。

### 10.5 キー入力・シリアル送信

#### 10.5.1 KeyPress

- ユーザースクリプトに**直接公開されない**
- `self.keys.neutral()`のみアクセス可能（コントローラーをニュートラル状態にリセット）
- `self.keys.ser.write()` で生シリアル書き込みが可能（PyO3でpySerial互換型変換）。引数は `bytes` 型のみ
- `self.keys.ser.writeRow()` でシリアル行書き込み（末尾に改行自動追加）
- **注**: `self.keys` は互換性維持のための旧API。ユーザースクリプトからは引き続き `self.keys` を使用する

#### 10.5.2 Sender

**PyO3実装**（限定公開API）:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `writeRow()` | `writeRow(row: str) -> None` | シリアル行を書き込み（末尾に改行を自動追加） |
| `write()` | `write(data: bytes | bytearray | memoryview | list[int]) -> None` | 直接シリアル書き込み（PyO3でpySerial互換型変換）。`to_bytes()` 関数により以下の変換が行われる:
- `bytes`: そのまま通過
- `bytearray`/`memoryview`: `bytes` に変換
- `list[int]`（バイト値のリスト）: `bytes(bytearray(seq))` に変換
- `str`: `TypeError`（`unicode strings are not supported, please encode to bytes`）
- `int`（スカラー）: N個のゼロバイトを書き込む（`bytearray(n)` の動作）。予期しない動作となるため避けること
- `float`: `TypeError`

その他のSenderメソッドは、以下のクラスに統合:
- シリアル接続管理 → `SerialManager` クラス（内部実装クラス。UI制御とバックエンドの橋渡しを行う。本ドキュメントではAPI仕様を定義しない）
- コネクション状態 → `pokecon.state` 名前空間（§11.5.6.3で定義）

### 10.6 ダイアログAPI

**旧API（互換性維持）**: `dialogue6widget()` — 互換性のために保持。旧APIも新APIと同等の完成度・品質でメンテナンスされる。

**新API（推奨）**: `show_dialog()` — 事前に作成したWidgetインスタンスを渡す方式。型安全性と一貫性が向上。

**API分類と扱い**:

| 分類 | 対象API | 扱い |
|------|---------|------|
| **新API（推奨）** | `show_dialog()` | 積極的に推奨。新規スクリプトではこちらを使用 |
| **旧API（互換性維持）** | `dialogue6widget()`, `dialogue6widget_select_settings()` | 互換性維持のため残す。APIシグネチャの変更はユーザーからの使用状況を考慮する必要がある |
| **旧API（他実装との互換性必須）** | `dialogue()`（他実装との互換性を維持する必要がある場合） | 他のPoke-Controller互換ソフトとの互換性維持が必要。シグネチャ: `dialogue(title: str, message: int | str | list[int | str], desc: str | None = None, need: type = list) -> list[str] | dict[int | str, str]` |

#### 10.6.1 ダイアログライフサイクル

**スクリプト停止時の挙動**: ユーザースクリプトが停止（Stopボタン、エラー、強制終了など）した場合、スクリプト内で作成したすべてのダイアログ（ブロッキング・非ブロッキング・`wait_dialog`待機中を含む）を自動で閉じる。

**不正終了時の挙動**: OKボタン以外でダイアログが閉じられた場合、スクリプトを停止する:
- **×ボタン**: 確認ダイアログを表示せずにダイアログを閉じ、スクリプトを停止する
- **Escキー**: ダイアログを閉じずにスクリプトを停止する（ダイアログはスクリプト停止時に自動で閉じられる）
- **強制終了（ウィンドウの完全削除等）**: 即座にスクリプトを停止する
- スクリプト停止に伴い、スクリプト内のすべてのダイアログが自動で閉じられる（上記「スクリプト停止時の挙動」を参照）

```python
class Widget[T]:
    @overload
    def __init__(self, widget_type: Literal["Entry"], label: str, default: str) -> None: ...

    @overload
    def __init__(self, widget_type: Literal["Check"], label: str, default: bool) -> None: ...

    @overload
    def __init__(self, widget_type: Literal["Combo"], label: str, options: list[T], default: T) -> None: ...

    @overload
    def __init__(self, widget_type: Literal["Spin"], label: str, options: list[int], default: int) -> None: ...

    @overload
    def __init__(self, widget_type: Literal["Spin"], label: str, min: int, max: int, default: int) -> None: ...

    @overload
    def __init__(self, widget_type: Literal["Scale"], label: str, min: float, max: float, default: float) -> None: ...

    @overload
    def __init__(self, widget_type: Literal["Next"]) -> None: ...

    def __init__(self, widget_type: str, label: str = "", *args: object, **kwargs: object) -> None:
        self.widget_type = widget_type
        self.label = label
        self.value: T | None = None  # ダイアログ後に結果を格納

```

```python
# 事前にWidgetインスタンスを作成
entry = Widget("Entry", "名前", "デフォルト")  # Widget[str]
check = Widget("Check", "有効", True)  # Widget[bool]

# ブロッキング（デフォルト）
dialog_id = self.show_dialog("タイトル", widgets=[entry, check])
# dialog_id == 0
print(entry.value)  # str
print(check.value)  # bool

# 非ブロッキング
dialog_id = self.show_dialog("タイトル", widgets=[entry, check], blocking=False)
# dialog_id > 0（固有の自然数）
# スクリプトの実行は継続される

# ダイアログが終了したか確認
if self.is_dialog_closed(dialog_id):
    print(entry.value)

# ブロッキング動作に切り替え
self.wait_dialog(dialog_id)
print(entry.value)
```

#### 10.6.2 ブロッキング（デフォルト）

```python
@overload
def show_dialog(self, title: str, widgets: list[Widget[str] | Widget[int] | Widget[float] | Widget[bool] | Widget[None]] | Widget[str] | Widget[int] | Widget[float] | Widget[bool] | Widget[None], blocking: Literal[True] = True) -> Literal[0]: ...

@overload
def show_dialog(self, title: str, widgets: list[Widget[str] | Widget[int] | Widget[float] | Widget[bool] | Widget[None]] | Widget[str] | Widget[int] | Widget[float] | Widget[bool] | Widget[None], blocking: Literal[False]) -> int: ...
```

- `blocking=True`の場合、ダイアログが閉じられるまでスクリプトの実行を停止
- 返り値は`0`
- 結果は各Widgetの`value`属性に格納される
- **ライフサイクル**: §10.6.1を参照
- **実装制約**: ブロッキング対象はユーザースクリプトの実行単位のみとする。
  Rustバックエンド、WebSocket/WebRTC、フロントエンド応答処理は継続して動作し、
  UIからのダイアログ完了通知を受け取れる状態を維持する。
  非同期ランタイムのイベントループを直接停止してはならない

#### 10.6.3 非ブロッキング

- `blocking=False`の場合、ダイアログを表示し、スクリプトの実行を継続
- 返り値はユーザースクリプトが開始してから停止するまでの間で固有の自然数（ダイアログID）
- 結果は各Widgetの`value`属性に格納される
- **ライフサイクル**: §10.6.1を参照

#### 10.6.4 ダイアログ状態確認

- `is_dialog_closed(dialog_id: int) -> bool`
- 指定したダイアログIDのダイアログが終了しているかどうかを確認
- 終了していれば`True`、表示中または未表示であれば`False`
- **ライフサイクル**: §10.6.1を参照

#### 10.6.5 ダイアログ待機

- `wait_dialog(dialog_id: int) -> Literal[0]`
- 指定したダイアログIDのダイアログが終了するまでブロッキングで待機
- 戻り値は固定で `0`（ダイアログの完了を示す。実際のダイアログ結果は各Widgetの `value` 属性から取得）
- 非ブロッキングで表示したダイアログを後からブロッキング動作に切り替える際に使用
- **ライフサイクル**: §10.6.1を参照
- **実装制約**: `show_dialog(..., blocking=True)` と同じく、
  ブロッキング対象はユーザースクリプトの実行単位のみとし、
  バックエンド通信処理やUI応答処理を停止してはならない

### 10.7 スクリプト互換性要件

| 要件 | 状態 |
|------|------|
| サンプルスクリプトが変更なしで動作 | ✅ 必須 |
| `from Commands.PythonCommandBase import PythonCommand` | ✅ モジュールパッチで保持 |
| `from Commands.Keys import Button, Hat, ...` | ✅ モジュールパッチで保持 |
| `self.keys.neutral()` | ✅ 利用可能 |
| `self.keys.ser.writeRow()` | ✅ 利用可能（末尾に改行自動追加） |
| `self.keys.ser.write()` | ✅ 利用可能（引数は `bytes` 型） |
| 画像処理API | ✅ Rust実装 |
| Discord通知 | ✅ 実装済み |
| LINE通知 | ⚠️ No-opスタブ（サービスEOL） |
| Windows通知 | ✅ 実装済み |

---

## 11. 設定ファイルシステム

### 11.1 設定の種類と対象ユーザー

| 種類 | ファイル | 言語 | 用途 | 対象ユーザー |
|------|---------|------|------|------------|
| **静的設定** | `settings.toml` | TOML | グローバル設定、プロファイル管理 | **全ユーザー** |
| **動的設定** | `init.py` | Python | イベントハンドラ、カスタムロジック | **パワーユーザー** |
| **動的設定** | `init.lua` | Lua | イベントハンドラ、カスタムロジック | **パワーユーザー** |

**重要**:

- TOMLは**動的ではない**。Python/Luaのみが動的設定ファイルとして使用される
- **動的設定にPythonを使用する場合、ユーザースクリプト用Pythonとは独立したPython実行環境を使用する**。分離の詳細は§1.2のPython実行環境の分離方針に従う
- 動的設定でPython（`init.py`）を使用しない場合、動的設定用Python実行環境は生成しない
- **Python実行環境の設定**: `settings.toml` の `[python.script]` と `[python.dynamic]` で、それぞれ別々にPython実行環境を指定可能。`[python]`（共通セクション）で同時に指定することも可能（詳細は§11.4参照）
- **PythonとLuaで同じ設定が可能**: どちらの動的設定ファイルでも、同じ項目を同じ要素名（`pokecon.opt.xxx`）で設定できる
- **API構造の統一**: PythonとLuaで設定項目名は完全に同一。言語間で設定の互換性を維持

### 11.2 設定ファイル

> **重要**: `settings.ini` は**廃止**されました。従来のINIベースの設定は、TOML形式の`settings.toml`に置き換えられます。設定ファイルの形式はTOML、保存場所は`~/.config/pokecon/settings.toml`（グローバル）および`~/.config/pokecon/profiles/<name>/settings.toml`（プロファイル）です。

### 11.3 優先順位とマージ方式

起動時の設定値は以下の5層で優先順位が決まる（**後勝ち**、未設定項目は上位から継承）：

1. **デフォルト値**（アプリケーション内蔵）
2. **グローバル設定**（`~/.config/pokecon/settings.toml`）
3. **プロファイル設定**（`~/.config/pokecon/profiles/<name>/settings.toml`）
4. **動的設定**（`~/.config/pokecon/init.py`/`init.lua`）
5. **起動時引数**（CLIオプション）

```
起動時優先順位: ①デフォルト → ②グローバル → ③プロファイル → ④動的設定 → ⑤CLI引数
               （低）                                              （高）
```

**注**: この優先順位は、アプリケーション起動時に最終的な初期設定値を決めるためのものです。CLI引数は起動時の明示指定であるため最優先とします。ただし起動後は、`AppStartupPost` 等のイベントハンドラ内で `pokecon.opt.*` を変更することにより、CLI引数由来の設定も動的設定から実質的に上書きできます。

**マージ方式**: 設定はキー単位でマージする。下位層に存在しないキーは上位層から継承する。配列やパッケージ一覧などの複合値は、値全体を1つのキーとして扱い、要素単位のdeep mergeは行わない。

### 11.4 静的設定（settings.toml）

**原則**: 静的設定で設定できる項目は、動的設定ファイルの指定（`dynamic_config_language`）を除き、**すべて動的設定（`init.py`/`init.lua`）からも設定可能**です。

**注**: `dynamic_config_language` は静的設定（`settings.toml`）のみで設定可能。動的設定ファイル内で言語を切り替えることはできない（循環依存を回避するため）。

#### 11.4.1 設定キーの命名規則

設定キーは、`pokecon.opt` からも同じ名前で参照できることを前提に、
**フラット構造**で定義します。命名規則は、セクション番号や
`settings.toml` の見出し由来ではなく、設定値の意味上のカテゴリに
基づきます。

- **グローバル設定**: プレフィックスなし。
  例: `language`, `active_profile`, `auto_reload_config`。
- **カテゴリ固有設定**: 意味上のカテゴリをプレフィックスとして付ける。
  例: `camera_fps`, `ui_fps`, `serial_port`, `dialog_button_position`。

**重要**:

- カテゴリ判定は、仕様書のセクション配置やTOMLの見出しではなく、
  設定値が自然に属する機能領域で行う。
- 例えば `camera_fps` はカメラ入力側のFPS、`ui_fps` は表示領域の
  描画FPSを表す。どちらも単に `fps` とはしない。
- `settings.toml` のセクションはファイル上の整理単位であり、
  `pokecon.opt.xxx` のAPI名を決める根拠ではない。
- `settings.toml` 内のキー名そのものが `pokecon.opt.<key>` の属性名になる。
  セクション名は属性名に付与しない。
- 例: `[notifications]` 内の `line_menu_behavior` は
  `pokecon.opt.line_menu_behavior` であり、
  `pokecon.opt.notifications_line_menu_behavior` ではない。
- メインブランチの設定ファイル形式との互換性は、意図的に維持しない。
  `settings.ini` から `settings.toml` へ移行するため、設定ファイル形式は
  本リファクタリングの仕様に従う。
- 互換性要件は領域ごとに異なる。ユーザースクリプトの公開インターフェイスは
  ほぼ完全な互換性が必要だが、UIや設定ファイル形式は、同等の機能を
  提供できればメインブランチと同一形式である必要はない。

```toml
# ~/.config/pokecon/settings.toml
# 注: 以下は主要な設定項目の例示です。網羅的な一覧ではありません。
# 未記載の項目も settings.toml で設定可能です（§11.3の優先順位に従う）。

[global]
language = "ja"  # 対応言語: "ja"（日本語）, "en"（英語）。将来的に拡張可能
auto_reload_config = false  # 動的設定ファイルの自動リロード（デフォルト無効）
dynamic_config_language = "lua"  # "python" | "lua" | "none"。動的設定ファイルの言語を指定。未指定時のデフォルトは "lua"（Neovimと同じ）

[websocket]
reconnect_interval_sec = 3  # 再接続間隔（秒）
reconnect_max_retries = 20  # リトライ回数上限

# Python実行環境設定
# 優先順位: [python.script] / [python.dynamic] > [python]（共通）
# 未設定項目は上位から継承（キー単位マージ。配列・パッケージ一覧は値全体を置換し、deep mergeなし）

# ---- 優先順位の詳細 ----
# 1. [python.script] / [python.dynamic] が存在する場合: その値を使用
# 2. [python.script] / [python.dynamic] が未設定の項目: [python]（共通）の値を使用
# 3. [python] も未設定の項目: デフォルト値を使用
#
# 例: [python.script] に interpreter のみ設定し、[python] に venv と packages を設定した場合:
#   - [python.script]: interpreter = 指定値, venv = [python]の値, packages = [python]の値
#   - [python.dynamic]: interpreter = デフォルト値, venv = [python]の値, packages = [python]の値

# ---- 別々に指定する場合（推奨） ----
[python.script]
# ユーザースクリプト用Python実行環境
# interpreter = "/usr/bin/python3.12"  # 例: システムPython
# venv = "~/.local/share/pokecon/venv-script"  # 例: 仮想環境

[python.script.packages]
# ユーザースクリプト用仮想環境への追加インストールパッケージ
# mode = "append"  # "append" = 初期値に追加 / "full" = 全指定（必須パッケージは自動追加）
# [[python.script.packages.list]]
# name = "requests"
# version = ">=2.28.0"

[python.dynamic]
# 動的設定用Python実行環境
# interpreter = "/usr/bin/python3.12"
# venv = "~/.local/share/pokecon/venv-dynamic"

[python.dynamic.packages]
# 動的設定用仮想環境への追加インストールパッケージ
# mode = "append"
# [[python.dynamic.packages.list]]
# name = "numpy"

# ---- 共通設定のみの場合（シンプル） ----
# [python] セクションを使用すると、[python.script] と [python.dynamic] の両方に
# 同じ設定が適用される。ただし、[python.script] / [python.dynamic] で上書き可能
# [python]
# interpreter = "/usr/bin/python3.12"
# venv = "~/.config/pokecon/venv"
# [python.packages]
# mode = "append"  # "append" = 初期値に追加 / "full" = 全指定（必須パッケージは自動追加）
# [[python.packages.list]]
# name = "requests"
# version = ">=2.28.0"

[profiles]
active_profile = "default"  # TOMLキー: active_profile（Python API: pokecon.opt.active_profile と同名）

# カメラ設定（グローバル）
[camera]
camera_fps = 60  # バックエンド処理FPS（上限なし。ソースの実FPSより高い場合はソースの上限で表示）
camera_resolution = "1280x720"  # カメラ解像度。選択肢: "640x360", "1280x720", "1920x1080"

# シリアル設定（グローバル）
[serial]
serial_port = "/dev/ttyUSB0"  # COMポート（環境に応じて変更）
serial_baudrate = 9600  # ボーレート
serial_data_format = "default"  # データ形式: "default", "qingpi", "3ds"

# WebRTC設定
[webrtc]
stun_server = "stun:stun.l.google.com:19302"  # STUNサーバーURL

# 映像フォールバック設定（Motion JPEG over WebSocket）
[video.fallback]
jpeg_quality = 85  # JPEG品質（1-100）。デフォルト: 85

# LINE通知メニュー項目の挙動（旧UI互換メニュー）
[notifications]
line_menu_behavior = "message"  # "message"（削除済みメッセージ表示、既定） / "noop"（何もしない）
discord_webhook_url = ""  # Discord Webhook URL。未設定時はDiscord通知を送信しない
discord_username = ""  # Discordメッセージのカスタムユーザー名（任意）
discord_avatar_url = ""  # DiscordメッセージのカスタムアバターURL（任意）

# ショートカットボタン割り当て（10ボタン）
[shortcuts]
button_1 = "Commands.PythonCommands.Samples.RankGlitch.MashA"  # 例: コマンドモジュールパス
button_2 = ""  # 未割り当て
button_3 = ""
button_4 = ""
button_5 = ""
button_6 = ""
button_7 = ""
button_8 = ""
button_9 = ""
button_10 = ""

# UI表示用FPSの選択肢（カスタマイズ可能）
[ui]
ui_fps_options = [5, 15, 30, 60]  # ラベルは自動生成（例: "5 FPS"）

```

### 11.5 動的設定

#### 11.5.1 読み込みタイミング

| タイミング | 動作 |
|-----------|------|
| **アプリケーション起動時** | 自動読み込み（`init.py`/`init.lua`） |
| **プロファイル切替時** | 自動読み込み（新プロファイルの設定を反映） |
| **手動** | メニュー「Load Dynamic Config」で読み込み |
| **自動リロード** | ファイル変更検知時（デフォルト無効、オプトイン）。OSネイティブのファイル監視を使用 |

#### 11.5.2 共存（Neovim準拠）

`init.py` と `init.lua` の両方が存在する場合、**Neovim/Vimと同様に一方のみ**読み込まれます。

| 設定 | 読み込まれるファイル |
|------|-------------------|
| `settings.toml` で `dynamic_config_language = "python"` を指定 | `init.py` |
| `settings.toml` で `dynamic_config_language = "lua"` を指定 | `init.lua` |
| `settings.toml` で `dynamic_config_language = "none"` を指定 | 動的設定を読み込まない |
| 未指定（デフォルト） | `init.lua` が優先（Neovimと同じ） |

**両方を使いたい場合**: 一方から `pokecon.source()` で另一方を読み込んでください。

```python
# init.py で init.lua を読み込む例
pokecon.source("~/.config/pokecon/init.lua")
```

```lua
-- init.lua で init.py を読み込む例
pokecon.source("~/.config/pokecon/init.py")
```

#### 11.5.3 共通特徴

**動的設定の共通特徴**:

- **即時反映**: 設定変更は即座にUIに反映される
- **メモリ上のみ**: 動的設定はファイルとして保存されているが、`pokecon.opt` の値はアプリケーション起動時にメモリに読み込まれ、終了時に破棄される。毎回 init ファイルから再評価される
- **起動時優先順位**: 起動時の初期値決定では、動的設定は静的設定（settings.toml）より優先されるが、CLI引数よりは低い（§11.3参照）
- **起動後の変更**: `AppStartupPost` 等のイベントハンドラ内で `pokecon.opt.*` を変更した場合、その変更は起動時引数由来の設定にも上書きとして作用する

#### 11.5.4 Python

```python
# ~/.config/pokecon/init.py
import pokecon

# 言語設定
pokecon.opt.language = "ja"

# 自動リロード設定
pokecon.opt.auto_reload_config = True

# プロファイル切替
pokecon.opt.active_profile = "default"

# カメラ設定（フラット構造）
# camera_fps: バックエンド処理FPS（上限なし。ソースの実FPSより高い場合はソースの上限で表示）
pokecon.opt.camera_fps = 60
# ui_fps: UI表示用FPS（getter/setterでUIのコンボボックスと連動）
pokecon.opt.ui_fps = 30
pokecon.opt.camera_resolution = "1280x720"

# シリアル設定（フラット構造）
pokecon.opt.serial_port = "COM3"
pokecon.opt.serial_baudrate = 115200
pokecon.opt.serial_data_format = "default"  # default | qingpi | 3ds

# 通知設定（フラット構造）
pokecon.opt.discord_webhook_url = "https://discord.com/api/webhooks/..."
pokecon.opt.discord_username = "PokeCon Bot"

# ウィジェットモード（文字列値: "ALL (default)" など7種類。§5.5参照）
pokecon.opt.widget_mode = "ALL (default)"  # §5.5参照

# ソフトウェアコントローラー位置
pokecon.opt.controller_position = "top"  # top | bottom

# ダイアログボタン位置（ダイアログのOK/Cancelボタンの配置）
pokecon.opt.dialog_button_position = "bottom"  # "top"（上部） / "bottom"（下部、既定） / "both"（上部と下部の両方に配置）

# UI FPS選択肢（カスタマイズ）
pokecon.opt.ui_fps_options = [5, 15, 30, 60]  # ラベルは自動生成

# 動的タグ追加（ScriptLoadPreイベント）
def add_dynamic_tags() -> None:
    for candidate in pokecon.state.command_candidates:
        if candidate.name.startswith("Auto"):
            candidate.tags.append("@Auto")

pokecon.autocmd.on("ScriptLoadPre", callback=add_dynamic_tags)
```

##### 11.5.4.1 エラーハンドリング（Python）

Pythonの動的設定ファイル読み込み時にエラーが発生しても、アプリケーションは継続して動作します。

- 構文エラー時はファイル全体の読み込みに失敗し、フォールバック設定を使用
- エラー内容はログパネルに出力（行番号・ファイル名・エラー内容）
- フォールバック機構により、前回の有効な設定を維持
- 初回起動時など前回の有効な設定が存在しない場合は、静的設定（`settings.toml`）を使用する。静的設定も存在しない項目は組み込みデフォルト値を使用する

#### 11.5.5 Lua

```lua
-- ~/.config/pokecon/init.lua
-- require不要で pokecon.* に直接アクセス

-- 設定（Pythonと同じ要素名・同じAPI構造）
pokecon.opt.language = "ja"
pokecon.opt.camera_fps = 60
pokecon.opt.ui_fps = 30

-- イベントハンドラ
pokecon.autocmd.on("CameraOpenPost", {
    callback = function()
        print("Camera opened")
    end
})
```

**Lua動的設定の特徴**:
- **テーブル構文**: リストは `{}`（例: `{5, 15, 30, 60}`）、辞書は `{key = value}` で指定
- **真偽値**: `true` / `false`（Pythonの `True` / `False` とは異なる）
- **コールバック**: Luaでは無名関数 `function() ... end` を使用

**注**: API構造の統一については§11.1を参照。PythonとLuaで設定項目名・API構造は完全に同一。

##### 11.5.5.1 Luaランタイム

Luaランタイムの実装にはLuaJITを使用します。

**エラーハンドリング**:
- Luaスクリプト内でエラーが発生した場合、エラーメッセージをログパネルにERRORレベルで出力
- エラーが発生してもアプリケーションの動作は継続（Luaランタイムの隔離）
- Pythonとの相互運用時は、各言語のエラーを個別に処理

```lua
-- エラー例: 存在しない関数を呼び出し
pokecon.autocmd.on("CameraOpenPost", function()
    local result = pokecon.nonexistent_function()  -- ERRORログに記録、処理継続
end)
```

| 項目 | 設定 |
|------|------|
| **Lua実装** | LuaJIT 2.1 |
| **ライセンス** | MIT（商用利用可能） |
| **バインディング** | Rustコアが管理するLuaJITランタイムで実行（Python実行環境とは独立） |

##### 11.5.5.2 エラーハンドリング（Lua）

Luaの動的設定ファイル読み込み時にエラーが発生しても、アプリケーションは継続して動作します。

- Luaスクリプト内でエラーが発生した場合、エラーメッセージをログパネルにERRORレベルで出力
- 構文エラー時に該当行をスキップし、残りを続行
- エラーが発生してもアプリケーションの動作は継続（Luaランタイムの隔離）
- Pythonとの相互運用時は、各言語のエラーを個別に処理
- エラー内容はログパネルに出力（行番号・ファイル名・エラー内容）
- フォールバック機構により、前回の有効な設定を維持
- 初回起動時など前回の有効な設定が存在しない場合は、静的設定（`settings.toml`）を使用する。静的設定も存在しない項目は組み込みデフォルト値を使用する

#### 11.5.6 動的設定の機能

##### 11.5.6.1 イベントシステム

動的設定ファイル（PythonおよびLua）で使用するイベント駆動のフックシステム。

###### 11.5.6.1.1 設計方針

- **Neovim/Vim風な設計**: `autocmd` スタイルのイベントハンドラ登録
- **Pre/Postフェーズ**: 原則としてすべてのイベントは `Pre`（事前）と `Post`（事後）の2フェーズを持つ。特別な理由がない限り両方を持つ必要がある
- **片方のみの例外**:
  - **Postのみ**: イベント発生前に処理を実行しても意味がない場合（例: `AppStartupPost` — アプリケーション起動前にはAPIが利用できない）
  - **Preのみ**: イベント発生後に処理を実行しても意味がない場合（例: `AppShutdownPre` — アプリケーション終了後に状態が失われる）
  - **Postのみ（ハードウェア接続）**: `SerialConnectPost`, `CameraOpenPost` — 接続/オープン前にはデバイスが利用できないため、Preフェーズのコールバックで行えることがない
  - **Preのみ（入力キャンセル）**: `InputPressedPre` — 入力押下をキャンセルするユースケースがあるが、Postフェーズでは入力が既に送信済みで何もできない
  - **Postのみ（入力解放）**: `InputReleasedPost` — 入力解放前にPreコールバックで行える実用的な処理がない
- **フェーズはイベント名に含める**: `phase` 引数ではなく、イベント名自体に `Pre`/`Post` を含める（型安全のため）
- **require不要**: Lua設定では `require` なしで `pokecon.*` にアクセス可能。グローバル名前空間に `pokecon` が注入される
- **Python/Lua両対応**: 両言語で同じAPI構造を使用

###### 11.5.6.1.2 名前空間設計

| 名前空間 | 用途 | API |
|---------|------|-----|
| `pokecon.autocmd` | イベントハンドラの登録・解除 | `on()`, `once()`, `off()`, `clear(group)` |
| `pokecon.event` | イベント定義・発火 | `define()`, `emit()`, `list_defined()` |

###### 11.5.6.1.3 イベントハンドラAPI

```python
# Python設定
import pokecon

# 基本的なイベント登録
# 戻り値: HandlerId（ハンドラ解除用）
# callback: 引数なし。pokecon.state に直接アクセスして情報を取得
# ※callbackに引数を渡す設計は現時点では不要（state経由で情報取得可能）。
#  ただし、将来の拡張性を考慮し、callbackに引数を渡す形への変更が容易な設計とする
handler_id = pokecon.autocmd.on("CameraOpenPost", callback=lambda: print("Camera opened"))

# 一度だけ実行
# 発火後の HandlerId は無効になり、off() は何もしない（エラーにはならない）
# group パラメータも使用可能（発火前にグループ単位で解除する場合）
handler_id_once = pokecon.autocmd.once("SerialConnectPost", callback=lambda: print("Serial connected"), group="serial_group")

# イベントハンドラ解除
# 引数: HandlerId（on() / once() の戻り値）
# 戻り値: None
pokecon.autocmd.off(handler_id)

# グループ単位で一括解除
# "all" = すべてのハンドラ解除
# "CameraOpenPost" = そのイベントの全ハンドラ解除
# "my_group" = ユーザ定義グループの全ハンドラ解除
# 戻り値: None
pokecon.autocmd.clear("all")
pokecon.autocmd.clear("CameraOpenPost")
pokecon.autocmd.clear("my_group")
```

**グループ（Neovimの `augroup` に相当）**:

グループは関連するイベントハンドラをまとめるための仕組みです。Neovim/Vimと同様に、グループを指定することでハンドラの管理が容易になります。

```python
# グループを指定して登録
pokecon.autocmd.on("CameraOpenPost", callback=lambda: print("Camera opened"), group="camera_group")
pokecon.autocmd.on("CameraClosePost", callback=lambda: print("Camera closed"), group="camera_group")

# グループ単位で一括解除
pokecon.autocmd.clear("camera_group")
```

**グループの特徴**:
- グループ名は任意の文字列（ただし予約グループ名は除く）
- 同じグループ名を複数のハンドラで共有可能
- `clear("group_name")` でグループ内の全ハンドラを一括解除
- グループを指定しない場合はデフォルトグループ（無名）に所属

**予約グループ名**:
- `"all"` — すべてのハンドラを対象とする特別なグループ
- 各イベント名（例: `"CameraOpenPost"`, `"SerialConnectPost"` 等）— そのイベントの全ハンドラを対象
- ユーザーは予約グループ名を `group` パラメータに指定できない（エラー）

```lua
-- Lua設定（Neovim風require-less、Pythonと同じAPI構造）
pokecon.autocmd.on("CameraOpenPost", {
    callback = function()
        print("Camera opened")
    end
})

-- 一度だけ実行
pokecon.autocmd.once("SerialConnectPost", {
    callback = function()
        print("Serial connected")
    end,
    group = "serial_group"
})

-- グループを指定して登録
pokecon.autocmd.on("CameraOpenPost", {
    callback = function()
        print("Camera opened")
    end,
    group = "my_group"
})

-- ハンドラ解除
pokecon.autocmd.off(handler_id)

-- グループ単位で一括解除
pokecon.autocmd.clear("all")
pokecon.autocmd.clear("CameraOpenPost")
pokecon.autocmd.clear("my_group")
```

###### 11.5.6.1.4 イベント定義・発火API

```python
# ユーザー定義イベント
# pokecon.event.define(event: str) -> None
# 戻り値: None
pokecon.event.define("MyCustomEvent")

# イベント発火
# pokecon.event.emit(event: str) -> None
# 戻り値: None
pokecon.event.emit("MyCustomEvent")

# 定義済みイベント一覧
# pokecon.event.list_defined() -> list[str]
# 戻り値: list[str]
print(pokecon.event.list_defined())
```

```lua
-- Lua設定（Pythonと同じAPI構造）
pokecon.event.define("MyCustomEvent")
pokecon.event.emit("MyCustomEvent")
print(pokecon.event.list_defined())
```

###### 11.5.6.1.5 組み込みイベント一覧

| イベント名 | フェーズ | 説明 |
|-----------|---------|------|
| `AppStartupPost` | Post | アプリケーション起動後 |
| `AppShutdownPre` | Pre | アプリケーション終了前 |
| `SerialConnectPost` | Post | シリアルポート接続後 |
| `SerialDisconnectPre` | Pre | シリアルポート切断前 |
| `SerialDisconnectPost` | Post | シリアルポート切断後 |
| `CameraOpenPost` | Post | カメラオープン後 |
| `CameraClosePre` | Pre | カメラクローズ前 |
| `CameraClosePost` | Post | カメラクローズ後 |
| `CommandStartPre` | Pre | コマンド実行開始前 |
| `CommandStartPost` | Post | コマンド実行開始後 |
| `CommandStopPre` | Pre | コマンド停止前 |
| `CommandStopPost` | Post | コマンド停止後 |
| `CommandErrorPre` | Pre | コマンドエラー処理前 |
| `CommandErrorPost` | Post | コマンドエラー処理後 |
| `ScriptLoadPre` | Pre | スクリプト読み込み前 |
| `ScriptLoadPost` | Post | スクリプト読み込み後 |
| `ConfigReloadPre` | Pre | 設定再読み込み前 |
| `ConfigReloadPost` | Post | 設定再読み込み後 |
| `InputPressedPre` | Pre | 入力押下前（コントローラー・キーボード両方） |
| `InputReleasedPost` | Post | 入力解放後（コントローラー・キーボード両方） |

**1. ScriptLoadPre/ScriptLoadPostのタイミング**

タグライフサイクルの順序:

1. コマンドクラスの抽出
2. 自動タグ生成
3. **ScriptLoadPre**発火 — `command_candidates` が設定済みの状態。ユーザーはコールバック内で `command_candidates` を変更可能（動的タグ追加）
4. 手動タグ統合（`@tag`形式のタグ）
5. 動的タグ追加（ScriptLoadPreコールバック内での変更を反映）
6. **ScriptLoadPost**発火 — すべてのタグ処理が完了した後
7. **SPA側へのデータ送信** — ScriptLoadPost後にUIにコマンドリストを送信

**2. 命名規則**

- **パスカルケース（アッパーキャメルケース）**: `CameraOpenPost`, `SerialConnectPost`
- **Pre/Post後置**: Neovim/Vim風（`BufReadPre`/`BufReadPost`に類似）
- **名前空間なし**: ドット区切りの名前空間は使用しない
- **動詞に限定しない**: 名詞・形容詞も可

**注**: 動的設定用イベントシステム（§11.5.6.1）とWebSocketイベント（§7.3.2）は**別々のシステム**です。両者は対応関係を持ちません。
- **動的設定イベント**: `CameraOpenPost`（PascalCase + Pre/Post後置）— 動的設定ファイル（`init.py`/`init.lua`）で使用
- **WebSocketイベント**: `camera.open`（lowercase + ドット区切り）— UIとバックエンド間の通信プロトコル。詳細は§7.3.2を参照

**命名規則の違い**:
- 動的設定イベント: イベント名に `Pre`/`Post` を含む（例: `CameraOpenPost`）
- WebSocketイベント: 動詞原形を使用（例: `camera.open`）

両者は同じタイミングで発火する場合もありますが、別々のシステムとして独立して動作します。ユーザーが両者の対応関係を意識する必要はありません。

###### 11.5.6.1.6 型注釈

```python
from typing import Literal

# 組み込みイベントの厳密な型定義
type BuiltinEvent = Literal[
    "AppStartupPost", "AppShutdownPre",
    "SerialConnectPost", "SerialDisconnectPre", "SerialDisconnectPost",
    "CameraOpenPost", "CameraClosePre", "CameraClosePost",
    "CommandStartPre", "CommandStartPost",
    "CommandStopPre", "CommandStopPost",
    "CommandErrorPre", "CommandErrorPost",
    "ScriptLoadPre", "ScriptLoadPost",
    "ConfigReloadPre", "ConfigReloadPost",
    "InputPressedPre", "InputReleasedPost"
]

# 組み込みイベント + ユーザー定義イベント
type EventName = BuiltinEvent | str
```

###### 11.5.6.1.7 エラーハンドリングとPreイベントのキャンセル

**エラーハンドリング**:

- イベントハンドラ内でエラーが発生しても、他のハンドラは継続して実行
- エラー内容はログに出力（イベント名、ハンドラID、エラーメッセージ、スタックトレース）
- フォールバック機構により、システム全体の動作を停止しない

**Preイベントのキャンセル**:

- Preフェーズのイベント（`CommandStartPre`, `InputPressedPre` 等）では、コールバックが**厳密なbool値 `False`** を返した場合のみ、該当処理をキャンセルする
- `None`, `0`, `""`, `nil` 等は全て「継続」として扱われる
- Postイベントではキャンセルは適用されない

```python
# Python例: CommandStartPreで特定コマンドの実行を阻止
def on_command_start() -> bool | None:
    if pokecon.state.current_command == "dangerous_script":
        return False  # キャンセル
    # return None  # 継続（明示的なFalse以外は全て継続）

pokecon.autocmd.on("CommandStartPre", callback=on_command_start)
```

```lua
-- Lua例: InputPressedPreで特定ボタンの入力を無視
pokecon.autocmd.on("InputPressedPre", {
    callback = function()
        if pokecon.state.last_input == "A" then
            return false  -- キャンセル
        end
        -- nil  -- 継続（明示的なfalse以外は全て継続）
    end
})
```

**コールバック型注釈**:

```python
from typing import Callable

# イベントコールバック: 戻り値なし（通常イベント）、または bool | None（Preイベントでキャンセル用）
type Callback = Callable[[], bool | None]
```

Postイベントおよびキャンセル不可イベントでコールバックが値を返した場合、その戻り値は無視される。キャンセル判定に使用されるのはPreイベントの厳密な `False` のみ。

| エラー種類 | 挙動 | ログ出力 |
|-----------|------|---------|
| コールバック内の例外 | 当該ハンドラのみ停止、他は継続 | ERRORレベル |
| 存在しないイベントへのemit | 無視（ハンドラがないだけ） | WARNINGレベル |
| ハンドラ登録時の無効なイベント名 | 登録拒否、例外を送出 | ERRORレベル |
| 循環参照（イベント発火中に同じイベントを発火） | 検出して無視（同一イベントの直接再入のみ検出。間接循環 A→B→A は検出対象外） | ERRORレベル |

##### 11.5.6.2 相互参照API

###### 11.5.6.2.1 設計方針

- **Neovimの`:source`に類似**: `pokecon.source(path)`
- **拡張子で自動判別**: `.py` → Python, `.lua` → Lua
- **相対パス・絶対パス両対応**

###### 11.5.6.2.2 API仕様

```python
# Python設定
import pokecon

# 絶対パス
pokecon.source("/home/user/.config/pokecon/extra_settings.py")  # -> None

# 相対パス（設定ディレクトリ基準）
pokecon.source("./extra_settings.py")  # -> None

# チルダ展開
pokecon.source("~/.config/pokecon/extra_settings.py")  # -> None
```

```lua
-- Lua設定（Pythonと同じAPI構造）
pokecon.source("~/.config/pokecon/extra_settings.lua")
```

###### 11.5.6.2.3 エラーハンドリング

- 指定されたファイルが存在しない場合はエラーをログに出力
- ファイルの読み込みに失敗しても、現在の設定は維持される
- 循環参照（AがBを読み込み、BがAを読み込む）を検出し、エラーを出力

##### 11.5.6.3 状態取得API

###### 11.5.6.3.1 設計方針

- **状態アクセス**: `pokecon.state.<property>` は現在の状態にアクセスする名前空間。原則として読み取りだが、動的設定からの変更が想定されるもの（タグ等）は書き込み可能
- **リアルタイム**: 現在の状態を即座に反映
- **スレッドセーフ**: 複数スレッドから安全に読み取り可能。書き込みは特定イベント（`ScriptLoadPre`等）のコールバック内または内部処理でのみ行われる

###### 11.5.6.3.2 状態プロパティ一覧

```python
from typing import Literal
type CommandState = Literal["running", "paused", "stopped", "error"]
```

| 属性 | 型 | 説明 |
|------|-----|------|
| `serial_port` | `str` | 現在のシリアルポート（例: `"COM3"`）。未設定時は空文字 `""` |
| `serial_baudrate` | `int` | 現在のボーレート（例: `115200`）。未設定時は `opt.serial_baudrate` または組み込みデフォルト値 |
| `serial_connected` | `bool` | 接続状態 |
| `camera_opened` | `bool` | カメラオープン状態 |
| `camera_fps` | `int` | 現在のFPS（`opt.camera_fps` をデバイス能力で制限した実際の値） |
| `camera_resolution` | `str` | 現在の解像度（例: `"1280x720"`） |
| `is_running` | `bool` | コマンド実行中 |
| `command_state` | `CommandState` | コマンド状態 |
| `current_command` | `str` | 現在実行中のコマンド名 |
| `command_candidates` | `list[CommandInfo]` | 読み込み候補コマンド一覧 |
| `tags` | `list[str]` | 利用可能なタグ一覧 |
| `active_profile` | `str` | 現在のアクティブプロファイル名 |
| `available_profiles` | `list[str]` | 利用可能なプロファイル一覧 |
| `last_input` | `str \| None` | 最後の入力（キー名またはボタン名） |
| `holding_buttons` | `list[str]` | 現在保持中のボタン一覧 |
| `pid` | `int` | アプリケーションのプロセスID |

**例**:
```python
# Python設定
import pokecon

# シリアル関連
print(pokecon.state.serial_port)        # 現在のシリアルポート（例: "COM3"）
print(pokecon.state.serial_baudrate)    # 現在のボーレート（例: 115200）
print(pokecon.state.serial_connected)   # 接続状態（True/False）

# カメラ関連
print(pokecon.state.camera_opened)      # カメラオープン状態（True/False）
print(pokecon.state.camera_fps)         # 現在のFPS（`opt.camera_fps` をデバイス能力で制限した実際の値）
print(pokecon.state.camera_resolution)  # 現在の解像度（例: "1280x720"）

# コマンド関連
print(pokecon.state.is_running)         # コマンド実行中（True/False）
print(pokecon.state.command_state)      # コマンド状態（"running" | "paused" | "stopped" | "error"）
print(pokecon.state.current_command)    # 現在実行中のコマンド名
print(pokecon.state.command_candidates) # 読み込み候補コマンド一覧（list[CommandInfo]）
print(pokecon.state.tags)               # 利用可能なタグ一覧（list[str]）

# プロファイル関連
print(pokecon.state.active_profile)     # 現在のアクティブプロファイル名
print(pokecon.state.available_profiles) # 利用可能なプロファイル一覧

# 入力関連
print(pokecon.state.last_input)         # 最後の入力（str | None。キー名またはボタン名）
print(pokecon.state.holding_buttons)    # 現在保持中のボタン一覧（list[str]）

# アプリケーション関連
print(pokecon.state.pid)                # アプリケーションのプロセスID
```

**`pokecon.opt` と `pokecon.state` の違い**:

| 名前空間 | 性質 | 説明 |
|---------|------|------|
| `opt` | 設定値（書き込み可能） | ユーザーが設定した**設定値**。UIコントロールや動的設定で変更される |
| `state` | 現在値（原則読み取り、一部書き込み可能） | デバイスやシステムが実際に使用している**現在値**。動的設定からの変更が想定されるもの（タグ等）は書き込み可能。デバイスの能力制限により、`opt` と異なる値を指すことがある |

**例**: `opt.camera_fps = 120` と設定しても、キャプチャデバイスが60fpsまでしか対応しない場合、`state.camera_fps` は `60` となる。

```lua
-- Lua設定
print(pokecon.state.serial_port)
print(pokecon.state.camera_opened)
print(pokecon.state.active_profile)
```

##### 11.5.6.4 プロファイルAPI

###### 11.5.6.4.1 設計方針

- **フラットAPI**: `pokecon.profile.current()`, `pokecon.profile.list()`, `pokecon.profile.switch(name)`
- **動的設定ファイル内で使用可能**
- **`pokecon.opt.active_profile` との関係**: `pokecon.opt.active_profile` のsetterは、内部的に `pokecon.profile.switch(name)` と同じプロファイル切替処理を呼び出す。どちらの場合も、存在しないプロファイル名の検証、プロファイル切替イベントの発火、新設定読み込み、ボタン強制解放、イベントハンドラ再登録を行う。成功/失敗を戻り値で扱いたい場合は `profile.switch()` を使用する
- **作成・削除**: プロファイルの作成・削除はAPIでは行わない。`~/.config/pokecon/profiles/<name>/` ディレクトリを手動で作成・削除する

###### 11.5.6.4.2 API仕様

```python
# Python設定
import pokecon

# 現在のプロファイル取得
# 戻り値: str（プロファイル名）
# pokecon.profile.current() -> str
current = pokecon.profile.current()
print(f"Current profile: {current}")

# 利用可能なプロファイル一覧
# 戻り値: list[str]
# pokecon.profile.list() -> list[str]
profiles = pokecon.profile.list()
print(f"Available profiles: {profiles}")

# プロファイル切替
# 引数: name: str
# 戻り値: bool（成功: True, 失敗: False）
# pokecon.profile.switch(name: str) -> bool
# エラー時: 存在しないプロファイル名を指定した場合はFalseを返し、エラーをログに出力
# 戻り値で成功/失敗を扱いたい場合は profile.switch() を使用する。pokecon.opt.active_profile への代入も同じ切替処理を実行する
success = pokecon.profile.switch("custom")
if not success:
    print("Failed to switch profile")
```

```lua
-- Lua設定（Pythonと同じAPI構造）
print(pokecon.profile.current())
print(pokecon.profile.list())
pokecon.profile.switch("custom")
```

###### 11.5.6.4.3 プロファイル切替時の動作

- 新しいプロファイルの設定を読み込み（`~/.config/pokecon/profiles/<name>/settings.toml`）
- 現在保持中のすべてのボタンを強制解放（holdEndSkip中のボタンを含む）
- イベントハンドラをクリアして再登録

##### 11.5.6.5 コントローラーAPI（動的設定用）

###### 11.5.6.5.1 設計方針

- **動的設定専用**: `init.py`/`init.lua` からコントローラーを操作するAPI
- **スクリプトAPIとは別**: `PythonCommand` クラスの `press()`, `hold()`, `holdEnd()` とは異なる設計。動的設定では状態ベースの一括更新が適切
- **複数ボタン同時操作**: 同じタイミングで複数ボタンの押下・離脱を表現できる必要がある
- **スティック入力**: x,y座標の絶対値指定、および角度+強度の指定の両方に対応
- **3DSタッチスクリーン対応**: タッチスクリーンのx,y座標指定

###### 11.5.6.5.2 型定義

```python
from typing import TypedDict, NotRequired, Literal

# スティック入力（x,y絶対値 または 角度+強度）
class StickInput(TypedDict):
    x: NotRequired[int]       # 0 ~ 255（絶対値指定時）。中心=128、デッドゾーン=103~153
    y: NotRequired[int]       # 0 ~ 255（絶対値指定時）。中心=128、デッドゾーン=103~153
    angle: NotRequired[float] # 0.0 ~ 360.0（角度+強度指定時）
    strength: NotRequired[float] # 0.0 ~ 1.0（角度+強度指定時）

# タッチスクリーン入力（3DS対応）
class TouchInput(TypedDict):
    x: int  # 0 ~ 319（3DS上画面幅。0-based）
    y: int  # 0 ~ 239（3DS上画面高さ。0-based）
    pressed: NotRequired[bool]  # True: タッチ開始, False: タッチ終了

# コントローラー状態更新
class ControllerUpdate(TypedDict):
    a: NotRequired[bool]
    b: NotRequired[bool]
    x: NotRequired[bool]
    y: NotRequired[bool]
    l: NotRequired[bool]
    r: NotRequired[bool]
    zl: NotRequired[bool]
    zr: NotRequired[bool]
    lclick: NotRequired[bool]
    rclick: NotRequired[bool]
    plus: NotRequired[bool]
    minus: NotRequired[bool]
    home: NotRequired[bool]
    capture: NotRequired[bool]
    left_stick: NotRequired[StickInput]
    right_stick: NotRequired[StickInput]
    hat: NotRequired[Literal["up", "down", "left", "right", "up_right", "up_left", "down_right", "down_left", "neutral"]]
    touch: NotRequired[TouchInput]
```

###### 11.5.6.5.3 API仕様

```python
# Python設定
import pokecon
from typing import TypedDict, NotRequired, Literal

# ボタン更新（指定したボタンのみ更新、未指定のボタンは現在の状態を維持）
# 引数: ControllerUpdate
# 戻り値: None
pokecon.controller.update({"a": True, "b": True})

# スティック絶対値指定
pokecon.controller.update({"left_stick": {"x": 50, "y": 50}})

# スティック角度+強度指定
pokecon.controller.update({"right_stick": {"angle": 45.0, "strength": 0.8}})

# タッチスクリーン（3DS対応）
pokecon.controller.update({"touch": {"x": 160, "y": 120, "pressed": True}})

# 全てのボタン・スティックを未入力状態に戻す
# 戻り値: None
pokecon.controller.reset()
```

```lua
-- Lua設定（Pythonと同じAPI構造）

-- ボタン更新
pokecon.controller.update({a = true, b = true})

-- スティック絶対値指定
pokecon.controller.update({left_stick = {x = 50, y = 50}})

-- スティック角度+強度指定
pokecon.controller.update({right_stick = {angle = 45.0, strength = 0.8}})

-- タッチスクリーン（3DS対応）
pokecon.controller.update({touch = {x = 160, y = 120, pressed = true}})

-- 全てリセット
pokecon.controller.reset()
```

---

#### 11.5.7 動的設定ファイルのUI

##### 11.5.7.1 メニュー配置

- **配置場所**: メニューバー内
- **メニュー構造**:

```
メニュー
├── コマンド
│   ├── LINE Token Assignment      ※旧UI互換（LINE通知UIは§4.3で削除済み）
│   ├── LINE Token Check           ※旧UI互換（LINE通知UIは§4.3で削除済み）
│   ├── Discord Setting Assignment
│   ├── Discord Check
│   ├── Generate Bat File & Profile Directory
│   ├── Pokemon Home 連携
│   ├── キーコンフィグ
│   └── 画面サイズのリセット
├── 設定（予約）
├── File
│   ├── Load Dynamic Config      ← 新規読み込み（拡張子で自動判別）
│   ├── Reload Dynamic Config    ← 現在のファイルを再読み込み
│   └── Open Config Directory    ← 設定ディレクトリを開く
└── 終了

ヘルプ
├── GitHub
├── Poke-Controller Guide
├── 質問テンプレート
├── バージョン確認
├── 更新履歴表示
├── アップデート確認
└── LICENSE
```

**注**: メインブランチのTkinter UIに準拠。新機能（Fileメニューの動的設定関連）はリファクタリング後の追加機能。LINE関連メニューは旧UI互換のため残存（§4.3参照）。

##### 11.5.7.2 ファイル選択と自動判別

- **ファイル選択ダイアログ**: 「Load Dynamic Config」メニューから開く
- **自動判別**: 拡張子で言語を自動判別
  - `.py` → Python動的設定ファイル
  - `.lua` → Lua動的設定ファイル
- **手動指定**: 拡張子が不明な場合はユーザーに選択を促す

##### 11.5.7.3 リロード種別

| 機能 | 説明 |
|------|------|
| **手動リロード** | 「Reload Dynamic Config」メニューで現在のファイルを再読み込み |
| **自動リロード** | ファイルウォッチャーによる自動リロード（**デフォルトで無効**） |
| **有効化方法** | `pokecon.opt.auto_reload_config = True` またはUI設定 |

---

## 12. 環境変数

| 変数 | 説明 | デフォルト |
|----------|-------------|---------|
| `POKECON_DISABLE_COMPOSITING` | コンポジットモードを無効化（Tauri） | `0` |
| `POKECON_WEB_DIR` | 静的ファイルディレクトリ | `web/dist` |
| `POKECON_PORT` | HTTPサーバーポート | `8020` |

---

## 13. クライアント側ストレージ

本仕様では、ショートカットボタン割り当てはクライアント側の
`localStorage` には保存しません。§6.4.2 の通り、
`settings.toml` の `[shortcuts]` セクションに永続化し、
ブラウザを変えても同一の設定を利用可能にします。

現時点で仕様化されたクライアント側永続化項目はありません。
UIの一時状態をブラウザメモリに保持することは実装詳細として許容しますが、
ユーザー設定の永続化先にはしません。

---

## 14. 開発環境自動構築

### 14.1 ディレクトリ構造

```
~/.config/pokecon/                    # XDG_CONFIG_HOME（ユーザーが編集する）
├── pyproject.toml                    # Python LSP設定
├── .luarc.json                       # Lua LSP設定（lua-language-server & EmmyLua共用）
├── .vscode/settings.json             # Pylance用（オプション）
├── settings.toml                     # ユーザー設定（グローバル）
├── profiles/                         # プロファイル設定
│   ├── default/
│   │   └── settings.toml
│   └── custom/
│       └── settings.toml
├── init.py                           # Python動的設定テンプレート
└── init.lua                          # Lua動的設定テンプレート
~/.local/share/pokecon/               # XDG_DATA_HOME（自動管理）
├── typings/                          # Python型定義（.pyi、Rust側で自動生成）
├── lua-typings/                      # Lua型定義（.d.lua、Rust側で自動生成）
├── venv/                             # Python仮想環境
└── python/                           # python-build-standalone（非nix環境）

# 開発用（リポジトリ内）
python/pokecon/typings/               # 型定義の元データ（開発・メンテナンス用）
├── __init__.pyi
├── keys.pyi
├── commands.pyi
└── events.pyi
```

**注**:
- **実行時生成**: `~/.local/share/pokecon/typings/` 配下の `.pyi` はアプリ起動時にRust側で自動生成
- **開発用元データ**: `python/pokecon/typings/` 配下の `.pyi` はリポジトリに含め、開発・メンテナンス用として使用
- **ユーザーが直接触らない**: XDG_DATA_HOME 配下は自動管理。ユーザーが編集するのは XDG_CONFIG_HOME 配下のみ

**確定事項**:
- 型定義ファイルの配布方式は **XDG_DATA_HOMEへの自動生成** で確定
- 開発用元データはリポジトリ内の `python/pokecon/typings/` に配置

### 14.2 設定ファイル生成タイミング

- **存在しない時に生成**（初回、アップデート、削除後等）
- **nix環境**: nix式で指定した場合のみnix側で生成。指定しなかった場合はアプリ起動時に存在しないためアプリ側で生成。
- **非nix環境**: アプリ側で自動生成

### 14.3 Python管理（nix環境）

nix環境では、Pythonインタープリターのパスを**ビルド時に決定**し、アプリケーションに組み込む。

**設計方針**:
- nixストアパスは不変なため、再現性が保証される
- グローバルPythonを使用しない（nixの隔離性を維持）
- 非nix環境では環境変数が未設定のため、実行時に別途Pythonを取得するフォールバック動作

**詳細な実装**: ビルドスクリプトおよびnix設定ファイルを参照。

### 14.4 Python管理（非nix環境）

非nix環境では、`PythonManager` がPythonインタープリターのセットアップを管理する。

**実装概要**:
- `~/.local/share/pokecon/` 配下にPythonをセットアップ
- 期待するバージョンがない場合はデフォルトを使用
- 既存のPythonが期待するバージョンかチェック
- ない場合はpython-build-standaloneをダウンロード
- 仮想環境を構築し、必須パッケージ + ユーザーパッケージをインストール

**詳細な実装**: Python管理モジュールを参照。

### 14.5 必須パッケージ管理

- **リポジトリ内`pyproject.toml`**からビルド時に取得
- **ビルドスクリプトでコード生成**、ソースに埋め込み
- pyproject.toml変更時に自動再ビルド

実装詳細はビルドスクリプトを参照。

### 14.6 LSP設定（pyproject.toml）

LSP（Language Server Protocol）設定は `pyproject.toml` で管理する。以下の項目を設定する必要がある:

- **型スタブパス**: `~/.local/share/pokecon/typings` を各LSPの検索パスに追加
- **仮想環境**: `~/.local/share/pokecon/venv` をPython環境として指定

対応LSP: basedpyright, pyright, mypy, pylsp, pyrefly, ty, ruff

**詳細な設定例**: リポジトリ内の `pyproject.toml` または開発者ドキュメントを参照。

### 14.7 Lua LSP設定（.luarc.json）

Lua LSP設定は `.luarc.json` で管理する。

- **型定義ライブラリ**: `~/.local/share/pokecon/lua-typings` をワークスペースライブラリに追加

**詳細な設定例**: リポジトリ内の `.luarc.json` または開発者ドキュメントを参照。

---

## 付録

本付録は、Tkinter UIの元の実装詳細およびコマンドクラスのメタクラス設計を参考情報として記載します。

### A. Tkinter UIリファレンス

#### A.1 元のタブ詳細

元のPython/Tkinter UIは `tkinter.ttk.Notebook` を使用し、以下の構造でした:

- **CameraTab**: スレッド化されたフレームリーダー、PILリサイズ、`ImageTk.PhotoImage` キャンバス表示による `cv2.VideoCapture`。キャンバスはマウス駆動のスティック制御、カラーピッカー、領域スクリーンショットをサポート。
- **SerialTab**: COMポートドロップダウン、ボーレートセレクター（9600/115200）、データ形式セレクター（デフォルト/Qingpi/3DS Controller）、ステータスインジケーター付き接続ボタン、Text+Scrollbar付きシリアルモニター。
- **ManualControlTab**: ソフトウェア制御（キーボードチェックボックス、LStick Mouse、RStick Mouse）、ハードウェア制御（ProController/Xinputラジオ、録画チェックボックス）、完全なJoy-ConレイアウトのSwitch Controller Simulator。
- **CommandTab**: 3サブタブ（Python Command、Mcu Command、Shortcut）、ファイルブラウザー、タグフィルタードロップダウン、Listbox/Treeview付きコマンドリスト、10ショートカットボタン、実行ボタン（開始/一時停止/再開/停止/再読み込み）。
- **NotificationTab**: Discord Webhook URL、ユーザー名、アバターURL入力（テストボタン付き）。Windows通知開始/終了チェックボックス（テストボタン付き）。LINE UI（削除 — サービスEOL）。
- **OthersTab**: 出力サイズ調整スライダー、stdout出力先ラジオ（出力#1/出力#2）、出力をクリアボタン、ウィジェットモードコンボボックス（7モード）、ソフトウェアコントローラー位置ラジオ（TOP/BOTTOM）、ダイアログボタン位置ラジオ（TOP/BOTTOM/BOTH）。

#### A.2 元のコントローラーレイアウト

- **ソフトウェアコントローラー**: CanvasベースのJoy-Con描画。`<Button-1>` イベントバインディングでホールド、`<ButtonRelease-1>` で解放、Shift+解放で `holdEndSkip`。
- **色**: L側シアン `#56CCF2`、R側赤 `#E9514E`、アクティブ状態黄色 `#FFD800`。
- **アナログスティックデッドゾーン**: §5.3.1参照。
- **ボタン**: A、B、X、Y、L、R、ZL、ZR、+、−、Home、Capture、D-pad（4方向）、Lスティック、Rスティック、タッチスクリーン（320×240）。

#### A.3 元の出力パネル

- **出力#1 と 出力#2**: スライダー（0～100）で比率調整可能なログ表示。
- **ソース**: WebSocket経由で受信したログ。
- **クリア**: その他タブの「出力をクリア」ボタン。

### B. メタクラス設計（CommandMeta）

**責務**:
- **実装切り替え**: クラス変数や関数使用パターンに基づいて、Python実装とPyO3（Rust）実装を切り替える
- **抽象クラスチェック**: `do()` メソッドを持つクラス（PythonCommand, ImageProcPythonCommand）を抽象クラスとして扱う。`do()` を実装しないサブクラスのインスタンス化を防止

**現在の挙動**:
- すべての実装がPyO3（Rustバインディング）に流れる
- インスタンス化時に `do()` メソッドの存在を確認し、未実装の場合は `TypeError` を送出

**将来の拡張**:
- クラス変数 `__target_implementation__` 等をチェックし、動的に実装クラスを選択
- 純粋Python実装とPyO3実装の切り替えをサポート

---

*本仕様書は生きたドキュメントです。新しい要件がユーザーから伝達された場合、更新を行う必要があります。*
