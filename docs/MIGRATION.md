# 旧実装からRust版へ移行する

この文書は、旧PokeConのcommand、profile、設定、assetをRust版へ移す利用者と管理者向けです。

新規導入だけを行う場合は[インストールガイド](INSTALL.md)を参照してください。

個々の設定fieldの意味は[設定リファレンス](SETTINGS.md)を正本とし、この文書では移行判断に必要な差だけを扱います。

## 上書きupgradeではなく並行移行する

Rust版は旧repositoryや旧保存directoryを直接更新しません。

旧環境を動作する状態で残し、新しいConfig、Data、Cache、State rootへ段階的に移します。

旧設定fileを新しい`settings.toml`へ丸ごとcopyしません。

旧Python sourceへ一括置換をかけません。

最初にcommand discovery、次に仮想I/O、最後に実機という順で範囲を広げます。

各段階で戻せるように、旧環境と新環境を別directory、別shortcut、別portで識別します。

## 移行対象を棚卸しする

移行前に次の対象を一覧へ記録します。

- 使用中の旧PokeCon versionまたはcommit。
- `SerialController/Commands/PythonCommands`以下のPython command。
- `SerialController/Commands/McuCommands`以下のMCU command。
- commandが読むtemplate、画像、音声、JSON、CSVなどのasset。
- profileごとのcommand、設定、shortcut、launcher。
- camera selector、resolution、flip、screenshot形式。
- serial port、baud rate、data format。
- Windows通知、Discord、MQTT、socketなどの外部接続。
- Python third-party packageとversion。
- `init.py`や同等の起動時customization。
- 外部作者から別途取得したmodule。

絶対path、device名、COM番号は新環境で変わる可能性があるため、値だけでなく用途も記録します。

token、password、webhook URLは一覧へ平文で書かず、別のsecret管理手段で移します。

## 旧環境を復元可能な形で保存する

旧repository、保存directory、profile、command、asset、package一覧をapplication停止後にbackupします。

backupにはsourceのhashまたはarchiveのSHA-256を付けます。

外部moduleは配布元、version、license、取得hashを記録します。

secretを含むbackupは暗号化し、通常のissue添付や受入artifactと分けます。

backupから旧環境を起動できることを確認してから新環境を変更します。

## Rust版の雛形を生成する

Rust版を一度起動し、正常に終了します。

初回起動でConfig、Data、Cache、Stateと`default` profileの雛形が作成されます。

保存場所は[インストールガイド](INSTALL.md#初回起動で作る保存場所)を参照してください。

生成されたrootを旧directoryへ向けず、新しい空の場所として維持します。

`settings.toml`や`Commands` directoryが生成されない場合は、移行を続けず[トラブルシュート](TROUBLESHOOTING.md#初回起動と保存先を診断する)を参照します。

## commandとassetを相対構造のまま移す

旧`PythonCommands`と`McuCommands`を、Data rootの`Commands`以下へ同じ相対構造でcopyします。

概略は次の形です。

```text
<Data>/
└── Commands/
    ├── PythonCommands/
    │   └── ...
    └── McuCommands/
        └── ...
```

templateやcommand固有assetも、sourceが期待する相対位置を保ちます。

旧sourceのimport、class名、`NAME`、`TAGS`を移行前に変更しません。

workerは`Commands.PythonCommandBase`、`Commands.McuCommandBase`、`Commands.Keys`を互換moduleとして提供します。

discovery結果を確認してから、必要なsource改善を別変更として行います。

異なるlicenseで作者が単体配布する`bridge_functions`はRust版に同梱されません。

必要な場合は作者の配布原本を確認し、利用者自身が`<Data>/Commands/PythonCommands/bridge_functions/`へ配置します。

repository、installer、移行archiveへ`bridge_functions`を再同梱しません。

公開互換surfaceとdiscovery規則は[ユーザースクリプト開発ガイド](SCRIPT_DEVELOPMENT.md)を参照してください。

## Python packageを専用環境へ固定する

旧system Pythonや共有venvを新workerへそのまま接続しません。

commandが使用するpackage名とversionを記録し、PokeCon専用venvの要求へ変換します。

managed workerの既定依存はbundled wheelhouseだけからoffline同期されます。

追加packageはuvが専用venvへexact syncするため、宣言にないpackageは残る前提にしません。

native extensionを含むpackageはPython 3.14と対象OSに対応するwheelまたはbuild条件を確認します。

package取得にcredentialが必要な場合は`POKECON_UV_`接頭辞の限定bridgeを使用し、TOML、source、logへtokenを書きません。

venvとpackage設定は[上級利用ガイド](ADVANCED_USAGE.md#user-script専用venvを管理する)を参照してください。

## 設定を意味ごとに再入力する

新しいWeb UIまたは生成済みTOMLへ、確認できた項目だけを入力します。

旧設定fileのsection名や未認識fieldをそのまま残しません。

主な移行先は次のとおりです。

| 旧環境での用途 | Rust版で確認する場所 |
|---|---|
| 起動profile | `profiles.active_profile`または`--profile` |
| server address | `server.bind_address`と`server.port` |
| camera | Camera tabと`camera.*` |
| serial | Serial tabと`serial.*` |
| controller | Manual Control tabと`controller.*` |
| command表示 | Commands tab、動的設定の`pokecon.commands` |
| Windows通知 | Notifications tabと`notifications.windows.*` |
| Discord | Notifications tabと`notifications.discord.*` |
| desktop close | `ui.desktop.close_behavior` |
| Python worker | `python.*` |

`server.bind_address`、`server.port`、root、worker環境などのstartup-onlyまたはbootstrap-only設定は再起動後に反映されます。

UIへ保存できたこととrunning serviceへ適用されたことを混同せず、`restart_required`と`apply_failures`を確認します。

secret fieldはmasked UIまたは対応する環境変数から設定します。

全fieldのscope、default、mutabilityは[設定リファレンス](SETTINGS.md)を参照してください。

## profileを一つずつ再構成する

最初は`default` profileだけへ少数のcommandを移します。

command一覧、tag、shortcut、camera、serialを確認した後に追加profileを作ります。

profileをcopyする場合もsecret、device selector、絶対pathが新profileに適切かを再確認します。

profile切替中はrunning commandを停止し、controllerがneutralになったことを確認します。

新profileのworkerが起動しない場合は旧profileへ戻し、両方のworker stateを同時に手修正しません。

Windows launcherを生成する場合はprofile完成後にUIまたは公開APIから作成します。

## 動的設定を別段階で移す

`init.py`と`init.lua`は起動時の動的設定とevent callback用です。

user commandの本体を動的設定へ移しません。

旧customizationが設定default、command sort、event callback、controller操作のどれに相当するかを分けます。

一つの小さい変更から読み込み、transaction成功時だけ次を追加します。

bootstrap-only fieldを動的設定から変更しようとせず、TOML、環境変数、CLIへ配置します。

相対`source()`はConfig root内に留め、旧環境のsymlinkや`..` traversalを持ち込みません。

APIとevent一覧は[動的設定ガイド](DYNAMIC_CONFIGURATION.md)を参照してください。

## hardwareなしで互換性を確認する

実機を接続する前にapplicationを起動し、command一覧をreloadします。

次を確認します。

- import errorがない。
- 期待する`module_path`と`class_name`が列挙される。
- `NAME`とtagが維持される。
- private helper fileやsymlinkがcommandとして誤検出されない。
- command開始、停止、一時停止、再開がworker errorなく完了する。
- script dialog、Tk surface、overlayを使うcommandが閉じた公開surfaceで動く。

本体checkoutでは固定互換corpusも実行します。

```bash
nix run .#compatibility
```

serialとcameraのnative経路はLinuxの仮想I/Oで確認できます。

```bash
nix run .#virtual-io-check
```

互換testを通すために移行sourceだけを一時変更せず、互換層の不足かsource固有依存かを切り分けます。

## 実機を段階的に接続する

最初にcameraだけを接続し、selector、resolution、色順、flip、screenshotを確認します。

次にserial loopbackまたは専用fixtureを接続し、initial neutral、操作frame、disconnect neutralを確認します。

最後にMCUと対象consoleを接続し、button、hat、stick、touchを低riskな操作から確認します。

通知はtest endpointで一channelずつ確認し、送信先の秘匿化済みrequest IDだけを記録します。

virtual I/Oの成功をUSB、firmware、console、物理cameraの合格へ読み替えません。

releaseまたは組織内展開では[外部受入ゲート](ACCEPTANCE.md)の対象capabilityを実施します。

## 主な挙動差を受け入れる

Rust版ではhardware resourceをRustが所有し、Pythonは直接handleを保持しません。

script停止、profile切替、transport切断、application終了ではcontroller入力をneutralへ戻します。

UIとAPIは同じ状態revisionを使用し、古いrevisionからの設定更新を競合として拒否できます。

notificationとlegacy network helperはbounded timeoutを持つRust proxyを経由します。

Tkinter互換はToplevel、Scale、Button、Label、messageboxなどの閉じたsurfaceで、未対応propertyを黙って無視しません。

command discoveryではsourceが実行され、execution generationでも再実行されます。

directory走査順は公開契約ではないため、表示順をfilesystem順へ依存させません。

これらを旧実装へ合わせて無効化せず、安全性と公開互換surfaceの範囲内でsourceを調整します。

## 問題時にrollbackする

新環境のcommandを停止し、serialを明示disconnectしてapplicationを終了します。

旧環境が使用するportとcameraを新processが保持していないことを確認します。

旧backupを上書きせず、保存した旧launcherまたは環境から再開します。

新環境で作成したDataとConfigは失敗調査用に別名で保存します。

rollback中に両applicationを同じMCUへ同時接続しません。

## 互換性不足を報告する

報告には旧versionまたはcommit、新versionまたはcommit、対象OS、Python sourceのhash、操作、期待結果、実結果を含めます。

hardwareが必要な場合はdevice種別、baud rate、data format、firmware version、console側結果を秘匿化して記録します。

token、webhook URL、broker credential、生のserial number、任意code全文を公開issueへ含めません。

最小再現sourceを作る場合も、外部作者のcodeやlicenseの異なるmoduleを無断で転載しません。

原因別の収集手順は[トラブルシュート](TROUBLESHOOTING.md#不具合報告に必要な情報を集める)を参照してください。
