# エンドユーザー向けガイド

Poke-Controller Modified Extension のインストールから日常使い方までを解説します。

## 目次

1. [インストール](#1-インストール)
2. [初回セットアップ](#2-初回セットアップ)
3. [Web UI の使い方](#3-web-ui-の使い方)
4. [スクリプトの実行](#4-スクリプトの実行)
5. [トラブルシューティング](#5-トラブルシューティング)

---

## 1. インストール

### 方法 A: Nix を使う（推奨）

Nixパッケージマネージャーがあれば、開発環境を自動で構築できます：

```bash
git clone https://github.com/yqYo1/Poke-Controller-Modified-Extension.git
cd Poke-Controller-Modified-Extension
nix develop
```

初回はRustツールチェインやPythonライブラリのダウンロードに時間がかかります。

### 方法 B: 手動インストール

#### 必要環境

- **OS**: Windows 10/11, macOS, Linux
- **Python**: 3.14以上
- **Rust**: 1.85以上（Rustup推奨）

#### 手順

```bash
# 1. Python仮想環境を作成
python3.14 -m venv .venv
source .venv/bin/activate  # Windows: .venv\Scripts\activate

# 2. Python依存関係をインストール
pip install -e ".[dev]"

# 3. Rust依存関係をビルド
maturin develop --manifest-path rust/pokecon-pybindings/Cargo.toml

# 4. 確認
pytest tests/ -v
```

---

## 2. 初回セットアップ

### シリアルポートの確認

ゲーム機と接続する前に、使用可能なシリアルポートを確認します：

```bash
# Linux/macOS
ls /dev/ttyUSB* /dev/ttyACM*

# Windows
# デバイスマネージャー → ポート (COM & LPT) を確認
```

### 設定ファイル

初回起動時に `profiles/default/settings.ini` が自動作成されます。主な設定項目：

| 設定項目 | 説明 | デフォルト |
|---------|------|----------|
| `serial_port` | シリアルポート番号 | 0 |
| `serial_baudrate` | ボーレート | 9600 |
| `camera_index` | カメラデバイス番号 | 0 |
| `format` | 送信フォーマット | Default |

---

## 3. Web UI の使い方

### 起動方法

#### Tauri デスクトップアプリ

```bash
nix run .#tauri-dev
```

ネイティブウィンドウが開きます。内部でHTTPサーバーも起動するため、ブラウザからも同じURLでアクセスできます。

#### Web ブラウザ

```bash
# バックエンドのみ起動
nix run .#web-dev

# 別ターミナルでフロントエンド
cd web && npm install && npm run dev
```

ブラウザで `http://localhost:8020` を開きます。

### 画面構成

| タブ | 用途 | スマホ操作 |
|------|------|----------|
| **📊 ダッシュボード** | カメラ映像、実行中コマンド、ログ確認 | 縦スクロールで全情報確認 |
| **🎮 コントローラー** | 仮想ゲームパッドで直接操作 | タップ・スワイプで入力 |
| **📜 スクリプト** | 自動化スクリプトの一覧と実行 | タップで開始/停止 |
| **⚙️ 設定** | シリアル接続、カメラ設定 | フォーム入力 |

### スマホでの PWA インストール

#### iOS Safari

1. Safariで `http://(PCのIP):8020` を開く
2. 共有ボタン（□に↑）をタップ
3. 「ホーム画面に追加」を選択
4. アイコンがホーム画面に追加される

#### Android Chrome

1. Chromeで `http://(PCのIP):8020` を開く
2. 「ホーム画面に追加」ポップアップが表示されたらタップ
3. または ⋮ メニュー → 「ホーム画面に追加」

### コントローラー画面の操作方法

| 入力 | 操作方法 |
|------|---------|
| **ボタン（A/B/X/Y等）** | タップで押下、離すと解放 |
| **D-Pad** | 方向ボタンをタップ |
| **L/R/ZL/ZR** | ショルダーボタンをタップ |
| **アナログスティック** | スティックエリアをドラッグ |
| **タッチスクリーン** | タッチエリアをタップ |

### ダッシュボード画面

- **カメラプレビュー**: リアルタイムでゲーム画面を表示（WebSocket経由）
- **実行中コマンド**: 現在動作中のスクリプト名と状態
- **ログ**: リアルタイムで出力されるメッセージ

---

## 4. スクリプトの実行

### 組み込みスクリプト

付属のサンプルスクリプト：

| スクリプト名 | 内容 |
|-------------|------|
| `MashA.py` | Aボタン連打 |
| `AutoLeague.py` | 剣盾リーグ自動化 |
| `RaidPassword.py` | レイドパスワード入力 |
| `AutoRelease.py` | 自動リリース |

### 自作スクリプトの配置

1. `SerialController/Commands/PythonCommands/` に `.py` ファイルを置く
2. Web UI の「スクリプト」タブで更新ボタンをタップ
3. スクリプト一覧に表示されたら「開始」をタップ

### スクリプトの一時停止

実行中のスクリプトを一時停止するには：

- **Web UI**: ダッシュボードの「⏸ 一時停止」ボタン
- **キーボード**: Pause/Break キー

一時停止中は `show_var()` で変数一覧が表示されます。

---

## 5. トラブルシューティング

### Q: Web UI にアクセスできない

```bash
# バックエンドが起動しているか確認
curl http://127.0.0.1:8020/api/status

# ファイアウォールを確認（Windows Defender等）
# ポート8020が開いているか確認
```

### Q: シリアルポートが見つからない

```bash
# Linux: 権限が必要な場合
sudo usermod -aG dialout $USER
# ログアウト・ログイン後に再試行

# Windows: ドライバ確認
# CH340/CP210x等のドライバがインストールされているか確認
```

### Q: カメラ映像が表示されない

1. 別アプリでカメラが占有されていないか確認
2. `v4l2-ctl --list-devices`（Linux）でデバイス番号を確認
3. 設定で `camera_index` を変更

### Q: スクリプトが動作しない

```bash
# 互換性テストを実行
pytest tests/test_script_compatibility.py -v

# ログを確認
# Web UI → ダッシュボード → ログパネル
```

### Q: スマホから接続できない

1. PCとスマホが同じWiFiに接続しているか確認
2. PCのIPアドレスを確認：`ip addr` または `ifconfig`
3. `http://(PCのIP):8020` でアクセス
4. ファイアウォールで8020ポートを開放

---

## 関連ドキュメント

- [スクリプト開発者向けガイド](script-guide.md) - PythonCommand API詳細
- [Web UI 詳細ガイド](web-ui-guide.md) - PWA設定、高度な操作
- [開発者向けガイド](developer-guide.md) - ビルド方法、アーキテクチャ
