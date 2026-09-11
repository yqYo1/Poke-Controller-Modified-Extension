# PokeCon文書案内

この案内は、読む人の役割と達成したい作業から適切な文書を選ぶための入口です。

役割は排他的ではなく、同じ人が利用者、スクリプト開発者、本体開発者を兼ねる場合があります。

## 読者の役割から入口を選ぶ

| 役割 | 想定する前提 | 読み始める文書 |
|---|---|---|
| **一般利用者** | OS上でアプリケーションと周辺機器を操作できる | [利用ガイド](USER_GUIDE.md) |
| **上級利用者** | TOML、環境変数、ネットワークの基本を理解している | [上級利用ガイド](ADVANCED_USAGE.md) |
| **ユーザースクリプト開発者** | Python 3.14と画像処理や外部通信の基礎を理解している | [ユーザースクリプト開発ガイド](SCRIPT_DEVELOPMENT.md) |
| **動的設定の開発者** | PythonまたはLuaのコードを信頼境界内で管理できる | [動的設定ガイド](DYNAMIC_CONFIGURATION.md) |
| **周辺機器開発者** | シリアル通信と対象MCUの電気的要件を理解している | [周辺機器開発ガイド](PERIPHERAL_DEVELOPMENT.md) |
| **HTTPクライアント開発者** | REST、WebSocket、JSON、ブラウザーのOriginを理解している | [HTTP APIガイド](HTTP_API.md) |
| **本体開発者** | Rust、TypeScript、Svelte、Nixの開発経験がある | [本体開発ガイド](DEVELOPMENT.md) |
| **リリース検証担当者** | 実機試験、実browser、再現可能な証拠を管理できる | [外部受入ゲート](ACCEPTANCE.md) |

上級利用者向け文書は一般操作を説明し直さず、[利用ガイド](USER_GUIDE.md)を読了していることを前提にします。

スクリプト開発者と周辺機器開発者は対象が異なるため、同じコントローラー概念でも各作業に必要な表現で説明します。

本体開発者は変更する公開境界に応じて、利用者向け文書やプロトコル文書も読みます。

## 作業の目的から文書を選ぶ

| 作業 | 文書 |
|---|---|
| 対応OS、導入、更新、削除、保存場所を確認する | [インストールガイド](INSTALL.md) |
| カメラ、シリアル、手動入力、コマンド、通知を操作する | [利用ガイド](USER_GUIDE.md) |
| 設定の優先順位、scope、反映時期、全設定IDを調べる | [設定リファレンス](SETTINGS.md) |
| プロファイル、LAN公開、worker環境、秘密値を管理する | [上級利用ガイド](ADVANCED_USAGE.md) |
| `init.py`または`init.lua`を書く | [動的設定ガイド](DYNAMIC_CONFIGURATION.md) |
| Pythonコマンドを書く、型検査する、依存packageを追加する | [ユーザースクリプト開発ガイド](SCRIPT_DEVELOPMENT.md) |
| MCU firmwareでcontroller frameを受信する | [周辺機器開発ガイド](PERIPHERAL_DEVELOPMENT.md) |
| RESTまたはWebSocketクライアントを実装する | [HTTP APIガイド](HTTP_API.md) |
| 旧実装からデータを移す | [移行ガイド](MIGRATION.md) |
| 症状から原因を切り分ける | [トラブルシュート](TROUBLESHOOTING.md) |
| process境界、所有権、状態transactionを理解する | [アーキテクチャ](ARCHITECTURE.md) |
| 本体を変更し、生成、検査、CIを通す | [本体開発ガイド](DEVELOPMENT.md) |
| 実機、実ブラウザー、securityを判定する | [外部受入ゲート](ACCEPTANCE.md) |

## 各文書の責務を区別する

| 文書 | 書く内容 | 書かない内容 |
|---|---|---|
| [利用ガイド](USER_GUIDE.md) | UIで完結する通常操作と安全な停止 | TOMLの全キー、worker内部、wire byte列 |
| [上級利用ガイド](ADVANCED_USAGE.md) | 理解せず変更すると危険な運用設定と復旧判断 | Rustのmodule構造、MCU firmware実装 |
| [設定リファレンス](SETTINGS.md) | 設定の解決規則、型、scope、mutability、設定ID | UIの逐次操作、動的APIの詳細 |
| [動的設定ガイド](DYNAMIC_CONFIGURATION.md) | `pokecon` namespace、event、callback、transaction | Pythonコマンドの互換API |
| [ユーザースクリプト開発ガイド](SCRIPT_DEVELOPMENT.md) | command discovery、lifecycle、公開Python API | Rust内部IPC、シリアルcodecの実装詳細 |
| [周辺機器開発ガイド](PERIPHERAL_DEVELOPMENT.md) | 3種類のwire形式、neutral、再接続、試験 | MCUごとの電圧、pin配置、対象機器固有の配線 |
| [HTTP APIガイド](HTTP_API.md) | request境界、revision、REST、WebSocket | UI componentの実装、内部worker IPC |
| [アーキテクチャ](ARCHITECTURE.md) | process、所有権、データ経路、停止順 | 開発コマンドの逐次手順 |
| [本体開発ガイド](DEVELOPMENT.md) | Nix環境、変更手順、正準レジストリ、検証階層 | 一般利用者向けの操作説明 |

## 必要な重複と正準情報を区別する

読者が別文書へ移動しなくても安全な操作を完了できるように、前提、安全上の警告、停止方法は必要な文書で繰り返します。

同じ事実でも、一般利用者には画面上の操作として、周辺機器開発者にはwire上の結果として説明します。

一方で、ほぼ同じAPI一覧や設定一覧を複数の文書へ複製しません。

設定の厳密な型とsurfaceは`rust/pokecon/registry/settings.json`を正本とします。

HTTPとWebSocketの厳密なschemaは`api/openapi.json`と`rust/pokecon/registry/protocol.json`を正本とします。

ユーザースクリプトの厳密なsignatureは起動時にData rootへ生成される`typings/Commands/*.pyi`を正本とします。

周辺機器のwire形式は[周辺機器開発ガイド](PERIPHERAL_DEVELOPMENT.md)を公開プロトコル文書とし、codecのテストで一致を検証します。

`legacy/CHANGELOG.txt`は旧Python実装の履歴資料であり、現在の機能、対応version、同梱物の正本ではありません。

## 文書を更新する開発者が守る規則

日本語の文章は[日本語技術文書の文章規範](https://gist.github.com/k16shikano/fd287c3133457c4fd8f5601d34aa817d#file-skill-md)を基準にします。

一つの文には一つの主要な判断だけを含め、一文ごとに改行します。

用語を初めて定義するときだけ太字にし、その後に用語自体を指す場合はかぎ括弧を使います。

見出しはその節で得られる具体的な情報を示し、内容を表さない「概要」や「その他」を避けます。

読者の作業に不要な内部識別子は載せず、正確な実装境界が必要な開発者文書へ移します。

将来の予定を現在の機能として書かず、実装、正準レジストリ、生成物、検査済みの挙動から確認できる内容だけを書きます。

文書間で同じ表を複製する代わりに正準文書へリンクし、読者の安全に必要な短い説明は各文書へ残します。

文書を変更したら、リンク、Markdown、表記、生成契約を[本体開発ガイド](DEVELOPMENT.md#文書を変更する)の手順で検査します。
