# インストールガイド

## 対応環境

現行の配布対象はWindows x86-64とLinux x86-64です。Linux `.deb` の検証基準はUbuntu 24.04 x86-64で、成果物が要求するglibcの上限は2.39です。Python 3.14、managed uv、固定lockから作ったoffline wheelhouse、Web UI、user-script workerはアプリケーション成果物へ同じ配置規約で収録します。macOSとPWAは対象外です。

## Linux

Nix環境では、署名済みrelease tagまたはcheckoutからインストールできます。

```bash
nix profile install github:yqYo1/Poke-Controller-Modified-Extension
pokecon --ui web
```

checkoutから検証する場合は次を実行します。

```bash
nix build .#pokecon-server
./result/bin/pokecon --ui web
```

GitHub Releaseでは`.deb`とNix closure exportを公開します。`.deb`はディストリビューションのパッケージ管理機能で導入してください。Nix closure exportはNixが導入済みのオフライン端末でimportした後、含まれるstore pathをprofileへ登録します。成果物と同じreleaseにある`SHA256SUMS`を先に照合してください。

`.deb`はactive local sessionへ`video4linux`、`ttyACM*`、`ttyUSB*`のアクセスを付与する`/usr/lib/udev/rules.d/70-pokecon-controller.rules`を収録し、install／remove時にudev ruleを再読込します。headless sessionや独自device nodeでは`video`／`dialout` groupまたは管理者定義ruleが別途必要です。デスクトップモードにはWebKitGTK 4.1系が必要で、`.deb`の依存関係はパッケージマネージャーが解決します。

checkoutから実際の配布物を検証するコマンドは次のとおりです。

```bash
nix run .#tauri-build -- --bundles deb
nix run .#package-smoke -- dist/tauri/*.deb
nix run .#package-install-smoke -- dist/tauri/*.deb
```

静的検査はresource manifest、SPA全asset、ELF interpreter／RPATH／glibc、udev rule、CPython、uv、全wheelを監査します。install検査は固定digestのUbuntu 24.04 containerで、非rootユーザーによる初回起動、upgrade、再起動、uninstall、ユーザーデータ保持を実行します。

## Windows

GitHub ReleaseのNSIS `setup.exe`と`SHA256SUMS`を同じディレクトリへ保存し、hashを照合してから実行します。インストーラにはoffline WebView2 installer、Python 3.14、uv、worker、Web UIを収録します。通常の実行時に開発用PythonやNode.jsは不要です。

シリアル機器のvendor driverが必要な場合は、機器メーカーの手順で先に導入します。カメラを初めて使うときはWindowsのprivacy設定でdesktop appのcamera accessを許可してください。

## 初回起動と保存場所

初回起動は欠けているファイルだけを作成し、既存ファイルを上書きしません。既定のアプリ名は`pokecon`、既定profileは`default`です。

Linuxの保存場所は次のとおりです。

- Config: `$XDG_CONFIG_HOME/pokecon`、未設定時は`~/.config/pokecon`
- Data: `$XDG_DATA_HOME/pokecon`、未設定時は`~/.local/share/pokecon`
- Cache: `$XDG_CACHE_HOME/pokecon`、未設定時は`~/.cache/pokecon`
- State: `$XDG_STATE_HOME/pokecon`、未設定時は`~/.local/state/pokecon`

WindowsではConfigが`%APPDATA%\pokecon`、Dataが`%LOCALAPPDATA%\pokecon\data`、Cacheが`%LOCALAPPDATA%\pokecon\cache`、Stateが`%LOCALAPPDATA%\pokecon\state`です。global設定はConfigの`settings.toml`、profile設定は`profiles/<name>/settings.toml`、スクリプトはDataの`Commands`へ置きます。

## アップグレード

1. ConfigとDataをバックアップします。
2. 実行中のスクリプトを停止し、アプリケーションを終了します。
3. 新しい成果物の`SHA256SUMS`を検証します。
4. Windowsは新しいinstallerを同じscopeへ実行します。Linuxはprofileまたはpackageを更新します。
5. 起動後にprofile、command一覧、camera／serial設定を確認します。

managed venvと生成typingsはData側でbuild identityに基づいて再検証します。ユーザー編集可能なTOML、`init.py`、`init.lua`、command sourceは保持します。旧versionへ戻す場合も、先にConfigとDataのbackupを取ってください。

## アンインストール

アンインストーラまたはpackage managerはアプリケーション本体を削除します。Config、Data、Cache、Stateはprofileとユーザースクリプトを守るため自動削除しません。完全削除が必要な場合だけ、保存場所を確認してから個別に削除してください。

## オフライン導入

Windows installerとLinux `.deb`は、既定worker依存を固定lockどおり同期できるPython runtime、uv、wheelhouseを収録します。そのため、OS依存libraryの導入後は初回worker起動もnetworkなしで完了します。Linuxの完全オフライン導入では、online端末で対象releaseのNix closure export、または`.deb`とそのOS依存packageを準備します。オフライン端末へ移す前後の両方で`SHA256SUMS`を照合します。user-scriptへ追加packageを指定した場合、そのwheelまたはuv cacheも別途用意する必要があります。
