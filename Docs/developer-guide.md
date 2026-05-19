# 開発者向けガイド

Poke-Controller Modified Extension のアーキテクチャ、ビルド方法、開発フローを解説します。

## 目次

1. [アーキテクチャ概要](#1-アーキテクチャ概要)
2. [プロジェクト構造](#2-プロジェクト構造)
3. [ビルド方法](#3-ビルド方法)
4. [Rust コア](#4-rust-コア)
5. [Python 互換層](#5-python-互換層)
6. [PyO3 バインディング](#6-pyo3-バインディング)
7. [Web/Tauri UI](#7-webtauri-ui)
8. [テスト](#8-テスト)
9. [CI/CD](#9-cicd)
10. [コントリビューション](#10-コントリビューション)

---

## 1. アーキテクチャ概要

### 設計思想

```
┌─────────────────────────────────────────────────────────┐
│                    ユーザースクリプト                      │
│              (Python - 後方互換性保持)                     │
├─────────────────────────────────────────────────────────┤
│                  Python 互換層                           │
│         (python/pokecon/ - インポートハック)              │
├─────────────────────────────────────────────────────────┤
│                  PyO3 バインディング                      │
│         (rust/pokecon-pybindings/ - Rust↔Python)         │
├─────────────────────────────────────────────────────────┤
│                    Rust コア                              │
│  ┌────────────────────────────────────────────────┐     │
│  │              pokecon-core                       │     │
│  │  ┌────────┐ ┌──────┐ ┌────────┐ ┌────────┐   │     │
│  │  │ serial │ │ cv   │ │ events │ │ net    │   │     │
│  │  └────────┘ └──────┘ └────────┘ └────────┘   │     │
│  │  ┌────────┐ ┌──────┐                          │     │
│  │  │ notify │ │ lua  │                          │     │
│  │  └────────┘ └──────┘                          │     │
│  └────────────────────────────────────────────────┘     │
├─────────────────────────────────────────────────────────┤
│                  HTTP/WebSocket API                      │
│              (axum + tokio - 非同期)                     │
├─────────────────────────────────────────────────────────┤
│                  Web フロントエンド                       │
│            (SvelteKit 2 + Svelte 5 (runes))             │
└─────────────────────────────────────────────────────────┘
```

### レイヤー責務

| レイヤー | 責務 | 技術 |
|---------|------|------|
| ユーザースクリプト | 自動化ロジック | Python 3.14+ |
| Python互換層 | 後方互換性提供、メタクラス制御 | Python (PEP 695) |
| PyO3バインディング | Rust↔Python橋渡し | PyO3, maturin |
| Rustコア | ハードウェア制御、画像処理 | Rust 2024 |
| HTTP API | REST/WebSocket提供 | axum, tokio |
| Webフロントエンド | UI表示、ユーザー操作 | SvelteKit 2 + Svelte 5 (runes) |

---

## 2. プロジェクト構造

### ディレクトリ構成

```
.
├── flake.nix                 # Nix開発環境
├── pyproject.toml            # Pythonパッケージ
├── Cargo.toml                # Rustワークスペース
├── rust-toolchain.toml       # Rustツールチェイン
│
├── rust/                       # Rustコア（2クレート）
│   ├── pokecon-core/           # 全コア機能を統合
│   │   ├── Cargo.toml
│   │   ├── build.rs
│   │   ├── src/
│   │   │   ├── lib.rs           # モジュール再エクスポート
│   │   │   ├── command_manager.rs
│   │   │   ├── settings.rs
│   │   │   ├── profile.rs
│   │   │   ├── serial/          # シリアル通信、キー入力
│   │   │   │   ├── mod.rs
│   │   │   │   ├── sender.rs
│   │   │   │   ├── keypress.rs
│   │   │   │   ├── keys.rs
│   │   │   │   └── format.rs
│   │   │   ├── cv/              # カメラ、画像処理
│   │   │   │   ├── mod.rs
│   │   │   │   ├── camera.rs
│   │   │   │   ├── image_processing.rs
│   │   │   │   └── backends.rs
│   │   │   ├── events/          # イベントバス、レジストリ
│   │   │   │   ├── mod.rs
│   │   │   │   ├── bus.rs
│   │   │   │   ├── registry.rs
│   │   │   │   ├── handler.rs
│   │   │   │   └── user_event.rs
│   │   │   ├── lua/             # LuaJIT統合
│   │   │   │   ├── mod.rs
│   │   │   │   ├── api.rs
│   │   │   │   └── runtime.rs
│   │   │   ├── net/             # Socket、MQTT
│   │   │   │   ├── mod.rs
│   │   │   │   ├── socket.rs
│   │   │   │   └── mqtt.rs
│   │   │   └── notify/          # Discord、LINE、Windows通知
│   │   │       ├── mod.rs
│   │   │       ├── discord.rs
│   │   │       ├── line.rs
│   │   │       └── windows.rs
│   │   └── tests/               # 統合テスト
│   └── pokecon-pybindings/      # PyO3バインディング
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           ├── keys.rs
│           ├── python_cmd.rs
│           ├── image_proc.rs
│           └── events.rs
│
├── python/pokecon/             # Python互換層
│   ├── __init__.py             # インポートハック
│   ├── commands.py             # PythonCommand実装
│   ├── keys.py                 # キー入力抽象化
│   ├── _meta.py                # CommandMetaメタクラス
│   ├── _adapter.py             # Rustアダプター
│   └── script_loader.py        # 動的ロード
│
├── src-server/                  # HTTP/WebSocket/WebRTC server (renamed from src-tauri)
│   ├── src/
│   │   ├── main.rs             # HTTP server, API, WebSocket
│   │   ├── webrtc.rs           # WebRTC signaling
│   │   └── vaapi_encoder.rs    # VAAPI hardware encoding
│   ├── Cargo.toml              # Server dependencies
│   └── tauri.conf.json
│
├── web/                        # Webフロントエンド（SvelteKit）
│   ├── package.json
│   ├── svelte.config.js        # SvelteKit設定（adapter-static, /ui base）
│   ├── vite.config.ts          # Vite設定
│   ├── tsconfig.json
│   ├── src/
│   │   ├── app.html            # HTMLテンプレート
│   │   ├── app.css             # グローバルスタイル
│   │   ├── app.d.ts            # 型定義
│   │   ├── service-worker.ts   # Service Worker (PWA)
│   │   ├── routes/             # ページルーティング
│   │   │   ├── +layout.svelte  # ルートレイアウト
│   │   │   ├── +layout.ts      # レイアウトローダー
│   │   │   ├── +page.svelte    # トップページ
│   │   │   ├── serial/
│   │   │   │   └── +page.svelte
│   │   │   ├── camera/
│   │   │   │   └── +page.svelte
│   │   │   ├── commands/
│   │   │   │   ├── +page.svelte
│   │   │   │   ├── CommandActions.svelte
│   │   │   │   ├── McuCommandList.svelte
│   │   │   │   ├── PythonCommandList.svelte
│   │   │   │   └── ShortcutButtons.svelte
│   │   │   ├── keyconfig/
│   │   │   │   └── +page.svelte
│   │   │   ├── manual/
│   │   │   │   └── +page.svelte
│   │   │   ├── notification/
│   │   │   │   └── +page.svelte
│   │   │   ├── others/
│   │   │   │   └── +page.svelte
│   │   │   └── pokemonhome/
│   │   │       └── +page.svelte
│   │   └── lib/
│   │       ├── api/            # APIクライアント
│   │       │   ├── client.ts
│   │       │   ├── types.ts
│   │       │   ├── websocket.ts
│   │       │   ├── webrtc-video.ts
│   │       │   └── datachannel.ts
│   │       ├── components/     # 再利用可能UIコンポーネント
│   │       │   ├── NavBar.svelte
│   │       │   ├── MenuBar.svelte
│   │       │   ├── StatusBar.svelte
│   │       │   ├── MainToolbar.svelte
│   │       │   ├── ThemeProvider.svelte
│   │       │   ├── SoftwareController.svelte
│   │       │   ├── HardwareControl.svelte
│   │       │   ├── CameraPreview.svelte
│   │       │   ├── SerialMonitor.svelte
│   │       │   ├── LogPanel.svelte
│   │       │   ├── OutputPanel.svelte
│   │       │   ├── ControllerSimulator.svelte
│   │       │   └── ... （その他多数）
│   │       └── theme.ts        # テーマ設定
│   └── dist/                   # ビルド出力
│
├── SerialController/           # 既存コード（後方互換性）
│   ├── Commands/
│   │   ├── PythonCommandBase.py
│   │   ├── Keys.py
│   │   └── PythonCommands/     # ユーザースクリプト
│   └── Window.py               # tkinter GUI
│
├── scripts/                    # ユーティリティスクリプト
│   └── ci-watch.sh
│
└── tests/                      # テスト
    ├── conftest.py             # モック設定
    └── test_script_compatibility.py
```

---

## 3. ビルド方法

### Nix を使う（推奨）

```bash
# 開発環境に入る
nix develop

# すべてのチェック
nix run .#check

# PyO3ビルド
nix run .#maturin-develop

# Tauri開発サーバー
nix run .#tauri-dev

# Tauriビルド
nix run .#tauri-build

# アプリケーション起動（デフォルト: Tauri）
nix run .

# Web UIのみ起動
nix run . -- --ui web
```

### 手動ビルド

```bash
# Python環境
python3.14 -m venv .venv
source .venv/bin/activate
pip install -e ".[dev]"

# Rustビルド
cargo check                    # ワークスペース全体
cargo check -p pokecon-core    # 特定クレート
cargo test                     # テスト実行

# PyO3ビルド
maturin develop --manifest-path rust/pokecon-pybindings/Cargo.toml

# Webフロントエンド
cd web
npm install
npm run build
```

### 個別ビルドコマンド

| ターゲット | コマンド |
|----------|---------|
| Rustチェック | `cargo check` |
| Rustテスト | `cargo test` |
| Clippy | `cargo clippy -- -D warnings` |
| Pythonテスト | `pytest tests/ -v` |
| Ruffチェック | `ruff check .` |
| Ruffフォーマット | `ruff format .` |
| treefmt | `treefmt` |
| PyO3ビルド | `maturin develop` |
| Webビルド | `cd web && npm run build` |
| Svelteチェック | `cd web && npm run check` |

---

## 4. Rust コア

### クレート一覧

| クレート | 用途 | 主要モジュール |
|---------|------|--------------|
| `pokecon-core` | 全コア機能（コマンド管理、シリアル通信、画像処理、イベント駆動、ネットワーク、通知、Lua統合、設定・プロファイル管理） | `command_manager`, `settings`, `profile`, `serial::{sender,keypress,keys,format}`, `cv::{camera,image_processing,backends}`, `events::{bus,registry,handler,user_event}`, `net::{socket,mqtt}`, `notify::{discord,line,windows}`, `lua::{api,runtime}` |
| `pokecon-pybindings` | Python連携 | `keys`, `python_cmd`, `image_proc`, `events` |

### シリアル通信フロー

```rust
// 1. Senderでポートを開く
let mut sender = Sender::new(true);
sender.open(0, None, 9600).await?;

// 2. KeyPressで入力を生成
let mut keypress = KeyPress::new(&sender);
keypress.press(Button::A, 0.1).await?;

// 3. フォーマット変換
let data = keypress.convert2str();  // Defaultフォーマット
sender.write_row(data).await?;
```

### イベントバス

```rust
use pokecon_core::events::{EventBus, Event};

let bus = EventBus::new();

// 購読
let mut rx = bus.subscribe("command.start");
tokio::spawn(async move {
    while let Ok(event) = rx.recv().await {
        println!("Event: {:?}", event);
    }
});

// 発行
bus.publish("command.start", Event::new("AutoLeague"));
```

---

## 5. Python 互換層

### インポートハック

`python/pokecon/__init__.py` で `sys.modules` を書き換え：

```python
# ユーザースクリプトが使う古いパス
from Commands.PythonCommandBase import PythonCommand

# → 内部的に新パッケージにリダイレクト
mod_python_cmd = types.ModuleType("Commands.PythonCommandBase")
mod_python_cmd.PythonCommand = PythonCommand  # 新実装
sys.modules["Commands.PythonCommandBase"] = mod_python_cmd
```

### メタクラスアーキテクチャ

```
ユーザースクリプト
    └── class MyCmd(PythonCommand):
            └── metaclass=CommandMeta
                    └── MRO制御
                            ├── Rust実装 (_RustPythonCommandImpl)
                            └── Pythonフォールバック
```

`CommandMeta` は `__backend__` クラス変数で実装を切り替え：

```python
class MyCommand(PythonCommand):
    __backend__ = "rust"  # または "python"
```

### 型ヒント（Python 3.14 + PEP 695）

```python
def pausedecorator[
    PythonCommandLike: "PythonCommand"
](func: Callable[Concatenate[PythonCommandLike, P], R]) -> ...
```

---

## 6. PyO3 バインディング

### モジュール構造

```rust
#[pymodule]
fn pokecon(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // pokecon.keys
    let keys_module = PyModule::new(m.py(), "keys")?;
    keys::register(&keys_module)?;
    m.add_submodule(&keys_module)?;
    
    // pokecon.command
    let cmd_module = PyModule::new(m.py(), "command")?;
    python_cmd::register(&cmd_module)?;
    m.add_submodule(&cmd_module)?;
    
    // pokecon.image_proc
    // pokecon.events
    ...
}
```

### PythonCommand の Rust 実装

```rust
#[pyclass]
pub struct PythonCommand {
    name: String,
    alive: bool,
    keypress: Option<KeyPress>,
    callbacks: Mutex<HashMap<String, PyObject>>,
}

#[pymethods]
impl PythonCommand {
    #[new]
    fn new(name: String) -> Self { ... }
    
    fn press(&self, buttons: String, duration: f64, wait: f64) -> PyResult<()> {
        // ボタン文字列をパースして KeyPress に送信
        let inputs = parse_buttons(&buttons);
        // ...
    }
    
    fn register_callback(&self, event: String, callback: PyObject) -> PyResult<()> {
        // Pythonコールバックを保存
        // trigger() で呼び出し
    }
}
```

---

## 7. Web/Tauri UI

### アーキテクチャ

```
┌─────────────┐     HTTP/WebSocket      ┌─────────────┐
│   Web UI    │ ◄─────────────────────► │   Tauri     │
│  (SvelteKit)│    http://127.0.0.1    │  (Rust)     │
│             │        :8020           │             │
└─────────────┘                        └─────────────┘
                                              │
                                              │ 内部呼び出し
                                              ▼
                                        ┌─────────────┐
                                        │  Rust コア   │
                                        │ (pokecon-core)│
                                        └─────────────┘
```

### APIエンドポイント

| カテゴリ | エンドポイント | メソッド | 説明 |
|---------|--------------|---------|------|
| **Status** | `/api/status` | GET | ヘルスチェック |
| | `/api/greet` | GET | グリーティング |
| | `/api/openapi.json` | GET | OpenAPIスキーマ |
| **Controller** | `/api/controller/type` | GET/POST | コントローラ種別取得/設定 |
| | `/api/controller/keyboard` | GET/POST | キーボード設定取得/設定 |
| | `/api/controller/mouse_stick` | GET/POST | マウススティック設定取得/設定 |
| **Camera** | `/api/cameras` | GET | カメラ一覧 |
| | `/api/camera/status` | GET | カメラ状態 |
| | `/api/camera/open` | POST | カメラ接続 |
| | `/api/camera/close` | POST | カメラ切断 |
| | `/api/camera/frame` | GET | フレーム取得 (base64) |
| | `/api/camera/capture` | POST | キャプチャ実行 |
| | `/api/camera/config` | POST | カメラ設定変更 |
| | `/camera/stream` | GET | MJPEGストリーム |
| **Input** | `/api/input/press` | POST | ボタン押下 |
| | `/api/input/hold` | POST | ボタン長押し開始 |
| | `/api/input/release` | POST | ボタン長押し解除 |
| | `/api/input/stick` | POST | スティック操作 |
| | `/api/input/touch` | POST | タッチ操作 |
| **Serial** | `/api/serial/ports` | GET | ポート一覧 |
| | `/api/serial/open` | POST | ポート接続 |
| | `/api/serial/close` | POST | ポート切断 |
| | `/api/serial/write` | POST | データ送信 |
| | `/api/serial/config` | POST | シリアル設定変更 |
| | `/api/serial/status` | GET | シリアル状態 |
| **Commands** | `/api/commands` | GET | スクリプト一覧 |
| | `/api/commands/load` | POST | スクリプト読み込み |
| | `/api/commands/start` | POST | スクリプト開始 |
| | `/api/commands/stop` | POST | スクリプト停止 |
| | `/api/commands/active` | GET | アクティブなスクリプト |
| | `/api/commands/filter` | POST | フィルター設定 |
| | `/api/commands/reload` | POST | スクリプト再読み込み |
| **Profile** | `/api/profile` | GET/POST | プロファイル一覧/設定 |
| **Notifications** | `/api/notifications/config` | GET/POST | 通知設定取得/更新 |
| | `/api/notifications/send` | POST | 通知送信 |
| **WebSocket** | `/ws` | GET | リアルタイムイベント |
| **Static** | `/ui/*` | GET | SvelteKit静的ファイル |

### WebSocket イベント

```javascript
// クライアント側
const ws = new WebSocket('ws://127.0.0.1:8020/ws');

ws.onmessage = (event) => {
    const data = JSON.parse(event.data);
    switch (data.type) {
        case 'camera.frame':
            // base64画像を表示
            img.src = `data:image/jpeg;base64,${data.payload.image}`;
            break;
        case 'command.start':
            console.log('Started:', data.payload.name);
            break;
        case 'command.error':
            console.error('Error:', data.payload.message);
            break;
    }
};
```

---

## 8. テスト

### テスト構成

| テスト | 場所 | 内容 |
|--------|------|------|
| Python互換性 | `tests/test_script_compatibility.py` | 58ケース（インポート、インスタンス化、API呼び出し） |
| Rustユニット | `rust/*/src/` | 各クレートのユニットテスト |
| Rust統合 | `rust/pokecon-core/src/serial/tests/` 等 | 統合テスト |

### モック戦略

```python
# conftest.py
class MockSender:
    """シリアル送信のモック"""
    def __init__(self):
        self.written_rows = []
        self.written_lists = []
    
    def writeRow(self, row):
        self.written_rows.append(row)
    
    def writeList(self, data):
        self.written_lists.append(data)

class MockCamera:
    """カメラのモック"""
    def readFrame(self):
        return np.zeros((720, 1280, 3), dtype=np.uint8)
```

---

## 9. CI/CD

### GitHub Actions ワークフロー

| ワークフロー | トリガー | 内容 |
|------------|---------|------|
| `python-lint` | push/PR | ruffチェック・フォーマット |
| `rust-lint` | push/PR | cargo clippy |
| `pytest` | push/PR | Pythonテスト |
| `nix-fmt` | push/PR | treefmtチェック |

### Nix アプリ一覧

```bash
nix run .#check          # すべてのチェック
nix run .#test           # pytestのみ
nix run .#cargo-test     # cargo testのみ
nix run .#clippy         # cargo clippyのみ
nix run .#fmt            # treefmt実行
nix run .#maturin-develop # PyO3ビルド
nix run .#tauri-dev      # Tauri開発サーバー
nix run .#tauri-build    # Tauriビルド
nix run .                # アプリケーション起動（デフォルト）
```

---

## 10. コントリビューション

### 開発フロー

1. **Issue作成** - バグ報告・機能提案
2. **ブランチ作成** - `feature/xxx` または `fix/xxx`
3. **実装** - テストを先に書く（TDD推奨）
4. **PR作成** - テスト通過を確認
5. **レビュー** - CIチェック必須

### コーディング規約

| 言語 | ツール | 設定 |
|------|-------|------|
| Python | ruff | `pyproject.toml` |
| Rust | clippy | `-D warnings` |
| Nix | nixfmt | `flake.nix` |
| 全般 | treefmt | `.treefmt.toml` |

### 型ヒント規約

- Python 3.14のPEP 695構文を積極的に使用
- `basedpyright`で型チェック
- `Final`, `ClassVar`, `Concatenate`, `ParamSpec` を適切に使用

---

## 関連ドキュメント

- [エンドユーザー向けガイド](user-guide.md) - インストール、使い方
- [スクリプト開発者向けガイド](script-guide.md) - PythonCommand API
- [APIリファレンス](api-reference.md) - 全API詳細
- [Web UI詳細](web-ui-guide.md) - PWA設定、フロントエンド

---

## 11. アーキテクチャレポートとテストカバレッジ

詳細なアーキテクチャ分析は `architecture_report.md` を参照。

### 現状のテストカバレッジ

| クレート | 単体テスト | 統合テスト | カバレッジ |
|---------|-----------|-----------|-----------|
| `pokecon-core` | | | |
| ├ serial | ✅ keys, format, keypress, sender | ✅ フォーマットパイプライン | 高 |
| ├ cv | ✅ camera, image_processing, backends | ✅ 画像処理パイプライン | 高 |
| ├ events | ✅ bus, registry, user_event, handler | ✅ イベントバス ＋ レジストリ連携 | 高 |
| ├ net | ✅ mqtt, socket | — | 中 |
| ├ notify | ✅ discord, line, windows | — | 中 |
| └ lua | ✅ api, runtime | — | 高 |
| `pokecon-pybindings` | ✅ keys, events, image_proc, python_cmd | — | 中 (新規) |
| src-server | ❌ (未対応) | — | 低 (未対応) |

### テスト実行方法

```bash
# 全Rustテスト
nix run .#cargo-test

# 特定モジュールのテスト
cargo test -p pokecon-core

# pokecon-core 内の特定モジュール
cargo test -p pokecon-core -- serial
cargo test -p pokecon-core -- cv
cargo test -p pokecon-core -- events

# 統合テストのみ
cargo test --test format_send_pipeline
cargo test --test image_pipeline_integration
cargo test --test event_bus_integration
```

### ベンチマーク実行

```bash
# シリアルフォーマットベンチ
cargo bench -p pokecon-core -- serial

# 画像処理ベンチ
cargo bench -p pokecon-core -- cv
```

### リファクタリング計画

`REFACTORING_PLAN.md` に詳細なリファクタリング計画を記載。優先順位：

1. **P0（即時）**: src-server/main.rs の分割、重複コード排除、不足テストの追加
2. **P1（重要）**: unsafe transmute の除去、Lua デッドロック修正、PythonCommand ランタイム管理改善
3. **P2（機能）**: Lua API の実機能化、イベントフェーズ配線
4. **P3（ポリッシュ）**: WindowsNotifier の改名、ドキュメント改善、CI 拡充
