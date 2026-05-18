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

詳細は [Docs/user-guide.md](Docs/user-guide.md) を参照。

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

詳細は [Docs/developer-guide.md](Docs/developer-guide.md) を参照。

---

## ドキュメント一覧

| ドキュメント | 対象読者 | 内容 |
|-------------|---------|------|
| [Docs/user-guide.md](Docs/user-guide.md) | エンドユーザー | インストール、使い方、Web UI操作、トラブルシューティング |
| [Docs/script-guide.md](Docs/script-guide.md) | スクリプト開発者 | PythonCommand API、画像認識、サンプル |
| [Docs/developer-guide.md](Docs/developer-guide.md) | 本体開発者 | アーキテクチャ、ビルド、Rust/Python連携 |
| [Docs/api-reference.md](Docs/api-reference.md) | スクリプト開発者 | 全APIリファレンス |
| [Docs/web-ui-guide.md](Docs/web-ui-guide.md) | エンドユーザー | Web UI詳細、スマホ操作、PWA |

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
├── src-tauri/                # Tauri v2 デスクトップアプリ
│   ├── Cargo.toml            # Tauri専用依存関係
│   ├── build.rs
│   └── src/
│       ├── main.rs           # HTTPサーバー + WebSocket + APIエンドポイント
│       ├── webrtc.rs         # WebRTCライブプレビュー
│       └── vaapi_encoder.rs  # VAAPIハードウェアエンコード
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
│   │   │   ├── keyconfig/+page.svelte      # キーコンフィグ
│   │   │   ├── notification/+page.svelte   # 通知設定
│   │   │   ├── pokemonhome/+page.svelte    # Pokémon Home連携
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
│   │   │   │   ├── DialogueButtonPosition.svelte # ダイアログボタン位置設定
│   │   │   │   ├── DiscordNotification.svelte    # Discord通知設定
│   │   │   │   ├── DisplaySettings.svelte        # 表示設定
│   │   │   │   ├── HardwareControl.svelte        # ハードウェア制御
│   │   │   │   ├── LogPanel.svelte               # ログパネル
│   │   │   │   ├── MainToolbar.svelte            # メインツールバー
│   │   │   │   ├── MenuBar.svelte                # メニューバー
│   │   │   │   ├── NavBar.svelte                 # タブナビゲーション
│   │   │   │   ├── OutputPanel.svelte            # 出力パネル
│   │   │   │   ├── OutputSizeAdjuster.svelte     # 出力サイズ調整
│   │   │   │   ├── RightPanel.svelte             # 右パネル
│   │   │   │   ├── SerialMonitor.svelte          # シリアルモニタ
│   │   │   │   ├── SoftwareControllerPosition.svelte # ソフトコン位置設定
│   │   │   │   ├── SoftwareController.svelte     # 仮想ゲームパッド
│   │   │   │   ├── SoftwareControl.svelte        # ソフトウェア制御
│   │   │   │   ├── StatusBar.svelte              # 接続状態表示
│   │   │   │   ├── StdoutDestination.svelte      # 標準出力先設定
│   │   │   │   ├── ThemeProvider.svelte           # テーマプロバイダ
│   │   │   │   ├── TkinterNotebook.svelte        # タブパネル（ノートブック）
│   │   │   │   ├── WidgetModeSelector.svelte     # ウィジェットモード選択
│   │   │   │   └── WindowsNotification.svelte    # Windows通知設定
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
