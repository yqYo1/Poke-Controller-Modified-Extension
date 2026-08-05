# PokeConインストールガイド

この文書は、PokeConの導入、初回起動、更新、削除、オフライン配置を担当する利用者を対象にします。

日常の操作は[利用ガイド](USER_GUIDE.md)で説明します。

本体開発用の環境構築は[本体開発ガイド](DEVELOPMENT.md)で説明します。

## 対応する配布環境を確認する

現行の配布対象はLinux x86-64とWindows x86-64です。

Linuxの`.deb`はUbuntu 24.04 x86-64を検証基準とし、成果物が要求するglibcの上限を2.39とします。

LinuxとWindowsの標準成果物はWeb UIとTauriデスクトップの両方を含み、起動時に表示形態を選択できます。

macOSとPWAは現行リリースの対象外です。

配布物にはPython 3.14、managed uv、固定lockから作るoffline wheelhouse、Web UI、user-script workerを同じ配置規約で収録します。

通常利用時にsystem Python、Bun、Node.jsを別途導入する必要はありません。

## 配布物の完全性を確認する

GitHub Releaseからinstallerまたはpackageと、同じreleaseの`SHA256SUMS`を取得します。

導入前に対象fileのSHA-256が`SHA256SUMS`と一致することを確認します。

checksumが一致しない成果物は実行しません。

upgradeでも新しい成果物のchecksumを再確認します。

## Linuxへ導入する

Nixが利用できる環境では、flakeをprofileへ導入できます。

```bash
nix profile install github:yqYo1/Poke-Controller-Modified-Extension
pokecon --ui web
```

checkoutをそのまま検証する場合は、同じcheckoutからbuildしたbinaryを起動します。

```bash
nix build .#pokecon-server
./result/bin/pokecon --ui web
```

GitHub Releaseの`.deb`は、distributionのpackage管理機能で導入します。

Desktop modeにはWebKitGTK 4.1系が必要であり、`.deb`の依存関係はpackage managerが解決します。

`.deb`は`/usr/lib/udev/rules.d/70-pokecon-controller.rules`を収録します。

このruleはactive local sessionへ一般的なVideo4Linux、`ttyACM*`、`ttyUSB*`のaccessを付与します。

installとremoveではudev ruleを再読込します。

headless session、独自device node、独自symlinkでは、`video`または`dialout` groupや管理者定義ruleが別途必要です。

Nix closure exportを使用する場合は、Nixを導入済みの端末へclosureをimportし、含まれるstore pathをprofileへ登録します。

closureを別端末へ移す前後でchecksumを照合します。

## Windowsへ導入する

GitHub ReleaseのNSIS `setup.exe`を取得し、同じreleaseの`SHA256SUMS`と照合します。

`setup.exe`を実行し、installerが示すscopeへ導入します。

installerにはoffline WebView2 installer、Python 3.14、uv、worker、Web UIを収録します。

シリアル機器がvendor driverを要求する場合は、機器メーカーの手順でdriverを先に導入します。

cameraを初めて使う場合は、Windowsのprivacy設定でdesktop appのcamera accessを許可します。

Windows notificationを使用する場合は、OS側でPokeConのnotificationを許可します。

## 初回起動で作る保存場所

初回起動は不足しているdirectoryとfileだけを作成します。

既存のユーザー編集fileは上書きしません。

既定のapp名は`pokecon`です。

既定のprofile名は`default`です。

LinuxではXDG base directoryを使用します。

| root | XDG変数を設定した場合 | 未設定時 |
|---|---|---|
| Config | `$XDG_CONFIG_HOME/pokecon` | `~/.config/pokecon` |
| Data | `$XDG_DATA_HOME/pokecon` | `~/.local/share/pokecon` |
| Cache | `$XDG_CACHE_HOME/pokecon` | `~/.cache/pokecon` |
| State | `$XDG_STATE_HOME/pokecon` | `~/.local/state/pokecon` |

相対pathまたは空のXDG変数はbase directoryとして採用せず、表のfallbackを使用します。

WindowsではAppDataのknown folderを使用します。

| root | path |
|---|---|
| Config | `%APPDATA%\pokecon` |
| Data | `%LOCALAPPDATA%\pokecon\data` |
| Cache | `%LOCALAPPDATA%\pokecon\cache` |
| State | `%LOCALAPPDATA%\pokecon\state` |

主要なfileとdirectoryは次の位置へ作成します。

```text
Config/
├── settings.toml
├── init.py
├── init.lua
├── pyproject.toml
├── .luarc.json
└── profiles/
    └── default/
        └── settings.toml

Data/
├── Commands/
├── Captures/
├── typings/
├── lua-typings/
├── venv-script/
└── venv-dynamic/
```

`settings.toml`、`init.py`、`init.lua`、`pyproject.toml`、`.luarc.json`はユーザー編集fileとして初回だけ作成します。

`typings`と`lua-typings`は公開契約から生成し、起動時に更新できます。

`app_name`を変更すると4個のrootすべてで別のapp directoryを使用するため、既存dataのrename操作にはなりません。

## Web modeを起動する

LinuxまたはWindowsでWeb UIを使用する場合は、terminalから次のように起動します。

```bash
pokecon --ui web
```

既定では`127.0.0.1:8020`で待ち受けます。

browserで`http://127.0.0.1:8020/ui/`を開きます。

待受addressやportを変更する場合は、先に[LAN公開の信頼境界](ADVANCED_USAGE.md#lan公開の信頼境界を確認する)を読んでください。

起動引数の現在の一覧は`pokecon --help`で確認します。

## Desktop modeを起動する

導入済みのapplication launcherからPokeConを起動します。

Nix checkoutからLinux desktopを起動する場合は次を実行します。

```bash
nix run .#tauri
```

Desktop modeでもbackend、REST、WebSocket、WebRTC、設定transactionはWeb modeと同じです。

最終windowを閉じたときの動作は`ask`、`shutdown`、`keep_backend`から選択できます。

`keep_backend`ではwindowを閉じてもtrayとbackendが残ります。

完全終了にはtrayのQuitを使用します。

## 初回起動後に基本機能を確認する

初回起動後は次の順に確認します。

1. 画面上部のbackend接続状態が`connected`になることを確認します。
2. Menuのprofileが`default`になっていることを確認します。
3. Camera tabでcameraを列挙し、必要な機器を選択します。
4. Serial tabでport、baud rate、data formatを選択します。
5. Connect後に`Connected`になることを確認します。
6. Manual Controlのbuttonを短く操作し、周辺機器側で一致を確認します。
7. Release all inputとDisconnectを実行してから終了します。

起動だけを自動確認する場合は`--exit-after-startup`を使用できますが、実機操作の確認にはなりません。

## 同じscopeでupgradeする

upgrade前にConfig rootとData rootをバックアップします。

実行中commandを停止し、serialを切断してからPokeConを終了します。

新しい成果物の`SHA256SUMS`を検証します。

Windowsでは新しいinstallerを以前と同じscopeへ実行します。

LinuxではNix profileまたは`.deb` packageを同じ導入方式で更新します。

起動後にprofile、command一覧、camera selector、serial selector、notification設定を確認します。

managed venvと生成typingsはbuild identityに基づいて再検証します。

ユーザー編集可能なTOML、`init.py`、`init.lua`、command sourceは保持します。

旧versionへ戻す場合も、先に現行のConfig rootとData rootをバックアップします。

設定形式や互換性の差がある場合は[移行ガイド](MIGRATION.md)を確認します。

## applicationを削除する

WindowsのuninstallerまたはLinuxのpackage managerはapplication本体を削除します。

Config、Data、Cache、Stateはprofileとユーザースクリプトを保護するため自動削除しません。

再導入する可能性がある場合はConfigとDataを残します。

完全削除する場合だけ、対象app名と4個のrootを確認してから個別に削除します。

rootの一括削除前に、command source、template、capture、秘密値を含む設定が不要であることを確認します。

## networkなしで導入する

Windows installerとLinux `.deb`は、既定worker依存を固定lockどおり同期できるPython runtime、uv、wheelhouseを収録します。

OS側の依存libraryが揃っていれば、既定workerの初回起動はnetworkへ接続しません。

Linuxの完全オフライン導入では、online端末で対象releaseのNix closure export、または`.deb`とOS依存packageを準備します。

オフライン端末へ移す前後の両方で`SHA256SUMS`を照合します。

ユーザースクリプトへ追加packageを指定した場合は、そのpackageのwheelまたは事前に埋めたuv cacheも別途用意します。

VCS URLやlocal pathを追加packageとして使う場合は、参照先の可用性とidentityをoffline環境でも満たす必要があります。

## 配布物を作る開発者が検証する

この節はrelease担当者向けであり、一般利用者の導入には不要です。

Linuxの`.deb`を作成して静的検査とclean install検査を実行する場合は次を使います。

```bash
nix run .#tauri-build -- --bundles deb
nix run .#package-smoke -- dist/tauri/*.deb
nix run .#package-install-smoke -- dist/tauri/*.deb
```

静的検査はresource manifest、SPA asset、ELF interpreter、RPATH、glibc、udev rule、CPython、uv、wheelを監査します。

install検査は固定digestのUbuntu 24.04 containerで、非root利用者による初回起動、upgrade、再起動、uninstall、ユーザーデータ保持を確認します。

実機まで含むrelease判定は[外部受入ゲート](ACCEPTANCE.md)で行います。
