# Web UI 詳細ガイド

Poke-Controller Modified Extension の Web UI（スマホ対応）の詳細を解説します。

## 目次

1. [概要](#1-概要)
2. [PWA インストール](#2-pwa-インストール)
3. [画面詳細](#3-画面詳細)
4. [スマホ最適化](#4-スマホ最適化)
5. [開発者向け](#5-開発者向け)

---

## 1. 概要

### 対応デバイス

| デバイス | 対応状況 | 推奨ブラウザ |
|---------|---------|------------|
| iPhone | ✅ 完全対応 | Safari, Chrome |
| Android | ✅ 完全対応 | Chrome, Firefox |
| iPad | ✅ 完全対応 | Safari |
| Android Tablet | ✅ 完全対応 | Chrome |
| デスクトップ | ✅ 対応 | Chrome, Firefox, Edge |

### 技術仕様

| 項目 | 仕様 |
|------|------|
| フレームワーク | React 19 |
| ビルドツール | Vite 6 |
| スタイリング | CSS Modules（モバイルファースト） |
| 通信 | Fetch API + WebSocket |
| PWA | Web App Manifest |
| アイコン | SVG + PNG（192px, 512px） |

---

## 2. PWA インストール

### iOS Safari

1. Safariで `http://(PCのIPアドレス):8020` を開く
2. 共有ボタン（□に↑）をタップ
3. 「ホーム画面に追加」を選択
4. アイコンがホーム画面に追加される

**注意事項:**
- iOS 16.4以降でService Worker対応
- ホーム画面追加後はフルスクリーンで動作
- ステータスバーは `black-translucent` で表示

### Android Chrome

1. Chromeで `http://(PCのIPアドレス):8020` を開く
2. 「ホーム画面に追加」ポップアップが表示されたらタップ
3. または ⋮ メニュー → 「ホーム画面に追加」
4. アイコンがホーム画面に追加される

**注意事項:**
- Android 5.0以降で完全対応
- インストール後はスタンドアロンアプリとして動作
- 戻るジェスチャーで前画面に戻る

### インストール後の確認

```javascript
// 開発者コンソールで確認
window.addEventListener('appinstalled', () => {
  console.log('PWA installed');
});

// スタンドアロン表示モード確認
if (window.matchMedia('(display-mode: standalone)').matches) {
  console.log('Running as PWA');
}
```

---

## 3. 画面詳細

### 📊 ダッシュボード

#### カメラプレビュー

- **リアルタイム映像**: WebSocket経由で1-30fps（設定可能）
- **base64 JPEG**: フレームをbase64エンコードして転送
- **フルスクリーン**: タップでフルスクリーン表示
- **キャプチャ**: 長押しで現在フレームを保存

```javascript
// フレーム表示
const img = document.getElementById('camera');
ws.onmessage = (event) => {
  const data = JSON.parse(event.data);
  if (data.type === 'camera.frame') {
    img.src = `data:image/jpeg;base64,${data.payload.image}`;
  }
};
```

#### 実行中コマンド

- **状態表示**: 緑のパルスドットで実行中表示
- **コマンド名**: 現在実行中のスクリプト名
- **経過時間**: 実行開始からの時間
- **停止ボタン**: ワンタップで停止

#### ログパネル

- **リアルタイム更新**: WebSocketで自動更新
- **色分け**: 情報（青）、警告（黄）、エラー（赤）、成功（緑）
- **スクロール**: 最新ログが常に表示
- **クリア**: 長押しでログクリア

### 🎮 コントローラー

#### ボタン配置

```
    ┌─────────┐
    │    Y    │
┌───┼────┬────┼───┐
│ X │    │    │ B │
└───┼────┴────┼───┘
    │    A    │
    └─────────┘
```

#### タッチ操作

| 操作 | 動作 |
|------|------|
| タップ | ボタン押下（0.1秒） |
| 長押し | ボタンhold |
| スワイプ（スティック） | アナログ入力 |
| ピンチ | 未使用 |

#### スティック操作

- **ドラッグ開始**: スティックエリアに触れる
- **ドラッグ中**: スティックノブが指に追従
- **リリース**: ノブが中央（128, 128）に戻る
- **デッドゾーン**: 中央±10%は無視

#### タッチスクリーン

- **タップ**: その位置にタッチイベント
- **ビジュアルフィードバック**: 赤いドットが表示されてフェードアウト
- **座標**: 320x240（Switchのタッチパネル解像度）

### 📜 スクリプト

#### スクリプト一覧

- **自動更新**: 画面表示時に自動取得
- **手動更新**: 更新ボタンで再取得
- **検索**: 名前でフィルタ（上部の検索バー）
- **タグ**: ディレクトリ名で分類表示

#### 実行制御

| 状態 | 表示 | 操作 |
|------|------|------|
| 停止中 | 灰色 | 「開始」ボタン有効 |
| 実行中 | 緑色パルス | 「停止」ボタン有効 |
| エラー | 赤色 | エラーメッセージ表示 |

### ⚙️ 設定

#### シリアル接続

- **ポート一覧**: 自動検出（更新ボタン）
- **直接入力**: 手動でポートパス入力
- **ボーレート**: 9600/19200/38400/57600/115200
- **接続状態**: インジケーター表示

#### カメラ設定

- **デバイス選択**: 接続カメラ一覧
- **解像度**: 640x480, 1280x720, 1920x1080
- **フレームレート**: 15/30/60fps
- **回転**: 0°/90°/180°/270°

#### 通知設定

- **LINE**: アクセストークン入力
- **Discord**: Webhook URL入力
- **テスト送信**: 各通知のテスト

---

## 4. スマホ最適化

### レスポンシブデザイン

#### ブレークポイント

| 画面幅 | レイアウト |
|--------|----------|
| < 360px | コンパクト（小さめボタン） |
| 360-768px | モバイル（標準） |
| 768-1024px | タブレット（2カラム） |
| > 1024px | デスクトップ（3カラム） |

#### タッチターゲットサイズ

- **最小タッチ領域**: 44x44px（Apple HIG準拠）
- **ボタン実際サイズ**: 48x48px以上
- **スティックエリア**: 100x100px
- **間隔**: 8px以上

### パフォーマンス最適化

#### 画像最適化

```css
/* カメラフレーム */
.camera-viewport img {
  width: 100%;
  height: 100%;
  object-fit: contain;
  will-change: transform; /* GPU加速 */
}
```

#### WebSocket最適化

```javascript
// フレームレート制限
let lastFrame = 0;
const FRAME_INTERVAL = 1000 / 30; // 30fps

ws.onmessage = (event) => {
  const now = Date.now();
  if (now - lastFrame < FRAME_INTERVAL) return;
  lastFrame = now;
  
  // フレーム更新
  updateFrame(event.data);
};
```

### バッテリー最適化

- **画面輝度**: 自動調整（Ambient Light Sensor）
- **WebSocket**: バックグラウンド時はフレーム更新停止
- **ジオロケーション**: 未使用（GPSオフ）

### アクセシビリティ

- **VoiceOver**: 全ボタンにラベル付与
- **TalkBack**: コンテンツ説明
- **ダイナミックタイプ**: フォントサイズ追従
- **Reduce Motion**: アニメーション抑制

---

## 5. 開発者向け

### フロントエンド開発

```bash
cd web

# 依存関係インストール
npm install

# 開発サーバー（ホットリロード）
npm run dev

# 本番ビルド
npm run build

# プレビュー
npm run preview
```

### APIクライアント拡張

```javascript
// api/client.js に追加
class APIClient {
  // 新しいAPI
  async getStats() {
    return this._fetch('/api/stats');
  }
  
  async setConfig(key, value) {
    return this._fetch('/api/config', {
      method: 'POST',
      body: JSON.stringify({ key, value }),
    });
  }
}
```

### コンポーネント追加

```jsx
// components/MyComponent.jsx
import React from 'react';

export default function MyComponent({ data }) {
  return (
    <div className="my-component">
      <h3>{data.title}</h3>
      <p>{data.description}</p>
    </div>
  );
}
```

### スタイル追加

```css
/* styles.css */
.my-component {
  padding: 12px;
  background: var(--bg-panel);
  border-radius: var(--radius);
}

@media (min-width: 768px) {
  .my-component {
    padding: 16px;
  }
}
```

### WebSocketイベント追加

```javascript
// App.jsx の onMessage に追加
ws.onMessage = (data) => {
  switch (data.type) {
    case 'my.custom.event':
      // カスタムイベント処理
      handleCustomEvent(data.payload);
      break;
  }
};
```

### ビルド設定

```javascript
// vite.config.js
export default defineConfig({
  plugins: [react()],
  build: {
    outDir: 'dist',
    sourcemap: true,
    rollupOptions: {
      output: {
        manualChunks: {
          vendor: ['react', 'react-dom'],
        },
      },
    },
  },
});
```

---

## 関連ドキュメント

- [エンドユーザー向けガイド](user-guide.md) - インストール、基本操作
- [スクリプト開発者向けガイド](script-guide.md) - PythonCommand API
- [開発者向けガイド](developer-guide.md) - アーキテクチャ、ビルド
