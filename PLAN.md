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
| 1.4 | flake.nixにJS/TSツール統合 | ✅（フェーズ3で完了） |
| 1.5 | `nix run .#check` でビルド確認 | 継続的に実施 |

### フェーズ2: APIクライアント移行（TypeScript化 + OpenAPI生成） ✅ 完了

**目的**: 型安全なAPIクライアント構築。Rust側から自動生成。

| タスクID | 内容 | 成果物 | 状態 |
|---------|------|--------|------|
| 2.1 | `utoipa` クレートを `src-tauri/Cargo.toml` に追加 | `Cargo.toml` 更新 | ✅ |
| 2.2 | 36エンドポイントすべてに `utoipa::path` マクロ追加 | `main.rs` 更新 | ✅ |
| 2.3 | OpenAPIスキーマ生成エンドポイント追加（`/api/openapi.json`） | 新規エンドポイント | ✅ |
| 2.4 | `openapi-typescript` をnix flakeに追加 | `flake.nix` 更新 | ✅（フェーズ6で完了） |
| 2.5 | `nix run .#generate-api-types` でTS型自動生成 | `Docs/api/openapi.ts` | ✅（フェーズ6で完了） |
| 2.6 | APIクライアント実装（`lib/api/client.ts`） | fetchラッパー | ✅（フェーズ4で実施） |
| 2.7 | WebSocketクライアント実装（`lib/api/websocket.ts`） | 自動再接続、イベント型付き | ✅（フェーズ5で完了） |
| 2.8 | WebRTC DataChannelクライアント実装（`lib/api/datachannel.ts`） | RTCPeerConnectionラッパー | ✅（フェーズ5で完了） |

**レビュー対応**:
- `api_openapi_json` を `#[openapi(paths)]` に追加（レビュー指摘 #1）
- `api_greet` パラメータ型は実装と一致（レビュー指摘 #2 は誤り）
- レスポンス型の `serde_json::Value` 問題はフェーズ4以降で対応

### フェーズ3: TypeScript CI整備 ✅ 完了

**目的**: フロントエンドの品質保証。フェーズ2と並列実行。

| タスクID | 内容 | 成果物 | 状態 |
|---------|------|--------|------|
| 3.1 | `eslint` + `@typescript-eslint` + `eslint-plugin-svelte` 設定 | `eslint.config.js` | ✅ |
| 3.2 | `svelte-check` 設定 | `package.json` scripts | ✅ |
| 3.3 | `vitest` 設定 | `vitest.config.ts` | ✅ |
| 3.4 | `flake.nix` にチェック用app追加 | `nix run .#web-check` | ✅ |
| 3.5 | GitHub Actionsワークフロー更新（`lint.yml`等） | `.github/workflows/` | ✅（フェーズ6で完了） |

**レビュー対応**:
- `web-check` に `svelte-kit sync` を `lint` の前に追加（レビュー指摘 #5）

### フェーズ4: コンポーネント再実装（SPA方式、Tkinter準拠） ✅ 完了

**目的**: 6タブ + 右側パネルの再実装。

#### 4.1 共通コンポーネント

| タスクID | コンポーネント | 内容 | 状態 |
|---------|--------------|------|------|
| 4.1.1 | `NavBar.svelte` | 6タブ切替（Camera/Serial/Manual Control/Commands/Notification/Others） | ✅ |
| 4.1.2 | `StatusBar.svelte` | WebSocket接続、シリアル/カメラ状態 | ✅ |
| 4.1.3 | `OutputPanel.svelte` | Output#1/#2、Widgetモード（7種類）対応 | ✅ |
| 4.1.4 | `SoftwareController.svelte` | SwitchコントローラーGUI（Joy-Con風） | ✅ |
| 4.1.5 | `LogPanel.svelte` | ログ表示（色分け、スクロール） | ✅ |

#### 4.2 Cameraページ

| タスクID | コンポーネント | 内容 | 状態 |
|---------|--------------|------|------|
| 4.2.1 | `CameraPreview.svelte` | カメラ映像表示、キャンバス操作（スティック/タッチ/範囲SS） | ✅ |
| 4.2.2 | `CameraSettings.svelte` | デバイス選択、FPS、反転 | ✅ |
| 4.2.3 | `DisplaySettings.svelte` | リアルタイム表示、類似度表示、ガイド表示、サイズ | ✅ |

#### 4.3 Serialページ

| タスクID | コンポーネント | 内容 | 状態 |
|---------|--------------|------|------|
| 4.3.1 | `SerialConnection.svelte` | ポート選択、ボーレート、データ形式 | ✅ |
| 4.3.2 | `SerialMonitor.svelte` | シリアルデータ送受信表示 | ✅ |

#### 4.4 Manual Controlページ

| タスクID | コンポーネント | 内容 | 状態 |
|---------|--------------|------|------|
| 4.4.1 | `SoftwareControl.svelte` | キーボード有効、L/Rスティックマウス | ✅ |
| 4.4.2 | `HardwareControl.svelte` | ゲームパッド種別（Pro/Xinput）、接続、記録 | ✅ |
| 4.4.3 | `ControllerSimulator.svelte` | Joy-Con風サブウィンドウ（別窓） | ✅ |

#### 4.5 Commandsページ

| タスクID | コンポーネント | 内容 | 状態 |
|---------|--------------|------|------|
| 4.5.1 | `PythonCommandList.svelte` | Pythonコマンド一覧、フィルタ（タグ） | ✅ |
| 4.5.2 | `McuCommandList.svelte` | MCUコマンド一覧、フィルタ（タグ） | ✅ |
| 4.5.3 | `ShortcutButtons.svelte` | ショートカットボタン（10個） | ✅ |
| 4.5.4 | `CommandActions.svelte` | Start/Pause/Restart/Stop/Reload | ✅ |

#### 4.6 Notificationページ

| タスクID | コンポーネント | 内容 | 状態 |
|---------|--------------|------|------|
| 4.6.1 | `WindowsNotification.svelte` | Windowsトースト通知設定 | ✅ |
| 4.6.2 | `DiscordNotification.svelte` | Webhook URL、テスト送信 | ✅ |

#### 4.7 Othersページ

| タスクID | コンポーネント | 内容 | 状態 |
|---------|--------------|------|------|
| 4.7.1 | `OutputSizeAdjuster.svelte` | Output#1/#2の比率調整 | ✅ |
| 4.7.2 | `StdoutDestination.svelte` | 標準出力先切替（Output#1/Output#2） | ✅ |
| 4.7.3 | `WidgetModeSelector.svelte` | Widgetモード（7種類） | ✅ |
| 4.7.4 | `SoftwareControllerPosition.svelte` | Software-Controller位置（TOP/BOTTOM） | ✅ |
| 4.7.5 | `DialogueButtonPosition.svelte` | ダイアログボタン位置（TOP/BOTTOM/BOTH） | ✅ |

### フェーズ5: カメラ映像配信 ✅ 完了

| タスクID | 内容 | 備考 | 状態 |
|---------|------|------|------|
| 5.1 | MJPEG over HTTPエンドポイント実装（Rust側） | `/camera/stream` | ✅ |
| 5.2 | MJPEGフロントエンド実装 | `<img>`タグ | ✅（フェーズ4で実施） |
| 5.3 | WebRTCシグナリング（WebSocket流用） | `offer`/`answer`/`ice-candidate` | ✅（フェーズ2.8で実施） |
| 5.4 | WebRTC video track実装 | RTCPeerConnection | ⏳ 未実施 |
| 5.5 | WebRTC DataChannel実装 | ログ・コントローラー入力 | ✅（フェーズ2.8で実施） |
| 5.6 | WebSocketフォールバック実装 | DataChannel接続失敗時 | ✅（フェーズ2.8で実施） |

**レビュー対応**:
- StatusBar.svelteでシングルトンwsClientを使用（レビュー指摘 #4）
- websocket.tsでCLOSING状態のソケットをクローズ（レビュー指摘 #3）
- main.rsにCache-Controlヘッダー追加（レビュー指摘 #7）
- 空のmultipartボディをスキップ（レビュー指摘 #5）
- 残存: シグナリングプロトコル型衝突（Critical）、フォールバックハンドラーリーク（High）→ フェーズ6で対応

### フェーズ6: ビルド・統合 ✅ 完了

| タスクID | 内容 | 備考 | 状態 |
|---------|------|------|------|
| 6.1 | SvelteKit静的ビルド設定 | `adapter-static`、出力先 `dist/`、base `/ui` | ✅ |
| 6.2 | `src-tauri/tauri.conf.json` 更新 | `frontendDist: "../web/dist"` | ✅（既存設定で対応） |
| 6.3 | URL構成実装 | `/`→`/ui/`リダイレクト、`/ui/*`配信、`/mobile`501 | ✅ |
| 6.4 | `nix run .#tauri-build` 確認 | — | ⏳ 未実施（WebKit/GTK依存） |
| 6.5 | `nix run .#tauri-dev` 確認 | — | ⏳ 未実施（WebKit/GTK依存） |
| 6.6 | `nix run .#check` パス確認 | — | ⏳ 未実施（libclang依存） |
| 6.7 | `Docs/web-ui-guide.md` 更新 | SvelteKit仕様に更新 | ⏳ 未実施 |

**レビュー対応**:
- `+layout.ts` を作成してSPAモード設定（レビュー指摘 #2）
- `+page.svelte` の未使用 `goto` インポートを削除（レビュー指摘 #1）
- ルートリダイレクトを `permanent` から `temporary` に変更（レビュー指摘 #3）

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
フェーズ2 ✅ 完了 + フェーズ3 ✅ 完了（並列）
  ↓
フェーズ4 ✅ 完了
  ↓
フェーズ5 ✅ 完了（一部⏳残存）
  │  ※ 5.4 WebRTC video track は未実施
  ↓
フェーズ6 ✅ 完了（一部⏳残存）
  │  ※ 6.4 tauri-build、6.5 tauri-dev、6.6 check、6.7 Docs更新 は未実施
  ↓
フェーズ7 ← 現在ここ（優先度：低）
  ↓
フェーズ8（優先度：低）
  ↓
フェーズ9（優先度：低）
```

## 未実施タスク一覧

以下のタスクは未実施のまま残っています。適切なフェーズで対応する必要があります。

| フェーズ | タスクID | 内容 | 備考 |
|---------|---------|------|------|
| フェーズ5 | 5.4 | WebRTC video track実装 | RTCPeerConnection |
| フェーズ6 | 6.4 | `nix run .#tauri-build` 確認 | WebKit/GTKシステム依存 |
| フェーズ6 | 6.5 | `nix run .#tauri-dev` 確認 | WebKit/GTKシステム依存 |
| フェーズ6 | 6.6 | `nix run .#check` パス確認 | libclangシステム依存 |
| フェーズ6 | 6.7 | `Docs/web-ui-guide.md` 更新 | SvelteKit仕様に更新 |
| フェーズ7 | 7.1 | キーコンフィグ画面 | 優先度：低 |
| フェーズ7 | 7.2 | Pokemon Home連携画面 | 優先度：低 |
| フェーズ7 | 7.3 | メニューバー機能（設定/ヘルプ/バージョン確認等） | 優先度：低 |
| フェーズ7 | 7.4 | キャプチャ範囲選択（マウスドラッグ） | 優先度：低 |
| フェーズ8 | 8.1 | Web App Manifest | 優先度：低 |
| フェーズ8 | 8.2 | Service Worker | 優先度：低 |
| フェーズ8 | 8.3 | オフライン対応 | 優先度：低 |
| フェーズ9 | 9.1 | テーマシステム設計（拡張性考慮） | 優先度：低 |
| フェーズ9 | 9.2 | プリセットテーマ（ダーク/ライト等） | 優先度：低 |
| フェーズ9 | 9.3 | ユーザー定義テーマ | 優先度：低 |

**対応方針**:
- 5.4 → フェーズ7以降で実施（WebRTC video trackは優先度低）
- 6.4, 6.5, 6.6 → システム依存ライブラリの問題解決後に実施
- 6.7 → フェーズ7完了後に実施
- フェーズ7〜9 → 優先度順に実施

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

## コミット・push方針

- **フェーズの区切りごとに必ずコミットを作成する**
- **各フェーズ完了後、必ずGitHubへpushする**
- コミットメッセージにはフェーズ番号と主要な変更内容を記載する（例: `feat(web): phase 2 - add utoipa OpenAPI generation`）
- 中間的な作業途中状態はコミットせず、フェーズ内のタスクが完了してからコミットする
- push前には必ず `nix run .#check` を実行し、CIが通ることを確認する
