# Poke-Con EX UI リファクタリング計画

## 前提条件

| 項目 | 決定 |
|------|------|
| フレームワーク | SvelteKit + Svelte 5（runes mode） |
| スタイリング | Tailwind CSS |
| UIコンポーネント | shadcn-svelte（必要になったら導入） |
| 状態管理 | Svelte 5 runes でスタート |
| 型定義 | Rust側utoipa → OpenAPIスキーマ → TypeScript型自動生成 |
| 型生成出力 | `Docs/api/` 配下に自動生成 |
| Tauri統合 | `src-tauri/` を修正・維持 |
| 既存JSコード | **完全削除済み**（参照しない） |
| Tkinter機能 | 準拠（7タブ構成、10ショートカット、Pause/Restart等） |
| Line通知UI | **削除**（script APIのみ維持） |
| nix統合 | すべてのツールをnix flake経由で実行 |
| CI整備 | フェーズ2と並列実行 |

## 通信方式

| 通信内容 | プライマリ | フォールバック |
|---------|-----------|---------------|
| カメラ映像 | WebRTC video track | MJPEG over HTTP |
| コントローラー入力 | WebRTC DataChannel | WebSocket |
| ログ | WebRTC DataChannel | WebSocket |
| その他API | HTTP REST API | — |

## フェーズ詳細

### フェーズ1: SvelteKit環境構築 + 現行JS完全削除 + nix統合 ✅ 完了

| タスクID | 内容 | 状態 |
|---------|------|------|
| 1.1 | `web/package.json` をSvelteKit用に書き換え | ✅ |
| 1.2 | Reactソースを完全削除 | ✅ |
| 1.3 | SvelteKitプロジェクト構造作成 | ✅ |
| 1.4 | flake.nixにJS/TSツール統合 | 後続フェーズで |
| 1.5 | `nix run .#check` でビルド確認 | 後続フェーズで |

### フェーズ2: APIクライアント移行（TypeScript化 + OpenAPI生成）

**目的**: 型安全なAPIクライアント構築。Rust側から自動生成。

| タスクID | 内容 | 成果物 |
|---------|------|--------|
| 2.1 | `utoipa` クレートを `src-tauri/Cargo.toml` に追加 | `Cargo.toml` 更新 |
| 2.2 | 36エンドポイントすべてに `utoipa::path` マクロ追加 | `main.rs` 更新 |
| 2.3 | OpenAPIスキーマ生成エンドポイント追加（`/api/openapi.json`） | 新規エンドポイント |
| 2.4 | `openapi-typescript` をnix flakeに追加 | `flake.nix` 更新 |
| 2.5 | `nix run .#generate-api-types` でTS型自動生成 | `Docs/api/openapi.ts` |
| 2.6 | APIクライアント実装（`lib/api/client.ts`） | fetchラッパー |
| 2.7 | WebSocketクライアント実装（`lib/api/websocket.ts`） | 自動再接続、イベント型付き |
| 2.8 | WebRTC DataChannelクライアント実装（`lib/api/datachannel.ts`） | RTCPeerConnectionラッパー |

### フェーズ3: TypeScript CI整備

**目的**: フロントエンドの品質保証。フェーズ2と並列実行。

| タスクID | 内容 | 成果物 |
|---------|------|--------|
| 3.1 | `eslint` + `@typescript-eslint` + `eslint-plugin-svelte` 設定 | `eslint.config.js` |
| 3.2 | `svelte-check` 設定 | `package.json` scripts |
| 3.3 | `vitest` 設定 | `vitest.config.ts` |
| 3.4 | `flake.nix` にチェック用app追加 | `nix run .#web-check` |
| 3.5 | GitHub Actionsワークフロー更新（`lint.yml`等） | `.github/workflows/` |

**並列実行タスク**:
- 3.1 ↔ 2.1（独立）
- 3.2 ↔ 2.2〜2.5（独立）
- 3.3 ↔ 2.6〜2.8（独立）
- 3.4, 3.5 → フェーズ2・3完了後

### フェーズ4: コンポーネント再実装（SPA方式、Tkinter準拠）

**目的**: 6タブ + 右側パネルの再実装。

#### 4.1 共通コンポーネント

| タスクID | コンポーネント | 内容 |
|---------|--------------|------|
| 4.1.1 | `NavBar.svelte` | 6タブ切替（Camera/Serial/Manual Control/Commands/Notification/Others） |
| 4.1.2 | `StatusBar.svelte` | WebSocket接続、シリアル/カメラ状態 |
| 4.1.3 | `OutputPanel.svelte` | Output#1/#2、Widgetモード（7種類）対応 |
| 4.1.4 | `SoftwareController.svelte` | SwitchコントローラーGUI（Joy-Con風） |
| 4.1.5 | `LogPanel.svelte` | ログ表示（色分け、スクロール） |

#### 4.2 Cameraページ

| タスクID | コンポーネント | 内容 |
|---------|--------------|------|
| 4.2.1 | `CameraPreview.svelte` | カメラ映像表示、キャンバス操作（スティック/タッチ/範囲SS） |
| 4.2.2 | `CameraSettings.svelte` | デバイス選択、FPS、反転 |
| 4.2.3 | `DisplaySettings.svelte` | リアルタイム表示、類似度表示、ガイド表示、サイズ |

#### 4.3 Serialページ

| タスクID | コンポーネント | 内容 |
|---------|--------------|------|
| 4.3.1 | `SerialConnection.svelte` | ポート選択、ボーレート、データ形式 |
| 4.3.2 | `SerialMonitor.svelte` | シリアルデータ送受信表示 |

#### 4.4 Manual Controlページ

| タスクID | コンポーネント | 内容 |
|---------|--------------|------|
| 4.4.1 | `SoftwareControl.svelte` | キーボード有効、L/Rスティックマウス |
| 4.4.2 | `HardwareControl.svelte` | ゲームパッド種別（Pro/Xinput）、接続、記録 |
| 4.4.3 | `ControllerSimulator.svelte` | Joy-Con風サブウィンドウ（別窓） |

#### 4.5 Commandsページ

| タスクID | コンポーネント | 内容 |
|---------|--------------|------|
| 4.5.1 | `PythonCommandList.svelte` | Pythonコマンド一覧、フィルタ（タグ） |
| 4.5.2 | `McuCommandList.svelte` | MCUコマンド一覧、フィルタ（タグ） |
| 4.5.3 | `ShortcutButtons.svelte` | ショートカットボタン（10個） |
| 4.5.4 | `CommandActions.svelte` | Start/Pause/Restart/Stop/Reload |

#### 4.6 Notificationページ

| タスクID | コンポーネント | 内容 |
|---------|--------------|------|
| 4.6.1 | `WindowsNotification.svelte` | Windowsトースト通知設定 |
| 4.6.2 | `DiscordNotification.svelte` | Webhook URL、テスト送信 |

#### 4.7 Othersページ

| タスクID | コンポーネント | 内容 |
|---------|--------------|------|
| 4.7.1 | `OutputSizeAdjuster.svelte` | Output#1/#2の比率調整 |
| 4.7.2 | `StdoutDestination.svelte` | 標準出力先切替（Output#1/Output#2） |
| 4.7.3 | `WidgetModeSelector.svelte` | Widgetモード（7種類） |
| 4.7.4 | `SoftwareControllerPosition.svelte` | Software-Controller位置（TOP/BOTTOM） |
| 4.7.5 | `DialogueButtonPosition.svelte` | ダイアログボタン位置（TOP/BOTTOM/BOTH） |

### フェーズ5: カメラ映像配信

| タスクID | 内容 | 備考 |
|---------|------|------|
| 5.1 | MJPEG over HTTPエンドポイント実装（Rust側） | `/camera/stream` |
| 5.2 | MJPEGフロントエンド実装 | `<img>`タグ |
| 5.3 | WebRTCシグナリング（WebSocket流用） | `offer`/`answer`/`ice-candidate` |
| 5.4 | WebRTC video track実装 | RTCPeerConnection |
| 5.5 | WebRTC DataChannel実装 | ログ・コントローラー入力 |
| 5.6 | WebSocketフォールバック実装 | DataChannel接続失敗時 |

### フェーズ6: ビルド・統合

| タスクID | 内容 | 備考 |
|---------|------|------|
| 6.1 | SvelteKit静的ビルド設定 | `adapter-static`、出力先 `dist/` |
| 6.2 | `src-tauri/tauri.conf.json` 更新 | `frontendDist: "../web/dist"` |
| 6.3 | URL構成実装 | `/api`、 `/ui`、 `/mobile`、 `/`（リダイレクト） |
| 6.4 | `nix run .#tauri-build` 確認 | — |
| 6.5 | `nix run .#tauri-dev` 確認 | — |
| 6.6 | `nix run .#check` パス確認 | — |
| 6.7 | `Docs/web-ui-guide.md` 更新 | SvelteKit仕様に更新 |

### フェーズ7: 追加機能（優先度：低）

| タスクID | 内容 |
|---------|------|
| 7.1 | キーコンフィグ画面 |
| 7.2 | Pokemon Home連携画面 |
| 7.3 | メニューバー機能（設定/ヘルプ/バージョン確認等） |
| 7.4 | キャプチャ範囲選択（マウスドラッグ） |

### フェーズ8: PWA対応（優先度：低）

| タスクID | 内容 |
|---------|------|
| 8.1 | Web App Manifest |
| 8.2 | Service Worker |
| 8.3 | オフライン対応 |

### フェーズ9: テーマ機能（優先度：低）

| タスクID | 内容 |
|---------|------|
| 9.1 | テーマシステム設計（拡張性考慮） |
| 9.2 | プリセットテーマ（ダーク/ライト等） |
| 9.3 | ユーザー定義テーマ |

## 実装順序（依存関係考慮）

```
フェーズ1 ✅ 完了
  ↓
フェーズ2 + フェーズ3（並列）
  ↓
フェーズ4
  ↓
フェーズ5
  ↓
フェーズ6
  ↓
フェーズ7（優先度：低）
  ↓
フェーズ8（優先度：低）
  ↓
フェーズ9（優先度：低）
```

## 注意事項

- **既存のReactコード（web-old/含む）は参照しない**
- **すべてのツールはnix flake経由で実行**
- **フェーズ2・3は並列実行可能**
- **優先度：低のフェーズも実装必須**（後回しにするだけでスキップしない）

## レビュー方針

各フェーズの完了時、およびサブエージェントへの委任タスク終了時には **opencode レビュー** を実施する。

- **レビュー対象**: サブエージェントの実装成果物、フェーズ区切りのコード変更
- **レビューツール**: `opencode` CLI（`--model litellm/glm-5.1`）
- **レビュー内容**: コード品質、設計の妥当性、型安全性、パフォーマンス、セキュリティ
- **対応方針**: 妥当な指摘には必ず対応し、修正後に再レビューを実施
- **必須タイミング**:
  - サブエージェントからの成果物受け取り後
  - 各フェーズ完了後
  - ユーザーへの成果提示前
  - GitHubへのpush前

レビュー結果と対応内容はコミットメッセージまたはPR説明に記載する。
