# Poke-Controller Modified Extension — 開発者ガイド

> **対象ブランチ**: `refactor/rust-core`
> **最終更新日**: 2026-05-24
> **対象読者**: 本プロジェクトにコントリビュートする開発者

---

## 目次

1. [アーキテクチャ概要](#1-アーキテクチャ概要)
2. [プロジェクト構造](#2-プロジェクト構造)
3. [開発環境のセットアップ](#3-開発環境のセットアップ)
4. [ビルドコマンド](#4-ビルドコマンド)
5. [Rust / Python 連携](#5-rust--python-連携)
6. [Web フロントエンド](#6-web-フロントエンド)
7. [テスト](#7-テスト)
8. [CI/CD ワークフロー](#8-cicd-ワークフロー)
9. [新機能の追加](#9-新機能の追加)
10. [コードスタイルと規約](#10-コードスタイルと規約)

---

## 1. アーキテクチャ概要

Poke-Controller Modified Extension は、Nintendo Switch をシリアル通信（UART）経由で制御するクロスプラットフォームデスクトップアプリケーションです。`refactor/rust-core` ブランチでは、従来の Python/PyQt5 アーキテクチャから **3 層アーキテクチャ** への全面刷新を行っています。

### 1.1 全体アーキテクチャ

```
┌─────────────────────────────────────────────────────────┐
│                    Web UI (SvelteKit 5)                  │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌───────────┐  │
│  │ Camera   │ │ Commands │ │ Output   │ │ Settings  │  │
│  │ Panel    │ │ Panel    │ │ Panel    │ │ Panel     │  │
│  └────┬─────┘ └────┬─────┘ └────┬─────┘ └─────┬─────┘  │
│       │            │            │              │         │
│  ┌────▼────────────▼────────────▼──────────────▼─────┐  │
│  │        WebSocket Client + Tauri IPC Bridge         │  │
│  └───────────────────────┬───────────────────────────┘  │
└──────────────────────────┼────────────────────────────┘
                           │
┌──────────────────────────┼────────────────────────────┐
│           Tauri IPC (invoke / events)                  │
│  ┌───────────────────────▼───────────────────────────┐ │
│  │              Rust Core (Tauri 2.x)                 │ │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────────────┐   │ │
│  │  │ Serial   │ │ Camera   │ │ WebSocket Server  │   │ │
│  │  │ I/O      │ │ Capture  │ │ (tokio-tungstenite)│   │ │
│  │  └──────────┘ └──────────┘ └──────────────────┘   │ │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────────────┐   │ │
│  │  │ Command  │ │Notifica- │ │ Python Runtime   │   │ │
│  │  │ Runner   │ │tion APIs │ │ (PyO3 / CPython)  │   │ │
│  │  └──────────┘ └──────────┘ └──────────────────┘   │ │
│  └───────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

### 1.2 3 層構造

| 層 | 役割 | 技術 |
|---|------|------|
| **Rust Core** | シリアル通信、カメラキャプチャ、WebSocket サーバー、コマンド実行、通知API | Rust + Tauri 2.x |
| **Python Compat Layer** | 既存の Python 自動化スクリプトとの互換性 | PyO3 + maturin + CPython |
| **Web UI** | ユーザーインターフェース全体 | SvelteKit 5 + Tailwind CSS |

### 1.3 技術スタック詳細

| カテゴリ | 技術 | 用途 |
|---------|------|------|
| バックエンドフレームワーク | Tauri 2.x | デスクトップアプリケーションシェル、IPC |
| シリアル通信 | `serialport` crate | UART 通信（Pico/RP2040） |
| カメラ | OpenCV（Tauri コマンド経由） | キャプチャ、画像処理 |
| WebSocket | `tokio-tungstenite` | シリアルデータのリアルタイム配信 |
| Rust/Python 連携 | PyO3 + maturin | Python スクリプト実行エンジン |
| フロントエンド | SvelteKit 5 + TypeScript | シングルページアプリケーション |
| UI フレームワーク | Tailwind CSS + shadcn-svelte | スタイリング、コンポーネント |
| リアルタイム通信 | WebSocket + WebRTC (DataChannel) | カメラストリーム、低遅延制御 |
| パッケージ管理 | Nix flake | 開発環境、依存関係 |
| ビルドツール | Cargo + Vite + maturin | 各層のビルド |

---

## 2. プロジェクト構造

```
Poke-Controller-Modified-Extension/
├── Cargo.toml                  # Rust ワークスペース定義
├── flake.nix                   # Nix flake 設定（開発環境・ビルド）
├── flake.lock                  # Nix flake ロックファイル
├── pyproject.toml              # Python パッケージ設定（maturin）
├── README.md                   # プロジェクト概要
├── AGENTS.md                   # AI エージェント向け指示書
├── SPECIFICATION.md            # 機能仕様書
│
├── src-tauri/                  # Rust / Tauri バックエンド
│   ├── Cargo.toml
│   ├── tauri.conf.json         # Tauri 設定
│   ├── capabilities/           # Tauri 権限設定
│   ├── icons/                  # アプリアイコン
│   └── src/
│       ├── main.rs             # Tauri エントリポイント
│       ├── lib.rs              # ライブラリルート
│       ├── serial/             # シリアル通信モジュール
│       │   ├── mod.rs
│       │   ├── sender.rs       # TX (送信)
│       │   └── receiver.rs     # RX (受信)
│       ├── camera/             # カメラキャプチャモジュール
│       │   ├── mod.rs
│       │   └── capture.rs      # OpenCV ラッパー
│       ├── commands/           # コマンド実行エンジン
│       │   ├── mod.rs
│       │   └── runner.rs       # Python コマンドランナー
│       ├── ws/                 # WebSocket サーバー
│       │   ├── mod.rs
│       │   └── server.rs       # tokio-tungstenite サーバー
│       ├── notification/       # 通知 API
│       │   ├── mod.rs
│       │   └── endpoints.rs    # Discord / Windows 通知
│       └── python/             # PyO3 連携
│           ├── mod.rs
│           └── runtime.rs      # CPython 埋め込みランタイム
│
├── python/                     # Python Compat Layer
│   ├── Cargo.toml              # PyO3 バインディング用 Cargo.toml
│   ├── src/
│   │   └── lib.rs              # maturin エントリポイント
│   ├── poke_controller/        # Python パッケージ
│   │   ├── __init__.py
│   │   ├── commands/           # 自動化コマンド定義
│   │   │   ├── __init__.py
│   │   │   └── base.py         # 基底コマンドクラス
│   │   └── utils/              # ユーティリティ
│   └── tests/                  # Python テスト
│
├── web/                        # SvelteKit 5 フロントエンド
│   ├── package.json
│   ├── svelte.config.js
│   ├── vite.config.ts
│   ├── tailwind.config.ts
│   ├── postcss.config.js
│   ├── tsconfig.json
│   └── src/
│       ├── app.html            # HTML テンプレート
│       ├── app.css             # グローバルスタイル
│       ├── routes/             # SvelteKit ルート
│       │   ├── +layout.svelte  # レイアウト（NavBar + Sidebar）
│       │   ├── +page.svelte    # ホーム画面
│       │   ├── camera/
│       │   │   ├── +page.svelte
│       │   │   └── CameraSettings.svelte
│       │   ├── commands/
│       │   │   └── +page.svelte
│       │   ├── output/
│       │   │   └── +page.svelte
│       │   ├── settings/
│       │   │   └── +page.svelte
│       │   └── about/
│       │       └── +page.svelte
│       ├── lib/
│       │   ├── components/     # 共通コンポーネント
│       │   │   ├── NavBar.svelte
│       │   │   ├── Sidebar.svelte
│       │   │   ├── RightPanel.svelte
│       │   │   ├── LogPanel.svelte
│       │   │   └── ...
│       │   ├── services/       # サービス層
│       │   │   ├── wsClient.ts       # WebSocket クライアント
│       │   │   ├── webrtcClient.ts   # WebRTC クライアント
│       │   │   └── tauriBridge.ts    # Tauri IPC ラッパー
│       │   └── stores/         # Svelte ストア（状態管理）
│       │       ├── serial.ts
│       │       ├── camera.ts
│       │       └── commands.ts
│       └── tests/              # フロントエンドテスト
│
├── docs/                       # ドキュメント
│   ├── user/
│   │   └── guide.md            # エンドユーザーガイド
│   └── developer/
│       └── guide.md            # 開発者ガイド（このファイル）
│
└── .github/
    └── workflows/
        └── ci.yml              # GitHub Actions CI
```

---

## 3. 開発環境のセットアップ

### 3.1 Nix を使用する場合（推奨）

プロジェクトは Nix flake に対応しています。Nix がインストールされている環境では、以下のコマンドで開発環境を構築できます。

```bash
# リポジトリをクローン
git clone <repository-url>
cd Poke-Controller-Modified-Extension

# 開発シェルに入る（依存関係を自動解決）
nix develop

# 内部で利用可能になるツール
#   - Rust (cargo, rustc, clippy, rustfmt)
#   - Node.js + npm/pnpm
#   - Python 3 + pip
#   - OpenCV
#   - 各種システムライブラリ
```

`nix develop` は以下の依存関係を自動的に解決します。

| 依存関係 | バージョン | 用途 |
|---------|-----------|------|
| Rust | stable（2024+） | バックエンド開発 |
| Node.js | 20.x LTS | フロントエンド開発 |
| Python | 3.11+ | Python 互換レイヤー |
| OpenCV | 4.x | カメラキャプチャ |
| pkg-config | latest | システムライブラリ検出 |
| CMake | latest | OpenCV ビルド |

### 3.2 手動セットアップ

Nix を使用しない場合、以下の依存関係を手動でインストールしてください。

```bash
# --- Rust ---
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default stable
rustup component add clippy rustfmt

# --- Node.js ---
# fnm または nvm を使用（推奨）
fnm install 20
fnm use 20

# --- Python ---
# system Python 3.11+ または pyenv を使用

# --- システムライブラリ（Ubuntu/Debian） ---
sudo apt-get install -y \
  libopencv-dev \
  libssl-dev \
  libwebkit2gtk-4.1-dev \
  libgtk-3-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  libpython3-dev \
  pkg-config \
  cmake

# --- macOS ---
brew install opencv cmake pkg-config

# --- フロントエンド依存関係 ---
cd web
npm install
```

### 3.3 開発サーバーの起動

```bash
# 開発モードで起動（ホットリロード対応）
nix run .#dev

# または手動で
cd web
npx tauri dev
```

---

## 4. ビルドコマンド

### 4.1 Nix Flake コマンド

| コマンド | 説明 |
|---------|------|
| `nix develop` | 開発環境に入る |
| `nix build` | アプリケーションをビルド |
| `nix run` | アプリケーションを実行 |
| `nix run .#build` | リリースビルド |
| `nix run .#check` | 全チェック（lint + test + build） |
| `nix run .#lint` | リンター実行 |
| `nix run .#fmt` | コードフォーマット |
| `nix run .#dev` | 開発サーバー起動 |
| `nix run .#test` | 全テスト実行 |
| `nix run .#test-rust` | Rust テストのみ実行 |
| `nix run .#test-py` | Python テストのみ実行 |
| `nix run .#test-web` | Web テストのみ実行 |
| `nix run .#clean` | ビルドキャッシュのクリーン |

### 4.2 個別ビルド

```bash
# Rust Core のみビルド
cd src-tauri
cargo build          # デバッグビルド
cargo build --release # リリースビルド

# Python Compat Layer のビルド
cd python
maturin develop     # 開発用インストール（pip install -e . 相当）
maturin build       # Wheel ビルド

# Web UI のみビルド
cd web
npm run dev         # 開発サーバー
npm run build       # プロダクションビルド
npm run preview     # ビルド結果のプレビュー

# Tauri アプリケーション全体をビルド
npx tauri build
```

### 4.3 リンターとフォーマット

```bash
# Rust
cargo clippy -- -D warnings
cargo fmt --check

# Python
ruff check python/
ruff format --check python/
mypy python/

# Web
cd web
npm run lint
npm run format:check

# 一括実行（Nix）
nix run .#check
```

---

## 5. Rust / Python 連携

### 5.1 アーキテクチャ

Rust Core と Python Compat Layer の連携は **PyO3** を介して行われます。Python の自動化スクリプトは、Rust プロセス内に埋め込まれた CPython インタプリタで実行されます。

```
┌──────────────────────────────────────┐
│          Rust Core (Tauri)            │
│  ┌────────────────────────────────┐  │
│  │   Command Runner (commands/)    │  │
│  │   ┌────────────────────────┐  │  │
│  │   │ Python Runtime (PyO3)   │  │  │
│  │   │ ┌────────────────────┐ │  │  │
│  │   │ │ CPython Interpreter │ │  │  │
│  │   │ └────────────────────┘ │  │  │
│  │   │ ┌────────────────────┐ │  │  │
│  │   │ │ User Scripts (.py) │ │  │  │
│  │   │ └────────────────────┘ │  │  │
│  │   └────────────────────────┘  │  │
│  └────────────────────────────────┘  │
└──────────────────────────────────────┘
```

### 5.2 PyO3 バインディング

Python コマンドは Rust 側で以下のように呼び出されます。

```rust
// src-tauri/src/python/runtime.rs
use pyo3::prelude::*;

pub fn run_python_command(script_path: &str, params: &[String]) -> Result<String, String> {
    Python::with_gil(|py| {
        let syspath = py.import("sys")?.getattr("path")?;
        syspath.call_method1("insert", (0, "/path/to/commands"))?;

        let module = py.import("commands.my_command")?;
        let result = module.call_method1("run", (params,))?;
        Ok(result.extract::<String>()?)
    })
    .map_err(|e: PyErr| format!("Python error: {}", e))
}
```

### 5.3 maturin によるパッケージング

Python 側のコードは **maturin** を使って Rust との統合パッケージとしてビルドします。

```toml
# python/Cargo.toml
[package]
name = "poke-controller-python"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
pyo3 = { version = "0.21", features = ["extension-module"] }

[package.metadata.maturin]
name = "poke_controller._core"
```

```python
# pyproject.toml（ルート）
[build-system]
requires = ["maturin>=1.5,<2.0"]
build-backend = "maturin"

[tool.maturin]
features = ["pyo3/extension-module"]
python-source = "python"
module-name = "poke_controller._core"
```

### 5.4 コマンド実行フロー

1. ユーザーが Web UI でコマンドを選択し「実行」
2. Tauri IPC が Rust Core の `commands::runner` にリクエスト
3. Runner が PyO3 経由で CPython インタプリタを呼び出し
4. Python スクリプトがシリアル通信・カメラ・WebSocket などの Rust API を利用
5. 実行結果（成功/失敗、ログ）が Web UI に返却

```rust
// コマンド実行の擬似コード
#[tauri::command]
fn execute_command(app: AppHandle, command_name: String) -> Result<(), String> {
    python::runtime::run_python_command(&command_name)?;
    let window = app.get_webview_window("main").unwrap();
    window.emit("command-complete", command_name)?;
    Ok(())
}
```

### 5.5 Python スクリプトからの Rust API 呼び出し

```python
# python/poke_controller/commands/my_command.py
from poke_controller._core import (
    serial_send,
    camera_capture,
    log_info,
    log_error,
)

class MyCommand:
    def run(self):
        log_info("コマンドを開始します")
        result = camera_capture()
        serial_send("0xABCD 08 80 80 80 80")
        log_info(f"キャプチャ完了: {result}")
```

---

## 6. Web フロントエンド

### 6.1 技術スタック

| 技術 | バージョン | 用途 |
|------|-----------|------|
| SvelteKit | 5.x | メタフレームワーク（ルーティング、SSR/CSR） |
| Svelte | 5.x | UI コンポーネント（runes 記法） |
| TypeScript | 5.x | 型安全な開発 |
| Tailwind CSS | 4.x | ユーティリティファースト CSS |
| shadcn-svelte | latest | アクセシブルな UI コンポーネント |
| Vite | 6.x | ビルドツール、HMR |
| Vitest | 2.x | ユニットテスト |
| Playwright | latest | E2E テスト |

### 6.2 SvelteKit 5 (runes)

SvelteKit 5 では、新しい **runes** 記法を使用します。

```svelte
<script lang="ts">
  // $state: リアクティブな状態
  let count = $state(0);

  // $derived: 派生状態
  let doubled = $derived(count * 2);

  // $effect: 副作用
  $effect(() => {
    console.log('count changed:', count);
  });

  // $props: コンポーネントプロパティ
  let { title = "Default" } = $props();
</script>

<button onclick={() => count++}>
  {title}: {count} (doubled: {doubled})
</button>
```

### 6.3 WebSocket 通信

```typescript
// web/src/lib/services/wsClient.ts
import { writable } from 'svelte/store';

export type WsMessage = {
  type: 'serial_data' | 'log' | 'command_status' | 'camera_frame';
  payload: unknown;
};

class WsClient {
  private ws: WebSocket | null = null;
  public messages = writable<WsMessage[]>([]);

  connect(url: string = 'ws://localhost:9876') {
    this.ws = new WebSocket(url);
    this.ws.onmessage = (event) => {
      const msg: WsMessage = JSON.parse(event.data);
      this.messages.update((m) => [...m, msg]);
    };
  }

  send(data: unknown) {
    this.ws?.send(JSON.stringify(data));
  }

  disconnect() {
    this.ws?.close();
    this.ws = null;
  }
}

export const wsClient = new WsClient();
```

### 6.4 WebRTC（カメラストリーム）

低遅延のカメラストリーミングには WebRTC を使用します。

```typescript
// web/src/lib/services/webrtcClient.ts
export class WebRTCClient {
  private pc: RTCPeerConnection;
  private channel: RTCDataChannel | null = null;

  constructor() {
    this.pc = new RTCPeerConnection({
      iceServers: [{ urls: 'stun:stun.l.google.com:19302' }],
    });
  }

  async startStream(): Promise<MediaStream> {
    const stream = new MediaStream();
    this.pc.ontrack = (event) => {
      stream.addTrack(event.track);
    };
    // Tauri コマンド経由で SDP オファーを取得
    const offer = await invoke('start_camera_webrtc');
    await this.pc.setRemoteDescription(offer);
    const answer = await this.pc.createAnswer();
    await this.pc.setLocalDescription(answer);
    await invoke('set_camera_webrtc_answer', { sdp: answer });
    return stream;
  }
}
```

### 6.5 Tauri IPC 連携

```typescript
// web/src/lib/services/tauriBridge.ts
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

// コマンド呼び出し
export async function serialConnect(port: string, baudRate: number) {
  return invoke('serial_connect', { port, baudRate });
}

export async function serialDisconnect() {
  return invoke('serial_disconnect');
}

export async function executeCommand(name: string) {
  return invoke('execute_command', { commandName: name });
}

// イベント購読
export function onSerialData(callback: (data: string) => void) {
  return listen<string>('serial-data', (event) => {
    callback(event.payload);
  });
}

export function onCommandComplete(callback: (name: string) => void) {
  return listen<string>('command-complete', (event) => {
    callback(event.payload);
  });
}
```

### 6.6 Tailwind CSS + shadcn-svelte

コンポーネントのスタイリングには Tailwind CSS と shadcn-svelte を使用します。

```svelte
<script lang="ts">
  import { Button } from '$lib/components/ui/button';
  import { Card } from '$lib/components/ui/card';
</script>

<Card class="p-4 space-y-2">
  <h2 class="text-lg font-bold text-slate-900 dark:text-slate-100">
    シリアル接続
  </h2>
  <Button
    variant="default"
    class="w-full"
    onclick={handleConnect}
  >
    接続
  </Button>
</Card>
```

---

## 7. テスト

### 7.1 Rust テスト（cargo test）

```bash
# ユニットテスト + 統合テスト
cargo test

# 特定のモジュールのみ
cargo test -p poke-controller-serial

# ドキュメントテストを含む
cargo test --doc

# テストカバレッジ（要: cargo-tarpaulin）
cargo tarpaulin --ignore-tests
```

テストコードの配置:

```rust
// インラインユニットテスト
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serial_command_format() {
        let cmd = SerialCommand::new(0xABCD, Hat::Center, Stick::default());
        assert_eq!(cmd.to_string(), "0xABCD 08 80 80 80 80\r\n");
    }

    #[test]
    fn test_baud_rate_validation() {
        assert!(BaudRate::new(115200).is_ok());
        assert!(BaudRate::new(0).is_err());
    }
}
```

```rust
// 統合テスト（tests/ ディレクトリ）
// src-tauri/tests/serial_integration.rs
use poke_controller::serial;

#[test]
fn test_serial_connect_disconnect() {
    // モックされたシリアルポートを使用
    let port = serial::connect("loopback://").unwrap();
    assert!(port.is_connected());
    port.disconnect();
    assert!(!port.is_connected());
}
```

### 7.2 Python テスト（pytest）

```bash
# Python パッケージのテスト
cd python
pip install -e ".[test]"
pytest tests/ -v

# カバレッジレポート
pytest tests/ --cov=poke_controller --cov-report=html

# 特定のテストファイル
pytest tests/test_commands.py -v
```

テストコードの例:

```python
# python/tests/test_commands.py
import pytest
from poke_controller.commands.base import CommandBase

class TestCommandBase:
    def test_command_name_default(self):
        cmd = CommandBase()
        assert cmd.name == ""

    def test_command_with_params(self):
        cmd = CommandBase(params=["--repeat", "3"])
        assert cmd.params == ["--repeat", "3"]

    def test_stdout_destination_default(self):
        cmd = CommandBase()
        assert cmd.stdout_destination == "1"
```

### 7.3 Web テスト（vitest + Playwright）

```bash
cd web

# ユニットテスト
npm run test           # vitest ワンショット実行
npm run test:watch     # ウォッチモード

# カバレッジ
npm run test:coverage

# E2E テスト（Playwright）
npm run test:e2e
npx playwright show-report  # レポート表示
```

ユニットテストの例:

```typescript
// web/src/tests/wsClient.test.ts
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { wsClient } from '$lib/services/wsClient';

describe('WsClient', () => {
  beforeEach(() => {
    wsClient.messages.set([]);
  });

  it('should parse WebSocket messages', () => {
    const mockEvent = new MessageEvent('message', {
      data: JSON.stringify({ type: 'serial_data', payload: '0xABCD' }),
    });
    wsClient['ws'] = { onmessage: null } as any;
    wsClient['ws']!.onmessage!(mockEvent);

    const unsub = wsClient.messages.subscribe((msgs) => {
      expect(msgs).toHaveLength(1);
      expect(msgs[0].type).toBe('serial_data');
    });
    unsub();
  });

  it('should handle connection lifecycle', () => {
    expect(wsClient.connect).not.toThrow();
  });
});
```

E2E テストの例:

```typescript
// web/e2e/serial.spec.ts
import { test, expect } from '@playwright/test';

test('シリアル接続画面が表示される', async ({ page }) => {
  await page.goto('/');
  await page.click('text=Serial');
  await expect(page.locator('select#port-select')).toBeVisible();
  await expect(page.locator('button:has-text("接続")')).toBeVisible();
});

test('コマンド一覧が表示される', async ({ page }) => {
  await page.goto('/commands');
  await expect(page.locator('[data-testid="command-list"]')).toBeVisible();
});
```

### 7.4 結合テスト（E2E）

```bash
# Tauri アプリケーション全体の E2E テスト
nix run .#test-e2e

# 個別実行
cd web
npx playwright test --project=tauri
```

---

## 8. CI/CD ワークフロー

### 8.1 GitHub Actions

`.github/workflows/ci.yml` で CI/CD パイプラインを定義しています。

```yaml
name: CI

on:
  push:
    branches: [main, develop, refactor/rust-core]
  pull_request:
    branches: [main]

jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 20
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - run: cargo clippy -- -D warnings
      - run: cargo fmt --check
      - run: cd web && npm ci && npm run lint
      - run: pip install ruff && ruff check python/

  test-rust:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test

  test-python:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with:
          python-version: "3.11"
      - run: pip install maturin pytest
      - run: cd python && maturin develop && pytest

  test-web:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 20
      - run: cd web && npm ci
      - run: cd web && npm run test

  build:
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
    needs: [lint, test-rust, test-python, test-web]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: actions/setup-node@v4
        with:
          node-version: 20
      - run: cd web && npm ci
      - run: npx tauri build
      - uses: actions/upload-artifact@v4
        with:
          name: poke-controller-${{ matrix.os }}
          path: src-tauri/target/release/bundle/
```

### 8.2 CI パイプラインの流れ

```
Push / PR
  │
  ├── lint (Rust clippy + fmt, Python ruff, Web ESLint)
  │
  ├── test-rust (cargo test)
  ├── test-python (pytest)
  ├── test-web (vitest)
  │
  └── build (cargo build --release + npx tauri build)
        │
        └── release (GitHub Releases へアップロード)

Nightly:
  └── e2e (Playwright + Tauri)
```

### 8.3 リリース手順

```bash
# 1. バージョンタグを作成
git tag v0.1.0

# 2. タグをプッシュ
git push origin v0.1.0

# 3. GitHub Actions がリリースビルドを実行
# 4. ビルド成果物が GitHub Releases に自動アップロード

# 手動リリースビルド
nix run .#build
```

---

## 9. 新機能の追加

### 9.1 新しい Rust モジュールの追加

```bash
# 新しいモジュールを作成
cd src-tauri/src
mkdir -p my_feature
touch my_feature/mod.rs
touch my_feature/implementation.rs
```

```rust
// src-tauri/src/my_feature/mod.rs
pub mod implementation;
pub use implementation::*;
```

```rust
// src-tauri/src/lib.rs（モジュールを登録）
pub mod my_feature;
```

```rust
// Tauri コマンドとして公開
#[tauri::command]
fn my_feature_action(param: String) -> Result<String, String> {
    my_feature::implementation::do_something(&param)
        .map_err(|e| e.to_string())
}
```

### 9.2 新しい Web ページの追加

SvelteKit のファイルベースルーティングに従って、`web/src/routes/` 以下にディレクトリを作成します。

```bash
cd web/src/routes
mkdir -p my-page
touch my-page/+page.svelte
```

```svelte
<!-- web/src/routes/my-page/+page.svelte -->
<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';

  let data = $state<string[]>([]);

  $effect(() => {
    invoke('get_my_data').then((result) => {
      data = result as string[];
    });
  });
</script>

<h1 class="text-2xl font-bold">マイページ</h1>
<ul>
  {#each data as item}
    <li>{item}</li>
  {/each}
</ul>
```

NavBar にタブを追加するには、`NavBar.svelte` のタブ一覧にエントリを追加します。

```svelte
<!-- web/src/lib/components/NavBar.svelte -->
<script lang="ts">
  const tabs = [
    { label: 'Camera', path: '/camera' },
    { label: 'Commands', path: '/commands' },
    { label: 'Output', path: '/output' },
    { label: 'Settings', path: '/settings' },
    { label: 'MyPage', path: '/my-page' },  // ← 追加
    { label: 'About', path: '/about' },
  ];
</script>
```

### 9.3 新しい Python コマンドの追加

```python
# python/poke_controller/commands/my_new_command.py
from poke_controller.commands.base import CommandBase

class MyNewCommand(CommandBase):
    """新しい自動化コマンドのテンプレート"""

    name = "my_new_command"
    description = "新しいコマンドの説明"
    params = ["--option1", "--option2"]

    # 通知設定
    isWinNotStart = False
    isWinNotEnd = True
    isDiscordNotStart = False
    isDiscordNotEnd = True

    # 出力設定
    stdout_destination = "1"
    pos_dialogue_buttons = 2  # 中央表示

    def run(self):
        self.print_t1("コマンドを開始します")
        # シリアルコマンドを送信
        self.serial_send("0xABCD 08 80 80 80 80")
        # カメラキャプチャ
        frame = self.camera_capture()
        # 画像処理...
        self.print_t1("コマンド完了")
```

### 9.4 機能追加の流れ

新機能を追加する際の推奨手順です。

1. **Issue を作成**: 機能の概要、モチベーション、受け入れ基準を記載
2. **フィーチャーブランチを作成**: `git checkout -b feature/my-feature`
3. **実装**:
   - Rust Core → Tauri コマンド → Web UI の順にレイヤーを実装
   - 各レイヤーでテストを並行して記述
4. **ドキュメントを更新**: 必要に応じてユーザーガイド、開発者ガイドを更新
5. **CI がパスすることを確認**: `nix run .#check`
6. **PR を作成**: 変更内容を記述し、レビューを依頼
7. **マージ**: `refactor/rust-core` ブランチにマージ

---

## 10. コードスタイルと規約

### 10.1 Rust

- **フォーマット**: `rustfmt`（`.rustfmt.toml` の設定に従う）
- **リンター**: `clippy`（`-D warnings` で警告をエラーとして扱う）
- **命名規則**:
  - ファイル名: `snake_case.rs`
  - 関数/メソッド: `snake_case`
  - 構造体/列挙型: `PascalCase`
  - 定数: `SCREAMING_SNAKE_CASE`
- **エラー処理**: カスタムエラー型を定義し、`anyhow` / `thiserror` を使用
- **非同期**: `tokio` ランタイムを使用。I/O 処理は `async/await`
- **コメント**: 公開 API には `///` ドキュメントコメントを必須とする

```rust
/// シリアルコマンドを表す構造体。
///
/// # フォーマット
/// `0xXXXX HH XX XX XX XX\r\n`
#[derive(Debug, Clone, PartialEq)]
pub struct SerialCommand {
    pub button_mask: u16,
    pub hat: HatPosition,
    pub left_stick: StickPosition,
    pub right_stick: StickPosition,
}

impl SerialCommand {
    /// コマンドをワイヤーフォーマットの文字列に変換する。
    pub fn to_wire_format(&self) -> String {
        format!(
            "0x{:04X} {:02X} {:02X} {:02X} {:02X} {:02X}\r\n",
            self.button_mask,
            self.hat as u8,
            self.left_stick.x,
            self.left_stick.y,
            self.right_stick.x,
            self.right_stick.y,
        )
    }
}
```

### 10.2 Python

- **フォーマット**: `ruff format`（ライン長 88 文字）
- **リンター**: `ruff check` + `mypy --strict`
- **命名規則**:
  - ファイル名: `snake_case.py`
  - 関数/メソッド: `snake_case`
  - クラス: `PascalCase`
  - 定数: `SCREAMING_SNAKE_CASE`
- **型ヒント**: すべての関数に型アノテーションを付与
- **エラー処理**: カスタム例外クラスを使用

```python
from typing import Optional


class CommandError(Exception):
    """コマンド実行に関するエラー。"""

    def __init__(self, message: str, exit_code: int = 1) -> None:
        self.exit_code = exit_code
        super().__init__(message)


class SerialCommandSender:
    """シリアルコマンドの送信を担当するクラス。"""

    def __init__(self, port: str, baud_rate: int = 115200) -> None:
        self.port = port
        self.baud_rate = baud_rate
        self._connected: bool = False

    @property
    def is_connected(self) -> bool:
        """シリアルポートが接続状態かを返す。"""
        return self._connected

    def send(self, command: str) -> None:
        """シリアルコマンドを送信する。

        Args:
            command: 送信するコマンド文字列。

        Raises:
            CommandError: 送信に失敗した場合。
        """
        if not self._connected:
            raise CommandError("シリアルポートが接続されていません")
        # 送信処理...
```

### 10.3 TypeScript / Svelte

- **フォーマット**: `prettier`（`web/.prettierrc` の設定に従う）
- **リンター**: `eslint`（`web/eslint.config.js` の設定に従う）
- **命名規則**:
  - ファイル名: `camelCase.ts` / `PascalCase.svelte`
  - 変数/関数: `camelCase`
  - 型/インターフェース: `PascalCase`
  - コンポーネント: `PascalCase.svelte`
- **Svelte 5 runes**: `$state`, `$derived`, `$effect`, `$props` を積極的に使用
- **状態管理**: グローバル状態は Svelte ストア（`writable`）、ローカル状態は `$state` rune を使用
- **Tauri IPC**: 直接 `invoke()` を呼び出すのではなく、`tauriBridge.ts` のラッパー関数を経由する

```typescript
// 型定義
interface SerialPortInfo {
  path: string;
  manufacturer: string | null;
  connected: boolean;
}

// サービス関数
export async function listSerialPorts(): Promise<SerialPortInfo[]> {
  return invoke<SerialPortInfo[]>('list_serial_ports');
}

// Svelte コンポーネント
<script lang="ts">
  import { onMount } from 'svelte';
  import { listSerialPorts } from '$lib/services/tauriBridge';

  let ports = $state<SerialPortInfo[]>([]);
  let selectedPort = $state<string>('');

  onMount(async () => {
    ports = await listSerialPorts();
  });
</script>
```

### 10.4 コミットメッセージ規約

[Conventional Commits](https://www.conventionalcommits.org/) に従います。

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

| タイプ | 説明 |
|-------|------|
| `feat` | 新機能 |
| `fix` | バグ修正 |
| `refactor` | リファクタリング（新機能でもバグ修正でもないコード変更） |
| `docs` | ドキュメントのみの変更 |
| `test` | テストの追加・修正 |
| `chore` | ビルドプロセス、ツール、依存関係の変更 |
| `style` | フォーマットのみの変更（ロジック変更なし） |
| `perf` | パフォーマンス改善 |

例:

```
feat(serial): シリアルポートの自動再接続機能を追加

切断後に自動的に再接続を試みる機能を実装。
再接続間隔は設定可能（デフォルト: 5秒）。

Closes #42
```

### 10.5 ブランチ戦略

| ブランチ | 用途 |
|---------|------|
| `main` | 安定版リリース。常にデプロイ可能な状態を保つ |
| `develop` | 開発の最新状態。次期リリースの統合ブランチ |
| `refactor/rust-core` | Rust-core リファクタリング専用ブランチ |
| `feature/*` | 機能開発ブランチ（例: `feature/auto-reconnect`） |
| `fix/*` | バグ修正ブランチ（例: `fix/serial-crash`） |
| `release/*` | リリース準備ブランチ（例: `release/v0.1.0`） |

### 10.6 PR レビューチェックリスト

- [ ] CI（lint + test + build）がパスしている
- [ ] 適切なテストが含まれている
- [ ] ドキュメントが更新されている
- [ ] コードスタイルが規約に沿っている
- [ ] エラーハンドリングが適切に行われている
- [ ] セキュリティ上の問題がないか確認した
- [ ] クロスプラットフォームで動作するか考慮した

---

## 付録

### A. 便利なコマンド一覧

```bash
# 開発環境
nix develop                    # 開発シェルに入る
nix run .#dev                  # 開発サーバー起動

# ビルド
nix run .#build                # リリースビルド
cargo build                    # Rust デバッグビルド
maturin develop                # Python 開発インストール

# テスト
nix run .#test                 # 全テスト
cargo test                     # Rust テスト
pytest python/tests            # Python テスト
npm run test                   # Web テスト

# リンター・フォーマット
nix run .#check                # 全チェック
cargo clippy -- -D warnings    # Rust リンター
ruff check python/             # Python リンター
npm run lint                   # Web リンター

# クリーン
nix run .#clean                # 全キャッシュ削除
cargo clean                    # Rust キャッシュ削除
rm -rf web/node_modules        # Node_modules 削除
```

### B. トラブルシューティング

#### B.1 開発環境

| 問題 | 解決策 |
|------|--------|
| `nix develop` が遅い | `nix develop --option substituters https://cache.nixos.org` でキャッシュを利用 |
| OpenCV が見つからない | `pkg-config --libs opencv4` でパスを確認。または Nix 環境を再作成 |
| Python モジュールがインポートできない | `maturin develop` を実行してバインディングを再ビルド |

#### B.2 ビルド

| 問題 | 解決策 |
|------|--------|
| Tauri ビルドで WebKit エラー | `sudo apt install libwebkit2gtk-4.1-dev`（Linux）|
| `cargo build` で依存関係エラー | `cargo update` を実行 |
| `npm install` でエラー | `node_modules` を削除して再試行、または `npm cache clean --force` |

#### B.3 テスト

| 問題 | 解決策 |
|------|--------|
| Python テストが `_core` モジュールを見つけられない | `cd python && maturin develop` を実行 |
| WebSocket テストがタイムアウトする | WebSocket サーバーが起動しているか確認 |
| E2E テストが Tauri を見つけられない | `npx tauri build` を先に実行 |

---

> **参考リンク**
> - [Tauri 2.x ドキュメント](https://v2.tauri.app/)
> - [SvelteKit 5 ドキュメント](https://kit.svelte.dev/)
> - [PyO3 ユーザーガイド](https://pyo3.rs/)
> - [maturin ドキュメント](https://maturin.rs/)
> - [Tailwind CSS v4](https://tailwindcss.com/)
> - [shadcn-svelte](https://shadcn-svelte.com/)
