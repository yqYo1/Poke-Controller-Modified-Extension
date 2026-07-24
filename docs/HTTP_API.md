# HTTP APIとリアルタイム通信

この文書は、PokeConを外部ツール、独自UI、監視プログラムから操作する統合開発者向けです。

一般的な画面操作は[利用ガイド](USER_GUIDE.md)を参照してください。

Python commandから本体機能を利用する場合は、HTTPを経由せず[ユーザースクリプト開発ガイド](SCRIPT_DEVELOPMENT.md)の公開APIを使用してください。

動的設定から状態やcommand表示を変更する場合は[動的設定ガイド](DYNAMIC_CONFIGURATION.md)を参照してください。

## 契約の正本を選ぶ

**OpenAPI文書**はREST endpoint、request、response、errorの機械可読な正本です。

repositoryには生成済みの[`api/openapi.json`](../api/openapi.json)を収録しています。

TypeScript型はOpenAPIから生成されるため、手書きの型を公開契約として扱いません。

HTTPとWebSocketのRust wire型は`rust/pokecon-server/src/api.rs`にあります。

設定のfield集合は`rust/pokecon-contracts/registry/settings.json`からOpenAPIへ組み込まれます。

この文書は接続方法と状態同期の規則を説明しますが、すべてのfieldを複製しません。

fieldを生成や検証に使用する場合は、必ずOpenAPIを入力にします。

## 接続先を決める

既定のHTTP originは`http://127.0.0.1:8020`です。

実際のaddressとportは`server.bind_address`と`server.port`で決まります。

loopback以外へbindすると、同じnetworkのclientからREST、WebSocket、動的PythonまたはLuaの読み込みを含む全操作が可能になります。

PokeConの公開serverにはlogin、API token、peerごとの権限分離がありません。

LAN公開は閉じた信頼networkでのみ使用し、Internetへ直接公開しません。

詳しい判断基準は[上級利用ガイド](ADVANCED_USAGE.md#lan公開の信頼境界を確認する)を参照してください。

## 共通request境界を満たす

すべてのrequestは、実際にbindしたauthorityと一致する`Host` headerを必要とします。

loopbackでbindした場合は、同じportの`localhost`も許可されます。

`Origin`を付ける場合は、bind addressと一致するHTTP originでなければなりません。

desktop modeでは`tauri://localhost`も許可されます。

`POST`、`PATCH`、`PUT`、`DELETE`には次のheaderが必要です。

```http
Content-Type: application/json
X-Pokecon-Request: 1
```

`Content-Type`のmedia typeは`application/json`である必要がありますが、`charset` parameterは使用できます。

mutating requestでheaderが不足すると、処理本体へ到達する前に拒否されます。

WebSocket upgradeには`Origin`が必須です。

browser以外のclientも`Origin`を省略できません。

CORS preflightは許可されたorigin、method、`Content-Type`、`X-Pokecon-Request`だけを受け入れます。

## JSON envelopeを処理する

通常の成功responseは`data`を一つ持ちます。

```json
{
  "data": {
    "changed": true,
    "revision": "42"
  }
}
```

error responseは`error`を一つ持ちます。

```json
{
  "error": {
    "code": "revision_conflict",
    "message": "the settings revision changed",
    "fields": null
  }
}
```

`error.code`は機械判定用の閉じた集合です。

`error.message`は人が診断する補助情報であり、分岐条件に使用しません。

`error.fields`はfield別の検証errorがない場合も`null`として存在します。

未知fieldは多くのrequest型で拒否されるため、OpenAPIにない値を先行送信しません。

画像downloadとWindows launcher downloadはJSON envelopeではなくbinary responseを返します。

clientはstatusだけでなく`Content-Type`と`Content-Disposition`も確認します。

## revisionを競合検出に使う

状態と設定のrevisionはJavaScriptの整数精度に依存しない10進文字列です。

先頭zeroのない非負整数として比較し、`Number`へ変換しません。

設定更新では、読み取ったrevisionを`expected_revision`へ渡せます。

```bash
curl --request PATCH --header 'Content-Type: application/json' --header 'X-Pokecon-Request: 1' --data '{"expected_revision":"41","values":{"ui.fps":30}}' http://127.0.0.1:8020/api/settings
```

その間に別の更新がcommitされていれば`409 revision_conflict`になります。

競合時は同じpayloadを盲目的に再送せず、現在値を再取得し、利用者の変更意図とmergeします。

`expected_revision`を省略すると競合検出を放棄するため、対話的な設定editorでは原則として指定します。

設定transaction、restart待ちの値、runtime apply failureは[設定リファレンス](SETTINGS.md#revision付き更新で競合を検出する)を参照してください。

## REST endpointを選ぶ

| method | path | 用途 |
|---|---|---|
| `GET` | `/api/state` | runtime状態の完全snapshotを取得する |
| `GET` | `/api/settings` | 設定値、revision、再起動待ち、適用失敗を取得する |
| `PATCH` | `/api/settings` | 複数設定を一つのtransactionとして更新する |
| `POST` | `/api/commands/control` | commandをstart、stop、pause、resumeする |
| `POST` | `/api/commands/reload` | active profileのcommand集合を再読込する |
| `GET` | `/api/devices/cameras` | cameraをnative selector付きで再列挙する |
| `GET` | `/api/devices/serial-ports` | serial portをnative selector付きで再列挙する |
| `POST` | `/api/serial/control` | serialをconnectまたはdisconnectする |
| `POST` | `/api/camera/retry` | 設定済みcameraを同じselectorで再試行する |
| `POST` | `/api/camera/screenshot` | capture保存、native path保存、downloadを実行する |
| `POST` | `/api/notifications/test` | WindowsまたはDiscord通知を試験する |
| `POST` | `/api/script-ui/action` | script dialog、Tk surface、overlayへ応答する |
| `POST` | `/api/dynamic-config/control` | 動的設定をpath、content、reloadで切り替える |
| `POST` | `/api/profiles/generate-launcher` | profileとlauncherを作成またはdownloadする |
| `POST` | `/api/update/check` | 現在versionと公開versionを比較する |
| `GET` | `/ws` | 状態変更、log、serial、script UI、映像、入力を接続する |

endpointごとのstatus code、request union、必須fieldはOpenAPIを参照してください。

未知の`/api/*` pathはSPA fallbackで隠さずJSONの`resource_not_found`として応答します。

### commandを安定したidentityで指定する

command開始は表示名ではなく`module_path`と`class_name`の組で指定します。

```bash
curl --request POST --header 'Content-Type: application/json' --header 'X-Pokecon-Request: 1' --data '{"action":"start","command":{"module_path":"Commands.PythonCommands.Sample","class_name":"Sample"}}' http://127.0.0.1:8020/api/commands/control
```

停止、一時停止、再開のpayloadはそれぞれ`{"action":"stop"}`、`{"action":"pause"}`、`{"action":"resume"}`です。

成功時の`changed`が`false`なら、要求後の状態がすでに成立していたことを示します。

### device列挙結果を保存する

cameraとserialの`selector`はOS native identityです。

表示用`label`を接続identityとして保存しません。

`available: false`の項目は保存済みselectorをUIに残すために返る場合があり、現在接続できることを意味しません。

接続失敗時に先頭deviceへ自動で切り替えず、利用者へ再選択を求めます。

### screenshotの出力先を区別する

`destination: "captures"`はPokeCon管理下のCaptures directoryへ保存します。

`destination: "path"`はdesktop modeで明示したnative pathへ保存します。

`destination: "download"`はPNGまたはJPEG bytesをresponse bodyとして返します。

`region`はframe全体に対する0以上1以下の正規化座標です。

範囲外、zero面積、frame外へはみ出すregionは受け入れられません。

既存fileを置換する意図がない場合は`overwrite: false`を使用します。

### 動的codeのendpointを隔離する

`/api/dynamic-config/control`の`load_content`はrequest本文のPythonまたはLuaを実行します。

`load_path`はserver filesystem上のpathを読み込みます。

これらは単なる設定値変更ではなく任意code実行です。

外部clientへこのendpointを公開する判断は、server processと同じ権限を渡す判断として扱います。

path jailとtransactionの詳細は[動的設定ガイド](DYNAMIC_CONFIGURATION.md#source-pathの境界を守る)を参照してください。

## WebSocketで状態を追従する

RESTの`GET /api/state`は完全snapshotを返し、WebSocketの`ui.state.changed`は一つのvisible transactionに対応する差分を返します。

clientは最初にWebSocketを接続して`ui.state.changed`をbufferし、その後にREST snapshotを取得します。

snapshot取得後は、snapshotより新しいbuffer済み差分をrevision順に適用します。

WebSocket接続前の変更やbuffer overflowで差分を取り逃した可能性がある場合は、再度snapshotを取得します。

`ui.state.changed`のrevisionが期待する次の状態と整合しない場合も、差分を推測せずsnapshotへ戻ります。

差分の`state`でfieldが欠けている場合は変更なしを意味します。

`null`を値として持てるfieldでは、field欠落と`null`を区別します。

serverから送るJSON messageは次の10種類です。

| `type` | 用途 |
|---|---|
| `ui.state.changed` | revision付きruntime状態と設定適用結果 |
| `serial.data` | 受信serial chunk |
| `log` | structured log |
| `script.ui` | script dialog、Tk、overlayの最新snapshot |
| `webrtc.offer` | WebRTC SDP offer |
| `webrtc.answer` | WebRTC SDP answer |
| `webrtc.ice_candidate` | WebRTC ICE candidate |
| `input.generation` | この接続が使用する入力世代 |
| `input.snapshot.applied` | 入力snapshotを適用したsequence |
| `ping` | heartbeat nonce |

clientから送れるJSON messageは次の9種類です。

| `type` | 用途 |
|---|---|
| `webrtc.offer` | WebRTC SDP offer |
| `webrtc.answer` | WebRTC SDP answer |
| `webrtc.ice_candidate` | WebRTC ICE candidate |
| `input.snapshot` | 接続世代の初期neutralを含む完全入力 |
| `keyboard_input` | keyboard差分 |
| `mouse_stick_input` | mouseによるstick差分 |
| `mouse_input` | mouse buttonまたは座標差分 |
| `gamepad_input` | button、stick、hat、touch差分 |
| `pong` | serverのnonceをそのまま返すheartbeat応答 |

JSON unionは`type`で判別し、payloadは原則として`data`内にあります。

WebRTCが使用できない場合も、JSON WebSocketは状態、signal、入力、logの基盤として残ります。

binary WebSocket frameはMJPEG fallback用であり、JSONとしてdecodeしません。

### 入力generationを確立する

接続ごとにserverは非空ASCIIの`generation`を発行し、`input.generation`として送ります。

clientはそのgenerationを使い、sequence `"0"`の`input.snapshot`を最初に送ります。

初期snapshotにはkeyboard、mouse、controller button、hat、左右stick、touchの完全状態が必要です。

安全なclientはbuttonをすべてrelease、hatをneutral、stickを`128,128`、touchを`null`にします。

serverが`input.snapshot.applied`で同じgenerationとsequenceを返した後に、`"1"`以降の差分を送ります。

sequenceは同一generation内で単調に増やす10進文字列です。

再接続時は以前のgenerationとsequenceを再利用せず、新しく受け取ったgenerationでneutral snapshotから始めます。

古い接続のeventを新しいgenerationへ混ぜません。

touchを押している場合は`x`が0から319、`y`が0から239で、`pressed`は`true`だけが有効です。

touch releaseは`touch: null`で表します。

### heartbeatへ応答する

`ping`を受信したclientは同じ`nonce`を`pong`で返します。

nonceを作り直したり、古いpingへまとめて応答したりしません。

heartbeat timeout後はserverが接続を終了するため、clientはneutralな新generationとして再接続します。

## 生成clientを更新する

公開契約を変更した本体開発者は、OpenAPIとTypeScript clientを同じ変更で再生成します。

```bash
nix run .#generate-api-types
nix run .#contract-check
```

生成物を手で修正しません。

外部統合側は固定したOpenAPI artifactをcode generationへ入力し、未認識のvariantを安全に拒否できるようにします。

version upgradeではrequestとresponseのschema差分に加え、設定registryとWebSocket unionも比較します。

## 障害を診断する

`403 request_forbidden`はHost、Origin、固定header、WebSocket Originの不一致を先に確認します。

`415 unsupported_media_type`はmutating requestの`Content-Type`を確認します。

`400 malformed_json`はJSON syntaxを、`422 invalid_request`はschemaまたは意味上の制約を確認します。

`409`はrevision、command state、profile switch、serial接続、camera状態などの競合を表すため、`error.code`で分岐します。

`500`を受けた場合もsecretや動的code本文を報告へ添付せず、操作、status、error code、診断ID、対象versionを記録します。

一般的な原因別手順は[トラブルシュート](TROUBLESHOOTING.md)を参照してください。
