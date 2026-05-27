# Poke-Controller Modified Extension — UIリファクタリング仕様書

> **バージョン**: 2.0.0-draft  
> **ブランチ**: `refactor/rust-core`  
> **日付**: 2026-05-21  
> **スコープ**: Web/デスクトップUI（SvelteKit）— バックエンドAPIおよびRustコアは対象外  
> **ソース**: セッション議事録から抽出した過去のユーザー要件（現在のコードベースではない）

---

## 1. 概要

### 1.1 目的

本ドキュメントは、従来のPython/Tkinter UIを置き換えるPoke-Controller Modified Extensionの新しいWeb/デスクトップUIの要件を規定します。本仕様は、リファクタリングセッション中に伝達された**過去のユーザー要件のみ**から導出されており、現在のコードベースからは導出されていません。

### 1.2 設計方針

- **Tkinterとの視覚的パリティ**: 新しいUIは、機能を機能的に再現するだけでなく、元のTkinterのレイアウトと外観に厳密に一致する必要があります。レイアウト、色、ボタンの間隔、ウィジェットの種類はオリジナルに準拠する必要があります。
- **スクリプト互換性**: リファクタリング前のバージョンで動作していたすべてのスクリプトは、引き続き正常に動作する必要があります。スクリプトAPIに破壊的変更は加えません。
- **モダンスタック**: SvelteKit + Svelte 5（runesモード）+ **Tailwind CSS v4**（確定、変更不可）。
- **低遅延通信**: プライマリとしてWebRTC、フォールバックとしてHTTP/MJPEGおよびWebSocketを使用。WebSocketは切断時に3秒ごとに自動再接続。
- **型安全性**: Rustバックエンドから `utoipa` v5 + `openapi-typescript` を介してOpenAPI生成のTypeScript型を使用。
- **認証なし**: アプリケーションはローカル/LAN専用に設計。API認証は不要。
- **Reactコードは完全に削除**: 要件により、Reactフロントエンドコードベースは「ゴミ」と見なされ、参照、インポート、または信頼できる情報源として使用してはなりません。仕様は、Reactの実装ではなく、ユーザーから伝達された元のTkinterレイアウト要件からのみ導出されます。

### 1.3 対象プラットフォーム

| プラットフォーム | UIモード | 備考 |
|----------|---------|-------|
| デスクトップ（Windows/Linux） | Tauri（WebViewラッパー） | Webモードとaxum HTTPサーバーを共有 |
| Webブラウザ | スタンドアロンSvelteKit SPA | axum HTTPサーバーによって提供 |
| モバイル（将来） | レスポンシブSPA | 同一コードベース、アダプティブレイアウト |

---

## 2. UIレイアウト（Tkinterパリティ）

### 2.1 全体構造

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

### 2.2 タブ構造（6メインタブ + 3サブタブ）

> **タブ数に関する注記**: 本仕様では、トップレベルに**6つのメインタブ**を記述します。Commandsタブには**3つのサブタブ**（Python Command、Mcu Command、Shortcut）が含まれ、合計9つの個別のタブ付きインターフェースとなります。PLAN.mdでは「8タブ構造」に言及していますが、これはCommandsサブタブの数え方が異なります。本仕様では、トップレベルのノートブックタブを指す「6メインタブ」で一貫しています。

| # | タブ名 | 優先度 | 説明 |
|--|--------|--------|-------------|
| 1 | **カメラ** | 高 | 映像表示（Canvas/CaptureArea）、デバイス選択、FPS、フリップ、表示モード切替、マウスベースのスティック制御、スクリーンショット |
| 2 | **シリアル** | 高 | COMポート選択、ボーレート、データ形式（3種類）、接続/切断、シリアルモニター |
| 3 | **手動制御** | 高 | ソフトウェア制御（キーボード、マウススティックエミュレーション）、ハードウェア制御（ProController/Xinput、録画）、完全なJoy-ConレイアウトのSwitch Controller Simulator |
| 4 | **コマンド** | 高 | 3サブタブ（Python Command、Mcu Command、Shortcut）、タグフィルター付きコマンドリスト、10ショートカットボタン、実行制御（開始/一時停止/再開/停止/再読み込み） |
| 5 | **通知** | 中 | Windows通知設定、Discord Webhook（URL、ユーザー名、アバター）、LINE UIは完全に削除 |
| 6 | **その他** | 中 | 出力サイズ調整、stdout出力先、ウィジェットモード選択、ソフトウェアコントローラー位置、ダイアログボタン位置、出力クリア |

### 2.3 右側パネル

#### 2.3.1 ソフトウェアコントローラー（Joy-Conレイアウト）

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

#### 2.3.2 出力パネル

- **出力 #1**: プライマリログ/出力表示。
- **出力 #2**: セカンダリログ/出力表示。
- **サイズ調整**: その他タブの「出力サイズ調整」スライダー（0～100）で制御。出力#1と出力#2の比率を決定。
- **ログソース**: バックエンドからWebSocket経由で受信したログ。
- **機能**: 自動スクロール、クリアボタン、クリップボードにコピー、ログレベルフィルタリング。
- **独立ボタン**: その他タブの「出力をクリア」ボタン。

### 2.4 サブタブ構造（コマンドタブ）

Commandsタブには3つのサブタブ（内部タブ）があります:

| # | サブタブ | 説明 |
|--|---------|-------------|
| 1 | **Python Command** | 利用可能なPythonコマンドスクリプトのリスト/ツリー |
| 2 | **Mcu Command** | 利用可能なMCUコマンドスクリプトのリスト/ツリー |
| 3 | **Shortcut** | 10ショートカットボタン割り当てグリッド |

---

## 3. ウィジェットモード（7種類）

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

## 4. タブ仕様

### 4.1 カメラタブ

#### 4.1.1 映像表示

- **表示方法**: 映像レンダリング用のCanvas要素（CaptureArea）。
- **プライマリストリーム**: WebRTCビデオトラック（低遅延）。
- **フォールバック**: WebRTCが利用できない場合、HTTP上のMJPEG（`<img>`タグまたは同等）。
- **フレームレート**: FPS設定（SpinboxまたはCombobox）で設定可能。

#### 4.1.2 カメラ設定

| コントロール | 種類 | 説明 |
|---------|------|-------------|
| **カメラデバイス選択** | Combobox | 利用可能なカメラデバイスのドロップダウン |
| **FPS** | Combobox | 設定可能なフレームレート（1～30fps） |
| **フリップ** | Checkbox | 水平/垂直フリップ切替 |

#### 4.1.3 表示モード切替（チェックボックス）

| モード | 説明 |
|------|-------------|
| **リアルタイム** | ライブ映像表示 |
| **値** | 数値ピクセル値またはオーバーレイデータの表示 |
| **ガイド** | ガイドオーバーレイまたは参照線の表示 |

これらは表示オーバーレイ切替用のチェックボックスです。

#### 4.1.4 キャンバス上のマウス操作

カメラキャンバス（CaptureArea）は以下のマウス操作をサポートします:

| アクション | トリガー | 動作 |
|--------|---------|----------|
| **Lスティック/Rスティック制御** | キャンバス上でマウスドラッグ | ドラッグ方向/距離に基づいて左/右アナログスティック移動をエミュレート |
| **カラーピッカー** | Ctrl+クリック | クリック位置の色の値を取得 |
| **範囲スクリーンショット** | Ctrl+Shift+ドラッグ | キャンバス上の選択した矩形領域のスクリーンショットをキャプチャ |
| **名前付き保存** | Ctrl+Alt+ドラッグ | 選択した領域を名前付きファイルプロンプトに保存 |

#### 4.1.5 スクリーンショットキャプチャ

- **保存場所**: `./Captures/` ディレクトリ。
- **形式**: PNG/JPEG（選択可能）。

#### 4.1.6 カメラバックエンド

- **バックエンド**: OpenCV（Windowsは `cv2.CAP_DSHOW`、Linuxは `cv2.CAP_V4L2`）。
- **スレッド**: フレームキャプチャは別スレッドで実行。

### 4.2 シリアルタブ

#### 4.2.1 接続制御

| コントロール | 種類 | 説明 |
|---------|------|-------------|
| **COMポート選択** | Combobox | 利用可能なCOM/シリアルポートのドロップダウン |
| **更新ボタン** | Button | 利用可能なポートを再スキャン |
| **接続/切断** | Toggle button | 選択したポートに接続または切断 |

#### 4.2.2 設定

| 設定 | オプション | デフォルト |
|---------|---------|---------|
| **ボーレート** | 9600 / 115200 | 9600 |
| **データ形式** | デフォルト / Qingpi / 3DS Controller | デフォルト |

- **デフォルト形式**: ボーレート9600。
- **Qingpi形式**: ボーレート9600。
- **3DS Controller形式**: ボーレート115200。

#### 4.2.3 シリアルモニター

- **コンポーネント**: スクロールバー付きテキストウィジェット。
- **機能**: リアルタイムで入出力シリアルデータを表示。
- **機能**: 最新エントリへの自動スクロール、クリアボタン。

### 4.3 手動制御タブ

#### 4.3.1 ソフトウェア制御セクション

| コントロール | 種類 | 説明 |
|---------|------|-------------|
| **キーボード** | Checkbox | キーボードベースのコントローラー入力を有効化（グローバルホットキー） |
| **Lスティックマウス** | Checkbox | キャンバス上の左アナログスティックのマウスエミュレーションを有効化 |
| **Rスティックマウス** | Checkbox | キャンバス上の右アナログスティックのマウスエミュレーションを有効化 |

#### 4.3.2 ハードウェア制御セクション

| コントロール | 種類 | 説明 |
|---------|------|-------------|
| **ProController** | Radio（Xinput連動） | Switch Pro Controller入力モード |
| **Xinput** | Radio（ProController連動） | Xbox互換コントローラー入力モード |
| **録画** | Checkbox | 入力記録を有効化 |

#### 4.3.3 Switch Controller Simulator

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

### 4.4 コマンドタブ

#### 4.4.1 サブタブ構造

Commandsタブには3つのサブタブ（内部タブ切替）が含まれます:

| サブタブ | 内容 |
|---------|---------|
| **Python Command** | 利用可能なPythonコマンドスクリプトを一覧表示 |
| **Mcu Command** | 利用可能なMCUコマンドスクリプトを一覧表示 |
| **Shortcut** | 10ショートカットボタン割り当てグリッド |

#### 4.4.2 コマンドリスト

- **表示**: 利用可能なコマンドを表示するListboxまたはTreeview。
- **タグフィルター**: ラベル/タグでコマンドをフィルタリングするドロップダウンまたはコンボボックス。
- **列**: コマンド名、タグ、説明（Treeviewの場合）。

#### 4.4.3 ショートカットボタン（10ボタン）

- **数**: 10ショートカットボタン（要件が元の4ボタンから10ボタンに変更）。
- **割り当て**: 読み込まれた任意のコマンドにユーザー割り当て可能（クリックで割り当て、Shift+クリックで割り当て）。
- **クリア**: 右クリックで割り当てをクリア。
- **表示**: ボタンラベルに割り当てられたコマンド名を表示。
- **キーボードショートカット**: F1～F10またはその他の割り当て可能なホットキー。
- **保存**: 設定は `localStorage` に保存。

#### 4.4.4 実行制御

| ボタン | キーボードショートカット | アクション |
|--------|-------------------|--------|
| **開始** | F5 | コマンド実行を開始 |
| **一時停止** | Shift+F6 | 実行を一時停止（再開可能） |
| **再開** | — | 最初から再実行 |
| **停止** | Escape | 実行を即座に中断 |
| **再読み込み** | — | ファイルシステムからコマンドリストを再読み込み |

- **状態表示**: 実行中 / 一時停止中 / 停止 / エラー。
- **進捗**: 対応コマンド用のプログレスバー。

### 4.5 通知タブ

#### 4.5.1 Windows通知

| コントロール | 種類 | 説明 |
|---------|------|-------------|
| **スクリプト開始時に通知** | Checkbox | スクリプト実行開始時にWindows通知を送信 |
| **スクリプト終了時に通知** | Checkbox | スクリプト実行終了時にWindows通知を送信 |
| **テスト** | Button | 設定を確認するためのテスト通知を送信 |

#### 4.5.2 Discord通知

| コントロール | 種類 | 説明 |
|---------|------|-------------|
| **Webhook URL** | Text input | 検証付きのDiscord Webhook URL |
| **ユーザー名** | Text input | Discordメッセージのカスタムユーザー名（オプション） |
| **アバターURL** | Text input | Discordメッセージのカスタムアバター画像URL（オプション） |
| **テスト** | Button | 設定を確認するためのテスト通知を送信 |

#### 4.5.3 LINE通知

- **ステータス**: サービス終了（EOL）— **通知タブからUIは完全に削除**。
- **後方互換性**: 既存のユーザースクリプト用にスクリプトAPI（`notify.line`）は維持。設定用のUIはなし。

### 4.6 その他タブ

#### 4.6.1 設定グループ

| セクション | コントロール | 種類 |
|---------|----------|------|
| **出力サイズ調整** | 出力#1と出力#2の幅比率を制御するスライダー（0～100） | Scale/Slider |
| **stdout出力先** | stdout出力の出力先を選択する出力#1/出力#2ラジオボタン | Radio button |
| **出力をクリア** | 両方の出力パネルをクリアするボタン | Button |
| **ウィジェットモード** | 7モードのコンボボックス（セクション3参照） | Combobox |
| **ソフトウェアコントローラーの位置** | 右パネル内の位置を指定するTOP/BOTTOMラジオボタン | Radio button |
| **ダイアログボタンの位置** | ダイアログボタン配置用のTOP/BOTTOM/BOTHラジオボタン | Radio button |

#### 4.6.2 将来 / フェーズ7項目（優先度低だが必須）

- キー設定エディター（高度なキーバインドUI）。
- Pokémon Home連携（詳細不明 — 予約セクション）。

---

## 5. 通信プロトコル

### 5.1 スタック概要

```
カメラ映像:     WebRTCビデオトラック ──→ MJPEG over HTTP フォールバック
コントローラー入力: WebRTC DataChannel ──→ WebSocket フォールバック
ログ/イベント:  WebRTC DataChannel ──→ WebSocket フォールバック
API呼び出し:    HTTP REST（axum）     ──→ （フォールバック不要）
```

### 5.2 WebRTC（プライマリ）

- **ビデオ**: ビデオトラックを使用したWebRTC `RTCPeerConnection`。
- **DataChannel**: コントローラー入力イベントとログストリーミング用。
- **シグナリング**: HTTPベースのSDP交換。
- **自動再接続**: 接続断時に3秒ごとに再試行。

### 5.3 WebSocket（フォールバック）

- **エンドポイント**: `/ws`。
- **メッセージ**: JSON形式。
- **自動再接続**: 接続断時に3秒ごとに再試行。
- **イベント**:

| イベント | 方向 | ペイロード |
|-------|-----------|---------|
| `camera.frame` | サーバー → クライアント | Base64エンコードJPEGフレームデータ |
| `command.start` | サーバー → クライアント | コマンド実行開始通知 |
| `command.stop` | サーバー → クライアント | コマンド実行停止通知 |
| `command.error` | サーバー → クライアント | コマンド実行エラー詳細 |
| `serial.data` | サーバー → クライアント | シリアルポート受信データ |
| `ping` | 双方向 | キープアライブping |
| `pong` | 双方向 | キープアライブpong応答 |

### 5.4 HTTP REST API

- **フレームワーク**: axum（Rustバックエンド）。
- **ドキュメント**: OpenAPI仕様を使用したutoipa v5。
- **コード生成**: TypeScriptクライアント型用の `openapi-typescript`。
- **認証**: なし（ローカル/LAN専用）。
- **モジュール**: 合計約39エンドポイントの約9モジュール。
- **応答形式**: 一貫した構造のJSON。

### 5.5 キーボード入力API

| メソッド | エンドポイント | 説明 |
|--------|----------|-------------|
| GET | `/api/controller/keyboard` | 現在のキーボード設定を取得 |
| POST | `/api/controller/keyboard` | キーボード設定を設定 |

- **ショートカット**: F5 = 再読み込み、F6 = 開始、ESC = 停止。
- **保存**: キーボード設定は `localStorage` に保存。

### 5.6 マウス入力API

| メソッド | エンドポイント | 説明 |
|--------|----------|-------------|
| GET | `/api/controller/mouse_stick?stick=LSTICK|RSTICK` | マウススティック設定を取得 |
| POST | `/api/controller/mouse_stick` | マウススティック設定を設定（stick、enabled、sensitivity） |
| POST | `/api/input/stick` | スティック入力を送信（`{x: 0–255, y: 0–255}`） |

### 5.7 ゲームパッド入力API

| メソッド | エンドポイント | 説明 |
|--------|----------|-------------|
| GET | `/api/controller/type` | 現在のゲームパッドタイプ設定を取得 |
| POST | `/api/controller/type` | ゲームパッドタイプを設定（`gamepad_type: "ProController" | "Xinput"`） |

- **対応ボタン**: A、B、X、Y、UP、DOWN、LEFT、RIGHT、L、R、ZL、ZR、MINUS、PLUS、HOME、CAPTURE。
- **アナログスティック**: 両軸とも0～255の範囲。
- **タッチパッド**: `{x: 0–320, y: 0–240}` 座標。

---

## 6. 型システム

### 6.1 OpenAPI → TypeScript

- **ソース**: `utoipa` v5マクロを使用したRustバックエンド。
- **生成**: `openapi-typescript` CLI。
- **出力**: `Docs/api/openapi.ts`。
- **使用法**: すべてのAPI呼び出しとWebSocketメッセージは生成された型を使用する必要があります。

### 6.2 型安全性要件

- 厳格なTypeScript（`strict: true`）。
- API関連コードに `any` 型は不使用。
- 外部入力に対するZodまたは同等の実行時検証。

---

## 7. PWA要件

> **ステータス: 未実装 — 将来フェーズのみ。**  
> 以下の要件は将来の実装のためのユーザー要求として記録されています。現在のスコープには含まれません。

### 7.1 マニフェスト

- アプリメタデータを含む `manifest.json`。
- 全プラットフォーム用のアイコン。
- 表示モード: `standalone`。

### 7.2 サービスワーカー

- UIアセットのオフライン対応。
- キューに入ったコマンドのバックグラウンド同期（将来）。

### 7.3 インストールプロンプト

- カスタムインストールボタン。
- プラットフォーム固有のインストールガイダンス。

---

## 8. テーマサポート

> **ステータス: 未実装 — 将来フェーズのみ。**  
> Tailwind CSS v4がスタイリングフレームワークとして確定しています。テーマシステムの要件は以下に記録されています。

### 8.1 組み込みテーマ

- ライトテーマ。
- ダークテーマ。
- システム設定の自動検出。

### 8.2 カスタムテーマ（将来）

- ユーザー定義の配色。
- CSS変数ベースのテーマ。

---

## 9. 設定システム

### 9.1 設定ファイル

> **重要**: `settings.ini` は**廃止**されました。従来のINIベースの設定は、Rustネイティブの設定管理に置き換えられます。正確な形式と保存場所はRustバックエンドチームが決定します（本UI仕様の範囲外）。

### 9.2 環境変数

| 変数 | 説明 | デフォルト |
|----------|-------------|---------|
| `POKECON_DISABLE_COMPOSITING` | コンポジットモードを無効化（Tauri） | `0` |
| `POKECON_WEB_DIR` | 静的ファイルディレクトリ | `web/dist` |
| `POKECON_PORT` | HTTPサーバーポート | `8020` |

### 9.3 クライアント側ストレージ

| 項目 | 保存方法 | 備考 |
|------|---------------|-------|
| ショートカットボタン割り当て | `localStorage` | 10ボタンキーバインド |
| キーボード設定 | `localStorage` | キーマッピング設定 |

---

## 10. 実装フェーズ（PLAN.mdより）

| フェーズ | 説明 | ステータス |
|-------|-------------|--------|
| 0 | プロジェクト設定、CI/CD、Nix flake | ✅ 完了 |
| 1 | SvelteKitスキャフォールド、React削除 | ✅ 完了 |
| 2 | API/OpenAPI統合 | 進行中 |
| 3 | TypeScript CI、リンティング、テスト | 進行中 |
| 4 | コンポーネント再実装 | 保留中 |
| 5 | カメラストリーミング（WebRTC/MJPEG） | 保留中 |
| 6 | ビルド統合、Tauri設定 | 保留中 |
| 7 | 優先度低の機能（キー設定、Pokémon Home） | 保留中 |
| 8 | PWAサポート | 保留中 |
| 9 | テーマサポート（Tailwind v4） | 保留中 |

### 10.1 プロセス要件（ユーザー指示）

以下のプロセス要件はユーザーから明示的に指示されており、全フェーズで遵守する必要があります:

| 要件 | 詳細 |
|-------------|---------|
| **nix flakeによる実行** | すべての開発、テスト、ビルドは `nix flake` コマンドを通じて実行する必要があります。ビルドツールの直接実行は許可されません。 |
| **opencodeレビュー区切り** | すべてのopencodeレビュー境界は論理的な区切り点として機能する必要があります。作業はレビュー境界に合わせて構造化されるべきです。 |
| **フェーズごとにコミットとプッシュ** | 完了した各フェーズはコミットされ、プッシュされる必要があります。複数のフェーズを1つのコミットにまとめることはできません。 |

---

## 11. 主要ユーザー要件と却下事項

このセクションでは、仕様を上書きまたは明確化する明示的なユーザー指示を記録します。

### 11.1 Reactコード — 完全削除

> **要件**: 既存のReactフロントエンドコードベースは「ゴミ」と見なされ、参照、インポート、または信頼できる情報源として使用してはなりません。SvelteKit実装は、Reactの実装ではなく、ユーザーから伝達された元のTkinterレイアウト要件からその仕様を導出する必要があります。

### 11.2 ショートカットボタン — 10個（4個ではない）

> **要件**: Commandsタブには正確に**10個**のショートカットボタンが必要です（4個ではありません）。これは元の数から明示的に変更されました。

### 11.3 実行制御 — 開始/一時停止/再開/停止（開始/停止だけではない）

> **要件**: 実行制御ボタンには**開始、一時停止、再開、および停止**を含める必要があります（開始と停止だけではありません）。一時停止は再開可能でなければなりません。

### 11.4 LINE通知 — UI削除

> **要件**: LINE通知UIは、サービス終了のため通知タブから削除されました。スクリプトAPI（`notify.line`）は後方互換性のためにのみ維持され、UI設定はありません。

### 11.5 設定ファイル — `settings.ini` 廃止

> **要件**: 従来の `settings.ini` ファイル形式は廃止されました。Rustネイティブの設定管理がそれを置き換えます。正確な形式と実装はUI仕様の範囲外です。

### 11.6 PWA — 将来フェーズのみ

> **要件**: PWAの実装は将来のフェーズに延期されます。現在のスコープには、マニフェスト生成、サービスワーカー、またはインストールプロンプトは含まれません。

### 11.8 スクリプト互換性 — リファクタリング前の全スクリプトが動作必須

> **要件**: リファクタリング前（Tkinter/Python）のバージョンで動作していたすべてのスクリプトは、引き続き正常に動作する必要があります。スクリプトAPIに破壊的変更はありません。Python互換レイヤーは完全な後方互換性を維持する必要があります。

> **要件**: テーマサポート（ライト/ダーク/カスタム）は将来のフェーズに延期されます。Tailwind CSS v4はスタイリングフレームワークとして確定しています。

---

## 12. 非機能要件

### 12.1 パフォーマンス

| 指標 | 目標 |
|--------|--------|
| ビデオ遅延（WebRTC） | < 100ms |
| ビデオ遅延（MJPEGフォールバック） | < 300ms |
| コントローラー入力遅延 | < 50ms |
| UI応答性 | 60fpsアニメーション、< 16ms入力応答 |

### 12.2 アクセシビリティ

- すべてのコントロールのキーボードナビゲーション。
- スクリーンリーダー用のARIAラベル。
- ハイコントラストモードのサポート。

### 12.3 ブラウザサポート

| ブラウザ | 最小バージョン |
|---------|----------------|
| Chrome/Edge | 90以上 |
| Firefox | 88以上 |
| Safari | 14以上 |

### 12.4 WebSocket自動再接続

- 接続断時に、3秒ごとに自動的に再接続を試行。
- 一時的なサーバー利用不能を適切に処理する必要があります。

---

## 13. 付録: Tkinter UIリファレンス

### 13.1 元のタブ詳細

元のPython/Tkinter UIは `tkinter.ttk.Notebook` を使用し、以下の構造でした:

- **CameraTab**: スレッド化されたフレームリーダー、PILリサイズ、`ImageTk.PhotoImage` キャンバス表示による `cv2.VideoCapture`。キャンバスはマウス駆動のスティック制御、カラーピッカー、領域スクリーンショットをサポート。
- **SerialTab**: COMポートドロップダウン、ボーレートセレクター（9600/115200）、データ形式セレクター（デフォルト/Qingpi/3DS Controller）、ステータスインジケーター付き接続ボタン、Text+Scrollbar付きシリアルモニター。
- **ManualControlTab**: ソフトウェア制御（キーボードチェックボックス、LStick Mouse、RStick Mouse）、ハードウェア制御（ProController/Xinputラジオ、録画チェックボックス）、完全なJoy-ConレイアウトのSwitch Controller Simulator。
- **CommandTab**: 3サブタブ（Python Command、Mcu Command、Shortcut）、ファイルブラウザー、タグフィルタードロップダウン、Listbox/Treeview付きコマンドリスト、10ショートカットボタン、実行ボタン（開始/一時停止/再開/停止/再読み込み）。
- **NotificationTab**: Discord Webhook URL、ユーザー名、アバターURL入力（テストボタン付き）。Windows通知開始/終了チェックボックス（テストボタン付き）。LINE UI（削除 — サービスEOL）。
- **OthersTab**: 出力サイズ調整スライダー、stdout出力先ラジオ（出力#1/出力#2）、出力をクリアボタン、ウィジェットモードコンボボックス（7モード）、ソフトウェアコントローラー位置ラジオ（TOP/BOTTOM）、ダイアログボタン位置ラジオ（TOP/BOTTOM/BOTH）。

### 13.2 元のコントローラーレイアウト

- **ソフトウェアコントローラー**: CanvasベースのJoy-Con描画。`<Button-1>` イベントバインディングでホールド、`<ButtonRelease-1>` で解放、Shift+解放で `holdEndSkip`。
- **色**: L側シアン `#56CCF2`、R側赤 `#E9514E`、アクティブ状態黄色 `#FFD800`。
- **アナログスティックデッドゾーン**: 中央から±10%（0～255スケールで値103～153はニュートラルとして扱われる）。
- **ボタン**: A、B、X、Y、L、R、ZL、ZR、+、−、Home、Capture、D-pad（4方向）、Lスティック、Rスティック、タッチスクリーン（320×240）。

### 13.3 元の出力パネル

- **出力 #1 と 出力 #2**: スライダー（0～100）で比率調整可能なログ表示。
- **ソース**: WebSocket経由で受信したログ。
- **クリア**: その他タブの「出力をクリア」ボタン。

---

## 14. Python公開API仕様と開発環境設定

> **Version**: 2.1.0-draft  
> **Date**: 2026-05-27  
> **Scope**: Python互換レイヤー、PyO3バインディング、開発環境自動構築  
> **Source**: Grill-meセッション決定事項（refactor/rust-coreブランチ）

---

### 14.1 設計方針

- **コアはRust**: すべてのコア処理はRustで実装。Pythonは必要な部分のみ（ユーザースクリプトAPI、互換レイヤー）。
- **メタクラスによる切り替え**: `CommandMeta`が将来の実装切り替え用フックを提供。現状はすべてPyO3（Rustバインディング）に流れる。
- **後方互換性**: リファクタリング前のスクリプトは変更なしで動作する必要がある。
- **型ヒント**: 新APIは動作する型ヒントを持つ。旧APIは非推奨として保持される。

### 14.2 パッケージ構造

```
pokecon/
├── __init__.py          # パッケージエントリ、モジュールパッチ
├── commands.py          # PythonCommand、ImageProcPythonCommand、CommandEngine
├── keys.py              # Button、Hat、Direction、Stick、Touchscreen、SendFormat
├── events.py            # EventBus（動的設定用）
├── dialogue.py          # ダイアログ関数（ブロッキングWebポップアップ）
├── _meta.py             # CommandMetaメタクラス
├── _adapter.py          # Rustコアアダプタ
├── cli_args.py          # CLI引数解析
├── script_loader.py     # スクリプト検出と読み込み
└── scripts_dir.py       # XDG準拠スクリプトディレクトリユーティリティ
```

PyO3モジュール（rust/pokecon-pybindings）:
- `pokecon.keys` — 入力型（Button、Hat、Direction、Stick、Touchscreen）
- `pokecon.command` — コマンドスキャン/読み込み
- `pokecon.events` — イベントバス
- `pokecon.notify` — 通知（Discord、LINEスタブ、Windows）
- `pokecon.sender` — シリアル通信
- `pokecon.dialogue` — ダイアログ関数
- `pokecon.image_proc` — 画像処理（opencv-rust）
- `pokecon.net` — Socket、MQTT、HTTPクライアント

### 14.3 コマンドクラス

#### 14.3.1 クラス階層

```
Command (ABC, metaclass=CommandMeta)
├── PythonCommand (ABC)
│   └── ImageProcPythonCommand (ABC)
└── McuCommandBase
```

#### 14.3.2 PythonCommand

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
| `print_t1()` | `print_t1(*objects, sep=' ', end='\n')` | 上部ログパネルへ出力 |
| `print_t2()` | `print_t2(*objects, sep=' ', end='\n')` | 下部ログパネルへ出力 |
| `print_t()` | `print_t(*objects, sep=' ', end='\n')` | stdout以外のログパネルへ出力 |
| `print_s()` | `print_s(*objects, sep=' ', end='\n')` | stdout割り当てパネルへ出力 |
| `print_ts()` | `print_ts(*objects, sep=' ', end='\n')` | `print_s`と同じ |
| `print_t1b()` | `print_t1b(mode, *objects, sep=' ', end='\n')` | 上部ログ（モード付き w/a/d） |
| `print_t2b()` | `print_t2b(mode, *objects, sep=' ', end='\n')` | 下部ログ（モード付き） |
| `print_tb()` | `print_tb(mode, *objects, sep=' ', end='\n')` | stdout以外ログ（モード付き） |
| `print_tbs()` | `print_tbs(mode, *objects, sep=' ', end='\n')` | stdoutログ（モード付き） |

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

#### 14.3.3 ImageProcPythonCommand

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

#### 14.3.4 McuCommandBase

**Import**: `from Commands.McuCommandBase import McuCommandBase`

ファームウェアベースコマンド用。PythonCommandと同じメタクラス切り替え。

### 14.4 メタクラス設計（CommandMeta）

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

### 14.5 KeyPressとSender

#### 14.5.1 KeyPress

- ユーザースクリプトに**直接公開されない**
- `self.keys.neutral()`のみアクセス可能（コントローラーをニュートラル状態にリセット）
- 内部実装はRust、PyO3経由で公開

#### 14.5.2 Sender

**PyO3実装**（限定公開API）:
| メソッド | シグネチャ | 説明 |
|--------|-----------|-------------|
| `writeRow()` | `writeRow(row: str)` | シリアル行を書き込み |
| `ser.write()` | `ser.write(data)` | 直接シリアル書き込み（PyO3でpySerial互換型変換） |

その他のSenderメソッドはSenderクラスとして公開されず、適切な他クラスに統合。

### 14.6 新ダイアログAPI（型安全）

**非推奨**: `dialogue()`、`dialogue6widget()` — 互換性のために保持、非推奨マーク。

**新API**: `show_dialog()` with Widgetクラスと型ヒント。

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

# 使用例
entry = Widget("Entry", "名前", "デフォルト")  # Widget[str]
check = Widget("Check", "有効", True)  # Widget[bool]
combo = Widget("Combo", "選択肢", ["A", "B", "C"], "A")  # Widget[str]
spin = Widget("Spin", "数値", [1, 2, 3], 1)  # Widget[int]

show_dialog("タイトル", widgets=[entry, check, combo, spin])

print(entry.value)  # str
print(check.value)  # bool
print(combo.value)  # str
print(spin.value)  # int
```

### 14.7 イベントシステム（動的設定）

動的設定ファイル（PythonおよびLua）で使用するイベント駆動のフックシステム。

#### 14.7.1 設計方針

- **Neovim/Vimライクな設計**: `autocmd` スタイルのイベントハンドラ登録
- **Pre/Postフェーズ**: すべてのイベントは `Pre`（事前）と `Post`（事後）の2フェーズを持つ
- **フェーズはイベント名に含める**: `phase` 引数ではなく、イベント名自体に `Pre`/`Post` を含める（LSP警告のため）
- **require不要**: Lua設定では `require` なしで `pokecon.*` にアクセス可能
- **Python/Lua両対応**: 両言語で同じAPI構造を使用

#### 14.7.2 名前空間設計

| 名前空間 | 用途 | API |
|---------|------|-----|
| `pokecon.autocmd` | イベントハンドラの登録・解除 | `on()`, `once()`, `off()`, `off_all()`, `clear(group)` |
| `pokecon.event` | イベント定義・発火 | `define()`, `emit()`, `list_defined()`, `get_schema()` |

#### 14.7.3 イベントハンドラAPI

```python
# Python設定
import pokecon

# 基本的なイベント登録
pokecon.autocmd.on("CameraOpenPost", callback=lambda: print("Camera opened"))

# 一度だけ実行
pokecon.autocmd.once("SerialConnectPost", callback=lambda: print("Connected"))

# イベントハンドラ解除
pokecon.autocmd.off("CameraOpenPost", callback=handler_func)

# すべてのハンドラ解除
pokecon.autocmd.off_all("CameraOpenPost")

# グループ単位で解除
pokecon.autocmd.clear("my_group")
```

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
        print("Connected")
    end
})
```

#### 14.7.4 イベント定義・発火API

```python
# ユーザー定義イベント
pokecon.event.define("MyCustomEvent")

# イベント発火
pokecon.event.emit("MyCustomEvent", data={"key": "value"})

# 定義済みイベント一覧
print(pokecon.event.list_defined())

# イベントスキーマ取得
schema = pokecon.event.get_schema("CameraOpenPost")
```

```lua
-- Lua設定
pokecon.event.define("MyCustomEvent")
pokecon.event.emit("MyCustomEvent", {key = "value"})
print(pokecon.event.list_defined())
```

#### 14.7.5 組み込みイベント一覧

| イベント名 | フェーズ | 説明 |
|-----------|---------|------|
| `AppStartupPost` | Post | アプリケーション起動後 |
| `AppShutdownPre` | Pre | アプリケーション終了前 |
| `SerialConnectPost` | Post | シリアルポート接続後 |
| `SerialDisconnectPost` | Post | シリアルポート切断後 |
| `CameraOpenPost` | Post | カメラオープン後 |
| `CameraClosePost` | Post | カメラクローズ後 |
| `CommandStartPre` | Pre | コマンド実行開始前 |
| `CommandStartPost` | Post | コマンド実行開始後 |
| `CommandStopPost` | Post | コマンド停止後 |
| `CommandErrorPost` | Post | コマンドエラー発生後 |
| `ScriptLoadPost` | Post | スクリプト読み込み後 |
| `ConfigReloadPost` | Post | 設定再読み込み後 |
| `InputPressedPre` | Pre | 入力押下前 |
| `InputReleasedPost` | Post | 入力解放後 |

**命名規則**:
- **キャメルケース**: `CameraOpenPost`, `SerialConnectPost`
- **Pre/Post後置**: Vim/Neovim風（`BufReadPre`/`BufReadPost`に類似）
- **名前空間なし**: ドット区切りの名前空間は使用しない
- **動詞に限定しない**: 名詞・形容詞も可

#### 14.7.6 型ヒント

```python
from typing import Literal, Union

# 組み込みイベントの厳密な型定義
BuiltinEvent = Literal[
    "AppStartupPost", "AppShutdownPre",
    "SerialConnectPost", "SerialDisconnectPost",
    "CameraOpenPost", "CameraClosePost",
    "CommandStartPre", "CommandStartPost",
    "CommandStopPost", "CommandErrorPost",
    "ScriptLoadPost", "ConfigReloadPost",
    "InputPressedPre", "InputReleasedPost"
]

# 組み込みイベント + ユーザー定義イベント
EventName = Union[BuiltinEvent, str]
```

#### 14.7.7 コールバックシグネチャ

```python
# 引数なし（デフォルト）
pokecon.autocmd.on("CameraOpenPost", callback=lambda: print("Camera opened"))

# 引数あり（将来の拡張）
pokecon.autocmd.on("CameraOpenPost", callback=lambda event: print(event.data))
```

#### 14.7.8 状態取得API

```python
# 読み取り専用で状態を取得
print(pokecon.state.serial_port)      # 現在のシリアルポート
print(pokecon.state.camera_opened)    # カメラがオープンか
print(pokecon.state.active_profile)   # 現在のアクティブプロファイル
print(pokecon.state.is_running)       # コマンド実行中か
```

```lua
-- Lua設定
print(pokecon.state.serial_port)
print(pokecon.state.camera_opened)
print(pokecon.state.active_profile)
```

#### 14.7.9 エラーハンドリング

- イベントハンドラ内でエラーが発生しても、他のハンドラは継続して実行
- エラー内容はログに出力
- フォールバック機構により、システム全体の動作を停止しない

### 14.8 設定ファイルシステム

#### 14.8.1 設定ファイルの種類と対象ユーザー

| 種類 | ファイル | 言語 | 用途 | 対象ユーザー |
|------|---------|------|------|------------|
| **静的設定** | `settings.toml` | TOML | グローバル設定、プロファイル管理 | **全ユーザー** |
| **動的設定** | `init.py` | Python | イベントハンドラ、カスタムロジック | **パワーユーザー** |
| **動的設定** | `init.lua` | Lua | イベントハンドラ、カスタムロジック | **パワーユーザー** |

**重要**: TOMLは**動的ではない**。Python/Luaのみが動的設定ファイルとして使用される。

#### 14.8.2 設定の優先順位とマージ方式

設定は以下の5層で優先順位が決まる（**後勝ち**、未設定項目は上位から継承）：

1. **デフォルト値**（アプリケーション内蔵）
2. **グローバル設定**（`~/.config/pokecon/settings.toml`）
3. **プロファイル設定**（`~/.config/pokecon/profiles/<name>.toml`）
4. **起動時引数**（CLIオプション）
5. **動的設定**（`~/.config/pokecon/init.py` / `init.lua`）

```
優先順位: ①デフォルト → ②グローバル → ③プロファイル → ④CLI引数 → ⑤動的設定
         （低）                                    （高）
```

#### 14.8.3 静的設定（settings.toml）

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

[[profiles.list]]
name = "default"
description = "デフォルトプロファイル"
```

#### 14.8.4 動的設定ファイルの読み込みタイミング

| タイミング | 動作 |
|-----------|------|
| **アプリケーション起動時** | 自動読み込み（`init.py` / `init.lua`） |
| **プロファイル切替時** | 自動読み込み（新プロファイルの設定を反映） |
| **手動** | メニュー「Load Dynamic Config」で読み込み |
| **自動リロード** | ファイル変更検知時（デフォルト無効、オプトイン） |

#### 14.8.5 動的設定（Python）

```python
# ~/.config/pokecon/init.py
import pokecon

# カメラ設定（フラット構造）
pokecon.opt.camera_fps = 60
pokecon.opt.camera_resolution = "1280x720"

# シリアル設定（フラット構造）
pokecon.opt.serial_port = "COM3"
pokecon.opt.serial_baudrate = 115200

# キーマッピング（Neovim風記法）
pokecon.keymap.set("A", lambda: pokecon.input.press(pokecon.keys.Button.A))
pokecon.keymap.set("<C-a>", lambda: print("Ctrl+A pressed"), state="press")
pokecon.keymap.set("<S-a>", lambda: print("Shift+A pressed"), state="hold")

# イベントハンドラ
pokecon.autocmd.on("CameraOpenPost", callback=lambda: print("Camera opened"))

# 相互参照（他の設定ファイルを読み込み）
pokecon.source("~/.config/pokecon/extra_settings.py")

# プロファイル取得
current_profile = pokecon.profile.current()
print(f"Current profile: {current_profile}")

# 状態取得（読み取り専用）
print(pokecon.state.serial_port)
print(pokecon.state.camera_opened)
print(pokecon.state.active_profile)
```

#### 14.8.6 動的設定（Lua）

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

#### 14.8.7 エラーハンドリング

- 動的設定ファイル読み込み時にエラーが発生しても、アプリケーションは継続して動作
- エラー内容はログパネルに出力（行番号・ファイル名・エラー内容）
- フォールバック機構により、前回の有効な設定を維持

#### 14.8.8 設定ファイルの階層構造

```
~/.config/pokecon/                    # XDG_CONFIG_HOME（デフォルト）
├── settings.toml                     # 静的設定（グローバル）
├── profiles/                         # プロファイル管理
│   ├── default.toml
│   └── custom.toml
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

### 14.9 スクリプト互換性要件

| 要件 | 状態 |
|------|------|
| 17以上のサンプルスクリプトが変更なしで動作 | ✅ 必須 |
| `from Commands.PythonCommandBase import PythonCommand` | ✅ モジュールパッチで保持 |
| `from Commands.Keys import Button, Hat, ...` | ✅ モジュールパッチで保持 |
| `self.keys.neutral()` | ✅ 利用可能 |
| `self.keys.ser.writeRow()` | ✅ 利用可能 |
| `self.keys.ser.ser.write()` | ✅ 利用可能（Rustシリアルラッパー） |
| 画像処理API | ✅ Rust実装（opencv-rust） |
| Discord通知 | ✅ 実装済み |
| LINE通知 | ⚠️ No-opスタブ（サービスEOL） |
| Windows通知 | ✅ 実装済み |

### 14.10 開発環境自動構築

#### 14.10.1 ディレクトリ構造

```
~/.config/pokecon/                    # XDG_CONFIG_HOME（ユーザーが編集する）
├── pyproject.toml                    # Python LSP設定
├── .luarc.json                       # Lua LSP設定（lua-language-server & EmmyLua共用）
├── .vscode/settings.json             # Pylance用（オプション）
├── settings.toml                     # ユーザー設定
│   [python.packages]                 # ユーザー追加ライブラリ
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

**注記**: 
- **実行時生成**: `~/.local/share/pokecon/typings/` 配下の `.pyi` はアプリ起動時にRust側で自動生成
- **開発用元データ**: `python/pokecon/typings/` 配下の `.pyi` はリポジトリに含め、開発・メンテナンス用として使用
- **ユーザーが直接触らない**: XDG_DATA_HOME 配下は自動管理。ユーザーが編集するのは XDG_CONFIG_HOME 配下のみ

**未確定事項（grill-me継続中）**:
- 型定義ファイルの最終的な配布方式については、別途検討が必要
- 現状: XDG_DATA_HOMEへの自動生成方式で確定、開発用元データはリポジトリ内に配置

#### 14.10.2 設定ファイル生成タイミング

- **存在しない時に生成**（初回、アップデート、削除後等）
- **nix環境**: nix式で指定した場合のみnix側で生成。指定しなかった場合はアプリ起動時に存在しないためアプリ側で生成。
- **非nix環境**: アプリ側で自動生成

#### 14.10.3 Python管理（非nix環境）

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

#### 14.10.4 必須パッケージ管理

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

#### 14.10.5 ユーザーパッケージ設定

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

#### 14.10.6 LSP設定（pyproject.toml）

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

#### 14.10.7 Lua LSP設定（.luarc.json）

```json
{
    "$schema": "https://raw.githubusercontent.com/LuaLS/vscode-lua/master/setting/schema.json",
    "workspace.library": [
        "/home/username/.local/share/pokecon/lua-typings"
    ]
}
```

### 14.11 動的設定ファイルのUI

#### 14.11.1 メニュー配置

- **配置場所**: メニューバー内
- **項目**: 単一の「Load Dynamic Config」メニュー項目

```
File
├── Load Dynamic Config      ← 新規読み込み（拡張子で自動判別）
├── Reload Dynamic Config    ← 現在のファイルを再読み込み
└── Open Config Directory    ← 設定ディレクトリを開く
```

#### 14.11.2 ファイル選択と自動判別

- **ファイル選択ダイアログ**: 単一の「Load Dynamic Config」メニューから開く
- **自動判別**: 拡張子で言語を自動判別
  - `.py` → Python動的設定ファイル
  - `.lua` → Lua動的設定ファイル
- **手動指定**: 拡張子が不明な場合はユーザーに選択を促す

#### 14.11.3 リロード機能

| 機能 | 説明 |
|------|------|
| **手動リロード** | 「Reload Dynamic Config」メニューで現在のファイルを再読み込み |
| **自動リロード** | ファイルウォッチャーによる自動リロード（**デフォルトで無効**） |
| **有効化方法** | `pokecon.opt.auto_reload_config = True` またはUI設定 |

#### 14.11.4 エラーハンドリング

- 動的設定ファイル読み込み時にエラーが発生しても、アプリケーションは継続して動作
- エラー内容はログパネルに出力
- フォールバック機構により、前回の有効な設定を維持

---

*本仕様書は生きたドキュメントです。新しい要件がユーザーから伝達された場合、更新を行う必要があります。*
