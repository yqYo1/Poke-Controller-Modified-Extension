# 本体開発ガイド

この文書は、Rust backend、TypeScript frontend、Python tooling、Nix、配布物を変更するPokeCon本体開発者向けです。

利用者がcommandを書く方法は[ユーザースクリプト開発ガイド](SCRIPT_DEVELOPMENT.md)を参照してください。

runtimeの責務境界を変更する前に[本体アーキテクチャ](ARCHITECTURE.md)を参照してください。

## projectのNix devShellを開発環境にする

repositoryの開発、生成、format、test、packageはすべてflakeが固定するtoolchainで実行します。

hostに入っているRust、Python、Bun、Node.jsを直接使用しません。

repositoryへ入ると`.envrc`の`use flake`が既定のtool-only devShellを読み込みます。新しいworktreeでは`direnv allow`を一度実行し、自動読込みを使わない場合は`nix develop`で入ります。このshellは開発toolだけを提供し、入るだけで製品をbuildしません。

`.direnv/`はdirenvが生成するlocal stateであり、Git、Nix source boundary、formatterの入力へ含めません。sourceへ必要な設定やtoolを追加する場合は、`.envrc`ではなくflakeの正本と専用taskへ反映します。

CIと同じ完了gateは`nix run .#<task>`または`nix flake check`で、formatは`nix fmt`、callerのworktreeへ書き込む対話操作は専用appで実行します。

代表的な入口は次のとおりです。

```bash
direnv allow
# または: nix develop
nix run .#cargo -- test --locked -p pokecon
nix run .#web-dev
nix run .#hooks-install
nix run .#editor -- --print
nix run .#editor-smoke
```

`cargo`と`web-dev`は明示的にcallerのworktreeを対象とします。読取り専用の完了gateはNix store上の正準sourceまたは隔離した一時copyを対象とします。対話用の`cargo`だけがcallerの`target/nix-tasks`を排他lock付きで再利用し、通常のbuild、test、Clippyではimmutableなoffline vendorを使用します。依存解決を変更する明示的な`cargo update`だけがcallerのCargo homeとregistryを使用します。長時間起動する`editor`は別の`target/nix-editor`を使用します。Rust完了gateは新しい入力ごとに空の一時Cargo targetから一度検証し、成功をNix storeへ保存します。同じ入力の再実行はその結果を再利用し、callerのCargo cacheは読取り、検査、変更しません。native WebRTC testはICE candidateをloopback addressだけに制限し、Darwin sandboxでも外部networkを許可せずlocalhostだけを使用します。

すべてのtask appはflakeが列挙したtoolだけの`PATH`を使用します。`editor`からhost editorを起動する場合は、`nix run .#editor -- /absolute/path/to/editor`のようにabsolute executable pathを指定します。

読取り専用gateとgeneratorはbuildへ影響するambient環境変数を除去し、一時`HOME`、一時`CARGO_HOME`、必要な場合だけ一時Cargo target、専用XDG directoryを使用します。Cargo dependencyはflake packageと同じimmutableなNix vendor directoryだけからofflineで解決し、ambientなCargo config、registry source、Git checkout、network cache、以前の実行が残したartifactを使用しません。`cargo`と`editor`は対話的な調査入口であるため、`RUST_LOG`や明示的なcompiler optionなどcallerが渡す非tool環境を意図的に継承します。この対話結果は完了gateの証跡にはせず、toolの`PATH`、Rust／Python toolchain、Cargo targetはappが固定します。

`hooks-install`は新しく作成した各worktreeで実行します。再実行するとNix生成configを収束させ、欠落、実行権限を失った、または認識済み生成形式のhookを再導入します。`.pre-commit-config.yaml`に通常fileまたは予期しないsymlinkがある場合、あるいはhook pathにcustom file、symlink、または認識できない内容がある場合は、退避や置換をせず失敗します。生成hookはprivileged Bash、固定`PATH`、一時`HOME`と最小Git contextを使い、`SKIP`などのambient設定では検査を迂回できません。

pre-commitのClippyはRust、Cargo、flake、toolchain入力をstageしたcommitだけを対象にし、固定toolchain、immutableなoffline vendor、排他lockを保ったまま`cargo` appの`target/nix-tasks`を再利用します。完了判定の`rust-ci-core`は隔離sourceと新規入力ごとの空のCargo targetを使うため、local feedbackのcacheは完了gateの証跡へ混入しません。成功した同一入力はNix storeから再利用します。

`nix fmt`と`nix run .#fmt`は同じ保護されたformatterです。callerのworktreeを対象にしつつ、shell startup、formatter設定、`HOME`、cacheなどのambient状態を除去してから固定treefmtを実行します。

localだけに存在する依存や環境変数でtestを通さず、必要なtoolは`flake.nix`へ宣言します。

`virtual-io-check`のhost privilege wrapperだけは例外です。setuid executableはimmutableなNix storeへ固定できないため、通常のtoolはflakeで固定したまま、権限昇格だけを下記の明示的なhost契約へ分離します。

既定devShellはtoolだけに保ち、追加の常駐環境や長時間処理が必要な用途には、必要なtoolと環境だけを持つ専用appを追加します。

repositoryの状態確認、commit、worktree作成にはGit／ghqをorchestration入口として使用できます。Gitから起動するproject検査は、host toolchainではなく上記flake outputを呼びます。

このlocal開発規則はNixを利用できるhostを対象にします。native WindowsのCI、package、releaseは明示的なplatform gateであり、各workflowが固定するWindows toolchainを使用します。

## 対象toolchainを理解する

Rust workspaceはedition 2024、Rust 1.95を対象にします。

Python workerとtoolingはCPython 3.14を対象にします。

Python codeは完全な型annotationを持たせ、PEP 695のtype parameterを使用できます。

frontendはTypeScript、Svelte 5、SvelteKitを使用します。

frontend package managerとscript runtimeはBunです。

JavaScriptである必要がない新規sourceはTypeScriptで作成します。

既存JavaScriptを変更する場合も、tool制約がなければ同じ変更でTypeScriptへ移行します。

Node.jsを直接呼ぶscriptを追加しません。script内部ではNixが固定したBunを`bun --bun`で使用し、開発者は対応するflake taskだけを入口にします。

## repository構造から変更先を選ぶ

| path | 内容 |
|---|---|
| `rust/` | Rust workspace |
| `web/` | SvelteKit SPA、TypeScript、frontend test |
| `python/pokecon/` | 純Python互換メタデータと生成stub |
| `scripts/` | quality、compatibility、release、integration tooling |
| `tests/` | Python toolingとcross-language fixture |
| `rust/pokecon/registry/` | 設定、event、受入記録などの正準registry |
| `generated/` | registryから生成するschemaやmetadata |
| `api/` | 生成済みOpenAPI契約 |
| `docs/` | 読者別の恒久文書 |
| `compatibility/` | 固定互換source、期待値、追補記録 |
| `flake.nix` | source filter、package、完了gate、対話用flake app |
| `Cargo.toml`と`Cargo.lock` | Rust workspaceと唯一のlock file |
| `web/package.json`と`web/bun.lock` | frontend依存と唯一のBun lock |

新しいtop-level directoryを追加する前に、既存の責務へ配置できない理由を確認します。

temporary artifact、venv、tool cacheをsource treeへcommitしません。

## source filterへ新しいsourceを含める

Nix buildはrepository全体を無条件にstoreへコピーせず、`flake.nix`のsource filterを使用します。

新しい拡張子を追加した場合は、実装fileだけでなくsource filterも同じ変更で更新します。

特に`.svelte`、`.ts`、`.tsx`、`.json`、`.svg`、`.md`の追加漏れを確認します。

source filterにfrontend fileがない場合、callerのsource treeには実装が存在してもNix buildではSvelteKitのfallback error pageだけが生成されることがあります。

HTTP statusが200なのに画面が`404 Not Found`になる場合は、`nix build .#web`の成果物にfallback以外のexpected chunkがあることを確認し、独立した`nix run .#web-check`も実行します。

filterと禁止sourceの検査は次で実行します。

```bash
nix run .#source-filter-check
nix run .#source-guard -- spa --require-applicable
```

CI専用のcopyやfallbackを追加せず、Nix buildがproduction sourceを正しく含むように直します。

## Rustを変更する

crateは[本体アーキテクチャ](ARCHITECTURE.md#rust-crateの責務を分ける)の所有境界に従います。

workspaceの既定は`unsafe_code = "forbid"`です。本体crateだけは`deny`を使用し、camera共有メモリーmappingの監査済み境界に限って局所的に`allow`します。

Clippyの`all`と`pedantic`をwarningではなくerrorとして扱います。

public APIにはerror、lifetime、thread、blockingの意味が分かるdocumentationを付けます。

native resourceを追加する場合は、owner、cancel、deadline、shutdown順、failure後のlifetimeを設計します。

async executor上でblocking I/Oを直接実行せず、専用threadまたは`spawn_blocking`との境界を明示します。

bounded queueの容量とoverflow policyをtest可能な設定またはconstantとして定義します。

変更中の素早い確認には、callerのworktreeを対象にする`cargo` appでpackageやtestを絞ります。

```bash
nix run .#cargo -- test --locked -p pokecon
```

共有の完了gateはflake taskを使用します。

```bash
nix run .#clippy
nix run .#cargo-test
nix run .#build-rust
```

`Cargo.lock`はworkspace rootの一つだけを使用します。

crate内に別のlock fileを作りません。

依存を変更した場合は`nix run .#cargo -- update ...`でroot lockを更新し、意図しないtransitive差分がないか確認します。

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

user package同期のproduction経路をsystem `pip`で置き換えません。

managed runtimeはbundled wheelhouseとuvを使用し、追加packageは専用venvへexact syncします。

## TypeScriptとSvelteを変更する

frontend commandは`web/package.json`へ定義し、Bun lockを固定します。

dependency取得では`--frozen-lockfile`を使用し、install scriptは必要性をreviewします。

通常のfrontend gateは次の一つでlint、Svelte type check、Vitest、production buildを実行します。

```bash
nix run .#web-check
```

dev serverには、固定Bunとfrozen lock fileを使ってcallerの`web/`を監視する専用appを使用します。

```bash
nix run .#web-dev
```

完了gateはnetwork installを行いません。`web-check`、`check`、`tauri-build`、API型生成はlock fileから作成したhash固定のNix dependency treeを使用します。

editor用language serverの統合は、host `PATH`へ依存せず次で検証します。

```bash
nix run .#editor-smoke
```

このsmokeは隔離したfixtureに対してRust、Python、TypeScript、Svelteの各serverを実際にinitializeし、documentをopenし、意図した型errorのdiagnostic code、message、rangeを確認してからshutdownします。version表示だけの検査ではありません。

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

既存deviceがなくmoduleのloadが必要な場合、rootでは対象commandを直接実行し、非rootではhostのsetuid privilege wrapperをnon-interactive modeで使用します。`POKECON_SUDO`にはabsolute executable pathだけを指定できます。未指定時は`/run/wrappers/bin/sudo`、`/usr/bin/sudo`、`/bin/sudo`の順で検出し、ambient `PATH`からは探索しません。これら以外の配置では、たとえば`POKECON_SUDO=/usr/local/bin/sudo nix run .#virtual-io-check`のように明示します。

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

Rust workspaceの配布用buildは次を使用します。

```bash
nix run .#build-rust
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

container install gateはflake固定Docker clientを使い、外部Docker daemonだけを明示的なhost境界とします。ambientから継承するDocker設定は`DOCKER_HOST`、`DOCKER_CONTEXT`、`DOCKER_CERT_PATH`、`DOCKER_TLS_VERIFY`、`DOCKER_CONFIG`に限定します。`DOCKER_CONFIG`と`DOCKER_CERT_PATH`を指定する場合は、読取りと探索が可能なabsolute directory pathでなければ実行前に拒否します。

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

push後はlocal成功だけで完了とせず、対象branchのremote HEAD SHAに対する2つの必須context（`Normal CI Required`と`Package CI Required`）を安定窓の完了まで監視します。必須contextが対象SHAで`completed`/`success`となり、かつ必須contextの集合が安定窓の間変化しなければ完了します。必須context以外の任意のcheckは、必須contextが完了すれば無視します。必須contextの欠落、古いSHA、実行中（queued/in_progress）、skipped、neutral、cancelled、timed_out、action_required、failureはいずれも成功としません。

CI監視appはbranchと任意のtimeoutをscriptへ渡し、必要なcommandをflakeから提供します。既定timeoutと安定窓は`nix run .#ci-watch -- --help`が表示するscript定義を正準とします。終了statusは成功が0、必須contextの完了失敗が1、timeoutが124、usageが2で区別します。改ページや同一SHAの重複した歴史的check-runは最新idを採用しfail-closedに扱います。

Normal CIのtiming gateは、同じ`change_kind`のcompleted runから検証済み`timing-report.json`を集め、10サンプルのnearest-rank p95をblocking判定します。履歴不足、30日を超えるstale、malformed report、失敗jobを含む履歴、またはthreshold超過はfail-closedです。履歴不足の初期runでもcurrent reportはartifactへ保存し、次のrunで履歴を蓄積します。

```bash
nix run .#ci-watch -- "$BRANCH"
```

CIが失敗した場合は該当workflowのlogを確認し、同じNix taskで再現して修正します。

環境差を理由にCIだけskipする条件を追加せず、flake、source filter、fixture、platform gateのどこが不一致かを調査します。
