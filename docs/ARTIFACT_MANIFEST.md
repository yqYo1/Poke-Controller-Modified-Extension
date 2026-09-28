# 成果物 manifest と OS 別 clean-install 記録

本書は `AR-11-38` の予定証跡「artifact manifest と OS 別 clean-install report」に該当する。

正は `.github/workflows/package.yml`、`.github/workflows/release.yml` と `scripts/release/` の各 script であり、
本書は現行成果物、provenance、OS 別受入の読み戻し記録だけを担う。workflow 自体の変更は行わない。

## 配布成果物の一覧

| OS | 成果物 | 生成 command | provenance | 配布先 |
| --- | --- | --- | --- | --- |
| Linux | `dist/tauri/*.deb`（artifact `package-linux-x86_64`） | `nix run .#tauri-build -- --bundles deb` | `signing-inputs.json`（Linux signing inputs）、clean-install smoke log | Package CI artifact、Release workflow（tag 起点、利用者 deferred） |
| Windows | NSIS `*setup.exe`（artifact `package-windows-x86_64`） | `nix run .#tauri-build -- --bundles nsis`（offline、PE metadata 正規化） | `signing-inputs.json`（policy `schema_version=1`、`canonical_sha256`、file `sha256`／size）、`windows-payload-manifest.json`、`windows-install-tree-manifest.json` | 同上 |
| 共通 stage | web 配布物、managed runtime、offline wheelhouse、`pokecon-worker` | `package-smoke`／`package-install-smoke` で監査 | `resource-manifest.json`（`content_sha256`）、SPA 検証、wheelhouse 検証 | 各 installer に同梱 |
| Nix | `.#pokecon`、`.#web` ほか flake apps | `nix build` | store path（build log） | 開発・CI 内 |

## provenance manifest 群

- `scripts/release/signing_manifest.py` は signing-inputs の policy、canonical SHA-256、file 単位 digest／size を記録する。
- `scripts/release/gate.py` は tool versions と SHA-256 を記録する。
- `scripts/release/package_smoke.py` は resource-manifest、SPA、wheelhouse を検証する。
- `scripts/acceptance/ci_check_inventory.py` は workflow 内の flake check app 呼出 site を (app, workflow, context) で inventory 化し、重複 0 と例外理由を検査する。
- `windows-install-tree-manifest.json` は clean-install 後の展開 tree を記録し、NSIS 再現性の三層比較に使う。

## OS 別 clean-install／再現性 report（読み戻し）

参照 run: Package CI `36353061684`（`pull_request`、head `fec66d9`、8/8 jobs SUCCESS）。

| job | ID | 内容 |
| --- | --- | --- |
| Debian bundle and clean-install smoke | `108715822568` | `.deb` 生成、package smoke、clean install／offline startup／upgrade／uninstall 検証 |
| NSIS bundle and clean-install smoke | `108715822604` | NSIS installer 生成、Windows での同検証 |
| Verify Debian package reproducibility | `108718837112` | primary／reproduction の byte-for-byte 比較 |
| Verify Windows NSIS reproducibility | `108720611250` | 外側 `.exe`、payload manifest、install tree の三層比較 |
| Required | `108720714749` | 集約 gate |

再現性検査の意図的な second build は [`PACKAGE_REPRODUCIBILITY_EXCEPTIONS.md`](PACKAGE_REPRODUCIBILITY_EXCEPTIONS.md) を正とする。

### 既知の flake 注記

`d644eeb` の `pull_request` run `36359352644` では、Windows NSIS 再現性検証だけが失敗した。

失敗は `pokecon-worker` の稀な codegen 非決定性（154 bytes 差、正準値は他 run で再現）であり、
同 run の clean-install smoke は成功している。

製品成果物の内容差を示す証拠はなく、継続観測の対象とする。

その後、`2d4baa7` の full build（Package CI run `36364102715`）で Windows NSIS 再現性検証は success を再確認しており、一過性の flake として扱う。

## Release との分離

`release.yml` は tag push を起点に `release-check --tag`、package smoke、install smoke、signing-input manifest を実行する。

`package.yml`（branch push／pull request 起点の検証）とは役割を分離している。

Release の実行実績はまだない。tag 作成は利用者 deferred であり、未実施は正当である。
