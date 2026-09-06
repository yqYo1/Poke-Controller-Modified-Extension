# Poke-Controller Modified Extension

Poke-Controller Modified Extensionは、ゲーム機向けのコントローラー入力、カメラ映像、Pythonコマンド、動的設定を一つのローカルアプリケーションで扱うソフトウェアです。

Rustのメインプロセスがシリアル機器、カメラ、設定、HTTP、WebSocket、WebRTCを所有し、ユーザースクリプトと動的設定は分離したworkerで実行します。

対応する配布対象はLinux x86-64とWindows x86-64です。

## 読む文書を選ぶ

文書は読者の役割と作業目的に分けています。

最初に[文書案内](docs/README.md)で、自分の役割に対応する入口を確認してください。

| 読者 | 最初に読む文書 |
|---|---|
| 初めて使う利用者 | [利用ガイド](docs/USER_GUIDE.md) |
| 導入や更新を担当する利用者 | [インストールガイド](docs/INSTALL.md) |
| 設定やLAN公開を管理する上級利用者 | [上級利用ガイド](docs/ADVANCED_USAGE.md) |
| Pythonコマンドを作る開発者 | [ユーザースクリプト開発ガイド](docs/SCRIPT_DEVELOPMENT.md) |
| シリアル通信先を作る周辺機器開発者 | [周辺機器開発ガイド](docs/PERIPHERAL_DEVELOPMENT.md) |
| 本体を変更する開発者 | [本体開発ガイド](docs/DEVELOPMENT.md) |

## Nixで起動する

Nixが利用できるLinux環境では、checkoutからWeb UIを起動できます。

```bash
nix run . -- --ui web
```

既定のURLは`http://127.0.0.1:8020/ui/`です。

Tauriデスクトップを起動する場合は次を実行します。

```bash
nix run .#tauri
```

ポートやプロファイルは設定用CLI引数で指定できます。

```bash
nix run . -- --ui web --port 8080 --profile example
```

初回起動では不足している設定ファイルと型情報を作成し、既存のユーザー編集ファイルは上書きしません。

配布物からの導入、Windowsでの起動、オフライン導入は[インストールガイド](docs/INSTALL.md)を参照してください。

## 対応範囲を確認する

LinuxとWindowsの標準成果物は、同じPokeCon本体にWeb UIとTauriデスクトップの両方を含み、起動時に表示形態を選択します。

macOSとPWAは現行リリースの対象外です。

Pythonユーザースクリプトの対象言語版はPython 3.14です。

Web UIの開発と構築にはBunを使用し、配布済みアプリケーションの実行にBunやNode.jsは必要ありません。

実機シリアル、物理カメラ、音声、外部通知には、仮想I/O試験に加えて環境ごとの[外部受入ゲート](docs/ACCEPTANCE.md)が必要です。

## Nix devShellで開発する

開発作業は、toolchainを固定したNix devShell内で行います。`.envrc`は`use flake`を指定しているため、新しいworktreeでは一度`direnv allow`を実行します。自動読込みを使わない場合は`nix develop`で同じ環境へ入れます。

hostに直接導入したtoolchainは使用しません。完了gate、formatter、用途別の隔離実行は引き続き固定したflake appから実行します。

```bash
direnv allow
# または: nix develop
nix fmt
nix run .#check
nix run .#cargo -- test --locked --workspace
nix run .#hooks-install
nix run .#editor-smoke
```

frontendのhot reloadには`nix run .#web-dev`、editor連携には`nix run .#editor -- --print`を使用します。Rust、Python、TypeScript、Svelteのlanguage server実動検査は`nix run .#editor-smoke`で実行します。

pre-commit hookは新しく作成したworktreeごとに`nix run .#hooks-install`で導入します。同じappを再実行すると、Nix生成configを収束させ、欠落、実行権限を失った、または認識済み生成形式のhookを再導入します。custom hookや予期しないsymlinkは置換せず拒否します。

変更対象ごとの検証方法と正準ファイルは[本体開発ガイド](docs/DEVELOPMENT.md)を参照してください。

## リポジトリの入口を確認する

```text
rust/                         RustワークスペースとTauriアプリ
web/                          SvelteKitとSvelteのWeb UI
python/pokecon/               純Python互換メタデータと生成用型情報
api/                          生成済みOpenAPI文書
generated/                    設定schemaとLua型情報などの生成物
compatibility/                固定互換コーパスと昇格履歴
scripts/                      品質、統合、互換性、配布用タスク
tests/                        Python toolingのテスト
docs/                         読者別ガイドと横断リファレンス
```

公開契約はRustのwire型と正準レジストリからOpenAPI、TypeScript、Python、Luaの型情報へ生成します。

生成物は手編集しません。

## ライセンスと謝辞

本リポジトリは[MIT License](LICENSE)で提供します。

[Poke-Controller](https://github.com/KawaSwitch/Poke-Controller)のKawaSwitch氏と、[Poke-Controller Modified](https://github.com/Moi-poke/Poke-Controller-Modified)のmoi_poke氏をはじめ、既存実装とスクリプト作者の皆様に感謝します。
