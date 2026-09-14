# PokeCon利用ガイド

この文書は、設定ファイルや内部構造を理解せずにPokeConを日常利用する人を対象にします。

導入が済んでいない場合は、先に[インストールガイド](INSTALL.md)を読んでください。

TOML、LAN公開、動的設定、Python環境を変更する作業は[上級利用ガイド](ADVANCED_USAGE.md)で説明します。

## 起動前に機器と対象プロファイルを確認する

カメラとシリアル機器を接続してからPokeConを起動します。

Linuxで機器を初めて接続する場合は、ログイン中のsessionがcameraとserial deviceへアクセスできることを確認します。

Windowsでcameraを初めて使用する場合は、OSのprivacy設定でdesktop appのcamera accessを許可します。

Web UIを起動した場合は、表示された待受URLを同じ端末のbrowserで開きます。

既定値のまま起動した場合のURLは`http://127.0.0.1:8020/ui/`です。

画面上部の接続表示が`connected`になり、`PID`が表示されればbackendとの接続は成立しています。

`Media`は映像経路を示し、通常はWebRTCを使用し、利用できない場合はMotion JPEGへ切り替わります。

## 画面の役割を把握する

主画面には次の6個のtabがあります。

| tab | 通常の用途 |
|---|---|
| Camera | 映像確認、撮影、camera選択 |
| Serial | port、baud rate、data formatの選択と受信確認 |
| Manual Control | software controllerと入力解除 |
| Commands | Python commandとMCU commandの実行 |
| Notifications | Windows通知とDiscord通知の設定と試験 |
| Other | 表示、終了動作、networkに関する設定 |

右側にはsoftware controllerと2個の出力欄があります。

狭い画面では右側の領域が主画面の下へ移動します。

tabは左右矢印、`Home`、`End`でも移動できます。

## プロファイルを切り替える

**プロファイル**は、command表示、shortcut、画面表示などを用途ごとに切り替える設定単位です。

画面上部の「メニュー」を開き、「プロファイル」で切替先を選びます。

「切替」を押すと、実行中のcommandを停止してから新しいプロファイルを準備します。

「切替先」が表示されている間は別の切替操作を重ねません。

切替後は、Camera、Serial、Commandsの状態を確認します。

Windowsでは同じmenuから、特定プロファイルを指定する`.bat` launcherを作成できます。

新規プロファイルを作る場合は、現在の内容をcopyするかをlauncher作成前に選択します。

## カメラを選択して映像を確認する

Camera tabを開き、「Camera device」から使用する機器を選びます。

接続後に機器が見つからない場合は「Refresh」を押します。

設定済みの機器が見つからないときは、PokeConは別のcameraを暗黙に選択しません。

「Camera open」と表示され、映像右下に解像度が表示されれば取得できています。

「Resolution」は取得解像度を変更し、「Capture FPS」はcameraから取得する最大頻度を変更します。

「UI FPS」は画面描画の頻度だけを変更します。

「Flip」は上下、左右、または両方向の反転を指定します。

「Live」を解除すると画面表示を止めますが、camera resourceの解放操作ではありません。

「Pixels」を有効にして`Ctrl`を押しながら映像をclickすると、対象座標のRGB値とHSV値を確認できます。

「Guide」は映像上へ3分割guideを表示します。

映像が止まった場合は「Retry camera」を押します。

映像経路だけがfallbackになった場合は「Retry WebRTC」を押します。

## 画面全体または範囲を保存する

「Screenshot format」でPNGまたはJPEGを選択します。

「Save to Captures」はData rootの`Captures`へ画面全体を保存します。

Desktop版の「Save as…」は保存先を選択し、Web版の「Download」はbrowserのdownloadとして保存します。

範囲だけをData rootへ保存する場合は、`Ctrl`と`Shift`を押しながら映像上をdragします。

範囲だけを任意の保存先へ出力する場合は、`Ctrl`と`Alt`を押しながらdragします。

保存完了時は画面上部に保存先またはfile名が表示されます。

## シリアル機器へ接続する

Serial tabを開き、「Refresh」で現在のport一覧を更新します。

「Port selector」で対象機器を選ぶか、Linuxのdevice pathまたはWindowsのCOM名を入力します。

「Baud rate」を周辺機器のfirmwareと一致させます。

「Data format」を周辺機器が実装している`Default`、`Qingpi`、`3DS Controller`のいずれかへ合わせます。

UIで`3DS Controller`を選択すると、baud rateも便宜上115200へ変更されます。

別の設定surfaceでは3DS形式を選んでもbaud rateを自動変更しないため、UI以外で設定した場合は両方を確認します。

「Connect」を押し、状態が`Connected`へ変わることを確認します。

接続成立時には、操作中の入力ではなくneutral frameが最初に送信されます。

周辺機器から受信したbyte列は「Raw receive data」にchunk単位で表示されます。

「Auto」は末尾への自動scrollを切り替え、「Clear」は表示だけを消去します。

接続を終える場合は「Disconnect」を押します。

抜線やI/O errorでは同じportへ自動再接続しますが、明示的な「Disconnect」は再試行を取り消します。

portを変更して失敗した場合は、別の機器を選ばず、変更前と同じ設定への復帰を試みます。

接続できない場合は[serialへ接続できない問題](TROUBLESHOOTING.md#serialへ接続できない問題を診断する)を参照してください。

## software controllerを操作する

右側の「Software Controller」またはManual Control tab内のcontrollerを使用します。

buttonはpointerを押している間だけ押下状態になります。

左右stickはpointerまたは矢印keyで動かし、操作を終えると中央へ戻ります。

中央のtouch areaは320×240の座標として送信します。

camera映像上で左stickを操作する場合は、Manual Controlの「L-stick mouse」を有効にして左dragします。

camera映像上で右stickを操作する場合は、「R-stick mouse」を有効にして右dragします。

`Ctrl`を押しながらcamera映像を右dragすると、script用touchscreen areaを更新します。

browserのfocusが外れた場合やtabが非表示になった場合は、保持中のbrowser入力をneutralへ戻します。

入力が残ったと感じた場合は、Manual Controlの「Release all input」を直ちに押します。

周辺機器側で押下が残る場合は接続を外し、[切断後に入力が残る問題](TROUBLESHOOTING.md#切断後に入力が残る問題を扱う)として記録します。

## Python commandを実行する

Commands tabを開き、「Python Command」を選びます。

tagで絞り込むか検索欄へcommand名、module path、class名を入力します。

一覧からcommandを選び、「Start」を押します。

実行中は状態が`running`になり、commandの標準出力や専用出力が右側へ表示されます。

一時停止に対応するcommandは「Pause」で止め、「Resume」で再開します。

終了する場合は「Stop」を押します。

sourceを追加または変更した後は「Reload」で一覧を再検出します。

commandがdialogを開いた場合は入力後に「OK」を押します。

dialogのclose buttonはdialogだけでなく実行中commandの停止も要求します。

読み込めない場合は[commandが一覧へ出ない問題](TROUBLESHOOTING.md#commandが一覧へ出ない問題を診断する)を参照してください。

## MCU commandを実行する

Commands tabの「Mcu Command」は、Python側の`McuCommand`として検出された項目を表示します。

対象項目を選び、「Start」と「Stop」をPython commandと同じ方法で操作します。

ここでいうMCU commandはユーザースクリプトの互換APIであり、Serial tabのdata formatを自動判定しません。

firmwareとscriptが期待するport、baud rate、wire形式を事前に一致させます。

## shortcutへcommandを割り当てる

Commands tabの「Shortcut」を開きます。

番号付きslotをclickし、表示されたcommand一覧から割当先を選びます。

割当済みslotの「Run」でcommandを開始できます。

slotを右clickするか、`Shift`を押しながらclickすると割当を解除します。

shortcutは現在のプロファイルへ保存されます。

## 通知を設定して試験する

Notifications tabではWindows通知とDiscord通知を別々に設定します。

Windows通知はWindowsでだけ配信され、他のOSでは設定値だけを保持します。

「Test Windows」でnative notificationが届くことを確認します。

DiscordではWebhook URLを入力し、必要に応じてusernameとavatar URLを指定します。

保存済みWebhook URLは固定maskで表示され、browserへ平文を戻しません。

「Test Discord」で送信を確認してから、script開始時と終了時の通知を有効にします。

Webhook URLやtokenをscreen capture、issue、logへ掲載しません。

## 出力と画面配置を調整する

Other tabの「出力と表示」では、2個の出力欄の比率、標準出力先、表示するwidget、言語、UI FPSを変更します。

「両方の出力をクリア」は表示だけを消去し、実行中commandを停止しません。

「レイアウト」ではcontrollerとdialog buttonの表示位置を選択します。

Desktop版の「最終ウィンドウを閉じる動作」は、毎回確認、全終了、backend継続から選択します。

`keep_backend`を選んだ場合はwindowを閉じてもtrayとbackendが残るため、完全終了にはtrayのQuitを使用します。

「サーバーとネットワーク」は上級設定であり、意味を理解していない場合は既定値を変更しません。

## 安全に終了する

実行中commandがある場合は先に「Stop」を押します。

Serial tabで「Disconnect」を押し、状態が`Disconnected`になることを確認します。

Desktop版は選択したclose behaviorに従ってwindowを閉じるか、trayのQuitを使用します。

Web版は起動したterminalで通常の終了signalを送り、processの終了を待ちます。

正常な終了処理では、入力をneutralへ戻し、cameraとworkerを停止し、最後にserialをneutral化して閉じます。

強制終了や電源断では周辺機器へ最後のneutral frameを届けられない場合があるため、通常の終了手順を優先します。

## 日常的にバックアップする

少なくともConfig rootとData rootをバックアップします。

Config rootにはglobal設定、profile設定、`init.py`、`init.lua`が含まれます。

Data rootにはcommand source、template、生成typings、venv、captureが含まれます。

生成typingsとvenvは再生成できますが、command sourceとtemplateは再生成できません。

OSごとのpathは[初回起動で作る保存場所](INSTALL.md#初回起動で作る保存場所)を参照してください。

## 問題を報告する前に情報を分ける

最初に[トラブルシュート](TROUBLESHOOTING.md)で、camera、serial、worker、networkのどこで失敗しているかを切り分けます。

問題報告にはPokeCon version、OS、実行mode、操作手順、表示された診断IDを含めます。

機器selector、ユーザー名を含むpath、Webhook URL、token、受信payloadは秘匿化します。

入力が解除されない問題では、安全確保を優先し、再現を繰り返す前に対象formatと停止操作を記録します。
