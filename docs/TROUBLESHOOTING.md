# トラブルシュート

## 起動しない

`pokecon --version`と`pokecon --help`が動くか確認します。Linux packageではWebKitGTK、PortAudio、udev、camera関連libraryが解決されている必要があります。Windowsではinstallerを再実行し、WebView2 runtimeと同梱resourceを修復します。

`packaged application is missing`というerrorは、`pokecon`、`pokecon-worker`、`web/dist`、`uv`、`python`の配置が壊れていることを示します。実行fileだけをinstall directoryから移動せず、installerまたはNix packageを再導入してください。

## UIが404になる

HTTP statusが200でも画面に`404 Not Found`と表示される場合は、Web buildがfallback pageだけになっています。開発checkoutでは次を実行します。

```bash
nix build .#web
nix build .#pokecon-server
```

`result/web/dist`に複数のJavaScript／CSS chunkと`index.html`があることを確認します。新しいfrontend拡張子を追加した場合は`flake.nix`のsource filterにも追加します。

## Python workerを開始できない

managed uvとCPythonは起動時にresource manifestのSHA-256を検証し、Dataのversion別directoryへcopyします。integrity errorが出た場合はpackageを再取得し、releaseの`SHA256SUMS`を確認します。user指定venvもexact-sync対象であり、解決済み閉包にないpackageは削除されます。専用venvを指定し、既存環境を共用しないでください。

既定worker依存の同期は同梱wheelhouseだけを使い、networkへ接続しません。追加packageの同期失敗ではproxy、certificate、明示したuv config、offline cacheを確認します。秘密値は`POKECON_UV_`接頭辞から必要な`UV_`変数へ限定的にbridgeされます。diagnosticへtokenそのものを貼り付けないでください。

## スクリプトを読み込めない

スクリプトはDataの`Commands`以下へ置き、`PythonCommands`／`McuCommands`の相対構造を保ちます。Python 3.14でparseできない構文、未収録のthird-party package、未対応Tk propertyは明示的errorになります。

固定コーパスとの差を確認するには次を実行します。

```bash
nix run .#compatibility
```

互換性ゲートはsource hash、import評価、command class検出をまとめて確認します。実行結果がdriftした場合、tracked sourceや生成結果を手で直さず原因を調査してください。

## シリアルへ接続できない

Linux `.deb`では`70-pokecon-controller.rules`が導入され、active local sessionへ一般的な`ttyACM*`／`ttyUSB*` accessを付与します。反映されない場合はdeviceを挿し直し、`udevadm info`、session種別、device nodeのgroupを確認します。headless環境や独自udev symlinkでは`dialout` groupまたは管理者ruleが必要です。WindowsではCOM portとvendor driverを確認します。他のアプリケーションがportを占有していないことも確認してください。loopbackでprotocolを検証してから実機を接続します。

切断時に入力が残る場合は安全性に関わる不具合です。再接続を繰り返さず、対象generation、route、serial modeを記録して停止してください。

## カメラが映らない

OSのprivacy設定とdevice permissionを確認し、別アプリがcameraを占有していない状態にします。Linux `.deb`のudev ruleはactive local sessionへ`video4linux` accessを付与しますが、headless環境では`video` groupまたは管理者ruleが必要です。resolutionやflip設定を変更した後は、状態revisionが更新されているかUIで確認します。WebRTCに失敗した場合はWebSocket fallbackへ移り、条件が戻ると自動復旧します。

実機検証では既知frameを用意し、capture resolution、色順、crop座標、template resultを記録します。

## デスクトップwindowが閉じない

`ui.desktop.close_behavior`は`ask`、`shutdown`、`keep_backend`のいずれかです。`keep_backend`ではwindowを閉じてもtrayとbackendが残ります。完全終了はtrayのQuitを使います。

Linuxで描画が不安定な場合は`ui.desktop.disable_compositing = true`を設定して再起動します。Windowsでは同じ設定がWebViewのGPU compositingを無効化します。

## 外部通知やMQTTが失敗する

socket、MQTT、Discordはbounded timeoutとcancelを持つRust proxy経由です。broker address、port、room ID、client ID、webhook URLを確認します。legacy helperは一部の外部失敗をfail-softで扱うため、UI diagnosticと送信先側のlogも確認してください。

token、password、webhook URL、受信payloadをissueへそのまま掲載しないでください。再現時はsecretを置換し、操作種別、timeout、HTTP status、diagnostic IDだけを共有します。
