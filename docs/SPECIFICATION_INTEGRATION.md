# PokeCon EXバックエンド・フロントエンド連携定義書

> 対象読者: PokeCon backend／frontend間の契約、外部client API、lifecycleを実装する開発者と実装支援agent。
>
> 状態: 製品目標の規範仕様です。Webのwire schemaは既存clientとの互換性契約です。GPUIのintegration mechanismはlocal HTTP clientとtyped in-process adapterをPoCで比較し、同一backend contractを保てる方式を選びます。

## 0. 担当範囲と要件分類

**必須要件**には、公開HTTP／WebSocket／WebRTC契約、browser UIがbackend状態を同期する規則、resource ownerとUIの境界、安全な停止・入力解放を含みます。既存clientと周辺機器を壊す変更は互換性判断なしに行いません。

**機能要件**には、操作が画面に反映される結果とlatency等の品質目標を含みます。操作結果は規範ですが、個別実装technologyに拘束されません。

| 分類 | 対象 |
|---|---|
| 必須要件 | §1.3の単一backend ownerと起動時mode選択、§3.4のWebSocket状態・再接続契約、§6.1.5の保存先安全性、§6.2.2のserial値検証・atomic reconnect、§7のwire contract、§8のschema正本、§15の停止・入力解放 |
| 機能要件 | §1.3の選択起動入口、§3.4の利用者向け回復、§6.1.5の保存UIと確認、§6.2.2のserial設定操作と結果表示、§15のnative window・tray操作、[フロントエンド定義書](SPECIFICATION_FRONTEND.md) §3.1の利用者向け応答性 |

Web/Tauri modeは既存HTTP／WebSocket／WebRTC contractを使用します。GPUI native modeのbackend接続は既存local HTTP API clientとtyped in-process adapterを比較して選択し、新たな公開endpointや第二の状態所有者を追加しません。どちらの方式でもoperation names、validation、result、error、revision、lifecycleは同じbackend contractに従います。adapterを選ぶ場合も、UIからdevice handle、canonical state、worker lifecycle、backend internal serviceへ直接依存しません。local HTTP clientを選ぶ場合は既存のOrigin・保存先制限を維持し、GPUIのためにorigin allow-listやremote path権限を広げません。既存境界で満たせない操作があればPoC比較でadapterを選択します。

WebのREST／WebSocket schemaの機械可読正本は`../api/openapi.json`と`../rust/pokecon/registry/protocol.json`です。生成物の変更だけで正本を変更したことにはなりません。

## 1.3 [必須要件] 対象プラットフォーム・プロセスモデル
PokeConは一つのRust backendを所有し、起動時に選択するfrontend modeから利用します。

現行のSvelte Web UIとTauri desktop UIを維持し、native GPUI frontendを追加します。

frontend選択はNixから独立して行えます。実行中の既存sessionを別modeへ暗黙に移行しません。

| frontend mode | 利用者向け形態 | backend接続 |
|---|---|---|
| Web | browser上の現行SvelteKit SPA | HTTP REST、WebSocket、WebRTC |
| Tauri desktop | 現行Web UIをnative windowで表示 | 同一Rust process内のAxumとTauri |
| GPUI desktop | native GPUI UI | local HTTP API clientまたはtyped in-process adapter。選択はPoCで比較して確定 |
| Mobile | 将来の候補 | 対応可否は別途判断 |

macOSは現時点では対象外です。macOS向けbuild成果物、実機検証、互換保証は現在のscopeに含めず、将来のversionで対応可否を判断します。

現行起動入口を保ち、次のfrontend selectorを目標とします。

```text
nix run . -- --ui web       # 現行Web UI
nix run .#tauri             # 現行Tauri desktop UI
nix run .#gpui              # 追加するGPUI native UI
```

`nix run .#gpui`と`--ui gpui`は未実装の目標です。各modeは同一backend ownerを一度だけ起動し、camera、serial、worker、settings、state、shutdownを重複所有しません。

```text
Web browser / Tauri WebView / GPUI native window
                    |
            mode-appropriate UI boundary
                    v
       one Rust application/backend process
          |                         |
 user-script worker       dynamic-config worker
```

ユーザースクリプトworkerは自動実行時の機能上の主です。Rust backendはworkerの監督に加えてhardware resources、canonical state、安全停止を所有します。TauriやGPUIのwindow/event loopはbackendの第二所有者になりません。


## 3.4 [必須要件と機能要件] WebSocket自動再接続

以下のWebSocket再接続wire・timeout規則はWeb browserおよび現行Tauri WebView clientに適用します。GPUI nativeは同じ利用者向け切断検出・回復操作を実現しますが、typed in-process adapterを選ぶ場合にWebSocket transportを要求しません。
- 接続断時に、自動的に再接続を試行します。
- 再接続間隔、リトライ回数上限はbackend定義書の[正準設定表](SPECIFICATION_BACKEND.md) §11.4.2で定義する。
- デフォルト値: 3秒ごとに試行、リトライ回数上限20回。上限到達後は接続状態・最後の秘匿化済みエラー・試行回数と「再接続」ボタンを持つ非モーダルUIを表示する。ボタン押下は即座に1回の接続試行を開始し、成功時は表示を閉じて通常状態へ戻る。失敗時は表示を維持し、次の明示押下まで自動試行を再開しない。`reconnect_max_retries=0`の場合も切断時に同じUIを直ちに表示する
- **無応答検出**: アプリケーションレベルの`ping`／`pong`（§7.3.2）を使用する。サーバーは接続ごとに`websocket.ping_interval_sec`間隔で一意の`nonce`を持つ`ping`を送信し、クライアントは受信後直ちに同じ`nonce`の`pong`を返す。サーバーが送信後`websocket.pong_timeout_sec`以内に対応する`pong`を受信しない場合は当該接続を切断状態へ遷移させる。クライアントも最後の`ping`受信から両設定値の合計秒を超えた場合は接続を能動的に閉じ、上記の再接続を開始する。通常メッセージの送受信はheartbeatを代替せず、タイマーは単調時計で計測する。両設定は1以上の整数、`pong_timeout_sec <= ping_interval_sec`を要求し、変更時は現在のheartbeat待機を取り消して設定適用時から新しい値で再スケジュールする

---

## 6.1.5 [必須要件と機能要件] スクリーンショットキャプチャ
本節はfrontendのdialog／download選択とbackendのcapture／formatをつなぐend-to-end契約です。backendがcaptureと画像bytesの生成を所有します。Web/Tauri server-path保存ではbackendが保存先閉じ込め・非上書きを適用します。GPUI nativeではOS file chooserを提供し、byte-return経路ではfrontend側がcancel／non-overwriteを守って選択pathへ保存します。backendはGPUIのためにTauri専用server-path HTTP variantを拡張しません。GPUIのHTTP clientは安全なbyte-return経路、in-process adapterは同じbackend保存serviceを使う方式をPoCで選びます。

- **保存場所**: 実効Dataルート（[バックエンド定義書](SPECIFICATION_BACKEND.md) §14.1.1 Data）配下の `Captures/` ディレクトリ（自動作成）。相対ファイル名はここへ解決。絶対ファイル名はそのまま絶対パスとして使用。相対パスによる `Data/Captures` 外へのトラバーサル（`../` 等）は拒否する。UI保存ダイアログは明示的なターゲットパスを指定する。
- **形式**: PNG（デフォルト）／JPEG。`camera.screenshot_format`（[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.4.2参照）で永続的なデフォルトを設定する。変更は以降の保存に即時反映され、既存ファイルに影響しない。
- **拡張子**: 生成されるファイル名の拡張子は実効形式に従う（PNG → `.png`、JPEG → `.jpg`）。
- **既定ファイル名**: `filename`が`None`または空文字の場合、固定ベースライン互換のローカル時刻`YYYY-MM-DD_HH-MM-SS`をベース名とし、実効形式の拡張子を付ける。同一秒の同名ファイルが既に存在する場合は上書きせず、`_1`、`_2`、...の最小未使用接尾辞を付け、排他的createで同時保存間の競合を防ぐ。タイムゾーンは実行ホストのローカルタイムゾーンとし、時刻取得失敗時は保存を失敗させて固定名へフォールバックしない。
- **上書き**: UI／RESTは既存ファイルを既定で上書きしない。明示名の保存先が存在する場合は`409 Conflict`とし、UIが対象パスを表示してユーザー確認を得た後の要求だけが`overwrite=true`を送信できる。`overwrite=true`でもシンボリックリンク・非通常ファイル・閉じ込め外パスは拒否する。固定ベースラインのユーザースクリプト`saveCapture()`はローカルコード実行権限を持つ互換APIとして、明示された既存通常ファイルを従来どおり置換できるが、`filename=None`／空文字の自動名は上記の非上書き接尾辞規則を使用する。
- **UI保存ダイアログでの一回限り上書き**: 名前を付けて保存ダイアログでは実効 `camera.screenshot_format` を初期選択として表示するが、ユーザーはその保存に限り PNG／JPEG を選択し直せる。この上書きは一時的であり、`camera.screenshot_format`設定やTOMLを変更しない。選択されたファイル種別がエンコード方式を決定する。ファイル名に拡張子がない場合は選択された拡張子を追加する。ファイル名に既に認識済み画像拡張子（`.png`／`.jpg`／`.jpeg`、大文字小文字不問）が含まれる場合は、それを選択された拡張子で置き換える。既存ファイルの上書きは通常の明示的確認に従う。
- **JPEG品質**: JPEG保存時は既存 `jpeg_quality`（1-100、デフォルト85）を使用する。PNG保存時は `jpeg_quality` を無視する。`jpeg_quality` の設定は Motion JPEGフォールバックの品質も兼ねる。

## 6.2.2 [必須要件と機能要件] 設定
本節はfrontendのserial settings操作とbackendのatomic reconnect／rollbackを結ぶ契約です。3DS選択時のUI convenience以外で値を暗黙変更せず、全surfaceで同一のbackend validationとserial wire formatを使用します。wire formatの正本は[周辺機器開発ガイド](PERIPHERAL_DEVELOPMENT.md)です。

| 設定 | オプション | デフォルト |
|---------|---------|---------|
| **ボーレート** | 4800 / 9600 / 115200、およびOS／ドライバーが受理する任意の正整数 | 9600 |
| **データ形式** | デフォルト / Qingpi / 3DS Controller | デフォルト |

**UI連動**: UIでデータ形式を3DS Controllerへ変更した場合は、現在確認済みの対応機器に合わせて、同一の設定書き込みトランザクションで`serial.baud_rate`も`115200`へ変更する。この変更はUI操作の補助動作であり、3DS Controller形式の検証制約ではない。TOML、CLI、環境変数、動的設定、OpenAPIからは`serial.data_format = "3ds"`と任意の正整数`serial.baud_rate`の組み合わせを受理し、値を暗黙に変更しない。UIで3DS Controllerから別形式へ戻した場合も、現在のボーレートを保持し、過去の値を暗黙に復元しない。

**シリアル設定の即時適用トランザクション**:

1. `serial.port`、`serial.baud_rate`、`serial.data_format`はグローバル専用の`runtime_immediate`設定として同じ直列化ロックで更新する。
2. シリアル未接続時は、検証後に実効値を即時更新する。UI／OpenAPI書き込みではその後にグローバル`settings.toml`へ原子的に保存する。`serial.port`の生セレクター値は解決・正規化せず、指定された生文字列をそのまま保持する。
3. 接続中は、Rustメインが全ボタン・スティック・タッチ状態を強制解放してから旧接続を閉じ、ロールバック用に旧`serial.port`の生セレクター値を保持した上で、新しい3設定の組み合わせで指定された新デバイスへ接続する。生セレクター値（`/dev/ttyACM0`、`/dev/serial/by-id/...`、`COM3`等）はそのまま渡し、シンボリックリンクはオープン時のみ追跡する。
4. 新設定で接続できた場合だけ、UI／OpenAPI書き込み先TOML、正準設定値、UI表示、および送信フォーマッターを新設定へ確定する。新しい`serial.port`生セレクター値をそのまま保存する。
5. 新設定で接続できない場合は旧3設定で再接続し、変更をTOML・正準設定値・UIへ反映せずエラーを返す。旧設定でも再接続できない場合は未接続状態とし、正準設定値と保存値は旧値のまま維持してERROR診断を出す。他機能は継続する。他のポートへの暗黙フォールバックは行わない。
6. UIで3DS Controllerを選択した場合だけ、`serial.data_format = "3ds"`と`serial.baud_rate = 115200`を同一トランザクションへ含める。その他の設定表面では入力された3設定だけを使用し、ボーレートを暗黙変更しない。
7. 動的設定代入も同じ再接続・ロールバックを使用するが、[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.4.1.5どおりTOMLへは書き戻さない。
8. `serial.port`はハードウェアセレクターであり、汎用ファイルシステムパスではない。環境変数展開・チルダ展開・相対パス解決・字句的正規化は適用しない。セレクターのシンボリックリンクはオープン時のみ追跡し、保存・保持する生セレクター値は変更しない。

**シリアルwire protocol**: frame layout／length、button・hat・axis byte values、neutral frames、parse/commit、test vectorsの正本は[周辺機器開発ガイド](PERIPHERAL_DEVELOPMENT.md)です。本節はfrontendのserial-format選択とatomic reconnect／rollbackだけを規定し、wire byte形式や長さを再定義しません。

## 7.1 [必須要件] スタック概要
本章はWeb browserおよび既存HTTP clientとの公開境界です。GPUI nativeがlocal HTTP clientを選択した場合も既存のOrigin規則を適用し、GPUI専用allow-Originを追加しません。in-process adapterを選択した場合はHTTP Origin経路を使用しません。server絶対pathなど既存routeがTauriに限定される機能を、GPUIのために拡張してはなりません。必要なnative保存は安全なbyte-return／local-save経路またはadapterをPoCで選択します。

```
カメラ映像:     WebRTCビデオトラック ──→ Motion JPEG over WebSocket フォールバック
コントローラー入力: WebRTC DataChannel ──→ WebSocket フォールバック
ログ/イベント:  WebRTC DataChannel ──→ WebSocket フォールバック
API呼び出し:    HTTP REST（axum）     ──→ （フォールバック不要）
```
## 7.2 [必須要件] WebRTC（プライマリ）
- **ビデオ**: ビデオトラックを使用したWebRTC `RTCPeerConnection`。
- **DataChannel**: コントローラー入力イベントとログストリーミング用。
- **シグナリング**: WebSocket上のJSONメッセージでSDP Offer/Answer/ICE candidateを交換。STUNサーバーは`settings.toml`等から設定できるが、既定値は空文字でありSTUNを使用しない。コーデック優先順位: H.264 > VP8 > VP9。
- **WebSocket再接続**: シグナリングWebSocket自体の切断再接続は§3.4に従う。
- **フォールバック条件**:
  - WebRTC接続が5秒以内に完了しない → WebSocketフォールバック起動
  - 接続確立後、3秒間連続でフレーム/データが受信できない → WebSocketにフォールバック

**フォールバック中のWebRTC自動復旧**:

- WebSocketフォールバックの映像・入力・ログ通信を維持したまま、バックグラウンドでWebRTC接続を再確立する。
- `webrtc.auto_recover`のデフォルトは`true`。`false`の場合は自動復旧プローブを行わず、手動再接続だけを提供する。
- `webrtc.recovery_probe_interval_sec`のデフォルトは`30`秒。1以上の整数とし、フォールバックが継続する間はこの間隔で無期限に試行する。同時に複数の復旧試行を開始しない。
- 復旧試行中もMotion JPEGおよびWebSocket DataChannel代替経路を停止しない。WebRTCビデオトラックとDataChannelの両方が利用可能になった場合だけ、プライマリ経路へ原子的に切り替える。
- 切替成功後はWebSocketのMotion JPEG映像送信を停止する。シグナリングおよび将来の再フォールバックに必要なWebSocket接続は維持する。
- `auto_recover`を実行時に`false`へ変更した場合、未開始のプローブを取り消す。実行中プローブは安全に完了させるが、成功しても自動昇格せずフォールバックを維持する。`true`への変更は設定適用時から新しい間隔でプローブを開始する。
- 間隔変更は現在の待機を取り消し、設定適用時を起点として次回プローブを再スケジュールする。

**通信内容**:

| 種類 | 内容 | フォールバック |
|------|------|--------------|
| 映像 | WebRTCビデオトラック | Motion JPEG over WebSocket |
| コントローラー入力 | WebRTC DataChannel | WebSocket |
| ログ | WebRTC DataChannel | WebSocket |
| API呼び出し | HTTP REST | なし（HTTP必須） |

**配送セマンティクス（DataChannel／WebSocket共通）**:

- ボタン・キー・タッチの押下／解放等の離散状態遷移は、同一入力世代内で順序付きかつ重複安全に適用する。解放イベントをドロップ可能な非信頼チャネルだけへ委ねてはならない。各遷移には接続世代と単調増加シーケンスを持たせ、重複および古い世代を無視する
- スティック／ポインター移動等の連続値は低遅延を優先して中間更新をドロップできるが、同一世代の単調増加シーケンスにより古い値が新しい値を上書きすることを禁止し、常に最新値優先とする
- ログは同一接続中は発行順を維持する。低速なログ購読者のバックプレッシャーで入力配送を停止させず、入力とログのキュー／優先度を分離する。切断境界を越える完全配送は保証しない
- WebRTCとWebSocketの経路切替時は、新経路上で現在のボタン・キー・スティック・タッチ全状態を含む世代付きスナップショットを送信し、サーバーが旧世代状態を原子的に置き換えた確認後に増分配送を再開する。どの制御経路も利用できない場合はRustメインが全入力を強制解放する
- 上記を満たすDataChannelの本数、`ordered`、`maxRetransmits`等の具体構成は内部実装詳細とする。ただし、単一のHead-of-Line blockingによって連続入力または安全な解放がログ転送待ちになる構成は禁止する

**映像解像度変更とWebRTC**:

初期ネゴシエーション時、少なくとも全閉じられたキャプチャ解像度（640x360、1280x720、1920x1080）をカバーするビデオエンベロープをSDPで合意する。その後の解像度変更は同一ビデオトラック/ソース内でエンベロープ範囲内であれば再ネゴシエーションなしで遷移する（`RTCRtpSender.replaceTrack`相当）。送信側/エンドポイントが合意コーデックエンベロープ外またはビットレート制約により変更を拒否した場合、旧トラックを維持したままSDP Offer/Answer再ネゴシエーションを試行し、カメラトランザクションのコミットは成功後にのみ行う。失敗時はロールバックする。Motion JPEGフォールバックは即座に次フレームから新しい寸法で動作する。
## 7.3 [必須要件] Motion JPEG + WebSocket（フォールバック）
### 7.3.1 映像フォールバック — Motion JPEG
映像フォールバックとして、**Motion JPEG over WebSocket** を採用します。各カメラフレームをJPEGとしてエンコードし、WebSocket経由で個別のバイナリメッセージとして送信します。

- **エンコーダー**: サーバーサイドで各フレームをJPEGにエンコード。品質パラメータは `settings.toml` で設定可能（デフォルト: 85、範囲: 1-100）。品質はJPEGスクリーンショット保存時にも使用され、`jpeg_quality` 設定を共有する
- **転送**: WebSocket経由で1フレーム=1バイナリメッセージとして送信。各フレームは自己完結したJPEGであり、フレーム間依存性がない
- **デコード**: ブラウザ標準のJPEGデコーダを使用（WebCodecs等の特殊API不要）
- **描画**: デコード結果を Canvas に描画

**特性**:

| 項目 | 値 |
|------|------|
| 遅延 | 50-150ms |
| エンコード | JPEG（品質パラメータ設定可能） |
| **フレーム独立性** | 各フレームは完全なJPEG。TCP輻輳でフレーム遅延が発生しても次フレームで即座に回復し、デコーダ状態が壊れない。ただしフレーム独立性だけではWebSocket/TCPキュー内でのフレーム蓄積遅延を防げないため、明示的なバックプレッシャー制御が必要（下記参照） |
| **ブラウザサポート** | 全ブラウザ対応（JPEGは標準機能） |

**Motion JPEG採用の理由**:
- **フォールバック層の最優先事項は確実動作**: WebRTCが失敗する環境（UDPブロック、古いブラウザ等）でも確実に動作する必要がある
- **各フレーム独立**: TCP上のWebSocketでは、H.264等のフレーム間圧縮方式は貧弱なネットワークでTCPバッファにフレームが蓄積し遅延が累積する問題がある。Motion JPEGは各フレームが独立しているため、フレームのドロップ/リカバリが可能であり、蓄積遅延をドロップにより解消できる
- **全ブラウザ対応**: WebCodecsを必要とせず、JPEGデコードは全ブラウザ標準機能
- **画像認識用途への適合**: 各フレームが自己完結しているため、フレーム間予測に由来する破損が後続フレームへ伝播せず、個別フレームを独立してデバッグ表示できる。JPEG圧縮ノイズの程度は`jpeg_quality`に依存する

**バックプレッシャー制御**: 各クライアント（WebSocket接続）に対して、未送信のビデオフレームは最大1つまでとする。より新しいフレームが到着した場合、キュー内に未送信のフレームが存在すればそれを破棄し、新しいフレームに置き換える。これにより、クライアントの処理能力を超えたフレームがWebSocket/TCPキューに無制限に蓄積されることを防ぐ。

### 7.3.2 コントロール/ログフォールバック — WebSocket
- **エンドポイント**: `/ws`。
- **メッセージ**: JSON形式。
- **自動再接続**: §3.4参照。
- **イベント／メッセージunion**:

  | `type` | 方向 | `revision` | `data` |
  |--------|------|------------|--------|
  | `ui.state.changed` | サーバー → クライアント | 必須 | UI可視状態の原子的変更通知。下記`UiStateChange` |
  | `serial.data` | サーバー → クライアント | なし | `{"encoding":"base64","data":string,"byte_length":int}`。`data`は受信した生バイト列の標準Base64 |
  | `log` | サーバー → クライアント | なし | `{"level":"debug"|"info"|"warning"|"error"|"critical","message":string,"target":"stdout"|"panel1"|"panel2"|"log","operation":"append"|"replace"|"clear"}`。`clear`では`message`を空文字とする |
  | `script.ui` | サーバー → クライアント | なし | アクティブなユーザースクリプト世代が所有するダイアログ、Tk互換ウィンドウ、描画オーバーレイ、画像ポップアップの完全スナップショット。非アクティブ時は`generation=null` |
  | `webrtc.offer` | 双方向 | なし | `{"sdp":string,"negotiation_id"?:string|null}`。クライアント起点のmanual offerでは一意な不透明IDを付与し、serverは同じIDをanswer/ICEへ引き継ぐ |
  | `webrtc.answer` | 双方向 | なし | `{"sdp":string,"negotiation_id"?:string|null}`。offerのIDがある場合は同じ値を返す。受信側は現在のnegotiationと一致しないIDを破棄する |
  | `webrtc.ice_candidate` | 双方向 | なし | `{"candidate":string,"negotiation_id"?:string|null,"sdp_mid":string|null,"sdp_mline_index":int|null,"username_fragment":string|null}`。offer/answerと同じnegotiationに属する候補だけを適用する |
  | `input.generation` | サーバー → クライアント | なし | `{"generation":string}`。制御経路確立・切替時にサーバーが割り当てる不透明な非空ASCII識別子 |
  | `input.snapshot` | クライアント → サーバー | なし | 現在のボタン・キー・両スティック・タッチ全状態、`generation`、`sequence` |
  | `input.snapshot.applied` | サーバー → クライアント | なし | `{"generation":string,"sequence":string}`。指定スナップショットの原子的適用完了確認 |
  | `ping` | サーバー → クライアント | なし | `{"nonce":string}` |
  | `pong` | クライアント → サーバー | なし | 対応する`{"nonce":string}` |

**共通JSON外形**:

- JSONメッセージは`type`を判別子とするOpenAPI componentのdiscriminated unionとして定義し、各variantは上表のフィールドだけを許可して`additionalProperties=false`とする。§7.5〜§7.7のキーボード／マウス／ゲームパッド入力variantも同じ生成unionへ含める
- `ui.state.changed`は`{"type":"ui.state.changed","revision":"<非負10進整数>","data":<UiStateChange>}`、その他は`{"type":"...","data":...}`とし、revisionを持たないvariantへ`revision`を追加しない
- `UiStateChange`は`cause`、`state`、`settings`を持つ。`cause`は`"settings"`／`"camera"`／`"serial"`／`"command"`／`"profile"`／`"dynamic_config"`／`"commands"`／`"shutdown"`／`"other"`の閉じたenum。`state`は`GET /api/state`の`data`から`revision`を除いた各プロパティをoptionalにした`additionalProperties=false`の疎な`StatePatch`。変更のないプロパティを含めない。`settings`は設定変更がない場合`null`、ある場合は正準IDをoptionalプロパティとする疎な`values`、`pending_restart_values`、`restart_required`、`apply_failures`を持ち、secretは[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.4.3.5のマスク済み表現だけを使用する
- UI可視状態を変更する1つの原子的トランザクションにつき、revisionを1回だけ増加させ、同じrevisionの`ui.state.changed`を正確に1件送信する。1トランザクションで設定・状態・コマンド表示一覧が同時に変化する場合も同じ`data`内へ両方の疎な差分を含め、同revisionの複数イベントへ分割しない
- `command_display_lists`、`command_candidates`、`tags`のいずれかが変わる場合、`state`には三つを同じ完成世代の完全値としてまとめて含める。未完成世代や異なる世代の組合せを送信しない
- `command.error`等の従来の個別WebSocket通知名は設けず、`cause="command"`の`ui.state.changed`と`StatePatch.command_state`／`current_command`で表す。診断詳細は共通RESTエラーまたは`log` variantで通知する。動的設定イベント名との対応関係は持たない
- `script.ui.data`は`generation:string|null`、`dialogs`、`tk_windows`、`overlay`、`popup_images`をすべて必須で持つ、世代内の最新完全値である。部分差分を送らず、ブラウザは受信ごとに直前のスクリプトUI投影を原子的に置換する。サーバーはアクティブ世代の最新値を接続ごとに即時再送し、ワーカー停止・プロファイル切替後は`generation=null`の空値を送る。クライアントは過去世代のREST操作を再送せず、世代不一致応答を破棄して最新スナップショットを待つ

**入力variantの共通外形**:

- §7.5〜§7.7の全入力メッセージも`{"type":"...","data":{...}}`外形を使用し、`data`には各節固有のフィールドに加えて`generation: string`と`sequence: string`を必須とする。`sequence`は同一generation内で0から開始する単調増加の非負10進整数文字列であり、JavaScript整数精度へ依存しない
- 経路確立・切替時、サーバーは新しい`input.generation`を発行する。クライアントはそのgenerationと`sequence="0"`を持つ`input.snapshot`を送信し、サーバーが全入力状態を原子的に置換した確認を返した後にだけ`sequence="1"`以降の増分入力を送信する。古いgeneration、重複sequence、現在値以下のsequenceは副作用なしで無視する
- `input.snapshot.data`は`generation: string`、`sequence: "0"`、`keyboard_keys: list[str]`、`mouse_buttons: {left:bool,right:bool,middle:bool}`、`buttons: {a:bool,b:bool,x:bool,y:bool,l:bool,r:bool,zl:bool,zr:bool,lclick:bool,rclick:bool,plus:bool,minus:bool,home:bool,capture:bool}`、`hat: "up"|"down"|"left"|"right"|"up_right"|"up_left"|"down_right"|"down_left"|"neutral"`、`left_stick: {x:int,y:int}`、`right_stick: {x:int,y:int}`、`touch: {x:int,y:int,pressed:true}|null`をすべて必須で持ち、各objectは`additionalProperties=false`とする。スティックは0〜255、touchはx=0〜319／y=0〜239。押下中touchがない場合だけ`touch=null`とする
- `keyboard_input`は`key:string`と`state:"pressed"|"released"`、`mouse_stick_input`は`stick:"LSTICK"|"RSTICK"`と0〜255の`x`／`y`、`mouse_input`は`button:"left"|"right"|"middle"`、`state:"pressed"|"released"`、非負の描画領域ピクセル`x`／`y`を持つ。`gamepad_input`は`kind`を第二判別子とし、`button`は`button`＋`state`、`stick`は`stick`＋`x`＋`y`、`hat`は`hat`、`touch`は`touch`だけを許可する。各形は他の形のフィールドを同時に含めず、`additionalProperties=false`とする

**初期化・再接続のrevision整合**:

1. SPAは最初にWebSocketへ接続し、`ui.state.changed`を一時保持する
2. 接続後に`GET /api/settings`と`GET /api/state`を並行取得し、それぞれのスナップショットrevisionを記録する
3. 保持イベントをrevision順に処理し、同一`instance_id`内では`settings`差分を設定スナップショットrevisionより新しい場合だけ、`state`差分を状態スナップショットrevisionより新しい場合だけ適用する。片方だけ新しい場合はその領域だけ適用する。再取得した設定snapshotの`instance_id`が現在の基準と異なる場合はrevision比較をリセットして新しいsnapshotを基準にし、退役済みinstanceから遅れて届いたsnapshotは適用しない
4. 保持イベントのrevisionに欠落がある、同revisionが複数ある、または差分を型検証できない場合は推測せず、両GETを再実行して新しい基準を作る
5. WebSocket再接続時も同じ手順を使用する。`serial.data`、`log`、シグナリング等のrevisionなしイベントはスナップショット再生対象にしない。`script.ui`だけはイベントではなく最新完全値であるため、上記の独立した世代付きスナップショットを再送する

上記全variantをutoipaのOpenAPI componentへ登録し、`openapi-typescript`でRustと同じ判別unionを生成する。WebSocketが内部通信であることは、型契約を実装時の口頭合意へ委ねる理由にはならない。
## 7.4 [必須要件] HTTP REST API
HTTP APIはWeb browserと外部clientの公開契約です。GPUI nativeが既存HTTP clientを使うかtyped in-process adapterを使うかはPoCで選択し、同じAPI semanticsを適用します。in-process adapterを使う場合もbackendのcanonical state、resource、lifecycleを所有せず、HTTP APIと重複する公開endpointを追加しません。

- **フレームワーク**: axum（Rustコア）。
- **ドキュメント**: OpenAPI仕様を使用したutoipa v5。
- **コード生成**: TypeScriptクライアント型用の `openapi-typescript`。生成結果はgit追跡するが、前回成功時の生成結果を型生成失敗時のフォールバックとして使用してはならない。Rust/OpenAPI対象ソースが存在する場合、生成失敗または追跡済み生成結果との差分はローカル検査・ビルド・CIのハードエラーとする。対象ソースがまだ存在しない仕様確定段階だけ、理由を明示したnoticeを出して成功スキップする
- **認証**: なし（ローカル/LAN専用）。
- **Host／Origin自動導出**: 設定は`server.bind_address`だけを使用し、別の許可Origin設定は設けない。HTTP用の許可Host／Originは実効`server.bind_address`と`server.port`から自動導出する。IPv4は`http://<address>:<port>`、IPv6は`http://[<address>]:<port>`とする。ループバック`127.0.0.0/8`または`::1`の場合だけ`http://localhost:<port>`も同値として許可する。Tauri desktop modeでは`tauri://localhost`も許可する。GPUI nativeがHTTP clientを選ぶ場合は、この既存Host／Origin境界を変更せず適用し、満たせない操作はin-process adapter経由とする。
- **HTTP Host／CORS検証**: すべてのHTTPリクエストでHostヘッダーのホスト・ポートが上記導出集合と一致することを検証し、不一致を拒否する。ブラウザOriginがあるRESTリクエストは同じ導出Origin集合と完全一致する場合だけ許可する。状態変更RESTは`Content-Type: application/json`と固定カスタムヘッダー`X-Pokecon-Request: 1`を必須とし、HTML formによる単純リクエストを受理しない。Originなしの非ブラウザクライアントもHost・Content-Type・固定ヘッダー検証を満たす必要がある
- **WebSocketハンドシェイクOrigin検証**: WebSocketアップグレード時も同じHost／Origin集合を使用する。Originがない、Host不一致、または許可集合外のOriginは例外なく拒否する。HTTP CORSヘッダーだけへ依存せず、新たな認証機構や未定義の内部バイパスを追加しない
- **Vite開発プロキシ**: 起動側は解決済み`server.bind_address`と`server.port`を`POKECON_BIND_ADDRESS`／`POKECON_PORT`としてViteへ渡す。`/api`・`/ws` proxyはターゲットHostとOriginをこの実効値へ書き換える。未指定時だけ`127.0.0.1`／`8020`を使用し、設定変更のために`vite.config.ts`を手編集しない
- **モジュール**: REST APIは設定、状態、コマンド、デバイス、通知等のリソース単位でモジュール分割する。公開契約は本節のパス・メソッドであり、Rust内部のモジュール名は規定しない。
- **設定API**:

  | メソッド | パス | 動作 |
  |----------|------|------|
  | `GET` | `/api/settings` | 正準設定レジストリでOpenAPI読み取り対象となる現在の実効設定を一括取得する |
  | `PATCH` | `/api/settings` | 正準設定IDをキーとする疎な設定集合を一括更新する |

  - `PATCH`のJSON要求は`{"expected_revision":"42","values":{"serial.baud_rate":115200,"serial.data_format":"3ds"}}`形式とし、指定されていない設定は変更しない。`expected_revision`はoptionalな非負10進整数文字列で、省略時は従来どおり最終書き込み勝ちとする。全UI可視状態の確定を直列化する単一のrevisionトランザクションゲートを使用し、各設定クラスの事前検証後、確定直前にこのゲートを取得して`expected_revision`を比較する。不一致ならデバイスの事前適用を含む当該クラスのロールバックを実行し、設定・TOML・revisionを変更せず現在の`revision`を含む`409 Conflict`を返す。一致時だけゲート保持中に設定スナップショットを確定してrevisionを1回増加させる。公式SPAは直前に取得・適用したrevisionを常に指定し、プロファイル切替後を含む他クライアントとの競合を検出する
  - OpenAPIでは`values`を`Record<string, ...>`や単一の巨大な値unionにせず、OpenAPI対象の各正準IDをリテラルなプロパティ名、その設定固有の型をプロパティ型として列挙した生成スキーマにする。`GET`用は全読み取り対象プロパティを必須、`PATCH`用は全書き込み対象プロパティをoptional、双方とも`additionalProperties=false`とし、正準IDと値型の不正な組み合わせをTypeScript LSPで検出可能にする
  - 要求内の全設定を型、値域、相互制約、書き込み可否、保存先、下記の要求クラスについて先に検証する。1件でも無効なら変更を一切適用せず、設定IDごとの診断を含む`422 Unprocessable Entity`を返す
  - 1要求は次のいずれか一つのクラスだけに属さなければならない: (A) `active_profile`単独、(B) カメラ取得トランザクション集合`camera.device`／`camera.capture_fps`／`camera.capture_resolution`の任意の部分集合、(C) シリアルトランザクション集合`serial.port`／`serial.baud_rate`／`serial.data_format`の任意の部分集合、(D) A〜Cを含まない通常設定集合。同じ要求でクラスを混在させた場合は`422`とする
  - クラスDの全設定は同じ永続化先に属さなければならない。グローバル`settings.toml`向けと現在のプロファイル`settings.toml`向けを同じ要求に混在させた場合は`422`とし、呼び出し側が保存先ごとに分割する。これにより複数ファイルをまたぐ擬似的な原子性を主張しない
  - クラスAは[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.5.6.4.3のプロファイル切替トランザクションだけを実行する。クラスB／Cは対応する専用デバイストランザクションを1回実行し、ランタイム適用とTOML保存の両方が成功した場合だけ設定値を確定する。適用・保存失敗時は当該専用ロールバックを実行し、設定値・対象TOML・revisionを変更せず`409 Conflict`を返す。ロールバック自体に失敗した場合も設定値は旧値のままとし、該当サブシステムを利用不能として秘匿化済み診断を返す
  - クラスDは対象TOMLパスの設定ロック下で最新内容を1回読み、全変更を1つの一時ファイルへ反映して1回の原子的置換で保存する。保存成功後、正準設定サービスの全値を1つの新しいスナップショットへ同時に切り替える。通常の`runtime_immediate`適用ハンドラは事前検証後に失敗しない設計とする。予期しない適用失敗が発生した場合も設定スナップショットの一部だけを旧値へ戻さず、保存・確定済みの新値を維持して該当機能だけを利用不能とする。設定UI／`GET`／`PATCH /api/settings`は利用可能なまま維持し、ユーザーは後続PATCHで値を修正できる。次回起動時は保存値の適用を再試行して同じ失敗を診断し、設定者の値を暗黙に既定値へ置換しない
  - 未知の正準ID、OpenAPI書き込み対象外、scopeが`bootstrap`の`app_name`、`dynamic_config_language`、`python.dynamic.venv`、`python.dynamic.packages.*`を`PATCH`へ含めた場合も`422`とする。mutabilityが`startup_only`でもscopeが`global`／`profile`の設定とは区別する
  - startup-only設定は更新・永続化を受理するが、現在のプロセスへは適用せず、応答の`pending_restart_values`と`restart_required`へ該当IDを含める
  - secret設定の読み取り値と更新後応答は[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.4.3.5のマスク規則に従い、平文を返さない。`PATCH`で現在設定済みsecretの固定マスク文字列`"********"`を受け取った場合は、そのIDを変更集合から除外して既存値を維持する。空文字は明示的な消去、それ以外の有効値は置換とする。マスクだけを含む要求は成功するno-opであり、revisionを増加させない
  - `GET`応答`data`は`revision`、全読み取り対象の現在実効値を持つ`values`、現在プロセスでは未適用のstartup-only保存値だけを持つ疎な`pending_restart_values`、その正準ID一覧`restart_required`を返す。`PATCH`成功応答も同じ形を返し、正規化後の値と再起動要否を呼び出し側が推測せず確認できるようにする。さらにクラスDで予期しないランタイム適用失敗が発生した場合だけ、正準IDをキー、秘匿化済み診断を値とする疎な`apply_failures`を返す。通常時と`GET`では`apply_failures`は空オブジェクトとする
  - `GET`は現在のアクティブプロファイルを反映した実効値を返す。API独自のscope指定は設けない
  - `PATCH`は正準設定レジストリのscopeに従い、グローバル専用設定をグローバル`settings.toml`へ、プロファイル対応設定を現在のプロファイル`settings.toml`へ保存する。上記の同一保存先規則により、1要求が両方へ書き込むことはない
  - `active_profile`は常に単独の`PATCH`要求とし、他のグローバル専用設定を含めても要求全体を`422`で拒否する。呼び出し側は切替完了後の`revision`と設定・状態を再取得してから、別の`PATCH`で後続設定を更新する
  - `GET`応答と`PATCH`成功応答はUI可視状態と共通の`revision`を含む。設定変更時は同じrevisionを持つ`ui.state.changed`を全クライアントへ通知し、`data.settings`に当該トランザクションの疎な設定差分を含める
  - 設定ごとの専用RESTエンドポイントおよび`/api/settings/{id}`形式は設けない
- **操作トランスポート**: コマンド制御、通知テスト、デバイス再スキャン等のバックエンド処理を伴う非ストリーミング離散UI操作はHTTP RESTへ統一する。クライアント表示だけを変える操作はREST化しない。WebRTC DataChannel／WebSocketはコントローラー入力、映像・ログ等のストリーミング、状態・イベント通知に使用し、離散操作の要求APIを重複定義しない。REST操作でUI可視状態が変化した場合はHTTP応答を返すとともに、同じrevisionの`ui.state.changed`を接続中の全クライアントへ通知する
- **コマンド操作API**:

  | メソッド | パス | 動作 |
  |----------|------|------|
  | `POST` | `/api/commands/control` | コマンド実行状態を`start`／`stop`／`pause`／`resume`のいずれかで制御する |
  | `POST` | `/api/commands/reload` | ファイルシステムからコマンド候補一覧を再読み込みし、タグ統合と全タグ分の表示一覧キャッシュを再構築する |

  - `/api/commands/control`要求は`action`を判別子とするOpenAPIのdiscriminated unionとする。`{"action":"start","command":{"module_path":"...","class_name":"..."}}`では`command`を必須とし、`stop`／`pause`／`resume`では`command`フィールドを許可しない
  - `start.command`は現在の`command_candidates`にある`module_path`と`class_name`の組へ解決する。未解決の場合は`404 Not Found`とし、名前だけによる曖昧な解決を行わない
  - `start`は`stopped`または`error`からだけ許可する。`running`／`paused`では実行中コマンドを暗黙に停止・置換せず`409 Conflict`を返す
  - `stop`は全状態から許可し、すでに`stopped`の場合は成功するno-opとする
  - `pause`は`running`から`paused`への遷移とし、すでに`paused`の場合は成功するno-op、その他の状態では`409`とする
  - `resume`は`paused`から`running`への遷移とし、すでに`running`の場合は成功するno-op、その他の状態では`409`とする
  - `/api/commands/reload`要求本文は空JSONオブジェクト`{}`とする
- **スクリプトUI操作API**:

  | メソッド | パス | 動作 |
  |----------|------|------|
  | `POST` | `/api/script-ui/action` | 現在のユーザースクリプト世代が所有するダイアログ、Tk互換ウィンドウ、画像ポップアップへの離散操作を適用する |

  - 要求は`action`を判別子とする閉じたOpenAPI unionとし、`dialog_confirm`、`dialog_abort`、`tk_scale_changed`、`tk_button_invoked`、`tk_window_closed`、`popup_closed`だけを受理する。全variantで直前の`script.ui.data.generation`を必須とし、世代不一致または既に消滅したUIオブジェクトは副作用なしの`409 Conflict`とする
  - `dialog_confirm`はWidget数、型、選択肢、数値範囲をRust側で再検証してから確定する。×／Escによる`dialog_abort`は当該ダイアログを中断し、キャンセル可能な`CommandStopPre`を経由せず異常停止としてコマンド世代を必ず解放する
  - Tk互換のScale／Button／window close操作は型付きIPCイベントとして所有ワーカーへ配送する。画像ポップアップcloseは表示資源だけを破棄し、コマンドを停止しない。各成功後は更新済み完全`script.ui`スナップショットを全接続へ通知する
- **状態API**:

  | メソッド | パス | 動作 |
  |----------|------|------|
  | `GET` | `/api/state` | SPA初期化に必要な現在の非設定ランタイム状態を一括取得する |

  - 応答`data`は`revision`に加え、[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.5.6.3と同名・同型の`serial_port`、`serial_baud_rate`、`serial_connected`、`camera_opened`、`camera_fps`、`camera_resolution`、`camera_device`、`is_running`、`command_state`、`current_command`、`command_candidates`、`tags`、`active_profile`、`pending_profile`、`available_profiles`、`last_input`、`holding_buttons`、`pid`を必須フィールドとして持つ。さらに`command_display_lists: dict[str, list[CommandDisplayItem]]`と`command_display_cache_loading: bool`を持つ。`command_display_lists`のキーは`"-"`および確定済み全タグである
  - 表示一覧キャッシュの`CommandDisplayItem` OpenAPI wire型は`kind`を判別子とするunionとし、コマンド行は`{"kind":"command","command":<CommandInfo>}`、セパレーター行は`{"kind":"separator","label":string|null}`で表す。このwire表現はHTTP／WebSocket境界専用であり、動的設定callbackの`CommandInfo | CommandSeparator`インターフェースへラッパーを要求しない
  - 設定レジストリの値は重複して完全収録せず`GET /api/settings`から取得する。上記状態フィールドは[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.5.6.3と同じ値契約を使用する
  - 取得後のUI可視状態変化は`ui.state.changed`で通知し、`data.state`に当該トランザクションの疎な`StatePatch`を含める。全体再取得は§7.3.2の初期化・再接続手順に従う
  - `GET /api/settings`、`PATCH /api/settings`、`GET /api/state`の応答、および`ui.state.changed`は、UI可視状態の原子的トランザクションごとに1回増加する単一のプロセス内グローバル`revision`カウンターを共有する。サーバープロセス起動時に`0`から開始し、同一プロセス内では減少・再利用・周回させない。JSON/OpenAPI上は非負10進整数文字列として表現し、クライアントは10進整数として比較する。文字列の辞書順比較やJavaScript `Number`の安全整数範囲へ依存せず、`BigInt`または桁数＋同桁辞書順等の正確な整数比較を使用する。設定snapshotはさらにプロセス起動ごとに生成する`instance_id`を必須で持つ。同一プロセスの全settings snapshotでは不変とし、プロセス再起動時はrevisionが`0`へ戻っても新しい`instance_id`を使用する。クライアントはinstance IDが変わった場合だけ新しいrevision基準へ切り替え、旧instanceから遅れて届いたsnapshotを適用しない
  - SPAの初期化・WebSocket再接続・revision欠落時の再取得と差分再生は§7.3.2の規範手順だけを使用し、本節で別の順序を定義しない
- **デバイスAPI**:

  | メソッド | パス | 動作 |
  |----------|------|------|
  | `GET` | `/api/devices/cameras` | 利用可能なカメラを再列挙する |
  | `GET` | `/api/devices/serial-ports` | 利用可能なシリアルポートを再列挙する |
  | `POST` | `/api/serial/control` | `connect`／`disconnect`でシリアル接続状態を制御する |
  | `POST` | `/api/camera/retry` | 現在の`camera.device`生セレクターで[フロントエンド定義書](SPECIFICATION_FRONTEND.md) §6.1.2のカメラ切替トランザクションを再試行する |

  - カメラ列挙結果は`selector: int | str`、表示専用`label: str`、`available: bool`を持つ。シリアル列挙結果は`selector: str`、表示専用`label: str`、`available: bool`を持つ。各GET自体が再スキャンを行うため、別のrefreshエンドポイントは設けない
  - 現在設定済みの生セレクターが列挙時に不在でも、`available=false`の項目として結果へ含める。列挙・重複排除・生セレクター保持規則は[バックエンド定義書](SPECIFICATION_BACKEND.md) §6.1.6および[フロントエンド定義書](SPECIFICATION_FRONTEND.md) §6.2.1に従う
  - `/api/serial/control`要求は`{"action":"connect"}`または`{"action":"disconnect"}`のdiscriminated unionとし、`connect`は現在の実効`serial.*`設定を使用する。すでに目標状態なら成功するno-opとし、接続失敗時は設定を変更せず秘匿化済み診断を返す
  - `/api/camera/retry`要求本文は`{}`とする。カメラがエラー／閉状態でない場合は再オープンせず成功するno-opとする
- **スクリーンショットAPI**:

  | メソッド | パス | 動作 |
  |----------|------|------|
  | `POST` | `/api/camera/screenshot` | 現在の公開カメラフレームの全体または指定矩形をPNG／JPEGで保存または返却する |

  - 要求は共通の`region`（省略時は全体、指定時は[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.4.1.6と同じ0.0～1.0の正規化矩形）と、`destination`を判別子とするunionを持つ
  - `{"destination":"captures","filename":string|null,"format":"png|jpeg"|null,"overwrite":bool}`は実効Dataルートの`Captures/`へ保存し、`overwrite`の既定は`false`、`format=null`では実効`camera.screenshot_format`を使用する。`filename`は`null`／空文字または相対パスだけを受理し、絶対パス、ドライブ／UNCプレフィックス、正規化後またはシンボリックリンク解決後に`Captures/`外となる値を`422`で拒否する。絶対保存は次の`path` variantだけを使用する。成功応答は保存先の表示用相対パスと実効形式を返し、サーバーホストの絶対Dataパスを公開しない
  - `{"destination":"path","path":"...","format":"png|jpeg","overwrite":bool}`は現行Tauri desktopのnative保存dialogで利用者が明示選択したserver host上の絶対pathだけをHTTP経由で受理し、`overwrite`の既定は`false`とする。既存対象へ`overwrite=true`を送るのはネイティブダイアログで上書き確認が完了した場合だけとする。Webモードからは受理せず`409`とする
  - `{"destination":"download","filename":string|null,"format":"png|jpeg"}`はWebモードとGPUI nativeのHTTP clientで使用できる。成功時だけ共通JSON外形の例外として画像バイト列を該当`Content-Type`と`Content-Disposition: attachment`付きで返す。サーバーファイルシステムへ保存しない。GPUIはnative file chooserで選択したローカルpathへ受信bytesを書き、cancel／non-overwriteを守る。`filename=null`／空文字は§6.1.5の既定ベース名を使用する。明示名は単一のベース名だけを受理し、パス区切り、制御文字、CR／LF、NULを含む値を`422`で拒否する。`Content-Disposition`は引用符とRFC 5987 `filename*`を安全にエンコードし、入力文字列をヘッダーへ未加工で連結しない。Tauriでも使用可能とする。
  - カメラ未オープンまたは有効な公開フレームがない場合は`409`、退化矩形は`422`とし、別デバイスや全体画像へ暗黙にフォールバックしない
- **通知テストAPI**:

  | メソッド | パス | 動作 |
  |----------|------|------|
  | `POST` | `/api/notifications/test` | 現在の実効設定を使ってWindowsまたはDiscordのテスト通知を送信する |

  - 要求は`channel`を判別子とする`{"channel":"windows"}`または`{"channel":"discord"}`とする。Discord Webhook未設定・不正は`422`、非Windows環境でのWindows通知は`409`とし、別チャネルへ暗黙に切り替えない
- **動的設定操作API**:

  | メソッド | パス | 動作 |
  |----------|------|------|
  | `POST` | `/api/dynamic-config/control` | 動的設定ファイルの新規読み込みまたは現在ファイルの再読み込みを行う |

  - 要求は`action`を判別子とするunionとし、デスクトップのネイティブファイル選択結果を読み込む`{"action":"load_path","path":"..."}`、Webブラウザから選択した内容を読み込む`{"action":"load_content","language":"python|lua","content":"..."}`、現在ファイルを再読み込みする`{"action":"reload"}`を受理する
  - `load_path.path`は[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.4.1.4および[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.5.6.2のConfig基準・絶対パス・チルダ・閉じ込め規則で解決し、拡張子`.py`／`.lua`で言語を判定する。その他の拡張子は`422`とし、UIが言語を選択した場合も元ファイルを暗黙に改名しない
  - `load_content`はWebブラウザがローカルパスを公開できない場合の経路とする。バックエンドは`language`に応じて実効Configルートの`init.py`または`init.lua`へ内容を原子的に保存してから読み込む。既存ファイルがある場合はUIで明示的な上書き確認を得た要求だけを送る
  - `reload`は最後に成功した`load_path`の解決済みパス、または`load_content`／起動時読込の正準initファイルを再評価する。現在ファイルがない場合は`409`とする
  - 応答は現在ファイルの秘匿化済み表示パス、言語、読み込み成否を返す。読み込み失敗時は前回の有効な動的設定を維持する既存フォールバック規則に従う
  - 非ループバックの`server.bind_address`でLAN公開している場合も、`load_path`／`load_content`／`reload`を接続元IPやクライアント種別で制限しない。認証を追加せず、§15.11の完全信頼境界としてLANクライアントによるサーバーホスト上のPython／Luaコード保存・評価を許可する
  - 「Open Config Directory」はnative desktop shellのローカル機能で実効ConfigルートをOS file managerに開く。Tauri／GPUIは各shellのfile manager起動機能を使い、Webモードでは実効Configルート文字列とコピーボタンを表示し、LANクライアントからサーバーホストのファイルマネージャーを起動するREST APIは設けない
- **プロファイルランチャー／更新確認API**:

  | メソッド | パス | 動作 |
  |----------|------|------|
  | `POST` | `/api/profiles/generate-launcher` | Windows用起動BATと対象プロファイルディレクトリを生成する |
  | `POST` | `/api/update/check` | 現在バージョンと配布元の最新バージョンを比較する |

  - ランチャー生成要求は`profile: str`、`copy_current: bool`、`destination`を持つ。プロファイル名は`active_profile`と同じ安全な単一パスコンポーネント規則で検証する。プロファイルが存在しない場合は実効Configルートの`profiles/<profile>/`を作成し、`copy_current=true`では現在のプロファイル内容を複製、`false`では空ディレクトリを作成する
  - 生成BATは現在の実行可能ファイルを引用符付きで呼び出し、`--profile <profile>`を渡す。`destination`は`{"kind":"path","path":"..."}`または`{"kind":"download","filename":string|null}`のunionとする。Tauriの`path`はネイティブ保存ダイアログで選択したWindowsホスト上の絶対`.bat`パスへ保存し、Webの`download`およびGPUIのHTTP clientでは同じ内容を`Content-Disposition: attachment`で返し、GPUIはnative file chooserで選んだlocal pathへ保存する。既存プロファイルは内容を変更せず`copy_current`も適用しない。Tauriで既存BATがある場合も上書きせず成功するno-opとし、応答で`profile_created`／`launcher_created`を個別に返す
  - 新しいプロファイルを作成した場合は`available_profiles`と共通revisionを更新し、状態変更WebSocketイベントを送信する
  - ランチャー生成はWindows専用とし、非Windowsではメニューを無効化して理由を表示する。直接要求された場合は`409`とする
  - 更新確認要求本文は`{}`とし、成功時は現在バージョン、最新バージョン、更新有無、配布ページURLを返す。確認失敗は現在アプリの動作へ影響させない
  - GitHub／ガイド／質問テンプレート／LICENSEは既定ブラウザで静的URLを開き、バージョン表示／更新履歴はバンドル済みメタデータを表示し、画面サイズのリセットはTauriまたはGPUIのnative window、もしくは現行SPAのローカル表示状態だけを変更する。これらにはREST APIを設けない
- **出力クリア**: 「出力をクリア」は各SPAクライアントが保持する出力#1／出力#2の表示バッファだけを消去するクライアントローカル操作とし、REST要求や他クライアントへの通知を行わない。バックエンドのログ履歴・永続ログは削除しない
- **応答形式**:
  - 成功応答は`{"data": <エンドポイント固有の型>}`、失敗応答は`{"error":{"code":"<安定した機械可読コード>","message":"<秘匿化済み表示文>","fields":<フィールド別診断またはnull>}}`を共通外形とする
  - `fields`は検証対象のフィールド名または正準設定IDをキー、診断文字列配列を値とするマップであり、フィールド別診断がない場合は`null`とする。エンドポイント固有の成功型と列挙可能な`error.code`はutoipaスキーマへ明示し、未型付けの任意JSON応答を使用しない
  - JSON構文不正は`400 Bad Request`、型・値・相互制約違反は`422 Unprocessable Entity`、現在状態との競合は`409 Conflict`、対象不存在は`404 Not Found`、予期しない内部失敗は`500 Internal Server Error`を基本とする。各操作でより具体的な状態コードを定義した場合はその定義を優先する
- **静的ファイル配信**: axumは `server.web_dir`（[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.4.2、§15.9参照）で指定されたディレクトリからSPA静的ファイル（`index.html`・JS・CSS・画像等）を配信する。デフォルト値はアプリケーションリソースルート基準の `web/dist`（バンドルされたSvelteKit SPAビルド出力）。起動時に静的ファイルルートが決定され、ランタイム中の動的差し替えは行わない。詳細は§15.9参照

**注**: WebSocket／DataChannelメッセージ名はUIとバックエンド間の内部通信プロトコルであり、ユーザーAPIではない。wire名・判別子・型は§7.3.2および本節で確定しており、実装時に別名へ変更しない。
## 7.5 [必須要件] キーボード入力API
キーボード入力は低遅延が要求されるため、**WebRTC DataChannel**または**WebSocket**を使用。**HTTP RESTは使用しない**。

| 通信方式 | 用途 | フォールバック |
|---------|------|--------------|
| WebRTC DataChannel | プライマリ — キー入力イベント送信 | WebSocket |
| WebSocket | フォールバック — キー入力イベント送信 | なし |

**入力イベント形式**（WebSocket / DataChannel共通）:

```json
{
  "type": "keyboard_input",
  "data": {
    "generation": "connection-42",
    "sequence": "1",
    "key": "F5",
    "state": "pressed"
  }
}
```
## 7.6 [必須要件] マウス入力API
マウス入力（スティック操作）は低遅延が要求されるため、**WebRTC DataChannel**または**WebSocket**を使用。**HTTP RESTは使用しない**。

| 通信方式 | 用途 | フォールバック |
|---------|------|--------------|
| WebRTC DataChannel | プライマリ — マウス/スティック入力イベント送信 | WebSocket |
| WebSocket | フォールバック — マウス/スティック入力イベント送信 | なし |

**入力イベント形式**（WebSocket / DataChannel共通）:

```json
{
  "type": "mouse_stick_input",
  "data": {
    "generation": "connection-42",
    "sequence": "2",
    "stick": "LSTICK",
    "x": 128,
    "y": 128
  }
}
```

```json
{
  "type": "mouse_input",
  "data": {
    "generation": "connection-42",
    "sequence": "3",
    "button": "left",
    "state": "pressed",
    "x": 100,
    "y": 200
  }
}
```

**設定取得/変更**: マウスによる左右スティック駆動の有効・無効は、正準設定`input.left_stick_mouse_enabled`／`input.right_stick_mouse_enabled`のOpenAPI R/W表面を使用する（[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.4.2）。専用の`/api/controller/mouse_stick`エンドポイントは提供しない。`sensitivity`設定は正準レジストリに存在せず、本APIの設定として発明しない。
## 7.7 [必須要件] ゲームパッド入力API
ゲームパッド入力は低遅延が要求されるため、**WebRTC DataChannel**または**WebSocket**を使用。**HTTP RESTは使用しない**。

| 通信方式 | 用途 | フォールバック |
|---------|------|--------------|
| WebRTC DataChannel | プライマリ — ゲームパッド入力イベント送信 | WebSocket |
| WebSocket | フォールバック — ゲームパッド入力イベント送信 | なし |

**入力イベント形式**（WebSocket / DataChannel共通）:

```json
{
  "type": "gamepad_input",
  "data": {
    "generation": "connection-42",
    "sequence": "4",
    "kind": "button",
    "button": "A",
    "state": "pressed"
  }
}
```

```json
{
  "type": "gamepad_input",
  "data": {
    "generation": "connection-42",
    "sequence": "5",
    "kind": "stick",
    "stick": "LSTICK",
    "x": 128,
    "y": 128
  }
}
```

```json
{
  "type": "gamepad_input",
  "data": {
    "generation": "connection-42",
    "sequence": "6",
    "kind": "touch",
    "touch": {
      "x": 160,
      "y": 120,
      "pressed": true
    }
  }
}
```

**設定取得/変更**: 本節は正規化済みゲームパッド入力イベントの転送だけを規定し、ゲームパッドタイプ設定や専用RESTエンドポイントを提供しない。ProController／Xinputを選択するハードウェア制御は初期バージョンでは非表示の将来機能であり、[フロントエンド定義書](SPECIFICATION_FRONTEND.md) §6.3.2に従う。

- **対応ボタン**: A、B、X、Y、L、R、ZL、ZR、LCLICK、RCLICK、MINUS、PLUS、HOME、CAPTURE。
- **十字キー（Hat）**: UP、DOWN、LEFT、RIGHT、TOP_RIGHT、BTM_RIGHT、BTM_LEFT、TOP_LEFT、CENTER。

`gamepad_input.kind="hat"`の大文字`GamepadHat`は入力操作enumです。`input.snapshot.data.hat`の小文字値は正規化済みの現在状態enumであり、同じwire enumではありません。backendは次表で状態値へ変換します。

| GamepadHat入力値 | snapshot.hat状態値 |
|---|---|
| `UP` | `up` |
| `DOWN` | `down` |
| `LEFT` | `left` |
| `RIGHT` | `right` |
| `TOP_RIGHT` | `up_right` |
| `BTM_RIGHT` | `down_right` |
| `BTM_LEFT` | `down_left` |
| `TOP_LEFT` | `up_left` |
| `CENTER` | `neutral` |

- **アナログスティック**: 両軸とも0～255の範囲。
- **タッチスクリーン**: `{x: 0–319, y: 0–239}` 座標（0-based）。

## 8. [必須要件と機能要件] 型システム

OpenAPIからTypeScript型を生成する規則はWeb UIに適用します。GPUI nativeのHTTP clientは正準schemaから生成したRust型を使い、in-process adapterは明示的なRust型契約を使います。いずれのfrontendもwire schemaを手作業で独立定義しません。

### 8.1 OpenAPI → TypeScript

- **ソース**: `utoipa` v5マクロを使用したRustコア。
- **生成**: `openapi-typescript` CLI。
- **出力**: `src/lib/api/openapi.ts`。
- **使用法**: すべてのAPI呼び出しとWebSocketメッセージは生成された型を使用する必要があります。

**ワークフロー**:

```bash
# 1. Rustコアから追跡対象のOpenAPI JSONを生成し、そのJSONから型を生成
nix run .#generate-api-types
#    OpenAPI JSON: api/openapi.json
#    TypeScript: web/src/lib/api/openapi.ts

# 2. フロントエンドで型を使用
import { paths, components } from '$lib/api/openapi.ts'
```

> **注**: HTTPサーバーが起動していなくても、Rustのschema型から生成したローカルJSONファイルに対して実行するため、型生成は独立して動作する。これによりサーバーが起動していない状態でも型生成が可能であり、CIでも同じ追跡対象JSONとTypeScriptのdriftを検査する。

**自動化**: 正準入口は`nix run .#generate-api-types`です。生成済み型はgit追跡し、OpenAPI対象ソースの存在時は生成結果をCIで検査します。対象ソース存在判定と未実装時の明示的notice／skip条件は§7.4で定義し、本節では重複定義しません。

### 8.2 型安全性要件

- 厳格なTypeScript（`strict: true`）。
- API関連コードに `any` 型は不使用。
- 外部入力に対するZodまたは同等の実行時検証。

---

## 15. [必須要件と機能要件] デスクトップライフサイクル・閉じる動作

本章はTauriまたはGPUIを使うnative desktop modeのlifecycleと最後のnative windowを閉じる動作を規定します。Web modeにはnative windowがないため、`close_behavior`は適用しません。OS shutdown、SIGTERM／Ctrl+C、fatal errorではすべてのmodeが同じRust backend shutdown契約を使います。

### 15.1 プロセスモデル

プロセス構成は§1.3を参照。本章ではデスクトップモードの閉じる動作と終了ライフサイクルを規定する。

### 15.2 `close_behavior` 設定

デスクトップモードにおいて、ユーザーが最後のnative windowを閉じようとした際の動作を `close_behavior` で設定する。

| 設定値 | 動作 |
|--------|------|
| `"ask"`（デフォルト） | 確認ダイアログを表示し、ユーザーに三つの選択肢を提示する |
| `"shutdown"` | 確認なしで完全グレースフルシャットダウンを実行する |
| `"keep_backend"` | 確認なしでnative windowのみを閉じ、バックエンドを継続する |

**設定経路**:

1. **TOML**: `settings.toml` の `[ui.desktop]` セクション
   ```toml
   [ui.desktop]
   close_behavior = "ask"
   ```

2. **Python動的設定**:
   ```python
   pokecon.opt.ui.desktop.close_behavior = "ask"
   ```

3. **Lua動的設定**:
   ```lua
   pokecon.opt.ui.desktop.close_behavior = "ask"
   ```

4. **環境変数**:

   ```text
   POKECON_UI_DESKTOP_CLOSE_BEHAVIOR="ask"
   ```

5. **CLI引数**:

   ```text
   --ui-desktop-close-behavior ask
   ```

型: 文字列リテラル（Python: `Literal["ask", "shutdown", "keep_backend"]`、Lua: 同一文字列値）。

デフォルト: `"ask"`。

### 15.3 閉じる動作の詳細

最後のnative windowを閉じる操作は、以下の動作に従う。二つ以上のnative windowが開いている状態で一つを閉じる操作は、単にそのウィンドウを閉じるだけで、閉じる動作の対象外である。

#### 15.3.1 `"ask"` — 確認ダイアログ

確認ダイアログは次の三つのアクションを提示する:

| アクション | 説明 |
|-----------|------|
| **バックエンドを継続** | native windowのみを閉じる。axum、カメラ、シリアル、ワーカー、アクティブなコマンドは継続して動作する（§15.4参照） |
| **すべて終了** | 完全グレースフルシャットダウンを実行する（§15.6参照） |
| **キャンセル** | ウィンドウを閉じない。アプリケーションは通常状態を維持する |

- この確認ダイアログは設定値を変更/永続化しない。
- 「次回から表示しない」などのチェックボックスは設けない。

#### 15.3.2 `"shutdown"` — 完全シャットダウン

ユーザーが最後のnative windowを閉じると、確認なしで§15.6の完全グレースフルシャットダウンを実行する。

#### 15.3.3 `"keep_backend"` — バックエンド継続

ユーザーが最後のnative windowを閉じると、確認なしでnative windowのみを閉じる。バックエンド（axum、カメラ、シリアル、ワーカー、アクティブなコマンド）は継続して動作する（§15.4参照）。

### 15.4 バックエンド継続モードの動作

バックエンドが継続されている状態では、以下の機能が提供される。

#### 15.4.1 システムトレイ

バックエンド継続中はnative desktop shellが再表示可能なOS操作を提供します。既存のTauri UIではsystem trayを使用します。GPUI UIは同等の再表示・終了操作を受入します。

| メニュー | 動作 |
|---------|------|
| **開く** | native windowを再作成してフォーカスする。既にウィンドウが存在する場合はフォーカスを移動する |
| **終了** | §15.6の完全グレースフルシャットダウンを実行する |

#### 15.4.2 既存インスタンス検出

backend継続中に同じfrontend modeを再起動しようとした場合、既存backend instanceを検出し、第二backendを起動せず既存の同mode UIを再表示します。別modeへの切替は実行中instanceの暗黙移行を意味しません。

1. 第二のバックエンドプロセスを起動しない
2. 既存backend processに選択済みfrontendのwindow再作成／focusを要求する

### 15.5 確認ダイアログをバイパスするケース

以下のケースでは、`close_behavior` の値にかかわらず、確認ダイアログを表示せずに完全グレースフルシャットダウンを実行する:

- システムトレイの「終了」メニューによる終了
- SIGTERMまたはCtrl+Cの受信
- OSのログアウト/シャットダウン
- アプリケーション内の致命エラー

確認ダイアログを表示できない状態（例: 既にnative windowが閉じられている、ダイアログ表示に失敗した）の場合も、フェイルセーフとして完全グレースフルシャットダウンを実行する。

### 15.6 完全グレースフルシャットダウンの手順

完全グレースフルシャットダウンは以下の順序で実行される。各ステップで失敗を診断ログへ記録し、依存前提を満たす後続ステップだけを継続する。個別リソースの停止失敗によって終了処理全体を無期限に停止してはならない。ユーザースクリプトワーカーは設定済み`python.script.shutdown_timeout_ms`を使用し、それ以外のカメラwriter、動的設定ワーカー、シリアル、axumの各停止ステップは固定2,000msの内部期限を使用する。この内部期限は終了安全性の上限であり、新しいユーザー設定項目ではない:

0. **`AppShutdownPre`発火**: 動的設定ワーカーが存在する場合、カメラ・シリアル・状態API等のリソースを停止する前に`AppShutdownPre`を発火する。コールバックは終了前の保存・ログ・通知に利用できるが、`False`を含む戻り値で終了をキャンセルできない。コールバック例外は記録して残りのハンドラと終了処理を継続する。発火開始から固定2,000msの内部期限を適用し、期限時点で未開始ハンドラを開始せず、実行中callbackの論理完了を待たない。全ハンドラ完了または期限到達のどちらでも、ステップ1の直前に動的設定ワーカー世代を`stopping`へ遷移させ、以後のcontroller・設定変更・リソース操作IPCを切断エラーで拒否し、診断ログだけを許可する。その後ステップ1へ進み、残留callback自体はステップ4の動的設定ワーカー停止・必要時強制終了で終了させる
1. **コントローラー安全状態の強制**: 全ボタン・スティック・タッチ入力を即時に強制解放（ニュートラル/リリース安全状態）
2. **カメラキャプチャスレッド停止と出版停止**: 新規キャプチャをキャンセルし、進行中のカメラキャプチャ／writerスレッドを内部期限までjoinする。writer終了を確認できた場合、残留する`state=1`は今後誰も書き込まない非カレントの未完成スロットとしてERROR診断後に`state=0`へ戻し、全スロットに`state=1`がないことを確認してから`published_token`を[バックエンド定義書](SPECIFICATION_BACKEND.md) §7.9.3の無効値`UINT64_MAX`へリリースストアする。writer終了前に無効値をストアして後続の出版ストアで上書きされる順序を禁止する。内部期限までにwriter終了を確認できない場合はcritical診断と`camera_writer_unstopped`内部フラグを記録し、`published_token`、スロット状態、共有マッピングを変更せずステップ3へ進む
3. **ユーザースクリプトワーカーの協調停止**: 既存の協調停止＋タイムアウト/強制終了ポリシー（`pokecon.opt.python.script.shutdown_timeout_ms`）に従ってユーザースクリプトワーカーを停止する（[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.5.6.4.3、[バックエンド定義書](SPECIFICATION_BACKEND.md) §1.2のポイント9参照）。OSプロセス終了確認後、[バックエンド定義書](SPECIFICATION_BACKEND.md) §7.9.4の単一reader不変条件に従い全スロットの`reader_pin_count`を`0`へリセットする（デッドワーカーの残留ピン除去）
4. **動的設定ワーカーの停止**: グローバル動的ワーカーへ協調停止を要求し、内部期限までOSプロセス終了を待つ（`dynamic_config_language="none"`の場合は該当せず）。期限までに終了しない場合は、通常運用中の「終了・再生成しない」不変条件に対するアプリケーション終了時だけの例外として当該動的設定ワーカープロセスを強制終了し、終了確認後に進む。callbackのfinally／Lua後処理は強制終了経路では保証しない
5. **共有メモリ解放**: `camera_writer_unstopped`が偽で、ステップ2の出版停止と全ワーカーのOSプロセス終了を確認できた場合だけ、共有メモリ領域のマッピングを解除して名前付き共有メモリをunlinkする。`camera_writer_unstopped`が真の場合はwrite-after-unmapを避けるためRustメインのマッピングを解除しない。POSIXでは新規マップを防ぐため名前だけをunlinkして既存マッピングをプロセス終了まで保持し、Windowsでは新規ワーカーを生成せず既存マッピングハンドルをプロセス終了まで保持する。この退避経路では通常のunmap完了を主張しない
6. **入力強制解放**: 全入力状態を再度強制解放（安全状態確認）
7. **シリアル切断**: シリアル接続へ切断を要求して内部期限まで待つ。期限超過または失敗時は診断を記録して後続へ進み、OSプロセス終了によるハンドル回収へ委ねる
8. **axumのグレースフルシャットダウン**: HTTPサーバーへグレースフル停止を要求して内部期限まで待つ。期限超過時は残存HTTP／WebSocketタスクをキャンセルし、新規接続を受理せず後続へ進む
9. **プロセス終了**: Rustメインプロセスを終了する。`camera_writer_unstopped`が真、または安全に停止できない内部スレッドが残存する場合は、共有メモリをunmapせずログの有限なflushだけを試みた後、OSプロセス終了によって全スレッド・マッピング・ハンドルを回収する。停止不能スレッドのjoinやデストラクタを再度無期限に待ってはならない

ユーザースクリプトワーカー停止はプロファイル切替時（[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.5.6.4.3参照）と同じ設定値・ポリシーを使用する。その他の終了専用内部期限は本節の固定値だけを使用し、設定レジストリへ新たなタイムアウト項目を導入しない。

### 15.7 Webモードにおける動作

Webモードではnative windowが存在しないため、`close_behavior` の設定は効果を持たない。SIGTERM/Ctrl+C/OSシャットダウンによる終了は§15.5および§15.6に従い、完全グレースフルシャットダウンを実行する。

### 15.8 コンポジット無効化（`disable_compositing`）

現行Tauri/WebView modeには、window合成を無効化する設定 `ui.desktop.disable_compositing`（[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.4.2参照）を提供する。

**動作**: 本設定が `true` の場合、アプリケーション起動時にnative windowに対してコンポジット無効化を適用する。これにより、特に低遅延が要求されるシーンでウィンドウ合成に起因する入力遅延を低減できる。一部のプラットフォームではGPU合成をバイパスすることでパフォーマンスが向上する場合がある。

**適用タイミング**: 本設定はstartup-only（restart-required）である。Tauri/WebViewの初期化は起動時に一度だけ行われ、その初期化前に適用する必要があります。起動後にWebViewが初期化された状態でコンポジット設定を変更することは安全に行えないため、ランタイムでの動的適用は行わない。

**設定経路**:
1. **TOML**: `settings.toml` の `[ui.desktop]` セクション
   ```toml
   [ui.desktop]
   disable_compositing = true
   ```

2. **環境変数**:
   ```text
   POKECON_DISABLE_COMPOSITING=true
   ```

3. **CLI引数**:
   ```text
   --disable-compositing true
   ```

4. **OpenAPI**: 読み取り／書き込み対応。書き込みはグローバル `settings.toml` へ永続化されるが、次回起動時に反映される。レスポンスは `restart_required=true` を返し、現在の実効値は変更されないことを示す。

**スコープ**: グローバル専用。プロファイルTOMLに指定された場合、無視され既存のグローバル値が使用される（[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.3の診断ポリシーに従う）。

**優先順位**: 組み込みデフォルト（`false`） < グローバルTOML < 環境変数 < CLI。

**動的パス**: `pokecon.opt.ui.desktop.disable_compositing` は存在しない。動的設定からは設定できない。

**Webモード**: 本設定の値は保持されるが、WebモードではTauri/WebViewが存在しないため効果を持たない。

**UI表面**: Tauri/WebViewを使用するdesktop settings画面のcheckbox（フロントエンド定義書 [フロントエンド定義書](SPECIFICATION_FRONTEND.md) §6.6.1参照）。GPUI UIでは当該設定を表示しません。当該checkboxは `settings.toml` のグローバル設定として[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.4.1.5の永続化規則に従い保存される。変更後は再起動が必要である旨をUI上に表示する。

### 15.9 Web UIディレクトリ（`server.web_dir`）

SPA静的ファイル配信用ディレクトリの設定 `server.web_dir`（[フロントエンド定義書](SPECIFICATION_FRONTEND.md) §6.7.1、[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.4.2参照）を提供する。axum HTTPサーバーはこのディレクトリから `index.html`・JavaScript・CSS・画像等の静的ファイルを配信する。

**動作**: 本設定で指定されたディレクトリをaxumの静的ファイルルートとして使用する。デフォルト値はアプリケーションにbundleされた現行SvelteKit Web SPAのbuild出力`web/dist`（application resource root基準）です。本設定はWeb UI配信にだけ適用し、GPUI native UIには影響しません。ユーザーが独自のSPAビルドやカスタム静的ファイルを配置したディレクトリを指定することで、標準のWeb UIをカスタマイズまたは置き換えることができる。

**適用タイミング**: 本設定はstartup-only（restart-required）である。axum HTTPサーバーの静的ファイルルートはアプリケーション起動時に一度だけ決定され、ランタイム中にルートを安全に差し替えることはできない。起動後にファイルが追加・変更された場合、個別ファイルの更新は次回HTTPリクエストから反映される可能性があるが、ルートディレクトリ自体の差し替えは行わない。

**設定経路**:
1. **TOML**: `settings.toml` の `[server]` セクション
   ```toml
   [server]
   web_dir = "/path/to/custom/web"
   ```

2. **環境変数**:
   ```text
   POKECON_WEB_DIR=/path/to/custom/web
   ```

3. **CLI引数**:
   ```text
   --web-dir /path/to/custom/web
   ```

4. **OpenAPI**: 読み取り／書き込み対応。書き込みはグローバル `settings.toml` へ永続化されるが、次回起動時に反映される。レスポンスは `restart_required=true` を返し、現在の実効値は変更されないことを示す。

**スコープ**: グローバル専用。プロファイルTOMLに指定された場合、無視され既存のグローバル値が使用される（[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.3の診断ポリシーに従う）。

**優先順位**: 組み込みデフォルト（バンドル `web/dist`） < グローバルTOML < 環境変数 < CLI。

**動的パス**: `pokecon.opt.server.web_dir` は存在しない。動的設定からは設定できない。

**パス検証**:
- 型: `str`（ディレクトリパス）
- `path_policy`: `"directory"`
- `path_must_exist`: `true`（存在しないパスは起動時エラー）
- `path_auto_create`: `false`（自動生成は行わない）
- `path_expected_type`: `"directory"`（ファイルやシンボリックリンク先がファイルの場合はエラー）
- `path_resolve_symlink`: `true`（シンボリックリンクは解決後検証）
- 読み取り・走査権限が必要（権限不足は起動時エラー）
- 明示的な無効オーバーライドが指定された場合、組み込みデフォルトへのフォールバックは行わず、起動時エラーとする

**HTTP要求パスの閉じ込め**: 静的ファイルハンドラは、各要求パスをURLデコード後にOSネイティブ区切りへ正規化し、空バイト、絶対パス、`.`／`..`コンポーネント、Windowsドライブ／UNCプレフィックスを拒否する。候補を`server.web_dir`へ結合した後、既存パスのシンボリックリンクを解決した正準絶対パスが、正準化済み`server.web_dir`自身またはその子孫である場合だけ配信する。外部を指すシンボリックリンク、二重エンコードによる遡行、正規化後にルート外となる要求は`403 Forbidden`とし、ファイル内容・正準ホストパスを応答へ含めない。存在しない安全なパスだけを`404 Not Found`とする

**ユーザー指定相対パス解決**:
- **CLI `--web-dir` 相対パス**: 起動時のカレントワーキングディレクトリ（cwd）基準（[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.4.1.4の汎用パス解決規則におけるCLI例外）
- **TOML（グローバル） / 環境変数 相対パス**: 実効Configルート基準（[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.4.1.4の一般規則に従う）

**UI表面**: サーバー設定のディレクトリピッカー（[フロントエンド定義書](SPECIFICATION_FRONTEND.md) §6.7.1参照）。当該ディレクトリピッカーは `settings.toml` のグローバル設定として[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.4.1.5の永続化規則に従い保存される。変更後は再起動が必要である旨をUI上に表示し、現在のサーバー動作は変更されない。現在の値は参照表示として読み取り専用で表示する。

---

### 15.10 サーバーポート（`server.port`）

HTTPサーバーバインドポート番号の設定 `server.port`（[フロントエンド定義書](SPECIFICATION_FRONTEND.md) §6.7.2、[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.4.2参照）を提供する。axum HTTPサーバーはこのポート番号でTCPソケットにバインドし、HTTPリクエストを受け付ける。

**動作**: 本設定で指定されたポート番号を使用してaxum HTTPサーバーを起動する。デフォルト値は `8020` である。有効範囲は 1～65535 の整数。デスクトップモードとWebモードの両方で同一のポート番号を使用する。

**適用タイミング**: 本設定はstartup-only（restart-required）である。TCPソケットのバインドはアプリケーション起動時に一度だけ行われ、ランタイム中にポートを安全に切り替えることはできない。

**設定経路**:
1. **TOML**: `settings.toml` の `[server]` セクション
   ```toml
   [server]
   port = 8080
   ```

2. **環境変数**:
   ```text
   POKECON_PORT=8080
   ```

3. **CLI引数**:
   ```text
   --port 8080
   ```

4. **OpenAPI**: 読み取り／書き込み対応。書き込みはグローバル `settings.toml` へ永続化されるが、次回起動時に反映される。レスポンスは `restart_required=true` を返し、現在の実効値は変更されないことを示す。

**スコープ**: グローバル専用。プロファイルTOMLに指定された場合、無視され既存のグローバル値が使用される（[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.3の診断ポリシーに従う）。

**優先順位**: 組み込みデフォルト（`8020`） < グローバルTOML < 環境変数 < CLI。

**動的パス**: `pokecon.opt.server.port` は存在しない。動的設定からは設定できない。

**検証**:
- 型: `int`
- 有効範囲: 1～65535
- 検証は起動時のアドレスバインド前に行われる。範囲外の値は明示的な起動時エラーとし、アプリケーションは起動に失敗する

**バインド失敗セマンティクス**:
- 指定されたポートのバインドに失敗した場合（ポート使用中、権限不足等）、**ポート自動インクリメントやフォールバックポートへのフォールバックは行わない**
- 明示的な起動時エラーとして、バインド失敗の理由（アドレス・ポート番号・エラー種別）を含む診断メッセージを出力し、アプリケーションは起動に失敗する

**CORS導出**:
- 許可Host／Originは実効`server.bind_address`と本`server.port`の組み合わせから動的に導出する。固定`localhost:8020`は使用しない。§7.4、§15.11参照

**UI表面**: サーバー設定の数値入力（[フロントエンド定義書](SPECIFICATION_FRONTEND.md) §6.7.2参照）。当該数値入力は `settings.toml` のグローバル設定として[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.4.1.5の永続化規則に従い保存される。変更後は再起動が必要である旨をUI上に表示し、現在のサーバー動作は変更されない。現在の値は参照表示として読み取り専用で表示する。

---

### 15.11 バインドアドレス（`server.bind_address`）

HTTPサーバー待受IPの設定`server.bind_address`（[フロントエンド定義書](SPECIFICATION_FRONTEND.md) §6.7.3、[バックエンド定義書](SPECIFICATION_BACKEND.md) §11.4.2参照）を提供する。組み込みデフォルトはローカルホスト`127.0.0.1`であり、明示変更しない限りLANへ公開しない。

**適用タイミング**: startup-only（restart-required）。起動後の待受ソケットを差し替えず、設定変更は次回起動時に反映する。

**設定経路**:
1. **TOML**:
   ```toml
   [server]
   bind_address = "192.168.1.10"
   ```
2. **環境変数**: `POKECON_BIND_ADDRESS=192.168.1.10`
3. **CLI引数**: `--bind-address 192.168.1.10`
4. **OpenAPI**: 読み取り／書き込み対応。グローバル`settings.toml`へ保存し、`restart_required=true`を返す。現在の実効待受IPは変更しない

**スコープ／優先順位**: グローバル専用。組み込みデフォルト（`127.0.0.1`） < グローバルTOML < 環境変数 < CLI。プロファイルTOMLの値は無視・診断する。

**動的パス**: `pokecon.opt.server.bind_address`は存在しない。

**検証**:
- Rustの`IpAddr`相当で解釈できる数値IPv4／IPv6リテラルだけを受理する
- IPv4未指定`0.0.0.0`、IPv6未指定`::`、マルチキャスト、IPv4ブロードキャスト、ホストに未割当の非ループバックIPを拒否する
- 値を暗黙に別アドレスへ正規化・置換しない。IPv6のOrigin／Host表現で必要な角括弧はHTTP層で付加し、保存値には含めない
- バインド失敗時はローカルホストや別インターフェースへフォールバックせず、アドレス・実効ポート・OSエラー種別を含む起動時エラーとする

**LAN公開と信頼境界**: 非ループバック値は認証なしHTTP／WebSocketを当該LANインターフェースへ公開する明示的オプトインであり、そのLANへ接続できるクライアントを完全に信頼する。`POST /api/dynamic-config/control`の`load_content`／`load_path`／`reload`を接続元IP、loopback、Tauri、ブラウザ／非ブラウザで制限せず、他の状態変更APIと同じHost／Origin／Content-Type／固定ヘッダー検証だけで受理する。これらの操作はサーバーホスト上でPython／Luaコードを保存・評価し、任意コード実行権限に相当する。Host／Origin／固定ヘッダー検証はCSRF・DNS rebinding対策であってクライアント認証ではない。UIは非ループバック値の保存時と再起動要求表示時に、LAN接続者へこの権限を与えることを明示する。別の認証、許可Origin、peer-IP制限、Tauri専用バイパスは設けない。

**UI表面**: サーバー設定のIPアドレス入力（[フロントエンド定義書](SPECIFICATION_FRONTEND.md) §6.7.3）。保存後も現在の実効値と再起動後の保存値を区別して表示する。

---
