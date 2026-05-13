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
├── rust/                     # Rustコア実装
│   ├── pokecon-core/         # コマンドマネージャー、設定、プロファイル
│   ├── pokecon-serial/       # シリアル通信、キー入力フォーマット
│   ├── pokecon-cv/           # カメラ、画像処理（OpenCV連携）
│   ├── pokecon-events/       # イベントバス、ハンドラレジストリ
│   ├── pokecon-net/          # Socket/MQTT通信
│   ├── pokecon-notify/       # Discord/LINE/Windows通知
│   ├── pokecon-lua/          # LuaJIT統合
│   └── pokecon-pybindings/   # PyO3 Pythonバインディング
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
│   ├── src/main.rs           # HTTPサーバー + WebSocket + APIエンドポイント
│   └── Cargo.toml            # Tauri専用依存関係
│
├── web/                      # Webフロントエンド（SvelteKit + Svelte 5）
│   ├── package.json          # SvelteKit依存関係・スクリプト
│   ├── svelte.config.js      # SvelteKit設定（adapter-static, /ui base）
│   ├── vite.config.ts        # Vite設定（HMR proxy → :8020）
│   ├── src/
│   │   ├── app.html          # HTMLエントリ（PWAメタタグ付き）
│   │   ├── routes/           # SvelteKitファイルベースルーティング
│   │   │   ├── +layout.svelte    # ルートレイアウト（MenuBar/NavBar/StatusBar）
│   │   │   ├── camera/          # カメラ設定・プレビュー
│   │   │   ├── serial/          # シリアル通信
│   │   │   ├── manual/          # 手動制御（ゲームパッド）
│   │   │   ├── commands/        # スクリプト一覧・実行
│   │   │   ├── keyconfig/       # キーコンフィグ
│   │   │   ├── notification/    # 通知設定
│   │   │   ├── pokemonhome/     # Pokémon Home連携
│   │   │   └── others/          # プロファイル・テーマ設定
│   │   ├── lib/
│   │   │   ├── api/
│   │   │   │   ├── client.ts      # 型付きREST/WebSocketクライアント
│   │   │   │   ├── types.ts       # OpenAPI自動生成型定義
│   │   │   │   ├── websocket.ts   # 型付きWebSocket（自動再接続）
│   │   │   │   └── datachannel.ts # WebRTC DataChannel（WSフォールバック付き）
│   │   │   ├── components/
│   │   │   │   ├── MenuBar.svelte          # メニューバー
│   │   │   │   ├── NavBar.svelte           # タブナビゲーション
│   │   │   │   ├── StatusBar.svelte        # 接続状態表示
│   │   │   │   ├── SoftwareController.svelte # 仮想ゲームパッド
│   │   │   │   ├── LogPanel.svelte         # ログパネル
│   │   │   │   └── CaptureRegion.svelte    # キャプチャ範囲選択
│   │   │   └── theme.ts       # テーマ管理（ライト/ダーク/カスタム）
│   │   └── service-worker.ts  # PWA Service Worker（オフライン対応）
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
    └── test_script_compatibility.py  # 互換性テスト（58ケース）
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
