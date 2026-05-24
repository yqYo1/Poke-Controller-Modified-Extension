# スクリプト開発ガイド

Poke-Controller Modified Extension での Python コマンドスクリプト作成方法

## 概要

Poke-Controller Modified Extension では、Python スクリプトを使ってゲーム機を自動制御できます。
既存の `PythonCommandBase` ベースのスクリプトはそのまま動作し、新しい `CommandBase` API も利用可能です。

## コマンドクラスの構造

### 最小構成

```python
from Commands.PythonCommandBase import PythonCommand
from Commands.Keys import Button

class MyCommand(PythonCommand):
    NAME = "My Command"
    
    def do(self):
        self.press(Button.A, duration=0.1, wait=0.2)
        self.finish()
```

### 必須要素

| 要素 | 型 | 説明 |
|------|-----|------|
| `NAME` | `str` | コマンド表示名 |
| `do()` | メソッド | コマンドのメイン処理 |

### オプション要素

| 要素 | 型 | 説明 |
|------|-----|------|
| `DESCRIPTION` | `str` | 詳細説明 |
| `onStart()` | メソッド | 開始時イベント |
| `onStop()` | メソッド | 停止時イベント |

## 入力操作

### ボタン

```python
from Commands.Keys import Button

# 単発押下
self.press(Button.A)

# 長押し
self.press(Button.A, duration=1.0)

# 押下後待機
self.press(Button.A, duration=0.1, wait=0.5)

# 複数ボタン同時押下
self.press(Button.A | Button.B)
```

| ボタン | 説明 |
|--------|------|
| `Button.A` | Aボタン |
| `Button.B` | Bボタン |
| `Button.X` | Xボタン |
| `Button.Y` | Yボタン |
| `Button.L` | Lボタン |
| `Button.R` | Rボタン |
| `Button.ZL` | ZLボタン |
| `Button.ZR` | ZRボタン |
| `Button.PLUS` | +ボタン |
| `Button.MINUS` | -ボタン |
| `Button.HOME` | HOMEボタン |
| `Button.CAPTURE` | キャプチャボタン |
| `Button.LCLICK` | Lスティック押し込み |
| `Button.RCLICK` | Rスティック押し込み |

### 方向キー（Hat）

```python
from Commands.Keys import Hat

self.press(Hat.TOP)
self.press(Hat.RIGHT)
self.press(Hat.BOTTOM)
self.press(Hat.LEFT)
self.press(Hat.TOP_RIGHT)
```

### アナログスティック

```python
from Commands.Keys import Stick

# 方向指定
self.press(Stick.LEFT, duration=0.5)
self.press(Stick.RIGHT, duration=0.5)
self.press(Stick.UP, duration=0.5)
self.press(Stick.DOWN, duration=0.5)

# 数値指定（-128〜127）
self.keys.l_stick(x=100, y=-50)
```

### タッチスクリーン

```python
# 座標指定でタップ
self.touch(100, 200)

# 長押し
self.touch(100, 200, duration=1.0)
```

## 画像処理

### テンプレートマッチング

```python
# 画像内にテンプレートが含まれるか確認
if self.isContainTemplate("template_name.png"):
    self.press(Button.A)

# 最大一致度を取得
max_val = self.isContainTemplate_max("template_name.png")
if max_val > 0.9:
    self.press(Button.A)
```

### 画像保存

```python
# カメラ画像を保存
self.saveCapture("screenshot.png")

# 指定領域を保存
self.saveCapture("region.png", x=100, y=100, w=200, h=200)
```

## 通知

### Discord

```python
# テキスト通知
self.discord_text("コマンド開始しました")

# 画像付き通知
self.discord_image("template_name.png", message="検出！")
```

## Print関数

```python
# 標準出力
self.print("メッセージ")

# エラー出力
self.printError("エラーが発生しました")

# 警告出力
self.printWarning("警告メッセージ")
```

## イベント

```python
class MyCommand(PythonCommand):
    NAME = "Event Sample"
    
    def onStart(self):
        self.print("コマンド開始")
        
    def do(self):
        self.press(Button.A)
        self.wait(1.0)
        
    def onStop(self):
        self.print("コマンド停止")
```

## 待機

```python
# 秒単位待機
self.wait(1.0)

# 短時間待機
self.short_wait()
```

## サンプルコマンド

### ボタン連打

```python
from Commands.PythonCommandBase import PythonCommand
from Commands.Keys import Button

class MashA(PythonCommand):
    NAME = "A連打"
    DESCRIPTION = "Aボタンを連打します"
    
    def do(self):
        while self.checkRunning():
            self.press(Button.A, duration=0.05, wait=0.05)
```

### 自動バトル

```python
from Commands.PythonCommandBase import PythonCommand
from Commands.Keys import Button

class AutoBattle(PythonCommand):
    NAME = "自動バトル"
    
    def do(self):
        while self.checkRunning():
            if self.isContainTemplate("battle.png"):
                self.press(Button.A, wait=0.5)
            else:
                self.wait(0.5)
```

## トラブルシューティング

| 問題 | 解決策 |
|------|--------|
| コマンドが停止しない | `finish()` を呼んでいるか確認 |
| 画像認識が動作しない | テンプレートパスを確認 |
| 通知が届かない | Webhook URL を確認 |
| シリアル通信エラー | ポート接続を確認 |
