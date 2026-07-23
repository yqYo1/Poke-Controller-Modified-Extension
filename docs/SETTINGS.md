# 設定ガイド

PokeConの設定はWeb／Tauri UIから変更する方法を推奨します。UIとHTTP APIは同じ設定transactionを使い、値の検証、hardware resourceへの適用、保存、revision更新を一つの処理として行います。適用に失敗した値は保存せず、変更前のresourceと設定へ戻します。

## 保存場所

global設定はConfig rootの`settings.toml`、profile設定は同じConfig rootの`profiles/<profile>/settings.toml`へ保存されます。OSごとのrootは[インストールガイド](INSTALL.md#初回起動と保存場所)を参照してください。初回起動時に不足する雛形を作りますが、既存のユーザー編集ファイルは上書きしません。

設定には次のscopeがあります。

| scope | 用途 |
|---|---|
| `bootstrap` | app名、profile、動的設定言語、Python環境など、他の設定を読む前に必要な値 |
| `global` | device selector、server、通知など、全profileで共有する値 |
| `profile` | command、camera処理、shortcutなど、active profileと一緒に切り替わる値 |

global専用キーをprofile TOMLへ書いても上書きには使われず、診断対象になります。profileを切り替えると、旧script workerを停止してから対象profileを原子的に公開します。

## 優先順位

起動時は同じ正準設定IDに対し、次の順で適用し、後の値が優先されます。

1. 組み込みdefault
2. global TOML
3. profile TOML
4. 環境変数
5. `init.py`または`init.lua`の動的設定
6. 明示的なCLI引数

CLI引数を省略した場合、CLIのdefaultを最後に再注入して環境変数等を上書きすることはありません。bootstrap設定はworker生成前に先行解決しますが、対応するglobal TOML、環境変数、明示CLIの優先順位は同じです。

listやobject等の複合値をCLI／環境変数へ渡す場合は、CSVではなく厳格なJSON文字列を使用します。空listは`[]`、空objectは`{}`であり、空文字は有効な複合値ではありません。

## 反映タイミング

| mutability | 動作 |
|---|---|
| `runtime_immediate` | hardware／runtimeへ適用できた場合だけ、その場でcommitして保存 |
| `runtime_deferred` | 保存後、次のWebRTC接続、再接続、worker生成、package解決等の指定triggerで反映 |
| `startup_only` | 保存して`pending_restart_values`へ表示し、次回起動で反映 |

UIの「現在」と「保存後」の値が異なる場合、再起動待ちまたはdeferred trigger待ちです。server port、bind address、Web root等のstartup-only値を現在processへ暗黙適用しません。

## Device selector

camera、serial、controller、audioにはUIで列挙されたnative selectorの生値を保存します。deviceが見つからない場合も別deviceを暗黙選択せず、同じselectorで再試行します。設定変更中に新deviceを開けなかった場合は、同じ旧selectorと旧設定へのrollbackを試みます。

Linuxのserialでは`/dev/serial/by-id`、`/dev/serial/by-path`、device nodeの順に安定selectorを優先します。Windowsではnative COM／camera IDをそのまま扱います。OS移行後はselectorをUIから再選択してください。

## LAN公開

`server.bind_address`のdefaultは`127.0.0.1`です。非loopback IPを保存すると、認証なしのREST／WebSocketと、サーバーhost上でのPython／Lua保存・評価を含む全操作をLAN clientへ公開します。これはHost／Origin検査だけで保護された完全信頼境界であり、利用者認証ではありません。信頼できる隔離LAN以外では設定しないでください。

## Secret

secret設定はUIでmaskされ、設定snapshot、WebSocket、log、診断、debug表示、signed manifestへ平文を返しません。現在のmasked値をそのまま保存するとrevisionを変更せず、secretを置換しません。token、webhook URL、passwordを移行メモ、受入記録、issue、logへ貼り付けないでください。

## 動的設定

`init.py`と`init.lua`は同じ`pokecon.opt.*`正準APIを使います。動的設定は起動中のmemory overlayであり、代入時に型、scope、mutabilityを検証します。bootstrap専用設定はworker自身の生成条件になるため動的変更できません。任意のdynamic codeをLANへ公開する場合は、[Security acceptance](ACCEPTANCE.md#security-acceptance)を必ず実施してください。

## 正準リファレンス

設定ID、default、型、範囲、scope、mutability、TOML／CLI／環境変数／動的／UI surfaceは、次の生成元・生成物で確認できます。

- `rust/pokecon-contracts/registry/settings.json`: 正準レジストリ
- `generated/settings.schema.json`: 78設定を必須キーとして持つ閉じたJSON Schema
- `generated/settings-ui.json`: UI control、access、secret、scope、mutabilityのmetadata
- `python/pokecon/typings/`と`generated/lua/pokecon.d.lua`: Python／Lua型情報

生成物は手編集せず、レジストリ変更後に`nix run .#generate-contracts`を実行します。CIでは`nix run .#contract-check`が全生成物のdriftと78設定の完全性を検査します。
