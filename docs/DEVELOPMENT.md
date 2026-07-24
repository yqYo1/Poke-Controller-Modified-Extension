# 本体開発ガイド

この文書は、Rust backend、TypeScript frontend、Python tooling、Nix、配布物を変更するPokeCon本体開発者向けです。

利用者がcommandを書く方法は[ユーザースクリプト開発ガイド](SCRIPT_DEVELOPMENT.md)を参照してください。

runtimeの責務境界を変更する前に[本体アーキテクチャ](ARCHITECTURE.md)を参照してください。

## Nix devShellを唯一の開発環境にする

repositoryの開発、生成、format、test、packageはすべてflakeが固定するtoolchainで実行します。

hostに入っているRust、Python、Bun、Node.jsを直接使用しません。

direnvを使用する場合はrepositoryへ移動し、初回だけ許可します。

```bash
direnv allow
```

明示的にshellへ入る場合は次を実行します。

```bash
nix develop
```

`.envrc`は`use flake`を使用します。

shell開始時にRust、Python、Bunのversionが表示されます。

CIと同じtaskは`nix run .#<task>`で実行します。

localだけに存在する依存や環境変数でtestを通さず、必要なtoolは`flake.nix`へ宣言します。

## 対象toolchainを理解する

Rust workspaceはedition 2024、Rust 1.95を対象にします。

Python workerとtoolingはCPython 3.14を対象にします。

Python codeは完全な型annotationを持たせ、PEP 695のtype parameterを使用できます。

frontendはTypeScript、Svelte 5、SvelteKitを使用します。

frontend package managerとscript runtimeはBunです。

JavaScriptである必要がない新規sourceはTypeScriptで作成します。

既存JavaScriptを変更する場合も、tool制約がなければ同じ変更でTypeScriptへ移行します。

Node.jsを直接呼ぶscriptを追加せず、`bun --bun`またはflake taskを使用します。

## repository構造から変更先を選ぶ

| path | 内容 |
|---|---|
| `rust/` | Rust workspaceの11 crate |
| `web/` | SvelteKit SPA、TypeScript、frontend test |
| `python/pokecon/` | workerから見えるPython packageと生成stub |
| `scripts/` | quality、compatibility、release、integration tooling |
| `tests/` | Python toolingとcross-language fixture |
| `rust/pokecon-contracts/registry/` | 設定、event、受入記録などの正準registry |
| `generated/` | registryから生成するschemaやmetadata |
| `api/` | 生成済みOpenAPI契約 |
| `docs/` | 読者別の恒久文書 |
| `compatibility/` | 固定互換source、期待値、追補記録 |
| `flake.nix` | source filter、package、devShell、全task |
| `Cargo.toml`と`Cargo.lock` | Rust workspaceと唯一のlock file |
| `web/package.json`と`web/bun.lock` | frontend依存と唯一のBun lock |

新しいtop-level directoryを追加する前に、既存の責務へ配置できない理由を確認します。

temporary artifact、venv、tool cacheをsource treeへcommitしません。

## source filterへ新しいsourceを含める

Nix buildはrepository全体を無条件にstoreへコピーせず、`flake.nix`のsource filterを使用します。

新しい拡張子を追加した場合は、実装fileだけでなくsource filterも同じ変更で更新します。

特に`.svelte`、`.ts`、`.tsx`、`.json`、`.svg`、`.md`の追加漏れを確認します。

source filterにfrontend fileがない場合、local Bun buildは成功してもNix buildではSvelteKitのfallback error pageだけが生成されることがあります。

HTTP statusが200なのに画面が`404 Not Found`になる場合は、`result/web/dist`とlocal `web/dist`のchunk数を比較します。

filterと禁止sourceの検査は次で実行します。

```bash
nix run .#source-filter-check
nix run .#source-guard -- spa --require-applicable
```

CI専用のcopyやfallbackを追加せず、Nix buildがproduction sourceを正しく含むように直します。

## Rustを変更する

crateは[本体アーキテクチャ](ARCHITECTURE.md#rust-crateの責務を分ける)の所有境界に従います。

workspace全体は`unsafe_code = "forbid"`です。

Clippyの`all`と`pedantic`をwarningではなくerrorとして扱います。

public APIにはerror、lifetime、thread、blockingの意味が分かるdocumentationを付けます。

native resourceを追加する場合は、owner、cancel、deadline、shutdown順、failure後のlifetimeを設計します。

async executor上でblocking I/Oを直接実行せず、専用threadまたは`spawn_blocking`との境界を明示します。

bounded queueの容量とoverflow policyをtest可能な設定またはconstantとして定義します。

変更中の素早い確認には対象packageを絞った`cargo test`をdevShell内で実行できます。

共有の完了gateはflake taskを使用します。

```bash
nix run .#clippy
nix run .#cargo-test
nix run .#build-rust
```

`Cargo.lock`はworkspace rootの一つだけを使用します。

crate内に別のlock fileを作りません。

依存を変更した場合はNix shell内でroot lockを更新し、意図しないtransitive差分がないか確認します。

## Python 3.14 codeを変更する

`python/`、`scripts/`、`tests/`のPythonは3.14としてparse、format、type checkします。

公開function、private helper、collection、callbackを含めて型を省略しません。

型だけの互換目的で古いPython syntaxへ戻しません。

generated `.pyi`は手で修正せず、正準contractまたはbindingを変更します。

Pythonの個別gateは次のとおりです。

```bash
nix run .#ruff-format
nix run .#ruff-check
nix run .#basedpyright
nix run .#test
```

Rust bindingをlocal interpreterへ導入して調査する場合は次を使用します。

```bash
nix run .#maturin-develop
```

この操作もdevShellが固定するPythonとPyO3を使用します。

user package同期のproduction経路をsystem `pip`で置き換えません。

managed runtimeはbundled wheelhouseとuvを使用し、追加packageは専用venvへexact syncします。

## TypeScriptとSvelteを変更する

frontend commandは`web/package.json`へ定義し、Bun lockを固定します。

dependency取得では`--frozen-lockfile`を使用し、install scriptは必要性をreviewします。

通常のfrontend gateは次の一つでlint、Svelte type check、Vitest、production buildを実行します。

```bash
nix run .#web-check
```

dev serverが必要な場合もNix devShell内でBunを使用します。

```bash
bun install --cwd web --frozen-lockfile --ignore-scripts
bun run --cwd web --bun dev
```

RESTとWebSocketの型を手書きで複製せず、OpenAPI生成物をimportします。

server stateのrevision gapをfrontendだけで補間せず、完全snapshotを再取得します。

browser raw keyboard codeをplatform表示名へ早期変換せず、入力bindingの正準表現を維持します。

新しい`.svelte`やasset拡張子を追加した場合はNix source filterを必ず更新します。

## 生成物を正準入力から更新する

設定、動的event、stub、schemaを変更した場合はcontract generatorを実行します。

```bash
nix run .#generate-contracts
nix run .#contract-check
```

HTTP型またはendpointを変更した場合はOpenAPIとTypeScript型を更新します。

```bash
nix run .#generate-api-types
nix run .#contract-check
```

`contract-check`の`--check`相当が差分を報告した場合は、生成fileを直接合わせず正準入力とgeneratorを調べます。

生成後はruntime側だけでなく、対象読者の文書も同じ変更で更新します。

設定field変更は[設定リファレンス](SETTINGS.md)へ反映します。

Python公開surface変更は[ユーザースクリプト開発ガイド](SCRIPT_DEVELOPMENT.md)へ反映します。

serial codec変更は[周辺機器開発ガイド](PERIPHERAL_DEVELOPMENT.md)のtest vectorを更新します。

HTTP契約変更は[HTTP APIとリアルタイム通信](HTTP_API.md)へ反映します。

## 変更種別に応じたtestを選ぶ

すべての変更は最終的に`nix run .#check`を通しますが、原因を短く特定するために対象gateを先に実行します。

| 変更 | 先に実行するgate |
|---|---|
| Rust logic | `nix run .#clippy`、`nix run .#cargo-test` |
| Python tooling | `nix run .#ruff-check`、`nix run .#basedpyright`、`nix run .#test` |
| frontend | `nix run .#web-check` |
| registryや公開型 | `nix run .#contract-check` |
| user-script互換層 | `nix run .#compatibility` |
| serialやcamera native I/O | `nix run .#virtual-io-check` |
| Markdown | `nix run .#markdownlint-check`、`nix run .#textlint-check`、`nix run .#typos-check` |
| GitHub Actions | `nix run .#actionlint` |
| release metadata | `nix run .#release-check` |
| desktop integration | `nix run .#tauri-check` |

最終gateは次です。

```bash
nix fmt
nix run .#check
```

`nix fmt`はcommit前に必ず実行します。

CIは`nix fmt -- --ci`相当のcheckを行うため、format差分を残しません。

## 仮想I/Oでproduction経路を検証する

Linuxのvirtual I/O taskはkernel PTYとV4L2 loopbackを使います。

```bash
nix run .#virtual-io-check
```

既存のV4L2 loopback indexを使う場合は引数で指定します。

```bash
nix run .#virtual-io-check -- 42
```

PTY試験はmasterとslave間のpartial read、partial write、native serial backend、codec経路を検査します。

V4L2試験はffmpegの既知patternを入力し、列挙、format交渉、BGR decode、再設定を検査します。

task自身がmoduleをloadした場合だけ終了時にunloadします。

実行kernelに対応する`v4l2loopback`と、必要時にmoduleをloadできる権限が必要です。

virtual I/Oのためにproduction codeへCI専用backendや分岐を追加しません。

OS native APIへfixtureを接続し、実機と同じRust経路を通します。

PTYはUSB抜線、MCU firmware、console認識、電気的noiseを再現しません。

V4L2 loopbackはcamera固有driver、色変換、帯域、抜線を完全には再現しません。

release candidateでは[外部受入ゲート](ACCEPTANCE.md)の実機試験も実施します。

## user-script互換性を検証する

固定互換corpusは旧sourceを改変せず、import、discovery、class、実行surfaceの差を検出します。

```bash
nix run .#compatibility
```

inventoryだけを確認する場合は次を使用します。

```bash
nix run .#compatibility-inventory
```

upstream追従のrollは日常のlocal testではなくreview用です。

```bash
nix run .#compatibility-roll
```

互換失敗をcorpus sourceの書換えやtest時だけのmonkey patchで隠しません。

production互換層を修正するか、実機が必要な能力として明示的に隔離します。

外部作者が単体配布する`bridge_functions`をrepositoryへ再同梱しません。

必要な互換試験では作者配布物を利用者Data rootへ別途配置し、licenseとsource identityを記録します。

## 配布物を検証する

Rust workspaceとPyO3 wheelのbuildは次を使用します。

```bash
nix run .#build
```

Linux desktop bundleは次を使用します。

```bash
nix run .#tauri-build
```

package単体とclean container installは別のgateです。

```bash
nix run .#package-smoke -- /absolute/path/to/package.deb
nix run .#package-install-smoke -- /absolute/path/to/package.deb
```

release artifactはbundled worker、uv、Python runtime、wheelhouse、`web/dist`のmanifestとhashを含みます。

実行fileだけを取り出して成功扱いにしません。

reproducible buildのtimestamp、path remap、ELF normalizationをlocal都合で無効化しません。

release判定は`nix run .#release-check`と[外部受入ゲート](ACCEPTANCE.md)の両方を必要とします。

## 文書を変更する

文書は[文書案内](README.md)の読者境界に従います。

異なる読者が安全な操作を完結するための短い重複は許容します。

同じ詳細表やprotocol定義を複数文書へ複製せず、正本へlinkします。

一文を一行に置き、段落は空行で分けます。

code、設定、log、wire bytesはfenced code blockへ置きます。

用語は最初に定義し、対象読者に不要な内部識別子を持ち込みません。

文書gateは次です。

```bash
nix run .#markdownlint-check
nix run .#textlint-check
nix run .#typos-check
```

新しい文書を追加した場合は`docs/README.md`の読者別入口と責務表も更新します。

一時的な作業計画や実装中の仮定を恒久文書から参照しません。

## commitとCIを完了させる

作業前に`git status`を確認し、利用者の未commit変更を上書きしません。

機械的生成と手書き修正をreview可能なまとまりに保ちます。

commit前にformat、対象gate、最終`check`を実行します。

commitはprojectの署名方針に従って署名します。

push後はlocal成功だけで完了とせず、対象branchのGitHub Actionsを最後まで監視します。

repository付属scriptはbranchとtimeoutを指定できます。

```bash
scripts/ci-watch.sh <branch> 3600
```

CIが失敗した場合は該当workflowのlogを確認し、同じNix taskで再現して修正します。

環境差を理由にCIだけskipする条件を追加せず、flake、source filter、fixture、platform gateのどこが不一致かを調査します。
