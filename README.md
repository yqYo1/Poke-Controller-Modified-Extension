# Poke-Controller Modified Extension

Rustコア + Python互換層 + Web/Tauri UI へのリファクタリング版

## 概要

本プロジェクトは、[Poke-Controller Modified](https://github.com/Moi-poke/Poke-Controller-Modified)をベースに、**Rustコア**へ段階的に移行しつつ、既存のPythonユーザースクリプトとの**完全な後方互換性**を保持したゲーム機自動化支援ソフトウェアです。

### 主な特徴

| 特徴 | 説明 |
|------|------|
| **Rustコア** | シリアル通信、画像処理、イベント駆動アーキテクチャをRustで再実装 |
| **Python互換層** | 既存スクリプト（`Commands.PythonCommandBase`等）がそのまま動作 |
| **PyO3バインディング** | Rust機能をPythonから直接利用可能 |
| **Web/Tauri UI** | スマホ対応のレスポンシブWeb UI + ネイティブデスクトップアプリ |
| **PWA対応** | ホーム画面追加、オフライン対応のプログレッシブWebアプリ |
| **リアルタイム通信** | WebSocketによるカメラ映像・コマンド状態のリアルタイム配信 |

### 対応プラットフォーム

- **OS**: Windows 10/11, macOS, Linux
- **Python**: 3.14以上（PEP 695型パラメータ構文対応）
- **ゲーム機**: Nintendo Switch, 3DS, DS, GameCube
- **ブラウザ**: Chrome, Safari, Firefox（スマホ・タブレット対応）

### UI/UX 機能

| 機能 | 説明 |
|------|------|
| **ナビゲーション** | NavBarは6タブ（カメラ/シリアル/手動制御/コマンド/通知/その他）。keyconfigはダイレクトURLでアクセス可能 |
| **カメラマウス操作** | ドラッグ=スティック入力、Ctrl+クリック=カラーピッカー、Ctrl+Shift+ドラッグ=範囲スクリーンショット、Ctrl+Alt+ドラッグ=名前付き保存 |
| **コマンドショートカット** | F5=再読み込み、F6=開始、Shift+F6=一時停止、Escape=停止 |
| **シリアルRX統合** | WebSocket `serial_data` メッセージをシリアルモニタに表示 |
| **出力パネル** | クリップボードコピー対応、ログレベルフィルタ（ALL/INFO/WARN/ERROR） |
| **MCUタグ** | MCUコマンド一覧にタグフィルタとタグバッジを表示 |
| **録画機能** | ボタンからチェックボックスに変更、記録状態を視覚表示 |
| **通知設定** | Windows通知設定を専用API（`getWindowsNotificationSettings` 等）経由で管理 |

---

## クイックスタート

### エンドユーザー向け

```bash
# 1. リポジトリをクローン
git clone https://github.com/yqYo1/Poke-Controller-Modified-Extension.git
cd Poke-Controller-Modified-Extension

# 2. Nix開発環境に入る（推奨）
nix develop

# 3. アプリケーションを起動
nix run .              # Tauriデスクトップアプリ（デフォルト）
# または
nix run . -- --ui web  # ブラウザで開く
```

**非NixOS Linux（Ubuntu等）の場合:**
Tauri UIを起動するには[nixGL](https://github.com/nix-community/nixGL)が必要です：

```bash
# nixGLのインストール
nix-channel --add https://github.com/nix-community/nixGL/archive/main.tar.gz nixgl
nix-channel --update
nix-env -iA nixgl.auto.nixGLDefault

# nixGL経由で起動
nixGL nix run . -- --ui tauri
```

ブラウザで `http://localhost:8020` を開くと、スマホ対応のWeb UIが表示されます。

### スクリプト開発者向け

既存のPythonスクリプトは**変更なし**で動作します。新規作成時は以下を継承してください：

```python
from Commands.PythonCommandBase import PythonCommand
from Commands.Keys import Button

class MyCommand(PythonCommand):
    NAME = "My Command"
    def do(self):
        self.press(Button.A, duration=0.1, wait=0.2)
```

詳細は [docs/user/guide.md](docs/user/guide.md) を参照。

### 本体開発者向け

```bash
# テスト実行
nix run .#check        # すべてのチェック（pytest + ruff + clippy + treefmt）
nix run .#test         # Pythonテストのみ
nix run .#cargo-test   # Rustテストのみ

# PyO3バインディングビルド
maturin develop --manifest-path rust/pokecon-pybindings/Cargo.toml

# フォーマット
nix run .#fmt
```

詳細は [docs/developer/guide.md](docs/developer/guide.md) を参照。

---

## ドキュメント一覧

| ドキュメント | 対象読者 | 内容 |
|-------------|---------|------|
| [docs/user/guide.md](docs/user/guide.md) | エンドユーザー | インストール、使い方、Web UI操作、トラブルシューティング |
| [docs/script/guide.md](docs/script/guide.md) | スクリプト開発者 | PythonCommand API、画像認識、サンプル |
| [docs/developer/guide.md](docs/developer/guide.md) | 本体開発者 | アーキテクチャ、ビルド、Rust/Python連携 |
| [docs/user/api-reference.md](docs/user/api-reference.md) | スクリプト開発者 | 全APIリファレンス |
| [docs/user/web-ui.md](docs/user/web-ui.md) | エンドユーザー | Web UI詳細、スマホ操作、PWA |

---

## プロジェクト構造

```
Poke-Controller-Modified-Extension/
├── README.md                 # 本ファイル
├── flake.nix                 # Nix開発環境定義
├── pyproject.toml            # Pythonパッケージ設定
├── Cargo.toml                # Rustワークスペース定義
│
├── rust/                     # Rustコア実装（2クレート）
│   ├── pokecon-core/         # 統合コアライブラリ
│   │   ├── Cargo.toml
│   │   ├── build.rs
│   │   └── src/
│   │       ├── lib.rs            # エントリ、モジュール宣言、re-export
│   │       ├── command_manager.rs # コマンド管理
│   │       ├── profile.rs        # プロファイル
│   │       ├── settings.rs       # 設定
│   │       ├── serial/           # シリアル通信
│   │       │   ├── mod.rs
│   │       │   ├── format.rs     # シリアルフォーマット
│   │       │   ├── keypress.rs   # キー入力
│   │       │   ├── keys.rs       # ボタン/スティック定義
│   │       │   └── sender.rs     # 送信
│   │       ├── cv/               # カメラ・画像処理（OpenCV）
│   │       │   ├── mod.rs
│   │       │   ├── camera.rs
│   │       │   ├── backends.rs
│   │       │   └── image_processing.rs
│   │       ├── events/           # イベントバス
│   │       │   ├── mod.rs
│   │       │   ├── bus.rs
│   │       │   ├── handler.rs
│   │       │   ├── registry.rs
│   │       │   └── user_event.rs
│   │       ├── notify/           # 通知（feature "notify"）
│   │       │   ├── mod.rs
│   │       │   ├── discord.rs
│   │       │   ├── line.rs
│   │       │   └── windows.rs
│   │       ├── net/              # ネットワーク（feature "mqtt"）
│   │       │   ├── mod.rs
│   │       │   ├── mqtt.rs
│   │       │   └── socket.rs
│   │       └── lua/              # LuaJIT統合（feature "lua"）
│   │           ├── mod.rs
│   │           ├── api.rs
│   │           └── runtime.rs
│   └── pokecon-pybindings/   # PyO3 Pythonバインディング
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs           # エントリ、モジュール宣言
│           ├── keys.rs          # Python側キー型公開
│           ├── python_cmd.rs    # PythonCommand関連
│           ├── image_proc.rs    # 画像処理関数
│           └── events.rs        # イベントリスナー
│
├── python/pokecon/           # Python互換層
│   ├── __init__.py           # インポートハック（後方互換性）
│   ├── commands.py           # PythonCommand/ImageProcPythonCommand
│   ├── keys.py               # Button/Hat/Stick/Direction
│   ├── _meta.py              # CommandMeta（メタクラス）
│   ├── _adapter.py           # Rustコアアダプター
│   └── script_loader.py      # スクリプト動的ロード
│
├── src-server/                # HTTP/WebSocket/WebRTC server (renamed from src-tauri)
│   ├── Cargo.toml            # Server dependencies
│   ├── build.rs
│   └── src/
│       ├── main.rs           # HTTP server + WebSocket + API endpoints
│       ├── webrtc.rs         # WebRTC live preview
│       └── vaapi_encoder.rs  # VAAPI hardware encoding
│
├── web/                      # Webフロントエンド（SvelteKit + Svelte 5）
│   ├── package.json          # SvelteKit依存関係・スクリプト
│   ├── svelte.config.js      # SvelteKit設定（adapter-static, /ui base）
│   ├── vite.config.ts        # Vite設定（HMR proxy → :8020）
│   ├── src/
│   │   ├── app.html          # HTMLエントリ（PWAメタタグ付き）
│   │   ├── app.css           # グローバルスタイル
│   │   ├── app.d.ts          # アプリ型定義
│   │   ├── routes/           # SvelteKitファイルベースルーティング
│   │   │   ├── +layout.svelte    # ルートレイアウト（MenuBar/NavBar/StatusBar）
│   │   │   ├── +layout.ts        # レイアウトローダー
│   │   │   ├── +page.svelte      # トップページ
│   │   │   ├── camera/+page.svelte         # カメラ設定・プレビュー
│   │   │   ├── serial/+page.svelte         # シリアル通信
│   │   │   ├── manual/+page.svelte         # 手動制御（ゲームパッド）
│   │   │   ├── commands/
│   │   │   │   ├── +page.svelte
│   │   │   │   ├── CommandActions.svelte
│   │   │   │   ├── McuCommandList.svelte
│   │   │   │   ├── PythonCommandList.svelte
│   │   │   │   └── ShortcutButtons.svelte
│   │   │   ├── keyconfig/+page.svelte      # キーコンフィグ（NavBar非表示・直接URLのみ）
│   │   │   ├── notification/+page.svelte   # 通知設定
│   │   │   └── others/+page.svelte         # プロファイル・テーマ設定
│   │   ├── lib/
│   │   │   ├── api/
│   │   │   │   ├── client.ts      # 型付きREST/WebSocketクライアント
│   │   │   │   ├── types.ts       # OpenAPI自動生成型定義
│   │   │   │   ├── websocket.ts   # 型付きWebSocket（自動再接続）
│   │   │   │   ├── webrtc-video.ts # WebRTC映像ストリーム管理
│   │   │   │   └── datachannel.ts # WebRTC DataChannel（WSフォールバック付き）
│   │   │   ├── assets/
│   │   │   │   └── favicon.svg
│   │   │   ├── components/
│   │   │   │   ├── CameraPreview.svelte          # カメラライブプレビュー
│   │   │   │   ├── CameraSettings.svelte         # カメラ設定UI
│   │   │   │   ├── CaptureRegion.svelte          # キャプチャ範囲選択
│   │   │   │   ├── ControllerSimulator.svelte    # コントローラシミュレータ
│   │   │   │   ├── DiscordNotification.svelte    # Discord通知設定
│   │   │   │   ├── DisplaySettings.svelte        # 表示設定
│   │   │   │   ├── HardwareControl.svelte        # ハードウェア制御
│   │   │   │   ├── LogPanel.svelte               # ログパネル
│   │   │   │   ├── MainToolbar.svelte            # メインツールバー
│   │   │   │   ├── MenuBar.svelte                # メニューバー
│   │   │   │   ├── NavBar.svelte                 # タブナビゲーション（6タブ）
│   │   │   │   ├── OutputPanel.svelte            # 出力パネル（ログレベルフィルタ）
│   │   │   │   ├── RightPanel.svelte             # 右パネル（クリップボードコピー）
│   │   │   │   ├── SerialMonitor.svelte          # シリアルモニタ（serial_data受信）
│   │   │   │   ├── SoftwareController.svelte     # 仮想ゲームパッド
│   │   │   │   ├── SoftwareControl.svelte        # ソフトウェア制御（キーボード+マウス）
│   │   │   │   ├── StatusBar.svelte              # 接続状態表示
│   │   │   │   ├── ThemeProvider.svelte           # テーマプロバイダ
│   │   │   │   ├── TkinterNotebook.svelte        # タブパネル（ノートブック）
│   │   │   │   └── WindowsNotification.svelte    # Windows通知設定（専用API）
│   │   │   └── theme.ts       # テーマ管理（ライト/ダーク/カスタム）
│   │   ├── service-worker.ts  # PWA Service Worker（オフライン対応）
│   │   ├── api/client.js      # 旧RESTクライアント（後方互換性）
│   │   └── usage_analysis_result.md  # 使用状況分析レポート
│
├── SerialController/         # 既存Pythonコード（後方互換性）
│   ├── Commands/
│   │   ├── PythonCommandBase.py
│   │   ├── CommandBase.py
│   │   ├── Keys.py
│   │   └── PythonCommands/   # ユーザースクリプト配置先
│   ├── Window.py             # tkinter GUI（従来版）
│   └── ...
│
└── tests/                    # テスト
    ├── conftest.py           # モック設定
    ├── test_script_compatibility.py  # 互換性テスト（58ケース）
    └── benchmarks/           # ベンチマークテスト
        ├── run_all_benchmarks.py
        ├── test_benchmark_image_processing.py
        ├── test_benchmark_serial_latency.py
        └── test_benchmark_template_matching.py

```

---

## 技術スタック

### バックエンド

| レイヤー | 技術 |
|---------|------|
| コア言語 | Rust 2024 Edition |
| シリアル通信 | tokio-serial |
| 画像処理 | opencv-rust, ndarray |
| 非同期ランタイム | tokio |
| HTTPサーバー | axum |
| WebSocket | axum::ws |
| Python連携 | PyO3 + maturin |

### フロントエンド

| レイヤー | 技術 |
|---------|------|
| フレームワーク | SvelteKit 2 + Svelte 5 (runes) |
| ビルドツール | Vite 6 |
| スタイリング | Tailwind CSS 3 |
| UI基盤 | CSS Custom Properties（テーマ対応） |
| PWA | Service Worker, Web App Manifest |
| 通信 | Fetch API + WebSocket + WebRTC DataChannel |
| API型定義 | openapi-typescript（OpenAPI自動生成） |

### 開発環境

| ツール | 用途 |
|-------|------|
| Nix | 再現性のある開発環境 |
| treefmt | 一括フォーマット（nixfmt, rustfmt, ruff） |
| ruff | Pythonリント・フォーマット |
| clippy | Rustリント |
| basedpyright | Python型チェック |
| pytest | Pythonテスト |
| cargo | Rustビルド・テスト |

---

## ライセンス

MIT License

## 謝辞

[Poke-Controller](https://github.com/KawaSwitch/Poke-Controller)の開発者である[KawaSwitch](https://github.com/KawaSwitch)氏、[Poke-Controller Modified](https://github.com/Moi-poke/Poke-Controller-Modified)の開発者である[moi_poke](https://github.com/Moi-poke)氏に感謝申し上げます。
