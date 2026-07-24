# 旧実装からの移行

Rust版は旧ディレクトリを直接更新しません。旧実装を残したまま新しい保存場所へ段階的に移し、各profileを検証してください。

## 1. バックアップ

旧リポジトリ全体、`SerialController/profiles`、command、template、通知設定を別媒体へコピーします。tokenやwebhook URLを含むbackupは秘密情報として扱います。

## 2. 新しい雛形を作る

新しいアプリケーションを一度起動して終了します。Config、Data、Cache、Stateと`default` profileが作成されます。保存場所は[インストールガイド](INSTALL.md)にあります。

## 3. スクリプトを移す

旧`SerialController/Commands/PythonCommands`と`McuCommands`を、Dataの`Commands`以下へ同じ相対構造でコピーします。sourceを書き換えたり、importを一括置換したりしないでください。managed workerは`Commands.PythonCommandBase`、`Commands.McuCommandBase`、`Commands.Keys`を互換moduleとして提供します。異なるライセンスで作者が単体配布する`bridge_functions`は同梱しないため、必要な場合は作者配布の原本を`<Data>/Commands/PythonCommands/bridge_functions/`へ別途配置してください。

templateやcommand固有assetは、スクリプトが期待する相対位置を保って移します。外部の絶対pathを使っていた場合は、profile設定またはスクリプト固有設定で新しいpathを明示します。

## 4. 設定を移す

旧設定ファイルを新しいTOMLへ丸ごと上書きしないでください。Web UIまたは生成済みの`settings.toml`へ、必要な項目だけ設定します。

- 起動profileは`profiles.active_profile`、CLIでは`--profile`です。
- serverは`server.bind_address`と`server.port`です。
- serial、camera、controller、notificationは各設定画面から再選択します。
- tokenとpasswordはUIのmasked fieldまたは対応する環境変数から設定し、logや移行メモへ残しません。

`init.py`と`init.lua`は動的設定用です。両言語は同じ正準APIを使い、bootstrap-only設定は変更できません。

## 5. 検証する

最初は実機を接続せずcommand一覧を読み込みます。import errorがないこと、class名とtag順が保たれていることを確認します。次にcamera fixture、loopback serial、最後に実機の順で検証します。

固定互換コーパスは次のコマンドで再実行できます。

```bash
nix run .#compatibility
```

昇格済みの追補コーパスも同じコマンドで毎回再実行されます。週次監視で新しいupstream SHAが見つかると、評価結果と昇格または隔離のhash chain記録を含むreview PRが作成されます。実機gateが残る候補は自動昇格しません。

実機が必要なMCU、camera、audio、外部notificationは自動fixtureの成功だけで完了扱いにせず、[外部受入ゲート](ACCEPTANCE.md)を実施してください。設定surface、scope、優先順位は[設定ガイド](SETTINGS.md)で確認できます。

## 主な挙動差

- ハードウェアresourceはRustが所有し、Pythonは直接handleを保持しません。
- script停止、profile切替、transport切断ではcontroller入力をneutralへ戻します。
- notificationとlegacy network helperは閉じたproxy経由で動き、秘密値をerrorへ含めません。
- Tkinterの対応surfaceはToplevel、Scale、Button、Label、messageboxなどの固定集合です。未対応propertyは黙って無視せず明示的errorになります。
- UIとAPIは同じ状態revisionを使います。古いrevisionからの更新は競合として拒否されます。

互換性の不足を見つけた場合は、元source、対象commit、実行結果、必要なhardware条件を記録します。source側の暫定変更で通過させず、互換層または明示的gateとして修正してください。
