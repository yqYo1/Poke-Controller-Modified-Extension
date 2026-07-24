# Poke-Controller Modified Extension

Poke-Controller Modified Extensionは、ゲーム機コントローラー自動化をRust中心の実行系へ再構築したローカルファーストのアプリケーションです。LinuxではWeb UIまたはTauriデスクトップ、WindowsではTauriデスクトップとして動作します。ユーザースクリプトにはPython 3.14互換APIを提供します。

## 実装状況

Rustメインプロセス、SvelteKit UI、Tauriシェル、Python／Lua動的設定、プロファイル単位のPython workerを接続済みです。主な境界は次のとおりです。

- Rustがシリアル、カメラ、コントローラー入力、設定、HTTP、WebSocket、WebRTCを所有します。
- Pythonユーザースクリプトは分離workerで実行し、ハードウェア操作を型付きIPC経由で依頼します。
- 動的Python／Lua設定は永続workerで評価し、同じ公開APIと設定レジストリを使います。
- Web UIとTauri UIは同じaxumバックエンド、OpenAPI、イベント系列を利用します。
- 固定3リポジトリの103スクリプトを、指定commitのままmanaged workerへ読み込む互換性ゲートを備えます。

詳細な規範は[SPECIFICATION.md](SPECIFICATION.md)、実装順序と受け入れ条件は[PLAN.md](PLAN.md)にあります。

## 起動

Nixが利用できるLinux環境では、次のコマンドで起動できます。

```bash
# Web UI。既定では http://127.0.0.1:8020/ui/
nix run . -- --ui web

# Tauriデスクトップ
nix run .#tauri

# ポートとプロファイルを指定
nix run . -- --ui web --port 8080 --profile example
```

`--bind-address`にはワイルドカードではない数値IPだけを指定できます。初回起動時に設定とプロファイルの雛形を作成します。既存のユーザー編集ファイルは上書きしません。

インストーラ、アップグレード、オフライン導入は[インストールガイド](docs/INSTALL.md)、設定のscope・優先順位・反映タイミングは[設定ガイド](docs/SETTINGS.md)、旧実装からの移行は[移行ガイド](docs/MIGRATION.md)を参照してください。

## 開発と検証

開発コマンドはNix devShell内で実行します。`.envrc`は`use flake`を設定済みです。

```bash
direnv allow        # 初回のみ
nix develop
nix fmt
nix run .#check
```

個別の再現可能タスクもflake appとして公開しています。

```bash
nix run .#clippy
nix run .#cargo-test
nix run .#virtual-io-check
nix run .#web-check
nix run .#compatibility
nix run .#tauri-check
nix run .#tauri-build -- --bundles deb
nix run .#package-smoke -- dist/tauri/*.deb
nix run .#package-install-smoke -- dist/tauri/*.deb
nix build .#pokecon-server
```

`nix run .#compatibility`は固定commitと昇格済みcommitを取得し、全スクリプトの内容hash、Python 3.14構文、import、クラス検出をmanaged workerで検証します。追跡済みの固定結果は[compatibility/fixed-results.json](compatibility/fixed-results.json)です。週次workflowは3 upstreamのdefault branchをimmutable SHAとして収集し、完全保証チェーン、runtime evidence、公開API契約が通った候補だけをhash chain付き履歴へ昇格します。失敗または未完了の実機gateは理由付きで隔離し、署名付きcommitのreview PRとして提出します。

Linuxの`nix run .#virtual-io-check`はkernelのPTYと`v4l2loopback`へテストpatternを流し、native serial／camera backendを実際のdevice node経由で検証します。実行中kernel用の`v4l2loopback` moduleとpasswordless `sudo`が必要です。このsmoke testは実機gateの前段であり、MCU、対象console、物理cameraを使う外部受入記録の代替ではありません。

Package CIはUbuntu 24.04へのクリーンインストール、完全オフラインのmanaged worker起動、upgrade／uninstall時のユーザーデータ保持、Linux成果物の2回buildによるバイト単位の再現性、Windows NSISのsilent install／startup／upgrade／uninstallを検証します。

実機、実ブラウザ、性能、統合stress、security、デスクトップライフサイクルは[外部受入ゲート](docs/ACCEPTANCE.md)の手順でrelease candidateごとに検証し、閉じたJSON記録を`nix run .#acceptance-record-check`で検査します。`--release-candidate <source commit>`はLinux／Windowsと4 browserを含む24件の必須matrixを集約検査します。未実施のgateやexample recordは合格証拠として扱いません。

## 構成

```text
rust/                         RustワークスペースとTauriアプリ
web/                          SvelteKit 2 / Svelte 5 UI
python/pokecon/               Python bindingと型情報
api/                          OpenAPIと生成TypeScript
compatibility/                固定コーパス、実行結果、昇格履歴
scripts/acceptance/           外部受入記録の検証
scripts/compatibility/        互換性corpusの収集・実行・昇格
scripts/integration/          仮想deviceを使う統合smoke test
scripts/quality/              source guardと生成contract検査
scripts/release/              配布物の構築・正規化・導入検査
scripts/ci-watch.sh           push後のGitHub Actions監視
tests/                        上記Python toolingの責務別test suite
docs/                         導入、設定、移行、受入、トラブルシュート
```

公開契約はRustレジストリからOpenAPI、TypeScript、Python/Lua typingsへ生成します。生成物を手編集せず、`nix run .#contract-check`でdriftを検出してください。

## 対象範囲

対象OSはWindowsとLinuxです。PWAとmacOSは将来機能であり、現行リリースの対象ではありません。実機シリアル、カメラ、音声、外部通知には、決定的fixtureに加えて環境ごとの明示的hardware gateが必要です。

問題の切り分けは[トラブルシュート](docs/TROUBLESHOOTING.md)、変更点は[CHANGELOG.md](CHANGELOG.md)にあります。

## ライセンスと謝辞

[MIT License](LICENSE)で提供します。

[Poke-Controller](https://github.com/KawaSwitch/Poke-Controller)のKawaSwitch氏と、[Poke-Controller Modified](https://github.com/Moi-poke/Poke-Controller-Modified)のmoi_poke氏をはじめ、既存実装とスクリプト作者の皆様に感謝します。
