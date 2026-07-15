# Poke-Controller Modified Extension

Poke-Controller Modified Extensionを、Rustコア、Python互換層、動的設定ワーカー、Web／Tauri UIで再構築するプロジェクトです。

## 現在の状態

このブランチは**再実装前の仕様確定段階**です。旧実装コードはクリーンな再実装に備えて削除されており、現時点では実行可能なアプリケーションを提供していません。

- 実装の唯一の規範は[`SPECIFICATION.md`](SPECIFICATION.md)です。
- Rustクレートには`Cargo.toml`だけがあり、`src/`はまだありません。
- Web UI、サーバー、Python互換層、テストの実装ソースはまだありません。
- `nix run .`、ビルド、アプリケーション起動、実装テストは、再実装が進むまで利用できません。
- CIは対象ソースが存在しない検査を明示的にスキップし、文書・Nix・ソースフィルター等の適用可能な検査だけを実行します。

## 目標

定義書では、次のアーキテクチャと互換性を規定しています。

- シリアル、カメラ、入力、HTTPを管理するRustメインプロセス
- SvelteKit 2、Svelte 5、Tailwind CSS v4によるレスポンシブUI
- TauriデスクトップモードとスタンドアロンWebモード
- PythonとLuaに同一の動的設定APIを提供するグローバル動的設定ワーカー
- プロファイルごとのCPythonユーザースクリプトワーカー
- 共有メモリによる低遅延カメラフレーム共有
- WebRTCを主経路とし、WebSocketへ自動フォールバック・復旧する通信
- 固定3ベースラインと自動追補コーパスに対するPythonユーザースクリプト互換性
- CLI、環境変数、TOML、動的設定、UI、OpenAPIを正準設定レジストリから一貫して公開する設定システム

PWAは将来機能です。macOSは現時点の対象外です。現在の対象デスクトップOSはWindowsとLinuxです。

## 開発環境

Nix開発環境は利用できます。

```bash
# 初回のみ
# direnv allow

# 開発シェル
nix develop

# 文書・設定を含む整形
nix fmt

# 文書のtypo検査
nix develop --command typos SPECIFICATION.md README.md
```

実装ソースが追加された後は、`flake.nix`に定義済みのビルド、テスト、リント用appを同じNix環境から使用します。ソースが存在しない現段階で、これらのappがアプリケーションの動作を保証することはありません。

## リポジトリ構造

現在の主要な追跡対象は次のとおりです。

```text
.
├── SPECIFICATION.md                 # 再実装の規範となる定義書
├── README.md                        # 現在状態と目標の概要
├── AGENTS.md                        # 開発時のプロジェクト指示
├── flake.nix / flake.lock           # Nix開発環境・CIタスク
├── Cargo.toml / Cargo.lock          # Rustワークスペース骨格
├── pyproject.toml                   # Python 3.14向け設定
├── rust/
│   ├── pokecon-core/Cargo.toml
│   └── pokecon-pybindings/Cargo.toml
├── docs/                            # 旧文書・移行時の参考資料
├── .github/workflows/               # CIワークフロー
└── src-server/icons/                # デスクトップアイコン
```

`docs/`配下の文書は旧実装や過去の計画を含む参考資料です。再実装時のAPI・挙動・構成は`SPECIFICATION.md`を優先してください。

## 実装時の方針

- 先に定義書を更新し、その後に実装します。
- 開発・検証は直接コマンドではなくNix flake appを使用します。
- PythonとLuaの公開APIは名前・名前空間・セマンティクスを揃えます。
- 既存スクリプト互換性は定義書の固定ベースラインと昇格規則で検証します。
- PWAやmacOSなど将来機能を、実装済みとしてREADMEへ記載しません。

## ライセンス

[MIT License](LICENSE)

## 謝辞

[Poke-Controller](https://github.com/KawaSwitch/Poke-Controller)の開発者であるKawaSwitch氏、[Poke-Controller Modified](https://github.com/Moi-poke/Poke-Controller-Modified)の開発者であるmoi_poke氏に感謝します。
