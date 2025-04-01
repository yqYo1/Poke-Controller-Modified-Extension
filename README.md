# Poke-Controller Modified Extension

[moi_poke](https://github.com/Moi-poke)氏が開発した[Poke-Controller Modified](https://github.com/Moi-poke/Poke-Controller-Modified)をベースに機能を追加したゲーム機自動化支援ソフトウェアです。

[Poke-Controller Modified](https://github.com/Moi-poke/Poke-Controller-Modified)からUIを一新、並列起動や対応ゲーム機の種類を増やしています。(ver. 0.1.6時点ではSwitch/3DS/DS/GCに対応。)

また、Modified版に対する後方互換性保持をMUSTとして開発をしています。
2025/3/31時点では、[Poke-Controller Modified](https://github.com/Moi-poke/Poke-Controller-Modified)で動作する自動化のスクリプト(後述)はすべて動作する(はず)です。

![カイリューかわいい](https://github.com/futo030/Poke-Controller-Modified-Extension/blob/image/pokecon_modified_extension_image_20250402.png)


## 更新履歴

[Github - 更新履歴](https://github.com/futo030/Poke-Controller-Modified-Extension/blob/master/changelog.txt)


## Poke-Controller とは?

Poke-Controllerの概要は[KawaSwitch](https://github.com/KawaSwitch)氏が開発した[Poke-Controller](https://github.com/KawaSwitch/Poke-Controller)および[moi_poke](https://github.com/Moi-poke)氏が開発した[Poke-Controller Modified](https://github.com/Moi-poke/Poke-Controller-Modified)を参照してください。


## Poke-Controller Modifiedとの差分について

[Poke-Controller Modified](https://github.com/Moi-poke/Poke-Controller-Modified)に対し、以下の機能追加および仕様変更を実施しています。
- 使用時における便利機能の追加
  - 2つ目のログ画面を追加(常時表示しておきたい情報の表示を想定)
  - 更新確認機能を追加
  - シリアルデバイスをコンボボックスで設定する機能を追加
  - スクリプトのFilter機能を追加
  - スクリプトのショートカット割り当て機能を追加
  - スクリプトの一時停止機能を追加
  - ログ画面クリア機能を追加
  - ログ画面の上書き用関数を追加
  - メインウィンドウに埋め込まれたソフトウェアコントローラーを追加(表示ON/OFF可能)
  - スクリプト一時停止機能を追加
  - ToolTip表示機能を追加
- スクリプト開発に役立つ機能の追加
  - 画像認識時の類似度を自動化スクリプトの記載によらず出力できる機能を追加(ON/OFF可能)
  - 画像認識時の探索範囲および認識結果をGUI上に表示する機能を追加(ON/OFF可能)
  - 画面キャプチャ機能を拡張
    - キャプチャしている画面部分を'Ctrl+Alt+左クリック'しながらドラッグした範囲をキャプチャすることが可能\
      このとき、名前をつけて保存のダイアログボックスが出て、任意の名前をつけることが可能
- スクリプト開発者への問い合わせ時に必要な情報の表示する機能の追加
  - ライブラリのversion情報の表示機能の追加
  - 問い合わせ用テンプレート文章作成アシスタント機能の追加
- 並列起動への対応
  - ProfileによるPokeCon側の設定複数保持機能を追加
  - 同一名称のキャプチャデバイスが接続された場合に対応できるよう仕様を変更
- 3DS自動化基板への対応([Qingpi](https://qiita.com/u1f992/items/09617ae326288a0df703)/3DS Controller)
  - 送信するシリアルデータのフォーマットを3種(Poke-Controller向け/Qingpi向け/3DS Controller向け)から選択する機能を追加
  - PokeConの画面上でタッチスクリーンを操作する機能を追加([Qingpi](https://qiita.com/u1f992/items/09617ae326288a0df703)のみ)
- ゲームパッドによる操作への対応
  - Pro-Controllerによる操作機能を追加(ver.0.1.5時点ではPro-Controllerのみ対応。)
  - 操作時のログを取得する機能を追加(再生可能)
- MQTTおよびSocket通信関連への対応
  - 関連する関数を追加
- 画像認識関連の関数の拡張
  - 画像の2値化に対応
  - 引数および関数を追加
- ダイアログ関数の拡張
  - ウィジェットの複数列表示(改行)機能を追加
  - 前回の入力の保持や呼び出しが可能なダイアログ関数を追加
- 通知機能の拡充
  - WindowsのNotificationによる通知機能を追加
  - ~~Discord Webhookを用いた通知機能を追加~~(同様の機能が[Poke-Controller Modified](https://github.com/Moi-poke/Poke-Controller-Modified)に実装済み)
  - プログラムの開始および終了時に通知する機能を追加(ON/OFF可能)
- UI刷新(設定画面のタブ化およびログ画面を2画面化)
  - ログ画面の2画面化に伴う機能の追加(サイズや標準出力先など)
  - レイアウトカスタマイズ機能の追加


## 推奨環境

- OS
  - Windows10/11
  - (一応Mac/Linuxでも動作するはずですが未確認です。issueおよびPRには対応します。)
- Python
  - 3.12以上(従来の3.7はサポートが終了しているためサポート対象外とします。)


## 開発環境

Python 3.12.2


## Installation

必要なライブラリは[Github - requirements](https://github.com/futo030/Poke-Controller-Modified-Extension/blob/master/requirements.txt)を参照してください。


## Wiki

現在作成を検討しております。
3DSの自動化については[こちら](https://draco-meteor.hatenablog.com/entry/20240514)の記事を参照ください。


## 謝辞

[Poke-Controller](https://github.com/KawaSwitch/Poke-Controller)の開発者である[KawaSwitch](https://github.com/KawaSwitch)氏、[Poke-Controller Modified](https://github.com/Moi-poke/Poke-Controller-Modified)の開発者である[moi_poke](https://github.com/Moi-poke)氏にそれぞれ感謝申し上げます。

<!-- ALL-CONTRIBUTORS-LIST:START - Do not remove or modify this section -->
<!-- prettier-ignore-start -->
<!-- markdownlint-disable -->
<table>
  <tr>
    <td align="center"><a href="https://github.com/KawaSwitch"><img src="https://avatars3.githubusercontent.com/u/41296626?v=4" width="100px;" alt=""/><br /><sub><b>KawaSwitch</b></sub></a><br /><a href="https://github.com/KawaSwitch/Poke-Controller/commits?author=KawaSwitch" title="Code">💻</a> <a href="#maintenance-KawaSwitch" title="Maintenance">🚧</a> <a href="https://github.com/KawaSwitch/Poke-Controller/commits?author=KawaSwitch" title="Documentation">📖</a> <a href="#question-KawaSwitch" title="Answering Questions">💬</a></td>
    <td align="center"><a href="https://github.com/Moi-poke"><img src="https://avatars1.githubusercontent.com/u/59233665?v=4" width="100px;" alt=""/><br /><sub><b>Moi-poke</b></sub></a><br /><a href="https://github.com/KawaSwitch/Poke-Controller/commits?author=Moi-poke" title="Code">💻</a> <a href="#question-Moi-poke" title="Answering Questions">💬</a></td>
  </tr>
</table>

<!-- markdownlint-enable -->
<!-- prettier-ignore-end -->
<!-- ALL-CONTRIBUTORS-LIST:END -->

## 貢献

このプロジェクトは, [all-contributors](https://github.com/all-contributors/all-contributors)仕様に準拠しています. どんな貢献も歓迎します。


## ライセンス

本プロジェクトはMITライセンスです。
詳細は [Github - LISENCE](https://github.com/futo030/Poke-Controller-Modified-Extension/blob/master/LICENSE) を参照ください。
※今後変更の可能性があります。

また, 本プロジェクトではLGPLライセンスのDirectShowLib-2005.dllを同梱し使用しています。
[About DirectShowLib](http://directshownet.sourceforge.net/)  
