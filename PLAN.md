# アーキテクチャレビュー反映計画

## 目的と規範

- 規範: `SPECIFICATION.md`、`ARCHITECTURE_REVIEW.md`
- 対象ブランチ: `refactor/rust-core`
- 開始基準: `e20fad5a`（レビュー書更新完了時点）
- 最終状態: PokeCon を単一 Cargo パッケージへ統合し、レビュー書で確定した責務、優先順位、開発入口、CI、配布契約を実装と検証へ反映する。
- 進捗規則: 完了を証明するコマンドまたは成果物がある項目だけを `[x]` にする。部分完了は子項目だけを更新する。
- 変更規則: 構造移行と実行時挙動変更を同じコミットへ混在させない。各チェックポイントで検証してから次へ進む。
- 委譲規則: 実装委譲は不可分な1タスクずつ行い、primary agentが差分と対応gateを確認し、当該タスクだけを新規文脈のSolへレビュー依頼してから次のタスクへ進む。節全体を一度に委譲しない。
- 実施順序: レビュー書の原順序は構造移行 → CI 再構成 → devShell 移行 → 実行時挙動変更である。2026-07-30の利用者指示「まずdevShell廃止から」に基づき、常駐するambient PATH／tool／環境変数依存を先に断つ非挙動の開発基盤変更としてフェーズ1だけを承認済み例外で先行する。完了後は構造（フェーズ2） → CI（フェーズ3） → 挙動（フェーズ4）の原順序へ戻る。
- コミット規則: 構造移行の2.1、2.2、2.3a、2.3b、2.3c、2.3d、2.4、2.5、2.6、2.7をそれぞれ独立したコミット境界とする。フェーズ4は4.1主経路と優先順位、4.2手動介入と入力調停、4.3 profile切替、4.4動的設定の候補世代切替、4.5通知隔離を独立した挙動変更checkpointとする。各checkpointの共通完了gateが失敗した状態で後続checkpointへ進まない。

## 現在地

- [x] `ARCHITECTURE_REVIEW.md` の確定方針、移行順序、受入条件を一対一の実装要件として抽出した（2026-07-30 Sol意味監査承認、129要件）。
- [x] 開始時の作業ツリーがcleanで、HEADが`e20fad5a`、`origin/refactor-rust-core`との差が0/0であることを変更前の`git status --short --branch`と`git rev-list --left-right --count`で記録した（2026-07-30 JST）。
- [x] 旧実装の不在を前提にした旧 `PLAN.md` を廃止し、本チェックリストへ置き換えた。
- [x] フェーズ 1「direnv／既定 devShell 廃止」を完了した（実装`23149e3`、受入`1e0836b`、再現性修正`7a01da0`。最終GitHub Actions 9/9 success）。
- [ ] 全フェーズ完了後の要件別監査を通過する。

## レビュー要件トレーサビリティ

- [x] `ARCHITECTURE_REVIEW.md` §10.4、§10.8、§10.9、§10.10、§10.11、§11、§13.1の各要件を、それぞれ一度だけ現れる個別ID付きcheckboxと同じ行の予定証跡へ展開した（証跡: 2026-07-30の許可済みVCS入口`git grep`機械監査で129件、群別`2 / 5 / 9 / 24 / 10 / 50 / 29`、重複0、予定証跡欠落0。コンテキストを切ったSol意味監査で順序、checkpoint、配置、意味対応を承認）。

IDはレビュー書の出現順に付与し、範囲IDや集約IDでの完了判定は行わない。各checkboxの完了時は「予定証跡」を実行結果、生成物、CI run、またはGitHub設定の読み戻し結果で置き換える。

予定証跡のproject操作は、各行に別のNix outputを明記しない限り`nix run .#check`を入口とし、専用操作は`nix run .#<task>`、`nix build .#pokecon`、`nix fmt`、`nix flake check`のいずれかを唯一の実行入口とする。flake output inventoryの読取りには`nix flake show`を使う。「test」、「report」、「log」、「matrix」はそのNix taskまたはCI workflowが生成する成果物を指し、hostの言語runtime、compiler、package manager、品質toolを直接起動しない。Git／GitHubの読取り、worktree操作、CI runの読み戻しはVCS／外部状態証跡として例外とし、native Windows CI／package／releaseだけはworkflowが固定するtoolchainを入口とする。

## 共通完了ゲート

適用対象はフェーズ1、2.1、2.2、2.3a、2.3b、2.3c、2.3d、2.4、2.5、2.6、2.7、フェーズ3、4.1、4.2、4.3、4.4、4.5の各checkpointとする。各checkpointで次をすべて実行し、該当しないgateは対象外となる具体的理由と代替証跡をcheckpoint記録に残す。失敗または理由のない未実行がある状態で後続checkpointへ進まない。

- [ ] **AR-13.1-01** 2.1、2.2、2.3a、2.3b、2.3c、2.3d、2.4、2.5、2.6、2.7の各構造移行を独立commitにし、当該段階のgate失敗時は後続のcrate／module移行を開始しない（予定証跡: VCSが読み戻す各commit SHAと順序、対応するNix gate log）。
- [ ] **AR-11-41** workspace全体の共通完了gateを全適用checkpointで実行する（予定証跡: checkpoint／commit SHAごとの下記Nix command終了コード、対象外理由／代替証跡、CI run URL）。
- [ ] **AR-13.1-03** `nix run .#contract-check`を通す（予定証跡: 各適用checkpointの`nix run .#contract-check` log）。
- [ ] **AR-13.1-04** `nix run .#cargo-test`を通す（予定証跡: 各適用checkpointの`nix run .#cargo-test` log）。
- [ ] **AR-13.1-05** `nix run .#clippy`を通す（予定証跡: 各適用checkpointの`nix run .#clippy` log）。
- [ ] **AR-13.1-06** `nix run .#build-rust`を通す（予定証跡: 各適用checkpointの`nix run .#build-rust` logとNix build成果物一覧）。
- [ ] **AR-13.1-10** `nix run .#compatibility`で固定互換性基準に対するPython commandを通す（予定証跡: `nix run .#compatibility`が出力するcorpus SHA付きreport）。
- [ ] `nix run .#web-check`
- [ ] `nix fmt`
- [ ] `nix fmt -- --ci`
- [ ] `nix flake check --no-build`
- [ ] `nix run .#editor-smoke`
- [ ] `nix run .#check`
- [ ] **AR-13.1-07** `pokecon --help`と`pokecon-worker --help`の公開CLI差分を検査する（予定証跡: `nix build .#pokecon --print-out-paths --no-link`が返すstore内binaryの`--help`をNix integration taskが正規化した移行前baseline差分）。
- [ ] **AR-13.1-08** Web modeとTauri modeの起動検査を通す（予定証跡: `nix build .#pokecon --print-out-paths --no-link`のstore内binaryを使う対応Nix integration taskの両mode health endpointと起動／停止log）。
- [ ] **AR-13.1-09** `pokecon-worker --kind script`と`--kind dynamic`の起動、IPC、協調停止、強制終了を検査する（予定証跡: `nix run .#cargo-test`がNix build済みworkerを使って出力するrole別integration／fault report）。
- [ ] push 後に `nix run .#ci-watch -- <branch> <timeout>` で GitHub Actions を完了まで監視し、失敗を解消する。

## レビュー引渡し情報の横断チェック

- [ ] **AR-11-24** 製品全体で優先する設計原則をmoduleと実行時機構へ対応付ける（予定証跡: 設計原則→module／queue／lock／task／threadの対応表とarchitecture test）。
- [ ] **AR-11-25** 正準controller状態とcamera／serial handle等のhardware resource所有者を一意にする（予定証跡: server socket、worker世代、設定revisionも含むownership tableと重複所有検査）。
- [ ] **AR-11-26** processごとの責務、寿命、再生成規則を確定する（予定証跡: Rustメイン／script worker／dynamic workerのstate-transition表とfault test）。
- [ ] **AR-11-27** UI、HTTP、IPC、user script、dynamic configの公開境界を確定する（予定証跡: 公開schema／API一覧とnative object境界越え禁止test）。
- [ ] **AR-11-28** 設定、profile、commandと互換corpusのlifecycleを確定する（予定証跡: `nix run .#contract-check`が出力するsettings／profile／commandの生成、切替、失敗、破棄の状態表と、`nix run .#compatibility`が証明する固定baseline不変、候補監視 → full-chain再評価 → 成功時のみappend-only昇格、失敗時の従前保証維持report）。
- [ ] **AR-11-29** 障害、timeout、rollback、終了処理の責任分担を確定する（予定証跡: 責任module／最終状態表と強制停止／resource解放／shutdown fault test）。
- [ ] **AR-11-30** Windows、Linux、Nix、non-Nixの配布形態を確定する（予定証跡: OS／build／package／runtime別の成果物matrixとPackage／Release CI）。
- [ ] **AR-11-33** 各内部moduleが所有する責務を確定する（予定証跡: module ownership manifestとarchitecture test）。
- [ ] **AR-11-34** 各内部moduleが所有しない責務を確定する（予定証跡: negative responsibility manifestとforbidden ownership test）。
- [ ] **AR-11-35** 許可する内部依存方向を確定する（予定証跡: dependency rule manifestとCargo／source dependency check）。
- [ ] **AR-11-36** 禁止する内部依存方向を確定する（予定証跡: forbidden-edge fixture付きdependency check）。
- [ ] **AR-11-39** 各移行前に移動／削除する型、trait、moduleをinventory化する（予定証跡: `nix run .#check`の段階別inventoryと許可済みVCS入口`git grep`の移行後残存参照log）。
- [ ] **AR-11-40** 移行段階、監督／資源service実行ファイル、共通workerごとの検証commandを確定する（予定証跡: 2.1、2.2、2.3a、2.3b、2.3c、2.3d、2.4、2.5、2.6、2.7ごとのNix command／期待結果表と実行log）。
- [ ] **AR-11-10** 主経路のlatency、throughput、停止、復旧の受入条件を定義する（予定証跡: 測定fixture、閾値、移行前baseline、移行後report）。
- [ ] **AR-11-37** 別processと同一processの境界を確定する（予定証跡: process／module deployment diagramとIPC境界test）。
- [ ] **AR-11-38** 個別成果物と配布方法を確定する（予定証跡: artifact manifestとOS別clean-install report）。

## フェーズ 1 — direnv／既定 devShell を廃止して Nix app へ統一

### 実装

- [x] **AR-10.11-APP1** callerのworktreeを直接対象にする`nix run .#cargo -- <subcommand> ...`を追加する（証跡: 2026-07-30の新規worktreeでmetadata、`pokecon-contracts`の1 test、0 package更新のlock操作、desktop checkが成功し、排他lockとtargetがcallerの`target/nix-tasks`を指した。Cargo／Rustは1.97.1、Pythonは3.14.6）。
  - [x] 固定 Rust toolchain、Python 3.14、uv、desktop native dependency を提供する。
  - [x] `PYO3_PYTHON`、build 用 Python／uv、script site-packages、bindgen、pkg-config を既存完了ゲートと一致させる。
  - [x] 対話用`cargo`だけがcaller側の共有`target/nix-tasks`と排他lockを使用する。
  - [x] package指定、個別test、`nix run .#cargo -- metadata --locked --no-deps`、lock file更新を引数透過で実行できる。
- [x] **AR-10.11-APP2** callerの`web/`を直接対象にする`nix run .#web-dev`を追加する（証跡: Bun 1.3.13、frozen lock、249 packageの導入、`src/routes/+page.svelte`の`hmr update`を観測。前後の`git status --short --untracked-files=all`は空で、ignored pathは`web/node_modules/`と`web/.svelte-kit/`だけだった）。
  - [x] `web/package.json` が固定する Bun を使用する。
  - [x] frozen lock file で依存を準備する。
  - [x] hot reload を caller の編集へ追従させる。
  - [x] 終了後に意図しない追跡対象差分を残さない。
- [x] **AR-10.11-APP3** `nix run .#hooks-install`を追加し、現在のworktreeへ`git-hooks.nix`生成hookを明示的に導入する（証跡: 新規worktreeからGit common hookへ導入し、固定`PATH`、一時`HOME`、`env -i`、hardening markerを読取り。Rust／Python／Markdownをstageしたtest commit `fb4dbeb`で全8 hookが`SKIP`、`BASH_ENV`、`ENV`のpoison下でも実行され成功した）。
- [x] **AR-10.11-APP4** `nix run .#editor`を追加し、host toolchainやdirenvなしでRust、Python、TypeScript／Svelteのlanguage serverを利用できるようにする（証跡: `--print`が5個のNix store executableを返し、`editor-smoke`が4言語すべてでinitialize、didOpen、documentSymbol、期待diagnostic、shutdownを成功させた）。
- [x] `nix run .#ci-watch` を追加し、CI監視に必要なGitHub CLI等をhost環境から排除する（証跡: 2026-07-30のhostile環境／subdirectoryからの`nix run .#ci-watch -- --help`成功、固定`gh`／`git`／`jq` path）。
- [x] `ci-watch` の暫定既定期限を現行critical pathの実測最大値 + 30% 以上へ延長し、正常CIを期限切れ扱いしない（証跡: 既定1200秒、settlement 120秒、最小指定130秒のhelp／境界test）。
- [x] `nix run .#workspace-lock-check` を追加し、pre-commitのlock検査をambientなCargo／GitとcallerのCargo cacheから排除する（証跡: 固定Nix appの生成、実行ごとの一時Cargo target、`nix flake check --no-build`のapp評価成功）。
- [x] pre-commitのworkspace lock hookを`workspace-lock-check` app経由へ変更する（証跡: 生成済みpre-commit設定のentry読み戻し）。
- [x] 既存`maturin-develop`appをambient venv、network、host configに依存しない専用`target/maturin-venv`へ移行する。productionのtracked `pyproject.toml`とwheel／sdist契約は変更せず、appの隔離source snapshot内だけでMaturin canonical mixed layoutへ補正し、実行ごとの一時Cargo targetで`pokecon/**`のwheelをoffline buildし、専用の永続venv lock下でinstallする（証跡: 2026-07-30のtracked `pyproject.toml`／lock無差分、fresh／同一venv再実行のhostile offline build成功、wheel payload／source byte照合、venv内`pokecon._native` import成功、`.pth`／想定外distribution／破損／symlink／不正RECORD保持negative test、`nix run .#test` 56件成功）。
- [x] **AR-10.11-RUN1** 書込み、watch、hot reload、対象限定testを行うappは一時copyでなくcaller worktreeを対象にする（証跡: 新規worktreeの絶対pathをCargo metadata／target lock、Viteのfile-change／HMR log、hook config／common hook pathからそれぞれ読取った）。
- [x] **AR-10.11-RUN2** 読取り専用完了gateはNix storeの正準sourceまたは隔離した一時copyと、実行ごとの一時Cargo targetを維持する（証跡: callerだけの未追跡`compile_error!`とambient dummy tool／関連変数のpoison下で`contract-check`が成功。caller cacheの`libserde` pathへ置いたpoisonのSHA-256はgate前後とも`f0e766b483a7c4b0631137d317797efe9a9cdd7fd375433b92679ae01202ca6f`で、gateは別の`/tmp/pokecon-rust-gate-home.../cargo-target`を使用した）。
- [x] **AR-10.11-RUN3** `devShells.default`、`.envrc`、正規手順としてのdirenv／`nix develop`を削除する（証跡: `devShells`は4 systemすべて空、app一覧は4 systemで同一の44件、`.envrc`はcommit `23149e3`で削除済み）。
- [x] **AR-10.11-RUN4** `AGENTS.md`、`SPECIFICATION.md`、`PLAN.md`、`README.md`、`docs/DEVELOPMENT.md`、`docs/TROUBLESHOOTING.md`をproject flake output唯一の入口へ更新する（証跡: PLAN commit `93f8b6f`と実装commit `23149e3`の6文書diff、tracked command検索で正規手順がflake outputだけであることを確認）。
- [x] **AR-10.11-RUN5** このworktreeとphase 1以後に移行を検証する全worktreeそれぞれで、非追跡`.direnv/`cacheが存在する場合は対象pathを記録して削除し、不在を確認する（証跡: branch、受入、baseline package比較の3 worktreeを`git worktree list --porcelain`で特定し、Nixの`builtins.pathExists`で`.direnv`がすべてfalseであることを確認。disposableな受入／baseline worktreeは検証後に削除した）。
- [x] **AR-10.11-RUN6** 新しい対話用途は既定devShellを復活させず、用途と環境を限定したappとして追加する規則を文書化する（証跡: `docs/DEVELOPMENT.md`と`AGENTS.md`の規則、4 systemの空`devShells`評価）。
- [x] **AR-11-48** direnv、`.envrc`、既定devShellを削除し、flake appを唯一の開発入口にする（証跡: detached clean worktreeを作成し、direnv／`nix develop`なしでformat、4つの対話app、個別gate、aggregate checkまで成功）。
- [x] **AR-13.1-22** Git管理対象から`.envrc`、direnv、`nix develop`、devShell参照を検索し、履歴説明を除いて正規手順に残っていないことを確認する（証跡: tracked-file検索の残存4件は`AGENTS.md`、`README.md`、`SPECIFICATION.md`、`docs/DEVELOPMENT.md`の禁止または移行説明だけだった）。

### 受入

- [x] **AR-13.1-17** 新しいworktreeでdirenvまたは`nix develop`を使わず、flake appだけから開発を開始できる（証跡: commit `23149e3`から作成したdetached worktreeで`nix fmt -- --ci`、Cargo metadata、aggregate `nix run .#check`まで成功）。
- [x] **AR-11-49** callerのworktreeを対象とするCargo、frontend dev server、hook導入、editor連携appを揃える（証跡: 4 systemで同一のapp一覧を評価し、新規worktreeで`cargo`、`web-dev`、`hooks-install`、`editor`のcaller smokeに成功）。
- [x] **AR-11-50** 完了gateの隔離実行と、書込みまたはwatch appのcaller-worktree実行を区別し、callerへ書き込むgeneratorもbuild artifactは実行ごとの一時targetへ隔離する（証跡: Cargo／Web／hookはcaller path、editorは`target/nix-editor`、読取りgateとgeneratorはNix sourceまたは隔離copyと実行ごとの`/tmp/.../cargo-target`を使用。未追跡sourceとcache poisonのnegative testに成功）。
- [x] **AR-13.1-18** `cargo` appがcaller worktreeでpackage限定、個別test、lock file更新、metadata確認を実行でき、固定toolchain／build環境がRust完了gateと一致する（証跡: Cargo 1.97.1／Rust 1.97.1を固定し、metadata、`pokecon-contracts`の`validates_http_urls_structurally` 1件、`update --workspace --locked`、`tauri-check`が成功。Cargo.lock差分は0）。
- [x] `nix run .#cargo -- metadata --locked --no-deps` が caller の workspace を読み取る。
- [x] `nix run .#cargo -- test --locked -p <package> <test-filter>` が対象を絞って実行できる。
- [x] **AR-13.1-19** `web-dev` appが固定Bunとlock fileを使い、callerの`web/`の変更をhot reloadし、終了後に依存差分や生成物を意図せずcommit対象へ残さない（証跡: Bun 1.3.13／frozen lockで起動し、callerのSvelte変更と復元を2回のHMRとして観測。前後の`git status --short --untracked-files=all`は空、ignored pathは許可した2 directoryだけだった）。
- [x] **AR-13.1-20** `hooks-install` appを新規worktreeで一度実行し、git hookがNixで固定したpre-commit検査を実行する（証跡: common hookの絶対pathとhardening内容を読取り、hostile環境の署名付きtest commit `fb4dbeb`でlock、clippy、Markdown、Ruff、textlint、treefmt、typosがすべて成功）。
- [x] `nix run .#workspace-lock-check` と`nix run .#ci-watch -- --help`がdevShell外で動作する。
- [x] **AR-13.1-21** `editor` appまたはNixが出力するlanguage serverだけで、Rust、Python、TypeScript／Svelteの解析がhost toolchainとdirenvへ依存せず動作する（証跡: 5 executableのstore pathを読取り、host tool／関連環境変数のpoison下を含む`editor-smoke`で4言語すべての実LSP sessionが成功）。
- [x] **AR-13.1-23** 移行を検証する各worktreeそれぞれに非追跡`.direnv/`cacheが残っていない（証跡: branch、受入、baseline package比較の3 worktreeで`.direnv`の`pathExists`はすべてfalse。disposableな受入／baseline worktreeを削除し、branch worktreeだけへ正規hookを再導入した）。
- [x] **AR-13.1-24** 既存のflake taskをdevShell外から実行し、CI、format、lint、test、build、生成、互換性、packageの結果が移行前と一致する（証跡: 下記Phase 1 checkpoint表。列挙したtaskの終了コード0、生成物Git object IDとpackage NAR hashが一致）。

### Phase 1 checkpoint証跡（2026-07-30）

| 対象 | 実測結果 |
| --- | --- |
| commit境界 | PLAN `93f8b6f`、実装 `23149e3`、受入 `1e0836b`、再現性修正 `7a01da0`。全commit objectにSSH `gpgsig`を確認 |
| fresh worktree | `23149e3`から作成したdetached worktreeで`.envrc`／`.direnv`不在、format、4対話app、desktop、editor、aggregate checkが成功 |
| 完了gate | `nix flake check --no-build`、`nix fmt -- --ci`、`contract-check`、`web-check`、`clippy`、`cargo-test`、`test`、`build-rust`、`compatibility`、個別lint／文書task、`nix run .#check`が終了コード0 |
| test件数 | Web 74件、Python 60件、Rust全workspace test成功。V4L2実機test 1件だけは既定どおりignored |
| package parity | 移行前`e20fad5`は`ih2419va26ikx0xjbq19kfpz4222svcb-pokecon-0.1.0`、移行後`23149e3`は`w40flwwp2fwjsl6haxmmd133zwiqpbyc-pokecon-0.1.0`。双方のNARは`sha256-jwKvZjYVhpnE0EaLW5/yADdlw5qn+dW/CW7BVy6WNME=`、135653616 bytes |
| package再現性 | 受入`1e0836b`の[Package CI](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30521312734)が、隔離rootの8文字suffixだけ異なるDebian packageを検出した。`7a01da0`で対象rootを同長の正準pathへ限定置換し、異なる`pdM2ZYyn`／`gizMUhcL` rootからapp `e889f14870631a3654fb62180f0e49450a82a8ac9db693c05225c05e74ba097f`、worker `ee347a39127ab19f0398f0c32cf763922e461994ed50a1d2c8a1488d6b24ec2f`、content manifest `671835908938703b296837f9b2ac3a5b27430f5e8918773407f1adfc6dec9c0f`、Debian package `bd9cae12a91c32f6fd062018280bbc16039e2e81a4c2759c157e94ac29cb5b7c`（197470220 bytes）が二回とも一致 |
| package受入 | `nix run .#package-smoke`と固定Ubuntu containerの`nix run .#package-install-smoke`で3801 resources、28 wheels、新規導入、offline起動、同一version更新、再起動、削除が成功。最終[Package CI](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30528517763)でも[Debian job](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30528517763/job/90825017375)の二回build／全バイト比較と[NSIS job](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30528517763/job/90825017380)の新規導入／起動／更新／削除がsuccess |
| source parity | Cargo.lock `5e5f3542b0267e093bd5796c6652cff14e00bfc7`、web lock `bf27f310dccfb763ad1df795bdcb38d59fbd568b`、generated tree `25ad44fa0c95be2254296d2d8e2e4784095d1f52`、OpenAPI `a39cab3bee21a37d54661673be901591b8346937`、Python typings tree `e479c1cd5716fdb4c733e2f88dc94354c6986d97`が移行前後で一致 |
| isolation | caller未追跡source／ambient tool poisonをgateが観測せず、caller cache poisonのSHA-256も前後一致 |
| GitHub CI | `7a01da0adb01e3853f72cdd351192ce42b4682c4`を対象に[Basedpyright](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30528517678)、[Ruff Check](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30528517685)、[Pytest](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30528517707)、[Remote Flake Test](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30528517719)、[Nix Source Filter Check](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30528517735)、[Rust CI](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30528517746)、[SPA 404 Check](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30528517762)、[Package CI](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30528517763)、[Lint](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30528517789)が全てsuccess。`nix run .#ci-watch -- refactor/rust-core 3600`は120秒settlement後に終了コード0 |
| cleanup | disposable受入／baseline worktreeを削除し、branch worktreeのtracked statusは空、正規hookを再導入 |

## フェーズ 2 — Cargo workspace を `rust/pokecon/` の単一パッケージへ統合

構造だけを移し、公開 CLI、設定、IPC、生成契約、配布物、実行時挙動は変更しない。

- [ ] **AR-13.1-02** 2.1、2.2、2.3a、2.3b、2.3c、2.3d、2.4、2.5、2.6、2.7の構造移行中は公開CLI、設定、IPC、生成契約、配布物の名前／配置、実行時挙動を変更しない（予定証跡: 各構造commitの`nix run .#contract-check`、`nix run .#cargo-test`、`nix run .#compatibility`、`nix build .#pokecon`が出力するCLI／config／IPC／generated／artifact manifest diffとbehavior regression report）。
- [ ] **AR-10.8-01** PokeConを独立再利用部品の集合ではなく一つの製品として設計する（予定証跡: package／process／artifact diagramとworkspace member検査）。
- [ ] **AR-10.8-02** 将来の独立再利用と交換可能性を設計要件にしない（予定証跡: public APIとextension pointの差分review、根拠のない抽象化不在監査）。
- [ ] **AR-10.8-03** 内部設計で単純さ、変更の追跡しやすさ、状態所有の一元化を優先する（予定証跡: ownership table、change-path review、重複状態検査）。
- [ ] **AR-10.8-04** 別process、信頼境界、任意のplatform依存、異なる配布成果物など実際の必要性がある場合だけ強い実装境界を設ける（予定証跡: `nix run .#check`の成果物として保存する境界ごとの必要性／信頼／配布根拠表とarchitecture review）。
- [ ] **AR-10.8-05** 再利用可能性だけを理由にtrait、service層、変換型、crateを追加しない（予定証跡: 追加抽象化inventoryの根拠reviewとcrate数検査）。
- [ ] **AR-10.9-02** 任意機能はCargo feature、OS差分はtarget条件、別OS processは複数`[[bin]]`による表現を別crateより先に検討する（予定証跡: feature／target／bin選択表、`nix run .#cargo -- metadata --locked --no-deps`、例外根拠review）。
- [ ] **AR-10.9-03** 単一packageで満たせない具体的要件または実測問題が生じた場合だけ別crateを採用する（予定証跡: `nix run .#cargo -- metadata --locked --no-deps`のworkspace member一覧と追加crate例外の要件／測定記録）。
- [ ] **AR-10.9-04** 責務の違い、file数、行数、別OS processであることだけを別crate化の根拠にしない（予定証跡: crate-boundary decision logと`nix run .#cargo -- metadata --locked --no-deps`のworkspace member監査）。

### 2.1 合成起点

- [x] **AR-10.9-01** Rust実装を大きな`rust/pokecon/` package `pokecon`と内部moduleとして作る（証跡: 下記2.1 checkpoint表のCargo合成起点、source境界、metadata／source）。
- [x] 現在の `pokecon` CLI と起動挙動を保ったまま合成起点を移す。
- [x] 旧crateを残したまま、本体libraryと合成起点のscaffoldだけを作る。
- [x] 内部 module を既定非公開にし、再利用想定だけの公開 API を増やさない。

#### 2.1 checkpoint証跡（2026-07-30、完了）

下表のcommandはcaller worktreeまたはNix store成果物に対して直接実行し、すべて終了コード0だった。

| 対象 | 実測結果 |
| --- | --- |
| Cargo合成起点 | workspace 11 packageのうち既存の下位10 crateを保持し、package `pokecon`を`rust/pokecon/`へ配置。library／binary targetはいずれも`pokecon` |
| source境界 | 正準pathのtracked 18件、旧pathのtracked 0件。旧pathのignored 6件は事前hash不変、正準pathのignored 0件 |
| 合成／外部API | `main.rs`は4行wrapper、旧mainから`entrypoint.rs`へのGit renameは94%。`pub mod`は0件、公開は`pokecon::{MainError, run_cli}`だけ |
| Nix成果物 | `nix build .#pokecon --print-out-paths --no-link`がsuffix `-pokecon-0.1.0`のNix store成果物を返す |
| 契約／Rust | `nix run .#contract-check`、`nix run .#clippy`、`nix run .#build-rust`、`nix run .#compatibility` |
| Rust統合test | `nix run .#cargo-test`。workerのscript／dynamic両roleの起動、IPC、協調停止、強制終了と、PokeConのlibrary 29件／startup 3件（Web／Tauri起動・停止）を含む |
| Web／Python | `nix run .#web-check`は74件、`nix run .#check`はPython 63件を含む |
| format／評価 | `nix fmt`後に`nix fmt -- --ci`、`nix flake check --no-build`、`nix run .#editor-smoke`は4/4 |
| CLI／成果物 | `nix run .#cli-help-check`で`pokecon`／`pokecon-worker`のbaseline一致、`nix build .#pokecon --print-out-paths --no-link` |
| metadata／source | `nix run .#cargo -- metadata --locked --no-deps`、`nix run .#source-guard -- rust`／`app`／`spa` |
| 個別gate | `nix run .#source-filter-check`、`nix run .#release-check`、`nix run .#actionlint` |
| 文書／VCS | `nix run .#markdownlint`、`nix run .#textlint`、`nix run .#typos`、`git diff HEAD --check` |
| commit／CI | 合成起点の署名commit `64a7dd5b7c7adfc7f57fbde96b3df98fd3ad25e7`、MSRV lint互換の署名commit `b48f1cea5c66624f6e640575d9e5a083e64ae103`。最終SHAの[GitHub Actions](https://github.com/yqYo1/Poke-Controller-Modified-Extension/commit/b48f1cea5c66624f6e640575d9e5a083e64ae103/checks)は9/9 success。`nix run .#ci-watch -- refactor/rust-core 3600`は120秒settlement後exit 0 |

### 2.2 基盤と契約

- [x] `pokecon-core` を `runtime`、`diagnostics`、`platform` へ移す。
- [x] `pokecon-contracts` を `contracts` と開発用 generator binary へ移す。
- [x] 検査専用データを runtime library 定数から test／検査側へ移す。
- [x] 停止経路と生成契約が移行前と一致することを検証する。

#### 2.2 checkpoint証跡（2026-07-31、完了）

下表の合格gateはcaller worktreeまたは実package成果物に対して実行し、すべて終了コード0だった。初回Package CIの失敗は、構造commitを9/9成功と誤記せず、修正根拠として明記する。

| 対象 | 実測結果 |
| --- | --- |
| core正準source | 正準実装sourceを物理的に`rust/pokecon/src/runtime/`、`diagnostics/`、`platform/`へ移動。shutdownのfirst-writer testとnative platform testを含む`pokecon-core --lib`は2/2 |
| contracts正準source | 正準実装sourceを物理的に`rust/pokecon/src/contracts/`へ、8 JSONを`rust/pokecon/registry/`へ、generatorを`src/bin/generate_contracts.rs`へ移動。generatorは既定無効の`contract-generator` required feature付き開発target |
| 移行facade | 移行中は`pokecon-core`／`pokecon-contracts`が`#[path]`で上記の正準fileをcompileし、`pokecon`の同名private moduleは各`facade.rs`をcompileして互換crateを再exportする。未移行の下位crate依存を維持しつつ実装sourceは重複させず、旧package削除は2.7で行う |
| 検査専用データ | compatibility、generation、CI、foundationの4 registry定数をruntime libraryから`rust/pokecon/tests/contract_sync.rs`へ移動。runtime側にはsettings／protocolとgeneratorに必要なfixed manifestだけを保持 |
| 停止／Rust parity | core 2/2、contracts library 8/8、PokeCon library 29/29、Web／Tauri startup 3/3。shutdown first-writer、target adapter、起動／停止経路を移行後sourceで再検証 |
| 生成契約 parity | generator `--check`は16/16 up-to-date、`contract_sync`は8/8。両commitとも生成済みcontract artifactの内容差分0 |
| Web／Python | Web 74件、Python 63件が成功 |
| 集約／format | `nix run .#check`はexit 0。formatは178 files、0 changed |
| Debian実package | `nix run .#tauri-build -- --bundles deb`、`nix run .#package-smoke -- dist/tauri/*.deb`、`nix run .#package-install-smoke -- dist/tauri/*.deb`が成功 |
| 構造commit | SSH署名commit `3af92b713a75c2463b441d1ba0f325f669f93c40`。Cargo.lockは`regex`／`sha2`の依存所有を`pokecon-contracts`から`pokecon`へ移す4行だけが変わり、version行は不変 |
| 初回Package CI | 構造commitの[Package CI 30576972468](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30576972468)はfailure。開発用`generate_contracts`がTauri bundle対象へ漏れ、存在しないrelease binaryのcopyを試みたことで発覚 |
| packaging修正 | SSH署名commit `5ee440b6dbfe888d1394afd81a3c7718283ac8aa`。generatorをrequired featureで隔離し、Nixの3呼出しだけで有効化。Tauri CLI 2.9.6はtarget filter後に`src/bin`を再走査してgeneratorを再追加するため、Windows Package／Release pinを2.11.4へ更新。この修正commitはCargo.lockと生成物の差分0 |
| package成果物 | [Debian job 91022531128](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30587596674/job/91022531128)はbuild、runtime audit、clean install／offline startup／upgrade／uninstall、再現buildに成功。[NSIS job 91022531202](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30587596674/job/91022531202)もinstaller buildとclean install／startup／upgrade／uninstallに成功 |
| 最終CI 1/3 | 最終SHAで[Rust 30587596670](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30587596670)、[Nix Source 30587596671](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30587596671)、[SPA 30587596673](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30587596673)がsuccess |
| 最終CI 2/3 | [Package 30587596674](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30587596674)、[Pytest 30587596675](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30587596675)、[Basedpyright 30587596684](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30587596684)がsuccess |
| 最終CI 3/3 | [Ruff 30587596704](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30587596704)、[Lint 30587596709](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30587596709)、[Remote Flake 30587596710](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30587596710)がsuccess。合計9/9 |
| review／監視 | packaging修正のfresh Sol final reviewはFinding 0。`nix run .#ci-watch -- refactor/rust-core 3600`は120秒settlement後exit 0 |

### 2.3a settings

- [ ] `pokecon-settings`を`settings`へ移す。
- [ ] settings移行だけの独立構造commitにし、workspace全体の共通完了gateを通す。

### 2.3b camera

- [ ] `pokecon-camera`を`camera`へ移す。
- [ ] cameraの具体的な設定applierを`runtime`の合成境界へ移す。
- [ ] cameraから設定永続化、runtime、server、desktopへの依存を排除する。
- [ ] camera移行だけの独立構造commitにし、workspace全体の共通完了gateを通す。

### 2.3c device

- [ ] `pokecon-device`を`device`へ移す。
- [ ] deviceの具体的な設定applierを`runtime`の合成境界へ移す。
- [ ] deviceから設定永続化、runtime、server、desktopへの依存を排除する。
- [ ] device移行だけの独立構造commitにし、workspace全体の共通完了gateを通す。

### 2.3d server

- [ ] `pokecon-server`を`server`へ移す。
- [ ] serverのwire型を追加crateにせず内部変換として維持する。
- [ ] server移行だけの独立構造commitにし、workspace全体の共通完了gateを通す。

### 2.4 dynamic と worker

- [ ] `pokecon-dynamic` の本体側契約／状態を `dynamic` へ移す。
- [ ] `pokecon-worker` の IPC、世代管理、親側監督を `worker` へ移す。
- [ ] CPython／LuaJIT 初期化と実行を `pokecon-worker` binary 固有 module に隔離する。
- [ ] **AR-10.9-06** 同じworker binaryが起動引数`--kind script`または`--kind dynamic`で役割を選択する（予定証跡: `nix run .#cargo -- metadata --locked --no-deps`のtarget一覧と`nix run .#cargo-test`の両role process／IPC smoke log）。
- [ ] **AR-11-07** user script workerを自動実行時の機能上の実行主体とし、資源要求を主制御として維持する（予定証跡: script command→IPC request→resource operation traceと実行判断所有test）。
- [ ] **AR-11-08** RustメインをOS上の監督兼資源serviceとし、process親子と製品機能上の主従を区別する（予定証跡: ownership／control-flow diagramとsupervisor lifecycle test）。
- [ ] **AR-11-09** worker→Rustメインの資源操作要求を主制御、Rustメイン→workerの起動／停止／世代切替を監督制御とする双方向IPCにする（予定証跡: direction／message-kind contractと双方向integration test）。
- [ ] 別OS process、双方向IPC、協調停止、強制停止、世代管理を移行前と同じ試験で証明する。
- [ ] **AR-13.1-26** worker統合直後、配布された`pokecon`が同じ配布物内の`pokecon-worker`を解決し、profile別環境でscript／dynamic両roleを起動できる（予定証跡: `nix build .#pokecon`成果物を使う対応Nix integration taskのclean-install worker resolutionとprofile別両role smoke log）。

### 2.5 desktop移行

- [ ] `pokecon-desktop`を`desktop`内部moduleへ移す。
- [ ] **AR-10.9-08** PokeCon本体binaryにWeb UIとTauriの両方を含め、起動引数で表示形態を選択する（予定証跡: `nix build .#pokecon --print-out-paths --no-link`が返す同一store binary digestによる対応Nix integration taskのWeb／Tauri起動／停止smoke log）。
- [ ] **AR-11-11** Web UIを主要UI、Tauriを下位の表示形態とする機能境界を維持する（予定証跡: `nix run .#cargo-test`のmode capability matrix、Web-first endpoint test、Tauri adapter dependency check）。
- [ ] **AR-11-22** Web UIとTauriを同じPokeCon本体実行ファイルに含め、起動引数で切り替える（予定証跡: `nix build .#pokecon`成果物manifestとstore内binaryを使うNix integration taskの両mode CLI／startup report）。
- [ ] **AR-13.1-27** desktop移行後、Tauri設定、icon、bundle resource、署名対象、Linux package、Windows installerを`rust/pokecon/`起点で生成する（予定証跡: `nix run .#tauri-build`とWindows Package CIのOS別build log、bundle／signing input manifest）。

### 2.6 `tauri-shell`とPython native extensionの削除

- [ ] **AR-10.9-09** 利用者向け`tauri-shell` featureを廃止し、非対応OSのみ内部target条件を使う（予定証跡: `nix run .#cargo -- metadata --locked --no-deps`のfeature／target情報、OS別Nix／Windows CI build matrix、許可済み`git grep`の`tauri-shell`残存参照log）。
- [ ] **AR-11-23** `tauri-shell`によるWeb専用buildを廃止し、対応OS向け標準成果物に両UI modeを常に含める（予定証跡: `nix build .#pokecon`とWindows Package CIのOS別artifact manifest、store／package binaryの両mode smoke、Web-only artifact不在検査）。
- [ ] **AR-10.9-07** `pokecon-pybindings`を削除する（予定証跡: `nix run .#cargo -- metadata --locked --no-deps`、`nix run .#source-guard`、`nix build .#pokecon`のpackage outputからのpybindings不在）。
- [ ] **AR-11-21** `pokecon-pybindings`とnative wheelを削除する移行手順を実施する（予定証跡: `nix run .#check`の移行順序log、Python import／wheelのnegative test、Nix artifact diff）。
- [ ] Python packageから`_native`のimport、registry、maturin、native wheel生成を削除する。
- [ ] Nix、release gate、成果物一覧からnative wheel参照を削除する。
- [ ] **AR-13.1-28** Python packageから`_native`のimportとwheel生成を削除し、Nix、maturin、release gate、成果物一覧にnative wheel参照が残っていない（予定証跡: 許可済み`git grep`のlog、`nix run .#test`のPython import negative test、`nix run .#check`とRelease CIのartifact manifest）。

### 2.7 旧crateとworkspace参照の削除

- [ ] **AR-11-31** workspace memberを`rust/pokecon/`だけにする（予定証跡: `nix run .#cargo -- metadata --locked --no-deps`のmember一覧）。
- [ ] **AR-11-32** Cargo package名を`pokecon`へ揃える（予定証跡: `nix run .#cargo -- metadata --locked --no-deps`、Cargo.lock、Nix／CI／releaseのpackage identityを検査する`nix run .#check`のreport）。
- [ ] Cargo.lock、Nix、CI、release、installer、文書のpath／package名を更新する。
- [ ] 各旧crateを削除する前に、そのcrate自身を除くCargo manifest、Nix式、CI、release script、Python build、Tauri設定から参照が消えたことを機械検査する。
- [ ] 旧crate directoryを削除する。
- [ ] **AR-10.9-05** `pokecon-pybindings`以外の実装をPokeCon本体1 packageへ統合し、監督／資源service、共通worker、開発用generatorを複数`[[bin]]`で配置する（予定証跡: `nix run .#cargo -- metadata --locked --no-deps`の1 package／target一覧と`nix build .#pokecon`のstore内binaryを使うNix smoke log）。
- [ ] **AR-11-20** PokeCon本体の1 package統合を完了し、共通worker実行ファイルを役割引数付きの別OS processとして起動する（予定証跡: `nix run .#cargo -- metadata --locked --no-deps`の最終package構成と`nix build .#pokecon`成果物を使うNix integration taskのprocess tree／両role起動／IPC／停止report）。
- [ ] **AR-13.1-29** repository全体で旧crate名と`rust/pokecon-*` pathを検索し、明示的に維持する履歴説明以外の参照がない（予定証跡: 許可済みVCS入口`git grep`のpattern別logと例外一覧）。

### 内部依存の受入

- [ ] `contracts` は他の実行時 module に依存しない。
- [ ] `runtime` は `server`／`desktop` に依存せず、合成起点が adapter を接続する。
- [ ] `server`／`desktop` は hardware handle、interpreter state、正準状態を直接所有しない。
- [ ] worker は main process 所有の hardware handle／正準状態へ直接アクセスしない。
- [ ] `settings`、`camera`、`device`、`worker`、`dynamic`、`contracts`、`diagnostics`、`platform` から `runtime` への逆依存がない。

## フェーズ 3 — 通常 CI の責務とフィードバック時間を再構成

### workflow とゲート

- [ ] **AR-10.10-01** 常に起動する一つの通常CI workflowで変更領域を判定し、安定名の集約gateを必ず完了させる（予定証跡: workflow数、job DAG、全fixtureの集約gate conclusion）。
- [ ] **AR-10.10-02** 文書、契約、Rust、Python、Web、製品smoke、remote flakeを独立した適用領域として判定する（予定証跡: path→領域判定matrixと領域別fixture output）。
- [ ] **AR-11-42** 通常CIの変更領域判定、各jobの所有検査、集約必須gateを確定する（予定証跡: region／job／check ownership matrixとworkflow fixture report）。
- [ ] **AR-10.10-03** workflowを常に起動し、workflow-level path filterで全体を省略しない（予定証跡: trigger定義と各single-area fixtureのworkflow run）。
- [ ] **AR-10.10-04** 不要な領域jobは成功扱いで明示的に省略し、必須gateを不定にしない（予定証跡: 非該当fixtureのskip reason／conclusionとaggregate output）。
- [ ] **AR-10.10-05** feature commitはpull request、既定branch／明示的な統合branchへの直接反映はpushで検査し、同一SHAを両eventで重複検査しない（予定証跡: event／branch matrixとSHA別workflow-run個数）。
- [ ] **AR-10.10-19** 通常CIへbranch単位の`cancel-in-progress`を設定し、新push後は旧SHAの重い検査を継続しない（予定証跡: 連続push fixtureの旧run cancelled／新run completed記録）。
- [ ] **AR-10.10-07** Package CIも常に軽量な集約gateを返し、無関係な変更は明示的成功、関係する変更はOS別package jobの成功を必須にする（予定証跡: package無関係／関係fixtureのjob DAGとaggregate conclusion）。
- [ ] **AR-10.10-06** 通常CI集約gateを既定branch rulesetのrequired status checkに指定し、失敗中または未完了のmergeを拒否する（予定証跡: ruleset API読み戻しJSONとpending／failing PRのmergeability）。
- [ ] **AR-13.1-13** 既定branch rulesetを読み戻し、通常CIとPackage CIの集約gateがrequired status checkで、未完了または失敗時にmerge可能と判定されないことを確認する（予定証跡: ruleset JSON、required context一覧、PR mergeability記録）。
- [ ] **AR-13.1-14** rulesetの検証で管理者権限、APIによる直接merge、rulesetの一時無効化を使用しない（予定証跡: GitHub ruleset／merge audit logと、管理者bypass、直接merge、一時無効化の操作件数0の記録）。

### 重複除去と Nix 成果物境界

- [ ] **AR-10.10-08** format、静的解析、generated drift、test、build、互換性を削除せず、同一SHA／同一対象環境で各論理検査を一度だけ実行する（予定証跡: check ownership inventoryとSHA／環境／logical-check別実行回数report）。
- [ ] **AR-11-43** 同一SHAと同一対象環境で論理検査を重複実行しないworkflowにする（予定証跡: logical-check execution matrixとworkflow runの重複数0）。
- [ ] **AR-10.10-09** 短い文書検査と静的検査を少数のfast jobへまとめ、同一runnerのNix storeを再利用する（予定証跡: fast job DAG／runner ID、step別store-path／wall-clock report）。
- [ ] **AR-10.10-10** RustのClippy、build、test、互換性検査を一つのLinux jobで`CARGO_TARGET_DIR`共有にし、Windows固有workspace checkを`Check workspace (Windows)`として別jobで維持する（予定証跡: job／step定義、target-dir path、Windows job名と`cargo check`ログ）。
- [ ] **AR-10.10-11** `contract-check`を契約生成、同期、schema、受入記録、API生成物driftだけに限定する（予定証跡: task dependency／command traceと対象検査一覧）。
- [ ] **AR-10.10-12** Basedpyright、source filter、shell lint、release identityを対応する原子的Nix taskで通常CI中に一度だけ実行する（予定証跡: flake app／check一覧とworkflow command trace）。
- [ ] **AR-10.10-13** PokeCon本体、Web、Python、文書、検査scriptごとにNix sourceを分け、無関係な変更で製品derivation hashを変えない（予定証跡: source definitionとsingle-area fixture前後のderivation hash matrix）。
- [ ] **AR-11-44** 製品、Web、Python、文書、検査scriptを分けるNix source境界を確定する（予定証跡: source-boundary manifest、include／exclude fixture、hash isolation report）。
- [ ] **AR-10.10-14** 製品package buildとRust test derivationを分け、SPA smokeではテスト済み製品成果物を再利用してserver起動と組込みWeb資源だけを検査する（予定証跡: derivation graph、store path同一性、SPA smoke command trace）。
- [ ] **AR-10.10-15** remote flakeはremote SHAのmetadataと既定appの起動可能性を検査し、localは`nix flake check --no-build`で全outputを評価する（予定証跡: remote SHA／metadata／app smoke logとlocal evaluation log）。
- [ ] **AR-10.10-16** 検査を実行しないlocal／remoteの`check --help`重複を削除する（予定証跡: 許可済み`git grep`のworkflow command logと`nix run .#check`の削除前後logical-check inventory）。
- [ ] **AR-10.10-17** PokeCon固有derivationのbinary cacheを導入し、信頼済みpushだけが書込み、pull requestは読取りだけを行う（予定証跡: event別credential／permission表、PR write negative test、trusted-push upload log）。
- [ ] **AR-11-45** PokeCon固有derivationを再利用するbinary cacheの信頼境界を確定する（予定証跡: actor／event／read-write permission matrixとcache audit log）。

### 計測と受入

- [ ] **AR-13.1-11** 文書だけ、定義書だけ、Rustだけ、Pythonだけ、Webだけ、flakeだけを変更したfixtureまたは実commitで、適用／省略jobが設計どおりか検証する（予定証跡: 6 fixtureのexpected／actual job matrixとrun URL）。
- [ ] **AR-13.1-12** 各論理検査を同一SHA／同一対象環境で一度だけ実行し、集約必須gateが成功、失敗、明示的省略を正しく集約する（予定証跡: logical-check実行回数表と3 conclusion fixtureのaggregate output）。
- [ ] **AR-10.10-18** 同一commitの再実行でsubstituteされたstore path、build対象derivation数、wall-clock時間を比較し、cache-hit表示だけで有効性を判断しない（予定証跡: 同一SHAの1回目／2回目のstore path／derivation／wall-clock diff）。
- [ ] **AR-13.1-15** 同一commitを二回実行し、二回目のPokeCon固有derivationがbinary cacheからsubstituteされ、build対象derivation数とwall-clock時間が減少する（予定証跡: 2 runのsubstitute path、build plan、時間比較report）。
- [ ] **AR-10.10-TIME-01** 通常CIの完了目標をfast job 3分以内、文書だけの変更 5分以内、製品code変更の必須gate 10分以内とする（予定証跡: 変更種別ごとの直近10回相当のp95 report）。
- [ ] **AR-11-46** fast job、文書変更、製品code変更のCI完了時間目標を受入gateにする（予定証跡: threshold definitionと変更種別ごとのp95 pass／fail report）。
- [ ] **AR-10.10-TIME-02** 同種変更の直近10回のp95が目標を超えた場合を回帰とし、job名だけでなくstepとderivation単位の時間を記録する（予定証跡: p95算出program出力、step／derivation timing report、回帰fixture）。
- [ ] **AR-11-47** CI時間をstep、derivation、cache substituteの単位で検証する（予定証跡: runごとのstep／derivation／substitute／wall-clock計測物）。
- [ ] **AR-10.10-WATCH** CI短縮後のp95に30%以上の余裕を加えた`ci-watch.sh`監視期限へ変更し、期限切れと`completed failure`を異なる終了理由で表示する（予定証跡: p95算出根拠、既定期限、timeout／failure fixtureのstderr／exit code）。
- [ ] **AR-13.1-16** 直近10回相当でfast job、文書変更、製品code変更の時間目標を満たし、`ci-watch.sh`が正常critical pathを期限切れにしない（予定証跡: p95 report、watch timeout計算、最長正常runの監視log）。
- [ ] **AR-10.10-PACKAGE** Package CIとReleaseをOS別成果物、clean install、upgrade、uninstall、再現可能性、署名対象の独立配布gateとして維持する（予定証跡: Package／Release workflowのOS別run URLとconclusion、clean install／upgrade／uninstallの操作log、同一入力2 buildの再現性artifact digest比較、署名対象manifest）。
- [ ] **AR-13.1-25** 通常CI再構成後もPackage CIとReleaseがOS別成果物、clean install、upgrade、uninstall、再現可能性を従来どおり検査する（予定証跡: 移行前後のjob／assertion matrixとOS別run URL）。
- [ ] **AR-10.10-PACKAGE-REPRO** 同一入力からpackageを二回生成する再現性検査は意図した重複として通常CIの重複削減対象から除外する（予定証跡: reproducibility jobの2 build trace、artifact digest比較、重複例外inventory）。

## フェーズ 4 — 確定した実行時挙動を反映

構造統合完了後、挙動ごとに独立した変更として進める。

### 4.1 主経路と優先順位

- [ ] **AR-10.4-FUNCTIONS** 機能の優先順位を理由に`SPECIFICATION.md`の機能を不要または省略可能と判断しない（予定証跡: specification機能→実装／受入testの全件matrixと未実装数0）。
- [ ] **AR-10.4-QUALITY** 後回しの機能も競合がない状態で定義済みの低遅延性、性能、安定性を満たし、優先度を品質要件緩和の理由にしない（予定証跡: 非競合時の機能別latency／throughput／stability report）。
- [ ] **AR-11-01** 定義書に記載する全機能の役割を実装と受入testへ対応付ける（予定証跡: 機能／役割／owner／testの全件matrix）。
- [ ] **AR-11-02** 機能同士が資源を競合した場合の処理優先順位をqueue、lock、task、threadに反映する（予定証跡: contention matrixと順序／飢餓／逆圧stress report）。
- [ ] **AR-11-03** 停止、全入力解放、neutral状態送信を最上位優先経路で処理する（予定証跡: 各通常処理との競合fault testで安全状態遷移が先行するtrace）。
- [ ] **AR-11-04** 通常運転時のserial出力と画像認識を同じ高優先度で独立して進める（予定証跡: 双方向同時load fixtureのlatency／jitter／progress report）。
- [ ] **AR-11-05** 明示的優先機構の導入前後で遅延、jitter、飢餓、queue停滞を比較し、改善しない機構は採用しない（予定証跡: before／after benchmarkと採用判定record）。
- [ ] **AR-11-06** command実行、画像認識、serial出力を結ぶ主経路を実装する（予定証跡: end-to-end command→frame→controller／serial traceとlatency report）。
- [ ] **AR-11-12** UI制御要求を表示配信より優先し、映像、状態、logの滞留を主経路へ伝播させない（予定証跡: slow／disconnected UI fixtureのcontrol latency、frame freshness、queue bound／backpressure report）。
- [ ] frame は最新の完全 frame へ追従し、状態更新は同一項目の旧値を集約する。
- [ ] log 配信を有界 queue とし、低速／切断 UI から主経路への逆圧を防ぐ。

### 4.2 手動介入と入力調停

- [ ] **AR-11-14** script実行中の手動介入を許可または拒否でき、停止と入力解放は常に受け付けるcanonical settingを追加する（予定証跡: setting全表面のdrift check、allow／deny／stop／release arbitration test）。
- [ ] CLI、TOML、環境変数、Web UI の全表面へ生成／投影する。
- [ ] 既定は介入許可とし、適用後の入力から即時反映する。
- [ ] 排他 mode でも停止と全入力解放を常に受け付ける。
- [ ] **AR-11-15** 介入許可時は操作中の入力要素だけを一時上書きし、操作終了後にscript入力へ戻す（予定証跡: button／stick／touchごとのelement-level arbitration state-transition test）。
- [ ] button の script／manual 合成規則を試験する。
- [ ] **AR-11-13** 手動操作が自動scriptへ不意に影響せず、手動単独利用時も低遅延かつ安定して動作する（予定証跡: script実行中のno-input／manual-input isolation testとmanual-only latency／stability report）。

### 4.3 profile切替

- [ ] **AR-11-16** 新規command受付停止、実行中script停止、全入力解放、旧worker終了後にprofile／関連設定を一括切替し、旧commandを自動再開しない（予定証跡: lifecycle state traceと切替後idle／no-auto-restart integration test）。
- [ ] 新規 command 受付停止 → 実行中 script 停止 → 全入力解放 → 旧 worker 終了の順序を保証する。
- [ ] profile と関連設定を一括切替し、新 worker 環境を初期化する。
- [ ] 切替後は旧 command を自動再開せず idle で明示実行を待つ。
- [ ] **AR-11-17** profile切替失敗時は旧設定だけを復元してidleへ戻し、復元失敗時は安全停止を維持して新しい実行を拒否する（予定証跡: 切替／復元の段階別fault injection、最終profile／input／command-acceptance state）。
- [ ] 復元失敗時は安全停止状態を維持し、明示復旧まで新規 command を拒否する。
- [ ] 失敗段階を UI と log へ明示する。

### 4.4 動的設定の候補世代切替

- [ ] **AR-11-18** 動的設定を候補世代で構築し、全読込み／検証成功時だけ自動実行を止めずに一括切替する（予定証跡: concurrent script／reload trace、candidate validation fault test、generation switch log）。
- [ ] reload 中も現世代を有効に保ち、自動 script を停止しない。
- [ ] Python／Lua 設定、callback、command 一覧を候補世代として構築する。
- [ ] 読込みと検証が全成功した場合だけ原子的に切り替える。
- [ ] 実行中の旧 callback は旧世代で完了させる。
- [ ] 失敗時は現設定を変更せず、UI と log へ通知する。
- [ ] reload が camera、serial、script 主経路を待たせないことを検証する。

### 4.5 通知隔離

- [ ] **AR-11-19** 通知を有界queueと期限付きretryへ隔離し、主経路へ待機と障害を伝播させない（予定証跡: queue-full、slow／failing provider、retry deadline fixtureの主経路latency／progress／failure report）。
- [ ] 通知要求を有界 queue へ入れ、主経路 lock を保持せず処理する。
- [ ] queue 上限時は主経路を待たせず、呼出元へ明示的失敗を返す。
- [ ] retry 回数と期限を制限する。
- [ ] 完了待ちは当該呼出しだけに限定し、camera、画像認識、serial、他要求を継続する。
- [ ] 外部通知障害を script worker 全体へ伝播させない。
- [ ] 停止と全入力解放を通知処理より優先する。

## フェーズ 5 — 配布、文書、最終監査

- [ ] Windows／Linux の標準成果物が Web／Tauri 両 mode、本体、worker、Web 資源、uv、管理 Python を含む。
- [ ] non-Nix 配布、clean install、upgrade、uninstall、再現可能性、署名対象を検証する。
- [ ] README と利用者／開発者文書を最終実装へ同期する。
- [ ] `ARCHITECTURE_REVIEW.md` §11 の各引渡し情報に対応する実装または受入証跡を列挙する。
- [ ] `ARCHITECTURE_REVIEW.md` §13.1 の全受入条件に直接の証拠があることを監査する。
- [ ] `SPECIFICATION.md` の対象機能を要件別に照合し、未検証項目を「暗黙に成功」と扱わない。
- [ ] 全共通完了ゲートを clean worktree で再実行する。
- [ ] Sol 役のコンテキストを切った辛口レビューを受け、重大・高・中の指摘をすべて解消する。
- [ ] 最終 push 後の GitHub CI を完了まで監視し、全 required gate の成功を確認する。
- [ ] `PLAN.md` の全項目を証拠に基づいて `[x]` に更新する。
