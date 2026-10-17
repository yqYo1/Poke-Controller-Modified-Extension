# Package 再現性検査の意図的 second build 例外 inventory

`package.yml` が同一入力から package を二回生成する再現性検査は、
意図した重複である。通常 CI で進められた重複削減の対象外とする。
本書は **AR-10.10-PACKAGE-REPRO** の予定証跡のうち「重複例外 inventory」に該当する。
規範は `.github/workflows/package.yml` が正であり、本書と
`tests/quality/test_package_reproducibility_exceptions.py` は inventory の記録と
回帰検出だけを担う。workflow 自体の変更は行わない。

## 例外ペア一覧

| # | primary job | second build job | 比較 job | 比較方法 | 比較対象 artifact |
| --- | --- | --- | --- | --- | --- |
| 1 | `linux` | `linux_repro` | `repro_check` | `nix run .#package-reproducibility-check -- primary reproduction` による byte-for-byte 比較 | `package-linux-x86_64` と `package-linux-x86_64-reproducibility` の `.deb` |
| 2 | `windows` | `windows_repro` | `windows_repro_check` | `sha256sum` 表示＋`cmp` による三層比較 | `package-windows-x86_64` と `package-windows-x86_64-reproducibility` の外側 NSIS `.exe`、`windows-payload-manifest.json`、`windows-install-tree-manifest.json` |

## ペア 1: `linux` ／ `linux_repro` ／ `repro_check`

- `linux`（Debian bundle and clean-install smoke）は `nix run .#tauri-build -- --bundles deb` で
  `.deb` を生成し、`package-smoke`（packaged runtime と offline wheelhouse の監査）、
  `package-install-smoke`（clean install、offline startup、upgrade、uninstall の検証）、
  Linux signing inputs の記録と handoff 時検証を行い、
  `dist/tauri/*.deb` と `dist/tauri/signing-inputs.json` を
  `package-linux-x86_64` として upload する。
- `linux_repro`（Independent Debian reproducibility build）は同一入力から
  `nix run .#tauri-build -- --bundles deb` で `.deb` を再生成するだけであり、
  install smoke や signing 検証は担わない。再生成物を
  `package-linux-x86_64-reproducibility` として upload する。
- `repro_check`（Verify Debian package reproducibility）は両 artifact を
  `primary`／`reproduction` に展開し、`package-reproducibility-check` で
  byte-for-byte の再現性を検証する。
- 意図する理由: 同一入力から二回生成しなければ再現性を主張できないため、
  見かけ上の build 重複は検査の本体であり、通常 CI の重複削減対象外とする。

## ペア 2: `windows` ／ `windows_repro` ／ `windows_repro_check`

- `windows`（NSIS bundle and clean-install smoke）は Web 配布物、managed runtime、
  `pokecon-worker` を stage して offline NSIS installer を生成し、PE metadata 正規化、
  Windows signing inputs の記録、clean install／startup／upgrade／uninstall 検証、
  handoff 時 signing 検証を行い、`*.exe`、`signing-inputs.json`、
  `windows-payload-manifest.json`、`windows-install-tree-manifest.json` を
  `package-windows-x86_64` として upload する。
- `windows_repro`（Independent NSIS reproducibility build）は同一手順で installer を
  再生成し、reproduction build 側の clean install tree
 （`windows-install-tree-manifest.json`）まで検証したうえで、
  `*.exe`、`windows-payload-manifest.json`、`windows-install-tree-manifest.json` を
  `package-windows-x86_64-reproducibility` として upload する。
- `windows_repro_check`（Verify Windows NSIS reproducibility）は両 artifact を
  `primary`／`reproduction` に展開し、次の三層を `sha256sum` 表示＋`cmp` で比較する。
  1. staged resource payload（`windows-payload-manifest.json`）
  2. clean-install tree（`windows-install-tree-manifest.json`）
  3. 外側 NSIS installer（`.exe`）
- 意図する理由: Linux 側と同様、第二 build なしに三層一致を主張できないため、
  見かけ上の build 重複は検査の本体であり、通常 CI の重複削減対象外とする。

## 通常 CI の重複削減との関係

- `tests/quality/test_ci_trigger_dedup.py` が保証する trigger 二重起動の排除
 （feature branch push を `pull_request` の一回だけにする）や、
  重複 probe・重複 workflow の廃止は、CI を一回で済ませるための削減である。
- 本書の二ペアは「同じ成果物を二回作って比較する」ことが検査目的そのものであり、
  上記の削減対象外の意図的例外である。削減作業で second build 側の job
  （`linux_repro`、`windows_repro`）や比較 job（`repro_check`、`windows_repro_check`）を
  削除してはならない。

## same-version probe の受入範囲

- `PLAN.md:517` が受け入れるのは same-version の reinstall／upgrade probe である
  （Debian 側の `SkippedVerified`、Windows 側の `SkippedVerified` と
  `profile_preserved true`、`user_data_preserved true`）。
- 旧バージョンからの upgrade 要件は課さない。本書も回帰 test もその要件を新設しない。

## 現行 HEAD の証跡

- 対象 commit: `82b973e187e0fac0a26abf65541834e4c6ac72d4`。
- Package CI push run `35900764463` と PR run `35900770266` はともに pass。
  監査では OS 別 artifact／digest と clean install／upgrade／uninstall log を確認した。
- 同一 SHA に対する push／pull_request の二重起動は trigger 層の観測であり、
  本書の workflow 内 second build とは別の事象である。
- Release tag（`v*`）の作成・push と tag 起点の Release 公開は利用者担当の外部操作であり、
  本 inventory の対象外とする。Release tag 証跡は本書に含めない。
