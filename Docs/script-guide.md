# スクリプト開発者向けガイド

Poke-Controller Modified Extension で自動化スクリプトを作成する開発者向けガイドです。

## 目次

1. [はじめに](#1-はじめに)
2. [スクリプトの基本構造](#2-スクリプトの基本構造)
3. [入力操作 API](#3-入力操作-api)
4. [画像認識 API](#4-画像認識-api)
5. [通知・通信 API](#5-通知通信-api)
6. [高度な機能](#6-高度な機能)
7. [サンプルスクリプト](#7-サンプルスクリプト)
8. [デバッグ・トラブルシューティング](#8-デバッグトラブルシューティング)

---

## 1. はじめに

### 対象読者

- Pythonの基本文法がわかる方
- Poke-Controller Modified Extension で自動化スクリプトを作りたい方
- 既存のスクリプトを改造・拡張したい方

### 既存スクリプトとの互換性

**既存のスクリプトは変更なしで動作します。**

`Commands.PythonCommandBase` や `Commands.Keys` へのインポートは、内部的に新しい `python/pokecon/` パッケージにリダイレクトされます。

---

## 2. スクリプトの基本構造

### 最小構成

```python
from Commands.PythonCommandBase import PythonCommand
from Commands.Keys import Button

class MyCommand(PythonCommand):
    NAME = "My Command"
    
    def do(self):
        self.press(Button.A, duration=0.1, wait=0.2)
```

### 配置場所

```
SerialController/Commands/PythonCommands/
├── MyCommand.py          # 自作スクリプト
├── AutoLeague.py         # 付属サンプル
└── ...
```

### クラスの要件

| 要素 | 必須 | 説明 |
|------|------|------|
| `PythonCommand` 継承 | ✅ | 基底クラス |
| `NAME` | ✅ | 表示名（日本語可） |
| `do(self)` | ✅ | 実行本体 |
| `TAGS` | ❌ | 分類タグ（自動付与可） |

### 画像認識を使う場合

```python
from Commands.PythonCommandBase import ImageProcPythonCommand
from Commands.Keys import Button

class MyImageCommand(ImageProcPythonCommand):
    NAME = "Image Recognition Sample"
    
    def do(self):
        # テンプレート画像が見つかるまで待機
        while not self.isContainTemplate("target.png"):
            self.wait(0.5)
        
        self.press(Button.A)
```

---

## 3. 入力操作 API

### ボタン入力

```python
from Commands.Keys import Button, Hat, Direction, Stick, Touchscreen

# 単一ボタン
self.press(Button.A)

# 複数ボタン同時押し
self.press([Button.A, Button.B])

# D-Pad
self.press(Hat.UP)
self.press(Hat.LEFT)

# アナログスティック（方向指定）
self.press(Direction(Stick.LEFT, 90))    # 左スティック 上
self.press(Direction(Stick.RIGHT, 180)) # 右スティック 右

# アナログスティック（座標指定）
self.press(Direction(Stick.LEFT, 128, 0))   # x=128, y=0（上）

# タッチスクリーン（Qingpi時）
self.press(Touchscreen(100, 200))
```

### 入力メソッド一覧

| メソッド | 説明 | 使用例 |
|---------|------|--------|
| `press(buttons, duration=0.1, wait=0.1)` | 押下→待機→解放→待機 | `self.press(Button.A)` |
| `pressRep(buttons, repeat, duration=0.1, interval=0.1, wait=0.1)` | 連打 | `self.pressRep(Button.A, 5)` |
| `hold(buttons, wait=0.1)` | 押しっぱなし | `self.hold(Button.A)` |
| `holdEnd(buttons)` | hold解除 | `self.holdEnd(Button.A)` |
| `wait(sec)` | 待機（0.1秒超はsleep） | `self.wait(1.0)` |
| `short_wait(sec)` | ビジーウェイト待機 | `self.short_wait(0.05)` |
| `finish()` | コマンド終了 | `self.finish()` |

### 引数の型

```python
# buttons の取りうる型
Button.A                    # 単一ボタン
[Button.A, Button.B]        # リスト
Hat.UP                      # D-Pad
Direction(Stick.LEFT, 90)   # スティック方向（角度）
Direction(Stick.LEFT, 128, 128)  # スティック座標
Touchscreen(100, 200)       # タッチ座標
```

### シリアル直接送信（上級者向け）

```python
# フォーマットに依存しない生データ送信
self.direct_serial(["btn_a", "btn_b"], [0.1, 0.1])
```

---

## 4. 画像認識 API

### テンプレートマッチング

```python
# テンプレート画像が画面内に存在するか
if self.isContainTemplate("template.png"):
    self.press(Button.A)

# 部分領域で検索（左上x, 左上y, 幅, 高さ）
if self.isContainTemplate("template.png", show_position=True, 
                          search_range=(100, 100, 200, 150)):
    self.press(Button.A)
```

### 画像包含判定

```python
# 大きな画像が小さな画像を含むか
if self.isContainedImage("large.png", "small.png"):
    self.press(Button.A)
```

### 画像保存

```python
# 現在のフレームを保存
self.save_capture("screenshot.png")

# 特定領域を切り出して保存
self.save_capture("region.png", crop=(100, 100, 200, 200))
```

### 画像認識メソッド一覧

| メソッド | 説明 |
|---------|------|
| `isContainTemplate(template_path, show_position=False, search_range=None)` | テンプレート存在判定 |
| `isContainedImage(large_path, small_path)` | 包含判定 |
| `save_capture(filename, crop=None)` | キャプチャ保存 |
| `readFrame()` | 現在フレーム取得（OpenCV形式） |
| `convertCv2Format(frame)` | フォーマット変換 |

---

## 5. 通知・通信 API

### 通知

```python
# LINE通知（トークン設定が必要）
self.LINE_text("コマンド完了しました！")

# Discord通知（Webhook URL設定が必要）
self.discord_text("シャイニーが出現しました！")

# Windows通知
self.notification("タイトル", "メッセージ")
```

### Socket通信

```python
# 接続
self.socket_connect("192.168.1.100", 8080)

# データ送信
self.socket0.send("hello".encode())

# 切断
self.socket_disconnect()
```

### MQTT通信

```python
# 接続
self.mqtt_connect("broker.hivemq.com", 1883, "pokecon/topic")

# データ送信
self.mqtt0.publish("pokecon/topic", "data")

# 切断
self.mqtt_disconnect()
```

---

## 6. 高度な機能

### 一時停止対応

```python
class PauseableCommand(PythonCommand):
    NAME = "Pauseable"
    
    @pausedecorator
    def custom_action(self):
        # このメソッドは一時停止可能
        self.press(Button.A)
    
    def do(self):
        self.custom_action()
        self.wait(1.0)
```

### ダイアログ

```python
# ユーザー入力を求める
text = self.dialogue("名前を入力してください")

# 選択式ダイアログ
choice = self.dialogue("モードを選択", options=["A", "B", "C"])
```

### 変数表示（デバッグ）

```python
def do(self):
    self.counter = 0
    self.target = "ピカチュウ"
    
    # 一時停止中に show_var() で変数一覧が表示される
    self.isPause = True
    self.show_var()
```

### ログ出力

```python
# 通常ログ
self.print("通常メッセージ")

# 情報ログ
self.print_info("情報メッセージ")

# 警告ログ
self.print_warning("警告メッセージ")

# エラーログ
self.print_error("エラーメッセージ")
```

---

## 7. サンプルスクリプト

### Aボタン連打

```python
from Commands.PythonCommandBase import PythonCommand
from Commands.Keys import Button

class MashA(PythonCommand):
    NAME = "A連打"
    
    def do(self):
        for _ in range(100):
            self.press(Button.A)
            self.checkIfAlive()  # 停止要求を確認
```

### 画像認識で待機

```python
from Commands.PythonCommandBase import ImageProcPythonCommand
from Commands.Keys import Button

class WaitForTarget(ImageProcPythonCommand):
    NAME = "画像待機"
    
    def do(self):
        # テンプレートが見つかるまで待機
        while not self.isContainTemplate("target.png"):
            self.wait(0.5)
            self.checkIfAlive()
        
        self.press(Button.A)
        self.wait(1.0)
```

### 条件分岐

```python
from Commands.PythonCommandBase import ImageProcPythonCommand
from Commands.Keys import Button

class Conditional(ImageProcPythonCommand):
    NAME = "条件分岐"
    
    def do(self):
        if self.isContainTemplate("shiny.png"):
            self.press(Button.A)  # 捕まえる
            self.discord_text("シャイニー出現！")
        else:
            self.press(Button.B)  # 逃げる
```

---

## 8. デバッグ・トラブルシューティング

### スクリプトが表示されない

1. `NAME` が定義されているか確認
2. ファイル名が `.py` で終わるか確認
3. Web UI の「スクリプト」タブで「更新」ボタンをタップ

### インポートエラー

```python
# 古いインポートパス（自動的に新パッケージにリダイレクト）
from Commands.PythonCommandBase import PythonCommand
from Commands.Keys import Button

# 新しいインポートパス（直接利用も可能）
from pokecon import PythonCommand, Button
```

### 型ヒントの活用（Python 3.14+）

```python
from typing import override

class MyCommand(PythonCommand):
    NAME: str = "My Command"
    
    @override
    def do(self) -> None:
        self.press(Button.A)
```

### テストの実行

```bash
# 特定のスクリプトの互換性をテスト
pytest tests/test_script_compatibility.py -v -k "my_script"

# 全テスト
pytest tests/ -v
```

---

## 関連ドキュメント

- [エンドユーザー向けガイド](user-guide.md) - インストール、使い方
- [APIリファレンス](api-reference.md) - 全APIの詳細仕様
- [開発者向けガイド](developer-guide.md) - アーキテクチャ、ビルド
