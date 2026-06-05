# Poke-Controller Modified Extension — 仕様書

> **バージョン**: 2.1.0-draft  
> **ブランチ**: `refactor/rust-core`  
> **日付**: 2026-05-28  
> **スコープ**: Web/デスクトップUI（SvelteKit）— バックエンドAPIおよびRustコアは対象外  
> **ソース**: セッション議事録から抽出した過去のユーザー要件（現在のコードベースではない）

---

## 0. 本ドキュメントの位置づけ

**本ドキュメントは要求仕様書・要件定義書として運用されます。**

- **仕様が先行し、実装が後に追従する**関係です
- 実装の都合で仕様を変更することはありません
- 実装と仕様に齟齬がある場合、**実装を修正してください**
- 齟齬が一時的なもの（実装予定だが未着手など）であれば、以下の対応を取ってください：
  - 他の適切なドキュメント（実装されている仕様を記すもの）に記載する
  - コード内にTODOコメントを記す
- **定義書確定後の設計変更についても、実装の都合で仕様を変更することはありません**。余程の事態（言語仕様的に実装が困難など）がない限り、実装を仕様に合わせて修正してください
- 各セクションの「確定事項」は、要件の綿密な確認と合意形成プロセスを経た設計決定です
- **今後のバージョンで実装予定の機能について**: 本仕様に記載されている「今後のバージョンで実装予定」の機能は、現在の設計に拡張性を持たせることを要件とします。

---

## 1. 概要・設計方針

### 1.1 目的

本ドキュメントは、従来のPython/Tkinter UIを置き換えるPoke-Controller Modified Extensionの新しいWeb/デスクトップUIの要件を規定します。本仕様は、リファクタリングセッション中に伝達された**過去のユーザー要件のみ**から導出されており、現在のコードベースからは導出されていません。

### 1.2 設計方針

- **Tkinterとの機能・視覚的パリティ**: 新しいUIは、元のTkinterの機能・レイアウト・外観に厳密に一致する必要があります。ただし、本ドキュメントで明示的に変更された項目（§4「主要ユーザー要件と却下事項」等）は除きます。レイアウト、色、ボタンの間隔、ウィジェットの種類はオリジナルに準拠する必要があります。
- **スクリプト互換性**: リファクタリング前のバージョンで動作していたすべてのスクリプトは、引き続き正常に動作する必要があります。スクリプトAPIに破壊的変更は加えません。
- **モダンスタック**: SvelteKit + Svelte 5（Runesモード）+ **Tailwind CSS v4**（確定、変更不可）。
- **低遅延通信**: プライマリとしてWebRTC、フォールバックとしてビデオ: WebCodecs + WebSocket、DataChannel: WebSocketを使用。WebSocketは切断時に3秒ごとに自動再接続。
- **型安全性**: Rustバックエンドから `utoipa` v5 + `openapi-typescript` を介してOpenAPI生成のTypeScript型を使用。
- **認証なし**: アプリケーションはローカル/LAN専用に設計。API認証は不要。

**実装レイヤー**:

| レイヤー | 言語 | 役割 | 例 |
|---------|------|------|-----|
| Rustコア | Rust | メインプロセス、すべてのコア処理 | イベントバス、シリアル通信、画像処理 |
| PyO3バインディング | Rust（Pythonに公開） | Python API提供 | `pokecon.events`, `pokecon.dialogue` |
| Python互換レイヤー | Python 3.14+（最小限） | 将来の実装切り替え用フック | `CommandMeta`（`_meta.py`のみ） |
| Luaランタイム | LuaJIT 2.1 | 動的設定（`init.lua`）の実行 | `pokecon.autocmd`, `pokecon.keymap` |

**言語仕様**:
- **Python**: 3.14以上をターゲット。ランタイムは3.14を使用するため、3.14で使用可能な記法を必須とする。可能な限り3.13にも存在する記法を使用し、3.14時点で非推奨・廃止予定の機能、および3.15/3.16で非推奨・廃止予定の機能は使用しない。PEP 695型パラメータ、basedpyrightによる厳格な型チェックを使用
- **Lua**: LuaJIT 2.1をターゲット。動的設定用のスクリプト言語として使用

**注**: ユーザースクリプトや動的設定（`init.py`/`init.lua`）から呼び出されるAPIは、原則としてPyO3（Rust製）で実装される。Pythonファイル（`commands.py`, `events.py`等）は型ヒント・ドキュメント・互換レイヤーのみを提供し、実際の処理はRust側で行う。

### 1.3 対象プラットフォーム

| プラットフォーム | UIモード | 備考 |
|----------|---------|-------|
| デスクトップ（Windows/Linux） | Tauri（WebViewラッパー） | Webモードとaxum HTTPサーバーを共有 |
| Webブラウザ | スタンドアロンSvelteKit SPA | axum HTTPサーバーによって提供 |
| モバイル（将来） | レスポンシブSPA | 同一コードベース、アダプティブレイアウト |

**注**: macOSは現時点では対象外。TauriのWebKit/GTK依存によるCI問題（AGENTS.md参照）により、将来的な対応を検討。

---

## 2. 用語集

| 用語 | 定義 |
|------|------|
| **フラット構造** | ドット区切りの階層を持たない、単一レベルの属性アクセス方式。例: `pokecon.opt.camera_fps`（フラット）vs `pokecon.opt.camera.fps`（階層） |
| **Neovim風** | Neovimエディタの設定・キーマッピング方式を模した設計。イベント名の`Pre`/`Post`後置（`BufReadPre`/`BufReadPost`に類似）、キー記法の`<C-a>`形式等 |
| **後勝ち** | 同じキー・設定に対して後から適用された値が優先される方式。設定の優先順位やキーマップの重複解決で使用 |
| **動的設定** | 実行時に評価される設定ファイル（`init.py`/`init.lua`）。イベントハンドラ登録やカスタムロジックを含む |
| **静的設定** | 起動時に読み込まれる設定ファイル（`settings.toml`）。TOML形式で、グローバル設定やプロファイル管理を含む |
| **Pre/Postフェーズ** | イベントの実行前（Pre）と実行後（Post）の2つのフェーズ。イベント名に`Pre`/`Post`を後置して区別 |
| **XDG Base Directory** | Linux/Unix系の設定・データ・キャッシュディレクトリの標準規格。`~/.config/`（XDG_CONFIG_HOME）、`~/.local/share/`（XDG_DATA_HOME）等 |
| **HandlerId** | イベントハンドラの登録時に返される識別子。ハンドラの解除（`off()`）に使用 |
| **フォールバック** | プライマリ方式が利用できない場合に使用される代替方式。例: WebRTC不可時のWebSocketフォールバック |
| **デッドゾーン** | アナログスティック等の入力デバイスにおいて、中央付近の微小な入力を無視する領域 |
| **チャタリング** | 機械的な接点のバウンスにより、意図しない短時間の連続入力が発生する現象 |
| **シグナリング** | WebRTCにおいて、通信相手との接続確立に必要な情報（SDP、ICE candidate等）を交換するプロセス |
|
---

## 3. 非機能要件

### 3.1 パフォーマンス

| 指標 | 目標 |
|--------|--------|
| ビデオ遅延（WebRTC） | < 100ms |
| ビデオ遅延（WebCodecs + WebSocketフォールバック） | 50-200ms |
| コントローラー入力遅延 | < 50ms |
| UI応答性 | 60 FPSアニメーション、< 16ms入力応答 |

### 3.2 アクセシビリティ

- すべてのコントロールのキーボードナビゲーション。
- スクリーンリーダー用のARIAラベル。
- ハイコントラストモードのサポート。

### 3.3 ブラウザサポート

| ブラウザ | 最小バージョン |
|---------|----------------|
| Chrome/Edge | 90以上 |
| Firefox | 130以上（WebCodecs対応のため） |
| Safari | 16.4以上（WebRTC対応。WebCodecs VideoはSafari 16.4+で対応） |

### 3.4 WebSocket自動再接続

- 接続断時に、自動的に再接続を試行します。
- 再接続間隔、リトライ回数上限は設定で変更可能（§11.4「静的設定（settings.toml）」参照）。
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

### 4.4 設定ファイル — `settings.ini` 廃止

> **要件**: 従来の `settings.ini` は廃止されました。すべての設定は `settings.toml` に移行されました。
- 静的設定: `settings.toml`
- 動的設定: `init.py` / `init.lua`

### 4.5 PWA — 今後のバージョンで実装予定

> **要件**: PWAは今後のバージョンで実装予定です。詳細は§15.1を参照。

### 4.6 スクリプト互換性 — リファクタリング前の全スクリプトが動作必須

> **要件**: リファクタリング前のバージョンで動作していたすべてのスクリプトは、引き続き正常に動作する必要があります。スクリプトAPIに破壊的変更は加えません。

### 4.7 テーマサポート — 今後のバージョンで実装予定

> **要件**: テーマサポート（ライト/ダーク/カスタム）は今後のバージョンで実装予定です。詳細は§9を参照。Tailwind CSS v4はスタイリングフレームワークとして確定しています。

---

## 5. UIレイアウト

### 5.1 全体構造

```
+-------------------------------------------------------------+
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
|  |  [その他]             |  |  | 出力 #1                |  |  |
|  |                       |  |  +-----------------------+  |  |
|  |                       |  |                             |  |
|  |                       |  |  +-----------------------+  |  |
|  |                       |  |  | 出力 #2                |  |  |
|  |                       |  |  +-----------------------+  |  |
|  +----------------------+  +-----------------------------+  |
+-------------------------------------------------------------+
```

### 5.2 タブ構造（6メインタブ + 3サブタブ）

> **タブ数に関する注記**: 本仕様では、トップレベルに**6つのメインタブ**を記述します。Commandsタブには**3つのサブタブ**（Python Command、Mcu Command、Shortcut）が含まれ、合計9つの個別のタブ付きインターフェースとなります。PLAN.md（リポジトリルートの旧設計ドキュメント）では「8タブ構造」に言及していますが、これは旧設計であり、本仕様書の「6メインタブ + 3サブタブ」が最新の正しい定義です。本仕様では、トップレベルのノートブックタブを指す「6メインタブ」で一貫しています。

| # | タブ名 | 説明 |
|--|--------|-------------|
| 1 | **カメラ** | 映像表示（Canvas/CaptureArea）、デバイス選択、FPS、フリップ、表示モード切替、マウスベースのスティック制御、スクリーンショット |
| 2 | **シリアル** | COMポート選択、ボーレート、データ形式（3種類）、接続/切断、シリアルモニター |
| 3 | **手動制御** | ソフトウェア制御（キーボード、マウススティックエミュレーション）、ハードウェア制御（ProController/Xinput、録画）、完全なJoy-ConレイアウトのSwitch Controller Simulator |
| 4 | **コマンド** | 3サブタブ（Python Command、Mcu Command、Shortcut）、タグフィルター付きコマンドリスト、10ショートカットボタン、実行制御（開始/一時停止/再開/停止/再読み込み） |
| 5 | **通知** | Windows通知設定、Discord Webhook（URL、ユーザー名、アバター）、LINE UIは完全に削除 |
| 6 | **その他** | 出力サイズ調整、stdout出力先、ウィジェットモード選択、ソフトウェアコントローラー位置、ダイアログボタン位置、出力クリア |

### 5.3 右側パネル

#### 5.3.1 ソフトウェアコントローラー（Joy-Conレイアウト）

- **位置**: 右パネル内でTOP/BOTTOMを設定可能（その他タブのラジオボタンで選択）。
- **外観**: Joy-Con L（シアン `#56CCF2`）+ R（赤 `#E9514E`）レイアウト。
- **アクティブ色**: ボタンが押されている/保持されている間は黄色 `#FFD800`。
- **入力方法**:
  - **ホールド**: `<Button-1>` 押下でボタンホールドをトリガー（押下シグナル送信）。
  - **解放**: `<ButtonRelease-1>` でボタン解放をトリガー（解放シグナル送信）。
  - **Shift+解放**: `holdEndSkip` をトリガー — バックエンドに解放シグナルを送信せずに視覚的状態のみを切り替え。用途: ボタンを押したままの状態で別の操作を行いたい場合（例: Aボタン長押し中に別のボタンを短押し）。アプリケーション終了時やプロファイル切替時には、holdEndSkip中のボタンも含めて全てのボタンを強制解放する
  - **ブラウザ対応**: Shift+クリックによるブラウザのデフォルト動作（テキスト選択等）を防ぐため、`event.preventDefault()` を使用する
- **ボタン**: すべての標準Switchコントローラーボタン:
  - A、B、X、Y
  - L、R、ZL、ZR
  - MINUS（−）、PLUS（+）
  - HOME、CAPTURE
  - D-pad（上、下、左、右）
  - Lスティック（アナログ、0～255座標）
  - Rスティック（アナログ、0～255座標）
  - タッチスクリーンシミュレーション（320×240座標入力）
- **アナログスティック**: X軸およびY軸ともに0～255の範囲。中央位置にはデッドゾーンあり（値103～153はニュートラルとして扱われる）。マウスドラッグ中は最低16ms間隔またはブラウザの`requestAnimationFrame`に同期して送信。無操作時は送信停止。

#### 5.3.2 出力パネル

- **出力 #1**: プライマリログ/出力表示。
- **出力 #2**: セカンダリログ/出力表示。
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

### 5.4 サブタブ構造（コマンドタブ）

Commandsタブには3つのサブタブ（内部タブ）があります:

| # | サブタブ | 説明 |
|--|---------|-------------|
| 1 | **Python Command** | 利用可能なPythonコマンドスクリプトのリスト/ツリー |
| 2 | **Mcu Command** | 利用可能なMCUコマンドスクリプトのリスト/ツリー |
| 3 | **Shortcut** | 10ショートカットボタン割り当てグリッド |

### 5.5 ウィジェットモード（7種類）

UIは、その他タブのコンボボックスで選択可能な、右側パネルの7つの表示組み合わせをサポートする必要があります:

| モード | 内部識別子 | ソフトウェアコントローラー | 出力 #1 | 出力 #2 | 説明 |
|------|----------|---------------------|-----------|-----------|-------------|
| 1 | `default` | 表示 | 表示 | 表示 | フルパネル（デフォルト） |
| 2 | `simple` | 表示 | 表示 | 非表示 | 単一出力 |
| 3 | `gamepad` | 表示 | 非表示 | 表示 | 単一出力（入れ替え） |
| 4 | `mouse` | 非表示 | 表示 | 表示 | 出力のみ |
| 5 | `keyboard` | 表示 | 非表示 | 非表示 | コントローラーのみ |
| 6 | `custom1` | 非表示 | 表示 | 非表示 | 出力 #1のみ |
| 7 | `custom2` | 非表示 | 非表示 | 表示 | 出力 #2のみ |

---

## 6. タブ仕様

### 6.1 カメラタブ

#### 6.1.1 映像表示

- **表示方法**: 映像レンダリング用のCanvas要素（旧API互換性のため `CaptureArea` としても参照可能）。
- **プライマリストリーム**: WebRTCビデオトラック（低遅延）。
- **フォールバック**: WebRTCが利用できない場合、WebCodecs + WebSocket（ブラウザネイティブHWデコード）。
- **フレームレート**: FPS設定（SpinboxまたはCombobox）で設定可能。

#### 6.1.2 カメラ設定

| コントロール | 種類 | 説明 |
|---------|------|-------------|
| **カメラデバイス選択** | Combobox | 利用可能なカメラデバイスのドロップダウン |
| **FPS** | Combobox | UI表示用フレームレート（選択肢: 5, 15, 30, 60）。バックエンド処理FPSとは独立。選択肢は静的設定でカスタマイズ可 |
| **フリップ** | Checkbox | 水平/垂直フリップ切替 |

#### 6.1.3 表示モード切替（チェックボックス）

| モード | 説明 |
|------|-------------|
| **リアルタイム** | ライブ映像表示 |
| **値** | カーソル位置のピクセル値（RGB/HSV/座標）をオーバーレイ表示 |
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

- **バックエンド**: OpenCV（Windowsは `cv2.CAP_DSHOW`、Linuxは `cv2.CAP_V4L2`）。
- **スレッド**: フレームキャプチャは別スレッドで実行。

### 6.2 シリアルタブ

#### 6.2.1 接続制御

| コントロール | 種類 | 説明 |
|---------|------|-------------|
| **COMポート選択** | Combobox | 利用可能なCOM/シリアルポートのドロップダウン |
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
- **機能**: リアルタイムで入出力シリアルデータを表示。
- **機能**: 最新エントリへの自動スクロール、クリアボタン。

### 6.3 手動制御タブ

#### 6.3.1 ソフトウェア制御セクション

| コントロール | 種類 | 説明 |
|---------|------|-------------|
| **キーボード** | Checkbox | キーボードベースのコントローラー入力を有効化（グローバルホットキー） |
| **Lスティックマウス** | Checkbox | キャンバス上の左アナログスティックのマウスエミュレーションを有効化 |
| **Rスティックマウス** | Checkbox | キャンバス上の右アナログスティックのマウスエミュレーションを有効化 |

#### 6.3.2 ハードウェア制御セクション

> **ステータス: 今後のバージョンで実装予定**

| コントロール | 種類 | 説明 |
|---------|------|-------------|
| **ProController** | Radio（Xinput連動） | Switch Pro Controller入力モード |
| **Xinput** | Radio（ProController連動） | Xbox互換コントローラー入力モード |
| **録画** | Checkbox | 入力記録を有効化 |

**注**: ハードウェア制御はブラウザのAPI制約により実装が複雑なため、今後のバージョンで実装予定です。今回のリファクタリングではソフトウェア制御（キーボード、マウス）のみを実装します。UI上はハードウェア制御セクションを**非表示**とします（グレーアウトではなく非表示）。

#### 6.3.3 Switch Controller Simulator

タブコンテンツ領域に表示される完全なJoy-Conスタイルのボタンレイアウト:

- **D-Pad**（上、下、左、右 — 方向十字）
- **L / ZL**（ショルダーボタン、左側）
- **MINUS（−） / CAPTURE**（小ボタン、左中央）
- **A / B / X / Y**（フェイスボタン、右側）
- **R / ZR**（ショルダーボタン、右側）
- **PLUS（+） / HOME**（小ボタン、右中央）
- **LSTICK**（クリック可能なアナログスティック、左側、0～255座標、±10%デッドゾーン）
- **RSTICK**（クリック可能なアナログスティック、右側、0～255座標、±10%デッドゾーン）
- **タッチスクリーン**（タッチエミュレーション用320×240座標グリッド）

### 6.4 コマンドタブ

#### 6.4.1 サブタブ構造

Commandsタブには3つのサブタブ（内部タブ切替）が含まれます:

| サブタブ | 内容 |
|---------|---------|
| **Python Command** | 利用可能なPythonコマンドスクリプトを一覧表示 |
| **Mcu Command** | 利用可能なMCUコマンドスクリプトを一覧表示 |
| **Shortcut** | 10ショートカットボタン割り当てグリッド |

#### 6.4.2 コマンドリスト

- **表示**: 利用可能なコマンドを表示するTreeview（階層構造を持つコマンドリスト）。
- **タグフィルター**: ラベル/タグでコマンドをフィルタリングするドロップダウンまたはコンボボックス。
- **列**: コマンド名、タグ、説明（Treeviewの場合）。

##### タグ体系

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
- Pythonモジュールパスから自動生成（`mod.__name__.split(".")[2:-1]` で中間ディレクトリ名を抽出）
- `@` プレフィックスを付与（例: `@Samples`, `@RankGlitch`）
- ネストしたディレクトリ構造に対応（例: `Commands.PythonCommands.Samples.RankGlitch.MashA` → `["@Samples", "@RankGlitch"]`）
- 元のディレクトリ名をそのまま使用（PascalCase変換なし）

**手動タグ（クラス属性）**:
- コマンドクラスの `TAGS` クラス属性で定義
- `list[str] | None` で指定可能
- `None` の場合は「タグなし」（空リストとして扱う）
- `@` プレフィックスは付かない（慣例）

**動的タグ（イベントによる追加）**:
- `ScriptLoadPre` イベントのコールバックで `pokecon.state.command_candidates` を変更することで追加可能（§11.12.5参照）
- コールバックは引数なし、`pokecon.state` に直接アクセスして変更
- 自動タグと手動タグは統合され、コマンドクラスの `TAGS` 属性に書き戻される

**動的タグ追加の例**:
```python
# init.py での動的タグ追加例
def add_dynamic_tags():
    for candidate in pokecon.state.command_candidates:
        if candidate.name.startswith("Auto"):
            candidate.tags.append("@Auto")

pokecon.autocmd.on("ScriptLoadPre", callback=add_dynamic_tags)
```

**タグの統合順序と具体例**:

```
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
- Pythonクラス属性とRust側のデータ構造の両方に書き戻す
- Python側はユーザースクリプトからの参照のみ
- Rust側は動的設定や実際のバックエンド処理に使用

**タグの重複**:
- 同じタグが複数回追加された場合、自動的に重複を除去
- 統合後のタグ一覧はユニークなリストとなる

**フィルター動作**:
- デフォルトは完全一致（`selected_tag == tag`）
- マッチング方式は設定で切り替え可能:
  - **静的設定** (`settings.toml`): `exact`（完全一致） / `partial`（部分一致） / `prefix`（前方一致） / `suffix`（後方一致）
  - **動的設定** (`init.py` / `init.lua`): カスタムマッチ関数を指定可能
    ```python
    # カスタムマッチ関数の例
    def custom_match(selected: str, tag: str) -> bool:
        return selected.lower() in tag.lower()
    
    pokecon.ui.tag_match_function = custom_match
    ```
- **バックエンド側の責務**: タグフィルターのマッチング（完全一致/部分一致/前方一致/後方一致/カスタム関数）。マッチング結果はコマンドリストの表示/非表示を制御
- **フロントエンド側の責務**: ファジーファインダーによる絞り込み（`fuse.js` を使用した部分一致スコアリング）。これはUI上の利便性向上のための補助機能であり、バックエンドのマッチング方式とは独立して動作する。フロントエンドの絞り込みはバックエンドのマッチング結果に対してさらにフィルタをかける2段階方式
- UI上では `@` なしタグが先、`@` 付きタグが後に表示
- ソート関数は動的設定ファイルで `pokecon.ui.tag_sort_function = my_sort_func` のように指定可能
  - 型: `Callable[[list[str]], list[str]]`
  - 動的設定（Lua）からも同様に指定可能: `pokecon.ui.tag_sort_function = my_sort_func`
- 先頭に `"-"`（フィルター無効）を配置

**タグ専用のstate**:
- `pokecon.state.tags`: タグ一覧のみ（UI表示、フィルター選択肢生成用）
  - 型: `list[str]`
- `pokecon.state.command_candidates`: コマンド候補 + タグリスト（動的タグ追加、フィルタリング、実行用）
  - 型: `list[CommandInfo]`
- タグとコマンドの紐づけはコマンド側で管理

#### 6.4.3 ショートカットボタン（10ボタン）

- **数**: 10ショートカットボタン（要件が元の4ボタンから10ボタンに変更）。
- **割り当て**: 読み込まれた任意のコマンドにユーザー割り当て可能（クリックで割り当て、Shift+クリックで割り当て解除）。
- **クリア**: 右クリックで割り当てをクリア。
- **表示**: ボタンラベルに割り当てられたコマンド名を表示。
- **キーボードショートカット**: F1～F10またはその他の単一キー（修飾キーなし）。修飾キー付きのショートカットはキーバインドシステム（§11.14）で管理。
- **ショートカット割り当て方法**: ショートカットボタンをクリック → コマンドリストからコマンドを選択 → 割り当て完了。Shift+クリックで割り当て解除。右クリックでクリア。
- **キーバインド競合**: ショートカットボタンのホットキーと他のキーバインドが重複した場合、後から登録されたものが優先（後勝ち）。実行制御キー（§11.14.5）はデフォルトで未割り当てのため、通常は競合しない
- **実行制御キーとの関係**: ショートカットボタンのF1〜F10は「ボタン押下」システム（UI上のボタンクリックと同等）、実行制御キー（§11.14.5）は「キーマップシステム」（`pokecon.keymap.set()` で登録）で管理。両者は別システムであり、同じFキーが両方に割り当てられていた場合、キーマップシステム（後勝ち）が優先される。デフォルトでは実行制御キーは未割り当て（§11.14.5参照）
- **保存**: 設定は `localStorage` に保存。

#### 6.4.4 実行制御ボタン

| ボタン | アクション |
|--------|------------|
| **開始** | コマンド実行を開始 |
| **停止** | 実行を停止 |
| **一時停止** | 実行を一時停止（再開可能） |
| **再開** | 一時停止から再開 |
| **リロード（再読み込み＋開始）** | コマンドを再読み込みして開始 |
| **コマンドリスト再読み込み** | ファイルシステムからコマンドリストを再読み込み |

キーボードショートカットの割り当ては **§6.4.3 ショートカットボタン** を参照。

- **状態表示**: 実行中 / 一時停止中 / 停止 / エラー。
- **進捗**: 対応コマンド用のプログレスバー。

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
- **後方互換性**: 既存のユーザースクリプト用にスクリプトAPI（`notify.line`）は維持。設定用のUIはなし。

### 6.6 その他タブ

#### 6.6.1 設定グループ

| セクション | コントロール | 種類 |
|---------|----------|------|
| **出力サイズ調整** | 出力#1と出力#2の幅比率を制御するスライダー（0〜100）。スライダー値は内部で10%〜90%にマッピングされる（`ratio_1 = 10 + slider * 0.8`、`ratio_2 = 100 - ratio_1`）。最小でも各出力は10%の幅を確保 | Scale/Slider |
| **stdout出力先** | stdout出力の出力先を選択する出力#1/出力#2ラジオボタン | Radio button |
| **出力をクリア** | 両方の出力パネルをクリアするボタン | Button |
| **ウィジェットモード** | 7モードのコンボボックス（§5.5参照） | Combobox |
| **ソフトウェアコントローラーの位置** | 右パネル内の位置を指定するTOP/BOTTOMラジオボタン | Radio button |
| **ダイアログボタンの位置** | ダイアログボタン配置用のTOP/BOTTOM/BOTHラジオボタン | Radio button |

---

## 7. 通信プロトコル（Web / ネットワーク）

### 7.1 スタック概要

```
カメラ映像:     WebRTCビデオトラック ──→ WebCodecs + WebSocket フォールバック
コントローラー入力: WebRTC DataChannel ──→ WebSocket フォールバック
ログ/イベント:  WebRTC DataChannel ──→ WebSocket フォールバック
API呼び出し:    HTTP REST（axum）     ──→ （フォールバック不要）
```

### 7.2 WebRTC（プライマリ）

- **ビデオ**: ビデオトラックを使用したWebRTC `RTCPeerConnection`。
- **DataChannel**: コントローラー入力イベントとログストリーミング用。
- **シグナリング**: HTTPベースのSDP交換。WebSocket上のJSONメッセージでOffer/Answer/ICE candidateを交換。STUNサーバー: `stun:stun.l.google.com:19302`（デフォルト）。コーデック優先順位: H.264 > VP8 > VP9。
- **自動再接続**: §3.4「WebSocket自動再接続」参照。
- **フォールバック条件**: 
  - WebRTC接続が5秒以内に完了しない → WebSocketフォールバック起動
  - 接続確立後、3秒間連続でフレーム/データが受信できない → WebSocketにフォールバック
  - フォールバック中のWebRTC復旧検出は行わない（手動再接続を促す）

**通信内容**:

| 種類 | 内容 | フォールバック |
|------|------|--------------|
| 映像 | WebRTCビデオトラック | WebCodecs + WebSocket |
| コントローラー入力 | WebRTC DataChannel | WebSocket |
| ログ | WebRTC DataChannel | WebSocket |
| API呼び出し | HTTP REST | なし（HTTP必須） |

### 7.3 WebCodecs + WebSocket（フォールバック）

#### 7.3.1 映像フォールバック — WebCodecs

映像フォールバックとして、ブラウザネイティブの **WebCodecs API** を使用した低遅延ストリーミングを採用します。

- **エンコーダー**: サーバーサイド（Rust/ffmpeg）でH.264/HEVC/AV1にエンコード。
- **転送**: WebSocket経由でエンコード済みビデオフレーム（アクセスユニット）を送信。
- **デコード**: ブラウザの **VideoDecoder**（WebCodecs）でHWデコードを利用。
- **描画**: デコード結果を **Canvas** または **VideoFrame** に描画。

**特性**:

| 項目 | 値 |
|------|------|
| 遅延 | 50-200ms（MJPEG比で50%以上改善） |
| エンコード | H.264（優先）/ HEVC / AV1（サーバーが対応可能なコーデックを自動選択） |
| ABR対応 | 固定品質（CRF）でエンコード。帯域測定と品質段階自動選択は将来の拡張 |
| HWデコード | ブラウザネイティブのハードウェアデコードを活用（CPU負荷低減） |

**ブラウザサポート**:

| ブラウザ | 対応状況 |
|---------|---------|
| Safari 16.4+ | Video対応（Audioは将来のSafariバージョンで対応予定） |
| Chrome 94+ | 完全対応 |
| Firefox 130+ | 対応（Video + Audio） |
| Edge 94+ | 完全対応 |

**WebCodecsの利点**:
- ブラウザネイティブのHWデコードにより低CPU負荷で高品質映像を実現
- 可変ビットレート・解像度制御によりネットワーク変動に適応
- MJPEGと比較して帯域使用率を60-80%削減

#### 7.3.2 コントロール/ログフォールバック — WebSocket

- **エンドポイント**: `/ws`。
- **メッセージ**: JSON形式。
- **自動再接続**: §3.4「WebSocket自動再接続」参照。
- **イベント**:

| イベント | 方向 | ペイロード |
|-------|-----------|---------|
| `camera.opened` | サーバー → クライアント | カメラオープン通知（`{"device_id": str, "resolution": [int, int]}`） |
| `command.start` | サーバー → クライアント | コマンド実行開始通知 |
| `command.stop` | サーバー → クライアント | コマンド実行停止通知 |
| `command.error` | サーバー → クライアント | コマンド実行エラー詳細 |
| `serial.data` | サーバー → クライアント | シリアルポート受信データ |
| `ping` | 双方向 | キープアライブping |
| `pong` | 双方向 | キープアライブpong応答 |

### 7.4 HTTP REST API

- **フレームワーク**: axum（Rustバックエンド）。
- **ドキュメント**: OpenAPI仕様を使用したutoipa v5。
- **コード生成**: TypeScriptクライアント型用の `openapi-typescript`。生成失敗時は前回成功時の生成結果をフォールバックとして使用（git追跡）。CIでは型生成ジョブが独立して失敗することを許容し、アラートのみ行う
- **認証**: なし（ローカル/LAN専用）。
- **セキュリティ**: Originヘッダーの検証または同一発行元ポリシー（Same-Origin）による保護。CORS設定: `Access-Control-Allow-Origin` は `localhost:*` のみ許可（デフォルト）
- **モジュール**: 複数のモジュールに分かれたREST API。
- **応答形式**: 一貫した構造のJSON。

**注**: 本要求仕様ではAPIの概要のみを記載します。具体的なエンドポイント定義はOpenAPI自動生成に従い、別途API仕様書として管理します。

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

**設定取得/変更**（HTTP REST — 設定変更のみ）:

| メソッド | エンドポイント | 説明 |
|--------|----------|-------------|
| GET | `/api/controller/keyboard` | 現在のキーボード設定を取得 |
| POST | `/api/controller/keyboard` | キーボード設定を設定 |

- **保存**: キーボード設定は `localStorage` に保存。

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

**設定取得/変更**（HTTP REST — 設定変更のみ）:

| メソッド | エンドポイント | 説明 |
|--------|----------|-------------|
| GET | `/api/controller/type` | 現在のゲームパッドタイプ設定を取得 |
| POST | `/api/controller/type` | ゲームパッドタイプを設定（`gamepad_type: "ProController" | "Xinput"`） |

- **対応ボタン**: A、B、X、Y、UP、DOWN、LEFT、RIGHT、L、R、ZL、ZR、MINUS、PLUS、HOME、CAPTURE。
- **アナログスティック**: 両軸とも0～255の範囲。
- **タッチパッド**: `{x: 0–320, y: 0–240}` 座標。

---

## 8. 型システム

### 8.1 OpenAPI → TypeScript

- **ソース**: `utoipa` v5マクロを使用したRustバックエンド。
- **生成**: `openapi-typescript` CLI。
- **出力**: `src/lib/api/openapi.ts`。
- **使用法**: すべてのAPI呼び出しとWebSocketメッセージは生成された型を使用する必要があります。

**ワークフロー**:

```bash
# 1. RustバックエンドでOpenAPI JSONを生成（ビルド時に自動実行）
cargo build

# 2. openapi-typescriptでTypeScript型を生成
npx openapi-typescript http://localhost:3000/api-docs/openapi.json -o src/lib/api/openapi.ts

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
- 動的設定専用API（`pokecon.keymap.trigger()` 等）は§11「設定ファイルシステム」に記載
- 内部実装の名前空間は本仕様で規定するものではない。ユーザーがアクセスできるAPI名のみを規定する

### 10.2 設計方針

- **コアはRust**: すべてのコア処理はRustで実装。Pythonは必要な部分のみ（ユーザースクリプトAPI、互換レイヤー）。
- **メタクラスによる切り替え**: `CommandMeta`が将来の実装切り替え用フックを提供。現状はすべてPyO3（Rustバインディング）に流れる。
- **後方互換性**: リファクタリング前のスクリプトは変更なしで動作する必要がある。
- **型ヒント**: 新APIは動作する型ヒントを持つ。旧APIは互換性のために保持され、新APIと同等の完成度・品質でメンテナンスされる。一般ユーザーには新APIの使用を推奨するが、開発時の扱いは新APIと変わらない。
- **内部実装の命名**: ユーザースクリプトに公開するAPI（新ダイアログAPI `show_dialog` 以外）は、互換性のため全く同じ名前でアクセスできる必要がある。アクセスできれば内部の命名は自由（妥当なものであれば）。

### 10.3 公開モジュール

**注**: 以下のモジュールはRust/PyO3で実装され、Pythonファイルは型ヒント・ドキュメント・互換レイヤーのみを提供する。実際の処理はRust側で行われる。

| モジュール | 内容 | ユーザースクリプトでのImport例 |
|-----------|------|------------------------------|
| `dialogue` | ダイアログ関数 | `from Commands import dialogue` |
| `image_proc` | 画像処理（opencv-rust）。詳細は §10.4.3 ImageProcPythonCommand 参照 | `from Commands import image_proc` |
| `net` | Socket、MQTT、HTTPクライアント。詳細は §10.4.2 PythonCommand（Socketメソッド）参照 | `from Commands import net` |

**注**: `events.py`（動的設定用EventBus）は§11「設定ファイルシステム」に含まれる。

### 10.4 コマンドクラス

#### 10.4.1 クラス階層

```
Command (metaclass=CommandMeta)
├── PythonCommand
│   └── ImageProcPythonCommand
└── McuCommandBase
```

**注**: すべてのクラスは `CommandMeta` メタクラスを使用。`PythonCommand` と `ImageProcPythonCommand` は抽象メソッド `do()` を持つが、Pythonの `ABC` クラスを継承しない（`CommandMeta` で抽象クラスとして扱われる）。ユーザースクリプトでは `PythonCommand` または `ImageProcPythonCommand` を継承して `do()` を実装する。

#### 10.4.2 PythonCommand

**Import**: `from Commands.PythonCommandBase import PythonCommand`

**ライフサイクルメソッド**:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `do()` | `do() -> None` | 抽象 — 自動化ロジックをオーバーライド |
| `finish()` | `finish() -> None` | スクリプトを正常停止 |
| `checkIfAlive()` | `checkIfAlive() -> Literal[True]` | 停止フラグ確認；終了時は`StopThread`を送出 |

**入力メソッド**:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `press()` | `press(buttons: Button \| list[Button], duration=0.1, wait=0.1)` | ボタン押下（指定秒数保持後解放） |
| `pressRep()` | `pressRep(buttons: Button \| list[Button], repeat, duration=0.1, interval=0.1, wait=0.1)` | 繰り返し押下 |
| `hold()` | `hold(buttons: Button \| list[Button], wait=0.1)` | ボタンを押下状態で保持 |
| `holdEnd()` | `holdEnd(buttons: Button \| list[Button])` | 保持中のボタンを解放 |
| `wait()` | `wait(wait: float)` | wait秒スリープ |
| `short_wait()` | `short_wait(wait: float)` | ビジーループ待機（高精度） |
| `direct_serial()` | `direct_serial(commands: list[str], waittimes: list[float])` | 生シリアルコマンド送信 |
| `reload_com_port()` | `reload_com_port()` | COMポート接続を再読み込み |

**出力メソッド**:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `print_t1()` | `print_t1(*objects, sep=' ', end='\\n')` | 上部ログパネルへ出力 |
| `print_t2()` | `print_t2(*objects, sep=' ', end='\\n')` | 下部ログパネルへ出力 |
| `print_t()` | `print_t(*objects, sep=' ', end='\\n')` | stdoutではない方のログパネルへ出力。`stdout_destination` の設定により動的に出力先を切り替える（"1"→出力#2、"2"→出力#1） |
| `print_s()` | `print_s(*objects, sep=' ', end='\\n')` | stdout割り当てパネルへ出力 |
| `print_ts()` | `print_ts(*objects, sep=' ', end='\\n')` | `print_s`と同じ（歴史的経緯で同じ動作のものが複数存在） |
| `print_t1b()` | `print_t1b(mode, *objects, sep=' ', end='\\n')` | 上部ログ（モード付き: w=上書き, a=追記, d=削除） |
| `print_t2b()` | `print_t2b(mode, *objects, sep=' ', end='\\n')` | 下部ログ（モード付き） |
| `print_tb()` | `print_tb(mode, *objects, sep=' ', end='\\n')` | stdout以外ログ（モード付き） |
| `print_tbs()` | `print_tbs(mode, *objects, sep=' ', end='\\n')` | stdout割り当てパネルへ出力（モード付き: w=上書き, a=追記, d=削除） |
| `show_var()` | `show_var()` | 内部変数の一覧をログパネルに表示。一時停止時に自動で呼び出されるほか、ユーザースクリプト内から手動で呼び出し可能。表示対象は `self` に定義した変数のみ（`keys`, `thread`, `_logger` 等の内部変数は除外） |

**ダイアログメソッド**（ブロッキングWebポップアップ）:

| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `dialogue()` | `dialogue(title: str, message: str, desc: str | None = None, need: list[str] | None = None)` | 単純入力ダイアログ（互換性維持） |
| `dialogue6widget()` | `dialogue6widget(title: str, dialogue_list: list[Any], desc: str | None = None, need: list[str] | None = None)` | マルチウィジェットダイアログ（互換性維持） |

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
| `discord_text()` | `discord_text(content='', index=0, keys='DISCORD_WEBHOOK')` | Discord webhook経由でテスト送信 |
| `discord_image()` | `discord_image(content='', index=0, crop_fmt='', crop=None, keys='DISCORD_WEBHOOK')` | Discord webhook経由でテキスト+スクリーンショット送信 |
| `LINE_text()` | `LINE_text(txt: str, token: str = '')` | No-opスタブ（LINEサービスEOL）。呼び出しても何も起こらず、WARNINGログを出力 |
| `LINE_image()` | `LINE_image(txt: str, crop_fmt: str = '', crop: Any = None, token: str = '')` | No-opスタブ（LINEサービスEOL）。呼び出しても何も起こらず、WARNINGログを出力 |
| `win_notification()` | `win_notification()` | Windowsデスクトップトースト通知 |

#### 10.4.3 ImageProcPythonCommand

**Import**: `from Commands.PythonCommandBase import ImageProcPythonCommand`

`PythonCommand`を拡張し、カメラと画像処理機能を追加。

**トリミングパラメータ**:
- `crop_fmt: str` — トリミング形式。以下の値を指定:
  - `""` (デフォルト): トリミングなし
  - `"1"`: Pillow形式 [x_start, y_start, x_end, y_end]
  - `"2"`: Pillow形式 [x_start, y_start, width, height]
  - `"3"`: Pillow形式 [x_start, x_end, y_start, y_end]
  - `"4"`: Pillow形式 [x_start, width, y_start, height]
  - `"11"`: OpenCV形式 [y_start, x_start, y_end, x_end]
  - `"12"`: OpenCV形式 [y_start, x_start, height, width]
  - `"13"`: OpenCV形式 [y_start, y_end, x_start, x_end]
  - `"14"`: OpenCV形式 [y_start, height, x_start, width]
- `crop: list[int] | None` — トリミング座標のリスト。`crop_fmt` に応じた4要素の整数リスト。`None`または空リストの場合はトリミングなし

**コンストラクタ**: `ImageProcPythonCommand(cam, gui=None)`

**画像処理メソッド**（Rust opencv-rust実装）:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `isContainTemplate()` | `isContainTemplate(template_path, threshold=0.7, use_gray=True, crop_fmt='', crop=None)` | カメラフレームに対するテンプレートマッチング |
| `isContainTemplate_max()` | `isContainTemplate_max(template_path_list, threshold=0.7, use_gray=True, crop_fmt='', crop=None)` | マルチテンプレートマッチング |
| `isContainTemplateGPU()` | `isContainTemplateGPU(template_path, threshold=0.7, use_gray=True, crop_fmt='', crop=None)` | `isContainTemplate()`と同じ処理。関数名は互換性のために維持。GPUは使用しない |
| `isContainedImage()` | `isContainedImage(image_path, threshold=0.7, use_gray=True, crop_fmt='', crop=None)` | 逆テンプレートマッチング |
| `saveCapture()` | `saveCapture(filename=None, crop_fmt='', crop=None, mode=True)` | カメラフレームを./Captures/へ保存 |
| `popupImage()` | `popupImage(crop_fmt='', crop=None, title='image')` | カメラフレームをポップアップ表示 |
| `getCameraImage()` | `getCameraImage(crop_fmt='', crop=None)` | カメラフレームをOpenCV画像配列で取得 |
| `openImage()` | `openImage(filename, mode='t')` | 画像ファイルを読み込み |
| `setTemplateDir()` | `setTemplateDir(path)` | テンプレート画像ディレクトリを変更 |
| `get_filespec()` | `get_filespec(filename, mode='t')` | 相対ファイル名をフルパスに解決 |
| `displayRectangle()` | `displayRectangle(max_loc, width, height, tag=None, ms=2000, color=None, crop_fmt='', crop=None)` | GUIキャンバスオーバーレイに矩形描画 |
| `displayText()` | `displayText(position, txt, tag=None, ms=2000, font='UD デジタル 教科書体 NP-B', fontsize=20, color='black')` | GUIキャンバスオーバーレイにテキスト描画 |

**内部関数**（互換レイヤー用に`_`プレフィックスで公開）:
| 関数 | シグネチャ | 説明 |
|------|-----------|-------------|
| `_template_match()` | `_template_match(image, template, threshold, use_gray, ...)` | コアテンプレートマッチング |
| `_grayscale()` | `_grayscale(image)` | グレースケール変換 |
| `_resize()` | `_resize(image, width, height)` | 画像リサイズ |

#### 10.4.4 McuCommandBase

**Import**: `from Commands.McuCommandBase import McuCommandBase`

ファームウェアベースコマンド用。PythonCommandと同じメタクラス切り替え。

**コンストラクタ**: `McuCommandBase(sync_name: str)`

- `sync_name`: ファームウェアとの同期に使用するコマンド名。`ser.writeRow(sync_name)` で送信される

**メソッド**:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `start()` | `start(ser: Sender, postProcess: Callable[[], None] | None) -> None` | コマンド開始。`sync_name` をシリアル送信し、`isRunning = True` を設定 |
| `end()` | `end(ser: Sender) -> None` | コマンド終了。`"end"` をシリアル送信し、`isRunning = False` を設定。`postProcess` が設定されていれば実行 |

**ライフサイクル**:
1. `McuCommandBase("command_name")` でインスタンス作成
2. `start(ser, postProcess)` でコマンド開始（ファームウェアに `sync_name` を送信）
3. ファームウェア側で処理実行
4. `end(ser)` でコマンド終了（ファームウェアに `"end"` を送信）

**注**: McuCommandはPythonCommandとは異なり、Pythonコード内で処理を実行するのではなく、ファームウェア（マイコン）側で処理を実行する。Python側はコマンドの開始・終了のシグナル送信のみを担当。

### 10.5 キー入力・シリアル送信

#### 10.5.1 KeyPress

- ユーザースクリプトに**直接公開されない**
- `self.keys.neutral()`のみアクセス可能（コントローラーをニュートラル状態にリセット）
- 内部実装はRust、PyO3経由で公開
- **注**: `self.keys` は互換性維持のための旧API。内部実装は別の名前（例: `self._controller`）でも構わない。ユーザースクリプトからは引き続き `self.keys` を使用する

#### 10.5.2 Sender

**PyO3実装**（限定公開API）:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `writeRow()` | `writeRow(row: str)` | シリアル行を書き込み（末尾に改行を自動追加） |
| `ser.write()` | `ser.write(data: bytes)` | 直接シリアル書き込み（PyO3でpySerial互換型変換）。`data` は `bytes` 型のみ受け付ける |

**注**: `ser.write()` の引数 `data` は `bytes` 型。`str` を渡す場合は事前にエンコードが必要（`data.encode('utf-8')`）。

その他のSenderメソッドはSenderクラスとして公開されず、適切な他クラスに統合。

### 10.6 ダイアログAPI

**旧API（互換性維持）**: `dialogue()`、`dialogue6widget()` — 互換性のために保持。旧APIも新APIと同等の完成度・品質でメンテナンスされる。一般ユーザーには新APIの使用を推奨するが、開発時の扱いは新APIと変わらない。

**新API（推奨）**: `show_dialog()` — 事前に作成したWidgetインスタンスを渡す方式。型安全性と一貫性が向上。

**API分類と扱い**:
- **新API**: 積極的に推奨。新規スクリプトではこちらを使用
- **旧API（この実装独自機能）**: 互換性維持のため残す。APIシグネチャの変更はユーザーからの使用状況を考慮する必要がある。API変更を伴わない内部的な改善（ログメッセージの形式変更等）は自由に行える
- **旧API（他実装との互換性必須）**: 他のPoke-Controller互換ソフトとの互換性維持が必要。APIシグネチャの変更は慎重に行う。API変更を伴わない内部的な改善は自由に行える

#### 10.6.1 ダイアログライフサイクル

**スクリプト停止時の挙動**: ユーザースクリプトが停止（Stopボタン、エラー、強制終了など）した場合、スクリプト内で作成したすべてのダイアログ（ブロッキング・非ブロッキング・`wait_dialog`待機中を含む）を自動で閉じる。

**不正終了時の挙動**: OKボタン以外でダイアログが閉じられた場合（×ボタン、Escキー、ダイアログの強制終了など）、ユーザースクリプトを停止する。スクリプト停止に伴い、スクリプト内のすべてのダイアログが自動で閉じられる（上記「スクリプト停止時の挙動」を参照）。

**注**: Escキーでの閉じた場合は確認ダイアログを表示し、ユーザーが意図的に停止することを確認する。強制終了（ウィンドウの完全削除等）のみ即座にスクリプトを停止する。

```python
from typing import Generic, TypeVar, overload, Literal

T = TypeVar('T')

class Widget(Generic[T]):
    @overload
    def __init__(self: "Widget[str]", widget_type: Literal["Entry"], label: str, default: str) -> None: ...
    
    @overload
    def __init__(self: "Widget[bool]", widget_type: Literal["Check"], label: str, default: bool) -> None: ...
    
    @overload
    def __init__(self: "Widget[T]", widget_type: Literal["Combo"], label: str, options: list[T], default: T) -> None: ...
    
    @overload
    def __init__(self: "Widget[int]", widget_type: Literal["Spin"], label: str, options: list[int], default: int) -> None: ...
    
    @overload
    def __init__(self: "Widget[int]", widget_type: Literal["Spin"], label: str, min: int, max: int, default: int) -> None: ...
    
    @overload
    def __init__(self: "Widget[float]", widget_type: Literal["Scale"], label: str, min: float, max: float, default: float) -> None: ...
    
    @overload
    def __init__(self: "Widget[str]", widget_type: Literal["Next"], label: str, default: str) -> None: ...
    
    def __init__(self, widget_type: str, label: str, *args: object, **kwargs: object) -> None:
        self.widget_type = widget_type
        self.label = label
        self.value: T | None = None  # ダイアログ後に結果を格納

# 事前にWidgetインスタンスを作成
entry = Widget("Entry", "名前", "デフォルト")  # Widget[str]
check = Widget("Check", "有効", True)  # Widget[bool]

# ブロッキング（デフォルト）
dialog_id = pokecon.dialogue.show_dialog("タイトル", widgets=[entry, check])
# dialog_id == 0
print(entry.value)  # str
print(check.value)  # bool

# 非ブロッキング
dialog_id = pokecon.dialogue.show_dialog("タイトル", widgets=[entry, check], blocking=False)
# dialog_id > 0（固有の自然数）
# スクリプトの実行は継続される

# ダイアログが終了したか確認
if pokecon.dialogue.is_dialog_closed(dialog_id):
    print(entry.value)

# ブロッキング動作に切り替え
pokecon.dialogue.wait_dialog(dialog_id)
print(entry.value)
```

#### 10.6.2 ブロッキング（デフォルト）

- `show_dialog(title: str, widgets: list[Widget] | Widget, blocking: bool = True) -> int`
- `blocking=True`の場合、ダイアログが閉じられるまでスクリプトの実行を停止
- 返り値は`0`
- 結果は各Widgetの`value`属性に格納される
- **ライフサイクル**: §10.6.1「ダイアログライフサイクル」を参照

#### 10.6.3 非ブロッキング

- `show_dialog(title: str, widgets: list[Widget] | Widget, blocking: bool = False) -> int`
- `blocking=False`の場合、ダイアログを表示し、スクリプトの実行を継続
- 返り値はユーザースクリプトが開始してから停止するまでの間で固有の自然数（ダイアログID）
- 結果は各Widgetの`value`属性に格納される
- **ライフサイクル**: §10.6.1「ダイアログライフサイクル」を参照

#### 10.6.4 ダイアログ状態確認

- `is_dialog_closed(dialog_id: int) -> bool`
- 指定したダイアログIDのダイアログが終了しているかどうかを確認
- 終了していれば`True`、表示中または未表示であれば`False`
- **ライフサイクル**: §10.6.1「ダイアログライフサイクル」を参照

#### 10.6.5 ダイアログ待機

- `wait_dialog(dialog_id: int) -> None`
- 指定したダイアログIDのダイアログが終了するまでブロッキングで待機
- 非ブロッキングで表示したダイアログを後からブロッキング動作に切り替える際に使用
- **ライフサイクル**: §10.6.1「ダイアログライフサイクル」を参照

---

### 10.7 スクリプト互換性要件

| 要件 | 状態 |
|------|------|
| サンプルスクリプトが変更なしで動作 | ✅ 必須 |
| `from Commands.PythonCommandBase import PythonCommand` | ✅ モジュールパッチで保持 |
| `from Commands.Keys import Button, Hat, ...` | ✅ モジュールパッチで保持 |
| `self.keys.neutral()` | ✅ 利用可能 |
| `self.keys.ser.writeRow()` | ✅ 利用可能（末尾に改行自動追加） |
| `self.keys.ser.write()` | ✅ 利用可能（引数は `bytes` 型） |
| 画像処理API | ✅ Rust実装（opencv-rust） |
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
- **PythonとLuaで同じ設定が可能**: どちらの動的設定ファイルでも、同じ項目を同じ要素名（`pokecon.opt.xxx`）で設定できる
- **API構造の統一**: PythonとLuaで設定項目名は完全に同一。言語間で設定の互換性を維持

### 11.2 設定ファイル

> **重要**: `settings.ini` は**廃止**されました。従来のINIベースの設定は、Rustネイティブの設定管理に置き換えられます。正確な形式と保存場所はRustバックエンドチームが決定します（本UI仕様の範囲外）。

### 11.3 優先順位とマージ方式

設定は以下の5層で優先順位が決まる（**後勝ち**、未設定項目は上位から継承）：

1. **デフォルト値**（アプリケーション内蔵）
2. **グローバル設定**（`~/.config/pokecon/settings.toml`）
3. **プロファイル設定**（`~/.config/pokecon/profiles/<name>/settings.toml`）
4. **起動時引数**（CLIオプション）
5. **動的設定**（`~/.config/pokecon/init.py` / `init.lua`）

```
優先順位: ①デフォルト → ②グローバル → ③プロファイル → ④CLI引数 → ⑤動的設定
         （低）                                    （高）
```

**注記**: 動的設定（⑤）が最も優先されるのは、パワーユーザーが最終的な制御権を持つことを意図した設計です。動的設定ファイルが読み込まれると、それまでの設定（CLI引数を含む）を上書きします。

### 11.4 静的設定（settings.toml）

**原則**: 静的設定で設定できる項目は、動的設定ファイルの指定（`dynamic_config_language`）を除き、**すべて動的設定（`init.py`/`init.lua`）からも設定可能**です。

**注**: `dynamic_config_language` は静的設定（`settings.toml`）のみで設定可能。動的設定ファイル内で言語を切り替えることはできない（chicken-and-egg問題を回避）。

```toml
# ~/.config/pokecon/settings.toml
[global]
language = "ja"  # 対応言語: "ja"（日本語）, "en"（英語）。将来的に拡張可能
auto_reload_config = false  # 動的設定ファイルの自動リロード（デフォルト無効）

[websocket]
reconnect_interval_sec = 3  # 再接続間隔（秒）
reconnect_max_retries = 20  # リトライ回数上限

[python]
# Pythonバージョン（オプション、デフォルト推奨）
# デフォルトは最新安定版（3.14系）を使用
# version = "3.14"  # 非推奨: 基本的にはデフォルトを使用

# ユーザー追加ライブラリ
[[python.packages]]
name = "requests"
version = ">=2.28.0"

[[python.packages]]
name = "numpy"

[profiles]
active_profile = "default"  # TOMLキー: active_profile（Python API: pokecon.opt.active_profile と同名）

# UI表示用FPSの選択肢（カスタマイズ可能）
[ui]
ui_fps_options = [5, 15, 30, 60]  # ラベルは自動生成（例: "5 FPS"）
key_chattering_threshold_ms = 10  # チャタリング判定閾値（ms）
```

### 11.5 動的設定の読み込みタイミング

| タイミング | 動作 |
|-----------|------|
| **アプリケーション起動時** | 自動読み込み（`init.py` / `init.lua`） |
| **プロファイル切替時** | 自動読み込み（新プロファイルの設定を反映） |
| **手動** | メニュー「Load Dynamic Config」で読み込み |
| **自動リロード** | ファイル変更検知時（デフォルト無効、オプトイン）。検知方式はOSネイティブのファイル監視（inotify/kqueue/ReadDirectoryChangesW等）を使用 |

**メニュー項目**（§11.13.1参照）:
```
File
├── Load Dynamic Config      ← 新規読み込み（拡張子で自動判別）
├── Reload Dynamic Config    ← 現在のファイルを再読み込み
└── Open Config Directory    ← 設定ディレクトリを開く
```

### 11.6 動的設定の共存（Neovim準拠）

`init.py` と `init.lua` の両方が存在する場合、**Neovimと同様に一方のみ**読み込まれます。

| 設定 | 読み込まれるファイル |
|------|-------------------|
| `settings.toml` で `dynamic_config_language = "python"` を指定 | `init.py` |
| `settings.toml` で `dynamic_config_language = "lua"` を指定 | `init.lua` |
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

### 11.7 動的設定（Python）

```python
# ~/.config/pokecon/init.py
import pokecon

# 言語設定
pokecon.opt.language = "ja"

# 自動リロード設定
pokecon.opt.auto_reload_config = True

# プロファイル設定
pokecon.opt.active_profile = "default"

# カメラ設定（フラット構造）
# camera_fps: バックエンド処理FPS（上限なし。ソースの実FPSより高い場合はソースの上限で表示）
pokecon.opt.camera_fps = 60
# ui_fps: UI表示用FPS（getter/setterでUIのComboboxと連動）
pokecon.opt.ui_fps = 30
pokecon.opt.camera_resolution = "1280x720"

# シリアル設定（フラット構造）
pokecon.opt.serial_port = "COM3"
pokecon.opt.serial_baudrate = 115200
pokecon.opt.serial_data_format = "default"  # default | qingpi | 3ds

# 通知設定（フラット構造）
pokecon.opt.discord_webhook_url = "https://discord.com/api/webhooks/..."
pokecon.opt.discord_username = "PokeCon Bot"

# ウィジェットモード（int型: 1〜7。UI表示用の内部識別子は別途マッピング表を参照 §5.5）
pokecon.opt.widget_mode = 1  # 1〜7（§5.5参照）

# ソフトウェアコントローラー位置
pokecon.opt.controller_position = "top"  # top | bottom

# ダイアログボタン位置
pokecon.opt.dialog_button_position = "bottom"  # top | bottom | both

# UI FPS選択肢（カスタマイズ）
pokecon.opt.ui_fps_options = [5, 15, 30, 60]  # ラベルは自動生成

# チャタリング判定閾値
pokecon.opt.key_chattering_threshold_ms = 10

# キーマッピング（Neovim風記法）
# 注: pokecon.controller は動的設定専用API。ユーザースクリプトでは self.keys を使用
# 詳細は §11.14.2 参照
pokecon.keymap.set("<C-a>", lambda: print("Ctrl+A pressed"))
```

**動的設定の特徴**:
- **即時反映**: 設定変更は即座にUIに反映される
- **永続化なし**: 動的設定はファイルとして保存されているが、`pokecon.opt` の値自体は永続化されない。毎回 init ファイルから再評価される
- **優先順位**: 動的設定 > 静的設定（settings.toml）
- **エラーハンドリング**: 
  - **目標**: 両言語とも構文エラー時に該当行をスキップし、残りを続行
  - **Python**: 言語仕様によりファイル全体の読み込みが必要。構文エラー時はファイル全体の読み込みに失敗し、フォールバック設定を使用
  - **Lua**: 構文エラー時に該当行をスキップし、残りを続行

### 11.8 動的設定（Lua）

```lua
-- ~/.config/pokecon/init.lua
-- require不要で pokecon.* に直接アクセス

-- 設定（Pythonと同じ要素名・同じAPI構造）
pokecon.opt.language = "ja"
pokecon.opt.camera_fps = 60
pokecon.opt.ui_fps = 30

-- タグマッチ関数
pokecon.ui.tag_match_function = function(selected, tag)
    return selected:lower() == tag:lower()
end

-- タグソート関数
pokecon.ui.tag_sort_function = function(tags)
    table.sort(tags)
    return tags
end

-- キーマッピング（Neovim風記法）
pokecon.keymap.set("a", function()
    pokecon.controller.press(pokecon.controller.Button.A)
end)

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

### 11.9 エラーハンドリング

- 動的設定ファイル読み込み時にエラーが発生しても、アプリケーションは継続して動作
- エラー内容はログパネルに出力（行番号・ファイル名・エラー内容）
- フォールバック機構により、前回の有効な設定を維持

### 11.10 Luaランタイム

Luaランタイムの実装にはmluaクレート（LuaJIT + vendored features）を使用します。

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
| **Rust統合** | mlua crate（`luajit` + `vendored` feature） |
| **ライセンス** | MIT（商用利用可能） |
| **バインディング** | Rustコアに埋め込み、PyO3と同じプロセス空間で実行 |

### 11.11 設定ファイルの階層構造

```
~/.config/pokecon/                    # XDG_CONFIG_HOME（デフォルト）
├── settings.toml                     # 静的設定（グローバル）
├── profiles/                         # プロファイル管理
│   ├── default/
│   │   └── settings.toml
│   ├── custom1/
│   │   └── settings.toml
│   └── custom2/
│       └── settings.toml
├── init.py                           # Python動的設定
├── init.lua                          # Lua動的設定
├── pyproject.toml                    # Python LSP設定（自動生成）
├── .luarc.json                       # Lua LSP設定（自動生成）
└── .vscode/                          # VS Code設定（オプション）
    └── settings.json
```

**設定ディレクトリのカスタマイズ**:
- 環境変数: `POKECON_HOME=/path/to/config`
- コマンドライン引数: `--config-dir /path/to/config`

### 11.12 イベントシステム

動的設定ファイル（PythonおよびLua）で使用するイベント駆動のフックシステム。

#### 11.12.1 設計方針

- **Neovim/Vimライクな設計**: `autocmd` スタイルのイベントハンドラ登録
- **Pre/Postフェーズ**: 原則としてすべてのイベントは `Pre`（事前）と `Post`（事後）の2フェーズを持つ。特別な理由がない限り両方を持つ必要がある
- **片方のみの例外**: 
  - **Postのみ**: イベント発生前に処理を実行しても意味がない場合（例: `AppStartupPost` — アプリケーション起動前にはAPIが利用できない）
  - **Preのみ**: イベント発生後に処理を実行しても意味がない場合（例: `AppShutdownPre` — アプリケーション終了後に状態が失われる）
- **フェーズはイベント名に含める**: `phase` 引数ではなく、イベント名自体に `Pre`/`Post` を含める（LSP警告のため）
- **require不要**: Lua設定では `require` なしで `pokecon.*` にアクセス可能。グローバル名前空間に `pokecon` が注入される
- **Python/Lua両対応**: 両言語で同じAPI構造を使用

#### 11.12.2 名前空間設計

| 名前空間 | 用途 | API |
|---------|------|-----|
| `pokecon.autocmd` | イベントハンドラの登録・解除 | `on()`, `once()`, `off()`, `clear(group)` |
| `pokecon.event` | イベント定義・発火 | `define()`, `emit()`, `list_defined()`, `get_schema()` |
| `pokecon.keymap` | キーマップの登録・解除・発火 | `set()`, `del()`, `trigger()` |
| `pokecon.controller` | コントローラー（ゲームパッド）操作 | `press()`, `hold()`, `holdEnd()`, `Button`, `Direction`, `Hat` |
| `pokecon.ui` | UI関連の動的設定 | `tag_match_function`, `tag_sort_function` |

#### 11.12.3 イベントハンドラAPI

```python
# Python設定
import pokecon

# 基本的なイベント登録
# 戻り値: HandlerId（ハンドラ解除用）
# callback: 引数なし（デフォルト）。pokecon.state に直接アクセスして情報を取得
handler_id = pokecon.autocmd.on("CameraOpenPost", callback=lambda: print("Camera opened"))

# 一度だけ実行
# 発火後の HandlerId は無効になり、off() は何もしない（エラーにはならない）
handler_id_once = pokecon.autocmd.once("SerialConnectPost", callback=lambda: print("Serial connected"))

# イベントハンドラ解除
# 引数: HandlerId（on() / once() の戻り値）
pokecon.autocmd.off(handler_id)

# グループ単位で一括解除
# "all" = すべてのハンドラ解除
# "CameraOpenPost" = そのイベントの全ハンドラ解除
# "my_group" = ユーザ定義グループの全ハンドラ解除
# 注: イベント名とグループ名が同名の場合、イベント名が優先される（グループ名を指定したい場合は別名を使用）
pokecon.autocmd.clear("all")
pokecon.autocmd.clear("CameraOpenPost")
pokecon.autocmd.clear("my_group")
```

**グループ（Neovimの `augroup` に相当）**:

グループは関連するイベントハンドラをまとめるための仕組みです。Neovimと同様に、グループを指定することでハンドラの管理が容易になります。

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

-- グループを指定して登録
pokecon.autocmd.on("CameraOpenPost", {
    callback = function()
        print("Camera opened")
    end,
    group = "my_group"
})
```

#### 11.12.4 イベント定義・発火API

```python
# ユーザー定義イベント
pokecon.event.define("MyCustomEvent")

# イベント発火
pokecon.event.emit("MyCustomEvent", data={"key": "value"})

# 定義済みイベント一覧
# 戻り値: list[str]
print(pokecon.event.list_defined())

# イベントスキーマ取得
# 戻り値: dict[str, Any]（イベントのメタデータ）
schema = pokecon.event.get_schema("CameraOpenPost")
```

```lua
-- Lua設定（Pythonと同じAPI構造）
pokecon.event.define("MyCustomEvent")
pokecon.event.emit("MyCustomEvent", {key = "value"})
print(pokecon.event.list_defined())
```

#### 11.12.5 組み込みイベント一覧

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

**ScriptLoadPre/ScriptLoadPostのタイミング**:

```
1. 初期処理: script_dirs の解決・存在確認
2. ファイル探索: 各ディレクトリ内の .py ファイルを探索
3. クラス抽出: モジュールインポート・コマンドクラス抽出・自動タグ生成
4. ScriptLoadPre 発火: pokecon.state.command_candidates が設定済み
   → ユーザーがコールバック内で command_candidates を変更可能
5. メイン処理: command_candidates を元に手動タグ統合・動的タグ追加
6. ScriptLoadPost 発火: すべてのタグ統合完了後
```

**命名規則**:
- **キャメルケース**: `CameraOpenPost`, `SerialConnectPost`
- **Pre/Post後置**: Vim/Neovim風（`BufReadPre`/`BufReadPost`に類似）
- **名前空間なし**: ドット区切りの名前空間は使用しない
- **動詞に限定しない**: 名詞・形容詞も可

**注記**: 動的設定用イベントシステム（§11.12）とWebSocketイベント（§7.3）は**別々のシステム**です。
- **動的設定イベント**: `CameraOpenPost`（PascalCase + Pre/Post後置）— 動的設定で使用
- **WebSocketイベント**: `camera.frame`（lowercase + ドット区切り）— UIとバックエンド間の通信

両者は内部で連携しますが、命名規則と用途が異なります。

**連携方法の概要**:
- 動的設定イベントはRustコア内のイベントバスで発火・購読される
- WebSocketイベントはUIとバックエンド間の通信プロトコルとして使用される
- 例: `CameraOpenPost` イベントが発火されると、RustコアはWebSocketで `camera.opened` イベントをUIに送信し、UIはカメラ映像の表示を開始する
- この連携はRustコア内で自動的に行われ、ユーザーが意識する必要はない

#### 11.12.6 型ヒント

```python
from typing import Literal, Union

# 組み込みイベントの厳密な型定義
BuiltinEvent = Literal[
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
# Union[BuiltinEvent, str] はLSPによりstrに単純化されるが、意図（組み込み vs ユーザー定義の区別）はドキュメントとして残す
EventName = Union[BuiltinEvent, str]  # 実際にはstrと同等だが、型ヒントの意図を明示
```

#### 11.12.7 エラーハンドリングとPreイベントのキャンセル

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

**コールバック型ヒント**:

```python
from typing import Callable

# 通常イベント（Post等）: 戻り値なし
Callback = Callable[[], None]

# Preイベント: Falseでキャンセル、それ以外は継続
PreCallback = Callable[[], bool | None]
```

| エラー種類 | 挙動 | ログ出力 |
|-----------|------|---------|
| コールバック内の例外 | 当該ハンドラのみ停止、他は継続 | ERRORレベル |
| 存在しないイベントへのemit | 無視（ハンドラがないだけ） | WARNINGレベル |
| ハンドラ登録時の無効なイベント名 | 登録拒否、例外を送出 | ERRORレベル |
| 循環参照（イベント発火中に同じイベントを発火） | 検出して無視（同一イベントの直接再入のみ検出。間接循環 A→B→A は検出対象外） | ERRORレベル |

### 11.13 動的設定ファイルのUI

#### 11.13.1 メニュー配置

- **配置場所**: メニューバー内
- **項目**: 単一の「Load Dynamic Config」メニュー項目

```
File
├── Load Dynamic Config      ← 新規読み込み（拡張子で自動判別）
├── Reload Dynamic Config    ← 現在のファイルを再読み込み
└── Open Config Directory    ← 設定ディレクトリを開く
```

#### 11.13.2 ファイル選択と自動判別

- **ファイル選択ダイアログ**: 単一の「Load Dynamic Config」メニューから開く
- **自動判別**: 拡張子で言語を自動判別
  - `.py` → Python動的設定ファイル
  - `.lua` → Lua動的設定ファイル
- **手動指定**: 拡張子が不明な場合はユーザーに選択を促す

#### 11.13.3 リロード機能

| 機能 | 説明 |
|------|------|
| **手動リロード** | 「Reload Dynamic Config」メニューで現在のファイルを再読み込み |
| **自動リロード** | ファイルウォッチャーによる自動リロード（**デフォルトで無効**） |
| **有効化方法** | `pokecon.opt.auto_reload_config = True` またはUI設定 |

#### 11.13.4 エラーハンドリング

- 動的設定ファイル読み込み時にエラーが発生しても、アプリケーションは継続して動作
- エラー内容はログパネルに出力
- フォールバック機構により、前回の有効な設定を維持

### 11.14 キーマップシステム

#### 11.14.1 設計方針

- **Neovim風キー記法**: `<C-a>`, `<S-a>`, `<M-a>`, `<C-S-a>` 等
- **Neovim準拠API**: `vim.keymap.set` と同じシグネチャ（`mode` 省略版）
  - `pokecon.keymap.set(lhs, rhs, remap=False, desc=None)`
  - `pokecon.keymap.trigger(key)` — キー入力イベントを仮想的に発火（動的設定専用）
  - デフォルトは `noremap`（`remap=False`）
  - `remap=True` で再帰マップ有効
  - Pythonでは型ヒントのためフラットな構造（dictを挟まない）
- **rhsの型**: キー文字列（`KBKeys | str`）またはコールバック関数（`Callable`）
- **キー解放イベント**: 仮想キー `<Release-*>` を全キーに自動提供（同時押し含む: `<Release-C-a>`）。これにより「押した時」と「離した時」で別々の動作をマップ可能
  - **長押しの表現例**: `<C-a>` に「開始処理」、`<Release-C-a>` に「終了処理」をそれぞれマップすることで、ユーザー側で長押し相当の動作を実現
  - **極短時間押下（チャタリング対策）**: `<Release-hoge>` は対応する `<hoge>` のコールバック実行が完了するまで発火を待機。完了後に `<Release-hoge>` のコールバックを実行
  - **チャタリング判定閾値**: 設定可能（デフォルト10ms）。直前のReleaseから閾値ms以内のPress+Releaseはチャタリングとみなし、`<Release-hoge>` をキャンセルする。修飾キーを除く同一キー間で判定。例: `<C-a>` と `<S-a>`、`<A>` は判定対象（ベースキーが同じ「A」）。`<C-a>` と `<C-b>` は判定対象外（ベースキーが異なる）
- **ユーザー定義仮想キー**: lhsに新しい名前を入れた時に自動登録。存在チェックは発火時に行う
- **クリア方式**: キーマップは「1キー = 1rhs」の単純な上書きモデルであるため、専用のクリアAPIは提供しない。キーの無効化は `<nop>` を rhs に登録することで実現する（§11.14.3参照）。

#### 11.14.2 API仕様

```python
# Python設定
import pokecon
from typing import Literal, Callable

# KBKeys: 定義済みキーの型
KBKeys = Literal[
    # アルファベット（小文字）
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m",
    "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z",
    # ファンクションキー
    "<F1>", "<F2>", "<F3>", "<F4>", "<F5>", "<F6>", "<F7>", "<F8>", "<F9>", "<F10>", "<F11>", "<F12>",
    # 特殊キー
    "<Space>", "<Enter>", "<Return>", "<CR>",
    "<Backspace>", "<BS>",
    "<Escape>", "<Esc>",
    "<Tab>",
    "<Delete>", "<Del>",
    "<Insert>", "<Ins>",
    "<Home>", "<End>", "<PageUp>", "<PageDown>",
    "<Up>", "<Down>", "<Left>", "<Right>",
    # 修飾キー（単体）
    "<Ctrl>", "<Shift>", "<Alt>", "<Meta>", "<Super>",
    # マウスボタン
    "<LeftMouse>", "<RightMouse>", "<MiddleMouse>",
]

# lhs: KBKeys | str（strはユーザー定義仮想キー用）
# rhs: KBKeys | str | Callable[[], None]
# remap: bool = False
# desc: str | None = None

# 基本的なキーマッピング（noremap、関数rhs）
pokecon.keymap.set("a", lambda: pokecon.controller.press(pokecon.controller.Button.A))

# 修飾キー付き（noremap、関数rhs）
pokecon.keymap.set("<C-a>", lambda: print("Ctrl+A pressed"))
pokecon.keymap.set("<S-a>", lambda: print("Shift+A pressed"))
pokecon.keymap.set("<M-a>", lambda: print("Alt+A pressed"))
pokecon.keymap.set("<C-S-a>", lambda: print("Ctrl+Shift+A pressed"))

# remap有効（キー→キーのマッピング）
pokecon.keymap.set("<C-a>", "<Release-A>", remap=True)

# 特殊キー
pokecon.keymap.set("<F1>", lambda: print("F1 pressed"))
pokecon.keymap.set("<Space>", lambda: print("Space pressed"))
pokecon.keymap.set("<Enter>", lambda: print("Enter pressed"))
pokecon.keymap.set("<Esc>", lambda: print("Escape pressed"))

# 長押し（Releaseキー）
pokecon.keymap.set("<Release-A>", lambda: print("A released"))
pokecon.keymap.set("<Release-C-a>", lambda: print("Ctrl+A released"))

# ユーザー定義仮想キー（自動登録）
pokecon.keymap.set("<MyCustomKey>", lambda: print("Custom key triggered"))
# 他のキーからユーザー定義キーを呼び出し（発火時に存在チェック）
pokecon.keymap.set("<C-m>", "<MyCustomKey>", remap=True)

# 説明文付き
pokecon.keymap.set("<F5>", lambda: None, desc="F5の動作を無効化")

# キーマップ削除
pokecon.keymap.del("<F5>")  # F5のキーマップを削除
```

```lua
-- Lua設定
-- Pythonと同じフラットな構造

-- 基本的なキーマッピング（noremap、関数rhs）
pokecon.keymap.set("a", function()
    pokecon.controller.press(pokecon.controller.Button.A)
end)

-- 修飾キー付き（noremap、関数rhs）
pokecon.keymap.set("<C-a>", function()
    print("Ctrl+A pressed")
end)

-- remap有効（キー→キーのマッピング）
pokecon.keymap.set("<C-a>", "<Release-A>", true)

-- 長押し（Releaseキー）
pokecon.keymap.set("<Release-A>", function()
    print("A released")
end)

-- ユーザー定義仮想キー（自動登録）
pokecon.keymap.set("<MyCustomKey>", function()
    print("Custom key triggered")
end)
-- 他のキーからユーザー定義キーを呼び出し（発火時に存在チェック）
pokecon.keymap.set("<C-m>", "<MyCustomKey>", true)

-- 説明文付き
pokecon.keymap.set("<F5>", function() end, false, "F5の動作を無効化")

-- キーマップ削除
pokecon.keymap.del("<F5>")  -- F5のキーマップを削除
```

#### 11.14.3 サポートするキー記法

**原則**: Neovimと同じく、**特殊キーのみ `<>` で囲み、通常の印字可能文字（アルファベット、数字、記号類）はそのまま**。

**印字可能文字の入力ルール**:
1. ほとんどの文字はそのまま入力（例: `a`, `A`, `1`, `9`, `>`, `;`, `:`, `,`, `@`, `!`, `#`, `$`, `%`, `&`, `*`, `(`, `)`, `-`, `_`, `=`, `+`, `[`, `]`, `{`, `}`, `.`, `/`, `?`）
2. **例外1**: バックスラッシュ `\` は `<Bslash>` または `\\` で入力
3. **例外2**: 小なり記号 `<` は `<lt>` または `\<` で入力
4. **例外3**: テンキー（`<k0>`〜`<k9>`, `<kPlus>`, `<kMinus>`, `<kEnter>` 等）は `<>` で囲む（メインキーボードの数字とは別のキー）

**同じキーの異なる表現**（Neovim準拠）:

| キー | 別名 |
|------|------|
| Enter | `<CR>`, `<Enter>`, `<Return>` |
| Backspace | `<BS>`, `<Backspace>` |
| Escape | `<Esc>`, `<Escape>` |
| Tab | `<Tab>` |
| Space | `<Space>` |
| Delete | `<Del>`, `<Delete>` |
| Meta/Alt | `<M-a>`, `<A-a>`（同じ） |
| Command/Super | `<D-a>`（Mac） |
| Nul | `<Nul>`, `<Null>` |
| Insert | `<Insert>`, `<Ins>` |
| Home | `<Home>` |
| End | `<End>` |
| PageUp | `<PageUp>` |
| PageDown | `<PageDown>` |
| Less-than | `<lt>` |
| Backslash | `<Bslash>` |
| Vertical bar | `<Bar>` |
| No-op | `<nop>` |
| Linefeed | `<NL>` |
| Ignore | `<Ignore>` |

**注意**: `a` と `A` は**同じキー**として扱われる。キーコードが異なる場合は別のキー（例: メインキーボードの `1` とテンキーの `<k1>` は別のキー）

| 記法 | 説明 | 例 |
|------|------|-----|
| `a`〜`z`, `A`〜`Z` | アルファベット（`<>` 不要、`a` と `A` は同じキー） | `a`, `A`, `z` |
| `0`〜`9` | 数字（メインキーボード、`<>` 不要） | `1`, `2`, `9` |
| `<C-x>` | Ctrl + x | `<C-a>`, `<C-c>` |
| `<S-x>` | Shift + x | `<S-a>`, `<S-1>` |
| `<M-x>` | Alt + x | `<M-a>`, `<M-F4>` |
| `<C-S-x>` | Ctrl + Shift + x | `<C-S-a>` |
| `<F1>`〜`<F12>` | ファンクションキー | `<F1>`, `<F12>` |
| `<Space>` | スペースキー | `<Space>` |
| `<Enter>` / `<CR>` / `<Return>` | エンターキー | `<Enter>` |
| `<Esc>` / `<Escape>` | エスケープキー | `<Esc>` |
| `<Tab>` | タブキー | `<Tab>` |
| `<BS>` / `<Backspace>` | バックスペース | `<BS>` |
| `<Del>` / `<Delete>` | 削除キー | `<Del>` |
| `<Insert>` / `<Ins>` | 挿入キー | `<Insert>` |
| `<Home>` | ホームキー | `<Home>` |
| `<End>` | エンドキー | `<End>` |
| `<PageUp>` | ページアップ | `<PageUp>` |
| `<PageDown>` | ページダウン | `<PageDown>` |
| `<Up>`/`<Down>`/`<Left>`/`<Right>` | 方向キー | `<Up>`, `<Down>` |
| `<k0>`〜`<k9>` | テンキー | `<k1>`, `<kEnter>` |
| `<kPlus>` | テンキープラス | `<kPlus>` |
| `<kMinus>` | テンキーマイナス | `<kMinus>` |
| `<kEnter>` | テンキーエンター | `<kEnter>` |
| `<lt>` | Less-than `<` | `<lt>` |
| `<Bslash>` | Backslash `\` | `<Bslash>` |
| `<Bar>` | Vertical bar `|` | `<Bar>` |
|| `<nop>` | No-op（何もしない） | `<nop>` |
| `<NL>` | Linefeed | `<NL>` |
| `<Ignore>` | 待機キャンセル | `<Ignore>` |
| `<Release-x>` | キー解放（全キーに自動提供） | `<Release-A>`, `<Release-C-a>` |
| `<CustomKey>` | ユーザー定義仮想キー（自動登録） | `<MyCustomKey>` |

**注意**:
- `<Release-*>` は全ての既存キーに対して自動的に存在する仮想キーです。同時押し（`<C-a>` 等）に対しても `<Release-C-a>` が使用可能です
- 通常の印字可能文字（`a`〜`z`, `A`〜`Z`, `0`〜`9`, `>`, `;`, `:`, `,`, `@` 等）に `<>` を付けると、それはユーザー定義仮想キーとして扱われます（例: `<A>` は仮想キー、`A` は通常キー）

#### 11.14.4 キー重複時の優先順位

- 後から登録されたキーバインドが優先される（後勝ち）
- 同じキーに複数のコールバックが登録されている場合、最後に登録されたものが実行される
- プロファイル切替時は、新プロファイルのキーバインドに置き換えられる

#### 11.14.5 デフォルトキーバインド

**実行制御キー（デフォルト未割り当て、ユーザー設定可能）**:

| キー | 動作 | 状態 | デフォルト |
|------|------|------|-----------|
| `<F5>` | コマンド開始 | press | 未割り当て |
| `<F6>` | コマンド停止 | press | 未割り当て |
| `<F7>` | コマンド一時停止 | press | 未割り当て |
| `<F8>` | コマンド再開 | press | 未割り当て |
| `<F9>` | コマンドリロード | press | 未割り当て |
| `<Esc>` | 停止 | press | 未割り当て |

**注**: デフォルトでは未割り当て。ユーザーが `pokecon.keymap.set()` で割り当てることで有効化される。ショートカットボタン（§6.4.3）はF1〜F10をデフォルトで使用するため、実行制御キーとの競合を避けるため、デフォルトでは両方とも未割り当てとする。

#### 11.14.6 キー入力の仮想発火

動的設定専用。スクリプトからキー入力イベントを仮想的に発火する。

```python
# Python
pokecon.keymap.trigger("a")        # a を押したことにする
pokecon.keymap.trigger("<C-a>")   # Ctrl+A を押したことにする
pokecon.keymap.trigger("<F1>")    # F1 を押したことにする
```

```lua
-- Lua
pokecon.keymap.trigger("a")        -- a を押したことにする
pokecon.keymap.trigger("<C-a>")   -- Ctrl+A を押したことにする
pokecon.keymap.trigger("<F1>")    -- F1 を押したことにする
```

**用途**:
- マクロ記録/再生
- スクリプトからのキーイベント発火
- テスト

**注意**:
- ユーザー定義仮想キー（`<MyCustomKey>`）も発火可能
- `<Release-*>` も発火可能（キーを離したことにする）
- このAPIは動的設定（`init.py`/`init.lua`）専用。ユーザー向けPython APIには公開しない

### 11.15 相互参照API

#### 11.15.1 設計方針

- **Neovimの`:source`に類似**: `pokecon.source(path)`
- **拡張子で自動判別**: `.py` → Python, `.lua` → Lua
- **相対パス・絶対パス両対応**

#### 11.15.2 API仕様

```python
# Python設定
import pokecon

# 絶対パス
pokecon.source("/home/user/.config/pokecon/extra_settings.py")

# 相対パス（設定ディレクトリ基準）
pokecon.source("./extra_settings.py")

# チルダ展開
pokecon.source("~/.config/pokecon/extra_settings.py")
```

```lua
-- Lua設定（Pythonと同じAPI構造）
pokecon.source("~/.config/pokecon/extra_settings.lua")
```

#### 11.15.3 エラーハンドリング

- 指定されたファイルが存在しない場合はエラーをログに出力
- ファイルの読み込みに失敗しても、現在の設定は維持される
- 循環参照（AがBを読み込み、BがAを読み込む）を検出し、エラーを出力

### 11.16 状態取得API

#### 11.16.1 設計方針

- **読み取り専用**: `pokecon.state.<property>`
- **リアルタイム**: 現在の状態を即座に反映
- **スレッドセーフ**: 複数スレッドから安全に読み取り可能。Rust側で`Arc<RwLock<T>>`で保護。書き込みは特定イベント（`ScriptLoadPre`等）のコールバック内または内部処理でのみ行われる

#### 11.16.2 利用可能な状態プロパティ

```python
# Python設定
import pokecon

# シリアル関連
print(pokecon.state.serial_port)        # 現在のシリアルポート（例: "COM3"）
print(pokecon.state.serial_baudrate)    # 現在のボーレート（例: 115200）
print(pokecon.state.serial_connected)   # 接続状態（True/False）

# カメラ関連
print(pokecon.state.camera_opened)      # カメラオープン状態（True/False）
print(pokecon.state.camera_fps)         # 現在のFPS
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
print(pokecon.state.last_input)         # 最後の入力
print(pokecon.state.holding_buttons)    # 現在保持中のボタン一覧

# アプリケーション関連
print(pokecon.state.pid)                # アプリケーションのプロセスID
```

**`pokecon.opt` と `pokecon.state` の違い**:

| 名前空間 | 性質 | 説明 |
|---------|------|------|
| `opt` | 設定値（書き込み可能） | ユーザーが設定した**設定値**。UIコントロールや動的設定で変更される |
| `state` | 現在値（読み取り専用） | デバイスやシステムが実際に使用している**現在値**。デバイスの能力制限により、`opt` と異なる値を指すことがある |

**例**: `opt.camera_fps = 120` と設定しても、キャプチャデバイスが60fpsまでしか対応しない場合、`state.camera_fps` は `60` となる。

```lua
-- Lua設定
print(pokecon.state.serial_port)
print(pokecon.state.camera_opened)
print(pokecon.state.active_profile)
```

### 11.17 プロファイルAPI

#### 11.17.1 設計方針

- **フラットAPI**: `pokecon.profile.current()`, `pokecon.profile.list()`, `pokecon.profile.switch(name)`
- **動的設定ファイル内で使用可能**
- **`pokecon.opt.active_profile` との関係**: `pokecon.profile.switch("custom")` は内部的に `pokecon.opt.active_profile = "custom"` を設定し、プロファイル切替イベントを発火する。両者は等価だが、`profile.switch()` はイベント発火とエラーハンドリング（存在しないプロファイル名の検証）を行う

#### 11.17.2 API仕様

```python
# Python設定
import pokecon

# 現在のプロファイル取得
# 戻り値: str（プロファイル名）
current = pokecon.profile.current()
print(f"Current profile: {current}")

# 利用可能なプロファイル一覧
# 戻り値: list[str]
profiles = pokecon.profile.list()
print(f"Available profiles: {profiles}")

# プロファイル切替
# 戻り値: bool（成功: True, 失敗: False）
# エラー時: 存在しないプロファイル名を指定した場合はFalseを返し、エラーをログに出力
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

#### 11.17.3 プロファイル切替時の動作

- 新しいプロファイルの設定を読み込み（`~/.config/pokecon/profiles/<name>/settings.toml`）
- キーマップをクリアしてから、デフォルトキーバインドを再登録
  - **クリア方法**: 内部テーブルをクリアし、全てのキーマップを削除する。実装方式は問わない（ハッシュテーブルのクリア、全キーに `<nop>` を登録する等）
- 静的設定のキーバインドを再登録
- 動的設定ファイル（`~/.config/pokecon/init.py`/`init.lua`）を読み直し、動的キーバインドを再登録
  - **注**: 動的設定ファイルはグローバル（プロファイル非依存）。プロファイル固有の動的設定が必要な場合は、`settings.toml` で `dynamic_config_language` を切り替えるか、`pokecon.source()` で別ファイルを読み込む
- イベントハンドラをクリアして再登録

**注**: キーマップのクリアは内部的な処理。ユーザーが直接キーマップを無効化する場合は、`pokecon.keymap.set("<F5>", "<nop>")` のように `<nop>` を rhs に登録することで実現する。

## 12. 環境変数

| 変数 | 説明 | デフォルト |
|----------|-------------|---------|
| `POKECON_DISABLE_COMPOSITING` | コンポジットモードを無効化（Tauri） | `0` |
| `POKECON_WEB_DIR` | 静的ファイルディレクトリ | `web/dist` |
| `POKECON_PORT` | HTTPサーバーポート | `8020` |

## 13. クライアント側ストレージ

| 項目 | 保存方法 | 備考 |
|------|---------------|-------|
| ショートカットボタン割り当て | `localStorage` | 10ボタンキーバインド。ブラウザ単位の設定 |
| キーボード設定 | `localStorage` | キーマッピング設定。ブラウザ単位の設定 |



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
```
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

**注記**: 
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

nix環境では、Pythonインタープリターのパスを**ビルド時にnixストアパスとして埋め込む**。

**実装概要**:
- `rust/pokecon-core/build.rs` で `POKECON_PYTHON_PATH` 環境変数を読み込み、ソースコードに埋め込む
- `flake.nix` で `POKECON_PYTHON_PATH = "${pythonEnv}/bin/python"` を設定
- nixストアパスは不変なため、再現性が保証される
- グローバルPythonを使用しない（nixの隔離性を維持）
- 非nix環境では環境変数が未設定のため、実行時に別途Pythonを取得するフォールバック動作

**詳細な実装**: `rust/pokecon-core/build.rs` および `flake.nix` を参照。

### 14.4 Python管理（非nix環境）

非nix環境では、`PythonManager` がPythonインタープリターのセットアップを管理する。

**実装概要**:
- `~/.local/share/pokecon/` 配下にPythonをセットアップ
- 期待するバージョンがない場合はデフォルトを使用
- 既存のPythonが期待するバージョンかチェック
- ない場合は astral-sh/python-build-standalone をダウンロード
- 仮想環境を構築し、必須パッケージ + ユーザーパッケージをインストール

**詳細な実装**: `rust/pokecon-core/src/python.rs` または同等のモジュールを参照。

### 14.5 必須パッケージ管理

- **リポジトリ内`pyproject.toml`**からビルド時に取得
- **`build.rs`で`OUT_DIR`にコード生成**、`include!`で埋め込み
- `cargo:rerun-if-changed=../pyproject.toml`で再ビルドトリガー

```rust
// build.rs
fn main() {
    println!("cargo:rerun-if-changed=../pyproject.toml");
    // pyproject.tomlを読み込み、依存関係をパース
    // 生成コードをOUT_DIRに書き出し
}
```

### 14.6 ユーザーパッケージ設定

```toml
# ~/.config/pokecon/settings.toml
# ユーザーが触る設定ファイル（必須パッケージは含まない）

[python]
# Pythonバージョン（オプション、デフォルト推奨）
# デフォルトは最新安定版（3.14系）を使用
# version = "3.14"  # 非推奨: 基本的にはデフォルトを使用

# ユーザー追加ライブラリ
[[python.packages]]
name = "requests"
version = ">=2.28.0"  # バージョン指定あり

[[python.packages]]
name = "numpy"        # バージョン指定なし（最新版）

[[python.packages]]
name = "custom-lib"
version = "1.0.0"
source = "git+https://github.com/user/custom-lib.git"  # 取得元指定

[[python.packages]]
name = "local-lib"
version = "0.5.0"
source = "path=/home/user/projects/local-lib"  # ローカルパス
```

### 14.7 LSP設定（pyproject.toml）

```toml
[tool.basedpyright]
extraPaths = ["/home/username/.local/share/pokecon/typings"]
venvPath = "/home/username/.local/share/pokecon"
venv = "venv"

[tool.pyright]
extraPaths = ["/home/username/.local/share/pokecon/typings"]
venvPath = "/home/username/.local/share/pokecon"
venv = "venv"

[tool.mypy]
mypy_path = ["/home/username/.local/share/pokecon/typings"]

[tool.pylsp.plugins.jedi]
extra_paths = ["/home/username/.local/share/pokecon/typings"]

[tool.pyrefly]
search_path = ["/home/username/.local/share/pokecon/typings"]

[tool.ty.environment]
extra-paths = ["/home/username/.local/share/pokecon/typings"]
python = "/home/username/.local/share/pokecon/venv/bin/python"

[tool.ruff]
# ruffはextraPaths未対応（LSP機能限定）
```

### 14.8 Lua LSP設定（.luarc.json）

```json
{
    "$schema": "https://raw.githubusercontent.com/LuaLS/vscode-lua/master/setting/schema.json",
    "workspace.library": [
        "/home/username/.local/share/pokecon/lua-typings"
    ]
}
```

## 15. 今後のバージョンで実装予定

### 15.1 PWA要件

> **ステータス: 未実装 — 今後のバージョンで実装予定。**  
> 以下の要件は将来の実装のためのユーザー要求として記録されています。現在のスコープには含まれません。

#### 15.1.1 マニフェスト

- アプリメタデータを含む `manifest.json`。
- 全プラットフォーム用のアイコン。
- 表示モード: `standalone`。

#### 15.1.2 サービスワーカー

- UIアセットのオフライン対応。
- キューに入ったコマンドのバックグラウンド同期（将来）。

#### 15.1.3 インストールプロンプト

- カスタムインストールボタン。
- プラットフォーム固有のインストールガイダンス。

### 15.2 その他将来的機能

- **ハードウェア制御**
  - ProController/Xinput対応（§6.3.2参照）
  - ブラウザのAPI制約により実装が複雑
  - UI上はハードウェア制御セクションを非表示（グレーアウトではなく非表示）
- **キー設定（設定ファイルベース）**
  - GUIからの編集は不要
  - 静的設定ファイル（`settings.toml`）で表現できる範囲で設定可能
  - 例: `keyboard.shortcuts.F5 = "command_start"`
- Pokémon Home連携（将来的に追加する — APIはmainブランチ準拠で実装）。

---

# 付録

## A. Tkinter UIリファレンス

### A.1 元のタブ詳細

元のPython/Tkinter UIは `tkinter.ttk.Notebook` を使用し、以下の構造でした:

- **CameraTab**: スレッド化されたフレームリーダー、PILリサイズ、`ImageTk.PhotoImage` キャンバス表示による `cv2.VideoCapture`。キャンバスはマウス駆動のスティック制御、カラーピッカー、領域スクリーンショットをサポート。
- **SerialTab**: COMポートドロップダウン、ボーレートセレクター（9600/115200）、データ形式セレクター（デフォルト/Qingpi/3DS Controller）、ステータスインジケーター付き接続ボタン、Text+Scrollbar付きシリアルモニター。
- **ManualControlTab**: ソフトウェア制御（キーボードチェックボックス、LStick Mouse、RStick Mouse）、ハードウェア制御（ProController/Xinputラジオ、録画チェックボックス）、完全なJoy-ConレイアウトのSwitch Controller Simulator。
- **CommandTab**: 3サブタブ（Python Command、Mcu Command、Shortcut）、ファイルブラウザー、タグフィルタードロップダウン、Listbox/Treeview付きコマンドリスト、10ショートカットボタン、実行ボタン（開始/一時停止/再開/停止/再読み込み）。
- **NotificationTab**: Discord Webhook URL、ユーザー名、アバターURL入力（テストボタン付き）。Windows通知開始/終了チェックボックス（テストボタン付き）。LINE UI（削除 — サービスEOL）。
- **OthersTab**: 出力サイズ調整スライダー、stdout出力先ラジオ（出力#1/出力#2）、出力をクリアボタン、ウィジェットモードコンボボックス（7モード）、ソフトウェアコントローラー位置ラジオ（TOP/BOTTOM）、ダイアログボタン位置ラジオ（TOP/BOTTOM/BOTH）。

### A.2 元のコントローラーレイアウト

- **ソフトウェアコントローラー**: CanvasベースのJoy-Con描画。`<Button-1>` イベントバインディングでホールド、`<ButtonRelease-1>` で解放、Shift+解放で `holdEndSkip`。
- **色**: L側シアン `#56CCF2`、R側赤 `#E9514E`、アクティブ状態黄色 `#FFD800`。
- **アナログスティックデッドゾーン**: 中央から±10%（0～255スケールで値103～153はニュートラルとして扱われる）。
- **ボタン**: A、B、X、Y、L、R、ZL、ZR、+、−、Home、Capture、D-pad（4方向）、Lスティック、Rスティック、タッチスクリーン（320×240）。

### A.3 元の出力パネル

- **出力 #1 と 出力 #2**: スライダー（0～100）で比率調整可能なログ表示。
- **ソース**: WebSocket経由で受信したログ。
- **クリア**: その他タブの「出力をクリア」ボタン。

## B. メタクラス設計（CommandMeta）

```python
class CommandMeta(type):
    """実装切り替え用メタクラス。
    
    現状はすべての実装がPyO3（Rustバインディング）に流れる。
    将来: クラス変数や関数使用パターンに基づいて切り替え。
    
    抽象クラスとしての機能:
    - `do()` メソッドを持つクラス（PythonCommand, ImageProcPythonCommand）を
      抽象クラスとして扱う。`do()` を実装しないサブクラスのインスタンス化を防止。
    - インスタンス化時に `do()` メソッドの存在を確認し、未実装の場合は
      `TypeError` を送出。
    """
    def __call__(cls, *args, **kwargs):
        # 抽象クラスチェック: do() メソッドが定義されているか
        if hasattr(cls, '__abstractmethods__') and cls.__abstractmethods__:
            raise TypeError(f"Can't instantiate abstract class {cls.__name__} with abstract method(s) {', '.join(cls.__abstractmethods__)}")
        # 将来: cls.__target_implementation__等をチェック
        # 現状: 常にPyO3実装を使用
        return super().__call__(*args, **kwargs)
```

---

*本仕様書は生きたドキュメントです。新しい要件がユーザーから伝達された場合、更新を行う必要があります。*
