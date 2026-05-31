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

- **Tkinterとの機能・視覚的パリティ**: 新しいUIは、元のTkinterの機能・レイアウト・外観に厳密に一致する必要があります。レイアウト、色、ボタンの間隔、ウィジェットの種類はオリジナルに準拠する必要があります。
- **スクリプト互換性**: リファクタリング前のバージョンで動作していたすべてのスクリプトは、引き続き正常に動作する必要があります。スクリプトAPIに破壊的変更は加えません。
- **モダンスタック**: SvelteKit + Svelte 5（runesモード）+ **Tailwind CSS v4**（確定、変更不可）。
- **低遅延通信**: プライマリとしてWebRTC、フォールバックとしてビデオ: WebCodecs + WebSocket、DataChannel: WebSocketを使用。WebSocketは切断時に3秒ごとに自動再接続。
- **型安全性**: Rustバックエンドから `utoipa` v5 + `openapi-typescript` を介してOpenAPI生成のTypeScript型を使用。
- **認証なし**: アプリケーションはローカル/LAN専用に設計。API認証は不要。

**実装レイヤー**:

| レイヤー | 言語 | 役割 | 例 |
|---------|------|------|-----|
| Rustコア | Rust | メインプロセス、すべてのコア処理 | イベントバス、シリアル通信、画像処理 |
| PyO3バインディング | Rust（Pythonに公開） | Python API提供 | `pokecon.events`, `pokecon.dialogue` |
| Python互換レイヤー | Python（最小限） | 将来の実装切り替え用フック | `CommandMeta`（`_meta.py`のみ） |

**注**: ユーザースクリプトや動的設定（`init.py`/`init.lua`）から呼び出されるAPIは、原則としてPyO3（Rust製）で実装される。Pythonファイル（`commands.py`, `events.py`等）は型ヒント・ドキュメント・互換レイヤーのみを提供し、実際の処理はRust側で行う。

### 1.3 対象プラットフォーム

| プラットフォーム | UIモード | 備考 |
|----------|---------|-------|
| デスクトップ（Windows/Linux） | Tauri（WebViewラッパー） | Webモードとaxum HTTPサーバーを共有 |
| Webブラウザ | スタンドアロンSvelteKit SPA | axum HTTPサーバーによって提供 |
| モバイル（将来） | レスポンシブSPA | 同一コードベース、アダプティブレイアウト |

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
|
---

## 3. 非機能要件

### 3.1 パフォーマンス

| 指標 | 目標 |
|--------|--------|
| ビデオ遅延（WebRTC） | < 100ms |
| ビデオ遅延（WebCodecs + WebSocketフォールバック） | 50-200ms |
| コントローラー入力遅延 | < 50ms |
| UI応答性 | 60fpsアニメーション、< 16ms入力応答 |

### 3.2 アクセシビリティ

- すべてのコントロールのキーボードナビゲーション。
- スクリーンリーダー用のARIAラベル。
- ハイコントラストモードのサポート。

### 3.3 ブラウザサポート

| ブラウザ | 最小バージョン |
|---------|----------------|
| Chrome/Edge | 90以上 |
| Firefox | 88以上 |
| Safari | 14以上 |

### 3.4 WebSocket自動再接続

- 接続断時に、3秒ごとに自動的に再接続を試行します。
- 一時的なサーバー利用不能を適切に処理する必要があります。

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

> **要件**: PWAは今後のバージョンで実装予定です。現在のスコープには、マニフェスト生成、サービスワーカー、またはインストールプロンプトは含まれません。

### 4.6 スクリプト互換性 — リファクタリング前の全スクリプトが動作必須

> **要件**: リファクタリング前のバージョンで動作していたすべてのスクリプトは、引き続き正常に動作する必要があります。スクリプトAPIに破壊的変更は加えません。

### 4.7 テーマサポート — 今後のバージョンで実装予定

> **要件**: テーマサポート（ライト/ダーク/カスタム）は今後のバージョンで実装予定です。Tailwind CSS v4はスタイリングフレームワークとして確定しています。

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

> **タブ数に関する注記**: 本仕様では、トップレベルに**6つのメインタブ**を記述します。Commandsタブには**3つのサブタブ**（Python Command、Mcu Command、Shortcut）が含まれ、合計9つの個別のタブ付きインターフェースとなります。PLAN.mdでは「8タブ構造」に言及していますが、これは旧設計ドキュメントであり、本仕様書の「6メインタブ + 3サブタブ」が最新の正しい定義です。本仕様では、トップレベルのノートブックタブを指す「6メインタブ」で一貫しています。

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
  - **Shift+解放**: `holdEndSkip` をトリガー — バックエンドに解放シグナルを送信せずに視覚的状態のみを切り替え。
- **ボタン**: すべての標準Switchコントローラーボタン:
  - A、B、X、Y
  - L、R、ZL、ZR
  - MINUS（−）、PLUS（+）
  - HOME、CAPTURE
  - D-pad（上、下、左、右）
  - Lスティック（アナログ、0～255座標）
  - Rスティック（アナログ、0～255座標）
  - タッチスクリーンシミュレーション（320×240座標入力）
- **アナログスティック**: X軸およびY軸ともに0～255の範囲。中央位置には±10%のデッドゾーンあり（値103～153はニュートラルとして扱われる）。

#### 5.3.2 出力パネル

- **出力 #1**: プライマリログ/出力表示。
- **出力 #2**: セカンダリログ/出力表示。
- **サイズ調整**: その他タブの「出力サイズ調整」スライダー（0～100）で制御。出力#1と出力#2の比率を決定。
- **ログソース**: バックエンドからWebRTC DataChannelまたはWebSocket経由で受信したログ。
- **機能**: 自動スクロール、クリアボタン、クリップボードにコピー、ログレベルフィルタリング。
- **ログレベル**: DEBUG、INFO、WARNING、ERROR、CRITICAL（フィルタリング用）。どのログがどのレベルかは実装時に決定。
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

| モード | ソフトウェアコントローラー | 出力 #1 | 出力 #2 | 説明 |
|------|---------------------|-----------|-----------|-------------|
| 1 | 表示 | 表示 | 表示 | フルパネル（デフォルト） |
| 2 | 表示 | 表示 | 非表示 | 単一出力 |
| 3 | 表示 | 非表示 | 表示 | 単一出力（入れ替え） |
| 4 | 非表示 | 表示 | 表示 | 出力のみ |
| 5 | 表示 | 非表示 | 非表示 | コントローラーのみ |
| 6 | 非表示 | 表示 | 非表示 | 出力 #1のみ |
| 7 | 非表示 | 非表示 | 表示 | 出力 #2のみ |

---

## 6. タブ仕様

### 6.1 カメラタブ

#### 6.1.1 映像表示

- **表示方法**: 映像レンダリング用のCanvas要素（CaptureArea）。
- **プライマリストリーム**: WebRTCビデオトラック（低遅延）。
- **フォールバック**: WebRTCが利用できない場合、WebCodecs + WebSocket（ブラウザネイティブHWデコード）。
- **フレームレート**: FPS設定（SpinboxまたはCombobox）で設定可能。

#### 6.1.2 カメラ設定

| コントロール | 種類 | 説明 |
|---------|------|-------------|
| **カメラデバイス選択** | Combobox | 利用可能なカメラデバイスのドロップダウン |
| **FPS** | Combobox | 設定可能なフレームレート（1～30fps） |
| **フリップ** | Checkbox | 水平/垂直フリップ切替 |

#### 6.1.3 表示モード切替（チェックボックス）

| モード | 説明 |
|------|-------------|
| **リアルタイム** | ライブ映像表示 |
| **値** | 数値ピクセル値またはオーバーレイデータの表示 |
| **ガイド** | ガイドオーバーレイまたは参照線の表示 |

これらは表示オーバーレイ切替用のチェックボックスです。

#### 6.1.4 キャンバス上のマウス操作

カメラキャンバス（CaptureArea）は以下のマウス操作をサポートします:

| アクション | トリガー | 動作 |
|--------|---------|----------|
| **Lスティック/Rスティック制御** | キャンバス上でマウスドラッグ | ドラッグ方向/距離に基づいて左/右アナログスティック移動をエミュレート |
| **カラーピッカー** | Ctrl+クリック | クリック位置の色の値を取得 |
| **範囲スクリーンショット** | Ctrl+Shift+ドラッグ | キャンバス上の選択した矩形領域のスクリーンショットをキャプチャ |
| **名前付き保存** | Ctrl+Alt+ドラッグ | 選択した領域を名前付きファイルプロンプトに保存 |

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

- **デフォルト形式**: ボーレート9600。
- **Qingpi形式**: ボーレート9600。
- **3DS Controller形式**: ボーレート115200。

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

**注**: ハードウェア制御はブラウザのAPI制約により実装が複雑なため、今後のバージョンで実装予定です。今回のリファクタリングではソフトウェア制御（キーボード、マウス）のみを実装します。

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

- **表示**: 利用可能なコマンドを表示するListboxまたはTreeview。
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

**タグの統合順序**:
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
- **バックエンド側の責務**: タグフィルターのマッチング（完全一致/部分一致/前方一致/後方一致/カスタム関数）
- **フロントエンド側の責務**: ファジーファインダーによる絞り込み（`fuse.js` を使用した部分一致スコアリング）。これはUI上の利便性向上のための補助機能であり、バックエンドのマッチング方式とは独立して動作する
- UI上では `@` なしタグが先、`@` 付きタグが後に表示
- ソート関数は動的設定ファイルで `pokecon.ui.tag_sort_function = my_sort_func` のように指定可能
  - 型: `Callable[[list[str]], list[str]]`
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
- **キーボードショートカット**: F1～F10またはその他の割り当て可能なホットキー。
- **保存**: 設定は `localStorage` に保存。

#### 6.4.4 実行制御ボタン

| ボタン | アクション |
|--------|------------|
| **開始** | コマンド実行を開始 |
| **停止** | 実行を停止 |
| **一時停止** | 実行を一時停止（再開可能） |
| **再開** | 一時停止から再開 |
| **リロード（再読み込み＋開始）** | コマンドを再読み込みして開始 |
| **緊急停止** | 実行を即座に中断（緊急時用） |
| **コマンドリスト再読み込み** | ファイルシステムからコマンドリストを再読み込み |

キーボードショートカットの割り当ては **§11.13.5 デフォルトキーバインド** を参照。

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
| **Webhook URL** | Text input | 検証付きのDiscord Webhook URL |
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
| **出力サイズ調整** | 出力#1と出力#2の幅比率を制御するスライダー（0〜100、0=出力#1最小/出力#2最大、100=出力#1最大/出力#2最小） | Scale/Slider |
| **stdout出力先** | stdout出力の出力先を選択する出力#1/出力#2ラジオボタン | Radio button |
| **出力をクリア** | 両方の出力パネルをクリアするボタン | Button |
| **ウィジェットモード** | 7モードのコンボボックス（§5.5参照） | Combobox |
| **ソフトウェアコントローラーの位置** | 右パネル内の位置を指定するTOP/BOTTOMラジオボタン | Radio button |
| **ダイアログボタンの位置** | ダイアログボタン配置用のTOP/BOTTOM/BOTHラジオボタン | Radio button |

#### 6.6.2 今後のバージョンで実装予定の項目

- 将来機能については **§17.2 その他将来的機能** を参照。

---

## 7. 通信プロトコル

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
- **シグナリング**: HTTPベースのSDP交換（実装詳細は別途決定）。
- **自動再接続**: 接続断時に3秒ごとに再試行。

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
| ABR対応 | 帯域に応じた動的解像度・ビットレート変更 |
| HWデコード | ブラウザネイティブのハードウェアデコードを活用（CPU負荷低減） |

**ブラウザサポート**:

| ブラウザ | 対応状況 |
|---------|---------|
| Safari 26.0+（iOS 26/macOS 26） | 完全対応（Video + Audio） |
| Safari 16.4-18.7 | Videoのみ対応（Audio非対応） |
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
- **自動再接続**: 接続断時に3秒ごとに再試行。
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
- **コード生成**: TypeScriptクライアント型用の `openapi-typescript`。
- **認証**: なし（ローカル/LAN専用）。
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

- **ショートカット**: F5 = 開始、F6 = 停止、F7 = 一時停止、F8 = 再開、F9 = リロード、ESC = 緊急停止。
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

### 10.1 設計方針

- **コアはRust**: すべてのコア処理はRustで実装。Pythonは必要な部分のみ（ユーザースクリプトAPI、互換レイヤー）。
- **メタクラスによる切り替え**: `CommandMeta`が将来の実装切り替え用フックを提供。現状はすべてPyO3（Rustバインディング）に流れる。
- **後方互換性**: リファクタリング前のスクリプトは変更なしで動作する必要がある。
- **型ヒント**: 新APIは動作する型ヒントを持つ。旧APIは非推奨として保持される。

### 10.2 コマンドクラスAPI

**注**: 以下のクラスはRust/PyO3で実装され、Pythonファイル（`commands.py`等）は型ヒント・ドキュメント・互換レイヤーのみを提供する。実際の処理はRust側で行われる。

#### 10.2.1 PythonCommand
- `pokecon.dialogue` — ダイアログ関数
- `pokecon.image_proc` — 画像処理（opencv-rust）
- `pokecon.net` — Socket、MQTT、HTTPクライアント

**注**: `events.py`（動的設定用EventBus）は§11「設定ファイルシステム」に含まれる。

### 10.3 コマンドクラス

#### 10.3.1 クラス階層

```
Command (ABC, metaclass=CommandMeta)
├── PythonCommand (ABC)
│   └── ImageProcPythonCommand (ABC)
└── McuCommandBase
```

#### 10.3.2 PythonCommand

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
| `press()` | `press(buttons, duration=0.1, wait=0.1)` | ボタンをduration秒押下後解放、wait秒待機 |
| `pressRep()` | `pressRep(buttons, repeat, duration=0.1, interval=0.1, wait=0.1)` | 繰り返し押下 |
| `hold()` | `hold(buttons, wait=0.1)` | ボタンを押下状態で保持 |
| `holdEnd()` | `holdEnd(buttons)` | 保持中のボタンを解放 |
| `wait()` | `wait(wait: float)` | wait秒スリープ |
| `short_wait()` | `short_wait(wait: float)` | ビジーループ待機（高精度） |
| `direct_serial()` | `direct_serial(commands, waittimes)` | 生シリアルコマンド送信 |
| `reload_com_port()` | `reload_com_port()` | COMポート接続を再読み込み |

**出力メソッド**（PyO3実装）:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `print_t1()` | `print_t1(*objects, sep=' ', end='\\n')` | 上部ログパネルへ出力 |
| `print_t2()` | `print_t2(*objects, sep=' ', end='\\n')` | 下部ログパネルへ出力 |
| `print_t()` | `print_t(*objects, sep=' ', end='\\n')` | stdout以外のログパネルへ出力 |
| `print_s()` | `print_s(*objects, sep=' ', end='\\n')` | stdout割り当てパネルへ出力 |
| `print_ts()` | `print_ts(*objects, sep=' ', end='\\n')` | `print_s`と同じ |
| `print_t1b()` | `print_t1b(mode, *objects, sep=' ', end='\\n')` | 上部ログ（モード付き w/a/d） |
| `print_t2b()` | `print_t2b(mode, *objects, sep=' ', end='\\n')` | 下部ログ（モード付き） |
| `print_tb()` | `print_tb(mode, *objects, sep=' ', end='\\n')` | stdout以外ログ（モード付き） |
| `print_tbs()` | `print_tbs(mode, *objects, sep=' ', end='\\n')` | stdoutログ（モード付き） |
| `show_var()` | `show_var(var, widget='print_t1')` | 変数の値を指定ウィジェットに表示 |

**ダイアログメソッド**（ブロッキングWebポップアップ）:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `dialogue()` | `dialogue(title, message, desc=None, need=list)` | 単純入力ダイアログ（非推奨、show_dialog使用） |
| `dialogue6widget()` | `dialogue6widget(title, dialogue_list, desc=None, need=list)` | マルチウィジェットダイアログ（非推奨、show_dialog使用） |

**Socketメソッド**:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `socket_connect()` | `socket_connect()` | Socketサーバへ接続 |
| `socket_disconnect()` | `socket_disconnect()` | Socketサーバから切断 |
| `socket_transmit_message()` | `socket_transmit_message(message)` | Socket経由でメッセージ送信 |
| `socket_receive_message()` | `socket_receive_message(header, show_msg=False)` | ヘッダーフィルタ付き受信 |
| `socket_receive_message2()` | `socket_receive_message2(headerlist, show_msg=False)` | 複数ヘッダーフィルタ付き受信 |
| `socket_change_ipaddr()` | `socket_change_ipaddr(addr)` | Socket IPアドレス変更 |
| `socket_change_port()` | `socket_change_port(port)` | Socketポート変更 |
| `socket_change_alive()` | `socket_change_alive(flag)` | Socket aliveフラグ設定 |

**MQTTメソッド**:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `mqtt_transmit_message()` | `mqtt_transmit_message(roomid, message)` | MQTTトピックへメッセージ公開 |
| `mqtt_receive_message()` | `mqtt_receive_message(roomid, header, show_msg=False)` | ヘッダーフィルタ付き購読/受信 |
| `mqtt_receive_message2()` | `mqtt_receive_message2(roomid, headerlist, show_msg=False)` | 複数ヘッダーフィルタ付き購読 |
| `mqtt_change_broker_address()` | `mqtt_change_broker_address(broker_address)` | MQTTブローカーアドレス変更 |
| `mqtt_change_id()` | `mqtt_change_id(mqtt_id)` | MQTTクライアントID変更 |
| `mqtt_change_clientId()` | `mqtt_change_clientId(clientId)` | MQTT接続名変更 |
| `mqtt_change_pub_token()` | `mqtt_change_pub_token(pub_token)` | 公開トークン変更 |
| `mqtt_change_sub_token()` | `mqtt_change_sub_token(sub_token)` | 購読トークン変更 |

**通知メソッド**:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `discord_text()` | `discord_text(content='', index=0, keys='DISCORD_WEBHOOK')` | Discord webhook経由でテスト送信 |
| `discord_image()` | `discord_image(content='', index=0, crop_fmt='', crop=None, keys='DISCORD_WEBHOOK')` | Discord webhook経由でテキスト+スクリーンショット送信 |
| `LINE_text()` | `LINE_text(txt, token='')` | No-opスタブ（LINEサービスEOL） |
| `LINE_image()` | `LINE_image(txt, crop_fmt='', crop=None, token='')` | No-opスタブ（LINEサービスEOL） |
| `win_notification()` | `win_notification()` | Windowsデスクトップトースト通知 |

#### 10.3.3 ImageProcPythonCommand

**Import**: `from Commands.PythonCommandBase import ImageProcPythonCommand`

`PythonCommand`を拡張し、カメラと画像処理機能を追加。

**コンストラクタ**: `ImageProcPythonCommand(cam, gui=None)`

**画像処理メソッド**（Rust opencv-rust実装）:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `isContainTemplate()` | `isContainTemplate(template_path, threshold=0.7, use_gray=True, ...)` | カメラフレームに対するテンプレートマッチング |
| `isContainTemplate_max()` | `isContainTemplate_max(template_path_list, threshold=0.7, ...)` | マルチテンプレートマッチング |
| `isContainTemplateGPU()` | `isContainTemplateGPU(template_path, threshold=0.7, ...)` | GPU高速テンプレートマッチング |
| `isContainedImage()` | `isContainedImage(image_path, threshold=0.7, ...)` | 逆テンプレートマッチング |
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

#### 10.3.4 McuCommandBase

**Import**: `from Commands.McuCommandBase import McuCommandBase`

ファームウェアベースコマンド用。PythonCommandと同じメタクラス切り替え。

### 10.4 キー入力・シリアル送信

#### 10.4.1 KeyPress

- ユーザースクリプトに**直接公開されない**
- `self.keys.neutral()`のみアクセス可能（コントローラーをニュートラル状態にリセット）
- 内部実装はRust、PyO3経由で公開

#### 10.4.2 Sender

**PyO3実装**（限定公開API）:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `writeRow()` | `writeRow(row: str)` | シリアル行を書き込み |
| `ser.write()` | `ser.write(data)` | 直接シリアル書き込み（PyO3でpySerial互換型変換） |

その他のSenderメソッドはSenderクラスとして公開されず、適切な他クラスに統合。

### 10.5 ダイアログAPI（型安全）

**非推奨**: `dialogue()`、`dialogue6widget()` — 互換性のために保持、非推奨マーク。

**新API**: `show_dialog()` — 事前に作成したWidgetインスタンスを渡す方式。

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
    
    def __init__(self, widget_type, label, *args, **kwargs) -> None:
        self.widget_type = widget_type
        self.label = label
        self.value: T | None = None  # ダイアログ後に結果を格納

# 事前にWidgetインスタンスを作成
entry = Widget("Entry", "名前", "デフォルト")  # Widget[str]
check = Widget("Check", "有効", True)  # Widget[bool]

# ブロッキング（デフォルト）
dialog_id = show_dialog("タイトル", widgets=[entry, check])
# dialog_id == 0
print(entry.value)  # str
print(check.value)  # bool

# 非ブロッキング
dialog_id = show_dialog("タイトル", widgets=[entry, check], blocking=False)
# dialog_id > 0（固有の自然数）
# スクリプトの実行は継続される

# ダイアログが終了したか確認
if is_dialog_closed(dialog_id):
    print(entry.value)

# ブロッキング動作に切り替え
wait_dialog(dialog_id)
print(entry.value)
```

#### 10.5.1 ブロッキング（デフォルト）

- `show_dialog(title: str, widgets: list[Widget] | Widget, blocking: bool = True) -> int`
- `blocking=True`の場合、ダイアログが閉じられるまでスクリプトの実行を停止
- 返り値は`0`
- 結果は各Widgetの`value`属性に格納される

#### 10.5.2 非ブロッキング

- `show_dialog(title: str, widgets: list[Widget] | Widget, blocking: bool = False) -> int`
- `blocking=False`の場合、ダイアログを表示し、スクリプトの実行を継続
- 返り値はユーザースクリプトが開始してから停止するまでの間で固有の自然数（ダイアログID）
- 結果は各Widgetの`value`属性に格納される

#### 10.5.3 ダイアログ状態確認

- `is_dialog_closed(dialog_id: int) -> bool`
- 指定したダイアログIDのダイアログが終了しているかどうかを確認
- 終了していれば`True`、表示中または未表示であれば`False`

#### 10.5.4 ダイアログ待機

- `wait_dialog(dialog_id: int) -> None`
- 指定したダイアログIDのダイアログが終了するまでブロッキングで待機
- 非ブロッキングで表示したダイアログを後からブロッキング動作に切り替える際に使用

---

## 11. 設定ファイルシステム

### 11.1 設定の種類と対象ユーザー

| 種類 | ファイル | 言語 | 用途 | 対象ユーザー |
|------|---------|------|------|------------|
| **静的設定** | `settings.toml` | TOML | グローバル設定、プロファイル管理 | **全ユーザー** |
| **動的設定** | `init.py` | Python | イベントハンドラ、カスタムロジック | **パワーユーザー** |
| **動的設定** | `init.lua` | Lua | イベントハンドラ、カスタムロジック | **パワーユーザー** |

**重要**: TOMLは**動的ではない**。Python/Luaのみが動的設定ファイルとして使用される。

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

**注記**: 動的設定（⑤）が最も優先されるのは、パワーユーザーが最終的な制御権を持つことを意図した設計です。CLI引数（④）で一時的な上書きを行っても、動的設定ファイルで恒久的な設定を適用できます。ただし、動的設定の自動リロードが有効な場合はCLI引数での設定が上書きされる可能性があります（§11.5参照）。

### 11.4 静的設定（settings.toml）

```toml
# ~/.config/pokecon/settings.toml
[global]
language = "ja"
auto_reload_config = false  # 動的設定ファイルの自動リロード（デフォルト無効）

[python]
# Pythonバージョン（オプション、デフォルト推奨）
# version = "3.12"

# ユーザー追加ライブラリ
[[python.packages]]
name = "requests"
version = ">=2.28.0"

[[python.packages]]
name = "numpy"

[profiles]
active = "default"
```

### 11.5 動的設定の読み込みタイミング

| タイミング | 動作 |
|-----------|------|
| **アプリケーション起動時** | 自動読み込み（`init.py` / `init.lua`） |
| **プロファイル切替時** | 自動読み込み（新プロファイルの設定を反映） |
| **手動** | メニュー「Load Dynamic Config」で読み込み |
| **自動リロード** | ファイル変更検知時（デフォルト無効、オプトイン） |

**メニュー項目**（§14.1参照）:
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

# カメラ設定（フラット構造）
# 注: UIのComboboxは1-30fpsだが、動的設定では60fpsも設定可能
pokecon.opt.camera_fps = 60
pokecon.opt.camera_resolution = "1280x720"

# シリアル設定（フラット構造）
pokecon.opt.serial_port = "COM3"
pokecon.opt.serial_baudrate = 115200
pokecon.opt.serial_data_format = "default"  # default | qingpi | 3ds

# 通知設定（フラット構造）
pokecon.opt.discord_webhook_url = "https://discord.com/api/webhooks/..."
pokecon.opt.discord_username = "PokeCon Bot"

# ウィジェットモード（フラット構造）
pokecon.opt.widget_mode = "mode1"  # mode1〜mode7

# ソフトウェアコントローラー位置
pokecon.opt.controller_position = "top"  # top | bottom

# ダイアログボタン位置
pokecon.opt.dialog_button_position = "bottom"  # top | bottom | both
```

**動的設定の特徴**:
- **即時反映**: 設定変更は即座にUIに反映される
- **永続化なし**: 動的設定はファイルとして保存されるが、Rust側の設定マネージャーとは別の経路で読み込まれる
- **優先順位**: 動的設定 > 静的設定（settings.toml）
- **エラーハンドリング**: 構文エラーの場合はその行をスキップし、残りを続行

### 11.8 動的設定（Lua）

```lua
-- ~/.config/pokecon/init.lua
-- require不要で pokecon.* に直接アクセス

-- カメラ設定（フラット構造）
pokecon.opt.camera_fps = 60
pokecon.opt.serial_port = "COM3"

-- キーマッピング（Neovim風記法）
pokecon.keymap.set("A", function()
    pokecon.input.press(pokecon.keys.Button.A)
end)

-- イベントハンドラ
pokecon.autocmd.on("CameraOpenPost", {
    callback = function()
        print("Camera opened")
    end
})

-- 相互参照
pokecon.source("~/.config/pokecon/extra_settings.lua")

-- 状態取得
print(pokecon.state.serial_port)
print(pokecon.state.active_profile)
```

### 11.9 エラーハンドリング

- 動的設定ファイル読み込み時にエラーが発生しても、アプリケーションは継続して動作
- エラー内容はログパネルに出力（行番号・ファイル名・エラー内容）
- フォールバック機構により、前回の有効な設定を維持

### 11.10 Luaランタイム

Luaランタイムの実装にはmluaクレート（LuaJIT + vendored features）を使用します。

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
- **Pre/Postフェーズ**: すべてのイベントは `Pre`（事前）と `Post`（事後）の2フェーズを持つ
- **フェーズはイベント名に含める**: `phase` 引数ではなく、イベント名自体に `Pre`/`Post` を含める（LSP警告のため）
- **require不要**: Lua設定では `require` なしで `pokecon.*` にアクセス可能
- **Python/Lua両対応**: 両言語で同じAPI構造を使用

#### 11.12.2 名前空間設計

| 名前空間 | 用途 | API |
|---------|------|-----|
| `pokecon.autocmd` | イベントハンドラの登録・解除 | `on()`, `once()`, `off()`, `clear(group)` |
| `pokecon.event` | イベント定義・発火 | `define()`, `emit()`, `list_defined()`, `get_schema()` |

#### 11.12.3 イベントハンドラAPI

```python
# Python設定
import pokecon

# 基本的なイベント登録
# 戻り値: HandlerId（ハンドラ解除用）
# callback: 引数なし（デフォルト）。pokecon.state に直接アクセスして情報を取得
handler_id = pokecon.autocmd.on("CameraOpenPost", callback=lambda: print("Camera opened"))

# 一度だけ実行
pokecon.autocmd.once("SerialConnectPost", callback=lambda: print("Serial connected"))

# イベントハンドラ解除
# 引数: HandlerId（on() / once() の戻り値）
pokecon.autocmd.off(handler_id)

# グループ単位で一括解除
# "all" = すべてのハンドラ解除
# "CameraOpenPost" = そのイベントの全ハンドラ解除
# "my_group" = ユーザ定義グループの全ハンドラ解除
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
-- Lua設定（Neovim風require-less）
pokecon.autocmd.on("CameraOpenPost", {
    callback = function()
        print("Camera opened")
    end,
    group = "my_group"
})

pokecon.autocmd.once("SerialConnectPost", {
    callback = function()
        print("Serial connected")
    end
})

-- イベントハンドラ解除
-- 引数: HandlerId（on() / once() の戻り値）
pokecon.autocmd.off(handler_id)

-- グループ単位で一括解除
-- "all" = すべてのハンドラ解除
-- "CameraOpenPost" = そのイベントの全ハンドラ解除
-- "my_group" = ユーザ定義グループの全ハンドラ解除
pokecon.autocmd.clear("all")
pokecon.autocmd.clear("CameraOpenPost")
pokecon.autocmd.clear("my_group")
```

**Luaでのグループ指定例**:

```lua
-- グループを指定して登録
pokecon.autocmd.on("CameraOpenPost", {
    callback = function()
        print("Camera opened")
    end,
    group = "camera_group"
})

pokecon.autocmd.on("CameraClosePost", {
    callback = function()
        print("Camera closed")
    end,
    group = "camera_group"
})

-- グループ単位で一括解除
pokecon.autocmd.clear("camera_group")
```

**コールバックシグネチャ**:
- **デフォルト**: 引数なし。コールバック内で `pokecon.state` に直接アクセスして情報を取得
- **将来の拡張**: 引数あり（`lambda event: print(event.data)`）。実装時に都合が良い方を選択可能
- イベントごとに異なるフィールドを持つ（§11.12.5参照）
- LSP対応: 実装時に `TypedDict` または `@dataclass` で各イベントのデータ型を定義し、`@overload` でイベント名に応じた型ヒントを提供

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
-- Lua設定
pokecon.event.define("MyCustomEvent")
pokecon.event.emit("MyCustomEvent", {key = "value"})
print(pokecon.event.list_defined())
```

#### 11.12.5 組み込みイベント一覧

| イベント名 | フェーズ | 説明 | イベントデータ（将来の拡張時にコールバック引数として使用） |
|-----------|---------|------|------------------|
| `AppStartupPost` | Post | アプリケーション起動後 | `{"pid": int}` |
| `AppShutdownPre` | Pre | アプリケーション終了前 | `{}` |
| `SerialConnectPost` | Post | シリアルポート接続後 | `{"port": str, "baudrate": int}` |
| `SerialDisconnectPost` | Post | シリアルポート切断後 | `{"port": str}` |
| `CameraOpenPost` | Post | カメラオープン後 | `{"device_id": str, "resolution": tuple[int, int]}` |
| `CameraClosePost` | Post | カメラクローズ後 | `{"device_id": str}` |
| `CommandStartPre` | Pre | コマンド実行開始前 | `{"command_name": str, "command_id": str}` |
| `CommandStartPost` | Post | コマンド実行開始後 | `{"command_name": str, "command_id": str}` |
| `CommandStopPost` | Post | コマンド停止後 | `{"command_name": str, "command_id": str}` |
| `CommandErrorPost` | Post | コマンドエラー発生後 | `{"command_name": str, "error": str}` |
| `ScriptLoadPre` | Pre | スクリプト読み込み前 | `{"source_dirs": list[str], "candidate_count": int}` |
| `ScriptLoadPost` | Post | スクリプト読み込み後 | `{"commands": list[CommandInfo], "loaded_count": int}` |
| `ConfigReloadPost` | Post | 設定再読み込み後 | `{"config_path": str}` |
| `InputPressedPre` | Pre | 入力押下前 | `{"button": str}` |
| `InputReleasedPost` | Post | 入力解放後 | `{"button": str}` |

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
    "SerialConnectPost", "SerialDisconnectPost",
    "CameraOpenPost", "CameraClosePost",
    "CommandStartPre", "CommandStartPost",
    "CommandStopPost", "CommandErrorPost",
    "ScriptLoadPre", "ScriptLoadPost",
    "ConfigReloadPost",
    "InputPressedPre", "InputReleasedPost"
]

# 組み込みイベント + ユーザー定義イベント
EventName = Union[BuiltinEvent, str]
```

**イベントデータ型**（実装時に TypedDict または @dataclass で定義）:

```python
from typing import TypedDict

class ScriptLoadPreData(TypedDict):
    source_dirs: list[str]
    candidate_count: int

class ScriptLoadPostData(TypedDict):
    commands: list[CommandInfo]
    loaded_count: int

class CameraOpenPostData(TypedDict):
    device_id: str
    resolution: tuple[int, int]

# ... その他のイベントデータ型
```

#### 11.12.7 コールバックシグネチャ

コールバックシグネチャの仕様は **§11.12.3 コールバックシグネチャ** を参照。内容は同一です。

#### 11.12.8 エラーハンドリング

- イベントハンドラ内でエラーが発生しても、他のハンドラは継続して実行
- エラー内容はログに出力（イベント名、ハンドラID、エラーメッセージ、スタックトレース）
- フォールバック機構により、システム全体の動作を停止しない

**エラーの種類と挙動**:

| エラー種類 | 挙動 | ログ出力 |
|-----------|------|---------|
| コールバック内の例外 | 当該ハンドラのみ停止、他は継続 | ERRORレベル |
| 存在しないイベントへのemit | 無視（ハンドラがないだけ） | WARNINGレベル |
| ハンドラ登録時の無効なイベント名 | 登録拒否、例外を送出 | ERRORレベル |
| 循環参照（イベント発火中に同じイベントを発火） | 検出して無視 | ERRORレベル |

### 11.13 キーマップシステム

#### 11.13.1 設計方針

- **Neovim風キー記法**: `<C-a>`, `<S-a>`, `<M-a>`, `<C-S-a>` 等
- **フラットAPI**: `pokecon.keymap.set(key, callback, state)`
- **状態指定**: `press`（デフォルト）, `release`, `hold`

#### 11.13.2 API仕様

```python
# Python設定
import pokecon

# 基本的なキーマッピング
# 戻り値: bool（成功: True, 失敗: False）
pokecon.keymap.set("A", lambda: pokecon.input.press(pokecon.keys.Button.A))

# 修飾キー付き
pokecon.keymap.set("<C-a>", lambda: print("Ctrl+A pressed"), state="press")
pokecon.keymap.set("<S-a>", lambda: print("Shift+A pressed"), state="hold")
pokecon.keymap.set("<M-a>", lambda: print("Alt+A pressed"), state="release")
pokecon.keymap.set("<C-S-a>", lambda: print("Ctrl+Shift+A pressed"))

# 特殊キー
pokecon.keymap.set("<F1>", lambda: print("F1 pressed"))
pokecon.keymap.set("<Space>", lambda: print("Space pressed"))
pokecon.keymap.set("<Enter>", lambda: print("Enter pressed"))
pokecon.keymap.set("<Esc>", lambda: print("Escape pressed"))
```

```lua
-- Lua設定
pokecon.keymap.set("A", function()
    pokecon.input.press(pokecon.keys.Button.A)
end)

pokecon.keymap.set("<C-a>", function()
    print("Ctrl+A pressed")
end, {state = "press"})
```

#### 11.13.3 サポートするキー記法

| 記法 | 説明 | 例 |
|------|------|-----|
| `<C-x>` | Ctrl + x | `<C-a>`, `<C-c>` |
| `<S-x>` | Shift + x | `<S-a>`, `<S-1>` |
| `<M-x>` | Alt + x | `<M-a>`, `<M-F4>` |
| `<C-S-x>` | Ctrl + Shift + x | `<C-S-a>` |
| `<F1>`〜`<F12>` | ファンクションキー | `<F1>`, `<F12>` |
| `<Space>` | スペースキー | `<Space>` |
| `<Enter>` | エンターキー | `<Enter>` |
| `<Esc>` | エスケープキー | `<Esc>` |
| `<Tab>` | タブキー | `<Tab>` |
| `<Up>`/`<Down>`/`<Left>`/`<Right>` | 方向キー | `<Up>`, `<Down>` |

#### 11.13.4 キー重複時の優先順位

- 後から登録されたキーバインドが優先される（後勝ち）
- 同じキーに複数のコールバックが登録されている場合、最後に登録されたものが実行される
- プロファイル切替時は、新プロファイルのキーバインドに置き換えられる

#### 11.13.5 デフォルトキーバインド

| キー | 動作 | 状態 |
|------|------|------|
| `<F5>` | コマンド開始 | press |
| `<F6>` | コマンド停止 | press |
| `<F7>` | コマンド一時停止 | press |
| `<F8>` | コマンド再開 | press |
| `<F9>` | コマンドリロード | press |
| `<Esc>` | 緊急停止 | press |

### 11.14 相互参照API

#### 11.14.1 設計方針

- **Neovimの`:source`に類似**: `pokecon.source(path)`
- **拡張子で自動判別**: `.py` → Python, `.lua` → Lua
- **相対パス・絶対パス両対応**

#### 11.14.2 API仕様

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
-- Lua設定
pokecon.source("~/.config/pokecon/extra_settings.lua")
```

#### 11.14.3 エラーハンドリング

- 指定されたファイルが存在しない場合はエラーをログに出力
- ファイルの読み込みに失敗しても、現在の設定は維持される
- 循環参照（AがBを読み込み、BがAを読み込む）を検出し、エラーを出力

### 11.15 状態取得API

#### 11.15.1 設計方針

- **読み取り専用**: `pokecon.state.<property>`
- **リアルタイム**: 現在の状態を即座に反映
- **スレッドセーフ**: 複数スレッドから安全に読み取り可能

#### 11.15.2 利用可能な状態プロパティ

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
print(pokecon.state.current_command)    # 現在実行中のコマンド名
print(pokecon.state.command_candidates) # 読み込み候補コマンド一覧（list[CommandInfo]）
print(pokecon.state.tags)               # 利用可能なタグ一覧（list[str]）

# プロファイル関連
print(pokecon.state.active_profile)     # 現在のアクティブプロファイル名
print(pokecon.state.available_profiles) # 利用可能なプロファイル一覧

# 入力関連
print(pokecon.state.last_input)         # 最後の入力
print(pokecon.state.holding_buttons)    # 現在保持中のボタン一覧
```

```lua
-- Lua設定
print(pokecon.state.serial_port)
print(pokecon.state.camera_opened)
print(pokecon.state.active_profile)
```

### 11.16 プロファイルAPI

#### 11.16.1 設計方針

- **フラットAPI**: `pokecon.profile.current()`, `pokecon.profile.list()`, `pokecon.profile.switch(name)`
- **動的設定ファイル内で使用可能**

#### 11.16.2 API仕様

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
-- Lua設定
print(pokecon.profile.current())
print(pokecon.profile.list())
pokecon.profile.switch("custom")
```

#### 11.16.3 プロファイル切替時の動作

- 新しいプロファイルの設定を読み込み（`~/.config/pokecon/profiles/<name>/settings.toml`）
- 動的設定ファイル（`~/.config/pokecon/init.py`/`init.lua`）を自動再読み込み
- イベントハンドラをクリアして再登録
- キーマップをクリアして再登録

## 12. 環境変数

| 変数 | 説明 | デフォルト |
|----------|-------------|---------|
| `POKECON_DISABLE_COMPOSITING` | コンポジットモードを無効化（Tauri） | `0` |
| `POKECON_WEB_DIR` | 静的ファイルディレクトリ | `web/dist` |
| `POKECON_PORT` | HTTPサーバーポート | `8020` |

## 13. クライアント側ストレージ

| 項目 | 保存方法 | 備考 |
|------|---------------|-------|
| ショートカットボタン割り当て | `localStorage` | 10ボタンキーバインド |
| キーボード設定 | `localStorage` | キーマッピング設定 |

## 14. 動的設定ファイルのUI

### 14.1 メニュー配置

- **配置場所**: メニューバー内
- **項目**: 単一の「Load Dynamic Config」メニュー項目

```
File
├── Load Dynamic Config      ← 新規読み込み（拡張子で自動判別）
├── Reload Dynamic Config    ← 現在のファイルを再読み込み
└── Open Config Directory    ← 設定ディレクトリを開く
```

### 14.2 ファイル選択と自動判別

- **ファイル選択ダイアログ**: 単一の「Load Dynamic Config」メニューから開く
- **自動判別**: 拡張子で言語を自動判別
  - `.py` → Python動的設定ファイル
  - `.lua` → Lua動的設定ファイル
- **手動指定**: 拡張子が不明な場合はユーザーに選択を促す

### 14.3 リロード機能

| 機能 | 説明 |
|------|------|
| **手動リロード** | 「Reload Dynamic Config」メニューで現在のファイルを再読み込み |
| **自動リロード** | ファイルウォッチャーによる自動リロード（**デフォルトで無効**） |
| **有効化方法** | `pokecon.opt.auto_reload_config = True` またはUI設定 |

### 14.4 エラーハンドリング

- 動的設定ファイル読み込み時にエラーが発生しても、アプリケーションは継続して動作
- エラー内容はログパネルに出力
- フォールバック機構により、前回の有効な設定を維持

---

## 15. スクリプト互換性要件

| 要件 | 状態 |
|------|------|
| サンプルスクリプトが変更なしで動作 | ✅ 必須 |
| `from Commands.PythonCommandBase import PythonCommand` | ✅ モジュールパッチで保持 |
| `from Commands.Keys import Button, Hat, ...` | ✅ モジュールパッチで保持 |
| `self.keys.neutral()` | ✅ 利用可能 |
| `self.keys.ser.writeRow()` | ✅ 利用可能 |
| `self.keys.ser.ser.write()` | ✅ 利用可能（Rustシリアルラッパー） |
| 画像処理API | ✅ Rust実装（opencv-rust） |
| Discord通知 | ✅ 実装済み |
| LINE通知 | ⚠️ No-opスタブ（サービスEOL） |
| Windows通知 | ✅ 実装済み |

## 16. 開発環境自動構築

### 16.1 ディレクトリ構造

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

### 16.2 設定ファイル生成タイミング

- **存在しない時に生成**（初回、アップデート、削除後等）
- **nix環境**: nix式で指定した場合のみnix側で生成。指定しなかった場合はアプリ起動時に存在しないためアプリ側で生成。
- **非nix環境**: アプリ側で自動生成

### 16.3 Python管理（nix環境）

nix環境では、Pythonインタープリターのパスを**ビルド時にnixストアパスとして埋め込む**。

**実装方式**:

```rust
// rust/pokecon-core/build.rs
// nix flakeから渡されたPOKECON_PYTHON_PATHを読み込み、ソースコードに埋め込む

fn main() {
    // nixビルド時に環境変数として渡される（flake.nixで設定）
    let python_path = env::var("POKECON_PYTHON_PATH")
        .unwrap_or_else(|_| "/usr/bin/python3".to_string());
    
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("python_path.rs");
    
    fs::write(&dest_path, format!(
        r#"pub const PYTHON_PATH: &str = "{}";"#,
        python_path
    )).unwrap();
    
    println!("cargo:rerun-if-env-changed=POKECON_PYTHON_PATH");
}
```

```rust
// rust/pokecon-core/src/python.rs
include!(concat!(env!("OUT_DIR"), "/python_path.rs"));

pub fn get_python_path() -> &'static str {
    PYTHON_PATH
}
```

```nix
# flake.nix（抜粋）
# nix式でPythonパッケージを指定した場合、POKECON_PYTHON_PATHを設定
pythonEnv = pkgs.python3.withPackages (ps: [ ... ]);

pokecon-server = rustPlatform.buildRustPackage {
  # ...
  POKECON_PYTHON_PATH = "${pythonEnv}/bin/python";
  # ...
};
```

**特徴**:
- nixストアパスは不変なため、再現性が保証される
- グローバルPythonを使用しない（nixの隔離性を維持）
- 非nix環境では環境変数が未設定のため、実行時に別途Pythonを取得するフォールバック動作

### 16.4 Python管理（非nix環境）

```rust
// Rust側
struct PythonManager {
    data_dir: PathBuf,           // ~/.local/share/pokecon/
    expected_python_version: Option<String>, // オプション（デフォルト推奨）
}

impl PythonManager {
    fn ensure_python(&self) -> PathBuf {
        // 1. 期待するバージョンがない場合はデフォルトを使用
        // 2. 既存のPythonが期待するバージョンかチェック
        // 3. ない場合はastral-sh/python-build-standaloneをダウンロード
        // 4. 仮想環境を構築
        // 5. 必須パッケージ + ユーザーパッケージをインストール
    }
}
```

### 16.5 必須パッケージ管理

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

### 16.6 ユーザーパッケージ設定

```toml
# ~/.config/pokecon/settings.toml
# ユーザーが触る設定ファイル（必須パッケージは含まない）

[python]
# Pythonバージョン（オプション、デフォルト推奨）
# version = "3.12"  # 非推奨: 基本的にはデフォルトを使用

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

### 16.7 LSP設定（pyproject.toml）

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

### 16.8 Lua LSP設定（.luarc.json）

```json
{
    "$schema": "https://raw.githubusercontent.com/LuaLS/vscode-lua/master/setting/schema.json",
    "workspace.library": [
        "/home/username/.local/share/pokecon/lua-typings"
    ]
}
```

## 17. 今後のバージョンで実装予定

### 17.1 PWA要件

> **ステータス: 未実装 — 今後のバージョンで実装予定。**  
> 以下の要件は将来の実装のためのユーザー要求として記録されています。現在のスコープには含まれません。

#### 17.1.1 マニフェスト

- アプリメタデータを含む `manifest.json`。
- 全プラットフォーム用のアイコン。
- 表示モード: `standalone`。

#### 17.1.2 サービスワーカー

- UIアセットのオフライン対応。
- キューに入ったコマンドのバックグラウンド同期（将来）。

#### 17.1.3 インストールプロンプト

- カスタムインストールボタン。
- プラットフォーム固有のインストールガイダンス。

### 17.2 その他将来的機能

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
    """
    def __call__(cls, *args, **kwargs):
        # 将来: cls.__target_implementation__等をチェック
        # 現状: 常にPyO3実装を使用
        return super().__call__(*args, **kwargs)
```

---

*本仕様書は生きたドキュメントです。新しい要件がユーザーから伝達された場合、更新を行う必要があります。*
