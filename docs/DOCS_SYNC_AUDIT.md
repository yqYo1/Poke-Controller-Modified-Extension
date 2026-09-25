# 文書同期監査

## 1. 目的と判定境界

この文書は、現行実装と利用者向け文書・開発者向け文書・CLI helpの同期状態を監査した記録です。

要件の正本は`SPECIFICATION.md`と三つの定義書であり、この文書は要件を変更しません。
文書に修正が不要だった項目も、実装との照合結果として記録します。

次の項目を文書同期の完了条件とします。

- 起動mode、CLI option、default値がCLI helpと一致する。
- 配布対象、起動形、保存場所、終了動作がsourceと一致する。
- 利用者向け操作が現在のWeb UI／Tauri UIの主要操作と一致する。
- 開発者向けgate名、Nix入口、source filter、remote CI手順が現在のflakeとworkflowに一致する。
- 未実装、対象外、外部受入待ちを実装済みと書かない。

この監査は外部browser、実機、production性能、Release tagの受入を代替しません。

## 2. 同期対象と直接根拠

| 対象 | 文書 | 直接根拠 | 判定 |
|---|---|---|---|
| CLI／起動mode | `docs/INSTALL.md`、`docs/USER_GUIDE.md`、`tests/fixtures/cli-help/pokecon.txt` | `rust/pokecon/src/entrypoint.rs`、CLI fixtureの`--ui web|desktop`と`--exit-after-startup` | 同期済み |
| 配布範囲 | `docs/INSTALL.md` | Linux／Windows x86-64、Web／Tauri、Python／uv／wheelhouse、macOS／PWA対象外 | 同期済み |
| 保存場所 | `docs/INSTALL.md`、`docs/USER_GUIDE.md` | XDG Config／Data／Cache／State、Windows AppData、`app_name`分離 | 同期済み |
| Web／Tauri | `docs/INSTALL.md`、`docs/USER_GUIDE.md` | `web`／`desktop`の同一backend、REST、WebSocket、WebRTC、設定transaction | 同期済み |
| Camera／Serial／Controller | `docs/USER_GUIDE.md` | `web/src/lib/components/`のCamera、Serial、Manual Control、Commandsの実装とtest | 同期済み |
| 画像経路 | `docs/USER_GUIDE.md` | WebRTC primary、Motion JPEG fallback、Retry WebRTC／Retry cameraのUI surface | 同期済み。ただしlive browser受入は未完了 |
| 終了動作 | `docs/INSTALL.md`、`docs/USER_GUIDE.md` | `ask`、`shutdown`、`keep_backend`、tray Quit、Web process終了 | 同期済み |
| 開発toolchain | `docs/DEVELOPMENT.md` | Nix devShell、Bun、Rust、Python、source filter、remote flake smoke | 同期済み |
| 開発gate | `docs/DEVELOPMENT.md` | `markdownlint-check`、`textlint-check`、`typos-check`、`web-check`、`contract-check`、`release-check` | task名をflakeで照合済み |
| 配布gate | `docs/INSTALL.md`、`docs/DEVELOPMENT.md` | `tauri-build`、`package-smoke`、`package-install-smoke`、Package CI | 引数付きtaskとして同期済み |
| GPUI | `docs/GPUI_FRONTEND_PLAN.md`、`docs/GPUI_PHASE0_INVENTORY.md` | Phase 0完了、Phase 1／Gate 1／native selector未実装 | 将来扱いで同期済み |
| 動的設定 | `docs/ADVANCED_USAGE.md`、`docs/DYNAMIC_CONFIGURATION.md` | `auto_reload_config`の設定surfaceと現在のreload経路 | OS-native watcherは未実装として扱う |

## 3. 実行した検証

次のlocal gateをNix経由で実行し、exit 0を確認しました。

- `nix run .#cli-help-check`
- `nix run .#contract-check`
- `nix run .#check`
- `nix run .#release-check`
- `nix run .#markdownlint`
- `nix run .#textlint`
- `nix run .#typos`
- `nix run .#source-filter-check`

最新local aggregateは574 passed、2 deselected、Web 26 files／112 tests、Svelte diagnostics 0、production-routing mutation 395/395でした。

同じsource baselineのb146e30ではNormal CI `36141338540`とPackage CI `36141338643`がsuccessしました。
その後のPhase 0文書更新を含む74a8624では、Normal CIの初回attemptで`overlay_pointer_events_run_fifo_callbacks_without_blocking_ipc`が一度だけforced stopになりましたが、同一SHAのattempt 2でRust／contract job、timing p95 gate、Normal aggregateがsuccessしました。
同SHAのPackage CI `36145567618`もDebian／Windows bundle、clean-install、独立reproducibility build、byte comparison、aggregateまでsuccessしました。
同じfocused script runtime全18テストはlocalで5回連続successし、初回の一過性failureを実装修正済みとは扱っていません。

## 4. 同期済みと未受入を分ける

次の項目は文書へ実装済みと記載していません。

- GPUI native view、GPUI Cargo依存、`--ui gpui`、`apps.gpui`、Gate 1-6。
- `auto_reload_config`のOS-native file watcher。
- browserでのWebRTC primary、Motion JPEG fallback、再昇格、keyboard／accessibility受入。
- production主経路のlatency、throughput、jitter、soakの製品閾値受入。
- 実camera、実serial、実MCUを使うhardware acceptance。
- `v*` Release tag作成とtag-triggered Release publication。
- clean detached worktreeを必要とするlive browser acceptance。

CI、virtual I/O、REST read-back、Svelte test、Package reproducibilityは、上記未受入の代替証拠ではありません。

## 5. 更新規則

CLI option、設定field、mode、保存場所、配布範囲を変更した場合は、source、CLI fixture、該当guide、`docs/TRACEABILITY_INDEX.md`を同じ作業で照合します。

要件の意味を変更する場合は、利用ガイドやPLANだけを先に変更せず、対応する定義書と受入条件を更新します。

新しい文書を追加した場合は`docs/README.md`の入口と`docs/TRACEABILITY_INDEX.md`の6文書対応表を更新します。

この監査artifactの存在は、未受入項目の完了を意味しません。
