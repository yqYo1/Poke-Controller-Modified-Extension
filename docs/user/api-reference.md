# Poke-Controller Modified Extension — API リファレンス

> **対象ブランチ**: `refactor/rust-core`  
> **最終更新日**: 2026-05-24  
> **対応アーキテクチャ**: Tauri (Rust) + SvelteKit 5

---

## 目次

1. [REST API エンドポイント](#1-rest-api-エンドポイント)
2. [WebSocket メッセージ](#2-websocket-メッセージ)
3. [WebRTC ストリーミング](#3-webrtc-ストリーミング)
4. [Tauri IPC コマンド](#4-tauri-ipc-コマンド)
5. [Tauri IPC イベント](#5-tauri-ipc-イベント)
6. [TypeScript 型定義](#6-typescript-型定義)
7. [Python API](#7-python-api)
8. [エラーハンドリング](#8-エラーハンドリング)

---

## 1. REST API エンドポイント

アプリケーションは通知関連の REST API エンドポイントを提供します。エンドポイントは Tauri バックエンドの HTTP サーバーまたは WebSocket サーバー経由で利用可能です。

### 1.1 通知 API

#### POST /api/notification/discord-text

Discord テキスト通知を送信します。

**リクエストボディ (JSON)**:

| フィールド | 型 | 必須 | 説明 |
|-----------|------|----------|-------------|
| `content` | `string` | はい | 通知メッセージ本文 |
| `index` | `number` | いいえ | カメラインデックス（デフォルト: `0`） |

**レスポンス**:

| ステータスコード | 説明 |
|-----------------|-------------|
| `200 OK` | 通知が送信されました |
| `400 Bad Request` | リクエストボディが不正です |
| `500 Internal Server Error` | Webhook URL が設定されていない、または送信に失敗しました |

**リクエスト例**:

```bash
curl -X POST http://localhost:9876/api/notification/discord-text \
  -H "Content-Type: application/json" \
  -d '{"content": "コマンドが完了しました", "index": 0}'
```

---

#### POST /api/notification/discord-image

テキストとカメラ画像を含む Discord 通知を送信します。

**リクエストボディ (JSON)**:

| フィールド | 型 | 必須 | 説明 |
|-----------|------|----------|-------------|
| `content` | `string` | はい | 通知メッセージ本文 |
| `index` | `number` | いいえ | カメラインデックス（デフォルト: `0`） |
| `crop_fmt` | `string` | いいえ | クロップフォーマット（例: `'100,100,200,200'`） |
| `crop` | `string` | いいえ | クロップ領域の指定（左上 x,y, 幅, 高さ） |

**レスポンス**:

| ステータスコード | 説明 |
|-----------------|-------------|
| `200 OK` | 画像付き通知が送信されました |
| `400 Bad Request` | リクエストボディが不正です |
| `500 Internal Server Error` | Webhook URL が設定されていない、または送信に失敗しました |

**リクエスト例**:

```bash
curl -X POST http://localhost:9876/api/notification/discord-image \
  -H "Content-Type: application/json" \
  -d '{"content": "キャプチャ完了", "index": 0, "crop": "100,100,400,300"}'
```

---

#### POST /api/notification/line-text

LINE テキスト通知を送信します（スタブ — LINE UI は削除済み）。

**リクエストボディ (JSON)**:

| フィールド | 型 | 必須 | 説明 |
|-----------|------|----------|-------------|
| `content` | `string` | はい | 通知メッセージ本文 |
| `index` | `number` | いいえ | カメラインデックス（デフォルト: `0`） |

**レスポンス**: `501 Not Implemented`

> **注意**: LINE 通知 UI は削除されましたが、API エンドポイントは後方互換性のために維持されています。

---

#### POST /api/notification/line-image

LINE 画像付き通知を送信します（スタブ — LINE UI は削除済み）。

**リクエストボディ (JSON)**:

| フィールド | 型 | 必須 | 説明 |
|-----------|------|----------|-------------|
| `content` | `string` | はい | 通知メッセージ本文 |
| `index` | `number` | いいえ | カメラインデックス（デフォルト: `0`） |
| `crop_fmt` | `string` | いいえ | クロップフォーマット |
| `crop` | `string` | いいえ | クロップ領域の指定 |

**レスポンス**: `501 Not Implemented`

---

### 1.2 共有 HTTP エラーレスポンス

すべてのエンドポイントで共通のエラーレスポンス形式:

```json
{
  "error": "エラーメッセージ",
  "code": "ERROR_CODE"
}
```

| エラーコード | 説明 |
|-------------|-------------|
| `INVALID_REQUEST` | リクエストボディが不正（JSON パースエラー、必須フィールド欠落） |
| `WEBSOCKET_NOT_FOUND` | 内部 Webhook URL が設定されていない |
| `SEND_FAILED` | 通知の送信に失敗しました |
| `NOT_IMPLEMENTED` | エンドポイントは実装されていません（LINE 系） |

---

## 2. WebSocket メッセージ

アプリケーションは Tauri バックエンドで WebSocket サーバー（`tokio-tungstenite`）を起動し、シリアルデータやログをリアルタイムでフロントエンドに配信します。

### 2.1 接続

```
ws://localhost:9876
```

**接続例**:

```typescript
import WebSocket from 'ws';

const ws = new WebSocket('ws://localhost:9876');

ws.onopen = () => {
  console.log('WebSocket 接続確立');
};

ws.onmessage = (event) => {
  const msg = JSON.parse(event.data);
  console.log('受信:', msg);
};

ws.onclose = () => {
  console.log('WebSocket 接続切断');
};
```

---

### 2.2 メッセージ形式

すべての WebSocket メッセージは JSON 形式で送受信されます。

**基本構造**:

```typescript
{
  type: 'serial_data' | 'log' | 'command_status' | 'camera_frame';
  payload: unknown;
  timestamp?: string;  // ISO 8601 形式
}
```

---

### 2.3 メッセージタイプ一覧

#### serial_data

シリアルポートから受信したデータを配信します。

```json
{
  "type": "serial_data",
  "payload": "0xABCD 08 80 80 80 80",
  "timestamp": "2026-05-24T10:55:00.000Z"
}
```

| フィールド | 型 | 説明 |
|-----------|------|-------------|
| `payload` | `string` | シリアルデータの生文字列（改行含む） |
| `timestamp` | `string` | 受信時刻（ISO 8601） |

---

#### log

アプリケーションのログメッセージを配信します。

```json
{
  "type": "log",
  "payload": {
    "level": "INFO",
    "message": "シリアルポートに接続しました",
    "tag": "serial",
    "panel": "1"
  },
  "timestamp": "2026-05-24T10:55:00.000Z"
}
```

| ペイロードフィールド | 型 | 説明 |
|----------------------|------|-------------|
| `level` | `string` | ログレベル: `DEBUG` / `INFO` / `WARN` / `ERROR` |
| `message` | `string` | ログメッセージ本文 |
| `tag` | `string` | ログタグ（例: `serial`, `camera`, `command`, `system`） |
| `panel` | `string` | 出力先パネル: `"1"`（上部） / `"2"`（下部） |

---

#### command_status

コマンド実行の状態変化を配信します。

```json
{
  "type": "command_status",
  "payload": {
    "name": "my_command",
    "status": "running",
    "message": "コマンドを実行中..."
  },
  "timestamp": "2026-05-24T10:55:00.000Z"
}
```

| ペイロードフィールド | 型 | 説明 |
|----------------------|------|-------------|
| `name` | `string` | コマンド名 |
| `status` | `string` | 状態: `running` / `completed` / `failed` / `stopped` / `paused` |
| `message` | `string` | 状態説明メッセージ |

---

#### camera_frame

カメラフレームデータを配信します（WebRTC 非使用時の代替）。

```json
{
  "type": "camera_frame",
  "payload": {
    "frame": "base64_encoded_image_data",
    "format": "jpeg",
    "width": 640,
    "height": 480
  },
  "timestamp": "2026-05-24T10:55:00.000Z"
}
```

| ペイロードフィールド | 型 | 説明 |
|----------------------|------|-------------|
| `frame` | `string` | Base64 エンコードされた画像データ |
| `format` | `string` | 画像フォーマット: `jpeg` / `png` |
| `width` | `number` | フレーム幅（px） |
| `height` | `number` | フレーム高さ（px） |

---

### 2.4 クライアント実装（wsClient.ts）

```typescript
// web/src/lib/services/wsClient.ts
import { writable } from 'svelte/store';

export type WsMessage = {
  type: 'serial_data' | 'log' | 'command_status' | 'camera_frame';
  payload: unknown;
};

class WsClient {
  private ws: WebSocket | null = null;
  public messages = writable<WsMessage[]>([]);

  connect(url: string = 'ws://localhost:9876') {
    this.ws = new WebSocket(url);
    this.ws.onmessage = (event) => {
      const msg: WsMessage = JSON.parse(event.data);
      this.messages.update((m) => [...m, msg]);
    };
  }

  send(data: unknown) {
    this.ws?.send(JSON.stringify(data));
  }

  disconnect() {
    this.ws?.close();
    this.ws = null;
  }
}

export const wsClient = new WsClient();
```

---

## 3. WebRTC ストリーミング

低遅延のカメラストリーミングには WebRTC を使用します。バックエンド（Rust Core）が WebRTC の Offer/SDP を生成し、フロントエンドが Answer を返すシグナリングフローを採用しています。

### 3.1 シグナリングフロー

```
フロントエンド                    バックエンド (Rust)
     │                                │
     │  invoke('start_camera_webrtc') │
     │───────────────────────────────>│
     │                                │
     │     SDP Offer (JSON)           │
     │<───────────────────────────────│
     │                                │
     │  createAnswer()                │
     │  setLocalDescription(answer)   │
     │                                │
     │  invoke('set_camera_webrtc_answer', { sdp }) │
     │───────────────────────────────>│
     │                                │
     │       ICE Candidate Exchange   │
     │◄══════════════════════════════►│
     │                                │
     │       MediaStream (video)      │
     │◄───────────────────────────────│
```

### 3.2 クライアント実装（webrtcClient.ts）

```typescript
// web/src/lib/services/webrtcClient.ts
import { invoke } from '@tauri-apps/api/core';

export class WebRTCClient {
  private pc: RTCPeerConnection;
  private channel: RTCDataChannel | null = null;

  constructor() {
    this.pc = new RTCPeerConnection({
      iceServers: [{ urls: 'stun:stun.l.google.com:19302' }],
    });
  }

  async startStream(): Promise<MediaStream> {
    const stream = new MediaStream();
    this.pc.ontrack = (event) => {
      stream.addTrack(event.track);
    };

    // Tauri コマンド経由で SDP オファーを取得
    const offer = await invoke('start_camera_webrtc');
    await this.pc.setRemoteDescription(offer);
    const answer = await this.pc.createAnswer();
    await this.pc.setLocalDescription(answer);
    await invoke('set_camera_webrtc_answer', { sdp: answer });

    return stream;
  }

  close() {
    this.channel?.close();
    this.pc.close();
  }
}
```

### 3.3 ICE サーバー設定

| サーバー | URL | 用途 |
|----------|-----|------|
| STUN | `stun:stun.l.google.com:19302` | NAT 越えのためのアドレス解決 |

### 3.4 WebRTC 関連 Tauri コマンド

| コマンド | 引数 | 戻り値 | 説明 |
|---------|------|---------|-------------|
| `start_camera_webrtc` | なし | `RTCSessionDescription` (Offer) | WebRTC 接続を開始し SDP Offer を返す |
| `set_camera_webrtc_answer` | `{ sdp: RTCSessionDescription }` | `void` | フロントエンドの SDP Answer を設定する |

---

## 4. Tauri IPC コマンド

フロントエンドは Tauri IPC（`invoke`）を介して Rust バックエンドの機能を呼び出します。直接 `invoke()` を呼び出すのではなく、`tauriBridge.ts` のラッパー関数を経由することを推奨します。

### 4.1 シリアル通信

#### serial_connect

シリアルポートに接続します。

```typescript
import { invoke } from '@tauri-apps/api/core';

const result = await invoke('serial_connect', {
  port: 'COM3',      // Windows の場合
  baudRate: 115200,   // デフォルト: 115200
});
```

| 引数 | 型 | 必須 | 説明 |
|---------|------|----------|-------------|
| `port` | `string` | はい | シリアルポートのパス（例: `COM3`, `/dev/ttyACM0`） |
| `baudRate` | `number` | はい | ボーレート（デフォルト: `115200`） |

**戻り値**: `Result<void, string>`

| エラーメッセージ | 説明 |
|-----------------|-------------|
| `Port not found: <port>` | 指定されたポートが見つかりません |
| `Failed to open port: <reason>` | ポートのオープンに失敗しました（権限不足、他のアプリが使用中） |
| `Invalid baud rate: <rate>` | 無効なボーレートです |
| `Already connected` | すでに接続済みです |

---

#### serial_disconnect

シリアルポートを切断します。

```typescript
await invoke('serial_disconnect');
```

**引数**: なし  
**戻り値**: `Result<void, string>`

| エラーメッセージ | 説明 |
|-----------------|-------------|
| `Not connected` | シリアルポートに接続されていません |

---

#### list_serial_ports

利用可能なシリアルポートの一覧を取得します。

```typescript
const ports = await invoke('list_serial_ports');
// 戻り値: Array<{ path: string; manufacturer: string | null }>
```

**引数**: なし  
**戻り値**: `Array<{ path: string; manufacturer: string | null }>`

---

#### serial_send

シリアルポートにコマンドを送信します。

```typescript
await invoke('serial_send', {
  command: '0xABCD 08 80 80 80 80',
});
```

| 引数 | 型 | 必須 | 説明 |
|---------|------|----------|-------------|
| `command` | `string` | はい | 送信するコマンド文字列（ワイヤーフォーマット） |

**戻り値**: `Result<void, string>`

---

### 4.2 カメラ制御

#### start_camera

カメラキャプチャを開始します。

```typescript
await invoke('start_camera', {
  source: 0,          // カメラデバイスインデックス
  width: 640,         // 解像度 幅（px）
  height: 480,        // 解像度 高さ（px）
  fps: 30,            // フレームレート（1〜30、範囲外は自動クランプ）
});
```

| 引数 | 型 | 必須 | デフォルト | 説明 |
|---------|------|----------|----------|-------------|
| `source` | `number` | いいえ | `0` | カメラデバイスインデックス |
| `width` | `number` | いいえ | `640` | キャプチャ解像度（幅） |
| `height` | `number` | いいえ | `480` | キャプチャ解像度（高さ） |
| `fps` | `number` | いいえ | `30` | フレームレート（1〜30） |

**戻り値**: `Result<void, string>`

---

#### stop_camera

カメラキャプチャを停止します。

```typescript
await invoke('stop_camera');
```

**引数**: なし  
**戻り値**: `Result<void, string>`

---

#### capture_screenshot

現在のカメラフレームのスクリーンショットを保存します。

```typescript
await invoke('capture_screenshot', {
  format: 'png',      // 'png' | 'jpeg'
  path: '/path/to/save/screenshot.png',
  crop: null,         // { x: number; y: number; width: number; height: number } | null
});
```

| 引数 | 型 | 必須 | デフォルト | 説明 |
|---------|------|----------|----------|-------------|
| `format` | `string` | いいえ | `'png'` | 保存形式: `'png'` / `'jpeg'` |
| `path` | `string` | いいえ | 自動生成 | 保存先のファイルパス |
| `crop` | `object` | いいえ | `null` | クロップ領域 `{ x, y, width, height }` |

**戻り値**: `Result<string, string>` — 保存先のファイルパスを返します。

---

### 4.3 コマンド実行

#### execute_command

Python 自動化コマンドを実行します。

```typescript
await invoke('execute_command', {
  commandName: 'my_auto_command',
});
```

| 引数 | 型 | 必須 | 説明 |
|---------|------|----------|-------------|
| `commandName` | `string` | はい | 実行するコマンド名（Python スクリプトのクラス名またはファイル名） |

**戻り値**: `Result<void, string>`

---

#### stop_command

実行中のコマンドを停止します。

```typescript
await invoke('stop_command');
```

**引数**: なし  
**戻り値**: `Result<void, string>`

---

#### list_commands

利用可能な Python コマンドの一覧を取得します。

```typescript
const commands = await invoke('list_commands');
// 戻り値: Array<{
//   name: string;
//   description: string;
//   tags: string[];
//   has_params: boolean;
// }>
```

**引数**: なし  
**戻り値**: コマンド情報の配列

---

#### reload_commands

コマンド一覧を再読み込みします。

```typescript
await invoke('reload_commands');
```

**引数**: なし  
**戻り値**: `Result<void, string>`

---

### 4.4 アプリケーション

#### get_app_info

アプリケーションのバージョン情報を取得します。

```typescript
const info = await invoke('get_app_info');
// 戻り値: {
//   version: string;
//   tauri_version: string;
//   platform: string;
// }
```

**引数**: なし

---

### 4.5 全 Tauri コマンド一覧

| コマンド | カテゴリ | 説明 |
|---------|----------|-------------|
| `serial_connect` | シリアル | シリアルポートに接続 |
| `serial_disconnect` | シリアル | シリアルポートを切断 |
| `list_serial_ports` | シリアル | 利用可能なシリアルポート一覧を取得 |
| `serial_send` | シリアル | シリアルコマンドを送信 |
| `start_camera` | カメラ | カメラキャプチャを開始 |
| `stop_camera` | カメラ | カメラキャプチャを停止 |
| `capture_screenshot` | カメラ | スクリーンショットを保存 |
| `start_camera_webrtc` | カメラ | WebRTC ストリーミングを開始（SDP Offer） |
| `set_camera_webrtc_answer` | カメラ | WebRTC SDP Answer を設定 |
| `execute_command` | コマンド | Python 自動化コマンドを実行 |
| `stop_command` | コマンド | 実行中のコマンドを停止 |
| `list_commands` | コマンド | 利用可能なコマンド一覧を取得 |
| `reload_commands` | コマンド | コマンド一覧を再読み込み |
| `get_app_info` | システム | アプリケーション情報を取得 |

---

## 5. Tauri IPC イベント

Tauri のイベントシステムを使用して、バックエンドからフロントエンドへのプッシュ通知を受信します。

### 5.1 イベント一覧

#### serial-data

シリアルポートからの受信データ。

```typescript
import { listen } from '@tauri-apps/api/event';

const unlisten = await listen<string>('serial-data', (event) => {
  console.log('シリアルデータ:', event.payload);
});
```

| ペイロード | 型 | 説明 |
|---------|------|-------------|
| `payload` | `string` | 受信したシリアルデータ |

---

#### command-complete

コマンド実行の完了通知。

```typescript
await listen<string>('command-complete', (event) => {
  console.log('コマンド完了:', event.payload);
});
```

| ペイロード | 型 | 説明 |
|---------|------|-------------|
| `payload` | `string` | 完了したコマンド名 |

---

### 5.2 全イベント一覧

| イベント名 | ペイロード型 | 説明 |
|-----------|-------------|-------------|
| `serial-data` | `string` | シリアル受信データ |
| `command-complete` | `string` | コマンド実行完了 |
| `serial-error` | `string` | シリアル通信エラー |
| `camera-error` | `string` | カメラエラー |
| `ws-status` | `{ connected: boolean }` | WebSocket 接続状態変化 |

### 5.3 ラッパー実装（tauriBridge.ts）

```typescript
// web/src/lib/services/tauriBridge.ts
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

// === シリアル通信 ===

export async function serialConnect(port: string, baudRate: number) {
  return invoke('serial_connect', { port, baudRate });
}

export async function serialDisconnect() {
  return invoke('serial_disconnect');
}

export async function listSerialPorts() {
  return invoke('list_serial_ports');
}

export async function serialSend(command: string) {
  return invoke('serial_send', { command });
}

// === カメラ ===

export async function startCamera(
  source = 0,
  width = 640,
  height = 480,
  fps = 30,
) {
  return invoke('start_camera', { source, width, height, fps });
}

export async function stopCamera() {
  return invoke('stop_camera');
}

export async function captureScreenshot(
  format: 'png' | 'jpeg' = 'png',
  path?: string,
  crop?: { x: number; y: number; width: number; height: number } | null,
) {
  return invoke('capture_screenshot', { format, path, crop });
}

// === コマンド ===

export async function executeCommand(commandName: string) {
  return invoke('execute_command', { commandName });
}

export async function stopCommand() {
  return invoke('stop_command');
}

export async function listCommands() {
  return invoke('list_commands');
}

export async function reloadCommands() {
  return invoke('reload_commands');
}

// === イベント購読 ===

export function onSerialData(callback: (data: string) => void) {
  return listen<string>('serial-data', (event) => {
    callback(event.payload);
  });
}

export function onCommandComplete(callback: (name: string) => void) {
  return listen<string>('command-complete', (event) => {
    callback(event.payload);
  });
}

export function onSerialError(callback: (error: string) => void) {
  return listen<string>('serial-error', (event) => {
    callback(event.payload);
  });
}

export function onCameraError(callback: (error: string) => void) {
  return listen<string>('camera-error', (event) => {
    callback(event.payload);
  });
}

export function onWsStatus(
  callback: (status: { connected: boolean }) => void,
) {
  return listen<{ connected: boolean }>('ws-status', (event) => {
    callback(event.payload);
  });
}
```

---

## 6. TypeScript 型定義

### 6.1 シリアル関連

```typescript
// web/src/lib/types.ts (シリアル関連)

/** シリアルポート情報 */
interface SerialPortInfo {
  path: string;
  manufacturer: string | null;
}

/** シリアル接続状態 */
type SerialConnectionState = 'disconnected' | 'connecting' | 'connected' | 'error';

/** シリアルコマンド（ワイヤーフォーマット） */
interface SerialCommand {
  buttonMask: number;       // 0xXXXX
  hat: HatPosition;         // HH
  leftStick: StickPosition; // XX XX
  rightStick: StickPosition;// XX XX
}

/** Hat（十字キー）位置 */
enum HatPosition {
  Center     = 0x08,
  Up         = 0x00,
  UpRight    = 0x01,
  Right      = 0x02,
  DownRight  = 0x03,
  Down       = 0x04,
  DownLeft   = 0x05,
  Left       = 0x06,
  UpLeft     = 0x07,
}

/** スティック位置 */
interface StickPosition {
  x: number;  // 0x00–0xFF (デフォルト: 0x80)
  y: number;  // 0x00–0xFF (デフォルト: 0x80)
}
```

### 6.2 カメラ関連

```typescript
// web/src/lib/types.ts (カメラ関連)

/** カメラ設定 */
interface CameraSettings {
  source: number;           // カメラデバイスインデックス
  width: number;            // キャプチャ解像度（幅）
  height: number;           // キャプチャ解像度（高さ）
  fps: number;              // フレームレート（1〜30）
  flip: boolean;            // 反転
  rotation: number;         // 回転角度（0, 90, 180, 270）
}

/** スクリーンショット保存形式 */
type ScreenshotFormat = 'png' | 'jpeg';

/** クロップ領域 */
interface CropRegion {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** カメラ表示オーバーレイ設定 */
interface DisplayOverlay {
  showTemplateMatch: boolean;
  showRectangles: boolean;
  showTextAnnotations: boolean;
}

/** マウス操作モード */
type MouseMode = 'pan' | 'zoom' | 'templateSelect' | 'crop';
```

### 6.3 コマンド関連

```typescript
// web/src/lib/types.ts (コマンド関連)

/** コマンド情報 */
interface CommandInfo {
  name: string;
  description: string;
  tags: string[];
  hasParams: boolean;
}

/** コマンド実行状態 */
type CommandStatus = 'idle' | 'running' | 'completed' | 'failed' | 'stopped' | 'paused';

/** コマンド状態イベント */
interface CommandStatusEvent {
  name: string;
  status: CommandStatus;
  message?: string;
}
```

### 6.4 ログ関連

```typescript
// web/src/lib/types.ts (ログ関連)

/** ログレベル */
type LogLevel = 'DEBUG' | 'INFO' | 'WARN' | 'ERROR';

/** ログエントリ */
interface LogEntry {
  level: LogLevel;
  message: string;
  tag?: string;
  panel: '1' | '2';
  timestamp: string;  // ISO 8601
}

/** ログフィルター設定 */
interface LogFilter {
  level: LogLevel | 'ALL';
  tag?: string;
  panel?: '1' | '2';
}
```

### 6.5 通知関連

```typescript
// web/src/lib/types.ts (通知関連)

/** 通知設定 */
interface NotificationSettings {
  discordWebhook: string | null;
  isWinNotStart: boolean;
  isWinNotEnd: boolean;
  isDiscordNotStart: boolean;
  isDiscordNotEnd: boolean;
}

/** Discord 通知リクエスト */
interface DiscordTextRequest {
  content: string;
  index?: number;
}

/** Discord 画像通知リクエスト */
interface DiscordImageRequest {
  content: string;
  index?: number;
  crop_fmt?: string;
  crop?: string;
}
```

### 6.6 ウィジェット関連

```typescript
// web/src/lib/types.ts (ウィジェット関連)

/** ウィジェットモード（7 モード） */
type WidgetMode = 'Entry' | 'Check' | 'Combo' | 'Radio' | 'Spin' | 'Scale' | 'Next';

/** ダイアログ表示位置 */
type DialoguePosition = 1 | 2 | 3;
// 1 = 上部, 2 = 中央（デフォルト）, 3 = 下部
```

### 6.7 アプリケーション

```typescript
// web/src/lib/types.ts (アプリケーション)

/** アプリケーション情報 */
interface AppInfo {
  version: string;
  tauriVersion: string;
  platform: string;
}

/** アプリケーション設定 */
interface AppSettings {
  port: string | null;
  baudRate: number;
  camera: CameraSettings;
  notification: NotificationSettings;
  logFilter: LogFilter;
  widgetMode: WidgetMode;
  dialoguePosition: DialoguePosition;
  stdoutDestination: '1' | '2';
  controllerPosition: { x: number; y: number };
  screenshotFormat: ScreenshotFormat;
}
```

---

## 7. Python API

Python 互換レイヤーは、既存の Python 自動化スクリプトとの互換性を提供します。PyO3 を介して Rust Core に埋め込まれた CPython インタプリタで実行されます。

### 7.1 基底クラス: `CommandBase`

```python
# python/poke_controller/commands/base.py

from typing import Optional


class CommandBase:
    """すべての自動化コマンドの基底クラス。"""

    # === クラス属性 ===

    name: str = ""
    """コマンド名。"""

    description: str = ""
    """コマンドの説明文。"""

    params: list[str] = []
    """コマンドパラメータ。"""

    # 通知設定
    isWinNotStart: bool = False
    """開始時に Windows 通知を表示するか。"""

    isWinNotEnd: bool = False
    """終了時に Windows 通知を表示するか。"""

    isDiscordNotStart: bool = False
    """開始時に Discord 通知を送信するか。"""

    isDiscordNotEnd: bool = False
    """終了時に Discord 通知を送信するか。"""

    # 出力設定
    stdout_destination: str = "1"
    """
    `print()` 関数の出力先パネル。
    - `"1"`: 上部パネル
    - `"2"`: 下部パネル
    """

    pos_dialogue_buttons: int = 2
    """
    ダイアログボタンの表示位置。
    - `1`: 画面上部
    - `2`: 画面中央（デフォルト）
    - `3`: 画面下部
    """
```

### 7.2 メソッド一覧

```python
class CommandBase:
    # 以下は run() 内で使用可能なメソッド

    # === 出力 ===

    def print_t1(self, message: str) -> None:
        """上部パネルにメッセージを出力します。

        Args:
            message: 出力するメッセージ。
        """
        ...

    def print_t2(self, message: str) -> None:
        """下部パネルにメッセージを出力します。

        Args:
            message: 出力するメッセージ。
        """
        ...

    def print_log(self, level: str, message: str, tag: str = "") -> None:
        """指定されたログレベルでメッセージを出力します。

        Args:
            level: ログレベル（'DEBUG', 'INFO', 'WARN', 'ERROR'）。
            message: 出力するメッセージ。
            tag: ログタグ（省略可能）。
        """
        ...

    # === シリアル通信 ===

    def serial_send(self, command: str) -> None:
        """シリアルコマンドを送信します。

        Args:
            command: 送信するコマンド文字列（例: '0xABCD 08 80 80 80 80'）。
        """
        ...

    # === カメラ ===

    def camera_capture(self) -> Optional[object]:
        """現在のカメラフレームをキャプチャします。

        Returns:
            キャプチャしたフレームデータ、または None（失敗時）。
        """
        ...

    # === 通知 ===

    def discord_text(
        self,
        content: str,
        index: int = 0,
    ) -> None:
        """Discord にテキスト通知を送信します。

        Args:
            content: 通知メッセージ。
            index: カメラインデックス（デフォルト: 0）。
        """
        ...

    def discord_image(
        self,
        content: str,
        index: int = 0,
        crop_fmt: str = "",
        crop: Optional[str] = None,
    ) -> None:
        """Discord に画像付き通知を送信します。

        Args:
            content: 通知メッセージ。
            index: カメラインデックス（デフォルト: 0）。
            crop_fmt: クロップフォーマット。
            crop: クロップ領域の指定。
        """
        ...

    # === ウィジェット ===

    def dialogue6widget(self, mode: str) -> None:
        """指定されたウィジェットモードのダイアログを表示します。

        Args:
            mode: ウィジェットモード。
                  'Entry' | 'Check' | 'Combo' | 'Radio' | 'Spin' | 'Scale' | 'Next'
        """
        ...

    # === ライフサイクル ===

    def run(self) -> None:
        """コマンドのエントリポイント。サブクラスでオーバーライドします。"""
        ...
```

### 7.3 カスタムエラー

```python
# python/poke_controller/commands/base.py

class CommandError(Exception):
    """コマンド実行に関するエラー。"""

    def __init__(self, message: str, exit_code: int = 1) -> None:
        self.exit_code = exit_code
        super().__init__(message)
```

### 7.4 PyO3 バインディングから公開される Rust API

```python
# python/poke_controller/_core (Rust モジュール)

# 以下の関数は Rust から PyO3 経由で Python に公開されます

def serial_send(command: str) -> None:
    """シリアルコマンドを送信する（Rust 実装）。"""
    ...

def camera_capture() -> object:
    """カメラフレームをキャプチャする（Rust 実装）。"""
    ...

def log_info(message: str) -> None:
    """INFO ログを出力する（Rust 実装）。"""
    ...

def log_error(message: str) -> None:
    """ERROR ログを出力する（Rust 実装）。"""
    ...
```

### 7.5 コマンド実装例

```python
# python/poke_controller/commands/my_command.py

from poke_controller.commands.base import CommandBase


class MyCommand(CommandBase):
    """サンプル自動化コマンド。"""

    name = "my_command"
    description = "サンプルコマンドの説明"
    params = ["--option1", "--option2"]

    isWinNotStart = False
    isWinNotEnd = True
    isDiscordNotStart = False
    isDiscordNotEnd = True

    stdout_destination = "1"
    pos_dialogue_buttons = 2  # 中央表示

    def run(self) -> None:
        self.print_t1("コマンドを開始します")

        # シリアルコマンドを送信
        self.serial_send("0xABCD 08 80 80 80 80")

        # カメラキャプチャ
        frame = self.camera_capture()

        # 画像処理...

        self.print_t1("コマンド完了")

        # Discord 通知
        self.discord_text(content="コマンドが完了しました")
```

---

## 8. エラーハンドリング

### 8.1 Tauri IPC エラーハンドリング

すべての Tauri コマンドは `Result<T, String>` を返します。失敗時はエラーメッセージを含む文字列が返されます。

```typescript
import { serialConnect } from '$lib/services/tauriBridge';

async function handleConnect() {
  try {
    await serialConnect('/dev/ttyACM0', 115200);
    console.log('接続成功');
  } catch (error) {
    // error はエラーメッセージ文字列
    console.error('接続失敗:', error);
  }
}
```

### 8.2 WebSocket エラーハンドリング

```typescript
wsClient.ws.onerror = (event) => {
  console.error('WebSocket エラー:', event);
};

wsClient.ws.onclose = (event) => {
  if (event.code !== 1000) {
    console.warn(
      `WebSocket 切断: code=${event.code}, reason=${event.reason}`,
    );
  }
};
```

| WebSocket クローズコード | 説明 |
|------------------------|-------------|
| `1000` | 正常切断 |
| `1006` | 異常切断（ネットワーク障害など） |
| `1011` | サーバー側エラー |

### 8.3 REST API エラーレスポンス

```json
// 400 Bad Request
{
  "error": "Missing required field: content",
  "code": "INVALID_REQUEST"
}

// 500 Internal Server Error
{
  "error": "DISCORD_WEBHOOK environment variable not set",
  "code": "WEBSOCKET_NOT_FOUND"
}
```

### 8.4 Python エラーハンドリング

```python
from poke_controller.commands.base import CommandBase, CommandError


class MyCommand(CommandBase):
    def run(self) -> None:
        try:
            self.serial_send("0xABCD 08 80 80 80 80")
        except CommandError as e:
            self.print_t1(f"シリアル送信エラー: {e}")
            self.discord_text(content=f"エラー発生: {e}")
            raise  # コマンドを失敗として報告
```

### 8.5 エラーコード一覧

| カテゴリ | エラーコード | 説明 |
|----------|-------------|-------------|
| **シリアル** | `SERIAL_PORT_NOT_FOUND` | ポートが見つかりません |
| | `SERIAL_OPEN_FAILED` | ポートのオープンに失敗しました |
| | `SERIAL_NOT_CONNECTED` | ポートに接続されていません |
| | `SERIAL_SEND_FAILED` | データの送信に失敗しました |
| | `SERIAL_INVALID_BAUD` | 無効なボーレートです |
| | `SERIAL_ALREADY_CONNECTED` | すでに接続済みです |
| **カメラ** | `CAMERA_NOT_FOUND` | カメラデバイスが見つかりません |
| | `CAMERA_OPEN_FAILED` | カメラのオープンに失敗しました |
| | `CAMERA_CAPTURE_FAILED` | フレームのキャプチャに失敗しました |
| | `CAMERA_INVALID_PARAMS` | 無効なカメラパラメータです |
| **コマンド** | `COMMAND_NOT_FOUND` | 指定されたコマンドが見つかりません |
| | `COMMAND_EXECUTION_FAILED` | コマンドの実行に失敗しました |
| | `COMMAND_ALREADY_RUNNING` | コマンドはすでに実行中です |
| **通知** | `NOTIFICATION_INVALID_REQUEST` | リクエストが不正です |
| | `NOTIFICATION_WEBSOCKET_MISSING` | Webhook URL が設定されていません |
| | `NOTIFICATION_SEND_FAILED` | 通知の送信に失敗しました |
| **WebSocket** | `WS_CONNECTION_FAILED` | WebSocket 接続に失敗しました |
| | `WS_INVALID_MESSAGE` | 無効なメッセージ形式です |

---

> **参考リンク**
> - [Tauri 2.x JavaScript API](https://v2.tauri.app/reference/javascript/)
> - [Tauri 2.x コマンド](https://v2.tauri.app/develop/calling-rust/)
> - [Tauri 2.x イベント](https://v2.tauri.app/develop/events/)
> - [SvelteKit 5 ドキュメント](https://kit.svelte.dev/)
> - [PyO3 ユーザーガイド](https://pyo3.rs/)
> - [tokio-tungstenite](https://docs.rs/tokio-tungstenite/)
