# アーキテクチャレビュー反映計画

## 目的と規範

- 製品規範: `SPECIFICATION.md`（入口）とbackend／frontend／integrationの三定義書
- 過去のarchitecture reviewはローカル専用アーカイブであり、現行の製品規範・実装状態の根拠にはしない。
- 対象ブランチ: `refactor/rust-core`
- 開始基準: `e20fad5a`（レビュー書更新完了時点）
- 最終状態: PokeCon を単一 Cargo パッケージへ統合し、本計画と製品定義書で確定した責務、優先順位、開発入口、CI、配布契約を実装と検証へ反映する。
- 進捗規則: 完了を証明するコマンドまたは成果物がある項目だけを `[x]` にする。部分完了は子項目だけを更新する。
- 変更規則: 構造移行と実行時挙動変更を同じコミットへ混在させない。各チェックポイントで検証してから次へ進む。
- 委譲規則: 実装委譲は不可分な1タスクずつ行い、primary agentが差分と対応gateを確認し、当該タスクだけを新規文脈のSolへレビュー依頼してから次のタスクへ進む。節全体を一度に委譲しない。
- 実施順序: レビュー書の原順序は構造移行 → CI 再構成 → 開発入口の確定 → 実行時挙動変更である。2026-07-30にはflake app化を先行し、2026-08-09の利用者指示でtool-only devShell／direnvを正規入口として復帰した。用途別appと隔離gateは維持し、現在は構造（フェーズ2）完了後のCI（フェーズ3） → 挙動（フェーズ4）の原順序で進める。
- コミット規則: 構造移行の2.1、2.2、2.3a、2.3b、2.3c、2.3d、2.4、2.5、2.6、2.7をそれぞれ独立したコミット境界とする。フェーズ4は4.1主経路と優先順位、4.2手動介入と入力調停、4.3 profile切替、4.4動的設定の候補世代切替、4.5通知隔離を独立した挙動変更checkpointとする。各checkpointの共通完了gateが失敗した状態で後続checkpointへ進まない。

## 現在地

- [x] 過去のarchitecture reviewで確定した方針、移行順序、受入条件を一対一の実装要件として本計画へ展開した（2026-07-30 Sol意味監査承認、129要件）。
- [x] 開始時の作業ツリーがcleanで、HEADが`e20fad5a`、`origin/refactor-rust-core`との差が0/0であることを変更前の`git status --short --branch`と`git rev-list --left-right --count`で記録した（2026-07-30 JST）。
- [x] 旧実装の不在を前提にした旧 `PLAN.md` を廃止し、本チェックリストへ置き換えた。
- [x] 2026-07-30時点の旧フェーズ1「flake appへの開発入口移行」を完了した（実装`23149e3`、受入`1e0836b`、再現性修正`7a01da0`。最終GitHub Actions 9/9 success）。
- [x] 現行実装を含む署名commit `d1de87c86e000192b85bb576736b115a94a06255`を`refactor/rust-core`へpushし、worktree cleanおよびlocal／remote SHA一致を確認した。
- [x] 同commitのNormal CI run `34272925506`とPackage CI run `34272925529`がcompleted／successとなり、required aggregateを通過した。
- [x] timing artifact `ci-timing-34272925506-1`（artifact ID `10075004892`）を保存し、同一`change_kind`の履歴を使うblocking／fail-closed p95 gateをfresh CIで通過した。
- [x] `PLAN.md`／将来の`TASK.md`を作業追跡用メタデータとしてCIのdocs領域分類から除外し、常時実行のfast checksでlintする契約を追加した（`scripts/ci/regions.py`、`tests/quality/test_ci_regions.py`）。
- [x] commit `50bd589ba67ec801f9be4579c7062984d9577b2f`のNix起動済みWeb runtimeを隔離rootで再受入した。clean rootの`POST /api/commands/reload`は`200`／`changed=true`、手動作成profileの`active_profile`切替は`Other`→`default`で`200`、stale revisionは`409 revision_conflict`となり、失敗write後のstate不変も確認した。ブラウザbackendは途中で`410 Gone`となったため、追加証跡はREST／state read-backに限定し、tailnet越しWebRTC primary映像成功とは扱わない。
- [x] commit `5c740398da8c345e3abe612cc72cc94adc226e36`でx86_64-darwin専用のlocked `nixpkgs-darwin`（26.05-darwin）を追加し、main Linux nixpkgs／uv 0.11.28を維持したまま4 systemのpackages、devShells、checks、appsを`nix flake check --no-build --all-systems`で評価した。`nix run .#check`、`nix build .#pokecon --no-link`、`nix build .#web --no-link`、`nix run .#web-check`、`nix run .#actionlint`も成功した。同commitのpush Normal CI `34788294682`、PR Normal CI attempt 3 `34788296839`、Package CI `34788294683`がsuccess、PR required contextsもSUCCESSとなった（PR Normal attempt 1／2のproduct p95は729秒／721秒でthreshold 720秒を超えたため採用せず、attempt 3で再検証）。
- [x] 署名commit `6ba7f4b64977f16bcea36a2c84e9476151b32285`でWindows PE normalization invocationの引数順序とtraceability参照を修正した。Normal CI `36127841885`とPackage CI `36127841813`がcompleted／success。同Package CIのDebian／Windows bundle、clean-install、独立二重build、byte-for-byte reproducibility、aggregate required job（Debian `108052948235`、Windows `108056470884`、aggregate `108056741526`）がすべてsuccessし、primary／repro artifact 4件（Linux／Windows）が未期限で存在することを読み戻した。

### 現行の残タスク（アシスタント担当）

- [ ] 2026-08-09の利用者指示で上書きされたtool-only devShell／direnv併用契約をclean detached worktreeで再受入する。RUN4／RUN5／RUN6、4 system評価、既存worktreeでのdirenv実行、tool inventory、source-filterは証跡付きで完了した。残るclean worktree証跡はRUN3、AR-11-48、AR-13.1-17に限定されるが、既存worktreeのみを使う指示により新規worktreeは作成しない。
- [ ] CI上のmock／virtual I/O性能計測を実装し、固定条件、統計値、artifact、regression threshold、required blocking gateを追加する。browser primitive smokeとそのblocking jobは実装済みだが、production主経路のCI証跡は未完了。実機latency／throughput計測環境は作成しない。
- [ ] GPUIフロントエンド段階導入を、[GPUI_FRONTEND_PLAN.md](docs/GPUI_FRONTEND_PLAN.md)のPoC gateに従って進める。現行Svelte/Tauri/Web経路は採否判断まで維持する。
- [ ] 全フェーズ完了後の要件別監査を行い、実装済み項目は証跡で`[x]`へ更新し、未完了項目は具体的な実作業へ整理する。

### 2026-09-14 要件別監査の結果

- [x] 構造移行、CI／runtime、配布／文書の4系統をread-onlyで監査し、source上の実装と証跡不足を分離した（監査対象の未完了18件、フェーズ3／4の未完了項目、フェーズ5の未完了項目をID・行番号・次作業へ分類）。監査だけで[x]へできる項目は追加しなかった。
- [ ] **devShell／direnv再受入**: `nix flake check --no-build --all-systems`と4 system（`x86_64-linux`、`aarch64-linux`、`aarch64-darwin`、`x86_64-darwin`）のdevShell attribute評価は成功した。x86_64-darwinはlocked `nixpkgs-darwin`（26.05-darwin）を専用pkgsとして使い、main Linux nixpkgs／uv固定を維持する実装へ更新した。現行worktreeでは`direnv allow`／`direnv exec .`、hostile PATH下の`nix develop`、source-filter／SPA guard、入室前後のtracked worktree無変更を確認した（2026-09-14）。ただしclean detached worktreeでの同一証跡は、既存worktreeのみを使う制約により未証明であり、RUN3全体は[x]にしない。
- [ ] **CI性能gate**: `performance-check`とrequired blocking jobは実装済み。既存artifact `10334767608`のreportは5 metric（各300 sample、60秒warm-up、p50／p95／maximum）条件を満たしていたが、run `34846200611`のbaseline解決stepはrunner非対応の`gh api --output`でartifact取得に失敗しbootstrapへ落ちていた（2026-09-14）。`.github/workflows/normal-ci.yml`を標準出力の`> "$zip_path"`へ修正し、quality test 2件、actionlint、formatを通過した。post-fix CI run `34862735176`でprior-passing artifactの`baseline_status=compared`まで到達したが、`ui_input_latency`の正当なゼロbaseline（p95=0.0）に対する相対閾値が0となる境界不具合を検出した。`scripts/performance/benchmark.py`をabsolute threshold fallbackへ修正し、focused testとaggregate checkを通過した。修正commit `09f55ad6d10670d9190f0a52cb72d92418c26038`後、Normal／Packageのpush／PR required checks全4件がsuccessし、`gh pr checks 27 --required` exit 0を確認した。production主経路のlatency／throughput／stabilityは未完了。既存のCI workflow timing／10-run p95は製品latency／throughputの代用品にしない。
- [x] WebRTC primaryがactiveな間に到着するMJPEG fallback frameで`MediaView`をfallbackへ降格しないよう`web/src/lib/media.ts:197-200`を修正し、`web/src/lib/media.test.ts`の回帰テストを追加した。`nix run .#web-check`はfrontend 103 tests、svelte-check 0 errors／0 warnings、buildを成功した。外部browser／tailnet WebRTCの実映像受入は別項目として未証明。

- [ ] **外部browser受入**: browser backendへのread-only probeは`example.com`で成功し、以前の`410 Gone`は現行の障害ではない。製品の最新commitはまだ隔離された一時server経由で受入していないため、WebRTC channel、fallback／再昇格、keyboard／accessibilityのcommit別証拠は未成立。writer完了後に専用HOME／XDGのephemeral serviceで試験し、必ず停止・cleanupする。既存camera／serial virtual-I/OとREST read-backは代替証拠にしない。
- [x] **最終対応表**: [Backend traceability](docs/TRACEABILITY_BACKEND.md)、[Frontend traceability](docs/TRACEABILITY_FRONTEND.md)、[Integration traceability](docs/TRACEABILITY_INTEGRATION.md)の要件別直接根拠を、[総合トレーサビリティ表](docs/TRACEABILITY_INDEX.md)へ統合した。root仕様、三定義書、ARCHITECTURE、PLANの6文書、各spec section、checkpoint共通gateを対応付け、auto_reload／GPUI／production性能／実browser／実機／clean worktree／Release tagは未完了として明示した（独立read-only reviewで確認、release tagは利用者担当）。

### 2026-09-24 追加再確認（この記録時点で未commit・未push）

- `flake.nix`のaggregate `check`がsource-filterを元worktree rootで実行するよう修正した。
- 分割後の三規範仕様書をRust contract-test source boundaryへ追加した。
- `nix run .#source-filter-check`、`nix run .#ci-rust-contracts`、`nix run .#contract-check`、production-routing mutation audit、`nix run .#check`はすべてexit 0だった。
- `nix run .#ci-rust-contracts`では`contract_sync`の14 testsが成功した。
- `nix flake check --no-build --all-systems`、`nix run .#release-check`、`nix run .#acceptance-record-check`、`native_serial_pty` testも成功した。
- `direnv exec`ではcargo、bun、uvがNix store内の実行ファイルへ解決した。
- clean detached worktreeでの証拠は、既存worktreeのみを使う制約により未取得である。
- `nix run .#virtual-io-check -- 42`はresource provenance、V4L2 readiness／writer permission、module ownershipを修正した後に再実行し、exit 0を確認した。regression testは6 passed、Ruff／format／shell syntax checksもexit 0。
- 修正後の終了時に`/dev/video42`、loopback module、`ffmpeg`、gate用一時directoryが残らないことを再確認した。
- GitHub run `35900764591`（push）と`35900770262`（pull_request）は同一`82b973e` SHAで、双方のNormal CI Rust／contract jobが失敗していた。
- Package CIの対応run `35900764463`と`35900770266`は両方passだったが、同じSHAに対して重複していた。
- Phase 2の表に記録された75個のworkflow run IDを`gh run view`で個別に読み戻し、73 success、2 failure、0 unavailableだった。
- 失敗はPackage CIのrun `30576972468`（SHA `3af92b7`）と`31030340585`（SHA `d1338b3`）だけで、両方の修正／follow-up successを各checkpoint表で記録している。
- 2.5／2.6は複数commitから成る部分完了で、対応SHAのworkflow数も4～5件の群があるため、AR-13.1-01の独立commit条件と全checkpoint共通gateはこの集計だけで完了扱いしない。
- この記録時点のPR required statusはNormal CI fail、Package CI passであり、未pushのローカル修正はこのremote結果を更新しない。
- `protect` rulesetはactiveでdefault branchを対象にし、Normal／Package CIの両required contextを指定している。
- PR #27の`mergeStateStatus`は`BLOCKED`だが、PR自体がDRAFTでもあるため、failing check単独のmergeability試験とはみなさず、AR-10.10-06／AR-13.1-13は未チェックのままにする。
- Repository secrets／environmentsの読み戻しではcache署名credentialを確認できず、credential値は取得していないため、trusted-push upload証跡は未成立である。
- GPUI候補のrelease／pin破損履歴、IME／accessibility／clipboard／packagingの上流課題とPoke-Con上での試験含意を[段階導入計画](docs/GPUI_FRONTEND_PLAN.md)へ記録した。
- `gpui-kit v0.6.6`と対応tag commitを確認した。直接依存metadataのApache-2.0とMIT OR Apache-2.0、同梱Lucide由来iconのISCとFeather由来iconのMIT表示条件、トップレベルNOTICE不在までは確認したが、1,248 packageの推移依存license台帳とGTK3動的リンク条件は未監査である。Cargo.lock統合、native window、IME／accessibility、clipboard、Tokio共存も未検証のため、GPUI Phase 1を完了扱いしない。
- `nix run .#cli-help-check`はexit 0で、現行binaryのCLI helpを検証した。これはnative window起動やGPUI PoCの証明ではない。
- `markdownlint`、`textlint`、`typos-check`は更新した`PLAN.md`、`README.md`、`docs/GPUI_FRONTEND_PLAN.md`でexit 0だった。
- その後の`nix run .#check`再実行は、`tests/quality/test_ci_trigger_dedup.py`にformatter差分が検出され、`--fail-on-change`によりexit 1だった。
- `nix flake check --no-build --all-systems`は、`expectedAuditTestHash`とcanonical flake hashを同期した後にexit 0。直近の`nix run .#source-filter-check`もexit 0だった。
- CI trigger回帰testは`nix run .#test -- tests/quality/test_ci_trigger_dedup.py`で12 passed、`nix run .#actionlint`もexit 0。integration-branch直接pushの要件は別の設計監査中。
- 追加した`test_default_devshell_is_tool_only_and_skips_product_builds`はfocused Nix testで1 passed。RUN4／RUN5／RUN6のPLAN evidenceは更新し、PLAN単体markdownlint／textlint／typosもexit 0。
- 当時の`nix run .#ci-rust-contracts`はnon-contract target数を9から10へ直した後も`concurrent_camera_serial_load`のClippy `too_many_lines`でexit 1だった。後続の構造的分割でfocused testとClippyは通過したが、そのaggregate runではcontract-sync testがfiltered source内に`docs/ARCHITECTURE.md`を欠いて失敗した。`flake.nix`のRust test sourceへ同文書を追加した。
- source boundary修正後、`expectedAuditTestHash`を現在の`test_ui_package_check.py`へ、canonical hashを現在の`flake.nix`へ再同期し、`nix flake check --no-build --all-systems`がexit 0。`nix run .#source-filter-check`もexit 0、`nix run .#test -- tests/quality/test_ui_package_check.py`は29 passed／2 deselected。
- Rust整形確認`nix develop --command rustfmt --edition 2024 --check rust/pokecon/src/script_host.rs`はexit 0。最新`nix develop --command cargo test --locked -p pokecon script_host::tests::`は12 passed、`nix run .#ci-rust-contracts`はexit 0（contract-sync 15 passed、Rust unit 475 passed、`concurrent_camera_serial_load` 1 passedを含む）。一方、同時点の`nix run .#check`はexit 1で、production-routing audit／mutation auditのRust CI区間hash pin不一致と、`test_version_contract.py::test_rust_ci_split_executes_every_declared_non_contract_target_once`の古いtarget数assertionが残った。web/staticではSvelte diagnostics 0 errors／0 warnings、frontend 106 testsとbuildが成功し、別途検出した綴り誤りは修正し、対象test 12 passed、対象file typos-check exit 0。その後の`nix run .#actionlint`、`nix run .#source-filter-check`、`nix flake check --no-build --all-systems`はいずれもexit 0（flake checkは評価のみでbuildなし）。
- `nix fmt -- --ci`はdelegated editsと同時に実行され、Ruff S603とconcurrent-write診断でexit 1。修正・writer完了後にformat／aggregate gateを再実行する。
- browser backendはread-onlyの`example.com` probeで利用可能、`tailscale0`も存在することを確認した。PokeCon serviceはまだ起動しておらず、隔離E2Eはwriters完了後に行う。

### 担当外の外部操作

- Release tag（`v*`）の作成・pushは利用者担当とする。tag作成およびtagを起点とするRelease公開は本計画のアシスタント残タスク／完了条件に含めず、利用者から明示指示があった場合だけ実施する。

## レビュー要件トレーサビリティ

- [x] 過去のarchitecture reviewで合意した要件を、それぞれ一度だけ現れる個別ID付きcheckboxと同じ行の予定証跡へ展開した（証跡: 2026-07-30の許可済みVCS入口`git grep`機械監査で129件、群別`2 / 5 / 9 / 24 / 10 / 50 / 29`、重複0、予定証跡欠落0。コンテキストを切ったSol意味監査で順序、checkpoint、配置、意味対応を承認）。

IDは当時のレビュー項目順に付与し、範囲IDや集約IDでの完了判定は行わない。各checkboxの完了時は「予定証跡」を実行結果、生成物、CI run、またはGitHub設定の読み戻し結果で置き換える。

予定証跡のproject操作はtool-only devShell内から実行し、各行に別のNix outputを明記しない限り`nix run .#check`を入口とする。専用操作は`nix run .#<task>`、`nix build .#pokecon`、`nix fmt`、`nix flake check`のいずれかを完了証跡の入口とし、flake output inventoryの読取りには`nix flake show`を使う。「test」、「report」、「log」、「matrix」はそのNix taskまたはCI workflowが生成する成果物を指し、hostの言語runtime、compiler、package manager、品質toolを直接起動しない。Git／GitHubの読取り、worktree操作、CI runの読み戻しはVCS／外部状態証跡として例外とし、native Windows CI／package／releaseだけはworkflowが固定するtoolchainを入口とする。

## 共通完了ゲート

適用対象はフェーズ1、2.1、2.2、2.3a、2.3b、2.3c、2.3d、2.4、2.5、2.6、2.7、フェーズ3、4.1、4.2、4.3、4.4、4.5の各checkpointとする。各checkpointで次をすべて実行し、該当しないgateは対象外となる具体的理由と代替証跡をcheckpoint記録に残す。失敗または理由のない未実行がある状態で後続checkpointへ進まない。

- [ ] **AR-13.1-01** 2.1、2.2、2.3a、2.3b、2.3c、2.3d、2.4、2.5、2.6、2.7の各構造移行を独立commitにし、当該段階のgate失敗時は後続のcrate／module移行を開始しない（予定証跡: VCSが読み戻す各commit SHAと順序、対応するNix gate log）。
- [ ] **AR-11-41** workspace全体の共通完了gateを全適用checkpointで実行する（予定証跡: checkpoint／commit SHAごとの下記Nix command終了コード、対象外理由／代替証跡、CI run URL）。
- [ ] **AR-13.1-03** `nix run .#contract-check`を通す（予定証跡: 各適用checkpointの`nix run .#contract-check` log）。
- [ ] **AR-13.1-04** `nix run .#cargo-test`を通す（予定証跡: 各適用checkpointの`nix run .#cargo-test` log）。
- [ ] **AR-13.1-05** `nix run .#clippy`を通す（予定証跡: 各適用checkpointの`nix run .#clippy` log）。
- [ ] **AR-13.1-06** `nix run .#build-rust`を通す（予定証跡: 各適用checkpointの`nix run .#build-rust` logとNix build成果物一覧）。
- [ ] **AR-13.1-10** `nix run .#compatibility`で固定互換性基準に対するPython commandを通す（予定証跡: `nix run .#compatibility`が出力するcorpus SHA付きreport）。
- [x] `nix run .#web-check`（2026-09-24、ESLint／Svelte diagnostics 0 errors／0 warnings、26 files・106 tests、production build成功）。
- [ ] `nix fmt`
- [x] `nix fmt -- --ci`（2026-09-24、212 filesを検証し、変更0件）。
- [x] `nix flake check --no-build`（2026-09-24、x86_64-linuxで全output評価・全check評価成功。実行環境非対応のaarch64-darwin／aarch64-linux／x86_64-darwinは明示的に省略）。
- [x] `nix run .#editor-smoke`（2026-09-24、Rust/Python/TypeScript/Svelte language serverのinitialize／didOpen／documentSymbol／diagnostic／shutdownが全て成功。fixtureの期待diagnosticを確認）。
- [x] `nix run .#check`（2026-09-24、dirty worktree上で実行、exit 0。production-routing audit、contract-sync 15 tests、Web 26 files／106 tests、pytest 546 passed／2 deselected、mutation audit 395件を含むaggregate gate成功。commit別CIではない）。
- [x] `nix run .#web-check`（2026-09-25、dirty worktree上で実行、ESLint 0 errors／0 warnings、Svelte diagnostics 0 errors／0 warnings、Web 26 files／112 tests、production build成功）。
- [x] **AR-13.1-07** `pokecon --help`と`pokecon-worker --help`の公開CLI差分を検査する（2026-09-25、dirty worktreeで`nix run .#cli-help-check` exit 0。store binaryの正規化helpをtracked fixtureと比較。`nix build .#pokecon --no-link`もexit 0でNAR `sha256-mwrz2tSs6EfTNZkhYwCsCM/qaOIoeDcUE9IWqmbA2uk=`／132706248 bytes、source store内`production.rs`／`script_host.rs`のSHA-256はworktreeと一致。commit別CIではない）。
- [ ] **AR-13.1-08** Web modeとTauri modeの起動検査を通す（予定証跡: `nix build .#pokecon --print-out-paths --no-link`のstore内binaryを使う対応Nix integration taskの両mode health endpointと起動／停止log）。
- [ ] **AR-13.1-09** `pokecon-worker --kind script`と`--kind dynamic`の起動、IPC、協調停止、強制終了を検査する（予定証跡: `nix run .#cargo-test`がNix build済みworkerを使って出力するrole別integration／fault report）。
- [ ] push 後に `nix run .#ci-watch -- <branch> <timeout>` で GitHub Actions を完了まで監視し、失敗を解消する。

## レビュー引渡し情報の横断チェック

- [x] **AR-11-24** 製品全体で優先する設計原則をmoduleと実行時機構へ対応付ける（証跡: `docs/ARCHITECTURE.md`に9行の設計原則→module／queue／lock／task／thread表を追加し、`contract_sync`の`design_principles_map_to_runtime_mechanisms_without_drift`が各必須module／symbol／sourceを検査。`nix run .#cargo -- test --locked -p pokecon --features integration-test-support --test contract_sync`は13 passed）。
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
- [ ] **AR-11-10** 主経路のlatency、throughput、停止、復旧の受入条件を定義する（予定証跡: CI上のmock／virtual I/O測定fixture、閾値、移行前baseline、移行後report。実機の性能測定環境は作成しない）。
  - 実装済み: `performance-check`がbrowser primitiveのloopback sample、統計、絶対閾値、prior-passing baseline比較、raw/report artifactを生成する。60秒／300件のCI実run、production backendの主経路、停止／復旧、実機I/Oの受入証跡は未完了のまま残す。
- [ ] **AR-11-37** 別processと同一processの境界を確定する（予定証跡: process／module deployment diagramとIPC境界test）。
- [ ] **AR-11-38** 個別成果物と配布方法を確定する（予定証跡: artifact manifestとOS別clean-install report）。

- 2026-09-26 architecture handoff packet: [`docs/ARCHITECTURE_HANDOFF.md`](docs/ARCHITECTURE_HANDOFF.md)に現行sourceのownership、process／resource lifecycle、公開boundary、distribution、dependency ruleを集約した。runtime／fault test、clean detached worktree、実機／実browser、Release tagの証拠は未成立のため、上記AR checkboxは未完了のまま保持する。

## フェーズ 1 — tool-only devShellと用途別Nix appへ開発入口を固定

2026-07-30の旧方針で得た用途別app／隔離gateの証跡は維持する。2026-08-09の利用者指示がdevShell廃止だけを上書きしたため、該当IDは現在の意味へ更新し、再受入が終わるまで未完了へ戻す。

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
- [x] **AR-10.11-APP4** `nix run .#editor`を追加し、host toolchainなしでRust、Python、TypeScript／Svelteのlanguage serverを利用できるようにする（証跡: `--print`が5個のNix store executableを返し、`editor-smoke`が4言語すべてでinitialize、didOpen、documentSymbol、期待diagnostic、shutdownを成功させた）。
- [x] `nix run .#ci-watch` を追加し、CI監視に必要なGitHub CLI等をhost環境から排除する（証跡: 2026-07-30のhostile環境／subdirectoryからの`nix run .#ci-watch -- --help`成功、固定`gh`／`git`／`jq` path）。
- [x] `ci-watch` の暫定既定期限を現行critical pathの実測最大値 + 30% 以上へ延長し、正常CIを期限切れ扱いしない（証跡: 既定1200秒、settlement 120秒、最小指定130秒のhelp／境界test）。
- [x] `nix run .#workspace-lock-check` を追加し、pre-commitのlock検査をambientなCargo／GitとcallerのCargo cacheから排除する（証跡: 固定Nix appの生成、実行ごとの一時Cargo target、`nix flake check --no-build`のapp評価成功）。
- [x] pre-commitのworkspace lock hookを`workspace-lock-check` app経由へ変更する（証跡: 生成済みpre-commit設定のentry読み戻し）。
- [x] 既存`maturin-develop`appをambient venv、network、host configに依存しない専用`target/maturin-venv`へ移行する。productionのtracked `pyproject.toml`とwheel／sdist契約は変更せず、appの隔離source snapshot内だけでMaturin canonical mixed layoutへ補正し、実行ごとの一時Cargo targetで`pokecon/**`のwheelをoffline buildし、専用の永続venv lock下でinstallする（証跡: 2026-07-30のtracked `pyproject.toml`／lock無差分、fresh／同一venv再実行のhostile offline build成功、wheel payload／source byte照合、venv内`pokecon._native` import成功、`.pth`／想定外distribution／破損／symlink／不正RECORD保持negative test、`nix run .#test` 56件成功）。
- [x] **AR-10.11-RUN1** 書込み、watch、hot reload、対象限定testを行うappは一時copyでなくcaller worktreeを対象にする（証跡: 新規worktreeの絶対pathをCargo metadata／target lock、Viteのfile-change／HMR log、hook config／common hook pathからそれぞれ読取った）。
- [x] **AR-10.11-RUN2** 読取り専用完了gateはNix storeの正準sourceまたは隔離した一時copyと、実行ごとの一時Cargo targetを維持する（証跡: callerだけの未追跡`compile_error!`とambient dummy tool／関連変数のpoison下で`contract-check`が成功。caller cacheの`libserde` pathへ置いたpoisonのSHA-256はgate前後とも`f0e766b483a7c4b0631137d317797efe9a9cdd7fd375433b92679ae01202ca6f`で、gateは別の`/tmp/pokecon-rust-gate-home.../cargo-target`を使用した）。
- [ ] **AR-10.11-RUN3** cleanな新規worktreeで追跡済みdirenv entry pointとtool-only `devShells.default`を併用する（予定証跡: clean worktree、4 systemのdevShell評価、tool／環境inventory、入室時のbuild／test副作用0）。
- [x] **AR-10.11-RUN4** `AGENTS.md`、`docs/SPECIFICATION_BACKEND.md`、`PLAN.md`、`README.md`、`docs/DEVELOPMENT.md`、`docs/TROUBLESHOOTING.md`をtool-only devShellと用途別flake appの併用へ更新する（証跡: 6文書を横断検索し、Nix入口、tool-only／no-entry-side-effect、CI・package例外の契約に相互矛盾0を確認。`docs/SPECIFICATION_BACKEND.md:93-95`が正本でroot `SPECIFICATION.md`は索引。`nix flake check --no-build --all-systems` exit 0。既存の`direnv exec .`でcargo／bun／uvがNix storeへ解決する読み戻しも確認済み）。
- [x] **AR-10.11-RUN5** direnvが生成する`.direnv/`をGit管理／配布対象から除外する（証跡: 現行worktreeで`.direnv/`は存在するが`git check-ignore -v .direnv`が`.gitignore:21`へ一致し、`git ls-files -- .direnv`は空。`flake.nix:681-692`のrepository source filterは`.direnv`を明示除外し、product／release sourceは明示path allowlistを使う。`nix run .#source-filter-check`と`nix flake check --no-build --all-systems`がexit 0。clean detached worktree証跡はRUN3に集約）。
- [x] **AR-10.11-RUN6** 既定devShellをtool-onlyに保ち、新しい常駐環境、watch、書込み、長時間処理は用途を限定したappへ追加する規則を文書化する（証跡: `docs/DEVELOPMENT.md:49`、`flake.nix:3467-3478`のtool-only package／export-only shellHook、専用`web-dev`／`hooks-install`／`editor`／`virtual-io-check` task定義。`test_default_devshell_is_tool_only_and_skips_product_builds`は`nix run .#test -- tests/quality/test_ui_package_check.py -k default_devshell_is_tool_only_and_skips_product_builds`で1 passed）。
- [ ] **AR-11-48** 追跡した`.envrc`、tool-only既定devShell、用途別flake appを併用し、host toolchainを開発入口にしない（予定証跡: hostile host PATHの新規worktreeでdirenv／`nix develop`、4対話app、個別gate、aggregate checkが成功）。
- [x] **AR-13.1-22** tracked direnv entry pointと、direnv／`nix develop`／devShellを説明する正規文書がtool-only契約で一致する（証跡: `git ls-files -- .envrc`はtracked pathのみを返し、`.envrc`本文は読み取っていない。`.direnv/`は`.gitignore:21`で除外。`tests/quality/test_devshell_docs_drift.py`が正規文書6件と矛盾fixtureを検査し、`nix run .#test -- tests/quality/test_devshell_docs_drift.py`は8 passed）。

### 受入

- [ ] **AR-13.1-17** 新しいworktreeで`direnv allow`または`nix develop`から固定toolchainへ入り、flake appだけで完了gateまで実行できる（予定証跡: clean detached worktreeの両入口、Cargo metadata、`nix fmt -- --ci`、aggregate check log）。
- [x] **AR-11-49** callerのworktreeを対象とするCargo、frontend dev server、hook導入、editor連携appを揃える（証跡: 4 systemで同一のapp一覧を評価し、新規worktreeで`cargo`、`web-dev`、`hooks-install`、`editor`のcaller smokeに成功）。
- [x] **AR-11-50** 完了gateの隔離実行と、書込みまたはwatch appのcaller-worktree実行を区別し、callerへ書き込むgeneratorもbuild artifactは実行ごとの一時targetへ隔離する（証跡: Cargo／Web／hookはcaller path、editorは`target/nix-editor`、読取りgateとgeneratorはNix sourceまたは隔離copyと実行ごとの`/tmp/.../cargo-target`を使用。未追跡sourceとcache poisonのnegative testに成功）。
- [x] **AR-13.1-18** `cargo` appがcaller worktreeでpackage限定、個別test、lock file更新、metadata確認を実行でき、固定toolchain／build環境がRust完了gateと一致する（証跡: Cargo 1.97.1／Rust 1.97.1を固定し、metadata、`pokecon-contracts`の`validates_http_urls_structurally` 1件、`update --workspace --locked`、`tauri-check`が成功。Cargo.lock差分は0）。
- [x] `nix run .#cargo -- metadata --locked --no-deps` が caller の workspace を読み取る。
- [x] `nix run .#cargo -- test --locked -p <package> <test-filter>` が対象を絞って実行できる。
- [x] **AR-13.1-19** `web-dev` appが固定Bunとlock fileを使い、callerの`web/`の変更をhot reloadし、終了後に依存差分や生成物を意図せずcommit対象へ残さない（証跡: Bun 1.3.13／frozen lockで起動し、callerのSvelte変更と復元を2回のHMRとして観測。前後の`git status --short --untracked-files=all`は空、ignored pathは許可した2 directoryだけだった）。
- [x] **AR-13.1-20** `hooks-install` appを新規worktreeで一度実行し、git hookがNixで固定したpre-commit検査を実行する（証跡: common hookの絶対pathとhardening内容を読取り、hostile環境の署名付きtest commit `fb4dbeb`でlock、clippy、Markdown、Ruff、textlint、treefmt、typosがすべて成功）。
- [x] `nix run .#workspace-lock-check` と`nix run .#ci-watch -- --help`が固定flake appとして動作する。
- [x] **AR-13.1-21** `editor` appまたはNixが出力するlanguage serverだけで、Rust、Python、TypeScript／Svelteの解析がhost toolchainへ依存せず動作する（証跡: 5 executableのstore pathを読取り、host tool／関連環境変数のpoison下を含む`editor-smoke`で4言語すべての実LSP sessionが成功）。
- [ ] **AR-13.1-23** 移行を検証する各worktreeで`.direnv/`がGit管理または配布対象になっていない（予定証跡: direnv読込み後のGit inventory、Nix source、package manifestのnegative report）。
- [ ] **AR-13.1-24** 既存のflake taskをdevShell内と隔離CIから実行し、CI、format、lint、test、build、生成、互換性、packageがambient shell状態へ依存せず移行前と一致する（予定証跡: current checkpointの全task log、生成物Git object ID、package NAR hash）。

### 2026-09-24 旧レビュー資料の扱い

`ARCHITECTURE_REVIEW.md`、`CURRENT_IMPLEMENTATION_REVIEW.md`、`REVIEW_REMEDIATION.md`は、過去の特定時点を対象にしたローカル専用アーカイブです。現行の製品要件、実装状態、進捗、受入証跡として参照しません。

旧finding台帳の基準SHAと集計値には既知の不整合があります。旧件数を現行の進捗へ転記せず、残作業と完了状態は本計画の項目および最新の実行証跡から判断します。

### 現行refactor/rust-coreの追加実装と検証状態

- [x] WebSocket heartbeatの`ping_interval_sec`／`pong_timeout_sec`をproduction runtime settings applierへ接続し、active connectionの待機取消と設定適用時点からの再スケジュールを実装した。
- [x] heartbeatの正値・cross-setting（`pong_timeout_sec <= ping_interval_sec`）検証、invalid updateのatomic rejection、rollback、production wiringをRust testで検証した。
- [x] Windows Package／Release workflowへ署名入力を保持したpayload manifest、clean-install expanded-tree manifest、outer NSIS `.exe`の独立比較とartifact保存を追加した。
- [x] `.direnv/`をGit、Nix repository source、formatter入力から除外し、source-filter／routing／mutation契約を更新した。
- [x] `nix run .#check`（production audit 1件、Web 74件、pytest 460件、Rust 414件、mutation 395件／4 shard）、`nix run .#web-check`、`nix run .#typos`、`nix run .#actionlint`、`nix flake check --no-build --show-trace`、`nix fmt -- --ci`、Clippyが成功した。
- [x] product timingの10-sample nearest-rank p95はGitHub run `34184269204`で実測`710.0s`となった。既定thresholdは実測値を60秒単位へ切り上げる導出（`ceil(710/60)*60 = 720`）としてregistry／workflow／contractへ更新し、次のfresh runで再評価する。
- [x] fresh push後のNormal CIで同一`change_kind`のcompleted timing artifactを保存し、10-sample p95履歴gateをblocking／fail-closedで通過した。Package CIでは実署名Windows runnerのclean install／startup／upgrade／uninstall、payload／expanded tree／outer NSIS installerの三層一致を確認した（Normal `34272925506`、Package `34272925529`）。
- [x] 2026-09-10のAstraレビューで検出されたnative serial close、dynamic controller update／resetのhardware投影、reconnect retry中の明示Disconnectの3件を修正し、Nix経由の`cargo check`、`cargo-test`、native PTY test、Clippy、aggregate check、format、flake checkを成功させ、通常サブエージェントの独立レビューでactionable finding 0を確認した。commit `bec64921ca8d27fbf07bf18f4cea85d9030614cd`のNormal CI（`34453366968`、`34453372575`）とPackage CI（`34453367054` attempt 2、`34453372441`）のrequired checksも全てpassした（attempt 1はNSIS依存取得の一過性`os error 10054`で失敗後、再実行で成功）。
- [x] `PLAN.md`／`TASK.md`だけのplanning-only変更はtiming対象regionなしの`none`として記録し、`ci-timing validate`は継続する。`fast`／`docs`／`product`の履歴が10件以上ある場合はp95 gateをblocking／fail-closedで適用し、10件未満のbootstrap期間はwarning付きでgateを未適用とする（p95の成功証拠には数えない）。
- [x] Windows package／release buildはMSVCの`/Brepro`、debug strip、`/DEBUG:NONE`をbuild boundaryで固定し、既存PE normalizerとpayload／expanded tree／outer NSIS installerの三層比較を維持する。
- [ ] CI上のmock／virtual I/Oでproduction `CameraManager`／`SerialManager`経路の性能artifactとrequired gateを作る。2026-09-24監査: Backend §7.9.6の28.23 ms／3.28 msは1920×1080単発転送の平均参考値（50 warm-up／200 sample）で、production backend用の合否閾値ではない。`docs/ACCEPTANCE.md`の60秒warm-up、各300 sample、p50／p95／maximumと5つの閾値はbrowser primitive用であり、production backend性能受入を代替しない。現concurrent fixture（R640×360、64 serial send、最新frame 8件、時間overlap）はlatency分布／throughput／jitter／raw artifactを測らない。次は1920×1080 known frame、release build、virtual I/O上の本番manager、raw sampleからの統計とSHA／runner／設定付きartifactを追加する。backend絶対閾値、jitter定義、soak時間は規範未定のため製品判断が必要。物理機器の性能試験は作らない。

- [x] 修正済みCI source boundaryを含むdirty worktreeで`nix run .#ci-rust-contracts`がexit 0。PR run `35900770262`の旧失敗は`docs/SPECIFICATION_BACKEND.md`が`rustTestSource`から欠落したことによる（`contract input docs/SPECIFICATION_BACKEND.md must be readable: No such file or directory`）。現在の`flake.nix`はBackend／Frontend／Integration定義書をRust test sourceへ含める。このローカル検証はclean checkpoint別のGitHub CI証跡ではないため、AR-11-41／AR-13.1-03などは未完了のまま。
- [x] 2026-09-25のdirty worktreeで`nix run .#compatibility`がexit 0。成功時の決定的reportは`schema=compatibility-report/1`、`baseline_count=3`、`script_count=103`、`discovered_command_count=98`、`manifest_sha256=c2e4bd4ec3e0c5e3749e9e8ee84b046a477b8da6a2a160ec8a6121a7a461aeaf`、`results_sha256=8443242348120cda654be48715b3e331b1c23831bda841cf1dc959cf6b275cd7`を出力する。これはdirty local gateであり、commit別CI証跡ではない。
- [x] `scripts/ci/nix_evidence.py`がNix 2.34.7 internal-jsonの`@nix` prefix、raw fields、actBuild／actSubstitute／actCopyPathを検査し、fallback無効・command成功・活動のstart/stop完結を満たす場合だけ`capture_complete=true`を出す。Nix実ログ（`nix --option fallback false --log-format internal-json path-info --json .#pokecon`）を`activity_count=66`で読み取り、`capture_complete=true`を確認した。Normal CIはjob-local artifactをダウンロードし、適用jobの証跡欠落／不完全時にfail-closedする。`test_nix_evidence.py` 12件、`test_ci_timing.py` 52件が成功。
- [x] Package CIの意図的な再現性二重build例外を`docs/PACKAGE_REPRODUCIBILITY_EXCEPTIONS.md`へ記録し、`test_package_reproducibility_exceptions.py` 9件で`linux+linux_repro→repro_check`／`windows+windows_repro→windows_repro_check`の依存・比較marker・同一version probe範囲をfail-closed検査する。current-82b Package CIのOS別artifact／install・upgrade・uninstall／manifest証跡は別監査で確認済みだが、Release tagと移行前後matrixは未完了。
- [x] Frontendの`OtherTab` controller position既定`bottom`・2つのposition radio、`WorkspaceMenu`のlegacy LINE Token Assignment／Check（`message`／`noop`）を実装し、Web gateは26 files／112 tests、0 diagnostics、production build成功。BackendのOS-native `auto_reload_config` watcherは未実装のため、関連受入は未完了。

### 旧Phase 1 checkpoint証跡（2026-07-30、devShell方針以外は継続有効）

下表はflake appと隔離gateの履歴証跡である。2026-08-09に上書きされたdevShell／direnv項目の現在完了証跡には使用しない。現行契約では`.envrc`は追跡対象の`use flake`だけを保持し、`.direnv/`は生成後もGit・Nix source・配布対象から除外する。

| 対象 | 実測結果 |
| --- | --- |
| commit境界 | PLAN `93f8b6f`、実装 `23149e3`、受入 `1e0836b`、再現性修正 `7a01da0`。全commit objectにSSH `gpgsig`を確認 |
| fresh worktree | `23149e3`から作成したdetached worktreeで追跡対象の`.envrc`は`use flake`、生成物`.direnv/`はGit・Nix source・配布対象から除外する現行契約へ更新。format、4対話app、desktop、editor、aggregate checkの履歴結果は維持 |
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
- [x] **AR-10.8-01** PokeConを独立再利用部品の集合ではなく一つの製品として設計する（証跡: 下記package/process/artifact図とNix経由workspace member監査）。
- [x] **AR-10.8-02** 将来の独立再利用と交換可能性を設計要件にしない（証跡: 下記「公開Rust library APIとextension pointの移行差分」および移行前後のmanifest／symbol監査）。
- [ ] **AR-10.8-03** 内部設計で単純さ、変更の追跡しやすさ、状態所有の一元化を優先する（予定証跡: ownership table、change-path review、重複状態検査）。
- [ ] **AR-10.8-04** 別process、信頼境界、任意のplatform依存、異なる配布成果物など実際の必要性がある場合だけ強い実装境界を設ける（予定証跡: `nix run .#check`の成果物として保存する境界ごとの必要性／信頼／配布根拠表とarchitecture review）。
- [ ] **AR-10.8-05** 再利用可能性だけを理由にtrait、service層、変換型、crateを追加しない（予定証跡: 追加抽象化inventoryの根拠reviewとcrate数検査）。
- [x] **AR-10.9-02** 任意機能はCargo feature、OS差分はtarget条件、別OS processは複数`[[bin]]`による表現を別crateより先に検討する（証跡: 下記feature／target／binary selection table、Nix経由Cargo metadata、crate-boundary decision record、CLI help gate）。
- [x] **AR-10.9-03** 単一packageで満たせない具体的要件または実測問題が生じた場合だけ別crateを採用する（証跡: `nix run .#cargo -- metadata --locked --no-deps --format-version 1`のworkspace member一件と上記crate-boundary decision record）。
- [x] **AR-10.9-04** 責務の違い、file数、行数、別OS processであることだけを別crate化の根拠にしない（証跡: 下記crate-boundary decision recordと`nix run .#cargo -- metadata --locked --no-deps --format-version 1`のworkspace member読み戻し）。

### 公開Rust library APIとextension pointの移行差分（2026-09-24）

- 移行前のparent commit `1621274585f86cf71c319faa0c78d77fa8ab45e6`（composition-root移行commit `64a7dd5`のparent）のroot `Cargo.toml`は11 workspace membersを宣言し、`rust/pokecon-app/src/lib.rs`は`command_service`、`dynamic_host`、`dynamic_runtime`、`profile_service`、`script_runtime`のpublic moduleと`UiMode`、`AppOptions`、`RunControl`、`RunSummary`、`AppError`、`run`、`run_with_dynamic`、`run_configured`、`run_configured_controlled`を公開していた。
- 同じbaselineの各member `src/lib.rs`にあるcolumn-0 `pub`宣言数はapp 14、contracts 18、core 4、camera 20、device 5、dynamic 17、desktop 8、pybindings 0、server 14、settings 15、worker 9だった。これはroot宣言行の比較値であり、公開項目総数とは区別する。
- 移行後の`nix run .#cargo -- metadata --locked --no-deps --format-version 1`はpackage `pokecon`一つを返す。現行`rust/pokecon/src/lib.rs`が通常re-exportするのは`MainError`と`run_cli`だけで、domain moduleはcrate-privateである。
- `binary_entrypoints`はdoc-hiddenなpublic moduleで、package内binary向けの`worker`／`compatibility`／generator関数だけがfeature-gatedで公開される。`integration_test_support`もdoc-hiddenかつ`integration-test-support` feature-gatedで、integration test用に一部のdomain type／traitをre-exportする。どちらもRustの型検査上はfeature選択後に到達可能だが、製品のsupported extension pointではない。その他のdomain moduleはcrate-privateである。移行前のPython `cdylib`／binding crateはretire commit `8726b23`で削除された。
- 製品の拡張面はCLI、HTTP／WebSocket／WebRTC、worker IPC、user-script／dynamic-config protocolであり、独立再利用のためのRust domain APIやcrate境界を設けない。現行`rust/pokecon` treeに別member／`cdylib`／Python extension attributeはない。

### Crate境界の判断記録（2026-09-24）

- root Cargo manifestはworkspace memberを`rust/pokecon`の一つに限定し、package metadataは`pokecon`一つを返した。
- `rust/pokecon/Cargo.toml`は本体、worker、compatibility tool、fault fixture、contract generatorを複数binary targetとfeatureで構成する。
- 現行構成はworkerを別binaryとして起動するが、本体とworkerは同じCargo packageのbinary targetとして構成しているため、package分割なしに実行ファイルを分けられる。
- 直接根拠はroot `Cargo.toml`のworkspace member、`rust/pokecon/Cargo.toml`の`[[bin]]`／feature、`rust/pokecon/src/entrypoint.rs`のworker asset inventory、`flake.nix`の同一package buildである。
- 現時点では独立versioning／distribution API、固有のsecurity boundary、解けないbuild graph競合、実測済みincremental-build問題を示す要件はない。
- よってworkspaceを単一package `pokecon`に保ち、別crateは上記の具体的要件または計測証拠が成立した場合だけ再検討する。
- 責務名、file数、行数、将来の再利用可能性のみではcrateを分割しない。

### Package、process、artifactの構成図（2026-09-24）

```text
Cargo workspace: one member (`rust/pokecon`)
└── Cargo package: `pokecon`
    ├── product binary: `pokecon`
    │   ├── runtime mode: `--ui web`
    │   └── runtime mode: `--ui desktop` (Tauri)
    ├── supervised worker process: `pokecon-worker`
    │   ├── role: `--kind script`
    │   └── role: `--kind dynamic`
    └── feature-gated auxiliary binaries
        ├── `pokecon-compatibility`
        ├── `pokecon-worker-fault-fixture`
        ├── `generate_contracts`
        └── `generate_openapi`
```

Product binaryとworker binaryは同一Cargo packageから別artifactとしてbuildし、起動時UI modeはproduct binary内で選択する。
Cargo metadataのworkspace member監査結果は`rust/pokecon`一件である。

### Feature、target、binaryの選択記録（2026-09-24）

| 要件 | 現行表現 | 根拠 |
|---|---|---|
| Web／Tauri mode | 同一`pokecon` binaryの起動時`--ui`選択。compile featureや別packageを使わない。 | `entrypoint.rs`の`UiArgument`とCLI help gate。 |
| Worker process | `pokecon-worker` binaryを`worker-binary` featureで有効化する。 | `rust/pokecon/Cargo.toml`と`flake.nix`の同一package build。 |
| Compatibility tool | `pokecon-compatibility` binaryを`compatibility-tool` featureで有効化する。 | `rust/pokecon/Cargo.toml`。 |
| Fault fixture | `pokecon-worker-fault-fixture` binaryを`worker-test-fixture` featureで有効化する。 | `rust/pokecon/Cargo.toml`。 |
| Contract generators | `generate_contracts`と`generate_openapi`を`contract-generator` featureで有効化する。 | `rust/pokecon/Cargo.toml`。 |
| OS固有camera／syscall dependency | Unix、Linux、Windowsのtarget conditionで依存を選択する。 | `rust/pokecon/Cargo.toml`の`[target.'cfg(...)'.dependencies]` sections。 |

Nix経由Cargo metadataはworkspace member `rust/pokecon`一件とpackage `pokecon`一件を返した。
selection tableは既存binary、feature、target条件の記録であり、追加crateや抽象化を導入しない。

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

- [x] `pokecon-settings`を`settings`へ移す。
- [x] settings移行だけの独立構造commitにし、workspace全体の共通完了gateを通す。

#### 2.3a checkpoint証跡（2026-07-31、完了）

下表の完了gateはcaller worktreeまたは実package成果物に対して実行し、すべて終了コード0だった。容量枯渇で停止した初回集約checkは、code failureと区別して復旧と再実行を記録する。

| 対象 | 実測結果 |
| --- | --- |
| 正準source | `rust/pokecon/src/settings/`の14 files（`mod.rs`、`hmac_key.rs`、`lock.rs`、`manifest.rs`、`package.rs`、`path.rs`、`persistence.rs`、`pipeline.rs`、`python.rs`、`roots.rs`、`scaffold.rs`、`service.rs`、`uv.rs`、`venv.rs`）へ物理移動。旧`rust/pokecon-settings/src/`はcompatibility facadeの`lib.rs`だけ |
| 移行compile／型 | `pokecon-settings`は`#[path = "../../pokecon/src/settings/mod.rs"]`で正準sourceを一度だけcompileし、`build.rs`、Cargo.toml、`cross_process` testを維持。package固有`OUT_DIR`でrequirements／managed-uvの2 embedded JSONを生成し、nominal type identityを保持 |
| PokeCon facade | `pokecon`は`#[path = "settings/facade.rs"] mod settings`から`pokecon-settings`の10 moduleを選択的に再export。camera／device／dynamicの下位依存は不変で、旧crate削除は2.7へ延期 |
| 静的境界 | PokeCon-owned sourceの直接`pokecon_settings`は`settings/facade.rs`だけ。正準settings内のbare `crate::<settings module>`は0件で、全内部参照は`crate::settings`。commit前stageは28 paths／13 renames、unstaged／protected diffは0 |
| 保護artifact | Cargo.lock blob `79dddc38a84f7aa61817dc811fe3b5df52a38584`、generated tree `25ad44fa0c95be2254296d2d8e2e4784095d1f52`、OpenAPI blob `a39cab3bee21a37d54661673be901591b8346937`、Python typings tree `e479c1cd5716fdb4c733e2f88dc94354c6986d97`を保持。`flake.nix`も不変 |
| focused test | `pokecon-settings --lib` 49/49、`cross_process` 6/6、`pokecon --lib` 29/29 |
| Rust／契約gate | `contract-check`、全target／feature Clippy、workspace全Cargo test、`build-rust`、`compatibility`がexit 0 |
| Web／成果物gate | `web-check`はSvelte error／warning 0、74 testとproduction buildに成功。`cli-help-check`、`nix build .#pokecon`、`nix flake check --no-build`、`editor-smoke` 4/4もexit 0 |
| 集約／format | `nix run .#check`はPython 63件を含めexit 0。`nix fmt -- --ci`と最終`nix fmt`もexit 0、180 files／0 changed |
| 容量枯渇の復旧 | 初回集約checkはhost filesystemの`No space left on device`で停止し、code assertion failureではなかった。調査したdead Nix store 19,056 pathsだけを`nix store gc`で削除して30.1 GiBを解放し、stage／保護ID不変を再確認後、link／testを含むfull checkを再実行してexit 0 |
| Debian実package | `nix run .#tauri-build -- --bundles deb`、`package-smoke`、`package-install-smoke`がexit 0 |
| package-smoke | amd64 0.1.0、Python 3.14.3、resources 3,801、wheels 28、JS 10、CSS 1、udev rules 3を検査 |
| install-smoke | Ubuntu 24.04でclean install／startup probe／clean shutdown、same-version reinstallの`SkippedVerified`、2回目のstartup／clean shutdown、uninstallまで完了 |
| 構造commit／署名 | `450a85497b1cfcb333bc82210891d8d33ed04fb6`。raw SSH `gpgsig`を保持し、`52419113+yqYo1@users.noreply.github.com`のED25519 fingerprint `SHA256:JPH7BePGgqgXSLXc86mmeGeCzD+BdP1cb63qH4J6Tig`でcryptographic `Good "git" signature`を確認 |
| 最終CI 1/3 | 構造SHAで[Remote Flake 30597614265](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30597614265)、[Package 30597614268](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30597614268)、[Lint 30597614272](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30597614272)がsuccess |
| 最終CI 2/3 | [Rust 30597614281](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30597614281)、[Nix Source 30597614287](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30597614287)、[Pytest 30597614290](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30597614290)がsuccess |
| 最終CI 3/3 | [Basedpyright 30597614293](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30597614293)、[SPA 30597614297](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30597614297)、[Ruff 30597614298](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30597614298)がsuccess。合計9/9 |
| Package CI／artifact | [Package run 30597614268](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30597614268)の[Debian job 91053312802](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30597614268/job/91053312802)は29m53s、artifact digest `sha256:0de38039650e8c87e1286feb765a7185f7f846996f01a8144b5b98ea48fa8062`。[NSIS job 91053312769](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30597614268/job/91053312769)は15m43s、digest `sha256:8dc69f419ea13792b9cc07d8a59de375caa630f9e1a240f5516477ee2376e9da`でsuccess |
| review／監視 | settings構造commitのfresh Sol final reviewはFinding 0。`nix run .#ci-watch -- refactor/rust-core 3600`は120秒settlement後exit 0 |

### 2.3b camera

- [x] `pokecon-camera`を`camera`へ移す。
- [x] cameraの具体的な設定applierを`runtime`の合成境界へ移す。
- [x] cameraから設定永続化、runtime、server、desktopへの依存を排除する。
- [x] camera移行だけの独立構造commitにし、workspace全体の共通完了gateを通す。

#### 2.3b checkpoint証跡（2026-07-31、完了）

下表の完了gateはcaller worktreeまたは実package成果物に対して実行し、すべて終了コード0だった。

| 対象 | 実測結果 |
| --- | --- |
| 正準source | `rust/pokecon/src/camera/`の`mod.rs`と9 domain modules（backend、frame、manager、media、native、screenshot、selector、shared_ring、virtual_camera）へ物理移動。旧`rust/pokecon-camera/src/`はcompatibility facadeの`lib.rs`だけ |
| compile／型境界 | `pokecon-camera`だけが`#[path]`で正準sourceをcompileし、PokeConはprivate `camera/facade.rs`から必要型だけを再exportするためnominal type identityは一つ。移動した`CameraSettingsApplier`はpublic compatibility exportに含めない |
| settings applier | `rust/pokecon/src/settings_runtime/camera.rs`へ移動し、valuesはprivate、applierは`pub(crate)`。local typeへのtrait実装でorphan-safeを維持し、production adapter順序host→desktop→camera→serial→notification→realtimeとrollback挙動は不変 |
| 依存境界 | camera→`pokecon-settings`をmanifestとCargo.lockの厳密に1行から削除。cameraからsettings／runtime／server／desktopへの依存は0で、OS別camera target dependencyとshared-ringのunsafe lint境界を維持 |
| 構造scope | 21 files、10 renames、2 additions、9 modifications、+126/-111。構造commitではgenerated／OpenAPI／Python typings／`flake.nix`／PLANに差分なし |
| 保護artifact | generated tree `25ad44fa0c95be2254296d2d8e2e4784095d1f52`、OpenAPI blob `a39cab3bee21a37d54661673be901591b8346937`、Python typings tree `e479c1cd5716fdb4c733e2f88dc94354c6986d97`、flake blob `84f1dae692191ff4f97de0246abfb8a53b9c1291`、pokecon Cargo blob `e0ece437a5cc2998e07d732e75f9c0e6ba64c57a`を保持 |
| focused test | camera library 22/22、PokeCon library 30/30。all-features focused checkもexit 0 |
| native／isolated test | native V4L2通常gateはhardware依存1件をignored。virtual-io isolated 1/1とserial PTY 1/1は成功 |
| Rust／契約gate | contract、全target／feature Clippy、workspace全Cargo test、Rust build、compatibilityがexit 0 |
| Web／成果物gate | Web 74件、CLI help、`nix build .#pokecon`、`nix flake check --no-build`、editor smoke 4/4が成功 |
| 集約／format | aggregate checkはPython 63件を含めexit 0。最終formatは182 files／0 changed |
| Debian実package | `tauri-build --bundles deb`、package smoke、package install smokeがexit 0 |
| package-smoke | amd64 0.1.0、Python 3.14.3、resources 3,801、wheels 28、JS 10、CSS 1、udev rules 3を検査 |
| install-smoke | Ubuntu 24.04でclean startup／shutdown、same-version reinstallの`SkippedVerified`、2回目のstartup／shutdown、uninstallまで完了 |
| local deb | SHA256 `788d62c90d199db3a33fec0bc3ee977e8a4ef63c4864397b46c35d0122698fab`、197,519,416 bytes |
| 構造commit／署名 | `336aec3b1b10898701c1386eae996f1b676f3b80`。raw SSH signatureを`52419113+yqYo1@users.noreply.github.com`、fingerprint `SHA256:JPH7BePGgqgXSLXc86mmeGeCzD+BdP1cb63qH4J6Tig`として検証 |
| 最終CI 1/3 | 構造SHAで[Nix Source 30604536174](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30604536174)、[Package 30604536176](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30604536176)、[Rust 30604536177](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30604536177)がsuccess |
| 最終CI 2/3 | [Pytest 30604536180](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30604536180)、[Lint 30604536187](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30604536187)、[Basedpyright 30604536193](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30604536193)がsuccess |
| 最終CI 3/3 | [SPA 30604536199](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30604536199)、[Ruff 30604536209](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30604536209)、[Remote Flake 30604536262](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30604536262)がsuccess。合計9/9 |
| Package CI／artifact | [NSIS job 91073990242](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30604536176/job/91073990242)は16m32s、digest `sha256:8238f919c9c70f08ec35016f4010162217dbd4129b191b4a73874161e52dceb9`。[Debian job 91073990263](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30604536176/job/91073990263)は29m58s、digest `sha256:762c9ce5bb685987220fd3e31dc8b4d54573429d0f4e576410ad25790cd47fca`でsuccess |
| review／監視 | camera構造commitのfresh Sol final reviewはFinding 0。`nix run .#ci-watch -- refactor/rust-core 3600`は120秒settlement後exit 0 |

### 2.3c device

- [x] `pokecon-device`を`device`へ移す。
- [x] deviceの具体的な設定applierを`runtime`の合成境界へ移す。
- [x] deviceから設定永続化、runtime、server、desktopへの依存を排除する。
- [x] device移行だけの独立構造commitにし、workspace全体の共通完了gateを通す。

#### 2.3c checkpoint証跡（2026-07-31、完了）

下表の完了gateはcaller worktreeまたは実package成果物に対して実行し、すべて終了コード0だった。

| 対象 | 実測結果 |
| --- | --- |
| 正準source | `rust/pokecon/src/device/`の`mod.rs`と10 domain files（controller、hardware、input、notification、serial配下6 files）へ物理移動し、private `facade.rs`を追加。旧`rust/pokecon-device/src/`はcompatibility facadeの`lib.rs`だけ |
| compile／型境界 | `pokecon-device`だけが`#[path]`で正準sourceをcompileし、PokeConはprivate `device/facade.rs`から必要型だけを再exportするため`pokecon-dynamic`とのnominal type identityは一つ。`SerialSettingsApplier`はpublic compatibility exportに含めない |
| settings applier | behavior-equivalentなadapterをprivate `rust/pokecon/src/settings_runtime/device.rs`へ移し、applierを`pub(crate)`に限定。production順序host→desktop→camera→serial→notification→realtimeとtransaction rollback挙動は不変 |
| 依存境界 | device→`pokecon-settings`の唯一のedgeをmanifestとCargo.lockから削除。正準deviceからsettings／runtime／server／desktopへの依存は0で、外部dynamic engineの所有境界は不変 |
| 構造scope | 24 files、10 renames、3 additions、11 modifications、+301/-252。構造commitではPLAN／generated／OpenAPI／Python typings／`flake.nix`に差分なし |
| 保護artifact | generated tree `25ad44fa0c95be2254296d2d8e2e4784095d1f52`、OpenAPI blob `a39cab3bee21a37d54661673be901591b8346937`、Python typings tree `e479c1cd5716fdb4c733e2f88dc94354c6986d97`、flake blob `84f1dae692191ff4f97de0246abfb8a53b9c1291`、pokecon Cargo blob `e0ece437a5cc2998e07d732e75f9c0e6ba64c57a`を保持 |
| focused test | device library 30/30、`controller_serial_contract` 2/2、実PTYの`native_serial_pty` 1/1、PokeCon library 31/31。all-target／all-feature checkもexit 0 |
| native／virtual I/O | `virtual-io-check`は実serial PTYとV4L2 cameraの両方に成功 |
| Rust／契約gate | `contract-check`、全target／feature Clippy、workspace全Cargo test、`build-rust`、`compatibility`がexit 0 |
| Web／成果物gate | `web-check`はSvelte error／warning 0、74 testとproduction buildに成功。`cli-help-check`、`nix build .#pokecon`、`nix flake check --no-build`、`editor-smoke` 4/4もexit 0 |
| 集約／format | aggregate checkはPython 63件を含めexit 0。最終formatは185 files／0 changed |
| Debian実package | `tauri-build --bundles deb`、`package-smoke`、`package-install-smoke`がexit 0 |
| package-smoke | amd64 0.1.0、Python 3.14.3、resources 3,801、wheels 28、JS 10、CSS 1、udev rules 3を検査 |
| install-smoke | Ubuntu 24.04でclean startup／shutdown、same-version reinstallの`SkippedVerified`、2回目のstartup／shutdown、uninstallまで完了 |
| 構造commit／署名 | `d9f44eea797903da1816f0a38e1d64f800577e2c`。raw SSH signatureを`52419113+yqYo1@users.noreply.github.com`のED25519 fingerprint `SHA256:JPH7BePGgqgXSLXc86mmeGeCzD+BdP1cb63qH4J6Tig`としてcryptographic `Good "git" signature`を確認 |
| 最終CI 1/3 | 構造SHAで[Basedpyright 30612881772](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30612881772)、[Ruff 30612881793](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30612881793)、[Lint 30612881807](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30612881807)がsuccess |
| 最終CI 2/3 | [Remote Flake 30612881818](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30612881818)、[SPA 30612881829](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30612881829)、[Rust 30612881859](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30612881859)がsuccess |
| 最終CI 3/3 | [Package 30612881884](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30612881884)、[Nix Source 30612881946](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30612881946)、[Pytest 30612882994](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30612882994)がsuccess。合計9/9 |
| Package CI／artifact | [NSIS job 91099443258](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30612881884/job/91099443258)は25m06s、artifact digest `sha256:48bd961ca371f326a5eb4aefec77345365e0b080a26330d16a0d5e17f98cdb73`。[Debian job 91099443302](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30612881884/job/91099443302)は28m58s、digest `sha256:33bb13e28938538a39b455638d6a5fcd733ba32230ef6c22368306ed005cf93f`でsuccess |
| review／監視 | device構造commitのfresh Sol final reviewはFinding 0。`nix run .#ci-watch -- refactor/rust-core 3600`は120秒settlement後exit 0 |

### 2.3d server

- [x] `pokecon-server`を`server`へ移す。
- [x] serverのwire型を追加crateにせず内部変換として維持する。
- [x] server移行だけの独立構造commitにし、workspace全体の共通完了gateを通す。

#### 2.3d checkpoint証跡（2026-07-31、完了）

下表の完了gateはcaller worktreeまたは実package成果物に対して実行し、すべて終了コード0だった。対象外の`virtual-io-check`は理由とserver経路の代替証跡を明記する。

| 対象 | 実測結果 |
| --- | --- |
| 正準source | `rust/pokecon/src/server/`の`mod.rs`と22 domain filesへ物理移動し、private `facade.rs`を追加。旧`rust/pokecon-server/src/`はcompatibility facadeの`lib.rs`だけ |
| compile／型境界 | `pokecon-server`だけが`#[path]`で正準sourceをcompileして従来のpublic APIを再exportし、PokeConはprivate `server/facade.rs`から同じmodule／型をaliasする。nominal type universeを一つに保ち、`pokecon`→`pokecon-server`→camera／contractsの非循環依存を維持 |
| wire／OpenAPI境界 | wire型の追加crateは作らずserver内部変換を維持し、wire／OpenAPIの挙動は不変 |
| 内部参照／generator | 正準sourceの内部参照77件を`crate::server`へ変更。PokeCon sourceの直接`pokecon_server`参照はprivate facadeとfeature-gated generatorだけ。OpenAPI generatorをbyte-identicalにPokeCon binへ移し、`required-features = ["contract-generator"]`とscriptの`--package pokecon --features contract-generator`を設定 |
| 依存／構造scope | server crateから`clap`依存と対応するCargo.lock 1 entryだけを削除。37 files、23 renames、2 additions、12 modifications、+295/-278。構造commitではPLANを除外 |
| 保護artifact | generated tree `25ad44fa0c95be2254296d2d8e2e4784095d1f52`、`rust/pokecon/registry` tree `d847c8088b9a1a2e5cc518b80f50cb9f578a169d`、OpenAPI blob `a39cab3bee21a37d54661673be901591b8346937`、Web OpenAPI TS `fdcefc68198a1089a384c2cb0edd58d62e92302f`、Python typings tree `e479c1cd5716fdb4c733e2f88dc94354c6986d97`、flake blob `84f1dae692191ff4f97de0246abfb8a53b9c1291`、root Cargo blob `4dad0e069af0b5d6564fcda985eb99752dbd8ee0`を保持。workflowも不変 |
| focused test | `workspace-lock-check`、server library 66/66、PokeCon library 31/31、OpenAPI check、all-target／all-feature focused checkがexit 0 |
| Rust／契約gate | `contract-check`、Clippy、workspace全Cargo test、`build-rust`、`compatibility`がexit 0。Cargo testはworker lifecycle／IPC／start-stop／force-killを含む |
| Web／成果物gate | `web-check`はSvelte diagnostics 0、74 testsとproduction buildに成功。`cli-help-check`、`nix flake check --no-build`、`editor-smoke` 4/4もexit 0。`nix build .#pokecon`と`nix build .#pokecon-server`は同じ`/nix/store/ym2ngngl3m3hp21y7cfdng0x115dfxzv-pokecon-0.1.0`を返した |
| virtual I/O | `virtual-io-check`はdevice serial PTY／camera V4L2だけを対象としserver／HTTP／WS経路を含まないため対象外。代替としてserver 66 tests、startup tests、Webと実packageのstartupを検証 |
| 集約／format | aggregate checkはPython 63件を含めexit 0。最終formatは187 files／0 changed |
| Debian実package | `tauri-build`、`package-smoke`、`package-install-smoke`がexit 0 |
| package-smoke | amd64 0.1.0、installed 348,986,519 bytes、Python 3.14.3、resources 3,801、wheels 28、native wheel members 257、JS 10、CSS 1、udev rules 3、uv 0.11.8を検査 |
| install-smoke／local deb | Ubuntu Nobleでclean install／startup／dynamic worker clean stop、same-version reinstallの`SkippedVerified`、restart、uninstallまで完了。local `.deb`はSHA256 `3f9111fba2c89b37b084a3d75bcc1f9ffbd832a1a5848f997d2eaa3984452608`、197,482,104 bytes |
| 構造commit／署名 | `46e275b4788e81e8e8f8895012a96fd98ea4f9e9`。`52419113+yqYo1@users.noreply.github.com`のED25519 fingerprint `SHA256:JPH7BePGgqgXSLXc86mmeGeCzD+BdP1cb63qH4J6Tig`でcryptographic `Good "git" signature`を確認 |
| 最終CI 1/3 | 構造SHAで[Package 30622494292](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30622494292)、[SPA 30622494300](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30622494300)、[Lint 30622494310](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30622494310)がsuccess |
| 最終CI 2/3 | [Ruff 30622494319](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30622494319)、[Nix Source 30622494381](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30622494381)、[Pytest 30622494398](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30622494398)がsuccess |
| 最終CI 3/3 | [Rust 30622494410](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30622494410)、[Remote Flake 30622494413](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30622494413)、[Basedpyright 30622494420](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30622494420)がsuccess。合計9/9 |
| Package CI／artifact | [Package run 30622494292](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30622494292)の[Debian job 91130056131](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30622494292/job/91130056131)は29m54s、artifact digest `sha256:f795661238fad76ca4eb5ebfba375001e2e6c75507d0fef36d89da7fff1299ee`、再現した`.deb`のSHA256は`cbb13e3aa3ebe2cc46bd87b495bb78bb682f2372aea0efcaa7bb84bf7406d382`。[NSIS job 91130056300](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30622494292/job/91130056300)は15m30s、artifact digest `sha256:3837866332e4a18675a70b5523384e480e7c99a0368bd6fc1cd32416aafe4827`でsuccess |
| review／監視 | server構造commitのfresh Sol final reviewはFinding 0。`nix run .#ci-watch -- refactor/rust-core 3600`は120秒settlement後exit 0 |

### 2.4 dynamic と worker

- [x] `pokecon-dynamic` の本体側契約／状態を `dynamic` へ移す。
- [x] `pokecon-worker` の IPC、世代管理、親側監督を `worker` へ移す。
- [x] CPython／LuaJIT 初期化と実行を `pokecon-worker` binary 固有 module に隔離する。
- [x] **AR-10.9-06** 同じworker binaryが起動引数`--kind script`または`--kind dynamic`で役割を選択する（予定証跡: `nix run .#cargo -- metadata --locked --no-deps`のtarget一覧と`nix run .#cargo-test`の両role process／IPC smoke log）。
- [x] **AR-11-07** user script workerを自動実行時の機能上の実行主体とし、資源要求を主制御として維持する（予定証跡: script command→IPC request→resource operation traceと実行判断所有test）。
- [x] **AR-11-08** RustメインをOS上の監督兼資源serviceとし、process親子と製品機能上の主従を区別する（予定証跡: ownership／control-flow diagramとsupervisor lifecycle test）。
- [x] **AR-11-09** worker→Rustメインの資源操作要求を主制御、Rustメイン→workerの起動／停止／世代切替を監督制御とする双方向IPCにする（予定証跡: direction／message-kind contractと双方向integration test）。
- [x] 別OS process、双方向IPC、協調停止、強制停止、世代管理を移行前と同じ試験で証明する。
- [x] **AR-13.1-26** worker統合直後、配布された`pokecon`が同じ配布物内の`pokecon-worker`を解決し、profile別環境でscript／dynamic両roleを起動できる（予定証跡: `nix build .#pokecon`成果物を使う対応Nix integration taskのclean-install worker resolutionとprofile別両role smoke log）。

#### 2.4 checkpoint証跡（2026-08-01、完了）

下表の完了gateはcaller worktreeまたは実package成果物に対して実行し、すべて終了コード0だった。

| 対象 | 実測結果 |
| --- | --- |
| 構造commit／署名 | `e25c1add817ea97edd9ec5bb2d7d64fed91ce7e9`、55 files、+1225/-784。`52419113+yqYo1@users.noreply.github.com`のED25519 fingerprint `SHA256:JPH7BePGgqgXSLXc86mmeGeCzD+BdP1cb63qH4J6Tig`でSSH `Good signature`を確認 |
| dynamic正準source | 親側本体を`rust/pokecon/src/dynamic/`へ物理移動。旧`rust/pokecon-dynamic/src/`はcompatibility facadeの`lib.rs`だけ |
| worker正準source | 親側のIPC／世代管理／監督を`rust/pokecon/src/worker/`へ物理移動。子process固有のrun loop、`DynamicEngine`、Python／Lua runtime、script actor／runtimeは`rust/pokecon/src/worker_binary/`だけに隔離 |
| 旧worker／2.7境界 | 旧`rust/pokecon-worker/src/`は`lib.rs`、`main.rs`、compatibility binだけ。target／test ownerとtarget／package統合は2.7まで維持 |
| 型／process境界 | PokeCon本体のprivate facade群が親側のnominal type identityを保持。子側の重複dynamic domainはprivate。別OS process境界ではtyped request／response／event／logと`MappingDescriptor`をframed MessagePack、camera bulk frameだけを`SharedFrameRing`で渡し、stderrは診断専用。interpreter object／native handle／正準状態は越えない |
| role／実行形態 | 同じ`pokecon-worker` binaryが`--kind script`／`--kind dynamic`を選択し、別OS processでPython／Luaを実行 |
| 主制御／双方向IPC | user scriptの判断を起点にcommand→resource request→controller／serial／output operationを実行。worker→Rustメインを資源の主制御、Rustメイン→workerを起動／停止／世代切替の監督制御とする双方向framed MessagePack IPCを確認 |
| lifecycle | generation管理、親側supervisor、協調停止、forced stopをprocess integration testで確認 |
| exact packaged worker | `nix build .#pokecon`は`/nix/store/qizi4ca8xnnzsaf3bv5d5n8fhwwxjv69-pokecon-0.1.0`を生成し、`nix run .#worker-package-check`が成功。同一配布物のexact sibling `execve`、profile別script／dynamic packaged smoke、Python／Lua、双方向framing、controller／serial／output、配布物の協調停止、source-built fixtureのfault policyを確認 |
| Rust／契約gate | workspace all-target／all-feature Clippy `-D warnings`、`build-rust`、`contract-check`、`compatibility`、Cargo metadataがexit 0。`cargo-test`は331 passed、0 failed、V4L2 hardware-only 1 ignored |
| source／release gate | workspace lock、source filter、rust／app／spa source guard、actionlint、release-check、`nix flake check --no-build`、`cli-help-check`がexit 0 |
| Web／editor gate | `web-check`はSvelte diagnostics 0、74/74 tests、production buildに成功。`editor-smoke`は4/4 |
| 集約／Python／format | `nix run .#check`、standalone `nix run .#test`のPython 63/63、`nix fmt`、`nix fmt -- --ci`がexit 0。formatは0 changes |
| Tauri／Debian gate | `tauri-check`、`tauri-build -- --bundles deb`、`package-smoke`、`package-install-smoke`がexit 0 |
| Debian package manifest | amd64 0.1.0、`installed_bytes` 348146823、Python 3.14.3、resources 3801、wheels 28、native members 257、JS 10、CSS 1、udev 3、uv 0.11.8を確認 |
| Debian install／local deb | Ubuntu Nobleでclean install／startup／dynamic cooperative stop、same-version reinstallの`SkippedVerified`、restart、uninstallまで完了。local `.deb`のSRIは`sha256-soHnAyRpuF8ZO68Osl0PJFzkxvqDaLKkLfA2U4Lw4pI=` |
| 保護artifact | generated tree `25ad44fa0c95be2254296d2d8e2e4784095d1f52`、registry tree `d847c8088b9a1a2e5cc518b80f50cb9f578a169d`、OpenAPI `a39cab3bee21a37d54661673be901591b8346937`、Web TS `fdcefc68198a1089a384c2cb0edd58d62e92302f`、Python typings `e479c1cd5716fdb4c733e2f88dc94354c6986d97`、CLI fixture tree `4cdb63b97d36e984a4b36abde175455b7db7eeff`、compatibility tree `4bd11c1cc611e05d2c5926e021be1062cbf1da95`を保持 |
| 最終CI 1/3 | 構造SHAで[SPA 30642068356](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30642068356)、[Pytest 30642068377](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30642068377)、[Nix Source 30642068397](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30642068397)がsuccess |
| 最終CI 2/3 | [Ruff 30642068400](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30642068400)、[Lint 30642068410](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30642068410)、[Rust 30642068424](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30642068424)がsuccess |
| 最終CI 3/3 | [Remote Flake 30642068441](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30642068441)、[Package 30642068473](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30642068473)、[Basedpyright 30642068783](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30642068783)がsuccess。合計9/9 |
| Package CI／Debian | [Debian job 91194230214](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30642068473/job/91194230214)は30m08s。artifact `package-linux-x86_64`、digest `sha256:1aaebb07fd1db6bdc17f6387a5afa7f88360de2dabaed0ddb39d40f15ccf314e`、再現した`.deb`のSHA256 `efa05e3b8f3b29e9f2121343af83d25f1aed8029f961d18a9d5e55602ac162e6`を確認 |
| Package CI／Windows | [NSIS job 91194230294](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/30642068473/job/91194230294)は16m44s。artifact `package-windows-x86_64`、digest `sha256:14342229a41d0de0ed5851286c59df16bfa4b880158eef0e4ec9775583c4a6d7`。clean install、startup 2回、dynamic cooperative stop、same-version upgradeの`SkippedVerified`、`profile_preserved true`、`user_data_preserved true`、uninstallを確認 |
| review／監視 | fresh Sol reviewの初回Low 1は`docs/ARCHITECTURE.md`のownership tableの陳腐化で、構造commit内で修正。follow-up fresh Sol reviewはFinding 0。`nix run .#ci-watch -- refactor/rust-core 3600`は9 workflow success後の120秒settlementを完了してexit 0 |

### 2.5 desktop移行

- [x] `pokecon-desktop`を`desktop`内部moduleへ移す（証跡: canonical実装／iconを`rust/pokecon/`へ移し、同起点のTauri設定を更新した`5fd5aa2`と下記checkpoint。旧crateは2.7までcompatibility facadeだけを維持）。
- [x] **AR-10.9-08** PokeCon本体binaryにWeb UIとTauriの両方を含め、起動引数で表示形態を選択する（証跡: 同一store binary digestを保ったWeb／Tauri起動／停止smokeと下記checkpoint）。
- [x] **AR-11-11（当時の完了判断）** 当時のWeb主要／Tauri下位という相対優先度を含むUI境界を実装・検証した。この優先順位は現在の三つの製品定義書により置き換えられており、Web-first source testは既存Web/Tauri modeの回帰証拠であってGPUIの優先度・受入証拠ではない。
- [x] **AR-11-22** Web UIとTauriを同じPokeCon本体実行ファイルに含め、起動引数で切り替える（証跡: 同一store binaryとinstalled packageを使う`--ui web`／`--ui desktop` startup reportと下記checkpoint）。
- [x] **AR-13.1-27** desktop移行後、Tauri設定、icon、bundle resource、署名対象、Linux package、Windows installerを`rust/pokecon/`起点で生成する（証跡: OS別Package CIのbundle／signing manifestと下記checkpoint）。

### 2.6 `tauri-shell`とPython native extensionの削除

- [x] **AR-10.9-09** 利用者向け`tauri-shell` featureを廃止し、非対応OSのみ内部target条件を使う（証跡: Cargo metadata／残存参照監査、Linux／Windows build matrixと下記checkpoint）。
- [x] **AR-11-23** `tauri-shell`によるWeb専用buildを廃止し、対応OS向け標準成果物に両UI modeを常に含める（証跡: OS別artifact manifest、store／installed packageの両mode smoke、Web-only artifact不在検査と下記checkpoint）。

#### 2.5／2.6 checkpoint証跡（2026-08-06、部分完了）

下表はdesktop配布境界、`tauri-shell`廃止、およびPython native extension削除の実装／通常CI証跡である。AR-10.9-07、AR-11-21とその2子項目、およびAR-13.1-28の実装条件は完了した。tagを起点とするRelease artifactの取得は利用者担当の外部操作であり、本計画のアシスタント残タスクには含めない。

| 対象 | 実測結果 |
| --- | --- |
| desktop実装commit | native shell統合`0df5e8503cca2be83ade51bc315899a5d2b320f8`、icon配布`23d3dffd614a06446463586f4e335824f9ee9786`、canonical module移動`5fd5aa2253578d3216a45bc3dde1b601cc132d08`、exact package検査`204a007cba30722130245b6ab1a6e15022df8cc6`、UI境界`fdf209d995ad49857e22194a60619e6d47f2d3c3`、`tauri-shell`廃止`52540e17780ca76c0b62b0e40de7b540aa41141b`。全commitにSSH signatureを格納 |
| CI改善commit | Debian desktop probe修正`fb2f62362673fe2fcc87059c2eac7d49c535cf06`、重複CI廃止`dad5f45d470f8dbbb765beca2f3f6825f6430cd2`、filtered source具現化`a9484a26ad8487870465cf92222aa6dd4e5ee01a`。全commitにSSH signatureを格納 |
| 当時のWeb-first／Tauri adapter境界 | 当時のRust CI 31008261621はmode capability、Web-first endpoint、Tauri adapter依存境界を検査し、exact `ui-package-check`は15 path／135 method matrixを検査した。これは過去のWeb/Tauri実装証跡であり、現在のUI優先順位やGPUI受入証拠ではない |
| 同一binaryの両UI mode | exact package `/nix/store/zz97vmdh48nfjch9hyaraaaxcz2lgwzq-pokecon-0.1.0`内の同じ`bin/pokecon`を`--ui web`／`--ui desktop`で起動し、両startup reportと停止を確認。Web専用feature／artifactは不在 |
| `tauri-shell`廃止 | Cargo metadataとclosed-world source／release監査で製品feature参照が0、desktop依存がnon-optionalであることを確認。[Rust CI 31008261621](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/31008261621)のLinux all-target／all-featureとWindows checkがsuccess |
| Linux／Windows配布 | [Package CI 31008261238](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/31008261238)が`rust/pokecon/`起点でDebian packageとNSIS installerを生成し、clean install後のWeb／Desktop両modeを検査。Windows installer SHA256は`c1d8d9464cf673b258e9cd8ef897b5ce74f105acd5906635e7c29e8edad5647c`、installed application SHA256は`23323dc4d6bb41fb351d9edcb64842a9f9f696b235f93602002405485cf31db0` |
| artifact／再現性 | Linux artifact digestは`sha256:3a85f82acc5f8d19b808757b2785b2e8f43c8ea5d61aad1626de3e7269c140ed`、Windowsは`sha256:0f7df04fde7ab753944a4a9a9440718e3b99d1ff7e50175d6eaf9fc12295f022`。独立buildした2個の`.deb`はSHA256 `e13954594b84360327cf94f7dd211e10a26ecb77eba020f76d978e8898e34062`で一致し、byte比較もsuccess |
| 必要性監査／責務集約 | Basedpyrightとsource／shell契約はLint、RuffはLint、Rust source検査はRust CI、exact worker／UI／CLI package検査はRust CI package job、OS別install／signing／Debian再現性はPackage CIへ一意に集約。独立SPA workflowは、より強いexact packageの404／route検査へ統合 |
| CI短縮実測 | Rust CIは27m21s→12m52s（14m29s、53.0%短縮）、Package CIは24m56s→18m07s（6m49s、27.3%短縮）、Remote Flakeは15m02s→7m39s（7m23s、49.1%短縮）。単独13m50sだったSPA workflowも廃止 |
| desktop／`tauri-shell`最終CI | [Package](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/31008261238)、[Pytest](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/31008261274)、[Remote Flake](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/31008261451)、[Rust](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/31008261621)、[Lint](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/31008262095)がすべてsuccess。`ci-watch`は120秒settlement後exit 0 |
| Python native extension削除commit | `8726b23142345bc702655ced24f35240f96acbb4`で未使用の`pokecon-pybindings` crate、`pokecon._native`／Maturin build surface、Nixの`build`／`maturin-develop` app、Linux／Windows Releaseのfirst-party native wheel生成／収集を削除。24 files、+828/-845、SSH signature格納 |
| Cargo／source／package不在 | Cargo metadataはworkspace 10 package、`pokecon-pybindings` 0件。SHA固定のtracked path／active marker監査と`source-guard`が成功し、exact package `/nix/store/x9b40b6wa910zhh12pbcma48al16b1jj-pokecon-0.1.0`およびclosure内のpybindings、`_native`、first-party wheelは0件 |
| Python／release negative test | [Pytest 31022369923](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/31022369923)は286 passed、1 deselected。`uv_build==0.11.28`のpure-Python wheelにnative payloadがなく、`pokecon._native` importが`ModuleNotFoundError`となり、native binding marker、first-party release wheel、wheelhouse inventoryの再導入をfail-closedで拒否 |
| Nix artifact差分 | 親commit `f42d15d5e2c00d96ba2559c7c17b47ee96ca712b`比でx86_64-linux appは51→49、削除は`build`と`maturin-develop`だけ。package outputは`default`、`pokecon`、`pokecon-server`、`web`の4件を維持し、native extension／wheel outputは不在 |
| native extensionのOS package成果物 | [Package CI 31022370244](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/31022370244)はOSごとにbundleとsigning manifestの2 filesだけをupload。Linux artifact `8937755264`は`sha256:6df2d5a897f1dad7f59f141bd2ed91c87d158d73092cd45fb302946be3197ac9`、Windows artifact `8937772674`は`sha256:afbed4ff60939232bacea7e60bb21f2e1f9183369645451b45c3b0a5c1673859`で、third-party worker wheelを維持しつつfirst-party wheelは不在 |
| native extension最終CI | `8726b23142345bc702655ced24f35240f96acbb4`の5 workflowは2026-08-06 00:51:05 JSTに開始し、[Pytest](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/31022369923) 00:52:57（1m52s）、[Lint](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/31022369976) 00:57:35（6m30s）、[Remote Flake](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/31022369920) 01:01:41（10m36s）、[Rust](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/31022369942) 01:03:31（12m26s）、[Package](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/31022370244) 01:10:48（19m43s）にすべてsuccess |
| aggregate高速化commit／実測 | 検査lane並列化`b4d172ea70d188d66a55ed4f5756e3c231b4406b`とmutation fixture更新`7c5318746416cc62ad732b023464c71c0c9af20e`。full aggregateの実測は680s→600.9s（-79s、-11.6%）。その後の最終3-worker wave単体は93s→85sで、最終構成のfull aggregateは約593sを見込むが、593sはfull aggregateの実測値ではない |
| aggregate traversal／gate維持 | treefmtのaggregate traversalは117272→380。必要なworkspace／all-features Cargo、pytest／production-routing mutation、package install smokeを維持し、treefmtが既に担当する重複Ruff invocationだけをaggregateから削除 |
| Package critical path | 既存Package jobsはすでに並列実行されている。実packageのinstall／startup／upgrade／uninstall検証は維持しつつ、Windows NSISではclean installのWeb＋exact-title Desktop実起動、upgrade後のWeb／resource検証、user-data保持、同一application SHAを残した。`da16013d6a5bd9c559cf1bdcd5bac7292981b225`で同一byte列に対するupgrade後2回目のDesktop初期化だけを削除し、Web probe 2回／Desktop probe 1回へ短縮 |
| Windows package probeの診断／短縮 | `7c5318746416cc62ad732b023464c71c0c9af20e`では[Pytest](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/31027857129)、[Lint](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/31027856831)、[Remote Flake](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/31027856671)、[Rust](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/31027855547)とPackageのDebian／再現性jobがsuccess。NSISだけがsingle-instance補助窓を`.NET MainWindowTitle`で選んだため、`f4c614926dedf2dadcf266628dcba775083a94cf`でroot PIDの全可視top-level窓を列挙し、exact titleの同一HWNDを連続確認する検査へ修正した。次の[Package](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/31030340585)は60秒内に`com.yqyo1.pokecon-siw`と無題Tao補助窓だけを観測し、列挙修正後の残因がbackend readiness上限と判明。直前successでは実窓readyに44秒／34秒を要していたため、`da16013d6a5bd9c559cf1bdcd5bac7292981b225`で成功時即終了の上限を120秒へ変更し、上記の重複Desktop probeを削除。埋め込みC# compile、Windows smoke 5件、release 142件がsuccess。最終CIは次回pushで取得する |

- [x] **AR-10.9-07** `pokecon-pybindings`を削除する（証跡: Cargo metadataのworkspace 10 package／対象0件、`source-guard`、exact `nix build .#pokecon` package output／closureのpybindings 0件と上記checkpoint）。
- [x] **AR-11-21** `pokecon-pybindings`とnative wheelを削除する移行手順を実施する（証跡: `nix run .#check`のfull aggregate実測600.9s、Python import／wheel negative test、Nix app 51→49のartifact差分と上記checkpoint）。
- [x] Python packageから`_native`のimport、registry、maturin、native wheel生成を削除する（証跡: pure-Python wheel／`ModuleNotFoundError` negative test、active marker 0件）。
- [x] Nix、release gate、成果物一覧からnative wheel参照を削除する（証跡: Nix app／Release workflow差分、first-party wheelを拒否するrelease／package negative test、Package CIのOS別2-file manifest）。
- [x] **AR-13.1-28** Python packageから`_native`のimportとwheel生成を削除し、Nix、maturin、release gate、成果物一覧にnative wheel参照が残っていない（証跡: 許可済み`git grep`のactive marker 0件、`nix run .#test`のPython import／wheel negative test、`nix run .#check`のfull aggregate、release／package negative test、Package CI `34272925529`のOS別manifest。tagを起点とするRelease artifactの取得はこの実装条件の証跡とは分離し、利用者担当の外部操作として扱う）。

### 2.7 旧crateとworkspace参照の削除

- [x] **AR-11-31** workspace memberを`rust/pokecon/`だけにする（証跡: 2026-08-06の`nix run .#cargo -- metadata --locked --no-deps --format-version 1`がworkspace member 1件、manifest `rust/pokecon/Cargo.toml`だけを返した）。
- [x] **AR-11-32** Cargo package名を`pokecon`へ揃える（証跡: 同metadataのpackageは`pokecon` 1件。Cargo.lock、Nix package／check、CI／releaseのpackage identityを閉世界監査するproduction-routing auditと384件のmutation auditが成功）。
- [x] Cargo.lock、Nix、CI、release、installer、文書のpath／package名を更新する（証跡: 正規化したworkspace／lock／flakeのSHA固定監査、`nix flake check --no-build`、exact packageのUI／worker／CLI検査が成功）。
- [x] 各旧crateを削除する前に、そのcrate自身を除くCargo manifest、Nix式、CI、release script、Python build、Tauri設定から参照が消えたことを機械検査する（証跡: crateごとの削除前inventoryとproduction-routing auditのmanifest／path／package negative assertion。削除後の同監査も成功）。
- [x] 旧crate directoryを削除する（証跡: `rust/pokecon-{camera,contracts,core,desktop,device,dynamic,server,settings,worker}`と`rust/pokecon-pybindings`は不在で、正準sourceとtestは`rust/pokecon/`内にある）。
- [x] **AR-10.9-05** `pokecon-pybindings`以外の実装をPokeCon本体1 packageへ統合し、監督／資源service、共通worker、開発用generatorを複数`[[bin]]`で配置する（証跡: metadataの1 packageに`pokecon`、`pokecon-worker`、`pokecon-compatibility`、fault fixture、2 generator binaryがあり、exact packageのCLI／UI／worker smokeが成功）。
- [x] **AR-11-20** PokeCon本体の1 package統合を完了し、共通worker実行ファイルを役割引数付きの別OS processとして起動する（証跡: `rust-ci-core`がsource-built workerのscript／dynamic両role、双方向IPC、Python／Lua、協調停止、fault終了を一度だけ検査し、`worker-package-check`がexact release packageの両role直接起動、productからのexact sibling `execve`、Lua marker、fail-soft拒否、協調停止を検査）。
- [x] **AR-13.1-29** repository全体で旧crate名と`rust/pokecon-*` pathを検索し、明示的に維持する履歴説明以外の参照がない（証跡: 許可済みVCS入口のpattern別検索とproduction-routing auditがactive Cargo／Nix／CI／release／installer／Rust identifier参照0件を確認。残存文字列はPLAN／reviewの履歴、retired-path negative fixture、互換性を維持するdiagnostic／Python namespaceだけ）。

#### 2.7 checkpoint証跡（2026-08-06、local／CI完了）

| 対象 | 実測結果 |
| --- | --- |
| workspace／target | Cargo metadataはworkspace／packageとも`pokecon` 1件。library、main、共通worker、compatibility、fault fixture、2 generator、9 integration-test target、build scriptを同じmanifestから列挙 |
| build／静的契約 | `cargo check --locked --package pokecon --all-targets --all-features`は共有cache再実行でCargo 10.83秒、Nix app全体16.6秒。変更対象の静的test 4件は8.96秒、`nix flake check --no-build`は5.5秒で成功 |
| exact package | `ui-package-check`、`worker-package-check`、`cli-help-check`は同じexact release packageを再利用する。worker gateはscript／dynamic両roleの直接startupと、product経由のexact sibling `execve`、隔離profile、Lua marker、fail-soft拒否、協調停止だけを担当し、source integration testを再compileしない。新gateはcold 521秒、warm 5秒で成功 |
| package test短縮 | 旧worker gateはexact package build後に毎回一時Cargo targetで全依存を再compileしていた。immutable harness化でwarm実行を10秒まで短縮したが、clean runnerではrelease製品とdebug harnessの全依存graphが競合し、local／CIとも約12分超を要した。full integration testは並行する`rust-ci-core`が既に全件実行するためmandatory package gateから重複debug harnessを除き、固有のexact release製品境界だけを残した。単独cold実測は521秒。GitHubの`Verify packaged worker roles`は13分08秒から7分49秒へ5分19秒（約40%）短縮 |
| test選定 | 変更前aggregateでRust／Web／379 mutationとPython 285/286件が成功し、残る1件はrustfmt空白に依存したdesktop source parserだけだった。意味境界へ修正後に対象test、exact package、Nix評価を再実行し、無変更のcold Rust／Web全件は重複実行しなかった |
| 内部依存／ownership | 全Rust production sourceを字句解析する静的監査で、contracts分離、runtime→adapter禁止、8 moduleのruntime逆依存禁止、server／desktopとworkerのowner型禁止、application backendのStateHub／camera／serial／arbiter所有を検査。各分類を破る5 mutationも拒否 |
| mutation監査 | 最終source／flake snapshotに対するproduction-routing mutation 384件を4 process shardで実行し、384/384件を約50秒で検知 |
| 最終外部gate | commit `f292a350e544d81f75a6efd81357614182a0f25f`で[Pytest 31068364663](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/31068364663)、[Remote Flake 31068364681](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/31068364681)、[Package 31068364683](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/31068364683)、[Lint 31068364694](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/31068364694)、[Rust 31068364697](https://github.com/yqYo1/Poke-Controller-Modified-Extension/actions/runs/31068364697)が全てsuccess。`ci-watch`は120秒settlement後exit 0 |

### 内部依存の受入

- [x] `contracts` は他の実行時 module に依存しない（証跡: 全contracts production sourceのroot-module参照監査と`contracts -> runtime` mutation拒否）。
- [x] `runtime` は `server`／`desktop` に依存せず、合成起点が adapter を接続する（証跡: runtime参照監査、production／lib合成検査、`runtime -> server` mutation拒否）。
- [x] `server`／`desktop` は hardware handle、interpreter state、正準controller／device状態を直接所有しない（証跡: owner型／native crate監査とserver hardware-owner mutation拒否。`StateHub`はapplication backendが所有するrevision付きUI projectionとして文書とsource契約を固定）。
- [x] worker は main process 所有の hardware handle／正準状態へ直接アクセスしない（証跡: worker／child path closureのowner型監査、typed IPC／shared-frame integration、worker main-state-owner mutation拒否）。
- [x] `settings`、`camera`、`device`、`worker`、`dynamic`、`contracts`、`diagnostics`、`platform` から `runtime` への逆依存がない（証跡: 8 moduleの全production source参照監査と`settings -> runtime` mutation拒否）。

## フェーズ 3 — 通常 CI の責務とフィードバック時間を再構成

### workflow とゲート

- [ ] **AR-10.10-01** 常に起動する一つの通常CI workflowで変更領域を判定し、安定名の集約gateを必ず完了させる（予定証跡: workflow数、job DAG、全fixtureの集約gate conclusion）。
- [ ] **AR-10.10-02** 文書、契約、Rust、Python、Web、製品smoke、remote flakeを独立した適用領域として判定する（予定証跡: path→領域判定matrixと領域別fixture output）。
- [ ] **AR-11-42** 通常CIの変更領域判定、各jobの所有検査、集約必須gateを確定する（予定証跡: region／job／check ownership matrixとworkflow fixture report）。
- [ ] **AR-10.10-03** workflowを常に起動し、workflow-level path filterで全体を省略しない（予定証跡: trigger定義と各single-area fixtureのworkflow run）。
- [ ] **AR-10.10-04** 不要な領域jobは成功扱いで明示的に省略し、必須gateを不定にしない（予定証跡: 非該当fixtureのskip reason／conclusionとaggregate output）。
- [ ] **AR-10.10-05** feature commitはpull request、既定branch／明示的な統合branchへの直接反映はpushで検査し、同じPR head commitを両eventで重複検査しない（部分証跡: `normal-ci.yml:4-7`／`package.yml`はpushを`main`／`master`に限定し、PR baseに`refactor/rust-core`を含む。`test_ci_trigger_dedup.py`はNix経由12 passed。GitHubの[複数event規則](https://docs.github.com/en/actions/writing-workflows/choosing-when-your-workflow-runs/triggering-a-workflow#using-multiple-events)では`push`と`pull_request`は独立にworkflow runを起動し、[branch filter](https://docs.github.com/en/actions/writing-workflows/choosing-when-your-workflow-runs/triggering-a-workflow#using-filters-to-target-specific-branches-for-pull-request-events)は前者でpush ref、後者でPR baseを対象とするため、静的`on:` filterだけでは直接push検査と重複ゼロを両立できない。また、[push](https://docs.github.com/en/actions/writing-workflows/choosing-when-your-workflow-runs/events-that-trigger-workflows#push)の`GITHUB_SHA`はpush先端、[pull_request](https://docs.github.com/en/actions/writing-workflows/choosing-when-your-workflow-runs/events-that-trigger-workflows#pull_request)の`GITHUB_SHA`はmerge commitであり、比較対象はPR `head.sha`と定義する必要がある。現設定はPR head同期の重複を避けるが、PRなし`refactor/rust-core` pushにCIを起動しない。active `protect` rulesetは`~DEFAULT_BRANCH`だけを含み、branch APIの`protected`はfalseのため、PR-only保護は未設定である。所有者がdirect pushを禁止してbranch protectionを強めるか、重複実行の危険を受け入れてbest-effort routingを許すか決めるまで未完了。same-head SHA比較の現行run証跡も未取得。
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
  workflowには署名検証とtrusted actor限定のpush write条件がある。
  Repository secrets／environmentsの読み戻しでは署名credentialを確認できず、任意exportはskipされるため、trusted-push upload証跡がなく未完了である。
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

- [ ] **AR-10.4-FUNCTIONS** 機能の優先順位を理由に製品定義書群の機能を不要または省略可能と判断しない（予定証跡: specification機能→実装／受入testの全件matrixと未実装数0）。
- [ ] **AR-10.4-QUALITY** 後回しの機能も競合がない状態で定義済みの低遅延性、性能、安定性を満たし、優先度を品質要件緩和の理由にしない（予定証跡: CI上のmock／virtual I/Oによる非競合時の機能別latency／throughput／stability report）。browser primitive smokeは実装済みだが、production主経路の機能別latency／throughput／stability reportは未完了。
- [x] **AR-11-01** 定義書に記載する全機能の役割を実装と受入testへ対応付けた（証跡: `docs/FUNCTION_TRACEABILITY_MATRIX.md` §2-§3の全backend／frontend／integration section coverage、機能／役割／owner／実装source／受入test／status／gap matrix）。部分／未実装／外部受入待ちを含む機能は完了扱いせず、次のowner／外部証跡を§5へ分離した。
- [ ] **AR-11-02** 機能同士が資源を競合した場合の処理優先順位をqueue、lock、task、threadに反映する（予定証跡: contention matrixと順序／飢餓／逆圧stress report）。
- [x] **AR-11-03** 停止、全入力解放、neutral状態送信を最上位優先経路で処理する（証跡: `cargo test --locked --workspace --all-targets --all-features` の input／notification／worker lifecycle fault tests）。
- [ ] **AR-11-04** 通常運転時のserial出力と画像認識を同じ高優先度で独立して進める（部分証跡: `rust/pokecon/tests/concurrent_camera_serial_load.rs`はproduction `CameraManager`のlatest-frame consumerと`SerialManager`のvirtual writerを同時実行し、64回のserial write、serial期間中8 distinct frame以上、時間区間の重なり、camera継続稼働を検査する。focused testは1 passed。さらに`nix run .#ci-rust-contracts` exit 0でRust unit 475件と同test 1件が成功した。これはprogress／overlapのsoftware-only証拠であり、画像認識処理、latency／jitter分布、throughput／stability reportを測定せず、要求全体は未完了）。
- [ ] **AR-11-05** 明示的優先機構の導入前後で遅延、jitter、飢餓、queue停滞を比較し、改善しない機構は採用しない（予定証跡: before／after benchmarkと採用判定record）。
- [ ] **AR-11-06** command実行、画像認識、serial出力を結ぶ主経路を実装する（予定証跡: end-to-end command→frame→controller／serial traceとlatency report）。
- [x] camera writerのshutdown timeout時にdurable `camera_writer_unstopped`を記録し、`UnstoppedCameraWriter`／production fallbackで既存mappingを保持する（証跡: `rust/pokecon/src/camera/manager.rs`のtimeout回帰test、POSIX `shm_unlink`後も既存mappingを読めるassert、production fallback test。`nix run .#cargo-test`、`nix run .#check`、`nix run .#clippy`、`nix run .#build-rust`が現行差分でexit 0。reader-pin全体の監査とproduction性能測定は別項目として未完了）。
- [x] **AR-11-12** UI制御要求を表示配信より優先し、映像、状態、logの滞留を主経路へ伝播させない（証跡: WebSocket の slow-client、latest-frame、control-latency、bounded-log tests）。
- [x] frame は最新の完全 frame へ追従し、状態更新は同一項目の旧値を集約する。
- [x] log 配信を有界 queue とし、低速／切断 UI から主経路への逆圧を防ぐ。

### 4.2 手動介入と入力調停

- [x] **AR-11-14** script実行中の手動介入を許可または拒否でき、停止と入力解放は常に受け付けるcanonical settingを追加する（証跡: settings registry／SPECIFICATION／generated OpenAPI・UI・typing drift tests、input arbitration tests）。
- [x] CLI、TOML、環境変数、Web UI の全表面へ生成／投影する。
- [x] 既定は介入許可とし、適用後の入力から即時反映する。
- [x] 排他 mode でも停止と全入力解放を常に受け付ける。
- [x] **AR-11-15** 介入許可時は操作中の入力要素だけを一時上書きし、操作終了後にscript入力へ戻す（証跡: button／stick／touch の element-level arbitration tests）。
- [x] button の script／manual 合成規則を試験する。
- [x] **AR-11-13** 手動操作が自動scriptへ不意に影響せず、手動単独利用時も低遅延かつ安定して動作する（証跡: denied/manual isolation、manual-only handoff tests）。

### 4.3 profile切替

- [x] **AR-11-16** 新規command受付停止、実行中script停止、全入力解放、旧worker終了後にprofile／関連設定を一括切替し、旧commandを自動再開しない（証跡: profile switch lifecycle、stop/reap、idle/no-auto-restart integration tests。追加で最新commitの隔離Web runtimeに対し、`active_profile` PATCHの`default`→`Other`→`default`、`revision` 3→4→5、`available_profiles` read-back、profile設定`ui.fps=60`／`ui.fps_options=[5,15,60]`を確認）。
- [x] 新規 command 受付停止 → 実行中 script 停止 → 全入力解放 → 旧 worker 終了の順序を保証する。
- [x] profile と関連設定を一括切替し、新 worker 環境を初期化する。
- [x] 切替後は旧 command を自動再開せず idle で明示実行を待つ。
- [x] **AR-11-17** profile切替失敗時は旧設定だけを復元してidleへ戻し、復元失敗時は安全停止を維持して新しい実行を拒否する（証跡: validation／commit／stop／restore failure-injection tests）。
- [x] 復元失敗時は安全停止状態を維持し、明示復旧まで新規 command を拒否する。
- [x] 失敗段階を UI と log へ明示する。

### 4.4 動的設定の候補世代切替

- [x] **AR-11-18** 動的設定を候補世代で構築し、全読込み／検証成功時だけ自動実行を止めずに一括切替する（証跡: 本番経路 `EvaluationTransaction::begin` → `commit_evaluation`/`commit_staged`（`rust/pokecon/src/worker_binary/dynamic/engine.rs`）の隔離評価と `EngineInner::generation` 原子世代管理、`CommandRegistry::build_display_cache` の完全キャッシュ逐次構築・`Superseded` 判定と `StartupDynamicHost::publish_command_cache`/`CommandDisplayCache` 原子一括公開（完全性）、失敗時 generation 不変と typed `dynamic_config_evaluation_failed` diagnostic、並行/superseded と `coordinator`（`AsyncMutex`）ロック隔離は `configuration_is_current` と `EngineInner::generation` で担保。`DynamicReloadCandidate`/`begin_reload_candidate`/`commit_reload_candidate`/`abort_reload_candidate`/`reload_generation`/`DynamicReloadStatus`/`try_commit_isolated_reload`/`build_reload_candidate` の別配線は不要で削除済み。既存テスト `dynamic/transaction` の隔離/rollback、`dynamic_host` の `publish_command_cache` 原子性、`dynamic/command` の `display_cache_is_complete_and_orders_manual_tags_before_automatic_tags`/`display_cache_discards_a_generation_changed_during_callback_execution`/`old_callbacks_finish_in_old_generation_while_new_candidate_builds` で網羅）。
- [x] reload 中も現世代を有効に保ち、自動 script を停止しない。
- [x] Python／Lua 設定、callback、command 一覧を候補世代として構築する。
- [x] 読込みと検証が全成功した場合だけ原子的に切り替える。
- [x] 実行中の旧 callback は旧世代で完了させる。
- [x] 失敗時は現設定を変更せず、UI と log へ通知する。
- [x] reload が camera、serial、script 主経路を待たせないことを検証する。

### 4.5 通知隔離

- [x] **AR-11-19** 通知を有界queueと期限付きretryへ隔離し、主経路へ待機と障害を伝播させない（証跡: queue-full、slow/failing provider、retry deadline、stop-priority tests）。
- [x] 通知要求を有界 queue へ入れ、主経路 lock を保持せず処理する。
- [x] queue 上限時は主経路を待たせず、呼出元へ明示的失敗を返す。
- [x] retry 回数と期限を制限する。
- [x] 完了待ちは当該呼出しだけに限定し、camera、画像認識、serial、他要求を継続する。
- [x] 外部通知障害を script worker 全体へ伝播させない。
- [x] 停止と全入力解放を通知処理より優先する。

## フェーズ 5 — 配布、文書、最終監査

- [x] Windows／Linux の標準成果物が Web／Tauri 両 mode、本体、worker、Web 資源、uv、管理 Python を含む（証跡: Package CI `34272925529`のLinux／Windows bundle、clean-install smoke、runtime／resource manifest）。
- [x] non-Nix 配布、clean install、upgrade、uninstall、再現可能性、署名対象を検証する（証跡: Package CI `34272925529`の実署名Windows runner、Linux／Windows package smoke、payload／expanded tree／outer NSIS installer比較、signing manifest）。
- [x] 2026-09-14のcommit `50bd589ba67ec801f9be4579c7062984d9577b2f`で隔離Web runtimeの外部hardware I/Oなし受入を再実行した（証跡: clean root command reload `200`、profile switch／read-back、stale revision `409`、Webhook secret-safe read projection）。ブラウザbackendの`410 Gone`により、同runのブラウザ操作とtailnet越しWebRTC primary映像は未完了として残す。
  - 2026-09-24 source-only follow-up（current runtime acceptanceではない）: `rust/pokecon/src/production.rs:309-332`は空の`Commands` rootを作成し、`CommandService`／`ProfileService`をrouter公開前にinstallするため、旧clean-root command reload失敗は現行source上では起動欠落が見つからない。`web/src/lib/components/CommandsTab.svelte:393`のReload buttonは空listでもenabledであり、実HTTP成功は未検証。live browser trialはskill必須のclean-repo cleanup conditionを満たせない（既存staged／dirty差分を保全し、別worktreeも作らない制約）ため未実施。
  - 同follow-up: `dynamic_host.rs:915-917`の`profile_list()`は実ディレクトリを再列挙する一方、UIの`WorkspaceMenu.svelte:76-80`は`StateSnapshot.available_profiles`を表示し、`refresh_available_profiles()`はlauncherによる新規作成時（`application_backend.rs:811-815`）にしか呼ばれない。実効Configルート配下のprofile directoryを実行中に外部作成／削除した後、UI listを更新する操作とその契約がないため、profile discoveryのlive refreshは未受入・未解決。`nix develop --command cargo test --locked -p pokecon dynamic_host::tests::profile_state_and_controller_are_rust_owned`は1 passedだが、internal host refresh pathのみでREST／SPAは検証しない。`nix run .#web-check`は26 test files／106 tests、Svelte diagnostics 0 errors／0 warnings、production build成功だがlive browser acceptanceの代替ではない。
- [x] README と利用者／開発者文書を最終実装へ同期した（証跡: `docs/DOCS_SYNC_AUDIT.md` §2-§4でCLI、INSTALL、USER_GUIDE、DEVELOPMENT、source／flake task、CI／local gateを照合。未受入のGPUI Phase 1、auto_reload watcher、browser、性能、実機、Releaseは実装済みと記載していない）。
- [x] architecture handoffに対応する実装または受入証跡を、本計画と製品定義書の要件ごとに列挙した（証跡: `docs/TRACEABILITY_INDEX.md` §2-§4がroot仕様、三定義書、ARCHITECTURE、PLANの6文書とbackend／frontend／integrationの全sectionを、各詳細traceability表の要件行へ対応付ける）。
- [x] 本計画に展開したarchitectureの各受入条件へ直接の証拠があることを監査した（証跡: `docs/TRACEABILITY_INDEX.md` §3のsection／要件対応表、§4のarchitecture／package／外部受入接続、§5のcheckpoint共通gate対応）。未成立のclean worktree、performance、browser、hardware、Release、owner decisionは§6へ明示した。
- [x] 製品定義書群の対象機能を要件別に照合し、未検証項目を「暗黙に成功」と扱わない監査を完了した（証跡: `docs/TRACEABILITY_INDEX.md` §3の三定義書詳細表と§6の未実装／外部証跡待ち一覧）。
- [ ] 全共通完了ゲートを clean worktree で再実行する。
- [ ] Sol 役のコンテキストを切った辛口レビューを受け、重大・高・中の指摘をすべて解消する。
- [x] b719ad5d126bb88f7260fb829a88bcae2246914e push後のfresh GitHub CIを完了まで監視し、同一SHAのNormal CI run `36096392708`／Package CI run `36096392743`がcompleted／successとなったことを確認した。Normal CIのrequired aggregate job `107951886810`を含む全11 job、Package CIの全6 jobがsuccess。Normal artifact `ci-timing-36096392708-1`はproduct 10-sample p95 `674.0s`／threshold `720.0s`／violations 0を含み、job-local Nix evidenceはRust 4 built derivations・295 substituted paths、Product 392・1033、Remote 2・97を`capture_complete=true`で記録した。別eventの重複runは成功証跡へ二重計上しない。
- [x] `PLAN.md`の未完了checkboxを要件別に監査し、完了証跡または具体的な残タスクへ更新する。利用者担当の外部操作はこの監査対象のアシスタントタスクに含めない（証跡: 現行89件を実装／文書、実装時選択、owner判断、外部／運用証跡へ分類し、各行の予定証跡または具体的な次作業を確認。`docs/TRACEABILITY_INDEX.md` §6、`docs/GPUI_PHASE0_INVENTORY.md` §5-§6、`docs/FUNCTION_TRACEABILITY_MATRIX.md` §5-§6、`docs/DOCS_SYNC_AUDIT.md` §4と照合。未完了要件、clean worktree、Solレビュー、性能、browser／実機、Release tagは未完了のまま保持。）
